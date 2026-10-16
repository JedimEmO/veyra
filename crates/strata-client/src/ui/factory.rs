//! The factory: a fabricator's screen, where a production blueprint is fed
//! from the stock and run as a standing job (ticket 0007, step 5;
//! `docs/design/crafting.md`, 5.4; the progression design's "Run the
//! workshop").
//!
//! One screen in two states. With the fabricator free it is the blueprints
//! the stock holds at the left, each with its lead line and the runs it has
//! left, and at the right the one picked: its batch size, which cannot pass
//! the runs left or what the stock feeds and says which holds it; its
//! slots, each saying what it needs and listing what the stock has for it,
//! the kinds that meet it first and marked, every kind that does not
//! dimmed in place with the figure it fails on and what is asked; one press
//! that fills every slot with the cheapest stock that meets the blueprint;
//! what the run would make and when it would be ready; who works it; and
//! Run. With a run on, the same screen shows it: how many of the batch are
//! made, the time left, the worker, whether it stands and why, what is
//! still in the hopper, and Stop.
//!
//! No rule lives here and nothing is worked out. Every figure, every order
//! and every reason is the sim's (`strata_sim::factory`): `feeds` for a
//! slot's stock with each verdict, `cheapest` for the fill, `limit` for the
//! largest batch and what holds it, `check_start` for why Run is grey,
//! `seconds` and `left` for the time, `can_stop` for Stop, and
//! `strata_sim::blueprint::held` for the blueprints in reach.
//! [`FactoryView::of`] gathers their answers into display words and
//! [`screen_def`] lays those out, so what the screen says can be checked
//! without one. Run, a change of worker and Stop put a `FactoryRequest` in
//! the sim's inbox for the tick to judge, and the sim's `FactoryAnswer` is
//! worded at the button and in the footer.
//!
//! The run is the sim's and the save's, so it goes on with the screen shut
//! and a screen opened later shows it. What a finished or a standing run
//! says of itself is a toast wherever the player is ([`toast_notices`]).
//! The definition is a function of the view and is swapped whole when the
//! view changes ([`rebuild`]); the one thing written in place is the time
//! left, which moves every second. Local-only: nothing here is replicated.

use std::sync::Arc;

use bevy::ecs::prelude::*;
use bevy::ui::widget::Text;
use slotted::menu::MenuChoice;
use slotted::registry::Value;
use slotted::theme::Role;
use slotted::ui::def::{
    Align, ButtonOpts, Justify, Layout, LayoutDirection, Length, LocKey, Padding, Presentation,
    PresentationMode, ScreenDef, ScreenKind, Tags, TextOpts, TextRole, Transition, UiNodeDef,
    WidgetKind,
};
use slotted::ui::{LocText, ScreenStack, TestId, push_screen, replace_screen};
use strata_registry::item::{QUALITY_MAX, Record};
use strata_registry::schematic::{FactoryRules, SlotDef};
use strata_registry::{GameRegistries, Namespaced};
use strata_sim::blueprint::{self, Blueprint, Blueprints, SlotNeed};
use strata_sim::deposits::Purse;
use strata_sim::factory::{
    self, Benched, FactoryAnswer, FactoryClock, FactoryDone, FactoryInbox, FactoryNotice,
    FactoryRefusal, FactoryRequest, FactoryState, Feed, FeedRefusal, Feeds, Held, Job, Pause,
};
use strata_sim::fitting::WorkbenchAccess;
use strata_sim::items::{HouseholdStore, Pack};
use strata_sim::progression::{CharacterSheet, Reach};
use strata_sim::{LocalPlayer, Registries};

use super::card;
use super::crafting::{
    ACT_ROLE, BOX_ROLE, FIGURE_ROLE, HEAD_ROLE, MIN_HEIGHT_VH, NAME_ROLE, NOTE_ROLE, PICK_ON_ROLE,
    PICK_ROLE, REFUSED_ROLE, SCHEM_NAME_CANT_ROLE, SCHEM_NAME_ROLE, SCHEM_ON_ROLE, SCHEM_ROLE,
    SCHEMATICS_WIDTH, SLOT_ROLE, STAT_ROLE, STAT_VALUE_ROLE, STATUS_ROLE, SUB_ROLE, Swatch,
    TAG_ROLE, WHY_HINT_ROLE, WHY_ROLE, WIDTH, act, column, fill, grows, head, key, line_in, panel,
    row, spacer, stock_name, swatch_of, text_in, well, with_id, words,
};
use super::strings::counted;
use super::weapon_diagram::FOCUS_MARK;

/// The screen kind.
pub const KIND: &str = "strata:factory";

/// The `test_id` of the screen's root.
pub const ROOT_ID: &str = "factory.root";
/// The `test_id` of the title.
pub const TITLE_ID: &str = "factory.title";
/// The `test_id` of who works the run, in the head.
pub const WORKER_ID: &str = "factory.worker";
/// The `test_id` of the household's credits, in the head.
pub const PURSE_ID: &str = "factory.purse";
/// The `test_id` of the footer's Close.
pub const CLOSE_ID: &str = "factory.close";
/// The `test_id` of the footer's line.
pub const STATUS_ID: &str = "factory.status";
/// The `test_id` of what the screen is for, when the footer's line is the
/// sim's last word.
pub const KEYS_ID: &str = "factory.keys";
/// Prefix of a blueprint's row; its number follows.
pub const PRINT_TAG: &str = "factory.bp:";
/// The `test_id` of the sentence that says there is no blueprint.
pub const NONE_ID: &str = "factory.none";
/// The `test_id` of the right column's heading.
pub const HEAD_ID: &str = "factory.head";
/// The `test_id` of the batch's size.
pub const BATCH_ID: &str = "factory.batch";
/// Prefix of a button that changes the batch; `less10`, `less`, `more`,
/// `more10` or `most` follows.
pub const BATCH_TAG: &str = "factory.batch.";
/// The `test_id` of what holds the batch where it is.
pub const BATCH_WHY_ID: &str = "factory.batch.why";
/// Prefix of a slot's box; its key follows.
pub const SLOT_TAG: &str = "factory.slot:";
/// Prefix of what a slot needs; its key follows.
pub const NEED_TAG: &str = "factory.need:";
/// Prefix of a kind of stack the stock has for a slot; the slot's key, a
/// colon and the row follow.
pub const FEED_TAG: &str = "factory.feed:";
/// Prefix of the mark on a kind that meets its slot; the slot's key, a
/// colon and the row follow.
pub const MARK_TAG: &str = "factory.mark:";
/// Prefix of the sentence a slot shows when nothing feeds it; its key
/// follows.
pub const NOTHING_TAG: &str = "factory.nothing:";
/// The `test_id` and `menu` tag of Cheapest stock.
pub const CHEAPEST: &str = "factory.cheapest";
/// The `test_id` of what the run would make and when.
pub const MAKES_ID: &str = "factory.makes";
/// Prefix of a worker's chip; nought is nobody, then the bench in order.
pub const WORKER_TAG: &str = "factory.worker:";
/// The `test_id` of why Run is grey.
pub const WHY_ID: &str = "factory.why";
/// The `test_id` and `menu` tag of Run.
pub const RUN: &str = "factory.run";
/// The `test_id` of how many of the batch are made.
pub const MADE_ID: &str = "factory.made";
/// The `test_id` of the bar of how many are made.
pub const BAR_ID: &str = "factory.bar";
/// The `test_id` of the time the run has left.
pub const LEFT_ID: &str = "factory.left";
/// The `test_id` of the line that says the run stands and why.
pub const PAUSED_ID: &str = "factory.paused";
/// The `test_id` of what is still in the hopper.
pub const HOPPER_ID: &str = "factory.hopper";
/// The `test_id` of why Stop is grey.
pub const STOP_WHY_ID: &str = "factory.stop.why";
/// The `test_id` and `menu` tag of Stop.
pub const STOP: &str = "factory.stop";

/// The batch a blueprint opens at, when the runs and the stock allow it:
/// what the mockup's worked example runs. A screen's first offer and no
/// rule, so it is the client's.
const OPENING_BATCH: u32 = 20;
/// The batch's larger step. A control's reach and no rule.
const BATCH_STEP: u32 = 10;

const NEED_ROLE: &str = "strata.factory.need";
const FAIL_ROLE: &str = "strata.factory.feed.fail";
const FEEDS_ROLE: &str = "strata.factory.feeds";
pub(super) const PROGRESS_ROLE: &str = "strata.factory.progress";
pub(super) const PROGRESS_FILL_ROLE: &str = "strata.factory.progress.fill";
pub(super) const PAUSED_ROLE: &str = "strata.factory.paused";
const BATCH_ROLE: &str = "strata.factory.batch";

/// Every role of the screen's own, for the test that holds them to the
/// theme; the rest are the fitting's and the crafting's, which theirs do.
pub const ROLES: [&str; 7] = [
    NEED_ROLE,
    FAIL_ROLE,
    FEEDS_ROLE,
    PROGRESS_ROLE,
    PROGRESS_FILL_ROLE,
    PAUSED_ROLE,
    BATCH_ROLE,
];

/// The screen kind.
#[must_use]
pub fn kind() -> ScreenKind {
    ScreenKind::new(KIND)
}

/// The `test_id` of the row of blueprint `id`.
#[must_use]
pub fn print_id(id: u32) -> String {
    format!("{PRINT_TAG}{id}")
}

/// The `test_id` of the kind at `row` of the slot `slot`.
#[must_use]
pub fn feed_id(slot: &str, row: usize) -> String {
    format!("{FEED_TAG}{slot}:{row}")
}

/// The `test_id` of the mark on the kind at `row` of the slot `slot`.
#[must_use]
pub fn mark_id(slot: &str, row: usize) -> String {
    format!("{MARK_TAG}{slot}:{row}")
}

