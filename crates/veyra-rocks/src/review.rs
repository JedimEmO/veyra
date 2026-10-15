//! Pictures of shaped rocks for review: a small orthographic rasteriser on
//! the CPU, so a contact sheet needs no GPU and no Bevy.
//!
//! It is not the game's renderer and does not try to be. A neutral grey
//! times the vertex tint, a low sun with a shadow map, and the baked
//! ambient occlusion are enough to judge facets, strata, moss and
//! silhouette; hull edges can be drawn over the top to check colliders.
#![allow(
    clippy::many_single_char_names,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::cast_precision_loss,
    reason = "pixel arithmetic and geometry notation"
)]
use ashlar::glam::DVec3;
use ashlar::{TriangleMesh, VertexData};

/// An sRGB image, eight bits per channel, rows top to bottom.
#[derive(Clone, Debug, PartialEq)]
pub struct Image {
    /// Width in pixels.
    pub width: u32,
    /// Height in pixels.
    pub height: u32,
    /// `width * height` RGBA texels.
    pub rgba: Vec<u8>,
}

impl Image {
    /// An image filled with one colour.
    #[must_use]
    pub fn filled(width: u32, height: u32, rgba: [u8; 4]) -> Self {
        Self {
            width,
            height,
            rgba: rgba.repeat((width * height) as usize),
        }
    }

    fn put(&mut self, x: u32, y: u32, rgba: [u8; 4]) {
        let i = ((y * self.width + x) * 4) as usize;
        self.rgba[i..i + 4].copy_from_slice(&rgba);
    }

    fn get(&self, x: u32, y: u32) -> [u8; 4] {
        let i = ((y * self.width + x) * 4) as usize;
        [
            self.rgba[i],
            self.rgba[i + 1],
            self.rgba[i + 2],
            self.rgba[i + 3],
        ]
    }

    /// Copy `other` in with its top left corner at `(x, y)`, clipped.
    pub fn blit(&mut self, other: &Self, x: u32, y: u32) {
        for oy in 0..other.height {
            for ox in 0..other.width {
                let (tx, ty) = (x + ox, y + oy);
                if tx < self.width && ty < self.height {
                    self.put(tx, ty, other.get(ox, oy));
                }
            }
        }
    }

    /// Average `factor` by `factor` blocks, for anti-aliasing a render made
    /// at a multiple of the size wanted.
    #[must_use]
    pub fn downsample(&self, factor: u32) -> Self {
        let (w, h) = (self.width / factor, self.height / factor);
        let mut out = Self::filled(w, h, [0; 4]);
        let n = factor * factor;
        for y in 0..h {
            for x in 0..w {
                let mut sum = [0_u32; 4];
                for dy in 0..factor {
                    for dx in 0..factor {
                        let p = self.get(x * factor + dx, y * factor + dy);
                        for c in 0..4 {
                            sum[c] += u32::from(p[c]);
                        }
                    }
                }
                out.put(x, y, sum.map(|s| (s / n) as u8));
            }
        }
        out
    }

    /// Lay images out in a grid, left to right and top to bottom, each in a
    /// cell as big as the largest, with `gap` pixels between.
    #[must_use]
    pub fn sheet(images: &[Self], columns: u32, gap: u32, background: [u8; 4]) -> Self {
        let cell_w = images.iter().map(|i| i.width).max().unwrap_or(1);
        let cell_h = images.iter().map(|i| i.height).max().unwrap_or(1);
        let columns = columns.max(1);
        let count = u32::try_from(images.len()).expect("a sheet of a few images");
        let rows = count.div_ceil(columns);
        let mut out = Self::filled(
            columns * (cell_w + gap) + gap,
            rows * (cell_h + gap) + gap,
            background,
        );
        for (i, image) in (0..).zip(images) {
            let (col, row) = (i % columns, i / columns);
            out.blit(
                image,
                gap + col * (cell_w + gap),
                gap + row * (cell_h + gap),
            );
        }
        out
    }

    /// Draw a line segment `width` pixels wide.
    fn line(&mut self, a: DVec3, b: DVec3, width: f64, rgba: [u8; 4]) {
        let length = (b - a).truncate().length().max(1.0);
        let steps = (length * 2.0).ceil() as u32;
        let r = (width * 0.5).ceil() as i64;
        for i in 0..=steps {
            let p = a.lerp(b, f64::from(i) / f64::from(steps));
            for dy in -r..=r {
                for dx in -r..=r {
                    let (x, y) = (p.x as i64 + dx, p.y as i64 + dy);
                    if x >= 0 && y >= 0 && x < i64::from(self.width) && y < i64::from(self.height) {
                        self.put(x as u32, y as u32, rgba);
                    }
                }
            }
        }
    }
}

