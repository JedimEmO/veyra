//! The adapter half of the `quests` rules: its requests, systems and plugin.

pub use veyra_rules::quests::*;

use std::collections::BTreeMap;

use bevy::app::{App, Plugin};

use bevy::ecs::prelude::*;

use bevy::ecs::system::SystemParam;

use strata_registry::quest::{Effect, Next, Objective};

use strata_registry::Namespaced;

use crate::combat::CombatImpact;

use crate::crafting::{self, CraftAnswer, CraftDone, Scratch};

use crate::deeds::Ledger;

use crate::deposits::{PingAnswer, PingDone, Purse, SampleAnswer, SampleDone, Survey};

use crate::encounters::Allegiance;

use crate::items::{self, Hands, HouseholdStore, Pack, Stock};

use crate::procurement::{PurchaseAnswer, SaleAnswer};

use crate::progression::{CharacterSheet, Reach, XpGained};

use crate::ride::{RideAnswer, RideDone};

use crate::{
    AppRequestExt, Inbox, LocalPlayer, Registries, Request, SimSet, SimTick, WorldPosition,
};

use strata_app::Actor;

pub use places::{Camp, QuestSites, camp, channel, metres, places};

/// What a person's conversation asks of the sim.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DialogueRequest {
    /// Start the person's conversation, with them in reach.
    Open(Namespaced),
    /// Choose the reply at this place among those shown.
    Choose(usize),
    /// Walk away: nothing is applied and nothing is raised.
    Close,
}

/// The requests not yet consumed.
pub type DialogueInbox = Inbox<DialogueRequest>;

/// A conversation is the household's and needs no one standing in it: it is
/// held to a person in reach, which `Reach` already says, so it is answered
/// whoever is there and has nothing to refuse for want of an actor.
impl Request for DialogueRequest {
    type Answer = DialogueView;

    fn no_actor(&self, _commands: &mut Commands) {}

    fn needs_actor(&self) -> bool {
        false
    }
}

/// The sim's refusal of a conversation request.
#[derive(Message, Debug, Clone, PartialEq, Eq)]
pub struct DialogueRefused {
    /// The request.
    pub request: DialogueRequest,
    /// Why.
    pub why: DialogueRefusal,
}

/// What the sim is asked to do with an [`Outcome`], as one parameter.
#[derive(SystemParam)]
pub struct Consequences<'w, 's> {
    registries: Res<'w, Registries>,
    purse: ResMut<'w, Purse>,
    store: ResMut<'w, HouseholdStore>,
    cast: Res<'w, QuestCast>,
    players: Query<'w, 's, (&'static mut Pack, &'static mut Hands)>,
    sheets: Query<'w, 's, (Entity, &'static mut CharacterSheet)>,
    locals: Query<'w, 's, Entity, With<LocalPlayer>>,
    ledger: Ledger<'w>,
    hostile: MessageWriter<'w, QuestHostile>,
    gained: MessageWriter<'w, XpGained>,
}

impl Consequences<'_, '_> {
    /// Whose pack and sheet a reply changes: the asker, or, for an event
    /// that no one asked for, the local player.
    fn resolve(&self, who: Option<Entity>) -> Option<Entity> {
        who.or_else(|| self.locals.single().ok())
    }

