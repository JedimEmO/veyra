//! What the value budget measures: the luminance of a baked base map as the
//! renderer would be given it, for the test that holds every shipped
//! material to its family's band (`assets/look/values.ron`).

use std::path::Path;

use anyhow::{Context, Result, bail};

use crate::bc7;

/// The `VkFormat`s a base map is baked as: BC7 sRGB and unorm, and the
/// RGBA8 sRGB of `just content --raw-textures`.
const BC7_SRGB: u32 = 146;
const BC7_UNORM: u32 = 145;
const RGBA8_SRGB: u32 = 43;

/// The mean and the standard deviation of a map's linear luminance over the
/// texels it covers (alpha of half or more, so a leaf card's cut-out does not
/// count as black).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Stats {
    /// Mean linear luminance, Rec. 709.
    pub mean: f32,
    /// Standard deviation of it.
    pub spread: f32,
}

fn srgb_to_linear(byte: u8) -> f32 {
    let c = f32::from(byte) / 255.0;
    if c <= 0.040_45 {
        c / 12.92
    } else {
        ((c + 0.055) / 1.055).powf(2.4)
    }
}

/// The statistics of RGBA8 sRGB texels.
#[must_use]
pub fn stats_of(texels: &[[u8; 4]]) -> Stats {
    let table: Vec<f32> = (0..=255).map(srgb_to_linear).collect();
    let (mut n, mut sum, mut square) = (0.0_f64, 0.0_f64, 0.0_f64);
    for t in texels {
        if t[3] < 128 {
            continue;
        }
        let l = f64::from(
            table[usize::from(t[0])] * 0.2126
                + table[usize::from(t[1])] * 0.7152
                + table[usize::from(t[2])] * 0.0722,
        );
        n += 1.0;
        sum += l;
        square += l * l;
    }
    if n == 0.0 {
        return Stats {
            mean: 0.0,
            spread: 0.0,
        };
    }
    let mean = sum / n;
    // The statistics of a map are far inside f32.
    #[allow(clippy::cast_possible_truncation)]
    Stats {
        mean: mean as f32,
        spread: (square / n - mean * mean).max(0.0).sqrt() as f32,
    }
}

/// The statistics of a baked base colour KTX2 file, at its first level.
///
/// # Errors
///
/// A file that is not readable, or is in a format the content step does not
/// write a base map in.
pub fn stats_of_ktx2(path: &Path) -> Result<Stats> {
    let bytes = std::fs::read(path).with_context(|| path.display().to_string())?;
    let levels = ashlar_material::ktx2::read_levels(&bytes)
        .map_err(|e| anyhow::anyhow!("{}: {e}", path.display()))?;
    let (w, h) = (levels.info.width as usize, levels.info.height as usize);
    let level = &levels.levels[0];
    let texels = match levels.info.vk_format {
        BC7_SRGB | BC7_UNORM => bc7::decode_level(level, w, h),
        RGBA8_SRGB => level.as_chunks::<4>().0.to_vec(),
        other => bail!(
            "{}: VkFormat {other} is not a base map format",
            path.display()
        ),
    };
    Ok(stats_of(&texels))
}

/// The statistics of the first image of a binary glTF, for the character.
///
/// # Errors
///
/// A file that is not a glb with an embedded PNG or JPEG first image.
pub fn stats_of_glb(path: &Path) -> Result<Stats> {
    let bytes = std::fs::read(path).with_context(|| path.display().to_string())?;
    let word = |at: usize| -> Result<usize> {
        let b: [u8; 4] = bytes
            .get(at..at + 4)
            .context("a truncated glb")?
            .try_into()
            .expect("four bytes");
        Ok(u32::from_le_bytes(b) as usize)
    };
    if bytes.get(..4) != Some(b"glTF") {
        bail!("{}: not a glb", path.display());
    }
    let json_len = word(12)?;
    let json: serde_json::Value = serde_json::from_slice(&bytes[20..20 + json_len])?;
    let bin_at = 20 + json_len + 8;
    let view = json["images"][0]["bufferView"]
        .as_u64()
        .context("the first image is not embedded")?;
    let view = &json["bufferViews"][usize::try_from(view)?];
    let offset = usize::try_from(view["byteOffset"].as_u64().unwrap_or(0))?;
    let length = usize::try_from(view["byteLength"].as_u64().context("a view length")?)?;
    let image =
        image::load_from_memory(&bytes[bin_at + offset..bin_at + offset + length])?.to_rgba8();
    let texels: Vec<[u8; 4]> = image.pixels().map(|p| p.0).collect();
    Ok(stats_of(&texels))
}

