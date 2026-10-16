//! The graphics preset (ticket 0015, step 3): Low, Medium or High on the
//! Display tab, chosen once from the adapter and kept once made.
//!
//! A preset is a set of limits from `assets/look/graphics.ron` laid over the
//! look in force, never a second copy of it: the render panel's writer
//! ([`super::render`]) applies [`Preset::limit`] to what it writes, so High,
//! which limits nothing, is exactly today's look, and a flag such as
//! `--no-shadows` or an edit in the panel still holds under every preset.
//! The foliage distance is the one limit that is not a look field: it
//! multiplies the slider ([`super::settings::apply_foliage_distance`]), so
//! the slider stays the player's own. Every limit applies at once; nothing
//! here needs a restart.

use bevy::ecs::prelude::*;
use bevy::light::DirectionalLightShadowMap;
use serde::Deserialize;
use slotted::menu::SettingsStorage;
use slotted::menu::settings::{SettingsApplied, SettingsReset};
use slotted::ui::{Value, ValueChanged, ValueStore};
use strata_atmosphere::look::LookFile;

/// The presets' file under the asset root.
pub const FILE: &str = "look/graphics.ron";

/// The presets as shipped, compiled in so a test without the asset
/// directory reads the same; the file under the asset root takes over at
/// startup.
const SHIPPED: &str = include_str!("../../../../assets/look/graphics.ron");

/// The three presets, in the order the tab lists them.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum Graphics {
    /// For an integrated GPU.
    Low,
    /// Between the two.
    Medium,
    /// Today's look.
    #[default]
    High,
}

impl Graphics {
    /// Every preset, Low first.
    pub const ALL: [Self; 3] = [Self::Low, Self::Medium, Self::High];

    /// The id the settings file and `--graphics` spell it by.
    #[must_use]
    pub const fn id(self) -> &'static str {
        match self {
            Self::Low => "low",
            Self::Medium => "medium",
            Self::High => "high",
        }
    }

    /// The preset an id names.
    #[must_use]
    pub fn from_id(id: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|g| g.id() == id)
    }
}

/// The limits one preset lays over the look.
#[derive(Debug, Clone, Copy, PartialEq, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Preset {
    /// Whether the sun may cast shadows at all.
    pub shadows: bool,
    /// The most texels a side of the shadow map may have.
    pub shadow_map_size: Option<usize>,
    /// The most cascades the sun's shadow may have.
    pub shadow_cascades: Option<u32>,
    /// The farthest the shadow cascades may reach, metres.
    pub shadow_distance: Option<f32>,
    /// Whether antialiasing may be drawn.
    pub antialiasing: bool,
    /// The share of the foliage distance the slider's value is, percent.
    pub foliage_percent: f32,
}

impl Default for Preset {
    fn default() -> Self {
        Self {
            shadows: true,
            shadow_map_size: None,
            shadow_cascades: None,
            shadow_distance: None,
            antialiasing: true,
            foliage_percent: 100.0,
        }
    }
}

impl Preset {
    /// Lays the limits over `file`: a value already under a limit stays.
    pub fn limit(&self, file: &mut LookFile) {
        let sky = &mut file.sky;
        sky.shadows &= self.shadows;
        if let Some(size) = self.shadow_map_size {
            sky.shadow_map_size = sky.shadow_map_size.min(size);
        }
        if let Some(count) = self.shadow_cascades {
            sky.shadow_cascades = sky.shadow_cascades.min(count);
        }
        if let Some(distance) = self.shadow_distance {
            sky.shadow_distance = sky.shadow_distance.min(distance);
        }
        file.look.no_aa |= !self.antialiasing;
    }

    /// The foliage distance as a multiple of the slider's.
    #[must_use]
    pub fn foliage_scale(&self) -> f32 {
        self.foliage_percent / 100.0
    }
}

/// The file's whole contents.
#[derive(Resource, Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Table {
    /// For an integrated GPU.
    pub low: Preset,
    /// Between the two.
    pub medium: Preset,
    /// Today's look.
    pub high: Preset,
    /// `vendor/architecture` prefixes of a browser's adapter that are
    /// integrated parts.
    pub web_integrated: Vec<String>,
    /// Prefixes of adapters that may be integrated or discrete and are taken
    /// for the middle preset until the player says otherwise.
    pub web_uncertain: Vec<String>,
}

