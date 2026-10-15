//! Settlements in the world function: a city put where its registry entry
//! says, standing on its own landform, terraced, with a quay, level platforms
//! and a road out into the wilderness (ADR 0009).
//!
//! The author places a city; the world makes room. Inside a site's extent the
//! ground is the settlement's own landform (a hillside profile bent into a
//! bay, with spurs, gullies and relief seeded by the settlement), blended over
//! whatever the world's noise made there across the falloff. So the city's
//! ground is a function of its registry entry alone: the generator can change
//! and every building still stands where the content step fitted it, which
//! [`Site::fingerprint`] lets a loader check.
//!
//! Neighbourhood leases cut finite level benches into that landform. Each
//! has its own step sequence, sometimes omitting a level to make a deeper
//! court; their rounded banks end in uncut gullies. Ground between the
//! waterline and the first terrace lies at the
//! quay's height, which leaves a drop at the water for the quay wall.
//! Platforms are level rounded rectangles cut and filled out of the terraces,
//! for what needs flat ground: pads, a square, the processor. The retaining
//! walls, stairs and quay wall are geometry the content step lays over this.
//!
//! It stays a pure function of the seed and the registry, so a server's
//! heightfield agrees with a client's with no extra data on the wire. Every
//! sample away from a site pays one bounds test. The per-sample arithmetic is
//! adds, multiplies, `floor` and `sqrt`; the frame's bearing and the road's
//! headings go through `libm` once, when the site is placed.

use serde::{Deserialize, Serialize};
use strata_registry::SettlementDef;

/// A placed site: where the settlement frame stands and how the ground is
/// shaped in it.
#[derive(Debug, Clone, PartialEq)]
pub struct Site {
    /// The settlement's id.
    pub id: String,
    /// The frame's origin, world metres `(x, z)`.
    pub centre: [f64; 2],
    /// The frame's `+z`, downhill towards the water, as a world unit vector
    /// `(x, z)`.
    pub downhill: [f64; 2],
    /// The city's ground at the origin, metres.
    pub height: f64,
    extent: Extent,
    landform: Landform,
    terraces: Terraces,
    /// The platforms, with their levels.
    pub platforms: Vec<Platform>,
    /// The road out.
    pub road: Road,
    /// Everything the site touches, world `[min_x, min_z, max_x, max_z]`.
    bounds: [f64; 4],
    /// A hash of everything that shapes the city's ground.
    fingerprint: u64,
}

#[derive(Debug, Clone, Copy, PartialEq)]
struct Extent {
    half_width: f64,
    uphill: f64,
    downhill: f64,
    falloff: f64,
}

/// The settlement's own ground, in its frame, before terracing.
#[derive(Debug, Clone, PartialEq)]
struct Landform {
    profile: Vec<[f64; 2]>,
    bay: f64,
    half_width: f64,
    /// `(x, half_width, rise)`.
    spurs: Vec<[f64; 3]>,
    relief: f64,
    wavelength: f64,
    seed: u64,
}

/// The terraces' levels, uneven, from the quay up.
#[derive(Debug, Clone, PartialEq)]
struct Terraces {
    quay: f64,
    wall: f64,
    /// Every level from the quay's up, strictly increasing.
    levels: Vec<f64>,
    /// Separate leases stop at gullies instead of cutting a contour across
    /// the entire hillside. Each owns its own sequence of level benches.
    leases: Vec<Lease>,
}

#[derive(Debug, Clone, PartialEq)]
struct Lease {
    from: f64,
    to: f64,
    levels: Vec<f64>,
    seed: u64,
}

/// A level platform, in the settlement frame.
#[derive(Debug, Clone, PartialEq)]
pub struct Platform {
    /// What stands on it.
    pub id: String,
    /// Its centre in the frame, `(x, z)`.
    pub at: [f64; 2],
    /// Half its size along the frame's `x` and `z`.
    pub half_size: [f64; 2],
    /// The rounding of its corners.
    pub corner: f64,
    /// The blend back to the terraces.
    pub falloff: f64,
    /// Its level, metres.
    pub height: f64,
}

/// A road: a polyline over the ground with its own gentle height profile.
#[derive(Debug, Clone, PartialEq)]
pub struct Road {
    /// `[x, z, height]`, world metres, from the city outward.
    pub points: Vec<[f64; 3]>,
    /// Half the carriageway's width, metres.
    pub half_width: f64,
    /// The blend from the carriageway back to the ground beside it.
    pub shoulder: f64,
}

/// A sample's ground after the site has shaped it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Shaped {
    /// Metres.
    pub height: f64,
    /// The height's gradient, `(d/dx, d/dz)`.
    pub gradient: (f64, f64),
    /// How much of the natural ground is left, `0` inside the city, `1`
    /// outside the site: vegetation and cover scale by it.
    pub natural: f64,
    /// Whether the point is on the road's carriageway.
    pub road: bool,
    /// What kind of city ground the point is, where the city owns it.
    pub ground: Option<GroundKind>,
}

/// A kind of city ground, which a settlement lays in its own terrain
/// material (`strata_registry::GroundDef`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GroundKind {
    /// A terrace's level ground.
    Lane,
    /// The quay and the platforms.
    Apron,
    /// A slope: a bank between levels.
    Bank,
}

/// The road's step between traced points, metres.
const ROAD_STEP_M: f64 = 25.0;
/// The shoulder either side of a carriageway, metres.
const ROAD_SHOULDER_M: f64 = 8.0;
/// The span the landform's gradient is measured over, metres.
const GRADIENT_SPAN_M: f64 = 0.5;
/// How much further the relief's features run down the slope than across
/// it: spurs and gullies, not blobs.
const RELIEF_STRETCH: f64 = 2.5;
/// Bump when the shaping algorithm changes, so an old bake is refused even
/// when its registry file has not changed.
const GROUND_REVISION: u64 = 2;

