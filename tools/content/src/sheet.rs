//! Contact sheets of the weapon pictures, for a person to judge (ticket
//! 0008).
//!
//! The pictures are accepted by eye before any screen draws them, and a
//! directory of a hundred and thirty small two-channel files is not
//! something an eye can judge. A sheet lays one maker's weapons out as the
//! game will use them: each chassis stacked from its pieces by their
//! rectangles, then every mount's alternatives side by side, then the
//! square icons a cell of a grid will draw for them, all drawn by their
//! luminance over one dark ground.
//!
//! A sheet is made from the bake's files and its index alone, never from
//! the rasteriser's own buffers, so what a person looks at is what the
//! client would read: a wrong rectangle or a file under the wrong name
//! shows here as a weapon put together wrongly.
use std::collections::BTreeMap;

use anyhow::Context;
use strata_registry::pictures::{ChassisPictures, PictureSet};
use veyra_weapons::{Maker, Weapon};

/// The ground every piece is drawn over: dark, as the interface's panels
/// are, so the tones are judged against what they will sit on.
const GROUND: [u8; 3] = [0x14, 0x1A, 0x24];
/// Labels a person reads first: the maker, a chassis, a piece's name.
const INK: [u8; 3] = [0xC4, 0xCE, 0xDC];
/// Labels a person reads second: sizes and scales.
const FAINT: [u8; 3] = [0x6E, 0x7B, 0x90];
/// The outline of a chassis's frame, just off the ground so it marks the
/// frame without competing with the picture.
const RULE: [u8; 3] = [0x2C, 0x37, 0x4A];

/// Clear space round a sheet and between a chassis and the next.
const MARGIN: usize = 40;
/// Clear space between two pictures on a line.
const GAP: usize = 28;
/// Extra space between one mount's alternatives and the next mount's.
const MOUNT_GAP: usize = 36;
/// Pixels to a dot of the face for a piece's label and its size.
const SMALL: usize = 2;
/// Pixels to a dot of the face for a chassis's heading.
const HEADING: usize = 3;
/// Pixels to a dot of the face for the maker's name.
const TITLE: usize = 5;

/// One contact sheet per maker, as a file name and the bytes of an RGB PNG.
///
/// `files` and `set` are what [`crate::weapons`] returned.
///
/// # Errors
///
/// A chassis the index does not hold, a file it names that was not baked,
/// or a file whose size is not its rectangle's.
pub fn weapon_sheets(
    files: &[(String, Vec<u8>)],
    set: &PictureSet,
) -> anyhow::Result<Vec<(String, Vec<u8>)>> {
    let files: BTreeMap<&str, &[u8]> = files
        .iter()
        .map(|(path, data)| (path.as_str(), data.as_slice()))
        .collect();
    Maker::ALL
        .into_iter()
        .map(|maker| {
            let blocks = Weapon::ALL
                .into_iter()
                .filter(|weapon| weapon.maker() == maker)
                .map(|weapon| {
                    let chassis = set
                        .chassis
                        .get(weapon.key())
                        .with_context(|| format!("the index has no {}", weapon.key()))?;
                    Block::read(weapon.key(), chassis, &files)
                })
                .collect::<anyhow::Result<Vec<_>>>()?;
            let name = maker.label().to_lowercase();
            let sheet = sheet(&name, &blocks);
            let data = crate::png(
                &sheet.rgb,
                u32::try_from(sheet.width)?,
                u32::try_from(sheet.height)?,
                image::ExtendedColorType::Rgb8,
            )?;
            Ok((format!("{name}.png"), data))
        })
        .collect()
}

/// A picture read back from its file: luminance then alpha, rows from the
/// top.
struct Raster {
    width: usize,
    height: usize,
    data: Vec<u8>,
}

impl Raster {
    /// Read the file the index names, and hold it to the rectangle the
    /// index gives it: a client sizes a piece by its rectangle and would
    /// stretch a file of another size.
    fn read(files: &BTreeMap<&str, &[u8]>, file: &str, rect: [u32; 4]) -> anyhow::Result<Self> {
        let bytes = files
            .get(file)
            .with_context(|| format!("the index names {file}, which was not baked"))?;
        let image = image::load_from_memory_with_format(bytes, image::ImageFormat::Png)
            .with_context(|| file.to_owned())?
            .into_luma_alpha8();
        anyhow::ensure!(
            [image.width(), image.height()] == [rect[2], rect[3]],
            "{file} is {} by {} and its rectangle is {} by {}",
            image.width(),
            image.height(),
            rect[2],
            rect[3]
        );
        Ok(Self {
            width: image.width() as usize,
            height: image.height() as usize,
            data: image.into_raw(),
        })
    }
}

