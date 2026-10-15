//! `--script-crafting`: the stops after the lesson at the workbench, the
//! fabricator and the deposit, with the report each writes. Separate
//! from the progression walk it extends because the stops, their limits and
//! their reports are long and change on their own.

use super::bench::Out;
use super::*;
use strata::registry::progression::XpKind;

/// The stock's layout at the locker with the store open, as the sim declares
/// it: a household with a bike has the saddle's cells before the store's, so
/// a click predicted on a layout without them lands on the wrong slot.
fn locker_layout(
    items: &ItemCatalogue,
    pack: &Pack,
    stable: &strata::sim::ride::Stable,
) -> items::StockLayout {
    let saddle = stable.bike.is_some().then(|| items::SaddleSlots {
        cells: u16::try_from(stable.saddle.len()).unwrap_or(u16::MAX),
        access: items::SaddleAccess::of(false, true),
    });
    items::layout(items.carry(), items, pack, true, saddle)
}

/// The saddle as the sim's stock has it at the locker, over a copy of its
/// cells; none for a household without a bike.
fn locker_saddle<'a>(
    stable: &strata::sim::ride::Stable,
    cells: &'a mut slotted_model::Inventory,
) -> Option<items::SaddleStock<'a>> {
    stable.bike.is_some().then(|| items::SaddleStock {
        cells,
        access: items::SaddleAccess::of(false, true),
    })
}

/// Whether a part is fitted to the carbine in hand. The loadout is never
/// empty: the bought carbine carries its own action line, which is no part.
fn has_parts(actor: &Combatant) -> bool {
    actor.loadout().is_some_and(|loadout| {
        loadout
            .mods
            .values()
            .flatten()
            .any(|factor| factor.property != strata::registry::item::Property::RepeatInterval)
    })
}

/// What `--script-crafting` read off the `Combatant` at the workbench, as
/// `--script-report` writes it: the carbine's row with nothing fitted,
/// with the magazine fitted and with it taken off again, and the rounds in
/// the magazine either side of the fit and of the removal.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub(super) struct CraftingReport {
    /// The row before anything was fitted.
    pub bare: Option<WeaponSpec>,
    /// The row with the magazine on the feed.
    pub fitted: Option<WeaponSpec>,
    /// Rounds before the fit and after it: kept, not refilled.
    pub ammo_at_fit: (u16, u16),
    /// Rounds after the reload that followed: the fitted magazine, full.
    pub reloaded_to: u16,
    /// Rounds before the removal and after it: clamped to the bare size.
    pub ammo_at_remove: (u16, u16),
    /// The row after the removal.
    pub restored: Option<WeaponSpec>,
    /// The lot sampled at the deposit; none if the sampling never happened.
    #[serde(default)]
    pub draw: Option<DrawReport>,
    /// What was made at the bench after the removal.
    #[serde(default)]
    pub made: MadeReport,
    /// What was recorded there and run at the fabricator afterwards.
    #[serde(default)]
    pub factory: FactoryReport,
    /// What the run's cores sold for at the board; none if they never did.
    #[serde(default)]
    pub sale: Option<SaleReport>,
    /// The carbine the smith made after the sale and whose hand it ended
    /// in.
    #[serde(default)]
    pub carbine: CarbineReport,
}

/// What `--script-crafting` made last: the smith's carbine, every point on
/// moddability, and the hand it was taken in.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub(super) struct CarbineReport {
    /// The batch it carries.
    pub batch: u32,
    /// Who made it.
    pub by: Option<String>,
    /// The points the session opened with.
    pub points: u8,
    /// The moddability the frame allowed, before any roll.
    pub ceiling: u16,
    /// Its three lines, finished.
    pub lines: BTreeMap<String, u16>,
    /// The mounts it opens, and the mounts the chassis has.
    pub open: (u32, u32),
    /// The mounts a bought carbine opens.
    pub bought_open: u32,
    /// Whose hand it ended in; empty if it never reached one.
    pub in_hand_of: String,
    /// Each rework of it before it went to the store: what the roll came
    /// to, the moddability before and after. One blank bought for each.
    #[serde(default)]
    pub reworks: Vec<(String, u16, u16)>,
    /// Every credit of Fabrication XP the path saw, in order: each thing
    /// made by hand, then each rework that raised the line.
    #[serde(default)]
    pub credits: Vec<u32>,
}

/// What `--script-crafting` sold last: the cores the fabricator made, on
/// the cell core line of Pelagic Works.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub(super) struct SaleReport {
    /// The batch the cores carry.
    pub batch: u32,
    /// Their charge density, which the line grades them on.
    pub grade: u16,
    /// How many went.
    pub count: u32,
    /// What they paid, in credits.
    pub paid: u32,
    /// The purse before the sale and after it.
    pub purse: (u32, u32),
    /// Every line the sold cores carry, to hold to the blueprint's.
    pub lines: std::collections::BTreeMap<String, u16>,
}

/// What `--script-crafting` recorded and ran: the second core as a
/// blueprint, as the bench drafted it at the whole of the core and at 85 %
/// before Record, and a batch run off it at the fabricator from the
/// cheapest stock while the rest of the path was walked.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub(super) struct FactoryReport {
    /// What the bench showed for each derating before Record was pressed.
    pub drafts: Vec<DraftReport>,
    /// The blueprint's number; none if it was never recorded.
    pub blueprint: Option<u32>,
    /// The prototype's batch.
    pub prototype: u32,
    /// The runs the blueprint started with, and the runs it had left when
    /// the run ended.
    pub runs: (u32, u32),
    /// The largest batch the stock at the fabricator fed, and the batch
    /// asked for.
    pub most: u32,
    /// The batch asked for.
    pub size: u32,
    /// The batch number the run's units carry.
    pub batch: u32,
    /// The played seconds the sim said a batch of that size takes: with a
    /// worker on it, and with nobody.
    pub promised_secs: (u32, u32),
    /// How many the fabricator said it had made when it finished.
    pub made: u32,
    /// The played seconds from the start to the finish, by the ticks
    /// counted between them.
    pub played_secs: f64,
}

/// A blueprint as the bench drafted it at one derating: its lines, what
/// each slot asks, which of the stock in reach meets it and the largest
/// batch that stock feeds.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub(super) struct DraftReport {
    /// The derating, per-mille of the prototype.
    pub derate: u16,
    /// The lines every unit would have.
    pub lines: BTreeMap<String, u16>,
    /// What each slot asks: the slot, the figure, the least and the most.
    pub needs: Vec<(String, String, Option<u16>, Option<u16>)>,
    /// The stock that fits each slot: the slot, the lot, its units, and
    /// why it does not meet the blueprint when it does not.
    pub stock: Vec<(String, String, u32, Option<String>)>,
    /// The largest batch that stock feeds.
    pub most: u32,
}

/// What `--script-crafting` made at the workbench: a cell core from the
/// first lots, the patrol magazine from that core, and a second core from
/// the same lots, each line as its finished record holds it; what the
/// bench said the magazine would do before it was fitted, and what the
/// `Combatant` read once it was.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub(super) struct MadeReport {
    /// The first core's lines as the bench showed them before Assemble:
    /// the line, the floor assembly gives, the ceiling.
    pub core_range: Vec<(String, u16, u16)>,
    /// The first core's lines, finished.
    pub core: BTreeMap<String, u16>,
    /// The magazine's lines as the bench showed them before Assemble, with
    /// that core in its slot.
    pub magazine_range: Vec<(String, u16, u16)>,
    /// The magazine's lines, finished.
    pub magazine: BTreeMap<String, u16>,
    /// What the bench said the finished magazine gives on the carbine,
    /// before Finish: the rounds, and the reload in hundredths of a second.
    pub previewed: (i32, i32),
    /// What the `Combatant` read with it fitted: the magazine, the reload.
    pub fitted: Option<(u16, f32)>,
    /// The second core's lines, finished: the same lots, other rolls.
    pub second: BTreeMap<String, u16>,
    /// What each of the three was rolled, in order, as `line: result`.
    pub rolled: Vec<String>,
    /// The household's roll counter when the stop ended.
    pub rolls: u64,
    /// And its batch counter.
    pub batches: u32,
}

/// What `--script-crafting` read at the deposit and at the locker
/// afterwards: the deposit as the world gives it, the lot as the pack held
/// it, the purse either side of the draw and the store either side of the
/// lot.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub(super) struct DrawReport {
    /// The deposit drawn from.
    pub deposit: DepositId,
    /// Where it lies, `Calder flats`.
    pub place: String,
    /// The deposit's figures, in its class's order.
    pub figures: Vec<(Attribute, u16)>,
    /// The lines of the lot the pack held after the draw, by attribute
    /// key.
    pub lot_lines: Vec<(String, u16)>,
    /// The units drawn, and the units the pack's lot held.
    pub units: (u32, u32),
    /// The purse before the draw.
    pub purse_before: u32,
    /// And after it.
    pub purse_after: u32,
    /// Units of this deposit in the household's store before the draw.
    pub store_before: u32,
    /// And after the lot was sent across at the locker.
    pub store_after: u32,
    /// Stacks in the pack's own cells before the draw.
    pub pack_before: u32,
    /// And after the lot was sent across.
    pub pack_after: u32,
}

