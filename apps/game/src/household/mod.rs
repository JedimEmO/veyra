//! The household: the bench of characters the player can play, who is
//! out, who can still join, what each carries and what the household
//! stores, and the bench places, trainers, lockers and workbenches in the
//! world where any of that changes (progression design, section 4; ticket
//! 0007).
//!
//! The household is a resource of the game, not of the sim: the sim knows
//! one local player with one sheet, one pack and one store, and a swap at
//! a terminal rewrites that player's sheet, weapon, pack and body. It is
//! saved as RON to `--household` (default `scratch/household.ron`) on
//! every change and read back at startup, which makes it
//! singleplayer-local: nothing here is replicated and the protocol does
//! not carry it. A connected client has no household.
//!
//! Stock is saved by name ([`Record`]), since the sim's stacks hold ids
//! only good for the run that froze them. The sim owns it while the game
//! runs (`strata_sim::items`: the player's `Pack` and `Hands`, the
//! `HouseholdStore`) and [`track_stock`] copies every change here, as
//! [`track_sheet`] does the sheet. A file from before items existed is
//! upgraded and kept beside the new one, and a file this build cannot
//! read is copied aside before the first day is written over it, so no
//! change of shape costs a played household.
//!
//! The screens are the client's (`strata_client::ui::bench` and
//! `::sheet`): the game publishes the household as a [`BenchView`] and
//! answers each [`BenchRequest`] a Switch or Join button sends, and the
//! sheet screen writes the local player's sheet, which [`track_sheet`]
//! copies here. `--script-progression` walks the path without a key or a
//! screen, learning and swapping directly, which the headless test at the
//! bottom runs; with `--open-sheet` or `--open-bench` it opens the screen
//! at the trainer or the terminal instead, for a screenshot. With
//! `--script-hurt 1` the script also has the
//! nearest hostile land one hit on the player early in the fight
//! ([`script_hurt`]), which no scripted run otherwise does, so the HUD's
//! hurt path can be captured and tested; a larger number lands that many,
//! a second apart, which brings the low-health pulse and then the fall.
//! `--script-crafting` adds a stop at the workbench after the lesson
//! ([`script_bench`]): the carbine is drawn and fired, the store's patrol
//! magazine fitted by request, the weapon reloaded and fired again, and
//! the magazine taken off, with what the `Combatant` read at each step
//! written to `--script-report`. From there it goes to the middle of the
//! nearest deposit ([`script_survey`]): the tool pings, a lot is sampled by
//! request, read in the pack and carried back to the locker's store.
//!
//! The household has a purse and a survey record too (ticket 0007, step
//! 3): its credits, the deposits it has found and the one it tracks.
//! The sim owns them while the game runs (`strata_sim::deposits`) and
//! [`track_stock`] copies them here. The first day's store holds lots
//! drawn from the real deposits nearest the start ([`grants`]), and a file
//! from before the purse is given the same lots and the purse once. The
//! survey record is of the world it was made in ([`SurveyedIn`]): started
//! in another, it is left in the file as it is and the sim is handed none
//! of it ([`survey_record`]).
//!
//! And it has a bench's work (ticket 0007, step 4): the counter rolls are
//! drawn from, the batch counter, the session left on the bench and what
//! each finished batch was made from. The sim owns that too while the game
//! runs (`strata_sim::crafting`), so a session opened and walked away from
//! is on the bench at the next start with its materials committed. A file
//! from before it is given the first day's bought parts once. With
//! `--script-crafting` the stop at the workbench goes on to make a cell
//! core from the first lots, the patrol magazine from that, fit it, and
//! make a second core from the same lots ([`script_craft`]).
//!
//! And a fabricator's (ticket 0007, step 5): the blueprints it has
//! recorded and the run its fabricator is on, which are the sim's while the
//! game runs (`strata_sim::blueprint`, `::factory`) and copied here like
//! the rest, so a run is there at the next start where it was left, a unit
//! under way begun again. Who works a run is a benched member by name: the
//! game tells the sim who is on the bench ([`publish_benched`]) and refuses
//! a switch to whoever is working ([`swap_to`]). A file from before it has
//! no blueprint and no run. With `--script-crafting` the second core is
//! recorded as a blueprint and a batch of three run off it at the
//! fabricator while the rest of the path is walked ([`script_factory`]).

#![allow(
    clippy::wildcard_imports,
    reason = "the files are one module split for size, and each sees what the others own"
)]

