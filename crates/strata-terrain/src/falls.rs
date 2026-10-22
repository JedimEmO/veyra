//! Falls drawn (ticket 0005, step 8; ticket 0004, step 11): a sheet of
//! white water down every fall the network marks, foam at its lip and in
//! the water below, spray near, and a white mark to [`DRAW_M`].
//!
//! # What a fall is on the ground
//!
//! The network marks a stretch between two of a reach's vertices as a
//! fall ([`Fall`]): its lip and its foot, the river's width and the way
//! down. The carve grades that stretch like any other: a straight ramp
//! from the lip's level to the foot's, the channel cut into it at the
//! river's width and depth, so the ground has no overhang and no plunge
//! pool. Most falls are creeks a metre and a half to three metres wide on
//! a ramp of one in four to one in one over 60 to 200 metres; the river
//! mesh ([`crate::river`]) draws that ramp as steep foamed water. The sheet
//! is drawn over it, leaving nothing in the ground to change.
//!
//! # What is built
//!
//! One small mesh per fall, in metres from its lip, built on the main
//! thread from the record and the network's own water levels (no sample,
//! no bake), a few each frame, nearest first:
//!
//! - **The sheet.** A ribbon from the lip to the foot, a little wider than
//!   the channel so its edges sink into the banks. It leaves the lip on the
//!   arc water thrown over an edge takes and then stands off the face by a
//!   metre or so (more for a taller fall), bulging across, so it reads as
//!   water falling clear down the face and not as a film on the ramp. Its
//!   texture coordinate down the fall is the water's time of travel, so
//!   the streaks the shader scrolls by it speed up as the water falls.
//! - **Foam** on the water: a patch upstream of the lip, where the river
//!   gathers and whitens over the edge, running on [`LIP_OVER_M`] past it
//!   under the sheet's first metre so the two do not meet on a line, and a
//!   wider one round the foot, where the water lands, at the network's own
//!   surface there. Where a patch reaches past the channel it lies under
//!   the banks, which hide it.
//! - **Spray.** Camera-facing puffs round the foot that rise, grow and
//!   fade on their own cycles, as many as the drop warrants; drawn as a
//!   second entity whose material writes no depth, so the puffs do not cut
//!   each other's soft edges.
//!
//! # At distance
//!
//! A creek's sheet is two or three metres wide, under a pixel from a few
//! kilometres. The vertex shader widens it across to at least
//! [`FallConfig::least_pixels`] and lowers its opacity by the share it was
//! widened, no further than [`FallConfig::far_opacity`], so a fall is a
//! thin white mark on its hillside to [`DRAW_M`] and fades out over the
//! last [`FADE_M`]. The coarse far ground can stand a few metres over the
//! true channel the sheet hangs in; the shader draws a far sheet nearer the
//! camera along its own ray by a share of its distance
//! ([`FallConfig::depth_pull`]), which moves nothing on the screen and
//! keeps it in front of the ground it stands on. The share follows the
//! ground's error: a region's grid spacing grows with its distance (about
//! a 160th of it at the coarsest a node is drawn), so a wall of slope one
//! stands up to about 0.3% of the distance over a channel it does not
//! resolve, and a grazing ray meets it further out still. Only the sheet
//! is pulled: the foam is meant to sink under the banks, and the spray
//! stands clear of the ground.
//!
//! # The order of the transparent draws
//!
//! The river mesh writes its depth over the ramp, and so do the sheet and
//! the foam. Two depth-writing transparent meshes are drawn right only one
//! way round: the water under the foam first. The transparent pass sorts by
//! each mesh's centre, and a fall's and a region's are unrelated points, so
//! the river's material sorts itself before every blended thing
//! ([`crate::river::RIVER_SORT_BIAS`]) and the foam and the sheet always
//! blend over it. The spray writes no depth and sorts after its own body
//! ([`SPRAY_SORT_BIAS`]), so the sheet hides a puff behind it.

// Mesh indices and counts are small; positions are metres within a fall's
// reach, well inside f32.
#![allow(
    clippy::cast_possible_truncation,
    clippy::cast_precision_loss,
    clippy::cast_sign_loss,
    reason = "vertex counts and fall-local metres are small and exact in every width used"
)]
// The geometry uses the conventional single letters: along and across, a
// quad's corners.
#![allow(clippy::many_single_char_names, reason = "sheet and quad geometry")]

use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

use bevy::asset::{Asset, Assets, RenderAssetUsages};
use bevy::camera::primitives::Aabb;
use bevy::camera::visibility::NoAutoAabb;
use bevy::camera::visibility::Visibility;
use bevy::color::Color;
use bevy::ecs::prelude::*;
use bevy::light::NotShadowCaster;
use bevy::material::AlphaMode;
use bevy::math::{Vec3, Vec4};
use bevy::mesh::{
    Indices, Mesh, Mesh3d, MeshVertexAttribute, MeshVertexBufferLayoutRef, PrimitiveTopology,
    VertexFormat,
};
use bevy::pbr::{
    ExtendedMaterial, MaterialExtension, MaterialExtensionKey, MaterialExtensionPipeline,
    MeshMaterial3d, StandardMaterial,
};
use bevy::reflect::Reflect;
use bevy::render::render_resource::{
    AsBindGroup, Face, RenderPipelineDescriptor, ShaderType, SpecializedMeshPipelineError,
};
use bevy::shader::ShaderRef;
use bevy::transform::components::Transform;
use strata_app::{Observers, RenderOrigin, WorldPosition};
use strata_core::WorldPos;
use strata_worldgen::hydrology::{Fall, Network};

/// The falls' shader, vertex and fragment, relative to the asset root.
pub const FALL_SHADER_PATH: &str = "shaders/fall_water.wgsl";

/// Falls are drawn within this of the observer, metres, measured to the
/// lip: the ticket's 15 to 20 km, past which a creek's fall is under a
/// pixel tall even widened.
pub const DRAW_M: f64 = 18_000.0;

