//! The procedural sites' buildings on the client (ticket 0005, step 4).
//!
//! Every building the site list stands (`strata::sim::sites::NearSites`)
//! within [`DRAW_M`] of the observer is spawned from its baked file in the
//! `generated://` source, at the pose the physics gives its body
//! (`strata::physics::sites::building_pose`), so what is drawn is what the
//! server collides with. Past [`DROP_M`] it is despawned: a flight over
//! hundreds of sites holds the few within four kilometres.
//!
//! Far detail. A city needs its skyline because thousands of pieces cost
//! frames from anywhere; a site is a handful of buildings whose own farthest
//! level (from 250 m, nothing under 1.5 m, joined per cell) is a few dozen
//! pieces, so it carries itself to [`DRAW_M`] and needs no skyline and no
//! impostor. Past it nothing is drawn: a mast twenty metres tall is three
//! pixels at four kilometres.
//!
//! A file that is missing is said once and its building left out; the pad
//! stays empty, as the server's does.

use std::collections::{BTreeMap, BTreeSet, HashMap};

use ashlar_bevy::prelude::{AshlarBuilding, MaterialLibraryAsset};
use bevy::prelude::*;
use strata::physics::sites::building_pose;
use strata::prelude::*;
use strata::sim::sites::{NearSites, SiteBuildingKey, buildings_within};
use strata::sim::{GeneratedRoot, Observers};

/// The material library the content step writes, in the `generated://` source.
const LIBRARY: &str = "generated://materials/library.materials.ron";

/// A site's buildings are drawn within this of the observer, metres,
/// measured to the site's centre.
pub const DRAW_M: f64 = 4_000.0;

/// And taken out past this, metres: room enough that an observer at the
/// line does not churn them.
pub const DROP_M: f64 = 4_400.0;

/// The most building roots spawned in one frame, so a flight that meets a
/// town and its neighbours at once spreads them over a few frames.
const SPAWNS_PER_FRAME: usize = 8;

/// A site building's root, named for queries and the inspector.
#[derive(Component, Debug, Clone)]
#[allow(
    dead_code,
    reason = "read by the inspector and by queries while debugging"
)]
pub struct SiteBuilding {
    /// The site's name.
    pub site: String,
    /// The baked file.
    pub file: String,
}

/// What is drawn, and which files exist.
#[derive(Resource, Default)]
struct Drawn {
    roots: BTreeMap<SiteBuildingKey, Entity>,
    /// Whether each file asked for is on disk; a missing one is said once,
    /// when it is first asked for.
    files: HashMap<String, bool>,
}

pub(super) fn install(app: &mut App) {
    app.init_resource::<Drawn>()
        .add_systems(Update, stream_sites.run_if(in_state(AppState::InWorld)))
        .add_systems(OnExit(AppState::InWorld), clear);
}

/// The buildings a client draws seen from `eye`: every building of the list
/// within `radius_m`, its key, its file and its pose.
pub(crate) fn drawn_from(
    near: &NearSites,
    eye: WorldPos,
    radius_m: f64,
) -> Vec<(SiteBuildingKey, String, String, WorldPos, Quat)> {
    buildings_within(near, &[eye], radius_m)
        .map(|(key, site, building)| {
            let (at, rotation) = building_pose(building);
            (
                key,
                site.site.name.clone(),
                building.file.clone(),
                at,
                rotation,
            )
        })
        .collect()
}

