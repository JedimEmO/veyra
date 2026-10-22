//! A session-local look laboratory over the existing slotted screen stack.

use bevy::app::{App, Startup, Update};
use bevy::ecs::prelude::*;
use bevy::ecs::schedule::{
    SystemCondition,
    common_conditions::{resource_changed, resource_exists_and_changed},
};
#[cfg(feature = "dev")]
use bevy::input::ButtonInput;
use bevy::input::keyboard::KeyCode;
use bevy::math::Vec2;
use bevy::ui::widget::Text;
// Exposure and the lens (lighting plan phase 4b).
use slotted::menu::{MenuChoice, SettingsRow, SettingsSpec};
use slotted::ui::{
    ButtonOpts, Layout, LayoutDirection, Length, LocKey, NineAnchor, Place, Presentation,
    PresentationMode, ScreenKind, ScreenStack, Screens, SelectOption, Tags, TestIdIndex, TextOpts,
    TextRole, ToggleStyle, UiNodeDef, Value, ValueRules, ValueStore, pop_screen, push_screen,
};
use strata_atmosphere::exposure::ExposureMode;
use strata_atmosphere::{
    SkySettings,
    look::{AaOverride, LookFile, PRESETS, RenderLook, ShadowFilter, effective_aa},
    study::StudySettings,
};

/// F6 toggles the panel. Escape closes it through the regular stack.
pub const KEY: KeyCode = KeyCode::F6;
/// Stable kind for test harnesses and scripted captures.
pub fn kind() -> ScreenKind {
    ScreenKind::new("strata:render")
}

