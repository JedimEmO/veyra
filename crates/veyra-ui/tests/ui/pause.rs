//! The pause menu, settings, the HUD and its vitals cluster, the sheet and
//! bench screens from the pause, and the small-window fits.

use crate::ui;
use bevy::camera::Projection;
use bevy::math::Rect;
use bevy::math::Vec2;
use bevy::prelude::*;
use slotted::menu::kinds;
use slotted::ui::ScreenStack;
use slotted::ui::SliderState;
use slotted::ui::Value;
use slotted_test::prelude::*;
use strata_client::OrbitCamera;
use strata_client::hud;
use strata_client::hud::HudVisible;
use strata_client::ui::settings::keys;
use strata_registry::GameRegistries;
use strata_registry::progression::SkillGraph;
use strata_registry::progression::XpKind;
use veyra_sim::LocalPlayer;
use veyra_sim::Player;
use veyra_sim::PlayerCommand;
use veyra_sim::PlayerId;
use veyra_sim::PlayerIntent;
use veyra_sim::Registries;
use veyra_sim::WorldPosition;
use veyra_sim::combat::Combatant;
use veyra_sim::progression::CharacterSheet;
use veyra_sim::progression::Interaction;
use veyra_sim::progression::InteractionKind;

use crate::common::*;

/// Escape opens the pause over the world, its Settings button (found by
/// role) opens the generated settings screen, a slider change reaches the
/// camera and the file, and a fresh app over the same file starts from
/// the saved value. The player's intent is cleared while a screen is up
/// and the world keeps ticking.
#[test]
#[allow(clippy::too_many_lines)]
fn escape_opens_the_pause_and_a_setting_persists() {
    let path = temp_settings("pause");
    let mut h = harness(path.clone());
    assert!(h.stack().is_empty());
    assert!(!path.exists(), "opening saves nothing");

    // A camera to apply the field of view to, and a player walking.
    let camera = h
        .world_mut()
        .spawn((OrbitCamera::default(), Projection::default()))
        .id();
    let player = h
        .world_mut()
        .spawn((
            Player { id: PlayerId(1) },
            LocalPlayer,
            WorldPosition(strata_core::WorldPos::new(0.0, 10.0, 0.0)),
            PlayerIntent {
                move_dir: Vec2::Y,
                ..default()
            },
        ))
        .id();
    h.step(2);
    assert!(
        (f64::from(fov_of(&h, camera)) - ui::settings::DEFAULT_FOV_DEGREES.to_radians()).abs()
            < 1e-6,
        "the default applies to a camera spawned after the settings loaded"
    );
    let tick_before = h.world().resource::<veyra_sim::SimClock>().tick();

    // Escape: the pause modal over the world, focus on Resume.
    h.key(ui::pause::PAUSE_KEY);
    h.settle();
    assert_eq!(h.stack_top(), Some(kinds::pause()));
    assert_eq!(h.focused(), Some(find(&h, "resume")));
    assert!(
        h.world().resource::<ScreenStack>().top().is_some(),
        "a modal counts as a screen for the input gate"
    );
    assert_eq!(
        h.world().get::<PlayerIntent>(player).unwrap().move_dir,
        Vec2::ZERO,
        "the menu has the keys: the player stops"
    );
    // And the command the next tick consumes, which is what a connected
    // client sends to the server, is the stop too: the server's copy of
    // the player halts one round trip later.
    h.advance(std::time::Duration::from_millis(120));
    h.settle();
    assert_eq!(
        h.world().get::<PlayerCommand>(player).unwrap().0.move_dir,
        Vec2::ZERO,
        "the consumed command carries the stop to the network"
    );

    // Settings, by role and its menu tag. It is a page: the pause under
    // it is hidden rather than showing through the glass.
    let pause_root = h.world().resource::<ScreenStack>().entries()[0].root;
    assert!(h.is_visible(pause_root));
    let settings = h.find(&by::role(SemanticRole::Button).tag("menu", "settings"));
    h.click(settings);
    h.settle();
    assert_eq!(
        h.stack(),
        vec![kinds::pause(), ui::settings::kind()],
        "the generated settings screen sits over the pause"
    );
    assert!(
        !h.is_visible(pause_root),
        "the pause is hidden under the settings page"
    );
    assert_eq!(
        h.menu_choices()
            .into_iter()
            .map(|c| c.id)
            .collect::<Vec<_>>(),
        vec!["settings"]
    );

    // The field of view slider, dragged to its end.
    let grass = h
        .world_mut()
        .spawn((
            MeshMaterial3d::<strata_foliage::GrassMaterial>(Handle::default()),
            bevy::camera::visibility::VisibilityRange {
                start_margin: 110.0..130.0,
                end_margin: 400.0..480.0,
                use_aabb: false,
            },
        ))
        .id();
    let foliage_slider = find(&h, keys::FOLIAGE_DISTANCE);
    h.drag_slider(foliage_slider, 0.0);
    h.settle();
    assert_eq!(h.value(keys::FOLIAGE_DISTANCE), Some(Value::Float(25.0)));
    assert!(
        (h.world()
            .get::<bevy::camera::visibility::VisibilityRange>(grass)
            .unwrap()
            .end_margin
            .end
            - 120.0)
            .abs()
            < 0.001
    );
    h.drag_slider(foliage_slider, 1.0);
    h.settle();
    assert!(
        (h.world()
            .get::<bevy::camera::visibility::VisibilityRange>(grass)
            .unwrap()
            .end_margin
            .end
            - 960.0)
            .abs()
            < 0.001
    );
    h.drag_slider(foliage_slider, 0.0);
    h.settle();
    assert!(
        (h.world()
            .resource::<strata_foliage::FoliageConfig>()
            .tree_fades
            .far
            .1
            - 400.0)
            .abs()
            < 0.001
    );
    let slider = find(&h, keys::FIELD_OF_VIEW);
    assert_eq!(
        h.value(keys::FIELD_OF_VIEW),
        Some(Value::Float(ui::settings::DEFAULT_FOV_DEGREES))
    );
    h.drag_slider(slider, 1.0);
    h.settle();
    assert_eq!(h.value(keys::FIELD_OF_VIEW), Some(Value::Float(100.0)));
    assert_eq!(h.world().get::<SliderState>(slider).unwrap().value, 100.0);
    assert!(
        (fov_of(&h, camera) - 100f32.to_radians()).abs() < 1e-6,
        "the projection follows the store"
    );
    h.set_value(keys::MOUSE_SENSITIVITY, 2.0);
    h.settle();
    let sensitivity = h.world().get::<OrbitCamera>(camera).unwrap().sensitivity;
    assert!(
        (sensitivity - 2.0 * ui::settings::BASE_SENSITIVITY).abs() < 1e-9,
        "{sensitivity}"
    );

    // Saved once the changes settled, to the temp file.
    h.step(3);
    let file = saved(&path);
    assert_eq!(
        file.values.get(keys::FOLIAGE_DISTANCE),
        Some(&Value::Float(25.0))
    );
    assert_eq!(
        file.values.get(keys::FIELD_OF_VIEW),
        Some(&Value::Float(100.0))
    );
    assert_eq!(
        file.values.get(keys::MOUSE_SENSITIVITY),
        Some(&Value::Float(2.0))
    );

    // Escape twice: the settings, then the pause. The keys are the game's
    // again and the sim ticked throughout.
    h.key(ui::pause::PAUSE_KEY);
    h.settle();
    assert_eq!(h.stack(), vec![kinds::pause()]);
    assert!(
        h.is_visible(pause_root),
        "the pause is back when the page pops"
    );
    h.key(ui::pause::PAUSE_KEY);
    h.settle();
    assert!(h.stack().is_empty());
    assert!(
        h.world().resource::<veyra_sim::SimClock>().tick() > tick_before,
        "the world does not pause for a menu"
    );

    // A second run over the same file starts from the saved values.
    let mut again = harness(path);
    let camera = again
        .world_mut()
        .spawn((OrbitCamera::default(), Projection::default()))
        .id();
    again.step(2);
    assert_eq!(
        again
            .world()
            .resource::<strata_foliage::FoliageConfig>()
            .far_fade
            .1,
        120.0
    );
    assert_eq!(again.value(keys::FIELD_OF_VIEW), Some(Value::Float(100.0)));
    assert!((fov_of(&again, camera) - 100f32.to_radians()).abs() < 1e-6);
}

