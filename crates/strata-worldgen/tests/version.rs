//! The world's version tells two worlds apart and one world from itself.

mod common;

use strata_worldgen::{WorldParams, world_version};

#[test]
fn the_same_world_twice_has_the_same_version() {
    let params = WorldParams::default();
    assert_eq!(
        world_version(&params, &common::registries()),
        world_version(&params, &common::registries())
    );
}

#[test]
fn one_parameter_changes_the_version() {
    let registries = common::registries();
    let params = WorldParams::default();
    let nearer = WorldParams {
        range_wavelength_m: params.range_wavelength_m * 0.5,
        ..params
    };
    assert_ne!(
        world_version(&params, &registries),
        world_version(&nearer, &registries)
    );
    // The last bit of one float is enough.
    let nudged = WorldParams {
        land_bias: f64::from_bits(params.land_bias.to_bits() ^ 1),
        ..params
    };
    assert_ne!(
        world_version(&params, &registries),
        world_version(&nudged, &registries)
    );
}

#[test]
fn one_biomes_amplitude_changes_the_version() {
    let registries = common::registries();
    let params = WorldParams::default();
    let before = world_version(&params, &registries);
    let text = std::fs::read_to_string(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../assets/biomes/plains.biome.ron"
    ))
    .expect("the plains ship");
    let def: strata_data::BiomeDef = ron::from_str(&text).expect("the plains parse");
    let mut taller = def.clone();
    taller.relief.amplitude_m += 1.0;
    assert_ne!(def, taller);
    // The copy with the file as it was is the same world, so what the
    // taller copy changes is the amplitude and nothing the copy lost.
    let assets = copy_assets_with("biomes/plains.biome.ron", &def);
    let same = strata_data::load_world_from_dir(&assets).expect("the copy loads");
    assert_eq!(before, world_version(&params, &same));
    let assets = copy_assets_with("biomes/plains.biome.ron", &taller);
    let changed = strata_data::load_world_from_dir(&assets).expect("the copy loads");
    assert_ne!(before, world_version(&params, &changed));
    let _ = std::fs::remove_dir_all(&assets);
}

#[test]
fn a_biomes_colour_grade_does_not_change_the_version() {
    let registries = common::registries();
    let params = WorldParams::default();
    let before = world_version(&params, &registries);
    let text = std::fs::read_to_string(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../assets/biomes/forest.biome.ron"
    ))
    .expect("the forest ships");
    // Its own file: each replaced file gets one scratch directory.
    let mut def: strata_data::BiomeDef = ron::from_str(&text).expect("the forest parses");
    def.grade.temperature += 0.01;
    def.grade.saturation = 0.5;
    let assets = copy_assets_with("biomes/forest.biome.ron", &def);
    let regraded = strata_data::load_world_from_dir(&assets).expect("the copy loads");
    assert_eq!(before, world_version(&params, &regraded));
    let _ = std::fs::remove_dir_all(&assets);
}

#[test]
fn a_settlement_changes_the_version() {
    let mut registries = common::registries();
    let params = WorldParams::default();
    let before = world_version(&params, &registries);
    registries.settlements[0].at[0] += 1.0;
    assert_ne!(before, world_version(&params, &registries));
    registries.settlements.clear();
    assert_ne!(before, world_version(&params, &registries));
}

/// A site kind's numbers move the world: the sites it places are its
/// ground (ADR 0012).
#[test]
fn one_site_kind_changes_the_version() {
    let mut registries = common::registries();
    let params = WorldParams::default();
    let before = world_version(&params, &registries);
    let text = std::fs::read_to_string(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../assets/sites/hamlet.site.ron"
    ))
    .expect("the hamlet ships");
    let value: ron::Value = ron::from_str(&text).expect("it parses");
    let def: strata_data::SiteKindDef = value.into_rust().expect("it is a kind");
    let assets = copy_assets_with("sites/hamlet.site.ron", &def);
    let same = strata_data::load_world_from_dir(&assets).expect("the copy loads");
    assert_eq!(before, world_version(&params, &same));
    let mut wider = def.clone();
    wider.footprint_m += 1.0;
    let assets = copy_assets_with("sites/hamlet.site.ron", &wider);
    let changed = strata_data::load_world_from_dir(&assets).expect("the copy loads");
    assert_ne!(before, world_version(&params, &changed));
    let _ = std::fs::remove_dir_all(&assets);
    registries.sites = strata_data::SiteKindStore::default();
    assert_ne!(before, world_version(&params, &registries));
}

