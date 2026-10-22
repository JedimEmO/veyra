//! The HUD on slotted's layer stack: the debug statistics top left, the
//! reticle in the middle, a hotbar along the bottom.
//!
//! A HUD layer is a named, ordered, anchored tree of UI nodes that the
//! plugin spawns, hides while a screen is up if asked, and that a mod can
//! insert above or below by name. Building the overlay as layers rather
//! than as one `bevy_ui` node (the Phase 3 HUD) is what makes the debug
//! statistics the same kind of thing as the health bar a later phase adds,
//! and what lets a position editor and a mod move either. The three
//! layers here take the built-in ids `debug`, `crosshair` and `hotbar`, so
//! `insert_above("hotbar", ..)` from a mod means what it says.
//!
//! The statistics use the same counters as the log ([`FrameStats`]),
//! over a shorter window so the HUD reacts, plus the network line (the
//! port's [`NetRole`] and [`NetStatus`]: which
//! end of the wire this is, the round trip, the players in sight and the
//! prediction corrections), rebuilt a few times a second into one text
//! node through a `HudUpdate`; `F3` shows and hides that layer and
//! nothing else. It starts hidden: a player has no use for frame times, a
//! screenshot is of the game rather than of its counters, and the numbers
//! still reach the log and the benches through [`FrameStats`] either way.
//! The game's `--stats` flag starts it shown.
//! The hotbar is eight empty slots over an [`Inventory`] entity behind a
//! menu with no screen, which is what a HUD hotbar is in slotted's model:
//! the inventory exists from now on, and the first item that lands in it
//! shows up without another line here. The reticle is the plugin's own
//! crosshair layer, hidden while a screen is open, and the settings' HUD
//! toggle hides it and the hotbar together.

use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::sync::Arc;

use bevy::ecs::prelude::*;
use bevy::input::ButtonInput;
use bevy::input::keyboard::KeyCode;
use bevy::math::Vec2;
use bevy::time::Time;
use slotted::ecs::{Inventory, MenuIdAllocator, open_menu};
use slotted::model::{Actor, InventoryRef, MenuDef, SlotBehaviour};
use slotted::theme::roles;
use slotted::ui::hud::builtin;
use slotted::ui::{
    HudAnchor, HudLayerDef, HudLayerId, HudLayers, HudMenu, HudUpdate, HudValue, Layout, LocKey,
    NineAnchor, Padding, Tags, TextOpts, TextRole, UiNodeDef,
};
use strata_net::{NetRole, NetStatus};
use strata_stream::LoadedRegions;
use strata_terrain::TerrainStats;
use veyra_sim::{Observers, RenderOrigin, TimeOfDay};

use crate::stats::FrameStats;

/// Toggles the debug statistics layer.
pub const TOGGLE_KEY: KeyCode = KeyCode::F3;

/// How often the text is rebuilt. Positions change every frame, but a
/// number that changes every frame cannot be read.
const REFRESH_SECS: f32 = 0.25;

/// Slots in the hotbar.
pub const HOTBAR_SLOTS: u16 = 8;

/// The `test_id` of the statistics text node inside the debug layer, the
/// path a `HudUpdate` writes to.
pub const STATS_ID: &str = "stats";

/// The debug layer: slotted's built-in `debug` slot, replaced.
#[must_use]
pub fn debug_layer() -> HudLayerId {
    builtin::DEBUG
}

/// The hotbar layer: slotted's built-in `hotbar` slot, replaced.
#[must_use]
pub fn hotbar_layer() -> HudLayerId {
    builtin::HOTBAR
}

/// The reticle: slotted's built-in `crosshair` layer, kept as it is.
#[must_use]
pub fn crosshair_layer() -> HudLayerId {
    builtin::CROSSHAIR
}

/// Whether the debug statistics are shown. A resource rather than the
/// layer's own flag so the state survives the layer being respawned, and
/// a test can read it. Off unless the app inserts it on (`--stats`) or F3
/// turns it on.
#[derive(Resource, Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct HudVisible(pub bool);

/// The entities behind the hotbar: the menu the slot grid binds to and
/// the one inventory it shows.
#[derive(Resource, Debug, Clone, Copy, PartialEq, Eq)]
pub struct HotbarInventory {
    /// The `OpenMenu` entity, also `HudMenu`.
    pub menu: Entity,
    /// The `Inventory` entity with [`HOTBAR_SLOTS`] slots.
    pub inventory: Entity,
}