/// Quit on the pause asks first; an accepted confirm exits the app.
#[test]
fn quit_asks_then_exits() {
    let mut h = harness(temp_settings("quit"));
    h.key(ui::pause::PAUSE_KEY);
    h.settle();
    let quit = h.find(&by::role(SemanticRole::Button).tag("menu", "quit"));
    h.click(quit);
    h.settle();
    assert_eq!(h.stack(), vec![kinds::pause(), kinds::confirm()]);
    h.confirm_accept();
    let exits: Vec<AppExit> = bevy::ecs::message::MessageCursor::default()
        .read(h.world().resource::<Messages<AppExit>>())
        .cloned()
        .collect();
    assert_eq!(exits, vec![AppExit::Success]);
}

/// The three HUD layers are spawned: the debug statistics start hidden,
/// F3 shows them with the frame numbers and hides them again, the hotbar is eight empty slots over the
/// hotbar inventory, and the settings' HUD toggle hides the reticle and
/// the hotbar together.
#[test]
fn the_hud_layers_exist_and_toggle() {
    let mut h = harness(temp_settings("hud"));
    let debug = h.hud_layer("debug").expect("the debug layer is spawned");
    let hotbar = h.hud_layer("hotbar").expect("the hotbar layer is spawned");
    let crosshair = h
        .hud_layer("crosshair")
        .expect("the crosshair layer is spawned");
    assert!(!h.world().resource::<HudVisible>().0);
    assert!(!h.is_visible(debug), "the statistics start hidden");
    assert!(h.is_visible(crosshair));
    // An empty inventory hides the hotbar in every mode; stock it so the
    // rest of this test can watch the other rules show and hide it.
    assert!(
        !h.is_visible(hotbar),
        "an empty hotbar inventory hides the layer"
    );
    let inventory = h.world().resource::<hud::HotbarInventory>().inventory;
    h.world_mut()
        .get_mut::<slotted::ecs::Inventory>(inventory)
        .expect("the hotbar inventory exists")
        .set(
            0,
            Some(slotted::model::ItemStack::new(slotted::model::ItemId(1), 1)),
        );
    h.step(2);
    assert!(h.is_visible(hotbar));

    // F3 shows them; the text is written a quarter of a second in.
    h.key(hud::TOGGLE_KEY);
    h.step(2);
    assert!(h.world().resource::<HudVisible>().0);
    assert!(h.is_visible(debug));
    h.advance(std::time::Duration::from_millis(600));
    h.settle();
    let stats = h.text_of(find(&h, hud::STATS_ID)).unwrap();
    assert!(stats.contains("fps"), "{stats}");
    assert!(stats.contains("regions"), "{stats}");

    // Eight slots, only the one stocked above holding anything, bound to the hotbar inventory's menu.
    let slots = h.find_all(&by::role(SemanticRole::Slot).tag("region", "hotbar"));
    assert_eq!(slots.len(), usize::from(hud::HOTBAR_SLOTS));
    assert_eq!(
        slots
            .iter()
            .filter(|slot| h.stack_at(**slot).is_some())
            .count(),
        1
    );
    let inventory = h.world().resource::<hud::HotbarInventory>();
    assert_eq!(
        h.world().resource::<slotted::ui::HudMenu>().0,
        Some(inventory.menu)
    );

    // F3 again: the statistics layer only.
    h.key(hud::TOGGLE_KEY);
    h.step(2);
    assert!(!h.world().resource::<HudVisible>().0);
    assert!(!h.is_visible(debug));
    assert!(h.is_visible(hotbar));
    h.key(hud::TOGGLE_KEY);
    h.step(2);
    assert!(h.world().resource::<HudVisible>().0);
    assert!(h.is_visible(debug));

    // The settings toggle: the reticle and the hotbar, not the statistics.
    h.set_value(keys::SHOW_HUD, false);
    h.step(2);
    assert!(!h.is_visible(hotbar));
    assert!(!h.is_visible(crosshair));
    assert!(h.is_visible(debug));
    h.set_value(keys::SHOW_HUD, true);
    h.step(2);
    assert!(h.is_visible(hotbar));
    assert!(h.is_visible(crosshair));

    // A screen hides the hotbar and the reticle, and the statistics stay.
    h.key(ui::pause::PAUSE_KEY);
    h.settle();
    assert!(!h.is_visible(hotbar));
    assert!(!h.is_visible(crosshair));
    assert!(h.is_visible(debug));
}

