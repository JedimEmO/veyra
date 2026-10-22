//! A weapon drawn from its baked pieces, with its mounts to pick (ticket
//! 0008, step 4).
//!
//! The content step rasterises every weapon side on, cut into its body and
//! its mounts, and writes an index beside the files
//! (`strata_registry::pictures`). The game reads that index into
//! [`WeaponPictures`] at startup, and the widget here,
//! `strata:weapon_diagram`, stacks the pieces of one chassis by the
//! index's rectangles, tints each by what its mount holds, and makes every
//! mount a button that answers with [`MountPicked`].
//!
//! The client does not link the weapon recipes, so the widget is told what
//! it cannot know: which mounts the weapon has, what each is called, what
//! is fitted and which are shut ([`DiagramParams`]). The index only says
//! where a picture goes.
//!
//! A drawn weapon is as wide as its parent gives it and as tall as its
//! frame's shape then makes it, up to a share of the window's height
//! ([`DiagramParams::max_height`]) and to what a parent with a height of
//! its own has left. A weapon that reaches that height is not left small
//! in the middle of a wide box: the well narrows to the frame's shape and
//! sits in the middle of its parent, so a pistol, whose frame is as tall
//! as it is wide, neither pushes the rest of a screen out of the window
//! nor stands in a field of nothing.
//!
//! The mount the screen has in focus is tinted, and tint alone does not
//! find a part a few pixels across, so it is also bracketed: four corners
//! round its rectangle, never closer together than [`BRACKET_MIN`]
//! ([`BRACKET_ID`]), and the mount's name on a tag in the frame's corner
//! ([`BRACKET_LABEL_ID`]).
//!
//! With no pictures for the chassis (a fresh clone, a headless test) the
//! same mounts are a row of labelled boxes with the same test ids, so a
//! screen built on the widget works before `just content` has run. The
//! box in focus has a mark before its name.

use std::collections::HashMap;

use bevy::asset::{AssetEvent, AssetServer, Assets};
use bevy::color::Color;
use bevy::ecs::prelude::*;
use bevy::input_focus::tab_navigation::TabIndex;
use bevy::picking::Pickable;
use bevy::picking::hover::Hovered;
use bevy::ui::auto_directional_navigation::AutoDirectionalNavigation;
use bevy::ui::widget::{ImageNode, NodeImageMode};
use bevy::ui::{
    AlignItems, AlignSelf, BorderRadius, FlexDirection, FlexWrap, JustifyContent, Node,
    PositionType, UiRect, UiTransform, Val, Val2,
};
use bevy::ui_widgets::Activate;
use serde::Deserialize;
use slotted::registry::Value;
use slotted::theme::{ActiveTheme, Paint, Role, Theme, Themed};
use slotted::ui::def::{ButtonVariant, LocKey, Tags, TextOpts, TextRole, UiNodeDef, WidgetKind};
use slotted::ui::screen::{SpawnCtx, Widget, WidgetRegistry};
use slotted::ui::widgets::{
    on_button_cancel, on_button_click, on_button_drag_end, on_button_press, on_button_release,
};
use slotted::ui::{
    ButtonRole, ButtonState, Focusable, SemanticLabel, SemanticRole, TestId, WidgetNode,
};
use strata_registry::pictures::{ChassisPictures, PICTURES_PATH, PictureSet, PiecePicture};

/// The widget kind.
pub const WIDGET: &str = "strata:weapon_diagram";
/// The kind a mount's button carries as its `WidgetNode`, whatever it
/// looks like, so slotted's tooltips, its test locators and a script's
/// `widget_activate` route see a mount as they see a built-in button.
pub const MOUNT_WIDGET: &str = "strata:weapon_mount";
/// The `test_id` prefix of a mount's button: `weapon.mount:muzzle`.
pub const MOUNT_ID: &str = "weapon.mount:";
/// The `test_id` of a diagram's root.
pub const DIAGRAM_ID: &str = "weapon.diagram";
/// The `test_id` of the box inside a pictured diagram that has the frame's
/// own shape: the pieces are placed in it, so a test measures against it.
pub const STAGE_ID: &str = "weapon.diagram.stage";
/// The `test_id` of the bracket round the mount in focus.
pub const BRACKET_ID: &str = "weapon.bracket";
/// The `test_id` of the tag that names the bracketed mount.
pub const BRACKET_LABEL_ID: &str = "weapon.bracket.label";

/// The least a bracket is across and down, in UI units. A reflex sight is
/// a dozen units across in a fitting screen's drawing, and corners that
/// close on something that small are a smudge; at this size they are a
/// mark, and the pointer's target is as large as a cell of a grid.
pub const BRACKET_MIN: f32 = 44.;
/// How far outside the part's rectangle the corners stand.
const BRACKET_OUT: f32 = 4.;
/// The length of a corner's arm and the weight of its line.
const BRACKET_ARM: f32 = 10.;
const BRACKET_LINE: f32 = 2.;
/// How far inside the frame's corner the bracketed mount's tag stands, and
/// its height.
const BRACKET_LABEL_INSET: f32 = 6.;
const BRACKET_LABEL_HEIGHT: f32 = 18.;

/// The mark before the name of the box in focus, where there is no picture
/// to bracket. A guillemet: the faces have no triangle.
pub const FOCUS_MARK: &str = "\u{203a} ";

/// The box's width over its height when there is no frame to take it from:
/// about a carbine's, so a screen laid out round the boxes keeps its shape
/// when the pictures arrive.
const DEFAULT_ASPECT: f32 = 2.6;

/// The tallest a pictured diagram grows when its screen sets no limit, in
/// percent of the window's height. A pistol's frame is as tall as it is
/// wide, so at the width of a carbine's it would be taller than the window
/// and push whatever is under it off the screen.
pub const MAX_HEIGHT_VH: f32 = 60.;

