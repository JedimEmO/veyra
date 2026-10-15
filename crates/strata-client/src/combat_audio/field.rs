//! The field's sounds: surveying, the bike, a harvester, a weapon drawn, a
//! guard broken and a blast, and the bench's and the stores' moments.
//!
//! Each one hangs on a thing the sim already raises (an answer message, a
//! component arriving, a notice), never on a key, so a refused request is
//! silent and a server's answer sounds the same as a local one. The three
//! loops are kept by one rule: a loop plays for as long as its owner carries
//! the component that makes it (`Sampling`, `Riding`, a [`LoopEmitter`]) and
//! is despawned the frame after it goes.

use bevy::audio::{AudioSinkPlayback, SpatialAudioSink};
use bevy::prelude::*;
use strata_core::WorldPos;
use strata_registry::schematic::Outcome;
use strata_sim::LocalPlayer;
use strata_sim::WorldPosition;
use strata_sim::combat::{BlasterBolt, CombatImpact};
use strata_sim::crafting::{CraftAnswer, CraftDone};
use strata_sim::deposits::{PingAnswer, PingDone, Pinging, SampleAnswer, SampleDone, Sampling};
use strata_sim::fitting::FitAnswer;
use strata_sim::harvest::{HarvestAnswer, HarvestDone};
use strata_sim::procurement::{PurchaseAnswer, SaleAnswer};
use strata_sim::ride::{Calling, RideNotice, Riding};
use strata_sim::wear::RepairAnswer;

use super::{Sound, Sounds, play};

/// Metres within which a harvester's hum is played at all: past it the
/// spatial falloff has made it inaudible, and a field of machines would
/// hold a sink each for nothing.
const EARSHOT_M: f64 = 60.;

/// The bike hum's playback speed standing and at its top speed. The pitch
/// climbs with the bike, and a wider spread than this reads as a siren.
const HUM_SPEED: (f32, f32) = (0.8, 1.35);

/// Sounds a prop can hold as a loop, named here because the table's own
/// [`Sound`] is private to the audio module.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EmitterSound {
    /// A harvester at work.
    HarvesterHum,
}

/// A looping sound at an entity, for as long as the component is there: the
/// game puts one on a harvester's prop while it runs and takes it off when it
/// stops or fills. Only the ones within earshot of the local player play.
#[derive(Component, Clone, Copy, Debug, PartialEq, Eq)]
pub struct LoopEmitter(pub EmitterSound);

impl EmitterSound {
    const fn sound(self) -> Sound {
        match self {
            Self::HarvesterHum => Sound::HarvesterHum,
        }
    }
}

/// The audio entity of a loop and whose it is.
#[derive(Component)]
struct LoopOf {
    owner: Entity,
    sound: Sound,
}

pub(super) fn install(app: &mut App) {
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
        .add_systems(
            Update,
            (
                survey_cues,
                ride_cues,
                harvest_cues,
                bench_cues,
                blast_cues,
                loops,
                hum_pitch,
            )
                .after(super::cues),
        );
}

/// Where the local member stands, for a sound that is theirs.
fn member(local: &Query<&WorldPosition, With<LocalPlayer>>) -> Option<WorldPos> {
    local.single().ok().map(|at| at.0)
}

/// `Update`: a ping leaving the tool, what it found, and a sample's cycle.
fn survey_cues(
    mut commands: Commands,
    mut sounds: ResMut<Sounds>,
    started: Query<&WorldPosition, Added<Pinging>>,
    mut pings: MessageReader<PingAnswer>,
    mut samples: MessageReader<SampleAnswer>,
    local: Query<&WorldPosition, With<LocalPlayer>>,
) {
    for at in &started {
        play(
            &mut commands,
            &mut sounds,
            Sound::SurveyPing,
            Some(at.0),
            None,
        );
    }
    for answer in pings.read() {
        if let Ok(PingDone::Met(result)) = &answer.outcome {
            let sound = if result.found.is_empty() {
                Sound::SurveyEmpty
            } else {
                Sound::SurveyFound
            };
            play(&mut commands, &mut sounds, sound, member(&local), None);
        }
    }
    for answer in samples.read() {
        if matches!(answer.outcome, Ok(SampleDone::Cycle { .. })) {
            play(
                &mut commands,
                &mut sounds,
                Sound::SampleDone,
                member(&local),
                None,
            );
        }
    }
}

