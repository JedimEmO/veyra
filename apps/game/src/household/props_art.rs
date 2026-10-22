//! The art of the household's props: the baked kit or the cuboids that stand
//! in, their dressing once the placement has spawned them, and the labels over
//! them. The window's business; where the props stand and what each does is
//! `veyra_game::household::props`.

use ashlar_bevy::prelude::AshlarProp;

use super::*;

/// The baked prop kit's index (review step six): which file draws which
/// interaction prop. Empty on a checkout that has not run `just content`, and
/// the cuboids stand in.
#[derive(Resource, Debug, Clone, Default, PartialEq)]
pub(crate) struct PropKit(pub(crate) strata::registry::props::PropModels);

impl PropKit {
    /// The index under `generated`, as a source rooted there. A missing one
    /// is no error: it is said once at info and the boxes are drawn.
    pub(crate) fn read(generated: &strata::registry::files::Files) -> Self {
        use strata::registry::props::{MODELS_PATH, PropModels};
        match generated
            .read_string(MODELS_PATH)
            .map_err(|error| error.to_string())
            .and_then(|text| PropModels::from_ron(&text).map_err(|error| error.to_string()))
        {
            Ok(set) => {
                info!(props = set.files.len(), "prop kit");
                Self(set)
            }
            Err(error) => {
                info!(%error, "no prop kit: boxes stand in until `just content` runs");
                Self::default()
            }
        }
    }
}

/// The kit's piece that draws a prop, where it has one: the harvester, the
/// cache, the mast and the pump are not in the kit, and keep their boxes.
pub(super) const fn kit_piece(prop: Prop) -> Option<veyra_buildings::props::Piece> {
    use veyra_buildings::props::Piece;
    match prop {
        Prop::Terminal => Some(Piece::Terminal),
        Prop::Blade => Some(Piece::Blade),
        Prop::Marksman => Some(Piece::Marksman),
        Prop::Locker => Some(Piece::Locker),
        Prop::Workbench => Some(Piece::Workbench),
        Prop::Fabricator => Some(Piece::Fabricator),
        Prop::Board => Some(Piece::Board),
        Prop::Surveyor => Some(Piece::Surveyor),
        Prop::Foreman => Some(Piece::Foreman),
        Prop::Harvester | Prop::Cache | Prop::Mast | Prop::Pump => None,
    }
}

/// The meshes and materials the props are built from: the cube, a dark
/// body, and an emissive strip per role so each reads at night.
#[derive(Resource)]
pub(super) struct PropArt {
    pub(super) cube: Handle<Mesh>,
    pub(super) body: Handle<StandardMaterial>,
    pub(super) strips: [Handle<StandardMaterial>; 9],
}

/// The strip colours by role: the terminal's teal, the blade trainer's
/// green, the marksman's amber, the locker's violet, the workbench's cold
/// white, which is a work lamp's, and the fabricator's red, which is a
/// machine running and no other prop's, and the board's gold, a price
/// list's and no one else's, the survey trainer's rose and the foreman's
/// hi-vis lime.
pub(super) const STRIP_COLOURS: [Color; 9] = [
    Color::srgb(0.2, 0.85, 0.9),
    Color::srgb(0.35, 0.95, 0.5),
    Color::srgb(1., 0.65, 0.2),
    Color::srgb(0.68, 0.5, 1.),
    Color::srgb(0.82, 0.92, 1.),
    Color::srgb(1., 0.28, 0.2),
    Color::srgb(0.98, 0.8, 0.28),
    Color::srgb(1., 0.45, 0.8),
    Color::srgb(0.78, 0.95, 0.2),
];

pub(super) fn load_prop_art(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    let mut strip = |colour: Color| {
        materials.add(StandardMaterial {
            base_color: colour,
            emissive: colour.to_linear() * 6.,
            ..default()
        })
    };
    let strips = STRIP_COLOURS.map(&mut strip);
    commands.insert_resource(PropArt {
        cube: meshes.add(Cuboid::new(1., 1., 1.)),
        body: materials.add(StandardMaterial {
            base_color: Color::srgb(0.2, 0.21, 0.23),
            metallic: 0.6,
            perceptual_roughness: 0.55,
            ..default()
        }),
        strips,
    });
}

