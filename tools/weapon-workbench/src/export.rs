//! Portable meshes and the actual cached procedural maps used by the studio.
use std::{collections::BTreeMap, fmt::Write as _, path::Path};

use anyhow::{Context, Result, ensure};
use ashlar_manifold::{ManifoldMesher, mesh_building};
use bevy::{prelude::*, render::render_resource::TextureFormat};
use veyra_weapons::Assembly;

use crate::materials;

fn write_file(path: &Path, text: &str) -> Result<()> {
    let pending = path.with_extension("tmp");
    std::fs::write(&pending, text).with_context(|| format!("writing {}", pending.display()))?;
    std::fs::rename(&pending, path).with_context(|| format!("replacing {}", path.display()))?;
    Ok(())
}

/// Export texture bytes without applying a display transfer to data maps.
/// Flip image rows because OBJ readers conventionally use bottom-left UV origins.
fn texture(
    images: &Assets<Image>,
    handle: &Handle<Image>,
    path: &Path,
    channel: Option<usize>,
    flip_normal_y: bool,
) -> Result<()> {
    let mut source = images
        .get(handle)
        .context("export map is not retained in CPU image assets")?
        .clone();
    // Bevy's dynamic conversion omits the linear RGBA8 case. Relabeling lets
    // the PNG encoder preserve these exact bytes; it does not gamma-convert them.
    if source.texture_descriptor.format == TextureFormat::Rgba8Unorm {
        source.texture_descriptor.format = TextureFormat::Rgba8UnormSrgb;
    }
    if source.texture_descriptor.format == TextureFormat::Rgba8UnormSrgb {
        let top_mip_bytes = usize::try_from(source.width())?
            .checked_mul(usize::try_from(source.height())?)
            .and_then(|pixels| pixels.checked_mul(4))
            .context("export texture size overflow")?;
        let bytes = source
            .data
            .as_mut()
            .context("export texture has no CPU bytes")?;
        ensure!(
            bytes.len() >= top_mip_bytes,
            "export texture data is truncated"
        );
        bytes.truncate(top_mip_bytes);
    }
    let mut pixels = source
        .try_into_dynamic()
        .context("decoding cached texture")?
        .to_rgba8();
    let (width, height) = pixels.dimensions();
    for pixel in pixels.pixels_mut() {
        if let Some(channel) = channel {
            let value = pixel.0[channel];
            pixel.0 = [value, value, value, 255];
        } else if flip_normal_y {
            pixel.0[1] = 255 - pixel.0[1];
        }
    }
    for y in 0..height / 2 {
        for x in 0..width {
            let top = *pixels.get_pixel(x, y);
            let bottom = *pixels.get_pixel(x, height - y - 1);
            pixels.put_pixel(x, y, bottom);
            pixels.put_pixel(x, height - y - 1, top);
        }
    }
    pixels
        .save(path)
        .with_context(|| format!("saving {}", path.display()))?;
    Ok(())
}

fn material(
    key: &str,
    material: &StandardMaterial,
    images: &Assets<Image>,
    directory: &Path,
    output: &mut String,
) -> Result<()> {
    let name = key.replace(':', "_");
    let color = material.base_color.to_srgba();
    let emissive = material.emissive;
    writeln!(
        output,
        "newmtl {name}\nKd {} {} {}\nd {}\nPr {}\nPm {}\nKe {} {} {}",
        color.red,
        color.green,
        color.blue,
        color.alpha,
        material.perceptual_roughness,
        material.metallic,
        emissive.red,
        emissive.green,
        emissive.blue
    )?;
    if let Some(handle) = &material.base_color_texture {
        let file = format!("{name}-base-color.png");
        texture(images, handle, &directory.join(&file), None, false)?;
        writeln!(output, "map_Kd {file}")?;
    }
    if let Some(handle) = &material.normal_map_texture {
        let file = format!("{name}-normal.png");
        texture(
            images,
            handle,
            &directory.join(&file),
            None,
            material.flip_normal_map_y,
        )?;
        // `norm` is the PBR MTL normal-map extension; map_Bump historically
        // means scalar height and would misinterpret an RGB tangent normal.
        writeln!(output, "norm {file}")?;
    }
    if let Some(handle) = &material.metallic_roughness_texture {
        let packed = format!("{name}-orm.png");
        texture(images, handle, &directory.join(&packed), None, false)?;
        writeln!(
            output,
            "# Packed map {packed}: R occlusion, G roughness, B metallic"
        )?;
        for (channel, suffix, directive) in [(1, "roughness", "map_Pr"), (2, "metallic", "map_Pm")]
        {
            let file = format!("{name}-{suffix}.png");
            texture(images, handle, &directory.join(&file), Some(channel), false)?;
            writeln!(output, "{directive} {file}")?;
        }
    }
    if let Some(handle) = &material.occlusion_texture {
        let file = format!("{name}-occlusion.png");
        texture(images, handle, &directory.join(&file), Some(0), false)?;
        writeln!(output, "# Ambient occlusion: {file} (linear scalar data)")?;
    }
    writeln!(output)?;
    Ok(())
}

