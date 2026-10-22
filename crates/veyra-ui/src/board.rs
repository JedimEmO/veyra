//! A buyer's procurement board: what Pelagic Works wants of the
//! household, and what the household can sell it (ticket 0007, step 7;
//! `docs/design/crafting.md`, section 7).
//!
//! A section an order, a row a line: what it wants, how much of it is
//! taken, the best of the household's stock for it and one button that
//! sells that stock up to what is wanted. No rule lives here and nothing
//! is worked out: `veyra_sim::procurement` gives the amount still
//! wanted, the offers in reach, the best grade held short of the minimum
//! and the quote a sale would make, and a press puts a
//! `ProcurementRequest::Sell` in the sim's inbox for the tick to judge.
//! [`BoardView::of`] gathers those answers into words and [`screen_def`]
//! lays them out, so what the screen says can be checked without one.
//! Local-only: nothing here is replicated.

use bevy::ecs::prelude::*;
use bevy::ecs::system::SystemParam;
use slotted::menu::MenuChoice;
use slotted::theme::Role;
use slotted::ui::def::{
    Align, ButtonOpts, Justify, Layout, Length, Padding, Presentation, PresentationMode, ScreenDef,
    ScreenKind, Tags, TextRole, Transition, UiNodeDef, WidgetKind,
};
use strata_core::Seed;
use strata_registry::item::ItemKind;
use strata_registry::procurement::{BuyerDef, GradeOn, LineSpec, OrderKind, Wants};
use strata_registry::{GameRegistries, Namespaced, PostingRules};
use veyra_sim::deeds::{LeaseRefusal, Terms, check_lease, lease_buyer};
use veyra_sim::deposits::Compass;
use veyra_sim::deposits::Purse;
use veyra_sim::encounters::Cleared;
use veyra_sim::fitting::WorkbenchAccess;
use veyra_sim::items::{HouseholdStore, Pack};
use veyra_sim::postings::{
    Candidate, JobAnswer, JobOutcome, JobRefusal, Jobs, Posting, PostingCandidates, check_collect,
    check_take, offered,
};
use veyra_sim::procurement::{
    self, LeaseAnswer, ProcurementInbox, ProcurementRequest, ProcurementState, PurchaseAnswer,
    PurchaseRefusal, SaleAnswer, SaleRefusal,
};
use veyra_sim::progression::Reach;
use veyra_sim::{LocalPlayer, Registries, WorldPosition, WorldSeed};

use super::card;
use super::claims::{Books, Standing};
use super::crafting::{
    BOX_ROLE, FIGURE_ROLE, HEAD_ROLE, NAME_ROLE, NOTE_ROLE, STAT_ROLE, STAT_VALUE_ROLE, SUB_ROLE,
    Swatch, WHY_ROLE, act, column, fill, grows, line_in, panel, row, spacer, stock_name,
    swatch_of_item, text_in, well, with_id, words,
};
use super::factory::{PAUSED_ROLE, PROGRESS_FILL_ROLE, PROGRESS_ROLE};
use super::strings::{counted, grouped};
use strata_ui::sim_screen::{self, ChangeSource, ScreenStep, Seen, SimScreen, SimScreenApp, moved};

/// The screen kind.
pub const KIND: &str = "strata:board";
/// The `test_id` of the screen's root.
pub const ROOT_ID: &str = "board.root";
/// The title.
pub const TITLE_ID: &str = "board.title";
/// The purse.
pub const PURSE_ID: &str = "board.purse";
/// The Close button.
pub const CLOSE_ID: &str = "board.close";
/// The line "Nothing sold here yet."
pub const NOTHING_ID: &str = "board.nothing";
/// The prefix of a line's Sell button, then the line's id.
pub const SELL_TAG: &str = "board.sell:";
/// The prefix of a line's wants text.
pub const WANTS_TAG: &str = "board.wants:";
/// The prefix of a line's state text (filled, refills with time).
pub const STATE_TAG: &str = "board.state:";
/// The prefix of a line's "yours" text.
pub const YOURS_TAG: &str = "board.yours:";
/// The prefix of the reason beside a grey Sell button.
pub const WHY_TAG: &str = "board.why:";
/// The prefix of a FOR SALE row's Buy button, then the item's id.
pub const BUY_TAG: &str = "board.buy:";
/// The prefix of a FOR SALE row's price text.
pub const PRICE_TAG: &str = "board.price:";
/// The prefix of a FOR SALE row's purse text.
pub const HAVE_TAG: &str = "board.have:";
/// The prefix of the reason beside a grey Buy button.
pub const BUY_WHY_TAG: &str = "board.buywhy:";
/// The line under the title that says how the board's faction is towards the
/// household.
pub const STANCE_ID: &str = "board.stance";
/// The Lease button.
pub const LEASE_ID: &str = "board.lease";
/// The lease row's price text.
pub const LEASE_PRICE_ID: &str = "board.lease.price";
/// The lease row's time left, while one runs.
pub const LEASE_LEFT_ID: &str = "board.lease.left";
/// The reason beside a grey Lease button.
pub const LEASE_WHY_ID: &str = "board.lease.why";
/// The prefix of an offered posting's Take button, then the camp's key.
pub const TAKE_TAG: &str = "board.take:";
/// The prefix of a done job's Collect button, then the camp's key.
pub const COLLECT_TAG: &str = "board.collect:";
/// The prefix of a work row's state text, then the camp's key.
pub const WORK_STATE_TAG: &str = "board.work.state:";
/// The prefix of the reason beside a grey Take or Collect button.
pub const WORK_WHY_TAG: &str = "board.work.why:";
/// The line that says no work is posted.
pub const WORK_NONE_ID: &str = "board.work.none";
/// The prefix of an entry of SOLD HERE, then its place from the top.
pub const SOLD_TAG: &str = "board.sold:";

/// The entries SOLD HERE shows.
const SOLD_SHOWN: usize = 6;
/// The bar of how much is taken, in the screen's units.
const BAR_WIDTH: f32 = 260.;
/// The panel's width, in the screen's units.
const WIDTH: f32 = 980.;

/// Every theme role of the screen's own: none, it draws with the
/// factory's and the fitting's.
pub const ROLES: [&str; 0] = [];

/// The screen kind.
#[must_use]
pub fn kind() -> ScreenKind {
    ScreenKind::new(KIND)
}

/// The `test_id` of the Sell button of `line`.
#[must_use]
pub fn sell_id(line: &str) -> String {
    format!("{SELL_TAG}{line}")
}

/// The `test_id` of the Buy button of `item`.
#[must_use]
pub fn buy_id(item: &Namespaced) -> String {
    format!("{BUY_TAG}{item}")
}

// ---- the state ----

/// The board screen's state: whose board it is, what it was last built
/// from and the control the focus goes to.
#[derive(Resource, Debug, Default)]
pub struct BoardUi {
    /// The buyer whose board is open.
    pub buyer: Option<Namespaced>,
    /// The `test_id` of the control the focus goes to at the next build.
    last: Option<String>,
    /// A rebuild is due whatever the view says.
    dirty: bool,
    /// What the open screen was built from.
    view: Option<BoardView>,
    /// The requests sent that the sim has not answered yet.
    asked: usize,
}

impl BoardUi {
    /// What the open screen was last built from.
    #[must_use]
    pub const fn view(&self) -> Option<&BoardView> {
        self.view.as_ref()
    }
}

// ---- the view ----