/// A settlement's layout as the content step baked it: which building files
/// stand where in the settlement's frame, read by the game and the server
/// alike.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SettlementLayout {
    /// The settlement's id.
    pub id: String,
    /// The [`Site::fingerprint`] of the ground it was fitted to. A building
    /// stands on the terraces that ground made, so a settlement whose ground
    /// changed cannot use it; the world's generator is not part of it.
    pub ground: u64,
    /// Every building.
    pub buildings: Vec<LayoutBuilding>,
    /// A point a walker can start from, settlement frame, `y` world height.
    pub spawn: [f64; 3],
    /// A point just inside a lobby, reachable from `spawn`.
    pub lobby: [f64; 3],
    /// The lamps and floodlights whose light falls on the streets at night.
    #[serde(default)]
    pub lights: Vec<LayoutLight>,
    /// The trees and bushes the plan planted, of the wilderness's own
    /// species.
    #[serde(default)]
    pub plants: Vec<LayoutPlant>,
    /// The whole settlement as one baked file, relative to the content
    /// step's output, for past the distance its buildings stream to. Placed
    /// as one building standing at the frame's origin ([`Site::origin`],
    /// height zero, so its heights are world heights) turned by the frame's
    /// [`Site::yaw`]. Not one of [`Self::buildings`], which the physics and
    /// the server build colliders from: it draws, and collides with nothing.
    /// `None` in a layout baked before it existed.
    #[serde(default)]
    pub skyline: Option<String>,
}

/// A plant of a [`SettlementLayout`]: a species from the flora registry
/// planted where the plan put it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LayoutPlant {
    /// The flora registry id, `strata:oak`.
    pub species: String,
    /// The root in the settlement frame; `y` is the world height.
    pub position: [f64; 3],
    /// Rotation about the vertical, in turns.
    pub yaw: f32,
    /// Size multiplier within the species' range.
    pub scale: f32,
    /// Which of its variants.
    pub variant: u8,
}

/// A light of a [`SettlementLayout`]: a point light a client places at
/// night, near the observer.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LayoutLight {
    /// In the settlement frame; `y` is the world height.
    pub position: [f64; 3],
    /// Linear RGB.
    pub color: [f32; 3],
    /// Luminous power, lumens.
    pub lumens: f32,
    /// How far it reaches, metres.
    pub range: f32,
}

/// One building of a [`SettlementLayout`].
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LayoutBuilding {
    /// The baked `.ashlar` file, relative to the content step's output.
    pub file: String,
    /// Its origin in the settlement frame; `y` is the world height.
    pub position: [f64; 3],
    /// Radians about `+y`, in the settlement frame.
    pub yaw: f64,
    /// `[min_x, min_z, max_x, max_z]` in the settlement frame.
    pub footprint: [f64; 4],
}

/// The natural height at a world point `(x, z)`.
pub type Ground<'a> = &'a dyn Fn(f64, f64) -> f64;

impl Site {
    /// Place the settlement `def` describes, level its platforms and trace
    /// its road over the world's `natural` ground beyond it.
    ///
    /// Nothing is searched: the author put the city where it is, and the
    /// ground inside it is the settlement's own, so only the road reads the
    /// world.
    #[must_use]
    pub fn place(def: &SettlementDef, natural: Ground<'_>) -> Self {
        let bearing = def.downhill_deg.to_radians();
        let downhill = [libm::sin(bearing), libm::cos(bearing)];
        let l = &def.landform;
        let landform = Landform {
            profile: l.profile.clone(),
            bay: l.bay_m,
            half_width: def.extent.half_width_m,
            spurs: l
                .spurs
                .iter()
                .map(|s| [s.x_m, s.half_width_m, s.rise_m])
                .collect(),
            relief: l.relief_m,
            wavelength: l.relief_wavelength_m,
            seed: l.seed,
        };
        let t = &def.terraces;
        let levels = terrace_levels(def);
        let leases = terrace_leases(def, &levels);
        let mut site = Self {
            id: def.id.to_string(),
            centre: def.at,
            downhill,
            height: 0.0,
            extent: Extent {
                half_width: def.extent.half_width_m,
                uphill: def.extent.uphill_m,
                downhill: def.extent.downhill_m,
                falloff: def.extent.falloff_m,
            },
            landform,
            terraces: Terraces {
                quay: t.quay_m,
                wall: t.wall,
                levels,
                leases,
            },
            platforms: Vec::new(),
            road: Road {
                points: Vec::new(),
                half_width: def.road.width_m * 0.5,
                shoulder: ROAD_SHOULDER_M,
            },
            bounds: [0.0; 4],
            fingerprint: 0,
        };
        site.platforms = def
            .platforms
            .iter()
            .map(|p| {
                // Where the design does not fix it, the mean height of the
                // landform over its footprint, never below the quay.
                let level = p.level_m.unwrap_or_else(|| {
                    let mut sum = 0.0;
                    for a in -2..=2 {
                        for b in -2..=2 {
                            sum += site.landform.height([
                                p.at[0] + f64::from(a) * p.half_size[0] * 0.45,
                                p.at[1] + f64::from(b) * p.half_size[1] * 0.45,
                            ]);
                        }
                    }
                    (sum / 25.0).max(t.quay_m)
                });
                Platform {
                    id: p.id.clone(),
                    at: p.at,
                    half_size: p.half_size,
                    corner: p.corner_m,
                    falloff: p.falloff_m,
                    height: level,
                }
            })
            .collect();
        site.height = site.city_ground([0.0, 0.0]);
        site.fingerprint = site.hash();
        // The road starts at its exit at whatever height the city leaves it,
        // and follows the ground as the city blends it back into the world's.
        let exit = site.to_world_xz(def.road.exit);
        let blended = |x: f64, z: f64| {
            site.shape_without_road(x, z, natural(x, z), (0.0, 0.0))
                .height
        };
        let road = Road::trace(
            def,
            exit,
            [exit[0] - site.centre[0], exit[1] - site.centre[1]],
            blended(exit[0], exit[1]),
            &blended,
        );
        site.road = road;
        site.bounds = site.reach();
        site
    }

    /// A hash of everything that shapes the city's ground: the frame, the
    /// extent, the landform, the terraces and the platforms. A layout fitted
    /// to one ground is fitted to every site with the same fingerprint,
    /// whatever the world's generator does outside it.
    #[must_use]
    pub fn fingerprint(&self) -> u64 {
        self.fingerprint
    }

    fn hash(&self) -> u64 {
        let mut h = Fnv::default();
        h.bytes(&GROUND_REVISION.to_le_bytes());
        h.floats(&self.centre);
        h.floats(&self.downhill);
        let e = &self.extent;
        h.floats(&[e.half_width, e.uphill, e.downhill, e.falloff]);
        let l = &self.landform;
        for k in &l.profile {
            h.floats(k);
        }
        for s in &l.spurs {
            h.floats(s);
        }
        h.floats(&[l.bay, l.half_width, l.relief, l.wavelength]);
        h.bytes(&l.seed.to_le_bytes());
        h.floats(&[self.terraces.quay, self.terraces.wall]);
        h.floats(&self.terraces.levels);
        for p in &self.platforms {
            h.bytes(strata_core::stable_label(&p.id).as_bytes());
            h.floats(&p.at);
            h.floats(&p.half_size);
            h.floats(&[p.corner, p.falloff, p.height]);
        }
        h.0
    }

