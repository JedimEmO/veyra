//! The feel contracts hold on the shipped tables. A failure here is a
//! table change (or a sim or physics change) that moved a number out of
//! its band in `assets/combat/contracts.ron`; the band's rationale says
//! what the number means, and `docs/balance/README.md` what to do.

use balance::contracts::{Contracts, table};
use strata_sim::combat_tables::CombatTables;

#[test]
fn every_shipped_band_holds_on_the_shipped_tables() {
    let assets = concat!(env!("CARGO_MANIFEST_DIR"), "/../../assets/combat");
    let contracts = Contracts::load(assets).expect("the shipped contracts load");
    assert_eq!(
        contracts,
        Contracts::shipped(),
        "the compiled-in copy is the file"
    );
    // The tables on disk are what the game and the server load; nothing
    // in this binary calls `install`, so the check below runs on the
    // compiled-in copy, which this asserts is the same thing.
    assert_eq!(
        CombatTables::load(assets).expect("the shipped tables load"),
        CombatTables::shipped()
    );
    // A debug build of the whole stack per bout; fewer at once than the
    // release binary runs, since the workspace suite is already busy.
    let jobs = std::thread::available_parallelism().map_or(1, |n| n.get().min(8));
    let verdicts = contracts.check(jobs).expect("every band names a preset");
    let report = table(&verdicts);
    println!("{report}");
    let failed: Vec<&str> = verdicts
        .iter()
        .filter(|v| v.fails())
        .map(|v| v.band.name.as_str())
        .collect();
    assert!(
        failed.is_empty(),
        "chosen bands out of their range: {failed:?}\n{report}"
    );
}
