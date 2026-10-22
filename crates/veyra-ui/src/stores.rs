//! The stores: what a character carries, the weapons in their hands and,
//! at a locker, the household's store beside them (ticket 0007, step 1).
//!
//! One screen in two forms. A locker's F opens it over the household's
//! store; key I and the pause menu's Inventory button open it anywhere
//! without the store. The pack is a grid handled the way Minecraft's is: a
//! click carries a stack, a right click takes half or puts one down, Shift
//! and a click send a stack across. Two bag slots stand under it, and a bag
//! put in one opens its own cells.
//!
//! No rule lives here. The truth is the sim's `Pack`, `Hands` and
//! `HouseholdStore`; the screen holds a mirror of them as slotted
//! inventories, on which slotted predicts each click, and [`StockAuthority`]
//! sends the click on as a `StockRequest` for the tick to replay on the
//! truth. The sim's answer comes back through the same port: an
//! acknowledgement, or the truth to overwrite the mirror with. The menu
//! both sides click through is the sim's `layout`, derived here from the
//! mirror after every change, so a bag that came in opens its cells and a
//! bag that holds something is locked without anything being sent across.
//!
//! Slotted refuses a click on a locked slot and tells nobody, so the screen
//! asks the sim's own `apply` on a copy of its mirror what the matter is
//! and says it at the slot: a bag that holds something says so in its
//! caption. A weapon in hand is not carried off either, and a click on it
//! opens its fitting ([`super::fitting`]).
//!
//! A cell draws its item's baked picture where the content step has made
//! one ([`follow_registries`]) and its library symbol where it has not, so
//! a clone with nothing baked shows every stack all the same. A lot's cell
//! is its class's symbol in its class's colour, its count the units, and
//! its pip where it stands in its type's spans. The household's credits stand at the right of the head and
//! follow the sim's purse.
//!
//! The screen is built once and never rebuilt while it is up, because
//! popping a screen closes its menu. What changes under it is written to
//! the nodes that show it: the bag cells a bag opens, the captions, the
//! filter chips, the line that names what is carried, and the card pinned
//! in the pane, which is that of the stack last focused or pointed at.
//! With the card pinned, the one slotted would raise beside the pointer
//! stands down: it would say the pane over again on top of the cells.
//! Local-only: nothing here is replicated.

use std::collections::{BTreeMap, HashMap, VecDeque};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use bevy::asset::AssetServer;
use bevy::camera::visibility::Visibility;
use bevy::color::{Alpha, Color};
use bevy::ecs::prelude::*;
use bevy::input::ButtonInput;
use bevy::input::keyboard::KeyCode;
use bevy::input_focus::tab_navigation::TabIndex;
use bevy::input_focus::{FocusCause, InputFocus};
use bevy::math::Vec2;
use bevy::picking::Pickable;
use bevy::picking::hover::Hovered;
use bevy::picking::pointer::{PointerAction, PointerInput};
use bevy::text::TextColor;
use bevy::time::Time;
use bevy::ui::widget::{ImageNode, Text};
use bevy::ui::{Display, InteractionDisabled, Node, PositionType, RepeatedGridTrack, Val};
use bevy::window::CursorMoved;
use slotted::ecs::{
    Carried, Dropped, Inventory, MenuAction, MenuClosed, MenuIdAllocator, Modifiers, OpenMenu,
    SlotClicked, SlotRef, SlottedEcsSet, open_menu,
};
use slotted::menu::{MenuChoice, ToastLevel, ToastSpec, toast};
use slotted::model::{
    Actor, AuthorityError, AuthorityEvent, Button, ClickAction, Delta, ItemId, ItemStack, MenuId,
    SlotBehaviour, SlotIx, SlotRange,
};
use slotted::theme::{Role, Themed};
use slotted::ui::def::{
    Align, ButtonOpts, Justify, Layout, LayoutDirection, Length, LocKey, Overflow, Padding,
    Presentation, PresentationMode, ScreenDef, ScreenKind, Tags, TextOpts, TextRole, Transition,
    UiNodeDef, WidgetKind,
};
use slotted::ui::{
    ButtonRole, ButtonState, FocusedAction, InputMode, ItemCount, ItemIcon, ItemView, LocText,
    ScreenStack, SemanticLabel, SlottedUiSet, TestId, TooltipParts, UiAction, UiActionClaims,
    active_tokens, pop_screen, push_screen,
};
use strata_registry::item::{BAG_SLOTS, CarryRules, HANDS, ItemCatalogue, ItemKind};
use strata_registry::pictures::PictureSet;
use veyra_sim::combat::{Action, Combatant, Weapon};
use veyra_sim::crafting::kind_of;
use veyra_sim::deposits::Purse;
use veyra_sim::equip::{self, EquipAnswer, EquipInbox, EquipRefusal, EquipRequest};
use veyra_sim::items::{
    self, Hands, HouseholdStore, Pack, Refusal, SaddleAccess, SaddleSlots, Stock, StockAnswer,
    StockInbox, StockLayout, StockRefused, StockRequest, StoreAccess,
};
use veyra_sim::progression::{CharacterSheet, Reach};
use veyra_sim::ride::{RideAnswer, RideDone, RideInbox, RideRefusal, RideRequest, Riding, Stable};
use veyra_sim::wear::{self, RepairAnswer, WearInbox, WearRequest};
use veyra_sim::{LocalPlayer, Registries};

use super::card::{self, CardContext, CardParams, CardPart, ItemCard};
use super::strings::{english, grouped};
use super::weapon_diagram::{FOCUS_MARK, WeaponPictures};
use veyra_sim::view::BikeModels;

/// The screen kind.
pub const KIND: &str = "strata:stores";
/// The key that opens the pack anywhere, and closes the screen again.
pub const KEY: KeyCode = KeyCode::KeyI;
/// The key that sends the stack in the focused cell, or the one under the
/// pointer, across: what Shift and a click do, for the keys alone.
pub const SEND_KEY: KeyCode = KeyCode::KeyT;

/// The `test_id` of the panel the whole screen is.
pub const ROOT_ID: &str = "stores.root";
/// The `test_id` of the scrolling body between the chips and the footer,
/// whose visible rect is what a column must fit in to be seen unscrolled.
pub const BODY_ID: &str = "stores.body";
/// The `test_id` of the title: `Stores` at a locker, `Carried` anywhere.
pub const TITLE_ID: &str = "stores.title";
/// The `test_id` of the Close button.
pub const CLOSE_ID: &str = "stores.close";
/// The `test_id` of the household's credits in the head.
pub const PURSE_ID: &str = "stores.purse";
/// The `test_id` of the pinned card.
pub const CARD_ID: &str = "stores.card";
/// The `test_id` and `menu` tag of the button that sends the pinned stack
/// across.
pub const SEND: &str = "stores.send";
/// The `test_id` and `menu` tag of the button that takes the pinned weapon
/// in hand.
pub const TAKE: &str = "stores.take";
/// The `test_id` of the footer's line: what is carried, or why not.
pub const STATUS_ID: &str = "stores.carrying";
/// The `test_id` of the note under the card for a weapon in hand.
pub const NOTE_ID: &str = "stores.note";
/// The scrolling region the card stands in.
pub const CARD_REGION_ID: &str = "stores.card_region";
/// The `test_id` of the line under the send button that names its stack.
pub const SENDS_ID: &str = "stores.sends";
/// The `test_id` of the box round the household's store.
pub const STORE_ID: &str = "stores.store";
/// The `test_id` of the box round the pack's own cells.
pub const PACK_ID: &str = "stores.pack";
/// The `menu` tag and `test_id` prefix of a filter chip:
/// `stores.filter:mods`.
pub const FILTER_TAG: &str = "stores.filter:";

/// The `region` tag of the household's store's cells.
pub const STORE_REGION: &str = "store";
/// The `region` tag of the pack's own cells.
pub const PACK_REGION: &str = "pack";
/// The `region` tag of the bag slots.
pub const BAG_SLOT_REGION: &str = "bag_slot";
/// The `region` tag of the hands.
pub const HAND_REGION: &str = "hand";
/// The `region` tag of the saddle bags' cells.
pub const SADDLE_REGION: &str = "saddle";
/// The RIDE pane's Repair button.
pub const RIDE_REPAIR: &str = "stores.ride.repair";
/// The RIDE pane's Put away button.
pub const RIDE_PUT: &str = "stores.ride.put";
/// The line saying why Repair is grey.
pub const RIDE_REPAIR_WHY: &str = "stores.ride.repair.why";
/// The line saying why Put away is grey.
pub const RIDE_PUT_WHY: &str = "stores.ride.put.why";
/// The line saying the saddle bags are out of reach.
pub const RIDE_SHUT: &str = "stores.ride.shut";
/// The saddle bags' grid.
pub const SADDLE_ID: &str = "stores.saddle";
/// The bike's name in the RIDE pane.
pub const RIDE_NAME_ID: &str = "stores.ride.name";
/// The RIDE pane's heading.
pub const RIDE_HEAD_ID: &str = "stores.ride";

/// The pinned card's column, in UI units.
const CARD_WIDTH: f32 = 288.;
/// The width of the hands' column when no store grid sets it.
const HAND_WIDTH: f32 = 320.;
/// How much of an icon and a count is left when the filter leaves its
/// stack out. The UI blends in linear light, where the mockup's 0.22 left
/// half the brightness and the filter was hard to read; this leaves about
/// a third.
pub const DIMMED: f32 = 0.08;
/// How far in from a cell's right edge its count stands. Slotted puts it
/// two units in, where a lot's three figures run into the cell's border.
const COUNT_INSET: Val = Val::Px(5.);
/// Seconds a refusal stays at the slot it was refused at.
const FLASH_SECS: f64 = 2.6;
/// The gap between the screen's columns and its padding, in spacing steps.
const STEPS: f32 = 3.;
/// The screen's one height, as a share of the window: the panel is this
/// tall whatever is pinned, so nothing outside a scrolling region moves.
const SCREEN_HEIGHT_VH: f32 = 98.;
/// The foot of the card's column: the way across, what it sends, Take in
/// hand and the note, room for each at once.
const FOOT_HEIGHT: f32 = 176.;
/// What a panel's border takes of its width, both sides together. Every
/// slotted panel has one, drawn or not, so a column that must hold a grid
/// to the unit is sized with the boxes the grid stands in.
const BOX: f32 = 2.;

const SECTION_ROLE: &str = "strata.stores.section";
const CHIP_ROLE: &str = "strata.stores.chip";
const CHIP_ON_ROLE: &str = "strata.stores.chip.on";
const CHIP_TEXT_ROLE: &str = "strata.stores.chip.text";
const CHIP_TEXT_ON_ROLE: &str = "strata.stores.chip.text.on";
const HAND_ROLE: &str = "strata.stores.hand";
const HAND_NAME_ROLE: &str = "strata.stores.hand.name";
const HAND_STATE_ROLE: &str = "strata.stores.hand.state";
const CAPTION_ROLE: &str = "strata.stores.caption";
const REFUSAL_ROLE: &str = "strata.stores.refusal";
const LOCK_ROLE: &str = "strata.stores.lock";
const SHACKLE_ROLE: &str = "strata.stores.lock.shackle";
const STATUS_ROLE: &str = "strata.stores.status";
const CARRYING_ROLE: &str = "strata.stores.carrying";
const PURSE_ROLE: &str = "strata.purse.value";
const PURSE_LABEL_ROLE: &str = "strata.purse.label";
const PIP_ROLE: &str = "strata.slot.pip";
const PIP_FILL_ROLE: &str = "strata.slot.pip.fill";
const PIP_CAP_ROLE: &str = "strata.slot.pip.cap";
const CONDITION_ROLE: &str = "strata.slot.condition";
const CONDITION_FILL_ROLE: &str = "strata.slot.condition.fill";

/// The screen kind.
#[must_use]
pub fn kind() -> ScreenKind {
    ScreenKind::new(KIND)
}

/// The `test_id` of bag slot `bag`'s caption.
#[must_use]
pub fn bag_caption_id(bag: usize) -> String {
    format!("stores.bag.{bag}")
}

/// The `test_id` of bag slot `bag` itself.
#[must_use]
pub fn bag_slot_id(bag: usize) -> String {
    format!("stores.bag_slot.{bag}")
}

/// The `region` tag of the cells under bag slot `bag`.
#[must_use]
pub fn bag_region(bag: usize) -> String {
    format!("bag{bag}")
}

/// The `test_id` of the hand `hand`'s slot.
#[must_use]
pub fn hand_slot_id(hand: usize) -> String {
    format!("stores.hand.{hand}")
}

/// The `test_id` of the weapon's picture in hand `hand`'s row.
#[must_use]
pub fn hand_picture_id(hand: usize) -> String {
    format!("stores.hand.picture:{hand}")
}

/// The `test_id` of the line under the hand `hand`'s weapon.
#[must_use]
pub fn hand_fitted_id(hand: usize) -> String {
    format!("stores.hand.{hand}.fitted")
}

fn bag_lock_id(bag: usize) -> String {
    format!("stores.bag.{bag}.lock")
}

/// What the grids show at full strength; the rest is dimmed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Filter {
    /// Everything.
    #[default]
    All,
    /// Lots of a resource.
    Lots,
    /// Parts: what is made of lots or bought in, and goes into something
    /// else.
    Parts,
    /// Modules.
    Mods,
    /// Weapons.
    Gear,
    /// Bags.
    Bags,
    /// Production blueprints.
    Blueprints,
    /// Repair kits.
    Kits,
}

impl Filter {
    /// Every filter, in the chips' order.
    pub const ALL: [Self; 8] = [
        Self::All,
        Self::Lots,
        Self::Parts,
        Self::Mods,
        Self::Blueprints,
        Self::Gear,
        Self::Bags,
        Self::Kits,
    ];

    /// The filter's name in its chip's tag.
    #[must_use]
    pub const fn id(self) -> &'static str {
        match self {
            Self::All => "all",
            Self::Lots => "lots",
            Self::Parts => "parts",
            Self::Mods => "mods",
            Self::Gear => "gear",
            Self::Bags => "bags",
            Self::Blueprints => "blueprints",
            Self::Kits => "kits",
        }
    }

    const fn label(self) -> &'static str {
        match self {
            Self::All => "strata.stores.filter.all",
            Self::Lots => "strata.stores.filter.lots",
            Self::Parts => "strata.stores.filter.parts",
            Self::Mods => "strata.stores.filter.mods",
            Self::Gear => "strata.stores.filter.gear",
            Self::Bags => "strata.stores.filter.bags",
            Self::Blueprints => "strata.stores.filter.blueprints",
            Self::Kits => "strata.stores.filter.kits",
        }
    }

    /// Whether an item of `kind` is one the filter shows.
    #[must_use]
    pub const fn shows(self, kind: &ItemKind) -> bool {
        matches!(
            (self, kind),
            (Self::All, _)
                | (Self::Lots, ItemKind::Lot { .. })
                | (Self::Parts, ItemKind::Component | ItemKind::Commodity)
                | (Self::Mods, ItemKind::Module { .. })
                | (Self::Blueprints, ItemKind::Blueprint)
                | (Self::Kits, ItemKind::Kit)
                | (
                    Self::Gear,
                    ItemKind::Weapon { .. } | ItemKind::Vehicle { .. } | ItemKind::Harvester { .. }
                )
                | (Self::Bags, ItemKind::Bag { .. })
        )
    }

    /// The `test_id` and `menu` tag of the filter's chip.
    #[must_use]
    pub fn chip_id(self) -> String {
        format!("{FILTER_TAG}{}", self.id())
    }
}

// ---- the authority ----

/// What goes between slotted's prediction and the sim: the clicks on their
/// way out and the answers on their way back.
#[derive(Debug, Default)]
struct Wire {
    /// The menu whose clicks are the sim's to answer.
    stores: Option<MenuId>,
    /// Clicks predicted and not yet handed to the sim's inbox.
    requests: Vec<StockRequest>,
    /// Answers slotted has not polled yet.
    events: VecDeque<AuthorityEvent>,
    /// Clicks the sim has not answered.
    in_flight: u32,
}

/// Slotted's authority for the game: a click on the stores' menu becomes a
/// `StockRequest` for the sim's tick and is settled by the sim's answer;
/// a click on any other menu (the HUD's hotbar) is the prediction's own
/// truth and is acknowledged at once, as slotted's local authority does.
///
/// `submit` has no world to write to, so it is two queues under a lock that
/// systems drain: [`forward_requests`] into the sim's inbox and
/// [`feed_answers`] back out of its messages.
#[derive(Resource, Debug, Clone, Default)]
pub struct StockAuthority(Arc<Mutex<Wire>>);

impl StockAuthority {
    fn wire(&self) -> std::sync::MutexGuard<'_, Wire> {
        self.0.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Clicks sent to the sim and not yet answered.
    #[must_use]
    pub fn in_flight(&self) -> u32 {
        self.wire().in_flight
    }
}

