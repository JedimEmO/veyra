//! The household while the game runs: copying the sim's sheet and stock
//! into the file, telling the sim who is benched, saving on change, and the
//! switch, join and trainer lessons that answer the client's bench screen.

use super::*;

/// How much played time passes between writes of the clock alone, seconds: a
/// clock saved every tick would write the file every tick.
const PLAYED_SAVE_S: f64 = 60.;

/// The local player's sheet is the active character's: every change to it
/// (a credit, a lesson, a forget on the sheet screen) is copied to the
/// household, which saves it. The swap writes the same sheet both ways,
/// so the copy is a no-op there.
pub(super) fn track_sheet(
    players: Query<&CharacterSheet, (With<LocalPlayer>, Changed<CharacterSheet>)>,
    mut household: ResMut<Household>,
) {
    let Ok(sheet) = players.single() else {
        return;
    };
    let active = household.active;
    if household.characters[active].sheet != *sheet {
        household.characters[active].sheet = sheet.clone();
    }
}

/// The local player's bar is the active character's: the sim places a
/// newly learned ability and takes a forgotten one out, and a screen's
/// request moves one; every change is copied to the household, which saves
/// it. The swap writes the bar both ways, so the copy is a no-op there.
pub(super) fn track_bar(
    players: Query<&Bar, (With<LocalPlayer>, Changed<Bar>)>,
    mut household: ResMut<Household>,
) {
    let Ok(bar) = players.single() else {
        return;
    };
    let active = household.active;
    if household.characters[active].bar != *bar {
        household.characters[active].bar = bar.clone();
    }
}

/// The local player's pack and weapons are the active character's and the
/// sim's store is the household's: every change the sim makes to them (a
/// click consumed in the tick, a close that put the cursor away) is copied
/// here, which saves it. The saved forms are compared first, so the
/// household changes only when the stock did and a tick that touched a
/// component without moving anything writes no file. The purse and the
/// survey record are the sim's in the same way (a lot drawn, a post stood
/// at, a deposit tracked) and are copied the same way, and so is the
/// bench's work (a session opened, a roll, a batch finished), which is how
/// a roll stands once it is made: this runs before [`save_on_change`], and
/// that before the screen hears the sim's answer, so the counter a roll was
/// drawn from is in the file before its outcome is shown. The blueprints
/// and the fabricator's run are the sim's too (a blueprint recorded, a run
/// started, a unit made, which is once a unit and never once a tick).
#[allow(
    clippy::too_many_arguments,
    clippy::too_many_lines,
    reason = "everything of the sim's that the household saves, and the household"
)]
pub(super) fn track_stock(
    players: Query<(Ref<Pack>, Ref<Hands>), With<LocalPlayer>>,
    store: Res<HouseholdStore>,
    registries: Res<strata::sim::Registries>,
    purse: Option<Res<Purse>>,
    surveyed: Option<Res<Surveyed>>,
    live: Option<ResMut<SurveyLive>>,
    harvesters: Option<Res<Harvesters>>,
    (clock, cleared, caches): (
        Option<Res<PlayedClock>>,
        Option<Res<Cleared>>,
        Option<Res<strata::sim::loot::Caches>>,
    ),
    books: (
        Option<Res<Deeds>>,
        Option<Res<CommonsTaken>>,
        Option<Res<Leases>>,
    ),
    bench: Option<Res<CraftState>>,
    (jobs, known): (
        Option<Res<strata::sim::postings::Jobs>>,
        Option<Res<strata::sim::postings::Known>>,
    ),
    (prints, run, sales, stable): (
        Option<Res<Blueprints>>,
        Option<Res<FactoryState>>,
        Option<Res<ProcurementState>>,
        Option<Res<strata::sim::ride::Stable>>,
    ),
    mut household: ResMut<Household>,
) {
    let items = &registries.0.items;
    let active = household.active;
    let character = &household.characters[active];
    if let Ok((pack, hands)) = players.single()
        && (pack.is_changed() || hands.is_changed())
        && let Some((pack, hands)) = character.stock_of(&pack, &hands, items)
        && (character.pack != pack || character.hands != hands)
    {
        let character = &mut household.characters[active];
        character.pack = pack;
        character.hands = hands;
    }
    if store.is_changed() {
        match store.saved(items) {
            Ok(saved) if household.store != saved => household.store = saved,
            Ok(_) => {}
            Err(error) => error!("household: the store does not read: {error}"),
        }
    }
    if let Some(purse) = purse
        && purse.is_changed()
        && household.purse != purse.0
    {
        household.purse = purse.0;
    }
    if let Some(bench) = bench
        && bench.is_changed()
        && household.crafting != *bench
    {
        household.crafting = bench.clone();
    }
    if let Some(prints) = prints
        && prints.is_changed()
        && household.blueprints != *prints
    {
        household.blueprints = prints.clone();
    }
    if let Some(run) = run
        && run.is_changed()
        && household.factory != *run
    {
        household.factory = run.clone();
    }
    if let Some(sales) = sales
        && sales.is_changed()
        && household.procurement != *sales
    {
        household.procurement = sales.clone();
    }
    if let Some(stable) = stable
        && stable.is_changed()
    {
        match stable.saved(items) {
            Ok(saved) if household.stable != saved => household.stable = saved,
            Ok(_) => {}
            Err(error) => error!("household: the stable does not read: {error}"),
        }
    }
    // The planted machines are of the survey record's world: copied only
    // when it is live, and stamped with it ([`SurveyLive`]).
    if let (Some(machines), Some(live)) = (harvesters, live.as_deref())
        && machines.is_changed()
    {
        if household.harvesters != *machines {
            household.harvesters = machines.clone();
        }
        if !machines.list.is_empty() && household.surveyed_in != Some(live.world) {
            household.surveyed_in = Some(live.world);
        }
    }
    track_camps(
        &mut household,
        clock.as_deref().copied(),
        cleared,
        live.as_deref(),
    );
    super::cache::track_caches(&mut household, caches, live.as_deref());
    super::cache::track_jobs(&mut household, jobs, known, live.as_deref());
    track_deeds(
        &mut household,
        books,
        live.as_deref(),
        clock.as_deref().copied(),
    );
    // Only a record of this world is the sim's to change ([`SurveyLive`]).
    if let (Some(surveyed), Some(mut live)) = (surveyed, live)
        && surveyed.is_changed()
    {
        let tracked = surveyed.tracked.as_ref().map(|deposit| deposit.id);
        // The old list of posts stood at is converted into the found at
        // load and goes from the file with the first record of today.
        if household.found != surveyed.found {
            household.found.clone_from(&surveyed.found);
        }
        if !household.visited.is_empty() && !surveyed.found.is_empty() {
            household.visited.clear();
        }
        // What the sim tracks is copied when the sim changed it, not when
        // it differs from the file: an id the world could not find at the
        // start was handed over as none, and is not a release.
        if live.tracked != tracked {
            live.tracked = tracked;
            if household.tracked != tracked {
                household.tracked = tracked;
            }
        }
        let recorded = !household.found.is_empty() || household.tracked.is_some();
        if recorded && household.surveyed_in != Some(live.world) {
            household.surveyed_in = Some(live.world);
        }
    }
}

