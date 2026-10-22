//! The adapter half of the `progression` rules: its requests, systems and plugin.

pub use veyra_rules::progression::*;

use bevy::app::{App, Plugin};

use bevy::ecs::prelude::*;

use strata_registry::progression::XpKind;

use crate::combat::{CombatImpact, Weapon};

use crate::{LocalPlayer, Registries, SimSet, SimTick, WorldPosition};

/// The ledger: every confirmed hit credits its attacker's sheet with the
/// damage it dealt in the kind of the weapon that dealt it (a bolt is
/// ranged, a blade is blade), and a killing hit the graph's kill bonus on
/// top. An attacker with no sheet earns nothing; an opponent's sheet earns
/// like anyone's.
pub fn credit_xp(
    registries: Option<Res<Registries>>,
    mut impacts: MessageReader<CombatImpact>,
    mut sheets: Query<&mut CharacterSheet>,
    mut gained: MessageWriter<XpGained>,
) {
    let Some(registries) = registries else {
        return;
    };
    let rates = &registries.0.progression.rates;
    for impact in impacts.read() {
        if impact.victim.is_none() || impact.dealt <= 0. {
            continue;
        }
        let Ok(mut sheet) = sheets.get_mut(impact.attacker) else {
            continue;
        };
        let kind = if impact.melee {
            XpKind::Blade
        } else {
            XpKind::Ranged
        };
        #[allow(
            clippy::cast_possible_truncation,
            clippy::cast_sign_loss,
            reason = "damage is bounded and non-negative; XP is whole points"
        )]
        let mut amount = (impact.dealt * rates.damage).round() as u32;
        if impact.lethal {
            amount += rates.kill;
        }
        if amount > 0 {
            sheet.credit(kind, amount);
            gained.write(XpGained {
                entity: impact.attacker,
                kind,
                amount,
                kill: impact.lethal,
            });
        }
        // A rifle's shooting also teaches the wild: its damage, and a bonus
        // for a hit from far off, beside the combat XP of the same hit.
        if let Some((Weapon::Rifle, flown)) = impact.shot {
            #[allow(
                clippy::cast_possible_truncation,
                clippy::cast_sign_loss,
                reason = "damage is bounded and non-negative; XP is whole points"
            )]
            let mut wild = (impact.dealt * rates.wilderness.rifle).round() as u32;
            if flown >= rates.wilderness.long_m {
                wild += rates.wilderness.long;
            }
            if wild > 0 {
                sheet.credit(XpKind::Wilderness, wild);
                gained.write(XpGained {
                    entity: impact.attacker,
                    kind: XpKind::Wilderness,
                    amount: wild,
                    kill: false,
                });
            }
        }
    }
}

/// The nearest prop within reach of the local player, every tick, so the
/// prompt and the learn rule read the same answer.
pub fn find_reach(
    players: Query<&WorldPosition, With<LocalPlayer>>,
    props: Query<(Entity, &WorldPosition, &Interaction)>,
    mut reach: ResMut<Reach>,
) {
    let mut next = Reach::default();
    if let Ok(player) = players.single() {
        let mut best: Option<(f64, Entity, &Interaction)> = None;
        for (entity, position, prop) in &props {
            let delta = position.0.0 - player.0.0;
            let distance = delta.x.hypot(delta.z);
            if distance > f64::from(prop.radius) {
                continue;
            }
            match &prop.kind {
                InteractionKind::Trainer { trees } => next.trees.extend(trees.iter().cloned()),
                InteractionKind::Workbench => next.workbench = true,
                InteractionKind::Fabricator => next.fabricator = true,
                InteractionKind::Board { buyer } => next.board = Some(buyer.clone()),
                InteractionKind::Guard { faction } => {
                    next.guards.insert(*faction);
                }
                InteractionKind::Person { id } => {
                    next.people.insert(id.clone());
                }
                InteractionKind::Terminal
                | InteractionKind::Locker
                | InteractionKind::Harvester { .. }
                | InteractionKind::Cache { .. } => {}
            }
            if best.is_none_or(|(d, _, _)| distance < d) {
                best = Some((distance, entity, prop));
            }
        }
        if let Some((_, entity, prop)) = best {
            next.nearest = Some((entity, prop.kind.clone()));
        }
    }
    // Written only on a change, so a reader's change detection means one.
    if *reach != next {
        *reach = next;
    }
}

