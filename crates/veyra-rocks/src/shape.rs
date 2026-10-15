//! From a recipe and a variant to a rock's field, in the rock's own frame:
//! centred on the origin for a loose rock (the builder then sinks it), and
//! standing on the origin, facing `+Z`, for a cliff piece.
//!
//! Each geology is a handful of art-directed rules. Granite is faceted
//! masses: rounded boxes cut by many fracture planes spread round them, so
//! the rock reads as bold planes with softened edges. Sandstone is slabs
//! squared off by vertical joints and quantised into strata of varying
//! thickness, some standing proud and some undercut. Variety comes from
//! proportions and from where the planes fall, not from noise, because a
//! stylised rock reads by its planes.
use core::f64::consts::{PI, TAU};

use ashlar::glam::{DVec2, DVec3};

use crate::field::{Block, Cut, Layer, Rock, Strata};
use crate::noise::Noise;
use crate::rng::Rng;
use crate::{CliffPiece, Detail, Geology, RockRecipe};

/// The golden angle, which spreads successive facets round a rock without
/// two landing side by side.
const GOLDEN: f64 = 2.399_963_229_728_653;

/// The size a variant gets: the eight variants are spread evenly over the
/// recipe's range, in a shuffled order so variant 0 is not always the
/// smallest.
pub(crate) fn size(recipe: &RockRecipe, variant: u32, rng: &mut Rng) -> f64 {
    // A fixed shuffle of 0..8: small, large and middling alternate.
    const ORDER: [u32; 8] = [3, 7, 0, 5, 2, 6, 1, 4];
    let slot = ORDER[variant as usize % ORDER.len()];
    let [low, high] = recipe.size;
    let t = (f64::from(slot) + rng.range(0.15, 0.85)) / 8.0;
    low + (high - low) * t
}

/// The field of a variant, before it is placed on the ground.
pub(crate) fn shape(recipe: &RockRecipe, variant: u32, rng: &mut Rng) -> Rock {
    let size = size(recipe, variant, rng);
    let noise = Noise::new(rng.next_u64());
    match (recipe.geology, recipe.piece(variant)) {
        (Geology::Granite, None) => granite_boulder(size, rng, noise),
        (Geology::Sandstone, None) => {
            sandstone_boulder(size, recipe.detail == Detail::Stone, rng, noise)
        }
        (geology, Some(piece)) => cliff(geology, piece, size, rng, noise),
    }
}

/// A direction at azimuth `azimuth` (radians from `+Z` towards `+X`) with
/// vertical component `y`.
fn direction(azimuth: f64, y: f64) -> DVec3 {
    let y = y.clamp(-1.0, 1.0);
    let flat = (1.0 - y * y).sqrt();
    DVec3::new(azimuth.sin() * flat, y, azimuth.cos() * flat)
}

/// A fracture plane across `block` along `normal`, keeping the inner `keep`
/// share of the block's reach that way.
fn fracture(block: &Block, normal: DVec3, keep: f64, soft: f64) -> Cut {
    let reach = block.support(normal);
    let centre = block.centre.dot(normal);
    Cut {
        normal,
        offset: centre + (reach - centre) * keep,
        soft,
    }
}

/// A plane through `point` facing `normal`.
fn plane(normal: DVec3, point: DVec3, soft: f64) -> Cut {
    Cut {
        normal,
        offset: normal.dot(point),
        soft,
    }
}

/// How a faceted mass is cut.
struct Facets {
    count: u32,
    /// The lowest a facet normal points, as its vertical component: a
    /// buried underside needs none.
    lowest: f64,
    /// The share of the reach each facet keeps, least and most.
    keep: [f64; 2],
    /// How much of the facets' spread is folded towards the front (`+Z`):
    /// 0 all round, 1 the front half only.
    front: f64,
    /// How much deeper a facet facing straight up cuts than one facing
    /// sideways, as a share of the reach: a rock that narrows to its top
    /// sits heavy on the ground.
    taper: f64,
    soft: f64,
}

