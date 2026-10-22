//! The request kit: how a screen, a script or (later) a client asks the sim
//! to do something to an actor, and how the sim answers.
//!
//! Fourteen modules of the game's sim each grew the same shape by hand: a
//! `Vec` of requests in a resource that a screen writes in `Update`, an
//! answer message, a `consume` system in the tick that drains the `Vec` and
//! finds the one local player, and a refusal for the frame the player is not
//! there. This module is that shape once.
//!
//! # The shape
//!
//! * A [`Request`] names its answer message and says what a request with no
//!   actor to act on is refused with ([`Request::no_actor`]).
//! * An [`Inbox`] holds requests in two queues. A caller pushes to the
//!   *pending* queue, with [`Inbox::push`] (the request is for the local
//!   player) or [`Inbox::push_for`] (for a named [`Actor`]). Pending is a
//!   resource and not a message because a screen writes it in `Update`
//!   while the tick may not run that frame or may run twice, and a request
//!   must be consumed exactly once.
//! * [`address`] is the one system that turns a pending request into an
//!   `(Actor, R)`: it resolves the local player once per tick, and a request
//!   with nobody to act on is refused there, with [`Request::no_actor`], and
//!   goes no further. The consumer drains the *ready* queue with
//!   [`Inbox::take`] and takes the components of the actor it was handed
//!   with `get_mut(actor.0)`, never `single_mut()` over `LocalPlayer`.
//! * [`AppRequestExt::add_request`] registers the inbox, the answer message
//!   and the consumer's systems in the set the game orders them by, with
//!   [`address`] first.
//!
//! # What is not here yet
//!
//! The net arm is not built. It is a client event carrying `R` that the
//! server turns into [`Inbox::push_for`] with the actor of the connection
//! the event came from, under `net-replicon`; nothing in this module has to
//! change for it, because [`Actor`] is a plain entity and `R` is the type a
//! consumer already takes. Household state is still resources (the store,
//! the purse, the ledgers), so two actors would share one household; moving
//! that onto components is a later step.

use bevy::app::App;
use bevy::ecs::message::Message;
use bevy::ecs::prelude::*;
use bevy::ecs::schedule::{IntoScheduleConfigs, ScheduleConfigs, SystemSet};
use bevy::ecs::system::ScheduleSystem;

use crate::{LocalPlayer, SimTick};

/// The entity a request acts on. In a single-player game that is the
/// [`LocalPlayer`]; under a server it is the player entity of the
/// connection that sent the request.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Actor(pub Entity);

impl Actor {
    /// Stands in for an actor on a request that [`Request::needs_actor`]
    /// says it can do without; it is no entity's id.
    pub const NOBODY: Self = Self(Entity::PLACEHOLDER);

    /// The local player, if there is one: the one place that question is
    /// asked. Every request pushed without an actor is addressed to it.
    #[must_use]
    pub fn local(players: &Query<Entity, With<LocalPlayer>>) -> Option<Self> {
        players.single().ok().map(Self)
    }
}

/// A thing a screen or a script asks the sim to do to an actor.
pub trait Request: Send + Sync + 'static {
    /// The message the sim answers with, which
    /// [`AppRequestExt::add_request`] registers. A request kind that is
    /// answered by more than one message registers the others itself.
    type Answer: Message;

    /// Writes what a request with no actor to act on is refused with, if it
    /// gets an answer at all. A [`Commands`] and not a writer because a
    /// request may be answered on any of its kind's messages, and so a
    /// consumer whose actor lacks the components it needs can refuse the
    /// same way. [`address`] calls it for a request with no actor, so the
    /// consumer never writes that case.
    fn no_actor(&self, commands: &mut Commands);

    /// Whether the request is refused when there is no actor. A request
    /// that only changes the household's own state (what is tracked on the
    /// HUD) is answered whoever is there, and is passed on with
    /// [`Actor::NOBODY`], which names no entity, so a consumer's
    /// `get_mut(actor.0)` fails for it as it does for a missing player.
    fn needs_actor(&self) -> bool {
        true
    }
}

/// The requests not yet consumed, in the order they were made.
#[derive(Resource)]
pub struct Inbox<R: Request> {
    pending: Vec<(Option<Actor>, R)>,
    ready: Vec<(Actor, R)>,
}

impl<R: Request> Default for Inbox<R> {
    fn default() -> Self {
        Self {
            pending: Vec::new(),
            ready: Vec::new(),
        }
    }
}

