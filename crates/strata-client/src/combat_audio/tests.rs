use super::*;
use bevy::audio::Decodable;
use bevy::ecs::system::RunSystemOnce;

pub(super) fn test_sounds() -> Sounds {
    Sounds {
        handles: EVENTS
            .iter()
            .map(|event| vec![Handle::default(); usize::from(event.takes)])
            .collect(),
        last: vec![None; EVENTS.len()],
        rng: Rng::new(42),
        interface: 0.8,
    }
}

/// One-shots authored with one take.
const SINGLE_TAKE: &[&str] = &[
    "survey_empty",
    "bike_call",
    "harvester_plant",
    "craft_great",
    "repair",
    "map_open",
];

#[test]
fn every_event_has_required_pcm_takes_that_bevy_decodes() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets");
    for event in EVENTS {
        // A few of the field's one-shots were made with a single take;
        // the rotation rule needs two only where there are two.
        let minimum = if event.looping || SINGLE_TAKE.contains(&event.name) {
            1
        } else {
            2
        };
        assert!(
            event.takes >= minimum,
            "{} needs {minimum} takes",
            event.name
        );
        for take in 0..event.takes {
            let path = root.join(event.path(take));
            let bytes = std::fs::read(&path).expect("table entry must exist on disk");
            assert_eq!(&bytes[0..4], b"RIFF", "{}", path.display());
            assert_eq!(&bytes[8..12], b"WAVE");
            let mut chunks = &bytes[12..];
            let mut format = None;
            let mut data = None;
            while chunks.len() >= 8 {
                let size =
                    u32::from_le_bytes(chunks[4..8].try_into().expect("chunk size")) as usize;
                assert!(size <= chunks.len() - 8, "truncated WAV");
                match &chunks[..4] {
                    b"fmt " => format = Some(&chunks[8..8 + size]),
                    b"data" => data = Some(&chunks[8..8 + size]),
                    _ => {}
                }
                chunks = &chunks[(8 + size + size % 2).min(chunks.len())..];
            }
            let format = format.expect("PCM format chunk");
            assert!(format.len() >= 16);
            assert_eq!(u16::from_le_bytes([format[0], format[1]]), 1, "PCM");
            assert_eq!(u16::from_le_bytes([format[2], format[3]]), 1, "mono");
            assert_eq!(
                u32::from_le_bytes(format[4..8].try_into().expect("rate")),
                48_000
            );
            assert_eq!(u16::from_le_bytes([format[14], format[15]]), 16);
            let pcm = data.expect("audio data");
            assert!(!pcm.is_empty() && pcm.len().is_multiple_of(2));
            let source = AudioSource {
                bytes: bytes.clone().into(),
            };
            assert_eq!(
                source.decoder().count(),
                pcm.len() / 2,
                "{}",
                path.display()
            );
        }
    }
}

#[test]
fn selection_never_repeats_and_pitch_stays_inside_each_events_range() {
    let mut rng = Rng::new(77);
    for event in EVENTS {
        let mut last = None;
        let mut seen = std::collections::HashSet::new();
        for _ in 0..100 {
            let take = choose_take(&mut rng, event.takes, last);
            if event.takes > 1 {
                assert_ne!(Some(take), last);
            }
            assert!(take < event.takes);
            seen.insert(take);
            last = Some(take);
        }
        assert_eq!(seen.len(), usize::from(event.takes));
    }
    let mut sounds = test_sounds();
    for _ in 0..100 {
        let (_, speed) = sounds.choose(Sound::BlasterShot);
        assert!((0.97..=1.03).contains(&speed));
    }
}

#[test]
fn run_contacts_follow_forward_reverse_and_wrapped_phase() {
    assert_eq!(foot_contacts(0., 0.14, 0.8), 0);
    assert_eq!(foot_contacts(0.14, 0.16, 0.8), 1);
    assert_eq!(foot_contacts(0.54, 0.56, 0.8), 1);
    assert_eq!(foot_contacts(0.79, 0.96, 0.8), 1);
    assert_eq!(foot_contacts(0.56, 0.54, 0.8), 1);
    assert_eq!(foot_contacts(0., -0.3, 0.8), 1);
    assert_eq!(foot_contacts(0.1, 0.6, 0.8), 2);
    assert_eq!(foot_contacts(0.1, 1.0, 0.8), 3);
    assert_eq!(foot_contacts(0.2, 0.2, 0.8), 0);
}

