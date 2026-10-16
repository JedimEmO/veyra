//! The one card: what any item shows of itself, the same anatomy for a
//! weapon, a module, a bag and a lot (ticket 0007, "The card").
//!
//! A card is its item's name and kind, where it came from, what it does in
//! the game's units, and its quality lines as bars on a track of nought to
//! a thousand with a tick at store grade. What it does is read against a
//! weapon the character carries ([`Against`]): a module's lines are the
//! figures that weapon would have with it, each with its change against
//! what is in that mount now, and a weapon's are its magazine, reload and
//! interval as fitted, against the same weapon bare. Every figure is the
//! sim's (`strata_sim::fitting::lines`); the card only words it
//! ([`value_words`], [`change_words`]), and a factor is never shown.
//!
//! A lot's card is the same frame in a form of its own: its name is its
//! deposit's (`Calder salt`), its kind its family, class and type, and it
//! has a line for each attribute its class carries and no other, each on a
//! track that draws the range its type can have anywhere as a band, with
//! the lot's figure marked in it and `of 300 to 950` beside it. The lines
//! and their spans are the sim's (`strata_sim::deposits::lot_lines`).
//!
//! One painter draws it in both places it appears. Pinned in a screen's
//! pane it is the widget [`WIDGET`], a `custom` node the screen repaints
//! with [`repaint`] as the slot of interest changes. Raised on hover it is
//! the same widget inside slotted's tooltip, put there by [`CardPart`],
//! which stands in for slotted's own name and count parts: the short card
//! on hover, the long one with Shift held. A screen that pins the card
//! asks for the raised one to stand down ([`CardContext::set_quiet`]),
//! since it would say the pane over again on top of the cells beside it.
//!
//! The words are [`CardParams`], made from a stack's record by
//! [`CardParams::of`], so what a card says can be checked without a screen.

use std::collections::BTreeMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, PoisonError};

use bevy::asset::{AssetServer, Assets};
use bevy::ecs::prelude::*;
use bevy::picking::Pickable;
use bevy::ui::widget::{ImageNode, NodeImageMode};
use bevy::ui::{AlignItems, FlexDirection, JustifyContent, Node, PositionType, UiRect, Val};
use serde::Deserialize;
use slotted::model::Value as Model;
use slotted::registry::Value;
use slotted::theme::{ActiveTheme, Paint, Role, Theme, Themed};
use slotted::ui::def::{Tags, TextOpts, TextRole, UiNodeDef, WidgetKind};
use slotted::ui::screen::{SpawnCtx, Widget, WidgetRegistry};
use slotted::ui::tooltip::{ItemCountPart, ItemNamePart};
use slotted::ui::{
    LocKey, ScreenKind, SemanticLabel, SemanticRole, TooltipCtx, TooltipPart, TooltipTier,
    WidgetNode,
};
use strata_registry::GameRegistries;
use strata_registry::item::{
    Fitted, HANDS, ItemCatalogue, ItemDefinition, ItemKind, Property, QUALITY_MAX, Record,
    STORE_GRADE,
};
use strata_registry::pictures::{PICTURES_PATH, PictureSet};
use strata_registry::schematic::{SlotDef, Takes};
use strata_registry::{Attribute, Reading};
use strata_sim::blueprint::{Blueprints, Need};
use strata_sim::combat::{Combatant, FireMode, WeaponSpec};
use strata_sim::crafting::{CraftState, Made};
use strata_sim::deposits::{self, Deposit};
use strata_sim::fitting::{self, CutFigure, FitRefusal, Line, MountState, Row, Unit};
use strata_sim::items::Hands;
use strata_sim::progression::CharacterSheet;
use strata_sim::{LocalPlayer, Registries};

use super::strings::{counted, english};
use super::weapon_diagram::WeaponPictures;

/// The card's widget kind.
pub const WIDGET: &str = "strata:item_card";
/// The widget kind of one quality line's bar.
pub const BAR_WIDGET: &str = "strata:quality_bar";

/// The asset source the baked pictures are mounted as.
pub(crate) const GENERATED_SOURCE: &str = "generated";
/// Where the pictures sit under it: the directory of the index, which an
/// item's `picture` is relative to. Taken from the index's own path, so
/// the two cannot part.
fn pictures_dir() -> &'static str {
    PICTURES_PATH.rsplit_once('/').map_or("", |(dir, _)| dir)
}

/// A baked file as the asset server is asked for it.
pub(crate) fn generated(file: &str) -> String {
    format!("{GENERATED_SOURCE}://{file}")
}

/// How many pixels square an item's image is asked for: the largest size a
/// cell or a card draws one at, so the library's export of at least that
/// many is picked and shrunk, never stretched.
pub const ICON_PX: u32 = 128;

/// The image an item is drawn by and whether the item's colour tints it.
#[derive(Debug, Clone, PartialEq)]
pub struct ItemArt {
    /// The asset path, with its source's prefix where it has one.
    pub path: String,
    /// The tint a tintable image takes; none for a painted square.
    pub tint: Option<bevy::color::Color>,
}

/// Every item's image by its id, filled by the stores' `follow_registries`
/// from the one rule ([`item_art`]) so a card, which has no registries of
/// its own to ask, draws what the grid draws.
#[derive(Resource, Debug, Clone, Default)]
pub struct ItemImages(pub std::collections::HashMap<String, ItemArt>);

/// Which image `def` is drawn by: [`strata_registry::item_image`] with the
/// asset path of its source. A baked picture counts only where `pictures`
/// and `vehicles` are given, which the caller does only when the generated
/// source can be loaded from. `None` is the stand-in shape.
#[must_use]
pub fn item_art(
    def: &ItemDefinition,
    pictures: Option<&PictureSet>,
    vehicles: Option<&strata_registry::vehicle::VehicleModels>,
    icons: &strata_registry::IconCatalogue,
) -> Option<ItemArt> {
    let none = PictureSet::default();
    let bare = strata_registry::vehicle::VehicleModels::default();
    let image = strata_registry::item_image(
        def,
        pictures.unwrap_or(&none),
        vehicles.unwrap_or(&bare),
        icons,
        ICON_PX,
    )?;
    let [red, green, blue, _] = def.icon.color.0;
    Some(ItemArt {
        path: match image.source {
            strata_registry::ImageSource::Generated => generated(&image.path),
            strata_registry::ImageSource::Assets => image.path,
        },
        tint: image
            .tinted
            .then(|| bevy::color::Color::srgb(red, green, blue)),
    })
}

/// The whole weapon's baked picture for `def`, as an asset path, with its
/// width and height in pixels: what a row draws a weapon by where a square
/// icon would make a long gun a line. `None` for an item that is no
/// pictured weapon and for an index that has not got it.
#[must_use]
pub fn weapon_picture(pictures: &PictureSet, def: &ItemDefinition) -> Option<(String, u32, u32)> {
    if !matches!(def.kind, ItemKind::Weapon { .. }) {
        return None;
    }
    let file = format!("{}/{}", pictures_dir(), def.icon.picture.as_ref()?);
    let whole = &pictures
        .chassis
        .values()
        .find(|chassis| chassis.whole.file == file)?
        .whole;
    let [_, _, wide, high] = whole.rect;
    (wide > 0 && high > 0).then(|| (generated(&file), wide, high))
}

/// How wide the card is drawn in a tooltip, which has no pane to fill.
const RAISED_WIDTH: f32 = 320.;
/// The height of the well a weapon's picture is fitted into.
const PICTURE_HEIGHT: f32 = 84.;
/// The side of the item image at the head of a card with no weapon picture.
const CARD_ICON: f32 = 48.;
/// The width kept for a bar's numeral, so the tracks of a card line up.
const VALUE_WIDTH: f32 = 34.;

const FRAME_ROLE: &str = "strata.card";
/// The backing of a raised card. The tooltip it is raised in is glass, and
/// bright words under glass show through it in the renderer's linear
/// blend, so the card brings a ground of its own to be read on.
const RAISED_ROLE: &str = "strata.card.raised";
const NAME_ROLE: &str = "strata.card.name";
const KIND_ROLE: &str = "strata.card.kind";
const FROM_ROLE: &str = "strata.card.from";
const RULE_ROLE: &str = "strata.card.rule";
const NOTE_ROLE: &str = "strata.card.note";
const LINE_ROLE: &str = "text.label";
const TRACK_ROLE: &str = "strata.quality.track";
const FILL_ROLE: &str = "strata.quality.fill";
/// The fill of a bar that shows a weapon's condition and not a quality.
pub const CONDITION_FILL_ROLE: &str = "strata.quality.fill.condition";
/// The same bar once the weapon is tired or worn.
pub const CONDITION_WORN_FILL_ROLE: &str = "strata.quality.fill.condition.worn";
const TICK_ROLE: &str = "strata.quality.tick";
const VALUE_ROLE: &str = "strata.quality.value";
/// A lot's track, the band of its type's range on it, the band's two ends
/// and the mark at the lot's own figure.
const LOT_TRACK_ROLE: &str = "strata.quality.track.lot";
const RANGE_ROLE: &str = "strata.quality.range";
const RANGE_END_ROLE: &str = "strata.quality.range.end";
const MARK_ROLE: &str = "strata.quality.mark";
/// A craft's line: the track up to what the materials allow, behind the
/// fill, and the mark where assembly alone lands it.
const CEILING_ROLE: &str = "strata.quality.ceiling";
const FLOOR_ROLE: &str = "strata.quality.floor";
/// `of 300 to 950`, beside a lot's figure.
const OF_ROLE: &str = "strata.card.of";
const EFFECT_NAME_ROLE: &str = "strata.card.effect.name";
const EFFECT_VALUE_ROLE: &str = "strata.card.effect.value";
const EFFECT_BETTER_ROLE: &str = "strata.card.effect.better";
const EFFECT_WORSE_ROLE: &str = "strata.card.effect.worse";
const EFFECT_LATER_ROLE: &str = "strata.card.effect.later";
/// The tint a weapon's picture is drawn in: the diagram's own for a part
/// at rest, so the card and the fitting screen show one weapon.
const PICTURE_ROLE: &str = "strata.weapon.piece.standard";

/// Every role a card paints with, for the test that holds them to the
/// theme.
pub const ROLES: [&str; 26] = [
    CEILING_ROLE,
    FLOOR_ROLE,
    LOT_TRACK_ROLE,
    RANGE_ROLE,
    RANGE_END_ROLE,
    MARK_ROLE,
    OF_ROLE,
    EFFECT_NAME_ROLE,
    EFFECT_VALUE_ROLE,
    EFFECT_BETTER_ROLE,
    EFFECT_WORSE_ROLE,
    EFFECT_LATER_ROLE,
    CONDITION_FILL_ROLE,
    CONDITION_WORN_FILL_ROLE,
    FRAME_ROLE,
    RAISED_ROLE,
    NAME_ROLE,
    KIND_ROLE,
    FROM_ROLE,
    RULE_ROLE,
    NOTE_ROLE,
    LINE_ROLE,
    TRACK_ROLE,
    FILL_ROLE,
    TICK_ROLE,
    VALUE_ROLE,
];

/// One quality line as the card shows it.
#[derive(Debug, Clone, PartialEq, Eq, Default, Deserialize)]
pub struct CardLine {
    /// What the item's data calls the line.
    pub name: String,
    /// Its quality, nought to a thousand.
    pub value: u16,
    /// A sentence under its bar: where a part's line goes next. Empty
    /// where the card has nothing to add.
    #[serde(default)]
    pub note: String,
}

/// The word of a band: 0 poor, 1 fair, 2 good, anything over fine.
#[must_use]
pub fn band_word(band: u8) -> &'static str {
    words(match band {
        0 => "strata.band.0",
        1 => "strata.band.1",
        2 => "strata.band.2",
        _ => "strata.band.3",
    })
}

/// One line of a lot as the card shows it: an attribute its class carries,
/// the lot's figure, and the floor and cap its type has for it anywhere.
#[derive(Debug, Clone, PartialEq, Eq, Default, Deserialize)]
pub struct CardRange {
    /// The attribute's name in full: `Heat tolerance`.
    pub name: String,
    /// The lot's figure.
    pub value: u16,
    /// The lowest the type has.
    pub floor: u16,
    /// The highest the type has.
    pub cap: u16,
    /// A band word (`fine`) in place of the figure, for a deposit a ping
    /// has read only to a band; empty where the figure is told.
    #[serde(default)]
    pub word: String,
}

impl CardRange {
    /// `of 300 to 950`: the range the figure stands in.
    #[must_use]
    pub fn of_words(&self) -> String {
        fill(
            &fill(
                words("strata.card.lot.of"),
                "floor",
                &self.floor.to_string(),
            ),
            "cap",
            &self.cap.to_string(),
        )
    }
}

/// One thing an item does, as the card shows it: a property of a weapon,
/// its figure in the game's units, and how that differs from the figure it
/// is set against.
#[derive(Debug, Clone, PartialEq, Eq, Default, Deserialize)]
pub struct CardEffect {
    /// The property: `Magazine`.
    pub name: String,
    /// The figure with its unit: `29 rounds`.
    pub value: String,
    /// The change: `+5`, `+0.14 s worse`; empty where there is none.
    #[serde(default)]
    pub change: String,
    /// Whether the change is for the better; none where there is none.
    #[serde(default)]
    pub better: Option<bool>,
    /// The fight has no number for the property yet: the figure is what
    /// the part would do, and the card says so.
    #[serde(default)]
    pub later: bool,
}

impl CardEffect {
    /// The effect of one of the sim's lines on a weapon that fires by
    /// `mode`, which names its interval for what it is.
    #[must_use]
    pub fn of_for(line: &Line, mode: FireMode) -> Self {
        let mut effect = Self::of(line);
        property_name_for(line.property, mode).clone_into(&mut effect.name);
        effect
    }

    /// The effect of one of the sim's lines.
    #[must_use]
    pub fn of(line: &Line) -> Self {
        let change = change_words(line);
        Self {
            name: property_name(line.property).to_owned(),
            value: value_words(line),
            better: change.as_ref().map(|(_, better)| *better),
            change: change.map_or_else(String::new, |(words, _)| words),
            later: !line.live,
        }
    }
}

/// A property's name on a weapon that fires by `mode`: the interval is the
/// pause between bursts for a burst and the least time between shots for
/// the rest.
#[must_use]
pub fn property_name_for(property: Property, mode: FireMode) -> &'static str {
    if property != Property::RepeatInterval {
        return property_name(property);
    }
    let key = match mode {
        FireMode::Burst { .. } => "strata.property.repeat_interval.burst",
        FireMode::Single | FireMode::Auto => "strata.property.repeat_interval.single",
    };
    words(key)
}

/// What a property is called on a card and in a table.
#[must_use]
pub fn property_name(property: Property) -> &'static str {
    english(&format!("strata.property.{}", property.key())).unwrap_or_else(|| property.key())
}

/// A figure as a player reads it: the sim's number with its unit, `29
/// rounds`, `1.49 s`, `65%`. A property the fight has no number for reads
/// in words against the weapon's standard: `6% tighter`, or `Standard`.
#[must_use]
pub fn value_words(line: &Line) -> String {
    let figure = |key: &'static str| fill(words(key), "n", &line.value.text());
    match line.value.unit {
        Unit::Rounds => figure("strata.figure.rounds"),
        Unit::Seconds => figure("strata.figure.seconds"),
        Unit::Degrees => figure("strata.figure.degrees"),
        Unit::Percent => figure("strata.figure.percent"),
        Unit::Points => figure("strata.figure.points"),
        Unit::Metres => figure("strata.figure.metres"),
        Unit::OfStandard => {
            let off = line.off_standard().unwrap_or(0);
            if off == 0 {
                return words("strata.figure.standard").to_owned();
            }
            let side = if off < 0 { "less" } else { "more" };
            let word =
                english(&format!("strata.property.{}.{side}", line.property.key())).unwrap_or(side);
            fill(
                &fill(
                    words("strata.figure.of_standard"),
                    "n",
                    &off.abs().to_string(),
                ),
                "word",
                word,
            )
        }
    }
}

/// A line's change as a player reads it, and whether it is for the better:
/// `+5`, `+0.14 s worse`. In the line's unit, as the sim worked it out
/// between the two figures shown; a change for the worse says so in a
/// word, so its colour is not what tells it. A property the fight has no
/// number for only says which way. `None` where the figures do not differ.
#[must_use]
pub fn change_words(line: &Line) -> Option<(String, bool)> {
    let change = line.change?;
    if !line.live {
        let key = if change.better {
            "strata.figure.better"
        } else {
            "strata.figure.worse"
        };
        return Some((words(key).to_owned(), change.better));
    }
    // A true minus, which is as wide as the plus it stands in a column with.
    let signed = line.value.signed(change.by).replace('-', "\u{2212}");
    let by = match line.value.unit {
        Unit::Seconds => fill(words("strata.figure.seconds"), "n", &signed),
        Unit::Degrees => fill(words("strata.figure.degrees"), "n", &signed),
        Unit::Percent => fill(words("strata.figure.percent"), "n", &signed),
        Unit::Metres => fill(words("strata.figure.metres"), "n", &signed),
        Unit::Rounds | Unit::Points | Unit::OfStandard => signed,
    };
    Some((
        if change.better {
            by
        } else {
            fill(words("strata.figure.change.worse"), "change", &by)
        },
        change.better,
    ))
}