impl slotted::model::Authority for StockAuthority {
    fn submit(
        &self,
        menu: MenuId,
        action: ClickAction,
        predicted: &Delta,
    ) -> Result<(), AuthorityError> {
        let mut wire = self.wire();
        if wire.stores == Some(menu) {
            wire.requests.push(StockRequest::Click {
                menu,
                action,
                predicted: predicted.clone(),
            });
            wire.in_flight = wire.in_flight.saturating_add(1);
        } else {
            wire.events.push_back(AuthorityEvent::Ack {
                menu,
                state_id: predicted.state_id,
            });
        }
        Ok(())
    }

    fn poll(&self) -> Vec<AuthorityEvent> {
        self.wire().events.drain(..).collect()
    }
}

/// `Update`, after slotted's submit: the clicks predicted this frame go
/// into the sim's inbox, in order, for the next tick.
pub fn forward_requests(link: Res<StockAuthority>, mut inbox: ResMut<StockInbox>) {
    let requests = std::mem::take(&mut link.wire().requests);
    if !requests.is_empty() {
        inbox.extend(requests);
    }
}

/// `Update`, before slotted's reconcile: the sim's answers become the
/// authority's events, and its refusals the words shown at the slot.
///
/// A refusal for want of a player answers no click, so every click in
/// flight is settled here and the screen, which has nobody's stock to show,
/// closes.
pub fn feed_answers(
    link: Res<StockAuthority>,
    time: Res<Time>,
    mut answers: MessageReader<StockAnswer>,
    mut refusals: MessageReader<StockRefused>,
    mut state: ResMut<StoresState>,
    mut commands: Commands,
) {
    if answers.is_empty() && refusals.is_empty() {
        return;
    }
    let mut wire = link.wire();
    for answer in answers.read() {
        wire.in_flight = wire.in_flight.saturating_sub(1);
        wire.events.push_back(match answer.clone() {
            StockAnswer::Ack { menu, state_id } => AuthorityEvent::Ack { menu, state_id },
            StockAnswer::Resync { menu, snapshot } => AuthorityEvent::Resync { menu, snapshot },
        });
    }
    for refused in refusals.read() {
        let Some(open) = state.open.as_mut().filter(|open| open.id == refused.menu) else {
            continue;
        };
        if refused.reason == Refusal::NoPlayer {
            for _ in 0..std::mem::take(&mut wire.in_flight) {
                wire.events.push_back(AuthorityEvent::Ack {
                    menu: refused.menu,
                    state_id: 0,
                });
            }
            pop_screen(&mut commands);
            continue;
        }
        if let Some(words) = refusal_words(refused.reason, &open.layout, open.access, refused.slot)
        {
            open.say(refused.slot, words, time.elapsed_secs_f64());
        }
    }
}

/// The words for a send that found no room, by the side the stack at
/// `slot` would have gone to, which is the sim's own routing: from the
/// store to the pack, from what is carried to the store, and without the
/// store between the pack and its bags.
fn no_room_key(layout: &StockLayout, access: SaddleAccess, slot: Option<SlotIx>) -> &'static str {
    let within = |range: SlotRange| slot.is_some_and(|slot| range.contains(slot));
    let in_saddle = layout.saddle.is_some_and(within);
    // Mounted, what leaves the pack or a bag goes to the saddle.
    if access == SaddleAccess::Mounted && !in_saddle && !layout.store.is_some_and(within) {
        return "strata.stores.refused.no_room.saddle";
    }
    match layout.store {
        Some(store) if within(store) => "strata.stores.refused.no_room.pack",
        Some(_) => "strata.stores.refused.no_room.store",
        None if within(layout.pack) => "strata.stores.refused.no_room.bags",
        None => "strata.stores.refused.no_room.pack",
    }
}

/// The player's words for why the sim would not do something at `slot`, or
/// `None` for a click that simply had nothing to do.
fn refusal_words(
    reason: Refusal,
    layout: &StockLayout,
    access: SaddleAccess,
    slot: Option<SlotIx>,
) -> Option<String> {
    let words = |key: &'static str| english(key).unwrap_or(key);
    Some(match reason {
        Refusal::BagHolds { stacks } => {
            words("strata.stores.refused.bag").replace("{stacks}", &stacks.to_string())
        }
        Refusal::HandFixed => words("strata.stores.refused.hand").to_owned(),
        Refusal::NotABag => words("strata.stores.refused.not_a_bag").to_owned(),
        Refusal::NoLocker => words("strata.stores.refused.no_locker").to_owned(),
        Refusal::SaddleShut => words("strata.stores.refused.saddle_shut").to_owned(),
        Refusal::NoRoom => words(no_room_key(layout, access, slot)).to_owned(),
        Refusal::NoDropping => words("strata.stores.refused.no_dropping").to_owned(),
        Refusal::Unsupported | Refusal::NotOpen | Refusal::NoPlayer | Refusal::Click(_) => {
            return None;
        }
    })
}

// ---- the state ----

/// Words shown at a slot for a few seconds in place of its caption.
#[derive(Debug, Clone, PartialEq)]
struct Flash {
    slot: SlotIx,
    words: String,
    /// When it ends, in seconds of the app's clock.
    until: f64,
}

/// A bag slot as its caption describes it.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
struct BagView {
    /// The bag's name; none for an empty slot.
    name: Option<String>,
    /// The cells it opens.
    cells: u16,
    /// The stacks in them.
    held: u32,
}

/// The nodes of the open screen that are written to while it is up, found
/// once after it spawns.
#[derive(Debug, Clone)]
struct Parts {
    card: Entity,
    send: Entity,
    send_label: Option<Entity>,
    sends: Option<Entity>,
    take: Option<Entity>,
    take_label: Option<Entity>,
    note: Entity,
    status: Entity,
    captions: [Option<Entity>; BAG_SLOTS],
    locks: [Option<Entity>; BAG_SLOTS],
    fitted: [Option<Entity>; HANDS],
    /// The RIDE pane's controls, when the household has a bike.
    ride: Option<RideParts>,
    /// Each chip and the text inside it.
    chips: Vec<(Filter, Entity, Option<Entity>)>,
}

/// The nodes of the RIDE pane that are written to while it is up.
#[derive(Debug, Clone)]
struct RideParts {
    repair: Entity,
    repair_label: Option<Entity>,
    repair_why: Entity,
    put: Entity,
    put_why: Entity,
    shut: Entity,
}

/// The open screen.
#[derive(Debug)]
struct Open {
    menu: Entity,
    id: MenuId,
    with_store: bool,
    /// The mirror inventories, in the menu's order.
    mirror: Vec<Entity>,
    /// The menu as the mirror now makes it.
    layout: StockLayout,
    /// The saddle's access the layout was last made with, since the
    /// definition depends on it as well as on the inventories.
    access: SaddleAccess,
    bags: [BagView; BAG_SLOTS],
    /// The line under each hand's weapon, to put back after a refusal.
    fitted: [String; HANDS],
    parts: Option<Parts>,
    /// The slot whose card is pinned: the last one focused or pointed at
    /// that held a stack.
    pinned: Option<Entity>,
    /// Whether the pointer has moved since the screen opened. One parked
    /// where a cell came up under it has pointed at nothing.
    pointer_moved: bool,
    /// The cell the pointer was last seen over, so the card is pinned as
    /// the pointer comes onto a cell and not for as long as it rests there.
    hovered: Option<Entity>,
    /// Whether the send button was shown when the card was last pinned.
    send_shown: bool,
    flash: Option<Flash>,
    /// Words for the footer's line that belong at no slot, and when they
    /// end.
    notice: Option<(String, f64)>,
}

impl Open {
    /// Whether a refusal at `slot` has a caption of its own to be said in.
    fn says_at(&self, slot: SlotIx) -> bool {
        self.layout.bag_slots.contains(slot) || self.layout.hands.contains(slot)
    }

    /// Shows `words` for a refusal at `slot` from `now` on: in the slot's
    /// own caption where it has one, else in the footer's line, which is
    /// the nearest a cell of a grid has to words of its own.
    fn say(&mut self, slot: Option<SlotIx>, words: String, now: f64) {
        let until = now + FLASH_SECS;
        match slot.filter(|slot| self.says_at(*slot)) {
            Some(slot) => self.flash = Some(Flash { slot, words, until }),
            None => self.notice = Some((words, until)),
        }
    }
}

/// The stores screen's state: whether it is open, over what, and the
/// filter.
#[derive(Resource, Debug, Default)]
pub struct StoresState {
    open: Option<Open>,
    /// An open asked for while the sim had yet to hear that the last
    /// screen closed, and whether it was over the store: see
    /// [`open_pending`].
    pending: Option<bool>,
    /// What the grids show at full strength.
    pub filter: Filter,
    /// A filter asked for from outside the screen ([`show_only`],
    /// [`show_nth`]), put in force once the cells exist with one of the
    /// stacks it shows pinned: the first, or the one so many after it.
    wanted: Option<(Filter, usize)>,
    /// Repairs of the bike asked for by this screen and not yet answered:
    /// a repair answer is shared with the fitting screen's, which is not
    /// this screen's to react to.
    bike_repairs: u32,
}

impl StoresState {
    /// The open screen's menu entity.
    #[must_use]
    pub fn menu(&self) -> Option<Entity> {
        self.open.as_ref().map(|open| open.menu)
    }

    /// Whether the open screen reaches the household's store; `None` while
    /// no screen is open.
    #[must_use]
    pub fn with_store(&self) -> Option<bool> {
        self.open.as_ref().map(|open| open.with_store)
    }

    /// The slot whose card is pinned.
    #[must_use]
    pub fn pinned(&self) -> Option<Entity> {
        self.open.as_ref().and_then(|open| open.pinned)
    }
}

/// On a mirror inventory, so a change to one is told from a change to any
/// other inventory in the world.
#[derive(Component, Debug, Clone, Copy, Default)]
pub struct StockMirror;

/// The stock as the open screen's mirror holds it, in the sim's own types,
/// so the sim's own functions can be asked about it.
struct Mirrored {
    pack: Pack,
    hands: Hands,
    saddle: slotted::model::Inventory,
    store: Option<HouseholdStore>,
}

/// The saddle as the menu declares it for a player who is `riding` or
/// whose store is reached: nothing without a bike, whose saddle has cells.
fn saddle_slots(
    saddle: &slotted::model::Inventory,
    riding: bool,
    store_reached: bool,
) -> Option<SaddleSlots> {
    let cells = u16::try_from(saddle.len())
        .ok()
        .filter(|cells| *cells > 0)?;
    Some(SaddleSlots {
        cells,
        access: SaddleAccess::of(riding, store_reached),
    })
}

fn mirrored(
    open: &Open,
    carried: Option<ItemStack>,
    inventories: &Query<&Inventory>,
) -> Option<Mirrored> {
    let read = |handle: slotted::model::InventoryRef| {
        let entity = open.mirror.get(handle.index())?;
        Some(inventories.get(*entity).ok()?.0.clone())
    };
    let mut bags = Vec::with_capacity(BAG_SLOTS);
    for bag in 0..BAG_SLOTS {
        bags.push(read(StockLayout::bag(bag))?);
    }
    Some(Mirrored {
        pack: Pack {
            cells: read(StockLayout::PACK)?,
            bag_slots: read(StockLayout::BAG_SLOTS)?,
            bags: bags.try_into().ok()?,
            cursor: carried,
        },
        hands: Hands(read(StockLayout::HANDS)?),
        saddle: read(StockLayout::SADDLE)?,
        store: if open.with_store {
            Some(HouseholdStore(read(StockLayout::STORE)?))
        } else {
            None
        },
    })
}

fn bag_views(pack: &Pack, items: &ItemCatalogue) -> [BagView; BAG_SLOTS] {
    std::array::from_fn(|bag| BagView {
        name: pack
            .bag_slots
            .get(bag)
            .and_then(|stack| items.def_of(stack.id))
            .map(|def| def.name.clone()),
        cells: pack.open_cells(bag, items),
        held: pack.held_in(bag),
    })
}

// ---- the definition ----

/// One hand as the screen shows it.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct HandRow {
    /// The weapon's name; none for an empty hand.
    pub name: Option<String>,
    /// What is fitted to it, in words.
    pub fitted: String,
    /// Its condition, nought to a thousand.
    pub condition: Option<u16>,
    /// Whether it is the one drawn.
    pub in_hand: bool,
    /// The weapon's baked picture as an asset path, and its width and
    /// height in pixels: a long gun is a thin line in its square cell, so
    /// the row draws it at its own shape as well. None with nothing baked.
    pub picture: Option<(String, u32, u32)>,
}

/// The height a hand's row draws its weapon at, and the widest it lets it
/// be, in UI units.
const HAND_PICTURE_HEIGHT: f32 = 34.;
const HAND_PICTURE_WIDTH: f32 = 120.;

/// Everything the definition is made from.
#[derive(Debug, Clone)]
pub struct StoresView<'a> {
    /// Whether the household's store is shown beside the pack.
    pub with_store: bool,
    /// Whose pack it is.
    pub who: &'a str,
    /// The carrying rules, which size every grid.
    pub carry: &'a CarryRules,
    /// Where each part of the menu is.
    pub layout: &'a StockLayout,
    /// The hands, in the menu's order.
    pub hands: [HandRow; HANDS],
    /// A cell's edge and the gap between cells, from the theme.
    pub cell: (f32, f32),
    /// The household's credits, shown at the right of the head.
    pub purse: u32,
    /// The RIDE pane, when the household has a bike.
    pub ride: Option<RideRow>,
}

/// The RIDE pane as the screen shows it: the bike and where its saddle
/// bags' cells are in the menu.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RideRow {
    /// The bike's name.
    pub name: String,
    /// Its condition, nought to a thousand.
    pub condition: Option<u16>,
    /// The saddle bags' columns and rows.
    pub grid: (u16, u16),
}

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

fn section(label: &str) -> UiNodeDef {
    text_in(label, TextRole::Caption, SECTION_ROLE)
}

/// A grid's width: its cells and the gaps between them.
fn grid_width(cols: u16, cell: (f32, f32)) -> f32 {
    let cols = f32::from(cols);
    cols * cell.0 + (cols - 1.).max(0.) * cell.1
}

/// A grid of cells in a box of its own, which carries the `test_id`: a
/// grid hands its tags to every cell, and an id is one node's.
fn grid(
    id: &str,
    region: &str,
    inventory: slotted::model::InventoryRef,
    cols: u16,
    rows: u16,
    first: u16,
) -> UiNodeDef {
    with_id(
        panel(
            "invisible",
            column(0.),
            vec![UiNodeDef::SlotGrid {
                inventory,
                cols,
                rows,
                first,
                tags: Tags::new().with("region", region),
            }],
        ),
        id,
    )
}

fn slot(index: u16, region: &str, id: &str) -> UiNodeDef {
    UiNodeDef::Slot {
        slot: SlotIx(index),
        tags: Tags::new().with("region", region).with(Tags::TEST_ID, id),
    }
}

/// The mark beside a bag that will not come out: a small padlock drawn
/// from two boxes, since neither typeface has the glyph. Shown by
/// [`show_bags`] while the bag holds anything.
fn lock_mark(bag: usize) -> UiNodeDef {
    let piece = |role: &str, width: f32, height: f32| {
        panel(
            role,
            Layout {
                width: Some(Length::Px(width)),
                height: Some(Length::Px(height)),
                ..column(0.)
            },
            Vec::new(),
        )
    };
    with_id(
        panel(
            "invisible",
            Layout {
                align: Some(Align::Center),
                ..column(0.)
            },
            vec![piece(SHACKLE_ROLE, 8., 6.), piece(LOCK_ROLE, 11., 7.)],
        ),
        &bag_lock_id(bag),
    )
}

/// One bag slot with its caption and, under the caption, the cells the bag
/// in it opens. The grid is declared at the most a bag can open, because a
/// grid does not grow; [`show_bags`] leaves out the cells this bag does
/// not.
fn bag_row(view: &StoresView<'_>, bag: usize) -> UiNodeDef {
    let cols = view.carry.pack.cols.max(1);
    let rows = view.carry.bag_cells.div_ceil(cols);
    let slots = view.layout.bag_slots;
    let cells = view.layout.bags[bag];
    let index = u16::try_from(bag).unwrap_or(u16::MAX);
    panel(
        "invisible",
        Layout {
            align: Some(Align::Start),
            ..row(1.)
        },
        vec![
            slot(
                slots.start.saturating_add(index),
                BAG_SLOT_REGION,
                &bag_slot_id(bag),
            ),
            panel(
                "invisible",
                Layout {
                    // The grid's own box, and this one round it.
                    width: Some(Length::Px(grid_width(cols, view.cell) + 2. * BOX)),
                    ..column(0.7)
                },
                vec![
                    panel(
                        "invisible",
                        Layout {
                            min_height: Some(Length::Px(18.)),
                            ..row(1.)
                        },
                        vec![
                            lock_mark(bag),
                            // One line, never wrapped. The layout asks a
                            // text that may wrap how tall it is at its
                            // narrowest, a word a line, while it sizes the
                            // row, and keeps that height for the column:
                            // the next bag slot then stands a dozen lines
                            // further down than the cells above it end.
                            with_id(
                                UiNodeDef::Text {
                                    key: key("strata.stores.bag.empty"),
                                    style: TextRole::Caption,
                                    opts: TextOpts {
                                        wrap: false,
                                        role: Some(Role::new(CAPTION_ROLE)),
                                        ..TextOpts::default()
                                    },
                                    tags: Tags::new(),
                                },
                                &bag_caption_id(bag),
                            ),
                        ],
                    ),
                    grid(
                        &format!("stores.bag.{bag}.cells"),
                        &bag_region(bag),
                        StockLayout::bag(bag),
                        cols,
                        rows,
                        cells.start,
                    ),
                ],
            ),
        ],
    )
}