/// One alternative of one mount: its picture and what is written under it.
struct Tile {
    /// Which mount of the chassis it belongs to, counted in the order the
    /// index lists them, so a line is not broken inside a mount if it can
    /// be helped.
    mount: usize,
    picture: Raster,
    /// The file's name without its extension: the mount and the module.
    label: String,
    /// The picture's size in pixels, and what the index says against it:
    /// that the chassis hides it, or that it is another's picture again.
    /// These are what a person at the sheet has to decide about.
    size: String,
}

impl Tile {
    fn width(&self) -> usize {
        self.picture
            .width
            .max(text_width(&self.label, SMALL))
            .max(text_width(&self.size, SMALL))
    }
}

/// Everything a sheet shows of one chassis.
struct Block {
    heading: String,
    scale: String,
    /// The frame's size in pixels.
    frame: [usize; 2],
    /// The body and each mount's standard part the chassis does not hide,
    /// with where each goes in the frame, in the order they stack.
    stock: Vec<([usize; 2], Raster)>,
    /// The stock weapon in one picture and where it goes in the frame.
    whole: ([usize; 2], Raster),
    /// Every mount's alternatives, the standard part first.
    tiles: Vec<Tile>,
    /// The square icons, the whole weapon's first and then each piece's in
    /// the index's order: what a cell of a grid draws.
    icons: Vec<Raster>,
}

impl Block {
    fn read(
        key: &str,
        chassis: &ChassisPictures,
        files: &BTreeMap<&str, &[u8]>,
    ) -> anyhow::Result<Self> {
        let at = |rect: [u32; 4]| [rect[0] as usize, rect[1] as usize];
        let stock = chassis
            .stock()
            .map(|piece| {
                Ok((
                    at(piece.rect),
                    Raster::read(files, &piece.file, piece.rect)?,
                ))
            })
            .collect::<anyhow::Result<Vec<_>>>()?;
        let whole = (
            at(chassis.whole.rect),
            Raster::read(files, &chassis.whole.file, chassis.whole.rect)?,
        );
        let mut mounts: Vec<&str> = Vec::new();
        let mut tiles = Vec::new();
        for piece in &chassis.pieces {
            let Some(mount) = piece.mount.as_deref() else {
                continue;
            };
            if mounts.last() != Some(&mount) {
                mounts.push(mount);
            }
            let module = piece.module.as_deref().unwrap_or("standard");
            let mut size = format!("{}x{}", piece.rect[2], piece.rect[3]);
            if piece.hidden {
                size.push_str("  hidden by the chassis");
            }
            if let Some(like) = &piece.same_as {
                let stem = like
                    .rsplit_once('/')
                    .map_or(like.as_str(), |(_, name)| name);
                let stem = stem.strip_suffix(".png").unwrap_or(stem);
                size.push_str("  same as ");
                size.push_str(stem);
            }
            tiles.push(Tile {
                mount: mounts.len() - 1,
                picture: Raster::read(files, &piece.file, piece.rect)?,
                label: format!("{mount}.{module}"),
                size,
            });
        }
        let square = [0, 0, crate::WEAPON_ICON_SIDE, crate::WEAPON_ICON_SIDE];
        let icons = std::iter::once(&chassis.whole.icon)
            .chain(chassis.pieces.iter().map(|piece| &piece.icon))
            .flatten()
            .map(|icon| Raster::read(files, icon, square))
            .collect::<anyhow::Result<Vec<_>>>()?;
        Ok(Self {
            heading: key.to_owned(),
            scale: format!(
                "{}x{}   {:.3} mm per pixel   {} pieces",
                chassis.width,
                chassis.height,
                chassis.metres_per_pixel * 1000.0,
                chassis.pieces.len()
            ),
            frame: [chassis.width as usize, chassis.height as usize],
            stock,
            whole,
            tiles,
            icons,
        })
    }

