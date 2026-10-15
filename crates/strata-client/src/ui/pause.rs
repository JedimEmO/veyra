//! The pause screen's consequences: the quit flow, the cursor and the
//! player's intent while a screen is open.
//!
//! Escape itself is slotted's: with nothing open `Menu` pushes the pause
//! template, and with the pause on top `Back` pops it, so this module
//! never reads a key. What it owns is what the crate leaves to the game:
//! the `quit` choice (a danger confirm, then `AppExit`), and the two
//! things a game must do while any page or modal is up: give the cursor
//! back so the player can click a button, and stop steering the character
//! with keys the menu is now reading. Neither touches the simulation; the
//! world keeps ticking under the menu because other players are in it.

use bevy::app::AppExit;
use bevy::ecs::prelude::*;
use bevy::input::keyboard::KeyCode;
use bevy::math::Vec2;
use bevy::window::{CursorGrabMode, CursorOptions, PrimaryWindow};
use slotted::menu::{ConfirmResult, ConfirmSpec, MenuChoice, MenuConfig, confirm};
use slotted::ui::{ScreenStack, Screens, push_screen};
use strata_sim::{LocalPlayer, PlayerIntent, ScriptedIntent};

/// The confirm id the quit flow answers to.
pub const QUIT_CONFIRM: &str = "quit";

/// The key that opens and closes the pause screen. Slotted binds it to
/// both `Menu` and `Back`; named here for the docs and the tests.
pub const PAUSE_KEY: KeyCode = KeyCode::Escape;

/// Run condition: no page or modal is open, so the keyboard and the mouse
/// are the game's. An overlay (a HUD layer, later a dialogue) does not
/// count. Without a screen stack at all (no UI wired) the game always has
/// its input.
pub fn game_input_allowed(stack: Option<Res<ScreenStack>>) -> bool {
    stack.is_none_or(|stack| stack.top().is_none())
}

/// Run condition: the opposite of [`game_input_allowed`].
pub fn screen_open(stack: Option<Res<ScreenStack>>) -> bool {
    !game_input_allowed(stack)
}

/// Pushes the pause screen (`MenuConfig::pause_kind`) unless it is already
/// on top. What Escape does with nothing open; a flag or a script can do
/// the same for a screenshot.
pub fn open_pause(commands: &mut Commands) {
    commands.queue(|world: &mut World| {
        let Some(kind) = world
            .get_resource::<MenuConfig>()
            .map(|config| config.pause_kind.clone())
        else {
            tracing::warn!("no MenuConfig: the UI is not wired, nothing to pause");
            return;
        };
        if world
            .resource::<ScreenStack>()
            .top()
            .is_some_and(|top| top.kind == kind)
        {
            return;
        }
        let Some(def) = world.resource::<Screens>().get(&kind).cloned() else {
            tracing::warn!(kind = %kind.0, "the pause screen is not registered yet");
            return;
        };
        push_screen(&mut world.commands(), def, None);
    });
}

/// `Update`: the pause screen's Quit asks first. Leaving is the one choice
/// a reflex press should not make.
pub fn quit_on_choice(mut choices: MessageReader<MenuChoice>, mut commands: Commands) {
    for choice in choices.read() {
        if choice.id == "quit" {
            confirm(
                &mut commands,
                ConfirmSpec::new(QUIT_CONFIRM, "strata.quit.message")
                    .title("strata.quit.title")
                    .buttons("strata.quit.accept", "slotted.menu.cancel")
                    .danger(),
            );
        }
    }
}

/// `Update`: an accepted quit confirm leaves the app.
pub fn exit_on_quit_confirmed(
    mut results: MessageReader<ConfirmResult>,
    mut exit: MessageWriter<AppExit>,
) {
    if results
        .read()
        .any(|result| result.id == QUIT_CONFIRM && result.accepted)
    {
        exit.write(AppExit::Success);
    }
}

/// While a screen is open: the cursor is free and shown, whatever the
/// camera's grab left it as. The flycam's grab only changes on a button
/// event, and a right button released over a menu never reaches it.
pub fn release_cursor(mut cursors: Query<&mut CursorOptions, With<PrimaryWindow>>) {
    for mut cursor in &mut cursors {
        if cursor.grab_mode != CursorGrabMode::None || !cursor.visible {
            cursor.grab_mode = CursorGrabMode::None;
            cursor.visible = true;
        }
    }
}

/// While a screen is open: the local player's intent is cleared, so the
/// keys the menu is reading do not also walk the character, and a W held
/// when the menu opened does not walk it into the sea. The systems that
/// write the intent from the keyboard do not run meanwhile; this is what
/// undoes the last thing they wrote. A scripted player keeps its script.
pub fn hold_player_still(
    mut players: Query<&mut PlayerIntent, (With<LocalPlayer>, Without<ScriptedIntent>)>,
) {
    for mut intent in &mut players {
        if intent.move_dir != Vec2::ZERO || intent.sprint || intent.jump {
            intent.move_dir = Vec2::ZERO;
            intent.sprint = false;
            intent.jump = false;
        }
    }
}
