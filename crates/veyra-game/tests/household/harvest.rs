//! Tests of the field harvesters as the household keeps them: planted by
//! the flag, an hour of played time, saved, read back, collected and picked
//! up, and left alone in another world.

use bevy::ecs::message::Messages;
use bevy::ecs::system::RunSystemOnce;
use strata::sim::harvest::{
    self, HarvestAnswer, HarvestDone, HarvestInbox, HarvestRefusal, HarvestRequest, Harvester,
};
use strata::sim::progression::CharacterSheet;

use super::*;
use veyra_game::household::changes;

/// The world's own deposit nearest the city, and the survey over it.
fn world(
    registries: &GameRegistries,
) -> (std::sync::Arc<strata::veyra_world::DepositField>, Deposit) {
    let world = std::sync::Arc::new(strata::worldgen::ProceduralWorld::with_params(
        strata::core::Seed(42),
        registries,
        strata::sim::world_params(),
    ));
    let city = world.sites()[0].centre;
    let field = std::sync::Arc::new(strata::veyra_world::DepositField::new(
        world,
        strata::core::Seed(42),
        std::sync::Arc::new(registries.clone()),
    ));
    let nearest = field.near(city[0], city[1], 2_000.0)[0].clone();
    (field, nearest)
}

/// A machine on `deposit`, empty.
#[allow(clippy::unwrap_used, reason = "a literal id")]
pub(super) fn machine(id: u32, deposit: DepositId) -> Harvester {
    Harvester {
        id,
        deposit,
        at: [0.0, 0.0],
        pm: 500,
        item: Record {
            item: "veyra:station.field_harvester".parse().unwrap(),
            count: 1,
            batch: 0,
            by: None,
            lines: BTreeMap::new(),
            condition: None,
            fitted: BTreeMap::new(),
            origin: None,
        },
        by: String::new(),
        rate: 360,
        hopper: 300,
        lot: Record {
            item: "veyra:station.field_harvester".parse().unwrap(),
            count: 1,
            batch: 0,
            by: None,
            lines: BTreeMap::new(),
            condition: None,
            fitted: BTreeMap::new(),
            origin: None,
        },
        held: 0,
        carry: 0,
        full: false,
    }
}

fn file(harvester: bool, hours: Option<f64>) -> HouseholdFile {
    HouseholdFile {
        path: PathBuf::from("scratch/none.ron"),
        fresh: true,
        fitted: Vec::new(),
        in_hand: Vec::new(),
        learn: Vec::new(),
        grant: Vec::new(),
        condition: None,
        member: None,
        crafted_carbine: None,
        blanks: None,
        give: Vec::new(),
        bike: false,
        bike_condition: None,
        purse: None,
        saddle_stock: false,
        harvester,
        harvest_hours: hours,
        open_harvester: false,
        quest_stage: None,
    }
}

