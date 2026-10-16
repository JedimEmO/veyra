//! Where the questline's two out-of-town places stand (ticket 0014): the
//! monitoring station and Wim's channel, each a pure function of the world
//! and `assets/quests/places.ron`. Not a site kind and in no world version:
//! the game asks once when the questline begins and stands what it finds.

use std::f64::consts::TAU;

use strata_registry::quest::PlaceRules;
use strata_worldgen::{ProceduralWorld, WorldSource};

/// The two places, `(x, z)` in world metres. Each is `None` where the world
/// has no acceptable ground, which the game words by leaving the quest at
/// its first stage with no marker.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct QuestSites {
    /// The monitoring station.
    pub station: Option<[f64; 2]>,
    /// Wim's channel.
    pub channel: Option<[f64; 2]>,
}

fn distance(a: [f64; 2], b: [f64; 2]) -> f64 {
    (a[0] - b[0]).hypot(a[1] - b[1])
}

/// Dry, gentle, undeveloped ground at `(x, z)`.
fn firm(world: &dyn WorldSource, rules: &PlaceRules, x: f64, z: f64) -> bool {
    let sample = world.sample(x, z);
    sample.water.is_none()
        && sample.height >= rules.min_height_m
        && sample.landform.slope <= rules.max_slope
        && sample.landform.developed <= 0.0
}

/// Every cell of a coarse scan round `city` that holds water, with the
/// river's downstream direction, so a station is judged against where water
/// is without sampling the same ground once a candidate.
fn waters(
    world: &ProceduralWorld,
    rules: &PlaceRules,
    city: [f64; 2],
) -> Vec<([f64; 2], [f32; 2])> {
    let reach = rules.station_m + rules.station_slack_m + rules.channel_far_m;
    annulus(city, 0.0, reach, rules.scan_step_m)
        .filter_map(|at| {
            let water = world.sample(at[0], at[1]).water?;
            Some((at, water.flow))
        })
        .collect()
}

/// The station and its channel, found together: the nearest ring to
/// `station_m` (then outward by `station_step_m`, up to the slack) with firm
/// ground off the road that has a dry bank `channel_near_m` to
/// `channel_far_m` away, and on it the bearing nearest the road, so it
/// stands at the end of a way in and not in the middle of the wild.
#[must_use]
pub fn places(world: &ProceduralWorld, rules: &PlaceRules, city: [f64; 2]) -> QuestSites {
    let steps = (rules.station_slack_m / rules.station_step_m)
        .floor()
        .max(0.0);
    #[allow(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "a small non-negative count of rings"
    )]
    let rings = steps as u32;
    let wet = waters(world, rules, city);
    for ring in 0..=rings {
        let radius = rules.station_m + f64::from(ring) * rules.station_step_m;
        let mut candidates: Vec<(f64, [f64; 2])> = Vec::new();
        for bearing in 0..rules.bearings {
            let angle = TAU * f64::from(bearing) / f64::from(rules.bearings);
            let at = [
                city[0] + radius * angle.cos(),
                city[1] + radius * angle.sin(),
            ];
            let near_water = wet.iter().any(|(cell, _)| {
                (rules.channel_near_m..=rules.channel_far_m).contains(&distance(*cell, at))
            });
            if !near_water || !firm(world, rules, at[0], at[1]) {
                continue;
            }
            let road = world
                .nearest_road(at[0], at[1], rules.road_reach_m)
                .map_or(f64::MAX, |road| road.distance_m);
            if road >= rules.road_gap_m {
                candidates.push((road, at));
            }
        }
        candidates.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal));
        for (_, at) in candidates {
            if let Some(bank) = channel(world, rules, at) {
                return QuestSites {
                    station: Some(at),
                    channel: Some(bank),
                };
            }
        }
    }
    QuestSites::default()
}

/// Whether the sample is a dry bank of a river or lake, and how it ranks: a
/// river's first.
fn bank(sample: &strata_worldgen::Sample, rules: &PlaceRules) -> Option<bool> {
    let water = sample.water?;
    if sample.in_water() || sample.landform.slope > rules.max_slope {
        return None;
    }
    let river = water.order > 0;
    let edge = if river {
        water.signed_distance_m.abs() - water.width_m / 2.0
    } else {
        water.signed_distance_m
    };
    (0.0..=rules.bank_margin_m).contains(&edge).then_some(river)
}

/// The grid cells of the annulus `near..far` round `centre`, `step` apart.
fn annulus(centre: [f64; 2], near: f64, far: f64, step: f64) -> impl Iterator<Item = [f64; 2]> {
    #[allow(
        clippy::cast_possible_truncation,
        reason = "a grid count of a few hundred"
    )]
    let cells = (far / step).ceil() as i64;
    (-cells..=cells)
        .flat_map(move |column| (-cells..=cells).map(move |row| (column, row)))
        .filter_map(move |(column, row)| {
            #[allow(clippy::cast_precision_loss, reason = "a grid index of a few hundred")]
            let at = [
                centre[0] + column as f64 * step,
                centre[1] + row as f64 * step,
            ];
            let apart = distance(at, centre);
            (apart >= near && apart <= far).then_some(at)
        })
}

