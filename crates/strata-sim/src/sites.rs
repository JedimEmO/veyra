//! What stands on the procedural sites near the observers, the same on a
//! server and on every client (ADR 0012, decision 6; ticket 0005, step 4).
//!
//! The world finds its sites in the river bake (`strata_worldgen::sites`)
//! and the content step bakes a building group per site kind. The loader
//! reads each kind's group ([`SiteGroups`]) and [`refresh_near_sites`] keeps
//! [`NearSites`]: every site within [`NEAR_SITES_M`] of an observer, with
//! the group stood on it by `strata_worldgen::sites::place_group`, a pure
//! function of the site and the layout. The physics builds colliders from
//! that list and the client draws from it, each within its own distance
//! ([`buildings_within`]), so what a client draws is what the server
//! collides with, and nothing about a site is replicated.
//!
//! The list is computed on the task pool and only when an observer has
//! moved [`REFRESH_M`] from where it was last computed: reading the sites
//! of a land mass is cheap once its rivers are baked, but a mass no one
//! has baked is baked by whoever asks, and that must never be a frame.

use std::path::Path;
use std::sync::Arc;

use bevy::ecs::prelude::*;
use bevy::tasks::{AsyncComputeTaskPool, Task, TaskPool, block_on, poll_once};
use strata_core::WorldPos;
use strata_registry::files::{Files, MountedDirs};
use strata_registry::{AssetSource, RoadClass, SiteKindId, SourceError};
use strata_worldgen::ProceduralWorld;
// What a consumer of the list reads off a site, so the render side needs
// no dependency on the world crate.
use strata_worldgen::sites::{FIT_MARGIN_M, SiteGroupLayout, group_layout_path, place_group};
pub use strata_worldgen::sites::{
    PlacedSiteBuilding, ProceduralSite, SKIRT_M, SiteId, SitePlacement,
};
use tracing::{info, warn};

use crate::Observers;

/// How far round each observer [`NearSites`] lists sites, metres: past the
/// farthest any consumer stands a building (the client draws to 4 km) by
/// more than [`REFRESH_M`], so the list is complete for each of them
/// however far an observer has moved since it was made.
pub const NEAR_SITES_M: f64 = 5_000.0;

/// How far an observer moves from every place the list was last made round
/// before it is made again, metres.
pub const REFRESH_M: f64 = 250.0;

/// One site kind as the runtime stands it.
#[derive(Debug, Clone)]
pub struct KindGroup {
    /// The kind's registry name, `strata:hamlet`.
    pub kind: String,
    /// What the kind is called, for a player: `Hamlet`.
    pub kind_name: String,
    /// The class of road that reaches it.
    pub road: RoadClass,
    /// Its building group as the content step baked it, or `None` when the
    /// layout is missing or unreadable: its sites keep their pads and stand
    /// nothing.
    pub layout: Option<Arc<SiteGroupLayout>>,
}

/// The world's sites and each kind's building group, published with the
/// world.
#[derive(Resource, Clone)]
pub struct SiteGroups {
    /// The world the sites are read from.
    pub world: Arc<ProceduralWorld>,
    /// Each kind's group, by kind id.
    pub kinds: Arc<std::collections::BTreeMap<SiteKindId, KindGroup>>,
}

impl SiteGroups {
    /// Every kind of `world`, with its group read from `generated` (the
    /// content step's output; `None` reads nothing). A group whose layout
    /// is missing is said once, here; an arrangement that no longer fits
    /// its kind's pad is said too, and [`place_group`] never chooses it.
    #[must_use]
    pub fn read(world: Arc<ProceduralWorld>, generated: Option<&dyn AssetSource>) -> Self {
        let mut kinds = std::collections::BTreeMap::new();
        for kind in world.site_kinds() {
            let layout = generated.and_then(|files| {
                let path = group_layout_path(&kind.def.group);
                read_layout(files, &path, kind.def.footprint_m).map(Arc::new)
            });
            kinds.insert(
                kind.id,
                KindGroup {
                    kind: kind.name.to_string(),
                    kind_name: kind.def.name.clone(),
                    road: kind.traits.road,
                    layout,
                },
            );
        }
        Self {
            world,
            kinds: Arc::new(kinds),
        }
    }
}