use bevy::prelude::*;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use strata::client::combat_art::BodyTint;
use strata::client::ui::bench::{BenchCard, BenchOutcome, BenchRequest, BenchView, BenchWork};
use strata::client::ui::progress::ProgressCue;
use strata::core::WorldPos;
use strata::physics::avian3d::prelude::{SpatialQuery, SpatialQueryFilter};
use strata::prelude::*;
use strata::registry::GameRegistries;
use strata::registry::item::LotOf;
use strata::registry::item::{
    Fitted, HANDS, ItemCatalogue, ItemKind, Record, STORE_GRADE, SavedPack,
};
use strata::registry::progression::{HouseholdDef, MemberDef, SheetDef, SkillGraph};
use strata::registry::schematic::Hand;
use strata::registry::{Attribute, Reading, ResourceSet};
use strata::sim::ability::Bar;
use strata::sim::blueprint::{self, Blueprints};
use strata::sim::combat::{Action, CombatEncounter, CombatIntent, Combatant, Weapon, WeaponSpec};
use strata::sim::crafting::{
    self, CraftAnswer, CraftDone, CraftInbox, CraftRequest, CraftState, Holder,
};
use strata::sim::deeds::{CommonsTaken, Deeds, Leases};
use strata::sim::deposits::{
    self, Deposit, DepositId, Found, NearDeposits, PingAnswer, PingDone, Purse, SampleAnswer,
    SampleDone, Survey, SurveyInbox, SurveyRequest, Surveyed,
};
use strata::sim::encounters::{Cleared, PlayedClock};
use strata::sim::equip::{EquipAnswer, EquipInbox, EquipRequest};
use strata::sim::factory::{
    self, Benched, FactoryAnswer, FactoryDone, FactoryInbox, FactoryNotice, FactoryRequest,
    FactoryState,
};
use strata::sim::fitting::{self, FitAnswer, FitInbox, FitRequest};
use strata::sim::harvest::Harvesters;
use strata::sim::items::{
    self, Hands, HouseholdStore, OpenStock, Pack, Stock, StockInbox, StockRequest,
};
use strata::sim::loot::Caches;
use strata::sim::postings::{Jobs, Known};
use strata::sim::procurement::ProcurementState;
use strata::sim::progression::{CharacterSheet, Interact, Interaction, InteractionKind, Reach};
use strata::sim::progression::{LearnRefused, XpGained};
use strata::sim::rework::{ReworkAnswer, ReworkInbox, ReworkRequest};

#[macro_use]
mod bench;
mod cache;
mod fall;
mod harvesters;
mod props;
mod save;
mod script;
mod script_crafting;
mod shots;
#[cfg(test)]
mod tests;

use self::bench::*;
use self::harvesters::*;
use self::props::*;
use self::save::*;
use self::script::*;
use self::script_crafting::*;
use self::shots::*;

pub(crate) use self::props::{
    bench_view, board_view, foreman_view, ground_at, pelagic_works, sites, survey_view,
    terminal_view,
};
pub(crate) use self::script::{ProgressionScript, drive_script, short_of_gate};
pub(crate) use self::shots::{BoardShot, FactoryShot, JoinShot, PoorLotShot};

/// The most characters a household holds (the design's bench of four).
pub(super) const BENCH: usize = 4;

/// How near a character stands to use a prop, metres.
const REACH_M: f32 = 2.5;

/// One character of the household: what they know, what they carry and
/// where they were left. The sheet is the sim's own component type, so
/// what is saved is what the sim read.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub(super) struct Character {
    /// Shown on the HUD.
    pub name: String,
    /// Boxes learned and XP earned.
    pub sheet: CharacterSheet,
    /// The weapon in hand when they were last played.
    pub weapon: Weapon,
    /// Rounds in that weapon's magazine.
    pub ammo: u16,
    /// Rounds in the holstered one's.
    pub holstered_ammo: u16,
    /// The body's colour, an index into the view's palette.
    pub tint: u8,
    /// Where they were left when the player switched away; `None` for a
    /// character never played, who arrives at the bench place.
    pub left_at: Option<WorldPos>,
    /// What they carry, slot for slot.
    pub pack: SavedPack,
    /// The two weapons they carry, any two of different families; which of
    /// the two is in hand is `weapon`. `None` only in a data set with no
    /// items to hold.
    pub hands: [Option<Record>; HANDS],
    /// Where their abilities sit on the bar, kept by the sim and copied here
    /// on every change. A file from before the bar reads as an empty one,
    /// which the sim fills from what their boxes grant.
    #[serde(default)]
    pub bar: Bar,
    /// The played second a fall's injury ends (ticket 0012, step 4a), on
    /// `Household.played`'s clock; none when they are well. Added with a
    /// serde default and no version change.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub injured_until: Option<f64>,
}

/// What `name` carries and holds on the first day, from the items'
/// `starting.ron`: nothing at all in a data set without one.
fn starting_stock(name: &str, items: &ItemCatalogue) -> (SavedPack, [Option<Record>; HANDS]) {
    match items.starting() {
        Some(starting) => (
            starting.packs.get(name).cloned().unwrap_or_default(),
            starting.hands.clone().map(Some),
        ),
        None => (SavedPack::default(), std::array::from_fn(|_| None)),
    }
}

/// The survey tool of the first day's packs, if the starting file gives one:
/// what a household saved before it is given.
fn starting_tool(items: &ItemCatalogue) -> Option<Record> {
    items
        .starting()?
        .packs
        .values()
        .flat_map(|pack| pack.cells.iter().chain(pack.bags.iter().flatten()))
        .flatten()
        .find(|record| {
            matches!(
                items.get(&record.item).map(|def| &def.kind),
                Some(ItemKind::Tool { .. })
            )
        })
        .cloned()
}

/// The household's store on the first day, filled from its first cell.
fn starting_store(items: &ItemCatalogue) -> Vec<Option<Record>> {
    items
        .starting()
        .map(|starting| starting.store.iter().cloned().map(Some).collect())
        .unwrap_or_default()
}