/// One line of an order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LineView {
    /// The line's id.
    pub id: String,
    /// The image of what it wants, the pack's own; for a class, the first
    /// lot of it the catalogue holds.
    pub swatch: Option<Swatch>,
    /// What it wants: the item's name, or the class's.
    pub wants: String,
    /// How much is still wanted, in words.
    pub asks: String,
    /// The minimum and what it grades on: `min Charge density 500`.
    pub min: String,
    /// What a unit is paid: `120 cr each`.
    pub price: String,
    /// How much of the quantity is taken, in thousandths.
    pub share: u16,
    /// `filled`, `refills with time`, or both.
    pub state: String,
    /// The household's side in words.
    pub yours: String,
    /// Whether `yours` is a miss, drawn in the warm colour.
    pub short: bool,
    /// The Sell button's label.
    pub label: String,
    /// The request the button sends; none when it cannot.
    pub request: Option<ProcurementRequest>,
    /// Why the button is grey, in words.
    pub why: Option<String>,
}

/// An order: a heading and its lines.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OrderView {
    /// The heading, in capitals.
    pub head: String,
    /// Its lines.
    pub lines: Vec<LineView>,
}

/// A row of FOR SALE: one thing the buyer sells.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SaleView {
    /// The item.
    pub item: Namespaced,
    /// Its image, the pack's own.
    pub swatch: Option<Swatch>,
    /// Its name.
    pub name: String,
    /// `450 cr`.
    pub price: String,
    /// `purse 600 cr`.
    pub purse: String,
    /// The Buy button's label.
    pub label: String,
    /// The request the button sends; none when it cannot.
    pub request: Option<ProcurementRequest>,
    /// Why the button is grey, in words.
    pub why: Option<String>,
}

/// An entry of SOLD HERE.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SoldView {
    /// `40 Cell cores · batch 0412 · 5,875 cr`.
    pub said: String,
}

/// The row that sells a lease of the board's faction's concessions.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LeaseView {
    /// `LEASE · Pelagic Works' concessions`.
    pub head: String,
    /// `380 cr for 6 hours`.
    pub price: String,
    /// `Leased · 5 h 12 min left` while one runs; empty otherwise.
    pub left: String,
    /// The button's label.
    pub label: String,
    /// The request the button sends; none when it cannot.
    pub request: Option<ProcurementRequest>,
    /// Why the button is grey, in words.
    pub why: Option<String>,
}

/// Where a piece of work stands.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WorkState {
    /// Posted, not taken.
    Offered,
    /// Taken, its camp not yet cleared.
    Taken,
    /// Taken and cleared, the pay not yet collected.
    Done,
}

/// A row of WORK: one camp to clear and what it pays.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkView {
    /// The camp's key.
    pub key: u64,
    /// `Clear Snogirk Cut`.
    pub head: String,
    /// `held by the Skan Gang`; empty where nobody holds it.
    pub who: String,
    /// `6 km north-east`, to the area it is heard of in.
    pub lies: String,
    /// `900 cr`.
    pub pay: String,
    /// Where it stands.
    pub state: WorkState,
    /// What the row says of a taken one; empty for an offered one.
    pub said: String,
    /// The button's label: Take, or Collect for a done one; empty for a
    /// taken one, which has none.
    pub label: String,
    /// The request the button sends; none when it cannot.
    pub request: Option<ProcurementRequest>,
    /// Why the button is grey, in words.
    pub why: Option<String>,
}

/// Everything the screen shows.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BoardView {
    /// `Pelagic Works · procurement`.
    pub title: String,
    /// How the board's faction is towards the household, in words; empty
    /// for a board no faction keeps.
    pub stance: String,
    /// The lease the board sells, if its faction has concessions.
    pub lease: Option<LeaseView>,
    /// The work posted and taken; none where the data has no postings.
    pub work: Option<Vec<WorkView>>,
    /// The household's credits.
    pub purse: u32,
    /// The orders.
    pub orders: Vec<OrderView>,
    /// What the buyer sells the household.
    pub sells: Vec<SaleView>,
    /// What was sold here, newest first.
    pub sold: Vec<SoldView>,
    /// The control the focus starts on.
    pub focus: Option<String>,
}

/// What the WORK rows are read from.
pub struct Work<'a> {
    /// The data's dials.
    pub rules: &'a PostingRules,
    /// The camps the board may post.
    pub candidates: &'a [Candidate],
    /// The camps cleared.
    pub cleared: &'a Cleared,
    /// The jobs held.
    pub jobs: &'a Jobs,
    /// The played second now.
    pub now: f64,
    /// The world's seed.
    pub seed: Seed,
    /// The board's seat, world `(x, z)`.
    pub seat: [f64; 2],
    /// Where the member stands, world `(x, z)`.
    pub at: [f64; 2],
}

/// What the view is read from.
pub struct Floor<'a> {
    /// The data.
    pub registries: &'a GameRegistries,
    /// The player's pack.
    pub pack: &'a Pack,
    /// The household's store.
    pub store: &'a HouseholdStore,
    /// The household's credits.
    pub purse: Purse,
    /// What is sold and taken.
    pub state: &'a ProcurementState,
    /// Whether the board is within reach, by the sim's own rule.
    pub at_board: bool,
    /// The faction books its stance and lease are read from.
    pub books: Option<Books<'a>>,
    /// The postings, where the data has them.
    pub work: Option<Work<'a>>,
}

/// A job refusal as a player reads it.
#[must_use]
pub fn job_words(refusal: JobRefusal) -> String {
    match refusal {
        JobRefusal::NotAtBoard => words("strata.board.why.reach"),
        JobRefusal::NotOffered => words("strata.board.why.work.offered"),
        JobRefusal::TooMany => words("strata.board.why.work.many"),
        JobRefusal::NotDone => words("strata.board.why.work.undone"),
        JobRefusal::NotTaken => words("strata.board.why.work.taken"),
    }
    .to_owned()
}

/// The camp's area as the member reads it: how far and which way.
fn lies_words(from: [f64; 2], to: [f64; 2]) -> String {
    super::bench::where_words(
        (to[0] - from[0]).hypot(to[1] - from[1]),
        Compass::of(from, to),
    )
}

/// A posting's verb and place: "Clear {camp}" unless the kind has a verb of
/// its own (a wreck's cargo is recovered, a machine is secured). A posting
/// saved before kinds were kept has none and is a camp.
fn work_head(kind: &str, name: &str) -> String {
    let said = super::strings::english(&format!("strata.board.work.head.{kind}"))
        .unwrap_or_else(|| words("strata.board.work.head"));
    fill(said, "camp", name)
}

/// Whose place it is: the band that holds a camp, and for a place with no
/// band nothing, or "unclaimed salvage" for a wreck or a machine, never the
/// corporate holder of its ground as if it were the enemy.
fn work_who(kind: &str, band: &str) -> String {
    if band.is_empty() {
        super::strings::english(&format!("strata.board.work.unheld.{kind}"))
            .unwrap_or_default()
            .to_owned()
    } else {
        fill(words("strata.board.work.who"), "band", band)
    }
}

