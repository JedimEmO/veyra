//! State the engine client and the game's combat view both read: the combat
//! camera's marker and boom, the shake switch, the ride's easing, the baked
//! bike index and the body tint. The game's view crate draws and writes
//! them; the engine's orbit camera, the settings and the screens (the
//! stores' bike icon, the sheet's tint) read them, so they live below both, in
//! the sim crate that already links the registries they read (`veyra-ui`'s
//! screens need them without linking the engine client). They use `bevy_ecs`
//! and `bevy_math` only, so the server carries them at no cost.

use bevy::ecs::component::Component;
use bevy::ecs::message::Message;
use bevy::ecs::resource::Resource;
use bevy::math::Vec3;
use strata_registry::vehicle::{MODELS_PATH, VehicleModels};

use veyra_rules::combat::Weapon;
use veyra_rules::progression::CharacterSheet;

/// Opt-in camera state, so connected play retains the existing orbit camera.
#[derive(Component, Debug)]
pub struct CombatCamera {
    /// The boom now, metres.
    pub distance: f32,
    /// The shoulder offset now, metres.
    pub shoulder: f32,
    /// The vertical field of view before the scope narrows it, radians.
    pub base_fov: f32,
    /// A boom length that replaces the walking and aiming ones, for a still
    /// taken close to the character.
    pub hold: Option<f32>,
    /// Radians the camera is turned round the character from behind it,
    /// the player still facing where the view's own heading says, for a
    /// still taken from the side. Nought in play.
    pub yaw_offset: f32,
}

impl Default for CombatCamera {
    fn default() -> Self {
        Self {
            distance: 3.8,
            shoulder: 0.65,
            base_fov: 45_f32.to_radians(),
            hold: None,
            yaw_offset: 0.,
        }
    }
}

/// Whether the camera may shake: the settings' Camera shake switch, on
/// until the member turns it off.
#[derive(Resource, Debug, Clone, Copy, PartialEq, Eq)]
pub struct CameraShake(pub bool);

impl Default for CameraShake {
    fn default() -> Self {
        Self(true)
    }
}

/// The camera's pull onto the bike, shared by both cameras.
#[derive(Resource, Debug, Clone, Copy, PartialEq)]
pub struct RideView {
    /// 0 on foot, 1 fully on the bike; moves by `dt / look.ease_s`.
    pub blend: f32,
    /// The boom while mounted, metres.
    pub distance: f32,
    /// The eye over the ground while mounted, metres.
    pub eye: f32,
    /// How high the bike hovers, metres.
    pub hover_m: f32,
    /// Where the rider sits in the bike's frame.
    pub seat: Vec3,
    /// The vehicle's look dials, copied from its file while mounted.
    pub look: Look,
}

/// The vehicle file's `view` dials that shape the look, with the values the
/// courier bike ships, so a view not yet filled eases and bobs sensibly.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Look {
    /// The lean at the slow-turn rate, radians.
    pub max_lean: f32,
    /// The pitch limit, radians.
    pub max_pitch: f32,
    /// The bob, metres.
    pub bob_m: f32,
    /// The bob's period, seconds.
    pub bob_period_s: f32,
    /// The camera's pull, seconds.
    pub ease_s: f32,
    /// The lean and pitch follow rate, per second.
    pub follow: f32,
}

impl Look {
    /// The dials of a vehicle's `view`.
    #[must_use]
    pub fn of(view: &strata_registry::vehicle::View) -> Self {
        Self {
            max_lean: view.lean_deg.to_radians(),
            max_pitch: view.pitch_max,
            bob_m: view.bob_m,
            bob_period_s: view.bob_period_s,
            ease_s: view.ease_s,
            follow: view.follow,
        }
    }
}

/// A look with nothing in it: the vehicle file's dials are put in at the
/// mount, and nothing reads a look before then.
impl Default for Look {
    fn default() -> Self {
        Self {
            max_lean: 0.0,
            max_pitch: 0.0,
            bob_m: 0.0,
            bob_period_s: 1.0,
            ease_s: 1.0,
            follow: 1.0,
        }
    }
}

impl Default for RideView {
    fn default() -> Self {
        Self {
            blend: 0.0,
            distance: 0.0,
            eye: 0.0,
            hover_m: 0.0,
            seat: Vec3::ZERO,
            look: Look::default(),
        }
    }
}

impl RideView {
    /// The eased weight, smooth at both ends.
    #[must_use]
    pub fn weight(&self) -> f32 {
        self.blend * self.blend * (3.0 - 2.0 * self.blend)
    }

    /// `foot` towards `ride` by the eased weight.
    #[must_use]
    pub fn mix(&self, foot: f32, ride: f32) -> f32 {
        foot + (ride - foot) * self.weight()
    }

    /// Moves the blend towards mounted or on foot over `dt` seconds.
    pub fn ease(&mut self, riding: bool, dt: f32) {
        let step = dt / self.look.ease_s.max(1e-3);
        self.blend = if riding {
            (self.blend + step).min(1.0)
        } else {
            (self.blend - step).max(0.0)
        };
    }
}

/// The baked bike's index. Empty on a clone that has not run `just content`:
/// a plain box stands in.
#[derive(Resource, Debug, Clone, Default, PartialEq)]
pub struct BikeModels(pub VehicleModels);

