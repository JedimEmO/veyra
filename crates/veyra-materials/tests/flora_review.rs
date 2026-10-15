//! Bakes the tree materials into PNGs for a person to look at.
//!
//! Ignored because it is a review tool rather than a check: it writes each
//! set into `scratch/flora-graphs/`, one at a time, so its shapes, value
//! structure, seams and margins can be judged by eye. Bark gets its base
//! colour, height, a preview under a low sun at the repeat's own proportions
//! and a three by three tiling of that; a leaf card gets its colour over a mid
//! grey by its alpha, the alpha, a preview lit from the front and through the
//! leaf from behind over a sky, and a two by two tiling that shows whether it
//! keeps off its edges. Run it with
//!
//! ```sh
//! cargo test --release -p veyra-materials --test flora_review -- --ignored --nocapture
//! VEYRA_FLORA=leaves-oak cargo test --release -p veyra-materials --test flora_review -- --ignored --nocapture
//! ```
use std::{collections::BTreeMap, num::NonZeroUsize, path::PathBuf};

use ashlar::Surface;
use ashlar_material::bake::{BakeRequest, Planes, linear_to_srgb, rasterise};
use image::{ImageBuffer, Rgb, RgbImage, imageops};

fn out_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../scratch/flora-graphs")
}

fn srgb8(value: f32) -> u8 {
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

fn unit(v: [f32; 3]) -> [f32; 3] {
    let l = (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt();
    v.map(|c| c / l)
}

fn dot(a: [f32; 3], b: [f32; 3]) -> f32 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

/// Base colour lit by a sun thirty degrees up from the top left, plus a sky
/// term darkened by the occlusion, in linear light.
fn lit_linear(planes: &Planes, i: usize) -> [f32; 3] {
    let sun = unit([-0.61, -0.50, 0.50]);
    let n = planes.normal[i];
    let diffuse = dot(n, sun).max(0.0);
    let sky = 0.35 * planes.occlusion[i] * (0.5 + 0.5 * n[2]);
    let light = sky + 1.1 * diffuse;
    planes.base_color[i].map(|c| c * light)
}

fn tone(v: f32) -> u8 {
    srgb8(v / (1.0 + 0.2 * v))
}

fn lit(planes: &Planes) -> RgbImage {
    image_of(planes.resolution, |i| lit_linear(planes, i).map(tone))
}

/// A leaf lit from the front as `lit` is, plus light let through from a sun
/// behind it, tinted by the leaf, over a pale sky where the card is cut away.
fn leaf_lit(planes: &Planes, opacity: &[f32]) -> RgbImage {
    let sky = [0.42, 0.55, 0.75];
    image_of(planes.resolution, |i| {
        let front = lit_linear(planes, i);
        let base = planes.base_color[i];
        let through = base.map(|c| 0.4 * 2.2 * c);
        let alpha = if opacity[i] >= 0.5 { 1.0 } else { 0.0 };
        [0, 1, 2].map(|k| tone(sky[k] * (1.0 - alpha) + (front[k] + through[k]) * alpha))
    })
}

/// The image scaled to `width` by `height` and laid `count` times each way.
fn tiled(image: &RgbImage, count: u32, width: u32, height: u32) -> RgbImage {
    let small = imageops::resize(image, width, height, imageops::FilterType::Triangle);
    let mut out = RgbImage::new(width * count, height * count);
    for ty in 0..count {
        for tx in 0..count {
            imageops::replace(
                &mut out,
                &small,
                i64::from(tx * width),
                i64::from(ty * height),
            );
        }
    }
    out
}

#[test]
#[ignore = "writes review PNGs; run by hand"]
fn bake_flora_for_review() {
    let only = std::env::var("VEYRA_FLORA").ok();
    let graphs = veyra_materials::graphs();
    let dir = out_dir();
    std::fs::create_dir_all(&dir).expect("the scratch directory can be made");
    for (key, definition) in veyra_materials::flora().materials {
        let set = key.trim_start_matches("flora:");
        if only.as_deref().is_some_and(|only| !set.starts_with(only)) {
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
        .expect("the set bakes");
        let coverage = planes
            .opacity
            .as_ref()
            .map(|opacity| ashlar_material::mips::coverage(opacity));
        // Texels in the outermost 2% of the card that are not fully cut away.
        let border = planes.opacity.as_ref().map(|opacity| {
            let n = planes.resolution as usize;
            let band = n.div_ceil(50);
            (0..n * n)
                .filter(|i| {
                    let (x, y) = (i % n, i / n);
                    (x < band || y < band || x >= n - band || y >= n - band) && opacity[*i] > 0.0
                })
                .count()
        });
        println!(
            "{set}: {:?}, {} ops, {} planes, height {:?}, lattice {:?}, strength {}, coverage {coverage:?}, border {border:?}, warnings {:?} {:?}",
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
        let height = planes.height.as_ref().expect("a flora set binds a height");
        save("height", &image_of(n, |i| [unit8(height[i]); 3]));
        if let Some(opacity) = &planes.opacity {
            let grey = 0.18;
            save(
                "base",
                &image_of(n, |i| {
                    let a = opacity[i].clamp(0.0, 1.0);
                    planes.base_color[i].map(|c| srgb8(c * a + grey * (1.0 - a)))
                }),
            );
            save("alpha", &image_of(n, |i| [unit8(opacity[i]); 3]));
            let preview = leaf_lit(&planes, opacity);
            save("lit", &preview);
            save("lit-2x2", &tiled(&preview, 2, n / 2, n / 2));
        } else {
            save("base", &image_of(n, |i| planes.base_color[i].map(srgb8)));
            let preview = lit(&planes);
            // The repeat at its own proportions, so a plate reads as long as
            // it is on the trunk.
            let [around, up] = definition.tile_metres;
            let width = n / 2;
            #[expect(
                clippy::cast_possible_truncation,
                clippy::cast_sign_loss,
                reason = "a small positive pixel count"
            )]
            let height = (f64::from(width) * f64::from(up / around)) as u32;
            save(
                "lit",
                &imageops::resize(&preview, width, height, imageops::FilterType::Triangle),
            );
            save("lit-3x3", &tiled(&preview, 3, width / 2, height / 2));
        }
    }
}
