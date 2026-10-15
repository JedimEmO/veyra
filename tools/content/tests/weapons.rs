//! The weapon pictures (ticket 0008, step 2) as the content step bakes
//! them: a file and an index entry for the chassis, every mount's standard
//! part and every module that fits it, and the stock weapon whole, each
//! under a name of its own, with an index that reads back as written, and
//! a square icon beside each (step 5). The item files that name those
//! pictures are held to the catalogue the pictures are baked from.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::LazyLock;

use strata_registry::item::ItemKind;
use strata_registry::pictures::{ChassisPictures, PICTURES_PATH, PictureSet};
use veyra_weapons::{MODULES, Maker, Weapon, modules};

/// One bake for every test here: the kernel and the rasteriser are the
/// slow part of this file, and a second bake would only prove determinism,
/// which the rasteriser's own tests hold.
static BAKE: LazyLock<(Vec<(String, Vec<u8>)>, PictureSet)> =
    LazyLock::new(|| content::weapons().expect("the weapons bake"));

/// The size a PNG file states in its header, without decoding it: the
/// eight-byte signature, the `IHDR` chunk's length and name, then the width
/// and the height.
fn png_size(data: &[u8]) -> [u32; 2] {
    assert_eq!(&data[..8], b"\x89PNG\r\n\x1a\n", "a PNG signature");
    let word = |at: usize| u32::from_be_bytes(data[at..at + 4].try_into().expect("four bytes"));
    [word(16), word(20)]
}

/// The icon of the picture `file`, which is filed beside it under the
/// picture's own name.
fn icon_beside<'a>(file: &str, icon: Option<&'a str>) -> &'a str {
    let icon = icon.unwrap_or_else(|| panic!("{file} has no icon"));
    let (dir, stem) = file.rsplit_once('/').expect("under its chassis");
    assert_eq!(icon, format!("{dir}/icon.{stem}"), "{file}");
    icon
}

/// One chassis of the index against the catalogue and the baked files:
/// the pieces the catalogue promises, in the order the index is documented
/// to hold them, each under the ticket's file name and inside the frame.
/// Every file it names is added to `named`, which refuses a second use.
fn check_chassis<'a>(
    weapon: Weapon,
    chassis: &'a ChassisPictures,
    baked: &BTreeMap<&str, &[u8]>,
    named: &mut BTreeSet<&'a str>,
) {
    let key = weapon.key();
    let dir = PICTURES_PATH.rsplit_once('/').expect("a directory").0;
    assert_eq!(
        chassis.width.max(chassis.height),
        content::WEAPON_LONG_SIDE,
        "{key}"
    );
    assert!(chassis.metres_per_pixel > 0.0, "{key}");

    let mut expected = vec![(None, None, format!("{dir}/{key}/chassis.png"))];
    for slot in weapon.slots() {
        let mount = slot.key();
        expected.push((
            Some(mount),
            None,
            format!("{dir}/{key}/{mount}.standard.png"),
        ));
        assert!(
            chassis.piece(mount, None).is_some(),
            "{key}: {mount} has no standard picture"
        );
        for module in modules(weapon, *slot) {
            let id = module.id;
            expected.push((
                Some(mount),
                Some(id),
                format!("{dir}/{key}/{mount}.{id}.png"),
            ));
            assert!(
                chassis.piece(mount, Some(id)).is_some(),
                "{key}: {mount} has no picture of {id}"
            );
        }
    }
    assert!(chassis.body().is_some(), "{key} has no chassis picture");
    let listed: Vec<_> = chassis
        .pieces
        .iter()
        .map(|piece| {
            (
                piece.mount.as_deref(),
                piece.module.as_deref(),
                piece.file.clone(),
            )
        })
        .collect();
    assert_eq!(listed, expected, "{key}");
    assert_eq!(
        chassis.whole.file,
        format!("{dir}/{key}/whole.png"),
        "the weapon's icon"
    );

    let placed = chassis
        .pieces
        .iter()
        .map(|piece| (&piece.file, piece.rect, &piece.icon))
        .chain([(&chassis.whole.file, chassis.whole.rect, &chassis.whole.icon)]);
    for (file, rect, icon) in placed {
        assert!(named.insert(file.as_str()), "{file} is named twice");
        assert!(
            named.insert(icon_beside(file, icon.as_deref())),
            "{file}: its icon is named twice"
        );
        let data = baked
            .get(file.as_str())
            .unwrap_or_else(|| panic!("{file} is named and not baked"));
        let [x, y, width, height] = rect;
        assert_eq!(png_size(data), [width, height], "{file}");
        assert!(width > 0 && height > 0, "{file}");
        assert!(
            x + width <= chassis.width && y + height <= chassis.height,
            "{file} leaves its frame"
        );
        // Grey with alpha, eight bits: colour type 4 at bit depth 8.
        assert_eq!(&data[24..26], &[8, 4], "{file}");
    }
    check_stack(weapon, chassis);
}