/// What a card is read against: the weapons the character carries, each
/// with its holder's row for it with nothing fitted (`Combatant::bare`),
/// which of them is drawn, the household's ledger of what each batch it
/// made was made from, and the blueprints it has recorded.
#[derive(Debug, Clone)]
pub struct Against {
    /// The weapon in each hand, whatever it is.
    pub hands: [Option<(Record, Row)>; HANDS],
    /// Who holds them: a melee weapon's figures are read off a combatant
    /// carrying it, and a weapon in the store has no hand to be read by.
    pub holder: Combatant,
    /// The hand whose weapon is drawn.
    pub drawn: usize,
    /// What each finished batch was made from, by its batch number: the
    /// sim's own ledger (`CraftState::made`), shared and not copied.
    pub made: Arc<BTreeMap<u32, Made>>,
    /// Every blueprint the household has recorded, by its number: the
    /// sim's own ledger (`Blueprints`), shared and not copied. A blueprint
    /// is an item whose body is here, so its card is read from it.
    pub blueprints: Arc<Blueprints>,
    /// The local member's sheet, whose boxes scale what a kit restores in
    /// the field: the card says what this member's kit does, as the fitting
    /// screen's Repair button does.
    pub sheet: Arc<CharacterSheet>,
}

/// The holder is left out of the comparison: what it contributes follows
/// the sheet and the rows already compared, and a combatant has no equality
/// of its own (its clocks tick every frame).
impl PartialEq for Against {
    fn eq(&self, other: &Self) -> bool {
        self.hands == other.hands
            && self.drawn == other.drawn
            && self.made == other.made
            && self.blueprints == other.blueprints
            && self.sheet == other.sheet
    }
}

impl Against {
    /// What `hands` hold, read for `actor`, or for an actor with no boxes
    /// when there is none: the tables' own rows.
    #[must_use]
    pub fn of(items: &ItemCatalogue, hands: &Hands, actor: Option<&Combatant>) -> Self {
        let fallback = Combatant::default();
        let holder = actor.unwrap_or(&fallback);
        let records = hands.saved(items).unwrap_or_default();
        Self {
            holder: holder.clone(),
            hands: records.map(|record| {
                let weapon = fitting::weapon_of(items, record.as_ref()?)?;
                Some((record?, holder.bare(weapon)))
            }),
            drawn: holder.hand(),
            made: Arc::default(),
            blueprints: Arc::default(),
            sheet: Arc::default(),
        }
    }

    /// The same with the member's sheet, so a kit's card reads what the
    /// sheet's boxes make of it.
    #[must_use]
    pub fn with_sheet(mut self, sheet: Arc<CharacterSheet>) -> Self {
        self.sheet = sheet;
        self
    }

    /// The same with the household's ledger of what it has made, so a long
    /// card can say what its batch was made from.
    #[must_use]
    pub fn with_made(mut self, made: Arc<BTreeMap<u32, Made>>) -> Self {
        self.made = made;
        self
    }

    /// The same with the household's blueprints, so a blueprint's card can
    /// say what it records.
    #[must_use]
    pub fn with_blueprints(mut self, blueprints: Arc<Blueprints>) -> Self {
        self.blueprints = blueprints;
        self
    }

    /// The hands in the order a module is measured against them: the
    /// drawn weapon first, so a part that fits both is read on the one in
    /// hand.
    fn by_reach(&self) -> impl Iterator<Item = (usize, &Record, &Row)> {
        let drawn = self.drawn;
        std::iter::once(drawn)
            .chain((0..HANDS).filter(move |hand| *hand != drawn))
            .filter_map(|hand| {
                let (record, row) = self.hands.get(hand)?.as_ref()?;
                Some((hand, record, row))
            })
    }
}

/// Everything a card says, in display words. Made from a record by
/// [`CardParams::of`]; carried through a `custom` node as its value.
#[derive(Debug, Clone, PartialEq, Eq, Default, Deserialize)]
pub struct CardParams {
    /// The item's name; empty for the card of an empty slot.
    #[serde(default)]
    pub name: String,
    /// What it is: `Weapon`, `Mod · Feed`, `Bag`.
    #[serde(default)]
    pub kind: String,
    /// Where it came from: who made it and in which batch.
    #[serde(default)]
    pub from: String,
    /// A weapon's baked picture, relative to the pictures' directory.
    #[serde(default)]
    pub picture: Option<String>,
    /// The id of the item whose image stands at the head of a card that has
    /// no weapon picture; the image is looked up in [`ItemImages`] when the
    /// card is drawn, since a card is made from the registries alone.
    #[serde(default)]
    pub icon: Option<String>,
    /// A weapon's condition, nought to a thousand.
    #[serde(default)]
    pub condition: Option<u16>,
    /// What it does, in the game's units, above the bars.
    #[serde(default)]
    pub effects: Vec<CardEffect>,
    /// The quality lines, in the item's own order.
    #[serde(default)]
    pub lines: Vec<CardLine>,
    /// A lot's lines in its class's order, each with its type's range. A
    /// card with these is drawn in the lot's form: its kind under its name
    /// and no store-grade tick.
    #[serde(default)]
    pub ranges: Vec<CardRange>,
    /// The sentences under the bars.
    #[serde(default)]
    pub notes: Vec<String>,
    /// Drawn in its own frame, to fill a pane. A raised card is drawn in
    /// the tooltip's.
    #[serde(default)]
    pub framed: bool,
}

/// `{name}` in `words` replaced by `with`.
fn fill(words: &str, name: &str, with: &str) -> String {
    words.replace(&format!("{{{name}}}"), with)
}

/// How a weapon fires, how its damage fades and how far it reaches, as
/// (name, figure) pairs. None of them is moved by a part, so they never
/// carry a change; every figure is the row's own.
#[must_use]
pub fn weapon_facts(spec: &WeaponSpec) -> Vec<(String, String)> {
    let facts = spec.facts();
    let mode = match facts.mode {
        FireMode::Single => words("strata.weapon.mode.single").to_owned(),
        FireMode::Burst { count, .. } => fill(
            words("strata.weapon.mode.burst"),
            "count",
            &count.to_string(),
        ),
        FireMode::Auto => words("strata.weapon.mode.auto").to_owned(),
    };
    let mode = if facts.hold {
        fill(words("strata.weapon.mode.hold"), "mode", &mode)
    } else {
        mode
    };
    let range = if facts.share_percent == 50 {
        words("strata.weapon.range.half")
    } else {
        words("strata.weapon.range.share")
    };
    let range = fill(range, "full", &facts.full.to_string());
    let range = fill(&range, "far", &facts.far.to_string());
    let range = fill(&range, "share", &facts.share_percent.to_string());
    let mut lines = vec![
        (words("strata.weapon.fact.mode").to_owned(), mode),
        (words("strata.weapon.fact.range").to_owned(), range),
        (
            words("strata.weapon.fact.reach").to_owned(),
            fill(
                words("strata.weapon.reach"),
                "range",
                &facts.reach.to_string(),
            ),
        ),
    ];
    // Only a weapon whose aim takes time says so; the rest settle at once.
    if facts.aim_time > 0. {
        lines.push((
            words("strata.weapon.fact.aim").to_owned(),
            fill(
                words("strata.weapon.aim.settles"),
                "time",
                &format!("{:.1}", facts.aim_time),
            ),
        ));
    }
    lines
}

