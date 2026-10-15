//! How much of a river valley's walls and a road's banks bares rock, by the
//! terrain's own rule (ticket 0005, step 6): an ignored probe, run in
//! release by hand:
//!
//! ```text
//! cargo test --release -p strata-terrain --test made_ground -- --ignored --nocapture
//! ```

use strata_core::Seed;
use strata_terrain::mesh::{BiomeLayers, made, made_rock};
use strata_worldgen::hydrology::WaterKind;
use strata_worldgen::{ProceduralWorld, Sample, WorldSource};

/// Whether a sample's slope is past the rise at which its rock bares: the
/// dominant biome's threshold times its province's `bare_slope`, the middle
/// of the shader's band.
fn steep_enough(sample: &Sample, layers: &BiomeLayers) -> bool {
    let rise = layers.slope_rise(sample.dominant_biome())
        * layers
            .province(sample.dominant_province())
            .map_or(1.0, |p| p.bare_slope);
    sample.landform.slope > rise
}

/// Over the 25 km round Port Serein every 23 m: of the ground a river's
/// valley cut into a wall (cut over a metre, steeper than a quarter) and of
/// the ground a road cut or filled by over half a metre, the share steep
/// enough to bare its rock, and the share that bares it now that made
/// ground is soil but where deep and steep.
#[test]
#[ignore = "a probe: release, by hand"]
#[allow(clippy::cast_precision_loss, reason = "a probe's counts")]
fn probe_made_ground() {
    let registries =
        strata_registry::load_from_dir(concat!(env!("CARGO_MANIFEST_DIR"), "/../../assets"))
            .expect("the shipped assets load");
    let world = ProceduralWorld::new(Seed(42), &registries);
    world.hydrology().set_threads(16);
    let city = world.sites()[0].centre;
    world.prepare_water(city[0], city[1], 27_000.0);
    let layers = BiomeLayers::from_registries(&registries);
    // (points, steep enough, baring now)
    let mut walls = (0_usize, 0_usize, 0.0_f64);
    let mut banks = (0_usize, 0_usize, 0.0_f64);
    let mut z = city[1] - 25_000.0;
    while z < city[1] + 25_000.0 {
        let mut x = city[0] - 25_000.0;
        while x < city[0] + 25_000.0 {
            let s = world.sample(x, z);
            let carved = s
                .water
                .filter(|w| w.kind != WaterKind::Lake)
                .map_or(0.0, |w| w.cut_m());
            let wall = carved > 1.0 && s.landform.slope > 0.25;
            let bank = s.landform.cut_m.abs() > 0.5 && s.landform.developed < 0.5;
            if wall || bank {
                let steep = steep_enough(&s, &layers);
                let bares = if steep {
                    f64::from(made_rock(&s, made(&s), x, z))
                } else {
                    0.0
                };
                let tally = if wall { &mut walls } else { &mut banks };
                tally.0 += 1;
                tally.1 += usize::from(steep);
                tally.2 += bares;
            }
            x += 23.0;
        }
        z += 23.0;
    }
    for (name, (n, steep, bares)) in [("valley walls", walls), ("road banks", banks)] {
        println!(
            "{name}: {n} points; steep enough to bare rock {:.1} % (all rock before), rock now {:.1} %, soil {:.1} %",
            100.0 * steep as f64 / n.max(1) as f64,
            100.0 * bares / n.max(1) as f64,
            100.0 - 100.0 * bares / n.max(1) as f64
        );
    }
}
