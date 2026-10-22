//! Enemy nameplates: who a hostile is and how much of it is left, over its
//! head, as a small glass pill rather than a bare bar in the world.
//!
//! One plate per hostile `Combatant`, made when the actor appears and
//! retired with it. Like the floating numbers it keeps its anchor in the
//! world and is projected through this frame's camera in `PostUpdate`, so
//! it stays on the body as the camera turns; it follows the interpolated
//! position, as the body does, so a walking hostile does not shake it.
//! The numbers rise from just above it ([`PLATE_CLEARANCE`]).
//!
//! A plate fades over the last [`FADE_M`] of its [`RANGE_M`] range, so a
//! far skirmish is not a field of labels, and hides when its actor is
//! down, under a screen, and while the local player is down. Its look is
//! theme roles painted at the plate's strength
//! ([`crate::combat_hud::Painted`]); the fill fraction and the fade are
//! pure functions with tests.

use bevy::prelude::*;
use bevy::ui::widget::ImageNode;
use bevy::ui::{UiScale, UiSystems, UiTransform, Val2};
use slotted::ui::ScreenStack;
use strata_core::WorldPos;
use veyra_sim::combat::{Action, Combatant};
use veyra_sim::{LocalPlayer, PreviousWorldPosition, RenderOrigin, WorldPosition};

use crate::action_bar::{MAX_STATUSES, PLATE_STATUS, set_image, status_view};
use crate::combat::CombatCamera;
use crate::combat_text::OVER_HEAD;
use strata_client::ClientSet;
use strata_ui::fonts::UiFonts;
use strata_ui::painted::Painted;
use strata_ui::world_ui::{ui_point, ui_scale, world_ui_shown};
use veyra_ui::icons::IconHandles;
use veyra_ui::world_ui::is_down;

/// A plate's width, px.
pub const PLATE_WIDTH: f32 = 120.0;
/// How far above its anchor a floating number's centre starts, px: the
/// plate's height, half a number's and a few pixels of air, so a number
/// rises from the plate's top edge and never through the name.
pub const PLATE_CLEARANCE: f32 = 52.0;
/// The farthest a plate is shown from the camera, metres: past the 80 m the
/// marksman holds, so the one hostile that stays far off is still named.
pub const RANGE_M: f32 = 100.0;
/// The last stretch of [`RANGE_M`] over which it fades out, metres.
pub const FADE_M: f32 = 15.0;
/// The highest the warning word is drawn, px: its height and a little air.
const WARNING_TOP_PX: f32 = 36.0;
/// The deepest of the three depths a plate takes by rank (this, +1, +2), so
/// even the nearest plate stays under the scope overlay, the down card and the
/// markers at -2, and over the hurt vignette at -6.
const PLATE_Z: i32 = -5;

/// Draws a plate at the depth of its rank, so the nearest is over the plates it
/// was lifted from.
fn set_depth(layer: &mut GlobalZIndex, rank: usize) {
    let z = PLATE_Z + 2 - i32::try_from(rank.min(2)).unwrap_or(2);
    if layer.0 != z {
        layer.0 = z;
    }
}

/// A plate's height as the declutter reads it, px: the clearance the
/// floating numbers already keep, which is the name and both bars.
pub const PLATE_HEIGHT: f32 = PLATE_CLEARANCE;
/// The air kept between two plates stacked on each other, px.
const PLATE_GAP: f32 = 3.0;
/// The most plates one stack lifts a plate over: past it a plate stays
/// where it falls, over the others, and the nearest is drawn on top.
const MAX_LIFTS: usize = 6;

