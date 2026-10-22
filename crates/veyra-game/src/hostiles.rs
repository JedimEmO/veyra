//! The people of a fight: what a hostile is born as and how it is made, shared
//! by the staging fight in the game binary and the camps and posts the sim
//! stands (`camps`). Headless: stats, a brain, a motor and the marks that say
//! which encounter it fights in. The marks a window draws (the marksman's
//! beam, the bulwark's slab) are plain markers the view reads.

use bevy::ecs::system::EntityCommands;
use bevy::prelude::*;
use strata::core::WorldPos;
use strata::physics::CharacterController;
use strata::prelude::*;
use strata::registry::GameRegistries;
use strata::sim::combat::{Combatant, Weapon};
use strata::sim::combat_ai::{OpponentBrain, OpponentKind};
use strata::sim::encounters::{Allegiance, EncounterId, hostility};
use strata::sim::progression::CharacterSheet;

/// An encounter opponent: whether it is the training target, and the
/// shipped AI (`veyra_sim::combat_ai`) that fights once it is released.
#[derive(Component)]
pub struct Opponent {
    /// The training target: it stands and does nothing.
    pub passive: bool,
    /// The shipped AI that fights once the encounter releases it.
    pub brain: OpponentBrain,
}

/// Where the staging fight stands when the household does not start beside
/// it: the default spawn and its heading, which the fight is placed from and
/// F5 returns the player to (ticket 0014, a new household starts on the quay).
#[derive(Resource, Debug, Clone, Copy)]
pub struct StagingSpawn {
    /// Where it stands.
    pub at: WorldPos,
    /// The heading it is placed from, radians.
    pub yaw: f32,
}

/// What a hostile is born as: the staging fight's people and a camp's are
/// made by [`spawn_hostile`] the same way and differ only in this.
#[derive(Debug, Clone, Copy)]
pub struct Birth {
    /// The training target: it stands and does nothing.
    pub passive: bool,
    /// Which kind of opponent it is.
    pub kind: OpponentKind,
    /// 0 for the staging fight, which `opponents.ron` states as written.
    pub tier: u8,
    /// The encounter it fights in.
    pub id: EncounterId,
    /// Whose side it is on.
    pub allegiance: Allegiance,
}

/// Makes one hostile at `position`: stats, brain, motor and the marks that
/// say which encounter it fights in.
pub fn spawn_hostile<'a>(
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
        opponent.insert(strata::sim::view::AimBeam);
    }
    if kind == OpponentKind::Bulwark && !passive {
        opponent.insert(strata::sim::view::Shield);
    }
    opponent
}

/// An opponent's pools and pressure, from `assets/combat/opponents.ron`,
/// and its moveset from its sheet in `assets/progression/opponents.ron`
/// (the rusher's One hand 2, the sentry's Carbine 2), fitted over the tables
/// the way the player's is. The passive target has an empty sheet: it
/// does nothing, so it may do nothing.
pub fn opponent_stats(birth: Birth, registries: &GameRegistries) -> (Combatant, CharacterSheet) {
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

/// Search only beside the city's western terraces and outside building bounds.
pub fn default_spawn(
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
