//! A generated skinned body follows the simulation, with explicit animation clocks.

use bevy::animation::AnimationTargetId;
use bevy::app::AnimationSystems;
use bevy::prelude::*;
use bevy::world_serialization::WorldInstanceReady;
use strata_physics::CharacterController;
use strata_sim::combat::{Action, Combatant};
use strata_sim::ride::Riding;
use strata_sim::{LocalPlayer, PreviousWorldPosition, RenderOrigin, WorldPosition};

use crate::weapon_model::{self, Wanted, WeaponModels};

const BODY: &str = "characters/combat/frontier_ranger.glb";
// This order is also the explicit bundle order in the generation recipe.
const CLIP_COUNT: usize = 8;

#[derive(Component)]
struct Visual {
    actor: Entity,
    half_height: f32,
}

#[derive(Component)]
struct Animated {
    actor: Entity,
    nodes: Vec<AnimationNodeIndex>,
    elapsed: f32,
    shot: u64,
    shot_age: f32,
    freeze: f32,
    action: Action,
    blend_from: Option<(AnimationNodeIndex, f32)>,
}

/// A held piece: the blade, or one of the two ranged weapons.
#[derive(Component)]
struct Weapon {
    actor: Entity,
    /// `None` is the blade.
    kind: Option<strata_sim::combat::Weapon>,
}

/// What a gun's root has drawn from the baked pieces: the pieces it was
/// built for and the entity that holds them, so a fit changes the weapon
/// and wear, which changes the hands too, does not.
#[derive(Component, Default)]
struct Dressed {
    wanted: Option<Wanted>,
    /// The model on show, whose pieces have all spawned.
    model: Option<Entity>,
    /// A model still loading, kept beside the one on show so a refit never
    /// leaves the hand empty and a failed load never hides the old art.
    pending: Option<Entity>,
    /// Where the muzzle goes once the pending model stands.
    tip: Vec3,
}

/// Marks a gun's baked model so the body tint leaves it in its maker's
/// colours however the load order falls.
#[derive(Component)]
struct WeaponModel;

/// Where the old art's barrel ends, for a weapon with no baked model.
fn old_tip(kind: strata_sim::combat::Weapon) -> Vec3 {
    match kind {
        strata_sim::combat::Weapon::Carbine | strata_sim::combat::Weapon::Rifle => {
            Vec3::new(0., 0.57, 0.)
        }
        strata_sim::combat::Weapon::Pistol => Vec3::new(0., 0.3, -0.012),
        strata_sim::combat::Weapon::Blade
        | strata_sim::combat::Weapon::Greatblade
        | strata_sim::combat::Weapon::Glaive => Vec3::ZERO,
    }
}

/// The tip of a ranged weapon's barrel, where the view starts a bolt. The
/// sim fires from a fixed gameplay muzzle; the view bends the first metres
/// of the bolt from here onto that path.
#[derive(Component)]
pub(crate) struct Muzzle {
    pub(crate) actor: Entity,
    pub(crate) kind: strata_sim::combat::Weapon,
}

/// The weapon the view shows in hand: the sim's, except that a swap holsters
/// the old one for its first part and draws the new one for the rest.
fn in_hand_shown(actor: &Combatant) -> Option<strata_sim::combat::Weapon> {
    match actor.action {
        Action::Dead => None,
        Action::Swap if actor.age < 0.2 => Some(actor.weapon),
        Action::Swap => actor.holstered(),
        _ => Some(actor.weapon),
    }
}

/// The gun the view shows in hand, if the weapon in hand is one.
pub(crate) fn shown_weapon(actor: &Combatant) -> Option<strata_sim::combat::Weapon> {
    in_hand_shown(actor).filter(|weapon| !weapon.is_melee())
}

/// Where the blade's grip sits along the hand bone's length, metres.
const BLADE_GRIP: f32 = 0.05;
/// The one blade shape's length at reach one, metres.
const BLADE_LENGTH: f32 = 0.78;
/// Its offset across the hand bone.
const BLADE_Z: f32 = 0.005;

/// Whether the blade shows: a melee weapon in hand is drawn at rest and
/// through its cuts, and a gun in hand never draws it.
pub(crate) fn shown_blade(actor: &Combatant) -> bool {
    in_hand_shown(actor).is_some_and(strata_sim::combat::Weapon::is_melee)
}

/// Whether the thin shape shows: a melee weapon is in hand and no baked
/// model of it has settled. Kept when nothing is baked, so a clone that has
/// not run `just content` still holds something.
fn thin_shape_shown(actor: &Combatant, modelled: bool) -> bool {
    shown_blade(actor) && !modelled
}

/// The left arm leaves the carbine's fore-grip and hangs when the pistol is
/// in hand; the aim and fire clips are two-handed.
#[derive(Component)]
struct ArmBone {
    actor: Entity,
    upper: bool,
}

/// The right arm's two bones, which hold the blade: raised across the body
/// while the guard is up.
#[derive(Component)]
struct GuardArm {
    actor: Entity,
    upper: bool,
}

/// Whether the body shows a raised guard: exactly while the sim says it is
/// up, so the view never decides one.
pub(crate) fn guard_pose(actor: &Combatant) -> bool {
    actor.guard_up
}

/// Which of the household's colours an actor's body wears, as an index
/// into [`TINTS`]: a swap at a terminal changes it, and the body follows,
/// so whose hands the player has is visible without a HUD. On the actor,
/// not the visual; the game sets it with the sheet.
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct BodyTint(pub u8);

/// A colour of a body's own, in sRGB bytes, which wins over [`BodyTint`]: a
/// faction's people and a band's wear their holder's colour so a guard reads
/// apart from an outlaw and from the household's members.
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq)]
pub struct BodyColour(pub [u8; 3]);

/// The weathered steel of a derelict machine's security units, sRGB bytes.
/// The lift is normalised by its peak, so a pure grey lifts to the same dim
/// grey on the dark body; a little green keeps it a metal and not a faction.
pub const MACHINE_STEEL: [u8; 3] = [150, 185, 172];

