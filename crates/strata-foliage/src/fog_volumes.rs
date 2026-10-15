//! Air under dense crowns (lighting plan phase 2c): a Bevy `FogVolume` per
//! 64 m cell whose near trees are many, a flat box from the ground to a
//! little under the crowns, within [`REACH_M`] of the observer and at most
//! [`MOST`] at once. The camera's `VolumetricFog` and the sun's
//! `VolumetricLight` are the look's (`strata-atmosphere`, `local_fog`);
//! with them off a volume here is an entity and nothing more.
//!
//! The cell's trees are read off its near tree entities: a resident cell's
//! root carries every plant of its placement as [`TreeInstance`]s, one per
//! piece and level, so the unique plant ids among them are its placement's
//! trees once they are installed (a few frames after the cell arrives;
//! the box is re-measured every half second until it settles).
//!
//! The shared [`density_image`] gives every local volume, here and the
//! city's, its soft sides: Bevy draws a volume as a box, and a constant
//! density shows the box's faces wherever the sun lights it. A cell's box is
//! wider than the cell by its fade, so two dense cells side by side fade
//! across each other into an even density rather than thinning at their
//! border.

use std::collections::{HashMap, HashSet};

use bevy::app::{App, Update};
use bevy::asset::{Assets, Handle, RenderAssetUsages};
use bevy::color::Color;
use bevy::ecs::prelude::*;
use bevy::image::Image;
use bevy::light::FogVolume;
use bevy::math::Vec3;
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};
use bevy::state::condition::in_state;
use bevy::state::state::OnExit;
use bevy::time::Time;
use bevy::transform::components::Transform;
use strata_core::{REGION_BASE_SIZE, WorldPos};
use strata_flora::PlantId;
use strata_registry::flora::FloraKind;
use strata_sim::{AppState, Observers, Registries, TreeLibraryRes, WorldPosition};

use crate::{FoliageSet, TreeInstance};

/// A cell gets a volume when the observer is nearer its box than this,
/// metres, horizontally.
pub const REACH_M: f64 = 80.0;
/// And loses it past this, so walking along the line does not churn.
pub const DROP_M: f64 = 120.0;
/// The most canopy volumes alive at once, the nearest. Each costs the
/// pixels it covers times the camera's steps, and the nearest is the one
/// the camera stands in.
pub const MOST: usize = 8;
/// Trees in a 64 m cell, unique plants of a tree species, from which it is
/// a wood rather than a field with trees in it. Measured 2026-10-02 at the
/// woodland study scene (open forest, cover 0.37 underfoot): its cells
/// within 120 m hold 0 to 42, half of them 17 or more; this takes the
/// denser third.
pub const DENSE_TREES: usize = 24;
/// Where a crown begins, as a share of its tree's height: the library
/// records the height to the crown's top and the trunk's collider, not the
/// crown's underside; the broadleaves' first branches are near two fifths
/// up, the conifers' lower.
const CROWN_BASE: f32 = 0.4;
/// The box's top is this far under the median crown's underside, metres:
/// air under the crowns, not fog through them.
const UNDER_CROWN_M: f32 = 2.0;
/// A box shallower than this is not worth a march: a wood on a slope steep
/// enough to bury its own crowns' undersides.
const SHALLOWEST_M: f32 = 1.5;
/// Extinction per metre at full density. Lower than the city's dust: the
/// wood's boxes are up to 80 m deep. At 0.006 the woodland showed nothing:
/// its boxes are 2 to 5 m deep under the crowns and thin through their
/// upper half, so the air a view crosses is mostly that half.
const DENSITY: f32 = 0.015;
/// How often the near cells are re-measured, seconds.
const TICK_S: f32 = 0.5;

/// The share of a local volume's extent over which its density fades to
/// nothing at each side ([`density_image`]).
pub const EDGE: f32 = 0.2;

