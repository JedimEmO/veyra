//! `--route`: the slice's route as a scripted walk. The character starts at
//! the far end of Port Serein's road, follows it into the city, walks the
//! street into the landmark's lobby, and goes back out the way it came.
//!
//! It is a bench fixture as much as a check: the walk streams forest,
//! rocks, the road and the city in the order a player meets them, and ends
//! where the plan's first slice says a player can go.
use bevy::prelude::*;
use strata::client::flight::{MotionHold, motion_held};
use strata::prelude::*;
use strata::sim::Settlements;

/// How near a waypoint counts as reached, metres.
const REACHED_M: f64 = 3.0;

/// The route's waypoints, world `(x, z)`, and how far along it the walker is.
#[derive(Resource, Debug, Default)]
struct Route {
    points: Vec<(f64, f64)>,
    next: usize,
}

pub(super) fn install(app: &mut App) {
    app.init_resource::<Route>().add_systems(Update, steer);
}

/// The waypoints: the road from its far end to the city edge, the street
/// to the lobby, then the same back out.
fn waypoints(settlements: &Settlements) -> Vec<(f64, f64)> {
    let Some(settlement) = settlements.0.first() else {
        return Vec::new();
    };
    let mut inbound: Vec<(f64, f64)> = settlement
        .site
        .road
        .points
        .iter()
        .rev()
        .map(|p| (p[0], p[1]))
        .collect();
    if let Some(layout) = &settlement.layout {
        for local in [layout.spawn, layout.lobby] {
            let at = settlement.to_world(local);
            inbound.push((at.x(), at.z()));
        }
    }
    let mut outbound: Vec<_> = inbound.iter().rev().skip(1).copied().collect();
    inbound.append(&mut outbound);
    inbound
}

/// Turns the scripted walker towards its next waypoint, and stops it at the
/// last. Sprinting, so a fixture covers the road in a bench run's minutes.
/// Under a benchmark's held [`MotionHold`] it stands at the start, facing
/// the first waypoint, until the streamers are idle.
fn steer(
    settlements: Option<Res<Settlements>>,
    hold: Option<Res<MotionHold>>,
    mut route: ResMut<Route>,
    mut walkers: Query<(&WorldPosition, &mut ScriptedIntent), With<LocalPlayer>>,
    mut ticks: Local<u32>,
) {
    *ticks += 1;
    let Some(settlements) = settlements else {
        return;
    };
    if route.points.is_empty() {
        route.points = waypoints(&settlements);
        route.next = 0;
    }
    for (at, mut intent) in &mut walkers {
        while let Some(&(x, z)) = route.points.get(route.next) {
            let (dx, dz) = (x - at.0.x(), z - at.0.z());
            if dx * dx + dz * dz > REACHED_M * REACHED_M {
                // Zero faces -z and positive turns towards -x.
                #[allow(clippy::cast_possible_truncation, reason = "a heading in radians")]
                {
                    intent.0.yaw = (-dx).atan2(-dz) as f32;
                }
                intent.0.move_dir = Vec2::Y;
                intent.0.sprint = true;
                if (*ticks).is_multiple_of(240) {
                    debug!(
                        waypoint = route.next,
                        x,
                        z,
                        distance = (dx * dx + dz * dz).sqrt(),
                        yaw = intent.0.yaw,
                        "route heading"
                    );
                }
                break;
            }
            route.next += 1;
            info!(
                waypoint = route.next,
                of = route.points.len(),
                "route waypoint reached"
            );
        }
        if route.next >= route.points.len() || motion_held(hold.as_deref()) {
            intent.0.move_dir = Vec2::ZERO;
        }
    }
}
