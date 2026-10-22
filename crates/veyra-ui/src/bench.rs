//! The bench: the household's characters as cards, who is out, who can
//! join, and the switch (progression design, section 4).
//!
//! The household is the game's, not the client's: the game fills
//! [`BenchView`] whenever the household changes and answers a
//! [`BenchRequest`] with a swap or a join, reporting back as a
//! [`BenchOutcome`] the client toasts. The client decides nothing about
//! the household; what it decides is when a button is live (a terminal
//! within reach, nobody fighting) and what a switch looks like: the
//! screen closes behind a short fade to black, the request goes out while
//! the screen is dark, and the world comes back with the other character
//! in it and their name on the HUD.
//!
//! From the pause menu the bench is read-only: it shows the household and
//! says that switching happens at a terminal. Nothing here is replicated.
//!
//! The terminal's second tab is the survey register (ticket 0007, step
//! 3): what the survey office publishes of the ground round the player,
//! every deposit within the dials' range, nearest first, each with its
//! class and type, the line it is best in, how far it is and
//! which way, and a mark on the ones the household has stood at. The list
//! is the sim's (`veyra_sim::deposits::register`), worded here
//! ([`RegisterView`]) and never worked out again. The row under the focus
//! or the pointer shows the card of a lot drawn there beside the list,
//! repainted in place. Accept on a row asks the sim to track it, or to let
//! the tracked one go; the HUD's marker ([`super::tracker`]) follows what
//! the sim then holds: which of the two a press is, is the sim's to say.
//! The tracked deposit keeps a row however far it is. The register reads
//! the same from the pause menu: tracking needs no terminal.

use std::sync::Arc;
use std::time::Duration;

use bevy::color::Color;
use bevy::ecs::prelude::*;
use bevy::input_focus::InputFocus;
use bevy::math::Vec2;
use bevy::picking::hover::Hovered;
use bevy::picking::pointer::{PointerAction, PointerInput};
use bevy::time::Time;
use bevy::ui::widget::Text;
use bevy::ui::{BackgroundColor, GlobalZIndex, Node, PositionType, Val};
use bevy::window::CursorMoved;
use slotted::menu::{MenuChoice, ToastLevel, ToastSpec, toast};
use slotted::theme::Role;
use slotted::ui::def::{
    Align, ButtonOpts, ButtonVariant, Justify, Layout, LayoutDirection, Length, LocKey, Padding,
    Presentation, PresentationMode, ScreenDef, ScreenKind, Tags, TextOpts, TextRole, Transition,
    UiNodeDef, WidgetKind,
};
use slotted::ui::{
    InputMode, LocText, ScreenStack, TestId, pop_screen, push_screen, replace_screen, zbands,
};
use strata_registry::progression::SkillGraph;
use strata_registry::{GameRegistries, Namespaced, Reading};
use veyra_sim::combat::{Action, CombatEncounter, Combatant, Weapon};
use veyra_sim::combat_tables;
use veyra_sim::deposits::{
    self, Compass, DepositId, NearDeposits, RegisterRow, Survey, SurveyInbox, SurveyRequest,
    Surveyed,
};
use veyra_sim::harvest::{HarvestInbox, HarvestRequest, Harvesters, Holding};
use veyra_sim::progression::{CharacterSheet, Interaction, InteractionKind, Reach, apply};
use veyra_sim::{LocalPlayer, Observers, Registries, WorldPosition};

use super::card::{self, CardParams, ItemCard};
use super::sheet::TrainerSpot;
use super::weapon_diagram::FOCUS_MARK;

/// The screen kind.
pub const KIND: &str = "strata:bench";

/// The `menu` tag prefix of a Switch button: `bench.switch:2`.
pub const SWITCH_TAG: &str = "bench.switch:";
/// Prefix of the line that says a member is at the fabricator; their
/// index on the bench follows.
pub const WORK_TAG: &str = "bench.work:";
/// Prefix of the line that says a member is injured; their index on the
/// bench follows.
pub const INJURED_TAG: &str = "bench.injured:";
/// Prefix of the line under a grey Switch that says why; the index of the
/// character on the bench follows.
pub const WHY_TAG: &str = "bench.why:";
/// The `menu` tag prefix of a Join button: `bench.join:0`.
const JOIN_TAG: &str = "bench.join:";
/// The `test_id` of the panel the whole screen is.
pub const ROOT_ID: &str = "bench.root";
/// The `test_id` of the Close button.
pub const CLOSE_ID: &str = "bench.close";
/// The `test_id` of the note under the title.
pub const NOTE_ID: &str = "bench.note";
/// The `test_id` of the title: the bench's, or the register's.
pub const TITLE_ID: &str = "bench.title";
/// The `menu` tag and `test_id` prefix of a tab: `bench.tab:register`.
pub const TAB_TAG: &str = "bench.tab:";
/// The `menu` tag and `test_id` prefix of a class chip of the register:
/// `register.class:all`, `register.class:strata:reactive`.
pub const CLASS_TAG: &str = "register.class:";
/// The word in a class chip's tag for every class.
pub const CLASS_ALL: &str = "all";
/// The `menu` tag and `test_id` prefix of a row of the register:
/// `register.row:3,-2`, the deposit's cell.
pub const ROW_TAG: &str = "register.row:";
/// The `test_id` prefix of a row's name: `register.name:3,-2`.
pub const NAME_TAG: &str = "register.name:";
/// The `test_id` prefix of a row's mark: `register.mark:3,-2`.
pub const MARK_TAG: &str = "register.mark:";
/// Prefix of a register row's harvester holding text, then the deposit's tag.
pub const HOLDING_TAG: &str = "register.holding:";
/// The prefix of a row's claim line, then the deposit.
pub const CLAIM_TAG: &str = "register.claim:";
/// The `test_id` of the card beside the register's list.
pub const REGISTER_CARD_ID: &str = "register.card";
/// The `test_id` of the register's list.
pub const LIST_ID: &str = "register.list";
/// The `test_id` of the line that says what the register lists.
pub const CAPTION_ID: &str = "register.caption";
/// The `test_id` of the count at the right of the register's head.
pub const COUNT_ID: &str = "register.count";

/// The register's card column, in UI units.
const REGISTER_CARD_WIDTH: f32 = 320.;
/// The widths of a row's columns: its best line, where it lies and the
/// word for the tracked one.
const BEST_WIDTH: f32 = 50.;
const WHERE_WIDTH: f32 = 128.;
const TRACKED_WIDTH: f32 = 58.;

const TAB_ROLE: &str = "strata.fitting.tab";
const TAB_ON_ROLE: &str = "strata.fitting.tab.on";
const TAB_TEXT_ROLE: &str = "strata.fitting.tab.text";
const TAB_TEXT_ON_ROLE: &str = "strata.fitting.tab.text.on";
const ROW_ROLE: &str = "strata.register.row";
const ROW_ON_ROLE: &str = "strata.register.row.on";
const ROW_NAME_ROLE: &str = "strata.register.name";
const ROW_KIND_ROLE: &str = "strata.register.kind";
const ROW_FIGURE_ROLE: &str = "strata.register.figure";
const ROW_WHERE_ROLE: &str = "strata.register.where";
const ROW_TRACKED_ROLE: &str = "strata.register.tracked";
const MARK_ROLE: &str = "strata.register.mark";
const MARK_NONE_ROLE: &str = "strata.register.mark.none";
const MARK_READ_ROLE: &str = "strata.register.mark.read";
const WORK_ROLE: &str = "strata.bench.work";

/// Seconds the fade to black takes.
const FADE_IN: f32 = 0.35;
/// Seconds the screen stays dark while the swap happens under it.
const FADE_HOLD: f32 = 0.2;
/// Seconds the world takes to come back.
const FADE_OUT: f32 = 0.55;

/// The screen kind.
#[must_use]
pub fn kind() -> ScreenKind {
    ScreenKind::new(KIND)
}

pub use veyra_sim::view::{BenchCard, BenchOutcome, BenchRequest, BenchView, BenchWork};

/// Which of the terminal's tabs is shown.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub enum BenchTab {
    /// The household's characters.
    #[default]
    Bench,
    /// The survey register.
    Register,
}

impl BenchTab {
    /// Both tabs, in the head's order.
    pub const ALL: [Self; 2] = [Self::Bench, Self::Register];

    /// The tab's name in its tag.
    #[must_use]
    pub const fn id(self) -> &'static str {
        match self {
            Self::Bench => "bench",
            Self::Register => "register",
        }
    }

    const fn label(self) -> &'static str {
        match self {
            Self::Bench => "strata.bench.tab.bench",
            Self::Register => "strata.bench.tab.register",
        }
    }

    /// The `test_id` and `menu` tag of the tab.
    #[must_use]
    pub fn tab_id(self) -> String {
        format!("{TAB_TAG}{}", self.id())
    }
}

/// How the screen was opened (at a terminal, where switching is allowed,
/// or from the pause, where it is read-only), which tab it shows and, for
/// the register, what it lists.
#[derive(Resource, Debug, Default, Clone, PartialEq)]
pub struct BenchState {
    /// Opened by a terminal's F.
    pub at_terminal: bool,
    /// The tab shown.
    pub tab: BenchTab,
    /// The class the register lists; none for every class.
    pub class: Option<Namespaced>,
    /// A rebuild is due.
    dirty: bool,
    /// The register the open screen was built from.
    register: Option<RegisterView>,
    /// The row whose card stands beside the list.
    shown: Option<DepositId>,
    /// The control last used, which keeps the keyboard across the rebuild
    /// it caused.
    last: Option<String>,
    /// Whether the pointer has moved since the screen opened: a row that
    /// comes up under a parked pointer was not pointed at.
    pointer_moved: bool,
}

impl BenchState {
    /// The register the open screen was built from.
    #[must_use]
    pub const fn register(&self) -> Option<&RegisterView> {
        self.register.as_ref()
    }

