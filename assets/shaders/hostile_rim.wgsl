// A hostile's body over the standard material: Bevy's lighting as it is, then
// a fresnel rim added on the pixels of the body that turn from the eye, so a
// target reads against grass at forty metres (visual.md F6). A few ALU on
// character pixels and no texture read; the dials are `character:` in
// assets/look/values.ron.

#import bevy_pbr::{
    forward_io::{VertexOutput, FragmentOutput},
    mesh_view_bindings::view,
    pbr_fragment::pbr_input_from_standard_material,
    pbr_functions::{alpha_discard, apply_pbr_lighting, main_pass_post_lighting_processing},
    pbr_types::STANDARD_MATERIAL_FLAGS_UNLIT_BIT,
}

// The rim colour in linear light, its strength in w.
@group(#{MATERIAL_BIND_GROUP}) @binding(100) var<uniform> rim_colour: vec4<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(101) var<uniform> rim_power: f32;

@fragment
fn fragment(
    in: VertexOutput,
    @builtin(front_facing) is_front: bool,
) -> FragmentOutput {
    var pbr_input = pbr_input_from_standard_material(in, is_front);
    pbr_input.material.base_color = alpha_discard(pbr_input.material, pbr_input.material.base_color);
    var out: FragmentOutput;
    if (pbr_input.material.flags & STANDARD_MATERIAL_FLAGS_UNLIT_BIT) == 0u {
        out.color = apply_pbr_lighting(pbr_input);
        let facing = clamp(dot(normalize(pbr_input.N), normalize(pbr_input.V)), 0.0, 1.0);
        let rim = pow(1.0 - facing, rim_power) * rim_colour.w;
        out.color = vec4<f32>(out.color.rgb + rim_colour.rgb * rim, out.color.a);
    } else {
        out.color = pbr_input.material.base_color;
    }
    out.color = main_pass_post_lighting_processing(pbr_input, out.color);
    return out;
}
