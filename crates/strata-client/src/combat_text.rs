//! Floating combat text: the damage a hit did, over whoever took it.
//!
//! A hit the local player lands shows its number over the victim's head,
//! a short rise and fade that says "that connected, for this much" where
//! the eye already is. A hit the local player takes shows a small danger
//! figure rising off the end of their own health bar, so the loss is read
//! beside the bar it came off rather than over the attacker.
//!
//! A world number keeps its anchor as a [`WorldPos`] and is projected
//! again every frame through the [`RenderOrigin`], so it stays over the
//! spot it was struck when the camera turns and across an origin rebase.
//! The projection runs in `PostUpdate` after the camera has been placed
//! for this frame and before the UI is laid out, so the number does not
//! trail the camera by a frame.
//!
//! Hits on one victim in quick succession are one number, not a column
//! of them: a hit within [`MERGE_SECS`] of the last adds to the figure,
//! pops it again and restarts its hold, while its rise carries on, so a
//! carbine's burst reads as one climbing total. A kill ends the run in
//! danger at the larger size. A new run over a victim whose last number
//! is still fading pushes that one up a line ([`STACK_GAP`] apart), as a
//! log strata: the old figure has stopped rising by then and the new
//! one would rise into it. A number over the world starts above the
//! victim's nameplate ([`crate::nameplates::PLATE_CLEARANCE`]).
//!
//! The words, the colours and the motion curve are pure functions with
//! tests; the systems only spawn, merge, place and retire nodes. Nothing
//! spawns while a screen is up, and what is in flight hides under it;
//! the numbers over the world hide while the local player is down.

use bevy::prelude::*;
use bevy::ui::{UiScale, UiSystems, UiTransform, Val2};
use slotted::ui::ScreenStack;
use slotted::ui::def::Tags;
use strata_core::WorldPos;
use strata_sim::combat::{CombatImpact, Combatant, GuardOutcome};
use strata_sim::{LocalPlayer, RenderOrigin, WorldPosition};

use crate::ClientSet;
use crate::combat::CombatCamera;
use crate::combat_hud::{DANGER, Meter, is_down, ui_point, ui_scale};
use crate::nameplates::PLATE_CLEARANCE;
use crate::ui::fonts::UiFonts;

/// The depth of a number over the world: with the markers, under the
/// scope's overlay and the HUD layers.
const NUMBER_Z: i32 = -2;
/// How long a number lives, seconds.
pub const LIFETIME: f32 = 0.9;
/// How long it holds full strength before fading, seconds.
pub const HOLD: f32 = 0.45;
/// How far it rises over its life, px.
pub const RISE: f32 = 48.0;
/// The pop: the scale it starts at, settling to 1 over [`POP_SECS`].
pub const POP_FROM: f32 = 1.3;
/// How long the pop takes, seconds.
pub const POP_SECS: f32 = 0.12;
/// The most numbers alive at once; the oldest makes room.
pub const LIVE_CAP: usize = 24;
/// How far above the victim's centre the number starts, metres: the
/// capsule's centre is at hip height, so this is just over the head.
pub const OVER_HEAD: f32 = 1.25;

/// A hit this soon after the last on the same victim joins its number,
/// seconds.
pub const MERGE_SECS: f32 = 0.3;
/// The air between a new number and the one it pushes up, px.
pub const STACK_GAP: f32 = 4.0;
/// How fast a pushed number moves up to its new line, per second: about
/// a pop's length.
const STACK_EASE: f32 = 25.0;

/// How far right of the health row a hurt's figure starts, px: past the
/// cluster panel's padding and border.
const HURT_GAP: f32 = 22.0;

/// The palette's text colour, for an ordinary hit.
const TEXT: Color = Color::srgb(0.902, 0.929, 0.953);
/// The palette's warm, for a melee hit.
const WARM: Color = Color::srgb(1.0, 0.706, 0.329);
/// The shadow under every number, so it reads over snow and sky.
const SHADOW: Color = Color::srgba(0.0, 0.0, 0.0, 0.75);

/// A number's colour and size.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Look {
    /// Text colour at full strength.
    pub colour: Color,
    /// Font size, px.
    pub size: f32,
}

/// The figure for `dealt`: a whole number, rounded.
#[must_use]
pub fn damage_words(dealt: f32) -> String {
    format!("{:.0}", dealt.max(0.0).round())
}

