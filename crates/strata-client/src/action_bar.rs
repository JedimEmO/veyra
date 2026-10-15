//! The action bar and the statuses: what the sim's bar of abilities looks
//! like on the HUD (ticket 0010, step 6).
//!
//! The bar is a slotted HUD layer, `strata:bar`, at the bottom centre: the
//! two weapons carried as marks, then the three places of the weapon in hand
//! and the two utility places as painted squares. Like the vitals, what
//! changes a frame is written onto the spawned nodes by `test_id`, because a
//! layer is respawned whenever a screen closes over it.
//!
//! The screen decides nothing. [`bar_view`] is a pure function of the
//! `Combatant`: a square's state is the answer of the sim's own
//! `Combatant::can_use` mapped, never rebuilt. [`status_view`] is likewise
//! the statuses an actor bears. The only things kept here are the short
//! cues of a press: the sim reports a refusal once (`refusals`), and the bar
//! remembers which place was pressed to flash it.

use bevy::prelude::*;
use bevy::ui::widget::{ImageNode, NodeImageMode, Text};
use slotted::theme::{ActiveTheme, Role, Theme, Themed};
use slotted::ui::def::{Align, Layout, LayoutDirection, Length, LocKey, Place, Tags, UiNodeDef};
use slotted::ui::{HudAnchor, HudLayerDef, HudLayerId, HudLayers, NineAnchor};
use strata_registry::IconCatalogue;
use strata_sim::ability::{
    AbilityEffect, AbilityRefusal, AbilityTable, Bar, Own, can_slot, known_to,
};
use strata_sim::combat::{Action, CombatIntent, Combatant, Refusal, Weapon};
use strata_sim::progression::CharacterSheet;
use strata_sim::ride::Riding;
use strata_sim::{LocalPlayer, Registries, combat_tables};

use crate::ClientSet;
use crate::combat_hud::{english, invisible, is_down, tags, text_aligned};
use crate::ui::icons::{self, EMPTY_PLACE, IconHandles, STAGGERED};

/// The bar's layer id.
pub const BAR_LAYER: &str = "strata:bar";

/// Places on the bar: the weapon in hand's three, then the two utility.
pub const PLACES: usize = 5;

/// A place's square, logical pixels.
pub const SQUARE: f32 = 56.0;
/// A weapon's mark, logical pixels.
pub const MARK: f32 = 28.0;
/// A status square on the player's row, logical pixels.
pub const STATUS: f32 = 28.0;
/// A status square under a hostile's plate, logical pixels.
pub const PLATE_STATUS: f32 = 18.0;
/// The most statuses drawn on one row.
pub const MAX_STATUSES: usize = 6;

/// How far the controls card sits above the window's edge beyond its usual
/// margin so it clears the bar: a square, and air.
pub const CARD_RAISE: f32 = SQUARE + 16.0;

/// A refused press flashes its frame this long, seconds.
pub const REFUSED_FLASH_SECS: f32 = 0.3;
/// The reason line above the bar stays this long, seconds.
pub const REASON_SECS: f32 = 1.2;
/// A press the sim took lights its frame this long, seconds.
pub const TAKEN_FLASH_SECS: f32 = 0.2;

/// Each filled square draws its ability's name under it this long after the
/// weapon in hand or the bar changes, then only its key, so a player who has
/// just swapped reads what is under each key without a menu.
pub const NAME_SECS: f32 = 4.0;

/// The name under a square wraps in this width, logical pixels: a square and
/// the gap beside it, so two names side by side do not touch.
const NAME_WIDTH: f32 = SQUARE + SQUARE_GAP;
/// Two lines of the name's size, reserved always so the squares do not move
/// when the names come and go.
const NAME_HEIGHT: f32 = 28.0;
/// The width of the caption beside the bar, and of the blank opposite it that
/// keeps the squares centred.
const HINT_WIDTH: f32 = 96.0;

/// What a square that cannot be pressed is dimmed to.
const DIM: f32 = 0.4;
/// The gap between the groups of the bar, logical pixels.
const GROUP_GAP: f32 = 14.0;
/// The gap between squares, logical pixels.
const SQUARE_GAP: f32 = 6.0;
/// The empty mark inside an empty place, logical pixels.
const EMPTY_MARK: f32 = 24.0;

/// The words of the keys of the five places, in the sim's numbering.
pub(crate) const KEY_WORDS: [&str; PLACES] = [
    "strata.bar.key.0",
    "strata.bar.key.1",
    "strata.bar.key.2",
    "strata.bar.key.3",
    "strata.bar.key.4",
];

/// The words of the keys that draw the two weapons.
const WEAPON_KEY_WORDS: [&str; 2] = ["strata.bar.key.weapon.0", "strata.bar.key.weapon.1"];

// ---------------------------------------------------------------------------
// The views, pure
// ---------------------------------------------------------------------------

/// What a place can do now, the sim's `can_use` answer mapped.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum CellState {
    /// A press would begin it.
    Ready,
    /// Cooling down: the share of the cooldown left and the whole seconds
    /// a player would wait.
    Cooling {
        /// `0..=1`.
        share: f32,
        /// Rounded up, so a nought is a nought left.
        seconds: u32,
    },
    /// Short of stamina.
    Short,
    /// Not with the weapon in hand, the magazine, or what there is to do.
    Wrong,
    /// Nothing can be pressed: disarmed, or another action under way.
    Held,
}

/// An ability in a place.
#[derive(Debug, Clone, PartialEq)]
pub struct Filled {
    /// The ability's id.
    pub id: String,
    /// Its name.
    pub name: String,
    /// Its catalogue id.
    pub icon: String,
    /// Its 64 pixel file under the asset root, if the catalogue has it.
    pub file: Option<String>,
    /// What a press would do.
    pub state: CellState,
}

/// One place of the bar.
#[derive(Debug, Clone, PartialEq)]
pub struct Cell {
    /// The key's word.
    pub key: &'static str,
    /// The ability, or none for an empty place.
    pub ability: Option<Filled>,
    /// The place is empty and the member knows an ability that could go in
    /// it: it draws a plus.
    pub open: bool,
}

/// A weapon carried, as its mark.
#[derive(Debug, Clone, PartialEq)]
pub struct WeaponMark {
    /// The weapon class.
    pub class: Weapon,
    /// The symbol's catalogue id.
    pub symbol: &'static str,
    /// Its file at the mark's size.
    pub file: Option<String>,
    /// The key that draws it.
    pub key: &'static str,
    /// It is the one in hand.
    pub in_hand: bool,
}

/// Everything the bar shows.
#[derive(Debug, Clone, PartialEq)]
pub struct BarView {
    /// The two slots; none for an empty one.
    pub weapons: [Option<WeaponMark>; 2],
    /// The places, in the sim's numbering.
    pub places: Vec<Cell>,
    /// Some place is open, so the caption says where to fill it.
    pub hint: bool,
}

