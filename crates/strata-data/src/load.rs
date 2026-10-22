//! The data stage for the world: files in, a frozen [`WorldDefs`] out.
//!
//! The layout under the asset root is flat rather than slotted's
//! `data/<modid>/<kind>/`, because the slice has one data set and no mods:
//!
//! ```text
//! biomes/*.biome.ron                        one BiomeDef per file
//! biomes/patches/{base,updates,final_fixes}/*.ron    PatchList over biomes
//! terrain/materials.ron                     a list of TerrainMaterialDef
//! terrain/patches/{base,updates,final_fixes}/*.ron   PatchList over materials
//! flora/*.flora.ron                          one FloraDef per file
//! flora/patches/{base,updates,final_fixes}/*.ron     PatchList over flora
//! geology/*.geology.ron                      one ProvinceDef per file
//! geology/patches/{base,updates,final_fixes}/*.ron   PatchList over provinces
//! settlements/*.settlement.ron               one SettlementDef per file
//! sites/*.site.ron                           one SiteKindDef per file
//! sites/patches/{base,updates,final_fixes}/*.ron     PatchList over site kinds
//! ```
//!
//! The game's own files (items, schematics, quests and the rest) are read by
//! `veyra-data` over the same helpers, after this half.
//!
//! Files parse to a raw RON `Value` first, patches run over the raw values
//! in slotted's three rounds, and only then does anything become a typed
//! def. That order is what lets a later mod add a field the base data does
//! not know about, and it is why the def types keep to the `Value`-safe
//! rule. IO goes through slotted's [`AssetSource`] port so a pack layer can
//! replace the directory later without this module noticing.

use std::path::Path;

use slotted_model::Namespaced;
use slotted_registry::loader::{AssetSource, DirSource, SourceError};
use slotted_registry::patch::{self, PatchError, PatchList, RawEntries, Round};
use slotted_registry::registry::RegistryError;

use crate::biome::BiomeDef;
use crate::defs::{FreezeError, OpenRegistries, WorldDefs};
use crate::flora::FloraDef;
use crate::geology::ProvinceDef;
use crate::material::TerrainMaterialDef;
use crate::settlement::SettlementDef;
use crate::site::SiteKindDef;
use crate::values::{VALUES_FILE, ValueBudget};

/// The directory of `.biome.ron` files, relative to the asset root.
pub const BIOMES_DIR: &str = "biomes";
/// The suffix a biome file must carry to be read.
pub const BIOME_SUFFIX: &str = ".biome.ron";
/// The material list, relative to the asset root.
pub const MATERIALS_FILE: &str = "terrain/materials.ron";
/// The directory of material patch rounds, relative to the asset root.
pub const TERRAIN_DIR: &str = "terrain";
/// The directory of `.flora.ron` files, relative to the asset root. Absent
/// for a world with nothing growing in it.
pub const FLORA_DIR: &str = "flora";
/// The suffix a flora file must carry to be read.
pub const FLORA_SUFFIX: &str = ".flora.ron";
/// The directory of `.geology.ron` files, relative to the asset root.
/// Absent for a world without geology, whose province field is off.
pub const GEOLOGY_DIR: &str = "geology";
/// The suffix a province file must carry to be read.
pub const GEOLOGY_SUFFIX: &str = ".geology.ron";
/// The directory of `.settlement.ron` files, relative to the asset root.
/// Absent for a world with no cities.
pub const SETTLEMENTS_DIR: &str = "settlements";
/// The suffix a settlement file must carry to be read.
pub const SETTLEMENT_SUFFIX: &str = ".settlement.ron";
/// The directory of `.site.ron` files, relative to the asset root. Absent
/// for a world that places no sites (ADR 0012).
pub const SITES_DIR: &str = "sites";
/// The suffix a site kind file must carry to be read.
pub const SITE_SUFFIX: &str = ".site.ron";