/// Cut `block` by facets spread evenly over the sphere above `lowest`: a
/// golden-angle spiral, jittered, so the planes cover the rock without
/// crowding.
fn facet(block: &mut Block, facets: &Facets, rng: &mut Rng) {
    let start = rng.range(0.0, TAU);
    for i in 0..facets.count {
        let t = (f64::from(i) + rng.range(0.25, 0.75)) / f64::from(facets.count);
        let y = facets.lowest + (0.97 - facets.lowest) * t;
        let mut azimuth = start + f64::from(i) * GOLDEN + rng.signed() * 0.3;
        if facets.front > 0.0 {
            let wrapped = (azimuth + PI).rem_euclid(TAU) - PI;
            azimuth = wrapped * (1.0 - facets.front * 0.5);
        }
        let normal = direction(azimuth, y);
        let keep = rng.range(facets.keep[0], facets.keep[1]) - facets.taper * y.max(0.0);
        block.cuts.push(fracture(block, normal, keep, facets.soft));
    }
}

fn granite_boulder(size: f64, rng: &mut Rng, noise: Noise) -> Rock {
    let radius = size * 0.5;
    let half = DVec3::new(
        radius,
        radius * rng.range(0.5, 0.85),
        radius * rng.range(0.55, 0.9),
    );
    let yaw = rng.range(0.0, TAU);
    let mut main = Block::new(
        DVec3::ZERO,
        half,
        half.min_element() * rng.range(0.25, 0.6),
        yaw,
    );
    main.tilt(rng.range(0.0, TAU), rng.range(0.05, 0.3));
    facet(
        &mut main,
        &Facets {
            count: rng.between(7, 11),
            lowest: -0.35,
            keep: [0.58, 0.86],
            front: 0.0,
            taper: 0.15,
            soft: size * rng.range(0.035, 0.045),
        },
        rng,
    );
    let mut blocks = vec![main];

    // Sometimes a second mass fused on, off one end and lower: the lumpy,
    // two-lobed boulders a field of them needs among the plain ones.
    if rng.chance(0.4) {
        let along = DVec3::new(yaw.cos(), 0.0, -yaw.sin());
        let end = if rng.chance(0.5) { 1.0 } else { -1.0 };
        let scale = rng.range(0.45, 0.7);
        let centre = along * end * half.x * rng.range(0.6, 0.85)
            + DVec3::Y * -half.y * rng.range(0.1, 0.35)
            + DVec3::new(-along.z, 0.0, along.x) * half.z * rng.signed() * 0.3;
        let lobe_half = half * scale * DVec3::new(1.0, rng.range(0.9, 1.25), 1.0);
        let mut lobe = Block::new(
            centre,
            lobe_half,
            lobe_half.min_element() * rng.range(0.4, 0.8),
            yaw + rng.signed() * 0.8,
        );
        facet(
            &mut lobe,
            &Facets {
                count: rng.between(4, 7),
                lowest: -0.3,
                keep: [0.58, 0.86],
                front: 0.0,
                taper: 0.1,
                soft: size * 0.035,
            },
            rng,
        );
        blocks.push(lobe);
    }
    Rock {
        blocks,
        blend: size * 0.05,
        strata: None,
        warp: (DVec3::splat(size * 0.02), 0.8 / size),
        bump: (size * 0.001, 4.0 / size),
        floor: None,
        noise,
        shift: DVec3::ZERO,
    }
}

