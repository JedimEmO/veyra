//! The slotted UI over the world: the glass theme, the pause and settings
//! screens from `slotted-menu`'s templates, and the rule that a screen on
//! the stack takes the keyboard and the mouse away from the game.
//!
//! PLAN 3.2 reserved slotted's UI for the debug overlay and then the real
//! UI, and this is where it lands. What the crate adds on top of
//! `SlottedPlugins` is small on purpose: a `MenuConfig` that points the
//! pause screen's Settings button at the game's generated settings screen
//! ([`settings`]), the settings spec and where it persists, the English
//! for the game's own keys ([`strings`]), the quit flow and the input gate
//! ([`pause`]). The templates, the stack, Escape, focus and persistence are
//! the crate's. The progression slice's screens are the character sheet
//! ([`sheet`], key C, a trainer's F, the pause menu) and the bench
//! ([`mod@bench`], a terminal's F, the pause menu read-only), with the
//! interaction chip ([`prompt`]), the XP toasts ([`progress`]) and the
//! name of the place underfoot ([`place`]) on the HUD. The stores
//! ([`stores`], a locker's F with the household's store, key I and the
//! pause menu without it) show what a character carries and move it by
//! slotted's own clicks, each sent to the sim; every item on them shows the
//! one card ([`card`]). The fitting screen ([`fitting`], a workbench's F
//! with its buttons live, a weapon's cell on the stores screen as a
//! reading) shows a weapon's mounts and what the stock has for them, in
//! the sim's own figures, and asks the sim to fit and to remove. The
//! workbench's second tab is the crafting session ([`crafting`]): a
//! schematic's slots filled from the stock, assembled, worked up a roll at
//! a time and finished, every figure the sim's, and a finished thing
//! recorded there as a production blueprint. A fabricator's F opens the
//! factory ([`factory`]): a blueprint fed from the stock and run as a
//! standing job, whose finish is a toast wherever the player is. They
//! answer the sim's `Interact` message here, which the keyboard and the
//! game's script both write. A weapon drawn from its baked pieces is a
//! widget of the game's own ([`weapon_diagram`], ticket 0008), which the
//! fitting screen draws its weapon with, or, where the weapon's models are
//! baked, with the same pieces turning on a stage of their own
//! ([`weapon_stage`], ticket 0007 step 8). The terminal's
//! second tab is the survey register ([`mod@bench`]), and the deposit
//! tracked there has a marker on the HUD ([`tracker`]), as has the
//! questline's current objective. A person's F opens their conversation
//! ([`dialogue`]) and J the journal ([`journal`]).
//!
//! The simulation does not pause. This is an MMO: the world goes on for
//! everyone else while one player reads a menu, so the pause screen is a
//! modal over a running world and only the local input stops.
//!
//! Nothing here runs without Bevy's UI plugins. A headless app that adds
//! `ClientPlugin` over `MinimalPlugins` (the facade's smoke test) gets no
//! screens and no HUD, which is what `register` reports.

pub mod bench;
pub mod board;
pub mod card;
pub mod claims;
pub mod crafting;
pub mod dialogue;
pub mod factory;
pub mod fitting;
pub mod harvester;
pub mod icons;
pub mod journal;
pub mod loot;
// The combat HUD's shared typefaces (combat_hud.rs, combat_text.rs, the
// game's prop labels).
pub mod fonts;
pub mod graphics;
pub mod map;
pub mod pause;
pub mod place;
pub mod progress;
pub mod prompt;
pub mod render;
pub mod ride;
pub mod scale;
pub mod settings;
pub mod sheet;
pub mod stores;
pub mod strings;
pub mod survey;
pub mod tracker;
pub mod weapon_diagram;
pub mod weapon_stage;