/// The WORK rows of `buyer`'s board: the jobs held, then what is posted now.
/// Every grey button is the sim's own `check_take` or `check_collect`.
fn work_views(work: &Work<'_>, buyer: &Namespaced, at_board: bool) -> Vec<WorkView> {
    let cr = |pay: u32| fill(words("strata.board.cr"), "n", &grouped(pay));
    let head = |posting: &Posting| work_head(&posting.kind, &posting.name);
    let who = |posting: &Posting| work_who(&posting.kind, &posting.faction);
    let mut rows = Vec::new();
    for (key, job) in work
        .jobs
        .taken
        .iter()
        .filter(|(_, job)| job.buyer == *buyer)
    {
        let posting = &job.posting;
        let lies = lies_words(work.at, posting.heard.centre);
        let collect = check_collect(at_board, work.jobs, buyer, *key);
        rows.push(WorkView {
            key: *key,
            head: head(posting),
            who: who(posting),
            lies: lies.clone(),
            pay: cr(posting.pay),
            state: if job.done {
                WorkState::Done
            } else {
                WorkState::Taken
            },
            said: if job.done {
                words("strata.board.work.cleared").to_owned()
            } else {
                fill(words("strata.board.work.taken"), "lies", &lies)
            },
            label: if job.done {
                words("strata.board.work.collect").to_owned()
            } else {
                String::new()
            },
            request: (job.done && collect.is_ok()).then(|| ProcurementRequest::Collect {
                buyer: buyer.clone(),
                key: *key,
            }),
            why: job.done.then(|| collect.err().map(job_words)).flatten(),
        });
    }
    let shown = offered(
        work.rules,
        buyer,
        work.seat,
        work.candidates,
        work.cleared,
        work.jobs,
        work.now,
        work.seed,
    );
    for posting in &shown {
        let check = check_take(work.rules, at_board, &shown, work.jobs, posting.key);
        rows.push(WorkView {
            key: posting.key,
            head: head(posting),
            who: who(posting),
            lies: lies_words(work.at, posting.heard.centre),
            pay: cr(posting.pay),
            state: WorkState::Offered,
            said: String::new(),
            label: words("strata.board.work.take").to_owned(),
            request: check.is_ok().then(|| ProcurementRequest::TakeJob {
                buyer: buyer.clone(),
                key: posting.key,
            }),
            why: check.err().map(job_words),
        });
    }
    rows
}

/// A sale's refusal as a player reads it.
#[must_use]
pub fn refusal_words(refusal: &SaleRefusal) -> String {
    match refusal {
        SaleRefusal::NoBoard => words("strata.board.why.reach").to_owned(),
        SaleRefusal::UnknownLine => words("strata.board.why.unknown").to_owned(),
        SaleRefusal::Filled => words("strata.board.filled").to_owned(),
        SaleRefusal::NotWanted => words("strata.board.why.unwanted").to_owned(),
        SaleRefusal::BelowMinimum { has, needs } => fill(
            &fill(words("strata.board.why.under"), "has", &has.to_string()),
            "needs",
            &needs.to_string(),
        ),
        SaleRefusal::NoneHeld => words("strata.board.none").to_owned(),
        SaleRefusal::Hostile => words("strata.board.why.hostile").to_owned(),
    }
}

/// A lease's refusal as a player reads it.
#[must_use]
pub fn lease_words(refusal: &LeaseRefusal) -> String {
    match refusal {
        LeaseRefusal::NotAtBoard => words("strata.board.why.reach").to_owned(),
        LeaseRefusal::NoLease => words("strata.board.why.lease").to_owned(),
        LeaseRefusal::Hostile => words("strata.board.why.hostile").to_owned(),
        LeaseRefusal::Short { needs, have } => fill(
            &fill(
                words("strata.board.why.broke"),
                "short",
                &grouped(needs.saturating_sub(*have)),
            ),
            "needs",
            &grouped(*needs),
        ),
    }
}

/// A purchase's refusal as a player reads it.
#[must_use]
pub fn purchase_words(refusal: &PurchaseRefusal) -> String {
    match refusal {
        PurchaseRefusal::NotAtBoard => words("strata.board.why.reach").to_owned(),
        PurchaseRefusal::NotSold => words("strata.board.why.unsold").to_owned(),
        PurchaseRefusal::Short { needs, have } => fill(
            &fill(
                words("strata.board.why.broke"),
                "short",
                &grouped(needs.saturating_sub(*have)),
            ),
            "needs",
            &grouped(*needs),
        ),
        PurchaseRefusal::NoRoom => words("strata.board.why.room").to_owned(),
        PurchaseRefusal::Hostile => words("strata.board.why.hostile").to_owned(),
    }
}

/// The FOR SALE rows of `buyer`, each with what asking for it would be told.
fn sale_views(floor: &Floor<'_>, buyer: &BuyerDef, terms: Terms) -> Vec<SaleView> {
    buyer
        .sells
        .iter()
        .map(|sale| {
            let name = floor
                .registries
                .items
                .get(&sale.item)
                .map_or_else(|| sale.item.to_string(), |def| def.name.clone());
            let check = procurement::check_purchase_on(
                floor.registries,
                floor.at_board,
                terms,
                &floor.purse,
                floor.pack,
                Some(floor.store),
                &buyer.id,
                &sale.item,
            );
            SaleView {
                swatch: swatch_of_item(floor.registries, &sale.item),
                item: sale.item.clone(),
                name,
                price: fill(
                    words("strata.board.cr"),
                    "n",
                    &grouped(Terms::scale(sale.price, terms.sells_pm)),
                ),
                purse: fill(words("strata.board.purse"), "n", &grouped(floor.purse.0)),
                label: words("strata.board.buy").to_owned(),
                request: check.is_ok().then(|| ProcurementRequest::Buy {
                    buyer: buyer.id.clone(),
                    item: sale.item.clone(),
                }),
                why: check.err().map(|refusal| purchase_words(&refusal)),
            }
        })
        .collect()
}

/// What a line wants, by name.
fn wants_name(registries: &GameRegistries, line: &LineSpec) -> String {
    match &line.wants {
        Wants::Item(item) => registries
            .items
            .get(item)
            .map_or_else(|| item.to_string(), |def| def.name.clone()),
        Wants::Class(class) => registries
            .resources
            .classes()
            .iter()
            .find(|def| def.id == *class)
            .map_or_else(|| class.to_string(), |def| def.name.clone()),
    }
}

/// What a line grades on, by name.
fn grade_name(registries: &GameRegistries, line: &LineSpec) -> String {
    match &line.grade {
        GradeOn::Attribute(attribute) => attribute.name().to_owned(),
        GradeOn::Line(id) => match &line.wants {
            Wants::Item(item) => registries
                .items
                .get(item)
                .and_then(|def| def.lines.iter().find(|own| own.id == *id))
                .map_or_else(|| id.clone(), |own| own.name.clone()),
            Wants::Class(_) => id.clone(),
        },
    }
}

/// A made item's batch as the ledger numbers it, a lot's origin by name,
/// and a bought item's as store grade.
fn batch_words(sold: &procurement::Sale) -> String {
    if sold.batch > 0 {
        fill(
            words("strata.board.sold.batch"),
            "batch",
            &format!("{:04}", sold.batch),
        )
    } else if let Some(origin) = &sold.origin {
        origin.root.clone()
    } else {
        words("strata.board.sold.store").to_owned()
    }
}

/// `units` of a stack, as a line names them.
fn amount_words(registries: &GameRegistries, item: &Namespaced, count: u32, name: &str) -> String {
    let lot = matches!(
        registries.items.get(item).map(|def| &def.kind),
        Some(ItemKind::Lot { .. })
    );
    if lot {
        fill(
            &fill(words("strata.board.units.of"), "n", &grouped(count)),
            "name",
            name,
        )
    } else {
        format!("{} {}", grouped(count), counted(name, count))
    }
}

/// The household's side of one line: the best offer in words, or why
/// there is none, with the request that sells it.
struct Side {
    yours: String,
    short: bool,
    label: String,
    request: Option<ProcurementRequest>,
    why: Option<String>,
}

/// A line with no offer: a filled one says so, then what is held short of
/// the minimum, then that nothing is.
fn none_to_sell(floor: &Floor<'_>, line: &LineSpec, left: u32) -> Side {
    let short = procurement::best_short(floor.registries, line, floor.pack, Some(floor.store));
    let (yours, why) = match short {
        Some(has) => (
            fill(
                &fill(words("strata.board.short"), "has", &has.to_string()),
                "needs",
                &line.min.to_string(),
            ),
            fill(words("strata.board.why.short"), "has", &has.to_string()),
        ),
        None => (
            words("strata.board.none").to_owned(),
            words("strata.board.none").to_owned(),
        ),
    };
    Side {
        yours,
        short: short.is_some(),
        label: words("strata.board.sell").to_owned(),
        request: None,
        why: Some(if left == 0 {
            words("strata.board.filled").to_owned()
        } else {
            why
        }),
    }
}