/// How far each plate is lifted so that no two overlap, px (up is positive),
/// in the order given. A plate is `(distance, bottom centre)` in the
/// interface's pixels and stands [`PLATE_WIDTH`] wide and [`PLATE_HEIGHT`]
/// tall over its point; the nearest is placed first and keeps its place, and
/// each farther one is lifted over the plates it would cover, so four
/// hostiles in a line read as four names and not as one bar.
#[must_use]
pub fn declutter(plates: &[(f32, Vec2)]) -> Vec<f32> {
    let mut order: Vec<usize> = (0..plates.len()).collect();
    order.sort_by(|a, b| plates[*a].0.total_cmp(&plates[*b].0));
    let mut lifts = vec![0.0; plates.len()];
    // The rectangles placed so far: left, right, top and bottom.
    let mut placed: Vec<[f32; 4]> = Vec::with_capacity(plates.len());
    for index in order {
        let (_, at) = plates[index];
        let (left, right) = (at.x - 0.5 * PLATE_WIDTH, at.x + 0.5 * PLATE_WIDTH);
        let mut bottom = at.y;
        for _ in 0..MAX_LIFTS {
            let top = bottom - PLATE_HEIGHT;
            let blocker = placed
                .iter()
                .filter(|r| r[0] < right && r[1] > left && r[2] < bottom && r[3] > top)
                .min_by(|a, b| a[2].total_cmp(&b[2]));
            match blocker {
                // Over the highest plate in the way, so one lift clears the lot.
                Some(r) => bottom = r[2] - PLATE_GAP,
                None => break,
            }
        }
        lifts[index] = at.y - bottom;
        placed.push([left, right, bottom - PLATE_HEIGHT, bottom]);
    }
    lifts
}

/// Where a hostile's anchor over its head falls on the screen, and how far
/// from the camera it is, when the plate is to be shown and the point is in
/// front of the camera.
fn project(
    shown: bool,
    camera: Option<&(&Camera, GlobalTransform)>,
    origin: &RenderOrigin,
    at: WorldPos,
) -> Option<(Vec3, f32)> {
    let (camera, view) = camera.filter(|_| shown)?;
    let anchor = WorldPos(at.0 + bevy::math::DVec3::Y * f64::from(OVER_HEAD));
    let render = origin.to_render(anchor);
    let screen = camera.world_to_viewport_with_depth(view, render).ok()?;
    (screen.z > 0.0).then(|| (screen, view.translation().distance(render)))
}

/// Each seen plate's lift and its rank by distance, nearest first, by the
/// entity of its root.
fn lifts_by_plate(
    seen: &[(Entity, f32, Vec2)],
) -> bevy::platform::collections::HashMap<Entity, (f32, usize)> {
    let lifts = declutter(
        &seen
            .iter()
            .map(|(_, distance, at)| (*distance, *at))
            .collect::<Vec<_>>(),
    );
    let mut order: Vec<usize> = (0..seen.len()).collect();
    order.sort_by(|a, b| seen[*a].1.total_cmp(&seen[*b].1));
    order
        .into_iter()
        .enumerate()
        .map(|(rank, i)| (seen[i].0, (lifts[i], rank)))
        .collect()
}

/// The share of the health track that is filled.
#[must_use]
pub fn fill(health: f32, max: f32) -> f32 {
    if max > 0.0 {
        (health / max).clamp(0.0, 1.0)
    } else {
        0.0
    }
}

/// A plate's strength at `distance` metres from the camera: full until
/// the last [`FADE_M`] of [`RANGE_M`], then eased out to nothing.
#[must_use]
pub fn plate_alpha(distance: f32) -> f32 {
    let x = ((distance - (RANGE_M - FADE_M)) / FADE_M).clamp(0.0, 1.0);
    1.0 - x * x * (3.0 - 2.0 * x)
}

/// The words over a hostile with no `Name`.
fn fallback_name() -> &'static str {
    veyra_ui::strings::english("strata.nameplate.fallback").unwrap_or("Hostile")
}

/// A plate's root, and the parts it writes each frame.
#[derive(Component, Debug)]
struct Nameplate {
    /// The hostile it names.
    actor: Entity,
    /// The most health it has been seen with. An opponent's maximum is
    /// not on its `Combatant` (its fitted actor table is the player's
    /// row) and the client cannot tell the training target's row from a
    /// hostile's, so a plate made with the actor at full health keeps
    /// that, and an encounter reset brings it back to it.
    max: f32,
    /// The zero-sized node the plate hangs from. It is moved with its
    /// `UiTransform`, which does not dirty layout, where moving the plate's
    /// own `left` and `top` would put taffy back to work every frame; the
    /// plate itself only shifts by a share of its own size.
    anchor: Entity,
    name: Entity,
    shield: Entity,
    fill: Entity,
    /// Every painted node, the root first, which fade together.
    parts: [Entity; 5],
    /// The squares of the statuses the hostile bears, under the bars: the
    /// square, its picture and its veil.
    statuses: [[Entity; 3]; MAX_STATUSES],
}

