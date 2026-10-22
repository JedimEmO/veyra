//! The crafting session: the workbench's second tab, where a schematic's
//! slots are filled from the stock, assembled, worked up a point at a time
//! and finished (ticket 0007, step 4; `docs/design/crafting.md`, 5.1 to 5.3
//! and 5.5).
//!
//! One screen in three states, never a wizard of pages. With nothing on the
//! bench it is the schematics at the left, the slots of the one shown in
//! the middle, each with what the pack and the store have for it, and at
//! the right what would come out: every line's floor and ceiling on a bar
//! and what they are worth in the game's units, before anything is spent.
//! After Assemble the right column is the bench hand: the points left and,
//! a line each, the figure of its ceiling with Steady and Bold and their
//! odds drawn as the five bars of the roll table. After Finish it is the
//! card of what was made, which offers Record as blueprint: a box that
//! opens in place with the derating as a slider and, as it moves, the lines
//! the blueprint would make and the minimum each slot would ask (ticket
//! 0007, step 5; `docs/design/crafting.md`, 5.4). A thing made earlier and
//! still in the stock is listed under the schematics and recorded the same
//! way.
//!
//! No rule lives here and nothing is multiplied. Every figure and every
//! order is the sim's (`veyra_sim::crafting`): `candidates` for a slot,
//! best first, `best_stock` to fill them all in a press, `check_picks` for
//! why Assemble is grey, `preview` and `now` for the lines and what they
//! mean, `hand_odds` for a hand and `can_roll` for whether it is grey,
//! `lifts_odds` for whether the stock moves them, `can_buy` for a bought
//! part, and `veyra_sim::blueprint`'s `draft` for everything the Record
//! box shows and `prototypes` for what can be recorded.
//! [`CraftingView::of`] gathers their answers into display words and
//! [`screen_def`] lays those out, so what the screen says can be checked
//! without one. Assemble, Roll, Finish and Buy put a `CraftRequest` in the
//! sim's inbox for the tick to judge, one at a time, and the sim's
//! `CraftAnswer` is worded at the button and in the footer. The screen
//! shows the odds before a press and never an outcome before the sim has
//! answered.
//!
//! The session is the sim's and the save's, so a screen closed with
//! something on the bench opens on it again. What is picked, which slot's
//! candidates are listed and the card of the thing just finished are the
//! screen's own. The definition is a function of the view and is swapped
//! whole when the view changes (the framework's rebuild ([`strata_ui::sim_screen`])); the one thing written in
//! place is a bar's fill as a roll lands ([`animate_roll`]) and the Record
//! box's words as its slider moves, so a drag is not cut by a rebuild.
//! Local-only: nothing here is replicated.
//!
//! The strip of two tabs both workbench screens start with is here too
//! ([`bench_tabs`]), with what switches between them ([`switch_tabs`]) and
//! what a workbench's F opens ([`open_bench`]).

use std::sync::Arc;

use bevy::ecs::name::Name;
use bevy::ecs::prelude::*;
use bevy::ecs::system::{SystemParam, SystemParamItem};
use bevy::input::ButtonInput;
use bevy::input::gamepad::Gamepad;
use bevy::input::keyboard::KeyCode;
use bevy::input_focus::InputFocus;
use bevy::picking::hover::Hovered;
use bevy::picking::pointer::PointerPress;
use bevy::time::Time;
use bevy::ui::{BackgroundColor, BorderColor, BorderRadius, Node, UiRect, UiTransform, Val};
use slotted::menu::MenuChoice;
use slotted::model::Value as Model;
use slotted::registry::Value;
use slotted::theme::{ActiveTheme, Paint, Role, Theme};
use slotted::ui::def::{
    Align, BindDef, ButtonOpts, ButtonVariant, Justify, Layout, LayoutDirection, Length, LocKey,
    Overflow, Padding, Presentation, PresentationMode, ScreenDef, ScreenKind, Tags, TextAlign,
    TextOpts, TextRole, Transition, UiNodeDef, WidgetKind,
};
use slotted::ui::screen::{SpawnCtx, Widget, WidgetRegistry};
use slotted::ui::{
    ButtonState, GlyphSet, InputMode, ScreenStack, SlottedUiSet, TestId, UiAction, UiActionClaims,
    UiActionEmit, UiActionEvent, UiBindings, ValueChanged, ValueStore, key_glyph_text, push_screen,
    replace_screen, resolved_glyph_set_for_vendor,
};
use strata_registry::item::{IconShape, ItemKind, Property, QUALITY_MAX, Record};
use strata_registry::progression::SkillGraph;
use strata_registry::schematic::{FactoryRules, Hand, Outcome, Schematic, SlotDef, Stat, Takes};
use strata_registry::{Attribute, GameRegistries, Namespaced};
use veyra_sim::blueprint;
use veyra_sim::combat::Combatant;
use veyra_sim::crafting::{
    self, Candidate, CraftAnswer, CraftDone, CraftInbox, CraftRefusal, CraftRequest, CraftState,
    Holder, LinePreview, Means, Picks, Session, SlotRefusal, WeaponMeans,
};
use veyra_sim::deposits::Purse;
use veyra_sim::fitting::{self as fit_rules, Amount, Line, Unit, WorkbenchAccess};
use veyra_sim::items::{self, Hands, HouseholdStore, Pack};
use veyra_sim::progression::{CharacterSheet, Reach};
use veyra_sim::{LocalPlayer, Registries};

use super::card::{self, Against, BarFill, BarParams, CardParams};
use super::fitting;
use super::strings::{credits, english, grouped};
use super::weapon_diagram::FOCUS_MARK;
use strata_ui::sim_screen::{self, ChangeSource, ScreenStep, Seen, SimScreen, SimScreenApp, moved};

/// The screen kind.
pub const KIND: &str = "strata:crafting";
/// The widget kind of the small shape a stack is drawn by in a list.
pub const SWATCH_WIDGET: &str = "strata:stock_swatch";
/// The widget kind of the five bars a hand's odds are drawn as.
pub const ODDS_WIDGET: &str = "strata:roll_odds";

/// The `test_id` of the panel the whole screen is.
pub const ROOT_ID: &str = "crafting.root";
/// The `test_id` of the screen's title.
pub const TITLE_ID: &str = "crafting.title";
/// The `test_id` of the line that says who is at the bench.
pub const WORKER_ID: &str = "crafting.worker";
/// The `test_id` of the household's credits.
pub const PURSE_ID: &str = "crafting.purse";
/// The `test_id` of the Close button.
pub const CLOSE_ID: &str = "crafting.close";
/// The `test_id` of the footer's line.
pub const STATUS_ID: &str = "crafting.status";
/// The `test_id` of the line under it that keeps the keys in sight while
/// the footer's line is the sim's last word.
pub const KEYS_ID: &str = "crafting.keys";
/// The schematics list's scroll.
pub const LIST_ID: &str = "crafting.list";
/// The `menu` tag and `test_id` prefix of a tab of the workbench:
/// `workbench.tab:crafting`.
pub const TAB_TAG: &str = "workbench.tab:";
/// The `menu` tag and `test_id` prefix of a schematic's row, by its id's
/// path: `crafting.schem:schematic.cell_core`.
pub const SCHEM_TAG: &str = "crafting.schem:";
/// The `menu` tag and `test_id` prefix of a chip over the list:
/// `crafting.filter:known`.
pub const FILTER_TAG: &str = "crafting.filter:";
/// The `test_id` prefix of the mark on a schematic's row.
pub const SCHEM_MARK_TAG: &str = "crafting.schem.mark:";
/// The `test_id` prefix of a slot's box: `crafting.slot:salt`.
pub const SLOT_TAG: &str = "crafting.slot:";
/// The `menu` tag and `test_id` prefix of the button that shows what is
/// picked for a slot and lists its candidates: `crafting.open:salt`.
pub const OPEN_TAG: &str = "crafting.open:";
/// The `menu` tag and `test_id` prefix of a candidate's row:
/// `crafting.pick:salt:0`.
pub const PICK_TAG: &str = "crafting.pick:";
/// The `test_id` prefix of the sentence in a slot nothing will do for.
pub const NONE_TAG: &str = "crafting.none:";
/// The `menu` tag and `test_id` prefix of a slot's Buy: `crafting.buy:seals`.
pub const BUY_TAG: &str = "crafting.buy:";
/// The `test_id` prefix of the line beside a Buy: why it is grey.
pub const BUY_WHY_TAG: &str = "crafting.buy.why:";
/// The `test_id` prefix of a line of the product: `crafting.line:capacity`.
pub const LINE_TAG: &str = "crafting.line:";
/// The `test_id` prefix of a line's bar.
pub const BAR_TAG: &str = "crafting.bar:";
/// The `test_id` prefix of what a line is worth in the game's units.
pub const MEANS_TAG: &str = "crafting.means:";
/// The `test_id` prefix of the sentence that gives a line's floor and
/// ceiling, or says that its slots are not filled.
pub const RANGE_TAG: &str = "crafting.range:";
/// The `test_id` prefix of what a component's line gives downstream.
pub const DOWNSTREAM_TAG: &str = "crafting.downstream:";
/// The `test_id` of the sentence that says what Assemble would use.
pub const USES_ID: &str = "crafting.uses";
/// The sentence that says the picked stock moves the odds a row upward.
pub const WORKABLE_ID: &str = "crafting.workable";
/// The `test_id` and `menu` tag of Best stock.
pub const BEST: &str = "crafting.best";
/// The `test_id` and `menu` tag of Assemble.
pub const ASSEMBLE: &str = "crafting.assemble";
/// The `test_id` of the line by the buttons: why one is grey.
pub const WHY_ID: &str = "crafting.why";
/// The `test_id` of the heading over what went in.
pub const WENT_ID: &str = "crafting.went";
/// The `test_id` of the points left.
pub const POINTS_ID: &str = "crafting.points";
/// The `test_id` prefix of the word a line's last roll said.
pub const WORD_TAG: &str = "crafting.word:";
/// The `test_id` prefix of `338 of 563` under a line's bar.
pub const STANDS_TAG: &str = "crafting.stands:";
/// The `menu` tag and `test_id` prefix of a hand on a line:
/// `crafting.roll:capacity:steady`.
pub const ROLL_TAG: &str = "crafting.roll:";
/// The `test_id` prefix of a hand's odds, `crafting.odds:capacity:steady`,
/// and of its five bars, `crafting.odds:capacity:steady:0` to `:4`.
pub const ODDS_TAG: &str = "crafting.odds:";
/// The `test_id` prefix of what a hand offers: `most often +56`.
pub const OFFER_TAG: &str = "crafting.offer:";
/// The `test_id` and `menu` tag of Finish.
pub const FINISH: &str = "crafting.finish";
/// The `test_id` of the finished thing's card.
pub const CARD_ID: &str = "crafting.card";
/// The `test_id` and `menu` tag of Make another.
pub const AGAIN: &str = "crafting.again";

/// The button that opens the Record box on the finished card.
pub const RECORD_OPEN: &str = "crafting.record.open";
/// The Record box.
pub const RECORD_ID: &str = "crafting.record";
/// What the blueprint would be recorded at: `85% of this one`.
pub const RECORD_AT_ID: &str = "crafting.record.at";
/// The derating's slider.
pub const DERATE_ID: &str = "crafting.record.derate";
/// The lines the blueprint would make.
pub const RECORD_MAKES_ID: &str = "crafting.record.makes";
/// Prefix of what a slot would ask; the slot's key follows.
pub const RECORD_NEED_TAG: &str = "crafting.record.need:";
/// The runs the blueprint would start with.
pub const RECORD_RUNS_ID: &str = "crafting.record.runs";
/// The button that records.
pub const RECORD_GO: &str = "crafting.record.go";
/// Why nothing is recorded, over the button that would open the box.
pub const RECORD_WHY_ID: &str = "crafting.record.why";
/// Prefix of a thing made earlier; its batch number follows.
pub const PROTO_TAG: &str = "crafting.proto:";
/// The `ValueStore` key the derating's slider is bound to: a share of the
/// prototype, nought to one.
pub const DERATE_KEY: &str = "strata.crafting.derate";

/// How long a roll's fill takes to land, in seconds: a third of one. A
/// look and no rule, so it is the client's.
pub const ROLL_FILL_SECS: f32 = 1. / 3.;

/// The panel's width, the schematics' column and the outcome's, in UI
/// units; the slots take the rest.
pub(super) const WIDTH: f32 = 1240.;
pub(super) const SCHEMATICS_WIDTH: f32 = 290.;
/// The least the panel's height is, in percent of the window's: the most it
/// takes, so the list that scrolls alone (it adds no height of its own) leaves
/// the frame one size in every state.
pub(super) const MIN_HEIGHT_VH: f32 = 94.;
const OUTCOME_WIDTH: f32 = 356.;
/// The well a stack's shape stands in, and the shape in it.
const ICON_WELL: f32 = 34.;
const ICON: f32 = 16.;
/// A hand's button and the box of its odds, so the two hands of a line and
/// every line's hands stand in columns.
const HAND_WIDTH: f32 = 84.;
const ODDS_WIDTH: f32 = 56.;
/// The box the five bars stand in, a bar's width, and how tall a percent
/// of chance is drawn: a certainty would be forty units.
const ODDS_HEIGHT: f32 = 24.;
const ODDS_BAR: f32 = 9.;
const ODDS_PER_PERCENT: f32 = 0.4;

pub(super) const HEAD_ROLE: &str = "strata.fitting.head";
pub(super) const BOX_ROLE: &str = "strata.fitting.box";
pub(super) const PICK_ROLE: &str = "strata.fitting.pick";
pub(super) const PICK_ON_ROLE: &str = "strata.fitting.pick.on";
pub(super) const PICK_ICON_ROLE: &str = "strata.fitting.pick.icon";
pub(super) const NAME_ROLE: &str = "strata.fitting.mount.name";
pub(super) const SUB_ROLE: &str = "strata.fitting.mount.sub";
pub(super) const STATUS_ROLE: &str = "strata.fitting.status";
pub(super) const REFUSED_ROLE: &str = "strata.fitting.status.refused";
pub(super) const WHY_ROLE: &str = "strata.fitting.why";
pub(super) const WHY_HINT_ROLE: &str = "strata.fitting.why.hint";
/// The family a button paints from while it cannot act, as on the fitting
/// screen: fainter than the theme's own grey.
pub(super) const ACT_ROLE: &str = "strata.fitting.act";
pub(super) const FIGURE_ROLE: &str = "strata.fitting.figure";
const TAB_ROLE: &str = "strata.fitting.tab";
const TAB_ON_ROLE: &str = "strata.fitting.tab.on";
const TAB_TEXT_ROLE: &str = "strata.fitting.tab.text";
const TAB_TEXT_ON_ROLE: &str = "strata.fitting.tab.text.on";
pub(super) const MARK_ROLE: &str = "strata.register.mark";
pub(super) const MARK_NONE_ROLE: &str = "strata.register.mark.none";
pub(super) const SCHEM_ROLE: &str = "strata.crafting.schem";
pub(super) const SCHEM_ON_ROLE: &str = "strata.crafting.schem.on";
pub(super) const SCHEM_NAME_ROLE: &str = "strata.crafting.schem.name";
pub(super) const SCHEM_NAME_CANT_ROLE: &str = "strata.crafting.schem.name.cant";
pub(super) const SLOT_ROLE: &str = "strata.crafting.slot";
const SLOT_OPEN_ROLE: &str = "strata.crafting.slot.open";
const PICK_SHORT_ROLE: &str = "strata.crafting.pick.short";
pub(super) const STAT_ROLE: &str = "strata.crafting.stat";
pub(super) const STAT_VALUE_ROLE: &str = "strata.crafting.stat.value";
pub(super) const TAG_ROLE: &str = "strata.crafting.tag";
pub(super) const MEANS_ROLE: &str = "strata.crafting.means";
pub(super) const NOTE_ROLE: &str = "strata.crafting.note";
/// The hairline round a stack's shape, whose fill is the item's own colour.
const SWATCH_ROLE: &str = "strata.crafting.swatch";
const WORD_ROLES: [&str; 5] = [
    "strata.crafting.word.good",
    "strata.crafting.word.good",
    "strata.crafting.word.fair",
    "strata.crafting.word.poor",
    "strata.crafting.word.setback",
];
const ODDS_ROLES: [&str; 5] = [
    "strata.crafting.odds.good",
    "strata.crafting.odds.good",
    "strata.crafting.odds.fair",
    "strata.crafting.odds.poor",
    "strata.crafting.odds.setback",
];

/// Every role the screen paints with, for the test that holds them to the
/// theme.
pub const ROLES: [&str; 37] = [
    HEAD_ROLE,
    BOX_ROLE,
    PICK_ROLE,
    PICK_ON_ROLE,
    PICK_ICON_ROLE,
    NAME_ROLE,
    SUB_ROLE,
    STATUS_ROLE,
    REFUSED_ROLE,
    WHY_ROLE,
    WHY_HINT_ROLE,
    ACT_ROLE,
    FIGURE_ROLE,
    TAB_ROLE,
    TAB_ON_ROLE,
    TAB_TEXT_ROLE,
    TAB_TEXT_ON_ROLE,
    MARK_ROLE,
    MARK_NONE_ROLE,
    SCHEM_ROLE,
    SCHEM_ON_ROLE,
    SCHEM_NAME_ROLE,
    SCHEM_NAME_CANT_ROLE,
    SLOT_ROLE,
    SLOT_OPEN_ROLE,
    PICK_SHORT_ROLE,
    STAT_ROLE,
    STAT_VALUE_ROLE,
    TAG_ROLE,
    MEANS_ROLE,
    NOTE_ROLE,
    SWATCH_ROLE,
    WORD_ROLES[1],
    WORD_ROLES[2],
    WORD_ROLES[3],
    WORD_ROLES[4],
    ODDS_ROLES[1],
];

/// The screen kind.
#[must_use]
pub fn kind() -> ScreenKind {
    ScreenKind::new(KIND)
}

/// The `test_id` of the row of the schematic `id`.
#[must_use]
pub fn schem_id(id: &Namespaced) -> String {
    format!("{SCHEM_TAG}{}", id.path())
}

/// The `test_id` of the button of the slot `slot`.
#[must_use]
pub fn open_id(slot: &str) -> String {
    format!("{OPEN_TAG}{slot}")
}

/// The `test_id` of the candidate at `row` of the slot `slot`.
#[must_use]
pub fn pick_id(slot: &str, row: usize) -> String {
    format!("{PICK_TAG}{slot}:{row}")
}

/// The `test_id` of the Buy of the slot `slot`.
#[must_use]
pub fn buy_id(slot: &str) -> String {
    format!("{BUY_TAG}{slot}")
}

/// The `test_id` of the thing of batch `batch` in the list of what was
/// made earlier.
#[must_use]
pub fn proto_id(batch: u32) -> String {
    format!("{PROTO_TAG}{batch}")
}

/// The `test_id` of what the slot `slot` would ask, in the Record box.
#[must_use]
pub fn record_need_id(slot: &str) -> String {
    format!("{RECORD_NEED_TAG}{slot}")
}

/// The `test_id` of `hand` on the line `line`.
#[must_use]
pub fn roll_id(line: &str, hand: Hand) -> String {
    format!("{ROLL_TAG}{line}:{}", hand.key())
}

/// The `test_id` of the bar at `bar` of `hand`'s odds on the line `line`.
#[must_use]
pub fn odds_id(line: &str, hand: Hand, bar: usize) -> String {
    format!("{ODDS_TAG}{line}:{}:{bar}", hand.key())
}

pub(super) fn words(key: &'static str) -> &'static str {
    english(key).unwrap_or(key)
}

pub(super) fn fill(words: &str, name: &str, with: &str) -> String {
    words.replace(&format!("{{{name}}}"), with)
}

/// A change in a figure with its sign: `+56`, and a true minus for a loss,
/// which is as wide as the plus it stands in a column with.
fn signed(change: i32) -> String {
    if change < 0 {
        format!("\u{2212}{}", change.unsigned_abs())
    } else {
        format!("+{change}")
    }
}

// ---- the two tabs of a workbench ----

/// A tab of the workbench's screen. As a resource it is the one last
/// shown, which a workbench's F opens on.
#[derive(Resource, Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum WorkbenchTab {
    /// The weapon in hand and its mounts ([`super::fitting`]).
    #[default]
    Fitting,
    /// The session.
    Crafting,
}

impl WorkbenchTab {
    /// Both tabs, in the strip's order.
    pub const ALL: [Self; 2] = [Self::Fitting, Self::Crafting];

    /// The tab's name in its tag.
    #[must_use]
    pub const fn id(self) -> &'static str {
        match self {
            Self::Fitting => "fitting",
            Self::Crafting => "crafting",
        }
    }

    const fn label(self) -> &'static str {
        match self {
            Self::Fitting => "strata.workbench.tab.fitting",
            Self::Crafting => "strata.workbench.tab.crafting",
        }
    }

    /// The `test_id` and `menu` tag of the tab.
    #[must_use]
    pub fn tab_id(self) -> String {
        format!("{TAB_TAG}{}", self.id())
    }

    const fn other(self) -> Self {
        match self {
            Self::Fitting => Self::Crafting,
            Self::Crafting => Self::Fitting,
        }
    }
}

/// The strip both workbench screens start with: a pill a tab, the one
/// shown lit and marked.
#[must_use]
pub fn bench_tabs(on: WorkbenchTab) -> UiNodeDef {
    let pill = |tab: WorkbenchTab| {
        let id = tab.tab_id();
        let lit = tab == on;
        UiNodeDef::Card {
            role: Role::new(if lit { TAB_ON_ROLE } else { TAB_ROLE }),
            layout: Layout {
                padding: Padding {
                    top: 0.5,
                    right: 2.2,
                    bottom: 0.5,
                    left: 2.2,
                },
                ..row(0.)
            },
            // The one shown says so by a mark as well as by its fill.
            children: vec![line_in(
                if lit {
                    format!("{FOCUS_MARK}{}", words(tab.label()))
                } else {
                    words(tab.label()).to_owned()
                },
                if lit { TAB_TEXT_ON_ROLE } else { TAB_TEXT_ROLE },
            )],
            disabled: false,
            tags: Tags::new().with("menu", &id).with(Tags::TEST_ID, &id),
        }
    };
    panel(
        "invisible",
        row(1.),
        WorkbenchTab::ALL.into_iter().map(pill).collect(),
    )
}

// ---- the state ----

/// A roll's fill on its way from the figure before to the figure after.
#[derive(Debug, Clone, PartialEq)]
struct RollFill {
    /// The line, by its id.
    line: String,
    /// The figure the fill starts at.
    from: u16,
    /// The figure it lands on.
    to: u16,
    /// Seconds since the sim answered.
    elapsed: f32,
    /// Whether the fill has landed.
    landed: bool,
}

/// What a run that opens the screen for a still wants it brought to, as a
/// player's presses would bring it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Still {
    /// The slots with nothing picked.
    Empty,
    /// The slots with the best stock in them, as the screen opens.
    Pick,
    /// After Assemble: the bench hand.
    Hand,
    /// After the points are spent and Finish: the finished card.
    Done,
    /// The finished card with Record as blueprint open at this derating,
    /// per-mille of the prototype.
    Record(u16),
}

/// What the schematics list shows: the stores' chips, reused. Known is the
/// default, since a list of everything the trades make buries what the
/// member at the bench can do.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ListFilter {
    /// What the member at the bench was taught.
    #[default]
    Known,
    /// What can be made now from what is in reach.
    Ready,
    /// Every schematic.
    All,
}

impl ListFilter {
    /// The chips, in the order they are drawn.
    pub const ALL: [Self; 3] = [Self::Known, Self::Ready, Self::All];

    /// The chip's `menu` tag and `test_id`.
    #[must_use]
    pub fn chip_id(self) -> String {
        format!("{FILTER_TAG}{}", self.key())
    }

    fn key(self) -> &'static str {
        match self {
            Self::Known => "known",
            Self::Ready => "ready",
            Self::All => "all",
        }
    }

    fn of_key(key: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|filter| filter.key() == key)
    }

    fn label(self) -> &'static str {
        match self {
            Self::Known => "strata.crafting.filter.known",
            Self::Ready => "strata.crafting.filter.ready",
            Self::All => "strata.crafting.filter.all",
        }
    }
}

