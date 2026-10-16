//! A repeatable open-world encounter, separate from terrain generation.

use bevy::ecs::system::EntityCommands;
use bevy::prelude::*;
#[cfg(not(target_arch = "wasm32"))]
use std::io::Write;
use strata::core::{SimTime, WorldPos};
use strata::physics::CharacterController;
use strata::physics::avian3d::prelude::{SpatialQuery, SpatialQueryFilter};
use strata::prelude::*;
use strata::registry::GameRegistries;
use strata::sim::combat::{Action, BlasterBolt, CombatEncounter, CombatIntent, Combatant, Weapon};
use strata::sim::combat_ai::{OpponentBrain, OpponentKind, Seen};
use strata::sim::encounters::{Allegiance, EncounterId, Fights, Present, hostility};
use strata::sim::progression::{CharacterSheet, XpGained};

#[cfg(not(target_arch = "wasm32"))]
#[derive(Resource)]
struct Trace(std::io::BufWriter<std::fs::File>);

/// An encounter opponent: whether it is the training target, and the
/// shipped AI (`strata_sim::combat_ai`) that fights once it is released.
#[derive(Component)]
struct Opponent {
    passive: bool,
    brain: OpponentBrain,
}

#[derive(Component)]
struct ResetPosition(WorldPos);

/// One hostile of `--hostiles`: its kind and how far ahead of the player it
/// stands, in metres, where the flag named one.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct HostileSpec {
    kind: OpponentKind,
    metres: Option<f32>,
}

impl HostileSpec {
    /// Where a kind stands when the flag names no distance: the sentry and
    /// the rusher where the encounter puts them, the marksman at the range
    /// it holds and the duelist at a few steps.
    fn metres(self) -> f32 {
        self.metres.unwrap_or(match self.kind {
            OpponentKind::Sentry => 27.,
            OpponentKind::Rusher => 18.,
            OpponentKind::Marksman => 60.,
            OpponentKind::Duelist => 6.,
            OpponentKind::Bulwark => 12.,
        })
    }
}

/// `kind[@metres]`, one entry of `--hostiles`.
pub(crate) fn parse_hostile(text: &str) -> Result<HostileSpec, String> {
    let (name, metres) = match text.split_once('@') {
        Some((name, metres)) => (name, Some(metres)),
        None => (text, None),
    };
    let kind = OpponentKind::from_name(name.trim()).ok_or_else(|| {
        let known: Vec<_> = OpponentKind::ALL.iter().map(|k| k.name()).collect();
        format!("no hostile kind `{name}`: one of {}", known.join(", "))
    })?;
    let metres = metres
        .map(|metres| {
            metres
                .trim()
                .parse::<f32>()
                .ok()
                .filter(|m| m.is_finite() && *m >= 1.)
                .ok_or_else(|| format!("`{metres}` is not a distance of a metre or more"))
        })
        .transpose()?;
    Ok(HostileSpec { kind, metres })
}

#[derive(Resource, Default)]
#[allow(
    clippy::struct_excessive_bools,
    reason = "three of them are flags of the run, each its own switch on the command line"
)]
struct Encounter {
    started: bool,
    elapsed: f32,
    scripted: bool,
    /// `--script-progression`: the fight is fought melee first, so the
    /// rusher's death earns the blade XP One hand 1 costs, and the household
    /// script takes the player over once it is won.
    progression: bool,
    /// `--no-hostiles`: the encounter stands up with nobody in it, so a
    /// still of the world has no fight and no nameplates in the frame.
    empty: bool,
    /// `--hostiles`: these replace the encounter's own.
    chosen: Option<Vec<HostileSpec>>,
    /// Frames in which the encounter's own three had ground and a clear
    /// line and its marksman, 60 m out, had neither.
    far_waited: u32,
    last_report: u32,
}

/// How long the encounter's own marksman is waited for once the others can
/// stand. Where a street, a wood or a hill leaves no clear line at 60 m the
/// fight begins without it: the old three must never wait on the new one.
const FAR_WAIT_FRAMES: u32 = 300;

