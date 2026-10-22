//! The fog field on the real world: seed 42 over the shipped registries.
//!
//! Compass directions here are the worldmap's (`+z` north).
//!
//! The places were the ones ticket 0003's first captures (its step 0) and
//! the worldmap review named: the hollows of the plateau at (-1500, -12000)
//! where the `valley` study scene stood until ticket 0004's step 2, the bay
//! head at (-5950, -13650) that step 0 filled with one hand-set top, the
//! shoulder above it that step 0 looked down from, the open sea west of the
//! coast and Port Serein's lower platform above its bay. Ticket 0004's step
//! 2 moved the ground under them and every one still was what its name
//! says. Its step 2b ran a mountain range through the plateau (the hollows,
//! the knolls between them and the lines across them stood on its flank at
//! 1 300 to 2 100 m), so those were moved, by measurement, to the plain
//! south-east of Port Serein, and the two tiles east of the city's were
//! added to the baked set so that the lines across that plain cross two
//! seams as the old ones did. The bay head, the open sea and the platform
//! are still what their names say and stay.
//!
//! The review of step 2b faded the ranges over a wider band towards the
//! coast, which moved the plain's ground by a few metres: the study hollow
//! moved again, 1.2 km south-east into the same basin, where its top is
//! level; the others kept their places.
//!
//! Ticket 0004's step 4 gave the plain the shapes of its provinces (granite
//! domes, sandstone benches) and the ranges a foot, and the basin the study
//! hollow stood in, filled to 206 m, is gone: the plain's hollows now fill
//! 10 to 40 m. The study hollow moved 300 m, to the deepest level-topped
//! hollow of that plain, the plain hollow and the knoll between them to the
//! nearest pair of hollows whose levels part by over 20 m with ground
//! between that full weather leaves clear (found with the step's probe,
//! `fogplaces`, over every baked tile). The bay head, the shoulder, the
//! knoll north of the study hollow, the open sea and the platform stay.
//!
//! The review of step 4 moved the ground again by metres (the ranges' foot
//! read from differences, a province's shape centred on its mean): the
//! plain hollow's level came within 18 m of the study hollow's and the fog
//! reached the knoll between them, so the plain hollow and the knoll moved
//! to the pair the same search found for the study hollow, 7.5 km east.
//!
//! Ticket 0005's step 2 carved the rivers: a creek's valley drained the
//! plain hollow to 145 m, 15 m over the study hollow's, so the plain hollow
//! and the knoll moved again, by [`fog_places`], to (12 150, -2 950), filled
//! to 176 m, and (9 929, -4 326). A valley narrower than the lattice is a
//! known gap of its own ([`fog_in_a_valley_is_a_known_gap`]).
//!
//! What the move leaves out: on the range's steep ground the 128 m lattice
//! knows the ground less well than a knoll's worth, and the fog there lies
//! on ground over its top where the plain keeps under the 5 m this file
//! holds: 29.9 m on step 2b's ground over the old window, across the
//! range's flank, 15.8 m since its review, 9.0 m since step 4, whose
//! flank is a smooth slope of the range's own, and 12.2 m since step 4's
//! review.
//! [`fog_on_a_range_flank_is_a_known_gap`] holds that bound, not the
//! plain's; the mountains are a known gap (ticket 0004, "Step 2b, as
//! built").

use std::sync::{Arc, OnceLock};

use strata_core::Seed;
use strata_weather::{BasinTile, CELL_M, FogField, TILE_M, TILE_NODES, TileCoord, Weather};
use strata_worldgen::{ProceduralWorld, WorldSource};