/// The asset source the baked files are mounted as.
const GENERATED_SOURCE: &str = "generated";

/// The frame behind the pieces and round the boxes.
pub(super) const FRAME_ROLE: &str = "strata.weapon.frame";
/// A piece under the pointer, unless it is the one in focus.
const HOVER_ROLE: &str = "strata.weapon.piece.hover";
/// A corner of the bracket round the mount in focus: its border is the
/// line, and it has no fill.
const BRACKET_ROLE: &str = "strata.weapon.bracket";
/// The tag that names the bracketed mount, with a ground of its own so it
/// reads over the frame's.
const BRACKET_LABEL_ROLE: &str = "strata.weapon.bracket.label";

/// Every baked weapon picture, by chassis. Empty until the game reads the
/// index, and empty for good on a clone that has not run `just content`.
#[derive(Resource, Debug, Clone, Default, PartialEq)]
pub struct WeaponPictures(pub PictureSet);

impl WeaponPictures {
    /// The index under `generated`, the content step's output root, as a
    /// source rooted there.
    ///
    /// A missing or unreadable index is no error: it is said once at info
    /// and the weapons are drawn as boxes. The pictures are no simulation
    /// state, so nothing else follows from their absence.
    #[must_use]
    pub fn read(generated: &strata_registry::files::Files) -> Self {
        let path = std::path::Path::new(PICTURES_PATH);
        let read = generated
            .read_string(PICTURES_PATH)
            .map_err(|error| error.to_string())
            .and_then(|text| PictureSet::from_ron(&text).map_err(|error| error.to_string()));
        match read {
            Ok(set) => {
                tracing::info!(
                    path = %path.display(),
                    chassis = set.chassis.len(),
                    "weapon pictures"
                );
                Self(set)
            }
            Err(error) => {
                tracing::info!(
                    path = %path.display(),
                    %error,
                    "no weapon pictures: weapons are drawn as boxes until `just content` runs"
                );
                Self::default()
            }
        }
    }
}

/// A mount was picked on a diagram, by a click or by the accept action.
#[derive(Message, Debug, Clone, PartialEq, Eq)]
pub struct MountPicked {
    /// The chassis the diagram shows, by its key.
    pub chassis: String,
    /// The mount, by its key.
    pub mount: String,
}

/// What a mount holds, which is how its piece is tinted.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MountState {
    /// The part the weapon is built with.
    #[default]
    Standard,
    /// A module.
    Fitted,
    /// Nothing may be fitted here now. Still to be picked: the screen is
    /// where the reason is read.
    Shut,
}

impl MountState {
    /// The name the parameters and the theme's roles use.
    const fn name(self) -> &'static str {
        match self {
            Self::Standard => "standard",
            Self::Fitted => "fitted",
            Self::Shut => "shut",
        }
    }
}

/// One mount as the caller describes it.
#[derive(Debug, Clone, PartialEq, Eq, Default, Deserialize)]
pub struct DiagramMount {
    /// The mount's stable key (`muzzle`), which the index and the answer
    /// both use.
    pub key: String,
    /// What the player reads: display text, not a locale key, since the
    /// names come from the weapon's own data.
    pub label: String,
    /// The module fitted, by its catalogue id; none for the standard part.
    #[serde(default)]
    pub module: Option<String>,
    /// What the mount holds.
    #[serde(default)]
    pub state: MountState,
}

/// The widget's parameters: the chassis, its mounts in the caller's order,
/// which of them the screen has in focus, and how tall the drawing may be.
#[derive(Debug, Clone, PartialEq, Default, Deserialize)]
pub struct DiagramParams {
    /// The chassis, by its key in the index (`patrol_carbine`).
    pub chassis: String,
    /// The mounts.
    #[serde(default)]
    pub mounts: Vec<DiagramMount>,
    /// The key of the mount in focus: the screen's selection, not the
    /// keyboard's.
    #[serde(default)]
    pub focus: Option<String>,
    /// The tallest the drawing grows, in percent of the window's height;
    /// [`MAX_HEIGHT_VH`] when the screen sets none. A screen with a list
    /// and a table under the drawing sets less.
    #[serde(default)]
    pub max_height: Option<f32>,
    /// Whether the well is always as tall as the limit allows, a weapon
    /// that is lower than that centred in it, so a screen whose weapon
    /// changes does not change height with it.
    #[serde(default)]
    pub fixed: bool,
}

impl DiagramParams {
    /// The parameters as the untyped value a `custom` node carries.
    ///
    /// Written by hand because slotted has a reader for its values and no
    /// writer; [`Self::from_value`] reads exactly this back.
    #[must_use]
    pub fn to_value(&self) -> Value {
        let text = |s: &str| Value::String(s.to_owned());
        let mounts = self
            .mounts
            .iter()
            .map(|mount| {
                let mut fields = vec![
                    (text("key"), text(&mount.key)),
                    (text("label"), text(&mount.label)),
                    (text("state"), text(mount.state.name())),
                ];
                if let Some(module) = &mount.module {
                    fields.push((text("module"), text(module)));
                }
                fields.into_iter().collect::<Value>()
            })
            .collect();
        let mut fields = vec![
            (text("chassis"), text(&self.chassis)),
            (text("mounts"), Value::Seq(mounts)),
        ];
        if let Some(focus) = &self.focus {
            fields.push((text("focus"), text(focus)));
        }
        if let Some(limit) = self.max_height {
            fields.push((
                text("max_height"),
                slotted::registry::from_model(&slotted::model::Value::Float(f64::from(limit))),
            ));
        }
        if self.fixed {
            fields.push((text("fixed"), Value::Bool(true)));
        }
        fields.into_iter().collect()
    }