use bevy::app::{App, Startup, Update};
use bevy::asset::{AssetServer, Handle};
use bevy::ecs::prelude::*;
use bevy::ecs::schedule::IntoScheduleConfigs;
use bevy::input_focus::directional_navigation::DirectionalNavigationPlugin;
use bevy::input_focus::tab_navigation::TabNavigationPlugin;
use slotted::menu::{MenuChoice, MenuConfig, Settings, SettingsStorage, kinds};
use slotted::prelude::SlottedPlugins;
use slotted::theme::{ActiveTheme, Role};
use slotted::ui::def::{
    Align, AnchorId, ButtonOpts, Layout, LayoutDirection, Length, Tags, UiNodeDef,
};
use slotted::ui::invalidate::Owner;
use slotted::ui::screen::{Injection, Injections};
use slotted::ui::{LocKey, Presentation, PresentationMode, Screens, SlottedUiSet};
use strata_sim::progression::{Interact, InteractionKind};

pub use pause::{QUIT_CONFIRM, game_input_allowed, open_pause, screen_open};
pub use settings::{SettingsFile, default_settings_path};

/// The theme, relative to the asset root. The file and the two font
/// families it names are copied from the slotted workspace with their
/// licences; `assets/README.md` says from where.
pub const THEME_PATH: &str = "themes/glass.theme.ron";

/// Wires the UI when the app has Bevy's UI plugins, and reports whether it
/// did so the HUD is only registered over a UI that exists.
///
/// `SlottedPlugins` is added unless something added slotted already: the
/// headless test harness builds its own stack first and this plugin sits
/// over it. The two navigation plugins are the ones `DefaultPlugins`
/// leaves out and slotted's focus systems need. The settings file is the
/// [`SettingsFile`] resource when the app inserted one (a test), else the
/// platform's configuration directory.
/// The client's systems that push, replace or pop a screen, and so despawn
/// one. They run after slotted's own sets: its accessibility sync
/// (`SlottedUiSet::Semantics`) queues an insert for every node whose label
/// changed this frame, and a screen torn down between that system and the
/// point its commands are applied leaves the insert a despawned entity,
/// which is a panic. Ordered after it, its commands are applied first.
/// Every system that changes the stack belongs here.
#[derive(SystemSet, Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ScreenWork;

pub(crate) fn register(app: &mut App) -> bool {
    if !app.is_plugin_added::<bevy::ui::UiPlugin>() {
        tracing::info!("no bevy_ui plugin: the slotted UI is not registered");
        return false;
    }
    if !app.is_plugin_added::<TabNavigationPlugin>() {
        app.add_plugins(TabNavigationPlugin);
    }
    if !app.is_plugin_added::<DirectionalNavigationPlugin>() {
        app.add_plugins(DirectionalNavigationPlugin);
    }
    if !app.is_plugin_added::<slotted::ecs::SlottedEcsPlugin>() {
        app.add_plugins(SlottedPlugins::default());
    }
    if !app.world().contains_resource::<SettingsStorage>() {
        let path = app
            .world()
            .get_resource::<SettingsFile>()
            .map_or_else(default_settings_path, |file| file.0.clone());
        tracing::info!(path = %path.display(), "settings file");
        app.insert_resource(SettingsStorage::new(settings::StoredSettings::new(&path)));
    }
    app.configure_sets(Update, ScreenWork.after(SlottedUiSet::Semantics));
    graphics::register(app);
    app.insert_resource(MenuConfig {
        settings_kind: settings::kind(),
        title: LocKey(strings::TITLE.to_owned()),
        version: env!("CARGO_PKG_VERSION").to_owned(),
        ..MenuConfig::default()
    })
    .insert_resource(Settings {
        spec: settings::spec(),
    })
    .add_systems(
        Startup,
        (
            load_theme,
            strings::install,
            register_settings_screen,
            inject_pause_buttons,
        ),
    )
    .add_systems(
        Update,
        (
            (pause::quit_on_choice, pause::exit_on_quit_confirmed).in_set(SlottedUiSet::Input),
            (open_on_pause_choice, open_on_interact).in_set(ScreenWork),
            (pause::release_cursor, pause::hold_player_still).run_if(screen_open),
            (
                settings::apply_field_of_view,
                settings::apply_mouse_sensitivity,
                settings::apply_hud_toggle,
                settings::apply_camera_shake,
                scale::apply_ui_scale,
                settings::apply_foliage_distance
                    .after(graphics::resolve)
                    .before(strata_foliage::FoliageSet::Dispatch),
                graphics::apply_shadow_map,
            ),
        ),
    );
    // After every `Update` system, slotted's own anchoring among them, and
    // before the layout reads the nodes. In `PostUpdate` and not ordered
    // after slotted's render set in `Update`: an edge from that set moves
    // where its commands are applied, and the screens' rebuilds lean on
    // where that is today.
    app.add_systems(
        bevy::app::PostUpdate,
        scale::centre_hud_layers.before(bevy::ui::UiSystems::Prepare),
    );
    // The combat HUD's typefaces, before anything that draws with them.
    fonts::register(app);
    render::register(app);
    sheet::register(app);
    bench::register(app);
    prompt::register(app);
    progress::register(app);
    place::register(app);
    weapon_diagram::register(app);
    weapon_stage::register(app);
    card::register(app);
    app.add_systems(bevy::app::Startup, icons::register_widget);
    stores::register(app);
    fitting::register(app);
    crafting::register(app);
    loot::register(app);
    factory::register(app);
    board::register(app);
    harvester::register(app);
    tracker::register(app);
    survey::register(app);
    map::register(app);
    ride::register(app);
    dialogue::register(app);
    journal::register(app);
    true
}