/// A step of the workbench stop.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum BenchStep {
    /// Drawing the carbine, if the fight left the pistol in hand.
    Draw,
    /// Reloading to a full magazine, so the counts that follow do not
    /// depend on how the fight went.
    Top,
    /// Firing three rounds.
    Fire,
    /// Asking for the magazine to be fitted.
    Fit,
    /// Reloading into the fitted magazine.
    Reload,
    /// Firing two rounds.
    FireFitted,
    /// Asking for it to be taken off.
    Remove,
    /// Assembling a cell core from the best stock: the first lots.
    Core,
    /// Spending its points.
    CoreRolls,
    /// Finishing it.
    CoreFinish,
    /// Assembling the patrol magazine with that core.
    Magazine,
    /// Spending its points.
    MagazineRolls,
    /// Finishing it.
    MagazineFinish,
    /// Fitting the magazine just made, which then stays on the carbine.
    FitMade,
    /// Assembling a second core from the same lots.
    Second,
    /// Spending its points.
    SecondRolls,
    /// Finishing it.
    SecondFinish,
    /// Recording that second core as a blueprint, which ends the stop.
    Record,
}

impl BenchStep {
    /// Whether the step is one of the crafting ([`script_craft`]) and not
    /// of the fitting and firing before it ([`script_bench`]).
    pub(super) const fn crafting(self) -> bool {
        !matches!(
            self,
            Self::Draw
                | Self::Top
                | Self::Fire
                | Self::Fit
                | Self::Reload
                | Self::FireFitted
                | Self::Remove
        )
    }
}

/// The workbench stop's state, with `--script-crafting`.
#[derive(Resource, Debug)]
pub(super) struct CraftingScript {
    pub(super) step: BenchStep,
    /// Seconds in the step.
    pub(super) since: f32,
    /// The actor's shot counter when the step's firing began.
    pub(super) mark: u64,
    /// Whether the step's request has been sent.
    pub(super) asked: bool,
    pub(super) report: CraftingReport,
    /// `--script-report`: where the report is written when the stop ends.
    pub(super) path: Option<PathBuf>,
    /// `--open-fitting`: open the fitting screen at the workbench instead
    /// of fitting by request, for a screenshot.
    pub(super) open_fitting: bool,
    /// `--open-crafting`: open the crafting tab at the workbench instead,
    /// on this schematic and brought to this state, for a screenshot.
    pub(super) open_crafting: Option<(String, strata::client::ui::crafting::Still)>,
    /// The last thing finished at the bench: the core the magazine is made
    /// with, then the magazine that is fitted.
    pub(super) made: Option<Record>,
    /// The stop at the deposit and the locker after it.
    pub(super) survey: SurveyWalk,
    /// The stop at the fabricator and the wait for its run.
    pub(super) run: RunWalk,
    /// The smith's carbine and its handing over.
    pub(super) carbine: CarbineWalk,
}

/// A step of the smith's carbine, at the workbench and then the locker.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(super) enum CarbineStep {
    /// Switching to the smith, directly.
    #[default]
    Smith,
    /// Assembling the carbine from the store's metal.
    Assemble,
    /// Spending every point on moddability.
    Rolls,
    /// Finishing it.
    Finish,
    /// Taking it in hand, so the bench can rework it.
    Lift,
    /// Buying a mount blank.
    Blank,
    /// Reworking the carbine in hand with it.
    Rework,
    /// Taking the smith's own carbine back in hand.
    Return,
    /// At the locker: opening the stores.
    Open,
    /// Sending the carbine across to the household's store.
    Send,
    /// Closing the stores.
    Close,
    /// Switching to the arrival, directly.
    Arrive,
    /// Opening the stores again, as the arrival.
    Reopen,
    /// Taking the carbine in hand.
    Take,
    /// Waiting for the file to be written, then the end.
    Settle,
}

/// The smith's carbine walk: where it is and what it has made.
#[derive(Debug, Default)]
pub(super) struct CarbineWalk {
    pub(super) step: CarbineStep,
    /// Whether the step's request has been sent.
    pub(super) asked: bool,
    /// The smith has been switched to for the bench stop.
    pub(super) smith_out: bool,
    /// The carbine, once finished.
    pub(super) product: Option<Record>,
    /// The carbine that was in hand before the made one was taken.
    pub(super) own: Option<Record>,
    /// Blanks bought so far.
    pub(super) blanks: u32,
    /// The script's clock when the last step began.
    pub(super) began: f32,
}

/// The fabricator stop's state: the blueprint the bench recorded, what has
/// been asked and what the fabricator has said since.
#[derive(Debug, Default)]
pub(super) struct RunWalk {
    /// The blueprint recorded at the workbench.
    pub(super) blueprint: Option<u32>,
    /// Whether the start has been asked for and not yet answered.
    pub(super) asked: bool,
    /// Whether the run was started.
    pub(super) started: bool,
    /// The ticks counted since it was.
    pub(super) ticks: u32,
    /// How many it had made when it said it was done.
    pub(super) finished: Option<u32>,
}

/// The survey stop's state: where the player was carried from, the deposit
/// carried to, and what has been asked.
#[derive(Debug, Default)]
pub(super) struct SurveyWalk {
    /// Where the player stood before being carried to the deposit.
    pub(super) home: Option<WorldPos>,
    /// The deposit the stop is at.
    pub(super) target: Option<Deposit>,
    /// Where the stop at the deposit has got to: 0 to ping, 1 pinging, 2
    /// sampling, 3 stopped.
    pub(super) stage: u8,
    /// Units the sampling has given so far.
    pub(super) got: u32,
    /// Frames waited since the stop was sent.
    pub(super) stopped: u8,
    /// The step of the stowing at the locker.
    pub(super) stow: u8,
    /// The purse and the store before the draw, and the pack's stacks.
    pub(super) before: (u32, u32, u32),
    /// What the draw came to, once it has.
    pub(super) drawn: Option<DrawReport>,
}

impl CraftingScript {
    pub(super) fn new(
        path: Option<PathBuf>,
        open_fitting: bool,
        open_crafting: Option<(String, strata::client::ui::crafting::Still)>,
    ) -> Self {
        Self {
            step: BenchStep::Draw,
            since: 0.,
            mark: 0,
            asked: false,
            report: CraftingReport::default(),
            path,
            open_fitting,
            open_crafting,
            made: None,
            survey: SurveyWalk::default(),
            run: RunWalk::default(),
            carbine: CarbineWalk::default(),
        }
    }

    pub(super) fn next(&mut self, step: BenchStep) {
        info!("script: bench {:?} -> {step:?}", self.step);
        self.step = step;
        self.since = 0.;
        self.asked = false;
    }

    /// Write the report, if a path was given. A browser has no files to write
    /// it to, so there it is skipped.
    #[cfg(target_arch = "wasm32")]
    pub(super) fn write(&self) {
        if self.path.is_some() {
            info!("script: the crafting report is not written on the web");
        }
    }

    /// Write the report, if a path was given.
    #[cfg(not(target_arch = "wasm32"))]
    pub(super) fn write(&self) {
        let Some(path) = &self.path else {
            return;
        };
        let written = ron::ser::to_string_pretty(&self.report, ron::ser::PrettyConfig::default())
            .map_err(|error| error.to_string())
            .and_then(|text| {
                if let Some(parent) = path.parent() {
                    std::fs::create_dir_all(parent).map_err(|error| error.to_string())?;
                }
                std::fs::write(path, text).map_err(|error| error.to_string())
            });
        match written {
            Ok(()) => info!("script: the crafting report is at {}", path.display()),
            Err(error) => error!("script: writing {}: {error}", path.display()),
        }
    }
}

/// The module the script fits and the mount it goes on.
pub(super) const SCRIPT_MODULE: &str = "veyra:module.patrol_extended_feed";

/// The hand the script works on: the carbine's.
pub(super) const SCRIPT_HAND: usize = 0;

/// The schematic the script makes first, and again last.
pub(super) const SCRIPT_CORE: &str = "veyra:schematic.cell_core";

/// The schematic it makes of the first core: the one that makes
/// [`SCRIPT_MODULE`].
pub(super) const SCRIPT_MAGAZINE: &str = "veyra:schematic.patrol_extended_feed";

/// What the script records its second core at, per-mille: the whole of it.
pub(super) const SCRIPT_DERATE: u16 = 1000;

/// The deratings the script reads the bench's draft at before it records:
/// the whole, and the one the design's table shows beside it.
pub(super) const SCRIPT_DRAFTS: [u16; 2] = [1000, 850];

/// The batch the script runs at the fabricator.
pub(super) const SCRIPT_BATCH: u32 = 3;

/// The wait for the fabricator's run that takes longer than this, seconds,
/// is given up: a batch of three with nobody on it is 72 s from its start,
/// most of which the rest of the path has used by then.
pub(super) const RUN_LIMIT_SECS: f32 = 100.;

/// A step of the workbench stop that takes longer than this, seconds, is
/// given up, and the path goes on to the terminal with what was read.
pub(super) const BENCH_STEP_LIMIT_SECS: f32 = 12.;

/// A bought example of the module `id`: every line at store grade. `None`
/// when the data set has no such module.
pub(super) fn bought_module(items: &ItemCatalogue, id: &str) -> Option<(Fitted, String)> {
    let def = items.get(&strata::registry::Namespaced::parse(id).ok()?)?;
    let ItemKind::Module { slot, .. } = &def.kind else {
        return None;
    };
    let module = Fitted {
        item: def.id.clone(),
        batch: 0,
        by: None,
        lines: def
            .lines
            .iter()
            .map(|line| (line.id.clone(), STORE_GRADE))
            .collect(),
        condition: None,
    };
    Some((module, slot.clone()))
}

