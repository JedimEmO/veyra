//! The province field (ticket 0004, step 3): pure, continuous, a whole
//! blend at every point; and its relief rules (step 4), which move heights
//! only where a province names a shape for a class, never on the sea floor
//! or the shore.

mod common;

use std::fmt::Write as _;

use proptest::prelude::*;
use rayon::prelude::*;
use strata_core::Seed;
use strata_registry::Namespaced;
use strata_worldgen::{ProceduralWorld, ProvinceMix, Sample, WorldSource};

use common::sweep;

fn mix(world: &ProceduralWorld, x: f64, z: f64) -> ProvinceMix {
    world
        .provinces(x, z)
        .expect("the shipped assets have geology")
}

/// FNV-1a, 64 bits, as `world_version` hashes.
struct Fnv(u64);

impl Fnv {
    fn new() -> Self {
        Self(0xcbf2_9ce4_8422_2325)
    }

    fn bytes(&mut self, bytes: &[u8]) {
        for &b in bytes {
            self.0 ^= u64::from(b);
            self.0 = self.0.wrapping_mul(0x0000_0100_0000_01b3);
        }
    }
}

/// The 201 by 201 lattice at 500 m round the origin that the world's
/// samples are hashed on (`z` outer, `x` fastest); step 3 recorded HEAD's
/// on it.
fn head_lattice() -> Vec<(f64, f64)> {
    (0..201 * 201)
        .map(|k| {
            (
                f64::from(k % 201 - 100) * 500.0,
                f64::from(k / 201 - 100) * 500.0,
            )
        })
        .collect()
}

/// The bits of a sample's height and landform, in the recorded order (the
/// bed, its phase and its hardness since ticket 0004's step 5).
fn shape_bits(s: &Sample) -> [u32; 10] {
    let l = s.landform;
    [
        s.height.to_bits(),
        l.slope.to_bits(),
        l.aspect[0].to_bits(),
        l.aspect[1].to_bits(),
        l.hollow.to_bits(),
        l.ridge.to_bits(),
        l.developed.to_bits(),
        l.bed.cast_unsigned(),
        l.bed_phase.to_bits(),
        u32::from(l.bed_hard),
    ]
}

/// FNV over every field of every lattice sample a world without geology
/// has: the shape, then moisture, hydration, cover, material, and the biome
/// pair. Recorded from HEAD (a0dab62) before step 3, which moved none of
/// it; re-pinned by step 4, which gave every range a foot (the ground
/// moved everywhere a range reaches), and by its review, which reads the
/// distance to a crest line from differences of the range field, and by
/// step 5, whose mountains' relief grows towards a range's crest line (and
/// whose landform's bed fields, zero here, joined the hash); then by the
/// finite settlement benches, which change the city samples; then by the
/// rivers' carve (ticket 0005, step 2), from `0x0e9a_9b2b_6f6d_403c`, at
/// the lattice points a river's valley or a lake reaches
/// (`0x09d1_12b6_78f6_5743` as step 2 was first built, before its review);
/// then by the rivers' shape (step 3b), from `0x5c92_2792_dee4_a8ed`;
/// then by the road's grade (step 6: its roads' banks and the valley walls'
/// wander), from `0xff5f_9970_99a7_dd1a`. Bless a change that moves the
/// ground on purpose by pasting the value the failure prints.
const PINNED_WHOLE: u64 = 0x5420_0e26_53c3_487c;
/// HEAD's terrain materials in their layer order. A material is hashed as
/// its layer in HEAD's set, found by name: step 3c added five rock layers,
/// which renumber the layers alphabetically without moving any sample's
/// material.
const HEAD_MATERIALS: [&str; 9] = [
    "strata:flagstones",
    "strata:forest_floor",
    "strata:grass",
    "strata:gravel",
    "strata:rock",
    "strata:sand",
    "strata:setts",
    "strata:snow",
    "strata:soil",
];

/// Points of the lattice that stood in a province border's blend, where
/// three provinces weighed, or in the range substitution's band on the
/// world step 3 was built on: read beside the lattice so that borders are
/// read whatever the lattice's spacing misses.
const BLEND_POINTS: [(f64, f64); 18] = [
    (3500.0, -49500.0),
    (-19500.0, -44500.0),
    (28000.0, 40500.0),
    (30000.0, 39500.0),
    (-19000.0, 3500.0),
    (12000.0, 48000.0),
    (-500.0, -48000.0),
    (16500.0, -26500.0),
    (41000.0, -33000.0),
    (-18000.0, 6500.0),
    (-24000.0, -45500.0),
    (-37500.0, 16000.0),
    (-48000.0, 0.0),
    (25500.0, 18000.0),
    (-48500.0, -2500.0),
    (-21500.0, -47000.0),
    (12000.0, -20500.0),
    (-23500.0, -47500.0),
];

/// A province that lists no relief rule is the ground without it (ticket
/// 0004, step 4): with every province's `relief` emptied, a world with the
/// field on has the height and landform of the world with no geology at
/// every point of the lattice and the blend points, to the bit. The class a
/// province does not list keeps the biome's own shape and numbers.
#[test]
fn a_province_with_no_rules_is_the_ground_without_one() {
    let bare = ProceduralWorld::new(Seed(42), registries_without_rules());
    let off = ProceduralWorld::new(Seed(42), registries_without_geology());
    let mut points = head_lattice();
    points.extend(BLEND_POINTS);
    let blends = sweep(|| {
        points
            .par_iter()
            .map(|&(x, z)| {
                // Before the carve: the two worlds route their rivers
                // apart (the provinces' water rules), and the carve is held
                // by `tests/carve.rs`.
                let (a, b) = (
                    bare.sample_before_carve(x, z),
                    off.sample_before_carve(x, z),
                );
                assert_eq!(
                    shape_bits(&a),
                    shape_bits(&b),
                    "height or landform at ({x}, {z})"
                );
                usize::from(mix(&bare, x, z).entries().len() > 1)
            })
            .sum::<usize>()
    });
    assert!(blends > 1_000, "only {blends} points in a blend");
}