    /// The deposit whose card stands beside the register's list.
    #[must_use]
    pub const fn shown(&self) -> Option<DepositId> {
        self.shown
    }
}

/// The switch under way: who is coming out and how far the fade is.
#[derive(Resource, Debug, Default, Clone, Copy, PartialEq)]
pub struct SwitchFade {
    /// The bench index, while a switch runs.
    pub target: Option<usize>,
    /// Seconds since it started.
    pub elapsed: f32,
    /// The request has gone out.
    pub sent: bool,
}

pub use veyra_sim::view::{FallFade, FallPhase};

/// The full-window black node the fade drives.
#[derive(Component, Debug, Clone, Copy, Default)]
pub struct FadeCurtain;

fn key(s: impl Into<String>) -> LocKey {
    LocKey(s.into())
}

fn text(s: impl Into<String>, style: TextRole, role: Option<&str>) -> UiNodeDef {
    UiNodeDef::Text {
        key: key(s),
        style,
        opts: TextOpts {
            role: role.map(Role::new),
            ..TextOpts::default()
        },
        tags: Tags::new(),
    }
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

/// Empty space that takes what is left: the whole of the parent is how
/// the spacer widget spells "grow".
fn spacer() -> UiNodeDef {
    UiNodeDef::Spacer {
        size: Length::Percent(100.),
        tags: Tags::new(),
    }
}

/// The strip across a card's top, in the character's tint. The theme has
/// no per-character colours, so the four rarity strips stand in for the
/// four palette entries: plain, red, blue, amber.
pub(crate) const fn tint_role(tint: u8) -> &'static str {
    match tint % 4 {
        0 => "slot.phantom",
        1 => "browser.rarity.legendary",
        2 => "browser.rarity.uncommon",
        _ => "browser.rarity.epic",
    }
}

/// What a character can do, in two lines: the trees they have boxes in
/// (`Blade · 4 boxes`, a count and not a level, which is what `Blade 4`
/// beside the sheet's `Cuts 4` would read as), and the moves and weapons
/// those boxes open.
#[must_use]
pub fn summary(graph: &SkillGraph, sheet: &CharacterSheet) -> (String, String) {
    let trees: Vec<String> = graph
        .trees
        .iter()
        .filter_map(|t| {
            let n = t.boxes().filter(|b| sheet.has(&b.id)).count();
            (n > 0).then(|| format!("{} · {n} {}", t.name, if n == 1 { "box" } else { "boxes" }))
        })
        .collect();
    let tables = combat_tables::current();
    let effective = apply(&tables, graph, sheet);
    let mut can: Vec<&str> = Vec::new();
    for (action, words) in [
        (Action::Light, "the light cut"),
        (Action::Followup, "the follow-up"),
        (Action::Heavy, "the heavy"),
    ] {
        if graph.gated_actions().contains(action.name()) && effective.unlocked.contains(&action) {
            can.push(words);
        }
    }
    for weapon in [
        Weapon::Pistol,
        Weapon::Carbine,
        Weapon::Rifle,
        Weapon::Blade,
        Weapon::Greatblade,
        Weapon::Glaive,
    ] {
        if graph.gated_weapons().contains(weapon.name()) && effective.certified.contains(&weapon) {
            can.push(family_word("the", weapon));
        }
    }
    let trees = if trees.is_empty() {
        "No boxes learned".to_owned()
    } else {
        trees.join("  ·  ")
    };
    let can = if can.is_empty() {
        "Nothing the fight uses yet".to_owned()
    } else {
        let mut line = can.join(", ");
        if let Some(first) = line.get_mut(0..1) {
            first.make_ascii_uppercase();
        }
        line
    };
    (trees, can)
}

/// `Carbine in hand`, or `Carbine in hand (not certified)` for a
/// character whose boxes do not certify what they carry: the trigger
/// does nothing, and the card is where that is read before a switch.
#[must_use]
pub fn weapon_words(graph: &SkillGraph, sheet: &CharacterSheet, weapon: Weapon) -> String {
    let in_hand = family_word("in_hand", weapon);
    let certified = apply(&combat_tables::current(), graph, sheet)
        .certified
        .contains(&weapon);
    if certified {
        in_hand.to_owned()
    } else {
        format!(
            "{in_hand} {}",
            super::strings::english("strata.bench.not_certified").unwrap_or_default()
        )
    }
}

/// `AT THE FABRICATOR · 7 OF 20`, on the card of the character at `at` of
/// the bench.
fn work_line(work: BenchWork, at: usize) -> UiNodeDef {
    with_id(
        line(
            super::strings::english("strata.bench.work")
                .unwrap_or_default()
                .replace("{made}", &work.made.to_string())
                .replace("{size}", &work.size.to_string()),
            WORK_ROLE,
        ),
        &format!("{WORK_TAG}{at}"),
    )
}

/// `Injured: back in 1 h 40 min`, on the card of the character at `at`.
fn injured_line(minutes: u32, at: usize) -> UiNodeDef {
    with_id(
        line(super::loot::injured_words(minutes), WORK_ROLE),
        &format!("{INJURED_TAG}{at}"),
    )
}

/// Why the character at `at` of the bench is not switched to, over their
/// grey Switch.
fn work_why(at: usize) -> UiNodeDef {
    with_id(
        text(
            "strata.bench.work.why",
            TextRole::Caption,
            Some("strata.fitting.why"),
        ),
        &format!("{WHY_TAG}{at}"),
    )
}

/// Why the injured character at `at` is not switched to, over their grey
/// Switch.
fn injured_why(at: usize) -> UiNodeDef {
    with_id(
        text(
            "strata.bench.injured.why",
            TextRole::Caption,
            Some("strata.fitting.why"),
        ),
        &format!("{WHY_TAG}{at}"),
    )
}

/// One card. `action` is the button under it, `None` for the character
/// who is out. `at` is the character's index on the bench, for the ids of
/// the lines that say they are working; none for a recruit, who cannot be.
fn card(
    graph: &SkillGraph,
    character: &BenchCard,
    out: bool,
    at: Option<usize>,
    action: Option<(&str, String, bool)>,
) -> UiNodeDef {
    let (trees, can) = summary(graph, &character.sheet);
    let mut name_row = vec![text(character.name.clone(), TextRole::Heading, None)];
    if out {
        name_row.push(spacer());
        name_row.push(text(
            "strata.bench.out",
            TextRole::Caption,
            Some("dialogue.speaker"),
        ));
    }
    let mut children = vec![
        panel(
            tint_role(character.tint),
            Layout {
                height: Some(Length::Px(4.)),
                width: Some(Length::Percent(100.)),
                ..column(0.)
            },
            Vec::new(),
        ),
        panel("invisible", row(1.), name_row),
        // The spent points as the card's one numeral, large enough in the
        // condensed face that its 6 and 8 stay apart, with the budget and
        // the word quieter beside it.
        panel(
            "invisible",
            Layout {
                align: Some(Align::End),
                ..row(0.6)
            },
            vec![
                text(
                    character.sheet.points_spent(graph).to_string(),
                    TextRole::Body,
                    Some("strata.bench.points"),
                ),
                text(
                    format!("/ {}", graph.budget),
                    TextRole::Caption,
                    Some("strata.bench.points.total"),
                ),
                text(
                    "strata.bench.points",
                    TextRole::Caption,
                    Some("text.caption"),
                ),
            ],
        ),
        text(
            weapon_words(graph, &character.sheet, character.weapon),
            TextRole::Caption,
            None,
        ),
        panel(
            "invisible",
            Layout {
                height: Some(Length::Px(1.)),
                ..column(0.)
            },
            Vec::new(),
        ),
        text(trees, TextRole::Body, Some("text.label")),
        text(can, TextRole::Body, None),
    ];
    let work = character.work.zip(at);
    // Where a working member is and how far the batch has come: said in
    // words on their card, and again over the button it greys.
    if let Some((work, at)) = work {
        children.insert(2, work_line(work, at));
    }
    let hurt = character.injured_min.zip(at);
    if let Some((minutes, at)) = hurt {
        children.insert(2, injured_line(minutes, at));
    }
    if let Some((label, tag, enabled)) = action {
        children.push(spacer());
        children.extend(work.map(|(_, at)| work_why(at)));
        if work.is_none() {
            children.extend(hurt.map(|(_, at)| injured_why(at)));
        }
        children.push(UiNodeDef::Button {
            widget: None,
            opts: ButtonOpts {
                label: Some(key(label)),
                variant: if enabled {
                    ButtonVariant::Primary
                } else {
                    ButtonVariant::Secondary
                },
                disabled: !enabled,
                ..ButtonOpts::default()
            },
            tags: Tags::new().with("menu", &tag).with(Tags::TEST_ID, &tag),
        });
    }
    panel(
        if out { "tab.side.open" } else { "browser.card" },
        Layout {
            width: Some(Length::Px(250.)),
            padding: Padding::all(2.),
            align: Some(Align::Stretch),
            ..column(0.8)
        },
        children,
    )
}

/// The read-only note: where switching happens, and how far the nearest
/// terminal is when the world has one, since a bench read from the pause
/// is most often read by someone looking for it.
#[must_use]
pub fn read_only_words(terminal: Option<&TrainerSpot>) -> String {
    let english = |key: &'static str| super::strings::english(key).unwrap_or(key);
    let words = english("strata.bench.read_only").to_owned();
    match terminal {
        Some(spot) => format!(
            "{words} {} {:.0} m to the {}.",
            capitalised(english("strata.bench.nearest_terminal")),
            spot.distance,
            spot.bearing
        ),
        None => words,
    }
}

fn capitalised(s: &str) -> String {
    let mut chars = s.chars();
    chars.next().map_or_else(String::new, |first| {
        first.to_uppercase().collect::<String>() + chars.as_str()
    })
}

