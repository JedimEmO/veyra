//! Tests of the fabricator's run as the household file keeps it, and of who
//! may be switched to while a run is worked.

use super::*;
use crate::household::bench::Out;

/// A household whose fabricator is running, as the sim leaves it and
/// [`track_stock`] copies it: a core made by the arrival and recorded
/// as blueprint 2, and a batch of three started off it from the first
/// lots with `worker` on it, `ticks` of play ago.
struct Running {
    household: Household,
    store: HouseholdStore,
    prints: Blueprints,
    state: FactoryState,
    clock: factory::FactoryClock,
}

#[allow(clippy::unwrap_used, reason = "the scenario owns its world")]
fn running(registries: &GameRegistries, attended: bool, ticks: u32) -> Running {
    use bevy::ecs::system::RunSystemOnce;
    let items = &registries.items;
    let grants = Grants {
        lots: vec![
            lot_of(registries, "strata:evaporite_salt", [3, -2], 160),
            lot_of(registries, "strata:coastal_brine", [5, 5], 160),
        ],
        purse: 2500,
        short: 0,
        deposits: Vec::new(),
    };
    let mut household = Household::starting(registries, &grants);
    join_all(&mut household);
    let worker = attended.then(|| household.characters[1].name.clone());
    let benched = Benched(
        household.characters[1..]
            .iter()
            .map(|c| c.name.clone())
            .collect(),
    );
    let maker = household.active().name.clone();
    let core = SCRIPT_CORE.parse().unwrap();
    let schematic = registries.schematics.get(&core).unwrap();
    let (.., mut pack, mut hands) = household.stats(registries);
    let mut store = HouseholdStore::from_saved(&household.store, items).unwrap();
    let mut bench = household.crafting.clone();
    let mut prints = Blueprints::default();
    let mut state = FactoryState::default();
    let mut clock = factory::FactoryClock::default();
    let picks = crafting::best_stock(registries, &pack, Some(&store), schematic);
    let mut stock = Stock {
        saddle: None,
        pack: &mut pack,
        hands: &mut hands,
        store: Some(&mut store),
    };
    let sheet = household.active().sheet.clone();
    crafting::assemble(
        registries, &mut stock, &mut bench, &core, &picks, &maker, &sheet,
    )
    .unwrap();
    let made = crafting::finish(registries, &mut stock, &mut bench).unwrap();
    assert_eq!(made.batch, 1);
    let (number, _) = blueprint::record(
        registries,
        &mut stock,
        &mut bench,
        &mut prints,
        1,
        1000,
        &maker,
    )
    .unwrap();
    assert_eq!(number, 2);
    let feeds = factory::cheapest(
        registries,
        stock.pack,
        stock.store.as_deref(),
        &prints.0[&2],
        3,
    );
    let started = factory::start(
        registries,
        &mut stock,
        &prints,
        &mut bench,
        &mut state,
        &benched,
        (2, 3),
        &feeds,
        (worker.as_deref(), None),
    );
    assert_eq!(started, Ok((3, if attended { 36 } else { 72 })));
    for _ in 0..ticks {
        factory::advance(registries, &mut prints, &mut state, &mut clock, &mut store);
    }
    // The sim's state is copied to the household, which the file holds.
    let mut world = World::new();
    world.insert_resource(strata::sim::Registries(std::sync::Arc::new(
        registries.clone(),
    )));
    world.insert_resource(store.clone());
    world.insert_resource(bench);
    world.insert_resource(prints.clone());
    world.insert_resource(state.clone());
    world.spawn((LocalPlayer, pack, hands));
    world.insert_resource(household);
    world.run_system_once(track_stock).unwrap();
    let household = world.resource::<Household>().clone();
    // Nothing moved, nothing copied: a second pass leaves it as it is.
    world.run_system_once(track_stock).unwrap();
    assert_eq!(*world.resource::<Household>(), household);
    Running {
        household,
        store,
        prints,
        state,
        clock,
    }
}