/// A bedded slab; a `stone` is a chunkier fragment of one, since a slab
/// thinner than a hand would vanish into the grass.
fn sandstone_boulder(size: f64, stone: bool, rng: &mut Rng, noise: Noise) -> Rock {
    let radius = size * 0.5;
    let tall = if stone { [0.5, 0.8] } else { [0.35, 0.6] };
    let half = DVec3::new(
        radius,
        radius * rng.range(tall[0], tall[1]),
        radius * rng.range(0.55, 0.9),
    );
    let yaw = rng.range(0.0, TAU);
    let mut main = Block::new(DVec3::ZERO, half, half.min_element() * 0.35, yaw);
    // Joints: near-vertical planes that square the slab off, and a bedding
    // plane across the top, tilted a little.
    let joints = rng.between(3, 5);
    let start = rng.range(0.0, TAU);
    for i in 0..joints {
        let azimuth = start + TAU * f64::from(i) / f64::from(joints) + rng.signed() * 0.35;
        let normal = direction(azimuth, rng.range(-0.05, 0.15));
        let keep = rng.range(0.72, 0.92);
        main.cuts.push(fracture(&main, normal, keep, size * 0.045));
    }
    let tilt = direction(rng.range(0.0, TAU), rng.range(0.94, 0.995));
    let keep = rng.range(0.82, 0.95);
    main.cuts.push(fracture(&main, tilt, keep, size * 0.045));
    let mut blocks = vec![main];

    // Sometimes a smaller slab stacked on top, off centre: a stepped block.
    if rng.chance(0.4) {
        let scale = rng.range(0.5, 0.75);
        let offset = DVec3::new(rng.signed(), 0.0, rng.signed()) * radius * 0.25;
        let top_half = DVec3::new(
            half.x * scale,
            half.y * rng.range(0.5, 0.75),
            half.z * scale,
        );
        let centre = offset + DVec3::Y * (half.y * 0.7 + top_half.y * 0.5);
        let mut top = Block::new(
            centre,
            top_half,
            top_half.min_element() * 0.35,
            yaw + rng.signed() * 0.4,
        );
        let normal = direction(rng.range(0.0, TAU), rng.range(-0.05, 0.1));
        let keep = rng.range(0.65, 0.85);
        top.cuts.push(fracture(&top, normal, keep, size * 0.045));
        blocks.push(top);
    }

    // The erosion is capped by the thinnest slab, or a small stacked slab
    // is eaten through to a film where its strata recede.
    let [low, high] = vertical_span(&blocks);
    let thinnest = blocks
        .iter()
        .map(|b| b.half.x.min(b.half.z))
        .fold(f64::INFINITY, f64::min);
    let erosion = (thinnest * 0.3 / (size * 0.105)).min(1.0);
    let strata = strata(
        low,
        high,
        [(high - low) / 3.2, (high - low) / 2.0],
        rng,
        &StrataStyle {
            shift: size * 0.012,
            inset: size * 0.03 * erosion,
            climb: 0.0,
            narrow: size * 0.04 * erosion,
            undercut: size * 0.025 * erosion,
            round: 0.3,
            min_round: size * 0.08,
        },
    );
    Rock {
        blocks,
        blend: size * 0.04,
        strata: Some(strata),
        warp: (DVec3::new(1.0, 0.3, 1.0) * size * 0.03, 0.8 / size),
        bump: (0.0, 0.0),
        floor: None,
        noise,
        shift: DVec3::ZERO,
    }
}

fn vertical_span(blocks: &[Block]) -> [f64; 2] {
    blocks
        .iter()
        .fold([f64::INFINITY, f64::NEG_INFINITY], |[lo, hi], b| {
            [lo.min(b.centre.y - b.half.y), hi.max(b.centre.y + b.half.y)]
        })
}

/// How a stack of strata varies layer to layer.
struct StrataStyle {
    /// Largest horizontal shift of a layer, metres.
    shift: f64,
    /// Largest inset of a soft layer, metres.
    inset: f64,
    /// How far the top layer stands back towards `-Z`, growing from none at
    /// the bottom: a cliff face stepping back as it climbs.
    climb: f64,
    /// How far the top layer is inset all round, growing from none at the
    /// bottom: flanks and skyline that step down like a mesa's.
    narrow: f64,
    /// Largest undercut of a soft layer, metres.
    undercut: f64,
    /// Edge rounding over the layer's thickness.
    round: f64,
    /// The least edge rounding, metres: under about two grid steps of the
    /// nearest level an edge beads instead of rounding.
    min_round: f64,
}

/// Layers from below `low` to past `high`, each between `thickness[0]` and
/// `thickness[1]` thick. About a third are hard: they stand proud with no
/// undercut, the ledges that make sandstone read as bedded; the soft ones
/// between them recede and are undercut.
fn strata(low: f64, high: f64, thickness: [f64; 2], rng: &mut Rng, style: &StrataStyle) -> Strata {
    let mut bounds = vec![low - thickness[1]];
    while let Some(&last) = bounds.last()
        && last < high + thickness[1]
    {
        bounds.push(last + rng.range(thickness[0], thickness[1]));
    }
    let span = (high - low).max(1e-6);
    let layers = bounds
        .windows(2)
        .map(|w| {
            let up = ((w[0] - low) / span).clamp(0.0, 1.0);
            let hard = rng.chance(0.5);
            let shift = DVec2::new(rng.signed(), rng.signed()) * style.shift
                - DVec2::new(0.0, style.climb * up);
            Layer {
                shift,
                inset: style.narrow * up
                    + if hard {
                        0.0
                    } else {
                        rng.range(0.3, 1.0) * style.inset
                    },
                undercut: if hard {
                    0.0
                } else {
                    rng.range(0.2, 1.0) * style.undercut
                },
                round: ((w[1] - w[0]) * style.round * rng.range(0.8, 1.2)).max(style.min_round),
            }
        })
        .collect();
    Strata { bounds, layers }
}