/// The boxes a prop is built from: each its centre and its size in the
/// prop's own frame and whether it is a lit strip, with the role whose
/// colour the strips and the prop's light take.
#[allow(clippy::too_many_lines, reason = "one arm of boxes a prop")]
pub(super) fn prop_parts(prop: Prop) -> (usize, Vec<(Vec3, Vec3, bool)>) {
    match prop {
        Prop::Terminal => (
            0,
            vec![
                (Vec3::new(0., 0.45, 0.), Vec3::new(0.3, 0.9, 0.3), false),
                (Vec3::new(0., 1.2, -0.1), Vec3::new(1.2, 0.8, 0.12), false),
                (Vec3::new(0., 1.2, -0.04), Vec3::new(1.0, 0.06, 0.02), true),
                (Vec3::new(0., 1.42, -0.04), Vec3::new(1.0, 0.03, 0.02), true),
            ],
        ),
        // The trainers share the post and differ above it: the blade
        // trainer carries a tall thin blade and its guard, the marksman
        // a wide target board with a bright centre, so each is told
        // from the other before its name can be read.
        Prop::Blade => (
            1,
            vec![
                (Vec3::new(0., 0.9, 0.), Vec3::new(0.35, 1.8, 0.35), false),
                (Vec3::new(0., 2.3, 0.), Vec3::new(0.12, 1.4, 0.04), false),
                (Vec3::new(0., 1.7, 0.), Vec3::new(0.6, 0.08, 0.08), true),
                (Vec3::new(0., 2.3, -0.025), Vec3::new(0.04, 1.3, 0.01), true),
                (Vec3::new(0., 0.4, -0.18), Vec3::new(0.3, 0.04, 0.02), true),
            ],
        ),
        Prop::Marksman => (
            2,
            vec![
                (Vec3::new(0., 0.9, 0.), Vec3::new(0.35, 1.8, 0.35), false),
                (Vec3::new(0., 2.1, -0.1), Vec3::new(1.2, 1.2, 0.08), false),
                (Vec3::new(0., 2.1, -0.15), Vec3::new(0.8, 0.8, 0.02), true),
                (Vec3::new(0., 2.1, -0.17), Vec3::new(0.3, 0.3, 0.02), false),
                (Vec3::new(0., 0.4, -0.18), Vec3::new(0.3, 0.04, 0.02), true),
            ],
        ),
        // Wider than it is deep and as tall as a character, with a lit
        // seam between its doors and a latch on each, so it reads as
        // something that opens and is told from the posts beside it.
        Prop::Locker => (
            3,
            vec![
                (Vec3::new(0., 0.95, 0.), Vec3::new(1.3, 1.9, 0.55), false),
                (Vec3::new(0., 0.95, -0.28), Vec3::new(0.03, 1.7, 0.02), true),
                (
                    Vec3::new(-0.14, 1.05, -0.29),
                    Vec3::new(0.05, 0.22, 0.03),
                    true,
                ),
                (
                    Vec3::new(0.14, 1.05, -0.29),
                    Vec3::new(0.05, 0.22, 0.03),
                    true,
                ),
                (Vec3::new(0., 1.93, 0.), Vec3::new(1.36, 0.06, 0.6), false),
            ],
        ),
        // Low and wide where the locker beside it is tall: a top at
        // working height on two pedestals with a shelf between them, a
        // low board behind it with a lit tool rail, a lamp on an arm
        // over the left end, a vice at the right and something long
        // lying on the top, so it reads as a place where a weapon is
        // worked on before its name can be read.
        Prop::Workbench => (
            4,
            vec![
                (
                    Vec3::new(-0.62, 0.44, 0.),
                    Vec3::new(0.36, 0.88, 0.62),
                    false,
                ),
                (
                    Vec3::new(0.62, 0.44, 0.),
                    Vec3::new(0.36, 0.88, 0.62),
                    false,
                ),
                (Vec3::new(0., 0.26, 0.), Vec3::new(0.9, 0.05, 0.56), false),
                (Vec3::new(0., 0.92, 0.), Vec3::new(1.7, 0.08, 0.76), false),
                (
                    Vec3::new(0., 0.92, -0.385),
                    Vec3::new(1.5, 0.03, 0.02),
                    true,
                ),
                (Vec3::new(0., 1.26, 0.35), Vec3::new(1.7, 0.6, 0.06), false),
                (Vec3::new(0., 1.42, 0.31), Vec3::new(1.3, 0.04, 0.02), true),
                (
                    Vec3::new(-0.66, 1.3, 0.24),
                    Vec3::new(0.05, 0.68, 0.05),
                    false,
                ),
                (
                    Vec3::new(-0.5, 1.62, 0.04),
                    Vec3::new(0.38, 0.05, 0.14),
                    false,
                ),
                (Vec3::new(-0.5, 1.59, 0.04), Vec3::new(0.3, 0.02, 0.1), true),
                (
                    Vec3::new(0.58, 1.03, -0.16),
                    Vec3::new(0.22, 0.14, 0.2),
                    false,
                ),
                (
                    Vec3::new(-0.05, 0.99, -0.02),
                    Vec3::new(0.8, 0.06, 0.1),
                    false,
                ),
                (
                    Vec3::new(-0.1, 1.04, -0.02),
                    Vec3::new(0.16, 0.04, 0.06),
                    false,
                ),
            ],
        ),
        // Stood by `harvesters::stand_harvesters`, not by `sites`.
        Prop::Harvester => (4, harvester_parts()),
        // Stood by `cache::stand_caches`: a lidded chest, its seam and two
        // clasps lit amber so it is seen from across a camp.
        Prop::Cache => (
            2,
            vec![
                (Vec3::new(0., 0.3, 0.), Vec3::new(1.1, 0.6, 0.7), false),
                (Vec3::new(0., 0.68, 0.), Vec3::new(1.14, 0.16, 0.74), false),
                (Vec3::new(0., 0.6, -0.36), Vec3::new(1.12, 0.05, 0.02), true),
                (
                    Vec3::new(-0.3, 0.55, -0.37),
                    Vec3::new(0.12, 0.2, 0.03),
                    true,
                ),
                (
                    Vec3::new(0.3, 0.55, -0.37),
                    Vec3::new(0.12, 0.2, 0.03),
                    true,
                ),
            ],
        ),
        // The station's mast: a hut, a tall mast on it and a dish at its
        // head, with its lamp unlit and a dark band where a harness was. Lit
        // teal, the terminal's, so it is told from the ground at a distance.
        Prop::Mast => (
            0,
            vec![
                (Vec3::new(0., 0.9, 0.), Vec3::new(2.2, 1.8, 1.6), false),
                (Vec3::new(0., 1.85, 0.), Vec3::new(2.4, 0.1, 1.8), false),
                (Vec3::new(0., 4.4, 0.), Vec3::new(0.18, 5.0, 0.18), false),
                (Vec3::new(0., 6.4, -0.2), Vec3::new(1.2, 1.2, 0.08), false),
                (Vec3::new(0., 6.4, -0.26), Vec3::new(0.4, 0.4, 0.02), true),
                (Vec3::new(0., 1.1, -0.82), Vec3::new(1.2, 0.5, 0.03), true),
            ],
        ),
        // The pump: a squat engine on a skid, a pipe running down to the
        // water and a lit gauge, in the workbench's cold white.
        Prop::Pump => (
            4,
            vec![
                (Vec3::new(0., 0.1, 0.), Vec3::new(1.6, 0.2, 1.2), false),
                (Vec3::new(0., 0.65, 0.), Vec3::new(1.0, 0.7, 0.8), false),
                (Vec3::new(0., 1.15, 0.), Vec3::new(0.7, 0.3, 0.6), false),
                (Vec3::new(1.1, 0.3, 0.), Vec3::new(0.35, 0.6, 0.35), false),
                (Vec3::new(1.9, 0.12, 0.), Vec3::new(1.6, 0.2, 0.3), false),
                (Vec3::new(0., 0.75, -0.41), Vec3::new(0.5, 0.2, 0.02), true),
            ],
        ),
        Prop::Fabricator => (5, fabricator_parts()),
        Prop::Board => (6, board_parts()),
        // The trainers' post with a mast on it and a dish at the mast's head
        // over a lit ring, so it reads as a place that listens to the ground.
        Prop::Surveyor => (
            7,
            vec![
                (Vec3::new(0., 0.9, 0.), Vec3::new(0.35, 1.8, 0.35), false),
                (Vec3::new(0., 2.4, 0.), Vec3::new(0.08, 1.2, 0.08), false),
                (Vec3::new(0., 3.05, -0.1), Vec3::new(0.9, 0.9, 0.06), false),
                (Vec3::new(0., 3.05, -0.14), Vec3::new(0.3, 0.3, 0.02), true),
                (Vec3::new(0., 1.7, -0.18), Vec3::new(0.5, 0.06, 0.02), true),
                (Vec3::new(0., 0.4, -0.18), Vec3::new(0.3, 0.04, 0.02), true),
            ],
        ),
        // The trainers' post with a hard hat on it and a wide lit clipboard
        // at chest height with a lit band across the post, so it reads as
        // somebody who runs the works.
        Prop::Foreman => (
            8,
            vec![
                (Vec3::new(0., 0.9, 0.), Vec3::new(0.35, 1.8, 0.35), false),
                (Vec3::new(0., 1.9, 0.), Vec3::new(0.5, 0.14, 0.5), false),
                (Vec3::new(0., 1.98, 0.), Vec3::new(0.3, 0.1, 0.3), false),
                (Vec3::new(0., 1.3, -0.12), Vec3::new(0.9, 0.7, 0.06), false),
                (Vec3::new(0., 1.3, -0.16), Vec3::new(0.7, 0.5, 0.02), true),
                (Vec3::new(0., 0.4, -0.18), Vec3::new(0.3, 0.04, 0.02), true),
            ],
        ),
    }
}

