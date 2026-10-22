//! Irradiance probes per streamed region (lighting plan phase 3b): shade
//! takes coloured light from the sky and one bounce off the ground.
//!
//! Each built LOD 0 region within [`PROBE_REACH_M`] of the observer gets,
//! on the async compute pool, a grid of 8 x 4 x 8 ambient cubes over its
//! square (grown by an eighth of
//! its side on every side, so neighbours overlap) from 8 m under its lowest
//! ground to 64 m over its highest. A cube face is the cosine-weighted mean
//! radiance of what the probe sees on that side: above the heightfield's
//! horizon, the sky; below it, the ground under the probe, lit by the sun
//! (where its own horizon lets the sun in) and by the open sky, times its
//! albedo (the biomes' far tint and the canopy colour by their cover). The
//! result is Bevy's [`IrradianceVolume`]: a child of the region entity, so
//! it goes when the region goes, in an `Rgba16Float` 3D image packed as
//! `bevy_pbr::light_probe::irradiance_volume` documents. Of those, the
//! [`DRAWN_REGIONS`] nearest a point ahead of the observer along the view
//! are drawn, which is what the GPU can afford (see there).
//!
//! # Two halves of the bake
//!
//! The rays are the expensive half and depend on the world alone: per
//! probe column, 8 azimuths of 9 heightfield samples out to 1 km give
//! the horizon's tangent per azimuth at the ground and at each probe. Only
//! divisions and square roots, so a region bakes to the same bits on every
//! machine. They are kept ([`ProbeGeometry`]). The light is the cheap
//! half: the sky's radiance by direction and the sun on the ground, which
//! [`pack_probes`] turns into texels for a [`SkyTable`]. When the sun has
//! moved more than 3.75 degrees (fifteen minutes of the day) or its light
//! changed by a tenth, every live probe is re-packed from its kept geometry
//! on the pool, and the images are replaced.
//!
//! # Why LOD 0 only
//!
//! Bevy 0.19.1 gives a view at most eight irradiance volumes
//! (`MAX_VIEW_LIGHT_PROBES`): those in the frustum, nearest centre first.
//! LOD 0 regions (64 m) tile the ground out to 160 m from the observer,
//! about 25 of them, so a LOD 1 region's probe would never be among the
//! nearest eight and its bake would be wasted; and the cost of a drawn
//! probe is the screen it covers, so only two are drawn in any case, both
//! within a region or two of the player, which is LOD 0 ground.
//!
//! # The sky fallback
//!
//! With any irradiance volume in a view, Bevy's PBR shader takes the
//! volume's light in place of the environment map's diffuse light for
//! every fragment (`found_diffuse_indirect` is set whether or not a volume
//! contains the fragment), so ground outside the probes would lose the sky
//! altogether. One more probe, [`SkyFallbackProbe`], a single ambient cube
//! 200 km wide that follows the camera, holds the open sky over flat
//! ground of the probes' mean albedo. Its falloff is 50, so its weight is
//! a hundredth at its centre and less outwards: Bevy normalises the
//! weights of the volumes a fragment is in, so inside a region probe the
//! fallback adds a percent, and outside every region probe it is all
//! there is. It sits at the camera, nearest of all, so it always has a slot.
//!
//! That hundredth is also why a drawn probe fades at its sides. Where a
//! drawn region probe's box ends against ground no other drawn probe
//! covers, its share against the fallback is `w / (w + 0.01)`, which runs
//! from nine tenths to nothing over the last metre and a half of the box,
//! not over its blend band: the ambient term showed a line 8 m into the
//! next region. So on every side whose neighbour is not drawn, a drawn
//! probe's texels hold the fallback's own light in their outermost ring
//! and half of it in the next (`EDGE_FADE`); wherever the weights
//! switch, the light is already the fallback's, and the probe's comes in
//! between the rings' centres and the third ring, from 3 m outside the
//! region to 17 m inside it. Between two drawn probes nothing fades: there
//! both weights ramp over the 16 m their boxes overlap. A region that
//! starts or stops being drawn fades all of its light in or out over
//! [`PRESENCE_S`], drawn until it is out, so the camera turning past the
//! ranking's hysteresis does not switch 64 m of ground at once.
//!
//! # With the terrain's sky visibility
//!
//! The terrain mesher bakes the sky's visibility per vertex into the
//! material's diffuse occlusion from rays that read the ground out to
//! [`VERTEX_SKY_REACH_M`], and Bevy multiplies the volume's light by that
//! occlusion. So the two split the sky by elevation: the vertex term
//! takes what the ground within its reach hides (a gully's walls, a bank),
//! and the probe, of what is left, what its whole horizon hides too (a
//! ridge a kilometre off over a low gully, or a ravine's walls where they
//! rise higher further out). Per direction the probe keeps the share of
//! the sky over its near horizon that is also over its whole horizon
//! (`sky_share`); the vertex term's share times that is the sky over the
//! whole horizon, so a gully darkens the ground once. A probe that took
//! its whole horizon counted the near walls twice, and one that only
//! dropped the near ray steps still did wherever the walls kept rising
//! past them. The near horizon is the probe's own, 1 m or more over its
//! column, standing in for the fragment's. What has no vertex term (a
//! character, a building, a tree's trunk) takes no occlusion from ground
//! that near; SSAO is what darkens it there. The ground's own sky and sun,
//! which light the bounce, read the whole horizon: that is light on
//! another surface, not the one the vertex term darkens.
//!
//! # Clustering
//!
//! Bevy sorts light probes into its clusters by their bounding spheres,
//! and by default ends the clusters' last depth slice at the farthest
//! clusterable object of the last frame. With the fallback that is about
//! 350 km, which spreads the 24 depth slices so thin that the street's
//! lamps share slices tens of metres deep. `bound_clusters` ends each
//! 3D camera's slices at the farthest point light, spot light and drawn
//! region probe instead, as Bevy would without the fallback. A fragment
//! past that falls in the last slice (Bevy clamps the slice when it
//! assigns and when it shades), and the fallback, which covers
//! everything, is in it.
//!
//! # Units
//!
//! Texels hold radiance per unit of the directional lights' illuminance as
//! Bevy's atmosphere uses it (lux in, cd/m^2 out); the sky's diffuse
//! environment map is in the same units. The volume's intensity is the
//! look's (`strata-atmosphere`, `RenderLook::irradiance` times the sky
//! fill), which every `IrradianceVolume` in the world takes, and at zero
//! the volumes are hidden, which gives the environment map back. The sky
//! fill (4) lifts the sky's light so shade does not go black under the
//! tonemapper; the sun's bounce is physical and needs no lift, so the pack
//! divides it by the fill the intensity multiplies back ([`Lighting`]).
//! The analytic sky is fitted to the light of Bevy's drawn one
//! (`SKY_GAIN_HIGH`), so with the probes on, open ground in shade is lit
//! as the environment map lit it, and what changes is the occlusion of
//! the sky by the land and the bounce.
//!
//! # Clouds
//!
//! The cloud layer's shadow is a texture on the sun this crate cannot
//! read; the client copies the layer's clear share at the camera and over
//! the sky into [`ProbeClouds`], and the sun's bounce is scaled by it (the
//! camera's for the region probes, the sky's for the fallback), re-packed
//! when it moves by a tenth.

// Probe grids are a handful of cells and region sizes are powers of two
// under 32 km: every cast between usize, u32, f32 and f64 here is exact or
// loses only what an f32 position never had.
#![allow(
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    clippy::cast_possible_wrap,
    clippy::cast_sign_loss,
    reason = "grid indices and region sizes are small and exact in every width used"
)]
// The radiative transfer uses the conventional single letters.
#![allow(clippy::many_single_char_names, reason = "radiative transfer maths")]
// `mu_s` and `m_s` are the sun's zenith cosine and air mass, as written.
#![allow(
    clippy::similar_names,
    reason = "the conventional names of the formulas"
)]
// A colour is three channels computed from several arrays in step; an
// index reads as the formula does, an iterator chain would not.
#![allow(
    clippy::needless_range_loop,
    reason = "per-channel loops index several arrays in step"
)]

use std::collections::{HashMap, HashSet};
use std::sync::{Arc, OnceLock};

use bevy::app::{App, PostUpdate, Update};
use bevy::asset::{Assets, RenderAssetUsages};
use bevy::camera::visibility::{RenderLayers, ViewVisibility, Visibility, VisibilitySystems};
use bevy::camera::{Camera, Camera3d};
use bevy::diagnostic::DiagnosticsStore;
use bevy::ecs::prelude::*;
use bevy::image::Image;
use bevy::light::cluster::{ClusterConfig, ClusterFarZMode, ClusterZConfig, Clusters};
use bevy::light::{
    DirectionalLight, EnvironmentMapLight, IrradianceVolume, LightProbe, PointLight,
    SimulationLightSystems, SpotLight,
};
use bevy::math::{Vec3, Vec3A};
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};
use bevy::state::condition::in_state;
use bevy::tasks::{AsyncComputeTaskPool, Task, TaskPool, block_on, poll_once};
use bevy::transform::TransformSystems;
use bevy::transform::components::{GlobalTransform, Transform};
use strata_app::{AppState, Observers, WorldSourceRes};
use strata_core::RegionCoord;
use strata_core::platform::time::Instant;
use strata_stream::{Region, RegionUnload, distance_to_region};
use strata_worldgen::{Sample, WorldSource};

use crate::mesh::BiomeLayers;
use crate::{MeshInputs, TerrainRegion, TerrainSet};

/// Ambient cubes across a region, east-west.
pub const PROBES_X: usize = 8;
/// Ambient cubes from the foot of the box to its top.
pub const PROBES_Y: usize = 4;
/// Ambient cubes across a region, north-south.
pub const PROBES_Z: usize = 8;

/// The coarsest LOD that gets probes. See the module docs, "Why LOD 0
/// only".
pub const MAX_PROBE_LOD: u8 = 0;

/// Regions nearer the primary observer than this, metres to their nearest
/// point, get probes. Bevy draws the seven nearest in view, all within
/// about 150 m of a third-person camera; a settlement's detail zone keeps
/// hundreds of LOD 0 regions loaded, and baking them all would only queue
/// work on the pool for probes that are never drawn.
pub const PROBE_REACH_M: f64 = 192.0;

/// How many region probes are drawn at once: those of the regions whose
/// centres are nearest a point [`FOCUS_AHEAD_M`] ahead of the observer
/// along the camera's view, so the bounce is where the player looks. The
/// others stay baked but without their `LightProbe`, which takes them out
/// of Bevy's gathering. The cost is the screen the region volumes cover,
/// not their number: a fragment in one samples it and the sky fallback
/// (two volumes, six 3D taps, through a binding array indexed per
/// fragment). Measured 2026-10-03 at 2560x1440 by in-run A/B against the
/// probes hidden (the render look study, phase 3b): at the canopy fixture
/// the opaque pass took 0.16 to 0.33 ms more with two drawn, 0.44 to 0.55
/// with three and about the same with all seven Bevy would take (a box
/// topped at 12 m instead of 64 cost the same, so it is the ground they
/// cover and not the crowns); at the dusk street 0.17 with three and 0.29
/// to 0.40 with seven. The phase's ceiling is 0.3 ms.
pub const DRAWN_REGIONS: usize = 2;

/// How much nearer a drawn probe's region counts when the drawn ones are
/// ranked, metres.
const DRAWN_HYSTERESIS_M: f64 = 12.0;

/// How far ahead of the observer, metres along the camera's horizontal
/// view, the drawn probes are ranked from.
pub const FOCUS_AHEAD_M: f64 = 32.0;

/// A probe whose region is further than this goes, and its region bakes
/// again if the observer comes back within reach.
pub const PROBE_DROP_M: f64 = 320.0;

/// How far the box reaches over the region's highest ground, metres.
pub const HEADROOM_M: f32 = 64.0;