/// Every baked base map under `generated/` that the budget covers, as the key
/// `values.ron` names it by and the file: each `materials/<set>/base.ktx2`
/// (the capacity gate's scratch bakes aside) and each impostor atlas
/// (`flora/impostors/<name>/albedo.ktx2`, `rocks/impostors/...`), the latter
/// as `impostors/<name>`.
#[must_use]
pub fn baked_maps(generated: &Path) -> Vec<(String, std::path::PathBuf)> {
    fn walk(dir: &Path, out: &mut Vec<std::path::PathBuf>) {
        let Ok(read) = std::fs::read_dir(dir) else {
            return;
        };
        let mut entries: Vec<_> = read.flatten().collect();
        entries.sort_by_key(std::fs::DirEntry::file_name);
        for entry in entries {
            let path = entry.path();
            if path.is_dir() {
                walk(&path, out);
            } else if path
                .file_name()
                .is_some_and(|n| n == "base.ktx2" || n == "albedo.ktx2")
            {
                out.push(path);
            }
        }
    }
    let mut files = Vec::new();
    for root in ["materials", "flora/impostors", "rocks/impostors"] {
        walk(&generated.join(root), &mut files);
    }
    files
        .into_iter()
        .filter_map(|file| {
            let dir = file.parent()?;
            let key = if let Ok(rel) = dir.strip_prefix(generated.join("materials")) {
                if rel.starts_with("capacity") {
                    return None;
                }
                rel.display().to_string()
            } else {
                let name = dir.file_name()?.to_str()?;
                format!("impostors/{name}")
            };
            Some((key, file))
        })
        .collect()
}

/// What the flora remap did to one graph, for the build's log.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Remap {
    /// The graph's mean luminance and spread as authored.
    pub from: Stats,
    /// The mean it is drawn at.
    pub target: f32,
    /// The scale on the colour: the one that reaches `target`, held down so
    /// the spread stays inside the family's.
    pub scale: f32,
    /// The luminance added back, in the hue of the mean, so a texture whose
    /// scale was held down still reaches `target` without its highlights
    /// leaving the band.
    pub floor: f32,
}

/// The remap that brings a texture of `from` to `target` inside a family's
/// `spread`: a scale about zero where that keeps the detail inside the band
/// (a birch coming down), and where it would stretch it past (a fir leaf
/// rising from 0.036) a smaller scale with the rest of the lift added as a
/// floor, so the mean is `target` and the highlights stop short of the
/// band's top instead of leaving it, which a pure gain cannot do.
#[must_use]
pub fn remap_for(from: Stats, target: f32, spread: f32) -> Remap {
    let want = target / from.mean.max(1e-4);
    let cap = if from.spread > 1e-5 {
        0.9 * spread / from.spread
    } else {
        want
    };
    let scale = want.min(cap);
    Remap {
        from,
        target,
        scale,
        floor: (target - scale * from.mean).max(0.0),
    }
}

