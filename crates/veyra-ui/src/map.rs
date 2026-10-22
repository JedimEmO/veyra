//! The map (ticket 0011, step 3): M opens a square picture of the ground
//! round the member, north up, with what the household has found on it.
//!
//! The screen is a mirror and decides no rule. The picture is
//! `veyra_sim::chart::ground`, made on the async compute pool and never on
//! the frame: the screen opens at once with the last picture for its scale,
//! and the word "Charting" lies over it until the new one lands. What stands
//! on the picture is `veyra_sim::chart::marks`, laid out as nodes by the
//! picture's square, so a mark needs no new picture. A deposit is on the map
//! once found and never before, as its real area in its class's colour with
//! the library's deposit symbol at its middle; Enter or a click on one sends
//! the track toggle the register sends.
//!
//! The panel beside the picture names the mark pointed at or focused. Its
//! lines are written in place and its box has a fixed size, so what it says
//! never moves the frame.

use std::sync::Arc;

use bevy::asset::{Assets, Handle, RenderAssetUsages};
use bevy::color::{Alpha, Color};
use bevy::ecs::change_detection::DetectChangesMut;
use bevy::ecs::prelude::*;
use bevy::ecs::system::SystemParam;
use bevy::image::Image;
use bevy::input::ButtonInput;
use bevy::input::keyboard::KeyCode;
use bevy::input_focus::tab_navigation::TabIndex;
use bevy::input_focus::{FocusCause, InputFocus};
use bevy::math::{Rot2, Vec2};
use bevy::picking::Pickable;
use bevy::picking::hover::Hovered;
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};
use bevy::tasks::{AsyncComputeTaskPool, Task, TaskPool, block_on, poll_once};
use bevy::transform::components::Transform;
use bevy::ui::auto_directional_navigation::AutoDirectionalNavigation;
use bevy::ui::widget::{ImageNode, NodeImageMode, Text};
use bevy::ui::{
    AlignItems, BackgroundColor, BorderColor, BorderRadius, JustifyContent, Node, Overflow,
    PositionType, UiRect, UiTransform, Val, Val2, ZIndex,
};
use bevy::ui_widgets::Activate;
use slotted::menu::MenuChoice;
use slotted::theme::Role;
use slotted::theme::Themed;
use slotted::ui::def::ButtonVariant;
use slotted::ui::def::{
    Align, ButtonOpts, Justify, Layout, LayoutDirection, Length, LocKey, Padding, Presentation,
    PresentationMode, ScreenDef, ScreenKind, Tags, TextOpts, TextRole, Transition, UiNodeDef,
    WidgetKind,
};
use slotted::ui::screen::{SpawnCtx, Widget, WidgetRegistry};
use slotted::ui::widgets::{
    on_button_cancel, on_button_click, on_button_drag_end, on_button_press, on_button_release,
};
use slotted::ui::{
    ButtonState, Focusable, InputMode, LocText, ScreenStack, SemanticLabel, SemanticRole, TestId,
    WidgetNode, pop_screen, push_screen, replace_screen,
};
use veyra_sim::chart::{self, Chart, HolderKey, SiteMark, TerritoryPicture};
use veyra_sim::deposits::{
    Compass, DepositId, NearDeposits, Survey, SurveyInbox, SurveyRequest, Surveyed, register_row,
};
use veyra_sim::encounters::{Cleared, PlayedClock};
use veyra_sim::factions::Factions;
use veyra_sim::harvest::{Harvesters, Holding};
use veyra_sim::postings::{Jobs, Known};
use veyra_sim::progression::{Interaction, InteractionKind};
use veyra_sim::quests::{QuestLog, QuestPlaces};
use veyra_sim::{LocalPlayer, ProceduralWorldRes, Registries, WorldPosition};

use super::bench::{RegisterRowView, holding_words, where_words};
use super::strings::{english, grouped};
use super::survey::known_words;
use super::tracker::marker_words;
use strata_ui::fonts::UiFonts;
use strata_ui::painted::Painted;
use strata_ui::sim_screen::{ChangeSource, ScreenStep, Seen, SimScreen, SimScreenApp, moved};
use veyra_sim::view::CombatCamera;

/// The screen kind.
pub const KIND: &str = "strata:map";
/// The key that opens and closes the map.
pub const KEY: KeyCode = KeyCode::KeyM;
/// The widget kind of the picture and its marks.
pub const WIDGET: &str = "strata:map_frame";
/// The screen's root.
pub const ROOT_ID: &str = "map.root";
/// The picture's frame.
pub const FRAME_ID: &str = "map.frame";
/// The word over a picture still being made.
pub const CHARTING_ID: &str = "map.charting";
/// The scale chips' prefix, then the scale's key.
pub const SCALE_TAG: &str = "map.scale:";
/// A mark's test id prefix, then its key.
pub const MARK_TAG: &str = "map.mark:";
/// The Holders chip.
pub const HOLDERS_ID: &str = "map.holders";
/// A faction's legend row, then its place in the view's list.
pub const HOLDER_ROW_TAG: &str = "map.holder:";
/// The close button.
pub const CLOSE_ID: &str = "map.close";
/// The panel's box.
pub const PANEL_ID: &str = "map.panel";
/// The panel's lines, title first.
pub const PANEL_LINES: [&str; 6] = [
    "map.panel.title",
    "map.panel.kind",
    "map.panel.best",
    "map.panel.known",
    "map.panel.lies",
    "map.panel.action",
];

/// How wide the scale of the nearest picture is, metres.
pub const NEAR_SPAN_M: f64 = 3_000.0;
/// How wide the middle picture is, metres.
pub const REGION_SPAN_M: f64 = 12_000.0;
/// How wide the farthest picture is, metres.
pub const FAR_SPAN_M: f64 = 48_000.0;
/// Pixels a side of a picture.
pub const PICTURE_PX: u32 = 512;
/// The share of its span the member may walk from a picture's centre before
/// the picture is made again.
const REMAKE_SHARE: f64 = 0.25;

/// The most factions the legend names, widest ground first, so the column
/// keeps its place beside the panel.
pub const LEGEND_HOLDERS: usize = 4;

/// The screen's width, UI units.
const WIDTH: f32 = 1000.;
/// The most the picture's side is, UI units.
const FRAME_PX: f32 = 580.;
/// The picture's side as a share of the window's height, so it is square
/// and whole in a short window.
const FRAME_VH: f32 = 74.;
/// The right column's width.
const COLUMN_W: f32 = 340.;
/// The panel's height, the same empty or full.
const PANEL_H: f32 = 176.;
/// A mark's button, a side.
const MARK_PX: f32 = 24.;
/// The ring round the tracked deposit's mark.
const RING_PX: f32 = 38.;
/// The least a deposit's disc is drawn across, so a small area is a dot.
const MIN_DISC_PX: f32 = 20.;
/// The member's arrow.
const ARROW_PX: f32 = 32.;
/// The arrow's picture, pixels a side.
const ARROW_IMAGE: u32 = 64;
/// How far inside the picture's edge the off-map arrow stands, as a share.
const EDGE_SHARE: f32 = 0.06;
/// A heading is kept to this many degrees, so a turning camera does not
/// rebuild the screen a frame.
const HEADING_STEP_DEG: f32 = 5.;
/// The share to the right of the picture past which a name goes to the
/// mark's left.
const NAME_FLIP: f32 = 0.62;
/// A name's estimated width a character, height and padding, in nominal
/// picture pixels, for laying names out so none covers another.
const LABEL_CHAR_PX: f32 = 7.;
const LABEL_H: f32 = 22.;
const LABEL_PAD: f32 = 14.;

const HEAD_ROLE: &str = "strata.stores.section";
const CHIP_ROLE: &str = "strata.stores.chip";
const CHIP_ON_ROLE: &str = "strata.stores.chip.on";
const CHIP_TEXT_ROLE: &str = "strata.stores.chip.text";
const CHIP_TEXT_ON_ROLE: &str = "strata.stores.chip.text.on";
const NOTE_ROLE: &str = "strata.survey.note";
const STATUS_ROLE: &str = "strata.stores.status";
const SWATCH_ROLE: &str = "strata.survey.swatch";

/// The ground's dark where no picture has landed yet.
const GROUND: Color = Color::srgb(0.06, 0.08, 0.11);
/// A mark's plate.
const PLATE: Color = Color::srgba(0.04, 0.05, 0.08, 0.86);
/// A mark's plate when it is pointed at or in focus.
const PLATE_LIT: Color = Color::srgba(0.16, 0.18, 0.24, 0.98);
/// The tracked deposit's ring and the focus's border.
const WARM: Color = Color::srgb(1.0, 0.71, 0.33);
/// A site's or a bench's border.
const NEUTRAL: Color = Color::srgb(0.85, 0.88, 0.92);

/// One of the three pictures.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Scale {
    /// A few streets and what is round them.
    Near,
    /// The country round.
    #[default]
    Region,
    /// The land mass.
    Far,
}

impl Scale {
    /// All three, near first.
    pub const ALL: [Self; 3] = [Self::Near, Self::Region, Self::Far];

    /// How many metres a side of its picture is.
    #[must_use]
    pub const fn span_m(self) -> f64 {
        match self {
            Self::Near => NEAR_SPAN_M,
            Self::Region => REGION_SPAN_M,
            Self::Far => FAR_SPAN_M,
        }
    }

    /// The key its chip and its words use.
    #[must_use]
    pub const fn key(self) -> &'static str {
        match self {
            Self::Near => "near",
            Self::Region => "region",
            Self::Far => "far",
        }
    }

    /// Its place among the three.
    #[must_use]
    pub const fn index(self) -> usize {
        match self {
            Self::Near => 0,
            Self::Region => 1,
            Self::Far => 2,
        }
    }

    /// The scale a flag names.
    #[must_use]
    pub fn parse(word: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|scale| scale.key() == word)
    }
}

/// What a mark stands for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MarkKey {
    /// A found deposit in the square.
    Deposit(DepositId),
    /// The tracked deposit past the square's edge.
    Away(DepositId),
    /// A settlement or site, by its name, which holds across pictures.
    Site(String),
    /// A harvester, by number.
    Harvester(u32),
    /// A bench place, by its place in the list.
    Bench(usize),
    /// The area a taken job's camp is heard of in, by the camp's key.
    Heard(u64),
    /// The last ping's strongest point.
    Lead,
    /// Where the questline's current objective leads.
    Quest,
}

impl MarkKey {
    /// The test id of the mark.
    #[must_use]
    pub fn id(&self) -> String {
        let tail = match self {
            Self::Deposit(id) => format!("deposit:{},{}", id.0[0], id.0[1]),
            Self::Away(id) => format!("away:{},{}", id.0[0], id.0[1]),
            Self::Site(name) => format!("site:{name}"),
            Self::Harvester(id) => format!("harvester:{id}"),
            Self::Bench(at) => format!("bench:{at}"),
            Self::Heard(key) => format!("heard:{key}"),
            Self::Lead => "lead".to_owned(),
            Self::Quest => "quest".to_owned(),
        };
        format!("{MARK_TAG}{tail}")
    }

