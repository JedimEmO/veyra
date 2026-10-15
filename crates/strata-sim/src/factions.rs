//! Who holds the ground the local player is on (ticket 0012, step 1).
//!
//! The world's factions are `strata_worldgen::factions`, a pure function of
//! the seed, the placed sites and the faction data; the game publishes them
//! as [`Factions`] wherever the world exists. [`HeldHere`] is what a screen
//! reads of them: the territory at the local player and the holder of the
//! site they stand on, made on the task pool, never on a frame, because a
//! land mass no one has baked is baked by whoever asks.

use std::sync::Arc;

use bevy::app::{App, Plugin, Update};
use bevy::ecs::prelude::*;
use bevy::state::condition::in_state;
use bevy::tasks::{AsyncComputeTaskPool, Task, TaskPool, block_on, poll_once};
pub use strata_worldgen::factions::{Faction, FactionId, FactionMap, Holding, Territory};

use crate::sites::{NearSite, NearSites, SKIRT_M, SiteId};
use crate::{AppState, LocalPlayer, WorldPosition};

/// How far the player moves before [`HeldHere`] is made again, metres,
/// unless they cross onto or off a site first. Far under a holding's reach,
/// so a border is noticed within a short walk.
pub const HELD_REFRESH_M: f64 = 100.0;

/// The world's factions, published by the game once its world exists. A
/// server and a harness have none.
#[derive(Resource, Debug, Clone)]
pub struct Factions(pub Arc<FactionMap>);

/// What holds the ground the local player stands on.
#[derive(Resource, Debug, Clone, PartialEq, Default)]
pub struct HeldHere {
    /// Where it was made, `(x, z)`; `None` before the first.
    pub at: Option<[f64; 2]>,
    /// Whose ground that is, and how strongly.
    pub territory: Option<Territory>,
    /// The holder of the ground: `None` for unclaimed wilds.
    pub holder: Option<Faction>,
    /// The site the player stands on (its pad and the skirt past it).
    pub site: Option<SiteId>,
    /// The holder of that site: `None` for a site of a kind no faction
    /// holds, or no site.
    pub site_holder: Option<Faction>,
}

/// The near site whose laid ground `(x, z)` stands on: within its pad's
/// radius and the skirt past it. The place card asks this same function, so
/// the card and [`HeldHere`] never disagree about where the member is.
#[must_use]
pub fn site_at(near: &NearSites, x: f64, z: f64) -> Option<&NearSite> {
    near.sites.iter().find(|near| {
        let (dx, dz) = (x - near.site.centre[0], z - near.site.centre[1]);
        let reach = near.site.radius_m + SKIRT_M;
        dx * dx + dz * dz <= reach * reach
    })
}

/// The id of [`site_at`].
#[must_use]
pub fn standing_on(near: &NearSites, x: f64, z: f64) -> Option<SiteId> {
    site_at(near, x, z).map(|near| near.site.id)
}

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
    use crate::sites::{NearSite, ProceduralSite};
    use crate::tests::app_in_world;
    use strata_core::Seed;
    use strata_worldgen::ProceduralWorld;

    fn assets() -> std::path::PathBuf {
        std::path::PathBuf::from(concat!(env!("CARGO_MANIFEST_DIR"), "/../../assets"))
    }

    fn near_site(site: ProceduralSite) -> NearSite {
        NearSite {
            site,
            kind: String::new(),
            kind_name: String::new(),
            placement: None,
        }
    }

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