/// `--script-crafting` at the workbench, without a screen: the carbine is
/// drawn and topped up, three rounds are fired, the store's patrol
/// magazine is fitted by the request a screen would send, the weapon is
/// reloaded into it and two more fired, and the magazine is taken off
/// again. Each number the report holds is read off the `Combatant`, which
/// is what the HUD reads. The crafting that follows is [`script_craft`]'s;
/// this keeps the aim and the step's limit through it.
///
/// The shots go up and ahead, at nothing. A request is sent once a step and
/// waited on by its effect; one the sim refuses is logged with its reason
/// and sent again, so a refusal for being mid-action cures itself and any
/// other runs into the step's limit.
#[allow(
    clippy::too_many_lines,
    clippy::too_many_arguments,
    reason = "one stop, a step at a time, in the order it happens; it reads the player and the catalogue and writes the input and the request"
)]
pub(super) fn script_bench(
    time: Res<Time<Fixed>>,
    frames: Option<Res<bevy::diagnostic::FrameCount>>,
    mut script: ResMut<ProgressionScript>,
    mut bench: ResMut<CraftingScript>,
    registries: Res<strata::sim::Registries>,
    reach: Res<Reach>,
    mut interactions: MessageWriter<Interact>,
    mut inbox: ResMut<FitInbox>,
    mut answers: MessageReader<FitAnswer>,
    mut players: Query<
        (&WorldPosition, &PlayerIntent, &mut CombatIntent, &Combatant),
        With<LocalPlayer>,
    >,
    mut commands: Commands,
) {
    for answer in answers.read() {
        match &answer.outcome {
            Ok(done) => info!("script: the {} answered {done:?}", answer.mount),
            Err(refusal) => {
                warn!("script: the {} was refused: {refusal}", answer.mount);
                bench.asked = false;
            }
        }
    }
    if script.phase != Phase::Bench {
        return;
    }
    if bench.open_fitting {
        // The workbench is used as F would use it, and the path ends with
        // the screen up.
        if let Some((entity, InteractionKind::Workbench)) = reach.nearest.clone() {
            interactions.write(Interact {
                entity,
                kind: InteractionKind::Workbench,
            });
            info!("script: the fitting screen opens at the workbench");
        } else {
            warn!("script: no workbench within reach to open the fitting at");
        }
        script.advance(Phase::Sheet, frames.as_ref().map(|f| f.0));
        return;
    }
    if let Some((schematic, still)) = bench.open_crafting.clone() {
        // The same for the crafting tab, which the screen then brings to
        // the state asked for by the requests a player's presses send.
        if reach.workbench {
            strata::client::ui::crafting::open_for_still(&mut commands, &schematic, still);
            info!("script: the crafting tab opens at the workbench on {schematic}");
        } else {
            warn!("script: no workbench within reach to open the crafting tab at");
        }
        script.advance(Phase::Sheet, frames.as_ref().map(|f| f.0));
        return;
    }
    let Ok((position, movement, mut input, actor)) = players.single_mut() else {
        return;
    };
    bench.since += time.delta_secs();
    let ahead = PlayerIntent::forward(movement.yaw).as_dvec3() * 60. + bevy::math::DVec3::Y * 25.;
    *input = CombatIntent {
        aim_point: WorldPos(position.0.0 + ahead),
        ..default()
    };
    let ready = actor.action == Action::Ready;
    let full = ready && actor.ammo == actor.magazine();
    let Some((module, mount)) = bought_module(&registries.0.items, SCRIPT_MODULE) else {
        warn!("script: the data set has no {SCRIPT_MODULE}");
        script.advance(Phase::ToTerminal, frames.as_ref().map(|f| f.0));
        return;
    };
    match bench.step {
        BenchStep::Draw => {
            if ready && actor.weapon == Weapon::Carbine {
                bench.next(BenchStep::Top);
            } else {
                input.weapon = Some(0);
            }
        }
        BenchStep::Top => {
            if full {
                bench.report.bare = Some(actor.weapon_spec());
                bench.mark = actor.shots;
                bench.next(BenchStep::Fire);
            } else {
                input.reload = ready;
            }
        }
        BenchStep::Fire => {
            if actor.shots >= bench.mark + 3 {
                bench.next(BenchStep::Fit);
            } else {
                input.fire = true;
            }
        }
        BenchStep::Fit => {
            if has_parts(actor) {
                bench.report.fitted = Some(actor.weapon_spec());
                bench.report.ammo_at_fit.1 = actor.ammo;
                info!(
                    "script: fitted, the magazine holds {} and has {}",
                    actor.magazine(),
                    actor.ammo
                );
                bench.next(BenchStep::Reload);
            } else if !bench.asked && ready {
                bench.asked = true;
                bench.report.ammo_at_fit.0 = actor.ammo;
                inbox.0.push(FitRequest::Fit {
                    hand: SCRIPT_HAND,
                    mount,
                    module,
                });
            }
        }
        BenchStep::Reload => {
            if full {
                bench.report.reloaded_to = actor.ammo;
                bench.mark = actor.shots;
                bench.next(BenchStep::FireFitted);
            } else {
                input.reload = ready;
            }
        }
        BenchStep::FireFitted => {
            if actor.shots >= bench.mark + 2 {
                bench.next(BenchStep::Remove);
            } else {
                input.fire = true;
            }
        }
        BenchStep::Remove => {
            if bench.asked && !has_parts(actor) {
                bench.report.ammo_at_remove.1 = actor.ammo;
                bench.report.restored = Some(actor.weapon_spec());
                info!(
                    "script: removed, the magazine holds {} and has {}",
                    actor.magazine(),
                    actor.ammo
                );
                bench.next(BenchStep::Core);
                return;
            } else if !bench.asked && ready {
                bench.asked = true;
                bench.report.ammo_at_remove.0 = actor.ammo;
                inbox.0.push(FitRequest::Remove {
                    hand: SCRIPT_HAND,
                    mount,
                });
            }
        }
        // The rest of the stop is `script_craft`'s.
        _ => {}
    }
    if bench.since > BENCH_STEP_LIMIT_SECS {
        warn!("script: the bench stopped at {:?}", bench.step);
        bench.write();
        script.advance(
            ProgressionScript::after_bench(),
            frames.as_ref().map(|f| f.0),
        );
    }
}

/// What the bench shows of each line before Assemble: the line, its
/// floor and its ceiling.
pub(super) fn ranges(lines: &[Option<crafting::LinePreview>]) -> Vec<(String, u16, u16)> {
    lines
        .iter()
        .flatten()
        .map(|line| (line.line.clone(), line.floor, line.ceiling))
        .collect()
}