/// The played clock and the camps cleared, copied into the household. The
/// camps are of the survey record's world and stamped with it
/// ([`SurveyLive`]). The clock rides along with them and is otherwise
/// written once a played minute, so a save is not made every tick. It takes
/// the resource itself and reads through `Deref` only, so the household is
/// marked changed (and saved) when a value really differs and not on every
/// run.
fn track_camps(
    household: &mut ResMut<Household>,
    now: Option<PlayedClock>,
    cleared: Option<Res<Cleared>>,
    live: Option<&SurveyLive>,
) {
    if let Some(now) = now
        && now.seconds - household.played.seconds >= PLAYED_SAVE_S
    {
        household.played = now;
    }
    if let (Some(cleared), Some(now), Some(live)) = (cleared, now, live)
        && cleared.is_changed()
    {
        if household.cleared != *cleared {
            household.cleared.clone_from(&cleared);
            household.played = now;
        }
        if !cleared.at.is_empty() && household.surveyed_in != Some(live.world) {
            household.surveyed_in = Some(live.world);
        }
    }
}

/// Who may be put on a fabricator's run, for the sim: everyone on the
/// bench but whoever is out, by name. The sim holds a worker to this list
/// and knows nothing else of the household.
pub(super) fn publish_benched(household: Res<Household>, mut benched: ResMut<Benched>) {
    let names: Vec<String> = household
        .characters
        .iter()
        .enumerate()
        .filter(|(index, _)| *index != household.active)
        .map(|(_, character)| character.name.clone())
        .collect();
    if benched.0 != names {
        benched.0 = names;
    }
}