/// The last metres inside [`DRAW_M`] over which a fall fades out, so its
/// range's edge is not a pop.
pub const FADE_M: f64 = 2_000.0;

/// And taken out past this, metres: a little past the fade, so an observer
/// at the edge does not spawn and despawn the same fall.
pub const DROP_M: f64 = 18_800.0;

/// The least drop a fall's record needs to be drawn, metres. The bake
/// marks a step as a fall where it drops 20 m or more, but a later pass
/// (a lake's level, the mouth's grade) can lower a lip's record to a few
/// metres over a stretch of 50 to 100: those are rapids, which the river
/// mesh foams, and a sheet over them stood on nothing.
pub const LEAST_DROP_M: f64 = 6.0;

/// The least slope a fall's record needs to be drawn, rise over run: under
/// it the stretch is rapids for the same reason.
pub const LEAST_SLOPE: f64 = 0.12;

/// The observer moves this far before the falls in range are found again,
/// metres.
pub const RESCAN_M: f64 = 250.0;

/// The most falls built and spawned in one frame: a fall's mesh is a few
/// hundred vertices and a few dozen of the network's lookups.
pub const BUILDS_PER_FRAME: usize = 6;

/// How far off the fall's line the network's level may stand from the
/// line's own and still be the same water, metres.
pub const SAME_WATER_M: f32 = 0.6;

/// How far the sheet stands off the face from half a metre past the lip,
/// metres, before its stand-off grows down the fall.
pub const LIP_STANDOFF_M: f32 = 0.3;

/// How far the lip's foam runs on past the lip, metres, fading out under
/// the sheet's first metre while the sheet fades in: the two met on a hard
/// line across the channel. Mirrored by `LIP_OVER_M` in `fall_water.wgsl`.
pub const LIP_OVER_M: f32 = 1.2;

/// How far the foam round the foot reaches up the ramp above it, as a share
/// of its reach below; the shader fades it to nothing at that end, which was
/// a straight edge across the channel. Mirrored by `FOOT_UPSTREAM` in
/// `fall_water.wgsl`.
pub const FOOT_UPSTREAM: f32 = 0.3;

/// Added to the spray's view depth in the transparent pass's sort, metres:
/// past any fall's own length, so its puffs always draw after its sheet and
/// foam, whose depth hides a puff behind them.
pub const SPRAY_SORT_BIAS: f32 = 300.0;

/// How far the shader can widen a sheet past its own half width at the end
/// of its range, metres: `least_pixels` at [`DRAW_M`] on a view as wide as
/// 70 degrees and as short as 720 pixels. The bounds are widened by it, so
/// a widened fall at the screen's edge is not culled before it fades.
#[must_use]
pub fn far_margin_m(least_pixels: f32) -> f32 {
    least_pixels * DRAW_M as f32 * 35.0_f32.to_radians().tan() / 720.0
}

/// The speed water leaves a lip at, m/s: a creek's, which sets the arc it
/// throws over the edge before it meets the face.
pub const LIP_SPEED_MPS: f32 = 2.5;

/// The share of gravity a falling sheet gains speed by: water down a face
/// is slowed by the face and the air, and the streaks read too fast at
/// free fall.
pub const FALL_ACCELERATION: f32 = 0.45 * 9.81;

/// `[part, a, b, c]`: the part ([`PART_SHEET`], [`PART_FOAM`],
/// [`PART_SPRAY`]); for the sheet the unit across axis in `a` and `b` and
/// its true half width in `c`; for a puff its corner in `a` and `b` and
/// its size in `c`.
pub const ATTRIBUTE_FALL_SHAPE: MeshVertexAttribute =
    MeshVertexAttribute::new("FallShape", 0x5c2a_71e0_0000_0011, VertexFormat::Float32x4);

/// `[a, b, c, d]` by part: the sheet's `[across -1..1, travel s, down
/// 0..1, drop m]`; foam's `[across -1..1, metres, reach m, 0 at the lip or
/// 1 at the foot]`; a puff's `[seed 0..1, cycle s, rise m, drift m]`.
pub const ATTRIBUTE_FALL_FLOW: MeshVertexAttribute =
    MeshVertexAttribute::new("FallFlow", 0x5c2a_71e0_0000_0012, VertexFormat::Float32x4);

/// The sheet, lip to foot.
pub const PART_SHEET: f32 = 0.0;
/// Foam on the water at the lip and the foot.
pub const PART_FOAM: f32 = 1.0;
/// A spray puff.
pub const PART_SPRAY: f32 = 2.0;

/// One fall's meshes as built: plain vectors, so the builder and its tests
/// never touch a GPU layout. Metres from the lip, world axes.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct FallPart {
    /// Positions; a puff's four corners all hold its centre.
    pub positions: Vec<[f32; 3]>,
    /// Unit normals; a puff's holds the way downstream, which it drifts.
    pub normals: Vec<[f32; 3]>,
    /// [`ATTRIBUTE_FALL_SHAPE`].
    pub shape: Vec<[f32; 4]>,
    /// [`ATTRIBUTE_FALL_FLOW`].
    pub flow: Vec<[f32; 4]>,
    /// Triangle list.
    pub indices: Vec<u32>,
}

impl FallPart {
    fn push(
        &mut self,
        position: [f32; 3],
        normal: [f32; 3],
        shape: [f32; 4],
        flow: [f32; 4],
    ) -> u32 {
        self.positions.push(position);
        self.normals.push(normal);
        self.shape.push(shape);
        self.flow.push(flow);
        (self.positions.len() - 1) as u32
    }

