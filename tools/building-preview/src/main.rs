//! The ashlar preview and reference gallery over the Veyra building content.
//!
//! Everything specific to this game is the table below and the asset root; the
//! viewer, the gallery and the command line are `ashlar-preview`'s.
use anyhow::Result;
use ashlar_preview::{Catalog, Options, Scene, run};
use veyra_buildings as study;
use veyra_settlements::types::house::review as houses;

const SCENES: [(&str, study::Scene, &str); 8] = [
    ("basic", study::Scene::Basic, "Primitive fixture"),
    ("facade", study::Scene::Facade, "Facade bay"),
    ("entrance", study::Scene::Entrance, "Service entrance"),
    ("outpost", study::Scene::Outpost, "Frontier outpost"),
    ("office", study::Scene::Office, "Three-storey office"),
    ("corporate", study::Scene::Corporate, "Corporate tower"),
    (
        "corporate-annex",
        study::Scene::CorporateAnnex,
        "Corporate annex",
    ),
    (
        "corporate-block",
        study::Scene::CorporateBlock,
        "Corporate courtyard",
    ),
];

fn catalog() -> Catalog {
    let mut catalog = Catalog::new();
    for (name, scene, display) in SCENES {
        let mut entry = Scene::new(name, move || Ok(study::building(scene)?)).display_name(display);
        if scene.dressed() {
            // The preview bakes the graphs itself, so an edit to a material
            // shows here without a content step.
            entry = entry
                .materials(veyra_materials::buildings)
                .graphs(veyra_materials::graphs);
        }
        catalog = catalog.scene(entry);
    }
    // Port Serein's house catalogue: every part, a street of composed
    // houses, and a few of them alone, for the same gallery angles as the
    // corporate kit they are held to.
    let city: [(&str, fn() -> anyhow::Result<ashlar::Building>, &str); 6] = [
        (
            "serein-catalogue",
            houses::catalogue,
            "Terrace house catalogue",
        ),
        ("serein-street", houses::street, "Terrace street"),
        (
            "serein-house-a",
            || houses::house(0),
            "Terrace house, set back",
        ),
        (
            "serein-house-c",
            || houses::house(2),
            "Terrace house, casement",
        ),
        (
            "serein-house-d",
            || houses::house(3),
            "Terrace house, vaulted",
        ),
        ("serein-workshop", || houses::house(5), "Terrace workshop"),
    ];
    for (name, build, display) in city {
        catalog = catalog.scene(
            Scene::new(name, build)
                .display_name(display)
                .materials(city_materials)
                .graphs(veyra_materials::graphs),
        );
    }
    catalog.default_scene("outpost")
}

/// Veyra's building materials and Port Serein's own, which the houses bind.
fn city_materials() -> ashlar::MaterialLibrary {
    let mut library = veyra_materials::buildings();
    library
        .materials
        .extend(veyra_settlements::materials().materials);
    library
}

fn main() -> Result<()> {
    let catalog = catalog();
    let options = Options::parse_for(&catalog);
    run(&catalog, options)
}
