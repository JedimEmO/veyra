//! Review pictures of every species, written to `scratch/trees/`:
//!
//! ```text
//! cargo test --release -p veyra-flora --test review -- --ignored --nocapture
//! ```
//!
//! Per species a lit contact sheet of the six variants at the nearest level,
//! a 64 px silhouette sheet from two sides, and variant 0's three levels
//! side by side with their silhouettes. Ignored because it takes a while and
//! writes files; nothing here is asserted, it is for eyes.
use std::path::PathBuf;
use std::time::Instant;

use ashlar::glam::DVec3;
use veyra_flora::review::{Camera, Cutout, Image, Style, render, side_camera};
use veyra_flora::{GrownTree, TreeRecipe, VARIANTS, grow, recipes};

/// Trees and scrub go to `scratch/trees/`, the understory to
/// `scratch/understory/`.
fn out_dir(recipe: &TreeRecipe) -> PathBuf {
    let folder = if recipe.impostor() {
        "trees"
    } else {
        "understory"
    };
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../scratch")
        .join(folder);
    std::fs::create_dir_all(&dir).expect("the scratch folder can be created");
    dir
}

fn save(recipe: &TreeRecipe, image: &Image, name: &str) {
    let path = out_dir(recipe).join(name);
    image::RgbaImage::from_raw(image.width, image.height, image.rgba.clone())
        .expect("the buffer is width by height")
        .save(&path)
        .expect("the PNG writes");
    println!("wrote {}", path.display());
}

/// Flat colours standing in for the baked materials, and the cut-out that
/// stands in for the leaf card's alpha.
fn style(recipe: &TreeRecipe) -> Style {
    let (bark, leaves, cards) = match recipe.name() {
        "oak" => ([0.16, 0.11, 0.075], [0.11, 0.24, 0.045], Cutout::Cluster),
        "pine" => ([0.24, 0.12, 0.07], [0.055, 0.16, 0.07], Cutout::Cluster),
        "birch" => ([0.72, 0.70, 0.66], [0.24, 0.40, 0.07], Cutout::Cluster),
        "fir" => ([0.15, 0.11, 0.09], [0.035, 0.13, 0.075], Cutout::Cluster),
        "woodland_fern" => ([0.2, 0.16, 0.08], [0.2, 0.42, 0.07], Cutout::Frond),
        // Flowers are white, painted by their vertex colours.
        "wildflower_amber" | "wildflower_violet" => ([0.8; 3], [0.8; 3], Cutout::Solid),
        "woodland_log" => ([0.2, 0.14, 0.09], [0.17, 0.32, 0.07], Cutout::Cluster),
        "woodland_shrub" => ([0.19, 0.14, 0.09], [0.12, 0.3, 0.06], Cutout::Cluster),
        _ => ([0.19, 0.14, 0.09], [0.17, 0.29, 0.07], Cutout::Cluster),
    };
    Style::lit(bark, leaves).with_cards(cards)
}

/// One box around every variant, so a sheet shows them at one scale.
fn shared_bounds(trees: &[GrownTree]) -> [DVec3; 2] {
    let mut min = DVec3::splat(f64::INFINITY);
    let mut max = DVec3::splat(f64::NEG_INFINITY);
    for tree in trees {
        min = min.min(tree.prop.bounds[0]);
        max = max.max(tree.prop.bounds[1]);
    }
    // Centre each tree on its own trunk: symmetric about the origin.
    let reach = min
        .x
        .abs()
        .max(max.x.abs())
        .max(min.z.abs())
        .max(max.z.abs());
    [
        DVec3::new(-reach, min.y, -reach),
        DVec3::new(reach, max.y, reach),
    ]
}

fn camera_for(bounds: [DVec3; 2], yaw: f64, pixels: u32) -> Camera {
    let [min, max] = bounds;
    let aspect = ((max.x - min.x) / (max.y - min.y.min(0.0))).clamp(0.45, 1.6);
    let mut camera = side_camera(bounds, yaw, pixels, aspect);
    camera.centre.x = 0.0;
    camera.centre.z = 0.0;
    camera
}

#[test]
#[ignore = "writes review pictures to scratch/trees; run by hand"]
fn review_sheets() {
    const SUPER: u32 = 2;
    for recipe in recipes() {
        let name = recipe.name();
        let start = Instant::now();
        let trees: Vec<GrownTree> = (0..VARIANTS).map(|v| grow(&recipe, v)).collect();
        println!(
            "{name}: grew {VARIANTS} variants in {:.2} s",
            start.elapsed().as_secs_f64()
        );
        for (v, tree) in trees.iter().enumerate() {
            let pivots = tree.prop.tables["pivots"].len() / 12;
            let split: Vec<String> = tree
                .prop
                .levels
                .iter()
                .map(|l| {
                    format!(
                        "{}+{}",
                        l.pieces[0].mesh.triangle_count(),
                        l.pieces[1].mesh.triangle_count()
                    )
                })
                .collect();
            println!(
                "  {name}-{v}: height {:.1} m, triangles {:?} (bark+leaves {}), pivots {pivots}",
                tree.height,
                tree.prop.triangles(),
                split.join(" ")
            );
        }
        let bounds = shared_bounds(&trees);
        let lit = style(&recipe);
        let black = Style::silhouette().with_cards(lit.cards);

        // Contact sheet: every variant at the nearest level, lit.
        let cells: Vec<Image> = trees
            .iter()
            .map(|t| {
                render(
                    &t.prop.levels[0],
                    &camera_for(bounds, 0.5, 520 * SUPER),
                    &lit,
                )
                .downsample(SUPER)
            })
            .collect();
        save(
            &recipe,
            &Image::sheet(&cells, 6, 6, [255; 4]),
            &format!("{name}-variants.png"),
        );

        // Silhouettes at 64 px, from the front and from the side.
        let silhouettes: Vec<Image> = [0.5, 0.5 + core::f64::consts::FRAC_PI_2]
            .iter()
            .flat_map(|&yaw| {
                trees.iter().map(move |t| {
                    render(&t.prop.levels[0], &camera_for(bounds, yaw, 64 * 4), &black)
                        .downsample(4)
                })
            })
            .collect();
        save(
            &recipe,
            &Image::sheet(&silhouettes, 6, 4, [200, 200, 200, 255]),
            &format!("{name}-silhouettes.png"),
        );

        // Variant 0's three levels, lit and as silhouettes.
        let tree = &trees[0];
        let own = shared_bounds(std::slice::from_ref(tree));
        let mut levels: Vec<Image> = tree
            .prop
            .levels
            .iter()
            .map(|l| render(l, &camera_for(own, 0.5, 520 * SUPER), &lit).downsample(SUPER))
            .collect();
        levels.extend(
            tree.prop
                .levels
                .iter()
                .map(|l| render(l, &camera_for(own, 0.5, 128 * 4), &black).downsample(4)),
        );
        save(
            &recipe,
            &Image::sheet(
                &levels,
                u32::try_from(tree.prop.levels.len()).expect("a few levels"),
                6,
                [255; 4],
            ),
            &format!("{name}-levels.png"),
        );
    }
}