    /// The Bevy mesh, for the render world only.
    #[must_use]
    pub fn to_mesh(&self) -> Mesh {
        Mesh::new(
            PrimitiveTopology::TriangleList,
            RenderAssetUsages::RENDER_WORLD,
        )
        .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, self.positions.clone())
        .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, self.normals.clone())
        .with_inserted_attribute(ATTRIBUTE_FALL_SHAPE, self.shape.clone())
        .with_inserted_attribute(ATTRIBUTE_FALL_FLOW, self.flow.clone())
        .with_inserted_indices(Indices::U32(self.indices.clone()))
    }

    /// The box round every position, widened by `margin` metres each way:
    /// what the shader moves a vertex by (a puff's rise and size, a far
    /// sheet's widening) is not in the positions.
    #[must_use]
    pub fn bounds(&self, margin: f32) -> ([f32; 3], [f32; 3]) {
        let mut lo = [f32::INFINITY; 3];
        let mut hi = [f32::NEG_INFINITY; 3];
        for p in &self.positions {
            for k in 0..3 {
                lo[k] = lo[k].min(p[k] - margin);
                hi[k] = hi[k].max(p[k] + margin);
            }
        }
        (lo, hi)
    }
}

/// A fall's drawing: the sheet and its foam, and the spray.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct FallMesh {
    /// The sheet and the foam on the water, which write their depth.
    pub body: FallPart,
    /// The spray's puffs, which do not.
    pub spray: FallPart,
    /// How far over the foot the spray's puffs can reach, metres: their
    /// rise and their largest size.
    pub spray_reach_m: f32,
}

/// The drop of a fall's record, metres, and its slope, rise over run.
#[must_use]
pub fn drop_and_slope(fall: &Fall) -> (f64, f64) {
    let drop = fall.top[1] - fall.bottom[1];
    let run = (fall.bottom[0] - fall.top[0]).hypot(fall.bottom[2] - fall.top[2]);
    (drop, drop / run.max(1e-3))
}

/// Whether a fall is drawn as one ([`LEAST_DROP_M`], [`LEAST_SLOPE`]).
#[must_use]
pub fn is_drawn(fall: &Fall) -> bool {
    let (drop, slope) = drop_and_slope(fall);
    drop >= LEAST_DROP_M && slope >= LEAST_SLOPE
}

/// A fall by its network's key and its index in [`Network::falls`].
pub type FallKey = ([i32; 2], u32);

/// The falls drawn from `eye` (world `x`, `z`): every one [`is_drawn`]
/// within `radius_m` of its lip, nearest first, ties by key.
pub fn falls_within<'a>(
    falls: impl IntoIterator<Item = (FallKey, &'a Fall)>,
    eye: [f64; 2],
    radius_m: f64,
) -> Vec<(f64, FallKey)> {
    let mut out: Vec<(f64, FallKey)> = falls
        .into_iter()
        .filter(|(_, fall)| is_drawn(fall))
        .filter_map(|(key, fall)| {
            let d = (fall.top[0] - eye[0]).hypot(fall.top[2] - eye[1]);
            (d <= radius_m).then_some((d, key))
        })
        .collect();
    out.sort_by(|a, b| a.0.total_cmp(&b.0).then(a.1.cmp(&b.1)));
    out
}

/// A small hash to `0..1`, for the puffs' seeds and the foam's sizes: the
/// same fall always draws the same.
fn unit_hash(a: u32, b: u32) -> f32 {
    let mut h = a.wrapping_mul(0x9e37_79b9) ^ b.wrapping_mul(0x85eb_ca6b);
    h ^= h >> 15;
    h = h.wrapping_mul(0x2c1b_3c6d);
    h ^= h >> 12;
    (h & 0x00ff_ffff) as f32 / 16_777_216.0
}

/// Columns across the sheet.
const ACROSS: usize = 7;

/// The sheet's height over the lip `s` metres down a face of `slope` that
/// drops `drop` over `run`: the arc water thrown off the lip at
/// [`LIP_SPEED_MPS`] takes, or the face plus the sheet's stand-off, the
/// higher of the two.
fn sheet_height(s: f32, run: f32, slope: f32, drop: f32) -> f32 {
    let t = (s / run).clamp(0.0, 1.0);
    let arc = -0.5 * 9.81 * s * s / (LIP_SPEED_MPS * LIP_SPEED_MPS);
    // Stand-off: none at the lip, a metre or so down the face (more for a
    // taller fall, whose sheet is thicker), and down to a hand's breadth
    // where it lands.
    let most = (0.45 + 0.012 * drop).min(1.6);
    let rise = smoothstep(0.0, 0.08, t);
    let land = 1.0 - 0.85 * smoothstep(0.82, 1.0, t);
    // A creek's channel is narrower than the finest ground's spacing, so
    // the ground drawn over it stands some way over its bed: a sheet a few
    // centimetres off the face at the lip was buried for its first metres.
    let lip = LIP_STANDOFF_M * smoothstep(0.0, 0.5, s);
    let face = -slope * s + (lip + (most - LIP_STANDOFF_M) * rise) * land;
    arc.max(face)
}