    /// How many icons go on a line of a block `width` wide, and how many
    /// lines they take.
    fn icon_lines(&self, width: usize) -> (usize, usize) {
        let side = crate::WEAPON_ICON_SIDE as usize;
        let across = ((width + GAP) / (side + GAP)).max(1);
        (across, self.icons.len().div_ceil(across))
    }

    /// Where each tile goes in a block `width` wide: its left edge and its
    /// line, and each line's tallest picture.
    ///
    /// Tiles run left to right and break to a new line at the width. A
    /// mount that would be split by the break starts the new line whole,
    /// unless it is wider than a line by itself.
    fn flow(&self, width: usize) -> (Vec<(usize, usize)>, Vec<usize>) {
        let mut places = Vec::with_capacity(self.tiles.len());
        let mut lines = vec![0];
        let mut x = 0;
        for (index, tile) in self.tiles.iter().enumerate() {
            let starts_mount = index == 0 || self.tiles[index - 1].mount != tile.mount;
            let needed = if starts_mount {
                let mount: Vec<_> = self.tiles[index..]
                    .iter()
                    .take_while(|other| other.mount == tile.mount)
                    .collect();
                let run = mount.iter().map(|other| other.width()).sum::<usize>()
                    + GAP * (mount.len() - 1);
                if run <= width { run } else { tile.width() }
            } else {
                tile.width()
            };
            if x > 0 {
                x += if starts_mount { MOUNT_GAP } else { GAP };
                if x + needed > width {
                    x = 0;
                    lines.push(0);
                }
            }
            let line = lines.len() - 1;
            lines[line] = lines[line].max(tile.picture.height);
            places.push((x, line));
            x += tile.width();
        }
        (places, lines)
    }

    /// The block drawn `width` wide, as tall as it needs.
    fn draw(&self, width: usize) -> Canvas {
        let caption = line_height(SMALL);
        let (places, lines) = self.flow(width);
        let heading = line_height(HEADING) + caption + GAP / 2;
        let frames = self.frame[1] + 2 + caption + GAP;
        let tiles: usize = lines
            .iter()
            .map(|picture| picture + 2 * caption + GAP)
            .sum();
        let side = crate::WEAPON_ICON_SIDE as usize;
        let (across, icon_lines) = self.icon_lines(width);
        let icons = if icon_lines == 0 {
            0
        } else {
            caption + icon_lines * (side + GAP)
        };
        let mut canvas = Canvas::new(width, heading + frames + tiles + icons);
        canvas.text(0, 0, &self.heading, HEADING, INK);
        canvas.text(0, line_height(HEADING), &self.scale, SMALL, FAINT);

        // The frame twice: the pieces stacked as the client will stack
        // them, and the one picture a single depth buffer drew, so a piece
        // that stacks over one it should sit behind shows as a difference.
        let top = heading + 1;
        let second = self.frame[0] + 2 + GAP;
        for (left, label, pieces) in [
            (1, "stacked from its pieces", self.stock.as_slice()),
            (second + 1, "whole", std::slice::from_ref(&self.whole)),
        ] {
            canvas.outline(
                left - 1,
                top - 1,
                self.frame[0] + 2,
                self.frame[1] + 2,
                RULE,
            );
            for ([x, y], picture) in pieces {
                canvas.over(left + x, top + y, picture);
            }
            canvas.text(left - 1, top + self.frame[1] + 3, label, SMALL, FAINT);
        }

        let mut tops = Vec::with_capacity(lines.len());
        let mut y = heading + frames;
        for picture in &lines {
            tops.push(y);
            y += picture + 2 * caption + GAP;
        }
        for (tile, (x, line)) in self.tiles.iter().zip(places) {
            // Pictures stand on a common line so their labels do too.
            let foot = tops[line] + lines[line];
            canvas.over(x, foot - tile.picture.height, &tile.picture);
            canvas.text(x, foot + 4, &tile.label, SMALL, INK);
            canvas.text(x, foot + 4 + caption, &tile.size, SMALL, FAINT);
        }

        // The icons in their squares, each outlined so the margin a cell's
        // marks stand in is seen with the picture.
        if icon_lines > 0 {
            canvas.text(0, y, "icons  whole then each piece", SMALL, FAINT);
            for (index, icon) in self.icons.iter().enumerate() {
                let left = (index % across) * (side + GAP);
                let top = y + caption + (index / across) * (side + GAP);
                canvas.outline(left, top, side, side, RULE);
                canvas.over(left, top, icon);
            }
        }
        canvas
    }
}