/// The screen for `view`: `may_switch` is a terminal within reach and
/// nobody fighting, joining the terminal and room on the bench;
/// `terminal` is the nearest one for the read-only note.
#[must_use]
#[allow(
    clippy::too_many_lines,
    reason = "one definition, read top to bottom as the screen is"
)]
pub fn screen_def(
    graph: &SkillGraph,
    view: &BenchView,
    may_switch: bool,
    at_terminal: bool,
    terminal: Option<&TrainerSpot>,
) -> ScreenDef {
    let room = view.characters.len() < view.capacity;
    let characters: Vec<UiNodeDef> = view
        .characters
        .iter()
        .enumerate()
        .map(|(i, c)| {
            let out = i == view.active;
            card(
                graph,
                c,
                out,
                Some(i),
                Some(if out {
                    ("strata.bench.playing", "bench.playing".to_owned(), false)
                } else {
                    // Somebody working the fabricator is not switched to:
                    // the button is grey and the line over it says why.
                    (
                        "strata.bench.switch",
                        format!("{SWITCH_TAG}{i}"),
                        may_switch && c.work.is_none() && c.injured_min.is_none(),
                    )
                }),
            )
        })
        .collect();
    let recruits: Vec<UiNodeDef> = view
        .recruits
        .iter()
        .enumerate()
        .map(|(i, c)| {
            card(
                graph,
                c,
                false,
                None,
                Some((
                    "strata.bench.join",
                    format!("{JOIN_TAG}{i}"),
                    at_terminal && room,
                )),
            )
        })
        .collect();
    let note = if !at_terminal {
        read_only_words(terminal)
    } else if !may_switch {
        "strata.bench.fighting".to_owned()
    } else {
        "strata.bench.here".to_owned()
    };
    let head = head_row(
        "strata.bench.title",
        BenchTab::Bench,
        text(
            format!(
                "{} of {} on the bench",
                view.characters.len(),
                view.capacity
            ),
            TextRole::Caption,
            Some("browser.status"),
        ),
    );
    let mut body = vec![
        UiNodeDef::Text {
            key: key(note),
            style: TextRole::Body,
            opts: TextOpts {
                role: Some(Role::new("text.label")),
                ..TextOpts::default()
            },
            tags: Tags::new().with(Tags::TEST_ID, NOTE_ID),
        },
        panel(
            "invisible",
            Layout {
                align: Some(Align::Stretch),
                ..row(2.)
            },
            characters,
        ),
        UiNodeDef::Separator {
            direction: LayoutDirection::Row,
            tags: Tags::new(),
        },
        text(
            "strata.bench.recruits",
            TextRole::Caption,
            Some("browser.status"),
        ),
    ];
    if recruits.is_empty() {
        body.push(text("strata.bench.nobody", TextRole::Caption, None));
    } else {
        body.push(panel(
            "invisible",
            Layout {
                align: Some(Align::Stretch),
                ..row(2.)
            },
            recruits,
        ));
    }
    // The cards scroll inside the panel on a short window, so the footer
    // and its Close stay inside the border at 720 tall.
    let scrolling = UiNodeDef::Scroll {
        layout: Layout {
            align: Some(Align::Stretch),
            grow: 1.,
            min_height: Some(Length::Px(0.)),
            ..column(2.)
        },
        scrollbar: true,
        children: body,
        tags: Tags::new(),
    };
    frame(vec![head, scrolling, footer(None)], 1140., None)
}

/// A tab of the terminal's screen: a pill that is the button, lit and
/// marked for the one shown.
fn tab(of: BenchTab, on: bool) -> UiNodeDef {
    let id = of.tab_id();
    let label = super::strings::english(of.label()).unwrap_or_default();
    UiNodeDef::Card {
        role: Role::new(if on { TAB_ON_ROLE } else { TAB_ROLE }),
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
        children: vec![line(
            if on {
                format!("{FOCUS_MARK}{label}")
            } else {
                label.to_owned()
            },
            if on { TAB_TEXT_ON_ROLE } else { TAB_TEXT_ROLE },
        )],
        disabled: false,
        tags: Tags::new().with("menu", &id).with(Tags::TEST_ID, &id),
    }
}

/// A line that stays on one line, in `role`.
fn line(s: impl Into<String>, role: &str) -> UiNodeDef {
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

/// The head of either tab: its title, the two tabs with `shown` lit, and
/// at the right what the tab counts.
fn head_row(title: &str, shown: BenchTab, count: UiNodeDef) -> UiNodeDef {
    panel(
        "invisible",
        row(2.),
        vec![
            with_id(text(title, TextRole::Title, None), TITLE_ID),
            panel(
                "invisible",
                row(1.),
                BenchTab::ALL
                    .into_iter()
                    .map(|of| tab(of, of == shown))
                    .collect(),
            ),
            spacer(),
            count,
        ],
    )
}

/// The foot of either tab: what the keys do when the tab has something to
/// say of them, the keys themselves and Close.
fn footer(keys: Option<&str>) -> UiNodeDef {
    let mut children: Vec<UiNodeDef> = keys
        .map(|keys| line(keys, "strata.stores.status"))
        .into_iter()
        .collect();
    children.extend([
        UiNodeDef::Custom {
            kind: WidgetKind::new("slotted:hint_bar"),
            params: slotted::registry::Value::Unit,
            children: Vec::new(),
            tags: Tags::new().with(Tags::TEST_ID, "bench.hints"),
        },
        spacer(),
        UiNodeDef::Button {
            widget: Some(WidgetKind::new("slotted:close")),
            opts: ButtonOpts {
                label: Some(key("strata.menu.close")),
                ..ButtonOpts::default()
            },
            tags: Tags::new().with(Tags::TEST_ID, CLOSE_ID),
        },
    ]);
    panel("invisible", row(2.), children)
}

/// The screen round a tab's rows: the glass panel, as wide as the tab
/// needs and no wider than the window.
fn frame(children: Vec<UiNodeDef>, max_width: f32, focus: Option<String>) -> ScreenDef {
    ScreenDef {
        kind: kind(),
        inherits: None,
        root: UiNodeDef::Panel {
            role: Role::new("panel"),
            layout: Layout {
                width: Some(Length::Percent(92.)),
                max_width: Some(Length::Px(max_width)),
                min_height: Some(Length::Percent(92.)),
                max_height: Some(Length::Percent(92.)),
                padding: Padding::all(3.),
                align: Some(Align::Stretch),
                justify: Justify::Start,
                ..column(2.)
            },
            children,
            tags: Tags::new().with(Tags::TEST_ID, ROOT_ID),
        },
        listring: Vec::new(),
        remove: Vec::new(),
        initial_focus: focus,
        presentation: Presentation {
            mode: PresentationMode::Page,
            scrim: Some(true),
            transition: Transition::Fade,
            wildcards: Some(false),
            ..Presentation::default()
        },
    }
}

// ---- the survey register ----

/// A deposit's id as a row's tag carries it: `3,-2`.
#[must_use]
pub fn deposit_tag(id: DepositId) -> String {
    format!("{},{}", id.0[0], id.0[1])
}

/// The deposit a row's tag names.
fn tagged(tag: &str) -> Option<DepositId> {
    let (x, z) = tag.split_once(',')?;
    Some(DepositId([x.parse().ok()?, z.parse().ok()?]))
}

/// The `test_id` and `menu` tag of the row of `id`.
#[must_use]
pub fn row_id(id: DepositId) -> String {
    format!("{ROW_TAG}{}", deposit_tag(id))
}

/// The `test_id` and `menu` tag of the chip of `class`, or of every class.
#[must_use]
pub fn class_id(class: Option<&Namespaced>) -> String {
    match class {
        Some(class) => format!("{CLASS_TAG}{class}"),
        None => format!("{CLASS_TAG}{CLASS_ALL}"),
    }
}

/// A bearing in a word: `north-east`.
#[must_use]
pub fn compass_words(bearing: Compass) -> &'static str {
    super::strings::english(&format!("strata.compass.{}", bearing.key())).unwrap_or(bearing.key())
}

/// A distance as a row and the HUD's marker write it, and whether it is in
/// kilometres: `850` metres to the nearest ten, a short one to the metre,
/// `1.2` kilometres from a kilometre on.
#[must_use]
pub fn distance_words(metres: f64) -> (String, bool) {
    if metres >= 995. {
        (format!("{:.1}", metres / 1000.), true)
    } else if metres >= 100. {
        (format!("{:.0}", (metres / 10.).round() * 10.), false)
    } else {
        (format!("{metres:.0}"), false)
    }
}

/// How far a deposit lies and which way: `1.2 km north-east`, or `Here`
/// within a few steps of it.
#[must_use]
pub fn where_words(metres: f64, bearing: Compass) -> String {
    let english = |key: &'static str| super::strings::english(key).unwrap_or(key);
    if metres < 5. {
        return english("strata.register.here").to_owned();
    }
    let (distance, km) = distance_words(metres);
    english(if km {
        "strata.register.where.km"
    } else {
        "strata.register.where.m"
    })
    .replace("{distance}", &distance)
    .replace("{bearing}", compass_words(bearing))
}

/// What the household knows of a deposit, which a row marks by a shape and
/// a word and not by colour alone.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mark {
    /// Sampled: a disc.
    Sampled,
    /// Read by a ping to a band or the figures: a square.
    Read,
    /// Known by its type only: a ring.
    TypeOnly,
}

impl Mark {
    /// The mark of a row.
    #[must_use]
    pub const fn of(reading: Reading, sampled: bool) -> Self {
        if sampled {
            Self::Sampled
        } else if matches!(reading, Reading::Type) {
            Self::TypeOnly
        } else {
            Self::Read
        }
    }

    /// Its word.
    #[must_use]
    pub fn word(self) -> &'static str {
        let key = match self {
            Self::Sampled => "strata.register.sampled",
            Self::Read => "strata.register.read",
            Self::TypeOnly => "strata.register.type_only",
        };
        super::strings::english(key).unwrap_or(key)
    }

    const fn role(self) -> &'static str {
        match self {
            Self::Sampled => MARK_ROLE,
            Self::Read => MARK_READ_ROLE,
            Self::TypeOnly => MARK_NONE_ROLE,
        }
    }
}

