//! Which world a process has: a number a client and a server compare
//! before one walks on the other's ground (ticket 0004, step 1).
//!
//! The ground is a pure function of the seed and of everything the
//! generator reads: its own code (and the code it calls: `strata-core`'s
//! seed derivation, the `noise` crate's simplex kernel, `libm`), the
//! [`WorldParams`] in force, the biome registry, the geology registry (the
//! provinces, since ticket 0004's step 3) and the settlement registry.
//! The flora registry decides which trees stand where and so which trunks a
//! character collides with. The seed travels on its own (the server's
//! `Welcome`, and the replication handshake hashes it beside this); the
//! rest is folded into one [`world_version`] that the handshake hashes, so
//! an old client on a new ground is refused with a reason instead of
//! predicting on hills the server does not have and being corrected every
//! half metre.
//!
//! The code is the one input a hash cannot read, so it is caught two ways:
//! a canary (the bits of a few fixed samples of a world built from the same
//! parameters and registries) joins the hash, which any code change that
//! moves those points changes by itself; and [`GENERATOR_VERSION`] is a
//! number a person bumps for the changes the canary's points miss.
//!
//! The parameters and registries are hashed whole, cosmetic fields too: a
//! biome's tint changes the version, and refuses clients, over a ground
//! that did not move. That errs on the safe side and is known.

use serde::Serialize;
use strata_registry::GameRegistries;

use crate::WorldParams;

/// The version of the generator's code.
///
/// **Bump this by hand, in the same commit, whenever a change moves the
/// ground**: a stage added or reordered, a constant in a stage, a noise, the
/// erosion filter, the settlement shaping, anything that changes what
/// [`WorldSource::sample`](crate::WorldSource::sample) returns for some
/// point. That includes changes outside this crate: `strata-core`'s
/// `Seed::derive` and `Rng`, which choose every noise's lattice and the
/// ranges' and the wind's axes; the `noise` crate's simplex kernel, so a
/// `cargo update -p noise` too; and `libm`. The parameters and the
/// registries are hashed as data and need no bump. A change that leaves
/// every sample's bits alone (a refactor, a doc, a faster path to the same
/// numbers) keeps the number.
///
/// **Bump it too for a change that moves the river network** (ADR 0010):
/// anything in the `hydrology` module or the coarse ground it reads
/// (`ProceduralWorld::coarse_ground`, the biomes' `lowest` octaves) that
/// changes a reach, a lake or a fall anywhere, though no sample moves. The
/// network's canaries in [`world_version`] catch a change on their own
/// ground (an island, two cut parts of it, a window round Port Serein) and
/// nothing elsewhere: a route a canary never takes, the cap on a mass far
/// larger than any of them. From the carve (ticket 0005, step 2) the
/// network is part of the ground and this rule is the rule above.
///
/// The canary in [`world_version`] catches a change that moves its few
/// points without anyone remembering this; one that moves only ground
/// elsewhere (a settlement's far edge, a rare biome) is caught only by this
/// number. The pinned samples in `tests/snapshots.rs` (which print it), the
/// drama measures in `tests/drama.rs` and the pinned version in
/// `tests/version.rs` change when the ground does; a diff there is the
/// reminder.
///
/// History: 1, the generator as ticket 0004 first measured it; 2, its step
/// 2, which brought the ranges near (parameters and biome files only, which
/// the hash already sees; bumped anyway so that it names the ground a build
/// was made for); 3, the mountains' relief waiting for the mountains to
/// stand alone, with its slope in the landform (ticket 0004's review of
/// step 2); 4, ranges along the zero contours of a signed range field on
/// a seed-chosen lattice offset, each standing on a long foot, and the
/// climate's equator moved so that Port Serein's country is temperate
/// (ticket 0004's step 2b); 5, a range fading towards the coast over a
/// wider, eased band of the continent field instead of the land mask's
/// ramp (ticket 0004's review of step 2b); 6, geological provinces: a
/// province field of the generator's own, the two heaviest provinces in
/// every sample and the ground's cover multiplied by their rock's
/// (ticket 0004's step 3; no height moved); 7, a middle scale and a foot
/// (ticket 0004's step 4): the relief a biome names takes the shape its
/// province gives the biome's class (the damped, hybrid and cellular
/// shapes and a stretch along the range axis), and a range's lift and
/// height of its own are read from the distance to its crest line; 8, the
/// review of step 4: that distance divides by a size of the range field's
/// gradient read from differences (the gullies stepped where the kernel's
/// second derivative jumps), a province's shape is centred on its own mean,
/// and the folded hills and the badlands' hills are lower; 9, beds (ticket
/// 0004's step 5): each province's hard and soft beds pass the relief
/// through a function of height before the gullies (cliff bands, benches,
/// mesa caps), and a range's mountain relief grows towards its crest line;
/// 10, the review of step 5: a range's own ramp opens the beds' gate only
/// in patches a few kilometres across, and the landform's bed is the one
/// that shaped the ground, not the one the gullies cut down to; 11, finite
/// neighbourhood benches and their final-height gradients at Port Serein;
/// 12, the carve (ticket 0005, step 2): every sample ends with the rivers'
/// valleys and channels and the lakes' beds, carries the water at the
/// point, and the network's surfaces are set onto the sample's ground and
/// its mouths graded to the sample's shore; 13, the rivers' shape (ticket
/// 0005, step 3b): every reach settled sideways onto the sample's low
/// ground, its tributaries bent to join it pointing downstream, a channel
/// meandering about its line with riffles and pools, lakes whose beds take
/// the fine ground's shape and whose rims hold them, and deep walls in
/// bedded rock stepped by their beds; 14, procedural sites (ticket 0005,
/// step 4; ADR 0012): each mass's bake places the site kinds' sites after
/// its rivers, and a sample on a site's ground takes its level pad, eased
/// into the ground round it and developed; 15, the road's grade (ticket
/// 0005, step 6): every service road and track has a profile of its own,
/// and a sample by one takes its carriageway, its verge and its cut and
/// fill banks, laid in its class's material; a trail clears its line; and
/// a river valley's walls wander; 16, the roads' alignment (the pass's
/// first two rounds): the search prices a graded road's cut and fill and a turn
/// near a river, crosses on a skew deck, and the line runs straight through a
/// bridge and swings onto it, so roads and the ground they grade moved; 17,
/// the finer wall search round a bridge (the alignment pass, round 3): where a
/// graded road's searched line comes down past its grade near a bridge, the
/// line a few hundred metres either side is laid again on 16 m nodes, its deck
/// slid and turned to where it costs least and its approaches carrying their
/// profile, through the nodes other roads end on and off their lines, and kept
/// only where it costs less than the searched line; so those roads,
/// their bridges and the ground they grade moved.
pub const GENERATOR_VERSION: u32 = 17;