/// The floor of a hollow on the plain south-east of Port Serein, 105 m,
/// filled to 131 m, with a kilometre of level top across it (since ticket
/// 0004's step 4, and its review left it there; (4100, -7700), 300 m off, from the review of step 2b,
/// when the basin there filled to 206 m; before that (3400, -6700); before
/// step 2b the old valley study scene's floor at (-1480, -12610), which
/// that step put on a mountainside at 2 093 m).
const STUDY_HOLLOW: (f64, f64) = (4000.0, -8000.0);
/// Another hollow 9.6 km north-east of it, 160 m, filled to 176 m, 46 m over
/// the study hollow's level (since ticket 0005's step 2, whose carve ran a
/// river through (11 500, -7 700), the review of step 4's, and drained it
/// to 145 m, 15 m over the study hollow's; found with [`fog_places`]; before
/// that (1000, -3400) from step 4, (700, -5200) from step 2b, and before
/// step 2b (-4640, -13140), now on the range's foot).
const PLAIN_HOLLOW: (f64, f64) = (12_150.0, -2_950.0);
/// The knoll between them, 174 m, where no fog lies in any weather (since
/// ticket 0005's step 2, with the plain hollow; (7150, -7874) from the
/// review of step 4, (2110, -5102) from step 4, (2151, -6006) from step
/// 2b, and before step 2b (-2600, -12000), now on the range).
const BETWEEN: (f64, f64) = (9_929.0, -4_326.0);
/// The bay head's floor, 28 m, under a rim that rises from 105 m at the
/// water's edge to over 400 m on the range to the east: ticket 0003's step
/// 0 valley.
const RAVINE: (f64, f64) = (-5950.0, -13650.0);
/// The shoulder above the bay head, 620 m, 2 km north of it: ticket 0003's
/// `RIDGE` viewpoint was (-5300, -13700), which step 2b's ground left as a
/// basin under full weather's fog; this, the nearest crest above the bay
/// head that full weather left clear on that ground, still is.
const RIDGE: (f64, f64) = (-5200.0, -11550.0);
/// A knoll crest north of the study hollow, 212 m, which full weather
/// leaves clear (on step 2b's ground the only crest on that plain it did;
/// since step 2b; it was (0, -12000), now on the range).
const KNOLL: (f64, f64) = (5000.0, -4800.0);
/// Open sea, 700 m deep, 6 km off the coast.
const OPEN_SEA: (f64, f64) = (-12000.0, -8000.0);
/// Port Serein's lower platform, 44 m over its bay.
const PORT_SEREIN: (f64, f64) = (-400.0, -500.0);

fn world() -> &'static ProceduralWorld {
    static WORLD: OnceLock<ProceduralWorld> = OnceLock::new();
    WORLD.get_or_init(|| {
        let registries =
            strata_data::load_world_from_dir(concat!(env!("CARGO_MANIFEST_DIR"), "/../../assets"))
                .expect("the shipped assets load");
        let world = ProceduralWorld::new(Seed(42), &registries);
        // The rivers under every tile here first, on the bake's own threads
        // (ticket 0005, step 2): a sample that met an unbaked land mass
        // would bake it on its own thread, which a debug build is slow at.
        world.hydrology().set_threads(16);
        world.prepare_water(0.0, 0.0, 100_000.0);
        world
    })
}

/// The field over the tiles every test here reads, baked once.
fn field() -> &'static FogField {
    static FIELD: OnceLock<FogField> = OnceLock::new();
    FIELD.get_or_init(|| {
        let mut field = FogField::new(Seed(42));
        // The last two since ticket 0004's step 2b, for the plain the
        // hollows and the lines moved to.
        let coords = [
            TileCoord { x: -2, z: -2 },
            TileCoord { x: -1, z: -2 },
            TileCoord { x: 0, z: -2 },
            TileCoord { x: -2, z: -1 },
            TileCoord { x: -1, z: -1 },
            TileCoord { x: 0, z: -1 },
            TileCoord { x: 1, z: -1 },
        ];
        let tiles: Vec<BasinTile> = std::thread::scope(|scope| {
            let handles: Vec<_> = coords
                .iter()
                .map(|&c| scope.spawn(move || BasinTile::bake(world(), c)))
                .collect();
            handles
                .into_iter()
                .map(|h| h.join().expect("bake"))
                .collect()
        });
        for tile in tiles {
            field.insert(Arc::new(tile)).expect("a bake is well formed");
        }
        field
    })
}

