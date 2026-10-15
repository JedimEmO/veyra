//! A weapon turning on a stage of its own, drawn into a picture a screen
//! shows (ticket 0007, step 8, second half).
//!
//! The hand draws a weapon from its baked pieces in the world
//! (`combat_art`). A screen wants the same pieces where the player can look
//! at them: side on, lit evenly, turning. So there is a second camera, and
//! the whole of this module is keeping it apart from the first. The world
//! has only ever had one `Camera3d` and the crates below draw, light and
//! measure by that.
//!
//! - [`WeaponStage`] is the picture and what is asked to be on it. It
//!   exists only where there are baked models, and the picture is kept for
//!   the session.
//! - While something is asked for, and only then, the stage is in the
//!   world: a camera that draws into the picture before the world's camera
//!   draws, two lights, and a turntable the pieces stand on. With nothing
//!   asked for they are despawned, so a run that never opens the fitting
//!   screen has the one camera it always had.
//! - Everything on the stage is on [`STAGE_LAYER`], the only render layer
//!   but the default in use: the stage's camera draws nothing of the world,
//!   the world's camera draws nothing of the stage, and Bevy matches a
//!   light to a view by layer, so neither lights the other's. The camera
//!   opts out of the sky ([`strata_atmosphere::NoSky`]) and of the terrain
//!   ([`strata_terrain::OffWorld`]), which is how every system that picks
//!   "the camera" passes it over. It draws to an image, so Bevy's UI and
//!   its shadow level of detail never take it for the window's.
//! - The stage stands at `STAGE_AT` in render space, clear of the ground.
//!   Layers already keep the two apart; the distance only keeps the
//!   world's meshes out of the cascades Bevy fits round every camera,
//!   whether or not the sun's layer reaches it.
//! - The stage leaves the world before the app does (`leave_before_exit`),
//!   or the app never finishes leaving.
//!
//! What a model on show costs the world is the second camera's own work
//! and two things Bevy does for every camera: the sun's cascades are
//! fitted to it and their casters culled on the CPU, though no shadow is
//! drawn for it, and GPU spans are kept by name, not by camera, so the
//! statistics read the stage's passes with the world's while it is up (the
//! local fog pass read 0.005 ms against 0.08 without). Neither changes a
//! pixel of the world.
//!
//! The widget `strata:weapon_model` ([`ModelParams`]) is the picture in a
//! well of its shape, with the mount in focus named on a tag as the flat
//! diagram names it. A drag across it turns the model by hand.

use std::f32::consts::{FRAC_PI_2, TAU};

use ashlar_bevy::prelude::{AshlarProp, PropAsset};
use bevy::app::{App, AppExit, Last, PostUpdate, Startup, Update};
use bevy::asset::{AssetServer, Assets, Handle};
use bevy::camera::visibility::{RenderLayers, Visibility, VisibilitySystems};
use bevy::camera::{
    Camera, Camera3d, ClearColorConfig, PerspectiveProjection, Projection, RenderTarget,
};
use bevy::color::Color;
use bevy::core_pipeline::tonemapping::Tonemapping;
use bevy::ecs::prelude::*;
use bevy::image::Image;
use bevy::light::{AmbientLight, DirectionalLight};
use bevy::math::{DVec3, Quat, Vec3};
use bevy::picking::Pickable;
use bevy::picking::events::{Drag, Pointer};
use bevy::render::render_resource::TextureFormat;
use bevy::render::view::Msaa;
use bevy::time::Time;
use bevy::transform::components::Transform;
use bevy::ui::widget::{ImageNode, NodeImageMode};
use bevy::ui::{AlignItems, AlignSelf, BorderRadius, JustifyContent, Node, UiRect, Val};
use serde::Deserialize;
use slotted::registry::Value;
use slotted::theme::{Role, Themed};
use slotted::ui::def::{Tags, UiNodeDef, WidgetKind};
use slotted::ui::screen::{SpawnCtx, Widget, WidgetRegistry};
use slotted::ui::{SemanticLabel, SemanticRole, TestId, WidgetNode};

use super::weapon_diagram::{FRAME_ROLE, spawn_tag, well_limits};
use crate::weapon_model::{self, Pieces, Standing, Wanted, WeaponModels};