    /// The test id of the deposit's mark.
    #[must_use]
    pub fn deposit_id(id: DepositId) -> String {
        Self::Deposit(id).id()
    }
}

/// What the panel says of a mark.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Words {
    /// Its name.
    pub title: String,
    /// The five lines under it: kind, reading, what is known, where, and
    /// what Enter does.
    pub lines: [String; 5],
}

impl Words {
    /// The six lines of the panel in order.
    fn all(&self) -> [&str; 6] {
        [
            &self.title,
            &self.lines[0],
            &self.lines[1],
            &self.lines[2],
            &self.lines[3],
            &self.lines[4],
        ]
    }
}

/// One mark as the screen lays it out.
#[derive(Debug, Clone, PartialEq)]
pub struct MarkView {
    /// What it stands for.
    pub key: MarkKey,
    /// Where on the picture, `0..=1` across and down.
    pub at: [f32; 2],
    /// Its library symbol; empty for the off-map arrow.
    pub symbol: &'static str,
    /// The name beside it; empty at the farthest scale and for a deposit
    /// that has none to show.
    pub label: String,
    /// Its colour, a deposit's class.
    pub colour: [f32; 4],
    /// The area it covers, as a share of the picture's side; none for a
    /// point.
    pub disc: Option<f32>,
    /// Whether it is the tracked deposit.
    pub tracked: bool,
    /// The way the off-map arrow points, radians clockwise from up.
    pub turn: Option<f32>,
    /// Whether the name goes to the mark's left; none of it when `label` is
    /// empty, which the layout also leaves it when no side is free.
    pub label_left: bool,
    /// What the panel says.
    pub words: Words,
}

/// A faction in the legend.
#[derive(Debug, Clone, PartialEq)]
pub struct HolderView {
    /// Its name.
    pub name: String,
    /// Its colour, opaque.
    pub colour: [f32; 4],
}

/// The map as the screen draws it: the picture's square, the marks on it.
#[derive(Debug, Clone, PartialEq)]
pub struct MapView {
    /// The scale shown.
    pub scale: Scale,
    /// The square the picture (or the blank one) covers.
    pub chart: Chart,
    /// Whether a picture is still being made for this scale.
    pub charting: bool,
    /// Whether the square has a picture to show.
    pub pictured: bool,
    /// The marks, in reading order.
    pub marks: Vec<MarkView>,
    /// Whether the Holders layer is on.
    pub holders: bool,
    /// The factions with ground in the square, the widest first, which the
    /// legend names while the layer is on.
    pub legend: Vec<HolderView>,
    /// The member's place on the picture.
    pub member: [f32; 2],
    /// Which way they face, radians clockwise from north.
    pub heading: f32,
}

/// A picture that has landed.
#[derive(Debug, Clone)]
struct Picture {
    /// The square it covers.
    chart: Chart,
    /// The image the frame shows; none when there is nowhere to keep one.
    image: Option<Handle<Image>>,
    /// The settlements and sites in its square, made on the same task: they
    /// come off the baked network over most of the square, not for a frame.
    sites: Arc<Vec<SiteMark>>,
    /// Whose ground it is, as a tint to lay over the image; none without a
    /// world's factions.
    territory: Option<Handle<Image>>,
    /// The factions with ground in the square, the widest first.
    holders: Arc<Vec<HolderKey>>,
}

/// What a task makes for a picture: its bytes, the sites in its square and
/// whose ground it is.
struct Made {
    ground: Vec<u8>,
    sites: Vec<SiteMark>,
    territory: Option<TerritoryPicture>,
}

/// The open map and the pictures kept for the three scales.
#[derive(Resource, Default)]
pub struct MapState {
    scale: Scale,
    holders: bool,
    pictures: [Option<Picture>; 3],
    tasks: [Option<(Chart, Task<Made>)>; 3],
    asked: [Option<Chart>; 3],
    asks: u32,
    landed: u32,
    arrow: Option<Handle<Image>>,
    /// What the open screen was last built from.
    built: Option<Shown>,
    /// A press asked for a rebuild whatever the inputs say.
    redo: bool,
    /// The mark last in focus, kept over the rebuilds.
    focus: Option<String>,
    refocus: Option<String>,
    /// The mark last written to the panel.
    shown: Option<String>,
    views: u32,
}

impl MapState {
    /// The scale shown.
    #[must_use]
    pub const fn scale(&self) -> Scale {
        self.scale
    }

    /// Whether the Holders layer is on.
    #[must_use]
    pub const fn holders(&self) -> bool {
        self.holders
    }

    /// The view the open screen was built from.
    #[must_use]
    pub fn view(&self) -> Option<&MapView> {
        self.built.as_ref().map(|shown| &*shown.view)
    }

    /// The square last asked for at `scale`: what a test reads to know a
    /// picture was asked for.
    #[must_use]
    pub fn asked(&self, scale: Scale) -> Option<Chart> {
        self.asked[scale.index()]
    }

    /// How many pictures have been asked for since the game began.
    #[must_use]
    pub const fn asks(&self) -> u32 {
        self.asks
    }

    /// How many views have been built, which a test holds still over idle
    /// frames.
    #[must_use]
    pub const fn views(&self) -> u32 {
        self.views
    }

    /// Whether `scale` has a picture that has landed.
    #[must_use]
    pub fn has_picture(&self, scale: Scale) -> bool {
        self.pictures[scale.index()].is_some()
    }

    /// The square a picture is wanted for at `scale` with the member at
    /// `at`: none while the one asked for or kept is near enough.
    fn wanted(&self, scale: Scale, at: [f64; 2]) -> Option<Chart> {
        let reference = self.asked[scale.index()]
            .or_else(|| self.pictures[scale.index()].as_ref().map(|each| each.chart));
        let span = scale.span_m();
        match reference {
            Some(chart)
                if (at[0] - chart.centre[0]).hypot(at[1] - chart.centre[1])
                    <= REMAKE_SHARE * span =>
            {
                None
            }
            _ => Some(Chart::square(at, span, PICTURE_PX)),
        }
    }

    /// Whether the picture shown is not yet the one wanted.
    fn charting(&self, scale: Scale, at: [f64; 2]) -> bool {
        let landed = self.pictures[scale.index()].as_ref().map(|each| each.chart);
        self.wanted(scale, at).is_some()
            || self.tasks[scale.index()].is_some()
            || self.asked[scale.index()].is_some_and(|asked| Some(asked) != landed)
    }
}

/// What the view is made from, so a frame in which none of it moved is known
/// without building the view.
#[derive(Debug, Clone, PartialEq)]
struct Gathered {
    scale: Scale,
    holders: bool,
    chart: Chart,
    charting: bool,
    pictured: bool,
    landed: u32,
    /// The member's place in whole picture pixels, which is what the screen
    /// is rebuilt on.
    at: [f64; 2],
    /// Where they stand exactly, for the words; never a reason to rebuild.
    here: Here,
    heading: i32,
    surveyed: Surveyed,
    harvesters: Standing,
    benches: Vec<[i64; 2]>,
    /// The work taken and the camps found, which decide what a camp shows.
    camps: (Jobs, Known),
    /// Where the journal's current objective leads.
    quest: Option<super::journal::QuestTarget>,
}

/// The harvesters as the map shows them. A machine's `carry` moves every
/// tick it works, which no mark shows, so only what a mark and its panel
/// say is compared: a unit made rebuilds the screen, the work between does
/// not.
#[derive(Debug, Clone)]
struct Standing(Harvesters);

impl PartialEq for Standing {
    fn eq(&self, other: &Self) -> bool {
        let shown = |list: &Harvesters| {
            list.list
                .iter()
                .map(|each| {
                    (
                        each.id,
                        each.deposit,
                        each.at.map(f64::to_bits),
                        each.held,
                        each.hopper,
                    )
                })
                .collect::<Vec<_>>()
        };
        shown(&self.0) == shown(&other.0)
    }
}

/// A position that compares equal to every other, so a standing character's
/// drift is carried to the words without rebuilding the screen.
#[derive(Debug, Clone, Copy)]
struct Here([f64; 2]);

impl PartialEq for Here {
    fn eq(&self, _: &Self) -> bool {
        true
    }
}

/// What the screen shows and what it was made from. Two are equal when
/// they were made from the same things: the words the member's exact place
/// gives are carried by the next rebuild, never a reason for one.
#[derive(Clone)]
pub struct Shown {
    key: Gathered,
    view: Arc<MapView>,
}

impl PartialEq for Shown {
    fn eq(&self, other: &Self) -> bool {
        self.key == other.key
    }
}

fn on_top(stack: &ScreenStack) -> bool {
    stack.top().is_some_and(|top| top.kind == kind())
}

/// The screen kind.
#[must_use]
pub fn kind() -> ScreenKind {
    ScreenKind::new(KIND)
}

