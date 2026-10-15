//! The terrain's shaders and the falls' compose and validate, and fit the
//! inter-stage limit.
//!
//! Shaders load from disk at run time, so an edit that breaks one shows only
//! when the game compiles its pipelines. This composes `terrain.wgsl` and
//! `terrain_prepass.wgsl` with `naga_oil`, the preprocessor Bevy uses, over
//! `terrain_common.wgsl` and stand-ins for the parts of `bevy_pbr` they
//! import, declared as Bevy 0.19.1 declares them, and validates the result
//! with naga. `naga_oil`'s own rules apply (a field named like `_0` is
//! refused, which an earlier edit of the terrain shader ran into).

use std::collections::HashMap;

use naga_oil::compose::{
    ComposableModuleDescriptor, Composer, NagaModuleDescriptor, ShaderDefValue, ShaderLanguage,
    ShaderType,
};

const SHADERS: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../assets/shaders/");

/// The parts of `bevy_pbr` the terrain shaders use, with Bevy 0.19.1's
/// signatures (`bevy_pbr/src/render/*.wgsl`, `prepass/prepass_io.wgsl`) and
/// bodies that only have to type-check.
const STUBS: &[(&str, &str)] = &[
    (
        "bevy_pbr::mesh_view_bindings",
        "#define_import_path bevy_pbr::mesh_view_bindings
struct View {
    world_position: vec3<f32>,
    lod_view_world_position: vec3<f32>,
    clip_from_world: mat4x4<f32>,
    clip_from_view: mat4x4<f32>,
    viewport: vec4<f32>,
}
@group(0) @binding(0) var<uniform> view: View;
struct Globals {
    time: f32,
    delta_time: f32,
    frame_count: u32,
}
@group(0) @binding(11) var<uniform> globals: Globals;
",
    ),
    (
        "bevy_pbr::view_transformations",
        "#define_import_path bevy_pbr::view_transformations
#import bevy_pbr::mesh_view_bindings::view
fn position_world_to_clip(world_pos: vec3<f32>) -> vec4<f32> {
    return view.clip_from_world * vec4<f32>(world_pos, 1.0);
}
",
    ),
    (
        "bevy_pbr::mesh_functions",
        "#define_import_path bevy_pbr::mesh_functions
@group(1) @binding(0) var<storage> meshes: array<mat4x4<f32>>;
fn get_world_from_local(instance_index: u32) -> mat4x4<f32> {
    return meshes[instance_index];
}
fn get_previous_world_from_local(instance_index: u32) -> mat4x4<f32> {
    return meshes[instance_index];
}
fn mesh_position_local_to_world(world_from_local: mat4x4<f32>, vertex_position: vec4<f32>) -> vec4<f32> {
    return world_from_local * vertex_position;
}
fn mesh_normal_local_to_world(vertex_normal: vec3<f32>, instance_index: u32) -> vec3<f32> {
    return normalize((meshes[instance_index] * vec4<f32>(vertex_normal, 0.0)).xyz);
}
fn get_visibility_range_dither_level(instance_index: u32, world_position: vec4<f32>) -> i32 {
    return i32(instance_index);
}
fn get_tag(instance_index: u32) -> u32 {
    return instance_index;
}
",
    ),
    (
        "bevy_pbr::forward_io",
        "#define_import_path bevy_pbr::forward_io
struct VertexOutput {
    @builtin(position) position: vec4<f32>,
    @location(0) world_position: vec4<f32>,
    @location(1) world_normal: vec3<f32>,
#ifdef VERTEX_UVS_A
    @location(2) uv: vec2<f32>,
#endif
#ifdef VERTEX_OUTPUT_INSTANCE_INDEX
    @location(6) @interpolate(flat) instance_index: u32,
#endif
#ifdef VISIBILITY_RANGE_DITHER
    @location(7) @interpolate(flat) visibility_range_dither: i32,
#endif
}
struct FragmentOutput {
    @location(0) color: vec4<f32>,
}
",
    ),
    (
        "bevy_pbr::prepass_io",
        "#define_import_path bevy_pbr::prepass_io
struct VertexOutput {
    @builtin(position) position: vec4<f32>,
#ifdef VERTEX_UVS_A
    @location(0) uv: vec2<f32>,
#endif
#ifdef NORMAL_PREPASS_OR_DEFERRED_PREPASS
    @location(2) world_normal: vec3<f32>,
#endif
    @location(4) world_position: vec4<f32>,
#ifdef MOTION_VECTOR_PREPASS
    @location(5) previous_world_position: vec4<f32>,
#endif
#ifdef UNCLIPPED_DEPTH_ORTHO_EMULATION
    @location(6) unclipped_depth: f32,
#endif
#ifdef VERTEX_OUTPUT_INSTANCE_INDEX
    @location(7) instance_index: u32,
#endif
#ifdef VISIBILITY_RANGE_DITHER
    @location(9) @interpolate(flat) visibility_range_dither: i32,
#endif
}
",
    ),
    (
        "bevy_pbr::pbr_types",
        "#define_import_path bevy_pbr::pbr_types
struct StandardMaterial {
    base_color: vec4<f32>,
    perceptual_roughness: f32,
    metallic: f32,
    flags: u32,
}
const STANDARD_MATERIAL_FLAGS_UNLIT_BIT: u32 = 1u << 5u;
struct PbrInput {
    material: StandardMaterial,
    diffuse_occlusion: vec3<f32>,
    specular_occlusion: f32,
    world_normal: vec3<f32>,
    N: vec3<f32>,
    V: vec3<f32>,
}
",
    ),
    (
        "bevy_pbr::pbr_fragment",
        "#define_import_path bevy_pbr::pbr_fragment
#import bevy_pbr::{forward_io::VertexOutput, pbr_types::{PbrInput, StandardMaterial}}
fn pbr_input_from_standard_material(in: VertexOutput, is_front: bool) -> PbrInput {
    var out: PbrInput;
    out.material.base_color = vec4<f32>(1.0);
    out.diffuse_occlusion = vec3<f32>(1.0);
    out.specular_occlusion = 1.0;
    out.N = in.world_normal;
    out.V = normalize(in.world_position.xyz);
    return out;
}
",
    ),
    (
        "bevy_pbr::pbr_functions",
        "#define_import_path bevy_pbr::pbr_functions
#import bevy_pbr::pbr_types::{PbrInput, StandardMaterial}
fn visibility_range_dither(frag_coord: vec4<f32>, dither: i32) {
    if dither > 16 { discard; }
}
fn alpha_discard(material: StandardMaterial, output_color: vec4<f32>) -> vec4<f32> {
    return output_color;
}
fn apply_pbr_lighting(in: PbrInput) -> vec4<f32> {
    return in.material.base_color * max(dot(in.N, in.V), 0.0);
}
fn main_pass_post_lighting_processing(pbr_input: PbrInput, input_color: vec4<f32>) -> vec4<f32> {
    return input_color;
}
",
    ),
];

