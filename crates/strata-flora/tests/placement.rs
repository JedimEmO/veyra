//! Placement is a pure function of the world: the same bits from any
//! thread and any number of independently built tables, and every plant
//! obeys its species' limits.
#![allow(
    clippy::items_after_statements,
    reason = "each test takes the serial lock before its local helpers"
)]

mod common;

use proptest::prelude::*;
use rayon::prelude::*;
use strata_core::{RegionCoord, Seed};
use strata_flora::{CellFlora, GRID, HeightGrid, place_cell};
use strata_registry::FloraKind;
use strata_worldgen::WorldSource;

/// One test of this file at a time. Run concurrently, this binary crashes
/// with a corrupted stack on the machine these tests were written on, in
/// debug builds only; valgrind and the address sanitiser find nothing, it does
/// the same at older commits and with older compilers, and a larger stack
/// or fewer threads makes it vanish (see `docs/WORLD_SLICE_PLAN.md`, Phase 9). The
/// tests' own parallelism (the rayon one) is kept; only their overlap goes.
fn serial() -> std::sync::MutexGuard<'static, ()> {
    static LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
    LOCK.lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

fn cell(seed: u64, x: i32, z: i32) -> CellFlora {
    let (world, table) = common::world_and_table(seed);
    let mut scratch = Vec::new();
    place_cell(
        RegionCoord::new(x, z, 0),
        &world,
        &table,
        Seed(seed),
        &mut scratch,
    )
}

/// Bitwise, so `-0.0` and `0.0` do not pass for each other.
fn same_bits(a: &CellFlora, b: &CellFlora) -> bool {
    a.coord == b.coord
        && a.grass == b.grass
        && a.plants.len() == b.plants.len()
        && a.plants.iter().zip(&b.plants).all(|(p, q)| {
            p.species == q.species
                && p.variant == q.variant
                && [p.x, p.y, p.z, p.yaw, p.scale]
                    .iter()
                    .zip([q.x, q.y, q.z, q.yaw, q.scale].iter())
                    .all(|(u, v)| u.to_bits() == v.to_bits())
        })
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(24))]

    #[test]
    fn two_tables_from_one_seed_place_the_same_bits(x in -400i32..400, z in -400i32..400) {
        let _serial = serial();
        prop_assert!(same_bits(&cell(42, x, z), &cell(42, x, z)));
    }
}

#[test]
fn parallel_placement_matches_serial_placement() {
    let _serial = serial();
    let coords: Vec<RegionCoord> = (-3..3)
        .flat_map(|z| (-3..3).map(move |x| RegionCoord::new(x - 8, z - 4, 0)))
        .collect();
    let (world, table) = common::world_and_table(42);
    let serial: Vec<CellFlora> = coords
        .iter()
        .map(|c| place_cell(*c, &world, &table, Seed(42), &mut Vec::new()))
        .collect();
    let parallel: Vec<CellFlora> = common::pool().install(|| {
        coords
            .par_iter()
            .map(|c| place_cell(*c, &world, &table, Seed(42), &mut Vec::new()))
            .collect()
    });
    assert!(serial.iter().zip(&parallel).all(|(a, b)| same_bits(a, b)));
}

#[test]
fn plants_obey_their_species_limits_and_keep_their_distance() {
    let _serial = serial();
    let (world, table) = common::world_and_table(42);
    let mut scratch = Vec::new();
    let mut seen_trees = 0;
    let mut seen_grass = 0;
    // The default spawn area is dry land near the origin, and it grows
    // grass. It was coast, where no tree species grows, and the trees were
    // counted there until ticket 0004's step 2b moved the climate so that
    // Port Serein's country is temperate: the forest now reaches into the
    // spawn's blend (a share of 0.39 at its `-x` corner) and one tree grows.
    // The treeless coast is asked of a block of the coast about 1.5 km
    // towards `-x` and 3.5 km towards `-z` of the city (x -2112 to -1728, z
    // -4288 to -3776) whose every sample (on a 16 m lattice) is coast, sea
    // or desert and nothing a tree grows in, found by measurement.
    let spawn = (-10..-4, -6..2);
    let coast = (-33..-27, -67..-59);
    for (area, (xs, zs)) in [("spawn", spawn), ("coast", coast)] {
        for z in zs {
            for x in xs.clone() {
                let coord = RegionCoord::new(x, z, 0);
                let flora = place_cell(coord, &world, &table, Seed(42), &mut scratch);
                let grid = HeightGrid::new(&scratch);
                assert_eq!(scratch.len(), GRID * GRID);
                for p in &flora.plants {
                    let species = &table.species()[p.species.0 as usize];
                    let probe = grid.probe(p.x, p.z);
                    assert!(
                        p.y >= world.sea_level() + species.sea_clearance_m,
                        "{p:?} is in the sea"
                    );
                    assert!(probe.slope <= species.slope_max, "{p:?} is on a cliff");
                    assert!((probe.height - p.y).abs() < 1e-4, "{p:?} floats");
                    assert!(p.scale >= species.size.0 && p.scale <= species.size.1);
                    assert!(p.variant < species.variants);
                    if species.kind == FloraKind::Tree && area == "coast" {
                        seen_trees += 1;
                    }
                }
                for (a, p) in flora.plants.iter().enumerate() {
                    for q in &flora.plants[a + 1..] {
                        // A child stands by its parent, and children by each
                        // other; the grid's plants keep their distance.
                        if p.id.is_child() || q.id.is_child() {
                            continue;
                        }
                        let r = table.species()[p.species.0 as usize]
                            .exclusion_m
                            .max(table.species()[q.species.0 as usize].exclusion_m);
                        let d = ((p.x - q.x).powi(2) + (p.z - q.z).powi(2)).sqrt();
                        assert!(d >= r - 1e-4, "{p:?} and {q:?} are {d} m apart, under {r}");
                    }
                }
                for g in &flora.grass {
                    let [x, y, z] = g.root();
                    assert!(y >= world.sea_level(), "a blade in the sea");
                    assert!((0.0..=64.0).contains(&x) && (0.0..=64.0).contains(&z));
                    assert!(g.height_cm > 0);
                    assert!((grid.probe(x, z).height - y).abs() < 1e-5, "a blade floats");
                    assert!(
                        flora.plants.iter().filter(|p| !p.id.is_child()).all(|p| {
                            let radius = table.species()[p.species.0 as usize].exclusion_m * 0.5;
                            (p.x - x).powi(2) + (p.z - z).powi(2) >= radius * radius
                        }),
                        "a blade grows under a trunk"
                    );
                }
                if area == "spawn" {
                    seen_grass += flora.grass.len();
                }
            }
        }
    }
    assert!(
        seen_grass > 1000,
        "the spawn area grows grass: {seen_grass}"
    );
    assert_eq!(
        seen_trees, 0,
        "the coast block is coast, where no tree species grows"
    );

    // Trees grow where a tree species has weight: find the first wooded
    // forest cell on a coarse sweep and place it. A grove, not a clearing:
    // cover is what says a forest's ground is wooded here and open there
    // (ADR 0006), and since ticket 0004's step 2 moved the ranges the first
    // forest cell the sweep meets, (-210, -600), is a clearing at the foot
    // of a range (cover 0.25 at its centre, three trees).
    let registries = common::registries();
    let forest = registries
        .biome_id(&strata_registry::Namespaced::parse("strata:forest").expect("valid"))
        .expect("the shipped assets have a forest");
    let mut forest_cell = None;
    'sweep: for z in -80..80 {
        for x in -80..80 {
            let coord = RegionCoord::new(x * 10, z * 10, 0);
            let c = coord.center();
            let s = world.sample(c.x(), c.z());
            if s.biomes[0].0 == forest && s.biomes[0].1 > 0.9 && s.height > 5.0 && s.cover > 0.5 {
                forest_cell = Some(coord);
                break 'sweep;
            }
        }
    }
    let coord = forest_cell.expect("seed 42 has a forest within 50 km");
    let flora = place_cell(coord, &world, &table, Seed(42), &mut scratch);
    let trees = flora.trees(&table).count();
    assert!(
        trees > 5,
        "a wooded forest cell (cover over 0.5) grows trees: {trees} in {coord:?}"
    );
}

