//! The open registry set the data stage fills and the frozen set the game
//! reads.
//!
//! Both sides are `slotted-registry` builders and registries; this module
//! adds what is specific to biomes and materials: the cross-references are
//! resolved once, at freeze, into dense ids, so the sampler never touches a
//! name on the compute pool. Ids are assigned in sorted-id order rather than
//! file order so the same asset set gives the same ids however the directory
//! lists, which is what makes the material layer index safe to bake into a
//! mesh.

use slotted_model::Namespaced;
use slotted_registry::registry::{Registry, RegistryBuilder, RegistryError, RegistryKind};

use crate::biome::{BiomeDef, BiomeId};
use crate::flora::{
    FloraDef, FloraId, FloraKind, MAX_GRASS_CANDIDATES_PER_CELL, MAX_GRASS_SPECIES,
    MAX_PLANT_CANDIDATES_PER_CELL,
};
use crate::geology::{MAX_PROVINCES, ProvinceDef, ProvinceId, ProvinceRock, ProvinceTraits};
use crate::material::{
    MAX_DRAWN_LAYERS, MAX_TERRAIN_MATERIALS, TerrainMaterialDef, TerrainMaterialId,
};
use crate::site::{SiteKindDef, SiteKindId, SiteKindTraits, SiteWant};

/// Why a registry set could not be frozen.
#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum FreezeError {
    /// A biome's numbers do not describe a usable climate window or relief.
    #[error("biome `{id}`: {reason}")]
    InvalidBiome {
        /// The biome at fault.
        id: Namespaced,
        /// What is wrong with it.
        reason: String,
    },
    /// A material's numbers or texture set name are unusable.
    #[error("terrain material `{id}`: {reason}")]
    InvalidMaterial {
        /// The material at fault.
        id: Namespaced,
        /// What is wrong with it.
        reason: String,
    },
    /// A biome's surface named a material nobody registered. Fatal, unlike a
    /// missing tag member in slotted: the terrain has nothing to draw.
    #[error("biome `{biome}` uses terrain material `{material}`, which is not registered")]
    UnknownMaterial {
        /// The biome holding the reference.
        biome: Namespaced,
        /// The material it named.
        material: Namespaced,
    },
    /// More materials than the `u8` layer index can address.
    #[error("{count} terrain materials registered; the layer index allows at most {max}")]
    TooManyMaterials {
        /// How many were registered.
        count: usize,
        /// The limit, [`MAX_TERRAIN_MATERIALS`].
        max: usize,
    },
    /// Nothing to sample. The classifier keeps the two best biomes per
    /// point, so a world needs at least one.
    #[error("no biomes registered; the world needs at least one")]
    NoBiomes,
    /// A species' numbers are unusable.
    #[error("flora `{id}`: {reason}")]
    InvalidFlora {
        /// The species at fault.
        id: Namespaced,
        /// What is wrong with it.
        reason: String,
    },
    /// The complete flora set exceeds a bounded placement or rendering budget.
    #[error("flora {budget} budget exceeded: {count} registered, maximum {max}")]
    FloraBudget {
        /// Which resource is exhausted.
        budget: &'static str,
        /// Registered demand.
        count: usize,
        /// Supported demand.
        max: usize,
    },
    /// A species grows in a biome nobody registered.
    #[error("flora `{flora}` grows in biome `{biome}`, which is not registered")]
    UnknownBiome {
        /// The species holding the reference.
        flora: Namespaced,
        /// The biome it named.
        biome: Namespaced,
    },
    /// A province's file is wrong: a number out of bounds, a string that is
    /// not one of its kind, or a name nobody registered. Names the file and
    /// the field, because the first thing anyone needs is which.
    #[error("`{file}` (province `{id}`): `{field}` {reason}")]
    InvalidProvince {
        /// The file the province came from, or empty for one registered by
        /// code.
        file: String,
        /// The province at fault.
        id: Namespaced,
        /// The field's path in the file, `rock.face` or `relief[1].shape`.
        field: String,
        /// What is wrong with it.
        reason: String,
    },
    /// More provinces than the field can deal into one block.
    #[error("{count} provinces registered; the province field allows at most {max}")]
    TooManyProvinces {
        /// How many were registered.
        count: usize,
        /// The limit, [`MAX_PROVINCES`].
        max: usize,
    },
    /// A species brings a child species nobody registered, or grass, which
    /// is not placed one at a time.
    #[error("flora `{flora}` brings child `{child}`, which is not a registered bush, tree or rock")]
    UnknownChild {
        /// The species holding the reference.
        flora: Namespaced,
        /// The child it named.
        child: Namespaced,
    },
    /// A site kind's file is wrong: a number out of bounds, a string that
    /// is not one of its kind, or a name nobody registered. Names the file
    /// and the field, as a province's error does.
    #[error("`{file}` (site kind `{id}`): `{field}` {reason}")]
    InvalidSiteKind {
        /// The file the kind came from, or empty for one registered by code.
        file: String,
        /// The kind at fault.
        id: Namespaced,
        /// The field's path in the file, `wants[1].any[0].feature`.
        field: String,
        /// What is wrong with it.
        reason: String,
    },
}

/// Where a frozen species grows, resolved to ids: per biome, the weight.
/// In biome id order, dense, zero where the species is not listed, so
/// placement indexes it by the sample's biome ids without a lookup.
///
/// Per province too, from the provinces' kits: dense in province id order,
/// zero where no province names the species, and empty when none does
/// (every tree and grass, and a rock no kit lists), so placement can leave
/// such a species alone. Resolved at freeze since ticket 0004's step 3 and
/// read by placement from its step 3e.
#[derive(Debug, Clone, PartialEq)]
pub struct FloraFit {
    weights: Vec<f32>,
    provinces: Vec<f32>,
}