/// What a household is given beside the items' first day: the lots drawn
/// from the world's own deposits, and the purse. Apart from the registries
/// because a lot's figures are its deposit's, which only the world knows.
#[derive(Debug, Clone, PartialEq, Default)]
pub(super) struct Grants {
    /// The first lots, in the starting file's order.
    pub lots: Vec<Record>,
    /// The deposits those lots were drawn from, which the household has
    /// found and sampled from the first day.
    pub deposits: Vec<Deposit>,
    /// The first day's credits.
    pub purse: u32,
    /// How many of the starting file's lots found no deposit to be drawn
    /// from: an upgrade that would give fewer lots than the file names is
    /// not written, so a later start can finish it.
    pub short: usize,
}

/// Whether `deposit` is what a first lot asks for.
fn is_of(set: &ResourceSet, deposit: &Deposit, of: &LotOf) -> bool {
    match of {
        LotOf::Type(wanted) => set.get(deposit.ty).is_some_and(|def| def.id == *wanted),
        LotOf::Class(wanted) => set.class_of(deposit.ty).id == *wanted,
    }
}

/// The first day's grants: the starting file's purse, and each of its lots
/// drawn from the nearest deposit to `from` that is of its kind, within
/// the file's distance. One that finds no deposit is said and counted;
/// without a survey (a data set with no resources) none is found.
///
/// The deposits are asked for in rings that double from two cells of the
/// lattice out to the file's distance, and no further than the last lot
/// needs: the nearest of each kind is a kilometre or two off, and the
/// whole distance is thousands of cells sampled on the thread that starts
/// the game.
pub(super) fn grants(
    registries: &GameRegistries,
    survey: Option<&Survey>,
    from: (f64, f64),
) -> Grants {
    let Some(starting) = registries.items.starting() else {
        return Grants::default();
    };
    let set = &registries.resources;
    let mut found: Vec<Option<(Deposit, Record)>> = vec![None; starting.lots.len()];
    if let Some(survey) = survey.filter(|_| !starting.lots.is_empty()) {
        let most = starting.lots_within_m;
        let mut radius = (2.0 * set.rules().lattice.cell_m).min(most);
        loop {
            // Nearest first, so the first that fits is the nearest that
            // does, and a ring that holds one holds the nearest of all.
            let near = survey.0.near(from.0, from.1, radius);
            for (wanted, lot) in starting.lots.iter().zip(&mut found) {
                if lot.is_some() {
                    continue;
                }
                let drawn = near
                    .iter()
                    .find(|deposit| is_of(set, deposit, &wanted.of))
                    .and_then(|deposit| {
                        Some((deposit, deposits::lot(deposit, set, wanted.units)?))
                    });
                if let Some((deposit, record)) = drawn {
                    info!(
                        "household: the first lot of {} is {} units of {}, {:.0} m off",
                        wanted.of,
                        wanted.units,
                        deposit.lot_name(set),
                        deposit.distance_m(from.0, from.1)
                    );
                    *lot = Some((deposit.clone(), record));
                }
            }
            if radius >= most || found.iter().all(Option::is_some) {
                break;
            }
            radius = (radius * 2.0).min(most);
        }
    }
    for (wanted, lot) in starting.lots.iter().zip(&found) {
        if lot.is_none() {
            warn!(
                "household: no deposit of {} within {:.0} m of the start; that lot is left out",
                wanted.of, starting.lots_within_m
            );
        }
    }
    let (deposits, lots): (Vec<Deposit>, Vec<Record>) = found.into_iter().flatten().unzip();
    Grants {
        short: starting.lots.len() - lots.len(),
        lots,
        deposits,
        purse: starting.purse,
    }
}

impl Character {
    /// A character as the files authored them: a full magazine in the
    /// first weapon their boxes certify, the carbine when neither is, and
    /// the first day's pack and weapons.
    fn fresh(member: &MemberDef, registries: &GameRegistries) -> Self {
        let sheet = CharacterSheet::authored(&member.sheet);
        let mut actor = Combatant::default();
        actor.fit(&registries.progression, &sheet);
        if !actor.certified(Weapon::Carbine) && actor.certified(Weapon::Pistol) {
            actor.weapon = Weapon::Pistol;
            std::mem::swap(&mut actor.ammo, &mut actor.holstered_ammo);
        }
        let (pack, hands) = starting_stock(&member.name, &registries.items);
        Self {
            name: member.name.clone(),
            sheet,
            weapon: actor.weapon,
            ammo: actor.ammo,
            holstered_ammo: actor.holstered_ammo,
            tint: member.tint,
            left_at: None,
            pack,
            hands,
            bar: Bar::default(),
            injured_until: None,
        }
    }

