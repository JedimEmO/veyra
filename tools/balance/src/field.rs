//! The first hour of a new household, worked from the shipped data (ticket
//! 0009, step 7).
//!
//! A pure function from the registries, the world's deposits and
//! `assets/balance/first_hour.ron` to a timeline: for each step of the path
//! its minutes, the running clock, the purse, the XP of the two trades and
//! what it made. Every figure is one of the sim's own functions
//! (`deposits::skill`, `ride::dials`, `crafting::preview_for`,
//! `procurement::pays`, `harvest::station` and so on); the model adds only the
//! arithmetic on the file's assumptions. A step the rules refuse is a row that
//! says so, and the clock stops there.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;
use std::path::Path;
use std::sync::Arc;

use anyhow::{Context, Result, bail, ensure};
use serde::Deserialize;
use strata_core::Seed;
use strata_registry::item::{ItemKind, QUALITY_MAX, Record, STORE_GRADE};
use strata_registry::procurement::{BuyerDef, Wants};
use strata_registry::progression::XpKind;
use strata_registry::schematic::{Hand, Outcome, Schematic};
use strata_registry::{GameRegistries, Namespaced};
use strata_worldgen::ProceduralWorld;
use veyra_rules::crafting::{self, Holder, Picks};
use veyra_rules::deposits::{SurveyEvent, lot};
use veyra_rules::deposits::{cycle_units, skill, survey_xp};
use veyra_rules::procurement::{self, ProcurementState};
use veyra_rules::progression::CharacterSheet;
use veyra_rules::{harvest, ride};
use veyra_world::{Deposit, DepositField, DepositId};

/// The file this module reads, under the assets directory.
pub const FILE: &str = "balance/first_hour.ron";

/// How the straight line becomes a path, and how fast it is covered.
#[derive(Debug, Clone, Copy, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Travel {
    /// The ridden or walked way over the straight line.
    pub detour: f64,
    /// The bike's mean speed over its off-road top speed.
    pub bike_mean: f64,
    /// The share of a walk that is run.
    pub sprint_share: f64,
    /// How far, metres, a deposit is looked for.
    pub search_m: f64,
}

/// What finding one deposit costs once the rider is in its area.
#[derive(Debug, Clone, Copy, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FindCost {
    /// Pings, at the base range.
    pub pings: u32,
    /// Metres walked after them, at the base range.
    pub walked_m: f64,
}

/// Played seconds of the screens.
#[derive(Debug, Clone, Copy, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Bench {
    /// A crafting session.
    pub session_s: f64,
    /// A visit to a board, trainer, terminal or locker.
    pub screen_s: f64,
}

/// The minute a milestone must come by.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Band {
    /// The mark a step carries.
    pub id: String,
    /// The latest minute.
    pub by_min: f64,
    /// Mounts the weapon made at the mark must have open, when the mark is a
    /// made weapon: a carbine that opens nothing a bought one does not is
    /// not what the band is for.
    #[serde(default)]
    pub opens: Option<usize>,
    /// Why the band is let fail, when only a forbidden change closes it.
    #[serde(default)]
    pub waived: Option<String>,
}

/// What a standing harvester must be worth.
#[derive(Debug, Clone, Copy, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HarvestBand {
    /// An hour of the machine is worth between these minutes of a hand.
    pub hour_worth_min: (f64, f64),
    /// Its hopper takes at least this many hours to fill.
    pub hopper_hours_min: f64,
}

/// Where a ride goes.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub enum Dest {
    /// The nearest deposit of a class.
    Nearest(Namespaced),
    /// The nearest deposit of a class whose `line` of the schematic's
    /// product, worked at its ceiling from the deposit alone, is at least
    /// `at_least`.
    Best {
        /// The class of resource.
        class: Namespaced,
        /// The schematic the line is read through.
        schematic: Namespaced,
        /// The product's line.
        line: String,
        /// The least figure.
        at_least: u16,
    },
}

fn one() -> u32 {
    1
}

/// What a step does.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub enum Do {
    /// Buy a store grade item at a board.
    Buy(Namespaced),
    /// Switch to a member of the household.
    Take(String),
    /// Ride (or walk, before the bike) to a deposit.
    Ride(Dest),
    /// Find the deposit the rider is at.
    Find,
    /// Sample at least this many units of it.
    Sample(u32),
    /// Plant a carried harvester here.
    Plant,
    /// Go to the planted harvester and collect it.
    Collect,
    /// Ride home.
    Home,
    /// Make a schematic's product `times` over.
    Craft {
        /// What to make.
        schematic: Namespaced,
        /// Whose sheet makes it.
        by: String,
        /// How many sessions.
        #[serde(default = "one")]
        times: u32,
    },
    /// Sell to the board.
    Sell {
        /// What.
        item: Namespaced,
        /// How many.
        count: u32,
    },
    /// Learn a box.
    Learn {
        /// The box.
        id: String,
        /// Whose sheet.
        member: String,
    },
}

/// One step of the path.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Step {
    /// The band this step reaches, if any.
    #[serde(default)]
    pub mark: Option<String>,
    /// What it does.
    pub what: Do,
}

/// `assets/balance/first_hour.ron`.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FirstHour {
    /// The file's shape.
    pub version: u32,
    /// The world's seed.
    pub seed: u64,
    /// The way and the speed.
    pub travel: Travel,
    /// Finding.
    pub find: FindCost,
    /// The concentration a walked lead is sampled at.
    pub sample_at_pm: u16,
    /// The screens.
    pub bench: Bench,
    /// The milestones.
    pub bands: Vec<Band>,
    /// The purse never goes under this.
    pub purse_floor: i64,
    /// The harvester's worth.
    pub harvest: HarvestBand,
    /// The order a bench session's points are spent on a product's lines,
    /// the lines it names first and the rest after in the item's order. An
    /// assumption: the bench lets the crafter choose.
    #[serde(default)]
    pub spend: Vec<String>,
    /// The path.
    pub path: Vec<Step>,
}

impl FirstHour {
    /// The file's current shape.
    pub const VERSION: u32 = 1;

    /// Read and check the file under `assets`.
    ///
    /// # Errors
    /// The file is missing, malformed, or fails [`FirstHour::check`].
    pub fn load(assets: &Path, registries: &GameRegistries) -> Result<Self> {
        let path = assets.join(FILE);
        let text = std::fs::read_to_string(&path).with_context(|| path.display().to_string())?;
        let file: Self =
            ron::from_str(&text).with_context(|| format!("parsing {}", path.display()))?;
        file.check(registries)?;
        Ok(file)
    }

