//! Procedural sites (ADR 0012, ticket 0005 step 4): the places the bake
//! finds for itself, and the level pad each one lays on the ground.
//!
//! A site kind is registry data (`assets/sites/*.site.ron`, see
//! `strata_registry::site`). The bake places sites per land mass after its
//! rivers (`hydrology::place`), so a site is part of the network a mass
//! bakes to: the same seed, registries and code give the same sites on any
//! machine and any thread count, and the set is folded into the network's
//! content hash, so into the world's version through the canaries.
//!
//! What a site does to the ground is [`Pad`]: within its footprint the
//! carved ground is replaced by a level pad at the site's level (a slight
//! crown sheds water), eased back into the ground over a band outside it
//! with a continuous height and gradient, and developed so nothing grows or
//! is placed there (ADR 0009, decision 4) out to a skirt past its edge.
//! Only its yard is laid in gravel ([`LAID_FROM`]); the rest is cleared,
//! level ground in the land's own material. A sample reads the pads of its
//! lattice cell from the cell it already reads for the carve, so a point
//! far from every site pays nothing more.
//!
//! For later steps: a site's [`arrival`](ProceduralSite::arrival) is where a
//! road should reach its pad, its [`facing`](ProceduralSite::facing) the way
//! its buildings' fronts look, and [`ProceduralWorld::sites_near`] and
//! [`ProceduralWorld::site_at`] find them.
//!
//! What stands on a site is a building group (the kind's `group`), baked
//! once per kind by the content step into a [`SiteGroupLayout`]: a few
//! arrangements of baked buildings in the site's frame, laid out on the
//! kind's pad alone. [`place_group`] stands one on a site: which
//! arrangement, where each building is and how it is turned, on the pad's
//! own height. It is a pure function of the site and the layout, so the
//! server building colliders and a client drawing the buildings stand the
//! same group the same way.
//!
//! [`ProceduralWorld::sites_near`]: crate::ProceduralWorld::sites_near
//! [`ProceduralWorld::site_at`]: crate::ProceduralWorld::site_at

use serde::{Deserialize, Serialize};
use strata_core::Rng;
use strata_registry::{
    NamesDef, Namespaced, RoadClass, SiteFeature, SiteKindDef, SiteKindId, SiteKindTraits,
};

/// One kind as the world reads it: its file and its parsed traits.
#[derive(Debug, Clone, PartialEq)]
pub struct SiteKind {
    /// Its dense id in the frozen registry.
    pub id: SiteKindId,
    /// Its namespaced name.
    pub name: Namespaced,
    /// The file.
    pub def: SiteKindDef,
    /// The parsed traits.
    pub traits: SiteKindTraits,
}

/// A site's identity within a world version (ADR 0012, decision 4): its
/// kind, the land mass it stands on (the mass's key) and the lattice node
/// its search started from. A generator change that moves the site moves
/// its id, as it moves a river.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct SiteId {
    /// The kind.
    pub kind: SiteKindId,
    /// The land mass's key ([`Network::key`](crate::hydrology::Network::key)).
    pub mass: [i32; 2],
    /// The lattice node the site was found from.
    pub node: [i32; 2],
}

impl SiteId {
    /// The id as one number, the same on every platform: what a save or a
    /// mission would store.
    #[must_use]
    pub fn key(&self) -> u64 {
        let mut hash = 0xcbf2_9ce4_8422_2325_u64;
        for v in [
            self.kind.0.cast_signed(),
            self.mass[0],
            self.mass[1],
            self.node[0],
            self.node[1],
        ] {
            for b in v.to_le_bytes() {
                hash ^= u64::from(b);
                hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
            }
        }
        hash
    }
}

impl std::fmt::Display for SiteId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "site {}/{},{}/{},{}",
            self.kind.0, self.mass[0], self.mass[1], self.node[0], self.node[1]
        )
    }
}

/// A placed site.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ProceduralSite {
    /// Its identity.
    pub id: SiteId,
    /// Its name, unique on its land mass.
    pub name: String,
    /// The pad's centre, world `(x, z)`.
    pub centre: [f64; 2],
    /// The pad's level at its centre, metres.
    pub level_m: f64,
    /// The pad's radius, metres.
    pub radius_m: f64,
    /// The band outside it where the ground eases back, metres.
    pub blend_m: f64,
    /// The pad's fall at its edge, rise over run.
    pub crown: f64,
    /// Which way its front looks, a unit `(x, z)`.
    pub facing: [f64; 2],
    /// Where a road should reach the pad: the point on its edge facing the
    /// gentlest approach over the band, world `(x, z)`.
    pub arrival: [f64; 2],
    /// The feature it stands by, where one decided its place: the fall a
    /// landmark overlooks, the ford a hamlet keeps.
    pub by: Option<SiteFeature>,
    /// The place's score, `0..=1`: how well the ground met the kind's wants.
    pub score: f64,
    /// The deepest cut or fill the pad makes over its footprint, metres.
    pub cut_m: f64,
}

impl ProceduralSite {
    /// The kind.
    #[must_use]
    pub const fn kind(&self) -> SiteKindId {
        self.id.kind
    }