impl FloraFit {
    /// A fit from a weight per biome in id order, for a table built by
    /// hand in a test. No province names it.
    #[must_use]
    pub fn from_weights(weights: Vec<f32>) -> Self {
        Self {
            weights,
            provinces: Vec::new(),
        }
    }

    /// The species' weight in `biome`, zero for a biome it does not name.
    #[must_use]
    pub fn weight(&self, biome: BiomeId) -> f32 {
        self.weights.get(biome.0 as usize).copied().unwrap_or(0.0)
    }

    /// Whether some province's kit names the species. Where none does, the
    /// province does not choose it and [`province_weight`] is not read.
    ///
    /// [`province_weight`]: Self::province_weight
    #[must_use]
    pub fn in_a_kit(&self) -> bool {
        !self.provinces.is_empty()
    }

    /// The weight `province`'s kit gives the species, zero where its kit
    /// does not name it (and for every province when no kit does).
    #[must_use]
    pub fn province_weight(&self, province: ProvinceId) -> f32 {
        self.provinces
            .get(province.0 as usize)
            .copied()
            .unwrap_or(0.0)
    }
}

/// The materials a frozen biome paints, resolved to ids.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BiomeSurface {
    /// The material on flat ground.
    pub material: TerrainMaterialId,
    /// The material on ground steeper than `slope_threshold`.
    pub slope_material: TerrainMaterialId,
    /// Rise over run at which `slope_material` takes over.
    pub slope_threshold: f32,
}

/// The registry set while the data stage is open.
#[derive(Debug, Clone, PartialEq)]
pub struct OpenRegistries {
    /// Biomes by id. Ids are provisional until [`freeze`](Self::freeze).
    pub biomes: RegistryBuilder<BiomeDef, BiomeId>,
    /// Terrain materials by id. Ids are provisional until
    /// [`freeze`](Self::freeze).
    pub materials: RegistryBuilder<TerrainMaterialDef, TerrainMaterialId>,
    /// Flora species by id. Ids are provisional until [`freeze`](Self::freeze).
    pub flora: RegistryBuilder<FloraDef, FloraId>,
    /// Geological provinces by id. Ids are provisional until
    /// [`freeze`](Self::freeze). Empty is legal: a world without geology.
    pub provinces: RegistryBuilder<ProvinceDef, ProvinceId>,
    /// The file each province came from, for the freeze's messages.
    province_files: Vec<(Namespaced, String)>,
    /// Site kinds by id (ADR 0012). Empty is legal: a world that places no
    /// sites, the world as it was before them.
    pub sites: RegistryBuilder<SiteKindDef, SiteKindId>,
    /// The file each site kind came from, for the freeze's messages.
    site_files: Vec<(Namespaced, String)>,
}

impl Default for OpenRegistries {
    fn default() -> Self {
        // `RegistryKind` only names the registry in slotted's error messages
        // and has no biome variant; the loader maps the error to its own
        // type, so the kind chosen here is never shown.
        Self {
            biomes: RegistryBuilder::with_kind(RegistryKind::Items),
            materials: RegistryBuilder::with_kind(RegistryKind::Items),
            flora: RegistryBuilder::with_kind(RegistryKind::Items),
            provinces: RegistryBuilder::with_kind(RegistryKind::Items),
            province_files: Vec::new(),
            sites: RegistryBuilder::with_kind(RegistryKind::Items),
            site_files: Vec::new(),
        }
    }
}

impl OpenRegistries {
    /// Empty registries.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Registers a biome under its own id.
    ///
    /// # Errors
    ///
    /// [`RegistryError::Duplicate`] if the id is taken.
    pub fn add_biome(&mut self, def: BiomeDef) -> Result<BiomeId, RegistryError> {
        let name = def.id.clone();
        self.biomes.insert(name, def)
    }

    /// Registers a material under its own id.
    ///
    /// # Errors
    ///
    /// [`RegistryError::Duplicate`] if the id is taken.
    pub fn add_material(
        &mut self,
        def: TerrainMaterialDef,
    ) -> Result<TerrainMaterialId, RegistryError> {
        let name = def.id.clone();
        self.materials.insert(name, def)
    }

    /// Registers a species under its own id.
    ///
    /// # Errors
    ///
    /// [`RegistryError::Duplicate`] if the id is taken.
    pub fn add_flora(&mut self, def: FloraDef) -> Result<FloraId, RegistryError> {
        let name = def.id.clone();
        self.flora.insert(name, def)
    }

    /// Registers a province under its own id.
    ///
    /// # Errors
    ///
    /// [`RegistryError::Duplicate`] if the id is taken.
    pub fn add_province(&mut self, def: ProvinceDef) -> Result<ProvinceId, RegistryError> {
        self.add_province_from(def, "")
    }

    /// Registers a province read from `file`, which the freeze names when
    /// the province is wrong.
    ///
    /// # Errors
    ///
    /// [`RegistryError::Duplicate`] if the id is taken.
    pub fn add_province_from(
        &mut self,
        def: ProvinceDef,
        file: &str,
    ) -> Result<ProvinceId, RegistryError> {
        let name = def.id.clone();
        let id = self.provinces.insert(name.clone(), def)?;
        self.province_files.push((name, file.to_owned()));
        Ok(id)
    }

    /// Registers a site kind read from `file`, which the freeze names when
    /// the kind is wrong.
    ///
    /// # Errors
    ///
    /// [`RegistryError::Duplicate`] if the id is taken.
    pub fn add_site_kind_from(
        &mut self,
        def: SiteKindDef,
        file: &str,
    ) -> Result<SiteKindId, RegistryError> {
        let name = def.id.clone();
        let id = self.sites.insert(name.clone(), def)?;
        self.site_files.push((name, file.to_owned()));
        Ok(id)
    }

