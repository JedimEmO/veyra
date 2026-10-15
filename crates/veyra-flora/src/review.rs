//! Pictures of grown trees for review: a small orthographic rasteriser on the
//! CPU, so a contact sheet needs no GPU and no Bevy.
//!
//! It is not the game's renderer and does not try to be. Bark and leaves are
//! flat colours, leaf cards are cut out by a procedural cluster of round
//! leaves standing in for the painted card, and a low sun with a shadow map
//! and the baked ambient occlusion is enough to judge a crown's masses,
//! gaps and silhouette.
#![allow(
    clippy::many_single_char_names,
    reason = "geometry reads best in its own notation: p, n, r, s"
)]
use ashlar::glam::DVec3;
use ashlar::prop::PropLevel;
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
                #[allow(clippy::cast_possible_truncation)]
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

/// How to colour a picture.
#[derive(Clone, Copy, Debug)]
pub struct Style {
    /// Bark colour, linear RGB.
    pub bark: [f64; 3],
    /// Leaf colour, linear RGB.
    pub leaves: [f64; 3],
    /// Background, sRGB.
    pub background: [u8; 4],
    /// Black on the background, with no lighting.
    pub silhouette: bool,
    /// Towards the sun, unit length.
    pub sun: DVec3,
    /// What cuts the leaf piece's cards out.
    pub cards: Cutout,
}

/// The stand-in for a leaf card's painted alpha.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Cutout {
    /// No cut-out: the leaf piece is solid geometry, as a flower's is.
    Solid,
    /// A cluster of round leaves on a twig: [`leaf_alpha`].
    Cluster,
    /// A frond section with pinnae either side of a rachis: [`frond_alpha`].
    Frond,
}

impl Cutout {
    fn alpha(self, u: f64, v: f64) -> Option<usize> {
        match self {
            Self::Solid => Some(0),
            Self::Cluster => leaf_alpha(u, v),
            Self::Frond => frond_alpha(u, v),
        }
    }
}

impl Style {
    /// Lit, with a low sun from the front left.
    #[must_use]
    pub fn lit(bark: [f64; 3], leaves: [f64; 3]) -> Self {
        Self {
            bark,
            leaves,
            background: [214, 226, 236, 255],
            silhouette: false,
            sun: DVec3::new(-0.75, 0.38, 0.55).normalize(),
            cards: Cutout::Cluster,
        }
    }

    /// The same, with another card cut-out.
    #[must_use]
    pub fn with_cards(self, cards: Cutout) -> Self {
        Self { cards, ..self }
    }

    /// Black on white.
    #[must_use]
    pub fn silhouette() -> Self {
        Self {
            bark: [0.0; 3],
            leaves: [0.0; 3],
            background: [255; 4],
            silhouette: true,
            sun: DVec3::Y,
            cards: Cutout::Cluster,
        }
    }
}

/// Whether card UV `(u, v)` is inside the stand-in leaf cluster: a stem up
/// the middle from the attachment and a dozen round leaves around it.
#[must_use]
pub fn leaf_alpha(u: f64, v: f64) -> Option<usize> {
    const LEAVES: [(f64, f64, f64); 12] = [
        (0.50, 0.16, 0.14),
        (0.32, 0.26, 0.13),
        (0.68, 0.24, 0.13),
        (0.18, 0.44, 0.12),
        (0.82, 0.42, 0.12),
        (0.40, 0.40, 0.12),
        (0.60, 0.38, 0.12),
        (0.28, 0.62, 0.11),
        (0.72, 0.60, 0.11),
        (0.50, 0.56, 0.11),
        (0.38, 0.78, 0.09),
        (0.63, 0.77, 0.09),
    ];
    for (i, (x, y, r)) in LEAVES.iter().enumerate() {
        let (dx, dy) = (u - x, (v - y) * 0.85);
        if dx * dx + dy * dy < r * r {
            return Some(i);
        }
    }
    ((u - 0.5).abs() < 0.015 && v > 0.2 && v < 0.97).then_some(99)
}

/// Whether card UV `(u, v)` is inside the stand-in frond: a rachis from
/// the stem to the tip at `v` 0.06, and eleven pairs of pinnae at about 70
/// degrees to it, longest a third of the way up, tapering to the tip.
#[must_use]
pub fn frond_alpha(u: f64, v: f64) -> Option<usize> {
    let along = (u - 0.5).abs();
    if along < 0.012 && (0.06..0.95).contains(&v) {
        return Some(99);
    }
    for k in 0..11_u32 {
        let t = (f64::from(k) + 0.5) / 11.0;
        let centre = 0.9 - t * 0.8;
        let lance = if t < 0.33 {
            0.75 + 0.25 * t / 0.33
        } else {
            1.0 - 0.9 * (t - 0.33) / 0.67
        };
        let reach = 0.44 * lance;
        let middle = centre - along * 0.36;
        let width = 0.035 * (1.0 - along / reach).max(0.0).sqrt();
        if along < reach && (v - middle).abs() < width {
            return Some(k as usize);
        }
    }
    None
}