/// The English of one of the card's own keys.
fn words(key: &'static str) -> &'static str {
    english(key).unwrap_or(key)
}

/// A mount's key as a word: `fore_end` reads `Fore end`.
fn mount_name(slot: &str) -> String {
    super::weapon_diagram::words(slot)
}

/// A weapon as a sentence names it: without its maker, which the card's
/// provenance line already gives, and in lower case.
pub(crate) fn plain_name(
    items: &ItemCatalogue,
    record: &strata_registry::Namespaced,
) -> Option<String> {
    let def = items.get(record)?;
    let name = def
        .name
        .strip_prefix(&def.maker)
        .map_or(def.name.as_str(), str::trim_start);
    Some(name.to_lowercase())
}

impl CardParams {
    /// The card of an empty slot: one sentence saying so.
    #[must_use]
    pub fn empty() -> Self {
        Self {
            notes: vec![words("strata.card.empty").to_owned()],
            ..Self::default()
        }
    }

    /// The card of `record`: the short one, or with `long` the one that
    /// also explains what its bars are measured against. What it does is
    /// read against the weapons of `against`; with none a module says that
    /// no weapon takes it and a weapon shows no figure. `None` for an item
    /// the catalogue does not know. A lot's card is [`Self::of_lot`]'s.
    #[must_use]
    #[allow(
        clippy::too_many_lines,
        reason = "one card read top to bottom, a kind of item an arm"
    )]
    pub fn of(
        record: &Record,
        registries: &GameRegistries,
        long: bool,
        against: Option<&Against>,
    ) -> Option<Self> {
        let items = &registries.items;
        let def = items.get(&record.item)?;
        if matches!(def.kind, ItemKind::Lot { .. }) {
            return Self::of_lot(record, registries, long);
        }
        if matches!(def.kind, ItemKind::Blueprint) {
            return Some(Self::of_blueprint(record, registries, long, against));
        }
        // A part's long card says under each line where the line goes
        // next: the first schematic that weighs it, which is the one a
        // crafting screen reads it into.
        let goes = |line: &str| match &def.kind {
            ItemKind::Component if long => goes_into(registries, &def.id, line),
            _ => String::new(),
        };
        let lines: Vec<CardLine> = def
            .lines
            .iter()
            .filter_map(|line| {
                Some(CardLine {
                    name: line.name.clone(),
                    value: *record.lines.get(&line.id)?,
                    note: goes(&line.id),
                })
            })
            .collect();
        let mut effects = Vec::new();
        let mut notes = Vec::new();
        match &def.kind {
            ItemKind::Weapon { mounts, .. } => {
                effects = weapon_effects(items, record, against);
                let fitted: Vec<&str> = record
                    .fitted
                    .values()
                    .filter_map(|fitted| items.get(&fitted.item))
                    .map(|def| def.name.as_str())
                    .collect();
                notes.push(if fitted.is_empty() {
                    words("strata.card.fitted.none").to_owned()
                } else {
                    fill(words("strata.card.fitted"), "names", &fitted.join(", "))
                });
                if long && !mounts.is_empty() {
                    let open = fitting::mounts(items, record)
                        .iter()
                        .filter(|(_, state)| !matches!(state, MountState::Shut { .. }))
                        .count();
                    notes.push(fill(
                        &fill(words("strata.card.mounts"), "open", &open.to_string()),
                        "all",
                        &mounts.len().to_string(),
                    ));
                }
            }
            ItemKind::Module { slot, fits, .. } => {
                let weapons: Vec<String> = fits
                    .iter()
                    .filter_map(|weapon| plain_name(items, weapon))
                    .collect();
                if !weapons.is_empty() {
                    notes.push(fill(
                        &fill(
                            words("strata.card.fits"),
                            "weapons",
                            &weapons.join(words("strata.card.fits.and")),
                        ),
                        "mount",
                        &mount_name(slot).to_lowercase(),
                    ));
                }
                let (figures, measured) = module_effects(items, record, slot, against);
                effects = figures;
                notes.push(measured);
            }
            ItemKind::Bag { cells } => {
                notes.push(fill(words("strata.card.bag"), "cells", &cells.to_string()));
            }
            ItemKind::Commodity => notes.push(words("strata.card.commodity").to_owned()),
            // A lot and a blueprint were returned above, each in its own
            // form. A component's lines are its whole card, each with
            // where it goes.
            ItemKind::Kit => {
                let line = record
                    .lines
                    .get(strata_registry::wear::RESTORE)
                    .copied()
                    .unwrap_or(STORE_GRADE);
                let bearer = against.map(|against| strata_sim::wear::Bearer {
                    graph: &registries.progression,
                    sheet: &against.sheet,
                });
                notes.push(fill(
                    words("strata.card.kit"),
                    "points",
                    &strata_sim::wear::field_amount_for(items.wear(), line, bearer).to_string(),
                ));
            }
            ItemKind::Vehicle { vehicle } => {
                effects = bike_effects(registries, record, vehicle);
            }
            ItemKind::Lot { .. }
            | ItemKind::Component
            | ItemKind::Blueprint
            | ItemKind::Tool { .. }
            | ItemKind::Harvester { .. } => {}
        }
        if long {
            if record.by.is_none() && !lines.is_empty() {
                notes.push(fill(
                    words("strata.card.store_grade"),
                    "grade",
                    &STORE_GRADE.to_string(),
                ));
            }
            notes.extend(against.and_then(|against| made_from(registries, record, &against.made)));
        } else {
            notes.push(words("strata.card.shift").to_owned());
        }
        Some(Self {
            name: def.name.clone(),
            kind: kind_words(def),
            from: from_words(def, record, against),
            picture: match def.kind {
                ItemKind::Weapon { .. } => def.icon.picture.clone(),
                _ => None,
            },
            icon: Some(def.id.to_string()),
            condition: record.condition,
            effects,
            lines,
            ranges: Vec::new(),
            notes,
            framed: false,
        })
    }

    /// The card of a blueprint: named for what it makes, recorded by whom
    /// and under what number, then what every run makes exactly, the
    /// product's lines as the product's own card shows them (a part's as
    /// bars with where each goes, a module's also as what it would do on
    /// the weapon carried), the runs it has left, and with `long` what
    /// each slot asks of its stock and what it was recorded from. The body
    /// is the household's ledger's (`Against::blueprints`) under the
    /// stack's batch number; an item whose body the ledger has not got
    /// says so.
    #[must_use]
    pub fn of_blueprint(
        record: &Record,
        registries: &GameRegistries,
        long: bool,
        against: Option<&Against>,
    ) -> Self {
        let items = &registries.items;
        let def = items.get(&record.item);
        let kind = def.map_or_else(String::new, kind_words);
        let print = against.and_then(|against| against.blueprints.0.get(&record.batch));
        let Some(print) = print else {
            return Self {
                name: def.map_or_else(|| record.item.to_string(), |def| def.name.clone()),
                kind,
                from: def.map_or_else(String::new, |def| from_words(def, record, None)),
                notes: vec![words("strata.card.blueprint.lost").to_owned()],
                ..Self::default()
            };
        };
        // What a run makes, read as the product's own card reads it.
        let unit = Self::of(&print.unit(record.batch, items), registries, long, against);
        let product = items.get(&print.makes);
        let plain = plain_name(items, &print.makes).unwrap_or_else(|| print.makes.to_string());
        let mut notes = vec![
            fill(
                &fill(
                    words("strata.card.blueprint.runs"),
                    "left",
                    &print.runs_left.to_string(),
                ),
                "runs",
                &print.runs.to_string(),
            ),
            fill(
                words("strata.card.blueprint.makes"),
                "names",
                &counted(&plain, 2),
            ),
        ];
        if long {
            notes.extend(print.slots.iter().map(|need| {
                let slot = &need.slot;
                let (key, name) = if slot.takes.is_resource() {
                    ("strata.card.blueprint.slot.units", "units")
                } else {
                    ("strata.card.blueprint.slot.count", "count")
                };
                let asked = needs_words(registries, slot, &need.needs);
                fill(
                    &fill(
                        &fill(words(key), "slot", &slot.name),
                        name,
                        &slot.amount().to_string(),
                    ),
                    "needs",
                    &if asked.is_empty() {
                        words("strata.card.blueprint.any").to_owned()
                    } else {
                        asked.join(" · ")
                    },
                )
            }));
            notes.push(fill(
                &fill(
                    words("strata.card.blueprint.recorded"),
                    "percent",
                    &percent_of(print.derate).to_string(),
                ),
                "batch",
                &print.prototype.to_string(),
            ));
        } else {
            notes.push(words("strata.card.shift").to_owned());
        }
        Self {
            name: fill(
                words("strata.card.blueprint.name"),
                "product",
                product.map_or(plain.as_str(), |product| product.name.as_str()),
            ),
            kind,
            from: fill(
                &fill(
                    words("strata.card.blueprint.from"),
                    "by",
                    if print.by.is_empty() {
                        words("strata.card.from.unnamed")
                    } else {
                        &print.by
                    },
                ),
                "id",
                &record.batch.to_string(),
            ),
            effects: unit
                .as_ref()
                .map_or_else(Vec::new, |unit| unit.effects.clone()),
            lines: unit.map_or_else(Vec::new, |unit| unit.lines),
            icon: def.map(|def| def.id.to_string()),
            notes,
            ..Self::default()
        }
    }

    /// The card of a lot: named for its deposit, its kind its family,
    /// class and type, where it was drawn and how many
    /// units, then a line for each attribute its class carries with the
    /// range its type has for it. The short card shows the
    /// [`SHORT_LINES`] it stands highest in, in the class's order, and
    /// says how many more the long one has. Commodity stock, which no
    /// deposit gave, says so. `None` for a record that is no lot.
    #[must_use]
    pub fn of_lot(record: &Record, registries: &GameRegistries, long: bool) -> Option<Self> {
        let (items, set) = (&registries.items, &registries.resources);
        let def = items.get(&record.item)?;
        let ItemKind::Lot { resource, .. } = &def.kind else {
            return None;
        };
        let ty = set.type_id(resource)?;
        let kind = set.get(ty)?;
        let lines = deposits::lot_lines(items, record);
        let mut shown: Vec<usize> = (0..lines.len()).collect();
        if !long && lines.len() > SHORT_LINES {
            // The highest in their spans, the earlier on a tie, then put
            // back in the class's order so both cards read the same way.
            shown.sort_by(|a, b| {
                let share = |line: usize| lines[line].span.share(lines[line].value);
                share(*b).total_cmp(&share(*a)).then(a.cmp(b))
            });
            shown.truncate(SHORT_LINES);
            shown.sort_unstable();
        }
        let units = units_words(record.count);
        let mut notes = Vec::new();
        if long {
            notes.push(match &record.origin {
                Some(origin) => fill(
                    &fill(words("strata.card.lot.long"), "place", &origin.place),
                    "type",
                    &kind.name.to_lowercase(),
                ),
                None => fill(
                    words("strata.card.lot.long.commodity"),
                    "grade",
                    &STORE_GRADE.to_string(),
                ),
            });
        } else if shown.len() < lines.len() {
            notes.push(fill(
                words("strata.card.lot.more"),
                "more",
                &(lines.len() - shown.len()).to_string(),
            ));
        } else {
            notes.push(words("strata.card.shift").to_owned());
        }
        Some(Self {
            name: record.origin.as_ref().map_or_else(
                || kind.name.clone(),
                |origin| format!("{} {}", origin.root, kind.short),
            ),
            kind: format!(
                "{} · {} · {}",
                set.family_of(ty).name,
                set.class_of(ty).name,
                kind.name
            ),
            from: match &record.origin {
                Some(origin) => fill(
                    &fill(words("strata.card.lot.from"), "place", &origin.place),
                    "units",
                    &units,
                ),
                None => fill(words("strata.card.lot.commodity"), "units", &units),
            },
            ranges: shown
                .into_iter()
                .map(|line| {
                    let line = lines[line];
                    CardRange {
                        name: line.attribute.name().to_owned(),
                        value: line.value,
                        floor: line.span.floor,
                        cap: line.span.cap,
                        word: String::new(),
                    }
                })
                .collect(),
            icon: Some(record.item.to_string()),
            notes,
            ..Self::default()
        })
    }

    /// The card of a deposit as the household reads it: at `Exact` the card
    /// of a lot sampled there with no count, at `Bands` each line a band
    /// word with the bar marked at the middle of its band, at `Type` the
    /// type and "Not read". `None` for a deposit of a type the data set has
    /// not got.
    #[must_use]
    pub fn of_deposit(
        deposit: &Deposit,
        registries: &GameRegistries,
        reading: Reading,
        long: bool,
    ) -> Option<Self> {
        let set = &registries.resources;
        let lot = deposits::lot(deposit, set, 1)?;
        let origin = lot.origin.clone()?;
        let mut card = Self::of_lot(&lot, registries, long)?;
        card.from = fill(words("strata.card.deposit.from"), "place", &origin.place);
        let type_name = set.get(deposit.ty)?.name.to_lowercase();
        match reading {
            Reading::Exact => {
                if long {
                    card.notes = vec![fill(words("strata.card.deposit.long"), "type", &type_name)];
                }
            }
            Reading::Bands => {
                let (a, b, c) = set.survey().bands;
                let edges = [0, a, b, c, 1000];
                for range in &mut card.ranges {
                    let Some(attribute) = Attribute::ALL
                        .into_iter()
                        .find(|attribute| attribute.name() == range.name)
                    else {
                        continue;
                    };
                    let Some(band) = deposits::line_band(set, deposit, attribute) else {
                        continue;
                    };
                    let band = usize::from(band).min(3);
                    band_word(u8::try_from(band).unwrap_or(3)).clone_into(&mut range.word);
                    // The mark stands in the middle of the band's share of
                    // the range, so the bar shows the word and not a figure.
                    let middle = u32::from(edges[band] + edges[band + 1]) / 2;
                    let span = u32::from(range.cap - range.floor);
                    range.value = range.floor + u16::try_from(span * middle / 1000).unwrap_or(0);
                }
                card.notes = if long {
                    vec![fill(
                        words("strata.card.deposit.banded"),
                        "type",
                        &type_name,
                    )]
                } else {
                    Vec::new()
                };
            }
            Reading::Type => {
                card.ranges.clear();
                card.notes = vec![
                    words(if long {
                        "strata.card.deposit.unread.long"
                    } else {
                        "strata.card.deposit.unread"
                    })
                    .to_owned(),
                ];
            }
        }
        Some(card)
    }

    /// The card in a frame of its own, for a pane.
    #[must_use]
    pub fn framed(mut self) -> Self {
        self.framed = true;
        self
    }

    /// Every word the card shows, in the order it shows them, for a test
    /// to read what a player would.
    #[must_use]
    pub fn text(&self) -> String {
        let mut all = vec![self.name.clone(), self.kind.clone(), self.from.clone()];
        if let Some(condition) = self.condition {
            all.push(condition_words(condition));
        }
        for effect in &self.effects {
            let mut line = format!("{} {}", effect.name, effect.value);
            if !effect.change.is_empty() {
                line.push(' ');
                line.push_str(&effect.change);
            }
            if effect.later {
                line.push_str(" (");
                line.push_str(&words("strata.figure.later").to_lowercase());
                line.push(')');
            }
            all.push(line);
        }
        for line in &self.lines {
            all.push(format!("{} {}", line.name, line.value));
            if !line.note.is_empty() {
                all.push(line.note.clone());
            }
        }
        for range in &self.ranges {
            all.push(if range.word.is_empty() {
                format!("{} {} {}", range.name, range.value, range.of_words())
            } else {
                format!("{} {}", range.name, range.word)
            });
        }
        all.extend(self.notes.iter().cloned());
        all.join("\n")
    }

    /// The parameters as the untyped value a `custom` node carries, which
    /// [`Self::from_value`] reads back.
    #[must_use]
    pub fn to_value(&self) -> Value {
        let text = |s: &str| Model::Str(s.to_owned());
        let mut map = BTreeMap::from([
            ("name".to_owned(), text(&self.name)),
            ("kind".to_owned(), text(&self.kind)),
            ("from".to_owned(), text(&self.from)),
            ("framed".to_owned(), Model::Bool(self.framed)),
            (
                "notes".to_owned(),
                Model::List(self.notes.iter().map(|note| text(note)).collect()),
            ),
            (
                "effects".to_owned(),
                Model::List(
                    self.effects
                        .iter()
                        .map(|effect| {
                            let mut map = BTreeMap::from([
                                ("name".to_owned(), text(&effect.name)),
                                ("value".to_owned(), text(&effect.value)),
                                ("change".to_owned(), text(&effect.change)),
                                ("later".to_owned(), Model::Bool(effect.later)),
                            ]);
                            if let Some(better) = effect.better {
                                map.insert("better".to_owned(), Model::Bool(better));
                            }
                            Model::Map(map)
                        })
                        .collect(),
                ),
            ),
            (
                "lines".to_owned(),
                Model::List(
                    self.lines
                        .iter()
                        .map(|line| {
                            Model::Map(BTreeMap::from([
                                ("name".to_owned(), text(&line.name)),
                                ("value".to_owned(), Model::Int(i64::from(line.value))),
                                ("note".to_owned(), text(&line.note)),
                            ]))
                        })
                        .collect(),
                ),
            ),
            (
                "ranges".to_owned(),
                Model::List(
                    self.ranges
                        .iter()
                        .map(|range| {
                            let number = |v: u16| Model::Int(i64::from(v));
                            Model::Map(BTreeMap::from([
                                ("name".to_owned(), text(&range.name)),
                                ("value".to_owned(), number(range.value)),
                                ("floor".to_owned(), number(range.floor)),
                                ("cap".to_owned(), number(range.cap)),
                                ("word".to_owned(), text(&range.word)),
                            ]))
                        })
                        .collect(),
                ),
            ),
        ]);
        if let Some(picture) = &self.picture {
            map.insert("picture".to_owned(), text(picture));
        }
        if let Some(icon) = &self.icon {
            map.insert("icon".to_owned(), text(icon));
        }
        if let Some(condition) = self.condition {
            map.insert("condition".to_owned(), Model::Int(i64::from(condition)));
        }
        slotted::registry::from_model(&Model::Map(map))
    }

    /// The parameters a `custom` node carries.
    ///
    /// # Errors
    ///
    /// A value that is not these parameters, in words.
    pub fn from_value(value: &Value) -> Result<Self, String> {
        let model = slotted::registry::to_model(value).map_err(|error| error.to_string())?;
        slotted::model::from_value(model).map_err(|error| error.to_string())
    }

    /// The `custom` node that draws these parameters, under `id` when the
    /// screen wants to find it again.
    #[must_use]
    pub fn node(&self, id: Option<&str>) -> UiNodeDef {
        UiNodeDef::Custom {
            kind: WidgetKind::new(WIDGET),
            params: self.to_value(),
            children: Vec::new(),
            tags: id.map_or_else(Tags::new, |id| Tags::new().with(Tags::TEST_ID, id)),
        }
    }
}

/// A thing as a sentence names one of it: `a patrol extended magazine`,
/// `an extended cell`. The item's name without its maker, in lower case,
/// after the article its definition gives or, when it gives none, the
/// language's usual one.
pub(crate) fn a_name(items: &ItemCatalogue, item: &strata_registry::Namespaced) -> Option<String> {
    let article = items.get(item)?.article.as_deref();
    Some(one_of(article, &plain_name(items, item)?))
}

/// `name` after `article`, or after the usual one when there is none.
fn one_of(article: Option<&str>, name: &str) -> String {
    fill(
        &fill(
            words("strata.article.phrase"),
            "article",
            article.unwrap_or_else(|| words("strata.article")),
        ),
        "name",
        name,
    )
}

/// Where the line `line` of the component `component` goes next: `Goes
/// into: Capacity of a patrol extended magazine`, by the first schematic
/// that weighs it, or that nothing reads it yet.
fn goes_into(
    registries: &GameRegistries,
    component: &strata_registry::Namespaced,
    line: &str,
) -> String {
    registries
        .schematics
        .first_using(component, line)
        .and_then(|(next, index)| {
            let product = registries.items.get(&next.def.makes)?;
            Some(fill(
                &fill(
                    words("strata.card.goes_into"),
                    "line",
                    &product.lines.get(index)?.name,
                ),
                "product",
                &a_name(&registries.items, &product.id)?,
            ))
        })
        .unwrap_or_else(|| words("strata.card.goes_nowhere").to_owned())
}

/// A share in per-mille as the whole percent a player reads: 850 is 85.
#[must_use]
pub const fn percent_of(per_mille: u16) -> u16 {
    per_mille / 10
}

/// What a figure a blueprint's slot asks of is called: an attribute's own
/// name for a slot that takes a lot, and for one that takes a part the
/// name the part gives its line.
pub(crate) fn need_name(registries: &GameRegistries, slot: &SlotDef, stat: &str) -> String {
    match &slot.takes {
        Takes::Component(item) | Takes::Commodity(item) => registries
            .items
            .get(item)
            .and_then(|def| def.lines.iter().find(|line| line.id == stat))
            .map(|line| line.name.clone()),
        _ => Attribute::ALL
            .into_iter()
            .find(|attribute| attribute.key() == stat)
            .map(|attribute| attribute.name().to_owned()),
    }
    .unwrap_or_else(|| stat.to_owned())
}

/// What a blueprint's slot asks of its stock, a phrase a requirement:
/// `Potency at least 537`, `Density at most 290`. None for a slot that
/// asks nothing.
pub(crate) fn needs_words(
    registries: &GameRegistries,
    slot: &SlotDef,
    needs: &[Need],
) -> Vec<String> {
    let mut said = Vec::new();
    for need in needs {
        let name = need_name(registries, slot, &need.stat);
        let phrase = |key: &'static str, figure: u16| {
            fill(&fill(words(key), "stat", &name), "n", &figure.to_string())
        };
        said.extend(
            need.at_least
                .map(|least| phrase("strata.crafting.record.least", least)),
        );
        said.extend(
            need.at_most
                .map(|most| phrase("strata.crafting.record.most", most)),
        );
    }
    said
}

/// The blueprint a batch was run off, when the household's ledger says it
/// was: a fabricator's copy, which is told from a thing made by hand.
fn run_off(record: &Record, against: Option<&Against>) -> Option<u32> {
    let made = against?.made.get(&record.batch)?;
    (made.item == record.item && record.by.as_deref() == Some(made.by.as_str()))
        .then_some(made.blueprint)
        .flatten()
}

/// One thing that went into a batch, as its card's last line names it: a
/// lot by its deposit, a made part by its batch, a bought one that has
/// lines by its grade, and a flat one by its name alone.
pub(crate) fn input_name(registries: &GameRegistries, input: &Record) -> String {
    let items = &registries.items;
    if let Some(lot) = CardParams::of_lot(input, registries, false) {
        return lot.name;
    }
    let name = plain_name(items, &input.item).unwrap_or_else(|| input.item.to_string());
    if input.by.is_some() {
        fill(
            &fill(words("strata.card.made_from.batch"), "name", &name),
            "batch",
            &input.batch.to_string(),
        )
    } else if input.lines.is_empty() {
        name
    } else {
        fill(words("strata.card.made_from.store"), "name", &name)
    }
}

/// `Made from cell core batch 1 · harness, store grade · Rake light alloy
/// · seal set.`: what the household's ledger holds for the batch `record`
/// is of. None for anything the household did not make: a batch number
/// alone is not enough, since a batch bought in could share it, so the
/// item and the maker must be the ledger's too.
fn made_from(
    registries: &GameRegistries,
    record: &Record,
    ledger: &BTreeMap<u32, Made>,
) -> Option<String> {
    let made = ledger.get(&record.batch)?;
    if made.item != record.item || record.by.as_deref() != Some(made.by.as_str()) {
        return None;
    }
    let from: Vec<String> = made
        .from
        .iter()
        .map(|(_, input)| input_name(registries, input))
        .collect();
    (!from.is_empty()).then(|| fill(words("strata.card.made_from"), "list", &from.join(" · ")))
}

/// A figure change of a bike's card as the weapons' are worded: signed
/// with a true minus, and `worse` after one that is for the worse.
fn bike_change(by: f32, unit_key: &'static str, higher_is_better: bool) -> (String, Option<bool>) {
    let rounded = by.round();
    if rounded == 0.0 {
        return (String::new(), None);
    }
    let signed = format!("{rounded:+}").replace('-', "\u{2212}");
    let said = fill(words(unit_key), "n", &signed);
    let better = (rounded > 0.0) == higher_is_better;
    if better {
        (said, Some(true))
    } else {
        (
            fill(words("strata.figure.change.worse"), "change", &said),
            Some(false),
        )
    }
}

