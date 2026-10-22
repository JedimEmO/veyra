//! The terrain's baked arrays: the layer maps `ship` wrote, folded into the
//! two block-compressed KTX2 arrays `strata-terrain` loads
//! (`strata_registry::terrain_arrays`).
//!
//! The mip chain is the layer maps' own, which ashlar built from float planes
//! (renormalised normals, roughness widened for the variance the normals lost),
//! so every level is folded and compressed as it stands. The terrain used to
//! rebuild the chain on the CPU with a box average of the packed bytes and take
//! its roughness from level 0.

use anyhow::{Context, Result, bail, ensure};
use ashlar_material::{
    bake::{Encoded, PlaneFormat},
    ktx2::{self, BlockFormat, Supercompression},
};
use strata_registry::terrain_arrays::{
    ArraysIndex, CavityShade, DEFAULT_CAVITY, fold_colour, fold_normal,
};

/// One layer's four maps, level by level, largest first, as the content step
/// wrote them: base colour, normal and ORM as RGBA8, height as 16-bit.
pub struct LayerMaps {
    /// Base colour, sRGB.
    pub base: Vec<Vec<u8>>,
    /// Normal, linear, green pointing down the image.
    pub normal: Vec<Vec<u8>>,
    /// Occlusion, roughness and metalness in red, green and blue.
    pub orm: Vec<Vec<u8>>,
    /// Height, little-endian 16-bit.
    pub height: Vec<Vec<u8>>,
}

/// Reads a layer's maps from `generated/materials/terrain/<set>/`.
///
/// # Errors
///
/// A file that is missing, is not a plain 2D KTX2 file, or is block-compressed
/// (the terrain layers are baked raw: this reads their bytes).
pub fn read_layer(root: &std::path::Path, set: &str) -> Result<LayerMaps> {
    let read = |map: &str, format: PlaneFormat| -> Result<Vec<Vec<u8>>> {
        let path = root.join(format!("materials/terrain/{set}/{map}.ktx2"));
        let bytes = std::fs::read(&path)
            .with_context(|| format!("{}: run `content` first", path.display()))?;
        let file =
            ktx2::read_levels(&bytes).with_context(|| format!("reading {}", path.display()))?;
        ensure!(
            file.info.plane_format() == Some(format),
            "{} is {:?}, not a raw {format:?} map (the terrain layers are baked raw)",
            path.display(),
            file.info.plane_format()
        );
        Ok(file.levels)
    };
    Ok(LayerMaps {
        base: read("base", PlaneFormat::Rgba8Srgb)?,
        normal: read("normal", PlaneFormat::Rgba8Unorm)?,
        orm: read("orm", PlaneFormat::Rgba8Unorm)?,
        height: read("height", PlaneFormat::R16Unorm)?,
    })
}

/// What the terrain step wrote.
pub struct Baked {
    /// The colour array's file.
    pub colour: Vec<u8>,
    /// The normal and height array's file.
    pub normal: Vec<u8>,
    /// The index.
    pub index: ArraysIndex,
}

/// Folds `layers` (named by `sets`, in layer order, `size` texels across) into
/// the two arrays, with the occlusion folded into the colour at the default
/// cavity.
///
/// # Errors
///
/// Layers that disagree about their levels, or an encoder that refuses a level.
pub fn bake(sets: &[String], layers: &[LayerMaps], size: u32) -> Result<Baked> {
    ensure!(sets.len() == layers.len(), "one set name a layer");
    let shade = CavityShade::new(DEFAULT_CAVITY);
    let mut colour = Vec::with_capacity(layers.len());
    let mut normal = Vec::with_capacity(layers.len());
    for (set, layer) in sets.iter().zip(layers) {
        let levels = layer.base.len();
        if [layer.normal.len(), layer.orm.len(), layer.height.len()] != [levels; 3] {
            bail!("terrain:{set} has maps with different numbers of levels");
        }
        colour.push(Encoded {
            format: PlaneFormat::Rgba8Srgb,
            mips: (0..levels)
                .map(|level| fold_colour(&layer.base[level], &layer.orm[level], &shade))
                .collect(),
        });
        normal.push(Encoded {
            format: PlaneFormat::Rgba8Unorm,
            mips: (0..levels)
                .map(|level| fold_normal(&layer.normal[level], &layer.height[level]))
                .collect(),
        });
    }
    let write = |layers: &[Encoded], format| {
        ktx2::write_block_array(layers, size, format, Supercompression::Zstd)
            .context("writing a terrain array")
    };
    Ok(Baked {
        colour: write(&colour, BlockFormat::Bc7Srgb)?,
        normal: write(&normal, BlockFormat::Bc7Unorm)?,
        index: ArraysIndex {
            layers: sets.to_vec(),
            size,
            cavity: DEFAULT_CAVITY,
        },
    })
}

/// The registry's terrain sets in layer order: the order the terrain loads
/// them in and its shader indexes them by.
#[must_use]
pub fn layer_sets(registries: &strata_registry::GameRegistries) -> Vec<String> {
    registries
        .materials
        .iter()
        .take(strata_registry::MAX_DRAWN_LAYERS)
        .map(|(_, _, def)| def.texture_set.clone())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A layer of `texels_across` squared texels with a full chain, flat.
    fn flat(texels_across: usize, base: [u8; 4], orm: [u8; 4]) -> LayerMaps {
        let mut layer = LayerMaps {
            base: Vec::new(),
            normal: Vec::new(),
            orm: Vec::new(),
            height: Vec::new(),
        };
        let mut edge = texels_across;
        loop {
            let texels = edge * edge;
            layer.base.push(base.repeat(texels));
            layer.normal.push([128, 128, 255, 255].repeat(texels));
            layer.orm.push(orm.repeat(texels));
            layer.height.push([0, 0x80].repeat(texels));
            if edge == 1 {
                return layer;
            }
            edge /= 2;
        }
    }

    fn u32_at(bytes: &[u8], at: usize) -> u32 {
        u32::from_le_bytes(bytes[at..at + 4].try_into().expect("four bytes"))
    }

    #[test]
    fn two_arrays_of_one_layer_each_per_set() {
        let sets = vec!["grass".to_owned(), "rock".to_owned()];
        let layers = [
            flat(16, [200, 100, 50, 255], [255, 90, 0, 255]),
            flat(16, [10, 20, 30, 255], [255, 200, 0, 255]),
        ];
        let baked = bake(&sets, &layers, 16).expect("bakes");
        for (file, format) in [
            (&baked.colour, BlockFormat::Bc7Srgb),
            (&baked.normal, BlockFormat::Bc7Unorm),
        ] {
            // vkFormat, then width, height, depth, layerCount, faceCount, levelCount.
            assert_eq!(u32_at(file, 12), format.vk_format());
            assert_eq!(u32_at(file, 20), 16);
            assert_eq!(u32_at(file, 32), 2, "two layers");
            assert_eq!(u32_at(file, 40), 5, "16 down to 1");
        }
        assert_eq!(baked.index.layers, sets);
        assert!(baked.index.serves(["grass", "rock"], DEFAULT_CAVITY));
    }

    #[test]
    fn layers_that_disagree_about_levels_are_refused() {
        let mut short = flat(8, [1, 2, 3, 255], [255, 0, 0, 255]);
        short.height.pop();
        let error = bake(&["a".to_owned()], &[short], 8).err().expect("refused");
        assert!(error.to_string().contains("different numbers of levels"));
    }
}