/// How far the box reaches under the region's lowest ground, metres, and
/// the depth of the box's vertical fade: the lowest ground is where the
/// fade ends, so every fragment on the ground takes the probe in full.
const FOOT_M: f32 = 8.0;

/// The box grows by this share of the region's side, half on each side,
/// and fades over it, so at the region's border the probes on either side
/// weigh a half each and no fragment falls between them to the fallback.
const BLEND_SHARE: f64 = 0.25;

/// No probe sits less than this over the ground under it: a probe in the
/// hill would see only rock and leak dark into the slope, and the cube's
/// trilinear filter would carry it up to the surface.
const PROBE_LIFT_M: f32 = 1.0;

/// Horizon azimuths per probe, 45 degrees apart, from +x towards +z.
const AZIMUTHS: usize = 8;

/// Equal-area bands of the sphere, by the sine of the elevation, so every
/// direction of the integration grid stands for the same solid angle.
const BANDS: usize = 8;

/// Directions of the integration grid.
const DIRECTIONS: usize = AZIMUTHS * BANDS;

/// The first band over the horizontal.
const UPPER_BAND: usize = BANDS / 2;

/// Distances, metres, at which the horizon rays sample the heightfield.
/// Doubling, so a near bank and a hill a kilometre off both count: a low
/// sun goes behind a ridge that far away (Port Serein's at 17:30), and
/// ground the bake took for sunlit then bounced a warm glow into shade.
const RAY_STEPS_M: [f64; 9] = [4.0, 8.0, 16.0, 32.0, 64.0, 128.0, 256.0, 512.0, 1024.0];

/// How far the terrain's vertex sky term reads the ground at LOD 0,
/// metres: its rays' last reach is 32 units of 2 m along an axis and 23
/// of 2 m along each axis on a diagonal (`mesh.rs`, `SKY_AXIS_REACH`,
/// `SKY_DIAGONAL_REACH` and `SKY_MIN_UNIT_M`). The ray steps within it
/// give a probe's near horizon, whose share of the sky the probe leaves
/// to the vertex term. See the module docs, "With the terrain's sky
/// visibility".
pub const VERTEX_SKY_REACH_M: f64 = 65.0;

/// A drawn probe's share of its own light in the rings of texels along a
/// side whose neighbour is not drawn, outermost first; the rest is the
/// fallback's. See the module docs, "The sky fallback".
const EDGE_FADE: [f32; 2] = [0.0, 0.5];

/// The eight neighbours of a region, as region steps along `x` and `z`;
/// bit `n` of a drawn mask is `NEIGHBOURS[n]`.
const NEIGHBOURS: [(i32, i32); 8] = [
    (1, 0),
    (1, 1),
    (0, 1),
    (-1, 1),
    (-1, 0),
    (-1, -1),
    (0, -1),
    (1, -1),
];

/// Seconds a region probe takes to fade in when it starts being drawn, and
/// out when it stops.
pub const PRESENCE_S: f32 = 0.6;

/// Steps of that fade an image is written at: a write replaces the
/// volume's texture, so a fade is eight writes and not one a frame.
const PRESENCE_STEPS: f32 = 8.0;

/// The eight azimuths as `(x, z)` unit vectors, literal so no platform
/// trigonometry enters the bake.
const AZIMUTH_XZ: [[f32; 2]; AZIMUTHS] = [
    [1.0, 0.0],
    [0.707_106_77, 0.707_106_77],
    [0.0, 1.0],
    [-0.707_106_77, 0.707_106_77],
    [-1.0, 0.0],
    [-0.707_106_77, -0.707_106_77],
    [0.0, -1.0],
    [0.707_106_77, -0.707_106_77],
];

/// Faces of an ambient cube in the order this module keeps them.
const FACES: usize = 6;

/// The cube's face normals: +x, -x, +y, -y, +z, -z.
const FACE_NORMALS: [[f32; 3]; FACES] = [
    [1.0, 0.0, 0.0],
    [-1.0, 0.0, 0.0],
    [0.0, 1.0, 0.0],
    [0.0, -1.0, 0.0],
    [0.0, 0.0, 1.0],
    [0.0, 0.0, -1.0],
];

/// Bytes of one `Rgba16Float` texel.
const TEXEL_BYTES: usize = 8;

/// The fallback's side, metres: past the 48 km view distance either way.
const FALLBACK_SIDE_M: f32 = 200_000.0;

/// The fallback's falloff: its weight at its centre is `1 / (2 * 50)`.
const FALLBACK_FALLOFF: f32 = 50.0;

/// A fallback albedo before any region probe exists: dry grass.
const DEFAULT_ALBEDO: [f32; 3] = [0.12, 0.12, 0.08];

/// The albedo of open water seen from above at the angles a probe sees it.
const WATER_ALBEDO: [f32; 3] = [0.03, 0.04, 0.05];

/// Re-pack when the sun's direction has turned further than this: cos of
/// 3.75 degrees, the sun's hour angle in fifteen minutes.
const REPACK_COS: f32 = 0.997_858_9;

/// Re-pack when a light's illuminance has changed by more than this share.
const REPACK_ILLUMINANCE: f32 = 0.1;

/// Re-pack when the sun's clear share through the clouds has changed by
/// more than this.
const REPACK_CLEAR: f32 = 0.1;

// ---- the sky ------------------------------------------------------------

/// Bevy's Earth medium at the slice's default haze (`SkySettings::haze`
/// 0.55 multiplies every term's density). The analytic sky reads the
/// default; a haze changed in the F6 panel moves the drawn sky and not the
/// probes, which only matters far from 0.55.
const HAZE: f32 = 0.55;

/// Rayleigh optical depth straight up, per channel: Bevy's
/// `ScatteringMedium::earth` scattering coefficients over its 8 km scale
/// height.
const RAYLEIGH_TAU: [f32; 3] = [0.046_416, 0.108_464, 0.264_8];

/// Mie optical depth straight up: Bevy's absorption plus scattering over
/// its 1.2 km scale height; the aerosols mostly absorb in Bevy's medium.
const MIE_EXTINCTION_TAU: f32 = 0.005_328;

/// Mie scattering optical depth straight up.
const MIE_SCATTERING_TAU: f32 = 0.000_532_8;

/// Ozone absorption straight up: Bevy's coefficients over half its 18 km
/// tent.
const OZONE_TAU: [f32; 3] = [0.005_85, 0.016_929, 0.000_765];

/// Planet radius over the Rayleigh scale height, for the air mass of a
/// spherical shell.
const AIR_MASS_K: f32 = 795.0;

/// The Mie phase function's asymmetry, Bevy's for Earth.
const MIE_G: f32 = 0.8;

/// Light scattered more than once, as a share of the once-scattered light
/// of a level surface, spread evenly over the sky. Bevy's multiple
/// scattering table is what the sky in the frame has; this stands in for
/// it, chosen against captures of the environment map's own light (the
/// render look study, phase 3b).
const MULTIPLE_SCATTERING: f32 = 1.2;

/// Per-channel gain from the analytic sky to the light of Bevy's drawn one
/// with the sun high, and what is added as it sinks, times `(1 - mu)^4`
/// (`mu` the sine of the sun's elevation). Fitted 2026-10-03 at Port
/// Serein's street (`--at city`, no clouds, the fallback alone and no
/// bounce) against the environment map's light on the same shaded
/// surfaces, compared in linear output: without it the analytic sky
/// matched in red at noon and was an eighth short in blue, and at 17:30
/// (sun six degrees up), where Bevy's multiple scattering keeps the
/// zenith blue while the sun reddens, it gave 0.8, 0.7 and 0.55 of the
/// light. With it the shaded floor, the character and the shadows agree
/// within about a tenth at noon, 08:00 and 17:30.
const SKY_GAIN_HIGH: [f32; 3] = [1.0, 1.06, 1.2];

/// See [`SKY_GAIN_HIGH`].
const SKY_GAIN_LOW: [f32; 3] = [0.33, 0.35, 0.61];

/// How far below the horizon, as the sine of the sun's elevation, the sky
/// keeps a twilight glow.
const TWILIGHT: f32 = 0.1;

/// One directional light as the probes see it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SkyLight {
    /// Unit vector from the ground towards the light.
    pub to_light: [f32; 3],
    /// Illuminance at the top of the air, lux, per linear channel (the
    /// light's colour times its illuminance).
    pub illuminance: [f32; 3],
}

/// What the probes are lit by: the directional lights and the look's sky
/// fill.
#[derive(Debug, Clone, PartialEq)]
pub struct Lighting {
    /// The directional lights, brightest first.
    pub lights: Vec<SkyLight>,
    /// The sky fill the volumes' intensity carries (the camera's
    /// environment map intensity, `SkySettings::environment_intensity`):
    /// the sky's light is drawn that much stronger than the atmosphere
    /// gives it, so shade does not go black under the tonemapper. The
    /// sun's bounce is physical and needs no lift, so the pack divides it
    /// by the fill the volume's intensity will multiply it by again.
    pub fill: f32,
    /// The share of the sun's light that reaches the ground through the
    /// cloud layer, 0 to 1: the camera's for the region probes, the
    /// layer's clear share of the sky for the fallback (see
    /// [`ProbeClouds`]). Only the bounce takes it; the sky's light is the
    /// atmosphere's, which has no clouds either.
    pub sun_clear: f32,
}

impl Default for Lighting {
    fn default() -> Self {
        Self {
            lights: Vec::new(),
            fill: 1.0,
            sun_clear: 1.0,
        }
    }
}

/// The light every probe is packed under: the directional lights as they
/// are, reduced to what [`pack_probes`] needs.
#[derive(Debug, Clone, PartialEq)]
pub struct SkyTable {
    /// The sky's radiance towards each upper direction of the integration
    /// grid, every light summed; zero below the horizontal.
    sky: [[f32; 3]; DIRECTIONS],
    /// Per light: towards it, its irradiance on a surface facing it after
    /// the air over the sky fill, and the tangent of its elevation.
    lights: Vec<GroundLight>,
}

#[derive(Debug, Clone, Copy, PartialEq)]
struct GroundLight {
    to_light: [f32; 3],
    irradiance: [f32; 3],
    tan_elevation: f32,
    /// The two azimuths either side of the light and the second's share.
    azimuths: (usize, usize, f32),
}

impl SkyTable {
    /// The sky and the ground light for `lighting`.
    #[must_use]
    pub fn new(lighting: &Lighting) -> Self {
        let lights = &lighting.lights;
        let fill = lighting.fill.max(1e-3);
        let clear = lighting.sun_clear.clamp(0.0, 1.0);
        let basis = basis();
        let mut sky = [[0.0f32; 3]; DIRECTIONS];
        for (d, dir) in basis.dirs.iter().enumerate() {
            if d / AZIMUTHS < UPPER_BAND {
                continue;
            }
            for light in lights {
                let l = sky_radiance(*dir, light);
                for c in 0..3 {
                    sky[d][c] += l[c];
                }
            }
        }
        let lights = lights
            .iter()
            .filter(|l| l.to_light[1] > 0.0)
            .map(|light| {
                let t = sun_transmittance(light.to_light[1]);
                let [x, y, z] = light.to_light;
                let flat = (x * x + z * z).sqrt();
                GroundLight {
                    to_light: light.to_light,
                    irradiance: [0, 1, 2].map(|c| light.illuminance[c] * t[c] * clear / fill),
                    tan_elevation: if flat > 1e-6 { y / flat } else { 1e6 },
                    azimuths: azimuth_pair(x, z),
                }
            })
            .collect();
        Self { sky, lights }
    }
}

