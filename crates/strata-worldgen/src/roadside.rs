//! What stands along a road (ticket 0005, step 8): marker posts on the
//! verges of the graded roads and lamps where a road arrives, at a site's
//! pad and at each end of a bridge. A pure function of the network's road,
//! the content step's kits and the world's ground, as [`place_crossing`] is
//! for a crossing, so the server and every client stand the same pieces and
//! nothing about them is replicated.
//!
//! - **Markers** stand on the verge's outer half, every
//!   [`marker_every_m`] along a service road (both sides) and a track (one
//!   side, the sides taking turns); a trail has none, and an authored
//!   settlement's own road is the settlement's to dress. None stands near a
//!   road's ends (a junction's mouth, a pad's way in), by a crossing (a
//!   ford's posts, a culvert's headwalls and a bridge's lamps mark those),
//!   in water, on another road's carriageway or a pad, or off the road's
//!   level (a bend whose verge the grade blended into a bank).
//! - **Lamps** stand past each end of a placed bridge's deck, either side,
//!   beyond the pilasters and the wing walls on the verge, unless that is
//!   in water or on another way (a junction's mouth by the bridge); and one
//!   on the verge where a road reaches a site's pad, on the pad's skirt,
//!   which is laid and developed by design and outside the pad's edge,
//!   where the site's buildings stand.
//!
//! Every station is measured along the road from its first end, so where a
//! piece stands does not depend on who asks or how much of the road they
//! asked about.

use serde::{Deserialize, Serialize};

use crate::WorldSource;
use crate::crossings::{
    CrossingId, CrossingKit, PlacedPiece, ProceduralCrossing, place_crossing, point_along,
};
use crate::roads::{CrossingKind, Road, RoadClass, RoadEnd};

/// Where the content step writes the roadside kit's layout, under its
/// output: beside the crossings' kit, whose directory its files share.
pub const ROADSIDE_PATH: &str = "crossings/veyra/roadside.ron";

/// The roadside kit as the content step baked it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RoadsideKit {
    /// A marker post's baked file, relative to the content step's output.
    pub marker: Option<String>,
    /// A lamp standard's baked file.
    pub lamp: Option<String>,
}

impl RoadsideKit {
    /// Every file the kit names, each once, in order.
    #[must_use]
    pub fn files(&self) -> Vec<String> {
        let mut files: Vec<String> = self.marker.iter().chain(&self.lamp).cloned().collect();
        files.sort();
        files.dedup();
        files
    }
}

/// A road's identity within a world version: its land mass's key and its
/// index in that network's roads.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct RoadId {
    /// The land mass's key ([`Network::key`](crate::hydrology::Network::key)).
    pub mass: [i32; 2],
    /// The road, an index into [`Network::roads`](crate::hydrology::Network::roads).
    pub road: u32,
}

impl RoadId {
    /// The id as one number, the same on every platform.
    #[must_use]
    pub fn key(&self) -> u64 {
        let mut hash = 0xcbf2_9ce4_8422_2325_u64 ^ 0x0d5e;
        for v in [self.mass[0], self.mass[1], self.road.cast_signed()] {
            for b in v.to_le_bytes() {
                hash ^= u64::from(b);
                hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
            }
        }
        hash
    }
}

/// Metres between a road's marker stations, by class; `None` for a class
/// that has none. A service road's every hundred metres, as a highway
/// authority sets them; a track's half again as far apart, one side at a
/// time.
#[must_use]
pub const fn marker_every_m(class: RoadClass) -> Option<f64> {
    match class {
        RoadClass::Service => Some(100.0),
        RoadClass::Track => Some(150.0),
        RoadClass::Trail => None,
    }
}

/// How far in from the verge's outer edge a marker stands, metres: on the
/// verge, clear of the carriageway's edge by a stride.
const MARKER_IN_M: f64 = 0.5;

/// No marker stands this near a road's end, metres: a junction's mouth and
/// a pad's way in are the other road's and the pad's.
const END_CLEAR_M: f64 = 30.0;

