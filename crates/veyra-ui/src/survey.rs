//! The survey tool's screen and its HUD (ticket 0009, step 3): G opens the
//! tool, a ping reads the ground round the player and sampling takes a lot
//! of what a ping found.
//!
//! The screen is a mirror and decides no rule. A class chip picks what to
//! ping; Ping puts a [`SurveyRequest::Ping`] in the sim's inbox and is grey,
//! with the reason beside it, exactly where `veyra_sim::deposits::can_ping`
//! refuses; Sample here does the same for `can_sample`. The last ping's
//! grid is the sim's [`LastPing`], kept so the screen opened after the
//! answer still draws it: a cell is a point of the lattice with its
//! concentration as a number, tinted by it in the class's colour (never
//! the colour alone), heading up (the ground ahead of the camera on top,
//! turned to the nearest quarter), the player's own point marked "you". The
//! channel is a bar the sim's own countdown fills in place, so a rebuild a
//! frame would not flicker it.
//!
//! Outside the screen the HUD says what is going on: a bar by the prompt
//! chip while pinging or sampling, a toast when a channel ends without
//! being finished (the sim's [`SurveyNotice`]) and a refusal's reason when
//! a request was refused. The lead marker is the tracker's.

use std::time::Duration;

use bevy::color::Color;
use bevy::ecs::name::Name;
use bevy::ecs::prelude::*;
use bevy::ecs::system::SystemParam;
use bevy::input::ButtonInput;
use bevy::input::keyboard::KeyCode;
use bevy::math::Vec2;
use bevy::transform::components::Transform;
use bevy::ui::widget::Text;
use bevy::ui::{BackgroundColor, Node, Val};
use strata_registry::{GameRegistries, Namespaced, Reading};
use veyra_sim::combat::Combatant;
use veyra_sim::deposits::{
    self, Compass, Deposit, HandState, LastPing, Met, NearDeposits, PingAnswer, PingResult,
    Pinging, SampleAnswer, SampleDone, SampleEnd, Sampling, Survey, SurveyInbox, SurveyNotice,
    SurveyRefusal, SurveyRequest, Surveyed,
};
use veyra_sim::harvest::{self as harvest, HarvestInbox, HarvestRequest, Harvesters};
use veyra_sim::items::Pack;
use veyra_sim::progression::CharacterSheet;
use veyra_sim::ride::{Calling, Riding};
use veyra_sim::{LocalPlayer, Registries, WorldPosition, WorldSourceRes};

use slotted::menu::{MenuChoice, ToastLevel, ToastSpec, toast};
use slotted::theme::Role;
use slotted::ui::def::{
    Align, ButtonOpts, ButtonVariant, Justify, Layout, LayoutDirection, Length, LocKey, Padding,
    Presentation, PresentationMode, ScreenDef, ScreenKind, Tags, TextAlign, TextOpts, TextRole,
    Transition, UiNodeDef, WidgetKind,
};
use slotted::ui::{
    HudAnchor, HudLayerDef, HudLayerId, HudLayers, NineAnchor, ScreenStack, TestId, pop_screen,
};
use veyra_sim::view::CombatCamera;

use super::bench::{compass_words, where_words};
use super::strings::english;
use super::weapon_diagram::FOCUS_MARK;
use strata_ui::sim_screen::{self, ChangeSource, ScreenStep, Seen, SimScreen, SimScreenApp, moved};

/// The screen kind.
pub const KIND: &str = "strata:survey";

/// The key that opens and closes the tool.
pub const KEY: KeyCode = KeyCode::KeyG;

/// The `test_id` of the panel the whole screen is.
pub const ROOT_ID: &str = "survey.root";
/// The `menu` tag and `test_id` prefix of a class chip: `survey.class:<id>`.
pub const CLASS_TAG: &str = "survey.class:";
/// The `menu` tag and `test_id` of Ping.
pub const PING: &str = "survey.ping";
/// The `menu` tag and `test_id` of Sample here.
pub const SAMPLE: &str = "survey.sample";
/// The `test_id` of the Plant harvester here button.
pub const PLANT: &str = "survey.plant";
/// The `test_id` of the line under Plant harvester here.
pub const PLANT_WHY_ID: &str = "survey.plant.why";
/// The `test_id` of the line why Ping is grey.
pub const PING_WHY_ID: &str = "survey.ping.why";
/// The `test_id` of the line why Sample here is grey, or what it would take.
pub const SAMPLE_WHY_ID: &str = "survey.sample.why";
/// The line under it that says whose claim the ground is.
pub const CLAIM_ID: &str = "survey.sample.claim";
/// The `test_id` of the channel bar's fill.
pub const FILL_ID: &str = "survey.fill";
/// The `test_id` of the channel's word.
pub const STATUS_ID: &str = "survey.status";
/// The `test_id` of the grid's frame.
pub const GRID_ID: &str = "survey.grid";
/// The `test_id` prefix of a cell: `survey.cell:<row>:<column>`, row 0 the
/// northernmost.
pub const CELL_TAG: &str = "survey.cell:";
/// The `test_id` of the line that stands in for the grid or the list when
/// the ping met nothing or there has been none.
pub const NOTE_ID: &str = "survey.note";
/// The `test_id` prefix of a deposit the ping met: `survey.met:<x>:<z>`.
pub const MET_TAG: &str = "survey.met:";
/// The `test_id` prefix of a met deposit's lines as read: `survey.lines:<x>:<z>`.
pub const LINES_TAG: &str = "survey.lines:";
/// The `test_id` prefix of the line that tells where a met deposit's richest
/// ground lies, there only with the Peak box: `survey.richest:<x>:<z>`.
pub const RICHEST_TAG: &str = "survey.richest:";
/// The `test_id` of the line that says the skill in force and whose it is.
pub const SKILL_ID: &str = "survey.skill";
/// The `test_id` of the Close button.
pub const CLOSE_ID: &str = "survey.close";

/// The HUD bar's layer id.
pub const BAR_LAYER: &str = "strata:survey_bar";
/// The `test_id` of the HUD bar's words.
pub const BAR_TEXT_ID: &str = "hud.survey.text";
/// The `test_id` of the HUD bar's fill.
pub const BAR_FILL_ID: &str = "hud.survey.fill";

const WIDTH: f32 = 920.;
const CELL_W: f32 = 76.;
const CELL_H: f32 = 54.;
const EDGE_W: f32 = 56.;
const BAR_W: f32 = 200.;
const LIST_W: f32 = 360.;

const HEAD_ROLE: &str = "strata.stores.section";
const CHIP_ROLE: &str = "strata.stores.chip";
const CHIP_ON_ROLE: &str = "strata.stores.chip.on";
const CHIP_TEXT_ROLE: &str = "strata.stores.chip.text";
const CHIP_TEXT_ON_ROLE: &str = "strata.stores.chip.text.on";
const NOTE_ROLE: &str = "strata.survey.note";
/// Why a button is grey, in the refusal's colour, as on the fitting screen.
const WHY_ROLE: &str = "strata.fitting.why";
/// A button while it cannot act: the fitting screen's family, whose disabled
/// look is fainter than the theme's own.
const ACT_ROLE: &str = "strata.fitting.act";
const STATUS_ROLE: &str = "strata.stores.status";

/// The screen kind.
#[must_use]
pub fn kind() -> ScreenKind {
    ScreenKind::new(KIND)
}

/// The `test_id` and `menu` tag of the chip for `class`.
#[must_use]
pub fn class_id(class: &Namespaced) -> String {
    format!("{CLASS_TAG}{class}")
}

/// The `test_id` of the cell in `row` (0 the northernmost) and `column`.
#[must_use]
pub fn cell_id(row: u32, column: u32) -> String {
    format!("{CELL_TAG}{row}:{column}")
}

fn words(key: &'static str) -> &'static str {
    english(key).unwrap_or(key)
}

fn fill(words: &str, name: &str, with: &str) -> String {
    words.replace(&format!("{{{name}}}"), with)
}

/// A refusal as a player reads it beside a button.
#[must_use]
pub fn refusal_words(refusal: SurveyRefusal) -> &'static str {
    words(match refusal {
        SurveyRefusal::NoPlayer => "strata.survey.why.no_player",
        SurveyRefusal::Mounted => "strata.survey.why.mounted",
        SurveyRefusal::NoTool => "strata.survey.why.no_tool",
        SurveyRefusal::InWater => "strata.survey.why.in_water",
        SurveyRefusal::Busy => "strata.survey.why.busy",
        SurveyRefusal::UnknownClass => "strata.survey.why.unknown_class",
        SurveyRefusal::Nothing => "strata.survey.why.nothing",
        SurveyRefusal::NoRoom => "strata.survey.why.no_room",
    })
}

/// The words of a channel that ended before it was finished.
#[must_use]
pub fn notice_words(notice: SurveyNotice) -> &'static str {
    words(match notice {
        SurveyNotice::PingMoved => "strata.survey.notice.ping_moved",
        SurveyNotice::PingStopped => "strata.survey.notice.ping_stopped",
        SurveyNotice::PingNoTool => "strata.survey.notice.ping_no_tool",
        SurveyNotice::SampleEnded(SampleEnd::Stopped) => "strata.survey.notice.sample.stopped",
        SurveyNotice::SampleEnded(SampleEnd::Moved) => "strata.survey.notice.sample.moved",
        SurveyNotice::SampleEnded(SampleEnd::Hit) => "strata.survey.notice.sample.hit",
        SurveyNotice::SampleEnded(SampleEnd::Swimming) => "strata.survey.notice.sample.swimming",
        SurveyNotice::SampleEnded(SampleEnd::NoRoom) => "strata.survey.notice.sample.no_room",
        SurveyNotice::SampleEnded(SampleEnd::NoTool) => "strata.survey.notice.sample.no_tool",
    })
}

/// What the player is doing with the tool.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Channel {
    /// A ping is being channelled.
    Pinging,
    /// Sampling is under way.
    Sampling,
}

/// Which way the grid is turned: the quarter turn nearest the camera's
/// heading. The variants are named by the axis ahead (`East` is `+x`), which
/// the ground's compass calls west: see [`Facing::compass`]. The right-hand
/// side of each facing is worked out here once and tested for all four.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Facing {
    /// Ahead is `+z`.
    #[default]
    North,
    /// Ahead is `+x`.
    East,
    /// Ahead is `-z`.
    South,
    /// Ahead is `-x`.
    West,
}

impl Facing {
    /// The facing nearest `forward`, a heading on the ground plane as
    /// `(x, z)`. A heading of no length (looking straight down) is north.
    #[must_use]
    pub fn of(forward: [f64; 2]) -> Self {
        let [x, z] = forward;
        if x.abs() > z.abs() {
            if x > 0. { Self::East } else { Self::West }
        } else if z < 0. {
            Self::South
        } else {
            Self::North
        }
    }

