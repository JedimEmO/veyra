//! The terrain's two baked texture arrays: what the content step writes and
//! the terrain loads, and the one place the two agree on a channel.
//!
//! `just content` folds the layer maps (base colour, normal, occlusion and
//! roughness, height) of every terrain material into two block-compressed
//! KTX2 arrays, one layer per material in the registry's order: colour with
//! roughness in alpha, and normal with height in alpha. The terrain used to
//! pack and mip them on the CPU at every start; baked, they are two files the
//! asset server hands to the GPU. The shade the cavity occlusion paints is
//! part of the colour, so the file records the strength it was baked at
//! ([`ArraysIndex::cavity`]) and a run that asks for another strength packs
//! from the layer maps again, with the same functions.

use serde::{Deserialize, Serialize};

/// Where the index sits in the files port (`generated/` is its root).
pub const INDEX_PATH: &str = "generated/terrain/arrays.ron";

/// The colour array as an asset path in the `generated://` source.
pub const COLOUR_ASSET: &str = "generated://terrain/colour.ktx2";

/// The normal and height array as an asset path in the `generated://` source.
pub const NORMAL_ASSET: &str = "generated://terrain/normal.ktx2";

/// The colour array's path under `generated/`.
pub const COLOUR_FILE: &str = "terrain/colour.ktx2";

/// The normal array's path under `generated/`.
pub const NORMAL_FILE: &str = "terrain/normal.ktx2";

/// The index's path under `generated/`.
pub const INDEX_FILE: &str = "terrain/arrays.ron";

/// The cavity strength the arrays are baked at: how much of the occlusion map
/// is multiplied into the colour. The terrain look's default; the look may ask
/// for another, which the baked arrays cannot give.
pub const DEFAULT_CAVITY: f32 = 0.8;

/// What the baked arrays say about themselves, so the terrain can tell
/// whether they are the arrays it would have packed.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ArraysIndex {
    /// The terrain set (`TerrainMaterialDef::texture_set`) of each layer, in
    /// layer order: a registry that lists other sets in another order is not
    /// the one these were baked for.
    pub layers: Vec<String>,
    /// Texels across a layer at level 0.
    pub size: u32,
    /// The cavity strength folded into the colour.
    pub cavity: f32,
}

impl ArraysIndex {
    /// The index as the file holds it.
    ///
    /// # Errors
    ///
    /// The RON error, as text, if `text` is not an index.
    pub fn parse(text: &str) -> Result<Self, String> {
        ron::from_str(text).map_err(|error| error.to_string())
    }

    /// The text of the file that holds this index.
    ///
    /// # Errors
    ///
    /// The RON error, as text; it cannot happen for this plain struct.
    pub fn to_text(&self) -> Result<String, String> {
        ron::ser::to_string_pretty(self, ron::ser::PrettyConfig::default())
            .map_err(|error| error.to_string())
    }

    /// Whether these arrays are what a start with `sets` (the registry's
    /// texture sets in layer order) and a look asking for `cavity` would pack.
    #[must_use]
    pub fn serves<'a>(&self, sets: impl IntoIterator<Item = &'a str>, cavity: f32) -> bool {
        (self.cavity - cavity).abs() < 1e-4 && self.layers.iter().map(String::as_str).eq(sets)
    }
}

/// The baked occlusion multiplied into an sRGB colour byte, as a table of
/// every occlusion byte by every colour byte: the darkening is in linear
/// light, and the table makes millions of texels three lookups each.
pub struct CavityShade {
    table: Vec<u8>,
}

impl CavityShade {
    /// The table for a `strength` from 0 (the colour as baked) to 1 (the
    /// colour times the whole occlusion).
    #[must_use]
    pub fn new(strength: f32) -> Self {
        let mut table = Vec::with_capacity(256 * 256);
        for ao in 0..=255_u8 {
            for byte in 0..=255_u8 {
                table.push(cavity_shade(byte, ao, strength));
            }
        }
        Self { table }
    }

    /// `byte` darkened by the occlusion `ao`.
    #[must_use]
    pub fn apply(&self, byte: u8, ao: u8) -> u8 {
        self.table[usize::from(ao) * 256 + usize::from(byte)]
    }
}

/// An sRGB colour byte darkened by a linear occlusion byte, by `strength`
/// of the way: `colour * (1 - strength * (1 - ao))` in linear light.
#[must_use]
pub fn cavity_shade(byte: u8, ao: u8, strength: f32) -> u8 {
    let linear = srgb_decode(f32::from(byte) / 255.0);
    let occlusion = f32::from(ao) / 255.0;
    let factor = 1.0 - strength.clamp(0.0, 1.0) * (1.0 - occlusion);
    // In 0..=255 after the clamp; the rounding is the conversion.
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let out = (srgb_encode(linear * factor).clamp(0.0, 1.0) * 255.0).round() as u8;
    out
}

