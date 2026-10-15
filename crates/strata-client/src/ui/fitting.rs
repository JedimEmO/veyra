//! The fitting screen: a weapon on the bench, its mounts, what the stock
//! has for the mount in focus, and what the weapon reads now against what
//! it would read (ticket 0007, step 2; `docs/design/crafting.md`, 4.2 to
//! 4.5).
//!
//! A workbench's F opens it with its buttons live; a weapon's cell on the
//! stores screen opens it from anywhere, and away from a workbench it is a
//! reading whose buttons say why they are grey. Both of the character's
//! weapons are on it, the one in hand and the one holstered, a tab each.
//!
//! No rule lives here and nothing is multiplied. Every word on the screen
//! comes out of the sim's pure functions over the local player's hands and
//! stock: `fitting::mounts` says what each mount holds, `candidates` what
//! the pack and the store have for one, `with` what the weapon would be
//! with a part changed, and `lines` the figures of that against the figures
//! now, already in the game's units. [`FittingView::of`] gathers them into
//! display words and [`screen_def`] lays those out, so what the screen says
//! can be checked without one. Fit and Remove put a `FitRequest` in the
//! sim's inbox for the tick to consume, and the sim's `FitAnswer` is worded
//! at the button and in the footer.
//!
//! The definition is a function of the view and is swapped whole when the
//! view changes ([`rebuild`]), as the sheet's is. One thing is written in
//! place instead: the AFTER column, which follows the part under the focus
//! or the pointer before anything is picked ([`write_preview`]) and is
//! there only while it has something to say. A rebuild for every part the
//! arrows cross would drop the focus that caused it. Local-only: nothing
//! here is replicated.

use std::collections::BTreeSet;
use std::sync::Arc;

use bevy::asset::AssetServer;
use bevy::ecs::prelude::*;
use bevy::input_focus::InputFocus;
use bevy::math::Vec2;
use bevy::picking::hover::Hovered;
use bevy::picking::pointer::{PointerAction, PointerInput};
use bevy::ui::widget::{ImageNode, Text};
use bevy::ui::{Node, Val};
use bevy::window::CursorMoved;
use slotted::menu::MenuChoice;
use slotted::theme::{Role, Themed};
use slotted::ui::def::{
    Align, ButtonOpts, ButtonVariant, Justify, Layout, LayoutDirection, Length, LocKey, Overflow,
    Padding, Place, Presentation, PresentationMode, ScreenDef, ScreenKind, Tags, TextOpts,
    TextRole, Transition, UiNodeDef, WidgetKind,
};
use slotted::ui::{
    InputMode, LocText, NineAnchor, ScreenStack, TestId, push_screen, replace_screen,
};
use strata_registry::GameRegistries;
use strata_registry::item::{
    Fitted, HANDS, ItemCatalogue, ItemKind, MODDABILITY, Property, QUALITY_MAX, Record,
};
use strata_registry::models::ModelSet;
use strata_registry::pictures::PictureSet;
use strata_sim::combat::Combatant;
use strata_sim::crafting::{self as craft_rules, CraftAnswer, CraftInbox, CraftRequest};
use strata_sim::deposits::Purse;
use strata_sim::fitting::{
    self, FitAnswer, FitDone, FitInbox, FitRefusal, FitRequest, MountState, Row, WorkbenchAccess,
};
use strata_sim::items::{Hands, HouseholdStore, Pack};
use strata_sim::progression::{CharacterSheet, Reach};
use strata_sim::rework::{self, ReworkAnswer, ReworkInbox, ReworkRefusal, ReworkRequest};
use strata_sim::wear::{self, Band, RepairAnswer, RepairRefusal, WearInbox, WearRequest};
use strata_sim::{LocalPlayer, Registries};

use super::card::{self, BarParams, CardEffect};
use super::crafting::{self, WorkbenchTab};
use super::strings::{credits, english};
use super::weapon_diagram::{
    self as diagram, DiagramMount, DiagramParams, MountPicked, WeaponPictures,
};
use super::weapon_stage::{ModelParams, WeaponStage};
use crate::weapon_model::{self, WeaponModels};

/// The screen kind.
pub const KIND: &str = "strata:fitting";

/// The `test_id` of the panel the whole screen is.
pub const ROOT_ID: &str = "fitting.root";
/// The `test_id` of the weapon's name in the head.
pub const TITLE_ID: &str = "fitting.title";
/// The `test_id` of the Close button.
pub const CLOSE_ID: &str = "fitting.close";
/// The `test_id` and `menu` tag of the Fit button.
pub const FIT: &str = "fitting.fit";
/// The `test_id` and `menu` tag of the Remove button.
pub const REMOVE: &str = "fitting.remove";
/// The `test_id` and `menu` tag of the well's Model / Diagram switch.
pub const WELL_SWITCH: &str = "fitting.well";
/// The Repair button.
pub const REPAIR: &str = "fitting.repair";
/// The line beside Repair: why it is grey, or what it would do.
pub const REPAIR_WHY_ID: &str = "fitting.repair.why";
/// The Rework button.
pub const REWORK: &str = "fitting.rework";
/// The line under Rework and Buy blank: what Rework would do or why it is
/// grey, then why Buy blank is grey.
pub const REWORK_WHY_ID: &str = "fitting.rework.why";
/// The Buy blank button.
pub const BUY_BLANK: &str = "fitting.rework.buy";
/// The line that says the weapon is worn.
pub const WORN_ID: &str = "fitting.worn";
/// The word beside the condition figure under the bar threshold.
pub const TIRED_ID: &str = "fitting.tired";
/// The condition figure.
pub const CONDITION_ID: &str = "fitting.condition.figure";
/// The `test_id` of the line beside the buttons: why one is grey.
pub const WHY_ID: &str = "fitting.why";
/// The `test_id` of the footer's line.
pub const STATUS_ID: &str = "fitting.status";
/// The `test_id` of the line under it that keeps the keys in sight while
/// the footer's line is the sim's last word.
pub const KEYS_ID: &str = "fitting.keys";
/// The `test_id` of the AFTER column's heading.
pub const AFTER_HEAD_ID: &str = "fitting.after";
/// The `test_id` prefix of the boxes of the AFTER column and of the
/// changes beside it, `fitting.col:after:capacity` and
/// `fitting.col:change:capacity`, which are given their width only while
/// there is something to show in them.
pub const COLUMN_TAG: &str = "fitting.col:";
/// The `test_id` prefix of a part's picture: `fitting.icon:0`.
pub const ICON_TAG: &str = "fitting.icon:";
/// The `test_id` of the words that stand where a weapon with no mounts has
/// none to draw.
const NO_MOUNTS_ID: &str = "fitting.nomounts";
/// The `test_id` of the count of open mounts.
pub const OPEN_ID: &str = "fitting.open";
/// The `test_id` of the scrolling list of what fits the mount.
pub const LIST_ID: &str = "fitting.list";
/// The `test_id` of the heading over the parts.
pub const FITS_ID: &str = "fitting.fits";
/// The `test_id` of the sentence shown when the stock has no part.
pub const NONE_ID: &str = "fitting.none";
/// The `test_id` of the list of what is not in the fight yet.
pub const LATER_ID: &str = "fitting.later";
/// The `test_id` of the moddability bar.
pub const MODDABILITY_ID: &str = "fitting.moddability";
/// The `test_id` of the sentence under it.
pub const THRESHOLDS_ID: &str = "fitting.thresholds";
/// The `menu` tag and `test_id` prefix of a hand's tab: `fitting.hand:1`.
pub const HAND_TAG: &str = "fitting.hand:";
/// The `menu` tag and `test_id` prefix of a mount's row:
/// `fitting.mount:feed`.
pub const MOUNT_TAG: &str = "fitting.mount:";
/// The `menu` tag and `test_id` prefix of a part's row: `fitting.pick:0`,
/// `fitting.pick:standard`, `fitting.pick:fitted`.
pub const PICK_TAG: &str = "fitting.pick:";
/// The `test_id` prefix of a table row, `fitting.row:capacity`, and of its
/// cells: `fitting.row:capacity:now`, `:after`, `:change`, `:bare`.
pub const ROW_TAG: &str = "fitting.row:";

/// The pick id of the part the weapon was built with.
pub const PICK_STANDARD: &str = "standard";
/// The pick id of the module on the mount now.
pub const PICK_FITTED: &str = "fitted";

/// The mount a screen opens on when it has it: the one this step's module
/// is made for.
pub const DEFAULT_MOUNT: &str = "feed";

/// The tallest the drawing grows, in percent of the window's height: the
/// parts and the two buttons stand under it, and at 720 units a list of
/// three must still be in reach.
const DIAGRAM_MAX_VH: f32 = 30.;
/// The least the list of parts is given before the column scrolls instead.
const LIST_MIN_HEIGHT: f32 = 96.;
/// The screen's one height, as a share of the window's, whatever mount or
/// weapon is shown.
const SCREEN_HEIGHT_VH: f32 = 94.;
/// The panel's width, the mounts' column and the table's, in UI units.
const WIDTH: f32 = 1240.;
const MOUNTS_WIDTH: f32 = 250.;
const TABLE_WIDTH: f32 = 392.;
/// The width of a mount's label, so the names line up.
const LABEL_WIDTH: f32 = 62.;
/// The widths of a table row's four figures; its name takes the rest.
const NOW_WIDTH: f32 = 60.;
const AFTER_WIDTH: f32 = 60.;
const CHANGE_WIDTH: f32 = 86.;
const BARE_WIDTH: f32 = 60.;
/// The well a part's picture stands in, and the picture in it.
const ICON_WELL: f32 = 40.;
const ICON: f32 = 32.;

const HEAD_ROLE: &str = "strata.fitting.head";
const BOX_ROLE: &str = "strata.fitting.box";
const MOUNT_ROLE: &str = "strata.fitting.mount";
const MOUNT_ON_ROLE: &str = "strata.fitting.mount.on";
const MOUNT_SHUT_ROLE: &str = "strata.fitting.mount.shut";
const MOUNT_LABEL_ROLE: &str = "strata.fitting.mount.label";
/// The label of the mount in focus, which stands on the row's lit fill.
const MOUNT_LABEL_ON_ROLE: &str = "strata.fitting.mount.label.on";
const MOUNT_NAME_ROLE: &str = "strata.fitting.mount.name";
const MOUNT_NAME_SHUT_ROLE: &str = "strata.fitting.mount.name.shut";
const MOUNT_SUB_ROLE: &str = "strata.fitting.mount.sub";
const PICK_ROLE: &str = "strata.fitting.pick";
const PICK_ON_ROLE: &str = "strata.fitting.pick.on";
const PICK_LEAD_ROLE: &str = "strata.fitting.pick.lead";
const PICK_ICON_ROLE: &str = "strata.fitting.pick.icon";
const CELL_NAME_ROLE: &str = "strata.fitting.cell.name";
const CELL_NOW_ROLE: &str = "strata.fitting.cell.now";
const CELL_AFTER_ROLE: &str = "strata.fitting.cell.after";
const CELL_BARE_ROLE: &str = "strata.fitting.cell.bare";
const BETTER_ROLE: &str = "strata.fitting.better";
const WORSE_ROLE: &str = "strata.fitting.worse";
const STATUS_ROLE: &str = "strata.fitting.status";
const REFUSED_ROLE: &str = "strata.fitting.status.refused";
const WHY_ROLE: &str = "strata.fitting.why";
/// The line beside the buttons while it only says what to do next.
const WHY_HINT_ROLE: &str = "strata.fitting.why.hint";
/// The family Remove paints from, and Fit while it cannot act: its
/// `.disabled` is fainter than the theme's own, so a button that can act
/// is told from one that cannot at a glance.
const ACT_ROLE: &str = "strata.fitting.act";
const FIGURE_ROLE: &str = "strata.fitting.figure";
const TAB_ROLE: &str = "strata.fitting.tab";
const TAB_ON_ROLE: &str = "strata.fitting.tab.on";
const TAB_TEXT_ROLE: &str = "strata.fitting.tab.text";
const TAB_TEXT_ON_ROLE: &str = "strata.fitting.tab.text.on";

/// Every role the screen paints with that its definition does not name at
/// rest, for the test that holds them to the theme.
pub const ROLES: [&str; 9] = [
    MOUNT_LABEL_ON_ROLE,
    MOUNT_ON_ROLE,
    MOUNT_SHUT_ROLE,
    PICK_ON_ROLE,
    TAB_ON_ROLE,
    BETTER_ROLE,
    WORSE_ROLE,
    WHY_ROLE,
    WHY_HINT_ROLE,
];

/// The properties a weapon's table always shows, in its order: the ones
/// the fight has a number for and a firearm always has.
const TABLE: [Property; 5] = [
    Property::Capacity,
    Property::ReloadTime,
    Property::RepeatInterval,
    Property::EquipTime,
    Property::AimMoveSpeed,
];

/// The properties a melee weapon's table always shows: what a cut does,
/// costs and leaves behind, and the draw.
const MELEE_TABLE: [Property; 5] = [
    Property::HitDamage,
    Property::HitPosture,
    Property::AttackStamina,
    Property::RecoveryTime,
    Property::EquipTime,
];

/// The screen kind.
#[must_use]
pub fn kind() -> ScreenKind {
    ScreenKind::new(KIND)
}

/// The `test_id` of hand `hand`'s tab.
#[must_use]
pub fn hand_id(hand: usize) -> String {
    format!("{HAND_TAG}{hand}")
}

/// The `test_id` of the mount `mount`'s row.
#[must_use]
pub fn mount_id(mount: &str) -> String {
    format!("{MOUNT_TAG}{mount}")
}

/// The `test_id` of the part `pick`'s row.
#[must_use]
pub fn pick_id(pick: &str) -> String {
    format!("{PICK_TAG}{pick}")
}

/// The `test_id` of the `cell` (`now`, `after`, `change`, `bare`) of the
/// table row of `property`.
#[must_use]
pub fn cell_id(property: Property, cell: &str) -> String {
    format!("{ROW_TAG}{}:{cell}", property.key())
}

fn words(key: &'static str) -> &'static str {
    english(key).unwrap_or(key)
}

fn fill(words: &str, name: &str, with: &str) -> String {
    words.replace(&format!("{{{name}}}"), with)
}

// ---- the state ----

/// The part picked for the mount in focus: what Fit will act on.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum Pick {
    /// Nothing picked: the mount as it is.
    #[default]
    Current,
    /// The part the weapon was built with, which is a removal when a
    /// module is on the mount.
    Standard,
    /// A module of the stock, named by what it is and not by where it
    /// lies, as a request names it.
    Module(Fitted),
}

/// How the well draws the weapon, for the session: the baked pieces
/// turning on their stage, or the flat diagram. The model is drawn only
/// where there is one ([`WeaponView::model`]); the diagram is always there.
#[derive(Resource, Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum WellView {
    /// The turning model.
    #[default]
    Model,
    /// The flat diagram, whose mounts are buttons.
    Diagram,
}

/// The fitting screen's state: which weapon and mount it shows, what is
/// picked, and what the sim last answered.
#[derive(Resource, Debug, Default)]
pub struct FittingState {
    /// The hand whose weapon is shown.
    pub hand: usize,
    /// The mount in focus; none until the screen has resolved one.
    pub mount: Option<String>,
    /// The part picked for it.
    pub pick: Pick,
    /// The `test_id` of the control last used, which keeps the keyboard's
    /// focus across the rebuild it caused.
    last: Option<String>,
    /// A rebuild is due whatever the view says.
    dirty: bool,
    /// The sim's last answer in words, and whether it was a refusal.
    notice: Option<(String, bool)>,
    /// What the open screen was built from.
    view: Option<FittingView>,
    /// The pick id whose figures the AFTER column last showed.
    previewed: Option<String>,
    /// Whether the pointer has moved since the screen opened. One parked
    /// where a part's row came up under it has looked at nothing.
    pointer_moved: bool,
    /// The requests sent that the sim has not answered yet. While there is
    /// one nothing more is asked: Accept pressed twice before the tick is
    /// one fit, not a fit and a refusal of it.
    asked: usize,
}

impl FittingState {
    /// What the open screen was last built from.
    #[must_use]
    pub fn view(&self) -> Option<&FittingView> {
        self.view.as_ref()
    }
}

// ---- the view ----

/// One hand's tab.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TabView {
    /// The hand.
    pub hand: usize,
    /// `CARBINE · IN HAND`.
    pub label: String,
    /// Whether it is the one shown.
    pub on: bool,
}

/// How a mount's row is drawn.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MountLook {
    /// The part the weapon was built with.
    Standard,
    /// A module.
    Fitted,
    /// Shut on this example of the weapon.
    Shut,
}

/// One mount's row.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MountView {
    /// The mount's key.
    pub key: String,
    /// `SIGHT`.
    pub label: String,
    /// `Standard`, the fitted module's name without the words it shares
    /// with the weapon's, or `Shut`.
    pub name: String,
    /// `The part it was built with`, `Store grade`, `Batch 412`, or
    /// `Opens at moddability 600`.
    pub sub: String,
    /// What it holds.
    pub look: MountLook,
    /// The fitted module's key in the picture index.
    pub part: Option<String>,
    /// Whether it is the mount in focus.
    pub on: bool,
}

/// What the weapon would read with a part on the mount: a figure and its
/// change for each row of the table, and why the part cannot be fitted.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Preview {
    /// For each row of the table: the figure, its change and whether the
    /// change is for the better. Empty figures where nothing would differ
    /// from the mount as it is.
    pub after: Vec<(String, String, Option<bool>)>,
    /// Why Fit would not take this part, in a player's words; empty when
    /// it would.
    pub why: String,
    /// Whether `why` is a reason something cannot be done, as against a
    /// word on what to do next: the first is said in the refusal's colour.
    pub blocked: bool,
}

/// One part that fits the mount in focus.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PickView {
    /// `standard`, `fitted`, or the candidate's place in the list.
    pub id: String,
    /// The part's name.
    pub name: String,
    /// Where it is from and how many there are.
    pub sub: String,
    /// The first thing it does, in units with its change; none for a part
    /// that changes nothing from what is there.
    pub lead: Option<CardEffect>,
    /// Whether it is the one picked.
    pub on: bool,
    /// Its square icon as an asset path, when the pictures are baked.
    pub icon: Option<String>,
    /// The colour its icon is drawn in: its item's own, as in a cell of
    /// the stores.
    pub tint: [u8; 3],
    /// What the weapon would read with it.
    pub preview: Preview,
    /// The module the row stands for; none for the standard part.
    pub module: Option<Fitted>,
    /// What Fit asks the sim for with this picked; none when it would be
    /// refused, for the reason the preview gives.
    pub request: Option<FitRequest>,
}

