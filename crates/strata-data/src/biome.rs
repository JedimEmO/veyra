//! The biome definition a `.biome.ron` file holds.
//!
//! A biome is a climate window, a relief recipe and a surface. The climate
//! window is what the worldgen classifier scores a point against; the relief
//! recipe is what it turns into metres once the biome wins; the surface is
//! what the terrain material paints. Everything is data so tuning a world is
//! editing RON, and later a mod can add a biome without a rebuild.
//!
//! Every type here follows slotted's `Value`-safe rule: patch lists rewrite
//! the raw RON before it becomes a typed def, and RON's `Value` forgets enum
//! variant names, so [`ReliefKind`] is a lowercase string, not a variant.

use core::fmt;
use core::str::FromStr;

use serde::{Deserialize, Deserializer, Serialize, Serializer};
use slotted_model::Namespaced;

/// A dense handle to a frozen biome. Assigned by the freeze in definition
/// order; the sampler passes these around instead of names.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct BiomeId(pub u32);

slotted_registry::impl_interned_id!(BiomeId);

/// A closed interval inside `0..=1`, the unit the climate fields are
/// normalised to.
///
/// Both ends are inclusive so a biome can claim the exact edge of the field
/// (`elevation: (min: 0.0, max: 0.0)` is a legal, if pointless, window).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UnitRange {
    /// Lower bound, inclusive.
    pub min: f32,
    /// Upper bound, inclusive.
    pub max: f32,
}

impl UnitRange {
    /// The whole unit interval, for a biome that does not care about a field.
    pub const FULL: Self = Self { min: 0.0, max: 1.0 };

    /// A range from `min` to `max`.
    #[must_use]
    pub const fn new(min: f32, max: f32) -> Self {
        Self { min, max }
    }

    /// Whether the range is ordered and inside the unit interval.
    #[must_use]
    pub fn is_valid(self) -> bool {
        self.min.is_finite()
            && self.max.is_finite()
            && (0.0..=1.0).contains(&self.min)
            && (0.0..=1.0).contains(&self.max)
            && self.min <= self.max
    }

    /// The midpoint, where the biome fits best.
    #[must_use]
    pub fn center(self) -> f32 {
        f32::midpoint(self.min, self.max)
    }

    /// Half the width, the distance from the centre to either edge.
    #[must_use]
    pub fn half_width(self) -> f32 {
        (self.max - self.min) * 0.5
    }

    /// How far `value` is outside the range, in the same unit; zero inside.
    #[must_use]
    pub fn distance(self, value: f32) -> f32 {
        if value < self.min {
            self.min - value
        } else if value > self.max {
            value - self.max
        } else {
            0.0
        }
    }
}

/// The climate window a biome claims.
///
/// Temperature and moisture are the climate stage's normalised fields;
/// elevation is the continent stage's base height normalised so that `0.5`
/// is sea level. Ocean claims the low elevations and mountains the high, and
/// the classifier only has to compare numbers.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ClimateWindow {
    /// Cold (`0`) to hot (`1`).
    pub temperature: UnitRange,
    /// Arid (`0`) to wet (`1`).
    pub moisture: UnitRange,
    /// Deep sea (`0`) through sea level (`0.5`) to the highest plateau (`1`).
    pub elevation: UnitRange,
}

impl ClimateWindow {
    /// Whether every range is valid.
    #[must_use]
    pub fn is_valid(self) -> bool {
        self.temperature.is_valid() && self.moisture.is_valid() && self.elevation.is_valid()
    }
}

/// Which noise shape the relief stage uses for a biome, and the class a
/// geological province's rules name: a province may give the class another
/// shape, amplitude and wavelength (`ShapeRule`, ticket 0004's step 4).
///
/// Serialised as a lowercase string rather than an enum variant so it
/// survives the `Value` round trip patch lists need.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub enum ReliefKind {
    /// Gentle low-octave fbm: a few metres of roll.
    #[default]
    Plains,
    /// Rolling fbm with more octaves and a broader amplitude.
    Hills,
    /// Ridged multifractal: sharp crests, deep valleys.
    Mountains,
    /// Billow: rounded, cushion-like dunes.
    Dunes,
    /// Ridged multifractal at a low amplitude: long parallel ribs, used for
    /// tundra and the like.
    Ridges,
}