/// One hand: the weapon's cell, its name over what is fitted, and at the
/// right which hand it is and its condition.
fn hand_row(view: &StoresView<'_>, hand: usize) -> UiNodeDef {
    let row_of = &view.hands[hand];
    let index = u16::try_from(hand).unwrap_or(u16::MAX);
    let name = row_of
        .name
        .clone()
        .unwrap_or_else(|| "strata.stores.hand.empty".to_owned());
    let mut right = vec![text_in(
        if row_of.in_hand {
            "strata.stores.hand.in_hand"
        } else {
            "strata.stores.hand.holstered"
        },
        TextRole::Caption,
        if row_of.in_hand {
            HAND_STATE_ROLE
        } else {
            SECTION_ROLE
        },
    )];
    if let Some(condition) = row_of.condition {
        right.push(text_in(
            english("strata.stores.hand.condition")
                .unwrap_or_default()
                .replace("{condition}", &condition.to_string()),
            TextRole::Caption,
            SECTION_ROLE,
        ));
    }
    let mut children = vec![
        slot(
            view.layout.hands.start.saturating_add(index),
            HAND_REGION,
            &hand_slot_id(hand),
        ),
        panel(
            "invisible",
            Layout {
                grow: 1.,
                min_width: Some(Length::Px(0.)),
                ..column(0.2)
            },
            vec![
                text_in(name, TextRole::Body, HAND_NAME_ROLE),
                with_id(
                    text_in(row_of.fitted.clone(), TextRole::Caption, CAPTION_ROLE),
                    &hand_fitted_id(hand),
                ),
            ],
        ),
    ];
    // Beside the store the hands have a wide column; on the pack's own
    // screen theirs is narrow, and a picture there would wrap the name.
    let pictured = row_of.picture.as_ref().filter(|_| view.with_store);
    children.extend(pictured.map(|(path, wide, high)| {
        // Fitted into the row's height, and into the widest a row gives
        // it: a carbine is held by its length and a pistol by its height.
        let (wide, high) = (f64::from(*wide).max(1.), f64::from(*high).max(1.));
        let scale =
            (f64::from(HAND_PICTURE_HEIGHT) / high).min(f64::from(HAND_PICTURE_WIDTH) / wide);
        #[allow(
            clippy::cast_possible_truncation,
            reason = "a picture's side in UI units, a few dozen"
        )]
        let side = |pixels: f64| Length::Px((pixels * scale) as f32);
        UiNodeDef::Image {
            path: path.clone(),
            width: side(wide),
            height: side(high),
            tags: Tags::new().with(Tags::TEST_ID, &hand_picture_id(hand)),
        }
    }));
    children.push(panel(
        "invisible",
        Layout {
            align: Some(Align::End),
            ..column(0.2)
        },
        right,
    ));
    panel(
        HAND_ROLE,
        Layout {
            width: Some(Length::Percent(100.)),
            padding: Padding {
                top: 0.8,
                right: 2.,
                bottom: 0.8,
                left: 1.,
            },
            ..row(1.5)
        },
        children,
    )
}

/// A button of the RIDE pane: a menu button with a test id, grey or live
/// by [`show_ride`] once it is up.
fn ride_button(label: &str, id: &str) -> UiNodeDef {
    UiNodeDef::Button {
        widget: None,
        opts: ButtonOpts {
            label: Some(key(label)),
            ..ButtonOpts::default()
        },
        tags: Tags::new().with("menu", id).with(Tags::TEST_ID, id),
    }
}

/// The RIDE pane: the bike's name and condition, Repair and Put away each
/// with its reason under it when grey, and the saddle bags' grid, drawn
/// shut with a line when it is out of reach.
#[allow(
    clippy::too_many_lines,
    reason = "one pane, read top to bottom as the screen is"
)]
fn ride_pane(view: &StoresView<'_>, ride: &RideRow, compact: bool) -> UiNodeDef {
    let mut name = vec![with_id(
        text_in(ride.name.clone(), TextRole::Body, HAND_NAME_ROLE),
        RIDE_NAME_ID,
    )];
    if let Some(condition) = ride.condition {
        name.push(text_in(
            english("strata.stores.hand.condition")
                .unwrap_or_default()
                .replace("{condition}", &condition.to_string()),
            TextRole::Caption,
            SECTION_ROLE,
        ));
    }
    let why_of = |why: &str| with_id(text_in("", TextRole::Caption, CAPTION_ROLE), why);
    let control = |label: &str, id: &str, why: &str| {
        let mut parts = vec![ride_button(label, id)];
        if !compact {
            parts.push(why_of(why));
        }
        panel(
            "invisible",
            Layout {
                align: Some(Align::Stretch),
                grow: 1.,
                min_width: Some(Length::Px(0.)),
                ..column(0.4)
            },
            parts,
        )
    };
    let name_box = panel(
        HAND_ROLE,
        Layout {
            width: if compact {
                None
            } else {
                Some(Length::Percent(100.))
            },
            grow: if compact { 1.6 } else { 0. },
            min_width: Some(Length::Px(0.)),
            padding: Padding::all(0.8),
            ..column(0.2)
        },
        name,
    );
    let repair = control("strata.stores.ride.repair", RIDE_REPAIR, RIDE_REPAIR_WHY);
    let put = control("strata.stores.ride.put", RIDE_PUT, RIDE_PUT_WHY);
    // Beside the store the middle column has the height for the bike's
    // name and its two buttons on one line, their reasons in one line each
    // under it, which keeps the grid on screen.
    let head = if compact {
        vec![
            panel(
                "invisible",
                Layout {
                    align: Some(Align::Stretch),
                    ..row(1.)
                },
                vec![name_box, repair, put],
            ),
            // Side by side, each under its own button's half, so the pane
            // spends one line on its reasons and not two.
            panel(
                "invisible",
                Layout {
                    align: Some(Align::Start),
                    ..row(1.)
                },
                vec![
                    panel(
                        "invisible",
                        Layout {
                            grow: 1.,
                            min_width: Some(Length::Px(0.)),
                            ..column(0.)
                        },
                        vec![why_of(RIDE_REPAIR_WHY)],
                    ),
                    panel(
                        "invisible",
                        Layout {
                            grow: 1.,
                            min_width: Some(Length::Px(0.)),
                            ..column(0.)
                        },
                        vec![why_of(RIDE_PUT_WHY)],
                    ),
                ],
            ),
        ]
    } else {
        vec![name_box, panel("invisible", row(1.), vec![repair, put])]
    };
    // The name box says what the pane is, and beside the store there is no
    // height to spare for a heading over it.
    let mut nodes = Vec::new();
    if !compact {
        nodes.push(with_id(section("strata.stores.ride"), RIDE_HEAD_ID));
    }
    nodes.extend(head);
    if !compact {
        nodes.push(section("strata.stores.saddle"));
    }
    nodes.push(grid(
        SADDLE_ID,
        SADDLE_REGION,
        StockLayout::SADDLE,
        ride.grid.0,
        ride.grid.1,
        view.layout.saddle.map_or(0, |range| range.start),
    ));
    nodes.push(with_id(
        text_in("strata.stores.saddle.shut", TextRole::Caption, CAPTION_ROLE),
        RIDE_SHUT,
    ));
    panel("invisible", column(if compact { 0.3 } else { 1. }), nodes)
}

/// A chip's words: its filter's, with the mark before them on the one in
/// force, so which filter is on is read in a word's place as well as in
/// the pill's fill.
fn chip_words(filter: Filter, on: bool) -> String {
    let label = english(filter.label()).unwrap_or(filter.label());
    if on {
        format!("{FOCUS_MARK}{label}")
    } else {
        label.to_owned()
    }
}