/// Nor this near a crossing's held stretch, metres: the crossing's own
/// pieces mark it.
const CROSSING_CLEAR_M: f64 = 15.0;

/// How far off the road's level a marker's ground may be, metres: further,
/// and the verge is a bank the grade blended into at a bend.
const LEVEL_MOST_M: f64 = 0.6;

/// Over this much of another way's carriageway or a pad, a station's
/// ground is not the road's verge.
const DEVELOPED_MOST: f32 = 0.3;

/// How far past a bridge's deck's end a lamp stands along the road, metres:
/// beyond the wing walls, which reach a little over four metres back from
/// the abutment's face and so end inside the deck's buried length.
const LAMP_PAST_DECK_M: f64 = 1.2;

/// How far out from the carriageway's edge a lamp stands, metres: on the
/// verge, past the deck's kerb and rails.
const LAMP_OUT_M: f64 = 1.5;

/// How far in from a road's end at a site a lamp stands, metres: on the way
/// in, short of the pad's board.
const LAMP_IN_FROM_PAD_M: f64 = 6.0;

/// The unit `(x, z)` the road runs at `along` metres from its first end.
fn direction_at(road: &Road, along: f64) -> [f64; 2] {
    let points = &road.points;
    if points.len() < 2 {
        return [0.0, 1.0];
    }
    let k = points
        .partition_point(|p| p.along_m <= along)
        .clamp(1, points.len() - 1);
    let (a, b) = (points[k - 1], points[k]);
    let (dx, dz) = (b.x - a.x, b.z - a.z);
    let length = (dx * dx + dz * dz).sqrt();
    if length > 1e-9 {
        [dx / length, dz / length]
    } else {
        [0.0, 1.0]
    }
}

/// The yaw that turns a piece's `+z` to the unit `(x, z)` `dir`.
fn yaw_of(dir: [f64; 2]) -> f64 {
    libm::atan2(dir[0], dir[1])
}

/// The road's crossings as the crossings' list gives them.
fn crossings_of(id: RoadId, road: &Road) -> Vec<ProceduralCrossing> {
    road.crossings
        .iter()
        .enumerate()
        .map(|(i, c)| ProceduralCrossing {
            id: CrossingId {
                mass: id.mass,
                road: id.road,
                index: u32::try_from(i).expect("a handful of crossings"),
            },
            class: road.class,
            crossing: *c,
            edges: c.abutments_m.map(|s| point_along(road, s)),
        })
        .collect()
}

/// Stands the roadside kit along road `id` of its network, reading
/// `ground` for each piece's footing and `crossings` (the crossings' kit)
/// for the bridges whose ends the lamps mark; without it no bridge has
/// lamps. In order along the road, the markers first and the lamps after,
/// so a piece's index in the list is the same for everyone who asks.
#[must_use]
pub fn place_roadside(
    id: RoadId,
    road: &Road,
    kit: &RoadsideKit,
    crossings: Option<&CrossingKit>,
    ground: &dyn WorldSource,
) -> Vec<PlacedPiece> {
    if road.is_settlements() || road.points.len() < 2 {
        return Vec::new();
    }
    let mut pieces = Vec::new();
    if let Some(marker) = &kit.marker {
        markers(road, marker, ground, &mut pieces);
    }
    if let Some(lamp) = &kit.lamp {
        if let Some(crossings) = crossings {
            bridge_lamps(id, road, lamp, crossings, ground, &mut pieces);
        }
        pad_lamps(road, lamp, ground, &mut pieces);
    }
    pieces
}

