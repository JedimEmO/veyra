//! The crossings' kit as the server collides with it: the convex proxies
//! derived from the evaluated solids (ticket 0005, step 7). A walker stands
//! on the deck's proxies, is stopped by the rails', and the deck bears on
//! the abutments' and the piers'.

use ashlar::InstanceCollider;
use ashlar_manifold::{ManifoldMesher, mesh_building};
use veyra_buildings::bridge::{
    self, Bridge, Headwall, PROUD_M, Piece, Pier, RAIL_M, RAMP_DROP_M, RAMP_M, TRUSS_M, Way,
};

fn colliders(piece: Piece) -> Vec<InstanceCollider> {
    let building = piece.building().expect("a valid piece");
    mesh_building(&building, &ManifoldMesher::default())
        .unwrap_or_else(|e| panic!("{piece:?}: {e}"))
        .colliders()
}

/// The `[min, max]` of the proxies named `prefix*` along one axis.
fn extent(colliders: &[InstanceCollider], prefix: &str, axis: usize) -> [f64; 2] {
    let mut out = [f64::INFINITY, f64::NEG_INFINITY];
    for c in colliders.iter().filter(|c| c.element.starts_with(prefix)) {
        for v in &c.solid.vertices {
            out = [out[0].min(v[axis]), out[1].max(v[axis])];
        }
    }
    assert!(out[0] <= out[1], "no proxy named {prefix}");
    out
}

/// The highest point of the deck's proxies over `(x, z)`: each slice is a
/// prism along `z` with a straight top edge between its two ends, full
/// width.
fn deck_top(colliders: &[InstanceCollider], x: f64, z: f64) -> Option<f64> {
    let mut best: Option<f64> = None;
    for c in colliders.iter().filter(|c| c.element.starts_with("deck-")) {
        let v = &c.solid.vertices;
        let (z0, z1) = v
            .iter()
            .fold((f64::INFINITY, f64::NEG_INFINITY), |(a, b), p| {
                (a.min(p.z), b.max(p.z))
            });
        let (x0, x1) = v
            .iter()
            .fold((f64::INFINITY, f64::NEG_INFINITY), |(a, b), p| {
                (a.min(p.x), b.max(p.x))
            });
        if z < z0 - 1e-9 || z > z1 + 1e-9 || x < x0 || x > x1 {
            continue;
        }
        let top_at = |end: f64| {
            v.iter()
                .filter(|p| (p.z - end).abs() < 1e-6)
                .map(|p| p.y)
                .fold(f64::NEG_INFINITY, f64::max)
        };
        let t = if z1 > z0 { (z - z0) / (z1 - z0) } else { 0.0 };
        let top = top_at(z0) + (top_at(z1) - top_at(z0)) * t;
        best = Some(best.map_or(top, |b: f64| b.max(top)));
    }
    best
}

fn bridges() -> impl Iterator<Item = Bridge> {
    bridge::catalogue().into_iter().filter_map(|p| match p {
        Piece::Bridge(b) => Some(b),
        _ => None,
    })
}

/// The deck's top is level end to end at [`PROUD_M`] over the road, every
/// slice meeting the next with no step, and over its last [`RAMP_M`] it
/// falls gently into the road's shelf, so a foot or a wheel meets no lip
/// at either end.
#[test]
fn the_deck_is_level_end_to_end_and_meets_the_road() {
    for b in bridges() {
        let c = colliders(Piece::Bridge(b));
        let end = 0.5 * b.length_m();
        let half = b.way.half_m() - 0.2;
        for x in [-half, 0.0, half] {
            let mut last: Option<f64> = None;
            let mut z = -end;
            while z <= end {
                let top =
                    deck_top(&c, x, z).unwrap_or_else(|| panic!("{b:?}: no deck at {x}, {z}"));
                if z.abs() <= end - RAMP_M {
                    assert!((top - PROUD_M).abs() < 1e-6, "{b:?}: {top} at {x}, {z}");
                } else {
                    assert!((PROUD_M - RAMP_DROP_M - 1e-6..=PROUD_M + 1e-6).contains(&top));
                }
                if let Some(last) = last {
                    // No step, and no slope a wheel would notice.
                    assert!((top - last).abs() <= 0.05 * 0.11, "{b:?}: a step at {z}");
                }
                last = Some(top);
                z += 0.05;
            }
        }
    }
}

