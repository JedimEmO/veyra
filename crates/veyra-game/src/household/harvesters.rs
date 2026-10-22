//! The field harvesters in the world (ticket 0009, step 6): each planted
//! machine stands as a prop with an `Interaction`, appears when planted and
//! goes when picked up, and the flags that plant or advance one for a still.
//! The machines, their clock and their rules are `veyra_sim::harvest`'s;
//! this is only where they stand; what they look like is the window's.

use strata::sim::harvest::{self, Harvester, Harvesters};

use super::*;

/// The machines' systems: the props and the flags that plant or advance one
/// run in every mode, once the player is in the world; the lamp, the hum and
/// the screen a flag opens are the window's.
pub fn install_harvesters(app: &mut App) {
    app.add_systems(
        Update,
        (harvester_at_start, stand_harvesters)
            .chain()
            .run_if(resource_exists::<Household>)
            .run_if(in_state(AppState::InWorld)),
    );
}

/// What the sim is handed of the household's machines: all of them in the
/// world the survey record is live in, none in another, where they stay in
/// the file, are not stood and are not advanced.
pub fn stood_harvesters(household: &Household, live: bool) -> Harvesters {
    if live {
        household.harvesters.clone()
    } else {
        Harvesters::default()
    }
}

/// A placed harvester's number, for the sim's list to find it by.
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq)]
pub struct HarvesterProp(pub u32);

/// Stand a prop for every planted machine and take away those no longer in
/// the list, on the frame the list changes (and again until the ground under
/// a new one has streamed). The sim side (position, interaction) is the same
/// in every mode; the art is the window's (`dress_props`).
#[allow(
    clippy::too_many_arguments,
    reason = "the household's machines, the props standing and the ground the new ones stand on"
)]
pub fn stand_harvesters(
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
        ));
    }
}

/// Which standing props the list no longer has, and which of its machines
/// have none yet.
pub fn changes(machines: &Harvesters, standing: &[u32]) -> (Vec<u32>, Vec<u32>) {
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

/// `--harvester` and `--harvest-hours`, once the player stands in the
/// world: a machine is planted where they stand, on the richest deposit
/// there, for the member who is out and ignoring their count and what they
/// carry; then every planted machine is advanced by that much played time.
/// For a still and a test: the real planting is the sim's request.
#[allow(
    clippy::too_many_arguments,
    reason = "the flags, the world, the household's record and the machines it changes"
)]
pub fn harvester_at_start(
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