/// The figure for a hurt the local player took: "-24".
#[must_use]
pub fn hurt_words(dealt: f32) -> String {
    crate::ui::strings::english("strata.combat_text.hurt")
        .unwrap_or("-{amount}")
        .replace("{amount}", &damage_words(dealt))
}

/// The look of a hit the local player landed: text white, warm for a
/// cut, danger and larger for the one that killed.
#[must_use]
pub fn hit_look(melee: bool, lethal: bool) -> Look {
    if lethal {
        Look {
            colour: DANGER,
            size: 30.0,
        }
    } else if melee {
        Look {
            colour: WARM,
            size: 22.0,
        }
    } else {
        Look {
            colour: TEXT,
            size: 22.0,
        }
    }
}

/// The word a guard's outcome floats over the one who guarded, or none for
/// a hit that landed in the open. A word and not a colour, so it reads
/// without telling the three apart by hue.
#[must_use]
pub fn guard_words(outcome: GuardOutcome) -> Option<&'static str> {
    let key = match outcome {
        GuardOutcome::Open => return None,
        GuardOutcome::Blocked => "strata.combat_text.blocked",
        GuardOutcome::Parried => "strata.combat_text.parried",
        GuardOutcome::Broken => "strata.combat_text.guard_broken",
    };
    crate::ui::strings::english(key)
}

/// The look of a guard's word: the palette's text, a size between a hit's
/// figure and a hurt's, and danger for a guard that broke.
#[must_use]
pub fn guard_look(outcome: GuardOutcome) -> Look {
    Look {
        colour: if outcome == GuardOutcome::Broken {
            DANGER
        } else {
            TEXT
        },
        size: 20.0,
    }
}

/// How far from a damage figure's height a guard's word starts, metres
/// (negative: lower, near the waist, since the clearance lifts it again), so it reads as the guarder's own and
/// the two over one victim do not sit on each other or on the nameplate.
const WORD_LIFT: f32 = -1.0;

/// The share of a figure's rise a guard's word is held down by.
const WORD_DROP: f32 = 1.3;

/// The frame's guarded hits the local player landed or took: who guarded
/// and how.
fn guard_calls(
    impacts: impl Iterator<Item = CombatImpact>,
    local: Entity,
) -> Vec<(Entity, GuardOutcome)> {
    impacts
        .filter(|impact| impact.guard != GuardOutcome::Open)
        .filter_map(|impact| {
            let victim = impact.victim?;
            (impact.attacker == local || victim == local).then_some((victim, impact.guard))
        })
        .collect()
}

/// The look of a hurt: small, so it reads beside the bar without
/// competing with the figures on it.
#[must_use]
pub fn hurt_look() -> Look {
    Look {
        colour: DANGER,
        size: 18.0,
    }
}

/// A cubic ease-out over `0..=1`.
fn ease_out(x: f32) -> f32 {
    let x = x.clamp(0.0, 1.0);
    1.0 - (1.0 - x).powi(3)
}

/// How far up a number is at age `t`, px.
#[must_use]
pub fn rise(t: f32) -> f32 {
    RISE * ease_out(t / LIFETIME)
}

/// Its strength at age `t`: full for [`HOLD`], then eased out to nothing
/// at [`LIFETIME`].
#[must_use]
pub fn alpha(t: f32) -> f32 {
    if t <= HOLD {
        return 1.0;
    }
    let x = ((t - HOLD) / (LIFETIME - HOLD)).clamp(0.0, 1.0);
    // Smoothstep, so the fade starts and ends without a corner.
    1.0 - x * x * (3.0 - 2.0 * x)
}

/// Its scale at age `t`: the pop settling to 1.
#[must_use]
pub fn pop(t: f32) -> f32 {
    POP_FROM + (1.0 - POP_FROM) * ease_out(t / POP_SECS)
}

/// The horizontal offsets numbers take in turn, px, so a carbine's burst
/// fans out rather than stacking in one column.
const SPREAD: [f32; 7] = [0.0, 10.0, -10.0, 4.0, -14.0, 14.0, -4.0];

/// The `n`th number's horizontal offset, px.
#[must_use]
pub fn spread(n: u32) -> f32 {
    SPREAD[n as usize % SPREAD.len()]
}