struct Number {
    key: &'static str,
    label: &'static str,
    tab: &'static str,
    min: f64,
    max: f64,
    step: f64,
    get: fn(&LookFile) -> f64,
    set: fn(&mut LookFile, f64),
}
macro_rules! number {
    ($field:ident.$name:ident, $label:literal, $tab:literal, $min:expr, $max:expr, $step:expr) => {
        Number {
            key: concat!("render.", stringify!($name)),
            label: $label,
            tab: $tab,
            min: $min,
            max: $max,
            step: $step,
            get: |f| f64::from(f.$field.$name),
            set: |f, v| f.$field.$name = v as _,
        }
    };
}
/// One channel of the light rays' tint, which is an array the field macro
/// cannot index.
macro_rules! tint {
    ($channel:literal, $label:literal) => {
        Number {
            key: concat!("render.light_rays_tint_", $channel),
            label: $label,
            tab: "light",
            min: 0.0,
            max: 2.0,
            step: 0.01,
            get: |f| f64::from(f.look.light_rays_tint[$channel]),
            set: |f, v| f.look.light_rays_tint[$channel] = v as f32,
        }
    };
}
#[allow(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "validated slider ranges fit their render parameter types"
)]
const NUMBERS: &[Number] = &[
    number!(sky.ev100, "Exposure (EV100)", "light", 8.0, 20.0, 0.05),
    number!(sky.sun_intensity, "Sun intensity", "light", 0.0, 4.0, 0.05),
    number!(
        sky.environment_intensity,
        "Sky fill",
        "light",
        0.0,
        4.0,
        0.05
    ),
    number!(sky.haze, "Distant haze", "light", 0.01, 3.0, 0.01),
    // Irradiance probes (lighting plan phase 3b): zero hides them and the
    // sky's environment map lights the shade alone.
    number!(
        look.irradiance,
        "Irradiance probes",
        "light",
        0.0,
        4.0,
        0.05
    ),
    // Sky visibility (lighting plan phase 3a): zero lights the shade with
    // the whole sky, as before the bake.
    number!(
        look.sky_visibility,
        "Sky visibility",
        "light",
        0.0,
        1.0,
        0.05
    ),
    // Grass against the low sun (lighting plan phase 5b): zero is the
    // grass before the term.
    number!(
        look.grass_rim,
        "Grass rim and transmission",
        "light",
        0.0,
        2.0,
        0.05
    ),
    number!(look.contrast, "Contrast", "look", 0.5, 2.0, 0.01),
    // The ground painted (ticket 0006, pass 1). Zero colour, warmth,
    // palette, matte, edges and air with grain and relief at one is the
    // ground before the pass but for what this tab cannot take back: the
    // cavity shade baked into the textures at start (TerrainParams'
    // `cavity`), the normal maps fading by 250 m where they faded by 400 m
    // (`normal_far`), and the biomes' ground colours in the far tint.
    number!(
        look.ground_colour,
        "Ground lush and dry",
        "ground",
        0.0,
        2.0,
        0.05
    ),
    number!(
        look.ground_warmth,
        "Ground warm and cool",
        "ground",
        0.0,
        2.0,
        0.05
    ),
    number!(
        look.ground_near_tint,
        "Ground palette near",
        "ground",
        0.0,
        1.0,
        0.05
    ),
    number!(
        look.ground_near_value,
        "Ground palette value",
        "ground",
        0.0,
        1.0,
        0.05
    ),
    number!(look.ground_grain, "Ground grain", "ground", 0.0, 1.5, 0.05),
    number!(
        look.ground_normal,
        "Ground relief (normal maps)",
        "ground",
        0.0,
        1.5,
        0.05
    ),
    number!(look.ground_matte, "Ground matte", "ground", 0.0, 1.0, 0.05),
    number!(
        look.ground_seam,
        "Ground layer edges",
        "ground",
        0.0,
        1.0,
        0.05
    ),
    number!(look.ground_air, "Ground air", "ground", 0.0, 2.0, 0.05),
    number!(look.saturation, "Saturation", "look", 0.0, 2.0, 0.01),
    number!(look.render_scale, "Render scale", "look", 0.5, 1.0, 0.05),
    // Colour scripting (lighting plan phase 4a): the grade by biome and
    // time of day over the look's contrast and saturation; zero is the
    // look's alone.
    number!(
        look.grade,
        "Colour grade (biome, time)",
        "look",
        0.0,
        1.0,
        0.05
    ),
    number!(look.bloom, "Bloom", "look", 0.0, 0.3, 0.005),
    number!(
        look.bloom_threshold,
        "Bloom threshold",
        "look",
        0.0,
        10.0,
        0.05
    ),
    number!(sky.sun_disk_intensity, "Sun disc", "light", 0.0, 10.0, 0.1),
    number!(
        look.light_rays_strength,
        "Light rays",
        "light",
        0.0,
        2.0,
        0.01
    ),
    number!(
        look.light_rays_decay,
        "Light ray decay",
        "light",
        0.8,
        1.0,
        0.005
    ),
    number!(
        look.light_rays_steps,
        "Light ray steps",
        "light",
        8.0,
        48.0,
        4.0
    ),
    tint!(0, "Light ray red"),
    tint!(1, "Light ray green"),
    tint!(2, "Light ray blue"),
    // An `Option` the field macro cannot convert: zero on the slider is
    // `None`, Bevy's ordinary shadow with no blocker search at all.
    Number {
        key: "render.soft_shadow_size",
        label: "Soft shadows (PCSS)",
        tab: "light",
        min: 0.0,
        max: 16.0,
        step: 0.25,
        get: |f| f64::from(f.sky.soft_shadow_size.unwrap_or(0.0)),
        set: |f, v| f.sky.soft_shadow_size = (v > 0.0).then_some(v as f32),
    },
    // The sun's cascades and the sky's environment map (lighting plan
    // phase 6): the cascades are rebuilt live.
    number!(
        sky.shadow_distance,
        "Shadow distance (m)",
        "light",
        500.0,
        20_000.0,
        250.0
    ),
    number!(
        sky.first_cascade_far_bound,
        "First cascade (m)",
        "light",
        10.0,
        1_000.0,
        10.0
    ),
    number!(
        sky.shadow_cascades,
        "Shadow cascades",
        "light",
        1.0,
        4.0,
        1.0
    ),
    number!(
        sky.building_shadow_cascades,
        "Cascades buildings cast into",
        "light",
        1.0,
        4.0,
        1.0
    ),
    number!(
        sky.environment_map_refresh_degrees,
        "Sky map refresh (sun degrees, 0 every frame)",
        "light",
        0.0,
        10.0,
        0.25
    ),
    // A power of two, so the slider is its exponent; Bevy reads the size
    // once per camera, so it takes effect on the next start.
    Number {
        key: "render.environment_map_size",
        label: "Sky map size (2^n, on restart)",
        tab: "light",
        min: 5.0,
        max: 10.0,
        step: 1.0,
        get: |f| f64::from(f.sky.environment_map_size.ilog2()),
        set: |f, v| f.sky.environment_map_size = 1 << (v.round() as u32).clamp(5, 10),
    },
    number!(look.fog_density, "Mist density", "air", 0.0, 0.1, 0.001),
    number!(
        look.fog_light_intensity,
        "Mist light strength",
        "air",
        0.0,
        16.0,
        0.1
    ),
    number!(look.fog_scattering, "Scattering", "air", 0.0, 1.0, 0.01),
    number!(
        look.fog_asymmetry,
        "Forward scattering",
        "air",
        0.0,
        0.95,
        0.01
    ),
    number!(look.fog_steps, "Raymarch steps", "air", 8.0, 128.0, 8.0),
    // Local fog volumes (lighting plan phase 2c).
    number!(
        look.local_fog_steps,
        "Local fog steps",
        "air",
        4.0,
        64.0,
        2.0
    ),
    // The valley fog's light (lighting plan phase 2a).
    number!(
        look.valley_fog_shadow,
        "Valley fog shadows",
        "air",
        0.0,
        1.0,
        0.05
    ),
    number!(
        look.valley_floor_density,
        "Valley air floor (per km)",
        "air",
        0.0,
        0.1,
        0.0005
    ),
    number!(
        look.valley_floor_height,
        "Valley air floor height (m)",
        "air",
        -500.0,
        5000.0,
        25.0
    ),
    number!(
        look.valley_floor_falloff,
        "Valley air floor falloff (m)",
        "air",
        10.0,
        5000.0,
        10.0
    ),
    number!(
        look.focus_distance,
        "Focus distance (m)",
        "lens",
        0.1,
        100.0,
        0.1
    ),
    number!(look.aperture, "Aperture (f-stop)", "lens", 0.5, 32.0, 0.1),
    number!(
        look.blur_diameter,
        "Maximum blur (px)",
        "lens",
        0.0,
        32.0,
        1.0
    ),
    number!(look.motion_blur, "Motion blur", "lens", 0.0, 1.0, 0.025),
    // The cloud layer (lighting plan phase 2b).
    number!(sky.cloud_cover, "Cloud cover", "air", 0.0, 1.0, 0.01),
    number!(
        sky.cloud_scale,
        "Cloud tile (m)",
        "air",
        500.0,
        50_000.0,
        250.0
    ),
    number!(sky.cloud_speed, "Cloud speed (m/s)", "air", 0.0, 60.0, 0.5),
    number!(
        sky.cloud_shear,
        "Upper cloud speed (x lower)",
        "air",
        0.0,
        4.0,
        0.05
    ),
    number!(
        sky.cloud_upper_cover,
        "Upper cloud cover (x cover)",
        "air",
        0.0,
        2.0,
        0.05
    ),
    number!(
        sky.cloud_thickness,
        "Cloud thickness (m)",
        "air",
        0.0,
        2_000.0,
        10.0
    ),
    number!(
        sky.cloud_evolution,
        "Cloud evolution (/s)",
        "air",
        0.0,
        0.5,
        0.005
    ),
    // Exposure and the lens (lighting plan phase 4b): the band is an array
    // the field macro cannot index.
    Number {
        key: "render.auto_exposure_range_low",
        label: "Auto exposure brightest (EV)",
        tab: "light",
        min: -4.0,
        max: 0.0,
        step: 0.1,
        get: |f| f64::from(f.look.auto_exposure_range_ev[0]),
        set: |f, v| f.look.auto_exposure_range_ev[0] = v as f32,
    },
    Number {
        key: "render.auto_exposure_range_high",
        label: "Auto exposure darkest (EV)",
        tab: "light",
        min: 0.0,
        max: 4.0,
        step: 0.1,
        get: |f| f64::from(f.look.auto_exposure_range_ev[1]),
        set: |f, v| f.look.auto_exposure_range_ev[1] = v as f32,
    },
    number!(
        look.auto_exposure_key,
        "Auto exposure key (log2)",
        "light",
        -8.0,
        2.0,
        0.05
    ),
    number!(
        look.auto_exposure_speed_brighten,
        "Auto exposure into light (stops/s)",
        "light",
        0.1,
        20.0,
        0.1
    ),
    number!(
        look.auto_exposure_speed_darken,
        "Auto exposure into shade (stops/s)",
        "light",
        0.1,
        20.0,
        0.1
    ),
    number!(look.vignette, "Vignette", "lens", 0.0, 1.0, 0.01),
    number!(
        look.vignette_radius,
        "Vignette radius",
        "lens",
        0.1,
        2.0,
        0.01
    ),
    number!(
        look.vignette_smoothness,
        "Vignette falloff",
        "lens",
        0.1,
        10.0,
        0.1
    ),
    number!(
        look.aberration,
        "Aberration on a hurt",
        "lens",
        0.0,
        1.0,
        0.01
    ),
    // Auto exposure by night (lighting plan phase 4, after review): how
    // much of the brightening end survives full darkness.
    number!(
        look.auto_exposure_night,
        "Auto exposure by night (share)",
        "light",
        0.0,
        1.0,
        0.05
    ),
    // The water (lighting plan phase 5a): zero is a matte sea, and a sea
    // with no foam.
    number!(
        look.water_reflection,
        "Water reflection",
        "look",
        0.0,
        1.0,
        0.05
    ),
    number!(look.water_foam, "Water foam", "look", 0.0, 1.0, 0.05),
];