/// What a bike does, from the same `strata_sim::ride::dials` the mount
/// reads (the file, the record's lines, the wear; a rider's boxes are the
/// rider's and not the bike's), each figure with its change from a bought
/// one: top speed on a road and off it in km/h, the turn in degrees a
/// second at a crawl and at speed, and the saddle bags' cells.
fn bike_effects(
    registries: &GameRegistries,
    record: &Record,
    vehicle: &strata_registry::vehicle::VehicleId,
) -> Vec<CardEffect> {
    let Some(def) = registries.vehicles.get(vehicle) else {
        return Vec::new();
    };
    let rules = registries.items.wear();
    let graph = strata_registry::progression::SkillGraph::default();
    let sheet = strata_sim::progression::CharacterSheet::default();
    let of = |bike: &Record| strata_sim::ride::dials(def, bike, rules, &graph, &sheet);
    let this = of(record);
    let bought = strata_sim::ride::store_grade_bike(&registries.items, vehicle)
        .map_or(this, |bought| of(&bought));
    let kmh = |metres: f32| metres * 3.6;
    let figure = |name: &'static str, value: String, change: (String, Option<bool>)| CardEffect {
        name: words(name).to_owned(),
        value,
        change: change.0,
        better: change.1,
        later: false,
    };
    vec![
        figure(
            "strata.card.bike.road",
            fill(
                words("strata.card.bike.kmh"),
                "n",
                &format!("{:.0}", kmh(this.speed.road)),
            ),
            bike_change(
                kmh(this.speed.road) - kmh(bought.speed.road),
                "strata.card.bike.kmh",
                true,
            ),
        ),
        figure(
            "strata.card.bike.ground",
            fill(
                words("strata.card.bike.kmh"),
                "n",
                &format!("{:.0}", kmh(this.speed.ground)),
            ),
            bike_change(
                kmh(this.speed.ground) - kmh(bought.speed.ground),
                "strata.card.bike.kmh",
                true,
            ),
        ),
        figure(
            "strata.card.bike.turn",
            fill(
                &fill(
                    words("strata.card.bike.turn.value"),
                    "slow",
                    &format!("{:.0}", this.turn.slow),
                ),
                "fast",
                &format!("{:.0}", this.turn.fast),
            ),
            bike_change(
                this.turn.fast - bought.turn.fast,
                "strata.card.bike.turn.change",
                true,
            ),
        ),
        figure(
            "strata.card.bike.cargo",
            fill(
                &fill(
                    &fill(
                        words("strata.card.bike.cells"),
                        "n",
                        &def.cargo.cells().to_string(),
                    ),
                    "columns",
                    &def.cargo.columns.to_string(),
                ),
                "rows",
                &def.cargo.rows.to_string(),
            ),
            (String::new(), None),
        ),
    ]
}

/// `780 of 1000`.
fn condition_words(condition: u16) -> String {
    fill(
        &fill(
            words("strata.card.condition.value"),
            "condition",
            &condition.to_string(),
        ),
        "full",
        &QUALITY_MAX.to_string(),
    )
}

/// How many of a lot's lines the short card shows.
pub const SHORT_LINES: usize = 4;

/// `80 units`, or `1 unit`.
fn units_words(units: u32) -> String {
    if units == 1 {
        words("strata.card.lot.unit").to_owned()
    } else {
        fill(words("strata.card.lot.units"), "units", &units.to_string())
    }
}

/// What a stack is called in a sentence: a lot by its deposit (`Calder
/// salt`), a blueprint by what it makes as its card is (`Cell core
/// blueprint`, read from the household's ledger in `against`; two
/// blueprints in a store are otherwise one name), anything else by its
/// item's name. `?` for a stack the catalogue does not know.
#[must_use]
pub fn stack_name(
    registries: &GameRegistries,
    stack: &slotted::model::ItemStack,
    against: Option<&Against>,
) -> String {
    let items = &registries.items;
    let def = items.def_of(stack.id);
    strata_sim::items::read(stack, items)
        .ok()
        .and_then(|record| match def.map(|def| &def.kind) {
            Some(ItemKind::Blueprint) => {
                Some(CardParams::of_blueprint(&record, registries, false, against).name)
            }
            _ => lot_name(&record, registries),
        })
        .or_else(|| def.map(|def| def.name.clone()))
        .unwrap_or_else(|| "?".to_owned())
}

/// A lot's name, for [`stack_name`]; none for anything else.
fn lot_name(record: &Record, registries: &GameRegistries) -> Option<String> {
    record.origin.as_ref()?;
    CardParams::of_lot(record, registries, false).map(|card| card.name)
}

/// What an item is, at the right of its name: `Weapon`, `Mod · Feed`.
fn kind_words(def: &ItemDefinition) -> String {
    match &def.kind {
        ItemKind::Weapon { .. } => words("strata.card.kind.weapon").to_owned(),
        ItemKind::Module { slot, .. } => format!(
            "{} · {}",
            words("strata.card.kind.module"),
            mount_name(slot)
        ),
        ItemKind::Bag { .. } => words("strata.card.kind.bag").to_owned(),
        ItemKind::Lot { .. } => words("strata.card.kind.lot").to_owned(),
        ItemKind::Component => words("strata.card.kind.component").to_owned(),
        ItemKind::Commodity => words("strata.card.kind.commodity").to_owned(),
        ItemKind::Blueprint => words("strata.card.kind.blueprint").to_owned(),
        ItemKind::Kit => words("strata.card.kind.kit").to_owned(),
        ItemKind::Tool { .. } => words("strata.card.kind.tool").to_owned(),
        ItemKind::Vehicle { .. } => words("strata.card.kind.vehicle").to_owned(),
        ItemKind::Harvester { .. } => words("strata.card.kind.harvester").to_owned(),
    }
}

/// Where a stack came from: who made it and in which batch, and how many
/// of it there are when it is more than one. A maker with no name (the
/// sim's empty one) is worded here, since the sim writes no English. A
/// batch a fabricator ran says the blueprint it was run off instead of a
/// maker: every unit of it is the same, and the count beside it is the
/// whole of what tells the stack.
fn from_words(def: &ItemDefinition, record: &Record, against: Option<&Against>) -> String {
    let mut from = match &record.by {
        Some(_) if run_off(record, against).is_some() => fill(
            &fill(
                words("strata.card.from.run"),
                "blueprint",
                &run_off(record, against).unwrap_or_default().to_string(),
            ),
            "batch",
            &record.batch.to_string(),
        ),
        Some(by) => fill(
            &fill(
                words("strata.card.from.crafted"),
                "by",
                if by.is_empty() {
                    words("strata.card.from.unnamed")
                } else {
                    by
                },
            ),
            "batch",
            &record.batch.to_string(),
        ),
        None => fill(words("strata.card.from.store"), "maker", &def.maker),
    };
    if record.count > 1 {
        from.push_str(" · ");
        from.push_str(&fill(
            words("strata.card.from.stack"),
            "count",
            &record.count.to_string(),
        ));
    }
    from
}

/// A melee weapon's cuts as (name, figure) pairs for a card or a table, from
/// the sim's own figures; none for anything else.
#[must_use]
pub fn cut_facts(
    items: &ItemCatalogue,
    holder: &Combatant,
    record: &Record,
) -> Vec<(String, String)> {
    fitting::cut_lines(items, holder, record)
        .into_iter()
        .map(|(figure, amount)| {
            let name = match figure {
                CutFigure::LightDamage => "strata.cut.light_damage",
                CutFigure::LightTime => "strata.cut.light_time",
                CutFigure::LightRecovery => "strata.cut.light_recovery",
                CutFigure::Draw => "strata.property.equip_time",
                CutFigure::LightStamina => "strata.cut.light_stamina",
                CutFigure::HeavyDamage => "strata.cut.heavy_damage",
                CutFigure::HeavyPosture => "strata.cut.heavy_posture",
                CutFigure::Reach => "strata.weapon.fact.reach",
                CutFigure::GuardShare => "strata.cut.guard_share",
                CutFigure::ParryWindow => "strata.cut.parry_window",
            };
            let unit = match amount.unit {
                Unit::Seconds => "strata.figure.seconds",
                Unit::Metres => "strata.figure.metres",
                Unit::Percent => "strata.figure.percent",
                _ => "strata.figure.points",
            };
            (
                words(name).to_owned(),
                fill(words(unit), "n", &amount.text()),
            )
        })
        .collect()
}

/// A weapon's magazine, reload and interval as it is fitted, each with its
/// change against the same weapon with nothing fitted. Read against the
/// hand that carries a weapon of its kind, whose holder's row it is; a
/// weapon nobody carries has no holder to read it for, and shows none.
fn weapon_effects(
    items: &ItemCatalogue,
    record: &Record,
    against: Option<&Against>,
) -> Vec<CardEffect> {
    // A melee weapon has cuts, not a magazine: its figures are the sim's,
    // read for the holder whether or not it is in a hand.
    if let Some(against) = against {
        let cuts = cut_facts(items, &against.holder, record);
        if !cuts.is_empty() {
            return cuts
                .into_iter()
                .map(|(name, value)| CardEffect {
                    name,
                    value,
                    ..CardEffect::default()
                })
                .collect();
        }
    }
    let Some((_, _, bare)) = against.and_then(|against| {
        against
            .by_reach()
            .find(|(_, carried, _)| carried.item == record.item)
    }) else {
        return Vec::new();
    };
    fitting::lines(
        items,
        bare,
        &fitting::bare_of(record),
        record,
        &[
            Property::Capacity,
            Property::ReloadTime,
            Property::RepeatInterval,
        ],
    )
    .iter()
    .map(|line| CardEffect::of_for(line, bare.spec.mode))
    .chain(
        weapon_facts(&bare.spec)
            .into_iter()
            .map(|(name, value)| CardEffect {
                name,
                value,
                ..CardEffect::default()
            }),
    )
    .collect()
}

/// What a module does, and the sentence that says against what: the
/// figures of the carried weapon that takes it with the module on its
/// mount, each changed against what that mount holds now. A module that
/// is the one fitted is read against the standard part instead, and one
/// whose mount is still shut is shown as it would be were it open. Which
/// weapon that is, and what it would read, are the sim's to say.
fn module_effects(
    items: &ItemCatalogue,
    record: &Record,
    slot: &str,
    against: Option<&Against>,
) -> (Vec<CardEffect>, String) {
    let Some((hand, weapon, bare)) = against.and_then(|against| {
        let carried = against.hands.each_ref().map(|hand| hand.as_ref());
        let records = carried.map(|hand| hand.map(|(record, _)| record));
        let hand = fitting::host(items, &records, against.drawn, &record.item)?;
        let (weapon, bare) = carried[hand]?;
        Some((hand == against.drawn, weapon, bare))
    }) else {
        return (Vec::new(), words("strata.card.against.none").to_owned());
    };
    let module = Fitted {
        item: record.item.clone(),
        batch: record.batch,
        by: record.by.clone(),
        lines: record.lines.clone(),
        condition: record.condition,
    };
    let holder = plain_name(items, &weapon.item).unwrap_or_default();
    let place = words(if hand {
        "strata.card.where.in_hand"
    } else {
        "strata.card.where.holstered"
    });
    let mount = mount_name(slot).to_lowercase();
    let refused = fitting::with(items, weapon, slot, Some(&module)).err();
    let current = weapon.fitted.get(slot);
    let mut now = weapon.clone();
    let mut then = weapon.clone();
    let mut properties = properties_of(items, &module.item);
    let note = if refused == Some(FitRefusal::AlreadyFitted) {
        now = fitting::preview(items, weapon, slot, None);
        fill(words("strata.card.against.this"), "mount", &mount)
    } else {
        // A shut mount refuses the part; the card still says what it
        // would do, and that the mount is shut.
        then = fitting::preview(items, weapon, slot, Some(&module));
        if matches!(refused, Some(FitRefusal::Shut { .. })) {
            fill(
                &fill(
                    &fill(words("strata.card.against.shut"), "weapon", &holder),
                    "where",
                    place,
                ),
                "mount",
                &mount,
            )
        } else if let Some(other) = current {
            for property in properties_of(items, &other.item) {
                if !properties.contains(&property) {
                    properties.push(property);
                }
            }
            fill(
                words("strata.card.against.fitted"),
                "name",
                &items
                    .get(&other.item)
                    .map_or_else(String::new, |def| def.name.to_lowercase()),
            )
        } else {
            fill(
                &fill(
                    &fill(words("strata.card.against.standard"), "weapon", &holder),
                    "where",
                    place,
                ),
                "mount",
                &mount,
            )
        }
    };
    let effects = fitting::lines(items, bare, &now, &then, &properties)
        .iter()
        .map(|line| CardEffect::of_for(line, bare.spec.mode))
        .collect();
    (effects, note)
}

/// The properties a module changes, in its own order.
pub(crate) fn properties_of(
    items: &ItemCatalogue,
    module: &strata_registry::Namespaced,
) -> Vec<Property> {
    match items.get(module).map(|def| &def.kind) {
        Some(ItemKind::Module { effects, .. }) => {
            effects.iter().map(|effect| effect.property).collect()
        }
        _ => Vec::new(),
    }
}

/// On a card's root: what it is showing, so a screen repaints it only when
/// the words change.
#[derive(Component, Debug, Clone, PartialEq, Eq)]
pub struct ItemCard(pub CardParams);

/// On a quality bar's root: the figure its fill and its numeral show.
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq)]
pub struct QualityBar(pub u16);

/// On a tick of a quality bar: the figure it stands at.
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq)]
pub struct BarTick(pub u16);

/// On the band of a lot's bar: the floor and the cap it runs between.
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq)]
pub struct BarRange(pub u16, pub u16);

/// On a bar's fill: the figure it is drawn to. A screen that moves a fill
/// in place (a roll landing) finds the node by this and writes its width.
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq)]
pub struct BarFill(pub u16);

/// On the part of a craft's track that its materials allow: the ceiling it
/// runs to.
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq)]
pub struct BarCeiling(pub u16);

/// On the mark of a craft's bar where assembly alone lands the line.
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq)]
pub struct BarFloor(pub u16);

/// A figure of nought to a thousand as a share of a track's width, in
/// percent.
#[must_use]
pub fn track_percent(figure: u16) -> f32 {
    100. * f32::from(figure.min(QUALITY_MAX)) / f32::from(QUALITY_MAX)
}

/// What one bar shows: a figure of nought to a thousand on its track, with
/// the ticks that mean something on that track. A quality line has one at
/// store grade; a weapon's moddability has one at each threshold a mount
/// opens at; a condition has none.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct BarParams {
    /// The figure.
    pub value: u16,
    /// Where the ticks stand.
    #[serde(default = "store_grade_tick")]
    pub ticks: Vec<u16>,
    /// Whether the figure is written at the bar's right. A screen that
    /// writes it elsewhere, in words of its own, leaves it out.
    #[serde(default = "yes")]
    pub numeral: bool,
    /// Whether the bar is a weapon's condition, which fills in its own
    /// colour.
    #[serde(default)]
    pub condition: bool,
    /// Whether that condition has slid, which the fill says in the warm
    /// colour beside the words.
    #[serde(default)]
    pub worn: bool,
    /// A lot's line: the floor and cap its type has, drawn as a band in
    /// the track with the fill from the floor to the figure and a mark at
    /// the figure. Such a bar has no store-grade tick: a lot is measured
    /// against its own type, not against a bought part.
    #[serde(default)]
    pub range: Option<(u16, u16)>,
    /// A craft's line: what its materials allow, drawn behind the fill as
    /// the part of the track the line can be worked along.
    #[serde(default)]
    pub ceiling: Option<u16>,
    /// A craft's line before it is assembled: where assembly alone lands
    /// it, marked on the track.
    #[serde(default)]
    pub floor: Option<u16>,
    /// The figure the numeral writes when it is not the fill's: a craft's
    /// ceiling, beside a fill that stands at its floor.
    #[serde(default)]
    pub reads: Option<u16>,
}

fn store_grade_tick() -> Vec<u16> {
    vec![STORE_GRADE]
}

const fn yes() -> bool {
    true
}

impl BarParams {
    /// A quality line: the figure with the tick at store grade.
    #[must_use]
    pub fn quality(value: u16) -> Self {
        Self {
            value,
            ticks: store_grade_tick(),
            numeral: true,
            condition: false,
            worn: false,
            range: None,
            ceiling: None,
            floor: None,
            reads: None,
        }
    }

    /// A lot's line: the figure in the range its type has, with no tick
    /// and no numeral, which the card writes beside the line's name.
    #[must_use]
    pub const fn ranged(value: u16, floor: u16, cap: u16) -> Self {
        Self {
            value,
            ticks: Vec::new(),
            numeral: false,
            condition: false,
            worn: false,
            range: Some((floor, cap)),
            ceiling: None,
            floor: None,
            reads: None,
        }
    }

    /// A craft's line: the fill at `value` on a track lit up to `ceiling`,
    /// with no store-grade tick, since the line is measured against what
    /// its own materials allow. The numeral is the figure.
    #[must_use]
    pub const fn worked(value: u16, ceiling: u16) -> Self {
        Self {
            value,
            ticks: Vec::new(),
            numeral: true,
            condition: false,
            worn: false,
            range: None,
            ceiling: Some(ceiling),
            floor: None,
            reads: None,
        }
    }

    /// A craft's line before anything is spent: the fill and a mark at the
    /// `floor` assembly gives, the track lit up to `ceiling`, and the
    /// ceiling as the numeral.
    #[must_use]
    pub const fn offered(floor: u16, ceiling: u16) -> Self {
        Self {
            value: floor,
            ticks: Vec::new(),
            numeral: true,
            condition: false,
            worn: false,
            range: None,
            ceiling: Some(ceiling),
            floor: Some(floor),
            reads: Some(ceiling),
        }
    }