    /// The actor this character is when they walk in: full pools on their
    /// own rows, their weapon in hand with the rounds they left in it, and
    /// equipped with what is fitted to their weapons. The sim equips an
    /// actor whose hands change, but an actor made anew over hands that
    /// did not (a reset of the encounter) would stand unequipped, so it is
    /// done here, where every such actor is made. With nothing fitted and no
    /// signature line the actor is not equipped at all.
    ///
    /// The rounds remembered are clamped to the magazines as they are now:
    /// they were counted when the character last left, and a module taken
    /// off since made the magazine smaller than the count.
    pub(super) fn actor(&self, graph: &SkillGraph, items: &ItemCatalogue) -> Combatant {
        let mut actor = Combatant::default();
        actor.fit(graph, &self.sheet);
        let fitted = fitting::loadout(items, &self.hands);
        if !fitted.is_empty() {
            actor.equip(&fitted);
        }
        // The slots are what the hands hold; the one in hand is the one the
        // character left with, or the first.
        let slots = self.hands.each_ref().map(|hand| {
            hand.as_ref()
                .and_then(|record| fitting::weapon_of(items, record))
        });
        let hand = slots
            .iter()
            .position(|slot| *slot == Some(self.weapon))
            .unwrap_or(0);
        let mut actor = actor.carrying(slots, hand);
        // The bar too, for the same reason: a reset builds the actor anew and
        // nothing changes the sheet to copy it in again.
        actor.set_bar(&self.bar);
        actor.ammo = self.ammo.min(actor.spec_of(actor.weapon).magazine);
        actor.holstered_ammo = self.holstered_ammo.min(
            actor
                .holstered()
                .map_or(0, |weapon| actor.spec_of(weapon).magazine),
        );
        actor
    }

    /// The pack and the weapons this character walks in with, as the
    /// sim's components. The save was checked when it was read and every
    /// write to it since came from the sim, so a record that does not
    /// stand is a bug: it is said, and the character carries nothing,
    /// rather than stopping the game at a spawn.
    fn carried(&self, items: &ItemCatalogue) -> (Pack, Hands) {
        let pack = Pack::from_saved(&self.pack, items).unwrap_or_else(|error| {
            error!("household: {}'s pack does not stand: {error}", self.name);
            Pack::empty(items.carry())
        });
        let hands = Hands::from_saved(&self.hands, items).unwrap_or_else(|error| {
            error!("household: {}'s weapons do not stand: {error}", self.name);
            Hands::default()
        });
        (pack, hands)
    }

    /// Remember what the actor is and holds, for the bench and the file.
    fn take_from(&mut self, actor: &Combatant, sheet: &CharacterSheet, at: WorldPos) {
        self.sheet = sheet.clone();
        self.weapon = actor.weapon;
        self.ammo = actor.ammo;
        self.holstered_ammo = actor.holstered_ammo;
        self.left_at = Some(at);
    }

    /// What the sim's pack and weapons are as the save writes them. A
    /// stack the catalogue cannot read, which no click produces, is said
    /// and gives nothing, so what was remembered is kept.
    fn stock_of(
        &self,
        pack: &Pack,
        hands: &Hands,
        items: &ItemCatalogue,
    ) -> Option<(SavedPack, [Option<Record>; HANDS])> {
        match (pack.saved(items), hands.saved(items)) {
            (Ok(pack), Ok(hands)) => Some((pack, hands)),
            (Err(error), _) | (_, Err(error)) => {
                error!("household: {}'s stock does not read: {error}", self.name);
                None
            }
        }
    }
}