/// Registers the layers and the hotbar's inventory at startup. Only
/// called once the UI is wired: everything here needs slotted's resources.
pub(crate) fn register(app: &mut bevy::app::App) {
    app.init_resource::<HudVisible>()
        .add_systems(
            bevy::app::Startup,
            (register_layers, spawn_hotbar_inventory),
        )
        .add_systems(
            bevy::app::Update,
            hide_empty_hotbar.after(crate::ui::settings::apply_hud_toggle),
        );
}

/// Hides the hotbar layer while its inventory holds nothing. An empty
/// panel at the bottom centre is noise in every mode, the study scenes
/// with no combat included, so the rule lives with the layer and not
/// with any one game mode. It runs after the settings' HUD toggle, which
/// would otherwise show the layer again each frame it flips.
fn hide_empty_hotbar(
    hotbar: Option<Res<HotbarInventory>>,
    inventories: Query<&Inventory>,
    mut layers: ResMut<HudLayers>,
) {
    let empty = hotbar
        .and_then(|hotbar| inventories.get(hotbar.inventory).ok())
        .is_none_or(|inventory| inventory.slots().iter().all(Option::is_none));
    let id = hotbar_layer();
    if empty && layers.get(&id).is_some_and(|layer| layer.visible) {
        layers.set_visible(&id, false);
    }
}

/// The debug layer: one `hud.panel` in the top left corner holding one
/// text node, unwrapped so a line is as wide as it needs to be. Not hidden
/// by a screen: the numbers are for reading while something is wrong,
/// which includes while a menu is open.
#[must_use]
pub fn debug_layer_def() -> HudLayerDef {
    HudLayerDef {
        id: debug_layer(),
        anchor: HudAnchor {
            anchor: NineAnchor::TopLeft,
            offset: Vec2::new(8.0, 8.0),
            scale: 1.0,
        },
        tree: UiNodeDef::Panel {
            role: roles::HUD_PANEL,
            layout: Layout {
                padding: Padding::from(1.0),
                ..Layout::default()
            },
            children: vec![UiNodeDef::Text {
                key: LocKey("strata.hud.stats".to_owned()),
                style: TextRole::Body,
                opts: TextOpts {
                    wrap: false,
                    ..TextOpts::default()
                },
                tags: Tags::new().with(Tags::TEST_ID, STATS_ID),
            }],
            tags: Tags::new().with(Tags::TEST_ID, "debug_hud"),
        },
        visible: true,
        hide_with_screen: false,
    }
}

/// The hotbar layer: a `hud.panel` at the bottom centre holding an
/// [`HOTBAR_SLOTS`] by one slot grid over the menu's first inventory. It
/// stands down while a screen is open, because an inventory screen draws
/// the same row itself.
#[must_use]
pub fn hotbar_layer_def() -> HudLayerDef {
    HudLayerDef {
        id: hotbar_layer(),
        anchor: HudAnchor {
            anchor: NineAnchor::Bottom,
            offset: Vec2::new(0.0, -24.0),
            scale: 1.0,
        },
        tree: UiNodeDef::Panel {
            role: roles::HUD_PANEL,
            layout: Layout {
                padding: Padding::from(1.0),
                ..Layout::default()
            },
            children: vec![UiNodeDef::SlotGrid {
                inventory: InventoryRef::new(0),
                cols: HOTBAR_SLOTS,
                rows: 1,
                first: 0,
                tags: Tags::new()
                    .with("region", "hotbar")
                    .with(Tags::TEST_ID, "hotbar"),
            }],
            tags: Tags::new().with(Tags::TEST_ID, "hotbar_panel"),
        },
        visible: true,
        hide_with_screen: true,
    }
}

/// The menu behind the hotbar: [`HOTBAR_SLOTS`] normal slots over one
/// inventory, which the number keys swap with.
#[must_use]
pub fn hotbar_menu_def() -> MenuDef {
    let mut def = MenuDef::new();
    let row = def.add_slots(MenuDef::CONTAINER, HOTBAR_SLOTS, SlotBehaviour::Normal);
    def.hotbar = row.iter().collect();
    def
}

/// `Startup`: the two layers take the built-in slots of the same name,
/// keeping their place in the order; with the built-ins off they are
/// appended.
fn register_layers(mut layers: ResMut<HudLayers>, visible: Res<HudVisible>) {
    let mut debug = debug_layer_def();
    debug.visible = visible.0;
    for def in [debug, hotbar_layer_def()] {
        let id = def.id.clone();
        if layers.replace(&id, def.clone()).is_err() {
            layers.register(def);
        }
    }
}