/// A filter chip: a pill that is the button, lit and marked when it is the
/// filter in force. [`paint_chips`] moves the light and the mark without a
/// rebuild.
fn chip(filter: Filter, on: bool) -> UiNodeDef {
    let id = filter.chip_id();
    UiNodeDef::Card {
        role: Role::new(if on { CHIP_ON_ROLE } else { CHIP_ROLE }),
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
            chip_words(filter, on),
            TextRole::Caption,
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

/// The household's credits as a screen's head shows them: the figure with
/// its thousands apart under `id`, and the word beside it. One line each,
/// so neither grows the head.
pub(crate) fn purse(id: &str, credits: u32) -> UiNodeDef {
    let line = |words: String, role: &str| UiNodeDef::Text {
        key: key(words),
        style: TextRole::Caption,
        opts: TextOpts {
            wrap: false,
            role: Some(Role::new(role)),
            ..TextOpts::default()
        },
        tags: Tags::new(),
    };
    panel(
        "invisible",
        Layout {
            align: Some(Align::End),
            ..row(0.8)
        },
        vec![
            with_id(line(grouped(credits), PURSE_ROLE), id),
            line("strata.purse.label".to_owned(), PURSE_LABEL_ROLE),
        ],
    )
}

/// The whole screen for `view`.
#[must_use]
#[allow(
    clippy::too_many_lines,
    reason = "one definition, read top to bottom as the screen is"
)]
pub fn screen_def(view: &StoresView<'_>) -> ScreenDef {
    let carry = view.carry;
    let store_width = grid_width(carry.store.cols, view.cell);
    let pack_width = grid_width(carry.pack.cols, view.cell);
    let left_width = if view.with_store {
        store_width
    } else {
        HAND_WIDTH
    };
    // The bag slot stands left of its cells, so the middle column is a
    // cell and a gap wider than the pack's own grid, and wider again by
    // the four boxes a bag's cells stand in: the column's, the row's, the
    // caption's and the grid's. A unit short and the row squeezes the bag
    // slot to make room, which puts its cells out of line with the pack's.
    let middle_width = view.cell.0 + view.cell.1 + pack_width + 4. * BOX;

    let head = panel(
        "invisible",
        Layout {
            align: Some(Align::End),
            ..row(2.)
        },
        vec![
            with_id(
                text_in(
                    if view.with_store {
                        "strata.stores.title"
                    } else {
                        "strata.stores.title.pack"
                    },
                    TextRole::Title,
                    "panel.title",
                ),
                TITLE_ID,
            ),
            text_in(
                if view.with_store {
                    english("strata.stores.sub").unwrap_or_default().to_owned()
                } else {
                    english("strata.stores.sub.pack")
                        .unwrap_or_default()
                        .replace("{name}", view.who)
                },
                TextRole::Caption,
                "text.caption",
            ),
            spacer(),
            purse(PURSE_ID, view.purse),
        ],
    );
    let chips = panel(
        "invisible",
        row(1.),
        Filter::ALL
            .into_iter()
            .map(|filter| chip(filter, filter == Filter::All))
            .collect(),
    );

    let mut left = Vec::new();
    if let Some(store) = view.layout.store {
        left.push(section("strata.stores.store"));
        left.push(grid(
            STORE_ID,
            STORE_REGION,
            StockLayout::STORE,
            carry.store.cols,
            carry.store.rows,
            store.start,
        ));
    }
    left.push(section("strata.stores.hands"));
    left.extend((0..HANDS).map(|hand| hand_row(view, hand)));
    if let Some(ride) = view.ride.as_ref().filter(|_| !view.with_store) {
        left.push(ride_pane(view, ride, false));
    }
    // The pack's cells start a cell and a gap in, under which the cells a
    // bag opens stand to the right of its slot: one grid to the eye, the
    // bags' rows continuing the pack's columns. The pack's grid is put in
    // the same boxes a bag's cells are in ([`bag_row`]), a row and a column
    // round the grid's own, so the two start at the same unit. The pack's
    // heading is in that column with it and so starts where its cells do:
    // at the column's edge it stood over the cell's width that is left
    // clear for the bags' slots, with nothing under it.
    let mut middle = vec![
        panel(
            "invisible",
            Layout {
                align: Some(Align::Start),
                ..row(1.)
            },
            vec![
                UiNodeDef::Spacer {
                    size: Length::Px(view.cell.0),
                    tags: Tags::new(),
                },
                panel(
                    "invisible",
                    column(1.),
                    vec![
                        section("strata.stores.pack"),
                        grid(
                            PACK_ID,
                            PACK_REGION,
                            StockLayout::PACK,
                            carry.pack.cols,
                            carry.pack.rows,
                            view.layout.pack.start,
                        ),
                    ],
                ),
            ],
        ),
        section("strata.stores.bags"),
    ];
    middle.extend((0..usize::from(carry.bag_slots).min(BAG_SLOTS)).map(|bag| bag_row(view, bag)));
    if let Some(ride) = view.ride.as_ref().filter(|_| view.with_store) {
        middle.push(ride_pane(view, ride, true));
    }
    // The card scrolls in the room that is left and the way across stands
    // in a foot of one height, all four parts always laid out, so a short
    // card or a tall one moves nothing but its own text.
    let card_region = UiNodeDef::Scroll {
        layout: Layout {
            // A definite width: left to size by content, the viewport gives
            // the card's label column no room and the labels vanish.
            width: Some(Length::Px(CARD_WIDTH)),
            grow: 1.,
            min_height: Some(Length::Px(0.)),
            align: Some(Align::Stretch),
            ..column(0.)
        },
        scrollbar: true,
        children: vec![CardParams::empty().framed().node(Some(CARD_ID))],
        tags: Tags::new().with(Tags::TEST_ID, CARD_REGION_ID),
    };
    let foot = panel(
        "invisible",
        Layout {
            height: Some(Length::Px(FOOT_HEIGHT)),
            min_height: Some(Length::Px(FOOT_HEIGHT)),
            max_height: Some(Length::Px(FOOT_HEIGHT)),
            align: Some(Align::Stretch),
            ..column(1.)
        },
        vec![
            UiNodeDef::Button {
                widget: None,
                opts: ButtonOpts {
                    label: Some(key("strata.stores.send.pack")),
                    ..ButtonOpts::default()
                },
                tags: Tags::new().with("menu", SEND).with(Tags::TEST_ID, SEND),
            },
            with_id(
                text_in("strata.stores.sends", TextRole::Caption, CAPTION_ROLE),
                SENDS_ID,
            ),
            UiNodeDef::Button {
                widget: None,
                opts: ButtonOpts {
                    label: Some(key("strata.stores.take")),
                    ..ButtonOpts::default()
                },
                tags: Tags::new().with("menu", TAKE).with(Tags::TEST_ID, TAKE),
            },
            with_id(
                text_in(
                    "strata.stores.hand.note",
                    TextRole::Caption,
                    "strata.card.note",
                ),
                NOTE_ID,
            ),
        ],
    );
    let right = vec![card_region, foot];
    let sized = |width: f32, fill: bool, children: Vec<UiNodeDef>| {
        panel(
            "invisible",
            Layout {
                width: Some(Length::Px(width)),
                height: fill.then_some(Length::Percent(100.)),
                align: Some(Align::Stretch),
                ..column(1.)
            },
            children,
        )
    };
    // The columns scroll inside the panel on a window too short for them,
    // so the footer and its Close stay inside the border.
    let body = panel(
        "invisible",
        Layout {
            align: Some(Align::Start),
            grow: 1.,
            min_height: Some(Length::Px(0.)),
            overflow: Overflow::Scroll,
            ..row(STEPS)
        },
        vec![
            sized(left_width, false, left),
            sized(middle_width, false, middle),
            sized(CARD_WIDTH, true, right),
        ],
    );
    let body = with_id(body, BODY_ID);
    let footer = panel(
        "invisible",
        row(2.),
        vec![
            with_id(
                text_in(
                    if view.with_store {
                        "strata.stores.hint"
                    } else {
                        "strata.stores.hint.pack"
                    },
                    TextRole::Caption,
                    STATUS_ROLE,
                ),
                STATUS_ID,
            ),
            spacer(),
            UiNodeDef::Custom {
                kind: WidgetKind::new("slotted:hint_bar"),
                params: slotted::registry::Value::Unit,
                children: Vec::new(),
                tags: Tags::new().with(Tags::TEST_ID, "stores.hints"),
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
    // As wide as its columns, their gaps and its own padding: a spacing
    // step is the theme's small spacing, which is also the gap between
    // cells.
    let step = view.cell.1;
    let width = left_width + middle_width + CARD_WIDTH + 4. * STEPS * step + 2.;
    ScreenDef {
        kind: kind(),
        inherits: None,
        root: UiNodeDef::Panel {
            role: Role::new("panel"),
            layout: Layout {
                width: Some(Length::Px(width)),
                max_width: Some(Length::Percent(96.)),
                // Near the whole window and a step tighter than the other
                // screens, so a 720p window holds the RIDE pane's saddle
                // beside the store with no scrolling.
                min_height: Some(Length::Percent(SCREEN_HEIGHT_VH)),
                max_height: Some(Length::Percent(SCREEN_HEIGHT_VH)),
                padding: Padding::all(STEPS - 1.5),
                align: Some(Align::Stretch),
                justify: Justify::Start,
                ..column(2.)
            },
            children: vec![head, chips, body, footer],
            tags: Tags::new().with(Tags::TEST_ID, ROOT_ID),
        },
        listring: Vec::new(),
        remove: Vec::new(),
        initial_focus: Some(if view.with_store { STORE_ID } else { PACK_ID }.to_owned()),
        presentation: Presentation {
            mode: PresentationMode::Page,
            scrim: Some(true),
            transition: Transition::Fade,
            wildcards: Some(false),
            ..Presentation::default()
        },
    }
}

// ---- open and close ----

fn on_top(stack: &ScreenStack) -> bool {
    stack.top().is_some_and(|top| top.kind == kind())
}

/// What is fitted to a weapon, in the line under its name.
fn fitted_words(items: &ItemCatalogue, stack: &ItemStack) -> String {
    let fitted: Vec<String> = items::read(stack, items)
        .map(|record| {
            record
                .fitted
                .values()
                .filter_map(|fitted| items.get(&fitted.item))
                .map(|def| def.name.clone())
                .collect()
        })
        .unwrap_or_default();
    if fitted.is_empty() {
        english("strata.card.fitted.none")
            .unwrap_or_default()
            .to_owned()
    } else {
        fitted.join(" · ")
    }
}

/// The hands as the screen shows them; `drawn` is the weapon out, which
/// the hands are ordered by.
fn hand_rows(
    items: &ItemCatalogue,
    hands: &Hands,
    drawn: Option<Weapon>,
    pictures: Option<&PictureSet>,
) -> [HandRow; HANDS] {
    std::array::from_fn(|hand| {
        let stack = hands.0.get(hand);
        HandRow {
            picture: stack
                .and_then(|stack| items.def_of(stack.id))
                .zip(pictures)
                .and_then(|(def, pictures)| card::weapon_picture(pictures, def)),
            name: stack
                .and_then(|stack| items.def_of(stack.id))
                .map(|def| def.name.clone()),
            fitted: stack.map_or_else(String::new, |stack| fitted_words(items, stack)),
            condition: stack
                .and_then(|stack| items::read(stack, items).ok())
                .and_then(|record| record.condition),
            in_hand: drawn.is_some()
                && stack
                    .and_then(|stack| items::read(stack, items).ok())
                    .and_then(|record| veyra_sim::fitting::weapon_of(items, &record))
                    == drawn,
        }
    })
}

/// The mirror of a stock, in the menu's order: the sim's inventories
/// copied, for slotted to predict on.
fn spawn_mirror(
    world: &mut World,
    pack: &Pack,
    hands: &Hands,
    saddle: slotted::model::Inventory,
    store: Option<HouseholdStore>,
) -> Vec<Entity> {
    let mut held = vec![pack.cells.clone(), pack.bag_slots.clone()];
    held.extend(pack.bags.iter().cloned());
    held.push(hands.0.clone());
    held.push(saddle);
    held.extend(store.map(|store| store.0));
    held.into_iter()
        .map(|inventory| world.spawn((Inventory(inventory), StockMirror)).id())
        .collect()
}

/// Opens the screen over the local player's stock: with the household's
/// store beside the pack (`with_store`, a locker's F), or the pack alone.
/// Does nothing while it is already open.
pub fn open(commands: &mut Commands, with_store: bool) {
    commands.queue(move |world: &mut World| open_now(world, with_store));
}

/// Whether the store is to hand, by the sim's own rule.
fn store_reached(world: &World) -> bool {
    let access = world
        .get_resource::<StoreAccess>()
        .copied()
        .unwrap_or_default();
    world
        .get_resource::<Reach>()
        .is_some_and(|reach| items::at_locker(access, reach))
}

/// The RIDE pane's row for the household's stabled bike; none without one.
fn ride_row_of(stable: &Stable, registries: &strata_registry::GameRegistries) -> Option<RideRow> {
    let bike = stable.bike.as_ref()?;
    let def = registries.items.get(&bike.item)?;
    let cargo = veyra_sim::ride::vehicle_of(&registries.items, &registries.vehicles, bike)
        .map(|vehicle| (vehicle.cargo.columns, vehicle.cargo.rows))?;
    Some(RideRow {
        name: def.name.clone(),
        condition: bike.condition,
        grid: cargo,
    })
}

#[allow(clippy::too_many_lines, reason = "one open, read top to bottom")]
fn open_now(world: &mut World, with_store: bool) {
    if world.resource::<StoresState>().open.is_some() {
        world.resource_mut::<StoresState>().pending = None;
        return;
    }
    // A screen that closed since the last tick has left its close in the
    // inbox, and the sim has yet to put its cursor's stack away. A mirror
    // copied now would show that stack as still carried, so the open waits
    // for the tick.
    let closing = world
        .resource::<StockInbox>()
        .iter()
        .any(|request| matches!(request, StockRequest::Close { .. }));
    world.resource_mut::<StoresState>().pending = closing.then_some(with_store);
    if closing {
        return;
    }
    let Some(registries) = world.get_resource::<Registries>().map(|r| r.0.clone()) else {
        tracing::warn!("no registries: nothing to show in the stores");
        return;
    };
    let items = &registries.items;
    let mut players = world
        .query_filtered::<(&Pack, &Hands, Option<&Combatant>, Option<&Name>), With<LocalPlayer>>();
    let Ok((pack, hands, actor, name)) = players.single(world) else {
        tracing::warn!("no local player carrying anything: nothing to show in the stores");
        return;
    };
    let (pack, hands) = (pack.clone(), hands.clone());
    let drawn = actor.map(|actor| actor.weapon);
    let who = name.map_or_else(
        || {
            english("strata.vitals.fallback_name")
                .unwrap_or_default()
                .to_owned()
        },
        |name| name.as_str().to_owned(),
    );
    let store = with_store.then(|| world.resource::<HouseholdStore>().clone());
    let saddle = world.resource::<Stable>().saddle.clone();
    let ride_row = ride_row_of(world.resource::<Stable>(), &registries);
    let riding = world
        .query_filtered::<(), (With<LocalPlayer>, With<Riding>)>()
        .iter(world)
        .next()
        .is_some();
    let reached = with_store && store_reached(world);
    let layout = items::layout(
        items.carry(),
        items,
        &pack,
        with_store,
        saddle_slots(&saddle, riding, reached),
    );

    let mirror = spawn_mirror(world, &pack, &hands, saddle, store);
    let def = Arc::new(layout.def.clone());
    let menu = world.resource_scope(|world, mut ids: Mut<MenuIdAllocator>| {
        open_menu(
            &mut world.commands(),
            &mut ids,
            def,
            mirror.clone(),
            Actor::SURVIVAL,
        )
    });
    world.flush();
    let Some(id) = world.get_mut::<OpenMenu>(menu).map(|mut open| {
        // What found no room when the screen last closed is still on the
        // cursor, and is carried again.
        open.state.carried.clone_from(&pack.cursor);
        open.id
    }) else {
        return;
    };
    if let Some(mut carried) = world.get_mut::<Carried>(menu) {
        carried.0.clone_from(&pack.cursor);
    }
    world.resource::<StockAuthority>().wire().stores = Some(id);
    world.resource_mut::<StockInbox>().push(StockRequest::Open {
        menu: id,
        with_store,
    });

    // A picture is named only where there is a source to load it from.
    let pictures = world
        .get_resource::<AssetServer>()
        .filter(|assets| assets.get_source(card::GENERATED_SOURCE).is_ok())
        .and(world.get_resource::<WeaponPictures>())
        .map(|pictures| &pictures.0);
    let hand_rows = hand_rows(items, &hands, drawn, pictures);
    let tokens = active_tokens(world);
    let def = screen_def(&StoresView {
        with_store,
        who: &who,
        carry: items.carry(),
        layout: &layout,
        hands: hand_rows.clone(),
        cell: (tokens.sizes.slot_size, tokens.slot_gap()),
        purse: world.get_resource::<Purse>().map_or(0, |purse| purse.0),
        ride: ride_row,
    });
    let mut state = world.resource_mut::<StoresState>();
    state.filter = Filter::All;
    state.open = Some(Open {
        menu,
        id,
        with_store,
        mirror,
        bags: bag_views(&pack, items),
        layout,
        access: SaddleAccess::of(riding, reached),
        fitted: hand_rows.map(|row| row.fitted),
        parts: None,
        pinned: None,
        pointer_moved: false,
        hovered: None,
        send_shown: false,
        flash: None,
        notice: None,
    });
    push_screen(&mut world.commands(), Arc::new(def), Some(menu));
}

/// Puts `filter` in force on the stores screen, open or about to be, and
/// pins the first stack it shows: for a still of one kind of stock, which
/// a player gets by the chip and the arrows.
pub fn show_only(commands: &mut Commands, filter: Filter) {
    show_nth(commands, filter, 0);
}

/// [`show_only`] with the stack pinned named: the one `nth` after the
/// first the filter shows, in the order of the screen's cells, or the last
/// when there are fewer. For a still of a stack that is not the first of
/// its kind, as what a fabricator made is not.
pub fn show_nth(commands: &mut Commands, filter: Filter, nth: usize) {
    commands.queue(move |world: &mut World| {
        world.resource_mut::<StoresState>().wanted = Some((filter, nth));
    });
}

/// `Update`, before the card is pinned: a filter asked for by [`show_only`]
/// or [`show_nth`] comes into force once the screen's cells are there, and
/// the focus goes to the stack asked for, which pins that stack's card.
pub fn apply_wanted(
    registries: Option<Res<Registries>>,
    mut focus: Option<ResMut<InputFocus>>,
    mut state: ResMut<StoresState>,
    slots: Query<(Entity, &SlotRef, &ItemView)>,
) {
    let (Some((filter, nth)), Some(registries)) = (state.wanted, registries) else {
        return;
    };
    let Some(menu) = state
        .open
        .as_ref()
        .filter(|open| open.parts.is_some())
        .map(|open| open.menu)
    else {
        return;
    };
    let items = &registries.0.items;
    let mut shown: Vec<_> = slots
        .iter()
        .filter(|(_, slot, view)| {
            slot.menu == menu
                && view
                    .stack
                    .as_ref()
                    .and_then(|stack| items.def_of(stack.id))
                    .is_some_and(|def| filter.shows(&def.kind))
        })
        .map(|(entity, slot, _)| (slot.slot.0, entity))
        .collect();
    shown.sort_unstable_by_key(|(slot, _)| *slot);
    let first = shown
        .get(nth.min(shown.len().saturating_sub(1)))
        .map(|(_, entity)| *entity);
    // The cells are filled a frame after they are spawned; with none
    // showing yet the wish is kept.
    if slots
        .iter()
        .all(|(_, slot, view)| slot.menu != menu || view.stack.is_none())
    {
        return;
    }
    state.wanted = None;
    state.filter = filter;
    if let (Some(focus), Some(first)) = (focus.as_mut(), first) {
        focus.set(first, FocusCause::Navigated);
    }
}

/// `Update`: an open that waited for the sim to hear of the last close
/// goes ahead once the tick has taken the close from the inbox.
pub fn open_pending(state: Res<StoresState>, mut commands: Commands) {
    if let Some(with_store) = state.pending {
        open(&mut commands, with_store);
    }
}

/// Observer on `MenuClosed`: the screen went, by Back, Close or key I. The
/// sim is told, which puts the cursor's stack away on the truth, and the
/// mirror is dropped with the copy of that stack slotted set aside as
/// dropped into the world, which it was not.
fn on_menu_closed(
    closed: On<MenuClosed>,
    link: Res<StockAuthority>,
    mut state: ResMut<StoresState>,
    mut inbox: ResMut<StockInbox>,
    mut dropped: ResMut<Dropped>,
    mut commands: Commands,
) {
    let Some(open) = state.open.take_if(|open| open.menu == closed.entity) else {
        return;
    };
    {
        // A click predicted this frame goes first, so the close stows
        // what that click left on the cursor.
        let mut wire = link.wire();
        inbox.extend(wire.requests.drain(..));
        wire.stores = None;
    }
    inbox.push(StockRequest::Close { menu: open.id });
    dropped.0.retain(|stack| stack.menu != open.menu);
    for inventory in open.mirror {
        commands.entity(inventory).try_despawn();
    }
}

/// `Update`: I opens the pack over nothing, and closes the screen from the
/// top, in either form.
pub fn toggle(keys: Res<ButtonInput<KeyCode>>, stack: Res<ScreenStack>, mut commands: Commands) {
    if !keys.just_pressed(KEY) {
        return;
    }
    if on_top(&stack) {
        pop_screen(&mut commands);
    } else if stack.top().is_none() {
        open(&mut commands, false);
    }
}

/// The items a cell draws by their baked picture, each with the colour its
/// definition gives it. A picture is tone and coverage with no colour of
/// its own, so the cell tints it ([`dress_slots`]).
#[derive(Resource, Debug, Clone, Default)]
pub struct PicturedIcons(pub HashMap<ItemId, Color>);

/// `Update`: slotted's item registry and its tooltip follow the game's
/// registries and its baked pictures. The frozen item set is the
/// catalogue's own, so a stack's id means the same item to the sim's
/// rules, to slotted's prediction and to the icon in a cell. Every item
/// is drawn by the image the one rule picks ([`card::item_art`]): a baked
/// picture where the content step made one, else the item's library
/// symbol, so a clone with nothing baked still draws a symbol and no
/// stand-in shape. The tooltip is the card and nothing else.
pub fn follow_registries(
    registries: Option<Res<Registries>>,
    pictures: Option<Res<WeaponPictures>>,
    bikes: Option<Res<BikeModels>>,
    assets: Option<Res<AssetServer>>,
    context: Res<CardContext>,
    mut commands: Commands,
) {
    let Some(registries) = registries else {
        return;
    };
    let repictured = pictures.as_ref().is_some_and(DetectChanges::is_changed)
        || bikes.as_ref().is_some_and(DetectChanges::is_changed);
    if !registries.is_changed() && !repictured {
        return;
    }
    let items = &registries.0.items;
    // A baked file is named only where there is a source to load it from.
    let drawable = assets.is_some_and(|assets| assets.get_source(card::GENERATED_SOURCE).is_ok());
    let baked = pictures.as_ref().filter(|_| drawable).map(|set| &set.0);
    let models = bikes.as_ref().filter(|_| drawable).map(|bikes| &bikes.0);
    let mut icons = BTreeMap::new();
    let mut tints = HashMap::new();
    let mut images = HashMap::new();
    for def in items.iter() {
        let Some(art) = card::item_art(def, baked, models, &registries.0.icons) else {
            continue;
        };
        if let Some(id) = items.id_of(&def.id)
            && let Some(tint) = art.tint
        {
            tints.insert(id, tint);
        }
        icons.insert(def.id.clone(), art.path.clone());
        images.insert(def.id.to_string(), art);
    }
    commands.insert_resource(slotted::ecs::Registries(items.frozen_with(&icons)));
    commands.insert_resource(PicturedIcons(tints));
    commands.insert_resource(card::ItemImages(images));
    let mut parts = TooltipParts::default();
    parts.push(CardPart(registries.0.clone(), context.clone()));
    commands.insert_resource(parts);
}

/// `Update`: while this screen is on top the card slotted would raise
/// beside the pointer stands down, since the pane already shows it.
pub fn hush_raised_card(stack: Res<ScreenStack>, context: Res<CardContext>) {
    context.set_quiet(on_top(&stack));
}

// ---- what follows the mirror ----

/// `Update`, after slotted's reconcile: the menu follows the mirror. A bag
/// put in a slot opens its cells and a bag with something in it is locked,
/// by the sim's own `layout` over the mirror, which is what the sim will
/// have made of the truth by the time the next click reaches it.
#[allow(
    clippy::too_many_arguments,
    reason = "the state, the mirror and what decides the saddle's access"
)]
pub fn follow_stock(
    mut state: ResMut<StoresState>,
    registries: Option<Res<Registries>>,
    changed: Query<(), (With<StockMirror>, Changed<Inventory>)>,
    inventories: Query<&Inventory>,
    mut menus: Query<&mut OpenMenu>,
    riding: Query<(), (With<LocalPlayer>, With<Riding>)>,
    access: Option<Res<StoreAccess>>,
    reach: Option<Res<Reach>>,
) {
    let (Some(open), Some(registries)) = (state.open.as_mut(), registries) else {
        return;
    };
    let reached = open.with_store
        && reach.is_some_and(|reach| {
            items::at_locker(access.map(|access| *access).unwrap_or_default(), &reach)
        });
    let now = SaddleAccess::of(!riding.is_empty(), reached);
    if changed.is_empty() && now == open.access {
        return;
    }
    let items = &registries.0.items;
    let Some(stock) = mirrored(open, None, &inventories) else {
        return;
    };
    let layout = items::layout(
        items.carry(),
        items,
        &stock.pack,
        open.with_store,
        saddle_slots(&stock.saddle, !riding.is_empty(), reached),
    );
    if let Ok(mut menu) = menus.get_mut(open.menu)
        && *menu.def != layout.def
    {
        menu.def = Arc::new(layout.def.clone());
    }
    open.bags = bag_views(&stock.pack, items);
    open.layout = layout;
    open.access = now;
}

/// The first node under `entity` that is a text.
fn first_text(
    entity: Entity,
    children: &Query<&Children>,
    texts: &Query<(), With<LocText>>,
) -> Option<Entity> {
    if texts.contains(entity) {
        return Some(entity);
    }
    children
        .get(entity)
        .ok()?
        .iter()
        .find_map(|child| first_text(child, children, texts))
}

/// `Update`: the nodes the screen writes to, found once it has spawned.
pub fn find_parts(
    mut state: ResMut<StoresState>,
    ids: Query<(Entity, &TestId)>,
    children: Query<&Children>,
    texts: Query<(), With<LocText>>,
) {
    let Some(open) = state.open.as_mut().filter(|open| open.parts.is_none()) else {
        return;
    };
    let find = |id: &str| ids.iter().find(|(_, test)| test.0 == id).map(|(e, _)| e);
    let (Some(card), Some(send), Some(note), Some(status)) =
        (find(CARD_ID), find(SEND), find(NOTE_ID), find(STATUS_ID))
    else {
        return;
    };
    open.parts = Some(Parts {
        card,
        send,
        send_label: first_text(send, &children, &texts),
        sends: find(SENDS_ID),
        take: find(TAKE),
        take_label: find(TAKE).and_then(|take| first_text(take, &children, &texts)),
        note,
        status,
        captions: std::array::from_fn(|bag| find(&bag_caption_id(bag))),
        locks: std::array::from_fn(|bag| find(&bag_lock_id(bag))),
        fitted: std::array::from_fn(|hand| find(&hand_fitted_id(hand))),
        ride: (|| {
            let repair = find(RIDE_REPAIR)?;
            Some(RideParts {
                repair,
                repair_label: first_text(repair, &children, &texts),
                repair_why: find(RIDE_REPAIR_WHY)?,
                put: find(RIDE_PUT)?,
                put_why: find(RIDE_PUT_WHY)?,
                shut: find(RIDE_SHUT)?,
            })
        })(),
        chips: Filter::ALL
            .into_iter()
            .filter_map(|filter| {
                let chip = find(&filter.chip_id())?;
                Some((filter, chip, first_text(chip, &children, &texts)))
            })
            .collect(),
    });
}

/// Writes `words` in `role` to a text node, when it says something else.
fn write(
    texts: &mut Query<(&mut LocText, &mut Text, &mut Themed)>,
    entity: Entity,
    words: &str,
    role: &'static str,
) {
    let Ok((mut loc, mut text, mut themed)) = texts.get_mut(entity) else {
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
}

/// Shows or blanks a node that keeps its place in the layout: the foot of
/// the card's column holds the same room whatever is pinned, so a button
/// that does not apply is blank and never `Display::None`.
fn reveal(visibility: &mut Mut<'_, Visibility>, shown: bool) {
    let want = if shown {
        Visibility::Inherited
    } else {
        Visibility::Hidden
    };
    if **visibility != want {
        **visibility = want;
    }
}

fn show(node: &mut Mut<'_, Node>, display: Display) {
    if node.display != display {
        node.display = display;
    }
}

/// The tab stop of a control that is shown, or none for one that is not:
/// Tab walks every stop there is, laid out or not, so a control left out
/// of the layout must be left out of the walk as well.
fn stop(tab: &mut Mut<'_, TabIndex>, shown: Option<i32>) {
    let wanted = shown.unwrap_or(-1);
    if tab.0 != wanted {
        tab.0 = wanted;
    }
}

/// The cells under the bag slots, shown or left out as the menu has them,
/// and each bag's grid with the edge of a cell, for its rows to be sized.
fn show_cells(
    open: &Open,
    cells: &mut Query<(&SlotRef, &mut Node, &ChildOf, &mut TabIndex)>,
) -> [Option<(Entity, f32)>; BAG_SLOTS] {
    let layout = &open.layout;
    let mut grids = [None; BAG_SLOTS];
    for (slot, mut node, parent, mut tab) in cells {
        if slot.menu != open.menu {
            continue;
        }
        let Some((bag, range)) = layout
            .bags
            .iter()
            .enumerate()
            .find(|(_, range)| range.contains(slot.slot))
        else {
            continue;
        };
        let open_cell = layout
            .def
            .slot(slot.slot)
            .is_some_and(|def| def.behaviour != SlotBehaviour::Disabled);
        show(
            &mut node,
            if open_cell {
                Display::Flex
            } else {
                Display::None
            },
        );
        // A grid numbers its cells' stops from its first, which is what an
        // open cell goes back to.
        stop(
            &mut tab,
            open_cell.then(|| i32::from(slot.slot.0 - range.start)),
        );
        if let Val::Px(size) = node.width {
            grids[bag] = Some((parent.parent(), size));
        }
    }
    grids
}

/// What a bag slot's caption says of it.
fn bag_words(view: &BagView) -> String {
    let words = |key: &'static str| english(key).unwrap_or(key);
    match &view.name {
        None => words("strata.stores.bag.empty").to_owned(),
        Some(name) if view.held > 0 => words("strata.stores.bag.holds")
            .replace("{name}", name)
            .replace("{stacks}", &view.held.to_string()),
        Some(name) => words("strata.stores.bag.open")
            .replace("{name}", name)
            .replace("{cells}", &view.cells.to_string()),
    }
}

/// The slot `offset` into `range`.
fn slot_at(range: SlotRange, offset: usize) -> SlotIx {
    SlotIx(
        range
            .start
            .saturating_add(u16::try_from(offset).unwrap_or(u16::MAX)),
    )
}

/// `Update`: the bags as the mirror has them. A bag's grid shows the cells
/// the bag opens and no others, which are no stops for Tab either, and no
/// rows for cells it does not; the
/// caption says what is in the slot, and for a bag that holds something
/// that it must be emptied first, beside a lock. A refusal is said in the
/// caption of the slot it was refused at, for a few seconds.
#[allow(
    clippy::type_complexity,
    reason = "the two node queries are told apart by their filters"
)]
pub fn show_bags(
    time: Res<Time>,
    registries: Option<Res<Registries>>,
    mut state: ResMut<StoresState>,
    mut cells: Query<(&SlotRef, &mut Node, &ChildOf, &mut TabIndex)>,
    mut nodes: Query<&mut Node, Without<SlotRef>>,
    mut texts: Query<(&mut LocText, &mut Text, &mut Themed)>,
) {
    let (Some(open), Some(registries)) = (state.open.as_mut(), registries) else {
        return;
    };
    let now = time.elapsed_secs_f64();
    if open.flash.as_ref().is_some_and(|flash| flash.until <= now) {
        open.flash = None;
    }
    let Some(parts) = open.parts.clone() else {
        return;
    };
    let layout = &open.layout;
    // A bag's grid is laid out in the pack's columns, as the definition
    // declared it.
    let cols = registries.0.items.carry().pack.cols.max(1);
    let grids = show_cells(open, &mut cells);
    let flashed = |slot: SlotIx| open.flash.as_ref().filter(|flash| flash.slot == slot);
    for (bag, (view, grid)) in open.bags.iter().zip(grids).enumerate() {
        if let Some((grid, size)) = grid
            && let Ok(mut node) = nodes.get_mut(grid)
        {
            let rows = view.cells.div_ceil(cols);
            show(
                &mut node,
                if rows == 0 {
                    Display::None
                } else {
                    Display::Grid
                },
            );
            let tracks = vec![RepeatedGridTrack::px(rows.max(1), size)];
            if node.grid_template_rows != tracks {
                node.grid_template_rows = tracks;
            }
        }
        if let Some(lock) = parts.locks[bag]
            && let Ok(mut node) = nodes.get_mut(lock)
        {
            show(
                &mut node,
                if view.held > 0 {
                    Display::Flex
                } else {
                    Display::None
                },
            );
        }
        let Some(caption) = parts.captions[bag] else {
            continue;
        };
        match flashed(slot_at(layout.bag_slots, bag)) {
            Some(flash) => write(&mut texts, caption, &flash.words, REFUSAL_ROLE),
            None => write(&mut texts, caption, &bag_words(view), CAPTION_ROLE),
        }
    }
    for (hand, (line, fitted)) in parts.fitted.iter().zip(&open.fitted).enumerate() {
        let Some(line) = *line else {
            continue;
        };
        match flashed(slot_at(layout.hands, hand)) {
            Some(flash) => write(&mut texts, line, &flash.words, REFUSAL_ROLE),
            None => write(&mut texts, line, fitted, CAPTION_ROLE),
        }
    }
}