/// The `test_id` of the chip of the worker at `index`: nought is nobody.
#[must_use]
pub fn worker_id(index: usize) -> String {
    format!("{WORKER_TAG}{index}")
}

/// The `test_id` of the batch's button `step`.
#[must_use]
pub fn batch_id(step: &str) -> String {
    format!("{BATCH_TAG}{step}")
}

// ---- the state ----

/// The factory screen's state: the blueprint picked, the batch asked for,
/// what feeds each slot and who is to work it, and what the sim last
/// answered.
#[derive(Resource, Debug, Default)]
pub struct FactoryUi {
    /// The blueprint picked, by its number; none until the screen has
    /// resolved one.
    pub blueprint: Option<u32>,
    /// The batch asked for.
    pub batch: u32,
    /// The kinds picked to feed each slot.
    pub feeds: Feeds,
    /// Who is to work the run, by name; nobody, and it runs unattended.
    pub worker: Option<String>,
    /// The batch is to be set to its opening size at the next build.
    reopen: bool,
    /// The slots hold the sim's own choice, the cheapest stock that meets
    /// the blueprint, and follow the batch as it changes. A row toggled by
    /// hand ends that until Cheapest stock is pressed again.
    auto: bool,
    /// The `test_id` of the control the keyboard's focus goes to at the
    /// next build.
    last: Option<String>,
    /// A rebuild is due whatever the view says.
    dirty: bool,
    /// The sim's last answer in words, and whether it was a refusal.
    notice: Option<(String, bool)>,
    /// What the open screen was built from.
    view: Option<FactoryView>,
    /// The requests sent that the sim has not answered yet.
    asked: usize,
}

impl FactoryUi {
    /// What the open screen was last built from.
    #[must_use]
    pub const fn view(&self) -> Option<&FactoryView> {
        self.view.as_ref()
    }
}

// ---- the view ----

/// One blueprint's row.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PrintRow {
    /// The blueprint's number.
    pub id: u32,
    /// What it makes: `Cell core`.
    pub name: String,
    /// Its lead line's figure: `436`.
    pub lead: String,
    /// `97 runs left`.
    pub runs: String,
    /// Whether it is the one shown.
    pub on: bool,
    /// Why it cannot be picked: the fabricator is running another.
    pub busy: Option<String>,
}

/// One kind of stack the stock holds for a slot.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FeedRow {
    /// The kind, as a feed names it.
    pub kind: Record,
    /// `Tolley salt`, `Cell core`.
    pub name: String,
    /// `batch 1` for a made part; empty for anything else.
    pub batch: String,
    /// Its shape; none for a kind the catalogue does not know.
    pub swatch: Option<Swatch>,
    /// Each figure the slot asks of, by name, as the kind's card writes it.
    pub figures: Vec<(String, String)>,
    /// How much the stock holds: `40 in pack · 120 in store`.
    pub held: String,
    /// Why it will not feed the slot: `Potency 410, needs 537`. None for a
    /// kind that meets it.
    pub refused: Option<String>,
    /// Whether it is picked to feed the slot.
    pub on: bool,
}

/// One slot of the blueprint shown.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SlotView {
    /// The slot's key.
    pub id: String,
    /// `REACTIVE`.
    pub label: String,
    /// `20 units each`.
    pub each: String,
    /// `needs Potency 537 · Stability 191`.
    pub needs: String,
    /// What the stock has for it: the kinds that meet it first, the
    /// cheapest of them first, then every kind that does not.
    pub rows: Vec<FeedRow>,
    /// The sentence shown when nothing the stock has feeds it.
    pub none: Option<String>,
}

/// One button of the batch's row.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BatchStep {
    /// `less10`, `less`, `more`, `more10` or `most`.
    pub id: &'static str,
    /// What the button says.
    pub label: String,
    /// The batch a press gives; none when it would change nothing.
    pub to: Option<u32>,
}

/// The batch and what holds it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BatchView {
    /// The batch asked for.
    pub size: u32,
    /// The largest the runs and the stock allow.
    pub most: u32,
    /// The buttons, left to right.
    pub steps: Vec<BatchStep>,
    /// What holds it: `Up to 4 · the Brine in stock feeds 4`.
    pub why: String,
    /// Whether the batch is at that limit, where a larger one is refused.
    pub held: bool,
}

/// One choice of who works the run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkerChip {
    /// Who: nobody, or a benched member by name.
    pub name: Option<String>,
    /// `Nobody · half speed`, `Ilse Marrow · full speed`.
    pub label: String,
    /// Whether it is the one chosen.
    pub on: bool,
}

/// The screen with the fabricator free and a blueprint shown.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IdleStage {
    /// The blueprint, by its number.
    pub id: u32,
    /// `CELL CORE · BLUEPRINT 4`.
    pub head: String,
    /// The batch.
    pub batch: BatchView,
    /// The slots, in the blueprint's order.
    pub slots: Vec<SlotView>,
    /// `Makes 20 cell cores at 436 · ready in 8 min`.
    pub makes: String,
    /// Who can work it, nobody first.
    pub workers: Vec<WorkerChip>,
    /// What Run asks the sim for; none when it is grey.
    pub request: Option<FactoryRequest>,
    /// Why it is grey; empty when it is not.
    pub why: String,
}

/// The screen with a run on.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RunStage {
    /// `CELL CORE · BLUEPRINT 4 · BATCH 5`.
    pub head: String,
    /// `7 of 20`.
    pub made: String,
    /// How much of the batch is made, per-mille, for the bar.
    pub share: u16,
    /// `Ready in 5 min`; empty while it stands.
    pub left: String,
    /// `PAUSED · no room in the store`, while it stands.
    pub paused: Option<String>,
    /// Who can work it, nobody first.
    pub workers: Vec<WorkerChip>,
    /// What is committed and not yet spent: each kind and how much.
    pub hopper: Vec<(String, String)>,
    /// Whether Stop is live.
    pub can_stop: bool,
    /// Why it is grey; empty when it is not.
    pub stop_why: String,
}

/// Which of its two states the screen is in.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Stage {
    /// The fabricator is free.
    Idle(IdleStage),
    /// A run is on.
    Run(RunStage),
}

/// Everything the definition is made from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FactoryView {
    /// `WORKED BY NOBODY · HALF SPEED`.
    pub worker: String,
    /// The household's credits.
    pub purse: u32,
    /// The blueprints the stock holds, and the one the fabricator holds
    /// while it runs, first.
    pub prints: Vec<PrintRow>,
    /// The sentence shown with no blueprint at all.
    pub none: Option<String>,
    /// The right column; none with no blueprint.
    pub stage: Option<Stage>,
    /// Whether a fabricator is in reach.
    pub at_fabricator: bool,
    /// The footer's line and whether it is a refusal.
    pub status: (String, bool),
    /// What the screen is for, when the footer's line is the sim's last
    /// word and no longer says so.
    pub keys: Option<String>,
    /// The control the keyboard's focus starts on.
    pub focus: Option<String>,
}

/// What a view is read from: the local player's stock, the household's
/// store, the blueprints, the fabricator and what is in reach.
#[derive(Debug, Clone, Copy)]
pub struct Floor<'a> {
    /// The data set.
    pub registries: &'a GameRegistries,
    /// What the character carries.
    pub pack: &'a Pack,
    /// The household's store, which is to hand at a fabricator.
    pub store: &'a HouseholdStore,
    /// The household's credits.
    pub purse: Purse,
    /// Every blueprint the household has recorded.
    pub prints: &'a Blueprints,
    /// The fabricator's run, if any.
    pub state: &'a FactoryState,
    /// How far the run is toward its next unit.
    pub clock: &'a FactoryClock,
    /// Who is on the bench and not out.
    pub benched: &'a Benched,
    /// Whether a fabricator is in reach.
    pub at_fabricator: bool,
    /// The sheet of the member at the fabricator, whose boxes set the pace
    /// a run starts at.
    pub sheet: &'a CharacterSheet,
}

impl<'a> Floor<'a> {
    /// The store, where it can be reached: at a fabricator.
    fn reach(&self) -> Option<&'a HouseholdStore> {
        self.at_fabricator.then_some(self.store)
    }

    /// The blueprints in reach, each with its body.
    fn held(&self) -> Vec<(u32, &'a Blueprint)> {
        blueprint::held(self.registries, self.pack, self.reach(), self.prints)
            .into_iter()
            .filter_map(|(id, _)| Some((id, self.prints.0.get(&id)?)))
            .collect()
    }
}

// ---- wording the sim ----

/// `97 runs left`, `1 run left`.
fn runs_words(left: u32) -> String {
    if left == 1 {
        words("strata.factory.runs.one").to_owned()
    } else {
        fill(words("strata.factory.runs"), "left", &left.to_string())
    }
}

/// A run's time as a player reads it: the seconds under a minute, else the
/// minutes, a part of one counted whole.
#[must_use]
pub fn time_words(seconds: u32) -> String {
    if seconds < 60 {
        fill(
            words("strata.factory.time.seconds"),
            "n",
            &seconds.to_string(),
        )
    } else {
        fill(
            words("strata.factory.time.minutes"),
            "n",
            &seconds.div_ceil(60).to_string(),
        )
    }
}

/// The rate a run goes at, per-mille of the worked one, in words: `full
/// speed`, `half speed`, or the share where the dial gives another.
fn speed_words(rate_pm: u16) -> String {
    if rate_pm >= QUALITY_MAX {
        words("strata.factory.speed.full").to_owned()
    } else if rate_pm == QUALITY_MAX / 2 {
        words("strata.factory.speed.half").to_owned()
    } else {
        fill(
            words("strata.factory.speed.share"),
            "percent",
            &card::percent_of(rate_pm).to_string(),
        )
    }
}