/// `Startup`: the empty inventory, the menu over it, and the binding that
/// makes the hotbar layer draw it.
fn spawn_hotbar_inventory(mut commands: Commands, mut ids: ResMut<MenuIdAllocator>) {
    let inventory = commands
        .spawn(Inventory::new(usize::from(HOTBAR_SLOTS)))
        .id();
    let menu = open_menu(
        &mut commands,
        &mut ids,
        Arc::new(hotbar_menu_def()),
        vec![inventory],
        Actor::SURVIVAL,
    );
    commands.insert_resource(HudMenu(Some(menu)));
    commands.insert_resource(HotbarInventory { menu, inventory });
}

/// `F3` flips [`HudVisible`] and the debug layer with it.
pub fn toggle_hud(
    keys: Res<ButtonInput<KeyCode>>,
    mut visible: ResMut<HudVisible>,
    mut layers: ResMut<HudLayers>,
) {
    if !keys.just_pressed(TOGGLE_KEY) {
        return;
    }
    visible.0 = !visible.0;
    layers.set_visible(&debug_layer(), visible.0);
}

/// Rebuilds the text a few times a second while the layer is shown and
/// spawned.
#[allow(
    clippy::too_many_arguments,
    reason = "the HUD reads every counter there is; that is its job"
)]
pub fn refresh_hud(
    time: Res<Time>,
    visible: Res<HudVisible>,
    layers: Res<HudLayers>,
    stats: Res<FrameStats>,
    loaded: Res<LoadedRegions>,
    terrain: Option<Res<TerrainStats>>,
    foliage: Option<Res<strata_foliage::FoliageStats>>,
    observers: Res<Observers>,
    origin: Res<RenderOrigin>,
    time_of_day: Res<TimeOfDay>,
    role: Option<Res<NetRole>>,
    net: Option<Res<NetStatus>>,
    mut since: Local<f32>,
    mut updates: MessageWriter<HudUpdate>,
) {
    if !visible.0 || layers.root(&debug_layer()).is_none() {
        return;
    }
    *since += time.delta_secs();
    if *since < REFRESH_SECS {
        return;
    }
    *since = 0.0;
    updates.write(HudUpdate {
        layer: debug_layer(),
        path: STATS_ID.to_owned(),
        value: HudValue::Text(hud_text(
            &stats,
            &loaded,
            terrain.as_deref(),
            foliage.as_deref(),
            &observers,
            &origin,
            &time_of_day,
            role.as_deref().copied().unwrap_or_default(),
            &net.as_deref().copied().unwrap_or_default(),
        )),
    });
}

/// The HUD's lines. Pure, so a test can read them without a window.
///
/// Takes the port's role and status rather than anything of the adapter's,
/// so the overlay reads the same on a standalone run (one line: the role)
/// and on a connected one (the round trip, the players in sight and the
/// prediction corrections).
#[must_use]
#[allow(
    clippy::too_many_arguments,
    reason = "the HUD reads every counter there is; that is its job"
)]
pub fn hud_text(
    stats: &FrameStats,
    loaded: &LoadedRegions,
    terrain: Option<&TerrainStats>,
    foliage: Option<&strata_foliage::FoliageStats>,
    observers: &Observers,
    origin: &RenderOrigin,
    time_of_day: &TimeOfDay,
    role: NetRole,
    net: &NetStatus,
) -> String {
    let mut out = String::new();
    let _ = writeln!(
        out,
        "{:.0} fps  {:.1} ms  worst {:.1} ms  gpu {:.1} ms  hitches {}",
        stats.fps, stats.avg_ms, stats.last_worst_ms, stats.gpu_ms, stats.hitches
    );
    let _ = writeln!(
        out,
        "p50 {:.1} ms  p95 {:.1} ms  p99 {:.1} ms",
        stats.p50_ms, stats.p95_ms, stats.p99_ms
    );
    let mut by_lod: BTreeMap<u8, usize> = BTreeMap::new();
    for (coord, _) in loaded.iter() {
        *by_lod.entry(coord.lod).or_default() += 1;
    }
    let _ = write!(
        out,
        "regions {} ({} pending):",
        loaded.len(),
        loaded.pending().count()
    );
    for (lod, n) in &by_lod {
        let _ = write!(out, " L{lod}:{n}");
    }
    out.push('\n');
    let (in_flight, built, cancelled) =
        terrain.map_or((0, 0, 0), |t| (t.in_flight, t.built, t.cancelled));
    let _ = writeln!(
        out,
        "meshing {in_flight} in flight, {built} built, {cancelled} cancelled"
    );
    if let Some(f) = foliage {
        let _ = writeln!(
            out,
            "foliage {} cells, {} blades, {} plants, {} entities, {} placing",
            f.cells, f.blades, f.trees, f.entities, f.in_flight
        );
    }
    let player = observers.primary.unwrap_or_default();
    let _ = writeln!(
        out,
        "player ({:.1}, {:.1}, {:.1})",
        player.x(),
        player.y(),
        player.z()
    );
    let _ = writeln!(
        out,
        "origin ({:.0}, {:.0}, {:.0})  offset {:.0} m",
        origin.0.x(),
        origin.0.y(),
        origin.0.z(),
        player.horizontal_distance(origin.0)
    );
    let (h, m) = time_of_day.hours_minutes();
    let _ = writeln!(out, "time {h:02}:{m:02} ({:.3})", time_of_day.0);
    let _ = writeln!(out, "{}", net_line(role, net));
    let _ = write!(
        out,
        "Esc menu  F1 controls  F3 hides these  F4 wireframe  [ ] time of day"
    );
    out
}