/// Where the camera looks from. Orthographic, so a view is a direction and a
/// scale.
#[derive(Clone, Copy, Debug)]
pub struct Camera {
    /// Turn about the vertical, radians; 0 looks along `-Z`.
    pub yaw: f64,
    /// How far the camera looks down, radians.
    pub pitch: f64,
    /// The point at the middle of the picture.
    pub centre: DVec3,
    /// Pixels per metre.
    pub scale: f64,
    /// Picture width in pixels.
    pub width: u32,
    /// Picture height in pixels.
    pub height: u32,
}

impl Camera {
    /// A camera at `yaw` and `pitch` framing everything in `bounds`, the
    /// picture `pixels` tall and `aspect` times as wide.
    #[must_use]
    pub fn framing(bounds: [DVec3; 2], yaw: f64, pitch: f64, pixels: u32, aspect: f64) -> Self {
        let (right, up, _) = basis(yaw, pitch);
        let [min, max] = bounds;
        let centre = (min + max) * 0.5;
        let mut reach_x: f64 = 0.0;
        let mut reach_y: f64 = 0.0;
        for i in 0..8 {
            let corner = DVec3::new(
                if i & 1 == 0 { min.x } else { max.x },
                if i & 2 == 0 { min.y } else { max.y },
                if i & 4 == 0 { min.z } else { max.z },
            ) - centre;
            reach_x = reach_x.max(corner.dot(right).abs());
            reach_y = reach_y.max(corner.dot(up).abs());
        }
        let width = (f64::from(pixels) * aspect).round() as u32;
        let scale = (f64::from(pixels) / (reach_y * 2.1))
            .min(f64::from(width) / (reach_x * 2.1))
            .max(1e-3);
        Self {
            yaw,
            pitch,
            centre,
            scale,
            width,
            height: pixels,
        }
    }

    fn project(&self, p: DVec3) -> DVec3 {
        let (right, up, forward) = basis(self.yaw, self.pitch);
        let d = p - self.centre;
        DVec3::new(
            f64::from(self.width) * 0.5 + d.dot(right) * self.scale,
            f64::from(self.height) * 0.5 - d.dot(up) * self.scale,
            d.dot(forward),
        )
    }
}

/// How to colour a picture.
#[derive(Clone, Copy, Debug)]
pub struct Style {
    /// The rock's grey before the tint, linear.
    pub grey: f64,
    /// Background, sRGB.
    pub background: [u8; 4],
    /// Black on the background, with no lighting.
    pub silhouette: bool,
    /// Towards the sun, unit length.
    pub sun: DVec3,
    /// Draw a ground plane at `y = 0` under the rock, in the sun's shadow.
    pub ground: bool,
}

impl Style {
    /// Lit, with a low sun from the left of the review cameras.
    #[must_use]
    pub fn lit() -> Self {
        Self {
            grey: 0.3,
            background: [214, 226, 236, 255],
            silhouette: false,
            sun: DVec3::new(-0.8, 0.5, 0.3).normalize(),
            ground: false,
        }
    }

    /// Black on white.
    #[must_use]
    pub fn silhouette() -> Self {
        Self {
            grey: 0.0,
            background: [255; 4],
            silhouette: true,
            sun: DVec3::Y,
            ground: false,
        }
    }
}

/// A view basis: right, up and forward (into the picture).
fn basis(yaw: f64, pitch: f64) -> (DVec3, DVec3, DVec3) {
    let forward = DVec3::new(
        yaw.sin() * pitch.cos(),
        -pitch.sin(),
        -yaw.cos() * pitch.cos(),
    );
    let right = forward.cross(DVec3::Y).normalize();
    let up = right.cross(forward);
    (right, up, forward)
}

#[derive(Clone, Copy)]
struct Fragment {
    world: DVec3,
    normal: DVec3,
    ao: f64,
    tint: [f64; 3],
}

