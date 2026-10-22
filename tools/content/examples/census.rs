//! What a settlement's levels cost in entities, from the baked files: per
//! level the pieces (placed pieces, or a joined level's batches, which are
//! entities alike), the distinct material bindings and the triangles drawn,
//! and the same for the skyline the layout names (ticket 0002).
//! `cargo run --release -p content --example census -- [generated] [layout.ron]`.
use std::collections::BTreeSet;

use anyhow::{Context, Result};
use ashlar::{BakedBuilding, CookedBuilding};

/// A baked building of either version the content step writes.
enum Baked {
    Full(BakedBuilding),
    Cooked(CookedBuilding),
}

impl Baked {
    fn read(bytes: &[u8]) -> Result<Self> {
        Ok(if CookedBuilding::is_cooked(bytes) {
            Self::Cooked(CookedBuilding::read(bytes)?)
        } else {
            Self::Full(BakedBuilding::read(bytes)?)
        })
    }

    fn level_count(&self) -> usize {
        match self {
            Self::Full(baked) => baked.levels.len(),
            Self::Cooked(cooked) => cooked.levels.len(),
        }
    }

    fn group_bounds(&self) -> Vec<(String, [ashlar::glam::DVec3; 2])> {
        match self {
            Self::Full(baked) => baked
                .levels
                .iter()
                .flat_map(|level| &level.groups)
                .filter_map(|group| Some((group.id.clone(), group.bounds?)))
                .collect(),
            Self::Cooked(cooked) => cooked
                .levels
                .iter()
                .flat_map(|level| &level.groups)
                .filter_map(|group| Some((group.id.clone(), group.bounds?)))
                .collect(),
        }
    }
}

/// Pieces, distinct bindings and drawn triangles of one level, summed over
/// buildings. Pieces are counted as `ashlar-bevy` spawns them, since this
/// counts entities: a piece with no vertices is not spawned and not
/// counted. `content::building_stats` counts every piece the file holds, so
/// a kit that bakes an empty piece makes the two differ by it.
#[derive(Default)]
struct Census {
    pieces: usize,
    bindings: BTreeSet<String>,
    triangles: usize,
}

impl Census {
    fn add(&mut self, baked: &Baked, level: usize) -> Result<usize> {
        let mut count = 0;
        match baked {
            Baked::Full(baked) => {
                let meshed = baked.level(level).context("a level in range")?;
                for piece in meshed.pieces() {
                    if piece.mesh.positions.is_empty() {
                        continue;
                    }
                    count += 1;
                    self.triangles += piece.mesh.triangle_count();
                    self.bindings.insert(format!("{:?}", piece.binding));
                }
            }
            // A cooked file leaves the empty pieces out already.
            Baked::Cooked(cooked) => {
                for piece in cooked.pieces(level) {
                    count += 1;
                    self.triangles += cooked
                        .meshes
                        .get(piece.mesh as usize)
                        .map_or(0, |mesh| mesh.indices.len() / 3);
                    self.bindings.insert(format!("{:?}", piece.binding));
                }
            }
        }
        self.pieces += count;
        Ok(count)
    }
}

fn main() -> Result<()> {
    let root = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "generated".to_owned());
    let layout = std::env::args()
        .nth(2)
        .unwrap_or_else(|| "settlements/port_serein.ron".to_owned());
    let text = std::fs::read_to_string(format!("{root}/{layout}")).context("the layout")?;
    let layout: strata_worldgen::SettlementLayout = ron::from_str(&text)?;
    let read = |file: &str| -> Result<Baked> {
        let bytes = std::fs::read(format!("{root}/{file}")).with_context(|| file.to_owned())?;
        Baked::read(&bytes)
    };
    let mut levels: Vec<Census> = Vec::new();
    println!("file: pieces by level");
    for building in &layout.buildings {
        let baked = read(&building.file)?;
        if levels.len() < baked.level_count() {
            levels.resize_with(baked.level_count(), Census::default);
        }
        let own = (0..baked.level_count())
            .map(|level| levels[level].add(&baked, level))
            .collect::<Result<Vec<_>>>()?;
        println!("{}: {own:?}", building.file);
    }
    for (level, census) in levels.iter().enumerate() {
        println!(
            "level {level}: {} pieces, {} bindings, {} triangles drawn",
            census.pieces,
            census.bindings.len(),
            census.triangles
        );
    }
    match &layout.skyline {
        Some(file) => {
            let baked = read(file)?;
            let mut skyline = Census::default();
            for level in 0..baked.level_count() {
                skyline.add(&baked, level)?;
            }
            let bytes = std::fs::metadata(format!("{root}/{file}"))?.len();
            #[allow(clippy::cast_precision_loss, reason = "a byte count for people")]
            let mb = bytes as f64 / 1e6;
            println!(
                "skyline {file}: {} pieces, {} bindings, {} triangles, {mb:.1} MB",
                skyline.pieces,
                skyline.bindings.len(),
                skyline.triangles,
            );
            // Where it is drawn about, in the settlement's frame: a joined
            // level's batches stand at the centre of their group's bounds.
            for (id, [low, high]) in baked.group_bounds() {
                println!(
                    "  group {id}: bounds {:.1?} to {:.1?}",
                    low.to_array(),
                    high.to_array()
                );
            }
        }
        None => println!("no skyline in the layout"),
    }
    Ok(())
}