#[test]
fn render_panel_captures_input_and_baseline_comparison_restores_edits() {
    use slotted::ui::ValueStore;
    use strata_atmosphere::{SkySettings, look::RenderLook};
    let path = temp_settings("render-panel");
    let mut h = harness(path.clone());
    let player = h
        .world_mut()
        .spawn((
            Player { id: PlayerId(7) },
            LocalPlayer,
            WorldPosition::new(0.0, 10.0, 0.0),
            PlayerIntent {
                move_dir: Vec2::Y,
                ..default()
            },
        ))
        .id();
    h.key(ui::render::KEY);
    h.settle();
    assert_eq!(h.stack_top(), Some(ui::render::kind()));
    assert_eq!(
        h.world().get::<PlayerIntent>(player).unwrap().move_dir,
        Vec2::ZERO
    );
    h.world_mut()
        .resource_mut::<ValueStore>()
        .insert("render.preset", "sunlit-mist");
    h.step(3);
    assert!(h.world().resource::<RenderLook>().fog_density > 0.0);
    h.world_mut()
        .resource_mut::<ValueStore>()
        .insert("render.ev100", 15.0_f64);
    h.step(3);
    assert_eq!(h.world().resource::<SkySettings>().ev100, 15.0);
    h.world_mut()
        .resource_mut::<ValueStore>()
        .insert("render.compare", true);
    h.step(3);
    assert_eq!(*h.world().resource::<RenderLook>(), RenderLook::default());
    h.world_mut()
        .resource_mut::<ValueStore>()
        .insert("render.compare", false);
    h.step(3);
    assert_eq!(h.world().resource::<SkySettings>().ev100, 15.0);
    assert!(h.world().resource::<RenderLook>().fog_density > 0.0);
    let export = h.find(&by::test_id("render.export"));
    h.click(export);
    h.settle();
    let exported = std::fs::read_to_string(path.with_file_name("render-look.ron")).unwrap();
    let file = strata_atmosphere::look::LookFile::from_ron(&exported).unwrap();
    assert_eq!(file.sky.ev100, 15.0);
    let reset = h.find(&by::test_id("render.reset"));
    h.click(reset);
    h.settle();
    assert_eq!(
        *h.world().resource::<SkySettings>(),
        strata_atmosphere::look::LookFile::preset("sunlit-mist")
            .unwrap()
            .sky
    );
    h.key(ui::render::KEY);
    h.settle();
    assert!(h.stack_top().is_none());
}