/// A province with no beds (`strength` 0) leaves the ground to the bit
/// (ticket 0004, step 5): where every province weighing has none (granite,
/// which ships with none, alone or with another such), the shipped world
/// has the height and landform of the world with every province's beds
/// taken out; and where the beds shape the ground (the landform names a
/// bed), the height moves at 49 points in 50 or more (4 012 of 4 056 on
/// step 5's ground: the rest sit where the beds weigh a hair or at a bed's
/// boundary, where they add nothing a sample's `f32` shows).
#[test]
fn a_province_with_no_beds_is_the_ground_without_them() {
    let registries = common::registries();
    let shipped = world42();
    let bedless = ProceduralWorld::new(Seed(42), registries_without_beds());
    let unbedded = |id: strata_registry::ProvinceId| {
        registries
            .provinces
            .iter()
            .find(|(p, _, _)| *p == id)
            .is_some_and(|(_, _, def)| def.beds.strength == 0.0)
    };
    let (none, same, bedded, moved) = sweep(|| {
        head_lattice()
            .par_iter()
            .map(|&(x, z)| {
                let (a, b) = (
                    shipped.sample_before_carve(x, z),
                    bedless.sample_before_carve(x, z),
                );
                let m = mix(shipped, x, z);
                let without = m.entries().iter().all(|&(id, _)| unbedded(id));
                let with = a.landform.bed != 0 || a.landform.bed_phase != 0.0;
                (
                    usize::from(without),
                    usize::from(without && shape_bits(&a) == shape_bits(&b)),
                    usize::from(with),
                    usize::from(with && a.height.to_bits() != b.height.to_bits()),
                )
            })
            .reduce(
                || (0, 0, 0, 0),
                |p, q| (p.0 + q.0, p.1 + q.1, p.2 + q.2, p.3 + q.3),
            )
    });
    println!("{same} of {none} unbedded points kept, {moved} of {bedded} bedded points moved");
    assert!(
        none > 2_000 && same == none,
        "{same} of {none} points with no bedded province kept their ground"
    );
    assert!(
        bedded > 1_000 && moved * 50 >= bedded * 49,
        "{moved} of {bedded} bedded points moved"
    );
}

/// Where the beds stand a riser, the landform names it: the bed it gives
/// is the one that shaped the ground (the height before the gullies), so a
/// hard bed's riser is labelled hard and in its riser share, what the rock
/// kit stands its cliff pieces on. The review of step 5 found the label
/// read from the surface after the gullies, a bed or two lower: of 11 492
/// bed risers within 100 km, 2 533 were labelled a hard bed's riser.
///
/// A bed riser here is ground steeper than 45 degrees at a metre where the
/// beds' own metres ([`strata_worldgen::procedural::Terms::beds_m`]) rise
/// more than a half in a metre. Nine in ten must be labelled a hard bed's
/// riser; the rest stand where two provinces' beds blend (the label is the
/// heavier one's) or where a soft bed's eased slope is cut into ground
/// steeper than its fade reads.
#[test]
fn a_beds_riser_is_labelled_as_its_riser() {
    let world = world42();
    let riser = {
        let params = strata_worldgen::WorldParams::default();
        (1.5 / params.bed_steepening).clamp(params.bed_riser, 1.0)
    };
    let (risers, labelled) = sweep(|| {
        (0..400_000_u64)
            .into_par_iter()
            .filter_map(|k| {
                let unit = |n: u64| {
                    #[allow(clippy::cast_precision_loss, reason = "53 bits, exactly")]
                    let v = (strata_core::mix64(n) >> 11) as f64 / (1_u64 << 53) as f64;
                    v
                };
                let (x, z) = (
                    (unit(2 * k + 31) - 0.5) * 200_000.0,
                    (unit(2 * k + 32) - 0.5) * 200_000.0,
                );
                let s = world.sample(x, z);
                if s.height <= 2.0 || (s.landform.bed == 0 && s.landform.bed_phase == 0.0) {
                    return None;
                }
                let slope = |field: &dyn Fn(f64, f64) -> f64| {
                    let gx = field(x + 0.5, z) - field(x - 0.5, z);
                    let gz = field(x, z + 0.5) - field(x, z - 0.5);
                    (gx * gx + gz * gz).sqrt()
                };
                let beds = slope(&|px, pz| world.terms(px, pz).beds_m);
                let ground = slope(&|px, pz| world.terms(px, pz).height_m);
                (ground > 1.0 && beds > 0.5).then_some(usize::from(
                    s.landform.bed_hard && f64::from(s.landform.bed_phase) < riser,
                ))
            })
            .fold(|| (0, 0), |(n, l), v| (n + 1, l + v))
            .reduce(|| (0, 0), |p, q| (p.0 + q.0, p.1 + q.1))
    });
    println!("{labelled} of {risers} bed risers labelled a hard bed's riser");
    assert!(risers > 300, "only {risers} bed risers");
    assert!(
        labelled * 10 >= risers * 9,
        "{labelled} of {risers} bed risers labelled a hard bed's riser"
    );
}