/// The marker posts along `road`.
fn markers(road: &Road, file: &str, ground: &dyn WorldSource, out: &mut Vec<PlacedPiece>) {
    let Some(every) = marker_every_m(road.class) else {
        return;
    };
    let way = crate::hydrology::grade::way(road.class);
    let out_m = way.half_m + way.verge_m - MARKER_IN_M;
    let length = road.length_m();
    let held: Vec<[f64; 2]> = road
        .crossings
        .iter()
        .map(|c| {
            [
                c.abutments_m[0].min(c.along_m) - CROSSING_CLEAR_M,
                c.abutments_m[1].max(c.along_m) + CROSSING_CLEAR_M,
            ]
        })
        .collect();
    let mut k = 0_u32;
    loop {
        let along = every * (f64::from(k) + 0.5);
        if along > length - END_CLEAR_M {
            break;
        }
        let station = k;
        k += 1;
        if along < END_CLEAR_M || held.iter().any(|[a, b]| along >= *a && along <= *b) {
            continue;
        }
        let at = point_along(road, along);
        let dir = direction_at(road, along);
        let across = [dir[1], -dir[0]];
        let level = road.level_at(along).map(|(level, _)| level);
        let sides: &[f64] = match road.class {
            RoadClass::Service => &[1.0, -1.0],
            _ if station.is_multiple_of(2) => &[1.0],
            _ => &[-1.0],
        };
        for &side in sides {
            let p = [
                at[0] + across[0] * out_m * side,
                at[1] + across[1] * out_m * side,
            ];
            let here = ground.sample(p[0], p[1]);
            let height = f64::from(here.height);
            if here.in_water()
                || here.landform.developed > DEVELOPED_MOST
                || level.is_some_and(|level| (height - level).abs() > LEVEL_MOST_M)
            {
                continue;
            }
            // The reflector faces the traffic coming up that side.
            let facing = [-dir[0] * side, -dir[1] * side];
            out.push(PlacedPiece {
                file: file.to_owned(),
                at: [p[0], height, p[1]],
                yaw: yaw_of(facing),
            });
        }
    }
}

/// A lamp either side past each end of every bridge on `road` that stands a
/// deck, its arm over the road.
fn bridge_lamps(
    id: RoadId,
    road: &Road,
    file: &str,
    kit: &CrossingKit,
    ground: &dyn WorldSource,
    out: &mut Vec<PlacedPiece>,
) {
    if road.class == RoadClass::Trail {
        return;
    }
    let out_m = crate::sites::way_in_half_m(road.class) + LAMP_OUT_M;
    for c in crossings_of(id, road) {
        if c.crossing.kind != CrossingKind::Bridge {
            continue;
        }
        let Ok(placed) = place_crossing(&c, kit, ground) else {
            continue;
        };
        let Some(deck) = placed.first() else {
            continue;
        };
        let Some(class) = kit.bridges.iter().find(|b| b.file == deck.file) else {
            continue;
        };
        let dir = [libm::sin(deck.yaw), libm::cos(deck.yaw)];
        let across = [dir[1], -dir[0]];
        let reach = 0.5 * class.length_m + LAMP_PAST_DECK_M;
        for end in [-1.0, 1.0] {
            for side in [-1.0, 1.0] {
                let p = [
                    deck.at[0] + dir[0] * reach * end + across[0] * out_m * side,
                    deck.at[2] + dir[1] * reach * end + across[1] * out_m * side,
                ];
                let here = ground.sample(p[0], p[1]);
                if here.in_water() || here.landform.developed > DEVELOPED_MOST {
                    continue;
                }
                out.push(PlacedPiece {
                    file: file.to_owned(),
                    at: [p[0], f64::from(here.height), p[1]],
                    yaw: yaw_of([-across[0] * side, -across[1] * side]),
                });
            }
        }
    }
}