/// A cliff kit piece `height` metres tall above its base, its foot running
/// 30% of that below it.
fn cliff(geology: Geology, piece: CliffPiece, height: f64, rng: &mut Rng, noise: Noise) -> Rock {
    let height = match piece {
        CliffPiece::Wall => height.max(13.0),
        CliffPiece::Buttress => height.clamp(11.0, 22.0),
        CliffPiece::Ledge => 10.0 + (height - 8.0) * 0.2,
    };
    let foot = -0.3 * height;
    // Every mass reaches below the irregular floor, which then trims them
    // all together into one ragged underside.
    let bottom = foot - height * 0.12;
    let sandstone = geology == Geology::Sandstone;
    let layout = Layout {
        height,
        bottom,
        sandstone,
    };
    let mut blocks = match piece {
        CliffPiece::Wall => layout.wall(rng),
        CliffPiece::Buttress => layout.buttress(rng),
        CliffPiece::Ledge => layout.ledge(rng),
    };
    let root = layout.root(&blocks, rng);
    blocks.push(root);
    let strata = sandstone.then(|| {
        let climb = rng.range(2.0, 5.0);
        let narrow = rng.range(0.5, 2.0);
        let [low, high] = vertical_span(&blocks);
        strata(
            low,
            high,
            [1.0, 3.5],
            rng,
            &StrataStyle {
                shift: 0.1,
                inset: 0.6,
                climb,
                narrow,
                undercut: 0.6,
                round: 0.15,
                min_round: 0.5,
            },
        )
    });
    Rock {
        blocks,
        blend: if sandstone { 0.6 } else { 0.5 },
        strata,
        warp: (
            if sandstone {
                DVec3::new(1.0, 0.15, 1.0) * height * 0.03
            } else {
                DVec3::splat(height * 0.012)
            },
            0.05,
        ),
        bump: (0.015, 0.4),
        floor: Some((foot, height * 0.06, 0.12)),
        noise,
        shift: DVec3::ZERO,
    }
}

/// What every piece of one cliff shares.
struct Layout {
    height: f64,
    bottom: f64,
    sandstone: bool,
}

impl Layout {
    /// A mass standing from the cliff's bottom up to `top`, leaning back
    /// `lean` radians, cut as its geology breaks: granite into bold facets
    /// over its front and a sloping head, sandstone squared by a vertical
    /// joint or two under a flat bedding top.
    fn mass(&self, at: DVec2, top: f64, half: DVec2, yaw: f64, lean: f64, rng: &mut Rng) -> Block {
        let half = DVec3::new(half.x, (top - self.bottom) * 0.5, half.y);
        let centre = DVec3::new(at.x, self.bottom.midpoint(top), at.y);
        let head = DVec3::new(at.x, top, at.y);
        if self.sandstone {
            let mut block = Block::new(centre, half, 0.5, yaw);
            block.tilt(yaw, -lean);
            for _ in 0..rng.between(2, 4) {
                let normal = direction(rng.signed() * 2.2, rng.range(-0.03, 0.06));
                let keep = rng.range(0.82, 0.95);
                block.cuts.push(fracture(&block, normal, keep, 0.6));
            }
            let bedding = direction(rng.range(0.0, TAU), rng.range(0.97, 0.998));
            block.cuts.push(plane(bedding, head, 0.6));
            block
        } else {
            let round = half.x.min(half.z) * rng.range(0.08, 0.15);
            let mut block = Block::new(centre, half, round, yaw);
            block.tilt(yaw, -lean);
            // The head first, so a mass ends in a bold sloping plane.
            let slope = direction(rng.range(0.0, TAU), rng.range(0.55, 0.9));
            block.cuts.push(plane(slope, head, 0.6));
            facet(
                &mut block,
                &Facets {
                    count: rng.between(6, 9),
                    lowest: -0.15,
                    keep: [0.62, 0.86],
                    front: 0.3,
                    taper: 0.05,
                    soft: 0.45,
                },
                rng,
            );
            block
        }
    }