    /// Validates everything, resolves the cross-references and closes every
    /// registry. Ids are assigned in sorted-id order.
    ///
    /// # Errors
    ///
    /// [`FreezeError`] for an invalid definition, a dangling material
    /// reference, an empty biome set or too many materials.
    pub fn freeze(self) -> Result<WorldDefs, FreezeError> {
        if self.materials.len() > MAX_TERRAIN_MATERIALS {
            return Err(FreezeError::TooManyMaterials {
                count: self.materials.len(),
                max: MAX_TERRAIN_MATERIALS,
            });
        }
        if self.biomes.is_empty() {
            return Err(FreezeError::NoBiomes);
        }

        let mut materials: Vec<(Namespaced, TerrainMaterialDef)> = self
            .materials
            .iter()
            .map(|(name, def)| (name.clone(), def.clone()))
            .collect();
        materials.sort_by(|a, b| a.0.cmp(&b.0));
        for (id, def) in &materials {
            if !def.is_valid() {
                return Err(FreezeError::InvalidMaterial {
                    id: id.clone(),
                    reason: "texture_set must be a plain directory name and tiling_m positive"
                        .to_owned(),
                });
            }
        }
        let materials: Registry<TerrainMaterialDef, TerrainMaterialId> = materials
            .into_iter()
            .collect::<RegistryBuilder<_, _>>()
            .freeze();

        let mut biomes: Vec<(Namespaced, BiomeDef)> = self
            .biomes
            .iter()
            .map(|(name, def)| (name.clone(), def.clone()))
            .collect();
        biomes.sort_by(|a, b| a.0.cmp(&b.0));
        let mut surfaces = Vec::with_capacity(biomes.len());
        for (id, def) in &biomes {
            if !def.climate.is_valid() {
                return Err(FreezeError::InvalidBiome {
                    id: id.clone(),
                    reason: "every climate range must satisfy 0 <= min <= max <= 1".to_owned(),
                });
            }
            if !def.relief.is_valid() {
                return Err(FreezeError::InvalidBiome {
                    id: id.clone(),
                    reason:
                        "relief needs a non-negative amplitude, a positive wavelength, 1 to 16 \
                             octaves, persistence in (0, 1] and lacunarity >= 1"
                            .to_owned(),
                });
            }
            check_grade(id, def)?;
            if !(def.surface.slope_threshold.is_finite() && def.surface.slope_threshold > 0.0) {
                return Err(FreezeError::InvalidBiome {
                    id: id.clone(),
                    reason: "surface.slope_threshold must be positive".to_owned(),
                });
            }
            let resolve = |material: &Namespaced| {
                materials
                    .id_of(material)
                    .ok_or_else(|| FreezeError::UnknownMaterial {
                        biome: id.clone(),
                        material: material.clone(),
                    })
            };
            surfaces.push(BiomeSurface {
                material: resolve(&def.surface.material)?,
                slope_material: resolve(&def.surface.slope_material)?,
                slope_threshold: def.surface.slope_threshold,
            });
        }
        let biomes: Registry<BiomeDef, BiomeId> = biomes
            .into_iter()
            .collect::<RegistryBuilder<_, _>>()
            .freeze();

        let (flora, mut fits) = freeze_flora(self.flora, &biomes)?;
        let (provinces, traits) = freeze_provinces(
            self.provinces,
            &self.province_files,
            &materials,
            &flora,
            &mut fits,
        )?;
        let (sites, site_traits) = freeze_sites(self.sites, &self.site_files, &biomes, &provinces)?;

        Ok(WorldDefs {
            biomes,
            materials,
            flora,
            provinces,
            surfaces,
            fits,
            traits,
            sites,
            site_traits,
            // The loader's to fill once the set is frozen: the settlements,
            // the progression, the items, the resources and the schematics.
            ..WorldDefs::default()
        })
    }
}

/// A biome's colour grade in range, or the field that is not.
fn check_grade(id: &Namespaced, def: &BiomeDef) -> Result<(), FreezeError> {
    match def.grade.invalid_field() {
        None => Ok(()),
        Some((field, range)) => Err(FreezeError::InvalidBiome {
            id: id.clone(),
            reason: format!("grade.{field} must be finite and in {range}"),
        }),
    }
}