/// The widget kind.
pub const WIDGET: &str = "strata:weapon_model";
/// The `test_id` of the picture.
pub const MODEL_ID: &str = "fitting.model";

/// The render layer the stage is drawn on. Nothing else in the game uses a
/// layer, so everything else is on the default one, 0.
pub const STAGE_LAYER: usize = 7;
/// The picture's size in pixels: two to one, as the well is.
pub const PICTURE: (u32, u32) = (1024, 512);
/// The picture's shape, wide to high.
pub const ASPECT: f32 = 2.;
/// How fast the model turns when left alone, radians a second: once round
/// in twenty seconds.
pub const TURN_RATE: f32 = TAU / 20.;
/// How long after the last drag the slow turn starts again, seconds.
pub const RESUME_AFTER_S: f32 = 3.;
/// How far a drag turns the model, radians a logical pixel: half a turn in
/// some three hundred pixels.
pub const DRAG_TURN: f32 = 0.01;

/// The longest weapon the bake makes, metres: the marksman rifle with its
/// stock and the shrouded muzzle, 1.109 from butt to tip. The frame is
/// fitted to it, and a shorter weapon is enlarged to it up to
/// [`FILL_LIMIT`].
const LONGEST_M: f32 = 1.11;
/// The most a short weapon is enlarged to fill the frame.
const FILL_LIMIT: f32 = 2.4;
/// The share of the frame's width left clear at each end of the longest
/// weapon.
const MARGIN: f32 = 0.06;
/// The lens, radians top to bottom: narrow, so a weapon end on is not
/// swollen at the muzzle.
const LENS_FOV: f32 = 0.16;
/// Where the stage stands in render space: over the world, clear of every
/// peak, and near enough the origin that a float still places a part to a
/// tenth of a millimetre.
const STAGE_AT: Vec3 = Vec3::new(0., 2000., 0.);
/// The key light: from over the camera's left shoulder.
const KEY_LUX: f32 = 15000.;
/// The fill: from low on the right and behind, so the underside and the
/// far end are not black.
const FILL_LUX: f32 = 4500.;
/// The stage's ambient light, which stands in for the sky it has none of.
const AMBIENT: f32 = 550.;

/// The picture a screen shows a weapon in, and what is asked to be on it.
///
/// Present only where there are baked models, so a screen that finds it
/// can count on a picture and one that does not draws the flat diagram.
#[derive(Resource, Debug, Clone)]
pub struct WeaponStage {
    image: Handle<Image>,
    wanted: Option<Wanted>,
}

impl WeaponStage {
    /// A stage that draws into `image`, with nothing asked for.
    #[must_use]
    pub fn new(image: Handle<Image>) -> Self {
        Self {
            image,
            wanted: None,
        }
    }

    /// The picture, which is there whether or not anything is drawn in it.
    #[must_use]
    pub fn image(&self) -> &Handle<Image> {
        &self.image
    }

    /// What is asked for; none when no screen asks.
    #[must_use]
    pub fn wanted(&self) -> Option<&Wanted> {
        self.wanted.as_ref()
    }

    /// Asks for `wanted` to be on the stage, or with none releases it: the
    /// camera, the lights and the model leave the world.
    pub fn show(&mut self, wanted: Option<Wanted>) {
        if self.wanted != wanted {
            self.wanted = wanted;
        }
    }
}

/// Everything the stage puts in the world, so it can all be taken out.
#[derive(Component, Debug, Clone, Copy, Default)]
pub struct OnStage;

/// What the pieces stand on: it turns, and the model under it is moved so
/// its middle is on the axis.
#[derive(Component, Debug, Clone, Default)]
pub struct Turntable {
    /// How far it has turned from side on, radians.
    pub angle: f32,
    /// Seconds since the last drag.
    idle: f32,
    /// The pieces the model on show or on its way was stood from.
    wanted: Option<Wanted>,
    /// The model on show.
    shown: Option<Entity>,
    /// The model still loading, hidden until every piece of it has spawned.
    pending: Option<Entity>,
    /// The chassis on show and the scale it is drawn at, kept while the
    /// chassis is the same so a part looked at does not resize the weapon.
    scale: Option<(String, f32)>,
}