/// The field harvester's boxes: a squat body on a plinth, a hopper window
/// low in its front, a mast rising from the back and, last, the lamp on the
/// mast's head, which [`dress_props`] marks so its colour follows the
/// machine (lit while it runs, steady another colour when full).
pub(super) fn harvester_parts() -> Vec<(Vec3, Vec3, bool)> {
    vec![
        (Vec3::new(0., 0.04, 0.), Vec3::new(1.5, 0.08, 1.5), false),
        (Vec3::new(0., 0.45, 0.), Vec3::new(1.2, 0.74, 1.2), false),
        (Vec3::new(0., 0.4, -0.61), Vec3::new(0.7, 0.3, 0.02), true),
        (Vec3::new(0., 0.9, 0.), Vec3::new(1.0, 0.1, 1.0), false),
        // The lamp sits under where a prop's name floats (2.3 m), or the
        // name would cover the one thing the machine says by sight.
        (Vec3::new(0.4, 1.4, 0.4), Vec3::new(0.08, 1.0, 0.08), false),
        (Vec3::new(0.4, 2.0, 0.4), Vec3::new(0.4, 0.4, 0.4), true),
    ]
}

/// The light a harvester throws, which wears its lamp's colour so the
/// machine reads as a beacon at dusk.
#[derive(Component, Debug, Clone, Copy)]
pub(super) struct HarvesterGlow(pub(super) u32);