fn read(name: &str) -> String {
    std::fs::read_to_string(format!("{SHADERS}{name}")).expect("the shader is on disk")
}

/// What the validators may assume. The region store's tint and cover are
/// halves read by `unpack2x16float`, which naga gates behind a capability
/// that wgpu grants on every Vulkan, Metal and DX12 adapter
/// (`DownlevelFlags::SHADER_F16_IN_F32`) and Bevy passes to its composer.
fn capabilities() -> naga::valid::Capabilities {
    naga::valid::Capabilities::default() | naga::valid::Capabilities::SHADER_FLOAT16_IN_FLOAT32
}

fn composer(defs: &HashMap<String, ShaderDefValue>) -> Composer {
    let mut composer = Composer::default().with_capabilities(capabilities());
    for (path, source) in STUBS {
        if let Err(e) = composer.add_composable_module(ComposableModuleDescriptor {
            source,
            file_path: path,
            language: ShaderLanguage::Wgsl,
            shader_defs: defs.clone(),
            ..Default::default()
        }) {
            panic!("stub {path}: {}", e.emit_to_string(&composer));
        }
    }
    let common = read("terrain_common.wgsl");
    if let Err(e) = composer.add_composable_module(ComposableModuleDescriptor {
        source: &common,
        file_path: "shaders/terrain_common.wgsl",
        language: ShaderLanguage::Wgsl,
        shader_defs: defs.clone(),
        ..Default::default()
    }) {
        panic!("terrain_common.wgsl: {}", e.emit_to_string(&composer));
    }
    composer
}

