//! The adapter half of the `fitting` rules: its requests, systems and plugin.

pub use veyra_rules::fitting::*;

use bevy::app::{App, Plugin};

use bevy::ecs::prelude::*;

use strata_registry::item::Fitted;

use crate::combat::{Action, Combatant};

pub use crate::combat_loadout::{Factor, Loadout, Row, live_on, merge};

use crate::items::{Hands, HouseholdStore, OpenStock, Pack, Stock, StockAnswer};

use crate::order::RequestSet;

use crate::progression::Reach;

use crate::{AppRequestExt, Inbox, Registries, Request, SimTick};

/// What a screen asks of a weapon's mounts.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FitRequest {
    /// Fit a module to a mount.
    Fit {
        /// The hand the weapon is in: the carbine's, then the pistol's.
        hand: usize,
        /// The mount.
        mount: String,
        /// The module, named by what it is and not by where it lies, so
        /// the stock may have moved under the screen since it was listed.
        module: Fitted,
    },
    /// Take the module off a mount.
    Remove {
        /// The hand the weapon is in.
        hand: usize,
        /// The mount.
        mount: String,
    },
}

/// The requests not yet consumed, in the order they were made. A resource
/// and not a message, for the reason the stock's inbox is one: a screen
/// writes it in `Update`, the tick may not run that frame or may run
/// twice, and a request is consumed exactly once.
pub type FitInbox = Inbox<FitRequest>;

impl FitRequest {
    /// The hand and mount a request names, for its answer.
    fn target(&self) -> (usize, String) {
        match self {
            Self::Fit { hand, mount, .. } | Self::Remove { hand, mount } => (*hand, mount.clone()),
        }
    }
}

/// With nobody to work on, a request is refused as such.
impl Request for FitRequest {
    type Answer = FitAnswer;

    fn no_actor(&self, commands: &mut Commands) {
        let (hand, mount) = self.target();
        commands.write_message(FitAnswer {
            hand,
            mount,
            outcome: Err(FitRefusal::NoPlayer),
        });
    }
}

/// What a request did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FitDone {
    /// The module is on the mount.
    Fitted {
        /// What the weapon's magazine now holds when full.
        magazine: u16,
    },
    /// The module is off the mount and put away.
    Removed,
}

/// The sim's answer to a [`FitRequest`].
#[derive(Message, Debug, Clone, PartialEq, Eq)]
pub struct FitAnswer {
    /// The hand the request named.
    pub hand: usize,
    /// The mount it named.
    pub mount: String,
    /// What happened, or why nothing did.
    pub outcome: Result<FitDone, FitRefusal>,
}

