// The crown's back-light (review step six, F5; ticket 0006 pass 2), shared by
// the near tree shader and the far trees' impostor shader so the swap between
// them does not show it. The dark core and the lit clumps are a factor on the
// albedo (`assets/look/canopy.ron`, baked into the impostor atlas and applied
// in `foliage.wgsl`); this is the one part that depends on the sun and the
// eye, so it is added at run time by both.

#define_import_path strata::canopy

#import bevy_pbr::mesh_view_bindings as view_bindings

// Sunlight through the leaves of a crown seen against the sun: a broad lobe
// round the sun and a narrow one right under it, on the pattern of grass's
// `backlight` but with no shadow lookup (a crown's own sky share, in
// `openness`, darkens it where the neighbours shade it, so it costs a dozen
// ALU and no read). `V` points from the surface to the eye.
fn canopy_rim(V: vec3<f32>, albedo: vec3<f32>, openness: f32, strength: f32) -> vec3<f32> {
    if strength <= 0.0 || view_bindings::lights.n_directional_lights == 0u {
        return vec3<f32>(0.0);
    }
    let sun = &view_bindings::lights.directional_lights[0];
    let L = (*sun).direction_to_light;
    // The moon by day and a set sun light nothing.
    let against = dot(-V, L);
    if against <= 0.0 || L.y <= 0.0 {
        return vec3<f32>(0.0);
    }
    let lobe = 0.4 * against * against + 0.6 * pow(against, 8.0);
    // Fades out as the sun reaches the horizon, where the ground between
    // takes it.
    let horizon = smoothstep(0.0, 0.06, L.y);
    // Bevy's directional colour is the sun's illuminance in colour; over pi,
    // a diffuse surface facing it.
    return albedo * (*sun).color.rgb * (strength * lobe * horizon * openness * 0.3183) * view_bindings::view.exposure;
}
