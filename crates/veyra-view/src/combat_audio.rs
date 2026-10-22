//! Recorded Forge takes follow confirmed combat cues and the rendered run phase,
//! and, with the UI, the screens' own moments (the `interface` module).

use std::collections::HashMap;

#[cfg(feature = "ui")]
use bevy::audio::AudioSinkPlayback;
use bevy::audio::{SpatialScale, Volume};
use bevy::prelude::*;
use strata_core::{Rng, WorldPos};
use strata_physics::CharacterController;
use veyra_sim::combat::{Action, CombatImpact, Combatant, GuardOutcome, ShotFired};
use veyra_sim::{
    LocalPlayer, Registries, RenderOrigin, Settlements, WorldPosition, WorldSourceRes,
};

// One table owns file names, take counts and mix settings. Counts exclude backend rejects.
macro_rules! events {
    ($($variant:ident => ($name:literal, $count:literal, $gain:literal, $pitch:literal, $loop:literal, $bus:ident)),+ $(,)?) => {
        #[derive(Clone, Copy, Debug, PartialEq, Eq)]
        #[cfg_attr(
            not(feature = "ui"),
            allow(dead_code, reason = "the interface's sounds are played by the optional UI")
        )]
        enum Sound { $($variant),+ }
        const EVENTS: &[Event] = &[$(Event {
            name: $name, takes: $count, gain: $gain, pitch: $pitch, looping: $loop, bus: Bus::$bus,
        }),+];
    };
}

