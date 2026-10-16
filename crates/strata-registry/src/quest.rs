//! The first questline's data (ticket 0014, step 1): people, conversations
//! and quests, three file kinds that carry every word and every rule of a
//! story, so a quest is data and never code.
//!
//! - `people/*.person.ron`: who stands where, whose role they speak for and
//!   which conversation they open by condition (first match).
//! - `dialogue/*.dialogue.ron`: a graph of nodes, each a speaker and a line,
//!   whose replies carry conditions and effects.
//! - `quests/*.quest.ron`: stages in order, each with a journal line, its
//!   objectives and what it does when they are all met.
//!
//! The sim's `quests` module plays them; this crate holds the shapes and the
//! checks. A node nobody can reach, a reply that leads nowhere, an ending no
//! conversation can end in or a name that does not exist is refused at load
//! with the file and the id. In no world version and no handshake.

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};
use slotted_model::Namespaced;

use crate::encounter::Person as Foe;
use crate::item::{ItemCatalogue, QUALITY_MAX};
use crate::progression::{SkillGraph, XpKind};
use crate::resource::ResourceSet;

/// The directory of `.quest.ron` files, relative to the asset root.
pub const QUESTS_DIR: &str = "quests";
/// The suffix a quest file must carry to be read.
pub const QUEST_SUFFIX: &str = ".quest.ron";
/// The directory of `.dialogue.ron` files.
pub const DIALOGUE_DIR: &str = "dialogue";
/// The suffix a dialogue file must carry to be read.
pub const DIALOGUE_SUFFIX: &str = ".dialogue.ron";
/// The directory of `.person.ron` files.
pub const PEOPLE_DIR: &str = "people";
/// The suffix a person file must carry to be read.
pub const PERSON_SUFFIX: &str = ".person.ron";
/// The only version of the three files this build reads.
pub const QUEST_VERSION: u32 = 1;

/// The faction roles a line may name between braces: `{holder}` is the
/// city's holder, `{assembly}` and `{keepers}` the nearest assembly and
/// conservation factions to the city. The sim fills them from the world.
pub const PLACEHOLDERS: [&str; 3] = ["holder", "assembly", "keepers"];

/// Whose side a person speaks for. The factions are made per world, so data
/// names a role and the world says which faction fills it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub enum Role {
    /// The company that holds the city.
    Holder,
    /// The nearest settlers' assembly.
    Assembly,
    /// The nearest conservation body.
    Keepers,
    /// Nobody's: a living, not a cause.
    Unaligned,
}

impl Role {
    /// The placeholder word of the role, where it has one.
    #[must_use]
    pub const fn word(self) -> Option<&'static str> {
        match self {
            Self::Holder => Some("holder"),
            Self::Assembly => Some("assembly"),
            Self::Keepers => Some("keepers"),
            Self::Unaligned => None,
        }
    }
}

/// A named place a person stands or an objective points to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub enum Place {
    /// Port Serein's quay terrace.
    Quay,
    /// The Exchange.
    Exchange,
    /// Pelagic's board.
    Board,
    /// The monitoring station, past the road's end.
    Station,
    /// Wim's channel, off the river.
    Channel,
    /// The training ground at the city's edge, where the staging fight
    /// stands; a household that arrives on the quay has to be led to it.
    Training,
}

/// What a deed was, named as the sim's `DeedKind` is; the registry sits
/// below the sim, so the names are held here and the sim maps them.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub enum DeedWord {
    /// Units worked on a concession without a lease.
    WorkedConcession,
    /// Units worked on a reserve.
    WorkedReserve,
    /// Units taken from a commons past the share.
    TookPastShare,
    /// A harvester planted without leave.
    Planted,
    /// Credits paid at a board.
    Sold,
    /// An outlaw camp cleared.
    ClearedCamp,
    /// A person struck.
    Hurt,
    /// A person killed.
    Killed,
    /// A posting collected.
    JobDone,
}

/// A fact about the household a reply or a conversation may need.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum Condition {
    /// The quest is at this stage.
    QuestAt(String, String),
    /// The quest is over.
    QuestDone(String),
    /// A flag is set.
    Flag(String),
    /// A flag is not set.
    NotFlag(String),
    /// The purse holds at least this.
    PurseAtLeast(u32),
    /// At least this many of an item are carried or stored.
    Holds(Namespaced, u32),
    /// The active member has learned this box.
    Learned(String),
    /// The pack, its bags or the store have room for this many of an item:
    /// what a `Give` needs, so a gift is never paid for and then lost.
    Room(Namespaced, u32),
    /// At least this many of an item made at a bench (a record with a batch
    /// and a maker, never store grade) are carried in the pack or its bags.
    CarriesMade(Namespaced, u32),
}

/// What a reply does when it is chosen.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum Effect {
    /// Begin a quest at its first stage.
    StartQuest(String),
    /// Move a quest to a stage.
    SetStage(String, String),
    /// Set a flag.
    SetFlag(String),
    /// Put items in the pack, store grade.
    Give(Namespaced, u32),
    /// Put items in the pack with these lines: a record above store grade
    /// that no bench made.
    GiveGraded(Namespaced, u32, BTreeMap<String, u16>),
    /// Take items from the pack or the store.
    Take(Namespaced, u32),
    /// Take items made at a bench from the pack alone: a thing that must be
    /// carried there and made for the job, not bought off a shelf.
    TakeMade(Namespaced, u32),
    /// The active member is credited with XP of a kind.
    Xp(XpKind, u32),
    /// The household pays credits, which the reply needs in the purse.
    Pay(u32),
    /// The household is paid credits.
    Earn(u32),
    /// A deed against or for the faction in a role, by units.
    Deed(Role, DeedWord, u32),
    /// Turn a quest crew, or a person, on the household.
    Hostile(String),
    /// The questline ends this way: sets the quest's ending and the flag
    /// `ending_<name>`.
    Ending(String),
    /// Run these effects only if every condition holds.
    If(Vec<Condition>, Vec<Effect>),
}

/// Where a reply leads.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum Next {
    /// To another node.
    Node(String),
    /// The conversation ends.
    End,
}

/// One thing a player can say.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Reply {
    /// What is said, with placeholders.
    pub text: String,
    /// Every one must hold for the reply to be open.
    #[serde(default)]
    pub when: Vec<Condition>,
    /// A reply whose conditions fail is left out, not greyed.
    #[serde(default)]
    pub hidden: bool,
    /// What choosing it does, in order.
    #[serde(default)]
    pub effects: Vec<Effect>,
    /// Where it leads.
    pub next: Next,
    /// Why it is greyed, in the world's words, when one of `when` fails:
    /// "You have not asked Teague about the intake". What a `Pay` or a `Give`
    /// needs is worded by the screen.
    #[serde(default)]
    pub why: Option<String>,
}