/// The marks of one chassis: a picture said to be another's again names an
/// earlier piece of its own mount, and the stack is the body and the
/// standard parts less the ones the chassis hides.
fn check_stack(weapon: Weapon, chassis: &ChassisPictures) {
    let key = weapon.key();
    for (index, piece) in chassis.pieces.iter().enumerate() {
        let Some(like) = piece.same_as.as_deref() else {
            continue;
        };
        let earlier = chassis.pieces[..index]
            .iter()
            .find(|other| other.file == like)
            .unwrap_or_else(|| panic!("{}: {like} is not an earlier piece", piece.file));
        assert_eq!(earlier.mount, piece.mount, "{}", piece.file);
        assert_eq!(earlier.same_as, None, "{}", piece.file);
    }
    let body = chassis.body().expect("a chassis picture");
    assert!(!body.hidden && body.same_as.is_none(), "{key}");
    assert!(
        chassis.stock().next().is_some_and(|first| first == body),
        "{key}: the stack starts with the body"
    );
    assert_eq!(
        chassis.stock().count(),
        1 + weapon
            .slots()
            .iter()
            .filter(|slot| !chassis
                .piece(slot.key(), None)
                .expect("checked above")
                .hidden)
            .count(),
        "{key}"
    );
}

/// The case a side-on picture cannot show, as the bake reports it (ticket
/// 0008's review): it is for the user to settle at the sheets, by the recipe
/// or by the view, and until then the index must say so. A recipe that
/// settles it makes the assertion here fail, which is when to take it out,
/// as the field blade's hooked edge was once the blade stood on its edge
/// (ticket 0010, step 3).
fn check_marks(set: &PictureSet) {
    let marked: Vec<(&str, bool, Option<&str>)> = set
        .chassis
        .values()
        .flat_map(|chassis| &chassis.pieces)
        .filter(|piece| piece.hidden || piece.same_as.is_some())
        .map(|piece| (piece.file.as_str(), piece.hidden, piece.same_as.as_deref()))
        .collect();
    assert_eq!(
        marked,
        [
            // The precision rifle's feed is on the far side of its receiver.
            (
                "weapons/veyra/precision_rifle/feed.standard.png",
                true,
                None
            ),
            (
                "weapons/veyra/precision_rifle/feed.precision_compact_feed.png",
                true,
                Some("weapons/veyra/precision_rifle/feed.standard.png")
            ),
        ]
    );
}

/// The contact sheets, which are made from the files and the index alone
/// and so are also the check that every file decodes to its rectangle's
/// size: one per maker, in ordinary colour, wide enough for a frame twice.
fn check_sheets(files: &[(String, Vec<u8>)], set: &PictureSet) {
    let sheets = content::sheet::weapon_sheets(files, set).expect("the sheets draw");
    let written: Vec<&str> = sheets.iter().map(|(name, _)| name.as_str()).collect();
    let makers: Vec<String> = Maker::ALL
        .iter()
        .map(|maker| format!("{}.png", maker.label().to_lowercase()))
        .collect();
    assert_eq!(written, makers);
    for (name, data) in &sheets {
        let [width, height] = png_size(data);
        assert!(
            width > 2 * content::WEAPON_LONG_SIDE && height > 0,
            "{name}"
        );
        // Truecolour, eight bits: colour type 2.
        assert_eq!(&data[24..26], &[8, 2], "{name}");
    }
}