/// Registers the systems. Called from the combat view plugin under `ui`.
pub(crate) fn install(app: &mut App) {
    app.add_systems(
        Update,
        (spawn_plates, update_plate_statuses, spawn_warning)
            .chain()
            .in_set(ClientSet::Hud),
    )
    .add_systems(
        PostUpdate,
        (place_plates, place_warning)
            .after(ClientSet::Follow)
            .before(UiSystems::Prepare),
    );
}

/// `Update`: a plate for every hostile that has none yet.
fn spawn_plates(
    mut commands: Commands,
    fonts: Option<Res<UiFonts>>,
    local: Query<&Combatant, With<LocalPlayer>>,
    actors: Query<(Entity, &Combatant, Option<&Name>), Without<LocalPlayer>>,
    plates: Query<&Nameplate>,
) {
    let Some(fonts) = fonts else {
        return;
    };
    let team = local.iter().next().map_or(0, |actor| actor.team);
    for (entity, actor, name) in &actors {
        if actor.team == team || plates.iter().any(|plate| plate.actor == entity) {
            continue;
        }
        let words = name.map_or_else(|| fallback_name().to_owned(), |n| n.as_str().to_owned());
        spawn_plate(&mut commands, &fonts, entity, actor.health, words);
    }
}

/// One plate: the name, then the shield line over the health track.
fn spawn_plate(commands: &mut Commands, fonts: &UiFonts, actor: Entity, max: f32, words: String) {
    let name = commands
        .spawn((
            Painted::new("strata.nameplate.name"),
            Text::new(words),
            // The theme's face before the first paint, so not one frame
            // shows Bevy's default.
            TextFont {
                font: fonts.body.clone().into(),
                font_size: FontSize::Px(12.0),
                ..default()
            },
            TextLayout::no_wrap(),
            Pickable::IGNORE,
        ))
        .id();
    let shield = commands
        .spawn((
            Painted::new("strata.nameplate.shield"),
            Node {
                width: percent(100),
                height: px(2.0),
                margin: UiRect::top(px(2.0)),
                align_self: AlignSelf::Start,
                ..default()
            },
            Pickable::IGNORE,
        ))
        .id();
    let fill = commands
        .spawn((
            Painted::new("strata.nameplate.health"),
            Node {
                width: percent(100),
                height: percent(100),
                ..default()
            },
            Pickable::IGNORE,
        ))
        .id();
    let track = commands
        .spawn((
            Painted::new("strata.nameplate.track"),
            Node {
                width: percent(100),
                height: px(4.0),
                margin: UiRect::top(px(2.0)),
                ..default()
            },
            Pickable::IGNORE,
        ))
        .add_child(fill)
        .id();
    let (strip, statuses) = spawn_statuses(commands);
    let root = commands
        .spawn((
            Name::new("hud.nameplate"),
            Painted::new("strata.nameplate"),
            Node {
                position_type: PositionType::Absolute,
                width: px(PLATE_WIDTH),
                flex_direction: FlexDirection::Column,
                align_items: AlignItems::Center,
                padding: UiRect::new(px(8.0), px(8.0), px(4.0), px(6.0)),
                border: UiRect::all(px(1.0)),
                ..default()
            },
            // Its bottom centre on the anchor.
            UiTransform::from_translation(Val2::percent(-50.0, -100.0)),
            GlobalZIndex(PLATE_Z),
            Pickable::IGNORE,
            // Placed before it is first drawn.
            Visibility::Hidden,
        ))
        .add_children(&[name, shield, track, strip])
        .id();
    let anchor = commands
        .spawn((
            Name::new("hud.nameplate.anchor"),
            Node {
                position_type: PositionType::Absolute,
                ..default()
            },
            UiTransform::default(),
            Pickable::IGNORE,
        ))
        .add_child(root)
        .id();
    commands.entity(root).insert(Nameplate {
        actor,
        max,
        anchor,
        name,
        shield,
        fill,
        parts: [root, name, shield, track, fill],
        statuses,
    });
}