/// The network line: the role, and for a client what the wire is doing.
#[must_use]
pub fn net_line(role: NetRole, net: &NetStatus) -> String {
    match role {
        NetRole::Standalone => "net standalone (local sim)".to_owned(),
        NetRole::Server => format!(
            "net server  {}  {} clients",
            if net.connected {
                "listening"
            } else {
                "not listening"
            },
            net.players
        ),
        NetRole::Client if !net.connected => "net client  connecting".to_owned(),
        NetRole::Client => format!(
            "net client  rtt {:.0} ms  players in sight {}  corrections {}",
            net.rtt_ms, net.players, net.corrections
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_text_carries_every_number() {
        let stats = FrameStats::default();
        let loaded = LoadedRegions::default();
        let observers = Observers {
            positions: vec![strata_core::WorldPos::new(1.0, 2.0, 3.0)],
            primary: Some(strata_core::WorldPos::new(1.0, 2.0, 3.0)),
        };
        let text = hud_text(
            &stats,
            &loaded,
            Some(&TerrainStats::default()),
            Some(&strata_foliage::FoliageStats::default()),
            &observers,
            &RenderOrigin::default(),
            &TimeOfDay(0.5),
            NetRole::Standalone,
            &NetStatus::default(),
        );
        assert!(text.contains("fps"), "{text}");
        assert!(text.contains("regions 0 (0 pending)"), "{text}");
        assert!(text.contains("foliage 0 cells"), "{text}");
        assert!(text.contains("player (1.0, 2.0, 3.0)"), "{text}");
        assert!(text.contains("time 12:00"), "{text}");
        assert!(text.contains("net standalone"), "{text}");
        assert!(text.contains("F3 hides these"), "{text}");
    }

    #[test]
    fn the_net_line_says_which_end_and_what_the_wire_does() {
        let connecting = NetStatus::default();
        assert_eq!(
            net_line(NetRole::Client, &connecting),
            "net client  connecting"
        );
        let up = NetStatus {
            connected: true,
            rtt_ms: 18.4,
            players: 2,
            corrections: 1,
        };
        assert_eq!(
            net_line(NetRole::Client, &up),
            "net client  rtt 18 ms  players in sight 2  corrections 1"
        );
        assert_eq!(
            net_line(NetRole::Server, &up),
            "net server  listening  2 clients"
        );
        assert!(net_line(NetRole::Standalone, &up).contains("standalone"));
    }

    #[test]
    fn the_hotbar_menu_has_eight_slots_over_one_inventory() {
        let def = hotbar_menu_def();
        assert_eq!(def.slots.len(), usize::from(HOTBAR_SLOTS));
        assert_eq!(def.hotbar.len(), usize::from(HOTBAR_SLOTS));
        assert_eq!(def.inventory_sizes(), vec![usize::from(HOTBAR_SLOTS)]);
    }

    #[test]
    fn the_statistics_start_hidden() {
        assert!(!HudVisible::default().0);
    }

    #[test]
    fn the_layers_take_the_builtin_slots() {
        assert_eq!(debug_layer_def().id, builtin::DEBUG);
        assert_eq!(hotbar_layer_def().id, builtin::HOTBAR);
        assert!(hotbar_layer_def().hide_with_screen);
        assert!(!debug_layer_def().hide_with_screen);
    }
}