/// Why a kind of stack, or what is picked for `slot`, will not feed it, as
/// a player reads it: the figure with its number and what is asked.
#[must_use]
pub fn feed_refusal_words(
    registries: &GameRegistries,
    slot: &SlotDef,
    refusal: &FeedRefusal,
) -> String {
    let named = |key: &'static str, stat: &str, has: u16, name: &str, asked: u16| {
        fill(
            &fill(
                &fill(words(key), "stat", &card::need_name(registries, slot, stat)),
                "has",
                &has.to_string(),
            ),
            name,
            &asked.to_string(),
        )
    };
    match refusal {
        FeedRefusal::Empty => words("strata.factory.refused.slot.empty").to_owned(),
        FeedRefusal::WrongKind => words("strata.factory.feed.wrong").to_owned(),
        FeedRefusal::Under { stat, has, needs } => {
            named("strata.factory.feed.under", stat, *has, "needs", *needs)
        }
        FeedRefusal::Over { stat, has, most } => {
            named("strata.factory.feed.over", stat, *has, "most", *most)
        }
        FeedRefusal::Short { have, needs } => fill(
            &fill(
                words("strata.factory.refused.slot.short"),
                "have",
                &have.to_string(),
            ),
            "needs",
            &needs.to_string(),
        ),
    }
}

/// Why the sim would not do it, as a player reads it. `print` is the
/// blueprint asked of, whose slots a refusal names.
#[must_use]
pub fn refusal_words(
    registries: &GameRegistries,
    refusal: &FactoryRefusal,
    print: Option<&Blueprint>,
) -> String {
    let plain = |key: &'static str| words(key).to_owned();
    match refusal {
        FactoryRefusal::NoPlayer => plain("strata.factory.refused.no_player"),
        FactoryRefusal::NoFabricator => plain("strata.factory.refused.no_fabricator"),
        FactoryRefusal::Busy => plain("strata.factory.refused.busy"),
        FactoryRefusal::NoBlueprint => plain("strata.factory.refused.no_blueprint"),
        FactoryRefusal::Stale => plain("strata.factory.refused.stale"),
        FactoryRefusal::NoBatch => plain("strata.factory.refused.no_batch"),
        FactoryRefusal::OverRuns { left } => fill(
            words("strata.factory.refused.over_runs"),
            "runs",
            &runs_words(*left),
        ),
        FactoryRefusal::Slot { slot, why } => {
            let need =
                print.and_then(|print| print.slots.iter().find(|need| need.slot.id == *slot));
            let said = need.map_or_else(
                || why.to_string(),
                |need| feed_refusal_words(registries, &need.slot, why),
            );
            fill(
                &fill(
                    words("strata.factory.refused.slot"),
                    "slot",
                    need.map_or(slot.as_str(), |need| need.slot.name.as_str()),
                ),
                "why",
                &said,
            )
        }
        FactoryRefusal::NoJob => plain("strata.factory.refused.no_job"),
        FactoryRefusal::NoRoom => plain("strata.factory.refused.no_room"),
        FactoryRefusal::NotOnBench => plain("strata.factory.refused.not_on_bench"),
    }
}

/// What a run said of itself, as its toast says it.
#[must_use]
pub fn notice_words(registries: &GameRegistries, notice: &FactoryNotice) -> String {
    match notice {
        FactoryNotice::Finished {
            item,
            made,
            batch,
            spent,
        } => {
            let name =
                card::plain_name(&registries.items, item).unwrap_or_else(|| item.to_string());
            let mut said = if *made == 1 {
                fill(words("strata.factory.toast.finished.one"), "name", &name)
            } else {
                fill(
                    &fill(
                        words("strata.factory.toast.finished"),
                        "count",
                        &made.to_string(),
                    ),
                    "names",
                    &counted(&name, *made),
                )
            };
            said = fill(&said, "batch", &batch.to_string());
            if *spent {
                said.push_str(words("strata.factory.toast.spent"));
            }
            said
        }
        FactoryNotice::Paused(Pause::NoRoom) => words("strata.factory.toast.paused").to_owned(),
        FactoryNotice::Resumed => words("strata.factory.toast.resumed").to_owned(),
    }
}

/// The first line of what `print` makes with its figure, as the item's
/// own order has it: the line a list leads a blueprint with.
fn lead_of(registries: &GameRegistries, print: &Blueprint) -> String {
    registries
        .items
        .get(&print.makes)
        .and_then(|def| def.lines.first())
        .and_then(|line| print.lines.get(&line.id))
        .map_or_else(String::new, u16::to_string)
}

/// What `item` is called at the head of a list: its item's name.
fn product_name(registries: &GameRegistries, item: &Namespaced) -> String {
    registries
        .items
        .get(item)
        .map_or_else(|| item.to_string(), |def| def.name.clone())
}

/// `20 units` of a lot, a bare count of a part.
fn amount_words(slot: &SlotDef, amount: u32) -> String {
    if slot.takes.is_resource() {
        fill(
            words("strata.factory.hopper.units"),
            "units",
            &amount.to_string(),
        )
    } else {
        amount.to_string()
    }
}

impl FeedRow {
    /// The row of `feed` for the slot `need`, picked or not.
    fn of(registries: &GameRegistries, need: &SlotNeed, feed: &Feed, on: bool) -> Self {
        let count = |key: &'static str, held: u32| fill(words(key), "n", &held.to_string());
        let held: Vec<String> = [
            ("strata.factory.feed.pack", feed.carried),
            ("strata.factory.feed.store", feed.stored),
        ]
        .into_iter()
        .filter(|(_, held)| *held > 0)
        .map(|(key, held)| count(key, held))
        .collect();
        Self {
            kind: feed.kind.clone(),
            name: stock_name(registries, &feed.kind),
            batch: if feed.kind.by.is_some() {
                fill(
                    words("strata.crafting.pick.batch"),
                    "batch",
                    &feed.kind.batch.to_string(),
                )
            } else {
                String::new()
            },
            swatch: swatch_of(registries, &feed.kind),
            figures: feed
                .figures
                .iter()
                .map(|(stat, figure)| {
                    (
                        card::need_name(registries, &need.slot, stat),
                        figure.to_string(),
                    )
                })
                .collect(),
            held: held.join(" \u{b7} "),
            refused: feed
                .verdict
                .as_ref()
                .err()
                .map(|refusal| feed_refusal_words(registries, &need.slot, refusal)),
            on: on && feed.verdict.is_ok(),
        }
    }
}

impl SlotView {
    /// The view of the slot `need` of `print`, with what `ui` has picked
    /// for it.
    fn of(floor: &Floor<'_>, ui: &FactoryUi, print: &Blueprint, need: &SlotNeed) -> Self {
        let registries = floor.registries;
        let slot = &need.slot;
        let picked = ui.feeds.get(&slot.id);
        let rows: Vec<FeedRow> =
            factory::feeds(registries, floor.pack, floor.reach(), print, &slot.id)
                .iter()
                .map(|feed| {
                    let on = picked.is_some_and(|picked| picked.contains(&feed.kind));
                    FeedRow::of(registries, need, feed, on)
                })
                .collect();
        let asked: Vec<String> = need
            .needs
            .iter()
            .flat_map(|asked| {
                let name = card::need_name(registries, slot, &asked.stat);
                let phrase = |key: &'static str, figure: u16| {
                    fill(&fill(words(key), "stat", &name), "n", &figure.to_string())
                };
                [
                    asked
                        .at_least
                        .map(|least| phrase("strata.factory.need.least", least)),
                    asked
                        .at_most
                        .map(|most| phrase("strata.factory.need.most", most)),
                ]
            })
            .flatten()
            .collect();
        let meets = rows.iter().any(|row| row.refused.is_none());
        let none = (!meets).then(|| {
            words(match (rows.is_empty(), floor.at_fabricator) {
                (true, true) => "strata.factory.slot.none",
                (true, false) => "strata.factory.slot.none.pack",
                (false, _) => "strata.factory.slot.unmet",
            })
            .to_owned()
        });
        let (each, name) = if slot.takes.is_resource() {
            ("strata.factory.slot.units", "units")
        } else {
            ("strata.factory.slot.count", "count")
        };
        Self {
            id: slot.id.clone(),
            label: slot.name.to_uppercase(),
            each: fill(words(each), name, &slot.amount().to_string()),
            needs: if asked.is_empty() {
                words("strata.factory.needs.none").to_owned()
            } else {
                fill(
                    words("strata.factory.needs"),
                    "list",
                    &asked.join(" \u{b7} "),
                )
            },
            rows,
            none,
        }
    }
}

impl BatchView {
    /// The batch `size` of `print`, with the sim's limit and what holds it.
    fn of(floor: &Floor<'_>, print: &Blueprint, size: u32) -> Self {
        let limit = factory::limit(floor.registries, floor.pack, floor.reach(), print);
        let most = limit.most;
        let slot_name = |id: &str| {
            print
                .slots
                .iter()
                .find(|need| need.slot.id == id)
                .map_or_else(|| id.to_owned(), |need| need.slot.name.clone())
        };
        let why = match &limit.held_by {
            Held::Runs { left } => fill(
                &fill(
                    words("strata.factory.batch.runs"),
                    "most",
                    &most.to_string(),
                ),
                "runs",
                &runs_words(*left),
            ),
            Held::Stock { slot, feeds } if *feeds == 0 => {
                fill(words("strata.factory.batch.none"), "slot", &slot_name(slot))
            }
            Held::Stock { slot, feeds } => fill(
                &fill(
                    &fill(
                        words("strata.factory.batch.stock"),
                        "most",
                        &most.to_string(),
                    ),
                    "slot",
                    &slot_name(slot),
                ),
                "feeds",
                &feeds.to_string(),
            ),
        };
        // A press that would change nothing is grey, and the line under
        // the row says what holds the batch.
        let to = |wanted: u32| {
            let held = wanted.clamp(1, most.max(1));
            (held != size).then_some(held)
        };
        let step = |id: &'static str, label: String, wanted: u32| BatchStep {
            id,
            label,
            to: to(wanted),
        };
        let signed = |sign: char, by: u32| format!("{sign}{by}");
        Self {
            size,
            most,
            steps: vec![
                step(
                    "less10",
                    signed('\u{2212}', BATCH_STEP),
                    size.saturating_sub(BATCH_STEP),
                ),
                step("less", signed('\u{2212}', 1), size.saturating_sub(1)),
                step("more", signed('+', 1), size.saturating_add(1)),
                step(
                    "more10",
                    signed('+', BATCH_STEP),
                    size.saturating_add(BATCH_STEP),
                ),
                step("most", words("strata.factory.batch.most").to_owned(), most),
            ],
            why,
            held: size >= most,
        }
    }
}

