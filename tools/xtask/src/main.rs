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
        }) => {
            let root =
                root.unwrap_or_else(|| PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../.."));
            bundle(&root, &output, raw.as_deref(), seed)?;
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

/// File extensions the files port reads, per logical root.
const BUNDLED: [(&str, &[&str]); 2] = [
    ("assets", &["ron", "json", "txt"]),
    ("generated", &["ron", "json", "ashlar"]),
];

/// Directories under `generated/` that only the prop-capacity bench reads.
const BENCH_ONLY: [&str; 2] = ["generated/capacity", "generated/materials/capacity"];

fn collect(
    root: &std::path::Path,
    logical: &str,
    extensions: &[&str],
    out: &mut Vec<(String, Vec<u8>)>,
) -> Result<()> {
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
            if !BENCH_ONLY.contains(&path.as_str()) {
                collect(root, &path, extensions, out)?;
            }
        } else if kind.is_file()
            && std::path::Path::new(name)
                .extension()
                .and_then(|e| e.to_str())
                .is_some_and(|e| extensions.contains(&e))
        {
            let bytes = std::fs::read(entry.path())
                .with_context(|| format!("reading {}", entry.path().display()))?;
            out.push((path, bytes));
        }
    }
    Ok(())
}

/// Packs the bundle and gzips it with the `gzip` program, which keeps a
/// codec crate out of the workspace.
fn bundle(
    root: &std::path::Path,
    output: &PathBuf,
    raw: Option<&std::path::Path>,
    seed: u64,
) -> Result<()> {
    use std::io::Write;
    use std::process::{Command, Stdio};

    let mut entries = Vec::new();
    for (logical, extensions) in BUNDLED {
        collect(root, logical, extensions, &mut entries)?;
    }
    // The bake a browser could not afford, done here on every thread (ticket
    // 0015, step 3): the spawn's rivers and the settlements', as the game
    // would bake them before it starts.
    let started = std::time::Instant::now();
    let (water, masses) = strata_sim::bake_water_file(
        &strata_registry::files::Files::dir(root),
        strata_core::Seed(seed),
        &strata_sim::PrepareWater {
            places: vec![(0.0, 0.0)],
            radius_m: strata_sim::PREPARE_WATER_RADIUS_M,
        },
    )
    .context("baking the rivers")?;
    println!(
        "rivers: {masses} masses, {} MB, baked in {:.1} s",
        water.len() / 1_000_000,
        started.elapsed().as_secs_f64()
    );
    entries.push((strata_sim::WATER_FILE.to_owned(), water));
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