/// Why the world half of the data stage stopped. Every variant names the file, because the
/// first thing anyone needs is which file is broken.
#[derive(Debug, thiserror::Error)]
pub enum LoadError {
    /// The source could not read or list something.
    #[error("{0}")]
    Source(#[from] SourceError),
    /// A file is not UTF-8.
    #[error("`{path}` is not UTF-8")]
    NotUtf8 {
        /// The file.
        path: String,
    },
    /// A file is not RON.
    #[error("`{path}`: {source}")]
    Parse {
        /// The file.
        path: String,
        /// The parser's complaint, with a line and column. Boxed because
        /// it is by far the largest variant and every loader function
        /// returns this type.
        source: Box<ron::error::SpannedError>,
    },
    /// A file has no `id: "namespace:path"` field, or an invalid one.
    #[error("`{path}`: {reason}")]
    MissingId {
        /// The file.
        path: String,
        /// What was found instead.
        reason: String,
    },
    /// Two files claim one id.
    #[error("`{path}` registers `{id}`, which `{first}` already registered")]
    Duplicate {
        /// The id.
        id: Namespaced,
        /// The file that had it first.
        first: String,
        /// The file that had it second.
        path: String,
    },
    /// A patch could not be applied.
    #[error("patch file `{path}`: {source}")]
    Patch {
        /// The patch file.
        path: String,
        /// What went wrong.
        source: PatchError,
    },
    /// A raw entry, after patching, does not fit its def type.
    #[error("`{path}` (`{id}`): {message}")]
    Deserialize {
        /// The file the entry came from.
        path: String,
        /// The entry.
        id: Namespaced,
        /// serde's complaint.
        message: String,
    },
    /// Registering an entry failed. Should not happen after the duplicate
    /// check, but the registry's own error is kept rather than unwrapped.
    #[error("{0}")]
    Registry(#[from] RegistryError),
    /// The set loaded but is not consistent.
    #[error("{0}")]
    Freeze(#[from] FreezeError),
}

/// Loads and freezes the world's registries under `root`, a directory laid out as
/// the module docs describe.
///
/// # Errors
///
/// [`LoadError`] for any unreadable, unparsable, duplicate or inconsistent
/// file.
pub fn load_world_from_dir(root: impl AsRef<Path>) -> Result<WorldDefs, LoadError> {
    load_world_from_source(&DirSource::new(root.as_ref()))
}

/// Loads and freezes the world's registries from any [`AssetSource`] with the same
/// layout. Tests use an in-memory source; a pack layer will use its own.
///
/// # Errors
///
/// [`LoadError`] for any unreadable, unparsable, duplicate or inconsistent
/// file.
pub fn load_world_from_source(source: &dyn AssetSource) -> Result<WorldDefs, LoadError> {
    let mut biomes = RawRegistry::default();
    for path in source.list(BIOMES_DIR)? {
        if !path.ends_with(BIOME_SUFFIX) {
            continue;
        }
        let value = parse_file(source, &path)?;
        biomes.insert(&path, value)?;
    }

    let mut materials = RawRegistry::default();
    match read_utf8(source, MATERIALS_FILE) {
        Ok(text) => {
            let list: ron::Value = ron::from_str(&text).map_err(|source| LoadError::Parse {
                path: MATERIALS_FILE.to_owned(),
                source: Box::new(source),
            })?;
            let ron::Value::Seq(entries) = list else {
                return Err(LoadError::MissingId {
                    path: MATERIALS_FILE.to_owned(),
                    reason: "expected a list of materials".to_owned(),
                });
            };
            for value in entries {
                materials.insert(MATERIALS_FILE, value)?;
            }
        }
        // No materials is legal for a data set of nothing but ocean; a biome
        // that names one will fail at freeze with the biome's own id.
        Err(LoadError::Source(SourceError::NotFound(_))) => {}
        Err(err) => return Err(err),
    }

    let mut flora = RawRegistry::default();
    for path in source.list(FLORA_DIR)? {
        if !path.ends_with(FLORA_SUFFIX) {
            continue;
        }
        let value = parse_file(source, &path)?;
        flora.insert(&path, value)?;
    }

    let mut provinces = RawRegistry::default();
    for path in source.list(GEOLOGY_DIR)? {
        if !path.ends_with(GEOLOGY_SUFFIX) {
            continue;
        }
        let value = parse_file(source, &path)?;
        provinces.insert(&path, value)?;
    }

    let mut sites = RawRegistry::default();
    for path in source.list(SITES_DIR)? {
        if !path.ends_with(SITE_SUFFIX) {
            continue;
        }
        let value = parse_file(source, &path)?;
        sites.insert(&path, value)?;
    }

    for round in Round::ALL {
        biomes.apply_round(source, BIOMES_DIR, round)?;
        materials.apply_round(source, TERRAIN_DIR, round)?;
        flora.apply_round(source, FLORA_DIR, round)?;
        provinces.apply_round(source, GEOLOGY_DIR, round)?;
        sites.apply_round(source, SITES_DIR, round)?;
    }

    let mut open = OpenRegistries::new();
    for (id, path, value) in materials.drain() {
        let def: TerrainMaterialDef = typed(&path, &id, value)?;
        open.add_material(def)?;
    }
    for (id, path, value) in biomes.drain() {
        let def: BiomeDef = typed(&path, &id, value)?;
        open.add_biome(def)?;
    }
    for (id, path, value) in flora.drain() {
        let def: FloraDef = typed(&path, &id, value)?;
        open.add_flora(def)?;
    }
    for (id, path, value) in provinces.drain() {
        let def: ProvinceDef = typed(&path, &id, value)?;
        open.add_province_from(def, &path)?;
    }
    for (id, path, value) in sites.drain() {
        let def: SiteKindDef = typed(&path, &id, value)?;
        open.add_site_kind_from(def, &path)?;
    }
    let mut frozen = open.freeze()?;
    frozen.settlements = settlements(source)?;
    frozen.values = values(source)?;
    Ok(frozen)
}

/// The value budget, or none where the data set ships no `look/values.ron`.
fn values(source: &dyn AssetSource) -> Result<ValueBudget, LoadError> {
    match read_utf8(source, VALUES_FILE) {
        Ok(text) => ValueBudget::parse(&text).map_err(|reason| LoadError::MissingId {
            path: VALUES_FILE.to_owned(),
            reason,
        }),
        Err(LoadError::Source(SourceError::NotFound(_))) => Ok(ValueBudget::default()),
        Err(err) => Err(err),
    }
}

/// Every settlement file, typed and checked, in id order.
fn settlements(source: &dyn AssetSource) -> Result<Vec<SettlementDef>, LoadError> {
    let paths = match source.list(SETTLEMENTS_DIR) {
        Ok(paths) => paths,
        Err(SourceError::NotFound(_)) => return Ok(Vec::new()),
        Err(err) => return Err(err.into()),
    };
    let mut defs = Vec::new();
    for path in paths {
        if !path.ends_with(SETTLEMENT_SUFFIX) {
            continue;
        }
        let value = parse_file(source, &path)?;
        let id = id_of(&path, &value)?;
        let def: SettlementDef = typed(&path, &id, value)?;
        if let Some(reason) = def.invalid_reason() {
            return Err(LoadError::MissingId {
                path,
                reason: reason.to_owned(),
            });
        }
        defs.push(def);
    }
    defs.sort_by(|a, b| a.id.cmp(&b.id));
    Ok(defs)
}

/// One registry's entries between parsing and typing, with the file each
/// came from for error messages.
#[derive(Default)]
struct RawRegistry {
    entries: RawEntries,
    origins: Vec<(Namespaced, String)>,
}

impl RawRegistry {
    fn insert(&mut self, path: &str, value: ron::Value) -> Result<(), LoadError> {
        let id = id_of(path, &value)?;
        if self.entries.contains_key(&id) {
            let first = self.origin(&id);
            return Err(LoadError::Duplicate {
                id,
                first,
                path: path.to_owned(),
            });
        }
        self.origins.push((id.clone(), path.to_owned()));
        self.entries.insert(id, value);
        Ok(())
    }

    fn apply_round(
        &mut self,
        source: &dyn AssetSource,
        dir: &str,
        round: Round,
    ) -> Result<(), LoadError> {
        let dir = format!("{dir}/patches/{}", round.dir());
        for path in source.list(&dir)? {
            let is_ron = Path::new(&path)
                .extension()
                .is_some_and(|ext| ext.eq_ignore_ascii_case("ron"));
            if !is_ron {
                continue;
            }
            let text = read_utf8(source, &path)?;
            let list: PatchList = ron::from_str(&text).map_err(|source| LoadError::Parse {
                path: path.clone(),
                source: Box::new(source),
            })?;
            for patch in &list {
                patch::apply(&mut self.entries, patch).map_err(|source| LoadError::Patch {
                    path: path.clone(),
                    source,
                })?;
            }
        }
        Ok(())
    }

    /// The file `id` was read from, or empty for an entry no file declared.
    fn origin(&self, id: &Namespaced) -> String {
        self.origins
            .iter()
            .find(|(other, _)| other == id)
            .map(|(_, path)| path.clone())
            .unwrap_or_default()
    }

    fn drain(self) -> Vec<(Namespaced, String, ron::Value)> {
        let origins = self.origins;
        self.entries
            .into_iter()
            .map(|(id, value)| {
                let path = origins
                    .iter()
                    .find(|(other, _)| *other == id)
                    .map(|(_, path)| path.clone())
                    .unwrap_or_default();
                (id, path, value)
            })
            .collect()
    }
}

/// A file as UTF-8 text, for the game half of the loader, which reads its
/// own files through the same port and the same error.
pub fn read_utf8(source: &dyn AssetSource, path: &str) -> Result<String, LoadError> {
    let bytes = source.read(path)?;
    String::from_utf8(bytes).map_err(|_| LoadError::NotUtf8 {
        path: path.to_owned(),
    })
}

fn parse_file(source: &dyn AssetSource, path: &str) -> Result<ron::Value, LoadError> {
    let text = read_utf8(source, path)?;
    ron::from_str(&text).map_err(|source| LoadError::Parse {
        path: path.to_owned(),
        source: Box::new(source),
    })
}

/// The `id` field of a raw entry, which is the registry key.
fn id_of(path: &str, value: &ron::Value) -> Result<Namespaced, LoadError> {
    let missing = |reason: &str| LoadError::MissingId {
        path: path.to_owned(),
        reason: reason.to_owned(),
    };
    let ron::Value::Map(map) = value else {
        return Err(missing("expected a struct with an `id` field"));
    };
    let key = ron::Value::String("id".to_owned());
    let Some(ron::Value::String(id)) = map.get(&key) else {
        return Err(missing("expected an `id: \"namespace:path\"` field"));
    };
    Namespaced::parse(id).map_err(|err| missing(&err.to_string()))
}

fn typed<T: serde::de::DeserializeOwned>(
    path: &str,
    id: &Namespaced,
    value: ron::Value,
) -> Result<T, LoadError> {
    value.clone().into_rust().map_err(|err| {
        let mut message = err.to_string();
        // serde names a missing or misspelt field but not one of the wrong
        // type ("expected f32 but found a string"), and a file of twenty
        // numbers needs to say which.
        if let Some(field) = blame::<T>(&value) {
            message = format!("{message} (at `{field}`)");
        }
        LoadError::Deserialize {
            path: path.to_owned(),
            id: id.clone(),
            message,
        }
    })
}

/// The field of a raw entry that stops it typing, found by leaving fields
/// out one at a time: the one whose absence makes the rest type (or fail
/// only for its absence) is the culprit. Looks two levels down, into a
/// nested struct or the structs of a list (`rock.cover`, `relief[1].shape`),
/// which is as deep as any def goes. Only run once a def has failed, so its
/// cost is nobody's.
///
/// It tells "only the field's absence" from other errors by the words
/// "field named `x`" in the missing-field message as ron 0.12.2 prints it.
/// A ron that rewords it
/// degrades every registry's message to serde's bare one without failing a
/// load; the wrong-type cases of `a_wrong_province_names_its_file_and_field`
/// catch that.
fn blame<T: serde::de::DeserializeOwned>(value: &ron::Value) -> Option<String> {
    let clears = |candidate: ron::Value, name: &str| match candidate.into_rust::<T>() {
        Ok(_) => true,
        Err(err) => err.to_string().contains(&format!("field named `{name}`")),
    };
    let ron::Value::Map(map) = value else {
        return None;
    };
    for (key, inner) in map.iter() {
        let ron::Value::String(name) = key else {
            continue;
        };
        let mut without = map.clone();
        without.remove(key);
        if !clears(ron::Value::Map(without), name) {
            continue;
        }
        // Which part of it, when it has parts.
        let nested: Vec<(String, ron::Map)> = match inner {
            ron::Value::Map(fields) => vec![(name.clone(), fields.clone())],
            ron::Value::Seq(items) => items
                .iter()
                .enumerate()
                .filter_map(|(i, item)| match item {
                    ron::Value::Map(fields) => Some((format!("{name}[{i}]"), fields.clone())),
                    _ => None,
                })
                .collect(),
            _ => Vec::new(),
        };
        for (at, fields) in nested {
            for (sub_key, _) in fields.iter() {
                let ron::Value::String(sub) = sub_key else {
                    continue;
                };
                let mut trimmed = fields.clone();
                trimmed.remove(sub_key);
                let mut candidate = map.clone();
                let replacement = match inner {
                    ron::Value::Seq(items) => {
                        let index = at
                            .rsplit('[')
                            .next()
                            .and_then(|rest| rest.trim_end_matches(']').parse::<usize>().ok());
                        let mut items = items.clone();
                        if let Some(item) = index.and_then(|i| items.get_mut(i)) {
                            *item = ron::Value::Map(trimmed);
                        }
                        ron::Value::Seq(items)
                    }
                    _ => ron::Value::Map(trimmed),
                };
                candidate.insert(key.clone(), replacement);
                if clears(ron::Value::Map(candidate), sub) {
                    return Some(format!("{at}.{sub}"));
                }
            }
        }
        return Some(name.clone());
    }
    None
}

#[cfg(test)]
mod tests {
    use std::fmt::Write as _;

    use pretty_assertions::assert_eq;
    use slotted_registry::loader::InMemorySource;

    use super::*;
    use crate::biome::{BiomeId, ReliefKind};
    use crate::material::TerrainMaterialId;

    const GRASS_AND_ROCK: &str = r#"[
        (id: "strata:rock", texture_set: "rock", tiling_m: 6.0),
        (id: "strata:grass", texture_set: "grass", tiling_m: 4.0),
    ]"#;

    fn plains(material: &str) -> String {
        format!(
            r#"(
                id: "strata:plains",
                name: "Plains",
                climate: (
                    temperature: (min: 0.3, max: 0.8),
                    moisture: (min: 0.2, max: 0.7),
                    elevation: (min: 0.5, max: 0.62),
                ),
                relief: (kind: "plains", amplitude_m: 12.0, wavelength_m: 900.0, octaves: 3),
                surface: (material: "{material}", slope_material: "strata:rock"),
                tint: (0.45, 0.62, 0.28),
            )"#
        )
    }

    fn minimal() -> InMemorySource {
        InMemorySource::new()
            .with("biomes/plains.biome.ron", plains("strata:grass"))
            .with("biomes/notes.txt", "ignored")
            .with("terrain/materials.ron", GRASS_AND_ROCK)
    }

    #[test]
    fn a_minimal_set_loads_and_freezes() {
        let frozen = load_world_from_source(&minimal()).expect("loads");
        assert_eq!(frozen.biomes.len(), 1);
        assert_eq!(frozen.materials.len(), 2);
        let plains = frozen
            .biome_id(&Namespaced::parse("strata:plains").expect("valid"))
            .expect("registered");
        assert_eq!(plains, BiomeId(0));
        let def = frozen.biomes.get(plains).expect("registered");
        assert_eq!(def.relief.kind, ReliefKind::Plains);
        assert!(
            (def.relief.persistence - 0.5).abs() < f32::EPSILON,
            "default applied"
        );
        let surface = frozen.surface(plains).expect("resolved");
        assert_eq!(
            surface.material,
            TerrainMaterialId(0),
            "grass sorts before rock"
        );
        assert_eq!(surface.slope_material, TerrainMaterialId(1));
        assert_eq!(surface.slope_material.layer(), 1);
    }

    #[test]
    fn flora_files_load_and_patch() {
        let source = minimal()
            .with(
                "flora/meadow.flora.ron",
                r#"(
                    id: "strata:meadow",
                    name: "Meadow grass",
                    kind: "grass",
                    biomes: [(biome: "strata:plains", weight: 0.9)],
                    density_per_m2: 6.0,
                    tint_dry: (0.5, 0.5, 0.2),
                    tint_lush: (0.2, 0.5, 0.1),
                )"#,
            )
            .with(
                "flora/patches/updates/thin.ron",
                r#"[(target: "strata:meadow", op: Merge({"density_per_m2": 2.0}))]"#,
            );
        let frozen = load_world_from_source(&source).expect("loads");
        let id = frozen
            .flora_id(&Namespaced::parse("strata:meadow").expect("valid"))
            .expect("registered");
        let def = frozen.flora.get(id).expect("registered");
        assert!((def.density_per_m2 - 2.0).abs() < f32::EPSILON, "patched");
        assert!((frozen.flora_fit(id).expect("fit").weight(BiomeId(0)) - 0.9).abs() < f32::EPSILON);
    }

