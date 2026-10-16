//! The questline's rules (ticket 0014, step 1): a [`QuestLog`] that advances
//! from [`QuestEvent`]s and from conversations, over the people, dialogue and
//! quests of `strata_registry::quest`.
//!
//! Everything that decides is pure and tested without an app: [`advance`]
//! (an event moves a quest's stages), [`shown`] (which replies a node offers
//! and which are greyed, and by what), [`apply`] (what a reply's effects do
//! to the log and what they ask of the world), [`opening`] (which
//! conversation a person starts) and [`fill`] (the faction names in a line).
//! What an effect asks of the world, a purse, a pack or a ledger, comes back
//! as [`Outcome`]s that one system executes through the functions that
//! already own those things.
//!
//! The objectives listen to answers that already exist: [`translate`] reads
//! them and raises a [`QuestEvent`] beside each (a ping, a sample, a finished
//! session, a purchase, a sale, a bike stabled, a kill), so the systems that
//! answer are untouched. The events the sim cannot see (learning a box, a
//! recruit, a switch) are written by the screens and the game.
//!
//! Local-only: nothing here is replicated and no handshake carries it.

use std::collections::{BTreeMap, BTreeSet};

use bevy::app::{App, Plugin};
use bevy::ecs::prelude::*;
use bevy::ecs::system::SystemParam;
use serde::{Deserialize, Serialize};
use strata_registry::item::Record;
use strata_registry::progression::XpKind;
use strata_registry::quest::{
    Beat, Condition, DeedWord, Effect, Next, Node, Objective, Opening, Place, QuestDef, QuestSet,
    Role,
};
use strata_registry::{FoeKind, Namespaced};
use strata_worldgen::factions::{FactionId, FactionMap};

use crate::combat::CombatImpact;
use crate::combat_ai::OpponentKind;
use crate::crafting::{self, CraftAnswer, CraftDone, Scratch};
use crate::deeds::{DeedKind, Ledger};
use crate::deposits::{PingAnswer, PingDone, Purse, SampleAnswer, SampleDone, Survey};
use crate::encounters::Allegiance;
use crate::items::{self, Hands, HouseholdStore, Pack, Stock};
use crate::procurement::{PurchaseAnswer, SaleAnswer};
use crate::progression::{CharacterSheet, Reach, XpGained};
use crate::ride::{RideAnswer, RideDone};
use crate::{LocalPlayer, Registries, SimSet, SimTick, WorldPosition};

mod places;

pub use places::{Camp, QuestSites, camp, channel, metres, places};

/// Where one quest stands.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct QuestState {
    /// The stage it is at; `None` once it is over.
    pub stage: Option<String>,
    /// Whether it is over.
    pub done: bool,
    /// The objectives of the stage already met, by index.
    #[serde(default)]
    pub met: BTreeSet<usize>,
    /// What a counting objective has so far, by index.
    #[serde(default)]
    pub counts: BTreeMap<usize, u32>,
}

/// A reply the player chose, kept for the journal.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Choice {
    /// The node it was said at.
    pub node: String,
    /// Its place in that node's replies.
    pub reply: usize,
}

/// Every quest's stage, the flags set, the choices made and the ending. The
/// game saves it in the household file as a serde default, so an older file
/// reads with an empty log.
#[derive(Resource, Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct QuestLog {
    /// By quest id.
    #[serde(default)]
    pub quests: BTreeMap<String, QuestState>,
    /// The flags set, by name.
    #[serde(default)]
    pub flags: BTreeSet<String>,
    /// The replies chosen, oldest first.
    #[serde(default)]
    pub choices: Vec<Choice>,
    /// How the questline ended, once it has.
    #[serde(default)]
    pub ending: Option<String>,
    /// The crews and people a reply turned on the household, by the id the
    /// `Hostile` effect named: a crossed crew that folds out of reach stands
    /// again crossed, not neutral.
    #[serde(default)]
    pub hostile: BTreeSet<String>,
}

impl QuestLog {
    /// The stage `quest` is at, while it is under way.
    #[must_use]
    pub fn stage_of(&self, quest: &str) -> Option<&str> {
        self.quests
            .get(quest)
            .filter(|state| !state.done)
            .and_then(|state| state.stage.as_deref())
    }

    /// Whether `quest` is over.
    #[must_use]
    pub fn is_done(&self, quest: &str) -> bool {
        self.quests.get(quest).is_some_and(|state| state.done)
    }

    /// Whether the flag is set.
    #[must_use]
    pub fn flag(&self, name: &str) -> bool {
        self.flags.contains(name)
    }
}

/// The facts about the household a condition reads that the log does not
/// hold, taken once so the rules stay pure.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Facts {
    /// Credits in the purse.
    pub purse: u32,
    /// Items carried or stored, counted by kind.
    pub items: BTreeMap<Namespaced, u32>,
    /// The boxes the active member has learned.
    pub learned: BTreeSet<String>,
    /// Items made at a bench carried in the pack and its bags, by kind.
    pub made: BTreeMap<Namespaced, u32>,
    /// Items a give could not place now, each with the least count that
    /// does not fit; one not here fits.
    pub full: BTreeMap<Namespaced, u32>,
}

/// Whether `condition` holds.
#[must_use]
pub fn holds(condition: &Condition, log: &QuestLog, facts: &Facts) -> bool {
    match condition {
        Condition::QuestAt(quest, stage) => log.stage_of(quest) == Some(stage.as_str()),
        Condition::QuestDone(quest) => log.is_done(quest),
        Condition::Flag(name) => log.flag(name),
        Condition::NotFlag(name) => !log.flag(name),
        Condition::PurseAtLeast(credits) => facts.purse >= *credits,
        Condition::Holds(item, count) => facts.items.get(item).copied().unwrap_or(0) >= *count,
        Condition::Learned(id) => facts.learned.contains(id),
        Condition::Room(item, count) => facts.full.get(item).is_none_or(|least| count < least),
        Condition::CarriesMade(item, count) => facts.made.get(item).copied().unwrap_or(0) >= *count,
    }
}

/// The first condition of `when` that fails, which is the reason a reply is
/// greyed.
#[must_use]
pub fn failing<'a>(when: &'a [Condition], log: &QuestLog, facts: &Facts) -> Option<&'a Condition> {
    when.iter().find(|condition| !holds(condition, log, facts))
}

/// The node a person opens: the first conversation whose conditions hold.
#[must_use]
pub fn opening<'a>(conversations: &'a [Opening], log: &QuestLog, facts: &Facts) -> Option<&'a str> {
    conversations
        .iter()
        .find(|opening| failing(&opening.when, log, facts).is_none())
        .map(|opening| opening.node.as_str())
}

/// A reply a node offers now.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Shown {
    /// Its place in the node's replies.
    pub index: usize,
    /// What it says, with its placeholders.
    pub text: String,
    /// Why it cannot be chosen, if it cannot: the first condition that
    /// fails, the implicit ones of a `Pay`, a `Take` and a `Give` last.
    pub blocked: Option<Condition>,
    /// The data's own words for it, when one of the reply's `when` is what
    /// greys it.
    pub why: Option<String>,
}

/// What a reply's effects spend or need, which the reply needs to be able
/// to: a `Pay`'s credits, a `Take`'s items and a `Give`'s room, so a reply
/// that pays for what it gives is greyed rather than paid for nothing.
fn implied(effects: &[Effect], into: &mut Vec<Condition>) {
    strata_registry::quest::effects_of(effects, &mut |effect| match effect {
        Effect::Pay(credits) => into.push(Condition::PurseAtLeast(*credits)),
        Effect::Take(item, count) => into.push(Condition::Holds(item.clone(), *count)),
        Effect::TakeMade(item, count) => {
            into.push(Condition::CarriesMade(item.clone(), *count));
        }
        Effect::Give(item, count) | Effect::GiveGraded(item, count, _) => {
            into.push(Condition::Room(item.clone(), *count));
        }
        _ => {}
    });
}

