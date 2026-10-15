use super::*;
use crate::combat_audio::tests::{cue_count, test_sounds};
use bevy::ecs::system::RunSystemOnce;
use strata_registry::Namespaced;
use strata_sim::ability::{Blast, ShotEffects};
use strata_sim::combat::{Falloff, GuardOutcome, Weapon};
use strata_sim::crafting::{CraftRefusal, CraftRequest, Rolled};
use strata_sim::deposits::{Deposit, DepositId, Met, PingResult, SurveyRefusal};
use strata_sim::fitting::{FitDone, FitRefusal};
use strata_sim::procurement::{Bought, Sold};
use strata_sim::ride::ThrownWhy;
use strata_sim::wear::Repaired;

fn app() -> App {
    let mut app = App::new();
    app.add_plugins(MinimalPlugins)
        .insert_resource(test_sounds());
    app.add_message::<PingAnswer>()
        .add_message::<SampleAnswer>()
        .add_message::<RideNotice>()
        .add_message::<HarvestAnswer>()
        .add_message::<CraftAnswer>()
        .add_message::<FitAnswer>()
        .add_message::<RepairAnswer>()
        .add_message::<SaleAnswer>()
        .add_message::<PurchaseAnswer>()
        .add_message::<CombatImpact>()
        .add_message::<crate::combat_fx::BlastWent>()
        .add_systems(
            Update,
            (
                survey_cues,
                ride_cues,
                harvest_cues,
                bench_cues,
                blast_cues,
                loops,
            ),
        );
    app.world_mut()
        .spawn((LocalPlayer, WorldPosition::new(0., 0., 0.)));
    app.update();
    app
}

fn id(text: &str) -> Namespaced {
    text.parse().expect("an id")
}

fn deposit() -> Deposit {
    Deposit {
        id: DepositId([0, 0]),
        ty: strata_registry::resource::ResourceTypeId(0),
        root: "Calder".to_owned(),
        place: "Calder flats".to_owned(),
        at: [5., 5.],
        height_m: 1.,
        figures: Vec::new(),
        radius_m: 30.,
        peak: 800,
        grade_pm: 500,
    }
}

fn ping(found: bool) -> PingAnswer {
    PingAnswer {
        class: id("strata:conductor"),
        outcome: Ok(PingDone::Met(PingResult {
            class: id("strata:conductor"),
            centre: [0., 0.],
            grid: 3,
            spacing_m: 10.,
            points: Vec::new(),
            found: if found {
                vec![Met {
                    deposit: deposit(),
                    pm: 700,
                    at: [5., 5.],
                    reading: strata_registry::Reading::Type,
                    richest: None,
                }]
            } else {
                Vec::new()
            },
        })),
    }
}

fn record() -> strata_registry::Record {
    strata_registry::Record {
        item: id("veyra:weapon.patrol_carbine"),
        count: 1,
        batch: 0,
        by: None,
        lines: std::collections::BTreeMap::new(),
        condition: None,
        fitted: std::collections::BTreeMap::new(),
        origin: None,
    }
}

#[test]
fn a_ping_begins_and_its_answer_says_found_or_empty_once_each() {
    let mut app = app();
    app.world_mut().spawn((
        WorldPosition::new(1., 0., 0.),
        Pinging {
            class: id("strata:conductor"),
            left: 2.,
        },
    ));
    app.update();
    app.update();
    assert_eq!(cue_count(app.world_mut(), Sound::SurveyPing), 1);
    app.world_mut().write_message(ping(false));
    app.update();
    assert_eq!(cue_count(app.world_mut(), Sound::SurveyEmpty), 1);
    assert_eq!(cue_count(app.world_mut(), Sound::SurveyFound), 0);
    app.world_mut().write_message(ping(true));
    // The channel starting is no answer to hear.
    app.world_mut().write_message(PingAnswer {
        class: id("strata:conductor"),
        outcome: Ok(PingDone::Started),
    });
    app.world_mut().write_message(PingAnswer {
        class: id("strata:conductor"),
        outcome: Err(SurveyRefusal::NoPlayer),
    });
    app.update();
    assert_eq!(cue_count(app.world_mut(), Sound::SurveyFound), 1);
    assert_eq!(cue_count(app.world_mut(), Sound::SurveyEmpty), 1);
}