#[test]
fn impact_action_and_terrain_routing() {
    assert_eq!(impact_sound(false, false, false, None), Sound::ImpactStone);
    assert_eq!(
        impact_sound(false, false, false, Some("metal")),
        Sound::ImpactMetal
    );
    assert_eq!(impact_sound(false, true, false, None), Sound::ImpactFlesh);
    assert_eq!(impact_sound(false, true, true, None), Sound::ImpactShield);
    assert_eq!(impact_sound(true, true, false, None), Sound::BladeHitArmor);
    assert_eq!(impact_sound(true, true, true, None), Sound::BladeHitShield);
    assert_eq!(
        action_sound(Action::Heavy, false),
        Some(Sound::RusherWindup)
    );
    assert_eq!(action_sound(Action::Heavy, true), Some(Sound::SwingHeavy));
    assert_eq!(foot_surface(Some("metal"), false), Sound::FootMetal);
    assert_eq!(foot_surface(Some("setts"), false), Sound::FootStone);
    assert_eq!(foot_surface(None, true), Sound::FootStone);
    assert_eq!(foot_surface(None, false), Sound::FootGravel);
}

#[test]
fn low_health_starts_once_and_stops_on_recovery_death_or_removal() {
    let mut world = World::new();
    world.insert_resource(test_sounds());
    let mut actor = Combatant::default();
    actor.health = 24.;
    let player = world.spawn((LocalPlayer, actor)).id();
    for _ in 0..3 {
        world
            .run_system_once(low_health)
            .expect("low health system");
    }
    assert_eq!(
        world
            .query_filtered::<Entity, With<LowHealth>>()
            .iter(&world)
            .count(),
        1
    );
    let settings = world
        .query_filtered::<&PlaybackSettings, With<LowHealth>>()
        .single(&world)
        .expect("low-health loop");
    assert!(matches!(settings.mode, bevy::audio::PlaybackMode::Loop));
    assert!(!settings.spatial);
    for (health, expected) in [(25., 0), (0., 0), (10., 1)] {
        world.get_mut::<Combatant>(player).expect("player").health = health;
        world
            .run_system_once(low_health)
            .expect("low health system");
        assert_eq!(
            world
                .query_filtered::<Entity, With<LowHealth>>()
                .iter(&world)
                .count(),
            expected
        );
    }
    world.despawn(player);
    world
        .run_system_once(low_health)
        .expect("low health system");
    assert_eq!(
        world
            .query_filtered::<Entity, With<LowHealth>>()
            .iter(&world)
            .count(),
        0
    );
}

pub(super) fn cue_count(world: &mut World, sound: Sound) -> usize {
    world
        .query::<&Playing>()
        .iter(world)
        .filter(|cue| cue.sound == sound)
        .count()
}

#[test]
fn warnings_recharge_and_hurt_are_edges_not_render_frame_repeats() {
    let mut app = App::new();
    app.insert_resource(test_sounds())
        .add_message::<CombatImpact>()
        .add_systems(Update, cues);
    let mut actor = Combatant::default();
    actor.shield = 10.;
    actor.hurts = 1;
    let player = app
        .world_mut()
        .spawn((
            LocalPlayer,
            actor,
            WorldPosition::new(0., 0., 0.),
            CharacterController {
                grounded: true,
                ..default()
            },
        ))
        .id();
    let enemy = app
        .world_mut()
        .spawn((Combatant::on_team(1, 85.), WorldPosition::new(3., 0., 0.)))
        .id();
    app.update();
    app.world_mut()
        .get_mut::<Combatant>(enemy)
        .expect("enemy")
        .charge = 0.05;
    for _ in 0..3 {
        app.update();
    }
    assert_eq!(cue_count(app.world_mut(), Sound::SentryCharge), 1);
    {
        let world = app.world_mut();
        let (settings, cue) = world
            .query::<(&PlaybackSettings, &Playing)>()
            .iter(world)
            .next()
            .expect("warning");
        assert!(settings.spatial);
        assert_eq!(
            settings.duration,
            Some(std::time::Duration::from_millis(650))
        );
        assert_eq!(cue.follow, Some(enemy));
    }
    app.world_mut()
        .get_mut::<Combatant>(enemy)
        .expect("enemy")
        .charge = 0.;
    app.update();
    assert_eq!(cue_count(app.world_mut(), Sound::SentryCharge), 0);
    for shield in [11., 11., 12., 12., 13.] {
        app.world_mut()
            .get_mut::<Combatant>(player)
            .expect("player")
            .shield = shield;
        app.update();
    }
    assert_eq!(cue_count(app.world_mut(), Sound::ShieldRecharge), 1);
    {
        let mut actor = app
            .world_mut()
            .get_mut::<Combatant>(player)
            .expect("player");
        actor.shield = 8.;
        actor.hurts += 1;
    }
    app.update();
    app.update();
    assert_eq!(cue_count(app.world_mut(), Sound::PlayerHurt), 1);
    app.world_mut()
        .get_mut::<Combatant>(player)
        .expect("player")
        .shield = 9.;
    app.update();
    assert_eq!(cue_count(app.world_mut(), Sound::ShieldRecharge), 2);
    // Encounter reset restores shield and resets the hit count, without a recharge cue.
    *app.world_mut()
        .get_mut::<Combatant>(player)
        .expect("player") = Combatant::default();
    app.update();
    assert_eq!(cue_count(app.world_mut(), Sound::ShieldRecharge), 2);
    app.world_mut()
        .get_mut::<Combatant>(enemy)
        .expect("enemy")
        .action = Action::Dead;
    app.update();
    app.update();
    assert_eq!(cue_count(app.world_mut(), Sound::EnemyDeath), 1);
}