/// The flora half of the freeze: every species validated and its biomes
/// resolved into a dense fit, in sorted-id order.
fn freeze_flora(
    open: RegistryBuilder<FloraDef, FloraId>,
    biomes: &Registry<BiomeDef, BiomeId>,
) -> Result<(Registry<FloraDef, FloraId>, Vec<FloraFit>), FreezeError> {
    let mut flora: Vec<(Namespaced, FloraDef)> = open
        .iter()
        .map(|(name, def)| (name.clone(), def.clone()))
        .collect();
    flora.sort_by(|a, b| a.0.cmp(&b.0));
    let mut fits = Vec::with_capacity(flora.len());
    for (id, def) in &flora {
        if let Some(reason) = def.invalid_reason() {
            return Err(FreezeError::InvalidFlora {
                id: id.clone(),
                reason: reason.to_owned(),
            });
        }
        let mut weights = vec![0.0; biomes.len()];
        for entry in &def.biomes {
            let biome = biomes
                .id_of(&entry.biome)
                .ok_or_else(|| FreezeError::UnknownBiome {
                    flora: id.clone(),
                    biome: entry.biome.clone(),
                })?;
            weights[biome.0 as usize] = entry.weight;
        }
        fits.push(FloraFit {
            weights,
            provinces: Vec::new(),
        });
    }
    for (id, def) in &flora {
        for child in &def.children {
            let known = flora
                .iter()
                .any(|(other, d)| *other == child.species && d.kind != FloraKind::Grass);
            if !known {
                return Err(FreezeError::UnknownChild {
                    flora: id.clone(),
                    child: child.species.clone(),
                });
            }
        }
    }
    for (budget, count, max) in [
        (
            "grass species",
            flora
                .iter()
                .filter(|(_, def)| def.kind == FloraKind::Grass)
                .count(),
            MAX_GRASS_SPECIES,
        ),
        (
            "grass candidates per cell",
            flora
                .iter()
                .filter(|(_, def)| def.kind == FloraKind::Grass)
                .fold(0usize, |total, (_, def)| {
                    total.saturating_add(def.candidate_count())
                }),
            MAX_GRASS_CANDIDATES_PER_CELL,
        ),
        (
            "plant candidates per cell",
            flora
                .iter()
                .filter(|(_, def)| def.kind != FloraKind::Grass)
                .fold(0usize, |total, (_, def)| {
                    total.saturating_add(def.candidate_count())
                }),
            MAX_PLANT_CANDIDATES_PER_CELL,
        ),
    ] {
        if count > max {
            return Err(FreezeError::FloraBudget { budget, count, max });
        }
    }
    let flora: Registry<FloraDef, FloraId> = flora
        .into_iter()
        .collect::<RegistryBuilder<_, _>>()
        .freeze();
    Ok((flora, fits))
}

/// A province's face resolved to a layer the terrain draws, after its
/// frequency is checked against the whole: the two checks that need the
/// other provinces or the materials, as `(field, reason)` on failure.
fn resolve_face(
    def: &ProvinceDef,
    frequencies: f32,
    materials: &Registry<TerrainMaterialDef, TerrainMaterialId>,
) -> Result<TerrainMaterialId, (String, String)> {
    // The field deals at most sixteen slots a block; a province under a
    // sixteenth of the whole could be dealt none and be nowhere.
    #[allow(clippy::cast_precision_loss, reason = "16 is exact")]
    if def.frequency * (MAX_PROVINCES as f32) < frequencies {
        return Err((
            "frequency".to_owned(),
            format!(
                "{} is {:.3} of the provinces' {frequencies}, under the sixteenth the field \
                 needs to deal it a cell of every block",
                def.frequency,
                def.frequency / frequencies
            ),
        ));
    }
    let face = materials.id_of(&def.rock.face).ok_or_else(|| {
        (
            "rock.face".to_owned(),
            format!(
                "names terrain material `{}`, which is not registered",
                def.rock.face
            ),
        )
    })?;
    if face.0 as usize >= MAX_DRAWN_LAYERS {
        return Err((
            "rock.face".to_owned(),
            format!(
                "names terrain material `{}`, layer {}, past the {MAX_DRAWN_LAYERS} layers the \
                 terrain draws",
                def.rock.face, face.0
            ),
        ));
    }
    Ok(face)
}

/// The geology half of the freeze: every province validated, its names
/// resolved (its face, the province it yields to in the ranges, its kit's
/// species), in sorted-id order; and every species a kit names given its
/// dense province fit.
fn freeze_provinces(
    open: RegistryBuilder<ProvinceDef, ProvinceId>,
    files: &[(Namespaced, String)],
    materials: &Registry<TerrainMaterialDef, TerrainMaterialId>,
    flora: &Registry<FloraDef, FloraId>,
    fits: &mut [FloraFit],
) -> Result<(Registry<ProvinceDef, ProvinceId>, Vec<ProvinceTraits>), FreezeError> {
    if open.len() > MAX_PROVINCES {
        return Err(FreezeError::TooManyProvinces {
            count: open.len(),
            max: MAX_PROVINCES,
        });
    }
    let mut provinces: Vec<(Namespaced, ProvinceDef)> = open
        .iter()
        .map(|(name, def)| (name.clone(), def.clone()))
        .collect();
    provinces.sort_by(|a, b| a.0.cmp(&b.0));
    let frequencies: f32 = provinces.iter().map(|(_, def)| def.frequency).sum();
    let mut traits = Vec::with_capacity(provinces.len());
    for (id, def) in &provinces {
        let invalid = |field: String, reason: String| FreezeError::InvalidProvince {
            file: files
                .iter()
                .find(|(name, _)| name == id)
                .map(|(_, file)| file.clone())
                .unwrap_or_default(),
            id: id.clone(),
            field,
            reason,
        };
        if let Some((field, reason)) = def.invalid_field() {
            return Err(invalid(field, reason));
        }
        let face = resolve_face(def, frequencies, materials)
            .map_err(|(field, why)| invalid(field, why))?;
        let in_ranges = match &def.in_ranges {
            None => ProvinceId(u32::try_from(traits.len()).expect("at most 16 provinces")),
            Some(other) => provinces
                .iter()
                .position(|(name, _)| name == other)
                .map(|index| ProvinceId(u32::try_from(index).expect("at most 16 provinces")))
                .ok_or_else(|| {
                    invalid(
                        "in_ranges".to_owned(),
                        format!("names province `{other}`, which is not registered"),
                    )
                })?,
        };
        let mut kit = Vec::with_capacity(def.kit.len());
        for (i, entry) in def.kit.iter().enumerate() {
            let species = flora
                .id_of(&entry.species)
                .filter(|&species| {
                    flora
                        .get(species)
                        .is_some_and(|species| species.kind == FloraKind::Rock)
                })
                .ok_or_else(|| {
                    invalid(
                        format!("kit[{i}].species"),
                        format!(
                            "names `{}`, which is not a registered flora species of kind rock",
                            entry.species
                        ),
                    )
                })?;
            kit.push((species, entry.weight));
        }
        traits.push(ProvinceTraits {
            rock: ProvinceRock {
                face,
                bare_slope: def.rock.bare_slope,
                far_tint: def.rock.far_tint,
                cover: def.rock.cover,
            },
            in_ranges,
            relief: def.parsed_relief(),
            section: def.water.section.parse().expect("checked by invalid_field"),
            kit,
        });
    }
    for (index, province) in traits.iter().enumerate() {
        for &(species, weight) in &province.kit {
            let fit = &mut fits[species.0 as usize];
            if fit.provinces.is_empty() {
                fit.provinces = vec![0.0; traits.len()];
            }
            fit.provinces[index] = weight;
        }
    }
    let provinces: Registry<ProvinceDef, ProvinceId> = provinces
        .into_iter()
        .collect::<RegistryBuilder<_, _>>()
        .freeze();
    Ok((provinces, traits))
}

