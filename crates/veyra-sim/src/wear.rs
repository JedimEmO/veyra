//! The adapter half of the `wear` rules: its requests, systems and plugin.

pub use veyra_rules::wear::*;

use bevy::app::{App, Plugin};

use bevy::ecs::prelude::*;

use strata_registry::item::HANDS;

use crate::combat::{Action, AttackKind, AttackRequest, Combatant, ShotFired};

use crate::fitting::{self, WorkbenchAccess};

use crate::household::Stable;

use crate::items::{self, Hands, HouseholdStore, OpenStock, Pack, Stock, StockAnswer};

use crate::order::RequestSet;

use crate::progression::{CharacterSheet, Reach};

use crate::{AppRequestExt, Inbox, Registries, Request, SimSet, SimTick};

/// `SimTick`, `SimSet::Observe`: the shots and cuts of the tick wear the
/// weapon in hand, and a fall knocks everything carried, once.
pub fn tick(
    registries: Res<Registries>,
    mut shots: MessageReader<ShotFired>,
    mut attacks: MessageReader<AttackRequest>,
    mut notices: MessageWriter<WearNotice>,
    mut actors: Query<(
        &Combatant,
        &mut Hands,
        &mut WearCarry,
        Option<&mut Pack>,
        Option<&CharacterSheet>,
    )>,
) {
    let items = &registries.0.items;
    let rules = items.wear();
    let graph = &registries.0.progression;
    for shot in shots.read() {
        if let Ok((actor, mut hands, mut carry, _, sheet)) = actors.get_mut(shot.attacker) {
            spend(
                items,
                actor,
                &mut hands,
                &mut carry,
                (rules.shot, bearing(graph, sheet)),
            );
        }
    }
    for attack in attacks.read() {
        if !matches!(
            attack.kind,
            AttackKind::Melee { .. } | AttackKind::Strike { .. }
        ) {
            continue;
        }
        if let Ok((actor, mut hands, mut carry, _, sheet)) = actors.get_mut(attack.attacker)
            && carry.last_cut != Some(attack.sequence)
        {
            carry.last_cut = Some(attack.sequence);
            spend(
                items,
                actor,
                &mut hands,
                &mut carry,
                (rules.cut, bearing(graph, sheet)),
            );
        }
    }
    for (actor, mut hands, mut carry, pack, _) in &mut actors {
        let dead = actor.action == Action::Dead;
        if dead == carry.down {
            continue;
        }
        carry.down = dead;
        if !dead || rules.down == 0 {
            continue;
        }
        for hand in 0..HANDS {
            let Some(mut record) = hands
                .0
                .get(hand)
                .and_then(|stack| items::read(stack, items).ok())
            else {
                continue;
            };
            if lose(rules, &mut record, rules.down)
                && let Ok(stack) = items::write(&record, items)
            {
                hands.0.set(hand, Some(stack));
                hands.0.clear_changed();
            }
            carry.hands[hand] = 0;
        }
        if let Some(mut pack) = pack {
            let open: [usize; strata_registry::item::BAG_SLOTS] =
                std::array::from_fn(|bag| usize::from(pack.open_cells(bag, items)));
            let cells = pack.cells.len();
            knock(items, &mut pack.cells, cells, rules.down);
            for (bag, open) in pack.bags.iter_mut().zip(open) {
                knock(items, bag, open, rules.down);
            }
            if let Some(stack) = &mut pack.cursor {
                knock_stack(items, stack, rules.down);
            }
        }
        notices.write(WearNotice::Knocked { points: rules.down });
    }
}

/// What a screen asks of wear.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WearRequest {
    /// Mend the stabled bike with a kit.
    RepairBike,
    /// Mend the weapon in a hand with a kit.
    Repair {
        /// The hand the weapon is in: the carbine's, then the pistol's.
        hand: usize,
    },
}

/// The requests not yet consumed, a resource for the reason the fitting
/// inbox is one.
pub type WearInbox = Inbox<WearRequest>;

/// With nobody to mend for, the answer is the one for empty hands, as it
/// always was.
impl Request for WearRequest {
    type Answer = RepairAnswer;

    fn no_actor(&self, commands: &mut Commands) {
        commands.write_message(RepairAnswer {
            result: Err(RepairRefusal::EmptyHand),
        });
    }
}