    /// What is wrong with the file against the data, if anything: a band
    /// twice or without a step, a mark without a band, an item, schematic,
    /// box or member the data has not got, a negative assumption.
    ///
    /// # Errors
    /// The first thing wrong.
    #[allow(
        clippy::too_many_lines,
        reason = "one check after another, each a line or two"
    )]
    pub fn check(&self, registries: &GameRegistries) -> Result<()> {
        ensure!(
            self.version == Self::VERSION,
            "first_hour.ron version {}",
            self.version
        );
        let t = &self.travel;
        for (what, v) in [
            ("detour", t.detour),
            ("bike_mean", t.bike_mean),
            ("search_m", t.search_m),
            ("pings", f64::from(self.find.pings)),
            ("walked_m", self.find.walked_m),
            ("session_s", self.bench.session_s),
            ("screen_s", self.bench.screen_s),
            ("hopper_hours_min", self.harvest.hopper_hours_min),
            ("hour_worth_min low", self.harvest.hour_worth_min.0),
        ] {
            ensure!(
                v.is_finite() && v >= 0.0,
                "{what} must be a number over nought"
            );
        }
        ensure!(
            t.detour > 0.0 && t.bike_mean > 0.0 && t.search_m > 0.0,
            "detour, bike_mean and search_m must be over nought"
        );
        ensure!(
            (0.0..=1.0).contains(&t.sprint_share),
            "sprint_share is 0 to 1"
        );
        ensure!(
            self.sample_at_pm > 0 && self.sample_at_pm <= 1000,
            "sample_at_pm is 1 to 1000"
        );
        ensure!(
            self.harvest.hour_worth_min.0 <= self.harvest.hour_worth_min.1,
            "hour_worth_min runs low to high"
        );
        let mut seen = BTreeSet::new();
        for band in &self.bands {
            ensure!(
                seen.insert(band.id.as_str()),
                "band `{}` is named twice",
                band.id
            );
            ensure!(
                band.by_min.is_finite() && band.by_min > 0.0,
                "band `{}` needs a minute over nought",
                band.id
            );
        }
        let mut marked = BTreeSet::new();
        for step in &self.path {
            if let Some(mark) = &step.mark {
                ensure!(seen.contains(mark.as_str()), "mark `{mark}` has no band");
                ensure!(
                    marked.insert(mark.as_str()),
                    "mark `{mark}` is on two steps"
                );
            }
        }
        for band in &self.bands {
            ensure!(
                marked.contains(band.id.as_str()),
                "band `{}` has no step",
                band.id
            );
        }
        let members = members(registries);
        let member = |name: &str| -> Result<()> {
            ensure!(
                members.contains_key(name),
                "`{name}` is no member of the household"
            );
            Ok(())
        };
        let items = &registries.items;
        let item = |id: &Namespaced| -> Result<()> {
            ensure!(items.get(id).is_some(), "`{id}` is no item");
            Ok(())
        };
        for step in &self.path {
            match &step.what {
                Do::Buy(id) => {
                    item(id)?;
                    ensure!(seller(registries, id).is_some(), "no buyer sells `{id}`");
                }
                Do::Take(name) => member(name)?,
                Do::Ride(Dest::Nearest(class)) => {
                    ensure!(
                        registries
                            .resources
                            .classes()
                            .iter()
                            .any(|c| c.id == *class),
                        "`{class}` is no class"
                    );
                }
                Do::Ride(Dest::Best {
                    class,
                    schematic,
                    line,
                    ..
                }) => {
                    ensure!(
                        registries
                            .resources
                            .classes()
                            .iter()
                            .any(|c| c.id == *class),
                        "`{class}` is no class"
                    );
                    let made = registries
                        .schematics
                        .get(schematic)
                        .with_context(|| format!("`{schematic}` is no schematic"))?;
                    let product = items
                        .get(&made.def.makes)
                        .context("a schematic makes no item")?;
                    ensure!(
                        product.lines.iter().any(|l| l.id == *line),
                        "`{line}` is no line of `{}`",
                        product.id
                    );
                    ensure!(
                        made.def.slots.iter().any(|s| matches!(&s.takes, strata_registry::schematic::Takes::Class(c) if c == class)),
                        "`{schematic}` takes no `{class}`"
                    );
                }
                Do::Craft {
                    schematic,
                    by,
                    times,
                } => {
                    ensure!(
                        registries.schematics.get(schematic).is_some(),
                        "`{schematic}` is no schematic"
                    );
                    ensure!(*times > 0, "a craft of nothing");
                    member(by)?;
                }
                Do::Sell { item: id, count } => {
                    item(id)?;
                    ensure!(*count > 0, "a sale of nothing");
                }
                Do::Learn { id, member: name } => {
                    ensure!(
                        registries.progression.find(id).is_some(),
                        "`{id}` is no box"
                    );
                    member(name)?;
                }
                Do::Find | Do::Sample(_) | Do::Plant | Do::Collect | Do::Home => {}
            }
        }
        Ok(())
    }
}

/// The deposits the model walks to: the world's, or a fixture's.
pub trait Ground {
    /// Every deposit within `radius_m` of `at`, nearest first.
    fn near(&self, at: [f64; 2], radius_m: f64) -> Vec<Deposit>;
}

impl Ground for DepositField {
    fn near(&self, at: [f64; 2], radius_m: f64) -> Vec<Deposit> {
        DepositField::near(self, at[0], at[1], radius_m)
    }
}

/// A fixed list of deposits.
#[derive(Debug, Clone, Default)]
pub struct Fixed(pub Vec<Deposit>);

impl Ground for Fixed {
    fn near(&self, at: [f64; 2], radius_m: f64) -> Vec<Deposit> {
        let mut held: Vec<(f64, Deposit)> = self
            .0
            .iter()
            .map(|d| (d.distance_m(at[0], at[1]), d.clone()))
            .filter(|(distance, _)| *distance <= radius_m)
            .collect();
        held.sort_by(|a, b| a.0.total_cmp(&b.0).then_with(|| a.1.id.cmp(&b.1.id)));
        held.into_iter().map(|(_, d)| d).collect()
    }
}

/// The shipped world: its deposits and the city's place.
pub struct Shipped {
    /// The registries.
    pub registries: Arc<GameRegistries>,
    /// The deposits.
    pub field: DepositField,
    /// Port Serein's anchor, world `(x, z)`.
    pub home: [f64; 2],
    /// Walking and sprinting, metres a second.
    pub foot: (f64, f64),
}

impl Shipped {
    /// Load `assets`, build the world of `seed` and bake the water round
    /// the city, which the deposits' refusals read.
    ///
    /// # Errors
    /// The assets do not load.
    pub fn open(assets: &Path, seed: u64) -> Result<Self> {
        let registries =
            Arc::new(strata_registry::load_from_dir(assets).context("loading the assets")?);
        let tables = veyra_rules::combat_tables::CombatTables::load(assets.join("combat"))
            .context("loading the combat tables")?;
        let world = Arc::new(ProceduralWorld::new(Seed(seed), &registries));
        let home = world
            .sites()
            .first()
            .context("the world has no city")?
            .centre;
        world.hydrology().set_threads(8);
        world.prepare_water(home[0], home[1], 12_000.0);
        let field = DepositField::new(Arc::clone(&world), Seed(seed), Arc::clone(&registries));
        Ok(Self {
            registries,
            field,
            home,
            foot: (
                f64::from(tables.actor.walk_speed),
                f64::from(tables.actor.sprint_speed),
            ),
        })
    }
}

