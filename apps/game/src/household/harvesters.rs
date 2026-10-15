//! The field harvesters in the world (ticket 0009, step 6): each planted
//! machine stands as a prop with an `Interaction`, appears when planted and
//! goes when picked up, and the flags that plant or advance one for a still.
//! The machines, their clock and their rules are `strata_sim::harvest`'s;
//! this is only where they stand and what they look like.

use strata::client::{DustPuff, EmitterSound, LoopEmitter};
use strata::sim::harvest::{self, Harvester, Harvesters};

use super::*;

/// The machines' systems: the props and the flags that plant or advance one
/// run in every mode, once the player is in the world; the lamp and the
/// screen a flag opens are the window's.
pub(super) fn install_harvesters(app: &mut App, cli: &crate::Cli) {
    app.add_systems(
        Update,
        (harvester_at_start, stand_harvesters)
            .chain()
            .run_if(resource_exists::<Household>)
            .run_if(in_state(AppState::InWorld)),
    );
    if !cli.headless {
        app.add_systems(
            Update,
            (
                light_harvesters.after(dress_props),
                (hum_harvesters, pump_harvesters).after(dress_props),
                open_harvester_for_shot.run_if(strata::client::ui::game_input_allowed),
            )
                .run_if(resource_exists::<PropArt>)
                .run_if(in_state(AppState::InWorld)),
        );
    }
}

/// What the sim is handed of the household's machines: all of them in the
/// world the survey record is live in, none in another, where they stay in
/// the file, are not stood and are not advanced.
pub(super) fn stood_harvesters(household: &Household, live: bool) -> Harvesters {
    if live {
        household.harvesters.clone()
    } else {
        Harvesters::default()
    }
}

/// A placed harvester's number, for the sim's list to find it by.
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct HarvesterProp(pub(super) u32);

/// Stand a prop for every planted machine and take away those no longer in
/// the list, on the frame the list changes (and again until the ground under
/// a new one has streamed). The sim side (position, interaction) is the same
/// in every mode; the art is the window's ([`dress_props`]).
#[allow(
    clippy::too_many_arguments,
    reason = "the household's machines, the props standing and the ground the new ones stand on"
)]
pub(super) fn stand_harvesters(
    mut commands: Commands,
    machines: Option<Res<Harvesters>>,
    registries: Res<strata::sim::Registries>,
    standing: Query<(Entity, &HarvesterProp)>,
    source: Res<WorldSourceRes>,
    origin: Res<RenderOrigin>,
    spatial: SpatialQuery,
    actors: Query<Entity, With<Combatant>>,
    mut waiting: Local<bool>,
) {
    let Some(machines) = machines else {
        return;
    };
    if !machines.is_changed() && !*waiting {
        return;
    }
    *waiting = false;
    let on: Vec<u32> = standing.iter().map(|(_, stood)| stood.0).collect();
    let (gone, _) = changes(&machines, &on);
    for (entity, stood) in &standing {
        if gone.contains(&stood.0) {
            commands.entity(entity).despawn();
        }
    }
    let reach = registries.0.resources.harvest().reach_m;
    let filter = SpatialQueryFilter::from_excluded_entities(actors.iter());
    #[allow(clippy::cast_possible_truncation, reason = "a reach of a few metres")]
    let radius = reach as f32;
    let (_, new) = changes(&machines, &on);
    for machine in machines.list.iter().filter(|each| new.contains(&each.id)) {
        let [x, z] = machine.at;
        let Some(ground) = ground_at(x, z, &source, &origin, &spatial, &filter) else {
            *waiting = true;
            continue;
        };
        commands.spawn((
            Name::new("Field harvester"),
            Prop::Harvester,
            HarvesterProp(machine.id),
            Interaction {
                kind: InteractionKind::Harvester { id: machine.id },
                radius,
            },
            WorldPosition(WorldPos::new(x, ground, z)),
            Transform::default(),
            Visibility::default(),
        ));
    }
}

/// Which standing props the list no longer has, and which of its machines
/// have none yet.
pub(super) fn changes(machines: &Harvesters, standing: &[u32]) -> (Vec<u32>, Vec<u32>) {
    let gone = standing
        .iter()
        .copied()
        .filter(|id| machines.get(*id).is_none())
        .collect();
    let new = machines
        .list
        .iter()
        .map(|each| each.id)
        .filter(|id| !standing.contains(id))
        .collect();
    (gone, new)
}