/// A range's foot in a bedded province is banded in patches, not along
/// its whole length (the review of step 5): a range's own ramp opens the
/// beds' gate only where [`strata_worldgen::WorldParams::bed_patch`]'s
/// noise says, so bands come and go along the foot. Step 5 opened the gate
/// on all of the ramp, and the sandstone foot north-east of Port Serein
/// carried the same ten lines along its contours for 20 km.
///
/// Across that foot, 22 lines a kilometre apart from x 39 to 60 km, each
/// from z 28 to 34 km at 50 m: with the ramp read everywhere (the patches
/// wide open, step 5's gate) the beds move the ground by a metre or more
/// on every line; with the shipped patches fewer than 85 % of those
/// points keep a band, at least two lines of ten points or more keep under
/// a third of theirs and at least three keep all of them (on this step's
/// ground 355 of 472, two lines at 29 and 30 %, six whole): the bands
/// are broken, not gone.
#[test]
fn a_range_foot_is_bedded_in_patches() {
    let shipped = world42();
    let open = ProceduralWorld::with_params(
        Seed(42),
        &common::registries(),
        strata_worldgen::WorldParams {
            bed_patch: (-10.0, -9.0),
            ..strata_worldgen::WorldParams::default()
        },
    );
    // A band shows where the beds move the ground by a metre or more.
    let bedded = |w: &ProceduralWorld, x: f64, z: f64| w.terms(x, z).beds_m.abs() >= 1.0;
    let lines: Vec<(usize, usize)> = sweep(|| {
        (39..=60)
            .into_par_iter()
            .map(|km| {
                let x = f64::from(km) * 1_000.0;
                (0..=120)
                    .map(|i| 28_000.0 + f64::from(i) * 50.0)
                    .filter(|&z| bedded(&open, x, z))
                    .fold((0, 0), |(n, kept), z| {
                        (n + 1, kept + usize::from(bedded(shipped, x, z)))
                    })
            })
            .collect()
    });
    println!("(bedded with the ramp read everywhere, kept by the patches) per line: {lines:?}");
    assert!(
        lines.iter().all(|&(n, _)| n > 0),
        "the ramp read everywhere beds every line: {lines:?}"
    );
    let (all, kept) = lines
        .iter()
        .fold((0, 0), |(n, k), &(ln, lk)| (n + ln, k + lk));
    let thin = lines.iter().filter(|&&(n, k)| n >= 10 && k * 3 < n).count();
    let whole = lines.iter().filter(|&&(n, k)| n >= 10 && k == n).count();
    println!("{kept} of {all} banded points kept; {thin} lines under a third, {whole} whole");
    assert!(
        kept * 100 < all * 85 && thin >= 2 && whole >= 3,
        "{kept} of {all} banded points kept (under 85 % wanted), {thin} lines keep under a third (2 wanted), {whole} whole (3 wanted): {lines:?}"
    );
}

/// The rules shape only the classes they name, and never the sea's or the
/// shore's: where every biome in the blend is the ocean or the coast, the
/// shipped world has the height of the world without rules to the bit, and
/// where a province with a rule for the dominant biome's class stands alone
/// the height moves at nearly every point.
#[test]
fn the_rules_shape_the_land_and_leave_the_shore() {
    let registries = common::registries();
    let shaped = world42();
    let bare = ProceduralWorld::new(Seed(42), registries_without_rules());
    let biome = |name: &str| {
        registries
            .biome_id(&Namespaced::parse(name).expect("valid"))
            .expect("ships")
    };
    let (ocean, coast) = (biome("strata:ocean"), biome("strata:coast"));
    let listed = |s: &Sample| {
        let (id, w) = s.provinces[0];
        let class = registries
            .biomes
            .iter()
            .find(|(b, _, _)| *b == s.dominant_biome())
            .map(|(_, _, def)| def.relief.kind)
            .expect("a frozen biome");
        w >= 0.999
            && registries
                .province(id)
                .is_some_and(|traits| traits.rule(class).is_some())
    };
    let (shore, same, ruled, moved) = sweep(|| {
        head_lattice()
            .par_iter()
            .map(|&(x, z)| {
                let (a, b) = (
                    shaped.sample_before_carve(x, z),
                    bare.sample_before_carve(x, z),
                );
                let at_shore = a
                    .biomes
                    .iter()
                    .all(|(id, w)| *w == 0.0 || *id == ocean || *id == coast);
                let is_ruled = !at_shore && listed(&a) && a.biomes[0].1 >= 0.999;
                (
                    usize::from(at_shore),
                    usize::from(at_shore && a.height.to_bits() == b.height.to_bits()),
                    usize::from(is_ruled),
                    usize::from(is_ruled && a.height.to_bits() != b.height.to_bits()),
                )
            })
            .reduce(
                || (0, 0, 0, 0),
                |p, q| (p.0 + q.0, p.1 + q.1, p.2 + q.2, p.3 + q.3),
            )
    });
    assert!(
        shore > 10_000 && same == shore,
        "{same} of {shore} shore points kept their height"
    );
    assert!(
        ruled > 5_000 && moved * 100 >= ruled * 99,
        "{moved} of {ruled} ruled points moved"
    );
}

