//! The combat HUD over the world: the vitals cluster bottom left, the
//! controls card bottom centre, the hit marker round the reticle and the
//! hurt vignette at the window's edge.
//!
//! The cluster and the card are slotted HUD layers, so the theme paints
//! them (the fonts, the glass, the bar colours are roles in
//! `veyra.theme.ron`), a screen hides them, and a mod or the position
//! editor can move them like any other layer. What changes per frame is
//! written straight onto the spawned nodes, found by their `test_id`,
//! rather than sent as `HudUpdate`s: a layer is respawned whenever a
//! screen closes over it, and a write that finds its node every frame
//! cannot be lost across that respawn. A role swap (health turning
//! danger, the magazine running low) is a `Themed` change, which repaints
//! that one node.
//!
//! The words and the thresholds are [`vitals`], a pure function of the
//! local player's `Combatant`, `Name` and the encounter, so a test reads
//! them without a window. The hit marker, the reticle and the vignette are
//! plain Bevy nodes: they are shapes, not text, and follow `Feedback`
//! every frame. Under thirty percent health the vignette and the health
//! figure pulse together, so the danger is felt at the edge of the eye
//! and confirmed where the eye goes to read it.
//!
//! When the local player falls the cluster stays, the world's own UI
//! (nameplates, numbers, the reticle, the controls card) goes, and a
//! card over a scrim says so; `world_ui_shown` is that gate. The card
//! is raw nodes painted from theme roles through `Painted`, which also
//! carries a strength, because a slotted layer cannot fade.

use bevy::input::ButtonInput;
use bevy::input::keyboard::KeyCode;
use bevy::prelude::*;
use bevy::ui::{
    BackgroundGradient, ColorStop, Gradient, RadialGradient, RadialGradientShape, UiPosition,
    UiTransform,
};
use slotted::theme::{Role, Themed, roles};
use slotted::ui::def::{
    Align, Justify, Layout, LayoutDirection, Length, LocKey, Padding, Tags, TextAlign, TextOpts,
    TextRole, UiNodeDef,
};
use slotted::ui::{
    HudAnchor, HudLayerDef, HudLayerId, HudLayers, NineAnchor, ScreenStack, TestIdIndex,
};
use veyra_sim::combat::{Action, CombatEncounter, Combatant, FireMode, Weapon, WeaponSpec};
use veyra_sim::items::Hands;
use veyra_sim::ride::{Calling, Riding};
use veyra_sim::wear::{self, Band, WearNotice};
use veyra_sim::{LocalPlayer, Registries};

use crate::combat::Feedback;
use strata_client::ClientSet;
use strata_ui::painted::Painted;
use strata_ui::world_ui::{hud_shown, ui_point, ui_scale, world_ui_shown};
use veyra_ui::world_ui::is_down;

/// The vitals cluster's layer id.
pub const VITALS_LAYER: &str = "strata:vitals";
/// The controls card's layer id.
pub const CONTROLS_LAYER: &str = "strata:controls";

/// Shows and hides the controls card.
pub const CONTROLS_KEY: KeyCode = KeyCode::F1;

/// How long the card stays up after the player arrives or a swap brings
/// someone else out, seconds: long enough to read once, short enough to
/// be gone before the first fight.
pub const CONTROLS_SECS: f32 = 8.0;

/// Below this share of its maximum the health bar turns danger.
pub const LOW_HEALTH: f32 = 0.30;

/// The `test_id`s of the cluster's live nodes.
pub mod ids {
    /// The cluster's panel.
    pub const PANEL: &str = "vitals";
    /// The character's name.
    pub const NAME: &str = "vitals.name";
    /// The weapon chip's text.
    pub const WEAPON: &str = "vitals.weapon";
    /// The "not certified" chip.
    pub const UNCERTIFIED: &str = "vitals.uncertified";
    /// The encounter's word.
    pub const ENCOUNTER: &str = "vitals.encounter";
    /// The rounds in the magazine.
    pub const AMMO: &str = "vitals.ammo";
    /// "/ 24" beside it.
    pub const MAGAZINE: &str = "vitals.magazine";
    /// The "R  RELOAD" hint.
    pub const RELOAD: &str = "vitals.reload";
    /// The action's word.
    pub const ACTION: &str = "vitals.action";
    /// The row under the magazine that holds the condition bar and chip.
    pub const WEAR: &str = "vitals.wear";
    /// The ride row: the bike's speed, or the call under way.
    pub const RIDE: &str = "vitals.ride";
    /// The ride row's figure.
    pub const SPEED: &str = "vitals.speed";
    /// The condition bar's track.
    pub const WEAR_TRACK: &str = "vitals.wear.track";
    /// The condition bar's fill.
    pub const WEAR_FILL: &str = "vitals.wear.fill";
    /// The "worn" chip.
    pub const WEAR_CHIP: &str = "vitals.wear.chip";
    /// The chip's words.
    pub const WEAR_WORD: &str = "vitals.wear.word";
    /// The controls card's panel.
    pub const CONTROLS: &str = "controls";
    /// The controls card's word for the trigger, which follows the weapon.
    pub const CONTROLS_FIRE: &str = "controls.fire";
    /// The controls card's right-button entry.
    pub const CONTROLS_AIM: &str = "controls.aim";
    /// The right-button entry's word, which follows the weapon: Aim for a
    /// gun, Guard for a melee weapon.
    pub const CONTROLS_AIM_LABEL: &str = "controls.aim.label";
    /// The controls card's reload entry, there for a gun.
    pub const CONTROLS_RELOAD: &str = "controls.reload";
    /// The controls card's heavy cut entry, there for a melee weapon.
    pub const CONTROLS_HEAVY: &str = "controls.heavy";
    /// The first weapon slot's chip text.
    pub const SLOT_ONE: &str = "vitals.slot.1";
    /// The second weapon slot's chip text.
    pub const SLOT_TWO: &str = "vitals.slot.2";
    /// The melee weapon's condition word, which takes the ammo row's place.
    pub const CONDITION: &str = "vitals.condition";
}

/// One of the three bars.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Meter {
    /// Health, success green, danger when low.
    Health,
    /// Shield, accent.
    Shield,
    /// Stamina, warm.
    Stamina,
}

impl Meter {
    /// Top to bottom.
    pub const ALL: [Self; 3] = [Self::Health, Self::Shield, Self::Stamina];

    /// The whole row's `test_id`: label, track and figure.
    #[must_use]
    pub const fn row(self) -> &'static str {
        match self {
            Self::Health => "vitals.health.row",
            Self::Shield => "vitals.shield.row",
            Self::Stamina => "vitals.stamina.row",
        }
    }

    /// The track's `test_id`.
    #[must_use]
    pub const fn track(self) -> &'static str {
        match self {
            Self::Health => "vitals.health.track",
            Self::Shield => "vitals.shield.track",
            Self::Stamina => "vitals.stamina.track",
        }
    }

    /// The fill's `test_id`.
    #[must_use]
    pub const fn fill(self) -> &'static str {
        match self {
            Self::Health => "vitals.health.fill",
            Self::Shield => "vitals.shield.fill",
            Self::Stamina => "vitals.stamina.fill",
        }
    }

    /// The figure's `test_id`.
    #[must_use]
    pub const fn value(self) -> &'static str {
        match self {
            Self::Health => "vitals.health.value",
            Self::Shield => "vitals.shield.value",
            Self::Stamina => "vitals.stamina.value",
        }
    }

    /// The label's `test_id`.
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Health => "vitals.health.label",
            Self::Shield => "vitals.shield.label",
            Self::Stamina => "vitals.stamina.label",
        }
    }

    /// The label's locale key.
    const fn key(self) -> &'static str {
        match self {
            Self::Health => "strata.vitals.health",
            Self::Shield => "strata.vitals.shield",
            Self::Stamina => "strata.vitals.stamina",
        }
    }

    /// The fill's role at rest.
    const fn role(self) -> &'static str {
        match self {
            Self::Health => "strata.vitals.health",
            Self::Shield => "strata.vitals.shield",
            Self::Stamina => "strata.vitals.stamina",
        }
    }
}

/// The encounter as the cluster words it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EncounterWord {
    /// The hostiles are not placed yet.
    Streaming,
    /// Placed and waiting.
    Ahead,
    /// Fighting.
    Engaged,
    /// Nobody left standing.
    Clear,
}

impl EncounterWord {
    /// The word for `encounter`'s state.
    #[must_use]
    pub fn of(encounter: &CombatEncounter) -> Self {
        if !encounter.ready {
            Self::Streaming
        } else if encounter.remaining == 0 {
            Self::Clear
        } else if encounter.engaged {
            Self::Engaged
        } else {
            Self::Ahead
        }
    }

    /// Its locale key.
    #[must_use]
    pub const fn key(self) -> &'static str {
        match self {
            Self::Streaming => "strata.vitals.streaming",
            Self::Ahead => "strata.vitals.ahead",
            Self::Engaged => "strata.vitals.engaged",
            Self::Clear => "strata.vitals.clear",
        }
    }

    /// Its role: danger while fighting, success when clear, muted else.
    #[must_use]
    pub const fn role(self) -> &'static str {
        match self {
            Self::Engaged => "strata.vitals.encounter.engaged",
            Self::Clear => "strata.vitals.encounter.clear",
            Self::Streaming | Self::Ahead => "strata.vitals.encounter.quiet",
        }
    }
}

/// How full the magazine is, as the numeral's colour says it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AmmoTone {
    /// More than a quarter left.
    Plenty,
    /// A quarter or less: time to reload.
    Low,
    /// Dry.
    Empty,
}

impl AmmoTone {
    /// The tone for `ammo` rounds of a `magazine`.
    #[must_use]
    pub fn of(ammo: u16, magazine: u16) -> Self {
        if ammo == 0 {
            Self::Empty
        } else if u32::from(ammo) * 4 <= u32::from(magazine) {
            Self::Low
        } else {
            Self::Plenty
        }
    }

    /// The numeral's role.
    #[must_use]
    pub const fn role(self) -> &'static str {
        match self {
            Self::Plenty => "strata.vitals.ammo",
            Self::Low => "strata.vitals.ammo.low",
            Self::Empty => "strata.vitals.ammo.empty",
        }
    }
}

/// A pool and its maximum.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Pool {
    /// What is left.
    pub value: f32,
    /// Full.
    pub max: f32,
}

impl Pool {
    /// The share left, in `0..=1`.
    #[must_use]
    pub fn fraction(self) -> f32 {
        if self.max > 0.0 {
            (self.value / self.max).clamp(0.0, 1.0)
        } else {
            0.0
        }
    }

    /// The figure beside the bar. Rounded up, so a sliver of health
    /// reads 1 rather than a 0 that looks like death.
    #[must_use]
    pub fn figure(self) -> String {
        format!("{:.0}", self.value.max(0.0).ceil())
    }
}

/// The action's word on the ammo row, when there is one to say.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ActionWord {
    /// Its locale key.
    pub key: &'static str,
    /// Said in danger (a stagger, the fall) rather than plain.
    pub danger: bool,
}

impl ActionWord {
    /// The word for `action`; nothing while ready or cutting, since a
    /// cut is seen on the body and a word would only lag it.
    #[must_use]
    pub const fn of(action: Action) -> Option<Self> {
        // Down has the card ([`spawn_down_card`] and friends): a second
        // line in the vitals said the same thing twice.
        let (key, danger) = match action {
            Action::Ready
            | Action::Light
            | Action::Followup
            | Action::Heavy
            | Action::Ability
            | Action::Dead => {
                return None;
            }
            Action::Reload => ("strata.vitals.reloading", false),
            Action::Stagger => ("strata.vitals.staggered", true),
            Action::Dodge => ("strata.vitals.dodge", false),
            Action::Swap => ("strata.vitals.swapping", false),
        };
        Some(Self { key, danger })
    }