/// Who can work a run: nobody, then everyone on the bench, with `chosen`
/// marked.
fn worker_chips(rules: &FactoryRules, benched: &Benched, chosen: Option<&str>) -> Vec<WorkerChip> {
    let nobody = WorkerChip {
        name: None,
        label: fill(
            words("strata.factory.worker.none"),
            "speed",
            &speed_words(rules.rate_pm(false)),
        ),
        on: chosen.is_none(),
    };
    std::iter::once(nobody)
        .chain(benched.0.iter().map(|name| WorkerChip {
            name: Some(name.clone()),
            label: fill(
                &fill(words("strata.factory.worker.one"), "name", name),
                "speed",
                &speed_words(rules.rate_pm(true)),
            ),
            on: chosen == Some(name.as_str()),
        }))
        .collect()
}

impl IdleStage {
    /// The blueprint `id` as it would be run with what `ui` holds.
    fn of(
        floor: &Floor<'_>,
        ui: &FactoryUi,
        rules: &FactoryRules,
        id: u32,
        print: &Blueprint,
    ) -> Self {
        let registries = floor.registries;
        let size = ui.batch.max(1);
        let worker = ui.worker.as_deref();
        // Asked in the sim's own order, so the reason over the button is
        // the one a request would be refused for.
        let refused = if floor.at_fabricator {
            factory::check_start(
                registries,
                floor.pack,
                floor.reach(),
                floor.prints,
                floor.state,
                id,
                size,
                &ui.feeds,
            )
            .err()
        } else {
            Some(FactoryRefusal::NoFabricator)
        };
        let plain = card::plain_name(&registries.items, &print.makes)
            .unwrap_or_else(|| print.makes.to_string());
        let makes = [
            ("count", size.to_string()),
            ("names", counted(&plain, size)),
            ("lead", lead_of(registries, print)),
            (
                "time",
                time_words(factory::seconds(
                    rules,
                    print.complexity,
                    size,
                    worker.is_some(),
                )),
            ),
        ]
        .iter()
        .fold(
            words("strata.factory.makes").to_owned(),
            |said, (name, with)| fill(&said, name, with),
        );
        Self {
            id,
            head: fill(
                &fill(
                    words("strata.factory.head"),
                    "name",
                    &product_name(registries, &print.makes).to_uppercase(),
                ),
                "id",
                &id.to_string(),
            ),
            batch: BatchView::of(floor, print, size),
            slots: print
                .slots
                .iter()
                .map(|need| SlotView::of(floor, ui, print, need))
                .collect(),
            makes,
            workers: worker_chips(rules, floor.benched, worker),
            why: refused.as_ref().map_or_else(String::new, |refusal| {
                refusal_words(registries, refusal, Some(print))
            }),
            request: refused.is_none().then(|| FactoryRequest::Start {
                blueprint: id,
                batch: size,
                feeds: ui.feeds.clone(),
                worker: ui.worker.clone(),
            }),
        }
    }
}

impl RunStage {
    /// The run `job` as it stands.
    fn of(floor: &Floor<'_>, rules: &FactoryRules, job: &Job) -> Self {
        let registries = floor.registries;
        let print = floor.prints.0.get(&job.blueprint);
        let name = print.map_or_else(String::new, |print| {
            product_name(registries, &print.makes).to_uppercase()
        });
        let head = [
            ("name", name),
            ("id", job.blueprint.to_string()),
            ("batch", job.batch.to_string()),
        ]
        .iter()
        .fold(
            words("strata.factory.head.run").to_owned(),
            |said, (name, with)| fill(&said, name, with),
        );
        let stop = if floor.at_fabricator {
            factory::can_stop(&registries.items, floor.prints, job, floor.store).err()
        } else {
            Some(FactoryRefusal::NoFabricator)
        };
        let share =
            u64::from(job.made.min(job.size)) * u64::from(QUALITY_MAX) / u64::from(job.size.max(1));
        Self {
            head,
            made: fill(
                &fill(words("strata.factory.made"), "made", &job.made.to_string()),
                "size",
                &job.size.to_string(),
            ),
            share: u16::try_from(share).unwrap_or(QUALITY_MAX),
            left: match (job.paused, print) {
                (None, Some(print)) => fill(
                    words("strata.factory.left"),
                    "time",
                    &time_words(factory::left(rules, print, job, floor.clock)),
                ),
                _ => String::new(),
            },
            paused: job.paused.map(|pause| match pause {
                Pause::NoRoom => words("strata.factory.paused").to_owned(),
            }),
            workers: worker_chips(rules, floor.benched, job.worker.as_deref()),
            hopper: job
                .inputs
                .iter()
                .map(|input| {
                    let slot = print.and_then(|print| {
                        print.slots.iter().find(|need| need.slot.id == input.slot)
                    });
                    (
                        stock_name(registries, &input.kind),
                        slot.map_or_else(
                            || input.left.to_string(),
                            |need| amount_words(&need.slot, input.left),
                        ),
                    )
                })
                .collect(),
            can_stop: stop.is_none(),
            stop_why: stop.as_ref().map_or_else(String::new, |refusal| {
                refusal_words(registries, refusal, print)
            }),
        }
    }
}

/// The blueprint the screen shows: the one the fabricator is running, else
/// the one the state names while the stock holds it, else the first the
/// stock holds.
fn shown_of(floor: &Floor<'_>, ui: &FactoryUi) -> Option<u32> {
    let held = floor.held();
    floor
        .state
        .job
        .as_ref()
        .map(|job| job.blueprint)
        .or_else(|| {
            ui.blueprint
                .filter(|id| held.iter().any(|(own, _)| own == id))
        })
        .or_else(|| held.first().map(|(id, _)| *id))
}

impl FactoryView {
    /// The view of `floor` for the screen's `ui`.
    #[must_use]
    #[allow(
        clippy::too_many_lines,
        reason = "one screen read left to right, as it is shown"
    )]
    pub fn of(floor: &Floor<'_>, ui: &FactoryUi) -> Self {
        let registries = floor.registries;
        let rules = registries.schematics.factory();
        let job = floor.state.job.as_ref();
        let shown = shown_of(floor, ui);
        let row = |id: u32, print: &Blueprint| PrintRow {
            id,
            name: product_name(registries, &print.makes),
            lead: lead_of(registries, print),
            runs: runs_words(print.runs_left),
            on: shown == Some(id),
            busy: job
                .filter(|job| job.blueprint != id)
                .map(|_| words("strata.factory.refused.busy").to_owned()),
        };
        // The fabricator holds the blueprint it is running: it is in no
        // stack, and is listed first all the same.
        let running =
            job.and_then(|job| Some((job.blueprint, floor.prints.0.get(&job.blueprint)?)));
        let prints: Vec<PrintRow> = running
            .into_iter()
            .chain(floor.held())
            .map(|(id, print)| row(id, print))
            .collect();
        let mut stage = match (rules, job, shown) {
            (Some(rules), Some(job), _) => Some(Stage::Run(RunStage::of(floor, rules, job))),
            (Some(rules), None, Some(id)) => floor.prints.0.get(&id).map(|print| {
                // The pace this sheet would start the run at, so the time
                // shown is the time it will take.
                let pace = factory::pace_of(rules, &floor.registries.progression, floor.sheet);
                let rules = factory::paced(rules, pace);
                Stage::Idle(IdleStage::of(floor, ui, &rules, id, print))
            }),
            _ => None,
        };
        // A refusal is said at the button that was pressed as well as in
        // the footer, when the screen had no reason of its own there.
        if let Some((said, true)) = &ui.notice {
            match &mut stage {
                Some(Stage::Idle(idle)) if idle.why.is_empty() => idle.why.clone_from(said),
                Some(Stage::Run(run)) if run.stop_why.is_empty() => run.stop_why.clone_from(said),
                _ => {}
            }
        }
        let resting = words(match (&stage, floor.at_fabricator) {
            (_, false) => "strata.factory.status.reading",
            (Some(Stage::Run(_)), true) => "strata.factory.status.running",
            (_, true) => "strata.factory.status.idle",
        })
        .to_owned();
        let (status, keys) = match &ui.notice {
            Some(notice) => (notice.clone(), Some(resting)),
            None => ((resting, false), None),
        };
        let working = match (&stage, job) {
            (_, Some(job)) => job.worker.clone(),
            _ => ui.worker.clone(),
        };
        let worker = fill(
            words("strata.factory.worker"),
            "name",
            &working
                .unwrap_or_else(|| {
                    fill(
                        words("strata.factory.worker.none"),
                        "speed",
                        &speed_words(rules.map_or(QUALITY_MAX, |rules| rules.rate_pm(false))),
                    )
                })
                .to_uppercase(),
        );
        let mut view = Self {
            worker,
            purse: floor.purse.0,
            none: prints.is_empty().then(|| {
                words(if floor.at_fabricator {
                    "strata.factory.none"
                } else {
                    "strata.factory.none.pack"
                })
                .to_owned()
            }),
            prints,
            stage,
            at_fabricator: floor.at_fabricator,
            status,
            keys,
            focus: None,
        };
        view.focus = view.focus_for(ui.last.as_deref());
        view
    }

    /// The row of the kind at `feed` (a row's `test_id` without its
    /// prefix), with its slot's key.
    fn feed_row(&self, feed: &str) -> Option<(&str, &FeedRow)> {
        let Some(Stage::Idle(idle)) = &self.stage else {
            return None;
        };
        let (slot, at) = feed.rsplit_once(':')?;
        let slot = idle.slots.iter().find(|own| own.id == slot)?;
        Some((slot.id.as_str(), slot.rows.get(at.parse::<usize>().ok()?)?))
    }

    /// Where the keyboard's focus starts: on the control last used, when
    /// it is still there; else on the row of the blueprint shown. A press
    /// on a blueprint or on Cheapest stock leads to Run, and to Cheapest
    /// stock while Run is grey; Run and Stop lead back to the blueprint's
    /// row, so that a second Accept does not undo the first. So a run is
    /// started from the fabricator's F by Accept, pressed in place.
    fn focus_for(&self, last: Option<&str>) -> Option<String> {
        let row = || {
            self.prints
                .iter()
                .find(|print| print.on)
                .map_or_else(|| CLOSE_ID.to_owned(), |print| print_id(print.id))
        };
        let Some(last) = last else {
            return Some(row());
        };
        let keep = || Some(last.to_owned());
        if let Some(id) = last.strip_prefix(PRINT_TAG) {
            let there = self
                .prints
                .iter()
                .any(|print| print.id.to_string() == id && print.busy.is_none());
            return if there { keep() } else { Some(row()) };
        }
        let chip = |workers: &[WorkerChip]| {
            last.strip_prefix(WORKER_TAG)
                .and_then(|at| at.parse::<usize>().ok())
                .is_some_and(|at| at < workers.len())
        };
        match &self.stage {
            Some(Stage::Idle(idle)) => {
                if last == RUN {
                    Some(
                        if idle.request.is_some() {
                            RUN
                        } else {
                            CHEAPEST
                        }
                        .to_owned(),
                    )
                } else if last == CHEAPEST
                    || chip(&idle.workers)
                    || last
                        .strip_prefix(BATCH_TAG)
                        .is_some_and(|step| idle.batch.steps.iter().any(|own| own.id == step))
                    || last
                        .strip_prefix(FEED_TAG)
                        .is_some_and(|feed| self.feed_row(feed).is_some())
                {
                    keep()
                } else {
                    Some(row())
                }
            }
            Some(Stage::Run(run)) if chip(&run.workers) => keep(),
            _ => Some(row()),
        }
    }
}