/// The crafting screen's state: which schematic it shows, what is picked
/// for its slots, and what the sim last answered.
#[derive(Resource, Debug, Default)]
pub struct CraftingState {
    /// What the list shows, kept while the screen is open and after.
    pub filter: ListFilter,
    /// The schematic shown; none until the screen has resolved one. A
    /// session on the bench is shown whatever this says.
    pub schematic: Option<Namespaced>,
    /// What is picked for each of its slots.
    pub picks: Picks,
    /// The slot whose candidates are listed.
    open: Option<String>,
    /// The thing just finished, whose card the right column shows until
    /// another is begun.
    done: Option<Record>,
    /// Every slot is to be filled with the best stock at the next build.
    refill: bool,
    /// Every slot with nothing picked is to be, at the next build: a part
    /// just bought goes into the slot it was bought for.
    fill_empty: bool,
    /// The `test_id` of the control the keyboard's focus goes to at the
    /// next build: the one last used, or the one its press leads to.
    last: Option<String>,
    /// A rebuild is due whatever the view says.
    dirty: bool,
    /// The sim's last answer in words, and whether it was a refusal.
    notice: Option<(String, bool)>,
    /// What the open screen was built from.
    view: Option<CraftingView>,
    /// The requests sent that the sim has not answered yet. While there is
    /// one nothing more is asked: Accept pressed twice before the tick is
    /// one roll.
    asked: usize,
    /// The fill of the last roll.
    fill: Option<RollFill>,
    /// The hand that was pressed and has been held since, by its
    /// `test_id`: it rolls again as each roll lands.
    holding: Option<String>,
    /// The hand the pointer went down on and rolled at once, by its
    /// `test_id`, until the pointer is let go: the click that release makes
    /// is that same press, and rolls nothing more.
    pressed: Option<String>,
    /// What a still wants the screen brought to.
    still: Option<Still>,
    /// The Record box, while it is open on the finished card: what its
    /// slider stands at, per-mille of the prototype.
    recording: Option<u16>,
}

impl CraftingState {
    /// What the open screen was last built from.
    #[must_use]
    pub const fn view(&self) -> Option<&CraftingView> {
        self.view.as_ref()
    }

    /// The thing just finished, while its card is shown.
    #[must_use]
    pub const fn finished(&self) -> Option<&Record> {
        self.done.as_ref()
    }
}

// ---- the view ----

/// One schematic's row.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SchemRow {
    /// The schematic.
    pub id: Namespaced,
    /// What it makes: `Cell core`.
    pub name: String,
    /// For a module, the mount it is for: `FEED`.
    pub mount: String,
    /// Whether the stock has enough for every slot now.
    pub can: bool,
    /// Whether it is the one shown.
    pub on: bool,
    /// Why it cannot be picked: something else is on the bench.
    pub busy: Option<String>,
    /// The box that teaches it, in words, while the bench's member lacks
    /// it. The row stays pickable so its slots can be read.
    pub untaught: Option<String>,
}

/// A heading of the list and the schematics under it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SchemGroup {
    /// `WEAPONS`, `GUN PARTS`.
    pub head: String,
    /// The rows the list shows under it.
    pub count: usize,
    /// Its schematics, by id.
    pub rows: Vec<SchemRow>,
}

/// One figure a slot is weighed by, of one kind of stack.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StatWords {
    /// `Potency`, or a part's line: `Charge density`.
    pub name: String,
    /// The figure as the stack's card writes it.
    pub figure: String,
    /// Whether less of it is better, which the row then says.
    pub lighter: bool,
    /// Whether it moves the odds of the session and no line, which the row
    /// then says: a lot's Workability.
    pub odds: bool,
}

/// What a kind of stack is drawn by in a list: the image the one rule picks
/// for its item, the same a cell of the pack draws, and the item's
/// stand-in shape only where the rule has none.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Swatch {
    /// The stand-in shape, the rule's last arm.
    pub shape: IconShape,
    /// The item's colour.
    pub tint: [u8; 3],
    /// The image's asset path, with its source's prefix.
    pub image: Option<String>,
    /// Whether the image takes `tint`; a painted square does not.
    pub tinted: bool,
}

/// One kind of stack the stock holds for a slot.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CandidateRow {
    /// The kind, as a pick names it.
    pub kind: Record,
    /// `Tolley salt`, `Cell core`.
    pub name: String,
    /// `batch 1` for a made part; empty for anything else.
    pub batch: String,
    /// Its shape; none for a kind the catalogue does not know.
    pub swatch: Option<Swatch>,
    /// The figures the slot weighs, in the order its lines name them, then
    /// its Workability when its lot carries it.
    pub stats: Vec<StatWords>,
    /// `12 units, needs 20` for one there is too little of.
    pub short: Option<String>,
    /// How much the stock holds: `160 u`, `3`.
    pub held: String,
    /// Whether it is the best the stock has, which the first row is.
    pub best: bool,
    /// Whether it is the one picked.
    pub on: bool,
}

/// A slot's Buy.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BuyView {
    /// What is bought.
    pub item: Namespaced,
    /// `Buy 1 · 80 credits`.
    pub label: String,
    /// What the button asks the sim for; none when it is grey.
    pub request: Option<CraftRequest>,
    /// Why it is grey; empty when it is not.
    pub why: String,
}

/// One slot of the schematic shown.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SlotView {
    /// The slot's key.
    pub id: String,
    /// `REACTIVE`.
    pub label: String,
    /// `any reactive · 20 units`, `one cell core`.
    pub wants: String,
    /// What is picked for it.
    pub picked: Option<CandidateRow>,
    /// `3 fit`, or `Close` while they are listed; empty with none, and
    /// with one that is already the one picked.
    pub count: String,
    /// Whether its candidates are listed.
    pub open: bool,
    /// Its candidates, best first.
    pub rows: Vec<CandidateRow>,
    /// The sentence shown when nothing the stock has will do.
    pub none: Option<String>,
    /// For a slot that takes something bought, with nothing that will do.
    pub buy: Option<BuyView>,
}

/// One line of what would come out.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OutLine {
    /// The line's key.
    pub id: String,
    /// `Capacity`.
    pub name: String,
    /// Whether the fight has no number yet for what it moves.
    pub later: bool,
    /// The floor assembly gives and the ceiling the materials allow; none
    /// while a slot the line weighs is empty.
    pub bar: Option<(u16, u16)>,
    /// `28 to 29 rounds`: both in the game's units. Empty for a part.
    pub means: String,
    /// `Assembles at 338, can be worked up to 563.`, or that the slots are
    /// not filled.
    pub range: String,
    /// For a part: `In a patrol extended magazine: 28 to 29 rounds`, or
    /// that it goes into nothing yet. Empty for a module.
    pub downstream: String,
}

/// The screen with nothing on the bench.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PickStage {
    /// The slots, in the schematic's order.
    pub slots: Vec<SlotView>,
    /// The product's lines.
    pub lines: Vec<OutLine>,
    /// `Uses 20 units of Tolley salt, 30 units of Oxbow brine.`
    pub uses: String,
    /// Whether what is picked would move the odds a row upward, which is
    /// said before the stock is spent.
    pub workable: bool,
    /// What Assemble asks the sim for; none when it is grey.
    pub request: Option<CraftRequest>,
    /// Why it is grey; empty when it is not.
    pub why: String,
}

/// One hand on one line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HandOffer {
    /// The chance of each result, best first: the five bars.
    pub odds: [u8; 5],
    /// `most often +56`, `up to +90, can slip 23`.
    pub words: String,
    /// What the button asks the sim for; none when it is grey.
    pub request: Option<CraftRequest>,
}

/// One line of what is on the bench.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HandLine {
    /// The line's key.
    pub id: String,
    /// `Capacity`.
    pub name: String,
    /// What its last roll said, `GOOD +56`, and the outcome it was.
    pub word: Option<(String, Outcome)>,
    /// What the figure is worth now: `29 rounds`. Empty for a part.
    pub means: String,
    /// For a part, what it would give downstream as it stands.
    pub downstream: String,
    /// The figure as it stands.
    pub figure: u16,
    /// What the materials allow.
    pub ceiling: u16,
    /// Where the bar's fill is drawn from: the figure, or the figure
    /// before a roll that is still landing.
    pub fill: u16,
    /// `338 of 563`, with ` · at its ceiling` when it is.
    pub stands: String,
    /// The steady hand.
    pub steady: HandOffer,
    /// The bold one.
    pub bold: HandOffer,
}

/// One thing that went into what is on the bench.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WentRow {
    /// The slot's name: `REACTIVE`.
    pub label: String,
    /// What went in.
    pub name: String,
    /// How much: `20 units`, `1`.
    pub amount: String,
}

/// The screen with something on the bench.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HandStage {
    /// `5 points left`.
    pub points: String,
    /// The product's lines.
    pub lines: Vec<HandLine>,
    /// What went in.
    pub went: Vec<WentRow>,
    /// Whether the stock moved the odds a row upward.
    pub workable: bool,
    /// What Finish asks the sim for; none when it is grey.
    pub request: Option<CraftRequest>,
    /// The line by Finish, and whether it is a refusal.
    pub why: (String, bool),
}

/// What one slot of a blueprint would ask, as the Record box says it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecordNeed {
    /// The slot's key.
    pub slot: String,
    /// The slot's name: `Reactive`.
    pub label: String,
    /// `Potency at least 537 · Stability at least 191`.
    pub asks: String,
}

/// The Record box: what recording the finished thing at the slider's
/// derating would write, before Record is pressed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecordBox {
    /// What the slider stands at, per-mille of the prototype.
    pub derate: u16,
    /// The least and the most it can stand at and the step between.
    pub range: (u16, u16, u16),
    /// `85% of this one`.
    pub at: String,
    /// The lines every run would make: `Charge density 436 · Stability 178`.
    pub makes: String,
    /// What each slot would ask, the slots that ask nothing left out.
    pub needs: Vec<RecordNeed>,
    /// The runs it would start with.
    pub runs: String,
    /// What Record asks the sim for; none when it is grey.
    pub request: Option<CraftRequest>,
    /// Why it is grey; empty when it is not.
    pub why: String,
}

/// The screen with a finished thing's card: one just made, or one made
/// earlier and picked from the list.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DoneStage {
    /// Its long card.
    pub card: CardParams,
    /// What went in.
    pub went: Vec<WentRow>,
    /// Why it cannot be recorded as a blueprint; empty when it can.
    pub cannot_record: String,
    /// The Record box, while it is open.
    pub record: Option<RecordBox>,
}

/// One thing made earlier by the schematic shown and still in the stock.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EarlierRow {
    /// The thing, as the stock holds it.
    pub kind: Record,
    /// `Cell core · batch 3 · 513`.
    pub label: String,
    /// Whether it is the one whose card is shown.
    pub on: bool,
}

/// Which of its three states the screen is in.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Stage {
    /// Nothing on the bench.
    Pick(PickStage),
    /// A session under way.
    Hand(HandStage),
    /// A thing just finished. Boxed: its card is several times the size
    /// of the other two.
    Done(Box<DoneStage>),
}

/// Everything the definition is made from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CraftingView {
    /// `WORKED BY TOVAN REYES`; empty for somebody with no name.
    pub worker: String,
    /// The household's credits.
    pub purse: u32,
    /// The schematics, in their groups.
    pub groups: Vec<SchemGroup>,
    /// What the list is filtered by.
    pub filter: ListFilter,
    /// The schematic shown; none in a world with no schematics.
    pub shown: Option<Namespaced>,
    /// What it makes: `Cell core`.
    pub product: String,
    /// What the schematic shown has made that the stock still holds and
    /// the household's ledger still knows the making of, the earliest
    /// first: each can be recorded as a blueprint. Empty while something
    /// is on the bench.
    pub earlier: Vec<EarlierRow>,
    /// The middle and the right columns; none with no schematic.
    pub stage: Option<Stage>,
    /// Whether a workbench is in reach.
    pub at_workbench: bool,
    /// The footer's line and whether it is a refusal.
    pub status: (String, bool),
    /// What the screen is for and its keys, when the footer's line is the
    /// sim's last word and no longer says so.
    pub keys: Option<String>,
    /// The control the keyboard's focus starts on.
    pub focus: Option<String>,
}

/// What a view is read from: the local player's stock, the purse, the
/// bench and what is in reach.
#[derive(Debug, Clone, Copy)]
pub struct Bench<'a> {
    /// The data set.
    pub registries: &'a GameRegistries,
    /// What the character carries.
    pub pack: &'a Pack,
    /// The household's store, which is to hand at a workbench.
    pub store: &'a HouseholdStore,
    /// The household's credits.
    pub purse: Purse,
    /// Whose weapons a module's line is read on.
    pub holder: &'a Holder,
    /// What the household has made and has under way.
    pub craft: &'a CraftState,
    /// Whether a workbench is in reach.
    pub at_workbench: bool,
    /// Who is at the bench.
    pub worker: &'a str,
    /// What they have learned: the boxes that teach a schematic, and the
    /// points a session opens with.
    pub sheet: &'a CharacterSheet,
    /// What is in reach, for who could teach a box here.
    pub standing: &'a Reach,
    /// What a finished thing's card is read against; none where there is
    /// nobody to read it for.
    pub against: Option<&'a Against>,
    /// What the device in use calls the two inputs that change the
    /// workbench's tab, the one before and the one after.
    pub tab_keys: &'a [String; 2],
}

/// A second sentence for a gate the character could open where they
/// stand: the box is not learned, and the sheet's own rule, asked with the
/// teacher the reach gives, would let them learn it.
fn learnable_here(bench: &Bench<'_>, refusal: &CraftRefusal) -> String {
    let CraftRefusal::NotTaught { needs } = refusal else {
        return String::new();
    };
    let graph = &bench.registries.progression;
    let teacher = bench.standing.teaches(graph, needs);
    if bench.sheet.can_learn(graph, needs, teacher).is_ok() {
        format!(" {}", words("strata.crafting.learn_here"))
    } else {
        String::new()
    }
}

impl<'a> Bench<'a> {
    /// The store, where it can be reached: at a workbench.
    fn reach(&self) -> Option<&'a HouseholdStore> {
        self.at_workbench.then_some(self.store)
    }
}

/// Why the sim would not do it, as a player reads it. `slot` gives the
/// name of a slot by its key.
#[must_use]
pub fn refusal_words(
    refusal: &CraftRefusal,
    graph: &SkillGraph,
    slot: impl Fn(&str) -> String,
) -> String {
    match refusal {
        CraftRefusal::NoPlayer => words("strata.crafting.refused.no_player").to_owned(),
        CraftRefusal::NoWorkbench => words("strata.crafting.refused.no_workbench").to_owned(),
        CraftRefusal::UnknownSchematic | CraftRefusal::UnknownLine => {
            words("strata.crafting.refused.unknown").to_owned()
        }
        CraftRefusal::SessionOpen => words("strata.crafting.refused.session").to_owned(),
        CraftRefusal::Slot { slot: key, why } => {
            let name = slot(key);
            match why {
                SlotRefusal::Empty => fill(words("strata.crafting.why.slot.empty"), "slot", &name),
                SlotRefusal::WrongKind => {
                    fill(words("strata.crafting.why.slot.wrong"), "slot", &name)
                }
                SlotRefusal::Short { have, needs } => fill(
                    &fill(
                        &fill(words("strata.crafting.why.slot.short"), "slot", &name),
                        "have",
                        &have.to_string(),
                    ),
                    "needs",
                    &needs.to_string(),
                ),
            }
        }
        CraftRefusal::NotTaught { needs } => {
            // The tree's and the box's names as the sheet screen draws them.
            let (tree, name) = graph.find_with_tree(needs).map_or_else(
                || (needs.clone(), needs.clone()),
                |(tree, found)| (tree.name.clone(), found.name.clone()),
            );
            fill(
                &fill(words("strata.crafting.refused.not_taught"), "tree", &tree),
                "box",
                &name,
            )
        }
        CraftRefusal::NoSession => words("strata.crafting.refused.no_session").to_owned(),
        CraftRefusal::NoPoints => words("strata.crafting.refused.no_points").to_owned(),
        CraftRefusal::AtCeiling => words("strata.crafting.refused.at_ceiling").to_owned(),
        CraftRefusal::NoRoom => words("strata.crafting.refused.no_room").to_owned(),
        CraftRefusal::NotForSale => words("strata.crafting.refused.not_for_sale").to_owned(),
        CraftRefusal::Short { needs, have } => fill(
            &fill(
                words("strata.crafting.buy.short"),
                "needs",
                &credits(*needs),
            ),
            "have",
            &grouped(*have),
        ),
        CraftRefusal::NoBlueprints => words("strata.crafting.refused.no_blueprints").to_owned(),
        CraftRefusal::NoPrototype => words("strata.crafting.refused.no_prototype").to_owned(),
        CraftRefusal::NoLedger => words("strata.crafting.refused.no_ledger").to_owned(),
        CraftRefusal::RunOff { blueprint } => fill(
            words("strata.crafting.refused.run_off"),
            "blueprint",
            &blueprint.to_string(),
        ),
        CraftRefusal::Recorded { blueprint } => fill(
            words("strata.crafting.refused.recorded"),
            "blueprint",
            &blueprint.to_string(),
        ),
        CraftRefusal::Derate { .. } => words("strata.crafting.refused.derate").to_owned(),
    }
}

// ---- reading the sim ----

/// What a kind of stack is called in a list: a lot by its deposit (`Tolley
/// salt`), anything else by its item's name.
pub(super) fn stock_name(registries: &GameRegistries, record: &Record) -> String {
    CardParams::of_lot(record, registries, false)
        .map(|card| card.name)
        .or_else(|| {
            registries
                .items
                .get(&record.item)
                .map(|def| def.name.clone())
        })
        .unwrap_or_else(|| record.item.to_string())
}

/// The shape and the colour a kind of stack is drawn by: its item's own,
/// as its cell on the stores screen is.
pub(super) fn swatch_of(registries: &GameRegistries, record: &Record) -> Option<Swatch> {
    swatch_of_item(registries, &record.item)
}

/// [`swatch_of`] for an item named by its id alone, which a board's order
/// and sale rows hold.
pub(super) fn swatch_of_item(
    registries: &GameRegistries,
    item: &strata_registry::Namespaced,
) -> Option<Swatch> {
    let def = registries.items.get(item)?;
    let [red, green, blue, _] = def.icon.color.0;
    #[allow(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "a colour channel, clamped to its range first"
    )]
    let tint = [red, green, blue].map(|channel| (channel.clamp(0., 1.) * 255.).round() as u8);
    // A list asks for symbols alone: what it lists (lots, parts, stock) has
    // no baked picture, so the rule gives the pack's own image.
    let art = card::item_art(def, None, None, &registries.icons);
    Some(Swatch {
        shape: def.icon.shape,
        tint,
        tinted: art.as_ref().is_some_and(|art| art.tint.is_some()),
        image: art.map(|art| art.path),
    })
}

/// What a stat a slot is weighed by is called: an attribute's own name, or
/// the name the part the slot takes gives its line.
fn stat_name(registries: &GameRegistries, slot: &SlotDef, stat: &Stat) -> String {
    match stat {
        Stat::Attribute(attribute) | Stat::Light(attribute) => attribute.name().to_owned(),
        Stat::Line(line) => match &slot.takes {
            Takes::Component(item) | Takes::Commodity(item) => registries
                .items
                .get(item)
                .and_then(|def| def.lines.iter().find(|own| own.id == *line))
                .map(|own| own.name.clone()),
            _ => None,
        }
        .unwrap_or_else(|| line.clone()),
    }
}

/// `any reactive · 20 units`, or `one cell core`: what a slot takes and how
/// much of it.
fn wants_words(registries: &GameRegistries, slot: &SlotDef) -> String {
    let set = &registries.resources;
    let lot = |name: &str| {
        fill(
            &fill(
                words("strata.crafting.wants.lot"),
                "kind",
                &name.to_lowercase(),
            ),
            "units",
            &slot.units.to_string(),
        )
    };
    match &slot.takes {
        Takes::Family(id) => set
            .families()
            .iter()
            .find(|family| family.id == *id)
            .map(|family| lot(&family.name)),
        Takes::Class(id) => set
            .classes()
            .iter()
            .find(|class| class.id == *id)
            .map(|class| lot(&class.name)),
        Takes::Type(id) => set
            .types()
            .iter()
            .find(|ty| ty.id == *id)
            .map(|ty| lot(&ty.name)),
        Takes::Component(id) | Takes::Commodity(id) => {
            card::plain_name(&registries.items, id).map(|name| {
                if slot.count == 1 {
                    fill(words("strata.crafting.wants.one"), "name", &name)
                } else {
                    fill(
                        &fill(
                            words("strata.crafting.wants.many"),
                            "count",
                            &slot.count.to_string(),
                        ),
                        "name",
                        &name,
                    )
                }
            })
        }
    }
    .unwrap_or_default()
}

/// An amount in the game's units, as a card writes it: `29 rounds`.
fn amount_words(property: Property, live: bool, value: Amount) -> String {
    card::value_words(&Line {
        property,
        live,
        value,
        change: None,
    })
}

/// What a weapon's line is worth: `4 to 5 of 6 mounts open`, or the shots
/// it lasts. A line the fight has no number for says nothing here and is
/// marked as coming later by its row.
fn weapon_means_words(means: WeaponMeans) -> String {
    let both = |key: &'static str, low: u32, high: u32| {
        let said = if low == high {
            low.to_string()
        } else {
            fill(
                &fill(
                    words("strata.crafting.weapon.range"),
                    "low",
                    &low.to_string(),
                ),
                "high",
                &high.to_string(),
            )
        };
        fill(words(key), "figure", &said)
    };
    match means {
        WeaponMeans::Mounts { low, high, of } => fill(
            &both("strata.crafting.weapon.mounts", low, high),
            "of",
            &of.to_string(),
        ),
        WeaponMeans::Shots { low, high } => both("strata.crafting.weapon.shots", low, high),
        WeaponMeans::Cuts { low, high } => both("strata.crafting.weapon.cuts", low, high),
        // The row's number in the fitting screen's own words.
        WeaponMeans::Row {
            property,
            low,
            high,
            burst,
        } => fill(
            words(match (property, burst) {
                // A blade's own number is its cut, not a pause between shots.
                (Property::HitDamage | Property::HitPosture, _) => "strata.crafting.weapon.row.cut",
                // A signature on the cone is the aimed spread, not a pause.
                (Property::AimSpread, _) => "strata.crafting.weapon.row.aim",
                (_, true) => "strata.crafting.weapon.row",
                (_, false) => "strata.crafting.weapon.row.single",
            }),
            "figure",
            &means_words(&Means {
                property,
                live: true,
                low,
                high,
            }),
        ),
        WeaponMeans::NotInTheFight => String::new(),
    }
}

/// What a line's figures are worth: `28 to 29 rounds`, `1.50 to 1.48 s`,
/// or one amount where the two ends read alike. The unit is said once.
fn means_words(means: &Means) -> String {
    let (low, high) = (means.low, means.high);
    if low == high {
        return amount_words(means.property, means.live, low);
    }
    let range = |low: &str, high: &str| {
        fill(
            &fill(words("strata.crafting.range"), "low", low),
            "high",
            high,
        )
    };
    let unit = match low.unit {
        Unit::Rounds => "strata.figure.rounds",
        Unit::Seconds => "strata.figure.seconds",
        Unit::Degrees => "strata.figure.degrees",
        Unit::Percent => "strata.figure.percent",
        Unit::Points => "strata.figure.points",
        Unit::Metres => "strata.figure.metres",
        // A share of standard reads in words, each end with its own.
        Unit::OfStandard => {
            return range(
                &amount_words(means.property, means.live, low),
                &amount_words(means.property, means.live, high),
            );
        }
    };
    fill(words(unit), "n", &range(&low.text(), &high.text()))
}

/// What a part's line gives where it goes next, `In a patrol extended
/// magazine: 28 to 29 rounds`, or that nothing reads it yet.
fn downstream_words(registries: &GameRegistries, line: &LinePreview) -> String {
    line.downstream
        .as_ref()
        .and_then(|(product, means)| {
            Some(fill(
                &fill(
                    words("strata.crafting.downstream"),
                    "product",
                    &card::a_name(&registries.items, product)?,
                ),
                "range",
                &means_words(means),
            ))
        })
        .unwrap_or_else(|| words("strata.card.goes_nowhere").to_owned())
}