/// `--script-crafting` at the workbench once the store's magazine is off
/// again, without a screen: a cell core is assembled from the best stock
/// (the first lots), every point is spent steady on the first line with
/// room, and it is finished; the patrol magazine is assembled with that
/// core, worked the same way and finished; the magazine is fitted, and
/// stays; a second core is made from the same lots; and that core is
/// recorded as a blueprint, what the bench drafts for it read first
/// ([`drafted`]). Every step is the request a screen would send, answered
/// in the tick, and the picks and the figures are the sim's own
/// (`best_stock`, `preview`, `now`, `draft`), so the walk proves what the
/// screen will call.
///
/// A request is sent once a step and waited on by its answer; one the sim
/// refuses is logged with its reason and sent again, and a step that never
/// ends runs into `script_bench`'s limit.
#[allow(
    clippy::too_many_arguments,
    clippy::too_many_lines,
    reason = "one stop, a step at a time, in the order it happens; it reads the stock and the session and writes two kinds of request"
)]
pub(super) fn script_craft(
    frames: Option<Res<bevy::diagnostic::FrameCount>>,
    mut script: ResMut<ProgressionScript>,
    mut bench: ResMut<CraftingScript>,
    registries: Res<strata::sim::Registries>,
    (state, store): (Res<CraftState>, Res<HouseholdStore>),
    (mut asks, mut fits): (ResMut<CraftInbox>, ResMut<FitInbox>),
    mut answers: MessageReader<CraftAnswer>,
    players: Query<(&Pack, &Hands, &Combatant, Option<&CharacterSheet>), With<LocalPlayer>>,
) {
    let answered: Vec<CraftAnswer> = answers.read().cloned().collect();
    if script.phase != Phase::Bench || !bench.step.crafting() {
        return;
    }
    let Ok((pack, hands, actor, sheet)) = players.single() else {
        return;
    };
    let sheet = sheet.cloned().unwrap_or_default();
    let registries = &registries.0;
    let mut done = Vec::new();
    for answer in answered {
        match answer.outcome {
            Ok(outcome) => done.push(outcome),
            Err(refusal) => {
                warn!("script: the bench refused {:?}: {refusal}", answer.request);
                bench.asked = false;
            }
        }
    }
    let holder = Holder::of(&registries.items, hands, Some(actor));
    let id = |name: &str| strata::registry::Namespaced::parse(name).ok();
    match bench.step {
        BenchStep::Core | BenchStep::Magazine | BenchStep::Second => {
            if let Some(CraftDone::Assembled { points }) = done.first() {
                info!("script: assembled, {points} points to spend");
                let next = match bench.step {
                    BenchStep::Core => BenchStep::CoreRolls,
                    BenchStep::Magazine => BenchStep::MagazineRolls,
                    _ => BenchStep::SecondRolls,
                };
                bench.next(next);
            } else if !bench.asked {
                let name = if bench.step == BenchStep::Magazine {
                    SCRIPT_MAGAZINE
                } else {
                    SCRIPT_CORE
                };
                let Some(schematic) = id(name).and_then(|id| registries.schematics.get(&id)) else {
                    warn!("script: the data set has no {name}");
                    bench.write();
                    script.advance(
                        ProgressionScript::after_bench(),
                        frames.as_ref().map(|f| f.0),
                    );
                    return;
                };
                let mut picks = crafting::best_stock(registries, pack, Some(&store), schematic);
                // The magazine is made with the core just made, whatever
                // else the stock holds for that slot.
                if let (BenchStep::Magazine, Some(core)) = (bench.step, &bench.made) {
                    picks.insert("core".to_owned(), crafting::kind_of(core));
                }
                let shown = ranges(&crafting::preview_for(
                    registries, &holder, schematic, &picks, &sheet,
                ));
                info!("script: assembling {name}: {shown:?}");
                match bench.step {
                    BenchStep::Core => bench.report.made.core_range = shown,
                    BenchStep::Magazine => bench.report.made.magazine_range = shown,
                    _ => {}
                }
                bench.asked = true;
                asks.0.push(CraftRequest::Assemble {
                    schematic: schematic.def.id.clone(),
                    picks,
                });
            }
        }
        BenchStep::CoreRolls | BenchStep::MagazineRolls | BenchStep::SecondRolls => {
            for outcome in &done {
                if let CraftDone::Rolled(rolled) = outcome {
                    info!(
                        "script: {:?} on {}: {} to {}, {} points left",
                        rolled.outcome, rolled.line, rolled.from, rolled.to, rolled.points
                    );
                    bench
                        .report
                        .made
                        .rolled
                        .push(format!("{}: {:?}", rolled.line, rolled.outcome));
                    bench.asked = false;
                }
            }
            if bench.asked {
                return;
            }
            // Every point steady, on the first line that still has room.
            let line = state.session.as_ref().and_then(|session| {
                let open = session.lines.iter().find(|line| !line.at_ceiling())?;
                (session.points > 0).then(|| open.line.clone())
            });
            if let Some(line) = line {
                bench.asked = true;
                asks.0.push(CraftRequest::Roll {
                    line,
                    hand: Hand::Steady,
                });
            } else {
                let next = match bench.step {
                    BenchStep::CoreRolls => BenchStep::CoreFinish,
                    BenchStep::MagazineRolls => BenchStep::MagazineFinish,
                    _ => BenchStep::SecondFinish,
                };
                bench.next(next);
            }
        }
        BenchStep::CoreFinish | BenchStep::MagazineFinish | BenchStep::SecondFinish => {
            if let Some(CraftDone::Finished { product }) = done.into_iter().next() {
                info!(
                    "script: finished {} batch {}: {:?}",
                    product.item, product.batch, product.lines
                );
                let step = bench.step;
                let made = &mut bench.report.made;
                match step {
                    BenchStep::CoreFinish => made.core.clone_from(&product.lines),
                    BenchStep::MagazineFinish => made.magazine.clone_from(&product.lines),
                    _ => made.second.clone_from(&product.lines),
                }
                (made.rolls, made.batches) = (state.rolls, state.batches);
                bench.made = Some(product);
                let next = match step {
                    BenchStep::CoreFinish => BenchStep::Magazine,
                    BenchStep::MagazineFinish => BenchStep::FitMade,
                    _ => BenchStep::Record,
                };
                bench.next(next);
            } else if !bench.asked {
                // What the bench says the magazine does, read before the
                // press as a screen shows it.
                if let (BenchStep::MagazineFinish, Some(session)) = (bench.step, &state.session) {
                    let says = crafting::now(registries, &holder, session);
                    let figure = |line: &str| {
                        says.iter()
                            .find(|shown| shown.line == line)
                            .and_then(|shown| shown.means)
                            .map_or(0, |means| means.high.number)
                    };
                    bench.report.made.previewed = (figure("capacity"), figure("reload_time"));
                }
                bench.asked = true;
                asks.0.push(CraftRequest::Finish);
            }
        }
        BenchStep::FitMade => {
            if bench.asked && has_parts(actor) {
                bench.report.made.fitted = Some((actor.magazine(), actor.reload_time()));
                info!(
                    "script: the made magazine is fitted, it holds {} and reloads in {:.3} s",
                    actor.magazine(),
                    actor.reload_time()
                );
                bench.next(BenchStep::Second);
            } else if !bench.asked && actor.action == Action::Ready {
                let Some((module, (_, mount))) = bench
                    .made
                    .clone()
                    .zip(bought_module(&registries.items, SCRIPT_MODULE))
                else {
                    return;
                };
                bench.asked = true;
                fits.0.push(FitRequest::Fit {
                    hand: SCRIPT_HAND,
                    mount,
                    module: Fitted {
                        item: module.item,
                        batch: module.batch,
                        by: module.by,
                        lines: module.lines,
                        condition: module.condition,
                    },
                });
            }
        }
        BenchStep::Record => {
            if let Some(CraftDone::Recorded {
                blueprint, runs, ..
            }) = done.first()
            {
                info!("script: recorded as blueprint {blueprint}, {runs} runs");
                bench.run.blueprint = Some(*blueprint);
                bench.report.factory.blueprint = Some(*blueprint);
                bench.report.factory.runs = (*runs, *runs);
                bench.write();
                script.advance(Phase::ToFabricator, frames.as_ref().map(|f| f.0));
            } else if !bench.asked {
                let Some(core) = bench.made.clone() else {
                    return;
                };
                let drafts: Vec<DraftReport> = SCRIPT_DRAFTS
                    .iter()
                    .filter_map(|derate| {
                        drafted(registries, &state, pack, &store, (&core, &sheet), *derate)
                    })
                    .collect();
                for draft in &drafts {
                    info!("script: the blueprint as drafted: {draft:?}");
                }
                bench.report.factory.drafts = drafts;
                bench.report.factory.prototype = core.batch;
                bench.asked = true;
                asks.0.push(CraftRequest::Record {
                    batch: core.batch,
                    derate: SCRIPT_DERATE,
                });
            }
        }
        // The fitting and the firing before all this are `script_bench`'s.
        _ => {}
    }
}

/// What the bench drafts for `prototype` at `derate` per-mille, as a
/// screen would show it before Record: the lines, what each slot asks,
/// which of the stock in reach meets it and the largest batch it feeds.
/// None when the sim would not draft it, which the request then says.
pub(super) fn drafted(
    registries: &GameRegistries,
    craft: &CraftState,
    pack: &Pack,
    store: &HouseholdStore,
    (prototype, sheet): (&Record, &CharacterSheet),
    derate: u16,
) -> Option<DraftReport> {
    let by = prototype.by.as_deref().unwrap_or_default();
    let draft = blueprint::draft_for(registries, craft, prototype, derate, (by, sheet)).ok()?;
    let needs = draft
        .slots
        .iter()
        .flat_map(|need| {
            need.needs.iter().map(|asked| {
                (
                    need.slot.id.clone(),
                    asked.stat.clone(),
                    asked.at_least,
                    asked.at_most,
                )
            })
        })
        .collect();
    let stock = draft
        .slots
        .iter()
        .flat_map(|need| {
            factory::feeds(registries, pack, Some(store), &draft, &need.slot.id)
                .into_iter()
                .map(|feed| {
                    let name = registries
                        .items
                        .get(&feed.kind.item)
                        .map_or_else(|| feed.kind.item.to_string(), |def| def.name.clone());
                    let lot = match &feed.kind.origin {
                        Some(origin) => format!("{} {}", origin.root, name.to_lowercase()),
                        None => name,
                    };
                    (
                        need.slot.id.clone(),
                        lot,
                        feed.held(),
                        feed.verdict.err().map(|why| why.to_string()),
                    )
                })
        })
        .collect();
    Some(DraftReport {
        derate,
        most: factory::limit(registries, pack, Some(store), &draft).most,
        lines: draft.lines,
        needs,
        stock,
    })
}