#[test]
fn one_flora_species_changes_the_version() {
    let registries = common::registries();
    let params = WorldParams::default();
    let before = world_version(&params, &registries);
    let (_, name, def) = registries
        .flora
        .iter()
        .next()
        .expect("the shipped registries have flora");
    let file = flora_file(name.as_ref());
    let text = std::fs::read_to_string(&file).expect("the species' file reads");
    // Through a value, as the loader reads it (a flora file leans on RON's
    // untyped forms that a typed parse refuses).
    let value: ron::Value = ron::from_str(&text).expect("it parses");
    let parsed: strata_data::FloraDef = value.into_rust().expect("it is a species");
    assert_eq!(&parsed, def, "{} is the file of {name}", file.display());
    let mut denser = parsed.clone();
    denser.spacing_m *= 0.8;
    assert_ne!(parsed, denser);
    let relative = file
        .strip_prefix(concat!(env!("CARGO_MANIFEST_DIR"), "/../../assets/"))
        .expect("under the assets")
        .to_str()
        .expect("utf-8")
        .to_owned();
    let assets = copy_assets_with(&relative, &parsed);
    let same = strata_data::load_world_from_dir(&assets).expect("the copy loads");
    assert_eq!(before, world_version(&params, &same));
    let assets = copy_assets_with(&relative, &denser);
    let changed = strata_data::load_world_from_dir(&assets).expect("the copy loads");
    assert_ne!(before, world_version(&params, &changed));
    let _ = std::fs::remove_dir_all(&assets);
}

/// A dressing species (`FloraDef::dressing`, the reeds) is the client's
/// alone and moves no other plant, so the world's version leaves it out:
/// with it, without it and with it sparser the version is the same, and
/// the same species not dressing is the world's and changes it.
#[test]
fn a_dressing_species_leaves_the_version_alone() {
    let registries = common::registries();
    let params = WorldParams::default();
    let before = world_version(&params, &registries);
    let (_, name, def) = registries
        .flora
        .iter()
        .find(|(_, _, def)| def.dressing)
        .expect("the shipped reeds are dressing");
    let file = flora_file(name.as_ref());
    let relative = file
        .strip_prefix(concat!(env!("CARGO_MANIFEST_DIR"), "/../../assets/"))
        .expect("under the assets")
        .to_str()
        .expect("utf-8")
        .to_owned();
    let mut sparser = def.clone();
    sparser.spacing_m *= 1.25;
    let assets = copy_assets_with(&relative, &sparser);
    let changed = strata_data::load_world_from_dir(&assets).expect("the copy loads");
    assert_eq!(before, world_version(&params, &changed), "sparser reeds");
    std::fs::remove_file(assets.join(&relative)).expect("the copy's reeds");
    let without = strata_data::load_world_from_dir(&assets).expect("the copy loads");
    assert!(without.flora.iter().all(|(_, _, def)| !def.dressing));
    assert_eq!(before, world_version(&params, &without), "no reeds");
    let mut worldly = def.clone();
    worldly.dressing = false;
    let assets = copy_assets_with(&relative, &worldly);
    let changed = strata_data::load_world_from_dir(&assets).expect("the copy loads");
    assert_ne!(
        before,
        world_version(&params, &changed),
        "reeds not dressing"
    );
    let _ = std::fs::remove_dir_all(&assets);
}

#[test]
fn one_province_changes_the_version() {
    let registries = common::registries();
    let params = WorldParams::default();
    let before = world_version(&params, &registries);
    let text = std::fs::read_to_string(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../assets/geology/badlands.geology.ron"
    ))
    .expect("the badlands ship");
    let value: ron::Value = ron::from_str(&text).expect("it parses");
    let def: strata_data::ProvinceDef = value.into_rust().expect("it is a province");
    let mut greener = def.clone();
    greener.rock.cover += 0.1;
    assert_ne!(def, greener);
    let assets = copy_assets_with("geology/badlands.geology.ron", &def);
    let same = strata_data::load_world_from_dir(&assets).expect("the copy loads");
    assert_eq!(before, world_version(&params, &same));
    let assets = copy_assets_with("geology/badlands.geology.ron", &greener);
    let changed = strata_data::load_world_from_dir(&assets).expect("the copy loads");
    assert_ne!(before, world_version(&params, &changed));
    let _ = std::fs::remove_dir_all(&assets);
}