/// The site kinds' half of the freeze: every kind validated, its strings
/// parsed and its names resolved (biomes, provinces, the kinds its terms
/// name, which must be placed before it), in sorted-id order.
fn freeze_sites(
    open: RegistryBuilder<SiteKindDef, SiteKindId>,
    files: &[(Namespaced, String)],
    biomes: &Registry<BiomeDef, BiomeId>,
    provinces: &Registry<ProvinceDef, ProvinceId>,
) -> Result<(Registry<SiteKindDef, SiteKindId>, Vec<SiteKindTraits>), FreezeError> {
    let mut kinds: Vec<(Namespaced, SiteKindDef)> = open
        .iter()
        .map(|(name, def)| (name.clone(), def.clone()))
        .collect();
    kinds.sort_by(|a, b| a.0.cmp(&b.0));
    // Placement order: by rank, ties by id (the list is in id order).
    let mut order: Vec<usize> = (0..kinds.len()).collect();
    order.sort_by_key(|&k| kinds[k].1.rank);
    let mut place = vec![0_u32; kinds.len()];
    for (at, &k) in order.iter().enumerate() {
        place[k] = u32::try_from(at).expect("a handful of kinds");
    }
    let mut traits = Vec::with_capacity(kinds.len());
    for (index, (id, def)) in kinds.iter().enumerate() {
        let invalid = |field: String, reason: String| FreezeError::InvalidSiteKind {
            file: files
                .iter()
                .find(|(name, _)| name == id)
                .map(|(_, file)| file.clone())
                .unwrap_or_default(),
            id: id.clone(),
            field,
            reason,
        };
        if let Some((field, reason)) = def.invalid_field() {
            return Err(invalid(field, reason));
        }
        let mut wants = Vec::with_capacity(def.wants.len());
        for (i, want) in def.wants.iter().enumerate() {
            let mut any = Vec::with_capacity(want.any.len());
            for (k, term) in want.any.iter().enumerate() {
                let kind = if term.kind.is_empty() {
                    None
                } else {
                    let other = Namespaced::parse(&term.kind).ok();
                    let at = other
                        .as_ref()
                        .and_then(|other| kinds.iter().position(|(name, _)| name == other));
                    let Some(at) = at.filter(|&at| place[at] < place[index]) else {
                        return Err(invalid(
                            format!("wants[{i}].any[{k}].kind"),
                            format!(
                                "names `{}`, which is not a registered kind placed before this \
                                 one (a lower rank, or the same and an earlier id)",
                                term.kind
                            ),
                        ));
                    };
                    Some(SiteKindId(u32::try_from(at).expect("a handful of kinds")))
                };
                any.push(SiteKindDef::term(term, kind));
            }
            wants.push(SiteWant {
                any,
                weight: want.weight,
                required: want.required,
            });
        }
        let mut biome_weights = vec![def.ground.biome_default; biomes.len()];
        for (i, entry) in def.ground.biomes.iter().enumerate() {
            let biome = biomes.id_of(&entry.id).ok_or_else(|| {
                invalid(
                    format!("ground.biomes[{i}].id"),
                    format!("names biome `{}`, which is not registered", entry.id),
                )
            })?;
            biome_weights[biome.0 as usize] = entry.weight;
        }
        let mut province_weights = vec![def.ground.province_default; provinces.len()];
        for (i, entry) in def.ground.provinces.iter().enumerate() {
            // A world without geology has no provinces to weigh: the list
            // is read only where there are some.
            if provinces.is_empty() {
                break;
            }
            let province = provinces.id_of(&entry.id).ok_or_else(|| {
                invalid(
                    format!("ground.provinces[{i}].id"),
                    format!("names province `{}`, which is not registered", entry.id),
                )
            })?;
            province_weights[province.0 as usize] = entry.weight;
        }
        traits.push(SiteKindTraits {
            order: place[index],
            wants,
            biomes: biome_weights,
            provinces: province_weights,
            facing: def.parsed_facing(),
            road: def.parsed_road(),
            by_feature: def.parsed_by_feature(),
        });
    }
    let kinds: Registry<SiteKindDef, SiteKindId> = kinds
        .into_iter()
        .collect::<RegistryBuilder<_, _>>()
        .freeze();
    Ok((kinds, traits))
}