/// The household as the bench screen shows it, whenever it changes. The
/// view is the client's resource; it exists only with the UI wired. Whoever
/// is working the fabricator's run has it on their card with the count so
/// far, which is the sim's and moves once a unit.
pub(super) fn publish_bench(
    household: Res<Household>,
    run: Option<Res<FactoryState>>,
    clock: Option<Res<PlayedClock>>,
    mut view: ResMut<BenchView>,
) {
    let job = run.as_deref().and_then(|run| run.job.as_ref());
    let now = clock.as_deref().map(|clock| clock.seconds);
    let card = |c: &Character| BenchCard {
        injured_min: now.and_then(|now| super::fall::injured_minutes(c, now)),
        name: c.name.clone(),
        tint: c.tint,
        weapon: c.weapon,
        sheet: c.sheet.clone(),
        work: job
            .filter(|job| job.worker.as_deref() == Some(c.name.as_str()))
            .map(|job| BenchWork {
                made: job.made,
                size: job.size,
            }),
    };
    let next = BenchView {
        characters: household.characters.iter().map(card).collect(),
        active: household.active,
        recruits: household.recruits.iter().map(card).collect(),
        capacity: BENCH,
    };
    if *view != next {
        *view = next;
    }
}

/// Set when the file on disk could not be read or copied aside: nothing
/// is written this run, so whatever it holds is still there afterwards.
#[derive(Resource, Debug, Default, Clone, Copy)]
pub(super) struct SaveHeld;

/// Write the file whenever the household changed: after the two systems
/// that copy the sim's changes into it, so a change is written in the frame
/// it is seen in and not the next.
pub(super) fn save_on_change(
    household: Res<Household>,
    file: Res<HouseholdFile>,
    held: Option<Res<SaveHeld>>,
) {
    if !household.is_changed() || held.is_some() {
        return;
    }
    if let Err(error) = household.save(&file.path) {
        error!("household: saving to {}: {error}", file.path.display());
    }
}

/// The local player as the household sees it, queried.
pub(super) type PlayerParts<'w> = (
    &'w WorldPosition,
    &'w mut Combatant,
    &'w mut CharacterSheet,
    &'w mut CombatIntent,
    &'w mut Name,
    &'w mut BodyTint,
    &'w mut Pack,
    &'w mut Hands,
    &'w mut Bar,
);

/// The local player as the household writes it: the parts of
/// [`PlayerParts`] borrowed out of their query item.
pub(super) struct Out<'a> {
    pub(super) position: WorldPos,
    pub(super) actor: &'a mut Combatant,
    pub(super) sheet: &'a mut CharacterSheet,
    pub(super) input: &'a mut CombatIntent,
    pub(super) name: &'a mut Name,
    pub(super) tint: &'a mut BodyTint,
    pub(super) pack: &'a mut Pack,
    pub(super) hands: &'a mut Hands,
    pub(super) bar: &'a mut Bar,
}

/// Borrow the query item's parts for one change.
macro_rules! out {
    ($item:expr) => {{
        let (position, actor, sheet, input, name, tint, pack, hands, bar) = &mut $item;
        Out {
            position: position.0,
            actor: &mut **actor,
            sheet: &mut **sheet,
            input: &mut **input,
            name: &mut **name,
            tint: &mut **tint,
            pack: &mut **pack,
            hands: &mut **hands,
            bar: &mut **bar,
        }
    }};
}

/// Everyone who can join does, at a terminal. Free in this slice.
pub(super) fn join_all(household: &mut Household) -> Vec<String> {
    let mut joined = Vec::new();
    while let Some(index) = household.join_next() {
        let name = household.characters[index].name.clone();
        info!(
            "household: {name} joined the bench ({} of {BENCH})",
            index + 1
        );
        joined.push(name);
    }
    joined
}

/// Why a switch to `name`, who is working `job`, is refused, as the bench
/// toasts it: the sentence is the client's (`ui/strings.rs`), where every
/// word a player reads is, and the count so far is filled in here.
pub(super) fn working_refusal(name: &str, job: &factory::Job) -> String {
    strata::client::ui::strings::english("strata.bench.refused.working")
        .unwrap_or_default()
        .replace("{name}", name)
        .replace("{made}", &job.made.to_string())
        .replace("{size}", &job.size.to_string())
}

