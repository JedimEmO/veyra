//! Prints the mean and spread of every baked base map under `generated/`, the
//! numbers `assets/look/values.ron` is filled from.
//!
//! `cargo run -p content --example value_table --release`

use std::path::Path;

fn main() {
    let root = Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/../../generated"));
    for (key, file) in content::values::baked_maps(root) {
        match content::values::stats_of_ktx2(&file) {
            Ok(s) => println!("{key:40} mean {:.3} spread {:.3}", s.mean, s.spread),
            Err(e) => println!("{key:40} {e}"),
        }
    }
    let glb = Path::new(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../assets/characters/combat/frontier_ranger.glb"
    ));
    if let Ok(s) = content::values::stats_of_glb(glb) {
        println!(
            "{:40} mean {:.3} spread {:.3}",
            "characters/frontier_ranger", s.mean, s.spread
        );
    }
}