/// One maker's blocks under its name.
fn sheet(maker: &str, blocks: &[Block]) -> Canvas {
    // Wide enough for the widest frame twice, which is every block's top
    // line; the alternatives below break to it.
    let width = blocks
        .iter()
        .map(|block| 2 * (block.frame[0] + 2) + GAP)
        .max()
        .unwrap_or(0)
        .max(text_width(maker, TITLE));
    let drawn: Vec<Canvas> = blocks.iter().map(|block| block.draw(width)).collect();
    let title = line_height(TITLE) + GAP;
    let height = 2 * MARGIN
        + title
        + drawn.iter().map(|block| block.height).sum::<usize>()
        + MARGIN * drawn.len().saturating_sub(1);
    let mut canvas = Canvas::new(width + 2 * MARGIN, height);
    canvas.text(MARGIN, MARGIN, maker, TITLE, INK);
    let mut y = MARGIN + title;
    for block in &drawn {
        canvas.paste(MARGIN, y, block);
        y += block.height + MARGIN;
    }
    canvas
}

/// An RGB picture being drawn: three bytes a pixel, rows from the top.
struct Canvas {
    width: usize,
    height: usize,
    rgb: Vec<u8>,
}

impl Canvas {
    fn new(width: usize, height: usize) -> Self {
        Self {
            width,
            height,
            rgb: GROUND.repeat(width * height),
        }
    }

    /// The pixel's three bytes, or nothing outside the canvas, so a label
    /// that runs off an edge is cut and not wrapped onto the next row.
    fn pixel(&mut self, x: usize, y: usize) -> Option<&mut [u8]> {
        if x >= self.width || y >= self.height {
            return None;
        }
        let at = (y * self.width + x) * 3;
        self.rgb.get_mut(at..at + 3)
    }

    /// Draw a picture by its luminance, covered by its alpha, over what is
    /// there.
    fn over(&mut self, left: usize, top: usize, picture: &Raster) {
        for y in 0..picture.height {
            for x in 0..picture.width {
                let at = (y * picture.width + x) * 2;
                let (tone, alpha) = (u32::from(picture.data[at]), u32::from(picture.data[at + 1]));
                if alpha == 0 {
                    continue;
                }
                let Some(pixel) = self.pixel(left + x, top + y) else {
                    continue;
                };
                for channel in pixel {
                    let mixed = (u32::from(*channel) * (255 - alpha) + tone * alpha + 127) / 255;
                    *channel = u8::try_from(mixed).unwrap_or(u8::MAX);
                }
            }
        }
    }

    fn dot(&mut self, x: usize, y: usize, colour: [u8; 3]) {
        if let Some(pixel) = self.pixel(x, y) {
            pixel.copy_from_slice(&colour);
        }
    }

    /// A rectangle's edge, one pixel wide, inside the size given.
    fn outline(&mut self, left: usize, top: usize, width: usize, height: usize, colour: [u8; 3]) {
        for x in left..left + width {
            self.dot(x, top, colour);
            self.dot(x, top + height - 1, colour);
        }
        for y in top..top + height {
            self.dot(left, y, colour);
            self.dot(left + width - 1, y, colour);
        }
    }

    /// Write `text` with its top-left corner at the point, `scale` pixels
    /// to a dot of the face.
    fn text(&mut self, left: usize, top: usize, text: &str, scale: usize, colour: [u8; 3]) {
        for (index, character) in text.chars().enumerate() {
            let rows = glyph(character);
            for (row, bits) in rows.iter().enumerate() {
                for column in 0..GLYPH_WIDTH {
                    if bits >> (GLYPH_WIDTH - 1 - column) & 1 == 0 {
                        continue;
                    }
                    let x = left + (index * ADVANCE + column) * scale;
                    let y = top + row * scale;
                    for dy in 0..scale {
                        for dx in 0..scale {
                            self.dot(x + dx, y + dy, colour);
                        }
                    }
                }
            }
        }
    }

    /// Copy another canvas in with its top-left corner at the point.
    fn paste(&mut self, left: usize, top: usize, other: &Canvas) {
        for y in 0..other.height {
            let from = y * other.width * 3;
            let row = &other.rgb[from..from + other.width * 3];
            for (x, colour) in row.as_chunks::<3>().0.iter().enumerate() {
                if let Some(pixel) = self.pixel(left + x, top + y) {
                    pixel.copy_from_slice(colour);
                }
            }
        }
    }
}