/// The two grid azimuths either side of the horizontal direction `(x,
/// z)` and how far towards the second it lies, 0 to 1: the nearest
/// azimuth, the one of its neighbours on the light's side (the nearer of
/// the two), and `b / (a + b)` from `(x, z) = a * first + b * second`,
/// which is monotonic in the angle and needs no arctangent.
fn azimuth_pair(x: f32, z: f32) -> (usize, usize, f32) {
    let dot = |k: usize| x * AZIMUTH_XZ[k][0] + z * AZIMUTH_XZ[k][1];
    let best = (0..AZIMUTHS)
        .max_by(|&a, &b| dot(a).total_cmp(&dot(b)))
        .unwrap_or(0);
    let next = (best + 1) % AZIMUTHS;
    let prev = (best + AZIMUTHS - 1) % AZIMUTHS;
    let other = if dot(next) >= dot(prev) { next } else { prev };
    let [fx, fz] = AZIMUTH_XZ[best];
    let [ox, oz] = AZIMUTH_XZ[other];
    let det = fx * oz - fz * ox;
    let a = (x * oz - z * ox) / det;
    let b = (fx * z - fz * x) / det;
    let share = if a + b > 1e-9 {
        (b / (a + b)).clamp(0.0, 1.0)
    } else {
        0.0
    };
    (best, other, share)
}

/// `exp(x)` for `x <= 0` from correctly rounded operations only (a range
/// reduction by `ln 2` and a degree-six polynomial, relative error under
/// 1e-6), so a pack is the same bits on every machine for the same light.
fn exp_det(x: f32) -> f32 {
    if x.is_nan() || x < -87.0 {
        return 0.0;
    }
    if x >= 0.0 {
        return 1.0;
    }
    let ln2 = core::f32::consts::LN_2;
    let n = (x / ln2 - 0.5) as i32;
    let r = x - n as f32 * ln2;
    let p = 1.0
        + r * (1.0
            + r * (0.5 + r * (1.0 / 6.0 + r * (1.0 / 24.0 + r * (1.0 / 120.0 + r / 720.0)))));
    // 2^n by its exponent bits; n >= -126 since x >= -87.
    p * f32::from_bits(((n + 127) as u32) << 23)
}

/// The air mass of a ray leaving the ground at zenith cosine `mu >= 0`,
/// through a spherical shell of the Rayleigh scale height: 1 straight up,
/// about 40 along the horizon.
fn air_mass(mu: f32) -> f32 {
    let k = AIR_MASS_K;
    let km = k * mu.max(0.0);
    (km * km + 2.0 * k + 1.0).sqrt() - km
}

/// Vertical optical depth per channel, everything.
fn extinction_tau() -> [f32; 3] {
    [0, 1, 2].map(|c| HAZE * (RAYLEIGH_TAU[c] + MIE_EXTINCTION_TAU + OZONE_TAU[c]))
}

/// The share of a light at zenith cosine `mu` that reaches the ground.
fn sun_transmittance(mu: f32) -> [f32; 3] {
    if mu <= 0.0 {
        return [0.0; 3];
    }
    let m = air_mass(mu);
    extinction_tau().map(|tau| exp_det(-tau * m))
}

/// The sky's radiance towards `view` (upper hemisphere) from one light:
/// single scattering in a plane-parallel atmosphere of Bevy's Earth medium
/// in closed form, plus an even share for the light scattered more than
/// once, faded out as the light sinks below the horizon.
fn sky_radiance(view: [f32; 3], light: &SkyLight) -> [f32; 3] {
    let mu_s = light.to_light[1];
    if mu_s < -TWILIGHT {
        return [0.0; 3];
    }
    let twilight = if mu_s >= 0.0 {
        1.0
    } else {
        let t = 1.0 + mu_s / TWILIGHT;
        t * t
    };
    let m_v = air_mass(view[1]);
    let m_s = air_mass(mu_s);
    let cos_g =
        view[0] * light.to_light[0] + view[1] * light.to_light[1] + view[2] * light.to_light[2];
    let rayleigh_phase = 3.0 / (16.0 * core::f32::consts::PI) * (1.0 + cos_g * cos_g);
    let g2 = MIE_G * MIE_G;
    let denom = 1.0 + g2 - 2.0 * MIE_G * cos_g;
    let mie_phase = 3.0 / (8.0 * core::f32::consts::PI) * (1.0 - g2) * (1.0 + cos_g * cos_g)
        / ((2.0 + g2) * denom * denom.sqrt());
    let tau = extinction_tau();
    let level = mu_s.max(0.0);
    let mut out = [0.0f32; 3];
    for c in 0..3 {
        let scatter_r = HAZE * RAYLEIGH_TAU[c];
        let scatter_m = HAZE * MIE_SCATTERING_TAU;
        let t = tau[c];
        // The share of the light scattered once towards the eye, over the
        // scattering coefficient: m_v (e^{-t m_v} - e^{-t m_s}) / ((m_s -
        // m_v) t), and its limit m_v e^{-t m_v} as the air masses meet.
        let path = if (m_s - m_v).abs() > 1e-3 {
            m_v * (exp_det(-t * m_v) - exp_det(-t * m_s)) / ((m_s - m_v) * t)
        } else {
            m_v * exp_det(-t * m_v)
        };
        let single = (scatter_r * rayleigh_phase + scatter_m * mie_phase) * path;
        let multiple = MULTIPLE_SCATTERING
            * (scatter_r + scatter_m)
            * exp_det(-t * m_s)
            * (level + 0.25 * TWILIGHT)
            / (4.0 * core::f32::consts::PI);
        let low = 1.0 - level;
        let gain = SKY_GAIN_HIGH[c] + SKY_GAIN_LOW[c] * (low * low) * (low * low);
        out[c] = gain * twilight * light.illuminance[c] * (single + multiple);
    }
    out
}

// ---- the integration grid -------------------------------------------------

/// The integration grid's directions and the normalised cosine weight of
/// each on each cube face.
struct Basis {
    dirs: [[f32; 3]; DIRECTIONS],
    /// `weights[face][dir]`, summing to one per face, so an even radiance
    /// gives that radiance on every face.
    weights: [[f32; DIRECTIONS]; FACES],
}

fn basis() -> &'static Basis {
    static BASIS: OnceLock<Basis> = OnceLock::new();
    BASIS.get_or_init(|| {
        let mut dirs = [[0.0f32; 3]; DIRECTIONS];
        for band in 0..BANDS {
            let z = -1.0 + (band as f32 + 0.5) * (2.0 / BANDS as f32);
            let flat = (1.0 - z * z).sqrt();
            for (a, [ax, az]) in AZIMUTH_XZ.iter().enumerate() {
                dirs[band * AZIMUTHS + a] = [flat * ax, z, flat * az];
            }
        }
        let mut weights = [[0.0f32; DIRECTIONS]; FACES];
        for (face, n) in FACE_NORMALS.iter().enumerate() {
            let mut sum = 0.0;
            for (d, dir) in dirs.iter().enumerate() {
                let w = (n[0] * dir[0] + n[1] * dir[1] + n[2] * dir[2]).max(0.0);
                weights[face][d] = w;
                sum += w;
            }
            for w in &mut weights[face] {
                *w /= sum;
            }
        }
        Basis { dirs, weights }
    })
}

/// How much of the integration grid's direction `d` (an upper one) lies
/// over a horizon whose elevation has sine `horizon` in its azimuth: the
/// share of its band, which is exact for equal-area bands.
fn band_visibility(d: usize, horizon: [f32; AZIMUTHS]) -> f32 {
    let band = d / AZIMUTHS;
    let width = 2.0 / BANDS as f32;
    let top = -1.0 + (band as f32 + 1.0) * width;
    ((top - horizon[d % AZIMUTHS]) / width).clamp(0.0, 1.0)
}

// ---- the bake -------------------------------------------------------------

/// The world box a region's probes cover.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ProbeBox {
    /// World minimum corner, metres.
    pub min: [f64; 3],
    /// Side lengths, metres.
    pub size: [f64; 3],
}

impl ProbeBox {
    /// The box over `coord` whose ground spans `min_height` to
    /// `max_height`.
    #[must_use]
    pub fn over(coord: RegionCoord, min_height: f32, max_height: f32) -> Self {
        let side = coord.size();
        let grow = side * BLEND_SHARE;
        let corner = coord.min_corner();
        let lo = f64::from(min_height - FOOT_M);
        let hi = f64::from(max_height + HEADROOM_M);
        Self {
            min: [corner.x() - grow * 0.5, lo, corner.z() - grow * 0.5],
            size: [side + grow, hi - lo, side + grow],
        }
    }

    /// The centre of probe `i` along `axis` of `n`.
    fn centre(&self, axis: usize, i: usize, n: usize) -> f64 {
        self.min[axis] + (i as f64 + 0.5) * self.size[axis] / n as f64
    }
}

/// What one probe column knows about the ground under it.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Column {
    /// Ground height (the sea's surface over water), metres.
    ground: f32,
    /// Unit normal of the ground.
    normal: [f32; 3],
    /// Linear albedo of what the probes see below them.
    albedo: [f32; 3],
    /// Horizon tangent per azimuth seen from the ground, for the sun.
    sun_horizon: [f32; AZIMUTHS],
    /// Horizon elevation sine per azimuth seen from the ground, not under
    /// zero, for the sky on the ground.
    sky_horizon: [f32; AZIMUTHS],
}

/// A region's probes before the light: the horizons and the ground.
#[derive(Debug, Clone, PartialEq)]
pub struct ProbeGeometry {
    /// The region baked.
    pub coord: RegionCoord,
    /// The box the probes fill.
    pub probe_box: ProbeBox,
    columns: Vec<Column>,
    /// Per probe, `x` fastest then `z` then `y`: the horizon elevation sine
    /// per azimuth, not under zero.
    horizons: Vec<[f32; AZIMUTHS]>,
    /// The same over the ground within [`VERTEX_SKY_REACH_M`] alone: the
    /// share of the sky the vertex term already takes, which the probe's
    /// own horizon does not take again ([`sky_share`]).
    near: Vec<[f32; AZIMUTHS]>,
}

impl ProbeGeometry {
    /// The mean albedo of the ground under the probes.
    #[must_use]
    pub fn mean_albedo(&self) -> [f32; 3] {
        let n = self.columns.len().max(1) as f32;
        let mut sum = [0.0f32; 3];
        for col in &self.columns {
            for c in 0..3 {
                sum[c] += col.albedo[c];
            }
        }
        sum.map(|s| s / n)
    }

    /// Open flat ground of `albedo` under an open sky, one probe: what the
    /// fallback holds.
    fn open(albedo: [f32; 3]) -> Self {
        Self {
            coord: RegionCoord::new(0, 0, 0),
            probe_box: ProbeBox {
                min: [0.0; 3],
                size: [1.0; 3],
            },
            columns: vec![Column {
                ground: 0.0,
                normal: [0.0, 1.0, 0.0],
                albedo,
                sun_horizon: [f32::NEG_INFINITY; AZIMUTHS],
                sky_horizon: [0.0; AZIMUTHS],
            }],
            horizons: vec![[0.0; AZIMUTHS]],
            near: vec![[0.0; AZIMUTHS]],
        }
    }

    fn dims(&self) -> [usize; 3] {
        if self.horizons.len() == 1 {
            [1, 1, 1]
        } else {
            [PROBES_X, PROBES_Y, PROBES_Z]
        }
    }
}

/// sRGB to linear by the usual cubic fit, from correctly rounded
/// operations only (the exact curve needs a power).
fn srgb_to_linear(v: f32) -> f32 {
    v * (v * (v * 0.305_306 + 0.682_171_1) + 0.012_522_878)
}

/// The albedo the probes see at a sample: the biome pair's far tints by
/// share, and over them the canopy's colour by its cover.
fn albedo_of(sample: &Sample, layers: &BiomeLayers, sea_level: f32) -> [f32; 3] {
    if sample.height < sea_level {
        return WATER_ALBEDO;
    }
    let [(biome_a, weight_a), (biome_b, weight_b)] = sample.biomes;
    let total = (weight_a + weight_b).max(1e-6);
    let (wa, wb) = (weight_a / total, weight_b / total);
    let ta = layers.tint(biome_a).map(srgb_to_linear);
    let tb = layers.tint(biome_b).map(srgb_to_linear);
    let ca = layers.cover(biome_a);
    let cb = layers.cover(biome_b);
    let cover = ((ca[3] * wa + cb[3] * wb) * sample.cover.clamp(0.0, 1.0)).clamp(0.0, 1.0);
    [0, 1, 2].map(|c| {
        let ground = ta[c] * wa + tb[c] * wb;
        let canopy = ca[c] * wa + cb[c] * wb;
        ground * (1.0 - cover) + canopy * cover
    })
}