fn sample(at: (f64, f64), weather: &Weather) -> strata_weather::FogSample {
    field()
        .sample(at.0, at.1, weather)
        .expect("the tile is baked")
}

const NO_SEA: Weather = Weather {
    fog: 1.0,
    sea_fog: 0.0,
    drift: [0.0, 0.0],
};

#[test]
fn the_study_hollow_fogs() {
    let s = sample(STUDY_HOLLOW, &NO_SEA);
    let ground = world().sample(STUDY_HOLLOW.0, STUDY_HOLLOW.1).height;
    assert!(s.density > 0.1, "{s:?}");
    assert!(s.top > ground + 8.0, "top {} over ground {ground}", s.top);
}

#[test]
fn the_bay_head_fogs_from_its_own_basin() {
    let s = sample(RAVINE, &NO_SEA);
    assert!(s.density > 0.5, "{s:?}");
    // Under the rim, near the 85 m step 0 set by hand: 99 m on ticket
    // 0004's step 2b ground and 106 m since its review, whose ranges fade
    // over a wider band towards the coast and lift the ground east and
    // north of the bay head by 20 to 60 m (the bound was 100 m until then).
    assert!((60.0..115.0).contains(&s.top), "{s:?}");
}

#[test]
fn neighbouring_basins_have_their_own_levels() {
    let study = sample(STUDY_HOLLOW, &NO_SEA);
    let plain = sample(PLAIN_HOLLOW, &NO_SEA);
    let between = sample(BETWEEN, &NO_SEA);
    assert!(study.density > 0.1 && plain.density > 0.1);
    assert!(between.density == 0.0, "{between:?}");
    assert!(
        (study.top - plain.top).abs() > 20.0,
        "study {} plain {}",
        study.top,
        plain.top
    );
}

#[test]
fn a_basin_top_is_a_level() {
    // Across the study hollow, a kilometre east to west through the floor.
    let tops: Vec<f32> = (0..=5)
        .map(|k| {
            sample(
                (
                    STUDY_HOLLOW.0 - 500.0 + 200.0 * f64::from(k),
                    STUDY_HOLLOW.1,
                ),
                &NO_SEA,
            )
            .top
        })
        .collect();
    let (lo, hi) = tops
        .iter()
        .fold((f32::MAX, f32::MIN), |(lo, hi), &t| (lo.min(t), hi.max(t)));
    eprintln!("tops across the study hollow {tops:?}");
    assert!(hi - lo < 2.0, "tops across the hollow {tops:?}");
}

#[test]
fn ridges_get_nothing() {
    for at in [RIDGE, KNOLL, BETWEEN] {
        let s = sample(at, &Weather::FULL);
        assert!(s.density == 0.0, "{at:?}: {s:?}");
    }
}

#[test]
fn the_open_sea_is_clear_without_sea_fog() {
    let s = sample(OPEN_SEA, &NO_SEA);
    assert!(s.density == 0.0, "{s:?}");
    // And a bank lies there under full weather, tens of metres deep.
    let full = sample(OPEN_SEA, &Weather::FULL);
    assert!(
        full.density > 0.3 && (20.0..100.0).contains(&full.top),
        "{full:?}"
    );
}

#[test]
fn no_fog_without_weather() {
    for at in [STUDY_HOLLOW, PLAIN_HOLLOW, RAVINE, OPEN_SEA] {
        assert!(sample(at, &Weather::CLEAR).density == 0.0, "{at:?}");
    }
}