/// The units of the lot from the deposit at `cell` in a saved store.
fn lot_units(household: &Household, cell: [i32; 2]) -> u32 {
    household
        .store
        .iter()
        .flatten()
        .filter(|record| record.origin.as_ref().is_some_and(|o| o.deposit == cell))
        .map(|record| record.count)
        .sum()
}

/// The stacks of cell cores in a saved store, as count and batch.
fn stored_cores(store: &[Option<Record>]) -> Vec<(u32, u32)> {
    store
        .iter()
        .flatten()
        .filter(|record| record.item.to_string() == "veyra:component.cell_core")
        .map(|record| (record.count, record.batch))
        .collect()
}

/// A run under way is in the file and on the fabricator at the next
/// start: what was committed is in the run and not in the store, what
/// was made is in the store, the blueprint is down by it, and from
/// there it runs to its end. The unit under way begins again, since
/// the clock within a unit is not saved.
#[test]
#[allow(clippy::unwrap_used, reason = "the scenario owns its world")]
#[allow(
    clippy::too_many_lines,
    reason = "one run, from the fabricator to the file and back to the fabricator"
)]
fn a_run_round_trips_the_save() {
    let registries = shipped();
    let items = &registries.items;
    let dir = tempfile::TempDir::new().unwrap();
    let path = dir.path().join("household.ron");
    // One unit made and a quarter of the next.
    let run = running(&registries, false, 600);
    let open = run.household.clone();
    assert_eq!(open.blueprints, run.prints);
    assert_eq!(open.factory, run.state);
    let job = open.factory.job.as_ref().unwrap();
    assert_eq!((job.blueprint, job.batch, job.size, job.made), (2, 3, 3, 1));
    assert_eq!((job.worker.clone(), job.paused), (None, None));
    let hopper: Vec<(&str, u32)> = job
        .inputs
        .iter()
        .map(|input| (input.slot.as_str(), input.left))
        .collect();
    assert_eq!(hopper, [("salt", 40), ("brine", 60)]);
    // 160 less the prototype's and the three's: committed at the start.
    assert_eq!(
        (lot_units(&open, [3, -2]), lot_units(&open, [5, 5])),
        (80, 40)
    );
    assert_eq!(stored_cores(&open.store), [(1, 3)]);
    assert_eq!(
        (open.blueprints.0[&2].runs, open.blueprints.0[&2].runs_left),
        (100, 99)
    );
    assert_eq!(open.crafting.batches, 3);
    assert_eq!(open.crafting.made[&3].blueprint, Some(2));
    assert_eq!(open.crafting.made[&1].recorded, Some(2));
    open.save(&path).unwrap();
    let written = std::fs::read_to_string(&path).unwrap();
    assert!(written.contains("blueprints") && written.contains("factory"));

    // The next start reads it back as it was, with nothing to mend.
    let read = Household::load_or_start(&path, false, &registries, &Grants::default);
    assert!(!read.first_day && !read.hold);
    assert_eq!(read.household, open);
    assert!(!beside(&path, ".mended.bak").exists());
    assert!(!beside(&path, ".v4.bak").exists());

    // And it runs on from there, with the stock as it was read: a unit
    // is 480 ticks from the start again, and the third ends it.
    let mut store = HouseholdStore::from_saved(&read.household.store, items).unwrap();
    let mut prints = read.household.blueprints.clone();
    let mut state = read.household.factory.clone();
    let mut clock = factory::FactoryClock::default();
    let mut tick = |ticks: u32| {
        (0..ticks)
            .filter_map(|_| {
                factory::advance(&registries, &mut prints, &mut state, &mut clock, &mut store)
            })
            .collect::<Vec<_>>()
    };
    assert_eq!(tick(479), []);
    assert_eq!(tick(1), []);
    let told = tick(480);
    assert_eq!(
        told,
        [FactoryNotice::Finished {
            item: "veyra:component.cell_core".parse().unwrap(),
            made: 3,
            batch: 3,
            spent: false
        }]
    );
    assert_eq!(state.job, None);
    assert_eq!(stored_cores(&store.saved(items).unwrap()), [(3, 3)]);
    assert_eq!(prints.0[&2].runs_left, 97);
    let (.., pack, _) = read.household.stats(&registries);
    assert_eq!(
        blueprint::held(&registries, &pack, Some(&store), &prints),
        [(2, true)]
    );

    // A run whose blueprint the file no longer has is dropped with
    // what it held put back in the store, said, and the file as it was
    // kept aside; the household is loaded all the same.
    let mut orphan = open.clone();
    orphan.blueprints.0.clear();
    let off = dir.path().join("orphan.ron");
    orphan.save(&off).unwrap();
    let written = std::fs::read_to_string(&off).unwrap();
    let mended = Household::load_or_start(&off, false, &registries, &Grants::default);
    assert!(!mended.first_day && !mended.hold);
    assert_eq!(mended.household.factory, FactoryState::default());
    assert_eq!(
        (
            lot_units(&mended.household, [3, -2]),
            lot_units(&mended.household, [5, 5])
        ),
        (120, 100)
    );
    assert_eq!(mended.household.characters, open.characters);
    assert_eq!(
        std::fs::read_to_string(beside(&off, ".mended.bak")).unwrap(),
        written
    );
    let (_, _, said) = Household::read(&written, &registries, &Grants::default).unwrap();
    assert_eq!(said.len(), 1, "{said:?}");
    assert!(said[0].contains("blueprint 2"), "{said:?}");
    // The sim's own copy, which the file was written from, is where
    // the file left it.
    assert_eq!(run.store.saved(items).unwrap(), open.store);
    assert_eq!(run.clock.0, 120 * 500, "a quarter of a unit, unsaved");
}