/// The canopy's volumes and what they are measured from.
#[derive(Resource, Default)]
struct CanopyFog {
    image: Option<Handle<Image>>,
    /// Crown underside per unit of scale, by flora id, for tree species.
    crowns: Option<Vec<Option<f32>>>,
    /// Cell roots that ever had a near tree.
    roots: HashSet<Entity>,
    /// Each cell root's volume.
    volumes: HashMap<Entity, Entity>,
    since: f32,
}

pub(crate) fn register(app: &mut App) {
    app.init_resource::<CanopyFog>()
        .add_systems(
            Update,
            canopy_fog
                .in_set(FoliageSet::Wind)
                .run_if(in_state(AppState::InWorld)),
        )
        .add_systems(OnExit(AppState::InWorld), clear);
}

/// The 32-cubed density every local volume is drawn with: one at its middle,
/// fading to nothing over the outer [`EDGE`] of each horizontal side and the
/// upper half of its height, broken by a soft noise of a few blobs across.
/// The bottom does not fade: a box stands on, or just under, the ground.
#[must_use]
pub fn density_image() -> Image {
    const SIZE: u32 = 32;
    let mut bytes = Vec::with_capacity((SIZE * SIZE * SIZE) as usize);
    for z in 0..SIZE {
        for y in 0..SIZE {
            for x in 0..SIZE {
                let [u, v, w] = [x, y, z].map(|i| texel(i, SIZE));
                bytes.push(to_byte(density(u, v, w)));
            }
        }
    }
    Image::new(
        Extent3d {
            width: SIZE,
            height: SIZE,
            depth_or_array_layers: SIZE,
        },
        TextureDimension::D3,
        bytes,
        TextureFormat::R8Unorm,
        RenderAssetUsages::RENDER_WORLD,
    )
}

/// A texel's centre in `0..1`.
fn texel(i: u32, size: u32) -> f32 {
    (f32::from(u16::try_from(i).unwrap_or(u16::MAX)) + 0.5)
        / f32::from(u16::try_from(size).unwrap_or(u16::MAX))
}

#[allow(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "a density clamped to 0..=1 scaled into a byte"
)]
fn to_byte(density: f32) -> u8 {
    (density.clamp(0.0, 1.0) * 255.0).round() as u8
}

fn smoothstep(edge0: f32, edge1: f32, x: f32) -> f32 {
    let t = ((x - edge0) / (edge1 - edge0)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// The density at `(u, v, w)` in the unit box, `v` up.
fn density(u: f32, v: f32, w: f32) -> f32 {
    // Smoothstep's two ramps over one overlap sum to one, so neighbouring
    // boxes overlapping by their fade add up to an even density.
    let side = |t: f32| smoothstep(0.0, EDGE, t) * smoothstep(0.0, EDGE, 1.0 - t);
    let height = 1.0 - smoothstep(0.45, 1.0, v);
    side(u) * side(w) * height * (0.7 + 0.3 * noise(u * 4.0, v * 3.0, w * 4.0))
}

/// Smooth value noise in `0..1` on an integer lattice, from a hash: the same
/// texture every run.
fn noise(x: f32, y: f32, z: f32) -> f32 {
    let (fx, fy, fz) = (x.floor(), y.floor(), z.floor());
    let (tx, ty, tz) = (x - fx, y - fy, z - fz);
    let ease = |t: f32| t * t * (3.0 - 2.0 * t);
    let (sx, sy, sz) = (ease(tx), ease(ty), ease(tz));
    #[allow(
        clippy::cast_possible_truncation,
        reason = "lattice coordinates of a few units"
    )]
    let at =
        |dx: f32, dy: f32, dz: f32| lattice((fx + dx) as i32, (fy + dy) as i32, (fz + dz) as i32);
    let lerp = |a: f32, b: f32, t: f32| a + (b - a) * t;
    let x00 = lerp(at(0.0, 0.0, 0.0), at(1.0, 0.0, 0.0), sx);
    let x10 = lerp(at(0.0, 1.0, 0.0), at(1.0, 1.0, 0.0), sx);
    let x01 = lerp(at(0.0, 0.0, 1.0), at(1.0, 0.0, 1.0), sx);
    let x11 = lerp(at(0.0, 1.0, 1.0), at(1.0, 1.0, 1.0), sx);
    lerp(lerp(x00, x10, sy), lerp(x01, x11, sy), sz)
}