/// Registers the ledger and the reach. Added by `SimPlugin`.
#[derive(Debug, Clone, Copy)]
pub struct ProgressionPlugin;

impl Plugin for ProgressionPlugin {
    fn build(&self, app: &mut App) {
        app.add_message::<XpGained>()
            .add_message::<Interact>()
            .init_resource::<Reach>()
            .add_systems(SimTick, (credit_xp, find_reach).in_set(SimSet::Observe));
    }
}

#[cfg(test)]
mod tests {
    #![allow(
        clippy::float_cmp,
        reason = "the design's numbers are set exactly; a bit off is a wrong table"
    )]

    use super::*;
    use bevy::ecs::system::RunSystemOnce;

    use crate::combat::Combatant;

    use bevy::app::App;
    use std::sync::Arc;

    use strata_registry::progression::SkillGraph;
    use strata_registry::progression::XpKind;

    use crate::combat::CombatImpact;
    use crate::combat::Weapon;

    use crate::LocalPlayer;
    use crate::Registries;

    use crate::WorldPosition;

    pub(crate) use veyra_rules::progression::test_support::*;

    #[test]
    fn reach_finds_the_nearest_prop_and_only_a_trainer_of_the_tree_teaches() {
        let graph = SkillGraph::shipped();
        let mut world = World::new();
        world.init_resource::<Reach>();
        let registries = strata_registry::GameRegistries {
            progression: graph.clone(),
            ..strata_registry::GameRegistries::default()
        };
        world.insert_resource(Registries(Arc::new(registries)));
        world.spawn((LocalPlayer, WorldPosition::new(0., 1., 0.)));
        let terminal = world
            .spawn((
                WorldPosition::new(2., 1., 0.),
                Interaction {
                    kind: InteractionKind::Terminal,
                    radius: 3.,
                },
            ))
            .id();
        world.spawn((
            WorldPosition::new(0., 1., 2.5),
            Interaction {
                kind: InteractionKind::Trainer {
                    trees: vec!["blade".into()],
                },
                radius: 3.,
            },
        ));
        world.spawn((
            WorldPosition::new(0., 1., 20.),
            Interaction {
                kind: InteractionKind::Trainer {
                    trees: vec!["marksman".into()],
                },
                radius: 3.,
            },
        ));
        world.run_system_once(find_reach).expect("reach");
        let reach = world.resource::<Reach>();
        assert_eq!(
            reach.nearest.as_ref().map(|(e, k)| (*e, k.clone())),
            Some((terminal, InteractionKind::Terminal))
        );
        assert!(reach.teaches(&graph, "blade.cuts.1"));
        assert!(!reach.teaches(&graph, "marksman.pistol.1"), "out of reach");
        let mut sheet = CharacterSheet::default();
        sheet.learned.insert("marksman.novice".into());
        sheet.credit(XpKind::Ranged, 1000);
        assert_eq!(
            sheet.can_learn(
                &graph,
                "marksman.pistol.1",
                reach.teaches(&graph, "marksman.pistol.1")
            ),
            Err(LearnRefused::NoTeacher)
        );
        // Walk away: nothing within reach, and the prompt goes.
        world
            .query::<&mut WorldPosition>()
            .iter_mut(&mut world)
            .next()
            .expect("player")
            .0 = strata_core::WorldPos::new(100., 1., 100.);
        world.run_system_once(find_reach).expect("reach");
        assert_eq!(*world.resource::<Reach>(), Reach::default());
    }

    /// A workbench stands beside a locker. Midway between them, a step
    /// nearer the locker, the prompt is the locker's and the bench is in
    /// reach all the same, so fitting does not turn on which is closer.
    #[test]
    fn a_workbench_is_in_reach_beside_a_nearer_locker() {
        let mut world = World::new();
        world.init_resource::<Reach>();
        let player = world
            .spawn((LocalPlayer, WorldPosition::new(1.5, 1., 0.)))
            .id();
        let prop = |kind| Interaction { kind, radius: 2.5 };
        let locker = world
            .spawn((
                WorldPosition::new(0., 1., 0.),
                prop(InteractionKind::Locker),
            ))
            .id();
        let bench = world
            .spawn((
                WorldPosition::new(3.5, 1., 0.),
                prop(InteractionKind::Workbench),
            ))
            .id();
        let mut reach_at = |x: f64| {
            world.get_mut::<WorldPosition>(player).expect("player").0 =
                strata_core::WorldPos::new(x, 1., 0.);
            world.run_system_once(find_reach).expect("reach");
            world.resource::<Reach>().clone()
        };
        let midway = reach_at(1.5);
        assert_eq!(midway.nearest, Some((locker, InteractionKind::Locker)));
        assert!(midway.workbench);
        // At the bench it is the prompt as well.
        let at = reach_at(3.5);
        assert_eq!(at.nearest, Some((bench, InteractionKind::Workbench)));
        assert!(at.workbench);
        // At the locker's far side the bench is out of reach.
        let far = reach_at(0.5);
        assert_eq!(far.nearest, Some((locker, InteractionKind::Locker)));
        assert!(!far.workbench);
        assert!(far.trees.is_empty() && at.trees.is_empty());
    }

    #[test]
    fn a_parried_hit_pays_the_attacker_nothing() {
        let mut app = crate::tests::app_in_world();
        let graph = Arc::new({
            strata_registry::GameRegistries {
                progression: graph(),
                ..strata_registry::GameRegistries::default()
            }
        });
        app.insert_resource(Registries(graph));
        let attacker = app
            .world_mut()
            .spawn((Combatant::default(), CharacterSheet::default()))
            .id();
        let victim = app.world_mut().spawn(Combatant::on_team(1, 30.)).id();
        let point = strata_core::WorldPos::ZERO;
        app.world_mut().write_message(CombatImpact {
            attacker,
            victim: Some(victim),
            from: point,
            point,
            melee: true,
            dealt: 0.,
            lethal: false,
            guard: crate::combat::GuardOutcome::Parried,
            shot: None,
        });
        app.update();
        let sheet = app.world().get::<CharacterSheet>(attacker).expect("sheet");
        assert_eq!(sheet.xp(XpKind::Blade), 0);
        assert_eq!(sheet.xp(XpKind::Ranged), 0);
    }

    #[test]
    fn a_rifle_hit_pays_wilderness_xp_and_a_pistol_hit_none() {
        let mut app = crate::tests::app_in_world();
        let registries = Arc::new({
            strata_registry::GameRegistries {
                progression: graph(),
                ..strata_registry::GameRegistries::default()
            }
        });
        let rates = registries.progression.rates.wilderness.clone();
        assert!(rates.rifle > 0. && rates.long > 0 && rates.long_m > 0.);
        app.insert_resource(Registries(registries));
        let attacker = app
            .world_mut()
            .spawn((Combatant::default(), CharacterSheet::default()))
            .id();
        let victim = app.world_mut().spawn(Combatant::on_team(1, 30.)).id();
        let point = strata_core::WorldPos::ZERO;
        let hit = |app: &mut App, dealt: f32, shot: Option<(Weapon, f32)>| {
            app.world_mut().write_message(CombatImpact {
                attacker,
                victim: Some(victim),
                from: point,
                point,
                melee: false,
                dealt,
                lethal: false,
                guard: crate::combat::GuardOutcome::Open,
                shot,
            });
            app.update();
        };
        let wild = |app: &App| {
            app.world()
                .get::<CharacterSheet>(attacker)
                .expect("sheet")
                .xp(XpKind::Wilderness)
        };
        // A pistol bolt from as far as it flies, and a carbine's, pay none.
        hit(&mut app, 24., Some((Weapon::Pistol, 85.)));
        hit(&mut app, 24., Some((Weapon::Carbine, 60.)));
        assert_eq!(wild(&app), 0);
        // A rifle hit from close pays its damage at the rate.
        hit(&mut app, 85., Some((Weapon::Rifle, 10.)));
        #[allow(
            clippy::cast_possible_truncation,
            clippy::cast_sign_loss,
            reason = "a few dozen points"
        )]
        let near = (85. * rates.rifle).round() as u32;
        assert_eq!(wild(&app), near);
        // From the distance it adds the bonus.
        hit(&mut app, 85., Some((Weapon::Rifle, rates.long_m)));
        assert_eq!(wild(&app), near * 2 + rates.long);
        // A rifle shot that struck scenery deals nothing and pays the bonus
        // of no one: only a hit that did damage counts.
        let before = wild(&app);
        app.world_mut().write_message(CombatImpact {
            attacker,
            victim: None,
            from: point,
            point,
            melee: false,
            dealt: 0.,
            lethal: false,
            guard: crate::combat::GuardOutcome::Open,
            shot: Some((Weapon::Rifle, 200.)),
        });
        app.update();
        assert_eq!(wild(&app), before);
        // The combat XP is paid beside it, as it always was.
        let ranged = app
            .world()
            .get::<CharacterSheet>(attacker)
            .expect("sheet")
            .xp(XpKind::Ranged);
        assert_eq!(ranged, 24 + 24 + 85 + 85);
    }

    #[test]
    fn a_hit_credits_damage_and_a_kill_the_bonus() {
        let mut app = crate::tests::app_in_world();
        let graph = Arc::new({
            strata_registry::GameRegistries {
                progression: graph(),
                ..strata_registry::GameRegistries::default()
            }
        });
        app.insert_resource(Registries(graph));
        let attacker = app
            .world_mut()
            .spawn((Combatant::default(), CharacterSheet::default()))
            .id();
        let victim = app.world_mut().spawn(Combatant::on_team(1, 30.)).id();
        let point = strata_core::WorldPos::ZERO;
        app.world_mut().write_message(CombatImpact {
            attacker,
            victim: Some(victim),
            from: point,
            point,
            melee: false,
            dealt: 24.,
            lethal: false,
            guard: crate::combat::GuardOutcome::Open,
            shot: None,
        });
        app.world_mut().write_message(CombatImpact {
            attacker,
            victim: Some(victim),
            from: point,
            point,
            melee: true,
            dealt: 34.,
            lethal: true,
            guard: crate::combat::GuardOutcome::Open,
            shot: None,
        });
        app.world_mut().write_message(CombatImpact {
            attacker,
            victim: None,
            from: point,
            point,
            melee: false,
            dealt: 0.,
            lethal: false,
            guard: crate::combat::GuardOutcome::Open,
            shot: None,
        });
        app.update();
        let sheet = app.world().get::<CharacterSheet>(attacker).expect("sheet");
        assert_eq!(sheet.xp(XpKind::Ranged), 24);
        assert_eq!(sheet.xp(XpKind::Blade), 34 + 50);
        let gained: Vec<XpGained> = app
            .world()
            .resource::<Messages<XpGained>>()
            .iter_current_update_messages()
            .copied()
            .collect();
        assert_eq!(
            gained,
            [
                XpGained {
                    entity: attacker,
                    kind: XpKind::Ranged,
                    amount: 24,
                    kill: false
                },
                XpGained {
                    entity: attacker,
                    kind: XpKind::Blade,
                    amount: 84,
                    kill: true
                }
            ]
        );
    }
}