/// What a run starts from besides the file.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Variant {
    /// Boxes every sheet is given, with what they need.
    pub grant: Vec<String>,
    /// Boxes taken off every sheet, with what needs them.
    pub strip: Vec<String>,
    /// Learn steps are left out, so a box taken off is not asked for by a
    /// later one: the question is what the box does to the path's time.
    pub no_learn: bool,
}

/// The household's members by name: the arrival and the recruits.
fn members(registries: &GameRegistries) -> BTreeMap<String, CharacterSheet> {
    let mut found = BTreeMap::new();
    if let Some(household) = &registries.sheets.household {
        for member in std::iter::once(&household.arrival).chain(&household.recruits) {
            found.insert(member.name.clone(), CharacterSheet::authored(&member.sheet));
        }
    }
    found
}

/// The buyer that sells `item`, and the price.
fn seller<'a>(registries: &'a GameRegistries, item: &Namespaced) -> Option<(&'a BuyerDef, u32)> {
    registries.buyers.iter().find_map(|buyer| {
        buyer
            .sells
            .iter()
            .find(|s| s.item == *item)
            .map(|s| (buyer, s.price))
    })
}

/// A weapon's figures as the fight will read them.
#[derive(Debug, Clone, PartialEq)]
pub struct WeaponFigures {
    /// Its lines, in the item's order.
    pub lines: Vec<(String, u16)>,
    /// Mounts open.
    pub open: usize,
    /// Mounts it has.
    pub mounts: usize,
    /// The signature's factor on its row, when it has one.
    pub factor: Option<f32>,
}

/// The figures of `record`, a weapon.
#[must_use]
pub fn weapon_figures(registries: &GameRegistries, record: &Record) -> Option<WeaponFigures> {
    let def = registries.items.get(&record.item)?;
    let ItemKind::Weapon {
        mounts, signature, ..
    } = &def.kind
    else {
        return None;
    };
    let moddability = record.lines.get("moddability").copied().unwrap_or(0);
    Some(WeaponFigures {
        lines: def
            .lines
            .iter()
            .map(|l| {
                (
                    l.id.clone(),
                    record.lines.get(&l.id).copied().unwrap_or(STORE_GRADE),
                )
            })
            .collect(),
        open: mounts
            .iter()
            .filter(|m| m.opens_at.is_none_or(|at| moddability >= at))
            .count(),
        mounts: mounts.len(),
        factor: signature
            .as_ref()
            .map(|s| s.factor(record.lines.get(&s.line).copied().unwrap_or(STORE_GRADE))),
    })
}

/// One row of the timeline.
#[derive(Debug, Clone, PartialEq)]
pub struct Row {
    /// What the step was.
    pub label: String,
    /// The mark it carried.
    pub mark: Option<String>,
    /// Seconds it took.
    pub secs: f64,
    /// The clock after it, seconds.
    pub clock_s: f64,
    /// The purse after it.
    pub purse: i64,
    /// The XP of the two trades, by member.
    pub xp: String,
    /// What it produced.
    pub made: String,
    /// The weapon it made.
    pub weapon: Option<WeaponFigures>,
    /// Why the rules refused it.
    pub refused: Option<String>,
}

/// The first hour.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Timeline {
    /// A row a step, to the first refusal.
    pub rows: Vec<Row>,
}

impl Timeline {
    /// The minute `mark` was reached, or `None` if the path stopped first.
    #[must_use]
    pub fn reached(&self, mark: &str) -> Option<f64> {
        self.rows
            .iter()
            .find(|r| r.mark.as_deref() == Some(mark) && r.refused.is_none())
            .map(|r| r.clock_s / 60.0)
    }

    /// The lowest purse on the path.
    #[must_use]
    pub fn lowest_purse(&self) -> i64 {
        self.rows.iter().map(|r| r.purse).min().unwrap_or(0)
    }

    /// The minute the path ended, or stopped.
    #[must_use]
    pub fn end_min(&self) -> f64 {
        self.rows.last().map_or(0.0, |r| r.clock_s / 60.0)
    }

    /// The refusal that stopped the path, if one did.
    #[must_use]
    pub fn stopped(&self) -> Option<&Row> {
        self.rows.iter().find(|r| r.refused.is_some())
    }

    /// The weapon made at `mark`.
    #[must_use]
    pub fn weapon_at(&self, mark: &str) -> Option<&WeaponFigures> {
        self.rows
            .iter()
            .find(|r| r.mark.as_deref() == Some(mark))
            .and_then(|r| r.weapon.as_ref())
    }
}

/// What the machine is worth, worked from the dials.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct HarvestNumbers {
    /// Units an hour of a store grade machine makes at the sampled ground.
    pub units_hour: u32,
    /// Units a minute an untrained hand takes there.
    pub hand_per_min: f64,
    /// Minutes of that hand an hour of the machine is worth.
    pub worth_min: f64,
    /// Hours its hopper takes to fill.
    pub hopper_hours: f64,
}

/// The harvester's worth on `sheet`'s planting, store grade, at the file's
/// sampled ground.
///
/// # Errors
/// The data has no machine or no survey tool.
#[allow(clippy::cast_precision_loss, reason = "units a few thousand at most")]
pub fn harvest_numbers(
    registries: &GameRegistries,
    file: &FirstHour,
    planter: &str,
) -> Result<HarvestNumbers> {
    let machine = item_of(registries, |k| matches!(k, ItemKind::Harvester { .. }))
        .context("the data has no harvester")?;
    let sheet = members(registries).remove(planter).context("no planter")?;
    let station = harvest::station(
        registries.resources.harvest(),
        &registries.items,
        &machine,
        &registries.progression,
        &sheet,
    );
    let pm = file.sample_at_pm;
    let units_hour = harvest::made_in(station.rate, pm, u32::MAX, 3600);
    let tool = survey_tool(registries).context("the data has no survey tool")?;
    let hand = skill(
        &registries.resources,
        &registries.items,
        &registries.progression,
        &tool,
        &CharacterSheet::default(),
    );
    let hand_per_min = f64::from(cycle_units(hand.units, pm)) / f64::from(hand.cycle_s) * 60.0;
    Ok(HarvestNumbers {
        units_hour,
        hand_per_min,
        worth_min: f64::from(units_hour) / hand_per_min,
        hopper_hours: f64::from(station.hopper)
            / (f64::from(station.rate) * f64::from(pm) / 1000.0),
    })
}

/// A store grade example of the first item whose kind `which` takes.
fn item_of(registries: &GameRegistries, which: impl Fn(&ItemKind) -> bool) -> Option<Record> {
    registries
        .items
        .iter()
        .find(|d| which(&d.kind))
        .map(crafting::bought)
}

fn survey_tool(registries: &GameRegistries) -> Option<Record> {
    item_of(registries, |k| matches!(k, ItemKind::Tool { .. }))
}