    /// The parameters a `custom` node carries.
    ///
    /// # Errors
    ///
    /// A value that is not these parameters, in words.
    pub fn from_value(value: &Value) -> Result<Self, String> {
        let model = slotted::registry::to_model(value).map_err(|error| error.to_string())?;
        slotted::model::from_value(model).map_err(|error| error.to_string())
    }

    /// How `mount` is tinted: by what it holds, unless it is the one in
    /// focus.
    fn tone(&self, mount: &DiagramMount) -> Tone {
        Tone::of(
            mount.state,
            self.focus.as_deref() == Some(mount.key.as_str()),
        )
    }

    /// The `custom` node that draws these parameters.
    #[must_use]
    pub fn node(&self) -> UiNodeDef {
        UiNodeDef::Custom {
            kind: WidgetKind::new(WIDGET),
            params: self.to_value(),
            children: Vec::new(),
            tags: Tags::new().with(Tags::TEST_ID, DIAGRAM_ID),
        }
    }
}

/// How a piece is tinted at rest.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum Tone {
    Body,
    Standard,
    Fitted,
    Shut,
    Focus,
}

impl Tone {
    const fn of(state: MountState, focus: bool) -> Self {
        if focus {
            return Self::Focus;
        }
        match state {
            MountState::Standard => Self::Standard,
            MountState::Fitted => Self::Fitted,
            MountState::Shut => Self::Shut,
        }
    }

    /// The role whose fill multiplies the picture's own tone.
    const fn piece_role(self) -> &'static str {
        match self {
            Self::Body => "strata.weapon.piece.body",
            Self::Standard => "strata.weapon.piece.standard",
            Self::Fitted => "strata.weapon.piece.fitted",
            Self::Shut => "strata.weapon.piece.shut",
            Self::Focus => "strata.weapon.piece.focus",
        }
    }

    /// The role family of a box or a marker; the button paints its hover
    /// and press from it.
    const fn box_role(self) -> &'static str {
        match self {
            Self::Body | Self::Standard => "strata.weapon.box.standard",
            Self::Fitted => "strata.weapon.box.fitted",
            Self::Shut => "strata.weapon.box.shut",
            Self::Focus => "strata.weapon.box.focus",
        }
    }

    /// The role of a box's or a marker's label.
    const fn label_role(self) -> &'static str {
        match self {
            Self::Body | Self::Standard => "strata.weapon.label.standard",
            Self::Fitted => "strata.weapon.label.fitted",
            Self::Shut => "strata.weapon.label.shut",
            Self::Focus => "strata.weapon.label.focus",
        }
    }
}

/// On a diagram's root: which chassis it shows and whether from pictures.
#[derive(Component, Debug, Clone, PartialEq, Eq)]
pub struct Diagram {
    /// The chassis, by its key.
    pub chassis: String,
    /// Drawn from the baked pieces; false for the boxes.
    pub pictured: bool,
}

/// On a mount's button, whatever it looks like: a piece, a marker or a box.
#[derive(Component, Debug, Clone, PartialEq, Eq)]
pub struct MountButton {
    /// The chassis, by its key.
    pub chassis: String,
    /// The mount, by its key.
    pub mount: String,
}

/// On a drawn piece's image.
#[derive(Component, Debug, Clone, PartialEq, Eq)]
pub struct DiagramPiece {
    /// The mount it fills; none for the body.
    pub mount: Option<String>,
    /// The file, relative to the generated root.
    pub file: String,
    tone: Tone,
    /// The button whose hover lifts the tint; none for the body.
    owner: Option<Entity>,
}

/// A frame coordinate as a float. Frames are 1024 pixels on their long
/// side, far inside what this keeps exact.
fn px(v: u32) -> f32 {
    f32::from(u16::try_from(v).unwrap_or(u16::MAX))
}

/// A rectangle of the frame as an absolutely placed node, in percent, so
/// the stack scales with whatever width the box is given.
fn placed(rect: [u32; 4], frame: &ChassisPictures) -> Node {
    let (width, height) = (px(frame.width), px(frame.height));
    Node {
        position_type: PositionType::Absolute,
        left: Val::Percent(100. * px(rect[0]) / width),
        top: Val::Percent(100. * px(rect[1]) / height),
        width: Val::Percent(100. * px(rect[2]) / width),
        height: Val::Percent(100. * px(rect[3]) / height),
        ..Node::default()
    }
}

/// The fill of `role` in `theme`, which is a piece's tint. A role the
/// theme lacks tints nothing, so the picture shows in its own tone.
fn fill(theme: &Theme, role: &'static str) -> Color {
    Paint::for_role(theme, &Role::new_static(role))
        .and_then(|paint| paint.background)
        .unwrap_or(Color::WHITE)
}

/// The tint a piece of `tone` rests in, from the theme in force.
fn rest_tint(world: &World, tone: Tone) -> Color {
    world
        .get_resource::<ActiveTheme>()
        .zip(world.get_resource::<Assets<Theme>>())
        .and_then(|(active, themes)| themes.get(&active.0))
        .map_or(Color::WHITE, |theme| fill(theme, tone.piece_role()))
}

/// The image of a baked file, stretched over its node. Without an asset
/// server or without the `generated://` source (the headless tests) the
/// node is there with nothing to draw, as slotted's own image is.
///
/// It is spawned in its tone's tint. A pick or a step replaces the whole
/// screen, the files are already loaded, and [`tint_pieces`] is not ordered
/// against the rebuild, so an image left white here would be drawn white
/// for a frame on every one of them.
fn piece_image(world: &World, file: &str, tone: Tone) -> ImageNode {
    let handle = world
        .get_resource::<AssetServer>()
        .filter(|assets| assets.get_source(GENERATED_SOURCE).is_ok())
        .map(|assets| assets.load(format!("{GENERATED_SOURCE}://{file}")));
    ImageNode {
        image_mode: NodeImageMode::Stretch,
        color: rest_tint(world, tone),
        ..handle.map_or_else(ImageNode::default, ImageNode::new)
    }
}

