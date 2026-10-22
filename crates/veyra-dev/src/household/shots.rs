//! Set-ups that exist only for screenshots (`--factory-run`,
//! `--factory-poor-lot`, `--join-all`) and the function that installs them.
//! Separate so that nothing a played game runs sits among them.

use super::*;

/// `--factory-run`: where the starting of the run stands.
#[derive(Resource, Debug, Default)]
pub struct FactoryShot {
    /// A request is with the sim.
    pub asked: bool,
    /// The sim started the run.
    pub started: bool,
    /// The run is on, or with `--factory-finish` has finished, or cannot
    /// be.
    pub done: bool,
}

/// `--factory-poor-lot`: where the stocking of the lot stands.
#[derive(Resource, Debug, Default)]
pub struct PoorLotShot {
    /// The lot is in the store, or there is none to put there.
    pub done: bool,
}

/// How many units `--factory-poor-lot` stocks: enough to read as a lot.
pub const POOR_LOT_UNITS: u32 = 60;

/// The frame `--factory-poor-lot` gives up on: the deposits round the
/// spawn are listed long before it.
pub const POOR_LOT_LAST_FRAME: u32 = 900;

/// `--factory-poor-lot`: once the first blueprint is in reach and the
/// deposits round the player are listed, the lot a draw would give of the
/// first of them that fits a slot of the blueprint and does not meet it
/// goes into the store, so a still of the factory shows one refused in
/// place. Which lots those are is the sim's to say (`crafting::fits`,
/// `factory::meets`); nothing is stocked when none is.
#[allow(
    clippy::too_many_arguments,
    reason = "the clock, the blueprints and the deposits the sim would judge, what the flag waits on, and the store"
)]
pub fn poor_lot_for_shot(
    frames: Res<bevy::diagnostic::FrameCount>,
    registries: Res<strata::sim::Registries>,
    prints: Res<Blueprints>,
    near: Option<Res<NearDeposits>>,
    first: Option<Res<crate::stills::CraftFirst>>,
    players: Query<&Pack, With<LocalPlayer>>,
    mut store: ResMut<HouseholdStore>,
    mut shot: ResMut<PoorLotShot>,
) {
    if shot.done || frames.0 < FACTORY_SHOT_FRAME || first.is_some_and(|first| !first.done) {
        return;
    }
    let late = frames.0 > POOR_LOT_LAST_FRAME;
    let registries = &registries.0;
    let found = players.single().ok().and_then(|pack| {
        blueprint::held(registries, pack, Some(&store), &prints)
            .first()
            .and_then(|(id, _)| prints.0.get(id))
    });
    let (Some(print), Some(near)) = (found, near.filter(|near| !near.deposits.is_empty())) else {
        if late {
            warn!("--factory-poor-lot: no blueprint in reach, or no deposit listed");
            shot.done = true;
        }
        return;
    };
    let items = &registries.items;
    let poor = print.slots.iter().find_map(|need| {
        near.deposits.iter().find_map(|deposit| {
            let lot = deposits::lot(deposit, &registries.resources, POOR_LOT_UNITS)?;
            let refused = factory::meets(need, &lot).err()?;
            crafting::fits(items, &need.slot, &lot).then_some((lot, refused))
        })
    });
    if let Some((lot, refused)) = poor {
        let placed = items::write(&lot, items)
            .is_ok_and(|stack| items::place_in(items, &mut store.0, stack).is_none());
        if placed {
            info!("--factory-poor-lot: stocked {} ({refused})", lot.item);
        } else {
            warn!("--factory-poor-lot: no room in the store for it");
        }
    } else {
        warn!("--factory-poor-lot: every lot near here meets the blueprint");
    }
    shot.done = true;
}

/// `--factory-full-store`: every empty cell of the store is taken by one
/// of what its first stack holds, put there and not merged, so the run
/// just started finds no room for its first unit and stands. A still's
/// stock, made from nothing: the flag is refused on the played household.
pub fn crowd_store(store: &mut HouseholdStore) {
    let Some(mut filler) = store.0.slots().iter().flatten().next().cloned() else {
        warn!("--factory-full-store: the store holds nothing to fill it with");
        return;
    };
    filler.count = 1;
    let empty: Vec<usize> = (0..store.0.len())
        .filter(|cell| store.0.get(*cell).is_none())
        .collect();
    info!("--factory-full-store: {} cells taken", empty.len());
    for cell in empty {
        store.0.set(cell, Some(filler.clone()));
    }
    // The dirty mask is the sync layer's and part of an inventory's
    // equality; no mirror is open to read it.
    store.0.clear_changed();
}