pub(super) fn install(app: &mut App, cli: &super::Cli) {
    if !cli.combat {
        return;
    }
    let (scripted, headless) = (cli.combat_script, cli.headless);
    let progression = cli.script_progression;
    let path = "characters/combat/melee.ron";
    let text = super::data_files(cli)
        .under("assets")
        .read_string(path)
        .unwrap_or_else(|error| panic!("combat curves {path}: {error}"));
    let curves =
        strata::sim::combat_curves::MeleeCurves::parse(&text).expect("valid combat blade curves");
    app.insert_resource(curves)
        .init_resource::<CombatEncounter>();
    #[cfg(not(target_arch = "wasm32"))]
    if let Some(path) = &cli.combat_trace {
        let mut file =
            std::io::BufWriter::new(std::fs::File::create(path).expect("create combat trace"));
        writeln!(
            file,
            "tick,seconds,action,age,x,y,z,motor_speed,health,shield,stamina,ammo,shots,hurts,hostiles,enemy_shots,bolts"
        )
        .expect("write trace header");
        app.insert_resource(Trace(file))
            .add_systems(SimTick, trace_state.in_set(SimSet::Observe));
    }
    app.insert_resource(Time::<Fixed>::from_hz(60.))
        .insert_resource(SimClock(SimTime::from_hz(60.)))
        .insert_resource(Encounter {
            scripted,
            progression,
            empty: cli.no_hostiles,
            chosen: (!cli.hostiles.is_empty()).then(|| cli.hostiles.clone()),
            ..default()
        })
        .add_systems(Update, setup.run_if(in_state(AppState::InWorld)))
        .add_systems(
            SimTick,
            (
                super::camps::sync_post_teams,
                drive_opponents,
                drive_script,
                super::household::drive_script
                    .run_if(resource_exists::<super::household::ProgressionScript>),
                report,
            )
                .chain()
                .before(strata::sim::player::apply_scripted_intents)
                .in_set(SimSet::Input),
        )
        .add_systems(SimTick, log_xp.in_set(SimSet::Observe));
    let forced = cli.camp_tier;
    app.init_resource::<super::camps::Waiting>()
        .add_systems(
            Startup,
            move |mut near: ResMut<strata::sim::encounters::NearEncounters>| {
                near.forced_tier = forced;
            },
        )
        .add_systems(
            Update,
            (
                super::camps::clear_nearest.before(strata::sim::encounters::update_near_encounters),
                super::factions::still_jobs,
                super::standing::set_stance_and_lease,
                super::camps::stage_camps.after(strata::sim::encounters::update_near_encounters),
            )
                .run_if(in_state(AppState::InWorld)),
        );
    if !headless {
        app.add_plugins(strata::client::combat::CombatViewPlugin)
            .add_systems(Update, reset.run_if(strata::client::ui::game_input_allowed));
        if scripted {
            app.add_systems(
                Update,
                scripted_camera
                    .in_set(strata::client::ClientSet::Camera)
                    .run_if(strata::client::ui::game_input_allowed),
            );
        }
    }
    if scripted || cli.script_survey.is_some() {
        app.insert_resource(bevy::time::TimeUpdateStrategy::ManualDuration(
            std::time::Duration::from_secs_f64(1. / 60.),
        ));
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn trace_state(
    clock: Res<SimClock>,
    mut trace: ResMut<Trace>,
    status: Res<CombatEncounter>,
    enemies: Query<&Combatant, With<Opponent>>,
    bolts: Query<(), With<BlasterBolt>>,
    players: Query<(&WorldPosition, &CharacterController, &Combatant), With<LocalPlayer>>,
) {
    if let Ok((p, m, a)) = players.single() {
        let result = writeln!(
            trace.0,
            "{},{:.6},{:?},{:.6},{:.6},{:.6},{:.6},{:.6},{:.3},{:.3},{:.3},{},{},{},{},{},{}",
            clock.tick(),
            clock.elapsed_secs(),
            a.action,
            a.age,
            p.0.x(),
            p.0.y(),
            p.0.z(),
            m.horizontal_velocity.length(),
            a.health,
            a.shield,
            a.stamina,
            a.ammo,
            a.shots,
            a.hurts,
            status.remaining,
            enemies.iter().map(|a| a.shots).sum::<u64>(),
            bolts.iter().count()
        );
        if let Err(error) = result {
            warn!("combat trace write failed: {error}");
        }
    }
}

#[allow(
    clippy::too_many_lines,
    clippy::too_many_arguments,
    reason = "stage three supported, visible placements before spawning any opponents"
)]
fn setup(
    mut commands: Commands,
    mut encounter: ResMut<Encounter>,
    mut status: ResMut<CombatEncounter>,
    players: Query<(Entity, &WorldPosition, &PlayerIntent), With<LocalPlayer>>,
    source: Res<WorldSourceRes>,
    origin: Res<RenderOrigin>,
    spatial: SpatialQuery,
    registries: Res<strata::sim::Registries>,
    staging: Option<Res<StagingSpawn>>,
) {
    if encounter.started {
        return;
    }
    let Ok((player, position, heading)) = players.single() else {
        return;
    };
    // A household that starts on the quay (ticket 0014) fights at the
    // training ground all the same: the fight stands where it always did.
    let (target, yaw) = staging
        .as_deref()
        .map_or((position.0, heading.yaw), |staging| {
            (staging.at, staging.yaw)
        });
    let mut placements = Vec::new();
    // The marksman stands ahead at the range it holds, the far side of the
    // staging ground from the bench props behind the start; it is engaged by
    // the rule the others are.
    let own = !encounter.empty && encounter.chosen.is_none();
    let hostiles: Vec<(f32, f32, OpponentKind, bool)> = if encounter.empty {
        Vec::new()
    } else if let Some(chosen) = &encounter.chosen {
        chosen
            .iter()
            .map(|spec| (0., -spec.metres(), spec.kind, false))
            .collect()
    } else {
        let mut default = vec![
            (-5., -9., OpponentKind::Rusher, true),
            (5., -18., OpponentKind::Rusher, false),
            (-3., -27., OpponentKind::Sentry, false),
            // The shield bearer comes in from the right of the line, so a
            // new household meets the guard that fire cannot answer in its
            // first fight.
            (9., -24., OpponentKind::Bulwark, false),
            (0., -60., OpponentKind::Marksman, false),
        ];
        // The scripted walks (progression, crafting) time a light-cut fight
        // against the rusher and the sentry; a guard that holds a light
        // cut's damage to a share would stall their clock, so they do not
        // meet it.
        if encounter.scripted {
            default.retain(|(_, _, kind, _)| *kind != OpponentKind::Bulwark);
        }
        default
    };
    for (x, z, kind, passive) in hostiles {
        let mut best: Option<(f32, WorldPos)> = None;
        let filter = SpatialQueryFilter::from_excluded_entities([player]);
        for dx in [0., -4., 4., -8., 8.] {
            for dz in [0., -4., 4.] {
                let offset = Quat::from_rotation_y(yaw) * Vec3::new(x + dx, 0., z + dz);
                let x = target.x() + f64::from(offset.x);
                let z = target.z() + f64::from(offset.z);
                let Some(ground) =
                    super::household::ground_at(x, z, &source, &origin, &spatial, &filter)
                else {
                    continue;
                };
                let eye = origin.to_render(target) + Vec3::Y * 0.45;
                let chest = origin.to_render(WorldPos::new(x, ground + 1.1, z));
                let line = chest - eye;
                let Ok(direction) = Dir3::new(line) else {
                    continue;
                };
                if spatial
                    .cast_ray(eye, direction, (line.length() - 0.3).max(0.), true, &filter)
                    .is_some()
                {
                    continue;
                }
                let at = WorldPos::new(x, ground + super::PLAYER_DROP + 0.9, z);
                if placements
                    .iter()
                    .any(|(p, _, _): &(WorldPos, OpponentKind, bool)| p.distance(at) < 3.)
                {
                    continue;
                }
                let score = dx * dx + dz * dz;
                if best.is_none_or(|(s, _)| score < s) {
                    best = Some((score, at));
                }
            }
        }
        // The encounter begins only when its enemies have streamed support
        // and a clear line from the player's starting shoulder height.
        let Some((_, at)) = best else {
            if own && kind == OpponentKind::Marksman {
                encounter.far_waited += 1;
                if encounter.far_waited > FAR_WAIT_FRAMES {
                    info!("combat: no clear ground for the marksman, standing without it");
                    continue;
                }
            }
            return;
        };
        placements.push((at, kind, passive));
    }
    encounter.started = true;
    status.ready = true;
    commands.entity(player).insert(ResetPosition(target));
    if encounter.scripted {
        commands.entity(player).insert(ScriptedIntent::default());
    }
    for (position, kind, passive) in placements {
        info!(
            "combat: {} at {:.0} m",
            kind.name(),
            position.distance(target)
        );
        let birth = Birth {
            passive,
            kind,
            tier: 0,
            id: EncounterId::Staging,
            allegiance: Allegiance::Staging,
        };
        spawn_hostile(&mut commands, position, birth, &registries.0)
            .insert(ResetPosition(position));
    }
    info!(
        "combat: 60 Hz study ready; passive target, melee rusher, ranged sentry, bulwark, far marksman"
    );
}

