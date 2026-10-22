//! The framework for a screen that shows the sim's state and rebuilds when
//! that state moves (client-app review, F1, step four).
//!
//! A screen used to poll: every frame it was up it read its whole view out
//! of the world, compared it with the one on show and, nearly always, threw
//! it away. A [`SimScreen`] declares what its view reads as a
//! [`SystemParam`] of its own (`Inputs`, whose members answer
//! `is_changed`) and the framework builds the view only on a frame in which
//! one of them moved, or in which the screen's own state marked a rebuild.
//! A quiet frame costs the screen a handful of tick comparisons.
//!
//! A value the view takes from one of the sim's functions (`can_make`,
//! `preview`, `at_board`, `offers`, `wanted`) is an input through the
//! resources and components that function reads, so those are listed too;
//! a screen whose inputs miss one shows a stale figure, which is why each
//! screen's list sits beside its view and its harness tests move each one.
//!
//! A changed view goes through slotted's `patch_screen`: the new definition
//! is diffed against the one on show by `test_id` and only the nodes that
//! moved are touched (a text rewritten, a row spawned, a button respawned
//! where it stood), instead of the whole tree despawned and spawned again.

use std::marker::PhantomData;
use std::sync::Arc;

use bevy::app::{App, Update};
use bevy::ecs::change_detection::{DetectChanges, DetectChangesMut, Mut};
use bevy::ecs::component::Mutable;
use bevy::ecs::prelude::*;
use bevy::ecs::system::{
    ReadOnlySystemParam, StaticSystemParam, SystemParam, SystemParamItem, SystemState,
};
use slotted::menu::MenuChoice;
use slotted::ui::{
    ScreenDef, ScreenKind, ScreenStack, patch_screen, patch_screen_following, pop_screen,
    push_screen,
};

use crate::{ScreenWork, screen_on_top, screen_up};

/// What a screen's inputs say about themselves: whether any moved since the
/// system holding them last ran. A `Local` among them can narrow a member
/// that is written without changing (a position written back each tick) to
/// a change of value, which is why it takes the item mutably.
pub trait ChangeSource: SystemParam {
    /// Whether anything the view reads has changed.
    fn changed(inputs: &mut SystemParamItem<'_, '_, Self>) -> bool;
}

/// Whether the resource `res` was added or written since the reading
/// system last ran. A missing optional resource has not changed.
#[must_use]
pub fn moved<T: Resource>(res: &Option<Res<'_, T>>) -> bool {
    res.as_ref().is_some_and(DetectChanges::is_changed)
}

/// A value kept between runs, for an input that is written every tick
/// whether or not it changed: it reports a change only when `now` differs
/// from the last one it saw.
#[derive(Debug)]
pub struct Seen<T>(Option<T>);

impl<T> Default for Seen<T> {
    fn default() -> Self {
        Self(None)
    }
}

impl<T: PartialEq> Seen<T> {
    /// Whether `now` is not what was last seen, remembering it.
    pub fn differs(&mut self, now: T) -> bool {
        if self.0.as_ref() == Some(&now) {
            return false;
        }
        self.0 = Some(now);
        true
    }
}

/// A screen over the sim's state.
pub trait SimScreen: Resource<Mutability = Mutable> + Default + Sized {
    /// Everything the screen shows, comparable so an equal one is no rebuild.
    type View: Clone + PartialEq + Send + Sync + 'static;
    /// What the view reads from the world, other than the screen's own state.
    type Inputs: ChangeSource + ReadOnlySystemParam + 'static;
    /// What a press needs beyond the screen's state: the sim's inboxes.
    type Answering: SystemParam + 'static;

    /// The screen's kind, as a string so a set can be named by it.
    const KIND: &'static str;
    /// Whether a view that cannot be made any more (the machine is gone)
    /// closes the screen; otherwise it is left as it is.
    const CLOSES_WHEN_GONE: bool = false;

    /// The screen's kind.
    #[must_use]
    fn kind() -> ScreenKind {
        ScreenKind::new(Self::KIND)
    }

    /// The view for the world as it is, or none without a player or a
    /// registry. Reads only `inputs` and `state`.
    fn view(inputs: &SystemParamItem<'_, '_, Self::Inputs>, state: &Self) -> Option<Self::View>;

    /// The definition of `view`, with the focus and picks `state` keeps.
    fn def(view: &Self::View, state: &Self) -> ScreenDef;

    /// What the open screen was last built from.
    fn shown(state: &Self) -> Option<&Self::View>;

    /// Whether the screen's own state asks for a rebuild whatever the view
    /// says: a press to be shown, the sim's answer to one.
    fn marked(state: &Self) -> bool;

    /// Before a build: brings the state to what the world now allows (a
    /// batch held inside the stock, the best stock in the slots) so the view
    /// built next is the one that is shown.
    fn settle(_inputs: &SystemParamItem<'_, '_, Self::Inputs>, _state: &mut Self) {}

    /// After a build: keeps `view` as what is shown, clears the mark and
    /// writes back whatever the view resolved, so a press acts on what is
    /// on the screen.
    fn built(state: &mut Self, view: Self::View);