/// How much of a slot's stock: `20 units` of a lot, a bare count of a part.
fn amount_of(slot: Option<&SlotDef>, amount: u32) -> String {
    if slot.is_none_or(|slot| slot.takes.is_resource()) {
        fill(
            words("strata.crafting.went.units"),
            "units",
            &amount.to_string(),
        )
    } else {
        amount.to_string()
    }
}

/// What went into a batch, a row a slot, named by `schematic`'s slots.
fn went_rows(
    registries: &GameRegistries,
    schematic: &Schematic,
    inputs: &[(String, Record)],
) -> Vec<WentRow> {
    inputs
        .iter()
        .map(|(slot, input)| {
            let def = schematic.slot(slot).map(|(_, def)| def);
            WentRow {
                label: def.map_or_else(|| slot.to_uppercase(), |def| def.name.to_uppercase()),
                name: stock_name(registries, input),
                amount: amount_of(def, input.count),
            }
        })
        .collect()
}

impl CandidateRow {
    /// The row of `candidate` for `slot` of `schematic`.
    fn of(
        registries: &GameRegistries,
        slot: &SlotDef,
        candidate: &Candidate,
        (best, on): (bool, bool),
    ) -> Self {
        let lot = slot.takes.is_resource();
        let short = candidate.short.map(|(have, needs)| {
            fill(
                &fill(
                    words(if lot {
                        "strata.crafting.pick.short.units"
                    } else {
                        "strata.crafting.pick.short.count"
                    }),
                    "have",
                    &have.to_string(),
                ),
                "needs",
                &needs.to_string(),
            )
        });
        let held = if lot {
            fill(
                words("strata.crafting.pick.units"),
                "units",
                &candidate.held().to_string(),
            )
        } else {
            candidate.held().to_string()
        };
        Self {
            kind: candidate.kind.clone(),
            name: stock_name(registries, &candidate.kind),
            batch: if candidate.kind.by.is_some() {
                fill(
                    words("strata.crafting.pick.batch"),
                    "batch",
                    &candidate.kind.batch.to_string(),
                )
            } else {
                String::new()
            },
            swatch: swatch_of(registries, &candidate.kind),
            stats: candidate
                .stats
                .iter()
                .map(|(stat, figure)| StatWords {
                    name: stat_name(registries, slot, stat),
                    figure: figure.to_string(),
                    lighter: stat.is_light(),
                    // A line that weighs it shows it once, as the odds' figure.
                    odds: matches!(stat, Stat::Attribute(Attribute::Workability)),
                })
                // What no line weighs and the whole session's odds turn
                // on, so it is seen before the stock is spent.
                .chain(
                    candidate
                        .workability
                        .filter(|_| {
                            !candidate.stats.iter().any(|(stat, _)| {
                                matches!(stat, Stat::Attribute(Attribute::Workability))
                            })
                        })
                        .map(|figure| StatWords {
                            name: Attribute::Workability.name().to_owned(),
                            figure: figure.to_string(),
                            lighter: false,
                            odds: true,
                        }),
                )
                .collect(),
            short,
            held,
            best,
            on,
        }
    }
}

impl SlotView {
    /// The view of `slot` of `schematic`, with what `state` has picked for
    /// it when the stock still holds that.
    fn of(bench: &Bench<'_>, state: &CraftingState, schematic: &Schematic, slot: &SlotDef) -> Self {
        let registries = bench.registries;
        let candidates =
            crafting::candidates(registries, bench.pack, bench.reach(), schematic, &slot.id);
        let pick = state.picks.get(&slot.id);
        let rows: Vec<CandidateRow> = candidates
            .iter()
            .enumerate()
            .map(|(index, candidate)| {
                // The top row is the best even when there is too little of
                // it, which its own line then says.
                let marks = (index == 0, pick == Some(&candidate.kind));
                CandidateRow::of(registries, slot, candidate, marks)
            })
            .collect();
        let open = state.open.as_deref() == Some(slot.id.as_str()) && !rows.is_empty();
        let will_do = candidates.iter().any(|candidate| candidate.short.is_none());
        let none = (!will_do).then(|| {
            words(match (rows.is_empty(), bench.at_workbench) {
                (true, true) => "strata.crafting.slot.none",
                (true, false) => "strata.crafting.slot.none.pack",
                (false, true) => "strata.crafting.slot.short",
                (false, false) => "strata.crafting.slot.short.pack",
            })
            .to_owned()
        });
        let picked = rows.iter().any(|row| row.on);
        let count = match (rows.len(), open) {
            (0, _) => String::new(),
            (_, true) => words("strata.crafting.slot.close").to_owned(),
            // The one there is, already in the slot: nothing more to list.
            (1, false) if picked => String::new(),
            (1, false) => words("strata.crafting.slot.fits").to_owned(),
            (fit, false) => fill(words("strata.crafting.slot.fit"), "count", &fit.to_string()),
        };
        Self {
            id: slot.id.clone(),
            label: slot.name.to_uppercase(),
            wants: wants_words(registries, slot),
            picked: rows.iter().find(|row| row.on).cloned(),
            count,
            open,
            rows,
            none,
            buy: (!will_do).then(|| BuyView::of(bench, slot)).flatten(),
        }
    }
}

impl BuyView {
    /// The Buy of `slot`, for a slot that takes something with a price.
    fn of(bench: &Bench<'_>, slot: &SlotDef) -> Option<Self> {
        let (Takes::Component(item) | Takes::Commodity(item)) = &slot.takes else {
            return None;
        };
        let price = bench.registries.items.get(item)?.price?;
        // The sim's own order: the bench first, then the purse and the room.
        let can = if bench.at_workbench {
            crafting::can_buy(
                bench.registries,
                bench.pack,
                bench.reach(),
                &bench.purse,
                item,
                1,
            )
        } else {
            Err(CraftRefusal::NoWorkbench)
        };
        Some(Self {
            item: item.clone(),
            label: fill(words("strata.crafting.buy"), "cost", &credits(price)),
            request: can.is_ok().then(|| CraftRequest::Buy {
                item: item.clone(),
                count: 1,
            }),
            why: can.err().map_or_else(String::new, |refusal| {
                refusal_words(&refusal, &bench.registries.progression, str::to_owned)
            }),
        })
    }
}

impl PickStage {
    #[allow(
        clippy::too_many_lines,
        reason = "the slots, the lines and the button, in the order the columns show them"
    )]
    fn of(bench: &Bench<'_>, state: &CraftingState, schematic: &Schematic) -> Self {
        let registries = bench.registries;
        let slots: Vec<SlotView> = schematic
            .def
            .slots
            .iter()
            .map(|slot| SlotView::of(bench, state, schematic, slot))
            .collect();
        // What is picked and still in the stock: a pick names a kind, and
        // the stock may have moved under the screen since it was made.
        let picks: Picks = slots
            .iter()
            .filter_map(|slot| Some((slot.id.clone(), slot.picked.as_ref()?.kind.clone())))
            .collect();
        let product = registries.items.get(&schematic.def.makes);
        let part = product.is_some_and(|def| matches!(def.kind, ItemKind::Component));
        let lines = crafting::preview_for(registries, bench.holder, schematic, &picks, bench.sheet)
            .into_iter()
            .enumerate()
            .filter_map(|(index, line)| {
                let own = product?.lines.get(index)?;
                Some(match line {
                    Some(line) => OutLine {
                        id: own.id.clone(),
                        name: own.name.clone(),
                        later: line.means.is_some_and(|means| !means.live)
                            || line.weapon == Some(WeaponMeans::NotInTheFight),
                        bar: Some((line.floor, line.ceiling)),
                        means: line.means.as_ref().map_or_else(
                            || line.weapon.map_or_else(String::new, weapon_means_words),
                            means_words,
                        ),
                        range: fill(
                            &fill(
                                words("strata.crafting.line.range"),
                                "floor",
                                &line.floor.to_string(),
                            ),
                            "ceiling",
                            &line.ceiling.to_string(),
                        ),
                        downstream: if part {
                            downstream_words(registries, &line)
                        } else {
                            String::new()
                        },
                    },
                    None => OutLine {
                        id: own.id.clone(),
                        name: own.name.clone(),
                        later: false,
                        bar: None,
                        means: String::new(),
                        range: words("strata.crafting.line.unfilled").to_owned(),
                        downstream: String::new(),
                    },
                })
            })
            .collect();
        let used: Vec<String> = schematic
            .def
            .slots
            .iter()
            .filter_map(|slot| {
                let name = stock_name(registries, picks.get(&slot.id)?);
                Some(if slot.takes.is_resource() {
                    fill(
                        &fill(
                            words("strata.crafting.uses.units"),
                            "units",
                            &slot.amount().to_string(),
                        ),
                        "name",
                        &name,
                    )
                } else {
                    fill(
                        &fill(
                            words("strata.crafting.uses.count"),
                            "count",
                            &slot.amount().to_string(),
                        ),
                        "name",
                        &name.to_lowercase(),
                    )
                })
            })
            .collect();
        let slot_name = |key: &str| {
            schematic
                .slot(key)
                .map_or_else(|| key.to_owned(), |(_, slot)| slot.name.clone())
        };
        // Asked in the sim's own order, so the reason beside the button is
        // the one a request would be refused for.
        let refused = if bench.at_workbench {
            crafting::check_picks(
                registries,
                bench.pack,
                bench.reach(),
                schematic,
                &picks,
                bench.sheet,
            )
            .err()
        } else {
            Some(CraftRefusal::NoWorkbench)
        };
        Self {
            slots,
            lines,
            uses: if used.is_empty() {
                String::new()
            } else {
                fill(words("strata.crafting.uses"), "list", &used.join(", "))
            },
            workable: crafting::lifts_odds(
                registries.schematics.rules(),
                crafting::workability(schematic, &picks),
            ),
            why: refused.as_ref().map_or_else(String::new, |refusal| {
                refusal_words(refusal, &registries.progression, slot_name)
                    + &learnable_here(bench, refusal)
            }),
            request: refused.is_none().then(|| CraftRequest::Assemble {
                schematic: schematic.def.id.clone(),
                picks,
            }),
        }
    }
}

impl HandStage {
    #[allow(
        clippy::too_many_lines,
        reason = "one session read line by line, each with its two hands"
    )]
    fn of(
        bench: &Bench<'_>,
        state: &CraftingState,
        schematic: &Schematic,
        session: &Session,
    ) -> Self {
        let registries = bench.registries;
        let rules = registries.schematics.rules();
        let product = registries.items.get(&schematic.def.makes);
        let part = product.is_some_and(|def| matches!(def.kind, ItemKind::Component));
        let stands = crafting::now(registries, bench.holder, session);
        let lines = session
            .lines
            .iter()
            .map(|line| {
                let shown = stands.iter().find(|shown| shown.line == line.line);
                let (figure, full) = (line.figure(), line.at_ceiling());
                // The sim's own question, so a hand is grey exactly when a
                // roll on it would be refused.
                let can = bench.at_workbench && crafting::can_roll(session, &line.line).is_ok();
                let offer = |hand: Hand| {
                    let odds = crafting::hand_odds(rules, line, hand, session.workability);
                    let gain = |by: i16| signed(i32::from(by));
                    let said = match (hand, odds.slip) {
                        (Hand::Steady, _) => fill(
                            words("strata.crafting.steady.offer"),
                            "gain",
                            &gain(odds.most_often),
                        ),
                        (Hand::Bold, None) => fill(
                            words("strata.crafting.bold.offer"),
                            "gain",
                            &gain(odds.best),
                        ),
                        (Hand::Bold, Some(slip)) => fill(
                            &fill(
                                words("strata.crafting.bold.offer.slip"),
                                "gain",
                                &gain(odds.best),
                            ),
                            "slip",
                            &slip.to_string(),
                        ),
                    };
                    HandOffer {
                        odds: odds.odds,
                        words: said,
                        request: can.then(|| CraftRequest::Roll {
                            line: line.line.clone(),
                            hand,
                        }),
                    }
                };
                HandLine {
                    id: line.line.clone(),
                    name: shown.map_or_else(|| line.line.clone(), |shown| shown.name.clone()),
                    word: line.last.map(|(outcome, moved)| {
                        let said = fill(
                            &fill(
                                words("strata.crafting.word"),
                                "outcome",
                                &outcome_word(outcome).to_uppercase(),
                            ),
                            "moved",
                            &signed(i32::from(moved)),
                        );
                        (said, outcome)
                    }),
                    means: shown.map_or_else(String::new, |shown| {
                        shown.means.as_ref().map_or_else(
                            || shown.weapon.map_or_else(String::new, weapon_means_words),
                            means_words,
                        )
                    }),
                    downstream: match shown {
                        Some(shown) if part => downstream_words(registries, shown),
                        _ => String::new(),
                    },
                    figure,
                    ceiling: line.ceiling,
                    fill: state
                        .fill
                        .as_ref()
                        .filter(|fill| fill.line == line.line)
                        .map_or(figure, |fill| fill.from),
                    stands: fill(
                        &fill(
                            words(if full {
                                "strata.crafting.stands.full"
                            } else {
                                "strata.crafting.stands"
                            }),
                            "figure",
                            &figure.to_string(),
                        ),
                        "ceiling",
                        &line.ceiling.to_string(),
                    ),
                    steady: offer(Hand::Steady),
                    bold: offer(Hand::Bold),
                }
            })
            .collect();
        // Beside Finish: why nothing here acts, or that the points are
        // gone, in the words the sim's refusal has.
        let spent = session
            .lines
            .iter()
            .map(|line| crafting::can_roll(session, &line.line))
            .find(|can| *can == Err(CraftRefusal::NoPoints));
        let why = if !bench.at_workbench {
            (
                refusal_words(
                    &CraftRefusal::NoWorkbench,
                    &bench.registries.progression,
                    str::to_owned,
                ),
                true,
            )
        } else if let Some(Err(refusal)) = spent {
            (
                refusal_words(&refusal, &bench.registries.progression, str::to_owned),
                false,
            )
        } else {
            (String::new(), false)
        };
        Self {
            points: if session.points == 1 {
                words("strata.crafting.point").to_owned()
            } else {
                fill(
                    words("strata.crafting.points"),
                    "points",
                    &session.points.to_string(),
                )
            },
            lines,
            went: went_rows(registries, schematic, &session.inputs),
            workable: crafting::lifts_odds(rules, session.workability),
            request: bench.at_workbench.then_some(CraftRequest::Finish),
            why,
        }
    }
}

impl RecordBox {
    /// What recording `done` at `derate` per-mille would write, by the
    /// sim's own draft.
    fn of(bench: &Bench<'_>, done: &Record, derate: u16) -> Self {
        let registries = bench.registries;
        let range = registries
            .schematics
            .factory()
            .map_or((QUALITY_MAX, QUALITY_MAX, 1), FactoryRules::derate);
        let at = fill(
            words("strata.crafting.record.share"),
            "percent",
            &card::percent_of(derate).to_string(),
        );
        match blueprint::draft_for(
            registries,
            bench.craft,
            done,
            derate,
            (bench.worker, bench.sheet),
        ) {
            Ok(print) => {
                let product = registries.items.get(&print.makes);
                let makes: Vec<String> = product
                    .into_iter()
                    .flat_map(|def| &def.lines)
                    .filter_map(|line| {
                        Some(format!("{} {}", line.name, print.lines.get(&line.id)?))
                    })
                    .collect();
                Self {
                    derate,
                    range,
                    at,
                    makes: makes.join(" \u{b7} "),
                    needs: print
                        .slots
                        .iter()
                        .filter(|need| !need.needs.is_empty())
                        .map(|need| RecordNeed {
                            slot: need.slot.id.clone(),
                            label: need.slot.name.clone(),
                            asks: card::needs_words(registries, &need.slot, &need.needs)
                                .join(" \u{b7} "),
                        })
                        .collect(),
                    runs: print.runs.to_string(),
                    request: Some(CraftRequest::Record {
                        batch: done.batch,
                        derate,
                    }),
                    why: String::new(),
                }
            }
            Err(refusal) => Self {
                derate,
                range,
                at,
                makes: String::new(),
                needs: Vec::new(),
                runs: String::new(),
                request: None,
                why: refusal_words(&refusal, &bench.registries.progression, str::to_owned),
            },
        }
    }
}

/// Whether `done` can be recorded as a blueprint from where the character
/// stands: the workbench, as a request is held to it, and then the sim's
/// own answer for a recording ([`blueprint::check_record`]) at the top of
/// the dial, which is where the Record box opens.
fn can_record(bench: &Bench<'_>, done: &Record) -> Result<(), CraftRefusal> {
    let registries = bench.registries;
    if !bench.at_workbench {
        return Err(CraftRefusal::NoWorkbench);
    }
    let top = registries
        .schematics
        .factory()
        .map_or(QUALITY_MAX, |rules| rules.derate().1);
    blueprint::check_record_for(
        registries,
        bench.pack,
        bench.reach(),
        bench.craft,
        done.batch,
        top,
        (bench.worker, bench.sheet),
    )
    .map(|_| ())
}

impl DoneStage {
    /// The card of `done`, what went into it, and whether and as what it
    /// would be recorded.
    fn of(bench: &Bench<'_>, state: &CraftingState, schematic: &Schematic, done: &Record) -> Self {
        let registries = bench.registries;
        let refused = can_record(bench, done).err();
        Self {
            card: CardParams::of(done, registries, true, bench.against)
                .unwrap_or_else(CardParams::empty)
                .framed(),
            went: bench
                .craft
                .made
                .get(&done.batch)
                .filter(|made| made.item == done.item)
                .map_or_else(Vec::new, |made| {
                    went_rows(registries, schematic, &made.from)
                }),
            record: state
                .recording
                .filter(|_| refused.is_none())
                .map(|derate| RecordBox::of(bench, done, derate)),
            cannot_record: refused.map_or_else(String::new, |refusal| {
                refusal_words(&refusal, &bench.registries.progression, str::to_owned)
            }),
        }
    }
}

/// What `schematic` has made that can be shown again and recorded: the
/// sim's list, each with the first of its lines as the figure that tells
/// two batches apart.
fn earlier_rows(
    bench: &Bench<'_>,
    schematic: &Schematic,
    shown: Option<&Record>,
) -> Vec<EarlierRow> {
    let registries = bench.registries;
    let product = registries.items.get(&schematic.def.makes);
    blueprint::prototypes(
        registries,
        bench.pack,
        bench.reach(),
        bench.craft,
        schematic,
    )
    .into_iter()
    .map(|kind| {
        let lead = product
            .and_then(|def| def.lines.first())
            .and_then(|line| kind.lines.get(&line.id))
            .map_or_else(String::new, u16::to_string);
        let label = [
            ("name", stock_name(registries, &kind)),
            ("batch", kind.batch.to_string()),
            ("lead", lead),
        ]
        .iter()
        .fold(
            words("strata.crafting.earlier.row").to_owned(),
            |said, (name, with)| fill(&said, name, with),
        );
        EarlierRow {
            on: shown.is_some_and(|shown| crafting::kind_of(shown) == kind),
            kind,
            label,
        }
    })
    .collect()
}

/// What a roll came to, as its line says it.
fn outcome_word(outcome: Outcome) -> &'static str {
    english(&format!("strata.crafting.outcome.{}", outcome.key())).unwrap_or(outcome.key())
}

/// The list's groups.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Group {
    /// The weapons.
    Weapons,
    /// The modules of a gun.
    GunParts,
    /// The modules of a blade.
    BladeParts,
    /// The components they are made of.
    Components,
    /// The carried tools and the bike.
    Tools,
    /// The kits.
    Kits,
}

/// The groups in the order the list draws them: what a player ends with
/// first, then what goes on it, then what it is made of.
const GROUPS: [(Group, &str); 6] = [
    (Group::Weapons, "strata.crafting.group.weapons"),
    (Group::GunParts, "strata.crafting.group.gun_parts"),
    (Group::BladeParts, "strata.crafting.group.blade_parts"),
    (Group::Components, "strata.crafting.group.components"),
    (Group::Tools, "strata.crafting.group.tools"),
    (Group::Kits, "strata.crafting.group.kits"),
];

/// The order the screen looks in for what to show when nothing says: what a
/// first day starts with (a core to make a magazine of) before the weapon
/// the list leads with.
const FIRST_SHOWN: [Group; 6] = [
    Group::Components,
    Group::GunParts,
    Group::BladeParts,
    Group::Tools,
    Group::Weapons,
    Group::Kits,
];

/// Whether a module is for a blade: every weapon it fits is a melee class,
/// by the family the weapon's item names.
fn fits_blades(registries: &GameRegistries, fits: &[Namespaced]) -> bool {
    !fits.is_empty()
        && fits.iter().all(|weapon| {
            registries
                .items
                .get(weapon)
                .is_some_and(|def| match &def.kind {
                    ItemKind::Weapon { family, .. } => veyra_sim::combat::Weapon::from_name(family)
                        .is_some_and(veyra_sim::combat::Weapon::is_melee),
                    _ => false,
                })
        })
}

/// The schematics of one of the list's groups, by id.
fn listed(registries: &GameRegistries, group: Group) -> impl Iterator<Item = &Schematic> {
    registries.schematics.iter().filter(move |schematic| {
        registries
            .items
            .get(&schematic.def.makes)
            .is_some_and(|def| match &def.kind {
                ItemKind::Component => group == Group::Components,
                ItemKind::Module { fits, .. } => {
                    group
                        == if fits_blades(registries, fits) {
                            Group::BladeParts
                        } else {
                            Group::GunParts
                        }
                }
                ItemKind::Weapon { .. } => group == Group::Weapons,
                ItemKind::Tool { .. } | ItemKind::Vehicle { .. } | ItemKind::Harvester { .. } => {
                    group == Group::Tools
                }
                ItemKind::Kit => group == Group::Kits,
                _ => false,
            })
    })
}

/// The session on the bench, when the data set still has its schematic.
fn session_of<'a>(bench: &Bench<'a>) -> Option<&'a Session> {
    bench.craft.session.as_ref().filter(|session| {
        bench
            .registries
            .schematics
            .get(&session.schematic)
            .is_some()
    })
}

/// The schematic the screen shows: the one on the bench, else the one the
/// state names, else the first of the list.
fn shown_of<'a>(bench: &Bench<'a>, state: &CraftingState) -> Option<&'a Schematic> {
    let registries = bench.registries;
    let schematics = &registries.schematics;
    // What the chip hides is not shown beside the list, unless it is on the
    // bench.
    let listed_now = |schematic: &&Schematic| list_wants(bench, state.filter, schematic);
    session_of(bench)
        .and_then(|session| schematics.get(&session.schematic))
        .or_else(|| {
            state
                .schematic
                .as_ref()
                .and_then(|id| schematics.get(id))
                .filter(listed_now)
        })
        .or_else(|| {
            FIRST_SHOWN
                .into_iter()
                .flat_map(|group| listed(registries, group))
                .filter(listed_now)
                .find(|schematic| crafting::taught(schematic, bench.sheet).is_ok())
        })
        .or_else(|| {
            FIRST_SHOWN
                .into_iter()
                .flat_map(|group| listed(registries, group))
                .find(listed_now)
        })
}

/// Whether the list under `filter` has `schematic` on it.
fn list_wants(bench: &Bench<'_>, filter: ListFilter, schematic: &Schematic) -> bool {
    match filter {
        ListFilter::Known => crafting::taught(schematic, bench.sheet).is_ok(),
        ListFilter::Ready => crafting::can_make(
            bench.registries,
            bench.pack,
            bench.reach(),
            schematic,
            bench.sheet,
        ),
        ListFilter::All => true,
    }
}