/// The horizon tangent over `y` along one azimuth, from the heights at
/// [`RAY_STEPS_M`].
fn horizon_tangent(heights: &[f32; RAY_STEPS_M.len()], y: f32) -> f32 {
    let mut best = f32::NEG_INFINITY;
    for (h, d) in heights.iter().zip(RAY_STEPS_M) {
        best = best.max((h - y) / d as f32);
    }
    best
}

/// The horizon tangent over `y` along one azimuth from the ray steps
/// within [`VERTEX_SKY_REACH_M`] alone: what the vertex term sees.
fn near_horizon_tangent(heights: &[f32; RAY_STEPS_M.len()], y: f32) -> f32 {
    let mut best = f32::NEG_INFINITY;
    for (h, d) in heights.iter().zip(RAY_STEPS_M) {
        if d <= VERTEX_SKY_REACH_M {
            best = best.max((h - y) / d as f32);
        }
    }
    best
}

/// The share of the integration grid's direction `d` (an upper one) a
/// probe takes as sky: of what its near horizon leaves, the part its whole
/// horizon leaves too. The vertex term multiplies the probe's light by
/// the share of the sky over the near horizon at the fragment, so the
/// product is the sky over the whole horizon and the near ground's
/// occlusion is counted once; a direction the near ground hides
/// altogether is the vertex term's, and open to the probe. See the module
/// docs, "With the terrain's sky visibility".
fn sky_share(d: usize, near: [f32; AZIMUTHS], whole: [f32; AZIMUTHS]) -> f32 {
    let over_near = band_visibility(d, near);
    if over_near <= 0.0 {
        return 1.0;
    }
    (band_visibility(d, whole) / over_near).min(1.0)
}

/// The sine of an elevation from its tangent, not under zero: the world
/// is flat, so beyond the last sample the ground still hides everything
/// below the horizontal.
fn horizon_sine(tangent: f32) -> f32 {
    let t = tangent.max(0.0);
    t / (1.0 + t * t).sqrt()
}

/// Bakes the horizons and the ground of a region's probes: a pure function
/// of the world, so it runs on the pool and in a test alike, and gives the
/// same bits for the same world on every machine.
#[must_use]
pub fn bake_probe_geometry(
    coord: RegionCoord,
    probe_box: ProbeBox,
    source: &dyn WorldSource,
    layers: &BiomeLayers,
) -> ProbeGeometry {
    let sea = source.sea_level();
    let mut columns = Vec::with_capacity(PROBES_X * PROBES_Z);
    let mut rays = Vec::with_capacity(PROBES_X * PROBES_Z);
    for k in 0..PROBES_Z {
        for i in 0..PROBES_X {
            let x = probe_box.centre(0, i, PROBES_X);
            let z = probe_box.centre(2, k, PROBES_Z);
            let here = source.sample(x, z);
            let ground = here.height.max(sea);
            let mut heights = [[0.0f32; RAY_STEPS_M.len()]; AZIMUTHS];
            for (a, [ax, az]) in AZIMUTH_XZ.iter().enumerate() {
                for (s, d) in RAY_STEPS_M.iter().enumerate() {
                    let sample = source.sample(x + d * f64::from(*ax), z + d * f64::from(*az));
                    heights[a][s] = sample.height.max(sea);
                }
            }
            // A central difference over the nearest step on the four axis
            // azimuths: +x, +z, -x, -z.
            let span = 2.0 * RAY_STEPS_M[0] as f32;
            let nx = -(heights[0][0] - heights[4][0]) / span;
            let nz = -(heights[2][0] - heights[6][0]) / span;
            let len = (nx * nx + 1.0 + nz * nz).sqrt();
            let mut sun_horizon = [0.0f32; AZIMUTHS];
            let mut sky_horizon = [0.0f32; AZIMUTHS];
            for a in 0..AZIMUTHS {
                let t = horizon_tangent(&heights[a], ground);
                sun_horizon[a] = t;
                sky_horizon[a] = horizon_sine(t);
            }
            columns.push(Column {
                ground,
                normal: [nx / len, 1.0 / len, nz / len],
                albedo: albedo_of(&here, layers, sea),
                sun_horizon,
                sky_horizon,
            });
            rays.push(heights);
        }
    }
    let mut horizons = Vec::with_capacity(PROBES_X * PROBES_Y * PROBES_Z);
    let mut near = Vec::with_capacity(PROBES_X * PROBES_Y * PROBES_Z);
    for j in 0..PROBES_Y {
        let y = probe_box.centre(1, j, PROBES_Y) as f32;
        for (col, heights) in columns.iter().zip(&rays) {
            let y = y.max(col.ground + PROBE_LIFT_M);
            horizons.push(core::array::from_fn(|a| {
                horizon_sine(horizon_tangent(&heights[a], y))
            }));
            near.push(core::array::from_fn(|a| {
                horizon_sine(near_horizon_tangent(&heights[a], y))
            }));
        }
    }
    ProbeGeometry {
        coord,
        probe_box,
        columns,
        horizons,
        near,
    }
}

/// The light leaving the ground under a column towards the probes: its
/// albedo times the sun on it (where its horizon lets the sun in) and the
/// open sky on it, as radiance.
fn ground_radiance(col: &Column, table: &SkyTable) -> [f32; 3] {
    let basis = basis();
    let mut light = [0.0f32; 3];
    for l in &table.lights {
        let n = col.normal;
        let cos = (n[0] * l.to_light[0] + n[1] * l.to_light[1] + n[2] * l.to_light[2]).max(0.0);
        if cos <= 0.0 {
            continue;
        }
        let (a, b, share) = l.azimuths;
        let horizon = col.sun_horizon[a] * (1.0 - share) + col.sun_horizon[b] * share;
        // A soft edge a few degrees wide, so a sun along a ridge line
        // dims the bounce rather than switching it.
        let lit = if horizon.is_finite() {
            ((l.tan_elevation - horizon) * 20.0 + 0.5).clamp(0.0, 1.0)
        } else {
            1.0
        };
        for c in 0..3 {
            light[c] += l.irradiance[c] * cos * lit / core::f32::consts::PI;
        }
    }
    let up = &basis.weights[2];
    for d in UPPER_BAND * AZIMUTHS..DIRECTIONS {
        let w = up[d] * band_visibility(d, col.sky_horizon);
        for c in 0..3 {
            light[c] += w * table.sky[d][c];
        }
    }
    [0, 1, 2].map(|c| light[c] * col.albedo[c])
}

/// The six faces of every probe in `geometry` under `table`, linear
/// radiance, `[probe][face]` in the geometry's probe order.
fn probe_faces(geometry: &ProbeGeometry, table: &SkyTable) -> Vec<[[f32; 3]; FACES]> {
    let basis = basis();
    let grounds: Vec<[f32; 3]> = geometry
        .columns
        .iter()
        .map(|col| ground_radiance(col, table))
        .collect();
    let per_level = geometry.columns.len();
    geometry
        .horizons
        .iter()
        .zip(&geometry.near)
        .enumerate()
        .map(|(p, (horizon, near))| {
            let ground = grounds[p % per_level];
            let mut vis = [0.0f32; DIRECTIONS];
            for (d, v) in vis.iter_mut().enumerate().skip(UPPER_BAND * AZIMUTHS) {
                *v = sky_share(d, *near, *horizon);
            }
            core::array::from_fn(|face| {
                let weights = &basis.weights[face];
                let mut sky = [0.0f32; 3];
                let mut open = 0.0f32;
                for d in UPPER_BAND * AZIMUTHS..DIRECTIONS {
                    let w = weights[d] * vis[d];
                    open += w;
                    for c in 0..3 {
                        sky[c] += w * table.sky[d][c];
                    }
                }
                // What the face does not see of the sky it sees of the
                // ground, since the weights sum to one.
                [0, 1, 2].map(|c| sky[c] + ground[c] * (1.0 - open))
            })
        })
        .collect()
}

/// IEEE half from a non-negative finite `f32`, rounded to nearest even;
/// negative and NaN give zero, over the range gives the largest half.
fn f16_bits(v: f32) -> u16 {
    if v.is_nan() || v <= 0.0 {
        return 0;
    }
    let x = v.min(65_504.0);
    let bits = x.to_bits();
    let exp = ((bits >> 23) & 0xff) as i32 - 127 + 15;
    let mant = bits & 0x007f_ffff;
    let round = |value: u32, rem: u32, half: u32| -> u32 {
        if rem > half || (rem == half && value & 1 == 1) {
            value + 1
        } else {
            value
        }
    };
    if exp <= 0 {
        if exp < -10 {
            return 0;
        }
        let m = mant | 0x0080_0000;
        let shift = (14 - exp) as u32;
        let value = m >> shift;
        let rem = m & ((1 << shift) - 1);
        return round(value, rem, 1 << (shift - 1)) as u16;
    }
    let value = ((exp as u32) << 10) | (mant >> 13);
    round(value, mant & 0x1fff, 0x1000) as u16
}

/// Packs `geometry`'s probes under `table` into the texels of Bevy's
/// irradiance volume layout: an `(Rx, 2 Ry, 3 Rz)` 3D image, `x` fastest,
/// then the image's rows, then its layers; `x`, `y` and `z` faces in the
/// three thirds of the layers, the positive faces in the first half of the
/// rows and the negative in the second.
///
/// That half is what `irradiance_volume.wgsl` in `bevy_pbr` 0.19.1 reads
/// (it adds half the rows for a normal's negative component), and its
/// module doc's table (positive faces on top), but not its formula, which
/// puts the positive faces at `y + Ry`: packed by the formula, a level
/// floor took the ground's light and a wall facing east the west's.
#[must_use]
pub fn pack_probes(geometry: &ProbeGeometry, table: &SkyTable) -> Vec<u8> {
    pack_faces(&probe_faces(geometry, table), geometry.dims())
}

/// The share of a drawn probe's own light at texel `(i, k)` of an `nx` by
/// `nz` grid, given which of its region's neighbours are drawn (`drawn`,
/// bit `n` for [`NEIGHBOURS`]`[n]`). See the module docs, "The sky
/// fallback".
fn edge_share(i: usize, k: usize, nx: usize, nz: usize, drawn: u8) -> f32 {
    let ring = |d: usize| EDGE_FADE.get(d).copied().unwrap_or(1.0);
    let has = |dx: i32, dz: i32| {
        NEIGHBOURS
            .iter()
            .position(|&n| n == (dx, dz))
            .is_some_and(|bit| drawn & (1 << bit) != 0)
    };
    let side = |dx: i32, dz: i32, d: usize| if has(dx, dz) { 1.0 } else { ring(d) };
    let (west, east) = (i, nx - 1 - i);
    let (south, north) = (k, nz - 1 - k);
    let mut share = side(-1, 0, west) * side(1, 0, east) * side(0, -1, south) * side(0, 1, north);
    // A corner whose two sides are drawn and whose diagonal is not: only
    // the texels in both rings are near ground nothing drawn covers.
    for (dx, dz, a, b) in [
        (-1, -1, west, south),
        (1, -1, east, south),
        (-1, 1, west, north),
        (1, 1, east, north),
    ] {
        if has(dx, 0) && has(0, dz) && !has(dx, dz) {
            share *= ring(a).max(ring(b));
        }
    }
    share
}