/// One row of the table.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RowView {
    /// The property.
    pub property: Property,
    /// `Magazine`.
    pub name: String,
    /// The figure as the weapon is.
    pub now: String,
    /// The figure with nothing fitted.
    pub bare: String,
    /// Whether the fight has a number for it.
    pub live: bool,
}

/// The weapon on the bench.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WeaponView {
    /// Its name.
    pub name: String,
    /// `FITTING · VIGIL`.
    pub sub: String,
    /// Its condition.
    pub condition: Option<u16>,
    /// Its chassis's key in the picture index.
    pub chassis: String,
    /// Whether the well draws it as the turning model: the view asks for
    /// one and its pieces are baked. The flat diagram otherwise.
    pub model: bool,
    /// Whether the well can draw it either way: there is a stage and its
    /// pieces are baked. The well's switch is there only then.
    pub switchable: bool,
    /// Whether it is the one drawn.
    pub in_hand: bool,
    /// Its mounts, in its definition's order.
    pub mounts: Vec<MountView>,
    /// The key of the mount in focus.
    pub mount: String,
    /// `4 of 6 open`.
    pub open: String,
    /// `FITS THE FEED, FROM THE PACK AND THE STORE`.
    pub heading: String,
    /// The parts for the mount in focus: the standard one, the one fitted,
    /// then the stock's, best first.
    pub picks: Vec<PickView>,
    /// The sentence shown when the stock has none.
    pub none: Option<String>,
    /// The table's rows: the live ones, then what is not in the fight yet.
    pub rows: Vec<RowView>,
    /// How the weapon fires, fades and reaches: (name, figure), no part
    /// changes them.
    pub facts: Vec<(String, String)>,
    /// What the AFTER column and the line beside the buttons show for what
    /// is picked.
    pub preview: Preview,
    /// What Fit would ask for; none when it is grey.
    pub fit: Option<FitRequest>,
    /// The Fit button's label.
    pub fit_label: &'static str,
    /// What Remove would ask for; none when it is grey.
    pub remove: Option<FitRequest>,
    /// The weapon's moddability and the thresholds its mounts open at.
    pub moddability: Option<(u16, Vec<u16>)>,
    /// The sentence under the bar.
    pub thresholds: String,
    /// How worn it reads, for the word and the line under the header.
    pub band: Band,
    /// What Repair would do, or why it is grey.
    pub repair: RepairView,
    /// Rework and Buy blank; none on a reading, and for a weapon with no
    /// moddability line.
    pub rework: Option<ReworkView>,
}

/// The Rework and Buy blank buttons and the lines beside them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReworkView {
    /// What Rework would ask for; none when it is grey.
    pub request: Option<ReworkRequest>,
    /// `Mount blank, temper 500. ...`, or the refusal in words.
    pub line: String,
    /// `Buy blank · 150 credits`.
    pub buy_label: String,
    /// What Buy blank would ask for; none when it is grey.
    pub buy: Option<CraftRequest>,
    /// The refusal in words when Buy blank is grey.
    pub buy_why: String,
}

impl ReworkView {
    /// Rework for the weapon `record` in `hand`, judged by the sim's own
    /// rules: `check_rework` and `can_buy`.
    fn of(bench: &Bench<'_>, hand: usize, record: &Record) -> Option<Self> {
        let read = bench.rework?;
        let rules = read.registries.rework.as_ref()?;
        if !bench.at_workbench || !record.lines.contains_key(&rules.weapon_line) {
            return None;
        }
        let items = bench.items;
        let hands = Hands::from_saved(bench.hands, items).ok()?;
        let (request, line) = match rework::check_rework(
            read.registries,
            read.access,
            read.reach,
            read.sheet,
            bench.pack,
            &hands,
            Some(bench.store),
            hand,
        ) {
            Ok(outlook) => (
                Some(ReworkRequest::Rework { hand }),
                [
                    ("temper", outlook.temper),
                    ("often", outlook.most_often),
                    ("least", outlook.least),
                    ("most", outlook.most),
                ]
                .into_iter()
                .fold(
                    words("strata.fitting.rework.plan").to_owned(),
                    |text, (name, figure)| fill(&text, name, &figure.to_string()),
                ),
            ),
            Err(refusal) => (None, rework_refusal_words(&refusal)),
        };
        let price = items.get(&rules.blank)?.price?;
        let can = craft_rules::can_buy(
            read.registries,
            bench.pack,
            Some(bench.store),
            &read.purse,
            &rules.blank,
            1,
        );
        Some(Self {
            request,
            line,
            buy_label: fill(words("strata.fitting.rework.buy"), "cost", &credits(price)),
            buy: can.is_ok().then(|| CraftRequest::Buy {
                item: rules.blank.clone(),
                count: 1,
            }),
            buy_why: can.err().map_or_else(String::new, |refusal| {
                crafting::refusal_words(&refusal, &read.registries.progression, str::to_owned)
            }),
        })
    }
}

/// The sim's refusal of a rework, in words.
#[must_use]
pub fn rework_refusal_words(refusal: &ReworkRefusal) -> String {
    match refusal {
        ReworkRefusal::NoBench => words("strata.fitting.refused.no_workbench").to_owned(),
        ReworkRefusal::NotTaught { needs } => {
            fill(words("strata.fitting.rework.needs"), "needs", needs)
        }
        ReworkRefusal::NoWeapon => words("strata.fitting.why.no_weapon").to_owned(),
        ReworkRefusal::Busy => words("strata.fitting.refused.busy").to_owned(),
        ReworkRefusal::NoBlank => words("strata.fitting.rework.no_blank").to_owned(),
        ReworkRefusal::NoGain {
            temper,
            moddability,
        } => fill(
            &fill(
                words("strata.fitting.rework.no_gain"),
                "temper",
                &temper.to_string(),
            ),
            "moddability",
            &moddability.to_string(),
        ),
    }
}

/// The Repair button and the line beside it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RepairView {
    /// `Repair · 1 kit · +220`, or `Repair` when it is grey.
    pub label: String,
    /// What Repair would ask for; none when it is grey.
    pub request: Option<WearRequest>,
    /// The refusal in words when it is grey.
    pub why: String,
}

impl RepairView {
    /// Repair for the weapon in `hand`, judged by the sim's own rule.
    fn of(bench: &Bench<'_>, hand: usize) -> Self {
        let items = bench.items;
        let plan = Hands::from_saved(bench.hands, items)
            .map_err(|_| RepairRefusal::EmptyHand)
            .and_then(|hands| {
                wear::can_repair(
                    items,
                    &hands,
                    bench.pack,
                    Some(bench.store),
                    hand,
                    (
                        bench.at_workbench,
                        bench.rework.map(|rework| wear::Bearer {
                            graph: &rework.registries.progression,
                            sheet: rework.sheet,
                        }),
                    ),
                )
            });
        match plan {
            Ok(plan) => Self {
                label: fill(
                    words(if plan.kit.is_some() {
                        "strata.fitting.repair.plan"
                    } else {
                        "strata.fitting.repair.plan_kitless"
                    }),
                    "gain",
                    &plan.gain.to_string(),
                ),
                request: Some(WearRequest::Repair { hand }),
                why: String::new(),
            },
            Err(refusal) => Self {
                label: words("strata.fitting.repair").to_owned(),
                request: None,
                why: repair_refusal_words(refusal),
            },
        }
    }
}

/// The sim's refusal of a repair, in words.
#[must_use]
pub fn repair_refusal_words(refusal: RepairRefusal) -> String {
    words(match refusal {
        RepairRefusal::EmptyHand => "strata.fitting.repair.empty",
        RepairRefusal::NothingToRepair => "strata.fitting.repair.nothing",
        RepairRefusal::NoKit => "strata.fitting.repair.no_kit",
    })
    .to_owned()
}

/// Everything the definition is made from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FittingView {
    /// The hand shown.
    pub hand: usize,
    /// A tab for each hand that holds a weapon.
    pub tabs: Vec<TabView>,
    /// The weapon in that hand; none for an empty one.
    pub weapon: Option<WeaponView>,
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

/// What a view is read from: the local player's weapons and stock, and
/// what is in reach.
#[derive(Debug, Clone, Copy)]
pub struct Bench<'a> {
    /// The catalogue.
    pub items: &'a ItemCatalogue,
    /// The weapon in each hand.
    pub hands: &'a [Option<Record>; HANDS],
    /// The holder's row for each hand's family with nothing fitted.
    pub bare: [Row; HANDS],
    /// Who holds them, whose sheet a melee weapon's cuts are read for.
    pub holder: &'a Combatant,
    /// The hand whose weapon is drawn.
    pub drawn: usize,
    /// What the character carries.
    pub pack: &'a Pack,
    /// The household's store, which is to hand at a workbench.
    pub store: &'a HouseholdStore,
    /// Whether a workbench is in reach.
    pub at_workbench: bool,
    /// What a rework is judged from; none where the screen offers none.
    pub rework: Option<ReworkBench<'a>>,
    /// The baked pictures, for a part's icon; none where they cannot be
    /// drawn.
    pub pictures: Option<&'a PictureSet>,
    /// The icon library, for the symbol a part falls back to where nothing
    /// is baked; none draws a part with no icon.
    pub symbols: Option<&'a strata_registry::IconCatalogue>,
    /// The baked models and the view the session chose, where there is a
    /// stage to draw a model on. None draws the flat diagram with no
    /// switch.
    pub models: Option<(&'a ModelSet, WellView)>,
}

/// What the sim's rework rules read besides the stock: the data, the bench
/// rule, the reach, the sheet and the purse.
#[derive(Debug, Clone, Copy)]
pub struct ReworkBench<'a> {
    /// The data.
    pub registries: &'a GameRegistries,
    /// How a workbench is judged to be in reach.
    pub access: WorkbenchAccess,
    /// What is in reach.
    pub reach: &'a Reach,
    /// The character's boxes.
    pub sheet: &'a CharacterSheet,
    /// The household's credits, for the blank.
    pub purse: Purse,
}

/// `Store grade`, or `Batch 412`.
fn grade_words(module: &Fitted) -> String {
    if module.by.is_none() && module.batch == 0 {
        words("strata.fitting.grade.store").to_owned()
    } else {
        fill(
            words("strata.fitting.grade.batch"),
            "batch",
            &module.batch.to_string(),
        )
    }
}

/// Why the sim would not do it, as a player reads it.
#[must_use]
pub fn refusal_words(refusal: FitRefusal) -> String {
    match refusal {
        FitRefusal::NoPlayer => words("strata.fitting.refused.no_player").to_owned(),
        FitRefusal::NoWorkbench => words("strata.fitting.refused.no_workbench").to_owned(),
        FitRefusal::Busy => words("strata.fitting.refused.busy").to_owned(),
        FitRefusal::NoWeapon => words("strata.fitting.why.no_weapon").to_owned(),
        FitRefusal::Shut { opens_at } => fill(
            words("strata.fitting.why.shut"),
            "threshold",
            &opens_at.to_string(),
        ),
        FitRefusal::NoMount | FitRefusal::WrongMount | FitRefusal::DoesNotFit => {
            words("strata.fitting.refused.does_not_fit").to_owned()
        }
        FitRefusal::AlreadyFitted => words("strata.fitting.why.already").to_owned(),
        FitRefusal::NotInStock => words("strata.fitting.refused.not_in_stock").to_owned(),
        FitRefusal::NothingFitted => words("strata.fitting.why.nothing").to_owned(),
        FitRefusal::NoRoom => words("strata.fitting.refused.no_room").to_owned(),
    }
}

/// The properties a weapon's table shows: the five every firearm has,
/// then whatever else a module made for this weapon changes, the ones the
/// fight has a number for before the ones it has not. The list is the
/// weapon's and not the pick's, so the table keeps its rows as the focus
/// walks the parts.
fn table_properties(items: &ItemCatalogue, weapon: &Record) -> Vec<Property> {
    let melee = fitting::weapon_of(items, weapon).is_some_and(strata_sim::combat::Weapon::is_melee);
    let base: &[Property] = if melee { &MELEE_TABLE } else { &TABLE };
    let mut all: Vec<Property> = base.to_vec();
    let mut extra = BTreeSet::new();
    for def in items.iter() {
        if let ItemKind::Module { fits, effects, .. } = &def.kind
            && fits.contains(&weapon.item)
        {
            extra.extend(effects.iter().map(|effect| effect.property));
        }
    }
    for live in [true, false] {
        all.extend(extra.iter().copied().filter(|property| {
            fitting::live_on(*property, melee) == live && !base.contains(property)
        }));
    }
    all
}

/// The AFTER column for `then` against `now`: a figure where it differs
/// and nothing where it does not, so the column shows only what the part
/// would move.
fn after_cells(
    items: &ItemCatalogue,
    bare: &Row,
    now: &Record,
    then: &Record,
    properties: &[Property],
) -> Vec<(String, String, Option<bool>)> {
    fitting::lines(items, bare, now, then, properties)
        .iter()
        .map(|line| match card::change_words(line) {
            Some((change, better)) => (card::value_words(line), change, Some(better)),
            None => (String::new(), String::new(), None),
        })
        .collect()
}

impl FittingView {
    /// The view of `bench` for the screen's `state`.
    #[must_use]
    pub fn of(bench: &Bench<'_>, state: &FittingState) -> Self {
        let items = bench.items;
        let hand = state.hand.min(HANDS - 1);
        let tabs = (0..HANDS)
            .filter_map(|tab| {
                let record = bench.hands[tab].as_ref()?;
                let family = match items.get(&record.item).map(|def| &def.kind) {
                    Some(ItemKind::Weapon { family, .. }) => family.to_uppercase(),
                    _ => return None,
                };
                Some(TabView {
                    hand: tab,
                    label: fill(
                        words(if tab == bench.drawn {
                            "strata.fitting.tab.in_hand"
                        } else {
                            "strata.fitting.tab.holstered"
                        }),
                        "weapon",
                        &family,
                    ),
                    on: tab == hand,
                })
            })
            .collect();
        let weapon = bench.hands[hand]
            .as_ref()
            .and_then(|record| WeaponView::of(bench, state, hand, record));
        let refused = state.notice.as_ref().filter(|(_, refused)| *refused);
        let cuts_only = weapon
            .as_ref()
            .is_some_and(|weapon| weapon.rows.is_empty() && !weapon.facts.is_empty());
        let resting = words(if bench.at_workbench && cuts_only {
            "strata.fitting.status.cuts"
        } else if bench.at_workbench {
            "strata.fitting.status.bench"
        } else {
            "strata.fitting.status.reading"
        });
        let (status, keys) = match &state.notice {
            Some(notice) => (notice.clone(), Some(resting.to_owned())),
            None => ((resting.to_owned(), false), None),
        };
        let mut view = Self {
            hand,
            tabs,
            weapon,
            at_workbench: bench.at_workbench,
            status,
            keys,
            focus: None,
        };
        // A refusal is said at the button that was pressed as well.
        if let (Some((words, _)), Some(weapon)) = (refused, view.weapon.as_mut()) {
            weapon.preview.why.clone_from(words);
            weapon.preview.blocked = true;
        }
        view.focus = view.focus_for(state.last.as_deref());
        view
    }

    /// Where the keyboard's focus starts: on the control last used when it
    /// is still there to be used, else on the row of the mount in focus,
    /// else on the first tab.
    fn focus_for(&self, last: Option<&str>) -> Option<String> {
        let fallback = || {
            self.weapon
                .as_ref()
                .map(|weapon| mount_id(&weapon.mount))
                .or_else(|| self.tabs.first().map(|tab| hand_id(tab.hand)))
        };
        let Some(last) = last else {
            return fallback();
        };
        let weapon = self.weapon.as_ref();
        let there = if last == FIT {
            weapon.is_some_and(|weapon| weapon.fit.is_some())
        } else if last == REMOVE {
            weapon.is_some_and(|weapon| weapon.remove.is_some())
        } else if last == REPAIR {
            weapon.is_some_and(|weapon| weapon.repair.request.is_some())
        } else if last == REWORK {
            weapon.is_some_and(|weapon| weapon.rework.as_ref().is_some_and(|r| r.request.is_some()))
        } else if last == WELL_SWITCH {
            weapon.is_some_and(|weapon| weapon.switchable)
        } else if last == BUY_BLANK {
            weapon.is_some_and(|weapon| weapon.rework.as_ref().is_some_and(|r| r.buy.is_some()))
        } else if let Some(pick) = last.strip_prefix(PICK_TAG) {
            weapon.is_some_and(|weapon| weapon.picks.iter().any(|row| row.id == pick))
        } else if let Some(mount) = last
            .strip_prefix(MOUNT_TAG)
            .or_else(|| last.strip_prefix(diagram::MOUNT_ID))
        {
            weapon.is_some_and(|weapon| weapon.mounts.iter().any(|row| row.key == mount))
        } else if let Some(hand) = last.strip_prefix(HAND_TAG) {
            self.tabs.iter().any(|tab| tab.hand.to_string() == hand)
        } else {
            false
        };
        if there {
            Some(last.to_owned())
        } else {
            fallback()
        }
    }

    /// The preview of the part `pick`, by its id.
    #[must_use]
    pub fn preview_of(&self, pick: &str) -> Option<&Preview> {
        self.weapon
            .as_ref()?
            .picks
            .iter()
            .find(|row| row.id == pick)
            .map(|row| &row.preview)
    }
}