    /// The `custom` node that draws the bar, under `id` when the screen
    /// wants to find it again.
    #[must_use]
    pub fn node(&self, id: Option<&str>) -> UiNodeDef {
        let number = |v: u16| Model::Int(i64::from(v));
        let mut params = BTreeMap::from([
            ("value".to_owned(), number(self.value)),
            (
                "ticks".to_owned(),
                Model::List(self.ticks.iter().copied().map(number).collect()),
            ),
            ("numeral".to_owned(), Model::Bool(self.numeral)),
            ("condition".to_owned(), Model::Bool(self.condition)),
            ("worn".to_owned(), Model::Bool(self.worn)),
        ]);
        if let Some((floor, cap)) = self.range {
            params.insert(
                "range".to_owned(),
                Model::List(vec![number(floor), number(cap)]),
            );
        }
        for (key, figure) in [
            ("ceiling", self.ceiling),
            ("floor", self.floor),
            ("reads", self.reads),
        ] {
            if let Some(figure) = figure {
                params.insert(key.to_owned(), number(figure));
            }
        }
        let params = Model::Map(params);
        UiNodeDef::Custom {
            kind: WidgetKind::new(BAR_WIDGET),
            params: slotted::registry::from_model(&params),
            children: Vec::new(),
            tags: id.map_or_else(Tags::new, |id| Tags::new().with(Tags::TEST_ID, id)),
        }
    }

    /// The parameters a `custom` node carries: these, or the bare figure
    /// of a quality line.
    fn from_value(value: &Value) -> Option<Self> {
        let model = slotted::registry::to_model(value).ok()?;
        slotted::model::from_value::<u16>(model.clone())
            .map(Self::quality)
            .or_else(|_| slotted::model::from_value(model))
            .ok()
    }
}

/// A text node under `parent`, in `role`.
fn text(ctx: &mut SpawnCtx<'_>, parent: Entity, words: &str, role: &'static str, wrap: bool) {
    ctx.spawn_children(
        parent,
        &[UiNodeDef::Text {
            key: LocKey(words.to_owned()),
            style: TextRole::Caption,
            opts: TextOpts {
                wrap,
                role: Some(Role::new_static(role)),
                ..TextOpts::default()
            },
            tags: Tags::new(),
        }],
    );
}

/// A row under `parent` whose first child sits left and whose last right.
fn row(ctx: &mut SpawnCtx<'_>, parent: Entity) -> Entity {
    ctx.world
        .spawn((
            Node {
                width: Val::Percent(100.),
                column_gap: Val::Px(10.),
                align_items: AlignItems::Baseline,
                justify_content: JustifyContent::SpaceBetween,
                ..Node::default()
            },
            ChildOf(parent),
        ))
        .id()
}

/// A hairline across the card.
fn rule(ctx: &mut SpawnCtx<'_>, parent: Entity) {
    ctx.world.spawn((
        Node {
            width: Val::Percent(100.),
            height: Val::Px(1.),
            margin: UiRect::vertical(Val::Px(4.)),
            flex_shrink: 0.,
            ..Node::default()
        },
        Themed(Role::new_static(RULE_ROLE)),
        ChildOf(parent),
    ));
}

/// The band of a lot's line on its `track`: the range its type has
/// anywhere, `floor` to `cap`, with a hairline at either end so the range
/// has edges where the band is faint.
fn spawn_range(ctx: &mut SpawnCtx<'_>, track: Entity, floor: u16, cap: u16) {
    let percent = |q: u16| 100. * f32::from(q) / f32::from(QUALITY_MAX);
    let from = floor.min(QUALITY_MAX);
    let to = cap.min(QUALITY_MAX).max(from);
    ctx.world.spawn((
        Node {
            position_type: PositionType::Absolute,
            left: Val::Percent(percent(from)),
            top: Val::Px(0.),
            bottom: Val::Px(0.),
            width: Val::Percent(percent(to - from)),
            ..Node::default()
        },
        Themed(Role::new_static(RANGE_ROLE)),
        BarRange(floor, to),
        ChildOf(track),
    ));
    for end in [from, to] {
        ctx.world.spawn((
            Node {
                position_type: PositionType::Absolute,
                left: Val::Percent(percent(end)),
                top: Val::Px(-2.),
                bottom: Val::Px(-2.),
                width: Val::Px(1.),
                ..Node::default()
            },
            Themed(Role::new_static(RANGE_END_ROLE)),
            ChildOf(track),
        ));
    }
}

/// The mark at a lot's own figure on its `track`, over the fill and proud
/// of the track, so it reads wherever in the band it stands.
fn spawn_mark(ctx: &mut SpawnCtx<'_>, track: Entity, value: u16) {
    ctx.world.spawn((
        Node {
            position_type: PositionType::Absolute,
            left: Val::Percent(100. * f32::from(value) / f32::from(QUALITY_MAX)),
            top: Val::Px(-3.),
            bottom: Val::Px(-3.),
            width: Val::Px(3.),
            margin: UiRect::left(Val::Px(-1.)),
            ..Node::default()
        },
        Themed(Role::new_static(MARK_ROLE)),
        ChildOf(track),
    ));
}

/// What a craft's materials allow, on its `track` under the fill: the part
/// of the track the line can be worked along.
fn spawn_ceiling(ctx: &mut SpawnCtx<'_>, track: Entity, ceiling: u16) {
    ctx.world.spawn((
        Node {
            position_type: PositionType::Absolute,
            left: Val::Px(0.),
            top: Val::Px(0.),
            bottom: Val::Px(0.),
            width: Val::Percent(track_percent(ceiling)),
            ..Node::default()
        },
        Themed(Role::new_static(CEILING_ROLE)),
        BarCeiling(ceiling),
        ChildOf(track),
    ));
}

/// Where assembly alone lands a craft's line, on its `track`: proud of the
/// track, so it reads at the fill's own end.
fn spawn_floor(ctx: &mut SpawnCtx<'_>, track: Entity, floor: u16) {
    ctx.world.spawn((
        Node {
            position_type: PositionType::Absolute,
            left: Val::Percent(track_percent(floor)),
            top: Val::Px(-2.),
            bottom: Val::Px(-2.),
            width: Val::Px(2.),
            ..Node::default()
        },
        Themed(Role::new_static(FLOOR_ROLE)),
        BarFloor(floor),
        ChildOf(track),
    ));
}

/// One figure on its track: the fill to a tenth of it in percent (a lot's
/// from its type's floor, inside the band of its range), its ticks, the
/// numeral at the right.
fn spawn_bar(ctx: &mut SpawnCtx<'_>, parent: Entity, bar: &BarParams) -> Entity {
    let value = bar.value.min(QUALITY_MAX);
    let percent = |q: u16| 100. * f32::from(q) / f32::from(QUALITY_MAX);
    let root = ctx
        .world
        .spawn((
            Node {
                width: Val::Percent(100.),
                column_gap: Val::Px(8.),
                align_items: AlignItems::Center,
                ..Node::default()
            },
            SemanticRole::Custom(BAR_WIDGET.to_owned()),
            SemanticLabel(value.to_string()),
            WidgetNode(WidgetKind::new(BAR_WIDGET)),
            QualityBar(value),
            ChildOf(parent),
        ))
        .id();
    let track = ctx
        .world
        .spawn((
            Node {
                flex_grow: 1.,
                height: Val::Px(5.),
                ..Node::default()
            },
            Themed(Role::new_static(if bar.range.is_some() {
                LOT_TRACK_ROLE
            } else {
                TRACK_ROLE
            })),
            ChildOf(root),
        ))
        .id();
    // A plain bar fills from nought. A lot's fills from its type's floor,
    // inside the band that is the type's range.
    let from = bar.range.map_or(0, |(floor, _)| floor.min(QUALITY_MAX));
    if let Some((floor, cap)) = bar.range {
        spawn_range(ctx, track, floor, cap);
    }
    if let Some(ceiling) = bar.ceiling {
        spawn_ceiling(ctx, track, ceiling);
    }
    ctx.world.spawn((
        Node {
            position_type: PositionType::Absolute,
            left: Val::Percent(percent(from)),
            top: Val::Px(0.),
            bottom: Val::Px(0.),
            width: Val::Percent(percent(value.saturating_sub(from))),
            ..Node::default()
        },
        Themed(Role::new_static(if bar.condition && bar.worn {
            CONDITION_WORN_FILL_ROLE
        } else if bar.condition {
            CONDITION_FILL_ROLE
        } else {
            FILL_ROLE
        })),
        BarFill(value),
        ChildOf(track),
    ));
    if bar.range.is_some() {
        spawn_mark(ctx, track, value);
    }
    if let Some(floor) = bar.floor {
        spawn_floor(ctx, track, floor);
    }
    for tick in &bar.ticks {
        ctx.world.spawn((
            Node {
                position_type: PositionType::Absolute,
                left: Val::Percent(percent((*tick).min(QUALITY_MAX))),
                top: Val::Px(-3.),
                bottom: Val::Px(-3.),
                width: Val::Px(2.),
                ..Node::default()
            },
            Themed(Role::new_static(TICK_ROLE)),
            BarTick(*tick),
            ChildOf(track),
        ));
    }
    if !bar.numeral {
        return root;
    }
    let figure = ctx
        .world
        .spawn((
            Node {
                width: Val::Px(VALUE_WIDTH),
                justify_content: JustifyContent::FlexEnd,
                flex_shrink: 0.,
                ..Node::default()
            },
            ChildOf(root),
        ))
        .id();
    let reads = bar.reads.unwrap_or(value);
    text(ctx, figure, &reads.to_string(), VALUE_ROLE, false);
    root
}

/// The picture of a weapon, fitted into a well of a fixed height so a
/// pistol, which is as tall as it is long, takes no more of the card than
/// a carbine. Nothing is drawn, and no room is kept, without the baked
/// index, which is what gives the picture's shape before its file loads.
fn spawn_picture(ctx: &mut SpawnCtx<'_>, parent: Entity, picture: &str) -> bool {
    let file = format!("{}/{picture}", pictures_dir());
    let Some(aspect) = ctx
        .world
        .get_resource::<WeaponPictures>()
        .and_then(|pictures| {
            pictures
                .0
                .chassis
                .values()
                .find(|chassis| chassis.whole.file == file)
        })
        .map(|chassis| chassis.whole.rect)
        .filter(|rect| rect[2] > 0 && rect[3] > 0)
        .map(|rect| {
            let side = |v: u32| f32::from(u16::try_from(v).unwrap_or(u16::MAX));
            side(rect[2]) / side(rect[3])
        })
    else {
        return false;
    };
    let Some(handle) = ctx
        .world
        .get_resource::<AssetServer>()
        .filter(|assets| assets.get_source(GENERATED_SOURCE).is_ok())
        .map(|assets| assets.load(generated(&file)))
    else {
        return false;
    };
    let tint = ctx
        .world
        .get_resource::<ActiveTheme>()
        .zip(ctx.world.get_resource::<Assets<Theme>>())
        .and_then(|(active, themes)| themes.get(&active.0))
        .and_then(|theme| Paint::for_role(theme, &Role::new_static(PICTURE_ROLE)))
        .and_then(|paint| paint.background)
        .unwrap_or(bevy::color::Color::WHITE);
    let well = ctx
        .world
        .spawn((
            Node {
                width: Val::Percent(100.),
                height: Val::Px(PICTURE_HEIGHT),
                align_items: AlignItems::Center,
                justify_content: JustifyContent::Center,
                flex_shrink: 0.,
                ..Node::default()
            },
            ChildOf(parent),
        ))
        .id();
    ctx.world.spawn((
        Node {
            width: Val::Percent(100.),
            max_height: Val::Percent(100.),
            aspect_ratio: Some(aspect),
            flex_shrink: 0.,
            ..Node::default()
        },
        ImageNode {
            image_mode: NodeImageMode::Stretch,
            color: tint,
            ..ImageNode::new(handle)
        },
        ChildOf(well),
    ));
    true
}

/// The image of the item `item`, as the grid draws it, at the head of a
/// card with no weapon picture. Nothing is drawn where the rule has no
/// image for it, so the stand-in shape never reaches a card.
fn spawn_icon(ctx: &mut SpawnCtx<'_>, parent: Entity, item: &str) {
    let Some(art) = ctx
        .world
        .get_resource::<ItemImages>()
        .and_then(|images| images.0.get(item))
        .cloned()
    else {
        return;
    };
    let Some(handle) = ctx
        .world
        .get_resource::<AssetServer>()
        .map(|assets| assets.load::<bevy::image::Image>(art.path))
    else {
        return;
    };
    ctx.world.spawn((
        Node {
            width: Val::Px(CARD_ICON),
            height: Val::Px(CARD_ICON),
            flex_shrink: 0.,
            ..Node::default()
        },
        ImageNode {
            color: art.tint.unwrap_or(bevy::color::Color::WHITE),
            ..ImageNode::new(handle)
        },
        Pickable::IGNORE,
        ChildOf(parent),
    ));
}

/// One effect: the property at the left, tagged when the fight has no
/// number for it, and at the right its figure and its change.
fn paint_effect(ctx: &mut SpawnCtx<'_>, parent: Entity, effect: &CardEffect) {
    let line = row(ctx, parent);
    let name = ctx
        .world
        .spawn((
            Node {
                flex_direction: FlexDirection::Column,
                flex_shrink: 1.,
                min_width: Val::Px(0.),
                ..Node::default()
            },
            ChildOf(line),
        ))
        .id();
    text(ctx, name, &effect.name, EFFECT_NAME_ROLE, false);
    if effect.later {
        text(
            ctx,
            name,
            words("strata.figure.later"),
            EFFECT_LATER_ROLE,
            false,
        );
    }
    let figure = ctx
        .world
        .spawn((
            Node {
                column_gap: Val::Px(8.),
                align_items: AlignItems::Baseline,
                flex_shrink: 0.,
                ..Node::default()
            },
            ChildOf(line),
        ))
        .id();
    text(ctx, figure, &effect.value, EFFECT_VALUE_ROLE, false);
    if let Some(better) = effect.better {
        text(
            ctx,
            figure,
            &effect.change,
            if better {
                EFFECT_BETTER_ROLE
            } else {
                EFFECT_WORSE_ROLE
            },
            false,
        );
    }
}

/// One line of a lot: the attribute at the left, at the right the lot's
/// figure and the range it stands in, and under them the track with that
/// range drawn on it. The bar has the card's whole width, which is what
/// lets two lots of a type be told apart by eye.
fn paint_range(ctx: &mut SpawnCtx<'_>, parent: Entity, range: &CardRange) {
    let line = row(ctx, parent);
    text(ctx, line, &range.name, LINE_ROLE, false);
    let figure = ctx
        .world
        .spawn((
            Node {
                column_gap: Val::Px(6.),
                align_items: AlignItems::Baseline,
                flex_shrink: 0.,
                ..Node::default()
            },
            ChildOf(line),
        ))
        .id();
    if range.word.is_empty() {
        text(ctx, figure, &range.value.to_string(), VALUE_ROLE, false);
        text(ctx, figure, &range.of_words(), OF_ROLE, false);
    } else {
        text(ctx, figure, &range.word, VALUE_ROLE, false);
    }
    let bar = spawn_bar(
        ctx,
        parent,
        &BarParams::ranged(range.value, range.floor, range.cap),
    );
    // The mark stands proud of the track, so the next line keeps clear.
    if let Some(mut node) = ctx.world.get_mut::<Node>(bar) {
        node.margin = UiRect::bottom(Val::Px(4.));
    }
}

/// Everything under a card's root, top to bottom.
fn paint(ctx: &mut SpawnCtx<'_>, root: Entity, params: &CardParams) {
    if !params.name.is_empty() {
        if params.ranges.is_empty() {
            let head = row(ctx, root);
            text(ctx, head, &params.name, NAME_ROLE, true);
            text(ctx, head, &params.kind.to_uppercase(), KIND_ROLE, false);
        } else {
            // A lot's kind is three words long, its family, class and
            // type: beside the name it would push the name onto two
            // lines, so it stands under it.
            text(ctx, root, &params.name, NAME_ROLE, true);
            text(ctx, root, &params.kind.to_uppercase(), KIND_ROLE, false);
        }
        text(ctx, root, &params.from, FROM_ROLE, true);
        rule(ctx, root);
    }
    // A weapon's picture is drawn where it is baked; where it is not the
    // card falls back to the item's image like any other.
    let pictured = params
        .picture
        .as_ref()
        .is_some_and(|picture| spawn_picture(ctx, root, picture));
    if !pictured && let Some(item) = &params.icon {
        spawn_icon(ctx, root, item);
    }
    for effect in &params.effects {
        paint_effect(ctx, root, effect);
    }
    if !params.effects.is_empty() && (params.condition.is_some() || !params.lines.is_empty()) {
        rule(ctx, root);
    }
    if let Some(condition) = params.condition {
        let line = row(ctx, root);
        text(ctx, line, words("strata.card.condition"), KIND_ROLE, false);
        text(ctx, line, &condition_words(condition), VALUE_ROLE, false);
        // Condition is the weapon's own and no heading for the lines under
        // it, so a rule parts them.
        if !params.lines.is_empty() {
            rule(ctx, root);
        }
    }
    for line in &params.lines {
        text(ctx, root, &line.name, LINE_ROLE, false);
        spawn_bar(ctx, root, &BarParams::quality(line.value));
        if !line.note.is_empty() {
            text(ctx, root, &line.note, NOTE_ROLE, true);
            // The note is the line's own: the next line's name keeps clear
            // of it.
            ctx.world.spawn((
                Node {
                    height: Val::Px(4.),
                    flex_shrink: 0.,
                    ..Node::default()
                },
                ChildOf(root),
            ));
        }
    }
    for range in &params.ranges {
        paint_range(ctx, root, range);
    }
    if !params.lines.is_empty() || !params.ranges.is_empty() || params.condition.is_some() {
        rule(ctx, root);
    }
    for note in &params.notes {
        text(ctx, root, note, NOTE_ROLE, true);
    }
    // A card is read and never clicked: nothing in it may take the pointer
    // from the slot it is raised beside.
    let mut below: Vec<Entity> = ctx
        .world
        .get::<Children>(root)
        .map(|children| children.to_vec())
        .unwrap_or_default();
    while let Some(child) = below.pop() {
        if let Some(grand) = ctx.world.get::<Children>(child) {
            below.extend(grand.iter());
        }
        ctx.world.entity_mut(child).insert(Pickable::IGNORE);
    }
}