impl Table {
    /// Parses the file's text and checks every preset leaves a valid look.
    ///
    /// # Errors
    /// RON that does not fit, or a limit that makes the look invalid.
    pub fn from_ron(text: &str) -> Result<Self, String> {
        let table: Self = ron::from_str(text).map_err(|e| e.to_string())?;
        for graphics in Graphics::ALL {
            let mut file = LookFile::default();
            table.of(graphics).limit(&mut file);
            file.validate()
                .map_err(|e| format!("{}: {e}", graphics.id()))?;
        }
        Ok(table)
    }

    /// The presets as shipped.
    ///
    /// # Panics
    /// If the compiled-in file is invalid: a test holds that it is not.
    #[must_use]
    pub fn shipped() -> Self {
        Self::from_ron(SHIPPED).expect("assets/look/graphics.ron is valid")
    }

    /// One preset's limits.
    #[must_use]
    pub const fn of(&self, graphics: Graphics) -> &Preset {
        match graphics {
            Graphics::Low => &self.low,
            Graphics::Medium => &self.medium,
            Graphics::High => &self.high,
        }
    }

    /// The preset a browser's `vendor/architecture` starts on: Low for a
    /// listed integrated part, Medium for one that may be either, High else.
    #[must_use]
    pub fn web_pick(&self, adapter: &str) -> Graphics {
        let adapter = adapter.to_ascii_lowercase();
        let any = |list: &[String]| {
            list.iter()
                .any(|prefix| adapter.starts_with(prefix.as_str()))
        };
        if any(&self.web_integrated) {
            Graphics::Low
        } else if any(&self.web_uncertain) {
            Graphics::Medium
        } else {
            Graphics::High
        }
    }
}

impl Default for Table {
    fn default() -> Self {
        Self::shipped()
    }
}

/// What the browser's WebGPU reports of the adapter, `vendor/architecture`,
/// put in by the page: wgpu on the web says only `Other`.
#[derive(Resource, Debug, Clone, PartialEq, Eq)]
pub struct BrowserAdapter(pub String);

/// A preset the command line named (`--graphics`): it wins for the run and
/// is not kept.
#[derive(Resource, Debug, Clone, Copy, PartialEq, Eq)]
pub struct Forced(pub Graphics);

/// The preset in force and its limits, read from the store each frame.
#[derive(Resource, Debug, Clone, Copy, PartialEq, Default)]
pub struct Active {
    /// Which preset.
    pub graphics: Graphics,
    /// Its limits.
    pub preset: Preset,
}

/// `Startup`: the file under the asset root over the compiled-in presets.
pub fn load_table(root: Option<Res<strata_sim::WorldAssetRoot>>, mut table: ResMut<Table>) {
    let Some(root) = root else { return };
    match root.0.read_string(FILE) {
        Ok(text) => match Table::from_ron(&text) {
            Ok(read) => *table = read,
            Err(error) => tracing::warn!("{FILE}: {error}; the shipped presets stay"),
        },
        Err(error) => tracing::debug!("{FILE}: {error}; the shipped presets stay"),
    }
}

/// The preset an adapter starts on: wgpu's own device type where it knows
/// one (a software rasteriser or a virtual GPU is the slowest case, so Low),
/// the page's report where it does not (a browser).
fn pick(
    info: &bevy::render::renderer::RenderAdapterInfo,
    web: Option<&BrowserAdapter>,
    table: &Table,
) -> Graphics {
    // wgpu's `DeviceType` is not re-exported by Bevy, and its Debug is the
    // variant's name.
    match format!("{:?}", info.device_type).as_str() {
        "IntegratedGpu" | "Cpu" | "VirtualGpu" => Graphics::Low,
        "Other" => web.map_or(Graphics::High, |adapter| table.web_pick(&adapter.0)),
        _ => Graphics::High,
    }
}

