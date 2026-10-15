//! The settlements' buildings on the client: every building of every placed
//! settlement's layout, from the baked files in the `generated://` source,
//! and the settlement's skyline, the whole city as one coarse building, for
//! when it is too far off for its buildings.
//!
//! A city is tens of thousands of piece entities under their visibility
//! ranges. Kept everywhere, they cost the woodland fixture, 36 km away,
//! 30 000 entities it never draws; so a settlement's buildings go in within
//! [`STREAM_M`] of its edge and come out past [`DESPAWN_M`], one building a
//! frame either way, so neither the approach nor the departure stalls a
//! frame. The server builds the same buildings' colliders from the same
//! layout (`strata_physics::settlement`), so what is drawn is what collides,
//! and the collision proxies `ashlar-bevy` spawns under each building are
//! despawned here: nothing on the client reads them.
//!
//! The terrain draws to its horizon, so a settlement also has a skyline
//! (ticket 0002, ADR 0009): one more baked file, present while the settlement
//! is inside the terrain's view distance, drawing nothing under 4 m and
//! colliding with nothing. The skyline and the buildings cannot hand over by
//! range: a range is measured to each entity's own origin, the skyline's the
//! city's middle and a cell's the cell's, so at any one distance some of the
//! city would be drawn twice and some not at all. So a settlement is in one
//! of two [`Detail`]s, switched for the whole city at once: far, only the
//! skyline draws; near, only the buildings. The buildings stream in hidden
//! before the switch and it waits until every one of them has spawned, so
//! the approach no longer shows them arriving one a frame.
//!
//! Inside the city a building's pieces are streamed too. A piece draws only
//! within its level's band (the finest to 60 m), but Bevy checks every piece
//! against its range every frame, and two in three of the city's pieces are
//! the finest level: from any one place almost all of them are checked only
//! to draw nothing, and the check is what a frame's CPU time scales with. So
//! the pieces are grouped by 40 m cell, level and band end, and a group
//! whose bands cannot reach the observer from anywhere in its cell is
//! `Disabled`, out of every query, until the observer comes back. The test
//! is conservative, so the per-piece range still decides exactly what draws.
//! While a settlement is far every group of it is out of play, in band or
//! not: its roots are hidden, so its pieces would draw nothing, and every
//! frame's visibility, range and extraction passes would still visit them
//! (a thousand pieces at 2 km, about a millisecond of the render thread).
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};

use ashlar_bevy::prelude::{
    AshlarBuilding, AshlarCollider, AshlarFailed, AshlarPiece, AshlarSpawned, MaterialLibraryAsset,
};
use bevy::camera::visibility::VisibilityRange;
use bevy::ecs::entity_disabling::Disabled;
use bevy::light::{CascadeShadowConfig, NotShadowCaster};
use bevy::prelude::*;
use strata::prelude::*;
use strata::sim::{Observers, PlacedSettlement, Settlements};
use strata::worldgen::SettlementLayout;

/// The material library the content step writes, in the `generated://` source.
const LIBRARY: &str = "generated://materials/library.materials.ron";

/// A settlement hands over from its skyline to its buildings when the
/// observer comes nearer its edge than this, metres.
///
/// A 1440-line view at the default 45 degrees spans about a metre a pixel
/// at 1 700 m. The skyline keeps nothing under 4 m across, so an element it
/// drops is up to four pixels across at the city's near edge and under
/// three at its far side, 700 m on; most of what it drops (windows, rails,
/// lamps, a metre or two) is one or two. Two things are more than a dot. A
/// long element is judged by its width, so the skyline keeps one at least
/// 1.5 m wide and 16 m long (`veyra_settlements::skyline::LINE_LENGTH`),
/// or the processor's pipelines would appear at the switch as lines three
/// pixels wide down the hill. And a row of thin members (the processor
/// hall's buttresses, its gantries, the towers' crowns) goes as a whole,
/// which reads as a change of texture rather than a thing appearing. At the
/// ticket's two pixels for 4 m the switch would be at 3.4 km, with every
/// building streamed in hidden for a kilometre and a half more.
const NEAR_M: f64 = 1_700.0;

/// And back to the skyline only past [`NEAR_M`] plus this, metres: an
/// observer walking along the line does not switch the whole city every few
/// steps.
const HYSTERESIS_M: f64 = 200.0;

/// Buildings stream in, hidden, once the observer is nearer the edge than
/// this, metres: 500 m before the switch, which is most of a minute at a
/// sprint (9 m/s) for thirty-odd buildings at one a frame and their files to
/// load, so the switch is not held back by a building still loading.
const STREAM_M: f64 = 2_200.0;

/// And stream out past this, metres: past where the settlement has gone back
/// to its skyline ([`NEAR_M`] plus [`HYSTERESIS_M`]), with room enough that
/// an observer wandering about the stream line does not churn them.
const DESPAWN_M: f64 = 2_700.0;

/// The skyline goes past the terrain's horizon plus this, metres, for the
/// same reason.
const SKYLINE_MARGIN_M: f64 = 2_000.0;

/// One settlement building's root, named for queries and the inspector.
#[derive(Component, Debug, Clone)]
#[allow(
    dead_code,
    reason = "read by the inspector and by queries while debugging"
)]
pub struct SettlementBuilding {
    /// The settlement's id.
    pub settlement: String,
    /// The baked file.
    pub file: String,
}

/// A settlement skyline's root. Not a [`SettlementBuilding`]: its pieces have
/// no bands to stream and it is never one of the buildings the switch waits
/// for.
#[derive(Component, Debug, Clone)]
#[allow(
    dead_code,
    reason = "read by the inspector and by queries while debugging"
)]
pub struct SettlementSkyline {
    /// The settlement's id.
    pub settlement: String,
}

/// Whether a settlement is drawn by its skyline or by its buildings.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum Detail {
    /// The skyline draws; buildings, if any are in, are hidden.
    #[default]
    Far,
    /// The buildings draw; the skyline is hidden.
    Near,
}

/// What of each settlement is in the world, by settlement index.
#[derive(Resource, Default)]
struct Present {
    /// Building roots, by settlement and building index.
    buildings: BTreeMap<(usize, usize), Entity>,
    /// Skyline roots.
    skylines: BTreeMap<usize, Entity>,
    /// Each settlement's detail; a settlement not listed is far.
    detail: BTreeMap<usize, Detail>,
    /// The settlements whose buildings draw this frame ([`buildings_shown`]),
    /// and so whose pieces may be in play; set by [`switch`] for
    /// [`stream_pieces`], which runs after it.
    drawn: BTreeSet<usize>,
}

impl Present {
    fn detail(&self, settlement: usize) -> Detail {
        self.detail.get(&settlement).copied().unwrap_or_default()
    }

    /// The building roots of one settlement.
    fn buildings_of(&self, settlement: usize) -> impl Iterator<Item = Entity> + '_ {
        self.buildings
            .range((settlement, 0)..=(settlement, usize::MAX))
            .map(|(_, entity)| *entity)
    }

    /// Whether one settlement's skyline can draw it, from its root's
    /// spawned and failed markers.
    fn skyline_ready(&self, settlement: usize, roots: &Query<Root>) -> bool {
        skyline_ready(
            self.skylines
                .get(&settlement)
                .and_then(|root| roots.get(*root).ok()),
        )
    }
}

/// What a building or skyline root says of its spawning: spawned, failed.
type Root = (Has<AshlarSpawned>, Has<AshlarFailed>);

/// The settlement's detail after this frame: near once the observer is
/// inside [`NEAR_M`] of its edge and every building has spawned its pieces,
/// far again past [`NEAR_M`] plus [`HYSTERESIS_M`].
fn next_detail(detail: Detail, edge: f64, all_spawned: bool) -> Detail {
    match detail {
        Detail::Far if edge < NEAR_M && all_spawned => Detail::Near,
        Detail::Near if edge > NEAR_M + HYSTERESIS_M => Detail::Far,
        unchanged => unchanged,
    }
}

/// Whether a settlement's buildings draw: when it is near, or when it has
/// no skyline to stand in for them. A skyline the layout does not name, or
/// that is not spawned yet, or whose file failed, draws nothing, so hiding
/// the buildings for it would leave the city blank where they are in.
fn buildings_shown(detail: Detail, skyline_ready: bool) -> bool {
    detail == Detail::Near || !skyline_ready
}

/// Whether a settlement's skyline draws: exactly when its buildings do not.
///
/// Not whenever the settlement is far: ashlar spawns the skyline's pieces
/// from its own system, in no order against [`switch`], so a skyline root
/// shown while it was still loading would draw its pieces on the frame they
/// arrive, before `switch` had seen it ready and hidden the buildings.
/// Hidden until it is ready, it is shown on the same frame the buildings
/// are hidden and taken out of play.
fn skyline_shown(detail: Detail, skyline_ready: bool) -> bool {
    !buildings_shown(detail, skyline_ready)
}

/// Whether a skyline root has spawned its pieces and can draw the city: it
/// exists, and it spawned rather than failed.
fn skyline_ready(root: Option<(bool, bool)>) -> bool {
    root.is_some_and(|(spawned, failed)| spawned && !failed)
}

/// `Inherited` for shown, `Hidden` for not: a root never forces its pieces
/// visible.
fn shown(show: bool) -> Visibility {
    if show {
        Visibility::Inherited
    } else {
        Visibility::Hidden
    }
}

/// The rectangle bounding a layout's footprints in its frame,
/// `[min_x, min_z, max_x, max_z]`, or `None` for a layout with no buildings.
fn extent(layout: &SettlementLayout) -> Option<[f64; 4]> {
    layout.buildings.iter().map(|b| b.footprint).reduce(|a, b| {
        [
            a[0].min(b[0]),
            a[1].min(b[1]),
            a[2].max(b[2]),
            a[3].max(b[3]),
        ]
    })
}