fn words(key: &'static str) -> &'static str {
    english(key).unwrap_or(key)
}

fn key(s: impl Into<String>) -> LocKey {
    LocKey(s.into())
}

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

/// A line that ends in an ellipsis at the column's edge and not past it.
fn one_line(mut node: UiNodeDef) -> UiNodeDef {
    if let UiNodeDef::Text { opts, .. } = &mut node {
        opts.wrap = true;
        opts.max_lines = Some(1);
    }
    node
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

/// A scale's chip.
fn chip(scale: Scale, on: bool) -> UiNodeDef {
    let id = format!("{SCALE_TAG}{}", scale.key());
    let name = words(match scale {
        Scale::Near => "strata.map.scale.near",
        Scale::Region => "strata.map.scale.region",
        Scale::Far => "strata.map.scale.far",
    });
    let label = if on {
        format!("{}{name}", super::weapon_diagram::FOCUS_MARK)
    } else {
        name.to_owned()
    };
    UiNodeDef::Card {
        role: Role::new(if on { CHIP_ON_ROLE } else { CHIP_ROLE }),
        layout: Layout {
            padding: Padding {
                top: 0.6,
                right: 1.4,
                bottom: 0.6,
                left: 1.4,
            },
            ..row(1.)
        },
        children: vec![line_in(
            label,
            if on {
                CHIP_TEXT_ON_ROLE
            } else {
                CHIP_TEXT_ROLE
            },
        )],
        disabled: false,
        tags: Tags::new().with("menu", &id).with(Tags::TEST_ID, &id),
    }
}

/// The Holders chip: off by default, a mark in a word's place when on.
fn holders_chip(on: bool) -> UiNodeDef {
    let name = words("strata.map.holders");
    let label = if on {
        format!("{}{name}", super::weapon_diagram::FOCUS_MARK)
    } else {
        name.to_owned()
    };
    UiNodeDef::Card {
        role: Role::new(if on { CHIP_ON_ROLE } else { CHIP_ROLE }),
        layout: Layout {
            padding: Padding {
                top: 0.6,
                right: 1.4,
                bottom: 0.6,
                left: 1.4,
            },
            ..row(1.)
        },
        children: vec![line_in(
            label,
            if on {
                CHIP_TEXT_ON_ROLE
            } else {
                CHIP_TEXT_ROLE
            },
        )],
        disabled: false,
        tags: Tags::new()
            .with("menu", HOLDERS_ID)
            .with(Tags::TEST_ID, HOLDERS_ID),
    }
}

/// While the layer is on: a swatch and name for each faction with ground in
/// the view, or a line saying there is none.
fn holder_rows(view: &MapView) -> Vec<UiNodeDef> {
    if !view.holders {
        return Vec::new();
    }
    if view.legend.is_empty() {
        return vec![line_in(words("strata.map.legend.holders.none"), NOTE_ROLE)];
    }
    view.legend
        .iter()
        .enumerate()
        .map(|(at, each)| {
            with_id(
                panel(
                    row(1.2),
                    vec![
                        UiNodeDef::Panel {
                            role: Role::new(SWATCH_ROLE),
                            layout: Layout {
                                width: Some(Length::Px(10.)),
                                height: Some(Length::Px(10.)),
                                ..column(0.)
                            },
                            children: Vec::new(),
                            tags: Tags::new()
                                .with(Tags::TEST_ID, &format!("map.swatch:holder:{at}")),
                        },
                        one_line(line_in(each.name.clone(), NOTE_ROLE)),
                    ],
                ),
                &format!("{HOLDER_ROW_TAG}{at}"),
            )
        })
        .collect()
}

/// What a legend row draws before its words.
#[derive(Clone, Copy)]
enum Sign {
    /// Nothing: the words stand alone.
    None,
    /// A library symbol.
    Icon(&'static str),
    /// A swatch of a colour the picture is drawn in, by name.
    Swatch(&'static str),
}

/// The legend: a line a kind of mark and of road.
const LEGEND: [(Sign, &str); 14] = [
    (Sign::None, "strata.map.legend.you"),
    (
        Sign::Icon("veyra:icon.map.settlement"),
        "strata.map.legend.settlement",
    ),
    (
        Sign::Icon("veyra:icon.map.mission"),
        "strata.map.legend.site",
    ),
    (Sign::Icon("veyra:icon.map.camp"), "strata.map.legend.camp"),
    (
        Sign::Icon("veyra:icon.map.water"),
        "strata.map.legend.machine",
    ),
    (
        Sign::Icon("veyra:icon.map.danger"),
        "strata.map.legend.heard",
    ),
    (
        Sign::Icon("veyra:icon.map.deposit"),
        "strata.map.legend.deposit",
    ),
    (
        Sign::Icon("veyra:icon.map.waypoint"),
        "strata.map.legend.lead",
    ),
    (
        Sign::Icon("veyra:icon.map.power"),
        "strata.map.legend.harvester",
    ),
    (
        Sign::Icon("veyra:icon.map.workshop"),
        "strata.map.legend.bench",
    ),
    (Sign::Swatch("service"), "strata.map.legend.service"),
    (Sign::Swatch("track"), "strata.map.legend.track"),
    (Sign::Swatch("trail"), "strata.map.legend.trail"),
    (Sign::Swatch("water"), "strata.map.legend.water"),
];

/// The colour a swatch is drawn in, from the picture's own constants.
fn swatch_colour(name: &str) -> Option<Color> {
    let [r, g, b] = match name {
        "service" => chart::SERVICE,
        "track" => chart::TRACK,
        "trail" => chart::TRAIL,
        "water" => chart::RIVER_LIGHT,
        _ => return None,
    };
    Some(Color::srgb(r / 255., g / 255., b / 255.))
}

fn legend_row(sign: Sign, text: &'static str) -> UiNodeDef {
    let lead = match sign {
        Sign::None => panel(
            Layout {
                width: Some(Length::Px(16.)),
                ..column(0.)
            },
            Vec::new(),
        ),
        Sign::Icon(id) => super::icons::node(id, 16., None, false, None),
        Sign::Swatch(name) => UiNodeDef::Panel {
            role: Role::new(SWATCH_ROLE),
            layout: Layout {
                width: Some(Length::Px(10.)),
                height: Some(Length::Px(10.)),
                ..column(0.)
            },
            children: Vec::new(),
            tags: Tags::new().with(Tags::TEST_ID, &format!("map.swatch:{name}")),
        },
    };
    panel(row(1.2), vec![lead, line_in(words(text), NOTE_ROLE)])
}

/// The screen for `view`, with `shown` in the panel.
#[must_use]
#[allow(
    clippy::too_many_lines,
    reason = "the head, the column, the body and the foot of one screen"
)]
pub fn screen_def(view: &MapView, shown: Option<&Words>) -> ScreenDef {
    let head = panel(
        Layout {
            align: Some(Align::End),
            ..row(2.)
        },
        vec![
            UiNodeDef::Text {
                key: key("strata.map.title"),
                style: TextRole::Title,
                opts: TextOpts {
                    wrap: false,
                    role: Some(Role::new("panel.title")),
                    ..TextOpts::default()
                },
                tags: Tags::new(),
            },
            line_in(words("strata.map.sub"), HEAD_ROLE),
        ],
    );
    let none = Words {
        title: words("strata.map.panel.none").to_owned(),
        ..Words::default()
    };
    let said = shown.unwrap_or(&none).all();
    let lines: Vec<UiNodeDef> = PANEL_LINES
        .iter()
        .zip(said)
        .enumerate()
        .map(|(at, (id, text))| {
            with_id(
                one_line(line_in(text, if at == 0 { HEAD_ROLE } else { NOTE_ROLE })),
                id,
            )
        })
        .collect();
    let right = panel(
        Layout {
            width: Some(Length::Px(COLUMN_W)),
            align: Some(Align::Stretch),
            ..column(1.4)
        },
        vec![
            panel(
                row(1.),
                Scale::ALL.map(|s| chip(s, s == view.scale)).to_vec(),
            ),
            panel(
                Layout {
                    align: Some(Align::Center),
                    ..row(1.)
                },
                vec![
                    line_in(words("strata.map.legend"), HEAD_ROLE),
                    UiNodeDef::Spacer {
                        size: Length::Percent(100.),
                        tags: Tags::new(),
                    },
                    holders_chip(view.holders),
                ],
            ),
            panel(
                column(0.8),
                LEGEND
                    .iter()
                    .map(|(sign, text)| legend_row(*sign, text))
                    .chain(holder_rows(view))
                    .collect(),
            ),
            UiNodeDef::Panel {
                role: Role::new("panel"),
                layout: Layout {
                    height: Some(Length::Px(PANEL_H)),
                    min_height: Some(Length::Px(PANEL_H)),
                    max_height: Some(Length::Px(PANEL_H)),
                    padding: Padding::all(1.4),
                    align: Some(Align::Stretch),
                    ..column(0.6)
                },
                children: lines,
                tags: Tags::new().with(Tags::TEST_ID, PANEL_ID),
            },
        ],
    );
    let body = panel(
        Layout {
            align: Some(Align::Start),
            grow: 1.,
            min_height: Some(Length::Px(0.)),
            ..row(3.)
        },
        vec![
            UiNodeDef::Custom {
                kind: WidgetKind::new(WIDGET),
                params: slotted::registry::Value::Unit,
                children: Vec::new(),
                tags: Tags::new().with(Tags::TEST_ID, FRAME_ID),
            },
            right,
        ],
    );
    let footer = panel(
        row(2.),
        vec![
            line_in(words("strata.map.keys"), STATUS_ROLE),
            UiNodeDef::Spacer {
                size: Length::Percent(100.),
                tags: Tags::new(),
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
        initial_focus: None,
        presentation: Presentation {
            mode: PresentationMode::Page,
            scrim: Some(true),
            transition: Transition::Fade,
            wildcards: Some(false),
            ..Presentation::default()
        },
    }
}

/// The symbol a site of `kind` is drawn with: the mission mark unless the kind
/// has a nearer one; the camp symbol is the outlaw camp's alone.
#[must_use]
pub fn site_symbol(kind: &str) -> &'static str {
    match kind {
        "strata:town" => "veyra:icon.map.settlement",
        "strata:hamlet" => "veyra:icon.map.home",
        "strata:landing" => "veyra:icon.map.transit",
        "strata:wild_camp" => "veyra:icon.map.camp",
        // The library holds no wreck mark, so a wreck keeps the landmark's;
        // the water mark is the pump, which a derelict machine often was.
        "strata:wreck_works" => "veyra:icon.map.water",
        _ => "veyra:icon.map.mission",
    }
}

/// A site's words for the panel: its kind, who holds it where a faction
/// does, and where it lies from the member.
fn site_words(site: &SiteMark, lies: String, cleared: bool) -> Words {
    Words {
        title: site.name.clone(),
        lines: [
            if site.settlement {
                words("strata.map.panel.settlement").to_owned()
            } else {
                site.kind_name.clone()
            },
            site.held_by.as_ref().map_or_else(String::new, |faction| {
                words("strata.map.panel.held").replace("{faction}", faction)
            }),
            if cleared {
                words("strata.map.panel.cleared").to_owned()
            } else {
                String::new()
            },
            lies,
            String::new(),
        ],
    }
}

/// A deposit's words for the panel, from the register's own row view.
pub(crate) fn deposit_words(
    registries: &strata_registry::GameRegistries,
    surveyed: &Surveyed,
    deposit: &veyra_sim::deposits::Deposit,
    from: [f64; 2],
    away: bool,
    books: Option<&super::claims::Books<'_>>,
) -> Words {
    let row = register_row(&registries.resources, surveyed, deposit, from);
    let view = RegisterRowView::of(registries, &row);
    // Whose claim it is, beside what the household knows of it.
    let known = match books.and_then(|books| books.claim_words(deposit)) {
        Some(claim) => format!("{} \u{b7} {claim}", known_words(surveyed, deposit.id)),
        None => known_words(surveyed, deposit.id),
    };
    Words {
        title: view.place,
        lines: [
            view.kind,
            if view.best.is_empty() {
                words("strata.card.deposit.unread").to_owned()
            } else {
                view.best
            },
            known,
            if away {
                words("strata.map.panel.off").replace("{lies}", &view.lies)
            } else {
                view.lies
            },
            words(if row.tracked {
                "strata.map.panel.untrack"
            } else {
                "strata.map.panel.track"
            })
            .to_owned(),
        ],
    }
}

/// What a posting's place kind is called on the map; a posting saved before
/// kinds were kept is a camp.
fn kind_name_of(kind: &str) -> String {
    let kind = if kind.is_empty() {
        "strata:wild_camp"
    } else {
        kind
    };
    super::strings::english(&format!("strata.place.kind.{kind}"))
        .unwrap_or("Outlaw camp")
        .to_owned()
}

/// What the map shows of the camps among `sites`: every other site, a camp
/// only once it is found, and for each taken job whose camp is not found the
/// job, whose heard-of area stands where the camp's point would. A camp
/// neither found nor heard of is not on the map.
fn camp_marks<'a>(
    sites: &'a [SiteMark],
    camp_kinds: &[String],
    known: &Known,
    jobs: &'a Jobs,
) -> (Vec<&'a SiteMark>, Vec<&'a veyra_sim::postings::Job>) {
    let shown = sites
        .iter()
        .filter(|site| {
            !camp_kinds.contains(&site.kind) || site.id.is_some_and(|id| known.is_found(id.key()))
        })
        .collect();
    let heard = jobs
        .taken
        .iter()
        .filter(|(key, _)| !known.is_found(**key))
        .map(|(_, job)| job)
        .collect();
    (shown, heard)
}

/// The colour of a heard-of area: warm, and told from a deposit's classes by
/// the danger sign on it and the word under it.
const HEARD: [f32; 4] = [0.93, 0.45, 0.28, 1.];

fn unit(chart: &Chart, at: [f64; 2]) -> [f32; 2] {
    let [x, y] = chart.unit(at[0], at[1]);
    #[allow(clippy::cast_possible_truncation, reason = "a share of a picture")]
    [x as f32, y as f32]
}

/// The mark of the objective the journal points to, or none when it lies
/// outside the picture.
fn quest_mark(
    chart: &Chart,
    from: [f64; 2],
    target: &super::journal::QuestTarget,
) -> Option<MarkView> {
    if !chart.touches(target.at[0], target.at[1], 0.0) {
        return None;
    }
    let metres = (target.at[0] - from[0]).hypot(target.at[1] - from[1]);
    Some(MarkView {
        key: MarkKey::Quest,
        at: unit(chart, target.at),
        symbol: "veyra:icon.map.mission",
        label: String::new(),
        colour: [1., 0.71, 0.33, 1.],
        disc: None,
        tracked: false,
        turn: None,
        label_left: false,
        words: Words {
            title: words("strata.map.panel.quest").to_owned(),
            lines: [
                words("strata.map.panel.quest.kind").to_owned(),
                target.name.clone(),
                String::new(),
                where_words(metres, Compass::of(from, target.at)),
                String::new(),
            ],
        },
    })
}

/// The view of what was gathered. Marks are asked of the sim on this thread:
/// they are lookups over the baked network, and the game bakes it before it
/// starts.
#[allow(
    clippy::too_many_lines,
    reason = "one list of the kinds of mark, each laid out beside the next"
)]
fn view_of(inputs: &MapInputs<'_, '_>, state: &MapState, g: &Gathered) -> Option<MapView> {
    let registries = &inputs.registries.as_deref()?.0;
    let (sites, holders) = state.pictures[g.scale.index()]
        .as_ref()
        .filter(|picture| picture.chart == g.chart)
        .map_or_else(Default::default, |picture| {
            (Arc::clone(&picture.sites), Arc::clone(&picture.holders))
        });
    let survey = inputs.survey.as_deref().map(|survey| &survey.0);
    let near = inputs.near.as_deref();
    let lookup = |id: DepositId| {
        survey
            .and_then(|field| field.get(id))
            .or_else(|| near.and_then(|near| near.get(id).cloned()))
    };
    let chart = g.chart;
    let books = inputs.books.books();
    let marked = chart::marks(&sites, &lookup, &g.surveyed, &g.harvesters.0, &chart);
    let named = !matches!(g.scale, Scale::Far);
    let from = g.here.0;
    let set = &registries.resources;
    let (cleared, clock) = (inputs.cleared.as_deref(), inputs.played.as_deref());
    let quiet = |site: &SiteMark| {
        site.id
            .zip(cleared.zip(clock))
            .is_some_and(|(id, (cleared, clock))| {
                veyra_sim::encounters::is_cleared(
                    &registries.encounters,
                    cleared,
                    clock.seconds,
                    &site.kind,
                    id.key(),
                )
            })
    };
    let mut marks = Vec::new();
    let camp_kinds: Vec<String> = registries
        .encounters
        .iter()
        .filter_map(|def| def.at.site().map(ToString::to_string))
        .collect();
    let (shown_sites, heard) = camp_marks(&marked.sites, &camp_kinds, &g.camps.1, &g.camps.0);
    for site in shown_sites {
        let metres = (site.at[0] - from[0]).hypot(site.at[1] - from[1]);
        let lies = where_words(metres, Compass::of(from, site.at));
        marks.push(MarkView {
            key: MarkKey::Site(site.name.clone()),
            at: unit(&chart, site.at),
            symbol: if site.settlement {
                "veyra:icon.map.settlement"
            } else {
                site_symbol(&site.kind)
            },
            label: if named {
                site.name.clone()
            } else {
                String::new()
            },
            colour: [0.85, 0.88, 0.92, 1.],
            disc: None,
            tracked: false,
            turn: None,
            label_left: false,
            words: site_words(site, lies, quiet(site)),
        });
    }
    for job in heard {
        let area = &job.posting.heard;
        if !chart.touches(area.centre[0], area.centre[1], area.radius) {
            continue;
        }
        let metres = (area.centre[0] - from[0]).hypot(area.centre[1] - from[1]);
        marks.push(MarkView {
            key: MarkKey::Heard(job.posting.key),
            at: unit(&chart, area.centre),
            symbol: "veyra:icon.map.danger",
            label: if named {
                words("strata.map.heard.label").replace("{camp}", &job.posting.name)
            } else {
                String::new()
            },
            colour: HEARD,
            #[allow(clippy::cast_possible_truncation, reason = "a share of a picture")]
            disc: Some((2. * area.radius / chart.span_m) as f32),
            tracked: false,
            turn: None,
            label_left: false,
            words: Words {
                title: job.posting.name.clone(),
                lines: [
                    words("strata.map.heard.kind")
                        .replace("{kind}", &kind_name_of(&job.posting.kind)),
                    words("strata.map.heard.job").replace("{pay}", &grouped(job.posting.pay)),
                    words("strata.map.heard.unfound").to_owned(),
                    where_words(metres, Compass::of(from, area.centre)),
                    String::new(),
                ],
            },
        });
    }
    for each in &marked.deposits {
        let deposit = &each.deposit;
        let colour = set.class_of(deposit.ty).color.0;
        let said = deposit_words(
            registries,
            &g.surveyed,
            deposit,
            from,
            false,
            books.as_ref(),
        );
        let label = if named {
            words("strata.map.label")
                .replace("{place}", &said.title)
                .replace("{reading}", &said.lines[1])
        } else {
            String::new()
        };
        marks.push(MarkView {
            key: MarkKey::Deposit(deposit.id),
            at: unit(&chart, deposit.at),
            symbol: "veyra:icon.map.deposit",
            label,
            colour,
            #[allow(clippy::cast_possible_truncation, reason = "a share of a picture")]
            disc: Some((2. * f64::from(deposit.radius_m) / chart.span_m) as f32),
            tracked: each.tracked,
            turn: None,
            label_left: false,
            words: said,
        });
    }
    if let Some(each) = &marked.tracked_away {
        let deposit = &each.deposit;
        let said = deposit_words(registries, &g.surveyed, deposit, from, true, books.as_ref());
        // On the edge toward it, along the line from the square's middle.
        let off = [
            deposit.at[0] - chart.centre[0],
            deposit.at[1] - chart.centre[1],
        ];
        let reach = off[0].abs().max(off[1].abs()).max(1e-6);
        #[allow(clippy::cast_possible_truncation, reason = "a share of a picture")]
        let (across, up) = (
            -(off[0] / reach) as f32 * (0.5 - EDGE_SHARE),
            (off[1] / reach) as f32 * (0.5 - EDGE_SHARE),
        );
        #[allow(clippy::cast_possible_truncation, reason = "radians of a bearing")]
        let turn = (-off[0]).atan2(off[1]) as f32;
        let metres = deposit.distance_m(from[0], from[1]);
        marks.push(MarkView {
            key: MarkKey::Away(deposit.id),
            at: [0.5 + across, 0.5 - up],
            symbol: "",
            label: marker_words(&deposit.place, metres),
            colour: set.class_of(deposit.ty).color.0,
            disc: None,
            tracked: true,
            turn: Some(turn),
            label_left: false,
            words: said,
        });
    }
    if let Some(lead) = &marked.lead {
        let metres = (lead.at[0] - from[0]).hypot(lead.at[1] - from[1]);
        let place = lookup(lead.deposit).map(|deposit| deposit.place);
        marks.push(MarkView {
            key: MarkKey::Lead,
            at: unit(&chart, lead.at),
            symbol: "veyra:icon.map.waypoint",
            label: String::new(),
            colour: [1., 0.71, 0.33, 1.],
            disc: None,
            tracked: false,
            turn: None,
            label_left: false,
            words: Words {
                title: words("strata.map.panel.lead").to_owned(),
                lines: [
                    words("strata.map.panel.lead.kind").to_owned(),
                    words("strata.map.panel.lead.best")
                        .replace("{pct}", &(u32::from(lead.pm) / 10).to_string())
                        .replace("{place}", &place.unwrap_or_default()),
                    String::new(),
                    where_words(metres, Compass::of(from, lead.at)),
                    String::new(),
                ],
            },
        });
    }
    marks.extend(
        g.quest
            .as_ref()
            .and_then(|target| quest_mark(&chart, from, target)),
    );
    for each in &marked.harvesters {
        let metres = (each.at[0] - from[0]).hypot(each.at[1] - from[1]);
        let place = lookup(each.deposit).map(|deposit| deposit.place);
        marks.push(MarkView {
            key: MarkKey::Harvester(each.id),
            at: unit(&chart, each.at),
            symbol: "veyra:icon.map.power",
            label: String::new(),
            colour: [0.85, 0.88, 0.92, 1.],
            disc: None,
            tracked: false,
            turn: None,
            label_left: false,
            words: Words {
                title: words("strata.map.panel.harvester").to_owned(),
                lines: [
                    words("strata.map.panel.harvester.on")
                        .replace("{place}", &place.unwrap_or_default()),
                    holding_words(&Holding {
                        count: 1,
                        held: each.held,
                        hopper: each.hopper,
                        full: each.full,
                    }),
                    String::new(),
                    where_words(metres, Compass::of(from, each.at)),
                    String::new(),
                ],
            },
        });
    }
    for (at, spot) in g.benches.iter().enumerate() {
        #[allow(clippy::cast_precision_loss, reason = "whole metres")]
        let spot = [spot[0] as f64, spot[1] as f64];
        if !chart.touches(spot[0], spot[1], 0.0) {
            continue;
        }
        let metres = (spot[0] - from[0]).hypot(spot[1] - from[1]);
        marks.push(MarkView {
            key: MarkKey::Bench(at),
            at: unit(&chart, spot),
            symbol: "veyra:icon.map.workshop",
            label: String::new(),
            colour: [0.85, 0.88, 0.92, 1.],
            disc: None,
            tracked: false,
            turn: None,
            label_left: false,
            words: Words {
                title: words("strata.map.panel.bench").to_owned(),
                lines: [
                    words("strata.map.panel.bench.kind").to_owned(),
                    String::new(),
                    String::new(),
                    where_words(metres, Compass::of(from, spot)),
                    String::new(),
                ],
            },
        });
    }
    lay_out_names(&mut marks);
    // Reading order: top to bottom in bands, then left to right.
    marks.sort_by(|a, b| {
        #[allow(clippy::cast_possible_truncation, reason = "a band of a picture")]
        let band = |m: &MarkView| (m.at[1] * 24.).floor() as i32;
        band(a)
            .cmp(&band(b))
            .then(a.at[0].total_cmp(&b.at[0]))
            .then_with(|| a.key.id().cmp(&b.key.id()))
    });
    #[allow(clippy::cast_precision_loss, reason = "a few degrees")]
    let heading = (g.heading as f32).to_radians();
    Some(MapView {
        scale: g.scale,
        chart,
        charting: g.charting,
        pictured: g.pictured,
        marks,
        holders: g.holders,
        legend: holders
            .iter()
            .take(LEGEND_HOLDERS)
            .map(|each| {
                let [r, g, b] = each.colour.map(|c| f32::from(c) / 255.);
                HolderView {
                    name: each.name.clone(),
                    colour: [r, g, b, 1.],
                }
            })
            .collect(),
        member: unit(&chart, from),
        heading,
    })
}

