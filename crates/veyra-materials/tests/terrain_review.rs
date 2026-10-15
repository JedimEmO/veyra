//! Bakes the terrain layers into PNGs for a person to look at.
//!
//! Ignored because it is a review tool rather than a check: it writes the
//! maps, a lit preview under a low sun and a three by three tiling of that
//! preview into `scratch/terrain-graphs/`, one set at a time, so a layer's
//! shape, value structure and seams can be judged by eye, and a two by two
//! tiling of the colour, the normal and the lit preview at full size, where a
//! seam would run through the middle. Run it with
//!
//! ```sh
//! cargo test --release -p veyra-materials --test terrain_review -- --ignored --nocapture
//! VEYRA_TERRAIN=grass cargo test --release -p veyra-materials --test terrain_review -- --ignored --nocapture
//! VEYRA_TERRAIN=granite,slate VEYRA_TERRAIN_OUT=scratch/review/terrain/step3/rock \
//!     cargo test --release -p veyra-materials --test terrain_review -- --ignored --nocapture
//! ```
//!
//! `VEYRA_TERRAIN` takes a comma-separated list of sets and
//! `VEYRA_TERRAIN_OUT` a directory relative to the workspace root.
use std::{collections::BTreeMap, num::NonZeroUsize, path::PathBuf};

use ashlar::Surface;
use ashlar_material::bake::{BakeRequest, Planes, linear_to_srgb, rasterise};
use image::{ImageBuffer, Rgb, RgbImage, imageops};

fn out_dir() -> PathBuf {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    std::env::var("VEYRA_TERRAIN_OUT").map_or_else(
        |_| root.join("scratch/terrain-graphs"),
        |dir| root.join(dir),
    )
}

fn srgb8(value: f32) -> u8 {
    // Truncation is the point: the value is clamped into one byte first.
    #[expect(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "clamped"
    )]
    let byte = (linear_to_srgb(value.clamp(0.0, 1.0)) * 255.0 + 0.5) as u8;
    byte
}

fn unit8(value: f32) -> u8 {
    #[expect(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "clamped"
    )]
    let byte = (value.clamp(0.0, 1.0) * 255.0 + 0.5) as u8;
    byte
}

fn image_of(n: u32, texel: impl Fn(usize) -> [u8; 3]) -> RgbImage {
    ImageBuffer::from_fn(n, n, |x, y| Rgb(texel((y * n + x) as usize)))
}

/// Base colour lit by a sun thirty degrees up from the top left, plus a sky
/// term darkened by the occlusion: the plainest light that still shows the
/// relief the way a low sun on the terrain will.
fn lit(planes: &Planes) -> RgbImage {
    let sun = {
        let v = [-0.61_f32, -0.50, 0.50];
        let l = (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt();
        v.map(|c| c / l)
    };
    image_of(planes.resolution, |i| {
        let n = planes.normal[i];
        let diffuse = (n[0] * sun[0] + n[1] * sun[1] + n[2] * sun[2]).max(0.0);
        let sky = 0.35 * planes.occlusion[i] * (0.5 + 0.5 * n[2]);
        let light = sky + 1.1 * diffuse;
        planes.base_color[i].map(|c| {
            let v = c * light;
            srgb8(v / (1.0 + 0.2 * v))
        })
    })
}

fn tiled(image: &RgbImage) -> RgbImage {
    let half = imageops::resize(
        image,
        image.width() / 2,
        image.height() / 2,
        imageops::FilterType::Triangle,
    );
    let n = half.width();
    let mut out = RgbImage::new(n * 3, n * 3);
    for ty in 0..3 {
        for tx in 0..3 {
            imageops::replace(&mut out, &half, i64::from(tx * n), i64::from(ty * n));
        }
    }
    out
}

/// Four copies at full size, so a seam would run as a cross through the
/// middle.
fn two_by_two(image: &RgbImage) -> RgbImage {
    let n = image.width();
    let mut out = RgbImage::new(n * 2, n * 2);
    for ty in 0..2 {
        for tx in 0..2 {
            imageops::replace(&mut out, image, i64::from(tx * n), i64::from(ty * n));
        }
    }
    out
}

#[test]
#[ignore = "writes review PNGs; run by hand"]
fn bake_terrain_for_review() {
    let only = std::env::var("VEYRA_TERRAIN").ok();
    let graphs = veyra_materials::graphs();
    let dir = out_dir();
    std::fs::create_dir_all(&dir).expect("the scratch directory can be made");
    for (key, definition) in veyra_materials::terrain().materials {
        let set = key.trim_start_matches("terrain:");
        if only
            .as_deref()
            .is_some_and(|only| !only.split(',').any(|one| one == set))
        {
            continue;
        }
        let Surface::Graph(bake) = &definition.surface else {
            panic!("{key} is not a graph bake");
        };
        let graph = graphs
            .get(&bake.graph)
            .expect("the graph is in the library");
        let material = graphs.build(&bake.graph).expect("the graph builds");
        let params: BTreeMap<_, _> = bake.params.clone();
        let (planes, report) = rasterise(&BakeRequest {
            graph,
            library: &graphs,
            params: &params,
            resolution: bake.resolution,
            mips: false,
            threads: NonZeroUsize::new(8),
        })
        .expect("the layer bakes");
        println!(
            "{set}: {:?}, {} ops, {} planes, height {:?}, lattice {:?}, strength {}, warnings {:?} {:?}",
            report.elapsed,
            report.ops,
            report.buffers,
            report.height_range,
            material.finest_lattice(),
            planes.normal_strength,
            material.warnings(),
            report.warnings,
        );
        let n = planes.resolution;
        let save = |name: &str, image: &RgbImage| {
            image
                .save(dir.join(format!("{set}-{name}.png")))
                .expect("the PNG is written");
        };
        let colour = image_of(n, |i| planes.base_color[i].map(srgb8));
        save("base", &colour);
        save("base-2x2", &two_by_two(&colour));
        let normal = image_of(n, |i| planes.normal[i].map(|c| unit8(c * 0.5 + 0.5)));
        save("normal", &normal);
        save("normal-2x2", &two_by_two(&normal));
        let height = planes
            .height
            .as_ref()
            .expect("a terrain layer binds a height");
        save("height", &image_of(n, |i| [unit8(height[i]); 3]));
        save(
            "orm",
            &image_of(n, |i| {
                [
                    unit8(planes.occlusion[i]),
                    unit8(planes.roughness[i]),
                    unit8(planes.metallic[i]),
                ]
            }),
        );
        let preview = lit(&planes);
        save("lit", &preview);
        save(
            "lit-crop",
            &imageops::crop_imm(&preview, 0, 0, n * 3 / 8, n * 3 / 8).to_image(),
        );
        save("lit-3x3", &tiled(&preview));
        save("lit-2x2", &two_by_two(&preview));
    }
}