impl WeaponView {
    /// The view of the weapon `record` in `hand`; none for a record that
    /// is no weapon.
    #[allow(
        clippy::too_many_lines,
        reason = "one weapon read top to bottom, as the screen shows it"
    )]
    fn of(bench: &Bench<'_>, state: &FittingState, hand: usize, record: &Record) -> Option<Self> {
        let items = bench.items;
        let def = items.get(&record.item)?;
        let ItemKind::Weapon {
            chassis,
            mounts: mount_defs,
            ..
        } = &def.kind
        else {
            return None;
        };
        let bare = &bench.bare[hand];
        let held = fitting::mounts(items, record);
        let mount = state
            .mount
            .clone()
            .filter(|mount| held.iter().any(|(key, _)| key == mount))
            .or_else(|| {
                held.iter()
                    .find(|(key, _)| key == DEFAULT_MOUNT)
                    .or_else(|| held.first())
                    .map(|(key, _)| key.clone())
            })
            .unwrap_or_default();
        let mounts: Vec<MountView> = held
            .iter()
            .map(|(key, state)| {
                let (name, sub, look, part) = match state {
                    MountState::Standard => (
                        words("strata.fitting.mount.standard").to_owned(),
                        words("strata.fitting.mount.standard.sub").to_owned(),
                        MountLook::Standard,
                        None,
                    ),
                    MountState::Fitted(module) => {
                        let module_def = items.get(&module.item);
                        (
                            module_def.map_or_else(String::new, |module| {
                                short_name(&def.name, &module.name)
                            }),
                            grade_words(module),
                            MountLook::Fitted,
                            module_def.and_then(|def| match &def.kind {
                                ItemKind::Module { part, .. } if !part.is_empty() => {
                                    Some(part.clone())
                                }
                                _ => None,
                            }),
                        )
                    }
                    MountState::Shut { opens_at } => (
                        words("strata.fitting.mount.shut").to_owned(),
                        fill(
                            words("strata.fitting.mount.shut.sub"),
                            "threshold",
                            &opens_at.to_string(),
                        ),
                        MountLook::Shut,
                        None,
                    ),
                };
                MountView {
                    key: key.clone(),
                    label: diagram::words(key).to_uppercase(),
                    name,
                    sub,
                    look,
                    part,
                    on: *key == mount,
                }
            })
            .collect();
        let open = mounts
            .iter()
            .filter(|row| row.look != MountLook::Shut)
            .count();
        let state_of = held
            .iter()
            .find(|(key, _)| *key == mount)
            .map(|(_, state)| state.clone());
        let mount_word = diagram::words(&mount).to_lowercase();

        // A melee weapon's table is its cuts' properties, and its cut
        // figures in the game's units stand beneath it.
        let mut cuts = card::cut_facts(items, bench.holder, record);
        // The table beside them already reads the draw and the recovery.
        // The guard's two figures are the class's and no part moves them,
        // which the card still shows.
        cuts.retain(|(name, _)| {
            ![
                "strata.property.equip_time",
                "strata.cut.light_recovery",
                "strata.cut.guard_share",
                "strata.cut.parry_window",
            ]
            .iter()
            .any(|key| name == words(key))
        });
        let properties = table_properties(items, record);
        let melee = !cuts.is_empty();
        let stripped = fitting::bare_of(record);
        let figures = |of: &Record| -> Vec<String> {
            fitting::lines(items, bare, of, of, &properties)
                .iter()
                .map(card::value_words)
                .collect()
        };
        let rows: Vec<RowView> = properties
            .iter()
            .zip(figures(record))
            .zip(figures(&stripped))
            .map(|((property, now), stripped_figure)| RowView {
                property: *property,
                name: card::property_name_for(*property, bare.spec.mode).to_owned(),
                now,
                bare: stripped_figure,
                live: fitting::live_on(*property, melee),
            })
            .collect();

        let picks = picks(
            bench,
            state,
            hand,
            record,
            &mount,
            state_of.as_ref(),
            &properties,
        );
        let none = (!picks
            .iter()
            .any(|row| row.id != PICK_STANDARD && row.id != PICK_FITTED))
        .then(|| {
            fill(
                words(if bench.at_workbench {
                    "strata.fitting.pick.none"
                } else {
                    "strata.fitting.pick.none.pack"
                }),
                "mount",
                &mount_word,
            )
        });
        let picked = picks.iter().find(|row| row.on);
        let fit = picked.and_then(|row| row.request.clone());
        let fitted_now = matches!(state_of, Some(MountState::Fitted(_)));
        // Whether there is anything to take off is the sim's to say.
        let remove =
            (bench.at_workbench && fitting::without(items, record, &mount).is_ok()).then(|| {
                FitRequest::Remove {
                    hand,
                    mount: mount.clone(),
                }
            });
        // Beside the buttons: why Fit is grey, and when it is not, why
        // Remove is. A grey button never stands alone.
        // A row lit only because it is what the mount holds is no pick:
        // the line then asks for one.
        let general = general_why(bench, state_of.as_ref());
        let (mut why, blocked) = picked.filter(|_| state.pick != Pick::Current).map_or_else(
            || {
                general.clone().map_or_else(
                    || (words("strata.fitting.why.pick").to_owned(), false),
                    |general| (general, true),
                )
            },
            |row| (row.preview.why.clone(), row.preview.blocked),
        );
        if remove.is_none() && general.is_none() {
            let nothing = words("strata.fitting.why.nothing");
            if why.is_empty() {
                nothing.clone_into(&mut why);
            } else if why != nothing {
                why = format!("{why} · {nothing}");
            }
        }
        let preview = Preview {
            after: picked.map_or_else(
                || vec![(String::new(), String::new(), None); rows.len()],
                |row| row.preview.after.clone(),
            ),
            why,
            blocked,
        };

        let thresholds: Vec<u16> = mount_defs
            .iter()
            .filter_map(|mount| mount.opens_at)
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect();
        // With no mounts there is nothing for moddability to open.
        let moddability = record
            .lines
            .get(MODDABILITY)
            .filter(|_| !mount_defs.is_empty())
            .map(|line| (*line, thresholds.clone()));
        let listed: Vec<String> = thresholds.iter().map(ToString::to_string).collect();
        let thresholds = match listed.split_last() {
            None => words("strata.fitting.moddability.all").to_owned(),
            Some((last, rest)) => {
                let mut all = rest.join(", ");
                if !rest.is_empty() {
                    all.push_str(words("strata.fitting.moddability.and"));
                }
                all.push_str(last);
                fill(
                    words("strata.fitting.moddability.opens"),
                    "thresholds",
                    &all,
                )
            }
        };
        Some(Self {
            name: def.name.clone(),
            sub: fill(
                words("strata.fitting.sub"),
                "maker",
                &def.maker.to_uppercase(),
            ),
            condition: record.condition,
            chassis: chassis.clone(),
            model: bench.models.is_some_and(|(models, well)| {
                well == WellView::Model && weapon_model::wanted(models, items, record).is_some()
            }),
            switchable: bench
                .models
                .is_some_and(|(models, _)| weapon_model::wanted(models, items, record).is_some()),
            in_hand: hand == bench.drawn,
            open: fill(
                &fill(
                    words("strata.fitting.mounts.open"),
                    "open",
                    &open.to_string(),
                ),
                "all",
                &mounts.len().to_string(),
            ),
            heading: fill(
                words(if bench.at_workbench {
                    "strata.fitting.fits"
                } else {
                    "strata.fitting.fits.pack"
                }),
                "mount",
                &mount_word.to_uppercase(),
            ),
            mounts,
            mount,
            picks,
            none,
            rows,
            facts: if cuts.is_empty() {
                card::weapon_facts(&bare.spec)
            } else {
                cuts
            },
            preview,
            fit_label: if matches!(state.pick, Pick::Standard) && fitted_now {
                "strata.fitting.fit.standard"
            } else {
                "strata.fitting.fit"
            },
            fit,
            remove,
            moddability,
            thresholds,
            band: wear::band(
                items.wear(),
                wear::condition_of(items.wear(), record.condition),
            ),
            repair: RepairView::of(bench, hand),
            rework: if mount_defs.is_empty() {
                None
            } else {
                ReworkView::of(bench, hand, record)
            },
        })
    }
}

/// A module's name on a mount of `weapon`: without the leading words it
/// shares with the weapon's own name, which the screen's head already
/// gives. `Patrol extended magazine` on the `Vigil patrol carbine` reads
/// `Extended magazine`, and stays on one line of the mounts' column.
fn short_name(weapon: &str, module: &str) -> String {
    let shared: Vec<String> = weapon.split_whitespace().map(str::to_lowercase).collect();
    let mut rest = module;
    while let Some((first, after)) = rest.split_once(' ') {
        if !shared.contains(&first.to_lowercase()) {
            break;
        }
        rest = after.trim_start();
    }
    let mut letters = rest.chars();
    letters.next().map_or_else(String::new, |first| {
        first.to_uppercase().chain(letters).collect()
    })
}

/// `Capacity 610`: the quality of the line a part is fitted for, which is
/// what tells two batches apart when both read the same in rounds.
fn quality_words(items: &ItemCatalogue, module: &Fitted) -> Option<String> {
    let effect = fitting::lead(items, &module.item)?;
    let line = items
        .get(&module.item)?
        .lines
        .iter()
        .find(|line| line.id == effect.line)?;
    Some(fill(
        &fill(words("strata.fitting.pick.quality"), "line", &line.name),
        "quality",
        &fitting::lead_quality(module, effect).to_string(),
    ))
}

/// The colour an item's icon is drawn in.
fn tint_of(items: &ItemCatalogue, item: &strata_registry::Namespaced) -> [u8; 3] {
    let [red, green, blue, _] = items.get(item).map_or([1.; 4], |def| def.icon.color.0);
    #[allow(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "a colour channel, clamped to its range first"
    )]
    [red, green, blue].map(|channel| (channel.clamp(0., 1.) * 255.).round() as u8)
}

/// Why nothing at all can be fitted to the mount from here, whatever is
/// picked: no workbench in reach, or a shut mount.
fn general_why(bench: &Bench<'_>, mount: Option<&MountState>) -> Option<String> {
    if !bench.at_workbench {
        return Some(words("strata.fitting.why.no_workbench").to_owned());
    }
    match mount {
        Some(MountState::Shut { opens_at }) => Some(refusal_words(FitRefusal::Shut {
            opens_at: *opens_at,
        })),
        _ => None,
    }
}

/// One mount of the weapon on the bench, as the rows of its parts are
/// made from it.
struct Parts<'a> {
    bench: &'a Bench<'a>,
    state: &'a FittingState,
    hand: usize,
    weapon: &'a Record,
    mount: &'a str,
    /// The module on the mount now.
    fitted: Option<&'a Fitted>,
    /// The weapon with the mount's standard part.
    standard: Option<Record>,
    /// Why nothing can be fitted to the mount from here.
    general: Option<String>,
    properties: &'a [Property],
}

impl Parts<'_> {
    fn bare(&self) -> &Row {
        &self.bench.bare[self.hand]
    }

    /// An AFTER column in which nothing differs.
    fn nothing(&self) -> Vec<(String, String, Option<bool>)> {
        vec![(String::new(), String::new(), None); self.properties.len()]
    }

    /// The first thing `module` does that a player would fit it for, read
    /// on `then` against `now`: its first benefit, as the sim ranks it,
    /// else its first effect.
    fn lead(&self, module: &Fitted, now: &Record, then: &Record) -> Option<CardEffect> {
        let items = self.bench.items;
        let effect = fitting::lead(items, &module.item)?;
        fitting::lines(items, self.bare(), now, then, &[effect.property])
            .first()
            .map(|line| CardEffect::of_for(line, self.bare().spec.mode))
    }

    fn name_of(&self, module: &Fitted) -> String {
        self.bench
            .items
            .get(&module.item)
            .map_or_else(String::new, |def| def.name.clone())
    }

    fn icon_of(&self, module: &Fitted) -> Option<String> {
        let def = self.bench.items.get(&module.item)?;
        let none = strata_registry::IconCatalogue::default();
        card::item_art(
            def,
            self.bench.pictures,
            None,
            self.bench.symbols.unwrap_or(&none),
        )
        .map(|art| art.path)
    }

    /// Where a module is from: its grade or batch, then the quality of
    /// the line it is fitted for.
    fn origin_words(&self, module: &Fitted) -> Vec<String> {
        let mut from = vec![grade_words(module)];
        from.extend(quality_words(self.bench.items, module));
        from
    }

    /// The standard part: what is there, or what a removal puts back.
    fn standard_row(&self) -> PickView {
        let items = self.bench.items;
        let mount_word = diagram::words(self.mount).to_lowercase();
        let then = self.standard.as_ref().zip(self.fitted);
        PickView {
            id: PICK_STANDARD.to_owned(),
            name: fill(words("strata.fitting.pick.standard"), "mount", &mount_word),
            sub: words(if self.fitted.is_some() {
                "strata.fitting.pick.put_back"
            } else {
                "strata.fitting.pick.now"
            })
            .to_owned(),
            lead: then.and_then(|(then, module)| self.lead(module, self.weapon, then)),
            on: matches!(self.state.pick, Pick::Standard)
                || (self.state.pick == Pick::Current && self.fitted.is_none()),
            icon: self.bench.pictures.and_then(|pictures| {
                let ItemKind::Weapon { chassis, .. } = &items.get(&self.weapon.item)?.kind else {
                    return None;
                };
                let piece = pictures.chassis.get(chassis)?.piece(self.mount, None)?;
                Some(card::generated(piece.icon.as_ref()?))
            }),
            tint: tint_of(items, &self.weapon.item),
            preview: Preview {
                after: then.map_or_else(
                    || self.nothing(),
                    |(then, _)| after_cells(items, self.bare(), self.weapon, then, self.properties),
                ),
                why: self.general.clone().unwrap_or_else(|| {
                    if self.fitted.is_some() {
                        String::new()
                    } else {
                        words("strata.fitting.why.already").to_owned()
                    }
                }),
                blocked: self.general.is_some(),
            },
            module: None,
            request: (self.general.is_none() && self.fitted.is_some()).then(|| {
                FitRequest::Remove {
                    hand: self.hand,
                    mount: self.mount.to_owned(),
                }
            }),
        }
    }

    /// The module on the mount. More of its batch in the stock are counted
    /// here, since another of the same is no change.
    fn fitted_row(&self, module: &Fitted, more: Option<&fitting::Candidate>) -> PickView {
        let mut sub = self.origin_words(module);
        if let Some(more) = more {
            sub.extend(stock_words(more.carried, more.stored));
        }
        sub.push(words("strata.fitting.pick.fitted").to_owned());
        PickView {
            id: PICK_FITTED.to_owned(),
            name: self.name_of(module),
            sub: sub.join(" · "),
            lead: self
                .standard
                .as_ref()
                .and_then(|standard| self.lead(module, standard, self.weapon)),
            on: self.state.pick == Pick::Current,
            icon: self.icon_of(module),
            tint: tint_of(self.bench.items, &module.item),
            preview: Preview {
                after: self.nothing(),
                why: self
                    .general
                    .clone()
                    .unwrap_or_else(|| words("strata.fitting.why.already").to_owned()),
                blocked: self.general.is_some(),
            },
            module: Some(module.clone()),
            request: None,
        }
    }

    /// A module of the stock, the `index`th listed.
    fn candidate_row(&self, index: usize, candidate: &fitting::Candidate) -> PickView {
        let items = self.bench.items;
        let module = &candidate.module;
        let mut sub = self.origin_words(module);
        sub.extend(stock_words(candidate.carried, candidate.stored));
        let why = self.general.clone().unwrap_or_else(|| {
            match fitting::with(items, self.weapon, self.mount, Some(module)) {
                Ok(_) => String::new(),
                Err(refusal) => refusal_words(refusal),
            }
        });
        // Shown as it would be even where the sim would refuse it, so a
        // part for a shut mount still says what it is for.
        let shown = fitting::preview(items, self.weapon, self.mount, Some(module));
        PickView {
            id: index.to_string(),
            name: self.name_of(module),
            sub: sub.join(" · "),
            lead: self.lead(module, self.weapon, &shown),
            on: self.state.pick == Pick::Module(module.clone()),
            icon: self.icon_of(module),
            tint: tint_of(items, &module.item),
            preview: Preview {
                after: after_cells(items, self.bare(), self.weapon, &shown, self.properties),
                blocked: !why.is_empty(),
                why: why.clone(),
            },
            module: Some(module.clone()),
            request: why.is_empty().then(|| FitRequest::Fit {
                hand: self.hand,
                mount: self.mount.to_owned(),
                module: module.clone(),
            }),
        }
    }
}

/// `2 carried`, `4 in store`: where a part's units are.
fn stock_words(carried: u32, stored: u32) -> Vec<String> {
    [
        (carried, "strata.fitting.pick.carried"),
        (stored, "strata.fitting.pick.stored"),
    ]
    .into_iter()
    .filter(|(count, _)| *count > 0)
    .map(|(count, key)| fill(words(key), "count", &count.to_string()))
    .collect()
}

/// The parts for `mount`: the standard one, the module on it now, then
/// what the pack and, at a workbench, the store hold for it, best first as
/// the sim orders them.
fn picks(
    bench: &Bench<'_>,
    state: &FittingState,
    hand: usize,
    weapon: &Record,
    mount: &str,
    held: Option<&MountState>,
    properties: &[Property],
) -> Vec<PickView> {
    let items = bench.items;
    let parts = Parts {
        bench,
        state,
        hand,
        weapon,
        mount,
        fitted: match held {
            Some(MountState::Fitted(module)) => Some(module),
            _ => None,
        },
        standard: fitting::with(items, weapon, mount, None).ok(),
        general: general_why(bench, held),
        properties,
    };
    let candidates = fitting::candidates(
        items,
        bench.pack,
        bench.at_workbench.then_some(bench.store),
        weapon,
        mount,
    );
    let mut rows = vec![parts.standard_row()];
    if let Some(module) = parts.fitted {
        let more = candidates
            .iter()
            .find(|candidate| candidate.module == *module);
        rows.push(parts.fitted_row(module, more));
    }
    rows.extend(
        candidates
            .iter()
            .filter(|candidate| Some(&candidate.module) != parts.fitted)
            .enumerate()
            .map(|(index, candidate)| parts.candidate_row(index, candidate)),
    );
    rows
}

// ---- the definition ----

fn key(s: impl Into<String>) -> LocKey {
    LocKey(s.into())
}