// ---- the definition ----

/// One blueprint's row: what it makes, its lead line's figure, and under
/// them the runs it has left. The whole row is the button.
fn print_row(view: &PrintRow) -> UiNodeDef {
    let id = print_id(view.id);
    let name_role = if view.busy.is_some() {
        SCHEM_NAME_CANT_ROLE
    } else {
        SCHEM_NAME_ROLE
    };
    let named = text_in(
        if view.on {
            format!("{FOCUS_MARK}{}", view.name)
        } else {
            view.name.clone()
        },
        TextRole::Body,
        name_role,
    );
    let mut children = vec![
        panel(
            "invisible",
            Layout {
                width: Some(Length::Percent(100.)),
                ..row(1.3)
            },
            vec![
                grows(0., vec![named]),
                line_in(view.lead.clone(), FIGURE_ROLE),
            ],
        ),
        line_in(view.runs.clone(), SUB_ROLE),
    ];
    // Why it cannot be picked, under the name across the row's width.
    children.extend(
        view.busy
            .iter()
            .map(|why| text_in(why.clone(), TextRole::Caption, SUB_ROLE)),
    );
    UiNodeDef::Card {
        role: Role::new(if view.on { SCHEM_ON_ROLE } else { SCHEM_ROLE }),
        layout: Layout {
            width: Some(Length::Percent(100.)),
            padding: Padding {
                top: 0.9,
                right: 1.4,
                bottom: 0.9,
                left: 1.4,
            },
            align: Some(Align::Stretch),
            ..column(0.3)
        },
        children,
        disabled: view.busy.is_some(),
        tags: Tags::new().with("menu", &id).with(Tags::TEST_ID, &id),
    }
}

/// The left column: the blueprints, or that there is none.
fn prints_column(view: &FactoryView) -> Vec<UiNodeDef> {
    let mut left = vec![head("strata.factory.blueprints")];
    left.extend(view.prints.iter().map(print_row));
    left.extend(
        view.none
            .iter()
            .map(|none| with_id(text_in(none.clone(), TextRole::Caption, NOTE_ROLE), NONE_ID)),
    );
    left
}

/// The figures a slot asks of, of one kind of stack: each one's name, then
/// its figure lit.
fn figures(of: &FeedRow) -> UiNodeDef {
    let mut cells = Vec::new();
    for (index, (name, figure)) in of.figures.iter().enumerate() {
        if index > 0 {
            cells.push(line_in("\u{b7}", STAT_ROLE));
        }
        cells.push(line_in(name.clone(), STAT_ROLE));
        cells.push(line_in(figure.clone(), STAT_VALUE_ROLE));
    }
    panel(
        "invisible",
        Layout {
            wrap: true,
            ..row(0.8)
        },
        cells,
    )
}

/// One kind of stack in a slot's list: its shape, its name with the mark
/// that says it meets the slot or feeds it, the figures the slot asks of,
/// and how much the stock holds. One that does not meet the slot is drawn
/// a step under the rest, inert, with the figure it fails on and what is
/// asked in the refusal's colour: it takes the focus and says why.
fn feed_row(slot: &SlotView, at: usize, of: &FeedRow) -> UiNodeDef {
    let id = feed_id(&slot.id, at);
    let mut named = vec![text_in(
        if of.on {
            format!("{FOCUS_MARK}{}", of.name)
        } else {
            of.name.clone()
        },
        TextRole::Body,
        if of.refused.is_some() {
            FAIL_ROLE
        } else {
            NAME_ROLE
        },
    )];
    if of.refused.is_none() {
        named.push(with_id(
            line_in(
                if of.on {
                    "strata.factory.feed.feeds"
                } else {
                    "strata.factory.feed.meets"
                },
                if of.on { FEEDS_ROLE } else { TAG_ROLE },
            ),
            &mark_id(&slot.id, at),
        ));
    }
    if !of.batch.is_empty() {
        named.push(line_in(of.batch.clone(), SUB_ROLE));
    }
    let under = of.refused.as_ref().map_or_else(
        || figures(of),
        |why| text_in(why.clone(), TextRole::Caption, WHY_ROLE),
    );
    UiNodeDef::Card {
        role: Role::new(if of.on { PICK_ON_ROLE } else { PICK_ROLE }),
        layout: Layout {
            width: Some(Length::Percent(100.)),
            padding: Padding {
                top: 0.6,
                right: 1.6,
                bottom: 0.6,
                left: 0.8,
            },
            ..row(1.4)
        },
        children: vec![
            well(of.swatch.clone()),
            grows(
                0.1,
                vec![
                    panel(
                        "invisible",
                        Layout {
                            wrap: true,
                            ..row(1.)
                        },
                        named,
                    ),
                    under,
                ],
            ),
            line_in(
                of.held.clone(),
                if of.refused.is_some() {
                    SUB_ROLE
                } else {
                    FIGURE_ROLE
                },
            ),
        ],
        disabled: of.refused.is_some(),
        tags: Tags::new().with("menu", &id).with(Tags::TEST_ID, &id).with(
            "state",
            words(match (&of.refused, of.on) {
                (Some(_), _) => "strata.factory.feed.wrong",
                (None, true) => "strata.factory.feed.feeds",
                (None, false) => "strata.factory.feed.meets",
            }),
        ),
    }
}

/// One slot's box: what it takes a unit, what it needs, and what the stock
/// has for it, or that it has nothing.
fn slot_box(slot: &SlotView) -> UiNodeDef {
    let mut children = vec![panel(
        "invisible",
        Layout {
            width: Some(Length::Percent(100.)),
            wrap: true,
            ..row(1.4)
        },
        vec![
            head(slot.label.clone()),
            line_in(slot.each.clone(), SUB_ROLE),
            with_id(
                line_in(slot.needs.clone(), NEED_ROLE),
                &format!("{NEED_TAG}{}", slot.id),
            ),
        ],
    )];
    children.extend(
        slot.rows
            .iter()
            .enumerate()
            .map(|(at, row)| feed_row(slot, at, row)),
    );
    children.extend(slot.none.iter().map(|none| {
        with_id(
            text_in(none.clone(), TextRole::Caption, WHY_ROLE),
            &format!("{NOTHING_TAG}{}", slot.id),
        )
    }));
    with_id(
        panel(
            SLOT_ROLE,
            Layout {
                width: Some(Length::Percent(100.)),
                padding: Padding {
                    top: 1.2,
                    right: 1.6,
                    bottom: 1.2,
                    left: 1.6,
                },
                align: Some(Align::Stretch),
                ..column(0.7)
            },
            children,
        ),
        &format!("{SLOT_TAG}{}", slot.id),
    )
}

/// A small button of a row of them: a batch's step.
fn small(label: &str, id: &str, can: bool) -> UiNodeDef {
    UiNodeDef::Button {
        widget: None,
        opts: ButtonOpts {
            label: Some(key(label)),
            disabled: !can,
            compact: true,
            role: Some(Role::new(ACT_ROLE)),
            ..ButtonOpts::default()
        },
        tags: Tags::new().with("menu", id).with(Tags::TEST_ID, id),
    }
}