// ---- the cells' dressing ----

/// On a stores cell: the marks drawn on it over slotted's icon and count,
/// and what they last showed.
#[derive(Component, Debug, Clone, PartialEq, Eq)]
pub struct SlotDressing {
    pip: Entity,
    pip_fill: Entity,
    pip_cap: Entity,
    condition: Entity,
    condition_fill: Entity,
    /// The stack's quality, the mean of its lines; none for an empty cell.
    pub grade: Option<u16>,
    /// Whether the stack is a crafted batch and not store grade.
    pub crafted: bool,
    /// A weapon's condition, when it has worn below new.
    pub worn: Option<u16>,
    /// Whether the filter in force leaves the stack out.
    pub dimmed: bool,
    /// The kind of the item in the cell, for the filter.
    kind: Option<ItemKind>,
}

impl SlotDressing {
    /// The cap across the head of the pip, shown on a crafted batch.
    #[must_use]
    pub const fn cap(&self) -> Entity {
        self.pip_cap
    }
}

/// On a child of a cell that is part of its dressing.
#[derive(Component, Debug, Clone, Copy, Default)]
pub struct SlotMark;

fn mark(parent: Entity, node: Node, role: &'static str) -> impl Bundle {
    (
        node,
        Themed(Role::new_static(role)),
        SlotMark,
        Pickable::IGNORE,
        ChildOf(parent),
    )
}

/// `Update`, after slotted has drawn its icons: a cell's dressing. A pip
/// up its left edge filled to the stack's quality, with a cap across its
/// head on a crafted batch, so two batches of one thing are told apart in
/// the grid even where their qualities average alike; a strip across its
/// top for a weapon that has worn; and, when the filter leaves its stack
/// out, the icon and the count at a fraction of their strength ([`DIMMED`]).
#[allow(
    clippy::type_complexity,
    clippy::too_many_arguments,
    reason = "the cells, their marks and slotted's own children are each a query"
)]
pub fn dress_slots(
    state: Res<StoresState>,
    registries: Option<Res<Registries>>,
    pictured: Option<Res<PicturedIcons>>,
    mut commands: Commands,
    bare: Query<(Entity, &SlotRef, &Node), (With<ItemView>, Without<SlotDressing>)>,
    mut cells: Query<(&SlotRef, Ref<ItemView>, &mut SlotDressing, &Children)>,
    mut marks: Query<(&mut Node, &mut Visibility), (With<SlotMark>, Without<SlotRef>)>,
    mut icons: Query<&mut ImageNode, With<ItemIcon>>,
    mut counts: Query<
        (&mut TextColor, &mut Node),
        (With<ItemCount>, Without<SlotMark>, Without<ItemView>),
    >,
) {
    let (Some(open), Some(registries)) = (state.open.as_ref(), registries) else {
        return;
    };
    let items = &registries.0.items;
    for (entity, slot, node) in &bare {
        if slot.menu != open.menu {
            continue;
        }
        if let Val::Px(size) = node.height {
            let dressing = spawn_marks(&mut commands, entity, size);
            commands.entity(entity).insert(dressing);
        }
    }
    let percent = |q: u16| f32::from(q) / 10.;
    for (slot, view, mut dressing, children) in &mut cells {
        if slot.menu != open.menu {
            continue;
        }
        if view.is_changed() || dressing.is_added() {
            let record = view
                .stack
                .as_ref()
                .and_then(|stack| items::read(stack, items).ok());
            // A lot's is where it stands in its type's spans, so two lots
            // of one type tell apart as two batches of a part do.
            let grade = record.as_ref().map(|record| items::grade_of(items, record));
            let worn = record
                .as_ref()
                .and_then(|record| record.condition)
                .filter(|condition| *condition < strata_registry::item::QUALITY_MAX);
            let kind = record
                .as_ref()
                .and_then(|record| items.get(&record.item))
                .map(|def| def.kind.clone());
            let crafted = record.as_ref().is_some_and(|record| record.batch > 0);
            if dressing.grade != grade
                || dressing.worn != worn
                || dressing.kind != kind
                || dressing.crafted != crafted
            {
                dressing.grade = grade;
                dressing.worn = worn;
                dressing.kind = kind;
                dressing.crafted = crafted;
            }
            if let Ok((_, mut visibility)) = marks.get_mut(dressing.pip_cap) {
                let wanted = if crafted {
                    Visibility::Inherited
                } else {
                    Visibility::Hidden
                };
                if *visibility != wanted {
                    *visibility = wanted;
                }
            }
            for (track, fill, value, upright) in [
                (dressing.pip, dressing.pip_fill, grade, true),
                (dressing.condition, dressing.condition_fill, worn, false),
            ] {
                if let Ok((_, mut visibility)) = marks.get_mut(track) {
                    let wanted = if value.is_some() {
                        Visibility::Inherited
                    } else {
                        Visibility::Hidden
                    };
                    if *visibility != wanted {
                        *visibility = wanted;
                    }
                }
                if let (Some(value), Ok((mut node, _))) = (value, marks.get_mut(fill)) {
                    let length = Val::Percent(percent(value));
                    if upright && node.height != length {
                        node.height = length;
                    } else if !upright && node.width != length {
                        node.width = length;
                    }
                }
            }
        }
        // Slotted repaints the icon white and the theme the count's colour
        // whenever either changes, so the strength, and a picture's tint,
        // are held every frame and written only where they have drifted.
        let dimmed = dressing
            .kind
            .as_ref()
            .is_some_and(|kind| !state.filter.shows(kind));
        if dressing.dimmed != dimmed {
            dressing.dimmed = dimmed;
        }
        let strength = if dimmed { DIMMED } else { 1. };
        let tint = view
            .stack
            .as_ref()
            .zip(pictured.as_ref())
            .and_then(|(stack, pictured)| pictured.0.get(&stack.id))
            .copied();
        hold_look(children, tint, strength, &mut icons, &mut counts);
    }
}

/// Holds a cell's icon and count at `strength`, a pictured icon in its
/// `tint`, and the count clear of the cell's edge. A shape keeps the
/// colours it was baked in and takes the strength alone.
#[allow(
    clippy::type_complexity,
    reason = "the counts' query is the caller's, filters and all"
)]
fn hold_look(
    children: &Children,
    tint: Option<Color>,
    strength: f32,
    icons: &mut Query<&mut ImageNode, With<ItemIcon>>,
    counts: &mut Query<
        (&mut TextColor, &mut Node),
        (With<ItemCount>, Without<SlotMark>, Without<ItemView>),
    >,
) {
    for child in children.iter() {
        if let Ok(mut icon) = icons.get_mut(child) {
            let wanted = tint.unwrap_or(icon.color).with_alpha(strength);
            if icon.color != wanted {
                icon.color = wanted;
            }
        }
        if let Ok((mut count, mut node)) = counts.get_mut(child) {
            if (count.0.alpha() - strength).abs() > f32::EPSILON {
                count.0.set_alpha(strength);
            }
            if node.right != COUNT_INSET {
                node.right = COUNT_INSET;
            }
        }
    }
}

/// The marks of one cell `size` across, hidden until it holds something:
/// the pip's track, its fill and its cap, the condition strip's and its
/// fill.
fn spawn_marks(commands: &mut Commands, entity: Entity, size: f32) -> SlotDressing {
    let absolute = || Node {
        position_type: PositionType::Absolute,
        ..Node::default()
    };
    let pip = commands
        .spawn((
            mark(
                entity,
                Node {
                    left: Val::Px(2.),
                    bottom: Val::Px(3.),
                    width: Val::Px(3.),
                    height: Val::Px(size - 10.),
                    ..absolute()
                },
                PIP_ROLE,
            ),
            Visibility::Hidden,
        ))
        .id();
    let pip_fill = commands
        .spawn(mark(
            pip,
            Node {
                left: Val::Px(0.),
                right: Val::Px(0.),
                bottom: Val::Px(0.),
                height: Val::Percent(0.),
                ..absolute()
            },
            PIP_FILL_ROLE,
        ))
        .id();
    // Across the head of the track and proud of it either side, so it
    // reads as a mark of its own whatever the fill has reached.
    let pip_cap = commands
        .spawn((
            mark(
                pip,
                Node {
                    left: Val::Px(-2.),
                    right: Val::Px(-2.),
                    top: Val::Px(-1.),
                    height: Val::Px(3.),
                    ..absolute()
                },
                PIP_CAP_ROLE,
            ),
            Visibility::Hidden,
        ))
        .id();
    let condition = commands
        .spawn((
            mark(
                entity,
                Node {
                    left: Val::Px(8.),
                    right: Val::Px(4.),
                    top: Val::Px(3.),
                    height: Val::Px(3.),
                    ..absolute()
                },
                CONDITION_ROLE,
            ),
            Visibility::Hidden,
        ))
        .id();
    let condition_fill = commands
        .spawn(mark(
            condition,
            Node {
                left: Val::Px(0.),
                top: Val::Px(0.),
                bottom: Val::Px(0.),
                width: Val::Percent(100.),
                ..absolute()
            },
            CONDITION_FILL_ROLE,
        ))
        .id();
    SlotDressing {
        pip,
        pip_fill,
        pip_cap,
        condition,
        condition_fill,
        grade: None,
        crafted: false,
        worn: None,
        dimmed: false,
        kind: None,
    }
}