/// How far a point of the frame `(x, z)` is from a rectangle of it, zero
/// inside.
fn distance_to(rect: [f64; 4], at: [f64; 2]) -> f64 {
    let dx = (rect[0] - at[0]).max(at[0] - rect[2]).max(0.0);
    let dz = (rect[1] - at[1]).max(at[1] - rect[3]).max(0.0);
    dx.hypot(dz)
}

/// How far the observer is from the settlement's edge on the ground: from
/// its buildings' bounding rectangle, since a city 700 m deep is reached a
/// long way before its centre is.
fn edge_distance(settlement: &PlacedSettlement, layout: &SettlementLayout, eye: WorldPos) -> f64 {
    edge_in_frame(layout, settlement.site.to_local_xz([eye.x(), eye.z()]))
}

/// [`edge_distance`] from a point of the frame; a layout with no buildings
/// has no edge, so it is measured to the frame's origin.
fn edge_in_frame(layout: &SettlementLayout, local: [f64; 2]) -> f64 {
    extent(layout).map_or_else(|| local[0].hypot(local[1]), |rect| distance_to(rect, local))
}

/// The side of a streaming cell, metres: small enough that a cell past a
/// band's edge is not held in play by its near corner, large enough that the
/// city is a few thousand groups.
const CELL: f32 = 40.0;

/// How far past its band a group comes into play, and how much further it
/// must be before it leaves again, metres: brought in a moment before its
/// first piece is in range, and not switched every frame by an observer
/// standing on the boundary.
const EARLY: f32 = 8.0;
const LATE: f32 = 16.0;

/// How finely a group tells its pieces' band ends apart, metres.
///
/// Not the band itself: a piece handing over to a concatenated level has its
/// band's end pushed out by its own distance to its cell's centre, so every
/// piece of that level ends somewhere else and keying on the exact end
/// would make a group of nearly every piece. Nor the level alone: a level
/// holds pieces that end at their own level's boundary and pieces carried
/// on unchanged to the next, and a group is in play to its farthest
/// member's end, so the finest level's pieces, which end at 60 m, would be
/// held in play to 300 m by the carried ones beside them. So the end is
/// bucketed: a group reaches at most this much past any member's end.
const END_STEP: f32 = 32.0;

/// A group's key: its building, its cell, its pieces' level, and the bucket
/// of their band's end ([`END_STEP`]).
type GroupKey = (Entity, i32, i32, usize, i32);

/// The key of a piece of `root` standing at `at` in the building's frame,
/// first drawn at `level`, whose band ends at `end`.
#[allow(
    clippy::cast_possible_truncation,
    reason = "cells of a city a kilometre across, and band ends in steps of tens of metres"
)]
fn group_key(root: Entity, at: Vec3, level: usize, end: f32) -> GroupKey {
    // The last level draws to any distance, `f32::MAX`.
    let bucket = if end < f32::MAX / 2.0 {
        (end / END_STEP).floor() as i32
    } else {
        i32::MAX
    };
    (
        root,
        (at.x / CELL).floor() as i32,
        (at.z / CELL).floor() as i32,
        level,
        bucket,
    )
}

/// The pieces of one building, one level, one cell and one bucket of band
/// ends.
struct Group {
    key: GroupKey,
    root: Entity,
    /// The bounds of the pieces' origins in the building's frame: a range is
    /// measured to a piece's origin.
    low: Vec3,
    high: Vec3,
    /// The nearest start and the farthest end of its members' bands, so the
    /// group is in play wherever any of them could be.
    near: f32,
    far: f32,
    members: Vec<Entity>,
    enabled: bool,
    /// Whether its pieces cast the sun's shadow ([`casts`]).
    casts: bool,
}

// Far buildings cast no shadow (lighting plan phase 6, the profile's 4.3).
/// How much farther than the buildings' cascade reach a group still casts,
/// when no perspective camera says ([`view_slack`]): the corner of a 16:9
/// frame at 45 degrees.
const CASTER_VIEW_SLACK: f32 = 1.31;
/// The group's bounds are its pieces' origins, so this covers the half of a
/// far level's 80 m cell a piece reaches past its origin.
const CASTER_CELL_SLACK: f32 = 40.0;

/// How much farther than a depth along the view the frame's corner is.
///
/// A cascade's bound is a depth along the view, a group's distance is
/// measured from the camera, and at the corner of the frame a depth is the
/// cosine of the corner's angle off the axis times the distance: 0.76 at 45
/// degrees and 16:9, 0.38 at the settings' 100. The reach over that cosine
/// is where no piece within the bound's depth is ever taken out.
fn view_slack(projection: &Projection) -> f32 {
    match projection {
        Projection::Perspective(p) => {
            let tan_v = (p.fov * 0.5).tan();
            let tan_h = tan_v * p.aspect_ratio;
            (1.0 + tan_v * tan_v + tan_h * tan_h).sqrt()
        }
        _ => CASTER_VIEW_SLACK,
    }
}

/// How far from the camera a group still casts, metres, when buildings cast
/// to `reach` metres of view depth (`None`: to every cascade).
fn caster_limit(reach: Option<f32>, slack: f32) -> Option<f32> {
    reach.map(|reach| reach * slack + CASTER_CELL_SLACK)
}

/// And how much nearer than its reach a group that stopped casting must come
/// before it casts again, metres: an observer walking along the line does
/// not switch it every few steps.
const CASTER_HYSTERESIS: f32 = 16.0;

/// Whether `group` casts the sun's shadow seen from the camera at `eye`, in
/// the building's frame, within `limit` metres of it ([`caster_limit`];
/// `None`: at every distance). Past it the cascades a piece would cast into
/// are metres to a texel, and the far levels there were most of the city's
/// caster draws (`docs/perf/profile-2026-10-03.md`, 4.3).
fn casts(group: &Group, eye: Vec3, limit: Option<f32>) -> bool {
    let Some(limit) = limit else {
        return true;
    };
    let nearest = eye.clamp(group.low, group.high).distance(eye);
    if group.casts {
        nearest < limit
    } else {
        nearest < limit - CASTER_HYSTERESIS
    }
}

/// How far buildings cast, from the sky settings and the sun's cascades.
fn building_reach(
    sky: Option<&strata::atmosphere::SkySettings>,
    suns: &Query<&CascadeShadowConfig, With<strata::atmosphere::Sun>>,
) -> Option<f32> {
    let sky = sky?;
    sky.building_shadow_reach(suns.iter().next()?)
}

/// The cameras whose views the sun's cascades are fitted to.
type ShadowCameras<'w, 's> = Query<
    'w,
    's,
    (
        &'static Camera,
        &'static GlobalTransform,
        &'static Projection,
    ),
    (With<Camera3d>, Without<strata::terrain::OffWorld>),
>;

/// The active 3D camera's position in render space and its frame's slack
/// ([`view_slack`]), if there is one.
fn shadow_view(cameras: &ShadowCameras) -> Option<(Vec3, f32)> {
    cameras
        .iter()
        .find(|(camera, _, _)| camera.is_active)
        .map(|(_, transform, projection)| (transform.translation(), view_slack(projection)))
}

/// Every group of every building in the world.
#[derive(Resource, Default)]
struct Groups {
    groups: Vec<Group>,
    index: HashMap<GroupKey, usize>,
}

impl Groups {
    /// Adds a piece standing at `at` with the band `near..far` to its group,
    /// and says whether that group is in play: a piece joining a group
    /// already out of play goes out with it.
    fn add(&mut self, key: GroupKey, piece: Entity, at: Vec3, near: f32, far: f32) -> bool {
        let groups = &mut self.groups;
        let index = *self.index.entry(key).or_insert_with(|| {
            groups.push(Group {
                key,
                root: key.0,
                low: at,
                high: at,
                near,
                far,
                members: Vec::new(),
                enabled: true,
                casts: true,
            });
            groups.len() - 1
        });
        let group = &mut groups[index];
        group.low = group.low.min(at);
        group.high = group.high.max(at);
        group.near = group.near.min(near);
        group.far = group.far.max(far);
        group.members.push(piece);
        group.enabled
    }

    /// Whether the group under `key` casts; a group not yet made does.
    fn casts(&self, key: &GroupKey) -> bool {
        self.index.get(key).is_none_or(|&i| self.groups[i].casts)
    }
}

pub(super) fn install(app: &mut App) {
    // The order is what keeps the handover to one frame. `switch` writes the
    // roots' `Visibility` directly and records which settlements draw;
    // `group_pieces` then files the pieces spawned since, and its `Disabled`
    // inserts are applied before `stream_pieces` runs (a chain puts an
    // `ApplyDeferred` between systems with commands). `stream_pieces` reads
    // what `switch` recorded this frame, so on the frame a city goes near it
    // takes `Disabled` off every group in band and inserts their `Transform`
    // and `Visibility` again, and on the frame it goes far it disables them.
    // Those commands are applied before `Update` ends, and transform and
    // visibility propagation run in `PostUpdate`: the roots shown, the
    // skyline hidden and the pieces back in play all reach the same frame's
    // propagation and extraction, so no frame draws the city twice or not
    // at all.
    app.init_resource::<Present>()
        .init_resource::<Groups>()
        .add_systems(
            Update,
            (
                stream,
                // Market fog (lighting plan phase 2c): reads the stalls'
                // counter proxies before `drop_proxies` takes them.
                market_counters,
                drop_proxies,
                switch,
                group_pieces,
                stream_pieces,
                // Far buildings cast no shadow (lighting plan phase 6).
                shade_skylines,
                census,
            )
                .chain()
                .run_if(in_state(AppState::InWorld)),
        )
        .add_systems(
            Update,
            (
                night_lights::<StandardMaterial>,
                night_lights::<ashlar_bevy::anti_tiling::AntiTilingMaterial>,
            ),
        )
        .init_resource::<LampPool>()
        .add_systems(Update, lamp_lights.run_if(in_state(AppState::InWorld)))
        .add_systems(OnExit(AppState::InWorld), clear)
        // Market fog (lighting plan phase 2c).
        .init_resource::<MarketFog>()
        .add_systems(
            Update,
            market_fog.after(census).run_if(in_state(AppState::InWorld)),
        )
        .add_systems(OnExit(AppState::InWorld), clear_market_fog);
}