/// The shipped world's version, pinned: the same on every platform, and a
/// change to it (a parameter, a registry, or code that moves the canary's
/// samples) shows here. If the ground moved on purpose, bump
/// `GENERATOR_VERSION` when code moved it, and paste the new value.
#[test]
fn the_shipped_world_has_the_pinned_version() {
    let version = world_version(&WorldParams::default(), &common::registries());
    assert_eq!(
        version, PINNED,
        "the shipped world's version is {version:#018x}, pinned {PINNED:#018x}"
    );
}

/// The value [`the_shipped_world_has_the_pinned_version`] holds. Last moved
/// by ticket 0012 step 5: the wreck and the derelict machine site kinds, two
/// data files (`assets/sites/wreck.site.ron`, `wreck_works.site.ron`), no
/// `GENERATOR_VERSION` bump; was `0x7e0f_4a47_794f_227d`. Before that by
/// ticket 0012 step 2: the outlaw camp site kind, a data file alone
/// (`assets/sites/wild_camp.site.ron`), no `GENERATOR_VERSION` bump; was
/// `0x0a24_b0e8_ffa4_563b`. Before that by the alignment pass's third round, the wall search round a bridge
/// whose searched line comes down past its grade: `GENERATOR_VERSION` 17
/// and the island canary's roads (the other three canaries did not move);
/// was `0x60a3_f938_2150_eb36`. Before that by the roads' alignment pass:
/// `GENERATOR_VERSION` 16, the four canary networks' roads (the search
/// prices cut and fill and turns near a river, bridges may stand skew, a
/// line runs straight through its bridge and swings onto it) and the ten
/// samples across the canary window's first graded road, which moved with
/// it; was `0xb032_9121_da5e_9251`
/// (`0xdf07_1046_06b5_678f` as the pass was first built, before its review
/// held each bridge's shelf out of the channel and moved the island's
/// roads; `0x127e_bf98_ea54_59e0` until the pass's second round, whose
/// search reads a crossing's banks as the record does and closes one the
/// kit cannot stand, moved the canary networks' roads and the samples by
/// them again, `GENERATOR_VERSION` still 16).
/// Before that by the road's grade (ticket 0005, step 6): `GENERATOR_VERSION` 15, the
/// four canary networks with their roads' profiles and crossings (a
/// gorge's creek bridged, a deck at its class's freeboard, its abutments)
/// and the sites the valley walls' wander moved, the canary's samples, and
/// ten samples across the canary window's first graded road (its ground,
/// cut and cleared shares, which no other canary reads); was
/// `0x3235_12a7_7f98_94aa` (`0xbdcc_83e3_c638_eb0b` as steps 5 and 6 were
/// first built, before their review).
/// Before that by the road network (ticket 0005, step 5): the four canary
/// networks with the roads their bakes lay between their sites; no sample
/// moved and `GENERATOR_VERSION` stays 14; was `0x7a48_c3e0_6fb9_5fb9`.
/// Before that by procedural sites (ticket 0005, step 4): `GENERATOR_VERSION` 14, the six
/// site kinds of `assets/sites`, the four canary networks with the sites
/// their bakes place, and the canary's samples, a pad among them where one
/// lies in the canary window; was `0x54d5_aa11_5e0a_56ee`
/// (`0xb4fb_cd64_a3a0_f989` as step 4 was first built, before its review
/// shrank four kinds' pads, refined places finer, cleared channels by their
/// own meanders and broke up the gravel's edge).
/// Before that by the rivers' shape (ticket 0005, step 3b): `GENERATOR_VERSION` 13, the
/// river numbers the shape adds, the four canary networks (settled,
/// meandering, their outlets held at their lakes' levels) and the canary's
/// samples by the rivers; was `0xca97_4757_c41f_38ee`.
/// Before that by the carve (ticket 0005, step 2): `GENERATOR_VERSION` 12, the river
/// numbers the carve adds, the four canary networks (surfaces set onto the
/// sample's ground, breaches graded, mouths graded to a vertex at the shore,
/// the routing along a province's grain), the canary's samples (each
/// carrying its water, read through the canary window alone) and four carve
/// points beside the rivers south of Port Serein; was
/// `0x741c_60ae_d128_7060` (`0xd02f_50a6_e383_bbcd` as step 2 was first
/// built, before its review).
/// Before that by the river network (ticket 0005, step 1, and its review): its numbers,
/// the constants a bake depends on and three canary networks (a small
/// island, two cut parts of it, a window round Port Serein) join the hash;
/// no sample moved and `GENERATOR_VERSION` stays 11.
/// Before that by finite settlement benches: `GENERATOR_VERSION` 11 and the
/// city's canary samples. Before that by the review of step 5: version 10, the beds' patch
/// parameters and the canary's samples (the gate's patches, the shaping
/// bed's label); the road's end did not move. Before that by ticket 0004's
/// step 5: `GENERATOR_VERSION` 9, the beds' and the
/// crests' parameters, three province files (basalt's and folded rock's
/// beds, sandstone's and basalt's slope gates) and the canary's samples;
/// the road's end did not move. Before that by the review of step 4:
/// `GENERATOR_VERSION` 8, two
/// province files and the canary's samples moved, and the canary's road
/// point followed the road's new end. Before that by step 4:
/// `GENERATOR_VERSION` 7, the new parameters, the provinces' relief rules,
/// the mountains' relief and the canary's samples all moved, and the
/// canary's road point followed the road's new end. Before that by step 3e, by data alone: the six rock species list the
/// same widened biomes, since the province now says which rock stands where
/// (no sample moved, so no generator bump). Before that by step 3c: five
/// provinces' faces and the five new layers the canary's samples carry.
const PINNED: u64 = 0xe3a4_b4f5_be38_2801;