impl CraftingView {
    /// The view of `bench` for the screen's `state`.
    #[must_use]
    #[allow(
        clippy::too_many_lines,
        reason = "one screen read left to right, as it is shown"
    )]
    pub fn of(bench: &Bench<'_>, state: &CraftingState) -> Self {
        let registries = bench.registries;
        let session = session_of(bench);
        let made = |schematic: &Schematic| registries.items.get(&schematic.def.makes);
        let shown = shown_of(bench, state);
        let row = |schematic: &Schematic| {
            let def = made(schematic);
            SchemRow {
                id: schematic.def.id.clone(),
                name: def.map_or_else(|| schematic.def.id.to_string(), |def| def.name.clone()),
                mount: match def.map(|def| &def.kind) {
                    Some(ItemKind::Module { slot, .. }) => {
                        super::weapon_diagram::words(slot).to_uppercase()
                    }
                    _ => String::new(),
                },
                can: crafting::can_make(
                    registries,
                    bench.pack,
                    bench.reach(),
                    schematic,
                    bench.sheet,
                ),
                on: shown.is_some_and(|shown| shown.def.id == schematic.def.id),
                busy: session
                    .filter(|session| session.schematic != schematic.def.id)
                    .map(|_| words("strata.crafting.schem.busy").to_owned()),
                untaught: crafting::taught(schematic, bench.sheet)
                    .err()
                    .map(|refusal| refusal_words(&refusal, &registries.progression, str::to_owned)),
            }
        };
        // Taught ones first in each group, the file's order kept within;
        // the chip over the list says which of them are drawn at all.
        let wanted = |schematic: &Schematic| list_wants(bench, state.filter, schematic);
        let groups = GROUPS
            .into_iter()
            .map(|(group, head)| {
                let (taught, untaught): (Vec<_>, Vec<_>) = listed(registries, group)
                    .filter(|schematic| wanted(schematic))
                    .partition(|schematic| crafting::taught(schematic, bench.sheet).is_ok());
                let rows: Vec<SchemRow> = taught.into_iter().chain(untaught).map(row).collect();
                SchemGroup {
                    head: words(head).to_owned(),
                    count: rows.len(),
                    rows,
                }
            })
            .filter(|group| !group.rows.is_empty())
            .collect();
        let finished = state
            .done
            .as_ref()
            .filter(|done| shown.is_some_and(|shown| shown.def.makes == done.item));
        let mut column = shown.map(|schematic| match (session, finished) {
            (Some(session), _) => Stage::Hand(HandStage::of(bench, state, schematic, session)),
            (None, Some(done)) => {
                Stage::Done(Box::new(DoneStage::of(bench, state, schematic, done)))
            }
            (None, None) => Stage::Pick(PickStage::of(bench, state, schematic)),
        });
        // A refusal is said at the button that was pressed as well as in
        // the footer, when the screen had no reason of its own there.
        if let Some((said, true)) = &state.notice {
            let bought = state
                .last
                .as_deref()
                .and_then(|last| last.strip_prefix(BUY_TAG));
            match (&mut column, bought) {
                (Some(Stage::Pick(pick)), Some(slot)) => {
                    let buy = pick
                        .slots
                        .iter_mut()
                        .find(|own| own.id == slot)
                        .and_then(|own| own.buy.as_mut());
                    if let Some(buy) = buy.filter(|buy| buy.why.is_empty()) {
                        buy.why.clone_from(said);
                    }
                }
                (Some(Stage::Pick(pick)), None) if pick.why.is_empty() => {
                    pick.why.clone_from(said);
                }
                (Some(Stage::Hand(hand)), _) if !hand.why.1 => hand.why = (said.clone(), true),
                (Some(Stage::Done(done)), _) => match &mut done.record {
                    Some(record) if record.why.is_empty() => record.why.clone_from(said),
                    None if done.cannot_record.is_empty() => done.cannot_record.clone_from(said),
                    _ => {}
                },
                _ => {}
            }
        }
        let [before, after] = bench.tab_keys;
        let resting = fill(
            &fill(
                words(match (&column, bench.at_workbench) {
                    (_, false) => "strata.crafting.status.reading",
                    (Some(Stage::Hand(_)), true) => "strata.crafting.status.hand",
                    (_, true) => "strata.crafting.status.bench",
                }),
                "prev",
                before,
            ),
            "next",
            after,
        );
        let (status, keys) = match &state.notice {
            Some(notice) => (notice.clone(), Some(resting)),
            None => ((resting, false), None),
        };
        let mut view = Self {
            worker: if bench.worker.is_empty() {
                String::new()
            } else {
                fill(
                    words("strata.crafting.worker"),
                    "name",
                    &bench.worker.to_uppercase(),
                )
            },
            purse: bench.purse.0,
            groups,
            filter: state.filter,
            shown: shown.map(|schematic| schematic.def.id.clone()),
            product: shown
                .and_then(made)
                .map_or_else(String::new, |def| def.name.clone()),
            earlier: shown
                .filter(|_| session.is_none())
                .map_or_else(Vec::new, |schematic| {
                    earlier_rows(bench, schematic, finished)
                }),
            stage: column,
            at_workbench: bench.at_workbench,
            status,
            keys,
            focus: None,
        };
        view.focus = view.focus_for(state.last.as_deref());
        view
    }

    /// The line `id` of what is on the bench.
    fn hand_line(&self, id: &str) -> Option<&HandLine> {
        match &self.stage {
            Some(Stage::Hand(hand)) => hand.lines.iter().find(|line| line.id == id),
            _ => None,
        }
    }

    /// What the hand at `roll` (a hand's `test_id` without its prefix)
    /// asks the sim for; none when it is grey or not there.
    fn roll_request(&self, roll: &str) -> Option<CraftRequest> {
        let (line, hand) = roll.rsplit_once(':')?;
        let line = self.hand_line(line)?;
        if hand == Hand::Steady.key() {
            line.steady.request.clone()
        } else if hand == Hand::Bold.key() {
            line.bold.request.clone()
        } else {
            None
        }
    }

    /// Where the keyboard's focus starts: on the control last used or the
    /// one its press leads to, when that is there to be used; else on the
    /// nearest thing that is. A press on a schematic leads to Best stock,
    /// that to Assemble, that to the first line's Steady; a roll stays on
    /// its hand while the line can still be rolled, then moves to the next
    /// line that can, then to Finish; and Finish leads back to the row of
    /// what was made. So the whole craft is Accept, pressed in place.
    #[allow(
        clippy::too_many_lines,
        reason = "where each control's press leads, a state of the screen an arm"
    )]
    fn focus_for(&self, last: Option<&str>) -> Option<String> {
        let row = || self.shown.as_ref().map(schem_id);
        let Some(last) = last else {
            return row();
        };
        let keep = || Some(last.to_owned());
        if WorkbenchTab::ALL.iter().any(|tab| tab.tab_id() == last) {
            return keep();
        }
        if last.starts_with(FILTER_TAG) {
            return keep();
        }
        if let Some(path) = last.strip_prefix(SCHEM_TAG) {
            let there = self
                .groups
                .iter()
                .flat_map(|group| &group.rows)
                .any(|row| row.id.path() == path && row.busy.is_none());
            return if there { keep() } else { row() };
        }
        if last.starts_with(PROTO_TAG) {
            let listed = self
                .earlier
                .iter()
                .any(|row| proto_id(row.kind.batch) == last);
            return if listed { keep() } else { row() };
        }
        match &self.stage {
            Some(Stage::Pick(pick)) => {
                let slot = |id: &str| pick.slots.iter().find(|slot| slot.id == id);
                if last == BEST || (last == ASSEMBLE && pick.request.is_some()) {
                    keep()
                } else if last == ASSEMBLE || last == AGAIN {
                    Some(BEST.to_owned())
                } else if let Some(of) = last.strip_prefix(OPEN_TAG) {
                    slot(of)
                        .filter(|slot| !slot.rows.is_empty())
                        .map(|_| last.to_owned())
                        .or_else(row)
                } else if let Some(of) = last.strip_prefix(BUY_TAG) {
                    match slot(of) {
                        Some(slot) if slot.buy.is_some() => keep(),
                        Some(slot) if !slot.rows.is_empty() => Some(open_id(&slot.id)),
                        _ => row(),
                    }
                } else if let Some((of, at)) = last
                    .strip_prefix(PICK_TAG)
                    .and_then(|rest| rest.rsplit_once(':'))
                {
                    match slot(of) {
                        Some(slot)
                            if slot.open
                                && at
                                    .parse::<usize>()
                                    .ok()
                                    .and_then(|at| slot.rows.get(at))
                                    .is_some_and(|row| row.short.is_none()) =>
                        {
                            keep()
                        }
                        Some(slot) if !slot.rows.is_empty() => Some(open_id(&slot.id)),
                        _ => row(),
                    }
                } else {
                    row()
                }
            }
            Some(Stage::Hand(hand)) => {
                if last == FINISH {
                    return keep();
                }
                let Some(roll) = last.strip_prefix(ROLL_TAG) else {
                    return row();
                };
                if self.roll_request(roll).is_some() {
                    return keep();
                }
                // The same hand on the next line that can still be rolled,
                // from the line just left and round; then Finish.
                let (line, bold) = roll
                    .rsplit_once(':')
                    .map_or(("", false), |(line, hand)| (line, hand == Hand::Bold.key()));
                let from = hand
                    .lines
                    .iter()
                    .position(|own| own.id == line)
                    .map_or(0, |at| at + 1);
                let next = (0..hand.lines.len())
                    .map(|step| &hand.lines[(from + step) % hand.lines.len()])
                    .find(|line| line.steady.request.is_some());
                Some(next.map_or_else(
                    || FINISH.to_owned(),
                    |line| roll_id(&line.id, if bold { Hand::Bold } else { Hand::Steady }),
                ))
            }
            Some(Stage::Done(_)) if last == AGAIN => keep(),
            // Record as blueprint leads to its slider, and what is in the
            // box leads back to Make another once the box is shut.
            Some(Stage::Done(done)) if last == RECORD_OPEN => {
                if done.record.is_some() {
                    Some(DERATE_ID.to_owned())
                } else {
                    keep()
                }
            }
            Some(Stage::Done(done)) if last == DERATE_ID || last == RECORD_GO => {
                if done.record.is_some() {
                    keep()
                } else {
                    Some(AGAIN.to_owned())
                }
            }
            _ => row(),
        }
    }
}

// ---- the definition ----

pub(super) fn key(s: impl Into<String>) -> LocKey {
    LocKey(s.into())
}

pub(super) fn text_in(s: impl Into<String>, style: TextRole, role: &str) -> UiNodeDef {
    UiNodeDef::Text {
        key: key(s),
        style,
        opts: TextOpts {
            role: Some(Role::new(role)),
            ..TextOpts::default()
        },
        tags: Tags::new(),
    }
}

/// A line that stays on one line: a figure, a label.
pub(super) fn line_in(s: impl Into<String>, role: &str) -> UiNodeDef {
    UiNodeDef::Text {
        key: key(s),
        style: TextRole::Caption,
        opts: TextOpts {
            wrap: false,
            role: Some(Role::new(role)),
            ..TextOpts::default()
        },
        tags: Tags::new(),
    }
}

pub(super) fn with_id(mut node: UiNodeDef, id: &str) -> UiNodeDef {
    if let UiNodeDef::Text { tags, .. } | UiNodeDef::Panel { tags, .. } = &mut node {
        *tags = tags.clone().with(Tags::TEST_ID, id);
    }
    node
}

pub(super) fn panel(role: &str, layout: Layout, children: Vec<UiNodeDef>) -> UiNodeDef {
    UiNodeDef::Panel {
        role: Role::new(role),
        layout,
        children,
        tags: Tags::new(),
    }
}

pub(super) fn column(gap: f32) -> Layout {
    Layout {
        direction: LayoutDirection::Column,
        gap,
        ..Layout::default()
    }
}

pub(super) fn row(gap: f32) -> Layout {
    Layout {
        direction: LayoutDirection::Row,
        gap,
        align: Some(Align::Center),
        ..Layout::default()
    }
}

pub(super) fn spacer() -> UiNodeDef {
    UiNodeDef::Spacer {
        size: Length::Percent(100.),
        tags: Tags::new(),
    }
}

pub(super) fn head(label: impl Into<String>) -> UiNodeDef {
    line_in(label, HEAD_ROLE)
}

/// A column that takes the room its row has left and wraps inside it.
pub(super) fn grows(gap: f32, children: Vec<UiNodeDef>) -> UiNodeDef {
    panel(
        "invisible",
        Layout {
            grow: 1.,
            min_width: Some(Length::Px(0.)),
            ..column(gap)
        },
        children,
    )
}

/// A button that takes the accent only when it can act, and is drawn in
/// the fainter grey with no request to send: the theme has no grey for a
/// primary button, which would stand lit while inert.
pub(super) fn act(label: &str, id: &str, primary: bool, can: bool) -> UiNodeDef {
    UiNodeDef::Button {
        widget: None,
        opts: ButtonOpts {
            label: Some(key(label)),
            variant: if primary && can {
                ButtonVariant::Primary
            } else {
                ButtonVariant::Secondary
            },
            disabled: !can,
            role: (!(primary && can)).then(|| Role::new(ACT_ROLE)),
            ..ButtonOpts::default()
        },
        tags: Tags::new().with("menu", id).with(Tags::TEST_ID, id),
    }
}

/// The well a stack's shape stands in, with the shape of `swatch`; empty
/// for a slot with nothing picked.
pub(super) fn well(swatch: Option<Swatch>) -> UiNodeDef {
    panel(
        PICK_ICON_ROLE,
        Layout {
            width: Some(Length::Px(ICON_WELL)),
            height: Some(Length::Px(ICON_WELL)),
            min_width: Some(Length::Px(ICON_WELL)),
            justify: Justify::Center,
            ..row(0.)
        },
        swatch
            .map(|swatch| {
                let [red, green, blue] = swatch.tint.map(|channel| Model::Int(i64::from(channel)));
                let params = Model::Map(
                    [
                        (
                            "shape".to_owned(),
                            Model::Str(shape_key(swatch.shape).to_owned()),
                        ),
                        ("tint".to_owned(), Model::List(vec![red, green, blue])),
                        (
                            "image".to_owned(),
                            Model::Str(swatch.image.clone().unwrap_or_default()),
                        ),
                        ("tinted".to_owned(), Model::Bool(swatch.tinted)),
                    ]
                    .into(),
                );
                UiNodeDef::Custom {
                    kind: WidgetKind::new(SWATCH_WIDGET),
                    params: slotted::registry::from_model(&params),
                    children: Vec::new(),
                    tags: Tags::new(),
                }
            })
            .into_iter()
            .collect(),
    )
}