/// The lamp follows the machine: lit green while it runs, steady amber
/// when its hopper is full.
pub(super) fn light_harvesters(
    machines: Res<Harvesters>,
    art: Res<PropArt>,
    mut lamps: Query<(&HarvesterLamp, &mut MeshMaterial3d<StandardMaterial>)>,
    mut glows: Query<(&HarvesterGlow, &mut PointLight)>,
) {
    for (glow, mut light) in &mut glows {
        let full = machines.get(glow.0).is_some_and(Harvester::is_full);
        let wanted = STRIP_COLOURS[if full { 2 } else { 1 }];
        if light.color != wanted {
            light.color = wanted;
        }
    }
    for (lamp, mut material) in &mut lamps {
        let full = machines.get(lamp.0).is_some_and(Harvester::is_full);
        let wanted = &art.strips[if full { 2 } else { 1 }];
        if material.0 != *wanted {
            material.0 = wanted.clone();
        }
    }
}

/// Whether the machine `id` is at work: standing in the list and not full.
/// A hopper at its mark stops the machine; nothing else does.
fn running(machines: &Harvesters, id: u32) -> bool {
    machines.get(id).is_some_and(|machine| !machine.is_full())
}

/// A running harvester hums: the client's looping emitter is on its prop
/// while it works and off when it fills or is gone.
pub(super) fn hum_harvesters(
    mut commands: Commands,
    machines: Res<Harvesters>,
    props: Query<(Entity, &HarvesterProp, Has<LoopEmitter>)>,
) {
    for (entity, stood, humming) in &props {
        let works = running(&machines, stood.0);
        if works && !humming {
            commands
                .entity(entity)
                .insert(LoopEmitter(EmitterSound::HarvesterHum));
        } else if !works && humming {
            commands.entity(entity).remove::<LoopEmitter>();
        }
    }
}

/// Seconds a stroke of the plate takes, how far it rises over where it
/// rests, and how fast its swing eases in and out, per second.
const STROKE_SECS: f32 = 0.9;
const STROKE_RISE: f32 = 0.3;
const STROKE_EASE: f32 = 2.5;

/// The plate's height over its rest at `phase` (a stroke is `0..1`): the
/// top at the start, the foot at the half.
fn stroke(phase: f32) -> f32 {
    0.5 + 0.5 * (phase * std::f32::consts::TAU).cos()
}

/// The plate pumps while its machine runs, with a puff of dust at the foot of
/// each stroke, and settles to rest when it fills or goes.
pub(super) fn pump_harvesters(
    time: Res<Time>,
    machines: Res<Harvesters>,
    owners: Query<&WorldPosition>,
    mut plates: Query<(&mut HarvesterPump, &mut Transform)>,
    mut puffs: MessageWriter<DustPuff>,
) {
    let dt = time.delta_secs();
    for (mut plate, mut pose) in &mut plates {
        let works = running(&machines, plate.id);
        let target = f32::from(u8::from(works));
        let step = STROKE_EASE * dt;
        plate.level = if target > plate.level {
            (plate.level + step).min(target)
        } else {
            (plate.level - step).max(target)
        };
        if plate.level <= 0. {
            pose.translation.y = plate.rest;
            plate.phase = 0.;
            continue;
        }
        let before = plate.phase;
        plate.phase = (plate.phase + dt / STROKE_SECS).fract();
        // The foot is the half: crossed once a stroke.
        if works
            && before < 0.5
            && plate.phase >= 0.5
            && let Ok(at) = owners.get(plate.owner)
        {
            puffs.write(DustPuff(WorldPos::new(
                at.0.x(),
                at.0.y() + f64::from(plate.rest),
                at.0.z(),
            )));
        }
        pose.translation.y = plate.rest + STROKE_RISE * plate.level * stroke(plate.phase);
    }
}