fn smoothstep(e0: f32, e1: f32, x: f32) -> f32 {
    let t = ((x - e0) / (e1 - e0)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// A fall's drawing from its record and `surface`, the water's level at a
/// world `(x, z)` (the network's, [`Network::water_at`]), which places the
/// foam on the water above the lip and below the foot. `seed` varies the
/// puffs between falls.
#[must_use]
#[allow(
    clippy::too_many_lines,
    reason = "the sheet, the two foam patches and the puffs, in one pass each"
)]
pub fn build_fall_mesh(
    fall: &Fall,
    surface: impl Fn(f64, f64) -> Option<f32>,
    seed: u32,
) -> FallMesh {
    let top = fall.top;
    let (drop, _) = drop_and_slope(fall);
    let drop = drop as f32;
    let run = ((fall.bottom[0] - top[0]).hypot(fall.bottom[2] - top[2])).max(1e-3) as f32;
    let slope = drop / run;
    let dir = [fall.direction[0], fall.direction[1]];
    let across = [-dir[1], dir[0]];
    // Wider than the channel: its edges sink into the banks, and it covers
    // the river mesh's own fall cells, which stand out past the channel in
    // steps of the ground's spacing and are all foam. Narrowed to the cut,
    // those steps showed beside it as white teeth with level tops.
    let half = f32::midpoint(fall.width_m * 1.25, 0.8);
    let local =
        |s: f32, a: f32, y: f32| [dir[0] * s + across[0] * a, y, dir[1] * s + across[1] * a];
    let raw_level = |s: f32, a: f32| {
        let x = top[0] + f64::from(dir[0] * s + across[0] * a);
        let z = top[2] + f64::from(dir[1] * s + across[1] * a);
        surface(x, z).map(|level| (f64::from(level) - top[1]) as f32)
    };
    // The level on the fall's own line, and off it only where it is the
    // same water: beside the channel the nearest reach can be a tributary
    // or the channel above, metres higher, which stood a sheet of foam
    // over the bank.
    let level_at = |s: f32, a: f32, fallback: f32| {
        let centre = raw_level(s, 0.0)
            .filter(|level| (level - fallback).abs() <= 0.5 * drop.max(4.0))
            .unwrap_or(fallback);
        raw_level(s, a)
            .filter(|level| (level - centre).abs() <= SAME_WATER_M)
            .unwrap_or(centre)
    };
    let mut body = FallPart::default();

    // The sheet: stations denser near the lip, where the arc bends.
    let along = ((drop / 3.0).ceil() as usize).clamp(12, 48);
    let mut travel = 0.0_f32;
    let mut last: Option<(f32, f32)> = None;
    let mut rows = Vec::with_capacity(along + 1);
    for k in 0..=along {
        let t = (k as f32 / along as f32).powf(1.4);
        let s = t * run;
        let y = sheet_height(s, run, slope, drop);
        if let Some((ps, py)) = last {
            let path = (s - ps).hypot(y - py);
            let v0 =
                (LIP_SPEED_MPS * LIP_SPEED_MPS + 2.0 * FALL_ACCELERATION * (-py).max(0.0)).sqrt();
            let v1 =
                (LIP_SPEED_MPS * LIP_SPEED_MPS + 2.0 * FALL_ACCELERATION * (-y).max(0.0)).sqrt();
            travel += path / f32::midpoint(v0, v1);
        }
        last = Some((s, y));
        // The sheet's tangent down the fall, for its normal.
        let ds = 0.25_f32.min(run * 0.01).max(0.01);
        let dy = (sheet_height((s + ds).min(run), run, slope, drop)
            - sheet_height((s - ds).max(0.0), run, slope, drop))
            / ((s + ds).min(run) - (s - ds).max(0.0));
        // Spreading as it falls, and more where it lands, over the river's
        // fall cells round the foot, which the churn starts below.
        let width = half * (0.9 + 0.35 * t + 0.3 * smoothstep(0.75, 1.0, t));
        // Thicker in the middle: a curtain, not a film.
        let bulge = 0.25 * (y - (-slope * s)).max(0.0);
        let mut row = Vec::with_capacity(ACROSS);
        for j in 0..ACROSS {
            let u = j as f32 / (ACROSS - 1) as f32 * 2.0 - 1.0;
            let a = u * width;
            let yy = y + bulge * (1.0 - u * u);
            // The normal: across the sheet and down it, turned up and out.
            let tangent = Vec3::new(dir[0], dy, dir[1]);
            let side = Vec3::new(across[0], -2.0 * bulge * u / width.max(1e-3), across[1]);
            let mut n = side.cross(tangent).normalize_or_zero();
            if n.y < 0.0 {
                n = -n;
            }
            row.push(body.push(
                local(s, a, yy),
                n.to_array(),
                [PART_SHEET, across[0], across[1], width],
                [u, travel, t, drop],
            ));
        }
        rows.push(row);
    }
    for pair in rows.windows(2) {
        for j in 0..ACROSS - 1 {
            let (a, b, c, d) = (pair[0][j], pair[0][j + 1], pair[1][j], pair[1][j + 1]);
            body.indices.extend_from_slice(&[a, b, c, b, d, c]);
        }
    }
    // The sheet's triangles go last: one draw blends its triangles in their
    // order, and the sheet writes its depth, so drawn before the foam under
    // it, its faint first metre hid the foam running on past the lip and
    // left a hard line across the channel.
    let sheet_indices = std::mem::take(&mut body.indices);

    // Foam above the lip: the river gathering over the edge, inside the
    // channel, whose banks hid nothing of a patch as wide as the sheet.
    let lip_reach = (fall.width_m * 2.5).clamp(3.0, 9.0);
    let foam_half = 0.5 * fall.width_m + 0.2;
    let foot = (fall.bottom[1] - top[1]) as f32;
    let patch = |body: &mut FallPart,
                 s0: f32,
                 s1: f32,
                 half: f32,
                 rows: usize,
                 cols: usize,
                 at_foot: bool,
                 fallback: f32| {
        let first = body.positions.len() as u32;
        for k in 0..=rows {
            let s = s0 + (s1 - s0) * k as f32 / rows as f32;
            for j in 0..=cols {
                let u = j as f32 / cols as f32 * 2.0 - 1.0;
                let y = level_at(s, u * half, fallback) + 0.04;
                let metres = if at_foot { s - run } else { s };
                body.push(
                    local(s, u * half, y),
                    [0.0, 1.0, 0.0],
                    [PART_FOAM, across[0], across[1], half],
                    [
                        u,
                        metres,
                        if at_foot { s1 - run } else { -s0 },
                        f32::from(u8::from(at_foot)),
                    ],
                );
            }
        }
        let stride = cols as u32 + 1;
        for k in 0..rows as u32 {
            for j in 0..cols as u32 {
                let a = first + k * stride + j;
                let (b, c, d) = (a + 1, a + stride, a + stride + 1);
                body.indices.extend_from_slice(&[a, b, c, b, d, c]);
            }
        }
    };
    patch(
        &mut body, -lip_reach, LIP_OVER_M, foam_half, 4, 4, false, 0.0,
    );
    // Foam round the foot: where the water lands and churns, wider and
    // longer the taller the fall.
    let pool_reach = (drop * 0.25).clamp(5.0, 30.0);
    // The ground has no pool: the channel runs on below the foot in its
    // trench, so the churn is the channel's width and a little more.
    let pool_half = half * 1.4;
    patch(
        &mut body,
        run - FOOT_UPSTREAM * pool_reach,
        run + pool_reach,
        pool_half,
        8,
        6,
        true,
        foot,
    );

    body.indices.extend(sheet_indices);

    // Spray: puffs round the foot, as many as the drop warrants.
    let mut spray = FallPart::default();
    let puffs = ((drop / 5.0).ceil() as u32).clamp(6, 24);
    let size_most = (drop * 0.04).clamp(1.5, 5.5);
    let rise = (drop * 0.12).clamp(2.0, 14.0);
    for p in 0..puffs {
        let r = |n: u32| unit_hash(seed, p * 8 + n);
        let s = run + (r(0) - 0.25) * pool_reach * 0.7;
        let a = (r(1) * 2.0 - 1.0) * pool_half * 0.6;
        let y = level_at(s, a, foot) + 0.5 + r(2) * 1.5;
        let size = size_most * (0.55 + 0.45 * r(3));
        let cycle = 2.5 + 3.0 * r(4);
        let drift = pool_reach * (0.2 + 0.4 * r(5));
        let centre = local(s, a, y);
        let first = spray.positions.len() as u32;
        for (cx, cy) in [(-1.0, -1.0), (1.0, -1.0), (-1.0, 1.0), (1.0, 1.0)] {
            spray.push(
                centre,
                [dir[0], 0.0, dir[1]],
                [PART_SPRAY, cx, cy, size],
                [r(6), cycle, rise * (0.5 + 0.5 * r(7)), drift],
            );
        }
        spray.indices.extend_from_slice(&[
            first,
            first + 2,
            first + 1,
            first + 1,
            first + 2,
            first + 3,
        ]);
    }
    FallMesh {
        body,
        spray,
        spray_reach_m: rise + 2.0 * size_most + pool_reach,
    }
}