/// Which places of the bar are empty and could take something the member
/// knows, in the sim's numbering: the one rule `can_slot` asked of every row.
#[must_use]
pub fn open_places(
    class: Weapon,
    bar: &Bar,
    known: &std::collections::BTreeSet<String>,
    table: &AbilityTable,
) -> [bool; PLACES] {
    let mut open = [false; PLACES];
    for (index, slot) in open
        .iter_mut()
        .enumerate()
        .take(table.bar.weapon + table.bar.utility)
    {
        let place = if index < table.bar.weapon {
            strata_sim::ability::Place::Weapon(class, index)
        } else {
            strata_sim::ability::Place::Utility(index - table.bar.weapon)
        };
        *slot = bar.get(place).is_none()
            && table
                .abilities
                .iter()
                .any(|row| can_slot(table, known, bar, place, &row.id).is_ok());
    }
    open
}

/// The bar of `actor`.
#[must_use]
pub fn bar_view(
    actor: &Combatant,
    tables: &AbilityTable,
    catalogue: &IconCatalogue,
    scale: f32,
    open: [bool; PLACES],
) -> BarView {
    let count = (tables.bar.weapon + tables.bar.utility).min(PLACES);
    let places: Vec<Cell> = (0..count)
        .map(|index| {
            let mut cell = cell(actor, index, catalogue, scale);
            cell.open = open[index] && cell.ability.is_none();
            cell
        })
        .collect();
    BarView {
        hint: places.iter().any(|cell| cell.open),
        places,
        weapons: [0, 1].map(|slot| {
            actor.slots()[slot].map(|class| WeaponMark {
                class,
                symbol: icons::symbol_of(class),
                file: icons::file_of(catalogue, icons::symbol_of(class), physical(MARK, scale)),
                key: english(WEAPON_KEY_WORDS[slot]),
                in_hand: actor.hand() == slot,
            })
        }),
    }
}

/// The export size a square of `logical` px wants on a screen at `scale`, so
/// a sharper file stands in where the UI is drawn larger.
#[allow(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "a positive size of a few hundred pixels"
)]
fn physical(logical: f32, scale: f32) -> u32 {
    (logical * scale.max(1.0)).ceil() as u32
}

fn cell(actor: &Combatant, index: usize, catalogue: &IconCatalogue, scale: f32) -> Cell {
    let key = english(KEY_WORDS[index]);
    let Some(row) = actor.ability_at(index) else {
        return Cell {
            key,
            ability: None,
            open: false,
        };
    };
    let state = match actor.can_use(index) {
        Ok(_) => CellState::Ready,
        Err(AbilityRefusal::Cooldown(left)) => CellState::Cooling {
            share: if row.cooldown > 0.0 {
                (left / row.cooldown).clamp(0.0, 1.0)
            } else {
                1.0
            },
            #[allow(
                clippy::cast_possible_truncation,
                clippy::cast_sign_loss,
                reason = "a few seconds, never negative"
            )]
            seconds: left.max(0.0).ceil() as u32,
        },
        Err(AbilityRefusal::Stamina(_)) => CellState::Short,
        Err(AbilityRefusal::WrongWeapon | AbilityRefusal::NoRounds | AbilityRefusal::Nothing) => {
            CellState::Wrong
        }
        // An empty place has no row, so it cannot be here.
        Err(AbilityRefusal::Busy | AbilityRefusal::Disarmed | AbilityRefusal::Empty) => {
            CellState::Held
        }
    };
    Cell {
        key,
        ability: Some(Filled {
            file: icons::file_of(catalogue, &row.icon, physical(SQUARE, scale)),
            id: row.id,
            name: row.name,
            icon: row.icon,
            state,
        }),
        open: false,
    }
}

/// A status borne, as a small square.
#[derive(Debug, Clone, PartialEq)]
pub struct StatusCell {
    /// The status's id; `stagger` for the sim's stagger.
    pub id: String,
    /// Its name.
    pub name: String,
    /// Its catalogue id.
    pub icon: String,
    /// The share of its time left, `0..=1`.
    pub share: f32,
}

/// The longest time any ability gives the status `id` for, which is what a
/// share of it is measured against.
fn status_total(tables: &AbilityTable, id: &str) -> Option<f32> {
    tables
        .abilities
        .iter()
        .filter_map(|row| match &row.effect {
            AbilityEffect::Own(Own::Status { id, seconds }) => Some((id.as_str(), *seconds)),
            AbilityEffect::Hit {
                status, seconds, ..
            } => Some((status.as_str(), *seconds)),
            AbilityEffect::Shot {
                on_hit: Some(hit), ..
            }
            | AbilityEffect::Strike {
                on_hit: Some(hit), ..
            } => Some((hit.status.as_str(), hit.seconds)),
            _ => None,
        })
        .filter(|(given, _)| *given == id)
        .map(|(_, seconds)| seconds)
        .reduce(f32::max)
}

/// The statuses `actor` bears, in the order they were applied, then the
/// stagger while it lasts.
#[must_use]
pub fn status_view(actor: &Combatant, tables: &AbilityTable) -> Vec<StatusCell> {
    let mut all: Vec<StatusCell> = actor
        .statuses()
        .iter()
        .filter_map(|(id, left)| {
            let row = tables.status(id)?;
            let total = status_total(tables, id).unwrap_or(*left);
            Some(StatusCell {
                id: id.clone(),
                name: row.name.clone(),
                icon: row.icon.clone(),
                share: if total > 0.0 {
                    (left / total).clamp(0.0, 1.0)
                } else {
                    1.0
                },
            })
        })
        .collect();
    if actor.action == Action::Stagger {
        let total = actor.duration();
        all.push(StatusCell {
            id: "stagger".to_owned(),
            name: english("strata.status.staggered").to_owned(),
            icon: STAGGERED.to_owned(),
            share: if total > 0.0 {
                (1.0 - actor.age / total).clamp(0.0, 1.0)
            } else {
                0.0
            },
        });
    }
    all
}

// ---------------------------------------------------------------------------
// The layer as data
// ---------------------------------------------------------------------------

/// The `test_id`s of the bar's nodes.
pub mod ids {
    /// The reason line above the bar.
    pub const REASON: &str = "bar.reason";

    /// A place's frame.
    #[must_use]
    pub fn frame(place: usize) -> String {
        format!("bar.{place}")
    }

    /// A place's part: `img`, `empty`, `cool` or `secs`.
    #[must_use]
    pub fn part(place: usize, part: &str) -> String {
        format!("bar.{place}.{part}")
    }

    /// The caption beside the bar.
    pub const HINT: &str = "bar.hint";

    /// The plus mark of an open place.
    #[must_use]
    pub fn plus(place: usize) -> String {
        part(place, "plus")
    }

    /// The name under a place.
    #[must_use]
    pub fn name(place: usize) -> String {
        part(place, "name")
    }

    /// A weapon mark's image.
    #[must_use]
    pub fn weapon(slot: usize) -> String {
        format!("bar.weapon.{slot}")
    }

    /// A weapon mark's key.
    #[must_use]
    pub fn weapon_key(slot: usize) -> String {
        format!("bar.weapon.{slot}.key")
    }

    /// A status square on the player's row.
    #[must_use]
    pub fn status(slot: usize) -> String {
        format!("vitals.status.{slot}")
    }