/// `--harvester` and `--harvest-hours`, once the player stands in the
/// world: a machine is planted where they stand, on the richest deposit
/// there, for the member who is out and ignoring their count and what they
/// carry; then every planted machine is advanced by that much played time.
/// For a still and a test: the real planting is the sim's request.
#[allow(
    clippy::too_many_arguments,
    reason = "the flags, the world, the household's record and the machines it changes"
)]
pub(super) fn harvester_at_start(
    file: Res<HouseholdFile>,
    registries: Res<strata::sim::Registries>,
    survey: Option<Res<Survey>>,
    household: Option<Res<Household>>,
    players: Query<(&WorldPosition, &CharacterSheet), With<LocalPlayer>>,
    mut surveyed: ResMut<Surveyed>,
    mut machines: ResMut<Harvesters>,
    mut done: Local<bool>,
) {
    if *done || (!file.harvester && file.harvest_hours.is_none()) {
        return;
    }
    let (Some(survey), Some(household), Ok((at, sheet))) = (survey, household, players.single())
    else {
        return;
    };
    *done = true;
    let registries = &registries.0;
    if file.harvester {
        let here = [at.0.x(), at.0.z()];
        let rules = registries.resources.harvest();
        let richest = deposits::concentration_here(&survey, here[0], here[1])
            .into_iter()
            // The rule's least holds, so a still cannot show a machine the
            // game would refuse: `--at deposit` starts near an area's edge
            // and needs `--spawn` nearer the centre.
            .filter(|(_, pm)| *pm >= rules.min_pm)
            .max_by_key(|(deposit, pm)| (*pm, std::cmp::Reverse(deposit.id)));
        let item = registries.items.iter().find_map(|def| {
            matches!(def.kind, ItemKind::Harvester { .. }).then(|| Record {
                item: def.id.clone(),
                count: 1,
                batch: 0,
                by: None,
                lines: def
                    .lines
                    .iter()
                    .map(|line| (line.id.clone(), STORE_GRADE))
                    .collect(),
                condition: None,
                fitted: BTreeMap::new(),
                origin: None,
            })
        });
        if let (Some((deposit, pm)), Some(item)) = (richest, item) {
            {
                let made = harvest::station(
                    rules,
                    &registries.items,
                    &item,
                    &registries.progression,
                    sheet,
                );
                if let Some(lot) = deposits::lot(&deposit, &registries.resources, 1) {
                    surveyed.found.entry(deposit.id).or_insert(Found {
                        reading: Reading::Exact,
                        sampled: true,
                        best_pm: pm,
                        best_at: here,
                    });
                    let id = machines.next;
                    machines.next += 1;
                    machines.list.push(Harvester {
                        id,
                        deposit: deposit.id,
                        at: here,
                        pm,
                        item,
                        by: household.active().name.clone(),
                        rate: made.rate,
                        hopper: made.hopper,
                        lot,
                        held: 0,
                        carry: 0,
                        full: false,
                    });
                    info!(
                        "--harvester: planted on {} at {pm} per-mille for {}",
                        deposit.place,
                        household.active().name
                    );
                }
            }
        } else {
            warn!("--harvester: no deposit holds this spot, or no harvester item");
        }
    }
    if let Some(hours) = file.harvest_hours {
        let ticks = (hours.max(0.) * 3600. * strata::core::SIM_TICK_HZ).round();
        #[allow(
            clippy::cast_possible_truncation,
            clippy::cast_sign_loss,
            reason = "a few hours of twenty ticks a second, floored at nought"
        )]
        let ticks = ticks as u32;
        harvest::advance_ticks(&mut machines, ticks);
        info!("--harvest-hours: {hours} played hours passed");
    }
}

/// `--open-harvester`: the nearest planted machine is used as an F would,
/// from wherever the player stands, once it has a prop to name.
pub(super) fn open_harvester_for_shot(
    file: Res<HouseholdFile>,
    frames: Res<bevy::diagnostic::FrameCount>,
    players: Query<&WorldPosition, With<LocalPlayer>>,
    props: Query<(Entity, &WorldPosition, &Interaction)>,
    mut interacts: MessageWriter<Interact>,
    mut done: Local<bool>,
) {
    if !file.open_harvester || *done || frames.0 < crate::OPEN_PAUSE_FRAME {
        return;
    }
    let Ok(player) = players.single() else {
        return;
    };
    let nearest = props
        .iter()
        .filter(|(.., prop)| matches!(prop.kind, InteractionKind::Harvester { .. }))
        .min_by(|a, b| {
            let away = |at: &WorldPosition| {
                let delta = at.0.0 - player.0.0;
                delta.x.hypot(delta.z)
            };
            away(a.1).total_cmp(&away(b.1))
        });
    if let Some((entity, _, prop)) = nearest {
        *done = true;
        interacts.write(Interact {
            entity,
            kind: prop.kind.clone(),
        });
    }
}