#[test]
fn reset_and_compare_from_a_fresh_start_keep_the_baseline_look() {
    use slotted::ui::ValueStore;
    use strata_atmosphere::look::{LookFile, RenderLook};
    // No saved look: the game starts on the default, which the panel names
    // `baseline`. Comparing against baseline and resetting to it must both
    // give that same look back, valley fog included.
    let mut h = harness(temp_settings("render-fresh"));
    assert_eq!(*h.world().resource::<RenderLook>(), RenderLook::default());
    assert!(h.world().resource::<RenderLook>().ground_fog);
    h.key(ui::render::KEY);
    h.settle();
    assert_eq!(h.stack_top(), Some(ui::render::kind()));
    h.world_mut()
        .resource_mut::<ValueStore>()
        .insert("render.compare", true);
    h.step(3);
    assert_eq!(*h.world().resource::<RenderLook>(), RenderLook::default());
    h.world_mut()
        .resource_mut::<ValueStore>()
        .insert("render.compare", false);
    h.step(3);
    let reset = h.find(&by::test_id("render.reset"));
    h.click(reset);
    h.settle();
    let look = h.world().resource::<RenderLook>().clone();
    assert_eq!(look, LookFile::preset("baseline").unwrap().look);
    assert_eq!(look, RenderLook::default());
    assert!(look.ground_fog);
}

/// The sheet is one size and Learn stands where it stood whichever box is
/// selected: the novice, with one effect, and boxes further up the blade
/// tree, whose text is longer. The details scroll inside their panel.
#[test]
fn the_sheet_does_not_move_with_the_selected_box() {
    for (width, height) in [(1600.0, 900.0), (1280.0, 720.0)] {
        let mut h = harness_at(temp_settings("sheet-fixed"), width, height);
        let graph = SkillGraph::shipped();
        let registries = GameRegistries {
            progression: graph.clone(),
            ..GameRegistries::default()
        };
        h.world_mut()
            .insert_resource(Registries(std::sync::Arc::new(registries)));
        let mut sheet = CharacterSheet::default();
        sheet.learned.insert("blade.novice".to_owned());
        sheet.credit(XpKind::Blade, 200);
        h.world_mut().spawn((
            Player { id: PlayerId(3) },
            LocalPlayer,
            Name::new("Test ranger"),
            WorldPosition::new(0.0, 10.0, 0.0),
            PlayerIntent::default(),
            Combatant::default(),
            sheet,
        ));
        h.step(2);
        h.key(ui::sheet::KEY);
        h.settle();
        assert_eq!(h.stack_top(), Some(ui::sheet::kind()));
        let rects = |h: &UiHarness| {
            // A button's width follows its state a pixel or two; where it
            // stands is what must hold.
            [ui::sheet::ROOT_ID, ui::sheet::LEARN, ui::sheet::FORGET].map(|id| {
                let rect = h.rect_of(find(h, id));
                if id == ui::sheet::ROOT_ID {
                    rect
                } else {
                    Rect::from_corners(rect.min, Vec2::new(rect.min.x, rect.max.y))
                }
            })
        };
        let first = rects(&h);
        let mut texts = Vec::new();
        for id in [
            "box:blade.novice",
            "box:blade.cuts.1",
            "box:blade.guard.1",
            "box:blade.breaking.1",
        ] {
            let Some(card) = h.try_find(&by::test_id(id)) else {
                continue;
            };
            h.click(card);
            h.settle();
            texts.push(
                h.rect_of(find(&h, ui::sheet::STATUS_ID))
                    .height()
                    .to_string(),
            );
            assert_eq!(rects(&h), first, "{width}x{height} box {id}");
        }
        assert!(texts.len() >= 2, "boxes were compared: {texts:?}");
        texts.sort();
        texts.dedup();
        assert!(
            texts.len() >= 2,
            "a short box and a tall one were among them: {texts:?}"
        );
    }
}