    /// A point of the settlement frame `(x, z)` in the world.
    #[must_use]
    pub fn to_world_xz(&self, local: [f64; 2]) -> [f64; 2] {
        let [dx, dz] = self.downhill;
        // +z is downhill; +x is downhill turned a quarter clockwise seen
        // from above.
        [
            self.centre[0] + local[0] * dz + local[1] * dx,
            self.centre[1] - local[0] * dx + local[1] * dz,
        ]
    }

    /// A world point `(x, z)` in the settlement frame.
    #[must_use]
    pub fn to_local_xz(&self, world: [f64; 2]) -> [f64; 2] {
        let [dx, dz] = self.downhill;
        let (wx, wz) = (world[0] - self.centre[0], world[1] - self.centre[1]);
        [wx * dz - wz * dx, wx * dx + wz * dz]
    }

    /// The frame's rotation about `+y`, radians: what a yaw given in the
    /// frame adds to become a world yaw, as `Quat::from_rotation_y` turns.
    #[must_use]
    pub fn yaw(&self) -> f64 {
        libm::atan2(self.downhill[0], self.downhill[1])
    }

    /// The settlement frame's origin in the world, at height zero: layout
    /// heights are world heights.
    #[must_use]
    pub fn origin(&self) -> [f64; 3] {
        [self.centre[0], 0.0, self.centre[1]]
    }

    /// A point of the settlement frame in the world, `x` and `z` in the
    /// frame and `y` a world height. With [`Self::world_yaw`], the one
    /// placement every consumer of a layout stands a building by: the game,
    /// the server's colliders and the content step's skyline test.
    #[must_use]
    pub fn to_world(&self, local: [f64; 3]) -> [f64; 3] {
        let [x, z] = self.to_world_xz([local[0], local[2]]);
        [x, local[1], z]
    }

    /// A yaw given in the settlement frame as a world yaw, about `+y` as
    /// `Quat::from_rotation_y` turns.
    #[must_use]
    pub fn world_yaw(&self, local: f64) -> f64 {
        local + self.yaw()
    }

    /// Whether `(x, z)` is anywhere the site shapes: the one test every
    /// sample away from a city pays.
    #[must_use]
    pub fn touches(&self, x: f64, z: f64) -> bool {
        x >= self.bounds[0] && x <= self.bounds[2] && z >= self.bounds[1] && z <= self.bounds[3]
    }

    /// The level a landform height is terraced to, and how fast it moves
    /// with that height: `0` on a terrace's flat, large on its wall.
    #[must_use]
    pub fn terrace(&self, height: f64) -> (f64, f64) {
        let t = &self.terraces;
        if height < 0.0 {
            // The sea: the quay wall drops into it.
            return (height, 1.0);
        }
        if height < t.quay {
            return (t.quay, 0.0);
        }
        let i = t.levels.partition_point(|&l| l <= height);
        if i >= t.levels.len() {
            return (t.levels[t.levels.len() - 1], 0.0);
        }
        let (lo, hi) = (t.levels[i - 1], t.levels[i]);
        let f = (height - lo) / (hi - lo);
        let (ramp, slope) = smoothstep_with_slope(1.0 - t.wall, 1.0, f);
        (lo + (hi - lo) * ramp, slope)
    }

    /// Whether the city's own ground reaches a frame point `(x, z)` whole,
    /// with none of the world's blended in: where a layout may build.
    #[must_use]
    pub fn owns(&self, local: [f64; 2]) -> bool {
        self.extent_distance(local) <= 0.0
    }

    /// The signed distance from the extent's rounded rectangle at a world
    /// point `(x, z)`, metres, negative inside: the ground the river
    /// network routes round (ADR 0010, decision 6), since a river through
    /// the extent would move the ground the layout is fitted to.
    #[must_use]
    pub fn extent_distance_at(&self, x: f64, z: f64) -> f64 {
        self.extent_distance(self.to_local_xz([x, z]))
    }

    /// The signed distance from the extent's rounded rectangle, negative
    /// inside.
    fn extent_distance(&self, local: [f64; 2]) -> f64 {
        let e = &self.extent;
        let half = [e.half_width, f64::midpoint(e.uphill, e.downhill)];
        let mid = [0.0, (e.downhill - e.uphill) * 0.5];
        rounded_rect(
            [local[0] - mid[0], local[1] - mid[1]],
            half,
            e.falloff.min(half[0]).min(half[1]),
        )
    }

    /// The city's ground at a frame point `(x, z)`: the terraced landform and
    /// the platforms, as it is everywhere inside the extent. This is what a
    /// layout is fitted to; the world's own ground never reaches it.
    #[must_use]
    pub fn city_ground(&self, local: [f64; 2]) -> f64 {
        self.city_height(local)
    }

    /// The city's ground at a frame point and its gradient in the frame.
    fn city(&self, local: [f64; 2]) -> (f64, (f64, f64)) {
        let d = GRADIENT_SPAN_M;
        let gx = (self.city_height([local[0] + d, local[1]])
            - self.city_height([local[0] - d, local[1]]))
            / (2.0 * d);
        let gz = (self.city_height([local[0], local[1] + d])
            - self.city_height([local[0], local[1] - d]))
            / (2.0 * d);
        (self.city_height(local), (gx, gz))
    }

