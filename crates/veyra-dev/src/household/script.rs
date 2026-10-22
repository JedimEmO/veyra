//! `--script-progression`: the headless walk that fights, learns, swaps
//! and saves, and `--script-hurt`. Separate from the crafting walks because
//! it is the spine they extend.

use super::*;

/// Where `--script-progression` is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Phase {
    /// The combat script fights the encounter (melee first, for blade XP).
    Fight,
    /// Walking to the blade trainer.
    ToTrainer,
    /// Asked the trainer; waiting a tick to see the sheet.
    Learn,
    /// `--script-crafting`: walking to the workbench.
    ToWorkbench,
    /// `--script-crafting`: at the workbench, fitting and firing
    /// ([`script_bench`]).
    Bench,
    /// `--script-crafting`: walking on to the fabricator, the second core
    /// recorded as a blueprint.
    ToFabricator,
    /// `--script-crafting`: at the fabricator, starting a run
    /// ([`script_factory`]).
    Factory,
    /// `--script-crafting`: carried to the middle of the nearest deposit
    /// ([`script_survey`]).
    ToPing,
    /// `--script-crafting`: at the deposit, pinging and sampling a lot.
    Sample,
    /// `--script-crafting`: carried back, walking to the locker.
    ToLocker,
    /// `--script-crafting`: at the locker, sending the lot to the store.
    Stow,
    /// Walking to the terminal.
    ToTerminal,
    /// Asked the terminal.
    Swap,
    /// `--script-crafting`: everything else is done and the fabricator's
    /// run is waited for ([`script_factory`]).
    Run,
    /// `--script-crafting`: the run's cores are sold at the board
    /// ([`script_sale`]), the last act.
    /// `--script-crafting`: walking to the procurement board.
    ToBoard,
    Sell,
    /// `--script-crafting`: walking back to the workbench for the smith's
    /// carbine ([`script_carbine`]).
    ToWorkbenchAgain,
    /// `--script-crafting`: at the workbench, the smith makes the carbine.
    Carbine,
    /// `--script-crafting`: walking to the locker with it.
    ToLockerAgain,
    /// `--script-crafting`: at the locker, the carbine goes to the store
    /// and the arrival takes it in hand, the last act.
    Handover,
    /// `--open-sheet`, or `--open-fitting` with `--script-crafting`:
    /// standing at the trainer with the sheet open, or at the workbench
    /// with the fitting screen open, for a screenshot; the path ends here.
    Sheet,
    /// Everything happened; the run may exit.
    Done,
}

/// The scripted path's state.
#[derive(Resource, Debug)]
pub struct ProgressionScript {
    /// The step under way.
    pub phase: Phase,
    /// Seconds in it.
    pub since: f32,
    /// `--open-sheet`: open the sheet at the trainer instead of asking
    /// for the lesson.
    pub open_sheet: bool,
    /// `--open-bench`: open the bench at the terminal instead of swapping.
    pub open_bench: bool,
    /// `--script-crafting`: stop at the workbench between the lesson and
    /// the terminal.
    pub crafting: bool,
    /// `--script-hurt n`: from [`HURT_AT_SECS`] into the fight the
    /// nearest hostile lands `n` hits on the player, [`HURT_EVERY_SECS`]
    /// apart, through the same `Combatant::hit` the physics calls and the
    /// same `CombatImpact` it writes. The blows still to land; none by
    /// default.
    pub hurts: u32,
    /// Seconds into the fight the next blow lands.
    pub hurt_at: f32,
}

impl Default for ProgressionScript {
    fn default() -> Self {
        Self {
            phase: Phase::Fight,
            since: 0.,
            open_sheet: false,
            open_bench: false,
            crafting: false,
            hurts: 0,
            hurt_at: HURT_AT_SECS,
        }
    }
}

/// How far into the fight the scripted hurt lands, seconds: the player is
/// still studying the moveset at the training target and the hostiles
/// have been placed, so the hurt reads alone.
pub const HURT_AT_SECS: f32 = 4.0;