/// Where the staging fight stands when the household does not start beside
/// it: the default spawn and its heading, which the fight is placed from and
/// F5 returns the player to (ticket 0014, a new household starts on the quay).
#[derive(Resource, Debug, Clone, Copy)]
pub(crate) struct StagingSpawn {
    pub(crate) at: WorldPos,
    pub(crate) yaw: f32,
}

/// What a hostile is born as: the staging fight's people and a camp's are
/// made by [`spawn_hostile`] the same way and differ only in this.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Birth {
    pub(crate) passive: bool,
    pub(crate) kind: OpponentKind,
    /// 0 for the staging fight, which `opponents.ron` states as written.
    pub(crate) tier: u8,
    pub(crate) id: EncounterId,
    pub(crate) allegiance: Allegiance,
}

/// Makes one hostile at `position`: stats, brain, motor and the marks that
/// say which encounter it fights in.
pub(crate) fn spawn_hostile<'a>(
    commands: &'a mut Commands,
    position: WorldPos,
    birth: Birth,
    registries: &GameRegistries,
) -> EntityCommands<'a> {
    let Birth {
        passive,
        kind,
        id,
        allegiance,
        ..
    } = birth;
    let (actor, sheet) = opponent_stats(birth, registries);
    let opponents = &strata::sim::combat_tables::current().opponents;
    let motor_speed = kind.motor_speed(opponents, actor.with_actor(|row| row.aim_speed_scale));
    let mut opponent = commands.spawn((
        Name::new(if passive {
            "Training target"
        } else {
            match kind {
                OpponentKind::Sentry => "Sentry",
                OpponentKind::Rusher => "Rusher",
                OpponentKind::Marksman => "Marksman",
                OpponentKind::Duelist => "Duelist",
                OpponentKind::Bulwark => "Bulwark",
            }
        }),
        Opponent {
            passive,
            brain: OpponentBrain::new(kind, opponents),
        },
        // What the questline counts a defeat by (ticket 0014); a crew's
        // members are marked again by the game that stands them.
        strata::sim::quests::QuestFoe {
            kind: strata::sim::quests::foe_kind(kind),
            crew: None,
        },
        id,
        allegiance,
        WorldPosition(position),
        PreviousWorldPosition::default(),
        PlayerIntent::default(),
        PlayerCommand::default(),
        CharacterController {
            height: 1.8,
            responsive: true,
            walk_speed: motor_speed,
            sprint_speed: opponents.sprint_speed,
            ..default()
        },
        actor,
        sheet,
    ));
    if kind == OpponentKind::Marksman && !passive {
        opponent.insert(strata::client::combat_beam::AimBeam);
    }
    if kind == OpponentKind::Bulwark && !passive {
        opponent.insert(strata::client::combat_shield::Shield);
    }
    opponent
}