    /// Its role.
    #[must_use]
    pub const fn role(self) -> &'static str {
        if self.danger {
            "strata.vitals.action.danger"
        } else {
            "strata.vitals.action"
        }
    }
}

/// Everything the cluster shows, worked out from the sim's state.
#[derive(Debug, Clone, PartialEq)]
#[allow(
    clippy::struct_excessive_bools,
    reason = "each flag is a separate word or chip of the cluster"
)]
pub struct Vitals {
    /// Who is out.
    pub name: String,
    /// The weapon in hand, as its chip says it.
    pub weapon: &'static str,
    /// The sheet does not certify the weapon in hand, so the trigger
    /// will do nothing; the warm chip says so before the player finds out.
    pub uncertified: bool,
    /// The encounter's word, when there is an encounter.
    pub encounter: Option<EncounterWord>,
    /// Health.
    pub health: Pool,
    /// Shield, out of the equipment's capacity.
    pub shield: Pool,
    /// Stamina.
    pub stamina: Pool,
    /// The guard is up: the stamina row says so in words, since the guard
    /// is what spends it.
    pub guarding: bool,
    /// What the two weapon slots hold, and which is in hand.
    pub slots: [SlotChip; 2],
    /// A melee weapon is in hand: it has no magazine, so the ammo row gives
    /// way to its condition word.
    pub melee: bool,
    /// The condition of the weapon in hand, for the melee word.
    pub condition: Band,
    /// Rounds in the magazine.
    pub ammo: u16,
    /// Rounds it holds.
    pub magazine: u16,
    /// The numeral's colour.
    pub ammo_tone: AmmoTone,
    /// Show "R  RELOAD": low or dry and not already reloading.
    pub reload_hint: bool,
    /// The action's word, if it has one.
    pub action: Option<ActionWord>,
    /// The weapon in hand's condition, when it is worth showing.
    pub wear: Option<WearReading>,
    /// The bike, while it is called or ridden.
    pub ride: Option<RideReading>,
}

/// One weapon slot as its chip says it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SlotChip {
    /// The weapon's name, or none for an empty slot.
    pub weapon: Option<&'static str>,
    /// The slot is the one in hand.
    pub in_hand: bool,
}

impl SlotChip {
    /// The chip's words: the draw key and the name, with an arrow ahead of
    /// the one in hand so it is marked by more than colour.
    #[must_use]
    pub fn words(self, slot: usize) -> String {
        let key = (slot + 1).to_string();
        let name = self
            .weapon
            .unwrap_or_else(|| english("strata.vitals.slot.empty"));
        let template = if self.in_hand {
            "strata.vitals.slot.hand"
        } else {
            "strata.vitals.slot"
        };
        english(template)
            .replace("{key}", &key)
            .replace("{weapon}", name)
    }

    /// The text role: the one in hand reads brighter.
    #[must_use]
    pub const fn role(self) -> &'static str {
        if self.in_hand {
            "strata.vitals.chip.text"
        } else {
            "strata.vitals.label"
        }
    }
}

/// The name a weapon family goes by on a chip.
#[must_use]
pub fn weapon_name(weapon: Weapon) -> &'static str {
    english(match weapon {
        Weapon::Carbine => "strata.vitals.carbine",
        Weapon::Pistol => "strata.vitals.pistol",
        Weapon::Blade => "strata.vitals.blade",
        Weapon::Greatblade => "strata.vitals.greatblade",
        Weapon::Glaive => "strata.vitals.glaive",
        Weapon::Rifle => "strata.vitals.rifle",
    })
}

/// What the ride row says: the call under way, or the speed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RideReading {
    /// The bike is on its way: the channel.
    Calling,
    /// Mounted, at this many kilometres an hour.
    Speed {
        /// The figure shown.
        kmh: u32,
    },
}

impl RideReading {
    /// The reading of a rider's signed `speed` in metres a second. The
    /// conversion is units, not a tunable.
    #[must_use]
    pub fn of_speed(speed: f32) -> Self {
        #[allow(
            clippy::cast_possible_truncation,
            clippy::cast_sign_loss,
            reason = "a rounded magnitude of a bounded speed"
        )]
        let kmh = (speed.abs() * 3.6).round() as u32;
        Self::Speed { kmh }
    }

    /// The words: `72 km/h`, or the call's.
    #[must_use]
    pub fn words(self) -> String {
        match self {
            Self::Calling => english("strata.ride.calling").to_owned(),
            Self::Speed { kmh } => english("strata.ride.speed").replace("{kmh}", &kmh.to_string()),
        }
    }

    /// The figure's role: large for a speed, the action's size for a call.
    #[must_use]
    pub const fn role(self) -> &'static str {
        match self {
            Self::Calling => "strata.vitals.action",
            Self::Speed { .. } => "strata.vitals.ammo",
        }
    }
}
/// The weapon in hand's condition, as the cluster shows it: a bar once
/// tired, a chip once the lines slide.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WearReading {
    /// Its condition.
    pub condition: u16,
    /// What full is.
    pub full: u16,
    /// Which band that is in.
    pub band: Band,
}

impl WearReading {
    /// The reading of a weapon at `condition`; none while it is sound.
    #[must_use]
    pub fn of(rules: &strata_registry::wear::WearRules, condition: u16) -> Option<Self> {
        let band = wear::band(rules, condition);
        (band != Band::Sound).then_some(Self {
            condition,
            full: rules.full,
            band,
        })
    }

    /// The fill's share of its track, in percent.
    #[must_use]
    pub fn percent(self) -> f32 {
        f32::from(self.condition) * 100.0 / f32::from(self.full.max(1))
    }
}

impl Vitals {
    /// Whether the health bar wears danger.
    #[must_use]
    pub fn health_low(&self) -> bool {
        self.health.fraction() < LOW_HEALTH
    }

    /// The health fill's role.
    #[must_use]
    pub fn health_role(&self) -> &'static str {
        if self.health_low() {
            "strata.vitals.health.low"
        } else {
            Meter::Health.role()
        }
    }

    /// One of the three pools.
    #[must_use]
    pub const fn pool(&self, meter: Meter) -> Pool {
        match meter {
            Meter::Health => self.health,
            Meter::Shield => self.shield,
            Meter::Stamina => self.stamina,
        }
    }
}