#[test]
fn a_sample_cycle_sounds_and_a_start_does_not() {
    let mut app = app();
    let deposit = DepositId([0, 0]);
    app.world_mut().write_message(SampleAnswer {
        outcome: Ok(SampleDone::Started { deposit, pm: 500 }),
    });
    app.update();
    assert_eq!(cue_count(app.world_mut(), Sound::SampleDone), 0);
    app.world_mut().write_message(SampleAnswer {
        outcome: Ok(SampleDone::Cycle {
            deposit,
            units: 3,
            total: 3,
        }),
    });
    app.update();
    assert_eq!(cue_count(app.world_mut(), Sound::SampleDone), 1);
}

#[test]
fn the_ride_sounds_follow_the_notice_and_the_call() {
    let mut app = app();
    app.world_mut()
        .spawn((WorldPosition::new(2., 0., 0.), Calling { left: 1. }));
    for notice in [
        RideNotice::Mounted,
        RideNotice::Dismounted,
        RideNotice::Thrown {
            why: ThrownWhy::Crash,
            speed: 9.,
            condition: None,
        },
        RideNotice::Cancelled {
            why: strata_sim::ride::CancelWhy::Moved,
        },
    ] {
        app.world_mut().write_message(notice);
    }
    app.update();
    app.update();
    for sound in [
        Sound::BikeCall,
        Sound::BikeMount,
        Sound::BikeDismount,
        Sound::BikeThrown,
    ] {
        assert_eq!(cue_count(app.world_mut(), sound), 1, "{sound:?}");
    }
}

#[test]
fn a_harvester_plants_and_collects_and_a_refusal_is_silent() {
    let mut app = app();
    let outcomes = [
        Ok(HarvestDone::Planted {
            id: 0,
            deposit: DepositId([0, 0]),
            pm: 500,
        }),
        Ok(HarvestDone::Collected {
            id: 0,
            units: 4,
            left: 0,
        }),
        Ok(HarvestDone::Led { id: 0 }),
        Err(strata_sim::harvest::HarvestRefusal::NoPlayer),
    ];
    for outcome in outcomes {
        app.world_mut().write_message(HarvestAnswer { outcome });
    }
    app.update();
    assert_eq!(cue_count(app.world_mut(), Sound::HarvesterPlant), 1);
    assert_eq!(cue_count(app.world_mut(), Sound::HarvesterCollect), 1);
}

fn rolled(outcome: Outcome) -> CraftAnswer {
    CraftAnswer {
        request: CraftRequest::Finish,
        outcome: Ok(CraftDone::Rolled(Rolled {
            line: "damage".to_owned(),
            hand: strata_registry::schematic::Hand::Steady,
            outcome,
            from: 500,
            to: 550,
            points: 2,
        })),
    }
}

#[test]
fn a_roll_sounds_with_its_outcome_band_and_a_refused_request_is_silent() {
    assert_eq!(outcome_sound(Outcome::Exceptional), Sound::CraftGreat);
    assert_eq!(outcome_sound(Outcome::Good), Sound::CraftGood);
    assert_eq!(outcome_sound(Outcome::Fair), Sound::CraftGood);
    assert_eq!(outcome_sound(Outcome::Poor), Sound::CraftPoor);
    assert_eq!(outcome_sound(Outcome::Setback), Sound::CraftPoor);
    let mut app = app();
    app.world_mut().write_message(rolled(Outcome::Exceptional));
    app.world_mut().write_message(CraftAnswer {
        request: CraftRequest::Finish,
        outcome: Err(CraftRefusal::NoPlayer),
    });
    app.update();
    assert_eq!(cue_count(app.world_mut(), Sound::CraftRoll), 1);
    assert_eq!(cue_count(app.world_mut(), Sound::CraftGreat), 1);
    assert_eq!(cue_count(app.world_mut(), Sound::CraftGood), 0);
    app.world_mut().write_message(rolled(Outcome::Setback));
    app.world_mut().write_message(CraftAnswer {
        request: CraftRequest::Finish,
        outcome: Ok(CraftDone::Finished { product: record() }),
    });
    app.update();
    assert_eq!(cue_count(app.world_mut(), Sound::CraftPoor), 1);
    assert_eq!(cue_count(app.world_mut(), Sound::CraftFinish), 1);
}