/// One row of the register, in display words.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RegisterRowView {
    /// The deposit.
    pub id: DepositId,
    /// Its place: `Calder flats`.
    pub place: String,
    /// Its class and type: `REACTIVE · Evaporite salt`.
    pub kind: String,
    /// The line it is best in, by its code: `PT 820`. A row is dense, and
    /// the card beside the list names the line in full.
    pub best: String,
    /// How far and which way: `1.2 km north-east`.
    pub lies: String,
    /// What the household knows of it.
    pub mark: Mark,
    /// Whether it is the one tracked.
    pub tracked: bool,
    /// Whose claim it is: `Concession of Pelagic Works`; empty on open
    /// ground.
    pub claim: String,
    /// What the harvesters planted on it hold; empty with none.
    pub holding: String,
    /// The long card of a lot drawn there.
    pub card: CardParams,
}

impl RegisterRowView {
    /// The sim's row in words.
    pub(crate) fn of(registries: &GameRegistries, row: &RegisterRow) -> Self {
        let english = |key: &'static str| super::strings::english(key).unwrap_or(key);
        let set = &registries.resources;
        let deposit = &row.deposit;
        let (attribute, value) = row.best;
        // A deposit sampled is read exactly whatever its ping told.
        let reading = if row.sampled {
            Reading::Exact
        } else {
            row.reading
        };
        Self {
            id: deposit.id,
            place: deposit.place.clone(),
            kind: english("strata.register.kind")
                .replace("{class}", &set.class_of(deposit.ty).name.to_uppercase())
                .replace(
                    "{type}",
                    set.get(deposit.ty).map_or("", |def| def.name.as_str()),
                ),
            // What the household reads of the best line: nothing at Type,
            // a band word at Bands, the figure at Exact.
            best: match reading {
                Reading::Type => String::new(),
                Reading::Bands => deposits::line_band(set, deposit, attribute)
                    .map(|band| {
                        english("strata.register.best.band")
                            .replace("{code}", attribute.code())
                            .replace("{band}", card::band_word(band))
                    })
                    .unwrap_or_default(),
                Reading::Exact => english("strata.register.best")
                    .replace("{code}", attribute.code())
                    .replace("{value}", &value.to_string()),
            },
            lies: where_words(row.distance_m, row.bearing),
            mark: Mark::of(reading, row.sampled),
            tracked: row.tracked,
            claim: String::new(),
            holding: String::new(),
            card: CardParams::of_deposit(deposit, registries, reading, true)
                .unwrap_or_else(CardParams::empty)
                .framed(),
        }
    }
}

/// What the harvesters on one deposit hold, as a register row says it:
/// `Harvester 120 of 300`, `2 harvesters 90 of 600, full`.
#[must_use]
pub fn holding_words(holding: &Holding) -> String {
    let english = |key: &'static str| super::strings::english(key).unwrap_or(key);
    let held = super::strings::grouped(holding.held);
    let hopper = super::strings::grouped(holding.hopper);
    let text = if holding.count == 1 {
        english("strata.harvester.register")
    } else {
        english("strata.harvester.register.n")
    }
    .replace("{count}", &holding.count.to_string())
    .replace("{held}", &held)
    .replace("{hopper}", &hopper);
    if holding.full {
        format!("{text}{}", english("strata.harvester.register.full"))
    } else {
        text
    }
}

/// Everything the register tab shows, in display words. Made from the
/// sim's list by [`RegisterView::of`], so what the tab says can be checked
/// without a screen.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RegisterView {
    /// The classes a chip filters by: each one's id and its name.
    pub classes: Vec<(Namespaced, String)>,
    /// The class listed; none for every class.
    pub class: Option<Namespaced>,
    /// What the list is of what the range holds: `NEAREST 60 OF 542 WITHIN
    /// 8 KM`, or `ALL 42 WITHIN 8 KM` when none was left out.
    pub caption: String,
    /// How many are listed and how many of them were stood at.
    pub count: String,
    /// The rows, nearest first.
    pub rows: Vec<RegisterRowView>,
    /// What stands in the list's place when it has no row.
    pub none: String,
}

impl RegisterView {
    /// The rows with whose claim each deposit is, where it is one.
    #[must_use]
    pub fn with_claims(
        mut self,
        books: Option<&super::claims::Books<'_>>,
        near: &NearDeposits,
    ) -> Self {
        let Some(books) = books else {
            return self;
        };
        for row in &mut self.rows {
            let Some(deposit) = near.deposits.iter().find(|deposit| deposit.id == row.id) else {
                continue;
            };
            row.claim = books.claim_words(deposit).unwrap_or_default();
            // The card beside the list says it too, with what working it means.
            if !row.claim.is_empty() {
                let said = [Some(row.claim.clone()), books.warning(deposit)];
                for (at, note) in said.into_iter().flatten().enumerate() {
                    row.card.notes.insert(at, note);
                }
            }
        }
        self
    }

    /// The rows with what the household's harvesters hold on each deposit.
    #[must_use]
    pub fn with_holdings(mut self, harvesters: &Harvesters) -> Self {
        for row in &mut self.rows {
            row.holding = harvesters
                .holding(row.id)
                .map_or_else(String::new, |holding| holding_words(&holding));
        }
        self
    }

    /// The register seen from `from` (`x`, `z`) over the deposits of
    /// `near`, of `class` when one is named. `published` is whether the
    /// world has a survey at all.
    #[must_use]
    pub fn of(
        registries: &GameRegistries,
        near: &NearDeposits,
        surveyed: &Surveyed,
        from: [f64; 2],
        class: Option<&Namespaced>,
        published: bool,
    ) -> Self {
        let english = |key: &'static str| super::strings::english(key).unwrap_or(key);
        let set = &registries.resources;
        let rules = set.rules().register;
        let range = format!("{:.0}", rules.range_m / 1000.);
        let listed = deposits::register(near, set, surveyed, from, class);
        let rows: Vec<RegisterRowView> = listed
            .rows
            .iter()
            .map(|row| RegisterRowView::of(registries, row))
            .collect();
        let visited = rows.iter().filter(|row| row.mark == Mark::Sampled).count();
        // The caption says what the list is of what there is: a list cut
        // to the nearest must not read as the whole range.
        let cut = listed.in_range > rules.rows as usize;
        Self {
            classes: set
                .classes()
                .iter()
                .map(|class| (class.id.clone(), class.name.to_uppercase()))
                .collect(),
            class: class.cloned(),
            caption: english(if cut {
                "strata.register.caption"
            } else {
                "strata.register.caption.all"
            })
            .replace("{rows}", &rules.rows.to_string())
            .replace("{total}", &listed.in_range.to_string())
            .replace("{range}", &range),
            count: english("strata.register.count")
                .replace("{shown}", &rows.len().to_string())
                .replace("{visited}", &visited.to_string()),
            none: english(if !published {
                "strata.register.no_survey"
            } else if class.is_some() {
                "strata.register.none.class"
            } else {
                "strata.register.none"
            })
            .replace("{range}", &range),
            rows,
        }
    }

    /// The row a card is shown for when neither the focus nor the pointer
    /// is on one: the tracked one, else the nearest.
    #[must_use]
    pub fn first(&self) -> Option<&RegisterRowView> {
        self.rows
            .iter()
            .find(|row| row.tracked)
            .or_else(|| self.rows.first())
    }
}

/// A box of a fixed width for one of a row's columns.
fn column_of(width: f32, justify: Justify, node: UiNodeDef) -> UiNodeDef {
    panel(
        "invisible",
        Layout {
            width: Some(Length::Px(width)),
            justify,
            ..row(0.)
        },
        vec![node],
    )
}

/// One row: the mark for whether it was stood at, its name over its class
/// and type, then its best line, where it lies and, on the
/// tracked one, the word. The whole row is the button.
fn register_row(view: &RegisterRowView, shown: bool) -> UiNodeDef {
    let id = row_id(view.id);
    let tag = deposit_tag(view.id);
    // A disc for one sampled, a square for one read and a ring for one
    // known by its type only: a shape, since neither typeface has the
    // glyphs, and the word beside it, and not a colour alone.
    let mark = UiNodeDef::Panel {
        role: Role::new(view.mark.role()),
        layout: Layout {
            width: Some(Length::Px(10.)),
            height: Some(Length::Px(10.)),
            ..column(0.)
        },
        children: Vec::new(),
        tags: Tags::new()
            .with(Tags::TEST_ID, &format!("{MARK_TAG}{tag}"))
            .with("state", view.mark.word()),
    };
    let mut texts = vec![
        with_id(
            line(row_name(&view.place, shown), ROW_NAME_ROLE),
            &format!("{NAME_TAG}{tag}"),
        ),
        line(
            format!("{} · {}", view.kind, view.mark.word()),
            ROW_KIND_ROLE,
        ),
    ];
    // A line for whose claim it is, only on a claimed deposit.
    if !view.claim.is_empty() {
        texts.push(with_id(
            line(view.claim.clone(), ROW_KIND_ROLE),
            &format!("{CLAIM_TAG}{tag}"),
        ));
    }
    // A third line only on a deposit that has a machine, so the other rows
    // keep their height.
    if !view.holding.is_empty() {
        texts.push(with_id(
            line(view.holding.clone(), ROW_KIND_ROLE),
            &format!("{HOLDING_TAG}{tag}"),
        ));
    }
    UiNodeDef::Card {
        role: Role::new(if view.tracked { ROW_ON_ROLE } else { ROW_ROLE }),
        layout: Layout {
            width: Some(Length::Percent(100.)),
            padding: Padding {
                top: 0.6,
                right: 1.6,
                bottom: 0.6,
                left: 1.4,
            },
            ..row(1.4)
        },
        children: vec![
            mark,
            panel(
                "invisible",
                Layout {
                    grow: 1.,
                    min_width: Some(Length::Px(0.)),
                    ..column(0.1)
                },
                texts,
            ),
            column_of(
                BEST_WIDTH,
                Justify::End,
                line(view.best.clone(), ROW_FIGURE_ROLE),
            ),
            column_of(
                WHERE_WIDTH,
                Justify::Start,
                line(view.lies.clone(), ROW_WHERE_ROLE),
            ),
            column_of(
                TRACKED_WIDTH,
                Justify::End,
                line(
                    if view.tracked {
                        "strata.register.tracked"
                    } else {
                        ""
                    },
                    ROW_TRACKED_ROLE,
                ),
            ),
        ],
        disabled: false,
        tags: Tags::new().with("menu", &id).with(Tags::TEST_ID, &id),
    }
}