/// The English for one of our keys, or the key itself.
pub(crate) fn english(key: &'static str) -> &'static str {
    veyra_ui::strings::english(key).unwrap_or(key)
}

/// The cluster's contents. The maxima are the actor's own: health and
/// stamina from its fitted table (a box may raise them), the shield from
/// the equipment's capacity.
#[must_use]
pub fn vitals(
    actor: &Combatant,
    name: Option<&str>,
    encounter: Option<&CombatEncounter>,
) -> Vitals {
    let (health_max, stamina_max) = actor.with_actor(|table| (table.health, table.stamina));
    let magazine = actor.magazine();
    let ammo_tone = AmmoTone::of(actor.ammo, magazine);
    Vitals {
        name: name.filter(|name| !name.trim().is_empty()).map_or_else(
            || english("strata.vitals.fallback_name").to_owned(),
            str::to_owned,
        ),
        weapon: weapon_name(actor.weapon),
        slots: [0, 1].map(|slot| SlotChip {
            weapon: actor.slots()[slot].map(weapon_name),
            in_hand: actor.hand() == slot,
        }),
        melee: actor.weapon.is_melee(),
        condition: Band::Sound,
        uncertified: !actor.certified(actor.weapon),
        encounter: encounter.map(EncounterWord::of),
        health: Pool {
            value: actor.health,
            max: health_max,
        },
        shield: Pool {
            value: actor.shield,
            max: actor.shield_capacity,
        },
        stamina: Pool {
            value: actor.stamina,
            max: stamina_max,
        },
        guarding: actor.guard_up,
        ammo: actor.ammo,
        magazine,
        ammo_tone,
        reload_hint: !actor.weapon.is_melee()
            && ammo_tone != AmmoTone::Plenty
            && actor.action != Action::Reload,
        action: ActionWord::of(actor.action),
        wear: None,
        ride: None,
    }
}

// ---------------------------------------------------------------------------
// The layers as data
// ---------------------------------------------------------------------------

/// The vitals layer's id.
#[must_use]
pub fn vitals_layer() -> HudLayerId {
    HudLayerId::new_static(VITALS_LAYER)
}

/// The controls card's layer id.
#[must_use]
pub fn controls_layer() -> HudLayerId {
    HudLayerId::new_static(CONTROLS_LAYER)
}

pub(crate) fn tags(id: Option<&str>) -> Tags {
    id.map_or_else(Tags::new, |id| Tags::new().with(Tags::TEST_ID, id))
}

/// One line of text in `role`, unwrapped.
pub(crate) fn text(key: &str, role: &'static str, id: Option<&str>) -> UiNodeDef {
    text_aligned(key, role, id, TextAlign::Left)
}

pub(crate) fn text_aligned(
    key: &str,
    role: &'static str,
    id: Option<&str>,
    align: TextAlign,
) -> UiNodeDef {
    UiNodeDef::Text {
        key: LocKey(key.to_owned()),
        style: TextRole::Body,
        opts: TextOpts {
            role: Some(Role::new_static(role)),
            wrap: false,
            align,
            ..TextOpts::default()
        },
        tags: tags(id),
    }
}

/// A container that paints nothing.
pub(crate) fn invisible(layout: Layout, children: Vec<UiNodeDef>, id: Option<&str>) -> UiNodeDef {
    UiNodeDef::Panel {
        role: Role::new_static("invisible"),
        layout,
        children,
        tags: tags(id),
    }
}

/// A row of children centred on one line.
pub(crate) fn row(gap: f32, children: Vec<UiNodeDef>, id: Option<&str>) -> UiNodeDef {
    invisible(
        Layout {
            direction: LayoutDirection::Row,
            gap,
            align: Some(Align::Center),
            ..Layout::default()
        },
        children,
        id,
    )
}

/// A pill chip around one uppercase word.
fn chip(
    role: &'static str,
    text_role: &'static str,
    key: &str,
    text_id: Option<&str>,
    id: Option<&str>,
) -> UiNodeDef {
    UiNodeDef::Panel {
        role: Role::new_static(role),
        layout: Layout {
            padding: Padding {
                top: 0.2,
                right: 1.4,
                bottom: 0.2,
                left: 1.4,
            },
            align: Some(Align::Center),
            ..Layout::default()
        },
        children: vec![text(key, text_role, text_id)],
        tags: tags(id),
    }
}

/// A grow-one spacer, which pushes what follows to the row's right end.
fn spacer() -> UiNodeDef {
    invisible(
        Layout {
            grow: 1.0,
            min_width: Some(Length::Px(12.0)),
            ..Layout::default()
        },
        Vec::new(),
        None,
    )
}

/// The width of a bar's track.
pub const BAR_WIDTH: f32 = 260.0;
/// Its height.
pub const BAR_HEIGHT: f32 = 8.0;
/// The label column left of the bars.
pub const LABEL_WIDTH: f32 = 56.0;
/// The figure column right of them.
const FIGURE_WIDTH: f32 = 34.0;

/// One bar's row: the label, the track and its fill, the figure.
fn meter_row(meter: Meter) -> UiNodeDef {
    row(
        1.0,
        vec![
            invisible(
                Layout {
                    width: Some(Length::Px(LABEL_WIDTH)),
                    ..Layout::default()
                },
                vec![text(
                    meter.key(),
                    "strata.vitals.label",
                    Some(meter.label()),
                )],
                None,
            ),
            UiNodeDef::Panel {
                role: Role::new_static("strata.vitals.track"),
                layout: Layout {
                    width: Some(Length::Px(BAR_WIDTH)),
                    height: Some(Length::Px(BAR_HEIGHT)),
                    ..Layout::default()
                },
                children: vec![UiNodeDef::Panel {
                    role: Role::new_static(meter.role()),
                    layout: Layout {
                        width: Some(Length::Percent(100.0)),
                        height: Some(Length::Percent(100.0)),
                        ..Layout::default()
                    },
                    children: Vec::new(),
                    tags: tags(Some(meter.fill())),
                }],
                tags: tags(Some(meter.track())),
            },
            invisible(
                Layout {
                    direction: LayoutDirection::Row,
                    min_width: Some(Length::Px(FIGURE_WIDTH)),
                    justify: Justify::End,
                    ..Layout::default()
                },
                vec![text_aligned(
                    "",
                    "strata.vitals.value",
                    Some(meter.value()),
                    TextAlign::Right,
                )],
                None,
            ),
        ],
        Some(meter.row()),
    )
}

/// The top row: the name, the weapon's chip, the warm chip when the
/// weapon is not certified, and the encounter's word at the right end.
fn title_row() -> UiNodeDef {
    invisible(
        Layout {
            direction: LayoutDirection::Row,
            gap: 1.0,
            align: Some(Align::Center),
            width: Some(Length::Percent(100.0)),
            ..Layout::default()
        },
        vec![
            text(
                "strata.vitals.fallback_name",
                "strata.vitals.name",
                Some(ids::NAME),
            ),
            chip(
                "strata.vitals.chip",
                "strata.vitals.chip.text",
                "strata.vitals.carbine",
                Some(ids::WEAPON),
                None,
            ),
            chip(
                "strata.vitals.chip.warm",
                "strata.vitals.chip.warm.text",
                "strata.vitals.uncertified",
                None,
                Some(ids::UNCERTIFIED),
            ),
            spacer(),
            text("", "strata.vitals.encounter.quiet", Some(ids::ENCOUNTER)),
        ],
        None,
    )
}

/// `child` lifted off the row's bottom edge by `bottom` steps, so a small
/// word sits on the large numeral's baseline rather than below it.
fn on_baseline(bottom: f32, child: UiNodeDef) -> UiNodeDef {
    invisible(
        Layout {
            padding: Padding {
                top: 0.0,
                right: 0.0,
                bottom,
                left: 0.0,
            },
            ..Layout::default()
        },
        vec![child],
        None,
    )
}

/// "R  RELOAD": the key in a warm outlined pill, the word beside it.
fn reload_hint() -> UiNodeDef {
    invisible(
        Layout {
            direction: LayoutDirection::Row,
            gap: 0.8,
            align: Some(Align::Center),
            padding: Padding {
                top: 0.0,
                right: 0.0,
                bottom: 0.4,
                left: 1.0,
            },
            ..Layout::default()
        },
        vec![
            UiNodeDef::Panel {
                role: Role::new_static("strata.vitals.reload.glyph"),
                layout: Layout {
                    min_width: Some(Length::Px(18.0)),
                    padding: Padding {
                        top: 0.1,
                        right: 0.7,
                        bottom: 0.1,
                        left: 0.7,
                    },
                    align: Some(Align::Center),
                    justify: Justify::Center,
                    ..Layout::default()
                },
                children: vec![text_aligned(
                    "strata.vitals.reload_key",
                    "strata.vitals.reload.glyph.text",
                    None,
                    TextAlign::Center,
                )],
                tags: Tags::new(),
            },
            text("strata.vitals.reload", "strata.vitals.reload", None),
        ],
        Some(ids::RELOAD),
    )
}

/// The bottom row: the magazine large, its capacity small beside it, the
/// reload hint, and the action's word at the right end.
fn ammo_row() -> UiNodeDef {
    invisible(
        Layout {
            direction: LayoutDirection::Row,
            gap: 0.8,
            align: Some(Align::End),
            width: Some(Length::Percent(100.0)),
            ..Layout::default()
        },
        vec![
            text("", "strata.vitals.ammo", Some(ids::AMMO)),
            on_baseline(0.7, text("", "strata.vitals.ammo.max", Some(ids::MAGAZINE))),
            reload_hint(),
            on_baseline(
                0.4,
                text("", "strata.vitals.chip.text", Some(ids::CONDITION)),
            ),
            spacer(),
            on_baseline(
                0.6,
                text_aligned(
                    "",
                    "strata.vitals.action",
                    Some(ids::ACTION),
                    TextAlign::Right,
                ),
            ),
        ],
        None,
    )
}

/// The wear row: a thin bar under the magazine and, once the lines slide,
/// a chip that says so in words as well as colour. The whole row is out
/// of the layout while the weapon is sound.
fn wear_row() -> UiNodeDef {
    let mut row = row(
        1.0,
        vec![
            UiNodeDef::Panel {
                role: Role::new_static("strata.vitals.track"),
                layout: Layout {
                    width: Some(Length::Px(WEAR_WIDTH)),
                    height: Some(Length::Px(WEAR_HEIGHT)),
                    ..Layout::default()
                },
                children: vec![UiNodeDef::Panel {
                    role: Role::new_static("strata.vitals.wear"),
                    layout: Layout {
                        width: Some(Length::Percent(100.0)),
                        height: Some(Length::Percent(100.0)),
                        ..Layout::default()
                    },
                    children: Vec::new(),
                    tags: tags(Some(ids::WEAR_FILL)),
                }],
                tags: tags(Some(ids::WEAR_TRACK)),
            },
            chip(
                "strata.vitals.chip.warm",
                "strata.vitals.chip.warm.text",
                "strata.vitals.worn",
                Some(ids::WEAR_WORD),
                Some(ids::WEAR_CHIP),
            ),
        ],
        Some(ids::WEAR),
    );
    if let UiNodeDef::Panel { layout, .. } = &mut row {
        layout.align = Some(Align::Center);
    }
    row
}

/// The ride row: the bike's speed in kilometres an hour while mounted, the
/// call's words while it is on its way. Out of the layout otherwise.
fn ride_row() -> UiNodeDef {
    row(
        1.0,
        vec![text("", "strata.vitals.ammo", Some(ids::SPEED))],
        Some(ids::RIDE),
    )
}

/// The width of the condition bar's track.
const WEAR_WIDTH: f32 = 110.0;
/// Its height, thinner than the pools' bars.
const WEAR_HEIGHT: f32 = 6.0;

/// The cluster: name, weapon and the encounter on top, the three bars,
/// then the magazine, the condition and the action's word.
#[must_use]
pub fn vitals_layer_def() -> HudLayerDef {
    let bars = invisible(
        Layout {
            direction: LayoutDirection::Column,
            gap: 0.8,
            ..Layout::default()
        },
        Meter::ALL.into_iter().map(meter_row).collect(),
        None,
    );
    let panel = UiNodeDef::Panel {
        role: Role::new_static("strata.vitals.panel"),
        layout: Layout {
            direction: LayoutDirection::Column,
            gap: 1.4,
            padding: Padding {
                top: 1.7,
                right: 2.0,
                bottom: 1.2,
                left: 2.0,
            },
            align: Some(Align::Stretch),
            ..Layout::default()
        },
        children: vec![title_row(), bars, ammo_row(), wear_row(), ride_row()],
        tags: tags(Some(ids::PANEL)),
    };
    HudLayerDef {
        id: vitals_layer(),
        anchor: HudAnchor {
            anchor: NineAnchor::BottomLeft,
            offset: Vec2::new(24.0, -24.0),
            scale: 1.0,
        },
        // The statuses the player bears sit in a row over the panel, in the
        // same layer so they follow its height.
        tree: invisible(
            Layout {
                direction: LayoutDirection::Column,
                gap: 0.6,
                ..Layout::default()
            },
            vec![crate::action_bar::status_row(), panel],
            None,
        ),
        visible: true,
        hide_with_screen: true,
    }
}

/// The card's entries, column by column: moving, fighting, the rest.
/// Each is a key glyph's locale key and its label's.
const CONTROLS: [&[(&str, &str)]; 3] = [
    &[
        ("strata.controls.key.move", "strata.controls.move"),
        ("strata.controls.key.sprint", "strata.controls.sprint"),
        ("strata.controls.key.jump", "strata.controls.jump"),
        ("strata.controls.key.dodge", "strata.controls.dodge"),
        ("strata.controls.key.use", "strata.controls.use"),
        ("strata.controls.key.map", "strata.controls.map"),
    ],
    &[
        ("strata.controls.key.fire", "strata.controls.fire"),
        ("strata.controls.key.aim", "strata.controls.aim"),
        ("strata.controls.key.reload", "strata.controls.reload"),
        ("strata.controls.key.weapons", "strata.controls.weapons"),
        ("strata.controls.key.abilities", "strata.controls.abilities"),
        ("strata.controls.key.ride", "strata.controls.ride"),
    ],
    &[
        ("strata.controls.key.heavy", "strata.controls.heavy"),
        ("strata.controls.key.sheet", "strata.controls.sheet"),
        ("strata.controls.key.loadout", "strata.controls.loadout"),
        ("strata.controls.key.inventory", "strata.controls.inventory"),
        ("strata.controls.key.survey", "strata.controls.survey"),
        ("strata.controls.key.utility", "strata.controls.utility"),
        ("strata.controls.key.menu", "strata.controls.menu"),
    ],
];

/// The key column inside a card column, so the labels line up.
const KEY_COLUMN: f32 = 48.0;

/// One entry: the key as a `hint.glyph` pill, the verb beside it.
fn control(key: &str, label: &str) -> UiNodeDef {
    row(
        0.8,
        vec![
            invisible(
                Layout {
                    direction: LayoutDirection::Row,
                    min_width: Some(Length::Px(KEY_COLUMN)),
                    ..Layout::default()
                },
                vec![UiNodeDef::Panel {
                    role: roles::HINT_GLYPH,
                    layout: Layout {
                        min_width: Some(Length::Px(24.0)),
                        padding: Padding {
                            top: 0.2,
                            right: 1.0,
                            bottom: 0.2,
                            left: 1.0,
                        },
                        align: Some(Align::Center),
                        justify: Justify::Center,
                        ..Layout::default()
                    },
                    children: vec![text_aligned(
                        key,
                        "hint.glyph.text",
                        None,
                        TextAlign::Center,
                    )],
                    tags: Tags::new(),
                }],
                None,
            ),
            text(
                label,
                "strata.controls.label",
                match label {
                    "strata.controls.fire" => Some(ids::CONTROLS_FIRE),
                    "strata.controls.aim" => Some(ids::CONTROLS_AIM_LABEL),
                    _ => None,
                },
            ),
        ],
        match label {
            "strata.controls.aim" => Some(ids::CONTROLS_AIM),
            "strata.controls.reload" => Some(ids::CONTROLS_RELOAD),
            "strata.controls.heavy" => Some(ids::CONTROLS_HEAVY),
            _ => None,
        },
    )
}

/// The controls card, bottom centre above the interaction chip.
#[must_use]
pub fn controls_layer_def() -> HudLayerDef {
    let columns = CONTROLS
        .iter()
        .map(|column| {
            invisible(
                Layout {
                    direction: LayoutDirection::Column,
                    gap: 0.8,
                    ..Layout::default()
                },
                column
                    .iter()
                    .map(|(key, label)| control(key, label))
                    .collect(),
                None,
            )
        })
        .collect();
    HudLayerDef {
        id: controls_layer(),
        anchor: HudAnchor {
            // The corner opposite the vitals: the middle of the window is
            // where the player, the props and the prompt chip are, and a
            // card there covers the first thing a new player looks at.
            anchor: NineAnchor::BottomRight,
            // Above the action bar, which shares the bottom edge.
            offset: Vec2::new(-24.0, -24.0 - crate::action_bar::CARD_RAISE),
            scale: 1.0,
        },
        tree: UiNodeDef::Panel {
            role: Role::new_static("strata.controls.panel"),
            layout: Layout {
                direction: LayoutDirection::Column,
                gap: 1.2,
                padding: Padding {
                    top: 1.6,
                    right: 2.4,
                    bottom: 1.8,
                    left: 2.4,
                },
                ..Layout::default()
            },
            children: vec![
                text("strata.controls.title", "strata.controls.title", None),
                invisible(
                    Layout {
                        direction: LayoutDirection::Row,
                        gap: 3.5,
                        ..Layout::default()
                    },
                    columns,
                    None,
                ),
            ],
            tags: tags(Some(ids::CONTROLS)),
        },
        visible: false,
        hide_with_screen: true,
    }
}

// ---------------------------------------------------------------------------
// Systems
// ---------------------------------------------------------------------------

/// Whether the controls card is up: for [`CONTROLS_SECS`] after an
/// arrival, or until F1 again once F1 brought it up.
#[derive(Resource, Debug, Default, Clone, Copy, PartialEq)]
pub struct ControlsCard {
    /// Seconds of the arrival's showing left.
    pub left: f32,
    /// F1 brought it up; it stays until F1 again.
    pub pinned: bool,
}

impl ControlsCard {
    /// Whether it is shown.
    #[must_use]
    pub fn shown(&self) -> bool {
        self.pinned || self.left > 0.0
    }
}

/// Raw nodes of the HUD that follow `Feedback`: the four ticks' parent.
#[derive(Component)]
struct HitMarker;

/// One of the four ticks.
#[derive(Component)]
struct HitTick;

/// The full-window danger gradient.
#[derive(Component)]
struct HurtVignette;

/// Registers the layers, the card's state and the systems. Called by
/// [`crate::combat::hud_only`].
pub(crate) fn install(app: &mut App) {
    // The sim registers it too; a HUD on its own app needs it to read.
    slotted::ui::TestIdIndex::install(app.world_mut());
    crate::action_bar::install(app);
    app.add_message::<WearNotice>()
        .init_resource::<ControlsCard>()
        .add_systems(Startup, (register_layers, spawn_overlays))
        .add_systems(
            Update,
            (
                (
                    (update_vitals, update_controls_fire).run_if(
                        bevy::ecs::schedule::common_conditions::any_with_component::<LocalPlayer>,
                    ),
                    knocked_toast,
                ),
                (
                    toggle_controls.run_if(strata_ui::game_input_allowed),
                    show_controls,
                )
                    .chain(),
                (
                    update_hit_marker,
                    update_reticle,
                    update_vignette,
                    update_down_card,
                ),
            )
                .in_set(ClientSet::Hud),
        )
        // The hurt's aberration (lighting plan phase 4b).
        .add_systems(Update, update_aberration.in_set(ClientSet::Hud));
    strata_ui::painted::install(app);
}

/// `Startup`: both layers; the card starts hidden and [`show_controls`]
/// raises it when the player arrives. Slotted's own crosshair, a plain
/// dot, gives way to [`Reticle`]: its layer stays, drawing nothing, so the
/// settings' HUD toggle and a screen still show and hide it, and the
/// reticle follows its visibility.
fn register_layers(layers: Option<ResMut<HudLayers>>) {
    let Some(mut layers) = layers else {
        return;
    };
    layers.register(vitals_layer_def());
    layers.register(controls_layer_def());
    let id = slotted::ui::hud::builtin::CROSSHAIR;
    if let Some(def) = layers.get(&id).cloned() {
        let empty = HudLayerDef {
            visible: def.visible,
            hide_with_screen: def.hide_with_screen,
            ..HudLayerDef::empty(id.clone())
        };
        // The id was just found, so the replace cannot miss.
        let _ = layers.replace(&id, empty);
    }
}

/// Whether the crosshair's layer is on: the settings' HUD toggle turns it
/// off, and the reticle with it. With no layers at all, on.
fn crosshair_on(layers: Option<Res<HudLayers>>) -> bool {
    layers.is_none_or(|layers| {
        layers
            .get(&slotted::ui::hud::builtin::CROSSHAIR)
            .is_none_or(|layer| layer.visible)
    })
}

/// The palette's accent, for the hit marker's ticks.
pub const ACCENT: Color = Color::srgb(0.498, 0.820, 1.0);
/// The palette's danger, for a lethal hit and the hurt vignette.
pub const DANGER: Color = Color::srgb(1.0, 0.420, 0.420);

/// How far each tick's centre sits from the reticle's, px, per axis.
const TICK_OFFSET: f32 = 9.0;
/// The tick's length.
const TICK_LENGTH: f32 = 8.0;
/// The box the four ticks are laid out in.
const MARKER_BOX: f32 = 40.0;

/// The vignette's depth: under the nameplates and the scope, which sit
/// under every HUD layer (slotted's `zbands::HUD` is zero).
const VIGNETTE_Z: i32 = -6;
/// The down card's depth: over the world's UI, under the vitals cluster,
/// which stays readable through the scrim.
const DOWN_Z: i32 = -1;

/// `Startup`: the hurt vignette under everything, the reticle, the down
/// card, and the hit marker's four diagonal ticks round the window's
/// centre, where the reticle is.
fn spawn_overlays(mut commands: Commands) {
    commands.spawn((
        HurtVignette,
        Name::new("hud.vignette"),
        Node {
            position_type: PositionType::Absolute,
            width: percent(100),
            height: percent(100),
            ..default()
        },
        BackgroundGradient::default(),
        GlobalZIndex(VIGNETTE_Z),
        Pickable::IGNORE,
        Visibility::Hidden,
    ));
    spawn_reticle(&mut commands);
    spawn_down_card(&mut commands);
    commands
        .spawn((
            Node {
                position_type: PositionType::Absolute,
                width: percent(100),
                height: percent(100),
                align_items: AlignItems::Center,
                justify_content: JustifyContent::Center,
                ..default()
            },
            Pickable::IGNORE,
        ))
        .with_children(|parent| {
            parent
                .spawn((
                    HitMarker,
                    Name::new("hud.hit_marker"),
                    Node {
                        width: px(MARKER_BOX),
                        height: px(MARKER_BOX),
                        ..default()
                    },
                    UiTransform::default(),
                    Pickable::IGNORE,
                    Visibility::Hidden,
                ))
                .with_children(|marker| {
                    let centre = MARKER_BOX * 0.5;
                    // A tick points at the centre: the upper-left and the
                    // lower-right lie along "\", the other two along "/".
                    // `UiTransform` rotates clockwise on screen.
                    for (dx, dy, degrees) in [
                        (-1.0, -1.0, -45.0_f32),
                        (1.0, -1.0, 45.0),
                        (-1.0, 1.0, 45.0),
                        (1.0, 1.0, -45.0),
                    ] {
                        marker.spawn((
                            HitTick,
                            Node {
                                position_type: PositionType::Absolute,
                                left: px(centre + dx * TICK_OFFSET - 1.0),
                                top: px(centre + dy * TICK_OFFSET - TICK_LENGTH * 0.5),
                                width: px(2.0),
                                height: px(TICK_LENGTH),
                                border_radius: BorderRadius::all(px(1.0)),
                                ..default()
                            },
                            BackgroundColor(ACCENT),
                            UiTransform::from_rotation(Rot2::degrees(degrees)),
                            Pickable::IGNORE,
                        ));
                    }
                });
        });
}

// ---------------------------------------------------------------------------
// The reticle
// ---------------------------------------------------------------------------

/// The palette's text colour, the reticle at rest.
const TEXT: Color = Color::srgb(0.902, 0.929, 0.953);
/// The palette's warm: the reticle over a hostile.
pub const WARM: Color = Color::srgb(1.0, 0.706, 0.329);

/// The gap between the centre and each bar's inner end at rest, px.
pub const RETICLE_GAP: f32 = 10.0;
/// The gap while aiming: the bars close in as the camera does.
pub const RETICLE_GAP_AIMED: f32 = 5.0;
/// How far the bars bloom out at full recoil, px.
pub const RETICLE_BLOOM: f32 = 8.0;
/// The recoil [`Feedback`] saturates at; a full burst is full bloom.
const RECOIL_FULL: f32 = 1.5;
/// A bar's length and thickness, px.
const BAR_LONG: f32 = 7.0;
const BAR_THIN: f32 = 2.0;
/// The centre dot's diameter, px.
const DOT: f32 = 3.0;
/// The box the reticle is laid out in; a wide cone and its bloom may reach
/// past it, which clips nothing.
const RETICLE_BOX: f32 = 64.0;
/// How fast the gap follows the aim, per second: the hit marker's and
/// the recoil's own decay, so the three settle together.
const RETICLE_EASE: f32 = 18.0;

/// The reticle's gap, px, for `aim` (`0` at rest to `1` aimed, eased by
/// the caller) and the camera's `recoil`.
#[must_use]
pub fn reticle_gap(aim: f32, recoil: f32) -> f32 {
    let aim = aim.clamp(0.0, 1.0);
    let bloom = (recoil / RECOIL_FULL).clamp(0.0, 1.0) * RETICLE_BLOOM;
    RETICLE_GAP + (RETICLE_GAP_AIMED - RETICLE_GAP) * aim + bloom
}

/// The screen radius, in UI pixels, of a cone of `half_angle_degrees` at
/// the centre of a view `fov_y` radians tall on a window `height` logical
/// pixels tall, at UI scale `scale`. The bars rest on the cone a bolt
/// leaves in, so the reticle says what the weapon will do.
#[must_use]
pub fn cone_gap(half_angle_degrees: f32, fov_y: f32, height: f32, scale: f32) -> f32 {
    let reach = half_angle_degrees.to_radians().tan() / (fov_y * 0.5).tan();
    ui_point(Vec2::splat(reach * height * 0.5), scale).x
}

/// How far the reticle has closed to its aimed gap. A weapon with an
/// `aim_time` closes its cone over that time, and the reticle must show the
/// cone as it is, so it follows the sim's share; any other weapon steps with
/// the aim, which the reticle's own ease softens.
#[must_use]
fn reticle_share(actor: &Combatant, eased: f32) -> f32 {
    if actor.weapon_spec().aim_time > 0. {
        actor.aim_share()
    } else {
        eased
    }
}

/// The reticle's parent.
#[derive(Component)]
struct Reticle;

/// One of the four bars, by the direction it sits in from the centre.
#[derive(Component)]
struct ReticleBar(Vec2);

/// The dot and the bars, which share a colour.
#[derive(Component)]
struct ReticlePart;

/// The shadow under each part, so the reticle reads over snow and sky.
fn reticle_shadow() -> BoxShadow {
    BoxShadow::new(
        Color::srgba(0.0, 0.0, 0.0, 0.55),
        px(0.0),
        px(0.0),
        px(0.0),
        px(2.0),
    )
}

/// The reticle: a dot at the window's centre and four bars at the compass
/// points, placed by [`update_reticle`] every frame.
fn spawn_reticle(commands: &mut Commands) {
    let centre = RETICLE_BOX * 0.5;
    commands
        .spawn((
            Node {
                position_type: PositionType::Absolute,
                width: percent(100),
                height: percent(100),
                align_items: AlignItems::Center,
                justify_content: JustifyContent::Center,
                ..default()
            },
            Pickable::IGNORE,
        ))
        .with_children(|parent| {
            parent
                .spawn((
                    Reticle,
                    Name::new("hud.reticle"),
                    Node {
                        width: px(RETICLE_BOX),
                        height: px(RETICLE_BOX),
                        ..default()
                    },
                    Pickable::IGNORE,
                    Visibility::Hidden,
                ))
                .with_children(|reticle| {
                    reticle.spawn((
                        ReticlePart,
                        Node {
                            position_type: PositionType::Absolute,
                            left: px(centre - DOT * 0.5),
                            top: px(centre - DOT * 0.5),
                            width: px(DOT),
                            height: px(DOT),
                            border_radius: BorderRadius::all(px(DOT * 0.5)),
                            ..default()
                        },
                        BackgroundColor(TEXT),
                        reticle_shadow(),
                        Pickable::IGNORE,
                    ));
                    for direction in [Vec2::NEG_Y, Vec2::Y, Vec2::NEG_X, Vec2::X] {
                        let (width, height) = if direction.x == 0.0 {
                            (BAR_THIN, BAR_LONG)
                        } else {
                            (BAR_LONG, BAR_THIN)
                        };
                        reticle.spawn((
                            ReticlePart,
                            ReticleBar(direction),
                            Node {
                                position_type: PositionType::Absolute,
                                width: px(width),
                                height: px(height),
                                border_radius: BorderRadius::all(px(1.0)),
                                ..default()
                            },
                            BackgroundColor(TEXT),
                            reticle_shadow(),
                            Pickable::IGNORE,
                        ));
                    }
                });
        });
}

/// Where a bar's top-left corner goes for `gap`, in the reticle's box.
fn bar_corner(direction: Vec2, gap: f32) -> Vec2 {
    let centre = RETICLE_BOX * 0.5;
    let reach = gap + BAR_LONG * 0.5;
    let middle = Vec2::splat(centre) + direction * reach;
    let size = if direction.x == 0.0 {
        Vec2::new(BAR_THIN, BAR_LONG)
    } else {
        Vec2::new(BAR_LONG, BAR_THIN)
    };
    middle - size * 0.5
}

/// `Update`: the bars close in while aiming and bloom with the recoil,
/// and the whole reticle turns warm over a standing hostile.
#[allow(
    clippy::too_many_arguments,
    reason = "the clock, the feedback, the gates and the reticle's own parts"
)]
fn update_reticle(
    time: Res<Time>,
    feedback: Res<Feedback>,
    stack: Option<Res<ScreenStack>>,
    layers: Option<Res<HudLayers>>,
    local: Query<&Combatant, With<LocalPlayer>>,
    riding: Query<(), (With<LocalPlayer>, With<veyra_sim::ride::Riding>)>,
    actors: Query<&Combatant>,
    mut aim: Local<f32>,
    cameras: Query<&Projection, With<crate::combat::CombatCamera>>,
    windows: Query<&Window>,
    ui_scale_res: Option<Res<bevy::ui::UiScale>>,
    mut reticles: Query<&mut Visibility, With<Reticle>>,
    mut bars: Query<(&ReticleBar, &mut Node)>,
    mut parts: Query<&mut BackgroundColor, With<ReticlePart>>,
) {
    let actor = local.iter().next();
    // A rider cannot fire, so the reticle has nothing to say.
    let shown = world_ui_shown(stack, is_down(actor)) && crosshair_on(layers) && riding.is_empty();
    for mut visibility in &mut reticles {
        let wanted = if shown {
            Visibility::Inherited
        } else {
            Visibility::Hidden
        };
        if *visibility != wanted {
            *visibility = wanted;
        }
    }
    let target = if actor.is_some_and(|a| a.aiming) {
        1.0
    } else {
        0.0
    };
    *aim += (target - *aim) * (1.0 - (-RETICLE_EASE * time.delta_secs()).exp());
    if !shown {
        return;
    }
    let share = actor.map_or(*aim, |actor| reticle_share(actor, *aim));
    let mut gap = reticle_gap(share, feedback.recoil);
    // The cone is the resting gap, never under the readable one; the
    // recoil bloom adds on top as before.
    let cone = actor.zip(cameras.iter().next()).zip(windows.iter().next());
    if let Some(((actor, Projection::Perspective(lens)), window)) = cone {
        let (hip, aimed) = (actor.cone_when(false), actor.cone_when(true));
        let half = hip + (aimed - hip) * share;
        let rest = reticle_gap(share, 0.0);
        let cone = cone_gap(
            half,
            lens.fov,
            window.height(),
            ui_scale(ui_scale_res.as_deref()),
        );
        gap += cone.max(rest) - rest;
    }
    for (bar, mut node) in &mut bars {
        let corner = bar_corner(bar.0, gap);
        let (left, top) = (px(corner.x), px(corner.y));
        if node.left != left || node.top != top {
            node.left = left;
            node.top = top;
        }
    }
    let team = actor.map_or(0, |a| a.team);
    let hostile = feedback
        .under_reticle
        .and_then(|entity| actors.get(entity).ok())
        .is_some_and(|other| {
            veyra_sim::encounters::enemies(other.team, team) && other.action != Action::Dead
        });
    let colour = if hostile { WARM } else { TEXT };
    for mut background in &mut parts {
        if background.0 != colour {
            background.0 = colour;
        }
    }
}