struct Flag {
    key: &'static str,
    label: &'static str,
    tab: &'static str,
    get: fn(&LookFile) -> bool,
    set: fn(&mut LookFile, bool),
}
macro_rules! flag {
    ($name:ident, $label:literal, $tab:literal) => {
        Flag {
            key: concat!("render.", stringify!($name)),
            label: $label,
            tab: $tab,
            get: |f| f.look.$name,
            set: |f, v| f.look.$name = v,
        }
    };
}
const FLAGS: &[Flag] = &[
    flag!(agx, "AgX tonemapping", "look"),
    flag!(taa, "Temporal antialiasing", "look"),
    flag!(contact_shadows, "Contact shadows", "light"),
    flag!(ssao, "Ambient occlusion", "light"),
    flag!(dof, "Depth of field", "lens"),
    flag!(ground_fog, "Valley fog", "air"),
    // Local fog volumes (lighting plan phase 2c).
    flag!(local_fog, "Local fog volumes", "air"),
    // The sun's cascades (lighting plan phase 6): a sky field, so not the
    // look's macro.
    Flag {
        key: "render.trim_shadow_casters",
        label: "Trim cascade casters",
        tab: "light",
        get: |f| f.sky.trim_shadow_casters,
        set: |f, v| f.sky.trim_shadow_casters = v,
    },
    // Exposure and the lens (lighting plan phase 4b): two modes, so a
    // switch rather than a choice.
    Flag {
        key: "render.auto_exposure",
        label: "Auto exposure",
        tab: "light",
        get: |f| f.look.exposure_mode == ExposureMode::Auto,
        set: |f, v| {
            f.look.exposure_mode = if v {
                ExposureMode::Auto
            } else {
                ExposureMode::Manual
            };
        },
    },
];