/// The time between scripted blows, seconds: each one's number and flash
/// have settled before the next.
pub const HURT_EVERY_SECS: f32 = 1.0;

/// What `attacker` lands with one blow, from the tables it fights by: a
/// shot of the weapon in hand if its sheet certifies it, else its heavy
/// cut if its boxes open one, scaled by its damage multiplier as the
/// physics scales it. `None` for an actor that cannot strike, such as the
/// passive training target.
pub fn strike(attacker: &Combatant) -> Option<(f32, f32, bool)> {
    let power = attacker.damage_scale;
    if attacker.certified(attacker.weapon) {
        let shot = attacker.weapon_spec();
        Some((shot.damage * power, shot.posture * power, false))
    } else if attacker.unlocked(Action::Heavy) {
        let cut = attacker.spec(Action::Heavy);
        Some((cut.damage * power, cut.posture * power, true))
    } else {
        None
    }
}

/// `SimTick`, with [`ProgressionScript::hurts`]: from [`HURT_AT_SECS`]
/// into the fight, the nearest living hostile that can strike hits the
/// player, once a blow. The sim's own `Combatant::hit` spends the pools and the
/// `CombatImpact` is the one the physics would write, so the HUD, the
/// camera and the ledger see an ordinary hit.
#[allow(
    clippy::type_complexity,
    reason = "the player and the hostiles are two views of the same actors"
)]
pub fn script_hurt(
    mut script: ResMut<ProgressionScript>,
    frames: Option<Res<bevy::diagnostic::FrameCount>>,
    mut impacts: MessageWriter<strata::sim::combat::CombatImpact>,
    mut players: Query<(Entity, &WorldPosition, &mut Combatant), With<LocalPlayer>>,
    hostiles: Query<(Entity, &WorldPosition, &Combatant), Without<LocalPlayer>>,
) {
    if script.hurts == 0 || script.phase != Phase::Fight || script.since < script.hurt_at {
        return;
    }
    let Ok((player, at, mut actor)) = players.single_mut() else {
        return;
    };
    let Some((attacker, from, (damage, posture, melee))) = hostiles
        .iter()
        .filter(|(_, _, other)| {
            strata::sim::encounters::enemies(other.team, actor.team) && other.action != Action::Dead
        })
        .filter_map(|(entity, position, other)| Some((entity, position, strike(other)?)))
        .min_by(|a, b| a.1.0.distance(at.0).total_cmp(&b.1.0.distance(at.0)))
        .map(|(entity, position, blow)| (entity, *position, blow))
    else {
        return;
    };
    script.hurts -= 1;
    script.hurt_at += HURT_EVERY_SECS;
    let toward = (from.0 - at.0).as_vec3();
    let kind = if melee {
        strata::sim::combat::BlowKind::Cut
    } else {
        strata::sim::combat::BlowKind::Bolt
    };
    let blow = strata::sim::combat::Blow::new(damage, posture, toward, kind);
    let Some(dealt) = actor.hit(blow) else {
        info!("script: the scripted hurt did not land (dodging or down)");
        return;
    };
    info!(
        "script: hurt for {:.1} by {attacker:?} (frame {})",
        dealt.pools,
        frames.map_or("?".to_owned(), |f| f.0.to_string())
    );
    impacts.write(strata::sim::combat::CombatImpact {
        attacker,
        victim: Some(player),
        from: from.0,
        point: at.0,
        melee,
        dealt: dealt.pools,
        lethal: dealt.lethal,
        guard: dealt.guard,
        shot: None,
    });
}

impl ProgressionScript {
    /// Whether the combat script still owns the player.
    pub fn fighting(&self) -> bool {
        self.phase == Phase::Fight
    }

    /// Whether the path has been walked to its end, which with
    /// `--open-sheet` is the trainer.
    pub fn done(&self) -> bool {
        matches!(self.phase, Phase::Done | Phase::Sheet)
    }

    /// Where the path goes once the trainer is done with: the workbench
    /// with `--script-crafting`, else straight to the terminal.
    pub fn after_lesson(&self) -> Phase {
        if self.crafting {
            Phase::ToWorkbench
        } else {
            Phase::ToTerminal
        }
    }