/// The lamp on a harvester's mast: lit green while it runs, steady amber
/// when it is full.
#[derive(Component, Debug, Clone, Copy)]
pub(super) struct HarvesterLamp(pub(super) u32);

/// The plate over a running harvester's body, which pumps up and down while
/// the machine works: where it rests, which machine it is and the prop it is
/// part of. `level` is how far into the stroke's swing it has eased, so it
/// starts and stops softly, and `phase` where the stroke is.
#[derive(Component, Debug, Clone, Copy)]
pub(super) struct HarvesterPump {
    pub(super) id: u32,
    pub(super) owner: Entity,
    pub(super) rest: f32,
    pub(super) level: f32,
    pub(super) phase: f32,
}

/// Which of [`harvester_parts`] pumps: the plate over the body.
pub(super) const PUMP_PART: usize = 3;

/// The board's boxes: a standing panel on two posts with a lit face, read
/// from the front, a frame round a field of lit rows and a lit foot rail,
/// so it is told from the trainers' signs by its width and its many lines.
pub(super) fn board_parts() -> Vec<(Vec3, Vec3, bool)> {
    vec![
        (Vec3::new(-0.8, 0.95, 0.), Vec3::new(0.12, 1.9, 0.12), false),
        (Vec3::new(0.8, 0.95, 0.), Vec3::new(0.12, 1.9, 0.12), false),
        (Vec3::new(0., 1.55, -0.02), Vec3::new(1.9, 1.2, 0.08), false),
        (Vec3::new(0., 1.55, -0.07), Vec3::new(1.7, 1.0, 0.02), false),
        (Vec3::new(0., 1.9, -0.085), Vec3::new(1.4, 0.06, 0.01), true),
        (
            Vec3::new(0., 1.68, -0.085),
            Vec3::new(1.4, 0.06, 0.01),
            true,
        ),
        (
            Vec3::new(0., 1.46, -0.085),
            Vec3::new(1.4, 0.06, 0.01),
            true,
        ),
        (
            Vec3::new(0., 1.24, -0.085),
            Vec3::new(1.0, 0.06, 0.01),
            true,
        ),
        (Vec3::new(0., 0.3, -0.1), Vec3::new(1.6, 0.04, 0.02), true),
    ]
}