/// The parts every mount's button shares: what makes it a button to
/// slotted's navigation, its tests, its tooltips and its accept action,
/// and to the pointer: everything its built-in button carries. Slotted's
/// own press and click observers are reused, so a click is the same
/// `Activate` a built-in button gives.
fn make_button(world: &mut World, entity: Entity, chassis: &str, mount: &DiagramMount) {
    let id = format!("{MOUNT_ID}{}", mount.key);
    let mut button = world.entity_mut(entity);
    button.insert((
        SemanticRole::Button,
        SemanticLabel(mount.label.clone()),
        ButtonState {
            variant: ButtonVariant::Secondary,
            disabled: false,
            pressed: false,
        },
        Hovered::default(),
        TabIndex(0),
        Focusable,
        AutoDirectionalNavigation::default(),
        Pickable::default(),
        WidgetNode(WidgetKind::new(MOUNT_WIDGET)),
        Tags::new()
            .with(Tags::TEST_ID, &id)
            .with("mount", &mount.key),
        TestId::new(id),
        MountButton {
            chassis: chassis.to_owned(),
            mount: mount.key.clone(),
        },
    ));
    button.observe(on_button_press);
    button.observe(on_button_release);
    button.observe(on_button_drag_end);
    button.observe(on_button_cancel);
    button.observe(on_button_click);
}

/// A mount as a small labelled button: a box in the row, or a marker on
/// the picture where the mount has no piece to show. `node` places it.
fn spawn_labelled(
    ctx: &mut SpawnCtx<'_>,
    parent: Entity,
    node: Node,
    chassis: &str,
    mount: &DiagramMount,
    tone: Tone,
) -> Entity {
    let tokens = ctx.tokens();
    let family = Role::new(tone.box_role());
    let entity = ctx
        .world
        .spawn((
            Node {
                padding: UiRect::axes(Val::Px(tokens.spacing.sm), Val::Px(tokens.spacing.xs)),
                border: UiRect::all(Val::Px(1.)),
                border_radius: BorderRadius::all(Val::Px(tokens.radii.sm)),
                align_items: AlignItems::Center,
                justify_content: JustifyContent::Center,
                flex_shrink: 0.,
                ..node
            },
            Themed(family.clone()),
            ButtonRole(family),
            ChildOf(parent),
        ))
        .id();
    make_button(ctx.world, entity, chassis, mount);
    // The box in focus says so with a mark as well as its colour.
    let mark = if tone == Tone::Focus { FOCUS_MARK } else { "" };
    ctx.spawn_children(
        entity,
        &[UiNodeDef::Text {
            key: LocKey(format!("{mark}{}", mount.label.to_uppercase())),
            style: TextRole::Caption,
            opts: TextOpts {
                wrap: false,
                role: Some(Role::new(tone.label_role())),
                ..TextOpts::default()
            },
            tags: Tags::new(),
        }],
    );
    // The label is content: a hit on it must be the button's, and the
    // button's own label already names it, so a role of its own would list
    // every marker twice in a tree snapshot, as slotted's control labels
    // would theirs.
    let children: Vec<Entity> = ctx
        .world
        .get::<Children>(entity)
        .map(|c| c.iter().collect())
        .unwrap_or_default();
    for child in children {
        ctx.world
            .entity_mut(child)
            .insert(Pickable::IGNORE)
            .remove::<SemanticRole>();
    }
    entity
}

/// A mount's piece as its button: the rectangle is the region to pick and
/// the image fills it.
fn spawn_piece(
    world: &mut World,
    stage: Entity,
    frame: &ChassisPictures,
    piece: &PiecePicture,
    chassis: &str,
    mount: &DiagramMount,
    tone: Tone,
) {
    let entity = world
        .spawn((placed(piece.rect, frame), ChildOf(stage)))
        .id();
    make_button(world, entity, chassis, mount);
    let image = piece_image(world, &piece.file, tone);
    world.spawn((
        Node {
            position_type: PositionType::Absolute,
            width: Val::Percent(100.),
            height: Val::Percent(100.),
            ..Node::default()
        },
        image,
        Pickable::IGNORE,
        DiagramPiece {
            mount: Some(mount.key.clone()),
            file: piece.file.clone(),
            tone,
            owner: Some(entity),
        },
        ChildOf(entity),
    ));
}

/// Where a marker goes: over the middle of `rect`, kept off the box's
/// edges so its label stays inside.
fn marker_at(rect: [u32; 4], frame: &ChassisPictures) -> (Node, UiTransform) {
    let x = 100. * (px(rect[0]) + px(rect[2]) / 2.) / px(frame.width);
    let y = 100. * (px(rect[1]) + px(rect[3]) / 2.) / px(frame.height);
    (
        Node {
            position_type: PositionType::Absolute,
            left: Val::Percent(x.clamp(8., 92.)),
            top: Val::Percent(y.clamp(10., 90.)),
            ..Node::default()
        },
        UiTransform::from_translation(Val2::new(Val::Percent(-50.), Val::Percent(-50.))),
    )
}