/// `SimTick`, `SimSet::Input`, after the stock's own requests: every fit
/// and removal asked for since the last tick, in order, on the local
/// player's weapons.
///
/// The workbench rule is asked at every request and not when the screen
/// opened, since the world runs under an open screen. A holder in the
/// middle of an action is refused: a magazine is not changed under a
/// reload. With a workbench in reach the household's store is to hand, so
/// a part comes from it and a part that comes off may go to it. The stores
/// screen, if it is open, is answered with the truth, since its mirror was
/// not told of a change it did not click.
#[allow(
    clippy::too_many_arguments,
    reason = "the requests, the rule, the stock and its open menu in, two answers out"
)]
pub fn consume(
    mut inbox: ResMut<FitInbox>,
    access: Res<WorkbenchAccess>,
    reach: Res<Reach>,
    registries: Res<Registries>,
    open: Res<OpenStock>,
    mut store: ResMut<HouseholdStore>,
    mut players: Query<(&mut Pack, &mut Hands, Option<&Combatant>)>,
    mut answers: MessageWriter<FitAnswer>,
    mut mirrors: MessageWriter<StockAnswer>,
) {
    let requests = inbox.take();
    if requests.is_empty() {
        return;
    }
    let items = &registries.0.items;
    let at_workbench = at_workbench(*access, &reach);
    let mut changed = false;
    let mut last = None;
    for (actor, request) in requests {
        let (hand, mount) = request.target();
        let mut player = players.get_mut(actor.0).ok();
        if player.is_some() {
            last = Some(actor.0);
        }
        let outcome = match player.as_mut() {
            None => Err(FitRefusal::NoPlayer),
            Some(_) if !at_workbench => Err(FitRefusal::NoWorkbench),
            Some((_, _, Some(actor))) if actor.action != Action::Ready => Err(FitRefusal::Busy),
            Some((pack, hands, actor)) => {
                // Worked on copies and written back only when something
                // moved, so a refusal leaves no change for a reader to
                // chase.
                let mut next = ((**pack).clone(), (**hands).clone(), store.clone());
                let mut stock = Stock {
                    saddle: None,
                    pack: &mut next.0,
                    hands: &mut next.1,
                    store: Some(&mut next.2),
                };
                let done = match &request {
                    FitRequest::Fit { module, .. } => fit(items, &mut stock, hand, &mount, module)
                        .map(|()| FitDone::Fitted {
                            magazine: magazine(items, &next.1, hand, *actor),
                        }),
                    FitRequest::Remove { .. } => {
                        remove(items, &mut stock, hand, &mount).map(|_| FitDone::Removed)
                    }
                };
                if done.is_ok() {
                    changed = true;
                    if **pack != next.0 {
                        **pack = next.0;
                    }
                    **hands = next.1;
                    if *store != next.2 {
                        *store = next.2;
                    }
                }
                done
            }
        };
        answers.write(FitAnswer {
            hand,
            mount,
            outcome,
        });
    }
    if changed
        && let Some(opened) = &open.0
        && let Some(actor) = last
        && let Ok((pack, hands, _)) = players.get_mut(actor)
    {
        // The snapshot only reads; the copies keep the components' change
        // detection out of it.
        let mut truth = ((*pack).clone(), (*hands).clone(), store.clone());
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
}

/// `SimTick`, `SimSet::Input`, after [`consume`]: an actor whose hands
/// changed is equipped with what they now carry, when that differs from
/// what it is equipped with. A fit, a removal, a swap of who is out and
/// the spawn all arrive here as a change of [`Hands`]; an opponent and a
/// harness side have no hands and are never touched.
pub fn sync_loadout(
    registries: Res<Registries>,
    mut actors: Query<(&Hands, &mut Combatant), Changed<Hands>>,
) {
    let items = &registries.0.items;
    for (hands, mut actor) in &mut actors {
        let Ok(saved) = hands.saved(items) else {
            continue;
        };
        actor.carry(
            saved
                .each_ref()
                .map(|hand| hand.as_ref().and_then(|record| weapon_of(items, record))),
        );
        let next = loadout(items, &saved);
        let differs = actor
            .loadout()
            .map_or_else(|| !next.is_empty(), |equipped| *equipped != next);
        if differs {
            actor.equip(&next);
        }
    }
}

/// Registers the fitting requests and consumes them in the tick. Added by
/// `SimPlugin`, so a server and a harness have it too; with no request and
/// no hands, which is all either ever has, it touches nothing.
#[derive(Debug, Clone, Copy)]
pub struct FittingPlugin;

impl Plugin for FittingPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<WorkbenchAccess>()
            .add_request::<FitRequest, _>(RequestSet::Fitting, consume)
            .add_systems(SimTick, sync_loadout.in_set(RequestSet::Loadout));
    }
}