/// The store key of the shadow filter, a choice rather than a number or a
/// flag.
const SHADOW_FILTER: &str = "render.shadow_filter";

/// The labels are shared by the schema and the fallback catalogue.
pub fn english(key: &str) -> Option<&'static str> {
    NUMBERS
        .iter()
        .find(|v| v.key == key)
        .map(|v| v.label)
        .or_else(|| FLAGS.iter().find(|v| v.key == key).map(|v| v.label))
        .or_else(|| {
            Some(match key {
                "render.title" => "Rendering study",
                "render.look" => "Look",
                "render.light" => "Light",
                "render.air" => "Air",
                "render.lens" => "Lens",
                "render.ground" => "Ground",
                "render.preset" => "Preset",
                SHADOW_FILTER => "Shadow filter",
                "render.shadow_filter.hardware2x2" => "Hardware 2x2",
                "render.shadow_filter.gaussian" => "Gaussian",
                "render.shadow_filter.temporal" => "Temporal (Gaussian without TAA)",
                "render.compare" => "Compare baseline (edits held)",
                "render.reset" => "Reset look",
                "render.export" => "Export RON",
                "render.close" => "Close (F6)",
                "render.status" => "F6 closes the panel. F3 shows performance.",
                "baseline" => "Baseline",
                "clear-contrast" => "Clear Contrast",
                "sunlit-mist" => "Sunlit Mist",
                "cinematic-woodland" => "Cinematic Woodland",
                _ => return None,
            })
        })
}

/// The look panel's words as a fallback catalogue of their own, because the
/// game's catalogue (`veyra-ui`) is a crate above this one and knows none of
/// them. The keys are disjoint from the game's, so the order they are asked
/// in does not matter.
#[derive(Debug, Default, Clone, Copy)]
struct RenderStrings;

impl slotted::ui::Localizer for RenderStrings {
    fn resolve(&self, key: &LocKey, _args: &slotted::ui::LocArgs) -> Option<String> {
        english(&key.0).map(str::to_owned)
    }
}

/// `Startup`: pushes the look panel's fallback.
pub(crate) fn install_strings(mut localization: ResMut<slotted::ui::Localization>) {
    localization.push_fallback(RenderStrings);
}

fn button(action: &str) -> UiNodeDef {
    UiNodeDef::Button {
        widget: None,
        opts: ButtonOpts {
            label: Some(LocKey(action.into())),
            compact: true,
            ..Default::default()
        },
        tags: Tags::new().with("menu", action).with(Tags::TEST_ID, action),
    }
}