/// The falls' look, mirrored by `FallParams` in `fall_water.wgsl`.
#[derive(ShaderType, Reflect, Debug, Clone, Copy, PartialEq, Default)]
pub struct FallParams {
    /// Aerated water's colour, linear RGB; in `w` how dark the glassy
    /// tongue over the lip is against it.
    pub white: Vec4,
    /// Where the falls fade out, metres (start, end), in `xy`; the least
    /// width a sheet is drawn, pixels, in `z`; how far a far vertex is
    /// drawn nearer the camera, as a share of its distance, in `w`.
    pub range: Vec4,
    /// The least opacity of a widened far sheet in `x`; the spray's
    /// opacity in `y`; the foam's in `z`; `w` unused.
    pub far: Vec4,
}

/// How the falls look. Insert before [`crate::WaterPlugin`] to override.
#[derive(Resource, Debug, Clone, Copy, PartialEq)]
pub struct FallConfig {
    /// Aerated water's colour, linear RGB: white a little toward the
    /// river's blue-grey.
    pub white: [f32; 3],
    /// How dark the clear tongue over the lip is, `0..=1` of the white.
    pub tongue: f32,
    /// The least width a far sheet is drawn, pixels: a mark, not a
    /// shimmer.
    pub least_pixels: f32,
    /// The least opacity a sheet widened past its own width keeps: a fall
    /// is a landmark, and at its true share of a pixel it was gone at 5 km.
    pub far_opacity: f32,
    /// How far a far vertex is drawn nearer the camera along its ray, as a
    /// share of its distance past 150 m.
    pub depth_pull: f32,
    /// The spray's opacity at the height of a puff's cycle.
    pub spray: f32,
    /// The foam's opacity at its whitest.
    pub foam: f32,
}

impl Default for FallConfig {
    fn default() -> Self {
        Self {
            white: [0.82, 0.86, 0.88],
            tongue: 0.7,
            least_pixels: 2.0,
            far_opacity: 0.9,
            depth_pull: 0.005,
            spray: 0.25,
            foam: 0.9,
        }
    }
}

impl FallConfig {
    /// The shader's numbers.
    #[must_use]
    pub fn params(&self) -> FallParams {
        let [r, g, b] = self.white;
        FallParams {
            white: Vec4::new(r, g, b, self.tongue),
            range: Vec4::new(
                (DRAW_M - FADE_M) as f32,
                DRAW_M as f32,
                self.least_pixels,
                self.depth_pull,
            ),
            far: Vec4::new(self.far_opacity, self.spray, self.foam, 0.0),
        }
    }
}

/// The `MaterialExtension` half of [`FallMaterial`]: the falls' look, and
/// whether this is the spray's material, which writes no depth.
#[derive(Asset, AsBindGroup, Reflect, Debug, Clone)]
#[bind_group_data(FallSurfaceKey)]
pub struct FallSurface {
    /// The look.
    #[uniform(100)]
    pub fall: FallParams,
    /// The spray's material: no depth written.
    pub spray: bool,
}

/// What a [`FallSurface`]'s pipeline is specialised on.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct FallSurfaceKey {
    /// [`FallSurface::spray`].
    pub spray: bool,
}

impl From<&FallSurface> for FallSurfaceKey {
    fn from(surface: &FallSurface) -> Self {
        Self {
            spray: surface.spray,
        }
    }
}

impl MaterialExtension for FallSurface {
    fn vertex_shader() -> ShaderRef {
        FALL_SHADER_PATH.into()
    }

    fn fragment_shader() -> ShaderRef {
        FALL_SHADER_PATH.into()
    }