/// Gives every name a side of its mark that no earlier name or mark covers,
/// the deposits' first (their readings are what a player came for), then the
/// settlements, then the rest; a name with no free side is left off the
/// picture, and the panel still says it. Boxes are estimated on a nominal
/// picture, as the text is not measured until it is drawn.
fn lay_out_names(marks: &mut [MarkView]) {
    let rank = |mark: &MarkView| match mark.key {
        MarkKey::Deposit(_) | MarkKey::Away(_) => 0,
        MarkKey::Site(_) if mark.symbol == "veyra:icon.map.settlement" => 1,
        _ => 2,
    };
    let at = |mark: &MarkView| [mark.at[0] * FRAME_PX, mark.at[1] * FRAME_PX];
    // The marks' own boxes are taken first: a name may not hide a symbol.
    let mut taken: Vec<[f32; 4]> = marks
        .iter()
        .map(|mark| {
            let [x, y] = at(mark);
            let half = MARK_PX / 2.;
            [x - half, y - half, x + half, y + half]
        })
        .collect();
    let mut order: Vec<usize> = (0..marks.len()).collect();
    order.sort_by_key(|&k| rank(&marks[k]));
    for k in order {
        if marks[k].label.is_empty() {
            continue;
        }
        let [x, y] = at(&marks[k]);
        #[allow(clippy::cast_precision_loss, reason = "a few dozen characters")]
        let width = marks[k].label.chars().count() as f32 * LABEL_CHAR_PX + LABEL_PAD;
        let right = [
            x + MARK_PX / 2. + 2.,
            y - LABEL_H / 2.,
            0.,
            y + LABEL_H / 2.,
        ];
        let right = [right[0], right[1], right[0] + width, right[3]];
        let left = [
            x - MARK_PX / 2. - 2. - width,
            right[1],
            x - MARK_PX / 2. - 2.,
            right[3],
        ];
        let sides = if marks[k].at[0] > NAME_FLIP {
            [(left, true), (right, false)]
        } else {
            [(right, false), (left, true)]
        };
        let free = sides.into_iter().find(|(each, _)| {
            each[0] >= 0.
                && each[2] <= FRAME_PX
                && taken.iter().all(|other| {
                    each[2] <= other[0]
                        || other[2] <= each[0]
                        || each[3] <= other[1]
                        || other[3] <= each[1]
                })
        });
        if let Some((each, left)) = free {
            marks[k].label_left = left;
            taken.push(each);
        } else {
            marks[k].label.clear();
        }
    }
}