fn side_of(floor: &Floor<'_>, terms: Terms, buyer: &BuyerDef, line: &LineSpec, left: u32) -> Side {
    let registries = floor.registries;
    if terms.refuses {
        return Side {
            yours: String::new(),
            short: false,
            label: words("strata.board.sell").to_owned(),
            request: None,
            why: Some(refusal_words(&SaleRefusal::Hostile)),
        };
    }
    let offers = procurement::offers(registries, buyer, line, floor.pack, Some(floor.store));
    let Some(best) = offers.first() else {
        return none_to_sell(floor, line, left);
    };
    let each = procurement::pay_for(Terms::scale(best.unit, terms.pays_pm), 1);
    let mut yours = if matches!(line.wants, Wants::Class(_)) {
        fill(
            &fill(
                &fill(
                    words("strata.board.yours.lot"),
                    "name",
                    &stock_name(registries, &best.kind),
                ),
                "grade",
                &best.grade.to_string(),
            ),
            "units",
            &grouped(best.held),
        )
    } else {
        fill(
            &fill(
                &fill(words("strata.board.yours"), "held", &grouped(best.held)),
                "grade",
                &best.grade.to_string(),
            ),
            "each",
            &each.to_string(),
        )
    };
    if matches!(line.wants, Wants::Class(_)) {
        yours = fill(
            &format!("{yours} \u{b7} {}", words("strata.board.pays")),
            "each",
            &each.to_string(),
        );
    }
    if offers.len() > 1 {
        yours = format!(
            "{yours} \u{b7} {}",
            fill(
                words("strata.board.more"),
                "n",
                &(offers.len() - 1).to_string()
            )
        );
    }
    let quote = procurement::check_sale_on(
        registries,
        floor.at_board,
        terms,
        &buyer.id,
        &line.id,
        floor.state,
        floor.pack,
        Some(floor.store),
        &best.kind,
        left,
    );
    match quote {
        Ok(quote) => Side {
            yours,
            short: false,
            label: fill(
                &fill(words("strata.board.sell.n"), "n", &grouped(quote.count)),
                "paid",
                &grouped(quote.paid),
            ),
            request: Some(ProcurementRequest::Sell {
                buyer: buyer.id.clone(),
                line: line.id.clone(),
                kind: best.kind.clone(),
                count: left,
            }),
            why: None,
        },
        Err(refusal) => Side {
            yours,
            short: false,
            label: words("strata.board.sell").to_owned(),
            request: None,
            why: Some(refusal_words(&refusal)),
        },
    }
}

/// The image a line's wanted thing is drawn by: its item's, or for a class
/// the first lot item of it, so the board shows what the pack would.
fn line_swatch(registries: &GameRegistries, line: &LineSpec) -> Option<Swatch> {
    match &line.wants {
        Wants::Item(item) => swatch_of_item(registries, item),
        Wants::Class(class) => registries
            .items
            .iter()
            .find(|def| matches!(&def.kind, ItemKind::Lot { class: own, .. } if own == class))
            .and_then(|def| swatch_of_item(registries, &def.id)),
    }
}

fn line_view(floor: &Floor<'_>, terms: Terms, buyer: &BuyerDef, line: &LineSpec) -> LineView {
    let registries = floor.registries;
    let left = procurement::wanted(buyer, line, floor.state);
    let class = matches!(line.wants, Wants::Class(_));
    let asks = if left == 0 {
        String::new()
    } else if class {
        fill(words("strata.board.wants.units"), "n", &grouped(left))
    } else {
        fill(words("strata.board.wants.more"), "n", &grouped(left))
    };
    let mut state = Vec::new();
    if left == 0 {
        state.push(words("strata.board.filled"));
    }
    if line.refill.is_some() {
        state.push(words("strata.board.refills"));
    }
    let side = side_of(floor, terms, buyer, line, left);
    LineView {
        id: line.id.clone(),
        swatch: line_swatch(registries, line),
        wants: wants_name(registries, line),
        asks,
        min: fill(
            &fill(
                words("strata.board.min"),
                "grade",
                &grade_name(registries, line),
            ),
            "min",
            &line.min.to_string(),
        ),
        price: fill(
            words(if class {
                "strata.board.price.unit"
            } else {
                "strata.board.price.each"
            }),
            "n",
            &grouped(Terms::scale(line.price, terms.pays_pm)),
        ),
        share: procurement::taken_pm(buyer, line, floor.state),
        state: state.join(" \u{b7} "),
        yours: side.yours,
        short: side.short,
        label: side.label,
        request: side.request,
        why: side.why,
    }
}

/// The lease row of `faction`'s board: what it costs at the board's terms and
/// for how long, the time left while one runs, and the sim's own answer to
/// whether it can be bought.
fn lease_view(
    floor: &Floor<'_>,
    books: &Books<'_>,
    terms: Terms,
    faction: &veyra_sim::factions::Faction,
) -> Option<LeaseView> {
    lease_buyer(floor.registries, faction)?;
    let claims = floor.registries.factions.claims()?;
    let check = check_lease(
        floor.registries,
        floor.at_board,
        terms,
        &floor.purse,
        faction,
    );
    let price = Terms::scale(claims.lease.price, terms.sells_pm);
    let hours = format!("{}", claims.lease.hours);
    let left_s = books.leases.left_s(faction.id, books.now);
    let left = if left_s > 0.0 {
        #[allow(
            clippy::cast_possible_truncation,
            clippy::cast_sign_loss,
            reason = "whole minutes of a lease that runs for hours"
        )]
        let minutes = (left_s / 60.0).ceil() as u64;
        fill(
            words("strata.board.lease.left"),
            "left",
            &fill(
                &fill(
                    words("strata.board.lease.time"),
                    "h",
                    &(minutes / 60).to_string(),
                ),
                "m",
                &(minutes % 60).to_string(),
            ),
        )
    } else {
        String::new()
    };
    let name = super::claims::possessive(&faction.name);
    Some(LeaseView {
        head: fill(words("strata.board.lease"), "faction", &name),
        price: fill(
            &fill(words("strata.board.lease.price"), "n", &grouped(price)),
            "hours",
            &hours,
        ),
        left,
        label: words(if left_s > 0.0 {
            "strata.board.lease.renew"
        } else {
            "strata.board.lease.button"
        })
        .to_owned(),
        request: check.is_ok().then_some(ProcurementRequest::Lease {
            faction: faction.id,
        }),
        why: check.err().map(|refusal| lease_words(&refusal)),
    })
}

impl WorkView {
    /// The `test_id` of the row's button.
    #[must_use]
    pub fn button_id(&self) -> String {
        match self.state {
            WorkState::Done => format!("{COLLECT_TAG}{}", self.key),
            _ => format!("{TAKE_TAG}{}", self.key),
        }
    }
}