/// The right column's first line: the blueprint shown and, with the
/// fabricator free, its batch between the buttons that change it.
fn stage_head(said: &str, batch: Option<&BatchView>) -> UiNodeDef {
    let mut cells = vec![with_id(head(said.to_owned()), HEAD_ID), spacer()];
    if let Some(batch) = batch {
        let (less, more) = batch.steps.split_at(2);
        let button = |step: &BatchStep| small(&step.label, &batch_id(step.id), step.to.is_some());
        cells.push(head("strata.factory.batch"));
        cells.extend(less.iter().map(button));
        cells.push(with_id(
            line_in(batch.size.to_string(), BATCH_ROLE),
            BATCH_ID,
        ));
        cells.extend(more.iter().map(button));
    }
    panel(
        "invisible",
        Layout {
            width: Some(Length::Percent(100.)),
            ..row(1.)
        },
        cells,
    )
}

/// The row of who works the run: a chip a choice, the one chosen lit and
/// marked.
fn workers_row(workers: &[WorkerChip]) -> UiNodeDef {
    let mut cells = vec![head("strata.factory.workers")];
    cells.extend(workers.iter().enumerate().map(|(at, chip)| {
        let id = worker_id(at);
        UiNodeDef::Card {
            role: Role::new(if chip.on { PICK_ON_ROLE } else { PICK_ROLE }),
            layout: Layout {
                padding: Padding {
                    top: 0.5,
                    right: 1.6,
                    bottom: 0.5,
                    left: 1.6,
                },
                ..row(0.)
            },
            // The one chosen says so by a mark as well as by its fill.
            children: vec![line_in(
                if chip.on {
                    format!("{FOCUS_MARK}{}", chip.label)
                } else {
                    chip.label.clone()
                },
                STAT_VALUE_ROLE,
            )],
            disabled: false,
            tags: Tags::new()
                .with("menu", &id)
                .with(Tags::TEST_ID, &id)
                .with("selected", if chip.on { "true" } else { "false" }),
        }
    }));
    if workers.len() < 2 {
        cells.push(line_in("strata.factory.worker.empty", SUB_ROLE));
    }
    panel(
        "invisible",
        Layout {
            width: Some(Length::Percent(100.)),
            wrap: true,
            ..row(1.)
        },
        cells,
    )
}

/// The right column with the fabricator free: the batch, the slots, and at
/// the foot what the run would make, who works it, and the two buttons
/// with why Run is grey over them.
fn idle_column(idle: &IdleStage) -> Vec<UiNodeDef> {
    let batch = &idle.batch;
    let mut right = vec![
        stage_head(&idle.head, Some(batch)),
        panel(
            "invisible",
            Layout {
                width: Some(Length::Percent(100.)),
                justify: Justify::End,
                ..row(0.)
            },
            vec![with_id(
                line_in(
                    batch.why.clone(),
                    if batch.held { WHY_ROLE } else { WHY_HINT_ROLE },
                ),
                BATCH_WHY_ID,
            )],
        ),
    ];
    right.extend(idle.slots.iter().map(slot_box));
    // What follows stands at the column's foot, so Run is where it was
    // whatever the slots list.
    right.push(spacer());
    right.push(with_id(
        line_in(idle.makes.clone(), "strata.crafting.means"),
        MAKES_ID,
    ));
    right.push(workers_row(&idle.workers));
    right.push(with_id(
        text_in(idle.why.clone(), TextRole::Caption, WHY_ROLE),
        WHY_ID,
    ));
    right.push(panel(
        "invisible",
        row(1.6),
        vec![
            act("strata.factory.cheapest", CHEAPEST, false, true),
            act("strata.factory.run", RUN, true, idle.request.is_some()),
        ],
    ));
    right
}

/// The bar of how much of the batch is made. Drawn from two panels: the
/// count beside it is the figure, and the bar only its picture.
fn progress(share: u16) -> UiNodeDef {
    with_id(
        panel(
            PROGRESS_ROLE,
            Layout {
                width: Some(Length::Percent(100.)),
                height: Some(Length::Px(8.)),
                ..row(0.)
            },
            vec![panel(
                PROGRESS_FILL_ROLE,
                Layout {
                    width: Some(Length::Percent(card::track_percent(share))),
                    height: Some(Length::Px(8.)),
                    ..row(0.)
                },
                Vec::new(),
            )],
        ),
        BAR_ID,
    )
}

/// The right column with a run on: how many are made, the time left or
/// why it stands, who works it, what is still in the hopper, and Stop.
fn run_column(run: &RunStage) -> Vec<UiNodeDef> {
    let mut made = vec![
        head("strata.factory.made.head"),
        with_id(line_in(run.made.clone(), "strata.crafting.means"), MADE_ID),
        spacer(),
    ];
    // A run that stands says so in a word, where the time would be.
    made.push(match &run.paused {
        Some(paused) => with_id(line_in(paused.clone(), PAUSED_ROLE), PAUSED_ID),
        None => with_id(line_in(run.left.clone(), FIGURE_ROLE), LEFT_ID),
    });
    let mut hopper = vec![with_id(head("strata.factory.hopper"), HOPPER_ID)];
    if run.hopper.is_empty() {
        hopper.push(text_in(
            "strata.factory.hopper.empty",
            TextRole::Caption,
            NOTE_ROLE,
        ));
    }
    hopper.extend(run.hopper.iter().map(|(name, amount)| {
        panel(
            "invisible",
            Layout {
                width: Some(Length::Percent(100.)),
                ..row(1.4)
            },
            vec![
                text_in(name.clone(), TextRole::Body, NAME_ROLE),
                spacer(),
                line_in(amount.clone(), FIGURE_ROLE),
            ],
        )
    }));
    vec![
        stage_head(&run.head, None),
        panel(
            BOX_ROLE,
            Layout {
                width: Some(Length::Percent(100.)),
                padding: Padding {
                    top: 1.4,
                    right: 1.6,
                    bottom: 1.4,
                    left: 1.6,
                },
                align: Some(Align::Stretch),
                ..column(1.)
            },
            vec![
                panel(
                    "invisible",
                    Layout {
                        width: Some(Length::Percent(100.)),
                        ..row(1.2)
                    },
                    made,
                ),
                progress(run.share),
            ],
        ),
        panel(
            SLOT_ROLE,
            Layout {
                width: Some(Length::Percent(100.)),
                padding: Padding {
                    top: 1.2,
                    right: 1.6,
                    bottom: 1.2,
                    left: 1.6,
                },
                align: Some(Align::Stretch),
                ..column(0.6)
            },
            hopper,
        ),
        spacer(),
        workers_row(&run.workers),
        with_id(
            text_in(run.stop_why.clone(), TextRole::Caption, WHY_ROLE),
            STOP_WHY_ID,
        ),
        panel(
            "invisible",
            row(1.6),
            vec![act("strata.factory.stop", STOP, false, run.can_stop)],
        ),
    ]
}

/// The head: the title, who works the run, and at the right the
/// household's credits.
fn head_row(view: &FactoryView) -> UiNodeDef {
    panel(
        "invisible",
        Layout {
            align: Some(Align::Center),
            ..row(2.)
        },
        vec![
            with_id(
                UiNodeDef::Text {
                    key: key("strata.factory.title"),
                    style: TextRole::Title,
                    opts: TextOpts {
                        wrap: false,
                        role: Some(Role::new("panel.title")),
                        ..TextOpts::default()
                    },
                    tags: Tags::new(),
                },
                TITLE_ID,
            ),
            with_id(line_in(view.worker.clone(), HEAD_ROLE), WORKER_ID),
            spacer(),
            super::stores::purse(PURSE_ID, view.purse),
        ],
    )
}

/// The foot: what the sim last said, or what the screen is for, then the
/// keys and Close.
fn footer(view: &FactoryView) -> UiNodeDef {
    let mut said = vec![with_id(
        text_in(
            view.status.0.clone(),
            TextRole::Caption,
            if view.status.1 {
                REFUSED_ROLE
            } else {
                STATUS_ROLE
            },
        ),
        STATUS_ID,
    )];
    said.extend(view.keys.iter().map(|keys| {
        with_id(
            text_in(keys.clone(), TextRole::Caption, STATUS_ROLE),
            KEYS_ID,
        )
    }));
    panel(
        "invisible",
        row(2.),
        vec![
            grows(0.2, said),
            UiNodeDef::Custom {
                kind: WidgetKind::new("slotted:hint_bar"),
                params: Value::Unit,
                children: Vec::new(),
                tags: Tags::new().with(Tags::TEST_ID, "factory.hints"),
            },
            UiNodeDef::Button {
                widget: Some(WidgetKind::new("slotted:close")),
                opts: ButtonOpts {
                    label: Some(key("strata.menu.close")),
                    ..ButtonOpts::default()
                },
                tags: Tags::new().with(Tags::TEST_ID, CLOSE_ID),
            },
        ],
    )
}

