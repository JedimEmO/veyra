//! The sheet's Loadout tab: where a member puts the abilities they know.
//!
//! [`view`] is a pure function of the sheet, the bar and the tables, so a
//! test reads it without a window. Nothing here decides a rule: which
//! ability may go in which place is [`veyra_sim::ability::change`]'s
//! answer, a press on a place or an ability is a [`BarRequest`] put in the
//! sim's inbox, and the screen is rebuilt from the bar as the sim has it.

use bevy::ecs::prelude::*;
use bevy::input_focus::InputFocus;
use bevy::math::Vec2;
use bevy::picking::hover::Hovered;
use bevy::picking::pointer::{PointerLocation, PointerPress};
use bevy::ui::ScrollPosition;
use slotted::menu::MenuChoice;
use slotted::theme::Role;
use slotted::ui::ScrollPanel;
use slotted::ui::def::{
    Align, Justify, Layout, Length, Overflow, Padding, Tags, TextRole, UiNodeDef,
};
use slotted::ui::{ScreenStack, TestId};
use strata_registry::progression::SkillGraph;
use veyra_sim::ability::{
    self, AbilityRow, Bar, BarAnswer, BarInbox, BarRefusal, BarRequest, EffectKind, Figure, Place,
    SlotKind, SlotRefusal,
};
use veyra_sim::combat::Weapon;
use veyra_sim::combat_tables::CombatTables;
use veyra_sim::progression::{CharacterSheet, apply};

use super::{
    SheetState, column, english, num, on_top, panel, row, separator, text, text_in, with_id,
};
use crate::icons::{self, EMPTY_PLACE};
use crate::strings::BAR_KEY_WORDS as KEY_WORDS;

/// The `menu` tag prefix of a place: `loadout.place:Carbine:0`.
const PLACE_TAG: &str = "loadout.place:";
/// The `menu` tag prefix of a known ability: `loadout.ability:aimed_burst`.
const ABILITY_TAG: &str = "loadout.ability:";
/// The `test_id` prefix of a place's card.
const PLACE_ID: &str = "place:";
/// The `test_id` prefix of a known ability's row.
const KNOWN_ID: &str = "known:";
/// The `menu` tag prefix of an ability not yet learned.
const LOCKED_TAG: &str = "loadout.locked:";
/// The `test_id` prefix of an unlearned ability's row.
const LOCKED_ID: &str = "locked:";
/// What `refocus` holds to ask for the first row of KNOWN.
const FIRST_KNOWN: &str = "known";
/// Frames after the pointer moves in which the card under it is looked for:
/// the picking that says what is under it can lag the move by a frame.
const POINTER_FRAMES: u8 = 3;
/// The `test_id` of the column of places.
const LEFT_ID: &str = "loadout.left";
/// The `test_id` of the column of known and unlearned abilities.
const MIDDLE_ID: &str = "loadout.middle";
/// The `test_id` of the line under the columns.
pub const NOTICE_ID: &str = "loadout.notice";
/// The `test_id` of the card of the ability in focus.
pub const CARD_ID: &str = "loadout.card";

/// The classes in the order the tab lists them: the sidearm first, then
/// the guns, then the blades.
const ORDER: [Weapon; 6] = [
    Weapon::Pistol,
    Weapon::Carbine,
    Weapon::Rifle,
    Weapon::Blade,
    Weapon::Greatblade,
    Weapon::Glaive,
];

/// What the tab remembers between rebuilds.
#[derive(Resource, Debug, Default, Clone, PartialEq)]
pub struct LoadoutState {
    /// The place being filled; `None` lists every known ability.
    pub filling: Option<Place>,
    /// The ability the card describes, as far as a choice or the keyboard's
    /// focus has named one.
    pub focus: Option<String>,
    /// What the last request answered.
    pub notice: Option<String>,
    /// The `test_id` the rebuilt screen puts its focus on.
    pub refocus: Option<String>,
    /// The requests sent and not yet answered: the place, and the name of
    /// what was put there (none for a clear).
    asked: Vec<(Place, Option<String>)>,
    /// The `test_id` the keyboard's focus was last seen on, so a focus put
    /// back after a rebuild is not mistaken for a move to it.
    seen_focus: Option<String>,
    /// Where the list of abilities stood when the pointer put another
    /// card up, put back on the rebuilt screen: a rebuild starts a scroll at
    /// the top, and the row under the pointer would move away from it.
    scroll: Option<f32>,
}

/// An ability in a place, as a square shows it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Held {
    /// Its id.
    pub id: String,
    /// Its name.
    pub name: String,
    /// Its catalogue id.
    pub icon: String,
}

/// One place of the bar.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlaceView {
    /// Where it is.
    pub place: Place,
    /// The key that presses it.
    pub key: &'static str,
    /// What is in it.
    pub held: Option<Held>,
    /// It is the place being filled.
    pub filling: bool,
}

/// A class's places, or the utility's.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SectionView {
    /// The class; `None` for the utility places.
    pub class: Option<Weapon>,
    /// Its places in order.
    pub places: Vec<PlaceView>,
}

/// A known ability in the middle column.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KnownRow {
    /// Its id.
    pub id: String,
    /// Its name.
    pub name: String,
    /// Its catalogue id.
    pub icon: String,
    /// Where it is slotted, or why it cannot go in the place being filled;
    /// empty when it is free to go.
    pub note: String,
    /// The note is a refusal.
    pub refused: bool,
}

/// An ability a box still teaches, under NOT LEARNED.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LockedRow {
    /// Its id.
    pub id: String,
    /// Its name.
    pub name: String,
    /// Its catalogue id.
    pub icon: String,
    /// "Taught by {tree}: {box}".
    pub note: String,
}