/// Every registry the world reads, frozen: what the sampler, the flora
/// placement and the world's version are a function of.
///
/// Built once in the `LoadingRegistries` state and then shared read-only;
/// the world source is constructed from this and the seed. There is no
/// `&mut self` method, so a [`BiomeId`] or a material layer stays valid for
/// the life of the process. The game's own sets (items, quests and the
/// rest) are not here: they live beside it in `veyra-data`'s
/// `GameRegistries`, which derefs to this, so the world crates take no
/// game schema and an item edit leaves them alone.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct WorldDefs {
    /// Biomes. `BiomeId(n)` is the `n`-th biome in sorted-id order.
    pub biomes: Registry<BiomeDef, BiomeId>,
    /// Terrain materials. `TerrainMaterialId(n)` is the `n`-th material in
    /// sorted-id order and the `n`-th layer of the texture array.
    pub materials: Registry<TerrainMaterialDef, TerrainMaterialId>,
    /// Flora species. `FloraId(n)` is the `n`-th species in sorted-id order.
    pub flora: Registry<FloraDef, FloraId>,
    /// Geological provinces. `ProvinceId(n)` is the `n`-th province in
    /// sorted-id order. Empty for a world without geology, whose province
    /// field is off.
    pub provinces: Registry<ProvinceDef, ProvinceId>,
    /// Per biome, in id order: its surface with the material names resolved.
    surfaces: Vec<BiomeSurface>,
    /// Per species, in id order: where it grows, resolved to biome ids.
    fits: Vec<FloraFit>,
    /// Per province, in id order: its names resolved and strings parsed.
    traits: Vec<ProvinceTraits>,
    /// Site kinds (ADR 0012). `SiteKindId(n)` is the `n`-th kind in
    /// sorted-id order. Empty for a world that places no sites.
    pub sites: Registry<SiteKindDef, SiteKindId>,
    /// Per site kind, in id order: its strings parsed and names resolved.
    site_traits: Vec<SiteKindTraits>,
    /// Settlements, in id order. Not a slotted registry: a world has a
    /// handful, nothing patches them, and nothing refers to one by a dense id.
    pub settlements: Vec<crate::settlement::SettlementDef>,
    /// The value budget of `look/values.ron` (look data: in no world
    /// version). Empty for a data set that ships none, which leaves the
    /// ground as baked.
    pub values: crate::values::ValueBudget,
}

impl WorldDefs {
    /// The resolved surface of a biome, or `None` for an id from another
    /// registry set.
    #[must_use]
    pub fn surface(&self, biome: BiomeId) -> Option<&BiomeSurface> {
        self.surfaces.get(biome.0 as usize)
    }

    /// Where a species grows, or `None` for an id from another registry set.
    #[must_use]
    pub fn flora_fit(&self, flora: FloraId) -> Option<&FloraFit> {
        self.fits.get(flora.0 as usize)
    }

    /// The id of a species by name.
    #[must_use]
    pub fn flora_id(&self, name: &Namespaced) -> Option<FloraId> {
        self.flora.id_of(name)
    }

    /// Dense grass-only palette slot, independent of tree and bush IDs.
    #[must_use]
    pub fn grass_palette_index(&self, id: FloraId) -> Option<u8> {
        self.flora
            .iter()
            .filter(|(_, _, def)| def.kind == FloraKind::Grass)
            .position(|(candidate, _, _)| candidate == id)
            .and_then(|index| u8::try_from(index).ok())
    }

    /// A province with its names resolved, or `None` for an id from another
    /// registry set (and for [`ProvinceId::NONE`]).
    #[must_use]
    pub fn province(&self, province: ProvinceId) -> Option<&ProvinceTraits> {
        self.traits.get(province.0 as usize)
    }

    /// The id of a province by name.
    #[must_use]
    pub fn province_id(&self, name: &Namespaced) -> Option<ProvinceId> {
        self.provinces.id_of(name)
    }

    /// A site kind with its strings parsed and its names resolved, or `None`
    /// for an id from another registry set.
    #[must_use]
    pub fn site_kind(&self, kind: SiteKindId) -> Option<&SiteKindTraits> {
        self.site_traits.get(kind.0 as usize)
    }

    /// The id of a site kind by name.
    #[must_use]
    pub fn site_kind_id(&self, name: &Namespaced) -> Option<SiteKindId> {
        self.sites.id_of(name)
    }