/// A province's shape changes the form of a rolling biome's relief (plains
/// and hills), not where its middle lies: where one province stands alone
/// under one such biome, the relief's mean over the lattice is within 20 m
/// of the world without rules'. And inland, where the continent field is
/// 0.06 or more, no ground stands under the sea. Step 4 centred a ridged,
/// billowed or cellular shape on 0.5, which is not its mean: the granite
/// and folded highlands stood 66 and 40 m lower on average, and a folded
/// trench at (22 895, -61 060) was cut to 45.6 m under the sea, where the
/// world before had nothing inland under 98.8 m (the review of step 4).
/// The means over the lattice, the sea over it and every 10 m round the
/// trench.
#[test]
fn a_shape_keeps_the_middle_of_the_relief_and_the_land_above_the_sea() {
    let registries = common::registries();
    let shaped = world42();
    let bare = ProceduralWorld::new(Seed(42), registries_without_rules());
    let rolling: Vec<strata_registry::BiomeId> = registries
        .biomes
        .iter()
        .filter(|(_, _, def)| {
            matches!(
                def.relief.kind,
                strata_registry::ReliefKind::Plains | strata_registry::ReliefKind::Hills
            )
        })
        .map(|(id, _, _)| id)
        .collect();
    let trench: Vec<(f64, f64)> = (0..41)
        .flat_map(|j| {
            (0..41).map(move |i| {
                (
                    22_750.0 + f64::from(i) * 10.0,
                    -61_250.0 + f64::from(j) * 10.0,
                )
            })
        })
        .collect();
    let lattice = head_lattice();
    let points: Vec<(f64, f64)> = lattice.iter().copied().chain(trench).collect();
    let (shifts, sunk): (
        Vec<(u32, strata_registry::BiomeId, f64)>,
        Vec<(f64, f64, f32)>,
    ) = sweep(|| {
        let shifts = lattice
            .par_iter()
            .filter_map(|&(x, z)| {
                let t = shaped.terms(x, z);
                let [(id, w)]: [(strata_registry::ProvinceId, f64); 1] =
                    mix(shaped, x, z).entries().try_into().ok()?;
                let [(biome, ..)] = t.biomes.as_slice() else {
                    return None;
                };
                (w >= 0.999 && rolling.contains(biome) && t.continent.land > 0.02)
                    .then(|| (id.0, *biome, t.relief_m - bare.terms(x, z).relief_m))
            })
            .collect();
        let sunk = points
            .par_iter()
            .filter(|&&(x, z)| shaped.continent(x, z).land >= 0.06)
            .filter_map(|&(x, z)| {
                let h = shaped.sample(x, z).height;
                (h <= 0.0).then_some((x, z, h))
            })
            .collect();
        (shifts, sunk)
    });
    assert!(
        sunk.is_empty(),
        "{} inland points under the sea, the first {:?}",
        sunk.len(),
        &sunk[..sunk.len().min(5)]
    );
    let mut groups: Vec<(u32, strata_registry::BiomeId)> =
        shifts.iter().map(|&(p, b, _)| (p, b)).collect();
    groups.sort_unstable_by_key(|&(p, b)| (p, b.0));
    groups.dedup();
    let mut report = String::new();
    let mut wrong = Vec::new();
    for (province, biome) in groups {
        let group: Vec<f64> = shifts
            .iter()
            .filter(|&&(p, b, _)| p == province && b == biome)
            .map(|&(.., d)| d)
            .collect();
        if group.len() < 100 {
            continue;
        }
        // A count of hundreds; exact in f64.
        #[allow(clippy::cast_precision_loss)]
        let mean = group.iter().sum::<f64>() / group.len() as f64;
        let _ = writeln!(
            report,
            "  province {province} biome {}: {} points, relief {mean:+.1} m",
            biome.0,
            group.len()
        );
        if mean.abs() >= 20.0 {
            wrong.push((province, biome.0, mean));
        }
    }
    println!("mean relief against the world without rules:\n{report}");
    assert!(
        wrong.is_empty(),
        "a shape moved the middle of a rolling relief (province, biome, metres): {wrong:?}\n{report}"
    );
}

/// Heights are continuous across a border between two provinces' shapes,
/// where two noises' heights blend (the weights alone are tested in
/// [`the_weights_are_continuous`]): from 150 points where a second province
/// weighs at least 0.3, lines of 600 m at a metre in four directions, every
/// metre that rises more than 1.5 m walked again by the centimetre; no
/// centimetre rises 0.3 m and no metre 8 m (`tests/continuity.rs`'s bound).
/// On step 4's ground this found 19 steps of 0.3 m or more within a
/// centimetre (the gullies' steer, see `continuity.rs`) and a metre rising
/// 14.2 m (the review of step 4); 5.6 m and none since.
#[test]
fn heights_are_continuous_across_a_border() {
    let world = world42();
    let unit = |k: u64| {
        // Top 53 bits of a mix, as a fraction in 0..1.
        #[allow(clippy::cast_precision_loss, reason = "53 bits, exact")]
        let fraction = (strata_core::mix64(k) >> 11) as f64 / (1_u64 << 53) as f64;
        fraction
    };
    let borders: Vec<(f64, f64)> = (0..400_000_u64)
        .map(|k| {
            (
                (unit(2 * k) * 2.0 - 1.0) * 90_000.0,
                (unit(2 * k + 1) * 2.0 - 1.0) * 90_000.0,
            )
        })
        .filter(|&(x, z)| {
            world.continent(x, z).land > 0.03 && {
                let m = mix(world, x, z);
                m.entries().len() >= 2 && m.entries()[1].1 >= 0.3
            }
        })
        .take(150)
        .collect();
    assert_eq!(borders.len(), 150, "too few border points");
    let height = |x: f64, z: f64| f64::from(world.sample(x, z).height);
    let (steepest, steps): (f64, Vec<(f64, f64, f64)>) = sweep(|| {
        borders
            .par_iter()
            .map(|&(x0, z0)| {
                let mut steepest = 0.0_f64;
                let mut steps = Vec::new();
                for (dx, dz) in [(1.0, 0.0), (0.0, 1.0), (0.6, 0.8), (0.8, -0.6)] {
                    let mut previous = height(x0 - dx * 300.0, z0 - dz * 300.0);
                    for k in 1..=600 {
                        let t = f64::from(k) - 300.0;
                        let here = height(x0 + dx * t, z0 + dz * t);
                        let rise = (here - previous).abs();
                        steepest = steepest.max(rise);
                        if rise > 1.5 {
                            let mut before = previous;
                            for j in 1..=100 {
                                let s = t - 1.0 + f64::from(j) * 0.01;
                                let (x, z) = (x0 + dx * s, z0 + dz * s);
                                let now = height(x, z);
                                if (now - before).abs() > 0.3 {
                                    steps.push((x, z, now - before));
                                }
                                before = now;
                            }
                        }
                        previous = here;
                    }
                }
                (steepest, steps)
            })
            .reduce(
                || (0.0, Vec::new()),
                |mut a, b| {
                    a.1.extend(b.1);
                    (a.0.max(b.0), a.1)
                },
            )
    });
    println!(
        "across 150 borders: steepest metre {steepest:.2} m, {} centimetre steps over 0.3 m",
        steps.len()
    );
    assert!(
        steps.is_empty(),
        "{} centimetre steps over 0.3 m across borders, the first {:?}",
        steps.len(),
        &steps[..steps.len().min(5)]
    );
    assert!(
        steepest < 8.0,
        "a metre across a border rose {steepest:.2} m"
    );
}

