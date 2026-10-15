//! The worth page keeps its shape: every weapon, every ability of
//! `abilities.ron` and every opponent kind has its row, and the five
//! sections are there. The figures are the contracts' business, so the bouts
//! here are few and short; the shipped page is held to the same rows.

use std::path::Path;

use balance::worth::page;
use strata_sim::combat::Weapon;
use strata_sim::combat_ai::OpponentKind;
use strata_sim::combat_tables::CombatTables;

const SECTIONS: [&str; 5] = [
    "## Guns by range",
    "## Melee",
    "## Abilities",
    "## Opponents",
    "## What changed in this tuning",
];

fn holds_its_shape(text: &str, what: &str) {
    for section in SECTIONS {
        assert!(text.contains(section), "{what}: no section `{section}`");
    }
    for weapon in Weapon::ALL {
        assert!(
            text.contains(&format!("| {} |", weapon.name())),
            "{what}: no row for {}",
            weapon.name()
        );
    }
    // Each table by itself: the weapon names recur in the abilities table, so
    // a bare weapon row would pass with the others gone.
    let section = |name: &str| -> &str {
        let from = text.find(name).expect("section present");
        let rest = &text[from + name.len()..];
        &rest[..rest.find("\n## ").unwrap_or(rest.len())]
    };
    for gun in ["Pistol", "Carbine", "Rifle"] {
        for range in [10, 30, 60] {
            assert!(
                section("## Guns by range").contains(&format!("| {gun} | {range} m |")),
                "{what}: no {gun} row at {range} m"
            );
        }
    }
    for class in ["Blade", "Greatblade", "Glaive"] {
        assert!(
            section("## Melee").contains(&format!("| {class} |")),
            "{what}: no melee row for {class}"
        );
    }
    for kind in OpponentKind::ALL {
        for weapon in Weapon::ALL {
            assert!(
                section("## Opponents").contains(&format!(
                    "| {} | {} |",
                    kind.name(),
                    weapon.name()
                )),
                "{what}: no {} row against {}",
                weapon.name(),
                kind.name()
            );
        }
    }
    for row in &CombatTables::shipped().abilities.abilities {
        assert!(
            text.contains(&format!("| {} |", row.id)),
            "{what}: no row for ability {}",
            row.id
        );
    }
    for kind in OpponentKind::ALL {
        assert!(
            text.contains(&format!("| {} |", kind.name())),
            "{what}: no row for {}",
            kind.name()
        );
        assert!(
            text.contains(&format!("the {} ", kind.name())),
            "{what}: no sentence for {}",
            kind.name()
        );
    }
}

#[test]
fn the_page_has_a_row_for_every_weapon_ability_and_opponent() {
    let assets = Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/../../assets"));
    strata_sim::combat_tables::install(CombatTables::shipped());
    let jobs = std::thread::available_parallelism().map_or(1, |n| n.get().min(8));
    let made = page(assets, 1, 1, 4., jobs).expect("the page is made from the shipped data");
    holds_its_shape(&made, "the made page");
    let shipped = std::fs::read_to_string(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../docs/balance/combat-worth.md"
    ))
    .expect("docs/balance/combat-worth.md is shipped; `just balance worth --write` makes it");
    holds_its_shape(&shipped, "the shipped page");
}