/// The card of an ability.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CardView {
    /// Its id.
    pub id: String,
    /// Its name.
    pub name: String,
    /// Its catalogue id.
    pub icon: String,
    /// The line the table gives it.
    pub says: String,
    /// What it works with.
    pub works: String,
    /// What it does, in a sentence.
    pub effect: String,
    /// The label and value of each figure, effect first.
    pub figures: Vec<(String, String)>,
    /// Where it is taught.
    pub taught: String,
}

/// Everything the tab shows.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LoadoutView {
    /// A section a certified class, in the tab's order.
    pub sections: Vec<SectionView>,
    /// The utility places.
    pub utility: SectionView,
    /// The middle column.
    pub known: Vec<KnownRow>,
    /// What the trees still teach, under the middle column's known rows.
    pub locked: Vec<LockedRow>,
    /// The right column.
    pub card: Option<CardView>,
    /// The line under the columns.
    pub notice: Option<String>,
    /// The `test_id` the screen focuses first.
    pub focus_id: Option<String>,
}

/// What [`view`] is built from.
#[derive(Debug, Clone, Copy)]
pub struct Input<'a> {
    /// The graph, for where an ability is taught.
    pub graph: &'a SkillGraph,
    /// The local player's sheet.
    pub sheet: &'a CharacterSheet,
    /// Their bar, as the sim holds it.
    pub bar: &'a Bar,
    /// The tables.
    pub tables: &'a CombatTables,
    /// The tab's own memory.
    pub state: &'a LoadoutState,
}

/// The `test_id` of a place's card.
#[must_use]
pub fn place_id(place: Place) -> String {
    match place {
        Place::Weapon(class, at) => format!("{PLACE_ID}{}:{at}", class.name()),
        Place::Utility(at) => format!("{PLACE_ID}U:{at}"),
    }
}

/// The place a card's id or tag tail names.
fn parse_place(tail: &str) -> Option<Place> {
    let (class, at) = tail.split_once(':')?;
    let at = at.parse().ok()?;
    if class == "U" {
        Some(Place::Utility(at))
    } else {
        Weapon::from_name(class).map(|class| Place::Weapon(class, at))
    }
}

/// The `test_id` of an unlearned ability's row.
#[must_use]
pub fn locked_id(id: &str) -> String {
    format!("{LOCKED_ID}{id}")
}

/// The `test_id` of a known ability's row.
#[must_use]
pub fn known_id(id: &str) -> String {
    format!("{KNOWN_ID}{id}")
}

/// The key of `place`, in the sim's numbering.
pub(crate) fn key_of(place: Place, tables: &CombatTables) -> &'static str {
    let index = match place {
        Place::Weapon(_, at) => at,
        Place::Utility(at) => tables.abilities.bar.weapon + at,
    };
    english(KEY_WORDS[index.min(KEY_WORDS.len() - 1)])
}

/// "Q of Carbine", "3": where a place is, in words.
fn place_words(
    place: Place,
    tables: &CombatTables,
    in_key: &'static str,
    in_utility: &'static str,
) -> String {
    let key = key_of(place, tables);
    match place {
        Place::Weapon(class, _) => in_key
            .replace("{key}", key)
            .replace("{class}", class.name()),
        Place::Utility(_) => in_utility.replace("{key}", key),
    }
}

fn refusal_words(refusal: SlotRefusal, class: Option<Weapon>) -> String {
    let key = match refusal {
        SlotRefusal::NoPlace => "strata.loadout.refused.place",
        SlotRefusal::Unknown => "strata.loadout.refused.unknown",
        SlotRefusal::NotKnown => "strata.loadout.refused.unlearned",
        SlotRefusal::WrongKind => "strata.loadout.refused.kind",
        SlotRefusal::WrongWeapon => "strata.loadout.refused.weapon",
        SlotRefusal::Placed(_) => "strata.loadout.refused.placed",
    };
    english(key).replace("{class}", class.map_or("", Weapon::name))
}

fn figure_words(kind: EffectKind, figure: Figure, value: f32) -> (String, String) {
    let (label, shape) = match figure {
        Figure::Rounds => ("rounds", "points"),
        Figure::Damage => ("damage", "points"),
        Figure::DamagePercent => ("damage", "percent"),
        Figure::Posture => ("posture", "points"),
        Figure::PosturePercent => ("posture", "percent"),
        Figure::Reach => ("reach", "metres"),
        Figure::ReachPercent => ("reach", "percent"),
        Figure::Arc => ("arc", "degrees"),
        Figure::Distance => ("distance", "metres"),
        Figure::Seconds if kind == EffectKind::Move => ("takes", "seconds"),
        Figure::Seconds => ("lasts", "seconds"),
    };
    let label = english(match label {
        "rounds" => "strata.loadout.figure.rounds",
        "damage" => "strata.loadout.figure.damage",
        "posture" => "strata.loadout.figure.posture",
        "reach" => "strata.loadout.figure.reach",
        "arc" => "strata.loadout.figure.arc",
        "distance" => "strata.loadout.figure.distance",
        "takes" => "strata.loadout.figure.takes",
        _ => "strata.loadout.figure.lasts",
    });
    let value = english(match shape {
        "points" => "strata.loadout.value.points",
        "percent" => "strata.loadout.value.percent",
        "metres" => "strata.loadout.value.metres",
        "degrees" => "strata.loadout.value.degrees",
        _ => "strata.loadout.value.seconds",
    })
    .replace("{n}", &num(value));
    (label.to_owned(), value)
}

