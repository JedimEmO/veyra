//! The contract the content step and the runtime code against.
#![allow(
    clippy::float_cmp,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::cast_precision_loss,
    reason = "the vertex data is f32 by contract and its indices are exact"
)]
use std::sync::OnceLock;
use std::time::{Duration, Instant};

use ashlar::VertexData;
use ashlar::glam::DVec3;
use ashlar::prop::PropCollider;

use crate::levels::{self, Built, MAX_PIVOTS, PIVOT_STRIDE};
use crate::{Habit, OLD, PLAIN, TreeRecipe, VARIANTS, YOUNG, recipes};

struct Grown {
    recipe: TreeRecipe,
    variants: Vec<(Built, Duration)>,
}

/// Every species and variant, grown once for all the tests.
fn all() -> &'static [Grown] {
    static ALL: OnceLock<Vec<Grown>> = OnceLock::new();
    ALL.get_or_init(|| {
        recipes()
            .into_iter()
            .map(|recipe| {
                let variants = (0..VARIANTS)
                    .map(|v| {
                        let start = Instant::now();
                        let built = levels::build(&recipe, v);
                        (built, start.elapsed())
                    })
                    .collect();
                Grown { recipe, variants }
            })
            .collect()
    })
}

fn each(mut check: impl FnMut(&TreeRecipe, u32, &Built)) {
    for grown in all() {
        for (v, (built, _)) in (0..).zip(&grown.variants) {
            check(&grown.recipe, v, built);
        }
    }
}

fn f32x4<'a>(built: &'a Built, level: usize, piece: usize, name: &str) -> &'a [[f32; 4]] {
    match &built.tree.prop.levels[level].pieces[piece].mesh.attributes[name] {
        VertexData::F32x4(values) => values,
        other => panic!("{name} is {other:?}, not F32x4"),
    }
}

/// Grown by the tree pipeline: a skeleton with pipe radii and leaf cards.
fn is_tree(habit: Habit) -> bool {
    matches!(
        habit,
        Habit::Broadleaf | Habit::Conifer | Habit::Shrub | Habit::Bush
    )
}

#[test]
fn the_species_are_the_registry_ids() {
    let ids: Vec<&str> = recipes().iter().map(|r| r.species).collect();
    assert_eq!(
        ids,
        [
            "strata:oak",
            "strata:pine",
            "strata:birch",
            "strata:fir",
            "strata:scrub",
            "strata:woodland_fern",
            "strata:wildflower_amber",
            "strata:wildflower_violet",
            "strata:woodland_shrub",
            "strata:woodland_log",
            "strata:reed",
        ]
    );
    let sets: Vec<(&str, &str)> = recipes().iter().map(|r| (r.bark, r.leaves)).collect();
    assert_eq!(
        sets,
        [
            ("oak", "oak"),
            ("pine", "pine"),
            ("birch", "birch"),
            ("fir", "fir"),
            ("shrub", "shrub"),
            ("shrub", "fern"),
            ("plain", "plain"),
            ("plain", "plain"),
            ("shrub", "shrub"),
            ("oak", "shrub"),
            ("plain", "plain"),
        ]
    );
}

#[test]
fn only_trees_and_scrub_get_an_impostor() {
    for recipe in recipes() {
        let expected = matches!(recipe.name(), "oak" | "pine" | "birch" | "fir" | "scrub");
        assert_eq!(recipe.impostor(), expected, "{}", recipe.name());
    }
}