/// The shader defs a main-pass or prepass pipeline of the terrain sets,
/// with and without the visibility-range dither.
fn defs(dither: bool, prepass: bool) -> HashMap<String, ShaderDefValue> {
    let mut defs: HashMap<String, ShaderDefValue> = [
        ("MATERIAL_BIND_GROUP", ShaderDefValue::UInt(3)),
        ("VERTEX_POSITIONS", ShaderDefValue::Bool(true)),
        ("VERTEX_NORMALS", ShaderDefValue::Bool(true)),
        ("VERTEX_UVS_A", ShaderDefValue::Bool(true)),
        ("VERTEX_OUTPUT_INSTANCE_INDEX", ShaderDefValue::Bool(true)),
    ]
    .into_iter()
    .map(|(k, v)| (k.to_owned(), v))
    .collect();
    if dither {
        defs.insert("VISIBILITY_RANGE_DITHER".into(), ShaderDefValue::Bool(true));
    }
    if prepass {
        for def in [
            "NORMAL_PREPASS_OR_DEFERRED_PREPASS",
            "MOTION_VECTOR_PREPASS",
            "UNCLIPPED_DEPTH_ORTHO_EMULATION",
        ] {
            defs.insert(def.into(), ShaderDefValue::Bool(true));
        }
    }
    defs
}

fn compose(name: &str, defs: &HashMap<String, ShaderDefValue>) -> naga::Module {
    let mut composer = composer(defs);
    let source = read(name);
    let module = match composer.make_naga_module(NagaModuleDescriptor {
        source: &source,
        file_path: name,
        shader_type: ShaderType::Wgsl,
        shader_defs: defs.clone(),
        ..Default::default()
    }) {
        Ok(module) => module,
        Err(e) => panic!("{name} does not compose: {}", e.emit_to_string(&composer)),
    };
    naga::valid::Validator::new(naga::valid::ValidationFlags::all(), capabilities())
        .validate(&module)
        .unwrap_or_else(|e| panic!("{name} does not validate: {e:?}"));
    module
}

/// The user-defined locations of a struct a stage passes on, and whether
/// the fragment stage takes `front_facing` (which wgpu counts against the
/// same limit).
fn locations(module: &naga::Module, entry: &str) -> Vec<u32> {
    let entry = module
        .entry_points
        .iter()
        .find(|e| e.name == entry)
        .unwrap_or_else(|| panic!("no entry point {entry}"));
    let ty = match entry.stage {
        naga::ShaderStage::Vertex => entry.function.result.as_ref().expect("a result").ty,
        _ => entry.function.arguments[0].ty,
    };
    let naga::TypeInner::Struct { members, .. } = &module.types[ty].inner else {
        panic!("{} passes a struct", entry.name);
    };
    members
        .iter()
        .filter_map(|m| match m.binding {
            Some(naga::Binding::Location { location, .. }) => Some(location),
            _ => None,
        })
        .collect()
}

/// With and without the dither, and with and without the far paint
/// (`TERRAIN_FAR_PAINT`, which `TerrainConfig::paint` sets).
#[test]
fn the_main_pass_shader_composes_and_validates() {
    for dither in [false, true] {
        for paint in [false, true] {
            let mut defs = defs(dither, false);
            if paint {
                defs.insert("TERRAIN_FAR_PAINT".into(), ShaderDefValue::Bool(true));
            }
            let module = compose("terrain.wgsl", &defs);
            assert!(module.entry_points.iter().any(|e| e.name == "vertex"));
            assert!(module.entry_points.iter().any(|e| e.name == "fragment"));
            // The paint's buffer is bound only where the paint is compiled.
            let bound = module
                .global_variables
                .iter()
                .any(|(_, g)| g.name.as_deref() == Some("terrain_paint"));
            assert_eq!(bound, paint, "dither {dither}, paint {paint}");
        }
    }
}