/// The version of what stands on the sites, at the roads' crossings and
/// along the roads, for the handshake: every kind of the registries under
/// `asset_root`, its group's baked layout under `generated` and every file
/// the layout stands, the crossings' kit (`crate::crossings::KIT_PATH`) and
/// the roadside kit (`crate::roadside::ROADSIDE_PATH`) and every file they
/// name, each one's bytes hashed (FNV-1a), a missing one as missing. Two
/// ends whose versions agree read the same layouts and collide with the
/// same baked proxies, so what a client draws and predicts against is what
/// the server has. The world's version already covers where the sites and
/// the crossings are; this covers what stands on them, which the content
/// step makes and the world does not. The name is the field's in the
/// handshake, which is append-only and kept.
///
/// # Errors
///
/// When the registries do not load.
pub fn site_groups_version(
    asset_root: impl AsRef<Path>,
    generated: impl AsRef<Path>,
) -> Result<u64, strata_registry::LoadError> {
    let files = Files::new(
        MountedDirs::new()
            .mount("assets", asset_root.as_ref())
            .mount("generated", generated.as_ref()),
    );
    site_groups_version_from(&files)
}

/// [`site_groups_version`] over a source with the logical roots `assets/` and
/// `generated/`, the installed one in the game.
///
/// # Errors
///
/// When the registries do not load.
pub fn site_groups_version_from(files: &Files) -> Result<u64, strata_registry::LoadError> {
    let registries = strata_registry::load_from_source(&files.under("assets"))?;
    let generated = files.under("generated");
    let generated: &dyn AssetSource = &generated;
    let mut hash = Fnv::default();
    let mut kinds: Vec<(String, String)> = registries
        .sites
        .iter()
        .map(|(_, name, def)| (name.to_string(), def.group.clone()))
        .collect();
    kinds.sort();
    let mut files = std::collections::BTreeSet::new();
    for (kind, group) in kinds {
        hash.text(&kind);
        hash.text(&group);
        let read = generated.read(&group_layout_path(&group)).ok();
        hash.bytes(read.as_deref());
        let layout = read
            .as_deref()
            .and_then(|bytes| std::str::from_utf8(bytes).ok())
            .and_then(|text| ron::from_str::<SiteGroupLayout>(text).ok());
        if let Some(layout) = layout {
            files.extend(
                layout
                    .arrangements
                    .iter()
                    .flat_map(|a| a.buildings.iter().map(|b| b.file.clone())),
            );
            files.extend(layout.sign.map(|sign| sign.file));
        }
    }
    // The crossings' kit (ticket 0005, step 7), last: its layout and every
    // file it names. A bridge's rails are walls as much as a site's.
    let kit = generated.read(crate::crossings::KIT_PATH).ok();
    if let Some(kit) = kit
        .as_deref()
        .and_then(|bytes| std::str::from_utf8(bytes).ok())
        .and_then(|text| ron::from_str::<crate::crossings::CrossingKit>(text).ok())
    {
        files.extend(kit.files());
    }
    // The roadside kit (ticket 0005, step 8), after it: its layout and its
    // files. A lamp's pole is a body the server collides with as well.
    let roadside = generated.read(crate::roadside::ROADSIDE_PATH).ok();
    if let Some(roadside) = roadside
        .as_deref()
        .and_then(|bytes| std::str::from_utf8(bytes).ok())
        .and_then(|text| ron::from_str::<crate::roadside::RoadsideKit>(text).ok())
    {
        files.extend(roadside.files());
    }
    for file in files {
        hash.text(&file);
        hash.bytes(generated.read(&file).ok().as_deref());
    }
    hash.text(crate::crossings::KIT_PATH);
    hash.bytes(kit.as_deref());
    hash.text(crate::roadside::ROADSIDE_PATH);
    hash.bytes(roadside.as_deref());
    Ok(hash.0)
}