#[test]
fn a_fit_a_repair_a_sale_and_a_purchase_each_sound_once_when_accepted() {
    let mut app = app();
    let fit = |outcome| FitAnswer {
        hand: 0,
        mount: "optic".to_owned(),
        outcome,
    };
    app.world_mut().write_message(fit(Ok(FitDone::Removed)));
    app.world_mut()
        .write_message(fit(Err(FitRefusal::NoPlayer)));
    app.world_mut().write_message(RepairAnswer {
        result: Ok(Repaired {
            gain: 10,
            to: 900,
            at_bench: true,
        }),
    });
    app.world_mut().write_message(SaleAnswer {
        result: Ok(Sold {
            buyer: id("veyra:buyer.pelagic"),
            line: "core".to_owned(),
            item: id("veyra:component.cell_core"),
            count: 1,
            paid: 10,
        }),
    });
    app.world_mut().write_message(PurchaseAnswer {
        result: Ok(Bought {
            buyer: id("veyra:buyer.pelagic"),
            item: id("veyra:component.cell_core"),
            paid: 10,
        }),
    });
    app.update();
    assert_eq!(cue_count(app.world_mut(), Sound::FitPart), 1);
    assert_eq!(cue_count(app.world_mut(), Sound::Repair), 1);
    assert_eq!(cue_count(app.world_mut(), Sound::Sale), 2);
}

fn bolt(owner: Entity, blast: bool) -> BlasterBolt {
    BlasterBolt {
        owner,
        team: 0,
        previous: WorldPos::new(0., 0., 0.),
        position: WorldPos::new(0., 0., 0.),
        velocity: Vec3::Z,
        damage: 10.,
        posture: 10.,
        remaining: 50.,
        flown: 0.,
        falloff: Falloff {
            full: 10.,
            far: 20.,
            share: 0.5,
        },
        weapon: Weapon::Carbine,
        effects: ShotEffects {
            blast: blast.then_some(Blast {
                radius: 4.,
                damage: 5.,
                posture: 5.,
            }),
            ..ShotEffects::default()
        },
    }
}

fn impact(attacker: Entity, guard: GuardOutcome) -> CombatImpact {
    CombatImpact {
        attacker,
        victim: None,
        from: WorldPos::new(0., 0., 0.),
        point: WorldPos::new(0., 0., 9.),
        melee: false,
        dealt: 0.,
        lethal: false,
        guard,
        shot: Some((Weapon::Carbine, 9.)),
    }
}

#[test]
fn a_blast_sounds_once_where_its_bolt_met_something_and_a_plain_bolt_is_dry() {
    let mut app = app();
    let owner = app.world_mut().spawn_empty().id();
    let plain = app.world_mut().spawn(bolt(owner, false)).id();
    app.update();
    app.world_mut().despawn(plain);
    app.world_mut()
        .write_message(impact(owner, GuardOutcome::Open));
    app.update();
    assert_eq!(cue_count(app.world_mut(), Sound::Blast), 0);
    let live = app.world_mut().spawn(bolt(owner, true)).id();
    app.update();
    // Still flying: an impact of another shot is not its blast.
    app.world_mut()
        .write_message(impact(owner, GuardOutcome::Open));
    app.update();
    assert_eq!(cue_count(app.world_mut(), Sound::Blast), 0);
    app.world_mut().despawn(live);
    // Two impacts of one blast (the struck and a splash victim): one sound.
    app.world_mut()
        .write_message(impact(owner, GuardOutcome::Open));
    app.world_mut()
        .write_message(impact(owner, GuardOutcome::Open));
    app.update();
    assert_eq!(cue_count(app.world_mut(), Sound::Blast), 1);
    // The ring is told where and how wide, once.
    let told: Vec<_> = app
        .world()
        .resource::<bevy::ecs::message::Messages<crate::combat_fx::BlastWent>>()
        .iter_current_update_messages()
        .copied()
        .collect();
    assert_eq!(told.len(), 1);
    assert!(
        (told[0].radius - 4.).abs() < f32::EPSILON,
        "{}",
        told[0].radius
    );
}