/// A row's name as it is written: with the mark before it on the row
/// whose card stands beside the list.
fn row_name(place: &str, shown: bool) -> String {
    if shown {
        format!("{FOCUS_MARK}{place}")
    } else {
        place.to_owned()
    }
}

/// A class chip of the register, lit for the class listed.
fn class_chip(class: Option<&Namespaced>, label: &str, on: bool) -> UiNodeDef {
    let id = class_id(class);
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
        children: vec![line(
            if on {
                format!("{FOCUS_MARK}{label}")
            } else {
                label.to_owned()
            },
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

/// The register tab for `view`, with the keyboard starting on `focus` and
/// the card of `shown` beside the list.
#[must_use]
pub fn register_def(
    view: &RegisterView,
    focus: Option<String>,
    shown: Option<DepositId>,
) -> ScreenDef {
    let english = |key: &'static str| super::strings::english(key).unwrap_or(key);
    let head = head_row(
        "strata.register.title",
        BenchTab::Register,
        with_id(line(view.count.clone(), "browser.status"), COUNT_ID),
    );
    let mut chips = vec![class_chip(
        None,
        english("strata.register.all"),
        view.class.is_none(),
    )];
    chips.extend(
        view.classes
            .iter()
            .map(|(id, name)| class_chip(Some(id), name, view.class.as_ref() == Some(id))),
    );
    chips.push(spacer());
    chips.push(with_id(
        line(view.caption.clone(), "strata.stores.section"),
        CAPTION_ID,
    ));
    let rows: Vec<UiNodeDef> = if view.rows.is_empty() {
        vec![text(view.none.clone(), TextRole::Body, Some("text.label"))]
    } else {
        view.rows
            .iter()
            .map(|row| register_row(row, shown == Some(row.id)))
            .collect()
    };
    let card = view
        .rows
        .iter()
        .find(|row| shown == Some(row.id))
        .map_or_else(
            || {
                CardParams {
                    notes: vec![english("strata.register.card.none").to_owned()],
                    ..CardParams::default()
                }
                .framed()
            },
            |row| row.card.clone(),
        );
    // The list scrolls on its own and the card beside it stays, so the
    // card of a row far down the list is still read beside it.
    let body = panel(
        "invisible",
        Layout {
            align: Some(Align::Stretch),
            grow: 1.,
            min_height: Some(Length::Px(0.)),
            ..Layout {
                direction: LayoutDirection::Row,
                gap: 2.6,
                ..Layout::default()
            }
        },
        vec![
            UiNodeDef::Scroll {
                layout: Layout {
                    grow: 1.,
                    min_width: Some(Length::Px(0.)),
                    min_height: Some(Length::Px(0.)),
                    align: Some(Align::Stretch),
                    ..column(0.6)
                },
                scrollbar: true,
                children: rows,
                tags: Tags::new().with(Tags::TEST_ID, LIST_ID),
            },
            panel(
                "invisible",
                Layout {
                    width: Some(Length::Px(REGISTER_CARD_WIDTH)),
                    align: Some(Align::Stretch),
                    ..column(1.)
                },
                vec![card.node(Some(REGISTER_CARD_ID))],
            ),
        ],
    );
    frame(
        vec![
            head,
            panel("invisible", row(1.), chips),
            body,
            footer(Some("strata.register.keys")),
        ],
        1240.,
        focus,
    )
}

/// Whether a switch may happen now: the screen was opened at a terminal
/// and the local player is not fighting.
fn may_switch(world: &mut World, at_terminal: bool) -> bool {
    if !at_terminal {
        return false;
    }
    let engaged = world
        .get_resource::<CombatEncounter>()
        .is_some_and(|e| e.engaged && e.remaining > 0);
    let mut players = world.query_filtered::<&Combatant, With<LocalPlayer>>();
    players
        .single(world)
        .is_ok_and(|actor| !actor.in_combat(engaged))
}

/// The nearest terminal to the local player, for the read-only note.
fn nearest_terminal(world: &mut World) -> Option<TrainerSpot> {
    let mut players = world.query_filtered::<&WorldPosition, With<LocalPlayer>>();
    let player = *players.single(world).ok()?;
    let mut props = world.query::<(&WorldPosition, &Interaction)>();
    props
        .iter(world)
        .filter(|(_, prop)| prop.kind == InteractionKind::Terminal)
        .map(|(at, _)| TrainerSpot::from_positions(Vec::new(), at, &player))
        .min_by(|a, b| a.distance.total_cmp(&b.distance))
}

/// The register as the world stands: the sim's near deposits and survey
/// record, seen from the local player, or from the primary observer when
/// nobody is out.
fn register_view(world: &mut World, registries: &GameRegistries) -> RegisterView {
    let class = world.resource::<BenchState>().class.clone();
    let mut players = world.query_filtered::<&WorldPosition, With<LocalPlayer>>();
    let from = players
        .single(world)
        .ok()
        .map(|at| at.0)
        .or_else(|| world.get_resource::<Observers>()?.primary)
        .map_or([0., 0.], |at| [at.x(), at.z()]);
    let empty = (NearDeposits::default(), Surveyed::default());
    let near = world.get_resource::<NearDeposits>().unwrap_or(&empty.0);
    RegisterView::of(
        registries,
        near,
        world.get_resource::<Surveyed>().unwrap_or(&empty.1),
        from,
        class.as_ref(),
        world.contains_resource::<Survey>(),
    )
    .with_claims(super::claims::books_of(world).as_ref(), near)
    .with_holdings(
        &world
            .get_resource::<Harvesters>()
            .cloned()
            .unwrap_or_default(),
    )
}

fn build(world: &mut World) -> Option<ScreenDef> {
    let registries = world.get_resource::<Registries>()?.0.clone();
    let state = world.resource::<BenchState>().clone();
    if state.tab == BenchTab::Register {
        let view = register_view(world, &registries);
        // The keyboard goes back to the control that caused the rebuild
        // while it is still there, else to the row a card is shown for.
        let there = |id: &String| {
            BenchTab::ALL.iter().any(|tab| tab.tab_id() == *id)
                || *id == class_id(None)
                || view
                    .classes
                    .iter()
                    .any(|(class, _)| class_id(Some(class)) == *id)
                || view.rows.iter().any(|row| row_id(row.id) == *id)
        };
        let shown = state
            .shown
            .filter(|shown| view.rows.iter().any(|row| row.id == *shown))
            .or_else(|| view.first().map(|row| row.id));
        let focus = state
            .last
            .filter(there)
            .or_else(|| shown.map(row_id))
            .or_else(|| Some(BenchTab::Register.tab_id()));
        let def = register_def(&view, focus, shown);
        let mut state = world.resource_mut::<BenchState>();
        state.register = Some(view);
        state.shown = shown;
        return Some(def);
    }
    let view = world.get_resource::<BenchView>()?.clone();
    let may_switch = may_switch(world, state.at_terminal);
    let terminal = nearest_terminal(world);
    let mut def = screen_def(
        &registries.progression,
        &view,
        may_switch,
        state.at_terminal,
        terminal.as_ref(),
    );
    def.initial_focus = state
        .last
        .filter(|last| BenchTab::ALL.iter().any(|tab| tab.tab_id() == *last));
    Some(def)
}

fn on_top(stack: &ScreenStack) -> bool {
    stack.top().is_some_and(|top| top.kind == kind())
}

/// Pushes the bench; `at_terminal` says whether its buttons may be live.
/// Rebuilds it in place when it is already on top.
pub fn open(commands: &mut Commands, at_terminal: bool) {
    open_on(commands, at_terminal, BenchTab::Bench);
}

/// [`open`] on a tab: the pause's Bench button and a terminal's F open the
/// bench, and the flag for a still the register.
pub fn open_on(commands: &mut Commands, at_terminal: bool, tab: BenchTab) {
    commands.queue(move |world: &mut World| {
        *world.resource_mut::<BenchState>() = BenchState {
            at_terminal,
            tab,
            ..BenchState::default()
        };
        let Some(def) = build(world) else {
            tracing::warn!("no household published: nothing to show on the bench");
            return;
        };
        if on_top(world.resource::<ScreenStack>()) {
            replace_screen(&mut world.commands(), Arc::new(def), None);
        } else {
            push_screen(&mut world.commands(), Arc::new(def), None);
        }
    });
}

/// `Update`: Switch starts the fade, and its sound with it; Join goes to
/// the game at once. A tab shows the other half of the screen, a class
/// chip of the register lists that class, and a row of it asks the sim to
/// track its deposit, or to let it go when it is the one tracked: what is
/// tracked is the sim's to hold.
#[allow(
    clippy::too_many_arguments,
    reason = "the choices, the bench's state and what a row choice tracks and leads to"
)]
pub fn answer_choices(
    mut choices: MessageReader<MenuChoice>,
    mut fade: ResMut<SwitchFade>,
    mut requests: MessageWriter<BenchRequest>,
    mut cues: MessageWriter<super::progress::ProgressCue>,
    mut state: ResMut<BenchState>,
    mut survey: Option<ResMut<SurveyInbox>>,
    mut harvest: Option<ResMut<HarvestInbox>>,
    surveyed: Option<Res<Surveyed>>,
    harvesters: Option<Res<Harvesters>>,
) {
    for choice in choices.read() {
        if choice.screen != kind() {
            continue;
        }
        let id = choice.id.as_str();
        if let Some(tab) = id
            .strip_prefix(TAB_TAG)
            .and_then(|tab| BenchTab::ALL.into_iter().find(|of| of.id() == tab))
        {
            if state.tab != tab {
                state.tab = tab;
                state.shown = None;
            }
            state.last = Some(id.to_owned());
            state.dirty = true;
            continue;
        }
        if let Some(class) = id.strip_prefix(CLASS_TAG) {
            state.class = (class != CLASS_ALL).then(|| class.parse().ok()).flatten();
            state.shown = None;
            state.last = Some(id.to_owned());
            state.dirty = true;
            continue;
        }
        if let Some(deposit) = id.strip_prefix(ROW_TAG).and_then(tagged) {
            // Which of the two is the sim's to say, against what is
            // tracked when it hears: the view here is a frame behind it.
            if let Some(inbox) = survey.as_mut() {
                inbox.push(SurveyRequest::ToggleTrack(deposit));
                // Tracking a deposit with a machine on it also leads to the
                // machine, since the deposit's centre can be far from it.
                let tracking = surveyed
                    .as_deref()
                    .and_then(|surveyed| surveyed.tracked.as_ref())
                    .is_some_and(|tracked| tracked.id == deposit);
                let machine = harvesters
                    .as_deref()
                    .and_then(|all| all.list.iter().find(|each| each.deposit == deposit));
                if let (false, Some(machine), Some(inbox)) = (tracking, machine, harvest.as_mut()) {
                    inbox.push(HarvestRequest::Lead { id: machine.id });
                }
            }
            state.shown = Some(deposit);
            state.last = Some(id.to_owned());
            continue;
        }
        if let Some(index) = choice
            .id
            .strip_prefix(SWITCH_TAG)
            .and_then(|s| s.parse().ok())
        {
            if fade.target.is_none() {
                *fade = SwitchFade {
                    target: Some(index),
                    elapsed: 0.,
                    sent: false,
                };
                cues.write(super::progress::ProgressCue::Switched);
            }
        } else if let Some(index) = choice
            .id
            .strip_prefix(JOIN_TAG)
            .and_then(|s| s.parse().ok())
        {
            requests.write(BenchRequest::Join(index));
        }
    }
}

/// `Startup`: the curtain, hidden until a switch.
pub fn spawn_curtain(mut commands: Commands) {
    commands.spawn((
        FadeCurtain,
        Node {
            position_type: PositionType::Absolute,
            width: Val::Percent(100.),
            height: Val::Percent(100.),
            ..Node::default()
        },
        BackgroundColor(Color::NONE),
        GlobalZIndex(zbands::TOAST + 1),
        bevy::picking::Pickable::IGNORE,
    ));
}

/// `Update`: the fade. Black comes down over the open screen, the request
/// goes out and the screen closes while nothing can be seen, and the
/// world comes back with the other character in it.
pub fn run_fade(
    time: Res<Time>,
    stack: Res<ScreenStack>,
    mut fade: ResMut<SwitchFade>,
    mut requests: MessageWriter<BenchRequest>,
    mut curtains: Query<&mut BackgroundColor, With<FadeCurtain>>,
    mut commands: Commands,
) {
    let Some(target) = fade.target else {
        return;
    };
    fade.elapsed += time.delta_secs();
    let t = fade.elapsed;
    let alpha = if t < FADE_IN {
        t / FADE_IN
    } else if t < FADE_IN + FADE_HOLD {
        1.
    } else {
        (1. - (t - FADE_IN - FADE_HOLD) / FADE_OUT).max(0.)
    };
    if t >= FADE_IN && !fade.sent {
        fade.sent = true;
        requests.write(BenchRequest::Switch(target));
        if on_top(&stack) {
            pop_screen(&mut commands);
        }
    }
    if t >= FADE_IN + FADE_HOLD + FADE_OUT {
        *fade = SwitchFade::default();
    }
    for mut curtain in &mut curtains {
        curtain.0 = Color::srgba(0., 0., 0., alpha);
    }
}

/// `Update`: a fall's curtain. It comes down and waits for the game, which
/// wakes it once the member is home.
pub fn run_fall_fade(
    time: Res<Time>,
    mut fade: ResMut<FallFade>,
    mut curtains: Query<&mut BackgroundColor, With<FadeCurtain>>,
) {
    let alpha = match fade.phase {
        FallPhase::Idle => return,
        FallPhase::Falling => {
            fade.elapsed += time.delta_secs();
            if fade.elapsed >= fade.length {
                fade.phase = FallPhase::Black;
            }
            (fade.elapsed / fade.length.max(f32::EPSILON)).min(1.)
        }
        FallPhase::Black => 1.,
        FallPhase::Waking => {
            fade.elapsed += time.delta_secs();
            let alpha = (1. - fade.elapsed / FADE_OUT).max(0.);
            if fade.elapsed >= FADE_OUT {
                *fade = FallFade::default();
            }
            alpha
        }
    };
    for mut curtain in &mut curtains {
        curtain.0 = Color::srgba(0., 0., 0., alpha);
    }
}

/// `Update`: the game's answers as toasts.
pub fn toast_outcomes(mut outcomes: MessageReader<BenchOutcome>, mut commands: Commands) {
    for outcome in outcomes.read() {
        toast(
            &mut commands,
            ToastSpec::new(outcome.message.clone())
                .level(if outcome.ok {
                    ToastLevel::Success
                } else {
                    ToastLevel::Warning
                })
                .duration(Duration::from_millis(2600)),
        );
    }
}

/// `Update`: a changed household or reach while the bench is up marks it
/// for a rebuild; a terminal walked away from turns its buttons off. The
/// register is marked when what it lists changed: the deposits near, or
/// the survey record (a row tracked, a post stood at). A screen pushed
/// this frame is left alone: a pop in the frame of its push races the
/// stack's own deferred work on the new tree.
pub fn mark_dirty(
    stack: Res<ScreenStack>,
    view: Option<Res<BenchView>>,
    reach: Option<Res<Reach>>,
    near: Option<Res<NearDeposits>>,
    surveyed: Option<Res<Surveyed>>,
    mut was_on_top: Local<bool>,
    mut state: ResMut<BenchState>,
) {
    let settled = *was_on_top;
    *was_on_top = on_top(&stack);
    if !settled || !*was_on_top {
        return;
    }
    if state.tab == BenchTab::Register {
        let moved = near.is_some_and(|near| near.is_changed())
            || surveyed.is_some_and(|surveyed| surveyed.is_changed());
        if moved && !state.dirty {
            state.dirty = true;
        }
        return;
    }
    if let Some(reach) = &reach
        && reach.is_changed()
    {
        let here = matches!(reach.nearest, Some((_, InteractionKind::Terminal)));
        if state.at_terminal && !here {
            state.at_terminal = false;
            state.dirty = true;
        }
    }
    if view.is_some_and(|v| v.is_changed()) && !state.dirty {
        state.dirty = true;
    }
}

/// `Update`: rebuilds the open bench when it is marked.
pub fn rebuild(world: &mut World) {
    if !world.resource::<BenchState>().dirty || !on_top(world.resource::<ScreenStack>()) {
        return;
    }
    world.resource_mut::<BenchState>().dirty = false;
    if let Some(def) = build(world) {
        replace_screen(&mut world.commands(), Arc::new(def), None);
    }
}

/// `Update`, after the rebuild: the card beside the register's list is
/// that of the row under the keyboard's focus, or under the pointer once
/// it has moved over the screen, and stays on the row last looked at when
/// neither is on one. The row is marked before its name. Both are written
/// in place: looking at a row rebuilds nothing, so the list keeps its
/// scroll.
#[allow(
    clippy::too_many_arguments,
    reason = "the screen, the device in use, the focus, the pointer and its motion, and the nodes"
)]
pub fn show_row_card(
    stack: Res<ScreenStack>,
    mode: Res<InputMode>,
    mut cursor: MessageReader<CursorMoved>,
    mut inputs: MessageReader<PointerInput>,
    focus: Option<Res<InputFocus>>,
    mut state: ResMut<BenchState>,
    ids: Query<(Entity, &TestId)>,
    hovered: Query<(&Hovered, &TestId)>,
    cards: Query<&ItemCard>,
    mut names: Query<(&TestId, &mut LocText, &mut Text)>,
    mut commands: Commands,
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
    if !on_top(&stack) || state.tab != BenchTab::Register {
        return;
    }
    if moved > 0 && !state.pointer_moved {
        state.pointer_moved = true;
    }
    let row_of = |id: &TestId| id.0.strip_prefix(ROW_TAG).and_then(tagged);
    let pointed = (state.pointer_moved && *mode == InputMode::Pointer)
        .then(|| {
            hovered
                .iter()
                .find(|(hovered, id)| hovered.get() && id.0.starts_with(ROW_TAG))
                .and_then(|(_, id)| row_of(id))
        })
        .flatten();
    let focused = focus
        .as_ref()
        .filter(|focus| focus.is_changed() || state.shown.is_none())
        .and_then(|focus| focus.get())
        .and_then(|entity| ids.get(entity).ok())
        .and_then(|(_, id)| row_of(id));
    let Some(view) = state.register.as_ref() else {
        return;
    };
    let wanted = pointed
        .or(focused)
        .filter(|id| view.rows.iter().any(|row| row.id == *id))
        .or(state.shown)
        .or_else(|| view.first().map(|row| row.id));
    let Some(row) = wanted.and_then(|id| view.rows.iter().find(|row| row.id == id)) else {
        return;
    };
    let Some(card) = ids
        .iter()
        .find(|(_, id)| id.0 == REGISTER_CARD_ID)
        .map(|(entity, _)| entity)
    else {
        // A screen replaced this frame has not spawned its nodes yet; the
        // card is painted when it has.
        return;
    };
    for (id, mut loc, mut text) in &mut names {
        let Some(of) = id.0.strip_prefix(NAME_TAG).and_then(tagged) else {
            continue;
        };
        let Some(listed) = view.rows.iter().find(|row| row.id == of) else {
            continue;
        };
        let words = row_name(&listed.place, of == row.id);
        if text.0 != words {
            loc.key = LocKey(words.clone());
            text.0 = words;
        }
    }
    if cards.get(card).is_ok_and(|shown| shown.0 != row.card) {
        let params = row.card.clone();
        commands.queue(move |world: &mut World| card::repaint(world, card, &params));
    }
    let shown = row.id;
    if state.shown != Some(shown) {
        state.shown = Some(shown);
    }
}