/// The row of status squares under the bars: every square is made once,
/// hidden, and shown when the hostile bears a status for it.
fn spawn_statuses(commands: &mut Commands) -> (Entity, [[Entity; 3]; MAX_STATUSES]) {
    let slots = std::array::from_fn(|_| {
        let image = commands
            .spawn((
                Node {
                    position_type: PositionType::Absolute,
                    width: percent(100),
                    height: percent(100),
                    ..default()
                },
                Pickable::IGNORE,
            ))
            .id();
        let veil = commands
            .spawn((
                Painted::new("strata.bar.cool"),
                Node {
                    position_type: PositionType::Absolute,
                    left: px(0.0),
                    bottom: px(0.0),
                    width: percent(100),
                    height: percent(0),
                    ..default()
                },
                Pickable::IGNORE,
            ))
            .id();
        let square = commands
            .spawn((
                Painted::new("strata.bar.frame"),
                Node {
                    width: px(PLATE_STATUS),
                    height: px(PLATE_STATUS),
                    margin: UiRect::top(px(3.0)),
                    border: UiRect::all(px(1.0)),
                    display: Display::None,
                    ..default()
                },
                Pickable::IGNORE,
            ))
            .add_children(&[image, veil])
            .id();
        [square, image, veil]
    });
    let strip = commands
        .spawn((
            Node {
                flex_direction: FlexDirection::Row,
                column_gap: px(2.0),
                justify_content: JustifyContent::Center,
                ..default()
            },
            Pickable::IGNORE,
        ))
        .add_children(&slots.map(|[square, ..]| square))
        .id();
    (strip, slots)
}

/// `Update`: what each hostile bears is drawn under its bars, at most
/// [`MAX_STATUSES`], each square's veil draining with its time.
#[allow(
    clippy::type_complexity,
    reason = "the plates, the actors and the parts of a node a status writes"
)]
fn update_plate_statuses(
    mut commands: Commands,
    registries: Option<Res<veyra_sim::Registries>>,
    assets: Option<Res<AssetServer>>,
    mut handles: ResMut<IconHandles>,
    plates: Query<&Nameplate>,
    actors: Query<&Combatant>,
    mut nodes: Query<(&mut Node, Option<&mut ImageNode>)>,
) {
    let empty = strata_registry::IconCatalogue::default();
    let catalogue = registries.as_deref().map_or(&empty, |r| &r.0.icons);
    let tables = veyra_sim::combat_tables::current();
    for plate in &plates {
        let Ok(actor) = actors.get(plate.actor) else {
            continue;
        };
        let view = status_view(actor, &tables.abilities);
        for (slot, [square, image, veil]) in plate.statuses.iter().enumerate() {
            let status = view.get(slot);
            if let Ok((mut node, _)) = nodes.get_mut(*square) {
                let display = if status.is_some() {
                    Display::Flex
                } else {
                    Display::None
                };
                if node.display != display {
                    node.display = display;
                }
            }
            if let Ok((mut node, _)) = nodes.get_mut(*veil) {
                let height = percent(status.map_or(0.0, |s| (1.0 - s.share) * 100.0));
                if node.height != height {
                    node.height = height;
                }
            }
            if let (Some(status), Some(assets)) = (status, assets.as_deref())
                && let Some(file) = catalogue.file(&status.icon, 64)
                && let Ok((_, current)) = nodes.get_mut(*image)
            {
                let handle = handles.handle(assets, file);
                set_image(&mut commands, *image, current, handle, Color::WHITE);
            }
        }
    }
}