/// The flags plant a machine on the richest found ground and pass an hour of
/// played time; the household copies it, the file holds it and reads it
/// back; in the same world it is stood, collected and picked up by the
/// sim's own requests, and in another world it stays in the file, unstood
/// and unadvanced.
#[test]
#[allow(clippy::unwrap_used, reason = "the scenario owns its world")]
#[allow(
    clippy::too_many_lines,
    reason = "one machine's life, from the flag to the pick-up"
)]
fn a_machine_is_planted_runs_an_hour_is_saved_collected_and_picked_up() {
    let registries = shipped();
    let (field, deposit) = world(&registries);
    let household = Household::starting(&registries, &Grants::default());
    let (_, sheet, name, _, _, hands) = household.stats(&registries);
    // An empty pack: the arrival's has room for two hundred units only.
    let pack = Pack::from_saved(&SavedPack::default(), &registries.items).unwrap();
    let here = SurveyedIn {
        seed: 42,
        cell_m: field.set().rules().lattice.cell_m,
    };
    let (surveyed, live) = survey_record(&household, Some(here), |_| None);
    let mut world = World::new();
    world.insert_resource(strata::sim::Registries(std::sync::Arc::new(
        registries.clone(),
    )));
    world.insert_resource(Survey(field));
    world.insert_resource(HouseholdStore::from_saved(&household.store, &registries.items).unwrap());
    world.insert_resource(surveyed);
    world.insert_resource(live.unwrap());
    world.insert_resource(file(true, Some(1.0)));
    world.insert_resource(Harvesters::default());
    world.insert_resource(household);
    let player = world
        .spawn((
            LocalPlayer,
            WorldPosition::new(deposit.at[0], 0.0, deposit.at[1]),
            sheet,
            name,
            pack,
            hands,
        ))
        .id();
    world.run_system_once(harvester_at_start).unwrap();
    // An hour at the centre: the rate times the concentration, capped at
    // the hopper, and the deposit is found.
    let one = world.resource::<Harvesters>().list[0].clone();
    assert_eq!(one.by, world.resource::<Household>().active().name);
    assert_eq!(one.pm, deposit.peak);
    let expected = (1200 * u32::from(deposit.peak) / 1000).min(1000);
    assert_eq!(one.held, expected);
    assert!(world.resource::<Surveyed>().found.contains_key(&deposit.id));
    // The household takes it and stamps the world, and a second pass moves
    // nothing.
    world.run_system_once(track_stock).unwrap();
    let saved = world.resource::<Household>().clone();
    assert_eq!(saved.harvesters, *world.resource::<Harvesters>());
    assert_eq!(saved.surveyed_in, Some(here));
    // The file holds it and reads back whole.
    let text = ron::ser::to_string_pretty(&saved, ron::ser::PrettyConfig::default()).unwrap();
    let (read, _, _) = Household::read(&text, &registries, &Grants::default).unwrap();
    assert_eq!(read, saved);
    // This world stands it; another keeps it in the file and stands none.
    assert_eq!(stood_harvesters(&read, true), read.harvesters);
    assert_eq!(stood_harvesters(&read, false), Harvesters::default());
    let (elsewhere, live) = survey_record(&read, Some(SurveyedIn { seed: 7, ..here }), |_| None);
    assert!(live.is_none() && elsewhere == Surveyed::default());

    // Read back, collected and picked up by the sim's requests.
    world.insert_resource(stood_harvesters(&read, true));
    world.init_resource::<HarvestInbox>();
    world.init_resource::<Messages<HarvestAnswer>>();
    world.init_resource::<Messages<strata::sim::items::StockAnswer>>();
    world.init_resource::<Messages<strata::sim::progression::XpGained>>();
    world.init_resource::<strata::sim::items::OpenStock>();
    let ask = |world: &mut World, request: HarvestRequest| {
        world.resource_mut::<HarvestInbox>().push(request);
        world
            .run_system_once(strata::sim::request::address::<HarvestRequest>)
            .unwrap();
        world.run_system_once(harvest::consume).unwrap();
        world
            .resource_mut::<Messages<HarvestAnswer>>()
            .drain()
            .next()
            .unwrap()
            .outcome
    };
    let id = one.id;
    assert_eq!(
        ask(&mut world, HarvestRequest::PickUp { id }),
        Err(HarvestRefusal::NotEmpty)
    );
    // A bare pack holds ten stacks: what fits comes, the rest stays, and the
    // machine cannot be picked up until it is empty.
    let Ok(HarvestDone::Collected {
        units: first, left, ..
    }) = ask(&mut world, HarvestRequest::Collect { id })
    else {
        panic!("collected");
    };
    assert_eq!(first + left, expected);
    // A full hopper is more than a pack takes at once: empty the pack and
    // collect again until nothing is left, the last lot being what stays.
    let (mut held, mut left, mut rounds) = (first, left, 0);
    while left > 0 {
        assert_eq!(
            ask(&mut world, HarvestRequest::PickUp { id }),
            Err(HarvestRefusal::NotEmpty)
        );
        let mut pack = world.get_mut::<Pack>(player).unwrap();
        for cell in 0..pack.cells.len() {
            pack.cells.set(cell, None);
        }
        let Ok(HarvestDone::Collected {
            units: more,
            left: rest,
            ..
        }) = ask(&mut world, HarvestRequest::Collect { id })
        else {
            panic!("collected again");
        };
        assert_eq!(more + rest, left);
        (held, left, rounds) = (more, rest, rounds + 1);
        assert!(rounds < 20, "a pack that takes nothing");
    }
    let pack = world.get::<Pack>(player).unwrap().clone();
    let units: u32 = pack
        .cells
        .slots()
        .iter()
        .flatten()
        .chain(
            pack.bags
                .iter()
                .flat_map(|bag| bag.slots().iter().flatten()),
        )
        .filter_map(|stack| strata::sim::items::read(stack, &registries.items).ok())
        .filter(|record| {
            record
                .origin
                .as_ref()
                .is_some_and(|o| o.deposit == deposit.id.0)
        })
        .map(|record| record.count)
        .sum();
    assert_eq!(units, held);
    assert_eq!(
        ask(&mut world, HarvestRequest::PickUp { id }),
        Ok(HarvestDone::PickedUp { id })
    );
    assert!(world.resource::<Harvesters>().list.is_empty());
    // And the household follows the sim to an empty list.
    world.run_system_once(track_stock).unwrap();
    assert!(world.resource::<Household>().harvesters.list.is_empty());
    let _: &CharacterSheet = world.get(player).unwrap();
}