/// Keep rendering and export on the same cached material definitions and bakes.
#[allow(
    clippy::cast_possible_truncation,
    reason = "the rendered UV transform and exported texture coordinates use f32"
)]
pub(crate) fn export(world: &mut World, assembly: &Assembly, directory: &Path) -> Result<()> {
    assembly.validate()?;
    std::fs::create_dir_all(directory)?;
    let handles = materials::palette(world, assembly.weapon.maker(), assembly.finish, false)?;
    let materials: BTreeMap<_, _> = handles
        .into_iter()
        .map(|(key, handle)| {
            let material = world
                .resource::<Assets<StandardMaterial>>()
                .get(&handle)
                .with_context(|| format!("missing exported material {key}"))?
                .clone();
            Ok((key, material))
        })
        .collect::<Result<_>>()?;
    let building = veyra_weapons::build(assembly)?;
    let mesh = mesh_building(&building, &ManifoldMesher::default())?;
    let mut obj = String::from(
        "# Veyra procedural weapon, metres, Y up, +Z forward\n# Material UV transforms are baked into vt; PNG rows are flipped for OBJ.\nmtllib assembly.mtl\n",
    );
    let mut offset = 1_u32;
    for (index, piece) in mesh.pieces().enumerate() {
        let material = materials
            .get(&piece.binding.material)
            .context("missing material for mesh piece")?;
        ensure!(
            piece.mesh.positions.len() == piece.mesh.uvs.len()
                && piece.mesh.positions.len() == piece.mesh.normals.len(),
            "OBJ needs one normal and UV per mesh vertex"
        );
        let component = match piece.origin {
            ashlar::PieceOrigin::Element { instance, .. } => instance,
            _ => "assembly",
        };
        writeln!(
            obj,
            "o {component}_{index}\nusemtl {}",
            piece.binding.material.replace(':', "_")
        )?;
        for point in &piece.mesh.positions {
            let p = piece.pose.transform_point(*point);
            writeln!(obj, "v {} {} {}", p.x, p.y, p.z)?;
        }
        for uv in &piece.mesh.uvs {
            let uv = material
                .uv_transform
                .transform_point2(Vec2::new(uv[0] as f32, uv[1] as f32));
            writeln!(obj, "vt {} {}", uv.x, uv.y)?;
        }
        for normal in &piece.mesh.normals {
            let n = piece.pose.rotation * *normal;
            writeln!(obj, "vn {} {} {}", n.x, n.y, n.z)?;
        }
        for tri in piece.mesh.indices.as_chunks::<3>().0 {
            let [a, b, c] = [tri[0] + offset, tri[1] + offset, tri[2] + offset];
            writeln!(obj, "f {a}/{a}/{a} {b}/{b}/{b} {c}/{c}/{c}")?;
        }
        offset = offset
            .checked_add(u32::try_from(piece.mesh.positions.len())?)
            .context("OBJ vertex index overflow")?;
    }
    let mut mtl = String::from(
        "# PBR MTL extensions: Pr, Pm, norm, map_Pr, map_Pm.\n# PNGs use OBJ bottom-left UV orientation. Base color is sRGB; other maps are linear data.\n",
    );
    for (key, definition) in &materials {
        material(
            key,
            definition,
            world.resource::<Assets<Image>>(),
            directory,
            &mut mtl,
        )?;
    }
    write_file(
        &directory.join("assembly.ron"),
        &ron::ser::to_string_pretty(assembly, ron::ser::PrettyConfig::default())?,
    )?;
    write_file(&directory.join("assembly.obj"), &obj)?;
    write_file(&directory.join("assembly.mtl"), &mtl)?;
    write_file(
        &directory.join("assembly.recipe.ron"),
        &ron::ser::to_string_pretty(building.recipe(), ron::ser::PrettyConfig::default())?,
    )?;
    let library = materials::definitions(assembly.weapon.maker(), assembly.finish);
    write_file(
        &directory.join("assembly.materials.ron"),
        &ron::ser::to_string_pretty(&library, ron::ser::PrettyConfig::default())?,
    )?;
    write_file(
        &directory.join("assembly.graphs.ron"),
        &ron::ser::to_string_pretty(
            &crate::surfaces::graphs(),
            ron::ser::PrettyConfig::default(),
        )?,
    )?;
    Ok(())
}