fn text_in(s: impl Into<String>, style: TextRole, role: &str) -> UiNodeDef {
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
fn line_in(s: impl Into<String>, role: &str) -> UiNodeDef {
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

fn with_id(mut node: UiNodeDef, id: &str) -> UiNodeDef {
    if let UiNodeDef::Text { tags, .. } | UiNodeDef::Panel { tags, .. } = &mut node {
        *tags = tags.clone().with(Tags::TEST_ID, id);
    }
    node
}

fn panel(role: &str, layout: Layout, children: Vec<UiNodeDef>) -> UiNodeDef {
    UiNodeDef::Panel {
        role: Role::new(role),
        layout,
        children,
        tags: Tags::new(),
    }
}

fn column(gap: f32) -> Layout {
    Layout {
        direction: LayoutDirection::Column,
        gap,
        ..Layout::default()
    }
}

fn row(gap: f32) -> Layout {
    Layout {
        direction: LayoutDirection::Row,
        gap,
        align: Some(Align::Center),
        ..Layout::default()
    }
}

fn spacer() -> UiNodeDef {
    UiNodeDef::Spacer {
        size: Length::Percent(100.),
        tags: Tags::new(),
    }
}

fn head(label: impl Into<String>) -> UiNodeDef {
    line_in(label, HEAD_ROLE)
}

/// A box of a fixed width whose one line ends at its right edge: a column
/// of figures.
fn figure_cell(width: f32, node: UiNodeDef) -> UiNodeDef {
    panel(
        "invisible",
        Layout {
            width: Some(Length::Px(width)),
            justify: Justify::End,
            ..row(0.)
        },
        vec![node],
    )
}

/// The gap between a figure of the AFTER column and its change.
const CHANGE_GAP: f32 = 6.;

/// Whether a preview has a figure for the AFTER column: the column and
/// the changes beside it are given their width only then, so a weapon
/// with nothing picked reads NOW beside BARE with no empty column between.
fn after_shown(preview: &Preview) -> bool {
    preview.after.iter().any(|cell| !cell.0.is_empty())
}

/// The width of one of the AFTER column's boxes, by its `test_id`, when
/// the column is shown and when it is not.
fn column_width(id: &str, shown: bool) -> Option<f32> {
    let of = id.strip_prefix(COLUMN_TAG)?;
    let width = if of.starts_with("after:") {
        AFTER_WIDTH
    } else {
        CHANGE_WIDTH
    };
    Some(if shown { width } else { 0. })
}

/// The `test_id` of the box of `column` (`after`, `change`) in the row
/// `of`: a property's key, or `head`.
fn column_id(column: &str, of: &str) -> String {
    format!("{COLUMN_TAG}{column}:{of}")
}

/// A figure of the AFTER column in the row `of`.
fn after_cell(of: &str, shown: bool, node: UiNodeDef) -> UiNodeDef {
    let id = column_id("after", of);
    let width = column_width(&id, shown).unwrap_or(AFTER_WIDTH);
    with_id(
        panel(
            "invisible",
            Layout {
                width: Some(Length::Px(width)),
                min_width: Some(Length::Px(0.)),
                justify: Justify::End,
                ..row(0.)
            },
            vec![node],
        ),
        &id,
    )
}

/// The box of a change: it starts beside the figure it is the change of,
/// so it is read with the AFTER column and not with the one after it.
fn change_cell(of: &str, shown: bool, node: UiNodeDef) -> UiNodeDef {
    let id = column_id("change", of);
    let width = column_width(&id, shown).unwrap_or(CHANGE_WIDTH);
    with_id(
        panel(
            "invisible",
            Layout {
                width: Some(Length::Px(width)),
                min_width: Some(Length::Px(0.)),
                ..row(0.)
            },
            vec![
                UiNodeDef::Spacer {
                    size: Length::Px(CHANGE_GAP),
                    tags: Tags::new(),
                },
                node,
            ],
        ),
        &id,
    )
}

/// The role of the line beside the buttons: a reason in the refusal's
/// colour, a word on what to do next in the footer's.
fn why_role(blocked: bool) -> &'static str {
    if blocked { WHY_ROLE } else { WHY_HINT_ROLE }
}

fn change_role(better: Option<bool>) -> &'static str {
    if better == Some(false) {
        WORSE_ROLE
    } else {
        BETTER_ROLE
    }
}

/// One hand's tab: a pill that is the button, lit for the weapon shown.
fn tab(view: &TabView) -> UiNodeDef {
    let id = hand_id(view.hand);
    UiNodeDef::Card {
        role: Role::new(if view.on { TAB_ON_ROLE } else { TAB_ROLE }),
        layout: Layout {
            padding: Padding {
                top: 0.5,
                right: 2.2,
                bottom: 0.5,
                left: 2.2,
            },
            ..row(0.)
        },
        // The chosen one says so by a mark as well as by its fill.
        children: vec![line_in(
            if view.on {
                format!("{}{}", diagram::FOCUS_MARK, view.label)
            } else {
                view.label.clone()
            },
            if view.on {
                TAB_TEXT_ON_ROLE
            } else {
                TAB_TEXT_ROLE
            },
        )],
        disabled: false,
        tags: Tags::new().with("menu", &id).with(Tags::TEST_ID, &id),
    }
}

/// One mount's row: its name in the label's column, then what it holds
/// over a line that says more. The whole row is the button.
fn mount_row(view: &MountView) -> UiNodeDef {
    let id = mount_id(&view.key);
    let role = match (view.on, view.look) {
        (true, _) => MOUNT_ON_ROLE,
        (false, MountLook::Shut) => MOUNT_SHUT_ROLE,
        (false, _) => MOUNT_ROLE,
    };
    UiNodeDef::Card {
        role: Role::new(role),
        layout: Layout {
            width: Some(Length::Percent(100.)),
            min_height: Some(Length::Px(46.)),
            padding: Padding {
                top: 0.9,
                right: 1.4,
                bottom: 0.9,
                left: 1.6,
            },
            ..row(1.4)
        },
        children: vec![
            // Held to its width, so a name long enough to wrap does not
            // push the labels' column in.
            panel(
                "invisible",
                Layout {
                    width: Some(Length::Px(LABEL_WIDTH)),
                    min_width: Some(Length::Px(LABEL_WIDTH)),
                    ..row(0.)
                },
                // The mount in focus says so by a mark as well as by
                // its fill, as its box on the drawing does.
                vec![line_in(
                    if view.on {
                        format!("{}{}", diagram::FOCUS_MARK, view.label)
                    } else {
                        view.label.clone()
                    },
                    if view.on {
                        MOUNT_LABEL_ON_ROLE
                    } else {
                        MOUNT_LABEL_ROLE
                    },
                )],
            ),
            panel(
                "invisible",
                Layout {
                    grow: 1.,
                    min_width: Some(Length::Px(0.)),
                    ..column(0.1)
                },
                vec![
                    text_in(
                        view.name.clone(),
                        TextRole::Body,
                        if view.look == MountLook::Shut {
                            MOUNT_NAME_SHUT_ROLE
                        } else {
                            MOUNT_NAME_ROLE
                        },
                    ),
                    text_in(view.sub.clone(), TextRole::Caption, MOUNT_SUB_ROLE),
                ],
            ),
        ],
        disabled: false,
        tags: Tags::new().with("menu", &id).with(Tags::TEST_ID, &id),
    }
}

/// One part's row: its picture, its name over where it is from, and at the
/// right what it would do, or why it cannot be fitted.
fn pick_row(view: &PickView) -> UiNodeDef {
    let id = pick_id(&view.id);
    let well = panel(
        PICK_ICON_ROLE,
        Layout {
            width: Some(Length::Px(ICON_WELL)),
            height: Some(Length::Px(ICON_WELL)),
            justify: Justify::Center,
            ..row(0.)
        },
        view.icon
            .iter()
            .map(|path| UiNodeDef::Image {
                path: path.clone(),
                width: Length::Px(ICON),
                height: Length::Px(ICON),
                tags: Tags::new().with(Tags::TEST_ID, &format!("{ICON_TAG}{}", view.id)),
            })
            .collect(),
    );
    let name = if view.on {
        fill(words("strata.fitting.pick.picked"), "name", &view.name)
    } else {
        view.name.clone()
    };
    let mut right = Vec::new();
    if let Some(lead) = &view.lead {
        right.push(line_in(
            format!("{} {}", lead.name, lead.value),
            PICK_LEAD_ROLE,
        ));
        if !lead.change.is_empty() {
            right.push(line_in(lead.change.clone(), change_role(lead.better)));
        }
    }
    UiNodeDef::Card {
        role: Role::new(if view.on { PICK_ON_ROLE } else { PICK_ROLE }),
        layout: Layout {
            width: Some(Length::Percent(100.)),
            padding: Padding {
                top: 0.7,
                right: 1.6,
                bottom: 0.7,
                left: 0.8,
            },
            ..row(1.4)
        },
        children: vec![
            well,
            panel(
                "invisible",
                Layout {
                    grow: 1.,
                    min_width: Some(Length::Px(0.)),
                    ..column(0.1)
                },
                vec![
                    text_in(name, TextRole::Body, MOUNT_NAME_ROLE),
                    text_in(view.sub.clone(), TextRole::Caption, MOUNT_SUB_ROLE),
                ],
            ),
            panel("invisible", row(1.2), right),
        ],
        disabled: false,
        tags: Tags::new().with("menu", &id).with(Tags::TEST_ID, &id),
    }
}

/// One row of the table: the property, the figure now, the figure after
/// and its change, the figure bare.
fn table_row(view: &RowView, after: &(String, String, Option<bool>), shown: bool) -> UiNodeDef {
    let of = view.property.key();
    let cell = |name: &str, words: &str, role: &str| {
        with_id(line_in(words, role), &cell_id(view.property, name))
    };
    with_id(
        panel(
            "invisible",
            Layout {
                width: Some(Length::Percent(100.)),
                min_height: Some(Length::Px(22.)),
                ..row(0.)
            },
            vec![
                panel(
                    "invisible",
                    Layout {
                        grow: 1.,
                        min_width: Some(Length::Px(0.)),
                        ..row(0.)
                    },
                    vec![line_in(view.name.clone(), CELL_NAME_ROLE)],
                ),
                figure_cell(NOW_WIDTH, cell("now", &view.now, CELL_NOW_ROLE)),
                after_cell(of, shown, cell("after", &after.0, CELL_AFTER_ROLE)),
                change_cell(of, shown, cell("change", &after.1, change_role(after.2))),
                figure_cell(BARE_WIDTH, cell("bare", &view.bare, CELL_BARE_ROLE)),
            ],
        ),
        &format!("{ROW_TAG}{}", view.property.key()),
    )
}

/// The AFTER column's heading, which is there while the column is.
fn after_heading(shown: bool) -> &'static str {
    if shown {
        words("strata.fitting.table.after")
    } else {
        ""
    }
}

/// The table: its headings, the rows the fight has a number for, then the
/// ones it has not under their own heading.
fn table(weapon: &WeaponView) -> UiNodeDef {
    let shown = after_shown(&weapon.preview);
    let headings = panel(
        "invisible",
        Layout {
            width: Some(Length::Percent(100.)),
            ..row(0.)
        },
        vec![
            spacer(),
            figure_cell(NOW_WIDTH, head("strata.fitting.table.now")),
            after_cell(
                "head",
                shown,
                with_id(head(after_heading(shown)), AFTER_HEAD_ID),
            ),
            change_cell("head", shown, head("")),
            figure_cell(BARE_WIDTH, head("strata.fitting.table.bare")),
        ],
    );
    // A weapon with cuts and no table has one column of figures, which
    // needs no NOW and BARE over it.
    let mut children = if weapon.rows.is_empty() {
        Vec::new()
    } else {
        vec![headings]
    };
    let rows = weapon.rows.iter().zip(&weapon.preview.after);
    children.extend(
        rows.clone()
            .filter(|(row, _)| row.live)
            .map(|(row, after)| table_row(row, after, shown)),
    );
    children.extend(weapon.facts.iter().map(|(name, value)| {
        panel(
            "invisible",
            Layout {
                width: Some(Length::Percent(100.)),
                min_height: Some(Length::Px(22.)),
                ..row(0.)
            },
            vec![
                panel(
                    "invisible",
                    Layout {
                        grow: 1.,
                        min_width: Some(Length::Px(0.)),
                        ..row(0.)
                    },
                    vec![line_in(name.clone(), CELL_NAME_ROLE)],
                ),
                line_in(value.clone(), CELL_NOW_ROLE),
            ],
        )
    }));
    let later: Vec<UiNodeDef> = rows
        .filter(|(row, _)| !row.live)
        .map(|(row, after)| table_row(row, after, shown))
        .collect();
    if !later.is_empty() {
        let mut list = vec![head("strata.fitting.later")];
        list.extend(later);
        children.push(with_id(
            panel(
                "invisible",
                Layout {
                    width: Some(Length::Percent(100.)),
                    padding: Padding {
                        top: 1.,
                        ..Padding::default()
                    },
                    ..column(0.3)
                },
                list,
            ),
            LATER_ID,
        ));
    }
    panel(
        BOX_ROLE,
        Layout {
            width: Some(Length::Percent(100.)),
            padding: Padding {
                top: 1.6,
                right: 2.,
                bottom: 1.6,
                left: 2.,
            },
            align: Some(Align::Stretch),
            ..column(0.3)
        },
        children,
    )
}

/// The weapon's moddability on its track, with a tick where each mount
/// opens.
fn moddability(weapon: &WeaponView) -> Option<UiNodeDef> {
    let (line, ticks) = weapon.moddability.clone()?;
    Some(panel(
        BOX_ROLE,
        Layout {
            width: Some(Length::Percent(100.)),
            padding: Padding {
                top: 1.6,
                right: 2.,
                bottom: 1.6,
                left: 2.,
            },
            align: Some(Align::Stretch),
            ..column(1.)
        },
        vec![
            panel(
                "invisible",
                row(1.),
                vec![
                    head("strata.fitting.moddability"),
                    spacer(),
                    line_in(line.to_string(), FIGURE_ROLE),
                ],
            ),
            BarParams {
                ticks,
                numeral: false,
                ..BarParams::quality(line)
            }
            .node(Some(MODDABILITY_ID)),
            with_id(
                text_in(
                    weapon.thresholds.clone(),
                    TextRole::Caption,
                    "strata.card.note",
                ),
                THRESHOLDS_ID,
            ),
        ]
        .into_iter()
        .chain(
            weapon
                .rework
                .as_ref()
                .map(rework_rows)
                .into_iter()
                .flatten(),
        )
        .collect(),
    ))
}

/// Rework and Buy blank side by side under the bar, with the rework line
/// under them and, when Buy blank is grey, its reason after it. A reason is words in
/// the hint role, so a grey button is never told by its colour alone.
fn rework_rows(rework: &ReworkView) -> Vec<UiNodeDef> {
    let grey = |label: &str, id: &str, blocked: bool| {
        button(
            label,
            id,
            if blocked {
                ButtonVariant::Secondary
            } else {
                ButtonVariant::Primary
            },
            blocked.then_some(ACT_ROLE),
            blocked,
        )
    };
    // One line for both: a second caption would stand the column taller than
    // a short window holds.
    let said = if rework.buy_why.is_empty() {
        rework.line.clone()
    } else {
        format!("{} {}", rework.line, rework.buy_why)
    };
    vec![
        panel(
            "invisible",
            row(1.6),
            vec![
                grey(
                    words("strata.fitting.rework"),
                    REWORK,
                    rework.request.is_none(),
                ),
                grey(&rework.buy_label, BUY_BLANK, rework.buy.is_none()),
            ],
        ),
        // A box of one height whatever it says: a reason that wraps to a
        // third line must not stand the column taller, or a window that
        // scrolls it toggles its scroll bar and the layout never settles.
        panel(
            "invisible",
            Layout {
                height: Some(Length::Px(REWORK_CAPTION_HEIGHT)),
                min_height: Some(Length::Px(REWORK_CAPTION_HEIGHT)),
                overflow: Overflow::Scroll,
                ..column(0.)
            },
            vec![with_id(
                text_in(said, TextRole::Caption, WHY_HINT_ROLE),
                REWORK_WHY_ID,
            )],
        ),
    ]
}

/// The height of the rework caption's box, in px: three lines of caption.
const REWORK_CAPTION_HEIGHT: f32 = 60.;

fn button(
    label: &str,
    id: &str,
    variant: ButtonVariant,
    role: Option<&str>,
    disabled: bool,
) -> UiNodeDef {
    UiNodeDef::Button {
        widget: None,
        opts: ButtonOpts {
            label: Some(key(label)),
            variant,
            disabled,
            role: role.map(Role::new),
            ..ButtonOpts::default()
        },
        tags: Tags::new().with("menu", id).with(Tags::TEST_ID, id),
    }
}

/// The mounts' column: how many are open, then a row a mount.
fn mounts_column(weapon: &WeaponView) -> Vec<UiNodeDef> {
    let mut head_row = vec![head("strata.fitting.mounts"), spacer()];
    if !weapon.mounts.is_empty() {
        head_row.push(with_id(
            text_in(weapon.open.clone(), TextRole::Caption, MOUNT_SUB_ROLE),
            OPEN_ID,
        ));
    }
    let mut left = vec![panel(
        "invisible",
        Layout {
            width: Some(Length::Percent(100.)),
            ..row(1.)
        },
        head_row,
    )];
    left.extend(weapon.mounts.iter().map(mount_row));
    if weapon.mounts.is_empty() {
        left.push(with_id(
            text_in(
                words("strata.fitting.nomounts").to_owned(),
                TextRole::Caption,
                MOUNT_SUB_ROLE,
            ),
            NO_MOUNTS_ID,
        ));
    }
    left
}

/// The flat diagram of the weapon, each mount a button and the one in
/// focus bracketed.
fn diagram_of(weapon: &WeaponView) -> UiNodeDef {
    DiagramParams {
        chassis: weapon.chassis.clone(),
        mounts: weapon
            .mounts
            .iter()
            .map(|mount| DiagramMount {
                key: mount.key.clone(),
                label: diagram::words(&mount.key),
                module: mount.part.clone(),
                state: match mount.look {
                    MountLook::Standard => diagram::MountState::Standard,
                    MountLook::Fitted => diagram::MountState::Fitted,
                    MountLook::Shut => diagram::MountState::Shut,
                },
            })
            .collect(),
        focus: Some(weapon.mount.clone()),
        max_height: Some(DIAGRAM_MAX_VH),
        fixed: true,
    }
    .node()
}

