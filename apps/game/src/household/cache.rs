//! The cache a cleared camp leaves (ticket 0012, step 4a): a chest at the
//! camp's centre while the sim's [`Caches`] holds its key, which F empties
//! into the pack. What it holds and the take are the sim's
//! ([`strata::sim::loot`]); this is where it stands, and the household's
//! copy of the caches so they are kept across a restart.

use strata::sim::loot::Caches;
use strata::sim::sites::NearSites;

use super::*;

/// A standing cache prop's camp key, for the list to find it by.
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct CacheProp(pub(super) u64);

/// What the sim is handed of the household's caches: all of them in the
/// world the survey record is live in, none in another, where they stay in
/// the file.
pub(super) fn stood_caches(household: &Household, live: bool) -> Caches {
    if live {
        household.caches.clone()
    } else {
        Caches::default()
    }
}

/// Stand a chest for every cache held and take away those no longer held,
/// on the frame the list changes (and again until the ground under a new one
/// has streamed). A cache whose camp is not near stands when it is: the sim
/// draws the camp's site only round the observers.
#[allow(
    clippy::too_many_arguments,
    reason = "the caches, the sites they stand at, the props standing and the ground under them"
)]
pub(super) fn stand_caches(
    mut commands: Commands,
    caches: Option<Res<Caches>>,
    sites: Option<Res<NearSites>>,
    standing: Query<(Entity, &CacheProp)>,
    source: Res<WorldSourceRes>,
    origin: Res<RenderOrigin>,
    spatial: SpatialQuery,
    actors: Query<Entity, With<Combatant>>,
    mut waiting: Local<bool>,
) {
    let Some(caches) = caches else {
        return;
    };
    let sites_changed = sites.as_ref().is_some_and(Res::is_changed);
    if !caches.is_changed() && !sites_changed && !*waiting {
        return;
    }
    *waiting = false;
    let on: Vec<u64> = standing.iter().map(|(_, stood)| stood.0).collect();
    let (gone, new) = changes(&caches, &on);
    for (entity, stood) in &standing {
        if gone.contains(&stood.0) {
            commands.entity(entity).despawn();
        }
    }
    let Some(sites) = sites else {
        return;
    };
    let filter = SpatialQueryFilter::from_excluded_entities(actors.iter());
    for key in &new {
        let Some(near) = sites.sites.iter().find(|near| near.site.id.key() == *key) else {
            continue;
        };
        let [x, z] = near.site.centre;
        let Some(ground) = ground_at(x, z, &source, &origin, &spatial, &filter) else {
            *waiting = true;
            continue;
        };
        commands.spawn((
            Name::new(chest_name(&near.kind)),
            Prop::Cache,
            CacheProp(*key),
            Interaction {
                kind: InteractionKind::Cache { key: *key },
                radius: REACH_M,
            },
            WorldPosition(WorldPos::new(x, ground, z)),
            Transform::default(),
            Visibility::default(),
        ));
    }
}

/// What the chest at a place of `kind` is called: a camp's cache, a wreck's
/// salvage, a machine's parts.
pub(super) fn chest_name(kind: &str) -> &'static str {
    strata::client::ui::strings::english(&format!("strata.cache.name.{kind}"))
        .unwrap_or("Outlaw cache")
}

/// Which standing props the caches no longer hold, and which caches have
/// none yet.
pub(super) fn changes(caches: &Caches, standing: &[u64]) -> (Vec<u64>, Vec<u64>) {
    let gone = standing
        .iter()
        .copied()
        .filter(|key| !caches.by.contains_key(key))
        .collect();
    let new = caches
        .by
        .keys()
        .copied()
        .filter(|key| !standing.contains(key))
        .collect();
    (gone, new)
}

/// The sim's caches copied into the household when they differ, stamped with
/// the world like the camps they belong to ([`SurveyLive`]).
pub(super) fn track_caches(
    household: &mut ResMut<Household>,
    caches: Option<Res<Caches>>,
    live: Option<&SurveyLive>,
) {
    let (Some(caches), Some(live)) = (caches, live) else {
        return;
    };
    if !caches.is_changed() || household.caches == *caches {
        return;
    }
    household.caches.clone_from(&caches);
    if !caches.by.is_empty() && household.surveyed_in != Some(live.world) {
        household.surveyed_in = Some(live.world);
    }
}

/// The jobs taken and the camps found that this run may read: the
/// household's when its record is of this world, none in another one, whose
/// record stays in the file.
pub(super) fn stood_jobs(household: &Household, live: bool) -> (Jobs, Known) {
    if live {
        (household.jobs.clone(), household.known.clone())
    } else {
        Default::default()
    }
}

/// The sim's jobs and found camps copied into the household when they
/// differ, stamped with the world like the camps they belong to
/// ([`SurveyLive`]).
pub(super) fn track_jobs(
    household: &mut ResMut<Household>,
    jobs: Option<Res<Jobs>>,
    known: Option<Res<Known>>,
    live: Option<&SurveyLive>,
) {
    let Some(live) = live else {
        return;
    };
    let mut stamp = false;
    if let Some(jobs) = jobs
        && jobs.is_changed()
        && household.jobs != *jobs
    {
        household.jobs.clone_from(&jobs);
        stamp |= !jobs.taken.is_empty();
    }
    if let Some(known) = known
        && known.is_changed()
        && household.known != *known
    {
        household.known.clone_from(&known);
        stamp |= !known.camps.is_empty();
    }
    if stamp && household.surveyed_in != Some(live.world) {
        household.surveyed_in = Some(live.world);
    }
}

#[cfg(test)]
mod tests {
    use super::chest_name;

    /// A chest is named by the kind of place that left it.
    #[test]
    fn a_chest_is_named_by_its_place() {
        assert_eq!(chest_name("strata:wild_camp"), "Outlaw cache");
        assert_eq!(chest_name("strata:wreck"), "Salvage");
        assert_eq!(chest_name("strata:wreck_works"), "Machine parts");
    }
}