/// Which look a body wants, as the key its materials are shared by: a
/// palette index below [`OWN_KEY`], a colour of its own above it.
const OWN_KEY: u32 = 0x0100_0000;

/// How much of an own colour is added on top of the multiply, so a dark body
/// still wears it.
const OWN_LIFT: f32 = 0.8;

/// The most metal an own-colour body keeps, so the colour shows.
const OWN_METALLIC: f32 = 0.3;

/// The share of the lift an own-colour body also gives off as light.
const OWN_GLOW: f32 = 1.0;

fn look_of(tint: Option<&BodyTint>, colour: Option<&BodyColour>) -> u32 {
    colour.map_or_else(
        || u32::from(tint.map_or(0, |t| t.0)),
        |c| OWN_KEY | u32::from(c.0[0]) << 16 | u32::from(c.0[1]) << 8 | u32::from(c.0[2]),
    )
}

fn colour_of(look: u32) -> [f32; 3] {
    if look >= OWN_KEY {
        let byte = |shift: u32| u8::try_from((look >> shift) & 0xff).unwrap_or(0);
        let linear = Color::srgb_u8(byte(16), byte(8), byte(0)).to_linear();
        [linear.red, linear.green, linear.blue]
    } else {
        TINTS[usize::try_from(look).unwrap_or(0) % TINTS.len()]
    }
}

/// The household's palette, multiplied into the body's base colour: the
/// arrival wears the body as authored, the others a cast of their own.
/// An index past the end wraps.
pub const TINTS: [[f32; 3]; 4] = [
    [1., 1., 1.],
    [1.0, 0.52, 0.42],
    [0.5, 0.92, 1.0],
    [1.0, 0.86, 0.45],
];

/// What a visual's materials were before a tint, so a second swap tints
/// the original and not the last tint.
#[derive(Component, Default)]
struct Tinted {
    tint: Option<u32>,
    originals: Vec<(Entity, Handle<StandardMaterial>)>,
}

/// One bone of the spine chain, with its share of the aim pitch and of the
/// hit-reaction spring, so a flinch runs through the back rather than
/// hinging at one joint.
#[derive(Component)]
struct SpineBone {
    actor: Entity,
    aim: f32,
    share: f32,
}

#[derive(Component)]
struct StrafeBone {
    actor: Entity,
    counter: bool,
}

#[derive(Resource)]
struct Art {
    clips: Vec<Handle<AnimationClip>>,
    cube: Handle<Mesh>,
    rifle: Handle<bevy::world_serialization::WorldAsset>,
    edge: Handle<StandardMaterial>,
    gunmetal: Handle<StandardMaterial>,
}

/// The procedural overlays on the animated pose (aim, strafe, reactions);
/// the leg IK solves after them.
#[derive(SystemSet, Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) struct PoseOverlays;

pub(super) struct CombatArtPlugin;

impl Plugin for CombatArtPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Startup, load)
            .add_systems(
                Update,
                (spawn, tint).chain().in_set(crate::ClientSet::Camera),
            )
            .init_resource::<WeaponModels>()
            .add_systems(Update, (dress_weapons, settle_models, hide_old_art).chain())
            .add_systems(Update, hide_scoped_body)
            .add_systems(
                PostUpdate,
                animate
                    .before(AnimationSystems)
                    .before(crate::combat_audio::FootstepAudio),
            )
            .add_systems(
                PostUpdate,
                (pose_upper_body, pose_guard, pose_strafe)
                    .in_set(PoseOverlays)
                    .after(AnimationSystems)
                    .before(bevy::transform::TransformSystems::Propagate),
            )
            .add_systems(PostUpdate, place.in_set(crate::ClientSet::Follow))
            .add_observer(bind);
        crate::combat_ik::install(app);
        crate::ride_art::install_art(app);
    }
}

/// `Update`: the bearer's body, and the weapon on it, are not drawn while
/// they look through a scope, since the camera sits at the sight inside
/// them. The visual's own visibility is the least that hides both. Only the
/// local player looks through the scope: another actor's aim leaves it drawn.
fn hide_scoped_body(
    actors: Query<&Combatant, With<LocalPlayer>>,
    mut visuals: Query<(&Visual, &mut Visibility)>,
) {
    for (visual, mut visibility) in &mut visuals {
        let wanted = if actors
            .get(visual.actor)
            .is_ok_and(|actor| crate::scope::scoped_zoom(actor).is_some())
        {
            Visibility::Hidden
        } else {
            Visibility::Inherited
        };
        if *visibility != wanted {
            *visibility = wanted;
        }
    }
}

fn load(
    mut commands: Commands,
    server: Res<AssetServer>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    commands.insert_resource(Art {
        clips: (0..CLIP_COUNT)
            .map(|i| server.load(GltfAssetLabel::Animation(i).from_asset(BODY)))
            .collect(),
        cube: meshes.add(Cuboid::new(1., 1., 1.)),
        rifle: server
            .load(GltfAssetLabel::Scene(0).from_asset("characters/combat/scrapyard_rifle.glb")),
        edge: materials.add(StandardMaterial {
            base_color: Color::srgb(0.2, 0.8, 0.85),
            emissive: LinearRgba::rgb(0.3, 2., 2.5),
            ..default()
        }),
        gunmetal: materials.add(StandardMaterial {
            base_color: Color::srgb(0.42, 0.44, 0.47),
            metallic: 0.7,
            perceptual_roughness: 0.4,
            ..default()
        }),
    });
}

fn spawn(
    mut commands: Commands,
    server: Res<AssetServer>,
    actors: Query<(Entity, &CharacterController), Added<Combatant>>,
) {
    for (entity, controller) in &actors {
        commands.spawn((
            Name::new("Combat character visual"),
            Visual {
                actor: entity,
                half_height: controller.half_height(),
            },
            Tinted::default(),
            WorldAssetRoot(server.load(GltfAssetLabel::Scene(0).from_asset(BODY))),
            Transform::default(),
        ));
    }
}