/// `Update`: the call beginning and the rider's mount, dismount and fall.
fn ride_cues(
    mut commands: Commands,
    mut sounds: ResMut<Sounds>,
    called: Query<&WorldPosition, Added<Calling>>,
    mut notices: MessageReader<RideNotice>,
    local: Query<&WorldPosition, With<LocalPlayer>>,
) {
    for at in &called {
        play(
            &mut commands,
            &mut sounds,
            Sound::BikeCall,
            Some(at.0),
            None,
        );
    }
    for notice in notices.read() {
        let sound = match notice {
            RideNotice::Mounted => Sound::BikeMount,
            RideNotice::Dismounted => Sound::BikeDismount,
            RideNotice::Thrown { .. } => Sound::BikeThrown,
            RideNotice::Cancelled { .. } => continue,
        };
        play(&mut commands, &mut sounds, sound, member(&local), None);
    }
}

/// `Update`: a machine planted and a hopper emptied into the pack.
fn harvest_cues(
    mut commands: Commands,
    mut sounds: ResMut<Sounds>,
    mut answers: MessageReader<HarvestAnswer>,
    local: Query<&WorldPosition, With<LocalPlayer>>,
) {
    for answer in answers.read() {
        let sound = match answer.outcome {
            Ok(HarvestDone::Planted { .. }) => Sound::HarvesterPlant,
            Ok(HarvestDone::Collected { .. }) => Sound::HarvesterCollect,
            _ => continue,
        };
        play(&mut commands, &mut sounds, sound, member(&local), None);
    }
}

/// How a roll's outcome sounds: the top of the table is great, the steady
/// middle good, and a barely-there lift or a setback poor.
const fn outcome_sound(outcome: Outcome) -> Sound {
    match outcome {
        Outcome::Exceptional => Sound::CraftGreat,
        Outcome::Good | Outcome::Fair => Sound::CraftGood,
        Outcome::Poor | Outcome::Setback => Sound::CraftPoor,
    }
}

/// `Update`: the bench's answers and the other screens' done deeds: a roll
/// and what it came to, a session finished, a part fitted or removed, a kit
/// used, and a sale or a purchase. Only an accepted request sounds.
fn bench_cues(
    mut commands: Commands,
    mut sounds: ResMut<Sounds>,
    mut crafts: MessageReader<CraftAnswer>,
    mut fits: MessageReader<FitAnswer>,
    mut repairs: MessageReader<RepairAnswer>,
    mut sales: MessageReader<SaleAnswer>,
    mut purchases: MessageReader<PurchaseAnswer>,
) {
    let mut say = |sound| {
        play(&mut commands, &mut sounds, sound, None, None);
    };
    for answer in crafts.read() {
        match &answer.outcome {
            Ok(CraftDone::Rolled(rolled)) => {
                say(Sound::CraftRoll);
                say(outcome_sound(rolled.outcome));
            }
            Ok(CraftDone::Finished { .. }) => say(Sound::CraftFinish),
            Ok(CraftDone::Bought { .. }) => say(Sound::Sale),
            _ => {}
        }
    }
    for _ in fits.read().filter(|answer| answer.outcome.is_ok()) {
        say(Sound::FitPart);
    }
    for _ in repairs.read().filter(|answer| answer.result.is_ok()) {
        say(Sound::Repair);
    }
    for _ in sales.read().filter(|answer| answer.result.is_ok()) {
        say(Sound::Sale);
    }
    for _ in purchases.read().filter(|answer| answer.result.is_ok()) {
        say(Sound::Sale);
    }
}