    /// The piece's back, running into the slope: a mass across the whole
    /// width reaching 30% of the height further towards `-Z`, its top a rough
    /// ramp falling away from just under the crest. Placement turns `+Z`
    /// down a slope of 45 to 70 degrees, so the ground rises behind the
    /// piece and swallows the ramp, where a plumb back would stand clear of
    /// the hill with a hard edge along its top.
    fn root(&self, blocks: &[Block], rng: &mut Rng) -> Block {
        let reach = |dir: DVec3| {
            blocks
                .iter()
                .map(|b| b.support(dir))
                .fold(f64::NEG_INFINITY, f64::max)
        };
        let (left, right) = (-reach(DVec3::NEG_X), reach(DVec3::X));
        let back = -reach(DVec3::NEG_Z);
        let length = self.height * 0.3;
        let crest = self.height * rng.range(0.7, 0.85);
        let centre = DVec3::new(
            left.midpoint(right),
            self.bottom.midpoint(crest),
            back - length * 0.5 + 1.0,
        );
        let half = DVec3::new(
            (right - left) * 0.45,
            (crest - self.bottom) * 0.5,
            length * 0.5 + 1.0,
        );
        let mut root = Block::new(centre, half, half.min_element() * 0.15, rng.signed() * 0.05);
        // The ramp falls towards -Z at 35 to 55 degrees from just behind
        // the masses' backs.
        let fall = rng.range(35.0_f64, 55.0).to_radians();
        let ramp = DVec3::new(rng.signed() * 0.1, fall.cos(), -fall.sin()).normalize();
        root.cuts
            .push(plane(ramp, DVec3::new(0.0, crest, back + 1.0), 0.6));
        if !self.sandstone {
            facet(
                &mut root,
                &Facets {
                    count: rng.between(3, 5),
                    lowest: 0.2,
                    keep: [0.8, 0.95],
                    front: 0.0,
                    taper: 0.0,
                    soft: 0.6,
                },
                rng,
            );
        }
        root
    }

    /// A loose chunk resting on or against the cliff: a fallen block at its
    /// foot or one perched on a head.
    fn chunk(&self, centre: DVec3, half: DVec3, rng: &mut Rng) -> Block {
        let yaw = rng.range(0.0, TAU);
        let mut block = Block::new(centre, half, half.min_element() * 0.12, yaw);
        if self.sandstone {
            let bedding = direction(rng.range(0.0, TAU), rng.range(0.95, 0.995));
            let keep = rng.range(0.8, 0.92);
            block.cuts.push(fracture(&block, bedding, keep, 0.6));
        } else {
            block.tilt(rng.range(0.0, TAU), rng.range(0.1, 0.35));
            facet(
                &mut block,
                &Facets {
                    count: rng.between(6, 9),
                    lowest: -0.3,
                    keep: [0.55, 0.85],
                    front: 0.0,
                    taper: 0.1,
                    soft: 0.35,
                },
                rng,
            );
        }
        block
    }

    /// Fallen chunks at the foot of the face, partly buried.
    fn talus(&self, blocks: &mut Vec<Block>, width: f64, front: f64, count: u32, rng: &mut Rng) {
        for _ in 0..count {
            let half = DVec3::new(
                rng.range(2.0, 3.5),
                rng.range(1.6, 2.8),
                rng.range(1.6, 2.8),
            );
            let centre = DVec3::new(
                rng.signed() * width * 0.35,
                half.y * rng.range(0.1, 0.5),
                front + rng.range(-0.5, 0.8),
            );
            blocks.push(self.chunk(centre, half, rng));
        }
    }

    /// Two or three leaning masses across the width, their heads at
    /// different heights and their faces at different depths and angles,
    /// with fallen chunks at the foot: a craggy wall. Sandstone's wall is
    /// one or two plumb masses whose strata do the work.
    fn wall(&self, rng: &mut Rng) -> Vec<Block> {
        let width = (self.height * rng.range(0.9, 1.3)).clamp(12.0, 26.0);
        let depth = rng.range(6.0, 9.0);
        let masses = if self.sandstone {
            rng.between(1, 2)
        } else {
            rng.between(2, 3)
        };
        let tall = rng.between(0, masses - 1);
        let w = width / f64::from(masses);
        let mut blocks = Vec::new();
        for i in 0..masses {
            let top = if i == tall {
                self.height
            } else {
                self.height * rng.range(0.72, 0.95)
            };
            let at = DVec2::new(
                -width * 0.5 + w * (f64::from(i) + 0.5) + rng.signed() * w * 0.12,
                rng.signed() * depth * 0.12,
            );
            let half = DVec2::new(w * 0.65, depth * 0.5 * rng.range(0.8, 1.1));
            let (yaw, lean) = if self.sandstone {
                (rng.signed() * 0.08, rng.range(0.0, 0.04))
            } else {
                (rng.signed() * 0.25, rng.range(0.06, 0.2))
            };
            blocks.push(self.mass(at, top, half, yaw, lean, rng));
        }
        if !self.sandstone {
            let count = rng.between(1, 2);
            self.talus(&mut blocks, width, depth * 0.5, count, rng);
        }
        blocks
    }