/// A copy of the shipped registries with no `geology/`, in a directory of
/// its own under the target dir.
fn copy_tree(from: &std::path::Path, to: &std::path::Path) {
    if !from.is_dir() {
        return;
    }
    std::fs::create_dir_all(to).expect("a scratch directory");
    for entry in std::fs::read_dir(from).expect("the shipped assets list") {
        let entry = entry.expect("a directory entry");
        let path = entry.path();
        if path.is_dir() {
            copy_tree(&path, &to.join(entry.file_name()));
        } else {
            std::fs::copy(&path, to.join(entry.file_name())).expect("a copy");
        }
    }
}

fn assets_without_geology() -> std::path::PathBuf {
    let source = std::path::Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/../../assets"));
    let target = std::path::Path::new(env!("CARGO_TARGET_TMPDIR")).join("assets-without-geology");
    let _ = std::fs::remove_dir_all(&target);
    for dir in ["biomes", "terrain", "flora", "settlements"] {
        copy_tree(&source.join(dir), &target.join(dir));
    }
    target
}

/// The shipped registries with no `geology/`, loaded once for the binary:
/// the tests that want them run on threads of their own, and each copying
/// the tree into one directory would pull it from under the others.
fn registries_without_geology() -> &'static strata_registry::GameRegistries {
    static REGISTRIES: std::sync::OnceLock<strata_registry::GameRegistries> =
        std::sync::OnceLock::new();
    REGISTRIES.get_or_init(|| {
        let assets = assets_without_geology();
        let registries = strata_registry::load_from_dir(&assets).expect("the copy loads");
        let _ = std::fs::remove_dir_all(&assets);
        registries
    })
}

/// A copy of the shipped registries whose provinces list no relief rule
/// and have no beds (ticket 0004, step 5: beds move heights too, and the
/// tests that want the ground without a province's shapes want it without
/// its beds), loaded once for the binary.
fn registries_without_rules() -> &'static strata_registry::GameRegistries {
    static REGISTRIES: std::sync::OnceLock<strata_registry::GameRegistries> =
        std::sync::OnceLock::new();
    REGISTRIES.get_or_init(|| edited_registries("assets-without-rules", true))
}

/// A copy of the shipped registries whose provinces have no beds and keep
/// their relief rules, loaded once for the binary.
fn registries_without_beds() -> &'static strata_registry::GameRegistries {
    static REGISTRIES: std::sync::OnceLock<strata_registry::GameRegistries> =
        std::sync::OnceLock::new();
    REGISTRIES.get_or_init(|| edited_registries("assets-without-beds", false))
}

/// The shipped registries with every province's beds taken out and, if
/// `rules`, its relief rules too, copied to `name` under the target dir.
fn edited_registries(name: &str, rules: bool) -> strata_registry::GameRegistries {
    {
        let source = std::path::Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/../../assets"));
        let target = std::path::Path::new(env!("CARGO_TARGET_TMPDIR")).join(name);
        let _ = std::fs::remove_dir_all(&target);
        for dir in ["biomes", "terrain", "flora", "settlements", "geology"] {
            copy_tree(&source.join(dir), &target.join(dir));
        }
        for entry in std::fs::read_dir(target.join("geology")).expect("the copy lists") {
            let path = entry.expect("a directory entry").path();
            if path.extension().is_none_or(|e| e != "ron") {
                continue;
            }
            let text = std::fs::read_to_string(&path).expect("a province file");
            let mut out = String::new();
            // What a skipped block ends with: a rule list's `],` or a beds
            // block's `),`.
            let mut skipping: Option<&str> = None;
            for line in text.lines() {
                let trimmed = line.trim_start();
                if let Some(end) = skipping {
                    if trimmed == end {
                        skipping = None;
                    }
                } else if rules && trimmed.starts_with("relief: [") {
                    out.push_str("    relief: [],\n");
                    skipping = Some("],");
                } else if trimmed.starts_with("beds: (") {
                    out.push_str("    beds: (strength: 0.0),\n");
                    if !trimmed.ends_with("),") {
                        skipping = Some("),");
                    }
                } else {
                    out.push_str(line);
                    out.push('\n');
                }
            }
            std::fs::write(&path, out).expect("the copy is writable");
        }
        let registries = strata_registry::load_from_dir(&target).expect("the copy loads");
        let _ = std::fs::remove_dir_all(&target);
        assert!(
            registries
                .provinces
                .iter()
                .all(|(id, _, def)| def.beds.strength == 0.0
                    && (!rules || registries.province(id).is_some_and(|t| t.relief.is_empty()))),
            "a rule or a bed survived the copy"
        );
        registries
    }
}