#[test]
fn every_variant_checks_and_keeps_its_budget() {
    each(|recipe, v, built| {
        let prop = &built.tree.prop;
        prop.check().expect("the prop validates");
        assert_eq!(prop.id, format!("flora:{}-{v}", recipe.name()));
        let (until, budget) = levels::plan(recipe.habit);
        assert_eq!(prop.levels.len(), until.len(), "{}", prop.id);
        for (index, level) in prop.levels.iter().enumerate() {
            assert_eq!(level.until, Some(until[index]), "{}", prop.id);
            // Trees and shrubs cast from their nearest level; the
            // understory never does.
            let casts = matches!(
                recipe.habit,
                Habit::Broadleaf | Habit::Conifer | Habit::Shrub
            );
            assert_eq!(level.cast_shadows, casts && index == 0, "{}", prop.id);
            assert!(level.impostor.is_none());
            assert_eq!(level.pieces.len(), 2, "{}", prop.id);
            assert_eq!(level.pieces[0].material, recipe.bark_material());
            assert_eq!(level.pieces[1].material, recipe.leaf_material());
            for piece in &level.pieces {
                assert!(piece.mesh.triangle_count() > 0, "{} level {index}", prop.id);
            }
        }
        let triangles = prop.triangles();
        for (index, (&t, &b)) in triangles.iter().zip(budget).enumerate() {
            assert!(t <= b, "{} level {index}: {t} triangles over {b}", prop.id);
        }
        assert!(
            triangles.windows(2).all(|w| w[1] < w[0]),
            "{}: {triangles:?} do not coarsen",
            prop.id
        );
        assert!(built.tree.height > 0.05);
    });
}

#[test]
fn material_keys_follow_the_sets() {
    for recipe in recipes() {
        if matches!(recipe.habit, Habit::Flower | Habit::Reed) {
            assert_eq!(recipe.bark_material(), PLAIN);
            assert_eq!(recipe.leaf_material(), PLAIN);
        } else {
            assert_eq!(
                recipe.bark_material(),
                format!("flora:bark-{}", recipe.bark)
            );
            assert_eq!(
                recipe.leaf_material(),
                format!("flora:leaves-{}", recipe.leaves)
            );
        }
    }
}

#[test]
fn broadleaves_use_most_of_their_nearest_budget() {
    each(|recipe, v, built| {
        if !matches!(recipe.habit, Habit::Broadleaf | Habit::Shrub | Habit::Bush) || v == YOUNG {
            return;
        }
        let budget = levels::plan(recipe.habit).1[0];
        let t = built.tree.prop.triangles()[0];
        assert!(
            t * 10 >= budget * 6,
            "{}: {t} of {budget}",
            built.tree.prop.id
        );
    });
}

#[test]
fn vertex_data_is_on_every_vertex_and_in_range() {
    each(|recipe, _, built| {
        let prop = &built.tree.prop;
        let pivots = prop.tables["pivots"].len() / PIVOT_STRIDE;
        for (l, level) in prop.levels.iter().enumerate() {
            for (p, piece) in level.pieces.iter().enumerate() {
                let n = piece.mesh.positions.len();
                assert_eq!(piece.mesh.attributes.len(), 2, "wind and shade only");
                let wind = f32x4(built, l, p, "wind");
                let shade = f32x4(built, l, p, "shade");
                assert_eq!(wind.len(), n);
                assert_eq!(shade.len(), n);
                for w in wind {
                    assert_eq!(w[0].fract(), 0.0, "pivot index {w:?}");
                    assert!(w[0] >= 0.0 && (w[0] as usize) < pivots, "{} {w:?}", prop.id);
                    assert!([0.0, 1.0, 2.0].contains(&w[1]), "{w:?}");
                    assert!((0.0..=1.0).contains(&w[2]), "{w:?}");
                    if p == 0 {
                        assert_eq!(w[2], 0.0, "bark does not flutter");
                    }
                    assert!((0.0..=1.0).contains(&w[3]), "{w:?}");
                }
                for s in shade {
                    assert!((0.0..=1.0).contains(&s[0]), "ao {s:?}");
                    assert!((0.0..=1.0).contains(&s[1]), "height {s:?}");
                    assert!((-1.0..=1.0).contains(&s[2]), "curvature {s:?}");
                    assert!((0.0..=1.0).contains(&s[3]), "depth {s:?}");
                }
            }
            // Leaves flutter most at their tips and edges and not at all at
            // the stem.
            let leaves = f32x4(built, l, 1, "wind");
            let (lo, hi) = leaves
                .iter()
                .fold((1.0_f32, 0.0_f32), |(a, b), w| (a.min(w[2]), b.max(w[2])));
            assert!(lo < 0.15 && hi > 0.45, "{}: flutter {lo}..{hi}", prop.id);
        }
        // The shade data means something: open sky on the outside and some
        // occlusion inside, where a crown has an inside.
        if is_tree(recipe.habit) {
            let shade = f32x4(built, 0, 1, "shade");
            let (lo, hi) = shade
                .iter()
                .fold((1.0_f32, 0.0_f32), |(a, b), s| (a.min(s[0]), b.max(s[0])));
            assert!(hi - lo > 0.3, "{}: ao {lo}..{hi}", prop.id);
            assert!(shade.iter().any(|s| s[2] > 0.1) && shade.iter().any(|s| s[2] < -0.1));
            assert!(shade.iter().any(|s| s[3] < 0.1) && shade.iter().any(|s| s[3] > 0.5));
        }
    });
}

