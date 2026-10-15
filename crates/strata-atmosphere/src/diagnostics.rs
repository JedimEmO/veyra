//! What the frame costs, for the statistics line: whole-camera GPU timing,
//! a GPU span over the sun's shadow cascades, and the batch sets each
//! binned phase draws.
//!
//! Bevy 0.19.1 scopes most passes with its own GPU timers but not the
//! cascades: `view_shadow_pass` opens its render passes with no diagnostic
//! span, and at the city the cascades are the largest GPU item
//! (`docs/perf/profile-2026-10-03.md`, section 2). The batch sets are the
//! draw calls that cost the CPU under multi-draw indirect, one bin
//! unpacking bind group and dispatch per set per view per frame (section
//! 3 of the profile); they live in the render world, so a shared slot
//! carries them to the main world's log.

use std::sync::{Arc, Mutex, PoisonError};

use bevy::app::App;
use bevy::core_pipeline::core_3d::{AlphaMask3d, Opaque3d};
use bevy::core_pipeline::mip_generation::experimental::depth::early_downsample_depth;
use bevy::core_pipeline::prepass::node::{early_prepass, late_prepass};
use bevy::core_pipeline::prepass::{AlphaMask3dPrepass, Opaque3dPrepass};
use bevy::core_pipeline::schedule::{Core3d, camera_driver};
use bevy::ecs::prelude::*;
use bevy::ecs::system::NonSendMarker;
use bevy::pbr::{EARLY_SHADOW_PASS, Shadow, per_view_shadow_pass, shared_shadow_pass};
use bevy::render::render_phase::{BinnedPhaseItem, ViewBinnedRenderPhases};
use bevy::render::{
    Render, RenderApp, RenderSystems,
    diagnostic::RecordDiagnostics,
    renderer::{RenderContext, RenderGraph, RenderGraphSystems},
};

/// The span over the sun's cascades in Bevy's render diagnostics, read as
/// `render/.../sun_shadows/elapsed_gpu`.
pub const SUN_SHADOWS_SPAN: &str = "sun_shadows";

/// Batch sets per binned phase, summed over that phase's views: the camera
/// for the four main phases, every cascade for the shadow phase.
///
/// A set is one pipeline and one material bind group, drawn with one
/// multi-draw; bins Bevy could not multi-draw are counted one set each, as
/// each is a draw of its own.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct BatchSets {
    /// Opaque main pass.
    pub opaque: usize,
    /// Alpha-masked main pass (leaf cards, blades).
    pub alpha_mask: usize,
    /// Opaque depth, normal and motion prepass.
    pub prepass: usize,
    /// Alpha-masked prepass.
    pub prepass_alpha_mask: usize,
    /// Shadow casters over every cascade.
    pub shadow: usize,
    /// Shadow views (cascades, and any point or spot light faces).
    pub shadow_views: usize,
}

impl BatchSets {
    /// Every set drawn in a frame.
    #[must_use]
    pub fn total(&self) -> usize {
        self.opaque + self.alpha_mask + self.prepass + self.prepass_alpha_mask + self.shadow
    }
}

/// The render world's counts as the main world reads them: the last
/// frame's batch sets. Inserted in both worlds over one slot.
#[derive(Resource, Clone, Default)]
pub struct RenderCounts {
    shared: Arc<Mutex<BatchSets>>,
}