/// `Update`: a blast going off. The sim raises no message for it, so it is
/// told from a bolt that carried one and is gone when its impact arrives;
/// the splash's other victims, each with an impact of their own, find no
/// bolt left and add nothing.
fn blast_cues(
    mut commands: Commands,
    mut sounds: ResMut<Sounds>,
    mut impacts: MessageReader<CombatImpact>,
    bolts: Query<(Entity, &BlasterBolt)>,
    mut went: MessageWriter<crate::combat_fx::BlastWent>,
    mut carried: Local<Vec<(Entity, Entity, f32)>>,
) {
    for impact in impacts.read().filter(|impact| impact.shot.is_some()) {
        let gone = carried
            .iter()
            .position(|(bolt, owner, _)| *owner == impact.attacker && !bolts.contains(*bolt));
        if let Some(at) = gone {
            let (_, _, radius) = carried.swap_remove(at);
            went.write(crate::combat_fx::BlastWent {
                point: impact.point,
                radius,
            });
            play(
                &mut commands,
                &mut sounds,
                Sound::Blast,
                Some(impact.point),
                None,
            );
        }
    }
    carried.clear();
    carried.extend(bolts.iter().filter_map(|(entity, bolt)| {
        Some((entity, bolt.owner, bolt.effects.blast.as_ref()?.radius))
    }));
}

/// The loops that should be playing, as owner, sound and where.
fn wanted(
    sampling: &Query<(Entity, &WorldPosition), With<Sampling>>,
    riding: &Query<(Entity, &WorldPosition), With<Riding>>,
    emitters: &Query<(Entity, &LoopEmitter, &WorldPosition)>,
    listener: Option<WorldPos>,
) -> Vec<(Entity, Sound, WorldPos)> {
    let mut out: Vec<_> = sampling
        .iter()
        .map(|(entity, at)| (entity, Sound::SampleLoop, at.0))
        .collect();
    out.extend(
        riding
            .iter()
            .map(|(entity, at)| (entity, Sound::BikeHum, at.0)),
    );
    if let Some(ear) = listener {
        out.extend(
            emitters
                .iter()
                .filter(|(_, _, at)| at.0.distance(ear) <= EARSHOT_M)
                .map(|(entity, emitter, at)| (entity, emitter.0.sound(), at.0)),
        );
    }
    out
}

/// `Update`: start a loop for each owner that has none and end the ones whose
/// owner has let go (or gone out of earshot).
fn loops(
    mut commands: Commands,
    mut sounds: ResMut<Sounds>,
    sampling: Query<(Entity, &WorldPosition), With<Sampling>>,
    riding: Query<(Entity, &WorldPosition), With<Riding>>,
    emitters: Query<(Entity, &LoopEmitter, &WorldPosition)>,
    local: Query<&WorldPosition, With<LocalPlayer>>,
    playing: Query<(Entity, &LoopOf)>,
) {
    let wanted = wanted(&sampling, &riding, &emitters, member(&local));
    for (entity, held) in &playing {
        if !wanted
            .iter()
            .any(|(owner, sound, _)| *owner == held.owner && *sound == held.sound)
        {
            commands.entity(entity).despawn();
        }
    }
    for (owner, sound, at) in wanted {
        if playing
            .iter()
            .any(|(_, held)| held.owner == owner && held.sound == sound)
        {
            continue;
        }
        let entity = play(&mut commands, &mut sounds, sound, Some(at), Some(owner));
        commands.entity(entity).insert(LoopOf { owner, sound });
    }
}

/// The hum's playback speed for a bike at `speed` of a `top`: the pitch
/// climbs from [`HUM_SPEED`]'s floor standing to its ceiling at top speed,
/// reverse counting as speed.
fn hum_speed(speed: f32, top: f32) -> f32 {
    let share = if top > 0. {
        (speed.abs() / top).clamp(0., 1.)
    } else {
        0.
    };
    HUM_SPEED.0 + (HUM_SPEED.1 - HUM_SPEED.0) * share
}

/// `Update`: the rider's hum follows the bike's speed.
fn hum_pitch(
    humming: Query<(
        &LoopOf,
        Option<&mut AudioSink>,
        Option<&mut SpatialAudioSink>,
    )>,
    riders: Query<&Riding>,
) {
    for (held, sink, spatial) in humming {
        let Some(bike) = (held.sound == Sound::BikeHum)
            .then(|| riders.get(held.owner).ok())
            .flatten()
        else {
            continue;
        };
        let top = bike.dials.speed.road.max(bike.dials.speed.ground);
        let speed = hum_speed(bike.speed, top);
        if let Some(sink) = sink {
            sink.set_speed(speed);
        }
        if let Some(sink) = spatial {
            sink.set_speed(speed);
        }
    }
}

#[cfg(test)]
mod tests;
