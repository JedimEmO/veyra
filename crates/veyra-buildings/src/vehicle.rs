//! The courier bike (ticket 0009, step 2): a hover bike of the colony's
//! plain issue, drawn once and stood under the rider by the client.
//!
//! The frame of the piece: the origin on the ground plane under the middle
//! of the hull, `+y` up, `+z` the way it points, `+x` to its left. The
//! saddle's top is the seat the vehicle's file names (`view.seat`), so the
//! rider's hips sit on it whatever the shape becomes. The bike has no
//! wheels and no collider: it hovers, and the capsule of the rider is what
//! the physics moves.

use ashlar::{
    Building, Collision, Element, Geometry, Instance, Part, Pose, UvMode, ValidationError,
};

/// The hull's length, nose to tail housing, metres.
pub const LENGTH_M: f64 = 2.25;

/// The top of the saddle over the origin, metres: where the rider sits.
pub const SADDLE_TOP_M: f64 = 0.56;

/// The material key each slot binds; the content step defines them in the
/// plain issued greys the weapons wear.
pub const MATERIALS: [(&str, &str); 5] = [
    ("hull", "vehicle:hull"),
    ("fairing", "vehicle:fairing"),
    ("trim", "vehicle:trim"),
    ("vane", "vehicle:vane"),
    ("glow", "vehicle:glow"),
];

fn element(name: &str, geometry: Geometry, slot: &str) -> Element {
    Element::new(name, geometry, slot).uv(UvMode::Box)
}

/// A mirrored pair of points about the bike's middle.
fn pair(x: f64, y: f64, z: f64) -> [[f64; 3]; 2] {
    [[x, y, z], [-x, y, z]]
}

/// The bike's solids: a tapered hull, a fairing over its nose, a saddle, two
/// skid vanes under its sides and a drive housing at the tail with its
/// exhaust glow.
fn solids() -> Vec<Element> {
    let hull: Vec<[f64; 3]> = [
        pair(0.34, 0.16, -1.0),
        pair(0.34, 0.44, -1.0),
        pair(0.40, 0.16, 0.0),
        pair(0.40, 0.44, 0.0),
        pair(0.11, 0.22, 1.12),
        pair(0.11, 0.36, 1.12),
    ]
    .into_iter()
    .flatten()
    .collect();
    let fairing: Vec<[f64; 3]> = [
        pair(0.30, 0.43, 0.35),
        pair(0.16, 0.43, 0.95),
        pair(0.22, 0.84, 0.42),
        pair(0.10, 0.62, 0.80),
    ]
    .into_iter()
    .flatten()
    .collect();
    let vane = |name: &str, x: f64| {
        element(
            name,
            Geometry::chamfered_cuboid([0.07, 0.17, 1.7], 0.02).placed(Pose::at([x, 0.0, -0.85])),
            "vane",
        )
    };
    vec![
        element("hull", Geometry::hull(hull), "hull"),
        element("fairing", Geometry::hull(fairing), "fairing"),
        element(
            "saddle",
            Geometry::chamfered_cuboid([0.36, SADDLE_TOP_M - 0.43, 0.72], 0.04)
                .placed(Pose::at([-0.18, 0.43, -0.5])),
            "trim",
        ),
        vane("vane_left", 0.28),
        vane("vane_right", -0.35),
        element(
            "drive",
            Geometry::chamfered_cuboid([0.56, 0.34, 0.5], 0.05)
                .placed(Pose::at([-0.28, 0.14, -1.13])),
            "vane",
        ),
        element(
            "exhaust",
            Geometry::cuboid([0.38, 0.09, 0.03]).placed(Pose::at([-0.19, 0.27, -1.16])),
            "glow",
        ),
    ]
}

/// The courier bike's recipe, its slots bound to [`MATERIALS`].
///
/// # Errors
/// A solid that does not validate: a bug here.
pub fn courier_bike() -> Result<Building, ValidationError> {
    let id = "veyra:vehicle.courier_bike";
    let mut part = Part::builder(id);
    for element in solids() {
        // A prop has no collider; the capsule is the body.
        part = part.element(element.collision(Collision::None));
    }
    let mut builder = Building::builder(id)
        .part(part.build()?)
        .instance(Instance::new("bike", id));
    for (slot, key) in MATERIALS {
        builder = builder.material(slot, key);
    }
    builder.build()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The recipe builds, is about as long as the file says it looks, and the
    /// saddle's top is where the seat is.
    #[test]
    fn the_bike_builds_and_the_saddle_is_at_seat_height() {
        courier_bike().expect("the recipe validates");
        let all = solids();
        let (mut lo, mut hi) = (f64::MAX, f64::MIN);
        for e in &all {
            let [a, b] = e.geometry.bounds().expect("bounds");
            lo = lo.min(a.z);
            hi = hi.max(b.z);
        }
        assert!((hi - lo - LENGTH_M).abs() < 0.1, "length {}", hi - lo);
        let saddle = all.iter().find(|e| e.id == "saddle").expect("a saddle");
        let [_, top] = saddle.geometry.bounds().expect("bounds");
        assert!((top.y - SADDLE_TOP_M).abs() < 1e-9);
    }
}
