//! The character sheet: the trees of `docs/design/progression.md` as a
//! screen, with the sim's own rules deciding what can be learned.
//!
//! The screen is built from the local player's [`CharacterSheet`], the
//! graph in [`Registries`] and the [`Reach`] the sim measured, and it is
//! rebuilt whenever any of those change while it is open, so a lesson, a
//! hit that paid an XP gate or a step towards a trainer is on screen the
//! same frame. Rebuilding a definition is cheaper to get right than
//! editing a spawned tree node by node, and the stack's `replace_screen`
//! keeps the screen where it was without fading it in a second time.
//! Nothing here decides a rule: Learn and Forget call
//! [`CharacterSheet::learn`] and [`CharacterSheet::forget`] and show the
//! refusal they return, and the state a box card shows is
//! [`CharacterSheet::can_learn`]'s answer put into words.
//!
//! The sheet is local play's: it reads the local player and writes to it,
//! and the game copies the sheet into its household. Nothing is sent
//! anywhere.

use std::collections::BTreeSet;
use std::sync::Arc;

use bevy::ecs::name::Name;
use bevy::ecs::prelude::*;
use bevy::input::ButtonInput;
use bevy::input::keyboard::KeyCode;
use slotted::menu::MenuChoice;
use strata_registry::Reading;
use strata_registry::progression::{BoxDef, Effect, Op, SkillGraph, TreeDef, XpKind};
use veyra_sim::combat::{
    Action, ActionSpec, CombatEncounter, Combatant, FireMode, Weapon, WeaponSpec,
};
use veyra_sim::combat_tables::{self, CombatTables};
use veyra_sim::progression::{
    CharacterSheet, EffectiveTables, ForgetRefused, Interaction, InteractionKind, LearnRefused,
    Reach, apply,
};
use veyra_sim::{LocalPlayer, Registries, WorldPosition};

use super::icons;
use slotted::theme::Role;
use slotted::ui::def::{
    Align, ButtonOpts, ButtonVariant, Justify, Layout, LayoutDirection, Length, LocKey, Overflow,
    Padding, Presentation, PresentationMode, ScreenDef, ScreenKind, Tags, TextAlign, TextOpts,
    TextRole, Transition, UiNodeDef, WidgetKind,
};
use slotted::ui::{ScreenStack, pop_screen, push_screen, replace_screen};

pub mod loadout;

/// The screen kind.
pub const KIND: &str = "strata:sheet";

/// Opens the sheet with nothing else open, and closes it when it is on
/// top. Escape closes it too, through the stack.
pub const KEY: KeyCode = KeyCode::KeyC;

/// The `menu` tag of the Learn button.
pub const LEARN: &str = "sheet.learn";
/// The `menu` tag of the Forget button.
pub const FORGET: &str = "sheet.forget";
/// The `test_id` of the status line under the description.
pub const STATUS_ID: &str = "sheet.status";
/// The `test_id` of the notice line under the buttons.
pub const NOTICE_ID: &str = "sheet.notice";
/// The `test_id` of the points line in the header.
pub const POINTS_ID: &str = "sheet.points";
/// The `test_id` of the panel the whole screen is.
pub const ROOT_ID: &str = "sheet.root";
/// The `test_id` of the region the box's details scroll in.
pub const DETAILS_SCROLL_ID: &str = "sheet.details";
/// The screen's one height, as a share of the window's: 96 so that 1080p at
/// its automatic UI scale shows the tallest tree whole, and the same for
/// every tree and box, so nothing outside a scrolling region moves.
const SCREEN_HEIGHT_VH: f32 = 96.;
/// The `test_id` of the scrolling body between the header and the footer.
pub const BODY_ID: &str = "sheet.body";
/// The `test_id` of the scroll the open tree is drawn in.
pub const TREE_ID: &str = "sheet.tree";
/// The `test_id` of the Close button.
pub const CLOSE_ID: &str = "sheet.close";
/// The `menu` tag of the Loadout tab.
pub const LOADOUT_TAB: &str = "sheet.loadout";
/// The `menu` tag of the Professions tab.
pub const PROFESSIONS_TAB: &str = "sheet.professions";
/// Opens the sheet on its Loadout tab, and closes it when it is on top. K
/// would do as well; L is free and sits beside the sheet's C on the
/// controls card.
pub const LOADOUT_KEY: KeyCode = KeyCode::KeyL;
/// The `menu` tag of the disclosure that lists the placeholder trees.
pub const MORE: &str = "sheet.more";

/// The `menu` tag prefix of a tree tab: `sheet.tree:blade`.
const TREE_TAG: &str = "sheet.tree:";
/// The `menu` tag prefix of a box card: `sheet.box:blade.cuts.1`.
const BOX_TAG: &str = "sheet.box:";

/// The screen kind.
#[must_use]
pub fn kind() -> ScreenKind {
    ScreenKind::new(KIND)
}

/// What the screen shows beyond the sheet itself: which tree and box are
/// picked, and what the last button press answered.
#[derive(Resource, Debug, Default, Clone, PartialEq, Eq)]
pub struct SheetState {
    /// The tree shown; `None` picks the one most points went into.
    pub tree: Option<String>,
    /// The box the details panel describes; `None` picks the next box the
    /// sheet has not learned.
    pub selected: Option<String>,
    /// What the last Learn or Forget said.
    pub notice: Option<String>,
    /// The placeholder trees are listed in the tabs column; folded by
    /// default, since eleven of thirteen trees are not in the game yet.
    pub show_placeholders: bool,
    /// The Loadout tab is shown rather than a tree.
    pub loadout: bool,
    /// A rebuild is due.
    dirty: bool,
}

/// A box as the card shows it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BoxState {
    /// On the sheet.
    Learned,
    /// Every rule passes and a trainer of the tree is within reach.
    Learnable,
    /// Every rule but the trainer passes.
    NeedsTrainer,
    /// Short of the XP gate.
    NeedsXp,
    /// The box below, or an elite's named boxes, are not learned.
    NeedsPrerequisite,
    /// Not enough free points.
    NoPoints,
    /// A box of a placeholder tree: nothing earns its XP and no trainer
    /// teaches it, so no other state could ever come true.
    NotYetInGame,
}

impl BoxState {
    /// The sim's answer for `id`, as a card state; a placeholder tree's
    /// box is [`BoxState::NotYetInGame`] whatever the rules would say.
    #[must_use]
    pub fn of(graph: &SkillGraph, sheet: &CharacterSheet, id: &str, teacher: bool) -> Self {
        if graph.tree_of(id).is_some_and(|t| !t.in_game()) {
            return Self::NotYetInGame;
        }
        if sheet.has(id) {
            return Self::Learned;
        }
        match sheet.can_learn(graph, id, teacher) {
            Ok(()) => Self::Learnable,
            Err(LearnRefused::NoTeacher) => Self::NeedsTrainer,
            Err(LearnRefused::NotEnoughXp { .. }) => Self::NeedsXp,
            Err(LearnRefused::NotEnoughPoints { .. }) => Self::NoPoints,
            Err(
                LearnRefused::MissingPrerequisite(_)
                | LearnRefused::UnknownBox(_)
                | LearnRefused::AlreadyLearned(_),
            ) => Self::NeedsPrerequisite,
        }
    }

    /// The locale key of the word on the card.
    const fn key(self) -> &'static str {
        match self {
            Self::Learned => "strata.sheet.state.learned",
            Self::Learnable => "strata.sheet.state.learnable",
            Self::NeedsTrainer => "strata.sheet.state.trainer",
            Self::NeedsXp => "strata.sheet.state.xp",
            Self::NeedsPrerequisite => "strata.sheet.state.locked",
            Self::NoPoints => "strata.sheet.state.no_points",
            Self::NotYetInGame => "strata.sheet.state.not_yet",
        }
    }

    /// The word on the card, at most two of them: twelve cards repeating
    /// one sentence read as a wall, so the card names the state and the
    /// details panel says the sentence. [`BoxState::NeedsXp`]'s is a
    /// pattern [`BoxState::words`] fills.
    #[must_use]
    pub fn word(self) -> &'static str {
        english(self.key())
    }

    /// The word on the card with the XP filled in: what the tree's kind
    /// has against what the box asks, since how far off it is is the one
    /// thing a waiting card can say in a figure.
    #[must_use]
    pub fn words(self, has: u32, needs: u32) -> String {
        self.word()
            .replace("{has}", &has.to_string())
            .replace("{needs}", &needs.to_string())
    }

    /// The text role the word is painted in: the accent for what is
    /// learned, the warm colour for what can be learned right here, the
    /// condensed figure face for an XP count and a quiet tone for the
    /// rest.
    const fn role(self) -> &'static str {
        match self {
            Self::Learned => "strata.sheet.state.learned",
            Self::Learnable => "strata.sheet.state.learnable",
            Self::NeedsXp => "strata.sheet.state.xp",
            _ => "strata.sheet.state.waiting",
        }
    }

    /// The text role of the status sentence in the details panel, which
    /// keeps the theme's long-standing colours for a sentence.
    const fn sentence_role(self) -> &'static str {
        match self {
            Self::Learned => "text.key",
            Self::Learnable => "text.icon",
            _ => "text.caption",
        }
    }

    /// The stripe down the card's left edge: the state read before a
    /// word is, from across the tree.
    const fn stripe(self) -> &'static str {
        match self {
            Self::Learned => "strata.sheet.stripe.learned",
            Self::Learnable => "strata.sheet.stripe.taught",
            Self::NotYetInGame => "strata.sheet.stripe.later",
            _ => "strata.sheet.stripe.locked",
        }
    }

    /// The tone of the name: lit for what is had or can be had here, one
    /// step down for what waits, faint for what the game cannot teach.
    const fn name_role(self) -> &'static str {
        match self {
            Self::Learned | Self::Learnable => "strata.sheet.name.lit",
            Self::NotYetInGame => "strata.sheet.name.later",
            _ => "strata.sheet.name.dim",
        }
    }

    /// The card's role family, which the card paints through hover,
    /// focus and press as a button does. One coloured border at a time:
    /// the selection ring wins, then the warm border of a box taught here,
    /// then the faint accent of a learned one, so learned boxes read as a
    /// lit path without ever doubling up with another ring.
    const fn frame(self, selected: bool) -> &'static str {
        if selected {
            return "strata.sheet.card.selected";
        }
        match self {
            Self::Learnable => "strata.sheet.card.taught",
            Self::Learned => "strata.sheet.card.learned",
            _ => "strata.sheet.card",
        }
    }
}

/// The English for one of the sheet's keys, or the key itself.
fn english(key: &'static str) -> &'static str {
    super::strings::english(key).unwrap_or(key)
}

/// A trainer the world has, as the sheet names it: which trees, how far
/// and which way from the player.
#[derive(Debug, Clone, PartialEq)]
pub struct TrainerSpot {
    /// Tree ids.
    pub trees: Vec<String>,
    /// Horizontal distance in metres.
    pub distance: f64,
    /// One of eight compass words.
    pub bearing: &'static str,
}

impl TrainerSpot {
    /// From a prop and the player, by the sim's compass: north along `+z`
    /// and east along `+x`, as the map draws them and as the survey
    /// register on the same terminal words a deposit's bearing.
    #[must_use]
    pub fn from_positions(
        trees: Vec<String>,
        prop: &WorldPosition,
        player: &WorldPosition,
    ) -> Self {
        let dx = prop.0.x() - player.0.x();
        let dz = prop.0.z() - player.0.z();
        let distance = dx.hypot(dz);
        let bearing = super::bench::compass_words(veyra_sim::deposits::Compass::of(
            [player.0.x(), player.0.z()],
            [prop.0.x(), prop.0.z()],
        ));
        Self {
            trees,
            distance,
            bearing,
        }
    }
}

/// Everything the definition is built from.
#[derive(Debug, Clone)]
pub struct SheetView<'a> {
    /// The graph.
    pub graph: &'a SkillGraph,
    /// The local player's sheet.
    pub sheet: &'a CharacterSheet,
    /// Their name.
    pub name: &'a str,
    /// Their body tint, so the portrait says whose sheet this is the way
    /// the bench's cards do.
    pub tint: u8,
    /// What is within reach.
    pub reach: &'a Reach,
    /// Whether a forget would be refused for fighting.
    pub in_combat: bool,
    /// Every trainer in the world, for the "where" line.
    pub trainers: &'a [TrainerSpot],
    /// The picks and the notice.
    pub state: &'a SheetState,
    /// For each box that teaches a schematic, the box's id and the name of
    /// what the schematic makes, in the schematics' order.
    pub teaches: &'a [(String, String)],
    /// The Loadout tab's view, when that tab is shown.
    pub loadout: Option<&'a loadout::LoadoutView>,
}