/// Colours a body by its actor's [`BodyTint`]: every material under the
/// visual is replaced by a copy with the palette colour multiplied into
/// its base colour, once the scene has loaded and again whenever the tint
/// changes. The copies are made once per original material and tint and
/// shared, so four bodies in two colours are two sets of materials.
#[allow(
    clippy::too_many_arguments,
    reason = "a Bevy system's parameters are its dependencies"
)]
fn tint(
    mut materials: ResMut<Assets<StandardMaterial>>,
    actors: Query<(Option<&BodyTint>, Option<&BodyColour>)>,
    children: Query<&Children>,
    parents: Query<&ChildOf>,
    models: Query<(), With<WeaponModel>>,
    mut visuals: Query<(Entity, &Visual, &mut Tinted)>,
    mut slots: Query<&mut MeshMaterial3d<StandardMaterial>>,
    mut copies: Local<
        std::collections::HashMap<(AssetId<StandardMaterial>, u32), Handle<StandardMaterial>>,
    >,
) {
    for (root, visual, mut tinted) in &mut visuals {
        let wanted = actors
            .get(visual.actor)
            .map_or(0, |(tint, colour)| look_of(tint, colour));
        if tinted.tint == Some(wanted) {
            continue;
        }
        if tinted.originals.is_empty() {
            tinted.originals = children
                .iter_descendants(root)
                .filter(|child| !parents.iter_ancestors(*child).any(|a| models.contains(a)))
                .filter_map(|child| slots.get(child).ok().map(|m| (child, m.0.clone())))
                .collect();
            if tinted.originals.is_empty() {
                // The scene has not loaded yet; try again next frame.
                continue;
            }
        }
        let colour = colour_of(wanted);
        // The lift is the colour at full brightness, so a dark band colour
        // still reads as its hue on a dark body.
        let peak = colour[0].max(colour[1]).max(colour[2]).max(f32::EPSILON);
        let lift = if wanted >= OWN_KEY {
            colour.map(|c| c / peak * OWN_LIFT)
        } else {
            [0.; 3]
        };
        for (child, original) in &tinted.originals {
            let Ok(mut slot) = slots.get_mut(*child) else {
                continue;
            };
            if wanted == 0 {
                slot.0 = original.clone();
                continue;
            }
            let key = (original.id(), wanted);
            let handle = if let Some(handle) = copies.get(&key) {
                handle.clone()
            } else {
                let Some(source) = materials.get(original) else {
                    continue;
                };
                let mut copy = source.clone();
                let base = copy.base_color.to_linear();
                copy.base_color = Color::linear_rgba(
                    base.red * colour[0] + lift[0],
                    base.green * colour[1] + lift[1],
                    base.blue * colour[2] + lift[2],
                    base.alpha,
                );
                if wanted >= OWN_KEY {
                    // A dark metal body takes none of the colour from a dim
                    // sky, so the colour is also a faint glow of its own.
                    copy.metallic = copy.metallic.min(OWN_METALLIC);
                    copy.emissive = LinearRgba::new(
                        lift[0] * OWN_GLOW,
                        lift[1] * OWN_GLOW,
                        lift[2] * OWN_GLOW,
                        1.0,
                    );
                }
                let handle = materials.add(copy);
                copies.insert(key, handle.clone());
                handle
            };
            slot.0 = handle;
        }
        tinted.tint = Some(wanted);
    }
}