// ---------------------------------------------------------------------------
// The down card
// ---------------------------------------------------------------------------

/// How long the scrim and the card take to come up, seconds.
pub const DOWN_FADE_SECS: f32 = 0.4;

/// The card's strength `t` seconds after the fall: an ease-out over
/// [`DOWN_FADE_SECS`], so most of it is there at once and it settles.
#[must_use]
pub fn down_alpha(t: f32) -> f32 {
    let x = (t / DOWN_FADE_SECS).clamp(0.0, 1.0);
    1.0 - (1.0 - x).powi(3)
}

/// The card's root: the full-window scrim.
#[derive(Component)]
struct DownCard;

/// Every painted node of the card, which fades together.
#[derive(Component)]
struct DownPart;

/// The character's name on the card.
#[derive(Component)]
struct DownName;

/// The card itself, which settles from a touch smaller as it fades in.
#[derive(Component)]
struct DownPanel;

/// One line of the card's text in `role`.
fn down_text(words: &str, role: &'static str) -> impl Bundle {
    (
        DownPart,
        Painted::new(role),
        Text::new(words),
        TextLayout::no_wrap(),
        Pickable::IGNORE,
    )
}

/// The down card, hidden: a scrim over the window and a glass card at its
/// centre saying who fell and how to get up.
fn spawn_down_card(commands: &mut Commands) {
    commands
        .spawn((
            DownCard,
            DownPart,
            Name::new("hud.down"),
            Painted::new("scrim"),
            Node {
                position_type: PositionType::Absolute,
                width: percent(100),
                height: percent(100),
                align_items: AlignItems::Center,
                justify_content: JustifyContent::Center,
                ..default()
            },
            GlobalZIndex(DOWN_Z),
            Pickable::IGNORE,
            Visibility::Hidden,
        ))
        .with_children(|scrim| {
            scrim
                .spawn((
                    DownPanel,
                    DownPart,
                    Painted::new("strata.down.card"),
                    Node {
                        flex_direction: FlexDirection::Column,
                        align_items: AlignItems::Center,
                        padding: UiRect::axes(px(44.0), px(22.0)),
                        row_gap: px(2.0),
                        border: UiRect::all(px(1.0)),
                        ..default()
                    },
                    UiTransform::default(),
                    Pickable::IGNORE,
                ))
                .with_children(|card| {
                    card.spawn(down_text(english("strata.down.title"), "strata.down.title"));
                    card.spawn((
                        DownName,
                        down_text(english("strata.vitals.fallback_name"), "strata.down.name"),
                    ));
                    card.spawn((
                        Node {
                            flex_direction: FlexDirection::Row,
                            align_items: AlignItems::Center,
                            column_gap: px(8.0),
                            margin: UiRect::top(px(14.0)),
                            ..default()
                        },
                        Pickable::IGNORE,
                    ))
                    .with_children(|hint| {
                        hint.spawn((
                            DownPart,
                            Painted::new("hint.glyph"),
                            Node {
                                min_width: px(24.0),
                                padding: UiRect::axes(px(7.0), px(1.5)),
                                justify_content: JustifyContent::Center,
                                ..default()
                            },
                            Pickable::IGNORE,
                        ))
                        .with_children(|glyph| {
                            glyph.spawn(down_text(english("strata.down.key"), "hint.glyph.text"));
                        });
                        hint.spawn(down_text(
                            english("strata.down.retry"),
                            "strata.controls.label",
                        ));
                    });
                });
        });
}