/// Spawns and despawns each settlement's skyline and buildings by the
/// observer's distance to its edge.
#[allow(
    clippy::too_many_arguments,
    reason = "a Bevy system's parameters are its dependencies"
)]
fn stream(
    mut commands: Commands,
    settlements: Option<Res<Settlements>>,
    observers: Res<Observers>,
    server: Res<AssetServer>,
    origin: Res<RenderOrigin>,
    config: Option<Res<StreamConfig>>,
    roots: Query<Root>,
    mut present: ResMut<Present>,
    mut library: Local<Option<Handle<MaterialLibraryAsset>>>,
) {
    let (Some(settlements), Some(eye)) = (settlements, observers.primary) else {
        return;
    };
    let horizon = config.map_or_else(
        || StreamConfig::default().view_distance,
        |config| config.view_distance,
    );
    let mut spawned_one = false;
    for (s, settlement) in settlements.0.iter().enumerate() {
        let Some(layout) = &settlement.layout else {
            continue;
        };
        let edge = edge_distance(settlement, layout, eye);
        let detail = present.detail(s);

        if let Some(file) = &layout.skyline {
            match present.skylines.get(&s).copied() {
                None if edge < horizon => {
                    let materials = library.get_or_insert_with(|| server.load(LIBRARY)).clone();
                    // The skyline's vertices are in the settlement's frame,
                    // heights world heights: it stands at the frame's origin,
                    // turned by the frame's yaw, as a building with no offset
                    // and no yaw of its own would.
                    let (at, rotation) = placement(settlement, [0.0; 3], 0.0);
                    let entity = commands
                        .spawn((
                            Name::new(format!("{}/skyline", layout.id)),
                            SettlementSkyline {
                                settlement: layout.id.clone(),
                            },
                            AshlarBuilding {
                                building: server.load(format!("generated://{file}")),
                                materials,
                            },
                            WorldPosition(at),
                            Transform::from_translation(origin.to_render(at))
                                .with_rotation(rotation),
                            // Not ready until its pieces are in.
                            shown(skyline_shown(detail, false)),
                        ))
                        .id();
                    present.skylines.insert(s, entity);
                }
                Some(entity) if edge > horizon + SKYLINE_MARGIN_M => {
                    commands.entity(entity).despawn();
                    present.skylines.remove(&s);
                }
                _ => {}
            }
        }

        if edge > DESPAWN_M {
            // One building out a frame.
            let first = present
                .buildings
                .range((s, 0)..=(s, usize::MAX))
                .next()
                .map(|(key, _)| *key);
            if let Some(entity) = first.and_then(|key| present.buildings.remove(&key)) {
                commands.entity(entity).despawn();
            }
            continue;
        }
        if edge >= STREAM_M || spawned_one {
            continue;
        }
        // One building in a frame, over every settlement.
        let Some((b, building)) = layout
            .buildings
            .iter()
            .enumerate()
            .find(|(b, _)| !present.buildings.contains_key(&(s, *b)))
        else {
            continue;
        };
        let materials = library.get_or_insert_with(|| server.load(LIBRARY)).clone();
        let (at, rotation) = placement(settlement, building.position, building.yaw);
        let entity = commands
            .spawn((
                Name::new(format!("{}/{}", layout.id, building.file)),
                SettlementBuilding {
                    settlement: layout.id.clone(),
                    file: building.file.clone(),
                },
                AshlarBuilding {
                    building: server.load(format!("generated://{}", building.file)),
                    materials,
                },
                WorldPosition(at),
                Transform::from_translation(origin.to_render(at)).with_rotation(rotation),
                shown(buildings_shown(detail, present.skyline_ready(s, &roots))),
            ))
            .id();
        present.buildings.insert((s, b), entity);
        spawned_one = true;
    }
}

/// Where a baked file standing at `position` with `yaw` in a settlement's
/// frame stands in the world, and how it is turned: the one placement the
/// buildings and the skyline are spawned by.
fn placement(settlement: &PlacedSettlement, position: [f64; 3], yaw: f64) -> (WorldPos, Quat) {
    #[allow(clippy::cast_possible_truncation, reason = "a yaw in radians")]
    let rotation = Quat::from_rotation_y(settlement.world_yaw(yaw) as f32);
    (settlement.to_world(position), rotation)
}

/// Whether a root is done spawning: spawned, or failed, so one bad file
/// cannot hold a city in its skyline for ever.
fn done(root: Option<(bool, bool)>) -> bool {
    root.is_some_and(|(spawned, failed)| spawned || failed)
}

/// Switches each settlement between its skyline and its buildings, and
/// keeps the roots' visibility to match.
fn switch(
    settlements: Option<Res<Settlements>>,
    observers: Res<Observers>,
    mut present: ResMut<Present>,
    roots: Query<Root>,
    mut visibility: Query<&mut Visibility>,
) {
    let (Some(settlements), Some(eye)) = (settlements, observers.primary) else {
        return;
    };
    let present = &mut *present;
    for (s, settlement) in settlements.0.iter().enumerate() {
        let Some(layout) = &settlement.layout else {
            continue;
        };
        let edge = edge_distance(settlement, layout, eye);
        let all_spawned = present
            .buildings_of(s)
            .filter(|root| done(roots.get(*root).ok()))
            .count()
            == layout.buildings.len();
        let before = present.detail(s);
        let after = next_detail(before, edge, all_spawned);
        if after != before {
            info!(
                "settlement {}: {before:?} to {after:?}, {edge:.0} m from its edge",
                layout.id
            );
            present.detail.insert(s, after);
        }
        let ready = present.skyline_ready(s, &roots);
        let drawn = buildings_shown(after, ready);
        if drawn {
            present.drawn.insert(s);
        } else {
            present.drawn.remove(&s);
        }
        let buildings = shown(drawn);
        for root in present.buildings_of(s) {
            if let Ok(mut visibility) = visibility.get_mut(root) {
                visibility.set_if_neq(buildings);
            }
        }
        if let Some(root) = present.skylines.get(&s)
            && let Ok(mut visibility) = visibility.get_mut(*root)
        {
            visibility.set_if_neq(shown(skyline_shown(after, ready)));
        }
    }
}

/// Logs, once every settlement near enough has its buildings and skyline
/// spawned, how many entities the city spawned and of what kind, and which
/// detail each settlement is drawn at: the numbers a frame's CPU work scales
/// with.
#[allow(
    clippy::too_many_arguments,
    reason = "a Bevy system's parameters are its dependencies"
)]
fn census(
    present: Res<Present>,
    settlements: Option<Res<Settlements>>,
    roots: Query<Root>,
    groups: Res<Groups>,
    buildings: Query<(), With<SettlementBuilding>>,
    skylines: Query<(), With<SettlementSkyline>>,
    pieces: Query<
        (
            &AshlarPiece,
            &ChildOf,
            Option<&VisibilityRange>,
            &GlobalTransform,
            &InheritedVisibility,
            Has<Disabled>,
        ),
        Allow<Disabled>,
    >,
    observers: Res<Observers>,
    origin: Res<RenderOrigin>,
    mut logged: Local<bool>,
    mut frames: Local<u32>,
) {
    let Some(settlements) = settlements else {
        return;
    };
    if *logged || settlements.0.iter().all(|s| s.layout.is_none()) {
        return;
    }
    // Every settlement with all of its buildings in or none, and everything
    // in spawned.
    let settled = settlements.0.iter().enumerate().all(|(s, settlement)| {
        let Some(layout) = &settlement.layout else {
            return true;
        };
        let count = present.buildings_of(s).count();
        (count == 0 || count == layout.buildings.len())
            && present
                .buildings_of(s)
                .chain(present.skylines.get(&s).copied())
                .all(|root| done(roots.get(root).ok()))
    });
    if !settled {
        *frames = 0;
        return;
    }
    // A second for the groups and the switch to have settled too.
    *frames += 1;
    if *frames < 60 {
        return;
    }
    *logged = true;
    let eye = observers.primary.map(|eye| origin.to_render(eye));
    let mut levels = [0_usize; 4];
    // Those whose band holds the observer: what every view's culling,
    // extraction and batching is paid for, in the frustum or out of it.
    let mut in_band = [0_usize; 4];
    // Of those, the ones under a hidden root, which are checked and not drawn.
    let mut hidden = 0;
    let mut disabled = 0;
    let (mut skyline, mut skyline_drawn, mut other) = (0, 0, 0);
    for (piece, parent, range, transform, visible, off) in &pieces {
        if skylines.contains(parent.parent()) {
            skyline += 1;
            skyline_drawn += usize::from(visible.get());
            continue;
        }
        if !buildings.contains(parent.parent()) {
            other += 1;
            continue;
        }
        let level = piece.level.min(3);
        levels[level] += 1;
        disabled += usize::from(off);
        let reached = !off
            && eye.is_none_or(|eye| {
                range.is_none_or(|range| {
                    range.is_visible_at_all(eye.distance(transform.translation()))
                })
            });
        in_band[level] += usize::from(reached);
        hidden += usize::from(reached && !visible.get());
    }
    let places: Vec<String> = settlements
        .0
        .iter()
        .enumerate()
        .filter_map(|(s, settlement)| {
            let layout = settlement.layout.as_ref()?;
            let edge = observers
                .primary
                .map_or(f64::NAN, |eye| edge_distance(settlement, layout, eye));
            Some(format!(
                "{} {:?} at {edge:.0} m from its edge, {} of {} buildings in, skyline {}",
                layout.id,
                present.detail(s),
                present.buildings_of(s).count(),
                layout.buildings.len(),
                if present.skylines.contains_key(&s) {
                    "in"
                } else {
                    "out"
                },
            ))
        })
        .collect();
    let in_play = levels.iter().sum::<usize>() - disabled;
    let groups_in_play = groups.groups.iter().filter(|g| g.enabled).count();
    info!(
        "settlement census: {}; pieces by level {levels:?}, in band here {in_band:?} \
         ({hidden} of them under a hidden root), {in_play} in play and {disabled} out of \
         play here, in {groups_in_play} of {} groups; skyline pieces {skyline} \
         ({skyline_drawn} shown); {other} pieces of other buildings",
        places.join("; "),
        groups.groups.len(),
    );
}