/// The whole screen for `view`.
#[must_use]
pub fn screen_def(view: &FactoryView) -> ScreenDef {
    let right = match &view.stage {
        Some(Stage::Idle(idle)) => idle_column(idle),
        Some(Stage::Run(run)) => run_column(run),
        None => Vec::new(),
    };
    ScreenDef {
        kind: kind(),
        inherits: None,
        root: UiNodeDef::Panel {
            role: Role::new("panel"),
            layout: Layout {
                width: Some(Length::Px(WIDTH)),
                max_width: Some(Length::Percent(96.)),
                // The workbench's frame, so the two stations read as one
                // place and the panel does not jump from idle to running.
                min_height: Some(Length::Percent(MIN_HEIGHT_VH)),
                max_height: Some(Length::Percent(94.)),
                padding: Padding::all(3.),
                align: Some(Align::Stretch),
                justify: Justify::Start,
                ..column(2.)
            },
            children: vec![
                head_row(view),
                // The columns scroll inside the panel on a window too
                // short for them, so the footer and its Close stay inside
                // the border.
                // A scroll panel, not a clipped one: the wheel and the bar move it.
                UiNodeDef::Scroll {
                    layout: Layout {
                        align: Some(Align::Stretch),
                        grow: 1.,
                        min_height: Some(Length::Px(0.)),
                        ..Layout {
                            direction: LayoutDirection::Row,
                            gap: 2.6,
                            ..Layout::default()
                        }
                    },
                    scrollbar: true,
                    children: vec![
                        panel(
                            "invisible",
                            Layout {
                                width: Some(Length::Px(SCHEMATICS_WIDTH)),
                                align: Some(Align::Stretch),
                                ..column(1.)
                            },
                            prints_column(view),
                        ),
                        panel(
                            "invisible",
                            Layout {
                                grow: 1.,
                                min_width: Some(Length::Px(0.)),
                                align: Some(Align::Stretch),
                                ..column(1.2)
                            },
                            right,
                        ),
                    ],
                    tags: Tags::new(),
                },
                footer(view),
            ],
            tags: Tags::new().with(Tags::TEST_ID, ROOT_ID),
        },
        listring: Vec::new(),
        remove: Vec::new(),
        initial_focus: view.focus.clone(),
        presentation: Presentation {
            mode: PresentationMode::Page,
            scrim: Some(true),
            transition: Transition::Fade,
            wildcards: Some(false),
            ..Presentation::default()
        },
    }
}

// ---- open, answer, rebuild ----

/// Whether the factory screen is the one on top.
fn on_top(stack: &ScreenStack) -> bool {
    stack.top().is_some_and(|top| top.kind == kind())
}