/// Every failing band, as a sentence naming the band and both numbers, and
/// the waived ones that fail, apart.
#[must_use]
pub fn judge(
    file: &FirstHour,
    line: &Timeline,
    harvest: Option<HarvestNumbers>,
) -> (Vec<String>, Vec<String>) {
    let (mut failed, mut waived) = (Vec::new(), Vec::new());
    for band in &file.bands {
        let said = match line.reached(&band.id) {
            Some(at) if at <= band.by_min => match (band.opens, line.weapon_at(&band.id)) {
                (Some(want), Some(made)) if made.open < want => format!(
                    "band `{}`: the weapon opens {} mounts, needs {want}",
                    band.id, made.open
                ),
                (Some(want), None) => {
                    format!(
                        "band `{}`: no weapon made there, needs {want} open",
                        band.id
                    )
                }
                _ => continue,
            },
            Some(at) => format!(
                "band `{}`: reached at {at:.1} min, needs {:.1}",
                band.id, band.by_min
            ),
            None => format!(
                "band `{}`: never reached (stopped at minute {:.1}: {}), needs {:.1}",
                band.id,
                line.end_min(),
                line.stopped()
                    .and_then(|r| r.refused.clone())
                    .unwrap_or_default(),
                band.by_min
            ),
        };
        match &band.waived {
            Some(why) => waived.push(format!("{said} (waived: {why})")),
            None => failed.push(said),
        }
    }
    if line.lowest_purse() < file.purse_floor {
        failed.push(format!(
            "purse floor: lowest {} against {}",
            line.lowest_purse(),
            file.purse_floor
        ));
    }
    if let Some(h) = harvest {
        let (lo, hi) = file.harvest.hour_worth_min;
        if h.worth_min < lo || h.worth_min > hi {
            failed.push(format!(
                "harvest worth: an hour of a machine is {:.1} min of a hand, needs {lo:.1} to {hi:.1}",
                h.worth_min
            ));
        }
        if h.hopper_hours < file.harvest.hopper_hours_min {
            failed.push(format!(
                "harvest hopper: fills in {:.2} h, needs {:.2}",
                h.hopper_hours, file.harvest.hopper_hours_min
            ));
        }
    }
    (failed, waived)
}

/// A machine standing on a deposit.
struct Planted {
    deposit: Deposit,
    pm: u16,
    rate: u32,
    hopper: u32,
    since_s: f64,
}

/// Everything the path changes.
struct State<'a> {
    reg: &'a GameRegistries,
    ground: &'a dyn Ground,
    file: &'a FirstHour,
    foot: (f64, f64),
    home: [f64; 2],
    pos: [f64; 2],
    clock: f64,
    purse: i64,
    sheets: BTreeMap<String, CharacterSheet>,
    active: String,
    bike: Option<Record>,
    stock: Vec<Record>,
    here: Option<Deposit>,
    found: BTreeSet<DepositId>,
    sampled: BTreeSet<DepositId>,
    made_once: BTreeSet<Namespaced>,
    crafts: crafting::CraftState,
    origin: Option<DepositId>,
    /// Lots carried that the locker has not seen.
    unstowed: bool,
    planted: Option<Planted>,
    batch: u32,
}

/// What a step gave, or why it was refused.
struct Done {
    secs: f64,
    made: String,
    weapon: Option<WeaponFigures>,
}

impl Done {
    fn of(secs: f64, made: impl Into<String>) -> Self {
        Self {
            secs,
            made: made.into(),
            weapon: None,
        }
    }
}