    /// A status square's picture or veil.
    #[must_use]
    pub fn status_part(slot: usize, part: &str) -> String {
        format!("vitals.status.{slot}.{part}")
    }
}

/// The bar layer's id.
#[must_use]
pub fn bar_layer() -> HudLayerId {
    HudLayerId::new_static(BAR_LAYER)
}

fn sized(width: f32, height: f32) -> Layout {
    Layout {
        width: Some(Length::Px(width)),
        height: Some(Length::Px(height)),
        ..Layout::default()
    }
}

fn filling() -> Layout {
    Layout {
        width: Some(Length::Percent(100.0)),
        height: Some(Length::Percent(100.0)),
        ..Layout::default()
    }
}

fn panel(
    role: &'static str,
    layout: Layout,
    children: Vec<UiNodeDef>,
    id: Option<&str>,
) -> UiNodeDef {
    UiNodeDef::Panel {
        role: Role::new_static(role),
        layout,
        children,
        tags: tags(id),
    }
}

fn gap(width: f32) -> UiNodeDef {
    invisible(sized(width, 1.0), Vec::new(), None)
}

/// A place's square: the picture, the empty mark, the veil that drains, the
/// seconds and the key on a chip at the lower left.
#[allow(
    clippy::too_many_lines,
    reason = "one square, its parts listed bottom to top as they are drawn"
)]
fn square(place: usize) -> UiNodeDef {
    let centred = |width: f32, height: f32| Layout {
        place: Some(Place {
            anchor: NineAnchor::Center,
            offset: Vec2::ZERO,
        }),
        ..sized(width, height)
    };
    panel(
        "strata.bar.frame",
        sized(SQUARE, SQUARE),
        vec![
            panel(
                "invisible",
                filling(),
                Vec::new(),
                Some(&ids::part(place, "img")),
            ),
            panel(
                "invisible",
                centred(EMPTY_MARK, EMPTY_MARK),
                Vec::new(),
                Some(&ids::part(place, "empty")),
            ),
            invisible(
                Layout {
                    place: Some(Place {
                        anchor: NineAnchor::Center,
                        offset: Vec2::ZERO,
                    }),
                    ..Layout::default()
                },
                vec![text_aligned(
                    "strata.bar.plus",
                    "strata.bar.plus",
                    Some(&ids::plus(place)),
                    slotted::ui::def::TextAlign::Center,
                )],
                None,
            ),
            panel(
                "strata.bar.cool",
                Layout {
                    place: Some(Place {
                        anchor: NineAnchor::BottomLeft,
                        offset: Vec2::ZERO,
                    }),
                    width: Some(Length::Percent(100.0)),
                    height: Some(Length::Percent(0.0)),
                    ..Layout::default()
                },
                Vec::new(),
                Some(&ids::part(place, "cool")),
            ),
            panel(
                "invisible",
                Layout {
                    place: Some(Place {
                        anchor: NineAnchor::Center,
                        offset: Vec2::ZERO,
                    }),
                    ..Layout::default()
                },
                vec![text_aligned(
                    "",
                    "strata.bar.secs",
                    Some(&ids::part(place, "secs")),
                    slotted::ui::def::TextAlign::Center,
                )],
                None,
            ),
            panel(
                "strata.bar.key",
                Layout {
                    place: Some(Place {
                        anchor: NineAnchor::BottomLeft,
                        offset: Vec2::new(3.0, -3.0),
                    }),
                    padding: slotted::ui::def::Padding {
                        top: 0.0,
                        right: 0.5,
                        bottom: 0.0,
                        left: 0.5,
                    },
                    align: Some(Align::Center),
                    ..Layout::default()
                },
                vec![UiNodeDef::Text {
                    key: LocKey(KEY_WORDS[place].to_owned()),
                    style: slotted::ui::def::TextRole::Body,
                    opts: slotted::ui::def::TextOpts {
                        role: Some(Role::new_static("strata.bar.key.text")),
                        wrap: false,
                        ..slotted::ui::def::TextOpts::default()
                    },
                    tags: Tags::new(),
                }],
                None,
            ),
        ],
        Some(&ids::frame(place)),
    )
}

/// The ability's name under its square, wrapped to two lines at most in a
/// box of a fixed size, so the bar does not move as the names come and go.
fn name(place: usize) -> UiNodeDef {
    invisible(
        Layout {
            align: Some(Align::Stretch),
            ..sized(NAME_WIDTH, NAME_HEIGHT)
        },
        vec![UiNodeDef::Text {
            key: LocKey(String::new()),
            style: slotted::ui::def::TextRole::Body,
            opts: slotted::ui::def::TextOpts {
                role: Some(Role::new_static("strata.bar.name")),
                wrap: true,
                align: slotted::ui::def::TextAlign::Center,
                ..slotted::ui::def::TextOpts::default()
            },
            tags: tags(Some(&ids::name(place))),
        }],
        None,
    )
}

/// A weapon's mark over its key.
fn weapon(slot: usize) -> UiNodeDef {
    invisible(
        Layout {
            direction: LayoutDirection::Column,
            gap: 0.2,
            align: Some(Align::Center),
            ..Layout::default()
        },
        vec![
            panel(
                "invisible",
                sized(MARK, MARK),
                Vec::new(),
                Some(&ids::weapon(slot)),
            ),
            text_aligned(
                WEAPON_KEY_WORDS[slot],
                "strata.bar.weapon.key.dim",
                Some(&ids::weapon_key(slot)),
                slotted::ui::def::TextAlign::Center,
            ),
        ],
        None,
    )
}

/// The bar: the reason line over a row of the two weapons, the weapon's
/// three places and the two utility places.
#[must_use]
pub fn bar_layer_def() -> HudLayerDef {
    // The blank on the left is the caption's width, so the squares stay
    // centred whether the caption is up or not.
    let mut bar = vec![
        gap(HINT_WIDTH),
        weapon(0),
        gap(SQUARE_GAP),
        weapon(1),
        gap(GROUP_GAP),
    ];
    for place in 0..PLACES {
        if place == 3 {
            bar.push(gap(GROUP_GAP - SQUARE_GAP));
        }
        bar.push(invisible(
            Layout {
                direction: LayoutDirection::Column,
                gap: 0.3,
                align: Some(Align::Center),
                ..Layout::default()
            },
            vec![square(place), name(place)],
            None,
        ));
        if place + 1 < PLACES {
            bar.push(gap(SQUARE_GAP));
        }
    }
    bar.push(gap(GROUP_GAP));
    bar.push(invisible(
        Layout {
            align: Some(Align::Stretch),
            ..sized(HINT_WIDTH - GROUP_GAP, SQUARE)
        },
        vec![UiNodeDef::Text {
            key: LocKey(String::new()),
            style: slotted::ui::def::TextRole::Body,
            opts: slotted::ui::def::TextOpts {
                role: Some(Role::new_static("strata.bar.hint")),
                wrap: true,
                ..slotted::ui::def::TextOpts::default()
            },
            tags: tags(Some(ids::HINT)),
        }],
        None,
    ));
    HudLayerDef {
        id: bar_layer(),
        anchor: HudAnchor {
            anchor: NineAnchor::Bottom,
            offset: Vec2::new(0.0, -24.0),
            scale: 1.0,
        },
        tree: invisible(
            Layout {
                direction: LayoutDirection::Column,
                gap: 0.6,
                align: Some(Align::Center),
                ..Layout::default()
            },
            vec![
                text_aligned(
                    "",
                    "strata.bar.reason",
                    Some(ids::REASON),
                    slotted::ui::def::TextAlign::Center,
                ),
                invisible(
                    Layout {
                        direction: LayoutDirection::Row,
                        align: Some(Align::Center),
                        ..Layout::default()
                    },
                    bar,
                    Some("bar"),
                ),
            ],
            None,
        ),
        visible: true,
        hide_with_screen: true,
    }
}