/// `Update`: the card comes up over [`DOWN_FADE_SECS`] when the local
/// player falls and goes the frame they stand again; a screen hides it.
#[allow(
    clippy::too_many_arguments,
    clippy::type_complexity,
    reason = "the clock, the gate, the player and the card's own parts"
)]
fn update_down_card(
    time: Res<Time>,
    stack: Option<Res<ScreenStack>>,
    local: Query<(&Combatant, Option<&Name>), With<LocalPlayer>>,
    mut since: Local<Option<f32>>,
    mut roots: Query<&mut Visibility, With<DownCard>>,
    mut parts: Query<&mut Painted, With<DownPart>>,
    mut panels: Query<&mut UiTransform, With<DownPanel>>,
    mut names: Query<&mut Text, With<DownName>>,
) {
    let actor = local.iter().next();
    let down = is_down(actor.map(|(actor, _)| actor));
    *since = if down {
        Some(since.map_or(0.0, |t| t + time.delta_secs()))
    } else {
        None
    };
    let shown = down && hud_shown(stack);
    for mut visibility in &mut roots {
        let wanted = if shown {
            Visibility::Inherited
        } else {
            Visibility::Hidden
        };
        if *visibility != wanted {
            *visibility = wanted;
        }
    }
    let Some(t) = *since else {
        return;
    };
    let alpha = down_alpha(t);
    for mut part in &mut parts {
        if (part.alpha - alpha).abs() > f32::EPSILON {
            part.alpha = alpha;
        }
    }
    for mut transform in &mut panels {
        transform.scale = Vec2::splat(0.96 + 0.04 * alpha);
    }
    if let Some((actor, name)) = actor {
        let wanted = vitals(actor, name.map(Name::as_str), None).name;
        for mut text in &mut names {
            if text.0 != wanted {
                text.0.clone_from(&wanted);
            }
        }
    }
}

/// What one live node of the cluster should say and wear.
#[derive(Debug, Default, PartialEq)]
struct Want {
    text: Option<String>,
    role: Option<&'static str>,
    /// A fill's width, percent of its track.
    width: Option<f32>,
    shown: Option<bool>,
}

/// The node `id` of the cluster, given `v` and whether the low-health
/// pulse is in its danger half; `None` for a node that never changes.
fn want(id: &str, v: &Vitals, pulse: bool) -> Option<Want> {
    let mut want = Want::default();
    match id {
        ids::NAME => want.text = Some(v.name.clone()),
        ids::WEAPON => want.text = Some(v.weapon.to_owned()),
        ids::UNCERTIFIED => want.shown = Some(v.uncertified),
        ids::ENCOUNTER => {
            want.text = Some(v.encounter.map_or("", |e| english(e.key())).to_owned());
            want.role = Some(
                v.encounter
                    .map_or("strata.vitals.encounter.quiet", EncounterWord::role),
            );
        }
        ids::SLOT_ONE | ids::SLOT_TWO => {
            let slot = usize::from(id == ids::SLOT_TWO);
            want.text = Some(v.slots[slot].words(slot));
            want.role = Some(v.slots[slot].role());
        }
        ids::AMMO => {
            want.text = Some(v.ammo.to_string());
            want.role = Some(v.ammo_tone.role());
            want.shown = Some(!v.melee);
        }
        ids::MAGAZINE => {
            want.text = Some(
                english("strata.vitals.magazine").replace("{magazine}", &v.magazine.to_string()),
            );
            want.shown = Some(!v.melee);
        }
        ids::CONDITION => {
            want.text = Some(
                english(match v.condition {
                    Band::Sound => "strata.vitals.condition.sound",
                    Band::Tired => "strata.vitals.condition.tired",
                    Band::Worn => "strata.vitals.condition.worn",
                })
                .to_owned(),
            );
            want.shown = Some(v.melee);
        }
        ids::RELOAD => want.shown = Some(v.reload_hint),
        ids::WEAR | ids::WEAR_CHIP => want.shown = Some(v.wear.is_some()),
        ids::RIDE => want.shown = Some(v.ride.is_some()),
        ids::SPEED => {
            want.text = v.ride.map(RideReading::words);
            want.role = v.ride.map(RideReading::role);
        }
        ids::WEAR_FILL => want.width = v.wear.map(WearReading::percent),
        ids::WEAR_WORD => {
            let key = if v.wear.is_some_and(|w| w.band == Band::Worn) {
                "strata.vitals.worn"
            } else {
                "strata.vitals.tired"
            };
            want.text = Some(english(key).replace("{weapon}", v.weapon));
        }
        ids::ACTION => {
            want.text = Some(v.action.map_or("", |a| english(a.key)).to_owned());
            want.role = Some(v.action.map_or("strata.vitals.action", ActionWord::role));
        }
        id if id == Meter::Stamina.label() => {
            want.text = Some(
                english(if v.guarding {
                    "strata.vitals.guarding"
                } else {
                    Meter::Stamina.key()
                })
                .to_owned(),
            );
        }
        _ => {
            let meter = Meter::ALL
                .into_iter()
                .find(|m| m.fill() == id || m.value() == id)?;
            let pool = v.pool(meter);
            if meter.value() == id {
                want.text = Some(pool.figure());
                if meter == Meter::Health {
                    want.role = Some(if v.health_low() && pulse {
                        "strata.vitals.value.danger"
                    } else {
                        "strata.vitals.value"
                    });
                }
            } else {
                want.width = Some(pool.fraction() * 100.0);
                // An empty fill would still show its border's two pixels.
                want.shown = Some(pool.fraction() > 0.0);
                if meter == Meter::Health {
                    want.role = Some(v.health_role());
                }
            }
        }
    }
    Some(want)
}