/// FNV-1a, 64 bits: the same on every platform and run.
struct Fnv(u64);

impl Default for Fnv {
    fn default() -> Self {
        Self(0xcbf2_9ce4_8422_2325)
    }
}

impl Fnv {
    fn byte(&mut self, byte: u8) {
        self.0 ^= u64::from(byte);
        self.0 = self.0.wrapping_mul(0x0100_0000_01b3);
    }

    /// A string and its length, so two strings in a row are told apart.
    fn text(&mut self, text: &str) {
        self.bytes(Some(text.as_bytes()));
    }

    /// Bytes and their length, or a mark for none.
    fn bytes(&mut self, bytes: Option<&[u8]>) {
        match bytes {
            Some(bytes) => {
                for byte in (bytes.len() as u64).to_le_bytes() {
                    self.byte(byte);
                }
                for &byte in bytes {
                    self.byte(byte);
                }
            }
            None => {
                for byte in u64::MAX.to_le_bytes() {
                    self.byte(byte);
                }
            }
        }
    }
}

/// One file of a source as text.
pub(crate) fn read_text(source: &dyn AssetSource, path: &str) -> Result<String, SourceError> {
    let bytes = source.read(path)?;
    String::from_utf8(bytes).map_err(|error| SourceError::Io {
        path: path.to_owned(),
        message: error.to_string(),
    })
}

/// A group's layout, or `None` with a warning when it is missing or
/// unreadable; every arrangement that reaches past a pad of `radius_m` is
/// named, since the sites of the kind will never stand it.
fn read_layout(generated: &dyn AssetSource, path: &str, radius_m: f64) -> Option<SiteGroupLayout> {
    let text = read_text(generated, path)
        .map_err(|error| {
            warn!(
                "no site building group at {path}: {error}; its sites stand empty until `just content`"
            );
        })
        .ok()?;
    let layout: SiteGroupLayout = ron::from_str(&text)
        .map_err(|error| warn!("site building group {path}: {error}"))
        .ok()?;
    for arrangement in &layout.arrangements {
        if arrangement.reach_m() > radius_m - FIT_MARGIN_M {
            warn!(
                "{}: arrangement {} reaches {:.1} m on a {radius_m} m pad and is left out; \
                 run `just content`",
                layout.group,
                arrangement.name,
                arrangement.reach_m()
            );
        }
    }
    info!(
        group = %layout.group,
        arrangements = layout.arrangements.len(),
        "site building group read"
    );
    Some(layout)
}

/// One site near an observer and what stands on it.
#[derive(Debug, Clone, PartialEq)]
pub struct NearSite {
    /// The site.
    pub site: ProceduralSite,
    /// Its kind's registry name.
    pub kind: String,
    /// What its kind is called, for a player.
    pub kind_name: String,
    /// Its group stood on it, or `None` when its kind's group is missing or
    /// has no arrangement that fits.
    pub placement: Option<SitePlacement>,
}

/// The sites within [`NEAR_SITES_M`] of the observers, by id, and where the
/// list was last made round.
#[derive(Resource, Debug, Default, Clone)]
pub struct NearSites {
    /// The sites, ordered by id: the same order everywhere.
    pub sites: Vec<NearSite>,
    /// The observers' places when it was made, `(x, z)`.
    pub around: Vec<[f64; 2]>,
}

