//! The props the household's places stand in the world: the bench places,
//! the trainers, terminal, locker, workbench and fabricator, their art, their
//! dressing once the ground streams, and the labels over them. Separate
//! because it is the one part of the household that touches the world's
//! ground and the renderer.

use super::*;

/// A prop to stand in the world once the ground under it has streamed:
/// which, where (world `x`, `z`), facing which way.
#[derive(Debug, Clone)]
pub(super) struct PropSite {
    pub(super) kind: InteractionKind,
    pub(super) name: &'static str,
    pub(super) x: f64,
    pub(super) z: f64,
    pub(super) yaw: f32,
}

/// The props not yet placed. Filled by [`sites`] when the player spawns,
/// drained by [`place_props`] as the ground under each arrives.
#[derive(Resource, Debug, Default)]
pub(crate) struct PropSites(pub(super) Vec<PropSite>);

/// A placed prop's role, for the art and for queries.
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Prop {
    Terminal,
    Blade,
    Marksman,
    Locker,
    Workbench,
    Fabricator,
    Board,
    Surveyor,
    Foreman,
    Harvester,
    Cache,
    /// The monitoring station's mast (ticket 0014), stood by `people`.
    Mast,
    /// Wim's pump on the cut channel (ticket 0014), stood by `people`.
    Pump,
}

/// The nine props of a bench place, around a point and heading: the
/// terminal in the middle, a trainer to either side, the locker past the
/// marksman trainer, the workbench past the locker and the fabricator past
/// the workbench, facing `yaw`'s opposite so a walker coming from `(x, z)`
/// meets their fronts.
///
/// The locker stands two spacings out and not beside the terminal because
/// the sim reports only the nearest prop within reach: nearer, it could be
/// what a character standing at the terminal or the blade trainer is
/// offered, and the scripted path waits at each for that prop and no
/// other. The workbench stands one spacing past it for the same reason,
/// and beside it because a part fitted there comes out of the store: at
/// this spacing a character midway between the two has both within reach.
/// The fabricator stands one spacing past the workbench, last in the row:
/// what is recorded at the one is run at the other, and a run draws on the
/// same store. The procurement board stands two spacings the other way,
/// past the blade trainer: the one prop with no stock to share, so it
/// needs no neighbour, and the trainer's stop is still the nearest at its
/// own post. The survey trainer stands three spacings that way, one past the
/// board, so that the board stays between it and the blade trainer and the
/// trainer's own stop is the nearest at its post. The works foreman, who
/// teaches Fabricator and Weaponsmith, stands one spacing past the
/// fabricator at the other end of the row, so the fabricator stays between
/// him and the workbench and his own stop is the nearest at his post.
pub(super) fn bench_place(x: f64, z: f64, yaw: f32, spacing: f64) -> Vec<PropSite> {
    let right = PlayerIntent::right(yaw);
    let along = |s: f64| (x + f64::from(right.x) * s, z + f64::from(right.z) * s);
    let facing = yaw + std::f32::consts::PI;
    let (tx, tz) = along(0.);
    let (bx, bz) = along(spacing);
    let (mx, mz) = along(-spacing);
    let (lx, lz) = along(-2. * spacing);
    let (wx, wz) = along(-3. * spacing);
    let (fx, fz) = along(-4. * spacing);
    let (px, pz) = along(2. * spacing);
    let (sx, sz) = along(3. * spacing);
    let (gx, gz) = along(-5. * spacing);
    vec![
        PropSite {
            kind: InteractionKind::Terminal,
            name: "Terminal",
            x: tx,
            z: tz,
            yaw: facing,
        },
        PropSite {
            kind: InteractionKind::Trainer {
                trees: vec!["blade".to_owned()],
            },
            name: "Blade trainer",
            x: bx,
            z: bz,
            yaw: facing,
        },
        PropSite {
            kind: InteractionKind::Trainer {
                trees: vec!["marksman".to_owned()],
            },
            name: "Marksman trainer",
            x: mx,
            z: mz,
            yaw: facing,
        },
        PropSite {
            kind: InteractionKind::Locker,
            name: "Locker",
            x: lx,
            z: lz,
            yaw: facing,
        },
        PropSite {
            kind: InteractionKind::Workbench,
            name: "Workbench",
            x: wx,
            z: wz,
            yaw: facing,
        },
        PropSite {
            kind: InteractionKind::Fabricator,
            name: "Fabricator",
            x: fx,
            z: fz,
            yaw: facing,
        },
        PropSite {
            kind: InteractionKind::Board {
                buyer: pelagic_works(),
            },
            name: "Pelagic procurement",
            x: px,
            z: pz,
            yaw: facing,
        },
        PropSite {
            kind: InteractionKind::Trainer {
                trees: vec!["surveyor".to_owned(), "ranger".to_owned()],
            },
            name: "Survey trainer",
            x: sx,
            z: sz,
            yaw: facing,
        },
        PropSite {
            kind: InteractionKind::Trainer {
                trees: vec!["fabricator".to_owned(), "weaponsmith".to_owned()],
            },
            name: "Works foreman",
            x: gx,
            z: gz,
            yaw: facing,
        },
    ]
}