/// `--script-crafting` at the fabricator and at the path's end, without a
/// screen. At the fabricator a batch of [`SCRIPT_BATCH`] of the blueprint
/// the bench recorded is started from the cheapest stock that meets it, by
/// the request a screen would send, with nobody on it; the path then goes
/// on to the deposit, the locker and the terminal while the fabricator runs.
/// When the swap is done the run is waited for, and what it came to goes
/// into the report beside the workbench's.
///
/// What the fabricator says is heard in every phase, since it finishes
/// when it finishes, and the ticks are counted from the start to that.
#[allow(
    clippy::too_many_arguments,
    reason = "one stop and one wait; it reads the stock and the blueprints and both ends of a request"
)]
pub(super) fn script_factory(
    frames: Option<Res<bevy::diagnostic::FrameCount>>,
    mut script: ResMut<ProgressionScript>,
    mut bench: ResMut<CraftingScript>,
    registries: Res<strata::sim::Registries>,
    reach: Res<Reach>,
    (state, prints, store, sim): (
        Res<FactoryState>,
        Res<Blueprints>,
        Res<HouseholdStore>,
        Res<strata::sim::SimClock>,
    ),
    mut asks: ResMut<FactoryInbox>,
    mut answers: MessageReader<FactoryAnswer>,
    mut notices: MessageReader<FactoryNotice>,
    players: Query<(&Pack, Option<&CharacterSheet>), With<LocalPlayer>>,
) {
    let frame = frames.as_ref().map(|f| f.0);
    let registries = &registries.0;
    for answer in answers.read() {
        bench.run.asked = false;
        match &answer.outcome {
            Ok(FactoryDone::Started { batch, seconds }) => {
                info!("script: the run is on, batch {batch}, ready in {seconds} s");
                bench.run.started = true;
                bench.run.ticks = 0;
                bench.report.factory.batch = *batch;
            }
            Ok(done) => info!("script: the fabricator answered {done:?}"),
            Err(refusal) => warn!("script: the fabricator refused: {refusal}"),
        }
    }
    // Counted before the finish is heard, so the tick it was made on is
    // one of them.
    if bench.run.started && bench.run.finished.is_none() {
        bench.run.ticks += 1;
    }
    for notice in notices.read() {
        info!("script: the fabricator says {notice:?}");
        if let FactoryNotice::Finished { made, .. } = notice {
            bench.run.finished = Some(*made);
        }
    }
    let print = bench
        .run
        .blueprint
        .and_then(|id| Some((id, prints.0.get(&id)?)));
    match script.phase {
        Phase::Factory => {
            if bench.run.started {
                script.advance(ProgressionScript::after_bench(), frame);
            } else if script.since > BENCH_STEP_LIMIT_SECS {
                warn!("script: no run was started at the fabricator");
                bench.write();
                script.advance(ProgressionScript::after_bench(), frame);
            } else if !bench.run.asked
                && reach.fabricator
                && let Some((id, print)) = print
                && let Ok((pack, sheet)) = players.single()
                && let Some(rules) = registries.schematics.factory()
            {
                let most = factory::limit(registries, pack, Some(&store), print).most;
                let batch = SCRIPT_BATCH.min(most);
                let feeds = factory::cheapest(registries, pack, Some(&store), print, batch);
                let from: Vec<String> = feeds
                    .iter()
                    .map(|(slot, kinds)| {
                        let lots: Vec<&str> = kinds
                            .iter()
                            .map(|kind| kind.origin.as_ref().map_or("stock", |o| o.root.as_str()))
                            .collect();
                        format!("{slot}: {}", lots.join(", "))
                    })
                    .collect();
                info!(
                    "script: the stock feeds {most}; starting {batch} from {}",
                    from.join("; ")
                );
                // At the pace of the member who starts it, as the run will go.
                let pace =
                    sheet.and_then(|sheet| factory::pace_of(rules, &registries.progression, sheet));
                let paced = factory::paced(rules, pace);
                let promised =
                    |worked: bool| factory::seconds(&paced, print.complexity, batch, worked);
                let report = &mut bench.report.factory;
                (report.most, report.size) = (most, batch);
                report.promised_secs = (promised(true), promised(false));
                bench.run.asked = true;
                asks.0.push(FactoryRequest::Start {
                    blueprint: id,
                    batch,
                    feeds,
                    worker: None,
                });
            }
        }
        Phase::Run => {
            let over = bench.run.finished.is_some() || state.job.is_none();
            if !over && script.since <= RUN_LIMIT_SECS {
                return;
            }
            if !over {
                warn!("script: the fabricator's run never finished");
            }
            let (ticks, made) = (bench.run.ticks, bench.run.finished.unwrap_or(0));
            let report = &mut bench.report.factory;
            report.made = made;
            // The script's ticks are the sim's steps, which combat play
            // takes sixty of a second.
            report.played_secs = f64::from(ticks) * sim.0.step;
            if let Some((_, print)) = print {
                report.runs.1 = print.runs_left;
            }
            info!(
                "script: the run made {} in {:.1} s of play",
                report.made, report.played_secs
            );
            script.advance(Phase::ToBoard, frame);
        }
        _ => {}
    }
}

/// The box that makes someone the household's smith: the one that teaches
/// the carbine's schematic.
pub(super) const SCRIPT_SMITH_BOX: &str = "weaponsmith.frames.1";

/// The schematic the smith makes after the sale.
pub(super) const SCRIPT_CARBINE: &str = "veyra:schematic.patrol_carbine";

/// The mount the rework is after: the first the made carbine's moddability
/// leaves shut.
const SCRIPT_MOUNT: &str = "stock";

/// The blanks the smith will buy at most.
const SCRIPT_BLANKS: u32 = 4;

/// The line the smith spends every point on.
pub(super) const SCRIPT_CARBINE_LINE: &str = "moddability";

/// The arrival, who the household starts with.
pub(super) const ARRIVAL: usize = 0;

/// The mounts of the carbine `record` that are open.
fn open_mounts(items: &ItemCatalogue, record: &Record) -> (u32, u32) {
    let Some(ItemKind::Weapon { mounts, .. }) = items.get(&record.item).map(|def| &def.kind) else {
        return (0, 0);
    };
    let open = mounts
        .iter()
        .filter(|mount| items.mount_opens(record, &mount.slot) == Some(Ok(())))
        .count();
    (
        u32::try_from(open).unwrap_or(u32::MAX),
        u32::try_from(mounts.len()).unwrap_or(u32::MAX),
    )
}

/// Switch who is out to `index`, as `--script-progression` does its own
/// swaps: directly, with no terminal and no screen.
fn switch_to(
    household: &mut Household,
    index: usize,
    registries: &GameRegistries,
    (encounter, run): (Option<&CombatEncounter>, Option<&FactoryState>),
    player: &mut bevy::ecs::query::QueryItem<'_, '_, PlayerParts>,
) -> Result<(), String> {
    swap_to(
        household,
        index,
        registries,
        encounter,
        run.and_then(|run| run.job.as_ref()),
        out!(*player),
    )
}