    /// A bench's ends feather into the uncut hillside. Flats remain exactly
    /// level for fitting lots; crests and toes share a smooth bank profile.
    fn leased_ground(&self, [x, z]: [f64; 2]) -> f64 {
        let raw = self.landform.height([x, z]);
        if raw <= self.terraces.quay {
            // The shore retains its established level.
            return self.terrace(raw).0;
        }
        let pass = smoothstep_with_slope(24.0, 50.0, z + self.extent.uphill).0;
        let original = self.terrace(raw).0;
        let keep_pass = |height| {
            if pass >= 1.0 {
                height
            } else {
                original + (height - original) * pass
            }
        };
        let bend = 12.0 * value_noise(self.landform.seed ^ 0x6265_6e64, x / 160.0, z / 95.0);
        let across = x + bend;
        let Some(lease) = self
            .terraces
            .leases
            .iter()
            .find(|l| across >= l.from && across < l.to)
        else {
            return keep_pass(raw);
        };
        let i = lease.levels.partition_point(|&level| level <= raw);
        if i == 0 || i == lease.levels.len() {
            return keep_pass(raw);
        }
        let (low, high) = (lease.levels[i - 1], lease.levels[i]);
        let key = hash3(lease.seed, low.to_bits(), high.to_bits());
        // Different leases omit different intermediate levels. The next
        // bench then widens into a court rather than another parallel row.
        let bank = self.terraces.wall + 0.03 + 0.07 * unit(key);
        let fraction = (raw - low) / (high - low);
        let ramp = smoothstep_with_slope(1.0 - bank, 1.0, fraction).0;
        let level = low + (high - low) * ramp;
        let end = (across - lease.from).min(lease.to - across);
        let strength = smoothstep_with_slope(2.0, 10.0, end).0;
        // No manufactured stripes continue past the developed core into
        // the large blend ring visible from the hillside.
        let edge = smoothstep_with_slope(0.0, 24.0, self.extent.half_width - x.abs()).0;
        let shaped = raw + (level - raw) * strength * edge;
        keep_pass(shaped)
    }

    fn city_height(&self, local: [f64; 2]) -> f64 {
        let mut height = self.leased_ground(local);
        for platform in &self.platforms {
            let dist = rounded_rect(
                [local[0] - platform.at[0], local[1] - platform.at[1]],
                platform.half_size,
                platform.corner,
            );
            if dist >= platform.falloff {
                continue;
            }
            let (t, _) = smoothstep_with_slope(0.0, platform.falloff, dist);
            height = platform.height + (height - platform.height) * t;
        }
        height
    }

    /// The ground at `(x, z)` after the site shapes the natural `height`
    /// with its `gradient`, or `None` where the site does not reach.
    #[must_use]
    pub fn shape(&self, x: f64, z: f64, height: f64, gradient: (f64, f64)) -> Option<Shaped> {
        if !self.touches(x, z) {
            return None;
        }
        let mut shaped = self.shape_without_road(x, z, height, gradient);
        // On the city's own ground the city lays its streets; the road only
        // shapes the ground from the city's edge outward.
        if shaped.natural > 0.5
            && let Some(on_road) = self.road.nearest(x, z)
        {
            let w = self.road.half_width;
            if on_road.distance < w + self.road.shoulder {
                let (t, _) = smoothstep_with_slope(w, w + self.road.shoulder, on_road.distance);
                shaped.height = on_road.height + (shaped.height - on_road.height) * t;
                shaped.gradient = (
                    on_road.gradient.0 + (shaped.gradient.0 - on_road.gradient.0) * t,
                    on_road.gradient.1 + (shaped.gradient.1 - on_road.gradient.1) * t,
                );
                shaped.natural = shaped.natural.min(t);
                shaped.road = on_road.distance < w;
            }
        }
        (shaped
            != Shaped {
                height,
                gradient,
                natural: 1.0,
                road: false,
                ground: None,
            })
        .then_some(shaped)
    }

    /// The city's ground blended over the natural one, without the road.
    fn shape_without_road(&self, x: f64, z: f64, height: f64, gradient: (f64, f64)) -> Shaped {
        let local = self.to_local_xz([x, z]);
        let distance = self.extent_distance(local);
        let inside = 1.0 - smoothstep_with_slope(0.0, self.extent.falloff, distance).0;
        if inside <= 0.0 {
            return Shaped {
                height,
                gradient,
                natural: 1.0,
                road: false,
                ground: None,
            };
        }
        let (city, (lx, lz)) = self.city(local);
        // The frame's gradient in world axes.
        let [dx, dz] = self.downhill;
        let world = (lx * dz + lz * dx, -lx * dx + lz * dz);
        let keep = 1.0 - inside;
        Shaped {
            height: height * keep + city * inside,
            gradient: (
                gradient.0 * keep + world.0 * inside,
                gradient.1 * keep + world.1 * inside,
            ),
            natural: keep,
            road: false,
            ground: (distance <= 0.0).then(|| self.ground_kind(local, city, (lx, lz))),
        }
    }

    /// What kind of city ground a frame point with the city's height `city`
    /// and gradient is: the quay and the platforms are aprons, a level
    /// terrace is a lane, anything sloping is a bank.
    fn ground_kind(&self, local: [f64; 2], city: f64, _: (f64, f64)) -> GroundKind {
        // By bands, not by a slope threshold alone, whose flicker at the top
        // and foot of a ramp leaves single vertices of one material among
        // another, which the terrain draws as fans of teeth: a platform's
        // own rectangle is apron, its whole falloff ring is the bank down
        // (or up) to the terraces, and only elsewhere does the measured
        // slope tell a terrace's lane from its bank.
        let platform = self
            .platforms
            .iter()
            .map(|p| {
                (
                    rounded_rect(
                        [local[0] - p.at[0], local[1] - p.at[1]],
                        p.half_size,
                        p.corner,
                    ),
                    p.falloff,
                )
            })
            .fold((f64::INFINITY, 0.0), |a, b| if b.0 < a.0 { b } else { a });
        if platform.0 <= 0.0 {
            return GroundKind::Apron;
        }
        if platform.0 <= platform.1 {
            return GroundKind::Bank;
        }
        // The slope of the ground as shaped, measured: the terraces' own
        // gradient is zero on every level run.
        let d = 0.75;
        let dx =
            self.city_ground([local[0] + d, local[1]]) - self.city_ground([local[0] - d, local[1]]);
        let dz =
            self.city_ground([local[0], local[1] + d]) - self.city_ground([local[0], local[1] - d]);
        let grade2 = (dx * dx + dz * dz) / (4.0 * d * d);
        if grade2 > 0.04 * 0.04 {
            GroundKind::Bank
        } else if city >= 0.0 && (city - self.terraces.quay).abs() < 1e-3 {
            GroundKind::Apron
        } else {
            GroundKind::Lane
        }
    }

    /// The world bounds of the ground the city owns whole, `[min_x, min_z,
    /// max_x, max_z]`: where its buildings stand.
    #[must_use]
    pub fn owned_bounds(&self) -> [f64; 4] {
        let e = &self.extent;
        let mut bounds = [
            f64::INFINITY,
            f64::INFINITY,
            f64::NEG_INFINITY,
            f64::NEG_INFINITY,
        ];
        for corner in [
            [-e.half_width, -e.uphill],
            [e.half_width, -e.uphill],
            [-e.half_width, e.downhill],
            [e.half_width, e.downhill],
        ] {
            let [x, z] = self.to_world_xz(corner);
            bounds = [
                bounds[0].min(x),
                bounds[1].min(z),
                bounds[2].max(x),
                bounds[3].max(z),
            ];
        }
        bounds
    }