events! {
    BlasterShot => ("blaster_shot", 3, 0.8, 0.03, false, World),
    SentryShot => ("sentry_shot", 2, 0.9, 0.03, false, World),
    ImpactStone => ("impact_stone", 3, 0.75, 0.04, false, World),
    ImpactMetal => ("impact_metal", 2, 0.75, 0.04, false, World),
    ImpactShield => ("impact_shield", 3, 0.8, 0.03, false, World),
    ImpactFlesh => ("impact_flesh", 2, 0.75, 0.03, false, World),
    SwingLight => ("swing_light", 2, 0.7, 0.04, false, World),
    SwingHeavy => ("swing_heavy", 2, 0.85, 0.03, false, World),
    BladeHitArmor => ("blade_hit_armor", 3, 0.85, 0.03, false, World),
    BladeHitShield => ("blade_hit_shield", 2, 0.85, 0.03, false, World),
    DodgeRoll => ("dodge_roll", 2, 0.9, 0.03, false, World),
    Reload => ("reload", 2, 0.9, 0.02, false, World),
    FootStone => ("foot_stone", 4, 0.8, 0.04, false, World),
    FootGravel => ("foot_gravel", 4, 0.8, 0.04, false, World),
    FootMetal => ("foot_metal", 4, 0.8, 0.04, false, World),
    Jump => ("jump", 2, 0.9, 0.03, false, World),
    Land => ("land", 2, 1.0, 0.03, false, World),
    SentryCharge => ("sentry_charge", 2, 1.0, 0.0, false, World),
    RusherWindup => ("rusher_windup", 2, 0.9, 0.03, false, World),
    EnemyDeath => ("enemy_death", 2, 0.9, 0.03, false, World),
    PlayerHurt => ("player_hurt", 3, 0.8, 0.02, false, World),
    HitMarker => ("hit_marker", 2, 0.6, 0.02, false, World),
    LowHealth => ("low_health", 1, 0.6, 0.0, true, World),
    ShieldRecharge => ("shield_recharge", 2, 0.8, 0.02, false, World),
    // Levelled quieter than the fight (tools/combat_audio.py), so unity gain;
    // the chord and the switch are not detuned.
    UiHover => ("ui_hover", 2, 1.0, 0.03, false, Interface),
    UiClick => ("ui_click", 2, 1.0, 0.02, false, Interface),
    UiOpen => ("ui_open", 2, 1.0, 0.01, false, Interface),
    UiClose => ("ui_close", 2, 1.0, 0.01, false, Interface),
    UiToast => ("ui_toast", 2, 1.0, 0.01, false, Interface),
    UiRefuse => ("ui_refuse", 2, 1.0, 0.01, false, Interface),
    XpCredit => ("xp_credit", 2, 1.0, 0.02, false, Interface),
    SkillLearn => ("skill_learn", 2, 1.0, 0.0, false, Interface),
    BenchSwitch => ("bench_switch", 2, 1.0, 0.0, false, Interface),
    // The field's sounds sit under the fight's (0.7 to 0.9, the files being
    // levelled: beds at -26 to -30 LUFS, events at -20 to -24), and the
    // loops take no jitter, a pitch spread being the hum's own business.
    SurveyPing => ("survey_ping", 2, 0.8, 0.03, false, World),
    SurveyFound => ("survey_found", 2, 0.8, 0.03, false, World),
    SurveyEmpty => ("survey_empty", 1, 0.7, 0.03, false, World),
    SampleLoop => ("sample_loop", 1, 0.7, 0.0, true, World),
    SampleDone => ("sample_done", 2, 0.8, 0.03, false, World),
    BikeHum => ("bike_hum", 1, 0.7, 0.0, true, World),
    BikeCall => ("bike_call", 1, 0.8, 0.02, false, World),
    BikeMount => ("bike_mount", 2, 0.8, 0.03, false, World),
    BikeDismount => ("bike_dismount", 2, 0.8, 0.03, false, World),
    BikeThrown => ("bike_thrown", 2, 0.9, 0.03, false, World),
    HarvesterHum => ("harvester_hum", 1, 0.7, 0.0, true, World),
    HarvesterPlant => ("harvester_plant", 1, 0.8, 0.03, false, World),
    HarvesterCollect => ("harvester_collect", 2, 0.8, 0.03, false, World),
    WeaponSwap => ("weapon_swap", 2, 0.8, 0.03, false, World),
    GuardBreak => ("guard_break", 2, 0.9, 0.03, false, World),
    Blast => ("blast", 2, 0.9, 0.03, false, World),
    // The bench's and the map's, where the interface's own sit (unity: the
    // files are levelled for the interface bus).
    CraftRoll => ("craft_roll", 2, 1.0, 0.02, false, Interface),
    CraftGood => ("craft_good", 2, 1.0, 0.02, false, Interface),
    CraftGreat => ("craft_great", 1, 1.0, 0.01, false, Interface),
    CraftPoor => ("craft_poor", 2, 1.0, 0.02, false, Interface),
    CraftFinish => ("craft_finish", 2, 1.0, 0.01, false, Interface),
    FitPart => ("fit_part", 2, 1.0, 0.02, false, Interface),
    Repair => ("repair", 1, 1.0, 0.02, false, Interface),
    Sale => ("sale", 2, 1.0, 0.02, false, Interface),
    MapOpen => ("map_open", 1, 1.0, 0.01, false, Interface),
}

struct Event {
    name: &'static str,
    takes: u8,
    gain: f32,
    pitch: f32,
    // Loops need one authored take; one-shots need alternatives to avoid repeats.
    looping: bool,
    bus: Bus,
}

/// Which volume a sound answers to under the master: the interface's own
/// slider scales the screens' sounds and leaves the fight's alone.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Bus {
    World,
    Interface,
}

impl Event {
    fn path(&self, take: u8) -> String {
        format!("audio/combat/{}_{}.ogg", self.name, take + 1)
    }
}

#[derive(Resource)]
struct Sounds {
    /// An event's takes, asked of the asset server the first time it is played:
    /// 117 files fetched at start were most of a browser's audio download, and
    /// a session plays a fraction of them. Indexed like [`EVENTS`].
    handles: Vec<Option<Vec<Handle<AudioSource>>>>,
    /// `None` only in tests, which have no asset server and take default handles.
    server: Option<AssetServer>,
    last: Vec<Option<u8>>,
    rng: Rng,
    /// The interface volume, `0..=1`; the settings store's value once the
    /// UI has read it.
    interface: f32,
}

impl Sounds {
    fn level(&self, sound: Sound) -> f32 {
        let event = &EVENTS[sound as usize];
        match event.bus {
            Bus::World => event.gain,
            Bus::Interface => event.gain * self.interface,
        }
    }