#[test]
fn the_pivot_table_is_a_shallow_tree() {
    each(|recipe, _, built| {
        let prop = &built.tree.prop;
        let table = &prop.tables["pivots"];
        assert_eq!(table.len() % PIVOT_STRIDE, 0);
        let count = table.len() / PIVOT_STRIDE;
        assert!(
            (1..=MAX_PIVOTS).contains(&count),
            "{}: {count} pivots",
            prop.id
        );
        let pivot = |i: usize| &table[i * PIVOT_STRIDE..(i + 1) * PIVOT_STRIDE];
        let root = pivot(0);
        assert_eq!(root[3], -1.0, "pivot 0 has no parent");
        assert_eq!(root[1], 0.0, "pivot 0 is at the ground");
        assert!(root[0].hypot(root[2]) < 0.5, "pivot 0 is at the trunk");
        let mut depth = vec![0_u32; count];
        for i in 0..count {
            let p = pivot(i);
            if i > 0 {
                let parent = p[3];
                assert_eq!(parent.fract(), 0.0);
                assert!(
                    parent >= 0.0 && (parent as usize) < i,
                    "{}: pivot {i} parent {parent}",
                    prop.id
                );
                depth[i] = depth[parent as usize] + 1;
            }
            assert!(depth[i] <= 2, "the runtime walks three levels");
            let axis = DVec3::new(f64::from(p[4]), f64::from(p[5]), f64::from(p[6]));
            assert!((axis.length() - 1.0).abs() < 1e-3, "unit axis {p:?}");
            assert!(p[7] > 0.0 && p[8] > 0.0, "length and radius {p:?}");
            assert!((0.3..=3.0).contains(&p[9]), "frequency {p:?}");
            if recipe.habit == Habit::Log {
                assert_eq!(p[10], 10_000.0, "a log does not move");
            } else {
                assert!((0.0..=1.0).contains(&p[10]), "stiffness {p:?}");
            }
            assert_eq!(p[11], 0.0);
        }
        // Each vertex's hierarchy level is its pivot's depth.
        for (l, level) in prop.levels.iter().enumerate() {
            for p in 0..level.pieces.len() {
                for w in f32x4(built, l, p, "wind") {
                    assert_eq!(w[1] as u32, depth[w[0] as usize]);
                }
            }
        }
        match recipe.habit {
            Habit::Log => assert_eq!(count, 1, "{}: one still pivot", prop.id),
            // A pivot per frond or stem, under the root.
            Habit::Fern | Habit::Flower | Habit::Reed => {
                assert!(count > 3, "{}: {count} pivots", prop.id);
                assert!((1..count).all(|i| pivot(i)[3] == 0.0), "{}", prop.id);
            }
            _ => assert!(count > 3, "{} has branches that sway on their own", prop.id),
        }
    });
}