#[allow(
    clippy::too_many_arguments,
    clippy::too_many_lines,
    reason = "scene binding inspects generated nodes and installs animation resources"
)]
fn bind(
    event: On<WorldInstanceReady>,
    mut commands: Commands,
    art: Res<Art>,
    visuals: Query<&Visual>,
    children: Query<&Children>,
    names: Query<(&Name, &AnimationTargetId)>,
    mut players: Query<&mut AnimationPlayer>,
    mut graphs: ResMut<Assets<AnimationGraph>>,
) {
    let Ok(visual) = visuals.get(event.entity) else {
        return;
    };
    let mut graph = AnimationGraph::new();
    for child in children.iter_descendants(event.entity) {
        if let Ok((name, id)) = names.get(child) {
            let lower = name.as_str() == "Hips"
                || ["UpLeg", "Leg", "Foot", "Toe"]
                    .iter()
                    .any(|s| name.contains(s));
            graph.add_target_to_mask_group(*id, u32::from(!lower));
        }
    }
    let root = graph.root;
    // Run (lower), idle (lower), aim/fire (upper), then the full-body actions.
    let mut nodes = vec![
        graph.add_clip_with_mask(art.clips[0].clone(), 2, 1., root),
        graph.add_clip_with_mask(art.clips[1].clone(), 2, 1., root),
        graph.add_clip_with_mask(art.clips[1].clone(), 1, 1., root),
        graph.add_clip_with_mask(art.clips[2].clone(), 1, 1., root),
    ];
    for clip in &art.clips[3..] {
        nodes.push(graph.add_clip(clip.clone(), 1., root));
    }
    let handle = graphs.add(graph);
    let mut leg_bones = std::collections::HashMap::new();
    for child in children.iter_descendants(event.entity) {
        if let Ok((name, _)) = names.get(child) {
            leg_bones.insert(name.to_string(), child);
        }
    }
    let leg = |side: &str| {
        Some(crate::combat_ik::Leg {
            thigh: *leg_bones.get(&format!("{side}UpLeg"))?,
            shin: *leg_bones.get(&format!("{side}Leg"))?,
            foot: *leg_bones.get(&format!("{side}Foot"))?,
        })
    };
    if let (Some(hips), Some(left), Some(right)) =
        (leg_bones.get("Hips"), leg("Left"), leg("Right"))
    {
        commands
            .entity(event.entity)
            .insert(crate::combat_ik::Legs {
                actor: visual.actor,
                root: event.entity,
                hips: *hips,
                sides: [left, right],
                lift: [0., 0.],
                drop: 0.,
            });
    }
    for child in children.iter_descendants(event.entity) {
        if let Ok(mut player) = players.get_mut(child) {
            for node in &nodes {
                player.play(*node).repeat().set_speed(0.).set_weight(0.);
            }
            commands.entity(child).insert((
                AnimationGraphHandle(handle.clone()),
                Animated {
                    actor: visual.actor,
                    nodes: nodes.clone(),
                    elapsed: 0.,
                    shot: 0,
                    shot_age: 1.,
                    freeze: 0.,
                    action: Action::Ready,
                    blend_from: None,
                },
            ));
        }
        if let Ok((name, _)) = names.get(child) {
            let chain = match name.as_str() {
                "Spine1" => Some((0.3, 0.3)),
                "Spine2" => Some((0.35, 0.35)),
                "Spine3" => Some((0.2, 0.2)),
                "Neck" => Some((0.15, 0.25)),
                // The head whips more than the chain under it carries.
                "Head" => Some((0., 0.35)),
                _ => None,
            };
            if let Some((aim, share)) = chain {
                commands.entity(child).insert(SpineBone {
                    actor: visual.actor,
                    aim,
                    share,
                });
            }
            if matches!(
                name.as_str(),
                "LeftArm" | "LeftForeArm" | "RightArm" | "RightForeArm"
            ) {
                commands.entity(child).insert(crate::ride_art::RiderArm {
                    actor: visual.actor,
                    upper: name.as_str().ends_with("Arm") && !name.as_str().ends_with("ForeArm"),
                    left: name.starts_with("Left"),
                });
            }
            if matches!(name.as_str(), "RightArm" | "RightForeArm") {
                commands.entity(child).insert(GuardArm {
                    actor: visual.actor,
                    upper: name.as_str() == "RightArm",
                });
            }
            if matches!(name.as_str(), "LeftArm" | "LeftForeArm") {
                commands.entity(child).insert(ArmBone {
                    actor: visual.actor,
                    upper: name.as_str() == "LeftArm",
                });
            }
        }
        if let Ok((name, _)) = names.get(child)
            && matches!(name.as_str(), "Hips" | "Spine")
        {
            commands.entity(child).insert(StrafeBone {
                actor: visual.actor,
                counter: name.as_str() == "Spine",
            });
        }
        if let Ok((name, _)) = names.get(child)
            && name.as_str() == "RightHand"
        {
            commands.entity(child).with_children(|parent| {
                parent.spawn((
                    Weapon {
                        actor: visual.actor,
                        kind: None,
                    },
                    Mesh3d(art.cube.clone()),
                    MeshMaterial3d(art.edge.clone()),
                    Transform::from_xyz(0., BLADE_GRIP + BLADE_LENGTH * 0.5, BLADE_Z)
                        .with_scale(Vec3::new(0.06, BLADE_LENGTH, 0.025)),
                    Visibility::Hidden,
                ));
                // One root a melee class: the baked model of what is fitted
                // stands under it at true size, where the thin shape stood.
                for kind in [
                    strata_sim::combat::Weapon::Blade,
                    strata_sim::combat::Weapon::Greatblade,
                    strata_sim::combat::Weapon::Glaive,
                ] {
                    parent.spawn((
                        Weapon {
                            actor: visual.actor,
                            kind: Some(kind),
                        },
                        Transform::default(),
                        Visibility::Hidden,
                    ));
                }
                // Derived from the exported aim pose: prop +Y becomes
                // character -Z. The rifle's magazine lies on +Z, so rotate
                // around its barrel to put the magazine underneath. The
                // pistol is built in the same frame.
                let mount = Transform::from_xyz(0., 0.055, 0.005)
                    .with_rotation(
                        Quat::from_xyzw(-0.243_855_4, -0.253_589_8, -0.131_152_3, 0.926_836_6)
                            * Quat::from_rotation_y(std::f32::consts::PI),
                    )
                    .with_scale(Vec3::splat(0.9));
                parent
                    .spawn((
                        Weapon {
                            actor: visual.actor,
                            kind: Some(strata_sim::combat::Weapon::Carbine),
                        },
                        WorldAssetRoot(art.rifle.clone()),
                        mount,
                    ))
                    .with_child((
                        Muzzle {
                            actor: visual.actor,
                            kind: strata_sim::combat::Weapon::Carbine,
                        },
                        Transform::from_xyz(0., 0.57, 0.),
                    ));
                // The marksman rifle stands in the carbine's frame and pose;
                // the old long-gun scene is its stand-in until a model loads.
                parent
                    .spawn((
                        Weapon {
                            actor: visual.actor,
                            kind: Some(strata_sim::combat::Weapon::Rifle),
                        },
                        WorldAssetRoot(art.rifle.clone()),
                        mount,
                        Visibility::Hidden,
                    ))
                    .with_child((
                        Muzzle {
                            actor: visual.actor,
                            kind: strata_sim::combat::Weapon::Rifle,
                        },
                        Transform::from_xyz(0., 0.57, 0.),
                    ));
                parent
                    .spawn((
                        Weapon {
                            actor: visual.actor,
                            kind: Some(strata_sim::combat::Weapon::Pistol),
                        },
                        mount,
                        Visibility::Hidden,
                    ))
                    .with_children(|pistol| {
                        // Receiver, barrel, grip and an emitter ring: a
                        // blocky sidearm in the rifle's frame (+Y barrel,
                        // +Z grip), authored here until a prop is lifted.
                        for (centre, size, edge) in [
                            (Vec3::new(0., 0.07, 0.), Vec3::new(0.05, 0.26, 0.07), false),
                            (
                                Vec3::new(0., 0.24, -0.012),
                                Vec3::new(0.034, 0.1, 0.034),
                                false,
                            ),
                            (
                                Vec3::new(0., -0.01, 0.09),
                                Vec3::new(0.04, 0.05, 0.13),
                                false,
                            ),
                            (
                                Vec3::new(0., 0.29, -0.012),
                                Vec3::new(0.042, 0.014, 0.042),
                                true,
                            ),
                            (
                                Vec3::new(0., 0.12, -0.04),
                                Vec3::new(0.02, 0.14, 0.01),
                                true,
                            ),
                            (Vec3::new(0.03, 0.1, 0.), Vec3::new(0.006, 0.12, 0.03), true),
                            (
                                Vec3::new(-0.03, 0.1, 0.),
                                Vec3::new(0.006, 0.12, 0.03),
                                true,
                            ),
                        ] {
                            pistol.spawn((
                                Mesh3d(art.cube.clone()),
                                MeshMaterial3d(if edge {
                                    art.edge.clone()
                                } else {
                                    art.gunmetal.clone()
                                }),
                                Transform::from_translation(centre).with_scale(size),
                            ));
                        }
                        pistol.spawn((
                            Muzzle {
                                actor: visual.actor,
                                kind: strata_sim::combat::Weapon::Pistol,
                            },
                            Transform::from_xyz(0., 0.3, -0.012),
                        ));
                    });
            });
        }
    }
    info!("combat art: skinned scene bound for {:?}", visual.actor);
}