/// The scale a model `length` metres long is drawn at: the one its chassis
/// already has, unless the model has grown past the frame at it, and
/// otherwise the one that fills the frame, up to a limit so a pistol is not
/// blown up past what it is and down to whatever a polearm needs to fit. At one scale for every weapon the carbine read
/// as a small grey shape in a large well; at a scale a record, a longer
/// muzzle looked at made the whole weapon shrink.
fn scale_for(kept: Option<&(String, f32)>, chassis: &str, length: f32) -> f32 {
    let fills = (LONGEST_M / length.max(f32::EPSILON)).min(FILL_LIMIT);
    kept.filter(|(of, _)| of == chassis)
        .map_or(fills, |(_, scale)| scale.min(fills))
}

/// How far from the model's middle the camera stands, metres, for the
/// frame to be [`LONGEST_M`] and its margins wide.
fn camera_distance() -> f32 {
    let width = LONGEST_M * (1. + 2. * MARGIN);
    let half_height = width / ASPECT / 2.;
    half_height / (LENS_FOV / 2.).tan()
}

/// `Update`, until there is a stage: the picture, once the game has read
/// an index with something in it and there is somewhere to keep an image.
fn make_stage(
    mut commands: Commands,
    models: Res<WeaponModels>,
    images: Option<ResMut<Assets<Image>>>,
) {
    let Some(mut images) = images.filter(|_| !models.0.chassis.is_empty()) else {
        return;
    };
    // Cleared to nothing, so the well's own ground shows round the weapon.
    let image = images.add(Image::new_target_texture(
        PICTURE.0,
        PICTURE.1,
        TextureFormat::Rgba8UnormSrgb,
        None,
    ));
    commands.insert_resource(WeaponStage::new(image));
}

/// The camera and the two lights, and the entity the turntable is to be.
fn spawn_scene(commands: &mut Commands, image: Handle<Image>) -> Entity {
    let layer = RenderLayers::layer(STAGE_LAYER);
    commands.spawn((
        Name::new("weapon stage camera"),
        OnStage,
        Camera3d::default(),
        Camera {
            // Before the world's camera, so the picture a screen shows this
            // frame is this frame's.
            order: -1,
            clear_color: ClearColorConfig::Custom(Color::NONE),
            ..Camera::default()
        },
        RenderTarget::Image(image.into()),
        Projection::Perspective(PerspectiveProjection {
            fov: LENS_FOV,
            near: 0.1,
            far: 20.,
            ..PerspectiveProjection::default()
        }),
        Msaa::Sample4,
        // The atmosphere's choice for the world, and one that needs no
        // lookup table to be compiled in.
        Tonemapping::AcesFitted,
        AmbientLight {
            brightness: AMBIENT,
            ..AmbientLight::default()
        },
        layer.clone(),
        strata_atmosphere::NoSky,
        strata_terrain::OffWorld,
        Transform::from_translation(STAGE_AT + Vec3::Z * camera_distance())
            .looking_at(STAGE_AT, Vec3::Y),
    ));
    for (name, lux, from) in [
        ("weapon stage key", KEY_LUX, Vec3::new(-1.2, 1.6, 2.2)),
        ("weapon stage fill", FILL_LUX, Vec3::new(1.6, -0.5, -0.6)),
    ] {
        commands.spawn((
            Name::new(name),
            OnStage,
            DirectionalLight {
                illuminance: lux,
                shadow_maps_enabled: false,
                ..DirectionalLight::default()
            },
            layer.clone(),
            Transform::from_translation(from).looking_at(Vec3::ZERO, Vec3::Y),
        ));
    }
    commands
        .spawn((
            Name::new("weapon stage turntable"),
            OnStage,
            layer,
            Transform::from_translation(STAGE_AT).with_rotation(side_on(0.)),
            Visibility::Inherited,
        ))
        .id()
}

/// The turntable's rotation `angle` round from side on. In the recipe's
/// frame the muzzle is along +z; a quarter turn lays it to the camera's
/// right, as the flat diagram draws it.
fn side_on(angle: f32) -> Quat {
    Quat::from_rotation_y(FRAC_PI_2 + angle)
}

