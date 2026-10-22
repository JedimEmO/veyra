//! Veyra's combat view: everything the player sees and hears of a fight and
//! of the bike, drawn over the engine client.
//!
//! The camera and its aim, the combat HUD (vitals, controls card, hit marker,
//! reticle, vignette), the action bar, nameplates and floating text, the
//! character's art and inverse kinematics, the beams, shields and effects, the
//! field's sounds, the scope and the bike under its rider. Every one is a view
//! over the sim's `Combatant` and its events; the sim never reads any of it.
//!
//! The crate links `strata-client` (cameras, input, statistics, settings, the
//! look wiring), the screen framework (`strata-ui`) and the game's screens
//! (`veyra-ui`, whose words, icons and toasts the HUD uses), `veyra-sim` and the
//! render crates, and nothing links it but the game. What the engine client's
//! orbit camera and the game's screens read of this view (the combat camera's
//! marker, the ride's easing, the body tint) lives in `veyra_sim::view`, below
//! both, and the HUD scaffolding they share (`Painted`, `ui_point`,
//! `world_ui_shown`) in `strata_ui`. [`combat::CombatViewPlugin`] is the one plugin; the game
//! adds it where it added the client's combat half.

#[cfg(feature = "ui")]
pub mod action_bar;
pub mod combat;
pub mod combat_art;
mod combat_audio;
pub mod combat_beam;
mod combat_fx;
#[cfg(feature = "ui")]
pub mod combat_hud;
mod combat_ik;
pub mod combat_shield;
#[cfg(feature = "ui")]
mod combat_text;
#[cfg(feature = "ui")]
mod nameplates;
pub mod ride_art;
mod scope;

pub use combat_audio::{EmitterSound, LoopEmitter};
pub use combat_fx::DustPuff;