#[test]
fn the_prepass_shader_composes_and_validates() {
    for dither in [false, true] {
        for prepass in [false, true] {
            let module = compose("terrain_prepass.wgsl", &defs(dither, prepass));
            assert!(module.entry_points.iter().any(|e| e.name == "vertex"));
        }
    }
}

/// wgpu 29 allows 16 inter-stage locations (15 on the WebGL2 downlevel
/// limits), numbered below the limit, and counts the fragment stage's
/// `front_facing` input as one more. The main pass is at 15 of 16 with the
/// dither; the rock of step 3 was packed into the locations there were.
#[test]
fn the_main_pass_fits_the_inter_stage_limit() {
    let module = compose("terrain.wgsl", &defs(true, false));
    let out = locations(&module, "vertex");
    let input = locations(&module, "fragment");
    let mut sorted = out.clone();
    sorted.sort_unstable();
    assert_eq!(
        sorted,
        (0..15).collect::<Vec<u32>>(),
        "the vertex stage's locations"
    );
    assert_eq!(input.len(), out.len());
    // One more for front_facing: 16, the limit, with none to spare.
    assert!(
        input.len() < 16,
        "{} locations and front_facing",
        input.len()
    );
    assert!(out.iter().all(|&l| l < 16));
}

/// The value of `const NAME: f32 = value;` in a shader's text.
fn wgsl_f32(source: &str, name: &str) -> f32 {
    let prefix = format!("const {name}: f32 = ");
    let line = source
        .lines()
        .find_map(|line| line.trim().strip_prefix(prefix.as_str()))
        .unwrap_or_else(|| panic!("no `{prefix}` in the shader"));
    line.trim_end_matches(';')
        .trim()
        .parse()
        .unwrap_or_else(|e| panic!("{name}: {e}"))
}

/// The falls' shader (ticket 0005, step 8) composes and validates, with
/// and without the instance index, and names its parts as the builder does.
#[test]
#[allow(clippy::float_cmp, reason = "the same literal, parsed")]
fn the_fall_shader_composes_and_validates() {
    for instance in [false, true] {
        let mut defs = defs(false, false);
        if !instance {
            defs.remove("VERTEX_OUTPUT_INSTANCE_INDEX");
        }
        let module = compose("fall_water.wgsl", &defs);
        assert!(module.entry_points.iter().any(|e| e.name == "vertex"));
        assert!(module.entry_points.iter().any(|e| e.name == "fragment"));
    }
    let source = read("fall_water.wgsl");
    assert_eq!(
        wgsl_f32(&source, "PART_FOAM"),
        strata_terrain::falls::PART_FOAM
    );
    assert_eq!(
        wgsl_f32(&source, "PART_SPRAY"),
        strata_terrain::falls::PART_SPRAY
    );
    assert_eq!(
        wgsl_f32(&source, "LIP_OVER_M"),
        strata_terrain::falls::LIP_OVER_M
    );
    assert_eq!(
        wgsl_f32(&source, "FOOT_UPSTREAM"),
        strata_terrain::falls::FOOT_UPSTREAM
    );
}

/// The fragment's rock decode is written twice, in `terrain.wgsl` and in
/// the mesher's `RockCorners`, whose tests hold the scheme; the two agree
/// only while their constants do.
#[test]
#[allow(clippy::float_cmp, reason = "the same literal, parsed")]
fn the_rock_decode_matches_the_mesher_mirror() {
    let source = read("terrain.wgsl");
    assert_eq!(
        wgsl_f32(&source, "ROCK_DECODE_FROM"),
        strata_terrain::mesh::DECODE_FROM
    );
    assert_eq!(
        wgsl_f32(&source, "ROCK_DECODE_FULL"),
        strata_terrain::mesh::DECODE_FULL
    );
}

/// The value of `const NAME: u32 = valueu;` in a shader's text.
fn wgsl_u32(source: &str, name: &str) -> u32 {
    let prefix = format!("const {name}: u32 = ");
    let line = source
        .lines()
        .find_map(|line| line.trim().strip_prefix(prefix.as_str()))
        .unwrap_or_else(|| panic!("no `{prefix}` in the shader"));
    line.trim_end_matches(';')
        .trim()
        .trim_end_matches('u')
        .parse()
        .unwrap_or_else(|e| panic!("{name}: {e}"))
}