/// The collision proxies under a building: the physics builds its own from
/// the baked file, so on the client they are only entities to carry. A
/// skyline should carry none (the content step checks its file); one that
/// does is said so once.
fn drop_proxies(
    mut commands: Commands,
    proxies: Query<(Entity, Option<&ChildOf>), Added<AshlarCollider>>,
    skylines: Query<(), With<SettlementSkyline>>,
    mut warned: Local<bool>,
) {
    for (proxy, parent) in &proxies {
        if !*warned && parent.is_some_and(|parent| skylines.contains(parent.parent())) {
            warn!("a settlement skyline carries collision proxies; its bake should have none");
            *warned = true;
        }
        commands.entity(proxy).despawn();
    }
}

/// Groups each newly spawned settlement piece by building, cell, level and
/// band end ([`group_key`]). A piece with no band draws at every distance
/// and is never streamed.
fn group_pieces(
    mut commands: Commands,
    mut groups: ResMut<Groups>,
    buildings: Query<(), With<SettlementBuilding>>,
    pieces: Query<
        (Entity, &ChildOf, &Transform, &AshlarPiece, &VisibilityRange),
        Added<AshlarPiece>,
    >,
) {
    for (entity, parent, transform, piece, range) in &pieces {
        let root = parent.parent();
        if !buildings.contains(root) {
            continue;
        }
        let at = transform.translation;
        let key = group_key(root, at, piece.level, range.end_margin.end);
        let in_play = groups.add(
            key,
            entity,
            at,
            range.start_margin.start,
            range.end_margin.end,
        );
        if !in_play {
            commands.entity(entity).try_insert(Disabled);
        }
        // Far buildings cast no shadow (lighting plan phase 6): a piece
        // joining a group that does not cast goes without, as the group's
        // other members do.
        if !groups.casts(&key) {
            commands.entity(entity).try_insert(NotShadowCaster);
        }
    }
}

/// Whether `group` should be in play: its building draws (its settlement is
/// near, or has no skyline to stand in for it), and one of its pieces could
/// be in band seen from `eye`, in the building's frame. A far city's pieces
/// are under a hidden root and draw nothing wherever the observer is.
fn should_play(drawn: bool, group: &Group, eye: Vec3) -> bool {
    drawn && in_play(group, eye)
}

/// Whether any piece of `group` could be in its band seen from `eye`, in the
/// building's frame, with the slack that keeps a group from switching every
/// frame.
fn in_play(group: &Group, eye: Vec3) -> bool {
    let slack = if group.enabled { LATE } else { EARLY };
    let nearest = eye.clamp(group.low, group.high).distance(eye);
    let farthest = (eye - group.low)
        .abs()
        .max((eye - group.high).abs())
        .length();
    farthest + slack >= group.near && nearest - slack < group.far
}

/// Takes groups out of play and brings them back as the observer moves, and
/// as the settlement goes far and near ([`should_play`]).
///
/// A group comes back with its `Transform` and `Visibility` inserted again:
/// while it was disabled no propagation reached it, and the render origin may
/// have moved, so what it had is stale; inserting them marks both changed.
/// A piece's own `Visibility` is `Inherited`, so marked changed it takes its
/// root's again, shown on the frame the city goes near (see [`install`] for
/// why that is the same frame).
///
/// Every piece of a far city is disabled, and ashlar-bevy's reload of a
/// modified file despawns only the children its default-filtered query sees,
/// so a hot reload would leave the disabled set behind to come back beside
/// the new one. The game enables no file watcher and reloads nothing, so
/// nothing reaches that path until ashlar's query allows `Disabled`.
///
/// It also takes a group's shadow away past the cascades buildings cast
/// into and gives it back as the observer comes near ([`casts`]), in or out
/// of play: a group's members carry `NotShadowCaster` exactly while it does
/// not cast.
#[allow(
    clippy::too_many_arguments,
    reason = "a Bevy system's parameters are its dependencies"
)]
fn stream_pieces(
    mut commands: Commands,
    mut groups: ResMut<Groups>,
    observers: Res<Observers>,
    origin: Res<RenderOrigin>,
    present: Res<Present>,
    sky: Option<Res<strata::atmosphere::SkySettings>>,
    suns: Query<&CascadeShadowConfig, With<strata::atmosphere::Sun>>,
    cameras: ShadowCameras,
    roots: Query<&GlobalTransform, With<SettlementBuilding>>,
    pieces: Query<(&Transform, &Visibility), Allow<Disabled>>,
) {
    let Some(eye) = observers.primary else {
        return;
    };
    let eye = origin.to_render(eye);
    // The cascades are fitted to the camera's frustum, which trails the
    // observer and may be wider than the default.
    let (camera, slack) = shadow_view(&cameras).unwrap_or((eye, CASTER_VIEW_SLACK));
    let limit = caster_limit(building_reach(sky.as_deref(), &suns), slack);
    let drawn: HashSet<Entity> = present
        .buildings
        .iter()
        .filter(|((s, _), _)| present.drawn.contains(s))
        .map(|(_, root)| *root)
        .collect();
    let groups = &mut *groups;
    // A building that left took its pieces with it.
    let before = groups.groups.len();
    groups.groups.retain(|g| roots.contains(g.root));
    if groups.groups.len() != before {
        groups.index = groups
            .groups
            .iter()
            .enumerate()
            .map(|(i, g)| (g.key, i))
            .collect();
    }
    let mut frames: HashMap<Entity, (Vec3, Vec3)> = HashMap::new();
    for group in &mut groups.groups {
        let Some((local, local_camera)) = frames.get(&group.root).copied().or_else(|| {
            let to_local = roots.get(group.root).ok()?.affine().inverse();
            let local = (
                to_local.transform_point3(eye),
                to_local.transform_point3(camera),
            );
            frames.insert(group.root, local);
            Some(local)
        }) else {
            continue;
        };
        let cast = casts(group, local_camera, limit);
        if cast != group.casts {
            group.casts = cast;
            for &member in &group.members {
                if cast {
                    commands.entity(member).try_remove::<NotShadowCaster>();
                } else {
                    commands.entity(member).try_insert(NotShadowCaster);
                }
            }
        }
        let wanted = should_play(drawn.contains(&group.root), group, local);
        if wanted == group.enabled {
            continue;
        }
        group.enabled = wanted;
        for &member in &group.members {
            if wanted {
                if let Ok((transform, visibility)) = pieces.get(member) {
                    commands
                        .entity(member)
                        .try_remove::<Disabled>()
                        .try_insert((*transform, *visibility));
                }
            } else {
                commands.entity(member).try_insert(Disabled);
            }
        }
    }
}

// Far buildings cast no shadow (lighting plan phase 6).
/// Keeps the skylines' pieces out of the sun's cascades while buildings
/// cast only into the near ones. A skyline draws only while its city's edge
/// is past [`NEAR_M`], beyond the reach of any cascade but the last, so
/// whether it casts is whether buildings cast that far. Its pieces have no
/// band and are never grouped, so they are visited here, a few hundred a
/// skyline, and written only when they differ.
fn shade_skylines(
    mut commands: Commands,
    sky: Option<Res<strata::atmosphere::SkySettings>>,
    suns: Query<&CascadeShadowConfig, With<strata::atmosphere::Sun>>,
    cameras: ShadowCameras,
    skylines: Query<&Children, With<SettlementSkyline>>,
    pieces: Query<Has<NotShadowCaster>, (With<AshlarPiece>, Allow<Disabled>)>,
) {
    #[allow(
        clippy::cast_possible_truncation,
        reason = "a settlement's switch distance, under two kilometres"
    )]
    let near = NEAR_M as f32;
    let slack = shadow_view(&cameras).map_or(CASTER_VIEW_SLACK, |(_, slack)| slack);
    let cast =
        caster_limit(building_reach(sky.as_deref(), &suns), slack).is_none_or(|limit| limit > near);
    for children in &skylines {
        for &piece in children {
            let Ok(shadowless) = pieces.get(piece) else {
                continue;
            };
            if cast && shadowless {
                commands.entity(piece).try_remove::<NotShadowCaster>();
            } else if !cast && !shadowless {
                commands.entity(piece).try_insert(NotShadowCaster);
            }
        }
    }
}

/// How bright a building's lights are by day, as a share of their night
/// brightness: a warning light still shows against the sky, a lit window
/// does not read as one, and nothing reaches the camera's bloom threshold
/// of 2 (lighting plan phase 5b). The night's windows were raised eightfold
/// to cross that threshold (`veyra_settlements::materials`), so this was
/// halved from 0.04: the brightest light, the beacon at 24, is 0.48 by day,
/// and in the dusk street at 0.70 no window reads as lit.
const DAY_GLOW: f32 = 0.02;

/// A material whose emission [`night_lights`] scales: a plain piece's, or
/// an anti-tiled piece's, whose emission lives on its base.
trait Glowing: Material {
    fn glow(&self) -> LinearRgba;
    fn set_glow(&mut self, glow: LinearRgba);
}

impl Glowing for StandardMaterial {
    fn glow(&self) -> LinearRgba {
        self.emissive
    }
    fn set_glow(&mut self, glow: LinearRgba) {
        self.emissive = glow;
    }
}