    /// The facts the conditions read, as they stand: the purse, what is
    /// carried and stored, what was made and is carried, which of the
    /// data's gifts would not fit, and the active member's boxes.
    fn facts(&self, who: Option<Entity>) -> Facts {
        let who = self.resolve(who);
        let registries = &self.registries.0;
        let catalogue = &registries.items;
        let mut items: BTreeMap<Namespaced, u32> = BTreeMap::new();
        let mut made: BTreeMap<Namespaced, u32> = BTreeMap::new();
        let mut full: BTreeMap<Namespaced, u32> = BTreeMap::new();
        if let Some(Ok((pack, hands))) = who.map(|who| self.players.get(who)) {
            for (record, stored) in crafting::reachable(catalogue, pack, Some(&self.store)) {
                *items.entry(record.item.clone()).or_default() += record.count;
                if !stored && is_made(&record) {
                    *made.entry(record.item).or_default() += record.count;
                }
            }
            let mut gifts = Vec::new();
            for node in registries.quests.nodes() {
                for reply in &node.replies {
                    strata_registry::quest::effects_of(
                        &reply.effects,
                        &mut |effect| match effect {
                            Effect::Give(item, count) => gifts.push((item, *count, None)),
                            Effect::GiveGraded(item, count, lines) => {
                                gifts.push((item, *count, Some(lines)));
                            }
                            _ => {}
                        },
                    );
                }
            }
            for (item, count, lines) in gifts {
                let Some(def) = catalogue.get(item) else {
                    continue;
                };
                let fits = items::write(&gift(def, count, lines), catalogue).is_ok_and(|stack| {
                    let mut scratch = Scratch {
                        pack: pack.clone(),
                        hands: hands.clone(),
                        store: Some(self.store.clone()),
                    };
                    scratch.put_away(catalogue, stack).is_ok()
                });
                if !fits {
                    let least = full.entry(item.clone()).or_insert(count);
                    *least = (*least).min(count);
                }
            }
        }
        Facts {
            purse: self.purse.0,
            items,
            learned: who
                .and_then(|who| self.sheets.get(who).ok())
                .map(|(_, sheet)| sheet.learned.clone())
                .unwrap_or_default(),
            made,
            full,
        }
    }

    /// Do what the effects asked, through the functions that own each thing.
    fn execute(&mut self, who: Option<Entity>, outcomes: Vec<Outcome>) {
        let who = self.resolve(who);
        for outcome in outcomes {
            match outcome {
                Outcome::Pay(credits) => self.purse.0 = self.purse.0.saturating_sub(credits),
                Outcome::Earn(credits) => self.purse.0 = self.purse.0.saturating_add(credits),
                Outcome::Give(item, count) => self.give(who, &item, count, None),
                Outcome::GiveGraded(item, count, lines) => {
                    self.give(who, &item, count, Some(&lines));
                }
                Outcome::Take(item, count) => self.take(who, &item, count, false),
                Outcome::TakeMade(item, count) => self.take(who, &item, count, true),
                Outcome::Xp(kind, amount) => {
                    if let Some(Ok((entity, mut sheet))) = who.map(|who| self.sheets.get_mut(who)) {
                        sheet.credit(kind, amount);
                        self.gained.write(XpGained {
                            entity,
                            kind,
                            amount,
                            kill: false,
                        });
                    }
                }
                Outcome::Deed(role, word, units) => {
                    if let Some(faction) = self.cast.factions.get(&role) {
                        self.ledger.deed_units(
                            &self.registries.0,
                            *faction,
                            deed_kind(word),
                            units,
                        );
                    }
                }
                Outcome::Hostile(target) => {
                    self.hostile.write(QuestHostile(target));
                }
            }
        }
    }

    /// Put `count` of `item` into the pack, its bags or the store. The reply
    /// was greyed unless there was room, so a give is never lost.
    fn give(
        &mut self,
        who: Option<Entity>,
        item: &Namespaced,
        count: u32,
        lines: Option<&BTreeMap<String, u16>>,
    ) {
        let items = &self.registries.0.items;
        let Some(def) = items.get(item) else {
            return;
        };
        let Ok(stack) = items::write(&gift(def, count, lines), items) else {
            return;
        };
        let Some(Ok((mut pack, mut hands))) = who.map(|who| self.players.get_mut(who)) else {
            return;
        };
        let mut held = Stock {
            saddle: None,
            pack: &mut pack,
            hands: &mut hands,
            store: Some(&mut self.store),
        };
        let mut scratch = Scratch::of(&held);
        if scratch.put_away(items, stack).is_ok() {
            scratch.commit(&mut held);
        }
    }