/// The replies of `node` the player is shown, in order. A reply whose
/// conditions fail is greyed with the reason, or left out when it is
/// `hidden`.
#[must_use]
pub fn shown(node: &Node, log: &QuestLog, facts: &Facts) -> Vec<Shown> {
    let mut list = Vec::new();
    for (index, reply) in node.replies.iter().enumerate() {
        let mut needs = reply.when.clone();
        implied(&reply.effects, &mut needs);
        let blocked = failing(&needs, log, facts).cloned();
        // Only the reply's own conditions hide it: one that fails for want
        // of room or credits is greyed, so the player sees the way is there.
        let own = failing(&reply.when, log, facts).is_some();
        if own && reply.hidden {
            continue;
        }
        list.push(Shown {
            index,
            text: reply.text.clone(),
            why: reply.why.clone().filter(|_| own),
            blocked,
        });
    }
    list
}

/// What an effect asks of the world, which the log cannot do itself.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    /// Credits leave the purse.
    Pay(u32),
    /// Credits join the purse.
    Earn(u32),
    /// Items go into the pack.
    Give(Namespaced, u32),
    /// Items go into the pack with these lines.
    GiveGraded(Namespaced, u32, BTreeMap<String, u16>),
    /// Items leave the pack or the store.
    Take(Namespaced, u32),
    /// Items made at a bench leave the pack.
    TakeMade(Namespaced, u32),
    /// XP joins the active member's sheet.
    Xp(XpKind, u32),
    /// A deed against or for the faction filling a role.
    Deed(Role, DeedWord, u32),
    /// A crew or a person turns on the household.
    Hostile(String),
}

/// What changed in the log.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Change {
    /// A quest began.
    Started(String),
    /// A quest entered a stage.
    Stage {
        /// The quest.
        quest: String,
        /// The stage.
        stage: String,
    },
    /// An objective of the stage was met.
    Objective {
        /// The quest.
        quest: String,
        /// The stage.
        stage: String,
        /// Its place in the stage's objectives.
        index: usize,
    },
    /// A quest ended.
    Done(String),
    /// A flag was set.
    Flag(String),
    /// The questline ended this way.
    Ending(String),
}

/// What an event or a reply did: what to ask of the world and what moved.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Applied {
    /// For the world, in order.
    pub outcomes: Vec<Outcome>,
    /// What moved in the log, in order.
    pub changes: Vec<Change>,
}

impl Applied {
    fn merge(&mut self, other: Self) {
        self.outcomes.extend(other.outcomes);
        self.changes.extend(other.changes);
    }
}

/// Something that happened which an objective may be waiting for.
#[derive(Message, Debug, Clone, PartialEq)]
pub enum QuestEvent {
    /// A conversation with a person reached its end.
    Talked(Namespaced),
    /// The household came within `distance_m` of a place.
    Reached {
        /// Which.
        place: Place,
        /// How near, metres.
        distance_m: f32,
    },
    /// A hostile fell to a member.
    Defeated {
        /// Its kind.
        kind: FoeKind,
        /// The quest crew it was of, if any.
        crew: Option<String>,
    },
    /// A box was learned.
    Learned(String),
    /// A ping read the ground.
    Pinged,
    /// A sampling gave units of a class.
    Sampled {
        /// The resource class.
        class: Namespaced,
        /// The units this cycle gave.
        units: u32,
    },
    /// An item was finished at the bench.
    Crafted(Namespaced),
    /// An item was sold at a board.
    Sold(Namespaced),
    /// An item was bought at a board.
    Bought(Namespaced),
    /// A bike became the household's ride.
    Stabled,
    /// A member joined the bench.
    Recruited(String),
    /// The household switched to a member.
    Switched(String),
    /// An item was handed to a person.
    Delivered {
        /// The item.
        item: Namespaced,
        /// The person.
        person: Namespaced,
    },
}

/// How much an event gives an objective, and how much it takes to meet it.
fn progress(def: &QuestDef, objective: &Objective, event: &QuestEvent) -> Option<(u32, u32)> {
    use QuestEvent as E;
    match (objective, event) {
        (Objective::Talk(want), E::Talked(who)) if want == who => Some((1, 1)),
        (Objective::Reach(want, radius), E::Reached { place, distance_m })
            if want == place
                && *distance_m <= f32::from(u16::try_from(*radius).unwrap_or(u16::MAX)) =>
        {
            Some((1, 1))
        }
        (Objective::Defeat(Beat::Opponent(want)), E::Defeated { kind, .. }) if want == kind => {
            Some((1, 1))
        }
        (
            Objective::Defeat(Beat::Crew(want)),
            E::Defeated {
                crew: Some(crew), ..
            },
        ) if want == crew => Some((1, def.crew_size(want).max(1))),
        (Objective::Learn, E::Learned(_))
        | (Objective::Ping, E::Pinged)
        | (Objective::Stable, E::Stabled) => Some((1, 1)),
        (
            Objective::Sample { class, units },
            E::Sampled {
                class: got,
                units: more,
            },
        ) if class == got => Some((*more, *units)),
        (Objective::Craft(want), E::Crafted(got))
        | (Objective::Sell(want), E::Sold(got))
        | (Objective::Buy(want), E::Bought(got))
            if want == got =>
        {
            Some((1, 1))
        }
        (Objective::Recruit(want), E::Recruited(got))
        | (Objective::Switch(want), E::Switched(got))
            if want == got =>
        {
            Some((1, 1))
        }
        (Objective::Deliver(want, to), E::Delivered { item, person })
            if want == item && to == person =>
        {
            Some((1, 1))
        }
        _ => None,
    }
}

fn enter(log: &mut QuestLog, data: &QuestSet, quest: &str, stage: &str, out: &mut Applied) {
    if data.quest(quest).and_then(|def| def.stage(stage)).is_none() {
        return;
    }
    if !log.quests.contains_key(quest) {
        out.changes.push(Change::Started(quest.to_owned()));
    }
    log.quests.insert(
        quest.to_owned(),
        QuestState {
            stage: Some(stage.to_owned()),
            ..QuestState::default()
        },
    );
    out.changes.push(Change::Stage {
        quest: quest.to_owned(),
        stage: stage.to_owned(),
    });
}

fn finish(log: &mut QuestLog, quest: &str, out: &mut Applied) {
    if let Some(state) = log.quests.get_mut(quest)
        && !state.done
    {
        state.done = true;
        state.stage = None;
        state.met.clear();
        state.counts.clear();
        out.changes.push(Change::Done(quest.to_owned()));
    }
}