impl SheetView<'_> {
    /// The tree shown: the state's, else the one most points went into,
    /// the file's order breaking ties.
    #[must_use]
    pub fn tree(&self) -> Option<&TreeDef> {
        if let Some(id) = &self.state.tree
            && let Some(tree) = self.graph.tree(id)
        {
            return Some(tree);
        }
        self.graph.trees.iter().rev().max_by_key(|t| {
            t.boxes()
                .filter(|b| self.sheet.has(&b.id))
                .map(|b| u32::from(b.cost))
                .sum::<u32>()
        })
    }

    /// The box described: the state's when it is of the shown tree, else
    /// the first box of the tree the sheet has not learned, else the
    /// novice.
    #[must_use]
    pub fn selected(&self) -> Option<&BoxDef> {
        let tree = self.tree()?;
        if let Some(id) = &self.state.selected
            && let Some(b) = tree.boxes().find(|b| &b.id == id)
        {
            return Some(b);
        }
        tree.boxes()
            .find(|b| !self.sheet.has(&b.id))
            .or(Some(&tree.novice))
    }

    fn state_of(&self, id: &str) -> BoxState {
        BoxState::of(
            self.graph,
            self.sheet,
            id,
            self.reach.teaches(self.graph, id),
        )
    }

    /// Whether the Learn button is live: the sim says yes, trainer
    /// included.
    #[must_use]
    pub fn can_learn_selected(&self) -> bool {
        self.selected()
            .is_some_and(|b| self.state_of(&b.id) == BoxState::Learnable)
    }

    /// Why Forget would be refused, or `None` when it would not.
    fn forget_refusal(&self, id: &str) -> Option<ForgetRefused> {
        self.sheet
            .clone()
            .forget(self.graph, id, self.in_combat)
            .err()
    }
}

// The node constructors: the definition is long, and this keeps every
// line of it about what it shows rather than which fields a variant has.

fn key(s: impl Into<String>) -> LocKey {
    LocKey(s.into())
}