/// `faces` (a grid of `dims` probes) moved towards the fallback's
/// `fallback` by each texel's [`edge_share`] times `presence`: what a
/// drawn probe's image holds.
fn faded_faces(
    faces: &[[[f32; 3]; FACES]],
    dims: [usize; 3],
    fallback: &[[f32; 3]; FACES],
    drawn: u8,
    presence: f32,
) -> Vec<[[f32; 3]; FACES]> {
    let [nx, _, nz] = dims;
    faces
        .iter()
        .enumerate()
        .map(|(p, probe)| {
            let s = edge_share(p % nx, (p / nx) % nz, nx, nz, drawn) * presence;
            core::array::from_fn(|f| {
                [0, 1, 2].map(|c| fallback[f][c] + s * (probe[f][c] - fallback[f][c]))
            })
        })
        .collect()
}

/// Packs the faces of a grid of `dims` probes, `[probe][face]` in the
/// geometry's probe order, as [`pack_probes`] does.
fn pack_faces(faces: &[[[f32; 3]; FACES]], dims: [usize; 3]) -> Vec<u8> {
    let [nx, ny, nz] = dims;
    let (width, height) = (nx, 2 * ny);
    let mut out = vec![0u8; nx * 2 * ny * 3 * nz * TEXEL_BYTES];
    for y in 0..ny {
        for z in 0..nz {
            for x in 0..nx {
                let probe = &faces[(y * nz + z) * nx + x];
                for (face, rgb) in probe.iter().enumerate() {
                    let axis = face / 2;
                    let positive = face % 2 == 0;
                    let t = y + if positive { 0 } else { ny };
                    let p = z + axis * nz;
                    let texel = (p * height + t) * width + x;
                    let at = texel * TEXEL_BYTES;
                    for c in 0..3 {
                        out[at + 2 * c..at + 2 * c + 2]
                            .copy_from_slice(&f16_bits(rgb[c]).to_le_bytes());
                    }
                    out[at + 6..at + 8].copy_from_slice(&f16_bits(1.0).to_le_bytes());
                }
            }
        }
    }
    out
}

/// The 3D image for packed texels of a grid of `dims` probes.
fn volume_image(dims: [usize; 3], texels: Vec<u8>) -> Image {
    Image::new(
        Extent3d {
            width: dims[0] as u32,
            height: 2 * dims[1] as u32,
            depth_or_array_layers: 3 * dims[2] as u32,
        },
        TextureDimension::D3,
        texels,
        TextureFormat::Rgba16Float,
        RenderAssetUsages::RENDER_WORLD,
    )
}

// ---- the ECS side ---------------------------------------------------------

/// A region's irradiance probe: on the child of the region entity that
/// carries its `IrradianceVolume`.
#[derive(Component, Debug, Clone)]
pub struct RegionProbe {
    /// The region's horizons and ground, kept for re-packing.
    pub geometry: Arc<ProbeGeometry>,
    /// Its `LightProbe` falloff, put back when it is drawn again.
    pub falloff: Vec3,
    /// Its faces under the light it was last packed under, before any
    /// fade: what its image is written from when it is drawn.
    faces: Arc<Vec<[[f32; 3]; FACES]>>,
    /// Whether [`draw_nearest`] ranked it in last frame.
    wanted: bool,
    /// How far it has faded in, 0 to 1.
    presence: f32,
    /// What its image was last written for; `None` when `faces` changed.
    written: Option<Written>,
}

/// What a drawn probe's image was written for: the drawn neighbours, the
/// fade's step and the fallback's light.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Written {
    drawn: u8,
    step: u8,
    fallback: u64,
}

/// The fallback's six faces, which a drawn probe fades to at its undrawn
/// sides, and a count of their changes.
#[derive(Resource, Debug, Clone, Copy, Default)]
struct FallbackFaces {
    faces: [[f32; 3]; FACES],
    generation: u64,
}

/// The single ambient cube of open sky the whole view falls back on. See
/// the module docs, "The sky fallback".
#[derive(Component, Debug, Clone, Copy, Default)]
pub struct SkyFallbackProbe;

/// How much of the sun the cloud layer lets through, which the client
/// copies from the atmosphere's cloud layer (`strata-atmosphere`'s
/// `clouds::SunVeil` and `SkySettings::cloud_cover`; this crate does not
/// see that one). The bounce is the sun's light off the ground, so under a
/// cloud's shadow it goes with the sun: without this a wall in a cloud's
/// shadow took a bright warm bounce from ground the bake took for sunlit.
/// The shadow is a kilometre across and the region probes reach 200 m,
/// so the camera's own share stands for theirs; the fallback, which
/// stands for ground anywhere in view, takes the sky's clear share.
#[derive(Resource, Debug, Clone, Copy, PartialEq)]
pub struct ProbeClouds {
    /// The share of the sun's disc the cloud layer lets through as seen
    /// from the camera, 0 to 1.
    pub sun_clear: f32,
    /// The share of the sky the layer leaves clear, 0 to 1.
    pub mean_clear: f32,
}

impl Default for ProbeClouds {
    fn default() -> Self {
        Self {
            sun_clear: 1.0,
            mean_clear: 1.0,
        }
    }
}

/// Counters for the log line and the tests.
#[derive(Resource, Debug, Clone, Copy, PartialEq, Default)]
pub struct ProbeStats {
    /// Region bakes finished since the start.
    pub baked: u64,
    /// Their summed wall time on the pool, milliseconds.
    pub bake_ms_total: f64,
    /// The longest one, milliseconds.
    pub bake_ms_max: f64,
    /// Bakes on the pool now.
    pub in_flight: usize,
    /// Re-packs of every live probe for a new sun.
    pub repacks: u64,
    /// The last re-pack's wall time on the pool, milliseconds.
    pub repack_ms: f64,
    /// Images written for drawn probes: a new neighbour, a fade's step, a
    /// re-pack or a new fallback.
    pub writes: u64,
}

/// The light the live probes were last packed under, and what was asked
/// for since.
#[derive(Resource, Debug, Clone, Default)]
struct ProbeLight {
    /// The lights now.
    current: Lighting,
    /// The lights the probes were packed under.
    packed: Lighting,
    /// A bake landed packed under other lights than the last re-pack's
    /// (it was dispatched before the light moved), so re-pack anyway.
    stale: bool,
    /// The cloud layer's clear share of the sky, for the fallback.
    mean_clear: f32,
}

impl ProbeLight {
    /// The lighting the fallback is packed under: the current lights with
    /// the layer's mean clear share, since it stands for ground anywhere
    /// in view, not only under the camera.
    fn fallback(&self) -> Lighting {
        Lighting {
            sun_clear: self.mean_clear,
            ..self.current.clone()
        }
    }
}

/// A region bake's result.
struct Baked {
    geometry: Arc<ProbeGeometry>,
    faces: Vec<[[f32; 3]; FACES]>,
    ms: f64,
    /// The lights it was packed under.
    lights: Lighting,
}

/// A re-pack's result: each probe's new faces, and its wall time.
type Repacked = (Vec<(Entity, Vec<[[f32; 3]; FACES]>)>, f64);

/// The camera's horizontal view direction, for ranking the drawn probes.
#[derive(Resource, Debug, Clone, Copy, Default)]
struct ProbeFocus {
    ahead: [f64; 2],
}

#[derive(Resource, Default)]
struct ProbeTasks {
    bakes: HashMap<Entity, Task<Baked>>,
    repack: Option<Task<Repacked>>,
}

/// Registers the bake, the collector, the re-pack and the fallback, all
/// idle without `Assets<Image>` (a headless app).
pub fn register(app: &mut App) {
    app.init_resource::<ProbeStats>()
        .init_resource::<ProbeLight>()
        .init_resource::<ProbeClouds>()
        .init_resource::<ProbeFocus>()
        .init_resource::<ProbeTasks>()
        .init_resource::<FallbackFaces>()
        .add_systems(
            Update,
            (
                read_lights,
                (dispatch_bakes, collect_bakes, repack, draw_nearest).chain(),
            )
                .chain()
                .after(TerrainSet::Collect)
                .run_if(in_state(AppState::InWorld)),
        )
        .add_systems(Update, log_probes)
        .add_systems(
            PostUpdate,
            (
                follow_camera.before(TransformSystems::Propagate),
                bound_clusters
                    .after(VisibilitySystems::CheckVisibility)
                    .before(SimulationLightSystems::AssignLightsToClusters),
            ),
        );
}

/// The directional lights as the probes see them, in a stable order. A
/// light on a render layer the world is not drawn on (a screen's model
/// stage) lights nothing a probe stands in and is left out.
fn read_lights(
    clouds: Res<ProbeClouds>,
    lights: Query<(&DirectionalLight, &GlobalTransform, Option<&RenderLayers>)>,
    cameras: Query<(&Camera, &EnvironmentMapLight), (With<Camera3d>, Without<crate::OffWorld>)>,
    mut state: ResMut<ProbeLight>,
) {
    let world = RenderLayers::default();
    let mut now: Vec<SkyLight> = lights
        .iter()
        .filter(|(light, _, layers)| {
            light.illuminance > 0.0 && layers.is_none_or(|layers| layers.intersects(&world))
        })
        .map(|(light, transform, _)| {
            let to_light = transform.back().as_vec3();
            let colour = light.color.to_linear();
            SkyLight {
                to_light: to_light.to_array(),
                illuminance: [colour.red, colour.green, colour.blue].map(|c| c * light.illuminance),
            }
        })
        .collect();
    now.sort_by(|a, b| b.illuminance[1].total_cmp(&a.illuminance[1]));
    let fill = cameras
        .iter()
        .filter(|(camera, _)| camera.is_active)
        .min_by_key(|(camera, _)| camera.order)
        .map_or(1.0, |(_, map)| map.intensity);
    state.current = Lighting {
        lights: now,
        fill,
        sun_clear: clouds.sun_clear,
    };
    state.mean_clear = clouds.mean_clear;
}

/// Whether `now` differs from `then` by more than the re-pack thresholds.
fn moved(now: &Lighting, then: &Lighting) -> bool {
    if now.lights.len() != then.lights.len()
        || (now.fill - then.fill).abs() > 0.01 * then.fill.max(1e-3)
        || (now.sun_clear - then.sun_clear).abs() > REPACK_CLEAR
    {
        return true;
    }
    now.lights.iter().zip(&then.lights).any(|(a, b)| {
        let dot = Vec3::from_array(a.to_light).dot(Vec3::from_array(b.to_light));
        let (ea, eb) = (a.illuminance[1], b.illuminance[1]);
        dot < REPACK_COS || (ea - eb).abs() > REPACK_ILLUMINANCE * eb.max(1.0)
    })
}

/// Marks a region whose probe is baked or baking, so it is dispatched once
/// while it stays near.
#[derive(Component, Debug, Clone, Copy, Default)]
struct ProbeRequested;