/// In another world the machines are neither copied from the sim nor
/// changed in the file.
#[test]
#[allow(clippy::unwrap_used, reason = "the scenario owns its world")]
fn machines_of_another_world_are_kept_and_not_advanced() {
    let registries = shipped();
    let mut household = Household::starting(&registries, &Grants::default());
    let here = SurveyedIn {
        seed: 42,
        cell_m: 800.0,
    };
    household.surveyed_in = Some(here);
    household.harvesters = Harvesters {
        list: vec![Harvester {
            id: 0,
            deposit: DepositId([3, -2]),
            at: [1.0, 2.0],
            pm: 500,
            item: Record {
                item: "veyra:station.field_harvester".parse().unwrap(),
                count: 1,
                batch: 0,
                by: None,
                lines: BTreeMap::new(),
                condition: None,
                fitted: BTreeMap::new(),
                origin: None,
            },
            by: "Tovan Reyes".to_owned(),
            rate: 360,
            hopper: 300,
            lot: Record {
                item: "veyra:station.field_harvester".parse().unwrap(),
                count: 1,
                batch: 0,
                by: None,
                lines: BTreeMap::new(),
                condition: None,
                fitted: BTreeMap::new(),
                origin: None,
            },
            held: 12,
            carry: 77,
            full: false,
        }],
        next: 1,
    };
    let other = Some(SurveyedIn { seed: 7, ..here });
    let (surveyed, live) = survey_record(&household, other, |_| None);
    assert!(live.is_none());
    // The sim is handed none, so none is advanced, and what it does with
    // its own empty list reaches the file nowhere.
    let handed = stood_harvesters(&household, live.is_some());
    assert!(handed.list.is_empty());
    let mut world = World::new();
    world.insert_resource(strata::sim::Registries(std::sync::Arc::new(
        registries.clone(),
    )));
    world.insert_resource(HouseholdStore::from_saved(&household.store, &registries.items).unwrap());
    world.insert_resource(surveyed);
    world.insert_resource(handed);
    world.insert_resource(household.clone());
    world.run_system_once(track_stock).unwrap();
    world.resource_mut::<Harvesters>().next = 9;
    world.run_system_once(track_stock).unwrap();
    assert_eq!(*world.resource::<Household>(), household);
    assert_eq!(household.harvesters.list[0].held, 12);
}