/// The figures a slot weighs, of one kind of stack: each stat's name, then
/// its figure lit, with a word where less of it is the better or where it
/// moves the odds and no line. A kind with none to show is a bought-in
/// part, and says so.
fn stats(of: &CandidateRow) -> UiNodeDef {
    if of.stats.is_empty() {
        return line_in("strata.crafting.slot.flat", SUB_ROLE);
    }
    let mut cells = Vec::new();
    for (index, stat) in of.stats.iter().enumerate() {
        if index > 0 {
            cells.push(line_in("\u{b7}", STAT_ROLE));
        }
        cells.push(line_in(stat.name.clone(), STAT_ROLE));
        cells.push(line_in(stat.figure.clone(), STAT_VALUE_ROLE));
        if stat.lighter {
            cells.push(line_in("strata.crafting.pick.lighter", TAG_ROLE));
        }
        if stat.odds {
            cells.push(line_in("strata.crafting.pick.odds", TAG_ROLE));
        }
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

/// One schematic's row: a disc when the stock can make it now and a ring
/// when not, its name, and for a module the mount it is for. The whole row
/// is the button.
fn schem_row(view: &SchemRow) -> UiNodeDef {
    let id = schem_id(&view.id);
    // A shape, since neither typeface has the glyphs, and not a colour
    // alone; the legend over the list says what the disc means.
    let mark = UiNodeDef::Panel {
        role: Role::new(if view.can { MARK_ROLE } else { MARK_NONE_ROLE }),
        layout: Layout {
            width: Some(Length::Px(10.)),
            height: Some(Length::Px(10.)),
            min_width: Some(Length::Px(10.)),
            ..column(0.)
        },
        children: Vec::new(),
        tags: Tags::new()
            .with(
                Tags::TEST_ID,
                &format!("{SCHEM_MARK_TAG}{}", view.id.path()),
            )
            .with(
                "state",
                words(if view.can {
                    "strata.crafting.schem.can"
                } else {
                    "strata.crafting.schem.cannot"
                }),
            ),
    };
    let name_role = if view.busy.is_some() || view.untaught.is_some() {
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
    let mut top = vec![mark, grows(0., vec![named])];
    if !view.mount.is_empty() {
        top.push(head(view.mount.clone()));
    }
    // One line a row: why it cannot be picked is said in the right column
    // when it is the one shown, where its Begin already says so.
    let children = vec![panel(
        "invisible",
        Layout {
            width: Some(Length::Percent(100.)),
            ..row(1.3)
        },
        top,
    )];
    UiNodeDef::Card {
        role: Role::new(if view.on { SCHEM_ON_ROLE } else { SCHEM_ROLE }),
        layout: Layout {
            width: Some(Length::Percent(100.)),
            padding: Padding {
                top: 0.7,
                right: 1.4,
                bottom: 0.7,
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

/// A chip over the list: a pill that is the button, lit and marked when it
/// is the filter in force. The stores' chips' roles, so the two read alike.
fn filter_chip(filter: ListFilter, on: bool) -> UiNodeDef {
    let id = filter.chip_id();
    let label = words(filter.label());
    UiNodeDef::Card {
        role: Role::new(if on {
            "strata.stores.chip.on"
        } else {
            "strata.stores.chip"
        }),
        layout: Layout {
            padding: Padding {
                top: 0.5,
                right: 2.2,
                bottom: 0.5,
                left: 2.2,
            },
            ..row(0.)
        },
        children: vec![text_in(
            if on {
                format!("{FOCUS_MARK}{label}")
            } else {
                label.to_owned()
            },
            TextRole::Caption,
            if on {
                "strata.stores.chip.text.on"
            } else {
                "strata.stores.chip.text"
            },
        )],
        disabled: false,
        tags: Tags::new().with("menu", &id).with(Tags::TEST_ID, &id),
    }
}

/// The schematics' column: the legend, the chips, then each group under its
/// heading.
fn schematics_column(view: &CraftingView) -> Vec<UiNodeDef> {
    let legend = panel(
        "invisible",
        Layout {
            width: Some(Length::Percent(100.)),
            ..row(1.)
        },
        vec![
            head("strata.crafting.schematics"),
            spacer(),
            panel(
                MARK_ROLE,
                Layout {
                    width: Some(Length::Px(8.)),
                    height: Some(Length::Px(8.)),
                    min_width: Some(Length::Px(8.)),
                    ..column(0.)
                },
                Vec::new(),
            ),
            line_in("strata.crafting.legend", SUB_ROLE),
        ],
    );
    let chips = panel(
        "invisible",
        Layout {
            width: Some(Length::Percent(100.)),
            ..row(0.6)
        },
        ListFilter::ALL
            .into_iter()
            .map(|filter| filter_chip(filter, filter == view.filter))
            .collect(),
    );
    let mut left = Vec::new();
    for group in &view.groups {
        left.push(head(format!("{}  ·  {}", group.head, group.count)));
        left.extend(group.rows.iter().map(schem_row));
    }
    if view.groups.is_empty() {
        left.push(text_in(
            match view.filter {
                ListFilter::Known => "strata.crafting.none.known",
                ListFilter::Ready => "strata.crafting.none.ready",
                ListFilter::All => "strata.crafting.none",
            },
            TextRole::Caption,
            NOTE_ROLE,
        ));
    }
    if !view.earlier.is_empty() {
        left.push(head("strata.crafting.earlier"));
        left.extend(view.earlier.iter().map(earlier_row));
    }
    // The legend stays and the rows scroll alone, on wheel, focus and page
    // keys, so a list longer than the window is still reached.
    vec![
        legend,
        chips,
        UiNodeDef::Scroll {
            layout: Layout {
                grow: 1.,
                min_height: Some(Length::Px(0.)),
                align: Some(Align::Stretch),
                ..column(0.6)
            },
            scrollbar: true,
            children: left,
            tags: Tags::new().with(Tags::TEST_ID, LIST_ID),
        },
    ]
}

/// One thing made earlier: the whole row is the button that shows its
/// finished card again, the one shown lit and marked.
fn earlier_row(earlier: &EarlierRow) -> UiNodeDef {
    let id = proto_id(earlier.kind.batch);
    UiNodeDef::Card {
        role: Role::new(if earlier.on {
            SCHEM_ON_ROLE
        } else {
            SCHEM_ROLE
        }),
        layout: Layout {
            width: Some(Length::Percent(100.)),
            padding: Padding {
                top: 0.7,
                right: 1.4,
                bottom: 0.7,
                left: 1.4,
            },
            ..row(1.)
        },
        children: vec![line_in(
            if earlier.on {
                format!("{FOCUS_MARK}{}", earlier.label)
            } else {
                earlier.label.clone()
            },
            STAT_VALUE_ROLE,
        )],
        disabled: false,
        tags: Tags::new().with("menu", &id).with(Tags::TEST_ID, &id),
    }
}

/// A candidate's name with what marks it: the guillemet on the one picked,
/// `BEST` on the first, and a made part's batch.
fn candidate_name(of: &CandidateRow, marked: bool, tagged: bool) -> UiNodeDef {
    let mut cells = vec![text_in(
        if marked && of.on {
            format!("{FOCUS_MARK}{}", of.name)
        } else {
            of.name.clone()
        },
        TextRole::Body,
        if of.short.is_some() {
            PICK_SHORT_ROLE
        } else {
            NAME_ROLE
        },
    )];
    if tagged && of.best {
        cells.push(line_in("strata.crafting.pick.best", TAG_ROLE));
    }
    if !of.batch.is_empty() {
        cells.push(line_in(of.batch.clone(), SUB_ROLE));
    }
    panel(
        "invisible",
        Layout {
            wrap: true,
            ..row(1.)
        },
        cells,
    )
}

/// One candidate of a slot's list: its shape, its name over the figures
/// the slot weighs or why it will not do, and how much the stock holds.
fn candidate_row(slot: &SlotView, at: usize, of: &CandidateRow) -> UiNodeDef {
    let id = pick_id(&slot.id, at);
    let under = of.short.as_ref().map_or_else(
        || stats(of),
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
            grows(0.1, vec![candidate_name(of, true, true), under]),
            line_in(of.held.clone(), FIGURE_ROLE),
        ],
        disabled: of.short.is_some(),
        tags: Tags::new().with("menu", &id).with(Tags::TEST_ID, &id),
    }
}

/// One slot's box: what it takes, the button that shows what is picked and
/// lists what the stock has, the list while it is open, and where nothing
/// will do the sentence that says so with Buy under it.
#[allow(
    clippy::too_many_lines,
    reason = "one box read top to bottom, as it is drawn"
)]
fn slot_box(slot: &SlotView) -> UiNodeDef {
    let mut children = vec![panel(
        "invisible",
        Layout {
            width: Some(Length::Percent(100.)),
            ..row(1.4)
        },
        vec![
            head(slot.label.clone()),
            text_in(slot.wants.clone(), TextRole::Caption, SUB_ROLE),
        ],
    )];
    if !slot.rows.is_empty() {
        let id = open_id(&slot.id);
        let shown = slot.picked.as_ref().map_or_else(
            || {
                vec![
                    well(None),
                    grows(
                        0.1,
                        vec![text_in(
                            "strata.crafting.slot.empty",
                            TextRole::Body,
                            PICK_SHORT_ROLE,
                        )],
                    ),
                ]
            },
            |row| {
                vec![
                    well(row.swatch.clone()),
                    grows(0.1, vec![candidate_name(row, false, false), stats(row)]),
                ]
            },
        );
        let mut inside = shown;
        if !slot.count.is_empty() {
            inside.push(head(slot.count.clone()));
        }
        children.push(UiNodeDef::Card {
            role: Role::new(PICK_ROLE),
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
            children: inside,
            disabled: false,
            tags: Tags::new()
                .with("menu", &id)
                .with(Tags::TEST_ID, &id)
                .with("expanded", if slot.open { "true" } else { "false" }),
        });
    }
    if slot.open {
        children.push(panel(
            "invisible",
            Layout {
                align: Some(Align::Stretch),
                ..column(0.7)
            },
            slot.rows
                .iter()
                .enumerate()
                .map(|(at, row)| candidate_row(slot, at, row))
                .collect(),
        ));
    }
    if let Some(none) = &slot.none {
        children.push(with_id(
            text_in(none.clone(), TextRole::Caption, WHY_ROLE),
            &format!("{NONE_TAG}{}", slot.id),
        ));
    }
    if let Some(buy) = &slot.buy {
        children.push(panel(
            "invisible",
            Layout {
                wrap: true,
                ..row(1.6)
            },
            vec![
                act(&buy.label, &buy_id(&slot.id), false, buy.request.is_some()),
                with_id(
                    text_in(buy.why.clone(), TextRole::Caption, WHY_ROLE),
                    &format!("{BUY_WHY_TAG}{}", slot.id),
                ),
            ],
        ));
    }
    with_id(
        panel(
            if slot.open { SLOT_OPEN_ROLE } else { SLOT_ROLE },
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
            children,
        ),
        &format!("{SLOT_TAG}{}", slot.id),
    )
}

/// One thing that went in, in a box as a slot's is.
fn went_box(went: &WentRow) -> UiNodeDef {
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
            ..column(0.4)
        },
        vec![
            head(went.label.clone()),
            panel(
                "invisible",
                row(1.4),
                vec![
                    text_in(went.name.clone(), TextRole::Body, NAME_ROLE),
                    line_in(went.amount.clone(), SUB_ROLE),
                ],
            ),
        ],
    )
}

/// The middle column once the stock is spent: what went in.
fn went_column(went: &[WentRow]) -> Vec<UiNodeDef> {
    let mut middle = vec![with_id(head("strata.crafting.went"), WENT_ID)];
    middle.extend(went.iter().map(went_box));
    middle.push(text_in(
        "strata.crafting.went.note",
        TextRole::Caption,
        NOTE_ROLE,
    ));
    middle
}

/// A line's name with the tag that says the fight has no number for it.
fn line_head(name: &str, later: bool, more: Vec<UiNodeDef>) -> UiNodeDef {
    let mut cells = vec![text_in(name.to_owned(), TextRole::Body, NAME_ROLE)];
    if later {
        cells.push(line_in("strata.figure.later", TAG_ROLE));
    }
    cells.extend(more);
    panel(
        "invisible",
        Layout {
            width: Some(Length::Percent(100.)),
            ..row(1.2)
        },
        cells,
    )
}

/// One line of what would come out: its bar with the floor marked and the
/// ceiling as its numeral, what both are worth, and the sentence that says
/// the two figures.
fn out_line(line: &OutLine) -> UiNodeDef {
    let mut children = vec![line_head(&line.name, line.later, Vec::new())];
    if let Some((floor, ceiling)) = line.bar {
        children
            .push(BarParams::offered(floor, ceiling).node(Some(&format!("{BAR_TAG}{}", line.id))));
    }
    // What the line is worth in the game's units is the figure a player
    // came for, so it is said first and lit: a module's own, or what a
    // part's would give where it goes next.
    if !line.means.is_empty() {
        children.push(with_id(
            line_in(line.means.clone(), MEANS_ROLE),
            &format!("{MEANS_TAG}{}", line.id),
        ));
    }
    if !line.downstream.is_empty() {
        children.push(downstream(&line.id, &line.downstream));
    }
    children.push(with_id(
        text_in(line.range.clone(), TextRole::Caption, NOTE_ROLE),
        &format!("{RANGE_TAG}{}", line.id),
    ));
    line_box(&line.id, children)
}

/// What a part's line gives where it goes next, lit as a module's own
/// figure is; that it goes into nothing yet is said quietly.
fn downstream(line: &str, said: &str) -> UiNodeDef {
    let nowhere = said == words("strata.card.goes_nowhere");
    with_id(
        text_in(
            said.to_owned(),
            TextRole::Caption,
            if nowhere { NOTE_ROLE } else { MEANS_ROLE },
        ),
        &format!("{DOWNSTREAM_TAG}{line}"),
    )
}

/// One line's block of the right column, clear of the next line's.
fn line_box(line: &str, children: Vec<UiNodeDef>) -> UiNodeDef {
    with_id(
        panel(
            "invisible",
            Layout {
                width: Some(Length::Percent(100.)),
                padding: Padding {
                    bottom: 1.6,
                    ..Padding::default()
                },
                align: Some(Align::Stretch),
                ..column(0.6)
            },
            children,
        ),
        &format!("{LINE_TAG}{line}"),
    )
}

/// The right column with nothing on the bench: every line, what Assemble
/// would use, and the two buttons with why Assemble is grey.
fn out_column(pick: &PickStage) -> Vec<UiNodeDef> {
    let mut right = vec![head("strata.crafting.out")];
    right.extend(pick.lines.iter().map(out_line));
    // What follows stands at the column's foot, so the buttons are where
    // they were whatever is picked and whichever state the screen is in.
    right.push(spacer());
    right.push(with_id(
        text_in(pick.uses.clone(), TextRole::Caption, NOTE_ROLE),
        USES_ID,
    ));
    if pick.workable {
        right.push(with_id(
            text_in(
                "strata.crafting.odds.workable",
                TextRole::Caption,
                NOTE_ROLE,
            ),
            WORKABLE_ID,
        ));
    }
    // The reason over the buttons and not under them: they are the last
    // thing in the column, and do not move when it comes and goes.
    right.push(with_id(
        text_in(pick.why.clone(), TextRole::Caption, WHY_ROLE),
        WHY_ID,
    ));
    right.push(panel(
        "invisible",
        row(1.6),
        vec![
            act("strata.crafting.best", BEST, false, true),
            act(
                "strata.crafting.assemble",
                ASSEMBLE,
                true,
                pick.request.is_some(),
            ),
        ],
    ));
    right
}

/// A hand's odds as five bars, best first, each as tall as its chance. No
/// figure is written: the bars are the odds.
fn odds_bars(line: &str, hand: Hand, odds: [u8; 5]) -> UiNodeDef {
    let id = format!("{ODDS_TAG}{line}:{}", hand.key());
    let params = Model::Map(
        [
            (
                "odds".to_owned(),
                Model::List(
                    odds.iter()
                        .map(|chance| Model::Int(i64::from(*chance)))
                        .collect(),
                ),
            ),
            ("id".to_owned(), Model::Str(id.clone())),
        ]
        .into(),
    );
    UiNodeDef::Custom {
        kind: WidgetKind::new(ODDS_WIDGET),
        params: slotted::registry::from_model(&params),
        children: Vec::new(),
        tags: Tags::new().with(Tags::TEST_ID, &id),
    }
}

/// One hand of a line: its button, its odds, and what it offers in words.
fn hand_row(line: &HandLine, hand: Hand) -> UiNodeDef {
    let (offer, label) = match hand {
        Hand::Steady => (&line.steady, "strata.crafting.steady"),
        Hand::Bold => (&line.bold, "strata.crafting.bold"),
    };
    let id = roll_id(&line.id, hand);
    panel(
        "invisible",
        Layout {
            width: Some(Length::Percent(100.)),
            ..row(1.4)
        },
        vec![
            panel(
                "invisible",
                Layout {
                    width: Some(Length::Px(HAND_WIDTH)),
                    min_width: Some(Length::Px(HAND_WIDTH)),
                    align: Some(Align::Stretch),
                    ..column(0.)
                },
                vec![UiNodeDef::Button {
                    widget: None,
                    opts: ButtonOpts {
                        label: Some(key(label)),
                        variant: ButtonVariant::Secondary,
                        disabled: offer.request.is_none(),
                        compact: true,
                        role: Some(Role::new(ACT_ROLE)),
                        ..ButtonOpts::default()
                    },
                    tags: Tags::new().with("menu", &id).with(Tags::TEST_ID, &id),
                }],
            ),
            odds_bars(&line.id, hand, offer.odds),
            with_id(
                line_in(offer.words.clone(), NOTE_ROLE),
                &format!("{OFFER_TAG}{}:{}", line.id, hand.key()),
            ),
        ],
    )
}

/// One line of what is on the bench: the word its last roll said, its bar
/// with the track lit to its ceiling, where it stands, and the two hands.
fn hand_line(line: &HandLine) -> UiNodeDef {
    let mut more = Vec::new();
    if let Some((said, outcome)) = &line.word {
        more.push(with_id(
            line_in(said.clone(), WORD_ROLES[outcome.index()]),
            &format!("{WORD_TAG}{}", line.id),
        ));
    }
    more.push(spacer());
    if !line.means.is_empty() {
        more.push(with_id(
            line_in(line.means.clone(), MEANS_ROLE),
            &format!("{MEANS_TAG}{}", line.id),
        ));
    }
    let mut children = vec![
        line_head(&line.name, false, more),
        BarParams {
            reads: Some(line.figure),
            ..BarParams::worked(line.fill, line.ceiling)
        }
        .node(Some(&format!("{BAR_TAG}{}", line.id))),
        with_id(
            line_in(line.stands.clone(), NOTE_ROLE),
            &format!("{STANDS_TAG}{}", line.id),
        ),
    ];
    if !line.downstream.is_empty() {
        children.push(downstream(&line.id, &line.downstream));
    }
    children.push(hand_row(line, Hand::Steady));
    children.push(hand_row(line, Hand::Bold));
    line_box(&line.id, children)
}

/// The right column with something on the bench: the points left, every
/// line with its hands, how the bars read, and Finish.
fn hand_column(hand: &HandStage) -> Vec<UiNodeDef> {
    let mut right = vec![panel(
        "invisible",
        Layout {
            width: Some(Length::Percent(100.)),
            ..row(1.)
        },
        vec![
            head("strata.crafting.hand"),
            spacer(),
            with_id(line_in(hand.points.clone(), FIGURE_ROLE), POINTS_ID),
        ],
    )];
    right.extend(hand.lines.iter().map(hand_line));
    right.push(spacer());
    right.push(text_in(
        "strata.crafting.odds.note",
        TextRole::Caption,
        NOTE_ROLE,
    ));
    if hand.workable {
        right.push(with_id(
            text_in(
                "strata.crafting.odds.workable",
                TextRole::Caption,
                NOTE_ROLE,
            ),
            WORKABLE_ID,
        ));
    }
    right.push(panel(
        "invisible",
        row(1.6),
        vec![
            act(
                "strata.crafting.finish",
                FINISH,
                true,
                hand.request.is_some(),
            ),
            with_id(
                text_in(
                    hand.why.0.clone(),
                    TextRole::Caption,
                    if hand.why.1 { WHY_ROLE } else { WHY_HINT_ROLE },
                ),
                WHY_ID,
            ),
        ],
    ));
    right
}

/// A heading at the left and what it heads at the right, on one line: the
/// Record box's rows.
fn headed(label: &str, said: &str, id: &str) -> UiNodeDef {
    panel(
        "invisible",
        Layout {
            width: Some(Length::Percent(100.)),
            ..row(1.2)
        },
        vec![
            head(label),
            spacer(),
            with_id(line_in(said.to_owned(), FIGURE_ROLE), id),
        ],
    )
}

/// The Record box: the derating as a slider, the lines the blueprint
/// would make, what each slot would ask of its stock, the runs it would
/// start with and Record. The patch rewrites the words as the slider
/// moves and leaves the value-bound slider alone, so each has an id.
fn record_box(record: &RecordBox) -> UiNodeDef {
    let (from, to, step) = record.range;
    let share = |per_mille: u16| f64::from(per_mille) / f64::from(QUALITY_MAX);
    let mut children = vec![
        headed("strata.crafting.record.at", &record.at, RECORD_AT_ID),
        UiNodeDef::Slider {
            label: None,
            min: share(from),
            max: share(to),
            step: share(step),
            // The line over it says the figure, in words.
            format: String::new(),
            bind: BindDef {
                bind: Some(DERATE_KEY.to_owned()),
                property: None,
                disabled: false,
            },
            tags: Tags::new().with(Tags::TEST_ID, DERATE_ID),
        },
        headed(
            "strata.crafting.record.makes",
            &record.makes,
            RECORD_MAKES_ID,
        ),
        text_in("strata.crafting.record.note", TextRole::Caption, NOTE_ROLE),
    ];
    children.extend(record.needs.iter().map(|need| {
        panel(
            "invisible",
            Layout {
                width: Some(Length::Percent(100.)),
                align: Some(Align::Start),
                ..row(1.2)
            },
            vec![
                head(need.label.to_uppercase()),
                grows(
                    0.,
                    vec![with_id(
                        UiNodeDef::Text {
                            key: key(need.asks.clone()),
                            style: TextRole::Caption,
                            opts: TextOpts {
                                role: Some(Role::new(STAT_ROLE)),
                                align: TextAlign::Right,
                                ..TextOpts::default()
                            },
                            tags: Tags::new(),
                        },
                        &record_need_id(&need.slot),
                    )],
                ),
            ],
        )
    }));
    children.push(headed(
        "strata.crafting.record.runs",
        &record.runs,
        RECORD_RUNS_ID,
    ));
    // The small button of the mockup: the column's own primary stands
    // under the box, and this one is the box's.
    let mut go = act(
        "strata.crafting.record.go",
        RECORD_GO,
        true,
        record.request.is_some(),
    );
    if let UiNodeDef::Button { opts, .. } = &mut go {
        opts.compact = true;
    }
    children.push(panel(
        "invisible",
        Layout {
            wrap: true,
            ..row(1.6)
        },
        vec![go, text_in(record.why.clone(), TextRole::Caption, WHY_ROLE)],
    ));
    with_id(
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
                ..column(0.8)
            },
            children,
        ),
        RECORD_ID,
    )
}

/// The right column with a finished thing: its card, the Record box while
/// it is open, and at the foot Record as blueprint and the way to another.
/// With the box open its own Record is the one there is; a thing that
/// cannot be recorded has the button grey and the reason over it.
fn done_column(done: &DoneStage) -> Vec<UiNodeDef> {
    let mut right = vec![head("strata.crafting.out"), done.card.node(Some(CARD_ID))];
    right.extend(done.record.as_ref().map(record_box));
    right.push(spacer());
    let mut foot = Vec::new();
    if done.record.is_none() {
        right.push(with_id(
            text_in(done.cannot_record.clone(), TextRole::Caption, WHY_ROLE),
            RECORD_WHY_ID,
        ));
        foot.push(act(
            "strata.crafting.record.open",
            RECORD_OPEN,
            false,
            done.cannot_record.is_empty(),
        ));
    }
    foot.push(act("strata.crafting.again", AGAIN, true, true));
    right.push(panel("invisible", row(1.6), foot));
    right
}