/// An opponent's pools and pressure, from `assets/combat/opponents.ron`,
/// and its moveset from its sheet in `assets/progression/opponents.ron`
/// (the rusher's One hand 2, the sentry's Carbine 2), fitted over the tables
/// the way the player's is. The passive target has an empty sheet: it
/// does nothing, so it may do nothing.
fn opponent_stats(birth: Birth, registries: &GameRegistries) -> (Combatant, CharacterSheet) {
    let Birth {
        passive,
        kind,
        tier,
        allegiance,
        ..
    } = birth;
    let table = strata::sim::combat_tables::current();
    let opponents = &table.opponents;
    // The tier scales the pools and the pressure, in the one function the
    // duel harness reads too.
    let pools = strata::sim::combat_tiers::opponent_pools(tier);
    let mut actor = Combatant::on_team(
        // Born neutral if a post's, until `sync_post_teams` hears it crossed.
        hostility(allegiance, false),
        if passive {
            opponents.passive_health
        } else {
            pools.health
        },
    );
    actor.shield = pools.shield;
    actor.shield_capacity = pools.shield;
    actor.damage_scale = pools.damage_scale;
    // The marksman fires the rifle's own mode and, set again below once it
    // carries the rifle, its own bolt speed.
    actor.bolt_speed = opponents.sentry.bolt_speed;
    actor.mode_override = (kind != OpponentKind::Marksman).then_some(opponents.sentry.mode);
    let name = (!passive).then(|| kind.name());
    let sheet = name
        .and_then(|name| registries.sheets.opponents.get(name))
        .map(CharacterSheet::authored)
        .unwrap_or_default();
    actor.fit(&registries.progression, &sheet);
    // The rusher cuts, and a cut is a blade's now: it carries one, with the
    // carbine it always had holstered, and plays as it did.
    if matches!(name, Some("rusher" | "duelist")) {
        actor = actor.carrying([Some(Weapon::Blade), Some(Weapon::Carbine)], 0);
    } else if name == Some("bulwark") {
        // The bulwark's guard is its own row, not the Blade class's.
        actor = actor
            .carrying([Some(Weapon::Blade), None], 0)
            .with_guard(Weapon::Blade, opponents.bulwark.guard);
    } else if name == Some("marksman") {
        // `carrying` reads the weapon's row into the bolt speed, so the
        // marksman's own goes on after it.
        actor = actor.carrying([Some(Weapon::Rifle), Some(Weapon::Carbine)], 0);
        actor.bolt_speed = opponents.marksman.bolt_speed;
    }
    (actor, sheet)
}

/// A line per XP credit, so a headless or scripted run shows the ledger
/// working before a HUD does.
fn log_xp(mut gained: MessageReader<XpGained>, players: Query<(), With<LocalPlayer>>) {
    for gain in gained.read() {
        if players.contains(gain.entity) {
            info!(
                "xp: +{} {}{}",
                gain.amount,
                gain.kind,
                if gain.kill { " (kill)" } else { "" }
            );
        }
    }
}

fn drive_opponents(
    time: Res<Time<Fixed>>,
    encounter: Res<Encounter>,
    mut status: ResMut<CombatEncounter>,
    mut fights: ResMut<Fights>,
    player: Query<(&WorldPosition, &Combatant), With<LocalPlayer>>,
    mut enemies: Query<
        (
            &mut Opponent,
            &EncounterId,
            &WorldPosition,
            &mut PlayerIntent,
            &mut CombatIntent,
            &mut Combatant,
        ),
        Without<LocalPlayer>,
    >,
) {
    let Ok((target, player)) = player.single() else {
        return;
    };
    let tables = strata::sim::combat_tables::current();
    let grace = encounter.elapsed < if encounter.scripted { 22. } else { 4. };
    // Each encounter engages on its own people alone; the staging fight's
    // first seconds are held, a camp's are not.
    fights.observe(
        enemies.iter().map(|(e, id, p, _, _, a)| Present {
            id: *id,
            hurt: a.hurts > 0,
            alive: a.action != Action::Dead,
            passive: e.passive,
            distance: p.0.distance(target.0),
        }),
        f64::from(tables.opponents.engage_distance),
        grace,
    );
    (status.engaged, status.remaining) = fights.headline();
    for (mut enemy, id, position, mut movement, mut input, mut actor) in &mut enemies {
        *movement = PlayerIntent::default();
        *input = CombatIntent::default();
        actor.charge = 0.;
        if enemy.passive || actor.action == Action::Dead || player.action == Action::Dead {
            continue;
        }
        // A post nobody has crossed stands as it is: not even a bulwark
        // raises its guard at the member.
        if fights.neutral(*id) {
            continue;
        }
        if (grace && *id == EncounterId::Staging) || !fights.engaged(*id) {
            enemy
                .brain
                .idle(position.0, target.0, &mut actor, &mut movement, &mut input);
            continue;
        }
        let ground = target.0.0 - position.0.0;
        #[allow(clippy::cast_possible_truncation, reason = "metres, render range")]
        let seen = Seen::of(player, ground.x.hypot(ground.z) as f32);
        enemy.brain.drive(
            time.delta_secs(),
            position.0,
            target.0,
            &seen,
            &tables.opponents,
            &mut actor,
            &mut movement,
            &mut input,
        );
    }
}