/// The fabricator's boxes: a machine's body, lower than the locker and
/// deeper than anything in the row, with a hopper standing on its top at
/// the left where the stock goes in, a lit mouth low in its front over a
/// tray where what it makes comes out, four lit segments up its right
/// front as a gauge, and a stack rising from its back to above head
/// height. So it reads as a machine that is fed and runs on its own, and
/// is told from the locker by its depth, its stack and its red before its
/// name can be read.
pub(super) fn fabricator_parts() -> Vec<(Vec3, Vec3, bool)> {
    let mut parts = vec![
        // The body, and a plinth a little wider under it.
        (Vec3::new(0., 0.64, 0.1), Vec3::new(1.9, 1.12, 1.0), false),
        (Vec3::new(0., 0.04, 0.1), Vec3::new(2.0, 0.08, 1.1), false),
        // The hopper: a neck, a wider mouth on it and a lit rim.
        (
            Vec3::new(-0.52, 1.36, 0.16),
            Vec3::new(0.5, 0.32, 0.5),
            false,
        ),
        (
            Vec3::new(-0.52, 1.6, 0.16),
            Vec3::new(0.76, 0.16, 0.76),
            false,
        ),
        (
            Vec3::new(-0.52, 1.6, -0.225),
            Vec3::new(0.6, 0.03, 0.02),
            true,
        ),
        // The mouth, and the tray under it.
        (
            Vec3::new(-0.2, 0.52, -0.405),
            Vec3::new(0.8, 0.26, 0.02),
            true,
        ),
        (
            Vec3::new(-0.2, 0.34, -0.56),
            Vec3::new(0.9, 0.05, 0.34),
            false,
        ),
        // The stack at the back, with a lit band under its cap.
        (
            Vec3::new(0.62, 1.65, 0.4),
            Vec3::new(0.22, 0.9, 0.22),
            false,
        ),
        (
            Vec3::new(0.62, 1.96, 0.4),
            Vec3::new(0.26, 0.04, 0.26),
            true,
        ),
        (Vec3::new(0.62, 2.1, 0.4), Vec3::new(0.3, 0.05, 0.3), false),
    ];
    // The gauge: four segments up the right of the front.
    parts.extend((0..4_u8).map(|segment| {
        (
            Vec3::new(0.66, 0.34 + 0.2 * f32::from(segment), -0.405),
            Vec3::new(0.26, 0.1, 0.02),
            true,
        )
    }));
    parts
}

/// The art under a placed prop, in the window: a terminal is a slab
/// leaning on a post with a strip across its face; a trainer is a post
/// with a sign and a band; a locker is a cabinet of two doors; a workbench
/// is a table on two pedestals under a tool board and a lamp, with a
/// weapon lying on it; a fabricator is a machine's body with a hopper on
/// it, a lit mouth over a tray, a gauge and a stack.
/// Each carries a small light of its strip's colour,
/// so the place is found at night.
pub(super) fn dress_props(
    mut commands: Commands,
    art: Res<PropArt>,
    kit: Option<Res<PropKit>>,
    server: Option<Res<AssetServer>>,
    props: Query<(Entity, &Prop), Added<Prop>>,
    harvesters: Query<&HarvesterProp>,
) {
    for (entity, prop) in &props {
        let (role, mut parts) = prop_parts(*prop);
        // A piece the kit has is drawn from its baked file; the boxes stay
        // for a prop it has none of, and for a checkout with nothing baked.
        let baked = kit_piece(*prop).and_then(|piece| {
            let kit = kit.as_deref()?;
            let file = kit.0.file(piece.name())?;
            let server = server.as_deref()?;
            Some(AshlarProp {
                prop: server.load(format!("generated://{file}")),
                materials: server.load(format!("generated://{}", kit.0.materials)),
            })
        });
        if baked.is_some() {
            parts.clear();
        }
        let lamp = (*prop == Prop::Harvester).then(|| parts.len() - 1);
        let id = harvesters.get(entity).ok().map(|stood| stood.0);
        commands.entity(entity).with_children(|parent| {
            if let Some(baked) = baked {
                parent.spawn((baked, Transform::default(), Visibility::Inherited));
            }
            for (index, (centre, size, strip)) in parts.into_iter().enumerate() {
                let mut part = parent.spawn((
                    Mesh3d(art.cube.clone()),
                    MeshMaterial3d(if strip {
                        art.strips[role].clone()
                    } else {
                        art.body.clone()
                    }),
                    Transform::from_translation(centre).with_scale(size),
                ));
                if let (Some(lamp), Some(id)) = (lamp, id)
                    && lamp == index
                {
                    part.insert(HarvesterLamp(id));
                }
                if let Some(id) = id
                    && *prop == Prop::Harvester
                    && index == PUMP_PART
                {
                    part.insert(HarvesterPump {
                        id,
                        owner: entity,
                        rest: centre.y,
                        level: 0.,
                        phase: 0.,
                    });
                }
            }
            let mut glow = parent.spawn((
                PointLight {
                    color: STRIP_COLOURS[if id.is_some() { 1 } else { role }],
                    intensity: 6_000.,
                    range: 7.,
                    shadow_maps_enabled: false,
                    ..default()
                },
                Transform::from_xyz(0., 1.6, -0.5),
            ));
            if let Some(id) = id {
                glow.insert(HarvesterGlow(id));
            }
        });
    }
}