/// Run condition for [`rebuild`]: marked, and the bench is the screen on
/// top. Both are what it tests first, so a clean or closed bench costs no
/// exclusive system.
fn dirty_and_on_top(state: Res<BenchState>, stack: Res<ScreenStack>) -> bool {
    state.dirty && on_top(&stack)
}

/// Registers the view, the messages and the systems. Only called once the
/// UI is wired.
pub(crate) fn register(app: &mut bevy::app::App) {
    app.init_resource::<BenchView>()
        .init_resource::<BenchState>()
        .init_resource::<SwitchFade>()
        .init_resource::<FallFade>()
        .add_message::<BenchRequest>()
        .add_message::<BenchOutcome>()
        .add_systems(bevy::app::Startup, spawn_curtain)
        .add_systems(
            bevy::app::Update,
            (
                answer_choices,
                run_fade,
                run_fall_fade,
                toast_outcomes,
                mark_dirty.run_if(super::screen_up(kind)),
                rebuild.run_if(dirty_and_on_top),
                show_row_card,
            )
                .chain()
                .in_set(super::ScreenWork),
        );
}

/// A family's words from the strings table: `kind` is `the`, `in_hand` or empty.
fn family_word(kind: &str, weapon: Weapon) -> &'static str {
    let key = if kind.is_empty() {
        format!("strata.family.{}", weapon.name().to_lowercase())
    } else {
        format!("strata.family.{kind}.{}", weapon.name().to_lowercase())
    };
    super::strings::english(&key).unwrap_or("weapon")
}