/// The vertex stage addresses the region store with the grid's side and
/// the record's words; the store packs with the crate's, and the two agree
/// only while these do. The unpacking itself is held to the mesher by the
/// store's own tests, through its CPU mirror.
#[test]
fn the_store_layout_matches_the_packer() {
    let source = read("terrain_common.wgsl");
    assert_eq!(
        wgsl_u32(&source, "TERRAIN_GRID_N") as usize,
        strata_core::GRID_N
    );
    assert_eq!(
        wgsl_u32(&source, "TERRAIN_VERTEX_WORDS") as usize,
        strata_terrain::store::VERTEX_WORDS
    );
    assert_eq!(
        wgsl_f32(&source, "TERRAIN_FAR_FACTOR_RANGE").to_bits(),
        strata_terrain::store::FAR_FACTOR_RANGE.to_bits()
    );
}

/// The fragment reads a region's paint with the slot's layout, the
/// stroke's flags and the near drawing's rules written out again in WGSL;
/// the packer and the mesher use the crate's. A slot resized, a flag moved,
/// the bucket bound or the water mesh's resolution changed on one side
/// alone would misread every slot or open a gap where the paint hands over
/// to the deck or the water mesh, with nothing else to say so.
#[test]
#[allow(clippy::float_cmp, reason = "the same literal, parsed")]
fn the_paint_layout_matches_the_packer() {
    use strata_terrain::paint;

    let source = read("terrain.wgsl");
    let u = |name: &str| wgsl_u32(&source, name) as usize;
    assert_eq!(u("PAINT_WORDS"), paint::PAINT_WORDS);
    assert_eq!(u("PAINT_GRID"), paint::PAINT_GRID);
    assert_eq!(u("PAINT_HEADER_WORDS"), paint::HEADER_WORDS);
    assert_eq!(u("PAINT_ENTRY_WORD"), paint::ENTRY_WORD);
    assert_eq!(u("PAINT_STROKE_WORDS"), paint::STROKE_WORDS);
    assert_eq!(u("PAINT_MOST"), paint::BUCKET_MOST);
    let flag = |name: &str| wgsl_u32(&source, name);
    assert_eq!(flag("PAINT_KIND_RIVER"), paint::KIND_RIVER);
    assert_eq!(flag("PAINT_FLAG_GAP"), paint::FLAG_GAP);
    assert_eq!(flag("PAINT_FLAG_FORD"), paint::FLAG_FORD);
    #[allow(
        clippy::cast_possible_truncation,
        reason = "metres held to f32 in the shader"
    )]
    let (drawn, fade) = (paint::DECK_DRAWN_M as f32, paint::DECK_FADE_M as f32);
    assert_eq!(wgsl_f32(&source, "PAINT_DECK_DRAWN_M"), drawn);
    assert_eq!(wgsl_f32(&source, "PAINT_DECK_FADE_M"), fade);
    assert_eq!(
        wgsl_f32(&source, "PAINT_RESOLVED_CELLS"),
        strata_terrain::river::RESOLVED_CELLS
    );
}

/// The uniform is written twice, as `TerrainParams` in Rust and in
/// `terrain_common.wgsl`; encase lays the Rust one out by its field order,
/// so the shader reads the right numbers only while the two name the same
/// fields in the same order (ticket 0006 added twelve).
#[test]
fn the_uniform_mirrors_the_params_field_for_field() {
    use bevy::reflect::structs::Struct;

    let source = read("terrain_common.wgsl");
    let start = source
        .find("struct TerrainParams {")
        .expect("the shader declares the uniform");
    let body = &source[start..];
    let body = &body[body.find('{').expect("a body") + 1..body.find("\n}").expect("an end")];
    let shader: Vec<&str> = body
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && !line.starts_with("//"))
        .map(|line| line.split(':').next().expect("a field").trim())
        .collect();
    let params = strata_terrain::material::TerrainParams::default();
    let rust: Vec<&str> = (0..params.field_len())
        .map(|i| params.name_at(i).expect("a field name"))
        .collect();
    assert_eq!(shader, rust);
}