// ---- the pane, the chips and the footer ----

/// `Update`: the card pinned in the pane is that of the stack last focused
/// or pointed at, the long one.
///
/// The pin moves only onto a cell that holds a stack. The send button acts
/// on the pin, and the way to it from a cell, by the arrows or the pointer,
/// lies across other cells; an empty one on the way must not take the pin
/// with it. The pointer pins only once it has moved since the screen
/// opened, and only while it is the device in use, so that a screen opened
/// and walked by the keys shows the focused stack's card and not that of
/// whatever cell came up under a parked pointer.
#[allow(
    clippy::too_many_arguments,
    reason = "the focus, the pointer and its motion, the slots and the card"
)]
pub fn pin_card(
    registries: Option<Res<Registries>>,
    context: Res<CardContext>,
    mode: Res<InputMode>,
    mut cursor: MessageReader<CursorMoved>,
    mut inputs: MessageReader<PointerInput>,
    focus: Option<Res<InputFocus>>,
    mut state: ResMut<StoresState>,
    pointed: Query<(Entity, &Hovered, &SlotRef)>,
    slots: Query<(&SlotRef, Ref<ItemView>)>,
    cards: Query<&ItemCard>,
    mut commands: Commands,
) {
    // Read whether or not a screen is up, so the motion that led to the
    // click that opened one is not taken for motion over it. A window
    // reports its cursor, a harness only the pointer's own input.
    let moved = cursor.read().count()
        + inputs
            .read()
            .filter(|input| match input.action {
                PointerAction::Move { delta } => delta != Vec2::ZERO,
                _ => false,
            })
            .count();
    let (Some(open), Some(registries)) = (state.open.as_mut(), registries) else {
        return;
    };
    let Some(card) = open.parts.as_ref().map(|parts| parts.card) else {
        return;
    };
    let items = &registries.0.items;
    let menu = open.menu;
    let holds = |entity: Entity| {
        slots
            .get(entity)
            .is_ok_and(|(slot, view)| slot.menu == menu && view.stack.is_some())
    };
    // The focused stack's, when the focus came to it or a stack came to
    // the focused cell, as one put down there does.
    if let Some(entity) = focus.as_deref().and_then(InputFocus::get)
        && holds(entity)
        && (open.pinned.is_none()
            || focus.as_ref().is_some_and(DetectChanges::is_changed)
            || slots.get(entity).is_ok_and(|(_, view)| view.is_changed()))
    {
        open.pinned = Some(entity);
    }
    open.pointer_moved |= moved > 0;
    if open.pointer_moved && *mode == InputMode::Pointer {
        let over = pointed
            .iter()
            .find(|(_, hovered, slot)| hovered.get() && slot.menu == menu)
            .map(|(entity, ..)| entity);
        if open.hovered != over {
            open.hovered = over;
            if let Some(entity) = over.filter(|entity| holds(*entity)) {
                open.pinned = Some(entity);
            }
        }
    } else {
        // Back on the pointer, the cell under it is come onto afresh.
        open.hovered = None;
    }
    let pinned = open
        .pinned
        .and_then(|entity| slots.get(entity).ok())
        .filter(|(slot, _)| slot.menu == menu);
    if pinned.is_none() {
        open.pinned = None;
    }
    // Read against the weapons carried as they are this frame, so a part
    // fitted on the bench moves the figures of the card pinned under it.
    let against = context.against();
    let params = pinned
        .and_then(|(_, view)| items::read(view.stack.as_ref()?, items).ok())
        .and_then(|record| CardParams::of(&record, &registries.0, true, against.as_ref()))
        .unwrap_or_else(CardParams::empty)
        .framed();
    if cards.get(card).is_ok_and(|card| card.0 != params) {
        commands.queue(move |world: &mut World| card::repaint(world, card, &params));
    }
}

/// `Update`, after the card is pinned: under it the way across for its
/// stack, which is the send button by the side the stack is on with the
/// stack it will send named under it, or for a weapon in hand the note
/// that a click on it opens its fitting. A button that is not shown is no
/// stop for Tab either.
#[allow(
    clippy::type_complexity,
    clippy::too_many_arguments,
    reason = "the nodes and the stops are told from the cells' by a filter"
)]
pub fn show_send(
    registries: Option<Res<Registries>>,
    context: Res<CardContext>,
    mut focus: Option<ResMut<InputFocus>>,
    mut state: ResMut<StoresState>,
    slots: Query<(&SlotRef, &ItemView)>,
    mut visibility: Query<&mut Visibility, Without<SlotRef>>,
    mut stops: Query<&mut TabIndex, Without<SlotRef>>,
    mut labels: Query<(&mut LocText, &mut Text)>,
    mut named: Query<&mut SemanticLabel>,
) {
    let Some(open) = state.open.as_mut() else {
        return;
    };
    let Some(parts) = open.parts.clone() else {
        return;
    };
    let pinned_stack = open
        .pinned
        .and_then(|entity| slots.get(entity).ok())
        .and_then(|(slot, view)| Some((slot.slot, view.stack.as_ref()?)));
    let pinned = pinned_stack.map(|(slot, _)| slot);
    let in_hand = pinned.is_some_and(|slot| open.layout.hands.contains(slot));
    let from_store =
        pinned.is_some_and(|slot| open.layout.store.is_some_and(|store| store.contains(slot)));
    let sends = open.with_store && pinned.is_some() && !in_hand;
    if let Ok(mut node) = visibility.get_mut(parts.send) {
        reveal(&mut node, sends);
    }
    if let Ok(mut tab) = stops.get_mut(parts.send) {
        stop(&mut tab, sends.then_some(0));
    }
    // A button that goes while it has the focus would leave the keys with
    // nowhere to walk from, so the focus goes back to the slot it sent.
    if open.send_shown
        && !sends
        && let (Some(focus), Some(slot)) = (focus.as_mut(), open.pinned)
        && focus.get() == Some(parts.send)
    {
        focus.set(slot, FocusCause::Navigated);
    }
    open.send_shown = sends;
    if sends && let Some(label) = parts.send_label {
        let words = english(if from_store {
            "strata.stores.send.pack"
        } else {
            "strata.stores.send.store"
        })
        .unwrap_or_default();
        if let Ok((mut loc, mut text)) = labels.get_mut(label)
            && text.0 != words
        {
            loc.key = LocKey(words.to_owned());
            words.clone_into(&mut text.0);
        }
        if let Ok(mut name) = named.get_mut(parts.send)
            && name.0 != words
        {
            words.clone_into(&mut name.0);
        }
    }
    // The pin follows what the focus crosses on its way to the button, so
    // the button says which stack it will send.
    if let Some(sends_line) = parts.sends {
        if let Ok(mut node) = visibility.get_mut(sends_line) {
            reveal(&mut node, sends);
        }
        if sends
            && let Some((_, stack)) = pinned_stack
            && let Ok((mut loc, mut text)) = labels.get_mut(sends_line)
        {
            let name = registries.as_ref().map_or_else(
                || "?".to_owned(),
                |registries| card::stack_name(&registries.0, stack, context.against().as_ref()),
            );
            let words = english("strata.stores.sends")
                .unwrap_or_default()
                .replace("{name}", &name)
                .replace("{count}", &stack.count.to_string());
            if text.0 != words {
                loc.key = LocKey(words.clone());
                text.0 = words;
            }
        }
    }
    if let Ok(mut node) = visibility.get_mut(parts.note) {
        reveal(&mut node, in_hand);
    }
}

/// What taking the pinned stack does: a weapon or module in a hand, or a
/// bike as the household's ride.
enum Taking {
    /// The kind, the hand, the verdict and the button's words, which name
    /// the weapon the hand would put away.
    Hand(
        strata_registry::item::Record,
        usize,
        Result<(), EquipRefusal>,
        String,
    ),
    /// The bike's kind and the verdict.
    Ride(strata_registry::item::Record, Result<(), RideRefusal>),
}

impl Taking {
    /// Whether the sim would refuse it, and the words if so.
    fn refusal(&self) -> Option<&'static str> {
        match self {
            Self::Hand(_, _, verdict, _) => verdict.err().map(take_refusal_words),
            Self::Ride(_, verdict) => verdict.err().map(super::ride::stable_words),
        }
    }

    /// The button's key.
    const fn label_key(&self) -> &'static str {
        match self {
            Self::Hand(..) => "strata.stores.take",
            Self::Ride(..) => "strata.stores.take.ride",
        }
    }
}

/// The sim's stable as the mirror's saddle holds it, and whether the local
/// player rides: what the sim's bike rules are asked over.
struct RideFacts<'a> {
    stable: &'a Stable,
    riding: bool,
    /// The hand in use, which a new weapon goes to the other of.
    in_use: usize,
    /// Whether the member is between actions: a weapon is not changed
    /// mid swing.
    ready: bool,
}

/// What the pinned stack would be taken as, and whether the sim's own rule
/// allows it over the mirror. None for a stack that is no weapon, module or
/// bike, or already in hand.
fn take_of(
    open: &Open,
    at: Option<Entity>,
    items: &ItemCatalogue,
    ride: &RideFacts<'_>,
    slots: &Query<(&SlotRef, &ItemView)>,
    menus: &Query<&OpenMenu>,
    inventories: &Query<&Inventory>,
) -> Option<Taking> {
    let (slot, view) = at.and_then(|entity| slots.get(entity).ok())?;
    if open.layout.hands.contains(slot.slot) {
        return None;
    }
    let stack = view.stack.as_ref()?;
    let def = items.def_of(stack.id)?;
    let mut hand = match &def.kind {
        ItemKind::Weapon { .. } | ItemKind::Module { .. } => 0,
        ItemKind::Vehicle { .. } => {
            let kind = kind_of(&items::read(stack, items).ok()?);
            let carried = menus.get(open.menu).ok()?.state.carried.clone();
            let mirror = mirrored(open, carried, inventories)?;
            let stable = Stable {
                bike: ride.stable.bike.clone(),
                saddle: mirror.saddle.clone(),
            };
            let verdict = veyra_sim::ride::can_stable(
                items,
                &stable,
                &mirror.pack,
                mirror.store.as_ref(),
                &kind,
                ride.riding,
            );
            return Some(Taking::Ride(kind, verdict));
        }
        _ => return None,
    };
    let kind = kind_of(&items::read(stack, items).ok()?);
    let carried = menus.get(open.menu).ok()?.state.carried.clone();
    let mirror = mirrored(open, carried, inventories)?;
    let saved = mirror.hands.saved(items).ok()?;
    if matches!(def.kind, ItemKind::Weapon { .. }) {
        hand = equip::hand_for(items, &saved, &kind, ride.in_use);
    }
    let verdict = equip::can_take(
        items,
        &saved,
        &mirror.pack,
        mirror.store.as_ref(),
        &kind,
        hand,
        ride.ready,
    );
    let words = take_words(items, &saved, hand);
    Some(Taking::Hand(kind, hand, verdict, words))
}

/// The button's words for taking a weapon in `hand`: which hand, and the
/// weapon that would leave it when there is one.
fn take_words(
    items: &ItemCatalogue,
    hands: &[Option<strata_registry::item::Record>; strata_registry::item::HANDS],
    hand: usize,
) -> String {
    let away = hands
        .get(hand)
        .and_then(Option::as_ref)
        .and_then(|record| items.get(&record.item));
    english(if away.is_some() {
        "strata.stores.take.hand.away"
    } else {
        "strata.stores.take.hand"
    })
    .unwrap_or_default()
    .replace("{hand}", &(hand + 1).to_string())
    .replace("{weapon}", away.map_or("", |def| def.name.as_str()))
}

/// The words for a weapon that would not be taken in hand.
fn take_refusal_words(refusal: EquipRefusal) -> &'static str {
    english(match refusal {
        EquipRefusal::NotAWeapon => "strata.stores.take.not_a_weapon",
        EquipRefusal::SameFamily => "strata.stores.take.same_family",
        EquipRefusal::Busy => "strata.stores.take.busy",
        EquipRefusal::NotHeld => "strata.stores.take.not_held",
        EquipRefusal::MidAction => "strata.stores.take.mid_action",
    })
    .unwrap_or_default()
}

/// `Update`, after the card is pinned: Take in hand under the send button
/// for a pinned weapon, and for a module, which it is grey for. Grey with
/// the sim's refusal in words under it, live when the sim would do it. The
/// rule is [`equip::can_take`], asked over the mirror; nothing is decided
/// here.
#[allow(
    clippy::type_complexity,
    clippy::too_many_arguments,
    reason = "the nodes and the stops are told from the cells' by a filter"
)]
pub fn show_take(
    registries: Option<Res<Registries>>,
    state: Res<StoresState>,
    slots: Query<(&SlotRef, &ItemView)>,
    menus: Query<&OpenMenu>,
    inventories: Query<&Inventory>,
    mut visibility: Query<&mut Visibility, Without<SlotRef>>,
    mut stops: Query<&mut TabIndex, Without<SlotRef>>,
    mut labels: Query<(&mut LocText, &mut Text)>,
    mut buttons: Query<&mut ButtonState>,
    mut named: Query<&mut SemanticLabel>,
    stable: Option<Res<Stable>>,
    riding: Query<(), (With<LocalPlayer>, With<Riding>)>,
    locals: Query<&Combatant, With<LocalPlayer>>,
    mut commands: Commands,
) {
    let (Some(open), Some(registries)) = (state.open.as_ref(), registries) else {
        return;
    };
    let Some(parts) = open.parts.as_ref() else {
        return;
    };
    let Some(button) = parts.take else {
        return;
    };
    let no_bike = Stable::default();
    let facts = RideFacts {
        stable: stable.as_deref().unwrap_or(&no_bike),
        riding: !riding.is_empty(),
        in_use: locals.single().map_or(0, Combatant::hand),
        ready: locals
            .single()
            .ok()
            // No Combatant counts as ready, as the sim's consume has it.
            .is_none_or(|actor| actor.action == Action::Ready),
    };
    let taking = take_of(
        open,
        open.pinned,
        &registries.0.items,
        &facts,
        &slots,
        &menus,
        &inventories,
    );
    let shown = taking.is_some();
    if let Ok(mut node) = visibility.get_mut(button) {
        reveal(&mut node, shown);
    }
    if let Ok(mut tab) = stops.get_mut(button) {
        stop(&mut tab, shown.then_some(0));
    }
    // The button's words follow what it would do.
    let label = match &taking {
        // A weapon says which hand it would go to.
        Some(Taking::Hand(kind, _, _, words)) if registries.0.items.family_of(kind).is_some() => {
            words.clone()
        }
        _ => english(
            taking
                .as_ref()
                .map_or("strata.stores.take", Taking::label_key),
        )
        .unwrap_or_default()
        .to_owned(),
    };
    let label = label.as_str();
    if let Some(entity) = parts.take_label
        && let Ok((mut loc, mut text)) = labels.get_mut(entity)
        && text.0 != label
    {
        loc.key = LocKey(label.to_owned());
        label.clone_into(&mut text.0);
    }
    if let Ok(mut name) = named.get_mut(button)
        && name.0 != label
    {
        label.clone_into(&mut name.0);
    }
    let refused = taking.as_ref().and_then(Taking::refusal);
    if let Ok(mut state) = buttons.get_mut(button)
        && state.disabled != refused.is_some()
    {
        state.disabled = refused.is_some();
        if refused.is_some() {
            commands.entity(button).insert(InteractionDisabled);
        } else {
            commands.entity(button).remove::<InteractionDisabled>();
        }
    }
    // The reason is said in the note's line under the buttons, which says
    // a weapon in hand opens its fitting when nothing else is to be said.
    // On the pack alone T takes a live weapon in hand, and the note says so.
    let by_key = shown && refused.is_none() && !open.with_store;
    let note = english(if by_key {
        "strata.stores.hand.note.pack"
    } else {
        "strata.stores.hand.note"
    })
    .unwrap_or_default();
    let said = refused.unwrap_or(note);
    if let Ok(mut node) = visibility.get_mut(parts.note)
        && (refused.is_some() || by_key)
    {
        reveal(&mut node, true);
    }
    if let Ok((mut loc, mut text)) = labels.get_mut(parts.note)
        && text.0 != said
    {
        loc.key = LocKey(said.to_owned());
        said.clone_into(&mut text.0);
    }
}

