//! GPU-free review: `cargo run -p content --example plan -- docs/design/city/plan-after`.
//! Writes the plan and ground as TSV and invokes the neighbouring matplotlib script.
use std::fmt::Write;
use std::path::Path;

use anyhow::{Context, Result};
use veyra_settlements::blockout::{Ground, diagram};

fn main() -> Result<()> {
    let output = std::env::args()
        .nth(1)
        .context("give an output stem, without .png")?;
    let assets = Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/../../assets"));
    let registries = strata_registry::load_from_dir(assets)?;
    let def = registries
        .settlements
        .iter()
        .find(|s| s.id.as_ref() == veyra_settlements::PORT_SEREIN)
        .context("Port Serein")?;
    let site = strata_worldgen::Site::place(def, &|_, _| 0.0);
    let platform = |id: &str| {
        let p = site
            .platforms
            .iter()
            .find(|p| p.id == id)
            .expect("a shipped platform");
        (p.at, p.half_size, p.height)
    };
    let height = |x, z| site.city_ground([x, z]);
    let owns = |x, z| site.owns([x, z]);
    let ground = Ground {
        height: &height,
        owns: &owns,
        starport: platform("starport"),
        exchange: platform("exchange"),
        processor: platform("processor"),
        headquarters: platform("headquarters"),
        quay: def.terraces.quay_m,
        road_exit: def.road.exit,
    };
    std::fs::write(format!("{output}.tsv"), diagram(&ground)?)?;
    let mut samples = String::new();
    for z in (-500..=280).step_by(2) {
        for x in (-360..=360).step_by(2) {
            writeln!(
                samples,
                "{x}\t{z}\t{:.5}",
                height(f64::from(x), f64::from(z))
            )?;
        }
    }
    std::fs::write(format!("{output}-ground.tsv"), samples)?;
    let result = std::process::Command::new("python3")
        .arg(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/examples/plot_plan.py"
        ))
        .arg(&output)
        .env("MPLCONFIGDIR", "/tmp/serein-matplotlib")
        .status()?;
    anyhow::ensure!(result.success(), "matplotlib failed");
    println!(
        "wrote {output}.png (ground fingerprint {:016x})",
        site.fingerprint()
    );
    Ok(())
}