/// The head: the workbench's two tabs, its title, who is at it, and at the
/// right the household's credits.
fn head_row(view: &CraftingView) -> UiNodeDef {
    panel(
        "invisible",
        Layout {
            align: Some(Align::Center),
            ..row(2.)
        },
        vec![
            bench_tabs(WorkbenchTab::Crafting),
            with_id(
                UiNodeDef::Text {
                    key: key("strata.crafting.title"),
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
            with_id(head(view.worker.clone()), WORKER_ID),
            spacer(),
            super::stores::purse(PURSE_ID, view.purse),
        ],
    )
}

/// The foot: what the sim last said, or what the screen is for, then the
/// keys and Close.
fn footer(view: &CraftingView) -> UiNodeDef {
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
                tags: Tags::new().with(Tags::TEST_ID, "crafting.hints"),
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

/// The three columns: the schematics, the slots or what went in, and what
/// comes out, the bench hand or the finished card.
fn body(view: &CraftingView) -> Vec<UiNodeDef> {
    let (middle, right) = match &view.stage {
        Some(Stage::Pick(pick)) => (
            std::iter::once(head(view.product.to_uppercase()))
                .chain(pick.slots.iter().map(slot_box))
                .collect(),
            out_column(pick),
        ),
        Some(Stage::Hand(hand)) => (went_column(&hand.went), hand_column(hand)),
        Some(Stage::Done(done)) => (went_column(&done.went), done_column(done)),
        None => (Vec::new(), Vec::new()),
    };
    let sized = |width: f32, children: Vec<UiNodeDef>| {
        panel(
            "invisible",
            Layout {
                width: Some(Length::Px(width)),
                align: Some(Align::Stretch),
                ..column(1.)
            },
            children,
        )
    };
    vec![
        sized(SCHEMATICS_WIDTH, schematics_column(view)),
        panel(
            "invisible",
            Layout {
                grow: 1.,
                min_width: Some(Length::Px(0.)),
                align: Some(Align::Stretch),
                ..column(1.2)
            },
            middle,
        ),
        sized(OUTCOME_WIDTH, right),
    ]
}

/// The whole screen for `view`.
#[must_use]
pub fn screen_def(view: &CraftingView) -> ScreenDef {
    ScreenDef {
        kind: kind(),
        inherits: None,
        root: UiNodeDef::Panel {
            role: Role::new("panel"),
            layout: Layout {
                width: Some(Length::Px(WIDTH)),
                max_width: Some(Length::Percent(96.)),
                // One frame for the three states, so the panel does not
                // jump as a craft goes from the slots to the hand to the
                // card.
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
                panel(
                    "invisible",
                    Layout {
                        align: Some(Align::Stretch),
                        grow: 1.,
                        min_height: Some(Length::Px(0.)),
                        overflow: Overflow::Scroll,
                        ..Layout {
                            direction: LayoutDirection::Row,
                            gap: 2.6,
                            ..Layout::default()
                        }
                    },
                    body(view),
                ),
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

// ---- the shape a stack is drawn by ----

fn shape_key(shape: IconShape) -> &'static str {
    match shape {
        IconShape::Cube => "cube",
        IconShape::Slab => "slab",
        IconShape::Ingot => "ingot",
        IconShape::Gem => "gem",
        IconShape::Rod => "rod",
        IconShape::Sphere => "sphere",
    }
}

/// `strata:stock_swatch`: an item's stand-in shape, flat, in its item's
/// colour. A list's row has no slot for slotted to bake an icon into, and
/// the fill is the item's and no role of the theme's, so it is painted
/// here; the hairline round it is chrome, and is the theme's
/// (`strata.crafting.swatch`, read once at the spawn as the card's picture
/// tint is).
#[derive(Debug, Default, Clone, Copy)]
pub struct SwatchWidget;

impl Widget for SwatchWidget {
    fn spawn(&self, ctx: &mut SpawnCtx<'_>, params: &Value, _children: &[UiNodeDef]) -> Entity {
        let model = slotted::registry::to_model(params).ok();
        let field = |name: &str| match &model {
            Some(Model::Map(map)) => map.get(name).cloned(),
            _ => None,
        };
        let shape = match field("shape") {
            Some(Model::Str(shape)) => shape,
            _ => String::new(),
        };
        let channels: Vec<u8> = match field("tint") {
            Some(Model::List(list)) => list
                .iter()
                .filter_map(|channel| match channel {
                    Model::Int(channel) => u8::try_from(*channel).ok(),
                    _ => None,
                })
                .collect(),
            _ => Vec::new(),
        };
        let image = match field("image") {
            Some(Model::Str(path)) if !path.is_empty() => Some(path),
            _ => None,
        };
        let tinted = matches!(field("tinted"), Some(Model::Bool(true)));
        // Parameters that name no colour draw the outline alone.
        let tint = match channels[..] {
            [red, green, blue] => bevy::color::Color::srgb_u8(red, green, blue),
            _ => bevy::color::Color::NONE,
        };
        // The item's image, where the one rule has one: no fill and no
        // outline, the picture is the swatch.
        if let Some(handle) = image.and_then(|path| {
            ctx.world
                .get_resource::<bevy::asset::AssetServer>()
                .map(|assets| assets.load::<bevy::image::Image>(path))
        }) {
            return ctx.spawn_node((
                Node {
                    width: Val::Px(ICON + 4.),
                    height: Val::Px(ICON + 4.),
                    flex_shrink: 0.,
                    ..Node::default()
                },
                bevy::ui::widget::ImageNode {
                    color: if tinted {
                        tint
                    } else {
                        bevy::color::Color::WHITE
                    },
                    ..bevy::ui::widget::ImageNode::new(handle)
                },
                bevy::picking::Pickable::IGNORE,
                slotted::ui::WidgetNode(WidgetKind::new(SWATCH_WIDGET)),
            ));
        }
        let edge = ctx
            .world
            .get_resource::<ActiveTheme>()
            .zip(ctx.world.get_resource::<bevy::asset::Assets<Theme>>())
            .and_then(|(active, themes)| themes.get(&active.0))
            .and_then(|theme| Paint::for_role(theme, &Role::new_static(SWATCH_ROLE)))
            .and_then(|paint| paint.border)
            .unwrap_or(bevy::color::Color::NONE);
        // Width, height and corner in UI units, and whether it stands on a
        // point: the six stand-in shapes as they read flat.
        let (wide, tall, corner, turned) = match shape.as_str() {
            "slab" => (ICON + 4., ICON * 0.5, 2., false),
            "ingot" => (ICON + 4., ICON * 0.62, 4., false),
            "gem" => (ICON * 0.82, ICON * 0.82, 2., true),
            "rod" => (ICON * 0.36, ICON + 4., 2., false),
            "sphere" => (ICON, ICON, ICON, false),
            _ => (ICON, ICON, 3., false),
        };
        let root = ctx.spawn_node((
            Node {
                width: Val::Px(wide),
                height: Val::Px(tall),
                flex_shrink: 0.,
                border: UiRect::all(Val::Px(1.)),
                border_radius: BorderRadius::all(Val::Px(corner)),
                ..Node::default()
            },
            BackgroundColor(tint),
            BorderColor::all(edge),
            bevy::picking::Pickable::IGNORE,
            slotted::ui::WidgetNode(WidgetKind::new(SWATCH_WIDGET)),
        ));
        if turned {
            ctx.world
                .entity_mut(root)
                .insert(UiTransform::from_rotation(bevy::math::Rot2::degrees(45.)));
        }
        root
    }
}

/// On one bar of a hand's odds: the chance it stands for, in percent.
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq)]
pub struct OddsBar(pub u8);

/// `strata:roll_odds`: the five bars of a hand's odds. Each is a bare node
/// of an exact height, where a panel's frame would make a chance of nought
/// as tall as a chance of five in a hundred; its colour is its role's and
/// its place in the row is what says which result it is.
#[derive(Debug, Default, Clone, Copy)]
pub struct OddsWidget;

impl Widget for OddsWidget {
    fn spawn(&self, ctx: &mut SpawnCtx<'_>, params: &Value, _children: &[UiNodeDef]) -> Entity {
        let model = slotted::registry::to_model(params).ok();
        let field = |name: &str| match &model {
            Some(Model::Map(map)) => map.get(name).cloned(),
            _ => None,
        };
        let odds: Vec<u8> = match field("odds") {
            Some(Model::List(list)) => list
                .iter()
                .filter_map(|chance| match chance {
                    Model::Int(chance) => u8::try_from(*chance).ok(),
                    _ => None,
                })
                .collect(),
            _ => Vec::new(),
        };
        let id = match field("id") {
            Some(Model::Str(id)) => id,
            _ => String::new(),
        };
        let root = ctx.spawn_node((
            Node {
                width: Val::Px(ODDS_WIDTH),
                min_width: Val::Px(ODDS_WIDTH),
                height: Val::Px(ODDS_HEIGHT),
                column_gap: Val::Px(2.),
                align_items: bevy::ui::AlignItems::FlexEnd,
                flex_shrink: 0.,
                ..Node::default()
            },
            bevy::picking::Pickable::IGNORE,
            slotted::ui::WidgetNode(WidgetKind::new(ODDS_WIDGET)),
        ));
        for (outcome, chance) in Outcome::ALL.into_iter().zip(odds) {
            let at = outcome.index();
            // A chance of nought keeps a hairline, so the row always reads
            // as five.
            let tall = (f32::from(chance) * ODDS_PER_PERCENT).max(1.);
            ctx.world.spawn((
                Node {
                    width: Val::Px(ODDS_BAR),
                    height: Val::Px(tall),
                    flex_shrink: 0.,
                    ..Node::default()
                },
                slotted::theme::Themed(Role::new_static(ODDS_ROLES[at])),
                TestId(format!("{id}:{at}")),
                OddsBar(chance),
                bevy::picking::Pickable::IGNORE,
                ChildOf(root),
            ));
        }
        root
    }
}

/// `Startup`: the swatch and the odds, under their kinds.
fn register_widgets(mut widgets: ResMut<WidgetRegistry>) {
    widgets.register(WidgetKind::new(SWATCH_WIDGET), SwatchWidget);
    widgets.register(WidgetKind::new(ODDS_WIDGET), OddsWidget);
}

// ---- open, answer, rebuild ----

/// Whether the crafting screen is the one on top.
pub(super) fn on_top(stack: &ScreenStack) -> bool {
    stack.top().is_some_and(|top| top.kind == kind())
}

/// What the crafting screen reads: the data, the sim's own bench rule, the
/// local player's stock, weapons, name and sheet, the store and the purse,
/// the household's crafting ledger, what is in reach, what the card is read
/// against, and what the device in use calls the tab keys (the pad's
/// vendor, the input mode, the glyph set, the bindings). The card's
/// weapons are kept behind a shared handle that is written without a
/// change mark, so they count by value.
#[derive(SystemParam)]
pub struct CraftingInputs<'w, 's> {
    registries: Option<Res<'w, Registries>>,
    reach: Option<Res<'w, Reach>>,
    access: Option<Res<'w, WorkbenchAccess>>,
    players: Query<
        'w,
        's,
        (
            Ref<'static, Pack>,
            Ref<'static, Hands>,
            Option<Ref<'static, Combatant>>,
            Option<Ref<'static, Name>>,
            Option<Ref<'static, CharacterSheet>>,
        ),
        With<LocalPlayer>,
    >,
    store: Option<Res<'w, HouseholdStore>>,
    purse: Option<Res<'w, Purse>>,
    craft: Option<Res<'w, CraftState>>,
    context: Option<Res<'w, card::CardContext>>,
    gamepads: Query<'w, 's, &'static Gamepad>,
    mode: Option<Res<'w, InputMode>>,
    glyphs: Option<Res<'w, GlyphSet>>,
    bindings: Option<Res<'w, UiBindings>>,
    against: Local<'s, Seen<Option<Against>>>,
    vendor: Local<'s, Seen<Option<u16>>>,
    drawn: Local<'s, Seen<crate::drawn::Drawn>>,
}

impl ChangeSource for CraftingInputs<'static, 'static> {
    fn changed(inputs: &mut CraftingInputs<'_, '_>) -> bool {
        let against = inputs
            .context
            .as_deref()
            .and_then(card::CardContext::against);
        let against = inputs.against.differs(against);
        let drawn = inputs
            .players
            .single()
            .ok()
            .and_then(|(_, _, actor, ..)| actor.as_deref().map(crate::drawn::drawn))
            .is_some_and(|drawn| inputs.drawn.differs(drawn));
        let now = inputs.vendor_id();
        let vendor = inputs.vendor.differs(now);
        let player = inputs
            .players
            .single()
            .is_ok_and(|(pack, hands, actor, name, sheet)| {
                pack.is_changed()
                    || hands.is_changed()
                    || actor.is_some_and(|actor| actor.is_added())
                    || name.is_some_and(|name| name.is_changed())
                    || sheet.is_some_and(|sheet| sheet.is_changed())
            });
        against
            || drawn
            || vendor
            || player
            || moved(&inputs.registries)
            || moved(&inputs.reach)
            || moved(&inputs.access)
            || moved(&inputs.store)
            || moved(&inputs.purse)
            || moved(&inputs.craft)
            || moved(&inputs.mode)
            || moved(&inputs.glyphs)
            || moved(&inputs.bindings)
    }
}

impl CraftingInputs<'_, '_> {
    fn vendor_id(&self) -> Option<u16> {
        self.gamepads.iter().next().and_then(Gamepad::vendor_id)
    }

    /// What the device in use calls the inputs bound to the tab before and
    /// the tab after, as slotted's hint bar would name them: the bindings are
    /// the player's to change, so no sentence names a key of its own.
    fn tab_keys(&self) -> [String; 2] {
        let mode = self.mode.as_deref().copied().unwrap_or_default();
        let set = resolved_glyph_set_for_vendor(
            self.glyphs.as_deref().copied().unwrap_or_default(),
            self.vendor_id(),
        );
        [UiAction::TabPrev, UiAction::TabNext].map(|action| {
            self.bindings.as_deref().map_or_else(
                || action.as_str().to_owned(),
                |bindings| key_glyph_text(action, mode, bindings, set),
            )
        })
    }

    /// Reads the world as a [`Bench`] and gives it to `read`: the local
    /// player's stock and weapons, the store, the purse, the crafting ledger
    /// and the reach. `None` without a player or the registries.
    fn with_bench<R>(&self, read: impl FnOnce(&Bench<'_>) -> R) -> Option<R> {
        let registries = &self.registries.as_deref()?.0;
        let tab_keys = self.tab_keys();
        // The sim's own rule, so the buttons are grey exactly when a request
        // would be refused for it.
        let at_workbench = self.reach.as_deref().is_some_and(|reach| {
            let access = self.access.as_deref().copied();
            fit_rules::at_workbench(access.unwrap_or_default(), reach)
        });
        let (pack, hands, actor, name, sheet) = self.players.single().ok()?;
        let holder = Holder::of(&registries.items, &hands, actor.as_deref());
        let (empty, idle) = (HouseholdStore::default(), CraftState::default());
        let untaught = CharacterSheet::default();
        let nowhere = Reach::default();
        let against = self.context.as_deref().and_then(card::CardContext::against);
        let bench = Bench {
            registries,
            pack: &pack,
            store: self.store.as_deref().unwrap_or(&empty),
            purse: self.purse.as_deref().copied().unwrap_or_default(),
            holder: &holder,
            craft: self.craft.as_deref().unwrap_or(&idle),
            at_workbench,
            worker: name.as_deref().map_or("", Name::as_str),
            sheet: sheet.as_deref().unwrap_or(&untaught),
            standing: self.reach.as_deref().unwrap_or(&nowhere),
            against: against.as_ref(),
            tab_keys: &tab_keys,
        };
        Some(read(&bench))
    }
}

impl SimScreen for CraftingState {
    type View = CraftingView;
    type Inputs = CraftingInputs<'static, 'static>;
    type Answering = (
        Option<Res<'static, Registries>>,
        ResMut<'static, CraftInbox>,
        ResMut<'static, ValueStore>,
    );
    const KIND: &'static str = KIND;

    fn view(inputs: &CraftingInputs<'_, '_>, state: &Self) -> Option<CraftingView> {
        inputs.with_bench(|bench| CraftingView::of(bench, state))
    }

    fn def(view: &CraftingView, _: &Self) -> ScreenDef {
        screen_def(view)
    }

    fn shown(state: &Self) -> Option<&CraftingView> {
        state.view.as_ref()
    }

    fn marked(state: &Self) -> bool {
        state.dirty
    }

    /// A fill of the best stock that a press asked for is made first, by
    /// the sim's own choice.
    fn settle(inputs: &CraftingInputs<'_, '_>, state: &mut Self) {
        let Some(filled) = inputs.with_bench(|bench| {
            if !(state.refill || state.fill_empty) || bench.craft.session.is_some() {
                return None;
            }
            let best = crafting::best_stock(
                bench.registries,
                bench.pack,
                bench.reach(),
                shown_of(bench, state)?,
            );
            Some(if state.refill {
                best
            } else {
                let mut picks = state.picks.clone();
                for (slot, kind) in best {
                    picks.entry(slot).or_insert(kind);
                }
                picks
            })
        }) else {
            return;
        };
        if let Some(picks) = filled {
            state.picks = picks;
        }
        state.refill = false;
        state.fill_empty = false;
        // A fill that has landed is done with: the bar is built at its
        // figure from here on.
        if state.fill.as_ref().is_some_and(|fill| fill.landed) {
            state.fill = None;
        }
    }

    fn built(state: &mut Self, view: CraftingView) {
        // The schematic the view resolved is written back, so a press acts on
        // the one that is shown.
        state.schematic.clone_from(&view.shown);
        state.dirty = false;
        state.view = Some(view);
    }

    fn answer(
        state: &mut Self,
        choice: &MenuChoice,
        with: &mut SystemParamItem<'_, '_, Self::Answering>,
    ) {
        let (registries, inbox, store) = with;
        press(state, choice, registries.as_deref(), inbox, store);
    }
}

/// Shows `def` as the workbench's screen: in place of whichever of its two
/// tabs is on top, or pushed when neither is.
fn show(world: &mut World, def: ScreenDef) {
    let stack = world.resource::<ScreenStack>();
    if on_top(stack) || fitting::on_top(stack) {
        replace_screen(&mut world.commands(), Arc::new(def), None);
    } else {
        push_screen(&mut world.commands(), Arc::new(def), None);
    }
}

/// Opens the crafting tab on `schematic`, or on the one last shown when
/// none is named, with the best stock in its slots; a session on the bench
/// is shown whatever is asked for. The tab strip, a workbench's F with
/// something on the bench and the flag for a still all come through here.
pub fn open(commands: &mut Commands, schematic: Option<Namespaced>) {
    commands.queue(move |world: &mut World| {
        *world.resource_mut::<WorkbenchTab>() = WorkbenchTab::Crafting;
        {
            let mut state = world.resource_mut::<CraftingState>();
            if schematic.is_some() {
                state.schematic = schematic;
            }
            state.refill = true;
            state.fill_empty = false;
            state.open = None;
            state.done = None;
            state.last = None;
            state.notice = None;
            state.asked = 0;
            state.fill = None;
            state.holding = None;
            state.pressed = None;
            state.still = None;
            state.recording = None;
        }
        let Some(def) = sim_screen::build::<CraftingState>(world) else {
            tracing::warn!("no local player with a pack: nothing to craft with");
            return;
        };
        show(world, def);
    });
}

/// Sets what the list shows, for a still: the flag's choice, before the
/// screen is opened.
pub fn set_filter(commands: &mut Commands, filter: ListFilter) {
    commands.queue(move |world: &mut World| {
        world.resource_mut::<CraftingState>().filter = filter;
    });
}

/// What a workbench's F opens: the tab last shown, and the crafting tab
/// whenever something is on the bench, so a session left open is what the
/// player comes back to.
pub fn open_bench(commands: &mut Commands) {
    commands.queue(|world: &mut World| {
        let session = world
            .get_resource::<CraftState>()
            .is_some_and(|craft| craft.session.is_some());
        let tab = *world.resource::<WorkbenchTab>();
        // The list's chip is kept while the screen is up (a tab switch comes
        // through `open`), and a fresh visit to the bench starts on Known.
        world.resource_mut::<CraftingState>().filter = ListFilter::Known;
        let mut commands = world.commands();
        if session || tab == WorkbenchTab::Crafting {
            open(&mut commands, None);
        } else {
            fitting::open(&mut commands, None);
        }
    });
}

/// The schematic a short name stands for: `cell_core` is the one whose id
/// ends so, and a full id is itself.
#[must_use]
pub fn schematic_named(registries: &GameRegistries, name: &str) -> Option<Namespaced> {
    let schematics = &registries.schematics;
    Namespaced::parse(name)
        .ok()
        .filter(|id| schematics.get(id).is_some())
        .or_else(|| {
            schematics
                .iter()
                .map(|schematic| &schematic.def.id)
                .find(|id| id.path() == name || id.path().rsplit('.').next() == Some(name))
                .cloned()
        })
}

/// [`open`] on the schematic `name` stands for, then brought to `still` as
/// a player's presses would bring it: for a still of one of the screen's
/// states.
pub fn open_for_still(commands: &mut Commands, name: &str, still: Still) {
    let name = name.to_owned();
    commands.queue(move |world: &mut World| {
        let schematic = world
            .get_resource::<Registries>()
            .and_then(|registries| schematic_named(&registries.0, &name));
        if schematic.is_none() {
            tracing::warn!("no schematic `{name}`: the screen opens on the first");
        }
        open(&mut world.commands(), schematic);
        world.flush();
        world.resource_mut::<CraftingState>().still = Some(still);
    });
}

/// Puts what `request` asks in the sim's inbox, unless a request is still
/// waiting for its answer: a second Accept before the tick would spend a
/// second point the player did not see the first land for.
fn ask(inbox: &mut CraftInbox, asked: &mut usize, request: Option<CraftRequest>) -> bool {
    match request {
        Some(request) if *asked == 0 => {
            inbox.push(request);
            *asked += 1;
            true
        }
        _ => false,
    }
}

/// `Update`: a schematic, a slot, a candidate, Best stock, Assemble, a
/// hand, Finish, Buy and Make another. Nothing is decided here: a pick is
/// the screen's own, and the four that change anything send the request
/// the view worked out for the sim to judge.
#[allow(
    clippy::too_many_lines,
    reason = "every control of the screen, an arm each, in the order the columns show them"
)]
fn press(
    state: &mut CraftingState,
    choice: &MenuChoice,
    registries: Option<&Registries>,
    inbox: &mut CraftInbox,
    store: &mut ValueStore,
) {
    let id = choice.id.as_str();
    let Some(view) = state.view.as_ref() else {
        return;
    };
    let pick = match &view.stage {
        Some(Stage::Pick(pick)) => Some(pick),
        _ => None,
    };
    let slot = |of: &str| pick.and_then(|pick| pick.slots.iter().find(|slot| slot.id == of));
    // Where the focus goes once the press has been built: the control
    // itself, unless the press leads on.
    let mut next = id.to_owned();
    if let Some(chip) = id.strip_prefix(FILTER_TAG) {
        let Some(filter) = ListFilter::of_key(chip) else {
            return;
        };
        state.filter = filter;
        // The schematic shown may be one the new chip hides.
        state.refill = true;
    } else if let Some(path) = id.strip_prefix(SCHEM_TAG) {
        let Some(row) = view
            .groups
            .iter()
            .flat_map(|group| &group.rows)
            .find(|row| row.id.path() == path && row.busy.is_none())
        else {
            return;
        };
        state.schematic = Some(row.id.clone());
        state.refill = true;
        state.open = None;
        state.done = None;
        state.recording = None;
        BEST.clone_into(&mut next);
    } else if id.starts_with(PROTO_TAG) {
        // A thing made earlier: its finished card again, where it is
        // recorded as one just made is.
        let Some(row) = view
            .earlier
            .iter()
            .find(|row| proto_id(row.kind.batch) == id)
        else {
            return;
        };
        state.done = Some(row.kind.clone());
        state.recording = None;
        state.open = None;
        RECORD_OPEN.clone_into(&mut next);
    } else if id == RECORD_OPEN {
        let can = matches!(
            &view.stage,
            Some(Stage::Done(done)) if done.cannot_record.is_empty()
        );
        let top = registries
            .and_then(|registries| registries.0.schematics.factory())
            .map(|rules| rules.derate().1);
        let (true, Some(top)) = (can, top) else {
            return;
        };
        // The box opens at the prototype itself, as the mockup's does.
        state.recording = Some(top);
        store.insert(DERATE_KEY, share_of(top));
    } else if id == RECORD_GO {
        let request = match &view.stage {
            Some(Stage::Done(done)) => done
                .record
                .as_ref()
                .and_then(|record| record.request.clone()),
            _ => None,
        };
        ask(inbox, &mut state.asked, request);
    } else if let Some(of) = id.strip_prefix(OPEN_TAG) {
        if slot(of).is_none() {
            return;
        }
        state.open = (state.open.as_deref() != Some(of)).then(|| of.to_owned());
    } else if let Some((of, at)) = id
        .strip_prefix(PICK_TAG)
        .and_then(|rest| rest.rsplit_once(':'))
    {
        let Some(row) = slot(of)
            .zip(at.parse::<usize>().ok())
            .and_then(|(slot, at)| slot.rows.get(at))
            .filter(|row| row.short.is_none())
        else {
            return;
        };
        state.picks.insert(of.to_owned(), row.kind.clone());
        state.open = None;
        next = open_id(of);
    } else if let Some(of) = id.strip_prefix(BUY_TAG) {
        let request = slot(of)
            .and_then(|slot| slot.buy.as_ref())
            .and_then(|buy| buy.request.clone());
        ask(inbox, &mut state.asked, request);
    } else if id == BEST {
        state.refill = true;
        state.open = None;
        ASSEMBLE.clone_into(&mut next);
    } else if id == ASSEMBLE {
        let request = pick.and_then(|pick| pick.request.clone());
        ask(inbox, &mut state.asked, request);
    } else if let Some(roll) = id.strip_prefix(ROLL_TAG) {
        // The click of a press the pointer already rolled with
        // (`repeat_held`) is that same press let go.
        if state.pressed.as_deref() == Some(id) {
            state.pressed = None;
            return;
        }
        // A hand pressed is a hand held until it is let go: it rolls
        // again as each roll lands (`repeat_held`).
        if ask(inbox, &mut state.asked, view.roll_request(roll)) {
            state.holding = Some(id.to_owned());
        }
    } else if id == FINISH {
        let request = match &view.stage {
            Some(Stage::Hand(hand)) => hand.request.clone(),
            _ => None,
        };
        ask(inbox, &mut state.asked, request);
    } else if id == AGAIN {
        state.done = None;
        state.recording = None;
        state.refill = true;
        ASSEMBLE.clone_into(&mut next);
    } else {
        return;
    }
    state.notice = None;
    state.last = Some(next);
    state.dirty = true;
}

/// A derating in per-mille as the share the slider's key holds.
fn share_of(per_mille: u16) -> f64 {
    f64::from(per_mille) / f64::from(QUALITY_MAX)
}

/// `Update`, before the rebuild: the derating's slider moved. The store's
/// share is held to the dial's steps and kept as the state's per-mille,
/// which the view is read from; nothing is marked for a rebuild, since a
/// rebuild that respawned the slider would cut the drag that moved it.
pub fn follow_derate(
    mut changes: MessageReader<ValueChanged>,
    registries: Option<Res<Registries>>,
    mut state: ResMut<CraftingState>,
) {
    let moved = changes
        .read()
        .filter(|change| change.key == DERATE_KEY)
        .filter_map(|change| change.new.as_f64())
        .last();
    let Some(share) = moved else {
        return;
    };
    let Some(rules) = registries
        .as_ref()
        .and_then(|registries| registries.0.schematics.factory())
    else {
        return;
    };
    let (from, to, step) = rules.derate();
    #[allow(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "a share of nought to one, held to the dial's range first"
    )]
    let asked = (share.clamp(share_of(from), share_of(to)) * f64::from(QUALITY_MAX)).round() as u16;
    // The nearest step from the least, which the slider's own are.
    let steps = (asked.saturating_sub(from) + step / 2) / step.max(1);
    let derate = (from + steps * step).min(to);
    if state.recording.is_some_and(|at| at != derate) {
        state.recording = Some(derate);
    }
}

/// Whether the stock the character carries holds a stack of `kind`: where
/// a thing just made went, for the line that says so.
fn carried(registries: &GameRegistries, pack: &Pack, kind: &Record) -> bool {
    let items = &registries.items;
    let kind = crafting::kind_of(kind);
    std::iter::once(&pack.cells)
        .chain(&pack.bags)
        .flat_map(|inventory| inventory.slots().iter().flatten())
        .filter_map(|stack| items::read(stack, items).ok())
        .any(|record| crafting::kind_of(&record) == kind)
}

/// `Update`: the sim's answer, worded in the footer and, for a refusal,
/// beside the button that asked. An assembly leads the focus to the first
/// hand; a roll starts its bar's fill from the figure before; a finished
/// thing is toasted and its card takes the right column; a purchase is
/// toasted with what the purse has left.
#[allow(
    clippy::too_many_lines,
    reason = "the sim's four answers and its refusal, each worded where it is heard"
)]
pub fn hear(
    mut answers: MessageReader<CraftAnswer>,
    registries: Option<Res<Registries>>,
    players: Query<&Pack, With<LocalPlayer>>,
    mut state: ResMut<CraftingState>,
    mut commands: Commands,
) {
    for answer in answers.read() {
        let state = &mut *state;
        state.asked = state.asked.saturating_sub(1);
        state.dirty = true;
        let items = registries.as_ref().map(|registries| &registries.0.items);
        let notice = match &answer.outcome {
            Ok(CraftDone::Assembled { points }) => {
                // Any hand: the view leads it to the first line that can
                // be rolled.
                state.last = Some(ROLL_TAG.to_owned());
                state.open = None;
                let points = if *points == 1 {
                    words("strata.crafting.points.one").to_owned()
                } else {
                    fill(
                        words("strata.crafting.points.count"),
                        "points",
                        &points.to_string(),
                    )
                };
                (
                    fill(words("strata.crafting.status.assembled"), "points", &points),
                    false,
                )
            }
            Ok(CraftDone::Rolled(rolled)) => {
                state.fill = Some(RollFill {
                    line: rolled.line.clone(),
                    from: rolled.from,
                    to: rolled.to,
                    elapsed: 0.,
                    landed: false,
                });
                let line = state
                    .view
                    .as_ref()
                    .and_then(|view| view.hand_line(&rolled.line))
                    .map_or_else(|| rolled.line.clone(), |line| line.name.clone());
                let said = [
                    ("outcome", outcome_word(rolled.outcome).to_owned()),
                    ("line", line),
                    ("from", rolled.from.to_string()),
                    ("to", rolled.to.to_string()),
                ]
                .iter()
                .fold(
                    words("strata.crafting.status.rolled").to_owned(),
                    |said, (name, with)| fill(&said, name, with),
                );
                (said, false)
            }
            Ok(CraftDone::Finished { product }) => {
                let name = items
                    .and_then(|items| items.get(&product.item))
                    .map_or_else(|| product.item.to_string(), |def| def.name.clone());
                let in_pack = registries
                    .as_ref()
                    .zip(players.single().ok())
                    .is_none_or(|(registries, pack)| carried(&registries.0, pack, product));
                let said = fill(
                    &fill(
                        words(if in_pack {
                            "strata.crafting.status.finished"
                        } else {
                            "strata.crafting.status.finished.store"
                        }),
                        "name",
                        &name,
                    ),
                    "batch",
                    &product.batch.to_string(),
                );
                super::progress::notice(&mut commands, said.clone());
                state.last = state.schematic.as_ref().map(schem_id);
                state.done = Some(product.clone());
                state.fill = None;
                state.holding = None;
                (said, false)
            }
            Ok(CraftDone::Bought { item, purse, .. }) => {
                let name = items
                    .and_then(|items| card::a_name(items, item))
                    .unwrap_or_else(|| item.to_string());
                let said = fill(
                    &fill(words("strata.crafting.status.bought"), "name", &name),
                    "left",
                    &credits(*purse),
                );
                super::progress::notice(&mut commands, said.clone());
                // What was bought goes into the slot it was bought for.
                state.fill_empty = true;
                (said, false)
            }
            Ok(CraftDone::Recorded { blueprint, .. }) => {
                let at = match &answer.request {
                    CraftRequest::Record { derate, .. } => *derate,
                    _ => state.recording.unwrap_or(QUALITY_MAX),
                };
                super::progress::notice(
                    &mut commands,
                    fill(
                        words("strata.crafting.record.done"),
                        "percent",
                        &card::percent_of(at).to_string(),
                    ),
                );
                // The box shuts; what stands in its place says the thing is
                // recorded, and the way on is Make another.
                state.recording = None;
                state.last = Some(AGAIN.to_owned());
                (
                    fill(
                        words("strata.crafting.status.recorded"),
                        "blueprint",
                        &blueprint.to_string(),
                    ),
                    false,
                )
            }
            Err(refusal) => {
                state.holding = None;
                // The slot by the name its box has, as the view's own
                // reason beside Assemble says it.
                let shown = registries
                    .as_ref()
                    .zip(state.schematic.as_ref())
                    .and_then(|(registries, id)| registries.0.schematics.get(id));
                let slot_name = |key: &str| {
                    shown
                        .and_then(|schematic| schematic.slot(key))
                        .map_or_else(|| key.to_owned(), |(_, slot)| slot.name.clone())
                };
                let empty = SkillGraph::default();
                let graph = registries.as_ref().map_or(&empty, |r| &r.0.progression);
                (refusal_words(refusal, graph, slot_name), true)
            }
        };
        tracing::info!("crafting: {}", notice.0);
        state.notice = Some(notice);
    }
}