/// The well with the switch. On the model, which fills its well, the
/// switch sits in its top right corner; the flat diagram of a long thin
/// weapon is a low strip the button would cover, so the switch goes in a
/// row of its own above it. The label names the view the button goes to,
/// so the words say what a press does.
fn well_with_switch(drawing: UiNodeDef, on_model: bool) -> UiNodeDef {
    let press = button(
        if on_model {
            "strata.fitting.well.diagram"
        } else {
            "strata.fitting.well.model"
        },
        WELL_SWITCH,
        ButtonVariant::Secondary,
        None,
        false,
    );
    if !on_model {
        return panel(
            "invisible",
            Layout {
                width: Some(Length::Percent(100.)),
                align: Some(Align::Stretch),
                ..column(0.4)
            },
            vec![
                panel(
                    "invisible",
                    Layout {
                        width: Some(Length::Percent(100.)),
                        justify: Justify::End,
                        ..row(0.)
                    },
                    vec![press],
                ),
                drawing,
            ],
        );
    }
    let switch = panel(
        "invisible",
        Layout {
            place: Some(Place {
                anchor: NineAnchor::TopRight,
                offset: Vec2::new(-6., 6.),
            }),
            ..row(0.)
        },
        vec![press],
    );
    panel(
        "invisible",
        Layout {
            width: Some(Length::Percent(100.)),
            align: Some(Align::Center),
            ..column(0.)
        },
        vec![drawing, switch],
    )
}

/// The middle column: the weapon drawn from its pieces, the parts for the
/// mount in focus, and the two buttons with why either is grey. The weapon
/// is the turning model where there is one, with its mount named on a tag
/// and the list on the left the way to pick another, and the flat diagram
/// where there is not.
#[allow(
    clippy::too_many_lines,
    reason = "one column read top to bottom as it stands"
)]
fn parts_column(weapon: &WeaponView) -> Vec<UiNodeDef> {
    /// The nodes ahead of the repair row: the drawing, the heading, the list
    /// and the fit row.
    const FIT_NODES: usize = 4;
    let drawing = if weapon.model {
        ModelParams {
            tag: Some(diagram::words(&weapon.mount)),
            max_height: Some(DIAGRAM_MAX_VH),
            fixed: true,
        }
        .node()
    } else {
        diagram_of(weapon)
    };
    let drawing = if weapon.switchable {
        well_with_switch(drawing, weapon.model)
    } else {
        drawing
    };
    let mut list: Vec<UiNodeDef> = weapon.picks.iter().map(pick_row).collect();
    if let Some(none) = &weapon.none {
        list.push(with_id(
            text_in(none.clone(), TextRole::Caption, "strata.card.note"),
            NONE_ID,
        ));
    }
    let mut nodes = vec![
        drawing,
        with_id(head(weapon.heading.clone()), FITS_ID),
        // The list takes the room the column has left and scrolls in it,
        // so the buttons under it stand where they stood whatever the
        // mount offers.
        UiNodeDef::Scroll {
            layout: Layout {
                grow: 1.,
                min_height: Some(Length::Px(LIST_MIN_HEIGHT)),
                align: Some(Align::Stretch),
                ..column(0.8)
            },
            scrollbar: true,
            children: list,
            tags: Tags::new().with(Tags::TEST_ID, LIST_ID),
        },
        panel(
            "invisible",
            row(1.6),
            vec![
                // The accent only when it can act: the theme has no grey
                // for a primary button, which would stand lit while inert.
                button(
                    weapon.fit_label,
                    FIT,
                    if weapon.fit.is_some() {
                        ButtonVariant::Primary
                    } else {
                        ButtonVariant::Secondary
                    },
                    weapon.fit.is_none().then_some(ACT_ROLE),
                    weapon.fit.is_none(),
                ),
                button(
                    "strata.fitting.remove",
                    REMOVE,
                    ButtonVariant::Secondary,
                    Some(ACT_ROLE),
                    weapon.remove.is_none(),
                ),
                with_id(
                    text_in(
                        weapon.preview.why.clone(),
                        TextRole::Caption,
                        why_role(weapon.preview.blocked),
                    ),
                    WHY_ID,
                ),
            ],
        ),
        panel(
            "invisible",
            row(1.6),
            vec![
                button(
                    &weapon.repair.label,
                    REPAIR,
                    if weapon.repair.request.is_some() {
                        ButtonVariant::Primary
                    } else {
                        ButtonVariant::Secondary
                    },
                    weapon.repair.request.is_none().then_some(ACT_ROLE),
                    weapon.repair.request.is_none(),
                ),
                with_id(
                    text_in(weapon.repair.why.clone(), TextRole::Caption, WHY_HINT_ROLE),
                    REPAIR_WHY_ID,
                ),
            ],
        ),
    ];
    if weapon.mounts.is_empty() {
        // Nothing to fit: the drawing, the parts and their buttons give way
        // to the repair row alone.
        nodes.drain(..FIT_NODES);
    }
    if weapon.band == Band::Worn {
        // The warm colour and the words together: never colour alone.
        nodes.push(with_id(
            text_in("strata.fitting.worn", TextRole::Caption, WHY_ROLE),
            WORN_ID,
        ));
    }
    nodes
}

/// The three columns of a weapon: its mounts, the drawing over the parts
/// and the buttons, and the table over the moddability.
fn body(weapon: &WeaponView) -> Vec<UiNodeDef> {
    let mut right = vec![
        head(if weapon.in_hand {
            "strata.fitting.table.in_hand"
        } else {
            "strata.fitting.table.holstered"
        }),
        table(weapon),
    ];
    right.extend(moddability(weapon));

    // Each column is fixed in width and scrolls inside itself, so a long
    // list or a tall table moves nothing outside the column it is in.
    let scrolling = |layout: Layout, children: Vec<UiNodeDef>| UiNodeDef::Scroll {
        layout: Layout {
            align: Some(Align::Stretch),
            min_height: Some(Length::Px(0.)),
            ..layout
        },
        scrollbar: true,
        children,
        tags: Tags::new(),
    };
    let sized = |width: f32, children: Vec<UiNodeDef>| {
        scrolling(
            Layout {
                width: Some(Length::Px(width)),
                ..column(1.)
            },
            children,
        )
    };
    vec![
        sized(MOUNTS_WIDTH, mounts_column(weapon)),
        // Not a scroll of its own: slotted keeps a scroll's direct children
        // from shrinking, which would stop the parts list inside it from
        // ever scrolling. The list is the one thing here that gives.
        panel(
            "invisible",
            Layout {
                grow: 1.,
                min_width: Some(Length::Px(0.)),
                min_height: Some(Length::Px(0.)),
                align: Some(Align::Stretch),
                ..column(1.4)
            },
            parts_column(weapon),
        ),
        sized(TABLE_WIDTH, right),
    ]
}

/// The head: the workbench's two tabs, the weapon's name and maker, a tab
/// a hand, and at the right the weapon's condition on a bar with its
/// figure.
fn head_row(view: &FittingView) -> UiNodeDef {
    let mut head_row = vec![crafting::bench_tabs(WorkbenchTab::Fitting)];
    if let Some(weapon) = &view.weapon {
        head_row.push(with_id(
            text_in(weapon.name.clone(), TextRole::Title, "panel.title"),
            TITLE_ID,
        ));
        head_row.push(head(weapon.sub.clone()));
    }
    head_row.push(panel(
        "invisible",
        row(1.),
        view.tabs.iter().map(tab).collect(),
    ));
    head_row.push(spacer());
    if let Some(condition) = view.weapon.as_ref().and_then(|weapon| weapon.condition) {
        head_row.push(head("strata.fitting.condition"));
        head_row.push(panel(
            "invisible",
            Layout {
                width: Some(Length::Px(170.)),
                ..row(0.)
            },
            vec![
                BarParams {
                    ticks: Vec::new(),
                    numeral: false,
                    condition: true,
                    worn: view
                        .weapon
                        .as_ref()
                        .is_some_and(|weapon| weapon.band != Band::Sound),
                    ..BarParams::quality(condition)
                }
                .node(None),
            ],
        ));
        head_row.push(with_id(
            line_in(
                fill(
                    &fill(
                        words("strata.fitting.condition.value"),
                        "condition",
                        &condition.to_string(),
                    ),
                    "full",
                    &QUALITY_MAX.to_string(),
                ),
                FIGURE_ROLE,
            ),
            CONDITION_ID,
        ));
        if let Some(band) = view.weapon.as_ref().map(|weapon| weapon.band)
            && band != Band::Sound
        {
            // One word a band, so the header and the line under the
            // buttons never name one state two ways.
            let word = if band == Band::Worn {
                "strata.fitting.worn.word"
            } else {
                "strata.fitting.tired"
            };
            head_row.push(with_id(line_in(word, WHY_ROLE), TIRED_ID));
        }
    }
    panel(
        "invisible",
        Layout {
            align: Some(Align::Center),
            ..row(2.)
        },
        head_row,
    )
}