/// The machines as the map compares them: where each stands and what it
/// holds. A machine's `carry` moves every tick it works and no mark shows it.
type Planted = Vec<(u32, DepositId, [u64; 2], u32, u32)>;

fn planted(list: &Harvesters) -> Planted {
    list.list
        .iter()
        .map(|each| {
            (
                each.id,
                each.deposit,
                each.at.map(f64::to_bits),
                each.held,
                each.hopper,
            )
        })
        .collect()
}

/// What the map reads: the data, the household's findings, the camps heard
/// of, the quest log, the deposit field, the books the words come from, the
/// local player's place, the camera's heading and the workbenches. The
/// place, the heading, the benches and the machines count by what the key
/// holds of them, as `Gathered` did; the books, the clock and the cleared
/// camps only word the marks and have never been a reason to rebuild.
#[derive(SystemParam)]
pub struct MapInputs<'w, 's> {
    registries: Option<Res<'w, Registries>>,
    surveyed: Option<Res<'w, Surveyed>>,
    harvesters: Option<Res<'w, Harvesters>>,
    jobs: Option<Res<'w, Jobs>>,
    known: Option<Res<'w, Known>>,
    log: Option<Res<'w, QuestLog>>,
    places: Option<Res<'w, QuestPlaces>>,
    near: Option<Res<'w, NearDeposits>>,
    survey: Option<Res<'w, Survey>>,
    cleared: Option<Res<'w, Cleared>>,
    played: Option<Res<'w, PlayedClock>>,
    books: super::claims::Standing<'w>,
    players: Query<'w, 's, &'static WorldPosition, With<LocalPlayer>>,
    cameras: Query<'w, 's, &'static Transform, With<CombatCamera>>,
    props: Query<'w, 's, (&'static Interaction, &'static WorldPosition)>,
    spot: Local<'s, Seen<[i64; 2]>>,
    turn: Local<'s, Seen<i32>>,
    benches: Local<'s, Seen<Vec<[i64; 2]>>>,
    standing: Local<'s, Seen<Planted>>,
}

impl MapInputs<'_, '_> {
    /// The camera's heading in whole steps of [`HEADING_STEP_DEG`], degrees.
    fn heading(&self) -> i32 {
        let heading = self.cameras.iter().next().map_or(0., |camera| {
            let ahead = camera.forward();
            // North is up and `+x` lies to the west, so turning toward
            // `-x` is turning right, clockwise on the picture.
            (-ahead.x).atan2(ahead.z).to_degrees()
        });
        #[allow(clippy::cast_possible_truncation, reason = "degrees to a step")]
        let steps = (heading / HEADING_STEP_DEG).round() as i32 * HEADING_STEP_DEG as i32;
        steps
    }

    /// Where the workbenches stand, in whole metres.
    fn benches(&self) -> Vec<[i64; 2]> {
        self.props
            .iter()
            .filter(|(interaction, _)| interaction.kind == InteractionKind::Workbench)
            .map(|(_, at)| {
                #[allow(clippy::cast_possible_truncation, reason = "whole metres")]
                [at.0.x().round() as i64, at.0.z().round() as i64]
            })
            .collect()
    }

    /// What the view is made from, or none without a local player.
    fn gather(&self, state: &MapState) -> Option<Gathered> {
        let registries = &self.registries.as_deref()?.0;
        let at = {
            let at = self.players.single().ok()?;
            [at.0.x(), at.0.z()]
        };
        let scale = state.scale;
        let picture = state.pictures[scale.index()].as_ref();
        // Where the member is drawn is in whole picture pixels, so a standing
        // character's drift does not rebuild the screen; and a square that has
        // no picture yet is the one asked for, or one on that rounded place,
        // so a coasting bike does not rebuild it every frame while it waits.
        let here = at;
        let step = (scale.span_m() / f64::from(PICTURE_PX)).max(1.);
        let at = [(at[0] / step).round() * step, (at[1] / step).round() * step];
        let chart = picture
            .map(|picture| picture.chart)
            .or(state.asked[scale.index()])
            .unwrap_or_else(|| Chart::square(at, scale.span_m(), PICTURE_PX));
        let quest = self
            .log
            .as_deref()
            .zip(self.places.as_deref())
            .and_then(|(log, places)| {
                super::journal::target_of(
                    registries,
                    log,
                    places,
                    self.near.as_deref().zip(self.surveyed.as_deref()),
                    here,
                )
            });
        Some(Gathered {
            scale,
            holders: state.holders,
            chart,
            charting: state.charting(scale, at),
            pictured: picture.is_some(),
            landed: state.landed,
            at,
            here: Here(here),
            heading: self.heading(),
            surveyed: self.surveyed.as_deref().cloned().unwrap_or_default(),
            harvesters: Standing(self.harvesters.as_deref().cloned().unwrap_or_default()),
            benches: self.benches(),
            camps: (
                self.jobs.as_deref().cloned().unwrap_or_default(),
                self.known.as_deref().cloned().unwrap_or_default(),
            ),
            quest,
        })
    }
}

impl ChangeSource for MapInputs<'static, 'static> {
    fn changed(inputs: &mut MapInputs<'_, '_>) -> bool {
        // Whole metres: the key rounds the place to a picture pixel (a metre
        // or more), so this asks for the key to be made a little more often
        // than it can change, and the key says whether anything did.
        let at = inputs.players.single().ok().map(|at| {
            #[allow(clippy::cast_possible_truncation, reason = "whole metres")]
            [at.0.x().round() as i64, at.0.z().round() as i64]
        });
        let spot = at.is_some_and(|at| inputs.spot.differs(at));
        let heading = inputs.heading();
        let turned = inputs.turn.differs(heading);
        let benches = inputs.benches();
        let moved_benches = inputs.benches.differs(benches);
        let machines = inputs
            .harvesters
            .as_deref()
            .is_some_and(|list| inputs.standing.differs(planted(list)));
        spot || turned
            || moved_benches
            || machines
            || moved(&inputs.registries)
            || moved(&inputs.surveyed)
            || moved(&inputs.jobs)
            || moved(&inputs.known)
            || moved(&inputs.log)
            || moved(&inputs.places)
            || moved(&inputs.near)
    }
}

impl SimScreen for MapState {
    type View = Shown;
    type Inputs = MapInputs<'static, 'static>;
    type Answering = ();
    const KIND: &'static str = KIND;