/// A dry bank within the annulus `near..far` of `station`: the coarse scan
/// finds where water is, and the nearest few finds are searched finely for
/// the bank itself. A river's bank wins over a lake's, then the nearest.
fn bank_in(
    world: &ProceduralWorld,
    rules: &PlaceRules,
    station: [f64; 2],
    near: f64,
    far: f64,
) -> Option<[f64; 2]> {
    /// Finds searched finely: water the coarse scan met is usually one
    /// reach, so a few are enough.
    const FINDS: usize = 12;
    let mut found: Vec<((bool, bool), f64, [f64; 2])> =
        annulus(station, near, far, rules.scan_step_m)
            .filter_map(|at| {
                let sample = world.sample(at[0], at[1]);
                let water = sample.water?;
                // Upstream: the station lies downstream of the bank, so the
                // flow carries water from the channel toward it.
                let flow = f64::from(water.flow[0]) * (station[0] - at[0])
                    + f64::from(water.flow[1]) * (station[1] - at[1]);
                Some(((water.order == 0, flow <= 0.0), distance(at, station), at))
            })
            .collect();
    found.sort_by(|a, b| {
        (a.0, a.1)
            .partial_cmp(&(b.0, b.1))
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    for (_, _, at) in found.into_iter().take(FINDS) {
        let mut best: Option<((bool, f64), [f64; 2])> = None;
        for cell in annulus(at, 0.0, rules.scan_step_m * 2.0, rules.refine_step_m) {
            let apart = distance(cell, station);
            if apart < near || apart > far {
                continue;
            }
            let Some(river) = bank(&world.sample(cell[0], cell[1]), rules) else {
                continue;
            };
            let key = (!river, apart);
            if best.is_none_or(|(held, _)| key < held) {
                best = Some((key, cell));
            }
        }
        if let Some((_, cell)) = best {
            return Some(cell);
        }
    }
    None
}

/// The channel: the nearest dry bank of a river (upstream of `station`
/// when it can be) or else a lake, between `channel_near_m` and
/// `channel_far_m` of the station.
#[must_use]
pub fn channel(world: &ProceduralWorld, rules: &PlaceRules, station: [f64; 2]) -> Option<[f64; 2]> {
    bank_in(
        world,
        rules,
        station,
        rules.channel_near_m,
        rules.channel_far_m,
    )
}

/// The metres between two points, for a still's caption and the test.
#[must_use]
pub fn metres(a: [f64; 2], b: [f64; 2]) -> f64 {
    distance(a, b)
}

/// Teague's field camp: the terminal, the workbench and the heading a walker
/// from the city has, which the props' fronts face the opposite of.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Camp {
    /// The terminal's `(x, z)`.
    pub terminal: [f64; 2],
    /// The workbench's `(x, z)`.
    pub bench: [f64; 2],
    /// The heading from the station toward the city.
    pub yaw: f32,
}

/// The camp back from `station` toward `city`: `back_m` along the line, then
/// nearer in steps of `spacing_m` while the ground is not firm, with the
/// bench to the right of the terminal or, failing that, to its left. A pure
/// function of the world, the dials and the two points; `None` where the two
/// coincide or no step along the line has firm ground for both props.
#[must_use]
pub fn camp(
    world: &dyn WorldSource,
    rules: &PlaceRules,
    station: [f64; 2],
    city: [f64; 2],
) -> Option<Camp> {
    let length = distance(station, city);
    if length < 1.0 {
        return None;
    }
    let (dx, dz) = (
        (city[0] - station[0]) / length,
        (city[1] - station[1]) / length,
    );
    #[allow(clippy::cast_possible_truncation, reason = "a heading in radians, f32")]
    let yaw = dx.atan2(dz) as f32;
    let right = crate::PlayerIntent::right(yaw);
    let right = [f64::from(right.x), f64::from(right.z)];
    let mut back = rules.camp_back_m;
    while back >= 2.0 * rules.camp_spacing_m {
        let terminal = [station[0] + dx * back, station[1] + dz * back];
        if firm(world, rules, terminal[0], terminal[1]) {
            for side in [1.0, -1.0] {
                let bench = [
                    terminal[0] + side * right[0] * rules.camp_spacing_m,
                    terminal[1] + side * right[1] * rules.camp_spacing_m,
                ];
                if firm(world, rules, bench[0], bench[1]) {
                    return Some(Camp {
                        terminal,
                        bench,
                        yaw,
                    });
                }
            }
        }
        back -= rules.camp_spacing_m;
    }
    None
}