/// `PostUpdate`: every plate follows its hostile, fills to its health,
/// fades with distance and hides when it should; a plate whose hostile is
/// gone goes with it.
#[allow(
    clippy::too_many_arguments,
    clippy::type_complexity,
    reason = "the clock, the origin, the camera, the gate, the actors and the plate's parts"
)]
fn place_plates(
    mut commands: Commands,
    fixed: Res<Time<Fixed>>,
    origin: Res<RenderOrigin>,
    stack: Option<Res<ScreenStack>>,
    ui_scale_res: Option<Res<UiScale>>,
    local: Query<&Combatant, With<LocalPlayer>>,
    cameras: Query<(&Camera, &Transform), With<CombatCamera>>,
    actors: Query<(
        &WorldPosition,
        Option<&PreviousWorldPosition>,
        &Combatant,
        Option<&Name>,
    )>,
    mut plates: Query<(Entity, &mut Nameplate, &mut Visibility, &mut GlobalZIndex)>,
    mut anchors: Query<&mut UiTransform, Without<Nameplate>>,
    mut nodes: Query<&mut Node, Without<Nameplate>>,
    mut painted: Query<&mut Painted>,
    mut texts: Query<&mut Text>,
) {
    let allowed = world_ui_shown(stack, is_down(local.iter().next()));
    let scale = ui_scale(ui_scale_res.as_deref());
    let camera = cameras
        .iter()
        .find(|(camera, _)| camera.is_active)
        .map(|(camera, transform)| (camera, GlobalTransform::from(*transform)));
    let overstep = fixed.overstep_fraction_f64();
    // Where every plate falls this frame, so the overlapping ones can be
    // lifted apart before any is moved.
    let mut seen = Vec::new();
    for (root, plate, ..) in &plates {
        let Ok((position, previous, actor, _)) = actors.get(plate.actor) else {
            continue;
        };
        let shown = allowed && actor.action != Action::Dead;
        let at = previous.map_or(position.0, |p| p.interpolate(position.0, overstep));
        if let Some((screen, distance)) = project(shown, camera.as_ref(), &origin, at)
            .filter(|(_, distance)| plate_alpha(*distance) > 0.01)
        {
            seen.push((root, distance, ui_point(screen.truncate(), scale)));
        }
    }
    let lifted = lifts_by_plate(&seen);
    for (root, mut plate, mut visibility, mut layer) in &mut plates {
        let Ok((position, previous, actor, name)) = actors.get(plate.actor) else {
            commands.entity(plate.anchor).despawn();
            continue;
        };
        plate.max = plate.max.max(actor.health);
        let at = previous.map_or(position.0, |p| p.interpolate(position.0, overstep));
        let placed = project(
            allowed && actor.action != Action::Dead,
            camera.as_ref(),
            &origin,
            at,
        )
        .map(|(screen, distance)| (screen, plate_alpha(distance)))
        .filter(|(_, strength)| *strength > 0.01);
        let wanted = if placed.is_some() {
            Visibility::Inherited
        } else {
            Visibility::Hidden
        };
        if *visibility != wanted {
            *visibility = wanted;
        }
        let Some((screen, strength)) = placed else {
            continue;
        };
        let (lift, rank) = lifted.get(&root).copied().unwrap_or((0.0, 0));
        let at = ui_point(screen.truncate(), scale) - Vec2::Y * lift;
        // The nearest plate is drawn over the plates it was lifted from.
        set_depth(&mut layer, rank);
        if let Ok(mut anchor) = anchors.get_mut(plate.anchor) {
            let wanted = Val2::px(at.x, at.y);
            if anchor.translation != wanted {
                anchor.translation = wanted;
            }
        }
        for part in plate.parts {
            if let Ok(mut part) = painted.get_mut(part)
                && (part.alpha - strength).abs() > 1e-3
            {
                part.alpha = strength;
            }
        }
        // A post's people nobody has crossed wear a calm fill, not the
        // hostile's red; their name says what they are.
        if let Ok(mut fill_paint) = painted.get_mut(plate.fill) {
            let role = Painted::new(if actor.team == veyra_sim::encounters::NEUTRAL_TEAM {
                "strata.nameplate.neutral"
            } else {
                "strata.nameplate.health"
            })
            .role;
            if fill_paint.role != role {
                fill_paint.role = role;
            }
        }
        if let Ok(mut fill_node) = nodes.get_mut(plate.fill) {
            let share = fill(actor.health, plate.max);
            set_width(&mut fill_node, share);
        }
        if let Ok(mut shield) = nodes.get_mut(plate.shield) {
            let display = if actor.shield_capacity > 0.0 {
                Display::Flex
            } else {
                Display::None
            };
            if shield.display != display {
                shield.display = display;
            }
            set_width(&mut shield, fill(actor.shield, actor.shield_capacity));
        }
        let words: &str = match name {
            Some(name) => name.as_str(),
            None => fallback_name(),
        };
        if let Ok(mut text) = texts.get_mut(plate.name)
            && text.0 != words
        {
            words.clone_into(&mut text.0);
        }
    }
}

/// The warning word over the nearest of a post's people.
#[derive(Component, Debug)]
struct PostWarning;

/// `Update`: the one text node the warning is written to, made once and
/// hidden until a warning runs.
fn spawn_warning(
    mut commands: Commands,
    fonts: Option<Res<UiFonts>>,
    made: Query<(), With<PostWarning>>,
) {
    let Some(fonts) = fonts else {
        return;
    };
    if !made.is_empty() {
        return;
    }
    commands.spawn((
        Name::new("hud.post_warning"),
        PostWarning,
        Painted::new("strata.post.warning"),
        Text::new(String::new()),
        TextFont {
            font: fonts.display.clone().into(),
            font_size: FontSize::Px(22.0),
            ..default()
        },
        TextLayout::no_wrap(),
        Node {
            position_type: PositionType::Absolute,
            ..default()
        },
        UiTransform::from_translation(Val2::percent(-50.0, -100.0)),
        GlobalZIndex(PLATE_Z),
        Pickable::IGNORE,
        Visibility::Hidden,
    ));
}

