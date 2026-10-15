// The sea for `strata_terrain::water::WaterMaterial`.
//
// Bevy's StandardMaterial lighting under a surface of our own, composited
// with premultiplied alpha as water is: what the eye gets is the light the
// water body scatters back, plus the sky and sun the surface reflects, plus
// whatever lies under it, dimmed by the water it crosses and by what the
// surface reflected away.
//
// - The opacity of the body comes from how much water the view ray crosses
//   before it meets the sea floor, read from the depth prepass.
//   Beer-Lambert absorption, `1 - exp(-d * k)`, makes a shoreline fade in
//   from nothing over the first metre or two and deep water opaque, so the
//   terrain grid the plane cuts through never shows as a saw-tooth edge and
//   a shelf under deep water does not show through.
// - The surface is two scrolled reads of one tileable wave image (slopes in
//   `rg`, height in `b`), the swell along the wind and the chop across it at
//   a quarter of its size, faded out with distance while the roughness
//   rises in their place: a normal map at a grazing angle only aliases,
//   and the slopes it would have drawn are what spread the sun's
//   reflection into a track.
// - The reflection: Bevy lights the body and the sun's glitter (its GGX
//   lobe at a low roughness on each facet that faces the sun, through the
//   atmosphere's transmittance and the cloud layer's shadow), with the
//   environment map's specular switched off; the sky's reflection is read
//   here from the same map at intensity one, since Bevy's would carry the
//   sky fill (4), which is a lift for the shade's diffuse light, not the
//   sky's radiance. The map holds the sky's inscattering alone: no sun disc
//   and no cloud layer (a quad drawn in this same transparent pass), so the
//   clouds are not reflected. Schlick's Fresnel from the perturbed normal
//   with the material's reflectance (tinted towards the water's blue) sets
//   how much of it there is and how much of the floor is lost to it.
// - Foam: a band along the shore and on the crests where the two layers'
//   heights add up high, both broken by the image's fourth channel read at
//   its own slow frame; a matte white body with no reflection. The shore's
//   band is where the water is shallow straight down or, since the world's
//   coasts mostly drop into deep water within a metre of the edge, where
//   the depth prepass shows land within a few metres along a world axis.
// - The air between the camera and the water dims the water's light and
//   adds the haze in front of it, taken as the sky just over the horizon
//   (a saturated path's inscattering) times what the air took away: Bevy's
//   aerial perspective is drawn before the transparent pass and only over
//   opaque depth, so far water was as bright as near water.

// The pieces both surfaces share (the parameters, the floor behind, the
// foam's fray and the lighting) are `water_common.wgsl`'s.

#import bevy_pbr::{
    forward_io::{VertexOutput, FragmentOutput},
    mesh_view_bindings as view_bindings,
    pbr_fragment::pbr_input_from_standard_material,
    pbr_functions::main_pass_post_lighting_processing,
}

#import strata::water_common::{water, wave_texture, wave_sampler, perp, frame_uv, land_near, floor_behind, shore_foam, water_light}

// How much of the crest's height comes from the swell; the rest is the
// chop's. The swell carries the long crests the foam should follow.
const SWELL_SHARE: f32 = 0.65;
// The crest's foam ramp, in the combined height over its threshold.
const CREST_RAMP: f32 = 0.08;
// How far from the camera the shore's foam is looked for by the land's
// distance, metres: past it the band is about a pixel wide.
const SHORE_RANGE: f32 = 400.0;

@fragment
fn fragment(
    in: VertexOutput,
    @builtin(front_facing) is_front: bool,
#ifdef MULTISAMPLED
    @builtin(sample_index) sample_index: u32,
#endif
) -> FragmentOutput {
#ifdef MULTISAMPLED
    let sample = sample_index;
#else
    let sample = 0u;
#endif
    let pbr_input = pbr_input_from_standard_material(in, is_front);
    let p = in.world_position.xyz;
    let camera = view_bindings::view.world_position;
    let range = length(p - camera);

    // Every read before any branch, so the mips' derivatives hold.
    let swell_heading = water.axes.xy;
    let chop_heading = water.axes.zw;
    let swell = textureSample(wave_texture, wave_sampler, frame_uv(p.xz, swell_heading, water.swell));
    let chop = textureSample(wave_texture, wave_sampler, frame_uv(p.xz, chop_heading, water.chop));
    let foam_noise = textureSample(wave_texture, wave_sampler, frame_uv(p.xz, swell_heading, water.foam)).a;

    // The surface: each layer's slope along and across its heading, back
    // in world x and z, faded out with distance.
    let detail = 1.0 - smoothstep(water.fade.x, water.fade.y, range);
    let gs = (swell.rg * 2.0 - 1.0) * water.swell.w;
    let gc = (chop.rg * 2.0 - 1.0) * water.chop.w;
    let gradient = (gs.x * swell_heading + gs.y * perp(swell_heading)
        + gc.x * chop_heading + gc.y * perp(chop_heading)) * detail;
    let N = normalize(vec3<f32>(-gradient.x, 1.0, -gradient.y));
    let roughness = mix(water.fade.z, pbr_input.material.perceptual_roughness, detail);

    // Path length through the water to whatever is behind this fragment,
    // and the water's depth straight down there.
    let behind = floor_behind(in.position, p, sample);

    // Foam at the shore, frayed by the noise, and on the high crests near
    // enough to have waves.
    // A branch, not `select`: a call's arguments are all evaluated, and the
    // search's depth loads would then run out to the horizon. Every
    // texture sample is above, so the branch costs no derivatives.
    var land = 0.0;
    if range < SHORE_RANGE {
        land = land_near(p, water.crest.y, sample);
    }
    let shore = max(1.0 - saturate(behind.y / water.foam.w), land);
    let height = swell.b * SWELL_SHARE + chop.b * (1.0 - SWELL_SHARE);
    let crest_foam = smoothstep(water.crest.x, water.crest.x + CREST_RAMP, height)
        * smoothstep(0.35, 0.65, foam_noise) * detail;
    let foam = saturate(max(shore_foam(shore, foam_noise), crest_foam)) * water.optics.w;

    let color = water_light(pbr_input, p, N, roughness, behind.x, water.optics.x, foam);
    var out: FragmentOutput;
    out.color = main_pass_post_lighting_processing(pbr_input, color);
    return out;
}