    /// How far from its centre the site changes the ground, metres.
    #[must_use]
    pub fn outer_m(&self) -> f64 {
        self.radius_m + self.blend_m
    }

    /// What the site does to the ground.
    #[must_use]
    pub const fn pad(&self) -> Pad {
        Pad {
            centre: self.centre,
            radius: self.radius_m,
            blend: self.blend_m,
            level: self.level_m,
            crown: self.crown,
        }
    }

    /// The pad's own height at `(x, z)` inside its footprint: the level less
    /// the crown's fall.
    #[must_use]
    pub fn pad_height_at(&self, x: f64, z: f64) -> f64 {
        self.pad().target(x, z).0
    }

    /// The facing as a yaw about `+y`, radians from `+z` towards `+x`, the
    /// convention of a settlement's `downhill_deg`. Not on any sample's
    /// path, so `libm`'s `atan2` is fine (and the same bits everywhere).
    #[must_use]
    pub fn yaw(&self) -> f64 {
        libm::atan2(self.facing[0], self.facing[1])
    }

    /// A point of the site's frame in the world, `(x, z)`: origin at the
    /// pad's centre, `+z` the facing, `+x` the facing turned a quarter
    /// clockwise seen from above, as a settlement's frame (and as
    /// `Quat::from_rotation_y(self.yaw())` turns).
    #[must_use]
    pub fn to_world_xz(&self, local: [f64; 2]) -> [f64; 2] {
        let [fx, fz] = self.facing;
        [
            self.centre[0] + local[0] * fz + local[1] * fx,
            self.centre[1] - local[0] * fx + local[1] * fz,
        ]
    }

    /// A world point `(x, z)` in the site's frame.
    #[must_use]
    pub fn to_local_xz(&self, world: [f64; 2]) -> [f64; 2] {
        let [fx, fz] = self.facing;
        let (wx, wz) = (world[0] - self.centre[0], world[1] - self.centre[1]);
        [wx * fz - wz * fx, wx * fx + wz * fz]
    }

    /// A yaw given in the site's frame as a world yaw.
    #[must_use]
    pub fn world_yaw(&self, local: f64) -> f64 {
        local + self.yaw()
    }
}

/// The ground a site lays: a level pad at `level` within `radius` of
/// `centre`, crowned by `crown` at its edge, eased into the ground over
/// `blend` outside it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Pad {
    /// World `(x, z)`.
    pub centre: [f64; 2],
    /// Metres.
    pub radius: f64,
    /// Metres.
    pub blend: f64,
    /// Metres.
    pub level: f64,
    /// Rise over run at the edge.
    pub crown: f64,
}

/// How far past a pad's edge its ground is laid and kept clear, metres:
/// fully developed to here, then fading out over [`SKIRT_FADE_M`].
pub const SKIRT_M: f64 = strata_registry::PAD_SKIRT_M;
/// The fade of the developed skirt, metres.
pub const SKIRT_FADE_M: f64 = strata_registry::PAD_SKIRT_FADE_M;

/// The laid share over which a pad's ground is gravel, in `(0.15, 0.85)`,
/// varying over a couple of metres: the material is chosen per vertex, so a
/// single threshold drew the gravel's edge as one hard ring, and this
/// breaks it into patches that thin out across the fade.
/// Integer hashing and bilinear blending only, so it is the same bits on
/// every platform.
#[must_use]
#[allow(
    clippy::many_single_char_names,
    reason = "a point, its cell and the cell's fractions, as value noise writes them"
)]
pub fn gravel_threshold(x: f64, z: f64) -> f64 {
    const CELL_M: f64 = 2.5;
    let (u, v) = (x / CELL_M, z / CELL_M);
    let (i, j) = (u.floor(), v.floor());
    let (fu, fv) = (u - i, v - j);
    #[allow(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        clippy::cast_precision_loss,
        reason = "a cell index floored from a world coordinate, and 53 bits of a hash"
    )]
    let at = |a: f64, b: f64| -> f64 {
        let (a, b) = (a as i64 as u64, b as i64 as u64);
        let mut k = a.wrapping_mul(0x9e37_79b9_7f4a_7c15) ^ b.wrapping_mul(0xc2b2_ae3d_27d4_eb4f);
        k ^= k >> 29;
        k = k.wrapping_mul(0xbf58_476d_1ce4_e5b9);
        k ^= k >> 32;
        (k >> 11) as f64 / (1_u64 << 53) as f64
    };
    let (su, sv) = (fu * fu * (3.0 - 2.0 * fu), fv * fv * (3.0 - 2.0 * fv));
    let top = at(i, j) + (at(i + 1.0, j) - at(i, j)) * su;
    let bottom = at(i, j + 1.0) + (at(i + 1.0, j + 1.0) - at(i, j + 1.0)) * su;
    let n = top + (bottom - top) * sv;
    0.15 + 0.7 * n
}

/// The share of a pad's radius out to which its ground is all gravel; it
/// thins out from there to nothing at the edge (by [`gravel_threshold`]).
/// A group stands round a yard at the centre and its pad reaches past the
/// buildings to hold the board and the way in, so a pad gravelled to its
/// edge read from above as a car park with a few houses at the back; the
/// rest of the pad and its skirt are cleared, level ground.
pub const LAID_FROM: f64 = 0.45;

