//! The command line of the development half: stills, scripts, the
//! benchmark's summary and the look lab's flags. `apps/game` flattens
//! [`DevArgs`] into its own `Cli` behind its `dev-tools` feature, so a
//! shipped or web build parses none of them.

use std::path::PathBuf;

use bevy::prelude::Resource;
use clap::Args;

/// `--crafting-state`: the state of the crafting screen a still is of.
#[derive(clap::ValueEnum, Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum CraftingShot {
    /// The slots with nothing picked.
    Empty,
    /// The slots, their candidates and what comes out.
    #[default]
    Pick,
    /// After Assemble: the bench hand.
    Hand,
    /// After Finish: the finished card.
    Done,
    /// The finished card with Record as blueprint open.
    Record,
}

/// `--crafting-filter`: the chip of the crafting list a still is of.
#[derive(clap::ValueEnum, Debug, Clone, Copy, PartialEq, Eq)]
pub enum CraftingListShot {
    /// What the member was taught.
    Known,
    /// What can be made now.
    Ready,
    /// Everything.
    All,
}

impl CraftingListShot {
    /// The list's filter this stands for.
    pub const fn filter(self) -> veyra_ui::crafting::ListFilter {
        use veyra_ui::crafting::ListFilter;
        match self {
            Self::Known => ListFilter::Known,
            Self::Ready => ListFilter::Ready,
            Self::All => ListFilter::All,
        }
    }
}

impl CraftingShot {
    /// What the screen is asked to bring itself to; `derate` is what a
    /// Record box opens at, percent of the prototype.
    pub const fn still(self, derate: u16) -> veyra_ui::crafting::Still {
        use veyra_ui::crafting::Still;
        match self {
            Self::Empty => Still::Empty,
            Self::Pick => Still::Pick,
            Self::Hand => Still::Hand,
            Self::Done => Still::Done,
            // The sim counts a share in per-mille.
            Self::Record => Still::Record(derate.saturating_mul(10)),
        }
    }
}

/// `--quest-stage`'s value: a quest's id and one of its stages.
pub fn parse_quest_stage(text: &str) -> Result<(String, String), String> {
    text.split_once(':')
        .filter(|(quest, stage)| !quest.is_empty() && !stage.is_empty())
        .map(|(quest, stage)| (quest.to_owned(), stage.to_owned()))
        .ok_or_else(|| "expected `<quest>:<stage>`, as `arrival:pell`".to_owned())
}

/// `--stores-filter`'s value: a filter chip of the stores screen, by the
/// word in its tag.
pub fn parse_filter(text: &str) -> Result<veyra_ui::stores::Filter, String> {
    use veyra_ui::stores::Filter;
    Filter::ALL
        .into_iter()
        .find(|filter| filter.id() == text)
        .ok_or_else(|| {
            let known: Vec<&str> = Filter::ALL.iter().map(|filter| filter.id()).collect();
            format!("expected one of {}", known.join(", "))
        })
}

/// `--press`: one `place@frame`.
pub fn parse_press(text: &str) -> Result<(usize, u32), String> {
    let (place, frame) = text.split_once('@').ok_or("want place@frame, as 0@200")?;
    Ok((
        place.parse().map_err(|error| format!("place: {error}"))?,
        frame.parse().map_err(|error| format!("frame: {error}"))?,
    ))
}