/// `Update`, after the rebuild: a roll's bar fills from the figure before
/// to the figure after over [`ROLL_FILL_SECS`], eased so it lands softly.
/// Written to the fill's node in place: a rebuild a frame would be a
/// flicker and no motion. The word the roll said is there at once; only
/// the bar takes its time.
pub fn animate_roll(
    stack: Res<ScreenStack>,
    time: Res<Time>,
    mut state: ResMut<CraftingState>,
    mut fills: Query<(&mut Node, &ChildOf), With<BarFill>>,
    parents: Query<&ChildOf>,
    ids: Query<&TestId>,
) {
    if !on_top(&stack) {
        return;
    }
    let Some(fill) = state.fill.as_ref().filter(|fill| !fill.landed) else {
        return;
    };
    let elapsed = fill.elapsed + time.delta_secs();
    let through = (elapsed / ROLL_FILL_SECS).clamp(0., 1.);
    let eased = 1. - (1. - through).powi(3);
    let (from, to) = (f32::from(fill.from), f32::from(fill.to));
    // A figure of nought to a thousand is a tenth of it in percent.
    let width = Val::Percent((from + (to - from) * eased) / 10.);
    let bar = format!("{BAR_TAG}{}", fill.line);
    let mut written = false;
    for (mut node, track) in &mut fills {
        // The fill is the track's child and the track the bar's.
        let on_bar = parents
            .get(track.parent())
            .ok()
            .and_then(|root| ids.get(root.parent()).ok())
            .is_some_and(|id| id.0 == bar);
        if on_bar {
            if node.width != width {
                node.width = width;
            }
            written = true;
        }
    }
    let state = state.bypass_change_detection();
    if let Some(fill) = state.fill.as_mut() {
        fill.elapsed = elapsed;
        // A bar the rebuild has not spawned yet is waited for, so the fill
        // is never left short of where it lands.
        fill.landed = written && through >= 1.;
    }
}

/// `Update`, after the fill: a hand that was pressed and is still held
/// rolls again once its last roll has been answered and its bar has
/// landed. slotted repeats only the directions, so the repeat is the
/// screen's: a pointer's primary button down over the button, or an Accept
/// key or button down with the focus on it. A hand held from before its
/// press (Accept kept down through Assemble) is not a hold: the press comes
/// first.
///
/// A key's press is slotted's `Activate` and comes through `answer`. A
/// pointer's would come only when it is let go, which is no start for a
/// hold, so a pointer going down on a hand rolls here and at once, and the
/// click its release makes is swallowed there.
#[allow(
    clippy::too_many_arguments,
    reason = "the screen, the three devices that can hold a button, the focus and the buttons"
)]
pub fn repeat_held(
    stack: Res<ScreenStack>,
    bindings: Res<UiBindings>,
    keys: Option<Res<ButtonInput<KeyCode>>>,
    pointers: Query<&PointerPress>,
    gamepads: Query<&Gamepad>,
    focus: Option<Res<InputFocus>>,
    buttons: Query<(Entity, &TestId, &ButtonState, Option<&Hovered>)>,
    mut state: ResMut<CraftingState>,
    mut inbox: ResMut<CraftInbox>,
) {
    let pointer = pointers.iter().any(PointerPress::is_primary_pressed);
    if state.pressed.is_some() && !pointer {
        state.pressed = None;
    }
    if !on_top(&stack) {
        if state.holding.is_some() {
            state.holding = None;
        }
        return;
    }
    // The pointer just down on a hand: its roll, and the start of its hold.
    if pointer && state.pressed.is_none() && state.holding.is_none() {
        let down = buttons
            .iter()
            .find(|(_, id, button, _)| button.pressed && id.0.starts_with(ROLL_TAG))
            .map(|(_, id, ..)| id.0.clone());
        if let Some(id) = down {
            let request = state
                .view
                .as_ref()
                .zip(id.strip_prefix(ROLL_TAG))
                .and_then(|(view, roll)| view.roll_request(roll));
            let state = &mut *state;
            if ask(&mut inbox, &mut state.asked, request) {
                state.notice = None;
                state.last = Some(id.clone());
                state.holding = Some(id.clone());
                state.pressed = Some(id);
            }
            return;
        }
    }
    let Some(held) = state.holding.clone() else {
        return;
    };
    let accept = bindings
        .keys
        .get(&UiAction::Accept)
        .zip(keys.as_deref())
        .is_some_and(|(codes, keys)| codes.iter().any(|code| keys.pressed(*code)))
        || bindings
            .buttons
            .get(&UiAction::Accept)
            .is_some_and(|bound| {
                gamepads
                    .iter()
                    .any(|pad| bound.iter().any(|button| pad.pressed(*button)))
            });
    if !(accept || pointer) {
        state.holding = None;
        return;
    }
    // The button is a new node after every rebuild, and may not be there
    // yet: the hold is then judged when it is.
    let Some((entity, _, button, hovered)) = buttons.iter().find(|(_, id, ..)| id.0 == held) else {
        return;
    };
    let focused = focus.as_deref().and_then(InputFocus::get) == Some(entity);
    // The node the pointer went down on is gone with the roll's rebuild,
    // and the one in its place was never pressed: the pointer still down
    // and over it is the hold.
    let on_it =
        button.pressed || (accept && focused) || (pointer && hovered.is_some_and(Hovered::get));
    if !on_it {
        // Held, but on something else: the roll's own rebuild moves the
        // focus a frame after the node, so only a hand that can no longer
        // be rolled ends the hold.
        if button.disabled {
            state.holding = None;
        }
        return;
    }
    let landed = state.fill.as_ref().is_none_or(|fill| fill.landed);
    if state.asked > 0 || !landed || state.dirty {
        return;
    }
    let request = state
        .view
        .as_ref()
        .zip(held.strip_prefix(ROLL_TAG))
        .and_then(|(view, roll)| view.roll_request(roll));
    let state = &mut *state;
    if !ask(&mut inbox, &mut state.asked, request) {
        state.holding = None;
    }
}

/// `Update`, in slotted's input set: a tab of the strip, or the tab keys
/// from anywhere on either screen, shows the workbench's other tab. A
/// press on the tab already shown does nothing.
pub fn switch_tabs(
    mut choices: MessageReader<MenuChoice>,
    mut actions: MessageReader<UiActionEvent>,
    stack: Res<ScreenStack>,
    fitted: Res<fitting::FittingState>,
    mut claims: ResMut<UiActionClaims>,
    mut commands: Commands,
) {
    let shown = if on_top(&stack) {
        Some(WorkbenchTab::Crafting)
    } else if fitting::on_top(&stack) {
        Some(WorkbenchTab::Fitting)
    } else {
        None
    };
    let Some(shown) = shown else {
        choices.clear();
        actions.clear();
        return;
    };
    let mut wanted = None;
    for choice in choices.read() {
        if let Some(tab) = choice
            .id
            .strip_prefix(TAB_TAG)
            .and_then(|id| WorkbenchTab::ALL.into_iter().find(|tab| tab.id() == id))
        {
            wanted = Some(tab);
        }
    }
    for event in actions.read() {
        let tabbed = matches!(event.action, UiAction::TabPrev | UiAction::TabNext);
        if tabbed && !event.repeat && !claims.is_claimed(event.action) {
            claims.claim(event.action);
            // Two tabs: either key shows the other.
            wanted = Some(shown.other());
        }
    }
    match wanted.filter(|tab| *tab != shown) {
        Some(WorkbenchTab::Crafting) => open(&mut commands, None),
        // Back on the weapon and the mount it was showing.
        Some(WorkbenchTab::Fitting) => {
            fitting::open_at(&mut commands, Some(fitted.hand), fitted.mount.clone());
        }
        None => {}
    }
}

/// `Update`, before the choices are read: brings the screen to what a
/// still asked for by sending what a player's presses would send, one
/// request at a time, each waited on by its answer.
pub fn drive_still(
    stack: Res<ScreenStack>,
    craft: Option<Res<CraftState>>,
    mut state: ResMut<CraftingState>,
    mut inbox: ResMut<CraftInbox>,
    mut store: ResMut<ValueStore>,
) {
    let Some(still) = state.still else {
        return;
    };
    if !on_top(&stack) || state.asked > 0 || state.dirty {
        return;
    }
    let state = &mut *state;
    let session = craft.is_some_and(|craft| craft.session.is_some());
    let Some(view) = state.view.as_ref() else {
        return;
    };
    let assemble = match &view.stage {
        Some(Stage::Pick(pick)) => pick.request.clone(),
        _ => None,
    };
    match (still, &view.stage) {
        (Still::Pick, _) | (Still::Done, Some(Stage::Done(_))) => state.still = None,
        // The finished card is there: the box opens on it at what was
        // asked for, as Record as blueprint and the slider would bring it.
        (Still::Record(derate), Some(Stage::Done(done))) => {
            if done.cannot_record.is_empty() {
                state.recording = Some(derate);
                store.insert(DERATE_KEY, share_of(derate));
                state.last = Some(DERATE_ID.to_owned());
                state.dirty = true;
            } else {
                tracing::warn!("a still of the Record box: {}", done.cannot_record);
            }
            state.still = None;
        }
        (Still::Empty, _) => {
            state.picks.clear();
            state.dirty = true;
            state.still = None;
        }
        (Still::Hand, _) if session => state.still = None,
        (Still::Done | Still::Record(_), Some(Stage::Hand(hand))) => {
            // Each point steady on the first line that can still be
            // worked, once the roll before has landed, then Finish.
            if state.fill.as_ref().is_some_and(|fill| !fill.landed) {
                return;
            }
            let roll = hand
                .lines
                .iter()
                .find_map(|line| line.steady.request.clone());
            ask(
                &mut inbox,
                &mut state.asked,
                roll.or_else(|| hand.request.clone()),
            );
        }
        (Still::Hand | Still::Done | Still::Record(_), _) => {
            if !ask(&mut inbox, &mut state.asked, assemble) {
                tracing::warn!("a still of the bench hand: the stock cannot be assembled");
                state.still = None;
            }
        }
    }
}

/// Registers the state, the widget and the systems. Only called once the
/// UI is wired.
pub(crate) fn register(app: &mut bevy::app::App) {
    app.add_sim_screen::<CraftingState>()
        .init_resource::<WorkbenchTab>()
        .add_systems(bevy::app::Startup, register_widgets)
        .add_systems(
            bevy::app::Update,
            (
                switch_tabs.in_set(SlottedUiSet::Input).after(UiActionEmit),
                (
                    // A still's presses come before the screen's own, the
                    // sim's answers and the slider's value before the
                    // comparison they feed.
                    drive_still.before(ScreenStep::answer::<CraftingState>()),
                    (
                        hear.after(veyra_sim::order::HouseholdTracked),
                        follow_derate,
                    )
                        .chain()
                        .after(ScreenStep::answer::<CraftingState>())
                        .before(ScreenStep::rebuild::<CraftingState>()),
                    (animate_roll.run_if(super::screen_on_top(kind)), repeat_held)
                        .chain()
                        .after(ScreenStep::rebuild::<CraftingState>()),
                )
                    .in_set(super::ScreenWork),
            ),
        );
}