/// Stands `wanted`'s pieces hidden on `table` beside the model on show,
/// in place of any still loading. [`settle_stage`] swaps them in, so the
/// weapon never flickers to nothing and the turn goes on. Without an asset
/// server (a headless app) nothing can be loaded and the turntable stays
/// as it is.
fn restand(
    commands: &mut Commands,
    server: Option<&AssetServer>,
    models: &WeaponModels,
    table: Entity,
    turntable: &mut Turntable,
    wanted: &Wanted,
) {
    let Some(server) = server else {
        return;
    };
    if let Some(stale) = turntable.pending.take() {
        commands.entity(stale).despawn();
    }
    turntable.pending = Some(weapon_model::stand(
        commands,
        server,
        &models.0,
        wanted,
        (
            Name::new("weapon stage model"),
            Transform::IDENTITY,
            Visibility::Hidden,
            ChildOf(table),
        ),
    ));
    turntable.wanted = Some(wanted.clone());
}

/// `Update`, after the screens have said what they show: the stage is in
/// the world exactly while something is asked for, and its turntable
/// carries the pieces asked for.
fn keep_stage(
    mut commands: Commands,
    stage: Res<WeaponStage>,
    models: Res<WeaponModels>,
    server: Option<Res<AssetServer>>,
    scene: Query<Entity, With<OnStage>>,
    mut tables: Query<(Entity, &mut Turntable)>,
) {
    let Some(wanted) = stage.wanted() else {
        for entity in &scene {
            commands.entity(entity).despawn();
        }
        return;
    };
    let server = server.as_deref();
    if let Some((table, mut turntable)) = tables.iter_mut().next() {
        if turntable.wanted.as_ref() != Some(wanted) {
            restand(
                &mut commands,
                server,
                &models,
                table,
                &mut turntable,
                wanted,
            );
        }
        return;
    }
    let table = spawn_scene(&mut commands, stage.image().clone());
    // Turning from the first frame: nobody has dragged it yet.
    let mut turntable = Turntable {
        idle: RESUME_AFTER_S,
        ..Turntable::default()
    };
    restand(
        &mut commands,
        server,
        &models,
        table,
        &mut turntable,
        wanted,
    );
    commands.entity(table).insert(turntable);
}

/// The middle of the box round every piece of `model`, in the recipe's
/// frame, and its length along the muzzle's axis, from the bounds each
/// baked prop carries.
fn middle(
    model: Entity,
    children: &Query<&Children>,
    pieces: &Query<&AshlarProp>,
    props: &Assets<PropAsset>,
) -> (Vec3, f32) {
    let boxes = children
        .get(model)
        .into_iter()
        .flatten()
        .filter_map(|piece| pieces.get(*piece).ok())
        .filter_map(|piece| props.get(&piece.prop))
        .map(|asset| asset.prop.bounds);
    let Some([low, high]) = boxes.reduce(|[low, high], [min, max]| [low.min(min), high.max(max)])
    else {
        return (Vec3::ZERO, LONGEST_M);
    };
    let size = (high - low).as_vec3();
    (DVec3::midpoint(low, high).as_vec3(), size.z)
}

/// `Update`: a model whose every piece has spawned is centred on the
/// turntable's axis and takes the place of the one on show; one with a
/// piece that failed is thrown away and the one on show stays.
fn settle_stage(
    mut commands: Commands,
    mut tables: Query<&mut Turntable>,
    standing: Pieces,
    children: Query<&Children>,
    pieces: Query<&AshlarProp>,
    props: Option<Res<Assets<PropAsset>>>,
) {
    for mut turntable in &mut tables {
        let Some(pending) = turntable.pending else {
            continue;
        };
        match standing.standing(pending) {
            Standing::Loading => {}
            Standing::Failed => {
                tracing::warn!("a weapon model piece failed to load; the stage keeps what it had");
                commands.entity(pending).despawn();
                turntable.pending = None;
            }
            Standing::Ready => {
                let (centre, length) = props.as_deref().map_or((Vec3::ZERO, LONGEST_M), |props| {
                    middle(pending, &children, &pieces, props)
                });
                let chassis = turntable
                    .wanted
                    .as_ref()
                    .map(|wanted| wanted.chassis.clone())
                    .unwrap_or_default();
                let scale = scale_for(turntable.scale.as_ref(), &chassis, length);
                turntable.scale = Some((chassis, scale));
                commands.entity(pending).insert((
                    Transform::from_translation(-centre * scale).with_scale(Vec3::splat(scale)),
                    Visibility::Inherited,
                ));
                if let Some(old) = turntable.shown.replace(pending) {
                    commands.entity(old).despawn();
                }
                turntable.pending = None;
            }
        }
    }
}