    fn choose(&mut self, sound: Sound) -> (Handle<AudioSource>, f32) {
        let index = sound as usize;
        let event = &EVENTS[index];
        let take = choose_take(&mut self.rng, event.takes, self.last[index]);
        self.last[index] = Some(take);
        let speed = 1. + (self.rng.next_f32() * 2. - 1.) * event.pitch;
        let takes = self.handles[index].get_or_insert_with(|| match &self.server {
            Some(server) => (0..event.takes)
                .map(|t| server.load(event.path(t)))
                .collect(),
            None => vec![Handle::default(); usize::from(event.takes)],
        });
        (takes[usize::from(take)].clone(), speed)
    }
}

fn choose_take(rng: &mut Rng, count: u8, previous: Option<u8>) -> u8 {
    let choices = count - u8::from(previous.is_some() && count > 1);
    let mut take = u8::try_from(rng.below(u32::from(choices))).unwrap_or(0);
    if count > 1 && previous.is_some_and(|last| take >= last) {
        take += 1;
    }
    take
}

#[derive(Component)]
struct Playing {
    #[cfg_attr(
        not(any(test, feature = "ui")),
        allow(dead_code, reason = "live mix changes need the optional settings UI")
    )]
    sound: Sound,
    // World coordinates let the source follow an origin rebase without a jump.
    point: Option<WorldPos>,
    follow: Option<Entity>,
}

#[derive(Component)]
struct LowHealth;

#[derive(Component)]
struct ChargeWarning(Entity);

#[derive(SystemSet, Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) struct FootstepAudio;

/// Emitted by the same clock that seeks the run clip, including reverse playback.
#[derive(Message)]
pub(crate) struct FootContact(pub(crate) Entity);

// Audited contact events in Forge assets/clips/run.json, on its 0.8 s clock.
pub(crate) fn foot_contacts(before: f32, after: f32, duration: f32) -> usize {
    [0.15, 0.55]
        .into_iter()
        .map(|contact| {
            let crossed = (((before - contact) / duration).floor()
                - ((after - contact) / duration).floor())
            .abs();
            // Preserve multiple crossings at low frame rates, but bound a stall's burst.
            #[allow(
                clippy::cast_possible_truncation,
                clippy::cast_sign_loss,
                reason = "absolute integral count, capped at four crossings per foot"
            )]
            {
                crossed.min(4.) as usize
            }
        })
        .sum()
}

pub(super) fn install(app: &mut App) {
    app.add_message::<FootContact>()
        .add_systems(Startup, load)
        .add_systems(Update, (listener, launches, cues, low_health).chain())
        .add_systems(PostUpdate, footsteps.in_set(FootstepAudio))
        .add_systems(
            PostUpdate,
            position_sources
                .after(strata_client::ClientSet::Follow)
                .before(bevy::transform::TransformSystems::Propagate),
        );
    field::install(app);
    #[cfg(feature = "ui")]
    {
        app.add_systems(Update, volume.before(launches));
        interface::install(app);
    }
}

#[cfg(feature = "ui")]
fn volume(
    store: Res<slotted::ui::ValueStore>,
    mut global: ResMut<bevy::audio::GlobalVolume>,
    mut sounds: ResMut<Sounds>,
    mut playing: Query<(
        &Playing,
        Option<&mut AudioSink>,
        Option<&mut SpatialAudioSink>,
    )>,
) {
    use strata_client::ui::settings::keys;
    #[allow(clippy::cast_possible_truncation, reason = "bounded volume slider")]
    let share = |key| {
        store
            .get(key)
            .and_then(slotted::ui::Value::as_f64)
            .map(|value| (value.clamp(0., 100.) / 100.) as f32)
    };
    let (master, interface) = (share(keys::MASTER_VOLUME), share(keys::UI_VOLUME));
    if master.is_none() && interface.is_none() {
        return;
    }
    if let Some(interface) = interface {
        sounds.interface = interface;
    }
    let volume = master.map_or(global.volume, Volume::Linear);
    global.volume = volume;
    // GlobalVolume only affects new sinks in Bevy 0.19. Update live loops too.
    for (sound, sink, spatial) in &mut playing {
        let level = Volume::Linear(sounds.level(sound.sound)) * volume;
        if let Some(mut sink) = sink {
            sink.set_volume(level);
        }
        if let Some(mut sink) = spatial {
            sink.set_volume(level);
        }
    }
}