/// What a pad does to a sample.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct PadShaped {
    pub height: f64,
    pub gradient: (f64, f64),
    /// `0..=1`: how much of the point the site has taken.
    pub developed: f64,
    /// `0..=1`: how much of it is laid in gravel: the yard round the
    /// centre, fading out before the pad's edge ([`LAID_FROM`]).
    pub laid: f64,
}

impl Pad {
    /// The pad's own surface at a point and its gradient: the level less a
    /// crown, `crown d^2 / 2r` inside and its tangent beyond, so the
    /// surface is continuous with its gradient at the edge.
    fn target(&self, x: f64, z: f64) -> (f64, (f64, f64)) {
        let (dx, dz) = (x - self.centre[0], z - self.centre[1]);
        let d = (dx * dx + dz * dz).sqrt();
        if d <= self.radius {
            let k = self.crown / self.radius;
            (self.level - 0.5 * k * d * d, (-k * dx, -k * dz))
        } else {
            let fall = self.crown / d;
            (
                self.level - self.crown * (0.5 * self.radius + (d - self.radius)),
                (-fall * dx, -fall * dz),
            )
        }
    }

    /// The pad at `(x, z)` over ground of `height` and `gradient`, or `None`
    /// where the pad does not reach. Inside the footprint the ground is the
    /// pad's; across the band it is a smoothstep blend of the two, its
    /// gradient carried, so height and gradient are continuous everywhere.
    pub(crate) fn shape(
        &self,
        x: f64,
        z: f64,
        height: f64,
        gradient: (f64, f64),
    ) -> Option<PadShaped> {
        let (dx, dz) = (x - self.centre[0], z - self.centre[1]);
        let d2 = dx * dx + dz * dz;
        let outer = self.radius + self.blend;
        if d2 >= outer * outer {
            return None;
        }
        let d = d2.sqrt();
        let (target, (tx, tz)) = self.target(x, z);
        // The weight of the pad and its gradient along the radius.
        let (w, dw) = if d <= self.radius {
            (1.0, 0.0)
        } else {
            let u = 1.0 - (d - self.radius) / self.blend;
            (u * u * (3.0 - 2.0 * u), -6.0 * u * (1.0 - u) / self.blend)
        };
        let (ux, uz) = if d > 0.0 {
            (dx / d, dz / d)
        } else {
            (0.0, 0.0)
        };
        let off = target - height;
        let shaped = height + w * off;
        let gradient = (
            w * tx + (1.0 - w) * gradient.0 + off * dw * ux,
            w * tz + (1.0 - w) * gradient.1 + off * dw * uz,
        );
        let developed = 1.0
            - crate::noise::smoothstep(
                self.radius + SKIRT_M,
                self.radius + SKIRT_M + SKIRT_FADE_M,
                d,
            );
        let laid = 1.0 - crate::noise::smoothstep(LAID_FROM * self.radius, self.radius, d);
        Some(PadShaped {
            height: shaped,
            gradient,
            developed,
            laid,
        })
    }
}

/// The directory under the content step's output that building groups'
/// layouts and files are written to.
pub const GROUPS_DIR: &str = "sites";

/// Where the content step writes the layout of the group `group` names,
/// under its output: `veyra:hamlet` is `sites/veyra/hamlet.ron`.
#[must_use]
pub fn group_layout_path(group: &str) -> String {
    match group.split_once(':') {
        Some((namespace, name)) => format!("{GROUPS_DIR}/{namespace}/{name}.ron"),
        None => format!("{GROUPS_DIR}/{group}.ron"),
    }
}

/// A site kind's building group as the content step baked it: a few
/// arrangements of baked buildings in the site's frame, any of which
/// stands on any pad of the kind (ADR 0012, decision 6).
///
/// Not fitted to one site: a site moves with the generator, and the pad is
/// the same shape for every site of a kind but for its level. So nothing
/// here carries a fingerprint; [`place_group`] checks each arrangement
/// against the site's own radius instead, and leaves out one that no
/// longer fits.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SiteGroupLayout {
    /// The group's name, as a kind's `group` names it.
    pub group: String,
    /// The open ground round the pad's centre that a road ends in, metres.
    pub yard_m: f64,
    /// The arrangements.
    pub arrangements: Vec<SiteArrangement>,
    /// The name board stood at the road's arrival, if the group has one.
    #[serde(default)]
    pub sign: Option<SiteSign>,
}

/// One arrangement of a [`SiteGroupLayout`].
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SiteArrangement {
    /// Its name, for logs and reviews.
    pub name: String,
    /// Its buildings.
    pub buildings: Vec<SiteBuilding>,
}

impl SiteArrangement {
    /// How far from the pad's centre its farthest footprint corner is,
    /// metres.
    #[must_use]
    pub fn reach_m(&self) -> f64 {
        self.buildings
            .iter()
            .flat_map(|b| b.footprint)
            .map(|[x, z]| (x * x + z * z).sqrt())
            .fold(0.0, f64::max)
    }
}