/// The one buyer of this step, whose board the bench places stand.
#[allow(clippy::expect_used, reason = "a literal id that parses")]
pub(crate) fn pelagic_works() -> strata::registry::Namespaced {
    strata::registry::Namespaced::parse("veyra:buyer.pelagic_works")
        .expect("the buyer's id is well formed")
}

/// The two bench places of the slice: one seven metres behind and five to
/// the right of the combat staging spawn, facing it, and one on Port
/// Serein's Exchange square, four metres uphill of the `--at city` street
/// spawn in the settlement's frame and along its row from there
/// ([`city_place`]). Heights come later, from the streamed ground.
pub(crate) fn sites(
    staging: Option<((f64, f64), f32)>,
    settlements: Option<&strata::sim::Settlements>,
) -> PropSites {
    let mut sites = Vec::new();
    // Behind the player's start, so the encounter's line of fire ahead is
    // clear and the walk back after the fight is short. Only at the
    // staging spawn: a start elsewhere (`--at city`) has the city's.
    if let Some((spawn, heading)) = staging {
        let behind = Quat::from_rotation_y(heading) * Vec3::new(5., 0., 7.);
        sites.extend(bench_place(
            spawn.0 + f64::from(behind.x),
            spawn.1 + f64::from(behind.z),
            heading + std::f32::consts::PI,
            3.5,
        ));
    }
    if let Some(((x, z), yaw)) = city_place(settlements) {
        sites.extend(bench_place(x, z, yaw, CITY_SPACING_M));
    }
    PropSites(sites)
}

/// How far along its row the city's bench place stands from the point
/// uphill of the street spawn, in spacings toward the blade trainer's end.
/// The row runs from the works foreman at -5 to the survey trainer at +3
/// spacings; a spacing of three metres puts the foreman 12 m and the survey
/// trainer 12 m from the place, the foreman clear of the curved wall that
/// closes the Exchange square on his side and the survey trainer a
/// couple of metres off the building at the other end, on open paving
/// (checked with `--at foreman`, `--at bench` and `--at survey`; at half a
/// spacing the foreman stood against the wall).
pub(super) const CITY_ROW_SHIFT: f64 = 1.0;

/// Where the city's bench place stands, its terminal's `(x, z)`, and its
/// heading. `None` without a settlement that has a layout.
pub(super) fn city_place(
    settlements: Option<&strata::sim::Settlements>,
) -> Option<((f64, f64), f32)> {
    let settlement = settlements?.0.first()?;
    let layout = settlement.layout.as_ref()?;
    let [sx, _, sz] = layout.spawn;
    let at = settlement.to_world([sx, 0., sz - 4.]);
    #[allow(
        clippy::cast_possible_truncation,
        reason = "a heading in radians, render-space f32"
    )]
    let yaw = settlement.world_yaw(0.) as f32;
    let right = PlayerIntent::right(yaw);
    let along = CITY_ROW_SHIFT * CITY_SPACING_M;
    Some((
        (
            at.x() + f64::from(right.x) * along,
            at.z() + f64::from(right.z) * along,
        ),
        yaw,
    ))
}

/// The camp's two props, for [`PropSites`], where [`strata::sim::quests::camp`]
/// finds firm ground for them.
pub(super) fn camp_sites(camp: Option<strata::sim::quests::Camp>) -> Vec<PropSite> {
    let Some(camp) = camp else {
        return Vec::new();
    };
    let facing = camp.yaw + std::f32::consts::PI;
    vec![
        PropSite {
            kind: InteractionKind::Terminal,
            name: "Camp terminal",
            x: camp.terminal[0],
            z: camp.terminal[1],
            yaw: facing,
        },
        PropSite {
            kind: InteractionKind::Workbench,
            name: "Camp workbench",
            x: camp.bench[0],
            z: camp.bench[1],
            yaw: facing,
        },
    ]
}