impl Glowing for ashlar_bevy::anti_tiling::AntiTilingMaterial {
    fn glow(&self) -> LinearRgba {
        self.base.emissive
    }
    fn set_glow(&mut self, glow: LinearRgba) {
        self.base.emissive = glow;
    }
}

/// Dims and lights the buildings' emissive materials with the night.
///
/// Bevy draws a material's emission without the camera's exposure, so a
/// window lit enough for the night would glow at noon. Each emissive
/// material's baked value is its brightness at full night; this scales it
/// by how dark it is, and only when that has moved enough to be seen, since
/// touching a material has it prepared again.
fn night_lights<M: Glowing>(
    night: Option<Res<strata::atmosphere::Night>>,
    mut materials: ResMut<Assets<M>>,
    mut events: MessageReader<AssetEvent<M>>,
    mut base: Local<HashMap<AssetId<M>, LinearRgba>>,
    mut applied: Local<Option<f32>>,
) {
    let Some(night) = night else {
        return;
    };
    let factor = DAY_GLOW + (1.0 - DAY_GLOW) * night.darkness;
    let mut fresh = Vec::new();
    for event in events.read() {
        if let AssetEvent::Added { id } = event {
            fresh.push(*id);
        }
    }
    for id in fresh {
        if let Some(material) = materials.get(id)
            && material.glow() != LinearRgba::BLACK
            && !base.contains_key(&id)
        {
            base.insert(id, material.glow());
            if let Some(mut material) = materials.get_mut(id) {
                material.set_glow(base[&id] * factor);
            }
        }
    }
    if applied.is_some_and(|a| (a - factor).abs() < 0.01) {
        return;
    }
    *applied = Some(factor);
    base.retain(|id, _| materials.contains(*id));
    for (id, emissive) in &*base {
        if let Some(mut material) = materials.get_mut(*id) {
            material.set_glow(*emissive * factor);
        }
    }
}

/// How many of the city's lamps shine at once: the nearest to the observer.
/// Bevy clusters its lights, but each costs every fragment near it, and a
/// lamp further than a street or two lights nothing a player sees.
const LAMPS: usize = 48;

/// The point lights lamps are drawn with, reused for whichever are nearest.
#[derive(Resource, Default)]
struct LampPool {
    lights: Vec<Entity>,
    /// Where the observer was when they were last assigned.
    at: Option<WorldPos>,
    darkness: f32,
}

/// Puts the pool's point lights on the lamps nearest the observer, at night.
///
/// Reassigned when the observer has moved a few metres or the night has
/// changed; by day they are all off.
fn lamp_lights(
    mut commands: Commands,
    mut pool: ResMut<LampPool>,
    settlements: Option<Res<Settlements>>,
    observers: Res<Observers>,
    origin: Res<RenderOrigin>,
    night: Option<Res<strata::atmosphere::Night>>,
    mut lights: Query<(&mut PointLight, &mut WorldPosition, &mut Visibility)>,
) {
    let (Some(settlements), Some(eye), Some(night)) = (settlements, observers.primary, night)
    else {
        return;
    };
    if pool.lights.is_empty() {
        for _ in 0..LAMPS {
            let entity = commands
                .spawn((
                    Name::new("lamp light"),
                    PointLight {
                        intensity: 0.0,
                        shadow_maps_enabled: false,
                        ..default()
                    },
                    // Glows in the market's air at night (lighting plan
                    // phase 2c); off the volumes it costs nothing.
                    bevy::light::VolumetricLight,
                    WorldPosition(eye),
                    Transform::from_translation(origin.to_render(eye)),
                    Visibility::Hidden,
                ))
                .id();
            pool.lights.push(entity);
        }
        return;
    }
    let moved = pool.at.is_none_or(|at| {
        let (dx, dz) = (at.x() - eye.x(), at.z() - eye.z());
        dx * dx + dz * dz > 16.0
    });
    if !moved && (pool.darkness - night.darkness).abs() < 0.02 {
        return;
    }
    pool.at = Some(eye);
    pool.darkness = night.darkness;
    let mut nearest: Vec<(f64, WorldPos, &strata::worldgen::LayoutLight)> = Vec::new();
    if night.darkness > 0.05 {
        for settlement in &settlements.0 {
            let Some(layout) = &settlement.layout else {
                continue;
            };
            for light in &layout.lights {
                let at = settlement.to_world(light.position);
                let (dx, dy, dz) = (at.x() - eye.x(), at.y() - eye.y(), at.z() - eye.z());
                let d = dx * dx + dy * dy + dz * dz;
                if d < 250.0 * 250.0 {
                    nearest.push((d, at, light));
                }
            }
        }
        nearest.sort_by(|a, b| a.0.total_cmp(&b.0));
        nearest.truncate(LAMPS);
    }
    for (i, entity) in pool.lights.iter().enumerate() {
        let Ok((mut light, mut position, mut visibility)) = lights.get_mut(*entity) else {
            continue;
        };
        if let Some((_, at, lamp)) = nearest.get(i) {
            let [r, g, b] = lamp.color;
            light.color = Color::linear_rgb(r, g, b);
            light.intensity = lamp.lumens * night.darkness;
            light.range = lamp.range;
            position.0 = *at;
            *visibility = Visibility::Inherited;
        } else if *visibility != Visibility::Hidden {
            light.intensity = 0.0;
            *visibility = Visibility::Hidden;
        }
    }
}

// Market fog (lighting plan phase 2c): dusty air under the Exchange's stall
// canopies, as Bevy `FogVolume`s the look's `local_fog` marches.
//
// Port Serein has no arcade or covered walkway; what covers its streets is
// the Exchange's market, rows of stalls along two aisles that bow across the
// square, a cloth canopy over each at 2 to 3.5 m. The layout knows only the
// whole market as one building, and the game links no generator, so the
// stalls are found where the baked file says they are: each stall's counter
// is its one collision proxy (element `counter`, which nothing else in the
// city declares), and those proxies reach the client as `AshlarCollider`s
// for the frame before `drop_proxies` takes them. The counters along an
// aisle, both sides of it, make a run, cut into lengths of [`RUN_M`]; each
// run gets a box from under the ground to over the lowest canopies, across
// the aisle and the stalls either side, its density fading out at its ends
// and sides and through its upper half ([`strata::foliage::fog_volumes::
// density_image`]), so it reads as dusty air where the sun reaches between
// the canopies and no box shows.

/// Counters nearer than this, metres, are in one run: across an aisle the
/// two rows' counters are 5.2 m apart and along it 4 to 5.5 m.
const LINK_M: f32 = 7.0;
/// The longest run one box covers, metres, so a bowed aisle is followed by
/// a few straight boxes rather than one box over the square.
const RUN_M: f32 = 18.0;
/// How far a run's box reaches past its counters across the aisle, metres:
/// a counter stands 0.4 m inside its stall's front, and the canopy runs
/// back over the stall up to 3.4 m deep.
const RUN_REACH_M: f32 = 3.0;
/// The box's height over the lowest counter's foot, metres. The canopies'
/// undersides are 2 to 3.4 m up, and the density thins from under half the
/// box's height to nothing at its top.
const RUN_HEIGHT_M: f32 = 3.2;
/// A run gets its volume when the observer is nearer its box than this,
/// metres, horizontally, and loses it past [`MARKET_DROP_M`].
const MARKET_REACH_M: f32 = 80.0;
const MARKET_DROP_M: f32 = 120.0;
/// The most market volumes alive at once, the nearest.
const MARKET_MOST: usize = 12;
/// Extinction per metre at full density: a ray down a 15 m run keeps about
/// four fifths of what is behind it, enough for a sunlit shaft to read.
const MARKET_DENSITY: f32 = 0.02;

/// A stall's counter, in its building's frame.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Counter {
    centre: Vec3,
    /// Its length's direction, horizontal and unit.
    along: Vec3,
    length: f32,
    /// The height of its foot.
    floor: f32,
}

impl Counter {
    /// The counter a box proxy's corners describe: its long horizontal axis
    /// by the corners' principal direction.
    fn from_corners(corners: &[Vec3]) -> Option<Self> {
        if corners.len() < 4 {
            return None;
        }
        #[allow(clippy::cast_precision_loss, reason = "eight corners")]
        let centre = corners.iter().copied().sum::<Vec3>() / corners.len() as f32;
        let (mut xx, mut xz, mut zz) = (0.0, 0.0, 0.0);
        for c in corners {
            let d = *c - centre;
            xx += d.x * d.x;
            xz += d.x * d.z;
            zz += d.z * d.z;
        }
        let angle = 0.5 * f32::atan2(2.0 * xz, xx - zz);
        let along = Vec3::new(angle.cos(), 0.0, angle.sin());
        let length = corners
            .iter()
            .map(|c| (*c - centre).dot(along).abs())
            .fold(0.0, f32::max)
            * 2.0;
        let floor = corners.iter().map(|c| c.y).fold(f32::INFINITY, f32::min);
        Some(Self {
            centre,
            along,
            length,
            floor,
        })
    }
}

/// One run's box, in its building's frame: centre, yaw of its length about
/// `+y` and size (length, height, width).
#[derive(Debug, Clone, Copy, PartialEq)]
struct Run {
    centre: Vec3,
    yaw: f32,
    size: Vec3,
}

/// The group `i` is in, by union-find with path halving.
fn root(group: &mut [usize], mut i: usize) -> usize {
    while group[i] != i {
        group[i] = group[group[i]];
        i = group[i];
    }
    i
}

