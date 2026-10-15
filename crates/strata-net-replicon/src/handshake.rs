//! What the two ends must share besides the protocol, and what a refused
//! client is told.
//!
//! Replicon refuses a client whose protocol hash is not the server's, and
//! says only "protocol mismatch". The hash carries more than the
//! registrations: the baked tree library's version, the world's version
//! (`strata_worldgen::world_version`), the world's seed, the combat
//! tables' version and the baked site groups' version are folded in after
//! them, so a client on other trees, other ground, other numbers or other
//! buildings on its sites is refused like an older build.
//! A player refused for that needs to hear which it was, so the server,
//! as it refuses, sends its own [`Versions`] to that client, and the
//! client compares them with its own and says in words what differs
//! ([`Refused::message`]).
//!
//! The versions arrive intact whenever the two ends registered the same
//! protocol, which is the case this is for: same build, other data. A
//! client of another build may not decode them at all, and replicon's own
//! "protocol mismatch" is then the whole story.

use bevy::ecs::prelude::*;
use bevy_replicon::prelude::{FromClient, ProtocolHash, SendTargets, ServerTriggerExt, ToClients};
use serde::{Deserialize, Serialize};

/// The versions of what both ends must share, as this process has them:
/// a resource on both ends, and the event a server sends a client it
/// refuses.
#[derive(Resource, Event, Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct Versions {
    /// The baked tree library's version, if the plugin was given one.
    pub tree_library: Option<u64>,
    /// The world's version, if the plugin was given one.
    pub world: Option<u64>,
    /// The world's seed, if the plugin was given one.
    pub seed: Option<u64>,
    /// The combat tables' version (`CombatTables::version`), if the plugin
    /// was given one.
    pub combat_tables: Option<u64>,
    /// The baked site groups' and crossings' kit's version
    /// (`strata_sim::sites::site_groups_version`, which since ticket 0005
    /// step 7 also hashes the crossings' kit and its files), if the plugin
    /// was given one. Last, as every later addition must be.
    pub site_groups: Option<u64>,
}

/// On a client the server refused, what differed. Inserted when the
/// server's [`Versions`] arrive.
#[derive(Resource, Debug, Clone, Copy, PartialEq, Eq)]
pub struct Refused {
    /// The server's versions.
    pub server: Versions,
    /// This client's versions.
    pub client: Versions,
}

impl Refused {
    /// Whether the two worlds differ.
    #[must_use]
    pub fn world_differs(&self) -> bool {
        self.server.world != self.client.world
    }

    /// Whether the two tree libraries differ.
    #[must_use]
    pub fn trees_differ(&self) -> bool {
        self.server.tree_library != self.client.tree_library
    }

    /// Whether the two seeds differ.
    #[must_use]
    pub fn seed_differs(&self) -> bool {
        self.server.seed != self.client.seed
    }

    /// Whether the two combat tables differ.
    #[must_use]
    pub fn combat_tables_differ(&self) -> bool {
        self.server.combat_tables != self.client.combat_tables
    }

    /// Whether the two ends stand other buildings on the sites.
    #[must_use]
    pub fn site_groups_differ(&self) -> bool {
        self.server.site_groups != self.client.site_groups
    }

    /// What a player is told: which of the shared things is another
    /// version, and what to do about it.
    #[must_use]
    pub fn message(&self) -> String {
        let hex = |v: Option<u64>| v.map_or_else(|| "none".to_owned(), |v| format!("{v:016x}"));
        let mut reasons = Vec::new();
        if self.world_differs() {
            reasons.push(format!(
                "this server's world is another version than yours (server {}, yours {}): the ground \
                 one of you would walk on is not the other's; update the game, or join a server of \
                 your version",
                hex(self.server.world),
                hex(self.client.world)
            ));
        }
        if self.seed_differs() {
            let seed = |v: Option<u64>| v.map_or_else(|| "none".to_owned(), |v| v.to_string());
            reasons.push(format!(
                "this server runs another seed than yours (server {}, yours {}): join with \
                 `--seed {}`",
                seed(self.server.seed),
                seed(self.client.seed),
                seed(self.server.seed)
            ));
        }
        if self.trees_differ() {
            reasons.push(format!(
                "this server's trees are another version than yours (server {}, yours {}): run \
                 `just content` or update the game",
                hex(self.server.tree_library),
                hex(self.client.tree_library)
            ));
        }
        if self.combat_tables_differ() {
            reasons.push(format!(
                "this server's combat tables are another version than yours (server {}, yours \
                 {}): update the game's assets/combat/, or join a server of your version",
                hex(self.server.combat_tables),
                hex(self.client.combat_tables)
            ));
        }
        if self.site_groups_differ() {
            reasons.push(format!(
                "this server's site buildings are another version than yours (server {}, yours \
                 {}): run `just content` or update the game",
                hex(self.server.site_groups),
                hex(self.client.site_groups)
            ));
        }
        if reasons.is_empty() {
            reasons.push(
                "this server's protocol is another version than yours: update the game".to_owned(),
            );
        }
        format!("the server refused this client: {}", reasons.join("; "))
    }
}

/// On the server, as replicon refuses a client whose hash is not ours:
/// send it our versions, so it can say what differs. Replicon's own check
/// disconnects it after this frame's sends, so the message goes out first.
pub fn explain_refusal(
    hash: On<FromClient<ProtocolHash>>,
    protocol: Res<ProtocolHash>,
    versions: Res<Versions>,
    mut commands: Commands,
) {
    if hash.message == *protocol {
        return;
    }
    tracing::info!(
        "refusing client {:?}: its protocol, trees or world are another version; telling it ours",
        hash.client_id
    );
    commands.server_trigger(ToClients {
        targets: SendTargets::Single(hash.client_id),
        message: *versions,
    });
}

/// On the client, when a refusing server's versions arrive: say in words
/// what differs, and keep it for whatever shows it.
pub fn on_server_versions(server: On<Versions>, ours: Res<Versions>, mut commands: Commands) {
    let refused = Refused {
        server: *server,
        client: *ours,
    };
    tracing::error!("{}", refused.message());
    commands.insert_resource(refused);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_refusal_says_which_thing_differs() {
        let client = Versions {
            tree_library: Some(1),
            world: Some(2),
            seed: Some(42),
            combat_tables: Some(5),
            site_groups: Some(8),
        };
        let world = Refused {
            server: Versions {
                world: Some(3),
                ..client
            },
            client,
        };
        assert!(world.world_differs() && !world.trees_differ());
        assert!(
            world
                .message()
                .contains("this server's world is another version than yours"),
            "{}",
            world.message()
        );
        let trees = Refused {
            server: Versions {
                tree_library: Some(9),
                ..client
            },
            client,
        };
        assert!(trees.message().contains("trees are another version"));
        assert!(!trees.message().contains("world"));
        let tables = Refused {
            server: Versions {
                combat_tables: Some(6),
                ..client
            },
            client,
        };
        assert!(tables.combat_tables_differ() && !tables.trees_differ());
        assert!(
            tables
                .message()
                .contains("combat tables are another version")
        );
        let sites = Refused {
            server: Versions {
                site_groups: Some(9),
                ..client
            },
            client,
        };
        assert!(sites.site_groups_differ() && !sites.world_differs());
        assert!(
            sites
                .message()
                .contains("site buildings are another version")
        );
        let protocol = Refused {
            server: client,
            client,
        };
        assert!(protocol.message().contains("protocol is another version"));
    }
}
