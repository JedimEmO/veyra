//! The field's rules on tiles with no world behind them: every channel set
//! by hand, so each test says what one rule does with numbers it chose.

use std::sync::Arc;

use strata_core::Seed;
use strata_weather::{BasinTile, FogField, FogSample, TILE_M, TILE_NODES, TileCoord, Weather};

/// One value per channel, the same at every node.
#[derive(Clone, Copy)]
struct Node {
    ground: f32,
    top: f32,
    floor: f32,
    hydration: f32,
    sea_distance: f32,
    offshore: f32,
}

/// A wet basin 60 m deep, well inland: fog under any weather that has it.
const WET_BASIN: Node = Node {
    ground: 40.0,
    top: 100.0,
    floor: 30.0,
    hydration: 1.0,
    sea_distance: 3000.0,
    offshore: 0.0,
};

/// The open sea: the surface as ground, no basin, a kilometre from land.
const OPEN_SEA: Node = Node {
    ground: 0.0,
    top: 0.0,
    floor: 0.0,
    hydration: 1.0,
    sea_distance: 0.0,
    offshore: 1024.0,
};

fn tile(coord: TileCoord, g: Node) -> BasinTile {
    let n = TILE_NODES * TILE_NODES;
    BasinTile {
        coord,
        ground: vec![g.ground; n],
        top: vec![g.top; n],
        floor: vec![g.floor; n],
        hydration: vec![g.hydration; n],
        sea_distance: vec![g.sea_distance; n],
        offshore: vec![g.offshore; n],
    }
}

/// A field of the same ground over the tiles from `(-r, -r)` to `(r, r)`.
fn field(g: Node, r: i32) -> FogField {
    let mut field = FogField::new(Seed(42));
    for z in -r..=r {
        for x in -r..=r {
            field
                .insert(tile(TileCoord { x, z }, g))
                .expect("well formed");
        }
    }
    field
}

fn weather(fog: f32, sea_fog: f32) -> Weather {
    Weather {
        fog,
        sea_fog,
        drift: [0.0, 0.0],
    }
}

fn at(field: &FogField, x: f64, z: f64, w: &Weather) -> FogSample {
    field.sample(x, z, w).expect("baked")
}

/// Points spread over the middle tile, for rules that hold everywhere.
fn points() -> impl Iterator<Item = (f64, f64)> {
    (0..20).flat_map(|j| (0..20).map(move |i| (f64::from(i) * 397.0, f64::from(j) * 401.0)))
}

#[test]
fn weaker_weather_lowers_the_top_and_keeps_only_the_strongest_pools() {
    let wet = field(WET_BASIN, 0);
    // A dry pool only 30 m under its full top: a haze under full weather.
    let weak = field(
        Node {
            ground: 70.0,
            hydration: 0.0,
            ..WET_BASIN
        },
        0,
    );
    for (x, z) in points() {
        let full = at(&wet, x, z, &weather(1.0, 0.0));
        let half = at(&wet, x, z, &weather(0.5, 0.0));
        let none = at(&wet, x, z, &weather(0.0, 0.0));
        // The top goes from the full top towards the floor: at half weather
        // three quarters of the way up (30 + 70 * 0.75).
        assert!((full.top - 100.0).abs() < 1e-4, "{full:?}");
        assert!((half.top - 82.5).abs() < 1e-4, "{half:?}");
        assert!(full.density > 0.5, "{full:?}");
        assert!(
            half.density > 0.2 && half.density < full.density,
            "{half:?}"
        );
        assert!(none.density == 0.0, "{none:?}");
        // The weak pool is there under full weather and gone at half.
        assert!(at(&weak, x, z, &weather(1.0, 0.0)).density > 0.05);
        assert!(
            at(&weak, x, z, &weather(0.5, 0.0)).density == 0.0,
            "the weak pool at half weather"
        );
    }
}

#[test]
fn drift_moves_the_sea_banks_and_leaves_the_basins_where_they_are() {
    let sea = field(OPEN_SEA, 1);
    let drift = [3000.0, -2000.0];
    let moved = Weather {
        drift,
        ..weather(0.0, 0.7)
    };
    let still = weather(0.0, 0.7);
    let (mut lo, mut hi) = (f32::MAX, f32::MIN);
    for (x, z) in points() {
        // What lay upwind by the drift lies here now.
        let now = at(&sea, x, z, &moved);
        let before = at(&sea, x - drift[0], z - drift[1], &still);
        assert_eq!(now, before, "({x}, {z})");
        lo = lo.min(now.density);
        hi = hi.max(now.density);
    }
    // The banks do vary over the points, or the test above says nothing.
    assert!(hi - lo > 0.3, "sea fog from {lo} to {hi}");

    // A basin's top and extent are the ground's: drift thins it in patches
    // that move with the wind (by at most `PATCH_DEPTH`, 30 %), no more.
    let basin = field(WET_BASIN, 0);
    for (x, z) in points() {
        let now = at(
            &basin,
            x,
            z,
            &Weather {
                drift,
                ..weather(1.0, 0.0)
            },
        );
        let before = at(&basin, x, z, &weather(1.0, 0.0));
        assert!(
            now.top.to_bits() == before.top.to_bits(),
            "{now:?} {before:?}"
        );
        // Each is thinned by a factor from 0.7 to 1, so they differ by
        // no more than 0.7 either way.
        assert!(now.density > 0.0);
        assert!(now.density >= 0.699 * before.density, "{now:?} {before:?}");
        assert!(before.density >= 0.699 * now.density, "{now:?} {before:?}");
    }
}