#[allow(
    clippy::too_many_arguments,
    clippy::too_many_lines,
    reason = "confirmed contacts briefly hold local poses without altering simulation clocks"
)]
fn animate(
    time: Res<Time>,
    fixed: Res<Time<Fixed>>,
    clips: Res<Assets<AnimationClip>>,
    art: Res<Art>,
    actors: Query<(&Combatant, &CharacterController)>,
    mut players: Query<(&mut AnimationPlayer, &mut Animated)>,
    mut weapons: Query<(&Weapon, &mut Visibility, &mut Transform, Option<&Dressed>)>,
    riders: Query<(), With<Riding>>,
    mut impacts: MessageReader<strata_sim::combat::CombatImpact>,
    mut contacts: MessageWriter<crate::combat_audio::FootContact>,
) {
    for event in impacts.read() {
        if event.melee && event.victim.is_some() {
            for (_, mut animated) in &mut players {
                if animated.actor == event.attacker || Some(animated.actor) == event.victim {
                    animated.freeze = if actors
                        .get(event.attacker)
                        .is_ok_and(|(actor, _)| actor.action == Action::Heavy)
                    {
                        0.036
                    } else {
                        0.024
                    };
                }
            }
        }
    }
    for (mut player, mut animated) in &mut players {
        if animated.freeze > 0. {
            animated.freeze = (animated.freeze - time.delta_secs()).max(0.);
            continue;
        }
        let Ok((actor, controller)) = actors.get(animated.actor) else {
            continue;
        };
        if actor.action != animated.action {
            animated.blend_from =
                if animated.action == Action::Light && actor.action == Action::Followup {
                    player
                        .animation(animated.nodes[4])
                        .map(|clip| (animated.nodes[4], clip.seek_time()))
                } else {
                    None
                };
            animated.action = actor.action;
        }
        // A rider's body does not run with the bike under it: the idle pose
        // is held, lowered onto the saddle by `place`.
        let speed = if riders.contains(animated.actor) {
            0.
        } else {
            controller.horizontal_velocity.length()
        };
        let backwards = controller
            .horizontal_velocity
            .dot(strata_sim::PlayerIntent::forward(actor.facing))
            < -0.1;
        let previous_phase = animated.elapsed;
        animated.elapsed += time.delta_secs() * speed / 2.5584 * if backwards { -1. } else { 1. };
        animated.shot_age += time.delta_secs();
        if actor.shots != animated.shot {
            animated.shot = actor.shots;
            animated.shot_age = 0.;
        }
        let elapsed = (actor.age - fixed.delta_secs() * (1. - fixed.overstep_fraction())).max(0.);
        let active = match actor.action {
            Action::Light => Some((4, 3)),
            Action::Followup => Some((5, 4)),
            Action::Heavy => Some((6, 5)),
            Action::Dodge => Some((7, 6)),
            Action::Dead => Some((8, 7)),
            _ => None,
        };
        for node in &animated.nodes {
            if let Some(a) = player.animation_mut(*node) {
                a.set_weight(0.);
            }
        }
        let action_weight = if active.is_some() {
            if actor.action == Action::Dead {
                (elapsed / 0.06).clamp(0., 1.)
            } else {
                let enter = if animated.blend_from.is_some() {
                    1.
                } else {
                    (elapsed / 0.035).clamp(0., 1.)
                };
                let exit = ((actor.spec(actor.action).duration - elapsed) / 0.08).clamp(0., 1.);
                let t = enter.min(exit);
                t * t * (3. - 2. * t)
            }
        } else {
            0.
        };
        let chain_blend = if animated.blend_from.is_some() {
            (1. - elapsed / 0.035).clamp(0., 1.)
        } else {
            0.
        };
        if let Some((node, age)) = animated.blend_from {
            player
                .play(node)
                .set_weight(chain_blend)
                .set_speed(0.)
                .set_seek_time(age);
        }
        if let Some((node, clip)) = active {
            let duration = clips
                .get(&art.clips[clip])
                .map_or(1., AnimationClip::duration);
            let age = if actor.action == Action::Dead {
                elapsed.min(duration)
            } else {
                actor.clip_place(actor.action, elapsed) * duration
            };
            player
                .play(animated.nodes[node])
                .set_weight(action_weight * (1. - chain_blend))
                .set_speed(0.)
                .set_seek_time(age.min(duration - 0.001));
        }
        {
            let base_weight = 1. - action_weight;
            let moving = (speed / 0.8).clamp(0., 1.);
            let run_duration = clips
                .get(&art.clips[0])
                .map_or(1., AnimationClip::duration)
                .max(0.01);
            if controller.grounded && speed > 0.2 && base_weight > 0.5 {
                for _ in 0..crate::combat_audio::foot_contacts(
                    previous_phase,
                    animated.elapsed,
                    run_duration,
                ) {
                    contacts.write(crate::combat_audio::FootContact(animated.actor));
                }
            }
            player
                .play(animated.nodes[0])
                .set_weight(moving * base_weight)
                .set_speed(0.)
                .set_seek_time(animated.elapsed.rem_euclid(run_duration));
            player
                .play(animated.nodes[1])
                .set_weight((1. - moving) * base_weight)
                .set_speed(0.)
                .set_seek_time(0.);
            let firing = 0.35 * (1. - animated.shot_age / 0.12).clamp(0., 1.);
            player
                .play(animated.nodes[2])
                .set_weight((1. - firing) * base_weight)
                .set_speed(0.)
                .set_seek_time(0.);
            player
                .play(animated.nodes[3])
                .set_weight(firing * base_weight)
                .set_speed(0.)
                .set_seek_time(animated.shot_age.min(0.119));
        }
    }
    // The melee roots whose baked model has settled: the thin shape gives
    // way to them and comes back when none stands.
    let modelled: Vec<_> = weapons
        .iter()
        .filter(|(_, _, _, dressed)| dressed.is_some_and(|dressed| dressed.model.is_some()))
        .map(|(weapon, ..)| (weapon.actor, weapon.kind))
        .collect();
    for (weapon, mut visibility, mut transform, _) in &mut weapons {
        if let Ok((actor, _)) = actors.get(weapon.actor) {
            let shown = !riders.contains(weapon.actor)
                && match weapon.kind {
                    None => thin_shape_shown(
                        actor,
                        in_hand_shown(actor)
                            .is_some_and(|kind| modelled.contains(&(weapon.actor, Some(kind)))),
                    ),
                    Some(kind) => in_hand_shown(actor) == Some(kind),
                };
            if weapon.kind.is_none() {
                // One shape for every class, stretched along its length by
                // the class's reach, from the hand outward.
                let reach = in_hand_shown(actor).map_or(1., Combatant::reach_of);
                *transform =
                    Transform::from_xyz(0., BLADE_GRIP + BLADE_LENGTH * 0.5 * reach, BLADE_Z)
                        .with_scale(Vec3::new(0.06, BLADE_LENGTH * reach, 0.025));
            }
            *visibility = if shown {
                Visibility::Inherited
            } else {
                Visibility::Hidden
            };
        }
    }
}