    /// The key from the world; the view is made afresh only when it differs
    /// from the one on show (a walk of a metre is no new view).
    fn view(inputs: &MapInputs<'_, '_>, state: &Self) -> Option<Shown> {
        let key = inputs.gather(state)?;
        if let Some(shown) = state.built.as_ref().filter(|shown| shown.key == key) {
            return Some(shown.clone());
        }
        let view = Arc::new(view_of(inputs, state, &key)?);
        Some(Shown { key, view })
    }

    fn def(view: &Shown, state: &Self) -> ScreenDef {
        let shown = state.shown.as_ref().and_then(|id| {
            view.view
                .marks
                .iter()
                .find(|mark| &mark.key.id() == id)
                .map(|mark| mark.words.clone())
        });
        screen_def(&view.view, shown.as_ref())
    }

    fn shown(state: &Self) -> Option<&Shown> {
        state.built.as_ref()
    }

    fn marked(state: &Self) -> bool {
        state.redo
    }

    fn built(state: &mut Self, view: Shown) {
        // A mark that is gone from the new view is not refocused, and a
        // chip chosen already asked for its own focus.
        if state.refocus.is_none() {
            state.refocus = state
                .focus
                .clone()
                .filter(|id| view.view.marks.iter().any(|mark| &mark.key.id() == id));
        }
        state.built = Some(view);
        state.views += 1;
        state.redo = false;
    }

    /// A scale chip picks the picture shown, and the Holders chip the layer.
    fn answer(state: &mut Self, choice: &MenuChoice, (): &mut ()) {
        if choice.id == HOLDERS_ID {
            state.holders = !state.holders;
        } else if let Some(scale) = choice.id.strip_prefix(SCALE_TAG).and_then(Scale::parse) {
            state.scale = scale;
        } else {
            return;
        }
        // The screen is rebuilt on the next frame, whatever the key says (the
        // words are made afresh), and the chip keeps the focus through it.
        state.redo = true;
        state.built = None;
        state.focus = None;
        state.refocus = Some(choice.id.clone());
    }
}

/// Pushes the map, or rebuilds it in place when it is on top. M, the pause
/// menu's button and the flag for a still come through here.
pub fn open(commands: &mut Commands) {
    open_at(commands, None);
}

/// [`open`] at `scale`, when one is named.
pub fn open_at(commands: &mut Commands, scale: Option<Scale>) {
    commands.queue(move |world: &mut World| {
        {
            let mut state = world.resource_mut::<MapState>();
            if let Some(scale) = scale {
                state.scale = scale;
            }
            // Words made for an earlier opening are not carried to this one.
            state.built = None;
        }
        let Some(def) = strata_ui::sim_screen::build::<MapState>(world) else {
            tracing::warn!("no local player: nothing to put on the map");
            return;
        };
        if on_top(world.resource::<ScreenStack>()) {
            replace_screen(&mut world.commands(), Arc::new(def), None);
        } else {
            // The keyboard starts on the tracked deposit, the mark a player
            // opens the map to find.
            let mut state = world.resource_mut::<MapState>();
            state.refocus = state
                .view()
                .and_then(|view| view.marks.iter().find(|mark| mark.tracked))
                .map(|mark| mark.key.id());
            push_screen(&mut world.commands(), Arc::new(def), None);
        }
    });
}

/// Turns the Holders layer on or off before the screen is opened, for a
/// still.
pub fn set_holders(commands: &mut Commands, on: bool) {
    commands.queue(move |world: &mut World| {
        world.resource_mut::<MapState>().holders = on;
    });
}

/// `Update`: M opens the map over nothing, and closes it from the top.
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

/// The picture of an arrow pointing up, white with a dark edge, which the
/// member and the off-map mark are drawn with, turned and tinted.
fn arrow_image() -> Image {
    let side = ARROW_IMAGE;
    let shape = [(32.0_f32, 4.0_f32), (54., 58.), (32., 45.), (10., 58.)];
    let inside = |p: Vec2| {
        let mut crossings = false;
        for i in 0..shape.len() {
            let (a, b) = (
                Vec2::from(shape[i]),
                Vec2::from(shape[(i + 1) % shape.len()]),
            );
            if (a.y > p.y) != (b.y > p.y) && p.x < (b.x - a.x) * (p.y - a.y) / (b.y - a.y) + a.x {
                crossings = !crossings;
            }
        }
        crossings
    };
    let edge = |p: Vec2| {
        (0..shape.len())
            .map(|i| {
                let (a, b) = (
                    Vec2::from(shape[i]),
                    Vec2::from(shape[(i + 1) % shape.len()]),
                );
                let along = (b - a).normalize_or_zero();
                let t = (p - a).dot(along).clamp(0., (b - a).length());
                (p - (a + along * t)).length()
            })
            .fold(f32::MAX, f32::min)
    };
    let mut data = Vec::with_capacity((side * side * 4) as usize);
    for y in 0..side {
        for x in 0..side {
            #[allow(clippy::cast_precision_loss, reason = "a pixel of a small picture")]
            let p = Vec2::new(x as f32 + 0.5, y as f32 + 0.5);
            let pixel = if inside(p) {
                [255, 255, 255, 255]
            } else if edge(p) < 3.2 {
                [10, 12, 18, 255]
            } else {
                [0, 0, 0, 0]
            };
            data.extend_from_slice(&pixel);
        }
    }
    Image::new(
        Extent3d {
            width: side,
            height: side,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        data,
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::default(),
    )
}

/// Writes `bytes` into the image `old` names, or makes one.
fn upsert(
    images: Option<&mut Assets<Image>>,
    old: Option<&Handle<Image>>,
    bytes: Vec<u8>,
) -> Option<Handle<Image>> {
    let images = images?;
    if let Some(handle) = old
        && let Some(mut each) = images.get_mut(handle)
    {
        each.data = Some(bytes);
        return Some(handle.clone());
    }
    Some(images.add(Image::new(
        Extent3d {
            width: PICTURE_PX,
            height: PICTURE_PX,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        bytes,
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::default(),
    )))
}

/// `Update`: asks for the pictures the open map wants, on the compute pool,
/// and takes in the ones that have landed.
#[allow(
    clippy::needless_pass_by_value,
    reason = "system parameters are taken by value"
)]
pub fn pictures(
    stack: Res<ScreenStack>,
    mut state: ResMut<MapState>,
    registries: Option<Res<Registries>>,
    source: Option<Res<ProceduralWorldRes>>,
    factions: Option<Res<Factions>>,
    images: Option<ResMut<Assets<Image>>>,
    players: Query<&WorldPosition, With<LocalPlayer>>,
) {
    // The body polls its tasks through a mutable borrow every frame, which
    // would stamp the state as written every frame and make it an input that
    // always moved; only a landed picture or a new ask is a change.
    let before = (state.arrow.is_some(), state.landed, state.asks);
    fill(
        &stack,
        state.bypass_change_detection(),
        registries,
        source,
        factions,
        images,
        &players,
    );
    if before != (state.arrow.is_some(), state.landed, state.asks) {
        state.set_changed();
    }
}

#[allow(
    clippy::needless_pass_by_value,
    reason = "the system's parameters, handed on"
)]
fn fill(
    stack: &ScreenStack,
    state: &mut MapState,
    registries: Option<Res<Registries>>,
    source: Option<Res<ProceduralWorldRes>>,
    factions: Option<Res<Factions>>,
    images: Option<ResMut<Assets<Image>>>,
    players: &Query<&WorldPosition, With<LocalPlayer>>,
) {
    let mut images = images;
    if state.arrow.is_none()
        && let Some(images) = images.as_mut()
    {
        state.arrow = Some(images.add(arrow_image()));
    }
    for index in 0..3 {
        let Some((chart, task)) = state.tasks[index].as_mut() else {
            continue;
        };
        let chart = *chart;
        let Some(bytes) = block_on(poll_once(task)) else {
            continue;
        };
        state.tasks[index] = None;
        let Made {
            ground,
            sites,
            territory,
        } = bytes;
        let image = upsert(
            images.as_deref_mut(),
            state.pictures[index]
                .as_ref()
                .and_then(|picture| picture.image.as_ref()),
            ground,
        );
        let (territory, holders) = match territory {
            Some(TerritoryPicture { bytes, holders }) => (
                upsert(
                    images.as_deref_mut(),
                    state.pictures[index]
                        .as_ref()
                        .and_then(|picture| picture.territory.as_ref()),
                    bytes,
                ),
                holders,
            ),
            None => (None, Vec::new()),
        };
        state.pictures[index] = Some(Picture {
            chart,
            image,
            sites: Arc::new(sites),
            territory,
            holders: Arc::new(holders),
        });
        state.landed += 1;
    }
    if !on_top(stack) {
        return;
    }
    let Ok(at) = players.single() else {
        return;
    };
    let scale = state.scale;
    let index = scale.index();
    if state.tasks[index].is_some() {
        return;
    }
    let Some(chart) = state.wanted(scale, [at.0.x(), at.0.z()]) else {
        return;
    };
    state.asked[index] = Some(chart);
    state.asks += 1;
    let (Some(registries), Some(source)) = (registries, source) else {
        return;
    };
    let (world, registries) = (Arc::clone(&source.0), Arc::clone(&registries.0));
    let map = factions.map(|factions| Arc::clone(&factions.0));
    let task = AsyncComputeTaskPool::get_or_init(TaskPool::new).spawn(async move {
        let ground = chart::ground(&world, &registries, &chart);
        let mut sites = chart::site_marks(&world, &chart);
        let territory = map.map(|map| {
            chart::name_holders(&map, &mut sites);
            // A camp's holding is a disc centred on its exact point, so the
            // tint would give away every camp the member has not found.
            let mut holdings = chart::holdings_for(&map, &chart);
            holdings.retain(|holding| {
                map.faction(holding.faction).is_none_or(|faction| {
                    faction.nature != strata_registry::faction::Nature::Outlaw
                })
            });
            chart::territory(&map, &holdings, &chart, &ground)
        });
        Made {
            ground,
            sites,
            territory,
        }
    });
    state.tasks[index] = Some((chart, task));
}

/// A mark node, and which of the view's marks it is.
#[derive(Component, Debug, Clone)]
pub struct MapMark {
    /// What it stands for.
    pub key: MarkKey,
    /// How high it lies over the others: settlements over what the
    /// household found over sites and benches.
    base: i32,
}

/// `strata:map_frame`: the picture and every mark on it.
#[derive(Debug, Default, Clone, Copy)]
pub struct FrameWidget;

fn colour_of([r, g, b, a]: [f32; 4]) -> Color {
    Color::srgba(r, g, b, a)
}

