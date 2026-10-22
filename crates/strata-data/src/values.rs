//! The value budget: how bright each family of surface may be, as data.
//!
//! The baked albedos were authored against different references and span
//! 0.01 to 0.96, so the scene's value structure followed the textures and
//! not the art direction (`docs/review/2026-10-16/visual.md`, F1). A painted
//! world keeps albedo in a band and lets the light and the grade make the
//! contrast. `assets/look/values.ron` names the families and their bands,
//! and says which family every shipped material belongs to.
//!
//! Three readers share it. The terrain draws each layer remapped to its
//! target at runtime (`strata_terrain`), and derives each biome's far colour
//! from its surface layer's target ([`ValueBudget::terrain_target`]), so the
//! plain at a kilometre is the ground it blends from. A test in the content
//! step reads every baked base map and holds the family bands.
//!
//! Look data: it is in no world version and no handshake.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

/// Where the file sits under the asset root.
pub const VALUES_FILE: &str = "look/values.ron";

/// Rec. 709 luminance weights, linear light.
pub const LUMA: [f32; 3] = [0.2126, 0.7152, 0.0722];

/// The linear luminance of a colour.
#[must_use]
pub fn luminance(rgb: [f32; 3]) -> f32 {
    rgb[0] * LUMA[0] + rgb[1] * LUMA[1] + rgb[2] * LUMA[2]
}

/// One family's band, all in linear luminance.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ValueFamily {
    /// What a member is drawn at where it has no target of its own.
    pub target: f32,
    /// The lowest mean the family may have.
    pub min: f32,
    /// The highest mean the family may have.
    pub max: f32,
    /// The most the standard deviation of a member's luminance may be, so a
    /// remap that lifts a dark texture does not stretch its detail past the
    /// band with it.
    pub spread: f32,
}

/// One shipped material's place in the budget.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Member {
    /// The family's name, a key of [`ValueBudget::families`].
    pub family: String,
    /// The mean this member is drawn at, where it is not its family's.
    #[serde(default)]
    pub target: Option<f32>,
    /// Why the member is out of band today and what lands it, for a material
    /// whose fix is a later step. The test holds a pending member to be out
    /// of band, so the list shrinks as the steps land.
    #[serde(default)]
    pub pending: Option<String>,
}

/// How a body wears its household or faction colour (`character:` in the
/// file). The ranger's cloth panels are in the character band, so the colour
/// multiplies a lit panel; these are the small additions on top of that.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct CharacterLook {
    /// The share of an own colour (at its peak) added to the cloth beside the
    /// multiply, so a very dark band colour still reads as its hue.
    pub own_lift: f32,
    /// The share of that lift a body gives off as light: a lamp's worth, not
    /// the lit body the first pass stood in for.
    pub own_glow: f32,
    /// The most metal an own-colour body keeps, so the colour shows.
    pub own_metallic: f32,
    /// The colour of the rim a hostile's body carries (sRGB), so a target
    /// reads against grass at forty metres (visual.md F6).
    pub rim_colour: [f32; 3],
    /// How much of that colour a grazing pixel adds, in linear light; 0
    /// leaves the body as authored.
    pub rim_strength: f32,
    /// The fresnel exponent: higher draws a thinner rim.
    pub rim_power: f32,
}

impl Default for CharacterLook {
    fn default() -> Self {
        Self {
            own_lift: 0.1,
            own_glow: 0.03,
            own_metallic: 0.3,
            rim_colour: [1.0, 0.45, 0.3],
            rim_strength: 0.6,
            rim_power: 2.5,
        }
    }
}

/// The budget as the file holds it.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ValueBudget {
    /// The families and their bands.
    #[serde(default)]
    pub families: BTreeMap<String, ValueFamily>,
    /// Every shipped material by its baked directory under
    /// `generated/materials/` (`terrain/grass`, `flora/leaves-oak`,
    /// `serein/paint-white`), the impostor atlases as `impostors/<name>` and
    /// the character as `characters/<name>`. A `terrain/` member is remapped
    /// to its target at runtime, so its band is held on the target and the
    /// gain it takes; every other member's on its baked mean.
    #[serde(default)]
    pub materials: BTreeMap<String, Member>,
    /// How a body wears a colour.
    #[serde(default)]
    pub character: CharacterLook,
}

impl ValueBudget {
    /// Parses and checks a values file.
    ///
    /// # Errors
    ///
    /// The parse error, a family with an empty or non-finite band or a
    /// target outside it, or a member naming a family that is not there.
    pub fn parse(text: &str) -> Result<Self, String> {
        let budget: Self = ron::from_str(text).map_err(|e| e.to_string())?;
        budget.check()?;
        Ok(budget)
    }