/// The runs a building's counters make: linked within [`LINK_M`], each
/// group cut along its mean direction into lengths of at most [`RUN_M`].
fn runs(counters: &[Counter]) -> Vec<Run> {
    // Union by distance; a market has a few dozen stalls.
    let mut group: Vec<usize> = (0..counters.len()).collect();
    for i in 0..counters.len() {
        for j in i + 1..counters.len() {
            let gap = counters[i].centre - counters[j].centre;
            if Vec2::new(gap.x, gap.z).length() < LINK_M {
                let (a, b) = (root(&mut group, i), root(&mut group, j));
                group[a] = b;
            }
        }
    }
    let mut groups: BTreeMap<usize, Vec<Counter>> = BTreeMap::new();
    for (i, counter) in counters.iter().enumerate() {
        groups
            .entry(root(&mut group, i))
            .or_default()
            .push(*counter);
    }
    let mut result = Vec::new();
    for members in groups.into_values() {
        let first = members[0].along;
        let along = members
            .iter()
            .map(|c| {
                if c.along.dot(first) < 0.0 {
                    -c.along
                } else {
                    c.along
                }
            })
            .sum::<Vec3>()
            .normalize_or(first);
        let across = Vec3::new(-along.z, 0.0, along.x);
        let mut placed: Vec<(f32, Counter)> =
            members.iter().map(|c| (c.centre.dot(along), *c)).collect();
        placed.sort_by(|a, b| a.0.total_cmp(&b.0));
        let mut start = 0;
        while start < placed.len() {
            let mut end = start + 1;
            while end < placed.len() && placed[end].0 - placed[start].0 <= RUN_M {
                end += 1;
            }
            let part = &placed[start..end];
            let (mut a0, mut a1) = (f32::INFINITY, f32::NEG_INFINITY);
            let (mut s0, mut s1) = (f32::INFINITY, f32::NEG_INFINITY);
            let mut floor = f32::INFINITY;
            for (t, c) in part {
                a0 = a0.min(t - c.length * 0.5);
                a1 = a1.max(t + c.length * 0.5);
                let s = c.centre.dot(across);
                s0 = s0.min(s);
                s1 = s1.max(s);
                floor = floor.min(c.floor);
            }
            let (a0, a1) = (a0 - 0.5, a1 + 0.5);
            let (s0, s1) = (s0 - RUN_REACH_M, s1 + RUN_REACH_M);
            let floor = floor - 0.3;
            let centre = along * f32::midpoint(a0, a1)
                + across * f32::midpoint(s0, s1)
                + Vec3::Y * (floor + RUN_HEIGHT_M * 0.5);
            result.push(Run {
                centre,
                // Bevy's turn about `+y` by `yaw` takes `+x` to
                // `(cos, 0, -sin)`.
                yaw: f32::atan2(-along.z, along.x),
                size: Vec3::new(a1 - a0, RUN_HEIGHT_M, s1 - s0),
            });
            start = end;
        }
    }
    result
}

/// The market's counters by building, their runs in the world, and the
/// runs' volumes.
#[derive(Resource, Default)]
struct MarketFog {
    counters: HashMap<Entity, Vec<Counter>>,
    /// World centre, rotation, size.
    runs: Vec<(WorldPos, Quat, Vec3)>,
    volumes: HashMap<usize, Entity>,
    image: Option<Handle<Image>>,
    stale: bool,
}

/// Files each new stall counter proxy under its building, before
/// `drop_proxies` despawns it.
fn market_counters(
    proxies: Query<(&AshlarCollider, &ChildOf), Added<AshlarCollider>>,
    buildings: Query<(), With<SettlementBuilding>>,
    mut market: ResMut<MarketFog>,
) {
    for (proxy, parent) in &proxies {
        if proxy.element != "counter" || !buildings.contains(parent.parent()) {
            continue;
        }
        if let Some(counter) = Counter::from_corners(&proxy.vertices) {
            market
                .counters
                .entry(parent.parent())
                .or_default()
                .push(counter);
            market.stale = true;
        }
    }
}

/// Keeps a volume over each market run near the observer, the nearest
/// [`MARKET_MOST`].
fn market_fog(
    mut commands: Commands,
    mut market: ResMut<MarketFog>,
    observers: Res<Observers>,
    buildings: Query<(&WorldPosition, &Transform), With<SettlementBuilding>>,
    mut images: ResMut<Assets<Image>>,
) {
    let before = market.counters.len();
    market
        .counters
        .retain(|building, _| buildings.contains(*building));
    if market.counters.len() != before {
        market.stale = true;
    }
    let market = &mut *market;
    if market.stale {
        rebuild_runs(&mut commands, market, &buildings);
    }
    let Some(eye) = observers.primary else {
        return;
    };
    let mut near: Vec<(f32, usize)> = market
        .runs
        .iter()
        .enumerate()
        .filter_map(|(i, (centre, rotation, size))| {
            #[allow(clippy::cast_possible_truncation, reason = "metres within a city")]
            let offset = Vec3::new(
                (eye.x() - centre.x()) as f32,
                0.0,
                (eye.z() - centre.z()) as f32,
            );
            let local = rotation.inverse() * offset;
            let gap = Vec2::new(
                (local.x.abs() - size.x * 0.5).max(0.0),
                (local.z.abs() - size.z * 0.5).max(0.0),
            )
            .length();
            let reach = if market.volumes.contains_key(&i) {
                MARKET_DROP_M
            } else {
                MARKET_REACH_M
            };
            (gap <= reach).then_some((gap, i))
        })
        .collect();
    near.sort_by(|a, b| a.0.total_cmp(&b.0));
    near.truncate(MARKET_MOST);
    market.volumes.retain(|i, volume| {
        let stays = near.iter().any(|n| n.1 == *i);
        if !stays {
            commands.entity(*volume).despawn();
        }
        stays
    });
    let image = market
        .image
        .get_or_insert_with(|| images.add(strata::foliage::fog_volumes::density_image()))
        .clone();
    for (_, i) in near {
        if market.volumes.contains_key(&i) {
            continue;
        }
        let (centre, rotation, size) = market.runs[i];
        let volume = commands
            .spawn((
                Name::new("market air"),
                WorldPosition(centre),
                Transform::from_rotation(rotation).with_scale(size),
                bevy::light::FogVolume {
                    // Warm dust off the square's paving.
                    fog_color: Color::srgb(1.0, 0.93, 0.84),
                    density_factor: MARKET_DENSITY,
                    density_texture: Some(image.clone()),
                    absorption: 0.08,
                    scattering: 0.5,
                    scattering_asymmetry: 0.6,
                    ..default()
                },
            ))
            .id();
        market.volumes.insert(i, volume);
    }
}

/// Places every building's market runs in the world afresh, dropping the
/// volumes of the old ones: a building came or went.
fn rebuild_runs(
    commands: &mut Commands,
    market: &mut MarketFog,
    buildings: &Query<(&WorldPosition, &Transform), With<SettlementBuilding>>,
) {
    market.stale = false;
    for (_, volume) in market.volumes.drain() {
        commands.entity(volume).despawn();
    }
    market.runs.clear();
    for (building, counters) in &market.counters {
        let Ok((at, transform)) = buildings.get(*building) else {
            continue;
        };
        for run in runs(counters) {
            let offset = transform.rotation * run.centre;
            market.runs.push((
                WorldPos::new(
                    at.0.x() + f64::from(offset.x),
                    at.0.y() + f64::from(offset.y),
                    at.0.z() + f64::from(offset.z),
                ),
                transform.rotation * Quat::from_rotation_y(run.yaw),
                run.size,
            ));
        }
    }
    if !market.runs.is_empty() {
        let at: Vec<String> = market
            .runs
            .iter()
            .map(|(c, _, s)| {
                format!(
                    "({:.0}, {:.1}, {:.0}) {:.0}x{:.1}x{:.0}",
                    c.x(),
                    c.y(),
                    c.z(),
                    s.x,
                    s.y,
                    s.z
                )
            })
            .collect();
        info!("market fog: {} runs: {}", market.runs.len(), at.join("; "));
    }
}

fn clear_market_fog(mut commands: Commands, mut market: ResMut<MarketFog>) {
    for (_, volume) in market.volumes.drain() {
        commands.entity(volume).despawn();
    }
    *market = MarketFog::default();
}