#[test]
fn bark_wraps_a_whole_number_of_tiles() {
    each(|recipe, _, built| {
        let tile = levels::bark_tile(recipe)[0];
        for (l, rings) in built.rings.iter().enumerate() {
            let mesh = &built.tree.prop.levels[l].pieces[0].mesh;
            assert!(!rings.is_empty());
            for &(start, count, repeats) in rings {
                assert_eq!(repeats.fract(), 0.0);
                assert!(repeats >= 1.0);
                let first = mesh.uvs[start][0];
                let last = mesh.uvs[start + count - 1][0];
                let span = (last - first) / tile;
                assert!(
                    (span - span.round()).abs() < 1e-9,
                    "{span} tiles around a ring"
                );
                assert!((span - repeats).abs() < 1e-9);
                // The seam closes: first and last vertex coincide.
                let gap = mesh.positions[start].distance(mesh.positions[start + count - 1]);
                assert!(gap < 1e-9);
            }
        }
    });
}

#[test]
fn trees_and_logs_have_one_capsule_and_the_rest_none() {
    each(|recipe, _, built| {
        let prop = &built.tree.prop;
        match recipe.habit {
            // The understory is walked through, as it is today.
            Habit::Bush | Habit::Fern | Habit::Flower | Habit::Reed => {
                assert!(prop.colliders.is_empty(), "{}", prop.id);
                return;
            }
            _ => assert_eq!(prop.colliders.len(), 1, "{}", prop.id),
        }
        let PropCollider::Capsule { a, b, radius } = &prop.colliders[0] else {
            panic!("{}: not a capsule", prop.id);
        };
        assert!(*radius > 0.01);
        if recipe.habit == Habit::Log {
            // Lying on the ground along +X, most of the log's length.
            assert!((a.y - radius).abs() < 1e-9 && (b.y - radius).abs() < 1e-9);
            let span = *b - *a;
            assert!(span.x > 0.0 && span.x.abs() > span.z.abs() * 4.0, "{span}");
            let [min, max] = prop.bounds;
            assert!(
                span.length() + 2.0 * radius > (max.x - min.x) * 0.8,
                "{}",
                prop.id
            );
            return;
        }
        assert!(
            a.y >= 0.0 && a.y <= radius * 1.01,
            "{}: starts at {a}",
            prop.id
        );
        let top = if recipe.habit == Habit::Shrub {
            0.25
        } else {
            0.55
        };
        assert!(
            b.y >= built.tree.height * top * 0.9 && b.y <= built.tree.height * 0.7,
            "{}: {b}",
            prop.id
        );
        assert!(
            a.x.hypot(a.z) < radius * 2.0,
            "{}: capsule on the trunk",
            prop.id
        );
    });
}

#[test]
fn cards_face_the_crown_masses_and_hang_on_wood() {
    each(|recipe, _, built| {
        // A flower's petals and a reed's blades keep half their own shape:
        // they are geometry, not cards.
        if matches!(recipe.habit, Habit::Flower | Habit::Reed) {
            return;
        }
        let mesh = &built.tree.prop.levels[0].pieces[1].mesh;
        for (p, n) in mesh.positions.iter().zip(&mesh.normals) {
            let envelope = built.field.normal(*p);
            assert!(
                n.dot(envelope) >= 0.95,
                "{}: {} leans off its envelope normal",
                built.tree.prop.id,
                n.dot(envelope)
            );
        }
        assert!(
            built.card_offsets.iter().all(|&d| d < 1e-9),
            "{}: a card off its shoot",
            built.tree.prop.id
        );
    });
}

#[test]
fn radii_follow_the_pipe_model() {
    each(|recipe, _, built| {
        if !is_tree(recipe.habit) {
            return;
        }
        assert!(!built.pipe_ratios.is_empty());
        for &ratio in &built.pipe_ratios {
            // The parent carries its children's pipes plus its own taper,
            // never less.
            assert!((0.999..4.0).contains(&ratio), "{}: {ratio}", recipe.name());
        }
    });
}