#[allow(
    clippy::too_many_lines,
    clippy::too_many_arguments,
    reason = "one script, phase by phase, the way the capture was timed; it reads the encounter, the graph and the ground"
)]
fn drive_script(
    time: Res<Time<Fixed>>,
    mut encounter: ResMut<Encounter>,
    script: Option<Res<super::household::ProgressionScript>>,
    guard_hold: Option<Res<crate::GuardHold>>,
    registries: Res<strata::sim::Registries>,
    origin: Res<RenderOrigin>,
    spatial: SpatialQuery,
    mut players: Query<
        (
            Entity,
            &WorldPosition,
            &mut ScriptedIntent,
            &mut CombatIntent,
            &Combatant,
            &CharacterSheet,
        ),
        With<LocalPlayer>,
    >,
    targets: Query<
        (&WorldPosition, &Opponent, &Combatant, &CharacterController),
        Without<LocalPlayer>,
    >,
) {
    if !encounter.started || !encounter.scripted {
        return;
    }
    let before = encounter.elapsed - 5.;
    encounter.elapsed += time.delta_secs();
    let now = encounter.elapsed - 5.;
    // Once the fight is won the household script walks the player.
    if script.is_some_and(|s| !s.fighting()) {
        return;
    }
    let hold_guard = guard_hold.is_some_and(|hold| hold.0);
    for (entity, position, mut movement, mut input, actor, sheet) in &mut players {
        movement.0 = PlayerIntent::default();
        // A draw request is held, not an edge: it survives the per-tick reset
        // the way a player's does until the sim takes it.
        let weapon = input.weapon;
        // `--hold-guard` writes the button once a frame, and the reset below
        // would take it before the sim does; its own clock says whether it
        // is holding now, so the script's output never feeds the latch.
        let held_aim = hold_guard;
        *input = CombatIntent::default();
        input.weapon = weapon;
        let target = targets
            .iter()
            .find(|(_, enemy, _, _)| enemy.passive)
            .map_or(
                position.0.0 + bevy::math::DVec3::NEG_Z * 100.,
                |(position, _, _, _)| position.0.0,
            );
        input.aim_point = WorldPos(target + bevy::math::DVec3::Y * 0.2);
        let delta = target - position.0.0;
        #[allow(clippy::cast_possible_truncation, reason = "local scripted heading")]
        {
            movement.0.yaw = (-delta.x).atan2(-delta.z) as f32;
        }
        // After the moveset study, fight both hostiles with ordinary inputs.
        // This exercises the real encounter without immunity or damage overrides.
        if now >= 17. {
            let mut target = targets
                .iter()
                .filter(|(_, enemy, actor, _)| !enemy.passive && actor.action != Action::Dead)
                .min_by(|(a, _, _, _), (b, _, _, _)| {
                    a.0.distance(position.0)
                        .total_cmp(&b.0.distance(position.0))
                });
            // Hostiles down and the lesson still unpaid: the training
            // target takes the rest of the cuts.
            if target.is_none()
                && encounter.progression
                && super::household::short_of_gate(sheet, &registries.0.progression)
            {
                target = targets.iter().find(|(_, enemy, _, _)| enemy.passive);
            }
            if let Some((target, enemy, _, motor)) = target {
                let distance = target.0.distance(position.0);
                let lead = motor.horizontal_velocity.as_dvec3() * (distance / 38.).min(0.6);
                input.aim_point = WorldPos(target.0.0 + lead + bevy::math::DVec3::Y * 0.2);
                let aim = input.aim_point.0 - position.0.0;
                #[allow(clippy::cast_possible_truncation, reason = "local scripted heading")]
                {
                    movement.0.yaw = (-aim.x).atan2(-aim.z) as f32;
                }
                // The rusher is taken with the blade, the sentry with the
                // carbine; the draw is held until the sim takes it.
                let wanted = if enemy.brain.kind.ranged() {
                    Weapon::Carbine
                } else {
                    Weapon::Blade
                };
                if actor.weapon != wanted
                    && let Some(slot) = actor.slots().iter().position(|slot| *slot == Some(wanted))
                {
                    input.weapon = Some(slot);
                }
                // The progression script takes the rusher with the blade:
                // close in and cut whenever the body is free, so the kill
                // earns blade XP and not ranged.
                if encounter.progression && !enemy.brain.kind.ranged() {
                    if distance > 1.5 {
                        movement.0.move_dir.y = 1.;
                        movement.0.sprint = distance > 6.;
                    } else {
                        input.light = actor.action == Action::Ready;
                    }
                    continue;
                }
                // A scripted hand taps: a pistol fires a press at a time and
                // a carbine a burst, so a held trigger would fire the pistol
                // once. The pulse is slow enough for every press to land.
                input.fire = !(18.0..21.0).contains(&now) && (now * 20.).floor() % 2. == 0.;
                input.aim = true;
                input.reload = actor.ammo == 0;
                movement.0.move_dir.x = if (now % 6.) < 3. { 0.6 } else { -0.6 };
                if distance < 5. {
                    movement.0.move_dir.y = -0.5;
                } else {
                    let shoulder = origin.to_render(position.0) + Vec3::Y * 0.45;
                    let path = origin.to_render(input.aim_point) - shoulder;
                    let blocked = Dir3::new(path).is_ok_and(|direction| {
                        spatial
                            .cast_ray(
                                shoulder,
                                direction,
                                (path.length() - 0.8).max(0.),
                                true,
                                &SpatialQueryFilter::from_excluded_entities([entity]),
                            )
                            .is_some()
                    });
                    if blocked || distance > 22. {
                        movement.0.move_dir.y = 0.8;
                    }
                }
            }
            continue;
        }
        // Windows leave enough time for streamed colliders and the first asset load.
        input.fire =
            (3.0..4.0).contains(&now) || (12.0..14.0).contains(&now) || (15.6..16.6).contains(&now);
        input.aim = input.fire || held_aim;
        // A cut is a blade's, a burst a gun's: draw the blade for the cuts
        // and the carbine for the shots that follow them. The fight that
        // starts at 17 s draws what each target wants.
        let slot_of = |weapon| actor.slots().iter().position(|slot| *slot == Some(weapon));
        if before < 4.2 && now >= 4.2 {
            input.weapon = slot_of(Weapon::Blade);
        }
        if before < 10.8 && now >= 10.8 {
            input.weapon = slot_of(Weapon::Carbine);
        }
        if (4.0..5.0).contains(&now) {
            movement.0.move_dir.y = 1.;
        }
        input.light = [5.2, 5.38, 10.0].iter().any(|t| before < *t && now >= *t);
        input.heavy = before < 7.0 && now >= 7.0;
        input.dodge = before < 8.4 && now >= 8.4;
        if (8.3..8.5).contains(&now) {
            movement.0.move_dir.x = 1.;
        }
        input.reload = before < 14.2 && now >= 14.2;
    }
}