/// Geology is optional, and a world without it has the provinces' fields
/// empty and is pinned, every field of every sample.
#[test]
fn a_world_without_geology_is_pinned() {
    let registries = registries_without_geology();
    assert!(registries.provinces.is_empty());
    let world = ProceduralWorld::new(Seed(42), registries);
    assert!(world.province_field().is_none());
    assert!(world.provinces(0.0, 0.0).is_none());
    // Its rivers under the lattice, on the bake's own threads.
    world.hydrology().set_threads(common::THREADS);
    world.prepare_water(0.0, 0.0, 50_000.0);
    let head_layer: Vec<u8> = registries
        .materials
        .iter()
        .map(|(_, name, _)| {
            HEAD_MATERIALS
                .iter()
                .position(|head| *head == name.to_string())
                .and_then(|layer| u8::try_from(layer).ok())
                .unwrap_or(u8::MAX)
        })
        .collect();
    let lattice = head_lattice();
    let samples: Vec<Sample> = sweep(|| {
        lattice
            .par_iter()
            .map(|&(x, z)| world.sample(x, z))
            .collect()
    });
    let mut hash = Fnv::new();
    for s in &samples {
        assert_eq!(s.provinces, Sample::NO_PROVINCES);
        for b in shape_bits(s) {
            hash.bytes(&b.to_le_bytes());
        }
        for v in [s.moisture, s.hydration, s.cover] {
            hash.bytes(&v.to_bits().to_le_bytes());
        }
        hash.bytes(&[head_layer[usize::from(s.material)]]);
        for (id, w) in s.biomes {
            hash.bytes(&id.0.to_le_bytes());
            hash.bytes(&w.to_bits().to_le_bytes());
        }
    }
    assert_eq!(
        hash.0, PINNED_WHOLE,
        "a world without geology moved (hash {:#018x}); if on purpose, bump `GENERATOR_VERSION` and paste it into `PINNED_WHOLE`",
        hash.0
    );
}

/// With the field on, a sample differs from the same world's without it
/// in its provinces and its cover alone: height, landform, material,
/// moisture, hydration and biomes to the bit, and cover the field-off
/// cover times the provinces' `rock.cover` by their whole mix's weights.
/// Over the lattice and the blend points, so badlands, borders,
/// three-way blends and the range band are all read; pure badlands carry
/// 0.15 of the cover.
#[test]
fn the_field_on_moves_only_cover_and_the_provinces() {
    // Its relief rules move heights (step 4); without them the field moves
    // only what step 3 let it.
    let on = &ProceduralWorld::new(Seed(42), registries_without_rules());
    let off = ProceduralWorld::new(Seed(42), registries_without_geology());
    let registries = common::registries();
    let rock_cover: Vec<f64> = registries
        .province_ids()
        .map(|id| f64::from(registries.province(id).expect("resolved").rock.cover))
        .collect();
    let badlands = registries
        .province_id(&Namespaced::parse("strata:badlands").expect("valid"))
        .expect("ships");
    assert_eq!(
        rock_cover[badlands.0 as usize].to_bits(),
        f64::from(0.15_f32).to_bits()
    );
    let mut points = head_lattice();
    points.extend(BLEND_POINTS);
    // (thinned, in a blend that thins, pure badlands with cover to thin)
    let counts = sweep(|| {
        points
            .par_iter()
            .map(|&(x, z)| {
                let (a, b) = (on.sample_before_carve(x, z), off.sample_before_carve(x, z));
                assert_eq!(shape_bits(&a), shape_bits(&b), "shape at ({x}, {z})");
                assert_eq!(a.material, b.material, "material at ({x}, {z})");
                for (field, u, v) in [
                    ("moisture", a.moisture, b.moisture),
                    ("hydration", a.hydration, b.hydration),
                ] {
                    assert_eq!(u.to_bits(), v.to_bits(), "{field} at ({x}, {z})");
                }
                for ((ia, wa), (ib, wb)) in a.biomes.iter().zip(&b.biomes) {
                    assert!(
                        ia == ib && wa.to_bits() == wb.to_bits(),
                        "biomes at ({x}, {z})"
                    );
                }
                let m = mix(on, x, z);
                assert_eq!(a.provinces, m.pair(), "provinces at ({x}, {z})");
                let gain: f64 = m
                    .entries()
                    .iter()
                    .map(|&(id, w)| rock_cover[id.0 as usize] * w)
                    .sum();
                // The sampler multiplies in f64 and rounds once; the field-off
                // cover is that f64 rounded already, so allow two roundings.
                #[allow(clippy::cast_possible_truncation, reason = "a sample's f32")]
                let want = (f64::from(b.cover) * gain) as f32;
                assert!(
                    (a.cover - want).abs() <= 2.0 * f32::EPSILON * want.max(f32::MIN_POSITIVE),
                    "cover at ({x}, {z}): {} with the field, {} without, gain {gain}",
                    a.cover,
                    b.cover
                );
                let thinned = a.cover < b.cover;
                let blend = thinned && m.entries().len() > 1 && gain > 0.15 && gain < 1.0;
                let pure = m.entries() == [(badlands, 1.0)] && b.cover > 0.0;
                if pure {
                    #[allow(clippy::cast_possible_truncation, reason = "a sample's f32")]
                    let want = (f64::from(b.cover) * rock_cover[badlands.0 as usize]) as f32;
                    assert!(
                        (a.cover - want).abs() <= 2.0 * f32::EPSILON * want,
                        "pure badlands at ({x}, {z}): {} of {}",
                        a.cover,
                        b.cover
                    );
                }
                (usize::from(thinned), usize::from(blend), usize::from(pure))
            })
            .reduce(|| (0, 0, 0), |p, q| (p.0 + q.0, p.1 + q.1, p.2 + q.2))
    });
    println!(
        "{} points: cover thinned at {}, {} of them in a blend; {} pure badlands",
        points.len(),
        counts.0,
        counts.1,
        counts.2
    );
    assert!(
        counts.0 > 1000 && counts.1 > 10 && counts.2 > 500,
        "too few points read: {counts:?}"
    );
}

