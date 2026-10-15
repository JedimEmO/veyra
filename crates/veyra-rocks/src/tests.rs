//! The contract the content step and the runtime code against.
#![allow(
    clippy::float_cmp,
    clippy::cast_possible_truncation,
    clippy::cast_precision_loss,
    reason = "the vertex data is f32 by contract and the pivot table is exact"
)]
use std::sync::OnceLock;
use std::time::{Duration, Instant};

use ashlar::VertexData;
use ashlar::glam::DVec3;
use ashlar::prop::PropCollider;

use crate::build::MAX_HULLS;
use crate::hull::Hull;
use crate::{Detail, GrownRock, RockKind, RockRecipe, VARIANTS, grow, recipes};

struct Shaped {
    recipe: RockRecipe,
    variants: Vec<(GrownRock, Duration)>,
}

/// Every species and variant, shaped once for all the tests.
fn all() -> &'static [Shaped] {
    static ALL: OnceLock<Vec<Shaped>> = OnceLock::new();
    ALL.get_or_init(|| {
        recipes()
            .into_iter()
            .map(|recipe| {
                let variants = (0..VARIANTS)
                    .map(|v| {
                        let start = Instant::now();
                        let built = grow(&recipe, v);
                        (built, start.elapsed())
                    })
                    .collect();
                Shaped { recipe, variants }
            })
            .collect()
    })
}

fn each(mut check: impl FnMut(&RockRecipe, u32, &GrownRock)) {
    for shaped in all() {
        for (v, (built, _)) in (0..).zip(&shaped.variants) {
            check(&shaped.recipe, v, built);
        }
    }
}

fn hulls(built: &GrownRock) -> Vec<Hull> {
    built
        .prop
        .colliders
        .iter()
        .map(|c| match c {
            PropCollider::ConvexHull(solid) => {
                Hull::new(&solid.vertices).expect("a hull collider spans a volume")
            }
            other => panic!("{other:?} is not a hull"),
        })
        .collect()
}

#[test]
fn the_species_are_the_registry_ids() {
    let ids: Vec<(&str, &str, RockKind)> = recipes()
        .iter()
        .map(|r| (r.species, r.material, r.kind))
        .collect();
    assert_eq!(
        ids,
        [
            ("strata:granite_boulder", "granite", RockKind::Boulder),
            ("strata:granite_stone", "granite", RockKind::Boulder),
            ("strata:sandstone_boulder", "sandstone", RockKind::Boulder),
            ("strata:sandstone_stone", "sandstone", RockKind::Boulder),
            ("strata:granite_cliff", "granite", RockKind::Cliff),
            ("strata:sandstone_cliff", "sandstone", RockKind::Cliff),
        ]
    );
}

#[test]
fn every_variant_checks_and_keeps_its_budget() {
    each(|recipe, v, built| {
        let prop = &built.prop;
        prop.check().expect("the prop validates");
        assert_eq!(prop.id, format!("rock:{}-{v}", recipe.name()));
        assert_eq!(prop.levels.len(), 3, "{}", prop.id);
        let until = recipe.detail.until();
        let budget = recipe.detail.budget();
        for (index, level) in prop.levels.iter().enumerate() {
            assert_eq!(level.until, Some(until[index]), "{}", prop.id);
            assert_eq!(
                level.cast_shadows,
                index == 0 || (recipe.detail == Detail::Cliff && index == 1),
                "{}",
                prop.id
            );
            assert!(level.impostor.is_none());
            assert_eq!(level.pieces.len(), 1, "{}", prop.id);
            assert_eq!(
                level.pieces[0].material,
                format!("rock:{}", recipe.material)
            );
        }
        let triangles = prop.triangles();
        for (index, (&t, &b)) in triangles.iter().zip(&budget).enumerate() {
            assert!(t <= b, "{} level {index}: {t} triangles over {b}", prop.id);
            assert!(
                t * 10 >= b * 6,
                "{} level {index}: {t} of {b} wastes the budget",
                prop.id
            );
        }
    });
}