/// `strata:item_card`.
#[derive(Debug, Default, Clone, Copy)]
pub struct CardWidget;

impl Widget for CardWidget {
    fn spawn(&self, ctx: &mut SpawnCtx<'_>, params: &Value, _children: &[UiNodeDef]) -> Entity {
        let params = CardParams::from_value(params).unwrap_or_else(|error| {
            tracing::warn!(%error, "bad params for {WIDGET}; drawing the empty card");
            CardParams::empty()
        });
        let node = Node {
            flex_direction: FlexDirection::Column,
            row_gap: Val::Px(3.),
            flex_shrink: 0.,
            ..Node::default()
        };
        let root = ctx.spawn_node((
            if params.framed {
                Node {
                    width: Val::Percent(100.),
                    padding: UiRect::axes(Val::Px(14.), Val::Px(12.)),
                    border: UiRect::all(Val::Px(1.)),
                    ..node
                }
            } else {
                Node {
                    width: Val::Px(RAISED_WIDTH),
                    padding: UiRect::axes(Val::Px(10.), Val::Px(8.)),
                    ..node
                }
            },
            SemanticRole::Custom(WIDGET.to_owned()),
            SemanticLabel(params.name.clone()),
            WidgetNode(WidgetKind::new(WIDGET)),
            Pickable::IGNORE,
            ItemCard(params.clone()),
        ));
        ctx.world
            .entity_mut(root)
            .insert(Themed(Role::new_static(if params.framed {
                FRAME_ROLE
            } else {
                RAISED_ROLE
            })));
        paint(ctx, root, &params);
        root
    }
}

/// `strata:quality_bar`: one bar on its own, for a screen that draws a
/// figure outside a card. Its value is the bare number of a quality line,
/// or [`BarParams`] for a bar with ticks of its own.
#[derive(Debug, Default, Clone, Copy)]
pub struct BarWidget;

impl Widget for BarWidget {
    fn spawn(&self, ctx: &mut SpawnCtx<'_>, params: &Value, _children: &[UiNodeDef]) -> Entity {
        let bar = BarParams::from_value(params).unwrap_or_else(|| {
            tracing::warn!("bad params for {BAR_WIDGET}; drawing an empty bar");
            BarParams::quality(0)
        });
        let parent = ctx.parent;
        spawn_bar(ctx, parent, &bar)
    }
}

/// Draws `params` on the card at `root` in place of what it shows, when it
/// shows something else. The card keeps its entity, its frame and its place
/// in the screen; only what is inside it is replaced.
pub fn repaint(world: &mut World, root: Entity, params: &CardParams) {
    let Some(card) = world.get::<ItemCard>(root) else {
        return;
    };
    if card.0 == *params {
        return;
    }
    let children: Vec<Entity> = world
        .get::<Children>(root)
        .map(|children| children.to_vec())
        .unwrap_or_default();
    for child in children {
        world.despawn(child);
    }
    let mut ctx = SpawnCtx {
        world,
        screen: root,
        kind: ScreenKind::new(WIDGET),
        menu: None,
        parent: root,
    };
    paint(&mut ctx, root, params);
    if let Some(mut card) = world.get_mut::<ItemCard>(root) {
        card.0 = params.clone();
    }
    if let Some(mut label) = world.get_mut::<SemanticLabel>(root) {
        label.0.clone_from(&params.name);
    }
}

/// What a card raised by slotted cannot be handed when it is built: a
/// tooltip part has no world to read. A system keeps it
/// ([`follow_context`]) and a screen sets its quiet, and the part reads
/// both through the lock.
#[derive(Debug, Default)]
struct Shared {
    quiet: AtomicBool,
    against: Mutex<Option<Against>>,
}

/// The card's view of the world: the weapons its figures are read against,
/// and whether the raised card stands down.
#[derive(Resource, Debug, Clone, Default)]
pub struct CardContext(Arc<Shared>);

impl CardContext {
    /// Whether the raised card stands down.
    #[must_use]
    pub fn quiet(&self) -> bool {
        self.0.quiet.load(Ordering::Relaxed)
    }

    /// Stands the raised card down, or up again. A screen that pins the
    /// card in a pane holds this while it is on top: the raised card would
    /// repeat the pane over the cells beside the one pointed at.
    pub fn set_quiet(&self, quiet: bool) {
        self.0.quiet.store(quiet, Ordering::Relaxed);
    }

    /// The weapons the local player carries, as the last frame read them.
    #[must_use]
    pub fn against(&self) -> Option<Against> {
        self.0
            .against
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }

    fn set_against(&self, against: Option<Against>) {
        let mut held = self
            .0
            .against
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        if *held != against {
            *held = against;
        }
    }
}

/// `Update`: the card's context follows the local player's weapons, so a
/// card painted this frame, pinned or raised, measures against what is in
/// their hands now.
///
/// The ledger of what the household has made is kept from the sim's
/// crafting state, copied when that changes and not every frame: it grows
/// with every batch finished. The blueprints are kept the same way, which
/// change once a unit while a run is on.
#[allow(
    clippy::too_many_arguments,
    reason = "the context, the registries, three ledgers the card reads and the local player"
)]
pub fn follow_context(
    context: Res<CardContext>,
    registries: Option<Res<Registries>>,
    craft: Option<Res<CraftState>>,
    prints: Option<Res<Blueprints>>,
    players: Query<(&Hands, Option<&Combatant>, Option<&CharacterSheet>), With<LocalPlayer>>,
    mut made: Local<Arc<BTreeMap<u32, Made>>>,
    mut recorded: Local<Arc<Blueprints>>,
    mut sheet: Local<Arc<CharacterSheet>>,
) {
    if let Some(craft) = craft.filter(|craft| craft.is_changed() || craft.made.len() != made.len())
    {
        *made = Arc::new(craft.made.clone());
    }
    if let Some(prints) = prints.filter(|prints| **prints != **recorded) {
        *recorded = Arc::new(prints.clone());
    }
    if let Some(own) = players
        .single()
        .ok()
        .and_then(|(_, _, own)| own)
        .filter(|own| **own != **sheet)
    {
        *sheet = Arc::new(own.clone());
    }
    let against = registries
        .zip(players.single().ok())
        .map(|(registries, (hands, actor, _))| {
            Against::of(&registries.0.items, hands, actor)
                .with_made(made.clone())
                .with_blueprints(recorded.clone())
                .with_sheet(sheet.clone())
        });
    context.set_against(against);
}

/// The card as slotted's tooltip: the short one on hover, the long one
/// with Shift held. It replaces slotted's own parts, whose name and count
/// lines the card already carries, and falls back to them for a stack the
/// catalogue does not know, so a slot filled from elsewhere still says
/// what is in it. It adds nothing while the context is quiet, and a
/// tooltip with nothing in it is not raised.
#[derive(Debug, Clone)]
pub struct CardPart(pub Arc<GameRegistries>, pub CardContext);

impl TooltipPart for CardPart {
    fn build(&self, ctx: &TooltipCtx<'_>, out: &mut Vec<UiNodeDef>) {
        let Some(stack) = ctx.stack else {
            return;
        };
        if self.1.quiet() {
            return;
        }
        let against = self.1.against();
        let card = strata_sim::items::read(stack, &self.0.items)
            .ok()
            .and_then(|record| {
                CardParams::of(
                    &record,
                    &self.0,
                    ctx.tier == TooltipTier::Expanded,
                    against.as_ref(),
                )
            });
        if let Some(card) = card {
            out.push(card.node(None));
        } else {
            ItemNamePart.build(ctx, out);
            ItemCountPart.build(ctx, out);
        }
    }
}

/// `Startup`: the two widgets, under their kinds.
fn register_widgets(mut widgets: ResMut<WidgetRegistry>) {
    widgets.register(WidgetKind::new(WIDGET), CardWidget);
    widgets.register(WidgetKind::new(BAR_WIDGET), BarWidget);
}