/// One building of an arrangement, in the site's frame.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SiteBuilding {
    /// The baked file, relative to the content step's output.
    pub file: String,
    /// Where its origin stands, `(x, z)`.
    pub position: [f64; 2],
    /// Its turn about `+y`, radians, as `Quat::from_rotation_y` turns.
    pub yaw: f64,
    /// The corners of the ground it covers, `(x, z)`, in order round it.
    pub footprint: [[f64; 2]; 4],
}

/// The name board of a [`SiteGroupLayout`].
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SiteSign {
    /// The baked file, relative to the content step's output.
    pub file: String,
    /// Half its plan along its `x` and its `z`, about its origin, metres.
    pub half_size: [f64; 2],
}

/// A building of a site, stood in the world.
#[derive(Debug, Clone, PartialEq)]
pub struct PlacedSiteBuilding {
    /// The baked file, relative to the content step's output.
    pub file: String,
    /// Its origin, world `(x, y, z)`: on the pad, a little into it.
    pub at: [f64; 3],
    /// Its world yaw about `+y`, as `Quat::from_rotation_y` turns.
    pub yaw: f64,
    /// The ground it covers, world `(x, z)` corners.
    pub footprint: [[f64; 2]; 4],
}

/// A building group stood on one site by [`place_group`].
#[derive(Debug, Clone, PartialEq)]
pub struct SitePlacement {
    /// Which arrangement of the layout.
    pub arrangement: usize,
    /// Whether the way in from the road's arrival to the yard is clear of
    /// every building. A site whose every fitting arrangement blocks it
    /// still gets one, with its arrival point itself kept clear if any can.
    pub way_in_clear: bool,
    /// The arrangement's buildings.
    pub buildings: Vec<PlacedSiteBuilding>,
    /// The name board at the arrival, unless no side of the way in had
    /// room for it.
    pub sign: Option<PlacedSiteBuilding>,
}

impl SitePlacement {
    /// Every building and the sign, in order.
    pub fn all(&self) -> impl Iterator<Item = &PlacedSiteBuilding> {
        self.buildings.iter().chain(self.sign.as_ref())
    }
}

/// How far inside a site's pad every footprint must keep, metres: the pad
/// is level only within its radius, and the band past it falls away.
pub const FIT_MARGIN_M: f64 = 1.0;

/// How far below the pad's lowest point under it a building stands,
/// metres. The pad's crown falls a few centimetres across a building, and a
/// building stood on its highest corner would show a slit of daylight under
/// its lowest; sunk this far below the lowest, no corner floats and none
/// sinks past its plinth.
pub const EMBED_M: f64 = 0.04;

/// Half the width of the way in a road of `class` needs from the arrival to
/// the yard, metres: the carriageway and a shoulder.
#[must_use]
pub const fn way_in_half_m(class: RoadClass) -> f64 {
    match class {
        RoadClass::Service => 4.0,
        RoadClass::Track => 3.0,
        RoadClass::Trail => 1.5,
    }
}

/// How much room a building keeps from the way in, metres.
const WAY_IN_MARGIN_M: f64 = 0.5;

/// How far inside the pad's edge the name board stands, metres, and how far
/// to the side of the way in.
const SIGN_INSET_M: f64 = 3.0;
const SIGN_ASIDE_M: f64 = 1.5;

/// What seeds the choice of arrangement, beside the site's id.
const ARRANGEMENT_SALT: u64 = 0x5173_a77a_6e3e_0001;

/// Whether two convex quads overlap, by separating axes. Touching is not
/// overlapping.
fn quads_overlap(a: &[[f64; 2]; 4], b: &[[f64; 2]; 4]) -> bool {
    for quad in [a, b] {
        for i in 0..4 {
            let (p, q) = (quad[i], quad[(i + 1) % 4]);
            let axis = [q[1] - p[1], p[0] - q[0]];
            let span = |s: &[[f64; 2]; 4]| {
                s.iter()
                    .map(|v| v[0] * axis[0] + v[1] * axis[1])
                    .fold((f64::INFINITY, f64::NEG_INFINITY), |(lo, hi), d| {
                        (lo.min(d), hi.max(d))
                    })
            };
            let (a0, a1) = span(a);
            let (b0, b1) = span(b);
            if a1 <= b0 || b1 <= a0 {
                return false;
            }
        }
    }
    true
}

/// A rectangle about `centre` with half sizes `half` along the unit `along`
/// and its quarter turn, as four corners in order.
fn rectangle(centre: [f64; 2], along: [f64; 2], half: [f64; 2]) -> [[f64; 2]; 4] {
    let across = [along[1], -along[0]];
    let corner = |a: f64, b: f64| {
        [
            centre[0] + across[0] * a * half[0] + along[0] * b * half[1],
            centre[1] + across[1] * a * half[0] + along[1] * b * half[1],
        ]
    };
    [
        corner(-1.0, -1.0),
        corner(1.0, -1.0),
        corner(1.0, 1.0),
        corner(-1.0, 1.0),
    ]
}

