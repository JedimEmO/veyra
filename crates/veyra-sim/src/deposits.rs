//! The adapter half of the `deposits` rules: its requests, systems and plugin.

pub use veyra_rules::deposits::*;

pub mod hand;

pub use hand::*;

use std::sync::Arc;

use bevy::app::{App, Plugin, Update};

use bevy::ecs::prelude::*;

use bevy::ecs::query::QueryData;

use bevy::state::condition::in_state;

use bevy::tasks::{AsyncComputeTaskPool, Task, TaskPool, block_on, poll_once};

use strata_registry::Namespaced;

pub use veyra_world::deposits::{Deposit, DepositField, DepositId};

use crate::combat::{Action, CombatSet, Combatant};

use crate::deeds::Ledger;

pub use crate::household::Purse;

use crate::items::{Hands, HouseholdStore, OpenStock, Pack, Stock, StockAnswer};

use crate::order::RequestSet;

use crate::progression::CharacterSheet;

use crate::ride::{Calling, Riding};

use crate::{
    AppRequestExt, Inbox, Observers, Registries, Request, SimSet, SimTick, WorldPosition,
    WorldSourceRes, water_here,
};

/// What a screen asks of the survey.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SurveyRequest {
    /// Ping the ground for deposits of a class.
    Ping {
        /// The class, by id.
        class: Namespaced,
    },
    /// Sample where the player stands.
    Sample,
    /// End a ping or a sampling under way.
    Stop,
    /// Track a deposit on the HUD, or none.
    Track(Option<DepositId>),
    /// Track a deposit, or let it go when it is the one tracked. Which of
    /// the two is decided when the request is consumed, against what is
    /// tracked then, so two presses before a screen has caught up are a
    /// track and a release and not two tracks.
    ToggleTrack(DepositId),
}

/// The requests not yet consumed, in the order they were made. A resource
/// and not a message, for the reason the stock's inbox is one.
pub type SurveyInbox = Inbox<SurveyRequest>;

/// A ping or a sample with nobody to make it is refused as such; what is
/// tracked and a stop need no one.
impl Request for SurveyRequest {
    type Answer = PingAnswer;

    fn no_actor(&self, commands: &mut Commands) {
        match self {
            Self::Ping { class } => {
                commands.write_message(PingAnswer {
                    class: class.clone(),
                    outcome: Err(SurveyRefusal::NoPlayer),
                });
            }
            Self::Sample => {
                commands.write_message(SampleAnswer {
                    outcome: Err(SurveyRefusal::NoPlayer),
                });
            }
            Self::Stop | Self::Track(_) | Self::ToggleTrack(_) => {}
        }
    }

    fn needs_actor(&self) -> bool {
        matches!(self, Self::Ping { .. } | Self::Sample)
    }
}

/// The list being made.
#[derive(Default)]
pub struct Refreshing(Option<Task<(Vec<[f64; 2]>, Vec<Deposit>)>>);

/// Makes [`NearDeposits`] again, on the task pool, whenever an observer has
/// moved [`REFRESH_M`](crate::sites::REFRESH_M) from every place it was
/// last made round (or the observers came or went), and publishes the list
/// when it is done. Without a [`Survey`] there is nothing to list.
pub fn refresh_near_deposits(
    survey: Option<Res<Survey>>,
    observers: Res<Observers>,
    mut near: ResMut<NearDeposits>,
    mut refreshing: Local<Refreshing>,
) {
    if let Some(task) = refreshing.0.as_mut() {
        let Some((around, deposits)) = block_on(poll_once(task)) else {
            return;
        };
        refreshing.0 = None;
        near.around = around;
        near.deposits = deposits;
    }
    let Some(survey) = survey else {
        return;
    };
    let here: Vec<[f64; 2]> = observers.positions.iter().map(|p| [p.x(), p.z()]).collect();
    let refresh = crate::sites::REFRESH_M;
    let stale = here.len() != near.around.len()
        || here.iter().any(|&[x, z]| {
            near.around.iter().all(|&[ax, az]| {
                let (dx, dz) = (x - ax, z - az);
                dx * dx + dz * dz > refresh * refresh
            })
        });
    if !stale {
        return;
    }
    if here.is_empty() {
        if !near.around.is_empty() || !near.deposits.is_empty() {
            near.around.clear();
            near.deposits.clear();
        }
        return;
    }
    let field = Arc::clone(&survey.0);
    refreshing.0 = Some(
        AsyncComputeTaskPool::get_or_init(TaskPool::new).spawn(async move {
            let range = field.set().rules().register.range_m;
            let deposits = near_deposits(&field, &here, range);
            (here, deposits)
        }),
    );
}

