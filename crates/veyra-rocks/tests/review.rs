//! Review pictures of every species, written to `scratch/rocks/`:
//!
//! ```text
//! cargo test --release -p veyra-rocks --test review -- --ignored --nocapture
//! ```
//!
//! Per species a lit contact sheet of the eight variants at the nearest
//! level, a 64 px silhouette sheet from two sides, and variant 0's three
//! levels side by side with their silhouettes; then the hulls of a boulder
//! and a cliff drawn over the rock. Ignored because it takes a while and
//! writes files; nothing here is asserted, it is for eyes.
use std::path::PathBuf;
use std::time::Instant;

use ashlar::glam::DVec3;
use ashlar::prop::PropCollider;
use veyra_rocks::review::{Camera, Image, Style, overlay, render};
use veyra_rocks::{GrownRock, Hull, VARIANTS, grow, recipes};

fn out_dir() -> PathBuf {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../scratch/rocks");
    std::fs::create_dir_all(&dir).expect("scratch/rocks can be created");
    dir
}

fn save(image: &Image, name: &str) {
    let path = out_dir().join(name);
    image::RgbaImage::from_raw(image.width, image.height, image.rgba.clone())
        .expect("the buffer is width by height")
        .save(&path)
        .expect("the PNG writes");
    println!("wrote {}", path.display());
}

/// One box around every variant, so a sheet shows them at one scale.
fn shared_bounds(rocks: &[GrownRock]) -> [DVec3; 2] {
    let mut min = DVec3::splat(f64::INFINITY);
    let mut max = DVec3::splat(f64::NEG_INFINITY);
    for rock in rocks {
        min = min.min(rock.prop.bounds[0]);
        max = max.max(rock.prop.bounds[1]);
    }
    let reach = min
        .x
        .abs()
        .max(max.x.abs())
        .max(min.z.abs())
        .max(max.z.abs());
    [
        DVec3::new(-reach, min.y, -reach),
        DVec3::new(reach, max.y, reach),
    ]
}

const YAW: f64 = 0.45;
const PITCH: f64 = 0.3;

#[test]
#[ignore = "writes review pictures to scratch/rocks; run by hand"]
fn review_sheets() {
    const SUPER: u32 = 2;
    for recipe in recipes() {
        let name = recipe.name();
        let start = Instant::now();
        let rocks: Vec<GrownRock> = (0..VARIANTS).map(|v| grow(&recipe, v)).collect();
        println!(
            "{name}: shaped {VARIANTS} variants in {:.2} s",
            start.elapsed().as_secs_f64()
        );
        for (v, rock) in rocks.iter().enumerate() {
            println!(
                "  {name}-{v}: height {:.2} m, radius {:.2} m, triangles {:?}, hulls {}",
                rock.height,
                rock.radius,
                rock.prop.triangles(),
                rock.prop.colliders.len()
            );
        }
        let bounds = shared_bounds(&rocks);
        let mut lit = Style::lit();
        lit.ground = true;

        let cells: Vec<Image> = rocks
            .iter()
            .map(|r| {
                let camera = Camera::framing(bounds, YAW, PITCH, 360 * SUPER, 1.3);
                render(&r.prop.levels[0].pieces[0].mesh, &camera, &lit).downsample(SUPER)
            })
            .collect();
        save(
            &Image::sheet(&cells, 4, 6, [255; 4]),
            &format!("{name}-variants.png"),
        );

        // Silhouettes at 64 px, from the front and from the side, level.
        let silhouettes: Vec<Image> = [0.0, core::f64::consts::FRAC_PI_2]
            .iter()
            .flat_map(|&yaw| {
                rocks.iter().map(move |r| {
                    let camera = Camera::framing(bounds, yaw, 0.0, 64 * 4, 1.0);
                    render(
                        &r.prop.levels[0].pieces[0].mesh,
                        &camera,
                        &Style::silhouette(),
                    )
                    .downsample(4)
                })
            })
            .collect();
        save(
            &Image::sheet(&silhouettes, 8, 4, [200, 200, 200, 255]),
            &format!("{name}-silhouettes.png"),
        );

        let rock = &rocks[0];
        let own = shared_bounds(std::slice::from_ref(rock));
        let mut levels: Vec<Image> = rock
            .prop
            .levels
            .iter()
            .map(|l| {
                let camera = Camera::framing(own, YAW, PITCH, 360 * SUPER, 1.3);
                render(&l.pieces[0].mesh, &camera, &lit).downsample(SUPER)
            })
            .collect();
        levels.extend(rock.prop.levels.iter().map(|l| {
            let camera = Camera::framing(own, YAW, 0.0, 360 * 2, 1.3);
            render(&l.pieces[0].mesh, &camera, &Style::silhouette()).downsample(2)
        }));
        save(
            &Image::sheet(&levels, 3, 6, [255; 4]),
            &format!("{name}-levels.png"),
        );
    }
}

/// The hulls of a boulder and two cliffs, drawn over the rock from two
/// sides, one colour per hull.
#[test]
#[ignore = "writes review pictures to scratch/rocks; run by hand"]
fn hull_overlays() {
    const SUPER: u32 = 2;
    for (species, variant) in [
        ("strata:granite_boulder", 1),
        ("strata:sandstone_cliff", 0),
        ("strata:granite_cliff", 1),
    ] {
        let recipe = recipes()
            .into_iter()
            .find(|r| r.species == species)
            .expect("the species exists");
        let rock = grow(&recipe, variant);
        let own = shared_bounds(std::slice::from_ref(&rock));
        let hulls: Vec<Hull> = rock
            .prop
            .colliders
            .iter()
            .filter_map(|c| match c {
                PropCollider::ConvexHull(solid) => Hull::new(&solid.vertices),
                _ => None,
            })
            .collect();
        let colours = [
            [230, 40, 40, 255],
            [40, 120, 230, 255],
            [240, 200, 20, 255],
            [40, 200, 90, 255],
        ];
        let views: Vec<Image> = [YAW, YAW + core::f64::consts::FRAC_PI_2]
            .iter()
            .map(|&yaw| {
                let camera = Camera::framing(own, yaw, 0.2, 520 * SUPER, 1.2);
                let mut image = render(&rock.prop.levels[0].pieces[0].mesh, &camera, &Style::lit());
                // Only the faces towards the camera, so the far side of a
                // hull does not hide the near side's fit.
                let (sin, cos) = yaw.sin_cos();
                let forward = DVec3::new(sin * 0.2_f64.cos(), -0.2_f64.sin(), -cos * 0.2_f64.cos());
                for (hull, colour) in hulls.iter().zip(colours.iter().cycle()) {
                    let edges: Vec<[DVec3; 2]> = hull
                        .faces
                        .iter()
                        .filter(|f| {
                            let [a, b, c] = f.map(|i| hull.points[i]);
                            (b - a).cross(c - a).dot(forward) < 0.0
                        })
                        .flat_map(|f| {
                            let [a, b, c] = f.map(|i| hull.points[i]);
                            [[a, b], [b, c], [c, a]]
                        })
                        .collect();
                    overlay(&mut image, &camera, &edges, 2.0, *colour);
                }
                image.downsample(SUPER)
            })
            .collect();
        save(
            &Image::sheet(&views, 2, 6, [255; 4]),
            &format!("{}-{variant}-hulls.png", recipe.name()),
        );
    }
}