/// `Update`, once the settings are seeded: when the file holds no choice
/// yet, one made from the adapter and kept, and again after the tab's Reset
/// (which writes the spec's High over an integrated part's start). A
/// player's choice, once in the file, is never touched. A `--graphics`
/// preset is not written anywhere: [`resolve`] reads it first.
#[allow(
    clippy::too_many_arguments,
    reason = "a Bevy system's parameters are its dependencies"
)]
pub fn decide(
    applied: Option<Res<SettingsApplied>>,
    forced: Option<Res<Forced>>,
    storage: Option<Res<SettingsStorage>>,
    info: Option<Res<bevy::render::renderer::RenderAdapterInfo>>,
    web: Option<Res<BrowserAdapter>>,
    table: Res<Table>,
    mut store: ResMut<ValueStore>,
    mut resets: MessageReader<SettingsReset>,
    mut changed: MessageWriter<ValueChanged>,
    mut done: Local<bool>,
    mut after_reset: Local<bool>,
) {
    // The reset's own writes land after this frame's, so the pick follows.
    let reapply = std::mem::replace(&mut *after_reset, resets.read().count() > 0);
    if applied.is_none() || forced.is_some() {
        return;
    }
    // No adapter yet (a headless run has none, and never will).
    let Some(info) = info else { return };
    if *done && !reapply {
        return;
    }
    let first = !*done;
    *done = true;
    if first
        && storage
            .and_then(|storage| storage.0.load())
            .is_some_and(|saved| saved.values.contains_key(super::settings::keys::GRAPHICS))
    {
        return;
    }
    let pick = pick(&info, web.as_deref(), &table);
    tracing::info!(adapter = ?info.0, preset = pick.id(), "graphics preset chosen from the adapter");
    store.insert(super::settings::keys::GRAPHICS, pick.id());
    changed.write(ValueChanged {
        key: super::settings::keys::GRAPHICS.to_owned(),
        old: None,
        new: Value::Text(pick.id().to_owned()),
        source: None,
    });
}

/// `Update`, before the render panel's writer: the store's choice as the
/// limits in force.
pub fn resolve(
    forced: Option<Res<Forced>>,
    store: Res<ValueStore>,
    table: Res<Table>,
    mut active: ResMut<Active>,
) {
    let graphics = forced.map_or_else(
        || {
            store
                .get(super::settings::keys::GRAPHICS)
                .and_then(Value::as_str)
                .and_then(Graphics::from_id)
                .unwrap_or_default()
        },
        |forced| forced.0,
    );
    let preset = *table.of(graphics);
    if active.graphics != graphics || active.preset != preset {
        *active = Active { graphics, preset };
    }
}

/// Puts the presets' resources and systems in.
pub(crate) fn register(app: &mut bevy::app::App) {
    use bevy::app::{Startup, Update};
    app.init_resource::<Table>()
        .init_resource::<Active>()
        .add_systems(Startup, load_table)
        .add_systems(Update, (decide, resolve).chain());
}

/// `Update`: the shadow map's side follows the look's, which the preset
/// limits; Bevy reallocates it when the resource changes.
pub fn apply_shadow_map(
    sky: Res<strata_atmosphere::SkySettings>,
    map: Option<ResMut<DirectionalLightShadowMap>>,
) {
    let Some(mut map) = map else { return };
    if map.size != sky.shadow_map_size {
        map.size = sky.shadow_map_size;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bevy::ecs::system::RunSystemOnce;

    #[test]
    fn the_shipped_file_is_valid_and_high_limits_nothing() {
        let table = Table::shipped();
        let mut file = LookFile::default();
        table.high.limit(&mut file);
        assert_eq!(file, LookFile::default());
    }

    #[test]
    fn low_limits_more_than_medium_and_never_raises_a_value() {
        let table = Table::shipped();
        let (mut low, mut medium) = (LookFile::default(), LookFile::default());
        table.low.limit(&mut low);
        table.medium.limit(&mut medium);
        assert!(low.sky.shadow_map_size < medium.sky.shadow_map_size);
        assert!(low.look.no_aa && !medium.look.no_aa);
        let mut small = LookFile::default();
        small.sky.shadow_map_size = 512;
        table.medium.limit(&mut small);
        assert_eq!(small.sky.shadow_map_size, 512, "a limit is a ceiling");
        let mut dark = LookFile::default();
        dark.sky.shadows = false;
        table.high.limit(&mut dark);
        assert!(!dark.sky.shadows, "a look with no shadows keeps none");
    }

    #[test]
    fn a_browser_adapter_starts_on_the_preset_its_prefix_names() {
        let table = Table::shipped();
        assert_eq!(table.web_pick("intel/gen-12lp"), Graphics::Low);
        assert_eq!(table.web_pick("amd/rdna-2"), Graphics::Medium);
        assert_eq!(table.web_pick("fallback/"), Graphics::Low);
        assert_eq!(table.web_pick("nvidia/lovelace"), Graphics::High);
    }

    #[test]
    fn a_forced_preset_rules_the_run_and_never_reaches_the_store() {
        let mut world = World::new();
        world.insert_resource(Table::shipped());
        world.insert_resource(Active::default());
        world.insert_resource(ValueStore::default());
        world.insert_resource(Forced(Graphics::Low));
        world.run_system_once(resolve).expect("the system runs");
        assert_eq!(world.resource::<Active>().graphics, Graphics::Low);
        assert!(
            world
                .resource::<ValueStore>()
                .get(super::super::settings::keys::GRAPHICS)
                .is_none()
        );
    }
}