impl ReliefKind {
    /// Every kind, for iteration and error messages.
    pub const ALL: [Self; 5] = [
        Self::Plains,
        Self::Hills,
        Self::Mountains,
        Self::Dunes,
        Self::Ridges,
    ];

    /// The lowercase name used in RON.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Plains => "plains",
            Self::Hills => "hills",
            Self::Mountains => "mountains",
            Self::Dunes => "dunes",
            Self::Ridges => "ridges",
        }
    }
}

impl fmt::Display for ReliefKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// A relief kind name that is not one of [`ReliefKind::ALL`].
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("unknown relief kind `{0}`; expected one of plains, hills, mountains, dunes, ridges")]
pub struct UnknownReliefKind(pub String);

impl FromStr for ReliefKind {
    type Err = UnknownReliefKind;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::ALL
            .into_iter()
            .find(|kind| kind.as_str() == s)
            .ok_or_else(|| UnknownReliefKind(s.to_owned()))
    }
}

impl Serialize for ReliefKind {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(self.as_str())
    }
}

impl<'de> Deserialize<'de> for ReliefKind {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let s = String::deserialize(deserializer)?;
        s.parse().map_err(serde::de::Error::custom)
    }
}

/// How a biome shapes the ground once it wins a point.
///
/// `wavelength_m` rather than a frequency because a designer thinks in "a
/// ridge every two kilometres", not in cycles per metre; the sampler inverts
/// it once at construction.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Relief {
    /// The noise shape.
    pub kind: ReliefKind,
    /// Peak-to-baseline height of the relief, in metres.
    pub amplitude_m: f32,
    /// Size of the largest feature, in metres.
    pub wavelength_m: f32,
    /// How many octaves of detail. More costs sampling time.
    pub octaves: u32,
    /// Amplitude falloff per octave. `0.5` is the textbook value.
    #[serde(default = "default_persistence")]
    pub persistence: f32,
    /// Frequency growth per octave. `2.0` doubles each time.
    #[serde(default = "default_lacunarity")]
    pub lacunarity: f32,
    /// Metres added to the base elevation regardless of noise, so a plateau
    /// biome can sit above its neighbours.
    #[serde(default)]
    pub lift_m: f32,
    /// How deeply water has carved this relief into gullies and ridges,
    /// from `0` (untouched, the default) to about `2`. Only the strength is
    /// the biome's: the gullies' spacing is the world's, so they carry on
    /// across a border instead of jumping (`strata-worldgen`'s erosion).
    #[serde(default)]
    pub erosion: f32,
}

const fn default_persistence() -> f32 {
    0.5
}

const fn default_lacunarity() -> f32 {
    2.0
}

impl Relief {
    /// Whether the numbers make sense for a noise stack.
    #[must_use]
    pub fn is_valid(self) -> bool {
        self.amplitude_m.is_finite()
            && self.amplitude_m >= 0.0
            && self.wavelength_m.is_finite()
            && self.wavelength_m > 0.0
            && (1..=16).contains(&self.octaves)
            && self.persistence.is_finite()
            && self.persistence > 0.0
            && self.persistence <= 1.0
            && self.lacunarity.is_finite()
            && self.lacunarity >= 1.0
            && self.lift_m.is_finite()
            && self.erosion.is_finite()
            && (0.0..=4.0).contains(&self.erosion)
    }
}

/// What the ground is made of.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Surface {
    /// The terrain material on flat ground.
    pub material: Namespaced,
    /// The terrain material once the slope passes `slope_threshold`.
    pub slope_material: Namespaced,
    /// Rise over run at which `slope_material` takes over. `1.0` is 45
    /// degrees.
    #[serde(default = "default_slope_threshold")]
    pub slope_threshold: f32,
}

const fn default_slope_threshold() -> f32 {
    0.7
}