/// The seed of the world the canary samples: any fixed one, since the
/// version is the same whatever seed a world is built with.
const CANARY_SEED: u64 = 42;

/// Where the canary samples, on seed 42's ground since ticket 0004's step
/// 2b (the road's end moves with the ground the road is traced over: at
/// (-191, -1911) before step 4, (-63, -1720) after it, (-1214, -1597) since
/// its review): the open sea, Port Serein's terraces (the settlement's shaping), its
/// road's far end, a coast under a steep shore, a tundra lowland (the
/// climate's cold side), a mountainside part-way up its rise (the
/// elevation's gradient), and two far points. Each reads a different stage,
/// so a code change anywhere in the sample is likely to move at least one.
const CANARY_POINTS: [(f64, f64); 8] = [
    (0.0, 0.0),
    (-400.0, -500.0),
    (-1_214.0, -1_597.0),
    (21_949.0, -15_945.0),
    (0.0, 25_000.0),
    (15_000.0, -17_000.0),
    (-12_345.0, 6_789.0),
    (45_000.0, 45_000.0),
];

/// Where the canary samples the carve (ticket 0005, step 2), inside the
/// canary window round Port Serein: the confluence 2.5 km south of the city,
/// the order-2 river's bank above it and its valley wall, and a creek's
/// channel near the city's road.
const CARVE_POINTS: [(f64, f64); 4] = [
    (-1_516.0, -3_977.0),
    (-1_300.0, -4_060.0),
    (-1_420.0, -4_200.0),
    (-2_660.0, -2_100.0),
];

/// Metres across the canary's graded road its grade is sampled at, either
/// side: the crown, the carriageway's edge, the verge, a bank's foot, its
/// slope and the fade into the ground.
const GRADE_OFFSETS: [f64; 10] = [0.0, 2.0, 4.2, 6.0, 8.5, -9.0, 14.0, -20.0, 33.0, -45.0];