    /// The world bounds of the extent, its falloff, and the road.
    fn reach(&self) -> [f64; 4] {
        let e = &self.extent;
        let mut bounds = [
            f64::INFINITY,
            f64::INFINITY,
            f64::NEG_INFINITY,
            f64::NEG_INFINITY,
        ];
        let mut include = |[x, z]: [f64; 2], pad: f64| {
            bounds[0] = bounds[0].min(x - pad);
            bounds[1] = bounds[1].min(z - pad);
            bounds[2] = bounds[2].max(x + pad);
            bounds[3] = bounds[3].max(z + pad);
        };
        for corner in [
            [-e.half_width, -e.uphill],
            [e.half_width, -e.uphill],
            [-e.half_width, e.downhill],
            [e.half_width, e.downhill],
        ] {
            include(self.to_world_xz(corner), e.falloff);
        }
        let pad = self.road.half_width + self.road.shoulder;
        for p in &self.road.points {
            include([p[0], p[1]], pad);
        }
        bounds
    }
}

impl Landform {
    /// The landform's height at a frame point, before terracing.
    #[allow(
        clippy::many_single_char_names,
        reason = "frame x, z and the height h read as the geometry does"
    )]
    fn height(&self, [x, z]: [f64; 2]) -> f64 {
        let mut h = profile_at(&self.profile, z);
        let across = x / self.half_width;
        h += self.bay * across * across;
        for &[at, half, rise] in &self.spurs {
            let u = (x - at) / half;
            if u.abs() < 1.0 {
                let bump = 1.0 - u * u;
                h += rise * bump * bump;
            }
        }
        let (u, v) = (x / self.wavelength, z / (self.wavelength * RELIEF_STRETCH));
        h + self.relief
            * (value_noise(self.seed, u, v) + 0.5 * value_noise(self.seed ^ 1, 2.0 * u, 2.0 * v))
            / 1.5
    }
}

/// The terraces' levels from the quay up past the highest ground the
/// landform can reach, each rise jittered by the settlement's seed.
fn terrace_levels(def: &SettlementDef) -> Vec<f64> {
    let (t, l) = (&def.terraces, &def.landform);
    let top = l
        .profile
        .iter()
        .map(|k| k[1])
        .fold(f64::NEG_INFINITY, f64::max)
        + l.bay_m.max(0.0)
        + l.spurs.iter().map(|s| s.rise_m.max(0.0)).sum::<f64>()
        + l.relief_m.abs() * 2.0;
    let mut levels = vec![t.quay_m];
    let mut i = 0_u64;
    while levels[levels.len() - 1] < top + t.step_m {
        let jitter = unit(hash3(l.seed ^ 0x7465_7272, i, 0)) * 2.0 - 1.0;
        levels.push(levels[levels.len() - 1] + t.step_m * (1.0 + t.step_jitter * jitter));
        i += 1;
    }
    levels
}

/// Neighbourhoods cut at different times: unequal widths, independently
/// stepped levels, and occasional double-depth benches for courts.
fn terrace_leases(def: &SettlementDef, levels: &[f64]) -> Vec<Lease> {
    let half = def.extent.half_width_m;
    let mut leases = Vec::new();
    let mut from = -half;
    let mut index = 0_u64;
    while from < half {
        let seed = hash3(def.landform.seed ^ 0x6c65_6173, index, 0);
        let to = (from + 88.0 + 48.0 * unit(seed)).min(half);
        let mut own = vec![levels[0]];
        for (i, &height) in levels.iter().enumerate().skip(1) {
            let key = hash3(seed, i as u64, 1);
            if i % 2 == 0 && i + 1 < levels.len() && unit(key) < 0.42 {
                continue;
            }
            own.push(height + (unit(hash3(seed, i as u64, 2)) - 0.5) * 1.8);
        }
        leases.push(Lease {
            from,
            to,
            levels: own,
            seed,
        });
        from = to;
        index += 1;
    }
    leases
}

/// The profile's height at `z`: linear between knots, held past the ends.
fn profile_at(profile: &[[f64; 2]], z: f64) -> f64 {
    let i = profile.partition_point(|k| k[0] <= z);
    if i == 0 {
        return profile[0][1];
    }
    if i == profile.len() {
        return profile[i - 1][1];
    }
    let ([z0, h0], [z1, h1]) = (profile[i - 1], profile[i]);
    h0 + (h1 - h0) * (z - z0) / (z1 - z0)
}

/// Value noise in `-1..1` on the integer lattice, smoothstep-interpolated:
/// hashes, adds and multiplies, the same bits on every platform.
#[allow(
    clippy::cast_possible_truncation,
    reason = "lattice indices of a city's few hundred metres"
)]
fn value_noise(seed: u64, x: f64, z: f64) -> f64 {
    let (fx, fz) = (x.floor(), z.floor());
    let (ix, iz) = (fx as i64, fz as i64);
    let (tx, tz) = (x - fx, z - fz);
    let (sx, sz) = (tx * tx * (3.0 - 2.0 * tx), tz * tz * (3.0 - 2.0 * tz));
    #[allow(clippy::cast_sign_loss, reason = "a lattice index's bits, hashed")]
    let at = |i: i64, k: i64| unit(hash3(seed, i as u64, k as u64)) * 2.0 - 1.0;
    let a = at(ix, iz) + (at(ix + 1, iz) - at(ix, iz)) * sx;
    let b = at(ix, iz + 1) + (at(ix + 1, iz + 1) - at(ix, iz + 1)) * sx;
    a + (b - a) * sz
}

/// Three words into one, every input bit reaching every output bit.
fn hash3(a: u64, b: u64, c: u64) -> u64 {
    let mut h = a ^ 0x9e37_79b9_7f4a_7c15;
    for word in [b, c] {
        h = (h ^ word).wrapping_mul(0xbf58_476d_1ce4_e5b9);
        h ^= h >> 31;
        h = h.wrapping_mul(0x94d0_49bb_1331_11eb);
        h ^= h >> 29;
    }
    h
}

/// A hash's top 53 bits as a float in `0..1`.
#[allow(clippy::cast_precision_loss, reason = "53 bits, exactly")]
fn unit(h: u64) -> f64 {
    (h >> 11) as f64 / (1_u64 << 53) as f64
}