/// One speech and the replies to it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Node {
    /// Unique over every dialogue file.
    pub id: String,
    /// Who speaks: a person's id.
    pub speaker: Namespaced,
    /// The line, with placeholders.
    pub line: String,
    /// What can be said back. None ends the conversation after the line.
    #[serde(default)]
    pub replies: Vec<Reply>,
}

/// One dialogue file.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DialogueDef {
    /// Must equal [`QUEST_VERSION`].
    pub version: u32,
    /// The nodes.
    pub nodes: Vec<Node>,
}

/// A conversation a person opens when its conditions hold.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Opening {
    /// Every one must hold. Empty always holds.
    #[serde(default)]
    pub when: Vec<Condition>,
    /// The first node.
    pub node: String,
}

/// One person file.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PersonDef {
    /// Must equal [`QUEST_VERSION`].
    pub version: u32,
    /// `veyra:person.maren_coyle`.
    pub id: Namespaced,
    /// The name on the plate.
    pub name: String,
    /// What the plate says under it, with placeholders.
    pub title: String,
    /// Whose side they speak for.
    pub role: Role,
    /// The body's colour, sRGB bytes.
    pub tint: [u8; 3],
    /// Where they stand.
    pub stand: Place,
    /// The way they face, radians about up, 0 along +z.
    pub facing: f32,
    /// A talkable thing with no body: the pump, the mast. The game stands a
    /// prop in its place and the plate is its name.
    #[serde(default)]
    pub prop: bool,
    /// What they open, first match.
    pub conversations: Vec<Opening>,
}

/// What a quest's stage waits for.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum Objective {
    /// A conversation with the person reaches its end.
    Talk(Namespaced),
    /// Come within the radius of a place, metres.
    Reach(Place, u32),
    /// Beat an opponent kind or a quest crew.
    Defeat(Beat),
    /// Learn any box.
    Learn,
    /// Ping the ground.
    Ping,
    /// Sample this many units of a resource class.
    Sample {
        /// The class, `strata:conductor`.
        class: Namespaced,
        /// Units, over all samplings.
        units: u32,
    },
    /// Finish making an item at the bench.
    Craft(Namespaced),
    /// Sell an item at a board.
    Sell(Namespaced),
    /// Buy an item at a board.
    Buy(Namespaced),
    /// Stable a bike.
    Stable,
    /// Recruit a member of the bench, by name.
    Recruit(String),
    /// Switch to a member, by name.
    Switch(String),
    /// Hand an item over to a person (a Take effect of their reply).
    Deliver(Namespaced, Namespaced),
}

/// Who must fall for a Defeat objective.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum Beat {
    /// One of an opponent kind, anywhere.
    Opponent(crate::encounter::FoeKind),
    /// Every person of a quest crew.
    Crew(String),
}

/// What a stage does when all its objectives are met.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Done {
    /// Run in order.
    #[serde(default)]
    pub effects: Vec<Effect>,
    /// The stage that follows. None leaves the quest at its end with no
    /// ending named.
    #[serde(default)]
    pub next: Option<String>,
}

/// One stage of a quest.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StageDef {
    /// Unique in the quest.
    pub id: String,
    /// The journal's line while it is the stage, with placeholders.
    pub journal: String,
    /// Every one must be met. A stage with none waits for a reply's
    /// `SetStage` or `Ending`.
    #[serde(default)]
    pub objectives: Vec<Objective>,
    /// What happens when they are.
    #[serde(default)]
    pub on_done: Done,
}

/// A group of people a quest stands at a place, who stay neutral until a
/// reply turns them.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CrewDef {
    /// Unique in the quest.
    pub id: String,
    /// Where they stand.
    pub at: Place,
    /// Who they are, as an encounter's roster says.
    pub roster: Vec<Foe>,
    /// The flag that sends them away: while it is unset they stand, and the
    /// game folds them when it is set. A crew led by `veyra:person.<id>`
    /// has that person for its first body.
    #[serde(default)]
    pub gone: Option<String>,
    /// The plate and colour of the bodies past the leader, who are nobody
    /// in particular. A crew of its leader alone needs none.
    #[serde(default)]
    pub hand: Option<CrewHand>,
}

/// The plate and colour a crew's nameless bodies stand with.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CrewHand {
    /// What the plate says.
    pub name: String,
    /// The body's colour, sRGB bytes.
    pub tint: [u8; 3],
}

/// One way the quest can end.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EndingDef {
    /// The name an `Ending` effect gives.
    pub id: String,
    /// The last journal line, with placeholders.
    pub journal: String,
}

/// One quest file.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct QuestDef {
    /// Must equal [`QUEST_VERSION`].
    pub version: u32,
    /// The quest's id.
    pub id: String,
    /// What the journal calls it, with placeholders.
    pub title: String,
    /// Who it stands at places.
    #[serde(default)]
    pub crews: Vec<CrewDef>,
    /// In order; the first is where it starts.
    pub stages: Vec<StageDef>,
    /// The ways it ends.
    pub endings: Vec<EndingDef>,
}

impl QuestDef {
    /// The stage `id`.
    #[must_use]
    pub fn stage(&self, id: &str) -> Option<&StageDef> {
        self.stages.iter().find(|stage| stage.id == id)
    }

    /// The crew `id`.
    #[must_use]
    pub fn crew(&self, id: &str) -> Option<&CrewDef> {
        self.crews.iter().find(|crew| crew.id == id)
    }

    /// The ending `id`.
    #[must_use]
    pub fn ending(&self, id: &str) -> Option<&EndingDef> {
        self.endings.iter().find(|ending| ending.id == id)
    }

    /// How many people a crew holds, for a Defeat objective.
    #[must_use]
    pub fn crew_size(&self, id: &str) -> u32 {
        self.crew(id).map_or(0, |crew| {
            crew.roster
                .iter()
                .map(|person| u32::from(person.count))
                .sum()
        })
    }
}

/// Where the file of the quest places' dials lives under the assets root.
pub const PLACES_FILE: &str = "quests/places.ron";