/// A lamp on the verge where `road` reaches a site's pad, on the right of
/// the way in, its arm over the road.
fn pad_lamps(road: &Road, file: &str, ground: &dyn WorldSource, out: &mut Vec<PlacedPiece>) {
    if road.class == RoadClass::Trail {
        return;
    }
    let way = crate::hydrology::grade::way(road.class);
    let out_m = way.half_m + way.verge_m - MARKER_IN_M;
    let length = road.length_m();
    for (end, road_end) in road.ends.iter().enumerate() {
        if !matches!(road_end, RoadEnd::Site(_)) || length < 2.0 * LAMP_IN_FROM_PAD_M {
            continue;
        }
        let (along, inward) = if end == 0 {
            (LAMP_IN_FROM_PAD_M, -1.0)
        } else {
            (length - LAMP_IN_FROM_PAD_M, 1.0)
        };
        let at = point_along(road, along);
        let dir = direction_at(road, along);
        // Driving towards the pad, its right (`y` up, so facing `-z` the
        // right is `+x`).
        let toward = [dir[0] * inward, dir[1] * inward];
        let right = [-toward[1], toward[0]];
        let p = [at[0] + right[0] * out_m, at[1] + right[1] * out_m];
        let here = ground.sample(p[0], p[1]);
        if here.in_water() {
            continue;
        }
        out.push(PlacedPiece {
            file: file.to_owned(),
            at: [p[0], f64::from(here.height), p[1]],
            yaw: yaw_of([-right[0], -right[1]]),
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::crossings::{BridgeClass, DeckEnd};
    use crate::roads::{Crossing, RoadPoint};
    use crate::sites::SiteId;
    use crate::{Landform, Sample};
    use strata_data::BiomeId;

    /// Level ground at 10 m, a river's water under `|z - 450| < 10`, and
    /// another way's carriageway over `x > 30`.
    struct Valley;

    impl WorldSource for Valley {
        fn sample(&self, x: f64, z: f64) -> Sample {
            let wet = (z - 450.0).abs() < 10.0;
            Sample {
                height: if wet { 8.0 } else { 10.0 },
                biomes: [(BiomeId(0), 1.0), (BiomeId(0), 0.0)],
                provinces: Sample::NO_PROVINCES,
                material: 0,
                moisture: 0.5,
                hydration: 0.5,
                cover: 0.5,
                landform: Landform {
                    developed: if x > 30.0 { 1.0 } else { 0.0 },
                    ..Landform::default()
                },
                water: wet.then_some(crate::SampleWater {
                    surface_m: 9.0,
                    bed_m: 8.0,
                    width_m: 16.0,
                    signed_distance_m: 0.0,
                    flow: [1.0, 0.0],
                    discharge_m3s: 10.0,
                    order: 2,
                    kind: crate::hydrology::WaterKind::Channel,
                    cut_dm: 0,
                }),
            }
        }

        fn sea_level(&self) -> f32 {
            0.0
        }
    }

    fn kit() -> RoadsideKit {
        RoadsideKit {
            marker: Some("marker.ashlar".to_owned()),
            lamp: Some("lamp.ashlar".to_owned()),
        }
    }

    /// A road of `class` up `+z` from the origin, `length` long, level at
    /// 10 m (graded unless a trail), from a site to a junction.
    fn road(class: RoadClass, length: f64) -> Road {
        let points: Vec<RoadPoint> = (0..=100)
            .map(|k| {
                let along = length * f64::from(k) / 100.0;
                RoadPoint {
                    x: 0.0,
                    z: along,
                    along_m: along,
                    ground_m: 10.0,
                }
            })
            .collect();
        let profile = if class == RoadClass::Trail {
            Vec::new()
        } else {
            vec![[10.0, 0.0]; points.len()]
        };
        Road {
            class,
            points,
            ends: [
                RoadEnd::Site(SiteId {
                    kind: strata_data::SiteKindId(0),
                    mass: [0, 0],
                    node: [0, 0],
                }),
                RoadEnd::Junction(0),
            ],
            crossings: Vec::new(),
            profile,
        }
    }

    const ID: RoadId = RoadId {
        mass: [0, 0],
        road: 4,
    };

    fn markers_of(pieces: &[PlacedPiece]) -> Vec<&PlacedPiece> {
        pieces
            .iter()
            .filter(|p| p.file == "marker.ashlar")
            .collect()
    }

    /// A service road's markers stand in pairs every hundred metres on the
    /// verge's outer half, on the ground, facing along the road; none near
    /// its ends; a track's one a side by turns every 150 m; a trail none.
    #[test]
    fn markers_stand_by_class_on_the_verge() {
        let service = road(RoadClass::Service, 1_000.0);
        let pieces = place_roadside(ID, &service, &kit(), None, &Valley);
        let markers = markers_of(&pieces);
        // Stations at 50, 150 ... 950, but 450's in the water.
        assert_eq!(markers.len(), 2 * 9, "{markers:?}");
        for m in &markers {
            assert!((m.at[0].abs() - 6.0).abs() < 1e-9, "{m:?}");
            assert!((m.at[1] - 10.0).abs() < 1e-9);
            assert!(m.at[2] >= END_CLEAR_M && m.at[2] <= 1_000.0 - END_CLEAR_M);
            let facing = [libm::sin(m.yaw), libm::cos(m.yaw)];
            assert!(facing[0].abs() < 1e-9 && (facing[1].abs() - 1.0).abs() < 1e-9);
        }
        let track = road(RoadClass::Track, 1_000.0);
        let markers: Vec<_> = place_roadside(ID, &track, &kit(), None, &Valley)
            .into_iter()
            .filter(|p| p.file == "marker.ashlar")
            .collect();
        assert_eq!(markers.len(), 6, "75, 225 ... 825: {markers:?}");
        for pair in markers.windows(2) {
            assert!(pair[0].at[0] * pair[1].at[0] < 0.0, "the sides take turns");
        }
        let trail = road(RoadClass::Trail, 1_000.0);
        assert!(markers_of(&place_roadside(ID, &trail, &kit(), None, &Valley)).is_empty());
        // The same pieces every time.
        assert_eq!(
            place_roadside(ID, &service, &kit(), None, &Valley),
            place_roadside(ID, &service, &kit(), None, &Valley)
        );
    }

    /// No marker stands in water, on another way, off the road's level or
    /// within a crossing's clear stretch.
    #[test]
    fn markers_keep_off_water_ways_banks_and_crossings() {
        let mut service = road(RoadClass::Service, 1_000.0);
        // Beside the way over `x > 30`: only the left side's stand.
        let mut beside = road(RoadClass::Service, 1_000.0);
        for p in &mut beside.points {
            p.x = 30.0;
        }
        let markers = place_roadside(ID, &beside, &kit(), None, &Valley);
        let markers = markers_of(&markers);
        assert!(
            !markers.is_empty() && markers.iter().all(|m| m.at[0] < 30.0),
            "{markers:?}"
        );
        service.crossings.push(Crossing {
            kind: CrossingKind::Culvert,
            at: [0.0, 250.0],
            along_m: 250.0,
            bearing: [0.0, 1.0],
            span_m: 2.0,
            deck_m: 10.0,
            surface_m: 9.0,
            banks_m: [10.0, 10.0],
            width_m: 1.5,
            depth_m: 0.3,
            river: 0,
            abutments_m: [249.0, 251.0],
        });
        // The road's level lifted off the ground from 600 m on.
        for (p, level) in service.points.iter().zip(service.profile.iter_mut()) {
            if p.along_m >= 600.0 {
                level[0] = 11.0;
            }
        }
        let pieces = place_roadside(ID, &service, &kit(), None, &Valley);
        let markers = markers_of(&pieces);
        let stations: Vec<f64> = markers.iter().map(|m| m.at[2]).collect();
        assert_eq!(
            stations,
            [50.0, 50.0, 150.0, 150.0, 350.0, 350.0, 550.0, 550.0],
            "250 is the culvert's, 450 in the water, 650 on off the level"
        );
    }

    /// A lamp stands on the verge on the right of the way in to a site's
    /// pad, its arm over the road; a bridge's ends get one either side past
    /// the deck, on the ground and not in the water.
    #[test]
    fn lamps_mark_a_pads_way_in_and_a_bridges_ends() {
        let service = road(RoadClass::Service, 1_000.0);
        let pieces = place_roadside(ID, &service, &kit(), None, &Valley);
        let lamps: Vec<_> = pieces.iter().filter(|p| p.file == "lamp.ashlar").collect();
        assert_eq!(lamps.len(), 1, "the site's end only: {lamps:?}");
        let lamp = lamps[0];
        assert!((lamp.at[2] - LAMP_IN_FROM_PAD_M).abs() < 1e-9);
        // Driving towards the pad (-z), its right is +x; the arm reaches
        // back over the road, towards x = 0.
        assert!((lamp.at[0] - 6.0).abs() < 1e-9, "{lamp:?}");
        let arm = [libm::sin(lamp.yaw), libm::cos(lamp.yaw)];
        assert!(arm[0] < -0.99, "{lamp:?}");
        // A bridge over the water at z = 450.
        let mut bridged = road(RoadClass::Service, 1_000.0);
        bridged.crossings.push(Crossing {
            kind: CrossingKind::Bridge,
            at: [0.0, 450.0],
            along_m: 450.0,
            bearing: [0.0, 1.0],
            span_m: 22.0,
            deck_m: 10.0,
            surface_m: 9.0,
            banks_m: [10.0, 10.0],
            width_m: 16.0,
            depth_m: 1.0,
            river: 0,
            abutments_m: [439.0, 461.0],
        });
        let crossings = CrossingKit {
            bridges: vec![BridgeClass {
                road: RoadClass::Service,
                file: "bridge-20.ashlar".to_owned(),
                clear_m: 20.0,
                gaps_m: [20.5, 24.5],
                length_m: 29.5,
                underside_m: 1.0,
                piers: 0,
                girder_most_m: 27.5,
            }],
            deck_end: DeckEnd {
                proud_m: 0.02,
                ramp_m: 1.2,
                drop_m: 0.12,
            },
            piers: Vec::new(),
            headwalls: Vec::new(),
            ford_post: None,
            stones: Vec::new(),
            stone_proud_m: 0.12,
        };
        let pieces = place_roadside(ID, &bridged, &kit(), Some(&crossings), &Valley);
        let lamps: Vec<_> = pieces
            .iter()
            .filter(|p| p.file == "lamp.ashlar" && (p.at[2] - 450.0).abs() < 50.0)
            .collect();
        assert_eq!(lamps.len(), 4, "{lamps:?}");
        for lamp in &lamps {
            let past = (lamp.at[2] - 450.0).abs();
            assert!(
                (past - (0.5 * 29.5 + LAMP_PAST_DECK_M)).abs() < 1e-9,
                "{lamp:?}"
            );
            assert!((lamp.at[0].abs() - 5.5).abs() < 1e-9);
            assert!((lamp.at[1] - 10.0).abs() < 1e-9);
        }
        // No marker by the bridge, which its lamps mark.
        assert!(
            markers_of(&pieces)
                .iter()
                .all(|m| (m.at[2] - 450.0).abs() > 11.0 + CROSSING_CLEAR_M - 1e-9)
        );
        // Beside the way over `x > 30`, the lamps on its carriageway are
        // left out and the other side's stand.
        let mut beside = bridged.clone();
        for p in &mut beside.points {
            p.x = 27.0;
        }
        for c in &mut beside.crossings {
            c.at[0] = 27.0;
        }
        let pieces = place_roadside(ID, &beside, &kit(), Some(&crossings), &Valley);
        let lamps: Vec<_> = pieces
            .iter()
            .filter(|p| p.file == "lamp.ashlar" && (p.at[2] - 450.0).abs() < 50.0)
            .collect();
        assert_eq!(lamps.len(), 2, "{lamps:?}");
        assert!(lamps.iter().all(|l| (l.at[0] - 21.5).abs() < 1e-9));
    }
}