/// The way in, in the site's frame: the strip `half` metres either side of
/// the line from the arrival point `arrival` to the yard's edge, `yard`
/// metres from the centre on the same line.
fn way_in(arrival: [f64; 2], yard: f64, half: f64) -> Option<[[f64; 2]; 4]> {
    let d = (arrival[0] * arrival[0] + arrival[1] * arrival[1]).sqrt();
    if d <= yard {
        return None;
    }
    let u = [arrival[0] / d, arrival[1] / d];
    let mid = f64::midpoint(d, yard);
    Some(rectangle(
        [u[0] * mid, u[1] * mid],
        u,
        [half, (d - yard) * 0.5],
    ))
}

/// How well an arrangement keeps the way in: 2 when the strip from the
/// arrival to the yard is clear, 1 when only the arrival point's own
/// neighbourhood is, 0 when a building stands on it.
fn way_in_score(arrangement: &SiteArrangement, arrival: [f64; 2], yard: f64, half: f64) -> u32 {
    let footprints = || arrangement.buildings.iter().map(|b| &b.footprint);
    let widened = half + WAY_IN_MARGIN_M;
    let clear = |strip: &[[f64; 2]; 4]| footprints().all(|f| !quads_overlap(strip, f));
    match way_in(arrival, yard, widened) {
        Some(strip) if clear(&strip) => 2,
        _ => {
            let d = (arrival[0] * arrival[0] + arrival[1] * arrival[1])
                .sqrt()
                .max(1e-9);
            let u = [arrival[0] / d, arrival[1] / d];
            let reach = widened + SIGN_INSET_M;
            let mouth = rectangle(
                [
                    arrival[0] - u[0] * reach * 0.5,
                    arrival[1] - u[1] * reach * 0.5,
                ],
                u,
                [widened, reach * 0.5],
            );
            u32::from(clear(&mouth))
        }
    }
}

/// Stands a site kind's building group on `site`, whose kind's road is of
/// `road` class: which arrangement, and where every building and the name
/// board stand in the world.
///
/// An arrangement that reaches past the site's pad (less
/// [`FIT_MARGIN_M`]) is never chosen. Of the rest, those that keep the way
/// in from the road's arrival to the yard clear are preferred, then those
/// that keep the arrival itself clear; among the best, the choice is seeded
/// by the site's id, so neighbouring sites of a kind differ and every
/// process makes the same choice. Each building stands at the pad's height
/// under its lowest corner less [`EMBED_M`]; the board stands just inside
/// the pad's edge beside the way in, facing out down the road, on whichever
/// side has room.
///
/// `None` when no arrangement fits the pad.
#[must_use]
pub fn place_group(
    site: &ProceduralSite,
    road: RoadClass,
    layout: &SiteGroupLayout,
) -> Option<SitePlacement> {
    let arrival = site.to_local_xz(site.arrival);
    let half = way_in_half_m(road);
    let fits = |a: &SiteArrangement| a.reach_m() <= site.radius_m - FIT_MARGIN_M;
    let scored: Vec<(usize, u32)> = layout
        .arrangements
        .iter()
        .enumerate()
        .filter(|(_, a)| fits(a))
        .map(|(i, a)| (i, way_in_score(a, arrival, layout.yard_m, half)))
        .collect();
    let best = scored.iter().map(|&(_, score)| score).max()?;
    let candidates: Vec<usize> = scored
        .iter()
        .filter(|&&(_, score)| score == best)
        .map(|&(i, _)| i)
        .collect();
    let mut rng = Rng::new(site.id.key() ^ ARRANGEMENT_SALT);
    let n = u32::try_from(candidates.len()).expect("a handful of arrangements");
    let chosen = candidates[rng.below(n) as usize];
    let arrangement = &layout.arrangements[chosen];
    let stand = |file: &str, position: [f64; 2], yaw: f64, footprint: &[[f64; 2]; 4]| {
        let world = footprint.map(|corner| site.to_world_xz(corner));
        let low = world
            .iter()
            .map(|[x, z]| site.pad_height_at(*x, *z))
            .fold(f64::INFINITY, f64::min);
        let [x, z] = site.to_world_xz(position);
        PlacedSiteBuilding {
            file: file.to_owned(),
            at: [x, low - EMBED_M, z],
            yaw: site.world_yaw(yaw),
            footprint: world,
        }
    };
    let buildings: Vec<PlacedSiteBuilding> = arrangement
        .buildings
        .iter()
        .map(|b| stand(&b.file, b.position, b.yaw, &b.footprint))
        .collect();
    let sign = layout.sign.as_ref().and_then(|sign| {
        let d = (arrival[0] * arrival[0] + arrival[1] * arrival[1]).sqrt();
        if d <= layout.yard_m {
            return None;
        }
        let u = [arrival[0] / d, arrival[1] / d];
        let across = [u[1], -u[0]];
        let back = SIGN_INSET_M + sign.half_size[1];
        let aside = half + WAY_IN_MARGIN_M + SIGN_ASIDE_M + sign.half_size[0];
        // Its face, `+z`, out along the way in, towards whoever arrives.
        let yaw = libm::atan2(u[0], u[1]);
        [1.0, -1.0].into_iter().find_map(|side: f64| {
            let centre = [
                arrival[0] - u[0] * back + across[0] * aside * side,
                arrival[1] - u[1] * back + across[1] * aside * side,
            ];
            let footprint = rectangle(centre, u, sign.half_size);
            let inside = footprint
                .iter()
                .all(|[x, z]| (x * x + z * z).sqrt() <= site.radius_m - FIT_MARGIN_M);
            let free = arrangement
                .buildings
                .iter()
                .all(|b| !quads_overlap(&footprint, &b.footprint));
            (inside && free).then(|| stand(&sign.file, centre, yaw, &footprint))
        })
    });
    Some(SitePlacement {
        arrangement: chosen,
        way_in_clear: best == 2,
        buildings,
        sign,
    })
}