/// The version of the world a [`ProceduralWorld`](crate::ProceduralWorld)
/// built from `params` and `registries` makes, whatever its seed:
/// [`GENERATOR_VERSION`], the parameters, every biome in id order, every
/// flora species but a dressing one in id order, every province in id
/// order, every settlement in registry order, every site kind in id order
/// (the sites themselves through the canary networks, whose content hashes
/// hold the sites their bakes place), the
/// canary (a world built from them at a fixed seed, sampled at fixed
/// points), and the river network's numbers with the content hashes of
/// three canary networks baked on that world (a small whole island, two cut
/// parts of it, a window round Port Serein; see `hydrology`), hashed.
///
/// Builds that canary world, so it costs what building a world does (the
/// settlements' roads are traced then): tens of milliseconds, once, before
/// a process connects.
///
/// The same on every platform and every build: FNV-1a over the inputs'
/// RON text (the tree library's version is made the same way), not
/// `std`'s `DefaultHasher`, whose output is not promised across Rust
/// versions. A float is written in RON as its shortest round-trip decimal,
/// so two finite values hash alike exactly when their bits are alike.
///
/// # Panics
///
/// If `registries` holds no biomes, as
/// [`ProceduralWorld::new`](crate::ProceduralWorld::new) does; a frozen set
/// always has some.
#[must_use]
#[allow(
    clippy::many_single_char_names,
    reason = "a road's segment and its direction, as the grade's canary reads them"
)]
pub fn world_version(params: &WorldParams, registries: &GameRegistries) -> u64 {
    let mut hash = Fnv::default();
    hash.bytes(b"scrolls world\0");
    hash.bytes(&GENERATOR_VERSION.to_le_bytes());
    hash.ron(params);
    for (_, name, def) in registries.biomes.iter() {
        hash.bytes(b"biome\0");
        hash.label(name.as_ref());
        // The colour grade is the client's look (lighting plan phase 4a),
        // not the world: a client on another grade stands on the same
        // ground. Neutral, it is not written, so the text is the one
        // hashed before grades existed.
        hash.ron(&strata_registry::BiomeDef {
            grade: strata_registry::Grade::NEUTRAL,
            ..def.clone()
        });
    }
    for (_, name, def) in registries.flora.iter() {
        // A dressing species is the client's alone (`FloraDef::dressing`):
        // no collider, and it takes no trunk's or rock's place, so the
        // ground and everything on it a server simulates are the same with
        // it or without. The tree library's version covers its baked props.
        if def.dressing {
            continue;
        }
        hash.bytes(b"flora\0");
        hash.label(name.as_ref());
        hash.ron(def);
    }
    for (_, name, def) in registries.provinces.iter() {
        hash.bytes(b"province\0");
        hash.label(name.as_ref());
        hash.ron(def);
    }
    for def in &registries.settlements {
        hash.bytes(b"settlement\0");
        hash.ron(def);
    }
    // The site kinds (ADR 0012): the sites they place are in the canary
    // networks' hashes below, as the rivers are. A set with none writes
    // nothing, so a world without sites hashes as one did before them.
    for (_, name, def) in registries.sites.iter() {
        hash.bytes(b"site\0");
        hash.label(name.as_ref());
        hash.ron(def);
    }
    let canary =
        crate::ProceduralWorld::with_params(strata_core::Seed(CANARY_SEED), registries, *params);
    // The samples read their rivers from the canary window alone, a cache
    // that never bakes: a handshake that baked the masses under the points
    // would cost seconds (ADR 0010). Points off the window are dry.
    let window = std::sync::Arc::new(crate::hydrology::canary_window(&canary));
    let sampled = canary.with_hydrology(std::sync::Arc::clone(&window));
    for (x, z) in CANARY_POINTS.iter().chain(&CARVE_POINTS) {
        hash.bytes(b"canary\0");
        hash.ron(&crate::WorldSource::sample(&sampled, *x, *z));
    }
    // The roads' grade (ticket 0005, step 6): the networks' hashes below
    // hold every road's profile, but not the ground a sample makes of it
    // (the carriageway, the verge, the banks) nor what it records of it,
    // which a sample's text leaves out. Across the window's first graded
    // road, at its middle, out past its banks' reach: the height, the cut
    // or fill and the cleared and developed shares.
    if let Some(road) = window
        .networks()
        .first()
        .and_then(|network| network.roads().iter().find(|road| road.graded()))
    {
        let mid = road.length_m() * 0.5;
        let k = road
            .points
            .partition_point(|p| p.along_m <= mid)
            .clamp(1, road.points.len() - 1);
        let (a, b) = (road.points[k - 1], road.points[k]);
        let (dx, dz) = (b.x - a.x, b.z - a.z);
        let l = (dx * dx + dz * dz).sqrt().max(1e-9);
        for off in GRADE_OFFSETS {
            let (x, z) = (a.x - dz / l * off, a.z + dx / l * off);
            let s = crate::WorldSource::sample(&sampled, x, z);
            hash.bytes(b"grade\0");
            hash.ron(&(
                s.height,
                s.landform.cut_m,
                s.landform.cleared,
                s.landform.developed,
                s.material,
            ));
        }
    }
    // The river network (ADR 0010): its numbers, and canary networks baked
    // on the canary world, since every mass would cost a handshake its
    // bakes.
    hash.bytes(b"rivers\0");
    hash.ron(&crate::hydrology::version_inputs(&canary, &window));
    hash.0
}