/// C opens the sheet over the world on the tree the points went into,
/// the Learn button follows the sim's rules (a box whose XP is paid stays
/// off until a trainer of its tree is within reach, and lights when one
/// is), a click learns the box and refits the actor, Forget refuses the
/// box the new one depends on, and Escape closes the sheet.
#[test]
#[allow(clippy::too_many_lines)]
fn the_sheet_screen_follows_the_learn_rules() {
    let mut h = harness(temp_settings("sheet"));
    let graph = SkillGraph::shipped();
    let registries = GameRegistries {
        progression: graph.clone(),
        ..GameRegistries::default()
    };
    h.world_mut()
        .insert_resource(Registries(std::sync::Arc::new(registries)));
    let mut sheet = CharacterSheet::default();
    sheet.learned.insert("blade.novice".to_owned());
    sheet.credit(XpKind::Blade, 200);
    let player = h
        .world_mut()
        .spawn((
            Player { id: PlayerId(3) },
            LocalPlayer,
            Name::new("Test ranger"),
            WorldPosition::new(0.0, 10.0, 0.0),
            PlayerIntent::default(),
            Combatant::default(),
            sheet,
        ))
        .id();
    h.step(2);

    h.key(ui::sheet::KEY);
    h.settle();
    assert_eq!(h.stack_top(), Some(ui::sheet::kind()));
    // The blade tree, since its novice is the only box; One hand 1 is the
    // next box, and its XP is paid but nobody teaches it here.
    assert_eq!(h.focused(), Some(find(&h, "box:blade.cuts.1")));
    assert!(button_disabled(&h, ui::sheet::LEARN), "no trainer in reach");
    assert!(
        button_disabled(&h, ui::sheet::FORGET),
        "One hand 1 is not learned"
    );
    let status = h.text_of(find(&h, ui::sheet::STATUS_ID)).unwrap();
    assert!(status.contains("blade trainer"), "{status}");
    assert_eq!(h.text_of(find(&h, ui::sheet::POINTS_ID)).unwrap(), "6");

    // A blade trainer within reach: the sim's reach finds it on the next
    // tick and the sheet rebuilds with Learn live.
    h.world_mut().spawn((
        WorldPosition::new(1.0, 10.0, 0.0),
        Interaction {
            kind: InteractionKind::Trainer {
                trees: vec!["blade".to_owned()],
            },
            radius: 3.0,
        },
    ));
    h.advance(std::time::Duration::from_millis(120));
    h.settle();
    assert!(
        !button_disabled(&h, ui::sheet::LEARN),
        "the trainer teaches it"
    );
    let status = h.text_of(find(&h, ui::sheet::STATUS_ID)).unwrap();
    assert!(status.contains("teaches it for 2 points"), "{status}");

    let learn = find(&h, ui::sheet::LEARN);
    h.click(learn);
    h.settle();
    let sheet = h.world().get::<CharacterSheet>(player).unwrap();
    assert!(sheet.has("blade.cuts.1"));
    assert_eq!(sheet.points_spent(&graph), 8);
    assert!(
        h.world()
            .get::<Combatant>(player)
            .unwrap()
            .unlocked(veyra_sim::combat::Action::Followup),
        "the actor was refitted"
    );
    assert_eq!(h.text_of(find(&h, ui::sheet::POINTS_ID)).unwrap(), "8");
    let notice = h.text_of(find(&h, ui::sheet::NOTICE_ID)).unwrap();
    assert!(notice.starts_with("Learned One hand 1"), "{notice}");
    // The selection stays on the box just learned, now with Forget live.
    assert_eq!(h.focused(), Some(find(&h, "box:blade.cuts.1")));
    assert!(button_disabled(&h, ui::sheet::LEARN));
    assert!(!button_disabled(&h, ui::sheet::FORGET));
    let status = h.text_of(find(&h, ui::sheet::STATUS_ID)).unwrap();
    assert!(status.contains("returns 2 points"), "{status}");

    // The novice: learned, but One hand 1 still needs it.
    let novice_card = find(&h, "box:blade.novice");
    h.click(novice_card);
    h.settle();
    assert!(button_disabled(&h, ui::sheet::LEARN));
    assert!(button_disabled(&h, ui::sheet::FORGET));
    let status = h.text_of(find(&h, ui::sheet::STATUS_ID)).unwrap();
    assert!(status.contains("One hand 1 still needs it"), "{status}");

    // One hand 1 can be forgotten, and is.
    let cuts_1 = find(&h, "box:blade.cuts.1");
    h.click(cuts_1);
    h.settle();
    assert!(!button_disabled(&h, ui::sheet::FORGET));
    let forget = find(&h, ui::sheet::FORGET);
    h.click(forget);
    h.settle();
    assert!(
        !h.world()
            .get::<CharacterSheet>(player)
            .unwrap()
            .has("blade.cuts.1")
    );
    assert_eq!(h.text_of(find(&h, ui::sheet::POINTS_ID)).unwrap(), "6");

    // Another tree's tab, then Escape closes the sheet and the keys are
    // the game's again.
    let marksman = find(&h, "tree:marksman");
    h.click(marksman);
    h.settle();
    assert_eq!(h.focused(), Some(find(&h, "box:marksman.novice")));
    h.key(ui::pause::PAUSE_KEY);
    h.settle();
    assert!(h.stack().is_empty());
}

