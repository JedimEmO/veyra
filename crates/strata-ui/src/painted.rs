//! Raw nodes painted from theme roles at a strength, for the nodes that
//! fade (the down card, a nameplate at the edge of its range, the map's
//! marks). Slotted's `Themed` cannot fade, so this is the engine's one
//! writer of such a node's colours, and the game's combat view and the
//! screens share it.

use bevy::prelude::*;
use bevy::ui::UiSystems;
use slotted::theme::{ActiveTheme, Paint, Role, Theme};

/// Registers `paint_roles` in `PostUpdate`, before layout. Called once by
/// whoever draws a `Painted` node; the combat HUD does.
pub fn install(app: &mut App) {
    app.add_systems(
        PostUpdate,
        paint_roles
            .run_if(resource_exists::<ActiveTheme>)
            .before(UiSystems::Prepare),
    );
}

/// A raw node painted from a theme role at a strength.
///
/// Slotted's `Themed` paints a node once, when its role or the theme
/// changes, at the role's own colours; a node that fades (the down card,
/// a nameplate at the edge of its range) needs those colours scaled every
/// frame it moves. `paint_roles` is the one writer of a `Painted`
/// node's colours, radius, shadow and type, so the theme stays the source
/// of every value and nothing fights over them. Solid and text roles
/// only: a glass role's backdrop cannot fade.
#[derive(Component, Debug, Clone, PartialEq)]
pub struct Painted {
    /// The role.
    pub role: Role,
    /// The strength, `0..=1`, multiplied into every colour's alpha.
    pub alpha: f32,
}

impl Painted {
    /// `role` at full strength.
    pub const fn new(role: &'static str) -> Self {
        Self {
            role: Role::new_static(role),
            alpha: 1.0,
        }
    }
}

/// `colour` at `alpha` of its own strength.
fn faded(colour: Color, alpha: f32) -> Color {
    colour.with_alpha(colour.alpha() * alpha)
}

/// One role resolved: its paint and, for text, the face to set.
#[derive(Clone)]
struct Resolved {
    paint: Paint,
    font: Option<TextFont>,
}

/// `PostUpdate`, before layout: every [`Painted`] node takes its role's
/// look at its strength. Roles are resolved once per theme load; a write
/// happens only where a value differs, so a still frame changes nothing.
#[allow(
    clippy::too_many_arguments,
    clippy::type_complexity,
    reason = "the theme, its loader and the parts of a node a role paints"
)]
fn paint_roles(
    mut commands: Commands,
    active: Res<ActiveTheme>,
    themes: Res<Assets<Theme>>,
    assets: Option<Res<AssetServer>>,
    mut events: MessageReader<AssetEvent<Theme>>,
    mut cache: Local<std::collections::HashMap<Role, Resolved>>,
    mut repaint_all: Local<bool>,
    mut nodes: ParamSet<(PaintQuery<'_, '_, Changed<Painted>>, PaintQuery<'_, '_>)>,
) {
    // A new theme repaints every node; otherwise only a node whose
    // `Painted` was written (spawned, faded, given another role) has
    // anything to take, so a still frame iterates none.
    if events.read().count() > 0 || active.is_changed() {
        cache.clear();
        *repaint_all = true;
    }
    let Some(theme) = themes.get(&active.0) else {
        return;
    };
    if std::mem::take(&mut *repaint_all) {
        paint(
            &mut commands,
            theme,
            assets.as_deref(),
            &mut cache,
            nodes.p1().iter_mut(),
        );
    } else {
        paint(
            &mut commands,
            theme,
            assets.as_deref(),
            &mut cache,
            nodes.p0().iter_mut(),
        );
    }
}

/// The parts of a node a role paints.
type PaintQuery<'w, 's, F = ()> = Query<
    'w,
    's,
    (
        Entity,
        &'static Painted,
        &'static mut Node,
        Option<&'static mut BackgroundColor>,
        Option<&'static mut BorderColor>,
        Option<&'static mut BoxShadow>,
        Option<&'static mut TextColor>,
        Option<&'static mut TextFont>,
    ),
    F,
>;

/// `paint_roles`' body over the nodes it was given.
#[allow(
    clippy::type_complexity,
    reason = "the parts of a node a role paints, as the query yields them"
)]
fn paint<'a>(
    commands: &mut Commands,
    theme: &Theme,
    assets: Option<&AssetServer>,
    cache: &mut std::collections::HashMap<Role, Resolved>,
    nodes: impl Iterator<
        Item = (
            Entity,
            &'a Painted,
            Mut<'a, Node>,
            Option<Mut<'a, BackgroundColor>>,
            Option<Mut<'a, BorderColor>>,
            Option<Mut<'a, BoxShadow>>,
            Option<Mut<'a, TextColor>>,
            Option<Mut<'a, TextFont>>,
        ),
    >,
) {
    for (entity, painted, mut node, background, border, shadow, text, font) in nodes {
        let resolved = cache.entry(painted.role.clone()).or_insert_with(|| {
            let paint = Paint::for_role(theme, &painted.role).unwrap_or_default();
            let font = paint.text_font(assets);
            Resolved { paint, font }
        });
        let paint = &resolved.paint;
        let alpha = painted.alpha;
        let mut entity = commands.entity(entity);
        if let Some(colour) = paint.background.map(|c| BackgroundColor(faded(c, alpha))) {
            match background {
                Some(mut current) if *current != colour => *current = colour,
                Some(_) => {}
                None => {
                    entity.insert(colour);
                }
            }
        }
        if let Some(colour) = paint.border.map(|c| BorderColor::all(faded(c, alpha))) {
            match border {
                Some(mut current) if *current != colour => *current = colour,
                Some(_) => {}
                None => {
                    entity.insert(colour);
                }
            }
        }
        if let Some(style) = paint.shadow {
            let wanted = BoxShadow(vec![ShadowStyle {
                color: faded(style.color, alpha),
                ..style
            }]);
            match shadow {
                Some(mut current) if *current != wanted => *current = wanted,
                Some(_) => {}
                None => {
                    entity.insert(wanted);
                }
            }
        }
        if let Some(radius) = paint.radius {
            let radius = BorderRadius::all(px(radius));
            if node.border_radius != radius {
                node.border_radius = radius;
            }
        }
        if let (Some(colour), Some(mut current)) = (paint.text_color(), text) {
            let colour = TextColor(faded(colour.0, alpha));
            if *current != colour {
                *current = colour;
            }
        }
        if let (Some(wanted), Some(mut current)) = (&resolved.font, font)
            && *current != *wanted
        {
            *current = wanted.clone();
        }
    }
}
