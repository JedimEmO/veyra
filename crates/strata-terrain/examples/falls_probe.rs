//! The falls near a point and the ground along each, for choosing what to
//! look at and for checking a fall's drawing against its ground:
//! `cargo run --release -p strata-terrain --example falls_probe -- [x z radius_m seed]`.
//! The falls are listed nearest first, or tallest first with `BY_DROP` set;
//! `FALL_PROFILE=n` also prints the ground and the water along the n-th
//! listed, from 40 m above its lip to 60 m past its foot. Ticket 0005's
//! step 8 chose its two falls with it.

use strata_core::Seed;
use strata_worldgen::{ProceduralWorld, WorldSource};

fn main() {
    let mut args = std::env::args().skip(1);
    let mut num = |default: f64| args.next().and_then(|a| a.parse().ok()).unwrap_or(default);
    let (x, z, radius) = (num(-700.0), num(-1_200.0), num(30_000.0));
    #[allow(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "a seed"
    )]
    let seed = num(42.0) as u64;
    let registries =
        strata_registry::load_from_dir(concat!(env!("CARGO_MANIFEST_DIR"), "/../../assets"))
            .expect("the shipped assets load");
    let world = ProceduralWorld::new(Seed(seed), &registries);
    let hydrology = world.hydrology();
    hydrology.set_threads(8);
    let networks = hydrology.bake_area(&world, (x - radius, z - radius), (x + radius, z + radius));
    let mut falls = Vec::new();
    for network in &networks {
        for fall in network.falls() {
            let d = ((fall.top[0] - x).powi(2) + (fall.top[2] - z).powi(2)).sqrt();
            if d <= radius {
                falls.push((d, *fall));
            }
        }
    }
    falls.sort_by(|a, b| a.0.total_cmp(&b.0));
    println!("{} falls within {radius} m of ({x}, {z})", falls.len());
    let mut slopes: Vec<f64> = falls
        .iter()
        .map(|(_, f)| {
            let run = ((f.bottom[0] - f.top[0]).powi(2) + (f.bottom[2] - f.top[2]).powi(2)).sqrt();
            (f.top[1] - f.bottom[1]) / run.max(1e-3)
        })
        .collect();
    slopes.sort_by(f64::total_cmp);
    let mut widths: Vec<f32> = falls.iter().map(|(_, f)| f.width_m).collect();
    widths.sort_by(f32::total_cmp);
    if !slopes.is_empty() {
        let q = |v: usize| v * (slopes.len() - 1) / 10;
        println!(
            "slope deciles {:?}",
            (0..=10)
                .map(|k| (slopes[q(k)] * 100.0).round() / 100.0)
                .collect::<Vec<_>>()
        );
        println!(
            "width deciles {:?}",
            (0..=10).map(|k| widths[q(k)]).collect::<Vec<_>>()
        );
    }
    if std::env::var("BY_DROP").is_ok() {
        falls.sort_by(|a, b| (b.1.top[1] - b.1.bottom[1]).total_cmp(&(a.1.top[1] - a.1.bottom[1])));
    }
    for (d, fall) in falls.iter().take(60) {
        let drop = fall.top[1] - fall.bottom[1];
        let run = ((fall.bottom[0] - fall.top[0]).powi(2) + (fall.bottom[2] - fall.top[2]).powi(2))
            .sqrt();
        println!(
            "{d:7.0} m  lip ({:.0}, {:.1}, {:.0}) foot ({:.0}, {:.1}, {:.0}) drop {drop:.1} run {run:.1} slope {:.2} width {:.1}",
            fall.top[0],
            fall.top[1],
            fall.top[2],
            fall.bottom[0],
            fall.bottom[1],
            fall.bottom[2],
            drop / run.max(1e-3),
            fall.width_m
        );
    }
    if let Some(pick) = std::env::var("FALL_PROFILE")
        .ok()
        .and_then(|s| s.parse::<usize>().ok())
    {
        let (_, fall) = falls[pick];
        let dir = [f64::from(fall.direction[0]), f64::from(fall.direction[1])];
        let run = ((fall.bottom[0] - fall.top[0]).powi(2) + (fall.bottom[2] - fall.top[2]).powi(2))
            .sqrt();
        println!("profile of fall {pick}, along its direction from 40 m above the lip:");
        let mut s = -40.0;
        while s <= run + 60.0 {
            let (px, pz) = (fall.top[0] + dir[0] * s, fall.top[2] + dir[1] * s);
            let sample = world.sample(px, pz);
            let water = sample
                .water
                .map(|w| (w.surface_m, w.bed_m, w.width_m, w.kind, w.signed_distance_m));
            println!("  {s:6.1}  ground {:8.2}  water {water:?}", sample.height);
            s += 4.0;
        }
    }
}