    /// One press of the screen, put where the sim will answer it.
    fn answer(
        _state: &mut Self,
        _choice: &MenuChoice,
        _with: &mut SystemParamItem<'_, '_, Self::Answering>,
    ) {
    }
}

/// The two steps of a screen's frame, for ordering the systems around them:
/// the sim's answers to a press are heard between them, and a screen's own
/// per-frame writes come after the rebuild.
#[derive(SystemSet, Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ScreenStep {
    /// The screen's [`SimScreen::KIND`].
    pub screen: &'static str,
    /// Which step.
    pub step: Step,
}

/// A step of [`ScreenStep`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Step {
    /// The presses of the screen are taken.
    Answer,
    /// The inputs are compared and the screen rebuilt if they moved.
    Rebuild,
}

impl ScreenStep {
    /// The answers of screen `S`.
    #[must_use]
    pub const fn answer<S: SimScreen>() -> Self {
        Self {
            screen: S::KIND,
            step: Step::Answer,
        }
    }

    /// The rebuild of screen `S`.
    #[must_use]
    pub const fn rebuild<S: SimScreen>() -> Self {
        Self {
            screen: S::KIND,
            step: Step::Rebuild,
        }
    }
}

/// What the framework keeps for screen `S`.
#[derive(Resource)]
pub struct ScreenSync<S: SimScreen> {
    /// The inputs as a system state, made on first use.
    inputs: Option<SystemState<S::Inputs>>,
    /// Whether the screen was on top the frame before, which is what makes
    /// a frame "settled": a screen pushed this frame is left alone, for a
    /// pop in the frame of its push races the stack's own deferred work on
    /// the new tree.
    was_top: bool,
    /// An input moved since the screen was last compared.
    moved: bool,
    /// A rebuild or a comparison is due this frame.
    due: bool,
    /// How many comparisons have run, which a probe holds still over quiet
    /// frames.
    compared: u32,
    marker: PhantomData<fn() -> S>,
}

impl<S: SimScreen> Default for ScreenSync<S> {
    fn default() -> Self {
        Self {
            inputs: None,
            was_top: false,
            moved: false,
            due: false,
            compared: 0,
            marker: PhantomData,
        }
    }
}

/// Registers screen `S`: its state and sync, its presses, and the dirty
/// check and the exclusive rebuild behind it, in [`ScreenStep`]s inside
/// [`ScreenWork`].
pub trait SimScreenApp {
    /// Adds screen `S`.
    fn add_sim_screen<S: SimScreen>(&mut self) -> &mut Self;
}

impl SimScreenApp for App {
    fn add_sim_screen<S: SimScreen>(&mut self) -> &mut Self {
        self.init_resource::<S>()
            .init_resource::<ScreenSync<S>>()
            .add_systems(
                Update,
                (
                    answer::<S>.in_set(ScreenStep::answer::<S>()),
                    (
                        mark_dirty::<S>.run_if(screen_up(S::kind)),
                        rebuild::<S>.run_if(screen_on_top(S::kind)).run_if(due::<S>),
                    )
                        .chain()
                        .in_set(ScreenStep::rebuild::<S>()),
                )
                    .chain()
                    .in_set(ScreenWork),
            )
    }
}

fn on_top(stack: &ScreenStack, kind: &ScreenKind) -> bool {
    stack.top().is_some_and(|top| top.kind == *kind)
}

/// `Update`: the screen's presses, one at a time, to its `answer`.
fn answer<S: SimScreen>(
    mut choices: MessageReader<MenuChoice>,
    stack: Res<ScreenStack>,
    mut state: ResMut<S>,
    mut with: StaticSystemParam<S::Answering>,
) {
    if !on_top(&stack, &S::kind()) {
        choices.clear();
        return;
    }
    for choice in choices.read() {
        if choice.screen != S::kind() {
            continue;
        }
        S::answer(&mut state, choice, &mut with);
    }
}

/// `Update`, while the screen is up: notes whether anything it reads moved,
/// and whether a frame is due. Runs for the one frame after the screen
/// leaves too, to clear the settled flag.
fn mark_dirty<S: SimScreen>(
    mut inputs: StaticSystemParam<S::Inputs>,
    stack: Res<ScreenStack>,
    state: Res<S>,
    mut sync: ResMut<ScreenSync<S>>,
) {
    // Read every run: a `Seen` among the inputs only learns by being asked.
    let changed = <S::Inputs as ChangeSource>::changed(&mut inputs);
    let top = on_top(&stack, &S::kind());
    let settled = sync.was_top;
    sync.was_top = top;
    if !top {
        // A screen covered by another keeps what moved under it, so the
        // frame it is uncovered compares and catches up.
        sync.moved |= changed || state.is_changed();
        sync.due = false;
        return;
    }
    // The screen's own state is an input too: a slider's value written with
    // nothing marked still has to reach the comparison.
    sync.moved |= changed || state.is_changed();
    sync.due = settled && (sync.moved || S::marked(&state));
}

fn due<S: SimScreen>(sync: Res<ScreenSync<S>>) -> bool {
    sync.due
}

