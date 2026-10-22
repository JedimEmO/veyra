//! The engine client's half of the ride view: the easing system. The state the
//! engine client and the game's combat view both read (the combat camera's
//! marker and boom, the shake switch, the ride's blend, the baked bike index
//! and the body tint) is `veyra_sim::view`, re-exported here so the paths stay.

use bevy::prelude::*;
use veyra_sim::ride::Riding;
use veyra_sim::{LocalPlayer, Registries};

pub use veyra_sim::view::{BikeModels, BodyTint, CameraShake, CombatCamera, Look, RideView};

/// `Update`: the camera's pull follows whether the local player rides.
pub(crate) fn follow_ride(
    time: Res<Time>,
    registries: Option<Res<Registries>>,
    players: Query<Option<&Riding>, With<LocalPlayer>>,
    mut view: ResMut<RideView>,
) {
    let riding = players.single().ok().flatten();
    if let Some(def) = riding
        .zip(registries.as_deref())
        .and_then(|(riding, registries)| registries.0.vehicles.get(&riding.vehicle))
    {
        view.distance = def.view.camera_distance;
        view.eye = def.view.camera_eye;
        view.hover_m = def.view.hover_m;
        view.seat = Vec3::from(def.view.seat);
        view.look = Look::of(&def.view);
    }
    view.ease(riding.is_some(), time.delta_secs());
}

/// Registers the view and its easing; the orbit camera reads it. Needs no
/// assets, so a client over a headless app has it too, with or without the
/// combat view.
pub(crate) fn install_ride(app: &mut App) {
    app.init_resource::<RideView>()
        .add_systems(Update, follow_ride);
}
