//! Terrain materials: a texture set and how it tiles.
//!
//! A material is a name the terrain shader turns into a layer of a texture
//! array. The layer index is the material's frozen id: materials are
//! inserted in sorted id order at load time, so the index is stable across
//! runs for the same asset set and a save file or a mesh built with it stays
//! valid until the assets change.

use serde::{Deserialize, Serialize};
use slotted_model::Namespaced;

/// A dense handle to a frozen terrain material. Also the texture array layer
/// the shader samples, see [`layer`](Self::layer).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct TerrainMaterialId(pub u32);

slotted_registry::impl_interned_id!(TerrainMaterialId);

/// The most materials one frozen set may hold: the layer index travels as a
/// `u8` in every terrain sample and every vertex.
pub const MAX_TERRAIN_MATERIALS: usize = 256;

/// The most layers the terrain draws: its texture arrays and vertex weights
/// hold sixteen, and its mesher packs a province's pair of rock faces into
/// one byte, four bits each. The freeze refuses a province face past it,
/// which would draw as another rock with no error.
pub const MAX_DRAWN_LAYERS: usize = 16;

impl TerrainMaterialId {
    /// The texture array layer this material occupies.
    ///
    /// Ids are dense and the freeze refuses more than
    /// [`MAX_TERRAIN_MATERIALS`], so the narrowing cannot lose bits.
    #[must_use]
    pub fn layer(self) -> u8 {
        u8::try_from(self.0).expect("the freeze bounds material ids to a byte")
    }
}

/// One terrain material, as `terrain/materials.ron` lists it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TerrainMaterialDef {
    /// The namespaced id, `strata:grass`.
    pub id: Namespaced,
    /// The terrain set `just content` bakes for this material, from
    /// `veyra-materials`: its maps land in
    /// `generated/materials/terrain/<texture_set>/`. Named rather than pathed
    /// so a resource pack can swap the set without touching the definition.
    pub texture_set: String,
    /// How many metres one repeat of the texture covers on the ground. The
    /// set's graph is drawn for one repeat, so this must be the same number
    /// the set's definition declares; a test in the terrain crate holds them
    /// together.
    pub tiling_m: f32,
    /// Whether the set is a laid pattern (setts, flagstones) whose rows and
    /// joints must stay aligned. The terrain reads every other set through
    /// randomly offset, quarter-turned cells to hide its repeat, which would
    /// turn a pattern's rows every few metres; a patterned set is read
    /// plainly, its repeat hidden by the pattern itself.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub patterned: bool,
    /// Whether the set has a grain that must keep its direction: the beds of
    /// a sandstone, the columns of a basalt, the cleavage of a slate. Such a
    /// set is still read through the stochastic cells, but each cell shifts
    /// it along `u` alone and never turns it, so a bed drawn level stays
    /// level on a cliff (the terrain projects a face with `v` running down
    /// it) and meets the same bed in the next cell. `patterned` wins where
    /// both are set.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub oriented: bool,
    /// Whether the set is living cover that the ground's palette paints
    /// (ticket 0006): the colour shapes (lush and dry, warm and cool) and
    /// the near pull to the biome's ground colour. Grass and a forest's
    /// floor; not the laid ways, the water's beds, sand, snow or rock,
    /// which keep their own colour wherever they lie.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub palette: bool,
    /// How much of the baked detail's contrast about the layer's mean the
    /// ground keeps, above 0 (review step six, F8): sand's ripples and
    /// gravel's pebbles are the loudest marks at eye height and want less
    /// than the global `grain` of the look. Absent reads the look's. A laid
    /// (`patterned`) set always keeps its full contrast. Read by the shader
    /// at run time, not baked into the arrays.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub grain: Option<f32>,
    /// The layout of a patterned set's stones, when the shader may tell one
    /// stone from the next (review step six, F7): the plaza's flags each take
    /// a value of their own on top of the baked variation.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub laid: Option<Laid>,
}