/// Spawns the site buildings within [`DRAW_M`] of the primary observer and
/// despawns those past [`DROP_M`].
#[allow(
    clippy::too_many_arguments,
    reason = "a Bevy system's parameters are its dependencies"
)]
fn stream_sites(
    mut commands: Commands,
    near: Option<Res<NearSites>>,
    observers: Res<Observers>,
    generated: Option<Res<GeneratedRoot>>,
    server: Res<AssetServer>,
    origin: Res<RenderOrigin>,
    mut drawn: ResMut<Drawn>,
    mut library: Local<Option<Handle<MaterialLibraryAsset>>>,
) {
    let (Some(near), Some(eye)) = (near, observers.primary) else {
        return;
    };
    let keep: BTreeSet<SiteBuildingKey> = buildings_within(&near, &[eye], DROP_M)
        .map(|(key, _, _)| key)
        .collect();
    let gone: Vec<SiteBuildingKey> = drawn
        .roots
        .keys()
        .filter(|key| !keep.contains(key))
        .copied()
        .collect();
    for key in gone {
        if let Some(entity) = drawn.roots.remove(&key) {
            commands.entity(entity).despawn();
        }
    }
    let Some(root) = generated.and_then(|g| g.0.clone()) else {
        return;
    };
    let mut spawned = 0;
    for (key, site, file, at, rotation) in drawn_from(&near, eye, DRAW_M) {
        if spawned >= SPAWNS_PER_FRAME {
            break;
        }
        if drawn.roots.contains_key(&key) {
            continue;
        }
        let exists = *drawn.files.entry(file.clone()).or_insert_with(|| {
            let exists = root.exists(&file);
            if !exists {
                warn!(
                    "no baked site building at generated/{}; its pads stand without it until `just content`",
                    file
                );
            }
            exists
        });
        if !exists {
            continue;
        }
        let materials = library.get_or_insert_with(|| server.load(LIBRARY)).clone();
        let entity = commands
            .spawn((
                Name::new(format!("{site}/{file}")),
                crate::pieces::Streamed,
                SiteBuilding {
                    site,
                    file: file.clone(),
                },
                AshlarBuilding {
                    building: server.load(format!("generated://{file}")),
                    materials,
                },
                WorldPosition(at),
                Transform::from_translation(origin.to_render(at)).with_rotation(rotation),
                Visibility::default(),
            ))
            .id();
        drawn.roots.insert(key, entity);
        spawned += 1;
    }
}

/// Leaving the world takes every drawn building with it, as the settlement
/// does, so a later entry starts from nothing and doubles none.
fn clear(mut commands: Commands, mut drawn: ResMut<Drawn>) {
    for (_, root) in std::mem::take(&mut drawn.roots) {
        commands.entity(root).despawn();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use strata::physics::sites::COLLIDE_M;
    use strata::registry::SiteKindId;
    use strata::sim::sites::{NearSite, PlacedSiteBuilding, ProceduralSite, SiteId, SitePlacement};

    fn site(x: f64, node: i32) -> NearSite {
        NearSite {
            site: ProceduralSite {
                id: SiteId {
                    kind: SiteKindId(0),
                    mass: [0, 0],
                    node: [node, 0],
                },
                name: format!("Site {node}"),
                centre: [x, 0.0],
                level_m: 3.0,
                radius_m: 30.0,
                blend_m: 30.0,
                crown: 0.02,
                facing: [0.6, 0.8],
                arrival: [x, -30.0],
                by: None,
                score: 1.0,
                cut_m: 0.0,
            },
            kind: "strata:hamlet".to_owned(),
            kind_name: "Hamlet".to_owned(),
            placement: Some(SitePlacement {
                arrangement: 1,
                way_in_clear: true,
                buildings: (0..3)
                    .map(|i| PlacedSiteBuilding {
                        file: format!("sites/veyra/house-{i}.ashlar"),
                        at: [x + f64::from(i) * 7.0, 2.96, 4.0],
                        yaw: 0.4 + f64::from(i),
                        footprint: [[x, 0.0]; 4],
                    })
                    .collect(),
                sign: None,
            }),
        }
    }

    /// What the client draws within the physics' reach is exactly what the
    /// physics collides with, at the same poses, and the client draws
    /// further.
    #[test]
    fn the_client_draws_what_the_server_collides_with() {
        let near = NearSites {
            sites: vec![site(100.0, 1), site(2_500.0, 2), site(9_000.0, 3)],
            around: Vec::new(),
        };
        let eye = WorldPos::new(0.0, 10.0, 0.0);
        let drawn = drawn_from(&near, eye, DRAW_M);
        assert_eq!(drawn.len(), 6, "the two sites within reach, three each");
        let collides: Vec<_> = buildings_within(&near, &[eye], COLLIDE_M)
            .map(|(key, _, building)| (key, building_pose(building)))
            .collect();
        assert_eq!(collides.len(), 3);
        for (key, (at, rotation)) in collides {
            let (_, _, _, drawn_at, drawn_rotation) = drawn
                .iter()
                .find(|d| d.0 == key)
                .expect("a body the client does not draw");
            assert_eq!((*drawn_at, *drawn_rotation), (at, rotation));
        }
    }
}