fn card_of(row: &AbilityRow, input: &Input<'_>) -> CardView {
    let facts = row.facts();
    let status_name = |id: &str| {
        input
            .tables
            .abilities
            .status(id)
            .map_or_else(|| id.to_owned(), |status| status.name.clone())
    };
    let subject = facts.subject.as_deref().map(|subject| match facts.what {
        EffectKind::Cut => subject.to_owned(),
        _ => status_name(subject),
    });
    let effect = english(match facts.what {
        EffectKind::Cut => "strata.loadout.effect.cut",
        EffectKind::Shot => "strata.loadout.effect.shot",
        EffectKind::Strike => "strata.loadout.effect.strike",
        EffectKind::Hit => "strata.loadout.effect.hit",
        EffectKind::Move => "strata.loadout.effect.move",
        EffectKind::Reload => "strata.loadout.effect.reload",
        EffectKind::Shield => "strata.loadout.effect.shield",
        EffectKind::Status => "strata.loadout.effect.status",
    })
    .replace("{subject}", subject.as_deref().unwrap_or(""));
    let mut figures: Vec<(String, String)> = facts
        .figures
        .iter()
        .map(|(figure, value)| figure_words(facts.what, *figure, *value))
        .collect();
    let seconds = |key: &'static str, value: f32| {
        (
            english(key).to_owned(),
            english("strata.loadout.value.seconds").replace("{n}", &num(value)),
        )
    };
    if facts.stamina > 0. {
        figures.push((
            english("strata.loadout.stamina").to_owned(),
            num(facts.stamina),
        ));
    }
    if facts.cooldown > 0. {
        figures.push(seconds("strata.loadout.cooldown", facts.cooldown));
    }
    if facts.windup > 0. {
        figures.push(seconds("strata.loadout.windup", facts.windup));
    }
    if facts.commitment > 0. {
        figures.push(seconds("strata.loadout.commitment", facts.commitment));
    }
    let classes = row
        .works_with
        .iter()
        .map(|class| class.name())
        .collect::<Vec<_>>()
        .join(", ");
    let works = match (row.slot, classes.is_empty()) {
        (SlotKind::Weapon, true) => english("strata.loadout.works_any").to_owned(),
        (SlotKind::Weapon, false) => {
            english("strata.loadout.works_with").replace("{classes}", &classes)
        }
        (SlotKind::Utility, true) => english("strata.loadout.utility_kind").to_owned(),
        (SlotKind::Utility, false) => format!(
            "{}. {}",
            english("strata.loadout.utility_kind"),
            english("strata.loadout.works_with").replace("{classes}", &classes)
        ),
    };
    let taught = veyra_sim::ability::taught_by(input.graph, input.sheet, input.bar, &row.id)
        .map_or_else(
            || english("strata.loadout.given").to_owned(),
            |b| {
                english("strata.loadout.taught")
                    .replace("{box}", &b.name)
                    .replace(
                        "{tree}",
                        input
                            .graph
                            .tree_of(&b.id)
                            .map_or("", |tree| tree.name.as_str()),
                    )
            },
        );
    CardView {
        id: row.id.clone(),
        name: row.name.clone(),
        icon: row.icon.clone(),
        says: row.says.clone(),
        works,
        effect,
        figures,
        taught,
    }
}

/// The tab for `input`.
#[must_use]
#[allow(
    clippy::too_many_lines,
    reason = "one view, read top to bottom: sections, rows, card, focus"
)]
pub fn view(input: &Input<'_>) -> LoadoutView {
    let tables = input.tables;
    let effective = apply(tables, input.graph, input.sheet);
    let known = ability::known_to(input.graph, input.sheet, input.bar);
    let held = |place: Place| {
        input
            .bar
            .get(place)
            .and_then(|id| tables.abilities.row(id))
            .map(|row| Held {
                id: row.id.clone(),
                name: row.name.clone(),
                icon: row.icon.clone(),
            })
    };
    let places_of = |class: Option<Weapon>| -> Vec<PlaceView> {
        let count = if class.is_some() {
            tables.abilities.bar.weapon
        } else {
            tables.abilities.bar.utility
        };
        (0..count)
            .map(|at| {
                let place = class.map_or(Place::Utility(at), |class| Place::Weapon(class, at));
                PlaceView {
                    place,
                    key: key_of(place, tables),
                    held: held(place),
                    filling: input.state.filling == Some(place),
                }
            })
            .collect()
    };
    let sections: Vec<SectionView> = ORDER
        .into_iter()
        .filter(|class| effective.certified.contains(class))
        .map(|class| SectionView {
            class: Some(class),
            places: places_of(Some(class)),
        })
        .collect();
    let utility = SectionView {
        class: None,
        places: places_of(None),
    };
    // A place of a class the tab has no section for is not one it can fill.
    let filling = input.state.filling.filter(|place| match place {
        Place::Weapon(class, _) => effective.certified.contains(class),
        Place::Utility(_) => true,
    });
    let rows: Vec<KnownRow> = tables
        .abilities
        .abilities
        .iter()
        .filter(|row| known.contains(&row.id))
        .filter_map(|row| {
            // Only the places the tab shows are worth naming.
            let slotted: Vec<Place> = input
                .bar
                .places_of(&row.id)
                .into_iter()
                .filter(|place| match place {
                    Place::Weapon(class, _) => effective.certified.contains(class),
                    Place::Utility(_) => true,
                })
                .collect();
            let mut note = String::new();
            let mut refused = false;
            if let Some(place) = filling {
                let mut next = input.bar.clone();
                let request = BarRequest::Set {
                    place,
                    ability: row.id.clone(),
                };
                match ability::change(&tables.abilities, &known, &mut next, &request) {
                    // The other kind of place can never take it: not listed.
                    Err(SlotRefusal::WrongKind) => return None,
                    Err(why) => {
                        refused = true;
                        note = refusal_words(
                            why,
                            match place {
                                Place::Weapon(class, _) => Some(class),
                                Place::Utility(_) => None,
                            },
                        );
                    }
                    Ok(_) => {}
                }
            }
            if note.is_empty() {
                note = slotted
                    .iter()
                    .map(|place| {
                        place_words(
                            *place,
                            tables,
                            english("strata.loadout.in"),
                            english("strata.loadout.in_utility"),
                        )
                    })
                    .collect::<Vec<_>>()
                    .join(", ");
            }
            Some(KnownRow {
                id: row.id.clone(),
                name: row.name.clone(),
                icon: row.icon.clone(),
                note,
                refused,
            })
        })
        .collect();
    let locked: Vec<LockedRow> = ability::unlearned(input.graph, input.sheet)
        .into_iter()
        // A hand-granted ability is known to the bar, so not offered twice.
        .filter(|row| !known.contains(&row.ability))
        .filter_map(|row| {
            let ability = tables.abilities.row(&row.ability)?;
            Some(LockedRow {
                id: ability.id.clone(),
                name: ability.name.clone(),
                icon: ability.icon.clone(),
                note: english("strata.loadout.locked_by")
                    .replace("{tree}", &row.tree_name)
                    .replace("{box}", &row.box_name),
            })
        })
        .collect();
    let card_id = input
        .state
        .focus
        .as_ref()
        .filter(|id| {
            rows.iter().any(|row| row.id == **id) || locked.iter().any(|row| row.id == **id)
        })
        .cloned()
        .or_else(|| {
            filling
                .and_then(|place| input.bar.get(place))
                .map(str::to_owned)
        })
        .or_else(|| rows.first().map(|row| row.id.clone()));
    let card = card_id
        .and_then(|id| {
            effective
                .abilities
                .get(&id)
                .or_else(|| tables.abilities.row(&id))
                .cloned()
        })
        .map(|row| card_of(&row, input));
    // The focus the screen starts with: a request for the first known row
    // lands on the ability in the place being filled when it has one.
    let focus_id = input.state.refocus.as_ref().and_then(|wanted| {
        if wanted == FIRST_KNOWN {
            let held = filling.and_then(|place| input.bar.get(place));
            rows.iter()
                .find(|row| Some(row.id.as_str()) == held)
                .or_else(|| rows.first())
                .map(|row| known_id(&row.id))
                .or_else(|| filling.map(place_id))
        } else {
            Some(wanted.clone())
        }
    });
    LoadoutView {
        sections,
        utility,
        known: rows,
        locked,
        card,
        notice: input.state.notice.clone(),
        focus_id,
    }
}