/// `--join-all`: where the joining stands.
#[derive(Resource, Debug, Default)]
pub struct JoinShot {
    /// Everyone who could has joined.
    pub done: bool,
}

/// `--join-all`: everyone who can joins the bench before the still's
/// screen opens, as at a terminal, so a still of the bench has more than
/// one member on it.
pub fn join_all_for_shot(
    frames: Res<bevy::diagnostic::FrameCount>,
    mut household: ResMut<Household>,
    mut shot: ResMut<JoinShot>,
) {
    if shot.done || frames.0 < FACTORY_SHOT_FRAME {
        return;
    }
    join_all(&mut household);
    shot.done = true;
}

/// The frame `--factory-run` starts on: the world is up and the household
/// has handed its stock to the sim.
pub const FACTORY_SHOT_FRAME: u32 = 40;

/// `--factory-run`: a run is started before the still's screen opens, by
/// the request a screen would send and the sim's own choices: the first
/// blueprint in reach, the batch asked for or the most the stock feeds if
/// that is fewer, the cheapest stock that meets it. With `--factory-worker`
/// the first on the bench who is not out works it, the first recruit
/// joining when the bench has nobody else; the request waits until the sim
/// has been told who is on the bench, and until `--craft-first` has made
/// and recorded its thing. One the sim refuses ends it, said in the log.
/// With `--factory-finish` it is done when the run is, in its own played
/// time.
#[allow(
    clippy::too_many_arguments,
    reason = "the flag, the clock, the stock and the blueprints the sim would pick from, the bench, the run, and the two ends of a request"
)]
pub fn factory_run_for_shot(
    cli: Res<crate::DevArgs>,
    frames: Res<bevy::diagnostic::FrameCount>,
    registries: Res<strata::sim::Registries>,
    (prints, mut store, benched): (Res<Blueprints>, ResMut<HouseholdStore>, Res<Benched>),
    (run, first): (Res<FactoryState>, Option<Res<crate::stills::CraftFirst>>),
    players: Query<&Pack, With<LocalPlayer>>,
    mut household: ResMut<Household>,
    mut inbox: ResMut<FactoryInbox>,
    mut answers: MessageReader<FactoryAnswer>,
    mut shot: ResMut<FactoryShot>,
) {
    for answer in answers.read() {
        shot.asked = false;
        if cli.factory_full_store && matches!(answer.outcome, Ok(FactoryDone::Started { .. })) {
            crowd_store(&mut store);
        }
        match &answer.outcome {
            Ok(FactoryDone::Started { .. }) if cli.factory_finish => {
                info!("--factory-run: {:?}", answer.outcome);
                shot.started = true;
            }
            Ok(done) => {
                info!("--factory-run: {done:?}");
                shot.done = true;
            }
            Err(refusal) => {
                warn!("--factory-run: {refusal}");
                shot.done = true;
            }
        }
    }
    if shot.started && !shot.done && run.job.is_none() {
        info!("--factory-finish: the run is finished");
        shot.done = true;
    }
    if shot.done
        || shot.started
        || shot.asked
        || frames.0 < FACTORY_SHOT_FRAME
        || first.is_some_and(|first| !first.done)
    {
        return;
    }
    let (Some(wanted), Ok(pack)) = (cli.factory_run, players.single()) else {
        return;
    };
    let registries = &registries.0;
    let found = blueprint::held(registries, pack, Some(&store), &prints)
        .first()
        .and_then(|(id, _)| Some((*id, prints.0.get(id)?)));
    let Some((id, print)) = found else {
        warn!("--factory-run: no blueprint is in the pack or the store");
        shot.done = true;
        return;
    };
    let worker = if cli.factory_worker {
        let active = household.active;
        let on_bench = household
            .characters
            .iter()
            .enumerate()
            .find(|(index, _)| *index != active)
            .map(|(_, character)| character.name.clone());
        let Some(name) = on_bench.or_else(|| {
            let index = household.join_next()?;
            Some(household.characters[index].name.clone())
        }) else {
            warn!("--factory-worker: nobody is on the bench and nobody can join");
            shot.done = true;
            return;
        };
        // The sim holds a worker to the bench as it was last told it.
        if !benched.0.contains(&name) {
            return;
        }
        Some(name)
    } else {
        None
    };
    let most = factory::limit(registries, pack, Some(&store), print).most;
    let batch = wanted.min(most);
    info!("--factory-run: blueprint {id}, {batch} of the {most} the stock feeds");
    inbox.push(FactoryRequest::Start {
        blueprint: id,
        batch,
        feeds: factory::cheapest(registries, pack, Some(&store), print, batch),
        worker,
    });
    shot.asked = true;
}