/// The pause menu's Character button opens the sheet as a page over it,
/// and its Bench button the bench, read-only, with the published view.
#[test]
fn the_pause_menu_reaches_both_screens() {
    let mut h = harness(temp_settings("pause-screens"));
    let registries = GameRegistries {
        progression: SkillGraph::shipped(),
        ..GameRegistries::default()
    };
    h.world_mut()
        .insert_resource(Registries(std::sync::Arc::new(registries)));
    h.world_mut().spawn((
        Player { id: PlayerId(4) },
        LocalPlayer,
        WorldPosition::new(0.0, 10.0, 0.0),
        PlayerIntent::default(),
        Combatant::default(),
        CharacterSheet::default(),
    ));
    let one = ui::bench::BenchCard {
        name: "Frontier ranger".to_owned(),
        tint: 0,
        weapon: veyra_sim::combat::Weapon::Carbine,
        sheet: CharacterSheet::default(),
        work: None,
        injured_min: None,
    };
    h.world_mut().insert_resource(ui::bench::BenchView {
        characters: vec![one.clone()],
        active: 0,
        recruits: vec![one],
        capacity: 4,
    });
    h.step(2);
    h.key(ui::pause::PAUSE_KEY);
    h.settle();
    let character = h.find(&by::role(SemanticRole::Button).tag("menu", "sheet"));
    h.click(character);
    h.settle();
    assert_eq!(h.stack(), vec![kinds::pause(), ui::sheet::kind()]);
    h.key(ui::pause::PAUSE_KEY);
    h.settle();
    let bench = h.find(&by::role(SemanticRole::Button).tag("menu", "bench"));
    h.click(bench);
    h.settle();
    assert_eq!(h.stack(), vec![kinds::pause(), ui::bench::kind()]);
    assert!(
        button_disabled(&h, "bench.join:0"),
        "read-only from the pause"
    );
    assert!(button_disabled(&h, "bench.playing"));
}

/// The vitals cluster lines up: the three tracks share a left edge and a
/// width, each label ends left of its track and each figure starts right
/// of it, the name's row is above the bars and the magazine below, and
/// the whole panel is inside the window. The controls card is up once
/// the player arrives, and F1 puts it away and brings it back.
#[test]
fn the_vitals_cluster_lines_up_and_f1_flips_the_controls() {
    use veyra_view::combat_hud::{self, BAR_WIDTH, Meter, ids};
    let mut h = harness(temp_settings("hud-layout"));
    spawn_local_player(&mut h, 5);
    let window = Rect::new(0., 0., 1600., 900.);
    let panel = h.rect_of(find(&h, ids::PANEL));
    assert!(
        within(panel, window),
        "the cluster {panel:?} spills the window"
    );
    let name = find(&h, ids::NAME);
    assert_eq!(h.text_of(name).as_deref(), Some("Frontier ranger"));
    let name = h.rect_of(name);
    let ammo = h.rect_of(find(&h, ids::AMMO));
    let mut left = None;
    for (meter, figure) in Meter::ALL.into_iter().zip(["100", "40", "100"]) {
        let track = h.rect_of(find(&h, meter.track()));
        let label = h.rect_of(find(&h, meter.label()));
        let value = find(&h, meter.value());
        assert_eq!(h.text_of(value).as_deref(), Some(figure), "{meter:?}");
        let value = h.rect_of(value);
        assert!(
            (track.width() - BAR_WIDTH).abs() < 1.,
            "{meter:?} track {track:?}"
        );
        let left = *left.get_or_insert(track.min.x);
        assert!((track.min.x - left).abs() < 0.5, "{meter:?} out of line");
        assert!(label.max.x <= track.min.x, "{meter:?} label over its bar");
        assert!(value.min.x >= track.max.x, "{meter:?} figure over its bar");
        assert!(within(track, panel) && within(value, panel));
        assert!(track.min.y >= name.max.y, "{meter:?} over the name");
        assert!(track.max.y <= ammo.min.y, "{meter:?} over the magazine");
    }
    let shown = |h: &UiHarness| {
        h.world()
            .resource::<slotted::ui::HudLayers>()
            .get(&combat_hud::controls_layer())
            .is_some_and(|def| def.visible)
    };
    assert!(shown(&h), "the card greets the player");
    h.key(combat_hud::CONTROLS_KEY);
    h.settle();
    assert!(!shown(&h), "F1 puts it away");
    h.key(combat_hud::CONTROLS_KEY);
    h.settle();
    assert!(shown(&h), "and brings it back");
}

