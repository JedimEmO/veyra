//! The interface's sounds: a screen arriving or leaving, a control coming
//! under the pointer or the keyboard, a press, a toast, and the two moments
//! of progression the screens mark. The same table, take rotation and
//! master volume as the fight's cues, never spatial, and scaled by the
//! interface volume besides ([`Sounds::level`]).
//!
//! Every cue is an edge of state slotted already keeps (the stack, the
//! focus, `Hovered`, the toasts) or a message a screen already writes, so
//! nothing here asks a screen to know about sound; the one exception is
//! [`ProgressCue`], because a lesson and a switch look like any other
//! press from outside.

use std::cmp::Ordering;
use std::time::Duration;

use bevy::input_focus::InputFocus;
use bevy::picking::hover::Hovered;
use bevy::prelude::*;
use slotted::menu::{MenuChoice, Toast, ToastLevel};
use slotted::ui::{
    Focusable, RadioState, RichText, ScreenKind, ScreenStack, SelectState, SliderState,
    StackChanged, ToggleState, ValueChanged,
};

use super::{Sound, Sounds, play};
use crate::ui::bench::SwitchFade;
use crate::ui::progress::{ProgressCue, is_xp_words};

/// Hovers closer together than this are one: a pointer swept down a column
/// of buttons ticks, it does not buzz.
const HOVER_GAP: Duration = Duration::from_millis(60);

/// Frames after a stack change in which a new focus or hover is the
/// screen's own doing (its initial focus, the control that appeared under
/// a still pointer, a rebuilt sheet's nodes), which the open sound covers.
/// Hover lands a frame after layout, so a push is seen for three.
const QUIET_FRAMES: u8 = 3;

/// Registers the cues over the slotted UI. Without Bevy's UI there are no
/// screens and no messages to read, so nothing is added.
pub(super) fn install(app: &mut App) {
    if !app.is_plugin_added::<bevy::ui::UiPlugin>() {
        return;
    }
    app.add_systems(
        Update,
        (screens, presses, toasts, pointer)
            // After the screens answered their presses, so a lesson's cue
            // and its press are read in one frame.
            .after(crate::ui::sheet::answer_choices)
            .after(crate::ui::bench::answer_choices),
    );
}

/// The open screens' kinds as of the last change seen.
#[derive(Default)]
struct Shown(Vec<ScreenKind>);

/// What a change of the stack from `before` to `after` sounds like. A
/// replace pops and pushes in one command, so only the frame's last
/// message is compared: a screen rebuilding itself under the same kind
/// changes nothing, a different kind in the same place is an arrival.
fn stack_sound(before: &[ScreenKind], after: &[ScreenKind]) -> Option<Sound> {
    match after.len().cmp(&before.len()) {
        Ordering::Greater => Some(Sound::UiOpen),
        Ordering::Less => Some(Sound::UiClose),
        Ordering::Equal if after != before => Some(Sound::UiOpen),
        Ordering::Equal => None,
    }
}

/// `Update`: a screen pushed or popped. The bench closing under a
/// switch's fade is left to the switch's own sound.
fn screens(
    mut commands: Commands,
    mut sounds: ResMut<Sounds>,
    mut changes: MessageReader<StackChanged>,
    fade: Option<Res<SwitchFade>>,
    mut shown: Local<Shown>,
) {
    let Some(last) = changes.read().last() else {
        return;
    };
    let sound = stack_sound(&shown.0, &last.kinds);
    shown.0.clone_from(&last.kinds);
    let switching = fade.is_some_and(|fade| fade.target.is_some());
    // The map has an arrival of its own.
    let sound = match sound {
        Some(Sound::UiOpen)
            if last
                .kinds
                .last()
                .is_some_and(|kind| *kind == ScreenKind::new(crate::ui::map::KIND)) =>
        {
            Some(Sound::MapOpen)
        }
        other => other,
    };
    match sound {
        Some(Sound::UiClose) if switching => {}
        Some(sound) => {
            play(&mut commands, &mut sounds, sound, None, None);
        }
        None => {}
    }
}