impl RenderCounts {
    /// The batch sets of the last frame the render world prepared.
    #[must_use]
    pub fn batch_sets(&self) -> BatchSets {
        // A poisoned lock only means a render frame panicked mid-write; the
        // counts are still plain numbers.
        *self.shared.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

pub(crate) fn register(app: &mut App) {
    let counts = RenderCounts::default();
    app.insert_resource(counts.clone());
    if let Some(render) = app.get_sub_app_mut(RenderApp) {
        render
            .insert_resource(counts)
            .add_systems(
                RenderGraph,
                (begin.before(camera_driver), end.after(camera_driver))
                    .in_set(RenderGraphSystems::Render),
            )
            // After batching has settled every phase's sets for the frame.
            .add_systems(
                Render,
                count_batch_sets.in_set(RenderSystems::PrepareBindGroups),
            )
            .add_systems(
                Core3d,
                (
                    // The early shadow pass runs between the prepass's
                    // indirect parameters and the depth downsample, and
                    // nothing orders it against the prepass itself; the
                    // span puts it after the prepass so the prepass is not
                    // inside it. The late passes run only with occlusion
                    // culling, which no camera here has.
                    begin_shadows
                        .after(early_prepass)
                        .before(per_view_shadow_pass::<EARLY_SHADOW_PASS>)
                        .before(shared_shadow_pass::<EARLY_SHADOW_PASS>),
                    end_shadows
                        .after(per_view_shadow_pass::<EARLY_SHADOW_PASS>)
                        .after(shared_shadow_pass::<EARLY_SHADOW_PASS>)
                        .before(early_downsample_depth)
                        .before(late_prepass),
                ),
            );
    }
}

// Exclusive systems keep both ends on the render schedule's calling thread.
// Bevy's recorder pairs spans by thread, even when encoders differ. Each
// nested system applies its command buffer before the next graph operation.
fn begin(world: &mut World) {
    world
        .run_system_cached(begin_span)
        .expect("GPU timing start");
}

fn end(world: &mut World) {
    world.run_system_cached(end_span).expect("GPU timing end");
}

fn begin_span(mut ctx: RenderContext) {
    if let Some(recorder) = ctx.diagnostic_recorder() {
        recorder.begin_time_span(ctx.command_encoder(), "camera_frame".into());
    }
}

fn end_span(mut ctx: RenderContext) {
    if let Some(recorder) = ctx.diagnostic_recorder() {
        recorder.end_time_span(ctx.command_encoder());
    }
}

// Not exclusive, as in `local_fog`: `Core3d` applies its systems' command
// buffers at its end in system order, while an exclusive system's nested
// run would apply at once, ahead of the pass it brackets. The marker keeps
// both ends on the thread running the schedule, which the recorder pairs
// spans by.
fn begin_shadows(mut ctx: RenderContext, _thread: NonSendMarker) {
    if let Some(recorder) = ctx.diagnostic_recorder() {
        recorder.begin_time_span(ctx.command_encoder(), SUN_SHADOWS_SPAN.into());
    }
}

fn end_shadows(mut ctx: RenderContext, _thread: NonSendMarker) {
    if let Some(recorder) = ctx.diagnostic_recorder() {
        recorder.end_time_span(ctx.command_encoder());
    }
}

/// Sets in every view of one phase, and how many views it has.
fn sets<P: BinnedPhaseItem>(phases: Option<&ViewBinnedRenderPhases<P>>) -> (usize, usize) {
    phases.map_or((0, 0), |phases| {
        let sets = phases
            .0
            .values()
            .map(|phase| {
                phase.multidrawable_meshes.len()
                    + phase.batchable_meshes.len()
                    + phase.unbatchable_meshes.len()
            })
            .sum();
        (sets, phases.0.len())
    })
}

/// A handful of map lengths a frame, so it runs every frame and the log
/// reads the last one.
fn count_batch_sets(
    counts: Res<RenderCounts>,
    opaque: Option<Res<ViewBinnedRenderPhases<Opaque3d>>>,
    alpha_mask: Option<Res<ViewBinnedRenderPhases<AlphaMask3d>>>,
    prepass: Option<Res<ViewBinnedRenderPhases<Opaque3dPrepass>>>,
    prepass_alpha_mask: Option<Res<ViewBinnedRenderPhases<AlphaMask3dPrepass>>>,
    shadow: Option<Res<ViewBinnedRenderPhases<Shadow>>>,
) {
    let (shadow, shadow_views) = sets(shadow.as_deref());
    let now = BatchSets {
        opaque: sets(opaque.as_deref()).0,
        alpha_mask: sets(alpha_mask.as_deref()).0,
        prepass: sets(prepass.as_deref()).0,
        prepass_alpha_mask: sets(prepass_alpha_mask.as_deref()).0,
        shadow,
        shadow_views,
    };
    *counts.shared.lock().unwrap_or_else(PoisonError::into_inner) = now;
}