#[test]
fn seed_42_cell_snapshot() {
    let _serial = serial();
    // The coast `-x` of Port Serein, outside the ground the city shapes, so
    // the city's layout never empties it. Scrubland until ticket 0004's
    // step 2b (31 plants), whose temperate climate blends the forest into it
    // (a share of 0.56 at its centre, 160 plants; 163 since that step's
    // review faded the ranges nearer the coast).
    let flora = cell(42, -20, -4);
    let summary = (
        flora.coord,
        flora.plants.len(),
        flora.grass.len(),
        flora.plants.iter().take(6).copied().collect::<Vec<_>>(),
        flora.grass.iter().take(4).copied().collect::<Vec<_>>(),
    );
    insta::assert_ron_snapshot!(summary);
}

/// Pure forest ground on granite, flat and ten metres up, with the cover and
/// hydration a test asks for: everything the fit reads except the ground's
/// fertility is held still, so the counts differ only by cover.
struct Uniform {
    biome: strata_registry::BiomeId,
    cover: f32,
    hydration: f32,
}

impl WorldSource for Uniform {
    fn sample(&self, _x: f64, _z: f64) -> strata_worldgen::Sample {
        strata_worldgen::Sample {
            height: 10.0,
            biomes: [(self.biome, 1.0), (self.biome, 0.0)],
            provinces: [(granite(), 1.0), (granite(), 0.0)],
            material: 0,
            moisture: 0.7,
            hydration: self.hydration,
            cover: self.cover,
            landform: strata_worldgen::Landform::default(),
            water: None,
        }
    }

    fn sea_level(&self) -> f32 {
        0.0
    }
}

/// The province [`Uniform`] stands on: granite, whose kit dresses a forest
/// with the rocks the forest had before kits were chosen by province
/// (ticket 0004's step 3e), so these tests see the ground they were written
/// on. Loaded once, since a cell asks it of every sample.
fn granite() -> strata_registry::ProvinceId {
    static GRANITE: std::sync::OnceLock<strata_registry::ProvinceId> = std::sync::OnceLock::new();
    *GRANITE.get_or_init(|| {
        common::registries()
            .province_id(&strata_registry::Namespaced::parse("strata:granite").expect("valid"))
            .expect("the shipped assets have granite")
    })
}

fn forest_biome() -> strata_registry::BiomeId {
    common::registries()
        .biome_id(&strata_registry::Namespaced::parse("strata:forest").expect("valid"))
        .expect("the shipped assets have a forest")
}

#[test]
fn understory_adds_all_forms_without_moving_the_existing_plants() {
    let _serial = serial();
    let (_, table) = common::world_and_table(42);
    let old = strata_flora::FloraTable::from_species(
        table
            .species()
            .iter()
            .filter(|species| species.tree.form == "tree")
            .cloned()
            .collect(),
    );
    let world = Uniform {
        biome: forest_biome(),
        cover: 1.0,
        hydration: 1.0,
    };
    let mut forms = std::collections::BTreeSet::new();
    for x in 0..8 {
        let coord = RegionCoord::new(x, -2, 0);
        let before = place_cell(coord, &world, &old, Seed(42), &mut Vec::new());
        let after = place_cell(coord, &world, &table, Seed(42), &mut Vec::new());
        let retained: Vec<_> = after
            .plants
            .iter()
            .filter(|p| table.species()[p.species.0 as usize].tree.form == "tree")
            .copied()
            .collect();
        assert_eq!(before.plants, retained);
        for plant in &after.plants {
            let species = &table.species()[plant.species.0 as usize];
            if species.tree.form != "tree" {
                assert_eq!(
                    species.kind,
                    FloraKind::Bush,
                    "details must not gain trunk colliders"
                );
                assert_eq!(
                    species.trunk(plant),
                    None,
                    "decorative plants cannot collide"
                );
                forms.insert(species.tree.form.as_str());
            }
        }
    }
    assert_eq!(
        forms,
        ["fern", "flower", "log", "shrub"].into_iter().collect()
    );
}

#[test]
fn bare_ground_grows_fewer_trees_and_thinner_grass() {
    let _serial = serial();
    let (_, table) = common::world_and_table(42);
    let coord = RegionCoord::new(3, -2, 0);
    let mut scratch = Vec::new();
    let biome = forest_biome();
    let mut count = |cover: f32, hydration: f32| {
        let world = Uniform {
            biome,
            cover,
            hydration,
        };
        let flora = place_cell(coord, &world, &table, Seed(42), &mut scratch);
        (flora.trees(&table).count(), flora.grass.len())
    };
    let (rich_trees, rich_grass) = count(1.0, 1.0);
    let (bare_trees, bare_grass) = count(0.2, 1.0);
    assert!(rich_trees > 5, "full cover grows a forest: {rich_trees}");
    assert!(
        bare_trees < rich_trees,
        "a bare patch thins the trees: {bare_trees} against {rich_trees}"
    );
    assert!(
        bare_grass < rich_grass,
        "and the grass with them: {bare_grass} against {rich_grass}"
    );

    // Hydration thins the blades without touching the trees.
    let (wet_trees, wet_grass) = count(1.0, 1.0);
    let (dry_trees, dry_grass) = count(1.0, 0.0);
    assert_eq!(wet_trees, dry_trees, "hydration is not a tree's business");
    assert!(
        dry_grass * 2 < wet_grass,
        "parched ground keeps far fewer blades: {dry_grass} against {wet_grass}"
    );
}