/// The foot: what the sim last said, or what the screen is for, then the
/// keys and Close.
fn footer(view: &FittingView) -> UiNodeDef {
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
    // The sim's last word has taken the line that said what the keys do,
    // so that is said under it.
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
            panel("invisible", column(0.2), said),
            spacer(),
            UiNodeDef::Custom {
                kind: WidgetKind::new("slotted:hint_bar"),
                params: slotted::registry::Value::Unit,
                children: Vec::new(),
                tags: Tags::new().with(Tags::TEST_ID, "fitting.hints"),
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
pub fn screen_def(view: &FittingView) -> ScreenDef {
    let columns = view.weapon.as_ref().map_or_else(
        || {
            vec![text_in(
                "strata.fitting.empty",
                TextRole::Body,
                "strata.card.note",
            )]
        },
        body,
    );
    ScreenDef {
        kind: kind(),
        inherits: None,
        root: UiNodeDef::Panel {
            role: Role::new("panel"),
            layout: Layout {
                width: Some(Length::Px(WIDTH)),
                max_width: Some(Length::Percent(96.)),
                min_height: Some(Length::Percent(SCREEN_HEIGHT_VH)),
                max_height: Some(Length::Percent(SCREEN_HEIGHT_VH)),
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
                    columns,
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

// ---- open, answer, rebuild ----

/// Whether the fitting screen is the one on top.
pub(super) fn on_top(stack: &ScreenStack) -> bool {
    stack.top().is_some_and(|top| top.kind == kind())
}

/// The view for the world as it is: the local player's weapons and stock,
/// the store, the reach. `None` without a player or the registries.
fn view_of(world: &mut World) -> Option<FittingView> {
    let registries = world.get_resource::<Registries>()?.0.clone();
    let items = &registries.items;
    // The sim's own rule, so the buttons are grey exactly when a request
    // would be refused for it.
    let at_workbench = world.get_resource::<Reach>().is_some_and(|reach| {
        let access = world.get_resource::<WorkbenchAccess>().copied();
        fitting::at_workbench(access.unwrap_or_default(), reach)
    });
    let mut players =
        world.query_filtered::<(&Pack, &Hands, Option<&Combatant>, Option<&CharacterSheet>), With<LocalPlayer>>();
    let (pack, hands, actor, sheet) = players.single(world).ok()?;
    let fallback = Combatant::default();
    let holder = actor.unwrap_or(&fallback);
    let records = hands.saved(items).unwrap_or_default();
    let bare = std::array::from_fn(|hand| {
        let family = records[hand]
            .as_ref()
            .and_then(|record| fitting::weapon_of(items, record));
        holder.bare(family.unwrap_or(holder.weapon))
    });
    let drawn = holder.hand();
    let pack = pack.clone();
    let sheet = sheet.cloned().unwrap_or_default();
    let empty = HouseholdStore::default();
    let store = world.get_resource::<HouseholdStore>().unwrap_or(&empty);
    // A picture is named only where there is a source to load it from, so
    // a run with an index and no files draws no broken image.
    let pictures = world
        .get_resource::<AssetServer>()
        .filter(|assets| assets.get_source(card::GENERATED_SOURCE).is_ok())
        .and(world.get_resource::<WeaponPictures>())
        .map(|pictures| &pictures.0);
    // The model is drawn where there is a stage to draw it on and the
    // session has not chosen the diagram; the switch is there either way.
    let well = world
        .get_resource::<WellView>()
        .copied()
        .unwrap_or_default();
    let models = world
        .get_resource::<WeaponStage>()
        .and(world.get_resource::<WeaponModels>())
        .map(|models| (&models.0, well));
    let state = world.get_resource::<FittingState>()?;
    Some(FittingView::of(
        &Bench {
            items,
            hands: &records,
            bare,
            holder,
            drawn,
            pack: &pack,
            store,
            at_workbench,
            rework: world.get_resource::<Reach>().map(|reach| ReworkBench {
                registries: &registries,
                access: world
                    .get_resource::<WorkbenchAccess>()
                    .copied()
                    .unwrap_or_default(),
                reach,
                sheet: &sheet,
                purse: world.get_resource::<Purse>().copied().unwrap_or_default(),
            }),
            pictures,
            symbols: Some(&registries.icons),
            models,
        },
        state,
    ))
}

/// Builds the view, keeps it as what the screen shows, and gives its
/// definition.
fn build(world: &mut World) -> Option<ScreenDef> {
    let view = view_of(world)?;
    let def = screen_def(&view);
    let mut state = world.resource_mut::<FittingState>();
    // The picks the view resolved are written back, so a choice acts on
    // the mount that is shown.
    state.hand = view.hand;
    state.mount = view.weapon.as_ref().map(|weapon| weapon.mount.clone());
    state.previewed = None;
    state.dirty = false;
    state.view = Some(view);
    Some(def)
}

/// Pushes the screen on the weapon in `hand`, or on the one drawn when no
/// hand is named, with `mount` in focus when the weapon has it; rebuilds it
/// in place when it is already on top. A workbench's F, a weapon's cell on
/// the stores screen and the flag for a still all come through here.
pub fn open(commands: &mut Commands, hand: Option<usize>) {
    open_at(commands, hand, None);
}

/// [`open`] with a mount in focus.
pub fn open_at(commands: &mut Commands, hand: Option<usize>, mount: Option<String>) {
    commands.queue(move |world: &mut World| {
        let drawn = world
            .query_filtered::<&Combatant, With<LocalPlayer>>()
            .single(world)
            .ok()
            .map(Combatant::hand);
        {
            let mut state = world.resource_mut::<FittingState>();
            state.hand = hand.or(drawn).unwrap_or(0);
            state.mount = mount;
            state.pick = Pick::Current;
            state.last = None;
            state.notice = None;
            state.pointer_moved = false;
            state.asked = 0;
        }
        let Some(def) = build(world) else {
            tracing::warn!("no local player carrying a weapon: nothing to fit");
            return;
        };
        // The workbench's other tab gives its place to this one: the two
        // are one screen to the player, and Back leaves both.
        *world.resource_mut::<WorkbenchTab>() = WorkbenchTab::Fitting;
        let stack = world.resource::<ScreenStack>();
        if on_top(stack) || crafting::on_top(stack) {
            replace_screen(&mut world.commands(), Arc::new(def), None);
        } else {
            push_screen(&mut world.commands(), Arc::new(def), None);
        }
    });
}

/// Picks the first part the stock has for the mount in focus, as Accept on
/// its row would, without fitting it: for a still of what a fit would do.
pub fn pick_first(commands: &mut Commands) {
    commands.queue(|world: &mut World| {
        let mut state = world.resource_mut::<FittingState>();
        let module = state
            .view
            .as_ref()
            .and_then(|view| view.weapon.as_ref())
            .and_then(|weapon| {
                weapon
                    .picks
                    .iter()
                    .filter(|row| row.id != PICK_FITTED)
                    .find_map(|row| row.module.clone())
            });
        if let Some(module) = module {
            state.pick = Pick::Module(module);
            state.dirty = true;
        }
    });
}

/// Puts what `request` asks in the sim's inbox, unless a request is still
/// waiting for its answer: a second Accept before the tick would be
/// refused as already done, over the fit it repeats.
fn ask(inbox: &mut FitInbox, asked: &mut usize, request: Option<FitRequest>) {
    if let Some(request) = request
        && *asked == 0
    {
        inbox.0.push(request);
        *asked += 1;
    }
}

/// Rework or Buy blank: the request the view worked out, unless one is still
/// waiting for its answer.
fn ask_rework(
    id: &str,
    state: &mut FittingState,
    rework_inbox: &mut ReworkInbox,
    craft_inbox: &mut CraftInbox,
) {
    let Some(rework) = state
        .view
        .as_ref()
        .and_then(|view| view.weapon.as_ref())
        .and_then(|weapon| weapon.rework.clone())
    else {
        return;
    };
    if state.asked != 0 {
        return;
    }
    if id == REWORK {
        if let Some(request) = rework.request {
            rework_inbox.0.push(request);
            state.asked += 1;
        }
    } else if let Some(request) = rework.buy {
        craft_inbox.0.push(request);
        state.asked += 1;
    }
}

/// `Update`: a tab, a mount, a part, Fit or Remove, and a mount picked on
/// the drawing. Nothing is decided here: a pick is the screen's own, and
/// Fit and Remove send the request the view worked out for the sim to
/// judge.
pub fn answer(
    mut choices: MessageReader<MenuChoice>,
    mut picked: MessageReader<MountPicked>,
    stack: Res<ScreenStack>,
    mut state: ResMut<FittingState>,
    mut inbox: ResMut<FitInbox>,
    (mut wear_inbox, mut rework_inbox, mut craft_inbox): (
        ResMut<WearInbox>,
        ResMut<ReworkInbox>,
        ResMut<CraftInbox>,
    ),
    mut well: ResMut<WellView>,
) {
    if !on_top(&stack) {
        choices.clear();
        picked.clear();
        return;
    }
    let chassis = state
        .view
        .as_ref()
        .and_then(|view| view.weapon.as_ref())
        .map(|weapon| weapon.chassis.clone());
    for pick in picked.read() {
        if Some(&pick.chassis) != chassis.as_ref() {
            continue;
        }
        state.mount = Some(pick.mount.clone());
        state.pick = Pick::Current;
        state.notice = None;
        state.last = Some(format!("{}{}", diagram::MOUNT_ID, pick.mount));
        state.dirty = true;
    }
    for choice in choices.read() {
        if choice.screen != kind() {
            continue;
        }
        let id = choice.id.as_str();
        if let Some(hand) = id.strip_prefix(HAND_TAG) {
            if let Ok(hand) = hand.parse::<usize>() {
                state.hand = hand;
                state.mount = None;
                state.pick = Pick::Current;
            }
        } else if let Some(mount) = id.strip_prefix(MOUNT_TAG) {
            state.mount = Some(mount.to_owned());
            state.pick = Pick::Current;
        } else if let Some(pick) = id.strip_prefix(PICK_TAG) {
            let Some(row) = state
                .view
                .as_ref()
                .and_then(|view| view.weapon.as_ref())
                .and_then(|weapon| weapon.picks.iter().find(|row| row.id == pick))
                .cloned()
            else {
                continue;
            };
            if row.on {
                // Accept on the part already picked fits it.
                ask(&mut inbox, &mut state.asked, row.request);
            } else {
                // A part that cannot be fitted from here is picked all the
                // same, so its figures stay in the AFTER column beside the
                // reason.
                state.pick = match (pick, row.module) {
                    (PICK_STANDARD, _) => Pick::Standard,
                    (PICK_FITTED, _) | (_, None) => Pick::Current,
                    (_, Some(module)) => Pick::Module(module),
                };
            }
        } else if id == FIT {
            let request = state
                .view
                .as_ref()
                .and_then(|view| view.weapon.as_ref())
                .and_then(|weapon| weapon.fit.clone());
            ask(&mut inbox, &mut state.asked, request);
        } else if id == REMOVE {
            let request = state
                .view
                .as_ref()
                .and_then(|view| view.weapon.as_ref())
                .and_then(|weapon| weapon.remove.clone());
            ask(&mut inbox, &mut state.asked, request);
        } else if id == WELL_SWITCH {
            *well = match *well {
                WellView::Model => WellView::Diagram,
                WellView::Diagram => WellView::Model,
            };
        } else if id == REPAIR {
            let request = state
                .view
                .as_ref()
                .and_then(|view| view.weapon.as_ref())
                .and_then(|weapon| weapon.repair.request);
            if let Some(request) = request
                && state.asked == 0
            {
                wear_inbox.0.push(request);
                state.asked += 1;
            }
        } else if id == REWORK || id == BUY_BLANK {
            ask_rework(id, &mut state, &mut rework_inbox, &mut craft_inbox);
        } else {
            continue;
        }
        state.notice = None;
        state.last = Some(id.to_owned());
        state.dirty = true;
    }
}

/// What the module now on `mount` of the weapon in `hand` does, as the
/// row that offered it said it: the property it is fitted for and the
/// figure the weapon now reads, `Magazine 29 rounds`, with the module's
/// name. The sim's own line; `None` when the hand holds no such thing.
fn fitted_words(
    items: &ItemCatalogue,
    hands: &Hands,
    actor: Option<&Combatant>,
    hand: usize,
    mount: &str,
) -> Option<(String, String)> {
    let record = hands.saved(items).ok()?.get(hand)?.clone()?;
    let module = record.fitted.get(mount)?;
    let name = items.get(&module.item)?.name.to_lowercase();
    let effect = fitting::lead(items, &module.item)?;
    let fallback = Combatant::default();
    let bare = actor
        .unwrap_or(&fallback)
        .bare(fitting::weapon_of(items, &record)?);
    let line = fitting::lines(items, &bare, &record, &record, &[effect.property])
        .first()
        .map(|line| CardEffect::of_for(line, bare.spec.mode))?;
    Some((name, format!("{} {}", line.name, line.value)))
}

/// `Update`: the sim's answer, worded in the footer and, for a refusal,
/// beside the button that asked. A fit is toasted with what the part does
/// to the weapon, in the words its row offered it by: the magazine the HUD
/// now shows for a feed, the draw for a grip.
///
/// A fit or a removal that was done is not then covered by the refusal of
/// its own echo (the same request asked twice in one tick is done once and
/// refused once as already done).
pub fn hear(
    mut answers: MessageReader<FitAnswer>,
    registries: Option<Res<Registries>>,
    players: Query<(&Hands, Option<&Combatant>), With<LocalPlayer>>,
    mut state: ResMut<FittingState>,
    mut commands: Commands,
) {
    let mut done = false;
    for answer in answers.read() {
        state.asked = state.asked.saturating_sub(1);
        let mount_word = diagram::words(&answer.mount).to_lowercase();
        let notice = match answer.outcome {
            Ok(FitDone::Fitted { .. }) => {
                let fitted = registries.as_ref().zip(players.single().ok()).and_then(
                    |(registries, (hands, actor))| {
                        fitted_words(
                            &registries.0.items,
                            hands,
                            actor,
                            answer.hand,
                            &answer.mount,
                        )
                    },
                );
                let status = fill(
                    words("strata.fitting.status.fitted"),
                    "name",
                    fitted.as_ref().map_or(&mount_word, |(name, _)| name),
                );
                super::progress::notice(
                    &mut commands,
                    fitted.map_or_else(
                        || status.clone(),
                        |(_, line)| fill(words("strata.fitting.toast.fitted"), "line", &line),
                    ),
                );
                (status, false)
            }
            Ok(FitDone::Removed) => (
                fill(words("strata.fitting.status.removed"), "mount", &mount_word),
                false,
            ),
            Err(FitRefusal::AlreadyFitted | FitRefusal::NothingFitted) if done => continue,
            Err(refusal) => (refusal_words(refusal), true),
        };
        tracing::info!("fitting: {}", notice.0);
        if answer.outcome.is_ok() {
            done = true;
            state.pick = Pick::Current;
        }
        state.notice = Some(notice);
        state.dirty = true;
    }
}

/// `Update`: the sim's answer to a repair, worded in the footer and as a
/// toast. Any screen may have asked, so the toast is not tied to this one
/// being on top.
pub fn hear_repair(
    mut answers: MessageReader<RepairAnswer>,
    mut state: ResMut<FittingState>,
    mut commands: Commands,
) {
    for answer in answers.read() {
        state.asked = state.asked.saturating_sub(1);
        let notice = match answer.result {
            Ok(done) => {
                let status = fill(
                    words("strata.fitting.toast.repaired"),
                    "gain",
                    &done.gain.to_string(),
                );
                super::progress::notice(&mut commands, status.clone());
                (status, false)
            }
            Err(refusal) => (repair_refusal_words(refusal), true),
        };
        tracing::info!("fitting: {}", notice.0);
        state.notice = Some(notice);
        state.dirty = true;
    }
}

/// `Update`: the sim's answer to a rework, worded in the footer and as a
/// toast, with the mounts it opened named from the screen's own view.
pub fn hear_rework(
    mut answers: MessageReader<ReworkAnswer>,
    mut state: ResMut<FittingState>,
    mut commands: Commands,
) {
    for answer in answers.read() {
        state.asked = state.asked.saturating_sub(1);
        let notice = match &answer.result {
            Ok(done) => {
                let mut status = fill(
                    &fill(
                        words("strata.fitting.toast.reworked"),
                        "from",
                        &done.from.to_string(),
                    ),
                    "to",
                    &done.to.to_string(),
                );
                for key in &done.opened {
                    let name = state
                        .view
                        .as_ref()
                        .and_then(|view| view.weapon.as_ref())
                        .and_then(|weapon| weapon.mounts.iter().find(|mount| &mount.key == key))
                        .map_or_else(|| key.clone(), |mount| mount.label.to_lowercase());
                    status.push_str(&fill(
                        words("strata.fitting.toast.reworked.opened"),
                        "mount",
                        &name,
                    ));
                }
                super::progress::notice(&mut commands, status.clone());
                (status, false)
            }
            Err(refusal) => (rework_refusal_words(refusal), true),
        };
        tracing::info!("fitting: {}", notice.0);
        state.notice = Some(notice);
        state.dirty = true;
    }
}

/// `Update`: the bench's answer to a Buy blank, which frees the button and
/// rebuilds the view from the stock and the purse as they now are. The
/// crafting tab words its own purchases.
pub fn hear_buy(mut answers: MessageReader<CraftAnswer>, mut state: ResMut<FittingState>) {
    for answer in answers.read() {
        if matches!(answer.request, CraftRequest::Buy { .. }) {
            state.asked = state.asked.saturating_sub(1);
            state.dirty = true;
        }
    }
}

/// `Update`: rebuilds the open screen when the view has changed under it
/// (a fit landed, the stock moved, the character stepped away from the
/// bench) or a choice marked it. A screen pushed this frame is left alone
/// unless it was marked: a pop in the frame of its push races the stack's
/// own deferred work on the new tree, and a choice can only come from a
/// screen that has been up a frame.
pub fn rebuild(world: &mut World, mut was_on_top: Local<bool>) {
    let settled = *was_on_top;
    *was_on_top = on_top(world.resource::<ScreenStack>());
    if !*was_on_top {
        return;
    }
    let dirty = world.resource::<FittingState>().dirty;
    if !dirty {
        if !settled {
            return;
        }
        let Some(view) = view_of(world) else {
            return;
        };
        if world.resource::<FittingState>().view.as_ref() == Some(&view) {
            return;
        }
    }
    if let Some(def) = build(world) {
        replace_screen(&mut world.commands(), Arc::new(def), None);
    }
}

/// `Update`, after the rebuild: the AFTER column and the line beside the
/// buttons follow the part under the keyboard's focus or the pointer, and
/// go back to the part picked when neither is on one. Written to the nodes
/// in place: the part is only looked at, and nothing else on the screen
/// changes for it. The pointer looks at a part only once it has moved
/// since the screen opened, as on the stores screen: a row that comes up
/// under a parked pointer was not pointed at.
#[allow(
    clippy::too_many_arguments,
    reason = "the screen, the device in use, the focus, the pointer and its motion, and the nodes"
)]
pub fn write_preview(
    stack: Res<ScreenStack>,
    mode: Res<InputMode>,
    mut cursor: MessageReader<CursorMoved>,
    mut inputs: MessageReader<PointerInput>,
    focus: Option<Res<InputFocus>>,
    mut state: ResMut<FittingState>,
    ids: Query<(Entity, &TestId)>,
    hovered: Query<(&Hovered, &TestId)>,
    mut texts: Query<(&mut LocText, &mut Text, &mut Themed)>,
    mut boxes: Query<(&TestId, &mut Node)>,
) {
    // Read whether or not the screen is up, so the motion that led to the
    // click that opened it is not taken for motion over it. A window
    // reports its cursor, a harness only the pointer's own input.
    let moved = cursor.read().count()
        + inputs
            .read()
            .filter(|input| match input.action {
                PointerAction::Move { delta } => delta != Vec2::ZERO,
                _ => false,
            })
            .count();
    if !on_top(&stack) {
        return;
    }
    if moved > 0 && !state.pointer_moved {
        state.pointer_moved = true;
    }
    let pointing = state.pointer_moved;
    let Some(weapon) = state.view.as_ref().and_then(|view| view.weapon.as_ref()) else {
        return;
    };
    let pick_of = |id: &TestId| id.0.strip_prefix(PICK_TAG).map(str::to_owned);
    let looked_at = if *mode == InputMode::Pointer {
        hovered
            .iter()
            .find(|(hovered, id)| pointing && hovered.get() && id.0.starts_with(PICK_TAG))
            .and_then(|(_, id)| pick_of(id))
    } else {
        focus
            .as_deref()
            .and_then(InputFocus::get)
            .and_then(|entity| ids.get(entity).ok())
            .and_then(|(_, id)| pick_of(id))
    };
    let picked = weapon.picks.iter().find(|row| row.on).map(|row| &row.id);
    let shown = looked_at.as_ref().or(picked).cloned().unwrap_or_default();
    if state.previewed.as_deref() == Some(shown.as_str()) {
        return;
    }
    // The part looked at, when it is another than the one picked; else
    // what the definition was built with.
    let preview = looked_at
        .as_ref()
        .filter(|id| Some(*id) != picked)
        .and_then(|id| weapon.picks.iter().find(|row| row.id == *id))
        .map_or(&weapon.preview, |row| &row.preview);
    let (why, blocked) = if preview.why.is_empty() {
        (&weapon.preview.why, weapon.preview.blocked)
    } else {
        (&preview.why, preview.blocked)
    };
    let find = |id: &str| ids.iter().find(|(_, test)| test.0 == id).map(|(e, _)| e);
    let mut written = true;
    let mut write = |id: &str, words: &str, role: &'static str| {
        let Some((mut loc, mut text, mut themed)) =
            find(id).and_then(|entity| texts.get_mut(entity).ok())
        else {
            written = false;
            return;
        };
        if loc.key.0 != words {
            loc.key = LocKey(words.to_owned());
        }
        if text.0 != words {
            words.clone_into(&mut text.0);
        }
        if themed.0.as_str() != role {
            themed.0 = Role::new_static(role);
        }
    };
    for (row, after) in weapon.rows.iter().zip(&preview.after) {
        write(&cell_id(row.property, "after"), &after.0, CELL_AFTER_ROLE);
        write(
            &cell_id(row.property, "change"),
            &after.1,
            change_role(after.2),
        );
    }
    write(WHY_ID, why, why_role(blocked));
    // The column is there while it has a figure in it, and its heading
    // with it.
    let column = after_shown(preview);
    write(AFTER_HEAD_ID, after_heading(column), HEAD_ROLE);
    for (id, mut node) in &mut boxes {
        if let Some(width) = column_width(&id.0, column)
            && node.width != Val::Px(width)
        {
            node.width = Val::Px(width);
        }
    }
    // A screen replaced this frame has not spawned its nodes yet; the
    // words are written when it has.
    if written {
        state.previewed = Some(shown);
    }
}

/// The record the AFTER column is showing, for the stage: the weapon as
/// it is, or as it would be with the part under focus on the mount. The
/// sim's `preview` gives it; nothing is worked out here.
#[must_use]
pub fn staged_record(
    items: &ItemCatalogue,
    weapon: &Record,
    mount: &str,
    row: &PickView,
) -> Record {
    match (row.id.as_str(), &row.module) {
        (PICK_FITTED, _) => weapon.clone(),
        (_, Some(module)) => fitting::preview(items, weapon, mount, Some(module)),
        (_, None) => fitting::preview(items, weapon, mount, None),
    }
}

/// `Update`, after the preview is written: the stage is asked for the
/// weapon the AFTER column shows while the fitting screen is the one on
/// top with its well on the model, and released when it leaves, whichever
/// way, or draws the diagram. The pieces are named from the record by the
/// one function the hand's model is.
pub fn show_on_stage(
    stack: Res<ScreenStack>,
    fitting: Res<FittingState>,
    stage: Option<ResMut<WeaponStage>>,
    models: Option<Res<WeaponModels>>,
    registries: Option<Res<Registries>>,
    players: Query<&Hands, With<LocalPlayer>>,
) {
    let Some(mut stage) = stage else {
        return;
    };
    let shown = on_top(&stack)
        .then_some(fitting.view.as_ref())
        .flatten()
        .and_then(|view| Some((view, view.weapon.as_ref().filter(|weapon| weapon.model)?)));
    let wanted =
        shown
            .zip(models)
            .zip(registries)
            .and_then(|(((view, weapon), models), registries)| {
                let items = &registries.0.items;
                let record = players.single().ok()?.saved(items).ok()?[view.hand].clone()?;
                // What write_preview last wrote, else the part picked; a row
                // that is gone shows the weapon as it is.
                let id = fitting.previewed.as_deref().or_else(|| {
                    weapon
                        .picks
                        .iter()
                        .find(|row| row.on)
                        .map(|row| row.id.as_str())
                });
                let record = id
                    .and_then(|id| weapon.picks.iter().find(|row| row.id == id))
                    .map_or_else(
                        || record.clone(),
                        |row| staged_record(items, &record, &weapon.mount, row),
                    );
                weapon_model::wanted(&models.0, items, &record)
            });
    if stage.wanted() != wanted.as_ref() {
        stage.show(wanted);
    }
}

/// `Update`: a part's picture is drawn in its item's colour, as its cell
/// on the stores screen is. A baked picture is tone and coverage with no
/// colour of its own, and the image node that draws it starts white, so
/// the tint is held on it.
pub fn tint_icons(
    stack: Res<ScreenStack>,
    state: Res<FittingState>,
    mut icons: Query<(&TestId, &mut ImageNode)>,
) {
    if !on_top(&stack) {
        return;
    }
    let Some(weapon) = state.view.as_ref().and_then(|view| view.weapon.as_ref()) else {
        return;
    };
    for (id, mut image) in &mut icons {
        let Some(row) =
            id.0.strip_prefix(ICON_TAG)
                .and_then(|pick| weapon.picks.iter().find(|row| row.id == pick))
        else {
            continue;
        };
        let [red, green, blue] = row.tint;
        let tint = bevy::color::Color::srgb_u8(red, green, blue);
        if image.color != tint {
            image.color = tint;
        }
    }
}

/// Registers the state and the systems. Only called once the UI is wired.
pub(crate) fn register(app: &mut bevy::app::App) {
    app.init_resource::<FittingState>()
        .init_resource::<WellView>()
        .add_systems(
            bevy::app::Update,
            (
                answer,
                hear,
                hear_repair,
                hear_rework,
                hear_buy,
                rebuild,
                write_preview,
                show_on_stage,
                tint_icons,
            )
                .chain()
                .in_set(super::ScreenWork),
        );
}

#[cfg(test)]
mod tests {
    use super::*;
    use strata_registry::GameRegistries;
    use strata_sim::combat::Weapon;

    const FEED: &str = "veyra:module.patrol_extended_feed";
    const CELL: &str = "veyra:module.pistol_extended_cell";
    const REFLEX: &str = "veyra:module.vigil_reflex";
    const GRIP: &str = "veyra:module.vigil_angled_grip";

    /// The first day, without a world: the shipped registries, the weapons
    /// of `starting.ron` in the hands of somebody with no boxes, the
    /// arrival's pack and the household's store.
    struct Day {
        registries: GameRegistries,
        hands: [Option<Record>; HANDS],
        pack: Pack,
        store: HouseholdStore,
    }

    fn day() -> Day {
        let assets = concat!(env!("CARGO_MANIFEST_DIR"), "/../../assets");
        let registries = strata_registry::load_from_dir(assets).expect("assets/ loads");
        let items = &registries.items;
        let starting = items.starting().expect("the first day's stock");
        let arrival = &registries
            .sheets
            .household
            .as_ref()
            .expect("the household's sheets")
            .arrival;
        let pack = Pack::from_saved(&starting.packs[&arrival.name], items).expect("a pack");
        let store: Vec<_> = starting.store.iter().cloned().map(Some).collect();
        let store = HouseholdStore::from_saved(&store, items).expect("a store");
        let hands = starting.hands.clone().map(Some);
        Day {
            registries,
            hands,
            pack,
            store,
        }
    }

    impl Day {
        fn view(&self, state: &FittingState, at_workbench: bool) -> FittingView {
            let holder = Combatant::default();
            FittingView::of(
                &Bench {
                    items: &self.registries.items,
                    hands: &self.hands,
                    bare: std::array::from_fn(|hand| {
                        let family = self.hands[hand].as_ref().and_then(|record| {
                            strata_sim::fitting::weapon_of(&self.registries.items, record)
                        });
                        holder.bare(family.unwrap_or([Weapon::Carbine, Weapon::Pistol][hand]))
                    }),
                    holder: &holder,
                    drawn: 0,
                    pack: &self.pack,
                    store: &self.store,
                    at_workbench,
                    rework: None,
                    pictures: None,
                    symbols: None,
                    models: None,
                },
                state,
            )
        }

        /// A bought example of `module`, as a request names it.
        fn bought(&self, module: &str) -> Fitted {
            let id = strata_registry::Namespaced::parse(module).expect("a valid id");
            let def = self.registries.items.get(&id).expect("a known module");
            Fitted {
                item: def.id.clone(),
                batch: 0,
                by: None,
                lines: def
                    .lines
                    .iter()
                    .map(|line| (line.id.clone(), strata_registry::item::STORE_GRADE))
                    .collect(),
                condition: None,
            }
        }
    }

    /// A store grade record of the melee item `id`.
    fn bought_weapon(day: &Day, id: &str) -> Record {
        let id = strata_registry::Namespaced::parse(id).expect("an id");
        let def = day.registries.items.get(&id).expect("the weapon");
        Record {
            item: def.id.clone(),
            count: 1,
            batch: 0,
            by: None,
            lines: def
                .lines
                .iter()
                .map(|line| (line.id.clone(), strata_registry::item::STORE_GRADE))
                .collect(),
            condition: None,
            fitted: std::collections::BTreeMap::new(),
            origin: None,
        }
    }

    /// Puts a bought `module` in the store.
    fn stock(day: &mut Day, module: &str) {
        let part = day.bought(module);
        let record = Record {
            item: part.item,
            count: 1,
            batch: 0,
            by: None,
            lines: part.lines,
            condition: None,
            fitted: std::collections::BTreeMap::new(),
            origin: None,
        };
        let stack = strata_sim::items::write(&record, &day.registries.items).expect("a stack");
        let cell = day.store.0.first_empty().expect("room in the store");
        day.store.0.set(cell, Some(stack));
    }

    /// The rifle at the bench with a long optic in the store: five mounts,
    /// the aim's time among the facts, and the part as a candidate whose
    /// AFTER column the sim fills.
    #[test]
    fn the_rifle_shows_its_five_mounts_its_aim_and_a_long_optic_as_a_candidate() {
        let mut day = day();
        day.hands[0] = Some(bought_weapon(&day, "veyra:weapon.marksman_rifle"));
        stock(&mut day, "veyra:module.vigil_long_optic");
        let view = day.view(&on("sight"), true);
        let rifle = weapon(&view);
        let keys: Vec<&str> = rifle.mounts.iter().map(|m| m.key.as_str()).collect();
        assert_eq!(keys, ["sight", "feed", "stock", "muzzle", "support"]);
        assert!(
            rifle
                .facts
                .iter()
                .any(|(name, value)| name == "Aim" && value.starts_with("Settles in")),
            "{:?}",
            rifle.facts
        );
        let optic = rifle
            .picks
            .iter()
            .find(|p| p.name == "Vigil long optic")
            .expect("the long optic is a candidate")
            .id
            .clone();
        let aim = rifle
            .rows
            .iter()
            .position(|row| row.property == Property::AimSpread)
            .expect("an aim spread row");
        let after = &pick(&view, &optic).preview.after[aim];
        assert_eq!(after.2, Some(true), "a tighter aim is better");
        assert!(!after.1.is_empty(), "the change is worked out");
    }

    /// A greatblade in hand: its cuts stand beneath a table, and a bought
    /// one has two of its three mounts open.
    #[test]
    fn a_melee_weapon_shows_its_table_its_cuts_and_its_mounts() {
        let mut day = day();
        day.hands[0] = Some(bought_weapon(&day, "veyra:weapon.yard_greatblade"));
        let view = day.view(&FittingState::default(), false);
        let greatblade = weapon(&view);
        let keys: Vec<&str> = greatblade.mounts.iter().map(|m| m.key.as_str()).collect();
        assert_eq!(keys, ["head", "grip", "power"]);
        assert_eq!(mount(&view, "power").look, MountLook::Shut);
        assert_eq!(mount(&view, "power").sub, "Opens at moddability 600");
        assert_eq!(greatblade.open, "2 of 3 open");
        let facts: Vec<&str> = greatblade.facts.iter().map(|(n, _)| n.as_str()).collect();
        assert_eq!(facts.first(), Some(&"Light cut damage"));
        assert_eq!(facts.last(), Some(&"Reach"));
        assert!(!facts.contains(&"Guard takes"), "the class's, not a part's");
        let names: Vec<&str> = greatblade.rows.iter().map(|r| r.name.as_str()).collect();
        assert!(names.contains(&"Recovery"), "{names:?}");
        assert!(
            screen_def(&view).root.find_mut(NO_MOUNTS_ID).is_none(),
            "a weapon with mounts says no such thing"
        );
    }

    /// A blade at the bench with a hooked edge in the store: the candidate
    /// in focus fills the AFTER column from the sim's lines.
    #[test]
    fn a_blades_candidate_fills_the_after_column() {
        let mut day = day();
        day.hands[0] = Some(bought_weapon(&day, "veyra:weapon.service_blade"));
        stock(&mut day, "veyra:module.blade_hooked_edge");
        let view = day.view(&on("edge"), true);
        let ids: Vec<&str> = weapon(&view).picks.iter().map(|p| p.id.as_str()).collect();
        assert_eq!(ids, [PICK_STANDARD, "0"]);
        let posture = &weapon(&view)
            .rows
            .iter()
            .position(|row| row.property == Property::HitPosture)
            .expect("a posture row");
        let after = &pick(&view, "0").preview.after[*posture];
        assert_eq!(after.2, Some(true), "more posture is better");
        assert!(!after.1.is_empty(), "the change is worked out");
        let recovery = weapon(&view)
            .rows
            .iter()
            .position(|row| row.property == Property::RecoveryTime)
            .expect("a recovery row");
        assert!(
            weapon(&view).rows[recovery].live,
            "recovery is in the fight"
        );
        assert!(
            weapon(&view).fit.is_none(),
            "nothing picked, nothing to fit yet"
        );
    }

    fn on(mount: &str) -> FittingState {
        FittingState {
            mount: Some(mount.to_owned()),
            ..FittingState::default()
        }
    }

    fn weapon(view: &FittingView) -> &WeaponView {
        view.weapon.as_ref().expect("a weapon in the hand")
    }

    fn mount<'a>(view: &'a FittingView, key: &str) -> &'a MountView {
        weapon(view)
            .mounts
            .iter()
            .find(|row| row.key == key)
            .unwrap_or_else(|| panic!("no {key} mount"))
    }

    fn pick<'a>(view: &'a FittingView, id: &str) -> &'a PickView {
        weapon(view)
            .picks
            .iter()
            .find(|row| row.id == id)
            .unwrap_or_else(|| panic!("no pick {id}"))
    }

    /// The words of the node `id` in the definition of `view`, and for a
    /// button whether it is grey.
    fn shown(view: &FittingView, id: &str) -> (String, bool) {
        let mut def = screen_def(view);
        match def.root.find_mut(id) {
            Some(UiNodeDef::Text { key, .. }) => (key.0.clone(), false),
            Some(UiNodeDef::Button { opts, .. }) => (
                opts.label.clone().map_or_else(String::new, |label| label.0),
                opts.disabled,
            ),
            other => panic!("{id} is {other:?}"),
        }
    }

    /// The role of the text `id`.
    fn role_of(view: &FittingView, id: &str) -> String {
        let mut def = screen_def(view);
        match def.root.find_mut(id) {
            Some(UiNodeDef::Text { opts, .. }) => opts
                .role
                .as_ref()
                .map_or_else(String::new, |role| role.as_str().to_owned()),
            other => panic!("{id} is {other:?}"),
        }
    }

    /// Whether the box `id` of the AFTER column is laid out with its
    /// width, as against none.
    fn open(view: &FittingView, id: &str) -> bool {
        let mut def = screen_def(view);
        match def.root.find_mut(id) {
            Some(UiNodeDef::Panel { layout, .. }) => match layout.width {
                Some(Length::Px(width)) => {
                    assert!(width <= 0. || width >= AFTER_WIDTH.min(CHANGE_WIDTH));
                    width > 0.
                }
                other => panic!("{id} is {other:?} wide"),
            },
            other => panic!("{id} is {other:?}"),
        }
    }

    /// Every line of text under the node `id`, in order.
    fn said_in(view: &FittingView, id: &str) -> Vec<String> {
        let mut def = screen_def(view);
        let mut said = Vec::new();
        def.root
            .find_mut(id)
            .unwrap_or_else(|| panic!("no {id}"))
            .walk(&mut |node| {
                if let UiNodeDef::Text { key, .. } = node {
                    said.push(key.0.clone());
                }
            });
        said
    }

    fn after(view: &FittingView, property: Property) -> (String, String, Option<bool>) {
        let weapon = weapon(view);
        let at = weapon
            .rows
            .iter()
            .position(|row| row.property == property)
            .expect("a row of the table");
        weapon.preview.after[at].clone()
    }

    /// A bought carbine's moddability is 500: the stock is open at 400,
    /// the muzzle and the fore end are not, and each says at its own row
    /// when it opens. With a shut mount in focus both buttons are grey and
    /// the line beside them says the same thing.
    #[test]
    fn a_shut_mount_says_when_it_opens() {
        let day = day();
        let view = day.view(&on("muzzle"), true);
        assert_eq!(weapon(&view).open, "4 of 6 open");
        for (key, name, sub) in [
            ("sight", "Standard", "The part it was built with"),
            ("stock", "Standard", "The part it was built with"),
            ("muzzle", "Shut", "Opens at moddability 600"),
            ("fore_end", "Shut", "Opens at moddability 800"),
        ] {
            let row = mount(&view, key);
            assert_eq!((row.name.as_str(), row.sub.as_str()), (name, sub), "{key}");
        }
        assert_eq!(mount(&view, "muzzle").look, MountLook::Shut);
        assert_eq!(mount(&view, "muzzle").label, "MUZZLE");
        assert_eq!(mount(&view, "fore_end").label, "FORE END");
        assert!(mount(&view, "muzzle").on);
        assert_eq!(weapon(&view).fit, None);
        assert_eq!(weapon(&view).remove, None);
        assert_eq!(shown(&view, WHY_ID).0, "Opens at moddability 600");
        // A reason nothing can be done here, in the refusal's colour.
        assert!(weapon(&view).preview.blocked);
        assert_eq!(role_of(&view, WHY_ID), WHY_ROLE);
        assert_eq!(said_in(&view, &mount_id("muzzle"))[0], "\u{203a} MUZZLE");
        assert!(shown(&view, FIT).1 && shown(&view, REMOVE).1);
        assert!(
            screen_def(&view)
                .root
                .find_mut(&mount_id("muzzle"))
                .is_some(),
            "the row is found by its mount"
        );
        assert_eq!(
            pick(&view, PICK_STANDARD).preview.why,
            "Opens at moddability 600"
        );
        // The store holds a vented muzzle, so there is something to say it
        // is for, though the shut mount cannot take it.
        assert_eq!(weapon(&view).none, None);
        assert_eq!(
            shown(&view, THRESHOLDS_ID).0,
            "A mount opens at 400, 600 and 800."
        );
        assert_eq!(weapon(&view).moddability, Some((500, vec![400, 600, 800])));
        // The pistol's sight opens at 400, which a bought one has passed.
        let pistol = day.view(
            &FittingState {
                hand: 1,
                ..FittingState::default()
            },
            true,
        );
        assert_eq!(weapon(&pistol).open, "3 of 3 open");
        assert_eq!(weapon(&pistol).mount, "feed", "the feed is where it opens");
    }

    /// Away from a workbench the screen is a reading: the figures are all
    /// there, the store is out of reach, and both buttons are grey with the
    /// reason beside them and at every part.
    #[test]
    fn away_from_a_workbench_the_buttons_say_why() {
        let day = day();
        let view = day.view(&on("feed"), false);
        assert_eq!(
            view.status,
            (
                "A reading. Fitting is done at a workbench.".to_owned(),
                false
            )
        );
        assert_eq!(shown(&view, FITS_ID).0, "FITS THE FEED, FROM THE PACK");
        assert_eq!(
            weapon(&view).none.as_deref(),
            Some("No feed mod in the pack."),
            "the store's magazines are not in reach"
        );
        assert_eq!(shown(&view, WHY_ID).0, "Fitting is done at a workbench");
        assert!(shown(&view, FIT).1 && shown(&view, REMOVE).1);
        assert_eq!(
            shown(&view, &cell_id(Property::Capacity, "now")).0,
            "24 rounds"
        );
        // The pistol's cell is carried, so it is listed with what it would
        // do, and says at its own row why it cannot be fitted here.
        let state = FittingState {
            hand: 1,
            ..on("feed")
        };
        let pistol = day.view(&state, false);
        let cell = pick(&pistol, "0");
        assert_eq!(cell.name, "Pistol extended cell");
        assert_eq!(cell.sub, "Store grade · Capacity 500 · 1 carried");
        assert_eq!(
            cell.lead.as_ref().map(|lead| (
                lead.name.as_str(),
                lead.value.as_str(),
                lead.change.as_str()
            )),
            Some(("Magazine", "13 rounds", "+3"))
        );
        assert_eq!(cell.preview.why, "Fitting is done at a workbench");
        assert_eq!(cell.request, None);
        // A fitted weapon read away from the bench cannot be stripped
        // there either.
        let mut fitted = day;
        let feed = fitted.bought(FEED);
        fitted.hands[0]
            .as_mut()
            .expect("the carbine")
            .fitted
            .insert("feed".to_owned(), feed);
        let view = fitted.view(&on("feed"), false);
        assert_eq!(weapon(&view).remove, None);
        assert_eq!(shown(&view, WHY_ID).0, "Fitting is done at a workbench");
    }

    #[test]
    fn the_stage_shows_the_record_the_after_column_shows() {
        let mut day = day();
        let weapon_now = day.hands[0].clone().expect("a carbine");
        let items = &day.registries.items;
        let sight = |record: &Record| record.fitted.get("sight").map(|f| f.item.to_string());

        // Nothing fitted: the standard part is the weapon as it is, and a
        // candidate under focus puts itself on the mount.
        let view = day.view(&on("sight"), true);
        let rows = &weapon(&view).picks;
        let as_is = staged_record(items, &weapon_now, "sight", pick(&view, PICK_STANDARD));
        assert_eq!(as_is, weapon_now);
        let previewed = staged_record(items, &weapon_now, "sight", &rows[1]);
        assert_eq!(sight(&previewed).as_deref(), Some(REFLEX));
        assert_eq!(sight(&weapon_now), None, "the weapon itself is untouched");

        // The reflex fitted: the part on show is the weapon as it is, and
        // the standard row is a removal, the standard part back.
        let fitted = day.bought(REFLEX);
        day.hands[0]
            .as_mut()
            .expect("a carbine")
            .fitted
            .insert("sight".to_owned(), fitted);
        let weapon_now = day.hands[0].clone().expect("a carbine");
        let view = day.view(&on("sight"), true);
        let kept = staged_record(items, &weapon_now, "sight", pick(&view, PICK_FITTED));
        assert_eq!(kept, weapon_now);
        let removed = staged_record(items, &weapon_now, "sight", pick(&view, PICK_STANDARD));
        assert_eq!(sight(&removed), None);
    }

    /// At a workbench the store's magazines are listed best first, each
    /// with what it would do in units; picking one fills the AFTER column
    /// and lights Fit, and the fitted weapon reads its new row against the
    /// bare one.
    #[test]
    #[allow(
        clippy::too_many_lines,
        reason = "one weapon followed from bare through picked to fitted"
    )]
    fn a_picked_feed_fills_the_after_column_and_a_fitted_one_the_now() {
        let mut day = day();
        let view = day.view(&on("feed"), true);
        assert_eq!(
            view.status.0,
            "At the workbench: pick a mount, then a part. Enter picks, Enter again fits."
        );
        assert_eq!(
            shown(&view, FITS_ID).0,
            "FITS THE FEED, FROM THE PACK AND THE STORE"
        );
        let names: Vec<&str> = weapon(&view)
            .rows
            .iter()
            .map(|row| row.name.as_str())
            .collect();
        assert_eq!(
            names,
            [
                "Magazine",
                "Reload",
                "Pause between bursts",
                "Draw",
                "Aimed pace",
                "Aimed spread",
                "Recoil"
            ],
            "the live rows, then what the carbine's parts change that is not in the fight yet"
        );
        let live: Vec<bool> = weapon(&view).rows.iter().map(|row| row.live).collect();
        assert_eq!(live, [true, true, true, true, true, true, false]);
        for (property, now) in [
            (Property::Capacity, "24 rounds"),
            (Property::ReloadTime, "1.35 s"),
            (Property::RepeatInterval, "0.462 s"),
            (Property::EquipTime, "0.45 s"),
            (Property::AimSpread, "2.00°"),
        ] {
            assert_eq!(shown(&view, &cell_id(property, "now")).0, now);
            assert_eq!(shown(&view, &cell_id(property, "bare")).0, now);
            assert_eq!(shown(&view, &cell_id(property, "after")).0, "");
        }
        // The standard part is what is there; the crafted batch, whose
        // capacity line is the higher, comes before the bought one.
        let ids: Vec<&str> = weapon(&view)
            .picks
            .iter()
            .map(|row| row.id.as_str())
            .collect();
        assert_eq!(ids, [PICK_STANDARD, "0", "1"]);
        assert!(pick(&view, PICK_STANDARD).on);
        assert_eq!(pick(&view, PICK_STANDARD).name, "Standard feed");
        assert_eq!(pick(&view, PICK_STANDARD).sub, "Fitted now");
        // The two read the same in rounds, so each says the quality of the
        // line that orders them.
        assert_eq!(
            pick(&view, "0").sub,
            "Batch 412 · Capacity 610 · 2 in store"
        );
        assert_eq!(
            pick(&view, "1").sub,
            "Store grade · Capacity 500 · 4 in store"
        );
        assert_eq!(pick(&view, "1").tint, [0x7F, 0xD1, 0xFF]);
        assert_eq!(pick(&view, PICK_STANDARD).tint, [0xC6, 0xD0, 0xDD]);
        for id in ["0", "1"] {
            let row = pick(&view, id);
            assert_eq!(row.name, "Patrol extended magazine");
            assert_eq!(
                row.lead.as_ref().map(|lead| (
                    lead.name.as_str(),
                    lead.value.as_str(),
                    lead.change.as_str(),
                    lead.better
                )),
                Some(("Magazine", "29 rounds", "+5", Some(true)))
            );
            assert_eq!(
                row.preview.after[..2],
                [
                    ("29 rounds".to_owned(), "+5".to_owned(), Some(true)),
                    ("1.49 s".to_owned(), "+0.14 s worse".to_owned(), Some(false)),
                ]
            );
            assert!(row.preview.why.is_empty());
            assert!(matches!(row.request, Some(FitRequest::Fit { hand: 0, .. })));
        }
        // Nothing picked: Fit has nothing to act on and says so, and so
        // does Remove.
        assert_eq!(weapon(&view).fit, None);
        assert_eq!(
            shown(&view, WHY_ID).0,
            "Pick a part to fit · Nothing fitted here"
        );
        // A word on what to do next, not a refusal: it is not said in the
        // refusal's colour before the player has done anything.
        assert!(!weapon(&view).preview.blocked);
        assert_eq!(role_of(&view, WHY_ID), WHY_HINT_ROLE);
        assert!(shown(&view, FIT).1 && shown(&view, REMOVE).1);
        assert_eq!(view.focus.as_deref(), Some("fitting.mount:feed"));
        // With nothing to show in it the AFTER column has no heading and
        // no width, so NOW stands beside BARE.
        assert_eq!(shown(&view, AFTER_HEAD_ID).0, "");
        for column in ["after", "change"] {
            for of in ["head", "capacity", "aim_spread"] {
                assert!(!open(&view, &column_id(column, of)), "{column} {of}");
            }
        }
        // The mount in focus and the weapon shown are marked, not only lit.
        assert_eq!(said_in(&view, &mount_id("feed"))[0], "\u{203a} FEED");
        assert_eq!(said_in(&view, &mount_id("grip"))[0], "GRIP");
        assert_eq!(said_in(&view, &hand_id(0)), ["\u{203a} CARBINE · IN HAND"]);
        assert_eq!(said_in(&view, &hand_id(1)), ["PISTOL · HOLSTERED"]);
        assert_eq!(view.keys, None, "the footer's line says the keys itself");

        // Picked, the bought one is what Fit asks for, and the table shows
        // it before anything is spent.
        let feed = day.bought(FEED);
        let state = FittingState {
            pick: Pick::Module(feed.clone()),
            last: Some(pick_id("1")),
            ..on("feed")
        };
        let picked = day.view(&state, true);
        assert!(pick(&picked, "1").on && !pick(&picked, PICK_STANDARD).on);
        assert_eq!(
            weapon(&picked).fit,
            Some(FitRequest::Fit {
                hand: 0,
                mount: "feed".to_owned(),
                module: feed.clone(),
            })
        );
        assert!(!shown(&picked, FIT).1);
        assert_eq!(shown(&picked, FIT).0, "strata.fitting.fit");
        assert_eq!(shown(&picked, WHY_ID).0, "Nothing fitted here");
        assert_eq!(
            after(&picked, Property::Capacity),
            ("29 rounds".to_owned(), "+5".to_owned(), Some(true))
        );
        assert_eq!(
            shown(&picked, &cell_id(Property::ReloadTime, "change")).0,
            "+0.14 s worse"
        );
        assert_eq!(after(&picked, Property::RepeatInterval).0, "");
        assert_eq!(picked.focus.as_deref(), Some("fitting.pick:1"));
        // The column is there now, with its heading.
        assert_eq!(shown(&picked, AFTER_HEAD_ID).0, "AFTER");
        assert!(open(&picked, &column_id("after", "capacity")));
        assert!(open(&picked, &column_id("change", "head")));
        assert_eq!(role_of(&picked, WHY_ID), WHY_HINT_ROLE);

        // Fitted: the mount names the module, the diagram is told its
        // part, the table's NOW is the fitted row beside the bare one, and
        // the standard part is what a removal puts back.
        day.hands[0]
            .as_mut()
            .expect("the carbine")
            .fitted
            .insert("feed".to_owned(), feed);
        let fitted = day.view(&on("feed"), true);
        let row = mount(&fitted, "feed");
        assert_eq!(
            row.name, "Extended magazine",
            "without the word the weapon's name has already said"
        );
        assert_eq!(row.sub, "Store grade");
        assert_eq!(row.look, MountLook::Fitted);
        assert_eq!(row.part.as_deref(), Some("patrol_extended_feed"));
        assert_eq!(
            shown(&fitted, &cell_id(Property::Capacity, "now")).0,
            "29 rounds"
        );
        assert_eq!(
            shown(&fitted, &cell_id(Property::Capacity, "bare")).0,
            "24 rounds"
        );
        assert_eq!(
            shown(&fitted, &cell_id(Property::ReloadTime, "now")).0,
            "1.49 s"
        );
        let ids: Vec<&str> = weapon(&fitted)
            .picks
            .iter()
            .map(|row| row.id.as_str())
            .collect();
        assert_eq!(
            ids,
            [PICK_STANDARD, PICK_FITTED, "0"],
            "more of the fitted batch are counted on its own row"
        );
        assert!(pick(&fitted, PICK_FITTED).on);
        assert_eq!(
            pick(&fitted, PICK_FITTED).sub,
            "Store grade · Capacity 500 · 4 in store · Fitted now"
        );
        let standard = pick(&fitted, PICK_STANDARD);
        assert_eq!(standard.sub, "Remove puts it back");
        assert_eq!(
            standard.preview.after[0],
            (
                "24 rounds".to_owned(),
                "\u{2212}5 worse".to_owned(),
                Some(false)
            )
        );
        assert_eq!(
            standard.request,
            Some(FitRequest::Remove {
                hand: 0,
                mount: "feed".to_owned()
            })
        );
        assert_eq!(weapon(&fitted).remove, standard.request);
        assert!(!shown(&fitted, REMOVE).1);
        assert_eq!(
            shown(&fitted, WHY_ID).0,
            "Pick a part to fit",
            "Fit is grey and Remove is not"
        );
        // With the standard part picked, Fit is the removal by its name.
        let state = FittingState {
            pick: Pick::Standard,
            ..on("feed")
        };
        let back = day.view(&state, true);
        assert_eq!(shown(&back, FIT).0, "strata.fitting.fit.standard");
        assert_eq!(weapon(&back).fit, standard.request);
        assert_eq!(after(&back, Property::Capacity).0, "24 rounds");
    }

    /// The sim's refusal is worded at the button that asked and in the
    /// footer, and every refusal has words.
    #[test]
    fn a_refusal_is_said_at_the_button() {
        let day = day();
        let state = FittingState {
            notice: Some((refusal_words(FitRefusal::Busy), true)),
            ..on("feed")
        };
        let view = day.view(&state, true);
        assert_eq!(
            view.status,
            ("Finish what you are doing first".to_owned(), true)
        );
        assert_eq!(shown(&view, WHY_ID).0, "Finish what you are doing first");
        assert_eq!(role_of(&view, WHY_ID), WHY_ROLE);
        assert_eq!(shown(&view, STATUS_ID).0, "Finish what you are doing first");
        // The sim's word has the footer's line, so the keys are said
        // under it and never leave the screen.
        assert!(
            shown(&view, KEYS_ID).0.starts_with("At the workbench"),
            "{:?}",
            view.keys
        );
        for (refusal, said) in [
            (FitRefusal::NoWorkbench, "Fitting is done at a workbench"),
            (FitRefusal::NoRoom, "No room"),
            (FitRefusal::NotInStock, "That part is no longer in stock"),
            (FitRefusal::DoesNotFit, "It does not fit this weapon"),
            (FitRefusal::WrongMount, "It does not fit this weapon"),
            (FitRefusal::AlreadyFitted, "Already fitted"),
            (FitRefusal::NothingFitted, "Nothing fitted here"),
            (
                FitRefusal::Shut { opens_at: 800 },
                "Opens at moddability 800",
            ),
        ] {
            assert_eq!(refusal_words(refusal), said);
        }
        assert!(!refusal_words(FitRefusal::NoPlayer).starts_with("strata."));
        assert!(!refusal_words(FitRefusal::NoWeapon).starts_with("strata."));
    }

    /// A module on a weapon's own mount drops the leading words the
    /// weapon's name has already said, and nothing else.
    #[test]
    fn a_fitted_modules_name_is_short_on_its_mount() {
        for (weapon, module, short) in [
            (
                "Vigil patrol carbine",
                "Patrol extended magazine",
                "Extended magazine",
            ),
            ("Vigil patrol carbine", "Vigil reflex sight", "Reflex sight"),
            (
                "Kestrel service pistol",
                "Pistol extended cell",
                "Extended cell",
            ),
            (
                "Kestrel service pistol",
                "Vigil reflex sight",
                "Vigil reflex sight",
            ),
            ("Vigil patrol carbine", "Vigil", "Vigil"),
            ("Vigil patrol carbine", "", ""),
        ] {
            assert_eq!(short_name(weapon, module), short, "{module} on {weapon}");
        }
    }

    /// The toast of a fit is what the fitted part does, in the words its
    /// row offered it by: the magazine for a feed, and not the magazine
    /// for a grip, which does not touch it.
    #[test]
    fn a_fit_is_toasted_with_what_the_part_does() {
        let day = day();
        let items = &day.registries.items;
        let said = |hand: usize, mount: &str, module: &str| {
            let mut records = day.hands.clone();
            let weapon = records[hand].as_mut().expect("a weapon");
            *weapon = fitting::with(items, weapon, mount, Some(&day.bought(module)))
                .expect("the part fits");
            let hands = Hands::from_saved(&records, items).expect("the hands stand");
            fitted_words(items, &hands, None, hand, mount)
        };
        assert_eq!(
            said(0, "feed", FEED),
            Some((
                "patrol extended magazine".to_owned(),
                "Magazine 29 rounds".to_owned()
            ))
        );
        let (_, grip) = said(0, "grip", GRIP).expect("the grip is fitted");
        assert_eq!(grip, "Draw 0.43 s");
        let (_, sight) = said(0, "sight", REFLEX).expect("the sight is fitted");
        assert_eq!(sight, "Aimed spread 1.40°");
        let (_, cell) = said(1, "feed", CELL).expect("the cell is fitted");
        assert_eq!(cell, "Magazine 13 rounds");
        for line in [&grip, &sight] {
            assert!(!line.contains("Magazine"), "{line}");
            let toast = fill(words("strata.fitting.toast.fitted"), "line", line);
            assert_eq!(toast, format!("Fitted. {line}."));
        }
        // Nothing on the mount, nothing to say.
        let hands = Hands::from_saved(&day.hands, items).expect("the hands stand");
        assert_eq!(fitted_words(items, &hands, None, 0, "feed"), None);
    }

    #[test]
    fn every_role_is_in_the_theme() {
        let mut day = day();
        let mut views = vec![
            day.view(&on("feed"), true),
            day.view(&on("muzzle"), false),
            day.view(
                &FittingState {
                    hand: 1,
                    ..FittingState::default()
                },
                true,
            ),
        ];
        let feed = day.bought(FEED);
        views.push(day.view(
            &FittingState {
                pick: Pick::Module(feed.clone()),
                notice: Some((refusal_words(FitRefusal::Busy), true)),
                ..on("feed")
            },
            true,
        ));
        day.hands[0]
            .as_mut()
            .expect("the carbine")
            .fitted
            .insert("feed".to_owned(), feed);
        views.push(day.view(&on("feed"), true));
        day.hands = [None, None];
        views.push(day.view(&FittingState::default(), true));
        assert!(views.last().is_some_and(|view| view.weapon.is_none()));

        let mut roles: BTreeSet<String> = BTreeSet::new();
        let mut keys = Vec::new();
        let states = ["", ".hover", ".focus", ".pressed"];
        for view in &views {
            screen_def(view).root.walk(&mut |node| match node {
                UiNodeDef::Panel { role, .. } => {
                    roles.insert(role.as_str().to_owned());
                }
                UiNodeDef::Text { key, opts, .. } => {
                    keys.push(key.0.clone());
                    if let Some(role) = &opts.role {
                        roles.insert(role.as_str().to_owned());
                    }
                }
                UiNodeDef::Button { opts, .. } => {
                    keys.extend(opts.label.iter().map(|label| label.0.clone()));
                    // A button with a family of its own paints from it in
                    // every state, the disabled one too.
                    if let Some(role) = &opts.role {
                        for state in states.iter().chain(&[".disabled"]) {
                            roles.insert(format!("{}{state}", role.as_str()));
                        }
                    }
                }
                UiNodeDef::Card { role, .. } => {
                    for state in states {
                        roles.insert(format!("{}{state}", role.as_str()));
                    }
                }
                _ => {}
            });
        }
        for role in [MOUNT_ON_ROLE, MOUNT_SHUT_ROLE, PICK_ON_ROLE, TAB_ON_ROLE] {
            for state in states {
                roles.insert(format!("{role}{state}"));
            }
        }
        roles.extend(ROLES.iter().map(|role| (*role).to_owned()));
        roles.extend([REFUSED_ROLE, CELL_AFTER_ROLE].map(str::to_owned));
        assert!(roles.contains(&format!("{ACT_ROLE}.disabled")));
        let path = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../assets/themes/glass.theme.ron"
        );
        let theme =
            slotted::theme::Theme::from_ron(&std::fs::read_to_string(path).expect("the theme"))
                .expect("the theme parses");
        let ours: Vec<&String> = roles.iter().filter(|r| r.starts_with("strata.")).collect();
        assert!(ours.len() > 40, "{ours:?}");
        for role in &roles {
            assert!(
                theme.roles.contains_key(&Role::new(role.clone())),
                "{role} is not in the theme"
            );
        }
        // Every key a definition names has English, and no sentence is
        // left as a key.
        for key in keys.iter().filter(|key| key.starts_with("strata.")) {
            assert!(english(key).is_some(), "{key} has no English");
        }
        // No em-dash and no factor is anywhere in what a player reads.
        for key in &keys {
            let said = english(key).unwrap_or(key);
            assert!(!said.contains('\u{2014}'), "{said}");
            assert!(!said.contains("1.2") && !said.contains("1.1"), "{said}");
        }
    }

    /// A mount blank of `temper`, as the store would hold it.
    fn blank(day: &Day, temper: u16) -> Record {
        Record {
            item: day.registries.rework.as_ref().expect("rules").blank.clone(),
            count: 1,
            batch: 0,
            by: None,
            lines: [("temper".to_owned(), temper)].into(),
            condition: None,
            fitted: std::collections::BTreeMap::new(),
            origin: None,
        }
    }

    /// The rework view of the first day's carbine with `blank` in the
    /// store, a smith's sheet or an empty one, and the bench or a reading.
    fn reworked_view(day: &mut Day, temper: Option<u16>, smith: bool, bench: bool) -> FittingView {
        let blank = temper.map(|temper| blank(day, temper));
        let items = &day.registries.items;
        let starting = items.starting().expect("the first day's stock");
        let records: Vec<_> = starting
            .store
            .iter()
            .cloned()
            .chain(blank)
            .map(Some)
            .collect();
        day.store = HouseholdStore::from_saved(&records, items).expect("a store");
        let sheet = if smith {
            CharacterSheet {
                learned: ["weaponsmith.frames.2".to_owned()].into(),
                ..CharacterSheet::default()
            }
        } else {
            CharacterSheet::default()
        };
        let reach = Reach {
            workbench: bench,
            ..Reach::default()
        };
        let holder = Combatant::default();
        FittingView::of(
            &Bench {
                items,
                hands: &day.hands,
                bare: [Weapon::Carbine, Weapon::Pistol].map(|weapon| holder.bare(weapon)),
                holder: &holder,
                drawn: 0,
                pack: &day.pack,
                store: &day.store,
                at_workbench: bench,
                rework: Some(ReworkBench {
                    registries: &day.registries,
                    access: WorkbenchAccess::default(),
                    reach: &reach,
                    sheet: &sheet,
                    purse: Purse(1000),
                }),
                pictures: None,
                symbols: None,
                models: None,
            },
            &FittingState::default(),
        )
    }

    fn rework_of(view: &FittingView) -> &ReworkView {
        weapon(view).rework.as_ref().expect("a rework line")
    }

    /// The rework line in each state it has: live with the outlook of a
    /// blank of temper 700 on a bought carbine, and grey with the sim's
    /// own reason when the box, the blank or the gain is missing.
    #[test]
    fn the_rework_line_says_what_it_would_do_or_why_not() {
        let mut day = day();
        let view = reworked_view(&mut day, Some(700), true, true);
        let live = rework_of(&view);
        assert_eq!(
            live.line,
            "Mount blank, temper 700. Most often to 650, between 550 and 700."
        );
        assert_eq!(live.request, Some(ReworkRequest::Rework { hand: 0 }));
        assert!(!shown(&view, REWORK).1);
        assert_eq!(live.buy_label, "Buy blank \u{b7} 150 credits");
        assert!(live.buy.is_some());
        let view = reworked_view(&mut day, Some(700), false, true);
        assert_eq!(rework_of(&view).line, "Needs Weaponsmith: Frames 2.");
        assert!(rework_of(&view).request.is_none() && shown(&view, REWORK).1);
        let view = reworked_view(&mut day, None, true, true);
        assert_eq!(rework_of(&view).line, "No mount blank to hand.");
        let view = reworked_view(&mut day, Some(500), true, true);
        assert_eq!(
            rework_of(&view).line,
            "This blank is no harder than the mounts: temper 500, moddability 500."
        );
        assert_eq!(shown(&view, REWORK_WHY_ID).0, rework_of(&view).line);
        assert!(rework_of(&view).buy.is_some());
    }

    /// A reading, opened away from a bench, shows neither button.
    #[test]
    fn a_reading_shows_no_rework() {
        let mut day = day();
        let view = reworked_view(&mut day, Some(700), true, false);
        assert!(weapon(&view).rework.is_none());
        let mut def = screen_def(&view);
        assert!(def.root.find_mut(REWORK).is_none());
        assert!(def.root.find_mut(BUY_BLANK).is_none());
    }
}