fn text(s: impl Into<String>, style: TextRole) -> UiNodeDef {
    UiNodeDef::Text {
        key: key(s),
        style,
        opts: TextOpts::default(),
        tags: Tags::new(),
    }
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

fn with_id(mut node: UiNodeDef, id: &str) -> UiNodeDef {
    if let UiNodeDef::Text { tags, .. } | UiNodeDef::Panel { tags, .. } = &mut node {
        *tags = tags.clone().with(Tags::TEST_ID, id);
    }
    node
}

fn centred(mut node: UiNodeDef) -> UiNodeDef {
    if let UiNodeDef::Text { opts, .. } = &mut node {
        opts.align = TextAlign::Center;
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

/// Empty space that takes what is left: the whole of the parent is how
/// the spacer widget spells "grow".
fn spacer() -> UiNodeDef {
    UiNodeDef::Spacer {
        size: Length::Percent(100.),
        tags: Tags::new(),
    }
}

fn separator() -> UiNodeDef {
    UiNodeDef::Separator {
        direction: LayoutDirection::Row,
        tags: Tags::new(),
    }
}

/// A button whose `menu` tag is `action`, which is also its `test_id`. A
/// disabled one is painted as the plain variant: the theme has a
/// `button.disabled` and no `button.primary.disabled`, so a disabled
/// primary would keep its accent and read as live.
fn button(label: &str, action: &str, variant: ButtonVariant, disabled: bool) -> UiNodeDef {
    UiNodeDef::Button {
        widget: None,
        opts: ButtonOpts {
            label: Some(key(label)),
            variant: if disabled {
                ButtonVariant::Secondary
            } else {
                variant
            },
            disabled,
            ..ButtonOpts::default()
        },
        tags: Tags::new().with("menu", action).with(Tags::TEST_ID, action),
    }
}

fn close_button() -> UiNodeDef {
    UiNodeDef::Button {
        widget: Some(WidgetKind::new("slotted:close")),
        opts: ButtonOpts {
            label: Some(key("strata.menu.close")),
            ..ButtonOpts::default()
        },
        tags: Tags::new().with(Tags::TEST_ID, CLOSE_ID),
    }
}

fn hint_bar() -> UiNodeDef {
    UiNodeDef::Custom {
        kind: WidgetKind::new("slotted:hint_bar"),
        params: slotted::registry::Value::Unit,
        children: Vec::new(),
        tags: Tags::new().with(Tags::TEST_ID, "sheet.hints"),
    }
}

/// A number with no more decimals than it needs.
fn num(v: f32) -> String {
    if (v - v.round()).abs() < 1e-3 {
        format!("{v:.0}")
    } else {
        let s = format!("{v:.2}");
        s.trim_end_matches('0').trim_end_matches('.').to_owned()
    }
}

fn capitalised(s: &str) -> String {
    let mut chars = s.chars();
    chars.next().map_or_else(String::new, |first| {
        first.to_uppercase().collect::<String>() + chars.as_str()
    })
}

/// An action in the words a player uses.
pub(crate) fn action_words(name: &str) -> String {
    match Action::from_name(name) {
        Some(Action::Light) => "light cut",
        Some(Action::Followup) => "follow-up cut",
        Some(Action::Heavy) => "heavy cut",
        Some(Action::Dodge) => "dodge",
        Some(Action::Reload) => "reload",
        Some(Action::Stagger) => "stagger",
        Some(Action::Swap) => "weapon swap",
        Some(Action::Ready) => "ready stance",
        Some(Action::Dead) => "death",
        Some(Action::Ability) => "ability",
        None => name,
    }
    .to_owned()
}

fn weapon_words(name: &str) -> String {
    match Weapon::from_name(name) {
        Some(weapon) => {
            super::strings::english(&format!("strata.family.{}", weapon.name().to_lowercase()))
                .unwrap_or(name)
        }
        None => name,
    }
    .to_owned()
}

/// A field of an action row: its words and the unit its values carry.
fn action_field(spec: &ActionSpec, field: &str) -> Option<(&'static str, f32, &'static str)> {
    let mut copy = *spec;
    let value = *copy.field_mut(field)?;
    Some(match field {
        "contact_start" => ("contact opens at", value, " s"),
        "contact_end" => ("contact closes at", value, " s"),
        "duration" => ("recovery", value, " s"),
        "dodge_cancel" => ("dodge cancel at", value, " s"),
        "stamina" => ("stamina cost", value, ""),
        "damage" => ("damage", value, ""),
        "posture" => ("posture damage", value, ""),
        _ => ("travel", value, " m"),
    })
}

fn weapon_field(spec: &WeaponSpec, field: &str) -> Option<(&'static str, f32, &'static str)> {
    Some(match field {
        "interval" => (
            if matches!(spec.mode, FireMode::Burst { .. }) {
                "burst pause"
            } else {
                "time between shots"
            },
            spec.interval * 1000.,
            " ms",
        ),
        "bolt_speed" => ("bolt speed", spec.bolt_speed, " m/s"),
        "damage" => ("damage", spec.damage, ""),
        "posture" => ("posture damage", spec.posture, ""),
        "magazine" => ("magazine", f32::from(spec.magazine), " rounds"),
        "reload" => ("reload", spec.reload, " s"),
        "range" => ("range", spec.range, " m"),
        "aim_time" => ("aim time", spec.aim_time, " s"),
        _ => return None,
    })
}

fn actor_field(
    table: &veyra_sim::combat_tables::ActorTable,
    field: &str,
) -> Option<(&'static str, f32, &'static str)> {
    let mut copy = table.clone();
    let value = *copy.field_mut(field)?;
    Some(match field {
        "health" => ("health", value, ""),
        "shield" => ("shield", value, ""),
        "stamina" => ("stamina", value, ""),
        "posture" => ("posture", value, ""),
        "stamina_regen" => ("stamina regeneration", value, " a second"),
        "shield_regen" => ("shield regeneration", value, " a second"),
        "posture_regen" => ("posture regeneration", value, " a second"),
        "recovery_wait" => ("recovery wait", value, " s"),
        "hurt_wait" => ("hurt wait", value, " s"),
        "buffer" => ("input buffer", value, " s"),
        "combo_window" => ("combo window", value, " s"),
        "aim_speed_scale" => ("aimed movement", value * 100., "%"),
        "walk_speed" => ("walk", value, " m/s"),
        "sprint_speed" => ("sprint", value, " m/s"),
        "jump_speed" => ("jump", value, " m/s"),
        _ => ("gravity", value, " m/s²"),
    })
}

/// One effect line in its parts, so the details panel can set the words
/// in the prose face and the figures in the numeral face.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EffectLine {
    /// What changes, capitalised: `Light cut dodge cancel at`.
    pub words: String,
    /// The figure before the box, when the box moves it.
    pub from: Option<String>,
    /// The figure with the box; `None` for an unlock or a certification.
    pub to: Option<String>,
}

impl EffectLine {
    fn words(words: String) -> Self {
        Self {
            words,
            from: None,
            to: None,
        }
    }

    /// The line as one sentence: `Light cut dodge cancel at 0.3 s to 0.26 s`.
    #[must_use]
    pub fn sentence(&self) -> String {
        match (&self.from, &self.to) {
            (Some(from), Some(to)) => format!("{} {from} to {to}", self.words),
            (None, Some(to)) => format!("{} {to}", self.words),
            _ => self.words.clone(),
        }
    }
}

fn change_line(subject: &str, before: (&str, f32, &str), after: f32) -> EffectLine {
    let (words, from, unit) = before;
    EffectLine {
        words: capitalised(format!("{subject} {words}").trim()),
        from: ((from - after).abs() >= 1e-4).then(|| format!("{}{unit}", num(from))),
        to: Some(format!("{}{unit}", num(after))),
    }
}

/// Each effect of `b` in plain words, with the number it moves from and
/// to on this sheet: the rows with `b` taken off the sheet against the
/// rows with it on, so a learned box reads the same as one to learn.
#[must_use]
pub fn effect_lines(
    graph: &SkillGraph,
    tables: &CombatTables,
    sheet: &CharacterSheet,
    b: &BoxDef,
) -> Vec<String> {
    effect_parts(graph, tables, sheet, b)
        .iter()
        .map(EffectLine::sentence)
        .collect()
}

/// [`effect_lines`] in parts.
#[must_use]
pub fn effect_parts(
    graph: &SkillGraph,
    tables: &CombatTables,
    sheet: &CharacterSheet,
    b: &BoxDef,
) -> Vec<EffectLine> {
    let mut without = sheet.clone();
    without.learned.remove(&b.id);
    let mut with = sheet.clone();
    with.learned.insert(b.id.clone());
    let before: EffectiveTables = apply(tables, graph, &without);
    let after: EffectiveTables = apply(tables, graph, &with);
    b.effects
        .iter()
        .filter_map(|effect| match effect {
            Effect::Unlock { action } => Some(EffectLine::words(capitalised(&format!(
                "{} unlocked",
                action_words(action)
            )))),
            Effect::Certify { weapon } => Some(EffectLine::words(format!(
                "Certified on the {}",
                weapon_words(weapon)
            ))),
            Effect::CraftPoints { add } => Some(EffectLine::words(format!(
                "+{add} experimentation point in a session"
            ))),
            Effect::Action { action, field, .. } => {
                let a = Action::from_name(action)?;
                let from = action_field(&before.spec(a), field)?;
                let to = action_field(&after.spec(a), field)?.1;
                Some(change_line(&action_words(action), from, to))
            }
            Effect::Weapon { weapon, field, .. } => {
                let w = Weapon::from_name(weapon)?;
                let from = weapon_field(&before.weapon_spec(w), field)?;
                let to = weapon_field(&after.weapon_spec(w), field)?.1;
                Some(change_line(&weapon_words(weapon), from, to))
            }
            Effect::Actor { field, .. } => {
                let from = actor_field(&before.actor, field)?;
                let to = actor_field(&after.actor, field)?.1;
                Some(change_line("", from, to))
            }
            Effect::Survey { field, op, value } => {
                survey_field_words(field, *op, *value).map(EffectLine::words)
            }
            Effect::Read { reading } => Some(EffectLine::words(
                english(match reading {
                    Reading::Type => "strata.sheet.effect.read.type",
                    Reading::Bands => "strata.sheet.effect.read.bands",
                    Reading::Exact => "strata.sheet.effect.read.exact",
                })
                .to_owned(),
            )),
            Effect::Peak => Some(EffectLine::words(
                english("strata.sheet.effect.peak").to_owned(),
            )),
            Effect::Ride { field, op, value } => {
                ride_field_words(field, *op, *value).map(EffectLine::words)
            }
            Effect::Craft { field, op, value } => {
                craft_field_words(field, *op, *value).map(EffectLine::words)
            }
            Effect::Factory { field, op, value } => {
                factory_field_words(field, *op, *value).map(EffectLine::words)
            }
            Effect::Wear { field, op, value } => {
                wear_field_words(field, *op, *value).map(EffectLine::words)
            }
            Effect::Station { field, op, value } => {
                station_field_words(field, *op, *value).map(EffectLine::words)
            }
            Effect::Grant { ability } => Some(EffectLine::words(
                english("strata.sheet.effect.grant").replace("{ability}", &ability_name(ability)),
            )),
            Effect::Ability {
                id,
                field,
                op,
                value,
            } => ability_field_words(id, field, *op, *value).map(EffectLine::words),
            Effect::Guard { field, op, value } => {
                guard_field_words(field, *op, *value).map(EffectLine::words)
            }
        })
        .collect()
}

/// An ability's name as its row words it, the id when the table has none.
fn ability_name(id: &str) -> String {
    combat_tables::current()
        .abilities
        .row(id)
        .map_or_else(|| id.to_owned(), |row| row.name.clone())
}

/// An ability figure's box line: `Precision shot stamina x0.8`.
fn ability_field_words(id: &str, field: &str, op: Op, value: f32) -> Option<String> {
    let (key, unit) = match field {
        "stamina" => ("strata.sheet.effect.ability.stamina", ""),
        "cooldown" => ("strata.sheet.effect.ability.cooldown", " s"),
        "windup" => ("strata.sheet.effect.ability.windup", " s"),
        "length" => ("strata.sheet.effect.ability.length", " s"),
        "damage" => ("strata.sheet.effect.ability.damage", ""),
        "posture" => ("strata.sheet.effect.ability.posture", ""),
        "seconds" => ("strata.sheet.effect.ability.seconds", " s"),
        _ => return None,
    };
    Some(
        english(key)
            .replace("{ability}", &ability_name(id))
            .replace("{change}", &op_words(op, value, unit)),
    )
}

/// A guard figure's box line: `Guard parry window +0.03 s`.
fn guard_field_words(field: &str, op: Op, value: f32) -> Option<String> {
    let (key, unit) = match field {
        "share" => ("strata.sheet.effect.guard.share", ""),
        "arc" => ("strata.sheet.effect.guard.arc", " deg"),
        "stamina" => ("strata.sheet.effect.guard.stamina", ""),
        "posture" => ("strata.sheet.effect.guard.posture", ""),
        "parry" => ("strata.sheet.effect.guard.parry", " s"),
        "riposte" => ("strata.sheet.effect.guard.riposte", ""),
        _ => return None,
    };
    // The share is a fraction in the file and a percent everywhere else a
    // screen shows it (the card, the fitting screen); a Mul is a factor.
    let (value, unit) = if field == "share" && op != Op::Mul {
        (value * 100., "%")
    } else {
        (value, unit)
    };
    Some(english(key).replace("{change}", &op_words(op, value, unit)))
}

/// What an op does to a figure, in the words a box states it in: `+64 m`,
/// `x0.8`, or the figure itself for a Set.
fn op_words(op: Op, value: f32, unit: &str) -> String {
    match op {
        Op::Set => format!("{}{unit}", num(value)),
        Op::Add if value < 0. => format!("-{}{unit}", num(-value)),
        Op::Add => format!("+{}{unit}", num(value)),
        Op::Mul => format!("x{}", num(value)),
    }
}

/// A survey figure's box line: `Ping reach +64 m`.
fn survey_field_words(field: &str, op: Op, value: f32) -> Option<String> {
    let (key, unit) = match field {
        "range_m" => ("strata.sheet.effect.survey.range_m", " m"),
        "grid" => ("strata.sheet.effect.survey.grid", ""),
        "channel_s" => ("strata.sheet.effect.survey.channel_s", " s"),
        "units" => ("strata.sheet.effect.survey.units", ""),
        "cycle_s" => ("strata.sheet.effect.survey.cycle_s", " s"),
        _ => return None,
    };
    Some(english(key).replace("{change}", &op_words(op, value, unit)))
}

/// A field harvester figure's box line: `Field harvester rate x1.25`.
fn station_field_words(field: &str, op: Op, value: f32) -> Option<String> {
    let key = match field {
        "count" => "strata.sheet.effect.station.count",
        "rate" => "strata.sheet.effect.station.rate",
        "hopper" => "strata.sheet.effect.station.hopper",
        _ => return None,
    };
    Some(english(key).replace("{change}", &op_words(op, value, "")))
}

/// A bike figure's box line: `Bike: call time x0.6`.
fn ride_field_words(field: &str, op: Op, value: f32) -> Option<String> {
    let (key, unit) = match field {
        "call_s" => ("strata.sheet.effect.ride.call_s", " s"),
        "speed_road" => ("strata.sheet.effect.ride.speed_road", " m/s"),
        "speed_ground" => ("strata.sheet.effect.ride.speed_ground", " m/s"),
        "stop_deg" => ("strata.sheet.effect.ride.stop_deg", " degrees"),
        "grip" => ("strata.sheet.effect.ride.grip", ""),
        _ => return None,
    };
    Some(english(key).replace("{change}", &op_words(op, value, unit)))
}

/// A share written as points of a hundred: `+2 %`, `x1.25`, or the figure
/// itself for a Set.
fn share_words(op: Op, value: f32) -> String {
    match op {
        Op::Mul => op_words(op, value, ""),
        Op::Set | Op::Add => op_words(op, value * 100., " %"),
    }
}

/// A factor as the percentage it moves a figure by, with whether that is
/// a rise: `x0.9` is `(10, false)`. None for any other op.
fn factor_move(op: Op, value: f32) -> Option<(String, bool)> {
    (op == Op::Mul).then(|| {
        (
            format!("{} %", num(((value - 1.) * 100.).abs())),
            value > 1.,
        )
    })
}

/// A bench figure's box line: `Bench: assembly +2 %`.
fn craft_field_words(field: &str, op: Op, value: f32) -> Option<String> {
    let key = match field {
        "assembly" => "strata.sheet.effect.craft.assembly",
        _ => return None,
    };
    Some(english(key).replace("{change}", &share_words(op, value)))
}

/// A fabricator figure's box line: `Fabricator: a unit in 10 % less time`.
fn factory_field_words(field: &str, op: Op, value: f32) -> Option<String> {
    let line = |key: &'static str, change: &str| english(key).replace("{change}", change);
    Some(match (field, factor_move(op, value)) {
        ("seconds", Some((change, more))) => line(
            if more {
                "strata.sheet.effect.factory.seconds_more"
            } else {
                "strata.sheet.effect.factory.seconds_less"
            },
            &change,
        ),
        ("seconds", None) => line(
            "strata.sheet.effect.factory.seconds",
            &op_words(op, value, " s"),
        ),
        ("runs", _) => line("strata.sheet.effect.factory.runs", &op_words(op, value, "")),
        ("unattended", _) => line(
            "strata.sheet.effect.factory.unattended",
            &share_words(op, value),
        ),
        _ => return None,
    })
}

/// A repair or wear box line: `Field repair restores 20 % more`.
fn wear_field_words(field: &str, op: Op, value: f32) -> Option<String> {
    let line = |key: &'static str, change: &str| english(key).replace("{change}", change);
    let moved = |more: &'static str, less: &'static str, plain: &'static str| {
        factor_move(op, value).map_or_else(
            || line(plain, &share_words(op, value)),
            |(change, up)| line(if up { more } else { less }, &change),
        )
    };
    Some(match field {
        "field" => moved(
            "strata.sheet.effect.wear.field_more",
            "strata.sheet.effect.wear.field_less",
            "strata.sheet.effect.wear.field",
        ),
        "cost" => moved(
            "strata.sheet.effect.wear.cost_more",
            "strata.sheet.effect.wear.cost_less",
            "strata.sheet.effect.wear.cost",
        ),
        "bench_kit" => english("strata.sheet.effect.wear.bench_kit").to_owned(),
        _ => return None,
    })
}

/// A box's place in its tree: `Novice`, `Cuts, step 2 of 4`, `Master`.
fn place_words(tree: &TreeDef, id: &str) -> String {
    if tree.novice.id == id {
        return "Novice box".to_owned();
    }
    if tree.master.id == id {
        return "Master box".to_owned();
    }
    tree.branches
        .iter()
        .find_map(|branch| {
            branch
                .boxes
                .iter()
                .position(|b| b.id == id)
                .map(|i| format!("{}, step {} of {}", branch.name, i + 1, branch.boxes.len()))
        })
        .unwrap_or_default()
}

/// The status line under the description: the state in a sentence, with
/// the number or the box or the trainer it waits on.
fn status_words(view: &SheetView<'_>, tree: &TreeDef, b: &BoxDef) -> (String, &'static str) {
    let graph = view.graph;
    let state = view.state_of(&b.id);
    let role = state.sentence_role();
    let words = match state {
        BoxState::NotYetInGame => tree
            .note
            .clone()
            .unwrap_or_else(|| "Not yet in the game.".to_owned()),
        BoxState::Learned => match view.forget_refusal(&b.id) {
            None => format!("Learned. Forgetting it returns {} points.", b.cost),
            Some(ForgetRefused::Dependent(d)) => format!(
                "Learned. {} still needs it.",
                graph.find(&d).map_or(d.clone(), |d| d.name.clone())
            ),
            Some(ForgetRefused::InCombat) => "Learned. Not while fighting.".to_owned(),
            Some(ForgetRefused::NotLearned(_)) => "Learned.".to_owned(),
        },
        BoxState::Learnable if tree.bench => english(if view.reach.workbench {
            "strata.sheet.learn.bench"
        } else {
            "strata.sheet.learn.foreman"
        })
        .replace("{points}", &b.cost.to_string()),
        BoxState::Learnable => format!(
            "The {} trainer here teaches it for {} points.",
            tree.name.to_lowercase(),
            b.cost
        ),
        BoxState::NeedsTrainer if tree.bench => english("strata.sheet.need.bench").to_owned(),
        BoxState::NeedsTrainer => {
            let name = tree.name.to_lowercase();
            let nearest = view
                .trainers
                .iter()
                .filter(|t| t.trees.contains(&tree.id))
                .min_by(|a, b| a.distance.total_cmp(&b.distance));
            match nearest {
                Some(t) => format!(
                    "Needs the {name} trainer, {:.0} m to the {}.",
                    t.distance, t.bearing
                ),
                None => format!("Needs a {name} trainer; none is known nearby."),
            }
        }
        BoxState::NeedsXp => format!(
            "Needs {} {} XP; has {}.",
            b.xp,
            tree.xp,
            view.sheet.xp(tree.xp)
        ),
        BoxState::NeedsPrerequisite => {
            let missing = graph
                .prerequisites(&b.id)
                .into_iter()
                .find(|p| !view.sheet.has(p))
                .and_then(|p| graph.find(p))
                .map_or("the box before".to_owned(), |p| p.name.clone());
            format!("Needs {missing} first.")
        }
        BoxState::NoPoints => format!(
            "Costs {} points; {} free.",
            b.cost,
            view.sheet.points_free(graph)
        ),
    };
    (words, role)
}

/// A box card's width.
const CARD: f32 = 150.;
/// The room around a card in its branch's column, so the branches sit
/// apart and the rails can run from one column's centre to another's.
const LANE: f32 = CARD + 10.;
/// The connectors' thickness and their length between two cards.
const LINK: f32 = 2.;
const TICK: f32 = 8.;

/// Whether `b` is on the sheet, which is what lights a connector.
fn learned(view: &SheetView<'_>, b: &BoxDef) -> bool {
    view.sheet.has(&b.id)
}

/// A fixed-size block of one role: a stripe, a swatch, a tick, a rail.
fn block(role: &str, width: f32, height: f32) -> UiNodeDef {
    panel(
        role,
        Layout {
            width: Some(Length::Px(width)),
            height: Some(Length::Px(height)),
            ..column(0.)
        },
        Vec::new(),
    )
}

/// A vertical connector, lit when the box below it is learned.
fn tick(lit: bool) -> UiNodeDef {
    block(link_role(lit), LINK, TICK)
}

const fn link_role(lit: bool) -> &'static str {
    if lit {
        "strata.sheet.link.lit"
    } else {
        "strata.sheet.link"
    }
}

/// The ability a box grants, as its row names it: name, icon and the line.
fn granted(b: &BoxDef) -> Option<(String, String, String)> {
    let tables = combat_tables::current();
    b.effects.iter().find_map(|effect| match effect {
        Effect::Grant { ability } => tables
            .abilities
            .row(ability)
            .map(|row| (row.name.clone(), row.icon.clone(), row.says.clone())),
        _ => None,
    })
}

/// The size of a granted ability's square beside a box's name.
const GRANT_SIZE: f32 = 32.;