/// A bake per built LOD 0 region within [`PROBE_REACH_M`] of the primary
/// observer; the probes of regions past [`PROBE_DROP_M`] go, and a drop
/// per unload.
#[allow(
    clippy::too_many_arguments,
    reason = "one system, one pass over the regions; the inputs are its dependencies"
)]
fn dispatch_bakes(
    mut commands: Commands,
    source: Option<Res<WorldSourceRes>>,
    inputs: Option<Res<MeshInputs>>,
    images: Option<Res<Assets<Image>>>,
    observers: Res<Observers>,
    light: Res<ProbeLight>,
    regions: Query<(Entity, &Region, Option<&TerrainRegion>, Has<ProbeRequested>)>,
    probes: Query<(Entity, &RegionProbe, &ChildOf)>,
    mut tasks: ResMut<ProbeTasks>,
    mut stats: ResMut<ProbeStats>,
    mut unloads: MessageReader<RegionUnload>,
) {
    for RegionUnload(_, entity) in unloads.read() {
        tasks.bakes.remove(entity);
    }
    let (Some(source), Some(inputs), Some(_), Some(observer)) =
        (source, inputs, images, observers.primary)
    else {
        return;
    };
    for (probe, region, parent) in &probes {
        if distance_to_region(observer, region.geometry.coord) > PROBE_DROP_M {
            commands.entity(probe).despawn();
            if let Ok(mut parent) = commands.get_entity(parent.parent()) {
                parent.remove::<ProbeRequested>();
            }
        }
    }
    let pool = AsyncComputeTaskPool::get_or_init(TaskPool::new);
    for (entity, region, summary, requested) in &regions {
        let coord = region.coord;
        let Some(summary) = summary else {
            continue;
        };
        if requested
            || coord.lod > MAX_PROBE_LOD
            || tasks.bakes.contains_key(&entity)
            || distance_to_region(observer, coord) > PROBE_REACH_M
        {
            continue;
        }
        commands.entity(entity).insert(ProbeRequested);
        let probe_box = ProbeBox::over(coord, summary.min_height, summary.max_height);
        let source = source.0.clone();
        let layers = inputs.layers.clone();
        let lights = light.current.clone();
        let table = SkyTable::new(&lights);
        tasks.bakes.insert(
            entity,
            pool.spawn(async move {
                let start = Instant::now();
                let geometry = bake_probe_geometry(coord, probe_box, source.as_ref(), &layers);
                let faces = probe_faces(&geometry, &table);
                Baked {
                    geometry: Arc::new(geometry),
                    faces,
                    ms: start.elapsed().as_secs_f64() * 1e3,
                    lights,
                }
            }),
        );
    }
    stats.in_flight = tasks.bakes.len();
}

/// Finished bakes onto their regions as child probes; the fallback kept
/// in step with the probes' ground.
#[allow(
    clippy::too_many_arguments,
    reason = "one system, one pass over the tasks; the image and the entities are its outputs"
)]
fn collect_bakes(
    mut commands: Commands,
    images: Option<ResMut<Assets<Image>>>,
    mut tasks: ResMut<ProbeTasks>,
    mut stats: ResMut<ProbeStats>,
    mut light: ResMut<ProbeLight>,
    mut fallback_faces: ResMut<FallbackFaces>,
    regions: Query<(), (With<Region>, With<Transform>)>,
    probes: Query<&RegionProbe>,
    fallback: Query<(&IrradianceVolume, &Visibility), With<SkyFallbackProbe>>,
) {
    let Some(mut images) = images else {
        return;
    };
    let (intensity, visibility) = fallback
        .single()
        .map_or((1.0, Visibility::Inherited), |(v, vis)| (v.intensity, *vis));
    if fallback.is_empty() {
        let geometry = ProbeGeometry::open(DEFAULT_ALBEDO);
        let faces = probe_faces(&geometry, &SkyTable::new(&light.fallback()));
        let handle = images.add(volume_image([1, 1, 1], pack_faces(&faces, [1, 1, 1])));
        fallback_faces.faces = faces[0];
        fallback_faces.generation += 1;
        commands.spawn((
            SkyFallbackProbe,
            Transform::from_scale(Vec3::splat(FALLBACK_SIDE_M)),
            LightProbe {
                falloff: Vec3::splat(FALLBACK_FALLOFF),
            },
            IrradianceVolume {
                voxels: handle,
                intensity: 1.0,
                affects_lightmapped_meshes: true,
            },
        ));
    }
    let finished: Vec<Entity> = tasks
        .bakes
        .iter()
        .filter(|(_, task)| task.is_finished())
        .map(|(entity, _)| *entity)
        .collect();
    let mut any = false;
    for region in finished {
        let Some(mut task) = tasks.bakes.remove(&region) else {
            continue;
        };
        let Some(baked) = block_on(poll_once(&mut task)) else {
            continue;
        };
        stats.baked += 1;
        stats.bake_ms_total += baked.ms;
        stats.bake_ms_max = stats.bake_ms_max.max(baked.ms);
        if !regions.contains(region) {
            continue;
        }
        if moved(&baked.lights, &light.packed) {
            light.stale = true;
        }
        let geometry = baked.geometry;
        let b = geometry.probe_box;
        let corner = geometry.coord.min_corner();
        // Unfaded until it is drawn, when `draw_nearest` writes it.
        let handle = images.add(volume_image(
            geometry.dims(),
            pack_faces(&baked.faces, geometry.dims()),
        ));
        // Relative to the region's corner, which the region's own
        // transform places (its `WorldPosition` is the corner at height 0).
        let translation = Vec3::new(
            (b.min[0] + 0.5 * b.size[0] - corner.x()) as f32,
            (b.min[1] + 0.5 * b.size[1]) as f32,
            (b.min[2] + 0.5 * b.size[2] - corner.z()) as f32,
        );
        let side = b.size[0] as f32;
        let blend = (geometry.coord.size() * BLEND_SHARE) as f32 / side;
        let falloff = Vec3::new(blend, FOOT_M / b.size[1] as f32, blend);
        commands.spawn((
            ChildOf(region),
            Transform::from_translation(translation).with_scale(Vec3::new(
                side,
                b.size[1] as f32,
                b.size[2] as f32,
            )),
            LightProbe { falloff },
            IrradianceVolume {
                voxels: handle,
                intensity,
                affects_lightmapped_meshes: true,
            },
            visibility,
            RegionProbe {
                geometry,
                falloff,
                faces: Arc::new(baked.faces),
                wanted: false,
                presence: 0.0,
                written: None,
            },
        ));
        any = true;
    }
    stats.in_flight = tasks.bakes.len();
    if any {
        refresh_fallback(
            probes.iter().map(|p| p.geometry.mean_albedo()),
            &fallback,
            &mut images,
            &light,
            &mut fallback_faces,
        );
    }
}

/// Re-packs the fallback over the mean of `albedos`, the live probes'.
fn refresh_fallback(
    albedos: impl Iterator<Item = [f32; 3]>,
    fallback: &Query<(&IrradianceVolume, &Visibility), With<SkyFallbackProbe>>,
    images: &mut Assets<Image>,
    lights: &ProbeLight,
    faces: &mut FallbackFaces,
) {
    let Ok((volume, _)) = fallback.single() else {
        return;
    };
    let mut sum = [0.0f32; 3];
    let mut n = 0.0f32;
    for a in albedos {
        for c in 0..3 {
            sum[c] += a[c];
        }
        n += 1.0;
    }
    let albedo = if n > 0.0 {
        sum.map(|s| s / n)
    } else {
        DEFAULT_ALBEDO
    };
    let open = probe_faces(
        &ProbeGeometry::open(albedo),
        &SkyTable::new(&lights.fallback()),
    );
    // An id from a live handle is always in the collection.
    let _ = images.insert(
        volume.voxels.id(),
        volume_image([1, 1, 1], pack_faces(&open, [1, 1, 1])),
    );
    faces.faces = open[0];
    faces.generation += 1;
}

/// Re-packs every live probe on the pool when the light has moved past
/// the thresholds, and hands the new faces to [`draw_nearest`] when it is
/// done.
fn repack(
    images: Option<ResMut<Assets<Image>>>,
    mut tasks: ResMut<ProbeTasks>,
    mut stats: ResMut<ProbeStats>,
    mut light: ResMut<ProbeLight>,
    mut fallback_faces: ResMut<FallbackFaces>,
    mut probes: Query<(Entity, &mut RegionProbe)>,
    fallback: Query<(&IrradianceVolume, &Visibility), With<SkyFallbackProbe>>,
) {
    let Some(mut images) = images else {
        return;
    };
    if let Some(task) = tasks.repack.as_mut() {
        let Some((packed, ms)) = block_on(poll_once(task)) else {
            return;
        };
        tasks.repack = None;
        for (entity, faces) in packed {
            let Ok((_, mut probe)) = probes.get_mut(entity) else {
                continue;
            };
            probe.faces = Arc::new(faces);
            probe.written = None;
        }
        stats.repacks += 1;
        stats.repack_ms = ms;
        refresh_fallback(
            probes.iter().map(|(_, p)| p.geometry.mean_albedo()),
            &fallback,
            &mut images,
            &light,
            &mut fallback_faces,
        );
        return;
    }
    if !light.stale && !moved(&light.current, &light.packed) {
        return;
    }
    light.stale = false;
    light.packed = light.current.clone();
    let table = SkyTable::new(&light.current);
    let work: Vec<(Entity, Arc<ProbeGeometry>)> = probes
        .iter()
        .map(|(entity, probe)| (entity, probe.geometry.clone()))
        .collect();
    let pool = AsyncComputeTaskPool::get_or_init(TaskPool::new);
    tasks.repack = Some(pool.spawn(async move {
        let start = Instant::now();
        let packed = work
            .into_iter()
            .map(|(entity, geometry)| (entity, probe_faces(&geometry, &table)))
            .collect();
        (packed, start.elapsed().as_secs_f64() * 1e3)
    }));
}

/// The drawn mask of the region at `(x, z)`: bit `n` set when the region
/// [`NEIGHBOURS`]`[n]` away is in `lit`.
fn drawn_mask(x: i32, z: i32, lit: &HashSet<(i32, i32)>) -> u8 {
    NEIGHBOURS
        .iter()
        .enumerate()
        .filter(|(_, (dx, dz))| lit.contains(&(x + dx, z + dz)))
        .fold(0, |mask, (bit, _)| mask | (1 << bit))
}

/// Draws the [`DRAWN_REGIONS`] region probes nearest the focus ahead of
/// the observer, and writes the images of the drawn ones.
///
/// A drawn probe counts [`DRAWN_HYSTERESIS_M`] nearer than it is, so two
/// regions about as near do not swap every frame as the camera turns. One
/// ranked in fades in over [`PRESENCE_S`] and one ranked out fades out,
/// keeping its `LightProbe` until it is gone; a drawn probe's image is
/// faded to the fallback's light on its sides whose neighbours are not
/// drawn, and written again when those, its fade's step, its faces or the
/// fallback change. See the module docs, "The sky fallback".
#[allow(
    clippy::too_many_arguments,
    reason = "one system, one pass over the probes; the images and the light probes are its outputs"
)]
fn draw_nearest(
    mut commands: Commands,
    observers: Res<Observers>,
    focus: Res<ProbeFocus>,
    time: Res<bevy::time::Time>,
    fallback: Res<FallbackFaces>,
    images: Option<ResMut<Assets<Image>>>,
    mut stats: ResMut<ProbeStats>,
    mut probes: Query<(Entity, &mut RegionProbe, &IrradianceVolume, Has<LightProbe>)>,
) {
    let Some(observer) = observers.primary else {
        return;
    };
    let fx = observer.x() + focus.ahead[0] * FOCUS_AHEAD_M;
    let fz = observer.z() + focus.ahead[1] * FOCUS_AHEAD_M;
    let mut ranked: Vec<(f64, Entity)> = probes
        .iter()
        .map(|(entity, probe, _, _)| {
            let coord = probe.geometry.coord;
            let half = coord.size() * 0.5;
            let corner = coord.min_corner();
            let (dx, dz) = (corner.x() + half - fx, corner.z() + half - fz);
            let bonus = if probe.wanted {
                DRAWN_HYSTERESIS_M
            } else {
                0.0
            };
            ((dx * dx + dz * dz).sqrt() - bonus, entity)
        })
        .collect();
    ranked.sort_by(|a, b| a.0.total_cmp(&b.0).then(a.1.cmp(&b.1)));
    let wanted: HashSet<Entity> = ranked
        .iter()
        .take(DRAWN_REGIONS)
        .map(|(_, entity)| *entity)
        .collect();
    let step = time.delta_secs() / PRESENCE_S;
    let mut lit = HashSet::new();
    for (entity, mut probe, _, _) in &mut probes {
        let want = wanted.contains(&entity);
        let presence = if want {
            (probe.presence + step).min(1.0)
        } else {
            (probe.presence - step).max(0.0)
        };
        // Bit for bit: a still probe is not marked changed every frame.
        if probe.wanted != want || probe.presence.to_bits() != presence.to_bits() {
            probe.wanted = want;
            probe.presence = presence;
        }
        if want || presence > 0.0 {
            let coord = probe.geometry.coord;
            lit.insert((coord.x, coord.z));
        }
    }
    let mut images = images;
    for (entity, mut probe, volume, probed) in &mut probes {
        let coord = probe.geometry.coord;
        if !lit.contains(&(coord.x, coord.z)) {
            if probed {
                commands.entity(entity).remove::<LightProbe>();
            }
            continue;
        }
        let key = Written {
            drawn: drawn_mask(coord.x, coord.z, &lit),
            step: (probe.presence * PRESENCE_STEPS).round() as u8,
            fallback: fallback.generation,
        };
        if probe.written != Some(key)
            && let Some(images) = images.as_deref_mut()
        {
            let dims = probe.geometry.dims();
            let faces = faded_faces(
                &probe.faces,
                dims,
                &fallback.faces,
                key.drawn,
                f32::from(key.step) / PRESENCE_STEPS,
            );
            let _ = images.insert(
                volume.voxels.id(),
                volume_image(dims, pack_faces(&faces, dims)),
            );
            probe.written = Some(key);
            stats.writes += 1;
        }
        if !probed {
            commands.entity(entity).insert(LightProbe {
                falloff: probe.falloff,
            });
        }
    }
}