#[allow(
    clippy::cast_sign_loss,
    clippy::cast_precision_loss,
    reason = "a hash's bits reinterpreted, its top 24 as a fraction"
)]
fn lattice(x: i32, y: i32, z: i32) -> f32 {
    let mut h = (x as u32).wrapping_mul(0x8da6_b343)
        ^ (y as u32).wrapping_mul(0xd816_3841)
        ^ (z as u32).wrapping_mul(0xcb1a_b31f);
    h ^= h >> 15;
    h = h.wrapping_mul(0x2c1b_3c6d);
    h ^= h >> 12;
    (h >> 8) as f32 / 16_777_216.0
}

/// One cell's box, world metres.
#[derive(Debug, Clone, Copy, PartialEq)]
struct CellBox {
    centre: WorldPos,
    size: Vec3,
}

/// The box under a cell's crowns, from each tree's ground height and
/// crown underside (cell-local `y`, metres): from under the lowest ground
/// to [`UNDER_CROWN_M`] under the median crown, over the cell widened by its
/// fade. `None` for a cell too sparse or too steep.
fn cell_box(corner: WorldPos, trees: &[(f32, f32)]) -> Option<CellBox> {
    if trees.len() < DENSE_TREES {
        return None;
    }
    let floor = trees.iter().map(|t| t.0).fold(f32::INFINITY, f32::min) - 0.5;
    let mut crowns: Vec<f32> = trees.iter().map(|t| t.1).collect();
    crowns.sort_by(f32::total_cmp);
    let top = crowns[crowns.len() / 2] - UNDER_CROWN_M;
    if top - floor < SHALLOWEST_M {
        return None;
    }
    #[allow(clippy::cast_possible_truncation, reason = "64 m")]
    let width = REGION_BASE_SIZE as f32 / (1.0 - EDGE);
    let half = REGION_BASE_SIZE * 0.5;
    Some(CellBox {
        centre: WorldPos::new(
            corner.x() + half,
            corner.y() + f64::from(f32::midpoint(floor, top)),
            corner.z() + half,
        ),
        size: Vec3::new(width, top - floor, width),
    })
}

/// Horizontal distance from `eye` to a box, metres; zero inside it.
fn reach(eye: WorldPos, cell: &CellBox) -> f64 {
    let dx = ((eye.x() - cell.centre.x()).abs() - f64::from(cell.size.x) * 0.5).max(0.0);
    let dz = ((eye.z() - cell.centre.z()).abs() - f64::from(cell.size.z) * 0.5).max(0.0);
    dx.hypot(dz)
}

/// The crown underside per unit of scale of every tree species, by flora id:
/// [`CROWN_BASE`] of the mean height of its baked variants.
fn crowns(registries: &Registries, library: Option<&TreeLibraryRes>) -> Vec<Option<f32>> {
    let library = library.and_then(|l| l.0.as_deref());
    registries
        .0
        .flora_ids()
        .map(|id| {
            let def = registries.0.flora.get(id)?;
            if def.kind != FloraKind::Tree {
                return None;
            }
            let variants = &library?.species.get(def.id.as_ref())?.variants;
            #[allow(clippy::cast_precision_loss, reason = "at most 16 variants")]
            let mean =
                variants.iter().map(|v| v.height).sum::<f32>() / variants.len().max(1) as f32;
            Some(mean * CROWN_BASE)
        })
        .collect()
}