/// `Update`: the cluster follows the local player. Every write checks
/// first, so a frame where nothing changed touches no layout. Low on
/// health, the figure swaps between its danger role and its plain one
/// with the vignette's pulse; down, it stays danger.
fn update_vitals(
    time: Res<Time>,
    actors: Query<
        (
            &Combatant,
            Option<&Name>,
            Option<&Hands>,
            Option<&Riding>,
            Option<&Calling>,
        ),
        With<LocalPlayer>,
    >,
    encounter: Option<Res<CombatEncounter>>,
    registries: Option<Res<Registries>>,
    index: Res<TestIdIndex>,
    mut nodes: Query<(Option<&mut Text>, Option<&mut Themed>, &mut Node)>,
) {
    let Ok((actor, name, hands, riding, calling)) = actors.single() else {
        return;
    };
    let mut v = vitals(actor, name.map(Name::as_str), encounter.as_deref());
    v.ride = match (riding, calling) {
        (Some(riding), _) => Some(RideReading::of_speed(riding.speed)),
        (None, Some(_)) => Some(RideReading::Calling),
        (None, None) => None,
    };
    v.wear = registries
        .as_deref()
        .zip(hands)
        .and_then(|(registries, hands)| {
            let items = &registries.0.items;
            let record = veyra_sim::items::read(hands.0.get(actor.hand())?, items).ok()?;
            let condition = record.condition?;
            WearReading::of(items.wear(), condition)
        });
    v.condition = v.wear.map_or(Band::Sound, |wear| wear.band);
    let pulse = actor.action == Action::Dead || pulse_danger(time.elapsed_secs());
    for (id, entity) in index.with_prefix("vitals.") {
        let Ok((text, themed, mut node)) = nodes.get_mut(entity) else {
            continue;
        };
        let Some(want) = want(id, &v, pulse) else {
            continue;
        };
        if let (Some(wanted), Some(mut text)) = (want.text, text)
            && text.0 != wanted
        {
            text.0 = wanted;
        }
        if let (Some(role), Some(mut themed)) = (want.role, themed)
            && themed.0.as_str() != role
        {
            *themed = Themed(Role::new_static(role));
        }
        if let Some(width) = want.width {
            let width = percent(width);
            if node.width != width {
                node.width = width;
            }
        }
        if let Some(shown) = want.shown {
            let display = if shown { Display::Flex } else { Display::None };
            if node.display != display {
                node.display = display;
            }
        }
    }
}

/// `Update`: going down is said once, as a toast, with what it cost.
fn knocked_toast(mut notices: MessageReader<WearNotice>, mut commands: Commands) {
    for WearNotice::Knocked { points } in notices.read() {
        veyra_ui::progress::notice(
            &mut commands,
            english("strata.vitals.knocked").replace("{points}", &points.to_string()),
        );
    }
}

/// The controls card's word for the trigger of a weapon: a burst that
/// goes on firing when held says so, anything else is plain "Fire".
#[must_use]
pub fn fire_label_key(spec: &WeaponSpec) -> &'static str {
    if matches!(spec.mode, FireMode::Burst { .. }) && spec.hold.is_some() {
        "strata.controls.fire.burst"
    } else {
        "strata.controls.fire"
    }
}

/// The controls card's word for the left button with `weapon` in hand: a
/// melee weapon cuts with it.
#[must_use]
pub fn trigger_key(weapon: Weapon) -> &'static str {
    if weapon.is_melee() {
        "strata.controls.cut"
    } else {
        fire_label_key(&weapon.spec())
    }
}

/// The controls card's word for the right button with `weapon` in hand: a
/// melee weapon raises its guard with it, a gun aims.
#[must_use]
pub fn aim_key(weapon: Weapon) -> &'static str {
    if weapon.is_melee() {
        "strata.controls.guard"
    } else {
        "strata.controls.aim"
    }
}

/// `Update`: the card's buttons follow the weapon in hand: the trigger's
/// and the right button's words, the reload entry for a gun only, the
/// heavy cut for a melee weapon only. Written only when it differs.
fn update_controls_fire(
    actors: Query<&Combatant, With<LocalPlayer>>,
    index: Res<TestIdIndex>,
    mut nodes: Query<(Option<&mut Text>, &mut Node)>,
) {
    let Ok(actor) = actors.single() else {
        return;
    };
    let melee = actor.weapon.is_melee();
    let wanted = english(trigger_key(actor.weapon));
    let aim = english(aim_key(actor.weapon));
    let rows = [
        ids::CONTROLS_FIRE,
        ids::CONTROLS_AIM_LABEL,
        ids::CONTROLS_RELOAD,
        ids::CONTROLS_HEAVY,
    ];
    for (id, entity) in rows
        .into_iter()
        .flat_map(|id| index.entities(id).iter().map(move |entity| (id, *entity)))
    {
        let Ok((text, mut node)) = nodes.get_mut(entity) else {
            continue;
        };
        let shown = match id {
            ids::CONTROLS_FIRE | ids::CONTROLS_AIM_LABEL => {
                let word: &str = if id == ids::CONTROLS_FIRE {
                    wanted
                } else {
                    aim
                };
                if let Some(mut text) = text
                    && text.0 != word
                {
                    word.clone_into(&mut text.0);
                }
                continue;
            }
            ids::CONTROLS_RELOAD => !melee,
            _ => melee,
        };
        let display = if shown { Display::Flex } else { Display::None };
        if node.display != display {
            node.display = display;
        }
    }
}

/// `Update`: F1 flips the card.
fn toggle_controls(keys: Res<ButtonInput<KeyCode>>, mut card: ResMut<ControlsCard>) {
    if !keys.just_pressed(CONTROLS_KEY) {
        return;
    }
    if card.shown() {
        *card = ControlsCard::default();
    } else {
        card.pinned = true;
    }
}

/// `Update`: the card comes up when the local player appears and when a
/// bench swap renames them, runs its clock down, and the layer follows;
/// it stands aside while the player is down.
fn show_controls(
    time: Res<Time>,
    mut card: ResMut<ControlsCard>,
    arrived: Query<(), (With<LocalPlayer>, Or<(Added<LocalPlayer>, Changed<Name>)>)>,
    local: Query<&Combatant, With<LocalPlayer>>,
    layers: Option<ResMut<HudLayers>>,
) {
    if !arrived.is_empty() {
        card.left = CONTROLS_SECS;
    } else if card.left > 0.0 {
        card.left = (card.left - time.delta_secs()).max(0.0);
    }
    let Some(mut layers) = layers else {
        return;
    };
    let shown = card.shown() && !is_down(local.iter().next());
    let id = controls_layer();
    if layers.get(&id).is_some_and(|def| def.visible != shown) {
        layers.set_visible(&id, shown);
    }
}

/// `Update`: the ticks pop out a little on a hit and settle as they fade
/// with [`Feedback::hit`]; a lethal hit turns them danger.
fn update_hit_marker(
    feedback: Res<Feedback>,
    stack: Option<Res<ScreenStack>>,
    local: Query<&Combatant, With<LocalPlayer>>,
    mut markers: Query<(&mut Visibility, &mut UiTransform), With<HitMarker>>,
    mut ticks: Query<&mut BackgroundColor, With<HitTick>>,
) {
    let strength = feedback.hit.clamp(0.0, 1.0);
    let shown = strength > 0.02 && world_ui_shown(stack, is_down(local.iter().next()));
    for (mut visibility, mut transform) in &mut markers {
        let wanted = if shown {
            Visibility::Inherited
        } else {
            Visibility::Hidden
        };
        if *visibility != wanted {
            *visibility = wanted;
        }
        if shown {
            transform.scale = Vec2::splat(1.0 + 0.35 * strength);
        }
    }
    if !shown {
        return;
    }
    let colour = if feedback.lethal { DANGER } else { ACCENT };
    for mut background in &mut ticks {
        background.0 = colour.with_alpha(strength);
    }
}

/// How dark the vignette's edge gets at a fresh hurt.
const VIGNETTE_ALPHA: f32 = 0.45;

/// The vignette's own decay, per second. [`Feedback::hurt`] falls at
/// eighteen a second for the camera's twitch, which on the window's
/// edge is a 150 ms blink too short to read as "hurt"; the edge holds
/// its own level and lets go over about half a second.
const VIGNETTE_DECAY: f32 = 4.0;

/// The low-health pulse's rate, Hz: a little faster than a resting
/// heart, slow enough to read as a throb and not a flicker.
pub const PULSE_HZ: f32 = 1.1;
/// The vignette's strength at the pulse's trough and crest: under the
/// hit flash's strength, so a fresh hurt still reads over it.
pub const PULSE_ALPHA: (f32, f32) = (0.10, 0.22);

/// Where the low-health pulse is at `t` seconds, `0..=1`: a sine, so it
/// eases in and out of both ends.
#[must_use]
pub fn pulse(t: f32) -> f32 {
    0.5 - 0.5 * (t * PULSE_HZ * std::f32::consts::TAU).cos()
}

/// The vignette's strength from the low-health pulse at `t`.
#[must_use]
pub fn pulse_alpha(t: f32) -> f32 {
    PULSE_ALPHA.0 + (PULSE_ALPHA.1 - PULSE_ALPHA.0) * pulse(t)
}

/// Whether the pulse is in its upper half at `t`, when the health figure
/// wears danger: the swap lands with the vignette's crest.
#[must_use]
pub fn pulse_danger(t: f32) -> bool {
    pulse(t) >= 0.5
}

/// The vignette's strength: the hit flash's envelope `level`, or, low on
/// health and standing, the pulse at `t` if that is stronger.
#[must_use]
pub fn vignette_alpha(level: f32, low: bool, t: f32) -> f32 {
    let flash = level.clamp(0.0, 1.0) * VIGNETTE_ALPHA;
    if low {
        flash.max(pulse_alpha(t))
    } else {
        flash
    }
}

/// `Update`: the window's edge reddens on a hurt and clears over its own
/// envelope, and throbs while health is low; the centre, where the aim
/// is, stays clear.
fn update_vignette(
    time: Res<Time>,
    feedback: Res<Feedback>,
    stack: Option<Res<ScreenStack>>,
    local: Query<&Combatant, With<LocalPlayer>>,
    mut level: Local<f32>,
    mut vignettes: Query<(&mut Visibility, &mut BackgroundGradient), With<HurtVignette>>,
) {
    *level = (*level * (-time.delta_secs() * VIGNETTE_DECAY).exp()).max(feedback.hurt);
    let actor = local.iter().next();
    let low = actor.is_some_and(|actor| {
        actor.action != Action::Dead
            && actor.with_actor(|table| actor.health < table.health * LOW_HEALTH)
    });
    let alpha = vignette_alpha(*level, low, time.elapsed_secs());
    let shown = alpha > 0.005 && hud_shown(stack);
    for (mut visibility, mut gradient) in &mut vignettes {
        let wanted = if shown {
            Visibility::Inherited
        } else {
            Visibility::Hidden
        };
        if *visibility != wanted {
            *visibility = wanted;
        }
        if shown {
            *gradient = vignette(alpha);
        }
    }
}

