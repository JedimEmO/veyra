//! Small cached studio palettes made with ashlar's procedural material graphs.
use std::{collections::BTreeMap, num::NonZeroUsize};

use anyhow::Result;
use ashlar::{Bake, MaterialDefinition, MaterialLibrary, ParamValue, Surface};
use ashlar_bevy::runtime_bake::{BakeCache, BakeContext, Baker, create_graph_material};
use ashlar_material::MaterialGraphLibrary;
use bevy::prelude::*;
use veyra_weapons::{Finish, Maker};

#[derive(Resource)]
pub(crate) struct MaterialBank {
    graphs: MaterialGraphLibrary,
    bakes: BakeCache,
    palettes: BTreeMap<(Maker, Finish, bool), BTreeMap<String, Handle<StandardMaterial>>>,
}

impl Default for MaterialBank {
    fn default() -> Self {
        Self {
            graphs: crate::surfaces::graphs(),
            bakes: BakeCache::default(),
            palettes: BTreeMap::new(),
        }
    }
}

fn linear(color: [f32; 3]) -> ParamValue {
    ParamValue::Color(color.map(|c| {
        if c <= 0.04045 {
            c / 12.92
        } else {
            ((c + 0.055) / 1.055).powf(2.4)
        }
    }))
}

pub(crate) fn definitions(maker: Maker, finish: Finish) -> MaterialLibrary {
    let (shell, accent) = match maker {
        Maker::Kestrel => ([0.48, 0.53, 0.51], [0.67, 0.43, 0.17]),
        Maker::Morrow => ([0.20, 0.28, 0.31], [0.37, 0.60, 0.59]),
        Maker::Bracken => ([0.38, 0.25, 0.18], [0.66, 0.56, 0.35]),
        Maker::Vigil => ([0.17, 0.195, 0.205], [0.34, 0.37, 0.26]),
    };
    let wear = if finish == Finish::Field { 0.68 } else { 0.065 };
    let graph = |key: &str, color: [f32; 3], coating: bool| {
        let mut params = BTreeMap::from([("color".into(), linear(color))]);
        if coating {
            params.insert("wear".into(), ParamValue::Float(wear));
            params.insert(
                "roughness".into(),
                ParamValue::Float(if maker == Maker::Vigil { 0.58 } else { 0.46 }),
            );
        }
        MaterialDefinition {
            base_color: [1.0; 3],
            roughness: 1.0,
            metallic: 1.0,
            tile_metres: if key == "weapon:polymer" {
                [0.10; 2]
            } else {
                [0.16; 2]
            },
            surface: Surface::Graph(Bake {
                graph: key.into(),
                params,
                resolution: 512,
            }),
            ..Default::default()
        }
    };
    let shell = if finish == Finish::Bare {
        graph("weapon:machined-metal", [0.57, 0.59, 0.61], false)
    } else {
        graph("weapon:coating", shell, true)
    };
    let metal_color = if maker == Maker::Vigil {
        [0.34, 0.37, 0.40]
    } else {
        [0.48, 0.51, 0.54]
    };
    let ceramic = if maker == Maker::Vigil {
        [0.38, 0.40, 0.34]
    } else {
        [0.63, 0.65, 0.61]
    };
    let plain = |base_color, roughness, metallic, emissive| MaterialDefinition {
        base_color,
        roughness,
        metallic,
        emissive,
        ..Default::default()
    };
    MaterialLibrary {
        materials: BTreeMap::from([
            ("weapon:shell".into(), shell),
            (
                "weapon:metal".into(),
                graph("weapon:machined-metal", metal_color, false),
            ),
            (
                "weapon:accent".into(),
                graph("weapon:coating", accent, true),
            ),
            (
                "weapon:grip".into(),
                graph("weapon:polymer", [0.08, 0.09, 0.10], false),
            ),
            ("weapon:ceramic".into(), plain(ceramic, 0.5, 0.0, [0.0; 3])),
            (
                "weapon:lens".into(),
                plain([0.025, 0.15, 0.16], 0.08, 0.6, [0.0; 3]),
            ),
            (
                "weapon:light".into(),
                plain([0.15, 0.50, 0.51], 0.3, 0.0, [0.1, 1.0, 1.1]),
            ),
        ]),
    }
}

pub(crate) fn palette(
    world: &mut World,
    maker: Maker,
    finish: Finish,
    clay: bool,
) -> Result<BTreeMap<String, Handle<StandardMaterial>>> {
    let server = world.resource::<AssetServer>().clone();
    world.resource_scope(|world, mut bank: Mut<MaterialBank>| {
        if let Some(palette) = bank.palettes.get(&(maker, finish, clay)) {
            return Ok(palette.clone());
        }
        let definitions = definitions(maker, finish);
        let mut palette = BTreeMap::new();
        world.resource_scope(|world, mut images: Mut<Assets<Image>>| -> Result<()> {
            let mut materials = world.resource_mut::<Assets<StandardMaterial>>();
            let MaterialBank { graphs, bakes, .. } = &mut *bank;
            let mut context = BakeContext {
                graphs,
                cache: bakes,
                images: &mut images,
                threads: NonZeroUsize::new(2),
                baker: Baker::Cpu,
            };
            for (key, definition) in &definitions.materials {
                let material = if clay {
                    StandardMaterial {
                        base_color: Color::srgb(0.57, 0.60, 0.60),
                        perceptual_roughness: 0.65,
                        ..default()
                    }
                } else {
                    create_graph_material(definition, &server, &mut context)?
                };
                // Exports reuse the bake after GPU upload without evaluating the graph again.
                for texture in [
                    &material.base_color_texture,
                    &material.normal_map_texture,
                    &material.metallic_roughness_texture,
                    &material.occlusion_texture,
                    &material.emissive_texture,
                ]
                .into_iter()
                .flatten()
                {
                    if let Some(mut image) = context.images.get_mut(texture) {
                        image.asset_usage |= bevy::asset::RenderAssetUsages::MAIN_WORLD;
                    }
                }
                palette.insert(key.clone(), materials.add(material));
            }
            Ok(())
        })?;
        bank.palettes.insert((maker, finish, clay), palette.clone());
        Ok(palette)
    })
}
