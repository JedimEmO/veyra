//! The cloud deck's look as data (`assets/look/clouds.ron`, review step six,
//! finding F3).
//!
//! The dials were constants in `strata_atmosphere::clouds` and
//! `clouds.wgsl`. They are here so a fixture or a debug panel can tune the
//! deck without a rebuild, and so the GPU, the CPU's shadow mask and the
//! sun's veil read one set of numbers and cannot drift apart. The density
//! image is baked on the CPU at start-up from [`DensityLook`]; [`ShapeLook`]
//! turns that density into an opacity; [`LightLook`] is how the deck is lit.
//!
//! Look data: it is in no world version and no handshake.

use serde::{Deserialize, Serialize};

/// Where the file sits under the asset root.
pub const CLOUDS_FILE: &str = "look/clouds.ron";

/// What the density image is made of.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct DensityLook {
    /// The heaps: a period to the tile and a height each.
    pub heaps: Vec<(i64, f64)>,
    /// The heaps' share of the detail.
    pub heap_weight: f64,
    /// The fine fractal's share, which creases an edge.
    pub fine_weight: f64,
    /// The squared ridge's share.
    pub ridge_weight: f64,
    /// The fine fractal's periods to the tile.
    pub fine_periods: Vec<i64>,
    /// The broad domain warp, in tiles.
    pub warp: f64,
    /// The fraying warp as a share of `warp`.
    pub fray: f64,
    /// The fraying warp's periods to the tile.
    pub fray_periods: Vec<i64>,
    /// The coverage noise value at which the group gate starts to open.
    pub group_lo: f64,
    /// The coverage noise value at which it is fully open.
    pub group_hi: f64,
}

impl Default for DensityLook {
    fn default() -> Self {
        Self {
            heaps: vec![(5, 1.0), (11, 0.8)],
            heap_weight: 0.8,
            fine_weight: 0.14,
            ridge_weight: 0.06,
            fine_periods: vec![16, 32],
            warp: 0.12,
            fray: 0.1,
            fray_periods: vec![20, 40],
            group_lo: 0.36,
            group_hi: 0.64,
        }
    }
}

/// How density becomes opacity.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct ShapeLook {
    /// The share of the detail density a gap between groups keeps.
    pub gate_floor: f32,
    /// The optical depth at a group's fringe over that in its middle.
    pub thin_group: f32,
    /// How far inside its edge a cloud reaches full opacity, in tiles.
    pub edge_fade: f32,
    /// Optical depth at a cloud's edge.
    pub tau_edge: f32,
    /// Optical depth at a cloud's densest.
    pub tau_heart: f32,
    /// The share of the sun a fully grown cloud takes off the ground.
    pub shadow_strength: f32,
    /// How far the finer read that frays an edge moves the density. It is
    /// the shader's alone: the shadow mask and the sun's veil do not see it.
    /// At 0.08 it pitted the thin gaps into a speckle at the zenith.
    pub fray_amount: f32,
    /// That read's frequency over the image's.
    pub fray_frequency: f32,
    /// The share of the fray a group's middle keeps.
    pub fray_middle: f32,
}

impl Default for ShapeLook {
    fn default() -> Self {
        Self {
            gate_floor: 0.08,
            thin_group: 0.25,
            edge_fade: 0.05,
            tau_edge: 1.0,
            tau_heart: 10.0,
            shadow_strength: 0.85,
            fray_amount: 0.03,
            fray_frequency: 7.0,
            fray_middle: 0.5,
        }
    }
}

/// How the deck is lit.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct LightLook {
    /// The body's optical depth top to bottom, in `tau_heart` units.
    pub body: f32,
    /// The sky's light on the deck as a share of the sun's at a high sun.
    pub sky_share: [f32; 3],
    /// The albedo's tint on a face turned up.
    pub top_tint: [f32; 3],
    /// The albedo's tint on a base seen from below.
    pub base_tint: [f32; 3],
}

impl Default for LightLook {
    fn default() -> Self {
        Self {
            body: 1.5,
            sky_share: [0.15, 0.21, 0.34],
            top_tint: [1.04, 1.0, 0.93],
            base_tint: [0.94, 0.99, 1.07],
        }
    }
}