/// Registers the widgets and the context. Only called once the UI is
/// wired.
pub(crate) fn register(app: &mut bevy::app::App) {
    app.init_resource::<CardContext>()
        .add_systems(bevy::app::Startup, register_widgets)
        .add_systems(bevy::app::Update, follow_context);
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;
    use strata_registry::Namespaced;
    use strata_sim::combat::Weapon;

    fn shipped() -> GameRegistries {
        let assets = concat!(env!("CARGO_MANIFEST_DIR"), "/../../assets");
        strata_registry::load_from_dir(assets).expect("assets/ loads")
    }

    fn item(name: &str) -> Namespaced {
        Namespaced::parse(name).expect("a valid id")
    }

    /// A row draws a weapon by its whole baked picture, at the picture's
    /// own shape: only a weapon, only one the index has, and nothing on a
    /// clone with nothing baked.
    #[test]
    fn a_weapons_picture_is_the_whole_one_with_its_shape() {
        use strata_registry::pictures::{ChassisPictures, PICTURES_VERSION, Placed};
        let registries = shipped();
        let items = &registries.items;
        let carbine = items.get(&item("veyra:weapon.patrol_carbine")).expect("it");
        let feed = items
            .get(&item("veyra:module.patrol_extended_feed"))
            .expect("the magazine");
        let mut pictures = PictureSet {
            version: PICTURES_VERSION,
            chassis: BTreeMap::new(),
        };
        assert_eq!(weapon_picture(&pictures, carbine), None, "nothing baked");
        let file = format!("{}/patrol_carbine/whole.png", pictures_dir());
        pictures.chassis.insert(
            "patrol_carbine".to_owned(),
            ChassisPictures {
                width: 1024,
                height: 398,
                metres_per_pixel: 0.001,
                origin: [0., 0.],
                whole: Placed {
                    rect: [12, 40, 1000, 300],
                    file: file.clone(),
                    icon: None,
                },
                pieces: Vec::new(),
            },
        );
        assert_eq!(
            weapon_picture(&pictures, carbine),
            Some((generated(&file), 1000, 300))
        );
        assert_eq!(weapon_picture(&pictures, feed), None, "a part has an icon");
    }

    /// The first day's weapons in the hands of somebody with no boxes, the
    /// carbine drawn: what a card is read against on the first day.
    fn first_day(items: &ItemCatalogue) -> Against {
        let starting = items.starting().expect("the first day's stock");
        let hands = Hands::from_saved(&starting.hands.clone().map(Some), items).expect("hands");
        Against::of(items, &hands, None)
    }

    /// A bought example of a module: every line at store grade.
    fn bought(items: &ItemCatalogue, name: &str) -> Record {
        let def = items.get(&item(name)).expect("a known module");
        Record {
            item: def.id.clone(),
            count: 1,
            batch: 0,
            by: None,
            lines: def
                .lines
                .iter()
                .map(|line| (line.id.clone(), STORE_GRADE))
                .collect(),
            condition: None,
            fitted: BTreeMap::new(),
            origin: None,
        }
    }

    fn effect(name: &str, value: &str, change: &str, better: Option<bool>) -> CardEffect {
        CardEffect {
            name: name.to_owned(),
            value: value.to_owned(),
            change: change.to_owned(),
            better,
            later: false,
        }
    }

    /// A melee weapon's card shows its cuts and reach in the game's units,
    /// and none of a magazine's figures, whether or not a hand holds it.
    #[test]
    fn a_blade_card_shows_its_cuts_and_no_magazine() {
        let registries = shipped();
        let items = &registries.items;
        let blade = bought(items, "veyra:weapon.service_blade");
        let against = first_day(items);
        let card = CardParams::of(&blade, &registries, true, Some(&against)).expect("a blade");
        let names: Vec<&str> = card.effects.iter().map(|e| e.name.as_str()).collect();
        assert_eq!(
            names,
            [
                "Light cut damage",
                "Light cut time",
                "Light cut recovery",
                "Light cut stamina",
                "Draw",
                "Heavy cut damage",
                "Heavy cut posture damage",
                "Reach",
                "Guard takes",
                "Parry window"
            ]
        );
        assert_eq!(card.effects[7].value, "1.7 m", "the Blade reaches 1.7 m");
        assert_eq!(card.effects[8].value, "60%");
        assert_eq!(card.effects[9].value, "0.20 s");
        let great = bought(items, "veyra:weapon.yard_greatblade");
        let card = CardParams::of(&great, &registries, true, Some(&against)).expect("a greatblade");
        assert_eq!(card.effects[7].value, "2.2 m");
    }

    /// A melee part's card reads against the blade carried: the change to
    /// that blade's figures, and a line the fight has no number for says so.
    #[test]
    fn a_melee_parts_card_reads_against_the_blade_carried() {
        let registries = shipped();
        let items = &registries.items;
        let mut blade = bought(items, "veyra:weapon.service_blade");
        blade.condition = Some(1000);
        let hands = Hands::from_saved(&[Some(blade), None], items).expect("hands");
        let against = Against::of(items, &hands, None);
        let edge = bought(items, "veyra:module.blade_hooked_edge");
        let card = CardParams::of(&edge, &registries, true, Some(&against)).expect("a part");
        let posture = card
            .effects
            .iter()
            .find(|effect| effect.name == "Posture damage")
            .unwrap_or_else(|| panic!("{:?}", card.effects));
        assert_eq!(posture.better, Some(true));
        assert!(!posture.change.is_empty() && !posture.later);
        let recovery = card
            .effects
            .iter()
            .find(|effect| effect.name == "Recovery")
            .unwrap_or_else(|| panic!("{:?}", card.effects));
        assert!(!recovery.later, "recovery is in the fight for a blade");
        let cell = bought(items, "veyra:module.cutter_compact_power");
        let mut great = bought(items, "veyra:weapon.yard_greatblade");
        great.condition = Some(1000);
        let hands = Hands::from_saved(&[Some(great), None], items).expect("hands");
        let against = Against::of(items, &hands, None);
        let card = CardParams::of(&cell, &registries, true, Some(&against)).expect("a part");
        let capacity = card
            .effects
            .iter()
            .find(|effect| effect.name == "Magazine")
            .unwrap_or_else(|| panic!("{:?}", card.effects));
        assert!(capacity.later, "a melee weapon has no magazine to change");
    }

    #[test]
    fn a_crafted_module_reads_as_the_tickets_card() {
        let registries = shipped();
        let items = &registries.items;
        let starting = items.starting().expect("the first day's stock");
        let crafted = starting
            .store
            .iter()
            .find(|record| record.batch == 412)
            .expect("the batch Tovan Reyes made");
        let against = first_day(items);
        let card =
            CardParams::of(crafted, &registries, true, Some(&against)).expect("a known item");
        assert_eq!(card.name, "Patrol extended magazine");
        assert_eq!(card.kind, "Mod · Feed");
        assert_eq!(
            card.from,
            "Crafted by Tovan Reyes · batch 412 · 2 in the stack"
        );
        assert_eq!(
            card.lines,
            vec![
                CardLine {
                    name: "Capacity".to_owned(),
                    value: 610,
                    note: String::new(),
                },
                CardLine {
                    name: "Handling".to_owned(),
                    value: 433,
                    note: String::new(),
                },
            ]
        );
        assert_eq!(
            card.notes,
            vec![
                "Fits the patrol carbine's feed".to_owned(),
                "Against the standard feed on the patrol carbine in hand.".to_owned(),
            ],
            "a crafted batch is not told that bought lines are 500"
        );
        assert_eq!(card.condition, None);
        assert_eq!(card.picture, None, "the picture is a weapon's");
    }

    /// A field repair kit's card says what the kit does in this member's
    /// hands: the sheet's Repair box lifts it, as the Repair button's own
    /// figure is.
    #[test]
    fn a_kits_card_reads_the_field_amount_on_the_members_sheet() {
        let registries = shipped();
        let items = &registries.items;
        let kit = bought(items, "veyra:kit.field_repair");
        let plain = first_day(items);
        let said = |against: &Against| {
            CardParams::of(&kit, &registries, false, Some(against))
                .expect("a known item")
                .notes
                .join(" ")
        };
        let base = strata_sim::wear::field_amount(items.wear(), 500);
        let mut sheet = CharacterSheet::default();
        sheet.learned.insert("fabricator.repair.1".to_owned());
        let lifted = strata_sim::wear::field_amount_for(
            items.wear(),
            500,
            Some(strata_sim::wear::Bearer {
                graph: &registries.progression,
                sheet: &sheet,
            }),
        );
        assert!(lifted > base, "the box lifts it: {base} to {lifted}");
        assert!(said(&plain).contains(&format!("Restores {base} points")));
        let skilled = first_day(items).with_sheet(Arc::new(sheet));
        assert!(said(&skilled).contains(&format!("Restores {lifted} points")));
    }

    /// Rule 2 of the ticket on the card: units, then the change against
    /// what is in that mount now. A longer reload is marked as worse, in a
    /// word, and no factor is anywhere on it.
    #[test]
    fn a_feeds_card_reads_29_rounds_plus_5() {
        let registries = shipped();
        let items = &registries.items;
        let against = first_day(items);
        let feed = bought(items, "veyra:module.patrol_extended_feed");
        let card = CardParams::of(&feed, &registries, false, Some(&against)).expect("a known item");
        assert_eq!(
            card.effects,
            vec![
                effect("Magazine", "29 rounds", "+5", Some(true)),
                effect("Reload", "1.49 s", "+0.14 s worse", Some(false)),
            ]
        );
        let text = card.text();
        assert!(text.contains("Magazine 29 rounds +5"), "{text}");
        assert!(text.contains("Reload 1.49 s +0.14 s worse"), "{text}");
        for factor in ["1.2", "1.1", "x1", "\u{d7}"] {
            assert!(!text.contains(factor), "a factor on the card: {text}");
        }

        // Fitted, the same card is read against the standard part, and
        // says so; a second one of the batch would change nothing.
        let mut fitted = against.clone();
        let module = Fitted {
            item: feed.item.clone(),
            batch: 0,
            by: None,
            lines: feed.lines.clone(),
            condition: None,
        };
        let (carbine, _) = fitted.hands[0].as_mut().expect("the carbine");
        carbine.fitted.insert("feed".to_owned(), module);
        let card = CardParams::of(&feed, &registries, false, Some(&fitted)).expect("a known item");
        assert_eq!(
            card.effects[0],
            effect("Magazine", "29 rounds", "+5", Some(true))
        );
        assert_eq!(
            card.notes[1], "Fitted now. Against the standard feed.",
            "{:?}",
            card.notes
        );
        // Another batch is read against the one fitted: the same rounds, a
        // touch slower, and the sentence names what it is set against.
        let starting = items.starting().expect("the first day's stock");
        let crafted = starting
            .store
            .iter()
            .find(|record| record.batch == 412)
            .expect("the crafted batch");
        let card =
            CardParams::of(crafted, &registries, false, Some(&fitted)).expect("a known item");
        assert_eq!(card.effects[0], effect("Magazine", "29 rounds", "", None));
        assert_eq!(
            card.notes[1],
            "Against the patrol extended magazine fitted now."
        );

        // The pistol's cell is no part of the carbine in hand: it is read
        // on the pistol, and the card says which weapon that is.
        let cell = bought(items, "veyra:module.pistol_extended_cell");
        let card = CardParams::of(&cell, &registries, false, Some(&against)).expect("a known item");
        assert_eq!(
            card.effects[0],
            effect("Magazine", "13 rounds", "+3", Some(true))
        );
        assert_eq!(
            card.notes[1],
            "Against the standard feed on the service pistol holstered."
        );
        // With nobody carrying anything there is nothing to measure on.
        let card = CardParams::of(&cell, &registries, false, None).expect("a known item");
        assert!(card.effects.is_empty());
        assert_eq!(card.notes[1], "No weapon in hand takes it.");

        // The sight's spread is a number of the fight, in degrees.
        let sight = bought(items, "veyra:module.vigil_reflex");
        let card =
            CardParams::of(&sight, &registries, false, Some(&against)).expect("a known item");
        assert_eq!(
            card.effects,
            vec![
                effect("Aimed spread", "1.40°", "−0.60°", Some(true)),
                effect("Draw", "0.46 s", "+0.01 s worse", Some(false)),
            ]
        );
        assert!(
            card.text().contains("Aimed spread 1.40° −0.60°"),
            "{}",
            card.text()
        );
    }

    /// A weapon's card shows the row its holder fights by: bare it is the
    /// tables' own, fitted it is the magazine the HUD reads, set against
    /// bare.
    #[test]
    fn a_weapons_card_reads_its_fitted_row() {
        let registries = shipped();
        let items = &registries.items;
        let mut against = first_day(items);
        let carbine = against.hands[0].clone().expect("the carbine").0;
        let bare =
            CardParams::of(&carbine, &registries, false, Some(&against)).expect("a known item");
        assert_eq!(
            bare.effects,
            vec![
                effect("Magazine", "24 rounds", "", None),
                effect("Reload", "1.35 s", "", None),
                effect("Pause between bursts", "0.462 s", "", None),
                effect("Fire", "Burst of 3, Hold for full auto", "", None),
                effect("Falloff", "Full to 25 m, 55% at 50 m", "", None),
                effect("Reach", "70 m", "", None),
            ]
        );
        let feed = bought(items, "veyra:module.patrol_extended_feed");
        let mut fitted = carbine.clone();
        fitted.fitted.insert(
            "feed".to_owned(),
            Fitted {
                item: feed.item.clone(),
                batch: 0,
                by: None,
                lines: feed.lines.clone(),
                condition: None,
            },
        );
        against.hands[0].as_mut().expect("the carbine").0 = fitted.clone();
        let card =
            CardParams::of(&fitted, &registries, true, Some(&against)).expect("a known item");
        assert_eq!(
            card.effects,
            vec![
                effect("Magazine", "29 rounds", "+5", Some(true)),
                effect("Reload", "1.49 s", "+0.14 s worse", Some(false)),
                effect("Pause between bursts", "0.462 s", "", None),
                effect("Fire", "Burst of 3, Hold for full auto", "", None),
                effect("Falloff", "Full to 25 m, 55% at 50 m", "", None),
                effect("Reach", "70 m", "", None),
            ]
        );
        assert_eq!(
            card.notes,
            vec![
                "Fitted: Patrol extended magazine".to_owned(),
                "4 of 6 mounts open".to_owned(),
                "Every line of a bought item is 500, the tick on each bar.".to_owned(),
            ]
        );
        // A weapon nobody carries has no holder to read its row for.
        let card = CardParams::of(&fitted, &registries, false, None).expect("a known item");
        assert!(card.effects.is_empty());
    }

    /// The fire mode, the falloff and the reach are the row's own words,
    /// for each gun, and no part moves them.
    #[test]
    fn the_interval_is_named_for_how_the_weapon_fires() {
        assert_eq!(
            property_name_for(Property::RepeatInterval, Weapon::Carbine.spec().mode),
            "Pause between bursts"
        );
        assert_eq!(
            property_name_for(Property::RepeatInterval, Weapon::Pistol.spec().mode),
            "Time between shots"
        );
        assert_eq!(
            property_name_for(Property::Capacity, Weapon::Carbine.spec().mode),
            property_name(Property::Capacity)
        );
    }

    #[test]
    fn a_guns_mode_range_and_reach_are_said_from_its_row() {
        let carbine = weapon_facts(&Weapon::Carbine.spec());
        assert_eq!(carbine[0].1, "Burst of 3, Hold for full auto");
        assert_eq!(carbine[1].1, "Full to 25 m, 55% at 50 m");
        assert_eq!(carbine[2].1, "70 m");
        let pistol = weapon_facts(&Weapon::Pistol.spec());
        assert_eq!(pistol[0].1, "Single shot");
        assert_eq!(pistol[1].1, "Full to 30 m, 60% at 60 m");
        assert_eq!(pistol[2].1, "90 m");
        let mut half = Weapon::Pistol.spec();
        half.falloff.share = 0.5;
        assert_eq!(weapon_facts(&half)[1].1, "Full to 30 m, half at 60 m");
    }

    #[test]
    fn the_rifles_card_says_a_single_shot_its_range_and_how_long_its_aim_takes() {
        let rifle = weapon_facts(&Weapon::Rifle.spec());
        assert_eq!(rifle[0].1, "Single shot");
        assert!(rifle[1].1.starts_with("Full to 150 m"), "{rifle:?}");
        assert_eq!(rifle[3], ("Aim".to_owned(), "Settles in 0.9 s".to_owned()));
        assert_eq!(
            weapon_facts(&Weapon::Carbine.spec()).len(),
            3,
            "a weapon that aims at once says nothing of it"
        );
    }

    #[test]
    fn a_bike_reads_its_figures_and_the_change_from_a_bought_one() {
        let registries = shipped();
        let vehicle = "veyra:vehicle.courier_bike".parse().expect("an id");
        let bought = strata_sim::ride::store_grade_bike(&registries.items, &vehicle)
            .expect("the shipped bike item");
        let card = CardParams::of(&bought, &registries, false, None).expect("a known item");
        assert_eq!(
            (card.name.as_str(), card.kind.as_str()),
            ("Courier bike", "Bike")
        );
        let figures: Vec<_> = card
            .effects
            .iter()
            .map(|effect| {
                (
                    effect.name.as_str(),
                    effect.value.as_str(),
                    effect.change.as_str(),
                )
            })
            .collect();
        assert_eq!(
            figures,
            vec![
                ("Top speed, road", "216 km/h", ""),
                ("Top speed, off road", "130 km/h", ""),
                ("Turn", "150 to 85\u{b0}/s", ""),
                ("Saddle bags", "18 cells (6 x 3)", ""),
            ]
        );
        assert_eq!(card.condition, Some(1000));
        // A fast, clumsy, worn one: each figure with its change.
        let mut other = bought.clone();
        other.lines.insert("speed".to_owned(), 1000);
        other.lines.insert("handling".to_owned(), 0);
        let card = CardParams::of(&other, &registries, false, None).expect("a known item");
        let road = &card.effects[0];
        assert_eq!(
            (road.value.as_str(), road.change.as_str()),
            ("238 km/h", "+22 km/h")
        );
        assert_eq!(road.better, Some(true));
        let turn = &card.effects[2];
        assert_eq!(turn.change, "\u{2212}13\u{b0}/s worse");
        assert_eq!(turn.better, Some(false));
        other.condition = Some(0);
        let worn = CardParams::of(&other, &registries, false, None).expect("a known item");
        assert!(
            worn.effects[0].change.ends_with("worse"),
            "{:?}",
            worn.effects[0]
        );
        assert_eq!(worn.condition, Some(0));
    }

    #[test]
    fn a_bought_weapon_reads_store_grade_and_its_condition() {
        let registries = shipped();
        let items = &registries.items;
        let carbine = &items.starting().expect("the first day's stock").hands[0];
        let short = CardParams::of(carbine, &registries, false, None).expect("a known item");
        assert_eq!(short.name, "Vigil patrol carbine");
        assert_eq!(short.kind, "Weapon");
        assert_eq!(short.from, "Store grade · Vigil");
        assert_eq!(short.condition, Some(1000));
        assert_eq!(short.picture.as_deref(), Some("patrol_carbine/whole.png"));
        assert_eq!(
            short.notes,
            vec![
                "Nothing fitted".to_owned(),
                "Hold Shift for the long card".to_owned()
            ]
        );
        let long = CardParams::of(carbine, &registries, true, None).expect("a known item");
        assert_eq!(
            long.notes.last().map(String::as_str),
            Some("Every line of a bought item is 500, the tick on each bar.")
        );
        assert!(long.text().contains("1000 of 1000"), "{}", long.text());
        let bag = Record {
            item: item("veyra:bag.field_satchel"),
            count: 1,
            batch: 0,
            by: None,
            lines: BTreeMap::new(),
            condition: None,
            fitted: BTreeMap::new(),
            origin: None,
        };
        let bag = CardParams::of(&bag, &registries, true, None).expect("a known item");
        assert_eq!(bag.kind, "Bag");
        assert_eq!(bag.from, "Store grade · the dealer");
        assert_eq!(bag.notes, vec!["In a bag slot it opens 6 more slots."]);
    }

    /// The ticket's lot card: named for its deposit, its family, class and
    /// type as its kind, where it was drawn and how many units, then a
    /// line for each attribute its class carries and no other, each with
    /// the range its type has. Two deposits of one class differ on it.
    #[test]
    #[allow(
        clippy::too_many_lines,
        reason = "one card in each of its forms, read top to bottom"
    )]
    fn a_lots_card_draws_its_spans() {
        use strata_registry::Attribute::{Potency, Purity, Stability};
        let registries = shipped();
        let set = &registries.resources;
        let salt = |root: &str, cell: [i32; 2], figures: [u16; 3]| {
            let mut deposit = crate::ui::testing::deposit(
                &registries,
                "strata:evaporite_salt",
                root,
                cell,
                [0., 0.],
            );
            deposit.figures = vec![
                (Purity, figures[0]),
                (Potency, figures[1]),
                (Stability, figures[2]),
            ];
            deposit
        };
        let calder = salt("Calder", [3, -2], [610, 820, 240]);
        let lot = deposits::lot(&calder, set, 80).expect("a lot");
        let card = CardParams::of(&lot, &registries, true, None).expect("a known item");
        assert_eq!(card.name, "Calder salt");
        assert_eq!(card.kind, "Mineral · Reactive · Evaporite salt");
        assert_eq!(card.from, "Calder flats · 80 units");
        let range = |name: &str, value, floor, cap| CardRange {
            name: name.to_owned(),
            value,
            floor,
            cap,
            word: String::new(),
        };
        assert_eq!(
            card.ranges,
            vec![
                range("Purity", 610, 200, 900),
                range("Potency", 820, 300, 950),
                range("Stability", 240, 100, 700),
            ],
            "the three a reactive carries, in its class's order, and no other"
        );
        assert!(card.lines.is_empty() && card.effects.is_empty());
        assert_eq!((card.condition, &card.picture), (None, &None));
        let text = card.text();
        assert!(text.contains("Potency 820 of 300 to 950"), "{text}");
        assert!(text.contains("Stability 240 of 100 to 700"), "{text}");
        for absent in ["Strength", "Conductivity", "Heat tolerance", "PT", "PU"] {
            assert!(!text.contains(absent), "{absent} on a salt's card: {text}");
        }
        assert_eq!(
            card.notes,
            vec![
                "Sampled at Calder flats. The band under each bar is the range evaporite salt \
                 can have anywhere on Veyra."
                    .to_owned()
            ]
        );
        let short = CardParams::of(&lot, &registries, false, None).expect("a known item");
        assert_eq!(short.ranges, card.ranges, "three lines need no cut");
        assert_eq!(short.notes, vec!["Hold Shift for the long card".to_owned()]);
        assert_eq!(CardParams::from_value(&card.to_value()), Ok(card.clone()));

        // Another deposit of the same type is another card: its own name
        // and its own figures in the same ranges.
        let harrow = salt("Harrow", [4, -2], [300, 410, 660]);
        let other = deposits::lot(&harrow, set, 80).expect("a lot");
        let other = CardParams::of(&other, &registries, true, None).expect("a known item");
        assert_eq!(other.name, "Harrow salt");
        assert_ne!(other.ranges, card.ranges);
        assert_eq!(
            other
                .ranges
                .iter()
                .map(|r| (r.floor, r.cap))
                .collect::<Vec<_>>(),
            card.ranges
                .iter()
                .map(|r| (r.floor, r.cap))
                .collect::<Vec<_>>()
        );
        // And the other type of the class has the class's lines in ranges
        // of its own.
        let sorrel =
            crate::ui::testing::deposit(&registries, "strata:sulphur", "Sorrel", [5, 1], [0., 0.]);
        let sulphur = deposits::lot(&sorrel, set, 1).expect("a lot");
        let sulphur = CardParams::of(&sulphur, &registries, true, None).expect("a known item");
        assert_eq!(sulphur.name, "Sorrel sulphur");
        assert_eq!(sulphur.kind, "Mineral · Reactive · Sulphur");
        assert_eq!(sulphur.from, "Sorrel vents · 1 unit");
        assert_eq!(
            sulphur
                .ranges
                .iter()
                .map(|r| r.name.as_str())
                .collect::<Vec<_>>(),
            ["Purity", "Potency", "Stability"]
        );
        assert_eq!(
            (sulphur.ranges[1].floor, sulphur.ranges[1].cap),
            (550, 1000),
            "sulphur's potency, not salt's"
        );

        // Eight lines are too many for the short card: it shows the four
        // the lot stands highest in, in the class's order, and says how
        // many more the long one has.
        let mut rake = crate::ui::testing::deposit(
            &registries,
            "strata:light_alloy_ore",
            "Rake",
            [0, 0],
            [0., 0.],
        );
        let ranges = &set.get(rake.ty).expect("the type").ranges;
        for (index, (attribute, value)) in rake.figures.iter_mut().enumerate() {
            let span = ranges[attribute];
            // Odd lines at their caps, even ones at their floors.
            *value = if index % 2 == 1 { span.cap } else { span.floor };
        }
        let alloy = deposits::lot(&rake, set, 200).expect("a lot");
        let long = CardParams::of(&alloy, &registries, true, None).expect("a known item");
        assert_eq!(long.name, "Rake light alloy");
        assert_eq!(long.kind, "Mineral · Structural metal · Light alloy ore");
        assert_eq!(long.ranges.len(), 8);
        let short = CardParams::of(&alloy, &registries, false, None).expect("a known item");
        assert_eq!(
            short
                .ranges
                .iter()
                .map(|r| r.name.as_str())
                .collect::<Vec<_>>(),
            ["Strength", "Hardness", "Stability", "Workability"],
            "the four at their caps, in the class's order"
        );
        assert_eq!(short.notes, vec!["4 more with Shift".to_owned()]);

        // Commodity stock, which no deposit gave: the type's own name, and
        // it says what it is.
        let mut commodity = lot.clone();
        commodity.origin = None;
        for value in commodity.lines.values_mut() {
            *value = STORE_GRADE;
        }
        let card = CardParams::of(&commodity, &registries, true, None).expect("a known item");
        assert_eq!(card.name, "Evaporite salt");
        assert_eq!(card.from, "Commodity grade · 80 units");
        assert_eq!(
            card.notes,
            vec!["Certified stock: every line is 500, whatever the ground gives.".to_owned()]
        );

        // A deposit's own card, before anything is drawn, names its place
        // in the place of a count.
        let exact = Reading::Exact;
        let card = CardParams::of_deposit(&calder, &registries, exact, true).expect("a known type");
        assert_eq!(card.name, "Calder salt");
        assert_eq!(card.from, "Calder flats");
        assert_eq!(card.ranges[1], range("Potency", 820, 300, 950));
        let free =
            CardParams::of_deposit(&sorrel, &registries, exact, false).expect("a known type");
        assert_eq!(free.from, "Sorrel vents");
        // A deposit is drawn by how well it is read: the type and "Not
        // read", each line a band word on its bar, or the figures.
        let unread = CardParams::of_deposit(&calder, &registries, Reading::Type, true).expect("it");
        assert_eq!(unread.name, "Calder salt");
        assert!(unread.ranges.is_empty() && unread.lines.is_empty());
        assert!(unread.text().contains("Not read"), "{}", unread.text());
        let banded =
            CardParams::of_deposit(&calder, &registries, Reading::Bands, true).expect("it");
        let said: Vec<&str> = banded
            .ranges
            .iter()
            .map(|range| range.word.as_str())
            .collect();
        // 610 of 200 to 900 is the share 586: good. 820 of 300 to 950: 800,
        // fine. 240 of 100 to 700: 233, poor.
        assert_eq!(said, ["good", "fine", "poor"]);
        assert!(banded.text().contains("Potency fine"), "{}", banded.text());
        assert!(!banded.text().contains("820"), "the figure is not told");
        assert_eq!(CardParams::from_value(&banded.to_value()), Ok(banded));
        // A stack is called by its deposit in a sentence.
        let stack = strata_sim::items::write(&lot, &registries.items).expect("a stack");
        assert_eq!(stack_name(&registries, &stack, None), "Calder salt");
        let bag = strata_sim::items::write(
            &Record {
                item: item("veyra:bag.field_satchel"),
                count: 1,
                batch: 0,
                by: None,
                lines: BTreeMap::new(),
                condition: None,
                fitted: BTreeMap::new(),
                origin: None,
            },
            &registries.items,
        )
        .expect("a stack");
        assert_eq!(stack_name(&registries, &bag, None), "Field satchel");
    }

    #[test]
    fn the_parameters_survive_a_custom_node() {
        let registries = shipped();
        let items = &registries.items;
        let starting = items.starting().expect("the first day's stock");
        let against = first_day(items);
        for record in starting.hands.iter().chain(&starting.store) {
            let card = CardParams::of(record, &registries, true, Some(&against))
                .expect("a known item")
                .framed();
            assert_eq!(CardParams::from_value(&card.to_value()), Ok(card));
        }
        let empty = CardParams::empty();
        assert_eq!(CardParams::from_value(&empty.to_value()), Ok(empty));
        assert!(CardParams::from_value(&Value::Bool(true)).is_err());
        // A bar reads the bare figure of a quality line and its own
        // parameters, and a moddability bar keeps its thresholds.
        let bar = BarParams {
            ticks: vec![400, 600, 800],
            numeral: false,
            ..BarParams::quality(500)
        };
        let UiNodeDef::Custom { params, .. } = bar.node(None) else {
            panic!("a custom node");
        };
        assert_eq!(BarParams::from_value(&params), Some(bar));
        let ranged = BarParams::ranged(820, 300, 950);
        assert!(ranged.ticks.is_empty() && !ranged.numeral);
        let UiNodeDef::Custom { params, .. } = ranged.node(None) else {
            panic!("a custom node");
        };
        assert_eq!(BarParams::from_value(&params), Some(ranged));
        let bare = slotted::registry::from_model(&Model::Int(610));
        assert_eq!(BarParams::from_value(&bare), Some(BarParams::quality(610)));
        // A craft's bar keeps its floor, its ceiling and what its numeral
        // reads, and has no store-grade tick.
        for bar in [BarParams::offered(358, 597), BarParams::worked(414, 597)] {
            assert!(bar.ticks.is_empty() && bar.ceiling == Some(597));
            let UiNodeDef::Custom { params, .. } = bar.node(None) else {
                panic!("a custom node");
            };
            assert_eq!(BarParams::from_value(&params), Some(bar));
        }
        assert_eq!(BarParams::offered(358, 597).reads, Some(597));
        assert_eq!(BarParams::offered(358, 597).floor, Some(358));
    }

    /// A stack of `name` made by the household in `batch`, with `lines` in
    /// the item's order.
    fn made(items: &ItemCatalogue, name: &str, batch: u32, lines: &[u16]) -> Record {
        let mut record = bought(items, name);
        record.batch = batch;
        record.by = Some("Frontier ranger".to_owned());
        let def = items.get(&item(name)).expect("a known item");
        for (line, figure) in def.lines.iter().zip(lines) {
            record.lines.insert(line.id.clone(), *figure);
        }
        record
    }

    /// A part's card is its lines as bars; the long one says under each
    /// where the line goes next, by the first schematic that weighs it.
    #[test]
    fn a_components_card_says_where_its_lines_go() {
        let registries = shipped();
        let items = &registries.items;
        let core = made(items, "veyra:component.cell_core", 1, &[466, 209]);
        let short = CardParams::of(&core, &registries, false, None).expect("a card");
        assert_eq!(short.name, "Cell core");
        assert_eq!(short.kind, "Part");
        assert_eq!(short.from, "Crafted by Frontier ranger · batch 1");
        assert!(short.effects.is_empty(), "a part does nothing on a weapon");
        assert_eq!(
            short
                .lines
                .iter()
                .map(|line| (line.name.as_str(), line.value, line.note.as_str()))
                .collect::<Vec<_>>(),
            [("Charge density", 466, ""), ("Stability", 209, "")]
        );
        assert_eq!(short.notes, ["Hold Shift for the long card"]);
        let long = CardParams::of(&core, &registries, true, None).expect("a card");
        assert_eq!(
            long.lines
                .iter()
                .map(|line| line.note.as_str())
                .collect::<Vec<_>>(),
            [
                "Goes into: Capacity of a patrol extended magazine",
                "Goes into nothing yet"
            ]
        );
        assert!(
            long.text()
                .contains("Charge density 466\nGoes into: Capacity of a patrol extended magazine"),
            "{}",
            long.text()
        );
        assert_eq!(CardParams::from_value(&long.to_value()), Ok(long));
        // A bought part reads store grade and says where it goes too.
        let harness = bought(items, "veyra:component.harness");
        let long = CardParams::of(&harness, &registries, true, None).expect("a card");
        assert_eq!(long.from, "Store grade · the dealer");
        // The carbine's schematic sorts before the magazine's, so it is
        // where the harness goes next.
        assert_eq!(long.lines[0].note, "Goes into: Action of a patrol carbine");
        assert_eq!(
            long.notes,
            ["Every line of a bought item is 500, the tick on each bar."]
        );
    }

    /// A bought-in part has no lines and says so, short card and long.
    #[test]
    fn a_commodity_card_is_flat() {
        let registries = shipped();
        let mut seals = bought(&registries.items, "veyra:commodity.seal_set");
        seals.count = 6;
        for long in [false, true] {
            let card = CardParams::of(&seals, &registries, long, None).expect("a card");
            assert_eq!(card.name, "Seal set");
            assert_eq!(card.kind, "Part · bought in");
            assert_eq!(card.from, "Store grade · the dealer · 6 in the stack");
            assert!(card.lines.is_empty() && card.effects.is_empty());
            assert_eq!(
                card.notes[0],
                "A certified part. It has no quality of its own and every one is the same."
            );
            assert_eq!(card.notes.len(), if long { 1 } else { 2 });
        }
    }

    /// The long card of a thing the household made says what went into it,
    /// from the household's ledger by its batch: a lot by its deposit, a
    /// made part by its batch, a bought one by its grade. Fitted or not, a
    /// module of that batch says the same; a batch somebody else made, or
    /// one the ledger has not got, says nothing.
    #[test]
    fn the_long_card_says_what_it_was_made_from() {
        let registries = shipped();
        let items = &registries.items;
        let tolley = crate::ui::testing::deposit(
            &registries,
            "strata:evaporite_salt",
            "Tolley",
            [3, -2],
            [0., 0.],
        );
        let salt = deposits::lot(&tolley, &registries.resources, 20).expect("a lot");
        let core = made(items, "veyra:component.cell_core", 1, &[466, 209]);
        let feed = made(items, "veyra:module.patrol_extended_feed", 2, &[475, 372]);
        let mut seals = bought(items, "veyra:commodity.seal_set");
        seals.count = 1;
        let ledger = BTreeMap::from([
            (
                1,
                Made {
                    item: core.item.clone(),
                    by: "Frontier ranger".to_owned(),
                    from: vec![("salt".to_owned(), salt)],
                    schematic: None,
                    recorded: None,
                    blueprint: None,
                },
            ),
            (
                2,
                Made {
                    item: feed.item.clone(),
                    by: "Frontier ranger".to_owned(),
                    from: vec![
                        ("core".to_owned(), core.clone()),
                        (
                            "harness".to_owned(),
                            bought(items, "veyra:component.harness"),
                        ),
                        ("seals".to_owned(), seals),
                    ],
                    schematic: None,
                    recorded: None,
                    blueprint: None,
                },
            ),
        ]);
        let against = first_day(items).with_made(Arc::new(ledger));
        let last = |record: &Record, long: bool| {
            CardParams::of(record, &registries, long, Some(&against))
                .expect("a card")
                .notes
                .last()
                .cloned()
                .unwrap_or_default()
        };
        assert_eq!(last(&core, true), "Made from Tolley salt.");
        assert_eq!(
            last(&feed, true),
            "Made from cell core batch 1 · harness, store grade · seal set."
        );
        assert_ne!(last(&core, false), last(&core, true), "the long card's own");
        // Tovan Reyes's batch 412 is no batch of this household's, and a
        // bought magazine has no batch at all.
        let starting = items.starting().expect("the first day's stock");
        for record in &starting.store {
            assert!(!last(record, true).starts_with("Made from"), "{record:?}");
        }
        let mut theirs = feed.clone();
        theirs.by = Some("Tovan Reyes".to_owned());
        assert!(!last(&theirs, true).starts_with("Made from"));
        // With no ledger to read, nothing is claimed.
        let card = CardParams::of(&feed, &registries, true, None).expect("a card");
        assert!(card.notes.iter().all(|note| !note.starts_with("Made from")));
    }

    /// A thing made by somebody with no name carries the sim's empty maker,
    /// and its card words it: the sim writes no English into a record. Its
    /// ledger entry is under the same empty name, so the card still finds
    /// what it was made from.
    #[test]
    fn a_maker_with_no_name_is_worded_by_the_card() {
        let registries = shipped();
        let items = &registries.items;
        let mut core = made(items, "veyra:component.cell_core", 3, &[466, 209]);
        assert_eq!(
            CardParams::of(&core, &registries, true, None)
                .expect("a card")
                .from,
            "Crafted by Frontier ranger \u{b7} batch 3"
        );
        core.by = Some(String::new());
        assert_eq!(items.check_record(&core), Ok(()));
        let mut seals = bought(items, "veyra:commodity.seal_set");
        seals.count = 1;
        let ledger = BTreeMap::from([(
            3,
            Made {
                item: core.item.clone(),
                by: String::new(),
                from: vec![("seals".to_owned(), seals)],
                schematic: None,
                recorded: None,
                blueprint: None,
            },
        )]);
        let against = first_day(items).with_made(Arc::new(ledger));
        let card = CardParams::of(&core, &registries, true, Some(&against)).expect("a card");
        assert_eq!(card.from, "Crafted by the household \u{b7} batch 3");
        assert_eq!(
            card.notes.last().map(String::as_str),
            Some("Made from seal set.")
        );
    }

    /// A sentence names one of a thing with the article its definition
    /// gives, and with the usual one when it gives none: no first letter
    /// is looked at.
    #[test]
    fn a_things_article_is_its_definitions() {
        let registries = shipped();
        let id = |name: &str| strata_registry::Namespaced::parse(name).expect("an id");
        assert_eq!(
            a_name(&registries.items, &id("veyra:module.patrol_extended_feed")).as_deref(),
            Some("a patrol extended magazine")
        );
        assert_eq!(one_of(None, "extended cell"), "a extended cell");
        assert_eq!(one_of(Some("an"), "extended cell"), "an extended cell");
        assert_eq!(one_of(Some("the"), "harness"), "the harness");
    }

    #[test]
    fn every_role_the_card_names_is_in_the_theme() {
        let path = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../assets/themes/glass.theme.ron"
        );
        let theme = Theme::from_ron(&std::fs::read_to_string(path).expect("the theme"))
            .expect("the theme parses");
        for role in ROLES.into_iter().chain([PICTURE_ROLE]) {
            assert!(
                theme.roles.contains_key(&Role::new_static(role)),
                "{role} is not in the theme"
            );
        }
        // The bar is its fill and its tick: a role that is no solid would
        // draw neither.
        for role in [
            TRACK_ROLE,
            FILL_ROLE,
            CONDITION_FILL_ROLE,
            CONDITION_WORN_FILL_ROLE,
            TICK_ROLE,
            RULE_ROLE,
            LOT_TRACK_ROLE,
            RANGE_ROLE,
            RANGE_END_ROLE,
            MARK_ROLE,
        ] {
            let paint = Paint::for_role(&theme, &Role::new_static(role));
            assert!(paint.and_then(|paint| paint.background).is_some(), "{role}");
        }
        // Every property has a name, and the five the fight has no number
        // for have a word for each way they can go.
        for property in Property::ALL {
            assert_ne!(property_name(property), property.key(), "{property:?}");
            if !(fitting::live_on(property, false) && fitting::live_on(property, true)) {
                for side in ["less", "more"] {
                    let key = format!("strata.property.{}.{side}", property.key());
                    assert!(english(&key).is_some(), "{key}");
                }
            }
        }
    }

    /// A weapon's card with nothing baked still heads itself with the
    /// item's image, found among the images the card is given.
    #[test]
    fn a_weapon_card_falls_back_to_the_items_image() {
        let registries = shipped();
        let rifle = registries
            .items
            .get(&item("veyra:weapon.marksman_rifle"))
            .expect("the rifle");
        let record = Record {
            item: rifle.id.clone(),
            count: 1,
            batch: 0,
            by: None,
            lines: BTreeMap::new(),
            condition: None,
            fitted: BTreeMap::new(),
            origin: None,
        };
        let params = CardParams::of(&record, &registries, false, None).expect("a card");
        assert_eq!(params.icon.as_deref(), Some(rifle.id.to_string().as_str()));
        assert!(
            item_art(rifle, None, None, &registries.icons).is_some(),
            "the rule has an image with nothing baked"
        );
    }
}