/// The player's row of status squares, which the vitals layer carries.
#[must_use]
pub fn status_row() -> UiNodeDef {
    invisible(
        Layout {
            direction: LayoutDirection::Row,
            gap: 0.4,
            ..Layout::default()
        },
        (0..MAX_STATUSES)
            .map(|slot| {
                panel(
                    "strata.bar.frame",
                    sized(STATUS, STATUS),
                    vec![
                        panel(
                            "invisible",
                            filling(),
                            Vec::new(),
                            Some(&ids::status_part(slot, "img")),
                        ),
                        panel(
                            "strata.bar.cool",
                            Layout {
                                place: Some(Place {
                                    anchor: NineAnchor::BottomLeft,
                                    offset: Vec2::ZERO,
                                }),
                                width: Some(Length::Percent(100.0)),
                                height: Some(Length::Percent(0.0)),
                                ..Layout::default()
                            },
                            Vec::new(),
                            Some(&ids::status_part(slot, "cool")),
                        ),
                    ],
                    Some(&ids::status(slot)),
                )
            })
            .collect(),
        Some("vitals.statuses"),
    )
}

// ---------------------------------------------------------------------------
// What a press looks like
// ---------------------------------------------------------------------------

/// The short cues of a press, kept between frames.
#[derive(Resource, Debug, Default)]
pub struct BarFeedback {
    /// The last place the player pressed, remembered because the sim's
    /// refusal does not say which.
    last_press: Option<usize>,
    seen_refusals: Option<u64>,
    /// Seconds each frame still flashes danger.
    pub danger: [f32; PLACES],
    /// Seconds each frame still flashes accent.
    pub accent: [f32; PLACES],
    /// The refusal in words above the bar, with the seconds it stays.
    pub reason: Option<(AbilityRefusal, f32)>,
    /// Each place's ability and its cooldown a frame ago.
    prev: [Option<(String, f32)>; PLACES],
    prev_in_use: Option<String>,
    /// Seconds the names under the squares are still drawn.
    pub names: f32,
    /// The weapon in hand a frame ago.
    hand: Option<Weapon>,
}

impl BarFeedback {
    /// Advances the cues by `dt` seconds over what `actor` is now.
    pub fn step(&mut self, actor: &Combatant, dt: f32) {
        for left in self.danger.iter_mut().chain(&mut self.accent) {
            *left = (*left - dt).max(0.0);
        }
        self.names = (self.names - dt).max(0.0);
        if self.hand.replace(actor.weapon) != Some(actor.weapon) {
            self.names = NAME_SECS;
        }
        if let Some((_, left)) = &mut self.reason {
            *left -= dt;
            if *left <= 0.0 {
                self.reason = None;
            }
        }
        // A first sight of an actor is a baseline, not a burst of everything
        // it was ever refused.
        let seen = self.seen_refusals.replace(actor.refusals);
        if seen.is_some_and(|seen| seen != actor.refusals)
            && let Some(Refusal::Ability(why)) = actor.refused
            && why != AbilityRefusal::Empty
        {
            if let Some(place) = self.last_press.take().filter(|place| *place < PLACES) {
                self.danger[place] = REFUSED_FLASH_SECS;
            }
            self.reason = Some((why, REASON_SECS));
        }
        let in_use = actor.ability_in_use().map(str::to_owned);
        for index in 0..PLACES {
            let now = actor.ability_at(index).map(|row| {
                let left = actor.cooldown_left(&row.id);
                (row, left)
            });
            if let Some((row, left)) = &now {
                let before = self.prev[index]
                    .as_ref()
                    .filter(|(id, _)| *id == row.id)
                    .map(|(_, left)| *left);
                // A commitment is taken in the frame the action names it; an
                // instant press, or a cut, never becomes that action, so its
                // cooldown starting is the edge.
                let taken = if row.is_instant() || matches!(row.effect, AbilityEffect::Cut(_)) {
                    before.is_some_and(|before| before <= 0.0) && *left > 0.0
                } else {
                    in_use.as_deref() == Some(row.id.as_str())
                        && self.prev_in_use.as_deref() != Some(row.id.as_str())
                };
                if taken {
                    self.accent[index] = TAKEN_FLASH_SECS;
                }
            }
            let id = now.as_ref().map(|(row, _)| row.id.as_str());
            if self.prev[index].as_ref().map(|(before, _)| before.as_str()) != id {
                self.names = NAME_SECS;
            }
            self.prev[index] = now.map(|(row, left)| (row.id, left));
        }
        self.prev_in_use = in_use;
    }
}

/// `PostUpdate`: the place pressed this frame, seen before the tick takes
/// the press away.
fn note_press(mut feedback: ResMut<BarFeedback>, players: Query<&CombatIntent, With<LocalPlayer>>) {
    if let Some(place) = players.iter().find_map(|intent| intent.ability)
        && feedback.last_press != Some(place)
    {
        feedback.last_press = Some(place);
    }
}

/// `Update`: the cues follow the local player.
fn track_feedback(
    time: Res<Time>,
    mut feedback: ResMut<BarFeedback>,
    players: Query<&Combatant, With<LocalPlayer>>,
) {
    if let Ok(actor) = players.single() {
        feedback.step(actor, time.delta_secs());
    }
}

// ---------------------------------------------------------------------------
// The nodes
// ---------------------------------------------------------------------------

/// Which part of the bar a node is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Part {
    Reason,
    Hint,
    Plus(usize),
    Name(usize),
    Frame(usize),
    Img(usize),
    Empty(usize),
    Cool(usize),
    Secs(usize),
    Weapon(usize),
    WeaponKey(usize),
    StatusFrame(usize),
    StatusImg(usize),
    StatusCool(usize),
}