/// What the player carries and is, as the survey's requests read it.
type PlayerRow = (
    Entity,
    &'static mut Pack,
    &'static Hands,
    &'static WorldPosition,
    Option<&'static Combatant>,
    Option<&'static CharacterSheet>,
    Has<Riding>,
    Has<Calling>,
    Has<Pinging>,
    Has<Sampling>,
);

/// The state of the hand [`can_ping`] and [`can_sample`] ask,
/// worked out of the player's components.
pub fn hand_state(
    at: &WorldPosition,
    combatant: Option<&Combatant>,
    flags: [bool; 4],
    source: Option<&WorldSourceRes>,
    swim_m: f32,
) -> HandState {
    let [mounted, calling, pinging, sampling] = flags;
    let water_m = source.map_or(0.0, |source| {
        water_here(source.source(), at.0.x(), at.0.z()).depth_m()
    });
    HandState {
        present: true,
        mounted,
        swimming: water_m > swim_m,
        busy: combatant.is_some_and(|c| c.action != Action::Ready || c.health <= 0.0)
            || calling
            || pinging
            || sampling,
    }
}

/// Resync an open stores screen with the truth after the pack changed, as
/// a fit does: the mirror was not told of a lot it did not click.
pub(crate) fn resync(
    open: &OpenStock,
    store: &HouseholdStore,
    pack: &Pack,
    hands: &Hands,
    mirrors: &mut MessageWriter<StockAnswer>,
) {
    let Some(opened) = &open.0 else {
        return;
    };
    // The snapshot only reads; the copies keep the components' change
    // detection out of it.
    let mut truth = (pack.clone(), hands.clone(), store.clone());
    let stock = Stock {
        saddle: None,
        pack: &mut truth.0,
        hands: &mut truth.1,
        store: opened.with_store.then_some(&mut truth.2),
    };
    mirrors.write(StockAnswer::Resync {
        menu: opened.menu,
        snapshot: stock.snapshot(&opened.state, &opened.saddle),
    });
}