    /// The step ahead on the lattice, `(x, z)`.
    const fn ahead(self) -> [i32; 2] {
        match self {
            Self::North => [0, 1],
            Self::East => [1, 0],
            Self::South => [0, -1],
            Self::West => [-1, 0],
        }
    }

    /// The step to the player's right on the lattice, `(x, z)`: the
    /// camera's own right, which in this world's axes is ahead turned
    /// to `(-z, x)`.
    #[must_use]
    pub const fn right(self) -> [i32; 2] {
        let [x, z] = self.ahead();
        [-z, x]
    }

    /// The bearing ahead. A facing is named by its axis (`+x` is `East`
    /// here), the compass by the ground, where `+x` lies to the west.
    #[must_use]
    pub const fn compass(self) -> Compass {
        match self {
            Self::North => Compass::North,
            Self::East => Compass::West,
            Self::South => Compass::South,
            Self::West => Compass::East,
        }
    }

    /// The lattice point `(row from the south, column from the west)` that
    /// the screen's cell `(row from the top, column from the left)` shows.
    #[must_use]
    pub fn lattice(self, side: u32, row: u32, column: u32) -> (u32, u32) {
        let middle = i64::from(side / 2);
        let ahead = middle - i64::from(row);
        let right = i64::from(column) - middle;
        let [ax, az] = self.ahead().map(i64::from);
        let [rx, rz] = self.right().map(i64::from);
        let x = middle + ahead * ax + right * rx;
        let z = middle + ahead * az + right * rz;
        (u32::try_from(z).unwrap_or(0), u32::try_from(x).unwrap_or(0))
    }
}

/// One class chip.
#[derive(Debug, Clone, PartialEq)]
pub struct ClassChipView {
    /// The class.
    pub id: Namespaced,
    /// Its name, in capitals.
    pub name: String,
    /// Its colour, linear-free sRGB, for the swatch beside the name.
    pub colour: [f32; 4],
    /// Whether it is the one to ping.
    pub on: bool,
}

/// One point of the last ping's grid.
#[derive(Debug, Clone, PartialEq)]
pub struct CellView {
    /// The row on the screen, 0 the ground ahead.
    pub row: u32,
    /// The column on the screen, 0 the player's left.
    pub column: u32,
    /// The concentration as a percentage, whole.
    pub pct: u32,
    /// The words: `57%`.
    pub text: String,
    /// Whether it is the player's own point.
    pub you: bool,
    /// The cell's tint, the class's colour with a strength that follows the
    /// concentration.
    pub tint: [f32; 4],
}

/// The last ping's grid.
#[derive(Debug, Clone, PartialEq)]
pub struct GridView {
    /// Points a side.
    pub side: u32,
    /// The cells, the row ahead first and from the player's left in each.
    pub cells: Vec<CellView>,
    /// The compass word of the ahead side, in capitals: `NORTH`.
    pub ahead: String,
    /// `64 m between readings`.
    pub span: String,
}

/// A deposit the last ping met.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MetView {
    /// The deposit.
    pub id: deposits::DepositId,
    /// Its place: `Calder flats`.
    pub place: String,
    /// Its type, then the strongest reading and how far and which way it
    /// lies: `Evaporite salt · strongest 57% · 80 m north-east`.
    pub detail: String,
    /// What the household knows of it.
    pub known: String,
    /// Each line of the deposit as the household reads it: bands at
    /// Bands, figures at Exact, nothing at Type.
    pub lines: String,
    /// How far and which way its richest ground lies, only when the ping
    /// had the Peak box: `Richest ground 71% · 120 m north-east`.
    pub richest: Option<String>,
}

/// The state of the Plant harvester here button.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlantButton {
    /// No harvester is carried, so there is no button.
    Hidden,
    /// The sim would plant one here.
    Live,
    /// Drawn, with the reason beside it.
    Grey,
}

/// Everything the screen shows, in display words. Made from the sim's own
/// answers by [`SurveyView::of`], so what the screen says can be checked
/// without one.
#[derive(Debug, Clone, PartialEq)]
pub struct SurveyView {
    /// A chip for each class of the resource set.
    pub classes: Vec<ClassChipView>,
    /// The class to ping.
    pub class: Namespaced,
    /// Whether Ping can act.
    pub can_ping: bool,
    /// Why not; empty when it can.
    pub ping_why: String,
    /// What the player is doing with the tool.
    pub channel: Option<Channel>,
    /// The last ping's grid, if there has been a ping.
    pub grid: Option<GridView>,
    /// The deposits the last ping met, strongest first.
    pub met: Vec<MetView>,
    /// What stands in the list's place when there is none: no ping yet, or
    /// a ping that met nothing.
    pub note: String,
    /// Whether Sample here can act.
    pub can_sample: bool,
    /// What the ground here is, or why Sample here is grey.
    pub sample_line: String,
    /// Whether Plant harvester here is drawn, and whether it can act.
    pub plant: PlantButton,
    /// What the planting would stand on, or why the button is grey.
    pub plant_line: String,
    /// Whose claim the ground is and what working it means, under the Sample
    /// line; empty on open ground.
    pub claim_line: String,
    /// Whether the claim line is a warning, drawn warm and not as a note.
    pub claim_warns: bool,
    /// The control the keyboard's focus starts on.
    pub focus: Option<String>,
    /// The skill in force and whose it is, in one line; none without a
    /// tool.
    pub skill: Option<String>,
    /// Units a sampling has taken so far.
    pub taken: u32,
}

/// What the view is made from.
pub struct Inputs<'a> {
    /// The data sets.
    pub registries: &'a GameRegistries,
    /// What the rules need to know of the player.
    pub hand: HandState,
    /// The pack.
    pub pack: &'a Pack,
    /// The sheet.
    pub sheet: &'a CharacterSheet,
    /// What the household knows.
    pub surveyed: &'a Surveyed,
    /// Every deposit holding the player's spot, with its concentration
    /// there: `concentration_here`.
    pub covering: &'a [(Deposit, u16)],
    /// The last ping.
    pub last: Option<&'a PingResult>,
    /// The class the player picked, if they did.
    pub class: Option<&'a Namespaced>,
    /// Where the player stands, `(x, z)`.
    pub at: [f64; 2],
    /// What the player is doing with the tool.
    pub channel: Option<Channel>,
    /// Units the sampling under way has taken.
    pub taken: u32,
    /// Which way the camera looks.
    pub facing: Facing,
    /// The name of the member who is out.
    pub name: &'a str,
    /// The household's planted harvesters.
    pub harvesters: &'a Harvesters,
    /// Whether the household's survey record is kept in this world.
    pub kept: bool,
    /// What a claim's words are read from; none in a world without factions.
    pub books: Option<super::claims::Books<'a>>,
}

/// The tint of a cell: the class's colour, stronger the more concentrated.
/// A ceiling keeps the figure on it readable.
fn tint(colour: [f32; 4], pm: u16) -> [f32; 4] {
    let share = f32::from(pm.min(1000)) / 1000.;
    [colour[0], colour[1], colour[2], 0.06 + 0.46 * share]
}

/// What the household knows of a deposit, in words.
pub(crate) fn known_words(surveyed: &Surveyed, id: deposits::DepositId) -> String {
    let Some(found) = surveyed.found.get(&id) else {
        return words("strata.survey.known.type").to_owned();
    };
    words(if found.sampled {
        "strata.survey.known.sampled"
    } else {
        match found.reading {
            Reading::Type => "strata.survey.known.type",
            Reading::Bands => "strata.survey.known.bands",
            Reading::Exact => "strata.survey.known.exact",
        }
    })
    .to_owned()
}

/// A met deposit's lines at the best this household has read it: a sampled
/// deposit is read exactly whatever the ping told.
fn line_words(surveyed: &Surveyed, set: &strata_registry::ResourceSet, met: &Met) -> String {
    let known = surveyed.found.get(&met.deposit.id);
    let reading = if known.is_some_and(|found| found.sampled) {
        Reading::Exact
    } else {
        met.reading
            .max(known.map_or(Reading::Type, |found| found.reading))
    };
    met.deposit
        .figures
        .iter()
        .filter_map(|&(attribute, value)| match reading {
            Reading::Type => None,
            Reading::Bands => deposits::line_band(set, &met.deposit, attribute).map(|band| {
                fill(
                    &fill(words("strata.survey.line.band"), "code", attribute.code()),
                    "band",
                    super::card::band_word(band),
                )
            }),
            Reading::Exact => Some(fill(
                &fill(words("strata.survey.line.exact"), "code", attribute.code()),
                "value",
                &value.to_string(),
            )),
        })
        .collect::<Vec<_>>()
        .join(" · ")
}

fn percent(pm: u16) -> u32 {
    (u32::from(pm) + 5) / 10
}