    /// Take `count` of `item` out of the pack and the store, store grade; or,
    /// `made`, of the first kind made at a bench the pack carries, from the
    /// pack alone. A take of what is not there takes nothing.
    fn take(&mut self, who: Option<Entity>, item: &Namespaced, count: u32, made: bool) {
        let items = &self.registries.0.items;
        let Some(Ok((mut pack, mut hands))) = who.map(|who| self.players.get_mut(who)) else {
            return;
        };
        let kind = if made {
            crafting::reachable(items, &pack, None)
                .into_iter()
                .map(|(record, _)| record)
                .find(|record| record.item == *item && is_made(record))
        } else {
            items.get(item).map(|def| gift(def, count, None))
        };
        let Some(stack) = kind.and_then(|kind| items::write(&kind, items).ok()) else {
            return;
        };
        let mut held = Stock {
            saddle: None,
            pack: &mut pack,
            hands: &mut hands,
            store: (!made).then_some(&mut *self.store),
        };
        let mut scratch = Scratch::of(&held);
        if scratch.take(items, &stack, count) {
            scratch.commit(&mut held);
        }
    }
}

/// `SimTick`, `SimSet::Input`: every conversation request made since the
/// last tick, in order. A conversation is opened only with its person in
/// reach and kept only while they stay in it; a chosen reply runs its
/// effects, and a reply that ends the conversation raises the person as
/// talked to.
#[allow(
    clippy::too_many_arguments,
    reason = "the requests, the open conversation, the log, the reach and the world's books"
)]
pub fn consume_dialogue(
    mut inbox: ResMut<DialogueInbox>,
    mut open: ResMut<OpenDialogue>,
    mut log: ResMut<QuestLog>,
    reach: Res<Reach>,
    mut world: Consequences,
    mut views: MessageWriter<DialogueView>,
    mut refusals: MessageWriter<DialogueRefused>,
) {
    let requests = inbox.take();
    if requests.is_empty() {
        return;
    }
    let registries = world.registries.0.clone();
    let data = &registries.quests;
    for (actor, request) in requests {
        let who = (actor != Actor::NOBODY).then_some(actor.0);
        let refuse = |why: DialogueRefusal, refusals: &mut MessageWriter<DialogueRefused>| {
            refusals.write(DialogueRefused {
                request: request.clone(),
                why,
            });
        };
        let facts = world.facts(who);
        match &request {
            DialogueRequest::Open(id) => {
                if !reach.people.contains(id) {
                    refuse(DialogueRefusal::NotNear, &mut refusals);
                    continue;
                }
                let node = data
                    .person(id)
                    .and_then(|person| opening(&person.conversations, &log, &facts))
                    .and_then(|node| data.node(node));
                let Some(node) = node else {
                    refuse(DialogueRefusal::NobodyHome, &mut refusals);
                    continue;
                };
                views.write(view(data, id, node, &log, &facts, &world.cast, true));
                open.0 = Some((id.clone(), node.id.clone()));
            }
            DialogueRequest::Close => {
                if let Some((id, node)) = open.0.take()
                    && let Some(node) = data.node(&node)
                {
                    views.write(view(data, &id, node, &log, &facts, &world.cast, false));
                }
            }
            DialogueRequest::Choose(place) => {
                let Some((id, at)) = open.0.clone() else {
                    refuse(DialogueRefusal::NothingOpen, &mut refusals);
                    continue;
                };
                if !reach.people.contains(&id) {
                    open.0 = None;
                    refuse(DialogueRefusal::NotNear, &mut refusals);
                    continue;
                }
                let Some(node) = data.node(&at) else {
                    open.0 = None;
                    continue;
                };
                let (mut done, next) = match choose(node, *place, &id, &mut log, data, &facts) {
                    Ok(chosen) => chosen,
                    Err(why) => {
                        refuse(why, &mut refusals);
                        continue;
                    }
                };
                match next {
                    Next::Node(next) => {
                        world.execute(who, std::mem::take(&mut done.outcomes));
                        let facts = world.facts(who);
                        if let Some(next) = data.node(next) {
                            views.write(view(data, &id, next, &log, &facts, &world.cast, true));
                            open.0 = Some((id.clone(), next.id.clone()));
                        }
                    }
                    Next::End => {
                        open.0 = None;
                        world.execute(who, std::mem::take(&mut done.outcomes));
                        let facts = world.facts(who);
                        let talked =
                            advance(&mut log, &QuestEvent::Talked(id.clone()), data, &facts);
                        world.execute(who, talked.outcomes);
                        views.write(view(data, &id, node, &log, &facts, &world.cast, false));
                    }
                }
            }
        }
    }
}

