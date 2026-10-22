//! The game's two contributions to the replication handshake, spelled once.
//!
//! The labels and their order are the contract with every older client, and
//! the game and the server both push them, so one function holds the spelling
//! and the test beside it pins the hash that spelling folds into.

use strata_net_replicon::VersionParts;

/// The combat tables' and the baked site groups' versions as handshake parts,
/// in the order every older client folded them.
#[must_use]
pub fn game_version_parts(combat_tables: u64, site_groups: u64) -> VersionParts {
    VersionParts::default()
        .with(
            "combat tables",
            combat_tables,
            "combat tables",
            "update the game's assets/combat/, or join a server of your version",
        )
        .with(
            "site groups",
            site_groups,
            "site buildings",
            "run `just content` or update the game",
        )
}

#[cfg(test)]
mod tests {
    use super::*;
    use bevy::app::App;
    use strata_net_replicon::{NoTransport, ReplicationPlugin};

    /// The folded handshake of a server built from the game's own parts. The
    /// value is the one the net adapter's pin test holds for the same inputs,
    /// so a typo in a label here moves it.
    #[test]
    fn the_games_parts_fold_to_the_pinned_handshake() {
        let mut app = App::new();
        app.insert_resource(NoTransport);
        app.insert_resource(game_version_parts(3, 4));
        app.add_plugins((
            bevy::app::TaskPoolPlugin::default(),
            bevy::time::TimePlugin,
            strata_app::AppPlugin,
            strata_stream::StreamPlugin,
            strata_physics::PhysicsPlugin,
            ReplicationPlugin::server(0)
                .with_tree_library(7)
                .with_world_version(1)
                .with_seed(42),
        ));
        app.finish();
        let hash = *app
            .world()
            .resource::<bevy_replicon::shared::protocol::ProtocolHash>();
        assert_eq!(format!("{hash:?}"), "ProtocolHash(9800350578829912043)");
    }
}