impl BikeModels {
    /// The index under `generated`, the content step's output root, as a
    /// source rooted there. A missing
    /// one is no error: it is said once at info and the box is drawn.
    #[must_use]
    pub fn read(generated: &strata_registry::files::Files) -> Self {
        let path = std::path::Path::new(MODELS_PATH);
        match generated
            .read_string(MODELS_PATH)
            .map_err(|error| error.to_string())
            .and_then(|text| VehicleModels::from_ron(&text).map_err(|error| error.to_string()))
        {
            Ok(set) => {
                tracing::info!(path = %path.display(), bikes = set.files.len(), "bike models");
                Self(set)
            }
            Err(error) => {
                tracing::info!(
                    path = %path.display(),
                    %error,
                    "no bike models: a plain box stands in until `just content` runs"
                );
                Self::default()
            }
        }
    }
}

/// Which of the household's colours an actor's body wears, as an index
/// into `combat_art`'s palette of tints: a swap at a terminal changes it, and the body follows,
/// so whose hands the player has is visible without a HUD. On the actor,
/// not the visual; the game sets it with the sheet.
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct BodyTint(pub u8);

/// A colour of a body's own, in sRGB bytes, which wins over [`BodyTint`]: a
/// faction's people and a band's wear their holder's colour so a guard reads
/// apart from an outlaw and from the household's members.
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq)]
pub struct BodyColour(pub [u8; 3]);

/// The weathered steel of a derelict machine's security units, sRGB bytes.
/// The lift is normalised by its peak, so a pure grey lifts to the same dim
/// grey on the dark body; a little green keeps it a metal and not a faction.
pub const MACHINE_STEEL: [u8; 3] = [150, 185, 172];

/// Marks an opponent whose charge is drawn as a beam to its aim point. The
/// game puts it on the marksman and on no other: the sentry's glow is its
/// whole telegraph.
#[derive(Component, Debug, Clone, Copy)]
pub struct AimBeam;

/// Marks an opponent that bears a slab. The game puts it on the bulwark and
/// on no other kind.
#[derive(Component, Debug, Clone, Copy)]
pub struct Shield;

/// A moment of progression the interface marks with its own sound rather
/// than a click: written by the sheet when Learn succeeds and by the bench
/// when a switch's fade begins, and by the game where it does the same
/// without a screen (the scripted lesson and swap).
#[derive(Message, Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProgressCue {
    /// A box was learned.
    Learned,
    /// Another character is coming out.
    Switched,
}

/// A plain line from the world (the first day's word about where the props
/// stand, the cache a fall leaves, the fall itself). The household has no
/// interface, so it writes the words here and the screens toast them.
#[derive(Message, Debug, Clone, PartialEq, Eq)]
pub struct WorldNotice(pub String);

/// One character as the bench shows them. The sheet is the sim's own
/// type, so the points and the summary are derived by the screen from the
/// graph rather than carried.
#[derive(Debug, Clone, PartialEq)]
pub struct BenchCard {
    /// Their name.
    pub name: String,
    /// Their body tint, an index into the view's palette.
    pub tint: u8,
    /// The weapon they carry.
    pub weapon: Weapon,
    /// What they know.
    pub sheet: CharacterSheet,
    /// The fabricator's run they are working, while they are: they cannot
    /// be switched to until it ends or is stopped.
    pub work: Option<BenchWork>,
    /// Played minutes left of an injury from a fall, rounded up, while they
    /// are hurt: they cannot be switched to until they have healed.
    pub injured_min: Option<u32>,
}

/// A run a benched member is working, as their card says it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BenchWork {
    /// How many of the batch are made.
    pub made: u32,
    /// How many the batch is.
    pub size: u32,
}

/// The household as the bench shows it. Written by the game.
#[derive(Resource, Debug, Clone, Default, PartialEq)]
pub struct BenchView {
    /// Everyone on the bench; the first is the arrival.
    pub characters: Vec<BenchCard>,
    /// Who is out.
    pub active: usize,
    /// Who can still join, in the order offered.
    pub recruits: Vec<BenchCard>,
    /// How many the bench holds.
    pub capacity: usize,
}

/// What a button on the bench asks the game for.
#[derive(Message, Debug, Clone, Copy, PartialEq, Eq)]
pub enum BenchRequest {
    /// Bring the character at this index of the bench out.
    Switch(usize),
    /// The recruit at this index of the recruits joins the bench.
    Join(usize),
}

/// The game's answer, shown as a toast.
#[derive(Message, Debug, Clone, PartialEq, Eq)]
pub struct BenchOutcome {
    /// Whether it happened.
    pub ok: bool,
    /// What happened, or why not.
    pub message: String,
}

/// A fall's fade (ticket 0012, step 4a): the game starts it, the curtain
/// comes down over `length` seconds and holds, the game does its work in
/// the dark and wakes it, and the world comes back. Apart from the switch
/// fade, whose request is a switch the player asked for.
#[derive(Resource, Debug, Default, Clone, Copy, PartialEq)]
pub struct FallFade {
    /// Where the fade is.
    pub phase: FallPhase,
    /// Seconds in the phase.
    pub elapsed: f32,
    /// Seconds the curtain takes to come down.
    pub length: f32,
}

/// The phases of a [`FallFade`].
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub enum FallPhase {
    /// No fall.
    #[default]
    Idle,
    /// The curtain is coming down.
    Falling,
    /// It is down and the game may work.
    Black,
    /// The curtain is going up.
    Waking,
}

impl FallFade {
    /// Start the curtain down over `length` seconds.
    pub fn begin(&mut self, length: f32) {
        *self = Self {
            phase: FallPhase::Falling,
            elapsed: 0.,
            length,
        };
    }

    /// The work behind the curtain is done: let the world come back.
    pub fn wake(&mut self) {
        self.phase = FallPhase::Waking;
        self.elapsed = 0.;
    }
}