impl State<'_> {
    fn sheet(&self) -> &CharacterSheet {
        &self.sheets[&self.active]
    }

    #[allow(
        clippy::float_cmp,
        reason = "home is assigned to the position whole, never computed"
    )]
    fn at_home(&self) -> bool {
        self.pos == self.home
    }

    fn foot_mps(&self) -> f64 {
        let s = self.file.travel.sprint_share;
        self.foot.0 * (1.0 - s) + self.foot.1 * s
    }

    /// Seconds to cover the way to `to`, stopping `short` metres before it,
    /// and the rider is there after.
    fn travel(&mut self, to: [f64; 2], short: f64) -> Result<f64, String> {
        let straight = ((to[0] - self.pos[0]).powi(2) + (to[1] - self.pos[1]).powi(2)).sqrt();
        let way = (straight - short).max(0.0) * self.file.travel.detour;
        self.pos = to;
        let Some(bike) = &self.bike else {
            return Ok(way / self.foot_mps());
        };
        let def = ride::vehicle_of(&self.reg.items, &self.reg.vehicles, bike)
            .ok_or("the bike has no vehicle file")?;
        let dials = ride::dials(
            def,
            bike,
            self.reg.items.wear(),
            &self.reg.progression,
            self.sheet(),
        );
        let call = f64::from(ride::call_s(def, &self.reg.progression, self.sheet()));
        Ok(call + way / (f64::from(dials.speed.ground) * self.file.travel.bike_mean))
    }

    fn credit(&mut self, member: &str, kind: XpKind, amount: u32) {
        if let Some(sheet) = self.sheets.get_mut(member) {
            sheet.credit(kind, amount);
        }
    }

    fn add_stock(&mut self, record: Record) {
        if let Some(same) = self.stock.iter_mut().find(|r| {
            r.item == record.item
                && r.origin == record.origin
                && r.lines == record.lines
                && r.batch == record.batch
                && r.by == record.by
        }) {
            same.count += record.count;
        } else {
            self.stock.push(record);
        }
    }

    fn take_stock(&mut self, index: usize, amount: u32) {
        self.stock[index].count -= amount;
        if self.stock[index].count == 0 {
            self.stock.remove(index);
        }
    }

    fn dest(&self, to: &Dest) -> Result<Deposit, String> {
        let set = &self.reg.resources;
        let near = self.ground.near(self.pos, self.file.travel.search_m);
        let metres = self.file.travel.search_m;
        match to {
            Dest::Nearest(class) => near
                .into_iter()
                .find(|d| set.class_of(d.ty).id == *class)
                .ok_or_else(|| format!("no {class} within {metres:.0} m")),
            Dest::Best {
                class,
                schematic,
                line,
                at_least,
            } => {
                let made = self
                    .reg
                    .schematics
                    .get(schematic)
                    .ok_or("no such schematic")?;
                let product = self
                    .reg
                    .items
                    .get(&made.def.makes)
                    .ok_or("no such product")?;
                let index = product
                    .lines
                    .iter()
                    .position(|l| l.id == *line)
                    .ok_or("no such line")?;
                let slot = made
                    .def
                    .slots
                    .iter()
                    .find(|s| matches!(&s.takes, strata_registry::schematic::Takes::Class(c) if c == class))
                    .ok_or("no such slot")?;
                near.into_iter()
                    .filter(|d| set.class_of(d.ty).id == *class)
                    .find(|d| {
                        let mut picks = Picks::new();
                        if let Some(record) = lot(d, set, slot.units) {
                            picks.insert(slot.id.clone(), record);
                        }
                        crafting::ceiling(made, index, &picks).is_some_and(|c| c >= *at_least)
                    })
                    .ok_or_else(|| {
                        format!("no {class} with {line} {at_least} within {metres:.0} m")
                    })
            }
        }
    }

    /// The picks and the stock they come out of for `schematic`.
    fn gather(&self, schematic: &Schematic) -> Result<(Picks, Vec<(usize, u32)>), String> {
        let mut picks = Picks::new();
        let mut spent = Vec::new();
        for slot in &schematic.def.slots {
            let need = slot.amount();
            let fits = |r: &Record| r.count >= need && crafting::fits(&self.reg.items, slot, r);
            // The lot of the deposit last ridden to first, so what the
            // rider sampled is what the bench uses; else the newest.
            let index = self
                .stock
                .iter()
                .rposition(|r| {
                    fits(r)
                        && self.origin.is_some()
                        && r.origin
                            .as_ref()
                            .is_some_and(|o| Some(DepositId(o.deposit)) == self.origin)
                })
                .or_else(|| self.stock.iter().rposition(fits))
                .ok_or_else(|| format!("short of {} for {}", need, slot.name))?;
            let mut pick = self.stock[index].clone();
            pick.count = need;
            picks.insert(slot.id.clone(), pick);
            spent.push((index, need));
        }
        Ok((picks, spent))
    }

    /// The share of its ceiling each line stands at when the bench's points
    /// are spent: the line starts at assembly and each point lifts it by the
    /// mean of a steady hand on the base row, in the order the file names,
    /// no line past its whole. The crafter's luck is taken at its mean.
    fn spent_shares(
        &self,
        made: &Schematic,
        lines: &[Option<crafting::LinePreview>],
        sheet: &CharacterSheet,
    ) -> Vec<u16> {
        let rules = self.reg.schematics.rules();
        let assembly = crafting::assembly_pm(rules, &self.reg.progression, sheet);
        let odds = rules.odds(Hand::Steady, rules.row(None));
        let mean: f64 = Outcome::ALL
            .iter()
            .map(|o| f64::from(odds[o.index()]) * f64::from(rules.lift_pm(*o)) / 100.0)
            .sum();
        let mut points = f64::from(crafting::points(
            rules,
            &self.reg.progression,
            sheet,
            made.def.complexity,
        ));
        let Some(product) = self.reg.items.get(&made.def.makes) else {
            return vec![assembly; lines.len()];
        };
        let mut order: Vec<usize> = Vec::new();
        for name in &self.file.spend {
            order.extend(product.lines.iter().position(|l| l.id == *name));
        }
        let rest: Vec<usize> = (0..lines.len()).filter(|i| !order.contains(i)).collect();
        order.extend(rest);
        let mut shares = vec![f64::from(assembly); lines.len()];
        for index in order {
            if lines[index].is_none() || mean <= 0.0 {
                continue;
            }
            let room = f64::from(QUALITY_MAX) - shares[index];
            let used = points.min((room / mean).ceil());
            shares[index] = (shares[index] + used * mean).min(f64::from(QUALITY_MAX));
            points -= used;
        }
        shares
            .into_iter()
            .map(|share| {
                #[allow(
                    clippy::cast_possible_truncation,
                    clippy::cast_sign_loss,
                    reason = "a share held between nought and the whole"
                )]
                let whole = share.round().clamp(0.0, f64::from(QUALITY_MAX)) as u16;
                whole
            })
            .collect()
    }

    fn craft(&mut self, schematic: &Namespaced, by: &str, times: u32) -> Result<Done, String> {
        let made = self
            .reg
            .schematics
            .get(schematic)
            .ok_or("no such schematic")?;
        let sheet = self.sheets.get(by).ok_or("no such member")?.clone();
        crafting::taught(made, &sheet).map_err(|e| format!("{by}: {e}"))?;
        let product = self
            .reg
            .items
            .get(&made.def.makes)
            .ok_or("no such product")?;
        let mut text = String::new();
        let mut weapon = None;
        for _ in 0..times {
            let (picks, mut spent) = self.gather(made)?;
            let lines = crafting::preview_for(self.reg, &Holder::default(), made, &picks, &sheet);
            let shares = self.spent_shares(made, &lines, &sheet);
            let figures: Vec<(String, u16)> = product
                .lines
                .iter()
                .zip(&lines)
                .zip(&shares)
                .map(|((def, line), share)| {
                    let figure = line
                        .as_ref()
                        .map_or(0, |l| crafting::figure(l.ceiling, *share));
                    (def.id.clone(), figure)
                })
                .collect();
            spent.sort_by_key(|each| std::cmp::Reverse(each.0));
            for (index, amount) in spent {
                self.take_stock(index, amount);
            }
            self.batch += 1;
            let record = Record {
                item: product.id.clone(),
                count: 1,
                batch: self.batch,
                by: Some(by.to_owned()),
                lines: figures.iter().cloned().collect(),
                condition: crafting::made_condition(&self.reg.items, &product.id),
                fitted: BTreeMap::new(),
                origin: None,
            };
            let first = self.made_once.insert(schematic.clone());
            let beaten = self
                .crafts
                .note_bests(schematic, &figures.iter().cloned().collect());
            let xp = crafting::craft_xp(
                &self.reg.progression.rates.craft,
                made.def.complexity,
                first,
                beaten,
            );
            self.credit(by, XpKind::Fabrication, xp);
            weapon = weapon_figures(self.reg, &record);
            text = format!(
                "{} {}",
                product.name,
                figures
                    .iter()
                    .zip(&lines)
                    .map(|((l, v), line)| match line {
                        Some(p) => format!("{l} {v} ({}..{})", p.floor, p.ceiling),
                        None => format!("{l} {v}"),
                    })
                    .collect::<Vec<_>>()
                    .join(", ")
            );
            self.add_stock(record);
        }
        if let Some(w) = &weapon {
            let _ = write!(text, "; {}/{} mounts open", w.open, w.mounts);
            if let Some(f) = w.factor {
                let _ = write!(text, ", interval x{f:.2}");
            }
        }
        Ok(Done {
            secs: self.file.bench.session_s * f64::from(times),
            made: text,
            weapon,
        })
    }

    fn step(&mut self, what: &Do) -> Result<Done, String> {
        let screen = self.file.bench.screen_s;
        let (place, who) = match what {
            Do::Buy(_) | Do::Sell { .. } => (Some("the board"), None),
            Do::Take(_) => (Some("the terminal"), None),
            Do::Craft { by, .. } => (Some("the workbench"), Some(by)),
            Do::Learn { member, .. } => (Some("the trainer"), Some(member)),
            _ => (None, None),
        };
        if let Some(place) = place
            && !self.at_home()
        {
            return Err(format!("away from home, not at {place}"));
        }
        if let Some(who) = who
            && *who != self.active
        {
            return Err(format!("{who} is not out, {} is", self.active));
        }
        match what {
            Do::Take(name) => {
                if !self.sheets.contains_key(name) {
                    return Err("no such member".to_owned());
                }
                self.active.clone_from(name);
                Ok(Done::of(screen, format!("{name} is out")))
            }
            Do::Buy(item) => {
                let (_, price) = seller(self.reg, item).ok_or("nobody sells it")?;
                let def = self.reg.items.get(item).ok_or("no such item")?;
                if self.purse < i64::from(price) {
                    return Err(format!(
                        "{} costs {price}, the purse holds {}",
                        def.name, self.purse
                    ));
                }
                self.purse -= i64::from(price);
                let record = crafting::bought(def);
                if matches!(def.kind, ItemKind::Vehicle { .. }) {
                    self.bike = Some(record);
                } else {
                    self.add_stock(record);
                }
                Ok(Done::of(screen, format!("{} for {price}", def.name)))
            }
            Do::Ride(to) => {
                let deposit = self.dest(to)?;
                let secs = self.travel(deposit.at, f64::from(deposit.radius_m))?;
                let metres = deposit.distance_m(self.home[0], self.home[1]);
                let text = format!(
                    "{} ({}), {metres:.0} m from the city",
                    deposit.lot_name(&self.reg.resources),
                    self.reg.resources.class_of(deposit.ty).name
                );
                self.origin = Some(deposit.id);
                self.here = Some(deposit);
                Ok(Done::of(secs, text))
            }
            Do::Home => {
                let secs = self.travel(self.home, 0.0)?;
                // What was sampled goes into the locker for the bench to use.
                let lots = std::mem::take(&mut self.unstowed);
                let stow = if lots { self.file.bench.screen_s } else { 0.0 };
                Ok(Done::of(secs + stow, "home"))
            }
            Do::Find => self.find(),
            Do::Sample(units) => self.sample(*units),
            Do::Craft {
                schematic,
                by,
                times,
            } => self.craft(schematic, by, *times),
            Do::Sell { item, count } => self.sell(item, *count),
            Do::Learn { id, member } => {
                let graph = &self.reg.progression;
                let sheet = self.sheets.get_mut(member).ok_or("no such member")?;
                match sheet.learn(graph, id, true) {
                    Ok(()) => Ok(Done::of(screen, format!("{member} learns {id}"))),
                    Err(veyra_rules::progression::LearnRefused::AlreadyLearned(_)) => {
                        Ok(Done::of(0.0, format!("{member} knows {id} already")))
                    }
                    Err(e) => Err(format!("{member}: {e}")),
                }
            }
            Do::Plant => self.plant(),
            Do::Collect => self.collect(),
        }
    }

    fn tool_skill(
        &self,
        sheet: &CharacterSheet,
    ) -> Result<veyra_rules::deposits::SurveySkill, String> {
        let tool = survey_tool(self.reg).ok_or("no survey tool")?;
        Ok(skill(
            &self.reg.resources,
            &self.reg.items,
            &self.reg.progression,
            &tool,
            sheet,
        ))
    }

    #[allow(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "pings are a few, rounded up from a ratio"
    )]
    fn find(&mut self) -> Result<Done, String> {
        let deposit = self.here.clone().ok_or("not at a deposit")?;
        let skill = self.tool_skill(&self.sheet().clone())?;
        let base = self.tool_skill(&CharacterSheet::default())?;
        let scale = (base.range_m / skill.range_m.max(1.0)).min(1.0);
        let (pings, walked) = if skill.peak {
            (1, 0.0)
        } else {
            (
                ((f64::from(self.file.find.pings) * scale).ceil() as u32).max(1),
                self.file.find.walked_m * scale,
            )
        };
        let secs = f64::from(pings) * f64::from(skill.channel_s) + walked / self.foot.0;
        let fresh = self.found.insert(deposit.id);
        let xp = survey_xp(
            &self.reg.progression.rates.survey,
            SurveyEvent::Ping {
                found: fresh,
                better: false,
            },
        );
        let who = self.active.clone();
        self.credit(&who, XpKind::Survey, xp);
        Ok(Done::of(
            secs,
            format!("{pings} pings and {walked:.0} m of walking to the lead"),
        ))
    }

    fn sample_pm(&self, deposit: &Deposit, peak: bool) -> u16 {
        if peak {
            deposit.peak
        } else {
            self.file.sample_at_pm.min(deposit.peak)
        }
    }

    fn sample(&mut self, units: u32) -> Result<Done, String> {
        let deposit = self.here.clone().ok_or("not at a deposit")?;
        if !self.found.contains(&deposit.id) {
            return Err("the deposit is not found yet".to_owned());
        }
        let skill = self.tool_skill(&self.sheet().clone())?;
        let pm = self.sample_pm(&deposit, skill.peak);
        let each = cycle_units(skill.units, pm).max(1);
        let cycles = units.div_ceil(each);
        let who = self.active.clone();
        for _ in 0..cycles {
            let first = self.sampled.insert(deposit.id);
            let xp = survey_xp(
                &self.reg.progression.rates.survey,
                SurveyEvent::Cycle { first, pm },
            );
            self.credit(&who, XpKind::Survey, xp);
        }
        let got = cycles * each;
        let record = lot(&deposit, &self.reg.resources, got).ok_or("no lot")?;
        self.add_stock(record);
        self.unstowed = true;
        Ok(Done::of(
            f64::from(cycles) * f64::from(skill.cycle_s),
            format!("{got} units at {pm} per-mille"),
        ))
    }

    fn sell(&mut self, item: &Namespaced, count: u32) -> Result<Done, String> {
        let screen = self.file.bench.screen_s;
        let state = ProcurementState::default();
        for buyer in self.reg.buyers.iter() {
            for (_, line) in buyer.lines() {
                let wanted = match &line.wants {
                    Wants::Item(i) => i == item,
                    Wants::Class(_) => false,
                };
                if !wanted {
                    continue;
                }
                let held: u32 = self
                    .stock
                    .iter()
                    .filter(|r| r.item == *item)
                    .map(|r| r.count)
                    .sum();
                if held < count {
                    return Err(format!("holds {held} of {count} to sell"));
                }
                if procurement::wanted(buyer, line, &state) < count {
                    return Err(format!(
                        "{} wants {} only",
                        buyer.name,
                        procurement::wanted(buyer, line, &state)
                    ));
                }
                let mut left = count;
                let mut pay = 0_u64;
                while left > 0 {
                    let index = self
                        .stock
                        .iter()
                        .position(|r| r.item == *item)
                        .ok_or("stock vanished")?;
                    let record = self.stock[index].clone();
                    let grade = procurement::grade_of(&self.reg.items, line, &record)
                        .ok_or("not graded")?;
                    if grade < line.min {
                        return Err(format!(
                            "{} wants {} {} or better, this is {grade}",
                            buyer.name,
                            line.grade.key(),
                            line.min
                        ));
                    }
                    let take = record.count.min(left);
                    pay += u64::from(procurement::pay_for(
                        procurement::pays(buyer, line, grade),
                        take,
                    ));
                    self.take_stock(index, take);
                    left -= take;
                }
                let pay = i64::try_from(pay).unwrap_or(i64::MAX);
                self.purse += pay;
                return Ok(Done::of(
                    screen,
                    format!("{count} sold to {} for {pay}", buyer.name),
                ));
            }
        }
        Err("no order wants it".to_owned())
    }

    fn plant(&mut self) -> Result<Done, String> {
        let deposit = self.here.clone().ok_or("not at a deposit")?;
        let index = self
            .stock
            .iter()
            .position(|r| {
                matches!(
                    self.reg.items.get(&r.item).map(|d| &d.kind),
                    Some(ItemKind::Harvester { .. })
                )
            })
            .ok_or("no harvester is carried")?;
        let machine = self.stock[index].clone();
        let station = harvest::station(
            self.reg.resources.harvest(),
            &self.reg.items,
            &machine,
            &self.reg.progression,
            self.sheet(),
        );
        if station.count == 0 {
            return Err(format!("{} has no Stations box", self.active));
        }
        if !self.found.contains(&deposit.id) {
            return Err("the deposit is not found yet".to_owned());
        }
        let skill = self.tool_skill(&self.sheet().clone())?;
        let pm = self.sample_pm(&deposit, skill.peak);
        if pm < self.reg.resources.harvest().min_pm {
            return Err(format!(
                "{pm} per-mille is under the least a machine plants on"
            ));
        }
        self.take_stock(index, 1);
        let text = format!(
            "{} units an hour, {} in the hopper",
            harvest::made_in(station.rate, pm, u32::MAX, 3600),
            station.hopper
        );
        self.planted = Some(Planted {
            deposit,
            pm,
            rate: station.rate,
            hopper: station.hopper,
            since_s: self.clock,
        });
        Ok(Done::of(self.file.bench.screen_s, text))
    }

    fn collect(&mut self) -> Result<Done, String> {
        let machine = self.planted.take().ok_or("nothing is planted")?;
        let secs = self.travel(machine.deposit.at, 0.0)?;
        let played = (self.clock + secs - machine.since_s).max(0.0);
        #[allow(
            clippy::cast_possible_truncation,
            clippy::cast_sign_loss,
            reason = "seconds of an hour or two"
        )]
        let units = harvest::made_in(machine.rate, machine.pm, machine.hopper, played as u32);
        let record = lot(&machine.deposit, &self.reg.resources, units).ok_or("no lot")?;
        self.add_stock(record);
        self.unstowed = true;
        let xp = survey_xp(
            &self.reg.progression.rates.survey,
            SurveyEvent::Harvest { units },
        );
        let who = self.active.clone();
        self.credit(&who, XpKind::Survey, xp);
        self.planted = Some(Planted {
            since_s: self.clock + secs,
            ..machine
        });
        Ok(Done::of(
            secs + self.file.bench.screen_s,
            format!("{units} units after {:.0} min", played / 60.0),
        ))
    }

    fn xp_text(&self) -> String {
        let mut text = Vec::new();
        for (name, sheet) in &self.sheets {
            let parts: Vec<String> = [(XpKind::Survey, "Survey"), (XpKind::Fabrication, "Fab")]
                .iter()
                .filter(|(kind, _)| sheet.xp(*kind) > 0)
                .map(|(kind, word)| format!("{word} {}", sheet.xp(*kind)))
                .collect();
            if !parts.is_empty() {
                text.push(format!(
                    "{}: {}",
                    name.split(' ').next().unwrap_or(name),
                    parts.join(", ")
                ));
            }
        }
        text.join("; ")
    }
}