/// Where a number hangs.
#[derive(Debug, Clone, Copy, PartialEq)]
enum Anchor {
    /// Over a point of the world.
    World(WorldPos),
    /// Just outside the cluster's right edge, level with the health
    /// bar; a child of the bar's row, so it hides and moves with it.
    HealthBar,
}

/// One floating number: one run of hits on one victim.
#[derive(Component, Debug)]
struct Floating {
    anchor: Anchor,
    /// Who took the hits.
    victim: Entity,
    /// Seconds since it spawned: the rise follows this, so a merge does
    /// not drop the figure back to its start.
    age: f32,
    /// Seconds since the latest hit joined it: the pop, the hold and the
    /// fade follow this, so every hit is felt and the total stays up.
    since_hit: f32,
    /// The running total.
    sum: f32,
    /// A kill ended the run; the next hit starts a new number.
    ended: bool,
    /// Spawn order, for retiring the oldest.
    serial: u64,
    offset: f32,
    /// How far it has been pushed up by newer numbers over the same
    /// victim, px.
    lift: f32,
    /// The push as drawn, easing towards [`Floating::lift`].
    lift_shown: f32,
    colour: Color,
}

impl Floating {
    /// Ages it by `dt` seconds.
    fn advance(&mut self, dt: f32) {
        self.age += dt;
        self.since_hit += dt;
        self.lift_shown += (self.lift - self.lift_shown) * (1.0 - (-STACK_EASE * dt).exp());
    }

    /// Whether a hit now joins this run.
    fn takes(&self) -> bool {
        !self.ended && self.since_hit < MERGE_SECS
    }

    /// Spent: faded out since its last hit.
    fn spent(&self) -> bool {
        self.since_hit >= LIFETIME
    }
}

/// One frame's hits on one victim, summed in arrival order: every hit in
/// a frame is within [`MERGE_SECS`] of the others.
#[derive(Debug, Clone, Copy)]
struct Run {
    victim: Entity,
    dealt: f32,
    melee: bool,
    lethal: bool,
    /// The local player took them.
    hurt: bool,
}

/// Registers the systems. Called from the combat view plugin under `ui`.
pub(crate) fn install(app: &mut App) {
    app.add_systems(
        Update,
        (spawn_numbers, spawn_guard_words).in_set(ClientSet::Hud),
    )
    .add_systems(
        PostUpdate,
        place_numbers
            .after(ClientSet::Follow)
            .before(UiSystems::Prepare),
    );
}

/// The frame's hits the local player landed or took, one [`Run`] per
/// victim in the order each was first hit.
fn runs(impacts: impl Iterator<Item = CombatImpact>, local: Entity) -> Vec<Run> {
    let mut runs: Vec<Run> = Vec::new();
    for impact in impacts {
        let Some(victim) = impact.victim else {
            continue;
        };
        // A figure that rounds to nought (a guard took the rest off) says
        // nothing: the guard's word says it instead.
        if impact.dealt < 0.5 || (impact.attacker != local && victim != local) {
            continue;
        }
        if let Some(run) = runs.iter_mut().find(|run| run.victim == victim) {
            run.dealt += impact.dealt;
            run.lethal |= impact.lethal;
            continue;
        }
        runs.push(Run {
            victim,
            dealt: impact.dealt,
            melee: impact.melee,
            lethal: impact.lethal,
            hurt: victim == local,
        });
    }
    runs
}

/// The words for a run's total.
fn words(hurt: bool, sum: f32) -> String {
    if hurt {
        hurt_words(sum)
    } else {
        damage_words(sum)
    }
}

