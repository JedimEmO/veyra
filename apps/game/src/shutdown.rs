//! Lets every shader pipeline still compiling finish before the app ends.
//!
//! Bevy compiles pipelines on the async compute pool, and each compile task
//! holds clones of the `PipelineCache`'s layout and shader caches and of the
//! render device. When the app exits with one in flight, the render world
//! drops the cache and cancels the task, but a cancelled task is dropped by
//! the pool thread that next picks it up, and a running one first finishes
//! its compile. Either way the last reference to the layout cache goes on a
//! pool thread, which then destroys pipeline layouts while the main thread
//! is inside the driver's own teardown: a SIGSEGV in
//! `libnvidia-gpucomp` after "exiting", on `destroy_pipeline_layout`. The
//! pool is a process-wide static that is never joined, so nothing in Bevy
//! orders the two.
//!
//! A compile only reaches the exit when it is slow, which is why this shows
//! on a loaded machine: the probe and cloud bakes share the same pool, and
//! a mesh pipeline specialized in the last frames of a short capture
//! (`pbr_opaque_mesh_pipeline`, `pbr_prepass_pipeline` in the logs) is
//! still compiling at frame 404. Waiting for every compile on the exit
//! frame, before the render world drops, keeps all GPU work ahead of the
//! teardown.

use bevy::app::{App, AppExit};
use bevy::ecs::message::Messages;
use bevy::ecs::prelude::*;
use bevy::log::info;
use bevy::render::render_resource::{
    CachedPipelineState, CachedRenderPipelineId, PipelineCache, PipelineDescriptor,
};
use bevy::render::{Extract, ExtractSchedule, Render, RenderApp, RenderSystems};

/// Whether the main world asked to exit this frame, as the render world
/// sees it at extraction.
#[derive(Resource, Default)]
struct Exiting(bool);

/// Adds the wait to the render app; a headless app has none and needs none.
pub fn install(app: &mut App) {
    let Some(render_app) = app.get_sub_app_mut(RenderApp) else {
        return;
    };
    render_app
        .init_resource::<Exiting>()
        .add_systems(ExtractSchedule, note_exit)
        .add_systems(Render, finish_pipelines.in_set(RenderSystems::Cleanup));
}

/// Any `AppExit` message ends the app after this update, whoever wrote it
/// (the frame limit, a closed window, Ctrl-C), so its presence is the whole
/// signal.
fn note_exit(exits: Extract<Res<Messages<AppExit>>>, mut exiting: ResMut<Exiting>) {
    exiting.0 = !exits.is_empty();
}

/// Blocks on every compile still in flight. It runs after the frame's
/// `process_queue`, so nothing queued this frame starts later; a pipeline
/// only queued has no task and is dropped without touching the device.
fn finish_pipelines(exiting: Res<Exiting>, mut cache: ResMut<PipelineCache>) {
    if !exiting.0 {
        return;
    }
    let creating: Vec<(usize, String)> = cache
        .pipelines()
        .enumerate()
        .filter(|(_, p)| matches!(p.state, CachedPipelineState::Creating(_)))
        .map(|(id, p)| (id, label(&p.descriptor)))
        .collect();
    if creating.is_empty() {
        return;
    }
    let names: Vec<&str> = creating.iter().map(|(_, name)| name.as_str()).collect();
    info!(
        "waiting for {} pipeline compiles before exiting: {}",
        creating.len(),
        names.join(", ")
    );
    for (id, _) in creating {
        // `block_on_render_pipeline` only indexes the cache's pipeline list
        // and awaits the task there, so it serves a compute pipeline's id as
        // well; there is no compute twin.
        cache.block_on_render_pipeline(CachedRenderPipelineId::new(id));
    }
}

/// The pipeline's label, so the log says which compile the exit caught.
fn label(descriptor: &PipelineDescriptor) -> String {
    let label = match descriptor {
        PipelineDescriptor::RenderPipelineDescriptor(d) => d.label.as_deref(),
        PipelineDescriptor::ComputePipelineDescriptor(d) => d.label.as_deref(),
    };
    label.unwrap_or("unlabelled").to_owned()
}
