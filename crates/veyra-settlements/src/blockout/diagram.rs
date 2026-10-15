//! A plan inspection without a renderer, from the same placement pass as the bake.
use std::fmt::Write;

use super::{Ground, Kit, Rect, layout_city, spawn, terraces_between};

/// Tab-separated outlines: kind, level, yaw in radians, then `x,z` vertices.
/// The content example plots these beside samples of the actual shaped ground.
///
/// # Errors
/// If a landmark recipe fails to validate.
pub fn diagram(ground: &Ground<'_>) -> anyhow::Result<String> {
    let city = layout_city(ground)?;
    let mut output = String::new();
    for terrace in terraces_between(ground, -320.0, 320.0) {
        let mut points: Vec<_> = terrace.edge.iter().map(|p| [p[0], p[1]]).collect();
        points.extend(terrace.edge.iter().rev().map(|p| [p[0], p[1] + p[2]]));
        record(&mut output, "terrace", terrace.level, 0.0, &points);
    }
    for (district, masses) in &city.districts {
        for mass in masses {
            record(
                &mut output,
                district,
                mass.pose.translation.y,
                0.0,
                &corners(Rect::from_box(mass.footprint)),
            );
        }
    }
    for lane in &city.open {
        record(
            &mut output,
            "reserved",
            0.0,
            0.0,
            &corners(Rect::from_box(*lane)),
        );
    }
    for (court, level) in &city.courts {
        record(
            &mut output,
            "court",
            *level,
            court.yaw,
            &corners(court.rect),
        );
    }
    for (name, kits) in &city.kits {
        for placed in kits {
            let size = match placed.kit {
                Kit::House(h) => [h.lot_width(), h.depth],
                Kit::SplitHouse { house, depth, .. } => [house.width, depth],
                Kit::Module(m) => [m.width, m.depth],
                Kit::Stall(s) => [s.width, s.depth],
                Kit::Dressing(f) => match f {
                    veyra_buildings::port::Fitting::Kerb => [6.0, 0.6],
                    veyra_buildings::port::Fitting::Paving => [6.0, 2.0],
                    veyra_buildings::port::Fitting::Bed => [4.0, 2.2],
                    veyra_buildings::port::Fitting::Shelter => [4.3, 2.8],
                    veyra_buildings::port::Fitting::Lanterns => [8.0, 0.3],
                    _ => [1.0, 0.5],
                },
                Kit::Furniture(f) => match f {
                    super::types::Furniture::Lamp => [0.35, 1.0],
                    super::types::Furniture::Bench => [1.8, 0.5],
                    super::types::Furniture::Planter | super::types::Furniture::Crates => {
                        [1.3, 0.7]
                    }
                    _ => [1.0, 1.0],
                },
                _ => continue,
            };
            let (sin, cos) = placed.yaw.sin_cos();
            let rect = Rect {
                centre: [placed.at[0], placed.at[2]],
                axes: [[cos, -sin], [sin, cos]],
                half: [size[0] * 0.5, size[1] * 0.5],
            };
            let kind = if matches!(placed.kit, Kit::SplitHouse { .. }) {
                "split-houses"
            } else {
                name
            };
            record(&mut output, kind, placed.at[1], placed.yaw, &corners(rect));
        }
    }
    for landmark in &city.landmarks {
        record(
            &mut output,
            &landmark.name,
            landmark.position[1],
            landmark.yaw,
            &corners(Rect::from_box(landmark.footprint)),
        );
    }
    for plant in &city.plants {
        record(
            &mut output,
            "plant",
            plant.position[1],
            0.0,
            &[[plant.position[0], plant.position[2]]],
        );
    }
    let [x, y, z] = spawn(&city, ground);
    record(&mut output, "spawn", y, 0.0, &[[x, z]]);
    record(&mut output, "road-exit", 0.0, 0.0, &[ground.road_exit]);
    Ok(output)
}

fn corners(rect: Rect) -> Vec<[f64; 2]> {
    [(-1.0, -1.0), (1.0, -1.0), (1.0, 1.0), (-1.0, 1.0)]
        .map(|(u, v)| {
            [0, 1].map(|i| {
                rect.centre[i]
                    + rect.axes[0][i] * rect.half[0] * u
                    + rect.axes[1][i] * rect.half[1] * v
            })
        })
        .to_vec()
}

fn record(output: &mut String, kind: &str, height: f64, yaw: f64, points: &[[f64; 2]]) {
    write!(output, "{kind}\t{height:.3}\t{yaw:.5}").expect("write to a string");
    for [x, z] in points {
        write!(output, "\t{x:.3},{z:.3}").expect("write to a string");
    }
    output.push('\n');
}