/// Switch who is out to `index`: the active character's sheet, weapon,
/// ammo, pack and position are written back, the new one's applied to the
/// local player with full pools, and the body and the HUD follow. Each
/// character carries their own pack and weapons, so what the one going in
/// has on them goes in with them, a stack left on the cursor included.
/// Refused while fighting (an action under way, or hostiles engaged), for
/// an index off the bench, and for whoever is working the fabricator's
/// `run`: they are at the fabricator until the run ends or is stopped
/// (the progression design's "Run the workshop"). The character arrives
/// where the player stands: the walk-in of the design is future work.
pub(super) fn swap_to(
    household: &mut Household,
    index: usize,
    registries: &GameRegistries,
    encounter: Option<&CombatEncounter>,
    run: Option<&factory::Job>,
    player: Out<'_>,
) -> Result<(), String> {
    if index >= household.characters.len() {
        return Err(format!("no character {index} on the bench"));
    }
    if index == household.active {
        return Err(format!(
            "{} is already out",
            household.characters[index].name
        ));
    }
    let engaged = encounter.is_some_and(|e| e.engaged && e.remaining > 0);
    if player.actor.in_combat(engaged) {
        return Err("not while fighting".to_owned());
    }
    if let Some(job) = run
        && job.worker.as_deref() == Some(household.characters[index].name.as_str())
    {
        return Err(working_refusal(&household.characters[index].name, job));
    }
    put_out(household, index, registries, player);
    Ok(())
}

/// The switch itself, once it is allowed: the active character's sheet,
/// weapon, ammo, pack and position are written back and the new one's
/// applied to the local player with full pools. A fall puts the next
/// member out by this without the fighting refusal, which a fall cannot
/// meet.
pub(super) fn put_out(
    household: &mut Household,
    index: usize,
    registries: &GameRegistries,
    player: Out<'_>,
) {
    let Out {
        position,
        actor,
        sheet,
        input,
        name,
        tint,
        pack,
        hands,
        bar,
    } = player;
    let active = household.active;
    let leaving = &mut household.characters[active];
    leaving.take_from(actor, sheet, position);
    leaving.bar.clone_from(bar);
    if let Some(stock) = leaving.stock_of(pack, hands, &registries.items) {
        (leaving.pack, leaving.hands) = stock;
    }
    let next = &household.characters[index];
    *actor = next.actor(&registries.progression, &registries.items);
    *sheet = next.sheet.clone();
    bar.clone_from(&next.bar);
    (*pack, *hands) = next.carried(&registries.items);
    *input = CombatIntent::default();
    name.set(next.name.clone());
    tint.0 = next.tint;
    household.active = index;
    info!(
        "household: {} is out (was {}), {:?} in hand",
        next.name, household.characters[active].name, next.weapon
    );
}

/// Learn the first box of `trees` the sheet can take, in graph order.
/// Within reach of the trainer is given, since the trainer answered. The
/// actor is refitted at once, so the box is felt on the next tick.
pub(super) fn learn_at_trainer(
    trees: &[String],
    graph: &SkillGraph,
    reach: &Reach,
    player: Out<'_>,
    household: &mut Household,
) -> Result<String, String> {
    let Out { actor, sheet, .. } = player;
    let mut refusals = Vec::new();
    for tree in trees.iter().filter_map(|id| graph.tree(id)) {
        for b in tree.boxes() {
            match sheet.learn(graph, &b.id, reach.teaches(graph, &b.id)) {
                Ok(()) => {
                    actor.fit(graph, sheet);
                    let active = household.active;
                    household.characters[active].sheet = sheet.clone();
                    info!(
                        "household: learned {} ({} points, {} free)",
                        b.id,
                        b.cost,
                        sheet.points_free(graph)
                    );
                    return Ok(b.id.clone());
                }
                Err(LearnRefused::AlreadyLearned(_) | LearnRefused::MissingPrerequisite(_)) => {}
                Err(refused) => refusals.push(format!("{}: {refused}", b.id)),
            }
        }
    }
    Err(if refusals.is_empty() {
        "nothing to learn here".to_owned()
    } else {
        refusals.join("; ")
    })
}

/// A recruit joins the bench, if there is room; their name.
pub(super) fn join(household: &mut Household, index: usize) -> Result<String, String> {
    if index >= household.recruits.len() {
        return Err("nobody to join".to_owned());
    }
    if household.characters.len() >= BENCH {
        return Err(format!("the bench holds {BENCH}; it is full"));
    }
    let character = household.recruits.remove(index);
    let name = character.name.clone();
    household.characters.push(character);
    info!(
        "household: {name} joined the bench ({} of {BENCH})",
        household.characters.len()
    );
    Ok(name)
}