#[test]
fn sea_fog_comes_ashore_over_low_ground_near_the_sea_only() {
    // Low coast by the sea: the banks come ashore where they lie.
    let low_coast = Node {
        ground: 5.0,
        top: 0.0,
        floor: 0.0,
        hydration: 0.5,
        sea_distance: 200.0,
        offshore: 0.0,
    };
    let near = field(low_coast, 0);
    // The same coast behind a wall of high ground: the path from the sea
    // over ground under `SEA_PASS_M` is longer than the reach.
    let walled = field(
        Node {
            sea_distance: 3000.0,
            ..low_coast
        },
        0,
    );
    // Near its end of the reach it has all but faded.
    let far_inland = field(
        Node {
            sea_distance: 2900.0,
            ..low_coast
        },
        0,
    );
    // A cliff top by the sea: the ground stands over any bank's top.
    let cliff = field(
        Node {
            ground: 120.0,
            ..low_coast
        },
        0,
    );
    let w = weather(0.0, 1.0);
    let mut ashore = 0;
    for (x, z) in points() {
        let s = at(&near, x, z, &w);
        if s.density > 0.1 {
            ashore += 1;
            let inland = at(&far_inland, x, z, &w);
            assert!(inland.density < 0.1 * s.density, "{s:?} {inland:?}");
        }
        assert!(at(&walled, x, z, &w).density == 0.0, "walled off");
        assert!(at(&cliff, x, z, &w).density == 0.0, "cliff top");
        // No sea fog without the weather for it.
        assert!(
            at(&near, x, z, &weather(0.0, 0.0)).density == 0.0,
            "no weather"
        );
    }
    assert!(ashore > 50, "{ashore} of 400 points under a bank ashore");
}

#[test]
fn two_layers_combine_as_media_and_a_layer_with_no_fog_never_moves_the_top() {
    // A wet basin on the shore whose floor is the sea: both kinds of fog.
    let shore = Node {
        ground: 10.0,
        top: 60.0,
        floor: 0.0,
        hydration: 1.0,
        sea_distance: 100.0,
        offshore: 0.0,
    };
    let f = field(shore, 0);
    let (mut sea_higher, mut basin_higher) = (0, 0);
    for (x, z) in points() {
        let basin = at(&f, x, z, &weather(1.0, 0.0));
        let sea = at(&f, x, z, &weather(0.0, 1.0));
        let both = at(&f, x, z, &weather(1.0, 1.0));
        let expected = 1.0 - (1.0 - basin.density) * (1.0 - sea.density);
        assert!(
            (both.density - expected).abs() < 1e-6,
            "{both:?} {expected}"
        );
        // Once the higher layer is more than a trace, its top is the top.
        if sea.density > 0.1 && sea.top > basin.top {
            sea_higher += 1;
            assert!((both.top - sea.top).abs() < 1e-4, "{both:?} {sea:?}");
        }
        if sea.density > 0.0 && sea.top < basin.top {
            basin_higher += 1;
            assert!((both.top - basin.top).abs() < 1e-4, "{both:?} {basin:?}");
        }
        assert!(both.top >= basin.top.min(sea.top) - 1e-4);
        assert!(both.top <= basin.top.max(sea.top) + 1e-4);
    }
    assert!(
        sea_higher > 10 && basin_higher > 10,
        "{sea_higher} {basin_higher}"
    );

    // Out over the water a basin's top can stand at 100 m with no fog under
    // it (basin fog fades out offshore); the sea fog's lower top is the
    // layer's, not something between the two.
    let bay = field(
        Node {
            ground: 0.0,
            top: 100.0,
            floor: 0.0,
            hydration: 1.0,
            sea_distance: 0.0,
            offshore: 1000.0,
        },
        0,
    );
    let mut checked = 0;
    for (x, z) in points() {
        let basin = at(&bay, x, z, &weather(1.0, 0.0));
        let sea = at(&bay, x, z, &weather(0.0, 1.0));
        let both = at(&bay, x, z, &weather(1.0, 1.0));
        assert!(basin.density == 0.0 && basin.top > 99.0, "{basin:?}");
        if sea.density > 0.0 {
            checked += 1;
            assert!(sea.top < basin.top);
            assert!((both.top - sea.top).abs() < 1e-3, "{both:?} {sea:?}");
        }
    }
    assert!(checked > 10, "{checked}");
}