/// A hash of everything a world built by
/// [`ProceduralWorld::with_params`](crate::ProceduralWorld::with_params)
/// reads that its river network depends on: the seed, the parameters, the
/// biomes, the provinces and the settlements. Two worlds with the same key
/// in one process share one network cache (`hydrology::Hydrology::shared`),
/// so a test binary or a tool that builds the same world twice bakes it
/// once.
pub(crate) fn network_key(
    seed: strata_core::Seed,
    params: &WorldParams,
    registries: &GameRegistries,
) -> u64 {
    let mut hash = Fnv::default();
    hash.bytes(&seed.0.to_le_bytes());
    hash.ron(params);
    for (_, name, def) in registries.biomes.iter() {
        hash.label(name.as_ref());
        hash.ron(def);
    }
    for (_, name, def) in registries.provinces.iter() {
        hash.label(name.as_ref());
        hash.ron(def);
    }
    for def in &registries.settlements {
        hash.ron(def);
    }
    // A mass's sites are placed in its bake (ADR 0012).
    for (_, name, def) in registries.sites.iter() {
        hash.label(name.as_ref());
        hash.ron(def);
    }
    hash.0
}

/// What a baked rivers file is stamped with
/// ([`Hydrology::save`](crate::hydrology::Hydrology::save)): the network
/// key, which holds everything the bake reads, the generator's version and
/// the hydrology's fixed numbers. Not [`world_version`], whose canary bakes would cost a browser
/// the seconds the file is there to save.
#[must_use]
pub fn water_stamp(
    seed: strata_core::Seed,
    params: &WorldParams,
    registries: &GameRegistries,
) -> u64 {
    let mut hash = Fnv(network_key(seed, params, registries));
    hash.bytes(&GENERATOR_VERSION.to_le_bytes());
    // The hydrology's own fixed numbers, which `world_version` also folds
    // in: a bundle from a checkout that moved one without a version bump is
    // refused here too.
    hash.ron(&crate::hydrology::fixed_inputs());
    hash.0
}

/// FNV-1a, 64 bits.
struct Fnv(u64);

impl Default for Fnv {
    fn default() -> Self {
        Self(0xcbf2_9ce4_8422_2325)
    }
}

impl Fnv {
    fn bytes(&mut self, bytes: &[u8]) {
        for &b in bytes {
            self.0 ^= u64::from(b);
            self.0 = self.0.wrapping_mul(0x0000_0100_0000_01b3);
        }
    }

    /// A data id or text naming them, as the world reads it
    /// ([`strata_core::stable_label`]).
    fn label(&mut self, text: &str) {
        self.bytes(strata_core::stable_label(text).as_bytes());
    }

    /// The value's RON text, then a separator so two adjacent values
    /// cannot run into each other.
    fn ron(&mut self, value: &impl Serialize) {
        let text = ron::to_string(value).expect("world inputs serialise to RON");
        self.label(&text);
        self.bytes(b"\0");
    }
}