    #[test]
    fn patch_rounds_rewrite_raw_entries_before_typing() {
        let source = minimal()
            .with(
                "biomes/patches/base/tune.ron",
                r#"[(target: "strata:plains", op: Merge({"relief": {"amplitude_m": 40.0}}))]"#,
            )
            .with(
                "biomes/patches/final_fixes/last_word.ron",
                r#"[(target: "strata:plains", op: Merge({"name": "Steppe"}))]"#,
            )
            .with(
                "terrain/patches/updates/tiling.ron",
                r#"[(target: "strata:grass", op: Merge({"tiling_m": 8.0}))]"#,
            );
        let frozen = load_world_from_source(&source).expect("loads");
        let plains = frozen.biomes.get(BiomeId(0)).expect("registered");
        assert!((plains.relief.amplitude_m - 40.0).abs() < f32::EPSILON);
        assert_eq!(plains.name, "Steppe");
        let grass = frozen
            .materials
            .get(TerrainMaterialId(0))
            .expect("registered");
        assert!((grass.tiling_m - 8.0).abs() < f32::EPSILON);
    }

    #[test]
    fn a_patch_can_remove_an_entry() {
        let source = minimal().with(
            "terrain/patches/base/drop.ron",
            r#"[(target: "strata:rock", op: Remove)]"#,
        );
        let err = load_world_from_source(&source).expect_err("plains needs rock for slopes");
        assert!(
            matches!(err, LoadError::Freeze(FreezeError::UnknownMaterial { .. })),
            "{err}"
        );
    }

