//! Shared setup: the harness over the real `ClientPlugin`, the temp settings,
//! and the `Day` fixture (the first day in the harness) that the stock,
//! fitting, survey, crafting and factory screens build on.

use crate::ui;
use bevy::asset::AssetPlugin;
use bevy::camera::Projection;
use bevy::math::Rect;
use bevy::prelude::*;
use bevy::state::state::State;
use slotted::menu::SavedSettings;
use slotted::model::ItemId;
use slotted::model::ItemStack;
use slotted::ui::ButtonState;
use slotted::ui::UiAction;
use slotted::ui::def::Tags;
use slotted_test::prelude::*;
use std::path::Path;
use std::path::PathBuf;
use strata_client::ClientPlugin;
use strata_client::ui::SettingsFile;
use strata_registry::GameRegistries;
use strata_registry::progression::SkillGraph;
use veyra_sim::AppState;
use veyra_sim::LocalPlayer;
use veyra_sim::Player;
use veyra_sim::PlayerId;
use veyra_sim::PlayerIntent;
use veyra_sim::Registries;
use veyra_sim::WorldPosition;
use veyra_sim::combat::Combatant;
use veyra_sim::items::Hands;
use veyra_sim::items::HouseholdStore;
use veyra_sim::items::Pack;
use veyra_sim::progression::CharacterSheet;
use veyra_sim::progression::Interact;
use veyra_sim::progression::Interaction;
use veyra_sim::progression::InteractionKind;
use veyra_ui::card::CardParams;
use veyra_ui::card::ItemCard;
use veyra_ui::stores;
use veyra_ui::stores::StoresState;
use veyra_ui::weapon_diagram::MountPicked;

/// The workspace `assets/` directory: Bevy resolves `AssetPlugin::file_path`
/// against the crate root, and the theme lives two levels up.
pub(crate) fn assets_dir() -> String {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../assets")
        .canonicalize()
        .expect("the workspace assets directory exists")
        .to_string_lossy()
        .into_owned()
}