fn load(mut commands: Commands, server: Res<AssetServer>) {
    let seed = strata_core::platform::time::SystemTime::now()
        .duration_since(strata_core::platform::time::UNIX_EPOCH)
        .map_or(42, |time| time.as_secs() ^ u64::from(time.subsec_nanos()));
    commands.insert_resource(Sounds {
        handles: vec![None; EVENTS.len()],
        server: Some(server.clone()),
        last: vec![None; EVENTS.len()],
        rng: Rng::new(seed),
        interface: 1.,
    });
}

fn listener(
    mut commands: Commands,
    cameras: Query<Entity, (With<super::combat::CombatCamera>, Without<SpatialListener>)>,
) {
    for camera in &cameras {
        commands.entity(camera).insert(SpatialListener::new(0.3));
    }
}

fn play(
    commands: &mut Commands,
    sounds: &mut Sounds,
    sound: Sound,
    point: Option<WorldPos>,
    follow: Option<Entity>,
) -> Entity {
    let (handle, speed) = sounds.choose(sound);
    let event = &EVENTS[sound as usize];
    let mut settings = if event.looping {
        PlaybackSettings::LOOP
    } else {
        PlaybackSettings::DESPAWN
    }
    .with_volume(Volume::Linear(sounds.level(sound)))
    .with_speed(speed)
    .with_spatial(point.is_some())
    .with_spatial_scale(SpatialScale::new(0.08));
    if sound == Sound::SentryCharge {
        settings = settings.with_duration(std::time::Duration::from_millis(650));
    }
    commands
        .spawn((
            AudioPlayer(handle),
            settings,
            Transform::default(),
            Playing {
                sound,
                point,
                follow,
            },
        ))
        .id()
}

fn position_sources(
    origin: Res<RenderOrigin>,
    actors: Query<&WorldPosition>,
    mut sources: Query<(&mut Playing, &mut Transform)>,
) {
    for (mut sound, mut transform) in &mut sources {
        if let Some(actor) = sound.follow.and_then(|entity| actors.get(entity).ok()) {
            sound.point = Some(actor.0);
        }
        if let Some(point) = sound.point {
            transform.translation = origin.to_render(point);
        }
    }
}

fn launches(
    mut commands: Commands,
    mut sounds: ResMut<Sounds>,
    mut events: MessageReader<ShotFired>,
    players: Query<Entity, With<LocalPlayer>>,
) {
    let local = players.single().ok();
    for shot in events.read() {
        let own = Some(shot.attacker) == local;
        play(
            &mut commands,
            &mut sounds,
            if own {
                Sound::BlasterShot
            } else {
                Sound::SentryShot
            },
            (!own).then_some(shot.from),
            None,
        );
    }
}

#[derive(Clone, Copy)]
struct Snapshot {
    sequence: u64,
    action: Action,
    shield: f32,
    hurts: u64,
    charging: bool,
    recovering: bool,
    grounded: bool,
}

impl Snapshot {
    fn new(actor: &Combatant, motor: Option<&CharacterController>) -> Self {
        Self {
            sequence: actor.sequence,
            action: actor.action,
            shield: actor.shield,
            hurts: actor.hurts,
            charging: actor.charge > 0.,
            recovering: false,
            grounded: motor.is_some_and(|m| m.grounded),
        }
    }
}