#[test]
fn flowers_are_coloured_on_every_vertex() {
    each(|recipe, _, built| {
        let Some(bloom) = &recipe.bloom else {
            return;
        };
        for level in &built.tree.prop.levels {
            for piece in &level.pieces {
                let colours = piece.mesh.colors.as_ref().expect("a flower is coloured");
                assert_eq!(colours.len(), piece.mesh.positions.len());
                assert!(colours.iter().flatten().all(|c| (0.0..=1.0).contains(c)));
            }
            // Stems are green; some of the rest is the petal colour.
            let stems = level.pieces[0].mesh.colors.as_ref().expect("coloured");
            assert!(stems.iter().all(|c| c[1] > c[0] && c[1] > c[2]));
            let close = |c: &[f32; 4]| {
                let scale = c[0] / bloom.petal[0];
                (0..3).all(|i| (c[i] - bloom.petal[i] * scale).abs() < 0.05)
            };
            let rest = level.pieces[1].mesh.colors.as_ref().expect("coloured");
            assert!(rest.iter().any(close), "{} has no petals", recipe.name());
        }
    });
}

#[test]
fn logs_are_mossy_on_top_only() {
    each(|recipe, _, built| {
        if recipe.habit != Habit::Log {
            return;
        }
        let mesh = &built.tree.prop.levels[0].pieces[0].mesh;
        let colours = mesh.colors.as_ref().expect("a log carries its moss");
        let green = |c: &[f32; 4]| c[1] - c[0] > 0.05;
        let mut mossy_up = false;
        for (c, n) in colours.iter().zip(&mesh.normals) {
            if n.y < 0.0 {
                assert!(!green(c), "moss under a log");
            }
            mossy_up |= n.y > 0.7 && green(c);
        }
        assert!(mossy_up, "{}: no moss on top", built.tree.prop.id);
    });
}

#[test]
fn growth_is_the_same_every_time() {
    for recipe in recipes() {
        for v in [0, OLD] {
            let a = crate::grow(&recipe, v).prop.write().expect("writes");
            let b = crate::grow(&recipe, v).prop.write().expect("writes");
            assert!(a == b, "{} {v} differs between runs", recipe.name());
        }
    }
}

#[test]
fn variants_differ() {
    for grown in all() {
        let bytes: Vec<Vec<u8>> = grown
            .variants
            .iter()
            .map(|(b, _)| b.tree.prop.write().expect("writes"))
            .collect();
        for i in 0..bytes.len() {
            for j in i + 1..bytes.len() {
                assert!(bytes[i] != bytes[j], "{} {i} and {j}", grown.recipe.name());
            }
        }
    }
}

#[test]
fn young_is_small_and_old_is_big() {
    for grown in all() {
        let height = |v: u32| grown.variants[v as usize].0.tree.height;
        let mature = (0..YOUNG).map(height).fold(f64::INFINITY, f64::min);
        let tallest = (0..YOUNG).map(height).fold(0.0, f64::max);
        assert!(height(YOUNG) < mature * 0.8, "{}", grown.recipe.name());
        // A flower clump grows wider with age, not much taller.
        if grown.recipe.habit != Habit::Flower {
            assert!(height(OLD) > tallest, "{}", grown.recipe.name());
        }
    }
}

#[test]
fn generation_time() {
    for grown in all() {
        for (v, (_, time)) in grown.variants.iter().enumerate() {
            println!(
                "{}-{v}: {:.0} ms",
                grown.recipe.name(),
                time.as_secs_f64() * 1000.0
            );
            if !cfg!(debug_assertions) {
                assert!(
                    time.as_secs_f64() < 2.0,
                    "{}-{v} took {time:?}",
                    grown.recipe.name()
                );
            }
        }
    }
}