// ---------------------------------------------------------------------------
// The nodes
// ---------------------------------------------------------------------------

/// The size of a place's square.
const PLACE_SIZE: f32 = 56.;
/// The size of a known ability's square.
const KNOWN_SIZE: f32 = 40.;
/// The size of the symbol that heads a section.
const SYMBOL_SIZE: f32 = 24.;
/// The size of the card's square, the 256 export at half.
const CARD_SIZE: f32 = 128.;
/// The width of a section's head, beside its places.
const HEAD: f32 = 84.;
/// The width of the left and middle columns.
const COLUMN: f32 = 300.;

fn menu_card(
    role: &str,
    layout: Layout,
    children: Vec<UiNodeDef>,
    tag: &str,
    id: &str,
) -> UiNodeDef {
    UiNodeDef::Card {
        role: Role::new(role),
        layout,
        children,
        disabled: false,
        tags: Tags::new().with("menu", tag).with(Tags::TEST_ID, id),
    }
}

fn place_card(place: &PlaceView) -> UiNodeDef {
    let square = match &place.held {
        Some(held) => icons::node(&held.icon, PLACE_SIZE, None, false, None),
        None => icons::node(
            EMPTY_PLACE,
            PLACE_SIZE,
            Some("strata.bar.symbol.dim"),
            false,
            None,
        ),
    };
    let tail = place_id(place.place);
    menu_card(
        if place.filling {
            "strata.sheet.card.selected"
        } else {
            "strata.sheet.card"
        },
        Layout {
            padding: Padding::all(0.6),
            align: Some(Align::Center),
            ..column(0.3)
        },
        vec![
            square,
            text_in(place.key, TextRole::Caption, "strata.bar.key.text"),
        ],
        &format!("{PLACE_TAG}{}", tail.trim_start_matches(PLACE_ID)),
        &tail,
    )
}

fn section(section: &SectionView) -> UiNodeDef {
    // The head sits beside its places, not over them: with three classes
    // and the utility places the column then fits a 720 pixel window.
    let head = match section.class {
        Some(class) => panel(
            "invisible",
            Layout {
                width: Some(Length::Px(HEAD)),
                ..column(0.3)
            },
            vec![
                icons::node(
                    icons::symbol_of(class),
                    SYMBOL_SIZE,
                    Some("strata.bar.symbol"),
                    false,
                    None,
                ),
                text(class.name(), TextRole::Body),
            ],
        ),
        None => panel(
            "invisible",
            Layout {
                width: Some(Length::Px(HEAD)),
                ..column(0.3)
            },
            vec![text_in(
                english("strata.loadout.utility"),
                TextRole::Caption,
                "browser.status",
            )],
        ),
    };
    panel(
        "invisible",
        row(0.8),
        std::iter::once(head)
            .chain(section.places.iter().map(place_card))
            .collect(),
    )
}

fn known_row(row: &KnownRow, picked: bool) -> UiNodeDef {
    let words = if row.note.is_empty() {
        None
    } else {
        Some(text_in(
            row.note.clone(),
            TextRole::Caption,
            if row.refused {
                "strata.sheet.state.waiting"
            } else {
                "strata.sheet.state.learned"
            },
        ))
    };
    let mut lines = vec![text_in(
        row.name.clone(),
        TextRole::Body,
        "strata.sheet.name.lit",
    )];
    lines.extend(words);
    menu_card(
        if picked {
            "strata.sheet.card.selected"
        } else {
            "strata.sheet.card"
        },
        Layout {
            width: Some(Length::Px(COLUMN)),
            padding: Padding::all(0.7),
            align: Some(Align::Center),
            ..super::row(1.)
        },
        vec![
            icons::node(&row.icon, KNOWN_SIZE, None, row.refused, None),
            panel(
                "invisible",
                Layout {
                    grow: 1.,
                    align: Some(Align::Stretch),
                    ..column(0.2)
                },
                lines,
            ),
        ],
        &format!("{ABILITY_TAG}{}", row.id),
        &known_id(&row.id),
    )
}