impl Part {
    fn of(id: &str) -> Option<Self> {
        if let Some(rest) = id.strip_prefix("vitals.status.") {
            let mut it = rest.split('.');
            let slot = it.next()?.parse().ok()?;
            return match it.next() {
                None => Some(Self::StatusFrame(slot)),
                Some("img") => Some(Self::StatusImg(slot)),
                Some("cool") => Some(Self::StatusCool(slot)),
                Some(_) => None,
            };
        }
        let rest = id.strip_prefix("bar.")?;
        if rest == "reason" {
            return Some(Self::Reason);
        }
        if rest == "hint" {
            return Some(Self::Hint);
        }
        let mut it = rest.split('.');
        let first = it.next()?;
        if first == "weapon" {
            let slot = it.next()?.parse().ok()?;
            return match it.next() {
                None => Some(Self::Weapon(slot)),
                Some("key") => Some(Self::WeaponKey(slot)),
                Some(_) => None,
            };
        }
        let place = first.parse().ok()?;
        match it.next() {
            None => Some(Self::Frame(place)),
            Some("img") => Some(Self::Img(place)),
            Some("empty") => Some(Self::Empty(place)),
            Some("cool") => Some(Self::Cool(place)),
            Some("secs") => Some(Self::Secs(place)),
            Some("plus") => Some(Self::Plus(place)),
            Some("name") => Some(Self::Name(place)),
            Some(_) => None,
        }
    }
}

/// What one node should show: whether it is there, its picture and tint, a
/// share for a veil, a word, a role.
#[derive(Debug, Default, PartialEq)]
struct Want {
    shown: Option<bool>,
    file: Option<String>,
    tint: Option<Color>,
    height: Option<f32>,
    text: Option<String>,
    role: Option<&'static str>,
}

fn dim() -> Color {
    Color::srgba(DIM, DIM, DIM, 1.0)
}

/// The frame role of place `index`: a refusal over a taken press over short
/// of stamina over rest.
fn frame_role(view: &BarView, feedback: &BarFeedback, index: usize) -> &'static str {
    if feedback.danger[index] > 0.0 {
        "strata.bar.frame.danger"
    } else if feedback.accent[index] > 0.0 {
        "strata.bar.frame.accent"
    } else if view
        .places
        .get(index)
        .and_then(|cell| cell.ability.as_ref())
        .is_some_and(|filled| filled.state == CellState::Short)
    {
        "strata.bar.frame.short"
    } else {
        "strata.bar.frame"
    }
}

/// The colours the symbols take, resolved from the theme.
struct Tints {
    hand: Color,
    holstered: Color,
}

#[allow(
    clippy::too_many_lines,
    reason = "one arm a part of the bar, which read better together"
)]
fn want(
    part: Part,
    view: &BarView,
    statuses: &[StatusCell],
    feedback: &BarFeedback,
    catalogue: &IconCatalogue,
    tints: &Tints,
) -> Want {
    let filled = |index: usize| {
        view.places
            .get(index)
            .and_then(|cell| cell.ability.as_ref())
    };
    match part {
        Part::Reason => {
            let words = feedback
                .reason
                .map(|(why, _)| crate::ui::progress::ability_words(why));
            Want {
                shown: Some(words.is_some()),
                text: Some(words.unwrap_or_default()),
                ..Want::default()
            }
        }
        Part::Hint => Want {
            text: Some(if view.hint {
                english("strata.bar.hint").to_owned()
            } else {
                String::new()
            }),
            ..Want::default()
        },
        Part::Plus(index) => Want {
            shown: Some(view.places.get(index).is_some_and(|cell| cell.open)),
            ..Want::default()
        },
        Part::Name(index) => Want {
            text: Some(
                filled(index)
                    .filter(|_| feedback.names > 0.0)
                    .map_or_else(String::new, |f| f.name.clone()),
            ),
            ..Want::default()
        },
        Part::Frame(index) => Want {
            role: Some(frame_role(view, feedback, index)),
            ..Want::default()
        },
        Part::Img(index) => {
            let filled = filled(index);
            Want {
                shown: Some(filled.is_some_and(|f| f.file.is_some())),
                file: filled.and_then(|f| f.file.clone()),
                tint: Some(match filled.map(|f| f.state) {
                    None | Some(CellState::Ready | CellState::Cooling { .. }) => Color::WHITE,
                    Some(_) => dim(),
                }),
                ..Want::default()
            }
        }
        Part::Empty(index) => Want {
            shown: Some(
                index < view.places.len() && filled(index).is_none() && !view.places[index].open,
            ),
            file: icons::file_of(catalogue, EMPTY_PLACE, 32),
            tint: Some(tints.holstered),
            ..Want::default()
        },
        Part::Cool(index) => {
            let share = match filled(index).map(|f| f.state) {
                Some(CellState::Cooling { share, .. }) => share,
                _ => 0.0,
            };
            Want {
                shown: Some(share > 0.0),
                height: Some(share * 100.0),
                ..Want::default()
            }
        }
        Part::Secs(index) => {
            let seconds = match filled(index).map(|f| f.state) {
                Some(CellState::Cooling { seconds, .. }) if seconds >= 1 => Some(seconds),
                _ => None,
            };
            Want {
                shown: Some(seconds.is_some()),
                text: Some(seconds.map_or_else(String::new, |s| s.to_string())),
                ..Want::default()
            }
        }
        Part::Weapon(slot) => {
            let mark = view.weapons[slot.min(1)].as_ref();
            Want {
                shown: Some(mark.is_some_and(|m| m.file.is_some())),
                file: mark.and_then(|m| m.file.clone()),
                tint: Some(if mark.is_some_and(|m| m.in_hand) {
                    tints.hand
                } else {
                    tints.holstered
                }),
                ..Want::default()
            }
        }
        Part::WeaponKey(slot) => Want {
            shown: Some(view.weapons[slot.min(1)].is_some()),
            role: Some(
                if view.weapons[slot.min(1)]
                    .as_ref()
                    .is_some_and(|m| m.in_hand)
                {
                    "strata.bar.weapon.key"
                } else {
                    "strata.bar.weapon.key.dim"
                },
            ),
            ..Want::default()
        },
        Part::StatusFrame(slot) => Want {
            shown: Some(slot < statuses.len().min(MAX_STATUSES)),
            ..Want::default()
        },
        Part::StatusImg(slot) => {
            let status = statuses.get(slot);
            Want {
                file: status.and_then(|s| icons::file_of(catalogue, &s.icon, 64)),
                tint: Some(Color::WHITE),
                ..Want::default()
            }
        }
        Part::StatusCool(slot) => {
            let share = statuses.get(slot).map_or(0.0, |s| 1.0 - s.share);
            Want {
                shown: Some(share > 0.0),
                height: Some(share * 100.0),
                ..Want::default()
            }
        }
    }
}

/// An image node showing `file`, stretched over its node.
pub(crate) fn set_image(
    commands: &mut Commands,
    entity: Entity,
    current: Option<Mut<ImageNode>>,
    handle: Handle<Image>,
    color: Color,
) {
    match current {
        Some(mut node) => {
            if node.image != handle {
                node.image = handle;
            }
            if node.color != color {
                node.color = color;
            }
        }
        None => {
            commands.entity(entity).insert(ImageNode {
                image_mode: NodeImageMode::Stretch,
                color,
                ..ImageNode::new(handle)
            });
        }
    }
}