/// Every site within `radius_m` of any of `around`, each once, ordered by
/// id, with its kind's group stood on it. Pure in its inputs: a server and
/// a client with the same world and the same groups get the same list.
#[must_use]
pub fn near_sites(groups: &SiteGroups, around: &[[f64; 2]], radius_m: f64) -> Vec<NearSite> {
    let mut found: std::collections::BTreeMap<_, ProceduralSite> =
        std::collections::BTreeMap::new();
    for &[x, z] in around {
        for site in groups.world.sites_near(x, z, radius_m) {
            found.entry(site.id).or_insert(site);
        }
    }
    found
        .into_values()
        .map(|site| {
            let kind = groups.kinds.get(&site.kind());
            let placement = kind.and_then(|kind| {
                kind.layout
                    .as_ref()
                    .and_then(|layout| place_group(&site, kind.road, layout))
            });
            NearSite {
                kind: kind.map_or_else(String::new, |k| k.kind.clone()),
                kind_name: kind.map_or_else(String::new, |k| k.kind_name.clone()),
                placement,
                site,
            }
        })
        .collect()
}

/// A building of a site, as one key the same everywhere: the site's id key
/// and the building's index in its placement (the board last).
pub type SiteBuildingKey = (u64, usize);

/// Every building of `near` whose site's centre is within `radius_m` of any
/// of `observers`, horizontally, with its key. The one selection the
/// physics' colliders and the client's drawing make, each with its own
/// radius.
pub fn buildings_within<'a>(
    near: &'a NearSites,
    observers: &'a [WorldPos],
    radius_m: f64,
) -> impl Iterator<Item = (SiteBuildingKey, &'a NearSite, &'a PlacedSiteBuilding)> + 'a {
    near.sites
        .iter()
        .filter(move |near| {
            observers.iter().any(|o| {
                let (dx, dz) = (o.x() - near.site.centre[0], o.z() - near.site.centre[1]);
                dx * dx + dz * dz < radius_m * radius_m
            })
        })
        .flat_map(|near| {
            near.placement.iter().flat_map(move |placement| {
                placement
                    .all()
                    .enumerate()
                    .map(move |(i, building)| ((near.site.id.key(), i), near, building))
            })
        })
}

/// The list being made.
#[derive(Default)]
pub struct Refreshing(Option<Task<(Vec<[f64; 2]>, Vec<NearSite>)>>);

