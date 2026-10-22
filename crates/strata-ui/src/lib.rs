//! The engine's screen framework over slotted, and nothing of the game's.
//!
//! Linked by `strata-client` (the settings and graphics screens, the HUD) and
//! by `veyra-ui` (the game's screens), which are siblings: neither sees the
//! other. What is here is what both stand on:
//!
//! - [`sim_screen`]: the `SimScreen` kit, a screen that declares what its view
//!   reads and rebuilds only when that moved;
//! - [`ScreenWork`], [`screen_up`] and [`screen_on_top`]: where a system that
//!   changes the stack runs and the run conditions for a screen's own
//!   systems;
//! - [`pause`]: the quit flow, the cursor and the player's intent while a
//!   screen is up, and [`game_input_allowed`], the gate every steering
//!   system takes;
//! - [`scale`], [`fonts`], [`painted`] and [`world_ui`]: the UI scale, the
//!   theme's two typefaces as handles, painted nodes and what every
//!   world-anchored piece of UI shares;
//! - [`register`]: slotted's plugins, the two navigation plugins and the
//!   pause flow, once, whichever crate asks first.
//!
//! Nothing here runs without Bevy's UI plugins: [`register`] reports whether
//! they were there. It owns no rule of the game and links no render crate of
//! the engine (terrain, atmosphere, foliage); it sits over `strata-app`
//! alone, which is why a screen crate can build beside the terrain and not
//! after it.

pub mod fonts;
pub mod painted;
pub mod pause;
pub mod scale;
pub mod sim_screen;
pub mod world_ui;

use bevy::app::{App, Update};
use bevy::ecs::prelude::*;
use bevy::ecs::schedule::IntoScheduleConfigs;
use bevy::input_focus::directional_navigation::DirectionalNavigationPlugin;
use bevy::input_focus::tab_navigation::TabNavigationPlugin;
use slotted::prelude::SlottedPlugins;
use slotted::ui::SlottedUiSet;

pub use pause::{QUIT_CONFIRM, game_input_allowed, open_pause, screen_open};

/// The main menu's title key (the pause screen does not show it, but
/// `MenuConfig` wants one).
pub const TITLE: &str = "strata.title";

/// The systems that push, replace or pop a screen, and so despawn
/// one. They run after slotted's own sets: its accessibility sync
/// (`SlottedUiSet::Semantics`) queues an insert for every node whose label
/// changed this frame, and a screen torn down between that system and the
/// point its commands are applied leaves the insert a despawned entity,
/// which is a panic. Ordered after it, its commands are applied first.
/// Every system that changes the stack belongs here.
#[derive(SystemSet, Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ScreenWork;

/// Run condition for a screen's systems that only matter while it is up:
/// true while a screen of `kind` is on top and for the one frame after it
/// leaves. Those systems keep a "was on top last frame" flag to give a
/// screen pushed this frame its grace frame; skipping them while it is shut
/// would leave the flag stale for the next opening, so the frame the screen
/// goes is let through to clear it. A closed screen costs this condition
/// and nothing else.
pub fn screen_up(
    kind: fn() -> slotted::ui::ScreenKind,
) -> impl FnMut(Option<Res<slotted::ui::ScreenStack>>, Local<bool>) -> bool + Clone {
    move |stack, mut was_up| {
        let up = stack
            .as_deref()
            .is_some_and(|stack| stack.top().is_some_and(|top| top.kind == kind()));
        let run = up || *was_up;
        *was_up = up;
        run
    }
}

/// Run condition: a screen of `kind` is on top now, with nothing to clear
/// afterwards. For systems that return at once unless it is.
pub fn screen_on_top(
    kind: fn() -> slotted::ui::ScreenKind,
) -> impl FnMut(Option<Res<slotted::ui::ScreenStack>>) -> bool + Clone {
    move |stack| {
        stack
            .as_deref()
            .is_some_and(|stack| stack.top().is_some_and(|top| top.kind == kind()))
    }
}

/// Marks that [`register`] has run, so the second crate to ask finds the
/// work done.
#[derive(Resource)]
struct Registered;