fn locked_row(row: &LockedRow, picked: bool) -> UiNodeDef {
    menu_card(
        if picked {
            "strata.sheet.card.selected"
        } else {
            "strata.sheet.card"
        },
        Layout {
            width: Some(Length::Px(COLUMN)),
            padding: Padding::all(0.7),
            align: Some(Align::Center),
            ..super::row(1.)
        },
        vec![
            icons::node(&row.icon, KNOWN_SIZE, None, true, None),
            panel(
                "invisible",
                Layout {
                    grow: 1.,
                    align: Some(Align::Stretch),
                    ..column(0.2)
                },
                vec![
                    text_in(row.name.clone(), TextRole::Body, "strata.sheet.name.dim"),
                    text_in(
                        row.note.clone(),
                        TextRole::Caption,
                        "strata.sheet.state.waiting",
                    ),
                ],
            ),
        ],
        &format!("{LOCKED_TAG}{}", row.id),
        &locked_id(&row.id),
    )
}

fn fact(label: &str, value: &str) -> UiNodeDef {
    panel(
        "invisible",
        Layout {
            justify: Justify::SpaceBetween,
            ..row(1.)
        },
        vec![
            text_in(label, TextRole::Caption, "browser.status"),
            text(value, TextRole::Body),
        ],
    )
}

fn card_panel(card: Option<&CardView>) -> UiNodeDef {
    let mut children = Vec::new();
    match card {
        Some(card) => {
            children.push(icons::node(&card.icon, CARD_SIZE, None, false, None));
            children.push(text(card.name.clone(), TextRole::Heading));
            children.push(text(card.says.clone(), TextRole::Body));
            children.push(text_in(
                card.works.clone(),
                TextRole::Caption,
                "browser.status",
            ));
            // Where it is taught stays with the name, above the figures:
            // the figures are the part of the card that may scroll.
            children.push(text_in(
                card.taught.clone(),
                TextRole::Caption,
                "browser.status",
            ));
            children.push(separator());
            children.push(text(card.effect.clone(), TextRole::Body));
            children.extend(card.figures.iter().map(|(label, value)| fact(label, value)));
        }
        None => children.push(text(english("strata.loadout.none"), TextRole::Caption)),
    }
    with_id(
        panel(
            "browser.panel",
            Layout {
                width: Some(Length::Px(COLUMN + 20.)),
                padding: Padding::all(2.),
                align: Some(Align::Stretch),
                min_height: Some(Length::Px(0.)),
                overflow: Overflow::Scroll,
                ..column(0.5)
            },
            children,
        ),
        CARD_ID,
    )
}

/// The body of the sheet with the tab shown: three columns and the line
/// under them.
#[must_use]
pub fn body(view: &LoadoutView) -> UiNodeDef {
    let filling_known = !view.known.is_empty();
    let scrolling = |children: Vec<UiNodeDef>, width: f32, id: &str| {
        with_id(
            panel(
                "invisible",
                Layout {
                    width: Some(Length::Px(width)),
                    align: Some(Align::Stretch),
                    min_height: Some(Length::Px(0.)),
                    overflow: Overflow::Scroll,
                    ..column(1.6)
                },
                children,
            ),
            id,
        )
    };
    let mut left: Vec<UiNodeDef> = view.sections.iter().map(section).collect();
    left.push(section(&view.utility));
    let mut middle = vec![text_in(
        english("strata.loadout.known"),
        TextRole::Caption,
        "browser.status",
    )];
    if filling_known {
        let picked = view.card.as_ref().map(|card| card.id.as_str());
        middle.extend(
            view.known
                .iter()
                .map(|row| known_row(row, Some(row.id.as_str()) == picked)),
        );
    } else {
        middle.push(text(english("strata.loadout.none"), TextRole::Caption));
    }
    if !view.locked.is_empty() {
        let picked = view.card.as_ref().map(|card| card.id.as_str());
        middle.push(text_in(
            english("strata.loadout.not_learned"),
            TextRole::Caption,
            "browser.status",
        ));
        middle.extend(
            view.locked
                .iter()
                .map(|row| locked_row(row, Some(row.id.as_str()) == picked)),
        );
    }
    let notice = view.notice.as_deref().unwrap_or("");
    panel(
        "invisible",
        Layout {
            grow: 1.,
            align: Some(Align::Stretch),
            min_height: Some(Length::Px(0.)),
            ..column(1.)
        },
        vec![
            panel(
                "invisible",
                Layout {
                    grow: 1.,
                    align: Some(Align::Stretch),
                    min_height: Some(Length::Px(0.)),
                    ..row(2.)
                },
                vec![
                    scrolling(left, COLUMN, LEFT_ID),
                    // A bar, since NOT LEARNED lists below the known rows
                    // and a column with nothing to show it scrolls would
                    // read as ending at the last row it shows.
                    UiNodeDef::Scroll {
                        layout: Layout {
                            width: Some(Length::Px(COLUMN + 14.)),
                            align: Some(Align::Stretch),
                            min_height: Some(Length::Px(0.)),
                            ..column(1.6)
                        },
                        scrollbar: true,
                        children: middle,
                        tags: Tags::new().with(Tags::TEST_ID, MIDDLE_ID),
                    },
                    card_panel(view.card.as_ref()),
                ],
            ),
            with_id(text_in(notice, TextRole::Caption, "text.icon"), NOTICE_ID),
            text_in(
                english("strata.loadout.hint"),
                TextRole::Caption,
                "browser.status",
            ),
        ],
    )
}

// ---------------------------------------------------------------------------
// The systems
// ---------------------------------------------------------------------------