/// `Update`: the slow turn, which waits [`RESUME_AFTER_S`] after a drag.
fn turn_stage(time: Res<Time>, mut tables: Query<(&mut Turntable, &mut Transform)>) {
    for (mut turntable, mut transform) in &mut tables {
        turntable.idle += time.delta_secs();
        // Side on until a model is seen, so the first frame of it is.
        if turntable.idle >= RESUME_AFTER_S && turntable.shown.is_some() {
            turntable.angle = (turntable.angle + TURN_RATE * time.delta_secs()) % TAU;
        }
        transform.rotation = side_on(turntable.angle);
    }
}

/// `PostUpdate`, before Bevy decides who sees what: every entity under the
/// turntable is on the stage's layer, the meshes a prop spawned this frame
/// among them, so none is ever drawn by the world's camera. Walks the
/// stage's own few entities and nothing else.
fn carry_layer(
    mut commands: Commands,
    tables: Query<Entity, With<Turntable>>,
    children: Query<&Children>,
    layered: Query<(), With<RenderLayers>>,
) {
    for table in &tables {
        for entity in children.iter_descendants(table) {
            if !layered.contains(entity) {
                commands
                    .entity(entity)
                    .insert(RenderLayers::layer(STAGE_LAYER));
            }
        }
    }
}

/// `Last`, on the frame the app is asked to exit: the stage leaves first,
/// so the last frame drawn has the one camera a last frame always had.
///
/// With the stage's camera still in the world the game did not exit: two
/// runs of two hung, and five of five were clean with this. The stacks of
/// a hung run: the main thread in the drop of Bevy's render channels,
/// waiting for the render thread to hand its world back, and the render
/// thread inside a camera's schedule, waiting on a system with every pool
/// thread idle, which is how a system bound to the main thread looks (the
/// atmosphere's GPU spans are such). Why the window's camera alone gets
/// through its last frame is not known, and the stage need not find out.
fn leave_before_exit(mut commands: Commands, scene: Query<Entity, With<OnStage>>) {
    for entity in &scene {
        commands.entity(entity).despawn();
    }
}

/// Observer: a pointer dragged across the picture turns the model by hand,
/// and the slow turn waits.
fn on_drag(
    drag: On<Pointer<Drag>>,
    pictures: Query<(), With<ModelPicture>>,
    mut tables: Query<&mut Turntable>,
) {
    if !pictures.contains(drag.entity) {
        return;
    }
    for mut turntable in &mut tables {
        turntable.drag_by(drag.delta.x);
    }
}

impl Turntable {
    /// A hand on the picture: turned by the pixels dragged, and the slow
    /// turn waits.
    fn drag_by(&mut self, pixels: f32) {
        self.angle = (self.angle + pixels * DRAG_TURN) % TAU;
        self.idle = 0.;
    }
}

/// The node that shows the stage's picture.
#[derive(Component, Debug, Clone, Copy, Default)]
pub struct ModelPicture;

/// The widget's parameters.
#[derive(Debug, Clone, PartialEq, Default, Deserialize)]
pub struct ModelParams {
    /// The name of the mount in focus, as display text, for the tag in the
    /// picture's corner; none draws no tag.
    #[serde(default)]
    pub tag: Option<String>,
    /// The tallest the picture grows, in percent of the window's height,
    /// as the diagram's.
    #[serde(default)]
    pub max_height: Option<f32>,
    /// Whether the well is always as tall as the limit, as the diagram's.
    #[serde(default)]
    pub fixed: bool,
}