    #[test]
    fn errors_name_the_file() {
        let broken = InMemorySource::new().with("biomes/broken.biome.ron", "(id: 42)");
        let err = load_world_from_source(&broken).expect_err("no id");
        assert!(err.to_string().contains("biomes/broken.biome.ron"), "{err}");

        let bad = InMemorySource::new().with("biomes/bad.biome.ron", "not ron at all (");
        let err = load_world_from_source(&bad).expect_err("not ron");
        assert!(matches!(err, LoadError::Parse { .. }), "{err}");

        let source = minimal().with("biomes/again.biome.ron", plains("strata:grass"));
        let err = load_world_from_source(&source).expect_err("duplicate id");
        assert!(matches!(err, LoadError::Duplicate { .. }), "{err}");

        let source = minimal().with(
            "biomes/patches/base/bad.ron",
            r#"[(target: "strata:nope", op: Remove)]"#,
        );
        let err = load_world_from_source(&source).expect_err("unknown patch target");
        assert!(matches!(err, LoadError::Patch { .. }), "{err}");

        let source = InMemorySource::new()
            .with(
                "biomes/plains.biome.ron",
                plains("strata:grass").replace("\"plains\"", "\"volcano\""),
            )
            .with("terrain/materials.ron", GRASS_AND_ROCK);
        let err = load_world_from_source(&source).expect_err("unknown relief kind");
        assert!(matches!(err, LoadError::Deserialize { .. }), "{err}");
        assert!(err.to_string().contains("volcano"), "{err}");
    }