/// A label for a step.
fn label(what: &Do) -> String {
    match what {
        Do::Buy(i) => format!("Buy {i}"),
        Do::Take(m) => format!("Take {m}"),
        Do::Ride(Dest::Nearest(c)) => format!("Ride to nearest {c}"),
        Do::Ride(Dest::Best {
            class,
            line,
            at_least,
            ..
        }) => format!("Ride to nearest {class} with {line} {at_least}+"),
        Do::Find => "Find".to_owned(),
        Do::Sample(n) => format!("Sample {n}"),
        Do::Plant => "Plant".to_owned(),
        Do::Collect => "Collect".to_owned(),
        Do::Home => "Home".to_owned(),
        Do::Craft {
            schematic,
            by,
            times,
        } => format!("Craft {schematic} x{times} ({by})"),
        Do::Sell { item, count } => format!("Sell {count} {item}"),
        Do::Learn { id, member } => format!("Learn {id} ({member})"),
    }
}

/// Walk the file's path over `ground` and give the timeline. A variant
/// changes the sheets before the first step.
#[must_use]
pub fn run(
    registries: &GameRegistries,
    ground: &dyn Ground,
    file: &FirstHour,
    home: [f64; 2],
    foot: (f64, f64),
    variant: &Variant,
) -> Timeline {
    let mut sheets = members(registries);
    let graph = &registries.progression;
    for id in &variant.strip {
        for sheet in sheets.values_mut() {
            sheet
                .learned
                .retain(|have| have != id && !needs(graph, have, id));
        }
    }
    for id in &variant.grant {
        for sheet in sheets.values_mut() {
            grant(graph, sheet, id);
        }
    }
    let arrival = registries
        .sheets
        .household
        .as_ref()
        .map(|h| h.arrival.name.clone())
        .unwrap_or_default();
    let mut state = State {
        reg: registries,
        ground,
        file,
        foot,
        home,
        pos: home,
        clock: 0.0,
        purse: registries
            .items
            .starting()
            .map_or(0, |s| i64::from(s.purse)),
        sheets,
        active: arrival,
        bike: None,
        stock: Vec::new(),
        here: None,
        found: BTreeSet::new(),
        sampled: BTreeSet::new(),
        made_once: BTreeSet::new(),
        crafts: crafting::CraftState::default(),
        origin: None,
        unstowed: false,
        planted: None,
        batch: 0,
    };
    if let Some(start) = registries.items.starting() {
        state.stock.extend(start.store.iter().cloned());
        for each in &start.lots {
            let wanted = |d: &Deposit| {
                let class = state.reg.resources.class_of(d.ty);
                match &each.of {
                    strata_registry::item::LotOf::Type(t) => state
                        .reg
                        .resources
                        .get(d.ty)
                        .is_some_and(|def| def.id == *t),
                    strata_registry::item::LotOf::Class(c) => class.id == *c,
                }
            };
            if let Some(d) = ground
                .near(home, start.lots_within_m)
                .into_iter()
                .find(wanted)
                && let Some(record) = lot(&d, &registries.resources, each.units)
            {
                state.stock.push(record);
            }
        }
    }
    let mut line = Timeline::default();
    for step in &file.path {
        if variant.no_learn && matches!(step.what, Do::Learn { .. }) {
            continue;
        }
        let outcome = state.step(&step.what);
        let (secs, made, weapon, refused) = match outcome {
            Ok(done) => (done.secs, done.made, done.weapon, None),
            Err(why) => (0.0, String::new(), None, Some(why)),
        };
        state.clock += secs;
        line.rows.push(Row {
            label: label(&step.what),
            mark: step.mark.clone(),
            secs,
            clock_s: state.clock,
            purse: state.purse,
            xp: state.xp_text(),
            made,
            weapon,
            refused: refused.clone(),
        });
        if refused.is_some() {
            break;
        }
    }
    line
}