    /// The position, the normal and the two fall attributes; both faces,
    /// since a sheet is seen from in front and from over the lip; the body
    /// writes its depth (the module docs) and the spray does not.
    fn specialize(
        _pipeline: &MaterialExtensionPipeline,
        descriptor: &mut RenderPipelineDescriptor,
        layout: &MeshVertexBufferLayoutRef,
        key: MaterialExtensionKey<Self>,
    ) -> Result<(), SpecializedMeshPipelineError> {
        let vertex_layout = layout.0.get_layout(&[
            Mesh::ATTRIBUTE_POSITION.at_shader_location(0),
            Mesh::ATTRIBUTE_NORMAL.at_shader_location(1),
            ATTRIBUTE_FALL_SHAPE.at_shader_location(2),
            ATTRIBUTE_FALL_FLOW.at_shader_location(3),
        ])?;
        descriptor.vertex.buffers = vec![vertex_layout];
        descriptor.primitive.cull_mode = None::<Face>;
        if let Some(depth) = descriptor.depth_stencil.as_mut() {
            depth.depth_write_enabled = Some(!key.bind_group_data.spray);
            // The spray's depth bias is the sort's (`SPRAY_SORT_BIAS`), not
            // an offset of the depth it is tested at.
            depth.bias.constant = 0;
        }
        Ok(())
    }
}

/// The falls' material: the standard material extended, so the falls light
/// like the rest of the scene rather than as unlit water.
pub type FallMaterial = ExtendedMaterial<StandardMaterial, FallSurface>;

/// The two fall materials, the body's and the spray's, made on first use.
#[derive(Resource, Debug, Clone)]
pub struct FallMaterials {
    /// The sheet and the foam.
    pub body: bevy::asset::Handle<FallMaterial>,
    /// The spray.
    pub spray: bevy::asset::Handle<FallMaterial>,
}

/// A drawn fall's root, for queries and the inspector.
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq)]
pub struct DrawnFall {
    /// The network's key and the fall's index in it.
    pub key: FallKey,
    /// Vertices of its two meshes.
    pub vertices: u32,
}

/// What is drawn, and where the falls in range were last found.
#[derive(Resource, Default)]
pub struct FallsDrawn {
    roots: BTreeMap<FallKey, Entity>,
    /// The falls within [`DRAW_M`] not drawn yet, nearest first.
    pending: Vec<FallKey>,
    scanned_at: Option<[f64; 2]>,
    networks: usize,
    /// Vertices of every fall spawned so far, and the microseconds their
    /// meshes took to build: what the log line reports.
    vertices: u64,
    build_micros: u64,
    by_key: BTreeMap<[i32; 2], Arc<Network>>,
}

impl FallsDrawn {
    /// How many falls are drawn.
    #[must_use]
    pub fn drawn(&self) -> usize {
        self.roots.len()
    }
}

/// Spawns the falls within [`DRAW_M`] of the primary observer, a few a
/// frame, nearest first, and despawns those past [`DROP_M`]. The falls in
/// range are found again when the observer has moved [`RESCAN_M`] or a
/// network has been baked since; only networks already baked are read, so
/// this never waits on a bake.
#[allow(
    clippy::too_many_arguments,
    clippy::too_many_lines,
    reason = "a Bevy system's parameters are its dependencies; the scan, the despawns and the builds, in order"
)]
pub(crate) fn stream_falls(
    mut commands: Commands,
    cfg: Option<Res<crate::TerrainConfig>>,
    look: Res<FallConfig>,
    world: Option<Res<strata_app::ProceduralWorldRes>>,
    observers: Res<Observers>,
    origin: Res<RenderOrigin>,
    meshes: Option<ResMut<Assets<Mesh>>>,
    materials: Option<ResMut<Assets<FallMaterial>>>,
    held: Option<Res<FallMaterials>>,
    mut drawn: ResMut<FallsDrawn>,
    mut frame: Local<u32>,
) {
    let (Some(world), Some(eye), Some(mut meshes), Some(mut materials)) =
        (world, observers.primary, meshes, materials)
    else {
        return;
    };
    if !cfg.is_none_or(|cfg| cfg.water && cfg.falls) {
        return;
    }
    let fall_materials = if let Some(held) = held {
        held.clone()
    } else {
        let material_of = |spray: bool| FallMaterial {
            base: StandardMaterial {
                base_color: Color::WHITE,
                perceptual_roughness: 0.55,
                metallic: 0.0,
                reflectance: 0.3,
                alpha_mode: AlphaMode::Blend,
                depth_bias: if spray { SPRAY_SORT_BIAS } else { 0.0 },
                ..Default::default()
            },
            extension: FallSurface {
                fall: look.params(),
                spray,
            },
        };
        let made = FallMaterials {
            body: materials.add(material_of(false)),
            spray: materials.add(material_of(true)),
        };
        commands.insert_resource(made.clone());
        made
    };
    let here = [eye.x(), eye.z()];
    *frame = frame.wrapping_add(1);
    // A cheap check for newly baked networks twice a second or so.
    let networks = if *frame % 30 == 1 || drawn.scanned_at.is_none() {
        Some(world.0.hydrology().networks())
    } else {
        None
    };
    let moved = drawn
        .scanned_at
        .is_none_or(|at| (at[0] - here[0]).hypot(at[1] - here[1]) > RESCAN_M);
    let baked = networks.as_ref().is_some_and(|n| n.len() != drawn.networks);
    if moved || baked {
        let networks = networks.unwrap_or_else(|| world.0.hydrology().networks());
        drawn.networks = networks.len();
        drawn.scanned_at = Some(here);
        drawn.by_key = networks.iter().map(|n| (n.key(), Arc::clone(n))).collect();
        let all = networks.iter().flat_map(|n| {
            n.falls()
                .iter()
                .enumerate()
                .map(move |(i, fall)| ((n.key(), i as u32), fall))
        });
        let keep = falls_within(all, here, DROP_M);
        let keep_keys: BTreeSet<FallKey> = keep.iter().map(|(_, key)| *key).collect();
        let gone: Vec<FallKey> = drawn
            .roots
            .keys()
            .filter(|key| !keep_keys.contains(key))
            .copied()
            .collect();
        for key in gone {
            if let Some(entity) = drawn.roots.remove(&key) {
                commands.entity(entity).despawn();
            }
        }
        let pending: Vec<FallKey> = keep
            .iter()
            .filter(|(d, key)| *d <= DRAW_M && !drawn.roots.contains_key(key))
            .map(|(_, key)| *key)
            .collect();
        tracing::info!(
            "falls: {} within {:.0} km of ({:.0}, {:.0}), {} drawn, {} vertices, built in {} us",
            keep.iter().filter(|(d, _)| *d <= DRAW_M).count(),
            DRAW_M / 1000.0,
            here[0],
            here[1],
            drawn.roots.len(),
            drawn.vertices,
            drawn.build_micros,
        );
        drawn.pending = pending;
    }
    let mut built = 0;
    while built < BUILDS_PER_FRAME && !drawn.pending.is_empty() {
        let key = drawn.pending.remove(0);
        let Some(network) = drawn.by_key.get(&key.0).cloned() else {
            continue;
        };
        let Some(fall) = network.falls().get(key.1 as usize) else {
            continue;
        };
        let seed = (key.0[0] as u32).wrapping_mul(73_856_093)
            ^ (key.0[1] as u32).wrapping_mul(19_349_663)
            ^ key.1.wrapping_mul(83_492_791);
        let began = strata_core::platform::time::Instant::now();
        let mesh = build_fall_mesh(
            fall,
            |x, z| network.water_at(x, z).map(|w| w.surface_m),
            seed,
        );
        drawn.build_micros += u64::try_from(began.elapsed().as_micros()).unwrap_or(u64::MAX);
        let at = WorldPos::new(fall.top[0], fall.top[1], fall.top[2]);
        let vertices = (mesh.body.positions.len() + mesh.spray.positions.len()) as u32;
        drawn.vertices += u64::from(vertices);
        let (lo, hi) = mesh.body.bounds(far_margin_m(look.least_pixels).max(6.0));
        let (slo, shi) = mesh.spray.bounds(mesh.spray_reach_m);
        let root = commands
            .spawn((
                Name::new("fall"),
                DrawnFall { key, vertices },
                WorldPosition(at),
                Transform::from_translation(origin.to_render(at)),
                Visibility::default(),
                Mesh3d(meshes.add(mesh.body.to_mesh())),
                MeshMaterial3d(fall_materials.body.clone()),
                Aabb::from_min_max(Vec3::from_array(lo), Vec3::from_array(hi)),
                NoAutoAabb,
                NotShadowCaster,
            ))
            .with_child((
                Mesh3d(meshes.add(mesh.spray.to_mesh())),
                MeshMaterial3d(fall_materials.spray.clone()),
                Aabb::from_min_max(Vec3::from_array(slo), Vec3::from_array(shi)),
                NoAutoAabb,
                NotShadowCaster,
                Transform::default(),
                Visibility::default(),
            ))
            .id();
        drawn.roots.insert(key, root);
        built += 1;
    }
}