#[test]
fn launches_and_impacts_keep_local_actions_dry_and_rebase_world_sources() {
    let mut app = App::new();
    app.insert_resource(test_sounds())
        .init_resource::<RenderOrigin>()
        .add_message::<CombatImpact>()
        .add_message::<ShotFired>()
        .add_systems(Update, (launches, cues, position_sources).chain());
    let player = app
        .world_mut()
        .spawn((
            LocalPlayer,
            Combatant::default(),
            WorldPosition::new(0., 0., 0.),
        ))
        .id();
    let enemy = app
        .world_mut()
        .spawn((Combatant::on_team(1, 85.), WorldPosition::new(3., 0., 0.)))
        .id();
    app.update();
    for attacker in [player, enemy] {
        app.world_mut().write_message(ShotFired {
            attacker,
            from: WorldPos::new(3., 1., 0.),
            direction: Vec3::Z,
        });
    }
    app.world_mut()
        .get_mut::<Combatant>(enemy)
        .expect("enemy")
        .shield -= 10.;
    app.world_mut().write_message(CombatImpact {
        attacker: player,
        victim: Some(enemy),
        from: WorldPos::new(0., 0., 0.),
        point: WorldPos::new(3., 1., 0.),
        melee: true,
        dealt: 28.,
        lethal: false,
        guard: strata_sim::combat::GuardOutcome::Open,
        shot: None,
    });
    app.update();
    for (settings, playing, transform) in app
        .world_mut()
        .query::<(&PlaybackSettings, &Playing, &Transform)>()
        .iter(app.world())
    {
        let spatial = matches!(playing.sound, Sound::SentryShot | Sound::BladeHitShield);
        assert_eq!(settings.spatial, spatial);
        if spatial {
            assert_eq!(transform.translation, Vec3::new(3., 1., 0.));
        }
    }
    assert_eq!(cue_count(app.world_mut(), Sound::BlasterShot), 1);
    assert_eq!(cue_count(app.world_mut(), Sound::SentryShot), 1);
    assert_eq!(cue_count(app.world_mut(), Sound::BladeHitShield), 1);
    assert_eq!(cue_count(app.world_mut(), Sound::HitMarker), 1);
    app.world_mut().resource_mut::<RenderOrigin>().0 = WorldPos::new(100., 0., 0.);
    app.update();
    for (playing, transform) in app
        .world_mut()
        .query::<(&Playing, &Transform)>()
        .iter(app.world())
    {
        if playing.point.is_some() {
            assert_eq!(transform.translation, Vec3::new(-97., 1., 0.));
        }
    }
}

#[test]
fn a_block_and_a_parry_sound_unlike_each_other_and_unlike_a_landed_hit() {
    let landed: Vec<Sound> = [true, false]
        .into_iter()
        .flat_map(|melee| [true, false].map(|shield| impact_sound(melee, true, shield, None)))
        .collect();
    for sound in &landed {
        let blocked = guard_sound(GuardOutcome::Blocked, *sound);
        let parried = guard_sound(GuardOutcome::Parried, *sound);
        assert!(!landed.contains(&blocked) && !landed.contains(&parried));
        assert_eq!(guard_sound(GuardOutcome::Open, *sound), *sound);
    }
    assert_eq!(guard_layer(GuardOutcome::Blocked), None);
    assert_eq!(guard_layer(GuardOutcome::Broken), Some(Sound::GuardBreak));
    assert!(guard_layer(GuardOutcome::Parried).is_some());
}