/// A benched member working the fabricator cannot be switched to: the
/// switch is refused with their name and the count so far, and nothing
/// of the household moves. Anyone else can be, and when the run ends
/// the worker is free. The sim is told who is on the bench: everyone
/// but whoever is out.
#[test]
#[allow(clippy::unwrap_used, reason = "the scenario owns its world")]
fn a_working_member_cannot_be_switched_to() {
    use bevy::ecs::system::RunSystemOnce;
    let registries = shipped();
    // A unit made at the worked rate, 240 ticks, and some of the next.
    let mut run = running(&registries, true, 300);
    let mut household = run.household.clone();
    assert_eq!(household.characters.len(), BENCH);
    let worker = household.characters[1].name.clone();
    let job = household.factory.job.clone().unwrap();
    assert_eq!(
        (job.worker.as_deref(), job.made),
        (Some(worker.as_str()), 1)
    );

    let (mut actor, mut sheet, mut name, mut tint, mut pack, mut hands) =
        household.stats(&registries);
    let mut bar = household.bar();
    let mut input = CombatIntent::default();
    macro_rules! player {
        () => {
            Out {
                position: WorldPos::new(1., 2., 3.),
                actor: &mut actor,
                sheet: &mut sheet,
                input: &mut input,
                name: &mut name,
                tint: &mut tint,
                pack: &mut pack,
                hands: &mut hands,
                bar: &mut bar,
            }
        };
    }
    let before = household.clone();
    let refused = swap_to(&mut household, 1, &registries, None, Some(&job), player!());
    assert_eq!(
        refused,
        Err(format!(
            "{worker} is working the fabricator: 1 of 3 made. Stop the run to switch."
        ))
    );
    assert_eq!(household, before, "nobody moved");
    assert_eq!(name.as_str(), "Frontier ranger");
    // Somebody else on the bench comes out as ever, and the worker is
    // still not to be had from there.
    assert_eq!(
        swap_to(&mut household, 2, &registries, None, Some(&job), player!()),
        Ok(())
    );
    assert_eq!(household.active, 2);
    assert!(swap_to(&mut household, 1, &registries, None, Some(&job), player!()).is_err());

    // The sim's list of who may work a run is the bench less who is
    // out, and follows a switch.
    let mut world = World::new();
    world.init_resource::<Benched>();
    world.insert_resource(household.clone());
    world.run_system_once(publish_benched).unwrap();
    let names = |household: &Household, out: usize| -> Vec<String> {
        household
            .characters
            .iter()
            .enumerate()
            .filter(|(index, _)| *index != out)
            .map(|(_, c)| c.name.clone())
            .collect()
    };
    assert_eq!(world.resource::<Benched>().0, names(&household, 2));
    assert!(world.resource::<Benched>().0.contains(&worker));

    // The run ends: 180 more ticks for the second unit and 240 for the
    // third. The fabricator is free and so is the worker.
    let mut told = Vec::new();
    for _ in 0..420 {
        told.extend(factory::advance(
            &registries,
            &mut run.prints,
            &mut run.state,
            &mut run.clock,
            &mut run.store,
        ));
    }
    assert!(
        matches!(told[..], [FactoryNotice::Finished { made: 3, .. }]),
        "{told:?}"
    );
    assert_eq!(run.state.job, None);
    assert_eq!(
        swap_to(
            &mut household,
            1,
            &registries,
            None,
            run.state.job.as_ref(),
            player!()
        ),
        Ok(())
    );
    assert_eq!(household.active, 1);
    assert_eq!(name.as_str(), worker);
    world.insert_resource(household.clone());
    world.run_system_once(publish_benched).unwrap();
    assert_eq!(world.resource::<Benched>().0, names(&household, 1));
}