/// How far before the props `--at bench` stands, metres: near enough that
/// the workbench's and the fabricator's own shapes are read, with the
/// locker beside them whole in the frame.
pub(super) const BENCH_VIEW_M: f64 = 3.;

/// `--at bench`: where to stand and which way to look to see the city's
/// locker, workbench and fabricator from the front. Before the gap
/// between the workbench and the fabricator (three and four spacings along
/// the row, the locker two), looking at their fronts. `None` without a
/// settlement that has a layout.
pub(crate) fn bench_view(
    settlements: Option<&strata::sim::Settlements>,
) -> Option<((f64, f64), f32)> {
    let ((x, z), yaw) = city_place(settlements)?;
    let (right, forward) = (PlayerIntent::right(yaw), PlayerIntent::forward(yaw));
    // Before the gap and not a prop: the camera is behind the character,
    // who would stand between it and whatever they stood before.
    let along = -3.3 * CITY_SPACING_M;
    // The props face the place's back, which is where a walker comes
    // from, so the view is from behind the place's own heading, looking
    // along it.
    Some((
        (
            x + f64::from(right.x) * along - f64::from(forward.x) * BENCH_VIEW_M,
            z + f64::from(right.z) * along - f64::from(forward.z) * BENCH_VIEW_M,
        ),
        yaw,
    ))
}

/// How far before the terminal `--at terminal` stands, metres: inside its
/// reach, so the bench opens as its F opens it.
pub(super) const TERMINAL_VIEW_M: f64 = 1.5;

/// `--at terminal`: where to stand and which way to look to be at the
/// city's terminal, facing it. `None` without a settlement that has a
/// layout.
pub(crate) fn terminal_view(
    settlements: Option<&strata::sim::Settlements>,
) -> Option<((f64, f64), f32)> {
    let ((x, z), yaw) = city_place(settlements)?;
    let forward = PlayerIntent::forward(yaw);
    Some((
        (
            x - f64::from(forward.x) * TERMINAL_VIEW_M,
            z - f64::from(forward.z) * TERMINAL_VIEW_M,
        ),
        yaw,
    ))
}

/// `--at board`: where to stand and which way to look to be at the city's
/// procurement board, facing it. `None` without a settlement that has a
/// layout.
pub(crate) fn board_view(
    settlements: Option<&strata::sim::Settlements>,
) -> Option<((f64, f64), f32)> {
    let ((x, z), yaw) = city_place(settlements)?;
    let (right, forward) = (PlayerIntent::right(yaw), PlayerIntent::forward(yaw));
    let along = 2. * CITY_SPACING_M;
    Some((
        (
            x + f64::from(right.x) * along - f64::from(forward.x) * TERMINAL_VIEW_M,
            z + f64::from(right.z) * along - f64::from(forward.z) * TERMINAL_VIEW_M,
        ),
        yaw,
    ))
}

/// `--at survey`: where to stand and which way to look to be at the city's
/// survey trainer, facing it. `None` without a settlement that has a layout.
pub(crate) fn survey_view(
    settlements: Option<&strata::sim::Settlements>,
) -> Option<((f64, f64), f32)> {
    let ((x, z), yaw) = city_place(settlements)?;
    let (right, forward) = (PlayerIntent::right(yaw), PlayerIntent::forward(yaw));
    let along = 3. * CITY_SPACING_M;
    Some((
        (
            x + f64::from(right.x) * along - f64::from(forward.x) * TERMINAL_VIEW_M,
            z + f64::from(right.z) * along - f64::from(forward.z) * TERMINAL_VIEW_M,
        ),
        yaw,
    ))
}

/// `--at foreman`: where to stand and which way to look to be at the city's
/// works foreman, facing him. `None` without a settlement that has a layout.
pub(crate) fn foreman_view(
    settlements: Option<&strata::sim::Settlements>,
) -> Option<((f64, f64), f32)> {
    let ((x, z), yaw) = city_place(settlements)?;
    let (right, forward) = (PlayerIntent::right(yaw), PlayerIntent::forward(yaw));
    let along = -5. * CITY_SPACING_M;
    Some((
        (
            x + f64::from(right.x) * along - f64::from(forward.x) * TERMINAL_VIEW_M,
            z + f64::from(right.z) * along - f64::from(forward.z) * TERMINAL_VIEW_M,
        ),
        yaw,
    ))
}

