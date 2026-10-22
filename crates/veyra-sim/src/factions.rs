//! The adapter half of the `factions` rules: its requests, systems and plugin.

pub use veyra_rules::factions::*;

use std::sync::Arc;

use bevy::app::{App, Plugin, Update};

use bevy::ecs::prelude::*;

use bevy::state::condition::in_state;

use bevy::tasks::{AsyncComputeTaskPool, Task, TaskPool, block_on, poll_once};

pub use veyra_world::factions::{Faction, FactionId, FactionMap, Holding, Territory};

use crate::sites::NearSites;

use crate::{AppState, LocalPlayer, WorldPosition};

/// Registers [`HeldHere`] and its refresh. Added by `SimPlugin`; with no
/// [`Factions`] it touches nothing.
#[derive(Debug, Clone, Copy)]
pub struct FactionsPlugin;

impl Plugin for FactionsPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<HeldHere>().add_systems(
            Update,
            refresh_held_here.run_if(in_state(AppState::InWorld)),
        );
    }
}

/// The reading being made, and where it is made.
type Making = Task<HeldHere>;

/// Makes [`HeldHere`] again, on the task pool, once the local player has
/// moved [`HELD_REFRESH_M`] from where it was made or crossed onto or off a
/// site, and publishes it when it is done.
pub fn refresh_held_here(
    factions: Option<Res<Factions>>,
    near: Option<Res<NearSites>>,
    player: Query<&WorldPosition, With<LocalPlayer>>,
    mut held: ResMut<HeldHere>,
    mut making: Local<Option<Making>>,
) {
    if let Some(task) = making.as_mut() {
        let Some(made) = block_on(poll_once(task)) else {
            return;
        };
        *making = None;
        *held = made;
    }
    let Some(factions) = factions else {
        return;
    };
    let Ok(position) = player.single() else {
        if *held != HeldHere::default() {
            *held = HeldHere::default();
        }
        return;
    };
    let (x, z) = (position.0.x(), position.0.z());
    let site = near.as_deref().and_then(|near| standing_on(near, x, z));
    let moved = held.at.is_none_or(|[hx, hz]| {
        let (dx, dz) = (x - hx, z - hz);
        dx * dx + dz * dz > HELD_REFRESH_M * HELD_REFRESH_M
    });
    if !moved && site == held.site {
        return;
    }
    let map = Arc::clone(&factions.0);
    *making = Some(
        AsyncComputeTaskPool::get_or_init(TaskPool::new).spawn(async move {
            let territory = map.territory(x, z);
            HeldHere {
                at: Some([x, z]),
                territory: Some(territory),
                holder: territory.holder.and_then(|id| map.faction(id)),
                site,
                site_holder: site
                    .and_then(|id| map.holder_of(&id))
                    .and_then(|id| map.faction(id)),
            }
        }),
    );
}

#[cfg(test)]
mod tests {
    #![allow(
        clippy::unwrap_used,
        reason = "a test that unwraps a None fails, which is the point"
    )]

    use super::*;

    use crate::tests::app_in_world;
    use bevy::app::App;
    use std::sync::Arc;
    use strata_core::Seed;
    use strata_worldgen::ProceduralWorld;

    use crate::sites::NearSites;

    use crate::LocalPlayer;
    use crate::WorldPosition;

    pub(crate) use veyra_rules::factions::test_support::*;

    /// Updates until the task pool has made a reading of `at`.
    fn settle(app: &mut App, at: [f64; 2]) -> HeldHere {
        for _ in 0..4_000 {
            app.update();
            let held = app.world().resource::<HeldHere>().clone();
            if held.at == Some(at) {
                return held;
            }
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        panic!("no reading of {at:?}");
    }

    /// A player who walks from Port Serein to a hamlet far outside Pelagic's
    /// reach is first on Pelagic Works' ground and then on the hamlet's
    /// holder's, standing on the hamlet's site, which that holder holds.
    #[test]
    fn held_here_follows_a_moved_player_from_one_holder_to_another() {
        let registries = Arc::new(strata_registry::load_from_dir(assets()).expect("the assets"));
        let world = Arc::new(ProceduralWorld::new(Seed(42), &registries).with_own_water());
        world.hydrology().set_threads(4);
        let city = world.sites()[0].centre;
        world.prepare_water(city[0], city[1], 25_000.0);
        let map = Arc::new(FactionMap::new(
            Arc::clone(&world),
            Seed(42),
            Arc::clone(&registries),
        ));
        let sites = world.sites_near(city[0], city[1], 20_000.0);
        let hamlet = sites
            .iter()
            .find(|site| {
                let (dx, dz) = (site.centre[0] - city[0], site.centre[1] - city[1]);
                dx * dx + dz * dz > 14_000.0_f64.powi(2)
                    && registries
                        .sites
                        .get(site.kind())
                        .is_some_and(|def| def.id.to_string() == "strata:hamlet")
            })
            .expect("a hamlet far from the city")
            .clone();
        let mut app = app_in_world();
        app.insert_resource(Factions(Arc::clone(&map)))
            .insert_resource(NearSites {
                sites: vec![near_site(hamlet.clone())],
                around: Vec::new(),
            });
        let player = app
            .world_mut()
            .spawn((LocalPlayer, WorldPosition::new(city[0], 0.0, city[1])))
            .id();

        let first = settle(&mut app, city);
        assert_eq!(first.holder.as_ref().unwrap().name, "Pelagic Works");
        assert_eq!(first.site, None);
        assert_eq!(first.site_holder, None);

        let to = hamlet.centre;
        *app.world_mut().get_mut::<WorldPosition>(player).unwrap() =
            WorldPosition::new(to[0], 0.0, to[1]);
        let second = settle(&mut app, to);
        assert_eq!(second.site, Some(hamlet.id));
        let holder = second.site_holder.as_ref().expect("a hamlet is held");
        assert_ne!(holder.name, "Pelagic Works");
        assert_eq!(second.holder.as_ref().unwrap().id, holder.id);
        assert_eq!(map.holder_of(&hamlet.id), Some(holder.id));
    }
}