/// The dials that find the station and Wim's channel: a pure function of
/// the world, so these numbers are the only place the distances live.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PlaceRules {
    /// Must equal [`QUEST_VERSION`].
    pub version: u32,
    /// How far from the city the station stands, metres.
    pub station_m: f64,
    /// How much farther the search may range for ground with water at the
    /// channel's distance.
    pub station_slack_m: f64,
    /// The step between rings of the station search, metres.
    pub station_step_m: f64,
    /// Bearings tried on a ring.
    pub bearings: u32,
    /// The steepest slope the station may stand on, rise over run.
    pub max_slope: f32,
    /// The lowest the station's ground may be, metres over the sea.
    pub min_height_m: f32,
    /// The station stands no nearer the road than this, metres.
    pub road_gap_m: f64,
    /// How far the road is looked for, metres.
    pub road_reach_m: f64,
    /// The nearest the channel may be to the station, metres.
    pub channel_near_m: f64,
    /// The farthest, metres.
    pub channel_far_m: f64,
    /// The step of the coarse scan for water, metres.
    pub scan_step_m: f64,
    /// The step of the fine scan for a bank round water the coarse scan
    /// found, metres.
    pub refine_step_m: f64,
    /// How far past the water's edge a bank cell may be, metres.
    pub bank_margin_m: f32,
    /// How far from the city a faction's seat is looked for to fill a role,
    /// metres.
    pub cast_reach_m: f64,
    /// Where on the quay the household arrives, metres along the strip from
    /// the foot of the Exchange's stair (positive toward the warehouse) and
    /// back from the water's edge. The quay is stacked with cargo; this is
    /// the open paving, found by looking.
    #[serde(default)]
    pub quay_along_m: f64,
    /// How far back from the water's edge, metres.
    #[serde(default)]
    pub quay_inset_m: f64,
    /// How far down the Exchange's stair the quay is looked for before the
    /// household arrives at the Exchange instead, metres.
    pub quay_search_m: f64,
    /// Ground this close over the sea is the water's edge, metres.
    pub shore_m: f64,
    /// Where Maren stands from the arriving household, which faces the sea:
    /// this far ahead, toward the water, and this far to its right, so the
    /// camera behind the household sees her face past its shoulder.
    pub greeter_ahead_m: f64,
    /// How far to the right of the household's line of sight, metres.
    pub greeter_aside_m: f64,
    /// How far before the Exchange's bench place Teague stands, metres.
    pub exchange_before_m: f64,
    /// How far along the row from the board Corin stands, metres.
    pub board_along_m: f64,
    /// How near the household a person is stood, metres.
    pub stand_m: f64,
    /// How far before a standing person is folded again, metres: a body left
    /// standing where the ground has unloaded would fall through the world.
    pub fold_m: f64,
    /// How far beside each other the people of one place stand, metres: the
    /// plates and the F of one must not cover the next.
    pub spread_m: f64,
    /// How far from its leader a crew's other bodies stand, metres.
    pub crew_ring_m: f64,
    /// How far back from the mast, toward the city, Teague's field camp
    /// stands, metres.
    pub camp_back_m: f64,
    /// How far the camp's workbench stands from its terminal, metres.
    pub camp_spacing_m: f64,
    /// How far from a quest place a `--quest-stage` still starts, metres.
    pub still_m: f64,
    /// How far over the household's own height the HUD's objective marker
    /// stands, metres: no ground is known at a person's stand, so it only
    /// points the way.
    pub marker_rise_m: f64,
}

impl PlaceRules {
    /// Parse the file's text.
    ///
    /// # Errors
    /// RON that does not fit, another version or a dial that breaks its rule.
    pub fn parse(text: &str) -> Result<Self, String> {
        let rules: Self = ron::from_str(text).map_err(|e| e.to_string())?;
        if rules.version != QUEST_VERSION {
            return Err(format!(
                "version {}, this build reads {QUEST_VERSION}",
                rules.version
            ));
        }
        let metres = [
            rules.station_m,
            rules.station_step_m,
            rules.road_reach_m,
            rules.channel_near_m,
            rules.channel_far_m,
            rules.scan_step_m,
            rules.refine_step_m,
            rules.cast_reach_m,
            rules.quay_search_m,
            rules.shore_m,
            rules.stand_m,
            rules.fold_m,
            rules.spread_m,
            rules.camp_back_m,
            rules.camp_spacing_m,
            rules.still_m,
            rules.marker_rise_m,
        ];
        if metres.iter().any(|dial| !dial.is_finite() || *dial <= 0.0) {
            return Err("every distance is over nought".to_owned());
        }
        if rules.channel_far_m <= rules.channel_near_m || rules.station_slack_m < 0.0 {
            return Err(
                "`channel_far_m` is past `channel_near_m` and the slack is not negative".to_owned(),
            );
        }
        if rules.bearings < 8 {
            return Err("`bearings` is at least 8".to_owned());
        }
        if rules.fold_m <= rules.stand_m {
            return Err("`fold_m` is past `stand_m`, or a body would flicker".to_owned());
        }
        let offsets = [
            rules.greeter_ahead_m,
            rules.greeter_aside_m,
            rules.exchange_before_m,
            rules.board_along_m,
            rules.crew_ring_m,
        ];
        if offsets.iter().any(|dial| !dial.is_finite()) {
            return Err("every offset is a number".to_owned());
        }
        Ok(rules)
    }
}

/// Why a file was refused.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum QuestError {
    /// Not valid RON for the file.
    #[error("{0}")]
    Ron(String),
    /// A file of another version than this build reads.
    #[error("version {found}, this build reads {expected}")]
    Version {
        /// The version the file states.
        found: u32,
        /// The version this build reads.
        expected: u32,
    },
    /// Something in the file breaks a rule.
    #[error("`{id}`: {reason}")]
    Rule {
        /// The quest, person or node.
        id: String,
        /// What is wrong.
        reason: String,
    },
}

fn rule(id: impl ToString, reason: impl Into<String>) -> QuestError {
    QuestError::Rule {
        id: id.to_string(),
        reason: reason.into(),
    }
}

/// The three kinds of file as read, each with the path it came from.
#[derive(Debug, Clone, Default)]
pub struct QuestFiles {
    /// `quests/*.quest.ron`.
    pub quests: Vec<(String, QuestDef)>,
    /// `dialogue/*.dialogue.ron`.
    pub dialogues: Vec<(String, DialogueDef)>,
    /// `people/*.person.ron`.
    pub people: Vec<(String, PersonDef)>,
}

/// What the files are held to: the data they name.
#[derive(Debug, Clone, Copy)]
pub struct QuestContext<'a> {
    /// Every item.
    pub items: &'a ItemCatalogue,
    /// Every box.
    pub graph: &'a SkillGraph,
    /// Every resource class.
    pub resources: &'a ResourceSet,
    /// The bench's names: the arrival's recruits. Empty where there are none,
    /// which leaves a member's name unchecked.
    pub members: &'a [String],
}