#[cfg(test)]
mod tests {
    #![allow(
        clippy::unwrap_used,
        reason = "a test that unwraps a None fails, which is the point"
    )]
    #![allow(
        clippy::float_cmp,
        reason = "a merged row is the product of two table values exactly; a bit off is a wrong merge"
    )]

    use super::*;
    use crate::LocalPlayer;

    use crate::items::StockInbox;
    use crate::items::StockRequest;
    use crate::progression::CharacterSheet;
    use crate::progression::Interaction;
    use crate::progression::InteractionKind;
    use crate::tests::app_in_world;
    use bevy::ecs::message::Messages;
    use slotted_model::MenuId;

    use strata_registry::item::MODDABILITY;
    use strata_registry::progression::SkillGraph;

    use bevy::app::App;

    use strata_registry::item::Fitted;
    use strata_registry::item::HANDS;
    use strata_registry::item::ItemCatalogue;

    use strata_registry::item::Property;
    use strata_registry::item::Record;

    use crate::combat::Action;
    use crate::combat::Combatant;
    use crate::combat::Weapon;
    use crate::items;
    use crate::items::Hands;
    use crate::items::HouseholdStore;

    use crate::items::Pack;

    use crate::items::StockAnswer;

    use crate::progression::Reach;

    use crate::Registries;

    pub(crate) use veyra_rules::fitting::test_support::*;

    /// The first day in a running sim, as the stock's own tests build it,
    /// with an actor to equip: the arrival with the pack and the weapons
    /// of `starting.ron`, the household's store, and nothing within reach.
    fn first_day() -> App {
        let mut app = app_in_world();
        let registries = shipped();
        let items = &registries.0.items;
        let starting = items.starting().unwrap();
        let arrival = &registries.0.sheets.household.as_ref().unwrap().arrival;
        let pack = Pack::from_saved(&starting.packs[&arrival.name], items).unwrap();
        let hands = Hands::from_saved(&starting.hands.clone().map(Some), items).unwrap();
        let store: Vec<Option<Record>> = starting.store.iter().cloned().map(Some).collect();
        let store = HouseholdStore::from_saved(&store, items).unwrap();
        app.world_mut().spawn((
            LocalPlayer,
            crate::WorldPosition::new(0., 0., 0.),
            pack,
            hands,
            Combatant::default(),
        ));
        app.insert_resource(store).insert_resource(registries);
        app.update();
        app
    }

    /// A prop the player stands at; a tick passes before it is in reach.
    fn prop(app: &mut App, kind: InteractionKind) -> Entity {
        let prop = app
            .world_mut()
            .spawn((
                crate::WorldPosition::new(0., 0., 0.),
                Interaction { kind, radius: 2.5 },
            ))
            .id();
        app.update();
        prop
    }

    fn ask(app: &mut App, request: FitRequest) -> Result<FitDone, FitRefusal> {
        app.world_mut().resource_mut::<FitInbox>().push(request);
        app.update();
        let mut answers: Vec<FitAnswer> = app
            .world_mut()
            .resource_mut::<Messages<FitAnswer>>()
            .drain()
            .collect();
        assert_eq!(answers.len(), 1, "one request, one answer");
        answers.pop().unwrap().outcome
    }

    fn fit_request(hand: usize, module: Fitted) -> FitRequest {
        FitRequest::Fit {
            hand,
            mount: "feed".to_owned(),
            module,
        }
    }

    fn remove_request(hand: usize) -> FitRequest {
        FitRequest::Remove {
            hand,
            mount: "feed".to_owned(),
        }
    }

    fn player(app: &mut App) -> (Pack, Hands, Combatant) {
        let world = app.world_mut();
        let mut players = world.query_filtered::<(&Pack, &Hands, &Combatant), With<LocalPlayer>>();
        let (pack, hands, actor) = players.single(world).unwrap();
        (pack.clone(), hands.clone(), actor.clone())
    }

    fn actor_mut(app: &mut App) -> Mut<'_, Combatant> {
        let world = app.world_mut();
        let mut players = world.query_filtered::<&mut Combatant, With<LocalPlayer>>();
        players.single_mut(world).unwrap()
    }

    #[test]
    fn fitting_keeps_the_rounds_and_removing_clamps_them() {
        let mut app = first_day();
        let registries = app.world().resource::<Registries>().clone();
        let items = &registries.0.items;
        prop(&mut app, InteractionKind::Workbench);
        let unfitted = player(&mut app).2;
        assert!(only_the_action(unfitted.loadout()));

        // Three rounds fired, then the magazine fitted: the 21 are kept.
        actor_mut(&mut app).ammo = 21;
        let done = ask(&mut app, fit_request(0, bought(items, FEED)));
        assert_eq!(done, Ok(FitDone::Fitted { magazine: 29 }));
        let (_, hands, actor) = player(&mut app);
        assert_eq!((actor.magazine(), actor.ammo), (29, 21));
        assert!((actor.reload_time() - 1.485).abs() < 1e-4);
        let carbine = held(items, &hands, 0).unwrap();
        assert_eq!(carbine.fitted.get("feed"), Some(&bought(items, FEED)));

        // Reloaded to 29 and two fired; the removal leaves what 24 holds.
        actor_mut(&mut app).ammo = 27;
        assert_eq!(ask(&mut app, remove_request(0)), Ok(FitDone::Removed));
        let (_, hands, actor) = player(&mut app);
        assert_eq!((actor.magazine(), actor.ammo), (24, 24));
        assert!(only_the_action(actor.loadout()));
        assert_eq!(actor.weapon_spec(), unfitted.weapon_spec());
        assert_eq!(held(items, &hands, 0).unwrap(), weapons(items)[0]);

        // The holstered pistol's magazine is clamped the same way.
        let done = ask(&mut app, fit_request(1, bought(items, CELL)));
        assert_eq!(done, Ok(FitDone::Fitted { magazine: 13 }));
        let actor = player(&mut app).2;
        assert_eq!((actor.magazine(), actor.holstered_ammo), (24, 10));
        actor_mut(&mut app).holstered_ammo = 15;
        assert_eq!(ask(&mut app, remove_request(1)), Ok(FitDone::Removed));
        assert_eq!(player(&mut app).2.holstered_ammo, 10);
        // Fewer rounds than the smaller magazine holds are left alone.
        actor_mut(&mut app).ammo = 7;
        ask(&mut app, fit_request(0, bought(items, FEED))).unwrap();
        assert_eq!(ask(&mut app, remove_request(0)), Ok(FitDone::Removed));
        assert_eq!(player(&mut app).2.ammo, 7);
    }

    /// Every answer to `requests`, all put in the inbox before one tick.
    fn ask_all(app: &mut App, requests: Vec<FitRequest>) -> Vec<Result<FitDone, FitRefusal>> {
        app.world_mut().resource_mut::<FitInbox>().extend(requests);
        app.update();
        app.world_mut()
            .resource_mut::<Messages<FitAnswer>>()
            .drain()
            .map(|answer| answer.outcome)
            .collect()
    }

    /// How many units of `item` the pack, its bags, the store and the
    /// mounts of the weapons in hand hold between them.
    fn units(app: &mut App, items: &ItemCatalogue, item: &str) -> u32 {
        let (pack, hands, _) = player(app);
        let store = app.world().resource::<HouseholdStore>().clone();
        let loose: u32 = carried_cells(&pack, items)
            .chain(store.0.slots().iter().flatten())
            .filter_map(|stack| items::read(stack, items).ok())
            .filter(|record| record.item.to_string() == item)
            .map(|record| record.count)
            .sum();
        let fitted = (0..HANDS)
            .filter_map(|hand| held(items, &hands, hand).ok())
            .flat_map(|weapon| weapon.fitted.into_values())
            .filter(|module| module.item.to_string() == item)
            .count();
        loose + u32::try_from(fitted).unwrap()
    }

    /// Requests that arrive in one tick are consumed in order, each on
    /// what the one before left: a fit, a second fit that swaps it out, a
    /// removal, and a fit asked twice, whose second asking is refused with
    /// nothing changed. No unit is made or lost on the way.
    #[test]
    fn requests_in_one_tick_are_answered_in_order_on_each_others_work() {
        let mut app = first_day();
        let registries = app.world().resource::<Registries>().clone();
        let items = &registries.0.items;
        prop(&mut app, InteractionKind::Workbench);
        let (store_grade, made) = (bought(items, FEED), batch_412(items));
        let all = units(&mut app, items, FEED);
        assert_eq!(all, 6);
        actor_mut(&mut app).ammo = 21;

        let answers = ask_all(
            &mut app,
            vec![
                fit_request(0, store_grade.clone()),
                fit_request(0, made.clone()),
                remove_request(0),
            ],
        );
        assert_eq!(
            answers,
            [
                Ok(FitDone::Fitted { magazine: 29 }),
                Ok(FitDone::Fitted { magazine: 29 }),
                Ok(FitDone::Removed),
            ]
        );
        let (pack, hands, actor) = player(&mut app);
        assert_eq!(held(items, &hands, 0).unwrap(), weapons(items)[0]);
        assert!(only_the_action(actor.loadout()));
        assert_eq!((actor.magazine(), actor.ammo), (24, 21));
        // The swap put the store's magazine in the pack, and the removal
        // the batch's beside it: carried, not sent back to the store.
        assert_eq!(triple(pack.cells.get(0), items), of(FEED, 1, 0));
        assert_eq!(triple(pack.cells.get(1), items), of(FEED, 1, 412));
        let store = app.world().resource::<HouseholdStore>().clone();
        assert_eq!(triple(store.0.get(0), items), of(FEED, 3, 0));
        assert_eq!(triple(store.0.get(1), items), of(FEED, 1, 412));
        assert_eq!(units(&mut app, items, FEED), all);

        // The same fit asked twice before the tick: once done, once
        // refused, and one unit taken.
        let answers = ask_all(
            &mut app,
            vec![
                fit_request(0, store_grade.clone()),
                fit_request(0, store_grade.clone()),
                remove_request(0),
                remove_request(0),
            ],
        );
        assert_eq!(
            answers,
            [
                Ok(FitDone::Fitted { magazine: 29 }),
                Err(FitRefusal::AlreadyFitted),
                Ok(FitDone::Removed),
                Err(FitRefusal::NothingFitted),
            ]
        );
        assert_eq!(units(&mut app, items, FEED), all);
        assert_eq!(
            held(items, &player(&mut app).1, 0).unwrap(),
            weapons(items)[0]
        );
    }

    #[test]
    fn away_from_a_workbench_it_is_refused() {
        let mut app = first_day();
        let registries = app.world().resource::<Registries>().clone();
        let items = &registries.0.items;
        let feed = bought(items, FEED);
        let before = (
            player(&mut app),
            app.world().resource::<HouseholdStore>().clone(),
        );
        let unchanged = |app: &mut App| {
            let now = (
                player(app),
                app.world().resource::<HouseholdStore>().clone(),
            );
            assert_eq!(format!("{now:?}"), format!("{before:?}"));
        };

        let refused = ask(&mut app, fit_request(0, feed.clone()));
        assert_eq!(refused, Err(FitRefusal::NoWorkbench));
        assert_eq!(
            ask(&mut app, remove_request(0)),
            Err(FitRefusal::NoWorkbench)
        );
        unchanged(&mut app);
        // A locker is not a workbench, though the store is reached there.
        let locker = prop(&mut app, InteractionKind::Locker);
        let refused = ask(&mut app, fit_request(0, feed.clone()));
        assert_eq!(refused, Err(FitRefusal::NoWorkbench));
        unchanged(&mut app);

        // At a bench beside it, with the locker the nearer prompt or not.
        let bench = prop(&mut app, InteractionKind::Workbench);
        assert!(app.world().resource::<Reach>().workbench);
        // Not in the middle of a reload.
        actor_mut(&mut app).action = Action::Reload;
        assert_eq!(
            ask(&mut app, fit_request(0, feed.clone())),
            Err(FitRefusal::Busy)
        );
        actor_mut(&mut app).action = Action::Ready;
        unchanged(&mut app);
        // The stores screen is open on another menu: its mirror is told.
        let menu = MenuId(7);
        app.world_mut()
            .resource_mut::<StockInbox>()
            .push(StockRequest::Open {
                menu,
                with_store: true,
            });
        app.update();
        let done = ask(&mut app, fit_request(0, feed.clone()));
        assert_eq!(done, Ok(FitDone::Fitted { magazine: 29 }));
        let mirrored: Vec<StockAnswer> = app
            .world_mut()
            .resource_mut::<Messages<StockAnswer>>()
            .drain()
            .collect();
        let [
            StockAnswer::Resync {
                menu: told,
                snapshot,
            },
        ] = &mirrored[..]
        else {
            panic!("{mirrored:?}");
        };
        assert_eq!(*told, menu);
        let (_, hands, _) = player(&mut app);
        let store = app.world().resource::<HouseholdStore>().clone();
        assert_eq!(snapshot.inventories[items::StockLayout::HANDS], hands.0);
        assert_eq!(snapshot.inventories[items::StockLayout::STORE], store.0);
        assert_eq!(triple(store.0.get(0), items), of(FEED, 3, 0));

        // The bench gone, the fitted magazine stays where it is.
        app.world_mut().despawn(bench);
        app.world_mut().despawn(locker);
        app.update();
        assert_eq!(
            ask(&mut app, remove_request(0)),
            Err(FitRefusal::NoWorkbench)
        );
        assert_eq!(player(&mut app).2.magazine(), 29);
        // A run that opens the screen for a still works from anywhere.
        app.insert_resource(WorkbenchAccess::Anywhere);
        assert_eq!(ask(&mut app, remove_request(0)), Ok(FitDone::Removed));
        assert_eq!(player(&mut app).2.magazine(), 24);

        // Nobody to hold a weapon.
        let mut empty = app_in_world();
        empty.insert_resource(shipped());
        assert_eq!(
            ask(&mut empty, fit_request(0, feed)),
            Err(FitRefusal::NoPlayer)
        );
    }

    #[test]
    fn a_shut_mount_refuses_with_its_threshold() {
        let registries = shipped();
        let items = &registries.0.items;
        let [carbine, pistol] = weapons(items);
        let feed = bought(items, FEED);
        // Asked of the mount before the part: a bought carbine's muzzle
        // opens at 600 whatever is offered to it.
        let shut = |opens_at| Err(FitRefusal::Shut { opens_at });
        assert_eq!(with(items, &carbine, "muzzle", Some(&feed)), shut(600));
        assert_eq!(with(items, &carbine, "fore_end", None), shut(800));
        assert_eq!(
            with(items, &carbine, "stock", Some(&feed)),
            Err(FitRefusal::WrongMount)
        );
        assert_eq!(
            with(items, &carbine, "cooling", Some(&feed)),
            Err(FitRefusal::NoMount)
        );
        assert_eq!(
            with(items, &pistol, "feed", Some(&feed)),
            Err(FitRefusal::DoesNotFit)
        );
        assert_eq!(
            with(items, &carbine, "feed", Some(&bought(items, HAUL))),
            Err(FitRefusal::DoesNotFit),
            "a bag is no module"
        );
        assert_eq!(
            with(items, &loose(&feed), "feed", Some(&feed)),
            Err(FitRefusal::NoWeapon)
        );
        let fitted = with(items, &carbine, "feed", Some(&feed)).unwrap();
        assert_eq!(
            with(items, &fitted, "feed", Some(&feed)),
            Err(FitRefusal::AlreadyFitted)
        );
        assert_eq!(with(items, &fitted, "feed", None).as_ref(), Ok(&carbine));
        assert_eq!(with(items, &carbine, "feed", None).as_ref(), Ok(&carbine));

        let states = |weapon: &Record| -> Vec<(String, MountState)> { mounts(items, weapon) };
        let state = |slot: &str, state| (slot.to_owned(), state);
        assert_eq!(
            states(&fitted),
            [
                state("sight", MountState::Standard),
                state("feed", MountState::Fitted(feed.clone())),
                state("grip", MountState::Standard),
                state("stock", MountState::Standard),
                state("muzzle", MountState::Shut { opens_at: 600 }),
                state("fore_end", MountState::Shut { opens_at: 800 }),
            ]
        );
        assert_eq!(states(&pistol).len(), 3);
        assert!(
            states(&pistol)
                .iter()
                .all(|(_, state)| *state == MountState::Standard)
        );
        // A rough pistol's sight is shut, a fine carbine's muzzle open.
        let mut rough = pistol.clone();
        rough.lines.insert(MODDABILITY.to_owned(), 399);
        assert_eq!(
            states(&rough)[0],
            state("sight", MountState::Shut { opens_at: 400 })
        );
        let mut fine = carbine.clone();
        fine.lines.insert(MODDABILITY.to_owned(), 600);
        assert_eq!(states(&fine)[4], state("muzzle", MountState::Standard));
        assert!(mounts(items, &loose(&feed)).is_empty());

        // The request is refused the same way and moves nothing.
        let mut app = first_day();
        prop(&mut app, InteractionKind::Workbench);
        let before = format!("{:?}", player(&mut app));
        let refused = ask(
            &mut app,
            FitRequest::Fit {
                hand: 0,
                mount: "muzzle".to_owned(),
                module: feed,
            },
        );
        assert_eq!(refused, Err(FitRefusal::Shut { opens_at: 600 }));
        assert_eq!(format!("{:?}", player(&mut app)), before);
    }

    #[test]
    fn a_line_adds_up_on_screen() {
        let registries = shipped();
        let items = &registries.0.items;
        let [carbine, pistol] = weapons(items);
        let actor = Combatant::default();
        let table = [
            Property::Capacity,
            Property::ReloadTime,
            Property::RepeatInterval,
            Property::EquipTime,
            Property::AimMoveSpeed,
        ];
        let shown = |now: &Record, then: &Record, bare: &Row, which: &[Property]| -> Vec<String> {
            lines(items, bare, now, then, which)
                .iter()
                .map(|line| {
                    let change = line.change.map_or_else(String::new, |change| {
                        let word = if change.better { "better" } else { "worse" };
                        format!(" {} {word}", line.value.signed(change.by))
                    });
                    format!("{}{change}", line.value.text())
                })
                .collect()
        };
        let bare = actor.bare(Weapon::Carbine);
        let fed = with(items, &carbine, "feed", Some(&bought(items, FEED))).unwrap();

        // Focusing the bought magazine, against the standard feed: five
        // more rounds, and 1.35 + 0.14 reads 1.49 though the reload is
        // 1.485.
        assert_eq!(
            shown(&carbine, &fed, &bare, &table),
            ["29 +5 better", "1.49 +0.14 worse", "0.462", "0.45", "65"]
        );
        // The bare column: the weapon with nothing fitted, against itself.
        assert_eq!(
            shown(&carbine, &carbine, &bare, &table),
            ["24", "1.35", "0.462", "0.45", "65"]
        );
        // Taking it off again, against what is in the mount now.
        assert_eq!(
            shown(&fed, &carbine, &bare, &table[..2]),
            ["24 -5 worse", "1.35 -0.14 better"]
        );
        // Batch 412 reads as the bought one does, so against it nothing
        // changes on the screen.
        let made = with(items, &carbine, "feed", Some(&batch_412(items))).unwrap();
        assert_eq!(shown(&fed, &made, &bare, &table[..2]), ["29", "1.49"]);
        // The best magazine against the bought one.
        let best = with(items, &carbine, "feed", Some(&graded(items, FEED, 1000))).unwrap();
        assert_eq!(
            shown(&fed, &best, &bare, &table[..2]),
            ["32 +3 better", "1.42 -0.07 better"]
        );

        let units: Vec<(Unit, u8, bool)> = lines(items, &bare, &carbine, &fed, &table)
            .iter()
            .map(|line| (line.value.unit, line.value.decimals, line.live))
            .collect();
        assert_eq!(
            units,
            [
                (Unit::Rounds, 0, true),
                (Unit::Seconds, 2, true),
                (Unit::Seconds, 3, true),
                (Unit::Seconds, 2, true),
                (Unit::Percent, 0, true),
            ]
        );

        // The pistol's 0.90 x 1.15 is 1.035 and reads 1.03.
        let holstered = actor.bare(Weapon::Pistol);
        let celled = with(items, &pistol, "feed", Some(&bought(items, CELL))).unwrap();
        assert_eq!(
            shown(&pistol, &celled, &holstered, &table[..2]),
            ["13 +3 better", "1.03 +0.13 worse"]
        );

        // The sight: a slower draw and a tighter aimed spread, which the
        // fight has both and which reads in degrees.
        let sighted = with(items, &carbine, "sight", Some(&bought(items, REFLEX))).unwrap();
        let sight = [Property::EquipTime, Property::AimSpread];
        assert_eq!(
            shown(&carbine, &sighted, &bare, &sight),
            ["0.46 +0.01 worse", "1.40 -0.60 better"]
        );
        let spread = lines(items, &bare, &carbine, &sighted, &sight)[1];
        assert_eq!((spread.live, spread.value.unit), (true, Unit::Degrees));
        assert_eq!(shown(&carbine, &carbine, &bare, &sight[1..]), ["2.00"]);
        let gripped = with(items, &sighted, "grip", Some(&bought(items, GRIP))).unwrap();
        // Both on: 0.45 x 1.03 x 0.95 is 0.44, a hundredth quicker than
        // standard, and the recoil the grip costs is in words only.
        assert_eq!(
            shown(
                &carbine,
                &gripped,
                &bare,
                &[Property::EquipTime, Property::RecoilRecovery]
            ),
            ["0.44 -0.01 better", "105 +5 worse"]
        );

        // A holder's own boxes are in the figures: Carbine 2's reload.
        let mut sheet = CharacterSheet::default();
        sheet.learned.insert("marksman.carbine.2".to_owned());
        let mut boxed = Combatant::default();
        boxed.fit(&SkillGraph::shipped(), &sheet);
        assert_eq!(
            shown(&carbine, &fed, &boxed.bare(Weapon::Carbine), &table[..2]),
            ["29 +5 better", "1.26 +0.11 worse"]
        );
        // The digits of a negative fraction keep their sign and zeros.
        let seconds = Amount::of(0.045, 2, Unit::Seconds);
        assert_eq!(
            (seconds.text().as_str(), seconds.signed(-5).as_str()),
            ("0.05", "-0.05")
        );
        assert_eq!(seconds.signed(0), "0.00");
    }
}