/// `Update`: a press. A lesson or a switch plays its own sound instead of
/// the click its button would; a toggle, a select or a radio committing a
/// value clicks like a button.
fn presses(
    mut commands: Commands,
    mut sounds: ResMut<Sounds>,
    mut choices: MessageReader<MenuChoice>,
    mut cues: MessageReader<ProgressCue>,
    mut values: MessageReader<ValueChanged>,
    switches: Query<(), Or<(With<ToggleState>, With<SelectState>, With<RadioState>)>>,
) {
    let mut marked = Vec::new();
    for cue in cues.read() {
        let sound = match cue {
            ProgressCue::Learned => Sound::SkillLearn,
            ProgressCue::Switched => Sound::BenchSwitch,
        };
        if !marked.contains(&sound) {
            marked.push(sound);
            play(&mut commands, &mut sounds, sound, None, None);
        }
    }
    let chose = choices.read().count() > 0;
    let committed = values
        .read()
        .any(|value| value.source.is_some_and(|source| switches.contains(source)));
    if (chose || committed) && marked.is_empty() {
        play(&mut commands, &mut sounds, Sound::UiClick, None, None);
    }
}

/// What a toast of `level` sounds like; `xp` for the ledger's credits.
fn toast_sound(level: ToastLevel, xp: bool) -> Sound {
    match level {
        ToastLevel::Warning | ToastLevel::Error => Sound::UiRefuse,
        ToastLevel::Info if xp => Sound::XpCredit,
        ToastLevel::Info | ToastLevel::Success => Sound::UiToast,
    }
}

/// `Update`: a toast arriving, once per sound per frame: the ledger's two
/// kinds landing together are one credit. Its words are the text node's
/// key, spawned with it.
fn toasts(
    mut commands: Commands,
    mut sounds: ResMut<Sounds>,
    fade: Option<Res<SwitchFade>>,
    added: Query<(Entity, &Toast), Added<Toast>>,
    children: Query<&Children>,
    texts: Query<&RichText>,
) {
    // The switch's own toast ("X is out") lands a third of a second into
    // the switch's sound; a chime over it is two sounds for one event.
    let switching = fade.is_some_and(|fade| fade.target.is_some());
    let mut heard = Vec::new();
    for (entity, toast) in &added {
        let xp = children
            .iter_descendants(entity)
            .any(|node| texts.get(node).is_ok_and(|text| is_xp_words(&text.key.0)));
        let sound = toast_sound(toast.level, xp);
        if switching && sound != Sound::UiRefuse {
            continue;
        }
        if !heard.contains(&sound) {
            heard.push(sound);
            play(&mut commands, &mut sounds, sound, None, None);
        }
    }
}

/// What the pointer and the keyboard last pointed at.
#[derive(Default)]
struct Pointing {
    /// The control the last hover sound was for, or the one a quiet frame
    /// settled on.
    target: Option<Entity>,
    /// The focus as last seen, so a change is told from a frame that kept it.
    focus: Option<Entity>,
    /// When the last hover played, on the real clock.
    last: Option<Duration>,
    /// Frames left in which nothing ticks.
    quiet: u8,
}

/// `Update`: a different control under the pointer or the keyboard, and a
/// slider's step, as one tick at most every [`HOVER_GAP`].
#[allow(
    clippy::too_many_arguments,
    reason = "the focus, the hover and a slider's step are one rate-limited tick"
)]
fn pointer(
    mut commands: Commands,
    mut sounds: ResMut<Sounds>,
    time: Res<Time<Real>>,
    stack: Res<ScreenStack>,
    focus: Option<Res<InputFocus>>,
    mut changes: MessageReader<StackChanged>,
    mut values: MessageReader<ValueChanged>,
    hovered: Query<(Entity, &Hovered), (Changed<Hovered>, With<Focusable>)>,
    controls: Query<(), With<Focusable>>,
    sliders: Query<(), With<SliderState>>,
    mut state: Local<Pointing>,
) {
    if changes.read().count() > 0 {
        state.quiet = QUIET_FRAMES;
    }
    let focused = focus
        .as_deref()
        .and_then(InputFocus::get)
        .filter(|entity| controls.contains(*entity));
    let mut next = None;
    if focused != state.focus {
        state.focus = focused;
        next = focused;
    }
    for (entity, hovered) in &hovered {
        if hovered.get() {
            next = Some(entity);
        } else if state.target == Some(entity) && focused != Some(entity) {
            // Off the control: coming back onto it is a new hover.
            state.target = None;
        }
    }
    let stepped = values
        .read()
        .any(|value| value.source.is_some_and(|source| sliders.contains(source)));
    let fresh = next.is_some() && next != state.target;
    if next.is_some() {
        state.target = next;
    }
    if state.quiet > 0 {
        state.quiet -= 1;
        return;
    }
    if !(fresh || stepped) || stack.entries().is_empty() {
        return;
    }
    let now = time.elapsed();
    if state
        .last
        .is_some_and(|last| now.saturating_sub(last) < HOVER_GAP)
    {
        return;
    }
    state.last = Some(now);
    play(&mut commands, &mut sounds, Sound::UiHover, None, None);
}