/// `Startup`: the Character, Bench and Inventory buttons under the pause
/// menu's own three, through the template's `buttons_end` anchor, so the template
/// stays the crate's and a theme change restyles these with the rest.
fn inject_pause_buttons(mut injections: ResMut<Injections>) {
    let button = |id: &str, label: &str| UiNodeDef::Button {
        widget: None,
        opts: ButtonOpts {
            label: Some(LocKey(label.to_owned())),
            ..ButtonOpts::default()
        },
        tags: Tags::new().with("menu", id).with(Tags::TEST_ID, id),
    };
    injections.0.push(Injection {
        target: kinds::pause(),
        anchor: AnchorId::new("buttons_end"),
        node: UiNodeDef::Panel {
            role: Role::new_static("invisible"),
            layout: Layout {
                direction: LayoutDirection::Column,
                gap: 2.0,
                width: Some(Length::Percent(100.0)),
                align: Some(Align::Stretch),
                ..Layout::default()
            },
            children: vec![
                button("sheet", "strata.menu.character"),
                button("loadout", "strata.menu.loadout"),
                button("bench", "strata.menu.bench"),
                button("map", "strata.menu.map"),
                button("journal", "strata.menu.journal"),
                button("inventory", "strata.menu.inventory"),
            ],
            tags: Tags::new().with(Tags::TEST_ID, "strata.pause.buttons"),
        },
        exclusion: false,
        owner: Owner::Game,
    });
}

/// `Update`: the pause menu's Character, Bench and Inventory buttons. The
/// bench from here is read-only, switching being a terminal's business,
/// and the inventory is the pack without the household's store, which is a
/// locker's.
fn open_on_pause_choice(mut choices: MessageReader<MenuChoice>, mut commands: Commands) {
    for choice in choices.read() {
        if choice.screen != kinds::pause() {
            continue;
        }
        match choice.id.as_str() {
            "sheet" => sheet::open_tab(&mut commands, false),
            "loadout" => sheet::open_loadout(&mut commands),
            "bench" => bench::open(&mut commands, false),
            "map" => map::open(&mut commands),
            "journal" => journal::open(&mut commands),
            "inventory" => stores::open(&mut commands, false),
            _ => {}
        }
    }
}