/// The `.flora.ron` file under `assets/flora` whose id is `name`.
fn flora_file(name: &str) -> std::path::PathBuf {
    fn walk(dir: &std::path::Path, name: &str) -> Option<std::path::PathBuf> {
        for entry in std::fs::read_dir(dir).ok()? {
            let path = entry.ok()?.path();
            if path.is_dir() {
                if let Some(found) = walk(&path, name) {
                    return Some(found);
                }
            } else if path.to_string_lossy().ends_with(".flora.ron")
                && std::fs::read_to_string(&path)
                    .is_ok_and(|text| text.contains(&format!("id: \"{name}\"")))
            {
                return Some(path);
            }
        }
        None
    }
    walk(
        std::path::Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/../../assets/flora")),
        name,
    )
    .unwrap_or_else(|| panic!("no file for {name}"))
}

/// A copy of the shipped registries (`assets/biomes`, `terrain`, `flora`,
/// `geology`, `settlements` and `sites`, patches included) with one file replaced by `value`,
/// in a directory of its own (one per file) under the target dir.
fn copy_assets_with(file: &str, value: &impl serde::Serialize) -> std::path::PathBuf {
    let source = std::path::Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/../../assets"));
    // One directory per replaced file: the tests run in parallel.
    let target = std::path::Path::new(env!("CARGO_TARGET_TMPDIR"))
        .join(format!("version-assets-{}", file.replace('/', "-")));
    let _ = std::fs::remove_dir_all(&target);
    for dir in [
        "biomes",
        "terrain",
        "flora",
        "geology",
        "settlements",
        "sites",
    ] {
        copy_tree(&source.join(dir), &target.join(dir));
    }
    let text =
        ron::ser::to_string_pretty(value, ron::ser::PrettyConfig::default()).expect("serialises");
    std::fs::write(target.join(file), text).expect("the replaced file");
    target
}

fn copy_tree(from: &std::path::Path, to: &std::path::Path) {
    if !from.is_dir() {
        return;
    }
    std::fs::create_dir_all(to).expect("a scratch directory");
    for entry in std::fs::read_dir(from).expect("the shipped assets list") {
        let entry = entry.expect("a directory entry");
        let path = entry.path();
        if path.is_dir() {
            copy_tree(&path, &to.join(entry.file_name()));
        } else {
            std::fs::copy(&path, to.join(entry.file_name())).expect("a copy");
        }
    }
}