/// FNV-1a, for a fingerprint that is the same on every platform and every
/// build.
struct Fnv(u64);

impl Default for Fnv {
    fn default() -> Self {
        Self(0xcbf2_9ce4_8422_2325)
    }
}

impl Fnv {
    fn bytes(&mut self, bytes: &[u8]) {
        for &b in bytes {
            self.0 ^= u64::from(b);
            self.0 = self.0.wrapping_mul(0x0000_0100_0000_01b3);
        }
    }

    fn floats(&mut self, values: &[f64]) {
        for v in values {
            self.bytes(&v.to_bits().to_le_bytes());
        }
    }
}

/// The signed distance from a rounded rectangle centred at the origin with
/// half sizes `half` and corner radius `r`: negative inside.
fn rounded_rect(p: [f64; 2], half: [f64; 2], r: f64) -> f64 {
    let qx = p[0].abs() - (half[0] - r);
    let qz = p[1].abs() - (half[1] - r);
    let outside = (qx.max(0.0) * qx.max(0.0) + qz.max(0.0) * qz.max(0.0)).sqrt();
    outside + qx.max(qz).min(0.0) - r
}

/// `smoothstep(a, b, x)` and its derivative in `x`.
#[allow(
    clippy::many_single_char_names,
    reason = "a curve's ends and its argument, as the maths names them"
)]
fn smoothstep_with_slope(a: f64, b: f64, x: f64) -> (f64, f64) {
    let u = ((x - a) / (b - a)).clamp(0.0, 1.0);
    let t = u * u * (3.0 - 2.0 * u);
    let dt = if u > 0.0 && u < 1.0 {
        6.0 * u * (1.0 - u) / (b - a)
    } else {
        0.0
    };
    (t, dt)
}

/// The nearest point of a road to a sample.
struct OnRoad {
    distance: f64,
    height: f64,
    gradient: (f64, f64),
}

impl Road {
    /// Trace a road from `exit` outward along `away`: each step tries a few
    /// headings around the current one and takes the gentlest grade that
    /// still leads away, then the heights are smoothed into a profile a
    /// cart could climb, easing out of `start`.
    #[allow(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        clippy::cast_precision_loss,
        reason = "a few dozen steps and headings"
    )]
    fn trace(
        def: &SettlementDef,
        exit: [f64; 2],
        away: [f64; 2],
        start: f64,
        height: Ground<'_>,
    ) -> Self {
        let len = (away[0] * away[0] + away[1] * away[1]).sqrt().max(1e-6);
        let outward = (away[0] / len, away[1] / len);
        let mut heading = libm::atan2(outward.1, outward.0);
        let mut at = exit;
        let mut points = vec![[at[0], at[1], start]];
        let steps = (def.road.length_m / ROAD_STEP_M).ceil() as usize;
        let mut last = start;
        let progress = |p: [f64; 2]| (p[0] - exit[0]) * outward.0 + (p[1] - exit[1]) * outward.1;
        for _ in 0..steps {
            let mut best: Option<(f64, f64, [f64; 2], f64)> = None;
            for turn in [-0.5_f64, -0.25, 0.0, 0.25, 0.5] {
                let h = heading + turn;
                let (s, c) = (libm::sin(h), libm::cos(h));
                let next = [at[0] + c * ROAD_STEP_M, at[1] + s * ROAD_STEP_M];
                let ground = height(next[0], next[1]);
                let mut cost = (ground - last).abs() / ROAD_STEP_M + 0.02 * turn.abs();
                if ground < 1.0 {
                    cost += 10.0;
                }
                if progress(next) < progress(at) {
                    cost += 1.0;
                }
                if best.is_none_or(|(b, ..)| cost < b) {
                    best = Some((cost, h, next, ground));
                }
            }
            let Some((_, h, next, ground)) = best else {
                break;
            };
            heading = h;
            at = next;
            last = ground;
            points.push([at[0], at[1], ground.max(1.0)]);
        }
        // Smooth the profile; the first few points ease out of the city's
        // height, so the road leaves the city level.
        let raw: Vec<f64> = points.iter().map(|p| p[2]).collect();
        for (k, point) in points.iter_mut().enumerate().skip(1) {
            let lo = k.saturating_sub(3);
            let hi = (k + 3).min(raw.len() - 1);
            let window = &raw[lo..=hi];
            let smooth = window.iter().sum::<f64>() / window.len() as f64;
            let ease = (k as f64 / 4.0).min(1.0);
            point[2] = start + (smooth - start) * ease;
        }
        Self {
            points,
            half_width: def.road.width_m * 0.5,
            shoulder: ROAD_SHOULDER_M,
        }
    }

    /// The nearest point of the carriageway's centreline to `(x, z)`, with
    /// the road's height and gradient there, when it is within reach.
    #[allow(
        clippy::many_single_char_names,
        reason = "coordinates and a segment's ends, as the maths names them"
    )]
    fn nearest(&self, x: f64, z: f64) -> Option<OnRoad> {
        let reach = self.half_width + self.shoulder;
        let mut best: Option<OnRoad> = None;
        for pair in self.points.windows(2) {
            let [a, b] = [pair[0], pair[1]];
            let (ex, ez) = (b[0] - a[0], b[1] - a[1]);
            if (x < a[0].min(b[0]) - reach)
                || (x > a[0].max(b[0]) + reach)
                || (z < a[1].min(b[1]) - reach)
                || (z > a[1].max(b[1]) + reach)
            {
                continue;
            }
            let len2 = ex * ex + ez * ez;
            if len2 < 1e-9 {
                continue;
            }
            let s = (((x - a[0]) * ex + (z - a[1]) * ez) / len2).clamp(0.0, 1.0);
            let (px, pz) = (a[0] + ex * s, a[1] + ez * s);
            let distance = ((x - px) * (x - px) + (z - pz) * (z - pz)).sqrt();
            if best.as_ref().is_none_or(|b| distance < b.distance) {
                let rise = (b[2] - a[2]) / len2;
                best = Some(OnRoad {
                    distance,
                    height: a[2] + (b[2] - a[2]) * s,
                    gradient: (rise * ex, rise * ez),
                });
            }
        }
        best
    }
}

#[cfg(test)]
mod tests {
    use strata_registry::{ExtentDef, LandformDef, PlatformDef, RoadDef, SpurDef, TerracesDef};

    use super::*;