struct Vertex {
    screen: DVec3,
    world: DVec3,
    normal: DVec3,
    uv: [f64; 2],
    ao: f64,
    colour: DVec3,
}

fn vertex_colour(mesh: &TriangleMesh, i: usize) -> DVec3 {
    mesh.colors.as_ref().map_or(DVec3::ONE, |c| {
        DVec3::new(f64::from(c[i][0]), f64::from(c[i][1]), f64::from(c[i][2]))
    })
}

fn shade_ao(mesh: &TriangleMesh, i: usize) -> f64 {
    match mesh.attributes.get("shade") {
        Some(VertexData::F32x4(values)) => f64::from(values[i][0]),
        _ => 1.0,
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

/// Rasterise every triangle of `pieces`, calling `fragment` for each covered
/// pixel that passes the depth test with the interpolated vertex, the piece
/// index and the pixel.
#[allow(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::cast_precision_loss
)]
fn raster(
    pieces: &[(&TriangleMesh, Option<Cutout>)],
    project: impl Fn(DVec3) -> DVec3,
    width: u32,
    height: u32,
    mut fragment: impl FnMut(u32, u32, usize, &Vertex),
) -> Vec<f64> {
    let mut depth = vec![f64::INFINITY; (width * height) as usize];
    // Two passes: depth first with cut-outs, then shading of the nearest.
    for pass in 0..2 {
        for (piece, (mesh, leaf)) in pieces.iter().enumerate() {
            for tri in mesh.indices.as_chunks::<3>().0 {
                let v: Vec<Vertex> = tri
                    .iter()
                    .map(|&i| {
                        let i = i as usize;
                        Vertex {
                            screen: project(mesh.positions[i]),
                            world: mesh.positions[i],
                            normal: mesh.normals[i],
                            uv: mesh.uvs[i],
                            ao: shade_ao(mesh, i),
                            colour: vertex_colour(mesh, i),
                        }
                    })
                    .collect();
                let (a, b, c) = (v[0].screen, v[1].screen, v[2].screen);
                let area = (b.x - a.x) * (c.y - a.y) - (b.y - a.y) * (c.x - a.x);
                if area.abs() < 1e-12 {
                    continue;
                }
                let x0 = a.x.min(b.x).min(c.x).floor().max(0.0) as u32;
                let y0 = a.y.min(b.y).min(c.y).floor().max(0.0) as u32;
                let x1 =
                    (a.x.max(b.x).max(c.x).ceil().max(0.0) as u32).min(width.saturating_sub(1));
                let y1 =
                    (a.y.max(b.y).max(c.y).ceil().max(0.0) as u32).min(height.saturating_sub(1));
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
                        let uv = [
                            v[0].uv[0] * w0 + v[1].uv[0] * w1 + v[2].uv[0] * w2,
                            v[0].uv[1] * w0 + v[1].uv[1] * w1 + v[2].uv[1] * w2,
                        ];
                        if leaf.is_some_and(|c| c.alpha(uv[0], uv[1]).is_none()) {
                            continue;
                        }
                        if pass == 0 {
                            if z < depth[slot] {
                                depth[slot] = z;
                            }
                        } else if (z - depth[slot]).abs() < 1e-9 {
                            let vertex = Vertex {
                                screen: DVec3::new(px, py, z),
                                world: v[0].world * w0 + v[1].world * w1 + v[2].world * w2,
                                normal: (v[0].normal * w0 + v[1].normal * w1 + v[2].normal * w2)
                                    .normalize_or(DVec3::Y),
                                uv,
                                ao: v[0].ao * w0 + v[1].ao * w1 + v[2].ao * w2,
                                colour: v[0].colour * w0 + v[1].colour * w1 + v[2].colour * w2,
                            };
                            fragment(x, y, piece, &vertex);
                        }
                    }
                }
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
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let byte = (s * 255.0).round() as u8;
    byte
}

/// Render a prop level: its first piece as bark, its second as leaf cards.
#[must_use]
#[allow(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::cast_precision_loss,
    clippy::too_many_lines,
    reason = "one render pass, shadow map then shading, read top to bottom"
)]
pub fn render(level: &PropLevel, camera: &Camera, style: &Style) -> Image {
    let pieces: Vec<(&TriangleMesh, Option<Cutout>)> = level
        .pieces
        .iter()
        .enumerate()
        .map(|(i, p)| (&p.mesh, (i == 1).then_some(style.cards)))
        .collect();
    let (right, up, forward) = basis(camera.yaw, camera.pitch);
    let (w, h) = (camera.width, camera.height);
    let project = |p: DVec3| {
        let d = p - camera.centre;
        DVec3::new(
            f64::from(w) * 0.5 + d.dot(right) * camera.scale,
            f64::from(h) * 0.5 - d.dot(up) * camera.scale,
            d.dot(forward),
        )
    };
    let mut image = Image::filled(w, h, style.background);
    if style.silhouette {
        let _ = raster(&pieces, project, w, h, |x, y, _, _| {
            image.put(x, y, [0, 0, 0, 255]);
        });
        return image;
    }
    // A shadow map from the sun, over the prop's bounds.
    let (min, max) = pieces.iter().flat_map(|(m, _)| m.positions.iter()).fold(
        (DVec3::splat(f64::INFINITY), DVec3::splat(f64::NEG_INFINITY)),
        |(a, b), p| (a.min(*p), b.max(*p)),
    );
    let middle = (min + max) * 0.5;
    let extent = (max - min).length().max(0.1);
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
    let shadow = raster(
        &pieces,
        sun_project,
        shadow_size,
        shadow_size,
        |_, _, _, _| {},
    );
    let lit = |p: DVec3| {
        let s = sun_project(p);
        let mut open = 0.0;
        for (dx, dy) in [(0.0, 0.0), (1.0, 0.0), (-1.0, 0.0), (0.0, 1.0), (0.0, -1.0)] {
            let (x, y) = (s.x + dx, s.y + dy);
            if x < 0.0 || y < 0.0 || x >= f64::from(shadow_size) || y >= f64::from(shadow_size) {
                open += 1.0;
                continue;
            }
            let d = shadow[(y as u32 * shadow_size + x as u32) as usize];
            if s.z <= d + extent * 0.004 {
                open += 1.0;
            }
        }
        open / 5.0
    };
    let sky = [0.55, 0.62, 0.75];
    let sun_colour = [1.25, 1.1, 0.9];
    let _ = raster(&pieces, project, w, h, |x, y, piece, v| {
        let leaf = piece == 1;
        let mut base = if leaf { style.leaves } else { style.bark };
        if leaf
            && style.cards != Cutout::Solid
            && let Some(i) = style.cards.alpha(v.uv[0], v.uv[1])
        {
            let tint = 0.9 + 0.2 * ((i * 7 % 5) as f64 / 4.0);
            base = base.map(|c| c * tint);
        }
        let base = [
            base[0] * v.colour.x,
            base[1] * v.colour.y,
            base[2] * v.colour.z,
        ];
        let n = v.normal;
        let ndl = n.dot(style.sun);
        let diffuse = if leaf {
            // Wrapped, with light through the leaf from behind.
            ((ndl * 0.5 + 0.5).powi(2) + (-ndl).max(0.0) * 0.25).min(1.0)
        } else {
            ndl.max(0.0)
        };
        let shadowed = lit(v.world);
        let ao = v.ao.clamp(0.0, 1.0);
        let ambient = 0.35 * ao * (0.6 + 0.4 * (n.y * 0.5 + 0.5));
        let rgb: Vec<u8> = (0..3)
            .map(|c| {
                srgb(
                    base[c]
                        * (sky[c] * ambient
                            + sun_colour[c] * diffuse * shadowed * (0.35 + 0.65 * ao)),
                )
            })
            .collect();
        image.put(x, y, [rgb[0], rgb[1], rgb[2], 255]);
    });
    image
}

/// A camera framing a box from the side at `yaw`, `pixels` tall, with the
/// ground at the bottom of the frame.
#[must_use]
pub fn side_camera(bounds: [DVec3; 2], yaw: f64, pixels: u32, aspect: f64) -> Camera {
    let [min, max] = bounds;
    let height = (max.y - min.y.min(0.0)).max(0.1);
    let width = (max.x - min.x).max(max.z - min.z).max(0.1);
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let w = (f64::from(pixels) * aspect).round() as u32;
    let scale = (f64::from(pixels) / (height * 1.06)).min(f64::from(w) / (width * 1.02));
    let centre = DVec3::new(
        min.x.midpoint(max.x),
        min.y.min(0.0) + f64::from(pixels) * 0.5 / scale - height * 0.02,
        min.z.midpoint(max.z),
    );
    Camera {
        yaw,
        pitch: 0.0,
        centre,
        scale,
        width: w,
        height: pixels,
    }
}