/// Finds the near cells with many trees and keeps a volume under each of
/// the nearest [`MOST`].
#[allow(
    clippy::too_many_arguments,
    reason = "a Bevy system's parameters are its dependencies"
)]
fn canopy_fog(
    mut commands: Commands,
    mut fog: ResMut<CanopyFog>,
    time: Res<Time>,
    observers: Res<Observers>,
    registries: Option<Res<Registries>>,
    library: Option<Res<TreeLibraryRes>>,
    mut images: ResMut<Assets<Image>>,
    added: Query<&ChildOf, Added<TreeInstance>>,
    roots: Query<(&WorldPosition, &Children)>,
    trees: Query<(&TreeInstance, &Transform)>,
    mut volumes: Query<(&mut WorldPosition, &mut Transform), VolumeFilter>,
) {
    for parent in &added {
        fog.roots.insert(parent.parent());
    }
    fog.since += time.delta_secs();
    if fog.since < TICK_S {
        return;
    }
    fog.since = 0.0;
    let (Some(eye), Some(registries)) = (observers.primary, registries) else {
        return;
    };
    if fog.crowns.is_none() {
        fog.crowns = Some(crowns(&registries, library.as_deref()));
    }
    let fog = &mut *fog;
    let crown_of = fog.crowns.as_deref().unwrap_or_default();
    fog.roots.retain(|root| roots.contains(*root));

    let mut wanted: Vec<(f64, Entity, CellBox)> = Vec::new();
    let mut seen: HashSet<PlantId> = HashSet::new();
    let mut cell_trees = Vec::new();
    for &root in &fog.roots {
        let Ok((corner, children)) = roots.get(root) else {
            continue;
        };
        let size = REGION_BASE_SIZE;
        let middle = WorldPos::new(corner.0.x() + size * 0.5, 0.0, corner.0.z() + size * 0.5);
        // Nearer than the drop distance plus the widened box's half diagonal.
        if (eye.x() - middle.x()).hypot(eye.z() - middle.z()) > DROP_M + size {
            continue;
        }
        seen.clear();
        cell_trees.clear();
        for child in children {
            let Ok((tree, transform)) = trees.get(*child) else {
                continue;
            };
            let Some(Some(crown)) = usize::try_from(tree.species.0)
                .ok()
                .and_then(|i| crown_of.get(i))
            else {
                continue;
            };
            if seen.insert(tree.id) {
                let ground = transform.translation.y;
                cell_trees.push((ground, ground + crown * transform.scale.y));
            }
        }
        tracing::debug!(
            x = corner.0.x(),
            z = corner.0.z(),
            trees = cell_trees.len(),
            "canopy cell"
        );
        let Some(cell) = cell_box(corner.0, &cell_trees) else {
            continue;
        };
        let distance = reach(eye, &cell);
        let keep = if fog.volumes.contains_key(&root) {
            DROP_M
        } else {
            REACH_M
        };
        if distance <= keep {
            wanted.push((distance, root, cell));
        }
    }
    wanted.sort_by(|a, b| a.0.total_cmp(&b.0));
    wanted.truncate(MOST);

    let image = fog
        .image
        .get_or_insert_with(|| images.add(density_image()))
        .clone();
    keep_volumes(&mut commands, fog, wanted, &image, &mut volumes);
}

/// Disjoint from the trees' and the roots' queries, which read the same
/// components.
type VolumeFilter = (With<FogVolume>, Without<Children>, Without<TreeInstance>);

/// Despawns the volumes of cells no longer `wanted`, re-measures the rest
/// while their trees are still arriving, and spawns the new ones.
fn keep_volumes(
    commands: &mut Commands,
    fog: &mut CanopyFog,
    wanted: Vec<(f64, Entity, CellBox)>,
    image: &Handle<Image>,
    volumes: &mut Query<(&mut WorldPosition, &mut Transform), VolumeFilter>,
) {
    let keep: HashSet<Entity> = wanted.iter().map(|w| w.1).collect();
    fog.volumes.retain(|root, volume| {
        let stays = keep.contains(root);
        if !stays {
            commands.entity(*volume).despawn();
        }
        stays
    });
    for (_, root, cell) in wanted {
        if let Some(&volume) = fog.volumes.get(&root) {
            // Re-measured while its trees are still arriving.
            if let Ok((mut position, mut transform)) = volumes.get_mut(volume) {
                if position.0 != cell.centre {
                    position.0 = cell.centre;
                }
                if transform.scale != cell.size {
                    transform.scale = cell.size;
                }
            }
            continue;
        }
        let volume = commands
            .spawn((
                Name::new("canopy air"),
                WorldPosition(cell.centre),
                Transform::from_scale(cell.size),
                FogVolume {
                    fog_color: Color::srgb(0.92, 0.95, 1.0),
                    density_factor: DENSITY,
                    density_texture: Some(image.clone()),
                    absorption: 0.05,
                    scattering: 0.5,
                    scattering_asymmetry: 0.6,
                    ..Default::default()
                },
            ))
            .id();
        fog.volumes.insert(root, volume);
    }
}