    /// Every province id, in order.
    pub fn province_ids(&self) -> impl ExactSizeIterator<Item = ProvinceId> + '_ {
        self.provinces.iter().map(|(id, _, _)| id)
    }

    /// Every species id, in order.
    pub fn flora_ids(&self) -> impl ExactSizeIterator<Item = FloraId> + '_ {
        self.flora.iter().map(|(id, _, _)| id)
    }

    /// The id of a biome by name.
    #[must_use]
    pub fn biome_id(&self, name: &Namespaced) -> Option<BiomeId> {
        self.biomes.id_of(name)
    }

    /// The id of a material by name.
    #[must_use]
    pub fn material_id(&self, name: &Namespaced) -> Option<TerrainMaterialId> {
        self.materials.id_of(name)
    }

    /// Every biome id, in order, for a sampler that scores them all.
    pub fn biome_ids(&self) -> impl ExactSizeIterator<Item = BiomeId> + '_ {
        self.biomes.iter().map(|(id, _, _)| id)
    }
}

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_eq;

    use super::*;
    use crate::biome::{ClimateWindow, Relief, ReliefKind, Surface, Tint, UnitRange};

    fn id(s: &str) -> Namespaced {
        Namespaced::parse(s).expect("a valid id")
    }

    fn material(name: &str) -> TerrainMaterialDef {
        TerrainMaterialDef {
            id: id(name),
            texture_set: name.rsplit(':').next().expect("has a path").to_owned(),
            tiling_m: 4.0,
            patterned: false,
            oriented: false,
            laid: None,
            palette: false,
            grain: None,
        }
    }

    fn biome(name: &str, material: &str) -> BiomeDef {
        BiomeDef {
            id: id(name),
            name: name.to_owned(),
            climate: ClimateWindow {
                temperature: UnitRange::FULL,
                moisture: UnitRange::FULL,
                elevation: UnitRange::FULL,
            },
            relief: Relief {
                kind: ReliefKind::Plains,
                amplitude_m: 10.0,
                wavelength_m: 500.0,
                octaves: 2,
                persistence: 0.5,
                lacunarity: 2.0,
                lift_m: 0.0,
                erosion: 0.0,
            },
            surface: Surface {
                material: id(material),
                slope_material: id(material),
                slope_threshold: 0.7,
            },
            tint: Tint([0.5; 3]),
            grade: crate::biome::Grade::NEUTRAL,
            ground: None,
        }
    }

    #[test]
    fn freezing_sorts_by_id_and_resolves_surfaces() {
        let mut open = OpenRegistries::new();
        open.add_material(material("strata:rock")).expect("fresh");
        open.add_material(material("strata:grass")).expect("fresh");
        open.add_biome(biome("strata:plains", "strata:grass"))
            .expect("fresh");
        open.add_biome(biome("strata:mountains", "strata:rock"))
            .expect("fresh");
        let frozen = open.freeze().expect("consistent");

        assert_eq!(
            frozen.material_id(&id("strata:grass")),
            Some(TerrainMaterialId(0))
        );
        assert_eq!(
            frozen.material_id(&id("strata:rock")),
            Some(TerrainMaterialId(1))
        );
        assert_eq!(frozen.biome_id(&id("strata:mountains")), Some(BiomeId(0)));
        assert_eq!(frozen.biome_id(&id("strata:plains")), Some(BiomeId(1)));
        assert_eq!(
            frozen.surface(BiomeId(1)).map(|s| s.material),
            Some(TerrainMaterialId(0))
        );
        assert_eq!(frozen.surface(BiomeId(9)), None);
        assert_eq!(frozen.biome_ids().count(), 2);
    }

    #[test]
    fn a_dangling_material_is_fatal() {
        let mut open = OpenRegistries::new();
        open.add_biome(biome("strata:plains", "strata:grass"))
            .expect("fresh");
        let err = open.freeze().expect_err("grass is not registered");
        assert_eq!(
            err,
            FreezeError::UnknownMaterial {
                biome: id("strata:plains"),
                material: id("strata:grass"),
            }
        );
    }

    #[test]
    fn invalid_numbers_are_refused() {
        let mut open = OpenRegistries::new();
        open.add_material(material("strata:grass")).expect("fresh");
        let mut bad = biome("strata:plains", "strata:grass");
        bad.climate.temperature = UnitRange::new(0.9, 0.1);
        open.add_biome(bad).expect("fresh");
        assert!(matches!(
            open.freeze(),
            Err(FreezeError::InvalidBiome { .. })
        ));

        // A colour grade out of range names its field.
        let mut open = OpenRegistries::new();
        open.add_material(material("strata:grass")).expect("fresh");
        let mut bad = biome("strata:plains", "strata:grass");
        bad.grade.temperature = 0.3;
        open.add_biome(bad).expect("fresh");
        assert!(matches!(
            open.freeze(),
            Err(FreezeError::InvalidBiome { reason, .. }) if reason.contains("grade.temperature")
        ));

        let mut open = OpenRegistries::new();
        let mut bad = material("strata:grass");
        bad.tiling_m = -1.0;
        open.add_material(bad).expect("fresh");
        open.add_biome(biome("strata:plains", "strata:grass"))
            .expect("fresh");
        assert!(matches!(
            open.freeze(),
            Err(FreezeError::InvalidMaterial { .. })
        ));

        assert_eq!(OpenRegistries::new().freeze(), Err(FreezeError::NoBiomes));
    }

    #[test]
    fn flora_resolves_biomes_into_a_dense_fit() {
        use crate::flora::{BiomeWeight, FloraDef, FloraKind, MetreRange, TreeParams};
        let mut open = OpenRegistries::new();
        open.add_material(material("strata:grass")).expect("fresh");
        open.add_biome(biome("strata:plains", "strata:grass"))
            .expect("fresh");
        open.add_biome(biome("strata:forest", "strata:grass"))
            .expect("fresh");
        let oak = FloraDef {
            id: id("strata:oak"),
            name: "Oak".into(),
            kind: FloraKind::Tree,
            biomes: vec![BiomeWeight {
                biome: id("strata:forest"),
                weight: 0.8,
            }],
            spacing_m: 9.0,
            density_per_m2: 0.0,
            grass_shape: [0.04, 0.2, 0.08, 0.1],
            slope_max: 0.6,
            altitude_m: MetreRange::new(0.0, 2000.0),
            sea_clearance_m: 0.5,
            moisture: UnitRange::FULL,
            cover_floor: 0.0,
            size: MetreRange::new(0.8, 1.2),
            exclusion_m: 1.5,
            tint_dry: Tint([0.4; 3]),
            tint_lush: Tint([0.3; 3]),
            emissive: Tint([0.0; 3]),
            tree: TreeParams {
                height_m: MetreRange::new(7.0, 11.0),
                ..TreeParams::default()
            },
            ground: crate::flora::GroundPrefs::default(),
            cluster: crate::flora::Clustering::default(),
            children: Vec::new(),
            dressing: false,
        };
        let mut bad = oak.clone();
        bad.id = id("strata:elm");
        bad.biomes[0].biome = id("strata:swamp");
        open.add_flora(oak).expect("fresh");
        open.add_flora(bad).expect("fresh");
        let err = open.clone().freeze().expect_err("swamp is not a biome");
        assert!(matches!(err, FreezeError::UnknownBiome { .. }), "{err}");

        open.flora = RegistryBuilder::with_kind(RegistryKind::Items);
        let mut oak_only = open;
        let oak = FloraDef {
            id: id("strata:oak"),
            name: "Oak".into(),
            kind: FloraKind::Tree,
            biomes: vec![BiomeWeight {
                biome: id("strata:forest"),
                weight: 0.8,
            }],
            spacing_m: 9.0,
            density_per_m2: 0.0,
            grass_shape: [0.04, 0.2, 0.08, 0.1],
            slope_max: 0.6,
            altitude_m: MetreRange::new(0.0, 2000.0),
            sea_clearance_m: 0.5,
            moisture: UnitRange::FULL,
            cover_floor: 0.0,
            size: MetreRange::new(0.8, 1.2),
            exclusion_m: 1.5,
            tint_dry: Tint([0.4; 3]),
            tint_lush: Tint([0.3; 3]),
            emissive: Tint([0.0; 3]),
            tree: TreeParams {
                height_m: MetreRange::new(7.0, 11.0),
                ..TreeParams::default()
            },
            ground: crate::flora::GroundPrefs::default(),
            cluster: crate::flora::Clustering::default(),
            children: Vec::new(),
            dressing: false,
        };
        oak_only.add_flora(oak).expect("fresh");
        let frozen = oak_only.freeze().expect("consistent");
        let oak = frozen.flora_id(&id("strata:oak")).expect("registered");
        let fit = frozen.flora_fit(oak).expect("resolved");
        // forest sorts before plains
        assert!((fit.weight(BiomeId(0)) - 0.8).abs() < f32::EPSILON);
        assert!(fit.weight(BiomeId(1)).abs() < f32::EPSILON);
        assert!(fit.weight(BiomeId(7)).abs() < f32::EPSILON);
        assert_eq!(frozen.flora_ids().count(), 1);
    }

    fn flora_fixture(kind: FloraKind, name: &str) -> FloraDef {
        let text = format!(
            r#"(
            id: "{name}", name: "Test", kind: "grass",
            biomes: [(biome: "strata:plains", weight: 1.0)],
            density_per_m2: 0.01,
            tint_dry: (0.3, 0.4, 0.1), tint_lush: (0.2, 0.5, 0.1),
        )"#
        );
        let mut def: FloraDef = ron::from_str(&text).expect("fixture");
        def.kind = kind;
        if kind != FloraKind::Grass {
            def.tree.height_m = crate::flora::MetreRange::new(1.0, 2.0);
        }
        def
    }

    fn flora_registry() -> OpenRegistries {
        let mut open = OpenRegistries::new();
        open.add_material(material("strata:grass")).expect("fresh");
        open.add_biome(biome("strata:plains", "strata:grass"))
            .expect("fresh");
        open
    }

    #[test]
    fn grass_palette_ignores_non_grass_species() {
        let mut open = flora_registry();
        for index in 0..40 {
            open.add_flora(flora_fixture(
                FloraKind::Tree,
                &format!("strata:tree{index:02}"),
            ))
            .expect("fresh");
        }
        open.add_flora(flora_fixture(FloraKind::Grass, "strata:z_grass"))
            .expect("fresh");
        let frozen = open.freeze().expect("valid");
        let grass = frozen.flora_id(&id("strata:z_grass")).expect("grass");
        assert_eq!(grass.0, 40);
        assert_eq!(frozen.grass_palette_index(grass), Some(0));
        assert_eq!(frozen.grass_palette_index(FloraId(0)), None);
        assert_eq!(frozen.grass_palette_index(FloraId(999)), None);
    }

    #[test]
    fn freeze_rejects_too_many_grass_palette_entries() {
        let mut open = flora_registry();
        for index in 0..=MAX_GRASS_SPECIES {
            open.add_flora(flora_fixture(
                FloraKind::Grass,
                &format!("strata:grass{index:02}"),
            ))
            .expect("fresh");
        }
        assert!(matches!(
            open.freeze(),
            Err(FreezeError::FloraBudget {
                budget: "grass species",
                ..
            })
        ));
    }

    #[test]
    fn freeze_bounds_aggregate_work_even_when_each_species_is_valid() {
        for kind in [FloraKind::Grass, FloraKind::Tree] {
            let mut open = flora_registry();
            for index in 0..2 {
                let mut def = flora_fixture(kind, &format!("strata:species{index}"));
                def.density_per_m2 = 16.0;
                def.spacing_m = 1.0;
                assert_eq!(def.invalid_reason(), None);
                open.add_flora(def).expect("fresh");
            }
            assert!(matches!(
                open.freeze(),
                Err(FreezeError::FloraBudget { .. })
            ));
        }
    }

    #[test]
    fn pathological_density_spacing_and_exclusion_are_invalid() {
        let mut grass = flora_fixture(FloraKind::Grass, "strata:grass");
        grass.density_per_m2 = f32::MAX;
        assert!(grass.invalid_reason().is_some());
        let mut tree = flora_fixture(FloraKind::Tree, "strata:tree");
        tree.spacing_m = 0.01;
        assert!(tree.invalid_reason().is_some());
        tree.spacing_m = 9.0;
        tree.exclusion_m = 17.0;
        assert!(tree.invalid_reason().is_some());
        tree.exclusion_m = 1.0;
        for value in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY] {
            tree.sea_clearance_m = value;
            assert!(tree.invalid_reason().is_some());
        }
    }

    #[test]
    fn duplicates_are_an_error() {
        let mut open = OpenRegistries::new();
        open.add_material(material("strata:grass")).expect("fresh");
        assert!(open.add_material(material("strata:grass")).is_err());
    }
}