/// The deck's look as the file holds it.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct CloudLook {
    /// The density image's make-up.
    pub density: DensityLook,
    /// Density to opacity.
    pub shape: ShapeLook,
    /// The lighting.
    pub light: LightLook,
}

impl CloudLook {
    /// Parses and checks a clouds file.
    ///
    /// # Errors
    ///
    /// The parse error, or the first dial out of its range.
    pub fn parse(text: &str) -> Result<Self, String> {
        let look: Self = ron::from_str(text).map_err(|e| e.to_string())?;
        look.check()?;
        Ok(look)
    }

    fn check(&self) -> Result<(), String> {
        let d = &self.density;
        let weights = [d.heap_weight, d.fine_weight, d.ridge_weight];
        if weights.iter().any(|w| !w.is_finite() || *w < 0.0) || weights.iter().sum::<f64>() <= 0.0
        {
            return Err("density weights must be finite, not negative, and not all zero".into());
        }
        if d.heaps.is_empty()
            || d.heaps
                .iter()
                .any(|&(p, h)| p < 1 || !(0.0..=2.0).contains(&h))
        {
            return Err("density.heaps needs a period of 1 or more and a height in 0 to 2".into());
        }
        if d.fine_periods.is_empty() || d.fray_periods.is_empty() {
            // An empty list makes the fractal average 0/0, and a NaN warp poisons the whole image;
            // fine_weight 0 is the way to switch the fine fractal off.
            return Err("density.fine_periods and fray_periods need at least one period (use fine_weight 0 to turn the fine fractal off)".into());
        }
        if d.fine_periods.iter().chain(&d.fray_periods).any(|&p| p < 1) {
            return Err("density periods must be 1 or more".into());
        }
        if !(0.0..=0.5).contains(&d.warp) || !(0.0..=1.0).contains(&d.fray) {
            return Err("density.warp must be in 0 to 0.5 and density.fray in 0 to 1".into());
        }
        if !(0.0..d.group_hi).contains(&d.group_lo) || d.group_hi > 1.0 {
            return Err("density.group_lo must be under group_hi, which is at most 1".into());
        }
        let s = &self.shape;
        let unit = |v: f32| (0.0..=1.0).contains(&v);
        if !unit(s.gate_floor) || !unit(s.thin_group) || !unit(s.shadow_strength) {
            return Err("shape.gate_floor, thin_group and shadow_strength are in 0 to 1".into());
        }
        if !(0.001..=0.5).contains(&s.edge_fade) {
            return Err("shape.edge_fade must be in 0.001 to 0.5".into());
        }
        if !(0.0..=0.5).contains(&s.fray_amount)
            || !(0.5..=64.0).contains(&s.fray_frequency)
            || !unit(s.fray_middle)
        {
            return Err("shape.fray_amount is in 0 to 0.5, fray_frequency in 0.5 to 64, fray_middle in 0 to 1".into());
        }
        if !(0.0..=64.0).contains(&s.tau_edge) || !(0.0..=64.0).contains(&s.tau_heart) {
            return Err("shape.tau_edge and tau_heart must be in 0 to 64".into());
        }
        let l = &self.light;
        let colours = l.sky_share.iter().chain(&l.top_tint).chain(&l.base_tint);
        if !l.body.is_finite()
            || l.body < 0.0
            || colours.into_iter().any(|c| !c.is_finite() || *c < 0.0)
        {
            return Err("light dials must be finite and not negative".into());
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_shipped_file_parses_and_is_the_default() {
        let shipped = CloudLook::parse(include_str!("../../../assets/look/clouds.ron"))
            .expect("assets/look/clouds.ron is valid");
        assert_eq!(shipped, CloudLook::default());
    }

    #[test]
    fn a_dial_out_of_range_is_refused() {
        assert!(CloudLook::parse("(shape: (gate_floor: 1.5))").is_err());
        assert!(CloudLook::parse("(density: (heaps: []))").is_err());
        assert!(CloudLook::parse("(density: (typo: 1))").is_err());
        assert!(CloudLook::parse("(density: (fine_periods: []))").is_err());
        assert!(CloudLook::parse("(density: (fray_periods: []))").is_err());
        assert!(CloudLook::parse("(density: (fine_weight: 0.0))").is_ok());
        assert!(CloudLook::parse("()").is_ok());
    }
}