/// One box card: the state stripe down the left edge, the name, and under
/// it the cost at the left and the state at the right in a word or a
/// figure. The whole card is the button (focus, hover and the click that
/// selects it), so it carries the `menu` and `test_id` tags and the parts
/// inside it are only its face.
fn box_card(view: &SheetView<'_>, b: &BoxDef, selected: bool) -> UiNodeDef {
    let state = view.state_of(&b.id);
    let mut name = text_in(b.name.clone(), TextRole::Body, state.name_role());
    if let Some((_, icon, _)) = granted(b) {
        name = panel(
            "invisible",
            row(0.6),
            vec![
                icons::node(
                    &icon,
                    GRANT_SIZE,
                    None,
                    false,
                    Some(&format!("grant:{}", b.id)),
                ),
                name,
            ],
        );
    }
    let has = view
        .graph
        .tree_of(&b.id)
        .map_or(0, |tree| view.sheet.xp(tree.xp));
    let mut word = Vec::new();
    if state == BoxState::Learned {
        // A square rather than a check glyph: the theme's fonts may not
        // carry one, and a square in the stripe's colour says the same.
        word.push(block("strata.sheet.stripe.learned", 8., 8.));
    }
    word.push(text_in(
        state.words(has, b.xp),
        TextRole::Caption,
        state.role(),
    ));
    let under = panel(
        "invisible",
        Layout {
            justify: Justify::SpaceBetween,
            ..row(1.)
        },
        vec![
            text_in(
                english("strata.sheet.cost").replace("{cost}", &b.cost.to_string()),
                TextRole::Caption,
                "strata.sheet.cost",
            ),
            panel("invisible", row(0.6), word),
        ],
    );
    let body = panel(
        "invisible",
        Layout {
            grow: 1.,
            align: Some(Align::Stretch),
            ..column(0.3)
        },
        vec![name, under],
    );
    UiNodeDef::Card {
        role: Role::new(state.frame(selected)),
        layout: Layout {
            width: Some(Length::Px(CARD)),
            padding: Padding {
                top: 0.7,
                right: 1.3,
                bottom: 0.7,
                left: 1.,
            },
            align: Some(Align::Stretch),
            ..row(1.)
        },
        children: vec![
            panel(
                state.stripe(),
                Layout {
                    width: Some(Length::Px(4.)),
                    ..column(0.)
                },
                Vec::new(),
            ),
            body,
        ],
        disabled: false,
        tags: Tags::new()
            .with("menu", &format!("{BOX_TAG}{}", b.id))
            .with(Tags::TEST_ID, &format!("box:{}", b.id)),
    }
}

/// The tree's head: its name, how much of it is learned as a bar and a
/// count, and the XP its kind has.
fn tree_head(view: &SheetView<'_>, tree: &TreeDef) -> UiNodeDef {
    const BAR: f32 = 260.;
    let learned = tree.boxes().filter(|b| view.sheet.has(&b.id)).count();
    let total = tree.boxes().count();
    // Two blocks side by side rather than a fill inside a track: a
    // panel's fill is painted inside its 1 px border, which would leave
    // a 4 px bar 2 px of colour.
    #[allow(
        clippy::cast_precision_loss,
        reason = "a tree has a dozen boxes, far inside f32's whole numbers"
    )]
    let lit = if total == 0 {
        0.
    } else {
        (BAR * learned as f32 / total as f32).round()
    };
    let mut bar = Vec::new();
    if lit > 0. {
        bar.push(block("strata.sheet.progress.fill", lit, 4.));
    }
    if lit < BAR {
        bar.push(block("strata.sheet.progress", BAR - lit, 4.));
    }
    let progress = panel(
        "invisible",
        row(1.5),
        vec![
            panel("invisible", row(0.), bar),
            text_in(
                english("strata.sheet.progress")
                    .replace("{learned}", &learned.to_string())
                    .replace("{total}", &total.to_string()),
                TextRole::Caption,
                "strata.sheet.progress.text",
            ),
        ],
    );
    // The bar and the XP share a line, so the tree under them has the
    // height a 720p pane leaves.
    let mut head = vec![
        text(tree.name.clone(), TextRole::Heading),
        panel(
            "invisible",
            row(3.),
            vec![
                progress,
                text_in(
                    format!("{} XP {}", tree.xp, view.sheet.xp(tree.xp)).to_uppercase(),
                    TextRole::Caption,
                    "browser.status",
                ),
            ],
        ),
    ];
    if tree.placeholder {
        head.push(text_in(
            tree.note
                .clone()
                .unwrap_or_else(|| "A placeholder: nothing earns its XP yet.".to_owned()),
            TextRole::Caption,
            "text.icon",
        ));
    } else {
        // A tree that is real in part says which part, beside its needs.
        if let Some(note) = &tree.note {
            head.push(text(note.clone(), TextRole::Caption));
        }
    }
    if !tree.placeholder && tree.is_elite() {
        let needs: Vec<String> = tree
            .prerequisites
            .iter()
            .map(|p| view.graph.find(p).map_or(p.clone(), |b| b.name.clone()))
            .collect();
        head.push(text(
            format!("An elite tree. Its novice needs {}.", needs.join(", ")),
            TextRole::Caption,
        ));
    }
    panel(
        "invisible",
        Layout {
            width: Some(Length::Percent(100.)),
            ..column(0.6)
        },
        head,
    )
}

/// The legend in the footer: the three stripe colours in words. It stood
/// under the tree until a 720p window needed that line for the tree's foot.
fn legend() -> UiNodeDef {
    let entry = |role: &str, label: &'static str| {
        panel(
            "invisible",
            row(0.8),
            vec![
                block(role, 8., 8.),
                text_in(english(label), TextRole::Caption, "browser.status"),
            ],
        )
    };
    panel(
        "invisible",
        row(3.),
        vec![
            entry("strata.sheet.stripe.learned", "strata.sheet.legend.learned"),
            entry("strata.sheet.stripe.taught", "strata.sheet.legend.taught"),
            entry("strata.sheet.stripe.locked", "strata.sheet.legend.locked"),
        ],
    )
}

/// The tree drawn as the design describes it: the novice at the bottom,
/// four branches rising, the master on top, joined by connectors so the
/// cards read as one tree. A connector is lit when the box under it is
/// learned, so what is learned is a lit path up to the next box.
#[allow(
    clippy::too_many_lines,
    reason = "one drawing, read top to bottom as the tree is"
)]
fn tree_panel(view: &SheetView<'_>, tree: &TreeDef, selected: Option<&str>) -> UiNodeDef {
    let is = |b: &BoxDef| selected == Some(b.id.as_str());
    let novice = learned(view, &tree.novice);
    // The rail under the master joins the branch tops; it is lit once
    // every branch reaches it.
    let tops = !tree.branches.is_empty()
        && tree
            .branches
            .iter()
            .all(|branch| branch.boxes.last().is_some_and(|b| learned(view, b)));
    let branches = tree
        .branches
        .iter()
        .map(|branch| {
            let mut column_children = Vec::new();
            // Top down, each box under the link that rises from it: to
            // the rail from the top box, to the box above from the rest.
            for b in branch.boxes.iter().rev() {
                column_children.push(tick(learned(view, b)));
                column_children.push(box_card(view, b, is(b)));
            }
            column_children.push(panel(
                "invisible",
                Layout {
                    padding: Padding {
                        top: 0.5,
                        right: 0.,
                        bottom: 0.3,
                        left: 0.,
                    },
                    ..column(0.)
                },
                vec![centred(text_in(
                    branch.name.to_uppercase(),
                    TextRole::Caption,
                    "browser.status",
                ))],
            ));
            column_children.push(tick(novice));
            panel(
                "invisible",
                Layout {
                    width: Some(Length::Px(LANE)),
                    align: Some(Align::Center),
                    ..column(0.)
                },
                column_children,
            )
        })
        .collect::<Vec<_>>();
    #[allow(
        clippy::cast_precision_loss,
        reason = "a tree has four branches, far inside f32's whole numbers"
    )]
    let rail_width = LANE * branches.len().saturating_sub(1) as f32;
    let rail = |lit: bool| block(link_role(lit), rail_width.max(LINK), LINK);
    let drawing = panel(
        "invisible",
        Layout {
            align: Some(Align::Center),
            ..column(0.)
        },
        vec![
            box_card(view, &tree.master, is(&tree.master)),
            tick(tops),
            rail(tops),
            panel(
                "invisible",
                Layout {
                    align: Some(Align::Start),
                    ..row(0.)
                },
                branches,
            ),
            rail(novice),
            tick(novice),
            box_card(view, &tree.novice, is(&tree.novice)),
        ],
    );
    // The tree scrolls inside its panel, as the crafting list does, so a
    // window too short for the tallest tree still reaches the novice at its
    // foot: wheel, focus and page keys.
    panel(
        "browser.panel",
        Layout {
            padding: Padding::all(1.5),
            grow: 1.,
            min_height: Some(Length::Px(0.)),
            align: Some(Align::Stretch),
            ..column(0.)
        },
        vec![UiNodeDef::Scroll {
            layout: Layout {
                grow: 1.,
                min_height: Some(Length::Px(0.)),
                align: Some(Align::Start),
                ..column(0.)
            },
            scrollbar: true,
            // As wide as the pane and centred in it when the tree is
            // narrower, and as wide as the tree, from the left edge, when it
            // is not: a centred row wider than its viewport would run off
            // both ends, the left one out of reach.
            children: vec![panel(
                "invisible",
                Layout {
                    min_width: Some(Length::Percent(100.)),
                    align: Some(Align::Center),
                    ..column(1.5)
                },
                vec![tree_head(view, tree), drawing],
            )],
            tags: Tags::new().with(Tags::TEST_ID, TREE_ID),
        }],
    )
}

/// The column of tree tabs: the trees with effects first, then the
/// placeholders under their own heading, smaller, so a tree the game
/// cannot teach yet is browsable but reads as not yet here.
fn tabs_panel(view: &SheetView<'_>, shown: Option<&str>) -> UiNodeDef {
    let tab = |t: &TreeDef| UiNodeDef::Button {
        widget: None,
        opts: ButtonOpts {
            label: Some(key(t.name.clone())),
            variant: if shown == Some(t.id.as_str()) && !view.state.loadout {
                ButtonVariant::Primary
            } else {
                ButtonVariant::Secondary
            },
            compact: !t.in_game(),
            ..ButtonOpts::default()
        },
        tags: Tags::new()
            .with("menu", &format!("{TREE_TAG}{}", t.id))
            .with(Tags::TEST_ID, &format!("tree:{}", t.id)),
    };
    let mut children = vec![text_in(
        "strata.sheet.professions",
        TextRole::Caption,
        "browser.status",
    )];
    children.extend(view.graph.trees.iter().filter(|t| t.in_game()).map(tab));
    let placeholders = view.graph.trees.iter().filter(|t| !t.in_game()).count();
    if placeholders > 0 {
        // Folded to one line unless asked, or unless the shown tree is
        // one of them, so the column is not three quarters placeholders.
        let unfolded = view.state.show_placeholders
            || view
                .graph
                .trees
                .iter()
                .any(|t| !t.in_game() && shown == Some(t.id.as_str()));
        let english = |key: &'static str| super::strings::english(key).unwrap_or(key);
        children.push(UiNodeDef::Button {
            widget: None,
            opts: ButtonOpts {
                label: Some(key(if unfolded {
                    english("strata.sheet.fewer").to_owned()
                } else {
                    format!("{placeholders} {}", english("strata.sheet.more"))
                })),
                variant: ButtonVariant::Secondary,
                compact: true,
                ..ButtonOpts::default()
            },
            tags: Tags::new().with("menu", MORE).with(Tags::TEST_ID, MORE),
        });
        if unfolded {
            children.extend(view.graph.trees.iter().filter(|t| !t.in_game()).map(tab));
        }
    }
    panel(
        "invisible",
        Layout {
            width: Some(Length::Px(160.)),
            align: Some(Align::Stretch),
            ..column(0.6)
        },
        children,
    )
}

/// One effect line: the words in the prose face and the figures in the
/// numeral face, the old figure in grey and the new one bright, so the
/// change reads at a glance and the words are there to be read after.
fn effect_row(line: EffectLine) -> UiNodeDef {
    let mut parts = vec![text_in(line.words, TextRole::Body, "text.label")];
    if let Some(from) = line.from {
        parts.push(text_in(from, TextRole::Body, "strata.sheet.figure.from"));
        parts.push(text_in(
            english("strata.sheet.effect.to"),
            TextRole::Caption,
            "text.caption",
        ));
    }
    if let Some(to) = line.to {
        parts.push(text_in(to, TextRole::Body, "strata.sheet.figure"));
    }
    panel(
        "invisible",
        Layout {
            wrap: true,
            ..row(0.8)
        },
        parts,
    )
}