/// Dots across a glyph of the face.
const GLYPH_WIDTH: usize = 5;
/// Dots down a glyph of the face.
const GLYPH_HEIGHT: usize = 7;
/// Dots from one glyph's left edge to the next's.
const ADVANCE: usize = GLYPH_WIDTH + 1;

/// How wide `text` is written at `scale`, in pixels.
fn text_width(text: &str, scale: usize) -> usize {
    (text.chars().count() * ADVANCE).saturating_sub(1) * scale
}

/// How far a line of text at `scale` is from the next, in pixels.
fn line_height(scale: usize) -> usize {
    (GLYPH_HEIGHT + 3) * scale
}

/// The sheet's own face: capitals, digits, the underscore and the full
/// stop, five dots by seven, a row a byte with the left dot highest.
///
/// A label here is a file's name and a size, so this is every character a
/// sheet writes, and it saves the content step a font and the crate that
/// would read it. Small letters are written as their capitals; anything
/// else is a space.
const FACE: [[u8; GLYPH_HEIGHT]; 38] = [
    [0x0E, 0x11, 0x11, 0x1F, 0x11, 0x11, 0x11], // A
    [0x1E, 0x11, 0x11, 0x1E, 0x11, 0x11, 0x1E], // B
    [0x0E, 0x11, 0x10, 0x10, 0x10, 0x11, 0x0E], // C
    [0x1E, 0x11, 0x11, 0x11, 0x11, 0x11, 0x1E], // D
    [0x1F, 0x10, 0x10, 0x1E, 0x10, 0x10, 0x1F], // E
    [0x1F, 0x10, 0x10, 0x1E, 0x10, 0x10, 0x10], // F
    [0x0E, 0x11, 0x10, 0x17, 0x11, 0x11, 0x0F], // G
    [0x11, 0x11, 0x11, 0x1F, 0x11, 0x11, 0x11], // H
    [0x0E, 0x04, 0x04, 0x04, 0x04, 0x04, 0x0E], // I
    [0x07, 0x02, 0x02, 0x02, 0x02, 0x12, 0x0C], // J
    [0x11, 0x12, 0x14, 0x18, 0x14, 0x12, 0x11], // K
    [0x10, 0x10, 0x10, 0x10, 0x10, 0x10, 0x1F], // L
    [0x11, 0x1B, 0x15, 0x15, 0x11, 0x11, 0x11], // M
    [0x11, 0x19, 0x15, 0x13, 0x11, 0x11, 0x11], // N
    [0x0E, 0x11, 0x11, 0x11, 0x11, 0x11, 0x0E], // O
    [0x1E, 0x11, 0x11, 0x1E, 0x10, 0x10, 0x10], // P
    [0x0E, 0x11, 0x11, 0x11, 0x15, 0x12, 0x0D], // Q
    [0x1E, 0x11, 0x11, 0x1E, 0x14, 0x12, 0x11], // R
    [0x0F, 0x10, 0x10, 0x0E, 0x01, 0x01, 0x1E], // S
    [0x1F, 0x04, 0x04, 0x04, 0x04, 0x04, 0x04], // T
    [0x11, 0x11, 0x11, 0x11, 0x11, 0x11, 0x0E], // U
    [0x11, 0x11, 0x11, 0x11, 0x11, 0x0A, 0x04], // V
    [0x11, 0x11, 0x11, 0x15, 0x15, 0x1B, 0x11], // W
    [0x11, 0x11, 0x0A, 0x04, 0x0A, 0x11, 0x11], // X
    [0x11, 0x11, 0x0A, 0x04, 0x04, 0x04, 0x04], // Y
    [0x1F, 0x01, 0x02, 0x04, 0x08, 0x10, 0x1F], // Z
    [0x0E, 0x11, 0x13, 0x15, 0x19, 0x11, 0x0E], // 0
    [0x04, 0x0C, 0x04, 0x04, 0x04, 0x04, 0x0E], // 1
    [0x0E, 0x11, 0x01, 0x02, 0x04, 0x08, 0x1F], // 2
    [0x1E, 0x01, 0x01, 0x0E, 0x01, 0x01, 0x1E], // 3
    [0x02, 0x06, 0x0A, 0x12, 0x1F, 0x02, 0x02], // 4
    [0x1F, 0x10, 0x1E, 0x01, 0x01, 0x11, 0x0E], // 5
    [0x06, 0x08, 0x10, 0x1E, 0x11, 0x11, 0x0E], // 6
    [0x1F, 0x01, 0x02, 0x04, 0x08, 0x08, 0x08], // 7
    [0x0E, 0x11, 0x11, 0x0E, 0x11, 0x11, 0x0E], // 8
    [0x0E, 0x11, 0x11, 0x0F, 0x01, 0x02, 0x0C], // 9
    [0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x1F], // _
    [0x00, 0x00, 0x00, 0x00, 0x00, 0x0C, 0x0C], // .
];