    fn def() -> SettlementDef {
        SettlementDef {
            id: strata_registry::Namespaced::parse("strata:test").expect("valid"),
            name: "Test".to_owned(),
            at: [0.0, 0.0],
            downhill_deg: 90.0,
            extent: ExtentDef {
                half_width_m: 200.0,
                uphill_m: 300.0,
                downhill_m: 200.0,
                falloff_m: 50.0,
            },
            landform: LandformDef {
                profile: vec![[-300.0, 90.0], [0.0, 30.0], [150.0, 0.0], [250.0, -10.0]],
                bay_m: 6.0,
                spurs: vec![SpurDef {
                    x_m: 0.0,
                    half_width_m: 80.0,
                    rise_m: 5.0,
                }],
                relief_m: 3.0,
                relief_wavelength_m: 100.0,
                seed: 7,
            },
            terraces: TerracesDef {
                quay_m: 4.0,
                step_m: 6.0,
                step_jitter: 0.3,
                wall: 0.15,
            },
            platforms: vec![PlatformDef {
                id: "hub".to_owned(),
                at: [0.0, 0.0],
                half_size: [60.0, 40.0],
                corner_m: 10.0,
                falloff_m: 20.0,
                level_m: None,
            }],
            road: RoadDef {
                exit: [0.0, -300.0],
                length_m: 400.0,
                width_m: 7.0,
            },
            layout: "settlements/test.ron".to_owned(),
            ground: None,
        }
    }

    /// Some world: a hillside roughly like the city's, rising west, tilted.
    fn ground(x: f64, z: f64) -> f64 {
        30.0 - x * 0.2 + z * 0.05
    }

    /// Another world, flat, for the city's ground not to notice.
    fn flat(_x: f64, _z: f64) -> f64 {
        12.0
    }

    #[test]
    fn a_site_stands_where_it_was_put_facing_its_bearing() {
        let site = Site::place(&def(), &ground);
        assert_eq!(site.centre.map(f64::to_bits), [0.0_f64.to_bits(); 2]);
        assert!((site.downhill[0] - 1.0).abs() < 1e-12 && site.downhill[1].abs() < 1e-12);
        // The frame round-trips.
        let local = [37.0, -12.0];
        let back = site.to_local_xz(site.to_world_xz(local));
        assert!((back[0] - local[0]).abs() < 1e-9 && (back[1] - local[1]).abs() < 1e-9);
        assert_eq!(Site::place(&def(), &ground), site);
    }

    /// A building placed by `to_world` and turned by `world_yaw` is the
    /// frame rotated about `+y` as `Quat::from_rotation_y` turns: its own
    /// `+x` and `+z` land on the frame's, so a file in the frame's
    /// coordinates (the skyline) placed at the origin with no yaw draws
    /// where each building does.
    #[test]
    fn a_placement_is_the_frame_turned_about_up() {
        let mut site = Site::place(&def(), &ground);
        site.centre = [-400.0, -500.0];
        let (sin, cos) = 0.87_f64.sin_cos();
        site.downhill = [sin, cos];
        let origin = site.to_world([0.0, 7.0, 0.0]);
        let bits = |p: [f64; 3]| p.map(f64::to_bits);
        assert_eq!(bits(origin), bits([-400.0, 7.0, -500.0]));
        assert_eq!(bits(site.origin()), bits([-400.0, 0.0, -500.0]));
        // from_rotation_y(a) takes (x, 0, z) to
        // (x cos a + z sin a, 0, z cos a - x sin a).
        let turn = |a: f64, [x, z]: [f64; 2]| {
            let (s, c) = a.sin_cos();
            [x * c + z * s, z * c - x * s]
        };
        for local in [[1.0, 0.0], [0.0, 1.0], [37.0, -12.0]] {
            let placed = site.to_world([local[0], 0.0, local[1]]);
            let turned = turn(site.world_yaw(0.0), local);
            assert!(
                (placed[0] - origin[0] - turned[0]).abs() < 1e-9
                    && (placed[2] - origin[2] - turned[1]).abs() < 1e-9,
                "{local:?}: placed {placed:?}, turned {turned:?}"
            );
        }
        assert!((site.world_yaw(0.25) - site.yaw() - 0.25).abs() < 1e-12);
    }

    #[test]
    fn the_city_ground_is_the_settlements_own_whatever_the_world() {
        let one = Site::place(&def(), &ground);
        let other = Site::place(&def(), &flat);
        assert_eq!(one.fingerprint(), other.fingerprint());
        for local in [[0.0, 0.0], [150.0, -250.0], [-150.0, 150.0], [80.0, 60.0]] {
            let [x, z] = one.to_world_xz(local);
            assert!(one.owns(local), "{local:?}");
            let a = one.shape(x, z, ground(x, z), (-0.2, 0.05)).expect("inside");
            let b = other.shape(x, z, flat(x, z), (0.0, 0.0)).expect("inside");
            // The same bits, not merely close.
            assert_eq!(a.height.to_bits(), b.height.to_bits(), "{local:?}");
            assert_eq!(a.height.to_bits(), one.city_ground(local).to_bits());
        }
        let mut moved = def();
        moved.landform.relief_m = 3.5;
        assert_ne!(
            Site::place(&moved, &ground).fingerprint(),
            one.fingerprint()
        );
    }

    #[test]
    fn the_city_ground_knows_its_lanes_aprons_and_banks() {
        let site = Site::place(&def(), &ground);
        let kind = |local: [f64; 2]| {
            let [x, z] = site.to_world_xz(local);
            site.shape(x, z, ground(x, z), (-0.2, 0.05))
                .and_then(|s| s.ground)
        };
        // The hub platform is an apron.
        assert_eq!(kind([0.0, 0.0]), Some(GroundKind::Apron));
        // Up the hillside, terraces and their banks.
        let kinds: Vec<_> = (0..120)
            .map(|i| kind([-150.0, -250.0 + f64::from(i)]))
            .collect();
        assert!(kinds.contains(&Some(GroundKind::Lane)), "{kinds:?}");
        assert!(kinds.contains(&Some(GroundKind::Bank)), "{kinds:?}");
    }