/// The whole seconds a warning has left, never under one while it runs.
#[must_use]
pub fn warning_left(until: f64, now: f64) -> u32 {
    #[allow(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "a countdown of seconds, clamped at nought"
    )]
    let whole = (until - now).max(0.0).ceil() as u32;
    whole
}

/// `PostUpdate`: while a post's warning runs, its countdown stands over the
/// nearest of the post's people, above their plate, and goes with the
/// warning or when no one of the post is on screen.
#[allow(
    clippy::too_many_arguments,
    clippy::type_complexity,
    reason = "the clock, the origin, the camera, the gate and the post's people"
)]
fn place_warning(
    fixed: Res<Time<Fixed>>,
    origin: Res<RenderOrigin>,
    stack: Option<Res<ScreenStack>>,
    ui_scale_res: Option<Res<UiScale>>,
    fights: Option<Res<veyra_sim::encounters::Fights>>,
    clock: Option<Res<veyra_sim::encounters::PlayedClock>>,
    local: Query<(&WorldPosition, &Combatant), With<LocalPlayer>>,
    cameras: Query<(&Camera, &Transform), With<CombatCamera>>,
    people: Query<(
        &veyra_sim::encounters::EncounterId,
        &WorldPosition,
        Option<&PreviousWorldPosition>,
        &Combatant,
    )>,
    mut word: Query<(&mut Node, &mut Visibility, &mut Text), With<PostWarning>>,
) {
    let Ok((mut node, mut visibility, mut text)) = word.single_mut() else {
        return;
    };
    let player = local.iter().next();
    let allowed = world_ui_shown(stack, is_down(player.map(|(_, actor)| actor)));
    let warning = fights
        .as_deref()
        .and_then(veyra_sim::encounters::Fights::warning);
    let camera = cameras
        .iter()
        .find(|(camera, _)| camera.is_active)
        .map(|(camera, transform)| (camera, GlobalTransform::from(*transform)));
    // The nearest of the post's people the camera can see: one behind the
    // member would put the word nowhere.
    let placed = warning
        .zip(player)
        .zip(camera)
        .filter(|_| allowed)
        .and_then(|((warning, (me, _)), (camera, view))| {
            people
                .iter()
                .filter(|(id, _, _, actor)| **id == warning.post && actor.action != Action::Dead)
                .filter_map(|(_, position, previous, _)| {
                    let at = previous.map_or(position.0, |p| {
                        p.interpolate(position.0, fixed.overstep_fraction_f64())
                    });
                    let anchor = WorldPos(at.0 + bevy::math::DVec3::Y * f64::from(OVER_HEAD));
                    let screen = camera
                        .world_to_viewport_with_depth(&view, origin.to_render(anchor))
                        .ok()?;
                    (screen.z > 0.0).then_some((position.0.distance(me.0), screen))
                })
                .min_by(|a, b| a.0.total_cmp(&b.0))
                .map(|(_, screen)| (warning, screen))
        });
    let wanted = if placed.is_some() {
        Visibility::Inherited
    } else {
        Visibility::Hidden
    };
    if *visibility != wanted {
        *visibility = wanted;
    }
    let Some((warning, screen)) = placed else {
        return;
    };
    let at = ui_point(screen.truncate(), ui_scale(ui_scale_res.as_deref()));
    node.left = px(at.x);
    // Never off the top edge: a guard high in the frame still has its
    // countdown read.
    node.top = px((at.y - PLATE_CLEARANCE).max(WARNING_TOP_PX));
    let words = veyra_ui::strings::english("strata.post.warning")
        .unwrap_or_default()
        .replace(
            "{n}",
            &warning_left(warning.until, clock.map_or(0.0, |c| c.seconds)).to_string(),
        );
    if text.0 != words {
        text.0 = words;
    }
}