#[test]
fn a_cover_floor_keeps_a_species_in_bare_ground() {
    let _serial = serial();
    use strata_flora::cover_gain;
    use strata_registry::FloraKind;

    // The floor is the share left at no cover, and full cover is full
    // chance whatever the floor.
    assert!((cover_gain(FloraKind::Tree, 0.0, 0.1) - 0.1).abs() < 1e-6);
    assert!((cover_gain(FloraKind::Tree, 1.0, 0.1) - 1.0).abs() < 1e-6);
    // Trees fall off faster than bushes, bushes faster than grass.
    let c = 0.4;
    let tree = cover_gain(FloraKind::Tree, c, 0.0);
    let bush = cover_gain(FloraKind::Bush, c, 0.0);
    let grass = cover_gain(FloraKind::Grass, c, 0.0);
    assert!(tree < bush && bush < grass, "{tree} {bush} {grass}");
    // The sqrt polynomial stands in for `cover^0.4`.
    for i in 0u8..=20 {
        let c = f32::from(i) / 20.0_f32;
        let want = c.powf(0.4);
        let have = cover_gain(FloraKind::Grass, c, 0.0);
        assert!((have - want).abs() < 7e-3, "at {c}: {have} against {want}");
    }
}

#[test]
fn roots_follow_the_terrain_triangles_instead_of_a_bilinear_saddle() {
    let _serial = serial();
    let world = Uniform {
        biome: forest_biome(),
        cover: 1.0,
        hydration: 1.0,
    };
    let mut samples = vec![world.sample(0.0, 0.0); GRID * GRID];
    samples[GRID + 1].height = 14.0;
    let grid = HeightGrid::new(&samples);
    // The lower triangle is flat despite the opposite corner being high.
    assert!((grid.probe(0.25, 0.25).height - 10.0).abs() < 1e-6);
    assert!((grid.probe(0.75, 0.75).height - 12.0).abs() < 1e-6);
    // Both triangles meet on the b-c diagonal.
    assert!((grid.probe(0.3, 0.7).height - 10.0).abs() < 1e-6);
}

#[test]
fn uniform_ground_has_local_patches_without_raising_the_blade_budget() {
    let _serial = serial();
    let (_, table) = common::world_and_table(42);
    let table = strata_flora::FloraTable::from_species(
        table
            .species()
            .iter()
            .filter(|s| s.kind == FloraKind::Grass)
            .cloned()
            .collect(),
    );
    let world = Uniform {
        biome: forest_biome(),
        cover: 1.0,
        hydration: 1.0,
    };
    let flora = place_cell(
        RegionCoord::new(-1, 2, 0),
        &world,
        &table,
        Seed(42),
        &mut Vec::new(),
    );
    let mut patches = [0usize; 64];
    for blade in &flora.grass {
        let x = usize::from(blade.x) * 8 / 65536;
        let z = usize::from(blade.z) * 8 / 65536;
        patches[z * 8 + x] += 1;
    }
    assert!(
        patches.iter().max().expect("64 patches")
            > &(patches.iter().min().expect("64 patches") * 2),
        "{patches:?}"
    );
    assert!(flora.grass.len() < 64 * 64 * 6);
    // Real tufts put several roots in a small footprint, not just a
    // smoothly varying probability over independently scattered stalks.
    let mut half_metre_bins = vec![0usize; 128 * 128];
    for blade in &flora.grass {
        let x = usize::from(blade.x) * 128 / 65536;
        let z = usize::from(blade.z) * 128 / 65536;
        half_metre_bins[z * 128 + x] += 1;
    }
    let clustered: usize = half_metre_bins.iter().filter(|&&n| n >= 3).sum();
    assert!(
        clustered * 2 > flora.grass.len(),
        "only {clustered} of {} roots share a tuft footprint",
        flora.grass.len()
    );
}

#[test]
fn registry_insertions_preserve_candidate_identity_shape_and_collision() {
    let _serial = serial();
    use strata_flora::FloraTable;
    use strata_registry::{Namespaced, OpenRegistries};
    let original = common::registries();
    let mut open = OpenRegistries::new();
    for (_, _, def) in original.materials.iter() {
        open.add_material(def.clone()).expect("valid fixture");
    }
    for (_, _, def) in original.biomes.iter() {
        open.add_biome(def.clone()).expect("valid fixture");
    }
    for (_, _, def) in original.flora.iter() {
        open.add_flora(def.clone()).expect("valid fixture");
    }
    // An earlier-sorting tree in another habitat changes both dense IDs and
    // tree enumeration, but cannot affect competition on our forest floor.
    let mut unrelated = original
        .flora
        .iter()
        .find(|(_, _, d)| d.kind == FloraKind::Tree)
        .expect("valid fixture")
        .2
        .clone();
    unrelated.id = Namespaced::parse("aaa:unrelated_tree").expect("valid fixture");
    for weight in &mut unrelated.biomes {
        weight.weight = 0.0;
    }
    open.add_flora(unrelated).expect("valid fixture");
    let extended = open.freeze().expect("valid fixture");
    let before_table = FloraTable::from_registries(&original);
    let after_table = FloraTable::from_registries(&extended);
    let world = Uniform {
        biome: forest_biome(),
        cover: 1.0,
        hydration: 1.0,
    };
    let coord = RegionCoord::new(-1, 0, 0);
    let before = place_cell(coord, &world, &before_table, Seed(42), &mut Vec::new());
    let after = place_cell(coord, &world, &after_table, Seed(42), &mut Vec::new());
    assert!(!before.plants.is_empty());
    assert_eq!(before.plants.len(), after.plants.len());
    for (p, q) in before.plants.iter().zip(&after.plants) {
        let a = &before_table.species()[p.species.0 as usize];
        let b = &after_table.species()[q.species.0 as usize];
        assert_ne!(p.species, q.species);
        assert_eq!(a.species_seed, b.species_seed);
        assert_eq!(p.id, q.id);
        let mut normalized = *q;
        normalized.species = p.species;
        assert_eq!(*p, normalized);
        assert_eq!(a.trunk(p), b.trunk(q));
    }
    assert_eq!(before.grass, after.grass);
}