#[cfg(test)]
mod pump_tests {
    use super::*;
    use std::time::Duration;

    fn machine(id: u32, held: u32) -> Harvester {
        Harvester {
            id,
            deposit: strata::sim::deposits::DepositId([0, 0]),
            at: [0., 0.],
            pm: 500,
            item: record(),
            by: "Ilse Marrow".to_owned(),
            rate: 10,
            hopper: 100,
            lot: record(),
            held,
            carry: 0,
            full: held >= 100,
        }
    }

    fn record() -> strata::registry::Record {
        strata::registry::Record {
            item: "veyra:station.field_harvester".parse().expect("an id"),
            count: 1,
            batch: 0,
            by: None,
            lines: BTreeMap::new(),
            condition: None,
            fitted: BTreeMap::new(),
            origin: None,
        }
    }

    fn world(held: u32) -> World {
        let mut world = World::new();
        world.init_resource::<Time>();
        world.init_resource::<Messages<DustPuff>>();
        world.insert_resource(Harvesters {
            list: vec![machine(0, held)],
            next: 1,
        });
        world
    }

    fn schedule<M>(
        system: impl IntoScheduleConfigs<bevy::ecs::system::ScheduleSystem, M>,
    ) -> Schedule {
        let mut schedule = Schedule::default();
        schedule.add_systems(system);
        schedule
    }

    #[test]
    fn a_running_machine_hums_and_a_full_or_gone_one_does_not() {
        let mut world = world(0);
        let prop = world.spawn(HarvesterProp(0)).id();
        let mut hum = schedule(hum_harvesters);
        hum.run(&mut world);
        assert_eq!(
            world.get::<LoopEmitter>(prop),
            Some(&LoopEmitter(EmitterSound::HarvesterHum))
        );
        world.resource_mut::<Harvesters>().list[0].held = 100;
        hum.run(&mut world);
        assert!(world.get::<LoopEmitter>(prop).is_none());
        world.resource_mut::<Harvesters>().list[0].held = 5;
        hum.run(&mut world);
        assert!(world.get::<LoopEmitter>(prop).is_some());
        world.resource_mut::<Harvesters>().list.clear();
        hum.run(&mut world);
        assert!(world.get::<LoopEmitter>(prop).is_none());
    }

    #[test]
    fn a_running_plate_pumps_with_a_puff_a_stroke_and_a_full_one_stands_still() {
        let mut world = world(0);
        let owner = world.spawn(WorldPosition::new(0., 0., 0.)).id();
        let plate = world
            .spawn((
                HarvesterPump {
                    id: 0,
                    owner,
                    rest: 0.9,
                    level: 0.,
                    phase: 0.,
                },
                Transform::from_xyz(0., 0.9, 0.),
            ))
            .id();
        let mut pump = schedule(pump_harvesters);
        let (mut high, mut low) = (f32::MIN, f32::MAX);
        let mut puffs = 0;
        // Three strokes at 20 frames a second.
        for _ in 0..60 {
            world
                .resource_mut::<Time>()
                .advance_by(Duration::from_millis(50));
            pump.run(&mut world);
            let y = world.get::<Transform>(plate).expect("plate").translation.y;
            high = high.max(y);
            low = low.min(y);
            puffs += world.resource_mut::<Messages<DustPuff>>().drain().count();
        }
        assert!(high > 0.9 + STROKE_RISE * 0.9, "it rises: {high}");
        assert!(low >= 0.9 - 1e-6, "never under its rest: {low}");
        assert!((2..=3).contains(&puffs), "a puff a stroke: {puffs}");
        // Full: it eases to rest and stays, with no more puffs.
        world.resource_mut::<Harvesters>().list[0].held = 100;
        for _ in 0..40 {
            world
                .resource_mut::<Time>()
                .advance_by(Duration::from_millis(50));
            pump.run(&mut world);
            assert_eq!(
                world.resource_mut::<Messages<DustPuff>>().drain().count(),
                0
            );
        }
        let y = world.get::<Transform>(plate).expect("plate").translation.y;
        assert!((y - 0.9).abs() < 1e-6, "{y}");
    }
}