impl SurveyView {
    /// The screen for what the sim says. `None` for a data set with no
    /// class at all.
    #[must_use]
    #[allow(
        clippy::too_many_lines,
        reason = "one view built top to bottom from the sim's answers"
    )]
    pub fn of(inputs: &Inputs<'_>) -> Option<Self> {
        let set = &inputs.registries.resources;
        let classes = set.classes();
        let class = inputs
            .class
            .filter(|class| classes.iter().any(|each| each.id == **class))
            .or_else(|| inputs.last.map(|last| &last.class))
            .or_else(|| classes.first().map(|first| &first.id))?
            .clone();
        let ping = deposits::can_ping(
            inputs.registries,
            inputs.hand,
            inputs.pack,
            inputs.sheet,
            &class,
        );
        let sample = deposits::can_sample(
            inputs.registries,
            inputs.hand,
            inputs.pack,
            inputs.sheet,
            inputs.covering,
            inputs.surveyed,
        );
        let plant = harvest::can_plant(
            inputs.registries,
            inputs.hand,
            inputs.pack,
            inputs.sheet,
            inputs.harvesters,
            inputs.covering,
            inputs.surveyed,
            inputs.kept,
            inputs.at,
        );
        let colour_of = |id: &Namespaced| {
            classes
                .iter()
                .find(|each| each.id == *id)
                .map_or([1.; 4], |each| each.color.0)
        };
        let grid = inputs.last.map(|last| {
            let colour = colour_of(&last.class);
            let side = last.grid;
            let here = last.cell_of(inputs.at);
            let cells = (0..side)
                .flat_map(|row| (0..side).map(move |column| (row, column)))
                .map(|(row, column)| {
                    // The lattice runs from the south and the west; the
                    // screen has the ground ahead of the camera on top.
                    let (lattice_row, lattice_column) = inputs.facing.lattice(side, row, column);
                    let index = (lattice_row * side + lattice_column) as usize;
                    let pm = last.points.get(index).map_or(0, |point| point.pm);
                    CellView {
                        row,
                        column,
                        pct: percent(pm),
                        text: fill(words("strata.survey.cell"), "pct", &percent(pm).to_string()),
                        you: here == Some((lattice_row, lattice_column)),
                        tint: tint(colour, pm),
                    }
                })
                .collect();
            let span = fill(
                words("strata.survey.span"),
                "metres",
                &format!("{:.0}", last.spacing_m),
            );
            // The grid is where the ping was made; say so once the player has
            // left that spot, so the YOU mark and the figures never lie.
            let back = last.moved_m(inputs.at);
            GridView {
                side,
                cells,
                ahead: compass_words(inputs.facing.compass()).to_uppercase(),
                span: if back >= last.spacing_m / 2.0 {
                    format!(
                        "{span} · {}",
                        fill(words("strata.survey.back"), "metres", &format!("{back:.0}"))
                    )
                } else {
                    span
                },
            }
        });
        let met: Vec<MetView> = inputs
            .last
            .map(|last| {
                last.found
                    .iter()
                    .map(|met| met_view(inputs, set, met))
                    .collect()
            })
            .unwrap_or_default();
        let note = match inputs.last {
            None => words("strata.survey.never").to_owned(),
            Some(last) if last.found.is_empty() => fill(
                words("strata.survey.nothing"),
                "metres",
                &format!("{:.0}", last.reach_m()),
            ),
            Some(_) => String::new(),
        };
        let ping_stops = inputs.channel == Some(Channel::Pinging);
        let sample_stops = inputs.channel == Some(Channel::Sampling);
        let sample_line = match &sample {
            Ok((deposit, pm)) => fill(
                &fill(
                    words("strata.survey.here"),
                    "pct",
                    &percent(*pm).to_string(),
                ),
                "place",
                &deposit.place,
            ),
            Err(SurveyRefusal::Nothing) => words("strata.survey.here.none").to_owned(),
            Err(_) if sample_stops => String::new(),
            Err(refusal) => refusal_words(*refusal).to_owned(),
        };
        // The deposit sampling takes, or the strongest here while one is
        // being sampled, so the line stays while the work counts.
        let claimed = match &sample {
            Ok((deposit, _)) => Some(deposit),
            // Only a found deposit: a claim is not a hint of what lies here.
            Err(_) => inputs
                .covering
                .iter()
                .filter(|(deposit, _)| inputs.surveyed.found.contains_key(&deposit.id))
                .max_by_key(|(_, pm)| *pm)
                .map(|(deposit, _)| deposit),
        };
        let warning = inputs
            .books
            .zip(claimed)
            .and_then(|(books, deposit)| books.warning(deposit));
        let claim_warns = warning.is_some();
        let claim_line = warning
            .or_else(|| {
                inputs
                    .books
                    .zip(claimed)
                    .and_then(|(books, deposit)| books.claim_words(deposit))
            })
            .unwrap_or_default();
        let plant_line = match &plant {
            Ok(plan) => fill(
                &fill(
                    words("strata.harvester.plant.ok"),
                    "pct",
                    &percent(plan.pm).to_string(),
                ),
                "place",
                &plan.deposit.place,
            ),
            Err(refusal) => super::harvester::refusal_words(*refusal),
        };
        let skill = deposits::skill_of(inputs.registries, inputs.pack, inputs.sheet)
            .map(|skill| skill_words(&skill, inputs.name));
        Some(Self {
            skill,
            taken: inputs.taken,
            classes: classes
                .iter()
                .map(|each| ClassChipView {
                    id: each.id.clone(),
                    name: each.name.to_uppercase(),
                    colour: each.color.0,
                    on: each.id == class,
                })
                .collect(),
            ping_why: ping.as_ref().err().map_or_else(String::new, |refusal| {
                if ping_stops {
                    String::new()
                } else {
                    refusal_words(*refusal).to_owned()
                }
            }),
            can_ping: ping.is_ok(),
            channel: inputs.channel,
            grid,
            met,
            note,
            can_sample: sample.is_ok(),
            sample_line,
            // Drawn by what is carried, not by the first refusal: the saddle
            // and the water come before it in the rule's order.
            plant: match plant {
                _ if harvest::best_machine(&inputs.registries.items, inputs.pack).is_none() => {
                    PlantButton::Hidden
                }
                Ok(_) => PlantButton::Live,
                Err(_) => PlantButton::Grey,
            },
            plant_line,
            claim_line,
            claim_warns,
            focus: Some(PING.to_owned()),
            class,
        })
    }
}

/// The skill in force as one line.
fn skill_words(skill: &deposits::SurveySkill, who: &str) -> String {
    let reads = words(match skill.reading {
        Reading::Type => "strata.survey.reads.type",
        Reading::Bands => "strata.survey.reads.bands",
        Reading::Exact => "strata.survey.reads.exact",
    });
    let line = fill(
        &fill(
            &fill(
                &fill(words("strata.survey.skill"), "who", who),
                "metres",
                &format!("{:.0}", skill.range_m),
            ),
            "grid",
            &skill.grid.to_string(),
        ),
        "units",
        &skill.units.to_string(),
    );
    let line = fill(&line, "reads", reads);
    if skill.peak {
        format!("{line}, {}", words("strata.survey.skill.peak"))
    } else {
        line
    }
}

fn met_view(inputs: &Inputs<'_>, set: &strata_registry::ResourceSet, met: &Met) -> MetView {
    let deposit = &met.deposit;
    let metres = (met.at[0] - inputs.at[0]).hypot(met.at[1] - inputs.at[1]);
    let lies = where_words(metres, Compass::of(inputs.at, met.at));
    MetView {
        id: deposit.id,
        place: deposit.place.clone(),
        detail: fill(
            &fill(
                &fill(
                    words("strata.survey.met.row"),
                    "type",
                    set.get(deposit.ty).map_or("", |def| def.name.as_str()),
                ),
                "pct",
                &percent(met.pm).to_string(),
            ),
            "where",
            &lies,
        ),
        known: known_words(inputs.surveyed, deposit.id),
        lines: line_words(inputs.surveyed, set, met),
        richest: met.richest.map(|richest| {
            let metres = (richest.at[0] - inputs.at[0]).hypot(richest.at[1] - inputs.at[1]);
            fill(
                &fill(
                    words("strata.survey.met.richest"),
                    "pct",
                    &percent(richest.pm).to_string(),
                ),
                "where",
                &where_words(metres, Compass::of(inputs.at, richest.at)),
            )
        }),
    }
}

fn key(s: impl Into<String>) -> LocKey {
    LocKey(s.into())
}

/// A line that stays on one line, in `role`.
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

/// A line that wraps, in `role`.
fn note_in(s: impl Into<String>, role: &str) -> UiNodeDef {
    UiNodeDef::Text {
        key: key(s),
        style: TextRole::Caption,
        opts: TextOpts {
            role: Some(Role::new(role)),
            ..TextOpts::default()
        },
        tags: Tags::new(),
    }
}