    fn check(&self) -> Result<(), String> {
        for (name, f) in &self.families {
            let ok = [f.target, f.min, f.max, f.spread]
                .iter()
                .all(|v| v.is_finite() && *v > 0.0)
                && f.min <= f.target
                && f.target <= f.max;
            if !ok {
                return Err(format!(
                    "family {name}: min <= target <= max, all positive and finite"
                ));
            }
        }
        for (path, member) in &self.materials {
            let Some(family) = self.families.get(&member.family) else {
                return Err(format!("{path} names the family {}", member.family));
            };
            if let Some(t) = member.target
                && !(family.min..=family.max).contains(&t)
            {
                return Err(format!(
                    "{path}: target {t} is outside {} ({} to {})",
                    member.family, family.min, family.max
                ));
            }
        }
        Ok(())
    }

    /// The linear luminance a baked material is drawn at, or `None` for one
    /// the budget does not name.
    #[must_use]
    pub fn target_of(&self, path: &str) -> Option<f32> {
        let member = self.materials.get(path)?;
        member
            .target
            .or_else(|| self.families.get(&member.family).map(|f| f.target))
    }

    /// The linear luminance terrain layer `texture_set` is drawn at (the
    /// terrain shader remaps the layer's mean to it), or `None` for a layer
    /// the budget leaves as baked.
    #[must_use]
    pub fn terrain_target(&self, texture_set: &str) -> Option<f32> {
        self.target_of(&format!("terrain/{texture_set}"))
    }
}

/// The sRGB transfer function, inverted.
#[must_use]
pub fn srgb_to_linear(c: f32) -> f32 {
    let c = c.clamp(0.0, 1.0);
    if c <= 0.040_45 {
        c / 12.92
    } else {
        ((c + 0.055) / 1.055).powf(2.4)
    }
}

/// The sRGB transfer function.
#[must_use]
pub fn linear_to_srgb(c: f32) -> f32 {
    let c = c.clamp(0.0, 1.0);
    if c <= 0.003_130_8 {
        c * 12.92
    } else {
        1.055 * c.powf(1.0 / 2.4) - 0.055
    }
}

/// The far colour of a ground whose near layer is drawn at `target` (linear
/// luminance): the hue of `ground` at that luminance, so the plain past the
/// drawn ground is as bright as the ground it blends from. Both `ground` and
/// the result are sRGB, because a biome's tint is stored and read as sRGB
/// (`strata_terrain` linearises it); the scaling is done in linear. A black
/// `ground` has no hue and gives a grey.
#[must_use]
pub fn far_colour(ground: [f32; 3], target: f32) -> [f32; 3] {
    let lin = ground.map(srgb_to_linear);
    let l = luminance(lin);
    if l <= 1e-4 {
        return [linear_to_srgb(target); 3];
    }
    lin.map(|c| linear_to_srgb(c / l * target))
}

#[cfg(test)]
mod tests {
    use super::*;

    const SHIPPED: &str = include_str!("../../../assets/look/values.ron");

    #[test]
    fn the_shipped_file_parses() {
        let budget = ValueBudget::parse(SHIPPED).expect("assets/look/values.ron is valid");
        assert!(budget.terrain_target("grass").is_some());
    }

    #[test]
    fn a_member_needs_a_family_and_a_target_in_its_band() {
        let bad_family = "(families: {}, materials: {\"terrain/x\": (family: \"ground\")})";
        assert!(ValueBudget::parse(bad_family).is_err());
        let family = "\"ground\": (target: 0.2, min: 0.1, max: 0.3, spread: 0.1)";
        let outside = format!(
            "(families: {{{family}}}, materials: {{\"terrain/x\": (family: \"ground\", target: Some(0.9))}})"
        );
        assert!(ValueBudget::parse(&outside).is_err());
        let fine =
            format!("(families: {{{family}}}, materials: {{\"terrain/x\": (family: \"ground\")}})");
        let budget = ValueBudget::parse(&fine).expect("valid");
        assert_eq!(budget.terrain_target("x"), Some(0.2));
        assert_eq!(budget.terrain_target("y"), None);
    }

    #[test]
    fn the_far_colour_keeps_the_hue_and_takes_the_luminance() {
        let ground = [0.56, 0.66, 0.33];
        let far = far_colour(ground, 0.19);
        let (lin, gl) = (far.map(srgb_to_linear), ground.map(srgb_to_linear));
        assert!((luminance(lin) - 0.19).abs() < 1e-4);
        assert!((lin[0] / lin[1] - gl[0] / gl[1]).abs() < 1e-3);
        let grey = far_colour([0.0; 3], 0.2);
        assert!(grey.iter().all(|c| (srgb_to_linear(*c) - 0.2).abs() < 1e-4));
    }
}