fn run(
    effect: &Effect,
    speaker: Option<&Namespaced>,
    log: &mut QuestLog,
    data: &QuestSet,
    facts: &Facts,
    out: &mut Applied,
) {
    match effect {
        Effect::StartQuest(quest) => {
            if !log.quests.contains_key(quest)
                && let Some(first) = data.quest(quest).and_then(|def| def.stages.first())
            {
                enter(log, data, quest, &first.id, out);
            }
        }
        Effect::SetStage(quest, stage) => enter(log, data, quest, stage, out),
        Effect::SetFlag(name) => {
            if log.flags.insert(name.clone()) {
                out.changes.push(Change::Flag(name.clone()));
            }
        }
        Effect::Give(item, count) => out.outcomes.push(Outcome::Give(item.clone(), *count)),
        Effect::GiveGraded(item, count, lines) => {
            out.outcomes
                .push(Outcome::GiveGraded(item.clone(), *count, lines.clone()));
        }
        Effect::Xp(kind, amount) => out.outcomes.push(Outcome::Xp(*kind, *amount)),
        Effect::Take(item, count) | Effect::TakeMade(item, count) => {
            out.outcomes
                .push(if matches!(effect, Effect::TakeMade(..)) {
                    Outcome::TakeMade(item.clone(), *count)
                } else {
                    Outcome::Take(item.clone(), *count)
                });
            if let Some(person) = speaker {
                let event = QuestEvent::Delivered {
                    item: item.clone(),
                    person: person.clone(),
                };
                out.merge(advance(log, &event, data, facts));
            }
        }
        Effect::Pay(credits) => out.outcomes.push(Outcome::Pay(*credits)),
        Effect::Earn(credits) => out.outcomes.push(Outcome::Earn(*credits)),
        Effect::Deed(role, kind, units) => out.outcomes.push(Outcome::Deed(*role, *kind, *units)),
        Effect::Hostile(target) => {
            log.hostile.insert(target.clone());
            out.outcomes.push(Outcome::Hostile(target.clone()));
        }
        Effect::Ending(name) => {
            log.ending = Some(name.clone());
            if log.flags.insert(format!("ending_{name}")) {
                out.changes.push(Change::Flag(format!("ending_{name}")));
            }
            out.changes.push(Change::Ending(name.clone()));
            for quest in data.quests().iter().filter(|q| q.ending(name).is_some()) {
                log.quests.entry(quest.id.clone()).or_default();
                finish(log, &quest.id, out);
            }
        }
        Effect::If(when, then) => {
            if failing(when, log, facts).is_none() {
                for effect in then {
                    run(effect, speaker, log, data, facts, out);
                }
            }
        }
    }
}

/// Run a reply's `effects` on the log, in order. A `Take` by `speaker` is
/// also a delivery to them, which may meet an objective. What the effects
/// ask of the world is returned, not done.
pub fn apply(
    effects: &[Effect],
    speaker: Option<&Namespaced>,
    log: &mut QuestLog,
    data: &QuestSet,
    facts: &Facts,
) -> Applied {
    let mut out = Applied::default();
    for effect in effects {
        run(effect, speaker, log, data, facts, &mut out);
    }
    out
}

/// Let `event` move every quest that is waiting for it: objectives are met
/// (a counting one by its count), a stage whose objectives are all met runs
/// its `on_done` effects and goes on to its next stage, or ends the quest.
/// A stage with no objectives waits for a reply and is never met by an event.
pub fn advance(log: &mut QuestLog, event: &QuestEvent, data: &QuestSet, facts: &Facts) -> Applied {
    let mut out = Applied::default();
    let active: Vec<(String, String)> = log
        .quests
        .iter()
        .filter(|(_, state)| !state.done)
        .filter_map(|(quest, state)| Some((quest.clone(), state.stage.clone()?)))
        .collect();
    for (quest, stage_id) in active {
        let Some(def) = data.quest(&quest) else {
            continue;
        };
        let Some(stage) = def.stage(&stage_id) else {
            continue;
        };
        // An earlier quest's effects may have moved this one.
        if log.stage_of(&quest) != Some(stage_id.as_str()) {
            continue;
        }
        let Some(progress_of) = log.quests.get_mut(&quest) else {
            continue;
        };
        for (index, objective) in stage.objectives.iter().enumerate() {
            if progress_of.met.contains(&index) {
                continue;
            }
            let Some((add, need)) = progress(def, objective, event) else {
                continue;
            };
            let count = progress_of.counts.entry(index).or_insert(0);
            *count = count.saturating_add(add);
            if *count >= need {
                progress_of.met.insert(index);
                out.changes.push(Change::Objective {
                    quest: quest.clone(),
                    stage: stage_id.clone(),
                    index,
                });
            }
        }
        let all = !stage.objectives.is_empty() && progress_of.met.len() == stage.objectives.len();
        if !all {
            continue;
        }
        let done = apply(&stage.on_done.effects, None, log, data, facts);
        out.merge(done);
        if log.stage_of(&quest) != Some(stage_id.as_str()) {
            continue;
        }
        match &stage.on_done.next {
            Some(next) => enter(log, data, &quest, next, &mut out),
            None => finish(log, &quest, &mut out),
        }
    }
    out
}

/// The factions that fill the three roles in the world, made from its map.
/// The game builds it once its factions exist.
#[derive(Resource, Debug, Clone, PartialEq, Default)]
pub struct QuestCast {
    /// The name of each role's faction.
    pub names: BTreeMap<Role, String>,
    /// The faction of each role, where a deed is written.
    pub factions: BTreeMap<Role, FactionId>,
}

impl QuestCast {
    /// The city's holder, and the nearest assembly and conservation factions
    /// within `reach_m` of the city at `(x, z)`.
    #[must_use]
    pub fn of(map: &FactionMap, city: [f64; 2], reach_m: f64) -> Self {
        let mut cast = Self::default();
        let mut put = |role: Role, faction: strata_worldgen::factions::Faction| {
            cast.names.insert(role, faction.name);
            cast.factions.insert(role, faction.id);
        };
        if let Some(holder) = map
            .territory(city[0], city[1])
            .holder
            .and_then(|id| map.faction(id))
        {
            put(Role::Holder, holder);
        }
        let near = map.factions_near(city[0], city[1], reach_m);
        for (role, nature) in [
            (Role::Assembly, strata_registry::faction::Nature::Assembly),
            (
                Role::Keepers,
                strata_registry::faction::Nature::Conservation,
            ),
        ] {
            let nearest = near
                .iter()
                .filter(|faction| faction.nature == nature)
                .min_by(|a, b| {
                    metres(a.seat, city)
                        .total_cmp(&metres(b.seat, city))
                        .then(a.id.cmp(&b.id))
                });
            if let Some(faction) = nearest {
                put(role, faction.clone());
            }
        }
        cast
    }
}

/// `text` with each `{holder}`, `{assembly}` and `{keepers}` replaced by the
/// name of the faction that fills the role. A role nobody fills reads as its
/// own word.
#[must_use]
pub fn fill(text: &str, cast: &QuestCast) -> String {
    let mut line = text.to_owned();
    for (role, word) in [
        (Role::Holder, "holder"),
        (Role::Assembly, "assembly"),
        (Role::Keepers, "keepers"),
    ] {
        let name = cast.names.get(&role).map_or(word, String::as_str);
        line = line.replace(&format!("{{{word}}}"), name);
    }
    line
}

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
#[derive(Resource, Debug, Default)]
pub struct DialogueInbox(pub Vec<DialogueRequest>);

/// The conversation open: who, and at which node.
#[derive(Resource, Debug, Clone, PartialEq, Eq, Default)]
pub struct OpenDialogue(pub Option<(Namespaced, String)>);

/// A reply as the screen shows it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReplyView {
    /// What it says, filled.
    pub text: String,
    /// Why it cannot be chosen.
    pub blocked: Option<Condition>,
    /// The data's words for why, filled, when the reply has them; else the
    /// screen words `blocked` itself.
    pub why: Option<String>,
}