#[test]
fn no_fog_lies_on_ground_over_its_top() {
    // Wherever the ground stands over the top nothing lies on it, to
    // within what 128 m nodes know of the ground: a knoll between four
    // nodes can stand a few metres over their interpolated height. Over
    // the plain south-east of Port Serein since ticket 0004's step 2b (12
    // by 6 km from (-808, -7192), across the seams at x = 0 and 8192);
    // until then over the plateau from (-9000, -16000), which that step's
    // range took, and where the fog lies over its top on the range's flank
    // (`fog_on_a_range_flank_is_a_known_gap`).
    let w = world();
    let (mut checked, mut worst, mut at) = (0, f32::MIN, (0.0, 0.0));
    let (mut by_road, mut road_worst) = (0, f32::MIN);
    for j in 0..60 {
        for i in 0..60 {
            let (x, z) = (
                -808.0 + 200.0 * f64::from(i),
                -7192.0 + 100.0 * f64::from(j),
            );
            let s = sample((x, z), &Weather::FULL);
            let here = w.sample(x, z);
            let ground = here.height.max(0.0);
            // In a river's valley the 128 m nodes know the ground less well
            // than a valley's width (`fog_in_a_valley_is_a_known_gap`), and
            // a road's cutting or fill (ticket 0005, step 6) is narrower
            // still: a node in a cutting reads it as a hollow the fog fills,
            // whose level the ground beside it stands over, and a fill
            // stands over the fog the ground beside it lies under. Those
            // points alone (a fill under the point, a cut under one of the
            // four nodes round it) are held apart, to a bound of their own.
            let road = here.landform.cut_m < 0.0
                || [(0.0, 0.0), (1.0, 0.0), (0.0, 1.0), (1.0, 1.0)]
                    .iter()
                    .any(|(i, j)| {
                        let node = (
                            ((x / CELL_M).floor() + i) * CELL_M,
                            ((z / CELL_M).floor() + j) * CELL_M,
                        );
                        w.sample(node.0, node.1).landform.cut_m > 0.0
                    });
            if s.density > 0.0 && here.water.is_none() && road {
                by_road += 1;
                road_worst = road_worst.max(ground - s.top);
            }
            if s.density > 0.0 && here.water.is_none() && !road && ground - s.top > worst {
                (worst, at) = (ground - s.top, (x, z));
            }
            if ground > s.top {
                checked += 1;
            }
        }
    }
    assert!(checked > 1000, "{checked}");
    eprintln!("fog on ground at most {worst} m over its top, at {at:?}");
    eprintln!("by a road's cut or fill, {by_road} points, at most {road_worst} m");
    // A known gap like the valley's: 5.7 m over the road's ground of ticket
    // 0005's steps 5 and 6 as reviewed, over 19 points; held so that it
    // does not grow unseen.
    assert!(
        road_worst < 6.0,
        "fog lies on a road's ground {road_worst} m over its top"
    );
    assert!(
        worst < 5.0,
        "fog lies on ground {worst} m over its top at {at:?}"
    );
}

/// In a river's valley (ticket 0005, step 2) the 128 m nodes know the
/// ground less well than the valley's width: a node on a creek's floor in a
/// slot cut through the plain fills with fog to over the floor, and a point
/// on the slot's wall between that node and one on the plain above reads
/// fog under a top its ground stands over. Over the plain
/// [`no_fog_lies_on_ground_over_its_top`] holds, at points in a valley:
/// 11.0 m on step 2's ground, against the plain's 5 m. A known gap, held so
/// that it does not grow unseen; the fog's bake reading the ground between
/// its nodes is ticket 0003's work.
#[test]
fn fog_in_a_valley_is_a_known_gap() {
    let w = world();
    let (mut checked, mut worst) = (0, f32::MIN);
    for j in 0..60 {
        for i in 0..60 {
            let (x, z) = (
                -808.0 + 200.0 * f64::from(i),
                -7192.0 + 100.0 * f64::from(j),
            );
            let here = w.sample(x, z);
            if here.water.is_none() {
                continue;
            }
            checked += 1;
            let s = sample((x, z), &Weather::FULL);
            if s.density > 0.0 {
                worst = worst.max(here.height.max(0.0) - s.top);
            }
        }
    }
    eprintln!("fog in a valley on ground at most {worst} m over its top ({checked} points)");
    assert!(checked > 100, "{checked} points in a valley");
    assert!(
        worst < 13.0,
        "fog in a valley lies on ground {worst} m over its top"
    );
}