impl BoardView {
    /// The view of `buyer`'s board for the world as it is; `None` when the
    /// data has no such buyer.
    #[must_use]
    pub fn of(floor: &Floor<'_>, buyer: &Namespaced, last: Option<&str>) -> Option<Self> {
        let def = floor.registries.buyers.get(buyer)?;
        let (faction, terms) = floor
            .books
            .map_or((None, Terms::NEUTRAL), |books| books.board(buyer));
        let stance = floor
            .books
            .zip(faction.as_ref())
            .and_then(|(books, faction)| books.stance_words(faction))
            .unwrap_or_default();
        let lease = floor
            .books
            .zip(faction.as_ref())
            .and_then(|(books, faction)| lease_view(floor, &books, terms, faction));
        let work = floor
            .work
            .as_ref()
            .map(|work| work_views(work, buyer, floor.at_board));
        let orders: Vec<OrderView> = def
            .orders
            .iter()
            .map(|order| {
                let name = order.name.to_uppercase();
                OrderView {
                    head: match order.kind {
                        OrderKind::Project => fill(words("strata.board.project"), "name", &name),
                        OrderKind::Upkeep => name,
                    },
                    lines: order
                        .lines
                        .iter()
                        .map(|line| line_view(floor, terms, def, line))
                        .collect(),
                }
            })
            .collect();
        let sold = floor
            .state
            .sales
            .iter()
            .rev()
            .filter(|sale| sale.buyer == *buyer)
            .take(SOLD_SHOWN)
            .map(|sale| {
                let name = floor
                    .registries
                    .items
                    .get(&sale.item)
                    .map_or_else(|| sale.item.to_string(), |def| def.name.clone());
                let said = format!(
                    "{} \u{b7} {} \u{b7} {}",
                    amount_words(floor.registries, &sale.item, sale.count, &name),
                    batch_words(sale),
                    fill(words("strata.board.cr"), "n", &grouped(sale.paid)),
                );
                SoldView { said }
            })
            .collect();
        let mut view = Self {
            title: fill(words("strata.board.title"), "buyer", &def.name),
            purse: floor.purse.0,
            stance,
            lease,
            work,
            orders,
            sells: sale_views(floor, def, terms),
            sold,
            focus: None,
        };
        view.focus = view.focus_for(last);
        Some(view)
    }

    /// Every line, in order.
    pub fn lines(&self) -> impl Iterator<Item = &LineView> {
        self.orders.iter().flat_map(|order| order.lines.iter())
    }

    /// The line of the Sell button `id`.
    #[must_use]
    pub fn line_of(&self, id: &str) -> Option<&LineView> {
        let line = id.strip_prefix(SELL_TAG)?;
        self.lines().find(|each| each.id == line)
    }

    /// The FOR SALE row of the Buy button `id`.
    #[must_use]
    pub fn sale_of(&self, id: &str) -> Option<&SaleView> {
        let item = id.strip_prefix(BUY_TAG)?;
        self.sells.iter().find(|sale| sale.item.to_string() == item)
    }

    /// The request the button `id` sends, of any kind.
    fn request_of(&self, id: &str) -> Option<ProcurementRequest> {
        if id == LEASE_ID {
            return self.lease.as_ref().and_then(|lease| lease.request.clone());
        }
        if let Some(key) = id
            .strip_prefix(TAKE_TAG)
            .or_else(|| id.strip_prefix(COLLECT_TAG))
        {
            return self
                .work
                .iter()
                .flatten()
                .find(|row| row.key.to_string() == key)
                .and_then(|row| row.request.clone());
        }
        self.line_of(id)
            .and_then(|line| line.request.clone())
            .or_else(|| self.sale_of(id).and_then(|sale| sale.request.clone()))
    }

    /// Where the focus starts: on the button last used while it can still
    /// act, else on the first that can, else on Close.
    fn focus_for(&self, last: Option<&str>) -> Option<String> {
        last.filter(|id| self.request_of(id).is_some())
            .map(str::to_owned)
            .or_else(|| {
                self.lines()
                    .find(|line| line.request.is_some())
                    .map(|line| sell_id(&line.id))
            })
            .or_else(|| {
                self.sells
                    .iter()
                    .find(|sale| sale.request.is_some())
                    .map(|sale| buy_id(&sale.item))
            })
            .or_else(|| {
                self.lease
                    .as_ref()
                    .filter(|lease| lease.request.is_some())
                    .map(|_| LEASE_ID.to_owned())
            })
            .or_else(|| {
                self.work
                    .iter()
                    .flatten()
                    .find(|row| row.request.is_some())
                    .map(WorkView::button_id)
            })
            .or_else(|| Some(CLOSE_ID.to_owned()))
    }
}

// ---- the definition ----

fn bar(share: u16) -> UiNodeDef {
    panel(
        PROGRESS_ROLE,
        Layout {
            width: Some(Length::Px(BAR_WIDTH)),
            height: Some(Length::Px(8.)),
            ..row(0.)
        },
        vec![panel(
            PROGRESS_FILL_ROLE,
            Layout {
                width: Some(Length::Percent(card::track_percent(share))),
                height: Some(Length::Px(8.)),
                ..row(0.)
            },
            Vec::new(),
        )],
    )
}

fn line_row(line: &LineView) -> UiNodeDef {
    let id = |tag: &str| format!("{tag}{}", line.id);
    let asked = vec![
        text_in(line.wants.clone(), TextRole::Body, NAME_ROLE),
        with_id(line_in(line.asks.clone(), FIGURE_ROLE), &id(WANTS_TAG)),
        line_in(line.min.clone(), STAT_ROLE),
        line_in(line.price.clone(), STAT_VALUE_ROLE),
    ];
    let under = vec![with_id(
        line_in(
            line.yours.clone(),
            if line.short { PAUSED_ROLE } else { SUB_ROLE },
        ),
        &id(YOURS_TAG),
    )];
    let mut button = vec![act(
        &line.label,
        &id(SELL_TAG),
        true,
        line.request.is_some(),
    )];
    if let Some(why) = &line.why {
        button.push(with_id(line_in(why.clone(), WHY_ROLE), &id(WHY_TAG)));
    }
    panel(
        BOX_ROLE,
        Layout {
            width: Some(Length::Percent(100.)),
            padding: Padding::all(1.4),
            align: Some(Align::Center),
            ..row(2.)
        },
        vec![
            well(line.swatch.clone()),
            grows(
                0.7,
                vec![
                    panel("invisible", row(1.4), asked),
                    panel(
                        "invisible",
                        row(1.),
                        vec![
                            bar(line.share),
                            with_id(line_in(line.state.clone(), SUB_ROLE), &id(STATE_TAG)),
                        ],
                    ),
                    panel("invisible", row(1.), under),
                ],
            ),
            panel(
                "invisible",
                Layout {
                    align: Some(Align::End),
                    ..column(0.5)
                },
                button,
            ),
        ],
    )
}

fn sale_row(sale: &SaleView) -> UiNodeDef {
    let id = |tag: &str| format!("{tag}{}", sale.item);
    let mut button = vec![act(&sale.label, &id(BUY_TAG), true, sale.request.is_some())];
    if let Some(why) = &sale.why {
        button.push(with_id(line_in(why.clone(), WHY_ROLE), &id(BUY_WHY_TAG)));
    }
    // One line a row, so the rows under the orders fit a short window.
    panel(
        BOX_ROLE,
        Layout {
            width: Some(Length::Percent(100.)),
            padding: Padding::all(0.3),
            align: Some(Align::Center),
            ..row(2.)
        },
        vec![
            well(sale.swatch.clone()),
            grows(
                0.7,
                vec![text_in(sale.name.clone(), TextRole::Body, NAME_ROLE)],
            ),
            with_id(line_in(sale.price.clone(), STAT_VALUE_ROLE), &id(PRICE_TAG)),
            with_id(line_in(sale.purse.clone(), SUB_ROLE), &id(HAVE_TAG)),
            panel(
                "invisible",
                Layout {
                    align: Some(Align::End),
                    ..column(0.3)
                },
                button,
            ),
        ],
    )
}

fn lease_row(lease: &LeaseView) -> UiNodeDef {
    let mut button = vec![act(&lease.label, LEASE_ID, true, lease.request.is_some())];
    if let Some(why) = &lease.why {
        button.push(with_id(line_in(why.clone(), WHY_ROLE), LEASE_WHY_ID));
    }
    let mut words = vec![with_id(
        line_in(lease.price.clone(), STAT_VALUE_ROLE),
        LEASE_PRICE_ID,
    )];
    if !lease.left.is_empty() {
        words.push(with_id(
            line_in(lease.left.clone(), SUB_ROLE),
            LEASE_LEFT_ID,
        ));
    }
    panel(
        BOX_ROLE,
        Layout {
            width: Some(Length::Percent(100.)),
            padding: Padding::all(0.3),
            align: Some(Align::Center),
            ..row(2.)
        },
        vec![
            grows(0.7, words),
            panel(
                "invisible",
                Layout {
                    align: Some(Align::End),
                    ..column(0.3)
                },
                button,
            ),
        ],
    )
}