fn report(
    time: Res<Time<Fixed>>,
    mut encounter: ResMut<Encounter>,
    status: Res<CombatEncounter>,
    enemies: Query<(&Opponent, &WorldPosition, &Combatant)>,
    players: Query<(&WorldPosition, &Combatant), With<LocalPlayer>>,
) {
    if !encounter.scripted {
        encounter.elapsed += time.delta_secs();
    }
    #[allow(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "bounded diagnostic seconds"
    )]
    let seconds = encounter.elapsed as u32;
    if seconds / 2 <= encounter.last_report {
        return;
    }
    encounter.last_report = seconds / 2;
    if encounter.scripted {
        for (enemy, position, actor) in &enemies {
            info!(
                "combat opponent: ranged={} passive={} hp={:.1} shield={:.1} shots={} position={:?}",
                enemy.brain.kind.ranged(),
                enemy.passive,
                actor.health,
                actor.shield,
                actor.shots,
                position.0
            );
        }
    }

    if let Ok((position, actor)) = players.single() {
        info!(
            "combat: t={:.2} action={:?} age={:.3} hp={:.1} shield={:.1} stamina={:.1} shots={} hurts={} hostiles={} position={:?}",
            encounter.elapsed,
            actor.action,
            actor.age,
            actor.health,
            actor.shield,
            actor.stamina,
            actor.shots,
            actor.hurts,
            status.remaining,
            position.0
        );
    }
}

#[allow(
    clippy::too_many_arguments,
    reason = "a reset touches every actor, the bolts, the encounter and the household"
)]
fn reset(
    mut commands: Commands,
    bolts: Query<Entity, With<BlasterBolt>>,
    mut status: ResMut<CombatEncounter>,
    mut fights: ResMut<Fights>,
    keys: Res<ButtonInput<KeyCode>>,
    mut actors: Query<(
        &ResetPosition,
        &mut WorldPosition,
        &mut PreviousWorldPosition,
        &mut CharacterController,
        &mut Combatant,
        &mut CombatIntent,
        Option<&mut Opponent>,
        Option<&CharacterSheet>,
    )>,
    mut encounter: ResMut<Encounter>,
    registries: Option<Res<strata::sim::Registries>>,
    household: Option<Res<super::household::Household>>,
) {
    // Runs in `Update` from the first frame, before the world and its
    // registries exist; nothing to reset until they do.
    let Some(registries) = registries else {
        return;
    };
    if !keys.just_pressed(KeyCode::F5) {
        return;
    }
    *status = CombatEncounter {
        ready: true,
        ..default()
    };
    fights.reset_staging();
    fights.abandon_camps();
    for bolt in &bolts {
        commands.entity(bolt).despawn();
    }
    encounter.elapsed = 0.;
    encounter.last_report = 0;
    let tables = strata::sim::combat_tables::current();
    for (reset, mut position, mut previous, mut controller, mut actor, mut input, enemy, sheet) in
        &mut actors
    {
        position.0 = reset.0;
        previous.0 = None;
        controller.vertical_velocity = 0.;
        controller.horizontal_velocity = Vec3::ZERO;
        controller.action_velocity = None;
        // Pools and position go back; the sheet is the character's and
        // stays, with whatever it learned or earned since, and so does
        // the weapon the household's active character had in hand.
        let is_enemy = enemy.is_some();
        *actor = if let Some(mut enemy) = enemy {
            enemy.brain.reset(&tables.opponents);
            let birth = Birth {
                passive: enemy.passive,
                kind: enemy.brain.kind,
                tier: 0,
                id: EncounterId::Staging,
                allegiance: Allegiance::Staging,
            };
            opponent_stats(birth, &registries.0).0
        } else if let Some(household) = &household {
            household
                .active()
                .actor(&registries.0.progression, &registries.0.items)
        } else {
            Combatant::default()
        };
        // An opponent was fitted from its sheet by `opponent_stats`; fitting
        // again would replace its own guard with the class's.
        if let (Some(sheet), false) = (sheet, is_enemy) {
            actor.fit(&registries.0.progression, sheet);
        }
        controller.coyote_left = 0.;
        controller.jump_buffer_left = 0.;
        *input = CombatIntent::default();
    }
}

