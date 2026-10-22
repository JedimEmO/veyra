//! The one place a colour picked by eye becomes a colour a graph can take.
//!
//! Every colour in the building kits is written as sRGB, the way it is seen
//! on a swatch, and converted here once. Graphs work in linear light, so a
//! raw sRGB triple handed to a graph's `color` parameter reads far too bright
//! (a charcoal of 0.25 comes out as 0.05 of the light it should). Before this
//! module `veyra-materials` converted and `veyra-settlements` did not, which
//! is the mismatch ticket 0006 found; both crates now go through [`param`].
//!
//! A material definition's own `base_color` is sRGB already (the renderer
//! converts it), so constants are not passed through here.
use ashlar::ParamValue;

/// One sRGB channel in linear light.
#[must_use]
pub fn to_linear(c: f32) -> f32 {
    if c <= 0.04045 {
        c / 12.92
    } else {
        ((c + 0.055) / 1.055).powf(2.4)
    }
}

/// One linear channel as sRGB, the inverse of [`to_linear`].
#[must_use]
pub fn to_srgb(c: f32) -> f32 {
    if c <= 0.003_130_8 {
        c * 12.92
    } else {
        1.055 * c.powf(1.0 / 2.4) - 0.055
    }
}

/// A graph colour parameter from an sRGB colour.
#[must_use]
pub fn param(srgb: [f32; 3]) -> ParamValue {
    ParamValue::Color(srgb.map(to_linear))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn colours_are_linearised() {
        let ParamValue::Color([r, _, _]) = param([0.5, 0.0, 1.0]) else {
            unreachable!();
        };
        assert!((r - 0.214).abs() < 1e-3, "{r}");
    }

    #[test]
    fn the_two_transfers_are_inverses() {
        for c in [0.0, 0.002, 0.04, 0.2, 0.5, 0.9, 1.0] {
            assert!((to_srgb(to_linear(c)) - c).abs() < 1e-4, "{c}");
        }
    }
}