/// `Update`: a press on a place or on an ability. A place puts itself in
/// focus as the one being filled, or clears when it is filled and already
/// the one; an ability asks the sim to put it there. The sim's answer is
/// read by [`answer_sim`].
pub fn answer_choices(
    mut choices: MessageReader<MenuChoice>,
    mut loadout: ResMut<LoadoutState>,
    mut sheet_state: ResMut<SheetState>,
    mut inbox: ResMut<BarInbox>,
    players: Query<&Bar, With<veyra_sim::LocalPlayer>>,
) {
    for choice in choices.read() {
        if choice.screen != super::kind() {
            continue;
        }
        if let Some(tail) = choice.id.strip_prefix(PLACE_TAG) {
            let Some(place) = parse_place(tail) else {
                continue;
            };
            let filled = players.single().is_ok_and(|bar| bar.get(place).is_some());
            loadout.notice = None;
            if loadout.filling == Some(place) && filled {
                inbox.push(BarRequest::Clear { place });
                loadout.asked.push((place, None));
                loadout.refocus = Some(place_id(place));
            } else {
                loadout.filling = Some(place);
                loadout.focus = players
                    .single()
                    .ok()
                    .and_then(|bar| bar.get(place).map(str::to_owned));
                loadout.refocus = Some(FIRST_KNOWN.to_owned());
            }
            sheet_state.dirty = true;
        } else if let Some(id) = choice.id.strip_prefix(LOCKED_TAG) {
            // Read, never chosen: a press only puts its card up.
            loadout.focus = Some(id.to_owned());
            loadout.notice = None;
            sheet_state.dirty = true;
        } else if let Some(id) = choice.id.strip_prefix(ABILITY_TAG) {
            loadout.focus = Some(id.to_owned());
            match loadout.filling {
                Some(place) => {
                    inbox.push(BarRequest::Set {
                        place,
                        ability: id.to_owned(),
                    });
                    let name = veyra_sim::combat_tables::current()
                        .abilities
                        .row(id)
                        .map_or_else(|| id.to_owned(), |row| row.name.clone());
                    loadout.asked.push((place, Some(name)));
                    loadout.refocus = Some(place_id(place));
                    loadout.notice = None;
                }
                None => {
                    loadout.notice = Some(english("strata.loadout.pick_first").to_owned());
                }
            }
            sheet_state.dirty = true;
        }
    }
}

/// `Update`, after [`answer_choices`]: the sim's answer to a request, said
/// in the line under the columns, and the screen rebuilt from the bar as
/// the sim now has it.
pub fn answer_sim(
    mut answers: MessageReader<BarAnswer>,
    mut loadout: ResMut<LoadoutState>,
    mut sheet_state: ResMut<SheetState>,
) {
    for answer in answers.read() {
        let asked = if loadout.asked.is_empty() {
            None
        } else {
            Some(loadout.asked.remove(0))
        };
        let tables = veyra_sim::combat_tables::current();
        loadout.notice = Some(match (answer.result, asked) {
            (Ok(place), Some((_, Some(name)))) => place_words(
                place,
                &tables,
                english("strata.loadout.set"),
                english("strata.loadout.set_utility"),
            )
            .replace("{ability}", &name),
            (Ok(place), _) => place_words(
                place,
                &tables,
                english("strata.loadout.cleared"),
                english("strata.loadout.cleared_utility"),
            ),
            (Err(BarRefusal::InFight), _) => english("strata.loadout.refused.fight").to_owned(),
            (Err(BarRefusal::NoPlayer), _) => english("strata.loadout.refused.nobody").to_owned(),
            (Err(BarRefusal::Slot(why)), asked) => refusal_words(
                why,
                asked.and_then(|(place, _)| match place {
                    Place::Weapon(class, _) => Some(class),
                    Place::Utility(_) => None,
                }),
            ),
        });
        sheet_state.dirty = true;
    }
}

/// The ability a card id or a place names: a row of KNOWN or NOT LEARNED
/// is its own, a place is what it holds.
fn ability_of(id: &str, players: &Query<&Bar, With<veyra_sim::LocalPlayer>>) -> Option<String> {
    if let Some(ability) = id
        .strip_prefix(KNOWN_ID)
        .or_else(|| id.strip_prefix(LOCKED_ID))
    {
        Some(ability.to_owned())
    } else if let Some(tail) = id.strip_prefix(PLACE_ID) {
        parse_place(tail).and_then(|place| {
            players
                .single()
                .ok()
                .and_then(|bar| bar.get(place).map(str::to_owned))
        })
    } else {
        None
    }
}

/// `Update`: the card follows the keyboard. The ability under the focus (a
/// row of KNOWN or NOT LEARNED, or a place that holds one) becomes the
/// card's, and the screen is rebuilt with the focus where it is. A move onto
/// anything else changes nothing, and so does a focus that has not moved
/// since the last look, which leaves the pointer's card alone.
pub fn follow_focus(
    stack: Res<ScreenStack>,
    focus: Option<Res<InputFocus>>,
    pointers: Query<&PointerPress>,
    ids: Query<&TestId>,
    players: Query<&Bar, With<veyra_sim::LocalPlayer>>,
    mut loadout: ResMut<LoadoutState>,
    mut sheet_state: ResMut<SheetState>,
) {
    if !sheet_state.loadout || !on_top(&stack) {
        return;
    }
    // A press moves the focus to the card under the pointer; rebuilding the
    // screen then would despawn the card before its release and swallow the
    // click. The choice the click makes sets the card itself. (The release
    // frame is safe: the click is delivered before this runs.)
    if pointers.iter().any(PointerPress::is_primary_pressed) {
        return;
    }
    let Some(id) = focus
        .as_deref()
        .and_then(InputFocus::get)
        .and_then(|entity| ids.get(entity).ok())
        .map(|id| id.0.clone())
    else {
        return;
    };
    if loadout.seen_focus.as_deref() == Some(id.as_str()) {
        return;
    }
    loadout.seen_focus = Some(id.clone());
    let Some(ability) = ability_of(&id, &players) else {
        return;
    };
    if loadout.focus.as_deref() != Some(ability.as_str()) {
        loadout.focus = Some(ability);
        loadout.refocus = Some(id);
        loadout.scroll = None;
        sheet_state.dirty = true;
    }
}