/// A mark's button: what makes it a button to slotted's navigation, its
/// tests and its accept action, and to the pointer.
fn make_button(world: &mut World, entity: Entity, mark: &MarkView) {
    let id = mark.key.id();
    let mut button = world.entity_mut(entity);
    button.insert((
        SemanticRole::Button,
        SemanticLabel(mark.words.title.clone()),
        ButtonState {
            variant: ButtonVariant::Secondary,
            disabled: false,
            pressed: false,
        },
        Hovered::default(),
        TabIndex(0),
        Focusable,
        AutoDirectionalNavigation::default(),
        Pickable::default(),
        WidgetNode(WidgetKind::new("strata:map_mark")),
        Tags::new().with(Tags::TEST_ID, &id),
        TestId::new(id),
        MapMark {
            key: mark.key.clone(),
            base: match mark.key {
                MarkKey::Site(_) if mark.symbol == "veyra:icon.map.settlement" => 3,
                MarkKey::Deposit(_) | MarkKey::Away(_) | MarkKey::Lead | MarkKey::Quest => 2,
                _ => 1,
            },
        },
    ));
    button.observe(on_button_press);
    button.observe(on_button_release);
    button.observe(on_button_drag_end);
    button.observe(on_button_cancel);
    button.observe(on_button_click);
}

/// A word on a plate, as the tracker's markers are, over the ground.
fn spawn_plate(ctx: &mut SpawnCtx<'_>, parent: Entity, node: Node, text: &str, id: &str) -> Entity {
    let font = ctx
        .world
        .get_resource::<UiFonts>()
        .map(|fonts| fonts.display.clone());
    let label = ctx
        .world
        .spawn((
            Painted::new("strata.tracker.text"),
            Text::new(text.to_owned()),
            bevy::text::TextFont {
                font: font.unwrap_or_default().into(),
                font_size: bevy::text::FontSize::Px(13.),
                ..Default::default()
            },
            bevy::text::TextLayout::no_wrap(),
            Pickable::IGNORE,
            TestId::new(format!("{id}.text")),
        ))
        .id();
    ctx.world
        .spawn((
            Painted::new("strata.tracker.plate"),
            Node {
                padding: UiRect::axes(Val::Px(6.), Val::Px(2.)),
                border: UiRect::all(Val::Px(1.)),
                ..node
            },
            Pickable::IGNORE,
            ChildOf(parent),
        ))
        .add_child(label)
        .id()
}

impl Widget for FrameWidget {
    #[allow(
        clippy::too_many_lines,
        reason = "the picture, then each kind of node on it"
    )]
    fn spawn(
        &self,
        ctx: &mut SpawnCtx<'_>,
        _params: &slotted::registry::Value,
        _children: &[UiNodeDef],
    ) -> Entity {
        let (view, image, tint, arrow) = {
            let state = ctx.world.resource::<MapState>();
            let picture = state.pictures[state.scale.index()].as_ref();
            (
                state.view().cloned(),
                picture.and_then(|picture| picture.image.clone()),
                picture
                    .filter(|_| state.holders)
                    .and_then(|picture| picture.territory.clone()),
                state.arrow.clone(),
            )
        };
        let root = ctx.spawn_node((
            Node {
                height: Val::Vh(FRAME_VH),
                max_height: Val::Px(FRAME_PX),
                aspect_ratio: Some(1.),
                flex_shrink: 0.,
                border: UiRect::all(Val::Px(1.)),
                overflow: Overflow::clip(),
                ..Node::default()
            },
            BackgroundColor(GROUND),
            BorderColor::all(Color::srgb(0.3, 0.34, 0.4)),
            SemanticRole::Decor,
            WidgetNode(WidgetKind::new(WIDGET)),
        ));
        let Some(view) = view else {
            return root;
        };
        if let Some(image) = image {
            ctx.world.spawn((
                Node {
                    position_type: PositionType::Absolute,
                    left: Val::Px(0.),
                    top: Val::Px(0.),
                    width: Val::Percent(100.),
                    height: Val::Percent(100.),
                    ..Node::default()
                },
                ImageNode {
                    image_mode: NodeImageMode::Stretch,
                    ..ImageNode::new(image)
                },
                Pickable::IGNORE,
                ChildOf(root),
            ));
        }
        if let Some(image) = tint {
            ctx.world.spawn((
                Node {
                    position_type: PositionType::Absolute,
                    left: Val::Px(0.),
                    top: Val::Px(0.),
                    width: Val::Percent(100.),
                    height: Val::Percent(100.),
                    ..Node::default()
                },
                ImageNode {
                    image_mode: NodeImageMode::Stretch,
                    ..ImageNode::new(image)
                },
                Pickable::IGNORE,
                ChildOf(root),
            ));
        }
        let centred = |at: [f32; 2]| Node {
            position_type: PositionType::Absolute,
            left: Val::Percent(at[0] * 100.),
            top: Val::Percent(at[1] * 100.),
            ..Node::default()
        };
        let middle = UiTransform::from_translation(Val2::percent(-50., -50.));
        // Areas first, so every symbol lies over every disc.
        for mark in view.marks.iter().filter(|mark| mark.disc.is_some()) {
            let across = mark.disc.unwrap_or(0.) * 100.;
            let mut fill = colour_of(mark.colour);
            fill.set_alpha(0.32);
            ctx.world.spawn((
                Node {
                    width: Val::Percent(across),
                    height: Val::Percent(across),
                    min_width: Val::Px(MIN_DISC_PX),
                    min_height: Val::Px(MIN_DISC_PX),
                    border: UiRect::all(Val::Px(2.)),
                    border_radius: BorderRadius::MAX,
                    ..centred(mark.at)
                },
                middle,
                BackgroundColor(fill),
                BorderColor::all(colour_of(mark.colour)),
                Pickable::IGNORE,
                ChildOf(root),
            ));
        }
        for mark in &view.marks {
            if mark.tracked && mark.turn.is_none() {
                ctx.world.spawn((
                    Node {
                        width: Val::Px(RING_PX),
                        height: Val::Px(RING_PX),
                        border: UiRect::all(Val::Px(3.)),
                        border_radius: BorderRadius::MAX,
                        ..centred(mark.at)
                    },
                    middle,
                    BorderColor::all(WARM),
                    Pickable::IGNORE,
                    ChildOf(root),
                ));
            }
        }
        for mark in &view.marks {
            let border = if mark.disc.is_some() {
                colour_of(mark.colour)
            } else {
                NEUTRAL
            };
            let button = ctx
                .world
                .spawn((
                    Node {
                        width: Val::Px(MARK_PX),
                        height: Val::Px(MARK_PX),
                        border: UiRect::all(Val::Px(if mark.disc.is_some() { 3. } else { 2. })),
                        border_radius: BorderRadius::MAX,
                        align_items: AlignItems::Center,
                        justify_content: JustifyContent::Center,
                        ..centred(mark.at)
                    },
                    middle,
                    BackgroundColor(PLATE),
                    BorderColor::all(border),
                    ChildOf(root),
                ))
                .id();
            make_button(ctx.world, button, mark);
            match (mark.turn, &arrow) {
                (Some(turn), Some(arrow)) => {
                    ctx.world.spawn((
                        Node {
                            width: Val::Px(MARK_PX - 6.),
                            height: Val::Px(MARK_PX - 6.),
                            ..Node::default()
                        },
                        UiTransform::from_rotation(Rot2::radians(turn)),
                        ImageNode {
                            color: WARM,
                            image_mode: NodeImageMode::Stretch,
                            ..ImageNode::new(arrow.clone())
                        },
                        Pickable::IGNORE,
                        ChildOf(button),
                    ));
                }
                _ if !mark.symbol.is_empty() => {
                    ctx.spawn_children(
                        button,
                        &[super::icons::node(
                            mark.symbol,
                            MARK_PX - 8.,
                            None,
                            false,
                            None,
                        )],
                    );
                }
                _ => {}
            }
            if !mark.label.is_empty() {
                let flip = mark.label_left;
                let node = Node {
                    position_type: PositionType::Absolute,
                    top: Val::Px(MARK_PX / 2. - 11.),
                    left: if flip {
                        Val::Auto
                    } else {
                        Val::Px(MARK_PX + 2.)
                    },
                    right: if flip {
                        Val::Px(MARK_PX + 2.)
                    } else {
                        Val::Auto
                    },
                    ..Node::default()
                };
                spawn_plate(ctx, button, node, &mark.label, &mark.key.id());
            }
        }
        // The member, last so nothing covers them.
        if let Some(arrow) = arrow {
            ctx.world.spawn((
                Node {
                    width: Val::Px(ARROW_PX),
                    height: Val::Px(ARROW_PX),
                    ..centred(view.member)
                },
                UiTransform {
                    translation: Val2::percent(-50., -50.),
                    rotation: Rot2::radians(view.heading),
                    ..UiTransform::IDENTITY
                },
                ImageNode {
                    image_mode: NodeImageMode::Stretch,
                    ..ImageNode::new(arrow)
                },
                Pickable::IGNORE,
                ZIndex(6),
                TestId::new("map.member"),
                ChildOf(root),
            ));
        }
        // North, so the picture says which way it is turned.
        spawn_plate(
            ctx,
            root,
            Node {
                position_type: PositionType::Absolute,
                top: Val::Px(6.),
                left: Val::Px(8.),
                ..Node::default()
            },
            words("strata.map.north"),
            "map.north",
        );
        if view.charting {
            let plate = spawn_plate(
                ctx,
                root,
                Node {
                    position_type: PositionType::Absolute,
                    top: Val::Px(6.),
                    right: Val::Px(8.),
                    ..Node::default()
                },
                words("strata.map.charting"),
                CHARTING_ID,
            );
            ctx.world.entity_mut(plate).insert(TestId::new(CHARTING_ID));
        }
        root
    }
}

/// Observer on `Activate`: a found deposit's mark, from the pointer's click
/// or the accept action, sends the register's track toggle.
fn on_mark_activate(
    activate: On<Activate>,
    marks: Query<&MapMark>,
    inbox: Option<ResMut<SurveyInbox>>,
) {
    let (Ok(mark), Some(mut inbox)) = (marks.get(activate.entity), inbox) else {
        return;
    };
    if let MarkKey::Deposit(id) | MarkKey::Away(id) = mark.key {
        inbox.push(SurveyRequest::ToggleTrack(id));
    }
}