/// The far plane of each 3D camera's clusters: the farthest a visible
/// point light, spot light or drawn region probe reaches along the view,
/// as Bevy's default reckons it but without the fallback. See the module
/// docs, "Clustering".
fn bound_clusters(
    mut commands: Commands,
    mut cameras: Query<
        (Entity, &GlobalTransform, Option<&mut ClusterConfig>),
        (With<Camera3d>, Without<crate::OffWorld>),
    >,
    points: Query<(&GlobalTransform, &ViewVisibility, &PointLight)>,
    spots: Query<(&GlobalTransform, &ViewVisibility, &SpotLight)>,
    probes: Query<(&GlobalTransform, &ViewVisibility), (With<RegionProbe>, With<LightProbe>)>,
    fallback: Query<(), With<SkyFallbackProbe>>,
) {
    if fallback.is_empty() {
        return;
    }
    for (entity, camera, config) in &mut cameras {
        let at = camera.translation();
        let forward = camera.forward().as_vec3();
        let reach = |p: Vec3, range: f32| forward.dot(p - at) + range;
        let mut far = 0.0f32;
        for (t, seen, light) in &points {
            if seen.get() {
                far = far.max(reach(t.translation(), light.range));
            }
        }
        for (t, seen, light) in &spots {
            if seen.get() {
                far = far.max(reach(t.translation(), light.range));
            }
        }
        for (t, seen) in &probes {
            if seen.get() {
                // Bevy's own bounding radius for a light probe.
                far = far.max(reach(t.translation(), t.radius_vec3a(Vec3A::ONE)));
            }
        }
        match config {
            Some(mut config) => {
                let bounded = with_far(*config, far);
                if far_of(&config) != far_of(&bounded) {
                    *config = bounded;
                }
            }
            None => {
                commands
                    .entity(entity)
                    .insert(with_far(ClusterConfig::default(), far));
            }
        }
    }
}

/// `config` with its last depth slice ending at `far`, metres; a config
/// with no depth slices as it is.
fn with_far(config: ClusterConfig, far: f32) -> ClusterConfig {
    let bound = |z_config: ClusterZConfig| ClusterZConfig {
        far_z_mode: ClusterFarZMode::Constant(far),
        ..z_config
    };
    match config {
        ClusterConfig::FixedZ {
            total,
            z_slices,
            z_config,
            dynamic_resizing,
        } => ClusterConfig::FixedZ {
            total,
            z_slices,
            z_config: bound(z_config),
            dynamic_resizing,
        },
        ClusterConfig::XYZ {
            dimensions,
            z_config,
            dynamic_resizing,
        } => ClusterConfig::XYZ {
            dimensions,
            z_config: bound(z_config),
            dynamic_resizing,
        },
        other @ (ClusterConfig::None | ClusterConfig::Single) => other,
    }
}

/// The constant far plane of `config`, if it has one.
fn far_of(config: &ClusterConfig) -> Option<f32> {
    match config {
        ClusterConfig::FixedZ { z_config, .. } | ClusterConfig::XYZ { z_config, .. } => {
            match z_config.far_z_mode {
                ClusterFarZMode::Constant(far) => Some(far),
                ClusterFarZMode::MaxClusterableObjectRange => None,
            }
        }
        ClusterConfig::None | ClusterConfig::Single => None,
    }
}

/// Keeps the fallback on the camera, so it is the nearest volume and
/// always has one of the view's eight slots. A render-space helper, like
/// the camera it follows; a frame's lag is a few metres of a 200 km box.
fn follow_camera(
    cameras: Query<(&Camera, &GlobalTransform), (With<Camera3d>, Without<crate::OffWorld>)>,
    mut fallback: Query<&mut Transform, With<SkyFallbackProbe>>,
    mut focus: ResMut<ProbeFocus>,
) {
    let Some((_, camera)) = cameras
        .iter()
        .filter(|(camera, _)| camera.is_active)
        .min_by_key(|(camera, _)| camera.order)
    else {
        return;
    };
    let at = camera.translation();
    let forward = camera.forward();
    let flat = (forward.x * forward.x + forward.z * forward.z).sqrt();
    if flat > 0.1 {
        focus.ahead = [f64::from(forward.x / flat), f64::from(forward.z / flat)];
    }
    for mut transform in &mut fallback {
        if transform.translation != at {
            transform.translation = at;
        }
    }
}