#[cfg(test)]
mod tests {
    use super::super::tests::{cue_count, test_sounds};
    use super::*;

    /// The four cue systems over `MinimalPlugins`, with the messages and
    /// resources slotted would register, and no audio device: the
    /// `Playing` entities are the evidence.
    fn app() -> App {
        let mut app = App::new();
        app.add_plugins(MinimalPlugins)
            .insert_resource(test_sounds())
            .init_resource::<ScreenStack>()
            .init_resource::<SwitchFade>()
            .add_message::<StackChanged>()
            .add_message::<MenuChoice>()
            .add_message::<ProgressCue>()
            .add_message::<ValueChanged>()
            .add_systems(Update, (screens, presses, toasts, pointer));
        app.update();
        app
    }

    fn stack(app: &mut App, kinds: &[&str]) {
        app.world_mut().write_message(StackChanged {
            kinds: kinds.iter().map(|k| ScreenKind::new(k)).collect(),
        });
    }

    #[test]
    fn a_choice_clicks_and_a_lesson_chimes_instead() {
        let mut app = app();
        let entity = app.world_mut().spawn_empty().id();
        let choice = MenuChoice {
            id: "resume".to_owned(),
            screen: ScreenKind::new("slotted:pause"),
            entity,
        };
        app.world_mut().write_message(choice.clone());
        app.update();
        assert_eq!(cue_count(app.world_mut(), Sound::UiClick), 1);
        app.world_mut().write_message(choice);
        app.world_mut().write_message(ProgressCue::Learned);
        app.update();
        assert_eq!(cue_count(app.world_mut(), Sound::UiClick), 1);
        assert_eq!(cue_count(app.world_mut(), Sound::SkillLearn), 1);
        app.world_mut().write_message(ProgressCue::Switched);
        app.update();
        assert_eq!(cue_count(app.world_mut(), Sound::BenchSwitch), 1);
        let world = app.world_mut();
        for settings in world.query::<&PlaybackSettings>().iter(world) {
            assert!(!settings.spatial);
        }
    }

    #[test]
    fn the_interface_volume_scales_only_the_screens_sounds() {
        let mut sounds = test_sounds();
        sounds.interface = 0.5;
        assert!((sounds.level(Sound::UiClick) - 0.5).abs() < 1e-6);
        assert!((sounds.level(Sound::HitMarker) - 0.6).abs() < 1e-6);
    }

    #[test]
    fn the_map_opens_with_its_own_sound_and_other_screens_with_the_plain_one() {
        let mut app = app();
        stack(&mut app, &["slotted:pause"]);
        app.update();
        assert_eq!(cue_count(app.world_mut(), Sound::MapOpen), 0);
        stack(&mut app, &["slotted:pause", crate::ui::map::KIND]);
        app.update();
        assert_eq!(cue_count(app.world_mut(), Sound::MapOpen), 1);
        assert_eq!(cue_count(app.world_mut(), Sound::UiOpen), 1);
    }