fn work_row(row_view: &WorkView) -> UiNodeDef {
    let key = row_view.key;
    let mut right = Vec::new();
    if row_view.state == WorkState::Taken {
        right.push(with_id(
            line_in(row_view.said.clone(), SUB_ROLE),
            &format!("{WORK_STATE_TAG}{key}"),
        ));
    } else {
        right.push(act(
            &row_view.label,
            &row_view.button_id(),
            true,
            row_view.request.is_some(),
        ));
        if let Some(why) = &row_view.why {
            right.push(with_id(
                line_in(why.clone(), WHY_ROLE),
                &format!("{WORK_WHY_TAG}{key}"),
            ));
        }
    }
    let mut under = vec![
        line_in(row_view.lies.clone(), STAT_ROLE),
        line_in(row_view.who.clone(), SUB_ROLE),
    ];
    if row_view.state == WorkState::Done {
        under.push(with_id(
            line_in(row_view.said.clone(), FIGURE_ROLE),
            &format!("{WORK_STATE_TAG}{key}"),
        ));
    }
    panel(
        BOX_ROLE,
        Layout {
            width: Some(Length::Percent(100.)),
            padding: Padding::all(0.6),
            align: Some(Align::Center),
            ..row(2.)
        },
        vec![
            grows(
                0.7,
                vec![
                    text_in(row_view.head.clone(), TextRole::Body, NAME_ROLE),
                    panel("invisible", row(1.4), under),
                ],
            ),
            line_in(row_view.pay.clone(), STAT_VALUE_ROLE),
            panel(
                "invisible",
                Layout {
                    align: Some(Align::End),
                    ..column(0.3)
                },
                right,
            ),
        ],
    )
}

fn sold_here(view: &BoardView) -> UiNodeDef {
    let mut rows = vec![line_in(words("strata.board.sold"), HEAD_ROLE)];
    if view.sold.is_empty() {
        rows.push(with_id(
            line_in(words("strata.board.sold.none"), NOTE_ROLE),
            NOTHING_ID,
        ));
    }
    rows.extend(view.sold.iter().enumerate().map(|(at, sold)| {
        with_id(
            line_in(sold.said.clone(), STAT_VALUE_ROLE),
            &format!("{SOLD_TAG}{at}"),
        )
    }));
    panel("invisible", column(0.6), rows)
}

/// The whole screen for `view`.
#[must_use]
#[allow(clippy::too_many_lines, reason = "one screen laid out top to bottom")]
pub fn screen_def(view: &BoardView) -> ScreenDef {
    let mut body = Vec::new();
    // The work first: it is what a board is visited for and the rows under
    // it scroll on a short window.
    if let Some(work) = &view.work {
        body.push(line_in(words("strata.board.work"), HEAD_ROLE));
        if work.is_empty() {
            body.push(with_id(
                line_in(words("strata.board.work.none"), NOTE_ROLE),
                WORK_NONE_ID,
            ));
        }
        body.extend(work.iter().map(work_row));
    }
    // What the buyer sells next, above its orders: a household sent to the
    // board for its bike (ticket 0014) should not have to scroll past every
    // line it could sell to find it.
    if !view.sells.is_empty() {
        body.push(line_in(words("strata.board.sale"), HEAD_ROLE));
        body.extend(view.sells.iter().map(sale_row));
    }
    for order in &view.orders {
        body.push(line_in(order.head.clone(), HEAD_ROLE));
        body.extend(order.lines.iter().map(line_row));
    }
    if let Some(lease) = &view.lease {
        body.push(line_in(lease.head.clone(), HEAD_ROLE));
        body.push(lease_row(lease));
    }
    body.push(sold_here(view));
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
                ..column(1.6)
            },
            children: vec![
                panel(
                    "invisible",
                    row(2.),
                    vec![
                        with_id(
                            UiNodeDef::Text {
                                key: super::crafting::key(view.title.clone()),
                                style: TextRole::Title,
                                opts: slotted::ui::def::TextOpts {
                                    wrap: false,
                                    role: Some(Role::new("panel.title")),
                                    ..slotted::ui::def::TextOpts::default()
                                },
                                tags: Tags::new(),
                            },
                            TITLE_ID,
                        ),
                        spacer(),
                        super::stores::purse(PURSE_ID, view.purse),
                    ],
                ),
                // How the board's faction is towards the household, in words.
                with_id(line_in(view.stance.clone(), SUB_ROLE), STANCE_ID),
                // The rows scroll inside the panel on a window too short
                // for them, so Close stays inside the border.
                // A scroll panel, not a clipped one: the wheel and the bar move it.
                UiNodeDef::Scroll {
                    layout: Layout {
                        align: Some(Align::Stretch),
                        grow: 1.,
                        min_height: Some(Length::Px(0.)),
                        ..column(1.2)
                    },
                    scrollbar: true,
                    children: body,
                    tags: Tags::new(),
                },
                panel(
                    "invisible",
                    row(2.),
                    vec![
                        spacer(),
                        UiNodeDef::Custom {
                            kind: WidgetKind::new("slotted:hint_bar"),
                            params: slotted::registry::Value::Unit,
                            children: Vec::new(),
                            tags: Tags::new().with(Tags::TEST_ID, "board.hints"),
                        },
                        UiNodeDef::Button {
                            widget: Some(WidgetKind::new("slotted:close")),
                            opts: ButtonOpts {
                                label: Some(super::crafting::key("strata.menu.close")),
                                ..ButtonOpts::default()
                            },
                            tags: Tags::new().with(Tags::TEST_ID, CLOSE_ID),
                        },
                    ],
                ),
            ],
            tags: Tags::new().with(Tags::TEST_ID, ROOT_ID),
        },
        listring: Vec::new(),
        remove: Vec::new(),
        initial_focus: view.focus.clone(),
        presentation: Presentation {
            mode: PresentationMode::Page,
            scrim: Some(true),
            transition: Transition::Fade,
            wildcards: Some(false),
            ..Presentation::default()
        },
    }
}

// ---- open, answer, rebuild ----

/// What the board reads: the data, the sim's own reach rule, the pack and
/// the store it sells from, the purse and the ledger, the books that set
/// the price, and what the WORK rows are made from (the jobs held, the
/// camps cleared, the postings candidates, the seed) with the player's
/// place. The place counts by value, the clock by whole second.
#[derive(SystemParam)]
pub struct BoardInputs<'w, 's> {
    registries: Option<Res<'w, Registries>>,
    reach: Option<Res<'w, Reach>>,
    access: Option<Res<'w, WorkbenchAccess>>,
    players: Query<'w, 's, (Ref<'static, Pack>, &'static WorldPosition), With<LocalPlayer>>,
    store: Option<Res<'w, HouseholdStore>>,
    purse: Option<Res<'w, Purse>>,
    ledger: Option<Res<'w, ProcurementState>>,
    books: Standing<'w>,
    candidates: Option<Res<'w, PostingCandidates>>,
    cleared: Option<Res<'w, Cleared>>,
    jobs: Option<Res<'w, Jobs>>,
    seed: Option<Res<'w, WorldSeed>>,
    clock: Local<'s, Seen<u64>>,
    at: Local<'s, Seen<[u64; 2]>>,
}