/// Whether box `have` needs `id` somewhere down its chain.
fn needs(graph: &strata_registry::progression::SkillGraph, have: &str, id: &str) -> bool {
    graph
        .prerequisites(have)
        .into_iter()
        .any(|p| p == id || needs(graph, p, id))
}

fn grant(graph: &strata_registry::progression::SkillGraph, sheet: &mut CharacterSheet, id: &str) {
    for p in graph.prerequisites(id) {
        grant(graph, sheet, p);
    }
    sheet.learned.insert(id.to_owned());
}

/// The timeline as a table of text.
#[must_use]
pub fn table(line: &Timeline) -> String {
    let mut out = String::new();
    let _ = writeln!(
        out,
        "{:>3} {:>6} {:>7} {:>6}  step",
        "#", "min", "clock", "purse"
    );
    for (n, r) in line.rows.iter().enumerate() {
        let mark = r
            .mark
            .as_deref()
            .map(|m| format!(" [{m}]"))
            .unwrap_or_default();
        let _ = writeln!(
            out,
            "{:>3} {:>6.2} {:>7.2} {:>6}  {}{mark}",
            n + 1,
            r.secs / 60.0,
            r.clock_s / 60.0,
            r.purse,
            r.label
        );
        if !r.made.is_empty() {
            let _ = writeln!(out, "{:>25}{}", "", r.made);
        }
        if let Some(why) = &r.refused {
            let _ = writeln!(out, "{:>25}REFUSED: {why}", "");
        }
        if !r.xp.is_empty() {
            let _ = writeln!(out, "{:>25}xp {}", "", r.xp);
        }
    }
    out
}