#[test]
fn every_chassis_mount_and_module_has_a_picture_under_a_name_of_its_own() {
    let (files, set) = &*BAKE;
    let baked: BTreeMap<&str, &[u8]> = files
        .iter()
        .map(|(path, data)| (path.as_str(), data.as_slice()))
        .collect();
    assert_eq!(baked.len(), files.len(), "a path is baked twice");
    assert!(
        !baked.contains_key(PICTURES_PATH),
        "a picture over the index"
    );

    assert_eq!(set.chassis.len(), Weapon::ALL.len());
    let mut named = BTreeSet::new();
    for weapon in Weapon::ALL {
        let chassis = set
            .chassis
            .get(weapon.key())
            .unwrap_or_else(|| panic!("{} is not in the index", weapon.key()));
        check_chassis(weapon, chassis, &baked, &mut named);
    }
    assert_eq!(named.len(), files.len(), "a file is baked and not named");
    assert_eq!(
        set.files().collect::<BTreeSet<_>>(),
        named,
        "the index lists its own files"
    );

    let text = set.to_ron();
    assert_eq!(&PictureSet::from_ron(&text).expect("the index reads"), set);

    check_marks(set);
    check_sheets(files, set);
}

/// Every picture, the hidden parts' and the whole weapons' too, has an icon
/// a cell can draw: a square of the size the index promises, in the
/// pictures' own two channels, with the picture in its middle, as large as
/// the margin allows and never more than doubled.
#[test]
fn every_picture_has_a_square_icon() {
    let (files, set) = &*BAKE;
    let baked: BTreeMap<&str, &[u8]> = files
        .iter()
        .map(|(path, data)| (path.as_str(), data.as_slice()))
        .collect();
    let side = content::WEAPON_ICON_SIDE;
    let fill = content::WEAPON_ICON_FILL;
    let mut seen = 0;
    for (key, chassis) in &set.chassis {
        let pictures = chassis
            .pieces
            .iter()
            .map(|piece| (&piece.icon, piece.rect))
            .chain([(&chassis.whole.icon, chassis.whole.rect)]);
        for (icon, rect) in pictures {
            let icon = icon.as_deref().unwrap_or_else(|| panic!("{key}: no icon"));
            let data = baked
                .get(icon)
                .unwrap_or_else(|| panic!("{icon} is named and not baked"));
            assert_eq!(png_size(data), [side, side], "{icon}");
            assert_eq!(&data[24..26], &[8, 4], "{icon}: grey with alpha");
            let image = image::load_from_memory_with_format(data, image::ImageFormat::Png)
                .expect("a PNG")
                .into_luma_alpha8();
            // What the icon covers: the box of every pixel that is not clear.
            let covered: Vec<(u32, u32)> = image
                .enumerate_pixels()
                .filter(|(_, _, pixel)| pixel.0[1] > 0)
                .map(|(x, y, _)| (x, y))
                .collect();
            assert!(!covered.is_empty(), "{icon} is clear");
            let min = |axis: fn(&(u32, u32)) -> u32| {
                covered.iter().map(axis).min().expect("it covers a pixel")
            };
            let max = |axis: fn(&(u32, u32)) -> u32| {
                covered.iter().map(axis).max().expect("it covers a pixel")
            };
            let (left, right) = (min(|p| p.0), max(|p| p.0));
            let (top, bottom) = (min(|p| p.1), max(|p| p.1));
            let (wide, high) = (right - left + 1, bottom - top + 1);
            let long = rect[2].max(rect[3]);
            // The whole of the margin, unless that would more than double it.
            let wanted = fill.min(long * 2);
            assert!(
                wide.max(high) <= wanted && wide.max(high) + 3 >= wanted,
                "{icon}: {wide} by {high}, of a picture {} by {}",
                rect[2],
                rect[3]
            );
            // In the middle, to the pixel the centring rounds to and the
            // one a soft edge may lose.
            assert!(
                (left + right + 1).abs_diff(side) <= 3 && (top + bottom + 1).abs_diff(side) <= 3,
                "{icon} is off centre: {left}..{right}, {top}..{bottom}"
            );
            seen += 1;
        }
    }
    assert_eq!(seen * 2, files.len(), "an icon for every picture");
}