impl ChangeSource for BoardInputs<'static, 'static> {
    fn changed(inputs: &mut BoardInputs<'_, '_>) -> bool {
        let place = inputs
            .players
            .single()
            .ok()
            .map(|(pack, at)| (pack.is_changed(), [at.0.x().to_bits(), at.0.z().to_bits()]));
        let stepped = place.is_some_and(|(_, at)| inputs.at.differs(at));
        let books = inputs.books.moved(&mut inputs.clock);
        books
            || stepped
            || place.is_some_and(|(pack, _)| pack)
            || moved(&inputs.registries)
            || moved(&inputs.reach)
            || moved(&inputs.access)
            || moved(&inputs.store)
            || moved(&inputs.purse)
            || moved(&inputs.ledger)
            || moved(&inputs.candidates)
            || moved(&inputs.cleared)
            || moved(&inputs.jobs)
            || moved(&inputs.seed)
    }
}

impl BoardInputs<'_, '_> {
    /// What the WORK rows are read from, where the data has postings and the
    /// world has its camps and its factions.
    fn work_floor<'a>(
        &'a self,
        registries: &'a GameRegistries,
        buyer: &Namespaced,
        at: [f64; 2],
    ) -> Option<Work<'a>> {
        let rules = registries.postings.as_ref()?;
        let seat =
            veyra_sim::deeds::anchor_faction(&self.books.factions()?.0, registries, buyer)?.seat;
        Some(Work {
            rules,
            candidates: &self.candidates.as_deref()?.0,
            cleared: self.cleared.as_deref()?,
            jobs: self.jobs.as_deref()?,
            now: self.books.now(),
            seed: self.seed.as_deref()?.0,
            seat,
            at,
        })
    }
}

impl SimScreen for BoardUi {
    type View = BoardView;
    type Inputs = BoardInputs<'static, 'static>;
    type Answering = ResMut<'static, ProcurementInbox>;
    const KIND: &'static str = KIND;

    fn view(inputs: &BoardInputs<'_, '_>, state: &Self) -> Option<BoardView> {
        let registries = &inputs.registries.as_deref()?.0;
        let buyer = state.buyer.clone()?;
        // The sim's own rule, so the buttons are grey exactly when a request
        // would be refused for it.
        let at_board = inputs.reach.as_deref().is_some_and(|reach| {
            let access = inputs.access.as_deref().copied();
            procurement::at_board(access.unwrap_or_default(), reach, &buyer)
        });
        let (pack, position) = inputs.players.single().ok()?;
        let (store, ledger) = (HouseholdStore::default(), ProcurementState::default());
        let work = inputs.work_floor(registries, &buyer, [position.0.x(), position.0.z()]);
        let floor = Floor {
            registries,
            pack: &pack,
            store: inputs.store.as_deref().unwrap_or(&store),
            purse: inputs.purse.as_deref().copied().unwrap_or_default(),
            state: inputs.ledger.as_deref().unwrap_or(&ledger),
            at_board,
            books: inputs.books.books(),
            work,
        };
        BoardView::of(&floor, &buyer, state.last.as_deref())
    }

    fn def(view: &BoardView, _: &Self) -> ScreenDef {
        screen_def(view)
    }

    fn shown(state: &Self) -> Option<&BoardView> {
        state.view.as_ref()
    }

    fn marked(state: &Self) -> bool {
        state.dirty
    }

    fn built(state: &mut Self, view: BoardView) {
        state.dirty = false;
        state.view = Some(view);
    }

    /// A Sell button. The request the view worked out goes to the sim's
    /// inbox, one at a time: Accept pressed twice before the tick is one
    /// sale.
    fn answer(ui: &mut Self, choice: &MenuChoice, inbox: &mut ResMut<ProcurementInbox>) {
        let request = ui
            .view
            .as_ref()
            .and_then(|view| view.request_of(&choice.id));
        let Some(request) = request else {
            return;
        };
        if ui.asked == 0 {
            inbox.push(request);
            ui.asked += 1;
        }
        ui.last = Some(choice.id.clone());
        ui.dirty = true;
    }
}

/// Opens the board of `buyer`, as a board's F does.
pub fn open(commands: &mut Commands, buyer: Namespaced) {
    commands.queue(move |world: &mut World| {
        {
            let mut ui = world.resource_mut::<BoardUi>();
            ui.buyer = Some(buyer);
            ui.last = None;
            ui.asked = 0;
        }
        let Some(def) = sim_screen::build::<BoardUi>(world) else {
            tracing::warn!("no local player or no such buyer: nothing to read the board with");
            return;
        };
        sim_screen::show::<BoardUi>(world, def);
    });
}

/// `Update`, always: the sim's answer as a toast wherever the player is.
pub fn hear(
    mut answers: MessageReader<SaleAnswer>,
    registries: Option<Res<Registries>>,
    mut ui: ResMut<BoardUi>,
    mut commands: Commands,
) {
    for answer in answers.read() {
        ui.asked = ui.asked.saturating_sub(1);
        ui.dirty = true;
        let said = match &answer.result {
            Ok(sold) => {
                let items = registries.as_ref().map(|registries| &registries.0);
                let name = items
                    .and_then(|registries| registries.items.get(&sold.item))
                    .map_or_else(|| sold.item.to_string(), |def| def.name.clone());
                let buyer = items
                    .and_then(|registries| registries.buyers.get(&sold.buyer))
                    .map_or_else(|| sold.buyer.to_string(), |def| def.name.clone());
                let lot = items.is_some_and(|registries| {
                    matches!(
                        registries.items.get(&sold.item).map(|def| &def.kind),
                        Some(ItemKind::Lot { .. })
                    )
                });
                let what = if lot {
                    fill(
                        &fill(words("strata.board.units.of"), "n", &grouped(sold.count)),
                        "name",
                        &name,
                    )
                } else {
                    format!("{} {}", grouped(sold.count), counted(&name, sold.count))
                };
                fill(
                    &fill(
                        &fill(words("strata.board.toast.sold"), "what", &what),
                        "buyer",
                        &buyer,
                    ),
                    "paid",
                    &grouped(sold.paid),
                )
            }
            Err(refusal) => refusal_words(refusal),
        };
        tracing::info!("board: {said}");
        super::progress::notice(&mut commands, said);
    }
}

/// `Update`, always: the sim's answer to a purchase as a toast.
pub fn hear_purchases(
    mut answers: MessageReader<PurchaseAnswer>,
    registries: Option<Res<Registries>>,
    mut ui: ResMut<BoardUi>,
    mut commands: Commands,
) {
    for answer in answers.read() {
        ui.asked = ui.asked.saturating_sub(1);
        ui.dirty = true;
        let said = match &answer.result {
            Ok(bought) => {
                let registries = registries.as_ref().map(|registries| &registries.0);
                let name = registries
                    .and_then(|registries| registries.items.get(&bought.item))
                    .map_or_else(|| bought.item.to_string(), |def| def.name.clone());
                let buyer = registries
                    .and_then(|registries| registries.buyers.get(&bought.buyer))
                    .map_or_else(|| bought.buyer.to_string(), |def| def.name.clone());
                fill(
                    &fill(
                        &fill(words("strata.board.toast.bought"), "what", &name),
                        "buyer",
                        &buyer,
                    ),
                    "paid",
                    &grouped(bought.paid),
                )
            }
            Err(refusal) => purchase_words(refusal),
        };
        tracing::info!("board: {said}");
        super::progress::notice(&mut commands, said);
    }
}

/// `Update`, always: the sim's answer to taking or collecting work as a toast.
pub fn hear_jobs(
    mut answers: MessageReader<JobAnswer>,
    registries: Option<Res<Registries>>,
    mut ui: ResMut<BoardUi>,
    mut commands: Commands,
) {
    for answer in answers.read() {
        ui.asked = ui.asked.saturating_sub(1);
        ui.dirty = true;
        let said = match &answer.result {
            Ok(JobOutcome::Taken(posting)) => fill(
                super::strings::english(&format!("strata.board.toast.work.taken.{}", posting.kind))
                    .unwrap_or_else(|| words("strata.board.toast.work.taken")),
                "camp",
                &posting.name,
            ),
            Ok(JobOutcome::Collected { name, pay }) => {
                let buyer = ui
                    .buyer
                    .as_ref()
                    .zip(registries.as_ref())
                    .and_then(|(id, registries)| registries.0.buyers.get(id))
                    .map_or_else(String::new, |def| def.name.clone());
                fill(
                    &fill(words("strata.board.toast.work.paid"), "buyer", &buyer),
                    "paid",
                    &grouped(*pay),
                )
                .replace("{camp}", name)
            }
            Err(refusal) => job_words(*refusal),
        };
        tracing::info!("board: {said}");
        super::progress::notice(&mut commands, said);
    }
}