    /// Where the path goes once the workbench and the fabricator are done
    /// with: the nearest deposit.
    pub fn after_bench() -> Phase {
        Phase::ToPing
    }

    /// Where the path goes once the cores are sold: back to the workbench,
    /// where the smith makes the carbine.
    pub fn after_sale() -> Phase {
        Phase::ToWorkbenchAgain
    }

    /// Where the path goes once the terminal is done with: with
    /// `--script-crafting` the fabricator's run is waited for, else it
    /// ends.
    pub fn after_swap(&self) -> Phase {
        if self.crafting {
            Phase::Run
        } else {
            Phase::Done
        }
    }

    pub fn advance(&mut self, phase: Phase, frame: Option<u32>) {
        info!(
            "script: {:?} -> {phase:?} (frame {})",
            self.phase,
            frame.map_or("?".to_owned(), |f| f.to_string())
        );
        self.phase = phase;
        self.since = 0.;
    }
}

/// The box the script learns: the first box up the Cuts branch.
pub const SCRIPT_BOX: &str = "blade.cuts.1";

/// Whether the sheet has yet to earn the blade XP [`SCRIPT_BOX`] costs:
/// while it has, the combat script keeps cutting the training target
/// after the hostiles are down, so the lesson is paid for by play and
/// not by a margin of one hit.
pub fn short_of_gate(sheet: &CharacterSheet, graph: &SkillGraph) -> bool {
    let Some((tree, b)) = graph.find_with_tree(SCRIPT_BOX) else {
        return false;
    };
    !sheet.has(SCRIPT_BOX) && sheet.xp(tree.xp) < b.xp
}

/// The fight is over when no hostile stands, or when it has gone on this
/// long, seconds, so a run that cannot finish it still walks the rest.
pub const FIGHT_LIMIT_SECS: f32 = 110.;

/// A walk that takes longer than this, seconds, is given up as blocked.
pub const WALK_LIMIT_SECS: f32 = 40.;