#[test]
fn a_loop_starts_with_its_component_and_stops_with_it() {
    let mut app = app();
    let member = app
        .world_mut()
        .query_filtered::<Entity, With<LocalPlayer>>()
        .single(app.world())
        .expect("member");
    app.world_mut().entity_mut(member).insert(Sampling {
        deposit: DepositId([0, 0]),
        from: [0., 0.],
        left: 3.,
        got: 0,
    });
    for _ in 0..3 {
        app.update();
    }
    assert_eq!(cue_count(app.world_mut(), Sound::SampleLoop), 1);
    {
        let world = app.world_mut();
        let settings = world
            .query_filtered::<&PlaybackSettings, With<LoopOf>>()
            .single(world)
            .expect("the loop");
        assert!(matches!(settings.mode, bevy::audio::PlaybackMode::Loop));
    }
    app.world_mut().entity_mut(member).remove::<Sampling>();
    app.update();
    app.update();
    assert_eq!(cue_count(app.world_mut(), Sound::SampleLoop), 0);
}

#[test]
fn a_harvester_hums_while_it_carries_its_emitter_and_is_within_earshot() {
    let mut app = app();
    let near = app
        .world_mut()
        .spawn((
            WorldPosition::new(10., 0., 0.),
            LoopEmitter(EmitterSound::HarvesterHum),
        ))
        .id();
    app.world_mut().spawn((
        WorldPosition::new(500., 0., 0.),
        LoopEmitter(EmitterSound::HarvesterHum),
    ));
    for _ in 0..3 {
        app.update();
    }
    assert_eq!(cue_count(app.world_mut(), Sound::HarvesterHum), 1);
    app.world_mut().entity_mut(near).remove::<LoopEmitter>();
    app.update();
    app.update();
    assert_eq!(cue_count(app.world_mut(), Sound::HarvesterHum), 0);
}

#[cfg(feature = "ui")]
#[test]
fn the_bike_hums_while_ridden_and_its_pitch_follows_its_speed() {
    let mut app = app();
    let registries = crate::ui::testing::shipped();
    let def = registries
        .vehicles
        .get(&id("veyra:vehicle.courier_bike"))
        .expect("the shipped bike");
    let member = app
        .world_mut()
        .query_filtered::<Entity, With<LocalPlayer>>()
        .single(app.world())
        .expect("member");
    let riding = Riding::new(def, 0.);
    let top = riding.dials.speed.road.max(riding.dials.speed.ground);
    app.world_mut().entity_mut(member).insert(riding);
    for _ in 0..3 {
        app.update();
    }
    assert_eq!(cue_count(app.world_mut(), Sound::BikeHum), 1);
    assert!((hum_speed(0., top) - HUM_SPEED.0).abs() < 1e-6);
    assert!((hum_speed(top, top) - HUM_SPEED.1).abs() < 1e-6);
    assert!((hum_speed(-top * 3., top) - HUM_SPEED.1).abs() < 1e-6);
    let half = hum_speed(top / 2., top);
    assert!(half > HUM_SPEED.0 && half < HUM_SPEED.1);
    app.world_mut().entity_mut(member).remove::<Riding>();
    app.update();
    app.update();
    assert_eq!(cue_count(app.world_mut(), Sound::BikeHum), 0);
    app.world_mut()
        .run_system_once(hum_pitch)
        .expect("pitch runs without sinks");
}