/// A fill's width as a share of its track; an empty one is not drawn, so
/// its rounded ends do not leave a dot.
fn set_width(node: &mut Node, share: f32) {
    let width = percent(share * 100.0);
    if node.width != width {
        node.width = width;
    }
    let display = if share > 0.0 {
        Display::Flex
    } else {
        Display::None
    };
    if node.display != display {
        node.display = display;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Four hostiles in a line at a few pixels' spacing are lifted into four
    /// stacked plates, the nearest where it was, none over another.
    #[test]
    fn plates_in_a_line_are_stacked_apart() {
        let plates = [
            (12.0, Vec2::new(800.0, 400.0)),
            (14.0, Vec2::new(812.0, 396.0)),
            (16.0, Vec2::new(820.0, 392.0)),
            (40.0, Vec2::new(830.0, 380.0)),
        ];
        let lifts = declutter(&plates);
        assert!(lifts[0].abs() < 1e-6, "the nearest stays: {lifts:?}");
        let rects: Vec<[f32; 2]> = plates
            .iter()
            .zip(&lifts)
            .map(|((_, at), lift)| [at.y - lift - PLATE_HEIGHT, at.y - lift])
            .collect();
        for (i, a) in rects.iter().enumerate() {
            for b in &rects[i + 1..] {
                assert!(a[0] >= b[1] || b[0] >= a[1], "{rects:?}");
            }
        }
    }

    /// Plates that already clear each other are not moved.
    #[test]
    fn plates_that_clear_each_other_stay() {
        let plates = [
            (10.0, Vec2::new(300.0, 400.0)),
            (11.0, Vec2::new(500.0, 400.0)),
            (12.0, Vec2::new(300.0, 200.0)),
        ];
        assert_eq!(declutter(&plates), vec![0.0; 3]);
    }

    /// The order the plates come in does not change who keeps its place.
    #[test]
    fn the_nearest_keeps_its_place_whatever_the_order() {
        let far = (30.0, Vec2::new(100.0, 100.0));
        let near = (5.0, Vec2::new(110.0, 100.0));
        let lifts = declutter(&[far, near]);
        assert!(lifts[1].abs() < 1e-6 && lifts[0] > 0.0);
    }

    fn close(a: f32, b: f32) -> bool {
        (a - b).abs() < 1e-5
    }

    #[test]
    fn the_fill_is_the_share_left() {
        assert!(close(fill(85.0, 85.0), 1.0));
        assert!(close(fill(42.5, 85.0), 0.5));
        assert!(close(fill(0.0, 85.0), 0.0));
        assert!(close(fill(90.0, 85.0), 1.0), "never past the track");
        assert!(close(fill(-3.0, 85.0), 0.0));
        assert!(close(fill(10.0, 0.0), 0.0), "no maximum, no fill");
    }

    #[test]
    fn a_plate_fades_over_the_last_fifteen_metres() {
        assert!(close(plate_alpha(0.0), 1.0));
        assert!(close(plate_alpha(RANGE_M - FADE_M), 1.0));
        assert!(close(plate_alpha(RANGE_M - FADE_M * 0.5), 0.5));
        assert!(close(plate_alpha(RANGE_M), 0.0));
        assert!(close(plate_alpha(RANGE_M + 10.0), 0.0));
        let mut last = 1.0;
        for step in 0..=60 {
            #[allow(clippy::cast_precision_loss, reason = "a small count")]
            let alpha = plate_alpha(step as f32);
            assert!(alpha <= last + 1e-6, "monotone at {step} m");
            last = alpha;
        }
    }

    #[test]
    #[allow(clippy::unwrap_used, reason = "the scenario owns its world")]
    fn a_hostile_gets_one_plate_and_an_ally_none() {
        use bevy::ecs::system::RunSystemOnce;
        let mut world = World::new();
        world.insert_resource(UiFonts {
            body: Handle::default(),
            display: Handle::default(),
        });
        world.spawn((LocalPlayer, Combatant::default()));
        let hostile = world
            .spawn((Combatant::on_team(1, 85.0), Name::new("Rusher")))
            .id();
        world.spawn(Combatant::default());
        world.run_system_once(spawn_plates).unwrap();
        world.run_system_once(spawn_plates).unwrap();
        let plates: Vec<(Entity, f32)> = world
            .query::<&Nameplate>()
            .iter(&world)
            .map(|plate| (plate.actor, plate.max))
            .collect();
        assert_eq!(plates, vec![(hostile, 85.0)]);
        let names: Vec<String> = world
            .query::<(&Text, &Painted)>()
            .iter(&world)
            .map(|(text, _)| text.0.clone())
            .collect();
        assert_eq!(names, vec!["Rusher".to_owned()]);
    }
}