/// The sim's answer to an open or a choice: what is said now, and the
/// replies to it. `open` is false when the conversation has ended.
#[derive(Message, Debug, Clone, PartialEq, Eq)]
pub struct DialogueView {
    /// The speaker's id.
    pub speaker: Namespaced,
    /// The name on their plate.
    pub name: String,
    /// The line, filled.
    pub line: String,
    /// What can be said back.
    pub replies: Vec<ReplyView>,
    /// Whether the conversation goes on.
    pub open: bool,
}

/// Why a conversation request was refused.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DialogueRefusal {
    /// The person is not within reach.
    NotNear,
    /// No such person, or nothing they would say now.
    NobodyHome,
    /// No conversation is open.
    NothingOpen,
    /// No reply at that place.
    NoReply,
    /// The reply is greyed, for this reason.
    Blocked(Condition),
}

/// The sim's refusal of a conversation request.
#[derive(Message, Debug, Clone, PartialEq, Eq)]
pub struct DialogueRefused {
    /// The request.
    pub request: DialogueRequest,
    /// Why.
    pub why: DialogueRefusal,
}

/// A crew or a person a reply turned on the household, for the game to arm.
#[derive(Message, Debug, Clone, PartialEq, Eq)]
pub struct QuestHostile(pub String);

/// What a hostile is to the questline, which a defeat is counted by: the
/// game puts it on every hostile it stands, with the quest crew it belongs
/// to, if any. The opponent's own brain is the game's, not the sim's.
#[derive(Component, Debug, Clone, PartialEq, Eq)]
pub struct QuestFoe {
    /// Its kind.
    pub kind: FoeKind,
    /// The quest crew it is one of.
    pub crew: Option<String>,
}

/// Where the quest places stand, placed by the game when the questline
/// begins.
#[derive(Resource, Debug, Clone, PartialEq, Default)]
pub struct QuestPlaces(pub BTreeMap<Place, [f64; 2]>);

/// What the sim is asked to do with an [`Outcome`], as one parameter.
#[derive(SystemParam)]
pub struct Consequences<'w, 's> {
    registries: Res<'w, Registries>,
    purse: ResMut<'w, Purse>,
    store: ResMut<'w, HouseholdStore>,
    cast: Res<'w, QuestCast>,
    players: Query<'w, 's, (&'static mut Pack, &'static mut Hands), With<LocalPlayer>>,
    sheets: Query<'w, 's, (Entity, &'static mut CharacterSheet), With<LocalPlayer>>,
    ledger: Ledger<'w>,
    hostile: MessageWriter<'w, QuestHostile>,
    gained: MessageWriter<'w, XpGained>,
}

/// The record a give puts in the pack: store grade, or with the lines the
/// data names.
fn gift(
    def: &strata_registry::item::ItemDefinition,
    count: u32,
    lines: Option<&BTreeMap<String, u16>>,
) -> Record {
    let mut record = crafting::bought(def);
    record.count = count;
    if let Some(lines) = lines {
        record.lines.clone_from(lines);
    }
    record
}

/// Whether a record was made at a bench: it names its batch and its maker,
/// which store grade never does.
const fn is_made(record: &Record) -> bool {
    record.batch > 0 && record.by.is_some()
}

impl Consequences<'_, '_> {
    /// The facts the conditions read, as they stand: the purse, what is
    /// carried and stored, what was made and is carried, which of the
    /// data's gifts would not fit, and the active member's boxes.
    fn facts(&self) -> Facts {
        let registries = &self.registries.0;
        let catalogue = &registries.items;
        let mut items: BTreeMap<Namespaced, u32> = BTreeMap::new();
        let mut made: BTreeMap<Namespaced, u32> = BTreeMap::new();
        let mut full: BTreeMap<Namespaced, u32> = BTreeMap::new();
        if let Ok((pack, hands)) = self.players.single() {
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
            learned: self
                .sheets
                .single()
                .map(|(_, sheet)| sheet.learned.clone())
                .unwrap_or_default(),
            made,
            full,
        }
    }

