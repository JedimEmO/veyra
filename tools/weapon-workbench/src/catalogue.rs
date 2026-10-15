//! Reviewable data is emitted from the same catalogue that drives the fitting UI.
use anyhow::Result;
use serde::Serialize;
use std::path::Path;
use veyra_weapons::{
    MODULES, Maker, Module, Slot, Weapon,
    presets::{PRESETS, Preset},
};

#[derive(Serialize)]
struct Catalogue {
    schema_version: u32,
    status: &'static str,
    manufacturers: Vec<Manufacturer>,
    weapons: Vec<Chassis>,
    modules: &'static [Module],
    presets: &'static [Preset],
}
#[derive(Serialize)]
struct Manufacturer {
    id: Maker,
    name: &'static str,
    design: &'static str,
}
#[derive(Serialize)]
struct Chassis {
    id: &'static str,
    name: &'static str,
    maker: Maker,
    nominal_length_m: f64,
    slots: &'static [Slot],
    role: &'static str,
}

pub(crate) fn write(path: &Path) -> Result<()> {
    let catalogue = Catalogue {
        schema_version: 1,
        status: "Procedural authoring catalogue. Module factors are design proposals, not measured combat balance.",
        manufacturers: Maker::ALL
            .into_iter()
            .map(|maker| Manufacturer {
                id: maker,
                name: maker.label(),
                design: maker.description(),
            })
            .collect(),
        weapons: Weapon::ALL
            .into_iter()
            .map(|weapon| Chassis {
                id: weapon.key(),
                name: weapon.label(),
                maker: weapon.maker(),
                nominal_length_m: weapon.length(),
                slots: weapon.slots(),
                role: weapon.description(),
            })
            .collect(),
        modules: &MODULES,
        presets: &PRESETS,
    };
    if let Some(parent) = path.parent().filter(|p| !p.as_os_str().is_empty()) {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(
        path,
        ron::ser::to_string_pretty(&catalogue, ron::ser::PrettyConfig::default())?,
    )?;
    println!(
        "wrote {} chassis, {} components and {} presets to {}",
        Weapon::ALL.len(),
        MODULES.len(),
        PRESETS.len(),
        path.display()
    );
    Ok(())
}