/// How a laid set's courses are drawn, mirrored from the graph that bakes it
/// (`veyra_materials::terrain::city`): the shader repeats the layout to know
/// which stone a point is on, and a test holds the two together.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Laid {
    /// Courses down one repeat.
    pub rows: u16,
    /// Stones to a course, on average.
    pub stones: u16,
    /// How far a joint moves along a course, as a share of a stone either way.
    pub joint: f32,
    /// How much a stone's value varies from the baked one, a share either
    /// way (0 to 0.5): the dial that breaks the grid. Zero leaves the set as
    /// baked.
    pub tone: f32,
}

impl TerrainMaterialDef {
    /// Whether the numbers and names make sense.
    #[must_use]
    pub fn is_valid(&self) -> bool {
        if self.grain.is_some_and(|g| !(g > 0.0 && g <= 2.0)) {
            return false;
        }
        if let Some(laid) = &self.laid
            && (laid.rows == 0
                || laid.stones == 0
                || !(0.0..=0.5).contains(&laid.joint)
                || !(0.0..=0.5).contains(&laid.tone))
        {
            return false;
        }
        !self.texture_set.is_empty()
            && !self.texture_set.contains(['/', '\\', '.'])
            && self.tiling_m.is_finite()
            && self.tiling_m > 0.0
    }

    /// The baked base colour, as an asset path in the `generated://` source.
    #[must_use]
    pub fn base_path(&self) -> String {
        self.baked("base")
    }

    /// The baked normal map, in image coordinates (green points down the
    /// texture, as ashlar bakes it).
    #[must_use]
    pub fn normal_path(&self) -> String {
        self.baked("normal")
    }

    /// The baked occlusion, roughness and metalness, in red, green and blue.
    #[must_use]
    pub fn orm_path(&self) -> String {
        self.baked("orm")
    }

    /// The baked height, white on top, which the terrain blends layers by.
    #[must_use]
    pub fn height_path(&self) -> String {
        self.baked("height")
    }

    fn baked(&self, map: &str) -> String {
        format!(
            "generated://materials/terrain/{}/{map}.ktx2",
            self.texture_set
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    #[test]
    fn paths_follow_the_texture_set_layout() {
        let def = TerrainMaterialDef {
            id: Namespaced::parse("strata:grass").expect("valid"),
            texture_set: "grass".into(),
            tiling_m: 4.0,
            patterned: false,
            oriented: false,
            palette: false,
            grain: None,
            laid: None,
        };
        assert!(def.is_valid());
        assert_eq!(
            def.base_path(),
            "generated://materials/terrain/grass/base.ktx2"
        );
        assert_eq!(
            def.normal_path(),
            "generated://materials/terrain/grass/normal.ktx2"
        );
        assert_eq!(
            def.orm_path(),
            "generated://materials/terrain/grass/orm.ktx2"
        );
        assert_eq!(
            def.height_path(),
            "generated://materials/terrain/grass/height.ktx2"
        );
    }

    #[test]
    fn a_texture_set_is_a_name_not_a_path() {
        let mut def = TerrainMaterialDef {
            id: Namespaced::parse("strata:grass").expect("valid"),
            texture_set: "../grass".into(),
            tiling_m: 4.0,
            patterned: false,
            oriented: false,
            palette: false,
            grain: None,
            laid: None,
        };
        assert!(!def.is_valid());
        def.texture_set = "grass".into();
        def.tiling_m = 0.0;
        assert!(!def.is_valid());
    }

    #[test]
    fn a_grain_is_above_nought() {
        let mut def = TerrainMaterialDef {
            id: Namespaced::parse("strata:sand").expect("valid"),
            texture_set: "sand".into(),
            tiling_m: 12.0,
            patterned: false,
            oriented: false,
            palette: false,
            grain: Some(0.5),
            laid: None,
        };
        assert!(def.is_valid());
        def.grain = Some(0.0);
        assert!(
            !def.is_valid(),
            "nought is the uniform's mark for the look's"
        );
    }

    #[test]
    fn the_layer_is_the_id() {
        assert_eq!(TerrainMaterialId(7).layer(), 7);
    }
}