/// The colour the map tool and the vertex-colour terrain of Phase 2 paint
/// a biome in. Linear RGB, each `0..=1`.
///
/// Transparent over the array rather than a newtype in serde's eyes: RON's
/// `Value` has no newtype, and a newtype visitor fed a sequence would take
/// the first channel as the whole colour.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Tint(pub [f32; 3]);

impl Tint {
    /// The tint as 8-bit sRGB-ish bytes for a PNG. No gamma curve is applied:
    /// the tints are authored by eye against the map, so what is written is
    /// what was seen.
    #[must_use]
    pub fn to_rgb8(self) -> [u8; 3] {
        // Truncation is the point of the conversion; the value is clamped
        // first so the cast is in range.
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        let byte = |c: f32| (c.clamp(0.0, 1.0) * 255.0).round() as u8;
        [byte(self.0[0]), byte(self.0[1]), byte(self.0[2])]
    }
}

/// A colour grade: how the frame is tinted while the observer stands in a
/// biome, or at a time of day (`assets/look/times.ron`). The render crate
/// sums the observer's biome grade and the time's as offsets from the
/// neutral grade, scales the sum by the look's strength and writes it on
/// the camera's Bevy `ColorGrading`, so every field is in Bevy's unit:
///
/// - `temperature` and `tint` shift the white point's CIE 1931 *x* and *y*
///   (Bevy's `ColorGradingGlobal`); positive warms and turns magenta,
///   negative cools and turns green. Bevy's range is nominally -1 to 1, but
///   the shift is of a chromaticity of about 0.31, so it is strong: 0.01 of
///   temperature takes about 9 % off red and adds 3 % to blue, and 0.08
///   takes red to a third. A grade stays within `GRADE_WHITE_SHIFT`.
/// - `hue` rotates every colour, in radians. Zero everywhere costs nothing:
///   Bevy compiles the rotation out while no grade uses it.
/// - `saturation` and `contrast` are factors, 1 neutral: the saturation
///   multiplies the look's after the tonemapper and the contrast the look's
///   on the shadows and midtones, so the look's own grading stays a global
///   multiplier. Bevy's contrast pivots at exposed linear 0.5, above most
///   of a frame, so a contrast over 1 darkens and saturates dark paint
///   (1.04 took a red awning at dusk from 42 to 33 of 255 in red); the
///   shipped grades leave it at 1 and make a place clear with `gain`.
/// - `lift` is added to the shadows before the tonemapper, in exposed
///   linear units (0 neutral; a few thousandths opens the darks), and
///   `gain` multiplies the highlights (1 neutral). Bevy's lift and gain are
///   grey: a section cannot be tinted, so a cool shade under a warm sun is
///   the sky's light, not the grade's.
///
/// Every field defaults to neutral, so a file names only what it changes.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Grade {
    /// White point shift on CIE *x*; positive is warmer.
    pub temperature: f32,
    /// White point shift on CIE *y*; positive is more magenta, negative
    /// greener.
    pub tint: f32,
    /// Hue rotation, radians.
    pub hue: f32,
    /// Saturation factor after the tonemapper.
    pub saturation: f32,
    /// Contrast factor on the shadows and midtones.
    pub contrast: f32,
    /// Added to the shadows, exposed linear units.
    pub lift: f32,
    /// Factor on the highlights.
    pub gain: f32,
    /// The colour of the shade, a linear multiplier per channel (1 neutral)
    /// on the light the sky and the probes give indirect diffuse, which is
    /// all the light a shaded surface has (visual.md F4). Bevy's lift is
    /// grey, so this is the shade's only colour control. The renderer
    /// divides the blend by its own luminance, so a tint moves the hue of
    /// the shade and never its brightness, and exposure stays put.
    ///
    /// Read but never written, like [`BiomeDef::ground`]: it is look data,
    /// and the world's version hashes every biome as it serialises.
    #[serde(skip_serializing)]
    pub shade: [f32; 3],
}

/// The largest white point shift a [`Grade`] may carry, on either axis: at
/// 0.05 red is already about 0.6 or 1.4 of itself, a different picture
/// rather than a grade.
pub const GRADE_WHITE_SHIFT: f32 = 0.05;