/// A picture's clear ground stays clear when it is shrunk and when it is
/// grown, and no edge takes the tone that lies under the clear pixels
/// beside it: the tone is weighted by the coverage. The lightest tone of
/// the icon is lifted to white, and a darker one by the same share, so a
/// dark part reads in a cell and keeps its shading.
#[test]
fn an_icon_keeps_the_pictures_shading_and_its_clear_ground() {
    for (width, height) in [(400_u32, 100_u32), (20, 30)] {
        // A picture in two flat tones, the upper half twice as light as
        // the lower, on a clear ground whose hidden tone is black.
        let mut pixels = vec![0_u8; (width * height * 2) as usize];
        for y in 2..height - 2 {
            for x in 2..width - 2 {
                let i = ((y * width + x) * 2) as usize;
                pixels[i] = if y < height / 2 { 100 } else { 50 };
                pixels[i + 1] = 255;
            }
        }
        let icon = content::weapon_icon(&pixels, width, height);
        let side = content::WEAPON_ICON_SIDE as usize;
        assert_eq!(icon.len(), side * side * 2);
        let covered: Vec<&[u8]> = icon.chunks(2).filter(|pixel| pixel[1] > 0).collect();
        assert!(!covered.is_empty());
        let lightest = covered.iter().map(|pixel| pixel[0]).max().expect("covered");
        let darkest = covered.iter().map(|pixel| pixel[0]).min().expect("covered");
        assert_eq!(lightest, 255, "the lightest tone is lifted to white");
        assert!(
            darkest >= 126,
            "an edge took the ground's tone, or the halves lost their share: {darkest}"
        );
        assert!(
            covered.iter().any(|pixel| pixel[0] <= 129),
            "the darker half"
        );
        assert_eq!(&icon[..2], &[0, 0], "the corner is clear");
        assert!(covered.iter().any(|pixel| pixel[1] == 255));
    }
    // A very dark picture is lifted by no more than the limit.
    let dark = [20_u8, 255].repeat(40 * 40);
    let icon = content::weapon_icon(&dark, 40, 40);
    let lightest = icon
        .chunks(2)
        .filter(|pixel| pixel[1] > 0)
        .map(|pixel| pixel[0])
        .max()
        .expect("covered");
    assert!(
        (f64::from(lightest) - 20. * content::WEAPON_ICON_LIFT).abs() < 0.5,
        "{lightest}"
    );
    // Nothing in, a clear square out.
    assert!(
        content::weapon_icon(&[], 0, 0)
            .iter()
            .all(|byte| *byte == 0)
    );
}