/// The gap between the props of the city's bench place, metres.
pub(super) const CITY_SPACING_M: f64 = 3.0;

/// Ground height under `(x, z)` by the encounter's query: the terrain's
/// sample as a guess, then a ray down from thirty metres above it through
/// whatever has streamed (terrain, a settlement's platform). `None` until
/// something is there to hit.
pub(crate) fn ground_at(
    x: f64,
    z: f64,
    source: &WorldSourceRes,
    origin: &RenderOrigin,
    spatial: &SpatialQuery,
    filter: &SpatialQueryFilter,
) -> Option<f64> {
    let sampled = f64::from(source.source().sample(x, z).height);
    let above = origin.to_render(WorldPos::new(x, sampled + 30., z));
    let hit = spatial.cast_ray(above, Dir3::NEG_Y, 60., true, filter)?;
    Some(origin.to_world(above - Vec3::Y * hit.distance).y())
}

/// Stand each pending prop on the ground once the ground under it has
/// streamed. The sim side (position, interaction) spawns in every mode;
/// the art is the window's business ([`dress_props`]).
pub(super) fn place_props(
    mut commands: Commands,
    mut sites: ResMut<PropSites>,
    source: Res<WorldSourceRes>,
    origin: Res<RenderOrigin>,
    spatial: SpatialQuery,
    actors: Query<Entity, With<Combatant>>,
) {
    if sites.0.is_empty() {
        return;
    }
    let filter = SpatialQueryFilter::from_excluded_entities(actors.iter());
    let mut pending = Vec::new();
    for site in sites.0.drain(..) {
        let Some(ground) = ground_at(site.x, site.z, &source, &origin, &spatial, &filter) else {
            pending.push(site);
            continue;
        };
        let at = WorldPos::new(site.x, ground, site.z);
        let prop = match site.kind {
            InteractionKind::Terminal => Prop::Terminal,
            InteractionKind::Trainer { ref trees } if trees.iter().any(|t| t == "blade") => {
                Prop::Blade
            }
            InteractionKind::Trainer { ref trees } if trees.iter().any(|t| t == "surveyor") => {
                Prop::Surveyor
            }
            InteractionKind::Trainer { ref trees } if trees.iter().any(|t| t == "fabricator") => {
                Prop::Foreman
            }
            InteractionKind::Trainer { .. } => Prop::Marksman,
            InteractionKind::Locker => Prop::Locker,
            InteractionKind::Workbench => Prop::Workbench,
            InteractionKind::Fabricator => Prop::Fabricator,
            InteractionKind::Board { .. } => Prop::Board,
            InteractionKind::Harvester { .. } => Prop::Harvester,
            // A guard is a person of a post, never a household prop.
            // A cache is its own prop, stood by the camps, never a bench place.
            // A person of the questline is stood by `people.rs`, not a bench prop.
            InteractionKind::Guard { .. }
            | InteractionKind::Cache { .. }
            | InteractionKind::Person { .. } => continue,
        };
        info!(
            "household: {} placed at ({:.1}, {:.1}, {:.1})",
            site.name,
            at.x(),
            at.y(),
            at.z()
        );
        commands.spawn((
            Name::new(site.name),
            prop,
            Interaction {
                kind: site.kind,
                radius: REACH_M,
            },
            WorldPosition(at),
            Transform::from_rotation(Quat::from_rotation_y(site.yaw)),
            Visibility::default(),
        ));
    }
    sites.0 = pending;
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
    props: Query<(Entity, &Prop), Added<Prop>>,
    harvesters: Query<&HarvesterProp>,
) {
    for (entity, prop) in &props {
        let (role, parts) = prop_parts(*prop);
        let lamp = (*prop == Prop::Harvester).then(|| parts.len() - 1);
        let id = harvesters.get(entity).ok().map(|stood| stood.0);
        commands.entity(entity).with_children(|parent| {
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
    let scale = strata::client::combat_hud::ui_scale(ui_scale.as_deref());
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
            let at = strata::client::combat_hud::ui_point(at.truncate(), scale);
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