    /// A prow standing out of the slope: a deep mass whose front is two
    /// planes meeting at a ridge, leaning back a little, with lower
    /// shoulders on either side.
    fn buttress(&self, rng: &mut Rng) -> Vec<Block> {
        let width = rng.range(8.0, 11.0);
        let depth = rng.range(9.0, 12.0);
        let half = DVec2::new(width * 0.5, depth * 0.5);
        let lean = if self.sandstone {
            0.02
        } else {
            rng.range(0.05, 0.12)
        };
        let mut main = self.mass(
            DVec2::ZERO,
            self.height,
            half,
            rng.signed() * 0.1,
            lean,
            rng,
        );
        let spread = rng.range(0.6, 0.9);
        let ridge = DVec3::new(rng.signed() * width * 0.1, 0.0, depth * 0.45);
        for side in [-1.0, 1.0] {
            let normal = direction(side * spread, rng.range(0.03, 0.1));
            main.cuts.push(plane(normal, ridge, 0.9));
        }
        let mut blocks = vec![main];
        for side in [-1.0, 1.0] {
            if blocks.len() > 1 && rng.chance(0.4) {
                continue;
            }
            let top = self.height * rng.range(0.4, 0.7);
            let half = DVec2::new(width * rng.range(0.3, 0.45), depth * rng.range(0.3, 0.4));
            let at = DVec2::new(
                side * (width * 0.5 + half.x * 0.4),
                -depth * rng.range(0.0, 0.2),
            );
            let lean = lean * rng.range(0.5, 1.5);
            blocks.push(self.mass(at, top, half, rng.signed() * 0.3, lean, rng));
        }
        if !self.sandstone {
            self.talus(&mut blocks, width, depth * 0.45, 1, rng);
        }
        blocks
    }

    /// A wide, low piece: a broad top on slabs of nearly one height, and a
    /// lower step standing out in front.
    fn ledge(&self, rng: &mut Rng) -> Vec<Block> {
        let width = rng.range(16.0, 22.0);
        let depth = rng.range(10.0, 13.0);
        let slabs = rng.between(2, 3);
        let w = width / f64::from(slabs);
        let mut blocks = Vec::new();
        for i in 0..slabs {
            let top = if i == 0 {
                self.height
            } else {
                self.height * rng.range(0.85, 1.0)
            };
            let at = DVec2::new(
                -width * 0.5 + w * (f64::from(i) + 0.5) + rng.signed() * w * 0.1,
                rng.signed() * 0.6 - depth * 0.1,
            );
            let half = DVec2::new(w * 0.62, depth * 0.5 * rng.range(0.9, 1.05));
            let lean = if self.sandstone {
                0.0
            } else {
                rng.range(0.03, 0.1)
            };
            let mut block = self.mass(at, top, half, rng.signed() * 0.12, lean, rng);
            // The ledge's top stays broad: flatten whatever head the mass
            // was given back towards level.
            let level = direction(rng.range(0.0, TAU), rng.range(0.985, 0.999));
            block
                .cuts
                .push(plane(level, DVec3::new(at.x, top, at.y), 0.7));
            blocks.push(block);
        }
        let step_top = self.height * rng.range(0.4, 0.6);
        let step_width = width * rng.range(0.4, 0.65);
        let at = DVec2::new(rng.signed() * (width - step_width) * 0.4, depth * 0.45);
        let half = DVec2::new(step_width * 0.5, depth * 0.3);
        blocks.push(self.mass(at, step_top, half, rng.signed() * 0.2, 0.0, rng));
        if !self.sandstone {
            self.talus(&mut blocks, width, depth * 0.7, 1, rng);
        }
        blocks
    }
}
