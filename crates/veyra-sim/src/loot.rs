//! The adapter half of the `loot` rules: its requests, systems and plugin.

pub use veyra_rules::loot::*;

use bevy::app::{App, Plugin};

use bevy::ecs::message::MessageWriter;

use bevy::ecs::prelude::*;

use crate::deposits::{Purse, resync};

use crate::items::{Hands, HouseholdStore, OpenStock, Pack, StockAnswer};

use crate::order::RequestSet;

use crate::progression::{InteractionKind, Reach};

use crate::{AppRequestExt, Inbox, Registries, Request};

/// What a screen asks of a cache.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CacheRequest {
    /// Empty the cache of the camp `key`, beside the player, into the pack.
    Take {
        /// The camp's key.
        key: u64,
    },
}

/// The requests not yet consumed.
pub type CacheInbox = Inbox<CacheRequest>;

/// With nobody beside the cache the take is refused as such.
impl Request for CacheRequest {
    type Answer = CacheAnswer;

    fn no_actor(&self, commands: &mut Commands) {
        let Self::Take { key } = self;
        commands.write_message(CacheAnswer {
            key: *key,
            outcome: Err(CacheRefusal::NoPlayer),
        });
    }
}

/// The sim's answer to a [`CacheRequest`].
#[derive(Message, Debug, Clone, PartialEq, Eq)]
pub struct CacheAnswer {
    /// The cache asked of.
    pub key: u64,
    /// What happened, or why nothing did.
    pub outcome: Result<CacheTaken, CacheRefusal>,
}

/// `SimTick`, `SimSet::Input`: every take asked for since the last tick.
/// Credits go to the purse, records into the pack as far as they fit; an
/// emptied cache is removed.
#[allow(
    clippy::too_many_arguments,
    reason = "the request, the cache, the reach, the household's purse and stock in, an answer and the stock's mirror out"
)]
pub fn consume(
    mut inbox: ResMut<CacheInbox>,
    registries: Res<Registries>,
    reach: Res<Reach>,
    open: Res<OpenStock>,
    store: Res<HouseholdStore>,
    mut caches: ResMut<Caches>,
    mut purse: ResMut<Purse>,
    mut players: Query<(&mut Pack, &Hands)>,
    mut answers: MessageWriter<CacheAnswer>,
    mut mirrors: MessageWriter<StockAnswer>,
) {
    let requests = inbox.take();
    if requests.is_empty() {
        return;
    }
    let mut moved = false;
    let mut last = None;
    for (actor, CacheRequest::Take { key }) in requests {
        let outcome = (|| {
            let Ok((mut pack, _)) = players.get_mut(actor.0) else {
                return Err(CacheRefusal::NoPlayer);
            };
            if !caches.by.contains_key(&key) {
                return Err(CacheRefusal::NoCache);
            }
            let at_it = matches!(
                &reach.nearest,
                Some((_, InteractionKind::Cache { key: nearest })) if *nearest == key
            );
            if !at_it {
                return Err(CacheRefusal::NotInReach);
            }
            let cache = caches.by.get_mut(&key).ok_or(CacheRefusal::NoCache)?;
            let taken = take(&registries.0.items, &mut pack, cache)?;
            purse.0 = purse.0.saturating_add(taken.credits);
            if cache.is_empty() {
                caches.by.remove(&key);
            }
            moved = true;
            last = Some(actor.0);
            Ok(taken)
        })();
        answers.write(CacheAnswer { key, outcome });
    }
    if moved
        && let Some(actor) = last
        && let Ok((pack, hands)) = players.get(actor)
    {
        resync(&open, &store, pack, hands, &mut mirrors);
    }
}

/// Registers the caches and their requests and consumes them in the tick.
/// Added by `SimPlugin`; with no cache and no request it touches nothing.
#[derive(Debug, Clone, Copy)]
pub struct LootPlugin;

impl Plugin for LootPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Caches>()
            .add_request::<CacheRequest, _>(RequestSet::Loot, consume);
    }
}

#[cfg(test)]
mod tests {
    #![allow(
        clippy::unwrap_used,
        reason = "a test that unwraps a None fails, which is the point"
    )]

    use super::*;
    use crate::LocalPlayer;

    use std::collections::BTreeMap;

    use crate::deposits::Purse;

    use crate::items::Hands;
    use crate::items::HouseholdStore;
    use crate::items::OpenStock;
    use crate::items::Pack;

    use crate::items::StockAnswer;

    use crate::progression::InteractionKind;
    use crate::progression::Reach;

    use crate::Registries;

    pub(crate) use veyra_rules::loot::test_support::*;

    /// F beside the cache moves it into the purse and the pack; away from
    /// it, or with it gone, nothing moves; an emptied cache is removed.
    #[test]
    fn a_take_in_reach_pays_and_empties() {
        use bevy::ecs::message::Messages;
        use bevy::ecs::system::RunSystemOnce;

        let registries = Registries(std::sync::Arc::new(registries()));
        let drawn = drawn(&registries.0, 2, 5);
        let credits = drawn.credits;
        let mut world = World::new();
        world.insert_resource(Registries(registries.0.clone()));
        world.insert_resource(Caches {
            by: BTreeMap::from([(7, drawn)]),
        });
        world.init_resource::<CacheInbox>();
        world.init_resource::<Purse>();
        world.init_resource::<OpenStock>();
        world.insert_resource(HouseholdStore(slotted_model::Inventory::new(1)));
        world.init_resource::<Messages<CacheAnswer>>();
        world.init_resource::<Messages<StockAnswer>>();
        world.insert_resource(Reach::default());
        world.spawn((
            LocalPlayer,
            Pack::empty(registries.0.items.carry()),
            Hands(slotted_model::Inventory::new(1)),
        ));
        let ask = |world: &mut World| {
            world
                .resource_mut::<CacheInbox>()
                .push(CacheRequest::Take { key: 7 });
            world
                .run_system_once(strata_app::request::address::<CacheRequest>)
                .unwrap();
            world.run_system_once(consume).unwrap();
            let mut heard = world.resource_mut::<Messages<CacheAnswer>>();
            heard.drain().next().unwrap().outcome
        };
        assert_eq!(ask(&mut world), Err(CacheRefusal::NotInReach));
        world.insert_resource(Reach {
            nearest: Some((Entity::PLACEHOLDER, InteractionKind::Cache { key: 7 })),
            ..Reach::default()
        });
        let taken = ask(&mut world).unwrap();
        assert_eq!(taken.credits, credits);
        assert_eq!(world.resource::<Purse>().0, credits);
        assert!(world.resource::<Caches>().by.is_empty(), "emptied is gone");
        assert_eq!(ask(&mut world), Err(CacheRefusal::NoCache));
    }
}