/// `Update`: F beside a prop. A terminal opens the bench with its buttons
/// live; a trainer opens the sheet on the first tree they teach; a locker
/// opens the stores over the household's store; a workbench opens the tab
/// of its screen last shown, the fitting of the weapon in hand or the
/// crafting session, and the session whenever something is on the bench; a
/// fabricator opens the
/// factory, on its run when one is on; a board opens its buyer's lines.
fn open_on_interact(
    mut interactions: MessageReader<Interact>,
    mut commands: Commands,
    inbox: Option<ResMut<strata_sim::procurement::ProcurementInbox>>,
    caches: Option<ResMut<strata_sim::loot::CacheInbox>>,
    talk: Option<ResMut<strata_sim::quests::DialogueInbox>>,
) {
    let mut inbox = inbox;
    let mut caches = caches;
    let mut talk = talk;
    for interaction in interactions.read() {
        match &interaction.kind {
            InteractionKind::Terminal => bench::open(&mut commands, true),
            InteractionKind::Trainer { trees } => {
                sheet::open(&mut commands, trees.first().cloned());
            }
            InteractionKind::Locker => stores::open(&mut commands, true),
            InteractionKind::Workbench => crafting::open_bench(&mut commands),
            InteractionKind::Fabricator => factory::open(&mut commands),
            InteractionKind::Board { buyer } => board::open(&mut commands, buyer.clone()),
            InteractionKind::Harvester { id } => harvester::open(&mut commands, *id),
            // F empties a cache into the pack; `loot::hear_caches` toasts the answer.
            InteractionKind::Cache { key } => {
                if let Some(caches) = caches.as_mut() {
                    caches
                        .0
                        .push(strata_sim::loot::CacheRequest::Take { key: *key });
                }
            }
            // A person's F opens their conversation; the sim answers with a
            // `DialogueView`, which `dialogue` draws.
            InteractionKind::Person { id } => {
                if let Some(talk) = talk.as_mut() {
                    talk.0
                        .push(strata_sim::quests::DialogueRequest::Open(id.clone()));
                }
            }
            // A guard's F buys its faction's lease, through the request the
            // board's button sends; the toast is `board::hear_leases`.
            InteractionKind::Guard { faction } => {
                if let Some(inbox) = inbox.as_mut() {
                    inbox
                        .0
                        .push(strata_sim::procurement::ProcurementRequest::Lease {
                            faction: *faction,
                        });
                }
            }
        }
    }
}

/// `Startup`: the settings screen registered as a page rather than the
/// modal its template is. slotted generates the screen itself in
/// `PostStartup` over the modal template and keeps the template's mode,
/// and a modal over the pause modal leaves the pause readable through the
/// glass panel, its buttons across the rows. A page hides everything
/// under it and Back brings the pause back. Registered first, so slotted
/// finds the kind taken and generates nothing; the scrim is restated
/// because the merge with the template keeps only what is written.
fn register_settings_screen(mut screens: ResMut<Screens>, settings: Res<Settings>) {
    let mut def = settings.spec.screen_def();
    def.presentation = Presentation {
        mode: PresentationMode::Page,
        scrim: Some(true),
        ..Presentation::default()
    };
    screens.register(def);
}

/// `Startup`: the glass theme through the asset server, unless the app
/// already chose one (the harness sets the theme it was asked for).
/// `SlottedThemePlugin` inserts an `ActiveTheme` holding the default
/// handle, so "none chosen" is that handle rather than a missing resource.
fn load_theme(mut active: ResMut<ActiveTheme>, assets: Option<Res<AssetServer>>) {
    let Some(assets) = assets else {
        return;
    };
    if active.0 == Handle::default() {
        active.0 = assets.load(THEME_PATH);
    }
}

/// What the tests of the survey's screens share: the shipped registries
/// and a deposit made by hand, so no world need be baked to word one.
#[cfg(test)]
pub(crate) mod testing {
    use strata_registry::GameRegistries;
    use strata_sim::deposits::{Deposit, DepositId};