fn pose_upper_body(
    actors: Query<(
        &Combatant,
        &strata_sim::combat::CombatIntent,
        &WorldPosition,
        Option<&crate::combat_fx::HitReaction>,
    )>,
    mut bones: Query<(&SpineBone, &mut Transform)>,
    mut arms: Query<(&ArmBone, &mut Transform), Without<SpineBone>>,
) {
    for (bone, mut transform) in &mut bones {
        let Ok((actor, input, position, reaction)) = actors.get(bone.actor) else {
            continue;
        };
        let tilt = match actor.action {
            Action::Ready if (input.aim || input.fire) && !actor.weapon.is_melee() => {
                let delta = input.aim_point.0 - position.0.0 - bevy::math::DVec3::Y * 0.45;
                #[allow(
                    clippy::cast_possible_truncation,
                    reason = "local clamped aiming angle"
                )]
                let pitch = delta.y.atan2(delta.x.hypot(delta.z)) as f32;
                pitch.clamp(-0.8, 0.8) * 0.65
            }
            Action::Reload => {
                -0.25
                    * (std::f32::consts::PI * (actor.age / actor.reload_time()).clamp(0., 1.)).sin()
            }
            // The draw's own length: something fitted to the weapon coming
            // up changes it, and the lean ends when the draw does.
            Action::Swap => {
                -0.18 * (std::f32::consts::PI * (actor.age / actor.duration()).clamp(0., 1.)).sin()
            }
            Action::Stagger => {
                -0.35 * (std::f32::consts::PI * (actor.age / actor.duration()).clamp(0., 1.)).sin()
            }
            _ => 0.,
        };
        // The spring's offset is a pitch (lean back from a frontal hit) and a
        // roll (away from a side hit); each bone carries its share.
        let (pitch, roll) = reaction.map_or((0., 0.), |reaction| {
            (
                reaction.offset.x * bone.share,
                reaction.offset.y * bone.share,
            )
        });
        transform.rotation *=
            Quat::from_rotation_x(tilt * bone.aim + pitch) * Quat::from_rotation_z(roll);
    }
    for (bone, mut transform) in &mut arms {
        let Ok((actor, _, _, _)) = actors.get(bone.actor) else {
            continue;
        };
        if shown_weapon(actor) != Some(strata_sim::combat::Weapon::Pistol) {
            continue;
        }
        // Drop the supporting hand from the fore-grip: the upper arm swings
        // down and the forearm relaxes toward the hip.
        let (swing, bend) = if bone.upper {
            (-1.05, 0.25)
        } else {
            (0., -0.6)
        };
        transform.rotation *= Quat::from_rotation_z(swing) * Quat::from_rotation_x(bend);
    }
}

/// The blade held up across the body for as long as the guard is up: the
/// upper arm comes forward and the forearm folds, on top of whatever clip is
/// playing, and the bones are left alone when it drops.
fn pose_guard(
    actors: Query<&Combatant>,
    mut arms: Query<(&GuardArm, &mut Transform), Without<ArmBone>>,
) {
    for (bone, mut transform) in &mut arms {
        if !actors.get(bone.actor).is_ok_and(guard_pose) {
            continue;
        }
        // Found against the rig: the upper arm forward and in, the forearm
        // folded up and turned so the blade crosses the body, tip toward the
        // left shoulder.
        let raise = if bone.upper {
            Quat::from_euler(EulerRot::XYZ, -0.9, 0., -0.6)
        } else {
            Quat::from_euler(EulerRot::XYZ, 0., -1., -1.6)
        };
        transform.rotation *= raise;
    }
}