/// The gradient: an ellipse that meets each edge of the window at its
/// midpoint, clear over the middle half where the aim and the body are,
/// reddening towards the edges and deepest in the corners.
fn vignette(alpha: f32) -> BackgroundGradient {
    BackgroundGradient(vec![Gradient::Radial(RadialGradient::new(
        UiPosition::CENTER,
        RadialGradientShape::Ellipse(percent(50), percent(50)),
        vec![
            ColorStop::percent(DANGER.with_alpha(0.0), 60),
            ColorStop::percent(DANGER.with_alpha(alpha * 0.6), 100),
            ColorStop::percent(DANGER.with_alpha(alpha), 140),
        ],
    ))])
}

// The hurt's aberration (lighting plan phase 4b).
/// The chromatic aberration's strength, from 0 to the look's `aberration`:
/// the hurt's envelope `level` times the look's, and while down at least
/// half of it, so the fall leaves the world a little out of true under the
/// card while a fresh hit still reads over it.
#[must_use]
pub fn aberration_strength(aberration: f32, level: f32, down: bool) -> f32 {
    let level = level.clamp(0.0, 1.0);
    aberration * if down { level.max(0.5) } else { level }
}

/// `Update`: the camera's chromatic aberration follows the hurt.
///
/// On the vignette's envelope ([`VIGNETTE_DECAY`]), not [`Feedback::hurt`]
/// itself, which falls at eighteen a second for the camera's twitch: on
/// the frame's edges that is a flicker, where the slower envelope reads as
/// a knock. A camera without the component (headless, or the look's
/// aberration at zero) is left alone.
fn update_aberration(
    time: Res<Time>,
    feedback: Res<Feedback>,
    look: Option<Res<strata_atmosphere::look::RenderLook>>,
    local: Query<&Combatant, With<LocalPlayer>>,
    mut level: Local<f32>,
    mut lenses: Query<&mut strata_atmosphere::exposure::ChromaticAberration>,
) {
    let Some(look) = look else {
        return;
    };
    if lenses.is_empty() {
        return;
    }
    *level = (*level * (-time.delta_secs() * VIGNETTE_DECAY).exp()).max(feedback.hurt);
    let strength = aberration_strength(look.aberration, *level, is_down(local.iter().next()));
    let intensity = strata_atmosphere::exposure::aberration_intensity(strength);
    for mut lens in &mut lenses {
        if (lens.intensity - intensity).abs() > 1e-6 {
            lens.intensity = intensity;
        }
    }
}

// The hurt's aberration (lighting plan phase 4b).
#[cfg(test)]
mod aberration_tests {
    use super::aberration_strength;