fn clear(
    mut commands: Commands,
    mut present: ResMut<Present>,
    mut groups: ResMut<Groups>,
    mut pool: ResMut<LampPool>,
) {
    for light in std::mem::take(&mut pool.lights) {
        commands.entity(light).despawn();
    }
    *pool = LampPool::default();
    *groups = Groups::default();
    let present = std::mem::take(&mut *present);
    for entity in present
        .buildings
        .into_values()
        .chain(present.skylines.into_values())
    {
        commands.entity(entity).despawn();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bevy::ecs::system::RunSystemOnce;

    fn group(near: f32, far: f32, enabled: bool) -> Group {
        Group {
            key: (Entity::PLACEHOLDER, 0, 0, 0, 0),
            root: Entity::PLACEHOLDER,
            low: Vec3::new(0.0, 0.0, 0.0),
            high: Vec3::new(40.0, 10.0, 40.0),
            near,
            far,
            members: Vec::new(),
            enabled,
            casts: true,
        }
    }

    // Far buildings cast no shadow (lighting plan phase 6).
    #[test]
    fn a_group_casts_within_the_buildings_reach_and_does_not_flicker_at_it() {
        let mut g = group(250.0, f32::MAX, true);
        // The cell spans 0 to 40 m; the reach is 400 m of depth.
        let limit = caster_limit(Some(400.0), CASTER_VIEW_SLACK).expect("a limit");
        let at = |d: f32| Vec3::new(40.0 + d, 5.0, 20.0);
        assert!(casts(&g, at(limit - 1.0), Some(limit)));
        assert!(!casts(&g, at(limit + 1.0), Some(limit)));
        // Every distance casts when buildings cast into every cascade.
        assert!(casts(&g, at(10_000.0), None));
        // Once out, it comes back only well inside the line.
        g.casts = false;
        assert!(!casts(&g, at(limit - 1.0), Some(limit)));
        assert!(casts(&g, at(limit - CASTER_HYSTERESIS - 1.0), Some(limit)));
        // By default buildings cast into every cascade; asked for two, the
        // reach is the second's bound of a geometric split of 120 m to the
        // shadow distance, which keeps the street's own buildings casting.
        let mut sky = strata::atmosphere::SkySettings::default();
        assert_eq!(sky.building_shadow_reach(&sky.cascades()), None);
        sky.building_shadow_cascades = 2;
        let reach = sky.building_shadow_reach(&sky.cascades()).expect("a reach");
        assert!(reach > 250.0 && reach < sky.shadow_distance, "{reach}");
    }

    // The settings' field of view widens the frame (review of phase 6).
    #[test]
    fn a_wider_view_reaches_farther_for_the_same_depth() {
        let view = |degrees: f32| {
            view_slack(&Projection::Perspective(PerspectiveProjection {
                fov: degrees.to_radians(),
                aspect_ratio: 16.0 / 9.0,
                ..default()
            }))
        };
        // The default 45 degrees: the corner is 40 degrees off the axis.
        assert!((view(45.0) - 1.31).abs() < 0.01, "{}", view(45.0));
        // The slider's widest, 100 degrees: depth 693 m is 1.8 km out.
        let wide = 693.0 * view(100.0);
        assert!((1_780.0..1_860.0).contains(&wide), "{wide}");
    }

    #[test]
    fn a_fine_group_plays_only_near_its_cell() {
        // Level zero: drawn to 60 m.
        assert!(in_play(
            &group(0.0, 60.0, false),
            Vec3::new(20.0, 2.0, 20.0)
        ));
        assert!(in_play(
            &group(0.0, 60.0, false),
            Vec3::new(90.0, 2.0, 20.0)
        ));
        assert!(!in_play(
            &group(0.0, 60.0, false),
            Vec3::new(120.0, 2.0, 20.0)
        ));
    }

    #[test]
    fn a_coarse_group_leaves_play_when_the_observer_is_inside_its_near_edge() {
        // Level two: drawn from 250 m on; every corner of the cell nearer.
        assert!(!in_play(
            &group(250.0, f32::INFINITY, true),
            Vec3::new(20.0, 2.0, 20.0)
        ));
        assert!(in_play(
            &group(250.0, f32::INFINITY, false),
            Vec3::new(400.0, 2.0, 20.0)
        ));
    }

    #[test]
    fn a_group_on_its_boundary_does_not_flicker() {
        // Just past the far edge plus the early slack: out if it was out, in
        // if it was in.
        let eye = Vec3::new(40.0 + 60.0 + 10.0, 0.0, 20.0);
        assert!(!in_play(&group(0.0, 60.0, false), eye));
        assert!(in_play(&group(0.0, 60.0, true), eye));
    }

    /// Adds each `(level, at, near, far)` to `groups` through the key
    /// `group_pieces` gives it, all of one building.
    fn add_all(groups: &mut Groups, pieces: &[(usize, Vec3, f32, f32)]) {
        let piece = Entity::from_raw_u32(1).expect("a valid index");
        for &(level, at, near, far) in pieces {
            let key = group_key(Entity::PLACEHOLDER, at, level, far);
            assert!(groups.add(key, piece, at, near, far));
        }
    }

    #[test]
    fn pieces_of_one_level_ending_close_together_make_one_group() {
        // Level one, each band pushed out by its distance to its far cell's
        // centre (275 m and some): the ends within one step share a group
        // reaching the farthest of them. Level two, in the same cell, is a
        // group of its own.
        let mut groups = Groups::default();
        add_all(
            &mut groups,
            &[
                (1, Vec3::new(2.0, 0.0, 3.0), 60.0, 290.0),
                (1, Vec3::new(30.0, 4.0, 10.0), 60.0, 311.5),
                (1, Vec3::new(12.0, 1.0, 38.0), 60.0, 297.0),
                (2, Vec3::new(20.0, 9.0, 20.0), 250.0, f32::MAX),
            ],
        );
        assert_eq!(groups.groups.len(), 2);
        let first = &groups.groups[0];
        assert_eq!(first.key.3, 1);
        assert_eq!(first.members.len(), 3);
        assert!((first.near - 60.0).abs() < f32::EPSILON);
        assert!((first.far - 311.5).abs() < f32::EPSILON);
        assert_eq!(first.low, Vec3::new(2.0, 0.0, 3.0));
        assert_eq!(first.high, Vec3::new(30.0, 4.0, 38.0));
        assert_eq!(groups.groups[1].key.4, i32::MAX, "drawn to any distance");
    }

    #[test]
    fn pieces_that_end_at_their_level_are_not_held_in_play_by_carried_ones() {
        // Level zero: pieces that end at 60 m (66 with the crossfade) and
        // pieces carried on unchanged to level one's end, in one cell. Seen
        // from 150 m off the cell, only the carried ones can draw, so only
        // their group is in play.
        let mut groups = Groups::default();
        add_all(
            &mut groups,
            &[
                (0, Vec3::new(5.0, 0.0, 5.0), 0.0, 66.0),
                (0, Vec3::new(35.0, 0.0, 30.0), 0.0, 283.0),
                (0, Vec3::new(20.0, 3.0, 12.0), 0.0, 66.0),
            ],
        );
        assert_eq!(groups.groups.len(), 2, "one group per end, not per level");
        let eye = Vec3::new(40.0 + 150.0, 2.0, 20.0);
        let wanted: Vec<(f32, bool)> = groups
            .groups
            .iter()
            .map(|g| (g.far, in_play(g, eye)))
            .collect();
        assert_eq!(wanted, [(66.0, false), (283.0, true)]);
    }

    #[test]
    fn a_far_citys_pieces_are_out_of_play_even_in_band() {
        // Level two from 2 km: in band, and still out while the city is far.
        let far = Vec3::new(2_000.0, 50.0, 20.0);
        assert!(!should_play(false, &group(250.0, f32::MAX, true), far));
        assert!(!should_play(false, &group(250.0, f32::MAX, false), far));
        // The frame it goes near, the same group comes back.
        assert!(should_play(true, &group(250.0, f32::MAX, false), far));
        // Near, a group out of band stays out.
        assert!(!should_play(true, &group(0.0, 60.0, false), far));
    }

    #[test]
    fn the_pieces_play_exactly_when_their_buildings_draw() {
        // Near, or far with no skyline ready to stand in: in band, in play.
        // Far with the skyline drawing: out, wherever the observer is.
        let beside = Vec3::new(20.0, 2.0, 20.0);
        let fine = group(0.0, 60.0, false);
        for (detail, ready, playing) in [
            (Detail::Near, true, true),
            (Detail::Near, false, true),
            (Detail::Far, false, true),
            (Detail::Far, true, false),
        ] {
            assert_eq!(
                should_play(buildings_shown(detail, ready), &fine, beside),
                playing,
                "{detail:?}, skyline ready {ready}"
            );
        }
    }

    #[test]
    fn a_piece_joining_a_group_out_of_play_is_told_so() {
        let mut groups = Groups::default();
        let key = (Entity::PLACEHOLDER, 3, -2, 0, 2);
        assert!(groups.add(key, Entity::PLACEHOLDER, Vec3::ZERO, 0.0, 66.0));
        groups.groups[0].enabled = false;
        assert!(!groups.add(key, Entity::PLACEHOLDER, Vec3::ONE, 0.0, 66.0));
    }

    #[test]
    fn the_buildings_draw_whenever_the_skyline_cannot() {
        // Near, the buildings; far, the skyline, once it is there.
        assert!(buildings_shown(Detail::Near, true));
        assert!(!buildings_shown(Detail::Far, true));
        // No skyline, a skyline still loading, or one whose file failed.
        assert!(buildings_shown(Detail::Far, false));
        assert!(!skyline_ready(None));
        assert!(!skyline_ready(Some((false, false))));
        assert!(!skyline_ready(Some((false, true))));
        assert!(!skyline_ready(Some((true, true))));
        assert!(skyline_ready(Some((true, false))));
        assert_eq!(shown(false), Visibility::Hidden);
        assert_eq!(shown(true), Visibility::Inherited);
        // The skyline draws exactly when they do not: not while it loads.
        for detail in [Detail::Near, Detail::Far] {
            for ready in [false, true] {
                assert_ne!(
                    skyline_shown(detail, ready),
                    buildings_shown(detail, ready),
                    "{detail:?}, skyline ready {ready}"
                );
            }
        }
        assert!(!skyline_shown(Detail::Far, false));
    }

    /// A far settlement whose skyline is still loading: its buildings draw,
    /// and its skyline root stays hidden, so pieces ashlar spawns under it
    /// after `switch` has run draw nothing until the next frame's `switch`
    /// hides the buildings and shows it, in whichever order the two ran.
    #[test]
    fn a_skyline_still_loading_is_hidden_and_shown_as_the_buildings_go() {
        let mut city = layout(&[[-10.0, -10.0, 10.0, 10.0]]);
        city.skyline = Some("skyline.ashlar".to_owned());
        let settlement = placed(Some(city));
        let eye = settlement.to_world([5_000.0, 0.0, 0.0]);
        let mut world = World::new();
        world.insert_resource(Settlements(vec![settlement]));
        world.insert_resource(Observers {
            positions: vec![eye],
            primary: Some(eye),
        });
        let building = world.spawn((AshlarSpawned, Visibility::Inherited)).id();
        let skyline = world.spawn(Visibility::Hidden).id();
        let mut present = Present::default();
        present.buildings.insert((0, 0), building);
        present.skylines.insert(0, skyline);
        world.insert_resource(present);
        let seen = |world: &World| {
            (
                *world.get::<Visibility>(building).expect("a root"),
                *world.get::<Visibility>(skyline).expect("a root"),
                world.resource::<Present>().drawn.contains(&0),
            )
        };

        world.run_system_once(switch).expect("switch runs");
        assert_eq!(world.resource::<Present>().detail(0), Detail::Far);
        assert_eq!(
            seen(&world),
            (Visibility::Inherited, Visibility::Hidden, true),
            "the buildings draw while the skyline loads, and only they"
        );

        // Its pieces arrive after this frame's `switch`; the next one hands
        // over.
        world.entity_mut(skyline).insert(AshlarSpawned);
        world.run_system_once(switch).expect("switch runs");
        assert_eq!(
            seen(&world),
            (Visibility::Hidden, Visibility::Inherited, false),
            "the skyline draws once it is ready, and only it"
        );

        // A skyline whose file failed never draws.
        world
            .entity_mut(skyline)
            .remove::<AshlarSpawned>()
            .insert(AshlarFailed("a test".to_owned()));
        world.run_system_once(switch).expect("switch runs");
        assert_eq!(
            seen(&world),
            (Visibility::Inherited, Visibility::Hidden, true)
        );
    }

    fn layout(footprints: &[[f64; 4]]) -> SettlementLayout {
        SettlementLayout {
            id: "test".to_owned(),
            ground: 0,
            buildings: footprints
                .iter()
                .map(|&footprint| strata::worldgen::LayoutBuilding {
                    file: "b.ashlar".to_owned(),
                    position: [0.0; 3],
                    yaw: 0.0,
                    footprint,
                })
                .collect(),
            spawn: [0.0; 3],
            lobby: [0.0; 3],
            lights: Vec::new(),
            plants: Vec::new(),
            skyline: None,
        }
    }

    /// The first shipped settlement, placed on flat ground, with `layout`.
    fn placed(layout: Option<SettlementLayout>) -> PlacedSettlement {
        let assets = std::path::Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/../../assets"));
        let registries = strata::registry::load_from_dir(assets).expect("the shipped assets load");
        let def = registries
            .settlements
            .first()
            .expect("a settlement is registered");
        PlacedSettlement {
            site: strata::worldgen::Site::place(def, &|_, _| 0.0),
            layout,
        }
    }

    /// The skyline, placed as a file at the frame's origin with no yaw,
    /// draws every point of every building where that building's own file
    /// does: the skyline's vertices are the buildings' placed in the frame.
    #[test]
    fn the_skyline_draws_each_building_where_the_building_stands() {
        let settlement = placed(None);
        assert!(
            settlement.site.yaw().abs() > 0.1,
            "a frame turned from the world's, or the test proves nothing"
        );
        let (origin, turn) = placement(&settlement, [0.0; 3], 0.0);
        let world = |at: WorldPos| bevy::math::DVec3::new(at.x(), at.y(), at.z());
        for (position, yaw) in [([12.0, 40.0, -30.0], 0.3), ([-150.0, 8.0, 90.0], -0.51)] {
            let (at, rotation) = placement(&settlement, position, yaw);
            for point in [Vec3::ZERO, Vec3::X * 20.0, Vec3::new(-7.0, 30.0, 14.0)] {
                let own = world(at) + (rotation * point).as_dvec3();
                // The same point in the frame: the building's placement in it.
                #[allow(clippy::cast_possible_truncation, reason = "a yaw in radians")]
                let in_frame = Vec3::from_array(position.map(|v| v as f32))
                    + Quat::from_rotation_y(yaw as f32) * point;
                let skyline = world(origin) + (turn * in_frame).as_dvec3();
                assert!(
                    own.distance(skyline) < 1e-3,
                    "{position:?} {yaw}: {point} at {own} alone, {skyline} in the skyline"
                );
            }
        }
    }

    #[test]
    fn the_edge_bounds_every_footprint_and_an_empty_layout_has_none() {
        let city = layout(&[[-10.0, -20.0, 30.0, 5.0], [0.0, -40.0, 12.0, 50.0]]);
        assert_eq!(extent(&city), Some([-10.0, -40.0, 30.0, 50.0]));
        assert!(edge_in_frame(&city, [0.0, 0.0]).abs() < 1e-12);
        assert!((edge_in_frame(&city, [130.0, 0.0]) - 100.0).abs() < 1e-9);
        let empty = layout(&[]);
        assert_eq!(extent(&empty), None);
        assert!((edge_in_frame(&empty, [30.0, 40.0]) - 50.0).abs() < 1e-9);
    }

    #[test]
    fn the_city_goes_near_only_with_every_building_in() {
        // Inside the switch but still loading: the skyline stays.
        assert_eq!(next_detail(Detail::Far, 1_500.0, false), Detail::Far);
        assert_eq!(next_detail(Detail::Far, 0.0, false), Detail::Far);
        assert_eq!(next_detail(Detail::Far, 1_500.0, true), Detail::Near);
        // Everything in, but not yet near enough.
        assert_eq!(next_detail(Detail::Far, 1_800.0, true), Detail::Far);
    }

    #[test]
    fn the_switch_back_to_the_skyline_waits_out_the_hysteresis() {
        let back = NEAR_M + HYSTERESIS_M;
        assert_eq!(next_detail(Detail::Near, NEAR_M + 1.0, true), Detail::Near);
        assert_eq!(next_detail(Detail::Near, back - 1.0, true), Detail::Near);
        assert_eq!(next_detail(Detail::Near, back + 1.0, true), Detail::Far);
        // A building that went away while near does not by itself switch.
        assert_eq!(next_detail(Detail::Near, 100.0, false), Detail::Near);
        // Far between the two lines stays far.
        assert_eq!(next_detail(Detail::Far, back - 1.0, true), Detail::Far);
    }

    #[test]
    fn the_edge_is_zero_inside_and_measured_to_the_nearest_side() {
        let rect = [-320.0, -460.0, 320.0, 226.0];
        assert!(distance_to(rect, [0.0, 0.0]).abs() < 1e-12);
        assert!((distance_to(rect, [0.0, 1_226.0]) - 1_000.0).abs() < 1e-9);
        assert!((distance_to(rect, [-420.0, -460.0]) - 100.0).abs() < 1e-9);
        // Off a corner, the diagonal.
        assert!((distance_to(rect, [323.0, 230.0]) - 5.0).abs() < 1e-9);
    }

    #[test]
    fn the_streaming_distances_nest() {
        // Buildings are in before the switch, and out only once it has gone
        // back to the skyline.
        const {
            assert!(NEAR_M < STREAM_M);
            assert!(NEAR_M + HYSTERESIS_M < STREAM_M);
            assert!(STREAM_M < DESPAWN_M);
        }
    }

    // Market fog (lighting plan phase 2c).

    /// The eight corners of a counter `length` long along `along`, 0.6 deep
    /// and 0.9 high, standing at `foot`.
    fn corners(foot: Vec3, along: Vec3, length: f32) -> Vec<Vec3> {
        let across = Vec3::new(-along.z, 0.0, along.x);
        let mut out = Vec::new();
        for a in [-0.5, 0.5] {
            for s in [-0.3, 0.3] {
                for y in [0.0, 0.9] {
                    out.push(foot + along * (a * length) + across * s + Vec3::Y * y);
                }
            }
        }
        out
    }

    #[test]
    fn a_counter_is_read_off_its_proxy_s_corners() {
        let along = Vec3::new(0.8, 0.0, 0.6);
        let foot = Vec3::new(-40.0, 38.0, 12.0);
        let counter = Counter::from_corners(&corners(foot, along, 3.2)).expect("a box");
        assert!(
            counter.along.dot(along).abs() > 0.9999,
            "{:?}",
            counter.along
        );
        assert!((counter.length - 3.2).abs() < 1e-4);
        assert!((counter.floor - 38.0).abs() < 1e-4);
        assert!(counter.centre.distance(foot + Vec3::Y * 0.45) < 1e-4);
    }

    #[test]
    fn an_aisle_s_two_rows_make_runs_no_longer_than_a_run() {
        // Two rows facing an aisle along `x`, a counter every 4.5 m over
        // 40.5 m, and a stall alone across the square.
        let mut counters = Vec::new();
        for i in 0..10 {
            #[allow(clippy::cast_precision_loss, reason = "ten stalls")]
            let x = i as f32 * 4.5;
            for z in [-2.6, 2.6] {
                let c = corners(Vec3::new(x, 10.0, z), Vec3::X, 3.0);
                counters.push(Counter::from_corners(&c).expect("a box"));
            }
        }
        let alone = corners(Vec3::new(20.0, 10.0, 60.0), Vec3::X, 3.0);
        counters.push(Counter::from_corners(&alone).expect("a box"));
        let runs = runs(&counters);
        // The aisle in two lengths of 18 m between counters, and the lone
        // stall.
        assert_eq!(runs.len(), 3, "{runs:?}");
        let aisle: Vec<&Run> = runs.iter().filter(|r| r.centre.z.abs() < 1.0).collect();
        assert_eq!(aisle.len(), 2);
        for run in &aisle {
            // Along the aisle, covering both rows and the canopies behind.
            assert!(run.yaw.abs() < 1e-3 || (run.yaw.abs() - std::f32::consts::PI).abs() < 1e-3);
            assert!(run.size.x <= RUN_M + 3.0 + 1.0 + 1e-3, "{run:?}");
            assert!(
                (run.size.z - (5.2 + 2.0 * RUN_REACH_M)).abs() < 1e-3,
                "{run:?}"
            );
            assert!((run.size.y - RUN_HEIGHT_M).abs() < 1e-6);
            let bottom = run.centre.y - run.size.y * 0.5;
            assert!((bottom - 9.7).abs() < 1e-4, "{bottom}");
        }
        // Every counter is inside a run's box.
        for counter in &counters {
            assert!(runs.iter().any(|r| {
                let local = Quat::from_rotation_y(r.yaw).inverse() * (counter.centre - r.centre);
                (local.abs() - r.size * 0.5).max_element() <= 0.0
            }));
        }
    }
}