/// The bench, who can still join and what the household stores. Saved
/// whole, so a sheet, a magazine, a stack and who is out survive a
/// restart.
#[derive(Resource, Debug, Clone, PartialEq, Serialize, Deserialize)]
pub(super) struct Household {
    /// The file's version, so a later shape can tell an older file.
    pub version: u32,
    /// Everyone on the bench; the first is the arrival, who is never
    /// removed.
    pub characters: Vec<Character>,
    /// Who is out.
    pub active: usize,
    /// Who can still join, in the order offered.
    pub recruits: Vec<Character>,
    /// The household's store, cell for cell, without the empty cells at
    /// its end.
    pub store: Vec<Option<Record>>,
    /// The household's credits.
    pub purse: u32,
    /// The deposits whose survey post the household had stood at, from a
    /// file written before posts went. Read, converted to `found` (found
    /// and sampled) at load and never written again.
    #[serde(default, skip_serializing_if = "BTreeSet::is_empty")]
    pub visited: BTreeSet<DepositId>,
    /// The deposits the household has found by pinging, with what is known
    /// of each (ticket 0009, step 3). Added with a serde default and no
    /// version change: a file without it has found nothing yet, apart from
    /// what `visited` gives.
    #[serde(default)]
    pub found: BTreeMap<DepositId, Found>,
    /// Whether the household has been given its survey tool, once: the first
    /// day's pack carries one, and a file saved before that is given one at
    /// load. Added with a serde default and no version change.
    #[serde(default)]
    pub survey_tool_given: bool,
    /// The deposit the HUD points to, if one is tracked.
    pub tracked: Option<DepositId>,
    /// The world `visited` and `tracked` were recorded in; none until one
    /// of them is. A deposit's id is its cell of one seed's lattice, so in
    /// another world the same ids name other ground, or none: there the
    /// record is neither shown nor written, and the file keeps it.
    #[serde(default)]
    pub surveyed_in: Option<SurveyedIn>,
    /// The bench's work: the roll and batch counters, the session left on
    /// the bench with its materials, and what each finished batch was made
    /// from.
    #[serde(default)]
    pub crafting: CraftState,
    /// Every blueprint the household has recorded, by its number: what
    /// each makes, what its slots ask for and the runs it has left. The
    /// items in the stock name these by their batch.
    #[serde(default)]
    pub blueprints: Blueprints,
    /// The fabricator: the run it is on, with what was committed to it.
    #[serde(default)]
    pub factory: FactoryState,
    /// What the buyers' boards have taken of the household and the ledger
    /// of who bought which batch (ticket 0007, step 7). Added with a serde
    /// default and no version change: a file without it sells nothing yet.
    #[serde(default)]
    pub procurement: ProcurementState,
    /// The vehicle the household owned before it was an item (ticket 0009,
    /// step 2, first half): read from an old file and turned into a store
    /// grade bike in `stable` at load, never written.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bike: Option<strata::registry::vehicle::VehicleId>,
    /// The household's ride and its saddle bags (ticket 0009, step 2). Added
    /// with a serde default and no version change: a file without it has
    /// no bike.
    #[serde(default)]
    pub stable: strata::sim::ride::SavedStable,
    /// The field harvesters planted (ticket 0009, step 6), stamped with the
    /// world like the survey record: in another world they are kept here,
    /// not stood and not advanced. Added with a serde default and no
    /// version change: a file without it has none planted.
    #[serde(default)]
    pub harvesters: Harvesters,
    /// Played seconds (ticket 0012, step 2): the clock a cleared camp's quiet
    /// is measured on. Added with a serde default and no version change.
    #[serde(default)]
    pub played: PlayedClock,
    /// The camps cleared and when, on `played`'s clock, stamped with the
    /// world like the survey record: in another world they are kept here and
    /// neither read nor advanced. Added with a serde default and no version
    /// change.
    #[serde(default)]
    pub cleared: Cleared,
    /// What the household did to each faction (ticket 0012, step 3), the
    /// hidden ledger of its stance, stamped with the world like `cleared`.
    /// Added with a serde default and no version change.
    #[serde(default)]
    pub deeds: Deeds,
    /// Units taken from each commons in the played day, stamped like `deeds`.
    #[serde(default)]
    pub commons: CommonsTaken,
    /// The leases bought, stamped like `deeds`.
    #[serde(default)]
    pub leases: Leases,
    /// The caches cleared camps left and nobody has emptied (ticket 0012,
    /// step 4a), stamped with the world like `cleared`. Added with a serde
    /// default and no version change.
    #[serde(default)]
    pub caches: Caches,
    /// The board jobs taken (ticket 0012, step 4b), stamped with the world
    /// like `cleared`. Added with a serde default and no version change.
    #[serde(default)]
    pub jobs: Jobs,
    /// The camps found, stamped like `jobs`.
    #[serde(default)]
    pub known: Known,
}

/// The world a household's survey record belongs to.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub(super) struct SurveyedIn {
    /// The world's seed.
    pub seed: u64,
    /// The side of the deposits' lattice, metres.
    pub cell_m: f64,
}

/// The blade the scripted fight cuts with: the scripted player cannot cut
/// with a gun in hand.
const SCRIPT_BLADE: &str = "veyra:weapon.service_blade";

/// Where the household is kept and whether an existing file is ignored,
/// from the command line.
#[derive(Resource, Debug, Clone)]
#[allow(
    clippy::struct_excessive_bools,
    reason = "one field a command line flag, each a still's or a test's own"
)]
pub(super) struct HouseholdFile {
    /// The RON file.
    pub path: PathBuf,
    /// `--fresh`: start the first day over.
    pub fresh: bool,
    /// `--fitted`: modules to start with on the active character's
    /// weapons, by item id.
    pub fitted: Vec<String>,
    /// `--in-hand`: store grade weapons for the two slots of whoever starts
    /// out, `-` leaving a slot be. The scripted fight, which cuts, begins
    /// with a blade in the second slot.
    pub in_hand: Vec<String>,
    /// `--learn`: boxes the member who starts out has learned.
    pub learn: Vec<String>,
    /// `--grant`: abilities the member who starts out knows and has slotted.
    pub grant: Vec<String>,
    /// `--condition`: the carbine in hand starts worn down to this.
    pub condition: Option<u16>,
    /// `--as`: the recruit who starts joined and out.
    pub member: Option<String>,
    /// `--crafted-carbine`: the moddability of the smith's carbine the
    /// arrival starts with, and whether it is in hand (a still of the
    /// fitting screen reads the weapon in hand).
    pub crafted_carbine: Option<(u16, bool)>,
    /// `--blanks`: store-grade mount blanks the character who starts out
    /// has in the pack.
    pub blanks: Option<u32>,
    /// `--give`: one of each of these items in the pack of the character
    /// who starts out.
    pub give: Vec<String>,
    /// `--bike`: a household with no bike is given a store grade one.
    pub bike: bool,
    /// `--bike-condition`: the stabled bike starts worn to this.
    pub bike_condition: Option<u16>,
    /// `--purse`: the purse starts at this.
    pub purse: Option<u32>,
    /// `--saddle-stock`: three lots and a part start in the saddle.
    pub saddle_stock: bool,
    /// `--harvester`: a harvester is planted where the run starts.
    pub harvester: bool,
    /// `--harvest-hours`: the planted machines are advanced this long.
    pub harvest_hours: Option<f64>,
    /// `--open-harvester`: the nearest machine's screen opens.
    pub open_harvester: bool,
}

