//! Compatibility cleanup for live prepass changes in Bevy 0.19.1.

use bevy::app::App;
use bevy::core_pipeline::prepass::{
    BackgroundMotionVectorsBindGroup, BackgroundMotionVectorsPipelineId, MotionVectorPrepass,
    NoBackgroundMotionVectors,
};
use bevy::ecs::prelude::*;
use bevy::render::{Render, RenderApp, RenderSystems};

pub(crate) fn register(app: &mut App) {
    if let Some(render) = app.get_sub_app_mut(RenderApp) {
        render.add_systems(
            Render,
            clear_disabled_background_motion_vectors
                .after(RenderSystems::Prepare)
                .before(RenderSystems::Render),
        );
    }
}

// Bevy's prepare systems only visit enabled views. They do not remove these
// render-world components when MotionVectorPrepass disappears or a view opts
// out. The prepass still draws any attached pipeline, even into a depth-only
// pass. Clear the stale draw state after preparation and before encoding.
fn clear_disabled_background_motion_vectors(
    mut commands: Commands,
    views: Query<
        (
            Entity,
            Has<MotionVectorPrepass>,
            Has<NoBackgroundMotionVectors>,
        ),
        With<BackgroundMotionVectorsPipelineId>,
    >,
) {
    for (entity, motion, disabled) in &views {
        if !motion || disabled {
            commands.entity(entity).remove::<(
                BackgroundMotionVectorsPipelineId,
                BackgroundMotionVectorsBindGroup,
            )>();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bevy::render::render_resource::CachedRenderPipelineId;

    fn verify_before_render(
        views: Query<
            (Has<MotionVectorPrepass>, Has<NoBackgroundMotionVectors>),
            With<BackgroundMotionVectorsPipelineId>,
        >,
    ) {
        assert!(views.iter().all(|(motion, disabled)| motion && !disabled));
    }

    #[test]
    fn disabling_motion_or_opting_out_removes_stale_pipeline_but_preserves_active_views() {
        let mut app = App::new();
        app.add_schedule(Render::base_schedule())
            .add_systems(
                Render,
                clear_disabled_background_motion_vectors
                    .after(RenderSystems::Prepare)
                    .before(RenderSystems::Render),
            )
            .add_systems(Render, verify_before_render.in_set(RenderSystems::Render));
        let pipeline = || BackgroundMotionVectorsPipelineId(CachedRenderPipelineId::INVALID);
        let active = app
            .world_mut()
            .spawn((pipeline(), MotionVectorPrepass))
            .id();
        let removed = app.world_mut().spawn(pipeline()).id();
        let opted_out = app
            .world_mut()
            .spawn((pipeline(), MotionVectorPrepass, NoBackgroundMotionVectors))
            .id();
        app.world_mut().run_schedule(Render);
        assert!(
            app.world()
                .get::<BackgroundMotionVectorsPipelineId>(active)
                .is_some()
        );
        for entity in [removed, opted_out] {
            assert!(
                app.world()
                    .get::<BackgroundMotionVectorsPipelineId>(entity)
                    .is_none()
            );
        }
        app.world_mut()
            .entity_mut(active)
            .remove::<MotionVectorPrepass>();
        app.world_mut().run_schedule(Render);
        assert!(
            app.world()
                .get::<BackgroundMotionVectorsPipelineId>(active)
                .is_none()
        );
        // Bevy prepares fresh draw state when motion is enabled again.
        app.world_mut()
            .entity_mut(active)
            .insert((MotionVectorPrepass, pipeline()));
        app.world_mut().run_schedule(Render);
        assert!(
            app.world()
                .get::<BackgroundMotionVectorsPipelineId>(active)
                .is_some()
        );
    }
}