    /// The shipped `assets/`.
    pub fn shipped() -> GameRegistries {
        let assets = concat!(env!("CARGO_MANIFEST_DIR"), "/../../assets");
        strata_registry::load_from_dir(assets).expect("assets/ loads")
    }

    /// A harvester record at store grade.
    pub fn harvester_record() -> strata_registry::Record {
        strata_registry::Record {
            item: "veyra:station.field_harvester".parse().expect("an id"),
            count: 1,
            batch: 0,
            by: None,
            lines: std::collections::BTreeMap::from([
                ("rate".to_owned(), strata_registry::item::STORE_GRADE),
                ("hopper".to_owned(), strata_registry::item::STORE_GRADE),
            ]),
            condition: None,
            fitted: std::collections::BTreeMap::new(),
            origin: None,
        }
    }

    /// The first member's pack, with `record` put into it when given.
    pub fn pack_with(
        registries: &GameRegistries,
        record: Option<&strata_registry::Record>,
    ) -> strata_sim::items::Pack {
        use strata_registry::item::SavedPack;
        use strata_sim::items::{Pack, place_in, write};
        let mut pack = Pack::from_saved(&SavedPack::default(), &registries.items).expect("a pack");
        if let Some(saved) = registries
            .items
            .starting()
            .and_then(|start| start.packs.values().next())
        {
            pack = Pack::from_saved(saved, &registries.items).expect("a pack");
        }
        if let Some(record) = record {
            let stack = write(record, &registries.items).expect("a stack");
            assert!(place_in(&registries.items, &mut pack.cells, stack).is_none());
        }
        pack
    }

    /// A sheet that has learned `boxes`.
    pub fn sheet_of(boxes: &[&str]) -> strata_sim::progression::CharacterSheet {
        strata_sim::progression::CharacterSheet {
            learned: boxes.iter().map(|each| (*each).to_owned()).collect(),
            ..strata_sim::progression::CharacterSheet::default()
        }
    }

    /// A machine planted on `deposit` at `at`, `pm` there, holding `held`
    /// of a hopper of 300.
    pub fn planted(
        registries: &GameRegistries,
        deposit: &Deposit,
        at: [f64; 2],
        pm: u16,
        held: u32,
    ) -> strata_sim::harvest::Harvester {
        let rules = registries.resources.harvest();
        strata_sim::harvest::Harvester {
            id: 0,
            deposit: deposit.id,
            at,
            pm,
            item: harvester_record(),
            by: "Ilse Marrow".to_owned(),
            rate: rules.rate,
            hopper: rules.hopper,
            lot: strata_sim::deposits::lot(deposit, &registries.resources, 1).expect("a lot"),
            held,
            carry: 0,
            full: held >= rules.hopper,
        }
    }

    /// A deposit of `ty` named `root` in `cell`, at `at`, each figure a little over its
    /// floor: what a query would give.
    pub fn deposit(
        registries: &GameRegistries,
        ty: &str,
        root: &str,
        cell: [i32; 2],
        at: [f64; 2],
    ) -> Deposit {
        let set = &registries.resources;
        let ty = set.type_id(&ty.parse().expect("an id")).expect("a type");
        let def = set.get(ty).expect("its definition");
        Deposit {
            id: DepositId(cell),
            ty,
            root: root.to_owned(),
            place: format!("{root} {}", def.grounds[0]),
            at,
            height_m: 12.,
            figures: set
                .class_of(ty)
                .carries
                .iter()
                .enumerate()
                .map(|(index, attribute)| {
                    let span = def.ranges[attribute];
                    let over = u16::try_from(60 * (index + 1)).expect("small");
                    (*attribute, (span.floor + over).min(span.cap))
                })
                .collect(),
            radius_m: 200.0,
            peak: 800,
            grade_pm: 500,
        }
    }
}