fn ao_of(mesh: &TriangleMesh, i: usize) -> f64 {
    match mesh.attributes.get("shade") {
        Some(VertexData::F32x4(values)) => f64::from(values[i][0]),
        _ => 1.0,
    }
}

fn tint_of(mesh: &TriangleMesh, i: usize) -> [f64; 3] {
    mesh.colors
        .as_ref()
        .map_or([1.0; 3], |c| [0, 1, 2].map(|k| f64::from(c[i][k])))
}

/// Rasterise `mesh` with a depth buffer, calling `fragment` for the nearest
/// surface at each covered pixel.
fn raster(
    mesh: &TriangleMesh,
    project: impl Fn(DVec3) -> DVec3,
    width: u32,
    height: u32,
    mut fragment: impl FnMut(u32, u32, &Fragment),
) -> Vec<f64> {
    let mut depth = vec![f64::INFINITY; (width * height) as usize];
    let mut nearest: Vec<Option<Fragment>> = vec![None; (width * height) as usize];
    let screen: Vec<DVec3> = mesh.positions.iter().map(|&p| project(p)).collect();
    for tri in mesh.indices.as_chunks::<3>().0 {
        let [i0, i1, i2] = tri.map(|i| i as usize);
        let (a, b, c) = (screen[i0], screen[i1], screen[i2]);
        let area = (b.x - a.x) * (c.y - a.y) - (b.y - a.y) * (c.x - a.x);
        if area.abs() < 1e-12 {
            continue;
        }
        let x0 = a.x.min(b.x).min(c.x).floor().max(0.0) as u32;
        let y0 = a.y.min(b.y).min(c.y).floor().max(0.0) as u32;
        let x1 = (a.x.max(b.x).max(c.x).ceil().max(0.0) as u32).min(width.saturating_sub(1));
        let y1 = (a.y.max(b.y).max(c.y).ceil().max(0.0) as u32).min(height.saturating_sub(1));
        for y in y0..=y1 {
            for x in x0..=x1 {
                let (px, py) = (f64::from(x) + 0.5, f64::from(y) + 0.5);
                let w0 = ((b.x - px) * (c.y - py) - (b.y - py) * (c.x - px)) / area;
                let w1 = ((c.x - px) * (a.y - py) - (c.y - py) * (a.x - px)) / area;
                let w2 = 1.0 - w0 - w1;
                if w0 < 0.0 || w1 < 0.0 || w2 < 0.0 {
                    continue;
                }
                let z = a.z * w0 + b.z * w1 + c.z * w2;
                let slot = (y * width + x) as usize;
                if z >= depth[slot] {
                    continue;
                }
                depth[slot] = z;
                let (t0, t1, t2) = (tint_of(mesh, i0), tint_of(mesh, i1), tint_of(mesh, i2));
                nearest[slot] = Some(Fragment {
                    world: mesh.positions[i0] * w0
                        + mesh.positions[i1] * w1
                        + mesh.positions[i2] * w2,
                    normal: (mesh.normals[i0] * w0 + mesh.normals[i1] * w1 + mesh.normals[i2] * w2)
                        .normalize_or(DVec3::Y),
                    ao: ao_of(mesh, i0) * w0 + ao_of(mesh, i1) * w1 + ao_of(mesh, i2) * w2,
                    tint: [0, 1, 2].map(|k| t0[k] * w0 + t1[k] * w1 + t2[k] * w2),
                });
            }
        }
    }
    for y in 0..height {
        for x in 0..width {
            if let Some(f) = &nearest[(y * width + x) as usize] {
                fragment(x, y, f);
            }
        }
    }
    depth
}

fn srgb(linear: f64) -> u8 {
    let c = linear.clamp(0.0, 1.0);
    let s = if c <= 0.003_130_8 {
        c * 12.92
    } else {
        1.055 * c.powf(1.0 / 2.4) - 0.055
    };
    (s * 255.0).round() as u8
}