/// `Update`: a number for every run of hits the local player lands or
/// takes; a hit soon after the last on the same victim joins its number.
#[allow(
    clippy::too_many_arguments,
    clippy::too_many_lines,
    reason = "the impacts, who is local, where the victim is, the fonts, the screen and the live numbers"
)]
fn spawn_numbers(
    mut commands: Commands,
    mut impacts: MessageReader<CombatImpact>,
    local: Query<Entity, With<LocalPlayer>>,
    positions: Query<&WorldPosition>,
    fonts: Option<Res<UiFonts>>,
    stack: Option<Res<ScreenStack>>,
    rows: Query<(Entity, &Tags)>,
    mut live: Query<(
        Entity,
        &mut Floating,
        &mut Text,
        &mut TextFont,
        &mut TextColor,
    )>,
    mut serial: Local<u64>,
) {
    let allowed = crate::ui::game_input_allowed(stack);
    let (Some(fonts), true, Ok(local)) = (fonts, allowed, local.single()) else {
        impacts.clear();
        return;
    };
    let mut alive: Vec<(u64, Entity)> = live.iter().map(|(e, f, ..)| (f.serial, e)).collect();
    alive.sort_unstable();
    for run in runs(impacts.read().copied(), local) {
        // A run under way on this victim takes the hits: the figure
        // becomes the total, the pop and the hold start again, the rise
        // goes on from where it is.
        let joined = live
            .iter_mut()
            .filter(|(_, number, ..)| number.victim == run.victim && number.takes())
            .max_by_key(|(_, number, ..)| number.serial);
        if let Some((_, mut number, mut text, mut font, mut colour)) = joined {
            number.sum += run.dealt;
            number.since_hit = 0.0;
            text.0 = words(run.hurt, number.sum);
            if run.lethal && !run.hurt {
                let look = hit_look(run.melee, true);
                number.colour = look.colour;
                colour.0 = look.colour;
                font.font_size = FontSize::Px(look.size);
            }
            number.ended |= run.lethal;
            continue;
        }
        let (anchor, look, parent) = if run.hurt {
            let Some((row, _)) = rows
                .iter()
                .find(|(_, tags)| tags.get(Tags::TEST_ID) == Some(Meter::Health.row()))
            else {
                continue;
            };
            (Anchor::HealthBar, hurt_look(), Some(row))
        } else {
            let Ok(position) = positions.get(run.victim) else {
                continue;
            };
            let over = WorldPos(position.0.0 + bevy::math::DVec3::Y * f64::from(OVER_HEAD));
            (Anchor::World(over), hit_look(run.melee, run.lethal), None)
        };
        // The numbers still over this victim move up a line for it.
        for (_, mut number, ..) in &mut live {
            if number.victim == run.victim {
                number.lift += look.size + STACK_GAP;
            }
        }
        if alive.len() >= LIVE_CAP {
            let (_, oldest) = alive.remove(0);
            commands.entity(oldest).despawn();
        }
        *serial += 1;
        #[allow(clippy::cast_possible_truncation, reason = "the spread only cycles")]
        let offset = if anchor == Anchor::HealthBar {
            0.0
        } else {
            spread(*serial as u32)
        };
        let mut entity = commands.spawn((
            Floating {
                anchor,
                victim: run.victim,
                age: 0.0,
                since_hit: 0.0,
                sum: run.dealt,
                ended: run.lethal,
                serial: *serial,
                offset,
                lift: 0.0,
                lift_shown: 0.0,
                colour: look.colour,
            },
            Name::new("hud.combat_text"),
            Text::new(words(run.hurt, run.dealt)),
            TextFont {
                font: fonts.display.clone().into(),
                font_size: FontSize::Px(look.size),
                ..default()
            },
            TextColor(look.colour),
            TextShadow {
                offset: Vec2::new(0.0, 1.5),
                color: SHADOW,
            },
            TextLayout::no_wrap(),
            Node {
                position_type: PositionType::Absolute,
                ..default()
            },
            UiTransform::default(),
            Pickable::IGNORE,
            // Placed before it is first drawn.
            Visibility::Hidden,
        ));
        if let Some(parent) = parent {
            entity.insert((ChildOf(parent), ZIndex(10)));
        } else {
            entity.insert(GlobalZIndex(NUMBER_Z));
        }
        alive.push((*serial, entity.id()));
    }
}