/// The most a well of a frame `aspect` wide to one high may be: as tall as
/// `limit` percent of the window's height, and as wide as the frame is at
/// that height, both in percent of the window's height. A frame that would
/// be taller than the limit at its parent's width is then a well of its own
/// shape in the middle, not a wide one with the weapon small inside it; a
/// low frame reaches its parent's width first and never meets either.
#[must_use]
pub fn well_limits(aspect: f32, limit: Option<f32>) -> (f32, f32) {
    let tall = limit.filter(|limit| *limit > 0.).unwrap_or(MAX_HEIGHT_VH);
    (tall * aspect, tall)
}

/// Where the bracket round `rect` goes: over the rectangle's middle and of
/// its size, but never smaller than [`BRACKET_MIN`] a side, so the corners
/// stand clear of a part a few pixels across.
fn bracket_at(rect: [u32; 4], frame: &ChassisPictures) -> (Node, UiTransform) {
    let (width, height) = (px(frame.width), px(frame.height));
    (
        Node {
            position_type: PositionType::Absolute,
            left: Val::Percent(100. * (px(rect[0]) + px(rect[2]) / 2.) / width),
            top: Val::Percent(100. * (px(rect[1]) + px(rect[3]) / 2.) / height),
            width: Val::Percent(100. * px(rect[2]) / width),
            height: Val::Percent(100. * px(rect[3]) / height),
            min_width: Val::Px(BRACKET_MIN),
            min_height: Val::Px(BRACKET_MIN),
            ..Node::default()
        },
        UiTransform::from_translation(Val2::new(Val::Percent(-50.), Val::Percent(-50.))),
    )
}

/// The bracket round the mount in focus: four corners outside its
/// rectangle, over everything and not to be picked, so a click inside it
/// still lands on the piece. Its name is on a tag in the frame's lower
/// right corner, under the barrel and ahead of the feed, where a weapon
/// drawn side on with its muzzle to the right has nothing: a tag at the
/// bracket lay over whichever part was next to the one it named.
fn spawn_bracket(
    ctx: &mut SpawnCtx<'_>,
    stage: Entity,
    frame: &ChassisPictures,
    rect: [u32; 4],
    label: &str,
) {
    let (node, shift) = bracket_at(rect, frame);
    let bracket = ctx
        .world
        .spawn((
            node,
            shift,
            Pickable::IGNORE,
            SemanticRole::Decor,
            TestId::new(BRACKET_ID),
            ChildOf(stage),
        ))
        .id();
    for (left, top) in [(true, true), (false, true), (true, false), (false, false)] {
        let line = |on: bool| Val::Px(if on { BRACKET_LINE } else { 0. });
        let out = |on: bool| if on { Val::Px(-BRACKET_OUT) } else { Val::Auto };
        ctx.world.spawn((
            Node {
                position_type: PositionType::Absolute,
                left: out(left),
                right: out(!left),
                top: out(top),
                bottom: out(!top),
                width: Val::Px(BRACKET_ARM),
                height: Val::Px(BRACKET_ARM),
                border: UiRect {
                    left: line(left),
                    right: line(!left),
                    top: line(top),
                    bottom: line(!top),
                },
                ..Node::default()
            },
            Themed(Role::new_static(BRACKET_ROLE)),
            Pickable::IGNORE,
            ChildOf(bracket),
        ));
    }
    spawn_tag(ctx, stage, label);
}

/// The name of the mount in focus on a tag in the lower right corner of
/// `stage`, not to be picked. The turning model's well names its mount the
/// same way, where it has no bracket to draw.
pub(super) fn spawn_tag(ctx: &mut SpawnCtx<'_>, stage: Entity, label: &str) {
    let tag = ctx
        .world
        .spawn((
            Node {
                position_type: PositionType::Absolute,
                right: Val::Px(BRACKET_LABEL_INSET),
                bottom: Val::Px(BRACKET_LABEL_INSET),
                height: Val::Px(BRACKET_LABEL_HEIGHT),
                padding: UiRect::horizontal(Val::Px(5.)),
                border: UiRect::all(Val::Px(1.)),
                align_items: AlignItems::Center,
                ..Node::default()
            },
            Themed(Role::new_static(BRACKET_LABEL_ROLE)),
            Pickable::IGNORE,
            SemanticRole::Decor,
            TestId::new(BRACKET_LABEL_ID),
            ChildOf(stage),
        ))
        .id();
    ctx.spawn_children(
        tag,
        &[UiNodeDef::Text {
            key: LocKey(format!("{FOCUS_MARK}{}", label.to_uppercase())),
            style: TextRole::Caption,
            opts: TextOpts {
                wrap: false,
                role: Some(Role::new_static(Tone::Focus.label_role())),
                ..TextOpts::default()
            },
            tags: Tags::new(),
        }],
    );
    let children: Vec<Entity> = ctx
        .world
        .get::<Children>(tag)
        .map(|c| c.iter().collect())
        .unwrap_or_default();
    for child in children {
        ctx.world
            .entity_mut(child)
            .insert(Pickable::IGNORE)
            .remove::<SemanticRole>();
    }
}