/// The points the pinned hash reads: a 64 by 64 lattice at 3.1 km, off the
/// round numbers.
fn hash_points() -> impl Iterator<Item = (f64, f64)> {
    (0..64 * 64).map(|k| {
        (
            f64::from(k % 64) * 3_137.0 - 99_000.0,
            f64::from(k / 64) * 3_089.0 - 97_000.0,
        )
    })
}

fn field_hash(world: &ProceduralWorld) -> u64 {
    let mut hash = Fnv::new();
    for (x, z) in hash_points() {
        for &(id, w) in mix(world, x, z).entries() {
            hash.bytes(&id.0.to_le_bytes());
            hash.bytes(&w.to_bits().to_le_bytes());
        }
        hash.bytes(b"\0");
    }
    hash.0
}

/// The field is a pure function of the point and the seed: two worlds from
/// one seed agree to the bit, and seed 42's answers are pinned.
#[test]
fn the_field_is_the_same_bits_every_time() {
    let a = common::world(42);
    let b = common::world(42);
    for (x, z) in hash_points().step_by(7) {
        assert_eq!(mix(&a, x, z), mix(&b, x, z), "at ({x}, {z})");
        let (sa, sb) = (a.sample(x, z), b.sample(x, z));
        for ((ia, wa), (ib, wb)) in sa.provinces.iter().zip(&sb.provinces) {
            assert!(ia == ib && wa.to_bits() == wb.to_bits(), "at ({x}, {z})");
        }
    }
    let hash = field_hash(&a);
    assert_eq!(
        hash, PINNED,
        "seed 42's province field hashes to {hash:#018x}, pinned {PINNED:#018x}. If the field \
         moved on purpose (its code, a `province_*` parameter, a province's frequency, deal \
         order or `in_ranges`, or the range field it reads), bump `GENERATOR_VERSION` when code \
         moved it and paste the new value into `PINNED` in tests/geology.rs."
    );
}

/// The value [`the_field_is_the_same_bits_every_time`] holds: re-pinned by
/// ticket 0004's step 4, whose ranges (read from the distance to their
/// crest lines) move the range field's lift the substitution reads, and by
/// its review, which reads that distance from differences of the field.
const PINNED: u64 = 0xae2c_8979_87b9_c42a;

proptest! {
    #![proptest_config(ProptestConfig::with_cases(512))]

    /// The mix is a whole: weights sum to one, heaviest first, each
    /// province once; the sample's pair is its top two, renormalised.
    #[test]
    fn the_weights_sum_to_one(
        x in -200_000.0f64..200_000.0,
        z in -200_000.0f64..200_000.0,
    ) {
        let world = world42();
        let m = mix(world, x, z);
        let e = m.entries();
        prop_assert!(!e.is_empty() && e.len() <= strata_worldgen::geology::MAX_MIX);
        let sum: f64 = e.iter().map(|p| p.1).sum();
        prop_assert!((sum - 1.0).abs() < 1e-12, "{sum}");
        for (k, a) in e.iter().enumerate() {
            prop_assert!(a.1 > 0.0);
            prop_assert!((a.0.0 as usize) < 6);
            for b in &e[k + 1..] {
                prop_assert!(a.0 != b.0, "{:?} twice", a.0);
                prop_assert!(a.1 >= b.1, "not heaviest first: {e:?}");
            }
        }
        let s = world.sample(x, z);
        let [(p1, w1), (p2, w2)] = s.provinces;
        prop_assert_eq!(p1, e[0].0);
        prop_assert!((w1 + w2 - 1.0).abs() < 1e-6, "{} + {}", w1, w2);
        prop_assert!(w1 >= w2);
        if e.len() > 1 {
            prop_assert_eq!(p2, e[1].0);
        } else {
            prop_assert_eq!((p2, w2), (p1, 0.0));
        }
    }
}

fn world42() -> &'static ProceduralWorld {
    static WORLD: std::sync::OnceLock<ProceduralWorld> = std::sync::OnceLock::new();
    WORLD.get_or_init(|| common::world(42))
}

/// The largest change of any province's weight between points a metre
/// apart along the line from `(x, z)` in direction `(dx, dz)`, `metres`
/// long, and where.
fn steepest_step(
    world: &ProceduralWorld,
    (x, z): (f64, f64),
    (dx, dz): (f64, f64),
    metres: u32,
) -> (f64, f64, f64) {
    let weights = |x: f64, z: f64| {
        let mut w = [0.0; 6];
        for &(id, v) in mix(world, x, z).entries() {
            w[id.0 as usize] = v;
        }
        w
    };
    let mut previous = weights(x, z);
    let mut worst = (0.0, x, z);
    for s in 1..=metres {
        let (px, pz) = (x + dx * f64::from(s), z + dz * f64::from(s));
        let w = weights(px, pz);
        let step = w
            .iter()
            .zip(&previous)
            .map(|(a, b)| (a - b).abs())
            .fold(0.0, f64::max);
        if step > worst.0 {
            worst = (step, px, pz);
        }
        previous = w;
    }
    worst
}

/// The design's bound: no weight changes by more than a hundredth a metre
/// (a border blends over a kilometre; a step would be a jump towards one).
const MAX_STEP_PER_M: f64 = 0.01;