/// Render one mesh.
#[must_use]
#[allow(clippy::too_many_lines, reason = "one pass after another, in order")]
pub fn render(mesh: &TriangleMesh, camera: &Camera, style: &Style) -> Image {
    let (w, h) = (camera.width, camera.height);
    let mut image = Image::filled(w, h, style.background);
    if style.silhouette {
        let _ = raster(
            mesh,
            |p| camera.project(p),
            w,
            h,
            |x, y, _| {
                image.put(x, y, [0, 0, 0, 255]);
            },
        );
        return image;
    }
    // A shadow map from the sun, over the mesh's bounds.
    let (min, max) = mesh.positions.iter().fold(
        (DVec3::splat(f64::INFINITY), DVec3::splat(f64::NEG_INFINITY)),
        |(a, b), p| (a.min(*p), b.max(*p)),
    );
    let middle = (min + max) * 0.5;
    let extent = (max - min).length().max(0.1) * 1.6;
    let shadow_size = 1024_u32;
    let shadow_scale = f64::from(shadow_size) / extent;
    let sun_yaw = (-style.sun.x).atan2(style.sun.z);
    let sun_pitch = style.sun.y.clamp(-1.0, 1.0).asin();
    let (sr, su, sf) = basis(sun_yaw, sun_pitch);
    let sun_project = |p: DVec3| {
        let d = p - middle;
        DVec3::new(
            f64::from(shadow_size) * 0.5 + d.dot(sr) * shadow_scale,
            f64::from(shadow_size) * 0.5 - d.dot(su) * shadow_scale,
            d.dot(sf),
        )
    };
    let shadow = raster(mesh, sun_project, shadow_size, shadow_size, |_, _, _| {});
    let lit = |p: DVec3| {
        let s = sun_project(p);
        let mut open = 0.0;
        for (dx, dy) in [(0.0, 0.0), (1.5, 0.0), (-1.5, 0.0), (0.0, 1.5), (0.0, -1.5)] {
            let (x, y) = (s.x + dx, s.y + dy);
            if x < 0.0 || y < 0.0 || x >= f64::from(shadow_size) || y >= f64::from(shadow_size) {
                open += 1.0;
                continue;
            }
            let d = shadow[(y as u32 * shadow_size + x as u32) as usize];
            if s.z <= d + extent * 0.003 {
                open += 1.0;
            }
        }
        open / 5.0
    };
    let sky = [0.55, 0.62, 0.75];
    let sun_colour = [1.3, 1.15, 0.95];
    if style.ground {
        // The ground plane first, shaded by the rock's shadow, so the rock
        // is seen sitting in it.
        let (right, up, forward) = basis(camera.yaw, camera.pitch);
        for y in 0..h {
            for x in 0..w {
                let sx = (f64::from(x) + 0.5 - f64::from(w) * 0.5) / camera.scale;
                let sy = -(f64::from(y) + 0.5 - f64::from(h) * 0.5) / camera.scale;
                let origin = camera.centre + right * sx + up * sy;
                if forward.y.abs() < 1e-6 {
                    continue;
                }
                let t = -origin.y / forward.y;
                let hit = origin + forward * t;
                let open = lit(hit);
                let ground = [0.20, 0.19, 0.15];
                let rgb = [0, 1, 2].map(|c| {
                    srgb(ground[c] * (sky[c] * 0.35 + sun_colour[c] * style.sun.y * open))
                });
                image.put(x, y, [rgb[0], rgb[1], rgb[2], 255]);
            }
        }
    }
    let _ = raster(
        mesh,
        |p| camera.project(p),
        w,
        h,
        |x, y, f| {
            if style.ground && f.world.y < 0.0 {
                return;
            }
            let n = f.normal;
            let diffuse = n.dot(style.sun).max(0.0);
            // Offset along the normal, so a face does not shadow itself.
            let shadowed = if diffuse > 0.0 {
                lit(f.world + n * extent * 0.008)
            } else {
                0.0
            };
            let ao = f.ao.clamp(0.0, 1.0);
            let ambient = 0.4 * ao * (0.55 + 0.45 * (n.y * 0.5 + 0.5));
            let rgb = [0, 1, 2].map(|c| {
                srgb(
                    style.grey
                        * f.tint[c]
                        * (sky[c] * ambient
                            + sun_colour[c] * diffuse * shadowed * (0.5 + 0.5 * ao)),
                )
            });
            image.put(x, y, [rgb[0], rgb[1], rgb[2], 255]);
        },
    );
    image
}

/// Draw line segments over a picture made with `camera`: hull edges, for
/// checking a collider against the rock.
pub fn overlay(
    image: &mut Image,
    camera: &Camera,
    edges: &[[DVec3; 2]],
    width: f64,
    rgba: [u8; 4],
) {
    for [a, b] in edges {
        image.line(camera.project(*a), camera.project(*b), width, rgba);
    }
}