/// `Update`: the card follows the pointer too. Pointing at a place or an
/// ability puts its card up and chooses nothing; the keyboard's focus is
/// kept where it was across the rebuild. Acts when the pointer arrives on
/// a new card, not every frame it rests there.
#[allow(
    clippy::too_many_arguments,
    reason = "the screen stack, the focus, the pointers, the cards under them and the tab's own state"
)]
pub fn follow_pointer(
    stack: Res<ScreenStack>,
    focus: Option<Res<InputFocus>>,
    pointers: Query<&PointerPress>,
    ids: Query<&TestId>,
    cards: Query<(&Hovered, &TestId)>,
    scrolls: Query<(&TestId, &ScrollPanel)>,
    positions: Query<&ScrollPosition>,
    locations: Query<&PointerLocation>,
    mut moved: Local<(Option<Vec2>, u8)>,
    players: Query<&Bar, With<veyra_sim::LocalPlayer>>,
    mut loadout: ResMut<LoadoutState>,
    mut sheet_state: ResMut<SheetState>,
    mut over: Local<Option<String>>,
) {
    if !sheet_state.loadout || !on_top(&stack) {
        return;
    }
    if pointers.iter().any(PointerPress::is_primary_pressed) {
        return;
    }
    // Only a pointer that moved puts a card up. A rebuild or a scroll moves
    // rows under a still pointer, and answering that would flip the card
    // between the rows that pass under it for as long as it rests.
    let at = locations
        .iter()
        .find_map(|pointer| pointer.location.as_ref().map(|location| location.position));
    if at != moved.0 {
        moved.0 = at;
        moved.1 = POINTER_FRAMES;
    }
    if moved.1 == 0 {
        return;
    }
    moved.1 -= 1;
    let now = cards
        .iter()
        .find(|(hovered, id)| {
            hovered.get()
                && [KNOWN_ID, LOCKED_ID, PLACE_ID]
                    .iter()
                    .any(|prefix| id.0.starts_with(prefix))
        })
        .map(|(_, id)| id.0.clone());
    if now == *over {
        return;
    }
    over.clone_from(&now);
    let Some(ability) = now.and_then(|id| ability_of(&id, &players)) else {
        return;
    };
    if loadout.focus.as_deref() != Some(ability.as_str()) {
        // Done looking until the pointer moves again: the rebuilt rows take
        // a few frames to settle, and what is under the pointer in them is
        // not where it pointed.
        moved.1 = 0;
        loadout.focus = Some(ability);
        loadout.refocus = focus
            .as_deref()
            .and_then(InputFocus::get)
            .and_then(|entity| ids.get(entity).ok())
            .map(|id| id.0.clone());
        let state = &mut *loadout;
        state.seen_focus.clone_from(&state.refocus);
        loadout.scroll = scrolls
            .iter()
            .find(|(id, _)| id.0 == MIDDLE_ID)
            .and_then(|(_, panel)| positions.get(panel.viewport).ok())
            .map(|at| at.y);
        sheet_state.dirty = true;
    }
}

/// `Update`, after the rebuild: the list of abilities of a screen rebuilt
/// for the pointer's card takes the scroll it had.
pub fn restore_scroll(
    mut loadout: ResMut<LoadoutState>,
    fresh: Query<(&TestId, &ScrollPanel), Added<ScrollPanel>>,
    mut positions: Query<&mut ScrollPosition>,
) {
    if loadout.scroll.is_none() {
        return;
    }
    for (id, panel) in &fresh {
        if id.0 == MIDDLE_ID
            && let (Some(y), Ok(mut at)) =
                (loadout.scroll.take(), positions.get_mut(panel.viewport))
        {
            at.y = y;
        }
    }
}