/// The timeline as Markdown.
#[must_use]
pub fn markdown(line: &Timeline) -> String {
    let mut out = String::from(
        "| # | Step | Min | Clock | Purse | Made | XP |\n| --- | --- | --- | --- | --- | --- | --- |\n",
    );
    for (n, r) in line.rows.iter().enumerate() {
        let mark = r
            .mark
            .as_deref()
            .map(|m| format!(" **{m}**"))
            .unwrap_or_default();
        let made = match &r.refused {
            Some(why) => format!("REFUSED: {why}"),
            None => r.made.clone(),
        };
        let _ = writeln!(
            out,
            "| {} | {}{mark} | {:.2} | {:.2} | {} | {made} | {} |",
            n + 1,
            r.label,
            r.secs / 60.0,
            r.clock_s / 60.0,
            r.purse,
            r.xp
        );
    }
    out
}

/// The band table: each band, the minute reached and the verdict.
#[must_use]
pub fn bands_table(file: &FirstHour, line: &Timeline) -> String {
    let mut out = String::from("| Band | By | Reached | |\n| --- | --- | --- | --- |\n");
    for b in &file.bands {
        let (reached, verdict) = match line.reached(&b.id) {
            Some(at) if at <= b.by_min => (format!("{at:.1}"), "ok"),
            Some(at) => (
                format!("{at:.1}"),
                if b.waived.is_some() {
                    "waived"
                } else {
                    "FAILS"
                },
            ),
            None => (
                "never".to_owned(),
                if b.waived.is_some() {
                    "waived"
                } else {
                    "FAILS"
                },
            ),
        };
        let _ = writeln!(
            out,
            "| {} | {:.0} | {reached} | {verdict} |",
            b.id, b.by_min
        );
    }
    out
}

/// The two questions of the document: what a Surveyor box buys in minutes on
/// the path, and what a made carbine has over a bought one at each mark.
#[must_use]
pub fn questions(shipped: &Shipped, file: &FirstHour) -> String {
    let reg = &shipped.registries;
    let go = |variant: &Variant| {
        run(
            reg,
            &shipped.field,
            file,
            shipped.home,
            shipped.foot,
            variant,
        )
    };
    let mut out = String::from(
        "## What a Surveyor box buys\n\nMinutes at the end of the path with the box on every sheet and with it taken off (Learn steps left out of both).\n\n| Box | Without | With | Saved |\n| --- | --- | --- | --- |\n",
    );
    for id in [
        "surveyor.deposits.1",
        "surveyor.deposits.2",
        "surveyor.deposits.3",
        "surveyor.deposits.4",
        "surveyor.reading.4",
        "surveyor.routes.2",
        "surveyor.routes.4",
    ] {
        // Without: the box's own prerequisites and not the box, so the
        // saving is this box's and not its branch's.
        let without = go(&Variant {
            strip: vec![id.to_owned()],
            grant: reg
                .progression
                .prerequisites(id)
                .into_iter()
                .map(str::to_owned)
                .collect(),
            no_learn: true,
        });
        let with = go(&Variant {
            grant: vec![id.to_owned()],
            no_learn: true,
            ..Variant::default()
        });
        let (a, b) = (without.end_min(), with.end_min());
        let note = if without.stopped().is_some() || with.stopped().is_some() {
            " (a run stopped)"
        } else {
            ""
        };
        let _ = writeln!(out, "| {id} | {a:.1} | {b:.1} | {:.1}{note} |", a - b);
    }
    out.push_str("\n## A made carbine against a bought one\n\n| Mark | Minute | Made: mounts open, interval factor | Bought: mounts open, interval factor | Bought still worse |\n| --- | --- | --- | --- | --- |\n");
    let line = go(&Variant::default());
    let bought =
        reg.items
            .iter()
            .find(|d| {
                matches!(d.kind, ItemKind::Weapon { .. })
                    && reg.schematics.iter().any(|s| {
                        s.def.makes == d.id && s.def.id.path() == "schematic.patrol_carbine"
                    })
            })
            .map(crafting::bought)
            .and_then(|r| weapon_figures(reg, &r));
    for mark in ["first_made_carbine", "muzzle_carbine", "fore_end_carbine"] {
        let (Some(at), Some(made), Some(base)) =
            (line.reached(mark), line.weapon_at(mark), bought.as_ref())
        else {
            let _ = writeln!(out, "| {mark} | never | | | |");
            continue;
        };
        let (mf, bf) = (made.factor.unwrap_or(1.0), base.factor.unwrap_or(1.0));
        let durability = |w: &WeaponFigures| {
            w.lines
                .iter()
                .find(|(id, _)| id == "durability")
                .map_or(0, |(_, v)| *v)
        };
        let (md, bd) = (durability(made), durability(base));
        let worse = base.open <= made.open
            && bf >= mf
            && bd <= md
            && (base.open < made.open || bf > mf || bd < md);
        let _ = writeln!(
            out,
            "| {mark} | {at:.1} | {}/{}, x{mf:.2}, {md} | {}/{}, x{bf:.2}, {bd} | {} |",
            made.open,
            made.mounts,
            base.open,
            base.mounts,
            if worse { "yes" } else { "no" }
        );
    }
    out
}

/// Fail with the band sentences when any band fails.
///
/// # Errors
/// The failing bands.
pub fn ensure_held(
    file: &FirstHour,
    line: &Timeline,
    harvest: Option<HarvestNumbers>,
) -> Result<()> {
    let (failed, _) = judge(file, line, harvest);
    if failed.is_empty() {
        Ok(())
    } else {
        bail!("{}", failed.join("\n"))
    }
}

#[cfg(test)]
mod tests;
