//! The network port. The slice implements none of it; `strata-net-replicon`
//! is the adapter (PLAN Phase 4b).
//!
//! It exists so the boundary is drawn before anything leans across it
//! (PLAN principle 4): the simulation talks to an authority, the authority
//! may talk over a [`Transport`], the entities that would cross the wire
//! are marked [`Replicated`], every process knows which side of the wire
//! it is on through [`NetRole`], and what the wire is doing is a
//! [`NetStatus`] the adapter keeps current. Nothing here sends a byte: a
//! build without the adapter is [`NetRole::Standalone`] and runs the local
//! sim exactly as before, which is what keeps the headless facade free of
//! a transport dependency.

use bevy::app::{App, Plugin};
use bevy::ecs::component::Component;
use bevy::ecs::resource::Resource;

/// A byte-message channel between one client and the server.
///
/// Deliberately minimal: framing, ordering and reliability are the
/// adapter's business, and the protocol on top is defined when the first
/// adapter lands. Unused in the slice.
pub trait Transport: Send + Sync + 'static {
    /// The error an adapter reports.
    type Error: core::error::Error + Send + Sync + 'static;

    /// Queue one message for the other end.
    fn send(&mut self, bytes: &[u8]) -> Result<(), Self::Error>;

    /// The next received message, or `None` when nothing is waiting.
    fn recv(&mut self) -> Result<Option<Vec<u8>>, Self::Error>;
}

/// Marks an entity whose sim state the server replicates to clients.
///
/// The port's vocabulary, not the adapter's: game code marks a player with
/// this and the adapter maps it onto whatever its library wants. Without an
/// adapter it is a plain marker and nothing crosses the wire.
#[derive(Component, Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Replicated;

/// Which side of the wire this process is, as a resource.
///
/// Read by systems that behave differently on a server (spawn a player per
/// connection) and a client (predict the local player) and by nothing else:
/// the simulation itself never looks. [`NetPlugin`] inserts
/// [`Standalone`](Self::Standalone) when no adapter said otherwise, so a
/// build without one runs the local sim exactly as it did before there was
/// a port.
#[derive(Resource, Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum NetRole {
    /// One process, the local sim is the truth: `game` without `--connect`.
    #[default]
    Standalone,
    /// Owns the simulation and serves it: `apps/server`.
    Server,
    /// Renders a replicated simulation and predicts its own player:
    /// `game --connect`.
    Client,
}

impl NetRole {
    /// Whether this process decides outcomes itself.
    #[must_use]
    pub fn is_authoritative(self) -> bool {
        !matches!(self, Self::Client)
    }
}

/// What the wire looks like right now, for a HUD or a log.
///
/// Written by the adapter and read by the client's overlay, so the overlay
/// can say "client, 18 ms, three players in sight" without depending on
/// the adapter's library (the HUD sits in a render crate; the adapter is
/// behind a feature the HUD cannot see). A standalone process never
/// writes it: every field stays at its default and the role says the
/// rest.
#[derive(Resource, Debug, Clone, Copy, PartialEq, Default)]
pub struct NetStatus {
    /// Whether the link to the other end is up: a client past the
    /// handshake, a server that is listening.
    pub connected: bool,
    /// The round trip the transport measures, in milliseconds. Zero when
    /// the backend does not say.
    pub rtt_ms: f32,
    /// Players this process knows of: the clients connected to a server,
    /// the replicated players a client currently sees, its own included.
    pub players: u32,
    /// How many times a client's predicted player was put back where the
    /// server said it was. Zero on a server. A rising count is the number
    /// a player feels as a hitch, so it belongs on the overlay.
    pub corrections: u32,
}

/// Registers the [`NetRole`] and the [`NetStatus`], and nothing else.
///
/// Inserts [`NetRole::Standalone`] unless a role is already there, so an
/// adapter added before or after this plugin wins either way: the adapter
/// inserts its role in its own `build`, and this one never overwrites.
#[derive(Debug, Clone, Copy, Default)]
pub struct NetPlugin;

impl Plugin for NetPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<NetRole>().init_resource::<NetStatus>();
    }
}

#[cfg(test)]
mod smoke {
    use super::*;

    /// A loopback that hands every sent message straight back, enough to
    /// show the port is usable by an adapter.
    #[derive(Default)]
    struct Loopback(std::collections::VecDeque<Vec<u8>>);

    impl Transport for Loopback {
        type Error = core::convert::Infallible;

        fn send(&mut self, bytes: &[u8]) -> Result<(), Self::Error> {
            self.0.push_back(bytes.to_vec());
            Ok(())
        }

        fn recv(&mut self) -> Result<Option<Vec<u8>>, Self::Error> {
            Ok(self.0.pop_front())
        }
    }

    #[test]
    fn the_plugin_keeps_a_role_inserted_by_an_adapter() {
        let mut app = App::new();
        app.add_plugins(NetPlugin);
        assert_eq!(*app.world().resource::<NetRole>(), NetRole::Standalone);
        assert_eq!(*app.world().resource::<NetStatus>(), NetStatus::default());
        assert!(NetRole::Standalone.is_authoritative());
        assert!(!NetRole::Client.is_authoritative());

        let mut app = App::new();
        app.insert_resource(NetRole::Server);
        app.add_plugins(NetPlugin);
        assert_eq!(*app.world().resource::<NetRole>(), NetRole::Server);
    }

    #[test]
    fn loopback_round_trips() {
        let mut t = Loopback::default();
        t.send(b"hello").expect("infallible");
        assert_eq!(t.recv().expect("infallible"), Some(b"hello".to_vec()));
        assert_eq!(t.recv().expect("infallible"), None);
    }
}