impl<R: Request + std::fmt::Debug> std::fmt::Debug for Inbox<R> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Inbox")
            .field("pending", &self.pending)
            .field("ready", &self.ready)
            .finish()
    }
}

impl<R: Request> Inbox<R> {
    /// Asks for `request` on the local player's behalf.
    pub fn push(&mut self, request: R) {
        self.pending.push((None, request));
    }

    /// Asks for `request` on `actor`'s behalf; what a server does with a
    /// request that arrived from a client.
    pub fn push_for(&mut self, actor: Actor, request: R) {
        self.pending.push((Some(actor), request));
    }

    /// Whether nothing is waiting, pending or addressed.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.pending.is_empty() && self.ready.is_empty()
    }

    /// How many requests are waiting, pending or addressed.
    #[must_use]
    pub fn len(&self) -> usize {
        self.pending.len() + self.ready.len()
    }

    /// Drops everything waiting.
    pub fn clear(&mut self) {
        self.pending.clear();
        self.ready.clear();
    }

    /// The waiting requests in the order they were made, addressed ones
    /// first (they are the older).
    pub fn iter(&self) -> impl Iterator<Item = &R> {
        self.ready
            .iter()
            .map(|(_, r)| r)
            .chain(self.pending.iter().map(|(_, r)| r))
    }

    /// Whether `request` is waiting.
    #[must_use]
    pub fn contains(&self, request: &R) -> bool
    where
        R: PartialEq,
    {
        self.iter().any(|waiting| waiting == request)
    }

    /// Every waiting request, addressed or not, in order, leaving the inbox
    /// empty; for a script or a test that reads what a screen asked.
    pub fn drain(&mut self) -> Vec<R> {
        let ready = std::mem::take(&mut self.ready);
        let pending = std::mem::take(&mut self.pending);
        ready
            .into_iter()
            .map(|(_, r)| r)
            .chain(pending.into_iter().map(|(_, r)| r))
            .collect()
    }

    /// The waiting requests as a `Vec`, for a test to compare.
    #[must_use]
    pub fn to_vec(&self) -> Vec<R>
    where
        R: Clone,
    {
        self.iter().cloned().collect()
    }

    /// Every addressed request, in order, leaving the inbox without them.
    /// What a consumer calls once a tick.
    pub fn take(&mut self) -> Vec<(Actor, R)> {
        std::mem::take(&mut self.ready)
    }

    /// The addressed requests `pick` accepts, in order, leaving the rest for
    /// the next consumer of the same inbox.
    pub fn take_if(&mut self, mut pick: impl FnMut(&R) -> bool) -> Vec<(Actor, R)> {
        self.ready.extract_if(.., |(_, r)| pick(r)).collect()
    }
}

impl<R: Request> Extend<R> for Inbox<R> {
    fn extend<I: IntoIterator<Item = R>>(&mut self, requests: I) {
        for request in requests {
            self.push(request);
        }
    }
}

/// `SimTick`: resolves the actor of every pending request, once, and
/// refuses those with none. Registered by
/// [`AppRequestExt::add_request`] ahead of the consumer.
pub fn address<R: Request>(
    mut inbox: ResMut<Inbox<R>>,
    players: Query<Entity, With<LocalPlayer>>,
    mut commands: Commands,
) {
    if inbox.pending.is_empty() {
        return;
    }
    let local = Actor::local(&players);
    let inbox = &mut *inbox;
    for (actor, request) in inbox.pending.drain(..) {
        match actor.or(local) {
            Some(actor) => inbox.ready.push((actor, request)),
            None if !request.needs_actor() => inbox.ready.push((Actor::NOBODY, request)),
            None => request.no_actor(&mut commands),
        }
    }
}

/// Registers requests on an [`App`].
pub trait AppRequestExt {
    /// Registers `R`'s inbox and answer message, and puts [`address`] and
    /// `consumer` (the systems that drain the inbox, which may be several
    /// chained) in `set`, address first. `set` is one of the sets the game
    /// orders its requests by in [`SimTick`].
    fn add_request<R: Request, M>(
        &mut self,
        set: impl SystemSet,
        consumer: impl IntoScheduleConfigs<ScheduleSystem, M>,
    ) -> &mut Self;

    /// Puts a second consumer of an inbox another set registered in `set`,
    /// which the game must order after that one. For a request enum that
    /// two modules answer a part each of.
    fn add_request_consumer<M>(
        &mut self,
        set: impl SystemSet,
        consumer: impl IntoScheduleConfigs<ScheduleSystem, M>,
    ) -> &mut Self;
}