/// The stack: the body, then every mount's chosen piece in the index's
/// order, then a marker for each mount that has no piece to show, then the
/// bracket round the mount in focus.
///
/// Two boxes. The well is what the screen lays out: as wide as its parent,
/// as tall as the frame's shape makes it, but never taller than its limit
/// ([`well_limits`]) nor than a parent with a height of its own has left
/// for it, and never wider than the frame is at that limit. The stage
/// inside is the frame itself: it has the frame's shape exactly, is as
/// large as fits the well, and sits in its middle. The border is the well's
/// because a percentage is measured inside a node's border, so on a node
/// with both the pieces would be scaled by a box that is not the frame's
/// shape.
fn spawn_pictures(
    ctx: &mut SpawnCtx<'_>,
    params: &DiagramParams,
    frame: &ChassisPictures,
) -> Entity {
    let radius = ctx.tokens().radii.sm;
    let aspect = px(frame.width) / px(frame.height);
    let (widest, tallest) = well_limits(aspect, params.max_height);
    let root = ctx.spawn_node((
        Node {
            width: Val::Percent(100.),
            // Free to shrink in a parent that is out of room, where an
            // item's content otherwise holds it at its full height.
            min_height: Val::Px(0.),
            max_height: Val::Vh(tallest),
            max_width: Val::Vh(widest),
            height: if params.fixed {
                Val::Vh(tallest)
            } else {
                Val::Auto
            },
            flex_shrink: if params.fixed { 0. } else { 1. },
            align_self: AlignSelf::Center,
            border: UiRect::all(Val::Px(1.)),
            border_radius: BorderRadius::all(Val::Px(radius)),
            align_items: AlignItems::Center,
            justify_content: JustifyContent::Center,
            ..Node::default()
        },
        Themed(Role::new_static(FRAME_ROLE)),
        SemanticRole::Custom(WIDGET.to_owned()),
        SemanticLabel(params.chassis.clone()),
        WidgetNode(WidgetKind::new(WIDGET)),
        Diagram {
            chassis: params.chassis.clone(),
            pictured: true,
        },
    ));
    // The layout carries a height limit over to the width through the
    // aspect ratio, so the stage keeps its shape when the well is short.
    let stage = ctx
        .world
        .spawn((
            Node {
                width: Val::Percent(100.),
                max_height: Val::Percent(100.),
                aspect_ratio: Some(aspect),
                flex_shrink: 0.,
                ..Node::default()
            },
            Pickable::IGNORE,
            TestId::new(STAGE_ID),
            ChildOf(root),
        ))
        .id();
    if let Some(body) = frame.body().filter(|body| !body.hidden) {
        let image = piece_image(ctx.world, &body.file, Tone::Body);
        ctx.world.spawn((
            placed(body.rect, frame),
            image,
            Pickable::IGNORE,
            SemanticRole::Decor,
            DiagramPiece {
                mount: None,
                file: body.file.clone(),
                tone: Tone::Body,
                owner: None,
            },
            ChildOf(stage),
        ));
    }
    // The index's order is the stacking order, so the pieces are walked as
    // it lists them and each mount drawn when its chosen piece comes up.
    let mut drawn: Vec<&str> = Vec::new();
    for piece in &frame.pieces {
        let Some(mount) = params.mounts.iter().find(|mount| {
            piece.mount.as_deref() == Some(mount.key.as_str())
                && piece.module == mount.module
                && !piece.hidden
        }) else {
            continue;
        };
        if drawn.contains(&mount.key.as_str()) {
            continue;
        }
        drawn.push(&mount.key);
        spawn_piece(
            ctx.world,
            stage,
            frame,
            piece,
            &params.chassis,
            mount,
            params.tone(mount),
        );
    }
    spawn_markers(ctx, stage, params, frame, &drawn);
    // Round the part drawn, or where its marker stands; a mount with
    // neither is a box in the row along the edge, which says it is in
    // focus by its own frame.
    if let Some(mount) = params
        .mounts
        .iter()
        .find(|mount| params.focus.as_deref() == Some(mount.key.as_str()))
        && let Some(piece) = frame
            .piece(&mount.key, mount.module.as_deref())
            .or_else(|| frame.piece(&mount.key, None))
    {
        spawn_bracket(ctx, stage, frame, piece.rect, &mount.label);
    }
    root
}

/// A marker for each mount that has nothing to show: the body hides the
/// part, or the index does not know it. A hidden part's marker sits where
/// the part is; one the index lacks takes its standard part's place when
/// there is one, and otherwise a row along the bottom edge.
fn spawn_markers(
    ctx: &mut SpawnCtx<'_>,
    stage: Entity,
    params: &DiagramParams,
    frame: &ChassisPictures,
    drawn: &[&str],
) {
    let mut row: Option<Entity> = None;
    for mount in &params.mounts {
        if drawn.contains(&mount.key.as_str()) {
            continue;
        }
        let tone = params.tone(mount);
        let rect = frame
            .piece(&mount.key, mount.module.as_deref())
            .or_else(|| frame.piece(&mount.key, None))
            .map(|piece| piece.rect);
        if let Some(rect) = rect {
            let (node, shift) = marker_at(rect, frame);
            let marker = spawn_labelled(ctx, stage, node, &params.chassis, mount, tone);
            ctx.world.entity_mut(marker).insert(shift);
        } else {
            let row = *row.get_or_insert_with(|| {
                ctx.world
                    .spawn((
                        Node {
                            position_type: PositionType::Absolute,
                            left: Val::Percent(2.),
                            bottom: Val::Percent(4.),
                            column_gap: Val::Px(6.),
                            ..Node::default()
                        },
                        Pickable::IGNORE,
                        ChildOf(stage),
                    ))
                    .id()
            });
            spawn_labelled(ctx, row, Node::default(), &params.chassis, mount, tone);
        }
    }
}

