//! The props the household's places stand in the world: the bench places,
//! the trainers, terminal, locker, workbench and fabricator, where they stand
//! and the interaction each carries once the ground under it has streamed.
//! Separate because it is the one part of the household that touches the
//! world's ground. Their art, the dressing and the labels are the window's
//! (`apps/game`'s `household::props_art`).

use super::*;

/// A prop to stand in the world once the ground under it has streamed:
/// which, where (world `x`, `z`), facing which way.
#[derive(Debug, Clone)]
pub struct PropSite {
    /// Which interaction the prop carries.
    pub kind: InteractionKind,
    /// What the prop is called.
    pub name: &'static str,
    /// World `x`.
    pub x: f64,
    /// World `z`.
    pub z: f64,
    /// Which way it faces.
    pub yaw: f32,
}

/// The props not yet placed. Filled by [`sites`] when the player spawns,
/// drained by [`place_props`] as the ground under each arrives.
#[derive(Resource, Debug, Default)]
pub struct PropSites(pub Vec<PropSite>);

/// A placed prop's role, for the art and for queries.
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq)]
pub enum Prop {
    /// The household's terminal.
    Terminal,
    /// The blade trainer.
    Blade,
    /// The marksman trainer.
    Marksman,
    /// The locker, which opens the household's store.
    Locker,
    /// The workbench.
    Workbench,
    /// The fabricator.
    Fabricator,
    /// The procurement board.
    Board,
    /// The survey trainer.
    Surveyor,
    /// The works foreman.
    Foreman,
    /// A planted field harvester.
    Harvester,
    /// A cache a cleared camp leaves.
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
pub fn bench_place(x: f64, z: f64, yaw: f32, spacing: f64) -> Vec<PropSite> {
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
pub fn pelagic_works() -> strata::registry::Namespaced {
    strata::registry::Namespaced::parse("veyra:buyer.pelagic_works")
        .expect("the buyer's id is well formed")
}

/// The two bench places of the slice: one seven metres behind and five to
/// the right of the combat staging spawn, facing it, and one on Port
/// Serein's Exchange square, four metres uphill of the `--at city` street
/// spawn in the settlement's frame and along its row from there
/// ([`city_place`]). Heights come later, from the streamed ground.
pub fn sites(
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

/// `OnEnter(InWorld)` on a run with no player of its own to place them from (a
/// server): the city's bench place is stood from the settlement alone, so a
/// workbench is there to be reached. The game binary does this from its
/// player's spawn, with the staging place beside it.
pub fn stand_city_props(
    mut commands: Commands,
    settlements: Option<Res<strata::sim::Settlements>>,
) {
    commands.insert_resource(sites(None, settlements.as_deref()));
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
pub const CITY_ROW_SHIFT: f64 = 1.0;

/// Where the city's bench place stands, its terminal's `(x, z)`, and its
/// heading. `None` without a settlement that has a layout.
pub fn city_place(settlements: Option<&strata::sim::Settlements>) -> Option<((f64, f64), f32)> {
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
pub fn camp_sites(camp: Option<strata::sim::quests::Camp>) -> Vec<PropSite> {
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
pub const BENCH_VIEW_M: f64 = 3.;

/// `--at bench`: where to stand and which way to look to see the city's
/// locker, workbench and fabricator from the front. Before the gap
/// between the workbench and the fabricator (three and four spacings along
/// the row, the locker two), looking at their fronts. `None` without a
/// settlement that has a layout.
pub fn bench_view(settlements: Option<&strata::sim::Settlements>) -> Option<((f64, f64), f32)> {
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
pub const TERMINAL_VIEW_M: f64 = 1.5;

/// `--at terminal`: where to stand and which way to look to be at the
/// city's terminal, facing it. `None` without a settlement that has a
/// layout.
pub fn terminal_view(settlements: Option<&strata::sim::Settlements>) -> Option<((f64, f64), f32)> {
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
pub fn board_view(settlements: Option<&strata::sim::Settlements>) -> Option<((f64, f64), f32)> {
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
pub fn survey_view(settlements: Option<&strata::sim::Settlements>) -> Option<((f64, f64), f32)> {
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
pub fn foreman_view(settlements: Option<&strata::sim::Settlements>) -> Option<((f64, f64), f32)> {
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
pub const CITY_SPACING_M: f64 = 3.0;

/// Ground height under `(x, z)` by the encounter's query: the terrain's
/// sample as a guess, then a ray down from thirty metres above it through
/// whatever has streamed (terrain, a settlement's platform). `None` until
/// something is there to hit.
pub fn ground_at(
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
/// the art is the window's business (`dress_props`).
pub fn place_props(
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
        ));
    }
    sites.0 = pending;
}