fn scripted_camera(
    players: Query<&ScriptedIntent, With<LocalPlayer>>,
    mut cameras: Query<&mut OrbitCamera>,
) {
    if let Ok(input) = players.single() {
        for mut camera in &mut cameras {
            camera.yaw = input.0.yaw;
        }
    }
}

/// Search only beside the city's western terraces and outside building bounds.
pub(super) fn default_spawn(
    source: &WorldSourceRes,
    settlements: Option<&strata::sim::Settlements>,
) -> Option<(f64, f64, f64)> {
    let settlement = settlements?.0.first()?;
    let world = source.source();
    let mut best: Option<(f64, f64, f64, f64)> = None;
    let edge = settlement.layout.as_ref().map_or(-320., |layout| {
        layout
            .buildings
            .iter()
            .map(|b| b.footprint[0])
            .fold(0., f64::min)
    });
    let wanted_x = edge - 55.;
    for ix in -5..=5 {
        for iz in -5..=5 {
            let local = [wanted_x + f64::from(ix) * 8., -70. + f64::from(iz) * 12.];
            if settlement.layout.as_ref().is_some_and(|layout| {
                layout.buildings.iter().any(|building| {
                    let [x0, z0, x1, z1] = building.footprint;
                    local[0] > x0 - 34.
                        && local[0] < x1 + 34.
                        && local[1] > z0 - 34.
                        && local[1] < z1 + 34.
                })
            }) {
                continue;
            }
            let [x, z] = settlement.site.to_world_xz(local);
            let center = world.sample(x, z).height;
            if center < world.sea_level() + 5. {
                continue;
            }
            let mut min = center;
            let mut max = center;
            for dx in [-10., 0., 30.] {
                for dz in [-12., 0., 12.] {
                    let [px, pz] = settlement.site.to_world_xz([local[0] + dx, local[1] + dz]);
                    let height = world.sample(px, pz).height;
                    min = min.min(height);
                    max = max.max(height);
                }
            }
            let score = f64::from(max - min) + (local[0] - wanted_x).hypot(local[1] + 70.) * 0.025;
            if best.is_none_or(|(s, _, _, _)| score < s) {
                best = Some((score, x, f64::from(center), z));
            }
        }
    }
    best.map(|(_, x, y, z)| (x, y, z))
}

#[cfg(test)]
mod tests {
    use super::*;
    use bevy::ecs::system::RunSystemOnce;

    fn staging(kind: OpponentKind) -> Birth {
        Birth {
            passive: false,
            kind,
            tier: 0,
            id: EncounterId::Staging,
            allegiance: Allegiance::Staging,
        }
    }

    fn ai_app(engaged: bool) -> (App, Entity, Entity) {
        let mut app = App::new();
        let mut fights = Fights::default();
        if engaged {
            fights.observe(
                [Present {
                    id: EncounterId::Staging,
                    hurt: true,
                    alive: true,
                    passive: false,
                    distance: 99.,
                }],
                11.,
                false,
            );
        }
        app.insert_resource(fights);
        let mut time = Time::<Fixed>::from_hz(60.);
        time.advance_by(std::time::Duration::from_millis(100));
        app.insert_resource(time)
            .insert_resource(CombatEncounter {
                ready: true,
                engaged,
                remaining: 1,
            })
            .insert_resource(Encounter {
                elapsed: 10.,
                ..default()
            });
        let player = app
            .world_mut()
            .spawn((
                LocalPlayer,
                WorldPosition::new(0., 1., 0.),
                Combatant::default(),
            ))
            .id();
        let sentry = app
            .world_mut()
            .spawn((
                WorldPosition::new(0., 1., 20.),
                PlayerIntent::default(),
                EncounterId::Staging,
                opponent_stats(staging(OpponentKind::Sentry), &GameRegistries::default()).0,
                Opponent {
                    passive: false,
                    brain: {
                        let mut brain = OpponentBrain::new(
                            OpponentKind::Sentry,
                            &strata::sim::combat_tables::current().opponents,
                        );
                        brain.cooldown = 0.;
                        brain
                    },
                },
            ))
            .id();
        (app, player, sentry)
    }

    #[test]
    fn opponents_fire_their_own_rows_bolt_speed_and_mode() {
        let table = strata::sim::combat_tables::current();
        let none = GameRegistries::default();
        let marksman = opponent_stats(staging(OpponentKind::Marksman), &none).0;
        assert_eq!(marksman.weapon, Weapon::Rifle);
        assert_eq!(
            marksman.bolt_speed.to_bits(),
            table.opponents.marksman.bolt_speed.to_bits()
        );
        assert_eq!(marksman.mode_override, None);
        let sentry = opponent_stats(staging(OpponentKind::Sentry), &none).0;
        assert_eq!(
            sentry.bolt_speed.to_bits(),
            table.opponents.sentry.bolt_speed.to_bits()
        );
        assert_eq!(sentry.mode_override, Some(table.opponents.sentry.mode));
    }