/// `SimTick`, `SimSet::Input`, after the fitting's requests: every ping,
/// sample, stop and change of what is tracked asked for since the last
/// tick, in order.
///
/// A ping and a sample start a channel on the player ([`Pinging`],
/// [`Sampling`]), which [`advance_ping`] and [`advance_sample`] run; the
/// rule is asked here, at the request, against the state then. A deposit to
/// track is looked for in [`NearDeposits`] and then, with a [`Survey`],
/// asked of the world by its id.
#[allow(
    clippy::too_many_arguments,
    clippy::too_many_lines,
    clippy::cast_possible_truncation,
    reason = "the requests, the world, the deposits and the player in, three answers out; a dial's seconds as a clock's f32"
)]
pub fn consume(
    mut commands: Commands,
    mut inbox: ResMut<SurveyInbox>,
    registries: Res<Registries>,
    near: Res<NearDeposits>,
    survey: Option<Res<Survey>>,
    source: Option<Res<WorldSourceRes>>,
    mut surveyed: ResMut<Surveyed>,
    mut players: Query<PlayerRow>,
    mut pings: MessageWriter<PingAnswer>,
    mut samples: MessageWriter<SampleAnswer>,
    mut notices: MessageWriter<SurveyNotice>,
    mut ledger: Ledger,
) {
    let requests = inbox.take();
    if requests.is_empty() {
        return;
    }
    let find = |id: DepositId| {
        near.get(id)
            .cloned()
            .or_else(|| survey.as_ref().and_then(|survey| survey.0.get(id)))
    };
    let registries = &registries.0;
    let rules = registries.resources.survey();
    let state_of = |row: &<PlayerRow as QueryData>::Item<'_, '_>| {
        hand_state(
            row.3,
            row.4,
            [row.6, row.7, row.8, row.9],
            source.as_deref(),
            rules.ping.swim_m,
        )
    };
    // What the requests already granted this tick, which the components
    // will not show until the commands apply.
    let (mut pinging, mut sampling) = (false, false);
    for (actor, request) in requests {
        let player = players.get_mut(actor.0).ok();
        match request {
            SurveyRequest::Track(_) | SurveyRequest::ToggleTrack(_) => {
                let held = surveyed.tracked.as_ref().map(|deposit| deposit.id);
                let wanted = match request {
                    SurveyRequest::ToggleTrack(id) => (held != Some(id)).then_some(id),
                    SurveyRequest::Track(wanted) => wanted,
                    _ => continue,
                };
                let next = wanted.and_then(&find);
                // An id the world has no deposit for leaves what is tracked
                // alone; none clears it.
                if (wanted.is_none() || next.is_some()) && surveyed.tracked != next {
                    surveyed.tracked = next;
                }
            }
            SurveyRequest::Ping { class } => {
                let outcome = match player.as_ref() {
                    None => Err(SurveyRefusal::NoPlayer),
                    Some(row) => {
                        let mut state = state_of(row);
                        state.busy |= pinging || sampling;
                        can_ping(
                            registries,
                            state,
                            &row.1,
                            row.5.unwrap_or(&CharacterSheet::default()),
                            &class,
                        )
                        .map(|skill| {
                            pinging = true;
                            commands.entity(row.0).insert(Pinging {
                                class: class.clone(),
                                left: skill.channel_s,
                            });
                            PingDone::Started
                        })
                    }
                };
                pings.write(PingAnswer { class, outcome });
            }
            SurveyRequest::Sample => {
                let outcome = match player.as_ref() {
                    None => Err(SurveyRefusal::NoPlayer),
                    Some(row) => {
                        let mut state = state_of(row);
                        state.busy |= pinging || sampling;
                        let at = [row.3.0.x(), row.3.0.z()];
                        let covering = survey
                            .as_deref()
                            .map(|survey| concentration_here(survey, at[0], at[1]))
                            .unwrap_or_default();
                        can_sample(
                            registries,
                            state,
                            &row.1,
                            row.5.unwrap_or(&CharacterSheet::default()),
                            &covering,
                            &surveyed,
                        )
                        .map(|(deposit, pm)| {
                            sampling = true;
                            // Working a claim without leave is warned at the
                            // start, not a cycle in.
                            ledger.trespass(registries, &deposit);
                            let cycle_s = skill_of(
                                registries,
                                &row.1,
                                row.5.unwrap_or(&CharacterSheet::default()),
                            )
                            .map_or(rules.sample.cycle_s as f32, |skill| skill.cycle_s);
                            commands.entity(row.0).insert(Sampling {
                                deposit: deposit.id,
                                from: at,
                                left: cycle_s,
                                got: 0,
                            });
                            SampleDone::Started {
                                deposit: deposit.id,
                                pm,
                            }
                        })
                    }
                };
                samples.write(SampleAnswer { outcome });
            }
            SurveyRequest::Stop => {
                let Some(row) = player.as_ref() else {
                    continue;
                };
                if row.9 || sampling {
                    commands.entity(row.0).remove::<Sampling>();
                    notices.write(SurveyNotice::SampleEnded(SampleEnd::Stopped));
                    sampling = false;
                }
                if row.8 || pinging {
                    commands.entity(row.0).remove::<Pinging>();
                    notices.write(SurveyNotice::PingStopped);
                    pinging = false;
                }
            }
        }
    }
}

/// Registers the survey's resources and requests and consumes them in the
/// tick. Added by `SimPlugin`, so a server and a harness have it too; with
/// no survey and no request, which is all either ever has, it touches
/// nothing.
#[derive(Debug, Clone, Copy)]
pub struct DepositsPlugin;

impl Plugin for DepositsPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<NearDeposits>()
            .init_resource::<Purse>()
            .init_resource::<Surveyed>()
            .init_resource::<LastPing>()
            .add_message::<SampleAnswer>()
            .add_message::<SurveyNotice>()
            // The list the survey reads is the sim's, so the plugin that owns
            // `NearDeposits` refreshes it, not the world loader.
            .add_systems(
                Update,
                refresh_near_deposits.run_if(in_state(crate::AppState::InWorld)),
            )
            .add_request::<SurveyRequest, _>(RequestSet::Survey, consume)
            .add_systems(
                SimTick,
                (advance_ping, advance_sample)
                    .in_set(SimSet::Observe)
                    .after(CombatSet::Finish),
            );
    }
}