/// Every flag that only a still, a script or a measurement run passes.
#[derive(Args, Resource, Debug, Clone)]
#[allow(
    clippy::struct_excessive_bools,
    reason = "each flag is a switch on the command line, which is what a bool is"
)]
pub struct DevArgs {
    /// Reproduce the combat timing capture with scripted inputs and a 60 Hz frame clock.
    #[arg(long, conflicts_with_all = ["connect", "flycam", "flight", "route", "grounding_study", "study_scene", "auto_walk", "no_combat", "no_hostiles", "hostiles"])]
    pub combat_script: bool,
    /// Record the local combatant's fixed-tick state to CSV for feel analysis.
    #[arg(long, conflicts_with_all = ["connect", "flycam", "flight", "route", "grounding_study", "study_scene", "auto_walk", "no_combat"])]
    pub combat_trace: Option<PathBuf>,
    /// The combat script, then the household's path by script: the
    /// rusher is taken with the blade, the player walks to the blade
    /// trainer, learns One hand 1 if the fight earned it, walks to the
    /// terminal, the recruits join and the pistol marksman comes out.
    /// Implies `--combat-script`; without `--frames` the run exits when
    /// the path is walked.
    #[arg(long, conflicts_with_all = ["connect", "flycam", "flight", "route", "grounding_study", "study_scene", "auto_walk", "no_combat", "no_hostiles", "hostiles"])]
    pub script_progression: bool,
    /// The scripted path with a stop at the workbench after the lesson:
    /// the carbine is drawn and fired, the store's patrol magazine fitted,
    /// the weapon reloaded and fired again, and the magazine taken off;
    /// then a cell core is made from the first lots, the patrol magazine
    /// made from it and fitted, and a second core made from the same lots,
    /// which is recorded as a blueprint; a batch of three is started off
    /// it at the fabricator and waited for at the path's end, all by the
    /// requests a screen would send. Implies `--script-progression`.
    #[arg(long, conflicts_with_all = ["connect", "flycam", "flight", "route", "grounding_study", "study_scene", "auto_walk", "no_combat"])]
    pub script_crafting: bool,
    /// With `--script-crafting`: write what the `Combatant` read at the
    /// workbench (the carbine's row bare, fitted and restored, and the
    /// rounds either side of the fit and the removal) and what was made
    /// there (each finished thing's lines, and the made magazine's rounds
    /// as the bench showed them and as the `Combatant` read them) to this
    /// file, as RON.
    /// With `--script-ride`: write what the ride measured to this file.
    #[arg(long, value_name = "PATH")]
    pub script_report: Option<PathBuf>,
    /// Headless (ticket 0014): walk the first questline to one of its
    /// endings (`keepers`, `assembly` or `holder`): talk to each person and
    /// choose the replies the ending needs, and meet each objective through
    /// the requests the sim answers. What it did goes to the log and to
    /// `--script-report`. It changes the household it is run on, so it is
    /// refused on the default `--household`.
    #[arg(long, value_name = "ENDING", value_parser = ["keepers", "assembly", "holder"], conflicts_with_all = ["connect", "flycam", "flight", "route", "grounding_study", "study_scene", "auto_walk", "no_combat", "script_progression", "script_crafting", "combat_script", "script_ride", "script_survey", "at", "spawn"])]
    pub script_quest: Option<String>,
    /// With `--script-quest`: how Pell's crew is met, `fight`, `pay` or
    /// `talk`.
    #[arg(long, value_name = "BRANCH", value_parser = ["fight", "pay", "talk"], default_value = "pay", requires = "script_quest")]
    pub script_pell: String,
    /// Start the questline's `quest` at `stage` (`arrival:pell`), beside the
    /// place it names, for a still.
    #[arg(long, value_name = "QUEST:STAGE", value_parser = parse_quest_stage)]
    pub quest_stage: Option<(String, String)>,
    /// The household owns a courier bike (ticket 0009, step 2), which V
    /// calls. It changes the household it is run on, so it is refused on
    /// the default `--household`.
    #[arg(long)]
    pub bike: bool,
    /// With `--bike`, or a household that has one: start with the stabled
    /// bike worn to this condition (0 to 1000), for a still. It is refused
    /// on the default `--household`.
    #[arg(long, value_name = "N")]
    pub bike_condition: Option<u16>,
    /// Start with this many credits in the purse, for a still. It changes
    /// the household it is run on, so it is refused on the default
    /// `--household`.
    #[arg(long, value_name = "CREDITS")]
    pub purse: Option<u32>,
    /// With a bike: start with three lots and a part in the saddle bags,
    /// copied from the store, for a still. It is refused on the default
    /// `--household`.
    #[arg(long)]
    pub saddle_stock: bool,
    /// Plant a field harvester where the run starts (`--at deposit`), on the
    /// richest deposit there, for the member who is out and ignoring their
    /// count and the least reading, for a still. It changes the household it is run on, so it is
    /// refused on the default `--household`.
    #[arg(long)]
    pub harvester: bool,
    /// Advance every planted field harvester by this many played hours at
    /// startup, for a still. It changes the household it is run on, so it
    /// is refused on the default `--household`.
    #[arg(long, value_name = "HOURS")]
    pub harvest_hours: Option<f64>,
    /// Open the nearest planted field harvester's screen on frame 120, as
    /// its F does.
    #[arg(long)]
    pub open_harvester: bool,
    /// Start mounted on the bike, without the call.
    #[arg(long, requires = "bike")]
    pub mounted: bool,
    /// Headless or windowed: start at the road's far end (`--at road`, the
    /// default here), call the bike, ride the road in for forty seconds and
    /// write what was measured to the log and to `--script-report`.
    #[arg(long, requires = "bike", conflicts_with_all = ["connect", "flycam", "flight", "route", "grounding_study", "study_scene", "auto_walk", "no_combat", "script_progression", "script_crafting", "combat_script"])]
    pub script_ride: bool,
    /// Headless or windowed (ticket 0009, step 3): start on dry ground a few
    /// hundred metres from the nearest deposit of this resource class
    /// (`conductor` when none is named, or a full id), walk a straight
    /// heading pinging with the survey tool every hundred metres until a
    /// ping meets something, walk to the strongest point of each ping
    /// until it is under the player's feet, and sample fifteen units. What
    /// it did goes to the log and to `--script-report`.
    #[arg(long, value_name = "CLASS", num_args = 0..=1, default_missing_value = "conductor", conflicts_with_all = ["connect", "flycam", "flight", "route", "grounding_study", "study_scene", "auto_walk", "no_combat", "script_progression", "script_crafting", "combat_script", "script_ride", "at"])]
    pub script_survey: Option<String>,
    /// Start with a bought example of this module (an item id,
    /// `veyra:module.patrol_extended_feed`) fitted to the weapon in hand
    /// it is made for, taken from the household's store as a workbench
    /// would take it; repeatable. For a still of a fitted weapon: it
    /// changes the household it is run on.
    #[arg(long, value_name = "ITEM")]
    pub fitted: Vec<String>,
    /// Start with store grade weapons of these items (comma separated, an
    /// item id each, `-` leaves a slot as it is) in weapon slot one and
    /// slot two of whoever starts out, for a still of a blade in hand. The
    /// member out must be certified on each, or the flag says why not and
    /// changes nothing. It changes the household it is run on, so it is
    /// refused on the default `--household`.
    #[arg(long, value_name = "ITEM", value_delimiter = ',')]
    pub in_hand: Vec<String>,
    /// Start with whoever starts out having learned these boxes (comma
    /// separated ids), past their prerequisites and without spending
    /// anything, so a still can show a weapon nobody starts certified on
    /// (`blade.breaking.1` for the greatblade). It changes the household
    /// it is run on, so it is refused on the default `--household`.
    #[arg(long, value_name = "BOX", value_delimiter = ',')]
    pub learn: Vec<String>,
    /// Start with whoever starts out knowing these abilities (comma
    /// separated ids of `assets/combat/abilities.ron`) and having them
    /// slotted on the bar, where the sim puts a newly known one, for the
    /// stills of the bar and a headless run that presses one
    /// (`precision_shot`). It changes the household it is run on, so it is
    /// refused on the default `--household`.
    #[arg(long, value_name = "ABILITY", value_delimiter = ',')]
    pub grant: Vec<String>,
    /// Press a place of the bar at a frame, `place@frame` (comma
    /// separated): 0 to 2 are the weapon in hand's places and 3 and 4 the
    /// utility's, written where the keys write them. It changes the
    /// household it is run on (what it spends), so it is refused on the
    /// default `--household`.
    #[arg(long, value_name = "PLACE@FRAME", value_delimiter = ',', value_parser = parse_press)]
    pub press: Vec<(usize, u32)>,
    /// Start with the carbine in hand worn down to this condition (0 to
    /// 1000), for a still of the wear. It changes the household it is run
    /// on, so it is refused on the default `--household`.
    #[arg(long, value_name = "POINTS")]
    pub condition: Option<u16>,
    /// Start with this recruit (`"Tovan Reyes"`) joined to the bench and
    /// out, for a still of what a member who knows a trade sees. It changes
    /// the household it is run on, so it is refused on the default
    /// `--household`.
    #[arg(long = "as", value_name = "MEMBER")]
    pub member: Option<String>,
    /// Start with a patrol carbine made by Tovan Reyes (batch 7, this
    /// moddability, the other lines 650) in the arrival's pack; with
    /// `--open-fitting` it is in hand instead, the bought one in the pack.
    /// It changes the household it is run on, so it is refused on the
    /// default `--household`.
    #[arg(long, value_name = "MODDABILITY")]
    pub crafted_carbine: Option<u16>,
    /// Start with this many store-grade mount blanks in the pack of
    /// whoever starts out, for a still of a rework. It changes the
    /// household it is run on, so it is refused on the default
    /// `--household`.
    #[arg(long, value_name = "N")]
    pub blanks: Option<u32>,
    /// Start with one of each of these items (comma separated, an item id
    /// each) in the pack of whoever starts out, for a still of the pack
    /// with every kind of stack in it. A lot is copied from the store when
    /// the store holds one. It changes the household it is run on, so it
    /// is refused on the default `--household`.
    #[arg(long, value_name = "ITEM", value_delimiter = ',')]
    pub give: Vec<String>,
    /// With `--open-fitting`: rework the weapon in the first hand once, by
    /// the request the screen's button sends, before the screen opens. It
    /// spends a blank, so it is refused on the default `--household`.
    #[arg(long, requires = "open_fitting")]
    pub rework_first: bool,
    /// Hold the combat camera this many metres behind the character, for a
    /// still of what is in the hand.
    #[arg(long, value_name = "METRES")]
    pub orbit_distance: Option<f32>,
    /// Turn the combat camera this many degrees round the character from
    /// behind it at startup, for a still from the side. The character keeps
    /// facing the view's heading.
    #[arg(long, value_name = "DEGREES")]
    pub orbit_yaw: Option<f32>,
    /// Start the combat camera at this pitch in degrees (negative looks
    /// down; the default is about -9), for a still of something far off on
    /// ground the default pitch looks over.
    #[arg(long, value_name = "DEGREES", allow_hyphen_values = true)]
    pub orbit_pitch: Option<f32>,
    /// Open the live rendering panel on frame 120.
    #[arg(long)]
    pub open_render_panel: bool,
    /// Save screenshots here. `{frame}` in the name is replaced by the
    /// frame number, which `--shot-frames` needs to keep them apart.
    /// Implies a window; the app exits once every shot is on disk.
    #[arg(long)]
    pub screenshot: Option<PathBuf>,
    /// Frames to capture on, comma separated. Defaults to `--frames`, or
    /// 60.
    #[arg(long, value_delimiter = ',')]
    pub shot_frames: Vec<u32>,
    /// Write a benchmark summary here as JSON when the run finishes: every
    /// frame after `--bench-warmup`, as percentiles, with the GPU time and
    /// what was loaded. `just bench` reads it.
    #[arg(long)]
    pub bench_out: Option<PathBuf>,
    /// Seconds of real time a benchmark ignores while the world streams in;
    /// with `--bench-warm idle`, the least it waits.
    #[arg(long, default_value_t = 10.0)]
    pub bench_warmup: f64,
    /// When a benchmark starts measuring: `fixed` after `--bench-warmup`
    /// seconds, `idle` once the foliage streamer has nothing in flight and
    /// has placed nothing for two seconds (after `--bench-warmup`, and by
    /// `--bench-warm-ceiling` at the latest). A static view takes about
    /// 50 s to fill its far trees, so `fixed` measures that streaming.
    /// `hold`, for a moving fixture (`--flight`, `--auto-walk`, `--route`):
    /// the motion waits at its start until terrain (no region waiting for a
    /// mesh, no meshing task held) and foliage are idle, within the same
    /// floor and ceiling, then starts, and measuring begins
    /// `--bench-lead` seconds later.
    #[arg(long, default_value = "fixed", value_parser = ["fixed", "idle", "hold"])]
    pub bench_warm: String,
    /// The most seconds `--bench-warm idle` or `hold` waits for the
    /// streamers before it goes on anyway; the summary then says `ceiling`.
    #[arg(long, default_value_t = 90.0)]
    pub bench_warm_ceiling: f64,
    /// With `--bench-warm hold`: seconds the motion runs before measuring,
    /// so the walker is up to speed and the streamers have met the first
    /// ground the motion brings.
    #[arg(long, default_value_t = 5.0)]
    pub bench_lead: f64,
    /// Exit once a benchmark has measured this many frames after its
    /// warm-up, whatever the warm-up took; `--frames` counts from the start.
    #[arg(long)]
    pub bench_frames: Option<usize>,
    /// Exit once a benchmark has measured this many seconds of real time
    /// after its warm-up. A walk's length is distance, which a frame count
    /// is not at a frame rate that varies.
    #[arg(long)]
    pub bench_secs: Option<f64>,
    /// Add Bevy's wireframe plugin, so F4 draws wireframes. Off by default:
    /// the plugin checks every mesh entity every frame for specialization
    /// whether or not F4 is ever pressed.
    #[arg(long)]
    pub wireframe: bool,
    /// Open the pause menu once the world has been drawn for a while
    /// (frame 120), for a screenshot of the screens over it.
    #[arg(long)]
    pub open_pause: bool,
    /// Open the character sheet on frame 120 (or, with
    /// `--script-progression`, at the blade trainer instead of the lesson,
    /// where the path then ends), for a screenshot.
    #[arg(long)]
    pub open_sheet: bool,
    /// Open the character sheet on its Loadout tab on frame 120, for a
    /// screenshot.
    #[arg(long)]
    pub open_loadout: bool,
    /// Open the bench on frame 120 (or, with `--script-progression`, at
    /// the terminal once the recruits joined, instead of the swap), for a
    /// screenshot.
    #[arg(long)]
    pub open_bench: bool,
    /// Open the stores on frame 120 with the household's store beside the
    /// pack, as a locker's F does, for a screenshot. No locker need be in
    /// reach: the run reaches the store from where it stands.
    #[arg(long)]
    pub open_stores: bool,
    /// Open the pack on frame 120 without the household's store, as key I
    /// does, for a screenshot.
    #[arg(long)]
    pub open_inventory: bool,
    /// Open the fitting screen on frame 120 with this mount in focus (the
    /// feed when none is named), with its buttons live as at a workbench,
    /// for a screenshot: no workbench need be in reach. With
    /// `--script-crafting` the script walks to the workbench and uses it as
    /// F would instead, where the path then ends.
    #[arg(long, value_name = "MOUNT", num_args = 0..=1, default_missing_value = "feed")]
    pub open_fitting: Option<String>,
    /// With `--open-fitting`: the hand whose weapon is on the bench, 0 the
    /// carbine's and 1 the pistol's; the weapon in hand when not given.
    #[arg(long, value_name = "HAND", requires = "open_fitting")]
    pub fitting_hand: Option<usize>,
    /// With `--open-fitting`: pick the first part the stock has for the
    /// mount, so the still shows what fitting it would do.
    #[arg(long, requires = "open_fitting")]
    pub fitting_pick: bool,
    /// How the fitting screen draws its weapon: `model`, the baked pieces
    /// turning on their stage, or `diagram`, the flat one. For a still of
    /// either; the model needs `just content` to have run.
    #[arg(long, value_name = "VIEW", default_value = "model", value_parser = ["model", "diagram"])]
    pub fitting_view: String,
    /// Open the workbench's crafting tab on frame 120 on this schematic
    /// (`cell_core` when none is named, or a full id), with its buttons
    /// live as at a workbench, for a screenshot: no workbench need be in
    /// reach. With `--script-crafting` the script walks to the workbench
    /// and opens it there instead, where the path then ends.
    #[arg(long, value_name = "SCHEMATIC", num_args = 0..=1, default_missing_value = "cell_core")]
    pub open_crafting: Option<String>,
    /// With `--open-crafting`: which of the screen's states the still
    /// shows. `pick` is the slots with the best stock in them and what
    /// comes out, as the screen opens; `empty` is the same with nothing
    /// picked; `hand` assembles the best stock first, for the bench hand;
    /// `done` also spends the points and finishes, for the finished card;
    /// `record` is that card with Record as blueprint open at
    /// `--record-derate`. The last three change the household they are run
    /// on, so they are refused on the default `--household`: name another.
    #[arg(
        long,
        value_name = "STATE",
        value_enum,
        default_value_t,
        requires = "open_crafting"
    )]
    pub crafting_state: CraftingShot,
    /// With `--open-crafting`: what the schematics list shows, as its chips
    /// do (Known is the default).
    #[arg(long, value_name = "FILTER", value_enum, requires = "open_crafting")]
    pub crafting_filter: Option<CraftingListShot>,
    /// Before a still's screen opens, make one of this schematic
    /// (`cell_core`, or a full id) from the best stock, every point spent
    /// steady, by the requests a screen would send: so a still of the
    /// magazine's schematic has a core to pick, and one of the stores a
    /// made thing to pin. It changes the household it is run on, so it is
    /// refused on the default `--household`: name another.
    #[arg(long, value_name = "SCHEMATIC", conflicts_with = "script_crafting")]
    pub craft_first: Option<String>,
    /// With `--open-crafting --crafting-state record`: what the blueprint
    /// is drafted at, percent of the prototype, one of the dial's steps.
    #[arg(
        long,
        value_name = "PERCENT",
        default_value_t = 85,
        requires = "open_crafting"
    )]
    pub record_derate: u16,
    /// With `--craft-first`: record what it made as a blueprint at this
    /// percent of it (85 when none is named), by the request the Record
    /// box would send, so a still of the factory has a blueprint to show.
    #[arg(
        long,
        value_name = "PERCENT",
        num_args = 0..=1,
        default_missing_value = "85",
        requires = "craft_first"
    )]
    pub record_first: Option<u16>,
    /// Open the factory on frame 120, as a fabricator's F does, for a
    /// screenshot: no fabricator need be in reach.
    #[arg(long)]
    pub open_factory: bool,
    /// Open the journal on frame 120, for a screenshot; with `--quest-stage`
    /// the stage it shows.
    #[arg(long)]
    pub open_journal: bool,
    /// Start before a person of the questline (`maren_coyle`, `pell`) and
    /// open their conversation once they are in reach, for a screenshot.
    #[arg(long, value_name = "PERSON")]
    pub open_dialogue: Option<String>,
    /// Open Pelagic Works' procurement board on frame 120, as a board's F
    /// does, for a screenshot: no board need be in reach.
    #[arg(long)]
    pub open_board: bool,
    /// Before a still's board opens, put 100 cell cores of batch 412 by
    /// Tovan Reyes at charge density 724 in the store, so the board has
    /// something to quote. It changes the household it is run on, so it is
    /// refused on the default `--household`: name another.
    #[arg(long, requires = "open_board", conflicts_with = "script_crafting")]
    pub board_stock: bool,
    /// With `--open-board`: a few frames after the board opens, its first
    /// row's sale is sent, by the request its button would send. It
    /// changes the household it is run on, so it is refused on the default
    /// `--household`: name another.
    #[arg(long, requires = "open_board", conflicts_with = "script_crafting")]
    pub board_sell: bool,
    /// Before a still's screen opens, start a run of this many (20 when
    /// none is named, and no more than the stock feeds) of the first
    /// blueprint in reach from the cheapest stock that meets it, by the
    /// request a screen would send: so a still of the factory or of the
    /// bench has a run on. It changes the household it is run on, so it is
    /// refused on the default `--household`: name another.
    #[arg(
        long,
        value_name = "N",
        num_args = 0..=1,
        default_missing_value = "20",
        conflicts_with = "script_crafting"
    )]
    pub factory_run: Option<u32>,
    /// With `--factory-run`: somebody on the bench works the run, the
    /// first recruit joining when nobody is there to.
    #[arg(long, requires = "factory_run")]
    pub factory_worker: bool,
    /// With `--factory-run`: the still's screen opens once the run has
    /// finished and not when it has started, for a still of what it made.
    /// The run takes its played time, so the screenshot's frame must be
    /// late enough for it.
    #[arg(long, requires = "factory_run")]
    pub factory_finish: bool,
    /// With `--factory-run`: once the run has started, every empty cell of
    /// the store is taken, so the run stands for want of room when its
    /// first unit is made, for a still of the factory paused. The
    /// screenshot's frame must be a unit's played time after the start.
    #[arg(long, requires = "factory_run", conflicts_with = "factory_finish")]
    pub factory_full_store: bool,
    /// Before a still's screen opens, everyone who can joins the bench, as
    /// at a terminal: so a still of the bench has more than one member on
    /// it. It changes the household it is run on, so it is refused on the
    /// default `--household`: name another.
    #[arg(long, conflicts_with = "script_crafting")]
    pub join_all: bool,
    /// Before a still of the factory opens, put in the store sixty units
    /// of the nearest deposit's lot that fits a slot of the first
    /// blueprint in reach and does not meet it, so the still shows a lot
    /// refused in place with its number. It changes the household it is
    /// run on, so it is refused on the default `--household`: name
    /// another.
    #[arg(long, conflicts_with = "script_crafting")]
    pub factory_poor_lot: bool,
    /// With `--open-stores` or `--open-inventory`: the filter chip in
    /// force (`all`, `lots`, `parts`, `mods`, `blueprints`, `gear`, `bags`),
    /// with the first stack it shows pinned, for a screenshot of one kind
    /// of stock.
    #[arg(long, value_name = "FILTER", value_parser = parse_filter)]
    pub stores_filter: Option<veyra_ui::stores::Filter>,
    /// With `--stores-filter`: pin the stack this many after the first the
    /// filter shows, in the order of the screen's cells (the last when
    /// there are fewer), for a still of a stack that is not the first of
    /// its kind.
    #[arg(
        long,
        value_name = "N",
        default_value_t = 0,
        requires = "stores_filter"
    )]
    pub stores_pin: usize,
    /// Open the survey screen on frame 120, for a screenshot: the tool's
    /// ping, its last grid and the deposits it met.
    #[arg(long)]
    pub open_survey: bool,
    /// Open the map on frame 120 at a scale (`near`, `region` or `far`;
    /// the region's when none is named), for a screenshot.
    #[arg(long, value_name = "SCALE", num_args = 0..=1, default_missing_value = "region")]
    pub open_map: Option<String>,
    /// Lay the map's Holders layer on, for a screenshot of whose ground it is.
    #[arg(long, requires = "open_map")]
    pub map_holders: bool,
    /// Ping for a class of resource (`conductor`, or a full id) once the
    /// player stands in the world, as the survey screen's Ping would, so a
    /// still has a grid to show. The channel plays out in the first
    /// seconds; the household finds what it meets.
    #[arg(long, value_name = "CLASS")]
    pub ping: Option<String>,
    /// Start sampling where the player stands once a ping has found a
    /// deposit there (`--ping`), as Sample here would. It puts lots in the
    /// pack, so it is refused on the default `--household`.
    #[arg(long)]
    pub sampling: bool,
    /// Open the terminal's survey register on frame 120, read-only as the
    /// pause's Bench button gives it, for a screenshot.
    #[arg(long)]
    pub open_register: bool,
    /// Track the nearest deposit of this resource type to the default
    /// start (`light_alloy_ore`, or a full id), as Accept on its row of
    /// the register does, so the HUD's marker is up for a screenshot.
    #[arg(long, value_name = "TYPE")]
    pub track: Option<String>,
    /// Hold the trigger down from frame 120 for this many seconds, as the
    /// left button would, for a screenshot of the reticle as the cone
    /// grows. It fires the player's rounds, so it is refused on the
    /// default `--household`: name another.
    #[arg(long, value_name = "SECONDS")]
    pub hold_fire: Option<f32>,
    /// Hold the player's guard (the aim button with a melee weapon in
    /// hand) for this many seconds from frame 120, for a screenshot of the
    /// raised blade. With `--script-progression --script-hurt n` a hostile's
    /// blow meets it. Refused on the default `--household`: name another.
    #[arg(long, value_name = "SECONDS")]
    pub hold_guard: Option<f32>,
    /// Hold the player's aim button for this many seconds from frame 120,
    /// for a screenshot of a cone closing and of the scoped view. With a
    /// melee weapon in hand it is the guard. Refused on the default
    /// `--household`: name another.
    #[arg(long, value_name = "SECONDS")]
    pub hold_aim: Option<f32>,
    /// The camp nearest the start is cleared from the first frame, for a
    /// still of the card on a quiet camp. It writes the household, so it is
    /// refused on the default `--household`.
    #[arg(long)]
    pub cleared: bool,
    /// The camp nearest the start is cleared from the first frame with its
    /// cache standing, for a still of the chest and its F chip. It writes
    /// the household, so it is refused on the default `--household`.
    #[arg(long)]
    pub cache: bool,
    /// Take this posting (1 is the board's first) of Pelagic's board from the
    /// first frame, for a still of the work and of the area heard of. It
    /// writes the household, so it is refused on the default `--household`.
    #[arg(long, value_name = "N", value_parser = clap::value_parser!(usize))]
    pub take_job: Option<usize>,
    /// With `--take-job`: the taken job's camp counts cleared after the take,
    /// for a still of the board's Collect.
    #[arg(long, requires = "take_job")]
    pub job_done: bool,
    /// The camp of this name, or the nearest to the start, starts found, for
    /// a still of its point on the map. It writes the household, so it is
    /// refused on the default `--household`.
    #[arg(long, value_name = "NAME|nearest")]
    pub found: Option<String>,
    /// The member who is out is carried home at frame 200 wherever they are,
    /// through the path a fall in the world takes. It writes the household,
    /// so it is refused on the default `--household`.
    #[arg(long)]
    pub fall: bool,
    /// This member starts injured for the fall's hours, for a still of the
    /// bench card. It writes the household, so it is refused on the default
    /// `--household`.
    #[arg(long, value_name = "MEMBER")]
    pub injured: Option<String>,
    /// Start with a faction's stance set, `name=welcoming|wary|hostile`, by
    /// a deed written at the start so the score sits in the band. Repeat for
    /// more. Refused on the default `--household`: name another.
    #[arg(long, value_name = "FACTION=STANCE")]
    pub stance: Vec<String>,
    /// Start with a live lease of Pelagic Works' concessions, for a still of
    /// the board and of working a concession lawfully. Refused on the
    /// default `--household`: name another.
    #[arg(long)]
    pub lease: bool,
    /// With `--script-progression`: the nearest hostile lands this many
    /// hits on the player from four seconds into the fight, a second
    /// apart, which no scripted run otherwise does. One shows the hurt
    /// path for a screenshot; eight bring the low-health pulse and the
    /// fall.
    #[arg(long, default_value_t = 0)]
    pub script_hurt: u32,
    /// Start with the debug statistics shown (F3 toggles them either
    /// way). Off by default so a screenshot is of the game; the numbers
    /// reach the log and `--bench-out` without the overlay.
    #[arg(long)]
    pub stats: bool,
}