/// [`World::resource_scope`] on the screen's state, which leaves its change
/// ticks as they were: the scope takes the resource out and puts it back
/// stamped as written, and a state that looks written by every rebuild
/// would be an input that moved every frame after the first.
fn quietly<S: SimScreen, U>(world: &mut World, scope: impl FnOnce(&mut World, Mut<S>) -> U) -> U {
    let (added, changed) = {
        let state = world.resource_ref::<S>();
        (state.added(), state.last_changed())
    };
    let out = world.resource_scope::<S, _>(scope);
    let mut state = world.resource_mut::<S>();
    state.set_last_added(added);
    state.set_last_changed(changed);
    out
}

/// The view for the world as it is, with `settle` first when asked.
fn make<S: SimScreen>(world: &mut World, settle: bool) -> Option<S::View> {
    let mut system = world
        .resource_mut::<ScreenSync<S>>()
        .bypass_change_detection()
        .inputs
        .take()
        .unwrap_or_else(|| SystemState::new(world));
    let view = if settle {
        quietly::<S, _>(world, |world, mut state| {
            let inputs = system.get(world).ok()?;
            S::settle(&inputs, state.bypass_change_detection());
            S::view(&inputs, &state)
        })
    } else {
        system
            .get(world)
            .ok()
            .and_then(|inputs| S::view(&inputs, world.resource::<S>()))
    };
    world
        .resource_mut::<ScreenSync<S>>()
        .bypass_change_detection()
        .inputs = Some(system);
    view
}

/// Builds the screen from the world as it is: settles the state, makes the
/// view, keeps it as what is shown and gives the definition. For the
/// screen's `open`, which then pushes or replaces it as it sees fit.
pub fn build<S: SimScreen>(world: &mut World) -> Option<ScreenDef> {
    let view = make::<S>(world, true)?;
    let def = quietly::<S, _>(world, |_, mut state| {
        let state = state.bypass_change_detection();
        let def = S::def(&view, state);
        S::built(state, view);
        def
    });
    // Whatever moved up to now is what this build shows.
    if let Some(mut sync) = world.get_resource_mut::<ScreenSync<S>>() {
        sync.moved = false;
        sync.due = false;
    }
    Some(def)
}

/// Brings the top screen to `def` by a keyed patch, so only the nodes that
/// moved are touched; a def the open screen cannot share a tree with (another
/// kind or presentation) replaces it. `follow` is for a rebuild a press
/// caused, whose `initial_focus` leads on to the next control even when it
/// repeats; a rebuild the clock or a drag caused leaves the focus where the
/// player put it.
pub fn patch_top(world: &mut World, def: ScreenDef, follow: bool) {
    let top = world.resource::<ScreenStack>().top().map(|e| e.root);
    match top {
        Some(root) if follow => patch_screen_following(&mut world.commands(), root, Arc::new(def)),
        Some(root) => patch_screen(&mut world.commands(), root, Arc::new(def)),
        None => {
            push_screen(&mut world.commands(), Arc::new(def), None);
        }
    }
}

/// Pushes `def`, or patches the top screen to it when screen `S` is already
/// there.
pub fn show<S: SimScreen>(world: &mut World, def: ScreenDef) {
    if on_top(world.resource::<ScreenStack>(), &S::kind()) {
        patch_top(world, def, true);
    } else {
        push_screen(&mut world.commands(), Arc::new(def), None);
    }
}

/// `Update`, on a due frame: compares the view with the one on show and
/// rebuilds the screen if it differs or the state marked it.
fn rebuild<S: SimScreen>(world: &mut World) {
    if let Some(mut sync) = world.get_resource_mut::<ScreenSync<S>>() {
        sync.due = false;
        sync.moved = false;
        sync.compared += 1;
    }
    let Some(view) = make::<S>(world, false) else {
        if S::CLOSES_WHEN_GONE {
            pop_screen(&mut world.commands());
        }
        return;
    };
    let marked = S::marked(world.resource::<S>());
    if !marked && S::shown(world.resource::<S>()) == Some(&view) {
        return;
    }
    if let Some(def) = build::<S>(world) {
        patch_top(world, def, marked);
    }
}

/// The view of screen `S` for the world as it is, without building the
/// screen: what a quiet frame cost before the dirty check. For probes.
#[doc(hidden)]
pub fn probe_view<S: SimScreen>(world: &mut World) -> Option<S::View> {
    make::<S>(world, false)
}

/// The dirty check of screen `S` as a system of its own, to be timed on a
/// quiet frame. For probes.
#[doc(hidden)]
pub fn probe_check<S: SimScreen>() -> impl bevy::ecs::system::System<In = (), Out = ()> {
    bevy::ecs::system::IntoSystem::into_system(mark_dirty::<S>)
}

/// How many times screen `S` has compared its view with the one on show
/// since the game began. For probes and tests.
#[must_use]
pub fn compared<S: SimScreen>(world: &World) -> u32 {
    world
        .get_resource::<ScreenSync<S>>()
        .map_or(0, |sync| sync.compared)
}