fn spec() -> SettingsSpec {
    let mut spec = SettingsSpec::new(kind());
    for tab in ["look", "light", "air", "lens", "ground"] {
        spec = spec.tab(tab, format!("render.{tab}"));
        if tab == "look" {
            spec = spec.row(SettingsRow::Select {
                key: "render.preset".into(),
                label: LocKey("render.preset".into()),
                options: PRESETS
                    .iter()
                    .map(|s| SelectOption {
                        id: (*s).into(),
                        label: LocKey((*s).into()),
                    })
                    .collect(),
                default: "baseline".into(),
            });
            spec = spec.row(SettingsRow::Toggle {
                key: "render.compare".into(),
                label: LocKey("render.compare".into()),
                default: false,
                style: ToggleStyle::Switch,
            });
        }
        if tab == "light" {
            spec = spec.row(SettingsRow::Select {
                key: SHADOW_FILTER.into(),
                label: LocKey(SHADOW_FILTER.into()),
                options: ShadowFilter::ALL
                    .iter()
                    .map(|f| SelectOption {
                        id: f.as_str().into(),
                        label: LocKey(format!("{SHADOW_FILTER}.{}", f.as_str())),
                    })
                    .collect(),
                default: RenderLook::default().shadow_filter.as_str().into(),
            });
        }
        // Without the `pcss` feature the light ignores the value, so the row
        // would do nothing.
        for n in NUMBERS.iter().filter(|n| {
            n.tab == tab && (strata_atmosphere::SOFT_SHADOWS || n.key != "render.soft_shadow_size")
        }) {
            spec = spec.row(SettingsRow::Slider {
                key: n.key.into(),
                label: LocKey(n.key.into()),
                min: n.min,
                max: n.max,
                step: n.step,
                default: (n.get)(&LookFile::default()),
                format: "{value:.3}".into(),
            });
        }
        for f in FLAGS.iter().filter(|f| f.tab == tab) {
            spec = spec.row(SettingsRow::Toggle {
                key: f.key.into(),
                label: LocKey(f.key.into()),
                default: false,
                style: ToggleStyle::Switch,
            });
        }
    }
    spec
}

#[derive(Resource, Default)]
struct PanelState {
    edited: LookFile,
    version: u64,
    status: String,
    initialized: bool,
}

pub(crate) fn register(app: &mut App) {
    slotted::ui::TestIdIndex::install(app.world_mut());
    #[cfg(feature = "dev")]
    app.add_systems(Update, toggle.before(actions).in_set(super::ScreenWork));
    app.init_resource::<SkySettings>()
        .init_resource::<RenderLook>()
        .init_resource::<StudySettings>()
        .init_resource::<PanelState>()
        .add_systems(Startup, setup)
        .add_systems(
            Update,
            (
                actions,
                apply_values.after(super::graphics::resolve),
                status,
            )
                .chain()
                .in_set(super::ScreenWork),
        )
        // Sky visibility (lighting plan phase 3a).
        .add_systems(
            Update,
            sync_sky_visibility.run_if(
                resource_changed::<RenderLook>
                    .or_eager(resource_exists_and_changed::<strata_terrain::TerrainLook>)
                    .or_eager(resource_exists_and_changed::<strata_foliage::FoliageSky>),
            ),
        )
        // The shade's colour (visual.md F4): the grade's `LookUniform` to
        // each material's own parameters.
        .add_systems(
            Update,
            sync_shade_tint.run_if(
                resource_exists_and_changed::<strata_atmosphere::grade::LookUniform>
                    .or_eager(
                        resource_exists_and_changed::<strata_atmosphere::grade::BuildingGradient>,
                    )
                    .or_eager(
                        resource_exists_and_changed::<
                            bevy::asset::Assets<ashlar_bevy::anti_tiling::AntiTilingMaterial>,
                        >,
                    ),
            ),
        )
        // The water (lighting plan phase 5a).
        .add_systems(
            Update,
            sync_water.run_if(
                resource_changed::<RenderLook>
                    .or_eager(resource_exists_and_changed::<strata_terrain::water::WaterLook>),
            ),
        )
        // Irradiance probes (lighting plan phase 3b).
        .add_systems(Update, sync_probe_clouds)
        // Grass against the low sun (lighting plan phase 5b).
        .add_systems(
            Update,
            sync_grass_rim.run_if(
                resource_changed::<RenderLook>
                    .or_eager(resource_exists_and_changed::<strata_foliage::FoliageSky>),
            ),
        )
        // The ground painted (ticket 0006, pass 1).
        .add_systems(
            Update,
            sync_ground.run_if(
                resource_changed::<RenderLook>
                    .or_eager(resource_exists_and_changed::<strata_terrain::TerrainLook>),
            ),
        );
}

// The ground painted (ticket 0006, pass 1).
/// The terrain's parameters with the look's ground fields written in. The
/// air is a share of the terrain's own, so the panel has one slider for
/// its two terms.
#[allow(
    clippy::large_types_passed_by_value,
    reason = "the struct update consumes `params`, and one call a frame"
)]
fn ground_params(
    look: &RenderLook,
    params: strata_terrain::material::TerrainParams,
) -> strata_terrain::material::TerrainParams {
    let air = strata_terrain::material::TerrainParams::default();
    strata_terrain::material::TerrainParams {
        colour_lush: look.ground_colour,
        colour_warm: look.ground_warmth,
        near_tint: look.ground_near_tint,
        near_value: look.ground_near_value,
        grain: look.ground_grain,
        normal_strength: look.ground_normal,
        matte: look.ground_matte,
        seam_shade: look.ground_seam,
        air_lift: (air.air_lift * look.ground_air).min(1.0),
        air_desaturate: (air.air_desaturate * look.ground_air).min(1.0),
        ..params
    }
}