impl AppRequestExt for App {
    fn add_request<R: Request, M>(
        &mut self,
        set: impl SystemSet,
        consumer: impl IntoScheduleConfigs<ScheduleSystem, M>,
    ) -> &mut Self {
        self.init_resource::<Inbox<R>>().add_message::<R::Answer>();
        let first: ScheduleConfigs<ScheduleSystem> = address::<R>.into_configs();
        self.add_systems(SimTick, (first, consumer.after(address::<R>)).in_set(set))
    }

    fn add_request_consumer<M>(
        &mut self,
        set: impl SystemSet,
        consumer: impl IntoScheduleConfigs<ScheduleSystem, M>,
    ) -> &mut Self {
        self.add_systems(SimTick, consumer.in_set(set))
    }
}

#[cfg(test)]
mod tests {
    #![allow(
        clippy::unwrap_used,
        reason = "a test that unwraps an Err fails, which is the point"
    )]

    use bevy::ecs::message::Messages;
    use bevy::ecs::system::RunSystemOnce;

    use super::*;

    #[derive(Debug, Clone, PartialEq, Eq)]
    enum Ask {
        Needs(u8),
        Free,
    }

    #[derive(Message, Debug, Clone, PartialEq, Eq)]
    struct Refused(u8);

    impl Request for Ask {
        type Answer = Refused;

        fn no_actor(&self, commands: &mut Commands) {
            if let Self::Needs(n) = self {
                commands.write_message(Refused(*n));
            }
        }

        fn needs_actor(&self) -> bool {
            matches!(self, Self::Needs(_))
        }
    }

    fn world() -> World {
        let mut world = World::new();
        world.init_resource::<Inbox<Ask>>();
        world.init_resource::<Messages<Refused>>();
        world
    }

    fn address_once(world: &mut World) {
        world.run_system_once(address::<Ask>).unwrap();
    }

    #[test]
    fn a_request_is_addressed_to_the_local_player() {
        let mut world = world();
        let player = world.spawn(LocalPlayer).id();
        world.resource_mut::<Inbox<Ask>>().push(Ask::Needs(1));
        address_once(&mut world);
        let taken = world.resource_mut::<Inbox<Ask>>().take();
        assert_eq!(taken, vec![(Actor(player), Ask::Needs(1))]);
        assert!(world.resource::<Inbox<Ask>>().is_empty());
    }

    #[test]
    fn a_request_for_a_named_actor_keeps_it() {
        let mut world = world();
        let local = world.spawn(LocalPlayer).id();
        let other = world.spawn_empty().id();
        let inbox = &mut world.resource_mut::<Inbox<Ask>>();
        inbox.push_for(Actor(other), Ask::Needs(2));
        inbox.push(Ask::Needs(3));
        address_once(&mut world);
        let taken = world.resource_mut::<Inbox<Ask>>().take();
        assert_eq!(
            taken,
            vec![(Actor(other), Ask::Needs(2)), (Actor(local), Ask::Needs(3))]
        );
    }

    #[test]
    fn with_nobody_there_a_request_is_refused_once_where_it_is_addressed() {
        let mut world = world();
        world.resource_mut::<Inbox<Ask>>().push(Ask::Needs(7));
        address_once(&mut world);
        assert!(world.resource::<Inbox<Ask>>().is_empty());
        let heard: Vec<_> = world.resource_mut::<Messages<Refused>>().drain().collect();
        assert_eq!(heard, vec![Refused(7)]);
    }

    #[test]
    fn a_request_that_needs_no_actor_goes_through_with_nobody() {
        let mut world = world();
        world.resource_mut::<Inbox<Ask>>().push(Ask::Free);
        address_once(&mut world);
        let taken = world.resource_mut::<Inbox<Ask>>().take();
        assert_eq!(taken, vec![(Actor::NOBODY, Ask::Free)]);
    }

    #[test]
    fn take_if_leaves_what_it_does_not_pick() {
        let mut world = world();
        world.spawn(LocalPlayer);
        let inbox = &mut world.resource_mut::<Inbox<Ask>>();
        inbox.push(Ask::Free);
        inbox.push(Ask::Needs(4));
        address_once(&mut world);
        let mut inbox = world.resource_mut::<Inbox<Ask>>();
        let first = inbox.take_if(|ask| *ask == Ask::Free);
        assert_eq!(first.len(), 1);
        assert_eq!(inbox.take().len(), 1);
    }
}
