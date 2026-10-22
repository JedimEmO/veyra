//! `cargo xtask`: developer commands that need more than a shell line.
//!
//! `shrink` scales a screenshot down for the docs: the game captures at
//! the window's size, which is several megabytes per frame, and the
//! repository wants a picture under a megabyte. Commands that belong here
//! later: baking texture arrays for the terrain material, regenerating
//! snapshot fixtures, and anything else that wants to be a Rust program
//! rather than a recipe.

use std::path::PathBuf;

use std::fs::File;
use std::io::BufWriter;

use anyhow::{Context, Result};
use clap::Parser;
use image::ImageEncoder;
use image::codecs::png::{CompressionType, FilterType as PngFilter, PngEncoder};
use image::imageops::FilterType;

#[derive(Parser, Debug)]
#[command(name = "xtask", version, about)]
struct Cli {
    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(clap::Subcommand, Debug)]
enum Command {
    /// Scale a PNG down to a width, keeping its aspect, and write it as PNG.
    Shrink {
        /// The image to read.
        input: PathBuf,
        /// Where to write the result.
        output: PathBuf,
        /// The output width in pixels.
        #[arg(long, default_value_t = 1280)]
        width: u32,
    },
    /// Pack every file the game reads through the files port into a gzipped
    /// data bundle, for the web build and for `game --data-bundle`.
    Bundle {
        /// Where to write the gzipped bundle (`dist/data.bundle.gz`).
        output: PathBuf,
        /// Also write the bundle uncompressed here: `game --data-bundle`
        /// reads this form, the browser unzips the other.
        #[arg(long)]
        raw: Option<PathBuf>,
        /// The checkout holding `assets/` and `generated/`.
        #[arg(long)]
        root: Option<PathBuf>,
        /// The seed whose rivers are baked into the bundle (the game's default).
        #[arg(long, default_value_t = 42)]
        seed: u64,
        /// Where the masses outside the bundle are written, as
        /// `generated/hydrology/mass/*.mass` under it: beside the output by
        /// default, which is `dist/` for the web build.
        #[arg(long)]
        far_dir: Option<PathBuf>,
    },
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    match cli.command {
        None => println!(
            "xtask: `cargo xtask shrink <in> <out> [--width N]`; see tools/xtask/src/main.rs"
        ),
        Some(Command::Shrink {
            input,
            output,
            width,
        }) => shrink(&input, &output, width)?,
        Some(Command::Bundle {
            output,
            raw,
            root,
            seed,
            far_dir,
        }) => {
            let root =
                root.unwrap_or_else(|| PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../.."));
            bundle(&root, &output, raw.as_deref(), far_dir.as_deref(), seed)?;
        }
    }
    Ok(())
}

fn shrink(input: &PathBuf, output: &PathBuf, width: u32) -> Result<()> {
    let image = image::open(input).with_context(|| format!("reading {}", input.display()))?;
    let height =
        (u64::from(width) * u64::from(image.height()) / u64::from(image.width().max(1))).max(1);
    let height = u32::try_from(height).context("the scaled height does not fit a u32")?;
    let scaled = image
        .resize_exact(width, height, FilterType::Lanczos3)
        .into_rgb8();
    // The best zlib level: these go into the repository, and a docs image
    // is written once and read forever.
    let file = File::create(output).with_context(|| format!("creating {}", output.display()))?;
    let encoder = PngEncoder::new_with_quality(
        BufWriter::new(file),
        CompressionType::Best,
        PngFilter::Adaptive,
    );
    encoder
        .write_image(
            scaled.as_raw(),
            scaled.width(),
            scaled.height(),
            image::ExtendedColorType::Rgb8,
        )
        .with_context(|| format!("writing {}", output.display()))?;
    let bytes = std::fs::metadata(output)?.len();
    println!(
        "{} -> {} ({width}x{height}, {} KB)",
        input.display(),
        output.display(),
        bytes / 1024
    );
    Ok(())
}

/// The logical roots the bundle takes files from.
const ROOTS: [&str; 2] = ["assets", "generated"];

/// Whether the files port reads `path` (`assets/...` or `generated/...`).
///
/// The bundle is what the page unpacks before its first frame, so a file goes
/// in only if a reader asks the port for it. `generated/` is an allow-list,
/// one arm per reader. `assets/` is read by directory and extension (the
/// registries list their folders), so it is the readable extensions minus the
/// folders and files only Bevy's asset server, over HTTP, or a tool opens.
fn read_by_port(path: &str) -> bool {
    let ext = std::path::Path::new(path)
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("");
    let Some((root, rest)) = path.split_once('/') else {
        return false;
    };
    let parts: Vec<&str> = rest.split('/').collect();
    let name = parts.last().copied().unwrap_or("");
    match root {
        "assets" => {
            matches!(ext, "ron" | "json" | "txt")
                // Fonts, the theme and the sounds are loaded by the asset
                // server; their licences and the leveller's report are read by no one.
                && !matches!(parts[0], "audio" | "fonts" | "themes")
                // The glTF import receipts beside the characters.
                && !(parts[0] == "characters" && ext == "json")
        }
        "generated" => match parts[0] {
            // Tree library: `TreeLibrary::read_from`.
            "flora" => rest == "flora/library.ron",
            // The layout (`settlements/<id>.ron`) and each building's
            // collision file (`ashlar::collision_file`), but never a `.ashlar`:
            // physics reads the small file, and the renderer fetches the
            // building itself over HTTP.
            "settlements" => {
                (parts.len() == 2 && ext == "ron") || (parts.len() == 3 && ext == "collision")
            }
            // The town site's depot is `buildings/outpost.ashlar`
            // (`veyra_settlements::sites::DEPOT`), named by a site layout and
            // hashed into `site_groups_version` as its collision file.
            "buildings" => parts.len() == 2 && ext == "collision",
            // Group layouts and their pieces' collision files; the crossings'
            // and the roadside kits the same.
            "sites" | "crossings" => {
                matches!(ext, "ron" | "collision")
                    && !name.ends_with(".materials.ron")
                    && name != "graphs.ron"
            }
            // `WeaponModels`, `PictureSet` and `BikeModels`; the models and
            // libraries themselves are fetched by the asset server.
            "weapons" => parts.len() == 3 && matches!(name, "models.ron" | "pictures.ron"),
            "vehicles" | "props" => parts.len() == 3 && name == "models.ron",
            // The terrain arrays' index (`strata_registry::terrain_arrays`),
            // which says whether the baked arrays are the ones to load; the
            // arrays themselves are fetched by the asset server.
            "terrain" => rest == "terrain/arrays.ron",
            _ => false,
        },
        _ => false,
    }
}

fn collect(root: &std::path::Path, logical: &str, out: &mut Vec<(String, Vec<u8>)>) -> Result<()> {
    let dir = root.join(logical);
    let mut entries = std::fs::read_dir(&dir)
        .with_context(|| format!("reading {}", dir.display()))?
        .collect::<std::io::Result<Vec<_>>>()?;
    entries.sort_by_key(std::fs::DirEntry::file_name);
    for entry in entries {
        let name = entry.file_name();
        let Some(name) = name.to_str() else { continue };
        let path = format!("{logical}/{name}");
        let kind = entry.file_type()?;
        if kind.is_dir() {
            collect(root, &path, out)?;
        } else if kind.is_file() && read_by_port(&path) {
            let bytes = std::fs::read(entry.path())
                .with_context(|| format!("reading {}", entry.path().display()))?;
            out.push((path, bytes));
        }
    }
    Ok(())
}

/// The length of `bytes` under `gzip -9`, what a browser would download.
fn gzip_len(bytes: &[u8]) -> Result<usize> {
    use std::io::Write;
    use std::process::{Command, Stdio};

    let mut gzip = Command::new("gzip")
        .args(["-9", "-c"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .context("running gzip")?;
    let mut stdin = gzip.stdin.take().context("gzip stdin")?;
    // Fed from a thread: a pipe holds 64 KB and gzip's output would fill it.
    let feeder = std::thread::scope(|scope| {
        let writer = scope.spawn(move || stdin.write_all(bytes));
        let out = gzip.wait_with_output();
        (writer.join(), out)
    });
    let (written, out) = feeder;
    written.map_err(|_| anyhow::anyhow!("gzip feeder panicked"))??;
    Ok(out?.stdout.len())
}

/// Bakes the rivers and adds the masses the start reads, and the index of
/// all of them, to the bundle's `entries`; the others are written under
/// `far_dir`.
fn water(
    root: &std::path::Path,
    output: &std::path::Path,
    far_dir: Option<&std::path::Path>,
    seed: u64,
    entries: &mut Vec<(String, Vec<u8>)>,
) -> Result<()> {
    // The bake a browser could not afford, done here on every thread (ticket
    // 0015, step 3): the rivers round the spawn and the settlements', as the
    // game would bake them before it starts, one file a mass. The masses the
    // start reads (within the web's radius) ride in the bundle; the others
    // are files beside the page that the game fetches ahead of the observer
    // (review 2026-10-16, F6).
    let started = std::time::Instant::now();
    let spawn = (0.0, 0.0);
    let area = |radius_m| strata_worldgen::hydrology::PrepareArea {
        places: vec![spawn],
        radius_m,
    };
    let (water, masses) = strata_worldgen::hydrology::bake_water_masses(
        &strata_registry::files::Files::dir(root),
        strata_core::Seed(seed),
        // Must equal veyra_sim::world_params(), which is the default; a different stamp makes the browser rebake.
        strata_worldgen::WorldParams::default(),
        &area(strata_worldgen::hydrology::WEB_PREPARE_RADIUS_M),
        &area(strata_worldgen::hydrology::PREPARE_WATER_RADIUS_M),
    )
    .context("baking the rivers")?;
    let far_dir = far_dir.map_or_else(
        || {
            output
                .parent()
                .unwrap_or_else(|| std::path::Path::new(""))
                .join(strata_worldgen::hydrology::MASS_DIR)
        },
        |dir| dir.join(strata_worldgen::hydrology::MASS_DIR),
    );
    if far_dir.exists() {
        for entry in std::fs::read_dir(&far_dir)? {
            let path = entry?.path();
            if path.extension().is_some_and(|e| e == "mass") {
                std::fs::remove_file(path)?;
            }
        }
    }
    std::fs::create_dir_all(&far_dir)?;
    let (mut near_gz, mut far_gz, mut near_raw, mut far_raw) = (0, 0, 0, 0);
    let mut near_count = 0;
    println!("rivers: mass key, nodes, raw bytes, gzip bytes, km from the spawn, where");
    for (entry, bytes) in &water.files {
        let gz = gzip_len(bytes)?;
        let near = water.is_near(entry);
        println!(
            "  {:>5},{:>5}{} {:>7} nodes {:>9} raw {:>9} gz {:>7.1} km  {}",
            entry.key[0],
            entry.key[1],
            if entry.capped { " cut" } else { "    " },
            entry.nodes,
            bytes.len(),
            gz,
            entry.distance_m(spawn.0, spawn.1) / 1000.0,
            if near { "bundle" } else { "fetched" }
        );
        if near {
            near_count += 1;
            near_raw += bytes.len();
            near_gz += gz;
            entries.push((entry.path(), bytes.clone()));
        } else {
            far_raw += bytes.len();
            far_gz += gz;
            let name = entry.path();
            let name = name.rsplit('/').next().unwrap_or(&name);
            std::fs::write(far_dir.join(name), bytes)
                .with_context(|| format!("writing {name} to {}", far_dir.display()))?;
        }
    }
    let index = water.index().to_bytes();
    println!(
        "rivers: {masses} masses baked in {:.1} s; {near_count} in the bundle ({} MB raw, {} MB gzip), \
         {} fetched ({} MB raw, {} MB gzip, in {}), index {} bytes",
        started.elapsed().as_secs_f64(),
        near_raw / 1_000_000,
        near_gz / 1_000_000,
        water.files.len() - near_count,
        far_raw / 1_000_000,
        far_gz / 1_000_000,
        far_dir.display(),
        index.len()
    );
    entries.push((strata_worldgen::hydrology::WATER_INDEX.to_owned(), index));
    Ok(())
}

/// Packs the bundle and gzips it with the `gzip` program, which keeps a
/// codec crate out of the workspace.
fn bundle(
    root: &std::path::Path,
    output: &PathBuf,
    raw: Option<&std::path::Path>,
    far_dir: Option<&std::path::Path>,
    seed: u64,
) -> Result<()> {
    use std::io::Write;
    use std::process::{Command, Stdio};

    let mut entries = Vec::new();
    for logical in ROOTS {
        collect(root, logical, &mut entries)?;
    }
    water(root, output, far_dir, seed, &mut entries)?;
    let blob = strata_registry::bundle::write(&entries);
    if let Some(raw) = raw {
        std::fs::write(raw, &blob).with_context(|| format!("writing {}", raw.display()))?;
    }
    if let Some(parent) = output.parent().filter(|p| !p.as_os_str().is_empty()) {
        std::fs::create_dir_all(parent)?;
    }
    let mut gzip = Command::new("gzip")
        .args(["-9", "-c"])
        .stdin(Stdio::piped())
        .stdout(File::create(output).with_context(|| format!("creating {}", output.display()))?)
        .spawn()
        .context("running gzip")?;
    gzip.stdin
        .take()
        .context("gzip stdin")?
        .write_all(&blob)
        .context("feeding gzip")?;
    anyhow::ensure!(gzip.wait()?.success(), "gzip failed");
    println!(
        "{} files, {} MB raw, {} MB gzipped -> {}",
        entries.len(),
        blob.len() / 1_000_000,
        std::fs::metadata(output)?.len() / 1_000_000,
        output.display()
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::read_by_port;

    #[test]
    fn the_port_reads_registries_layouts_and_colliders() {
        for path in [
            "assets/items/carry.ron",
            "assets/icons/catalogue.json",
            "assets/characters/combat/melee.ron",
            "generated/flora/library.ron",
            "generated/settlements/port_serein.ron",
            "generated/settlements/port_serein/core-0-0.collision",
            "generated/sites/veyra/hamlet.ron",
            "generated/sites/veyra/shed.collision",
            "generated/crossings/veyra/kit.ron",
            "generated/buildings/outpost.collision",
            "generated/crossings/veyra/bridge-foot-04.collision",
            "generated/weapons/veyra/models.ron",
            "generated/weapons/veyra/pictures.ron",
            "generated/vehicles/veyra/models.ron",
            "generated/props/veyra/models.ron",
            "generated/terrain/arrays.ron",
        ] {
            assert!(read_by_port(path), "{path}");
        }
    }

    #[test]
    fn what_no_reader_asks_for_stays_out() {
        for path in [
            "assets/audio/combat/levelling.json",
            "assets/fonts/manrope/OFL.txt",
            "assets/themes/glass.theme.ron",
            "assets/characters/combat/frontier_ranger.bundle.json",
            "assets/shaders/terrain.wgsl",
            "generated/materials/graphs.ron",
            "generated/materials/library.materials.ron",
            "generated/stats/buildings.ron",
            "generated/settlements/port_serein.skyline.ashlar",
            "generated/settlements/port_serein/core-0-0.ashlar",
            "generated/sites/veyra/shed.ashlar",
            "generated/buildings/outpost.ashlar",
            "generated/crossings/veyra/bridge-foot-04.ashlar",
            "generated/weapons/veyra/graphs.ron",
            "generated/weapons/veyra/library.materials.ron",
            "generated/vehicles/veyra/graphs.ron",
            "generated/props/veyra/terminal.ashprop",
            "generated/flora/oak-0.ashprop",
            "generated/capacity/a.ron",
            "generated/hydrology/water.bin",
            "generated/terrain/colour.ktx2",
        ] {
            assert!(!read_by_port(path), "{path}");
        }
    }
}