/// The details panel: the selected box, its place, its words, its
/// effects, its state and the two buttons.
#[allow(
    clippy::too_many_lines,
    reason = "one panel read top to bottom as it stands"
)]
fn details_panel(view: &SheetView<'_>, tree: &TreeDef, b: &BoxDef) -> UiNodeDef {
    let tables = combat_tables::current();
    let state = view.state_of(&b.id);
    let (status, status_role) = status_words(view, tree, b);
    let mut children = vec![
        text(b.name.clone(), TextRole::Heading),
        text_in(
            format!(
                "{}  ·  {} points  ·  {} {} XP",
                place_words(tree, &b.id),
                b.cost,
                b.xp,
                tree.xp
            ),
            TextRole::Caption,
            "browser.status",
        ),
        separator(),
        text(b.description.clone(), TextRole::Body),
    ];
    if let Some((name, icon, says)) = granted(b) {
        children.push(panel(
            "invisible",
            row(1.),
            vec![
                icons::node(&icon, GRANT_SIZE, None, false, Some("sheet.grant")),
                panel(
                    "invisible",
                    Layout {
                        grow: 1.,
                        align: Some(Align::Stretch),
                        ..column(0.2)
                    },
                    vec![
                        text(name, TextRole::Body),
                        text_in(says, TextRole::Caption, "browser.status"),
                    ],
                ),
            ],
        ));
    }
    for (_, product) in view.teaches.iter().filter(|(box_id, _)| *box_id == b.id) {
        children.push(text_in(
            english("strata.sheet.teaches").replace("{name}", product),
            TextRole::Caption,
            "browser.status",
        ));
    }
    let effects = effect_parts(view.graph, &tables, view.sheet, b);
    if effects.is_empty() {
        children.push(text(english("strata.sheet.no_effect"), TextRole::Caption));
    } else {
        children.extend(effects.into_iter().map(effect_row));
    }
    children.push(separator());
    children.push(with_id(
        text_in(status, TextRole::Body, status_role),
        STATUS_ID,
    ));
    if let Some(notice) = &view.state.notice {
        children.push(with_id(
            text_in(notice.clone(), TextRole::Caption, "text.icon"),
            NOTICE_ID,
        ));
    }
    // The text scrolls in the room that is left and the buttons stand at
    // the panel's foot, so a box with one effect and one with many leave
    // them where they were.
    let buttons = panel(
        "invisible",
        row(1.),
        vec![
            button(
                "strata.sheet.learn",
                LEARN,
                ButtonVariant::Primary,
                state != BoxState::Learnable,
            ),
            button(
                "strata.sheet.forget",
                FORGET,
                ButtonVariant::Danger,
                state != BoxState::Learned || view.forget_refusal(&b.id).is_some(),
            ),
        ],
    );
    panel(
        "browser.panel",
        Layout {
            width: Some(Length::Px(280.)),
            padding: Padding::all(2.),
            align: Some(Align::Stretch),
            ..column(1.)
        },
        vec![
            UiNodeDef::Scroll {
                layout: Layout {
                    grow: 1.,
                    min_height: Some(Length::Px(0.)),
                    align: Some(Align::Stretch),
                    ..column(1.)
                },
                scrollbar: true,
                children,
                tags: Tags::new().with(Tags::TEST_ID, DETAILS_SCROLL_ID),
            },
            buttons,
        ],
    )
}

/// The header: the portrait slot, the name, the points and the XP chips.
#[allow(
    clippy::too_many_lines,
    reason = "one definition, read top to bottom as the header is"
)]
fn header(view: &SheetView<'_>, tree: Option<&TreeDef>) -> UiNodeDef {
    let graph = view.graph;
    let spent = view.sheet.points_spent(graph);
    let free = view.sheet.points_free(graph);
    let initial: String = view.name.chars().take(1).collect();
    // The strip under the initial is the character's tint, the same
    // role the bench's card wears, so a sheet opened after a switch says
    // whose it is before the name is read.
    let portrait = panel(
        "dialogue.portrait",
        Layout {
            width: Some(Length::Px(64.)),
            height: Some(Length::Px(64.)),
            align: Some(Align::Center),
            justify: Justify::End,
            ..column(0.)
        },
        vec![
            spacer(),
            text_in(initial, TextRole::Display, "text.display"),
            spacer(),
            panel(
                super::bench::tint_role(view.tint),
                Layout {
                    height: Some(Length::Px(5.)),
                    width: Some(Length::Percent(100.)),
                    ..column(0.)
                },
                Vec::new(),
            ),
        ],
    );
    let who = panel(
        "invisible",
        column(0.),
        vec![
            text(view.name.to_owned(), TextRole::Title),
            text_in("strata.sheet.subtitle", TextRole::Caption, "browser.status"),
        ],
    );
    let points = panel(
        "invisible",
        Layout {
            align: Some(Align::End),
            ..column(0.)
        },
        vec![
            panel(
                "invisible",
                Layout {
                    align: Some(Align::End),
                    ..row(0.8)
                },
                vec![
                    with_id(
                        text_in(spent.to_string(), TextRole::Display, "text.display"),
                        POINTS_ID,
                    ),
                    text_in(
                        format!("/ {}", graph.budget),
                        TextRole::Heading,
                        "text.caption",
                    ),
                ],
            ),
            text_in(
                format!("skill points spent  ·  {free} free"),
                TextRole::Caption,
                "browser.status",
            ),
        ],
    );
    let mut chips = vec![text_in("XP", TextRole::Caption, "browser.status")];
    let shown_kind = tree.map(|t| t.xp);
    for kind in XpKind::ALL {
        let xp = view.sheet.xp(kind);
        if xp == 0 && shown_kind != Some(kind) {
            continue;
        }
        chips.push(panel(
            if shown_kind == Some(kind) {
                "browser.chip.active"
            } else {
                "browser.chip"
            },
            Layout {
                padding: Padding {
                    top: 0.3,
                    right: 1.5,
                    bottom: 0.3,
                    left: 1.5,
                },
                ..row(0.8)
            },
            vec![
                text_in(
                    kind.to_string(),
                    TextRole::Caption,
                    if shown_kind == Some(kind) {
                        "browser.chip.text.active"
                    } else {
                        "browser.chip.text"
                    },
                ),
                text_in(
                    xp.to_string(),
                    TextRole::Caption,
                    if shown_kind == Some(kind) {
                        "strata.sheet.chip.figure.active"
                    } else {
                        "strata.sheet.chip.figure"
                    },
                ),
            ],
        ));
    }
    panel(
        "invisible",
        Layout {
            align: Some(Align::Stretch),
            ..column(1.)
        },
        vec![
            panel("invisible", row(2.), vec![portrait, who, spacer(), points]),
            panel("invisible", row(0.8), chips),
        ],
    )
}

/// The two tabs of the screen, side by side at its top: the trees and the
/// bar. One strip, so the way to the Loadout is where a player looks first.
fn tab_strip(view: &SheetView<'_>) -> UiNodeDef {
    let tab = |label: &'static str, tag: &'static str, on: bool| UiNodeDef::Button {
        widget: None,
        opts: ButtonOpts {
            label: Some(key(label)),
            variant: if on {
                ButtonVariant::Primary
            } else {
                ButtonVariant::Secondary
            },
            ..ButtonOpts::default()
        },
        tags: Tags::new().with("menu", tag).with(Tags::TEST_ID, tag),
    };
    panel(
        "invisible",
        row(0.8),
        vec![
            tab(
                "strata.sheet.tab.professions",
                PROFESSIONS_TAB,
                !view.state.loadout,
            ),
            tab("strata.sheet.loadout", LOADOUT_TAB, view.state.loadout),
        ],
    )
}

/// The whole screen for `view`.
#[must_use]
pub fn screen_def(view: &SheetView<'_>) -> ScreenDef {
    let tree = view.tree();
    let selected = view.selected();
    let shown = tree.map(|t| t.id.as_str());
    let picked = selected.map(|b| b.id.as_str());
    let body = match (tree, selected) {
        _ if view.state.loadout && view.loadout.is_some() => {
            let shown_tab = view.loadout.map(loadout::body);
            vec![tabs_panel(view, shown)]
                .into_iter()
                .chain(shown_tab)
                .collect()
        }
        (Some(tree), Some(b)) => vec![
            tabs_panel(view, shown),
            tree_panel(view, tree, picked),
            details_panel(view, tree, b),
        ],
        _ => vec![text("strata.sheet.empty", TextRole::Body)],
    };
    let footer = panel(
        "invisible",
        row(2.),
        vec![
            hint_bar(),
            legend(),
            spacer(),
            text("strata.sheet.keys", TextRole::Caption),
            close_button(),
        ],
    );
    // The panel takes most of the window and no more than the design's
    // width, and the trees scroll inside it, so a 1280 by 720 window
    // keeps the details panel, the footer and Close inside the border.
    // 96 % of the height rather than less: 1080p at its automatic UI
    // scale is 864 units tall, and the tallest tree with its legend
    // needs that much to show whole without scrolling.
    ScreenDef {
        kind: kind(),
        inherits: None,
        root: UiNodeDef::Panel {
            role: Role::new("panel"),
            layout: Layout {
                width: Some(Length::Percent(94.)),
                max_width: Some(Length::Px(1320.)),
                min_height: Some(Length::Percent(SCREEN_HEIGHT_VH)),
                max_height: Some(Length::Percent(SCREEN_HEIGHT_VH)),
                padding: Padding::all(3.),
                align: Some(Align::Stretch),
                ..column(2.)
            },
            children: vec![
                with_id(header(view, tree), "sheet"),
                tab_strip(view),
                separator(),
                with_id(
                    panel(
                        "invisible",
                        Layout {
                            align: Some(Align::Stretch),
                            grow: 1.,
                            min_height: Some(Length::Px(0.)),
                            overflow: Overflow::Scroll,
                            ..row(2.)
                        },
                        body,
                    ),
                    BODY_ID,
                ),
                footer,
            ],
            tags: Tags::new().with(Tags::TEST_ID, ROOT_ID),
        },
        listring: Vec::new(),
        remove: Vec::new(),
        initial_focus: match view.loadout.filter(|_| view.state.loadout) {
            Some(loadout) => loadout.focus_id.clone(),
            None => picked.map(|id| format!("box:{id}")),
        },
        presentation: Presentation {
            mode: PresentationMode::Page,
            scrim: Some(true),
            transition: Transition::Fade,
            wildcards: Some(false),
            ..Presentation::default()
        },
    }
}

/// The definition for the world as it is: the local player's sheet, the
/// graph, the reach and every trainer. `None` without a player or a
/// graph.
fn build(world: &mut World) -> Option<ScreenDef> {
    let registries = world.get_resource::<Registries>()?.0.clone();
    let reach = world.get_resource::<Reach>().cloned().unwrap_or_default();
    let engaged = world
        .get_resource::<CombatEncounter>()
        .is_some_and(|e| e.engaged && e.remaining > 0);
    let state = world.resource::<SheetState>().clone();
    let mut players = world.query_filtered::<(
        &CharacterSheet,
        &veyra_sim::ability::Bar,
        &Combatant,
        Option<&Name>,
        Option<&veyra_sim::view::BodyTint>,
        &WorldPosition,
    ), With<LocalPlayer>>();
    let (sheet, bar, actor, name, tint, position) = players.single(world).ok()?;
    let sheet = sheet.clone();
    let bar = bar.clone();
    let in_combat = actor.in_combat(engaged);
    let name = name.map_or_else(|| "Ranger".to_owned(), |n| n.as_str().to_owned());
    let tint = tint.map_or(0, |t| t.0);
    let position = *position;
    let mut props = world.query::<(&WorldPosition, &Interaction)>();
    let trainers: Vec<TrainerSpot> = props
        .iter(world)
        .filter_map(|(at, prop)| match &prop.kind {
            InteractionKind::Trainer { trees } => {
                Some(TrainerSpot::from_positions(trees.clone(), at, &position))
            }
            InteractionKind::Terminal
            | InteractionKind::Locker
            | InteractionKind::Workbench
            | InteractionKind::Fabricator
            | InteractionKind::Board { .. }
            | InteractionKind::Guard { .. }
            | InteractionKind::Cache { .. }
            | InteractionKind::Person { .. }
            | InteractionKind::Harvester { .. } => None,
        })
        .collect();
    let teaches: Vec<(String, String)> = registries
        .schematics
        .iter()
        .filter_map(|schematic| {
            let profession = schematic.def.profession.clone()?;
            let product = registries.items.get(&schematic.def.makes)?;
            Some((profession, product.name.clone()))
        })
        .collect();
    let loadout = state.loadout.then(|| {
        let memory = world
            .get_resource::<loadout::LoadoutState>()
            .cloned()
            .unwrap_or_default();
        loadout::view(&loadout::Input {
            graph: &registries.progression,
            sheet: &sheet,
            bar: &bar,
            tables: &combat_tables::current(),
            state: &memory,
        })
    });
    let view = SheetView {
        loadout: loadout.as_ref(),
        teaches: &teaches,
        graph: &registries.progression,
        sheet: &sheet,
        name: &name,
        tint,
        reach: &reach,
        in_combat,
        trainers: &trainers,
        state: &state,
    };
    let def = screen_def(&view);
    // The picks the view resolved are written back, so Learn and Forget
    // act on the box that is shown rather than on a pick never made.
    let tree = view.tree().map(|t| t.id.clone());
    let selected = view.selected().map(|b| b.id.clone());
    let mut state = world.resource_mut::<SheetState>();
    if state.tree != tree || state.selected != selected {
        state.tree = tree;
        state.selected = selected;
    }
    Some(def)
}

/// Pushes the sheet, on `tree` when one is named (a trainer's F), else on
/// the tree the state remembers; rebuilds it in place when it is already
/// on top. A flag, a key or an interaction all come through here.
pub fn open(commands: &mut Commands, tree: Option<String>) {
    commands.queue(move |world: &mut World| {
        {
            let mut state = world.resource_mut::<SheetState>();
            if let Some(tree) = tree {
                state.tree = Some(tree);
                state.selected = None;
                state.loadout = false;
            }
            state.notice = None;
            state.dirty = false;
        }
        let Some(def) = build(world) else {
            tracing::warn!("no local player or no graph: nothing to show on the sheet");
            return;
        };
        let on_top = world
            .resource::<ScreenStack>()
            .top()
            .is_some_and(|top| top.kind == kind());
        if on_top {
            replace_screen(&mut world.commands(), Arc::new(def), None);
        } else {
            // A sheet opened afresh forgets the last visit's place, notice
            // and card, which may name a class the member no longer has.
            world.insert_resource(loadout::LoadoutState::default());
            push_screen(&mut world.commands(), Arc::new(def), None);
        }
    });
}