/// `SimTick`, `SimSet::Observe`: raise a [`QuestEvent`] beside each answer an
/// objective names, and for each kill of a hostile by a member. The systems
/// that answer are not touched.
#[allow(
    clippy::too_many_arguments,
    reason = "one reader for each answer an objective listens to"
)]
pub fn translate(
    registries: Res<Registries>,
    survey: Option<Res<Survey>>,
    mut pings: MessageReader<PingAnswer>,
    mut samples: MessageReader<SampleAnswer>,
    mut crafts: MessageReader<CraftAnswer>,
    mut buys: MessageReader<PurchaseAnswer>,
    mut sales: MessageReader<SaleAnswer>,
    mut rides: MessageReader<RideAnswer>,
    mut impacts: MessageReader<CombatImpact>,
    members: Query<(), (With<CharacterSheet>, Without<Allegiance>)>,
    victims: Query<&QuestFoe>,
    mut events: MessageWriter<QuestEvent>,
) {
    for answer in pings.read() {
        if matches!(answer.outcome, Ok(PingDone::Met(_))) {
            events.write(QuestEvent::Pinged);
        }
    }
    for answer in samples.read() {
        let Ok(SampleDone::Cycle { deposit, units, .. }) = answer.outcome else {
            continue;
        };
        let Some(found) = survey.as_ref().and_then(|survey| survey.0.get(deposit)) else {
            continue;
        };
        let class = registries.0.resources.class_of(found.ty).id.clone();
        events.write(QuestEvent::Sampled { class, units });
    }
    for answer in crafts.read() {
        if let Ok(CraftDone::Finished { product }) = &answer.outcome {
            events.write(QuestEvent::Crafted(product.item.clone()));
        }
    }
    for answer in buys.read() {
        if let Ok(bought) = &answer.result {
            events.write(QuestEvent::Bought(bought.item.clone()));
        }
    }
    for answer in sales.read() {
        if let Ok(sold) = &answer.result {
            events.write(QuestEvent::Sold(sold.item.clone()));
        }
    }
    for answer in rides.read() {
        if matches!(answer.result, Ok(RideDone::Stabled)) {
            events.write(QuestEvent::Stabled);
        }
    }
    for impact in impacts.read() {
        let Some(victim) = impact.victim else {
            continue;
        };
        if !impact.lethal || !members.contains(impact.attacker) {
            continue;
        }
        let Ok(foe) = victims.get(victim) else {
            continue;
        };
        events.write(QuestEvent::Defeated {
            kind: foe.kind,
            crew: foe.crew.clone(),
        });
    }
}

/// `SimTick`, `SimSet::Observe`: raise [`QuestEvent::Reached`] for each place
/// a current stage's Reach objective names, while the local player is within
/// its radius.
pub fn watch_places(
    registries: Res<Registries>,
    log: Res<QuestLog>,
    places: Res<QuestPlaces>,
    players: Query<&WorldPosition, With<LocalPlayer>>,
    mut events: MessageWriter<QuestEvent>,
) {
    let Ok(position) = players.single() else {
        return;
    };
    let data = &registries.0.quests;
    for (quest, state) in &log.quests {
        let Some(stage) = state
            .stage
            .as_deref()
            .filter(|_| !state.done)
            .and_then(|stage| data.quest(quest)?.stage(stage))
        else {
            continue;
        };
        for (index, objective) in stage.objectives.iter().enumerate() {
            let Objective::Reach(place, _) = objective else {
                continue;
            };
            let Some(at) = places.0.get(place).filter(|_| !state.met.contains(&index)) else {
                continue;
            };
            let apart = metres(*at, [position.0.x(), position.0.z()]);
            #[allow(
                clippy::cast_possible_truncation,
                reason = "metres to a screen distance"
            )]
            events.write(QuestEvent::Reached {
                place: *place,
                distance_m: apart as f32,
            });
        }
    }
}

/// `SimTick`, `SimSet::Observe`, after [`translate`] and [`watch_places`]:
/// let every event raised move the log, and do what the stages it finished
/// asked of the world.
pub fn advance_events(
    mut events: MessageReader<QuestEvent>,
    mut log: ResMut<QuestLog>,
    registries: Res<Registries>,
    mut world: Consequences,
) {
    for event in events.read() {
        let facts = world.facts(None);
        let applied = advance(&mut log, event, &registries.0.quests, &facts);
        world.execute(None, applied.outcomes);
    }
}