/// The rows of a character's glyph, top first.
fn glyph(character: char) -> [u8; GLYPH_HEIGHT] {
    let upper = character.to_ascii_uppercase();
    let index = match upper {
        'A'..='Z' => upper as usize - 'A' as usize,
        '0'..='9' => 26 + upper as usize - '0' as usize,
        '_' => 36,
        '.' => 37,
        _ => return [0; GLYPH_HEIGHT],
    };
    FACE[index]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_face_has_a_glyph_for_every_character_a_label_uses() {
        let mut seen = std::collections::BTreeSet::new();
        for character in ('a'..='z')
            .chain('A'..='Z')
            .chain('0'..='9')
            .chain(['_', '.'])
        {
            let rows = glyph(character);
            assert!(rows.iter().any(|row| *row != 0), "{character} is blank");
            assert!(
                rows.iter().all(|row| *row < 1 << GLYPH_WIDTH),
                "{character} is wider than the face"
            );
            seen.insert(rows);
        }
        assert_eq!(seen.len(), FACE.len(), "two characters share a glyph");
        assert_eq!(glyph(' '), [0; GLYPH_HEIGHT]);
        assert_eq!(glyph('a'), glyph('A'));
    }

    #[test]
    fn a_picture_is_drawn_by_its_luminance_under_its_alpha() {
        let mut canvas = Canvas::new(4, 2);
        // Opaque white, half-covered white, and uncovered: the last must
        // leave the ground, whatever its luminance says.
        let picture = Raster {
            width: 3,
            height: 1,
            data: vec![255, 255, 255, 128, 255, 0],
        };
        canvas.over(1, 1, &picture);
        assert_eq!(
            &canvas.rgb[..12],
            GROUND.repeat(4).as_slice(),
            "the row above"
        );
        let row = &canvas.rgb[12..];
        assert_eq!(&row[..3], &GROUND, "left of the picture");
        assert_eq!(&row[3..6], &[255, 255, 255]);
        assert!(row[6] > GROUND[0] + 100 && row[6] < 160, "{}", row[6]);
        assert_eq!(&row[9..12], &GROUND);
        // Off the edge is cut, not wrapped.
        canvas.over(3, 0, &picture);
        assert_eq!(&canvas.rgb[..9], GROUND.repeat(3).as_slice());
        assert_eq!(&canvas.rgb[9..12], &[255, 255, 255]);
    }

    #[test]
    fn a_mount_is_not_split_across_lines_when_it_fits_one() {
        let tile = |mount, width| Tile {
            mount,
            picture: Raster {
                width,
                height: 10,
                data: vec![0; width * 10 * 2],
            },
            label: String::new(),
            size: String::new(),
        };
        let block = Block {
            heading: String::new(),
            scale: String::new(),
            frame: [100, 10],
            stock: Vec::new(),
            whole: (
                [0, 0],
                Raster {
                    width: 0,
                    height: 0,
                    data: Vec::new(),
                },
            ),
            tiles: vec![tile(0, 300), tile(0, 300), tile(1, 300), tile(1, 300)],
            icons: Vec::new(),
        };
        let (places, lines) = block.flow(1000);
        assert_eq!(lines.len(), 2);
        assert_eq!(
            places,
            vec![(0, 0), (300 + GAP, 0), (0, 1), (300 + GAP, 1)],
            "the second mount starts a line of its own"
        );
        // A mount wider than a line breaks where it must.
        let (places, lines) = block.flow(500);
        assert_eq!(lines.len(), 4);
        assert!(places.iter().all(|(x, _)| *x == 0));
    }
}