/// `Update`: the bar and the player's statuses follow the local player. Every
/// write checks first, so a frame where nothing changed touches no layout.
#[allow(
    clippy::too_many_arguments,
    clippy::type_complexity,
    reason = "the actor, the art, the theme and the parts of a node a cue writes"
)]
fn update_bar(
    mut commands: Commands,
    feedback: Res<BarFeedback>,
    registries: Option<Res<Registries>>,
    assets: Option<Res<AssetServer>>,
    active: Option<Res<ActiveTheme>>,
    themes: Option<Res<Assets<Theme>>>,
    layers: Option<ResMut<HudLayers>>,
    mut handles: ResMut<IconHandles>,
    scale: Option<Res<bevy::ui::UiScale>>,
    actors: Query<
        (
            &Combatant,
            Option<&Riding>,
            Option<&CharacterSheet>,
            Option<&Bar>,
        ),
        With<LocalPlayer>,
    >,
    mut nodes: Query<(
        Entity,
        &Tags,
        &mut Node,
        Option<&mut Themed>,
        Option<&mut Text>,
        Option<&mut ImageNode>,
    )>,
) {
    let Ok((actor, riding, sheet, bar)) = actors.single() else {
        return;
    };
    if let Some(mut layers) = layers {
        // A rider has no places, and a body on the ground has no bar.
        let shown = riding.is_none() && !is_down(Some(actor));
        let id = bar_layer();
        if layers.get(&id).is_some_and(|def| def.visible != shown) {
            layers.set_visible(&id, shown);
        }
    }
    let empty = IconCatalogue::default();
    let catalogue = registries.as_deref().map_or(&empty, |r| &r.0.icons);
    let theme = active
        .as_deref()
        .zip(themes.as_deref())
        .and_then(|(active, themes)| themes.get(&active.0));
    let tints = Tints {
        hand: icons::tint_of(theme, "strata.bar.symbol"),
        holstered: icons::tint_of(theme, "strata.bar.symbol.dim"),
    };
    let tables = combat_tables::current();
    let open = match (registries.as_deref(), sheet, bar) {
        (Some(registries), Some(sheet), Some(bar)) => open_places(
            actor.weapon,
            bar,
            &known_to(&registries.0.progression, sheet, bar),
            &tables.abilities,
        ),
        _ => [false; PLACES],
    };
    let view = bar_view(
        actor,
        &tables.abilities,
        catalogue,
        scale.map_or(1.0, |s| s.0),
        open,
    );
    let statuses = status_view(actor, &tables.abilities);
    for (entity, node_tags, mut node, themed, text, current) in &mut nodes {
        let Some(part) = node_tags.get(Tags::TEST_ID).and_then(Part::of) else {
            continue;
        };
        let want = want(part, &view, &statuses, &feedback, catalogue, &tints);
        if let (Some(role), Some(mut themed)) = (want.role, themed)
            && themed.0.as_str() != role
        {
            *themed = Themed(Role::new_static(role));
        }
        if let (Some(words), Some(mut text)) = (want.text, text)
            && text.0 != words
        {
            text.0 = words;
        }
        if let Some(share) = want.height {
            let height = percent(share);
            if node.height != height {
                node.height = height;
            }
        }
        if let Some(shown) = want.shown {
            let display = if shown { Display::Flex } else { Display::None };
            if node.display != display {
                node.display = display;
            }
        }
        if let (Some(file), Some(assets)) = (&want.file, assets.as_deref()) {
            let handle = handles.handle(assets, file);
            set_image(
                &mut commands,
                entity,
                current,
                handle,
                want.tint.unwrap_or(Color::WHITE),
            );
        }
    }
}

/// Registers the bar's state and systems. Called by the combat HUD.
pub(crate) fn install(app: &mut App) {
    app.init_resource::<BarFeedback>()
        .init_resource::<IconHandles>()
        .add_systems(Startup, register_layer)
        .add_systems(
            Update,
            (track_feedback, update_bar).chain().in_set(ClientSet::Hud),
        )
        .add_systems(PostUpdate, note_press);
}

fn register_layer(layers: Option<ResMut<HudLayers>>) {
    if let Some(mut layers) = layers {
        layers.register(bar_layer_def());
    }
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    reason = "a test that unwraps a None fails, which is the point"
)]
mod tests {
    use std::time::Duration;

    use bevy::ecs::system::RunSystemOnce;
    use strata_sim::PlayerIntent;
    use strata_sim::ability::{Bar, Place};
    use strata_sim::combat::CombatMotion;

    use super::*;

    const DT: f32 = 1.0 / 60.0;

    fn tables() -> AbilityTable {
        combat_tables::current().abilities.clone()
    }