/// How far a prop's name floats over it, metres.
pub(super) const LABEL_RANGE: f32 = 30.;

/// Over the last this many metres of [`LABEL_RANGE`] a label fades, so
/// it arrives and leaves rather than blinking at the edge.
pub(super) const LABEL_FADE: f32 = 10.;

/// The label's text: the theme's `text` at 90 %.
pub(super) const LABEL_TEXT: Color = Color::srgba(0.902, 0.929, 0.953, 0.9);
/// Its pill: the `tooltip` role's dark tint, a little lighter so the
/// world shows through.
pub(super) const LABEL_FILL: Color = Color::srgba(0.043, 0.055, 0.078, 0.78);
/// The pill's hairline.
pub(super) const LABEL_HAIRLINE: Color = Color::srgba(1.0, 1.0, 1.0, 0.08);
/// The shadow under the text.
pub(super) const LABEL_SHADOW: Color = Color::srgba(0.0, 0.0, 0.0, 0.7);

/// A floating name over a prop, in the window.
#[derive(Component, Debug)]
pub(super) struct PropLabel(pub(super) Entity);

/// The label's strength at `distance` metres: full near, fading over the
/// last [`LABEL_FADE`] of the range, nothing past it.
pub(super) fn label_alpha(distance: f32) -> f32 {
    ((LABEL_RANGE - distance) / LABEL_FADE).clamp(0., 1.)
}