/// The UI scale follows the window's height in quarter steps, a fixed
/// choice overrides it, and a resize is picked up without a restart.
#[test]
fn the_ui_scale_follows_the_window_and_the_setting() {
    let mut h = harness_at(temp_settings("ui-scale"), 1920.0, 1080.0);
    let scale = |h: &UiHarness| h.world().resource::<bevy::ui::UiScale>().0;
    assert_eq!(scale(&h), 1.25, "1080p is a quarter larger than the design");

    h.set_value(keys::UI_SCALE, Value::Text("100".to_owned()));
    h.step(1);
    assert_eq!(scale(&h), 1.0);
    h.set_value(keys::UI_SCALE, Value::Text("200".to_owned()));
    h.step(1);
    assert_eq!(scale(&h), 2.0);

    h.set_value(keys::UI_SCALE, Value::Text("auto".to_owned()));
    let window = h.window();
    h.world_mut()
        .get_mut::<Window>(window)
        .unwrap()
        .resolution
        .set(3840.0, 2160.0);
    h.step(1);
    assert_eq!(scale(&h), 2.0, "4K is the largest automatic scale");
    h.world_mut()
        .get_mut::<Window>(window)
        .unwrap()
        .resolution
        .set(1280.0, 720.0);
    h.step(1);
    assert_eq!(scale(&h), 1.0, "below the design height nothing shrinks");
}

/// At 1080p the automatic scale is 1.25, and the sheet still fits: its
/// panel inside the window and Close inside the panel. Rects are in UI
/// units, so the window is divided by the scale.
#[test]
fn the_sheet_fits_1080p_at_its_automatic_scale() {
    let mut h = harness_at(temp_settings("sheet-1080p"), 1920.0, 1080.0);
    let scale = h.world().resource::<bevy::ui::UiScale>().0;
    assert_eq!(scale, 1.25);
    spawn_local_player(&mut h, 6);
    let window = Rect::new(0., 0., 1920. / scale, 1080. / scale);
    h.key(ui::sheet::KEY);
    h.settle();
    assert_eq!(h.stack_top(), Some(ui::sheet::kind()));
    let root = h.rect_of(find(&h, ui::sheet::ROOT_ID));
    assert!(within(root, window), "the sheet {root:?} spills {window:?}");
    let close = h.rect_of(find(&h, ui::sheet::CLOSE_ID));
    assert!(
        within(close, root),
        "Close {close:?} outside the sheet {root:?}"
    );
}

/// The sheet and the bench fit a 1280 by 720 window: the panel inside
/// the window, the Close button inside the panel.
#[test]
fn both_screens_fit_a_small_window() {
    let mut h = harness_at(temp_settings("small-window"), 1280.0, 720.0);
    spawn_local_player(&mut h, 6);
    let window = Rect::new(0., 0., 1280., 720.);
    h.key(ui::sheet::KEY);
    h.settle();
    assert_eq!(h.stack_top(), Some(ui::sheet::kind()));
    let root = h.rect_of(find(&h, ui::sheet::ROOT_ID));
    assert!(within(root, window), "the sheet {root:?} spills the window");
    let close = h.rect_of(find(&h, ui::sheet::CLOSE_ID));
    assert!(
        within(close, root),
        "Close {close:?} outside the sheet {root:?}"
    );
    h.key(ui::pause::PAUSE_KEY);
    h.settle();
    assert!(h.stack().is_empty());

    let one = ui::bench::BenchCard {
        name: "Frontier ranger".to_owned(),
        tint: 0,
        weapon: veyra_sim::combat::Weapon::Carbine,
        sheet: CharacterSheet::default(),
        work: None,
        injured_min: None,
    };
    h.world_mut().insert_resource(ui::bench::BenchView {
        characters: vec![one.clone(), one.clone(), one.clone(), one.clone()],
        active: 0,
        recruits: vec![one],
        capacity: 4,
    });
    h.step(2);
    h.key(ui::pause::PAUSE_KEY);
    h.settle();
    let bench = h.find(&by::role(SemanticRole::Button).tag("menu", "bench"));
    h.click(bench);
    h.settle();
    assert_eq!(h.stack_top(), Some(ui::bench::kind()));
    let root = h.rect_of(find(&h, ui::bench::ROOT_ID));
    assert!(within(root, window), "the bench {root:?} spills the window");
    let close = h.rect_of(find(&h, ui::bench::CLOSE_ID));
    assert!(
        within(close, root),
        "Close {close:?} outside the bench {root:?}"
    );
}