    /// Do what the effects asked, through the functions that own each thing.
    fn execute(&mut self, outcomes: Vec<Outcome>) {
        for outcome in outcomes {
            match outcome {
                Outcome::Pay(credits) => self.purse.0 = self.purse.0.saturating_sub(credits),
                Outcome::Earn(credits) => self.purse.0 = self.purse.0.saturating_add(credits),
                Outcome::Give(item, count) => self.give(&item, count, None),
                Outcome::GiveGraded(item, count, lines) => self.give(&item, count, Some(&lines)),
                Outcome::Take(item, count) => self.take(&item, count, false),
                Outcome::TakeMade(item, count) => self.take(&item, count, true),
                Outcome::Xp(kind, amount) => {
                    if let Ok((entity, mut sheet)) = self.sheets.single_mut() {
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
    fn give(&mut self, item: &Namespaced, count: u32, lines: Option<&BTreeMap<String, u16>>) {
        let items = &self.registries.0.items;
        let Some(def) = items.get(item) else {
            return;
        };
        let Ok(stack) = items::write(&gift(def, count, lines), items) else {
            return;
        };
        let Ok((mut pack, mut hands)) = self.players.single_mut() else {
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
    fn take(&mut self, item: &Namespaced, count: u32, made: bool) {
        let items = &self.registries.0.items;
        let Ok((mut pack, mut hands)) = self.players.single_mut() else {
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

/// The sim's deed of a quest's word.
#[must_use]
pub const fn deed_kind(word: DeedWord) -> DeedKind {
    match word {
        DeedWord::WorkedConcession => DeedKind::WorkedConcession,
        DeedWord::WorkedReserve => DeedKind::WorkedReserve,
        DeedWord::TookPastShare => DeedKind::TookPastShare,
        DeedWord::Planted => DeedKind::Planted,
        DeedWord::Sold => DeedKind::Sold,
        DeedWord::ClearedCamp => DeedKind::ClearedCamp,
        DeedWord::Hurt => DeedKind::Hurt,
        DeedWord::Killed => DeedKind::Killed,
        DeedWord::JobDone => DeedKind::JobDone,
    }
}

fn view(
    data: &QuestSet,
    person: &Namespaced,
    node: &Node,
    log: &QuestLog,
    facts: &Facts,
    cast: &QuestCast,
    open: bool,
) -> DialogueView {
    DialogueView {
        speaker: node.speaker.clone(),
        name: data
            .person(person)
            .map(|def| def.name.clone())
            .unwrap_or_default(),
        line: fill(&node.line, cast),
        replies: shown(node, log, facts)
            .into_iter()
            .map(|reply| ReplyView {
                text: fill(&reply.text, cast),
                why: reply.why.as_deref().map(|why| fill(why, cast)),
                blocked: reply.blocked,
            })
            .collect(),
        open,
    }
}

/// Choose the reply at `place` among those `node` shows: record it, run its
/// effects and say where it leads. The one rule a conversation request and a
/// script ask. A reply that ends the conversation leaves the caller to raise
/// [`QuestEvent::Talked`] once the world has done what the effects asked.
///
/// # Errors
/// No reply at that place, or one that is greyed.
pub fn choose<'a>(
    node: &'a Node,
    place: usize,
    speaker: &Namespaced,
    log: &mut QuestLog,
    data: &QuestSet,
    facts: &Facts,
) -> Result<(Applied, &'a Next), DialogueRefusal> {
    let list = shown(node, log, facts);
    let chosen = list.get(place).ok_or(DialogueRefusal::NoReply)?;
    if let Some(why) = &chosen.blocked {
        return Err(DialogueRefusal::Blocked(why.clone()));
    }
    let reply = node
        .replies
        .get(chosen.index)
        .ok_or(DialogueRefusal::NoReply)?;
    log.choices.push(Choice {
        node: node.id.clone(),
        reply: chosen.index,
    });
    let done = apply(&reply.effects, Some(speaker), log, data, facts);
    Ok((done, &reply.next))
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
    if inbox.0.is_empty() {
        return;
    }
    let requests = std::mem::take(&mut inbox.0);
    let registries = world.registries.0.clone();
    let data = &registries.quests;
    for request in requests {
        let refuse = |why: DialogueRefusal, refusals: &mut MessageWriter<DialogueRefused>| {
            refusals.write(DialogueRefused {
                request: request.clone(),
                why,
            });
        };
        let facts = world.facts();
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
                        world.execute(std::mem::take(&mut done.outcomes));
                        let facts = world.facts();
                        if let Some(next) = data.node(next) {
                            views.write(view(data, &id, next, &log, &facts, &world.cast, true));
                            open.0 = Some((id.clone(), next.id.clone()));
                        }
                    }
                    Next::End => {
                        open.0 = None;
                        world.execute(std::mem::take(&mut done.outcomes));
                        let facts = world.facts();
                        let talked =
                            advance(&mut log, &QuestEvent::Talked(id.clone()), data, &facts);
                        world.execute(talked.outcomes);
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

/// The registry's name of an opponent kind.
#[must_use]
pub const fn foe_kind(kind: OpponentKind) -> FoeKind {
    match kind {
        OpponentKind::Sentry => FoeKind::Sentry,
        OpponentKind::Rusher => FoeKind::Rusher,
        OpponentKind::Marksman => FoeKind::Marksman,
        OpponentKind::Duelist => FoeKind::Duelist,
        OpponentKind::Bulwark => FoeKind::Bulwark,
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
        let facts = world.facts();
        let applied = advance(&mut log, event, &registries.0.quests, &facts);
        world.execute(applied.outcomes);
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
            .init_resource::<DialogueInbox>()
            .init_resource::<OpenDialogue>()
            .add_message::<QuestEvent>()
            .add_message::<DialogueView>()
            .add_message::<DialogueRefused>()
            .add_message::<QuestHostile>()
            .add_systems(SimTick, consume_dialogue.in_set(SimSet::Input))
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

    use std::sync::Arc;

    use bevy::ecs::message::Messages;
    use strata_registry::quest::{QuestContext, QuestFiles};
    use strata_worldgen::ProceduralWorld;

    use super::*;
    use crate::crafting::tests::{id, shipped};

    const PERSON: &str = r#"(
        version: 1, id: "veyra:person.t", name: "Tee", title: "of {holder}",
        role: Holder, tint: (1, 2, 3), stand: Board, facing: 0.0,
        conversations: [
            (when: [QuestDone("t")], node: "t.after"),
            (when: [QuestAt("t", "s1")], node: "t.hub"),
            (node: "t.hello"),
        ],
    )"#;
    const DIALOGUE: &str = r#"(
        version: 1,
        nodes: [
            (id: "t.hello", speaker: "veyra:person.t", line: "Hello, {assembly}.", replies: [
                (text: "Begin.", effects: [StartQuest("t")], next: Node("t.hub")),
            ]),
            (id: "t.hub", speaker: "veyra:person.t", line: "Well?", replies: [
                (text: "Pay up.", when: [Flag("on")], effects: [Pay(50), Earn(7)], next: End),
                (text: "Hand over.", effects: [Take("veyra:component.harness", 1)], next: End),
                (text: "Secret.", when: [PurseAtLeast(1000)], hidden: true, next: End),
                (text: "Greyed.", when: [Learned("blade.novice")], next: End),
                (text: "Set.", effects: [
                    SetFlag("on"),
                    If([Flag("on")], [Give("veyra:component.harness", 2), Deed(Keepers, Hurt, 3)]),
                    If([NotFlag("on")], [Hostile("c")]),
                    Ending("fin"),
                ], next: End),
            ]),
            (id: "t.after", speaker: "veyra:person.t", line: "Done.", replies: []),
        ],
    )"#;
    const QUEST: &str = r#"(
        version: 1, id: "t", title: "Test",
        crews: [(id: "c", at: Station, roster: [(kind: Rusher, count: 2)], hand: Some((name: "Hand", tint: (1, 2, 3))))],
        stages: [
            (id: "s1", journal: "One.", objectives: [
                Talk("veyra:person.t"), Reach(Station, 30), Defeat(Crew("c")),
                Defeat(Opponent(Sentry)), Learn, Ping,
                Sample(class: "strata:conductor", units: 45),
                Craft("veyra:component.harness"), Sell("veyra:component.harness"),
                Buy("veyra:vehicle.courier_bike"), Stable,
                Recruit("Ilse Marrow"), Switch("Tovan Reyes"),
                Deliver("veyra:component.harness", "veyra:person.t"),
            ], on_done: (next: Some("s2"))),
            (id: "s2", journal: "Two.", objectives: [Learn],
             on_done: (effects: [SetFlag("done")])),
        ],
        endings: [(id: "fin", journal: "Fin.")],
    )"#;

    fn fixture() -> QuestSet {
        let registries = shipped();
        let files = QuestFiles {
            quests: vec![("q".to_owned(), ron::from_str(QUEST).unwrap())],
            dialogues: vec![("d".to_owned(), ron::from_str(DIALOGUE).unwrap())],
            people: vec![("p".to_owned(), ron::from_str(PERSON).unwrap())],
        };
        QuestSet::build(
            files,
            &QuestContext {
                items: &registries.0.items,
                graph: &registries.0.progression,
                resources: &registries.0.resources,
                members: &[],
            },
        )
        .unwrap()
    }

    fn raise(log: &mut QuestLog, data: &QuestSet, event: &QuestEvent) -> Applied {
        advance(log, event, data, &Facts::default())
    }

    /// Every objective kind is met by its event and nothing else, a count
    /// fills over several events, and the stage goes on when all are met.
    #[test]
    fn every_objective_is_met_by_its_event() {
        use QuestEvent as E;
        let data = fixture();
        let mut log = QuestLog::default();
        apply(
            &[Effect::StartQuest("t".to_owned())],
            None,
            &mut log,
            &data,
            &Facts::default(),
        );
        assert_eq!(log.stage_of("t"), Some("s1"));
        let who = id("veyra:person.t");
        let harness = id("veyra:component.harness");
        // Wrong events change nothing.
        for wrong in [
            E::Talked(id("veyra:person.other")),
            E::Reached {
                place: Place::Station,
                distance_m: 31.0,
            },
            E::Reached {
                place: Place::Channel,
                distance_m: 1.0,
            },
            E::Sampled {
                class: id("strata:brine"),
                units: 99,
            },
            E::Defeated {
                kind: FoeKind::Rusher,
                crew: None,
            },
        ] {
            assert!(
                raise(&mut log, &data, &wrong).changes.is_empty(),
                "{wrong:?}"
            );
        }
        let crew = |kind| E::Defeated {
            kind,
            crew: Some("c".to_owned()),
        };
        let events = [
            E::Talked(who.clone()),
            E::Reached {
                place: Place::Station,
                distance_m: 29.0,
            },
            crew(FoeKind::Rusher),
            E::Defeated {
                kind: FoeKind::Sentry,
                crew: None,
            },
            E::Learned("blade.novice".to_owned()),
            E::Pinged,
            E::Sampled {
                class: id("strata:conductor"),
                units: 20,
            },
            E::Sampled {
                class: id("strata:conductor"),
                units: 25,
            },
            E::Crafted(harness.clone()),
            E::Sold(harness.clone()),
            E::Bought(id("veyra:vehicle.courier_bike")),
            E::Stabled,
            E::Recruited("Ilse Marrow".to_owned()),
            E::Switched("Tovan Reyes".to_owned()),
        ];
        for event in &events {
            raise(&mut log, &data, event);
        }
        // The crew has one of two down, and the last delivery is not made.
        assert_eq!(log.stage_of("t"), Some("s1"));
        assert_eq!(log.quests["t"].met.len(), 12);
        raise(&mut log, &data, &crew(FoeKind::Rusher));
        let last = raise(
            &mut log,
            &data,
            &E::Delivered {
                item: harness,
                person: who,
            },
        );
        assert_eq!(log.stage_of("t"), Some("s2"));
        assert!(last.changes.contains(&Change::Stage {
            quest: "t".to_owned(),
            stage: "s2".to_owned()
        }));
        // A stage entered starts with nothing met, and its on_done ends the
        // quest with no ending named.
        assert!(log.quests["t"].met.is_empty());
        let end = raise(&mut log, &data, &E::Learned("x".to_owned()));
        assert!(log.is_done("t") && log.flag("done"));
        assert!(end.changes.contains(&Change::Done("t".to_owned())));
    }

    /// Replies are shown, greyed with the first reason, or left out when
    /// hidden; a Pay and a Take need what they spend.
    #[test]
    fn replies_are_shown_greyed_or_hidden() {
        let data = fixture();
        let node = data.node("t.hub").unwrap();
        let log = QuestLog::default();
        let mut facts = Facts::default();
        let list = shown(node, &log, &facts);
        let by = |text: &str| {
            list.iter()
                .find(|r| r.text == text)
                .unwrap()
                .blocked
                .clone()
        };
        assert_eq!(list.len(), 4, "the hidden one is left out");
        assert_eq!(by("Pay up."), Some(Condition::Flag("on".to_owned())));
        assert_eq!(
            by("Hand over."),
            Some(Condition::Holds(id("veyra:component.harness"), 1))
        );
        assert_eq!(
            by("Greyed."),
            Some(Condition::Learned("blade.novice".to_owned()))
        );
        assert_eq!(by("Set."), None);
        facts.purse = 1000;
        facts.items.insert(id("veyra:component.harness"), 1);
        facts.learned.insert("blade.novice".to_owned());
        let list = shown(
            node,
            &QuestLog {
                flags: ["on".to_owned()].into(),
                ..log
            },
            &facts,
        );
        assert_eq!(list.len(), 5);
        assert!(list.iter().all(|reply| reply.blocked.is_none()));
        // A Pay is blocked by an empty purse even with its flag set.
        facts.purse = 10;
        let flagged = QuestLog {
            flags: ["on".to_owned()].into(),
            ..QuestLog::default()
        };
        let list = shown(node, &flagged, &facts);
        assert_eq!(
            list.iter().find(|r| r.text == "Pay up.").unwrap().blocked,
            Some(Condition::PurseAtLeast(50))
        );
    }

    /// Effects run in order on the log, ask the world through outcomes, and
    /// an `If` runs only when its conditions hold; an ending ends the quest.
    #[test]
    fn effects_apply_in_order_and_ask_the_world() {
        let data = fixture();
        let mut log = QuestLog::default();
        let start = apply(
            &[Effect::StartQuest("t".to_owned())],
            None,
            &mut log,
            &data,
            &Facts::default(),
        );
        assert_eq!(start.changes[0], Change::Started("t".to_owned()));
        let node = data.node("t.hub").unwrap();
        let (applied, next) = choose(
            node,
            3,
            &id("veyra:person.t"),
            &mut log,
            &data,
            &Facts::default(),
        )
        .unwrap();
        assert_eq!(*next, Next::End);
        assert_eq!(
            applied.outcomes,
            vec![
                Outcome::Give(id("veyra:component.harness"), 2),
                Outcome::Deed(Role::Keepers, DeedWord::Hurt, 3),
            ],
            "the NotFlag branch did not run"
        );
        assert!(log.flag("on") && log.flag("ending_fin") && log.is_done("t"));
        assert_eq!(log.ending.as_deref(), Some("fin"));
        assert_eq!(log.choices.len(), 1);
        // A blocked reply refuses and records nothing.
        let mut fresh = QuestLog::default();
        let refused = choose(
            node,
            2,
            &id("veyra:person.t"),
            &mut fresh,
            &data,
            &Facts::default(),
        );
        assert!(matches!(
            refused,
            Err(DialogueRefusal::Blocked(Condition::Learned(_)))
        ));
        assert!(
            choose(
                node,
                9,
                &id("veyra:person.t"),
                &mut fresh,
                &data,
                &Facts::default()
            )
            .is_err()
        );
        assert!(fresh.choices.is_empty());
    }

    /// A person opens the first conversation whose conditions hold, and the
    /// roles fill into a line.
    #[test]
    fn a_person_opens_by_condition_and_lines_are_filled() {
        let data = fixture();
        let person = data.person(&id("veyra:person.t")).unwrap();
        let mut log = QuestLog::default();
        let facts = Facts::default();
        assert_eq!(
            opening(&person.conversations, &log, &facts),
            Some("t.hello")
        );
        apply(
            &[Effect::StartQuest("t".to_owned())],
            None,
            &mut log,
            &data,
            &facts,
        );
        assert_eq!(opening(&person.conversations, &log, &facts), Some("t.hub"));
        log.quests.get_mut("t").unwrap().done = true;
        assert_eq!(
            opening(&person.conversations, &log, &facts),
            Some("t.after")
        );
        let mut cast = QuestCast::default();
        cast.names.insert(Role::Assembly, "Rue Assembly".to_owned());
        assert_eq!(
            fill("Hello, {assembly} and {holder}.", &cast),
            "Hello, Rue Assembly and holder."
        );
    }

    /// Plays the shipped questline through the pure rules: a talk picks the
    /// replies whose text starts with each of `picks`, the world's books are
    /// kept in `facts`, and events are raised as the sim would.
    struct Play {
        data: QuestSet,
        log: QuestLog,
        facts: Facts,
        deeds: Vec<(Role, DeedWord, u32)>,
        hostile: Vec<String>,
        graded: Vec<(Namespaced, BTreeMap<String, u16>)>,
        xp: Vec<(XpKind, u32)>,
    }

    impl Play {
        fn new() -> Self {
            Self {
                data: shipped().0.quests.clone(),
                log: QuestLog::default(),
                facts: Facts {
                    purse: 2500,
                    ..Facts::default()
                },
                deeds: Vec::new(),
                hostile: Vec::new(),
                graded: Vec::new(),
                xp: Vec::new(),
            }
        }

        fn world(&mut self, outcomes: Vec<Outcome>) {
            for outcome in outcomes {
                match outcome {
                    Outcome::Pay(n) => self.facts.purse -= n,
                    Outcome::Earn(n) => self.facts.purse += n,
                    Outcome::Give(item, n) => *self.facts.items.entry(item).or_default() += n,
                    Outcome::GiveGraded(item, n, lines) => {
                        *self.facts.items.entry(item.clone()).or_default() += n;
                        self.graded.push((item, lines));
                    }
                    Outcome::Take(item, n) => *self.facts.items.entry(item).or_default() -= n,
                    Outcome::TakeMade(item, n) => {
                        *self.facts.items.entry(item.clone()).or_default() -= n;
                        *self.facts.made.entry(item).or_default() -= n;
                    }
                    Outcome::Xp(kind, amount) => self.xp.push((kind, amount)),
                    Outcome::Deed(role, word, units) => self.deeds.push((role, word, units)),
                    Outcome::Hostile(target) => self.hostile.push(target),
                }
            }
        }

        fn event(&mut self, event: &QuestEvent) {
            let applied = advance(&mut self.log, event, &self.data, &self.facts);
            self.world(applied.outcomes);
        }

        /// The replies `person` opens with now, as shown.
        fn shown_by(&self, person: &str) -> Vec<Shown> {
            let def = self.data.person(&id(person)).unwrap();
            let node = opening(&def.conversations, &self.log, &self.facts).unwrap();
            shown(self.data.node(node).unwrap(), &self.log, &self.facts)
        }

        /// Open `person` and choose by text prefix until the conversation
        /// ends; returns the lines said.
        fn talk(&mut self, person: &str, picks: &[&str]) -> Vec<String> {
            let who = id(person);
            let def = self.data.person(&who).unwrap().clone();
            let mut node = self
                .data
                .node(opening(&def.conversations, &self.log, &self.facts).expect("an opening"))
                .unwrap()
                .clone();
            let mut said = vec![node.line.clone()];
            let mut picks = picks.iter();
            loop {
                let Some(pick) = picks.next() else {
                    return said;
                };
                let list = shown(&node, &self.log, &self.facts);
                let place = list
                    .iter()
                    .position(|reply| reply.text.starts_with(pick))
                    .unwrap_or_else(|| panic!("`{pick}` is not shown at {}", node.id));
                let (applied, next) =
                    choose(&node, place, &who, &mut self.log, &self.data, &self.facts).unwrap();
                let next = next.clone();
                self.world(applied.outcomes);
                match next {
                    Next::Node(next) => {
                        node = self.data.node(&next).unwrap().clone();
                        said.push(node.line.clone());
                    }
                    Next::End => {
                        self.event(&QuestEvent::Talked(who));
                        return said;
                    }
                }
            }
        }
    }

    /// The questline's whole structure plays to each ending with each of
    /// Pell's three branches and each of Wim's, by the pure rules alone:
    /// stages in order, the bike bought and stabled, the rewards and the
    /// deeds' direction right, the endings exclusive.
    #[test]
    #[allow(clippy::too_many_lines, reason = "the whole questline, once, in order")]
    fn the_shipped_questline_plays_to_each_ending() {
        use QuestEvent as E;
        let maren = "veyra:person.maren_coyle";
        let teague = "veyra:person.anouk_teague";
        let corin = "veyra:person.corin_hale";
        let pell = "veyra:person.pell";
        let wim = "veyra:person.wim_larrow";
        let mast = "veyra:person.station_mast";
        let pump = "veyra:person.wim_pump";
        let harness = id("veyra:component.harness");
        let record = id("veyra:item.station_record");
        for (ending, branch) in [
            ("keepers", "talk"),
            ("assembly", "pay"),
            ("holder", "fight"),
        ] {
            let mut play = Play::new();
            play.talk(maren, &["That's us"]);
            assert_eq!(play.log.stage_of("arrival"), Some("landfall"));
            assert_eq!(play.facts.purse, 2700, "the borough's allowance");
            play.talk(maren, &["Where should we go first?", "We'll go"]);
            assert_eq!(play.log.stage_of("arrival"), Some("steady_hand"));
            let asked = branch != "pay";
            if asked {
                play.talk(teague, &["What does", "We'll be back"]);
            } else {
                play.talk(teague, &["We'll be back"]);
            }
            play.event(&E::Defeated {
                kind: FoeKind::Sentry,
                crew: None,
            });
            play.event(&E::Learned("blade.cuts.1".to_owned()));
            assert_eq!(play.log.stage_of("arrival"), Some("quiet_station"));
            play.talk(corin, &["We'll take the newcomer's credit", "Thanks"]);
            assert!(play.log.flag("newcomer_credit") && play.facts.purse == 2850);
            play.talk(teague, &["How do we get out there?", "We'll see Corin"]);
            play.talk(teague, &["We're on our way"]);
            // The station is reached before the bike is bought: the stage
            // waits for the bike.
            play.event(&E::Reached {
                place: Place::Station,
                distance_m: 20.0,
            });
            assert_eq!(play.log.stage_of("arrival"), Some("quiet_station"));
            play.event(&E::Bought(id("veyra:vehicle.courier_bike")));
            play.facts.purse -= 450;
            play.event(&E::Stabled);
            assert_eq!(play.log.stage_of("arrival"), Some("pell"));
            // Without Teague's word on the intake the river reply is greyed,
            // in the world's words.
            let river = play
                .shown_by(pell)
                .into_iter()
                .find(|reply| reply.text.starts_with("Teague says the processor's intake"))
                .unwrap();
            assert_eq!(river.blocked.is_some(), !asked);
            assert_eq!(river.why.is_some(), !asked);
            match branch {
                "fight" => {
                    play.talk(pell, &["Hand over"]);
                    assert_eq!(play.log.stage_of("arrival"), Some("pell_fight"));
                    assert_eq!(play.hostile, ["pell"]);
                    assert!(play.log.hostile.contains("pell"), "the crossing is kept");
                    let rusher = E::Defeated {
                        kind: FoeKind::Rusher,
                        crew: Some("pell".to_owned()),
                    };
                    play.event(&rusher);
                    assert_eq!(play.log.stage_of("arrival"), Some("pell_fight"));
                    play.event(&rusher);
                    assert!(play.deeds.contains(&(Role::Assembly, DeedWord::Hurt, 1)));
                }
                "pay" => {
                    let before = play.facts.purse;
                    play.talk(pell, &["Two hundred", "Nobody will"]);
                    assert_eq!(play.facts.purse, before - 200);
                    assert!(!play.log.flag("array"));
                }
                _ => {
                    play.talk(
                        pell,
                        &["Teague says the processor's intake", "That's worth"],
                    );
                    assert!(play.log.flag("array") && play.log.flag("core"));
                }
            }
            assert_eq!(play.log.stage_of("arrival"), Some("mending"));
            assert!(play.log.flag("core"));
            for event in [
                E::Sampled {
                    class: id("strata:conductor"),
                    units: 45,
                },
                E::Recruited("Ilse Marrow".to_owned()),
                E::Switched("Tovan Reyes".to_owned()),
                E::Crafted(harness.clone()),
            ] {
                play.event(&event);
            }
            assert_eq!(play.log.stage_of("arrival"), Some("mending"));
            // A store harness is no use to the mast: the fit is greyed, with
            // its reason, until one made at a bench is carried.
            play.facts.items.insert(harness.clone(), 1);
            let fit = play.shown_by(mast).remove(0);
            assert_eq!(
                fit.blocked,
                Some(Condition::CarriesMade(harness.clone(), 1))
            );
            assert!(fit.why.is_some());
            play.facts.items.insert(harness.clone(), 2);
            play.facts.made.insert(harness.clone(), 1);
            play.talk(mast, &["Fit the harness", "Copy the readings"]);
            assert_eq!(play.log.stage_of("arrival"), Some("upstream"));
            assert!(play.log.flag("station_restored"));
            assert_eq!(play.facts.made[&harness], 0, "the made one went");
            assert_eq!(play.facts.items[&harness], 1, "the store one stayed");
            match ending {
                "keepers" => {
                    play.talk(wim, &["How much", "We'll decide"]);
                    play.talk(pump, &["Close the valve"]);
                    assert!(play.log.flag("pump_shut"));
                }
                "assembly" => {
                    play.talk(wim, &["We'll leave your pump running", "We'll tell them"]);
                    assert!(play.log.flag("pump_left"));
                }
                _ => {
                    play.talk(wim, &["Shut the channel now"]);
                    assert_eq!(play.log.stage_of("arrival"), Some("wim_fight"));
                    play.event(&E::Defeated {
                        kind: FoeKind::Rusher,
                        crew: Some("wim_larrow".to_owned()),
                    });
                    assert!(play.log.flag("wim_fallen"));
                    assert!(play.deeds.contains(&(Role::Assembly, DeedWord::Killed, 1)));
                }
            }
            assert_eq!(play.log.stage_of("arrival"), Some("who_hears"));
            let purse = play.facts.purse;
            let before = play.deeds.len();
            match ending {
                "keepers" => {
                    play.talk(teague, &["Publish it", "Thank you"]);
                }
                "assembly" => {
                    play.talk(maren, &["Take the record", "Good luck"]);
                }
                _ => {
                    play.talk(corin, &["Sell it to", "We'll take it"]);
                    assert_eq!(play.log.stage_of("arrival"), Some("selling"));
                    assert_eq!(play.facts.items[&record], 1, "the slate to sell");
                    play.event(&E::Sold(record.clone()));
                }
            }
            assert_eq!(play.log.ending.as_deref(), Some(ending));
            assert!(play.log.is_done("arrival"));
            let deeds = &play.deeds[before..];
            match ending {
                "keepers" => {
                    assert!(deeds.contains(&(Role::Keepers, DeedWord::JobDone, 2)));
                    assert!(deeds.contains(&(Role::Holder, DeedWord::Hurt, 1)));
                    assert!(
                        deeds.contains(&(Role::Holder, DeedWord::Hurt, 2)),
                        "the array"
                    );
                    assert_eq!(
                        play.graded,
                        [(id("veyra:tool.survey"), [("range".to_owned(), 700)].into())]
                    );
                    assert_eq!(play.xp, [(XpKind::Survey, 300)]);
                }
                "assembly" => {
                    assert!(deeds.contains(&(Role::Assembly, DeedWord::JobDone, 2)));
                    assert_eq!(play.facts.items[&id("veyra:station.field_harvester")], 1);
                }
                _ => {
                    assert_eq!(play.facts.purse, purse, "the board pays, not the reply");
                    assert!(deeds.contains(&(Role::Holder, DeedWord::JobDone, 2)));
                    assert!(deeds.contains(&(Role::Keepers, DeedWord::Hurt, 2)));
                }
            }
            // Afterwards each of the three says a line of its own to this ending.
            let lines: Vec<String> = [maren, teague, corin]
                .iter()
                .map(|person| play.talk(person, &[]).remove(0))
                .collect();
            assert!(lines.iter().all(|line| !line.is_empty()));
            assert_eq!(
                lines
                    .iter()
                    .collect::<std::collections::BTreeSet<_>>()
                    .len(),
                3,
                "{ending}: {lines:?}"
            );
        }
    }

    /// The skip leaves at once with the allowance and the other ending's
    /// flags unset; Wim's threat is a fight whose fall is a killed deed
    /// against the assembly; a reward with no room is greyed, not lost.
    #[test]
    fn skip_threat_and_a_full_pack() {
        let mut play = Play::new();
        play.talk("veyra:person.maren_coyle", &["We've done this before"]);
        assert_eq!(play.log.ending.as_deref(), Some("skip"));
        assert_eq!(play.facts.purse, 2700);
        assert!(play.log.is_done("arrival") && play.log.flag("ending_skip"));
        let mut play = Play::new();
        play.log.quests.insert(
            "arrival".to_owned(),
            QuestState {
                stage: Some("upstream".to_owned()),
                ..QuestState::default()
            },
        );
        play.talk("veyra:person.wim_larrow", &["Shut the channel now"]);
        assert_eq!(play.hostile, ["wim_larrow"]);
        assert!(play.deeds.contains(&(Role::Assembly, DeedWord::Hurt, 1)));
        assert_eq!(play.log.stage_of("arrival"), Some("wim_fight"));
        play.event(&QuestEvent::Defeated {
            kind: FoeKind::Rusher,
            crew: Some("wim_larrow".to_owned()),
        });
        assert!(play.deeds.contains(&(Role::Assembly, DeedWord::Killed, 1)));
        assert_eq!(play.log.stage_of("arrival"), Some("who_hears"));
        // The borough's harvester has nowhere to go: Maren's reply is greyed
        // by the room it needs, and choosing it is refused.
        play.facts
            .full
            .insert(id("veyra:station.field_harvester"), 1);
        let take = play
            .shown_by("veyra:person.maren_coyle")
            .into_iter()
            .find(|reply| reply.text.starts_with("Take the record"))
            .unwrap();
        assert_eq!(
            take.blocked,
            Some(Condition::Room(id("veyra:station.field_harvester"), 1))
        );
        assert!(take.why.is_none(), "the screen words a room it needs");
        // Teague's keepers reply is hidden by its flags, never by the room:
        // with no room it stays on the screen, greyed.
        play.facts.full.insert(id("veyra:tool.survey"), 1);
        let publish = play
            .shown_by("veyra:person.anouk_teague")
            .into_iter()
            .find(|reply| reply.text.starts_with("Publish it"))
            .expect("the keepers reply shows with the pack full");
        assert_eq!(
            publish.blocked,
            Some(Condition::Room(id("veyra:tool.survey"), 1))
        );
    }

    /// The quest places on seed 42: dry ground near the dial's distance from
    /// the city and a bank within the dial's band of the station.
    #[test]
    fn the_places_stand_on_seed_42() {
        let registries = shipped();
        let rules = registries.0.quests.rules().expect("places.ron").clone();
        let world =
            Arc::new(ProceduralWorld::new(strata_core::Seed(42), &registries.0).with_own_water());
        world.hydrology().set_threads(4);
        let city = world.sites()[0].centre;
        world.prepare_water(city[0], city[1], 20_000.0);
        let began = std::time::Instant::now();
        let found = places(&world, &rules, city);
        println!("places took {:?}", began.elapsed());
        let station = found.station.expect("a station");
        let channel = found.channel.expect("a channel");
        let to_station = metres(city, station);
        let to_channel = metres(station, channel);
        println!(
            "station {to_station:.0} m from the city, channel {to_channel:.0} m from the station"
        );
        assert!(
            (rules.station_m - 1.0..=rules.station_m + rules.station_slack_m + 1.0)
                .contains(&to_station),
            "{to_station}"
        );
        assert!(
            (rules.channel_near_m..=rules.channel_far_m).contains(&to_channel),
            "{to_channel}"
        );
        assert_eq!(
            places(&world, &rules, city),
            found,
            "a pure function of the world"
        );
        println!("channel {:.0} m from the city", metres(city, channel));
        let camp = camp(&*world, &rules, station, city).expect("a camp");
        assert!(metres(station, camp.terminal) <= rules.camp_back_m + 0.5);
        for at in [camp.terminal, camp.bench] {
            let sample = strata_worldgen::WorldSource::sample(&*world, at[0], at[1]);
            assert!(sample.water.is_none() && sample.landform.developed <= 0.0);
            assert!(sample.landform.slope <= rules.max_slope);
        }
    }

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
            .0
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
            .0
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
            .0
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
            .0
            .push(DialogueRequest::Close);
        tick(&mut app);
        assert!(app.world().resource::<OpenDialogue>().0.is_none());
    }
}