/// Reads the world as a [`Floor`] and gives it to `read` with the world.
/// `None` without a player or the registries.
fn with_floor<R>(world: &mut World, read: impl FnOnce(&Floor<'_>, &World) -> R) -> Option<R> {
    let registries = world.get_resource::<Registries>()?.0.clone();
    let mut players = world.query_filtered::<(&Pack, Option<&CharacterSheet>), With<LocalPlayer>>();
    let world = &*world;
    // The sim's own rule, so the buttons are grey exactly when a request
    // would be refused for it.
    let at_fabricator = world.get_resource::<Reach>().is_some_and(|reach| {
        let access = world.get_resource::<WorkbenchAccess>().copied();
        factory::at_fabricator(access.unwrap_or_default(), reach)
    });
    let (pack, sheet) = players.single(world).ok()?;
    let plain = CharacterSheet::default();
    let (store, prints) = (HouseholdStore::default(), Blueprints::default());
    let (state, clock, benched) = (
        FactoryState::default(),
        FactoryClock::default(),
        Benched::default(),
    );
    let floor = Floor {
        registries: &registries,
        pack,
        store: world.get_resource::<HouseholdStore>().unwrap_or(&store),
        purse: world.get_resource::<Purse>().copied().unwrap_or_default(),
        prints: world.get_resource::<Blueprints>().unwrap_or(&prints),
        state: world.get_resource::<FactoryState>().unwrap_or(&state),
        clock: world.get_resource::<FactoryClock>().unwrap_or(&clock),
        benched: world.get_resource::<Benched>().unwrap_or(&benched),
        at_fabricator,
        sheet: sheet.unwrap_or(&plain),
    };
    Some(read(&floor, world))
}

/// The view for the world as it is.
fn view_of(world: &mut World) -> Option<FactoryView> {
    with_floor(world, |floor, world| {
        FactoryView::of(floor, world.resource::<FactoryUi>())
    })
}

/// What the screen's state settles to before it is shown: the blueprint
/// resolved, the batch held inside what the sim allows, and the slots
/// filled by the sim's own choice while nothing was picked by hand.
struct Settled {
    blueprint: Option<u32>,
    batch: u32,
    feeds: Feeds,
    worker: Option<String>,
}

/// [`Settled`] for the world as it is.
fn settle(floor: &Floor<'_>, ui: &FactoryUi) -> Settled {
    let registries = floor.registries;
    let blueprint = shown_of(floor, ui);
    let print = blueprint.and_then(|id| floor.prints.0.get(&id));
    let worker = ui
        .worker
        .clone()
        .filter(|name| floor.benched.0.contains(name));
    let Some(print) = print.filter(|_| floor.state.job.is_none()) else {
        return Settled {
            blueprint,
            batch: ui.batch,
            feeds: ui.feeds.clone(),
            worker,
        };
    };
    let fresh = ui.reopen || blueprint != ui.blueprint;
    let most = factory::limit(registries, floor.pack, floor.reach(), print).most;
    let wanted = if fresh { OPENING_BATCH } else { ui.batch };
    let batch = wanted.clamp(1, most.max(1));
    let feeds = if fresh || ui.auto {
        factory::cheapest(registries, floor.pack, floor.reach(), print, batch)
    } else {
        // What was picked by hand, less what the stock no longer holds or
        // no longer meets, in the sim's own order.
        print
            .slots
            .iter()
            .filter_map(|need| {
                let picked = ui.feeds.get(&need.slot.id)?;
                let kept: Vec<Record> =
                    factory::feeds(registries, floor.pack, floor.reach(), print, &need.slot.id)
                        .into_iter()
                        .filter(|feed| feed.verdict.is_ok() && picked.contains(&feed.kind))
                        .map(|feed| feed.kind)
                        .collect();
                (!kept.is_empty()).then(|| (need.slot.id.clone(), kept))
            })
            .collect()
    };
    Settled {
        blueprint,
        batch,
        feeds,
        worker,
    }
}

/// Builds the view, keeps it as what the screen shows, and gives its
/// definition.
fn build(world: &mut World) -> Option<ScreenDef> {
    let settled = with_floor(world, |floor, world| {
        settle(floor, world.resource::<FactoryUi>())
    })?;
    {
        let mut ui = world.resource_mut::<FactoryUi>();
        if ui.reopen || ui.blueprint != settled.blueprint {
            ui.auto = true;
        }
        ui.blueprint = settled.blueprint;
        ui.batch = settled.batch;
        ui.feeds = settled.feeds;
        ui.worker = settled.worker;
        ui.reopen = false;
    }
    let view = view_of(world)?;
    let def = screen_def(&view);
    let mut ui = world.resource_mut::<FactoryUi>();
    ui.dirty = false;
    ui.view = Some(view);
    Some(def)
}

/// Opens the factory, as a fabricator's F does: on the run when one is on,
/// else on the blueprint last shown, or the first the stock holds, with
/// the cheapest stock that meets it in its slots.
pub fn open(commands: &mut Commands) {
    commands.queue(|world: &mut World| {
        {
            let mut ui = world.resource_mut::<FactoryUi>();
            ui.reopen = true;
            ui.auto = true;
            ui.last = None;
            ui.notice = None;
            ui.asked = 0;
        }
        let Some(def) = build(world) else {
            tracing::warn!("no local player with a pack: nothing to run a blueprint from");
            return;
        };
        if on_top(world.resource::<ScreenStack>()) {
            replace_screen(&mut world.commands(), Arc::new(def), None);
        } else {
            push_screen(&mut world.commands(), Arc::new(def), None);
        }
    });
}

/// Puts what `request` asks in the sim's inbox, unless a request is still
/// waiting for its answer: Accept pressed twice before the tick is one
/// run.
fn ask(inbox: &mut FactoryInbox, asked: &mut usize, request: Option<FactoryRequest>) -> bool {
    match request {
        Some(request) if *asked == 0 => {
            inbox.0.push(request);
            *asked += 1;
            true
        }
        _ => false,
    }
}

/// `Update`: a blueprint, a batch step, a kind of stack, Cheapest stock, a
/// worker, Run and Stop. Nothing is decided here: what is picked is the
/// screen's own, and the three that change anything send the request the
/// view worked out for the sim to judge.
pub fn answer(
    mut choices: MessageReader<MenuChoice>,
    stack: Res<ScreenStack>,
    mut ui: ResMut<FactoryUi>,
    mut inbox: ResMut<FactoryInbox>,
) {
    if !on_top(&stack) {
        choices.clear();
        return;
    }
    for choice in choices.read() {
        if choice.screen != kind() {
            continue;
        }
        let ui = &mut *ui;
        let id = choice.id.as_str();
        let Some(view) = ui.view.as_ref() else {
            continue;
        };
        let idle = match &view.stage {
            Some(Stage::Idle(idle)) => Some(idle),
            _ => None,
        };
        let running = matches!(&view.stage, Some(Stage::Run(_)));
        // Where the focus goes once the press has been built: the control
        // itself, unless the press leads on.
        let mut next = id.to_owned();
        if let Some(print) = id.strip_prefix(PRINT_TAG) {
            let Some(row) = view
                .prints
                .iter()
                .find(|row| row.id.to_string() == print && row.busy.is_none())
            else {
                continue;
            };
            if !running {
                ui.blueprint = Some(row.id);
                ui.reopen = true;
                RUN.clone_into(&mut next);
            }
        } else if let Some(step) = id.strip_prefix(BATCH_TAG) {
            let Some(to) = idle
                .and_then(|idle| idle.batch.steps.iter().find(|own| own.id == step))
                .and_then(|step| step.to)
            else {
                continue;
            };
            ui.batch = to;
        } else if let Some(feed) = id.strip_prefix(FEED_TAG) {
            let Some((slot, row)) = view.feed_row(feed).filter(|(_, row)| row.refused.is_none())
            else {
                continue;
            };
            // A toggle: its kind goes in or out of what feeds the slot.
            let picked = ui.feeds.entry(slot.to_owned()).or_default();
            if let Some(at) = picked.iter().position(|kind| *kind == row.kind) {
                picked.remove(at);
            } else {
                picked.push(row.kind.clone());
            }
            ui.auto = false;
        } else if id == CHEAPEST {
            if idle.is_none() {
                continue;
            }
            ui.auto = true;
            RUN.clone_into(&mut next);
        } else if let Some(at) = id
            .strip_prefix(WORKER_TAG)
            .and_then(|at| at.parse::<usize>().ok())
        {
            let workers = match &view.stage {
                Some(Stage::Idle(idle)) => &idle.workers,
                Some(Stage::Run(run)) => &run.workers,
                None => continue,
            };
            let Some(chip) = workers.get(at) else {
                continue;
            };
            if running {
                // The run is the sim's: its worker is changed by asking.
                let request = (!chip.on).then(|| FactoryRequest::Assign {
                    worker: chip.name.clone(),
                });
                ask(&mut inbox, &mut ui.asked, request);
            } else {
                ui.worker.clone_from(&chip.name);
            }
        } else if id == RUN {
            let request = idle.and_then(|idle| idle.request.clone());
            ask(&mut inbox, &mut ui.asked, request);
        } else if id == STOP {
            let request = match &view.stage {
                Some(Stage::Run(run)) if run.can_stop => Some(FactoryRequest::Stop),
                _ => None,
            };
            ask(&mut inbox, &mut ui.asked, request);
        } else {
            continue;
        }
        ui.notice = None;
        ui.last = Some(next);
        ui.dirty = true;
    }
}

/// `Update`: the sim's answer, worded in the footer and, for a refusal,
/// over the button that asked.
pub fn hear(
    mut answers: MessageReader<FactoryAnswer>,
    registries: Option<Res<Registries>>,
    prints: Option<Res<Blueprints>>,
    mut ui: ResMut<FactoryUi>,
) {
    for answer in answers.read() {
        let ui = &mut *ui;
        ui.asked = ui.asked.saturating_sub(1);
        ui.dirty = true;
        let notice = match &answer.outcome {
            Ok(FactoryDone::Started { batch, seconds }) => {
                // Back to the blueprint's row: Stop now stands where Run
                // did, and a second Accept must not press it.
                ui.last = None;
                (
                    fill(
                        &fill(
                            words("strata.factory.status.started"),
                            "batch",
                            &batch.to_string(),
                        ),
                        "time",
                        &time_words(*seconds),
                    ),
                    false,
                )
            }
            Ok(FactoryDone::Assigned) => {
                let worker = match &answer.request {
                    FactoryRequest::Assign { worker } => worker.clone(),
                    _ => None,
                };
                (
                    worker.map_or_else(
                        || words("strata.factory.status.unassigned").to_owned(),
                        |name| fill(words("strata.factory.status.assigned"), "name", &name),
                    ),
                    false,
                )
            }
            Ok(FactoryDone::Stopped { made }) => {
                ui.last = None;
                ui.auto = true;
                (
                    fill(
                        words("strata.factory.status.stopped"),
                        "made",
                        &made.to_string(),
                    ),
                    false,
                )
            }
            Err(refusal) => {
                let print = match &answer.request {
                    FactoryRequest::Start { blueprint, .. } => {
                        prints.as_ref().and_then(|prints| prints.0.get(blueprint))
                    }
                    _ => None,
                };
                let said = registries.as_ref().map_or_else(
                    || refusal.to_string(),
                    |registries| refusal_words(&registries.0, refusal, print),
                );
                (said, true)
            }
        };
        tracing::info!("factory: {}", notice.0);
        ui.notice = Some(notice);
    }
}

/// `Update`, always: what a run did on its own, toasted wherever the
/// player is. A batch finished says how many of what are in the store and
/// under which batch; a run that stands says the store is full.
pub fn toast_notices(
    mut notices: MessageReader<FactoryNotice>,
    registries: Option<Res<Registries>>,
    mut commands: Commands,
) {
    for notice in notices.read() {
        let Some(registries) = registries.as_ref() else {
            continue;
        };
        super::progress::notice(&mut commands, notice_words(&registries.0, notice));
    }
}

/// Writes the time left to its node when it is all that differs between
/// what the screen shows and `next`, and keeps `next` as what it shows.
/// Whether it did: when not, the screen is rebuilt. The time moves every
/// second, and a rebuild a second would take the focus and the pointer's
/// hover with it.
fn write_left(world: &mut World, next: &FactoryView) -> bool {
    let ui = world.resource::<FactoryUi>();
    let (Some(shown), Some(Stage::Run(now))) = (ui.view.as_ref(), next.stage.as_ref()) else {
        return false;
    };
    let Some(Stage::Run(was)) = shown.stage.as_ref() else {
        return false;
    };
    if was.paused.is_some() || now.paused.is_some() {
        return false;
    }
    let mut same = next.clone();
    if let Some(Stage::Run(run)) = same.stage.as_mut() {
        run.left.clone_from(&was.left);
    }
    if *shown != same {
        return false;
    }
    let left = now.left.clone();
    let mut texts = world.query::<(&TestId, &mut LocText, &mut Text)>();
    for (id, mut loc, mut text) in texts.iter_mut(world) {
        if id.0 != LEFT_ID {
            continue;
        }
        if loc.key.0 != left {
            loc.key = LocKey(left.clone());
        }
        if text.0 != left {
            text.0.clone_from(&left);
        }
    }
    world.resource_mut::<FactoryUi>().view = Some(next.clone());
    true
}

/// `Update`: rebuilds the open screen when the view has changed under it
/// (a unit made, the stock moved, the run finished, the character stepped
/// away from the fabricator) or a choice marked it. A screen pushed this
/// frame is left alone unless it was marked, for the reason the fitting
/// screen's is.
pub fn rebuild(world: &mut World, mut was_on_top: Local<bool>) {
    let settled = *was_on_top;
    *was_on_top = on_top(world.resource::<ScreenStack>());
    if !*was_on_top {
        return;
    }
    if !world.resource::<FactoryUi>().dirty {
        if !settled {
            return;
        }
        let Some(view) = view_of(world) else {
            return;
        };
        if world.resource::<FactoryUi>().view.as_ref() == Some(&view) {
            return;
        }
        if write_left(world, &view) {
            return;
        }
    }
    if let Some(def) = build(world) {
        replace_screen(&mut world.commands(), Arc::new(def), None);
    }
}

/// Registers the state and the systems. Only called once the UI is wired.
pub(crate) fn register(app: &mut bevy::app::App) {
    app.init_resource::<FactoryUi>().add_systems(
        bevy::app::Update,
        (
            (answer, hear, rebuild).chain().in_set(super::ScreenWork),
            toast_notices,
        ),
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::testing::shipped;

    #[test]
    fn a_runs_time_reads_in_seconds_then_whole_minutes() {
        assert_eq!(time_words(36), "36 s");
        assert_eq!(time_words(59), "59 s");
        assert_eq!(time_words(60), "1 min");
        assert_eq!(
            time_words(61),
            "2 min",
            "a part of a minute is counted whole"
        );
        assert_eq!(time_words(240), "4 min");
    }

    #[test]
    fn a_rate_is_said_in_words() {
        assert_eq!(speed_words(1000), "full speed");
        assert_eq!(speed_words(500), "half speed");
        assert_eq!(speed_words(250), "25% speed", "another dial is a share");
        assert_eq!(runs_words(1), "1 run left");
        assert_eq!(runs_words(97), "97 runs left");
        assert_eq!(counted("cell core", 1), "cell core");
        assert_eq!(counted("cell core", 20), "cell cores");
        assert_eq!(counted("harness", 2), "harnesses");
    }

    /// Every refusal of the sim's has words of its own, and one that names
    /// a figure says the figure and what is asked.
    #[test]
    fn a_refusal_has_words() {
        let registries = shipped();
        let said = |refusal: FactoryRefusal| refusal_words(&registries, &refusal, None);
        assert_eq!(
            said(FactoryRefusal::NoFabricator),
            "Step up to a fabricator"
        );
        assert_eq!(
            said(FactoryRefusal::OverRuns { left: 3 }),
            "The blueprint has 3 runs left"
        );
        assert_eq!(
            said(FactoryRefusal::NoRoom),
            "No room in the store for what the run holds"
        );
        assert_eq!(
            said(FactoryRefusal::Slot {
                slot: "salt".to_owned(),
                why: FeedRefusal::Short {
                    have: 60,
                    needs: 400
                },
            }),
            "salt: 60 in stock, the batch takes 400",
            "with no blueprint to name it, the slot is its key"
        );
        for refusal in [
            FactoryRefusal::NoPlayer,
            FactoryRefusal::Busy,
            FactoryRefusal::NoBlueprint,
            FactoryRefusal::Stale,
            FactoryRefusal::NoBatch,
            FactoryRefusal::NoJob,
            FactoryRefusal::NotOnBench,
        ] {
            assert!(!said(refusal.clone()).starts_with("strata."), "{refusal:?}");
        }
        let slot = &registries
            .schematics
            .iter()
            .find(|schematic| schematic.def.id.path() == "schematic.cell_core")
            .expect("the cell core's schematic")
            .def
            .slots[0];
        let under = FeedRefusal::Under {
            stat: "potency".to_owned(),
            has: 410,
            needs: 700,
        };
        assert_eq!(
            feed_refusal_words(&registries, slot, &under),
            "Potency 410, needs 700"
        );
        let over = FeedRefusal::Over {
            stat: "density".to_owned(),
            has: 420,
            most: 290,
        };
        assert_eq!(
            feed_refusal_words(&registries, slot, &over),
            "Density 420, at most 290"
        );
    }
}