/// A settings file of this test's own under the system's temporary
/// directory, removed first so a run never reads the last one's values.
pub(crate) fn temp_settings(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("strata-ui-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    dir.join("settings.ron")
}

/// The client over the headless stack, in the world, with the theme
/// loaded and the settings persisting to `settings`.
pub(crate) fn harness(settings: PathBuf) -> UiHarness {
    harness_at(settings, 1600.0, 900.0)
}

/// [`harness`] over a window of `width` by `height`.
pub(crate) fn harness_at(settings: PathBuf, width: f32, height: f32) -> UiHarness {
    let mut h = UiHarness::builder()
        .plugins(SlottedPlugins::headless().set(AssetPlugin {
            file_path: assets_dir(),
            ..default()
        }))
        .plugins(move |app: &mut App| {
            app.insert_resource(SettingsFile(settings.clone()));
            app.init_resource::<strata_foliage::FoliageConfig>();
            veyra_view::combat::hud_only(app);
            app.init_resource::<Picks>()
                .add_systems(Update, record_picks);
        })
        .plugins((
            veyra_sim::SimPlugin,
            strata_stream::StreamPlugin,
            ClientPlugin,
            veyra_ui::VeyraUiPlugin,
        ))
        .resolution(width, height)
        .theme("glass")
        .build();
    wait_for_theme(&mut h);
    wait_for_world(&mut h);
    h.settle();
    h
}

pub(crate) fn wait_for_theme(h: &mut UiHarness) {
    for _ in 0..600 {
        let world = h.world();
        let active = world.resource::<slotted::theme::ActiveTheme>().0.clone();
        if world
            .resource::<Assets<slotted::theme::Theme>>()
            .get(&active)
            .is_some()
        {
            return;
        }
        h.step(1);
    }
    panic!("the theme never loaded; is the asset root right?");
}

pub(crate) fn wait_for_world(h: &mut UiHarness) {
    for _ in 0..60 {
        if *h.world().resource::<State<AppState>>().get() == AppState::InWorld {
            return;
        }
        h.step(1);
    }
    panic!("the app never reached InWorld");
}

pub(crate) fn find(h: &UiHarness, id: &str) -> Entity {
    h.find(&by::test_id(id))
}

pub(crate) fn fov_of(h: &UiHarness, camera: Entity) -> f32 {
    match h.world().get::<Projection>(camera).expect("a projection") {
        Projection::Perspective(p) => p.fov,
        other => panic!("not a perspective projection: {other:?}"),
    }
}

pub(crate) fn saved(path: &Path) -> SavedSettings {
    let text =
        std::fs::read_to_string(path).unwrap_or_else(|e| panic!("reading {}: {e}", path.display()));
    ron::from_str(&text).expect("the settings file is RON")
}

pub(crate) fn button_disabled(h: &UiHarness, id: &str) -> bool {
    h.world()
        .get::<ButtonState>(find(h, id))
        .expect("a button")
        .disabled
}

/// A local player with a sheet, so the HUD has someone to describe.
pub(crate) fn spawn_local_player(h: &mut UiHarness, id: u64) {
    let registries = GameRegistries {
        progression: SkillGraph::shipped(),
        ..GameRegistries::default()
    };
    h.world_mut()
        .insert_resource(Registries(std::sync::Arc::new(registries)));
    h.world_mut().spawn((
        Player { id: PlayerId(id) },
        LocalPlayer,
        Name::new("Frontier ranger"),
        WorldPosition::new(0.0, 10.0, 0.0),
        PlayerIntent::default(),
        Combatant::default(),
        CharacterSheet::default(),
    ));
    h.step(3);
    h.settle();
}

/// Whether `inner` lies within `outer`, with a pixel's slack.
pub(crate) fn within(inner: Rect, outer: Rect) -> bool {
    inner.min.x >= outer.min.x - 1.
        && inner.min.y >= outer.min.y - 1.
        && inner.max.x <= outer.max.x + 1.
        && inner.max.y <= outer.max.y + 1.
}

/// Every [`MountPicked`] since the harness was built: a message lives two
/// frames and a settle runs more.
#[derive(Resource, Default)]
pub(crate) struct Picks(pub(crate) Vec<MountPicked>);

pub(crate) fn record_picks(mut picked: MessageReader<MountPicked>, mut picks: ResMut<Picks>) {
    picks.0.extend(picked.read().cloned());
}

/// Every text under `entity`, in the tree's order.
pub(crate) fn texts_under(h: &UiHarness, entity: Entity) -> Vec<String> {
    let world = h.world();
    let mut found = Vec::new();
    let mut below = vec![entity];
    while let Some(at) = below.pop() {
        if let Some(text) = world.get::<Text>(at) {
            found.push(text.0.clone());
        }
        if let Some(children) = world.get::<Children>(at) {
            below.extend(children.iter().rev());
        }
    }
    found
}

pub(crate) const FEED: &str = "veyra:module.patrol_extended_feed";
pub(crate) const CELL: &str = "veyra:module.pistol_extended_cell";
pub(crate) const SATCHEL: &str = "veyra:bag.field_satchel";
pub(crate) const HAUL: &str = "veyra:bag.haul_pack";

/// The first day, in the harness: the shipped registries, the arrival as
/// the local player with the pack and the hands of `starting.ron`, the
/// household's store filled from it, and a locker within reach.
pub(crate) struct Day {
    pub(crate) h: UiHarness,
    pub(crate) registries: std::sync::Arc<GameRegistries>,
    pub(crate) player: Entity,
    pub(crate) locker: Entity,
}

pub(crate) fn first_day(name: &str, width: f32, height: f32) -> Day {
    let mut h = harness_at(temp_settings(name), width, height);
    let registries = std::sync::Arc::new(
        strata_registry::load_from_dir(assets_dir()).expect("the shipped assets load"),
    );
    let items = &registries.items;
    let starting = items.starting().expect("the first day's stock");
    let arrival = &registries
        .sheets
        .household
        .as_ref()
        .expect("the household's sheets")
        .arrival;
    let pack = Pack::from_saved(&starting.packs[&arrival.name], items).unwrap();
    let hands = Hands::from_saved(&starting.hands.clone().map(Some), items).unwrap();
    // The first day's nine stacks, which fill the store's first row: these
    // screens' focus walks count on the second row being empty, so the
    // vented muzzle and the two blades the first day adds after them are
    // left out here.
    let store: Vec<_> = starting
        .store
        .iter()
        .filter(|record| {
            !matches!(
                record.item.path(),
                "module.vigil_compensator" | "weapon.service_blade"
            )
        })
        .cloned()
        .map(Some)
        .collect();
    let store = HouseholdStore::from_saved(&store, items).unwrap();
    h.world_mut()
        .insert_resource(Registries(registries.clone()));
    h.world_mut().insert_resource(store);
    let player = h
        .world_mut()
        .spawn((
            Player { id: PlayerId(9) },
            LocalPlayer,
            Name::new(arrival.name.clone()),
            WorldPosition::new(0.0, 10.0, 0.0),
            PlayerIntent::default(),
            Combatant::default(),
            // Taught every box that teaches and adds no points, so the
            // crafting tests that are not about who may see the dials'.
            CharacterSheet {
                learned: registries
                    .progression
                    .boxes()
                    .filter(|b| b.effects.is_empty())
                    .map(|b| b.id.clone())
                    .collect(),
                ..CharacterSheet::default()
            },
            pack,
            hands,
        ))
        .id();
    let locker = h
        .world_mut()
        .spawn((
            WorldPosition::new(1.0, 10.0, 0.0),
            Interaction {
                kind: InteractionKind::Locker,
                radius: 3.0,
            },
        ))
        .id();
    // A tick, so the sim's reach has found the locker.
    h.advance(std::time::Duration::from_millis(120));
    h.settle();
    Day {
        h,
        registries,
        player,
        locker,
    }
}

impl Day {
    /// F at the locker, as the keyboard and the game's script both raise it.
    pub(crate) fn use_locker(&mut self) {
        let locker = self.locker;
        self.h.world_mut().write_message(Interact {
            entity: locker,
            kind: InteractionKind::Locker,
        });
        self.h.step(2);
        self.h.settle();
        assert_eq!(self.h.stack_top(), Some(stores::kind()));
    }

    pub(crate) fn id(&self, item: &str) -> ItemId {
        self.registries
            .items
            .id_of(&slotted::model::Namespaced::parse(item).unwrap())
            .unwrap_or_else(|| panic!("{item} is in the catalogue"))
    }

    /// The cell `index` of the grid tagged `region`.
    pub(crate) fn cell(&self, region: &str, index: usize) -> Entity {
        self.h.find(
            &by::role(SemanticRole::Slot)
                .tag("region", region)
                .index(index),
        )
    }

    /// The item and the count the mirror shows in `slot`.
    pub(crate) fn shows(&self, slot: Entity) -> Option<(ItemId, u32)> {
        self.h.stack_at(slot).map(|stack| (stack.id, stack.count))
    }

    pub(crate) fn menu(&self) -> Entity {
        self.h
            .world()
            .resource::<StoresState>()
            .menu()
            .expect("the stores are open")
    }

    pub(crate) fn carried(&self) -> Option<(ItemId, u32)> {
        self.h
            .carried(self.menu())
            .map(|stack| (stack.id, stack.count))
    }

    pub(crate) fn says(&self, id: &str) -> String {
        self.h.text_of(find(&self.h, id)).unwrap_or_default()
    }

    /// The sim's own pack: the truth the screen mirrors.
    pub(crate) fn pack(&self) -> Pack {
        self.h.world().get::<Pack>(self.player).unwrap().clone()
    }

    pub(crate) fn store(&self) -> HouseholdStore {
        self.h.world().resource::<HouseholdStore>().clone()
    }

    /// Every item the sim holds, per kind: the pack with its bags and its
    /// cursor, the hands and the household's store.
    pub(crate) fn truth(&self) -> std::collections::BTreeMap<ItemId, u64> {
        let pack = self.pack();
        let hands = self.h.world().get::<Hands>(self.player).unwrap().clone();
        let store = self.store();
        let mut all = std::collections::BTreeMap::new();
        let mut add = |stack: &ItemStack| {
            *all.entry(stack.id).or_default() += u64::from(stack.count);
        };
        let inventories = [&pack.cells, &pack.bag_slots, &hands.0, &store.0]
            .into_iter()
            .chain(&pack.bags);
        for inventory in inventories {
            inventory.slots().iter().flatten().for_each(&mut add);
        }
        pack.cursor.iter().for_each(&mut add);
        all
    }

    /// Escape, and the tick in which the sim hears of it.
    pub(crate) fn close(&mut self) {
        self.h.key(ui::pause::PAUSE_KEY);
        self.h.advance(std::time::Duration::from_millis(120));
        self.h.settle();
    }

    /// The slot whose card is pinned in the pane.
    pub(crate) fn pinned(&self) -> Option<Entity> {
        self.h.world().resource::<StoresState>().pinned()
    }

    /// The card in the pane, as it was last painted.
    pub(crate) fn card(&self) -> CardParams {
        let card = find(&self.h, stores::CARD_ID);
        self.h.world().get::<ItemCard>(card).unwrap().0.clone()
    }

    pub(crate) fn role_of(&self, id: &str) -> String {
        self.h
            .world()
            .get::<slotted::theme::Themed>(find(&self.h, id))
            .unwrap()
            .0
            .as_str()
            .to_owned()
    }

    /// Whether `entity` takes room in the layout and is drawn.
    pub(crate) fn laid_out(&self, entity: Entity) -> bool {
        self.h
            .world()
            .get::<ComputedNode>(entity)
            .is_some_and(|node| !node.is_empty())
            && self.h.is_visible(entity)
    }

    /// Presses `action` until the focus is on `target`, as a player
    /// walking there would, and fails when `most` presses do not get there.
    pub(crate) fn walk_to(&mut self, action: UiAction, target: Entity, most: usize) {
        for _ in 0..most {
            if self.h.focused() == Some(target) {
                return;
            }
            self.h.action(action);
            self.h.settle();
        }
        assert_eq!(
            self.h.focused(),
            Some(target),
            "{action:?} pressed {most} times did not reach {}, and is on {}",
            self.named(Some(target)),
            self.named(self.h.focused()),
        );
    }

    /// What a node is, for a failure to say where the focus went: its
    /// `test_id`, or for a cell of a grid its region.
    pub(crate) fn named(&self, entity: Option<Entity>) -> String {
        let Some(entity) = entity else {
            return "nothing".to_owned();
        };
        let world = self.h.world();
        world
            .get::<slotted::ui::TestId>(entity)
            .map(|id| id.0.clone())
            .or_else(|| {
                world
                    .get::<Tags>(entity)
                    .and_then(|tags| tags.get("region").map(str::to_owned))
            })
            .unwrap_or_else(|| format!("{entity:?}"))
    }

    /// Fills every cell of the pack and every open cell of its first bag
    /// with a single of a batch of its own, which joins nothing, before a
    /// screen opens.
    pub(crate) fn fill_the_pack(&mut self) {
        let items = &self.registries.items;
        let mut one = items.starting().unwrap().store[1].clone();
        one.count = 1;
        let mut pack = self.h.world_mut().get_mut::<Pack>(self.player).unwrap();
        let open = usize::from(pack.open_cells(0, items));
        let Pack { cells, bags, .. } = &mut *pack;
        let all = cells.len();
        for (inventory, open) in [(cells, all), (&mut bags[0], open)] {
            for cell in 0..open {
                if inventory.get(cell).is_none() {
                    one.batch += 1;
                    let single = veyra_sim::items::write(&one, items).unwrap();
                    inventory.set(cell, Some(single));
                }
            }
            inventory.clear_changed();
        }
    }
}