#[test]
fn neighboring_cells_keep_plants_and_grass_clear_across_negative_boundaries() {
    let _serial = serial();
    let (_, table) = common::world_and_table(42);
    let world = Uniform {
        biome: forest_biome(),
        cover: 1.0,
        hydration: 1.0,
    };
    let cells: Vec<_> = (-1..=1)
        .flat_map(|z| (-1..=1).map(move |x| RegionCoord::new(x, z, 0)))
        .map(|c| place_cell(c, &world, &table, Seed(42), &mut Vec::new()))
        .collect();
    let mut cross_boundary_checks = 0;
    for a in &cells {
        for b in &cells {
            if a.coord == b.coord {
                continue;
            }
            for p in a.plants.iter().filter(|p| !p.id.is_child()) {
                let px = f64::from(a.coord.x) * 64.0 + f64::from(p.x);
                let pz = f64::from(a.coord.z) * 64.0 + f64::from(p.z);
                let radius = f64::from(table.species()[p.species.0 as usize].exclusion_m);
                for q in b.plants.iter().filter(|q| !q.id.is_child()) {
                    let qx = f64::from(b.coord.x) * 64.0 + f64::from(q.x);
                    let qz = f64::from(b.coord.z) * 64.0 + f64::from(q.z);
                    let r =
                        radius.max(f64::from(table.species()[q.species.0 as usize].exclusion_m));
                    assert!((px - qx).powi(2) + (pz - qz).powi(2) >= r * r);
                    if (px - qx).abs() < 8.0 && (pz - qz).abs() < 8.0 {
                        cross_boundary_checks += 1;
                    }
                }
                for g in &b.grass {
                    let [x, _, z] = g.root();
                    let gx = f64::from(b.coord.x) * 64.0 + f64::from(x);
                    let gz = f64::from(b.coord.z) * 64.0 + f64::from(z);
                    assert!((px - gx).powi(2) + (pz - gz).powi(2) >= radius * radius * 0.25 - 1e-5);
                }
            }
        }
    }
    assert!(cross_boundary_checks > 0);
    let ids: std::collections::HashSet<_> = cells
        .iter()
        .flat_map(|c| c.plants.iter().map(|p| p.id))
        .collect();
    assert_eq!(
        ids.len(),
        cells.iter().map(|c| c.plants.len()).sum::<usize>()
    );
}

/// The pair correlation of the forest's trees at short range: pairs within
/// `near` metres over the pairs a uniform scatter of the same count would
/// give. One is uniform; above one is clumped.
#[allow(clippy::cast_precision_loss, reason = "a few thousand trees")]
fn tree_clumping(table: &strata_flora::FloraTable, near: f64) -> (f64, usize) {
    let world = Uniform {
        biome: forest_biome(),
        cover: 1.0,
        hydration: 1.0,
    };
    let mut trees = Vec::new();
    for z in 0..8 {
        for x in 0..8 {
            let cell = place_cell(
                RegionCoord::new(x, z, 0),
                &world,
                table,
                Seed(42),
                &mut Vec::new(),
            );
            for p in cell
                .plants
                .iter()
                .filter(|p| table.species()[p.species.0 as usize].kind == FloraKind::Tree)
            {
                trees.push((
                    f64::from(x) * 64.0 + f64::from(p.x),
                    f64::from(z) * 64.0 + f64::from(p.z),
                ));
            }
        }
    }
    let area = 512.0 * 512.0;
    let n = trees.len() as f64;
    let mut pairs = 0.0;
    for (i, a) in trees.iter().enumerate() {
        for b in &trees[i + 1..] {
            let d2 = (a.0 - b.0).powi(2) + (a.1 - b.1).powi(2);
            if d2 < near * near {
                pairs += 1.0;
            }
        }
    }
    let expected = n * (n - 1.0) / 2.0 * std::f64::consts::PI * near * near / area;
    (pairs / expected, trees.len())
}

#[test]
fn forest_trees_gather_into_groves_where_they_once_stood_evenly() {
    let _serial = serial();
    let (_, table) = common::world_and_table(42);
    let unclumped = strata_flora::FloraTable::from_species(
        table
            .species()
            .iter()
            .cloned()
            .map(|mut s| {
                s.cluster = strata_registry::Clustering::default();
                s
            })
            .collect(),
    );
    let (clumped, count) = tree_clumping(&table, 20.0);
    let (even, even_count) = tree_clumping(&unclumped, 20.0);
    eprintln!(
        "pair correlation within 20 m: clumped {clumped:.2} over {count} trees, \
         without clumps {even:.2} over {even_count}"
    );
    assert!(even < 1.15, "the grid alone is near uniform: {even:.2}");
    assert!(clumped > 1.3, "groves: {clumped:.2}");
    // 26 hectares of pure forest: groves, not a thinned-out wood.
    assert!(count > 1500, "{count} trees over 26 ha");
}

#[test]
#[ignore = "a probe: where the seed-42 world stands its cliffs"]
fn where_are_the_cliffs() {
    let _serial = serial();
    let (world, table) = common::world_and_table(42);
    let cliffs: Vec<_> = table
        .species()
        .iter()
        .filter(|s| s.cluster.radius_m == 0.0 && s.ground.face_downhill)
        .map(|s| s.id)
        .collect();
    let mut found = 0;
    for z in (-120..40).step_by(4) {
        for x in (280..440).step_by(4) {
            let cell = place_cell(
                RegionCoord::new(x, z, 0),
                &world,
                &table,
                Seed(42),
                &mut Vec::new(),
            );
            for p in cell.plants.iter().filter(|p| cliffs.contains(&p.species)) {
                println!(
                    "cliff at {} {} y {}",
                    f64::from(x) * 64.0 + f64::from(p.x),
                    f64::from(z) * 64.0 + f64::from(p.z),
                    p.y
                );
                found += 1;
            }
        }
    }
    println!("{found} cliffs");
}

/// The table as it placed before ticket 0004's step 3e: no species scaled
/// by a province, and the six rock species on the biome lists they had
/// then, when a kit's biomes said which rock as well as how rocky.
fn table_before_the_kits(table: &strata_flora::FloraTable) -> strata_flora::FloraTable {
    let registries = common::registries();
    let biome = |name: &str| {
        registries
            .biome_id(&strata_registry::Namespaced::parse(name).expect("valid"))
            .expect("shipped")
    };
    let old: [(&str, &[(&str, f32)]); 6] = [
        (
            "strata:granite_boulder",
            &[
                ("strata:forest", 0.06),
                ("strata:highlands", 0.18),
                ("strata:mountains", 0.22),
                ("strata:tundra", 0.12),
                ("strata:plains", 0.02),
            ],
        ),
        (
            "strata:granite_stone",
            &[
                ("strata:forest", 0.04),
                ("strata:highlands", 0.12),
                ("strata:mountains", 0.2),
                ("strata:tundra", 0.1),
            ],
        ),
        (
            "strata:granite_cliff",
            &[
                ("strata:mountains", 0.55),
                ("strata:highlands", 0.35),
                ("strata:tundra", 0.2),
                ("strata:forest", 0.1),
            ],
        ),
        (
            "strata:sandstone_boulder",
            &[
                ("strata:savanna", 0.1),
                ("strata:desert", 0.14),
                ("strata:coast", 0.08),
                ("strata:plains", 0.03),
            ],
        ),
        (
            "strata:sandstone_stone",
            &[
                ("strata:savanna", 0.06),
                ("strata:desert", 0.1),
                ("strata:coast", 0.05),
            ],
        ),
        (
            "strata:sandstone_cliff",
            &[
                ("strata:coast", 0.45),
                ("strata:desert", 0.4),
                ("strata:savanna", 0.3),
                ("strata:plains", 0.1),
            ],
        ),
    ];
    let biomes = registries.biomes.len();
    let species = table
        .species()
        .iter()
        .cloned()
        .map(|mut s| {
            let id = &registries.flora.get(s.id).expect("registered").id;
            let mut weights: Vec<f32> = (0..biomes)
                .map(|b| {
                    s.fit
                        .weight(strata_registry::BiomeId(u32::try_from(b).expect("few")))
                })
                .collect();
            if let Some((_, list)) = old.iter().find(|(name, _)| id.as_ref() == *name) {
                weights = vec![0.0; biomes];
                for (name, weight) in *list {
                    weights[biome(name).0 as usize] = *weight;
                }
            }
            s.fit = strata_registry::FloraFit::from_weights(weights);
            s
        })
        .collect();
    strata_flora::FloraTable::from_species(species)
}