/// On a range's steep flank the 128 m nodes know the ground less well than
/// on the plain, and fog lies on ground over its top by more than the 5 m
/// [`no_fog_lies_on_ground_over_its_top`] holds there: across the flank
/// of the range through the old plateau, 29.9 m on ticket 0004's step 2b
/// ground, 15.8 m since its review, 9.0 m since step 4 and 12.2 m since
/// step 4's review. A known gap (ticket 0004), held here so that it does
/// not grow unseen.
#[test]
fn fog_on_a_range_flank_is_a_known_gap() {
    let w = world();
    let mut worst = f32::MIN;
    for j in 0..60 {
        for i in 0..60 {
            let (x, z) = (
                -9000.0 + 200.0 * f64::from(i),
                -16000.0 + 100.0 * f64::from(j),
            );
            let s = sample((x, z), &Weather::FULL);
            if s.density > 0.0 {
                worst = worst.max(w.sample(x, z).height.max(0.0) - s.top);
            }
        }
    }
    eprintln!("fog on the range's flank at most {worst} m over its top");
    // The bound records the gap as it is, so that a change that widens it
    // is seen: 12 m after ticket 0004's step 2b, 33 m after its step 5 gave
    // the ranges their crests back. Closing it is the fog's own work
    // (ticket 0003: a bake that reads the ground's steepness), not a reason
    // to keep the crests soft.
    assert!(
        worst < 40.0,
        "fog lies on the range's flank {worst} m over its top"
    );
}

#[test]
fn the_field_is_continuous() {
    // Along a line across the bay's edge, two tile seams (x = -8192 and
    // x = 0), the plain and its hollows: neighbouring samples 4 m apart
    // never jump. A renderer draws a jump as a wall. The lines are on the
    // plain south-west of the study hollow since ticket 0004's step 2b
    // (z = -5192 and -3192 from x = -8808, crossing the bay west of Port
    // Serein); until then they were z = -12000 and -13650 from x = -9000,
    // across the plateau that step's range took.
    let step = 4.0;
    // Sea fog meeting a steep coast (a slope of 1 in 4) fades in over
    // thirty metres, hence the larger bound where it is on.
    for (weather, z, bound) in [
        (Weather::FULL, -5192.0, 0.1),
        (NO_SEA, -5192.0, 0.02),
        (Weather::FULL, -3192.0, 0.1),
        (NO_SEA, -3192.0, 0.05),
    ] {
        let mut prev: Option<strata_weather::FogSample> = None;
        let (mut worst_d, mut worst_t) = (0.0f32, 0.0f32);
        for k in 0..3000 {
            let x = -8808.0 + step * f64::from(k);
            let s = sample((x, z), &weather);
            if let Some(p) = prev {
                worst_d = worst_d.max((s.density - p.density).abs());
                if s.density.min(p.density) > 0.01 {
                    worst_t = worst_t.max((s.top - p.top).abs());
                }
            }
            prev = Some(s);
        }
        eprintln!(
            "sea_fog {} z {z}: worst density step {worst_d:.4}, worst top step {worst_t:.3} m",
            weather.sea_fog
        );
        assert!(worst_d < bound, "density jumps {worst_d} along z = {z}");
        assert!(worst_t < 1.5, "top jumps {worst_t} m along z = {z}");
    }
}