#[test]
fn basin_fog_fades_out_over_the_first_four_hundred_metres_of_water() {
    let w = weather(1.0, 0.0);
    let density = |offshore: f32| {
        let f = field(
            Node {
                offshore,
                ..WET_BASIN
            },
            0,
        );
        at(&f, 1000.0, 1000.0, &w).density
    };
    let (shore, out_100, out_200, out_300, out_400, out_1000) = (
        density(0.0),
        density(100.0),
        density(200.0),
        density(300.0),
        density(400.0),
        density(1000.0),
    );
    assert!(shore > 0.5, "{shore}");
    assert!(shore > out_100 && out_100 > out_200 && out_200 > out_300);
    // A smoothstep: half way out it holds half.
    assert!((out_200 / shore - 0.5).abs() < 1e-3, "{out_200} {shore}");
    assert!(out_400 == 0.0 && out_1000 == 0.0);
}

#[test]
fn tile_coordinates_floor_and_an_edge_belongs_to_the_next_tile() {
    let c = TileCoord::containing;
    assert_eq!(c(0.0, 0.0), TileCoord { x: 0, z: 0 });
    assert_eq!(c(-0.001, -0.001), TileCoord { x: -1, z: -1 });
    assert_eq!(c(TILE_M - 0.001, 0.0), TileCoord { x: 0, z: 0 });
    assert_eq!(c(TILE_M, -TILE_M), TileCoord { x: 1, z: -1 });
    assert_eq!(c(-TILE_M - 0.001, 3.0 * TILE_M), TileCoord { x: -2, z: 3 });
    assert_eq!(
        TileCoord { x: -2, z: 3 }.origin(),
        (-2.0 * TILE_M, 3.0 * TILE_M)
    );
    // `x` fastest, both ends included.
    let covered: Vec<TileCoord> = TileCoord::covering(-1.0, -1.0, TILE_M, 0.0).collect();
    assert_eq!(
        covered,
        [(-1, -1), (0, -1), (1, -1), (-1, 0), (0, 0), (1, 0)].map(|(x, z)| TileCoord { x, z })
    );
}

#[test]
fn the_field_says_what_it_is_missing_and_forgets_what_it_is_told_to() {
    let mut f = FogField::new(Seed(1));
    let a = TileCoord { x: 0, z: 0 };
    let b = TileCoord { x: -1, z: 0 };
    f.insert(Arc::new(tile(a, WET_BASIN))).expect("well formed");
    assert!(f.contains(a) && !f.contains(b));
    assert_eq!(f.missing(-10.0, 10.0, 10.0, 20.0), [b]);
    assert!(f.sample(-10.0, 10.0, &Weather::FULL).is_none());
    assert!(f.sample(10.0, 10.0, &Weather::FULL).is_some());

    // A grid across both: the unbaked half reads nothing and is counted.
    let mut out = vec![FogSample::NONE; 3];
    let step = 1000.0;
    let unbaked = f.sample_grid(-4000.0, 0.0, step, 8, &Weather::FULL, &mut out);
    assert_eq!(out.len(), 64);
    assert_eq!(unbaked, 32);
    for (k, s) in out.iter().enumerate() {
        let x = -4000.0 + step * f64::from(u32::try_from(k % 8).expect("small"));
        if x < 0.0 {
            assert_eq!(*s, FogSample::NONE, "{k}");
        } else {
            assert!(s.density > 0.0, "{k}: {s:?}");
        }
    }

    f.insert(Arc::new(tile(b, WET_BASIN))).expect("well formed");
    f.retain(|c| c.x >= 0);
    assert!(f.contains(a) && !f.contains(b));
    assert!(f.tile(a).is_some() && f.tile(b).is_none());
}

#[test]
fn a_tile_with_the_wrong_length_is_refused() {
    let mut f = FogField::new(Seed(1));
    let mut bad = tile(TileCoord { x: 0, z: 0 }, WET_BASIN);
    bad.offshore.pop();
    let err = f.insert(bad).expect_err("short channel");
    assert_eq!(err.channel, "offshore");
    assert_eq!(err.len, TILE_NODES * TILE_NODES - 1);
    assert!(f.sample(10.0, 10.0, &Weather::FULL).is_none());
    assert!(err.to_string().contains("offshore"), "{err}");
}

#[test]
fn a_sample_has_its_density_between_base_and_top_only() {
    let s = FogSample {
        base: 10.0,
        top: 50.0,
        density: 0.4,
    };
    for (y, d) in [
        (9.9, 0.0),
        (10.0, 0.4),
        (30.0, 0.4),
        (50.0, 0.4),
        (50.1, 0.0),
        (-100.0, 0.0),
    ] {
        assert!((s.density_at(y) - d).abs() < f32::EPSILON, "{y}");
    }
    assert!(FogSample::NONE.density_at(0.0) == 0.0, "nothing");
}