#[test]
fn a_species_no_kit_names_is_placed_as_before_the_kits() {
    let _serial = serial();
    let (world, table) = common::world_and_table(42);
    let before = table_before_the_kits(&table);
    let rock_count = |flora: &CellFlora| {
        flora
            .plants
            .iter()
            .filter(|p| table.kind(p.species) == Some(FloraKind::Rock))
            .count()
    };

    // Every tree, bush and grass keeps its chance to the bit wherever a
    // province speaks: the two cells below, and step 3d's folded-granite and
    // granite-sandstone borders and its three-way point. A kit names only
    // rocks.
    let mut probed = 0;
    for (x, z) in [
        (-80, -320),
        (100, -800),
        (-530, -773),
        (697, 553),
        (492, 600),
    ] {
        let mut scratch = Vec::new();
        let grid = HeightGrid::sample(RegionCoord::new(x, z, 0), &world, &mut scratch);
        for (now, then) in table.species().iter().zip(before.species()) {
            assert_eq!(now.id, then.id);
            if now.fit.in_a_kit() {
                assert_eq!(now.kind, FloraKind::Rock, "a kit names only rocks");
                continue;
            }
            for j in 0u8..32 {
                for i in 0u8..32 {
                    let probe = grid.probe(f32::from(i) * 2.0 + 0.5, f32::from(j) * 2.0 + 0.5);
                    let a = strata_flora::fit_at(now, &probe, world.sea_level());
                    let b = strata_flora::fit_at(then, &probe, world.sea_level());
                    assert_eq!(a.to_bits(), b.to_bits(), "{:?} moved", now.id);
                    probed += 1;
                }
            }
        }
    }
    assert!(probed > 0);

    // A cell with no rock in reach, before or after: trees, understory and
    // grass on basalt highlands, placed to the bit as they were.
    let bare = RegionCoord::new(100, -800, 0);
    let (then, now) = (
        place_cell(bare, &world, &before, Seed(42), &mut Vec::new()),
        place_cell(bare, &world, &table, Seed(42), &mut Vec::new()),
    );
    assert_eq!(
        (rock_count(&then), rock_count(&now)),
        (0, 0),
        "no rock in reach"
    );
    assert!(now.trees(&table).count() > 10 && now.grass.len() > 1000);
    assert!(same_bits(&then, &now), "the rockless cell moved");

    // A cell with rocks: granite forest round Port Serein's country, where
    // granite's kit weighs one and its biome weights did not move, so the
    // rocks stand where they stood and everything round them with them.
    let rocky = RegionCoord::new(-80, -320, 0);
    let (then, now) = (
        place_cell(rocky, &world, &before, Seed(42), &mut Vec::new()),
        place_cell(rocky, &world, &table, Seed(42), &mut Vec::new()),
    );
    assert!(rock_count(&now) > 0, "granite stands in the granite forest");
    assert!(now.trees(&table).count() > 10 && now.grass.len() > 1000);
    assert!(same_bits(&then, &now), "the granite forest moved");
}

/// Where one province alone speaks, every rock is of a species its kit
/// names: granite and sandstone country their own, karst and folded rock
/// granite's, basalt granite's stones alone and badlands sandstone stones.
#[test]
fn each_province_wears_its_kit() {
    let _serial = serial();
    let (world, table) = common::world_and_table(42);
    let registries = common::registries();
    // Step 3d's viewpoints: one a province, then two borders and a
    // three-way point (scratch/research/terrain/step3d/views.txt); and a
    // folded range's flank, since ticket 0004's step 4 moved the ranges
    // (read from the distance to their crest lines) and with them the
    // ground the range rocks take, so that step 3d's slate cliff at
    // (-50327, -42505) stands in a blend now.
    let views = [
        (-36000.0, -26000.0),
        (56516.0, 33389.0),
        (6152.0, -20144.0),
        (37311.0, 8572.0),
        (-31205.0, -50058.0),
        (-50327.0, -42505.0),
        (-58917.0, -49786.0),
        (-33910.0, -49437.0),
        (44611.0, 35430.0),
        (31511.0, 38411.0),
    ];
    let mut seen: std::collections::BTreeMap<(String, String), usize> =
        std::collections::BTreeMap::default();
    for (x, z) in views {
        let centre = RegionCoord::containing(strata_core::WorldPos::new(x, 0.0, z), 0);
        for dz in -2..=2 {
            for dx in -2..=2 {
                let coord = RegionCoord::new(centre.x + dx, centre.z + dz, 0);
                let mut scratch = Vec::new();
                let flora = place_cell(coord, &world, &table, Seed(42), &mut scratch);
                let grid = HeightGrid::new(&scratch);
                for p in &flora.plants {
                    let species = &table.species()[p.species.0 as usize];
                    if species.kind != FloraKind::Rock {
                        continue;
                    }
                    let [(province, share), _] = grid.probe(p.x, p.z).sample.provinces;
                    if share < 1.0 {
                        continue;
                    }
                    assert!(
                        species.fit.province_weight(province) > 0.0,
                        "{:?} stands in {:?}, whose kit does not name it",
                        species.id,
                        province
                    );
                    let name = |id: Option<&strata_registry::Namespaced>| {
                        id.map_or(String::new(), ToString::to_string)
                    };
                    *seen
                        .entry((
                            name(registries.provinces.get(province).map(|d| &d.id)),
                            name(registries.flora.get(p.species).map(|d| &d.id)),
                        ))
                        .or_default() += 1;
                }
            }
        }
    }
    eprintln!("rocks by province and species where one province speaks: {seen:#?}");
    for (province, species) in [
        ("strata:granite", "strata:granite_stone"),
        ("strata:sandstone", "strata:sandstone_stone"),
        ("strata:folded", "strata:granite_stone"),
    ] {
        assert!(
            seen.contains_key(&(province.to_owned(), species.to_owned())),
            "{province} wears {species}"
        );
    }
}