/// `Update`: the word over whoever guarded a hit the local player landed or
/// took. It is a number of its own that never merges, with no figure.
#[allow(
    clippy::too_many_arguments,
    reason = "the impacts, the player, the words standing, the fonts and the gate"
)]
fn spawn_guard_words(
    mut commands: Commands,
    mut impacts: MessageReader<CombatImpact>,
    local: Query<Entity, With<LocalPlayer>>,
    positions: Query<&WorldPosition>,
    mut standing: Query<(&mut Floating, &Text)>,
    fonts: Option<Res<UiFonts>>,
    stack: Option<Res<ScreenStack>>,
    mut serial: Local<u64>,
) {
    let allowed = crate::ui::game_input_allowed(stack);
    let (Some(fonts), true, Ok(local)) = (fonts, allowed, local.single()) else {
        impacts.clear();
        return;
    };
    for (victim, outcome) in guard_calls(impacts.read().copied(), local) {
        let (Some(words), Ok(position)) = (guard_words(outcome), positions.get(victim)) else {
            continue;
        };
        // A held trigger lands a bolt every few frames: the word already
        // floating over this victim is kept fresh rather than stacked, or a
        // burst piles the same word into an unreadable smear.
        if let Some((mut word, _)) = standing.iter_mut().find(|(word, text)| {
            word.serial >= u64::MAX / 2
                && word.victim == victim
                && word.since_hit < MERGE_SECS
                && text.0 == words
        }) {
            word.since_hit = 0.0;
            continue;
        }
        let look = guard_look(outcome);
        // A different word over the same victim moves the standing ones up a
        // line, as a figure does, so two never sit on each other.
        for (mut word, _) in &mut standing {
            if word.serial >= u64::MAX / 2 && word.victim == victim {
                word.lift += look.size + STACK_GAP;
            }
        }
        let over = WorldPos(position.0.0 + bevy::math::DVec3::Y * f64::from(OVER_HEAD + WORD_LIFT));
        // Their own serial range, past the numbers', so retiring the oldest
        // number never takes a word first.
        *serial += 1;
        commands.spawn((
            Floating {
                anchor: Anchor::World(over),
                victim,
                age: 0.0,
                since_hit: 0.0,
                sum: 0.0,
                ended: true,
                serial: u64::MAX / 2 + *serial,
                offset: 0.0,
                // Held down by most of the rise, so the word ends under the
                // nameplate and never climbs over the name it stands beside.
                lift: -WORD_DROP * RISE,
                lift_shown: -WORD_DROP * RISE,
                colour: look.colour,
            },
            Name::new("hud.combat_text.guard"),
            Text::new(words),
            TextFont {
                font: fonts.display.clone().into(),
                font_size: FontSize::Px(look.size),
                ..default()
            },
            TextColor(look.colour),
            TextShadow {
                offset: Vec2::new(0.0, 1.5),
                color: SHADOW,
            },
            TextLayout::no_wrap(),
            Node {
                position_type: PositionType::Absolute,
                ..default()
            },
            UiTransform::default(),
            Pickable::IGNORE,
            GlobalZIndex(NUMBER_Z),
            Visibility::Hidden,
        ));
    }
}