impl DevArgs {
    /// `--crafting-state` as the crafting screen takes it.
    #[must_use]
    pub const fn crafting_still(&self) -> veyra_ui::crafting::Still {
        self.crafting_state.still(self.record_derate)
    }

    /// The flag of the first still that spends the stock of the household it
    /// is run on and moves its roll and batch counters: the flags send the
    /// requests a player's presses would, and what they do is saved. The
    /// scripted walk is no still and is not held to this.
    #[must_use]
    pub fn spends_stock(&self) -> Option<&'static str> {
        Some(if self.craft_first.is_some() {
            "--craft-first"
        } else if self.factory_run.is_some() {
            "--factory-run"
        } else if self.factory_poor_lot {
            "--factory-poor-lot"
        } else if self.cleared {
            "--cleared"
        } else if self.cache {
            "--cache"
        } else if self.take_job.is_some() {
            "--take-job"
        } else if self.found.is_some() {
            "--found"
        } else if self.fall {
            "--fall"
        } else if self.injured.is_some() {
            "--injured"
        } else if !self.stance.is_empty() {
            "--stance"
        } else if self.lease {
            "--lease"
        } else if self.hold_fire.is_some() {
            "--hold-fire"
        } else if self.hold_guard.is_some() {
            "--hold-guard"
        } else if self.hold_aim.is_some() {
            "--hold-aim"
        } else if self.board_stock {
            "--board-stock"
        } else if self.board_sell {
            "--board-sell"
        } else if self.join_all {
            "--join-all"
        } else if !self.learn.is_empty() {
            "--learn"
        } else if !self.in_hand.is_empty() {
            "--in-hand"
        } else if !self.grant.is_empty() {
            "--grant"
        } else if !self.press.is_empty() {
            "--press"
        } else if self.condition.is_some() {
            "--condition"
        } else if self.member.is_some() {
            "--as"
        } else if self.crafted_carbine.is_some() {
            "--crafted-carbine"
        } else if self.blanks.is_some() {
            "--blanks"
        } else if !self.give.is_empty() {
            "--give"
        } else if self.purse.is_some() {
            "--purse"
        } else if self.saddle_stock {
            "--saddle-stock"
        } else if self.harvester {
            "--harvester"
        } else if self.harvest_hours.is_some() {
            "--harvest-hours"
        } else if self.bike_condition.is_some() {
            "--bike-condition"
        } else if self.rework_first {
            "--rework-first"
        } else if self.sampling {
            "--sampling"
        } else if self.ping.is_some() {
            "--ping"
        } else if self.script_survey.is_some() {
            "--script-survey"
        } else if self.script_quest.is_some() {
            "--script-quest"
        } else if self.quest_stage.is_some() {
            "--quest-stage"
        } else if self.open_crafting.is_some()
            && !self.script_crafting
            && matches!(
                self.crafting_state,
                CraftingShot::Hand | CraftingShot::Done | CraftingShot::Record
            )
        {
            "--crafting-state hand|done|record"
        } else {
            return None;
        })
    }

    /// Whether a flag opens a screen over the world, for a still.
    #[must_use]
    pub fn opens_a_screen(&self) -> bool {
        self.open_sheet
            || self.open_loadout
            || self.open_bench
            || self.open_stores
            || self.open_inventory
            || self.open_survey
            || self.open_map.is_some()
            || self.open_journal
            || self.open_dialogue.is_some()
            || self.open_register
            || self.open_fitting.is_some()
            || self.open_crafting.is_some()
            || self.open_factory
            || self.open_board
    }

    /// What the headless side reads of these flags, set on its [`veyra_game::Launch`].
    pub fn launch(&self, launch: &mut veyra_game::Launch) {
        launch.combat_script = self.combat_script;
        launch.script_survey.clone_from(&self.script_survey);
        launch.script_ride = self.script_ride;
        launch.script_quest.clone_from(&self.script_quest);
        launch.open_dialogue.clone_from(&self.open_dialogue);
        launch.quest_stage.clone_from(&self.quest_stage);
        launch.fall = self.fall;
        launch.injured.clone_from(&self.injured);
    }

    /// The implications between scripts, which a run states once: a
    /// crafting script is the progression script, which is the combat
    /// script.
    pub fn resolve(&mut self) {
        if self.script_crafting {
            self.script_progression = true;
        }
        if self.script_progression {
            self.combat_script = true;
        }
    }
}
