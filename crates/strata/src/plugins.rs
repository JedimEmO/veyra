//! Plugin groups.

use bevy::app::{PluginGroup, PluginGroupBuilder};

/// The entry points. Assumes Bevy's own plugins are already added:
/// `MinimalPlugins` (plus whatever the app wants) for headless,
/// `DefaultPlugins` for the client. `SimPlugin` adds `StatesPlugin` itself
/// when the app has not.
#[derive(Debug, Clone, Copy, Default)]
pub struct StrataPlugins;

impl StrataPlugins {
    /// The simulation and the streamer, no renderer: what a dedicated server
    /// and `game --headless` run.
    #[must_use]
    pub fn headless() -> HeadlessPlugins {
        HeadlessPlugins
    }

    /// [`Self::headless`] plus terrain, atmosphere and the camera.
    #[cfg(feature = "client")]
    #[must_use]
    pub fn client() -> ClientPlugins {
        ClientPlugins
    }
}

/// `SimPlugin`, `StreamPlugin`, `PhysicsPlugin`, then `NetPlugin`, which
/// only records that the process is `NetRole::Standalone` unless an
/// adapter (`ReplicationPlugin`, behind `net-replicon`) says otherwise.
///
/// The physics plugin registers avian, which needs the task pools (Bevy's
/// `TaskPoolPlugin`, in `MinimalPlugins`) and finishes its setup in
/// `Plugin::finish`, which `App::run` calls; a test that drives `update`
/// by hand calls `finish` and `cleanup` itself.
#[derive(Debug, Clone, Copy, Default)]
pub struct HeadlessPlugins;

impl PluginGroup for HeadlessPlugins {
    fn build(self) -> PluginGroupBuilder {
        PluginGroupBuilder::start::<Self>()
            .add(strata_sim::SimPlugin)
            .add(strata_stream::StreamPlugin)
            .add(strata_physics::PhysicsPlugin)
            .add(strata_net::NetPlugin)
    }
}

/// [`HeadlessPlugins`] then `TerrainPlugin`, `TerrainMaterialPlugin`,
/// `WaterPlugin`, `AtmospherePlugin`, `FoliagePlugin` and `ClientPlugin`.
///
/// The material plugin is added here and not by `TerrainPlugin` because it
/// needs the asset server and the renderer, and `TerrainPlugin` alone runs
/// under `MinimalPlugins` in the terrain crate's headless tests.
#[cfg(feature = "client")]
#[derive(Debug, Clone, Copy, Default)]
pub struct ClientPlugins;

#[cfg(feature = "client")]
impl PluginGroup for ClientPlugins {
    fn build(self) -> PluginGroupBuilder {
        PluginGroupBuilder::start::<Self>()
            .add_group(HeadlessPlugins)
            .add(strata_terrain::TerrainPlugin)
            .add(strata_terrain::TerrainMaterialPlugin)
            .add(strata_terrain::WaterPlugin)
            .add(strata_atmosphere::AtmospherePlugin)
            .add(strata_foliage::FoliagePlugin)
            .add(strata_client::ClientPlugin)
    }
}

#[cfg(test)]
mod smoke {
    use super::*;
    use bevy::app::App;
    use bevy::state::state::State;
    use strata_sim::AppState;

    #[test]
    fn headless_reaches_the_world() {
        let mut app = App::new();
        app.add_plugins((
            bevy::app::TaskPoolPlugin::default(),
            bevy::time::TimePlugin,
            StrataPlugins::headless(),
        ));
        app.finish();
        app.cleanup();
        // Boot, LoadingRegistries, InWorld: one state transition per frame.
        app.update();
        app.update();
        app.update();
        assert_eq!(
            *app.world().resource::<State<AppState>>().get(),
            AppState::InWorld
        );
        assert_eq!(
            *app.world().resource::<strata_net::NetRole>(),
            strata_net::NetRole::Standalone
        );
    }

    /// The material plugin registers an asset type and loads images, so
    /// the group needs an asset server and the image asset; nothing here
    /// needs a GPU.
    #[cfg(feature = "client")]
    #[test]
    fn client_group_builds() {
        use bevy::asset::AssetApp;
        let mut app = App::new();
        app.add_plugins((
            bevy::MinimalPlugins,
            bevy::asset::AssetPlugin::default(),
            bevy::input::InputPlugin,
        ));
        app.init_asset::<bevy::image::Image>();
        app.add_plugins(StrataPlugins::client());
        app.finish();
        app.cleanup();
        app.update();
    }
}