/// `Update`, always: the sim's answer to a lease as a toast.
pub fn hear_leases(
    mut answers: MessageReader<LeaseAnswer>,
    factions: Option<Res<veyra_sim::factions::Factions>>,
    mut ui: ResMut<BoardUi>,
    mut commands: Commands,
) {
    for answer in answers.read() {
        ui.asked = ui.asked.saturating_sub(1);
        ui.dirty = true;
        let said = match &answer.result {
            Ok(leased) => {
                let name = factions
                    .as_ref()
                    .and_then(|factions| factions.0.faction(leased.faction))
                    .map_or_else(String::new, |faction| {
                        super::claims::possessive(&faction.name)
                    });
                fill(
                    &fill(words("strata.board.toast.leased"), "faction", &name),
                    "paid",
                    &grouped(leased.paid),
                )
            }
            Err(refusal) => lease_words(refusal),
        };
        tracing::info!("board: {said}");
        super::progress::notice(&mut commands, said);
    }
}

/// Registers the state and the systems. Only called once the UI is wired.
pub(crate) fn register(app: &mut bevy::app::App) {
    // The sim's answers mark the screen before the comparison.
    let heard = |step: bevy::ecs::schedule::ScheduleConfigs<_>| {
        step.after(ScreenStep::answer::<BoardUi>())
            .before(ScreenStep::rebuild::<BoardUi>())
    };
    app.add_sim_screen::<BoardUi>().add_systems(
        bevy::app::Update,
        (
            heard(hear.into_configs()),
            heard(hear_purchases.into_configs()),
            heard(hear_leases.into_configs()),
            heard(hear_jobs.into_configs()),
        ),
    );
}

#[cfg(test)]
mod tests {
    #![allow(
        clippy::unwrap_used,
        reason = "a test that unwraps a None fails, which is the point"
    )]

    use veyra_sim::postings::Job;

    use super::*;

    const BUYER: &str = "veyra:buyer.pelagic_works";

    fn rules() -> PostingRules {
        PostingRules::parse(include_str!("../../../assets/procurement/postings.ron")).unwrap()
    }

    fn camps() -> Vec<Candidate> {
        (1..=5u32)
            .map(|key| Candidate {
                key: u64::from(key),
                name: format!("Camp {key}"),
                centre: [-400.0 - 2_000.0 * f64::from(key), -500.0],
                tier: 1,
                band: "the Skan Gang".to_owned(),
                kind: "strata:wild_camp".to_owned(),
                returns_h: 1.0,
            })
            .collect()
    }

    fn rows(jobs: &Jobs, cleared: &Cleared, at_board: bool) -> Vec<WorkView> {
        let rules = rules();
        let camps = camps();
        let work = Work {
            rules: &rules,
            candidates: &camps,
            cleared,
            jobs,
            now: 0.0,
            seed: Seed(42),
            seat: [-400.0, -500.0],
            at: [-400.0, -500.0],
        };
        work_views(&work, &BUYER.parse().unwrap(), at_board)
    }

    #[test]
    fn offered_work_has_a_take_that_sends_the_request() {
        let shown = rows(&Jobs::default(), &Cleared::default(), true);
        assert_eq!(shown.len(), 3, "the data's open count");
        for row in &shown {
            assert_eq!(row.state, WorkState::Offered);
            assert!(row.head.starts_with("Clear Camp"), "{}", row.head);
            assert!(row.pay.ends_with(" cr"), "{}", row.pay);
            assert!(row.lies.contains("km"), "{}", row.lies);
            assert!(matches!(
                row.request,
                Some(ProcurementRequest::TakeJob { key, .. }) if key == row.key
            ));
            assert!(row.why.is_none());
        }
        let away = rows(&Jobs::default(), &Cleared::default(), false);
        assert!(away.iter().all(|row| row.request.is_none()));
        assert_eq!(
            away[0].why.as_deref(),
            Some("Out of reach of the board"),
            "grey with the sim's reason"
        );
    }

    #[test]
    fn a_taken_job_says_taken_and_a_done_one_offers_collect() {
        let first = rows(&Jobs::default(), &Cleared::default(), true)[0].clone();
        let posting = veyra_sim::postings::Posting {
            key: first.key,
            name: first.head.trim_start_matches("Clear ").to_owned(),
            tier: 1,
            pay: 400,
            faction: "the Skan Gang".to_owned(),
            kind: "strata:wild_camp".to_owned(),
            heard: veyra_sim::postings::Heard {
                centre: [-5_000.0, -500.0],
                radius: 900.0,
            },
        };
        let mut jobs = Jobs::default();
        jobs.taken.insert(
            first.key,
            Job {
                posting,
                buyer: BUYER.parse().unwrap(),
                taken_at: 0.0,
                done: false,
            },
        );
        let shown = rows(&jobs, &Cleared::default(), true);
        let taken = shown.iter().find(|row| row.key == first.key).unwrap();
        assert_eq!(taken.state, WorkState::Taken);
        assert!(taken.said.starts_with("Taken"), "{}", taken.said);
        assert!(taken.request.is_none() && taken.why.is_none());
        assert!(
            shown
                .iter()
                .filter(|row| row.state == WorkState::Offered)
                .all(|row| row.key != first.key),
            "a taken camp is not posted again"
        );

        jobs.taken.get_mut(&first.key).unwrap().done = true;
        let shown = rows(&jobs, &Cleared::default(), true);
        let done = shown.iter().find(|row| row.key == first.key).unwrap();
        assert_eq!(done.state, WorkState::Done);
        assert_eq!(done.label, "Collect");
        assert!(matches!(
            done.request,
            Some(ProcurementRequest::Collect { key, .. }) if key == first.key
        ));
        assert!(done.button_id().starts_with(COLLECT_TAG));
    }

    /// A board row says its own verb per kind and never makes a corporate
    /// holder the enemy: a band is named, a wreck's salvage is unclaimed.
    #[test]
    fn a_work_row_has_its_verb_per_kind() {
        assert_eq!(
            work_head("strata:wild_camp", "Wack's Hold"),
            "Clear Wack's Hold"
        );
        assert_eq!(work_head("", "Wack's Hold"), "Clear Wack's Hold");
        assert_eq!(
            work_head("strata:wreck", "Cold Hull"),
            "Recover the cargo at Cold Hull"
        );
        assert_eq!(
            work_head("strata:wreck_works", "Old Works"),
            "Secure Old Works"
        );
        assert_eq!(
            work_who("strata:wild_camp", "Red Hands"),
            "held by Red Hands"
        );
        assert_eq!(work_who("strata:wild_camp", ""), "");
        assert_eq!(work_who("strata:wreck", ""), "unclaimed salvage");
        for (kind, said) in [
            ("strata:wild_camp", "Taken: clear"),
            ("", "Taken: clear"),
            ("strata:wreck", "Taken: recover the cargo at"),
            ("strata:wreck_works", "Taken: secure"),
        ] {
            let toast =
                super::super::strings::english(&format!("strata.board.toast.work.taken.{kind}"))
                    .unwrap_or_else(|| words("strata.board.toast.work.taken"));
            assert!(toast.starts_with(said), "{kind}: {toast}");
        }
    }
}