/// Registers the log, the conversation's requests and answers, the events and
/// their systems. Added by `SimPlugin`; with no questline in the data they
/// touch nothing.
#[derive(Debug, Clone, Copy)]
pub struct QuestsPlugin;

impl Plugin for QuestsPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<QuestLog>()
            .init_resource::<QuestCast>()
            .init_resource::<QuestPlaces>()
            .init_resource::<OpenDialogue>()
            .add_message::<QuestEvent>()
            .add_message::<DialogueRefused>()
            .add_message::<QuestHostile>()
            .add_request::<DialogueRequest, _>(SimSet::Input, consume_dialogue)
            .add_systems(
                SimTick,
                (translate, watch_places, advance_events)
                    .chain()
                    .in_set(SimSet::Observe),
            );
    }
}

#[cfg(test)]
mod tests {
    #![allow(
        clippy::unwrap_used,
        reason = "a test that unwraps a None fails, which is the point"
    )]

    use bevy::ecs::message::Messages;

    use super::*;
    use crate::crafting::tests::id;
    use crate::crafting::tests::shipped;

    use bevy::app::App;

    use crate::deposits::Purse;

    use crate::items::Hands;

    use crate::progression::CharacterSheet;

    use crate::LocalPlayer;

    use crate::SimTick;
    use crate::WorldPosition;

    /// The conversation system: opened only in reach, chosen on the sim's
    /// truth, effects through the purse and the stock, the end raising talked.
    #[test]
    fn the_system_runs_a_conversation() {
        use crate::items::Pack;
        use crate::progression::{Interaction, InteractionKind};
        let registries = shipped();
        let mut app = App::new();
        app.add_plugins((bevy::time::TimePlugin, crate::SimPlugin))
            .insert_resource(registries.clone());
        let carry = *registries.0.items.carry();
        app.world_mut().spawn((
            LocalPlayer,
            WorldPosition::new(0.0, 0.0, 0.0),
            Pack::empty(&carry),
            Hands::default(),
            CharacterSheet::default(),
        ));
        let maren = id("veyra:person.maren_coyle");
        app.world_mut().spawn((
            WorldPosition::new(2.0, 0.0, 0.0),
            Interaction {
                kind: InteractionKind::Person { id: maren.clone() },
                radius: 4.0,
            },
        ));
        app.world_mut().resource_mut::<Purse>().0 = 100;
        let tick = |app: &mut App| {
            app.world_mut().run_schedule(SimTick);
        };
        // Out of reach: refused. The reach is found in the Observe set.
        app.world_mut()
            .resource_mut::<DialogueInbox>()
            .push(DialogueRequest::Open(maren.clone()));
        tick(&mut app);
        let refused: Vec<_> = app
            .world_mut()
            .resource_mut::<Messages<DialogueRefused>>()
            .drain()
            .collect();
        assert_eq!(refused.len(), 1);
        assert_eq!(refused[0].why, DialogueRefusal::NotNear);
        // In reach: opens, then the first reply starts the quest and pays.
        tick(&mut app);
        app.world_mut()
            .resource_mut::<DialogueInbox>()
            .push(DialogueRequest::Open(maren.clone()));
        tick(&mut app);
        let views: Vec<_> = app
            .world_mut()
            .resource_mut::<Messages<DialogueView>>()
            .drain()
            .collect();
        assert_eq!(views.len(), 1, "opened");
        assert!(views[0].open && views[0].replies.len() == 2);
        app.world_mut()
            .resource_mut::<DialogueInbox>()
            .push(DialogueRequest::Choose(0));
        tick(&mut app);
        assert_eq!(
            app.world().resource::<QuestLog>().stage_of("arrival"),
            Some("landfall")
        );
        assert_eq!(app.world().resource::<Purse>().0, 300);
        assert!(app.world().resource::<OpenDialogue>().0.is_some());
        app.world_mut()
            .resource_mut::<DialogueInbox>()
            .push(DialogueRequest::Close);
        tick(&mut app);
        assert!(app.world().resource::<OpenDialogue>().0.is_none());
    }
}