fn clear(mut commands: Commands, mut fog: ResMut<CanopyFog>) {
    for (_, volume) in fog.volumes.drain() {
        commands.entity(volume).despawn();
    }
    fog.roots.clear();
    fog.since = 0.0;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn neighbouring_boxes_fade_across_each_other_into_an_even_density() {
        // Two boxes side by side, overlapping by their fade: the sum of their
        // sides across the overlap is flat.
        let side = |t: f32| smoothstep(0.0, EDGE, t) * smoothstep(0.0, EDGE, 1.0 - t);
        for i in 0..=20 {
            #[allow(clippy::cast_precision_loss, reason = "0 to 20")]
            let s = EDGE * i as f32 / 20.0;
            // Box A's last `EDGE` and box B's first, at the same point.
            let sum = side(1.0 - EDGE + s) + side(s);
            assert!((sum - 1.0).abs() < 1e-5, "{s}: {sum}");
        }
    }

    #[test]
    fn the_density_fades_to_nothing_at_every_side_and_the_top_but_not_the_bottom() {
        for t in [0.0, 1.0] {
            assert!(density(t, 0.2, 0.5) < 1e-6);
            assert!(density(0.5, 0.2, t) < 1e-6);
        }
        assert!(density(0.5, 1.0, 0.5) < 1e-6);
        assert!(density(0.5, 0.0, 0.5) > 0.6);
        let image = density_image();
        assert_eq!(image.data.as_ref().map(Vec::len), Some(32 * 32 * 32));
    }

    fn corner() -> WorldPos {
        WorldPos::new(640.0, 0.0, -1280.0)
    }

    #[test]
    fn a_dense_cell_gets_a_box_from_its_floor_to_under_its_crowns() {
        let trees: Vec<(f32, f32)> = (0..DENSE_TREES)
            .map(|i| {
                #[allow(clippy::cast_precision_loss, reason = "a few dozen")]
                let ground = 100.0 + i as f32 * 0.1;
                (ground, ground + 5.0)
            })
            .collect();
        let cell = cell_box(corner(), &trees).expect("dense");
        let bottom = cell.centre.y() - f64::from(cell.size.y) * 0.5;
        let top = cell.centre.y() + f64::from(cell.size.y) * 0.5;
        assert!((bottom - 99.5).abs() < 1e-3, "{bottom}");
        let median = f64::from(trees[trees.len() / 2].1) - f64::from(UNDER_CROWN_M);
        assert!((top - median).abs() < 1e-3, "{top}");
        // Wider than the cell by its fade on each side.
        assert!(cell.size.x > 64.0 && (cell.size.x * (1.0 - EDGE) - 64.0).abs() < 1e-3);
        assert!((cell.centre.x() - 672.0).abs() < 1e-9 && (cell.centre.z() + 1248.0).abs() < 1e-9);
        assert!(reach(cell.centre, &cell) < 1e-9);
    }

    #[test]
    fn a_sparse_or_buried_cell_gets_none() {
        let sparse = vec![(100.0, 106.0); DENSE_TREES - 1];
        assert!(cell_box(corner(), &sparse).is_none());
        // Crowns that start under 3 m over the lowest ground leave less than
        // the shallowest box once the gap under them is taken off and the
        // half metre under the floor added.
        let low = vec![(100.0, 102.9); DENSE_TREES];
        assert!(cell_box(corner(), &[(100.0, 103.1); DENSE_TREES]).is_some());
        assert!(cell_box(corner(), &low).is_none());
    }
}
