//! The first hour holds its bands on the shipped data and the shipped world.
//! A failure here is a dial (a survey or harvest figure, an XP rate, a
//! price, a session) that moved a milestone past its minute in
//! `assets/balance/first_hour.ron`; `docs/balance/first-hour.md` says what
//! each band means and what may be moved to close one.

use std::path::Path;

use balance::field::{FirstHour, Shipped, Variant, harvest_numbers, judge, run, table};

#[test]
fn every_band_of_the_first_hour_holds() {
    let assets = Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/../../assets"));
    let registries = strata_registry::load_from_dir(assets).expect("the shipped assets load");
    let file = FirstHour::load(assets, &registries).expect("the shipped file loads");
    let shipped = Shipped::open(assets, file.seed).expect("the shipped world opens");
    let line = run(
        &shipped.registries,
        &shipped.field,
        &file,
        shipped.home,
        shipped.foot,
        &Variant::default(),
    );
    let harvest = harvest_numbers(&shipped.registries, &file, "Ilse Marrow").expect("a planter");
    let report = table(&line);
    println!("{report}");
    let (failed, waived) = judge(&file, &line, Some(harvest));
    for why in &waived {
        println!("waived: {why}");
    }
    assert!(failed.is_empty(), "{}\n{report}", failed.join("\n"));
}