impl HouseholdFile {
    /// What the command line says about the household's file and its
    /// starts for a still.
    fn of(cli: &super::Cli) -> Self {
        Self {
            path: cli.household.clone(),
            fresh: cli.fresh,
            fitted: cli.fitted.clone(),
            in_hand: if cli.in_hand.is_empty() && cli.combat_script {
                vec!["-".to_owned(), SCRIPT_BLADE.to_owned()]
            } else {
                cli.in_hand.clone()
            },
            learn: cli.learn.clone(),
            grant: cli.grant.clone(),
            condition: cli.condition,
            member: cli.member.clone(),
            crafted_carbine: cli
                .crafted_carbine
                .map(|moddability| (moddability, cli.open_fitting.is_some())),
            blanks: cli.blanks,
            give: cli.give.clone(),
            bike: cli.bike,
            bike_condition: cli.bike_condition,
            purse: cli.purse,
            saddle_stock: cli.saddle_stock,
            harvester: cli.harvester,
            harvest_hours: cli.harvest_hours,
            open_harvester: cli.open_harvester,
        }
    }
}

/// Marks the first day: the household was started, not loaded, so the
/// player has never seen the bench place. Removed once said.
#[derive(Resource, Debug, Default, Clone, Copy)]
struct FirstDay;

/// The household's survey record is this world's, and the sim's to change:
/// present when the file's record was made in the world this run is in (or
/// the file has none yet), absent when it is another world's, which is
/// then left in the file untouched.
#[derive(Resource, Debug, Clone, Copy, PartialEq)]
struct SurveyLive {
    /// The world this run is in, which a record made in it is stamped with.
    world: SurveyedIn,
    /// What the sim tracked when it was last copied to the household.
    tracked: Option<DepositId>,
}

/// What the sim is handed of a household's survey record in the world
/// `here`, and whether the record is live there. `find` is the world's own
/// answer for an id.
///
/// A record made in another world (another seed, another lattice) is not
/// handed over: its ids are cells, which here name other deposits or none,
/// and marking those as stood at would be wrong about the household. It
/// stays in the file. In its own world every id is handed over as it is,
/// with no look at the ground: only the tracked one is asked of the world,
/// since the HUD needs the deposit whole. A deposit of the old `visited`
/// list, from a file written when posts stood, is found and sampled; the
/// world is asked for its centre and peak, and one it cannot give is left
/// out.
fn survey_record(
    household: &Household,
    here: Option<SurveyedIn>,
    find: impl Fn(DepositId) -> Option<Deposit>,
) -> (Surveyed, Option<SurveyLive>) {
    let ours = here.filter(|here| household.surveyed_in.is_none_or(|was| was == *here));
    let Some(world) = ours else {
        return (Surveyed::default(), None);
    };
    let tracked = household.tracked.and_then(&find);
    let live = SurveyLive {
        world,
        tracked: tracked.as_ref().map(|deposit| deposit.id),
    };
    let mut found = household.found.clone();
    for id in &household.visited {
        if found.contains_key(id) {
            continue;
        }
        if let Some(deposit) = find(*id) {
            found.insert(
                *id,
                Found {
                    reading: Reading::Exact,
                    sampled: true,
                    best_pm: deposit.peak,
                    best_at: deposit.at,
                },
            );
        }
    }
    let surveyed = Surveyed {
        found,
        tracked,
        lead: None,
    };
    (surveyed, Some(live))
}

/// Clears a survey record made in this seed on another lattice, which will
/// never come back (a retuned dial, not another world), so the run is live
/// and the next record is stamped with the lattice of today. Another seed's
/// record is left alone: its world may be played again. True when cleared.
fn drop_old_lattice(household: &mut Household, here: Option<SurveyedIn>) -> bool {
    let (Some(here), Some(was)) = (here, household.surveyed_in) else {
        return false;
    };
    if was.seed != here.seed || was == here {
        return false;
    }
    household.visited.clear();
    household.found.clear();
    household.tracked = None;
    // A machine keeps its own lot, point and figures, so a retuned lattice
    // leaves it standing rather than destroying the stock inside.
    household.surveyed_in = None;
    true
}