/// Wires slotted over Bevy's UI plugins, once, and reports whether the app
/// has them: the screens and the HUD are registered only over a UI that
/// exists. Both `strata-client` and `veyra-ui` call this first, so the order
/// the game adds their plugins in does not matter.
///
/// `SlottedPlugins` is added unless something added slotted already: the
/// headless test harness builds its own stack first and this sits over it.
/// The two navigation plugins are the ones `DefaultPlugins` leaves out and
/// slotted's focus systems need.
pub fn register(app: &mut App) -> bool {
    if !app.is_plugin_added::<bevy::ui::UiPlugin>() {
        tracing::info!("no bevy_ui plugin: the slotted UI is not registered");
        return false;
    }
    if app.world().contains_resource::<Registered>() {
        return true;
    }
    app.insert_resource(Registered);
    if !app.is_plugin_added::<TabNavigationPlugin>() {
        app.add_plugins(TabNavigationPlugin);
    }
    if !app.is_plugin_added::<DirectionalNavigationPlugin>() {
        app.add_plugins(DirectionalNavigationPlugin);
    }
    if !app.is_plugin_added::<slotted::ecs::SlottedEcsPlugin>() {
        app.add_plugins(SlottedPlugins::default());
    }
    app.configure_sets(Update, ScreenWork.after(SlottedUiSet::Semantics))
        .add_systems(
            Update,
            (
                (pause::quit_on_choice, pause::exit_on_quit_confirmed).in_set(SlottedUiSet::Input),
                (pause::release_cursor, pause::hold_player_still).run_if(screen_open),
            ),
        );
    // After every `Update` system, slotted's own anchoring among them, and
    // before the layout reads the nodes. In `PostUpdate` and not ordered
    // after slotted's render set in `Update`: an edge from that set moves
    // where its commands are applied, and the screens' rebuilds lean on
    // where that is today.
    app.add_systems(
        bevy::app::PostUpdate,
        scale::centre_hud_layers.before(bevy::ui::UiSystems::Prepare),
    );
    fonts::register(app);
    true
}

#[cfg(test)]
mod screen_up_tests {
    use super::{screen_on_top, screen_up};
    use bevy::prelude::*;
    use slotted::ui::{Presentation, ScreenKind, ScreenStack, StackEntry};

    fn kind() -> ScreenKind {
        ScreenKind::new("test:screen")
    }

    fn set(app: &mut App, open: bool) {
        let mut stack = app.world_mut().resource_mut::<ScreenStack>();
        stack.entries_mut().clear();
        if open {
            stack.entries_mut().push(StackEntry {
                root: Entity::PLACEHOLDER,
                kind: kind(),
                presentation: Presentation::default(),
                menu: None,
                focus: None,
            });
        }
    }

    /// The extra frame after a close is what lets a screen's own "was on
    /// top" flag clear, so a reopen is a fresh push and a cover that lifts
    /// is not a close.
    #[test]
    fn screen_up_holds_one_frame_after_leaving() {
        let mut app = App::new();
        app.init_resource::<ScreenStack>();
        let mut up = bevy::ecs::system::IntoSystem::into_system(screen_up(kind));
        let mut on_top = bevy::ecs::system::IntoSystem::into_system(screen_on_top(kind));
        up.initialize(app.world_mut());
        on_top.initialize(app.world_mut());
        let mut frame = |app: &mut App, open: bool| {
            set(app, open);
            (
                up.run((), app.world_mut()).expect("runs"),
                on_top.run((), app.world_mut()).expect("runs"),
            )
        };
        assert_eq!(
            frame(&mut app, false),
            (false, false),
            "closed costs nothing"
        );
        assert_eq!(frame(&mut app, true), (true, true));
        assert_eq!(frame(&mut app, true), (true, true));
        assert_eq!(frame(&mut app, false), (true, false), "one frame to clear");
        assert_eq!(frame(&mut app, false), (false, false));
        assert_eq!(frame(&mut app, true), (true, true), "a reopen runs at once");
        assert_eq!(frame(&mut app, false), (true, false));
        assert_eq!(frame(&mut app, true), (true, true), "a cover lifting");
    }
}