/// A kit's weight is shared across a border by the provinces' shares, and a
/// sample that names no province leaves the choice to the biome.
#[test]
fn a_province_gain_is_its_kits_by_their_shares() {
    use strata_flora::province_gain;
    let registries = common::registries();
    let province = |name: &str| {
        registries
            .province_id(&strata_registry::Namespaced::parse(name).expect("valid"))
            .expect("shipped")
    };
    let boulder = registries
        .flora_fit(
            registries
                .flora
                .id_of(
                    &strata_registry::Namespaced::parse("strata:granite_boulder").expect("valid"),
                )
                .expect("shipped"),
        )
        .expect("resolved");
    let at = |provinces: [(strata_registry::ProvinceId, f32); 2]| {
        let sample = strata_worldgen::Sample {
            height: 10.0,
            biomes: [(forest_biome(), 1.0), (forest_biome(), 0.0)],
            provinces,
            material: 0,
            moisture: 0.5,
            hydration: 1.0,
            cover: 1.0,
            landform: strata_worldgen::Landform::default(),
            water: None,
        };
        province_gain(boulder, &sample)
    };
    let (granite, karst, basalt, sandstone) = (
        province("strata:granite"),
        province("strata:karst"),
        province("strata:basalt"),
        province("strata:sandstone"),
    );
    assert!(boulder.in_a_kit());
    assert_eq!(
        at([(granite, 1.0), (granite, 0.0)]).to_bits(),
        1.0f32.to_bits()
    );
    assert_eq!(
        at([(sandstone, 1.0), (sandstone, 0.0)]).to_bits(),
        0.0f32.to_bits()
    );
    // Karst borrows granite's boulders at 0.6; basalt has none.
    assert!((at([(karst, 0.5), (basalt, 0.5)]) - 0.3).abs() < 1e-6);
    assert_eq!(
        at(strata_worldgen::Sample::NO_PROVINCES).to_bits(),
        1.0f32.to_bits()
    );
}

/// Nothing is placed in a river or a lake, nor nearer its surface than a
/// species keeps from the sea, read from the sample as the server reads it
/// (ticket 0005, step 2); the same ground with no water near keeps its
/// chance.
#[test]
fn nothing_stands_in_water() {
    let (_, table) = common::world_and_table(42);
    let ground =
        |height: f32, water: Option<strata_worldgen::SampleWater>| strata_worldgen::Sample {
            height,
            biomes: [(forest_biome(), 1.0), (forest_biome(), 0.0)],
            provinces: strata_worldgen::Sample::NO_PROVINCES,
            material: 0,
            moisture: 0.6,
            hydration: 0.8,
            cover: 1.0,
            landform: strata_worldgen::Landform::default(),
            water,
        };
    let river = strata_worldgen::SampleWater {
        surface_m: 50.0,
        bed_m: 49.5,
        width_m: 4.0,
        signed_distance_m: 1.0,
        flow: [0.5, 0.0],
        discharge_m3s: 0.8,
        order: 1,
        kind: strata_worldgen::hydrology::WaterKind::Channel,
        cut_dm: 0,
    };
    let fit = |sample: &strata_worldgen::Sample, species: &strata_flora::Species| {
        let probe = strata_flora::Probe {
            height: sample.height,
            normal: [0.0, 1.0, 0.0],
            slope: 0.0,
            sample,
        };
        strata_flora::fit_at(species, &probe, 0.0)
    };
    let mut growing = 0;
    for species in table.species() {
        let dry = ground(49.6, None);
        if fit(&dry, species) <= 0.0 {
            continue;
        }
        growing += 1;
        // Under the surface: in the channel.
        assert!(
            fit(&ground(49.6, Some(river)), species) <= 0.0,
            "{:?}",
            species.id
        );
        // On a bank closer to the surface than the species keeps from the
        // sea, and well clear of it.
        let bank = ground(50.0 + species.sea_clearance_m * 0.5, Some(river));
        if species.sea_clearance_m > 0.0 {
            assert!(fit(&bank, species) <= 0.0, "{:?}", species.id);
        }
        let clear = ground(50.0 + species.sea_clearance_m + 1.0, Some(river));
        assert!(fit(&clear, species) > 0.0, "{:?}", species.id);
    }
    assert!(growing > 3, "{growing} species grow on the test's ground");
}

/// A cliff kit's piece holds to a natural cliff up to its own limit, but
/// on a wall a river's valley cut nothing is placed past the walkable
/// slope (ticket 0005, step 3): the gorge's smooth walls had boulders
/// stuck to them. The same steep ground with no river near keeps them, and
/// so does a natural cliff inside the valley that the carve did not cut.
#[test]
fn nothing_clings_to_a_carved_wall() {
    let (_, table) = common::world_and_table(42);
    let wall = |slope: f32, water: Option<strata_worldgen::SampleWater>| strata_worldgen::Sample {
        height: 120.0,
        biomes: [(mountain_biome(), 1.0), (mountain_biome(), 0.0)],
        provinces: strata_worldgen::Sample::NO_PROVINCES,
        material: 0,
        moisture: 0.5,
        hydration: 0.5,
        cover: 1.0,
        landform: strata_worldgen::Landform {
            slope,
            ..strata_worldgen::Landform::default()
        },
        water,
    };
    let uncut = strata_worldgen::SampleWater {
        surface_m: 40.0,
        bed_m: 39.4,
        width_m: 6.0,
        signed_distance_m: 60.0,
        flow: [0.6, 0.0],
        discharge_m3s: 2.0,
        order: 2,
        kind: strata_worldgen::hydrology::WaterKind::Channel,
        cut_dm: 0,
    };
    // The same point, the carve having taken 35 m off the hillside.
    let cut = strata_worldgen::SampleWater {
        cut_dm: 350,
        ..uncut
    };
    let fit = |sample: &strata_worldgen::Sample, species: &strata_flora::Species| {
        let probe = strata_flora::Probe {
            height: sample.height,
            normal: [0.0, 1.0, 0.0],
            slope: sample.landform.slope,
            sample,
        };
        strata_flora::fit_at(species, &probe, 0.0)
    };
    let steep = strata_flora::CARVED_WALL_SLOPE * 2.0;
    let mut clinging = 0;
    for species in table.species() {
        let natural = fit(&wall(steep, None), species);
        if natural <= 0.0 {
            continue;
        }
        clinging += 1;
        assert!(
            fit(&wall(steep, Some(cut)), species) <= 0.0,
            "{:?} stands on a carved wall",
            species.id
        );
        assert!(
            (fit(&wall(steep, Some(uncut)), species) - natural).abs() < 1e-6,
            "{:?} lost a natural cliff in a river's valley",
            species.id
        );
    }
    assert!(clinging > 0, "no species holds to a natural cliff");
}