#[cfg(test)]
mod tests {
    #![allow(
        clippy::similar_names,
        reason = "a test's state and the stage it is in are both what they are"
    )]

    use super::*;
    use std::collections::{BTreeMap, BTreeSet};

    use strata_registry::item::LotOrigin;
    use strata_registry::progression::XpKind;
    use veyra_sim::items::Stock;

    use crate::testing::shipped;

    const CORE: &str = "veyra:schematic.cell_core";
    const FEED: &str = "veyra:schematic.patrol_extended_feed";
    const SALT: &str = "strata:evaporite_salt";
    const BRINE: &str = "strata:coastal_brine";
    const ALLOY: &str = "strata:light_alloy_ore";

    fn id(name: &str) -> Namespaced {
        Namespaced::parse(name).expect("an id")
    }

    /// A lot of `resource` drawn at `root`, with `figures` in its class's
    /// order.
    fn lot(
        registries: &GameRegistries,
        resource: &str,
        root: &str,
        units: u32,
        figures: &[u16],
    ) -> Record {
        let def = registries.items.lot_of(&id(resource)).expect("a lot item");
        assert_eq!(def.lines.len(), figures.len(), "{resource}");
        let cell = i32::try_from(root.bytes().map(u32::from).sum::<u32>()).expect("small");
        Record {
            item: def.id.clone(),
            count: units,
            batch: 0,
            by: None,
            lines: def
                .lines
                .iter()
                .zip(figures)
                .map(|(line, figure)| (line.id.clone(), *figure))
                .collect(),
            condition: None,
            fitted: BTreeMap::new(),
            origin: Some(LotOrigin {
                deposit: [cell, 0],
                root: root.to_owned(),
                place: format!("{root} flats"),
            }),
        }
    }

    /// The first day at the default seed, without a world: the shipped
    /// registries, the arrival's pack and weapons, and the store of
    /// `starting.ron` with the three lots the game draws for it.
    struct Day {
        registries: GameRegistries,
        pack: Pack,
        hands: Hands,
        store: HouseholdStore,
        holder: Holder,
        craft: CraftState,
        sheet: CharacterSheet,
        purse: Purse,
        at_workbench: bool,
        standing: Reach,
        tab_keys: [String; 2],
    }

    fn day() -> Day {
        let registries = shipped();
        let items = &registries.items;
        let starting = items.starting().expect("the first day's stock");
        let arrival = &registries
            .sheets
            .household
            .as_ref()
            .expect("the household's sheets")
            .arrival;
        let pack = Pack::from_saved(&starting.packs[&arrival.name], items).expect("a pack");
        let hands = Hands::from_saved(&starting.hands.clone().map(Some), items).expect("hands");
        let lots = [
            lot(&registries, SALT, "Tolley", 160, &[389, 632, 225]),
            lot(&registries, BRINE, "Oxbow", 160, &[534, 605, 495]),
            lot(
                &registries,
                ALLOY,
                "Rake",
                200,
                &[533, 298, 312, 231, 451, 633, 165, 492],
            ),
        ];
        let store: Vec<Option<Record>> = starting
            .store
            .iter()
            .chain(&lots)
            .cloned()
            .map(Some)
            .collect();
        let store = HouseholdStore::from_saved(&store, items).expect("a store");
        let holder = Holder::of(items, &hands, None);
        let purse = Purse(starting.purse);
        // Taught everything that teaches without adding points, so the
        // tests that are not about who may see the points the dials give.
        let sheet = CharacterSheet {
            learned: registries
                .progression
                .boxes()
                .filter(|b| b.effects.is_empty())
                .map(|b| b.id.clone())
                .collect(),
            ..CharacterSheet::default()
        };
        Day {
            registries,
            pack,
            hands,
            store,
            holder,
            craft: CraftState::default(),
            sheet,
            purse,
            at_workbench: true,
            standing: Reach {
                workbench: true,
                ..Reach::default()
            },
            tab_keys: ["Q".to_owned(), "E".to_owned()],
        }
    }

    impl Day {
        fn bench(&self) -> Bench<'_> {
            Bench {
                registries: &self.registries,
                pack: &self.pack,
                store: &self.store,
                purse: self.purse,
                holder: &self.holder,
                craft: &self.craft,
                at_workbench: self.at_workbench,
                worker: "Frontier ranger",
                sheet: &self.sheet,
                standing: &self.standing,
                against: None,
                tab_keys: &self.tab_keys,
            }
        }

        /// The state a screen opened on `schematic` would have: the best
        /// stock in its slots.
        fn state(&self, schematic: &str) -> CraftingState {
            let found = self
                .registries
                .schematics
                .get(&id(schematic))
                .expect("a schematic");
            let bench = self.bench();
            CraftingState {
                schematic: Some(id(schematic)),
                picks: crafting::best_stock(&self.registries, &self.pack, bench.reach(), found),
                ..CraftingState::default()
            }
        }

        fn view(&self, state: &CraftingState) -> CraftingView {
            CraftingView::of(&self.bench(), state)
        }

        /// Assembles what `state` has picked, as the tick would.
        fn assemble(&mut self, state: &CraftingState) {
            let mut stock = Stock {
                saddle: None,
                pack: &mut self.pack,
                hands: &mut self.hands,
                store: Some(&mut self.store),
            };
            crafting::assemble(
                &self.registries,
                &mut stock,
                &mut self.craft,
                state.schematic.as_ref().expect("a schematic"),
                &state.picks,
                "Frontier ranger",
                &self.sheet,
            )
            .expect("the picks assemble");
        }
    }

    fn pick(view: &CraftingView) -> &PickStage {
        match &view.stage {
            Some(Stage::Pick(pick)) => pick,
            other => panic!("not picking: {other:?}"),
        }
    }

    fn hand(view: &CraftingView) -> &HandStage {
        match &view.stage {
            Some(Stage::Hand(hand)) => hand,
            other => panic!("nothing on the bench: {other:?}"),
        }
    }

    /// Every schematic lists in six groups with their counts, the taught
    /// ones first in each, and every untaught one says the box that teaches
    /// it.
    #[test]
    fn the_list_of_all_puts_the_taught_first_in_six_groups() {
        let mut day = day();
        day.sheet = CharacterSheet {
            learned: ["fabricator.materials.1".to_owned()].into(),
            ..CharacterSheet::default()
        };
        let all = CraftingState {
            filter: ListFilter::All,
            ..CraftingState::default()
        };
        let view = day.view(&all);
        let heads: Vec<&str> = view.groups.iter().map(|g| g.head.as_str()).collect();
        assert_eq!(
            heads,
            [
                "WEAPONS",
                "GUN PARTS",
                "BLADE PARTS",
                "COMPONENTS",
                "TOOLS AND VEHICLES",
                "KITS"
            ]
        );
        assert_eq!(view.groups.iter().map(|g| g.rows.len()).sum::<usize>(), 31);
        assert!(view.groups.iter().all(|g| g.count == g.rows.len()));
        let group = |head: &str| {
            view.groups
                .iter()
                .find(|g| g.head == head)
                .unwrap_or_else(|| panic!("no group {head}"))
        };
        let components: Vec<(&str, bool)> = group("COMPONENTS")
            .rows
            .iter()
            .map(|row| (row.name.as_str(), row.untaught.is_none()))
            .collect();
        assert_eq!(
            components[..3],
            [("Cell core", true), ("Drive coil", true), ("Harness", true)]
        );
        assert!(
            components[3..].iter().all(|(_, taught)| !taught),
            "{components:?}"
        );
        for group in &view.groups {
            let taught: Vec<bool> = group.rows.iter().map(|r| r.untaught.is_none()).collect();
            assert!(
                taught.windows(2).all(|pair| pair[0] || !pair[1]),
                "{}: taught ones first, {taught:?}",
                group.head
            );
        }
        let plate = group("COMPONENTS")
            .rows
            .iter()
            .find(|row| row.id.path().ends_with("hull_plate"))
            .expect("the hull plate");
        assert_eq!(
            plate.untaught.as_deref(),
            Some("Needs Fabricator: Materials 2.")
        );
        // The shown schematic is the first one taught, never a grey one.
        assert!(
            view.groups
                .iter()
                .flat_map(|g| &g.rows)
                .any(|row| row.on && row.untaught.is_none())
        );
        let tools = &group("TOOLS AND VEHICLES").rows;
        assert_eq!(tools[0].name, "Survey tool");
        assert!(tools[0].untaught.is_none());
        assert!(tools[1].untaught.is_some(), "the bike waits on Materials 3");
        // A blade's part is under BLADE PARTS and a gun's under GUN PARTS.
        let has = |head: &str, path: &str| {
            group(head)
                .rows
                .iter()
                .any(|row| row.id.path().ends_with(path))
        };
        assert!(has("BLADE PARTS", "blade_hooked_edge"));
        assert!(has("GUN PARTS", "patrol_extended_feed"));
    }

    /// The list opens on what the member was taught, and Ready narrows it to
    /// what the stock can make; a group with nothing in it is not drawn, and
    /// a row is one line with no reason under it.
    #[test]
    fn the_list_opens_on_known_and_ready_narrows_it() {
        let mut day = day();
        day.sheet = CharacterSheet {
            learned: ["fabricator.materials.1".to_owned()].into(),
            ..CharacterSheet::default()
        };
        let known = day.view(&CraftingState::default());
        assert_eq!(known.filter, ListFilter::Known);
        assert!(
            known
                .groups
                .iter()
                .flat_map(|g| &g.rows)
                .all(|row| row.untaught.is_none())
        );
        let all = day.view(&CraftingState {
            filter: ListFilter::All,
            ..CraftingState::default()
        });
        let count = |view: &CraftingView| view.groups.iter().map(|g| g.count).sum::<usize>();
        assert!(count(&known) > 0 && count(&known) < count(&all));
        assert!(known.groups.len() < all.groups.len(), "empty groups drop");
        let ready = day.view(&CraftingState {
            filter: ListFilter::Ready,
            ..CraftingState::default()
        });
        assert!(
            ready
                .groups
                .iter()
                .flat_map(|g| &g.rows)
                .all(|row| row.can && row.untaught.is_none())
        );
        assert!(count(&ready) <= count(&known));
        // Nobody taught anything lists nothing under Known.
        day.sheet = CharacterSheet::default();
        let none = day.view(&CraftingState::default());
        assert!(
            none.groups
                .iter()
                .flat_map(|g| &g.rows)
                .all(|row| row.untaught.is_none())
        );
        let def = schematics_column(&none);
        assert_eq!(def.len(), 3, "the legend, the chips and the list");
    }

    /// A smith with Edges 1 sees the blade and the hooked edge taught and
    /// the other melee schematics grey, each with the box that teaches it.
    #[test]
    fn the_melee_schematics_list_under_edges_with_their_greyed_reasons() {
        let mut day = day();
        day.sheet = CharacterSheet {
            learned: ["weaponsmith.edges.1".to_owned()].into(),
            ..CharacterSheet::default()
        };
        let view = day.view(&CraftingState {
            filter: ListFilter::All,
            ..CraftingState::default()
        });
        let row = |path: &str| {
            view.groups
                .iter()
                .flat_map(|group| &group.rows)
                .find(|row| row.id.path().ends_with(path))
                .unwrap_or_else(|| panic!("no schematic {path}"))
        };
        assert_eq!(row("service_blade").untaught, None);
        assert_eq!(row("blade_hooked_edge").untaught, None);
        let grip = row("blade_balanced_grip").untaught.as_deref();
        assert!(
            grip.is_some_and(|why| why.starts_with("Needs ")),
            "{grip:?}"
        );
        assert!(row("yard_greatblade").untaught.is_some());
        assert!(row("pier_glaive").untaught.is_some());
    }

    /// The first day's lots read the design's figures on the screen: each
    /// slot's best stock with the figures the slot weighs, each line's
    /// floor and ceiling, and what the core would give in the magazine it
    /// goes into, in rounds.
    #[test]
    fn the_first_days_core_reads_its_floor_its_ceiling_and_its_rounds() {
        let day = day();
        let state = day.state(CORE);
        let view = day.view(&state);
        assert_eq!(view.product, "Cell core");
        assert_eq!(view.worker, "WORKED BY FRONTIER RANGER");
        assert_eq!(view.purse, 2500);
        let heads: Vec<&str> = view
            .groups
            .iter()
            .map(|group| group.head.as_str())
            .collect();
        assert_eq!(
            heads,
            [
                "WEAPONS",
                "GUN PARTS",
                "BLADE PARTS",
                "COMPONENTS",
                "TOOLS AND VEHICLES",
                "KITS"
            ]
        );
        let core = &view.groups[3].rows[0];
        assert!(core.on && core.can && core.busy.is_none(), "{core:?}");
        let feed = view.groups[1]
            .rows
            .iter()
            .find(|row| row.mount == "FEED")
            .expect("the feed");
        assert_eq!(
            (feed.name.as_str(), feed.mount.as_str(), feed.can),
            ("Patrol extended magazine", "FEED", false),
            "no core in the stock yet"
        );
        let stage = pick(&view);
        let salt = &stage.slots[0];
        assert_eq!(salt.label, "REACTIVE");
        assert_eq!(salt.wants, "any reactive · 20 units");
        let picked = salt.picked.as_ref().expect("the best salt");
        assert_eq!(picked.name, "Tolley salt");
        assert_eq!(picked.held, "160 u");
        assert_eq!(
            picked
                .stats
                .iter()
                .map(|stat| (stat.name.as_str(), stat.figure.as_str()))
                .collect::<Vec<_>>(),
            [("Potency", "632"), ("Stability", "225")]
        );
        let brine = stage.slots[1].picked.as_ref().expect("the best brine");
        assert_eq!(brine.name, "Oxbow brine");
        assert_eq!(
            brine
                .stats
                .iter()
                .map(|stat| (stat.name.as_str(), stat.figure.as_str()))
                .collect::<Vec<_>>(),
            [
                ("Potency", "605"),
                ("Conductivity", "495"),
                ("Purity", "534")
            ]
        );
        let density = &stage.lines[0];
        assert_eq!(density.name, "Charge density");
        assert_eq!(density.bar, Some((358, 597)));
        assert_eq!(density.range, "Assembles at 358, can be worked up to 597.");
        assert_eq!(
            density.downstream,
            "In a patrol extended magazine: 27 to 29 rounds"
        );
        assert_eq!(density.means, "", "a part's line has no unit of its own");
        let stability = &stage.lines[1];
        assert_eq!(stability.bar, Some((209, 349)));
        assert_eq!(stability.downstream, "Goes into nothing yet");
        assert_eq!(
            stage.uses,
            "Uses 20 units of Tolley salt, 30 units of Oxbow brine."
        );
        assert!(stage.request.is_some() && stage.why.is_empty());
        assert_eq!(view.focus, Some(schem_id(&id(CORE))));
    }

    /// A pick moves what comes out before anything is spent, an emptied
    /// slot says what is missing at its line and at Assemble, and a stack
    /// too small for its slot is listed with its reason.
    #[test]
    fn a_pick_moves_the_lines_and_an_empty_slot_says_so() {
        let mut day = day();
        // A second, better salt, and too little of it.
        let calder = lot(&day.registries, SALT, "Calder", 12, &[640, 820, 450]);
        day.store.0.set(
            20,
            Some(items::write(&calder, &day.registries.items).expect("a stack")),
        );
        let mut state = day.state(CORE);
        state.open = Some("salt".to_owned());
        let view = day.view(&state);
        let salt = &pick(&view).slots[0];
        assert!(salt.open);
        assert_eq!(salt.count, "Close");
        assert_eq!(salt.rows[0].name, "Calder salt");
        assert!(salt.rows[0].best && !salt.rows[0].on);
        assert_eq!(
            salt.rows[0].short.as_deref(),
            Some("12 units, needs 20"),
            "the best is listed where its figures put it, with why it will not do"
        );
        assert!(salt.rows[1].on && salt.rows[1].short.is_none());
        assert_eq!(
            salt.picked.as_ref().map(|row| row.name.as_str()),
            Some("Tolley salt"),
            "the best stock is the best there is enough of"
        );
        // With more of it, picking it moves the ceiling.
        let mut plenty = calder.clone();
        plenty.count = 80;
        day.store.0.set(
            20,
            Some(items::write(&plenty, &day.registries.items).expect("a stack")),
        );
        let before = pick(&day.view(&state)).lines[0].bar;
        state
            .picks
            .insert("salt".to_owned(), crafting::kind_of(&plenty));
        let view = day.view(&state);
        let after = pick(&view).lines[0].bar;
        assert_eq!(before, Some((358, 597)));
        assert_eq!(
            after,
            Some((415, 691)),
            "0.5 of 820, 0.3 of 605, 0.2 of 495"
        );
        assert!(day.craft.session.is_none(), "nothing is spent by looking");
        // An emptied slot.
        state.picks.remove("brine");
        let view = day.view(&state);
        let stage = pick(&view);
        assert!(stage.slots[1].picked.is_none());
        assert_eq!(stage.lines[0].bar, None);
        assert_eq!(stage.lines[0].range, "Fill the slots to see this.");
        assert!(stage.request.is_none());
        assert_eq!(stage.why, "Brine: nothing is picked.");
    }

    /// The magazine's slots: a made part with its batch, a bought one, a
    /// casing whose density is the better the lower, and a flat seal set
    /// that weighs nothing. Its lines read in the carbine's own units.
    #[test]
    fn the_magazine_reads_in_rounds_and_seconds() {
        let mut day = day();
        let core = Record {
            item: id("veyra:component.cell_core"),
            count: 1,
            batch: 1,
            by: Some("Frontier ranger".to_owned()),
            lines: [("charge_density", 597), ("stability", 349)]
                .map(|(line, figure)| (line.to_owned(), figure))
                .into(),
            condition: None,
            fitted: BTreeMap::new(),
            origin: None,
        };
        day.pack.cells.set(
            0,
            Some(items::write(&core, &day.registries.items).expect("a stack")),
        );
        let state = day.state(FEED);
        let view = day.view(&state);
        let stage = pick(&view);
        let labels: Vec<&str> = stage.slots.iter().map(|slot| slot.label.as_str()).collect();
        assert_eq!(labels, ["CELL CORE", "HARNESS", "CASING", "SEALS"]);
        let made = stage.slots[0].picked.as_ref().expect("the core");
        assert_eq!(
            (made.name.as_str(), made.batch.as_str(), made.held.as_str()),
            ("Cell core", "batch 1", "1")
        );
        assert_eq!(stage.slots[0].wants, "one cell core");
        assert_eq!(made.stats[0].name, "Charge density");
        assert_eq!(stage.slots[2].wants, "any light alloy ore · 40 units");
        let casing = stage.slots[2].picked.as_ref().expect("the alloy");
        // What the lines weigh, then the one figure that moves the odds.
        assert_eq!(
            casing
                .stats
                .iter()
                .map(|stat| (
                    stat.name.as_str(),
                    stat.figure.as_str(),
                    stat.lighter,
                    stat.odds
                ))
                .collect::<Vec<_>>(),
            [
                ("Purity", "533", false, false),
                ("Density", "165", true, false),
                ("Strength", "298", false, false),
                ("Workability", "492", false, true)
            ]
        );
        assert!(made.stats.iter().all(|stat| !stat.odds));
        let seals = stage.slots[3].picked.as_ref().expect("a seal set");
        assert!(seals.stats.is_empty(), "bought in, nothing to weigh");
        // One of each, and each already in its slot: nothing to list.
        assert_eq!(stage.slots[0].count, "");
        assert_eq!(stage.slots[3].count, "");
        assert!(!stage.workable, "the first day's alloy is no help");
        let capacity = &stage.lines[0];
        assert_eq!(capacity.bar, Some((338, 563)));
        assert_eq!(capacity.means, "28 to 29 rounds");
        assert_eq!(capacity.downstream, "");
        assert!(!capacity.later);
        let handling = &stage.lines[1];
        assert_eq!(handling.bar, Some((372, 620)));
        assert_eq!(handling.means, "1.50 to 1.47 s");
        assert_eq!(
            stage.uses,
            "Uses 1 cell core, 1 harness, 40 units of Rake light alloy, 1 seal set."
        );
    }

    /// A list's row and a slot's picked stack draw the image the pack's cell
    /// draws for the item (the one rule), a lot in its class's symbol and
    /// colour, and none of them falls back to the stand-in shape.
    #[test]
    fn the_lists_draw_the_image_the_pack_does() {
        let day = day();
        let state = day.state(FEED);
        let view = day.view(&state);
        let stage = pick(&view);
        let mut seen = 0;
        for slot in &stage.slots {
            for row in slot.rows.iter().chain(slot.picked.iter()) {
                let swatch = row.swatch.as_ref().expect("the catalogue knows the kind");
                let def = day.registries.items.get(&row.kind.item).expect("an item");
                let art = card::item_art(def, None, None, &day.registries.icons)
                    .unwrap_or_else(|| panic!("{} has no image", def.id));
                assert_eq!(
                    swatch.image.as_deref(),
                    Some(art.path.as_str()),
                    "{}",
                    def.id
                );
                assert_eq!(swatch.tinted, art.tint.is_some(), "{}", def.id);
                seen += 1;
            }
        }
        assert!(seen >= 4, "the slots and their lists were walked: {seen}");
        let casing = stage.slots[2].picked.as_ref().expect("the alloy");
        let swatch = casing.swatch.as_ref().expect("a swatch");
        let lot = day.registries.items.get(&casing.kind.item).expect("a lot");
        assert!(swatch.tinted, "a lot takes its class's colour");
        assert_eq!(
            swatch.image.as_deref(),
            lot.icon
                .symbol
                .as_deref()
                .and_then(|id| day.registries.icons.file(id, card::ICON_PX)),
            "its class's symbol"
        );
    }

    /// Workable stock moves the odds of the whole session, so the screen
    /// says it of the picks before anything is spent: the casing's row
    /// shows the figure and what it does, and the sentence under what
    /// Assemble would use comes and goes with the pick. Assembled, the
    /// bench hand says the same and its bars are the better row.
    #[test]
    fn workable_stock_is_said_before_it_is_spent() {
        let mut day = day();
        let core = Record {
            item: id("veyra:component.cell_core"),
            count: 1,
            batch: 1,
            by: Some("Frontier ranger".to_owned()),
            lines: [("charge_density", 597), ("stability", 349)]
                .map(|(line, figure)| (line.to_owned(), figure))
                .into(),
            condition: None,
            fitted: BTreeMap::new(),
            origin: None,
        };
        day.pack.cells.set(
            0,
            Some(items::write(&core, &day.registries.items).expect("a stack")),
        );
        let harrow = lot(
            &day.registries,
            ALLOY,
            "Harrow",
            200,
            &[610, 540, 560, 410, 520, 700, 320, 760],
        );
        day.store.0.set(
            20,
            Some(items::write(&harrow, &day.registries.items).expect("a stack")),
        );
        let mut state = day.state(FEED);
        let casing = |view: &CraftingView, name: &str| -> CandidateRow {
            pick(view).slots[2]
                .rows
                .iter()
                .find(|row| row.name == name)
                .expect("listed")
                .clone()
        };
        let odds = |row: &CandidateRow| -> Vec<(String, String)> {
            row.stats
                .iter()
                .filter(|stat| stat.odds)
                .map(|stat| (stat.name.clone(), stat.figure.clone()))
                .collect()
        };
        let view = day.view(&state);
        let (rake, workable) = (
            casing(&view, "Rake light alloy"),
            casing(&view, "Harrow light alloy"),
        );
        assert_eq!(odds(&rake), [("Workability".to_owned(), "492".to_owned())]);
        assert_eq!(
            odds(&workable),
            [("Workability".to_owned(), "760".to_owned())]
        );
        state.picks.insert("casing".to_owned(), rake.kind.clone());
        assert!(!pick(&day.view(&state)).workable);
        state
            .picks
            .insert("casing".to_owned(), workable.kind.clone());
        let view = day.view(&state);
        assert!(pick(&view).workable, "said before Assemble");
        assert!(day.craft.session.is_none(), "and nothing is spent");
        day.assemble(&state);
        let view = day.view(&state);
        let stage = hand(&view);
        assert!(stage.workable);
        assert_eq!(stage.lines[0].steady.odds, [15, 65, 20, 0, 0]);
    }

    /// The footer names the keys that change the tab by what the device in
    /// use calls them, not by a name of the screen's own: the bindings are
    /// the player's to change.
    #[test]
    fn the_footer_names_the_tab_keys_as_they_are_bound() {
        let mut day = day();
        let state = day.state(CORE);
        assert_eq!(
            day.view(&state).status.0,
            "At the workbench: pick a schematic, fill its slots, assemble. Q and E change the tab."
        );
        day.tab_keys = ["LB".to_owned(), "RB".to_owned()];
        assert_eq!(
            day.view(&state).status.0,
            "At the workbench: pick a schematic, fill its slots, assemble. LB and RB change the tab."
        );
        assert!(
            english("strata.crafting.status.bench").is_some_and(|said| !said.contains("Q and E"))
        );
    }

    /// Away from a workbench the store is out of reach and every request is
    /// grey with the reason; a slot with nothing that will do offers Buy,
    /// and says why not when the purse or the bench is wanting.
    #[test]
    fn away_from_a_workbench_and_short_of_credits_say_why() {
        let mut day = day();
        // No seal sets left in the store.
        let seals = day
            .store
            .0
            .slots()
            .iter()
            .position(|stack| {
                stack.as_ref().is_some_and(|stack| {
                    day.registries.items.def_of(stack.id).map(|def| &def.kind)
                        == Some(&ItemKind::Commodity)
                })
            })
            .expect("the seal sets");
        day.store.0.set(seals, None);
        let state = day.state(FEED);
        let view = day.view(&state);
        let slot = &pick(&view).slots[3];
        assert_eq!(
            slot.none.as_deref(),
            Some("Nothing in the pack or the store fits")
        );
        let buy = slot.buy.as_ref().expect("a seal set is for sale");
        assert_eq!(buy.label, "Buy 1 · 20 credits");
        assert!(buy.request.is_some() && buy.why.is_empty());
        // A purse that does not cover it.
        day.purse = Purse(8);
        let view = day.view(&state);
        let buy = pick(&view).slots[3].buy.clone().expect("still offered");
        assert!(buy.request.is_none());
        assert_eq!(buy.why, "Needs 20 credits, you have 8");
        // Away from the bench.
        day.purse = Purse(2500);
        day.at_workbench = false;
        let view = day.view(&day.state(CORE));
        let stage = pick(&view);
        assert_eq!(
            stage.slots[0].none.as_deref(),
            Some("Nothing in the pack fits"),
            "the store's lots are not in reach"
        );
        assert!(stage.request.is_none());
        assert_eq!(stage.why, "Step up to a workbench");
        assert_eq!(
            view.status,
            (
                "A reading. Things are made at a workbench.".to_owned(),
                false
            )
        );
        let view = day.view(&day.state(FEED));
        let buy = pick(&view).slots[3].buy.clone().expect("offered, and grey");
        assert!(buy.request.is_none());
        assert_eq!(buy.why, "Step up to a workbench");
    }

    /// The bench hand: the points, each line at its floor of its ceiling,
    /// the two hands with the roll table's rows and what each offers, and
    /// the focus led from Assemble to the first hand, along the lines as
    /// they fill, and to Finish.
    #[test]
    fn the_bench_hand_reads_the_table_and_leads_the_focus() {
        let mut day = day();
        let mut state = day.state(CORE);
        day.assemble(&state);
        state.last = Some(ROLL_TAG.to_owned());
        let view = day.view(&state);
        let stage = hand(&view);
        assert_eq!(stage.points, "3 points left");
        assert!(!stage.workable);
        let density = &stage.lines[0];
        assert_eq!(
            (density.figure, density.ceiling, density.fill),
            (358, 597, 358)
        );
        assert_eq!(density.stands, "358 of 597");
        assert_eq!(density.word, None, "nothing rolled, nothing said");
        assert_eq!(density.steady.odds, [5, 55, 35, 5, 0]);
        assert_eq!(density.steady.words, "most often +60");
        assert_eq!(density.bold.odds, [25, 25, 25, 15, 10]);
        assert_eq!(density.bold.words, "up to +96, can slip 24");
        assert_eq!(
            density.downstream,
            "In a patrol extended magazine: 27 to 28 rounds"
        );
        assert_eq!(
            stage
                .went
                .iter()
                .map(|went| (
                    went.label.as_str(),
                    went.name.as_str(),
                    went.amount.as_str()
                ))
                .collect::<Vec<_>>(),
            [
                ("REACTIVE", "Tolley salt", "20 units"),
                ("BRINE", "Oxbow brine", "30 units")
            ]
        );
        assert_eq!(
            view.focus,
            Some(roll_id("charge_density", Hand::Steady)),
            "Assemble leads to the first hand"
        );
        // The other schematics wait for the bench.
        let feed = &view.groups[1].rows[0];
        assert_eq!(
            feed.busy.as_deref(),
            Some("Finish what is on the bench first")
        );
        // A line at its ceiling passes the focus on; no points, to Finish.
        let rules = day.registries.schematics.rules().clone();
        let session = day.craft.session.as_mut().expect("a session");
        for _ in 0..4 {
            crafting::lift(&rules, &mut session.lines[0], Outcome::Exceptional);
        }
        state.last = Some(roll_id("charge_density", Hand::Bold));
        let view = day.view(&state);
        let stage = hand(&view);
        assert_eq!(stage.lines[0].stands, "597 of 597 · at its ceiling");
        assert_eq!(
            stage.lines[0].word,
            Some(("EXCEPTIONAL +0".to_owned(), Outcome::Exceptional))
        );
        assert!(stage.lines[0].steady.request.is_none());
        assert_eq!(view.focus, Some(roll_id("stability", Hand::Bold)));
        day.craft.session.as_mut().expect("a session").points = 0;
        let view = day.view(&state);
        assert_eq!(hand(&view).why, ("No points left.".to_owned(), false));
        assert_eq!(view.focus.as_deref(), Some(FINISH));
        assert!(hand(&view).request.is_some());
        // Away from the bench every hand and Finish are grey.
        day.craft.session.as_mut().expect("a session").points = 2;
        day.at_workbench = false;
        let view = day.view(&state);
        let stage = hand(&view);
        assert!(stage.request.is_none() && stage.lines[1].bold.request.is_none());
        assert_eq!(stage.why, ("Step up to a workbench".to_owned(), true));
    }

    /// The carbine's own Action line reads in seconds between bursts, from the
    /// table's interval, and is no longer marked as not in the fight.
    #[test]
    fn the_carbines_action_reads_in_seconds_a_shot() {
        let day = day();
        let state = day.state("veyra:schematic.patrol_carbine");
        let view = day.view(&state);
        let action = pick(&view)
            .lines
            .iter()
            .find(|line| line.name == "Action")
            .expect("the Action line");
        assert_eq!(action.means, "0.489 to 0.451 s between bursts");
        assert!(!action.later);
    }

    /// The gate on the carbine says what it needs; where the character
    /// could learn that box, standing here, it says so as well, and not
    /// otherwise.
    #[test]
    fn a_gate_says_when_it_can_be_learned_here() {
        const CARBINE: &str = "veyra:schematic.patrol_carbine";
        let mut day = day();
        day.sheet = CharacterSheet {
            learned: ["weaponsmith.novice".to_owned()].into(),
            ..CharacterSheet::default()
        };
        day.sheet.credit(XpKind::Fabrication, 300);
        // The carbine is untaught here, so only the All chip lists it.
        let mut state = day.state(CARBINE);
        state.filter = ListFilter::All;
        let gate = "Needs Weaponsmith: Frames 1.";
        // At a bench that teaches the tree, with the XP: learnable.
        let view = day.view(&state);
        assert_eq!(
            pick(&view).why,
            format!("{gate} It can be learned at this bench: close this screen and press C.")
        );
        // Away from a teacher the sentence is not there.
        day.standing.workbench = false;
        assert_eq!(pick(&day.view(&state)).why, gate);
        // At the bench without the XP it is not there either.
        day.standing.workbench = true;
        day.sheet.xp.clear();
        assert_eq!(pick(&day.view(&state)).why, gate);
    }

    #[test]
    fn a_refusal_has_words() {
        let said = |refusal: CraftRefusal| {
            refusal_words(&refusal, &SkillGraph::shipped(), str::to_uppercase)
        };
        assert_eq!(said(CraftRefusal::NoWorkbench), "Step up to a workbench");
        assert_eq!(said(CraftRefusal::NoRoom), "No room");
        assert_eq!(said(CraftRefusal::NoPoints), "No points left.");
        assert_eq!(
            said(CraftRefusal::SessionOpen),
            "Finish what is on the bench first"
        );
        assert_eq!(
            said(CraftRefusal::Slot {
                slot: "salt".to_owned(),
                why: SlotRefusal::Short {
                    have: 12,
                    needs: 20
                },
            }),
            "SALT: 12 in stock, needs 20."
        );
        assert_eq!(
            said(CraftRefusal::Short {
                needs: 80,
                have: 1040
            }),
            "Needs 80 credits, you have 1,040"
        );
        assert_eq!(
            said(CraftRefusal::Recorded { blueprint: 4 }),
            "Recorded already as blueprint 4"
        );
        assert_eq!(
            said(CraftRefusal::RunOff { blueprint: 4 }),
            "Run off blueprint 4: a copy is no prototype"
        );
        for refusal in [
            CraftRefusal::NoPlayer,
            CraftRefusal::UnknownSchematic,
            CraftRefusal::NoSession,
            CraftRefusal::AtCeiling,
            CraftRefusal::NotForSale,
            CraftRefusal::NoBlueprints,
            CraftRefusal::NoPrototype,
            CraftRefusal::NoLedger,
            CraftRefusal::Derate { asked: 849 },
        ] {
            assert!(!said(refusal.clone()).starts_with("strata."), "{refusal:?}");
        }
        assert_eq!(signed(-23), "\u{2212}23");
        assert_eq!(signed(0), "+0");
    }

    /// Every role and every key the three states name is in the theme and
    /// has English, and nothing on the bench hand is a percentage.
    #[test]
    fn every_role_and_key_is_in_the_theme() {
        let mut day = day();
        let mut state = day.state(CORE);
        state.open = Some("salt".to_owned());
        let mut defs = vec![screen_def(&day.view(&state))];
        let mut empty = day.state(FEED);
        empty.picks.clear();
        defs.push(screen_def(&day.view(&empty)));
        day.assemble(&state);
        let rules = day.registries.schematics.rules().clone();
        let session = day.craft.session.as_mut().expect("a session");
        crafting::lift(&rules, &mut session.lines[0], Outcome::Setback);
        defs.push(screen_def(&day.view(&state)));
        let mut roles: BTreeSet<String> = ROLES.into_iter().map(str::to_owned).collect();
        roles.extend(WORD_ROLES.map(str::to_owned));
        roles.extend(ODDS_ROLES.map(str::to_owned));
        let mut keys = Vec::new();
        for def in &defs {
            def.root.walk(&mut |node| match node {
                UiNodeDef::Panel { role, .. } => {
                    roles.insert(role.as_str().to_owned());
                }
                UiNodeDef::Text { key, opts, .. } => {
                    keys.push(key.0.clone());
                    if let Some(role) = &opts.role {
                        roles.insert(role.as_str().to_owned());
                    }
                }
                UiNodeDef::Card { role, .. } => {
                    for state in ["", ".hover", ".focus", ".pressed"] {
                        roles.insert(format!("{}{state}", role.as_str()));
                    }
                }
                UiNodeDef::Button { opts, .. } => {
                    keys.extend(opts.label.iter().map(|label| label.0.clone()));
                }
                _ => {}
            });
        }
        let path = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../assets/themes/veyra.theme.ron"
        );
        let theme =
            slotted::theme::Theme::from_ron(&std::fs::read_to_string(path).expect("the theme"))
                .expect("the theme parses");
        for role in roles {
            assert!(
                theme.roles.contains_key(&Role::new(role.clone())),
                "{role} is not in the theme"
            );
        }
        for key in &keys {
            if key.starts_with("strata.") {
                assert!(english(key).is_some(), "{key} has no English");
            }
            assert!(!english(key).unwrap_or(key).contains('%'), "{key}");
        }
        assert!(
            keys.iter().any(|key| key == "SETBACK \u{2212}24"),
            "{keys:?}"
        );
    }

    #[test]
    fn a_short_name_finds_its_schematic() {
        let registries = shipped();
        assert_eq!(schematic_named(&registries, "cell_core"), Some(id(CORE)));
        assert_eq!(schematic_named(&registries, FEED), Some(id(FEED)));
        assert_eq!(schematic_named(&registries, "no_such_thing"), None);
    }

    /// The rifle's Bore line moves the aimed cone, so the tab words it as a
    /// spread and never as a pause between shots.
    #[test]
    fn a_cone_signature_reads_as_a_spread_not_a_pause() {
        let deg = |number| Amount {
            number,
            decimals: 2,
            unit: Unit::Degrees,
        };
        let said = weapon_means_words(WeaponMeans::Row {
            property: Property::AimSpread,
            low: deg(31),
            high: deg(29),
            burst: false,
        });
        assert!(said.ends_with("aimed spread"), "{said}");
        assert!(!said.contains("between"), "{said}");
    }
}