/// The same box with no pictures: a labelled box a mount, in a row.
fn spawn_boxes(ctx: &mut SpawnCtx<'_>, params: &DiagramParams) -> Entity {
    let tokens = ctx.tokens();
    let root = ctx.spawn_node((
        Node {
            width: Val::Percent(100.),
            aspect_ratio: Some(DEFAULT_ASPECT),
            border: UiRect::all(Val::Px(1.)),
            border_radius: BorderRadius::all(Val::Px(tokens.radii.sm)),
            padding: UiRect::all(Val::Px(tokens.spacing.md)),
            flex_direction: FlexDirection::Row,
            flex_wrap: FlexWrap::Wrap,
            align_items: AlignItems::Center,
            align_content: bevy::ui::AlignContent::Center,
            justify_content: JustifyContent::Center,
            column_gap: Val::Px(tokens.spacing.md),
            row_gap: Val::Px(tokens.spacing.md),
            flex_shrink: 0.,
            ..Node::default()
        },
        Themed(Role::new_static(FRAME_ROLE)),
        SemanticRole::Custom(WIDGET.to_owned()),
        SemanticLabel(params.chassis.clone()),
        WidgetNode(WidgetKind::new(WIDGET)),
        Diagram {
            chassis: params.chassis.clone(),
            pictured: false,
        },
    ));
    for mount in &params.mounts {
        spawn_labelled(
            ctx,
            root,
            Node {
                min_width: Val::Px(tokens.sizes.control_height * 2.),
                min_height: Val::Px(tokens.sizes.control_height),
                ..Node::default()
            },
            &params.chassis,
            mount,
            params.tone(mount),
        );
    }
    root
}

/// `strata:weapon_diagram`.
#[derive(Debug, Default, Clone, Copy)]
pub struct DiagramWidget;

impl Widget for DiagramWidget {
    fn spawn(&self, ctx: &mut SpawnCtx<'_>, params: &Value, _children: &[UiNodeDef]) -> Entity {
        let params = DiagramParams::from_value(params).unwrap_or_else(|error| {
            tracing::warn!(%error, "bad params for {WIDGET}; drawing an empty box");
            DiagramParams::default()
        });
        let frame = ctx
            .world
            .get_resource::<WeaponPictures>()
            .and_then(|pictures| pictures.0.chassis.get(&params.chassis))
            .filter(|frame| frame.width > 0 && frame.height > 0)
            .cloned();
        match frame {
            Some(frame) => spawn_pictures(ctx, &params, &frame),
            None => spawn_boxes(ctx, &params),
        }
    }
}

/// Observer on `Activate`: a mount's button, from the pointer's click or
/// the accept action, is the pick.
fn on_mount_activate(
    activate: On<Activate>,
    mounts: Query<&MountButton>,
    mut picked: MessageWriter<MountPicked>,
) {
    if let Ok(mount) = mounts.get(activate.entity) {
        picked.write(MountPicked {
            chassis: mount.chassis.clone(),
            mount: mount.mount.clone(),
        });
    }
}

/// `Update`: keeps every piece in its role's fill, which the renderer
/// multiplies with the picture's own tone, so the shading the bake drew
/// survives the colour. A piece is spawned in its resting tint already
/// ([`piece_image`]); what is left for here is the pointer, which lifts a
/// piece under it unless it is the one in focus, and a theme that loads or
/// changes under an open screen. Roles are resolved once per theme load
/// and a write happens only where the colour differs.
fn tint_pieces(
    active: Res<ActiveTheme>,
    themes: Res<Assets<Theme>>,
    mut events: MessageReader<AssetEvent<Theme>>,
    mut cache: Local<HashMap<&'static str, Color>>,
    hovered: Query<&Hovered>,
    mut pieces: Query<(&DiagramPiece, &mut ImageNode)>,
) {
    if events.read().count() > 0 || active.is_changed() {
        cache.clear();
    }
    let Some(theme) = themes.get(&active.0) else {
        return;
    };
    for (piece, mut image) in &mut pieces {
        let lifted = piece.tone != Tone::Focus
            && piece
                .owner
                .and_then(|owner| hovered.get(owner).ok())
                .is_some_and(Hovered::get);
        let role = if lifted {
            HOVER_ROLE
        } else {
            piece.tone.piece_role()
        };
        let colour = *cache.entry(role).or_insert_with(|| fill(theme, role));
        if image.color != colour {
            image.color = colour;
        }
    }
}

/// `Startup`: the widget, under its kind.
fn register_widget(mut widgets: ResMut<WidgetRegistry>) {
    widgets.register(WidgetKind::new(WIDGET), DiagramWidget);
}

/// A stable key as words: `patrol_carbine` reads `Patrol carbine`, and
/// `fore_end` `Fore end`. What a mount is called where the data has only
/// its key.
#[must_use]
pub fn words(key: &str) -> String {
    let spaced = key.replace('_', " ");
    let mut chars = spaced.chars();
    chars.next().map_or_else(String::new, |first| {
        first.to_uppercase().collect::<String>() + chars.as_str()
    })
}

/// Registers the pictures, the widget and the message. Only called once
/// the UI is wired. The pictures are kept when the game put them there
/// first.
pub(crate) fn register(app: &mut bevy::app::App) {
    app.init_resource::<WeaponPictures>()
        .add_message::<MountPicked>()
        .add_observer(on_mount_activate)
        .add_systems(bevy::app::Startup, register_widget)
        .add_systems(
            bevy::app::Update,
            tint_pieces.run_if(resource_exists::<ActiveTheme>),
        );
}

#[cfg(test)]
mod tests {
    use super::*;

    fn params() -> DiagramParams {
        DiagramParams {
            chassis: "patrol_carbine".to_owned(),
            mounts: vec![
                DiagramMount {
                    key: "stock".to_owned(),
                    label: "Stock".to_owned(),
                    module: None,
                    state: MountState::Standard,
                },
                DiagramMount {
                    key: "muzzle".to_owned(),
                    label: "Muzzle".to_owned(),
                    module: Some("vigil_compensator".to_owned()),
                    state: MountState::Fitted,
                },
                DiagramMount {
                    key: "feed".to_owned(),
                    label: "Feed".to_owned(),
                    module: None,
                    state: MountState::Shut,
                },
            ],
            focus: Some("muzzle".to_owned()),
            max_height: None,
            fixed: false,
        }
    }