/// No weight jumps: along lines through province borders and three-way
/// points (found on a lattice, away from the ranges so the cells' own
/// borders are what is crossed, four directions each), and along random
/// lines, which cross the range substitution too.
#[test]
fn the_weights_are_continuous() {
    let world = world42();
    let lattice: Vec<(f64, f64, usize, f64)> = sweep(|| {
        (0..241 * 241)
            .into_par_iter()
            .filter_map(|k| {
                let (x, z) = (
                    f64::from(k % 241) * 500.0 - 60_000.0,
                    f64::from(k / 241) * 500.0 - 60_000.0,
                );
                if world.continent(x, z).range > 0.0 {
                    return None;
                }
                let m = mix(world, x, z);
                let e = m.entries();
                // A border: two near equal. A three-way point: a third weighing.
                let kind = if e.len() >= 3 && e[2].1 > 0.1 {
                    3
                } else if e.len() == 2 && e[1].1 > 0.4 {
                    2
                } else {
                    return None;
                };
                Some((x, z, kind, e[e.len() - 1].1))
            })
            .collect()
    });
    let triples: Vec<_> = lattice.iter().filter(|p| p.2 == 3).take(12).collect();
    let borders: Vec<_> = lattice
        .iter()
        .filter(|p| p.2 == 2)
        .step_by(9)
        .take(12)
        .collect();
    assert!(
        triples.len() >= 6 && borders.len() >= 12,
        "{} three-way points and {} border points found",
        triples.len(),
        borders.len()
    );
    let mut lines = Vec::new();
    let h = core::f64::consts::FRAC_1_SQRT_2;
    for p in triples.iter().chain(&borders) {
        for (dx, dz) in [(1.0, 0.0), (0.0, 1.0), (h, h), (h, -h)] {
            lines.push(((p.0 - dx * 1_500.0, p.1 - dz * 1_500.0), (dx, dz), 3_000));
        }
    }
    let mut rng = strata_core::Rng::new(11);
    for _ in 0..40 {
        let (x, z) = (
            rng.range_f64(-100_000.0, 100_000.0),
            rng.range_f64(-100_000.0, 100_000.0),
        );
        let a = rng.range_f64(0.0, core::f64::consts::TAU);
        // `libm` in a test is not the float rule; it is only a direction.
        lines.push(((x, z), (libm::cos(a), libm::sin(a)), 5_000));
    }
    let worst = sweep(|| {
        lines
            .par_iter()
            .map(|&(from, dir, metres)| steepest_step(world, from, dir, metres))
            .reduce(|| (0.0, 0.0, 0.0), |a, b| if b.0 > a.0 { b } else { a })
    });
    println!(
        "{} lines ({} three-way points, {} borders): the steepest weight step is {:.5} a metre at \
         ({:.0}, {:.0})",
        lines.len(),
        triples.len(),
        borders.len(),
        worst.0,
        worst.1,
        worst.2
    );
    assert!(
        worst.0 < MAX_STEP_PER_M,
        "a weight changed by {:.5} in a metre at ({:.0}, {:.0})",
        worst.0,
        worst.1,
        worst.2
    );
}

/// Seed 42 within 100 km of the origin, on a 250 m lattice: every province
/// takes 10 to 25 % of the land (the design's bound), a blend 7 to 13 %,
/// the crests (the range field over 0.6) no sandstone, basalt or badlands,
/// and Port Serein stands on granite alone.
#[test]
fn seed_42_provinces_share_the_land() {
    let world = world42();
    let registries = common::registries();
    let id = |name: &str| {
        registries
            .province_id(&Namespaced::parse(name).expect("valid"))
            .expect("ships")
    };
    let n = 801;
    let points: Vec<(ProvinceMix, f64)> = sweep(|| {
        (0..n * n)
            .into_par_iter()
            .filter_map(|k| {
                let (x, z) = (
                    f64::from(k % n - n / 2) * 250.0,
                    f64::from(k / n - n / 2) * 250.0,
                );
                if x * x + z * z > 1e10 || world.sample(x, z).height < 0.0 {
                    return None;
                }
                Some((mix(world, x, z), world.continent(x, z).range))
            })
            .collect()
    });
    #[allow(clippy::cast_precision_loss, reason = "a count of lattice points")]
    let land = points.len() as f64;
    let mut report = String::new();
    for (index, name, _) in registries.provinces.iter() {
        #[allow(clippy::cast_precision_loss, reason = "a count of lattice points")]
        let share = points.iter().filter(|p| p.0.dominant() == index).count() as f64 / land;
        write!(report, " {name} {:.2}%", share * 100.0).expect("a String takes it");
        assert!(
            (0.10..=0.25).contains(&share),
            "{name}: {:.2}%",
            share * 100.0
        );
    }
    #[allow(clippy::cast_precision_loss, reason = "a count of lattice points")]
    let blend = points.iter().filter(|p| p.0.entries()[0].1 < 0.999).count() as f64 / land;
    println!(
        "land within 100 km:{report}; in a blend {:.2}%",
        blend * 100.0
    );
    assert!(
        (0.07..=0.13).contains(&blend),
        "{:.2}% in a blend",
        blend * 100.0
    );
    let crests: Vec<_> = points.iter().filter(|p| p.1 > 0.6).collect();
    assert!(crests.len() > 1000);
    for name in ["strata:sandstone", "strata:basalt", "strata:badlands"] {
        let on = crests
            .iter()
            .filter(|p| p.0.weight_of(id(name)) > 0.0)
            .count();
        assert_eq!(on, 0, "{name} on {on} crest points");
    }
    let city = mix(world, -400.0, -500.0);
    assert_eq!(city.entries(), &[(id("strata:granite"), 1.0)]);
    assert_eq!(
        world.sample(-400.0, -500.0).provinces,
        [(id("strata:granite"), 1.0), (id("strata:granite"), 0.0)]
    );
}
