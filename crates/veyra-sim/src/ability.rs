//! The adapter half of the `ability` rules: its requests, systems and plugin.

pub use veyra_rules::ability::*;

use std::collections::BTreeSet;

use bevy::app::{App, Plugin};

use bevy::ecs::prelude::*;

use crate::combat::{CombatEncounter, Combatant};

use crate::combat_tables::{self};

use crate::order::RequestSet;

use crate::progression::CharacterSheet;

use crate::{AppRequestExt, Inbox, Registries, Request};

/// What a request or a screen asks of the bar.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BarRequest {
    /// Put an ability in a place; one that sits in another place of the
    /// class moves.
    Set {
        /// Where.
        place: Place,
        /// Which, by id.
        ability: String,
    },
    /// Empty a place.
    Clear {
        /// Which.
        place: Place,
    },
}

/// The requests not yet consumed.
pub type BarInbox = Inbox<BarRequest>;

/// With no one to hold a bar, the answer says so.
impl Request for BarRequest {
    type Answer = BarAnswer;

    fn no_actor(&self, commands: &mut Commands) {
        commands.write_message(BarAnswer {
            result: Err(BarRefusal::NoPlayer),
        });
    }
}

/// The sim's answer to a [`BarRequest`].
#[derive(Message, Debug, Clone, Copy, PartialEq, Eq)]
pub struct BarAnswer {
    /// The place that changed, or why nothing did.
    pub result: Result<Place, BarRefusal>,
}

/// Apply one request to `bar`, as `known` and the table allow.
///
/// # Errors
/// What [`can_slot`] refuses; a clear of a place the bar has not refuses with
/// [`SlotRefusal::NoPlace`].
pub fn change(
    table: &AbilityTable,
    known: &BTreeSet<String>,
    bar: &mut Bar,
    request: &BarRequest,
) -> Result<Place, SlotRefusal> {
    match request {
        BarRequest::Set { place, ability } => {
            // An ability that sits in another place of the class moves: take
            // it out of there and ask the rule on what is left.
            let mut next = bar.clone();
            for other in next.places_of(ability) {
                let same = match (place, &other) {
                    (Place::Weapon(a, _), Place::Weapon(b, _)) => a == b,
                    (Place::Utility(_), Place::Utility(_)) => true,
                    _ => false,
                };
                if same && other != *place {
                    next.clear(other);
                }
            }
            can_slot(table, known, &next, *place, ability)?;
            next.set(*place, ability);
            *bar = next;
            Ok(*place)
        }
        BarRequest::Clear { place } => {
            let in_range = match place {
                Place::Weapon(_, at) => *at < table.bar.weapon,
                Place::Utility(at) => *at < table.bar.utility,
            };
            if !in_range {
                return Err(SlotRefusal::NoPlace);
            }
            bar.clear(*place);
            Ok(*place)
        }
    }
}

/// `SimTick`, `SimSet::Input`, after equip's: every request made since the
/// last tick, in order, each on the bar the one before left, refused while
/// the player is fighting.
pub fn consume(
    mut inbox: ResMut<BarInbox>,
    registries: Res<Registries>,
    encounter: Option<Res<CombatEncounter>>,
    mut players: Query<(&CharacterSheet, &mut Bar, &Combatant)>,
    mut answers: MessageWriter<BarAnswer>,
) {
    let requests = inbox.take();
    if requests.is_empty() {
        return;
    }
    let engaged = encounter.is_some_and(|e| e.engaged && e.remaining > 0);
    for (actor, request) in requests {
        let mut player = players.get_mut(actor.0).ok();
        let result = match player.as_mut() {
            None => Err(BarRefusal::NoPlayer),
            Some((sheet, bar, actor)) => {
                if actor.in_combat(engaged) {
                    Err(BarRefusal::InFight)
                } else {
                    let known = known_to(&registries.0.progression, sheet, bar);
                    let mut next = (**bar).clone();
                    let done = combat_tables::with(|tables| {
                        change(&tables.abilities, &known, &mut next, &request)
                    })
                    .map_err(BarRefusal::Slot);
                    if done.is_ok() && **bar != next {
                        **bar = next;
                    }
                    done
                }
            }
        };
        answers.write(BarAnswer { result });
    }
}

/// `SimTick`, `SimSet::Input`, after [`consume`]: a member whose sheet or
/// bar changed has its bar held to what the sheet grants (a learn places,
/// a forget takes out, a save read back is mended), and the combatant is
/// given the bar. The sheet is fitted wherever it changes; this follows it.
pub fn follow_sheet(
    registries: Res<Registries>,
    mut members: Query<
        (&CharacterSheet, &mut Bar, &mut Combatant),
        Or<(Changed<CharacterSheet>, Changed<Bar>)>,
    >,
) {
    for (sheet, mut bar, mut actor) in &mut members {
        let known = known_to(&registries.0.progression, sheet, &bar);
        let mut next = bar.clone();
        combat_tables::with(|tables| next.follow(&tables.abilities, &known));
        if *bar != next {
            *bar = next;
        }
        actor.set_bar(&bar);
    }
}

/// Registers the bar's request and consumes it in the tick. Added by
/// `SimPlugin`.
#[derive(Debug, Clone, Copy)]
pub struct AbilityPlugin;