/// The item files a player's weapons and modules are read from say what
/// the catalogue the pictures are baked from says: a weapon's mounts are
/// its chassis's, a module's mount, weapons and factors are its part's.
/// The game links only the files, so this is where the two are held
/// together.
#[test]
#[allow(
    clippy::too_many_lines,
    reason = "one walk over every item, then the melee ones by name"
)]
fn the_item_files_carry_the_catalogues_factors_and_mounts() {
    let assets = concat!(env!("CARGO_MANIFEST_DIR"), "/../../assets");
    let registries = strata_registry::load_from_dir(assets).expect("assets/ loads");
    let items = &registries.items;
    let chassis_of = |key: &str| {
        Weapon::ALL
            .into_iter()
            .find(|weapon| weapon.key() == key)
            .unwrap_or_else(|| panic!("no chassis `{key}` in the catalogue"))
    };
    let (mut weapons, mut parts) = (0, 0);
    for def in items.iter() {
        match &def.kind {
            ItemKind::Weapon {
                chassis, mounts, ..
            } => {
                // Every weapon stands on a chassis, the three melee ones too.
                assert!(!chassis.is_empty(), "{} has no chassis", def.id);
                let weapon = chassis_of(chassis);
                let ours: BTreeSet<&str> = mounts.iter().map(|mount| mount.slot.as_str()).collect();
                let theirs: BTreeSet<&str> = weapon.slots().iter().map(|slot| slot.key()).collect();
                assert_eq!(ours, theirs, "{}", def.id);
                assert_eq!(mounts.len(), ours.len(), "{}: a mount twice", def.id);
                assert_eq!(
                    def.icon.picture,
                    Some(format!("{chassis}/whole.png")),
                    "{}",
                    def.id
                );
                weapons += 1;
            }
            ItemKind::Module {
                slot,
                fits,
                part,
                effects,
            } => {
                let module = MODULES
                    .iter()
                    .find(|module| module.id == part)
                    .unwrap_or_else(|| panic!("{}: no module `{part}` in the catalogue", def.id));
                assert_eq!(slot, module.slot.key(), "{}", def.id);
                assert_eq!(def.name, module.name, "{}", def.id);
                let ours: Vec<(String, f32)> = effects
                    .iter()
                    .map(|effect| (effect.property.key().to_owned(), effect.factor))
                    .collect();
                let theirs: Vec<(String, f32)> = module
                    .effects
                    .iter()
                    .map(|effect| (effect.label.replace(' ', "_"), effect.factor))
                    .collect();
                assert_eq!(ours, theirs, "{}", def.id);
                assert!(!fits.is_empty(), "{}", def.id);
                for weapon in fits {
                    let Some(ItemKind::Weapon { chassis, .. }) =
                        items.get(weapon).map(|def| &def.kind)
                    else {
                        panic!("{}: fits `{weapon}`, which is no weapon", def.id);
                    };
                    assert!(
                        module.weapons.contains(&chassis_of(chassis)),
                        "{}: the catalogue's `{part}` does not fit {chassis}",
                        def.id
                    );
                }
                let Some(ItemKind::Weapon { chassis, .. }) =
                    items.get(&fits[0]).map(|def| &def.kind)
                else {
                    unreachable!("checked above");
                };
                assert_eq!(
                    def.icon.picture,
                    Some(format!("{chassis}/{slot}.{part}.png")),
                    "{}",
                    def.id
                );
                parts += 1;
            }
            ItemKind::Bag { .. }
            | ItemKind::Lot { .. }
            | ItemKind::Component
            | ItemKind::Commodity
            | ItemKind::Kit
            | ItemKind::Tool { .. }
            | ItemKind::Blueprint
            | ItemKind::Vehicle { .. }
            | ItemKind::Harvester { .. } => {}
        }
    }
    // Three guns, three melee weapons, and the seven melee parts and the
    // rifle's four among them.
    assert!(
        weapons >= 6 && parts >= 15,
        "{weapons} weapons, {parts} parts"
    );
    for (item, chassis) in [
        ("veyra:weapon.service_blade", "field_blade"),
        ("veyra:weapon.yard_greatblade", "heavy_cutter"),
        ("veyra:weapon.pier_glaive", "reach_glaive"),
        ("veyra:weapon.marksman_rifle", "marksman_rifle"),
    ] {
        let Some(ItemKind::Weapon { chassis: own, .. }) = items
            .get(&item.parse().expect("an id"))
            .map(|def| &def.kind)
        else {
            panic!("{item} is not a weapon");
        };
        assert_eq!(own, chassis, "{item}");
    }
    // The rifle's mounts are the chassis's five, and the parts it takes
    // are items on the sight, feed, stock and support; the prism sight and
    // the shrouded muzzle stay out.
    for (part, slot) in [
        ("vigil_long_optic", "sight"),
        ("marksman_compact_feed", "feed"),
        ("vigil_precision_stock", "stock"),
        ("marksman_field_rest", "support"),
    ] {
        let id = format!("veyra:module.{part}");
        let Some(ItemKind::Module {
            slot: own, fits, ..
        }) = items.get(&id.parse().expect("an id")).map(|def| &def.kind)
        else {
            panic!("{id} is not a module");
        };
        assert_eq!(own, slot, "{id}");
        assert!(
            fits.iter()
                .any(|f| f.to_string() == "veyra:weapon.marksman_rifle")
        );
    }
    for part in ["vigil_reflex", "vigil_compensator"] {
        let id = format!("veyra:module.{part}");
        let Some(ItemKind::Module { fits, .. }) =
            items.get(&id.parse().expect("an id")).map(|d| &d.kind)
        else {
            panic!("{id} is not a module");
        };
        assert!(
            fits.iter()
                .any(|f| f.to_string() == "veyra:weapon.marksman_rifle"),
            "{id}"
        );
    }
    for out in ["vigil_prism", "vigil_shrouded_muzzle"] {
        assert!(
            items
                .get(&format!("veyra:module.{out}").parse().expect("an id"))
                .is_none(),
            "{out}"
        );
    }
    for part in [
        "blade_hooked_edge",
        "blade_balanced_grip",
        "cutter_reinforced_head",
        "cutter_compact_power",
        "cutter_wrapped_grip",
        "glaive_broad_edge",
        "glaive_counterweight",
    ] {
        let id = format!("veyra:module.{part}");
        assert!(
            items.get(&id.parse().expect("an id")).is_some(),
            "{id} is not an item"
        );
    }
}