/// The value of `const NAME: i32 = value;` in a shader's text.
fn wgsl_i32(source: &str, name: &str) -> i32 {
    let prefix = format!("const {name}: i32 = ");
    let line = source
        .lines()
        .find_map(|line| line.trim().strip_prefix(prefix.as_str()))
        .unwrap_or_else(|| panic!("no `{prefix}` in the shader"));
    line.trim_end_matches(';')
        .trim()
        .parse()
        .unwrap_or_else(|e| panic!("{name}: {e}"))
}

/// The value of `const NAME: vec3<f32> = vec3<f32>(x, y, z);`.
fn wgsl_vec3(source: &str, name: &str) -> [f32; 3] {
    let prefix = format!("const {name}: vec3<f32> = vec3<f32>(");
    let line = source
        .lines()
        .find_map(|line| line.trim().strip_prefix(prefix.as_str()))
        .unwrap_or_else(|| panic!("no `{prefix}` in the shader"));
    let values: Vec<f32> = line
        .trim_end_matches(");")
        .split(',')
        .map(|v| v.trim().parse().unwrap_or_else(|e| panic!("{name}: {e}")))
        .collect();
    [values[0], values[1], values[2]]
}

/// `terrain.wgsl`'s `hue_shift`, written again to hold its constants to.
fn hue_shift(
    albedo: [f32; 3],
    amount: f32,
    plus: [f32; 3],
    minus: [f32; 3],
    luma: [f32; 3],
) -> [f32; 3] {
    let a = amount.clamp(-1.0, 1.0);
    let towards = if a >= 0.0 { plus } else { minus };
    let shifted: [f32; 3] =
        std::array::from_fn(|c| albedo[c] * (1.0 + (towards[c] - 1.0) * a.abs()));
    let dot = |v: [f32; 3]| v[0] * luma[0] + v[1] * luma[1] + v[2] * luma[2];
    let k = dot(albedo) / dot(shifted).max(1e-5);
    shifted.map(|c| c * k)
}

/// The colour shapes (ticket 0006) are hue and never value: each end gives
/// the ground back its luminance and pulls the way it is named (lush
/// greener, dry and warm redder against blue, cool bluer), their two scales
/// are the 420 m wash and the 30 m patches the pass describes, and the
/// drawn edge is narrower than the blend it sits on.
#[test]
fn the_colour_shapes_keep_the_value_and_pull_the_named_way() {
    let source = read("terrain.wgsl");
    let luma = wgsl_vec3(&source, "LUMA");
    assert!((luma.iter().sum::<f32>() - 1.0).abs() < 1e-4);
    let lush = wgsl_vec3(&source, "COLOUR_LUSH");
    let dry = wgsl_vec3(&source, "COLOUR_DRY");
    let warm = wgsl_vec3(&source, "COLOUR_WARM");
    let cool = wgsl_vec3(&source, "COLOUR_COOL");
    // A meadow's grass and a wood's litter, linear.
    for ground in [[0.06, 0.09, 0.03], [0.12, 0.07, 0.035]] {
        let dot = |v: [f32; 3]| v[0] * luma[0] + v[1] * luma[1] + v[2] * luma[2];
        for (plus, minus) in [(lush, dry), (warm, cool)] {
            for amount in [-1.0, -0.4, 0.3, 1.0] {
                let out = hue_shift(ground, amount, plus, minus, luma);
                assert!((dot(out) - dot(ground)).abs() < 1e-6, "{amount}: {out:?}");
            }
        }
        let greener = hue_shift(ground, 1.0, lush, dry, luma);
        assert!(greener[1] / greener[0] > ground[1] / ground[0]);
        let drier = hue_shift(ground, -1.0, lush, dry, luma);
        assert!(drier[0] / drier[2] > ground[0] / ground[2]);
        let warmer = hue_shift(ground, 1.0, warm, cool, luma);
        assert!(warmer[0] / warmer[2] > ground[0] / ground[2]);
        let cooler = hue_shift(ground, -1.0, warm, cool, luma);
        assert!(cooler[2] / cooler[0] > ground[2] / ground[0]);
    }
    let period = strata_terrain::material::UV_PERIOD_M;
    #[allow(clippy::cast_precision_loss, reason = "small cell counts")]
    let metres = |name: &str| period / wgsl_i32(&source, name) as f32;
    assert!((metres("SHAPE_BROAD_CELLS") - 420.0).abs() < 1e-3);
    assert!((metres("SHAPE_PATCH_CELLS") - 30.0).abs() < 1e-3);
    assert!(wgsl_f32(&source, "SEAM_POWER") > 1.0);
}