/// Every person, conversation and quest, checked.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct QuestSet {
    rules: Option<PlaceRules>,
    quests: Vec<QuestDef>,
    people: Vec<PersonDef>,
    nodes: BTreeMap<String, Node>,
}

impl QuestSet {
    /// Check every file against the others and against `ctx`.
    ///
    /// # Errors
    /// The path of the first file refused, and why.
    pub fn build(files: QuestFiles, ctx: &QuestContext<'_>) -> Result<Self, (String, QuestError)> {
        let mut set = Self::default();
        let mut node_path: BTreeMap<String, String> = BTreeMap::new();
        let mut person_path: BTreeMap<Namespaced, String> = BTreeMap::new();
        let mut quest_path: BTreeMap<String, String> = BTreeMap::new();
        for (path, def) in files.people {
            version(&path, def.version)?;
            if person_path.insert(def.id.clone(), path.clone()).is_some() {
                return Err((path, rule(&def.id, "is defined twice")));
            }
            set.people.push(def);
        }
        for (path, def) in files.dialogues {
            version(&path, def.version)?;
            for node in def.nodes {
                if node_path.insert(node.id.clone(), path.clone()).is_some() {
                    return Err((path, rule(&node.id, "is defined twice")));
                }
                set.nodes.insert(node.id.clone(), node);
            }
        }
        for (path, def) in files.quests {
            version(&path, def.version)?;
            if quest_path.insert(def.id.clone(), path.clone()).is_some() {
                return Err((path, rule(&def.id, "is defined twice")));
            }
            set.quests.push(def);
        }
        set.quests.sort_by(|a, b| a.id.cmp(&b.id));
        set.people.sort_by(|a, b| a.id.cmp(&b.id));
        for person in &set.people {
            set.check_person(person, ctx)
                .map_err(|e| (at(&person_path, &person.id), e))?;
        }
        for (id, node) in &set.nodes {
            set.check_node(node, ctx)
                .map_err(|e| (at(&node_path, id), e))?;
        }
        for quest in &set.quests {
            set.check_quest(quest, ctx)
                .map_err(|e| (at(&quest_path, &quest.id), e))?;
        }
        set.check_reach(&node_path, &quest_path)?;
        set.check_flags(&node_path)?;
        Ok(set)
    }

    /// The dials that place the station and the channel, where the data has
    /// them.
    #[must_use]
    pub const fn rules(&self) -> Option<&PlaceRules> {
        self.rules.as_ref()
    }

    /// The set with its place dials.
    #[must_use]
    pub fn with_rules(mut self, rules: PlaceRules) -> Self {
        self.rules = Some(rules);
        self
    }

    /// Every quest, by id.
    #[must_use]
    pub fn quests(&self) -> &[QuestDef] {
        &self.quests
    }

    /// The quest `id`.
    #[must_use]
    pub fn quest(&self, id: &str) -> Option<&QuestDef> {
        self.quests.iter().find(|quest| quest.id == id)
    }

    /// Every person, by id.
    #[must_use]
    pub fn people(&self) -> &[PersonDef] {
        &self.people
    }

    /// The person `id`.
    #[must_use]
    pub fn person(&self, id: &Namespaced) -> Option<&PersonDef> {
        self.people.iter().find(|person| person.id == *id)
    }

    /// The node `id`.
    #[must_use]
    pub fn node(&self, id: &str) -> Option<&Node> {
        self.nodes.get(id)
    }

    /// Every node, by id.
    pub fn nodes(&self) -> impl ExactSizeIterator<Item = &Node> {
        self.nodes.values()
    }