/// Registers the tab's state and systems after the sheet's own choices.
pub(super) fn register(app: &mut bevy::app::App) {
    app.init_resource::<LoadoutState>();
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, reason = "the shipped tables have these")]

    use super::*;
    use strata_registry::progression::XpKind;

    fn sheet() -> CharacterSheet {
        let mut sheet = CharacterSheet::default();
        for id in ["blade.novice", "marksman.novice", "marksman.carbine.1"] {
            sheet.learned.insert(id.to_owned());
        }
        sheet.credit(XpKind::Ranged, 150);
        sheet
    }

    fn bar(given: &[&str]) -> Bar {
        Bar {
            granted: given.iter().map(|id| (*id).to_owned()).collect(),
            ..Bar::default()
        }
    }

    fn built(sheet: &CharacterSheet, bar: &Bar, state: &LoadoutState) -> LoadoutView {
        view(&Input {
            graph: &SkillGraph::shipped(),
            sheet,
            bar,
            tables: &veyra_sim::combat_tables::current(),
            state,
        })
    }

    fn ids(view: &LoadoutView) -> Vec<&str> {
        view.known.iter().map(|row| row.id.as_str()).collect()
    }

    #[test]
    fn a_section_is_a_certified_class_in_the_tabs_order() {
        let graph = SkillGraph::shipped();
        let tables = veyra_sim::combat_tables::current();
        let arrival = sheet();
        let certified = apply(&tables, &graph, &arrival).certified;
        let all = built(&arrival, &bar(&[]), &LoadoutState::default());
        let classes: Vec<Weapon> = all.sections.iter().filter_map(|s| s.class).collect();
        assert!(classes.contains(&Weapon::Carbine));
        assert!(
            !classes.contains(&Weapon::Rifle),
            "the rifle is not certified"
        );
        assert!(classes.iter().all(|class| certified.contains(class)));
        let order: Vec<usize> = classes
            .iter()
            .map(|class| ORDER.iter().position(|c| c == class).unwrap())
            .collect();
        assert!(
            order.windows(2).all(|pair| pair[0] < pair[1]),
            "{classes:?}"
        );
        assert_eq!(all.sections[0].places.len(), tables.abilities.bar.weapon);
        assert_eq!(all.utility.places.len(), tables.abilities.bar.utility);
        let keys: Vec<&str> = all.utility.places.iter().map(|p| p.key).collect();
        assert_eq!(keys, ["3", "4"]);
    }

    #[test]
    fn known_is_filtered_by_the_place_in_focus_with_the_reason_from_the_sim() {
        let arrival = sheet();
        let given = bar(&["aimed_burst", "rapid_reload", "heavy_cut"]);
        let all = built(&arrival, &given, &LoadoutState::default());
        assert_eq!(
            ids(&all).len(),
            4,
            "every known ability with no place in focus"
        );

        let state = |place| LoadoutState {
            filling: Some(place),
            ..LoadoutState::default()
        };
        let carbine = built(&arrival, &given, &state(Place::Weapon(Weapon::Carbine, 0)));
        assert_eq!(
            ids(&carbine),
            ["heavy_cut", "butt_strike", "aimed_burst"],
            "no utility in a class place"
        );
        let heavy = &carbine.known[0];
        assert!(heavy.refused);
        assert_eq!(heavy.note, "Does not work with Carbine");
        assert!(!carbine.known[1].refused && !carbine.known[2].refused);

        let utility = built(&arrival, &given, &state(Place::Utility(0)));
        assert_eq!(ids(&utility), ["rapid_reload"]);
    }

    #[test]
    fn a_slotted_ability_says_where() {
        let arrival = sheet();
        let mut given = bar(&["aimed_burst"]);
        given.set(Place::Weapon(Weapon::Carbine, 1), "aimed_burst");
        let view = built(
            &arrival,
            &given,
            &LoadoutState {
                filling: Some(Place::Weapon(Weapon::Carbine, 0)),
                ..LoadoutState::default()
            },
        );
        let burst = view
            .known
            .iter()
            .find(|row| row.id == "aimed_burst")
            .unwrap();
        assert_eq!(burst.note, "In E of Carbine");
        assert!(!burst.refused, "the sim moves it");
        let held = view
            .sections
            .iter()
            .find(|s| s.class == Some(Weapon::Carbine))
            .unwrap();
        assert_eq!(held.places[1].held.as_ref().unwrap().id, "aimed_burst");
        assert!(held.places[0].filling);
    }

    #[test]
    fn the_card_is_the_focus_with_figures_from_the_sim_and_where_it_is_taught() {
        let arrival = sheet();
        let given = bar(&["aimed_burst", "heavy_cut"]);
        // Every ability has a box that teaches it, learned or not.
        let by_hand = built(&arrival, &given, &LoadoutState::default());
        let card = by_hand.card.as_ref().unwrap();
        assert_eq!(
            card.id, "heavy_cut",
            "the first known when nothing is in focus"
        );
        assert!(card.taught.contains("Blade"), "{}", card.taught);
        assert!(card.taught.contains("Heavy cut") || card.taught.contains("Taught"));

        let focused = built(
            &arrival,
            &given,
            &LoadoutState {
                focus: Some("aimed_burst".to_owned()),
                ..LoadoutState::default()
            },
        );
        let card = focused.card.as_ref().unwrap();
        assert_eq!(card.name, "Aimed burst");
        assert!(!card.says.is_empty());
        assert!(
            card.figures.iter().any(|(label, _)| label == "Cooldown"),
            "{:?}",
            card.figures
        );
        assert_eq!(card.taught, "Taught by Carbine 1, Marksman");
        let by_hand = built(
            &arrival,
            &bar(&["rapid_reload"]),
            &LoadoutState {
                focus: Some("rapid_reload".to_owned()),
                ..LoadoutState::default()
            },
        );
        assert_eq!(by_hand.card.as_ref().unwrap().taught, "Given by hand");
        // Disarm has two teachers; the line names the one this member learned.
        let mut smith = sheet();
        smith.learned.insert("blade.cuts.2".to_owned());
        let disarm = built(
            &smith,
            &bar(&[]),
            &LoadoutState {
                focus: Some("disarm".to_owned()),
                ..LoadoutState::default()
            },
        );
        assert_eq!(
            disarm.card.as_ref().unwrap().taught,
            "Taught by One hand 2, Blade"
        );
    }

    #[test]
    fn an_instant_focus_and_the_status_it_gives_read_rightly_on_the_card() {
        let arrival = sheet();
        for (id, status) in [("focus", "Focused"), ("kinetic_absorb", "Fortified")] {
            let view = built(
                &arrival,
                &bar(&[id]),
                &LoadoutState {
                    focus: Some(id.to_owned()),
                    ..LoadoutState::default()
                },
            );
            let card = view.card.as_ref().unwrap();
            assert_eq!(card.id, id);
            assert!(card.effect.contains(status), "{}", card.effect);
            assert!(!card.says.is_empty() && !card.says.contains('\u{2014}'));
            let has = |label: &str| card.figures.iter().any(|(l, _)| l == label);
            assert!(has("Cooldown") && has("Stamina"), "{:?}", card.figures);
            if id == "focus" {
                assert!(
                    !has("Windup"),
                    "an instant press has no windup: {:?}",
                    card.figures
                );
            }
        }
    }

    #[test]
    fn a_place_and_its_id_round_trip() {
        for place in [Place::Weapon(Weapon::Greatblade, 2), Place::Utility(1)] {
            let id = place_id(place);
            assert_eq!(parse_place(id.strip_prefix(PLACE_ID).unwrap()), Some(place));
        }
        assert_eq!(parse_place("Nonsense:1"), None);
    }
}