fn pose_strafe(
    actors: Query<(&Combatant, &CharacterController)>,
    riders: Query<(), With<Riding>>,
    mut bones: Query<(&StrafeBone, &mut Transform)>,
) {
    let hips: std::collections::HashMap<_, _> = bones
        .iter()
        .filter(|(bone, _)| !bone.counter)
        .map(|(bone, transform)| (bone.actor, transform.rotation))
        .collect();
    for (bone, mut transform) in &mut bones {
        let Ok((actor, controller)) = actors.get(bone.actor) else {
            continue;
        };
        if riders.contains(bone.actor)
            || (actor.action != Action::Ready && actor.action != Action::Reload)
        {
            continue;
        }
        let local = Quat::from_rotation_y(-actor.facing) * controller.horizontal_velocity;
        if local.length_squared() < 0.01 {
            continue;
        }
        // Reverse the cycle for backpedal, and turn the lower body at most 60°.
        // The first spine joint compensates so the weapon keeps aiming forward.
        let direction = if local.z > 0.1 { -local } else { local };
        let yaw = (-direction.x).atan2(-direction.z).clamp(-1.047, 1.047);
        let turn = Quat::from_rotation_y(yaw);
        let correction = if bone.counter {
            let Some(hip) = hips.get(&bone.actor) else {
                continue;
            };
            counter_hip_turn(*hip, turn)
        } else {
            turn
        };
        transform.rotation = correction * transform.rotation;
    }
}

// Spine is a child of the rotated hips. Convert the inverse turn into that
// bone's local basis; a raw local Y counter-turn would also roll the torso.
fn counter_hip_turn(hip: Quat, turn: Quat) -> Quat {
    hip.inverse() * turn.inverse() * hip
}

fn place(
    mut commands: Commands,
    fixed: Res<Time<Fixed>>,
    origin: Res<RenderOrigin>,
    time: Res<Time>,
    view: Res<crate::ride_art::RideView>,
    actors: Query<(
        &WorldPosition,
        &PreviousWorldPosition,
        &Combatant,
        Option<&Riding>,
    )>,
    mut visuals: Query<(Entity, &Visual, &mut Transform)>,
) {
    for (entity, visual, mut transform) in &mut visuals {
        let Ok((position, previous, actor, riding)) = actors.get(visual.actor) else {
            commands.entity(entity).despawn();
            continue;
        };
        transform.translation = origin
            .to_render(previous.interpolate(position.0, fixed.overstep_fraction_f64()))
            - Vec3::Y * visual.half_height;
        transform.rotation = Quat::from_rotation_y(actor.facing);
        if let Some(riding) = riding {
            // On the saddle, facing the way the bike points.
            transform.translation += Vec3::Y
                * crate::ride_art::rider_lift(&view, time.elapsed_secs())
                + strata_sim::PlayerIntent::forward(riding.heading) * view.seat.z;
            transform.rotation = Quat::from_rotation_y(riding.heading);
        }
    }
}

/// `Update`: keep each gun an actor with hands carries drawn from what is
/// fitted. A gun is rebuilt only when the pieces it wants differ from the
/// pieces it shows.
#[allow(
    clippy::too_many_arguments,
    reason = "a Bevy system's parameters are its dependencies"
)]
fn dress_weapons(
    mut commands: Commands,
    server: Res<AssetServer>,
    models: Res<WeaponModels>,
    registries: Option<Res<strata_sim::Registries>>,
    actors: Query<Ref<strata_sim::items::Hands>>,
    roots: Query<(Entity, &Weapon, Option<&Dressed>)>,
    mut muzzles: Query<(&Muzzle, &mut Transform)>,
) {
    let Some(registries) = registries else {
        return;
    };
    for (root, weapon, dressed) in &roots {
        let Some(kind) = weapon.kind else {
            continue;
        };
        let Ok(hands) = actors.get(weapon.actor) else {
            continue;
        };
        if dressed.is_some() && !hands.is_changed() && !models.is_changed() {
            continue;
        }
        let items = &registries.0.items;
        // The hand that holds this gun's family, whichever hand that is.
        let wanted = hands
            .saved(items)
            .unwrap_or_default()
            .into_iter()
            .flatten()
            .find(|record| strata_sim::fitting::weapon_of(items, record) == Some(kind))
            .and_then(|record| weapon_model::wanted(&models.0, items, &record));
        if dressed.is_some_and(|dressed| dressed.wanted == wanted) {
            continue;
        }
        let (mut model, old_pending) = dressed.map_or((None, None), |d| (d.model, d.pending));
        if let Some(stale) = old_pending {
            commands.entity(stale).despawn();
        }
        let frame = if kind.is_melee() {
            weapon_model::melee_frame()
        } else {
            weapon_model::frame()
        };
        let mut pending = None;
        let tip = if let Some(wanted) = &wanted {
            pending = Some(weapon_model::stand(
                &mut commands,
                &server,
                &models.0,
                wanted,
                (
                    Name::new("model"),
                    WeaponModel,
                    frame,
                    Visibility::Inherited,
                    ChildOf(root),
                ),
            ));
            frame.transform_point(Vec3::from(wanted.tip))
        } else {
            if let Some(old) = model.take() {
                commands.entity(old).despawn();
            }
            set_muzzle(&mut muzzles, weapon.actor, kind, old_tip(kind));
            old_tip(kind)
        };
        commands.entity(root).insert(Dressed {
            wanted,
            model,
            pending,
            tip,
        });
    }
}

fn set_muzzle(
    muzzles: &mut Query<(&Muzzle, &mut Transform)>,
    actor: Entity,
    kind: strata_sim::combat::Weapon,
    tip: Vec3,
) {
    for (muzzle, mut transform) in muzzles {
        if muzzle.actor == actor && muzzle.kind == kind {
            transform.translation = tip;
        }
    }
}