/// Each prop within [`LABEL_RANGE`] wears its name over its head, so the
/// terminal and the two trainers are told apart from a distance; the
/// label follows the camera every frame. It is a small glass pill in the
/// theme's Manrope, centred on the point over the prop whatever the
/// name's width. Runs only while the game has the input, and
/// [`hide_labels`] takes over under a screen.
#[allow(
    clippy::too_many_arguments,
    clippy::type_complexity,
    reason = "a label is a node, its pill, its text and its fade, over the camera, the props and the player"
)]
pub(super) fn label_props(
    mut commands: Commands,
    origin: Res<RenderOrigin>,
    ui_scale: Option<Res<bevy::ui::UiScale>>,
    fonts: Option<Res<strata::client::ui::fonts::UiFonts>>,
    cameras: Query<
        (&Camera, &GlobalTransform),
        (With<Camera3d>, Without<strata::terrain::OffWorld>),
    >,
    props: Query<(Entity, &Name), With<Prop>>,
    positions: Query<&WorldPosition, With<Prop>>,
    players: Query<&WorldPosition, (With<LocalPlayer>, Without<Prop>)>,
    mut labels: Query<(
        Entity,
        &PropLabel,
        &mut Node,
        &mut Visibility,
        &mut TextColor,
        &mut TextShadow,
        &mut BackgroundColor,
        &mut BorderColor,
    )>,
) {
    let Some((camera, camera_transform)) = cameras.iter().find(|(c, _)| c.is_active) else {
        return;
    };
    let Ok(player) = players.single() else {
        return;
    };
    let Some(fonts) = fonts else {
        return;
    };
    let scale = strata::client::ui::world_ui::ui_scale(ui_scale.as_deref());
    for (entity, name) in &props {
        if !labels.iter().any(|(_, label, ..)| label.0 == entity) {
            commands.spawn((
                PropLabel(entity),
                Text::new(name.as_str()),
                TextFont {
                    font: fonts.body.clone().into(),
                    font_size: FontSize::Px(14.),
                    weight: FontWeight::SEMIBOLD,
                    ..default()
                },
                TextColor(LABEL_TEXT),
                TextShadow {
                    offset: Vec2::new(0., 1.),
                    color: LABEL_SHADOW,
                },
                TextLayout::no_wrap(),
                Node {
                    position_type: PositionType::Absolute,
                    padding: UiRect::axes(px(10.), px(4.)),
                    border: UiRect::all(px(1.)),
                    border_radius: BorderRadius::all(px(8.)),
                    ..default()
                },
                BackgroundColor(LABEL_FILL),
                BorderColor::all(LABEL_HAIRLINE),
                // Centred on its point: half its own size up and left.
                UiTransform::from_translation(Val2::percent(-50., -50.)),
                Visibility::Hidden,
                Pickable::IGNORE,
            ));
        }
    }
    for (
        label_entity,
        label,
        mut node,
        mut visibility,
        mut text,
        mut shadow,
        mut fill,
        mut border,
    ) in &mut labels
    {
        let Ok(position) = positions.get(label.0) else {
            commands.entity(label_entity).despawn();
            continue;
        };
        let delta = position.0.0 - player.0.0;
        #[allow(clippy::cast_possible_truncation, reason = "a distance in metres")]
        let distance = delta.x.hypot(delta.z) as f32;
        let strength = label_alpha(distance);
        let over = origin.to_render(position.0) + Vec3::Y * 2.3;
        let at = camera
            .world_to_viewport_with_depth(camera_transform, over)
            .ok()
            .filter(|at| at.z > 0. && strength > 0.);
        let wanted = if at.is_some() {
            Visibility::Visible
        } else {
            Visibility::Hidden
        };
        if *visibility != wanted {
            *visibility = wanted;
        }
        if let Some(at) = at {
            let at = strata::client::ui::world_ui::ui_point(at.truncate(), scale);
            node.left = px(at.x);
            node.top = px(at.y);
            text.0 = LABEL_TEXT.with_alpha(LABEL_TEXT.alpha() * strength);
            shadow.color = LABEL_SHADOW.with_alpha(LABEL_SHADOW.alpha() * strength);
            fill.0 = LABEL_FILL.with_alpha(LABEL_FILL.alpha() * strength);
            *border =
                BorderColor::all(LABEL_HAIRLINE.with_alpha(LABEL_HAIRLINE.alpha() * strength));
        }
    }
}

/// The labels stand down under a screen, as the HUD does.
pub(super) fn hide_labels(mut labels: Query<&mut Visibility, With<PropLabel>>) {
    for mut visibility in &mut labels {
        if *visibility != Visibility::Hidden {
            *visibility = Visibility::Hidden;
        }
    }
}

#[cfg(test)]
mod kit_tests {
    use super::*;

    /// The kit's panels wear the colours the props' lights and cuboid strips
    /// do, so a prop's glow and its panel are one role.
    #[test]
    fn the_kit_panels_wear_the_strip_colours() {
        for (role, colour) in veyra_buildings::props::ROLES.iter().enumerate() {
            let strip = STRIP_COLOURS[role].to_srgba();
            let kit = [colour[0], colour[1], colour[2]];
            for (a, b) in [strip.red, strip.green, strip.blue].iter().zip(kit) {
                assert!((a - b).abs() < 1e-6, "role {role}: {strip:?} vs {kit:?}");
            }
        }
    }

    /// Every prop the household's places stand has a piece of the kit.
    #[test]
    fn every_bench_prop_has_a_piece() {
        for prop in [
            Prop::Terminal,
            Prop::Blade,
            Prop::Marksman,
            Prop::Locker,
            Prop::Workbench,
            Prop::Fabricator,
            Prop::Board,
            Prop::Surveyor,
            Prop::Foreman,
        ] {
            let piece = kit_piece(prop).expect("a piece");
            assert_eq!(prop_parts(prop).0, piece.role(), "{prop:?}");
        }
    }
}