#[allow(
    clippy::too_many_arguments,
    reason = "presentation joins messages with prior actor state"
)]
fn cues(
    mut commands: Commands,
    mut sounds: ResMut<Sounds>,
    mut impacts: MessageReader<CombatImpact>,
    actors: Query<(
        Entity,
        &Combatant,
        &WorldPosition,
        Option<&CharacterController>,
    )>,
    local: Query<Entity, With<LocalPlayer>>,
    mut previous: Local<HashMap<Entity, Snapshot>>,
    charges: Query<(Entity, &ChargeWarning)>,
) {
    let local = local.single().ok();
    for impact in impacts.read() {
        let shield = impact
            .victim
            .and_then(|victim| {
                actors.get(victim).ok().map(|(_, actor, _, _)| {
                    previous
                        .get(&victim)
                        .map_or(actor.shield > 0., |old| old.shield > actor.shield)
                })
            })
            .unwrap_or(false);
        let sound = guard_sound(
            impact.guard,
            impact_sound(impact.melee, impact.victim.is_some(), shield, None),
        );
        play(&mut commands, &mut sounds, sound, Some(impact.point), None);
        if let Some(layer) = guard_layer(impact.guard) {
            play(&mut commands, &mut sounds, layer, Some(impact.point), None);
        }
        if Some(impact.attacker) == local && impact.victim.is_some() {
            play(&mut commands, &mut sounds, Sound::HitMarker, None, None);
        }
    }
    for (entity, actor, position, motor) in &actors {
        let own = Some(entity) == local;
        let old = previous
            .get(&entity)
            .copied()
            .unwrap_or_else(|| Snapshot::new(actor, motor));
        let mut next = Snapshot::new(actor, motor);
        let point = (!own).then_some(position.0);
        if old.action != actor.action || old.sequence != actor.sequence {
            let cue = action_sound(actor.action, own);
            if let Some(sound) = cue {
                play(
                    &mut commands,
                    &mut sounds,
                    sound,
                    point,
                    (!own).then_some(entity),
                );
            }
        }
        if !own && next.charging && !old.charging {
            let warning = play(
                &mut commands,
                &mut sounds,
                Sound::SentryCharge,
                point,
                Some(entity),
            );
            commands.entity(warning).insert(ChargeWarning(entity));
        }
        if own && actor.hurts > old.hurts {
            play(&mut commands, &mut sounds, Sound::PlayerHurt, None, None);
        }
        if own && actor.action != Action::Dead {
            // A reset also raises shields; the damage counter distinguishes it.
            next.recovering = actor.shield > old.shield && actor.hurts == old.hurts;
            if next.recovering && !old.recovering {
                play(
                    &mut commands,
                    &mut sounds,
                    Sound::ShieldRecharge,
                    None,
                    None,
                );
            }
            // Preserve recovery between fixed ticks; stop at capacity or on damage/reset.
            next.recovering |= old.recovering
                && actor.shield >= old.shield
                && actor.shield < actor.shield_capacity
                && actor.hurts == old.hurts;
            if let Some(motor) = motor {
                if old.grounded && !motor.grounded && motor.vertical_velocity > 0. {
                    play(&mut commands, &mut sounds, Sound::Jump, None, None);
                } else if !old.grounded && motor.grounded {
                    play(&mut commands, &mut sounds, Sound::Land, None, None);
                }
            }
        }
        previous.insert(entity, next);
    }
    previous.retain(|entity, _| actors.contains(*entity));
    for (entity, warning) in &charges {
        if !actors.get(warning.0).is_ok_and(|(_, actor, _, _)| {
            actor.charge > 0. && !matches!(actor.action, Action::Dead | Action::Stagger)
        }) {
            commands.entity(entity).despawn();
        }
    }
}

fn action_sound(action: Action, own: bool) -> Option<Sound> {
    match (action, own) {
        (Action::Light | Action::Followup, true) => Some(Sound::SwingLight),
        (Action::Heavy, true) => Some(Sound::SwingHeavy),
        (Action::Light | Action::Followup | Action::Heavy, false) => Some(Sound::RusherWindup),
        (Action::Dodge, true) => Some(Sound::DodgeRoll),
        (Action::Swap, _) => Some(Sound::WeaponSwap),
        (Action::Reload, true) => Some(Sound::Reload),
        (Action::Dead, false) => Some(Sound::EnemyDeath),
        _ => None,
    }
}