/// `--script-crafting`'s smith: the first time the path is at the
/// workbench the household's smith (whoever has [`SCRIPT_SMITH_BOX`], and
/// joins for it) is switched to directly, so every act that follows is
/// theirs; after the sale the path comes back, the smith makes a patrol
/// carbine from the store's metal with every point on moddability and
/// carries it to the locker, where it goes to the household's store, the
/// arrival is switched to directly and takes it in hand by the request
/// the stores screen would send. The last act: what it came to goes into
/// the report.
///
/// A request is sent once a step and waited on by its answer; one the sim
/// refuses is logged and sent again, and a stop that never ends is given
/// up at its limit.
#[allow(
    clippy::too_many_arguments,
    clippy::too_many_lines,
    clippy::type_complexity,
    reason = "two stops, a step at a time, in the order they happen; it reads the stock and the session and writes three kinds of request and the household"
)]
pub(super) fn script_carbine(
    frames: Option<Res<bevy::diagnostic::FrameCount>>,
    mut script: ResMut<ProgressionScript>,
    mut bench: ResMut<CraftingScript>,
    mut household: ResMut<Household>,
    registries: Res<strata::sim::Registries>,
    (encounter, run): (Option<Res<CombatEncounter>>, Option<Res<FactoryState>>),
    (state, store, open, stable): (
        Res<CraftState>,
        Res<HouseholdStore>,
        Res<OpenStock>,
        Res<strata::sim::ride::Stable>,
    ),
    (mut asks, mut clicks, mut takes): (ResMut<CraftInbox>, ResMut<StockInbox>, ResMut<EquipInbox>),
    (mut bench_heard, mut equip_heard): (MessageReader<CraftAnswer>, MessageReader<EquipAnswer>),
    (mut reworks, mut rework_heard, mut xp_heard): (
        ResMut<ReworkInbox>,
        MessageReader<ReworkAnswer>,
        MessageReader<XpGained>,
    ),
    mut players: Query<PlayerParts, With<LocalPlayer>>,
) {
    let frame = frames.as_ref().map(|f| f.0);
    for gained in xp_heard.read() {
        if gained.kind == XpKind::Fabrication {
            bench.report.carbine.credits.push(gained.amount);
        }
    }
    let reworked: Vec<_> = rework_heard.read().map(|a| a.result.clone()).collect();
    let registries = &registries.0;
    let items = &registries.items;
    let done: Vec<CraftDone> = bench_heard
        .read()
        .filter_map(|answer| match &answer.outcome {
            Ok(done) => Some(done.clone()),
            Err(refusal) if script.phase == Phase::Carbine => {
                warn!("script: the bench refused {:?}: {refusal}", answer.request);
                bench.carbine.asked = false;
                None
            }
            Err(_) => None,
        })
        .collect();
    let took: Vec<_> = equip_heard
        .read()
        .map(|answer| answer.result.clone())
        .collect();
    let Ok(mut player) = players.single_mut() else {
        return;
    };
    let switch = |household: &mut Household, index: usize, player: &mut _| {
        switch_to(
            household,
            index,
            registries,
            (encounter.as_deref(), run.as_deref()),
            player,
        )
    };
    // The smith takes the bench for the stop after the lesson, unless the
    // path is only opening a screen there for a still.
    if script.phase == Phase::Bench && !bench.carbine.smith_out {
        bench.carbine.smith_out = true;
        if bench.open_fitting || bench.open_crafting.is_some() {
            return;
        }
        while household.index_with(SCRIPT_SMITH_BOX).is_none() {
            if household.join_next().is_none() {
                break;
            }
        }
        if let Some(index) = household.index_with(SCRIPT_SMITH_BOX) {
            match switch(&mut household, index, &mut player) {
                Ok(()) => info!("script: the smith is out for the bench"),
                Err(reason) => warn!("script: the smith could not come out: {reason}"),
            }
        } else {
            warn!("script: nobody in the household is a smith");
        }
        return;
    }
    if !matches!(script.phase, Phase::Carbine | Phase::Handover) {
        return;
    }
    if script.since > 2. * BENCH_STEP_LIMIT_SECS {
        warn!("script: the carbine stopped at {:?}", bench.carbine.step);
        bench.write();
        script.advance(Phase::Done, frame);
        return;
    }
    let id = |name: &str| strata::registry::Namespaced::parse(name).ok();
    match bench.carbine.step {
        CarbineStep::Smith => {
            let Some(index) = household.index_with(SCRIPT_SMITH_BOX) else {
                warn!("script: nobody in the household is a smith");
                script.advance(Phase::Done, frame);
                return;
            };
            if household.active == index {
                bench.carbine.step = CarbineStep::Assemble;
            } else if let Err(reason) = switch(&mut household, index, &mut player) {
                warn!("script: the smith could not come out: {reason}");
                script.advance(Phase::Done, frame);
            }
        }
        CarbineStep::Assemble => {
            if let Some(CraftDone::Assembled { points }) = done.first() {
                let ceiling = state
                    .session
                    .as_ref()
                    .and_then(|session| {
                        session
                            .lines
                            .iter()
                            .find(|line| line.line == SCRIPT_CARBINE_LINE)
                    })
                    .map_or(0, |line| line.ceiling);
                info!(
                    "script: the carbine is assembled, {points} points, moddability allows {ceiling}"
                );
                bench.report.carbine.points = *points;
                bench.report.carbine.ceiling = ceiling;
                bench.carbine.asked = false;
                bench.carbine.step = CarbineStep::Rolls;
            } else if !bench.carbine.asked {
                let Some(schematic) =
                    id(SCRIPT_CARBINE).and_then(|id| registries.schematics.get(&id))
                else {
                    warn!("script: the data set has no {SCRIPT_CARBINE}");
                    script.advance(Phase::Done, frame);
                    return;
                };
                let picks = crafting::best_stock(registries, &player.6, Some(&store), schematic);
                bench.carbine.asked = true;
                asks.0.push(CraftRequest::Assemble {
                    schematic: schematic.def.id.clone(),
                    picks,
                });
            }
        }
        CarbineStep::Rolls => {
            for outcome in &done {
                if let CraftDone::Rolled(rolled) = outcome {
                    info!(
                        "script: {:?} on {}: {} to {}, {} points left",
                        rolled.outcome, rolled.line, rolled.from, rolled.to, rolled.points
                    );
                    bench.carbine.asked = false;
                }
            }
            if bench.carbine.asked {
                return;
            }
            let more = state.session.as_ref().is_some_and(|session| {
                session.points > 0
                    && session
                        .lines
                        .iter()
                        .any(|line| line.line == SCRIPT_CARBINE_LINE && !line.at_ceiling())
            });
            if more {
                bench.carbine.asked = true;
                asks.0.push(CraftRequest::Roll {
                    line: SCRIPT_CARBINE_LINE.to_owned(),
                    hand: Hand::Steady,
                });
            } else {
                bench.carbine.step = CarbineStep::Finish;
            }
        }
        CarbineStep::Finish => {
            if let Some(CraftDone::Finished { product }) = done.into_iter().next() {
                info!(
                    "script: finished {} batch {} by {:?}: {:?}",
                    product.item, product.batch, product.by, product.lines
                );
                let bought = items.get(&product.item).map_or(0, |def| {
                    let bought = Record {
                        lines: def
                            .lines
                            .iter()
                            .map(|line| (line.id.clone(), STORE_GRADE))
                            .collect(),
                        batch: 0,
                        by: None,
                        ..product.clone()
                    };
                    open_mounts(items, &bought).0
                });
                let report = &mut bench.report.carbine;
                report.batch = product.batch;
                report.by.clone_from(&product.by);
                report.lines = product.lines.clone().into_iter().collect();
                report.open = open_mounts(items, &product);
                report.bought_open = bought;
                bench.carbine.product = Some(product);
                bench.carbine.asked = false;
                bench.carbine.step = CarbineStep::Lift;
            } else if !bench.carbine.asked {
                bench.carbine.asked = true;
                asks.0.push(CraftRequest::Finish);
            }
        }
        CarbineStep::Lift => {
            match took.into_iter().next() {
                Some(Ok(equipped)) => {
                    info!("script: the made carbine is in hand {}", equipped.hand);
                    bench.carbine.asked = false;
                    bench.carbine.step = CarbineStep::Blank;
                    return;
                }
                Some(Err(refusal)) => {
                    warn!("script: the made carbine was not taken to rework: {refusal}");
                    bench.carbine.asked = false;
                    bench.carbine.step = CarbineStep::Open;
                    script.advance(Phase::ToLockerAgain, frame);
                    return;
                }
                None => {}
            }
            if !bench.carbine.asked
                && let Some(kind) = bench.carbine.product.clone()
            {
                bench.carbine.own = player
                    .7
                    .0
                    .get(SCRIPT_HAND)
                    .and_then(|stack| items::read(stack, items).ok())
                    .map(|record| crafting::kind_of(&record));
                bench.carbine.asked = true;
                takes.0.push(EquipRequest::Take {
                    kind,
                    hand: SCRIPT_HAND,
                });
            }
        }
        CarbineStep::Blank => {
            let hand = player
                .7
                .0
                .get(SCRIPT_HAND)
                .and_then(|stack| items::read(stack, items).ok());
            let stock_open = hand
                .as_ref()
                .is_some_and(|record| items.mount_opens(record, SCRIPT_MOUNT) == Some(Ok(())));
            if stock_open || bench.carbine.blanks >= SCRIPT_BLANKS {
                bench.carbine.asked = false;
                bench.carbine.step = CarbineStep::Return;
                return;
            }
            if matches!(done.first(), Some(CraftDone::Bought { .. })) {
                bench.carbine.asked = false;
                bench.carbine.step = CarbineStep::Rework;
                return;
            }
            let blank = registries.rework.as_ref().map(|rules| rules.blank.clone());
            if !bench.carbine.asked
                && let Some(item) = blank
            {
                bench.carbine.asked = true;
                bench.carbine.blanks += 1;
                asks.0.push(CraftRequest::Buy { item, count: 1 });
            }
        }
        CarbineStep::Rework => {
            match reworked.into_iter().next() {
                Some(Ok(done)) => {
                    info!(
                        "script: reworked, {:?}: moddability {} to {}",
                        done.outcome, done.from, done.to
                    );
                    bench.report.carbine.reworks.push((
                        format!("{:?}", done.outcome),
                        done.from,
                        done.to,
                    ));
                    bench.carbine.asked = false;
                    bench.carbine.step = CarbineStep::Blank;
                    return;
                }
                Some(Err(refusal)) => {
                    warn!("script: the rework was refused: {refusal}");
                    bench.carbine.asked = false;
                    bench.carbine.blanks = SCRIPT_BLANKS;
                    bench.carbine.step = CarbineStep::Blank;
                    return;
                }
                None => {}
            }
            if !bench.carbine.asked {
                bench.carbine.asked = true;
                reworks.0.push(ReworkRequest::Rework { hand: SCRIPT_HAND });
            }
        }
        CarbineStep::Return => {
            if let Some(result) = took.into_iter().next() {
                if let Err(refusal) = result {
                    warn!("script: the smith's own carbine was not taken back: {refusal}");
                }
                // The made carbine lies in the pack as it now reads.
                let made = bench
                    .carbine
                    .product
                    .as_ref()
                    .map(|p| (p.item.clone(), p.batch));
                let found = player
                    .6
                    .cells
                    .slots()
                    .iter()
                    .flatten()
                    .filter_map(|stack| items::read(stack, items).ok())
                    .find(|record| made.as_ref() == Some(&(record.item.clone(), record.batch)));
                if let Some(found) = found {
                    bench.carbine.product = Some(crafting::kind_of(&found));
                }
                bench.carbine.asked = false;
                bench.carbine.step = CarbineStep::Open;
                script.advance(Phase::ToLockerAgain, frame);
                return;
            }
            if !bench.carbine.asked {
                bench.carbine.asked = true;
                if let Some(kind) = bench.carbine.own.clone() {
                    takes.0.push(EquipRequest::Take {
                        kind,
                        hand: SCRIPT_HAND,
                    });
                } else {
                    bench.carbine.step = CarbineStep::Open;
                    script.advance(Phase::ToLockerAgain, frame);
                }
            }
        }
        CarbineStep::Open | CarbineStep::Reopen => {
            let next = if bench.carbine.step == CarbineStep::Open {
                CarbineStep::Send
            } else {
                CarbineStep::Take
            };
            if open
                .0
                .as_ref()
                .is_some_and(|opened| opened.menu == SCRIPT_MENU)
            {
                bench.carbine.asked = false;
                bench.carbine.step = next;
            } else if !bench.carbine.asked {
                bench.carbine.asked = true;
                clicks.0.push(StockRequest::Open {
                    menu: SCRIPT_MENU,
                    with_store: true,
                });
            }
        }
        CarbineStep::Send => {
            let (pack, hands) = (&player.6, &player.7);
            let Some(opened) = open.0.as_ref().filter(|opened| opened.menu == SCRIPT_MENU) else {
                return;
            };
            let kind = bench.carbine.product.as_ref().map(crafting::kind_of);
            let layout = locker_layout(items, pack, &stable);
            let cell = pack.cells.slots().iter().position(|slot| {
                slot.as_ref()
                    .and_then(|stack| items::read(stack, items).ok())
                    .is_some_and(|record| Some(crafting::kind_of(&record)) == kind)
            });
            let click = cell.and_then(|cell| {
                let slot = slotted_model::SlotIx(layout.pack.start + u16::try_from(cell).ok()?);
                let action = slotted_model::ClickAction::QuickMove { slot };
                let mut copy = (Pack::clone(pack), Hands::clone(hands), store.clone());
                let mut menu = opened.state.clone();
                let mut saddle = stable.saddle.clone();
                let mut stock = Stock {
                    saddle: locker_saddle(&stable, &mut saddle),
                    pack: &mut copy.0,
                    hands: &mut copy.1,
                    store: Some(&mut copy.2),
                };
                let predicted = items::apply(items, &mut stock, &mut menu, action)
                    .map_err(|refusal| warn!("script: the send was refused: {refusal}"))
                    .ok()?;
                Some(StockRequest::Click {
                    menu: SCRIPT_MENU,
                    action,
                    predicted,
                })
            });
            if let Some(click) = click {
                clicks.0.push(click);
            } else {
                warn!("script: no carbine in the pack to send to the store");
            }
            bench.carbine.step = CarbineStep::Close;
        }
        CarbineStep::Close => {
            clicks.0.push(StockRequest::Close { menu: SCRIPT_MENU });
            bench.carbine.asked = false;
            bench.carbine.step = CarbineStep::Arrive;
        }
        CarbineStep::Arrive => {
            let stored = bench.carbine.product.as_ref().is_some_and(|product| {
                store
                    .0
                    .slots()
                    .iter()
                    .flatten()
                    .filter_map(|stack| items::read(stack, items).ok())
                    .any(|record| crafting::kind_of(&record) == crafting::kind_of(product))
            });
            if !stored {
                return;
            }
            match switch(&mut household, ARRIVAL, &mut player) {
                Ok(()) => {
                    info!("script: the arrival is out to take the carbine");
                    bench.carbine.step = CarbineStep::Reopen;
                }
                Err(reason) => {
                    warn!("script: the arrival could not come out: {reason}");
                    bench.write();
                    script.advance(Phase::Done, frame);
                }
            }
        }
        CarbineStep::Take => {
            match took.into_iter().next() {
                Some(Ok(equipped)) => {
                    let name = household.characters[household.active].name.clone();
                    info!(
                        "script: {name} has {} batch {} in hand {}",
                        equipped.item.item, equipped.item.batch, equipped.hand
                    );
                    bench.report.carbine.in_hand_of = name;
                    clicks.0.push(StockRequest::Close { menu: SCRIPT_MENU });
                    bench.carbine.began = script.since;
                    bench.carbine.step = CarbineStep::Settle;
                    return;
                }
                Some(Err(refusal)) => {
                    warn!("script: the carbine was not taken: {refusal}");
                    bench.write();
                    script.advance(Phase::Done, frame);
                    return;
                }
                None => {}
            }
            if !bench.carbine.asked
                && let Some(kind) = bench.carbine.product.clone()
            {
                bench.carbine.asked = true;
                takes.0.push(EquipRequest::Take {
                    kind,
                    hand: SCRIPT_HAND,
                });
            }
        }
        CarbineStep::Settle => {
            // The household copies the hands a few frames after the sim
            // changes them; the path ends once the file has had them.
            if script.since - bench.carbine.began > 0.5 {
                bench.write();
                script.advance(Phase::Done, frame);
            }
        }
    }
}