/// The bench screen's buttons: a switch, behind the client's fade, or a
/// join. Each is answered with a line the client toasts. The screen only
/// lights a button at a terminal and out of combat, but the rule is the
/// game's: a request from anywhere is refused away from a terminal, and
/// the swap checks the fighting again, because a tick may have passed.
#[allow(
    clippy::too_many_arguments,
    reason = "the request and its answer, the household, and the four things a switch is held to"
)]
pub(super) fn answer_bench(
    mut requests: MessageReader<BenchRequest>,
    mut outcomes: MessageWriter<BenchOutcome>,
    mut household: ResMut<Household>,
    registries: Res<strata::sim::Registries>,
    encounter: Option<Res<CombatEncounter>>,
    run: Option<Res<FactoryState>>,
    clock: Option<Res<PlayedClock>>,
    reach: Res<Reach>,
    mut players: Query<PlayerParts, With<LocalPlayer>>,
    riders: Query<Entity, With<LocalPlayer>>,
    mut commands: Commands,
) {
    for request in requests.read() {
        let at_terminal = matches!(reach.nearest, Some((_, InteractionKind::Terminal)));
        let outcome = if at_terminal {
            match *request {
                BenchRequest::Switch(index) => {
                    let Ok(mut player) = players.single_mut() else {
                        continue;
                    };
                    let hurt = clock.as_deref().and_then(|clock| {
                        super::fall::injured_refusal(
                            household.characters.get(index)?,
                            clock.seconds,
                        )
                    });
                    if let Some(reason) = hurt {
                        info!("household: refused: {reason}");
                        outcomes.write(BenchOutcome {
                            ok: false,
                            message: reason,
                        });
                        continue;
                    }
                    swap_to(
                        &mut household,
                        index,
                        &registries.0,
                        encounter.as_deref(),
                        run.as_deref().and_then(|run| run.job.as_ref()),
                        out!(player),
                    )
                    .map(|()| {
                        // The incoming character is not on the bike.
                        if let Ok(rider) = riders.single() {
                            commands
                                .entity(rider)
                                .remove::<(strata::sim::ride::Riding, strata::sim::ride::Calling)>(
                                );
                        }
                        format!("{} is out", household.active().name)
                    })
                }
                BenchRequest::Join(index) => {
                    join(&mut household, index).map(|name| format!("{name} joined the bench"))
                }
            }
        } else {
            Err("not at a terminal".to_owned())
        };
        let (ok, message) = match outcome {
            Ok(message) => (true, message),
            Err(reason) => {
                info!("household: refused: {reason}");
                (false, reason)
            }
        };
        outcomes.write(BenchOutcome { ok, message });
    }
}

/// The deeds, the commons' tallies and the leases, copied into the
/// household when the sim changed them and they differ, and stamped with the
/// world like the camps. Like [`track_camps`] it reads the household through
/// `Deref` until a value really differs.
fn track_deeds(
    household: &mut ResMut<Household>,
    (deeds, commons, leases): (
        Option<Res<Deeds>>,
        Option<Res<CommonsTaken>>,
        Option<Res<Leases>>,
    ),
    live: Option<&SurveyLive>,
    now: Option<PlayedClock>,
) {
    let Some(live) = live else {
        return;
    };
    let mut kept = false;
    if let Some(deeds) = deeds
        && deeds.is_changed()
    {
        kept |= !deeds.by.is_empty();
        if household.deeds != *deeds {
            household.deeds.clone_from(&deeds);
            // The clock goes with it so a reload cannot run a lease long.
            if let Some(now) = now {
                household.played = now;
            }
        }
    }
    if let Some(commons) = commons
        && commons.is_changed()
    {
        kept |= !commons.by.is_empty();
        if household.commons != *commons {
            household.commons.clone_from(&commons);
            // The clock goes with it so a reload cannot run a lease long.
            if let Some(now) = now {
                household.played = now;
            }
        }
    }
    if let Some(leases) = leases
        && leases.is_changed()
    {
        kept |= !leases.until.is_empty();
        if household.leases != *leases {
            household.leases.clone_from(&leases);
            // The clock goes with it so a reload cannot run a lease long.
            if let Some(now) = now {
                household.played = now;
            }
        }
    }
    if kept && household.surveyed_in != Some(live.world) {
        household.surveyed_in = Some(live.world);
    }
}