/// How many roots [`name`] draws before it takes the last one cut to nine
/// letters.
const NAME_TRIES: u32 = 256;

/// A name from a kind's rules: a root of syllables in one of its patterns
/// (the feature's own patterns when the site stands by one that has some).
/// Pure in `rng`, so a site's name is a function of the seed and its id.
pub(crate) fn name(names: &NamesDef, patterns: &[String], rng: &mut Rng) -> String {
    let pick = |list: &[String], rng: &mut Rng| -> String {
        let n = u32::try_from(list.len()).expect("a short list");
        list[rng.below(n) as usize].clone()
    };
    let mut tries = 0;
    let root = loop {
        tries += 1;
        let span = names.syllables[1] - names.syllables[0] + 1;
        let count = names.syllables[0] + rng.below(span);
        let mut root = String::new();
        for _ in 0..count {
            let mut onset = pick(&names.onsets, rng);
            // No letter doubled across a syllable's join ("Tallla").
            if root.ends_with(|c: char| onset.starts_with(c)) {
                onset = pick(&names.onsets, rng);
            }
            root.push_str(&onset);
            root.push_str(&pick(&names.vowels, rng));
        }
        let coda = pick(&names.codas, rng);
        if !root.ends_with(|c: char| coda.starts_with(c)) {
            root.push_str(&coda);
        }
        // Long enough to be a name and short enough to sit on a sign and a
        // map.
        if (4..=9).contains(&root.len()) {
            break root;
        }
        // The registry refuses rules that can make no such root, but one
        // that only rarely does must still end the bake: past the bound the
        // last root is cut to length. No shipped rule comes near it.
        if tries >= NAME_TRIES {
            root.truncate(9);
            break root;
        }
    };
    let mut chars = root.chars();
    let root: String = chars
        .next()
        .map(|c| c.to_ascii_uppercase())
        .into_iter()
        .chain(chars)
        .collect();
    pick(patterns, rng).replace("{root}", &root)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pad() -> Pad {
        Pad {
            centre: [100.0, -50.0],
            radius: 40.0,
            blend: 30.0,
            level: 12.0,
            crown: 0.02,
        }
    }

    /// The height and the carried gradient agree with differences, inside
    /// the pad, across its edge and across the band, over sloping ground.
    #[test]
    #[allow(
        clippy::many_single_char_names,
        reason = "a point, the ground's height and gradient, the step"
    )]
    fn the_pad_is_continuous_with_its_gradient() {
        let pad = pad();
        // Ground rising east at 0.2 with a ripple.
        let ground = |x: f64, z: f64| (5.0 + 0.2 * x + 0.001 * z * z, (0.2, 0.002 * z));
        let at = |x: f64, z: f64| {
            let (h, g) = ground(x, z);
            pad.shape(x, z, h, g).map_or(h, |s| s.height)
        };
        let mut prev = None;
        let mut d = 0.0;
        while d < 80.0 {
            let (x, z) = (100.0 + d * 0.8, -50.0 + d * 0.6);
            let (h, g) = ground(x, z);
            let shaped = pad.shape(x, z, h, g);
            let height = shaped.map_or(h, |s| s.height);
            let gradient = shaped.map_or(g, |s| s.gradient);
            let e = 1e-4;
            let gx = (at(x + e, z) - at(x - e, z)) / (2.0 * e);
            let gz = (at(x, z + e) - at(x, z - e)) / (2.0 * e);
            assert!(
                (gx - gradient.0).abs() < 1e-5 && (gz - gradient.1).abs() < 1e-5,
                "at {d} m the gradient {gradient:?} against ({gx}, {gz})"
            );
            if let Some(p) = prev {
                let step: f64 = height - p;
                // The band meets ground 25 m above the pad over 30 m, 1.25
                // a metre at its steepest: 0.0625 m a step.
                assert!(step.abs() < 0.07, "a step of {step} m at {d} m");
            }
            prev = Some(height);
            d += 0.05;
        }
    }

    /// Level inside but for the crown, the ground's own past the band, and
    /// developed only near the pad.
    #[test]
    fn the_pad_is_level_and_ends_at_its_band() {
        let pad = pad();
        let s = pad
            .shape(100.0, -50.0, 30.0, (0.3, 0.0))
            .expect("on the pad");
        assert!((s.height - 12.0).abs() < 1e-12 && (s.developed - 1.0).abs() < 1e-12);
        let edge = pad
            .shape(140.0, -50.0, 30.0, (0.3, 0.0))
            .expect("on the edge");
        assert!(
            (edge.height - (12.0 - 0.02 * 20.0)).abs() < 1e-9,
            "{edge:?}"
        );
        assert!(pad.shape(170.5, -50.0, 30.0, (0.3, 0.0)).is_none());
        let band = pad
            .shape(165.0, -50.0, 30.0, (0.3, 0.0))
            .expect("in the band");
        assert!(band.developed.abs() < 1e-12 && band.height > 12.0 && band.height < 30.0);
    }

    /// A site with its pad at `(1000, -500)`, facing `(0.6, 0.8)`, its road
    /// arriving at `arrival` in its own frame.
    fn site(arrival: [f64; 2], node: i32) -> ProceduralSite {
        let mut site = ProceduralSite {
            id: SiteId {
                kind: SiteKindId(1),
                mass: [3, 4],
                node: [node, 6],
            },
            name: "Test".to_owned(),
            centre: [1000.0, -500.0],
            level_m: 20.0,
            radius_m: 30.0,
            blend_m: 30.0,
            crown: 0.02,
            facing: [0.6, 0.8],
            arrival: [0.0; 2],
            by: None,
            score: 1.0,
            cut_m: 0.0,
        };
        site.arrival = site.to_world_xz(arrival);
        site
    }

    /// A 10 m square building standing with its middle at `(x, z)`.
    fn square(file: &str, x: f64, z: f64) -> SiteBuilding {
        SiteBuilding {
            file: file.to_owned(),
            position: [x, z],
            yaw: 0.0,
            footprint: [
                [x - 5.0, z - 5.0],
                [x + 5.0, z - 5.0],
                [x + 5.0, z + 5.0],
                [x - 5.0, z + 5.0],
            ],
        }
    }

    /// Two arrangements: one across the back of the yard, one across its
    /// front.
    fn layout() -> SiteGroupLayout {
        SiteGroupLayout {
            group: "test:group".to_owned(),
            yard_m: 6.0,
            arrangements: vec![
                SiteArrangement {
                    name: "back".to_owned(),
                    buildings: vec![square("back", 0.0, -14.0)],
                },
                SiteArrangement {
                    name: "front".to_owned(),
                    buildings: vec![square("front", 0.0, 14.0)],
                },
            ],
            sign: Some(SiteSign {
                file: "board".to_owned(),
                half_size: [1.4, 0.4],
            }),
        }
    }

    /// The frame turns as `Quat::from_rotation_y(yaw)` turns, about the
    /// pad's centre, and back.
    #[test]
    fn the_site_frame_is_the_facing_turned() {
        let s = site([0.0, -30.0], 1);
        let [x, z] = s.to_world_xz([0.0, 10.0]);
        assert!((x - 1006.0).abs() < 1e-12 && (z + 492.0).abs() < 1e-12);
        let (sin, cos) = s.yaw().sin_cos();
        let local = [3.0, -7.0];
        let world = s.to_world_xz(local);
        let turned = [
            local[0] * cos + local[1] * sin,
            -local[0] * sin + local[1] * cos,
        ];
        assert!((world[0] - 1000.0 - turned[0]).abs() < 1e-12);
        assert!((world[1] + 500.0 - turned[1]).abs() < 1e-12);
        let back = s.to_local_xz(world);
        assert!((back[0] - 3.0).abs() < 1e-12 && (back[1] + 7.0).abs() < 1e-12);
        assert!((s.world_yaw(0.25) - s.yaw() - 0.25).abs() < 1e-15);
    }

    /// A road arriving from behind gets the arrangement with the front row,
    /// one arriving in front the back row, and both keep the way in clear.
    #[test]
    fn the_arrangement_keeps_the_way_in() {
        let layout = layout();
        let from_back =
            place_group(&site([0.0, -30.0], 1), RoadClass::Track, &layout).expect("a placement");
        assert_eq!((from_back.arrangement, from_back.way_in_clear), (1, true));
        let from_front =
            place_group(&site([0.0, 30.0], 1), RoadClass::Track, &layout).expect("a placement");
        assert_eq!((from_front.arrangement, from_front.way_in_clear), (0, true));
        // From the side both are clear, and the site's id chooses.
        let chosen: std::collections::BTreeSet<usize> = (0..32)
            .map(|node| {
                place_group(&site([30.0, 0.0], node), RoadClass::Track, &layout)
                    .expect("a placement")
                    .arrangement
            })
            .collect();
        assert_eq!(chosen.len(), 2, "neighbours of a kind differ");
    }

    /// The same site and layout stand the same group, bit for bit, however
    /// many times it is asked: the server and a client agree.
    #[test]
    fn a_placement_is_a_function_of_the_site() {
        let layout = layout();
        for node in 0..16 {
            let s = site([21.0, -21.0], node);
            assert_eq!(
                place_group(&s, RoadClass::Service, &layout),
                place_group(&s.clone(), RoadClass::Service, &layout.clone())
            );
        }
    }

    /// An arrangement reaching past the pad is never chosen, and a layout
    /// with nothing that fits stands nothing.
    #[test]
    fn nothing_stands_past_the_pad() {
        let mut layout = layout();
        layout.arrangements[1].buildings[0] = square("front", 0.0, 26.0);
        for node in 0..16 {
            let placed = place_group(&site([30.0, 0.0], node), RoadClass::Track, &layout)
                .expect("the back row fits");
            assert_eq!(placed.arrangement, 0);
        }
        layout.arrangements[0].buildings[0] = square("back", 0.0, -26.0);
        assert!(place_group(&site([30.0, 0.0], 1), RoadClass::Track, &layout).is_none());
    }

    /// Every building stands just into the pad under its lowest corner, and
    /// the board stands beside the way in, inside the pad, facing out down
    /// the road.
    #[test]
    fn buildings_and_board_stand_on_the_pad() {
        let s = site([-21.0, -21.0], 3);
        let placed = place_group(&s, RoadClass::Track, &layout()).expect("a placement");
        for b in placed.all() {
            let heights: Vec<f64> = b
                .footprint
                .iter()
                .map(|[x, z]| s.pad_height_at(*x, *z))
                .collect();
            let low = heights.iter().copied().fold(f64::INFINITY, f64::min);
            assert!((b.at[1] - (low - EMBED_M)).abs() < 1e-12, "{b:?}");
            // No corner floats: the pad is at least the embedding above it
            // everywhere under the building.
            assert!(
                heights.iter().all(|h| *h >= b.at[1] + EMBED_M - 1e-12),
                "{heights:?}"
            );
        }
        let sign = placed.sign.expect("room for the board");
        let local = s.to_local_xz([sign.at[0], sign.at[2]]);
        assert!(local[0].hypot(local[1]) < s.radius_m - FIT_MARGIN_M);
        let strip = way_in(
            s.to_local_xz(s.arrival),
            6.0,
            way_in_half_m(RoadClass::Track),
        )
        .expect("a way in");
        let board = sign.footprint.map(|corner| s.to_local_xz(corner));
        assert!(!quads_overlap(&strip, &board));
        // Out along the way in: towards the arrival from the centre.
        let out = libm::atan2(-1.0, -1.0) + s.yaw();
        let turn = (sign.yaw - out).rem_euclid(std::f64::consts::TAU);
        assert!(
            turn < 1e-9 || std::f64::consts::TAU - turn < 1e-9,
            "{}",
            sign.yaw
        );
    }

    #[test]
    fn a_name_is_its_patterns_root() {
        let names = NamesDef {
            onsets: vec!["v".into(), "s".into()],
            vowels: vec!["e".into()],
            codas: vec![String::new(), "n".into()],
            syllables: [2, 2],
            patterns: vec!["{root} Ford".into()],
            by_feature: Vec::new(),
        };
        let mut rng = Rng::new(7);
        let a = name(&names, &names.patterns, &mut rng);
        assert!(a.ends_with(" Ford") && a.starts_with(['V', 'S']), "{a}");
        let mut again = Rng::new(7);
        assert_eq!(a, name(&names, &names.patterns, &mut again));
    }

    /// Only the yard is laid in gravel: all of it inside [`LAID_FROM`] of
    /// the radius, none at the edge, where the pad is still developed so
    /// nothing grows; the threshold that breaks the gravel's edge up stays
    /// inside its band and moves over a few metres.
    #[test]
    fn the_yard_is_gravel_and_the_rest_of_the_pad_is_cleared() {
        let pad = pad();
        let at = |d: f64| {
            pad.shape(pad.centre[0] + d, pad.centre[1], 12.0, (0.0, 0.0))
                .expect("on the pad")
        };
        assert!((at(0.0).laid - 1.0).abs() < 1e-12);
        assert!((at(LAID_FROM * pad.radius).laid - 1.0).abs() < 1e-12);
        let edge = at(pad.radius);
        assert!(edge.laid.abs() < 1e-12 && (edge.developed - 1.0).abs() < 1e-12);
        let (mut least, mut most) = (1.0_f64, 0.0_f64);
        for k in 0..400 {
            let t = gravel_threshold(f64::from(k) * 0.37 - 50.0, f64::from(k) * -0.91 + 20.0);
            least = least.min(t);
            most = most.max(t);
        }
        assert!(
            least > 0.15 && most < 0.85 && most - least > 0.3,
            "{least} {most}"
        );
    }

    /// Rules that can only make a root too short or too long still give a
    /// name, cut to length, instead of drawing forever.
    #[test]
    fn rules_that_cannot_make_a_root_still_end() {
        let short = NamesDef {
            onsets: vec!["k".into()],
            vowels: vec!["a".into()],
            codas: vec![String::new()],
            syllables: [1, 1],
            patterns: vec!["{root}".into()],
            by_feature: Vec::new(),
        };
        assert_eq!(name(&short, &short.patterns, &mut Rng::new(3)), "Ka");
        let long = NamesDef {
            onsets: vec!["stra".into()],
            vowels: vec!["eiou".into()],
            syllables: [4, 4],
            ..short
        };
        let cut = name(&long, &long.patterns, &mut Rng::new(3));
        assert_eq!(cut.len(), 9, "{cut}");
    }
}