/// Pushes the sheet on its Loadout tab.
pub fn open_loadout(commands: &mut Commands) {
    open_tab(commands, true);
}

/// Pushes the sheet on the tab named: the Loadout when `loadout`.
pub fn open_tab(commands: &mut Commands, loadout: bool) {
    commands.queue(move |world: &mut World| world.resource_mut::<SheetState>().loadout = loadout);
    open(commands, None);
}

/// Whether the sheet is the top screen.
fn on_top(stack: &ScreenStack) -> bool {
    stack.top().is_some_and(|top| top.kind == kind())
}

/// `Update`: C opens the sheet over nothing, L opens it on the Loadout, and
/// either closes it from the top.
pub fn toggle(keys: Res<ButtonInput<KeyCode>>, stack: Res<ScreenStack>, mut commands: Commands) {
    let loadout = keys.just_pressed(LOADOUT_KEY);
    if !keys.just_pressed(KEY) && !loadout {
        return;
    }
    if on_top(&stack) {
        pop_screen(&mut commands);
    } else if stack.top().is_none() {
        open_tab(&mut commands, loadout);
    }
}

/// `Update`: a tab, a card, Learn or Forget. The two rules are the sim's;
/// what this adds is the sentence shown for the answer and the refit of
/// the actor, so the box is felt on the next tick.
#[allow(
    clippy::too_many_arguments,
    reason = "the choice, the data, the world's reach, the sheet's state and what the lesson tells the cue and the questline"
)]
pub fn answer_choices(
    mut choices: MessageReader<MenuChoice>,
    registries: Option<Res<Registries>>,
    reach: Option<Res<Reach>>,
    encounter: Option<Res<CombatEncounter>>,
    mut state: ResMut<SheetState>,
    mut players: Query<(&mut CharacterSheet, &mut Combatant), With<LocalPlayer>>,
    mut cues: MessageWriter<super::progress::ProgressCue>,
    // The questline's objective "learn a box" (ticket 0014); absent in an
    // app that has no sim plugin.
    mut quest: Option<ResMut<Messages<veyra_sim::quests::QuestEvent>>>,
) {
    for choice in choices.read() {
        if choice.screen != kind() {
            continue;
        }
        if choice.id == LOADOUT_TAB || choice.id == PROFESSIONS_TAB {
            state.loadout = choice.id == LOADOUT_TAB;
            state.notice = None;
            state.dirty = true;
            continue;
        }
        if let Some(tree) = choice.id.strip_prefix(TREE_TAG) {
            state.loadout = false;
            state.tree = Some(tree.to_owned());
            state.selected = None;
            state.notice = None;
            state.dirty = true;
            continue;
        }
        if let Some(id) = choice.id.strip_prefix(BOX_TAG) {
            state.selected = Some(id.to_owned());
            state.notice = None;
            state.dirty = true;
            continue;
        }
        if choice.id == MORE {
            state.show_placeholders = !state.show_placeholders;
            state.dirty = true;
            continue;
        }
        if choice.id != LEARN && choice.id != FORGET {
            continue;
        }
        let (Some(registries), Some(id)) = (registries.as_ref(), state.selected.clone()) else {
            continue;
        };
        let graph = &registries.0.progression;
        let Ok((mut sheet, mut actor)) = players.single_mut() else {
            continue;
        };
        let name = graph.find(&id).map_or(id.clone(), |b| b.name.clone());
        let notice = if choice.id == LEARN {
            let teacher = reach.as_ref().is_some_and(|r| r.teaches(graph, &id));
            match sheet.learn(graph, &id, teacher) {
                Ok(()) => {
                    actor.fit(graph, &sheet);
                    cues.write(super::progress::ProgressCue::Learned);
                    if let Some(quest) = quest.as_mut() {
                        quest.write(veyra_sim::quests::QuestEvent::Learned(id.clone()));
                    }
                    format!("Learned {name}. {} points free.", sheet.points_free(graph))
                }
                Err(refused) => capitalised(&format!("Not learned: {refused}.")),
            }
        } else {
            let engaged = encounter
                .as_ref()
                .is_some_and(|e| e.engaged && e.remaining > 0);
            let in_combat = actor.in_combat(engaged);
            match sheet.forget(graph, &id, in_combat) {
                Ok(points) => {
                    actor.fit(graph, &sheet);
                    format!("Forgot {name}; {points} points returned.")
                }
                Err(refused) => capitalised(&format!("Not forgotten: {refused}.")),
            }
        };
        tracing::info!("sheet: {notice}");
        state.notice = Some(notice);
        state.dirty = true;
    }
}

/// `Update`: a change to the sheet or the reach while the screen is up
/// marks it for a rebuild. A screen pushed this frame is left alone: a
/// pop in the frame of its push races the stack's own deferred work on
/// the new tree.
pub fn mark_dirty(
    stack: Res<ScreenStack>,
    reach: Option<Res<Reach>>,
    sheets: Query<
        (),
        (
            With<LocalPlayer>,
            Or<(Changed<CharacterSheet>, Changed<veyra_sim::ability::Bar>)>,
        ),
    >,
    mut was_on_top: Local<bool>,
    mut state: ResMut<SheetState>,
) {
    let settled = *was_on_top;
    *was_on_top = on_top(&stack);
    if !settled || !*was_on_top || state.dirty {
        return;
    }
    if !sheets.is_empty() || reach.is_some_and(|r| r.is_changed()) {
        state.dirty = true;
    }
}

/// `Update`: rebuilds the open sheet when it is marked. A button press
/// marks it without the frame's grace, so the press itself is answered
/// at once; a press can only land on a screen that has been up a frame.
pub fn rebuild(world: &mut World) {
    if !world.resource::<SheetState>().dirty || !on_top(world.resource::<ScreenStack>()) {
        return;
    }
    world.resource_mut::<SheetState>().dirty = false;
    if let Some(def) = build(world) {
        replace_screen(&mut world.commands(), Arc::new(def), None);
    }
}

/// Run condition for [`rebuild`]: marked, and the sheet is the screen on
/// top. Both are what it tests first, so a clean or closed sheet costs no
/// exclusive system.
fn dirty_and_on_top(state: Res<SheetState>, stack: Res<ScreenStack>) -> bool {
    state.dirty && on_top(&stack)
}

/// Registers the state and the systems. Only called once the UI is wired.
pub(crate) fn register(app: &mut bevy::app::App) {
    loadout::register(app);
    app.init_resource::<SheetState>().add_systems(
        bevy::app::Update,
        (
            toggle,
            answer_choices,
            loadout::answer_choices,
            loadout::answer_sim,
            loadout::follow_focus.run_if(super::screen_on_top(kind)),
            loadout::follow_pointer.run_if(super::screen_on_top(kind)),
            mark_dirty.run_if(super::screen_up(kind)),
            rebuild.run_if(dirty_and_on_top),
            loadout::restore_scroll,
        )
            .chain()
            .in_set(super::ScreenWork),
    );
}