impl Default for Grade {
    fn default() -> Self {
        Self::NEUTRAL
    }
}

impl Grade {
    /// The grade that changes nothing.
    pub const NEUTRAL: Self = Self {
        temperature: 0.0,
        tint: 0.0,
        hue: 0.0,
        saturation: 1.0,
        contrast: 1.0,
        lift: 0.0,
        gain: 1.0,
        shade: [1.0; 3],
    };

    /// Whether this is [`Grade::NEUTRAL`] in everything the camera's grading
    /// reads; the shade is ignored because it is not part of that grading.
    #[must_use]
    pub fn is_neutral(&self) -> bool {
        Self {
            shade: [1.0; 3],
            ..*self
        } == Self::NEUTRAL
    }

    /// The first field out of range and the range it must be in, or `None`
    /// when the grade is usable. The ranges are wide for a grade and
    /// narrow for a picture: past them the frame is a different image.
    #[must_use]
    pub fn invalid_field(&self) -> Option<(&'static str, &'static str)> {
        let w = GRADE_WHITE_SHIFT;
        [
            ("temperature", self.temperature, -w, w, "-0.05..=0.05"),
            ("tint", self.tint, -w, w, "-0.05..=0.05"),
            (
                "hue",
                self.hue,
                -core::f32::consts::PI,
                core::f32::consts::PI,
                "-pi..=pi",
            ),
            ("saturation", self.saturation, 0.0, 2.0, "0..=2"),
            ("contrast", self.contrast, 0.5, 2.0, "0.5..=2"),
            ("lift", self.lift, -0.1, 0.1, "-0.1..=0.1"),
            ("gain", self.gain, 0.5, 2.0, "0.5..=2"),
        ]
        .into_iter()
        .chain([
            ("shade.r", self.shade[0], 0.5, 1.5, "0.5..=1.5"),
            ("shade.g", self.shade[1], 0.5, 1.5, "0.5..=1.5"),
            ("shade.b", self.shade[2], 0.5, 1.5, "0.5..=1.5"),
        ])
        .find(|(_, v, min, max, _)| !v.is_finite() || !(*min..=*max).contains(v))
        .map(|(name, _, _, _, range)| (name, range))
    }

    /// This grade's offset from neutral, scaled: what it adds to another.
    #[must_use]
    pub fn offset(&self, scale: f32) -> Self {
        Self {
            temperature: self.temperature * scale,
            tint: self.tint * scale,
            hue: self.hue * scale,
            saturation: (self.saturation - 1.0) * scale,
            contrast: (self.contrast - 1.0) * scale,
            lift: self.lift * scale,
            gain: (self.gain - 1.0) * scale,
            shade: self.shade.map(|c| (c - 1.0) * scale),
        }
    }

    /// Neutral plus the sum of `parts`' offsets, each scaled by its weight:
    /// the weighted mean of grades when the weights sum to one, their sum
    /// when each weighs one.
    #[must_use]
    pub fn sum<'a>(parts: impl IntoIterator<Item = (&'a Self, f32)>) -> Self {
        let mut total = Self {
            saturation: 0.0,
            contrast: 0.0,
            gain: 0.0,
            shade: [0.0; 3],
            ..Self::NEUTRAL
        };
        for (grade, weight) in parts {
            let o = grade.offset(weight);
            total.temperature += o.temperature;
            total.tint += o.tint;
            total.hue += o.hue;
            total.saturation += o.saturation;
            total.contrast += o.contrast;
            total.lift += o.lift;
            total.gain += o.gain;
            for (t, c) in total.shade.iter_mut().zip(o.shade) {
                *t += c;
            }
        }
        total.saturation += 1.0;
        total.contrast += 1.0;
        total.gain += 1.0;
        total.shade = total.shade.map(|c| c + 1.0);
        total
    }
}