    /// Whether the data set has no questline.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.quests.is_empty() && self.people.is_empty()
    }

    fn check_person(&self, person: &PersonDef, ctx: &QuestContext<'_>) -> Result<(), QuestError> {
        let id = &person.id;
        if person.name.trim().is_empty() {
            return Err(rule(id, "has no name"));
        }
        if !person.facing.is_finite() {
            return Err(rule(id, "`facing` is not a number"));
        }
        check_words(id, &person.title)?;
        if person.conversations.is_empty() {
            return Err(rule(id, "opens no conversation"));
        }
        for opening in &person.conversations {
            if !self.nodes.contains_key(&opening.node) {
                return Err(rule(
                    id,
                    format!("opens `{}`, which is no node", opening.node),
                ));
            }
            for condition in &opening.when {
                self.check_condition(id, condition, ctx)?;
            }
        }
        Ok(())
    }

    fn check_node(&self, node: &Node, ctx: &QuestContext<'_>) -> Result<(), QuestError> {
        let id = &node.id;
        if self.person(&node.speaker).is_none() {
            return Err(rule(
                id,
                format!("`{}` speaks and is no person", node.speaker),
            ));
        }
        check_words(id, &node.line)?;
        for (index, reply) in node.replies.iter().enumerate() {
            let at = format!("{id} reply {}", index + 1);
            check_words(&at, &reply.text)?;
            if let Some(why) = &reply.why {
                if reply.when.is_empty() {
                    return Err(rule(
                        at,
                        "says why it is greyed and has no `when` to grey it",
                    ));
                }
                check_words(&at, why)?;
            }
            if let Next::Node(next) = &reply.next
                && !self.nodes.contains_key(next)
            {
                return Err(rule(at, format!("leads to `{next}`, which is no node")));
            }
            for condition in &reply.when {
                self.check_condition(&at, condition, ctx)?;
            }
            for effect in &reply.effects {
                self.check_effect(&at, effect, ctx)?;
            }
        }
        Ok(())
    }

    fn check_quest(&self, quest: &QuestDef, ctx: &QuestContext<'_>) -> Result<(), QuestError> {
        let id = &quest.id;
        check_words(id, &quest.title)?;
        if quest.stages.is_empty() {
            return Err(rule(id, "has no stages"));
        }
        unique(id, "stage", quest.stages.iter().map(|stage| &stage.id))?;
        unique(id, "crew", quest.crews.iter().map(|crew| &crew.id))?;
        unique(id, "ending", quest.endings.iter().map(|ending| &ending.id))?;
        for ending in &quest.endings {
            check_words(format!("{id} ending {}", ending.id), &ending.journal)?;
        }
        for crew in &quest.crews {
            if crew.roster.is_empty() || crew.roster.iter().any(|person| person.count == 0) {
                return Err(rule(id, format!("crew `{}` has nobody in it", crew.id)));
            }
            let led = self
                .people
                .iter()
                .any(|person| person.id.to_string() == format!("veyra:person.{}", crew.id));
            let bodies: u32 = crew
                .roster
                .iter()
                .map(|person| u32::from(person.count))
                .sum();
            match &crew.hand {
                None if bodies > u32::from(led) => {
                    return Err(rule(
                        id,
                        format!(
                            "crew `{}` has nameless bodies and no `hand` for them",
                            crew.id
                        ),
                    ));
                }
                Some(hand) => check_words(format!("{id} crew {}", crew.id), &hand.name)?,
                None => {}
            }
        }
        for stage in &quest.stages {
            let at = format!("{id} stage {}", stage.id);
            check_words(&at, &stage.journal)?;
            if let Some(next) = &stage.on_done.next
                && quest.stage(next).is_none()
            {
                return Err(rule(at, format!("goes on to `{next}`, which is no stage")));
            }
            for effect in &stage.on_done.effects {
                self.check_effect(&at, effect, ctx)?;
            }
            for objective in &stage.objectives {
                self.check_objective(&at, quest, objective, ctx)?;
            }
        }
        Ok(())
    }

    fn check_objective(
        &self,
        at: &str,
        quest: &QuestDef,
        objective: &Objective,
        ctx: &QuestContext<'_>,
    ) -> Result<(), QuestError> {
        let item = |item: &Namespaced| {
            ctx.items
                .get(item)
                .map(|_| ())
                .ok_or_else(|| rule(at, format!("`{item}` is no item")))
        };
        let person = |person: &Namespaced| {
            self.person(person)
                .map(|_| ())
                .ok_or_else(|| rule(at, format!("`{person}` is no person")))
        };
        let member = |name: &String| {
            if ctx.members.is_empty() || ctx.members.contains(name) {
                Ok(())
            } else {
                Err(rule(at, format!("`{name}` is nobody on the bench")))
            }
        };
        match objective {
            Objective::Talk(who) => person(who),
            Objective::Reach(_, radius) if *radius == 0 => {
                Err(rule(at, "a Reach radius is over nought"))
            }
            Objective::Defeat(Beat::Crew(crew)) if quest.crew(crew).is_none() => {
                Err(rule(at, format!("`{crew}` is no crew of the quest")))
            }
            Objective::Sample { class, units } => {
                if *units == 0 {
                    return Err(rule(at, "a Sample objective needs units"));
                }
                if ctx.resources.classes().iter().any(|each| each.id == *class) {
                    Ok(())
                } else {
                    Err(rule(at, format!("`{class}` is no resource class")))
                }
            }
            Objective::Craft(it) | Objective::Sell(it) | Objective::Buy(it) => item(it),
            Objective::Recruit(name) | Objective::Switch(name) => member(name),
            Objective::Deliver(it, who) => item(it).and_then(|()| person(who)),
            Objective::Reach(..)
            | Objective::Defeat(Beat::Opponent(_) | Beat::Crew(_))
            | Objective::Learn
            | Objective::Ping
            | Objective::Stable => Ok(()),
        }
    }

    fn check_condition(
        &self,
        at: impl std::fmt::Display,
        condition: &Condition,
        ctx: &QuestContext<'_>,
    ) -> Result<(), QuestError> {
        match condition {
            Condition::QuestAt(quest, stage) => match self.quest(quest) {
                None => Err(rule(at, format!("`{quest}` is no quest"))),
                Some(def) if def.stage(stage).is_none() => {
                    Err(rule(at, format!("`{quest}` has no stage `{stage}`")))
                }
                Some(_) => Ok(()),
            },
            Condition::QuestDone(quest) if self.quest(quest).is_none() => {
                Err(rule(at, format!("`{quest}` is no quest")))
            }
            Condition::Holds(item, count)
            | Condition::Room(item, count)
            | Condition::CarriesMade(item, count) => {
                if *count == 0 || ctx.items.get(item).is_none() {
                    Err(rule(
                        at,
                        format!("`{item}` is no item or the count is nought"),
                    ))
                } else {
                    Ok(())
                }
            }
            Condition::Learned(id) if ctx.graph.find(id).is_none() => {
                Err(rule(at, format!("`{id}` is no box")))
            }
            Condition::Flag(name) | Condition::NotFlag(name) if name.is_empty() => {
                Err(rule(at, "a flag has a name"))
            }
            _ => Ok(()),
        }
    }

    fn check_effect(
        &self,
        at: &str,
        effect: &Effect,
        ctx: &QuestContext<'_>,
    ) -> Result<(), QuestError> {
        match effect {
            Effect::StartQuest(quest) if self.quest(quest).is_none() => {
                Err(rule(at, format!("starts `{quest}`, which is no quest")))
            }
            Effect::SetStage(quest, stage) => match self.quest(quest) {
                None => Err(rule(at, format!("`{quest}` is no quest"))),
                Some(def) if def.stage(stage).is_none() => {
                    Err(rule(at, format!("`{quest}` has no stage `{stage}`")))
                }
                Some(_) => Ok(()),
            },
            Effect::Give(item, count)
            | Effect::Take(item, count)
            | Effect::TakeMade(item, count) => {
                if *count == 0 || ctx.items.get(item).is_none() {
                    Err(rule(
                        at,
                        format!("`{item}` is no item or the count is nought"),
                    ))
                } else {
                    Ok(())
                }
            }
            Effect::GiveGraded(item, count, lines) => {
                let Some(def) = ctx.items.get(item).filter(|_| *count > 0) else {
                    return Err(rule(
                        at,
                        format!("`{item}` is no item or the count is nought"),
                    ));
                };
                let theirs: BTreeSet<&str> =
                    def.lines.iter().map(|line| line.id.as_str()).collect();
                let given: BTreeSet<&str> = lines.keys().map(String::as_str).collect();
                if theirs != given || lines.values().any(|value| *value > QUALITY_MAX) {
                    return Err(rule(
                        at,
                        format!(
                            "gives `{item}` with lines other than its own {theirs:?}, or past {QUALITY_MAX}"
                        ),
                    ));
                }
                Ok(())
            }
            Effect::Xp(_, 0) => Err(rule(at, "XP is over nought")),
            Effect::Pay(credits) | Effect::Earn(credits) if *credits == 0 => {
                Err(rule(at, "credits are over nought"))
            }
            Effect::Deed(_, _, 0) => Err(rule(at, "a deed has units")),
            Effect::Hostile(target) => {
                let crew = self.quests.iter().any(|quest| quest.crew(target).is_some());
                let person = self
                    .people
                    .iter()
                    .any(|person| person.id.to_string() == *target);
                if crew || person {
                    Ok(())
                } else {
                    Err(rule(at, format!("`{target}` is no crew or person to turn")))
                }
            }
            Effect::Ending(name) => {
                if self.quests.iter().any(|quest| quest.ending(name).is_some()) {
                    Ok(())
                } else {
                    Err(rule(at, format!("`{name}` is no ending of any quest")))
                }
            }
            Effect::If(when, then) => {
                for condition in when {
                    self.check_condition(at, condition, ctx)?;
                }
                for effect in then {
                    self.check_effect(at, effect, ctx)?;
                }
                Ok(())
            }
            _ => Ok(()),
        }
    }

    /// The endings a stage's own effects make: a sale at the board is how
    /// one ending is made, and no reply is said at a board.
    fn stage_endings<'a>(&'a self, endings: &mut BTreeSet<&'a str>) {
        for quest in &self.quests {
            for stage in &quest.stages {
                effects_of(&stage.on_done.effects, &mut |effect| {
                    if let Effect::Ending(name) = effect {
                        endings.insert(name);
                    }
                });
            }
        }
    }

    /// Every node is reachable from a person's conversation; every stage
    /// from the first by a `next` or a `SetStage` of a reachable reply;
    /// every ending from a reachable reply or a stage's own effects.
    fn check_reach(
        &self,
        node_path: &BTreeMap<String, String>,
        quest_path: &BTreeMap<String, String>,
    ) -> Result<(), (String, QuestError)> {
        let mut seen: BTreeSet<&str> = BTreeSet::new();
        let mut stack: Vec<&str> = self
            .people
            .iter()
            .flat_map(|person| person.conversations.iter().map(|o| o.node.as_str()))
            .collect();
        while let Some(id) = stack.pop() {
            if !seen.insert(id) {
                continue;
            }
            if let Some(node) = self.nodes.get(id) {
                for reply in &node.replies {
                    if let Next::Node(next) = &reply.next {
                        stack.push(next);
                    }
                }
            }
        }
        for id in self.nodes.keys() {
            if !seen.contains(id.as_str()) {
                let path = node_path.get(id).cloned().unwrap_or_default();
                return Err((path, rule(id, "no conversation reaches this node")));
            }
        }
        let mut stages: BTreeSet<(&str, &str)> = BTreeSet::new();
        let mut endings: BTreeSet<&str> = BTreeSet::new();
        for id in &seen {
            let Some(node) = self.nodes.get(*id) else {
                continue;
            };
            for reply in &node.replies {
                effects_of(&reply.effects, &mut |effect| match effect {
                    Effect::SetStage(quest, stage) => {
                        stages.insert((quest, stage));
                    }
                    Effect::Ending(name) => {
                        endings.insert(name);
                    }
                    Effect::StartQuest(quest) => {
                        if let Some(first) = self.quest(quest).and_then(|q| q.stages.first()) {
                            stages.insert((quest, &first.id));
                        }
                    }
                    _ => {}
                });
            }
        }
        self.stage_endings(&mut endings);
        for quest in &self.quests {
            let path = quest_path.get(&quest.id).cloned().unwrap_or_default();
            let mut reached: BTreeSet<&str> = BTreeSet::new();
            let mut work: Vec<&str> = quest
                .stages
                .first()
                .map(|stage| stage.id.as_str())
                .into_iter()
                .chain(
                    stages
                        .iter()
                        .filter(|(q, _)| *q == quest.id)
                        .map(|(_, stage)| *stage),
                )
                .collect();
            while let Some(stage) = work.pop() {
                if !reached.insert(stage) {
                    continue;
                }
                if let Some(next) = quest.stage(stage).and_then(|s| s.on_done.next.as_deref()) {
                    work.push(next);
                }
            }
            for stage in &quest.stages {
                if !reached.contains(stage.id.as_str()) {
                    return Err((
                        path,
                        rule(
                            format!("{} stage {}", quest.id, stage.id),
                            "is never reached",
                        ),
                    ));
                }
            }
            for ending in &quest.endings {
                if !endings.contains(ending.id.as_str()) {
                    return Err((
                        path,
                        rule(
                            format!("{} ending {}", quest.id, ending.id),
                            "no conversation can end the quest this way",
                        ),
                    ));
                }
            }
        }
        Ok(())
    }

    /// A flag read and never set is a typo.
    fn check_flags(
        &self,
        node_path: &BTreeMap<String, String>,
    ) -> Result<(), (String, QuestError)> {
        let mut set: BTreeSet<String> = BTreeSet::new();
        let mut read: Vec<(String, String)> = Vec::new();
        let mut note = |from: &str, effects: &[Effect], conditions: &[Condition]| {
            effects_of(effects, &mut |effect| match effect {
                Effect::SetFlag(name) => {
                    set.insert(name.clone());
                }
                Effect::Ending(name) => {
                    set.insert(format!("ending_{name}"));
                }
                _ => {}
            });
            for condition in conditions {
                if let Condition::Flag(name) | Condition::NotFlag(name) = condition {
                    read.push((from.to_owned(), name.clone()));
                }
            }
        };
        for node in self.nodes.values() {
            for reply in &node.replies {
                note(&node.id, &reply.effects, &reply.when);
                effects_if_reads(&reply.effects, &mut |when| note(&node.id, &[], when));
            }
        }
        for person in &self.people {
            for opening in &person.conversations {
                note(person.id.as_ref(), &[], &opening.when);
            }
        }
        for quest in &self.quests {
            for stage in &quest.stages {
                note(&quest.id, &stage.on_done.effects, &[]);
                effects_if_reads(&stage.on_done.effects, &mut |when| {
                    note(&quest.id, &[], when);
                });
            }
        }
        for (from, name) in read {
            if !set.contains(&name) {
                let path = node_path.get(&from).cloned().unwrap_or_default();
                return Err((
                    path,
                    rule(from, format!("reads flag `{name}`, which nothing sets")),
                ));
            }
        }
        Ok(())
    }
}

