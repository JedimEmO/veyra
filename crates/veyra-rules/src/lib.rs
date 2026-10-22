//! Veyra's rules as plain functions and state types, with no schedule in them.
//!
//! This is what `veyra-sim` used to hold under its Bevy adapters: combat and
//! the opponents' AI, abilities, progression, items and the stock, fitting,
//! wear and rework, crafting, blueprints and the factory, surveying, the
//! harvesters, the ride, the factions' deeds, encounters, loot, postings,
//! procurement, quests and the chart. Each module keeps its rules, its state
//! types (`Component`, `Resource` and `Message` derives from `bevy_ecs`) and
//! the checks a screen asks; the request types, the `consume` systems, the
//! plugins and the system parameters stay in `veyra-sim`, which re-exports
//! every module at its old path.
//!
//! # Who links it
//!
//! `veyra-sim` (the adapters) and, through it, everything above; the balance
//! harness links it directly for what needs no world. It links `bevy_ecs` and
//! `bevy_math` and never `bevy_app`, so it compiles beside Bevy's own crates
//! and a rule edit rebuilds nothing under the render layer.
//!
//! # Layers
//!
//! The modules are layered, lowest first, and a `crate::` path never points
//! up; `tests/layers.rs` holds the table:
//!
//! 1. state: `household`, `standing`, `factions`, `deeds`
//! 2. combat and progression: `combat`, `combat_loadout`, `combat_tables`,
//!    `combat_tiers`, `combat_ai`, `combat_curves`, `ability`, `progression`
//! 3. gear: `items`, `fitting`, `wear`, `equip`
//! 4. crafting: `crafting`, `blueprint`, `factory`, `rework`
//! 5. land: `deposits`, `harvest`, `ride`
//! 6. life: `encounters`, `loot`, `postings`, `procurement`, `quests`, `fall`,
//!    `chart`

pub mod ability;
pub mod blueprint;
pub mod chart;
pub mod combat;
pub mod combat_ai;
pub mod combat_curves;
pub mod combat_loadout;
pub mod combat_tables;
pub mod combat_tiers;
pub mod crafting;
pub mod deeds;
pub mod deposits;
pub mod encounters;
pub mod equip;
pub mod factions;
pub mod factory;
pub mod fall;
pub mod fitting;
pub mod harvest;
pub mod household;
pub mod items;
pub mod loot;
pub mod postings;
pub mod procurement;
pub mod progression;
pub mod quests;
pub mod rework;
pub mod ride;
pub mod standing;
pub mod wear;