/// One biome, as a `.biome.ron` file spells it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BiomeDef {
    /// The namespaced id, `strata:plains`. Also the file's key in the data
    /// stage, so two files with one id collide rather than shadow.
    pub id: Namespaced,
    /// What the HUD calls it.
    pub name: String,
    /// The climate the biome claims.
    pub climate: ClimateWindow,
    /// How it shapes the ground.
    pub relief: Relief,
    /// What the ground is made of.
    pub surface: Surface,
    /// Map and vertex colour.
    pub tint: Tint,
    /// The colour grade the frame takes while the observer is here (the
    /// lighting plan's phase 4), blended across a border by the biomes'
    /// weights. Optional: a file without one is neutral and writes none.
    #[serde(default, skip_serializing_if = "Grade::is_neutral")]
    pub grade: Grade,
    /// The colour the ground is painted in (ticket 0006, pass 1): the
    /// terrain draws it in place of [`tint`](Self::tint) where it is set,
    /// at range as the far colour and near as the hue its layers are pulled
    /// towards. The map keeps `tint`. Optional: a file without one paints
    /// the ground in its tint.
    ///
    /// Read but never written, like a palette rather than a property of the
    /// ground: the world's version hashes every biome as it serialises
    /// (`strata_worldgen::world_version`), and a client on another palette
    /// stands on the same ground, as one on another grade does. A tool that
    /// writes a biome file back must copy this field by hand.
    ///
    /// Spelt as a bare colour, not `Some(..)`: the loader reads the file
    /// through RON's `Value`, which has no implicit option.
    #[serde(default, skip_serializing, deserialize_with = "some_tint")]
    pub ground: Option<Tint>,
}

/// A colour present in the file, as an `Option` that is `Some`.
fn some_tint<'de, D: Deserializer<'de>>(d: D) -> Result<Option<Tint>, D::Error> {
    Tint::deserialize(d).map(Some)
}

impl BiomeDef {
    /// The colour the terrain paints the biome's ground in: its
    /// [`ground`](Self::ground) colour, or its tint without one.
    #[must_use]
    pub fn ground_colour(&self) -> Tint {
        self.ground.unwrap_or(self.tint)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    #[test]
    fn relief_kind_round_trips_through_a_string() {
        for kind in ReliefKind::ALL {
            let text = ron::to_string(&kind).expect("serialises");
            assert_eq!(text, format!("\"{kind}\""));
            let back: ReliefKind = ron::from_str(&text).expect("parses");
            assert_eq!(back, kind);
        }
        assert!(ron::from_str::<ReliefKind>("\"volcano\"").is_err());
    }

    #[test]
    fn a_def_survives_the_value_round_trip() {
        let def = BiomeDef {
            id: Namespaced::parse("strata:test").expect("valid"),
            name: "Test".into(),
            climate: ClimateWindow {
                temperature: UnitRange::new(0.2, 0.8),
                moisture: UnitRange::FULL,
                elevation: UnitRange::new(0.5, 0.7),
            },
            relief: Relief {
                kind: ReliefKind::Dunes,
                amplitude_m: 30.0,
                wavelength_m: 400.0,
                octaves: 3,
                persistence: 0.5,
                lacunarity: 2.0,
                lift_m: 0.0,
                erosion: 0.0,
            },
            surface: Surface {
                material: Namespaced::parse("strata:sand").expect("valid"),
                slope_material: Namespaced::parse("strata:rock").expect("valid"),
                slope_threshold: 0.7,
            },
            tint: Tint([0.9, 0.8, 0.5]),
            grade: Grade {
                temperature: -0.01,
                saturation: 0.95,
                ..Grade::NEUTRAL
            },
            ground: None,
        };
        let text = ron::to_string(&def).expect("serialises");
        let value: ron::Value = ron::from_str(&text).expect("parses as a value");
        let back: BiomeDef = value.into_rust().expect("typed from the value");
        assert_eq!(back, def);
    }

    /// The ground's palette colour is read from a file and painted in place
    /// of the tint, but never written: the world's version hashes a biome
    /// as it serialises, and a palette is not the ground.
    #[test]
    fn the_ground_colour_is_read_and_never_written() {
        let text = r#"(
            id: "strata:test",
            name: "Test",
            climate: (
                temperature: (min: 0.0, max: 1.0),
                moisture: (min: 0.0, max: 1.0),
                elevation: (min: 0.5, max: 0.7),
            ),
            relief: (kind: "plains", amplitude_m: 10.0, wavelength_m: 500.0, octaves: 2),
            surface: (material: "strata:grass", slope_material: "strata:rock"),
            tint: (0.4, 0.6, 0.3),
            ground: (0.5, 0.6, 0.3),
        )"#;
        // Through RON's `Value`, as the loader reads a file.
        let value: ron::Value = ron::from_str(text).expect("parses as a value");
        let def: BiomeDef = value.into_rust().expect("typed from the value");
        assert_eq!(def.ground, Some(Tint([0.5, 0.6, 0.3])));
        assert_eq!(def.ground_colour(), Tint([0.5, 0.6, 0.3]));
        let written = ron::to_string(&def).expect("serialises");
        assert!(!written.contains("ground"), "{written}");
        let unpainted = BiomeDef {
            ground: None,
            ..def.clone()
        };
        assert_eq!(written, ron::to_string(&unpainted).expect("serialises"));
        assert_eq!(unpainted.ground_colour(), def.tint);
    }