#[test]
fn every_vertex_carries_its_data_in_range() {
    each(|_, _, built| {
        let prop = &built.prop;
        for (index, level) in prop.levels.iter().enumerate() {
            let mesh = &level.pieces[0].mesh;
            let count = mesh.positions.len();
            let colours = mesh.colors.as_ref().expect("the tint is present");
            assert_eq!(colours.len(), count);
            for c in colours {
                assert!(
                    c.iter().all(|v| (0.0..=1.2).contains(v)),
                    "{} level {index}: tint {c:?}",
                    prop.id
                );
                assert_eq!(c[3], 1.0);
            }
            let VertexData::F32x4(wind) = &mesh.attributes["wind"] else {
                panic!("wind is not F32x4");
            };
            assert_eq!(wind.len(), count);
            assert!(wind.iter().all(|w| *w == [0.0; 4]), "rocks do not sway");
            let VertexData::F32x4(shade) = &mesh.attributes["shade"] else {
                panic!("shade is not F32x4");
            };
            assert_eq!(shade.len(), count);
            for s in shade {
                assert!(
                    s.iter().all(|v| (0.0..=1.0).contains(v)),
                    "{} level {index}: shade {s:?}",
                    prop.id
                );
            }
            for n in &mesh.normals {
                assert!((n.length() - 1.0).abs() < 1e-6);
            }
        }
    });
}

#[test]
fn moss_grows_on_tops_and_never_on_walls_or_undersides() {
    each(|_, _, built| {
        let mesh = &built.prop.levels[0].pieces[0].mesh;
        let VertexData::F32x4(shade) = &mesh.attributes["shade"] else {
            panic!("shade is not F32x4");
        };
        for (n, s) in mesh.normals.iter().zip(shade) {
            if n.y < 0.3 {
                assert_eq!(s[3], 0.0, "moss on a face with normal {n}");
            }
        }
    });
}

#[test]
fn stones_fade_and_the_rest_get_impostors() {
    for recipe in recipes() {
        assert_eq!(
            recipe.impostor(),
            recipe.detail != Detail::Stone,
            "{}",
            recipe.species
        );
    }
    assert_eq!(Detail::Stone.until(), [25.0, 60.0, 150.0]);
    assert_eq!(Detail::Boulder.until(), [60.0, 150.0, 350.0]);
    assert_eq!(Detail::Cliff.until(), [60.0, 150.0, 350.0]);
}

#[test]
fn walls_read_down_the_world_and_tops_read_along_x_and_z() {
    each(|_, _, built| {
        let mesh = &built.prop.levels[0].pieces[0].mesh;
        for (p, uv) in mesh.positions.iter().zip(&mesh.uvs) {
            // Every vertex's v is its -y on a wall or its z on a top; both
            // cannot fail at once unless the projection is wrong.
            assert!(
                (uv[1] + p.y).abs() < 1e-9 || (uv[1] - p.z).abs() < 1e-9,
                "{}: v {} at {p}",
                built.prop.id,
                uv[1]
            );
        }
    });
}

#[test]
fn the_pivot_table_is_one_still_pivot() {
    each(|_, _, built| {
        let rock = built;
        assert_eq!(
            rock.prop.tables["pivots"],
            vec![
                0.0,
                0.0,
                0.0,
                -1.0,
                0.0,
                1.0,
                0.0,
                rock.height as f32,
                rock.radius as f32,
                0.1,
                10_000.0,
                0.0
            ]
        );
        assert_eq!(rock.prop.tables.len(), 1);
    });
}