/// `Update`: the panel says the mark pointed at, else the one in focus, and
/// the mark in focus lies lit. The pointer is only read in the pointer's own
/// mode, so a key walk over the marks is not pulled back by a parked mouse.
#[allow(
    clippy::too_many_arguments,
    reason = "the screen, the device in use, the focus, the marks and the lines"
)]
pub fn write_panel(
    stack: Res<ScreenStack>,
    mode: Res<InputMode>,
    focus: Option<Res<InputFocus>>,
    mut state: ResMut<MapState>,
    mut marks: Query<(
        Entity,
        &MapMark,
        &Hovered,
        &mut BackgroundColor,
        &mut ZIndex,
    )>,
    ids: Query<(Entity, &TestId)>,
    mut texts: Query<(&mut LocText, &mut Text, &mut Themed)>,
) {
    if !on_top(&stack) {
        return;
    }
    let focused = focus.as_deref().and_then(InputFocus::get);
    let pointed = (*mode == InputMode::Pointer)
        .then(|| marks.iter().find(|(_, _, hovered, _, _)| hovered.get()))
        .flatten()
        .map(|(entity, mark, ..)| (entity, mark.key.id()));
    let focused = focused.and_then(|entity| {
        marks
            .get(entity)
            .ok()
            .map(|(entity, mark, ..)| (entity, mark.key.id()))
    });
    let lit = pointed.as_ref().or(focused.as_ref());
    if let Some((_, id)) = &focused
        && state.focus.as_ref() != Some(id)
    {
        state.bypass_change_detection().focus = Some(id.clone());
    }
    for (entity, mark, _, mut plate, mut lift) in &mut marks {
        let on = lit.is_some_and(|(lit, _)| *lit == entity);
        let wanted = if on { PLATE_LIT } else { PLATE };
        if plate.0 != wanted {
            plate.0 = wanted;
        }
        // The mark looked at lies over its neighbours and their names.
        let wanted = ZIndex(if on { 8 } else { mark.base });
        if *lift != wanted {
            *lift = wanted;
        }
    }
    let shown = lit.map(|(_, id)| id.clone());
    let said = shown
        .as_ref()
        .and_then(|id| {
            state
                .view()
                .and_then(|view| view.marks.iter().find(|mark| &mark.key.id() == id))
        })
        .map(|mark| mark.words.clone());
    state.bypass_change_detection().shown.clone_from(&shown);
    let none = Words {
        title: words("strata.map.panel.none").to_owned(),
        ..Words::default()
    };
    let said = said.unwrap_or(none);
    for (id, text) in PANEL_LINES.iter().zip(said.all()) {
        let Some((entity, _)) = ids.iter().find(|(_, test)| test.0 == *id) else {
            continue;
        };
        if let Ok((mut loc, mut line, _)) = texts.get_mut(entity) {
            if loc.key.0 != text {
                loc.key = LocKey(text.to_owned());
            }
            if line.0 != text {
                text.clone_into(&mut line.0);
            }
        }
    }
}

/// `Update`: after a rebuild the mark that had the focus has it again.
pub fn restore_focus(
    stack: Res<ScreenStack>,
    mut state: ResMut<MapState>,
    focus: Option<ResMut<InputFocus>>,
    marks: Query<(Entity, &MapMark)>,
    chips: Query<(Entity, &TestId)>,
) {
    if !on_top(&stack) {
        return;
    }
    let (Some(wanted), Some(mut focus)) = (state.refocus.clone(), focus) else {
        return;
    };
    let found = marks
        .iter()
        .find(|(_, mark)| mark.key.id() == wanted)
        .map(|(entity, _)| entity)
        .or_else(|| {
            chips
                .iter()
                .find(|(_, id)| id.0 == wanted)
                .map(|(entity, _)| entity)
        });
    if let Some(entity) = found {
        focus.set(entity, FocusCause::Navigated);
        state.bypass_change_detection().refocus = None;
    }
}

/// `Update`: the legend's swatches take the colours the picture is drawn in.
pub fn paint_swatches(
    stack: Res<ScreenStack>,
    state: Res<MapState>,
    mut commands: Commands,
    mut swatches: Query<(Entity, &TestId, Option<&mut BackgroundColor>)>,
) {
    if !on_top(&stack) {
        return;
    }
    for (entity, id, colour) in &mut swatches {
        let Some(name) = id.0.strip_prefix("map.swatch:") else {
            continue;
        };
        let Some(wanted) = name.strip_prefix("holder:").map_or_else(
            || swatch_colour(name),
            |at| {
                let each = state.view()?.legend.get(at.parse::<usize>().ok()?)?;
                Some(colour_of(each.colour))
            },
        ) else {
            continue;
        };
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

/// `Startup`: the frame's widget, under its kind.
fn register_widget(mut widgets: ResMut<WidgetRegistry>) {
    widgets.register(WidgetKind::new(WIDGET), FrameWidget);
}

/// Registers the state, the key, the widget and the systems. Only called
/// once the UI is wired.
pub(crate) fn register(app: &mut bevy::app::App) {
    app.add_sim_screen::<MapState>()
        .add_observer(on_mark_activate)
        .add_systems(bevy::app::Startup, register_widget)
        .add_systems(
            bevy::app::Update,
            (
                toggle.before(ScreenStep::answer::<MapState>()),
                pictures
                    .after(ScreenStep::answer::<MapState>())
                    .before(ScreenStep::rebuild::<MapState>()),
                (
                    restore_focus.run_if(super::screen_on_top(kind)),
                    write_panel.run_if(super::screen_on_top(kind)),
                )
                    .chain()
                    .after(ScreenStep::rebuild::<MapState>()),
            )
                .in_set(super::ScreenWork),
        )
        .add_systems(
            bevy::app::Update,
            // After the theme has painted the role's white, or its repaint wins.
            paint_swatches
                .after(slotted::theme::SlottedThemeSet::Apply)
                .run_if(super::screen_on_top(kind)),
        );
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, reason = "a test that unwraps a None fails")]

    use super::*;

    fn site(held_by: Option<&str>) -> SiteMark {
        SiteMark {
            name: "Wack's Hold".to_owned(),
            kind_name: "Hamlet".to_owned(),
            kind: "strata:hamlet".to_owned(),
            at: [0.0, 0.0],
            settlement: false,
            id: None,
            held_by: held_by.map(str::to_owned),
        }
    }

    #[test]
    fn a_camp_shows_only_when_found_and_a_taken_one_as_an_area() {
        use strata_worldgen::sites::SiteId;
        use veyra_sim::postings::{Heard, Job, Posting};

        let id = |node: i32| SiteId {
            kind: strata_registry::site::SiteKindId(0),
            mass: [0, 0],
            node: [node, 0],
        };
        let camp = |node: i32| SiteMark {
            name: format!("Camp {node}"),
            kind_name: "Outlaw camp".to_owned(),
            kind: "strata:wild_camp".to_owned(),
            at: [0.0, 0.0],
            settlement: false,
            id: Some(id(node)),
            held_by: None,
        };
        let sites = vec![site(None), camp(1), camp(2), camp(3)];
        let kinds = vec!["strata:wild_camp".to_owned()];
        let job = |key: u64| Job {
            posting: Posting {
                key,
                name: "Camp 2".to_owned(),
                tier: 1,
                pay: 400,
                faction: String::new(),
                kind: "strata:wild_camp".to_owned(),
                heard: Heard {
                    centre: [100.0, 0.0],
                    radius: 900.0,
                },
            },
            buyer: "veyra:buyer.pelagic_works".parse().unwrap(),
            taken_at: 0.0,
            done: false,
        };
        let mut jobs = Jobs::default();
        jobs.taken.insert(id(2).key(), job(id(2).key()));
        let mut known = Known::default();
        known.camps.insert(id(3).key());
        let (shown, heard) = camp_marks(&sites, &kinds, &known, &jobs);
        let names: Vec<&str> = shown.iter().map(|site| site.name.as_str()).collect();
        assert_eq!(
            names,
            ["Wack's Hold", "Camp 3"],
            "camp 1 is neither found nor heard of"
        );
        assert_eq!(heard.len(), 1);
        assert_eq!(heard[0].posting.key, id(2).key());
        // Found too, the point replaces the area.
        known.camps.insert(id(2).key());
        let (shown, heard) = camp_marks(&sites, &kinds, &known, &jobs);
        assert_eq!(shown.len(), 3);
        assert!(heard.is_empty());
    }

    #[test]
    fn the_objective_is_marked_when_it_lies_in_the_picture() {
        use crate::journal::QuestTarget;
        let chart = Chart::square([0.0, 0.0], REGION_SPAN_M, PICTURE_PX);
        let target = |at: [f64; 2]| QuestTarget {
            at,
            name: "Station mast".to_owned(),
        };
        let mark = quest_mark(&chart, [0.0, 0.0], &target([500.0, 500.0])).unwrap();
        assert_eq!(mark.key, MarkKey::Quest);
        assert_eq!(mark.words.lines[1], "Station mast");
        assert!(quest_mark(&chart, [0.0, 0.0], &target([1.0e7, 0.0])).is_none());
    }

    fn view(holders: bool, legend: &[&str]) -> MapView {
        MapView {
            scale: Scale::Region,
            chart: Chart::square([0.0, 0.0], REGION_SPAN_M, PICTURE_PX),
            charting: false,
            pictured: true,
            marks: Vec::new(),
            holders,
            legend: legend
                .iter()
                .map(|name| HolderView {
                    name: (*name).to_owned(),
                    colour: [0.2, 0.8, 0.7, 1.0],
                })
                .collect(),
            member: [0.5, 0.5],
            heading: 0.0,
        }
    }

    /// Whether a node with the test id is anywhere under `node`.
    fn has_id(node: &UiNodeDef, id: &str) -> bool {
        match node {
            UiNodeDef::Panel { children, tags, .. } | UiNodeDef::Card { children, tags, .. } => {
                tags.get(Tags::TEST_ID) == Some(id) || children.iter().any(|c| has_id(c, id))
            }
            UiNodeDef::Text { tags, .. } => tags.get(Tags::TEST_ID) == Some(id),
            _ => false,
        }
    }

    #[test]
    fn a_sites_panel_names_its_holder() {
        let said = site_words(
            &site(Some("Cethand Cooperative")),
            "2 km north".to_owned(),
            false,
        );
        assert_eq!(said.title, "Wack's Hold");
        assert_eq!(said.lines[0], "Hamlet");
        assert_eq!(said.lines[1], "Held by Cethand Cooperative");
        assert_eq!(said.lines[3], "2 km north");
        assert!(site_words(&site(None), String::new(), false).lines[1].is_empty());
    }

    #[test]
    fn the_legend_lists_a_line_per_faction_only_while_the_layer_is_on() {
        let names = ["Pelagic Works", "Nothath Trust"];
        let on = screen_def(&view(true, &names), None).root;
        assert!(has_id(&on, HOLDERS_ID));
        assert!(has_id(&on, &format!("{HOLDER_ROW_TAG}0")));
        assert!(has_id(&on, &format!("{HOLDER_ROW_TAG}1")));
        assert!(!has_id(&on, &format!("{HOLDER_ROW_TAG}2")));
        let off = screen_def(&view(false, &names), None).root;
        assert!(has_id(&off, HOLDERS_ID));
        assert!(!has_id(&off, &format!("{HOLDER_ROW_TAG}0")));
    }
}