    const ROCK_KIT: &str = r#"(
        id: "strata:granite_stone",
        name: "Granite stone",
        kind: "rock",
        biomes: [(biome: "strata:plains", weight: 0.2)],
        spacing_m: 6.0,
        tint_dry: (0.5, 0.5, 0.5),
        tint_lush: (0.5, 0.5, 0.5),
        tree: (height_m: (min: 0.3, max: 0.8)),
    )"#;

    fn province(id: &str, extra: &str) -> String {
        format!(
            r#"(
                id: "{id}",
                name: "A province",
                tint: (0.5, 0.4, 0.3),
                rock: (face: "strata:rock", cover: 0.5),
                {extra}
            )"#
        )
    }

    fn with_geology() -> InMemorySource {
        minimal()
            .with("flora/granite_stone.flora.ron", ROCK_KIT)
            .with(
                "geology/granite.geology.ron",
                province(
                    "strata:granite",
                    r#"kit: [(species: "strata:granite_stone", weight: 0.7)],"#,
                ),
            )
            .with(
                "geology/sandstone.geology.ron",
                province("strata:sandstone", r#"in_ranges: Some("strata:granite"),"#),
            )
            .with("geology/notes.txt", "ignored")
    }

    #[test]
    fn provinces_load_resolve_and_fit_their_kits() {
        let frozen = load_world_from_source(&with_geology()).expect("loads");
        assert_eq!(frozen.provinces.len(), 2);
        let granite = frozen
            .province_id(&Namespaced::parse("strata:granite").expect("valid"))
            .expect("registered");
        let sandstone = frozen
            .province_id(&Namespaced::parse("strata:sandstone").expect("valid"))
            .expect("registered");
        assert_eq!((granite.0, sandstone.0), (0, 1), "sorted by id");
        let traits = frozen.province(sandstone).expect("resolved");
        assert_eq!(traits.in_ranges, granite);
        assert_eq!(traits.rock.face, TerrainMaterialId(1), "rock sorts second");
        assert!((traits.rock.cover - 0.5).abs() < f32::EPSILON);
        assert_eq!(
            frozen.province(granite).expect("resolved").in_ranges,
            granite,
            "none named is itself"
        );
        let stone = frozen
            .flora_id(&Namespaced::parse("strata:granite_stone").expect("valid"))
            .expect("registered");
        let fit = frozen.flora_fit(stone).expect("fit");
        assert!(fit.in_a_kit());
        assert!((fit.province_weight(granite) - 0.7).abs() < f32::EPSILON);
        assert!(fit.province_weight(sandstone).abs() < f32::EPSILON);
        assert!(fit.province_weight(crate::ProvinceId::NONE).abs() < f32::EPSILON);
        assert_eq!(frozen.province(crate::ProvinceId::NONE), None);
    }

    #[test]
    fn a_world_without_geology_has_no_provinces() {
        let frozen = load_world_from_source(&minimal()).expect("loads");
        assert!(frozen.provinces.is_empty());
        assert_eq!(frozen.province_ids().count(), 0);
    }

    #[test]
    fn province_patches_run_in_three_rounds() {
        let source = with_geology()
            .with(
                "geology/patches/base/soften.ron",
                r#"[(target: "strata:granite", op: Merge({"rock": {"cover": 0.25}}))]"#,
            )
            .with(
                "geology/patches/updates/rename.ron",
                r#"[(target: "strata:sandstone", op: Merge({"name": "Redrock"}))]"#,
            )
            .with(
                "geology/patches/final_fixes/drop.ron",
                r#"[(target: "strata:sandstone", op: Merge({"in_ranges": None}))]"#,
            );
        let frozen = load_world_from_source(&source).expect("loads");
        let granite = frozen.provinces.get(crate::ProvinceId(0)).expect("there");
        assert!((granite.rock.cover - 0.25).abs() < f32::EPSILON);
        let sandstone = frozen.provinces.get(crate::ProvinceId(1)).expect("there");
        assert_eq!(sandstone.name, "Redrock");
        assert_eq!(sandstone.in_ranges, None);
        assert_eq!(
            frozen
                .province(crate::ProvinceId(1))
                .expect("resolved")
                .in_ranges,
            crate::ProvinceId(1)
        );
    }

    /// A wrong province file is refused at load, with a message naming the
    /// file and the field, for every kind of wrong: a name nobody
    /// registered, a number out of bounds, a string that is not of its
    /// kind, a field that is missing, misspelt or of the wrong type.
    #[test]
    fn a_wrong_province_names_its_file_and_field() {
        let cases: [(&str, &str, &str); 14] = [
            (
                r#"in_ranges: Some("strata:gneiss"),"#,
                "in_ranges",
                "gneiss",
            ),
            (
                r#"kit: [(species: "strata:oak", weight: 0.5)],"#,
                "kit[0].species",
                "oak",
            ),
            (
                r#"kit: [(species: "strata:meadow", weight: 0.5)],"#,
                "kit[0].species",
                "kind rock",
            ),
            ("frequency: -1.0,", "frequency", "over 0"),
            // 0.1 of 2.1: the field could deal it no cell.
            ("frequency: 0.1,", "frequency", "sixteenth"),
            (
                r#"relief: [(class: "volcano", shape: "fbm")],"#,
                "relief[0].class",
                "volcano",
            ),
            (
                r#"relief: [(class: "hills", shape: "perlin")],"#,
                "relief[0].shape",
                "perlin",
            ),
            ("beds: (strength: 2.0),", "beds.strength", "0..=1"),
            (r#"water: (section: "canyon"),"#, "water.section", "canyon"),
            ("water: (hang: 0.0),", "water.hang", "(0, 1]"),
            ("colour: (1.0, 0.0, 0.0),", "colour", ""),
            ("beds: (strenght: 0.5),", "strenght", ""),
            (
                r#"relief: [(class: "hills", shape: "fbm", damping: "lots")],"#,
                "`relief[0].damping`",
                "string",
            ),
            (r#"frequency: "often","#, "`frequency`", "string"),
        ];
        for (extra, field, detail) in cases {
            let source = with_geology()
                .with(
                    "flora/meadow.flora.ron",
                    r#"(id: "strata:meadow", name: "Meadow", kind: "grass",
                        biomes: [(biome: "strata:plains", weight: 0.9)],
                        density_per_m2: 6.0,
                        tint_dry: (0.5, 0.5, 0.2), tint_lush: (0.2, 0.5, 0.1))"#,
                )
                .with("geology/wrong.geology.ron", province("strata:wrong", extra));
            let err = load_world_from_source(&source).expect_err(extra);
            let text = err.to_string();
            assert!(
                text.contains("geology/wrong.geology.ron"),
                "{extra}: {text}"
            );
            assert!(text.contains(field), "{extra}: `{field}` not in {text}");
            assert!(text.contains(detail), "{extra}: `{detail}` not in {text}");
        }

        let source = with_geology().with(
            "geology/faceless.geology.ron",
            province("strata:faceless", "").replace("strata:rock", "strata:marble"),
        );
        let err = load_world_from_source(&source).expect_err("marble is not a material");
        let text = err.to_string();
        assert!(
            text.contains("geology/faceless.geology.ron") && text.contains("rock.face"),
            "{text}"
        );
        assert!(text.contains("marble"), "{text}");

        let source = with_geology().with(
            "geology/cover.geology.ron",
            province("strata:cover", "").replace("cover: 0.5", "cover: true"),
        );
        let err = load_world_from_source(&source).expect_err("a boolean cover");
        let text = err.to_string();
        assert!(
            text.contains("geology/cover.geology.ron") && text.contains("`rock.cover`"),
            "{text}"
        );

        let source = with_geology().with(
            "geology/bare.geology.ron",
            r#"(id: "strata:bare", name: "Bare", tint: (0.5, 0.5, 0.5))"#,
        );
        let err = load_world_from_source(&source).expect_err("no rock");
        let text = err.to_string();
        assert!(
            text.contains("geology/bare.geology.ron") && text.contains("rock"),
            "{text}"
        );

        let source =
            with_geology().with("geology/again.geology.ron", province("strata:granite", ""));
        let err = load_world_from_source(&source).expect_err("two granites");
        assert!(matches!(err, LoadError::Duplicate { .. }), "{err}");
    }

    /// A frequency at a sixteenth of the whole is enough for the field to
    /// deal it a cell of every block, and is taken.
    #[test]
    fn a_sixteenth_of_the_frequencies_is_enough() {
        // 0.2 of 3.2 is a sixteenth exactly.
        let source = with_geology().with(
            "geology/rare.geology.ron",
            province("strata:rare", "frequency: 0.2,"),
        );
        let frozen = load_world_from_source(&source).expect("a sixteenth loads");
        assert_eq!(frozen.provinces.len(), 3);
    }

    /// A province's face must be one of the layers the terrain draws: its
    /// mesher packs a pair of faces into four bits each, and a face past
    /// the sixteenth layer drew as another rock with no error.
    #[test]
    fn a_face_past_the_drawn_layers_is_refused() {
        let mut materials = String::from("[\n");
        for k in 0..17 {
            writeln!(
                materials,
                r#"(id: "strata:a{k:02}", texture_set: "a{k:02}", tiling_m: 4.0),"#
            )
            .expect("a String takes it");
        }
        materials.push_str(
            "(id: \"strata:grass\", texture_set: \"grass\", tiling_m: 4.0),\n\
             (id: \"strata:rock\", texture_set: \"rock\", tiling_m: 6.0),\n]",
        );
        let source = |face: &str| {
            InMemorySource::new()
                .with("biomes/plains.biome.ron", plains("strata:grass"))
                .with("terrain/materials.ron", materials.clone())
                .with(
                    "geology/deep.geology.ron",
                    province("strata:deep", "").replace("strata:rock", face),
                )
        };
        // Sorted, a00 to a15 are layers 0 to 15 and a16 is layer 16.
        load_world_from_source(&source("strata:a15")).expect("layer 15 is drawn");
        let err = load_world_from_source(&source("strata:a16")).expect_err("layer 16 is not");
        let text = err.to_string();
        assert!(
            text.contains("geology/deep.geology.ron")
                && text.contains("rock.face")
                && text.contains("layer 16"),
            "{text}"
        );
    }

    #[test]
    fn at_most_sixteen_provinces() {
        let mut source = with_geology();
        for k in 0..15 {
            source = source.with(
                format!("geology/p{k:02}.geology.ron"),
                province(&format!("strata:p{k:02}"), ""),
            );
        }
        let err = load_world_from_source(&source).expect_err("seventeen");
        assert!(
            matches!(
                err,
                LoadError::Freeze(FreezeError::TooManyProvinces { count: 17, .. })
            ),
            "{err}"
        );
    }

    fn site_kind(id: &str, rank: u32, wants: &str) -> String {
        format!(
            r#"(
                id: "{id}", name: "A kind", rank: {rank}, per_1000_km2: 10.0,
                footprint_m: 30.0, blend_m: 20.0, max_cut_m: 4.0,
                spacing: (own_m: 2000.0, others_m: 800.0),
                ground: (slope_max: 0.2, height_m: (min: 2.0, max: 500.0),
                         biomes: [(id: "strata:plains", weight: 0.5)]),
                wants: [{wants}],
                facing: "water", road: "track", group: "veyra:test",
                names: (onsets: ["b", "d"], vowels: ["a"], codas: ["", "n"],
                        syllables: [2, 2], patterns: ["{{root}} Ford"]),
            )"#
        )
    }

    /// Site kinds load, patch and freeze: their strings parsed, their
    /// biomes resolved, a kind a term names resolved to its id and placed
    /// first; a set without the directory has none.
    #[test]
    fn site_kinds_load_patch_and_resolve() {
        assert!(
            load_world_from_source(&minimal())
                .expect("loads")
                .sites
                .is_empty()
        );
        let source = minimal()
            .with("sites/town.site.ron", site_kind("strata:town", 0, ""))
            .with(
                "sites/hamlet.site.ron",
                site_kind(
                    "strata:hamlet",
                    1,
                    r#"(any: [(feature: "kind", kind: "strata:town", within_m: 5000.0),
                              (feature: "view", at_least: 0.2, full: 0.8)], required: true)"#,
                ),
            )
            .with(
                "sites/patches/updates/wider.ron",
                r#"[(target: "strata:town", op: Merge({"footprint_m": 60.0}))]"#,
            );
        let frozen = load_world_from_source(&source).expect("loads");
        let town = frozen
            .site_kind_id(&Namespaced::parse("strata:town").expect("valid"))
            .expect("registered");
        let hamlet = frozen
            .site_kind_id(&Namespaced::parse("strata:hamlet").expect("valid"))
            .expect("registered");
        assert_eq!((hamlet.0, town.0), (0, 1), "sorted by id");
        assert!((frozen.sites.get(town).expect("there").footprint_m - 60.0).abs() < 1e-12);
        let traits = frozen.site_kind(hamlet).expect("resolved");
        assert_eq!(traits.order, 1, "placed after the town");
        assert_eq!(traits.wants[0].any[0].kind, Some(town));
        assert!(traits.wants[0].any[1].feature.is_measure());
        assert!((traits.biomes[0] - 0.5).abs() < 1e-12);
        assert_eq!(traits.facing, crate::site::Facing::Water);
    }

    /// A wrong site kind is refused at load with its file and field: a
    /// feature nobody knows, a kind placed after it, a measure without its
    /// bounds, a pad too wide for its spacing, a biome nobody registered.
    #[test]
    fn a_wrong_site_kind_names_its_file_and_field() {
        let cases = [
            (
                r#"(any: [(feature: "volcano", within_m: 10.0)])"#,
                "wants[0].any[0].feature",
            ),
            (
                r#"(any: [(feature: "kind", kind: "strata:town", within_m: 10.0)])"#,
                "wants[0].any[0].kind",
            ),
            (r#"(any: [(feature: "view")])"#, "wants[0].any[0].full"),
            (r#"(any: [(feature: "river")])"#, "wants[0].any[0].within_m"),
        ];
        for (wants, field) in cases {
            let source = minimal()
                .with("sites/town.site.ron", site_kind("strata:town", 1, ""))
                .with("sites/wrong.site.ron", site_kind("strata:wrong", 0, wants));
            let text = load_world_from_source(&source)
                .expect_err(wants)
                .to_string();
            assert!(
                text.contains("sites/wrong.site.ron") && text.contains(field),
                "{wants}: {text}"
            );
        }
        let source = minimal().with(
            "sites/wide.site.ron",
            site_kind("strata:wide", 0, "").replace("own_m: 2000.0", "own_m: 60.0"),
        );
        let text = load_world_from_source(&source)
            .expect_err("too close")
            .to_string();
        assert!(text.contains("spacing.own_m"), "{text}");
        let source = minimal().with(
            "sites/swamp.site.ron",
            site_kind("strata:swamp", 0, "").replace("strata:plains", "strata:swamp"),
        );
        let text = load_world_from_source(&source)
            .expect_err("no swamp")
            .to_string();
        assert!(text.contains("ground.biomes[0].id"), "{text}");
        // A band narrower than the developed skirt and its fade.
        let source = minimal().with(
            "sites/thin.site.ron",
            site_kind("strata:thin", 0, "").replace("blend_m: 20.0", "blend_m: 10.0"),
        );
        let text = load_world_from_source(&source)
            .expect_err("too thin")
            .to_string();
        assert!(text.contains("blend_m"), "{text}");
        // Syllables that can only make a root of two letters, or of over
        // nine: the bake would draw names forever.
        for (parts, syllables) in [
            ((r#"codas: ["", "n"]"#, r#"codas: [""]"#), "[1, 1]"),
            ((r#"vowels: ["a"]"#, r#"vowels: ["aeio"]"#), "[4, 4]"),
        ] {
            let kind = site_kind("strata:mute", 0, "");
            assert!(kind.contains(parts.0), "{}", parts.0);
            let kind = kind
                .replace(parts.0, parts.1)
                .replace("syllables: [2, 2]", &format!("syllables: {syllables}"));
            let source = minimal().with("sites/mute.site.ron", kind);
            let text = load_world_from_source(&source)
                .expect_err("no name")
                .to_string();
            assert!(
                text.contains("sites/mute.site.ron") && text.contains("names"),
                "{text}"
            );
        }
    }

    #[test]
    fn a_missing_material_file_is_an_empty_list() {
        let source = InMemorySource::new().with("biomes/plains.biome.ron", plains("strata:grass"));
        let err = load_world_from_source(&source).expect_err("grass is not registered");
        assert!(
            matches!(err, LoadError::Freeze(FreezeError::UnknownMaterial { .. })),
            "{err}"
        );
    }
}