    #[test]
    fn a_hurt_reaches_the_strength_the_fall_holds_half_and_rest_is_clear() {
        let close = |a: f32, b: f32| (a - b).abs() < 1e-6;
        assert!(close(aberration_strength(0.6, 1.0, false), 0.6));
        assert!(close(aberration_strength(0.6, 0.0, true), 0.3));
        assert!(close(aberration_strength(0.6, 0.0, false), 0.0));
        // The lethal hit reads over the fall's floor, then settles on it.
        assert!(close(aberration_strength(0.6, 1.0, true), 0.6));
        assert!(close(aberration_strength(0.6, 0.2, true), 0.3));
        // A level over one (a stacked hurt) is the strength, not more.
        assert!(close(aberration_strength(0.6, 3.0, false), 0.6));
        assert!(close(aberration_strength(0.0, 1.0, true), 0.0));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use slotted::theme::{Paint, Theme};

    fn encounter(ready: bool, engaged: bool, remaining: usize) -> CombatEncounter {
        CombatEncounter {
            ready,
            engaged,
            remaining,
        }
    }

    #[test]
    fn the_name_falls_back_and_the_weapon_is_named() {
        let actor = Combatant::default();
        let v = vitals(&actor, None, None);
        assert_eq!(v.name, "Ranger");
        assert_eq!(v.weapon, "CARBINE");
        assert_eq!(vitals(&actor, Some("  "), None).name, "Ranger");
        assert_eq!(
            vitals(&actor, Some("Tovan Reyes"), None).name,
            "Tovan Reyes"
        );
        assert_eq!(v.encounter, None, "no encounter, no word");
        assert_eq!(v.action, None);
    }

    #[test]
    fn an_uncertified_weapon_wears_the_chip() {
        use strata_registry::progression::SkillGraph;
        use veyra_sim::progression::CharacterSheet;
        assert!(
            !vitals(&Combatant::default(), None, None).uncertified,
            "an actor with no sheet may fire anything"
        );
        let graph = SkillGraph::shipped();
        let mut sheet = CharacterSheet::default();
        sheet.learned.insert("blade.novice".to_owned());
        let mut actor = Combatant::default();
        actor.fit(&graph, &sheet);
        assert!(!actor.certified(actor.weapon), "the fixture's premise");
        assert!(vitals(&actor, None, None).uncertified);
    }

    #[test]
    fn the_encounter_has_four_words() {
        let word = |e: CombatEncounter| {
            let w = EncounterWord::of(&e);
            (english(w.key()), w.role())
        };
        assert_eq!(
            word(encounter(false, false, 3)),
            ("STREAMING", "strata.vitals.encounter.quiet")
        );
        assert_eq!(
            word(encounter(true, false, 3)),
            ("HOSTILES AHEAD", "strata.vitals.encounter.quiet")
        );
        assert_eq!(
            word(encounter(true, true, 2)),
            ("ENGAGED", "strata.vitals.encounter.engaged")
        );
        assert_eq!(
            word(encounter(true, true, 0)),
            ("AREA CLEAR", "strata.vitals.encounter.clear")
        );
    }

    #[test]
    fn a_quarter_magazine_is_low_and_none_is_empty() {
        assert_eq!(AmmoTone::of(24, 24), AmmoTone::Plenty);
        assert_eq!(AmmoTone::of(7, 24), AmmoTone::Plenty);
        assert_eq!(AmmoTone::of(6, 24), AmmoTone::Low);
        assert_eq!(AmmoTone::of(1, 24), AmmoTone::Low);
        assert_eq!(AmmoTone::of(0, 24), AmmoTone::Empty);
        let mut actor = Combatant::default();
        let magazine = actor.magazine();
        actor.ammo = magazine / 4;
        let v = vitals(&actor, None, None);
        assert_eq!(v.ammo_tone, AmmoTone::Low);
        assert!(v.reload_hint);
        actor.action = Action::Reload;
        let v = vitals(&actor, None, None);
        assert!(!v.reload_hint, "already reloading: the word says so");
        assert_eq!(v.action.map(|a| english(a.key)), Some("RELOADING"));
    }

    #[test]
    fn health_turns_danger_below_thirty_percent() {
        let mut actor = Combatant::default();
        let max = actor.health;
        actor.health = max * 0.31;
        assert_eq!(
            vitals(&actor, None, None).health_role(),
            "strata.vitals.health"
        );
        actor.health = max * 0.29;
        let v = vitals(&actor, None, None);
        assert!(v.health_low());
        assert_eq!(v.health_role(), "strata.vitals.health.low");
        actor.health = 0.4;
        assert_eq!(vitals(&actor, None, None).health.figure(), "1");
    }

    #[test]
    fn the_shield_is_out_of_its_capacity() {
        let mut actor = Combatant::default();
        actor.shield_capacity = 80.0;
        actor.shield = 20.0;
        let v = vitals(&actor, None, None);
        assert!((v.shield.fraction() - 0.25).abs() < 1e-6);
    }

    #[test]
    fn cuts_say_nothing_and_the_fall_says_how_to_get_up() {
        for action in [
            Action::Ready,
            Action::Light,
            Action::Followup,
            Action::Heavy,
            Action::Dead,
        ] {
            assert_eq!(ActionWord::of(action), None, "{action:?}");
        }
        assert!(ActionWord::of(Action::Stagger).is_some_and(|w| w.danger));
        assert!(ActionWord::of(Action::Dodge).is_some_and(|w| !w.danger));
    }

    #[test]
    fn every_word_and_label_has_english() {
        for key in Meter::ALL.map(Meter::key) {
            assert!(veyra_ui::strings::english(key).is_some(), "{key}");
        }
        for column in CONTROLS {
            for (key, label) in column {
                assert!(veyra_ui::strings::english(key).is_some(), "{key}");
                assert!(veyra_ui::strings::english(label).is_some(), "{label}");
            }
        }
        assert!(vitals_layer_def().hide_with_screen);
        assert!(controls_layer_def().hide_with_screen);
        assert!(!controls_layer_def().visible, "raised by an arrival");
    }

    #[test]
    fn the_controls_card_names_the_bike_key() {
        let entry = CONTROLS
            .iter()
            .flat_map(|column| column.iter())
            .find(|(_, label)| *label == "strata.controls.ride")
            .expect("the card has the bike");
        assert_eq!(veyra_ui::strings::english(entry.0), Some("V"));
        assert_eq!(veyra_ui::strings::english(entry.1), Some("Bike"));
    }

    #[test]
    fn a_speed_reads_in_kilometres_an_hour() {
        assert_eq!(RideReading::of_speed(30.0).words(), "108 km/h");
        assert_eq!(RideReading::of_speed(-4.0).words(), "14 km/h");
        assert_eq!(RideReading::Calling.words(), "Calling the bike");
    }

    #[test]
    fn the_reticle_closes_when_aiming_and_blooms_with_recoil() {
        let close = |a: f32, b: f32| (a - b).abs() < 1e-5;
        assert!(close(reticle_gap(0.0, 0.0), 10.0));
        assert!(close(reticle_gap(1.0, 0.0), 5.0));
        assert!(close(reticle_gap(0.5, 0.0), 7.5), "eased between");
        assert!(close(reticle_gap(0.0, RECOIL_FULL), 18.0), "full bloom");
        assert!(close(reticle_gap(0.0, 10.0), 18.0), "never past it");
        assert!(close(reticle_gap(1.0, RECOIL_FULL * 0.5), 9.0));
        assert!(close(reticle_gap(0.0, -1.0), 10.0));
        // The bars sit on the axes, their inner ends `gap` from the centre.
        let centre = RETICLE_BOX * 0.5;
        let top = bar_corner(Vec2::NEG_Y, 10.0);
        assert!(close(top.x, centre - 1.0) && close(top.y + BAR_LONG, centre - 10.0));
        let right = bar_corner(Vec2::X, 5.0);
        assert!(close(right.x, centre + 5.0) && close(right.y, centre - 1.0));
    }

    #[test]
    fn the_cone_becomes_a_gap_in_ui_pixels() {
        let close = |a: f32, b: f32| (a - b).abs() < 1e-3;
        let fov = 90f32.to_radians();
        // At 90 degrees tall a 45 degree half angle fills half the height.
        assert!(close(cone_gap(45.0, fov, 800.0, 1.0), 400.0));
        assert!(close(cone_gap(45.0, fov, 800.0, 2.0), 200.0), "UI units");
        assert!(close(cone_gap(0.0, fov, 800.0, 1.0), 0.0));
        assert!(cone_gap(1.2, fov, 800.0, 1.0) > cone_gap(0.35, fov, 800.0, 1.0));
    }

    #[test]
    fn the_reticle_gap_grows_with_the_bloom_and_settles() {
        use veyra_sim::PlayerIntent;
        use veyra_sim::combat::{CombatIntent, CombatMotion};
        let fov = 90f32.to_radians();
        let gap = |actor: &Combatant| cone_gap(actor.cone_when(false), fov, 800.0, 1.0);
        let mut actor = Combatant::default();
        actor.weapon = Weapon::Carbine;
        actor.ammo = 1000;
        let rest = gap(&actor);
        let movement = PlayerIntent::default();
        let mut motion = CombatMotion::default();
        let dt = 1.0 / 60.0;
        let mut held = CombatIntent {
            fire: true,
            ..CombatIntent::default()
        };
        for _ in 0..60 {
            actor.step(&mut held, &movement, dt, &mut motion);
        }
        let bloomed = gap(&actor);
        assert!(bloomed > rest, "{bloomed} against {rest}");
        let mut off = CombatIntent::default();
        for _ in 0..240 {
            actor.step(&mut off, &movement, dt, &mut motion);
        }
        assert!((gap(&actor) - rest).abs() < 1e-3, "it settles back");
    }

    #[test]
    fn the_reticle_gap_follows_the_cone_as_a_rifles_aim_closes() {
        use veyra_sim::PlayerIntent;
        use veyra_sim::combat::{CombatIntent, CombatMotion};
        let fov = 90f32.to_radians();
        let mut actor = Combatant::default().carrying([Some(Weapon::Rifle), None], 0);
        let gap = |actor: &Combatant| {
            let (hip, aimed) = (actor.cone_when(false), actor.cone_when(true));
            let half = hip + (aimed - hip) * reticle_share(actor, 1.0);
            cone_gap(half, fov, 800.0, 1.0)
        };
        let hip = gap(&actor);
        let mut motion = CombatMotion::default();
        let mut previous = hip;
        let mut shrank = false;
        for _ in 0..15 {
            let mut held = CombatIntent {
                aim: true,
                ..CombatIntent::default()
            };
            actor.step(&mut held, &PlayerIntent::default(), 1.0 / 60.0, &mut motion);
            let now = gap(&actor);
            assert!(now <= previous + 1e-4, "it never opens while aim is held");
            shrank |= now < previous - 1e-3;
            previous = now;
        }
        assert!(shrank && previous > cone_gap(actor.cone_when(true), fov, 800.0, 1.0));
        assert!(
            previous < hip,
            "a quarter second in the gap is between the two ends"
        );
        for _ in 0..90 {
            let mut held = CombatIntent {
                aim: true,
                ..CombatIntent::default()
            };
            actor.step(&mut held, &PlayerIntent::default(), 1.0 / 60.0, &mut motion);
        }
        let aimed = cone_gap(actor.cone_when(true), fov, 800.0, 1.0);
        assert!((gap(&actor) - aimed).abs() < 1e-3);
    }

    #[test]
    fn the_chips_name_each_slot_and_mark_the_one_in_hand() {
        use Weapon::{Blade, Carbine, Glaive, Pistol};
        for (slots, hand, words) in [
            ([Carbine, Pistol], 0, ["> 1  CARBINE", "2  PISTOL"]),
            ([Carbine, Pistol], 1, ["1  CARBINE", "> 2  PISTOL"]),
            ([Blade, Pistol], 0, ["> 1  BLADE", "2  PISTOL"]),
            ([Pistol, Glaive], 1, ["1  PISTOL", "> 2  GLAIVE"]),
        ] {
            let actor = Combatant::default().carrying(slots.map(Some), hand);
            let v = vitals(&actor, None, None);
            for (slot, chip) in v.slots.iter().enumerate() {
                assert_eq!(chip.words(slot), words[slot]);
                assert_eq!(chip.in_hand, slot == hand);
            }
            assert_ne!(
                v.slots[0].role(),
                v.slots[1].role(),
                "the one in hand is also brighter"
            );
        }
        let alone = Combatant::default().carrying([Some(Blade), None], 0);
        assert_eq!(vitals(&alone, None, None).slots[1].words(1), "2  EMPTY");
    }

    #[test]
    fn a_melee_weapon_has_no_ammo_row_and_a_condition_word() {
        let gun = vitals(&Combatant::default(), None, None);
        let blade = vitals(
            &Combatant::default().carrying([Some(Weapon::Blade), Some(Weapon::Carbine)], 0),
            None,
            None,
        );
        let shown = |v: &Vitals, id| want(id, v, false).and_then(|w| w.shown);
        assert_eq!(shown(&gun, ids::AMMO), Some(true));
        assert_eq!(shown(&gun, ids::CONDITION), Some(false));
        assert_eq!(shown(&blade, ids::AMMO), Some(false));
        assert_eq!(shown(&blade, ids::MAGAZINE), Some(false));
        assert_eq!(shown(&blade, ids::RELOAD), Some(false));
        assert_eq!(shown(&blade, ids::CONDITION), Some(true));
        assert_eq!(
            want(ids::CONDITION, &blade, false).and_then(|w| w.text),
            Some("SOUND".to_owned())
        );
    }

    #[test]
    fn the_controls_card_words_the_buttons_by_what_is_in_hand() {
        assert_eq!(english(trigger_key(Weapon::Blade)), "Cut");
        assert_eq!(english(trigger_key(Weapon::Glaive)), "Cut");
        assert_eq!(english(trigger_key(Weapon::Pistol)), "Fire");
        assert_eq!(english("strata.controls.heavy"), "Heavy cut");
        let keys: Vec<&str> = CONTROLS
            .iter()
            .flat_map(|c| c.iter().map(|e| e.0))
            .collect();
        assert!(
            !keys.contains(&"strata.controls.key.cut"),
            "E no longer cuts"
        );
    }

    #[test]
    fn the_controls_card_follows_the_weapon_in_hand() {
        let mut app = App::new();
        slotted::ui::TestIdIndex::install(app.world_mut());
        app.add_systems(Update, update_controls_fire);
        let player = app
            .world_mut()
            .spawn((
                LocalPlayer,
                Combatant::default().carrying([Some(Weapon::Blade), Some(Weapon::Carbine)], 0),
            ))
            .id();
        let spawn = |app: &mut App, id: &str| {
            app.world_mut()
                .spawn((
                    tags(Some(id)),
                    slotted::ui::TestId::new(id),
                    Node::default(),
                    Text(String::new()),
                ))
                .id()
        };
        let fire = spawn(&mut app, ids::CONTROLS_FIRE);
        let aim = spawn(&mut app, ids::CONTROLS_AIM_LABEL);
        let reload = spawn(&mut app, ids::CONTROLS_RELOAD);
        let heavy = spawn(&mut app, ids::CONTROLS_HEAVY);
        app.update();
        let display = |app: &App, entity| app.world().get::<Node>(entity).expect("node").display;
        assert_eq!(app.world().get::<Text>(aim).expect("text").0, "Guard");
        assert_eq!(display(&app, reload), Display::None);
        assert_eq!(display(&app, heavy), Display::Flex);
        assert_eq!(app.world().get::<Text>(fire).expect("text").0, "Cut");
        let mut actor = app.world_mut().get_mut::<Combatant>(player).expect("actor");
        actor.carry([Some(Weapon::Carbine), Some(Weapon::Blade)]);
        app.update();
        assert_eq!(app.world().get::<Text>(aim).expect("text").0, "Aim");
        assert_eq!(display(&app, reload), Display::Flex);
        assert_eq!(display(&app, heavy), Display::None);
        assert_ne!(app.world().get::<Text>(fire).expect("text").0, "Cut");
    }

    #[test]
    fn the_stamina_row_says_guard_while_the_guard_is_up() {
        let mut actor = Combatant::default().carrying([Some(Weapon::Blade), None], 0);
        let label = |actor: &Combatant| {
            want(Meter::Stamina.label(), &vitals(actor, None, None), false)
                .and_then(|want| want.text)
        };
        assert_eq!(label(&actor).as_deref(), Some("STAMINA"));
        actor.guard_up = true;
        assert_eq!(label(&actor).as_deref(), Some("GUARD"));
    }

    #[test]
    fn the_controls_card_says_the_trigger_in_the_weapons_words() {
        let say = |weapon: Weapon| english(fire_label_key(&weapon.spec()));
        assert_eq!(say(Weapon::Pistol), "Fire");
        assert_eq!(say(Weapon::Carbine), "Burst (hold: full auto)");
    }

    #[test]
    fn low_health_pulses_between_its_bounds_at_its_rate() {
        let close = |a: f32, b: f32| (a - b).abs() < 1e-4;
        assert!(close(pulse_alpha(0.0), 0.10));
        let crest = 0.5 / PULSE_HZ;
        assert!(close(pulse_alpha(crest), 0.22));
        assert!(close(pulse_alpha(1.0 / PULSE_HZ), 0.10), "one period");
        assert!(!pulse_danger(0.0) && pulse_danger(crest));
        for step in 0..200 {
            #[allow(clippy::cast_precision_loss, reason = "a small count")]
            let alpha = pulse_alpha(step as f32 * 0.013);
            assert!((0.10 - 1e-4..=0.22 + 1e-4).contains(&alpha));
        }
        // The hit flash still reads over the pulse, and the pulse only
        // shows when low.
        assert!(close(vignette_alpha(1.0, true, crest), VIGNETTE_ALPHA));
        assert!(close(vignette_alpha(0.0, true, crest), 0.22));
        assert!(close(vignette_alpha(0.0, false, crest), 0.0));
    }

    #[test]
    fn the_health_figure_swaps_roles_only_when_low() {
        let mut actor = Combatant::default();
        let v = vitals(&actor, None, None);
        let role = |v: &Vitals, pulse| want(Meter::Health.value(), v, pulse).and_then(|w| w.role);
        assert_eq!(role(&v, true), Some("strata.vitals.value"));
        actor.health *= 0.2;
        let v = vitals(&actor, None, None);
        assert_eq!(role(&v, true), Some("strata.vitals.value.danger"));
        assert_eq!(role(&v, false), Some("strata.vitals.value"));
    }

    #[test]
    fn the_down_card_eases_in_over_four_hundred_milliseconds() {
        assert!(down_alpha(0.0).abs() < 1e-6);
        assert!(down_alpha(0.1) > 0.5, "most of it at once");
        assert!((down_alpha(DOWN_FADE_SECS) - 1.0).abs() < 1e-6);
        assert!((down_alpha(2.0) - 1.0).abs() < 1e-6);
        for key in ["strata.down.title", "strata.down.key", "strata.down.retry"] {
            assert!(veyra_ui::strings::english(key).is_some(), "{key}");
        }
    }

    #[test]
    #[allow(clippy::unwrap_used, reason = "the shipped theme parses")]
    fn every_painted_role_is_in_the_theme() {
        let path = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../assets/themes/veyra.theme.ron"
        );
        let theme = Theme::from_ron(&std::fs::read_to_string(path).unwrap()).unwrap();
        for role in [
            "scrim",
            "hint.glyph",
            "hint.glyph.text",
            "strata.controls.label",
            "strata.down.card",
            "strata.down.title",
            "strata.down.name",
            "strata.vitals.value.danger",
            "strata.nameplate",
            "strata.nameplate.name",
            "strata.nameplate.track",
            "strata.nameplate.health",
            "strata.nameplate.shield",
            "strata.bar.frame",
            "strata.bar.cool",
        ] {
            let paint = Paint::for_role(&theme, &Role::new_static(role));
            assert!(paint.is_some(), "{role}");
            assert!(
                paint.is_some_and(|p| p.glass.is_none()),
                "{role} is painted raw and cannot be glass"
            );
        }
    }

    #[test]
    fn the_card_counts_down_unless_pinned() {
        let mut card = ControlsCard::default();
        assert!(!card.shown());
        card.left = 0.5;
        assert!(card.shown());
        card = ControlsCard {
            left: 0.0,
            pinned: true,
        };
        assert!(card.shown());
    }
}