fn mountain_biome() -> strata_registry::BiomeId {
    common::registries()
        .biome_id(&strata_registry::Namespaced::parse("strata:mountains").expect("valid"))
        .expect("the shipped assets have mountains")
}

/// Nothing grows or stands on a procedural site's pad (ticket 0005, step 4;
/// ADR 0009, decision 4): its ground is developed out to a skirt past the
/// pad, so no tree, bush, rock or blade of grass is placed within it. The
/// sites within 30 km of Port Serein, every cell their pads touch.
#[test]
fn nothing_grows_on_a_site_pad() {
    let _serial = serial();
    let (world, table) = common::world_and_table(42);
    let city = world.sites()[0].centre;
    let sites = world.sites_near(city[0], city[1], 30_000.0);
    assert!(sites.len() >= 20, "{} sites near the city", sites.len());
    let mut cells = 0;
    for site in sites.iter().take(40) {
        let reach = site.radius_m + strata_worldgen::sites::SKIRT_M;
        let span = |c: f64| {
            // Cell indices of a few kilometres' metres; exact.
            #[allow(clippy::cast_possible_truncation)]
            let (lo, hi) = (
                ((c - reach) / 64.0).floor() as i32,
                ((c + reach) / 64.0).floor() as i32,
            );
            lo..=hi
        };
        for cz in span(site.centre[1]) {
            for cx in span(site.centre[0]) {
                let coord = RegionCoord::new(cx, cz, 0);
                let flora = place_cell(coord, &world, &table, Seed(42), &mut Vec::new());
                let (ox, oz) = (f64::from(cx) * 64.0, f64::from(cz) * 64.0);
                let inside = |x: f64, z: f64| {
                    let (dx, dz) = (ox + x - site.centre[0], oz + z - site.centre[1]);
                    dx * dx + dz * dz < reach * reach
                };
                for plant in &flora.plants {
                    assert!(
                        !inside(f64::from(plant.x), f64::from(plant.z)),
                        "{:?} on {}'s pad",
                        plant.species,
                        site.name
                    );
                }
                for blade in &flora.grass {
                    let (x, z) = (
                        f64::from(blade.x) / 65_535.0 * 64.0,
                        f64::from(blade.z) / 65_535.0 * 64.0,
                    );
                    assert!(!inside(x, z), "grass on {}'s pad", site.name);
                }
                cells += 1;
            }
        }
    }
    assert!(cells > 50, "{cells} cells");
}

/// Nothing grows on a road's carriageway, and only grass on its verge
/// (ticket 0005, step 6): along the graded roads within 15 km of Port
/// Serein, every plant and blade in the cells their centrelines pass
/// through, by the sample at its point, as the server and the client read
/// it.
#[test]
fn nothing_grows_on_a_carriageway() {
    let _serial = serial();
    let (world, table) = common::world_and_table(42);
    let city = world.sites()[0].centre;
    let network = world
        .hydrology()
        .bake_area(&world, (city[0], city[1]), (city[0], city[1]))
        .into_iter()
        .max_by_key(|n| n.stats().nodes)
        .expect("the city's mass");
    let mut seen = std::collections::BTreeSet::new();
    for road in network.roads().iter().filter(|r| r.graded()) {
        for p in road.points.iter().step_by(8) {
            let (dx, dz) = (p.x - city[0], p.z - city[1]);
            if dx * dx + dz * dz > 15_000.0 * 15_000.0 {
                continue;
            }
            // Cell indices of tens of kilometres' metres; exact.
            #[allow(clippy::cast_possible_truncation)]
            seen.insert(((p.x / 64.0).floor() as i32, (p.z / 64.0).floor() as i32));
        }
    }
    assert!(seen.len() > 100, "{} cells on roads", seen.len());
    let (mut plants, mut on_verges) = (0, 0);
    for &(cx, cz) in seen.iter().take(400) {
        let flora = place_cell(
            RegionCoord::new(cx, cz, 0),
            &world,
            &table,
            Seed(42),
            &mut Vec::new(),
        );
        let (ox, oz) = (f64::from(cx) * 64.0, f64::from(cz) * 64.0);
        // Placement reads the cell's nearest metre-grid sample.
        let near = |x: f32, z: f32| {
            let (x, z) = (ox + f64::from(x.round()), oz + f64::from(z.round()));
            (x, z, world.sample(x, z))
        };
        for plant in &flora.plants {
            let (x, z, s) = near(plant.x, plant.z);
            let kind = table.species()[plant.species.0 as usize].kind;
            assert!(
                s.landform.developed <= 0.5,
                "{kind:?} on a carriageway at ({x:.1}, {z:.1})"
            );
            assert!(
                s.landform.cleared <= 0.5 || kind == FloraKind::Grass,
                "{kind:?} on a verge at ({x:.1}, {z:.1})"
            );
            plants += 1;
            on_verges += usize::from(s.landform.cleared > 0.5);
        }
        for blade in &flora.grass {
            #[allow(clippy::cast_possible_truncation, reason = "a cell-local metre")]
            let (x, z, s) = near(
                (f64::from(blade.x) / 65_535.0 * 64.0) as f32,
                (f64::from(blade.z) / 65_535.0 * 64.0) as f32,
            );
            assert!(
                s.landform.developed <= 0.5,
                "grass on a carriageway at ({x:.1}, {z:.1})"
            );
        }
    }
    assert!(
        plants > 500,
        "{plants} plants by the roads, {on_verges} on verges"
    );
}