/// The box ids whose every rule but the trainer passes, for the HUD's
/// "a box is within reach" hint.
#[must_use]
pub fn available(graph: &SkillGraph, sheet: &CharacterSheet) -> BTreeSet<String> {
    graph
        .trees
        .iter()
        .filter(|t| !t.placeholder)
        .flat_map(TreeDef::boxes)
        .filter(|b| sheet.can_learn(graph, &b.id, true).is_ok())
        .map(|b| b.id.clone())
        .collect()
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, reason = "the shipped graph has these")]

    use super::*;

    fn graph() -> SkillGraph {
        SkillGraph::shipped()
    }

    fn arrival() -> CharacterSheet {
        let mut sheet = CharacterSheet::default();
        for id in ["blade.novice", "marksman.novice", "marksman.carbine.1"] {
            sheet.learned.insert(id.to_owned());
        }
        sheet.credit(XpKind::Ranged, 150);
        sheet
    }

    #[test]
    fn a_box_that_grants_an_ability_shows_its_square_and_its_line() {
        let graph = graph();
        let sheet = arrival();
        let reach = Reach::default();
        let state = SheetState {
            tree: Some("blade".to_owned()),
            selected: Some("blade.novice".to_owned()),
            ..SheetState::default()
        };
        let view = SheetView {
            teaches: &[],
            loadout: None,
            graph: &graph,
            sheet: &sheet,
            name: "Frontier ranger",
            tint: 0,
            reach: &reach,
            in_combat: false,
            trainers: &[],
            state: &state,
        };
        let def = screen_def(&view);
        let mut squares = Vec::new();
        let mut lines = Vec::new();
        def.root.walk(&mut |node| match node {
            UiNodeDef::Custom { kind, tags, .. } if kind.0.as_str() == icons::WIDGET => {
                squares.push(tags.get(Tags::TEST_ID).unwrap_or_default().to_owned());
            }
            UiNodeDef::Text { key, .. } => lines.push(key.0.clone()),
            _ => {}
        });
        assert!(
            squares.contains(&"grant:blade.novice".to_owned()),
            "{squares:?}"
        );
        assert!(squares.contains(&"sheet.grant".to_owned()), "{squares:?}");
        assert!(
            lines
                .iter()
                .any(|line| line.starts_with("The slow cut that breaks posture.")),
            "the details name the ability with its line: {lines:?}"
        );
        // A box that grants nothing has neither.
        let state_plain = SheetState {
            tree: Some("blade".to_owned()),
            selected: Some("blade.guard.1".to_owned()),
            ..SheetState::default()
        };
        let plain = screen_def(&SheetView {
            state: &state_plain,
            ..view
        });
        let mut any = false;
        plain.root.walk(&mut |node| {
            if let UiNodeDef::Custom { tags, .. } = node
                && tags.get(Tags::TEST_ID) == Some("sheet.grant")
            {
                any = true;
            }
        });
        assert!(!any);
    }

    #[test]
    fn a_card_state_is_the_rule_that_refuses_in_words() {
        let graph = graph();
        let mut sheet = arrival();
        assert_eq!(
            BoxState::of(&graph, &sheet, "blade.novice", false),
            BoxState::Learned
        );
        assert_eq!(
            BoxState::of(&graph, &sheet, "blade.cuts.1", true),
            BoxState::NeedsXp
        );
        assert_eq!(
            BoxState::of(&graph, &sheet, "blade.cuts.2", true),
            BoxState::NeedsPrerequisite
        );
        assert_eq!(
            BoxState::of(&graph, &sheet, "marksman.pistol.1", false),
            BoxState::NeedsTrainer
        );
        assert_eq!(
            BoxState::of(&graph, &sheet, "marksman.pistol.1", true),
            BoxState::Learnable
        );
        sheet.credit(XpKind::Blade, 150);
        assert_eq!(
            BoxState::of(&graph, &sheet, "blade.cuts.1", true),
            BoxState::Learnable
        );
    }

    #[test]
    fn a_card_says_its_state_in_two_words_at_most_and_never_the_same_one() {
        let states = [
            BoxState::Learned,
            BoxState::Learnable,
            BoxState::NeedsTrainer,
            BoxState::NeedsXp,
            BoxState::NeedsPrerequisite,
            BoxState::NoPoints,
            BoxState::NotYetInGame,
        ];
        let words: Vec<String> = states.iter().map(|s| s.words(95, 150)).collect();
        assert_eq!(
            words,
            [
                "Learned",
                "Learn here",
                "Trainer",
                "95 / 150 XP",
                "Locked",
                "No points",
                "Not yet",
            ]
        );
        for (state, word) in states.iter().zip(&words) {
            if *state != BoxState::NeedsXp {
                assert!(word.split_whitespace().count() <= 2, "{word}");
            }
        }
        let unique: BTreeSet<&String> = words.iter().collect();
        assert_eq!(unique.len(), words.len(), "{words:?}");
    }

    #[test]
    fn a_card_wears_one_coloured_border_at_a_time() {
        for state in [BoxState::Learned, BoxState::Learnable, BoxState::NeedsXp] {
            assert_eq!(
                state.frame(true),
                "strata.sheet.card.selected",
                "selection wins"
            );
        }
        assert_eq!(BoxState::Learnable.frame(false), "strata.sheet.card.taught");
        assert_eq!(BoxState::Learned.frame(false), "strata.sheet.card.learned");
        assert_eq!(
            BoxState::NeedsPrerequisite.frame(false),
            "strata.sheet.card"
        );
        assert_eq!(BoxState::Learned.stripe(), "strata.sheet.stripe.learned");
        assert_eq!(BoxState::Learnable.stripe(), "strata.sheet.stripe.taught");
        assert_eq!(
            BoxState::NeedsTrainer.stripe(),
            "strata.sheet.stripe.locked"
        );
        assert_eq!(BoxState::NotYetInGame.stripe(), "strata.sheet.stripe.later");
    }

    /// Every card is one button that keeps the `menu` and `test_id` tags
    /// the script and the tests find it by, with its name a plain text in
    /// the tone of its state; and every `strata.` role the screen names,
    /// a card's every state included, is one the theme defines, since a
    /// missing role falls back to its parent without a word.
    #[test]
    fn the_cards_keep_their_tags_and_every_role_is_in_the_theme() {
        let graph = graph();
        let sheet = arrival();
        let mut reach = Reach::default();
        reach.trees.insert("marksman".to_owned());
        let state = SheetState::default();
        let view = SheetView {
            teaches: &[],
            loadout: None,
            graph: &graph,
            sheet: &sheet,
            name: "Frontier ranger",
            tint: 0,
            reach: &reach,
            in_combat: false,
            trainers: &[],
            state: &state,
        };
        let def = screen_def(&view);
        // The role of the first text on a card, which is its name.
        let name_role = |card: &UiNodeDef| {
            let mut found = None;
            card.walk(&mut |node| {
                if let (None, UiNodeDef::Text { opts, .. }) = (&found, node) {
                    found = opts.role.as_ref().map(|r| r.as_str().to_owned());
                }
            });
            found
        };
        match def.root.find("box:marksman.pistol.1") {
            Some(card @ UiNodeDef::Card { tags, .. }) => {
                assert_eq!(tags.get("menu"), Some("sheet.box:marksman.pistol.1"));
                assert_eq!(name_role(card).as_deref(), Some("strata.sheet.name.lit"));
            }
            other => panic!("{other:?}"),
        }
        match def.root.find("box:marksman.carbine.3") {
            Some(card @ UiNodeDef::Card { .. }) => {
                assert_eq!(name_role(card).as_deref(), Some("strata.sheet.name.dim"));
            }
            other => panic!("{other:?}"),
        }
        let mut buttons_in_cards = 0;
        def.root.walk(&mut |node| {
            if let UiNodeDef::Card { children, .. } = node {
                for child in children {
                    child.walk(&mut |inner| {
                        if matches!(inner, UiNodeDef::Button { .. } | UiNodeDef::Card { .. }) {
                            buttons_in_cards += 1;
                        }
                    });
                }
            }
        });
        assert_eq!(buttons_in_cards, 0, "a card is the only button it holds");
        let mut roles = BTreeSet::new();
        let mut texts = Vec::new();
        def.root.walk(&mut |node| match node {
            UiNodeDef::Panel { role, .. } => {
                roles.insert(role.as_str().to_owned());
            }
            UiNodeDef::Text { key, opts, .. } => {
                texts.push(key.0.clone());
                if let Some(role) = &opts.role {
                    roles.insert(role.as_str().to_owned());
                }
            }
            UiNodeDef::Card { role, .. } => {
                roles.insert(role.as_str().to_owned());
                for state in ["hover", "focus", "pressed"] {
                    roles.insert(format!("{}.{state}", role.as_str()));
                }
            }
            UiNodeDef::Button { opts, .. } => {
                if let Some(role) = &opts.role {
                    roles.insert(role.as_str().to_owned());
                    for state in ["hover", "focus", "pressed"] {
                        roles.insert(format!("{}.{state}", role.as_str()));
                    }
                }
            }
            _ => {}
        });
        assert!(texts.iter().any(|t| t == "150 / 400 XP"), "{texts:?}");
        assert!(texts.iter().any(|t| t == "2 of 18 boxes"), "{texts:?}");
        let path = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../assets/themes/veyra.theme.ron"
        );
        let theme =
            slotted::theme::Theme::from_ron(&std::fs::read_to_string(path).unwrap()).unwrap();
        let ours: Vec<&String> = roles.iter().filter(|r| r.starts_with("strata.")).collect();
        assert!(ours.len() > 8, "{ours:?}");
        for role in ours {
            assert!(
                theme.roles.contains_key(&Role::new(role.clone())),
                "{role} is not in the theme"
            );
        }
    }

    #[test]
    fn the_four_guard_boxes_read_in_the_units_the_screens_use() {
        let graph = graph();
        let tables = combat_tables::current();
        let sheet = arrival();
        let line =
            |id: &str| effect_lines(&graph, &tables, &sheet, graph.find(id).expect("the box"));
        assert!(line("blade.guard.1").contains(&"Guard: stamina cost x0.9".to_owned()));
        assert!(line("blade.guard.2").contains(&"Guard: parry window +0.03 s".to_owned()));
        assert!(line("blade.guard.3").contains(&"Guard: damage taken off +5%".to_owned()));
        assert!(line("blade.guard.4").contains(&"Guard: riposte x1.25".to_owned()));
    }

    #[test]
    fn effects_read_as_plain_words_with_the_numbers_they_move() {
        let graph = graph();
        let tables = combat_tables::current();
        let mut sheet = arrival();
        sheet.learned.insert("blade.footwork.1".to_owned());
        let footwork_2 = graph.find("blade.footwork.2").expect("the box");
        let lines = effect_lines(&graph, &tables, &sheet, footwork_2);
        assert_eq!(
            lines.len(),
            3,
            "two cancels and the sweeping cut: {lines:?}"
        );
        assert!(
            lines[0].starts_with("Light cut dodge cancel at ") && lines[0].ends_with("s to 0.26 s"),
            "{lines:?}"
        );
        // The same line in parts: the words for the prose face, the two
        // figures for the numeral face.
        let parts = effect_parts(&graph, &tables, &sheet, footwork_2);
        assert_eq!(parts[0].words, "Light cut dodge cancel at");
        assert!(parts[0].from.is_some(), "{parts:?}");
        assert_eq!(parts[0].to.as_deref(), Some("0.26 s"));
        let cuts_1 = graph.find("blade.cuts.1").expect("the box");
        assert_eq!(
            effect_lines(&graph, &tables, &sheet, cuts_1),
            vec![
                "Follow-up cut unlocked".to_owned(),
                "Learn the Thrust ability".to_owned()
            ]
        );
        assert_eq!(
            effect_parts(&graph, &tables, &sheet, cuts_1)[0].to,
            None,
            "an unlock has no figure"
        );
        let novice = graph.find("marksman.novice").expect("the box");
        assert_eq!(
            effect_lines(&graph, &tables, &sheet, novice),
            vec![
                "Certified on the pistol".to_owned(),
                "Learn the Butt strike ability".to_owned()
            ]
        );
        // A learned box reads the same as one to learn: with against
        // without, never the row against itself.
        let carbine_1 = graph.find("marksman.carbine.1").expect("the box");
        assert_eq!(
            effect_lines(&graph, &tables, &sheet, carbine_1),
            vec![
                "Certified on the carbine".to_owned(),
                "Learn the Aimed burst ability".to_owned()
            ]
        );
    }

    /// Every effect a Fabricator box can have is worded, and the tree is
    /// drawn as a profession.
    #[test]
    fn the_fabricators_boxes_are_worded_and_the_tree_is_no_placeholder() {
        let graph = graph();
        let tables = combat_tables::current();
        let sheet = CharacterSheet::default();
        let words =
            |id: &str| effect_lines(&graph, &tables, &sheet, graph.find(id).expect("a box"));
        assert_eq!(words("fabricator.novice"), ["Bench: assembly +2 %"]);
        assert_eq!(
            words("fabricator.fixtures.1"),
            ["Fabricator: a unit in 10 % less time"]
        );
        assert_eq!(
            words("fabricator.fixtures.2"),
            ["Fabricator: runs a blueprint starts with x1.25"]
        );
        assert_eq!(
            words("fabricator.fixtures.3"),
            ["Fabricator: unattended share +15 %"]
        );
        assert_eq!(
            words("fabricator.fixtures.4"),
            ["Fabricator: a unit in 20 % less time"]
        );
        assert_eq!(
            words("fabricator.batches.1"),
            ["+1 experimentation point in a session"]
        );
        assert_eq!(words("fabricator.batches.2"), ["Bench: assembly +3 %"]);
        assert_eq!(words("fabricator.batches.4"), ["Bench: assembly +4 %"]);
        assert_eq!(
            words("fabricator.repair.1"),
            ["Field repair restores 20 % more"]
        );
        assert_eq!(
            words("fabricator.repair.2"),
            ["A workbench repair uses no kit"]
        );
        assert_eq!(
            words("fabricator.repair.3"),
            ["Field repair restores 30 % more"]
        );
        assert_eq!(
            words("fabricator.repair.4"),
            ["A shot or a cut wears the weapon 20 % less"]
        );
        assert_eq!(
            words("fabricator.master"),
            [
                "+1 experimentation point in a session",
                "Fabricator: a unit in 20 % less time"
            ]
        );
        for boxed in graph.boxes().filter(|b| b.id.starts_with("fabricator.")) {
            assert_eq!(
                effect_lines(&graph, &tables, &sheet, boxed).len(),
                boxed.effects.len(),
                "{} words each of its effects",
                boxed.id
            );
        }
        let tree = graph.tree("fabricator").expect("ships");
        assert!(tree.in_game() && !tree.placeholder && tree.note.is_none());
    }

    /// A box that teaches a schematic says its product in its description,
    /// so the sheet names what learning it opens.
    #[test]
    fn a_teaching_box_names_its_schematic() {
        let registries = crate::testing::shipped();
        let mut taught = 0;
        // The eight this step added; the older boxes word their feed and
        // blank their own way.
        let new = [
            "drive_coil",
            "hull_plate",
            "survey_tool",
            "courier_bike",
            "vigil_reflex",
            "vigil_angled_grip",
            "vigil_compensator",
            "service_pistol",
        ];
        for schematic in registries
            .schematics
            .iter()
            .filter(|s| new.iter().any(|n| s.def.id.path().ends_with(n)))
        {
            let Some(needs) = &schematic.def.profession else {
                continue;
            };
            let boxed = registries.progression.find(needs).expect("a box");
            let product = registries
                .items
                .get(&schematic.def.makes)
                .expect("an item")
                .name
                .to_lowercase();
            let description = boxed.description.to_lowercase();
            // A module's name carries its maker's brand; the box says the
            // part.
            let part = product.rsplit(' ').next().unwrap_or(&product);
            assert!(
                description.contains(&product) || description.contains(part),
                "{needs} teaches {product}: {description}"
            );
            taught += 1;
        }
        assert_eq!(taught, 8);
    }

    /// Every effect a Surveyor box can have is worded, with the figure it
    /// moves, and the tree is drawn as a profession.
    #[test]
    fn the_surveyors_boxes_are_worded_and_the_tree_is_no_placeholder() {
        let graph = graph();
        let tables = combat_tables::current();
        let sheet = CharacterSheet::default();
        let words =
            |id: &str| effect_lines(&graph, &tables, &sheet, graph.find(id).expect("a box"));
        assert_eq!(
            words("surveyor.novice"),
            ["Units a sampling cycle +1", "Ping time x0.8"]
        );
        assert_eq!(words("surveyor.deposits.1"), ["Ping reach +64 m"]);
        assert_eq!(words("surveyor.deposits.4")[1], "Sampling cycle time x0.8");
        assert_eq!(
            words("surveyor.reading.1"),
            ["Reads each line of a deposit as a band"]
        );
        assert_eq!(
            words("surveyor.reading.2"),
            ["Reads each line of a deposit exactly"]
        );
        assert_eq!(words("surveyor.reading.3"), ["Ping grid, points a side, 7"]);
        assert_eq!(
            words("surveyor.reading.4"),
            ["A ping also tells where each deposit's richest ground lies"]
        );
        assert_eq!(words("surveyor.routes.1"), ["Bike: call time x0.6"]);
        assert_eq!(words("surveyor.routes.2"), ["Bike: off-road speed x1.1"]);
        assert_eq!(
            words("surveyor.routes.3"),
            ["Bike: steepest wall it rides over +5 degrees"]
        );
        assert_eq!(words("surveyor.routes.4"), ["Bike: road speed x1.1"]);
        assert_eq!(
            words("surveyor.master"),
            ["Ping reach +128 m", "Ping time x0.6"]
        );
        for boxed in graph.boxes().filter(|b| b.id.starts_with("surveyor.")) {
            assert_eq!(
                effect_lines(&graph, &tables, &sheet, boxed).len(),
                boxed.effects.len(),
                "{} words each of its effects",
                boxed.id
            );
        }
        let tree = graph.tree("surveyor").expect("ships");
        assert!(tree.in_game() && !tree.placeholder);
        let state = SheetState {
            tree: Some("surveyor".to_owned()),
            ..SheetState::default()
        };
        let reach = Reach::default();
        let view = SheetView {
            teaches: &[],
            loadout: None,
            graph: &graph,
            sheet: &sheet,
            name: "Ilse Marrow",
            tint: 1,
            reach: &reach,
            in_combat: false,
            trainers: &[],
            state: &state,
        };
        let drawn = format!("{:?}", screen_def(&view));
        assert!(!drawn.contains("A placeholder"), "no placeholder note");
        assert!(drawn.contains("Surveyor"));
        // A Stations box says what it does now that harvesters stand: its
        // description, and its effect worded from the Station family.
        let state = SheetState {
            tree: Some("surveyor".to_owned()),
            selected: Some("surveyor.stations.1".to_owned()),
            ..SheetState::default()
        };
        let view = SheetView {
            teaches: &[],
            loadout: None,
            state: &state,
            ..view
        };
        let drawn = format!("{:?}", screen_def(&view));
        assert!(
            !drawn.contains("does nothing yet") && !drawn.contains("it will run harvesters"),
            "the stations box no longer waits"
        );
        assert!(
            drawn.contains("You may have one more field harvester planted")
                && drawn.contains("Field harvesters you may have planted +1"),
            "the stations box says what it does"
        );
    }

    #[test]
    fn ranger_is_on_the_sheet_with_long_shots_live_and_the_rest_teaching_nothing() {
        let graph = graph();
        let tables = combat_tables::current();
        let sheet = CharacterSheet::default();
        let tree = graph.tree("ranger").expect("ships");
        assert!(tree.in_game() && !tree.placeholder);
        let words =
            |id: &str| effect_lines(&graph, &tables, &sheet, graph.find(id).expect("a box"));
        assert_eq!(
            words("ranger.long_shots.1"),
            ["Certified on the rifle", "Learn the Focus ability"]
        );
        for id in [
            "ranger.long_shots.2",
            "ranger.long_shots.3",
            "ranger.long_shots.4",
        ] {
            assert_eq!(
                words(id).len(),
                2,
                "{id} says what it moves and what it teaches"
            );
        }
        assert!(words("ranger.long_shots.2")[0].contains("aim time"));
        for boxed in graph.boxes().filter(|b| {
            b.id.starts_with("ranger.tracking") || b.id.starts_with("ranger.wilderness")
        }) {
            assert_eq!(boxed.description, "Teaches nothing yet.", "{}", boxed.id);
            assert!(boxed.effects.is_empty());
        }
        let state = SheetState {
            tree: Some("ranger".to_owned()),
            ..SheetState::default()
        };
        let reach = Reach::default();
        let view = SheetView {
            teaches: &[],
            loadout: None,
            graph: &graph,
            sheet: &sheet,
            name: "Ilse Marrow",
            tint: 1,
            reach: &reach,
            in_combat: false,
            trainers: &[],
            state: &state,
        };
        let drawn = format!("{:?}", screen_def(&view));
        assert!(!drawn.contains("A placeholder"));
        assert!(drawn.contains("Long shots 1") && drawn.contains("Ranger"));
        assert!(
            drawn.contains("Only Long shots is real, for the rifle"),
            "the tree's note is drawn under its head"
        );
    }

    #[test]
    fn the_screen_picks_the_tree_with_the_points_and_the_next_box() {
        let graph = graph();
        let sheet = arrival();
        let reach = Reach::default();
        let state = SheetState::default();
        let view = SheetView {
            teaches: &[],
            loadout: None,
            graph: &graph,
            sheet: &sheet,
            name: "Frontier ranger",
            tint: 0,
            reach: &reach,
            in_combat: false,
            trainers: &[],
            state: &state,
        };
        assert_eq!(view.tree().map(|t| t.id.as_str()), Some("marksman"));
        assert_eq!(
            view.selected().map(|b| b.id.as_str()),
            Some("marksman.carbine.2")
        );
        assert!(!view.can_learn_selected());
        let def = screen_def(&view);
        assert_eq!(def.kind, kind());
        assert_eq!(def.initial_focus.as_deref(), Some("box:marksman.carbine.2"));
        let state = SheetState {
            tree: Some("blade".to_owned()),
            ..SheetState::default()
        };
        let view = SheetView {
            teaches: &[],
            loadout: None,
            state: &state,
            ..view
        };
        assert_eq!(view.selected().map(|b| b.id.as_str()), Some("blade.cuts.1"));
        let (status, _) = status_words(
            &view,
            graph.tree("blade").unwrap(),
            view.selected().unwrap(),
        );
        assert_eq!(status, "Needs 150 blade XP; has 0.");
    }

    #[test]
    fn the_trainer_line_says_who_and_which_way() {
        let graph = graph();
        let mut sheet = arrival();
        sheet.credit(XpKind::Blade, 200);
        let reach = Reach::default();
        let state = SheetState {
            tree: Some("blade".to_owned()),
            selected: Some("blade.cuts.1".to_owned()),
            ..SheetState::default()
        };
        let trainers = [TrainerSpot::from_positions(
            vec!["blade".to_owned()],
            &WorldPosition::new(100., 0., 100.),
            &WorldPosition::new(0., 0., 0.),
        )];
        assert_eq!(trainers[0].bearing, "north-west");
        let view = SheetView {
            teaches: &[],
            loadout: None,
            graph: &graph,
            sheet: &sheet,
            name: "Frontier ranger",
            tint: 0,
            reach: &reach,
            in_combat: false,
            trainers: &trainers,
            state: &state,
        };
        let (status, _) = status_words(
            &view,
            graph.tree("blade").unwrap(),
            view.selected().unwrap(),
        );
        assert_eq!(status, "Needs the blade trainer, 141 m to the north-west.");
        let mut reach = Reach::default();
        reach.trees.insert("blade".to_owned());
        let view = SheetView {
            teaches: &[],
            loadout: None,
            reach: &reach,
            ..view
        };
        assert!(view.can_learn_selected());
        let (status, _) = status_words(
            &view,
            graph.tree("blade").unwrap(),
            view.selected().unwrap(),
        );
        assert_eq!(status, "The blade trainer here teaches it for 2 points.");
    }

    #[test]
    fn a_placeholder_box_is_not_yet_in_the_game_whatever_the_rules_say() {
        let graph = graph();
        let mut sheet = arrival();
        sheet.credit(XpKind::Medical, 10_000);
        assert_eq!(
            BoxState::of(&graph, &sheet, "medic.novice", true),
            BoxState::NotYetInGame,
            "a trainer and the XP change nothing"
        );
        sheet.learned.insert("medic.novice".to_owned());
        assert_eq!(
            BoxState::of(&graph, &sheet, "medic.novice", true),
            BoxState::NotYetInGame,
            "learned or not"
        );
        let reach = Reach::default();
        let state = SheetState {
            tree: Some("medic".to_owned()),
            ..SheetState::default()
        };
        let view = SheetView {
            teaches: &[],
            loadout: None,
            graph: &graph,
            sheet: &sheet,
            name: "Dr. Anneke Faal",
            tint: 3,
            reach: &reach,
            in_combat: false,
            trainers: &[],
            state: &state,
        };
        let tree = graph.tree("medic").unwrap();
        let (status, _) = status_words(&view, tree, view.selected().unwrap());
        assert_eq!(status, tree.note.clone().unwrap());
        let def = screen_def(&view);
        let mut def = def.clone();
        match def.root.find_mut(LEARN) {
            Some(UiNodeDef::Button { opts, .. }) => assert!(opts.disabled),
            other => panic!("{other:?}"),
        }
        // The placeholders are listed because the shown tree is one;
        // on a basic tree the column folds them to one line.
        assert!(def.root.find_mut("tree:medic").is_some());
        let state = SheetState::default();
        let view = SheetView {
            teaches: &[],
            loadout: None,
            state: &state,
            ..view
        };
        let mut def = screen_def(&view);
        assert!(def.root.find_mut("tree:medic").is_none(), "folded");
        assert!(def.root.find_mut(MORE).is_some());
    }

    #[test]
    fn a_bench_tree_is_offered_at_a_workbench_and_waits_away_from_one() {
        let graph = graph();
        let sheet = arrival();
        let bench = Reach {
            workbench: true,
            ..Reach::default()
        };
        let away = Reach::default();
        let id = "fabricator.novice";
        assert_eq!(
            BoxState::of(&graph, &sheet, id, bench.teaches(&graph, id)),
            BoxState::Learnable
        );
        assert_eq!(
            BoxState::of(&graph, &sheet, id, away.teaches(&graph, id)),
            BoxState::NeedsTrainer
        );
        let state = SheetState {
            tree: Some("fabricator".to_owned()),
            ..SheetState::default()
        };
        let view = SheetView {
            teaches: &[],
            loadout: None,
            graph: &graph,
            sheet: &sheet,
            name: "Dr. Anneke Faal",
            tint: 3,
            reach: &bench,
            in_combat: false,
            trainers: &[],
            state: &state,
        };
        let mut def = screen_def(&view);
        assert!(def.root.find_mut("tree:fabricator").is_some());
        match def.root.find_mut(LEARN) {
            Some(UiNodeDef::Button { opts, .. }) => assert!(!opts.disabled),
            other => panic!("{other:?}"),
        }
        let tree = graph.tree("fabricator").unwrap();
        let (status, _) = status_words(&view, tree, view.selected().unwrap());
        assert!(status.starts_with("A workbench teaches it"), "{status}");
    }

    #[test]
    fn a_bench_tree_names_what_is_in_reach_and_what_it_needs_away() {
        let graph = graph();
        let sheet = arrival();
        let state = SheetState {
            tree: Some("fabricator".to_owned()),
            ..SheetState::default()
        };
        let teaches = [("fabricator.novice".to_owned(), "Patrol carbine".to_owned())];
        let said = |reach: &Reach| {
            let view = SheetView {
                teaches: &teaches,
                loadout: None,
                graph: &graph,
                sheet: &sheet,
                name: "Dr. Anneke Faal",
                tint: 3,
                reach,
                in_combat: false,
                trainers: &[],
                state: &state,
            };
            let tree = graph.tree("fabricator").unwrap();
            let status = status_words(&view, tree, view.selected().unwrap()).0;
            let mut def = screen_def(&view);
            let line = def.root.find_mut(STATUS_ID).is_some();
            assert!(line);
            status
        };
        let foreman = Reach {
            trees: ["fabricator".to_owned()].into_iter().collect(),
            ..Reach::default()
        };
        let bench = Reach {
            workbench: true,
            ..Reach::default()
        };
        assert!(said(&bench).starts_with("A workbench teaches it"));
        assert!(said(&foreman).starts_with("The works foreman teaches it"));
        assert_eq!(
            said(&Reach::default()),
            "Needs a workbench or the works foreman."
        );
    }

    #[test]
    fn the_available_set_ignores_the_trainer_and_the_placeholders() {
        let graph = graph();
        let mut sheet = arrival();
        assert_eq!(
            available(&graph, &sheet),
            BTreeSet::from([
                "fabricator.novice".to_owned(),
                "marksman.field_craft.1".to_owned(),
                "marksman.pistol.1".to_owned(),
                "marksman.steadiness.1".to_owned(),
                "surveyor.novice".to_owned(),
            ]),
            "150 ranged XP pays every branch's first gate but the learned carbine's, and the Surveyor and the Fabricator, real trees now, open with their novices"
        );
        sheet.credit(XpKind::Blade, 150);
        assert!(available(&graph, &sheet).contains("blade.cuts.1"));
        sheet.learned.insert("medic.novice".to_owned());
        assert!(
            available(&graph, &sheet)
                .iter()
                .all(|id| !id.starts_with("medic.")),
            "a placeholder tree never hints"
        );
    }

    /// Learn on the sheet raises the questline's `Learned` with the box's id
    /// (ticket 0014): the objective "learn a box" is met by the screen's own
    /// answer.
    #[test]
    fn learning_on_the_sheet_raises_learned_for_the_questline() {
        use bevy::ecs::message::Messages;
        use bevy::ecs::system::RunSystemOnce;
        use veyra_sim::quests::QuestEvent;
        let registries =
            strata_registry::load_from_dir(concat!(env!("CARGO_MANIFEST_DIR"), "/../../assets"))
                .unwrap();
        let graph = registries.progression.clone();
        let mut sheet = arrival();
        for kind in XpKind::ALL {
            sheet.credit(kind, 10_000);
        }
        let learnable = graph
            .boxes()
            .map(|each| each.id.clone())
            .find(|id| sheet.clone().learn(&graph, id, true).is_ok())
            .unwrap();
        let tree = graph.tree_of(&learnable).unwrap().id.clone();
        let mut world = World::new();
        world.init_resource::<Messages<MenuChoice>>();
        world.init_resource::<Messages<super::super::progress::ProgressCue>>();
        world.init_resource::<Messages<QuestEvent>>();
        world.insert_resource(Registries(Arc::new(registries)));
        let mut reach = Reach::default();
        reach.trees.insert(tree);
        world.insert_resource(reach);
        world.insert_resource(SheetState {
            selected: Some(learnable.clone()),
            ..SheetState::default()
        });
        world.spawn((LocalPlayer, sheet, Combatant::default()));
        let button = world.spawn_empty().id();
        world
            .resource_mut::<Messages<MenuChoice>>()
            .write(MenuChoice {
                id: LEARN.to_owned(),
                screen: kind(),
                entity: button,
            });
        world.run_system_once(answer_choices).unwrap();
        let raised: Vec<QuestEvent> = world
            .resource_mut::<Messages<QuestEvent>>()
            .drain()
            .collect();
        assert_eq!(raised, [QuestEvent::Learned(learnable)]);
    }
}