/// Reads the saved household, or starts it, once the world, its registries
/// and its deposits exist and before the player spawns from it, and hands
/// the sim the household's store, its purse, its survey record, its
/// bench's work, its blueprints and its fabricator's run. The
/// file is written by [`save_on_change`], which sees the insert as a
/// change, unless reading it said to hold.
#[allow(
    clippy::too_many_lines,
    reason = "one start, each still flag applied to the household in turn"
)]
pub(super) fn init(
    mut commands: Commands,
    file: Res<HouseholdFile>,
    registries: Res<strata::sim::Registries>,
    survey: Option<Res<Survey>>,
    seed: Option<Res<WorldSeed>>,
    source: Option<Res<WorldSourceRes>>,
    settlements: Option<Res<strata::sim::Settlements>>,
) {
    // The first lots are those nearest the default start, wherever this
    // run starts: the household's stock should not turn on a flag. Asked
    // only on a first day and by an older file's upgrade.
    let first_lots = || {
        let from = source
            .as_deref()
            .and_then(|source| super::combat::default_spawn(source, settlements.as_deref()))
            .map_or((0., 0.), |(x, _, z)| (x, z));
        grants(&registries.0, survey.as_deref(), from)
    };
    let Start {
        household,
        first_day,
        hold,
    } = Household::load_or_start(&file.path, file.fresh, &registries.0, &first_lots);
    if first_day {
        commands.insert_resource(FirstDay);
    }
    if hold {
        commands.insert_resource(SaveHeld);
    }
    let mut household = household;
    if let Some(condition) = file.condition
        && let Err(reason) = household.wear_at_start(condition, &registries.0)
    {
        error!("household: --condition {condition}: {reason}");
    }
    if let Some(name) = &file.member
        && let Err(reason) = household.join_named_at_start(name)
    {
        error!("household: --as {name}: {reason}");
    }
    // After `--as`, before the weapons: they are certified against what is learned.
    if !file.learn.is_empty()
        && let Err(reason) = household.learn_at_start(&file.learn, &registries.0)
    {
        error!("household: --learn: {reason}");
    }
    // After `--learn`: what a box grants is known before what is granted by hand.
    if !file.grant.is_empty()
        && let Err(reason) = household.grant_at_start(&file.grant, &registries.0)
    {
        error!("household: --grant: {reason}");
    }
    // After `--as`: the weapons asked for are certified against whoever is out.
    if !file.in_hand.is_empty()
        && let Err(reason) = household.in_hand_at_start(&file.in_hand, &registries.0)
    {
        error!("household: --in-hand: {reason}");
    }
    // After `--in-hand`: a melee part fits the blade the flag put in a hand.
    for id in &file.fitted {
        if let Err(reason) = household.fit_at_start(id, &registries.0) {
            error!("household: --fitted {id}: {reason}");
        }
    }
    if let Some((moddability, in_hand)) = file.crafted_carbine
        && let Err(reason) = household.crafted_carbine_at_start(moddability, in_hand, &registries.0)
    {
        error!("household: --crafted-carbine {moddability}: {reason}");
    }
    if let Some(count) = file.blanks
        && let Err(reason) = household.blanks_at_start(count, &registries.0)
    {
        error!("household: --blanks {count}: {reason}");
    }
    if !file.give.is_empty()
        && let Err(reason) = household.give_at_start(&file.give, &registries.0)
    {
        error!("household: --give: {reason}");
    }
    household.ride_at_start(&file, &registries.0);
    let stable = strata::sim::ride::Stable::from_saved(
        &household.stable,
        &registries.0.items,
        &registries.0.vehicles,
    )
    .unwrap_or_else(|error| {
        error!("household: the stable does not stand: {error}");
        strata::sim::ride::Stable::default()
    });
    commands.insert_resource(stable);
    let items = &registries.0.items;
    let store = HouseholdStore::from_saved(&household.store, items).unwrap_or_else(|error| {
        error!("household: the store does not stand: {error}");
        HouseholdStore::from_saved(&[], items).unwrap_or_default()
    });
    commands.insert_resource(store);
    let here = survey
        .as_deref()
        .zip(seed.as_deref())
        .map(|(survey, seed)| SurveyedIn {
            seed: seed.0.0,
            cell_m: survey.0.set().rules().lattice.cell_m,
        });
    if drop_old_lattice(&mut household, here) {
        info!(
            "household: the survey record was made on an older lattice of this world's deposits; it is dropped and this run records afresh"
        );
    }
    let (surveyed, live) = survey_record(&household, here, |id| survey.as_deref()?.0.get(id));
    let is_live = live.is_some();
    if live.is_none() {
        commands.insert_resource(strata::sim::deposits::SurveyUnkept);
    }
    match (live, here, household.surveyed_in) {
        (Some(live), ..) => commands.insert_resource(live),
        (None, Some(here), Some(was)) => info!(
            "household: the survey record in {} is of another world (seed {}, cells of {:.0} m; this one is seed {}, {:.0} m); it is kept as it is and this run records none",
            file.path.display(),
            was.seed,
            was.cell_m,
            here.seed,
            here.cell_m
        ),
        _ => {}
    }
    commands.insert_resource(Purse(household.purse));
    commands.insert_resource(surveyed);
    commands.insert_resource(household.crafting.clone());
    commands.insert_resource(household.blueprints.clone());
    commands.insert_resource(household.factory.clone());
    // The machines are of the world the survey record is of: in another
    // one they stay in the file and stand nowhere.
    commands.insert_resource(stood_harvesters(&household, is_live));
    commands.insert_resource(household.played);
    commands.insert_resource(stood_cleared(&household, is_live));
    let (deeds, commons, leases) = stood_deeds(&household, is_live);
    commands.insert_resource(deeds);
    commands.insert_resource(commons);
    commands.insert_resource(leases);
    commands.insert_resource(cache::stood_caches(&household, is_live));
    let (jobs, known) = cache::stood_jobs(&household, is_live);
    commands.insert_resource(jobs);
    commands.insert_resource(known);
    commands.insert_resource(household.procurement.clone());
    commands.insert_resource(household);
}