    #[test]
    fn terraces_are_level_and_step_unevenly_at_their_walls() {
        let site = Site::place(&def(), &ground);
        let levels = &site.terraces.levels;
        let rises: Vec<f64> = levels.windows(2).map(|w| w[1] - w[0]).collect();
        assert!(rises.iter().all(|r| (4.2..=7.8).contains(r)), "{rises:?}");
        assert!(
            rises.windows(2).any(|w| (w[0] - w[1]).abs() > 0.3),
            "{rises:?}"
        );
        let (lo, hi) = (levels[2], levels[3]);
        // Mid-terrace: level, and flat.
        let (level, rate) = site.terrace(lo + (hi - lo) * 0.3);
        assert!((level - lo).abs() < 1e-9 && rate.abs() < 1e-9);
        // At a wall: between two levels, and steep.
        let (level, rate) = site.terrace(lo + (hi - lo) * 0.95);
        assert!(level > lo && level < hi && rate > 1.0);
        // Between the waterline and the first terrace: the quay.
        assert_eq!(site.terrace(2.0), (4.0, 0.0));
    }

    #[test]
    fn lease_ends_are_continuous_and_do_not_stripe_the_blend_ring() {
        let site = Site::place(&def(), &ground);
        for z in [-230.0, -180.0, -120.0] {
            for ix in -220..=220 {
                let x = f64::from(ix);
                let left = site.city_ground([x - 0.001, z]);
                let right = site.city_ground([x + 0.001, z]);
                assert!(
                    (right - left).abs() < 0.02,
                    "a seam at {x},{z}: {left} to {right}"
                );
            }
            for x in [-230.0, 230.0] {
                assert_eq!(
                    site.city_ground([x, z]).to_bits(),
                    site.landform.height([x, z]).to_bits()
                );
            }
        }
    }

    #[test]
    fn the_pass_blends_continuously_at_the_outer_lease_ends() {
        let site = Site::place(&def(), &ground);
        for z in [-288.0, -264.0] {
            for boundary in [-200.0, 200.0] {
                let (mut lo, mut hi) = (boundary - 20.0, boundary + 20.0);
                for _ in 0..40 {
                    let x = f64::midpoint(lo, hi);
                    let bend =
                        12.0 * value_noise(site.landform.seed ^ 0x6265_6e64, x / 160.0, z / 95.0);
                    if x + bend < boundary {
                        lo = x;
                    } else {
                        hi = x;
                    }
                }
                let x = f64::midpoint(lo, hi);
                assert!(
                    (site.city_ground([x - 0.001, z]) - site.city_ground([x + 0.001, z])).abs()
                        < 0.02,
                    "outer pass seam at {x},{z}"
                );
            }
        }
    }

    #[test]
    fn independent_neighbourhoods_keep_flats_without_a_hillside_wide_level() {
        let site = Site::place(&def(), &ground);
        let mut flats = 0;
        let mut longest = 0;
        for z in [-210.0, -170.0, -130.0] {
            let mut run = 0;
            for ix in -190..190 {
                let x = f64::from(ix);
                if (site.city_ground([x, z]) - site.city_ground([x + 1.0, z])).abs() < 1e-8 {
                    run += 1;
                    flats += 1;
                    longest = longest.max(run);
                } else {
                    run = 0;
                }
            }
        }
        assert!(flats > 150, "only {flats} metres of buildable flats");
        assert!(longest < 150, "a {longest} m continuous band");
    }

    #[test]
    fn leased_ground_is_bit_identical_across_sampling_threads() {
        let site = Site::place(&def(), &ground);
        let sample = || {
            (-100..100)
                .map(|i| {
                    site.city_ground([f64::from(i) * 1.83, -175.0 + f64::from(i) * 0.13])
                        .to_bits()
                })
                .collect::<Vec<_>>()
        };
        let expected = sample();
        std::thread::scope(|scope| {
            let jobs: Vec<_> = (0..4).map(|_| scope.spawn(sample)).collect();
            for job in jobs {
                assert_eq!(job.join().expect("sample thread"), expected);
            }
        });
    }

    #[test]
    fn the_hub_platform_is_level_across_its_footprint() {
        let site = Site::place(&def(), &ground);
        let hub = &site.platforms[0];
        for local in [[0.0, 0.0], [40.0, 20.0], [-45.0, -25.0]] {
            let [x, z] = site.to_world_xz(local);
            let shaped = site
                .shape(x, z, ground(x, z), (-0.2, 0.05))
                .expect("inside");
            assert!((shaped.height - hub.height).abs() < 1e-9, "{local:?}");
            assert!(shaped.natural < 1e-9);
        }
    }

    #[test]
    fn the_road_leaves_level_and_climbs_gently() {
        let site = Site::place(&def(), &ground);
        let road = &site.road;
        assert!(road.points.len() > 5);
        for pair in road.points.windows(2) {
            let grade = (pair[1][2] - pair[0][2]).abs() / ROAD_STEP_M;
            assert!(grade < 0.25, "a steep road: {grade}");
        }
    }

    #[test]
    fn a_layout_round_trips_through_ron() {
        let layout = SettlementLayout {
            id: "strata:test".to_owned(),
            ground: 42,
            buildings: vec![LayoutBuilding {
                file: "settlements/test/core.ashlar".to_owned(),
                position: [1.0, 12.0, -2.0],
                yaw: 0.5,
                footprint: [-10.0, -10.0, 10.0, 10.0],
            }],
            spawn: [0.0, 16.0, 20.0],
            lobby: [0.0, 16.0, 5.0],
            lights: vec![LayoutLight {
                position: [1.0, 15.0, 2.0],
                color: [1.0, 0.7, 0.4],
                lumens: 1500.0,
                range: 14.0,
            }],
            plants: vec![LayoutPlant {
                species: "strata:oak".to_owned(),
                position: [3.0, 15.0, 4.0],
                yaw: 0.25,
                scale: 1.0,
                variant: 2,
            }],
            skyline: Some("settlements/test.skyline.ashlar".to_owned()),
        };
        let text = ron::to_string(&layout).expect("serialises");
        assert_eq!(
            ron::from_str::<SettlementLayout>(&text).expect("reads"),
            layout
        );
    }

    /// A layout written before the skyline existed still loads, with none.
    #[test]
    fn a_layout_without_a_skyline_still_reads() {
        let text = r#"(
            id: "strata:test",
            ground: 42,
            buildings: [(
                file: "settlements/test/core.ashlar",
                position: (1.0, 12.0, -2.0),
                yaw: 0.5,
                footprint: (-10.0, -10.0, 10.0, 10.0),
            )],
            spawn: (0.0, 16.0, 20.0),
            lobby: (0.0, 16.0, 5.0),
        )"#;
        let layout: SettlementLayout = ron::from_str(text).expect("reads");
        assert_eq!(layout.skyline, None);
        assert_eq!(layout.buildings.len(), 1);
    }
}