/// Only a benched member works a run. A file in which the worker is
/// the one out, or is nobody on the bench, is loaded with nobody on
/// the run, said, and kept aside as it was; one whose worker is on the
/// bench is left alone.
#[test]
#[allow(clippy::unwrap_used, reason = "the scenario owns its files")]
fn a_worker_who_is_out_is_taken_off_at_load() {
    let registries = shipped();
    let dir = tempfile::TempDir::new().unwrap();
    let run = running(&registries, true, 300);
    let sound = run.household;
    let worker = sound.characters[1].name.clone();
    let load = |household: &Household, name: &str| {
        let path = dir.path().join(name);
        household.save(&path).unwrap();
        let written = std::fs::read_to_string(&path).unwrap();
        let read = Household::load_or_start(&path, false, &registries, &Grants::default);
        assert!(!read.first_day && !read.hold, "{name}");
        let (_, _, said) = Household::read(&written, &registries, &Grants::default).unwrap();
        let kept = beside(&path, ".mended.bak");
        let kept = kept
            .exists()
            .then(|| std::fs::read_to_string(&kept).unwrap() == written);
        (read.household, said, kept)
    };
    let worked_by = |household: &Household| {
        let job = household.factory.job.as_ref().unwrap();
        (job.worker.clone(), job.made, job.size)
    };

    let (read, said, kept) = load(&sound, "sound.ron");
    assert_eq!(read, sound);
    assert_eq!(worked_by(&read), (Some(worker.clone()), 1, 3));
    assert_eq!((said, kept), (Vec::new(), None));

    let mut out = sound.clone();
    out.active = 1;
    let (read, said, kept) = load(&out, "out.ron");
    assert_eq!(worked_by(&read), (None, 1, 3), "the run goes on unattended");
    assert_eq!(said.len(), 1, "{said:?}");
    assert!(
        said[0].contains(&worker) && said[0].contains("is out"),
        "{said:?}"
    );
    assert_eq!(kept, Some(true), "the file as it was is kept");
    // Everything else is as it was written.
    let mut expected = out.clone();
    expected.factory.job.as_mut().unwrap().worker = None;
    assert_eq!(read, expected);

    let mut gone = sound.clone();
    gone.factory.job.as_mut().unwrap().worker = Some("A stranger".to_owned());
    let (read, said, kept) = load(&gone, "gone.ron");
    assert_eq!(worked_by(&read), (None, 1, 3));
    assert!(said[0].contains("not on the bench"), "{said:?}");
    assert_eq!(kept, Some(true));
}