/// Every channel of a tile, by name, for comparing two tiles' nodes.
fn channels(tile: &BasinTile) -> [(&'static str, &[f32]); 6] {
    [
        ("ground", &tile.ground),
        ("top", &tile.top),
        ("floor", &tile.floor),
        ("hydration", &tile.hydration),
        ("sea_distance", &tile.sea_distance),
        ("offshore", &tile.offshore),
    ]
}

#[test]
fn neighbouring_tiles_share_their_edge_bits() {
    let n = TILE_NODES;
    // Across `x = 0` (west's last column is east's first) and across
    // `z = -8192` (north's last row is south's first).
    let pairs = [
        (TileCoord { x: -1, z: -2 }, TileCoord { x: 0, z: -2 }, true),
        (
            TileCoord { x: -2, z: -2 },
            TileCoord { x: -2, z: -1 },
            false,
        ),
    ];
    for (a, b, along_x) in pairs {
        let a = field().tile(a).expect("baked");
        let b = field().tile(b).expect("baked");
        for ((name, ca), (_, cb)) in channels(a).into_iter().zip(channels(b)) {
            for k in 0..n {
                let (ia, ib) = if along_x {
                    (k * n + n - 1, k * n)
                } else {
                    ((n - 1) * n + k, k)
                };
                assert_eq!(
                    ca[ia].to_bits(),
                    cb[ib].to_bits(),
                    "{name} at node {k} between {:?} and {:?}",
                    a.coord,
                    b.coord
                );
            }
        }
    }
    // And a point on the seam reads the same from either side.
    let west = field().tile(TileCoord { x: -1, z: -2 }).expect("baked");
    let (x, z) = (0.0, -12000.0);
    let from_east = sample((x, z), &Weather::FULL);
    let from_west = field().sample_tile(west, x, z, &Weather::FULL);
    assert_eq!(from_east, from_west);
    let _ = (CELL_M, TILE_M);
}

#[test]
fn sea_fog_comes_ashore_on_port_serein_lower_platform() {
    // The platform stands 44 m over the bay, a few hundred metres from the
    // water along low ground: no basin holds fog there, and the sea's banks
    // at their fullest (tops to 85 m) come up over it.
    let clear = sample(PORT_SEREIN, &NO_SEA);
    let full = sample(PORT_SEREIN, &Weather::FULL);
    let ground = world().sample(PORT_SEREIN.0, PORT_SEREIN.1).height;
    assert!(clear.density == 0.0, "{clear:?}");
    assert!(full.density > 0.1, "{full:?}");
    assert!(full.top > ground, "top {} under ground {ground}", full.top);
}

#[test]
fn half_sea_fog_leaves_clear_water_between_the_banks() {
    // The game's fixed weather until step 3 is `sea_fog` 0.5: separate
    // banks, so a coast shows both fog and clear water. Over the open sea
    // of the baked tiles (a kilometre or more from land) part is under a
    // bank and part is not, and full weather covers more.
    let half = Weather {
        fog: 1.0,
        sea_fog: 0.5,
        drift: [0.0, 0.0],
    };
    let tile = field().tile(TileCoord { x: -2, z: -1 }).expect("baked");
    let (ox, oz) = tile.coord.origin();
    let (mut open, mut under_half, mut under_full) = (0_usize, 0_usize, 0_usize);
    // The tile's own square, less its last row and column, which belong
    // to the next tiles (not baked here).
    for j in (0..TILE_NODES - 1).step_by(2) {
        for i in (0..TILE_NODES - 1).step_by(2) {
            if tile.offshore[j * TILE_NODES + i] < 1024.0 {
                continue;
            }
            open += 1;
            #[allow(clippy::cast_precision_loss)]
            let (x, z) = (ox + i as f64 * CELL_M, oz + j as f64 * CELL_M);
            under_half += usize::from(sample((x, z), &half).density > 0.3);
            under_full += usize::from(sample((x, z), &Weather::FULL).density > 0.3);
        }
    }
    #[allow(clippy::cast_precision_loss)]
    let (h, f) = (
        under_half as f64 / open as f64,
        under_full as f64 / open as f64,
    );
    eprintln!("open sea nodes {open}: under a bank {h:.2} at half, {f:.2} at full");
    assert!(open > 200, "{open}");
    assert!((0.05..0.8).contains(&h), "{h}");
    assert!(f > h + 0.1, "{f} {h}");
}

#[test]
fn a_bake_is_the_same_bits_twice() {
    let coord = TileCoord::containing(RAVINE.0, RAVINE.1);
    let again = BasinTile::bake(world(), coord);
    let first = field().tile(coord).expect("baked");
    assert_eq!(**first, again);
    assert_eq!(first.content_hash(), again.content_hash());
}

/// FNV-1a over the bits of a set of samples, as `BasinTile::content_hash`
/// does over a tile.
fn samples_hash(samples: &[strata_weather::FogSample]) -> u64 {
    let mut hash = 0xcbf2_9ce4_8422_2325_u64;
    for s in samples {
        for value in [s.base, s.top, s.density] {
            for byte in value.to_bits().to_le_bytes() {
                hash ^= u64::from(byte);
                hash = hash.wrapping_mul(0x0100_0000_01b3);
            }
        }
    }
    hash
}

/// Fails with the new value in hex, pasteable over the constant.
fn assert_hash(what: &str, got: u64, recorded: u64) {
    assert!(
        got == recorded,
        "{what}: {got:#018x}, recorded {recorded:#018x}. If the change is \
         intended, compare the worldmap's `-fog` layer before and after and \
         set the constant to the new value. The tile's ground and hydration \
         are the world's, so an edit to the shipped biomes or terrain assets \
         moves it too, with nothing wrong in the fog field."
    );
}

/// The recorded hashes of the tile under the bay head and of the fog
/// sampled over it: at its nodes under full weather, and between its nodes
/// (the bilinear path) under a partial, drifted weather. A change that
/// moves the field changes one of them. CI runs these in debug; they were
/// checked to be the same bits in release. Last moved by ticket 0012's
/// outlaw camps, whose camp Bogal Bend and its trail lie in the tile, from
/// `0x4f41_30f4_1eba_2092`, `0x26e3_a811_591e_c7fe` and
/// `0xfaf2_fb4f_080c_b499` (checked in debug only); before that by the roads'
/// alignment pass (`GENERATOR_VERSION` 16), whose roads moved and whose
/// banks in the tile moved with them, from `0x46ee_4e77_acb9_c3ed`,
/// `0x1914_ca40_e209_dbc9` and `0x4075_c179_65a6_cd48` (checked in release
/// only); before that by ticket 0005's
/// step 6, the road's grade, whose roads' banks in the tile are its ground
/// now (and the valley walls' wander and the sites it moved), from
/// `0x6b62_a4ba_1909_f8a9`, `0x5e67_1082_0c3f_2f5f` and
/// `0x7e0a_dc6b_c20a_cc2b` as steps 5 and 6 were first built (their review
/// moved the lines, the decks, the junctions and the banks by a channel);
/// before that
/// `0x8abc_ba8a_b3cf_b208`, `0x5cc1_062e_78db_bdcd` and
/// `0x9465_4bdc_454c_7a8b`; before that by its step 4, procedural sites,
/// whose pads in the tile (a monitoring station and hamlets south-west of
/// the city) are its ground now, from
/// `0xe8cd_45e1_e9fa_d846`, `0x338a_cf71_6b6e_3af5` and
/// `0x709e_9c11_5c68_c3fe` (`0x6bce_3ccb_313c_db4f`, `0xa152_1fd8_6b27_b21e`
/// and `0x1e91_ed1e_ff41_5a37` as step 4 was first built, before its review
/// moved the pads); before that by its step 3b, the rivers' shape,
/// from `0xc00d_f986_ea57_ba22`,
/// `0x2ceb_b31b_396d_5e53` and `0x3739_f096_995f_3c0b`; before that by its
/// step 2, the rivers' carve, from `0xcf66_767e_236b_3900`,
/// `0xecc6_dbd7_fce4_bc9a` and `0x2cd4_2dde_cec3_f299`.
#[test]
fn the_field_has_not_moved() {
    let coord = TileCoord::containing(RAVINE.0, RAVINE.1);
    let tile = field().tile(coord).expect("baked");
    assert_hash("the bay tile", tile.content_hash(), 0x2227_f352_92f9_1758);
    let (ox, oz) = coord.origin();
    let mut out = Vec::new();
    let missing = field().sample_grid(ox, oz, 128.0, 64, &Weather::FULL, &mut out);
    assert_eq!(missing, 0);
    assert_hash(
        "the fog at the bay tile's nodes",
        samples_hash(&out),
        0x84cb_49dd_e5bd_bc92,
    );
    let partial = Weather {
        fog: 0.7,
        sea_fog: 0.5,
        drift: [1234.5, -678.25],
    };
    let missing = field().sample_grid(ox + 37.0, oz + 53.0, 101.0, 80, &partial, &mut out);
    assert_eq!(missing, 0);
    assert_hash(
        "the fog between the bay tile's nodes under partial weather",
        samples_hash(&out),
        0xcea2_eed3_ef87_2d5d,
    );
}

/// The search the plain hollow and the knoll between were found with (since
/// ticket 0005's step 2): every basin over the baked tiles whose fog stands
/// at least 15 m over its floor under full basin weather, out of the rivers'
/// valleys, by its deepest point; and for each whose top is level 200 m
/// round it and that lies 3 to 10 km from the study hollow with a level
/// more than 20 m over it, the highest point between them that full weather
/// leaves clear. Ignored; run in release with
/// `--ignored --nocapture`.
#[test]
#[ignore = "a search, printed"]
#[allow(
    clippy::many_single_char_names,
    reason = "the world, a distance, a fraction and a point along a line"
)]
fn fog_places() {
    use std::collections::BTreeMap;
    let w = world();
    let study = sample(STUDY_HOLLOW, &NO_SEA);
    // Per basin (its top to a decimetre), its deepest point.
    let mut basins: BTreeMap<i64, (f32, (f64, f64), f32)> = BTreeMap::new();
    for j in 0..164 {
        for i in 0..328 {
            let (x, z) = (
                -16_350.0 + 100.0 * f64::from(i),
                -16_350.0 + 100.0 * f64::from(j),
            );
            let Some(s) = field().sample(x, z, &NO_SEA) else {
                continue;
            };
            let here = w.sample(x, z);
            if s.density <= 0.1 || here.water.is_some() || here.height <= 1.0 {
                continue;
            }
            let depth = s.top - here.height;
            if depth < 15.0 {
                continue;
            }
            #[allow(clippy::cast_possible_truncation, reason = "a level in decimetres")]
            let key = (f64::from(s.top) * 10.0).round() as i64;
            let entry = basins.entry(key).or_insert((depth, (x, z), s.top));
            if depth > entry.0 {
                *entry = (depth, (x, z), s.top);
            }
        }
    }
    for (depth, at, top) in basins.values() {
        let d = ((at.0 - STUDY_HOLLOW.0).powi(2) + (at.1 - STUDY_HOLLOW.1).powi(2)).sqrt();
        if !(3_000.0..=10_000.0).contains(&d) || top - study.top <= 20.0 {
            continue;
        }
        // A basin, not a slope: its top level to 2 m over 400 m round its
        // floor wherever fog lies.
        let tops: Vec<f32> = [(-200.0, 0.0), (200.0, 0.0), (0.0, -200.0), (0.0, 200.0)]
            .iter()
            .filter_map(|(dx, dz)| {
                let s = field().sample(at.0 + dx, at.1 + dz, &NO_SEA)?;
                (s.density > 0.1).then_some(s.top)
            })
            .collect();
        if tops.iter().any(|t| (t - top).abs() > 2.0) {
            continue;
        }
        // The highest clear point on the line between.
        let mut knoll: Option<((f64, f64), f32)> = None;
        for k in 1..400 {
            let f = f64::from(k) / 400.0;
            let p = (
                STUDY_HOLLOW.0 + (at.0 - STUDY_HOLLOW.0) * f,
                STUDY_HOLLOW.1 + (at.1 - STUDY_HOLLOW.1) * f,
            );
            let clear = field()
                .sample(p.0, p.1, &Weather::FULL)
                .is_some_and(|s| s.density == 0.0);
            let h = w.sample(p.0, p.1).height;
            if clear && knoll.is_none_or(|(_, held)| h > held) {
                knoll = Some((p, h));
            }
        }
        // Clear only if every point between the two basins' fog is: the
        // line leaves the study basin, crosses clear ground, and enters
        // the other.
        println!(
            "hollow ({:.0}, {:.0}) floor {:.1} top {top:.1} depth {depth:.1}, {:.1} km, {:.1} m from the study's {:.1}; clear knoll {knoll:?}",
            at.0,
            at.1,
            top - depth,
            d / 1000.0,
            top - study.top,
            study.top
        );
    }
}