    #[test]
    fn the_params_survive_the_custom_nodes_value() {
        // A limit that is a whole number and one that is not both come
        // back as they went.
        for limit in [30., 27.5] {
            let limited = DiagramParams {
                max_height: Some(limit),
                ..params()
            };
            assert_eq!(DiagramParams::from_value(&limited.to_value()), Ok(limited));
        }
        let params = params();
        assert_eq!(DiagramParams::from_value(&params.to_value()), Ok(params));
        // A mount with only its key and label is a standard part, and no
        // focus is no focus: what a screen file would write by hand.
        let bare: Value = ron::from_str(
            r#"{"chassis": "field_blade", "mounts": [{"key": "edge", "label": "Edge"}]}"#,
        )
        .expect("a value");
        let read = DiagramParams::from_value(&bare).expect("reads");
        assert_eq!(read.mounts[0].state, MountState::Standard);
        assert_eq!(read.mounts[0].module, None);
        assert_eq!(read.focus, None);
        assert_eq!(read.max_height, None);
        assert!(DiagramParams::from_value(&Value::Bool(true)).is_err());
    }

    #[test]
    fn every_role_the_diagram_names_is_in_the_theme() {
        let path = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../assets/themes/veyra.theme.ron"
        );
        let theme = Theme::from_ron(&std::fs::read_to_string(path).expect("the theme"))
            .expect("the theme parses");
        let mut roles = vec![FRAME_ROLE, HOVER_ROLE, BRACKET_ROLE, BRACKET_LABEL_ROLE];
        for tone in [
            Tone::Body,
            Tone::Standard,
            Tone::Fitted,
            Tone::Shut,
            Tone::Focus,
        ] {
            roles.extend([tone.piece_role(), tone.box_role(), tone.label_role()]);
        }
        for role in roles {
            assert!(
                theme.roles.contains_key(&Role::new_static(role)),
                "{role} is not in the theme"
            );
        }
        // The tint is a fill: a piece role that is not a solid would tint
        // nothing and the state would not show.
        for tone in [Tone::Body, Tone::Fitted, Tone::Focus] {
            let paint = Paint::for_role(&theme, &Role::new_static(tone.piece_role()));
            assert!(paint.and_then(|paint| paint.background).is_some());
        }
    }

    fn frame(width: u32, height: u32) -> ChassisPictures {
        ChassisPictures {
            width,
            height,
            metres_per_pixel: 0.001,
            origin: [0., 0.],
            whole: strata_registry::pictures::Placed {
                rect: [0, 0, width, height],
                file: String::new(),
                icon: None,
            },
            pieces: Vec::new(),
        }
    }

    /// The bracket is the part's rectangle, about its middle, and never
    /// closes on a part smaller than a cell: a reflex sight is 46 by 41 of
    /// a frame 1024 across, which a fitting screen draws a fifth of that
    /// size.
    #[test]
    fn the_bracket_has_a_least_size() {
        let (node, shift) = bracket_at([312, 46, 46, 41], &frame(1024, 398));
        assert_eq!(node.position_type, PositionType::Absolute);
        assert_eq!(node.min_width, Val::Px(BRACKET_MIN));
        assert_eq!(node.min_height, Val::Px(BRACKET_MIN));
        let percent = |val: Val| match val {
            Val::Percent(percent) => percent,
            other => panic!("{other:?} is no percentage"),
        };
        // Its middle is the part's middle, and the shift takes half its
        // own size back, whatever size the least makes it.
        assert!((percent(node.left) - 100. * 335. / 1024.).abs() < 1e-3);
        assert!((percent(node.top) - 100. * 66.5 / 398.).abs() < 1e-3);
        assert!((percent(node.width) - 100. * 46. / 1024.).abs() < 1e-3);
        assert!((percent(node.height) - 100. * 41. / 398.).abs() < 1e-3);
        assert_eq!(
            shift.translation,
            Val2::new(Val::Percent(-50.), Val::Percent(-50.))
        );
        const { assert!(BRACKET_MIN >= 44., "as large as a cell of a grid") };
        // The line is the role's border: a role with none would draw no
        // bracket.
        let path = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../assets/themes/veyra.theme.ron"
        );
        let theme = Theme::from_ron(&std::fs::read_to_string(path).expect("the theme"))
            .expect("the theme parses");
        let paint = Paint::for_role(&theme, &Role::new_static(BRACKET_ROLE)).expect("the role");
        assert!(paint.border.is_some(), "the bracket's line is its border");
    }

    /// A pistol's frame is about as tall as it is wide. Held to a height,
    /// its well is that height across too, give or take its shape, and not
    /// its parent's whole width with the pistol small in the middle; a
    /// carbine's well may be nearly three times as wide as it is tall.
    #[test]
    fn a_short_weapon_narrows_the_well() {
        let (pistol_wide, pistol_tall) = well_limits(975. / 1024., Some(30.));
        assert!((pistol_tall - 30.).abs() < f32::EPSILON);
        assert!((pistol_wide - 30. * 975. / 1024.).abs() < 1e-4);
        assert!(pistol_wide < pistol_tall);
        let (carbine_wide, carbine_tall) = well_limits(1024. / 398., Some(30.));
        assert!((carbine_tall - 30.).abs() < f32::EPSILON);
        assert!(carbine_wide > 2.5 * carbine_tall);
        // No limit, or none that means anything, is the widget's own.
        assert_eq!(well_limits(2., None), (2. * MAX_HEIGHT_VH, MAX_HEIGHT_VH));
        assert_eq!(
            well_limits(2., Some(0.)),
            (2. * MAX_HEIGHT_VH, MAX_HEIGHT_VH)
        );
        assert_eq!(words("fore_end"), "Fore end");
        assert_eq!(words("patrol_carbine"), "Patrol carbine");
    }
}