/// `Update`: a model that is still loading takes the place of the one on
/// show once every piece has spawned, and is thrown away with the old art
/// kept if any piece failed (a stale index, a partial `generated/`).
fn settle_models(
    mut commands: Commands,
    mut roots: Query<(&Weapon, &mut Dressed)>,
    pieces: weapon_model::Pieces,
    mut muzzles: Query<(&Muzzle, &mut Transform)>,
) {
    for (weapon, mut dressed) in &mut roots {
        let (Some(pending), Some(kind)) = (dressed.pending, weapon.kind) else {
            continue;
        };
        match pieces.standing(pending) {
            weapon_model::Standing::Loading => {}
            weapon_model::Standing::Failed => {
                commands.entity(pending).despawn();
                if let Some(old) = dressed.model.take() {
                    commands.entity(old).despawn();
                }
                dressed.pending = None;
                set_muzzle(&mut muzzles, weapon.actor, kind, old_tip(kind));
            }
            weapon_model::Standing::Ready => {
                if let Some(old) = dressed.model.replace(pending) {
                    commands.entity(old).despawn();
                }
                dressed.pending = None;
                let tip = dressed.tip;
                set_muzzle(&mut muzzles, weapon.actor, kind, tip);
            }
        }
    }
}

/// `Update`: the old art under a gun's root is hidden while a baked model
/// stands and shown again when none does. The model and the muzzle marker
/// are the root's own children and are left alone.
fn hide_old_art(
    mut commands: Commands,
    roots: Query<(&Dressed, &Children)>,
    old: Query<Option<&Visibility>, Without<Muzzle>>,
) {
    for (dressed, children) in &roots {
        let want = if dressed.model.is_some() {
            Visibility::Hidden
        } else {
            Visibility::Inherited
        };
        for child in children {
            if Some(*child) == dressed.model || Some(*child) == dressed.pending {
                continue;
            }
            if let Ok(current) = old.get(*child)
                && current != Some(&want)
            {
                commands.entity(*child).insert(want);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_body_is_hidden_while_scoped_and_shown_again_when_the_scope_is_left() {
        use strata_sim::PlayerIntent;
        use strata_sim::combat::{CombatIntent, CombatMotion, Weapon};
        let mut app = App::new();
        app.add_systems(Update, hide_scoped_body);
        let actor = app
            .world_mut()
            .spawn((
                Combatant::default().carrying([Some(Weapon::Rifle), None], 0),
                LocalPlayer,
            ))
            .id();
        // Another actor with a closed aim keeps its body drawn.
        let other = app
            .world_mut()
            .spawn(Combatant::default().carrying([Some(Weapon::Rifle), None], 0))
            .id();
        let other_body = app
            .world_mut()
            .spawn((
                Visual {
                    actor: other,
                    half_height: 0.9,
                },
                Visibility::Inherited,
            ))
            .id();
        let body = app
            .world_mut()
            .spawn((
                Visual {
                    actor,
                    half_height: 0.9,
                },
                Visibility::Inherited,
            ))
            .id();
        let shown = |app: &App| *app.world().get::<Visibility>(body).expect("body");
        let mut motion = CombatMotion::default();
        let mut run = |app: &mut App, ticks: u32, aim: bool| {
            for _ in 0..ticks {
                let mut input = CombatIntent {
                    aim,
                    ..CombatIntent::default()
                };
                for who in [actor, other] {
                    app.world_mut()
                        .get_mut::<Combatant>(who)
                        .expect("actor")
                        .step(
                            &mut input,
                            &PlayerIntent::default(),
                            1.0 / 60.0,
                            &mut motion,
                        );
                }
            }
            app.update();
        };
        run(&mut app, 120, true);
        assert_eq!(shown(&app), Visibility::Hidden, "scoped");
        assert_eq!(
            *app.world().get::<Visibility>(other_body).expect("body"),
            Visibility::Inherited,
            "a scoped actor that is not the local player stays drawn"
        );
        run(&mut app, 1, false);
        assert_eq!(shown(&app), Visibility::Inherited, "aim dropped");
    }

    #[test]
    fn the_guard_pose_is_asked_for_exactly_while_the_sim_says_the_guard_is_up() {
        let mut app = App::new();
        app.add_systems(Update, pose_guard);
        let actor = app
            .world_mut()
            .spawn(
                Combatant::default().carrying([Some(strata_sim::combat::Weapon::Blade), None], 0),
            )
            .id();
        let arm = app
            .world_mut()
            .spawn((
                GuardArm {
                    actor,
                    upper: false,
                },
                Transform::default(),
            ))
            .id();
        app.update();
        let still = |app: &App| app.world().get::<Transform>(arm).expect("arm").rotation;
        assert_eq!(still(&app), Quat::IDENTITY, "down: the bone is untouched");
        assert!(!guard_pose(
            app.world().get::<Combatant>(actor).expect("actor")
        ));
        app.world_mut()
            .get_mut::<Combatant>(actor)
            .expect("actor")
            .guard_up = true;
        app.update();
        assert!(guard_pose(
            app.world().get::<Combatant>(actor).expect("actor")
        ));
        assert_ne!(still(&app), Quat::IDENTITY, "up: the arm is raised");
    }

    #[test]
    fn the_thin_shape_is_hidden_only_once_a_model_is_settled() {
        let blade =
            Combatant::default().carrying([Some(strata_sim::combat::Weapon::Blade), None], 0);
        assert!(thin_shape_shown(&blade, false), "nothing baked: kept");
        assert!(!thin_shape_shown(&blade, true), "a model stands: hidden");
        let gun =
            Combatant::default().carrying([Some(strata_sim::combat::Weapon::Carbine), None], 0);
        assert!(!thin_shape_shown(&gun, false), "a gun never draws it");
    }

    #[test]
    fn strafing_turns_the_legs_without_rolling_the_aimed_upper_body() {
        let hip = Quat::from_rotation_x(1.3) * Quat::from_rotation_z(0.2);
        let spine = Quat::from_rotation_x(-0.15);
        let turn = Quat::from_rotation_y(1.047);
        let original = hip * spine;
        let corrected = turn * hip * counter_hip_turn(hip, turn) * spine;
        assert!(original.angle_between(corrected).abs() < 0.001);
    }
}