/// Sets a RIDE pane button live or grey and says why under it.
#[allow(
    clippy::too_many_arguments,
    reason = "the button, its reason and the queries that write them"
)]
fn set_ride_button(
    button: Entity,
    why: Entity,
    label: Option<(Entity, &str)>,
    refused: Option<&str>,
    buttons: &mut Query<&mut ButtonState>,
    texts: &mut Query<(&mut LocText, &mut Text)>,
    named: &mut Query<&mut SemanticLabel>,
    commands: &mut Commands,
) {
    if let Ok(mut state) = buttons.get_mut(button)
        && state.disabled != refused.is_some()
    {
        state.disabled = refused.is_some();
        if refused.is_some() {
            commands.entity(button).insert(InteractionDisabled);
        } else {
            commands.entity(button).remove::<InteractionDisabled>();
        }
    }
    if let Some((entity, words)) = label {
        if let Ok((mut loc, mut text)) = texts.get_mut(entity)
            && text.0 != words
        {
            loc.key = LocKey(words.to_owned());
            words.clone_into(&mut text.0);
        }
        if let Ok(mut name) = named.get_mut(button)
            && name.0 != words
        {
            words.clone_into(&mut name.0);
        }
    }
    let said = refused.unwrap_or_default();
    if let Ok((mut loc, mut text)) = texts.get_mut(why)
        && text.0 != said
    {
        loc.key = LocKey(said.to_owned());
        said.clone_into(&mut text.0);
    }
}

/// `Update`, after the card is pinned: the RIDE pane's Repair and Put away
/// live or grey by the sim's own rules over the mirror (the kits to hand
/// and the saddle as the cells hold it), each grey one with its reason in
/// words, and the line that the saddle bags are out of reach unless they
/// are in reach. Nothing is decided here.
#[allow(
    clippy::type_complexity,
    clippy::too_many_arguments,
    reason = "the nodes and the stops are told from the cells' by a filter"
)]
pub fn show_ride(
    registries: Option<Res<Registries>>,
    state: Res<StoresState>,
    menus: Query<&OpenMenu>,
    inventories: Query<&Inventory>,
    mut nodes: Query<&mut Node, Without<SlotRef>>,
    mut buttons: Query<&mut ButtonState>,
    mut texts: Query<(&mut LocText, &mut Text)>,
    mut named: Query<&mut SemanticLabel>,
    stable: Option<Res<Stable>>,
    riding: Query<(), (With<LocalPlayer>, With<Riding>)>,
    sheets: Query<&CharacterSheet, With<LocalPlayer>>,
    access: Option<Res<veyra_sim::fitting::WorkbenchAccess>>,
    store_access: Option<Res<StoreAccess>>,
    reach: Option<Res<Reach>>,
    truth: Option<Res<HouseholdStore>>,
    mut commands: Commands,
) {
    let (Some(open), Some(registries), Some(stable)) = (state.open.as_ref(), registries, stable)
    else {
        return;
    };
    let Some(ride) = open.parts.as_ref().and_then(|parts| parts.ride.as_ref()) else {
        return;
    };
    let items = &registries.0.items;
    let carried = menus
        .get(open.menu)
        .ok()
        .and_then(|menu| menu.state.carried.clone());
    let Some(mirror) = mirrored(open, carried, &inventories) else {
        return;
    };
    let riding = !riding.is_empty();
    let seen = Stable {
        bike: stable.bike.clone(),
        saddle: mirror.saddle.clone(),
    };
    let at_bench = access
        .zip(reach.as_deref())
        .is_some_and(|(access, reach)| veyra_sim::fitting::at_workbench(*access, reach));
    let repair = wear::can_repair_bike(
        items,
        seen.bike.as_ref(),
        &mirror.pack,
        truth.as_deref(),
        (
            at_bench,
            sheets.single().ok().map(|sheet| veyra_sim::wear::Bearer {
                graph: &registries.0.progression,
                sheet,
            }),
        ),
    );
    let words_of = |key: &'static str| english(key).unwrap_or_default();
    let (label, refusal) = match &repair {
        Ok(plan) => (
            words_of(if plan.kit.is_some() {
                "strata.fitting.repair.plan"
            } else {
                "strata.fitting.repair.plan_kitless"
            })
            .replace("{gain}", &plan.gain.to_string()),
            None,
        ),
        Err(refusal) => (
            words_of("strata.fitting.repair").to_owned(),
            Some(super::fitting::repair_refusal_words(*refusal)),
        ),
    };
    set_ride_button(
        ride.repair,
        ride.repair_why,
        ride.repair_label.map(|entity| (entity, label.as_str())),
        refusal.as_deref(),
        &mut buttons,
        &mut texts,
        &mut named,
        &mut commands,
    );
    let put =
        veyra_sim::ride::can_put_away(items, &seen, &mirror.pack, mirror.store.as_ref(), riding);
    set_ride_button(
        ride.put,
        ride.put_why,
        None,
        put.err().map(super::ride::stable_words),
        &mut buttons,
        &mut texts,
        &mut named,
        &mut commands,
    );
    let reached = open.with_store
        && reach.is_some_and(|reach| {
            items::at_locker(
                store_access.map(|access| *access).unwrap_or_default(),
                &reach,
            )
        });
    let shut = SaddleAccess::of(riding, reached) == SaddleAccess::Shut;
    if let Ok(mut node) = nodes.get_mut(ride.shut) {
        show(&mut node, if shut { Display::Flex } else { Display::None });
    }
}

/// `Update`: a bike taken as the ride, put away or repaired changes what
/// the RIDE pane and the cells say, so the screen is opened again over the
/// truth as it is after a weapon is taken in hand.
pub fn answer_rides(
    mut rides: MessageReader<RideAnswer>,
    mut repairs: MessageReader<RepairAnswer>,
    mut state: ResMut<StoresState>,
    stack: Res<ScreenStack>,
    mut commands: Commands,
) {
    let mut changed = rides
        .read()
        .any(|answer| matches!(answer.result, Ok(RideDone::Stabled | RideDone::PutAway)));
    for answer in repairs.read() {
        if state.bike_repairs > 0 {
            state.bike_repairs -= 1;
            changed |= answer.result.is_ok();
        }
    }
    if changed
        && on_top(&stack)
        && let Some(with_store) = state.open.as_ref().map(|shown| shown.with_store)
    {
        pop_screen(&mut commands);
        open(&mut commands, with_store);
    }
}

/// `Update`: the sim's answer to a weapon taken in hand. A toast says it
/// and the screen is opened again over the truth, since the mirror is made
/// from clicks and a swap made behind it would leave the hands' rows and
/// the cells saying what they said before.
pub fn answer_takes(
    mut answers: MessageReader<EquipAnswer>,
    registries: Option<Res<Registries>>,
    state: Res<StoresState>,
    mut commands: Commands,
) {
    for answer in answers.read() {
        match &answer.result {
            Ok(taken) => {
                let name = registries
                    .as_ref()
                    .and_then(|registries| registries.0.items.get(&taken.item.item))
                    .map_or_else(|| "?".to_owned(), |def| def.name.clone());
                toast(
                    &mut commands,
                    ToastSpec::new(
                        english("strata.stores.take.done")
                            .unwrap_or_default()
                            .replace("{name}", &name),
                    )
                    .duration(Duration::from_millis(1600)),
                );
                if let Some(with_store) = state.open.as_ref().map(|shown| shown.with_store) {
                    pop_screen(&mut commands);
                    open(&mut commands, with_store);
                }
            }
            Err(refusal) => toast(
                &mut commands,
                ToastSpec::new(take_refusal_words(*refusal))
                    .level(ToastLevel::Warning)
                    .duration(Duration::from_millis(1600)),
            ),
        }
    }
}

/// `Update`: the chip of the filter in force is the lit one, and the one
/// with the mark before its word.
pub fn paint_chips(
    state: Res<StoresState>,
    mut chips: Query<&mut ButtonRole>,
    mut texts: Query<(&mut Themed, &mut LocText, &mut Text)>,
) {
    let Some(parts) = state.open.as_ref().and_then(|open| open.parts.as_ref()) else {
        return;
    };
    for (filter, chip, text) in &parts.chips {
        let on = *filter == state.filter;
        let role = if on { CHIP_ON_ROLE } else { CHIP_ROLE };
        if let Ok(mut family) = chips.get_mut(*chip)
            && family.0.as_str() != role
        {
            family.0 = Role::new_static(role);
        }
        let role = if on {
            CHIP_TEXT_ON_ROLE
        } else {
            CHIP_TEXT_ROLE
        };
        let Some((mut themed, mut loc, mut words)) = text.and_then(|text| texts.get_mut(text).ok())
        else {
            continue;
        };
        if themed.0.as_str() != role {
            themed.0 = Role::new_static(role);
        }
        let said = chip_words(*filter, on);
        if words.0 != said {
            loc.key = LocKey(said.clone());
            words.0 = said;
        }
    }
}

/// `Update`: the footer's line. The carried stack follows the pointer and
/// nothing else, so with the keys alone this line is where it is seen:
/// what is carried and how many. A refusal that belongs at no slot is said
/// here for a few seconds, and otherwise the line says how a stack is
/// sent.
pub fn write_status(
    time: Res<Time>,
    registries: Option<Res<Registries>>,
    context: Res<CardContext>,
    mut state: ResMut<StoresState>,
    carried: Query<&Carried>,
    mut texts: Query<(&mut LocText, &mut Text, &mut Themed)>,
) {
    let (Some(open), Some(registries)) = (state.open.as_mut(), registries) else {
        return;
    };
    let now = time.elapsed_secs_f64();
    if open.notice.as_ref().is_some_and(|(_, until)| *until <= now) {
        open.notice = None;
    }
    let Some(status) = open.parts.as_ref().map(|parts| parts.status) else {
        return;
    };
    let words = |key: &'static str| english(key).unwrap_or(key);
    if let Some((notice, _)) = &open.notice {
        write(&mut texts, status, notice, REFUSAL_ROLE);
        return;
    }
    let held = carried.get(open.menu).ok().and_then(|held| held.0.as_ref());
    match held {
        Some(stack) => {
            let name = card::stack_name(&registries.0, stack, context.against().as_ref());
            let line = words("strata.stores.carrying")
                .replace("{name}", &name)
                .replace("{count}", &stack.count.to_string());
            write(&mut texts, status, &line, CARRYING_ROLE);
        }
        None => write(
            &mut texts,
            status,
            words(if open.access == SaddleAccess::Mounted {
                "strata.stores.hint.saddle"
            } else if open.with_store {
                "strata.stores.hint"
            } else {
                "strata.stores.hint.pack"
            }),
            STATUS_ROLE,
        ),
    }
}

/// `Update`: the head's credits follow the sim's purse, which a draw at a
/// survey post or a later sale moves while the screen is up. Written when
/// the node says something else, so a head spawned this frame has it too.
pub fn write_purse(
    state: Res<StoresState>,
    purse: Option<Res<Purse>>,
    mut texts: Query<(&TestId, &mut LocText, &mut Text)>,
) {
    let (Some(_), Some(purse)) = (state.open.as_ref(), purse) else {
        return;
    };
    let words = grouped(purse.0);
    for (id, mut loc, mut text) in &mut texts {
        if id.0 == PURSE_ID && text.0 != words {
            loc.key = LocKey(words.clone());
            text.0.clone_from(&words);
        }
    }
}

/// `Update`: a chip sets the filter; the send button sends the pinned
/// stack across, as Shift and a click on its slot would.
#[allow(
    clippy::too_many_arguments,
    reason = "the choices, the screen, its cells, the mirror and the two inboxes"
)]
pub fn answer_choices(
    mut choices: MessageReader<MenuChoice>,
    mut state: ResMut<StoresState>,
    registries: Option<Res<Registries>>,
    slots: Query<(&SlotRef, &ItemView)>,
    menus: Query<&OpenMenu>,
    inventories: Query<&Inventory>,
    mut takes: ResMut<EquipInbox>,
    mut rides: ResMut<RideInbox>,
    mut repairs: ResMut<WearInbox>,
    stable: Option<Res<Stable>>,
    riding: Query<(), (With<LocalPlayer>, With<Riding>)>,
    locals: Query<&Combatant, With<LocalPlayer>>,
    mut commands: Commands,
) {
    for choice in choices.read() {
        if choice.screen != kind() {
            continue;
        }
        if choice.id == RIDE_REPAIR {
            repairs.push(WearRequest::RepairBike);
            state.bike_repairs += 1;
            continue;
        }
        if choice.id == RIDE_PUT {
            rides.push(RideRequest::PutAway);
            continue;
        }
        if let Some(id) = choice.id.strip_prefix(FILTER_TAG) {
            if let Some(filter) = Filter::ALL.into_iter().find(|filter| filter.id() == id)
                && state.filter != filter
            {
                state.filter = filter;
            }
            continue;
        }
        if choice.id == TAKE {
            let Some((open, registries)) = state.open.as_ref().zip(registries.as_ref()) else {
                continue;
            };
            let no_bike = Stable::default();
            let facts = RideFacts {
                stable: stable.as_deref().unwrap_or(&no_bike),
                riding: !riding.is_empty(),
                in_use: locals.single().map_or(0, Combatant::hand),
                ready: locals
                    .single()
                    .ok()
                    // No Combatant counts as ready, as the sim's consume has it.
                    .is_none_or(|actor| actor.action == Action::Ready),
            };
            match take_of(
                open,
                open.pinned,
                &registries.0.items,
                &facts,
                &slots,
                &menus,
                &inventories,
            ) {
                Some(Taking::Hand(kind, hand, Ok(()), _)) => {
                    takes.push(EquipRequest::Take { kind, hand });
                }
                Some(Taking::Ride(kind, Ok(()))) => rides.push(RideRequest::Stable { kind }),
                _ => {}
            }
            continue;
        }
        if choice.id != SEND {
            continue;
        }
        let Some(open) = state.open.as_ref() else {
            continue;
        };
        if let Some((slot, _)) = open.pinned.and_then(|entity| slots.get(entity).ok()) {
            commands.trigger(MenuAction {
                entity: open.menu,
                action: ClickAction::QuickMove { slot: slot.slot },
            });
        }
    }
}

/// `Update`: [`SEND_KEY`] sends the stack in the focused cell across, or
/// with the pointer in use the one under it and nothing when it is on no
/// cell, as Shift and a click on the cell would. The send button acts on the pinned stack, and the pin moves
/// with the focus on its way to the button; the key acts on the cell the
/// player is on.
///
/// Which device is in use is told by what moved last, the pointer or the
/// focus, and not by slotted's input mode: the key's own press puts that
/// in keyboard mode before this runs.
#[allow(
    clippy::too_many_arguments,
    reason = "the key, the screen, the pointer and its motion, the focus and the cells"
)]
pub fn send_focused(
    keys: Res<ButtonInput<KeyCode>>,
    stack: Res<ScreenStack>,
    mut cursor: MessageReader<CursorMoved>,
    mut inputs: MessageReader<PointerInput>,
    focus: Option<Res<InputFocus>>,
    state: Res<StoresState>,
    slots: Query<(&SlotRef, &ItemView)>,
    pointed: Query<(Entity, &Hovered), With<SlotRef>>,
    mut by_pointer: Local<bool>,
    registries: Option<Res<Registries>>,
    menus: Query<&OpenMenu>,
    inventories: Query<&Inventory>,
    locals: Query<&Combatant, With<LocalPlayer>>,
    mut takes: ResMut<EquipInbox>,
    mut commands: Commands,
) {
    // A window reports its cursor, a harness only the pointer's own input.
    let moved = cursor.read().count()
        + inputs
            .read()
            .filter(|input| match input.action {
                PointerAction::Move { delta } => delta != Vec2::ZERO,
                _ => false,
            })
            .count();
    if moved > 0 {
        *by_pointer = true;
    } else if focus.as_ref().is_some_and(DetectChanges::is_changed) {
        *by_pointer = false;
    }
    if !keys.just_pressed(SEND_KEY) || !on_top(&stack) {
        return;
    }
    let Some(menu) = state.menu() else {
        return;
    };
    // With the pointer in use and on no cell the key sends nothing: the
    // cell the focus was left on is not one the player is looking at.
    let cell = if *by_pointer {
        pointed
            .iter()
            .find(|(_, hovered)| hovered.get())
            .map(|(entity, _)| entity)
    } else {
        focus.as_deref().and_then(InputFocus::get)
    };
    if let Some((slot, _)) = cell
        .and_then(|entity| slots.get(entity).ok())
        .filter(|(slot, view)| slot.menu == menu && view.stack.is_some())
    {
        // With no store open there is nothing to send a weapon across to
        // but a bag, so the key takes it in hand, from anywhere.
        if let Some((open, registries)) = state.open.as_ref().zip(registries.as_ref())
            && !open.with_store
            && open.access != SaddleAccess::Mounted
            && !open
                .layout
                .saddle
                .is_some_and(|range| range.contains(slot.slot))
        {
            let facts = RideFacts {
                stable: &Stable::default(),
                riding: false,
                in_use: locals.single().map_or(0, Combatant::hand),
                ready: locals
                    .single()
                    .ok()
                    // No Combatant counts as ready, as the sim's consume has it.
                    .is_none_or(|actor| actor.action == Action::Ready),
            };
            if let Some(Taking::Hand(kind, hand, _, _)) = take_of(
                open,
                cell,
                &registries.0.items,
                &facts,
                &slots,
                &menus,
                &inventories,
            ) && registries.0.items.family_of(&kind).is_some()
            {
                // The sim answers a refusal and the answer is toasted.
                takes.push(EquipRequest::Take { kind, hand });
                return;
            }
        }
        commands.trigger(MenuAction {
            entity: menu,
            action: ClickAction::QuickMove { slot: slot.slot },
        });
    }
}