/// Carries the look's ground to the terrain's look, which the terrain
/// material carries on to the drawn material. Written only when it differs.
fn sync_ground(look: Res<RenderLook>, terrain: Option<ResMut<strata_terrain::TerrainLook>>) {
    if let Some(mut terrain) = terrain {
        let wanted = ground_params(&look, terrain.params);
        if terrain.params != wanted {
            terrain.params = wanted;
        }
    }
}

// Grass against the low sun (lighting plan phase 5b).
/// Carries the look's grass rim to the foliage, which does not see the
/// atmosphere crate's look. Written only when it differs, so an unchanged
/// look leaves the foliage's resource untouched.
#[allow(
    clippy::float_cmp,
    reason = "a copy is exact: any difference is a stale copy"
)]
fn sync_grass_rim(look: Res<RenderLook>, foliage: Option<ResMut<strata_foliage::FoliageSky>>) {
    if let Some(mut foliage) = foliage
        && foliage.grass_rim != look.grass_rim
    {
        foliage.grass_rim = look.grass_rim;
    }
}

// The water (lighting plan phase 5a).
/// Carries the look's water reflection and foam to the sea's material,
/// which does not see the atmosphere crate's look. Written only when it
/// differs, so an unchanged look leaves the water's resource untouched.
fn sync_water(look: Res<RenderLook>, water: Option<ResMut<strata_terrain::water::WaterLook>>) {
    let wanted = strata_terrain::water::WaterLook {
        reflection: look.water_reflection,
        foam: look.water_foam,
    };
    if let Some(mut water) = water
        && *water != wanted
    {
        *water = wanted;
    }
}

// Irradiance probes (lighting plan phase 3b).
/// Carries the cloud layer's shade to the terrain's irradiance probes,
/// which do not see the atmosphere crate: the sun's bounce goes with the
/// sun under a cloud's shadow. Written only when it differs.
#[allow(
    clippy::float_cmp,
    reason = "a copy is exact: any difference is a stale copy"
)]
fn sync_probe_clouds(
    sky: Res<SkySettings>,
    veil: Option<Res<strata_atmosphere::clouds::SunVeil>>,
    probes: Option<ResMut<strata_terrain::probes::ProbeClouds>>,
) {
    let Some(mut probes) = probes else {
        return;
    };
    let wanted = strata_terrain::probes::ProbeClouds {
        sun_clear: veil.map_or(1.0, |v| v.clear),
        mean_clear: 1.0 - sky.cloud_cover.clamp(0.0, 1.0),
    };
    if *probes != wanted {
        *probes = wanted;
    }
}

// Sky visibility (lighting plan phase 3a).
/// Carries the look's sky visibility to the terrain material and the
/// foliage, which do not see the atmosphere crate's look. Written only when
/// it differs, so an unchanged look touches neither.
#[allow(
    clippy::float_cmp,
    reason = "a copy is exact: any difference is a stale copy"
)]
fn sync_sky_visibility(
    look: Res<RenderLook>,
    terrain: Option<ResMut<strata_terrain::TerrainLook>>,
    foliage: Option<ResMut<strata_foliage::FoliageSky>>,
) {
    let strength = look.sky_visibility;
    if let Some(mut terrain) = terrain
        && terrain.params.sky_visibility != strength
    {
        terrain.params.sky_visibility = strength;
    }
    if let Some(mut foliage) = foliage
        && foliage.strength != strength
    {
        foliage.strength = strength;
    }
}