#[test]
fn rocks_sit_in_the_ground() {
    each(|recipe, _, built| {
        let rock = built;
        let [min, max] = rock.prop.levels[0].pieces[0]
            .mesh
            .positions
            .iter()
            .fold([DVec3::INFINITY, DVec3::NEG_INFINITY], |[a, b], p| {
                [a.min(*p), b.max(*p)]
            });
        assert!(rock.height > 0.1 && rock.radius > 0.1, "{}", rock.prop.id);
        let total = max.y - min.y;
        let buried = -min.y / total;
        match recipe.kind {
            RockKind::Boulder => assert!(
                (0.09..=0.16).contains(&buried),
                "{}: {buried} buried",
                rock.prop.id
            ),
            RockKind::Cliff => {
                assert!(
                    (8.0..=25.5).contains(&rock.height),
                    "{}: {} m tall",
                    rock.prop.id,
                    rock.height
                );
                // The back runs into the slope a good way behind the front.
                assert!(
                    -min.z >= 0.3 * rock.height,
                    "{}: back reaches {} m for {} m of height",
                    rock.prop.id,
                    -min.z,
                    rock.height
                );
                let below = -min.y / rock.height;
                assert!(
                    (0.25..=0.45).contains(&below),
                    "{}: foot {below}",
                    rock.prop.id
                );
            }
        }
        let size = (max.x - min.x).max(max.z - min.z);
        if recipe.kind == RockKind::Boulder {
            let [low, high] = recipe.size;
            assert!(
                size >= low * 0.8 && size <= high * 1.2,
                "{}: {size} m across for {low} to {high}",
                rock.prop.id
            );
        }
    });
}

#[test]
fn hulls_hold_the_visible_rock() {
    each(|recipe, _, built| {
        let prop = &built.prop;
        let hulls = hulls(built);
        match recipe.kind {
            RockKind::Boulder => assert_eq!(hulls.len(), 1, "{}", prop.id),
            RockKind::Cliff => assert!((1..=MAX_HULLS).contains(&hulls.len()), "{}", prop.id),
        }
        let [min, max] = prop.levels[0].pieces[0]
            .mesh
            .positions
            .iter()
            .fold([DVec3::INFINITY, DVec3::NEG_INFINITY], |[a, b], p| {
                [a.min(*p), b.max(*p)]
            });
        let slack = DVec3::splat(0.05);
        for hull in &hulls {
            assert!(
                (4..=64).contains(&hull.points.len()),
                "{}: {} points",
                prop.id,
                hull.points.len()
            );
            for p in &hull.points {
                assert!(
                    p.cmpge(min - slack).all() && p.cmple(max + slack).all(),
                    "{}: hull point {p} outside {min}..{max}",
                    prop.id
                );
            }
        }
        let positions = &prop.levels[0].pieces[0].mesh.positions;
        let held = positions
            .iter()
            .filter(|&&p| hulls.iter().any(|h| h.outside(p) <= 0.1))
            .count();
        let share = held as f64 / positions.len() as f64;
        println!("{}: hulls hold {share:.3} of level 0", prop.id);
        assert!(
            share >= 0.85,
            "{}: hulls hold {share:.3} of level 0",
            prop.id
        );
    });
}

#[test]
fn the_same_variant_writes_the_same_bytes() {
    for recipe in recipes() {
        for v in [0, 5] {
            let a = grow(&recipe, v).prop.write().expect("writes");
            let b = grow(&recipe, v).prop.write().expect("writes");
            assert!(a == b, "{} {v} differs between runs", recipe.species);
        }
    }
}

#[test]
fn variants_differ() {
    for shaped in all() {
        let sizes: Vec<[i64; 3]> = shaped
            .variants
            .iter()
            .map(|(b, _)| {
                let [min, max] = b.prop.bounds;
                ((max - min) * 100.0).round().as_i64vec3().to_array()
            })
            .collect();
        for (i, a) in sizes.iter().enumerate() {
            for b in &sizes[i + 1..] {
                assert_ne!(a, b, "{}: two variants share a box", shaped.recipe.species);
            }
        }
    }
}

#[test]
fn shaping_is_quick() {
    for shaped in all() {
        let times: Vec<f64> = shaped
            .variants
            .iter()
            .map(|(_, t)| t.as_secs_f64())
            .collect();
        let worst = times.iter().copied().fold(0.0, f64::max);
        let mean = times.iter().sum::<f64>() / times.len() as f64;
        println!(
            "{}: mean {:.0} ms, worst {:.0} ms per variant, triangles {:?}",
            shaped.recipe.species,
            mean * 1e3,
            worst * 1e3,
            shaped.variants[0].0.prop.triangles()
        );
        if !cfg!(debug_assertions) {
            assert!(
                worst < 1.0,
                "{}: {worst:.2} s for one variant",
                shaped.recipe.species
            );
        }
    }
}