/// `terrain.wgsl`'s `hash_cell`, written again to measure its noise by.
fn hash_cell(x: i32, y: i32) -> f32 {
    #[allow(clippy::cast_sign_loss, reason = "WGSL's bitcast")]
    let (x, y) = (x as u32, y as u32);
    let mut h = x.wrapping_mul(0x8da6_b343) ^ y.wrapping_mul(0xd816_3841);
    h ^= h >> 13;
    h = h.wrapping_mul(0x27d4_eb2d);
    h ^= h >> 15;
    #[allow(clippy::cast_precision_loss, reason = "sixteen bits")]
    let v = (h & 0xffff) as f32 / 65535.0;
    v
}

/// `terrain.wgsl`'s `value_noise`.
#[allow(clippy::cast_possible_truncation, reason = "a lattice index")]
fn value_noise(p: [f32; 2], n: i32) -> f32 {
    let (ix, iy) = (p[0].floor() as i32, p[1].floor() as i32);
    let (fx, fy) = (p[0] - p[0].floor(), p[1] - p[1].floor());
    let (ux, uy) = (fx * fx * (3.0 - 2.0 * fx), fy * fy * (3.0 - 2.0 * fy));
    let wrap = |i: i32| i.rem_euclid(n);
    let at = |dx: i32, dy: i32| hash_cell(wrap(ix + dx), wrap(iy + dy));
    let lower = at(0, 0) + (at(1, 0) - at(0, 0)) * ux;
    let upper = at(0, 1) + (at(1, 1) - at(0, 1)) * ux;
    lower + (upper - lower) * uy
}

/// The colour shapes' field is documented and tuned as a spread of one:
/// `SHAPE_SD` is what its two octaves spread before it is divided out,
/// measured over the whole period with the shader's own hash. A spread
/// above one clamped a quarter of the ground into flat patches of one end
/// colour before the review caught it.
#[test]
fn the_shape_field_spreads_one() {
    let source = read("terrain.wgsl");
    let sd = wgsl_f32(&source, "SHAPE_SD");
    let broad = wgsl_i32(&source, "SHAPE_BROAD_CELLS");
    let patches = wgsl_i32(&source, "SHAPE_PATCH_CELLS");
    let period = strata_terrain::material::UV_PERIOD_M;
    #[allow(clippy::cast_precision_loss, reason = "small cell counts")]
    let (broad_m, patch_m) = (period / broad as f32, period / patches as f32);
    let steps = 1000;
    let (mut sum, mut sum2) = (0.0_f64, 0.0_f64);
    for j in 0..steps {
        for i in 0..steps {
            #[allow(clippy::cast_precision_loss, reason = "small counts")]
            let p = [
                (i as f32 + 0.37) * period / steps as f32,
                (j as f32 + 0.71) * period / steps as f32,
            ];
            let b = value_noise([p[0] / broad_m, p[1] / broad_m], broad);
            let q = value_noise([p[0] / patch_m, p[1] / patch_m], patches);
            let field = f64::from((0.6 * (b - 0.5) + 0.4 * (q - 0.5)) / sd);
            sum += field;
            sum2 += field * field;
        }
    }
    let count = f64::from(steps * steps);
    let mean = sum / count;
    let spread = (sum2 / count - mean * mean).sqrt();
    assert!(
        (spread - 1.0).abs() < 0.05,
        "the shape field spreads {spread}"
    );
    assert!(mean.abs() < 0.1, "the shape field's mean is {mean}");
}