/// Makes [`NearSites`] again, on the task pool, whenever an observer has
/// moved [`REFRESH_M`] from every place it was last made round (or the
/// observers came or went), and publishes the list when it is done.
pub fn refresh_near_sites(
    groups: Option<Res<SiteGroups>>,
    observers: Res<Observers>,
    mut near: ResMut<NearSites>,
    mut refreshing: Local<Refreshing>,
) {
    if let Some(task) = refreshing.0.as_mut() {
        let Some((around, sites)) = block_on(poll_once(task)) else {
            return;
        };
        refreshing.0 = None;
        near.around = around;
        near.sites = sites;
    }
    let Some(groups) = groups else {
        return;
    };
    let here: Vec<[f64; 2]> = observers.positions.iter().map(|p| [p.x(), p.z()]).collect();
    let stale = here.len() != near.around.len()
        || here.iter().any(|&[x, z]| {
            near.around.iter().all(|&[ax, az]| {
                let (dx, dz) = (x - ax, z - az);
                dx * dx + dz * dz > REFRESH_M * REFRESH_M
            })
        });
    if !stale {
        return;
    }
    if here.is_empty() {
        near.around.clear();
        near.sites.clear();
        return;
    }
    let groups = groups.clone();
    refreshing.0 = Some(
        AsyncComputeTaskPool::get_or_init(TaskPool::new).spawn(async move {
            let sites = near_sites(&groups, &here, NEAR_SITES_M);
            (here, sites)
        }),
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    fn assets() -> std::path::PathBuf {
        std::path::PathBuf::from(concat!(env!("CARGO_MANIFEST_DIR"), "/../../assets"))
    }

    /// One arrangement of two small sheds either side of the yard: fits
    /// every shipped kind's pad.
    fn layout(group: &str) -> SiteGroupLayout {
        use strata_worldgen::sites::{SiteArrangement, SiteBuilding, SiteSign};
        let shed = |x: f64| SiteBuilding {
            file: format!("{group}/shed.ashlar"),
            position: [x, -4.0],
            yaw: 0.3,
            footprint: [
                [x - 2.0, -6.0],
                [x + 2.0, -6.0],
                [x + 2.0, -2.0],
                [x - 2.0, -2.0],
            ],
        };
        SiteGroupLayout {
            group: group.to_owned(),
            yard_m: 2.0,
            arrangements: vec![
                SiteArrangement {
                    name: "pair".to_owned(),
                    buildings: vec![shed(-6.0), shed(6.0)],
                },
                SiteArrangement {
                    name: "one".to_owned(),
                    buildings: vec![shed(0.0)],
                },
            ],
            sign: Some(SiteSign {
                file: "board.ashlar".to_owned(),
                half_size: [1.4, 0.4],
            }),
        }
    }

    /// The world's groups with [`layout`] for every kind, as if baked.
    fn groups(world: Arc<ProceduralWorld>) -> SiteGroups {
        let mut groups = SiteGroups::read(world, None);
        let kinds = Arc::make_mut(&mut groups.kinds);
        for kind in kinds.values_mut() {
            kind.layout = Some(Arc::new(layout(&kind.kind)));
        }
        groups
    }

    /// Two worlds built apart from the same seed and registries, each
    /// baking its own rivers and placing its own sites on its own thread
    /// count, as a server's and a client's in two processes do, list the
    /// same sites round Port Serein and stand the same buildings on them:
    /// the same arrangement, the same positions and turns, bit for bit.
    #[test]
    fn a_server_and_a_client_stand_the_same_sites() {
        let registries = strata_registry::load_from_dir(assets()).expect("the assets");
        // The client with a cache of its own: the process's shared one
        // would hand it the server's bake. (The server keeps the shared
        // one, which `a_missing_group_stands_nothing` bakes too.)
        let make = || {
            ProceduralWorld::with_params(strata_core::Seed(42), &registries, crate::world_params())
        };
        let server = groups(Arc::new(make()));
        let client = groups(Arc::new(make().with_own_water()));
        assert!(!server.world.shares_water_with(&client.world));
        let city = server.world.sites()[0].to_world_xz([0.0, 0.0]);
        let around = [[city[0] + 2_000.0, city[1] - 3_000.0]];
        // Baked on a few threads first, as the loader would: the list would
        // otherwise bake the city's mass on this thread alone. The two on
        // different counts, which the networks do not depend on.
        server
            .world
            .prepare_water_on(around[0][0], around[0][1], 4_500.0, 4);
        client
            .world
            .prepare_water_on(around[0][0], around[0][1], 4_500.0, 3);
        let a = near_sites(&server, &around, 4_000.0);
        let b = near_sites(&client, &around, 4_000.0);
        assert!(!a.is_empty(), "no sites within 4 km of the place");
        assert_eq!(a, b);
        for near in &a {
            let placement = near.placement.as_ref().expect("every kind has a group");
            for building in placement.all() {
                assert!(building.at.iter().all(|v| v.is_finite()));
            }
        }
        // And the selection each side makes from it is the same.
        let observers = [WorldPos::new(around[0][0], 0.0, around[0][1])];
        let near = NearSites {
            sites: a,
            around: around.to_vec(),
        };
        let keys: Vec<_> = buildings_within(&near, &observers, 3_000.0)
            .map(|(key, _, b)| (key, b.file.clone(), b.at.map(f64::to_bits)))
            .collect();
        let again: Vec<_> = buildings_within(&near, &observers, 3_000.0)
            .map(|(key, _, b)| (key, b.file.clone(), b.at.map(f64::to_bits)))
            .collect();
        assert_eq!(keys, again);
    }

    /// The site groups' version moves with a layout and with any file it
    /// stands, and tells a missing group from a present one.
    #[test]
    fn the_site_groups_version_covers_layouts_and_their_files() {
        let root = std::env::temp_dir().join(format!("strata-site-groups-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let version = || site_groups_version(assets(), &root).expect("the assets");
        let missing = version();
        let registries = strata_registry::load_from_dir(assets()).expect("the assets");
        let (_, _, def) = registries.sites.iter().next().expect("a kind");
        let path = root.join(group_layout_path(&def.group));
        std::fs::create_dir_all(path.parent().expect("a directory")).expect("writable");
        let layout = layout(&def.group);
        let text = ron::to_string(&layout).expect("serialises");
        std::fs::write(&path, &text).expect("writable");
        let shed = root.join(&layout.arrangements[0].buildings[0].file);
        std::fs::create_dir_all(shed.parent().expect("a directory")).expect("writable");
        std::fs::write(&shed, b"one").expect("writable");
        let baked = version();
        assert_ne!(missing, baked);
        assert_eq!(baked, version(), "the same files, the same version");
        std::fs::write(&shed, b"two").expect("writable");
        let rebaked = version();
        assert_ne!(baked, rebaked, "a building's bytes moved");
        std::fs::write(&path, text.replace("pair", "twin")).expect("writable");
        let relaid = version();
        assert_ne!(rebaked, relaid, "the layout moved");
        // The crossings' kit and its files are covered the same way.
        let kit = crate::crossings::tests::kit();
        let kit_path = root.join(crate::crossings::KIT_PATH);
        std::fs::create_dir_all(kit_path.parent().expect("a directory")).expect("writable");
        std::fs::write(&kit_path, ron::to_string(&kit).expect("serialises")).expect("writable");
        let with_kit = version();
        assert_ne!(relaid, with_kit, "the kit arrived");
        let bridge = root.join(&kit.bridges[0].file);
        std::fs::create_dir_all(bridge.parent().expect("a directory")).expect("writable");
        std::fs::write(&bridge, b"deck").expect("writable");
        let with_deck = version();
        assert_ne!(with_kit, with_deck, "a bridge's bytes arrived");
        // And the roadside kit and its files (ticket 0005, step 8).
        let roadside = crate::roadside::RoadsideKit {
            marker: Some("crossings/veyra/road-marker.ashlar".to_owned()),
            lamp: Some("crossings/veyra/road-lamp.ashlar".to_owned()),
        };
        std::fs::write(
            root.join(crate::roadside::ROADSIDE_PATH),
            ron::to_string(&roadside).expect("serialises"),
        )
        .expect("writable");
        let with_roadside = version();
        assert_ne!(with_deck, with_roadside, "the roadside kit arrived");
        let lamp = root.join(roadside.lamp.as_deref().expect("a lamp"));
        std::fs::write(&lamp, b"pole").expect("writable");
        assert_ne!(with_roadside, version(), "a lamp's bytes arrived");
        let _ = std::fs::remove_dir_all(&root);
    }

    /// A kind whose group was never baked keeps its sites, with nothing
    /// standing on them.
    #[test]
    fn a_missing_group_stands_nothing() {
        let registries = strata_registry::load_from_dir(assets()).expect("the assets");
        let world = Arc::new(ProceduralWorld::with_params(
            strata_core::Seed(42),
            &registries,
            crate::world_params(),
        ));
        let missing = Files::dir(std::env::temp_dir().join("strata-no-generated-here"));
        let groups = SiteGroups::read(world, Some(&missing));
        assert!(groups.kinds.values().all(|k| k.layout.is_none()));
        let city = groups.world.sites()[0].to_world_xz([0.0, 0.0]);
        groups.world.prepare_water_on(city[0], city[1], 4_500.0, 4);
        let near = near_sites(&groups, &[city], 4_000.0);
        assert!(!near.is_empty());
        assert!(near.iter().all(|n| n.placement.is_none()));
    }
}