fn at<K: Ord>(paths: &BTreeMap<K, String>, key: &K) -> String {
    paths.get(key).cloned().unwrap_or_default()
}

/// Every effect of a list, the ones inside an `If` too.
pub fn effects_of<'a>(effects: &'a [Effect], each: &mut impl FnMut(&'a Effect)) {
    for effect in effects {
        each(effect);
        if let Effect::If(_, then) = effect {
            effects_of(then, each);
        }
    }
}

/// The conditions of every `If` in a list.
fn effects_if_reads<'a>(effects: &'a [Effect], each: &mut impl FnMut(&'a [Condition])) {
    for effect in effects {
        if let Effect::If(when, then) = effect {
            each(when);
            effects_if_reads(then, each);
        }
    }
}

fn version(path: &str, found: u32) -> Result<(), (String, QuestError)> {
    if found == QUEST_VERSION {
        Ok(())
    } else {
        Err((
            path.to_owned(),
            QuestError::Version {
                found,
                expected: QUEST_VERSION,
            },
        ))
    }
}

fn unique<'a>(
    quest: &str,
    what: &str,
    ids: impl Iterator<Item = &'a String>,
) -> Result<(), QuestError> {
    let mut seen = BTreeSet::new();
    for id in ids {
        if !seen.insert(id) {
            return Err(rule(quest, format!("{what} `{id}` is defined twice")));
        }
    }
    Ok(())
}