/// Carries the grade's shade tint to the terrain material, the foliage and
/// the anti-tiled building materials, which do not see the atmosphere
/// crate. Written only where it differs, and a building material that is
/// new since the last change takes the current tint on the frame it is added.
#[allow(
    clippy::float_cmp,
    reason = "a copy is exact: any difference is a stale copy"
)]
fn sync_shade_tint(
    uniform: Option<Res<strata_atmosphere::grade::LookUniform>>,
    terrain: Option<ResMut<strata_terrain::TerrainLook>>,
    foliage: Option<ResMut<strata_foliage::FoliageSky>>,
    buildings: Option<ResMut<bevy::asset::Assets<ashlar_bevy::anti_tiling::AntiTilingMaterial>>>,
    gradient: Option<Res<strata_atmosphere::grade::BuildingGradient>>,
) {
    let Some(uniform) = uniform else { return };
    let tint = uniform.shade_tint;
    let wide = tint.extend(1.0);
    if let Some(mut terrain) = terrain
        && terrain.params.shade_tint != wide
    {
        terrain.params.shade_tint = wide;
    }
    if let Some(mut foliage) = foliage
        && foliage.shade_tint != tint
    {
        foliage.shade_tint = tint;
    }
    let Some(mut buildings) = buildings else {
        return;
    };
    // The height gradient's dials (assets/look/buildings.ron): the shader
    // holds none, and a material built since the last change takes them here.
    let (height, warm) = gradient.map_or((None, None), |g| {
        let look = g.0;
        (
            Some(bevy::math::Vec4::new(
                look.grime_m,
                look.grime,
                look.reach_m,
                look.lift,
            )),
            Some(bevy::math::Vec4::new(
                look.warm[0],
                look.warm[1],
                look.warm[2],
                1.0,
            )),
        )
    });
    let stale: Vec<_> = buildings
        .iter()
        .filter(|(_, m)| {
            m.extension.uniform.shade != wide
                || height.is_some_and(|h| m.extension.uniform.gradient != h)
                || warm.is_some_and(|w| m.extension.uniform.warm != w)
        })
        .map(|(id, _)| id)
        .collect();
    for id in stale {
        if let Some(mut material) = buildings.get_mut(id) {
            material.extension.uniform.shade = wide;
            if let Some(height) = height {
                material.extension.uniform.gradient = height;
            }
            if let Some(warm) = warm {
                material.extension.uniform.warm = warm;
            }
        }
    }
}

fn seed(store: &mut ValueStore, file: &LookFile) {
    store.insert("render.preset", file.look.preset.clone());
    store.insert(SHADOW_FILTER, file.look.shadow_filter.as_str());
    for n in NUMBERS {
        store.insert(n.key, (n.get)(file));
    }
    for f in FLAGS {
        store.insert(f.key, (f.get)(file));
    }
}

fn setup(
    mut screens: ResMut<Screens>,
    mut store: ResMut<ValueStore>,
    mut rules: ResMut<ValueRules>,
) {
    let mut spec = spec();
    rules.0.extend(spec.rules().0);
    let preset = spec.tabs[0].rows.remove(0).node();
    let compare = spec.tabs[0].rows.remove(0).node();
    store.insert(spec.tab_key(), "look");
    store.insert("render.compare", false);
    let mut screen = spec.screen_def();
    screen.inherits = None;
    screen.presentation = Presentation {
        mode: PresentationMode::Modal,
        scrim: Some(false),
        ..Default::default()
    };
    if let UiNodeDef::Panel {
        layout,
        children,
        tags,
        ..
    } = &mut screen.root
    {
        layout.width = Some(Length::Px(500.0));
        layout.max_height = Some(Length::Percent(96.0));
        layout.place = Some(Place {
            anchor: NineAnchor::TopRight,
            offset: Vec2::new(-12.0, 12.0),
        });
        *tags = Tags::new().with(Tags::TEST_ID, "render.panel");
        children.insert(
            0,
            UiNodeDef::Text {
                key: LocKey("render.title".into()),
                style: TextRole::Title,
                opts: TextOpts::default(),
                tags: Tags::new(),
            },
        );
        children.insert(1, preset);
        children.insert(2, compare);
        children.push(UiNodeDef::Text {
            key: LocKey("render.status".into()),
            style: TextRole::Caption,
            opts: TextOpts::default(),
            tags: Tags::new().with(Tags::TEST_ID, "render.status"),
        });
        children.push(UiNodeDef::Panel {
            role: slotted::theme::Role::new("invisible"),
            layout: Layout {
                direction: LayoutDirection::Row,
                gap: 1.0,
                ..Default::default()
            },
            children: vec![
                button("render.reset"),
                button("render.export"),
                button("render.close"),
            ],
            tags: Tags::new(),
        });
    }
    screens.register(screen);
}

/// Opens the same screen used by F6, for reproducible UI screenshots.
pub fn open(commands: &mut Commands) {
    commands.queue(|world: &mut World| {
        if world.resource::<ScreenStack>().top().is_some() {
            return;
        }
        if let Some(def) = world.resource::<Screens>().get(&kind()).cloned() {
            push_screen(&mut world.commands(), def, None);
        }
    });
}

/// F6 opens the panel only with the `dev` feature: the panel's resources and
/// systems stay (the render look is read by the graphics presets), but a
/// shipped or web build has no key to the laboratory.
#[cfg(feature = "dev")]
fn toggle(keys: Res<ButtonInput<KeyCode>>, stack: Res<ScreenStack>, mut commands: Commands) {
    if !keys.just_pressed(KEY) {
        return;
    }
    if stack.top().is_some_and(|s| s.kind == kind()) {
        pop_screen(&mut commands);
    } else if stack.top().is_none() {
        open(&mut commands);
    }
}