/// One model bake for the tests below, for the reason `BAKE` is one.
static MODELS: LazyLock<content::WeaponModelFiles> =
    LazyLock::new(|| content::weapon_models().expect("the weapon models bake"));

/// The models are the pictures' pieces: the same mounts and modules of every
/// weapon, so a screen and the hand can never disagree about what fits.
#[test]
fn the_models_hold_the_same_pieces_as_the_pictures() {
    let (_, models) = &*MODELS;
    let (_, pictures) = &*BAKE;
    assert_eq!(
        models.chassis.keys().collect::<Vec<_>>(),
        pictures.chassis.keys().collect::<Vec<_>>()
    );
    let keys = |pieces: Vec<(&Option<String>, &Option<String>)>| {
        pieces
            .into_iter()
            .map(|(a, b)| (a.clone(), b.clone()))
            .collect::<BTreeSet<_>>()
    };
    for (chassis, ours) in &models.chassis {
        let theirs = &pictures.chassis[chassis];
        assert_eq!(
            keys(ours.pieces.iter().map(|p| (&p.mount, &p.module)).collect()),
            keys(
                theirs
                    .pieces
                    .iter()
                    .map(|p| (&p.mount, &p.module))
                    .collect()
            ),
            "{chassis}"
        );
    }
}

/// Every piece wears a material the weapon library defines, and the tip is
/// on the muzzle mount's pieces alone.
#[test]
fn every_model_piece_wears_a_defined_material() {
    let (props, models) = &*MODELS;
    let library = content::weapon_definitions();
    assert_eq!(models.materials, content::WEAPON_LIBRARY);
    let by_file: BTreeMap<&str, _> = props.iter().map(|(f, p)| (f.as_str(), p)).collect();
    for (chassis, set) in &models.chassis {
        for piece in &set.pieces {
            let prop = by_file
                .get(piece.file.as_str())
                .unwrap_or_else(|| panic!("{chassis}: no prop for {}", piece.file));
            assert_eq!(
                (prop.levels.len(), prop.colliders.len()),
                (1, 0),
                "{}",
                piece.file
            );
            for level_piece in &prop.levels[0].pieces {
                assert!(
                    library.materials.contains_key(&level_piece.material),
                    "{}: `{}` is not in the weapon library",
                    piece.file,
                    level_piece.material
                );
            }
            assert_eq!(
                piece.tip.is_some(),
                piece.mount.as_deref() == Some("muzzle"),
                "{}",
                piece.file
            );
        }
    }
}

/// The item files name pieces the models have: a weapon its chassis, a
/// module its (slot, part) on every chassis it fits.
#[test]
fn every_item_file_has_its_model_pieces() {
    let (_, models) = &*MODELS;
    let assets = concat!(env!("CARGO_MANIFEST_DIR"), "/../../assets");
    let registries = strata_registry::load_from_dir(assets).expect("assets/ loads");
    let items = &registries.items;
    for def in items.iter() {
        match &def.kind {
            ItemKind::Weapon { chassis, .. } => {
                if chassis.is_empty() {
                    continue;
                }
                let set = models
                    .chassis
                    .get(chassis)
                    .unwrap_or_else(|| panic!("{}: no models for {chassis}", def.id));
                assert!(set.piece(None, None).is_some(), "{}", def.id);
            }
            ItemKind::Module {
                slot, fits, part, ..
            } => {
                for weapon in fits {
                    let Some(ItemKind::Weapon { chassis, .. }) =
                        items.get(weapon).map(|def| &def.kind)
                    else {
                        panic!("{}: fits `{weapon}`, which is no weapon", def.id);
                    };
                    assert!(
                        models.chassis[chassis]
                            .piece(Some(slot), Some(part))
                            .is_some(),
                        "{}: no model piece on {chassis}",
                        def.id
                    );
                }
            }
            _ => {}
        }
    }
}