/// The Surveyor's tree at 1280 by 720 shows every box of it, the novice at
/// its foot included, inside the pane the trees are drawn in: nothing is
/// cut off below the fold.
#[test]
fn the_sheet_shows_the_novice_box_at_the_foot_of_a_tree_at_720p() {
    let mut h = harness_at(temp_settings("sheet-720p-tree"), 1280.0, 720.0);
    spawn_local_player(&mut h, 6);
    let commands = &mut h.world_mut().commands();
    ui::sheet::open(commands, Some("surveyor".to_owned()));
    h.settle();
    assert_eq!(h.stack_top(), Some(ui::sheet::kind()));
    let pane = find(&h, ui::sheet::TREE_ID);
    let novice = find(&h, "box:surveyor.novice");
    let view = h.rect_of(pane);
    if !within(h.rect_of(novice), view) {
        // Not fitted, so it must scroll: the scroll runs to its foot and
        // the novice is then in the pane's view.
        let viewport = h
            .world()
            .get::<slotted::ui::ScrollPanel>(pane)
            .unwrap()
            .viewport;
        let content = h
            .world()
            .get::<ComputedNode>(viewport)
            .unwrap()
            .content_size()
            .y;
        h.world_mut()
            .get_mut::<bevy::ui::ScrollPosition>(viewport)
            .unwrap()
            .y = content;
        h.step(2);
        h.settle();
    }
    let at = h.rect_of(novice);
    assert!(at.width() > 0., "the novice box is not laid out");
    assert!(
        within(at, view),
        "the novice box {at:?} is cut off by the tree's pane {view:?}"
    );
    // And the pane itself is not under the body's fold, which would hide the
    // novice with the pane still holding it.
    let body = h.rect_of(find(&h, ui::sheet::BODY_ID));
    assert!(
        within(view, body),
        "the tree's pane {view:?} runs past the sheet's body {body:?}"
    );
}

/// A box that teaches a schematic says what it makes: "Teaches: <product>"
/// in its details, built from the schematics whose profession is the box.
#[test]
fn a_box_that_teaches_a_schematic_says_what_it_makes() {
    let mut day = first_day("sheet-teaches", 1600.0, 900.0);
    let registries = day.registries.clone();
    let schematic = registries
        .schematics
        .iter()
        .find(|schematic| schematic.def.profession.is_some())
        .expect("a schematic taught by a box");
    let box_id = schematic.def.profession.clone().unwrap();
    let product = registries
        .items
        .get(&schematic.def.makes)
        .unwrap()
        .name
        .clone();
    let tree = registries.progression.tree_of(&box_id).unwrap().id.clone();
    let commands = &mut day.h.world_mut().commands();
    ui::sheet::open(commands, Some(tree));
    day.h.settle();
    assert_eq!(day.h.stack_top(), Some(ui::sheet::kind()));
    let card = find(&day.h, &format!("box:{box_id}"));
    day.h.click(card);
    day.h.settle();
    let want = format!("Teaches: {product}");
    let texts: Vec<String> = day
        .h
        .world_mut()
        .query::<&Text>()
        .iter(day.h.world())
        .map(|text| text.0.clone())
        .collect();
    assert!(texts.contains(&want), "no {want:?} among {texts:?}");
}

/// The controls card names the keys the field loop added: G opens the survey
/// tool and V calls the bike or gets off it.
#[test]
fn the_controls_card_lists_g_and_v() {
    let mut h = harness(temp_settings("controls-gv"));
    spawn_local_player(&mut h, 5);
    let texts: Vec<String> = h
        .world_mut()
        .query::<&Text>()
        .iter(h.world())
        .map(|text| text.0.clone())
        .collect();
    let has = |wanted: &str| texts.iter().any(|text| text == wanted);
    assert!(
        has("G") && has("Survey"),
        "no G for the survey tool among {texts:?}"
    );
    assert!(has("V") && has("Bike"), "no V for the bike among {texts:?}");
}