/// `--script-progression` after the fight: walk to the blade trainer,
/// learn One hand 1 if the fight earned it ([`script_learn`]), with
/// `--script-crafting` walk to the workbench and fit, fire and remove
/// there ([`script_bench`]) and on to the fabricator to start a run
/// ([`script_factory`]), walk to the terminal, let the recruits join and
/// swap to the pistol marksman ([`script_swap`]). Each step is a log
/// line, which the headless test reads beside the file.
#[allow(
    clippy::too_many_arguments,
    clippy::too_many_lines,
    reason = "the script reads the encounter, the props and the player and writes one intent, a phase at a time"
)]
pub fn drive_script(
    time: Res<Time<Fixed>>,
    frames: Option<Res<bevy::diagnostic::FrameCount>>,
    mut script: ResMut<ProgressionScript>,
    encounter: Option<Res<CombatEncounter>>,
    reach: Res<Reach>,
    registries: Res<strata::sim::Registries>,
    household: Res<Household>,
    props: Query<(Entity, &WorldPosition, &Interaction)>,
    mut players: Query<
        (
            &WorldPosition,
            &mut ScriptedIntent,
            &mut CombatIntent,
            &Combatant,
            &CharacterSheet,
        ),
        With<LocalPlayer>,
    >,
) {
    let Ok((position, mut movement, mut input, actor, sheet)) = players.single_mut() else {
        return;
    };
    let Some(encounter) = encounter else {
        return;
    };
    if !encounter.ready {
        return;
    }
    script.since += time.delta_secs();
    let frame = frames.as_ref().map(|f| f.0);
    let nearest = |wanted: &dyn Fn(&InteractionKind) -> bool| {
        props
            .iter()
            .filter(|(_, _, i)| wanted(&i.kind))
            .min_by(|a, b| {
                a.1.0
                    .distance(position.0)
                    .total_cmp(&b.1.0.distance(position.0))
            })
            .map(|(e, p, _)| (e, p.0))
    };
    let is_terminal = |k: &InteractionKind| *k == InteractionKind::Terminal;
    let is_workbench = |k: &InteractionKind| *k == InteractionKind::Workbench;
    let is_fabricator = |k: &InteractionKind| *k == InteractionKind::Fabricator;
    let is_board = |k: &InteractionKind| matches!(k, InteractionKind::Board { .. });
    let is_locker = |k: &InteractionKind| *k == InteractionKind::Locker;
    let is_blade = |k: &InteractionKind| matches!(k, InteractionKind::Trainer { trees } if trees.iter().any(|t| t == "blade"));
    let walk_to = |target: WorldPos, movement: &mut ScriptedIntent, input: &mut CombatIntent| {
        movement.0 = PlayerIntent::default();
        *input = CombatIntent::default();
        let delta = target.0 - position.0.0;
        let distance = delta.x.hypot(delta.z);
        #[allow(clippy::cast_possible_truncation, reason = "a heading in radians")]
        {
            movement.0.yaw = (-delta.x).atan2(-delta.z) as f32;
        }
        if distance > f64::from(REACH_M) - 1. {
            movement.0.move_dir = Vec2::Y;
        }
    };
    match script.phase {
        Phase::Fight => {
            let graph = &registries.0.progression;
            let over = encounter.remaining == 0
                && actor.action == Action::Ready
                && !short_of_gate(sheet, graph);
            if over || script.since > FIGHT_LIMIT_SECS {
                info!(
                    "script: fight over after {:.1} s, {} hostiles left, blade xp {}, ranged xp {}",
                    script.since,
                    encounter.remaining,
                    sheet.xp(strata::registry::progression::XpKind::Blade),
                    sheet.xp(strata::registry::progression::XpKind::Ranged)
                );
                script.advance(Phase::ToTrainer, frame);
            }
        }
        Phase::ToTrainer => {
            let Some((entity, target)) = nearest(&is_blade) else {
                return;
            };
            walk_to(target, &mut movement, &mut input);
            if reach.nearest.as_ref().is_some_and(|(e, _)| *e == entity) {
                movement.0.move_dir = Vec2::ZERO;
                info!("script: at the blade trainer, asking for a lesson");
                script.advance(Phase::Learn, frame);
            } else if script.since > WALK_LIMIT_SECS {
                warn!("script: could not reach the blade trainer");
                let next = script.after_lesson();
                script.advance(next, frame);
            }
        }
        Phase::Learn => {
            // The lesson itself is `script_learn`'s; this waits a few
            // ticks for the sheet to show it.
            movement.0.move_dir = Vec2::ZERO;
            if script.since > 0.1 {
                let graph = &registries.0.progression;
                info!(
                    "script: cuts 1 {}, {} points free",
                    if sheet.has("blade.cuts.1") {
                        "learned"
                    } else {
                        "not learned"
                    },
                    sheet.points_free(graph)
                );
                let next = script.after_lesson();
                script.advance(next, frame);
            }
        }
        Phase::ToWorkbench => {
            let Some((entity, target)) = nearest(&is_workbench) else {
                return;
            };
            walk_to(target, &mut movement, &mut input);
            if reach.nearest.as_ref().is_some_and(|(e, _)| *e == entity) {
                movement.0.move_dir = Vec2::ZERO;
                info!("script: at the workbench");
                script.advance(Phase::Bench, frame);
            } else if script.since > WALK_LIMIT_SECS {
                warn!("script: could not reach the workbench");
                script.advance(Phase::ToTerminal, frame);
            }
        }
        Phase::Bench => {
            // The fitting and the firing are `script_bench`'s, which owns
            // the combat input here; this only keeps the feet still.
            movement.0.move_dir = Vec2::ZERO;
        }
        Phase::ToFabricator => {
            let Some((entity, target)) = nearest(&is_fabricator) else {
                return;
            };
            walk_to(target, &mut movement, &mut input);
            if reach.nearest.as_ref().is_some_and(|(e, _)| *e == entity) {
                movement.0.move_dir = Vec2::ZERO;
                info!("script: at the fabricator");
                script.advance(Phase::Factory, frame);
            } else if script.since > WALK_LIMIT_SECS {
                warn!("script: could not reach the fabricator");
                script.advance(ProgressionScript::after_bench(), frame);
            }
        }
        Phase::Factory => {
            // The run is `script_factory`'s to start.
            movement.0.move_dir = Vec2::ZERO;
            *input = CombatIntent::default();
        }
        Phase::ToPing | Phase::Sample | Phase::Stow => {
            // The carrying, the ping, the sampling and the stowing are `script_survey`'s.
            movement.0 = PlayerIntent {
                yaw: movement.0.yaw,
                ..default()
            };
            *input = CombatIntent::default();
        }
        Phase::ToBoard => {
            let Some((_, target)) = nearest(&is_board) else {
                return;
            };
            walk_to(target, &mut movement, &mut input);
            if reach.board.is_some() {
                movement.0.move_dir = Vec2::ZERO;
                info!("script: at the board");
                script.advance(Phase::Sell, frame);
            } else if script.since > WALK_LIMIT_SECS {
                warn!("script: could not reach the board");
                script.advance(Phase::Done, frame);
            }
        }
        Phase::ToLocker => {
            let Some((entity, target)) = nearest(&is_locker) else {
                return;
            };
            walk_to(target, &mut movement, &mut input);
            if reach.nearest.as_ref().is_some_and(|(e, _)| *e == entity) {
                movement.0.move_dir = Vec2::ZERO;
                info!("script: at the locker");
                script.advance(Phase::Stow, frame);
            } else if script.since > WALK_LIMIT_SECS {
                warn!("script: could not reach the locker");
                script.advance(Phase::ToTerminal, frame);
            }
        }
        Phase::ToWorkbenchAgain => {
            let Some((entity, target)) = nearest(&is_workbench) else {
                return;
            };
            walk_to(target, &mut movement, &mut input);
            if reach.nearest.as_ref().is_some_and(|(e, _)| *e == entity) {
                movement.0.move_dir = Vec2::ZERO;
                info!("script: back at the workbench");
                script.advance(Phase::Carbine, frame);
            } else if script.since > WALK_LIMIT_SECS {
                warn!("script: could not reach the workbench again");
                script.advance(Phase::Done, frame);
            }
        }
        Phase::ToLockerAgain => {
            let Some((entity, target)) = nearest(&is_locker) else {
                return;
            };
            walk_to(target, &mut movement, &mut input);
            if reach.nearest.as_ref().is_some_and(|(e, _)| *e == entity) {
                movement.0.move_dir = Vec2::ZERO;
                info!("script: at the locker with the carbine");
                script.advance(Phase::Handover, frame);
            } else if script.since > WALK_LIMIT_SECS {
                warn!("script: could not reach the locker with the carbine");
                script.advance(Phase::Done, frame);
            }
        }
        Phase::Carbine | Phase::Handover => {
            // The smith's work and the handing over are `script_carbine`'s.
            movement.0.move_dir = Vec2::ZERO;
            *input = CombatIntent::default();
        }
        Phase::ToTerminal => {
            let Some((entity, target)) = nearest(&is_terminal) else {
                return;
            };
            walk_to(target, &mut movement, &mut input);
            if reach.nearest.as_ref().is_some_and(|(e, _)| *e == entity) {
                movement.0.move_dir = Vec2::ZERO;
                info!("script: at the terminal");
                script.advance(Phase::Swap, frame);
            } else if script.since > WALK_LIMIT_SECS {
                warn!("script: could not reach the terminal");
                script.advance(Phase::Done, frame);
            }
        }
        Phase::Swap => {
            movement.0.move_dir = Vec2::ZERO;
            // The swap itself is `script_swap`'s; this only waits for it
            // to have happened or to have been refused for good.
            if script.since > 0.5 {
                info!("script: {} is out", household.active().name);
                let next = script.after_swap();
                script.advance(next, frame);
            }
        }
        // The wait for the fabricator is `script_factory`'s.
        Phase::Run | Phase::Sell | Phase::Sheet | Phase::Done => {
            movement.0 = PlayerIntent::default();
        }
    }
}