impl ModelParams {
    /// The parameters as the untyped value a `custom` node carries.
    #[must_use]
    pub fn to_value(&self) -> Value {
        let text = |s: &str| Value::String(s.to_owned());
        let mut fields = Vec::new();
        if let Some(tag) = &self.tag {
            fields.push((text("tag"), text(tag)));
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

    /// The `custom` node that draws these parameters.
    #[must_use]
    pub fn node(&self) -> UiNodeDef {
        UiNodeDef::Custom {
            kind: WidgetKind::new(WIDGET),
            params: self.to_value(),
            children: Vec::new(),
            tags: Tags::new(),
        }
    }
}

/// `strata:weapon_model`.
#[derive(Debug, Default, Clone, Copy)]
pub struct ModelWidget;

impl Widget for ModelWidget {
    /// Two boxes, as the pictured diagram: the well the screen lays out,
    /// and inside it the picture, which keeps its shape whatever the well
    /// is given. Without a stage the well is empty; a screen asks first.
    fn spawn(&self, ctx: &mut SpawnCtx<'_>, params: &Value, _children: &[UiNodeDef]) -> Entity {
        let params = ModelParams::from_value(params).unwrap_or_else(|error| {
            tracing::warn!(%error, "bad params for {WIDGET}; drawing no tag");
            ModelParams::default()
        });
        let radius = ctx.tokens().radii.sm;
        let (widest, tallest) = well_limits(ASPECT, params.max_height);
        let root = ctx.spawn_node((
            Node {
                width: Val::Percent(100.),
                min_height: Val::Px(0.),
                max_height: Val::Vh(tallest),
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
            SemanticLabel(params.tag.clone().unwrap_or_default()),
            WidgetNode(WidgetKind::new(WIDGET)),
        ));
        let image = ctx
            .world
            .get_resource::<WeaponStage>()
            .map(|stage| stage.image().clone());
        let mut picture = ctx.world.spawn((
            Node {
                width: Val::Percent(100.),
                // The picture is held to the frame's shape and the well is
                // as wide as its column, as the diagram's is, so a switch a
                // screen puts in the well's corner is in it in both views.
                max_width: Val::Vh(widest),
                max_height: Val::Percent(100.),
                aspect_ratio: Some(ASPECT),
                flex_shrink: 0.,
                ..Node::default()
            },
            ModelPicture,
            SemanticRole::Decor,
            TestId::new(MODEL_ID),
            ChildOf(root),
        ));
        match image {
            Some(image) => {
                picture.insert((
                    ImageNode {
                        image_mode: NodeImageMode::Stretch,
                        ..ImageNode::new(image)
                    },
                    Pickable::default(),
                ));
            }
            None => {
                picture.insert(Pickable::IGNORE);
            }
        }
        if let Some(tag) = &params.tag {
            spawn_tag(ctx, root, tag);
        }
        root
    }
}

/// `Startup`: the widget, under its kind.
fn register_widget(mut widgets: ResMut<WidgetRegistry>) {
    widgets.register(WidgetKind::new(WIDGET), ModelWidget);
}

/// Registers the stage and its widget. Only called once the UI is wired.
/// The stage's entities are its own and no screen's, so its systems need
/// no place among the screens' work.
pub(crate) fn register(app: &mut App) {
    // The index is the game's to read; without one there is never a stage.
    app.init_resource::<WeaponModels>()
        .add_observer(on_drag)
        .add_systems(Startup, register_widget)
        .add_systems(
            Update,
            (
                make_stage.run_if(not(resource_exists::<WeaponStage>)),
                // After the screens, so what one asks for on the frame it
                // opens is on the stage that frame, and the picture is
                // never a frame of what the last screen left in it.
                (keep_stage, settle_stage, turn_stage)
                    .chain()
                    .after(super::ScreenWork)
                    .run_if(resource_exists::<WeaponStage>),
            ),
        )
        .add_systems(
            PostUpdate,
            carry_layer.before(VisibilitySystems::CheckVisibility),
        )
        .add_systems(Last, leave_before_exit.run_if(on_message::<AppExit>));
}

#[cfg(test)]
mod tests {
    use super::*;
    use bevy::MinimalPlugins;

    fn carbine() -> Wanted {
        Wanted {
            chassis: "patrol_carbine".to_owned(),
            files: vec!["weapons/veyra/patrol_carbine/chassis.ashprop".to_owned()],
            tip: [0., 0.071, 0.49],
        }
    }

    /// What the stage has in the world: cameras, lights, turntables.
    fn on_stage(app: &mut App) -> (usize, usize, usize) {
        let world = app.world_mut();
        let cameras = world
            .query_filtered::<(), (With<OnStage>, With<Camera3d>)>()
            .iter(world)
            .count();
        let lights = world
            .query_filtered::<(), (With<OnStage>, With<DirectionalLight>)>()
            .iter(world)
            .count();
        let tables = world.query::<&Turntable>().iter(world).count();
        (cameras, lights, tables)
    }

    #[test]
    fn a_chassis_keeps_its_scale_until_a_model_outgrows_the_frame() {
        let first = scale_for(None, "patrol_carbine", 0.74);
        assert!((first - 1.5).abs() < 1e-3, "{first}");
        let kept = Some(("patrol_carbine".to_owned(), first));
        // A shorter muzzle looked at leaves the weapon the size it was.
        assert!((scale_for(kept.as_ref(), "patrol_carbine", 0.70) - first).abs() < 1e-6);
        // A longer one that no longer fits brings it in.
        let longer = scale_for(kept.as_ref(), "patrol_carbine", 0.90);
        assert!(longer < first && (longer - LONGEST_M / 0.90).abs() < 1e-6);
        // Another chassis starts again, and a pistol stops at the limit.
        let pistol = scale_for(kept.as_ref(), "service_pistol", 0.2);
        assert!((pistol - FILL_LIMIT).abs() < 1e-6);
    }

    /// The pointer's observer only forwards `delta.x` to this; that the
    /// picture is hit under the switch and the tag is a real pointer's to show.
    #[test]
    fn a_drag_turns_the_table_and_the_slow_turn_waits() {
        let mut table = Turntable {
            idle: RESUME_AFTER_S,
            shown: Some(Entity::PLACEHOLDER),
            ..Turntable::default()
        };
        table.drag_by(100.);
        assert!((table.angle - 100. * DRAG_TURN).abs() < 1e-6);
        assert!(table.idle < RESUME_AFTER_S, "the slow turn waits");
    }

    /// No renderer and no asset server: the scene's entities are plain
    /// entities, which is all this holds. That the camera draws the model
    /// and nothing else draws it is a still's to show.
    #[test]
    fn the_stage_is_in_the_world_only_while_asked_for() {
        let mut app = App::new();
        app.add_plugins(MinimalPlugins)
            .init_resource::<WeaponModels>()
            .insert_resource(WeaponStage::new(Handle::default()))
            .add_systems(Update, (keep_stage, settle_stage, turn_stage).chain())
            .add_systems(PostUpdate, carry_layer);
        app.update();
        assert_eq!(on_stage(&mut app), (0, 0, 0), "nothing asked for");
        assert_eq!(
            app.world_mut()
                .query::<&Camera3d>()
                .iter(app.world())
                .count(),
            0,
            "a run that never asks has no camera of the stage's"
        );

        app.world_mut()
            .resource_mut::<WeaponStage>()
            .show(Some(carbine()));
        app.update();
        app.update();
        assert_eq!(on_stage(&mut app), (1, 2, 1), "asked for, and not twice");
        // The camera is apart from the world's on every count the crates
        // below tell cameras by, and so are the lights.
        let world = app.world_mut();
        let layer = RenderLayers::layer(STAGE_LAYER);
        let (layers, target, camera) = world
            .query_filtered::<(&RenderLayers, &RenderTarget, &Camera), (
                With<Camera3d>,
                With<strata_atmosphere::NoSky>,
                With<strata_terrain::OffWorld>,
            )>()
            .single(world)
            .expect("the stage's camera, opted out of the sky and the terrain");
        assert_eq!(layers, &layer);
        assert!(!layers.intersects(&RenderLayers::default()));
        assert!(target.as_image().is_some(), "it draws to the picture");
        assert!(camera.order < 0, "before the world's camera");
        assert!(
            world
                .query::<(&DirectionalLight, &RenderLayers)>()
                .iter(world)
                .all(|(light, layers)| layers == &layer && !light.shadow_maps_enabled)
        );
        // Whatever comes to stand on the turntable is carried to its layer
        // before anything is drawn.
        let table = world
            .query_filtered::<Entity, With<Turntable>>()
            .single(world)
            .expect("the turntable");
        let piece = world.spawn(ChildOf(table)).id();
        let mesh = world.spawn(ChildOf(piece)).id();
        app.update();
        assert_eq!(app.world().get::<RenderLayers>(mesh), Some(&layer));

        app.world_mut().resource_mut::<WeaponStage>().show(None);
        app.update();
        assert_eq!(on_stage(&mut app), (0, 0, 0), "released");
        assert!(
            app.world().get_entity(mesh).is_err(),
            "and its model with it"
        );
    }
}
