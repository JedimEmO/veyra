//! The courier bike (ticket 0009, step 2) as the content step bakes it: one
//! prop of the recipe's meshes in the plain issued materials, and the index
//! the client reads.

use strata_registry::vehicle::MODELS_PATH;

#[test]
fn the_bike_bakes_to_one_prop_in_materials_its_library_defines() {
    let (props, index) = content::vehicle_models().expect("the bike builds");
    assert_eq!(props.len(), 1);
    let (file, prop) = &props[0];
    let id = "veyra:vehicle.courier_bike".parse().expect("an id");
    assert_eq!(index.file(&id), Some(file.as_str()));
    assert!(file.starts_with(MODELS_PATH.rsplit_once('/').expect("a directory").0));
    let library = content::vehicle_definitions();
    let level = &prop.levels[0];
    assert!(level.pieces.len() >= 5, "{} pieces", level.pieces.len());
    for piece in &level.pieces {
        assert!(
            library.materials.contains_key(&piece.material),
            "{} is not defined",
            piece.material
        );
    }
    assert!(prop.triangles()[0] > 100);
}

#[test]
fn the_bike_has_a_picture_that_is_wider_than_tall_and_an_icon_the_index_names() {
    let (files, [wide, high]) = content::vehicle_icons().expect("the bike draws");
    assert!(wide > 0 && high > 0, "an empty picture");
    assert!(
        wide > high,
        "{wide} by {high}: a bike seen from the side is long"
    );
    let (_, index) = content::vehicle_models().expect("the bike builds");
    let id = "veyra:vehicle.courier_bike".parse().expect("an id");
    let named = index.icon(&id).expect("the index names the icon");
    let (path, data) = &files[0];
    assert_eq!(named, path);
    assert!(data.len() > 100, "{} bytes", data.len());
}

/// Writes the bike's prop, materials and index into `generated/` alone, as
/// `just content` would: for looking at the bike without baking the rest.
#[test]
#[ignore = "writes generated/: by hand, while iterating on the bike"]
fn rebake_vehicles() {
    let root = concat!(env!("CARGO_MANIFEST_DIR"), "/../../generated");
    let content = ashlar_content::Content::new(root);
    let (props, index) = content::vehicle_models().expect("the bike builds");
    for (path, prop) in &props {
        content.write_prop(path, prop).expect("written");
    }
    content
        .write_materials(
            &ashlar_material::MaterialGraphLibrary::default(),
            &content::vehicle_definitions(),
            content::VEHICLE_MATERIALS,
        )
        .expect("written");
    for (path, data) in &content::vehicle_icons().expect("the bike draws").0 {
        content.write_file(path, data).expect("written");
    }
    content
        .write_file(MODELS_PATH, index.to_ron().as_bytes())
        .expect("written");
}