/// Mean luminance, spread and mean colour of a graph's base colour over the
/// texels it covers, baked unquantised.
///
/// # Errors
///
/// The bake's error, as text.
pub fn graph_stats(
    graph: &ashlar_material::MaterialGraph,
    library: &ashlar_material::MaterialGraphLibrary,
    resolution: u32,
) -> Result<(Stats, [f32; 3])> {
    let (planes, _) = ashlar_material::bake::rasterise(&ashlar_material::bake::BakeRequest {
        graph,
        library,
        params: &std::collections::BTreeMap::new(),
        resolution,
        mips: false,
        threads: None,
    })
    .map_err(|e| anyhow::anyhow!("{}: {e}", graph.id))?;
    let (mut n, mut sum, mut square) = (0.0_f64, 0.0_f64, 0.0_f64);
    let mut rgb = [0.0_f64; 3];
    for (i, c) in planes.base_color.iter().enumerate() {
        if planes.opacity.as_ref().is_some_and(|o| o[i] < 0.5) {
            continue;
        }
        let l = f64::from(luminance(*c));
        n += 1.0;
        sum += l;
        square += l * l;
        for k in 0..3 {
            rgb[k] += f64::from(c[k]);
        }
    }
    let n = n.max(1.0);
    let mean = sum / n;
    // Statistics of a map are far inside f32.
    #[allow(clippy::cast_possible_truncation)]
    let stats = Stats {
        mean: mean as f32,
        spread: (square / n - mean * mean).max(0.0).sqrt() as f32,
    };
    #[allow(clippy::cast_possible_truncation)]
    Ok((stats, rgb.map(|c| (c / n) as f32)))
}

fn luminance(c: [f32; 3]) -> f32 {
    strata_data::luminance(c)
}

/// Rewrites a graph's base colour output as `colour * scale + floor` (the
/// floor in the hue of the mean), so what is baked, near or as an impostor,
/// is the budget's value and no later multiply is needed.
pub fn apply_remap(graph: &mut ashlar_material::MaterialGraph, remap: &Remap, mean_rgb: [f32; 3]) {
    use ashlar_material::{Input, MathOp, nodes::Math};
    let lum = remap.from.mean.max(1e-4);
    let floor = mean_rgb.map(|c| remap.floor * c / lum);
    let source = std::mem::take(&mut graph.output.base_color);
    graph.nodes.insert(
        "value_scaled".to_owned(),
        Math::new(MathOp::Mul, source, Input::float(remap.scale)).into(),
    );
    graph.nodes.insert(
        "value_budget".to_owned(),
        Math::new(
            MathOp::Add,
            Input::node("value_scaled"),
            Input::color(floor),
        )
        .into(),
    );
    graph.output.base_color = Input::node("value_budget");
}

/// Remaps every flora graph to its budgeted value (`flora/<set>` in
/// `assets/look/values.ron`): the baked bark and leaves, and so the
/// impostors drawn from them, land in their family's band, and the tree
/// materials in `strata-foliage` take the texture as it is.
///
/// # Errors
///
/// A graph that does not bake, or a flora material with no budget entry.
pub fn budget_flora(
    graphs: &mut ashlar_material::MaterialGraphLibrary,
    definitions: &ashlar::MaterialLibrary,
    budget: &strata_data::ValueBudget,
) -> Result<Vec<(String, Remap)>> {
    let mut done = Vec::new();
    for (key, definition) in &definitions.materials {
        let Some(set) = key.strip_prefix("flora:") else {
            continue;
        };
        let ashlar::Surface::Graph(bake) = &definition.surface else {
            continue;
        };
        let path = format!("flora/{set}");
        let Some(target) = budget.target_of(&path) else {
            continue;
        };
        let family = &budget.families[&budget.materials[&path].family];
        let graph = graphs
            .get(&bake.graph)
            .with_context(|| format!("{key} names the graph {}", bake.graph))?
            .clone();
        let (stats, rgb) = graph_stats(&graph, graphs, bake.resolution)?;
        let remap = remap_for(stats, target, family.spread);
        let mut graph = graph;
        apply_remap(&mut graph, &remap, rgb);
        graphs.graphs.insert(bake.graph.clone(), graph);
        done.push((path, remap));
    }
    Ok(done)
}