/// The camps cleared that this run may read: the household's when its record
/// is of this world, none in another one, whose record stays in the file.
pub(super) fn stood_cleared(household: &Household, live: bool) -> Cleared {
    if live {
        household.cleared.clone()
    } else {
        Cleared::default()
    }
}

/// The ledger, the commons' tallies and the leases this run may read: the
/// household's when its record is of this world, empty in another one, whose
/// record stays in the file.
pub(super) fn stood_deeds(household: &Household, live: bool) -> (Deeds, CommonsTaken, Leases) {
    if live {
        (
            household.deeds.clone(),
            household.commons.clone(),
            household.leases.clone(),
        )
    } else {
        Default::default()
    }
}

/// The first day's line, once the props stand: where the terminal and
/// the trainers are, since they are behind the spawn and the camera
/// looks the other way.
fn first_day_notice(mut commands: Commands, props: Query<(), With<Prop>>, mut said: Local<bool>) {
    if *said || props.is_empty() {
        return;
    }
    *said = true;
    commands.remove_resource::<FirstDay>();
    if let Some(words) = strata::client::ui::strings::english("strata.household.first_day") {
        strata::client::ui::progress::notice(&mut commands, words);
        info!("household: {words}");
    }
}

/// The cache a cleared camp leaves, and the fall that carries a member
/// home, in every mode: a headless run is carried home as a window is.
fn install_fall(app: &mut App) {
    app.init_resource::<fall::FallState>().add_systems(
        Update,
        (
            cache::stand_caches,
            fall::injured_at_start.run_if(resource_exists::<Household>),
            fall::carried_home.run_if(resource_exists::<Household>),
        )
            .run_if(in_state(AppState::InWorld)),
    );
}

/// The household's systems, in combat play only: a connected client or a
/// flycam has no household.
pub(super) fn install(app: &mut App, cli: &super::Cli) {
    if !cli.combat {
        return;
    }
    app.insert_resource(HouseholdFile::of(cli))
        .init_resource::<PropSites>()
        .add_systems(Update, place_props.run_if(in_state(AppState::InWorld)))
        .add_systems(
            SimTick,
            (script_learn, script_swap)
                .chain()
                .run_if(resource_exists::<ProgressionScript>)
                .run_if(resource_exists::<Household>)
                .in_set(SimSet::Input)
                .after(strata::sim::player::apply_scripted_intents),
        )
        .add_systems(
            SimTick,
            script_hurt
                .run_if(resource_exists::<ProgressionScript>)
                .in_set(SimSet::Input)
                .after(strata::sim::player::apply_scripted_intents),
        )
        // What the sim changed is copied and then written, in that order and
        // before the crafting screen words the sim's answer: a roll's counter
        // is in the file by the time its outcome can be read.
        .add_systems(
            Update,
            (track_sheet, track_bar, track_stock, save_on_change)
                .chain()
                .run_if(resource_exists::<Household>)
                .run_if(in_state(AppState::InWorld))
                .before(strata::client::ui::crafting::hear)
                .before(strata::client::ui::fitting::hear_rework),
        )
        // Who is on the bench is the sim's rule for a run's worker, in every
        // mode: a headless run starts and works a run as a window does.
        .add_systems(
            Update,
            publish_benched
                .run_if(resource_exists::<Household>)
                .run_if(in_state(AppState::InWorld)),
        );
    install_harvesters(app, cli);
    install_factory_shots(app, cli);
    install_fall(app);
    if cli.script_progression {
        app.insert_resource(ProgressionScript {
            open_sheet: cli.open_sheet,
            open_bench: cli.open_bench,
            crafting: cli.script_crafting,
            hurts: cli.script_hurt,
            ..default()
        });
    }
    if cli.script_crafting {
        // A screen is opened only where there is one: a headless run has
        // no UI for the flag to open.
        let open_crafting = cli
            .open_crafting
            .clone()
            .filter(|_| !cli.headless)
            .map(|schematic| (schematic, cli.crafting_still()));
        app.insert_resource(CraftingScript::new(
            cli.script_report.clone(),
            cli.open_fitting.is_some(),
            open_crafting,
        ))
        .add_systems(
            SimTick,
            (
                script_carbine,
                script_bench,
                script_craft,
                script_factory,
                script_survey,
                script_sale,
            )
                .chain()
                .run_if(resource_exists::<ProgressionScript>)
                .in_set(SimSet::Input)
                .after(strata::sim::player::apply_scripted_intents),
        );
    }
    // The bench view and its requests are the client UI's; a headless app
    // has neither, and a system over an unregistered message would be
    // skipped with a warning every frame.
    if !cli.headless {
        app.add_systems(Startup, load_prop_art)
            .add_systems(Update, dress_props.run_if(resource_exists::<PropArt>))
            .add_systems(
                Update,
                (
                    first_day_notice.run_if(resource_exists::<FirstDay>),
                    label_props.run_if(strata::client::ui::game_input_allowed),
                    hide_labels.run_if(not(strata::client::ui::game_input_allowed)),
                )
                    .run_if(in_state(AppState::InWorld)),
            )
            .add_systems(
                Update,
                (publish_bench, answer_bench)
                    .chain()
                    .run_if(resource_exists::<Household>)
                    .run_if(resource_exists::<BenchView>)
                    .run_if(in_state(AppState::InWorld)),
            );
    }
}