/// Every two seconds while Bevy's render diagnostics are on (the client's
/// statistics): the probes, their bake and the opaque pass they cost in.
fn log_probes(
    diagnostics: Option<Res<DiagnosticsStore>>,
    stats: Res<ProbeStats>,
    probes: Query<(&ViewVisibility, &Visibility, Has<LightProbe>), With<RegionProbe>>,
    cameras: Query<(&Camera, &Clusters), (With<Camera3d>, Without<crate::OffWorld>)>,
    time: Res<bevy::time::Time<bevy::time::Real>>,
    mut last: Local<f64>,
) {
    let now = time.elapsed_secs_f64();
    if now - *last < 2.0 {
        return;
    }
    *last = now;
    let Some(diagnostics) = diagnostics else {
        return;
    };
    let ms = |span: &str| {
        let suffix = format!("/{span}/elapsed_gpu");
        diagnostics
            .iter()
            .find(|d| d.path().as_str().ends_with(&suffix))
            .and_then(bevy::diagnostic::Diagnostic::smoothed)
    };
    let Some(opaque) = ms("main_opaque_pass_3d") else {
        return;
    };
    // The clustering pass sorts the volumes into froxels, so it is part of
    // their cost; SSAO is a full-screen pass the probes do not touch, a
    // yardstick for the card's clock, which a lone game under Xvfb lets
    // sag and swing (the render look study, phase 2c).
    let show = |v: Option<f64>| v.map_or_else(|| "-".to_owned(), |v| format!("{v:.3}"));
    let (clustering, ssao) = (show(ms("clustering")), show(ms("ssao")));
    let live = probes.iter().count();
    let hidden = probes.iter().any(|(_, v, _)| *v == Visibility::Hidden);
    let drawn = probes.iter().filter(|(_, _, drawn)| *drawn).count();
    let in_view = probes
        .iter()
        .filter(|(v, _, drawn)| *drawn && v.get())
        .count();
    let mean = if stats.baked > 0 {
        stats.bake_ms_total / stats.baked as f64
    } else {
        0.0
    };
    // The far plane `bound_clusters` set, as Bevy's clustering used it.
    let far = cameras
        .iter()
        .filter(|(camera, _)| camera.is_active)
        .min_by_key(|(camera, _)| camera.order)
        .map_or(0.0, |(_, clusters)| clusters.far);
    tracing::info!(
        "irradiance probes: {live} live{}, {drawn} drawn, {in_view} of them in view, and the sky fallback, bakes {} at {mean:.2} ms mean {:.2} max on the pool, {} in flight, repacks {} (last {:.2} ms), {} image writes, clusters end at {far:.0} m; ms of GPU time: opaque pass {opaque:.3}, clustering {clustering}, ssao {ssao}",
        if hidden {
            " (hidden: irradiance 0)"
        } else {
            ""
        },
        stats.baked,
        stats.bake_ms_max,
        stats.in_flight,
        stats.repacks,
        stats.repack_ms,
        stats.writes,
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use strata_registry::BiomeId;

    /// Level ground at `height`, or a pit: ground at `height` within
    /// `radius` of the middle of region (0, 0) and `rim` metres higher outside it.
    struct Ground {
        height: f32,
        radius: f64,
        rim: f32,
    }

    impl WorldSource for Ground {
        fn sample(&self, x: f64, z: f64) -> Sample {
            // Centred on the middle of region (0, 0) at LOD 0.
            let (dx, dz) = (x - 32.0, z - 32.0);
            let inside = (dx * dx + dz * dz).sqrt() < self.radius;
            Sample {
                height: if inside {
                    self.height
                } else {
                    self.height + self.rim
                },
                biomes: [(BiomeId(0), 1.0), (BiomeId(0), 0.0)],
                provinces: Sample::NO_PROVINCES,
                material: 0,
                moisture: 0.5,
                hydration: 0.5,
                cover: 0.0,
                landform: strata_worldgen::Landform::default(),
                water: None,
            }
        }

        fn sea_level(&self) -> f32 {
            -100.0
        }
    }

    fn noon() -> Lighting {
        lit(vec![SkyLight {
            to_light: [0.0, 0.819_152, -0.573_576],
            illuminance: [130_000.0; 3],
        }])
    }

    fn lit(lights: Vec<SkyLight>) -> Lighting {
        Lighting {
            lights,
            ..Lighting::default()
        }
    }

    fn bake(source: &Ground) -> ProbeGeometry {
        let coord = RegionCoord::new(0, 0, 0);
        let layers = BiomeLayers::from_layers(vec![0]);
        bake_probe_geometry(
            coord,
            ProbeBox::over(coord, source.height, source.height),
            source,
            &layers,
        )
    }

    #[test]
    fn halves_round_to_nearest() {
        assert_eq!(f16_bits(1.0), 0x3c00);
        assert_eq!(f16_bits(0.5), 0x3800);
        assert_eq!(f16_bits(65_504.0), 0x7bff);
        assert_eq!(f16_bits(1e9), 0x7bff);
        assert_eq!(f16_bits(-1.0), 0);
        assert_eq!(f16_bits(f32::NAN), 0);
        // The smallest subnormal half, and a value under half of it.
        assert_eq!(f16_bits(5.960_464_5e-8), 1);
        assert_eq!(f16_bits(2e-8), 0);
        // 1 + 2^-11 is halfway between 1 and the next half; ties go even.
        assert_eq!(f16_bits(1.0 + 1.0 / 2048.0), 0x3c00);
        assert_eq!(f16_bits(1.0 + 3.0 / 2048.0), 0x3c02);
    }

    #[test]
    fn the_exponential_is_close_without_the_platform_one() {
        for i in 0..2000 {
            let x = -(i as f32) * 0.04;
            let want = x.exp();
            let got = exp_det(x);
            assert!(
                (got - want).abs() <= want * 5e-6 + 1e-30,
                "exp({x}) = {got}, want {want}"
            );
        }
        assert!((exp_det(0.0) - 1.0).abs() < f32::EPSILON);
    }

    #[test]
    fn an_even_light_is_the_same_on_every_face() {
        let basis = basis();
        for face in 0..FACES {
            let sum: f32 = basis.weights[face].iter().sum();
            assert!((sum - 1.0).abs() < 1e-5, "face {face} sums to {sum}");
        }
    }

    #[test]
    fn the_pack_follows_bevys_layout() {
        // One probe of the fallback's shape: each face's red is its index.
        let geometry = ProbeGeometry::open([1.0; 3]);
        let faces = probe_faces(&geometry, &SkyTable::new(&noon()));
        let texels = pack_probes(&geometry, &SkyTable::new(&noon()));
        // (1, 2, 3): row 0 positive, row 1 negative; layer 0 x, 1 y, 2 z.
        assert_eq!(texels.len(), 6 * TEXEL_BYTES);
        let red = |row: usize, layer: usize| {
            let at = (layer * 2 + row) * TEXEL_BYTES;
            u16::from_le_bytes([texels[at], texels[at + 1]])
        };
        for (face, rgb) in faces[0].iter().enumerate() {
            let row = usize::from(face % 2 == 1);
            assert_eq!(red(row, face / 2), f16_bits(rgb[0]), "face {face}");
        }
    }

    #[test]
    fn open_ground_sees_sky_above_and_sunlit_ground_below() {
        let geometry = bake(&Ground {
            height: 10.0,
            radius: 1e9,
            rim: 0.0,
        });
        assert!(
            geometry
                .horizons
                .iter()
                .all(|h| h.iter().all(|&s| s == 0.0))
        );
        let faces = probe_faces(&geometry, &SkyTable::new(&noon()));
        let probe = faces[0];
        let (up, down) = (probe[2], probe[3]);
        // The sky is blue over the ground's white; the ground under a noon
        // sun is brighter than the sky's light on a level surface.
        assert!(up[2] > up[0], "the sky is blue: {up:?}");
        assert!(
            down[1] > up[1],
            "the sunlit ground outshines the sky: {down:?} {up:?}"
        );
        // A side face sees half of each.
        let side = probe[0];
        for c in 0..3 {
            let mid = f32::midpoint(up[c], down[c]);
            assert!(
                (side[c] - mid).abs() < 0.05 * mid,
                "side {side:?} against {mid}"
            );
        }
    }

    #[test]
    fn a_pit_hides_the_sky_and_its_floor_the_sun() {
        let open = bake(&Ground {
            height: 0.0,
            radius: 1e9,
            rim: 0.0,
        });
        // Its rim past the vertex term's reach, so the probes take it.
        let pit = bake(&Ground {
            height: 0.0,
            radius: 100.0,
            rim: 60.0,
        });
        // The share of the sky a face of the lowest probe nearest the
        // pit's middle sees.
        let p = 3 * PROBES_X + 3;
        let seen = |g: &ProbeGeometry, face: usize| -> f32 {
            (UPPER_BAND * AZIMUTHS..DIRECTIONS)
                .map(|d| basis().weights[face][d] * sky_share(d, g.near[p], g.horizons[p]))
                .sum()
        };
        assert!(
            seen(&pit, 2) < seen(&open, 2) - 0.02,
            "less sky overhead in the pit"
        );
        assert!(
            seen(&pit, 0) < seen(&open, 0) - 0.05,
            "less sky to the side in the pit"
        );
        // A low sun from the side: the pit's floor is in shade.
        let low = vec![SkyLight {
            to_light: [0.97, 0.243, 0.0],
            illuminance: [130_000.0; 3],
        }];
        let pit_col = &bake(&Ground {
            height: 0.0,
            radius: 30.0,
            rim: 60.0,
        })
        .columns[3 * PROBES_X + 3];
        let open_col = &bake(&Ground {
            height: 0.0,
            radius: 1e9,
            rim: 0.0,
        })
        .columns[3 * PROBES_X + 3];
        let low = SkyTable::new(&lit(low));
        assert!(ground_radiance(pit_col, &low)[1] < 0.5 * ground_radiance(open_col, &low)[1]);
    }

    #[test]
    fn no_probe_is_in_the_ground_and_the_bake_is_repeatable() {
        let source = Ground {
            height: 5.0,
            radius: 20.0,
            rim: 40.0,
        };
        let a = bake(&source);
        let b = bake(&source);
        assert_eq!(a, b);
        assert_eq!(
            pack_probes(&a, &SkyTable::new(&noon())),
            pack_probes(&b, &SkyTable::new(&noon()))
        );
        assert_eq!(a.horizons.len(), PROBES_X * PROBES_Y * PROBES_Z);
        assert!(a.probe_box.min[1] < 5.0 - f64::from(FOOT_M) + 1e-9);
    }

    #[test]
    fn the_sky_dims_and_reddens_as_the_sun_sets() {
        let at = |y: f32| {
            let flat = (1.0 - y * y).sqrt();
            SkyTable::new(&lit(vec![SkyLight {
                to_light: [flat, y, 0.0],
                illuminance: [130_000.0; 3],
            }]))
        };
        let sum = |t: &SkyTable| {
            let mut s = [0.0f32; 3];
            for d in &t.sky {
                for c in 0..3 {
                    s[c] += d[c];
                }
            }
            s
        };
        let (noon, dusk, night) = (sum(&at(0.8)), sum(&at(0.05)), sum(&at(-0.2)));
        assert!(dusk[2] < noon[2]);
        assert!(dusk[0] / dusk[2] > noon[0] / noon[2], "dusk is redder");
        assert!(night.iter().all(|&c| c == 0.0));
    }

    /// Level ground with a bank `rim` metres high between `inner` and
    /// `outer` metres from the middle of region (0, 0): a gully's walls,
    /// all of them nearer the probes than the vertex term reaches.
    struct Bank {
        inner: f64,
        outer: f64,
        rim: f32,
    }

    impl WorldSource for Bank {
        fn sample(&self, x: f64, z: f64) -> Sample {
            let r = ((x - 32.0) * (x - 32.0) + (z - 32.0) * (z - 32.0)).sqrt();
            let mut sample = Ground {
                height: 0.0,
                radius: 1e9,
                rim: 0.0,
            }
            .sample(x, z);
            if r > self.inner && r < self.outer {
                sample.height = self.rim;
            }
            sample
        }

        fn sea_level(&self) -> f32 {
            -100.0
        }
    }

    #[test]
    fn walls_within_the_vertex_terms_reach_are_its_to_darken() {
        let coord = RegionCoord::new(0, 0, 0);
        let layers = BiomeLayers::from_layers(vec![0]);
        let bank = Bank {
            inner: 20.0,
            outer: 50.0,
            rim: 30.0,
        };
        let geometry = bake_probe_geometry(coord, ProbeBox::over(coord, 0.0, 30.0), &bank, &layers);
        // Every probe in the box is under 50 m from the middle, so every
        // ray step past the vertex term's reach lands on the level ground
        // beyond the bank: what the bank hides is the near horizon's, and
        // no probe takes it again.
        let p = 3 * PROBES_X + 3;
        assert!(geometry.near[p].iter().all(|&s| s > 0.3));
        for (near, whole) in geometry.near.iter().zip(&geometry.horizons) {
            for d in UPPER_BAND * AZIMUTHS..DIRECTIONS {
                let share = sky_share(d, *near, *whole);
                assert!((share - 1.0).abs() < 1e-6, "a probe took the bank: {share}");
            }
        }
        // The ground in the gully still sees the bank, for the light the
        // bounce leaves it with.
        let floor = &geometry.columns[3 * PROBES_X + 3];
        assert!(floor.sky_horizon.iter().all(|&s| s > 0.3), "{floor:?}");
        // A ridge past the reach still cuts the sky.
        let ridge = bake(&Ground {
            height: 0.0,
            radius: 100.0,
            rim: 80.0,
        });
        let p = 3 * PROBES_X + 3;
        assert!(ridge.horizons[p].iter().all(|&s| s > 0.2));
        let low = UPPER_BAND * AZIMUTHS;
        assert!(sky_share(low, ridge.near[p], ridge.horizons[p]) < 0.5);
    }

    #[test]
    fn the_near_shares_and_the_probes_multiply_to_the_whole_horizon() {
        // A bank 20 m high at 30 m and a ridge 200 m high at 130 m.
        let near = [20.0 / (20.0f32 * 20.0 + 30.0 * 30.0).sqrt(); AZIMUTHS];
        let whole = [200.0 / (200.0f32 * 200.0 + 130.0 * 130.0).sqrt(); AZIMUTHS];
        for d in UPPER_BAND * AZIMUTHS..DIRECTIONS {
            let product = band_visibility(d, near) * sky_share(d, near, whole);
            let want = band_visibility(d, whole);
            assert!(
                (product - want).abs() < 1e-6,
                "{d}: {product} against {want}"
            );
        }
        // A near wall as high as anything past it leaves the probe open.
        for d in UPPER_BAND * AZIMUTHS..DIRECTIONS {
            assert!((sky_share(d, whole, whole) - 1.0).abs() < 1e-6);
        }
    }

    #[test]
    fn an_undrawn_side_fades_to_the_fallback() {
        let dims = [PROBES_X, PROBES_Y, PROBES_Z];
        let faces = vec![[[1.0f32; 3]; FACES]; PROBES_X * PROBES_Y * PROBES_Z];
        let fallback = [[0.0f32; 3]; FACES];
        let east = 1 << 0;
        let out = faded_faces(&faces, dims, &fallback, east, 1.0);
        let at = |x: usize, y: usize, z: usize| out[(y * PROBES_Z + z) * PROBES_X + x][0][0];
        let mid = PROBES_Z / 2;
        // West, north and south are not drawn: their rings fade.
        assert!(at(0, 0, mid).abs() < 1e-6);
        assert!((at(1, 0, mid) - 0.5).abs() < 1e-6);
        assert!((at(2, 0, mid) - 1.0).abs() < 1e-6);
        assert!(at(4, 2, 0).abs() < 1e-6);
        assert!((at(4, 2, PROBES_Z - 2) - 0.5).abs() < 1e-6);
        // East is drawn: its ring keeps the probe's own light.
        assert!((at(PROBES_X - 1, 0, mid) - 1.0).abs() < 1e-6);
        // Drawn all round, nothing fades; half faded in, half of it.
        let all = faded_faces(&faces, dims, &fallback, u8::MAX, 0.5);
        assert!(all.iter().all(|p| (p[3][1] - 0.5).abs() < 1e-6));
        // Two sides drawn but not the corner between them: the corner fades.
        let north_east = (1 << 0) | (1 << 2);
        let out = faded_faces(&faces, dims, &fallback, north_east, 1.0);
        let corner = out[(PROBES_Z - 1) * PROBES_X + PROBES_X - 1][0][0];
        let edge = out[(PROBES_Z - 1) * PROBES_X + PROBES_X - 3][0][0];
        assert!(corner.abs() < 1e-6, "{corner}");
        assert!((edge - 1.0).abs() < 1e-6, "{edge}");
    }

    #[test]
    fn the_drawn_mask_names_the_neighbours() {
        let lit: HashSet<(i32, i32)> = [(0, 0), (1, 0), (-1, -1)].into_iter().collect();
        assert_eq!(drawn_mask(0, 0, &lit), (1 << 0) | (1 << 5));
        assert_eq!(drawn_mask(1, 0, &lit), 1 << 4);
    }

    #[test]
    fn the_clusters_end_where_they_are_told() {
        let bounded = with_far(ClusterConfig::default(), 240.0);
        assert_eq!(far_of(&bounded), Some(240.0));
        assert_eq!(far_of(&ClusterConfig::default()), None);
        assert!(matches!(
            with_far(ClusterConfig::Single, 240.0),
            ClusterConfig::Single
        ));
    }

    #[test]
    fn the_pair_of_azimuths_brackets_the_light() {
        let (a, b, share) = azimuth_pair(1.0, 0.0);
        assert_eq!(a, 0);
        assert!(share < 1e-6, "{a} {b} {share}");
        let (a, b, share) = azimuth_pair(0.923_88, 0.382_683);
        assert_eq!((a.min(b), a.max(b)), (0, 1));
        assert!((share - 0.5).abs() < 0.05, "{share}");
    }
}