fn centred(s: impl Into<String>, role: &str) -> UiNodeDef {
    UiNodeDef::Text {
        key: key(s),
        style: TextRole::Caption,
        opts: TextOpts {
            wrap: false,
            align: TextAlign::Center,
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

fn panel(layout: Layout, children: Vec<UiNodeDef>) -> UiNodeDef {
    UiNodeDef::Panel {
        role: Role::new("invisible"),
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

/// A class's chip: a swatch in its colour (painted by [`paint`]) and its
/// name, lit and marked when it is the one to ping.
fn chip(view: &ClassChipView) -> UiNodeDef {
    let id = class_id(&view.id);
    let label = if view.on {
        format!("{FOCUS_MARK}{}", view.name)
    } else {
        view.name.clone()
    };
    UiNodeDef::Card {
        role: Role::new(if view.on { CHIP_ON_ROLE } else { CHIP_ROLE }),
        layout: Layout {
            padding: Padding {
                top: 0.6,
                right: 2.2,
                bottom: 0.6,
                left: 1.6,
            },
            ..row(1.)
        },
        children: vec![
            UiNodeDef::Panel {
                role: Role::new("strata.survey.swatch"),
                layout: Layout {
                    width: Some(Length::Px(10.)),
                    height: Some(Length::Px(10.)),
                    ..column(0.)
                },
                children: Vec::new(),
                tags: Tags::new().with(Tags::TEST_ID, &format!("survey.swatch:{}", view.id)),
            },
            line_in(
                label,
                if view.on {
                    CHIP_TEXT_ON_ROLE
                } else {
                    CHIP_TEXT_ROLE
                },
            ),
        ],
        disabled: false,
        tags: Tags::new().with("menu", &id).with(Tags::TEST_ID, &id),
    }
}

/// One cell: its percentage over, on the player's own, the word "you".
fn cell(view: &CellView) -> UiNodeDef {
    let mut children = Vec::new();
    if view.you {
        children.push(centred("strata.survey.you", "strata.survey.you"));
    }
    children.push(centred(view.text.clone(), "strata.survey.pct"));
    UiNodeDef::Panel {
        role: Role::new(if view.you {
            "strata.survey.cell.you"
        } else {
            "strata.survey.cell"
        }),
        layout: Layout {
            width: Some(Length::Px(CELL_W)),
            height: Some(Length::Px(CELL_H)),
            align: Some(Align::Center),
            justify: Justify::Center,
            ..column(0.1)
        },
        children,
        tags: Tags::new().with(Tags::TEST_ID, &cell_id(view.row, view.column)),
    }
}

/// The grid with its frame, the ground ahead on top. The compass word of
/// the ahead side stands small beside AHEAD, so the grid still says which
/// way on the map it looks.
fn grid(view: &GridView) -> UiNodeDef {
    let rows = (0..view.side)
        .map(|r| {
            panel(
                row(0.4),
                view.cells
                    .iter()
                    .filter(|cell_view| cell_view.row == r)
                    .map(cell)
                    .collect(),
            )
        })
        .collect();
    let edge = |key: &'static str| centred(key, "strata.survey.edge");
    let side = |key: &'static str| {
        panel(
            Layout {
                width: Some(Length::Px(EDGE_W)),
                align: Some(Align::Center),
                justify: Justify::Center,
                ..column(0.)
            },
            vec![edge(key)],
        )
    };
    panel(
        Layout {
            align: Some(Align::Center),
            ..column(0.6)
        },
        vec![
            panel(
                Layout {
                    align: Some(Align::End),
                    ..row(0.8)
                },
                vec![
                    edge("strata.survey.edge.ahead"),
                    line_in(view.ahead.clone(), "strata.survey.edge"),
                ],
            ),
            panel(
                row(0.8),
                vec![
                    side("strata.survey.edge.left"),
                    with_id(panel(column(0.4), rows), GRID_ID),
                    side("strata.survey.edge.right"),
                ],
            ),
            edge("strata.survey.edge.behind"),
            line_in(view.span.clone(), NOTE_ROLE),
        ],
    )
}

/// A button that is the accent only when it can act: the theme has no grey
/// for a primary button, which would stand lit while inert.
fn button(id: &str, label: &str, can: bool) -> UiNodeDef {
    UiNodeDef::Button {
        widget: None,
        opts: ButtonOpts {
            label: Some(key(label)),
            variant: if can {
                ButtonVariant::Primary
            } else {
                ButtonVariant::Secondary
            },
            disabled: !can,
            role: (!can).then(|| Role::new(ACT_ROLE)),
            ..ButtonOpts::default()
        },
        tags: Tags::new().with("menu", id).with(Tags::TEST_ID, id),
    }
}

/// The channel's track with its fill, which [`fill_channel`] sets in place.
fn bar(id: &str, width: f32) -> UiNodeDef {
    UiNodeDef::Panel {
        role: Role::new("strata.survey.track"),
        layout: Layout {
            width: Some(Length::Px(width)),
            height: Some(Length::Px(8.)),
            ..column(0.)
        },
        children: vec![UiNodeDef::Panel {
            role: Role::new("strata.survey.fill"),
            layout: Layout {
                width: Some(Length::Percent(0.)),
                height: Some(Length::Percent(100.)),
                ..column(0.)
            },
            children: Vec::new(),
            tags: Tags::new().with(Tags::TEST_ID, id),
        }],
        tags: Tags::new(),
    }
}

/// A deposit the ping met.
fn met_row(view: &MetView) -> UiNodeDef {
    let id = format!("{MET_TAG}{}:{}", view.id.0[0], view.id.0[1]);
    let mut rows = vec![
        with_id(
            panel(
                row(1.),
                vec![
                    line_in(view.place.clone(), "strata.survey.met"),
                    spacer(),
                    line_in(view.known.clone(), HEAD_ROLE),
                ],
            ),
            &id,
        ),
        line_in(view.detail.clone(), "strata.survey.met.sub"),
    ];
    if !view.lines.is_empty() {
        rows.push(with_id(
            line_in(view.lines.clone(), "strata.survey.met.sub"),
            &format!("{LINES_TAG}{}:{}", view.id.0[0], view.id.0[1]),
        ));
    }
    if let Some(richest) = &view.richest {
        rows.push(with_id(
            line_in(richest.clone(), "strata.survey.met.sub"),
            &format!("{RICHEST_TAG}{}:{}", view.id.0[0], view.id.0[1]),
        ));
    }
    panel(column(0.1), rows)
}

/// The whole screen for `view`.
#[must_use]
#[allow(
    clippy::too_many_lines,
    reason = "one definition, read top to bottom as the screen is"
)]
pub fn screen_def(view: &SurveyView) -> ScreenDef {
    let head = panel(
        Layout {
            align: Some(Align::End),
            ..row(2.)
        },
        vec![
            UiNodeDef::Text {
                key: key("strata.survey.title"),
                style: TextRole::Title,
                opts: TextOpts {
                    wrap: false,
                    role: Some(Role::new("panel.title")),
                    ..TextOpts::default()
                },
                tags: Tags::new(),
            },
            line_in("strata.survey.sub", HEAD_ROLE),
        ],
    );
    let status = match view.channel {
        Some(Channel::Pinging) => words("strata.survey.hud.pinging").to_owned(),
        Some(Channel::Sampling) => fill(
            words("strata.survey.status.sampling"),
            "units",
            &view.taken.to_string(),
        ),
        None => String::new(),
    };
    let left = panel(
        Layout {
            align: Some(Align::Start),
            ..column(1.4)
        },
        vec![
            panel(row(1.), view.classes.iter().map(chip).collect()),
            with_id(
                note_in(view.skill.clone().unwrap_or_default(), NOTE_ROLE),
                SKILL_ID,
            ),
            if let Some(grid_view) = &view.grid {
                grid(grid_view)
            } else {
                with_id(
                    panel(
                        Layout {
                            width: Some(Length::Px(CELL_W * 5. + EDGE_W * 2.)),
                            ..column(0.)
                        },
                        vec![note_in(view.note.clone(), NOTE_ROLE)],
                    ),
                    NOTE_ID,
                )
            },
            panel(
                row(1.6),
                vec![
                    button(
                        PING,
                        if view.channel == Some(Channel::Pinging) {
                            "strata.survey.stop"
                        } else {
                            "strata.survey.ping"
                        },
                        view.can_ping || view.channel == Some(Channel::Pinging),
                    ),
                    bar(FILL_ID, BAR_W),
                    with_id(line_in(status, HEAD_ROLE), STATUS_ID),
                ],
            ),
            with_id(line_in(view.ping_why.clone(), WHY_ROLE), PING_WHY_ID),
        ],
    );
    let met: Vec<UiNodeDef> = if view.met.is_empty() {
        Vec::new()
    } else {
        view.met.iter().map(met_row).collect()
    };
    let mut right = vec![line_in("strata.survey.met.head", HEAD_ROLE)];
    if met.is_empty() {
        right.push(note_in(view.note.clone(), NOTE_ROLE));
    }
    right.extend(met);
    right.push(button(
        SAMPLE,
        if view.channel == Some(Channel::Sampling) {
            "strata.survey.stop_sampling"
        } else {
            "strata.survey.sample"
        },
        view.can_sample || view.channel == Some(Channel::Sampling),
    ));
    right.push(with_id(
        note_in(
            view.sample_line.clone(),
            if view.can_sample { NOTE_ROLE } else { WHY_ROLE },
        ),
        SAMPLE_WHY_ID,
    ));
    if !view.claim_line.is_empty() {
        right.push(with_id(
            note_in(
                view.claim_line.clone(),
                if view.claim_warns {
                    WHY_ROLE
                } else {
                    NOTE_ROLE
                },
            ),
            CLAIM_ID,
        ));
    }
    if view.plant != PlantButton::Hidden {
        right.push(button(
            PLANT,
            "strata.harvester.plant",
            view.plant == PlantButton::Live,
        ));
        right.push(with_id(
            note_in(
                view.plant_line.clone(),
                if view.plant == PlantButton::Live {
                    NOTE_ROLE
                } else {
                    WHY_ROLE
                },
            ),
            PLANT_WHY_ID,
        ));
    }
    // A scroll panel, not a clipped one: the wheel and the bar move it.
    let body = UiNodeDef::Scroll {
        layout: Layout {
            align: Some(Align::Start),
            grow: 1.,
            min_height: Some(Length::Px(0.)),
            ..row(3.)
        },
        scrollbar: true,
        children: vec![
            left,
            panel(
                Layout {
                    width: Some(Length::Px(LIST_W)),
                    align: Some(Align::Stretch),
                    ..column(1.2)
                },
                right,
            ),
        ],
        tags: Tags::new(),
    };
    let footer = panel(
        row(2.),
        vec![
            line_in("strata.survey.keys", STATUS_ROLE),
            spacer(),
            UiNodeDef::Custom {
                kind: WidgetKind::new("slotted:hint_bar"),
                params: slotted::registry::Value::Unit,
                children: Vec::new(),
                tags: Tags::new().with(Tags::TEST_ID, "survey.hints"),
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
    );
    ScreenDef {
        kind: kind(),
        inherits: None,
        root: UiNodeDef::Panel {
            role: Role::new("panel"),
            layout: Layout {
                width: Some(Length::Px(WIDTH)),
                max_width: Some(Length::Percent(96.)),
                min_height: Some(Length::Percent(94.)),
                max_height: Some(Length::Percent(94.)),
                padding: Padding::all(3.),
                align: Some(Align::Stretch),
                justify: Justify::Start,
                ..column(2.)
            },
            children: vec![head, body, footer],
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

/// The open screen: the class picked and what was last shown.
#[derive(Resource, Debug, Default)]
pub struct SurveyState {
    /// The class picked; none until a chip is, then the last ping's.
    class: Option<Namespaced>,
    /// The view the open screen was built from. A press clears it, which is
    /// how the next comparison knows to rebuild.
    view: Option<SurveyView>,
    /// The control last used, which keeps the keyboard across the rebuild
    /// it caused.
    last: Option<String>,
    /// How many views have been made, which a test holds still over idle
    /// frames.
    views: std::sync::atomic::AtomicU32,
}

impl SurveyState {
    /// How many views have been built since the game began.
    #[must_use]
    pub fn views(&self) -> u32 {
        self.views.load(std::sync::atomic::Ordering::Relaxed)
    }

    /// The view the open screen was built from.
    #[must_use]
    pub const fn view(&self) -> Option<&SurveyView> {
        self.view.as_ref()
    }
}

fn on_top(stack: &ScreenStack) -> bool {
    stack.top().is_some_and(|top| top.kind == kind())
}

/// What a machine on the list is compared by: its place. A machine's
/// `carry` moves every tick it works and only the planting rule (how many,
/// and where) is drawn, so the work between is not a rebuild.
fn planted(list: &Harvesters) -> Vec<(u32, [u64; 2])> {
    list.list
        .iter()
        .map(|each| (each.id, each.at.map(f64::to_bits)))
        .collect()
}

/// What the player's spot says to the view, worked out of the world: where
/// they stand in whole metres (the view shows distances no finer), the
/// rules' hand, what the tool is doing, which way the camera looks and the
/// deposits holding the spot.
#[derive(Debug, Clone, PartialEq)]
struct Here {
    at: [f64; 2],
    hand: HandState,
    channel: Option<Channel>,
    taken: u32,
    facing: Facing,
    covering: Vec<(Deposit, u16)>,
}

/// What `Here` was last worked out for, so a frame in which none of it
/// moved does not sample the water and the deposit field again.
#[derive(Debug, Default)]
pub struct HereCache {
    key: Option<([u64; 2], [bool; 4], bool, u32)>,
}

/// What the survey reads: the data, the household's findings and the last
/// ping, the deposit field, the harvesters (by where they stand), the
/// leases and the books the claims are worded from, the local player's
/// pack, sheet, name, spot and what the tool is doing, and the camera's
/// heading. The spot, the camera and the harvesters count by what the view
/// shows of them.
#[derive(SystemParam)]
pub struct SurveyInputs<'w, 's> {
    registries: Option<Res<'w, Registries>>,
    surveyed: Option<Res<'w, Surveyed>>,
    last_ping: Option<Res<'w, LastPing>>,
    survey: Option<Res<'w, Survey>>,
    source: Option<Res<'w, WorldSourceRes>>,
    harvesters: Option<Res<'w, Harvesters>>,
    unkept: Option<Res<'w, veyra_sim::deposits::SurveyUnkept>>,
    books: super::claims::Standing<'w>,
    cameras: Query<'w, 's, &'static Transform, With<CombatCamera>>,
    players: Query<
        'w,
        's,
        (
            Ref<'static, Pack>,
            &'static WorldPosition,
            Option<&'static Combatant>,
            Option<Ref<'static, CharacterSheet>>,
            Option<Ref<'static, Name>>,
            Has<Riding>,
            Has<Calling>,
            Has<Pinging>,
            Option<&'static Sampling>,
        ),
        With<LocalPlayer>,
    >,
    clock: Local<'s, Seen<u64>>,
    standing: Local<'s, Seen<Vec<(u32, [u64; 2])>>>,
    kept: Local<'s, Seen<bool>>,
    facing: Local<'s, Seen<Facing>>,
    spot: Local<'s, HereCache>,
    place: Local<'s, Seen<Option<Here>>>,
}

impl SurveyInputs<'_, '_> {
    fn facing(&self) -> Facing {
        self.cameras
            .iter()
            .next()
            .map_or_else(Facing::default, |camera| {
                let ahead = camera.forward();
                Facing::of([f64::from(ahead.x), f64::from(ahead.z)])
            })
    }

    /// The player's spot, or none without a local player.
    fn here(&self, facing: Facing) -> Option<Here> {
        let registries = &self.registries.as_deref()?.0;
        let swim_m = registries.resources.survey().ping.swim_m;
        let (_, at, actor, _, _, riding, calling, pinging, sampling) =
            self.players.single().ok()?;
        let hand = deposits::hand_state(
            at,
            actor,
            [riding, calling, pinging, sampling.is_some()],
            self.source.as_deref(),
            swim_m,
        );
        let here = [at.0.x(), at.0.z()];
        let covering = self
            .survey
            .as_deref()
            .map(|survey| deposits::concentration_here(survey, here[0], here[1]))
            .unwrap_or_default();
        Some(Here {
            // Whole metres: a standing character's position is not bit-stable
            // and the view shows distances no finer.
            at: [here[0].round(), here[1].round()],
            hand,
            channel: if pinging {
                Some(Channel::Pinging)
            } else if sampling.is_some() {
                Some(Channel::Sampling)
            } else {
                None
            },
            taken: sampling.map_or(0, |sampling| sampling.got),
            facing,
            covering,
        })
    }
}

impl ChangeSource for SurveyInputs<'static, 'static> {
    fn changed(inputs: &mut SurveyInputs<'_, '_>) -> bool {
        let books = inputs.books.moved(&mut inputs.clock);
        let standing = inputs
            .harvesters
            .as_deref()
            .is_some_and(|list| inputs.standing.differs(planted(list)));
        let kept = inputs.kept.differs(inputs.unkept.is_none());
        let facing = inputs.facing();
        let turned = inputs.facing.differs(facing);
        // The spot is worked out afresh only when what it is made of moved:
        // the position, the tool's flags, the combat state the rules call
        // busy, the deposit field.
        let key = inputs.players.single().ok().map(
            |(_, at, actor, _, _, riding, calling, pinging, sampling)| {
                (
                    [at.0.x().to_bits(), at.0.z().to_bits()],
                    [riding, calling, pinging, sampling.is_some()],
                    actor.is_some_and(|actor| {
                        actor.action != veyra_sim::combat::Action::Ready || actor.health <= 0.0
                    }),
                    sampling.map_or(0, |sampling| sampling.got),
                )
            },
        );
        let spot = if inputs.spot.key == key && !moved(&inputs.survey) && !moved(&inputs.source) {
            false
        } else {
            let here = inputs.here(facing);
            inputs.spot.key = key;
            inputs.place.differs(here)
        };
        let player = inputs
            .players
            .single()
            .is_ok_and(|(pack, _, _, sheet, name, ..)| {
                pack.is_changed()
                    || sheet.is_some_and(|sheet| sheet.is_changed())
                    || name.is_some_and(|name| name.is_changed())
            });
        books
            || standing
            || kept
            || turned
            || spot
            || player
            || moved(&inputs.registries)
            || moved(&inputs.surveyed)
            || moved(&inputs.last_ping)
    }
}

impl SimScreen for SurveyState {
    type View = SurveyView;
    type Inputs = SurveyInputs<'static, 'static>;
    type Answering = (ResMut<'static, SurveyInbox>, ResMut<'static, HarvestInbox>);
    const KIND: &'static str = KIND;

    /// The view of what the world holds.
    fn view(inputs: &SurveyInputs<'_, '_>, state: &Self) -> Option<SurveyView> {
        let registries = &inputs.registries.as_deref()?.0;
        let here = inputs.here(inputs.facing())?;
        let (pack, _, _, sheet, name, ..) = inputs.players.single().ok()?;
        state
            .views
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let (surveyed, harvesters) = (Surveyed::default(), Harvesters::default());
        let sheet = sheet.as_deref().cloned().unwrap_or_default();
        let name = name.as_deref().map_or_else(
            || words("strata.survey.ranger").to_owned(),
            |name| name.as_str().to_owned(),
        );
        let last = inputs.last_ping.as_deref().and_then(|last| last.0.clone());
        let mut view = SurveyView::of(&Inputs {
            registries,
            hand: here.hand,
            pack: &pack,
            sheet: &sheet,
            surveyed: inputs.surveyed.as_deref().unwrap_or(&surveyed),
            covering: &here.covering,
            last: last.as_ref(),
            class: state.class.as_ref(),
            at: here.at,
            channel: here.channel,
            taken: here.taken,
            facing: here.facing,
            name: &name,
            harvesters: inputs.harvesters.as_deref().unwrap_or(&harvesters),
            kept: inputs.unkept.is_none(),
            books: inputs.books.books(),
        })?;
        if let Some(used) = state.last.clone().filter(|used| {
            used == PING
                || used == SAMPLE
                || used == PLANT
                || view.classes.iter().any(|chip| class_id(&chip.id) == *used)
        }) {
            view.focus = Some(used);
        }
        Some(view)
    }

    fn def(view: &SurveyView, _: &Self) -> ScreenDef {
        screen_def(view)
    }

    fn shown(state: &Self) -> Option<&SurveyView> {
        state.view.as_ref()
    }

    fn marked(_: &Self) -> bool {
        false
    }

    fn built(state: &mut Self, view: SurveyView) {
        state.view = Some(view);
    }

    /// A class chip picks the class; Ping and Sample here ask the sim.
    /// Nothing is decided here: the request is judged in the tick.
    fn answer(
        state: &mut Self,
        choice: &MenuChoice,
        (inbox, planting): &mut (ResMut<SurveyInbox>, ResMut<HarvestInbox>),
    ) {
        // The button a channel is under way on is Stop; the view says which.
        let stops = state.view.as_ref().and_then(|view| view.channel);
        let id = choice.id.as_str();
        if let Some(class) = id.strip_prefix(CLASS_TAG) {
            state.class = class.parse().ok();
        } else if (id == PING && stops == Some(Channel::Pinging))
            || (id == SAMPLE && stops == Some(Channel::Sampling))
        {
            inbox.push(SurveyRequest::Stop);
        } else if id == PING {
            let class = state
                .view
                .as_ref()
                .map(|view| view.class.clone())
                .or_else(|| state.class.clone());
            if let Some(class) = class {
                inbox.push(SurveyRequest::Ping { class });
            }
        } else if id == SAMPLE {
            inbox.push(SurveyRequest::Sample);
        } else if id == PLANT {
            planting.push(HarvestRequest::Plant);
        } else {
            return;
        }
        state.last = Some(id.to_owned());
        // The screen is rebuilt on the next frame's comparison.
        state.view = None;
    }
}

/// Pushes the survey screen, or rebuilds it in place when it is already on
/// top. G and the flag for a still both come through here. Does nothing,
/// and says so in the log, with nobody to survey for.
pub fn open(commands: &mut Commands) {
    commands.queue(|world: &mut World| {
        let Some(def) = sim_screen::build::<SurveyState>(world) else {
            tracing::warn!("no local player with a pack: nothing to survey for");
            return;
        };
        sim_screen::show::<SurveyState>(world, def);
    });
}

/// `Update`: G opens the tool over nothing, and closes it from the top.
pub fn toggle(keys: Res<ButtonInput<KeyCode>>, stack: Res<ScreenStack>, mut commands: Commands) {
    if !keys.just_pressed(KEY) {
        return;
    }
    if on_top(&stack) {
        pop_screen(&mut commands);
    } else if stack.top().is_none() {
        open(&mut commands);
    }
}

/// `Update`: the cells and swatches take their tints, which a theme role
/// cannot give because the colour is the class's. Written when it differs.
pub fn paint(
    stack: Res<ScreenStack>,
    state: Res<SurveyState>,
    mut commands: Commands,
    mut painted: Query<(Entity, &TestId, Option<&mut BackgroundColor>)>,
) {
    if !on_top(&stack) {
        return;
    }
    let Some(view) = state.view.as_ref() else {
        return;
    };
    for (entity, id, colour) in &mut painted {
        let wanted = if let Some(class) = id.0.strip_prefix("survey.swatch:") {
            view.classes
                .iter()
                .find(|chip| chip.id.to_string() == class)
                .map(|chip| chip.colour)
        } else if let Some(at) = id.0.strip_prefix(CELL_TAG) {
            view.grid.as_ref().and_then(|grid| {
                grid.cells
                    .iter()
                    .find(|cell| cell_id(cell.row, cell.column) == format!("{CELL_TAG}{at}"))
                    .map(|cell| cell.tint)
            })
        } else {
            None
        };
        let Some([r, g, b, a]) = wanted else {
            continue;
        };
        let wanted = Color::srgba(r, g, b, a);
        match colour {
            Some(mut colour) => {
                if colour.0 != wanted {
                    colour.0 = wanted;
                }
            }
            None => {
                commands.entity(entity).insert(BackgroundColor(wanted));
            }
        }
    }
}

/// How far through its seconds the player's channel is, 0 to 1; none when
/// nothing is being channelled. The seconds are the skill's, which a
/// trained hand shortens, and the dials' for a hand with no tool left.
fn through(
    registries: &GameRegistries,
    skill: Option<&deposits::SurveySkill>,
    pinging: Option<&Pinging>,
    sampling: Option<&Sampling>,
) -> Option<f32> {
    let rules = registries.resources.survey();
    #[allow(
        clippy::cast_possible_truncation,
        reason = "a dial's seconds as a clock's f32"
    )]
    let (left, total) = match (pinging, sampling) {
        (Some(pinging), _) => (
            pinging.left,
            skill.map_or(rules.ping.channel_s as f32, |skill| skill.channel_s),
        ),
        (None, Some(sampling)) => (
            sampling.left,
            skill.map_or(rules.sample.cycle_s as f32, |skill| skill.cycle_s),
        ),
        (None, None) => return None,
    };
    Some((1. - left / total.max(f32::EPSILON)).clamp(0., 1.))
}

/// `Update`: the channel bars, on the screen and on the HUD, fill from the
/// sim's own countdown. Written to the fill's node in place, as the
/// crafting roll's is: a rebuild a frame would be a flicker and no motion.
pub fn fill_channel(
    registries: Option<Res<Registries>>,
    players: Query<
        (
            Option<&Pinging>,
            Option<&Sampling>,
            Option<&Pack>,
            Option<&CharacterSheet>,
        ),
        With<LocalPlayer>,
    >,
    mut fills: Query<(&TestId, &mut Node)>,
) {
    let Some(registries) = registries else {
        return;
    };
    let Ok((pinging, sampling, pack, sheet)) = players.single() else {
        return;
    };
    // The pack scan and the graph folds are for a running channel only.
    let skill = pack
        .filter(|_| pinging.is_some() || sampling.is_some())
        .and_then(|pack| {
            deposits::skill_of(
                &registries.0,
                pack,
                sheet.unwrap_or(&CharacterSheet::default()),
            )
        });
    let share = through(&registries.0, skill.as_ref(), pinging, sampling).unwrap_or(0.);
    let width = Val::Percent(share * 100.);
    for (id, mut node) in &mut fills {
        if (id.0 == FILL_ID || id.0 == BAR_FILL_ID) && node.width != width {
            node.width = width;
        }
    }
}

/// The HUD bar's layer id.
#[must_use]
pub fn bar_layer() -> HudLayerId {
    HudLayerId::new_static(BAR_LAYER)
}

/// The bar by the prompt chip: what is being done over a track that fills.
#[must_use]
pub fn bar_layer_def() -> HudLayerDef {
    HudLayerDef {
        id: bar_layer(),
        anchor: HudAnchor {
            anchor: NineAnchor::Bottom,
            offset: Vec2::new(0.0, -210.0),
            scale: 1.0,
        },
        tree: UiNodeDef::Panel {
            role: Role::new("tooltip"),
            layout: Layout {
                padding: Padding {
                    top: 1.0,
                    right: 2.5,
                    bottom: 1.2,
                    left: 2.5,
                },
                align: Some(Align::Center),
                ..column(0.8)
            },
            children: vec![
                with_id(
                    UiNodeDef::Text {
                        key: key("strata.survey.hud.pinging"),
                        style: TextRole::Body,
                        opts: TextOpts {
                            role: Some(Role::new("toast.text")),
                            wrap: false,
                            ..TextOpts::default()
                        },
                        tags: Tags::new(),
                    },
                    BAR_TEXT_ID,
                ),
                bar(BAR_FILL_ID, 220.),
            ],
            tags: Tags::new().with(Tags::TEST_ID, "hud.survey"),
        },
        visible: false,
        hide_with_screen: true,
    }
}

/// The HUD bar's words for what the player is doing: "Surveying", or
/// "Sampling Scobie lode · 12 units" with the count of what has been got.
#[must_use]
pub fn bar_words(
    registries: &GameRegistries,
    pinging: Option<&Pinging>,
    sampling: Option<(&Sampling, Option<&Deposit>)>,
) -> Option<String> {
    if pinging.is_some() {
        return Some(words("strata.survey.hud.pinging").to_owned());
    }
    let (sampling, deposit) = sampling?;
    let lot = deposit.map_or_else(
        || words("strata.survey.hud.lot").to_owned(),
        |deposit| deposit.lot_name(&registries.resources),
    );
    Some(fill(
        &fill(words("strata.survey.hud.sampling"), "lot", &lot),
        "units",
        &sampling.got.to_string(),
    ))
}

/// `Startup`: the bar's layer, hidden.
pub fn register_layer(mut layers: ResMut<HudLayers>) {
    layers.register(bar_layer_def());
}

/// `Update`: the HUD bar follows the channel. The words are written
/// straight onto the text node, as the prompt's are, so a layer respawned
/// under a closing screen gets them too.
#[allow(
    clippy::too_many_arguments,
    reason = "the channel, the data, the deposits and the layer with its words"
)]
pub fn follow_channel(
    registries: Option<Res<Registries>>,
    survey: Option<Res<Survey>>,
    near: Option<Res<NearDeposits>>,
    players: Query<(Option<&Pinging>, Option<&Sampling>), With<LocalPlayer>>,
    mut layers: ResMut<HudLayers>,
    mut wanted: Local<Option<String>>,
    mut texts: Query<(&Tags, &mut Text)>,
) {
    let (pinging, sampling) = players.single().unwrap_or((None, None));
    let now = registries.as_ref().and_then(|registries| {
        let deposit = sampling.and_then(|sampling| {
            near.as_ref()
                .and_then(|near| near.get(sampling.deposit).cloned())
                .or_else(|| survey.as_ref().and_then(|s| s.0.get(sampling.deposit)))
        });
        bar_words(
            &registries.0,
            pinging,
            sampling.map(|sampling| (sampling, deposit.as_ref())),
        )
    });
    if now.is_some() != wanted.is_some() {
        layers.set_visible(&bar_layer(), now.is_some());
    }
    *wanted = now;
    let Some(text) = wanted.as_ref() else {
        return;
    };
    for (tags, mut node) in &mut texts {
        if tags.get(Tags::TEST_ID) == Some(BAR_TEXT_ID) && node.0 != *text {
            node.0.clone_from(text);
        }
    }
}

/// `Update`: a channel that ended unfinished and a refusal are toasts with
/// their reasons. A cycle's lot and a started channel are not: the bar and
/// the pack say them.
pub fn toasts(
    mut notices: MessageReader<SurveyNotice>,
    mut pings: MessageReader<PingAnswer>,
    mut samples: MessageReader<SampleAnswer>,
    mut commands: Commands,
) {
    let mut say = |said: &str| {
        tracing::info!("survey: {said}");
        toast(
            &mut commands,
            ToastSpec::new(said)
                .level(ToastLevel::Warning)
                .duration(Duration::from_millis(2400)),
        );
    };
    for notice in notices.read() {
        say(notice_words(*notice));
    }
    for answer in pings.read() {
        if let Err(refusal) = answer.outcome {
            say(refusal_words(refusal));
        }
    }
    for answer in samples.read() {
        match answer.outcome {
            Err(refusal) => say(refusal_words(refusal)),
            Ok(SampleDone::Started { .. } | SampleDone::Cycle { .. }) => {}
        }
    }
}

/// Registers the state, the key, the bar and the systems. Only called once
/// the UI is wired.
pub(crate) fn register(app: &mut bevy::app::App) {
    app.add_sim_screen::<SurveyState>()
        .init_resource::<LastPing>()
        .add_message::<SurveyNotice>()
        .add_message::<PingAnswer>()
        .add_message::<SampleAnswer>()
        .add_systems(bevy::app::Startup, register_layer)
        .add_systems(
            bevy::app::Update,
            toggle
                .before(ScreenStep::answer::<SurveyState>())
                .in_set(super::ScreenWork),
        )
        .add_systems(
            bevy::app::Update,
            (
                paint.run_if(super::screen_on_top(kind)),
                fill_channel,
                follow_channel,
                toasts,
            ),
        );
}

#[cfg(test)]
mod tests {
    use super::*;
    use strata_registry::item::SavedPack;
    use veyra_sim::deposits::{Found, PingPoint, ping_with};
    use veyra_sim::harvest::HarvestRefusal;

    use crate::testing::{deposit, shipped};

    fn pack(registries: &GameRegistries) -> Pack {
        let start = registries.items.starting().expect("the first day's stock");
        let mut pack = Pack::from_saved(&SavedPack::default(), &registries.items).expect("a pack");
        // The first member's pack is the one with the tool in it.
        if let Some(saved) = start.packs.values().next() {
            pack = Pack::from_saved(saved, &registries.items).expect("a pack");
        }
        pack
    }

    fn ready() -> HandState {
        HandState {
            present: true,
            ..HandState::default()
        }
    }

    fn view(
        registries: &GameRegistries,
        pack: &Pack,
        hand: HandState,
        surveyed: &Surveyed,
        covering: &[(Deposit, u16)],
        last: Option<&PingResult>,
    ) -> SurveyView {
        view_at(registries, pack, hand, surveyed, covering, last, [0., 0.])
    }

    fn view_at(
        registries: &GameRegistries,
        pack: &Pack,
        hand: HandState,
        surveyed: &Surveyed,
        covering: &[(Deposit, u16)],
        last: Option<&PingResult>,
        at: [f64; 2],
    ) -> SurveyView {
        view_of_sheet(
            registries,
            pack,
            &CharacterSheet::default(),
            "Frontier ranger",
            Facing::North,
            surveyed,
            last,
            at,
            hand,
            covering,
        )
    }

    #[allow(
        clippy::too_many_arguments,
        reason = "a test's view, every input named"
    )]
    fn view_of_sheet(
        registries: &GameRegistries,
        pack: &Pack,
        sheet: &CharacterSheet,
        name: &str,
        facing: Facing,
        surveyed: &Surveyed,
        last: Option<&PingResult>,
        at: [f64; 2],
        hand: HandState,
        covering: &[(Deposit, u16)],
    ) -> SurveyView {
        SurveyView::of(&Inputs {
            registries,
            hand,
            pack,
            sheet,
            surveyed,
            covering,
            last,
            class: None,
            at,
            channel: None,
            taken: 0,
            facing,
            name,
            harvesters: &Harvesters::default(),
            kept: true,
            books: None,
        })
        .expect("a view")
    }

    #[test]
    fn an_unfound_claim_is_not_given_away_by_the_sample_line() {
        let registries = crate::testing::shipped();
        let (map, lot) = crate::claims::tests::city();
        let (deeds, leases) = (
            veyra_sim::deeds::Deeds::default(),
            veyra_sim::deeds::Leases::default(),
        );
        let books = crate::claims::Books {
            registries: &registries,
            map: Some(map),
            deeds: &deeds,
            leases: &leases,
            now: 0.0,
        };
        assert!(books.warning(lot).is_some(), "the control is a claim");
        let covering = [(lot.clone(), 800_u16)];
        let view = SurveyView::of(&Inputs {
            registries: &registries,
            hand: HandState::default(),
            pack: &pack(&registries),
            sheet: &CharacterSheet::default(),
            surveyed: &Surveyed::default(),
            covering: &covering,
            last: None,
            class: None,
            at: [0., 0.],
            channel: None,
            taken: 0,
            facing: Facing::North,
            name: "Ilse Marrow",
            harvesters: &Harvesters::default(),
            kept: true,
            books: Some(books),
        })
        .expect("a view");
        assert_eq!(view.claim_line, "");
    }

    fn test_skill(peak: bool) -> deposits::SurveySkill {
        deposits::SurveySkill {
            channel_s: 2.5,
            cycle_s: 4.0,
            peak,
            range_m: 128.,
            grid: 5,
            units: 8,
            reading: Reading::Type,
        }
    }

    /// A ping on one deposit at `at`, from the origin.
    fn ping_on(registries: &GameRegistries, calder: &Deposit, peak: bool) -> PingResult {
        let set = &registries.resources;
        let class = set.class_of(calder.ty).id.clone();
        let covering = |x: f64, z: f64| {
            let pm = calder.concentration(x, z);
            if pm == 0 {
                Vec::new()
            } else {
                vec![(calder.clone(), pm)]
            }
        };
        ping_with(&covering, set, &test_skill(peak), [0., 0.], &class)
    }

    /// The grid is drawn heading up for every facing: a deposit ahead sits
    /// in the top row and one to the player's right in the right column,
    /// where right is the camera's own right in this world's axes (facing
    /// north, east is on the left), and the YOU mark stays in the middle.
    #[test]
    fn the_grid_is_drawn_heading_up_for_the_four_facings() {
        let registries = shipped();
        let pack = pack(&registries);
        for facing in [Facing::North, Facing::East, Facing::South, Facing::West] {
            let [ax, az] = facing.ahead().map(f64::from);
            // A literal table, so the test does not check `right` by itself:
            // north is +z and east +x, so facing north the right hand is -x.
            let [rx, rz] = match facing {
                Facing::North => [-1., 0.],
                Facing::East => [0., 1.],
                Facing::South => [1., 0.],
                Facing::West => [0., -1.],
            };
            for (label, vector, want) in [
                ("ahead", [ax, az], (0, 2)),
                ("right", [rx, rz], (2, 4)),
                ("left", [-rx, -rz], (2, 0)),
                ("behind", [-ax, -az], (4, 2)),
            ] {
                let calder = deposit(
                    &registries,
                    "strata:evaporite_salt",
                    "Calder",
                    [0, 0],
                    [vector[0] * 130., vector[1] * 130.],
                );
                let result = ping_on(&registries, &calder, false);
                let shown = view_of_sheet(
                    &registries,
                    &pack,
                    &CharacterSheet::default(),
                    "Ranger",
                    facing,
                    &Surveyed::default(),
                    Some(&result),
                    [0., 0.],
                    ready(),
                    &[],
                );
                let grid = shown.grid.expect("a grid");
                let strongest = grid
                    .cells
                    .iter()
                    .max_by_key(|cell| cell.pct)
                    .expect("a cell");
                assert_eq!(
                    (strongest.row, strongest.column),
                    want,
                    "{label} of {facing:?}"
                );
                let you: Vec<_> = grid.cells.iter().filter(|cell| cell.you).collect();
                assert_eq!(you.len(), 1);
                assert_eq!((you[0].row, you[0].column), (2, 2), "{facing:?}");
                assert_eq!(
                    grid.ahead,
                    compass_words(facing.compass()).to_uppercase(),
                    "the ahead side's compass word"
                );
            }
        }
        // The camera's own right, checked against the engine: a camera
        // looking along +z has its right on -x.
        let camera = Transform::from_translation(bevy::math::Vec3::ZERO)
            .looking_to(bevy::math::Vec3::Z, bevy::math::Vec3::Y);
        let (ahead, right) = (camera.forward(), camera.right());
        assert_eq!(
            Facing::of([f64::from(ahead.x), f64::from(ahead.z)]),
            Facing::North
        );
        assert!(
            (f32::from(i16::try_from(Facing::North.right()[0]).unwrap_or(0)) - right.x).abs()
                < 1e-3
                && (f32::from(i16::try_from(Facing::North.right()[1]).unwrap_or(0)) - right.z)
                    .abs()
                    < 1e-3,
            "{right:?}"
        );
    }

    #[test]
    fn the_skill_row_names_who_is_out_and_what_their_hand_does() {
        let registries = shipped();
        let pack = pack(&registries);
        let row = |sheet: &CharacterSheet, name: &str| {
            view_of_sheet(
                &registries,
                &pack,
                sheet,
                name,
                Facing::North,
                &Surveyed::default(),
                None,
                [0., 0.],
                ready(),
                &[],
            )
            .skill
            .expect("a tool, so a skill")
        };
        let arrival = row(&CharacterSheet::default(), "Frontier ranger");
        assert_eq!(
            arrival,
            "Frontier ranger: reach 128 m · 5 by 5 grid · 8 units a cycle · reads the type only"
        );
        let mut ilse = CharacterSheet::default();
        for id in [
            "surveyor.novice",
            "surveyor.deposits.1",
            "surveyor.reading.1",
        ] {
            ilse.learned.insert(id.to_owned());
        }
        assert_eq!(
            row(&ilse, "Ilse Marrow"),
            "Ilse Marrow: reach 192 m · 5 by 5 grid · 9 units a cycle · reads each line in bands"
        );
        let mut full = CharacterSheet::default();
        let tree = registries.progression.tree("surveyor").expect("ships");
        full.learned.extend(tree.boxes().map(|b| b.id.clone()));
        let line = row(&full, "Ilse Marrow");
        assert!(line.contains("7 by 7 grid") && line.contains("reads each line exactly"));
        assert!(
            line.ends_with("also tells where the richest ground lies"),
            "{line}"
        );
    }

    #[test]
    fn a_peak_ping_tells_how_far_and_which_way_the_richest_ground_is() {
        let registries = shipped();
        let pack = pack(&registries);
        let calder = deposit(
            &registries,
            "strata:evaporite_salt",
            "Calder",
            [0, 0],
            [0., 130.],
        );
        let mut surveyed = Surveyed::default();
        for (peak, shown) in [(false, false), (true, true)] {
            let result = ping_on(&registries, &calder, peak);
            deposits::learn(&mut surveyed, &result);
            let view = view_at(
                &registries,
                &pack,
                ready(),
                &surveyed,
                &[],
                Some(&result),
                [0., 0.],
            );
            assert_eq!(view.met.len(), 1);
            assert_eq!(view.met[0].richest.is_some(), shown);
            if let Some(richest) = &view.met[0].richest {
                assert!(
                    richest.starts_with("Richest ground ") && richest.ends_with("130 m north"),
                    "{richest}"
                );
            }
        }
    }

    #[test]
    fn a_met_deposit_shows_its_lines_as_the_ping_read_them() {
        let registries = shipped();
        let pack = pack(&registries);
        let calder = deposit(
            &registries,
            "strata:evaporite_salt",
            "Calder",
            [0, 0],
            [0., 130.],
        );
        let result = ping_on(&registries, &calder, false);
        for (reading, expect) in [
            (Reading::Type, false),
            (Reading::Bands, true),
            (Reading::Exact, true),
        ] {
            let mut result = result.clone();
            for met in &mut result.found {
                met.reading = reading;
            }
            let mut surveyed = Surveyed::default();
            deposits::learn(&mut surveyed, &result);
            let view = view_at(
                &registries,
                &pack,
                ready(),
                &surveyed,
                &[],
                Some(&result),
                [0., 0.],
            );
            let lines = &view.met[0].lines;
            assert_eq!(!lines.is_empty(), expect, "{reading:?}: {lines}");
            let figure = calder.figures[0].0.code();
            if reading == Reading::Exact {
                let value = calder.figures[0].1.to_string();
                assert!(lines.contains(&format!("{figure} {value}")), "{lines}");
            } else if reading == Reading::Bands {
                assert!(
                    lines.starts_with(figure) && !lines.chars().any(|c| c.is_ascii_digit()),
                    "{lines}"
                );
            }
        }
    }

    #[test]
    fn every_word_has_english() {
        for key in [
            "strata.survey.title",
            "strata.survey.cell",
            "strata.survey.hud.sampling",
            "strata.survey.hud.sampling_short",
            "strata.survey.hud.lot",
        ] {
            assert!(english(key).is_some(), "{key}");
        }
        let all = [
            SurveyRefusal::NoPlayer,
            SurveyRefusal::Mounted,
            SurveyRefusal::NoTool,
            SurveyRefusal::InWater,
            SurveyRefusal::Busy,
            SurveyRefusal::UnknownClass,
            SurveyRefusal::Nothing,
            SurveyRefusal::NoRoom,
        ];
        let said: std::collections::BTreeSet<_> = all.into_iter().map(refusal_words).collect();
        assert_eq!(said.len(), all.len(), "each refusal has words of its own");
        assert!(said.iter().all(|said| !said.starts_with("strata.")));
        for notice in [
            SurveyNotice::PingMoved,
            SurveyNotice::PingStopped,
            SurveyNotice::PingNoTool,
            SurveyNotice::SampleEnded(SampleEnd::Stopped),
            SurveyNotice::SampleEnded(SampleEnd::Moved),
            SurveyNotice::SampleEnded(SampleEnd::Hit),
            SurveyNotice::SampleEnded(SampleEnd::Swimming),
            SurveyNotice::SampleEnded(SampleEnd::NoRoom),
            SurveyNotice::SampleEnded(SampleEnd::NoTool),
        ] {
            assert!(!notice_words(notice).starts_with("strata."), "{notice:?}");
        }
    }

    #[test]
    fn ping_is_grey_with_the_sims_reason() {
        let registries = shipped();
        let pack = pack(&registries);
        let surveyed = Surveyed::default();
        let at = |hand| view(&registries, &pack, hand, &surveyed, &[], None);
        let free = at(ready());
        assert!(free.can_ping && free.ping_why.is_empty());
        for (hand, refusal) in [
            (HandState::default(), SurveyRefusal::NoPlayer),
            (
                HandState {
                    mounted: true,
                    ..ready()
                },
                SurveyRefusal::Mounted,
            ),
            (
                HandState {
                    swimming: true,
                    ..ready()
                },
                SurveyRefusal::InWater,
            ),
            (
                HandState {
                    busy: true,
                    ..ready()
                },
                SurveyRefusal::Busy,
            ),
        ] {
            let grey = at(hand);
            assert!(!grey.can_ping, "{refusal:?}");
            assert_eq!(grey.ping_why, refusal_words(refusal));
        }
        // No tool in the pack.
        let empty = Pack::from_saved(&SavedPack::default(), &registries.items).expect("a pack");
        let none = view(&registries, &empty, ready(), &surveyed, &[], None);
        assert!(!none.can_ping);
        assert_eq!(none.ping_why, refusal_words(SurveyRefusal::NoTool));
    }

    #[test]
    fn the_grid_has_the_ground_ahead_on_top_and_the_player_in_the_middle() {
        let registries = shipped();
        let set = &registries.resources;
        let pack = pack(&registries);
        let calder = deposit(
            &registries,
            "strata:evaporite_salt",
            "Calder",
            [0, 0],
            [0., 130.],
        );
        let class = set.class_of(calder.ty).id.clone();
        let skill = deposits::SurveySkill {
            channel_s: 2.5,
            cycle_s: 4.0,
            peak: false,
            range_m: 128.,
            grid: 5,
            units: 8,
            reading: Reading::Type,
        };
        let covering = |x: f64, z: f64| {
            let pm = calder.concentration(x, z);
            if pm == 0 {
                Vec::new()
            } else {
                vec![(calder.clone(), pm)]
            }
        };
        let result = ping_with(&covering, set, &skill, [0., 0.], &class);
        assert_eq!(result.grid, 5);
        let mut surveyed = Surveyed::default();
        deposits::learn(&mut surveyed, &result);
        let shown = view(&registries, &pack, ready(), &surveyed, &[], Some(&result));
        let grid = shown.grid.expect("a grid");
        assert_eq!(grid.cells.len(), 25);
        let you: Vec<_> = grid.cells.iter().filter(|cell| cell.you).collect();
        assert_eq!(you.len(), 1);
        assert_eq!((you[0].row, you[0].column), (2, 2));
        // The deposit lies ahead (the camera faces north): its strongest cells are in the top
        // rows, and every cell has a number.
        let strongest = grid
            .cells
            .iter()
            .max_by_key(|cell| cell.pct)
            .expect("a cell");
        assert!(strongest.pct > 0 && strongest.row <= 1, "{strongest:?}");
        assert!(grid.cells.iter().all(|cell| cell.text.ends_with('%')));
        let north = result
            .points
            .iter()
            .max_by_key(|point| point.pm)
            .expect("a point");
        assert!(north.at[1] > 0.0);
        assert_eq!(shown.met.len(), 1);
        assert_eq!(shown.met[0].place, calder.place);
        assert_eq!(shown.met[0].known, "Type only");
        assert!(
            shown.met[0].detail.contains("north"),
            "{}",
            shown.met[0].detail
        );
        assert!(shown.note.is_empty());
    }

    /// The YOU mark follows the player off the ping's middle and goes when
    /// they leave the grid, with the grid saying how far back it was made.
    #[test]
    fn the_you_mark_follows_the_player_after_a_walk() {
        let registries = shipped();
        let pack = pack(&registries);
        let set = &registries.resources;
        let calder = deposit(
            &registries,
            "strata:evaporite_salt",
            "Calder",
            [0, 0],
            [0., 130.],
        );
        let class = set.class_of(calder.ty).id.clone();
        let skill = deposits::SurveySkill {
            channel_s: 2.5,
            cycle_s: 4.0,
            peak: false,
            range_m: 128.,
            grid: 5,
            units: 8,
            reading: Reading::Type,
        };
        let covering = |x: f64, z: f64| {
            let pm = calder.concentration(x, z);
            if pm == 0 {
                Vec::new()
            } else {
                vec![(calder.clone(), pm)]
            }
        };
        let result = ping_with(&covering, set, &skill, [0., 0.], &class);
        let surveyed = Surveyed::default();
        let at = |where_: [f64; 2]| {
            view_at(
                &registries,
                &pack,
                ready(),
                &surveyed,
                &[],
                Some(&result),
                where_,
            )
        };
        let you = |shown: &SurveyView| {
            shown
                .grid
                .as_ref()
                .expect("a grid")
                .cells
                .iter()
                .filter(|cell| cell.you)
                .map(|cell| (cell.row, cell.column))
                .collect::<Vec<_>>()
        };
        let unmoved = at([0., 0.]);
        assert_eq!(you(&unmoved), [(2, 2)]);
        assert!(!unmoved.grid.as_ref().expect("a grid").span.contains("back"));
        // One spacing north (64 m) is the row above; one east, facing north,
        // is the column on the left, since east is the player's left here.
        let north = at([0., 64.]);
        assert_eq!(you(&north), [(1, 2)]);
        assert!(
            north
                .grid
                .as_ref()
                .expect("a grid")
                .span
                .contains("pinged 64 m back")
        );
        assert_eq!(you(&at([64., 0.])), [(2, 1)]);
        assert!(you(&at([0., 400.])).is_empty());
    }

    #[test]
    fn a_ping_that_met_nothing_says_so_and_so_does_no_ping_yet() {
        let registries = shipped();
        let pack = pack(&registries);
        let surveyed = Surveyed::default();
        let never = view(&registries, &pack, ready(), &surveyed, &[], None);
        assert!(never.grid.is_none() && never.met.is_empty());
        assert_eq!(never.note, "No ping yet. Pick a class and ping.");
        let class = registries.resources.classes()[0].id.clone();
        let result = PingResult {
            class,
            centre: [0., 0.],
            grid: 5,
            spacing_m: 64.,
            points: (0..25)
                .map(|n| PingPoint {
                    at: [f64::from(n), 0.],
                    deposit: None,
                    pm: 0,
                })
                .collect(),
            found: Vec::new(),
        };
        let nothing = view(&registries, &pack, ready(), &surveyed, &[], Some(&result));
        assert!(nothing.met.is_empty());
        assert!(
            nothing.note.starts_with("The ping met no deposit"),
            "{}",
            nothing.note
        );
        assert!(
            nothing
                .grid
                .is_some_and(|grid| grid.cells.iter().all(|cell| cell.pct == 0))
        );
    }

    #[test]
    fn sample_here_is_live_only_where_the_sim_says() {
        let registries = shipped();
        let pack = pack(&registries);
        let calder = deposit(
            &registries,
            "strata:evaporite_salt",
            "Calder",
            [0, 0],
            [0., 0.],
        );
        let covering = vec![(calder.clone(), 500_u16)];
        let mut surveyed = Surveyed::default();
        let unfound = view(&registries, &pack, ready(), &surveyed, &covering, None);
        assert!(!unfound.can_sample, "a deposit must be pinged first");
        assert_eq!(
            unfound.sample_line,
            "Nothing found reads here. Ping, then walk to a reading."
        );
        surveyed.found.insert(
            calder.id,
            Found {
                reading: Reading::Type,
                sampled: false,
                best_pm: 500,
                best_at: [0., 0.],
            },
        );
        let found = view(&registries, &pack, ready(), &surveyed, &covering, None);
        assert!(found.can_sample);
        assert_eq!(
            found.sample_line,
            "Here the ground reads 50% of Calder flats."
        );
        let busy = view(
            &registries,
            &pack,
            HandState {
                busy: true,
                ..ready()
            },
            &surveyed,
            &covering,
            None,
        );
        assert!(!busy.can_sample);
        assert_eq!(busy.sample_line, refusal_words(SurveyRefusal::Busy));
    }

    #[test]
    fn the_hud_words_name_the_lot_and_the_count() {
        let registries = shipped();
        let calder = deposit(
            &registries,
            "strata:evaporite_salt",
            "Calder",
            [0, 0],
            [0., 0.],
        );
        let pinging = Pinging {
            class: registries.resources.classes()[0].id.clone(),
            left: 1.,
        };
        assert_eq!(
            bar_words(&registries, Some(&pinging), None).as_deref(),
            Some("Surveying")
        );
        let sampling = Sampling {
            deposit: calder.id,
            from: [0., 0.],
            left: 1.,
            got: 12,
        };
        assert_eq!(
            bar_words(&registries, None, Some((&sampling, Some(&calder)))).as_deref(),
            Some("Sampling Calder salt · 12 units")
        );
        assert!(bar_words(&registries, None, None).is_none());
        assert!(bar_layer_def().hide_with_screen);
        assert!(!bar_layer_def().visible);
    }

    #[allow(
        clippy::too_many_lines,
        reason = "one fixture, then each refusal in order"
    )]
    #[test]
    fn plant_here_is_drawn_with_a_harvester_and_live_or_grey_by_the_sims_rule() {
        use crate::testing::{harvester_record, pack_with, planted, sheet_of};
        use veyra_sim::deposits::Found;

        let registries = shipped();
        let calder = deposit(
            &registries,
            "strata:evaporite_salt",
            "Calder",
            [0, 0],
            [0., 0.],
        );
        let covering = vec![(calder.clone(), 800)];
        let mut found = Surveyed::default();
        found.found.insert(
            calder.id,
            Found {
                reading: Reading::Exact,
                sampled: true,
                best_pm: 800,
                best_at: [0., 0.],
            },
        );
        let machine = harvester_record();
        let carried = pack_with(&registries, Some(&machine));
        let taught = sheet_of(&["surveyor.stations.1"]);
        let planted_here = Harvesters {
            list: vec![planted(&registries, &calder, [0., 0.], 800, 0)],
            next: 1,
        };
        let view = |pack: &Pack,
                    sheet: &CharacterSheet,
                    hand: HandState,
                    covering: &[(Deposit, u16)],
                    harvesters: &Harvesters| {
            SurveyView::of(&Inputs {
                registries: &registries,
                hand,
                pack,
                sheet,
                surveyed: &found,
                covering,
                last: None,
                class: None,
                at: [0., 0.],
                channel: None,
                taken: 0,
                facing: Facing::North,
                name: "Ilse Marrow",
                harvesters,
                kept: true,
                books: None,
            })
            .expect("a view")
        };
        let none = Harvesters::default();
        let grey = |view: &SurveyView, said: &str| {
            assert_eq!(view.plant, PlantButton::Grey, "{said}");
            assert_eq!(view.plant_line, said);
        };

        // None carried: the button is not drawn.
        let empty = pack_with(&registries, None);
        assert_eq!(
            view(&empty, &taught, ready(), &covering, &none).plant,
            PlantButton::Hidden
        );

        for hand in [
            HandState {
                mounted: true,
                ..ready()
            },
            HandState {
                swimming: true,
                ..ready()
            },
        ] {
            assert_eq!(
                view(&empty, &taught, hand, &covering, &none).plant,
                PlantButton::Hidden,
                "none carried is not drawn on the bike or in the water"
            );
        }

        let live = view(&carried, &taught, ready(), &covering, &none);
        assert_eq!(live.plant, PlantButton::Live);
        assert!(live.plant_line.contains("Calder"), "{}", live.plant_line);
        assert!(live.plant_line.contains("80%"), "{}", live.plant_line);

        let words = |refusal| crate::harvester::refusal_words(refusal);
        grey(
            &view(
                &carried,
                &taught,
                HandState {
                    mounted: true,
                    ..ready()
                },
                &covering,
                &none,
            ),
            &words(HarvestRefusal::Mounted),
        );
        grey(
            &view(
                &carried,
                &taught,
                HandState {
                    swimming: true,
                    ..ready()
                },
                &covering,
                &none,
            ),
            &words(HarvestRefusal::InWater),
        );
        grey(
            &view(&carried, &sheet_of(&[]), ready(), &covering, &none),
            &words(HarvestRefusal::NotTaught),
        );
        grey(
            &view(&carried, &taught, ready(), &covering, &planted_here),
            &words(HarvestRefusal::AtLimit { count: 1 }),
        );
        grey(
            &view(&carried, &taught, ready(), &[], &none),
            &words(HarvestRefusal::NothingHere),
        );
        let two = sheet_of(&["surveyor.stations.1", "surveyor.stations.3"]);
        grey(
            &view(&carried, &two, ready(), &covering, &planted_here),
            &words(HarvestRefusal::TooNear),
        );
    }
}