/// Leaving the world takes every drawn fall with it.
pub(crate) fn clear_falls(mut commands: Commands, mut drawn: ResMut<FallsDrawn>) {
    for (_, root) in std::mem::take(&mut drawn.roots) {
        commands.entity(root).despawn();
    }
    *drawn = FallsDrawn::default();
}

#[cfg(test)]
#[allow(
    clippy::float_cmp,
    reason = "the ends of the sheet and a puff's corners are written as exact values"
)]
mod tests {
    use super::*;

    /// A fall of `drop` metres over `run`, heading `+x`, its lip at
    /// `(100, 300, 50)`, three metres wide.
    fn fall(drop: f64, run: f64) -> Fall {
        Fall {
            top: [100.0, 300.0, 50.0],
            bottom: [100.0 + run, 300.0 - drop, 50.0],
            width_m: 3.0,
            direction: [1.0, 0.0],
            river: 0,
        }
    }

    /// The water's level on the fall's line: the lip's upstream, the
    /// foot's downstream, the ramp between.
    fn level(f: &Fall) -> impl Fn(f64, f64) -> Option<f32> + '_ {
        move |x, _| {
            let run = f.bottom[0] - f.top[0];
            let t = ((x - f.top[0]) / run).clamp(0.0, 1.0);
            Some((f.top[1] + (f.bottom[1] - f.top[1]) * t) as f32)
        }
    }

    /// The sheet starts on the lip's water and ends just over the foot's,
    /// and between them never dips under the face the river's surface
    /// ramps down.
    #[test]
    fn the_sheet_runs_from_the_lip_to_the_foot_over_the_face() {
        for (drop, run) in [(149.0, 125.0), (20.0, 87.5), (30.7, 72.0), (8.0, 60.0)] {
            let f = fall(drop, run);
            let mesh = build_fall_mesh(&f, level(&f), 1);
            let slope = (drop / run) as f32;
            let sheet: Vec<_> = mesh
                .body
                .positions
                .iter()
                .zip(&mesh.body.shape)
                .zip(&mesh.body.flow)
                .filter(|((_, shape), _)| shape[0] == PART_SHEET)
                .collect();
            assert!(!sheet.is_empty());
            for ((p, _), flow) in &sheet {
                let face = -slope * p[0];
                assert!(p[1] >= face - 1e-3, "{drop} m: {p:?} under the face {face}");
                assert!(p[1] <= face + 3.0, "{drop} m: {p:?} far off the face");
                assert!((0.0..=1.0).contains(&flow[2]));
            }
            let lip = sheet.iter().filter(|(_, flow)| flow[2] == 0.0);
            for ((p, _), _) in lip {
                assert!(p[0].abs() < 1e-3 && p[1].abs() < 0.3, "the lip {p:?}");
            }
            let foot = sheet.iter().filter(|(_, flow)| flow[2] == 1.0);
            for ((p, _), _) in foot {
                assert!((p[0] - run as f32).abs() < 1e-2, "the foot {p:?}");
                assert!((p[1] + drop as f32).abs() < 0.5, "the foot {p:?} at {drop}");
            }
            // The water's time of travel grows down the fall.
            let mut times: Vec<(f32, f32)> =
                sheet.iter().map(|(_, flow)| (flow[2], flow[1])).collect();
            times.sort_by(|a, b| a.0.total_cmp(&b.0));
            assert!(times.windows(2).all(|w| w[1].1 >= w[0].1 - 1e-6));
            assert!(times.last().is_some_and(|t| t.1 > 1.0), "{times:?}");
        }
    }

    /// Every triangle of the sheet and the foam faces up, counter-clockwise
    /// from above as the terrain's do: wound the other way, the shader
    /// lit their backs, turned to the ground, and the foam drew dark green.
    #[test]
    fn the_body_faces_up() {
        let f = fall(149.0, 125.0);
        let mesh = build_fall_mesh(&f, level(&f), 3);
        for t in mesh.body.indices.chunks(3) {
            let [a, b, c] =
                [t[0], t[1], t[2]].map(|i| Vec3::from_array(mesh.body.positions[i as usize]));
            let n = (b - a).cross(c - a);
            assert!(n.y > 0.0, "a triangle facing down: {a} {b} {c}");
        }
    }

    /// The foam lies on the water the surface gives, a few centimetres
    /// over it, at the lip and round the foot.
    #[test]
    fn the_foam_lies_on_the_water() {
        let f = fall(60.0, 80.0);
        let mesh = build_fall_mesh(&f, level(&f), 2);
        let surface = level(&f);
        let mut lip = 0;
        let mut foot = 0;
        for ((p, shape), flow) in mesh
            .body
            .positions
            .iter()
            .zip(&mesh.body.shape)
            .zip(&mesh.body.flow)
        {
            if shape[0] != PART_FOAM {
                continue;
            }
            let water = surface(f.top[0] + f64::from(p[0]), f.top[2] + f64::from(p[2]))
                .unwrap_or(0.0)
                - f.top[1] as f32;
            assert!((p[1] - water - 0.04).abs() < 1e-3, "{p:?} against {water}");
            if flow[3] > 0.5 {
                foot += 1;
                assert!(p[0] > 60.0, "the foot's foam {p:?}");
            } else {
                lip += 1;
                assert!(p[0] <= LIP_OVER_M + 1e-3, "the lip's foam {p:?}");
            }
        }
        assert!(lip > 0 && foot > lip);
    }

    /// The puffs are quads round the foot, four corners at one centre, each
    /// standing half a metre to two over the water where it stands (some
    /// are up the ramp above the foot), and the same fall always draws the
    /// same.
    #[test]
    fn the_spray_stands_round_the_foot() {
        let f = fall(100.0, 100.0);
        let surface = level(&f);
        for seed in 0..64 {
            let a = build_fall_mesh(&f, level(&f), seed);
            assert_eq!(a, build_fall_mesh(&f, level(&f), seed));
            assert_ne!(a.spray, build_fall_mesh(&f, level(&f), seed + 1).spray);
            assert_eq!(a.spray.positions.len() % 4, 0);
            assert_eq!(a.spray.indices.len(), a.spray.positions.len() / 4 * 6);
            for quad in a.spray.positions.chunks(4) {
                let p = quad[0];
                assert!(quad.iter().all(|q| q == &p));
                assert!(p[0] > 70.0 && p[0] < 130.0, "{p:?}");
                let water = surface(f.top[0] + f64::from(p[0]), f.top[2] + f64::from(p[2]))
                    .unwrap_or(0.0)
                    - f.top[1] as f32;
                assert!(
                    p[1] >= water + 0.5 - 1e-3 && p[1] <= water + 2.0 + 1e-3,
                    "seed {seed}: {p:?} over the water at {water}"
                );
            }
            let (lo, hi) = a.spray.bounds(a.spray_reach_m);
            assert!(hi[1] - lo[1] > a.spray_reach_m * 2.0);
        }
    }

    /// Beside a creek the nearest water can be another reach, metres
    /// higher, and over its foot a reach far above it: the foam and the
    /// puffs keep to the fall's own water, as if neither were there.
    #[test]
    fn the_foam_keeps_to_its_own_water() {
        let f = fall(60.0, 80.0);
        let line = level(&f);
        let other = |x: f64, z: f64| {
            let own = line(x, z)?;
            if x > f.bottom[0] + 1.0 && (z - f.top[2]).abs() < 0.5 {
                // A reach far over the foot's water on the fall's own line.
                Some(own + 200.0)
            } else if (z - f.top[2]).abs() > 1.0 {
                // A higher reach beside the channel.
                Some(own + 8.0)
            } else {
                Some(own)
            }
        };
        let guarded = build_fall_mesh(&f, other, 5);
        assert_eq!(guarded, build_fall_mesh(&f, level(&f), 5));
        // The surface did reach the patches' sides: the guard was needed.
        let widest = guarded
            .body
            .positions
            .iter()
            .zip(&guarded.body.shape)
            .filter(|(_, shape)| shape[0] == PART_FOAM)
            .map(|(p, _)| p[2].abs())
            .fold(0.0_f32, f32::max);
        assert!(widest > 1.0, "{widest}");
    }

    /// Rapids are not falls: a record lowered to a few metres, or one too
    /// gentle, is left to the river mesh; the rest are found nearest first
    /// within the radius.
    #[test]
    fn only_falls_within_reach_are_drawn_nearest_first() {
        let tall = fall(149.0, 125.0);
        let low = fall(3.5, 87.0);
        let gentle = fall(8.0, 160.0);
        let moved = |x: f64| {
            let mut f = fall(20.0, 80.0);
            f.top[0] = x;
            f.bottom[0] = x + 80.0;
            f
        };
        let (far, near) = (moved(40_100.0), moved(0.0));
        assert!(is_drawn(&tall) && is_drawn(&near));
        assert!(!is_drawn(&low) && !is_drawn(&gentle));
        let all = [
            (([0, 0], 0), &tall),
            (([0, 0], 1), &low),
            (([0, 0], 2), &gentle),
            (([1, 0], 0), &far),
            (([1, 0], 1), &near),
        ];
        let found = falls_within(all, [0.0, 50.0], DRAW_M);
        let keys: Vec<FallKey> = found.iter().map(|(_, key)| *key).collect();
        assert_eq!(keys, vec![([1, 0], 1), ([0, 0], 0)]);
    }
}