/// The buyer the script sells to.
const SCRIPT_BUYER: &str = "veyra:buyer.pelagic_works";
/// The line it sells the run's cores to.
const SCRIPT_LINE: &str = "cell_cores";

/// `--script-crafting`, the last act: the cores the fabricator made go to
/// the cell core line by the request the board would send, and what it
/// paid goes into the report, with the player stood at the board.
#[allow(
    clippy::too_many_arguments,
    reason = "the script, its report, the stock and both ends of a request"
)]
pub(super) fn script_sale(
    frames: Option<Res<bevy::diagnostic::FrameCount>>,
    mut script: ResMut<ProgressionScript>,
    mut bench: ResMut<CraftingScript>,
    registries: Res<strata::sim::Registries>,
    (store, purse): (Res<HouseholdStore>, Res<Purse>),
    mut asks: ResMut<strata::sim::procurement::ProcurementInbox>,
    mut answers: MessageReader<strata::sim::procurement::SaleAnswer>,
) {
    if script.phase != Phase::Sell {
        return;
    }
    let frame = frames.as_ref().map(|f| f.0);
    let items = &registries.0.items;
    if let Some(answer) = answers.read().next() {
        let report = bench.report.sale.get_or_insert_with(SaleReport::default);
        match &answer.result {
            Ok(sold) => {
                info!("script: sold {} for {} cr", sold.count, sold.paid);
                report.count = sold.count;
                report.paid = sold.paid;
                report.purse.1 = purse.0;
            }
            Err(refusal) => warn!("script: the sale was refused: {refusal:?}"),
        }
        bench.run.asked = false;
        bench.write();
        script.advance(ProgressionScript::after_sale(), frame);
        return;
    }
    if bench.run.asked {
        return;
    }
    if script.since > BENCH_STEP_LIMIT_SECS {
        warn!("script: no cores were sold");
        bench.write();
        script.advance(ProgressionScript::after_sale(), frame);
        return;
    }
    let batch = bench.report.factory.batch;
    let cores = store
        .0
        .slots()
        .iter()
        .flatten()
        .filter_map(|stack| items::read(stack, items).ok())
        .find(|record| {
            record.batch == batch && record.batch != 0 && record.item.path().ends_with("cell_core")
        });
    let Some(kind) = cores else {
        return;
    };
    let grade = kind.lines.get("charge_density").copied().unwrap_or(0);
    info!("script: the run's cores carry charge density {grade}");
    bench.report.sale = Some(SaleReport {
        batch,
        grade,
        purse: (purse.0, 0),
        lines: kind.lines.clone().into_iter().collect(),
        ..SaleReport::default()
    });
    bench.run.asked = true;
    let Ok(buyer) = strata::registry::Namespaced::parse(SCRIPT_BUYER) else {
        return;
    };
    asks.0
        .push(strata::sim::procurement::ProcurementRequest::Sell {
            buyer,
            line: SCRIPT_LINE.to_owned(),
            count: kind.count,
            kind,
        });
}

/// The step at the deposit that takes longer than this, seconds, is given
/// up: the ping's channel and three sampling cycles are some fifteen seconds
/// of play, and the player is carried to a place it has never been.
pub(super) const SURVEY_LIMIT_SECS: f32 = 90.;

/// Seconds the script waits after carrying the player to a deposit before
/// it pings: the sim has ticked with the player there.
pub(super) const SURVEY_SETTLE_SECS: f32 = 1.;

/// Frames the script waits after its stop before it reads the pack and
/// carries the player home.
pub(super) const SURVEY_STOP_FRAMES: u8 = 4;

/// The units the script samples before it stops: more than the old draw
/// of ten, which a sampling cycle's few units do not hit exactly.
pub(super) const SAMPLE_UNITS: u32 = 10;

/// The menu id the script opens the stores under at the locker.
pub(super) const SCRIPT_MENU: slotted_model::MenuId = slotted_model::MenuId(0x5c21_9700);

/// Units of the lot of `deposit` in `inventory`.
pub(super) fn units_of(
    inventory: &slotted_model::Inventory,
    deposit: DepositId,
    items: &ItemCatalogue,
) -> u32 {
    inventory
        .slots()
        .iter()
        .flatten()
        .filter_map(|stack| items::read(stack, items).ok())
        .filter(|record| {
            record
                .origin
                .as_ref()
                .is_some_and(|o| o.deposit == deposit.0)
        })
        .map(|record| record.count)
        .sum()
}