/// A guarded hit rings differently from one that landed, from the sounds
/// the game has: a block is the metal impact, a parry the same with the
/// marker's chime laid over it (`guard_layer`), and a broken guard is the
/// hit that got through.
fn guard_sound(guard: GuardOutcome, landed: Sound) -> Sound {
    match guard {
        GuardOutcome::Open => landed,
        GuardOutcome::Blocked | GuardOutcome::Parried => Sound::ImpactMetal,
        GuardOutcome::Broken => Sound::BladeHitArmor,
    }
}

/// The chime a parry adds to the metal impact, so a parry and a block differ
/// and neither sounds like a cut that landed on a victim, and the crack a
/// broken guard adds to the hit that got through.
fn guard_layer(guard: GuardOutcome) -> Option<Sound> {
    match guard {
        GuardOutcome::Parried => Some(Sound::HitMarker),
        GuardOutcome::Broken => Some(Sound::GuardBreak),
        GuardOutcome::Open | GuardOutcome::Blocked => None,
    }
}

fn impact_sound(melee: bool, victim: bool, shield: bool, material: Option<&str>) -> Sound {
    match (victim, melee, shield, material) {
        (false, _, _, Some("metal" | "steel")) => Sound::ImpactMetal,
        (false, _, _, _) => Sound::ImpactStone,
        (true, true, true, _) => Sound::BladeHitShield,
        (true, true, false, _) => Sound::BladeHitArmor,
        (true, false, true, _) => Sound::ImpactShield,
        (true, false, false, _) => Sound::ImpactFlesh,
    }
}

fn low_health(
    mut commands: Commands,
    mut sounds: ResMut<Sounds>,
    players: Query<&Combatant, With<LocalPlayer>>,
    loops: Query<Entity, With<LowHealth>>,
) {
    let needed = players
        .single()
        .is_ok_and(|actor| actor.health > 0. && actor.health < 25. && actor.action != Action::Dead);
    if needed && loops.is_empty() {
        let entity = play(&mut commands, &mut sounds, Sound::LowHealth, None, None);
        commands.entity(entity).insert(LowHealth);
    } else if !needed {
        for entity in &loops {
            commands.entity(entity).despawn();
        }
    }
}

fn foot_surface(material: Option<&str>, city: bool) -> Sound {
    match material {
        Some("metal" | "steel") => Sound::FootMetal,
        Some("rock" | "granite" | "sandstone" | "basalt" | "slate" | "setts" | "flagstones") => {
            Sound::FootStone
        }
        Some("gravel" | "grass" | "forest_floor" | "sand" | "snow" | "soil" | "clay") => {
            Sound::FootGravel
        }
        _ if city => Sound::FootStone,
        _ => Sound::FootGravel,
    }
}

#[allow(
    clippy::too_many_arguments,
    reason = "terrain material and city fallback are optional world data"
)]
fn footsteps(
    mut commands: Commands,
    mut sounds: ResMut<Sounds>,
    mut contacts: MessageReader<FootContact>,
    players: Query<&WorldPosition, With<LocalPlayer>>,
    source: Option<Res<WorldSourceRes>>,
    registries: Option<Res<Registries>>,
    settlements: Option<Res<Settlements>>,
) {
    for contact in contacts.read() {
        let Ok(position) = players.get(contact.0) else {
            continue;
        };
        let point = position.0;
        let material = source
            .as_ref()
            .zip(registries.as_ref())
            .and_then(|(world, registries)| {
                let layer = world.source().sample(point.x(), point.z()).material;
                registries
                    .0
                    .materials
                    .iter()
                    .nth(usize::from(layer))
                    .map(|(_, _, def)| def.texture_set.as_str())
            });
        let city = settlements.as_ref().is_some_and(|settlements| {
            settlements
                .0
                .iter()
                .any(|s| s.site.owns(s.site.to_local_xz([point.x(), point.z()])))
        });
        play(
            &mut commands,
            &mut sounds,
            foot_surface(material, city),
            None,
            None,
        );
    }
}

mod field;
pub use field::{EmitterSound, LoopEmitter};

#[cfg(feature = "ui")]
mod interface;

#[cfg(test)]
mod tests;