/// `SimTick`, `SimSet::Input`, after the factory's requests: every repair
/// asked for since the last tick, in order, each on the stock the one
/// before left. With a workbench in reach the store's kits are to hand and
/// everything is mended; away from one only a carried kit is, and part.
#[allow(
    clippy::too_many_arguments,
    reason = "the requests, the rule, the stock and its open menu in, two answers out"
)]
pub fn consume(
    mut inbox: ResMut<WearInbox>,
    access: Res<WorkbenchAccess>,
    reach: Res<Reach>,
    registries: Res<Registries>,
    open: Res<OpenStock>,
    mut store: ResMut<HouseholdStore>,
    mut stable: ResMut<Stable>,
    mut players: Query<(
        &mut Pack,
        &mut Hands,
        &mut WearCarry,
        Option<&CharacterSheet>,
    )>,
    mut answers: MessageWriter<RepairAnswer>,
    mut mirrors: MessageWriter<StockAnswer>,
) {
    let requests = inbox.take();
    if requests.is_empty() {
        return;
    }
    let items = &registries.0.items;
    let at_bench = fitting::at_workbench(*access, &reach);
    let mut changed = false;
    let mut last = None;
    for (actor, request) in requests {
        let mut player = players.get_mut(actor.0).ok();
        if player.is_some() {
            last = Some(actor.0);
        }
        let result = match player.as_mut() {
            None => Err(RepairRefusal::EmptyHand),
            Some((pack, hands, carry, sheet)) => {
                let mut next = ((**pack).clone(), (**hands).clone(), store.clone());
                let mut bike = stable.bike.clone();
                let bearer = sheet.map(|sheet| Bearer {
                    graph: &registries.0.progression,
                    sheet,
                });
                let mut stock = Stock {
                    saddle: None,
                    pack: &mut next.0,
                    hands: &mut next.1,
                    store: Some(&mut next.2),
                };
                let done = match request {
                    WearRequest::Repair { hand } => {
                        repair(items, &mut stock, hand, (at_bench, bearer))
                    }
                    WearRequest::RepairBike => {
                        repair_bike(items, &mut stock, &mut bike, (at_bench, bearer))
                    }
                };
                if done.is_ok() {
                    changed = true;
                    if stable.bike != bike {
                        stable.bike = bike;
                    }
                    if **pack != next.0 {
                        **pack = next.0;
                    }
                    **hands = next.1;
                    if *store != next.2 {
                        *store = next.2;
                    }
                    if let WearRequest::Repair { hand } = request {
                        carry.hands[hand] = 0;
                    }
                }
                done
            }
        };
        answers.write(RepairAnswer { result });
    }
    if changed
        && let Some(opened) = &open.0
        && let Some(actor) = last
        && let Ok((pack, hands, _, _)) = players.get(actor)
    {
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

/// Registers wear and its request. Added by `SimPlugin`, so a server and a
/// harness have it too; with no hands and no request it touches nothing.
#[derive(Debug, Clone, Copy)]
pub struct WearPlugin;

impl Plugin for WearPlugin {
    fn build(&self, app: &mut App) {
        app.add_message::<WearNotice>()
            .add_request::<WearRequest, _>(RequestSet::Wear, consume)
            .add_systems(SimTick, tick.in_set(SimSet::Observe));
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
        reason = "a worn row is the table's number times a factor exactly; a bit off is a wrong merge"
    )]

    use super::*;
    use crate::LocalPlayer;
    use crate::combat::Weapon;
    use crate::fitting;
    use crate::fitting::FitInbox;
    use crate::fitting::FitRequest;
    use crate::fitting::loadout;
    use crate::progression::Interaction;
    use crate::progression::InteractionKind;
    use crate::tests::app_in_world;
    use bevy::ecs::message::Messages;
    use bevy::math::Vec3;

    use std::collections::BTreeMap;

    use bevy::app::App;
    use strata_core::WorldPos;
    use strata_registry::item::Fitted;
    use strata_registry::item::Property;

    use strata_registry::item::Record;

    use crate::combat::Action;
    use crate::combat::AttackKind;
    use crate::combat::AttackRequest;
    use crate::combat::Combatant;
    use crate::combat::ShotFired;

    use crate::household::Stable;
    use crate::items;
    use crate::items::Hands;
    use crate::items::HouseholdStore;

    use crate::items::Pack;

    use crate::progression::CharacterSheet;

    use crate::Registries;

    pub(crate) use veyra_rules::wear::test_support::*;

    /// The first day: the player holding the starting weapons, the store
    /// as `starting.ron` has it (two store kits among it), nothing in reach.
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

    fn bench(app: &mut App) {
        app.world_mut().spawn((
            crate::WorldPosition::new(0., 0., 0.),
            Interaction {
                kind: InteractionKind::Workbench,
                radius: 2.5,
            },
        ));
        app.update();
    }

    fn player(app: &mut App) -> (Entity, Pack, Hands, Combatant) {
        let world = app.world_mut();
        let mut players =
            world.query_filtered::<(Entity, &Pack, &Hands, &Combatant), With<LocalPlayer>>();
        let (entity, pack, hands, actor) = players.single(world).unwrap();
        (entity, pack.clone(), hands.clone(), actor.clone())
    }

    fn hand(app: &mut App, index: usize) -> Record {
        let items = app.world().resource::<Registries>().0.clone();
        let (_, _, hands, _) = player(app);
        items::read(hands.0.get(index).unwrap(), &items.items).unwrap()
    }

    fn put_hand(app: &mut App, index: usize, record: &Record) {
        let items = app.world().resource::<Registries>().0.clone();
        let stack = items::write(record, &items.items).unwrap();
        let world = app.world_mut();
        let mut players = world.query_filtered::<&mut Hands, With<LocalPlayer>>();
        players.single_mut(world).unwrap().0.set(index, Some(stack));
    }

    fn shoot(app: &mut App, shots: usize) {
        let (entity, ..) = player(app);
        for _ in 0..shots {
            app.world_mut()
                .resource_mut::<Messages<ShotFired>>()
                .write(ShotFired {
                    attacker: entity,
                    from: WorldPos::ZERO,
                    direction: Vec3::X,
                });
        }
        app.update();
    }

    fn repair_all(app: &mut App, hands: &[usize]) -> Vec<Result<Repaired, RepairRefusal>> {
        app.world_mut()
            .resource_mut::<WearInbox>()
            .extend(hands.iter().map(|hand| WearRequest::Repair { hand: *hand }));
        app.update();
        app.world_mut()
            .resource_mut::<Messages<RepairAnswer>>()
            .drain()
            .map(|answer| answer.result)
            .collect()
    }

    fn kits_in(app: &mut App) -> Vec<u16> {
        let items = app.world().resource::<Registries>().0.clone();
        let (_, pack, _, _) = player(app);
        let store = app.world().resource::<HouseholdStore>().clone();
        let mut lines = Vec::new();
        for (record, _) in crate::crafting::reachable(&items.items, &pack, Some(&store)) {
            if record.item == id(KIT) {
                lines.extend(std::iter::repeat_n(
                    restore_line(&record),
                    record.count as usize,
                ));
            }
        }
        lines.sort_unstable();
        lines
    }

    fn put_in_pack(app: &mut App, record: &Record) {
        let items = app.world().resource::<Registries>().0.clone();
        let stack = items::write(record, &items.items).unwrap();
        let world = app.world_mut();
        let mut players = world.query_filtered::<&mut Pack, With<LocalPlayer>>();
        let mut pack = players.single_mut(world).unwrap();
        let cell = pack.cells.slots().iter().position(Option::is_none).unwrap();
        pack.cells.set(cell, Some(stack));
    }

    #[test]
    fn a_swing_costs_once_however_many_ticks_it_touches() {
        let mut app = first_day();
        let (entity, ..) = player(&mut app);
        let swing = |app: &mut App, sequence: u64| {
            app.world_mut()
                .resource_mut::<Messages<AttackRequest>>()
                .write(AttackRequest {
                    attacker: entity,
                    sequence,
                    kind: AttackKind::Melee {
                        action: Action::Dead,
                        from: 0.0,
                        to: 0.1,
                    },
                });
            app.update();
        };
        let carried = |app: &mut App| {
            app.world_mut()
                .query_filtered::<&WearCarry, With<LocalPlayer>>()
                .single(app.world())
                .unwrap()
                .hands[0]
        };
        for _ in 0..3 {
            swing(&mut app, 7);
        }
        // Durability 500: a cut is the dial itself.
        let cut = app.world().resource::<Registries>().0.items.wear().cut;
        assert_eq!(carried(&mut app), cut);
        swing(&mut app, 8);
        assert_eq!(carried(&mut app), 2 * cut);
    }

    #[test]
    fn a_butt_strike_wears_the_weapon_once_like_a_cut() {
        let mut app = first_day();
        let (entity, ..) = player(&mut app);
        for _ in 0..3 {
            app.world_mut()
                .resource_mut::<Messages<AttackRequest>>()
                .write(AttackRequest {
                    attacker: entity,
                    sequence: 4,
                    kind: AttackKind::Strike {
                        reach: 1.7,
                        arc: 50.,
                        damage: 8.,
                        posture: 70.,
                        on_hit: None,
                    },
                });
            app.update();
        }
        let cut = app.world().resource::<Registries>().0.items.wear().cut;
        let carried = app
            .world_mut()
            .query_filtered::<&WearCarry, With<LocalPlayer>>()
            .single(app.world())
            .unwrap()
            .hands[0];
        assert_eq!(carried, cut);
    }

    #[test]
    fn a_cut_wears_the_blade_that_made_it_and_a_worn_blade_cuts_for_less() {
        let mut app = first_day();
        let (entity, ..) = player(&mut app);
        let items = app.world().resource::<Registries>().0.clone();
        let blade = crate::crafting::bought(
            items
                .items
                .get(&strata_registry::Namespaced::parse("veyra:weapon.service_blade").unwrap())
                .unwrap(),
        );
        // The blade takes the pistol's hand and is drawn.
        put_hand(&mut app, 1, &blade);
        app.update();
        app.world_mut().get_mut::<Combatant>(entity).unwrap().weapon = Weapon::Blade;
        app.world_mut()
            .resource_mut::<Messages<AttackRequest>>()
            .write(AttackRequest {
                attacker: entity,
                sequence: 3,
                kind: AttackKind::Melee {
                    action: Action::Light,
                    from: 0.0,
                    to: 0.1,
                },
            });
        app.update();
        let carried = app
            .world_mut()
            .query_filtered::<&WearCarry, With<LocalPlayer>>()
            .single(app.world())
            .unwrap()
            .hands;
        assert_eq!(carried[0], 0, "the carbine did not make the cut");
        assert_eq!(carried[1], items.items.wear().cut, "the blade did");
        // Sound, it cuts at its edge's figure; worn to nothing, for less.
        let damage = |app: &mut App| {
            app.world()
                .get::<Combatant>(entity)
                .unwrap()
                .spec(Action::Light)
                .damage
        };
        app.update();
        let sound = damage(&mut app);
        assert!((sound - 28. * 0.95).abs() < 1e-4, "{sound}");
        let mut worn = hand(&mut app, 1);
        worn.condition = Some(0);
        put_hand(&mut app, 1, &worn);
        app.update();
        let worn = damage(&mut app);
        assert!((worn - 28. * 0.95 * 0.8).abs() < 1e-3, "{worn}");
        assert!(worn < sound);
    }

    #[test]
    fn condition_moves_in_steps_and_never_under_nought() {
        let mut app = first_day();
        // At durability 500 a shot is 2 thousandths of a point: a step of
        // 25 points is 12.5 shots.
        shoot(&mut app, 12);
        assert_eq!(hand(&mut app, 0).condition, Some(1000));
        shoot(&mut app, 1);
        assert_eq!(hand(&mut app, 0).condition, Some(975));
        let mut weapon = hand(&mut app, 0);
        weapon.condition = Some(10);
        put_hand(&mut app, 0, &weapon);
        shoot(&mut app, 40);
        assert_eq!(hand(&mut app, 0).condition, Some(0));
        shoot(&mut app, 40);
        assert_eq!(hand(&mut app, 0).condition, Some(0));
        // The pistol in the other hand was never touched.
        assert_eq!(hand(&mut app, 1).condition, Some(1000));
    }

    #[test]
    fn a_fall_knocks_everything_carried_once() {
        let mut app = first_day();
        let items = app.world().resource::<Registries>().0.clone();
        let mut worn = feed();
        worn.condition = Some(600);
        let mut spare = Record {
            item: worn.item.clone(),
            count: 1,
            batch: 0,
            by: None,
            lines: worn.lines.clone(),
            condition: worn.condition,
            fitted: BTreeMap::new(),
            origin: None,
        };
        put_in_pack(&mut app, &spare);
        spare.condition = None;
        put_in_pack(&mut app, &spare);
        let mut host = hand(&mut app, 0);
        host.fitted.insert("feed".to_owned(), feed());
        put_hand(&mut app, 0, &host);
        app.world_mut()
            .query_filtered::<&mut Combatant, With<LocalPlayer>>()
            .single_mut(app.world_mut())
            .unwrap()
            .action = Action::Dead;
        app.update();
        assert_eq!(app.world().resource::<Messages<WearNotice>>().len(), 1);
        for _ in 0..4 {
            app.update();
        }
        let weapon = hand(&mut app, 0);
        assert_eq!(weapon.condition, Some(850));
        assert_eq!(weapon.fitted["feed"].condition, Some(850));
        assert_eq!(hand(&mut app, 1).condition, Some(850));
        let (_, pack, _, _) = player(&mut app);
        let conditions: Vec<Option<u16>> = pack
            .cells
            .slots()
            .iter()
            .flatten()
            .filter_map(|stack| items::read(stack, &items.items).ok())
            .filter(|record| record.item == id(FEED))
            .map(|record| record.condition)
            .collect();
        // The worn spare lost 150 and the unworn one, which reads full,
        // is knocked from there.
        assert!(conditions.contains(&Some(450)), "{conditions:?}");
        assert!(conditions.contains(&Some(850)), "{conditions:?}");
    }

    #[test]
    fn a_part_wears_with_its_host_and_keeps_it_when_moved() {
        let registries = shipped();
        let items = &registries.0.items;
        let rules = items.wear();
        let mut host = fitting::with(items, &carbine(items), "feed", Some(&feed())).unwrap();
        let mut carry = 0;
        // 750 thousandths short of a step, then past it.
        assert!(!accrue(rules, &mut host, &mut carry, 24_000));
        assert!(accrue(rules, &mut host, &mut carry, 1_000));
        assert_eq!(host.condition, Some(975));
        assert_eq!(host.fitted["feed"].condition, Some(975));
        let (bare, part) = fitting::without(items, &host, "feed").unwrap();
        assert_eq!(bare.condition, Some(975));
        assert_eq!(part.condition, Some(975));
        // Another carbine takes it as worn as it came off.
        let other = fitting::with(items, &carbine(items), "feed", Some(&part)).unwrap();
        assert_eq!(other.condition, Some(1000));
        assert_eq!(other.fitted["feed"].condition, Some(975));
        // And the loose stack it is put away as is a worn module.
        let stack = items::write(&fitting::loose_for_tests(&part), items).unwrap();
        assert_eq!(items::read(&stack, items).unwrap().condition, Some(975));
        through_the_stock();
    }

    /// A melee part wears with its blade, a worn one reads for less, and a
    /// bench repair mends it, by the rules a gun's part is under.
    #[test]
    fn a_fitted_melee_part_wears_with_its_blade_and_a_bench_repair_mends_it() {
        let mut app = first_day();
        bench(&mut app);
        let items = app.world().resource::<Registries>().0.clone();
        let blade =
            crate::crafting::bought(items.items.get(&id("veyra:weapon.service_blade")).unwrap());
        let hooked = Fitted {
            item: id("veyra:module.blade_hooked_edge"),
            batch: 0,
            by: None,
            lines: BTreeMap::from([
                ("hit_posture".to_owned(), 500),
                ("recovery_time".to_owned(), 500),
            ]),
            condition: None,
        };
        let mut host = fitting::with(&items.items, &blade, "edge", Some(&hooked)).unwrap();
        let rules = items.items.wear();
        let mut carry = 0;
        assert!(accrue(rules, &mut host, &mut carry, 25_000));
        assert_eq!(host.condition, Some(975));
        assert_eq!(host.fitted["edge"].condition, Some(975));
        // Sound, the part reads as made; worn to nothing, its posture is
        // less than made and still a benefit, its tail less than made too.
        let posture = |host: &Record| {
            fitting::factors(&items.items, host)
                .iter()
                .find(|factor| factor.property == Property::HitPosture)
                .unwrap()
                .factor
        };
        let sound = fitting::with(&items.items, &blade, "edge", Some(&hooked)).unwrap();
        assert_eq!(posture(&sound), 1.08);
        let mut worn = sound.clone();
        worn.fitted.get_mut("edge").unwrap().condition = Some(0);
        assert!(
            posture(&worn) < 1.08 && posture(&worn) > 1.,
            "{}",
            posture(&worn)
        );
        // At the bench a kit mends the blade and its part.
        put_in_pack(&mut app, &kit(900, 1));
        worn.condition = Some(120);
        worn.fitted.get_mut("edge").unwrap().condition = Some(80);
        put_hand(&mut app, 1, &worn);
        let done = repair_all(&mut app, &[1]);
        assert!(
            matches!(done.as_slice(), [Ok(Repaired { to: 1000, .. })]),
            "{done:?}"
        );
        let mended = hand(&mut app, 1);
        assert_eq!(mended.condition, Some(1000));
        assert_eq!(mended.fitted["edge"].condition, None);
    }

    #[test]
    fn a_bench_repair_takes_the_poorest_kit_and_mends_everything() {
        let mut app = first_day();
        bench(&mut app);
        let items = app.world().resource::<Registries>().0.clone();
        put_in_pack(&mut app, &kit(900, 1));
        put_in_pack(&mut app, &kit(100, 1));
        let mut worn =
            fitting::with(&items.items, &carbine(&items.items), "feed", Some(&feed())).unwrap();
        worn.condition = Some(120);
        worn.fitted.get_mut("feed").unwrap().condition = Some(80);
        put_hand(&mut app, 0, &worn);
        assert_eq!(kits_in(&mut app), [100, 500, 500, 900]);
        app.update();
        let worn_loadout = player(&mut app).3.loadout().cloned();
        assert!(
            worn_loadout.as_ref().is_some_and(|loadout| loadout
                .mods
                .values()
                .flatten()
                .any(|factor| factor.property == Property::RepeatInterval)),
            "{worn_loadout:?}"
        );
        let done = repair_all(&mut app, &[0]);
        assert_eq!(
            done,
            [Ok(Repaired {
                gain: 920,
                to: 1000,
                at_bench: true
            })]
        );
        assert_eq!(kits_in(&mut app), [500, 500, 900]);
        let weapon = hand(&mut app, 0);
        assert_eq!(weapon.condition, Some(1000));
        assert_eq!(weapon.fitted["feed"].condition, None);
        app.update();
        let never =
            fitting::with(&items.items, &carbine(&items.items), "feed", Some(&feed())).unwrap();
        let pistol = Some(carbine_pistol(&items));
        let expected = loadout(&items.items, &[Some(never), pistol]);
        assert_eq!(player(&mut app).3.loadout(), Some(&expected));
        assert_ne!(worn_loadout.as_ref(), Some(&expected));
    }

    #[test]
    fn a_field_repair_takes_the_best_carried_kit_and_gains_what_the_dials_say() {
        let mut app = first_day();
        let mut worn = hand(&mut app, 0);
        worn.condition = Some(100);
        put_hand(&mut app, 0, &worn);
        put_in_pack(&mut app, &kit(900, 1));
        put_in_pack(&mut app, &kit(100, 1));
        // The store's kits are out of reach away from a workbench.
        let done = repair_all(&mut app, &[0]);
        // 400 + 600 * 900 / 1000 is 940, over what a weapon at 100 lacks.
        assert_eq!(
            done,
            [Ok(Repaired {
                gain: 900,
                to: 1000,
                at_bench: false
            })]
        );
        assert_eq!(kits_in(&mut app), [100, 500, 500]);
        // The next is the best left carried: line 100 is 460 points, and
        // the store kits stay put.
        worn.condition = Some(100);
        put_hand(&mut app, 0, &worn);
        let done = repair_all(&mut app, &[0]);
        assert_eq!(
            done,
            [Ok(Repaired {
                gain: 460,
                to: 560,
                at_bench: false
            })]
        );
        assert_eq!(kits_in(&mut app), [500, 500]);
        assert_eq!(hand(&mut app, 0).condition, Some(560));
    }

    #[test]
    fn a_repair_is_refused_for_an_empty_hand_a_sound_weapon_and_no_kit() {
        let mut app = first_day();
        bench(&mut app);
        assert_eq!(
            repair_all(&mut app, &[0]),
            [Err(RepairRefusal::NothingToRepair)]
        );
        let mut worn = hand(&mut app, 0);
        worn.condition = Some(300);
        put_hand(&mut app, 0, &worn);
        {
            let world = app.world_mut();
            let mut players = world.query_filtered::<&mut Hands, With<LocalPlayer>>();
            players.single_mut(world).unwrap().0.set(1, None);
        }
        assert_eq!(repair_all(&mut app, &[1]), [Err(RepairRefusal::EmptyHand)]);
        // Both store kits spent.
        let done = repair_all(&mut app, &[0]);
        assert!(done[0].is_ok());
        worn.condition = Some(300);
        put_hand(&mut app, 0, &worn);
        assert!(repair_all(&mut app, &[0])[0].is_ok());
        worn.condition = Some(300);
        put_hand(&mut app, 0, &worn);
        assert_eq!(repair_all(&mut app, &[0]), [Err(RepairRefusal::NoKit)]);
    }

    #[test]
    fn two_repairs_in_a_tick_take_two_kits_and_the_second_has_nothing_to_mend() {
        let mut app = first_day();
        bench(&mut app);
        let mut worn = hand(&mut app, 0);
        worn.condition = Some(300);
        put_hand(&mut app, 0, &worn);
        assert_eq!(kits_in(&mut app), [500, 500]);
        let done = repair_all(&mut app, &[0, 0]);
        assert!(done[0].is_ok());
        assert_eq!(done[1], Err(RepairRefusal::NothingToRepair));
        // One kit went, and a second request on a still-worn weapon would
        // take the second.
        assert_eq!(kits_in(&mut app), [500]);
        worn.condition = Some(300);
        put_hand(&mut app, 0, &worn);
        let mut pistol = hand(&mut app, 1);
        pistol.condition = Some(300);
        put_hand(&mut app, 1, &pistol);
        put_in_pack(&mut app, &kit(500, 1));
        let done = repair_all(&mut app, &[0, 1]);
        assert!(done.iter().all(Result::is_ok));
        assert_eq!(kits_in(&mut app), Vec::<u16>::new());
    }

    /// The fit path carries a worn module's condition through the stock.
    fn through_the_stock() {
        let mut app = first_day();
        bench(&mut app);
        let items = app.world().resource::<Registries>().0.clone();
        let mut worn = feed();
        worn.condition = Some(300);
        let loose = fitting::loose_for_tests(&worn);
        put_in_pack(&mut app, &loose);
        app.world_mut()
            .resource_mut::<FitInbox>()
            .push(FitRequest::Fit {
                hand: 0,
                mount: "feed".to_owned(),
                module: worn.clone(),
            });
        app.update();
        assert_eq!(hand(&mut app, 0).fitted["feed"].condition, Some(300));
        app.world_mut()
            .resource_mut::<FitInbox>()
            .push(FitRequest::Remove {
                hand: 0,
                mount: "feed".to_owned(),
            });
        app.update();
        let (_, pack, _, _) = player(&mut app);
        let back = pack
            .cells
            .slots()
            .iter()
            .flatten()
            .filter_map(|stack| items::read(stack, &items.items).ok())
            .any(|record| record.item == id(FEED) && record.condition == Some(300));
        assert!(back, "the module came back as worn as it went");
    }

    /// A store grade bike of condition `condition` as the household's ride.
    fn stable_bike(app: &mut App, condition: u16) {
        let registries = app.world().resource::<Registries>().0.clone();
        let vehicle = id("veyra:vehicle.courier_bike");
        let mut bike = crate::ride::store_grade_bike(&registries.items, &vehicle).unwrap();
        bike.condition = Some(condition);
        *app.world_mut().resource_mut::<Stable>() =
            Stable::with_bike(bike, &registries.items, &registries.vehicles);
    }

    fn repair_bike_now(app: &mut App) -> Result<Repaired, RepairRefusal> {
        app.world_mut()
            .resource_mut::<WearInbox>()
            .push(WearRequest::RepairBike);
        app.update();
        let mut answers: Vec<_> = app
            .world_mut()
            .resource_mut::<Messages<RepairAnswer>>()
            .drain()
            .map(|answer| answer.result)
            .collect();
        assert_eq!(answers.len(), 1);
        answers.pop().unwrap()
    }

    fn bike_condition(app: &App) -> Option<u16> {
        app.world()
            .resource::<Stable>()
            .bike
            .as_ref()
            .unwrap()
            .condition
    }

    /// The bike is mended by the same kits, the same field amount and the
    /// same bench rule as a weapon.
    #[test]
    fn a_bike_is_repaired_in_the_field_and_at_a_bench_like_a_weapon() {
        let mut app = first_day();
        stable_bike(&mut app, 100);
        put_in_pack(&mut app, &kit(900, 1));
        put_in_pack(&mut app, &kit(100, 1));
        // Away from a bench: the best carried kit, 400 + 600 * 900 / 1000.
        assert_eq!(
            repair_bike_now(&mut app),
            Ok(Repaired {
                gain: 900,
                to: 1000,
                at_bench: false
            })
        );
        assert_eq!(bike_condition(&app), Some(1000));
        assert_eq!(kits_in(&mut app), [100, 500, 500]);
        // A sound bike has nothing to mend.
        assert_eq!(
            repair_bike_now(&mut app),
            Err(RepairRefusal::NothingToRepair)
        );
        // The next best is line 100: 460 points.
        stable_bike(&mut app, 100);
        assert_eq!(
            repair_bike_now(&mut app),
            Ok(Repaired {
                gain: 460,
                to: 560,
                at_bench: false
            })
        );
        assert_eq!(kits_in(&mut app), [500, 500]);
        // At a bench the poorest kit mends all of it.
        bench(&mut app);
        stable_bike(&mut app, 300);
        assert_eq!(
            repair_bike_now(&mut app),
            Ok(Repaired {
                gain: 700,
                to: 1000,
                at_bench: true
            })
        );
        assert_eq!(kits_in(&mut app), [500]);
        assert_eq!(bike_condition(&app), Some(1000));
    }

    /// No bike and no kit are refused, and the weapon in hand is not the
    /// bike.
    #[test]
    fn a_bike_repair_is_refused_without_a_bike_or_a_kit() {
        let mut app = first_day();
        assert_eq!(repair_bike_now(&mut app), Err(RepairRefusal::EmptyHand));
        stable_bike(&mut app, 100);
        // The store's kits are out of reach away from a bench; empty the pack.
        {
            let world = app.world_mut();
            let mut players = world.query_filtered::<&mut Pack, With<LocalPlayer>>();
            let mut pack = players.single_mut(world).unwrap();
            for cell in 0..pack.cells.len() {
                pack.cells.set(cell, None);
            }
            pack.cells.clear_changed();
        }
        assert_eq!(repair_bike_now(&mut app), Err(RepairRefusal::NoKit));
        assert_eq!(bike_condition(&app), Some(100), "nothing moved");
    }

    /// The player's sheet: the boxes of `ids` and nothing else.
    fn learn(app: &mut App, ids: &[&str]) {
        let (entity, ..) = player(app);
        app.world_mut().entity_mut(entity).insert(CharacterSheet {
            learned: ids.iter().map(|id| (*id).to_owned()).collect(),
            ..CharacterSheet::default()
        });
    }

    /// Repair 4 makes a shot cost a fifth less of the weapon in the bearer's
    /// hand; anyone without it, and an actor with no sheet, pay the dials.
    #[test]
    fn the_bearers_boxes_scale_what_a_shot_costs() {
        let spent = |boxes: Option<&[&str]>| {
            let mut app = first_day();
            if let Some(boxes) = boxes {
                learn(&mut app, boxes);
            }
            shoot(&mut app, 100);
            1000 - hand(&mut app, 0).condition.unwrap_or(1000)
        };
        let plain = spent(None);
        assert_eq!(spent(Some(&[])), plain, "a sheet with none reads the dials");
        assert_eq!(spent(Some(&["fabricator.novice"])), plain);
        let sparing = spent(Some(&["fabricator.repair.4"]));
        // A hundred shots of two points at the store grade, a fifth less,
        // in steps of the record's own.
        assert_eq!(plain, 200);
        assert_eq!(sparing, 150, "1.6 points a shot is 160, stepped by 25");
    }

    /// A field repair mends what the bearer's kit boxes say, and a workbench
    /// repair with the Repair 2 box takes no kit and is not refused for want
    /// of one.
    #[test]
    fn repair_boxes_scale_the_field_amount_and_a_bench_needs_no_kit() {
        // Repair 1 and 3: 400 + 600 * 500 / 1000 is 700, and a fifth and a
        // third more is 1092, held to a whole item.
        let mut app = first_day();
        learn(&mut app, &["fabricator.repair.1"]);
        let mut worn = hand(&mut app, 0);
        worn.condition = Some(100);
        put_hand(&mut app, 0, &worn);
        put_in_pack(&mut app, &kit(500, 1));
        assert_eq!(
            repair_all(&mut app, &[0]),
            [Ok(Repaired {
                gain: 840,
                to: 940,
                at_bench: false
            })],
            "700 and a fifth"
        );
        // No kit anywhere, a workbench and the box: mended, nothing spent.
        let mut app = first_day();
        bench(&mut app);
        let before = kits_in(&mut app);
        let mut worn = hand(&mut app, 0);
        worn.condition = Some(300);
        put_hand(&mut app, 0, &worn);
        learn(&mut app, &["fabricator.repair.2"]);
        let done = repair_all(&mut app, &[0]);
        assert_eq!(
            done,
            [Ok(Repaired {
                gain: 700,
                to: 1000,
                at_bench: true
            })]
        );
        assert_eq!(kits_in(&mut app), before, "no kit was used");
        // The same member away from a bench still needs one.
        let mut app = first_day();
        learn(&mut app, &["fabricator.repair.2"]);
        let mut worn = hand(&mut app, 0);
        worn.condition = Some(300);
        put_hand(&mut app, 0, &worn);
        assert_eq!(repair_all(&mut app, &[0]), [Err(RepairRefusal::NoKit)]);
        // And without the box the bench still takes the poorest kit.
        let mut app = first_day();
        bench(&mut app);
        learn(&mut app, &["fabricator.novice"]);
        let before = kits_in(&mut app);
        let mut worn = hand(&mut app, 0);
        worn.condition = Some(300);
        put_hand(&mut app, 0, &worn);
        assert!(repair_all(&mut app, &[0])[0].is_ok());
        assert_eq!(kits_in(&mut app).len() + 1, before.len());
    }
}