#[cfg_attr(
    target_arch = "wasm32",
    allow(
        unused_variables,
        reason = "only the native export reads the settings file"
    )
)]
fn actions(
    mut choices: MessageReader<MenuChoice>,
    settings_file: Option<Res<super::SettingsFile>>,
    mut state: ResMut<PanelState>,
    mut store: ResMut<ValueStore>,
    mut commands: Commands,
) {
    for choice in choices.read() {
        match choice.id.as_str() {
            "render.close" => pop_screen(&mut commands),
            "render.reset" => {
                if let Ok(file) = LookFile::preset(&state.edited.look.preset) {
                    state.edited = file;
                    seed(&mut store, &state.edited);
                    store.insert("render.compare", false);
                }
            }
            // A developer's export: native only, there are no files on the web,
            // where the screen offers nothing to write.
            #[cfg(not(target_arch = "wasm32"))]
            "render.export" => {
                let path = settings_file
                    .as_ref()
                    .map_or_else(super::default_settings_path, |f| f.0.clone())
                    .with_file_name("render-look.ron");
                let result = state.edited.to_ron().and_then(|text| {
                    if let Some(parent) = path.parent() {
                        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
                    }
                    std::fs::write(&path, text).map_err(|e| e.to_string())
                });
                state.status = match result {
                    Ok(()) => format!("Exported {}", path.display()),
                    Err(e) => format!("Export failed: {e}"),
                };
            }
            _ => {}
        }
    }
}

fn apply_values(
    mut state: ResMut<PanelState>,
    graphics: Res<super::graphics::Active>,
    mut store: ResMut<ValueStore>,
    mut sky: ResMut<SkySettings>,
    mut look: ResMut<RenderLook>,
) {
    if !state.initialized {
        state.edited = LookFile {
            sky: *sky,
            look: look.clone(),
        };
        seed(&mut store, &state.edited);
        state.initialized = true;
    }
    if state.version == store.version() {
        return;
    }
    let selected = store
        .get("render.preset")
        .and_then(Value::as_str)
        .unwrap_or("baseline");
    if selected != state.edited.look.preset
        && let Ok(file) = LookFile::preset(selected)
    {
        state.edited = file;
        seed(&mut store, &state.edited);
    }
    for n in NUMBERS {
        if let Some(v) = store.get(n.key).and_then(Value::as_f64) {
            (n.set)(&mut state.edited, v);
        }
    }
    for f in FLAGS {
        if let Some(v) = store.get(f.key).and_then(Value::as_bool) {
            (f.set)(&mut state.edited, v);
        }
    }
    if let Some(filter) = store
        .get(SHADOW_FILTER)
        .and_then(Value::as_str)
        .and_then(|name| ShadowFilter::try_from(name.to_owned()).ok())
    {
        state.edited.look.shadow_filter = filter;
    }
    if let Err(error) = state.edited.validate() {
        state.status = error;
        state.version = store.version();
        return;
    }
    let compare = store
        .get("render.compare")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let mut effective = if compare {
        LookFile::default()
    } else {
        state.edited.clone()
    };
    // The graphics preset limits what the panel edits, never its own copy.
    graphics.preset.limit(&mut effective);
    if *sky != effective.sky {
        *sky = effective.sky;
    }
    if *look != effective.look {
        *look = effective.look;
    }
    state.version = store.version();
}

fn status(
    state: Res<PanelState>,
    look: Res<RenderLook>,
    study: Res<StudySettings>,
    cameras: Query<Option<&AaOverride>, With<strata_atmosphere::SkyCamera>>,
    index: Res<TestIdIndex>,
    mut labels: Query<&mut Text>,
) {
    let suppressed = cameras
        .iter()
        .any(|aa| look.ssao && !effective_aa(&look, aa.copied()).2);
    let text = format!(
        "{}\n{}\n{}",
        if suppressed {
            "SSAO inactive: MSAA override selected."
        } else {
            "F3: performance. F6 / Escape: close."
        },
        if study.scene.is_none() {
            "Local mist needs --study-scene."
        } else {
            "Mist remains at the study location."
        },
        state.status
    );
    for entity in index.entities("render.status") {
        if let Ok(mut label) = labels.get_mut(*entity)
            && label.0 != text
        {
            label.0.clone_from(&text);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The look's ground is the terrain's own default, so a fresh look
    /// leaves the terrain as its crate tuned it (ticket 0006, pass 1).
    #[test]
    fn the_look_s_ground_is_the_terrain_s_default() {
        let params = strata_terrain::material::TerrainParams::default();
        assert_eq!(ground_params(&RenderLook::default(), params), params);
    }
}