#[cfg(test)]
mod tests {
    /// Every deposit of `list` found, the ones in `sampled` sampled.
    fn found_of(
        list: &NearDeposits,
        sampled: &[DepositId],
    ) -> std::collections::BTreeMap<DepositId, veyra_sim::deposits::Found> {
        list.deposits
            .iter()
            .map(|deposit| {
                let sampled = sampled.contains(&deposit.id);
                (
                    deposit.id,
                    veyra_sim::deposits::Found {
                        reading: strata_registry::Reading::Exact,
                        sampled,
                        best_pm: deposit.peak,
                        best_at: deposit.at,
                    },
                )
            })
            .collect()
    }

    use super::*;
    use strata_registry::progression::XpKind;

    /// A deposit's row says what the household's harvesters on it hold, and
    /// nothing at all for a deposit with none.
    #[test]
    fn a_row_says_what_its_harvesters_hold() {
        use crate::testing::{deposit, planted, shipped};
        let registries = shipped();
        let calder = deposit(
            &registries,
            "strata:evaporite_salt",
            "Calder",
            [3, -2],
            [900., 0.],
        );
        let list = NearDeposits {
            deposits: vec![calder.clone()],
            around: Vec::new(),
        };
        let mut surveyed = Surveyed::default();
        surveyed.found.insert(
            calder.id,
            deposits::Found {
                reading: Reading::Exact,
                sampled: true,
                best_pm: 300,
                best_at: calder.at,
            },
        );
        let view = |harvesters: &Harvesters| {
            RegisterView::of(&registries, &list, &surveyed, [0., 0.], None, true)
                .with_holdings(harvesters)
        };
        assert_eq!(view(&Harvesters::default()).rows[0].holding, "");
        let mut all = Harvesters::default();
        all.list
            .push(planted(&registries, &calder, [900., 0.], 600, 120));
        assert_eq!(view(&all).rows[0].holding, "Harvester 120 of 1,000");
        let mut second = planted(&registries, &calder, [950., 0.], 600, 1000);
        second.id = 1;
        all.list.push(second);
        assert_eq!(view(&all).rows[0].holding, "2 harvesters 1,120 of 2,000");
        all.list[0].held = 1000;
        assert_eq!(
            view(&all).rows[0].holding,
            "2 harvesters 2,000 of 2,000, full"
        );
    }

    /// Choosing a row with a machine on it tracks the deposit and leads to
    /// the machine; choosing it again lets go and leads nowhere.
    #[test]
    fn a_row_with_a_machine_leads_to_it() {
        use crate::testing::{deposit, planted, shipped};
        let registries = shipped();
        let calder = deposit(
            &registries,
            "strata:evaporite_salt",
            "Calder",
            [3, -2],
            [900., 0.],
        );
        let mut app = bevy::app::App::new();
        app.add_message::<MenuChoice>()
            .add_message::<BenchRequest>()
            .add_message::<super::super::progress::ProgressCue>()
            .init_resource::<SwitchFade>()
            .init_resource::<BenchState>()
            .init_resource::<SurveyInbox>()
            .init_resource::<HarvestInbox>()
            .init_resource::<Surveyed>()
            .add_systems(bevy::app::Update, answer_choices);
        let mut all = Harvesters::default();
        let mut one = planted(&registries, &calder, [950., 40.], 600, 10);
        one.id = 4;
        all.list.push(one);
        app.insert_resource(all);
        let choose = |app: &mut bevy::app::App| {
            app.world_mut().write_message(MenuChoice {
                id: row_id(calder.id),
                screen: kind(),
                entity: Entity::PLACEHOLDER,
            });
            app.update();
            let survey = app.world_mut().resource_mut::<SurveyInbox>().drain();
            let lead = app.world_mut().resource_mut::<HarvestInbox>().drain();
            (survey, lead)
        };
        let (survey, lead) = choose(&mut app);
        assert_eq!(survey, [SurveyRequest::ToggleTrack(calder.id)]);
        assert_eq!(lead, [HarvestRequest::Lead { id: 4 }]);
        app.world_mut().resource_mut::<Surveyed>().tracked = Some(calder.clone());
        let (survey, lead) = choose(&mut app);
        assert_eq!(survey, [SurveyRequest::ToggleTrack(calder.id)]);
        assert!(lead.is_empty(), "letting go leads nowhere");
    }

    /// A row marks what the household knows of the deposit by a shape and a
    /// word, and tells of its best line as much as that: nothing at Type, a
    /// band word at Bands, the figure at Exact and once sampled.
    #[test]
    fn a_row_tells_a_deposit_as_far_as_it_is_read() {
        use crate::testing::{deposit, shipped};
        let registries = shipped();
        let calder = deposit(
            &registries,
            "strata:evaporite_salt",
            "Calder",
            [3, -2],
            [900., 0.],
        );
        let list = NearDeposits {
            deposits: vec![calder.clone()],
            around: Vec::new(),
        };
        let row = |reading, sampled| {
            let mut surveyed = Surveyed::default();
            surveyed.found.insert(
                calder.id,
                deposits::Found {
                    reading,
                    sampled,
                    best_pm: 300,
                    best_at: calder.at,
                },
            );
            RegisterView::of(&registries, &list, &surveyed, [0., 0.], None, true)
                .rows
                .remove(0)
        };
        let typed = row(Reading::Type, false);
        assert_eq!((typed.mark, typed.best.as_str()), (Mark::TypeOnly, ""));
        assert!(
            typed.card.text().contains("Not read"),
            "{}",
            typed.card.text()
        );
        assert_eq!(typed.mark.word(), "Type only");
        let banded = row(Reading::Bands, false);
        assert_eq!(banded.mark, Mark::Read);
        assert!(
            banded.best.chars().any(char::is_alphabetic),
            "a band word: {}",
            banded.best
        );
        assert!(
            !banded.best.chars().any(|c| c.is_ascii_digit()),
            "{}",
            banded.best
        );
        let exact = row(Reading::Exact, false);
        assert_eq!(exact.mark, Mark::Read);
        assert!(
            exact.best.chars().any(|c| c.is_ascii_digit()),
            "{}",
            exact.best
        );
        // Sampled is read exactly, whatever the ping told.
        let sampled = row(Reading::Type, true);
        assert_eq!(sampled.mark, Mark::Sampled);
        assert_eq!(sampled.best, exact.best);
        assert_eq!(sampled.mark.word(), "Sampled");
        // Three shapes, three words.
        let words: std::collections::BTreeSet<_> = [typed.mark, banded.mark, sampled.mark]
            .iter()
            .map(|mark| mark.word())
            .collect();
        assert_eq!(words.len(), 3);
        // An empty register says how to start.
        let none = RegisterView::of(
            &registries,
            &list,
            &Surveyed::default(),
            [0., 0.],
            None,
            true,
        );
        assert!(none.rows.is_empty());
        assert!(
            none.none.contains("Open the survey tool with G"),
            "{}",
            none.none
        );
    }

    #[test]
    fn the_summary_names_the_trees_and_what_the_boxes_open() {
        let graph = SkillGraph::shipped();
        let mut sheet = CharacterSheet::default();
        for id in ["blade.novice", "blade.cuts.1", "marksman.novice"] {
            sheet.learned.insert(id.to_owned());
        }
        sheet.credit(XpKind::Blade, 10);
        let (trees, can) = summary(&graph, &sheet);
        assert_eq!(trees, "Marksman · 1 box  ·  Blade · 2 boxes");
        assert_eq!(
            weapon_words(&graph, &sheet, Weapon::Carbine),
            "Carbine in hand (not certified)"
        );
        assert_eq!(
            weapon_words(&graph, &sheet, Weapon::Pistol),
            "Pistol in hand"
        );
        assert_eq!(
            can,
            "The light cut, the follow-up, the heavy, the pistol, the blade"
        );
        let (trees, can) = summary(&graph, &CharacterSheet::default());
        assert_eq!(trees, "No boxes learned");
        assert_eq!(can, "Nothing the fight uses yet");
    }