/// The real chain, with no flag: a machine carried in the pack is planted by
/// the sim's request (so the pack loses it), and an empty one picked up
/// leaves the pack with exactly the one it began with.
#[test]
#[allow(clippy::unwrap_used, reason = "the scenario owns its world")]
#[allow(
    clippy::too_many_lines,
    reason = "one world set up, then the two requests"
)]
fn a_carried_machine_is_planted_and_picked_up_by_request() {
    use strata::registry::item::{HARVEST_HOPPER, HARVEST_RATE};
    let registries = shipped();
    let (field, deposit) = world(&registries);
    let sheet = {
        let member = registries
            .sheets
            .household
            .as_ref()
            .unwrap()
            .recruits
            .iter()
            .find(|member| member.name == "Ilse Marrow")
            .unwrap();
        CharacterSheet::authored(&member.sheet)
    };
    let record = Record {
        item: "veyra:station.field_harvester".parse().unwrap(),
        count: 1,
        batch: 0,
        by: None,
        lines: BTreeMap::from([
            (HARVEST_RATE.to_owned(), 500),
            (HARVEST_HOPPER.to_owned(), 500),
        ]),
        condition: None,
        fitted: BTreeMap::new(),
        origin: None,
    };
    let mut pack = Pack::from_saved(&SavedPack::default(), &registries.items).unwrap();
    let stack = strata::sim::items::write(&record, &registries.items).unwrap();
    assert!(strata::sim::items::place_in(&registries.items, &mut pack.cells, stack).is_none());
    let carried = |pack: &Pack| {
        pack.cells
            .slots()
            .iter()
            .flatten()
            .filter_map(|stack| strata::sim::items::read(stack, &registries.items).ok())
            .filter(|each| each.item == record.item)
            .count()
    };
    let mut world = World::new();
    world.insert_resource(strata::sim::Registries(std::sync::Arc::new(
        registries.clone(),
    )));
    world.insert_resource(Survey(field));
    world.insert_resource(HouseholdStore::from_saved(&[], &registries.items).unwrap());
    let mut surveyed = Surveyed::default();
    surveyed.found.insert(
        deposit.id,
        strata::sim::deposits::Found {
            reading: strata::registry::Reading::Exact,
            sampled: true,
            best_pm: deposit.peak,
            best_at: deposit.at,
        },
    );
    world.insert_resource(surveyed);
    world.init_resource::<Harvesters>();
    world.init_resource::<HarvestInbox>();
    world.init_resource::<Messages<HarvestAnswer>>();
    world.init_resource::<Messages<strata::sim::items::StockAnswer>>();
    world.init_resource::<Messages<strata::sim::progression::XpGained>>();
    world.init_resource::<strata::sim::items::OpenStock>();
    let player = world
        .spawn((
            LocalPlayer,
            WorldPosition::new(deposit.at[0], 0.0, deposit.at[1]),
            sheet,
            Name::new("Ilse Marrow"),
            pack,
            Hands::default(),
        ))
        .id();
    let ask = |world: &mut World, request: HarvestRequest| {
        world.resource_mut::<HarvestInbox>().push(request);
        world
            .run_system_once(strata::sim::request::address::<HarvestRequest>)
            .unwrap();
        world.run_system_once(harvest::consume).unwrap();
        world
            .resource_mut::<Messages<HarvestAnswer>>()
            .drain()
            .next()
            .unwrap()
            .outcome
    };
    assert_eq!(carried(world.get::<Pack>(player).unwrap()), 1);
    let Ok(HarvestDone::Planted { id, .. }) = ask(&mut world, HarvestRequest::Plant) else {
        panic!("planted");
    };
    assert_eq!(
        carried(world.get::<Pack>(player).unwrap()),
        0,
        "left the pack"
    );
    assert_eq!(world.resource::<Harvesters>().list.len(), 1);
    assert_eq!(
        ask(&mut world, HarvestRequest::PickUp { id }),
        Ok(HarvestDone::PickedUp { id })
    );
    assert_eq!(
        carried(world.get::<Pack>(player).unwrap()),
        1,
        "exactly one"
    );
    assert!(world.resource::<Harvesters>().list.is_empty());
}

/// A prop appears for a machine planted and goes for one picked up: what
/// `stand_harvesters` spawns and despawns is this difference.
#[test]
fn props_follow_the_list() {
    let registries = shipped();
    let (_, deposit) = world(&registries);
    let machine = |id| machine(id, deposit.id);
    let mut list = Harvesters::default();
    assert_eq!(changes(&list, &[]), (vec![], vec![]));
    list.list.push(machine(3));
    assert_eq!(changes(&list, &[]), (vec![], vec![3]), "planted");
    assert_eq!(changes(&list, &[3]), (vec![], vec![]), "standing");
    list.list.clear();
    assert_eq!(changes(&list, &[3]), (vec![3], vec![]), "picked up");
}