#[cfg(test)]
mod tests {
    #![allow(
        clippy::unwrap_used,
        reason = "a test that unwraps a None fails, which is the point"
    )]

    use super::*;
    use crate::LocalPlayer;
    use crate::tests::app_in_world;
    use strata_registry::item::SavedPack;

    use std::collections::BTreeMap;

    use bevy::app::App;

    use strata_registry::item::Record;

    use strata_registry::Reading;

    use crate::items::Hands;

    use crate::items::Pack;

    use crate::WorldPosition;

    pub(crate) use veyra_rules::deposits::test_support::*;

    /// The local player with an empty pack and one satchel of six cells,
    /// and `deposits` published as near.
    fn at_large(deposits: Vec<Deposit>) -> (App, Entity) {
        let mut app = app_in_world();
        let registries = shipped();
        let items = &registries.0.items;
        let saved = SavedPack {
            bag_slots: vec![Some(Record {
                item: id("veyra:bag.field_satchel"),
                count: 1,
                batch: 0,
                by: None,
                lines: BTreeMap::new(),
                condition: None,
                fitted: BTreeMap::new(),
                origin: None,
            })],
            ..SavedPack::default()
        };
        let pack = Pack::from_saved(&saved, items).unwrap();
        let hands = Hands::default();
        let player = app
            .world_mut()
            .spawn((LocalPlayer, pack, hands, WorldPosition::new(0., 0., 0.)))
            .id();
        app.insert_resource(registries)
            .insert_resource(NearDeposits {
                deposits,
                around: vec![[0.0, 0.0]],
            });
        (app, player)
    }

    fn ask(app: &mut App, request: SurveyRequest) {
        app.world_mut().resource_mut::<SurveyInbox>().push(request);
        app.update();
    }

    /// Nothing found yet lists nothing; a tracked deposit is kept whole,
    /// tracking none clears it, and a toggle tracks and lets go.
    #[test]
    fn tracking_is_kept_whole_and_toggles() {
        let registries = shipped();
        let set = &registries.0.resources;
        let calder = deposit(set, SALT, [3, -2], [0.0, 0.0]);
        let harrow = deposit(set, ALLOY, [4, -2], [900.0, 0.0]);
        let (mut app, _) = at_large(vec![calder.clone(), harrow.clone()]);
        app.update();
        assert_eq!(*app.world().resource::<Surveyed>(), Surveyed::default());
        let near = app.world().resource::<NearDeposits>().clone();
        let none = register(&near, set, app.world().resource(), [0.0, 0.0], None);
        assert!(
            none.rows.is_empty() && none.in_range == 0,
            "nothing is found"
        );

        ask(&mut app, SurveyRequest::Track(Some(harrow.id)));
        assert_eq!(
            app.world().resource::<Surveyed>().tracked.as_ref(),
            Some(&harrow)
        );
        // The tracked deposit has its row though nothing was found.
        let near = app.world().resource::<NearDeposits>().clone();
        let one = register(&near, set, app.world().resource(), [0.0, 0.0], None);
        assert_eq!(one.rows.len(), 1);
        assert!(one.rows[0].tracked && !one.rows[0].sampled);
        assert_eq!(one.rows[0].reading, Reading::Type);
        // An id the world has no deposit for leaves it alone.
        ask(&mut app, SurveyRequest::Track(Some(DepositId([99, 99]))));
        assert_eq!(
            app.world().resource::<Surveyed>().tracked.as_ref(),
            Some(&harrow)
        );
        ask(&mut app, SurveyRequest::Track(None));
        assert_eq!(app.world().resource::<Surveyed>().tracked, None);

        // A toggle tracks, and a second lets go. Two in one tick are a
        // track and a release, each decided on what the one before left;
        // a toggle onto another deposit moves the tracking there.
        let tracked = |app: &App| {
            let surveyed = app.world().resource::<Surveyed>();
            surveyed.tracked.as_ref().map(|deposit| deposit.id)
        };
        ask(&mut app, SurveyRequest::ToggleTrack(harrow.id));
        assert_eq!(tracked(&app), Some(harrow.id));
        ask(&mut app, SurveyRequest::ToggleTrack(calder.id));
        assert_eq!(tracked(&app), Some(calder.id));
        ask(&mut app, SurveyRequest::ToggleTrack(calder.id));
        assert_eq!(tracked(&app), None);
        app.world_mut().resource_mut::<SurveyInbox>().extend(vec![
            SurveyRequest::ToggleTrack(
                harrow.id
            );
            2
        ]);
        app.update();
        assert_eq!(tracked(&app), None, "tracked, then let go");
        ask(&mut app, SurveyRequest::ToggleTrack(DepositId([99, 99])));
        assert_eq!(tracked(&app), None, "no such deposit");
    }
}