    #[test]
    fn ranges_know_their_distance() {
        let range = UnitRange::new(0.25, 0.75);
        assert!(range.is_valid());
        assert!(!UnitRange::new(0.8, 0.2).is_valid());
        assert!(!UnitRange::new(-0.1, 0.2).is_valid());
        assert!(range.distance(0.5).abs() < f32::EPSILON);
        assert!((range.distance(0.0) - 0.25).abs() < 1e-6);
        assert!((range.distance(1.0) - 0.25).abs() < 1e-6);
        assert!((range.center() - 0.5).abs() < 1e-6);
    }

    #[test]
    fn a_grade_is_optional_neutral_by_default_and_range_checked() {
        let text = "(saturation: 0.9)";
        let grade: Grade = ron::from_str(text).expect("parses");
        assert_eq!(
            grade,
            Grade {
                saturation: 0.9,
                ..Grade::NEUTRAL
            }
        );
        assert!(Grade::default().is_neutral());
        assert!(grade.invalid_field().is_none());
        for (bad, field) in [
            (
                Grade {
                    temperature: 0.08,
                    ..Grade::NEUTRAL
                },
                "temperature",
            ),
            (
                Grade {
                    saturation: f32::NAN,
                    ..Grade::NEUTRAL
                },
                "saturation",
            ),
            (
                Grade {
                    contrast: 0.2,
                    ..Grade::NEUTRAL
                },
                "contrast",
            ),
        ] {
            assert_eq!(bad.invalid_field().map(|(f, _)| f), Some(field));
        }
        assert!(ron::from_str::<Grade>("(warmth: 1.0)").is_err());
    }

    #[test]
    fn grades_sum_as_offsets_from_neutral() {
        let warm = Grade {
            temperature: 0.01,
            saturation: 1.2,
            ..Grade::NEUTRAL
        };
        let cold = Grade {
            temperature: -0.01,
            saturation: 0.8,
            lift: 0.004,
            ..Grade::NEUTRAL
        };
        let mean = Grade::sum([(&warm, 0.5), (&cold, 0.5)]);
        assert!(mean.temperature.abs() < 1e-7);
        assert!((mean.saturation - 1.0).abs() < 1e-6);
        assert!((mean.lift - 0.002).abs() < 1e-7);
        let both = Grade::sum([(&warm, 1.0), (&cold, 1.0)]);
        assert!((both.saturation - 1.0).abs() < 1e-6);
        assert!((Grade::sum([(&warm, 1.0)]).saturation - 1.2).abs() < 1e-6);
        assert!(Grade::sum([]).is_neutral());
    }

    #[test]
    fn tints_clamp_to_bytes() {
        assert_eq!(Tint([0.0, 1.0, 2.0]).to_rgb8(), [0, 255, 255]);
        assert_eq!(Tint([-1.0, 0.5, 0.25]).to_rgb8(), [0, 128, 64]);
    }
}