/// The script's lesson, apart from the keyboard's: at the trainer the
/// first box of its trees the sheet can take is learned, or with
/// `--open-sheet` the trainer is used as F would use it and the sheet
/// screen opens on the blade tree instead, where the path ends.
#[allow(
    clippy::too_many_arguments,
    reason = "the lesson reads the reach, the graph and the player and writes the household, the interaction or the cue"
)]
pub fn script_learn(
    mut script: ResMut<ProgressionScript>,
    frames: Option<Res<bevy::diagnostic::FrameCount>>,
    mut done: Local<bool>,
    mut household: ResMut<Household>,
    registries: Res<strata::sim::Registries>,
    reach: Res<Reach>,
    mut interactions: MessageWriter<Interact>,
    mut players: Query<PlayerParts, With<LocalPlayer>>,
    // A headless run has no UI to register the cue, nor to hear it.
    cues: Option<ResMut<Messages<ProgressCue>>>,
) {
    if script.phase != Phase::Learn || *done {
        return;
    }
    let Ok(mut player) = players.single_mut() else {
        return;
    };
    *done = true;
    let Some((entity, InteractionKind::Trainer { trees })) = reach.nearest.clone() else {
        warn!("script: no trainer within reach");
        return;
    };
    if script.open_sheet {
        interactions.write(Interact {
            entity,
            kind: InteractionKind::Trainer { trees },
        });
        info!("script: the sheet opens at the trainer");
        script.advance(Phase::Sheet, frames.as_ref().map(|f| f.0));
        return;
    }
    match learn_at_trainer(
        &trees,
        &registries.0.progression,
        &reach,
        out!(player),
        &mut household,
    ) {
        // The screen's lesson has its sound; the script's stands in for it.
        Ok(_) => {
            if let Some(mut cues) = cues {
                cues.write(ProgressCue::Learned);
            }
        }
        Err(reason) => info!("household: nothing learned: {reason}"),
    }
}