/// Both sides carry a rail (a footbridge, its truss) from end to end of the
/// deck, as one proxy from inside the deck to over a walker's hip, so no
/// gap under it, between its posts or at its ends lets a walker off.
#[test]
fn the_rails_are_continuous_and_stop_a_walker() {
    for b in bridges() {
        let c = colliders(Piece::Bridge(b));
        let end = 0.5 * b.length_m();
        let (names, top) = if b.way == Way::Foot {
            (["truss-left", "truss-right"], TRUSS_M)
        } else {
            (["rail-left", "rail-right"], RAIL_M)
        };
        for name in names {
            let proxies: Vec<_> = c.iter().filter(|p| p.element == name).collect();
            assert_eq!(proxies.len(), 1, "{b:?}: {name} is one proxy");
            let [z0, z1] = extent(&c, name, 2);
            assert!(
                z0 <= -end + 0.4 && z1 >= end - 0.4,
                "{b:?}: {name} spans {z0}..{z1}"
            );
            let [y0, y1] = extent(&c, name, 1);
            assert!(y0 <= 0.0, "{b:?}: {name} starts {y0} over the deck");
            assert!(y1 >= top - 0.01, "{b:?}: {name} is {y1} tall");
            // Outside the way, inside the deck's edge.
            let [x0, x1] = extent(&c, name, 0);
            let (inner, outer) = if name.ends_with("left") {
                (-x1, -x0)
            } else {
                (x0, x1)
            };
            assert!(inner >= b.way.half_m() - 1e-6, "{b:?}: {name} in the way");
            assert!(
                outer <= b.way.deck_half_m() + 0.2,
                "{b:?}: {name} off the deck"
            );
        }
    }
}

/// The deck bears on both abutments' seats at the clear span's edges, and
/// each abutment reaches down into the bank; a girder deck's girders bear
/// on the seats and their bottom is where a pier's cap is set.
#[test]
fn the_deck_bears_on_its_abutments() {
    for b in bridges() {
        let c = colliders(Piece::Bridge(b));
        let span = 0.5 * b.clear_m;
        for name in ["seat-near", "seat-far"] {
            let [_, top] = extent(&c, name, 1);
            assert!(
                (top + b.underside_m()).abs() < 1e-6,
                "{b:?}: {name} at {top}"
            );
            let [z0, z1] = extent(&c, name, 2);
            let face = if name.ends_with("near") { -span } else { span };
            assert!(z0 <= face + 0.13 && z1 >= face - 0.13, "{b:?}: {name}");
        }
        for name in ["abutment-near", "abutment-far"] {
            let [y0, _] = extent(&c, name, 1);
            assert!(y0 <= -2.9, "{b:?}: {name} reaches only {y0}");
        }
        if b.girders() {
            let [bottom, _] = extent(&c, "girders", 1);
            assert!(
                (bottom + b.underside_m()).abs() < 1e-6,
                "{b:?}: girders at {bottom}"
            );
            let [z0, z1] = extent(&c, "girders", 2);
            assert!(z0 < -span && z1 > span, "{b:?}: girders short of the seats");
        }
    }
}

/// A pier's cap is at its origin, where the girders' bottom is set, and
/// its shaft reaches its height down.
#[test]
fn a_pier_reaches_its_height() {
    for way in [Way::Service, Way::Track] {
        for height_m in bridge::PIER_HEIGHTS_M {
            let pier = Pier { way, height_m };
            let c = colliders(Piece::Pier(pier));
            let [_, top] = extent(&c, "cap", 1);
            assert!(top.abs() < 1e-6);
            let [bottom, _] = extent(&c, "shaft", 1);
            assert!((bottom + height_m).abs() < 1e-6, "{pier:?}: {bottom}");
            let [z0, z1] = extent(&c, "", 2);
            assert!(z1.max(-z0) <= pier.half_along_m() + 1e-6);
        }
    }
}

/// The small pieces mesh and carry the proxies a walker meets.
#[test]
fn the_small_pieces_collide() {
    let headwalls = bridge::BOX_HEIGHTS_M.map(|box_m| Piece::Headwall(Headwall { box_m }));
    let stones = (0..bridge::STONES).map(Piece::Stone);
    for piece in headwalls.into_iter().chain([Piece::FordPost]).chain(stones) {
        assert!(!colliders(piece).is_empty(), "{piece:?}");
    }
    // A headwall's coping is where the layout says, so the world can keep
    // it under the road.
    for box_m in bridge::BOX_HEIGHTS_M {
        let headwall = Headwall { box_m };
        let c = colliders(Piece::Headwall(headwall));
        let [_, top] = extent(&c, "headwall", 1);
        assert!(
            (top + bridge::COPING_M - headwall.top_m()).abs() < 1e-9,
            "{headwall:?}: the wall's top at {top}"
        );
    }
    // A stone's top is its origin, flat enough to stand on.
    let c = colliders(Piece::Stone(1));
    let [y0, y1] = extent(&c, "stone", 1);
    assert!(y1.abs() < 1e-9 && (y0 + bridge::STONE_DEPTH_M).abs() < 1e-9);
}