/// `--board-stock` and `--board-sell`: where the still's stocking and sale
/// stand.
#[derive(Resource, Debug, Default)]
pub struct BoardShot {
    /// The cores are in the store, or `--board-stock` was not asked.
    pub stocked: bool,
    /// The sale was sent.
    sold: bool,
}

/// How many frames after the board opens `--board-sell` sends the sale:
/// the screen has been built and the view read.
pub const BOARD_SELL_AFTER: u32 = 6;

/// The cores `--board-stock` puts in the store.
pub const BOARD_CORES: u32 = 100;

/// `--board-stock`: 100 cell cores of batch 412 by Tovan Reyes at charge
/// density 724 and stability 600 go into the store, so the board has
/// something to quote. `--board-sell`: a few frames after the board opens,
/// the first row's sale is sent, by the request its button holds. A still's
/// stock made from nothing, so both are refused on the played household.
#[allow(
    clippy::too_many_lines,
    reason = "the stocking and the sale are two steps of one still's set-up"
)]
pub fn board_for_shot(
    cli: Res<crate::DevArgs>,
    frames: Res<bevy::diagnostic::FrameCount>,
    registries: Res<strata::sim::Registries>,
    board: Res<veyra_ui::board::BoardUi>,
    mut store: ResMut<HouseholdStore>,
    mut inbox: ResMut<strata::sim::procurement::ProcurementInbox>,
    mut shot: ResMut<BoardShot>,
) {
    if !shot.stocked {
        if frames.0 < FACTORY_SHOT_FRAME {
            return;
        }
        shot.stocked = true;
        if cli.board_stock {
            let items = &registries.0.items;
            let core = Record {
                item: strata::registry::Namespaced::parse("veyra:component.cell_core")
                    .unwrap_or_else(|_| unreachable!("a literal id")),
                count: BOARD_CORES,
                batch: 412,
                by: Some("Tovan Reyes".to_owned()),
                lines: [("charge_density", 724), ("stability", 600)]
                    .into_iter()
                    .map(|(line, figure)| (line.to_owned(), figure))
                    .collect(),
                condition: None,
                fitted: BTreeMap::new(),
                origin: None,
            };
            let placed = items::write(&core, items)
                .is_ok_and(|stack| items::place_in(items, &mut store.0, stack).is_none());
            if placed {
                info!("--board-stock: {BOARD_CORES} cell cores of batch 412 in the store");
            } else {
                warn!("--board-stock: no room in the store for them");
            }
        }
        return;
    }
    if !cli.board_sell || shot.sold || frames.0 < crate::stills::OPEN_PAUSE_FRAME + BOARD_SELL_AFTER
    {
        return;
    }
    let request = board
        .view()
        .and_then(|view| view.lines().next())
        .and_then(|line| line.request.clone());
    shot.sold = true;
    if let Some(request) = request {
        info!("--board-sell: {request:?}");
        inbox.push(request);
    } else {
        warn!("--board-sell: the first row cannot sell");
    }
}

/// What a still of the factory is set up by before its screen opens:
/// `--factory-run`'s run, `--factory-poor-lot`'s lot and `--join-all`'s
/// bench. The script walks to the fabricator and the terminal itself and
/// takes none of them.
pub fn install_factory_shots(app: &mut App, cli: &crate::DevArgs) {
    if cli.script_progression {
        return;
    }
    if cli.join_all {
        app.init_resource::<JoinShot>().add_systems(
            Update,
            join_all_for_shot
                .run_if(resource_exists::<Household>)
                .run_if(in_state(AppState::InWorld)),
        );
    }
    if cli.open_board {
        app.init_resource::<BoardShot>().add_systems(
            Update,
            board_for_shot
                .run_if(resource_exists::<Household>)
                .run_if(in_state(AppState::InWorld)),
        );
    }
    if cli.factory_run.is_some() {
        app.init_resource::<FactoryShot>().add_systems(
            Update,
            factory_run_for_shot
                .run_if(resource_exists::<Household>)
                .run_if(in_state(AppState::InWorld)),
        );
    }
    if cli.factory_poor_lot {
        app.init_resource::<PoorLotShot>().add_systems(
            Update,
            poor_lot_for_shot
                .run_if(resource_exists::<Household>)
                .run_if(in_state(AppState::InWorld)),
        );
    }
}