    #[test]
    fn the_screen_lights_switch_only_at_a_terminal_out_of_combat() {
        let graph = SkillGraph::shipped();
        let one = BenchCard {
            name: "A".to_owned(),
            tint: 0,
            weapon: Weapon::Carbine,
            sheet: CharacterSheet::default(),
            work: None,
            injured_min: None,
        };
        let view = BenchView {
            characters: vec![one.clone(), one.clone()],
            active: 0,
            recruits: vec![one],
            capacity: 4,
        };
        let live = |def: &ScreenDef, tag: &str| {
            let mut def = def.clone();
            match def.root.find_mut(tag) {
                Some(UiNodeDef::Button { opts, .. }) => !opts.disabled,
                other => panic!("{tag}: {other:?}"),
            }
        };
        let read_only = screen_def(&graph, &view, false, false, None);
        assert!(!live(&read_only, "bench.switch:1"));
        assert!(!live(&read_only, "bench.join:0"));
        assert!(!live(&read_only, "bench.playing"));
        let fighting = screen_def(&graph, &view, false, true, None);
        assert!(!live(&fighting, "bench.switch:1"));
        assert!(live(&fighting, "bench.join:0"), "joining needs no calm");
        let here = screen_def(&graph, &view, true, true, None);
        assert!(live(&here, "bench.switch:1"));
    }

    /// The register is the sim's list in words: nearest first, each row
    /// with its class and type, its best line by its code, how
    /// far and which way, whether it was stood at and whether it is the one
    /// tracked, and the card of a lot drawn there.
    #[test]
    #[allow(
        clippy::too_many_lines,
        reason = "one list, read row by row as the tab is"
    )]
    fn the_register_is_the_sims_list_in_words() {
        use crate::testing::{deposit, shipped};
        let registries = shipped();
        let far = deposit(
            &registries,
            "strata:evaporite_salt",
            "Calder",
            [3, -2],
            [900., 900.],
        );
        let near = deposit(
            &registries,
            "strata:light_alloy_ore",
            "Rake",
            [0, 0],
            [0., -510.],
        );
        let west = deposit(
            &registries,
            "strata:sulphur",
            "Sorrel",
            [-3, 0],
            [-2_400., 0.],
        );
        let list = NearDeposits {
            deposits: vec![far.clone(), near.clone(), west.clone()],
            around: Vec::new(),
        };
        let surveyed = Surveyed {
            found: found_of(&list, &[near.id]),
            tracked: Some(far.clone()),
            ..Surveyed::default()
        };
        let view = RegisterView::of(&registries, &list, &surveyed, [0., 0.], None, true);
        assert_eq!(view.caption, "ALL 3 WITHIN 8 KM");
        assert_eq!(view.count, "3 found · 1 sampled");
        assert_eq!(
            view.classes
                .iter()
                .map(|(_, name)| name.as_str())
                .collect::<Vec<_>>(),
            ["REACTIVE", "BRINE", "STRUCTURAL METAL", "CONDUCTOR"]
        );
        let rows: Vec<(&str, &str, &str, bool, bool)> = view
            .rows
            .iter()
            .map(|row| {
                (
                    row.place.as_str(),
                    row.kind.as_str(),
                    row.lies.as_str(),
                    row.mark == Mark::Sampled,
                    row.tracked,
                )
            })
            .collect();
        assert_eq!(
            rows,
            [
                (
                    "Rake scar",
                    "STRUCTURAL METAL · Light alloy ore",
                    "510 m south",
                    true,
                    false
                ),
                (
                    "Calder flats",
                    "REACTIVE · Evaporite salt",
                    "1.3 km north-west",
                    false,
                    true
                ),
                (
                    "Sorrel vents",
                    "REACTIVE · Sulphur",
                    "2.4 km east",
                    false,
                    false
                ),
            ]
        );
        // The best line is the sim's, by its code in the row and in full
        // on the card beside it.
        let (attribute, value) = deposits::best_line(&registries.resources, &far);
        assert_eq!(view.rows[1].best, format!("{} {value}", attribute.code()));
        assert_eq!(view.rows[1].card.name, "Calder salt");
        assert!(view.rows[1].card.framed);
        assert!(
            view.rows[1]
                .card
                .ranges
                .iter()
                .any(|range| range.name == attribute.name() && range.value == value)
        );
        assert_eq!(view.first().map(|row| row.id), Some(far.id), "the tracked");

        let all = Surveyed {
            found: found_of(&list, &[]),
            ..Surveyed::default()
        };
        // A class chip lists that class alone.
        let reactive: Namespaced = "strata:reactive".parse().expect("an id");
        let only = RegisterView::of(&registries, &list, &all, [0., 0.], Some(&reactive), true);
        assert_eq!(
            only.rows.iter().map(|row| row.id).collect::<Vec<_>>(),
            [far.id, west.id]
        );
        assert_eq!(only.first().map(|row| row.id), Some(far.id), "the nearest");
        let brine: Namespaced = "strata:brine".parse().expect("an id");
        let none = RegisterView::of(&registries, &list, &all, [0., 0.], Some(&brine), true);
        assert!(none.rows.is_empty());
        assert_eq!(none.none, "No deposit of that class found within 8 km.");
        let unpublished = RegisterView::of(
            &registries,
            &NearDeposits::default(),
            &Surveyed::default(),
            [0., 0.],
            None,
            false,
        );
        assert_eq!(unpublished.none, "No survey is published in this world.");

        // A list cut to the dials' rows says so, and the tracked deposit
        // the nearest rows do not reach keeps the last of them.
        let many: Vec<_> = (0..70)
            .map(|n| {
                let at = [100. * f64::from(n + 1), 0.];
                deposit(&registries, "strata:sulphur", "Sorrel", [n, 9], at)
            })
            .collect();
        let crowded = NearDeposits {
            deposits: many.clone(),
            around: Vec::new(),
        };
        let surveyed = Surveyed {
            found: found_of(&crowded, &[]),
            tracked: Some(many[66].clone()),
            ..Surveyed::default()
        };
        let cut = RegisterView::of(&registries, &crowded, &surveyed, [0., 0.], None, true);
        assert_eq!(cut.caption, "NEAREST 60 OF 70 WITHIN 8 KM");
        assert_eq!(cut.rows.len(), 60);
        let kept = cut.rows.last().expect("sixty rows");
        assert!(kept.tracked && kept.id == surveyed.tracked.as_ref().expect("one").id);
        assert_eq!(kept.lies, "6.7 km west");

        // The ids a row is found and tracked by survive their tag.
        assert_eq!(row_id(far.id), "register.row:3,-2");
        assert_eq!(tagged("3,-2"), Some(far.id));
        assert_eq!(tagged("-3,0"), Some(west.id));
        assert_eq!(tagged("3"), None);
        assert_eq!(where_words(3., Compass::North), "Here");
        assert_eq!(distance_words(42.4), ("42".to_owned(), false));
        assert_eq!(distance_words(994.), ("990".to_owned(), false));
        assert_eq!(distance_words(996.), ("1.0".to_owned(), true));
        for bearing in Compass::ALL {
            let key = format!("strata.compass.{}", bearing.key());
            assert!(super::super::strings::english(&key).is_some(), "{key}");
            assert!(!compass_words(bearing).contains('_'), "{bearing:?}");
        }
    }

    /// Every role and key either tab names is in the theme and has
    /// English, the row's families in each of their states.
    #[test]
    fn every_role_the_register_names_is_in_the_theme() {
        use crate::testing::{deposit, shipped};
        use std::collections::BTreeSet;
        let registries = shipped();
        let calder = deposit(
            &registries,
            "strata:evaporite_salt",
            "Calder",
            [3, -2],
            [900., 900.],
        );
        let list = NearDeposits {
            deposits: vec![calder.clone()],
            around: Vec::new(),
        };
        let mut roles: BTreeSet<String> = [
            ROW_ROLE,
            ROW_ON_ROLE,
            MARK_ROLE,
            MARK_NONE_ROLE,
            TAB_ROLE,
            TAB_TEXT_ROLE,
        ]
        .into_iter()
        .map(str::to_owned)
        .collect();
        let mut keys = Vec::new();
        for (tracked, published) in [(None, true), (Some(calder.clone()), true), (None, false)] {
            let surveyed = Surveyed {
                found: found_of(&list, &[]),
                tracked,
                ..Surveyed::default()
            };
            let near = if published {
                list.clone()
            } else {
                NearDeposits::default()
            };
            let view = RegisterView::of(&registries, &near, &surveyed, [0., 0.], None, published);
            register_def(&view, None, view.first().map(|row| row.id))
                .root
                .walk(&mut |node| match node {
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
        for key in keys.iter().filter(|key| key.starts_with("strata.")) {
            assert!(
                super::super::strings::english(key).is_some(),
                "{key} has no English"
            );
        }
        for role in [
            "strata.tracker.plate",
            "strata.tracker.pin",
            "strata.tracker.text",
            "strata.purse.value",
            "strata.purse.label",
        ] {
            assert!(
                theme.roles.contains_key(&Role::new(role)),
                "{role} is not in the theme"
            );
        }
    }

    #[test]
    fn the_read_only_note_says_where_the_terminal_is() {
        let spot = TrainerSpot::from_positions(
            Vec::new(),
            &WorldPosition::new(0., 0., 12.),
            &WorldPosition::new(0., 0., 0.),
        );
        let words = read_only_words(Some(&spot));
        assert!(
            words.ends_with("The nearest terminal is 12 m to the north."),
            "{words}"
        );
        assert!(!read_only_words(None).contains("nearest"));
    }
}