    #[test]
    fn a_push_opens_a_replace_is_silent_and_a_pop_closes() {
        let mut app = app();
        stack(&mut app, &["slotted:pause"]);
        app.update();
        assert_eq!(cue_count(app.world_mut(), Sound::UiOpen), 1);
        // A replace by the same kind: the pop and the push in one frame.
        stack(&mut app, &["slotted:pause"]);
        stack(&mut app, &["slotted:pause", "strata:sheet"]);
        app.update();
        assert_eq!(cue_count(app.world_mut(), Sound::UiOpen), 2);
        stack(&mut app, &["slotted:pause"]);
        stack(&mut app, &["slotted:pause", "strata:sheet"]);
        app.update();
        assert_eq!(cue_count(app.world_mut(), Sound::UiOpen), 2);
        assert_eq!(cue_count(app.world_mut(), Sound::UiClose), 0);
        stack(&mut app, &["slotted:pause"]);
        app.update();
        assert_eq!(cue_count(app.world_mut(), Sound::UiClose), 1);
        // The bench leaving under a switch's fade is the switch's sound.
        app.world_mut().resource_mut::<SwitchFade>().target = Some(1);
        stack(&mut app, &[]);
        app.update();
        assert_eq!(cue_count(app.world_mut(), Sound::UiClose), 1);
    }

    /// A toast as slotted spawns one: the `Toast` on the panel, the words
    /// on a rich text node under it.
    fn raise(app: &mut App, words: &str, level: ToastLevel) {
        let text = RichText {
            key: slotted::ui::LocKey(words.to_owned()),
            args: slotted::ui::LocArgs::new(),
            style: slotted::ui::TextRole::Body,
            role: None,
            inline: false,
            layout: bevy::text::TextLayout::default(),
        };
        app.world_mut().spawn((
            Toast {
                level,
                remaining: Duration::from_secs(1),
            },
            children![(Node::default(), children![text])],
        ));
        app.update();
    }

    #[test]
    fn a_toast_under_the_switch_fade_leaves_the_switch_its_sound() {
        let mut app = app();
        app.world_mut().resource_mut::<SwitchFade>().target = Some(1);
        raise(&mut app, "Ilse Marrow is out", ToastLevel::Success);
        app.update();
        assert_eq!(cue_count(app.world_mut(), Sound::UiToast), 0);
        raise(&mut app, "Not while fighting", ToastLevel::Warning);
        app.update();
        assert_eq!(cue_count(app.world_mut(), Sound::UiRefuse), 1);
    }

    #[test]
    fn an_xp_toast_credits_and_a_warning_refuses() {
        let mut app = app();
        let words =
            crate::ui::progress::xp_words(strata_registry::progression::XpKind::Blade, 24, false);
        raise(&mut app, &words, ToastLevel::Info);
        assert_eq!(cue_count(app.world_mut(), Sound::XpCredit), 1);
        assert_eq!(cue_count(app.world_mut(), Sound::UiToast), 0);
        raise(&mut app, "Not certified on the pistol", ToastLevel::Warning);
        assert_eq!(cue_count(app.world_mut(), Sound::UiRefuse), 1);
        raise(
            &mut app,
            "A terminal and two trainers stand behind you",
            ToastLevel::Info,
        );
        assert_eq!(cue_count(app.world_mut(), Sound::UiToast), 1);
        assert_eq!(cue_count(app.world_mut(), Sound::XpCredit), 1);
    }

    #[test]
    fn hover_ticks_on_a_new_control_but_not_on_a_screens_first_focus() {
        let mut app = app();
        app.init_resource::<InputFocus>();
        let a = app.world_mut().spawn((Focusable, Hovered(false))).id();
        let b = app.world_mut().spawn((Focusable, Hovered(false))).id();
        app.world_mut()
            .resource_mut::<ScreenStack>()
            .entries_mut()
            .push(slotted::ui::StackEntry {
                root: a,
                kind: ScreenKind::new("slotted:pause"),
                presentation: slotted::ui::Presentation::default(),
                menu: None,
                focus: None,
            });
        // The push and its initial focus land together: no tick.
        stack(&mut app, &["slotted:pause"]);
        app.world_mut()
            .resource_mut::<InputFocus>()
            .set(a, bevy::input_focus::FocusCause::Navigated);
        for _ in 0..=QUIET_FRAMES {
            app.update();
        }
        assert_eq!(cue_count(app.world_mut(), Sound::UiHover), 0);
        app.world_mut().entity_mut(b).insert(Hovered(true));
        app.update();
        assert_eq!(cue_count(app.world_mut(), Sound::UiHover), 1);
        // Still on it: nothing more.
        app.update();
        assert_eq!(cue_count(app.world_mut(), Sound::UiHover), 1);
    }
}