/// The wet bank (ticket 0005, step 8): the reeds stand within their band
/// over a lake's or a wide river's water, thickest at its edge, and never
/// by a creek or a fall, nor away from water; nothing else that grows
/// stands on the wetted bed, though a stone may lie there.
#[test]
fn reeds_line_slow_water_and_nothing_else_grows_on_the_wetted_bed() {
    use strata_worldgen::hydrology::WaterKind;
    let table = strata_flora::FloraTable::from_registries(&common::registries());
    let reed = table
        .species()
        .iter()
        .find(|s| s.ground.bank_m > 0.0)
        .expect("the shipped reeds");
    let bank = |above: f32, width: f32, kind: WaterKind| strata_worldgen::Sample {
        height: 40.0 + above,
        biomes: [(forest_biome(), 1.0), (forest_biome(), 0.0)],
        provinces: strata_worldgen::Sample::NO_PROVINCES,
        material: 0,
        moisture: 0.6,
        hydration: 0.8,
        cover: 0.8,
        landform: strata_worldgen::Landform::default(),
        water: Some(strata_worldgen::SampleWater {
            surface_m: 40.0,
            bed_m: 39.0,
            width_m: width,
            signed_distance_m: 0.5 * width + 1.0,
            flow: [0.6, 0.0],
            discharge_m3s: 2.0,
            order: 2,
            kind,
            cut_dm: 0,
        }),
    };
    let fit = |sample: &strata_worldgen::Sample, species: &strata_flora::Species| {
        let probe = strata_flora::Probe {
            height: sample.height,
            normal: [0.0, 1.0, 0.0],
            slope: 0.0,
            sample,
        };
        strata_flora::fit_at(species, &probe, 0.0)
    };
    let edge = fit(&bank(0.1, 12.0, WaterKind::Channel), reed);
    let up = fit(&bank(0.6, 12.0, WaterKind::Channel), reed);
    assert!(edge > 0.0 && up > 0.0 && up < edge, "{edge} then {up}");
    assert!(fit(&bank(0.2, 30.0, WaterKind::Lake), reed) > 0.0);
    assert!(
        fit(&bank(0.9, 12.0, WaterKind::Channel), reed) <= 0.0,
        "over its band"
    );
    assert!(
        fit(&bank(0.2, 3.0, WaterKind::Channel), reed) <= 0.0,
        "a creek"
    );
    assert!(
        fit(&bank(0.2, 12.0, WaterKind::Fall), reed) <= 0.0,
        "a fall"
    );
    assert!(
        fit(&bank(-0.1, 12.0, WaterKind::Channel), reed) <= 0.0,
        "in the water"
    );
    let mut dry = bank(0.2, 12.0, WaterKind::Channel);
    dry.water = None;
    assert!(fit(&dry, reed) <= 0.0, "away from water");
    for species in table.species() {
        if species.ground.bank_m > 0.0 || species.kind == strata_registry::FloraKind::Rock {
            continue;
        }
        assert!(
            fit(&bank(0.3, 12.0, WaterKind::Channel), species) <= 0.0,
            "{:?} grows on the wetted bed",
            species.id
        );
    }
}

/// A reed keeps off a crossing's ground: from under a bridge's deck to
/// past its ends, a ford's way and its posts; every other plant, and a
/// reed off the crossing, stays where it was placed.
#[test]
fn reeds_keep_off_the_crossings() {
    use strata_worldgen::crossings::{CrossingId, ProceduralCrossing};
    use strata_worldgen::roads::{Crossing, CrossingKind, RoadClass};
    let table = strata_flora::FloraTable::from_registries(&common::registries());
    let reed = table
        .species()
        .iter()
        .find(|s| s.ground.bank_m > 0.0)
        .expect("the shipped reeds")
        .id;
    let other = table
        .species()
        .iter()
        .find(|s| s.kind == strata_registry::FloraKind::Tree)
        .expect("a tree")
        .id;
    let coord = strata_core::RegionCoord::new(0, 0, 0);
    let plant = |species, x: f32, z: f32| strata_flora::Plant {
        id: strata_flora::PlantId {
            species_key: 1,
            cell: coord,
            slot: [0, 0],
        },
        species,
        x,
        y: 0.0,
        z,
        yaw: 0.0,
        scale: 1.0,
        variant: 0,
        sky: 255,
    };
    // A service road's bridge up `+z` over x = 32, its gap 20..44.
    let bridge = ProceduralCrossing {
        id: CrossingId {
            mass: [0, 0],
            road: 0,
            index: 0,
        },
        class: RoadClass::Service,
        crossing: Crossing {
            kind: CrossingKind::Bridge,
            at: [32.0, 32.0],
            along_m: 32.0,
            bearing: [0.0, 1.0],
            span_m: 24.0,
            deck_m: 5.0,
            surface_m: 2.0,
            banks_m: [3.0, 3.0],
            width_m: 12.0,
            depth_m: 1.0,
            river: 0,
            abutments_m: [20.0, 44.0],
        },
        edges: [[32.0, 20.0], [32.0, 44.0]],
    };
    let mut flora = strata_flora::CellFlora {
        coord,
        plants: vec![
            plant(reed, 33.0, 32.0),
            plant(reed, 26.0, 16.0),
            plant(reed, 32.0, 52.0),
            plant(reed, 50.0, 32.0),
            plant(other, 33.0, 30.0),
        ],
        grass: Vec::new(),
    };
    strata_flora::clear_bank_plants_off_crossings(&mut flora, &table, &[bridge]);
    let left: Vec<[f32; 2]> = flora.plants.iter().map(|p| [p.x, p.z]).collect();
    assert_eq!(left, [[32.0, 52.0], [50.0, 32.0], [33.0, 30.0]]);
}

/// Dressing (`FloraDef::dressing`, the reeds) never moves another plant:
/// every plant but the dressing stands where a table with none (a
/// server's, [`FloraTable::without_dressing`]) puts it. Asked twice: round
/// a lake's shore with the shipped reeds, and over the forest by the spawn
/// with the reeds let off the bank and widened, where they crowd every
/// shrub, log and fern there is. That is what lets the world's version
/// leave a dressing species out.
#[test]
fn dressing_moves_no_other_plant() {
    let _serial = serial();
    let (world, shipped) = common::world_and_table(42);
    assert!(
        shipped.species().iter().any(|s| s.dressing),
        "the shipped reeds"
    );
    let everywhere = strata_flora::FloraTable::from_species(
        shipped
            .species()
            .iter()
            .cloned()
            .map(|mut s| {
                if s.dressing {
                    s.ground.bank_m = 0.0;
                    s.exclusion_m = 2.0;
                    s.cover_floor = 1.0;
                    s.cluster = strata_registry::Clustering::default();
                    s.moisture = (0.0, 1.0);
                }
                s
            })
            .collect(),
    );
    // The shore of the lake at (22 042, -4 762), and the spawn's forest.
    for (table, (cx, cz)) in [(&shipped, (344, -75)), (&everywhere, (-7, -2))] {
        let server = table.clone().without_dressing();
        let dressing = |p: &strata_flora::Plant| table.species()[p.species.0 as usize].dressing;
        let (mut reeds, mut others) = (0, 0);
        for dz in -4..4 {
            for dx in -4..4 {
                let coord = RegionCoord::new(cx + dx, cz + dz, 0);
                let place = |table| {
                    strata_flora::place_cell_with(
                        coord,
                        &world,
                        table,
                        Seed(42),
                        &mut Vec::new(),
                        false,
                    )
                };
                let (full, bare) = (place(table), place(&server));
                reeds += full.plants.iter().filter(|p| dressing(p)).count();
                let kept: Vec<_> = full.plants.iter().filter(|p| !dressing(p)).collect();
                assert!(bare.plants.iter().all(|p| !dressing(p)));
                assert_eq!(kept, bare.plants.iter().collect::<Vec<_>>(), "{coord:?}");
                others += kept.len();
            }
        }
        assert!(
            reeds > 100 && others > 100,
            "{reeds} reeds among {others} at ({cx}, {cz})"
        );
    }
}