impl Plugin for AbilityPlugin {
    fn build(&self, app: &mut App) {
        app.add_request::<BarRequest, _>(RequestSet::Ability, (consume, follow_sheet).chain());
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
        reason = "a table figure and a factor on it are compared to the bit"
    )]

    use super::*;
    use crate::LocalPlayer;

    use bevy::ecs::message::Messages;
    use bevy::ecs::system::RunSystemOnce;

    use std::sync::Arc;
    use strata_registry::registries::GameRegistries;

    use crate::combat::Action;
    use crate::combat::CombatEncounter;
    use crate::combat::Combatant;
    use crate::combat::Weapon;
    use strata_registry::progression::SkillGraph;

    use crate::progression::CharacterSheet;

    use crate::Registries;

    pub(crate) use veyra_rules::ability::test_support::*;

    #[test]
    fn setting_an_ability_that_sits_elsewhere_moves_it() {
        let table = table();
        let known = set(&["butt_strike"]);
        let mut bar = Bar::default();
        let at = |n| gun_place(Weapon::Carbine, n);
        let set_at = |n| BarRequest::Set {
            place: at(n),
            ability: "butt_strike".to_owned(),
        };
        assert_eq!(change(&table, &known, &mut bar, &set_at(0)), Ok(at(0)));
        assert_eq!(change(&table, &known, &mut bar, &set_at(2)), Ok(at(2)));
        assert_eq!(bar.get(at(0)), None);
        assert_eq!(bar.get(at(2)), Some("butt_strike"));
        assert_eq!(
            change(
                &table,
                &known,
                &mut bar,
                &BarRequest::Clear { place: at(2) }
            ),
            Ok(at(2))
        );
        assert_eq!(bar.get(at(2)), None);
        assert_eq!(
            change(
                &table,
                &known,
                &mut bar,
                &BarRequest::Clear { place: at(3) }
            ),
            Err(SlotRefusal::NoPlace)
        );
    }

    fn world_with(sheet: &[&str]) -> (World, Entity) {
        let mut world = World::new();
        let registries = GameRegistries {
            progression: SkillGraph::shipped(),
            ..GameRegistries::default()
        };
        world.insert_resource(Registries(Arc::new(registries)));
        world.init_resource::<BarInbox>();
        world.init_resource::<Messages<BarAnswer>>();
        let player = world
            .spawn((
                LocalPlayer,
                CharacterSheet {
                    learned: sheet.iter().map(|id| (*id).to_owned()).collect(),
                    ..CharacterSheet::default()
                },
                Combatant::default(),
            ))
            .id();
        (world, player)
    }

    fn answers(world: &mut World) -> Vec<BarAnswer> {
        world
            .resource_mut::<Messages<BarAnswer>>()
            .drain()
            .collect()
    }

    #[test]
    fn a_request_in_a_fight_is_refused_and_out_of_one_is_answered() {
        let (mut world, player) = world_with(&["blade.novice"]);
        world.run_system_once(follow_sheet).unwrap();
        let place = gun_place(Weapon::Glaive, 2);
        let ask = |world: &mut World| {
            world.resource_mut::<BarInbox>().push(BarRequest::Set {
                place,
                ability: "heavy_cut".to_owned(),
            });
            world
                .run_system_once(strata_app::request::address::<BarRequest>)
                .unwrap();
            world.run_system_once(consume).unwrap();
            answers(world)
        };
        // The cut learned stands in the first place of each class it works with.
        let bar = world.get::<Bar>(player).unwrap();
        assert_eq!(bar.get(gun_place(Weapon::Glaive, 0)), Some("heavy_cut"));
        world.get_mut::<Combatant>(player).unwrap().action = Action::Light;
        assert_eq!(
            ask(&mut world),
            [BarAnswer {
                result: Err(BarRefusal::InFight)
            }]
        );
        assert_eq!(
            world.get::<Bar>(player).unwrap().get(place),
            None,
            "a refusal changes nothing"
        );
        world.get_mut::<Combatant>(player).unwrap().action = Action::Ready;
        assert_eq!(ask(&mut world), [BarAnswer { result: Ok(place) }]);
        let bar = world.get::<Bar>(player).unwrap();
        assert_eq!(bar.get(place), Some("heavy_cut"));
        assert_eq!(bar.get(gun_place(Weapon::Glaive, 0)), None, "it moved");
        // The encounter being engaged is a fight too.
        world.insert_resource(CombatEncounter {
            engaged: true,
            remaining: 1,
            ..CombatEncounter::default()
        });
        assert_eq!(
            ask(&mut world),
            [BarAnswer {
                result: Err(BarRefusal::InFight)
            }]
        );
        // A won fight is over: engaged stays set, nothing remains.
        world.insert_resource(CombatEncounter {
            engaged: true,
            remaining: 0,
            ..CombatEncounter::default()
        });
        assert_eq!(ask(&mut world), [BarAnswer { result: Ok(place) }]);
    }

    #[test]
    fn learning_and_forgetting_the_shipped_box_goes_through_the_sheet() {
        let (mut world, player) = world_with(&[]);
        world.run_system_once(follow_sheet).unwrap();
        assert!(
            world
                .get::<Bar>(player)
                .unwrap()
                .places_of("heavy_cut")
                .is_empty()
        );
        world
            .get_mut::<CharacterSheet>(player)
            .unwrap()
            .learned
            .insert("blade.novice".to_owned());
        world.run_system_once(follow_sheet).unwrap();
        let places = world.get::<Bar>(player).unwrap().places_of("heavy_cut");
        assert_eq!(places.len(), 3, "one in each melee class: {places:?}");
        assert_eq!(
            world
                .get::<Combatant>(player)
                .unwrap()
                .bar()
                .places_of("heavy_cut"),
            places,
            "and the combatant has it"
        );
        world
            .get_mut::<CharacterSheet>(player)
            .unwrap()
            .learned
            .clear();
        world.run_system_once(follow_sheet).unwrap();
        assert!(
            world
                .get::<Bar>(player)
                .unwrap()
                .places_of("heavy_cut")
                .is_empty()
        );
        assert!(
            world
                .get::<Combatant>(player)
                .unwrap()
                .bar()
                .places_of("heavy_cut")
                .is_empty()
        );
    }
}