/// The sRGB transfer function, inverted.
fn srgb_decode(c: f32) -> f32 {
    if c <= 0.040_45 {
        c / 12.92
    } else {
        ((c + 0.055) / 1.055).powf(2.4)
    }
}

/// The sRGB transfer function.
fn srgb_encode(c: f32) -> f32 {
    if c <= 0.003_130_8 {
        c * 12.92
    } else {
        1.055 * c.powf(1.0 / 2.4) - 0.055
    }
}

/// One level of the colour plane: the base colour (RGBA8, sRGB) darkened by
/// the occlusion in the ORM map's red through `shade`, with the ORM's
/// roughness (its green) in alpha.
///
/// The occlusion is folded into the colour rather than kept as a plane of its
/// own (ticket 0006, "To decide" 3): the stochastic read already takes two
/// array reads per lattice corner, three corners per layer, up to three layers
/// and two rock planes a fragment, and a third array would add a read to each
/// of them. Folded, it costs nothing per frame, and the cavities are dark
/// under every light, which is what a painted ground's shade is.
#[must_use]
pub fn fold_colour(base: &[u8], orm: &[u8], shade: &CavityShade) -> Vec<u8> {
    let mut colour = Vec::with_capacity(base.len());
    for (texel, orm) in base.as_chunks::<4>().0.iter().zip(orm.as_chunks::<4>().0) {
        let ao = orm[0];
        colour.extend_from_slice(&[
            shade.apply(texel[0], ao),
            shade.apply(texel[1], ao),
            shade.apply(texel[2], ao),
            orm[1],
        ]);
    }
    colour
}

/// One level of the normal plane: the normal (RGBA8) with its green turned to
/// point up the texture, since ashlar bakes image-space normals, and the
/// height in alpha for the shader's height blend. `height` is the 16-bit
/// height map, little-endian; its high byte is plenty for a blend weight.
#[must_use]
pub fn fold_normal(normal: &[u8], height: &[u8]) -> Vec<u8> {
    let mut packed = Vec::with_capacity(normal.len());
    for (texel, height) in normal
        .as_chunks::<4>()
        .0
        .iter()
        .zip(height.as_chunks::<2>().0)
    {
        packed.extend_from_slice(&[texel[0], 255 - texel[1], texel[2], height[1]]);
    }
    packed
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The cavity shade is the colour in linear light times the share of
    /// the occlusion asked for: none at no strength or an occlusion of
    /// one, and an sRGB byte darkened by half its light at strength one
    /// and an occlusion of a half.
    #[test]
    fn the_cavity_shade_darkens_in_linear_light() {
        for byte in [0_u8, 1, 17, 128, 200, 255] {
            assert_eq!(cavity_shade(byte, 255, 1.0), byte);
            assert_eq!(cavity_shade(byte, 0, 0.0), byte);
        }
        assert_eq!(cavity_shade(200, 0, 1.0), 0);
        // 188 is linear 0.5029; times 128 / 255 it is 0.2524, 138 in sRGB.
        assert_eq!(cavity_shade(188, 128, 1.0), 138);
        // Monotone in the occlusion.
        let mut last = 0;
        for ao in 0..=255_u8 {
            let out = cavity_shade(160, ao, 0.8);
            assert!(out >= last, "{ao}: {out} after {last}");
            last = out;
        }
        let shade = CavityShade::new(0.8);
        assert_eq!(shade.apply(160, 77), cavity_shade(160, 77, 0.8));
    }

    #[test]
    fn the_planes_take_their_channels_from_the_maps() {
        let shade = CavityShade::new(1.0);
        let colour = fold_colour(&[200, 100, 50, 255], &[255, 77, 0, 255], &shade);
        // No occlusion leaves the colour, and the roughness goes to alpha.
        assert_eq!(colour, vec![200, 100, 50, 77]);
        let dark = fold_colour(&[200, 100, 50, 255], &[0, 9, 0, 255], &shade);
        assert_eq!(dark, vec![0, 0, 0, 9]);
        // Green turns over; the height's high byte is the alpha.
        let normal = fold_normal(&[128, 10, 250, 255], &[0x34, 0x12]);
        assert_eq!(normal, vec![128, 245, 250, 0x12]);
    }

    #[test]
    fn an_index_serves_only_its_own_sets_and_cavity() {
        let index = ArraysIndex {
            layers: vec!["grass".into(), "rock".into()],
            size: 1024,
            cavity: DEFAULT_CAVITY,
        };
        assert!(index.serves(["grass", "rock"], 0.8));
        assert!(!index.serves(["rock", "grass"], 0.8));
        assert!(!index.serves(["grass"], 0.8));
        assert!(!index.serves(["grass", "rock"], 0.5));
        let text = index.to_text().expect("writes");
        assert_eq!(ArraysIndex::parse(&text), Ok(index));
    }
}