// ---- what slotted leaves unsaid ----

/// Observer on `MenuAction`: a click the rules will refuse is refused by
/// slotted without a word, so before slotted has it the sim's own `apply`
/// is asked on a copy of the mirror, and what it says is the matter is
/// shown at the slot, or in the footer's line for a cell of a grid, which
/// has no caption: a stack sent across that found no room there. Nothing is
/// decided here: the copy is thrown away and the click goes on to slotted
/// and the sim as it would have.
///
/// A plain click on a weapon in hand, with nothing carried, is no attempt
/// to carry it off: it opens the weapon's fitting, which is where a weapon
/// in hand is worked on, or read when no workbench is near.
#[allow(
    clippy::too_many_arguments,
    reason = "the click, the screen, the mirror and the two inboxes it answers to"
)]
fn say_why(
    action: On<MenuAction>,
    time: Res<Time>,
    registries: Option<Res<Registries>>,
    mut state: ResMut<StoresState>,
    menus: Query<&OpenMenu>,
    inventories: Query<&Inventory>,
    mut takes: ResMut<EquipInbox>,
    mut commands: Commands,
) {
    let (Some(open), Some(registries)) = (state.open.as_mut(), registries) else {
        return;
    };
    if action.entity != open.menu {
        return;
    }
    let Ok(menu) = menus.get(open.menu) else {
        return;
    };
    // A weapon carried onto a hand is taken in it. The rules refuse the
    // click itself, so the take is a request of its own and its answer
    // is the sim's.
    if let ClickAction::Pickup {
        slot,
        button: Button::Left,
    } = action.action
        && open.layout.hands.contains(slot)
        && let Some(carried) = menu.state.carried.as_ref()
        && let Ok(record) = items::read(carried, &registries.0.items)
        && registries.0.items.family_of(&kind_of(&record)).is_some()
    {
        takes.push(EquipRequest::Take {
            kind: kind_of(&record),
            hand: usize::from(slot.0 - open.layout.hands.start),
        });
        return;
    }
    if let ClickAction::Pickup {
        slot,
        button: Button::Left,
    } = action.action
        && open.layout.hands.contains(slot)
        && menu.state.carried.is_none()
    {
        let hand = usize::from(slot.0 - open.layout.hands.start);
        super::fitting::open(&mut commands, Some(hand));
        return;
    }
    let Some(mut stock) = mirrored(open, menu.state.carried.clone(), &inventories) else {
        return;
    };
    let mut scratch = menu.state.clone();
    let access = open.access;
    let refused = items::apply(
        &registries.0.items,
        &mut Stock {
            saddle: open.layout.saddle.map(|_| items::SaddleStock {
                cells: &mut stock.saddle,
                access,
            }),
            pack: &mut stock.pack,
            hands: &mut stock.hands,
            store: stock.store.as_mut(),
        },
        &mut scratch,
        action.action,
    )
    .err();
    let (ClickAction::Pickup { slot, .. }
    | ClickAction::QuickMove { slot }
    | ClickAction::PickupAll { slot, .. }) = action.action
    else {
        return;
    };
    let slot = Some(slot).filter(|slot| !slot.is_outside());
    if let Some(words) =
        refused.and_then(|reason| refusal_words(reason, &open.layout, open.access, slot))
    {
        open.say(slot, words, time.elapsed_secs_f64());
    }
}

/// Observer on `FocusedAction`: the secondary action on a focused cell is
/// a right click, so the keys and a pad take half a stack and put one
/// down as the mouse does. Slotted gives the action no meaning on a slot.
fn secondary_is_a_right_click(
    action: On<FocusedAction>,
    state: Res<StoresState>,
    slots: Query<&SlotRef>,
    mut claims: ResMut<UiActionClaims>,
    mut commands: Commands,
) {
    if action.action != UiAction::Secondary || action.repeat {
        return;
    }
    let Some(menu) = state.menu() else {
        return;
    };
    if !slots.get(action.entity).is_ok_and(|slot| slot.menu == menu) {
        return;
    }
    claims.claim(UiAction::Secondary);
    commands.trigger(SlotClicked {
        entity: action.entity,
        button: Button::Right,
        modifiers: Modifiers::NONE,
    });
}

/// Registers the authority, the state, the observers and the systems.
/// Only called once the UI is wired.
pub(crate) fn register(app: &mut bevy::app::App) {
    let link = StockAuthority::default();
    app.insert_resource(slotted::ecs::Authority::new(link.clone()))
        .insert_resource(link)
        .init_resource::<StoresState>()
        .init_resource::<PicturedIcons>()
        .init_resource::<card::ItemImages>()
        .add_observer(on_menu_closed)
        .add_observer(say_why)
        .add_observer(secondary_is_a_right_click)
        .add_systems(
            bevy::app::Update,
            (
                (
                    follow_registries,
                    hush_raised_card,
                    toggle,
                    open_pending,
                    answer_choices,
                    send_focused,
                    answer_takes,
                    answer_rides,
                )
                    .in_set(super::ScreenWork),
                feed_answers.before(SlottedEcsSet::Reconcile),
                forward_requests
                    .after(SlottedEcsSet::Submit)
                    .before(SlottedEcsSet::Reconcile),
                (
                    follow_stock,
                    find_parts,
                    show_bags,
                    apply_wanted,
                    pin_card,
                    show_send,
                    show_take,
                    show_ride,
                    paint_chips,
                    write_status,
                    write_purse,
                )
                    .chain()
                    .after(SlottedEcsSet::Reconcile)
                    .before(SlottedUiSet::Render),
                dress_slots.after(SlottedUiSet::Render),
            ),
        );
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeSet;
    use strata_registry::GameRegistries;

    fn shipped() -> GameRegistries {
        let assets = concat!(env!("CARGO_MANIFEST_DIR"), "/../../assets");
        strata_registry::load_from_dir(assets).expect("assets/ loads")
    }

    #[test]
    fn take_in_hand_names_the_weapon_it_puts_away() {
        let registries = shipped();
        let items = &registries.items;
        let record = |name: &str| {
            let def = items
                .get(&name.parse().expect("an id"))
                .expect("a shipped weapon");
            strata_registry::item::Record {
                item: def.id.clone(),
                count: 1,
                batch: 0,
                by: None,
                lines: std::collections::BTreeMap::new(),
                condition: None,
                fitted: std::collections::BTreeMap::new(),
                origin: None,
            }
        };
        let hands = [None, Some(record("veyra:weapon.service_pistol"))];
        assert_eq!(take_words(items, &hands, 0), "Take in hand 1");
        assert_eq!(
            take_words(items, &hands, 1),
            "Swap out Kestrel service pistol"
        );
    }

    #[test]
    fn a_second_weapon_of_a_family_is_refused_in_words() {
        assert_eq!(
            take_refusal_words(EquipRefusal::SameFamily),
            "The other hand holds one of those."
        );
    }

    #[test]
    fn the_gear_chip_shows_a_field_harvester() {
        let registries = shipped();
        let harvester = registries
            .items
            .iter()
            .find(|def| matches!(def.kind, ItemKind::Harvester { .. }))
            .expect("the shipped harvester");
        assert!(Filter::Gear.shows(&harvester.kind));
        assert!(!Filter::Kits.shows(&harvester.kind));
    }

    fn definition(with_store: bool) -> ScreenDef {
        let registries = shipped();
        let items = &registries.items;
        let pack = Pack::empty(items.carry());
        let layout = items::layout(items.carry(), items, &pack, with_store, None);
        screen_def(&StoresView {
            with_store,
            who: "Frontier ranger",
            carry: items.carry(),
            layout: &layout,
            hands: std::array::from_fn(|hand| HandRow {
                name: Some(format!("Weapon {hand}")),
                fitted: "Nothing fitted".to_owned(),
                condition: Some(1000),
                in_hand: hand == 0,
                picture: None,
            }),
            cell: (44., 6.),
            purse: 2500,
            ride: None,
        })
    }

    /// A hand's row draws its weapon at the weapon's own shape, held by
    /// the row's height or by the widest a row lets it be, and draws none
    /// when nothing is baked.
    #[test]
    fn a_hands_row_draws_the_weapon_at_its_own_shape() {
        let registries = shipped();
        let items = &registries.items;
        let pack = Pack::empty(items.carry());
        let layout = items::layout(items.carry(), items, &pack, true, None);
        let row_with = |picture: Option<(String, u32, u32)>| HandRow {
            name: Some("A weapon".to_owned()),
            fitted: "Nothing fitted".to_owned(),
            condition: Some(1000),
            in_hand: true,
            picture,
        };
        let sizes = |rows: [HandRow; HANDS]| {
            let mut def = screen_def(&StoresView {
                with_store: true,
                who: "Frontier ranger",
                carry: items.carry(),
                layout: &layout,
                hands: rows,
                cell: (44., 6.),
                purse: 2500,
                ride: None,
            });
            [0, 1].map(|hand| match def.root.find_mut(&hand_picture_id(hand)) {
                Some(UiNodeDef::Image {
                    width: Length::Px(wide),
                    height: Length::Px(high),
                    ..
                }) => Some((*wide, *high)),
                None => None,
                other => panic!("{other:?}"),
            })
        };
        let long = ("generated://long.png".to_owned(), 1000, 250);
        let short = ("generated://short.png".to_owned(), 300, 300);
        let [carbine, pistol] = sizes([row_with(Some(long)), row_with(Some(short))]);
        let (wide, high) = carbine.expect("the carbine's picture");
        assert!((wide - HAND_PICTURE_WIDTH).abs() < 0.01 && (high - 30.).abs() < 0.01);
        let (wide, high) = pistol.expect("the pistol's picture");
        assert!(
            (wide - HAND_PICTURE_HEIGHT).abs() < 0.01 && (high - HAND_PICTURE_HEIGHT).abs() < 0.01
        );
        assert_eq!(sizes([row_with(None), row_with(None)]), [None, None]);
        // The pack's own screen has a narrow column for the hands, and
        // draws none there.
        let narrow = items::layout(items.carry(), items, &pack, false, None);
        let mut def = screen_def(&StoresView {
            with_store: false,
            who: "Frontier ranger",
            carry: items.carry(),
            layout: &narrow,
            hands: std::array::from_fn(|_| {
                row_with(Some(("generated://long.png".to_owned(), 1000, 250)))
            }),
            cell: (44., 6.),
            purse: 2500,
            ride: None,
        });
        assert!(def.root.find_mut(&hand_picture_id(0)).is_none());
    }

    #[test]
    fn every_role_the_screen_names_is_in_the_theme() {
        let mut roles: BTreeSet<String> = [
            REFUSAL_ROLE,
            CARRYING_ROLE,
            CHIP_ON_ROLE,
            CHIP_TEXT_ON_ROLE,
            PIP_ROLE,
            PIP_FILL_ROLE,
            PIP_CAP_ROLE,
            CONDITION_ROLE,
            CONDITION_FILL_ROLE,
        ]
        .into_iter()
        .map(str::to_owned)
        .collect();
        let mut keys = Vec::new();
        for def in [definition(true), definition(false)] {
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
                _ => {}
            });
        }
        for state in [".hover", ".focus", ".pressed"] {
            roles.insert(format!("{CHIP_ON_ROLE}{state}"));
        }
        let path = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../assets/themes/veyra.theme.ron"
        );
        let theme =
            slotted::theme::Theme::from_ron(&std::fs::read_to_string(path).expect("the theme"))
                .expect("the theme parses");
        let ours: Vec<&String> = roles.iter().filter(|r| r.starts_with("strata.")).collect();
        assert!(ours.len() > 16, "{ours:?}");
        for role in roles {
            assert!(
                theme.roles.contains_key(&Role::new(role.clone())),
                "{role} is not in the theme"
            );
        }
        // Every key the definition names has English, and no sentence is
        // left as a key.
        for key in keys.iter().filter(|key| key.starts_with("strata.")) {
            assert!(english(key).is_some(), "{key} has no English");
        }
    }

    #[test]
    fn the_definition_is_the_store_beside_the_pack_or_the_pack_alone() {
        let registries = shipped();
        let carry = registries.items.carry();
        let grids = |def: &ScreenDef| {
            let mut found = Vec::new();
            def.root.walk(&mut |node| {
                if let UiNodeDef::SlotGrid {
                    cols,
                    rows,
                    first,
                    tags,
                    ..
                } = node
                {
                    found.push((
                        tags.get("region").unwrap_or_default().to_owned(),
                        *cols,
                        *rows,
                        *first,
                    ));
                }
            });
            found
        };
        let with = definition(true);
        assert_eq!(with.initial_focus.as_deref(), Some(STORE_ID));
        assert_eq!(
            grids(&with),
            vec![
                ("store".to_owned(), carry.store.cols, carry.store.rows, 46),
                ("pack".to_owned(), carry.pack.cols, carry.pack.rows, 0),
                ("bag0".to_owned(), 6, 2, 20),
                ("bag1".to_owned(), 6, 2, 32),
            ],
            "the grids are the carrying rules' and start where the sim's layout says"
        );
        let without = definition(false);
        assert_eq!(without.initial_focus.as_deref(), Some(PACK_ID));
        assert_eq!(grids(&without).len(), 3, "no store under key I");
        let mut without = without;
        assert!(without.root.find_mut(STORE_ID).is_none());
        for id in [
            ROOT_ID.to_owned(),
            CARD_ID.to_owned(),
            SEND.to_owned(),
            STATUS_ID.to_owned(),
            bag_slot_id(1),
            bag_caption_id(0),
            hand_slot_id(1),
            hand_fitted_id(0),
            Filter::Bags.chip_id(),
            Filter::Lots.chip_id(),
            PURSE_ID.to_owned(),
        ] {
            assert!(without.root.find_mut(&id).is_some(), "{id}");
        }
    }

    #[test]
    fn a_refusal_has_words_and_a_click_on_nothing_has_none() {
        let registries = shipped();
        let items = &registries.items;
        let pack = Pack::empty(items.carry());
        let with = items::layout(items.carry(), items, &pack, true, None);
        let without = items::layout(items.carry(), items, &pack, false, None);
        let said = |reason| refusal_words(reason, &with, SaddleAccess::Shut, None);
        assert_eq!(
            said(Refusal::BagHolds { stacks: 3 }).as_deref(),
            Some("Holds 3: empty the bag to take it out")
        );
        assert_eq!(
            said(Refusal::HandFixed).as_deref(),
            Some("A hand takes a weapon. Carry one here, or press T on it.")
        );
        assert!(said(Refusal::NotABag).is_some());
        assert!(said(Refusal::NoLocker).is_some());
        assert!(said(Refusal::NoDropping).is_some());
        assert_eq!(said(Refusal::Unsupported), None);
        assert_eq!(
            said(Refusal::Click(slotted::model::ClickError::NothingToDo)),
            None
        );
        // A send with no room names the side that is full: the one the
        // sim's routes send a stack at that slot to.
        let no_room = |layout: &StockLayout, slot: u16| {
            refusal_words(
                Refusal::NoRoom,
                layout,
                SaddleAccess::Shut,
                Some(SlotIx(slot)),
            )
        };
        let store = with.store.expect("opened over the store").start;
        let (pack, bag) = (with.pack.start, with.bags[0].start);
        assert_eq!(
            no_room(&with, store).as_deref(),
            Some("No room in the pack")
        );
        assert_eq!(
            no_room(&with, pack).as_deref(),
            Some("No room in the store")
        );
        assert_eq!(no_room(&with, bag).as_deref(), Some("No room in the store"));
        assert_eq!(no_room(&without, pack).as_deref(), Some("No room in a bag"));
        assert_eq!(
            no_room(&without, bag).as_deref(),
            Some("No room in the pack")
        );
        assert!(Filter::Mods.shows(&ItemKind::Module {
            slot: "feed".to_owned(),
            fits: Vec::new(),
            part: String::new(),
            effects: Vec::new(),
        }));
        assert!(!Filter::Mods.shows(&ItemKind::Bag { cells: 6 }));
        assert!(Filter::All.shows(&ItemKind::Bag { cells: 6 }));
        // The Lots chip shows every lot and nothing else.
        for def in items.iter() {
            assert_eq!(
                Filter::Lots.shows(&def.kind),
                matches!(def.kind, ItemKind::Lot { .. }),
                "{}",
                def.id
            );
        }
        assert_eq!(Filter::ALL[1], Filter::Lots, "the chip after All");
    }
}