    #[test]
    fn sentry_commits_aim_before_its_visible_charge_and_cannot_track_a_dodge() {
        let (mut app, player, sentry) = ai_app(true);
        app.world_mut()
            .run_system_once(drive_opponents)
            .expect("AI tick");
        app.world_mut()
            .get_mut::<WorldPosition>(player)
            .expect("player")
            .0
            .0
            .x = 5.;
        for tick in 0..7 {
            app.world_mut()
                .run_system_once(drive_opponents)
                .expect("AI tick");
            let intent = app.world().get::<CombatIntent>(sentry).expect("intent");
            assert_eq!(
                intent.aim_point.x().to_bits(),
                0_f64.to_bits(),
                "aim was captured before player moved"
            );
            assert_eq!(intent.fire, tick == 6, "650 ms charge precedes release");
        }
        app.world_mut()
            .run_system_once(drive_opponents)
            .expect("recovery tick");
        assert!(
            !app.world()
                .get::<CombatIntent>(sentry)
                .expect("intent")
                .fire
        );
    }

    #[test]
    fn idle_spawn_is_safe_until_the_player_approaches_or_damages_a_hostile() {
        let (mut app, _, sentry) = ai_app(false);
        for _ in 0..100 {
            app.world_mut()
                .run_system_once(drive_opponents)
                .expect("AI tick");
        }
        assert!(!app.world().resource::<CombatEncounter>().engaged);
        assert!(
            !app.world()
                .get::<CombatIntent>(sentry)
                .expect("intent")
                .fire
        );
        app.world_mut()
            .get_mut::<Combatant>(sentry)
            .expect("sentry")
            .damage(1., 0.);
        app.world_mut()
            .run_system_once(drive_opponents)
            .expect("aggro tick");
        assert!(app.world().resource::<CombatEncounter>().engaged);
    }

    #[test]
    fn a_bulwark_has_its_own_guard_and_a_rusher_the_tables() {
        let registries =
            strata::registry::load_from_dir(concat!(env!("CARGO_MANIFEST_DIR"), "/../../assets"))
                .expect("the shipped registries load");
        let tables = strata::sim::combat_tables::current();
        let (bulwark, _) = opponent_stats(staging(OpponentKind::Bulwark), &registries);
        let (rusher, _) = opponent_stats(staging(OpponentKind::Rusher), &registries);
        assert_eq!(
            bulwark.guard_row(Weapon::Blade),
            Some(tables.opponents.bulwark.guard)
        );
        assert_eq!(
            rusher.guard_row(Weapon::Blade),
            tables.melee.class(Weapon::Blade).map(|class| class.guard)
        );
    }

    #[test]
    fn a_reset_leaves_a_bulwark_its_own_guard() {
        let registries =
            strata::registry::load_from_dir(concat!(env!("CARGO_MANIFEST_DIR"), "/../../assets"))
                .expect("the shipped registries load");
        let tables = strata::sim::combat_tables::current();
        let (actor, sheet) = opponent_stats(staging(OpponentKind::Bulwark), &registries);
        let mut app = App::new();
        let mut keys = ButtonInput::<KeyCode>::default();
        keys.press(KeyCode::F5);
        app.insert_resource(keys)
            .insert_resource(CombatEncounter::default())
            .insert_resource(Fights::default())
            .insert_resource(Encounter::default())
            .insert_resource(strata::sim::Registries(std::sync::Arc::new(registries)));
        let bulwark = app
            .world_mut()
            .spawn((
                ResetPosition(WorldPos::new(0., 1., 12.)),
                WorldPosition::new(0., 1., 3.),
                PreviousWorldPosition::default(),
                CharacterController::default(),
                actor,
                CombatIntent::default(),
                Opponent {
                    passive: false,
                    brain: OpponentBrain::new(OpponentKind::Bulwark, &tables.opponents),
                },
                sheet,
            ))
            .id();
        app.world_mut().run_system_once(reset).expect("reset");
        let actor = app.world().get::<Combatant>(bulwark).expect("bulwark");
        assert_eq!(
            actor.guard_row(Weapon::Blade),
            Some(tables.opponents.bulwark.guard)
        );
    }

    #[test]
    fn hostiles_name_a_kind_and_a_distance() {
        let marksman = parse_hostile("marksman@60").expect("parses");
        assert_eq!(marksman.kind, OpponentKind::Marksman);
        assert!((marksman.metres() - 60.).abs() < f32::EPSILON);
        let duelist = parse_hostile("duelist").expect("parses");
        assert_eq!(duelist.metres, None);
        assert!(duelist.metres() >= 1.);
        let bulwark = parse_hostile("bulwark").expect("parses");
        assert_eq!(bulwark.kind, OpponentKind::Bulwark);
        assert!((bulwark.metres() - 12.).abs() < f32::EPSILON);
        let near = parse_hostile("bulwark@14").expect("parses");
        assert!((near.metres() - 14.).abs() < f32::EPSILON);
        for bad in ["archer", "marksman@", "marksman@-3", "duelist@nan", "@5"] {
            assert!(parse_hostile(bad).is_err(), "{bad}");
        }
    }
}