    fn catalogue() -> IconCatalogue {
        let text = std::fs::read_to_string(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../assets/icons/catalogue.json"
        ))
        .unwrap();
        IconCatalogue::parse(&text).unwrap()
    }

    fn with_bar(mut actor: Combatant, places: &[(Place, &str)]) -> Combatant {
        let mut bar = Bar::default();
        for (place, id) in places {
            bar.set(*place, id);
        }
        actor.set_bar(&bar);
        actor
    }

    fn carbine() -> Combatant {
        with_bar(
            Combatant::default().carrying([Some(Weapon::Carbine), Some(Weapon::Pistol)], 0),
            &[
                (Place::Weapon(Weapon::Carbine, 0), "aimed_burst"),
                (Place::Weapon(Weapon::Carbine, 1), "butt_strike"),
                (Place::Weapon(Weapon::Pistol, 0), "precision_shot"),
                (Place::Utility(0), "rapid_reload"),
                (Place::Utility(1), "combat_slide"),
            ],
        )
    }

    fn state(view: &BarView, place: usize) -> Option<CellState> {
        view.places[place].ability.as_ref().map(|f| f.state)
    }

    fn tick(actor: &mut Combatant, press: Option<usize>) {
        let mut input = CombatIntent {
            ability: press,
            ..CombatIntent::default()
        };
        actor.step(
            &mut input,
            &PlayerIntent::default(),
            DT,
            &mut CombatMotion::default(),
        );
    }

    #[test]
    fn each_state_is_the_sims_answer_mapped() {
        let (tables, catalogue) = (tables(), catalogue());
        let mut actor = carbine();
        let view = bar_view(&actor, &tables, &catalogue, 1.0, [false; PLACES]);
        assert_eq!(state(&view, 0), Some(CellState::Ready));
        assert!(view.places[2].ability.is_none(), "an empty place");
        assert_eq!(view.places.len(), PLACES);
        assert_eq!(
            view.places.iter().map(|c| c.key).collect::<Vec<_>>(),
            ["Q", "E", "T", "3", "4"]
        );
        let burst = view.places[0].ability.as_ref().unwrap();
        assert_eq!(burst.name, "Aimed burst");
        assert!(burst.file.as_deref().unwrap().ends_with(".64.png"));
        // The magazine is full, so a quick reload has nothing to do.
        assert_eq!(state(&view, 3), Some(CellState::Wrong));

        // Cooling: a second after the press, nine tenths of ten seconds left.
        tick(&mut actor, Some(0));
        for _ in 0..59 {
            tick(&mut actor, None);
        }
        let view = bar_view(&actor, &tables, &catalogue, 1.0, [false; PLACES]);
        let Some(CellState::Cooling { share, seconds }) = state(&view, 0) else {
            panic!("cooling, got {:?}", state(&view, 0));
        };
        assert!((0.85..0.95).contains(&share), "{share}");
        assert!((9..=10).contains(&seconds), "{seconds}");

        // Short of stamina.
        let mut tired = carbine();
        tired.stamina = 1.0;
        let view = bar_view(&tired, &tables, &catalogue, 1.0, [false; PLACES]);
        assert_eq!(state(&view, 0), Some(CellState::Short));

        // Disarmed holds every square.
        let mut bound = carbine();
        bound.give_status("disarmed", 2.0);
        let view = bar_view(&bound, &tables, &catalogue, 1.0, [false; PLACES]);
        assert_eq!(state(&view, 0), Some(CellState::Held));
        assert_eq!(state(&view, 4), Some(CellState::Held));

        // A blade in hand: the carbine's quick reload is not for it.
        let blade = with_bar(
            Combatant::default().carrying([Some(Weapon::Blade), Some(Weapon::Carbine)], 0),
            &[(Place::Utility(0), "rapid_reload")],
        );
        let view = bar_view(&blade, &tables, &catalogue, 1.0, [false; PLACES]);
        assert_eq!(state(&view, 3), Some(CellState::Wrong));
    }

    #[test]
    fn a_swap_changes_the_three_places_and_not_the_two() {
        let (tables, catalogue) = (tables(), catalogue());
        let mut actor = carbine();
        let before = bar_view(&actor, &tables, &catalogue, 1.0, [false; PLACES]);
        actor = actor.carrying([Some(Weapon::Carbine), Some(Weapon::Pistol)], 1);
        let after = bar_view(&actor, &tables, &catalogue, 1.0, [false; PLACES]);
        let id = |view: &BarView, place: usize| {
            view.places[place]
                .ability
                .as_ref()
                .map(|filled| filled.id.clone())
        };
        assert_eq!(id(&before, 0).as_deref(), Some("aimed_burst"));
        assert_eq!(id(&after, 0).as_deref(), Some("precision_shot"));
        assert_ne!(id(&before, 1), id(&after, 1));
        assert_eq!(id(&before, 3), id(&after, 3));
        assert_eq!(id(&before, 4), id(&after, 4));
        assert_eq!(
            before.weapons.clone().map(|m| m.unwrap().in_hand),
            [true, false]
        );
        assert_eq!(
            after.weapons.clone().map(|m| m.unwrap().in_hand),
            [false, true]
        );
        let marks = after.weapons.map(|m| m.unwrap());
        assert_eq!(marks[0].symbol, "veyra:icon.weapon.carbine");
        assert_eq!(marks[1].key, "2");
        assert!(marks[1].file.is_some());
    }

    #[test]
    fn statuses_come_in_the_order_applied_with_their_share_and_the_stagger() {
        let tables = tables();
        let mut actor = carbine();
        actor.give_status("focused", 5.0);
        actor.give_status("fortified", 1.0);
        let all = status_view(&actor, &tables);
        let ids: Vec<&str> = all.iter().map(|s| s.id.as_str()).collect();
        assert_eq!(ids, ["focused", "fortified"]);
        assert!((all[0].share - 1.0).abs() < 1e-6);
        assert!((all[1].share - 0.5).abs() < 1e-6, "1 s of the 2 it lasts");
        assert_eq!(all[0].name, "Focused");
        actor.action = Action::Stagger;
        actor.age = actor.duration() / 4.0;
        let all = status_view(&actor, &tables);
        let last = all.last().unwrap();
        assert_eq!(last.id, "stagger");
        assert_eq!(last.icon, STAGGERED);
        assert!((last.share - 0.75).abs() < 1e-5);
        actor.action = Action::Ready;
        assert_eq!(status_view(&actor, &tables).len(), 2);
    }

    #[test]
    fn the_layer_has_five_squares_and_two_marks_by_test_id() {
        let def = bar_layer_def();
        assert!(def.hide_with_screen);
        for place in 0..PLACES {
            for id in [
                ids::frame(place),
                ids::part(place, "img"),
                ids::part(place, "empty"),
                ids::part(place, "cool"),
                ids::part(place, "secs"),
            ] {
                assert!(def.tree.find(&id).is_some(), "{id}");
            }
            assert!(def.tree.find(&ids::frame(PLACES + place)).is_none());
        }
        for slot in 0..2 {
            assert!(def.tree.find(&ids::weapon(slot)).is_some());
            assert!(def.tree.find(&ids::weapon_key(slot)).is_some());
        }
        assert!(def.tree.find(ids::REASON).is_some());
        // Every id the layer names is one the update knows.
        def.tree.walk(&mut |node| {
            if let Some(id) = node.id()
                && id.starts_with("bar.")
                && id != "bar"
            {
                assert!(Part::of(id).is_some(), "{id}");
            }
        });
        let row = status_row();
        for slot in 0..MAX_STATUSES {
            assert!(row.find(&ids::status(slot)).is_some());
            assert!(Part::of(&ids::status_part(slot, "cool")).is_some());
        }
    }

    #[test]
    fn the_keys_and_places_match_the_sims_bar() {
        let tables = tables();
        assert_eq!(tables.bar.weapon + tables.bar.utility, PLACES);
        assert_eq!(crate::combat::ABILITY_KEYS.len(), PLACES);
    }

    fn feedback_world(actor: Combatant) -> (World, Entity) {
        let mut world = World::new();
        world.insert_resource(Time::<()>::default());
        world.init_resource::<BarFeedback>();
        let player = world
            .spawn((LocalPlayer, actor, CombatIntent::default()))
            .id();
        (world, player)
    }

    fn advance(world: &mut World, secs: f32) {
        world
            .resource_mut::<Time>()
            .advance_by(Duration::from_secs_f32(secs));
        world.run_system_once(track_feedback).unwrap();
    }

    fn press(world: &mut World, player: Entity, place: usize) {
        world.get_mut::<CombatIntent>(player).unwrap().ability = Some(place);
        world.run_system_once(note_press).unwrap();
        world.get_mut::<CombatIntent>(player).unwrap().ability = None;
        tick(
            &mut world.get_mut::<Combatant>(player).unwrap(),
            Some(place),
        );
    }

    #[test]
    fn a_refused_press_flashes_its_frame_and_says_why() {
        let mut actor = carbine();
        actor.stamina = 1.0;
        let (mut world, player) = feedback_world(actor);
        advance(&mut world, 0.0);
        press(&mut world, player, 0);
        advance(&mut world, 0.016);
        let fb = world.resource::<BarFeedback>();
        assert!(fb.danger[0] > 0.2, "the pressed square flashes");
        assert!(fb.danger[1] <= 0.0);
        let (why, _) = fb.reason.unwrap();
        assert_eq!(
            crate::ui::progress::ability_words(why),
            "Not enough stamina"
        );
        advance(&mut world, REFUSED_FLASH_SECS);
        let fb = world.resource::<BarFeedback>();
        assert!(fb.danger[0] <= 0.0, "the flash is brief");
        assert!(fb.reason.is_some(), "the word stays longer");
        advance(&mut world, REASON_SECS);
        assert!(world.resource::<BarFeedback>().reason.is_none());
    }

    #[test]
    fn a_cooldown_refusal_names_the_whole_seconds_left() {
        let (mut world, player) = feedback_world(carbine());
        advance(&mut world, 0.0);
        press(&mut world, player, 0);
        for _ in 0..60 {
            tick(&mut world.get_mut::<Combatant>(player).unwrap(), None);
        }
        advance(&mut world, 0.016);
        assert_eq!(
            world.resource::<BarFeedback>().reason,
            None,
            "taken, not refused"
        );
        // The commitment is over; the same place again is on cooldown.
        for _ in 0..60 {
            tick(&mut world.get_mut::<Combatant>(player).unwrap(), None);
        }
        press(&mut world, player, 0);
        advance(&mut world, 0.016);
        let fb = world.resource::<BarFeedback>();
        let (why, _) = fb.reason.unwrap();
        let words = crate::ui::progress::ability_words(why);
        assert!(words.starts_with("On cooldown, "), "{words}");
        assert!(words.ends_with(" s"), "{words}");
        assert!(fb.danger[0] > 0.0);
    }

    #[test]
    fn a_press_the_sim_takes_pulses_the_frame_once() {
        let (mut world, player) = feedback_world(carbine());
        advance(&mut world, 0.0);
        press(&mut world, player, 0);
        advance(&mut world, 0.016);
        let fb = world.resource::<BarFeedback>();
        assert!(fb.accent[0] > 0.1, "the committed ability lit its square");
        assert!(fb.danger[0] <= 0.0);
        assert!(fb.reason.is_none());
        // Under way, no second pulse.
        advance(&mut world, TAKEN_FLASH_SECS);
        tick(&mut world.get_mut::<Combatant>(player).unwrap(), None);
        advance(&mut world, 0.016);
        assert!(world.resource::<BarFeedback>().accent[0] <= 0.0);
    }

    #[test]
    fn an_instant_press_is_seen_in_its_cooldown_starting() {
        let blade = with_bar(
            Combatant::default().carrying([Some(Weapon::Blade), None], 0),
            &[(Place::Weapon(Weapon::Blade, 0), "reflect_shot")],
        );
        let (mut world, player) = feedback_world(blade);
        advance(&mut world, 0.0);
        assert!(
            world
                .get::<Combatant>(player)
                .unwrap()
                .ability_at(0)
                .unwrap()
                .is_instant()
        );
        press(&mut world, player, 0);
        assert!(
            world
                .get::<Combatant>(player)
                .unwrap()
                .ability_in_use()
                .is_none(),
            "an instant press never becomes the action"
        );
        advance(&mut world, 0.016);
        assert!(world.resource::<BarFeedback>().accent[0] > 0.1);
    }

    #[test]
    fn an_empty_place_that_could_be_filled_draws_a_plus_and_the_caption() {
        let (tables, catalogue) = (tables(), catalogue());
        let actor = with_bar(
            Combatant::default().carrying([Some(Weapon::Carbine), None], 0),
            &[],
        );
        let known: std::collections::BTreeSet<String> =
            ["aimed_burst".to_owned()].into_iter().collect();
        let mut bar = Bar::default();
        let open = open_places(Weapon::Carbine, &bar, &known, &tables);
        assert_eq!(open, [true, true, true, false, false]);
        let view = bar_view(&actor, &tables, &catalogue, 1.0, open);
        assert!(view.hint, "the caption says where to fill it");
        assert!(view.places[0].open && !view.places[3].open);
        let feedback = BarFeedback::default();
        let tints = Tints {
            hand: Color::WHITE,
            holstered: Color::WHITE,
        };
        let plus = want(Part::Plus(0), &view, &[], &feedback, &catalogue, &tints);
        assert_eq!(plus.shown, Some(true));
        let hint = want(Part::Hint, &view, &[], &feedback, &catalogue, &tints);
        assert_eq!(hint.text.as_deref(), Some("L: abilities"));
        let empty = want(Part::Empty(0), &view, &[], &feedback, &catalogue, &tints);
        assert_eq!(
            empty.shown,
            Some(false),
            "the plus stands in for the dim mark"
        );

        // Once the one ability is placed nothing else could go anywhere.
        bar.set(Place::Weapon(Weapon::Carbine, 0), "aimed_burst");
        let open = open_places(Weapon::Carbine, &bar, &known, &tables);
        assert_eq!(open, [false; PLACES]);
        let view = bar_view(&carbine(), &tables, &catalogue, 1.0, open);
        assert!(!view.hint);
        let hint = want(Part::Hint, &view, &[], &feedback, &catalogue, &tints);
        assert_eq!(hint.text.as_deref(), Some(""));
    }

    #[test]
    fn the_names_show_after_a_change_and_then_only_the_key() {
        let (tables, catalogue) = (tables(), catalogue());
        let mut feedback = BarFeedback::default();
        let mut actor = carbine();
        let view = bar_view(&actor, &tables, &catalogue, 1.0, [false; PLACES]);
        let tints = Tints {
            hand: Color::WHITE,
            holstered: Color::WHITE,
        };
        let name = |feedback: &BarFeedback| {
            want(Part::Name(0), &view, &[], feedback, &catalogue, &tints)
                .text
                .unwrap()
        };
        assert_eq!(name(&feedback), "", "before the bar is first seen");
        feedback.step(&actor, DT);
        assert_eq!(name(&feedback), "Aimed burst");
        // Time passes with nothing changing: the name goes.
        while feedback.names > 0.0 {
            feedback.step(&actor, DT);
        }
        assert_eq!(name(&feedback), "");
        // A swap of the weapon in hand brings the names back.
        actor = actor.carrying([Some(Weapon::Carbine), Some(Weapon::Pistol)], 1);
        feedback.step(&actor, DT);
        assert!(feedback.names > 0.0);
    }

    #[test]
    fn the_nodes_follow_the_view() {
        let mut actor = carbine();
        let mut world = World::new();
        world.init_resource::<BarFeedback>();
        world.init_resource::<IconHandles>();
        tick(&mut actor, Some(0));
        world.spawn((LocalPlayer, actor));
        let node = |world: &mut World, id: String, text: bool| {
            let mut entity = world.spawn((Tags::new().with(Tags::TEST_ID, &id), Node::default()));
            if text {
                entity.insert(Text(String::new()));
            }
            entity.id()
        };
        let cool = node(&mut world, ids::part(0, "cool"), false);
        let secs = node(&mut world, ids::part(0, "secs"), true);
        let empty = node(&mut world, ids::part(2, "empty"), false);
        let other = node(&mut world, ids::part(1, "cool"), false);
        world.run_system_once(update_bar).unwrap();
        let height = world.get::<Node>(cool).unwrap().height;
        let Val::Percent(share) = height else {
            panic!("a percent, got {height:?}");
        };
        assert!(share > 90.0, "just pressed, nearly all of it left: {share}");
        assert_eq!(world.get::<Text>(secs).unwrap().0, "10");
        assert_eq!(world.get::<Node>(empty).unwrap().display, Display::Flex);
        assert_eq!(world.get::<Node>(other).unwrap().display, Display::None);
    }
}