/// The words between braces in `text`, in order.
#[must_use]
pub fn words(text: &str) -> Vec<&str> {
    let mut found = Vec::new();
    let mut rest = text;
    while let Some(open) = rest.find('{') {
        let after = &rest[open + 1..];
        let Some(close) = after.find('}') else {
            found.push(after);
            break;
        };
        found.push(&after[..close]);
        rest = &after[close + 1..];
    }
    found
}

fn check_words(at: impl std::fmt::Display, text: &str) -> Result<(), QuestError> {
    for word in words(text) {
        if !PLACEHOLDERS.contains(&word) {
            return Err(rule(
                at,
                format!("`{{{word}}}` is no placeholder (use {PLACEHOLDERS:?})"),
            ));
        }
    }
    if text.trim().is_empty() {
        return Err(rule(at, "has no words"));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    #![allow(
        clippy::unwrap_used,
        reason = "a test that unwraps a None fails, which is the point"
    )]

    use super::*;
    use crate::load::load_from_dir;

    /// A small fixture: two people, one quest of two stages with a crew and
    /// two endings, every kind of condition, effect and objective the rules
    /// check.
    pub(crate) const PERSON_A: &str = r#"(
        version: 1, id: "veyra:person.a", name: "Ana", title: "warden for {assembly}",
        role: Assembly, tint: (200, 100, 50), stand: Quay, facing: 0.0,
        conversations: [
            (when: [QuestDone("q")], node: "a.after"),
            (when: [QuestAt("q", "one")], node: "a.hub"),
            (node: "a.hello"),
        ],
    )"#;
    pub(crate) const PERSON_B: &str = r#"(
        version: 1, id: "veyra:person.b", name: "Bo", title: "of {holder}",
        role: Holder, tint: (10, 10, 10), stand: Board, facing: 1.0,
        conversations: [(node: "b.hub")],
    )"#;
    pub(crate) const DIALOGUE: &str = r#"(
        version: 1,
        nodes: [
            (id: "a.hello", speaker: "veyra:person.a", line: "Hello from {assembly}.", replies: [
                (text: "Begin.", effects: [StartQuest("q")], next: Node("a.hub")),
                (text: "Skip.", effects: [StartQuest("q"), Ending("skip")], next: End),
            ]),
            (id: "a.hub", speaker: "veyra:person.a", line: "Go to Bo.", replies: [
                (text: "Ask.", effects: [SetFlag("asked")], next: End),
                (text: "Pay.", when: [PurseAtLeast(10)], effects: [Pay(10)], next: End),
            ]),
            (id: "a.after", speaker: "veyra:person.a", line: "Done.", replies: []),
            (id: "b.hub", speaker: "veyra:person.b", line: "{holder} listens.", replies: [
                (text: "Choose.", when: [Flag("asked")], effects: [
                    If([Flag("asked")], [Deed(Holder, JobDone, 1)]),
                    SetStage("q", "two"),
                    Earn(5),
                    Ending("good"),
                ], next: End),
            ]),
        ],
    )"#;
    pub(crate) const QUEST: &str = r#"(
        version: 1, id: "q", title: "Quest of {keepers}",
        crews: [(id: "crew", at: Station, roster: [(kind: Sentry, count: 2)],
                 hand: Some((name: "Hand", tint: (9, 9, 9))))],
        stages: [
            (id: "one", journal: "Do one.", objectives: [Talk("veyra:person.b"), Reach(Station, 30),
                Defeat(Crew("crew")), Defeat(Opponent(Sentry)), Learn, Ping, Stable],
                on_done: (next: Some("two"))),
            (id: "two", journal: "Do two.", objectives: [], on_done: ()),
        ],
        endings: [(id: "good", journal: "Good."), (id: "skip", journal: "Skipped.")],
    )"#;

    fn real() -> crate::GameRegistries {
        load_from_dir(concat!(env!("CARGO_MANIFEST_DIR"), "/../../assets")).unwrap()
    }

    fn files(a: &str, b: &str, d: &str, q: &str) -> QuestFiles {
        QuestFiles {
            quests: vec![("quests/q.quest.ron".to_owned(), ron::from_str(q).unwrap())],
            dialogues: vec![(
                "dialogue/d.dialogue.ron".to_owned(),
                ron::from_str(d).unwrap(),
            )],
            people: vec![
                ("people/a.person.ron".to_owned(), ron::from_str(a).unwrap()),
                ("people/b.person.ron".to_owned(), ron::from_str(b).unwrap()),
            ],
        }
    }

    fn build(files: QuestFiles) -> Result<QuestSet, (String, QuestError)> {
        let registries = real();
        QuestSet::build(
            files,
            &QuestContext {
                items: &registries.items,
                graph: &registries.progression,
                resources: &registries.resources,
                members: &[],
            },
        )
    }

    fn refused(a: &str, b: &str, d: &str, q: &str, reason: &str) {
        let error = build(files(a, b, d, q)).expect_err("the fixture is refused");
        let said = format!("{} {}", error.0, error.1);
        assert!(said.contains(reason), "`{said}` should say `{reason}`");
    }

    fn with(text: &str, from: &str, to: &str) -> String {
        assert!(text.contains(from), "`{from}` is in the fixture");
        text.replace(from, to)
    }

    /// The fixture loads whole.
    #[test]
    fn the_fixture_loads() {
        let set = build(files(PERSON_A, PERSON_B, DIALOGUE, QUEST)).unwrap();
        assert_eq!(set.people().len(), 2);
        assert_eq!(set.nodes().len(), 4);
        assert_eq!(set.quest("q").unwrap().crew_size("crew"), 2);
    }

    /// Every refusal, once, naming the file and the id.
    #[test]
    #[allow(clippy::too_many_lines, reason = "one case a rule, in one place")]
    fn each_rule_refuses_once() {
        // A reply to a node that is not there.
        refused(
            PERSON_A,
            PERSON_B,
            &with(
                DIALOGUE,
                "next: Node(\"a.hub\")",
                "next: Node(\"a.nowhere\")",
            ),
            QUEST,
            "a.nowhere",
        );
        // A node nobody can reach.
        let orphan = with(
            DIALOGUE,
            "(id: \"a.after\"",
            "(id: \"a.orphan\", speaker: \"veyra:person.a\", line: \"x\", replies: []),\n(id: \"a.after\"",
        );
        refused(
            PERSON_A,
            PERSON_B,
            &orphan,
            QUEST,
            "no conversation reaches",
        );
        // An unknown placeholder.
        refused(
            PERSON_A,
            PERSON_B,
            &with(DIALOGUE, "{holder} listens", "{nobody} listens"),
            QUEST,
            "no placeholder",
        );
        // A speaker who is no person.
        refused(
            PERSON_A,
            PERSON_B,
            &with(DIALOGUE, "veyra:person.b\", line", "veyra:person.z\", line"),
            QUEST,
            "is no person",
        );
        // A stage the quest does not have.
        refused(
            PERSON_A,
            PERSON_B,
            &with(
                DIALOGUE,
                "SetStage(\"q\", \"two\")",
                "SetStage(\"q\", \"three\")",
            ),
            QUEST,
            "no stage `three`",
        );
        // An ending nobody can reach.
        refused(
            PERSON_A,
            PERSON_B,
            &with(DIALOGUE, "Ending(\"good\")", "SetFlag(\"asked\")"),
            QUEST,
            "no conversation can end the quest this way",
        );
        // An ending no quest names.
        refused(
            PERSON_A,
            PERSON_B,
            &with(DIALOGUE, "Ending(\"good\")", "Ending(\"bad\")"),
            QUEST,
            "is no ending",
        );
        // A stage nothing reaches.
        refused(
            PERSON_A,
            PERSON_B,
            &with(DIALOGUE, "SetStage(\"q\", \"two\"),", ""),
            &with(QUEST, "on_done: (next: Some(\"two\"))", "on_done: ()"),
            "is never reached",
        );
        // A stage's next that is no stage.
        refused(
            PERSON_A,
            PERSON_B,
            DIALOGUE,
            &with(QUEST, "Some(\"two\")", "Some(\"nine\")"),
            "no stage",
        );
        // A flag read and never set.
        refused(
            PERSON_A,
            PERSON_B,
            &with(DIALOGUE, "SetFlag(\"asked\")", "SetFlag(\"other\")"),
            QUEST,
            "nothing sets",
        );
        // An item that does not exist, in an effect.
        refused(
            PERSON_A,
            PERSON_B,
            &with(DIALOGUE, "Earn(5)", "Give(\"veyra:none\", 1)"),
            QUEST,
            "is no item",
        );
        // A person who opens a node that is not there.
        refused(
            PERSON_A,
            &with(PERSON_B, "\"b.hub\"", "\"b.gone\""),
            DIALOGUE,
            QUEST,
            "b.gone",
        );
        // An objective naming a person and a crew that do not exist.
        refused(
            PERSON_A,
            PERSON_B,
            DIALOGUE,
            &with(
                QUEST,
                "Talk(\"veyra:person.b\")",
                "Talk(\"veyra:person.q\")",
            ),
            "is no person",
        );
        refused(
            PERSON_A,
            PERSON_B,
            DIALOGUE,
            &with(QUEST, "Crew(\"crew\")", "Crew(\"mob\")"),
            "is no crew",
        );
        // A hostile target that is neither.
        refused(
            PERSON_A,
            PERSON_B,
            &with(DIALOGUE, "Earn(5)", "Hostile(\"mob\")"),
            QUEST,
            "to turn",
        );
        // A crew with nameless bodies and no plate for them.
        refused(
            PERSON_A,
            PERSON_B,
            DIALOGUE,
            &with(
                QUEST,
                ",\n                 hand: Some((name: \"Hand\", tint: (9, 9, 9))))]",
                ")]",
            ),
            "no `hand`",
        );
        // A gift with lines its item lacks.
        refused(
            PERSON_A,
            PERSON_B,
            &with(
                DIALOGUE,
                "Earn(5)",
                "GiveGraded(\"veyra:tool.survey\", 1, {\"reach\": 700})",
            ),
            QUEST,
            "lines other than its own",
        );
        // A reason with nothing to grey the reply.
        refused(
            PERSON_A,
            PERSON_B,
            &with(
                DIALOGUE,
                "(text: \"Ask.\",",
                "(text: \"Ask.\", why: Some(\"No.\"),",
            ),
            QUEST,
            "no `when`",
        );
        // Two files for one person, and another version.
        let mut two = files(PERSON_A, PERSON_B, DIALOGUE, QUEST);
        two.people.push(two.people[0].clone());
        assert!(build(two).unwrap_err().1.to_string().contains("twice"));
        refused(
            PERSON_A,
            &with(PERSON_B, "version: 1", "version: 9"),
            DIALOGUE,
            QUEST,
            "version 9",
        );
    }

    /// The place dials parse and a bad one is refused.
    #[test]
    fn the_place_dials_are_held_to_their_rules() {
        let shipped = include_str!("../../../assets/quests/places.ron");
        let rules = PlaceRules::parse(shipped).unwrap();
        assert!((rules.station_m - 900.0).abs() < f64::EPSILON);
        assert!(real().quests.rules().is_some());
        for (from, to) in [
            ("station_m: 900.0", "station_m: 0.0"),
            ("bearings: 72", "bearings: 3"),
            ("channel_far_m: 2000.0", "channel_far_m: 300.0"),
            ("camp_back_m: 16.0", "camp_back_m: 0.0"),
            ("version: 1", "version: 2"),
        ] {
            let bad = shipped.replace(from, to);
            assert_ne!(bad, shipped, "{from}");
            assert!(PlaceRules::parse(&bad).is_err(), "{to}");
        }
    }

    /// The shipped files load: five people, the questline, every ending.
    #[test]
    fn the_shipped_files_load() {
        let registries = real();
        let set = &registries.quests;
        assert!(set.people().len() >= 5, "the cast");
        let quest = set.quest("arrival").expect("the arrival quest");
        for ending in ["keepers", "assembly", "holder", "skip"] {
            assert!(quest.ending(ending).is_some(), "{ending}");
        }
        assert!(quest.crew("pell").is_some());
    }
}