/// `--script-crafting` after the workbench, without a screen: the player
/// is carried to the middle of the nearest deposit, where it reads
/// strongest, pings its class with the tool in the pack and samples by the
/// requests the survey screen would send until it has [`SAMPLE_UNITS`], and
/// stops. The lot is read in the pack with the purse, the player is carried
/// back, walks to the locker (`drive_script`) and sends the lot to the
/// household's store by the click the stores screen would send. What it
/// read goes into the report beside the workbench's.
///
/// The player is carried and not walked because the nearest deposit is
/// hundreds of metres off, which a scripted run has no need to cover
/// (`--script-survey` walks and finds one). A character over ground that
/// has not streamed holds still, so the carrying waits on the world and no
/// step is taken on air.
#[allow(
    clippy::too_many_arguments,
    clippy::too_many_lines,
    clippy::type_complexity,
    reason = "one stop, a step at a time, in the order it happens; it reads the deposits, the purse and the stock and writes the player's place and its requests"
)]
pub(super) fn script_survey(
    frames: Option<Res<bevy::diagnostic::FrameCount>>,
    mut script: ResMut<ProgressionScript>,
    mut bench: ResMut<CraftingScript>,
    registries: Res<strata::sim::Registries>,
    near: Res<NearDeposits>,
    origin: Res<RenderOrigin>,
    (purse, store, open, stable): (
        Res<Purse>,
        Res<HouseholdStore>,
        Res<OpenStock>,
        Res<strata::sim::ride::Stable>,
    ),
    (mut asks, mut clicks): (ResMut<SurveyInbox>, ResMut<StockInbox>),
    (mut pings, mut samples): (MessageReader<PingAnswer>, MessageReader<SampleAnswer>),
    mut players: Query<
        (
            &mut WorldPosition,
            &mut strata::physics::avian3d::prelude::Position,
            &CharacterController,
            &Pack,
            &Hands,
        ),
        With<LocalPlayer>,
    >,
) {
    let pinged: Vec<PingAnswer> = pings.read().cloned().collect();
    let cycles: Vec<SampleAnswer> = samples.read().copied().collect();
    if !matches!(
        script.phase,
        Phase::ToPing | Phase::Sample | Phase::ToLocker | Phase::Stow
    ) {
        return;
    }
    let Ok((mut position, mut body, controller, pack, hands)) = players.single_mut() else {
        return;
    };
    let frame = frames.as_ref().map(|f| f.0);
    let items = &registries.0.items;
    let set = &registries.0.resources;
    let here = position.0;
    // Carried: the sim's position and the body's, which the character's
    // step reads, are both set, feet a short drop over the place.
    let mut carry = |to: WorldPos| {
        position.0 = to;
        body.0 = origin.to_render(to);
    };
    let pack_stacks = |pack: &Pack| {
        u32::try_from(pack.cells.slots().iter().flatten().count()).unwrap_or(u32::MAX)
    };
    let walk = &mut bench.survey;
    match script.phase {
        Phase::ToPing => {
            let Some(target) = &walk.target else {
                let by_distance = |a: &&Deposit, b: &&Deposit| {
                    a.distance_m(here.x(), here.z())
                        .total_cmp(&b.distance_m(here.x(), here.z()))
                };
                let nearest = near.deposits.iter().min_by(by_distance);
                match nearest {
                    Some(deposit) => {
                        info!(
                            "script: carried to the middle of {} ({}), {:.0} m off",
                            deposit.place,
                            deposit.lot_name(set),
                            deposit.distance_m(here.x(), here.z())
                        );
                        walk.home = Some(here);
                        walk.target = Some(deposit.clone());
                        carry(WorldPos::new(
                            deposit.at[0],
                            f64::from(deposit.height_m)
                                + crate::PLAYER_DROP
                                + f64::from(controller.half_height()),
                            deposit.at[1],
                        ));
                    }
                    None if script.since > SURVEY_LIMIT_SECS => {
                        warn!("script: no deposit is near; the survey stop is skipped");
                        script.advance(Phase::ToTerminal, frame);
                    }
                    None => {}
                }
                return;
            };
            if script.since > SURVEY_SETTLE_SECS {
                info!("script: at the middle of {}", target.place);
                script.advance(Phase::Sample, frame);
            }
        }
        Phase::Sample => {
            let Some(target) = walk.target.clone() else {
                script.advance(Phase::ToTerminal, frame);
                return;
            };
            let mut gave_up = false;
            if walk.stage == 0 {
                walk.before = (
                    purse.0,
                    units_of(&store.0, target.id, items),
                    pack_stacks(pack),
                );
                asks.0.push(SurveyRequest::Ping {
                    class: set.class_of(target.ty).id.clone(),
                });
                walk.stage = 1;
            }
            for answer in &pinged {
                match &answer.outcome {
                    Ok(PingDone::Met(result)) if walk.stage == 1 => {
                        if result.found.iter().any(|met| met.deposit.id == target.id) {
                            info!("script: the ping met {}", target.place);
                            asks.0.push(SurveyRequest::Sample);
                            walk.stage = 2;
                        } else {
                            warn!("script: the ping did not meet {}", target.place);
                            gave_up = true;
                            break;
                        }
                    }
                    Err(refusal) => {
                        warn!("script: the ping was refused: {refusal}");
                        gave_up = true;
                        break;
                    }
                    _ => {}
                }
            }
            for answer in &cycles {
                match answer.outcome {
                    Ok(SampleDone::Cycle { total, .. }) if walk.stage == 2 => {
                        walk.got = total;
                        if total >= SAMPLE_UNITS {
                            info!("script: sampled {total} units of {}", target.lot_name(set));
                            asks.0.push(SurveyRequest::Stop);
                            walk.stage = 3;
                        }
                    }
                    Err(refusal) => {
                        warn!("script: the sample was refused: {refusal}");
                        gave_up = true;
                        break;
                    }
                    _ => {}
                }
            }
            if gave_up {
                if let Some(home) = walk.home {
                    carry(home);
                }
                script.advance(Phase::ToTerminal, frame);
            } else if walk.stage == 3 && walk.stopped < SURVEY_STOP_FRAMES {
                // The stop is consumed in the next tick; wait it out so the
                // carrying home is not a move that ends a sampling.
                walk.stopped += 1;
            } else if walk.stage == 3 {
                let lot = pack
                    .cells
                    .slots()
                    .iter()
                    .flatten()
                    .filter_map(|stack| items::read(stack, items).ok())
                    .find(|record| {
                        record
                            .origin
                            .as_ref()
                            .is_some_and(|o| o.deposit == target.id.0)
                    });
                walk.drawn = Some(DrawReport {
                    deposit: target.id,
                    place: target.place.clone(),
                    figures: target.figures.clone(),
                    lot_lines: lot
                        .as_ref()
                        .map(|lot| lot.lines.iter().map(|(k, v)| (k.clone(), *v)).collect())
                        .unwrap_or_default(),
                    units: (walk.got, lot.as_ref().map_or(0, |lot| lot.count)),
                    purse_before: walk.before.0,
                    purse_after: purse.0,
                    store_before: walk.before.1,
                    store_after: walk.before.1,
                    pack_before: walk.before.2,
                    pack_after: pack_stacks(pack),
                });
                if let Some(home) = walk.home {
                    carry(home);
                }
                script.advance(Phase::ToLocker, frame);
            } else if script.since > SURVEY_LIMIT_SECS {
                warn!("script: nothing was sampled at {}", target.place);
                if let Some(home) = walk.home {
                    carry(home);
                }
                script.advance(Phase::ToTerminal, frame);
            }
        }
        Phase::Stow => {
            let Some(target) = walk.target.clone() else {
                script.advance(Phase::ToTerminal, frame);
                return;
            };
            let opened = open.0.as_ref().filter(|opened| opened.menu == SCRIPT_MENU);
            match (walk.stow, opened) {
                (0, _) => {
                    clicks.0.push(StockRequest::Open {
                        menu: SCRIPT_MENU,
                        with_store: true,
                    });
                    walk.stow = 1;
                }
                (1, Some(opened)) => {
                    // The click the screen would send, predicted as the
                    // screen predicts it: the same rules on a copy.
                    let layout = locker_layout(items, pack, &stable);
                    let held = pack.cells.slots().iter().position(|slot| {
                        slot.as_ref()
                            .and_then(|stack| items::read(stack, items).ok())
                            .and_then(|record| record.origin)
                            .is_some_and(|o| o.deposit == target.id.0)
                    });
                    let click = held.and_then(|cell| {
                        let slot =
                            slotted_model::SlotIx(layout.pack.start + u16::try_from(cell).ok()?);
                        let action = slotted_model::ClickAction::QuickMove { slot };
                        let mut copy = (pack.clone(), hands.clone(), store.clone());
                        let mut state = opened.state.clone();
                        let mut saddle = stable.saddle.clone();
                        let mut stock = Stock {
                            saddle: locker_saddle(&stable, &mut saddle),
                            pack: &mut copy.0,
                            hands: &mut copy.1,
                            store: Some(&mut copy.2),
                        };
                        let predicted = items::apply(items, &mut stock, &mut state, action)
                            .map_err(|refusal| warn!("script: the send was refused: {refusal}"))
                            .ok()?;
                        Some(StockRequest::Click {
                            menu: SCRIPT_MENU,
                            action,
                            predicted,
                        })
                    });
                    if let Some(click) = click {
                        clicks.0.push(click);
                    } else {
                        warn!("script: no lot in the pack to send to the store");
                    }
                    walk.stow = 2;
                }
                (2, _) => {
                    if let Some(drawn) = walk.drawn.as_mut() {
                        drawn.store_after = units_of(&store.0, target.id, items);
                        drawn.pack_after = pack_stacks(pack);
                        info!(
                            "script: the store holds {} units of {} (it held {})",
                            drawn.store_after,
                            target.lot_name(set),
                            drawn.store_before
                        );
                    }
                    clicks.0.push(StockRequest::Close { menu: SCRIPT_MENU });
                    let drawn = bench.survey.drawn.clone();
                    bench.report.draw = drawn;
                    bench.write();
                    script.advance(Phase::ToTerminal, frame);
                }
                _ if script.since > BENCH_STEP_LIMIT_SECS => {
                    warn!("script: the stores never opened at the locker");
                    let drawn = bench.survey.drawn.clone();
                    bench.report.draw = drawn;
                    bench.write();
                    script.advance(Phase::ToTerminal, frame);
                }
                _ => {}
            }
        }
        // The walk to the locker between the two is `drive_script`'s.
        _ => {}
    }
}