/// `PostUpdate`: ages every number, retires the spent ones and places the
/// rest: the world ones projected through this frame's camera.
#[allow(
    clippy::too_many_arguments,
    reason = "the clock, the origin, the camera, the gate and the numbers' own parts"
)]
fn place_numbers(
    mut commands: Commands,
    time: Res<Time>,
    origin: Res<RenderOrigin>,
    stack: Option<Res<ScreenStack>>,
    ui_scale_res: Option<Res<UiScale>>,
    local: Query<&Combatant, With<LocalPlayer>>,
    cameras: Query<(&Camera, &Transform), With<CombatCamera>>,
    mut numbers: Query<(
        Entity,
        &mut Floating,
        &mut Node,
        &mut UiTransform,
        &mut TextColor,
        &mut TextShadow,
        &mut Visibility,
    )>,
) {
    let allowed = crate::ui::game_input_allowed(stack);
    let scale = ui_scale(ui_scale_res.as_deref());
    // Down, the numbers over the world go with the rest of its UI; the
    // hurt beside the health bar stays with the cluster.
    let down = is_down(local.iter().next());
    // The camera is a root entity placed in `ClientSet::Follow`, before
    // transform propagation: its `Transform` is this frame's view.
    let camera = cameras
        .iter()
        .find(|(camera, _)| camera.is_active)
        .map(|(camera, transform)| (camera, GlobalTransform::from(*transform)));
    let dt = time.delta_secs();
    for (entity, mut number, mut node, mut transform, mut colour, mut shadow, mut visibility) in
        &mut numbers
    {
        number.advance(dt);
        if number.spent() {
            commands.entity(entity).despawn();
            continue;
        }
        let t = number.since_hit;
        let strength = alpha(t);
        let up = rise(number.age) + number.lift_shown;
        let shown = allowed
            && match number.anchor {
                Anchor::World(at) => {
                    !down
                        && camera.is_some_and(|(camera, view)| {
                            let render = origin.to_render(at);
                            match camera.world_to_viewport_with_depth(&view, render) {
                                Ok(screen) if screen.z > 0.0 => {
                                    let at = ui_point(screen.truncate(), scale);
                                    node.left = px(at.x + number.offset);
                                    node.top = px(at.y - PLATE_CLEARANCE - up);
                                    true
                                }
                                _ => false,
                            }
                        })
                }
                Anchor::HealthBar => {
                    // Past the row's figure and the panel's padding, so it
                    // rises over the world beside the cluster rather than
                    // through the name and the encounter's word above.
                    node.left = percent(100);
                    node.margin.left = px(HURT_GAP);
                    node.top = px(-4.0 - up);
                    true
                }
            };
        let wanted = if shown {
            Visibility::Inherited
        } else {
            Visibility::Hidden
        };
        if *visibility != wanted {
            *visibility = wanted;
        }
        if !shown {
            continue;
        }
        // Centred on its anchor, whatever its width; the pop scales about
        // the centre.
        transform.translation = match number.anchor {
            Anchor::World(_) => Val2::percent(-50.0, -50.0),
            Anchor::HealthBar => Val2::ZERO,
        };
        transform.scale = Vec2::splat(pop(t));
        colour.0 = number.colour.with_alpha(strength);
        shadow.color = SHADOW.with_alpha(SHADOW.alpha() * strength);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close(a: f32, b: f32) -> bool {
        (a - b).abs() < 1e-4
    }

    #[test]
    fn a_guard_says_its_outcome_in_words() {
        assert_eq!(guard_words(GuardOutcome::Open), None);
        assert_eq!(guard_words(GuardOutcome::Blocked), Some("Blocked"));
        assert_eq!(guard_words(GuardOutcome::Parried), Some("Parried"));
        assert_eq!(guard_words(GuardOutcome::Broken), Some("Guard broken"));
        assert_eq!(guard_look(GuardOutcome::Broken).colour, DANGER);
    }

    #[test]
    fn only_guarded_hits_of_the_local_player_are_called() {
        let mut world = World::new();
        let (local, other, third) = (
            world.spawn_empty().id(),
            world.spawn_empty().id(),
            world.spawn_empty().id(),
        );
        let hit = |attacker, victim, guard| CombatImpact {
            attacker,
            victim: Some(victim),
            from: WorldPos(bevy::math::DVec3::ZERO),
            point: WorldPos(bevy::math::DVec3::ZERO),
            melee: true,
            dealt: 0.0,
            lethal: false,
            guard,
            shot: None,
        };
        let calls = guard_calls(
            [
                hit(other, local, GuardOutcome::Blocked),
                hit(local, other, GuardOutcome::Parried),
                hit(other, third, GuardOutcome::Parried),
                hit(other, local, GuardOutcome::Open),
            ]
            .into_iter(),
            local,
        );
        assert_eq!(
            calls,
            [
                (local, GuardOutcome::Blocked),
                (other, GuardOutcome::Parried)
            ]
        );
    }

    #[test]
    fn the_figure_is_a_whole_number() {
        assert_eq!(damage_words(23.6), "24");
        assert_eq!(damage_words(23.4), "23");
        assert_eq!(damage_words(0.0), "0");
        assert_eq!(hurt_words(11.2), "-11");
    }

    #[test]
    fn a_kill_is_danger_and_larger_and_a_cut_is_warm() {
        let shot = hit_look(false, false);
        let cut = hit_look(true, false);
        let kill = hit_look(false, true);
        assert_eq!(shot.colour, TEXT);
        assert_eq!(cut.colour, WARM);
        assert_eq!(kill.colour, DANGER);
        assert_eq!(hit_look(true, true).colour, DANGER, "the kill wins");
        assert!(kill.size > shot.size);
        assert!(close(kill.size, 30.0) && close(shot.size, 22.0));
        assert!(hurt_look().size < shot.size);
    }

    #[test]
    fn the_curve_rises_holds_and_fades() {
        assert!(close(alpha(0.0), 1.0));
        assert!(close(alpha(0.45), 1.0));
        assert!(close(alpha(0.9), 0.0));
        assert!(alpha(0.7) > 0.0 && alpha(0.7) < 1.0);
        assert!(close(rise(0.0), 0.0));
        assert!(close(rise(0.45), RISE * 0.875), "{}", rise(0.45));
        assert!(close(rise(0.9), RISE));
        assert!(close(pop(0.0), 1.3));
        assert!(close(pop(POP_SECS), 1.0));
        assert!(close(pop(0.5), 1.0));
    }

    /// A world with the local player, a hostile, the health row and the
    /// fonts, ready for [`spawn_numbers`].
    fn scenario() -> (World, Entity, Entity, Entity) {
        use bevy::ecs::message::Messages;
        let mut world = World::new();
        world.init_resource::<Messages<CombatImpact>>();
        world.insert_resource(UiFonts {
            body: Handle::default(),
            display: Handle::default(),
        });
        let local = world
            .spawn((LocalPlayer, WorldPosition::new(0., 0., 0.)))
            .id();
        let hostile = world.spawn(WorldPosition::new(0., 0., 10.)).id();
        let row = world
            .spawn(Tags::new().with(Tags::TEST_ID, Meter::Health.row()))
            .id();
        (world, local, hostile, row)
    }

    fn impact(attacker: Entity, victim: Entity, dealt: f32, lethal: bool) -> CombatImpact {
        CombatImpact {
            attacker,
            victim: Some(victim),
            from: WorldPos::new(0., 0., 0.),
            point: WorldPos::new(0., 0., 0.),
            melee: false,
            dealt,
            lethal,
            guard: strata_sim::combat::GuardOutcome::Open,
            shot: None,
        }
    }

    /// Runs [`spawn_numbers`] over what was written, then drops it: each
    /// run is a fresh system whose reader would read it all again.
    #[allow(clippy::unwrap_used, reason = "the scenario owns its world")]
    fn frame(world: &mut World) {
        use bevy::ecs::system::RunSystemOnce;
        world.run_system_once(spawn_numbers).unwrap();
        world
            .resource_mut::<bevy::ecs::message::Messages<CombatImpact>>()
            .clear();
    }

    /// Writes `impact`, then spawns or merges its number.
    fn hit(world: &mut World, impact: CombatImpact) {
        world.write_message(impact);
        frame(world);
    }

    /// Ages every number by `dt`, as `place_numbers` does each frame.
    fn wait(world: &mut World, dt: f32) {
        for mut number in world.query::<&mut Floating>().iter_mut(world) {
            number.advance(dt);
        }
    }

    /// The live numbers' words, sizes and lifts, oldest first.
    fn numbers(world: &mut World) -> Vec<(String, f32, f32)> {
        let mut all: Vec<(u64, String, f32, f32)> = world
            .query::<(&Floating, &Text, &TextFont)>()
            .iter(world)
            .map(|(number, text, font)| {
                let FontSize::Px(size) = font.font_size else {
                    unreachable!("numbers are sized in pixels")
                };
                (number.serial, text.0.clone(), size, number.lift)
            })
            .collect();
        all.sort_by_key(|n| n.0);
        all.into_iter().map(|(_, w, s, l)| (w, s, l)).collect()
    }

    #[test]
    fn a_hurt_hangs_off_the_health_row_and_the_cap_holds() {
        let (mut world, local, hostile, row) = scenario();
        hit(&mut world, impact(hostile, local, 11.4, false));
        let hurts: Vec<(String, Entity)> = world
            .query::<(&Text, &ChildOf)>()
            .iter(&world)
            .map(|(text, parent)| (text.0.clone(), parent.parent()))
            .collect();
        assert_eq!(hurts, vec![("-11".to_owned(), row)]);
        for _ in 0..30 {
            let victim = world.spawn(WorldPosition::new(0., 0., 10.)).id();
            world.write_message(impact(local, victim, 23.6, false));
        }
        frame(&mut world);
        let live = world.query::<&Floating>().iter(&world).count();
        assert_eq!(live, LIVE_CAP, "the oldest make room");
        let scenery = CombatImpact {
            victim: None,
            ..impact(local, hostile, 5., false)
        };
        world.write_message(scenery);
        frame(&mut world);
        assert_eq!(world.query::<&Floating>().iter(&world).count(), LIVE_CAP);
    }

    #[test]
    fn a_burst_on_one_victim_is_one_climbing_number() {
        let (mut world, local, hostile, _) = scenario();
        hit(&mut world, impact(local, hostile, 24., false));
        wait(&mut world, 0.1);
        hit(&mut world, impact(local, hostile, 24., false));
        wait(&mut world, 0.1);
        hit(&mut world, impact(local, hostile, 24., false));
        assert_eq!(numbers(&mut world), vec![("72".to_owned(), 22.0, 0.0)]);
        let number = world
            .query::<&Floating>()
            .single(&world)
            .unwrap_or_else(|_| unreachable!());
        assert!(close(number.since_hit, 0.0), "the pop starts again");
        assert!(close(number.age, 0.2), "the rise carries on");
        // A pause as long as the merge window starts a new number, and
        // the one still fading over the same victim moves up a line.
        wait(&mut world, MERGE_SECS);
        hit(&mut world, impact(local, hostile, 24., false));
        assert_eq!(
            numbers(&mut world),
            vec![
                ("72".to_owned(), 22.0, 22.0 + STACK_GAP),
                ("24".to_owned(), 22.0, 0.0)
            ]
        );
        // Another victim's number is its own, and starts level.
        let other = world.spawn(WorldPosition::new(3., 0., 10.)).id();
        hit(&mut world, impact(local, other, 10., false));
        assert_eq!(numbers(&mut world)[2], ("10".to_owned(), 22.0, 0.0));
    }

    #[test]
    fn a_kill_joins_the_total_and_ends_the_run_in_danger() {
        let (mut world, local, hostile, _) = scenario();
        hit(&mut world, impact(local, hostile, 24., false));
        wait(&mut world, 0.1);
        hit(&mut world, impact(local, hostile, 13., true));
        assert_eq!(numbers(&mut world), vec![("37".to_owned(), 30.0, 0.0)]);
        let colour = world
            .query::<&TextColor>()
            .single(&world)
            .unwrap_or_else(|_| unreachable!())
            .0;
        assert_eq!(colour, DANGER);
        // The run is over: a hit straight after starts a new number.
        wait(&mut world, 0.05);
        hit(&mut world, impact(local, hostile, 5., false));
        assert_eq!(numbers(&mut world).len(), 2);
    }

    #[test]
    fn hurts_merge_into_one_figure_off_the_bar() {
        let (mut world, local, hostile, _) = scenario();
        hit(&mut world, impact(hostile, local, 6., false));
        wait(&mut world, 0.1);
        hit(&mut world, impact(hostile, local, 6., false));
        assert_eq!(numbers(&mut world), vec![("-12".to_owned(), 18.0, 0.0)]);
    }

    /// Runs [`spawn_guard_words`] over what was written, then drops it.
    #[allow(clippy::unwrap_used, reason = "the scenario owns its world")]
    fn words_frame(world: &mut World) {
        use bevy::ecs::system::RunSystemOnce;
        world.run_system_once(spawn_guard_words).unwrap();
        world
            .resource_mut::<bevy::ecs::message::Messages<CombatImpact>>()
            .clear();
    }

    /// The words floating now.
    fn words(world: &mut World) -> Vec<String> {
        world
            .query::<(&Floating, &Text)>()
            .iter(world)
            .filter(|(word, _)| word.serial >= u64::MAX / 2)
            .map(|(_, text)| text.0.clone())
            .collect()
    }

    #[test]
    fn a_bolt_the_slab_blocks_floats_its_word_once_for_a_burst() {
        // The word never showed in a still of the bulwark blocking bolts
        // because the still's shots never reached the slab; the system
        // itself has to speak for a blocked bolt as it speaks for a cut.
        let (mut world, local, hostile, _) = scenario();
        let mut bolt = impact(local, hostile, 0.7, false);
        bolt.guard = GuardOutcome::Blocked;
        bolt.shot = Some((strata_sim::combat::Weapon::Carbine, 9.0));
        world.write_message(bolt);
        words_frame(&mut world);
        assert_eq!(words(&mut world), ["Blocked"]);
        wait(&mut world, 0.1);
        world.write_message(bolt);
        words_frame(&mut world);
        assert_eq!(words(&mut world), ["Blocked"], "the next bolt keeps it");
        wait(&mut world, MERGE_SECS);
        world.write_message(bolt);
        words_frame(&mut world);
        assert_eq!(words(&mut world).len(), 2, "a pause starts another");
    }

    #[test]
    fn a_burst_fans_out_within_fourteen_pixels() {
        let offsets: Vec<f32> = (0..14).map(spread).collect();
        assert!(offsets.iter().all(|o| o.abs() <= 14.0));
        assert!(offsets.windows(2).all(|w| !close(w[0], w[1])));
    }
}