/// The script's swap, apart from the screen's: everyone joins and the
/// pistol marksman (the recruit certified on the pistol) is picked by
/// name rather than by a button, so the log says who. With
/// `--open-bench` the terminal is used as F would use it instead, and
/// the bench screen opens over the full household.
#[allow(
    clippy::too_many_arguments,
    reason = "the swap reads the reach, the graph and the player and writes the household, the interaction or the cue"
)]
pub fn script_swap(
    script: Res<ProgressionScript>,
    mut done: Local<bool>,
    mut household: ResMut<Household>,
    registries: Res<strata::sim::Registries>,
    (encounter, run): (Option<Res<CombatEncounter>>, Option<Res<FactoryState>>),
    reach: Res<Reach>,
    mut interactions: MessageWriter<Interact>,
    mut players: Query<PlayerParts, With<LocalPlayer>>,
    cues: Option<ResMut<Messages<ProgressCue>>>,
) {
    if script.phase != Phase::Swap || *done {
        return;
    }
    let Ok(mut player) = players.single_mut() else {
        return;
    };
    *done = true;
    join_all(&mut household);
    if script.open_bench {
        if let Some((entity, InteractionKind::Terminal)) = reach.nearest.clone() {
            interactions.write(Interact {
                entity,
                kind: InteractionKind::Terminal,
            });
            info!("script: the bench opens at the terminal");
        } else {
            warn!("script: no terminal within reach to open the bench at");
        }
        return;
    }
    let Some(index) = household.index_with("marksman.pistol.1") else {
        warn!("script: no pistol marksman on the bench");
        return;
    };
    match swap_to(
        &mut household,
        index,
        &registries.0,
        encounter.as_deref(),
        run.as_deref().and_then(|run| run.job.as_ref()),
        out!(player),
    ) {
        Ok(()) => {
            if let Some(mut cues) = cues {
                cues.write(ProgressCue::Switched);
            }
        }
        Err(reason) => warn!("script: swap refused: {reason}"),
    }
}
