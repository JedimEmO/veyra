//! The weapon diagram widget: a button for every mount, framing, brackets,
//! tints, focus walks and the boxes drawn when nothing is baked.

use bevy::math::Rect;
use bevy::math::Vec2;
use bevy::picking::pointer::PointerButton;
use bevy::prelude::*;
use slotted::ui::ButtonState;
use slotted::ui::Focusable;
use slotted::ui::SemanticLabel;
use slotted::ui::UiAction;
use slotted::ui::def::Layout;
use slotted::ui::def::Length;
use slotted::ui::def::Padding;
use slotted::ui::def::Presentation;
use slotted::ui::def::ScreenDef;
use slotted::ui::def::Tags;
use slotted::ui::def::UiNodeDef;
use slotted::ui::def::WidgetKind;
use slotted_test::prelude::*;
use strata_client::ui;
use strata_client::ui::weapon_diagram as diagram;
use strata_client::ui::weapon_diagram::Diagram;
use strata_client::ui::weapon_diagram::DiagramMount;
use strata_client::ui::weapon_diagram::DiagramParams;
use strata_client::ui::weapon_diagram::DiagramPiece;
use strata_client::ui::weapon_diagram::MountState;
use strata_client::ui::weapon_diagram::WeaponPictures;
use strata_registry::pictures::ChassisPictures;
use strata_registry::pictures::PictureSet;
use strata_registry::pictures::PiecePicture;
use strata_registry::pictures::Placed;

use crate::common::*;

pub(crate) fn picks(h: &mut UiHarness) -> Vec<String> {
    picks_on(h, "patrol_carbine")
}

pub(crate) fn picks_on(h: &mut UiHarness, chassis: &str) -> Vec<String> {
    std::mem::take(&mut h.world_mut().resource_mut::<Picks>().0)
        .into_iter()
        .map(|pick| {
            assert_eq!(pick.chassis, chassis);
            pick.mount
        })
        .collect()
}

/// One piece of `chassis` in a hand-made index, its file named as the bake
/// names it.
pub(crate) fn piece_of(
    chassis: &str,
    mount: Option<&str>,
    module: Option<&str>,
    rect: [u32; 4],
    hidden: bool,
) -> PiecePicture {
    let stem = match (mount, module) {
        (None, _) => "chassis".to_owned(),
        (Some(mount), None) => format!("{mount}.standard"),
        (Some(mount), Some(module)) => format!("{mount}.{module}"),
    };
    PiecePicture {
        mount: mount.map(str::to_owned),
        module: module.map(str::to_owned),
        rect,
        file: format!("weapons/veyra/{chassis}/{stem}.png"),
        icon: None,
        hidden,
        same_as: None,
    }
}

pub(crate) fn frame_of(
    chassis: &str,
    width: u32,
    height: u32,
    pieces: Vec<PiecePicture>,
) -> ChassisPictures {
    ChassisPictures {
        width,
        height,
        metres_per_pixel: 0.001,
        origin: [0., 0.],
        whole: Placed {
            rect: [0, 0, width, height],
            file: format!("weapons/veyra/{chassis}/whole.png"),
            icon: None,
        },
        pieces,
    }
}

/// A carbine's index made by hand, with no file behind it: four mounts
/// whose rectangles do not meet, a feed the body hides, and a module or
/// two where a test walks them.
pub(crate) fn carbine_pictures() -> WeaponPictures {
    let piece =
        |mount, module, rect, hidden| piece_of("patrol_carbine", mount, module, rect, hidden);
    let mut set = PictureSet::default();
    set.chassis.insert(
        "patrol_carbine".to_owned(),
        frame_of(
            "patrol_carbine",
            1000,
            400,
            vec![
                piece(None, None, [250, 100, 600, 160], false),
                piece(Some("sight"), None, [320, 60, 200, 30], false),
                piece(
                    Some("sight"),
                    Some("vigil_reflex"),
                    [320, 40, 60, 50],
                    false,
                ),
                piece(Some("feed"), None, [480, 120, 80, 100], true),
                piece(Some("stock"), None, [20, 100, 230, 150], false),
                piece(Some("muzzle"), None, [880, 130, 45, 40], false),
                piece(
                    Some("muzzle"),
                    Some("vigil_compensator"),
                    [880, 130, 70, 40],
                    false,
                ),
                piece(
                    Some("muzzle"),
                    Some("vigil_shrouded_muzzle"),
                    [880, 120, 110, 60],
                    false,
                ),
            ],
        ),
    );
    WeaponPictures(set)
}

/// Two frames of the shapes the carbine's is not, as the bake makes them:
/// a pistol about as tall as it is wide, whose feed stands in its grip and
/// is listed before it, and a cutter ten times as long as it is high.
pub(crate) fn odd_pictures() -> WeaponPictures {
    let mut set = PictureSet::default();
    let pistol = |mount, rect| piece_of("service_pistol", mount, None, rect, false);
    set.chassis.insert(
        "service_pistol".to_owned(),
        frame_of(
            "service_pistol",
            950,
            1000,
            vec![
                pistol(None, [100, 100, 750, 200]),
                pistol(Some("feed"), [150, 760, 150, 140]),
                pistol(Some("grip"), [120, 300, 220, 540]),
            ],
        ),
    );
    let cutter = |mount, rect| piece_of("heavy_cutter", mount, None, rect, false);
    set.chassis.insert(
        "heavy_cutter".to_owned(),
        frame_of(
            "heavy_cutter",
            1000,
            100,
            vec![
                cutter(None, [100, 20, 800, 60]),
                cutter(Some("edge"), [0, 0, 1000, 100]),
            ],
        ),
    );
    WeaponPictures(set)
}

pub(crate) fn odd_params(chassis: &str, mounts: &[&str]) -> DiagramParams {
    DiagramParams {
        chassis: chassis.to_owned(),
        mounts: mounts
            .iter()
            .map(|key| DiagramMount {
                key: (*key).to_owned(),
                label: diagram::words(key),
                module: None,
                state: MountState::Standard,
            })
            .collect(),
        focus: None,
        max_height: None,
        fixed: false,
    }
}

/// What a fitting screen would tell the widget: the four mounts of the
/// index and a rail the index has never heard of.
pub(crate) fn carbine_params() -> DiagramParams {
    let mount = |key: &str, label: &str, module: Option<&str>, state| DiagramMount {
        key: key.to_owned(),
        label: label.to_owned(),
        module: module.map(str::to_owned),
        state,
    };
    DiagramParams {
        chassis: "patrol_carbine".to_owned(),
        mounts: vec![
            mount("stock", "Stock", None, MountState::Standard),
            mount("sight", "Sight", Some("vigil_reflex"), MountState::Fitted),
            mount("feed", "Feed", None, MountState::Shut),
            mount("muzzle", "Muzzle", None, MountState::Standard),
            mount("rail", "Rail", None, MountState::Standard),
        ],
        focus: Some("sight".to_owned()),
        max_height: None,
        fixed: false,
    }
}

/// A screen that is the diagram and nothing else, 900 units wide.
pub(crate) fn diagram_screen(params: &DiagramParams) -> ScreenDef {
    ScreenDef {
        kind: ScreenKind::new("test:weapon_diagram"),
        inherits: None,
        root: UiNodeDef::Panel {
            role: slotted::theme::Role::new("panel"),
            layout: Layout {
                width: Some(Length::Px(900.)),
                padding: Padding::all(2.),
                ..Layout::default()
            },
            children: vec![params.node()],
            tags: Tags::new(),
        },
        listring: Vec::new(),
        remove: Vec::new(),
        initial_focus: None,
        presentation: Presentation::default(),
    }
}

pub(crate) fn mount_button(h: &UiHarness, key: &str) -> Entity {
    find(h, &format!("{}{key}", diagram::MOUNT_ID))
}

/// The box the pieces are placed in, which has the frame's own shape.
pub(crate) fn stage(h: &UiHarness) -> Entity {
    find(h, diagram::STAGE_ID)
}

/// Where a rectangle of a `width` by `height` frame lands in `stage`.
pub(crate) fn in_stage(stage: Rect, width: f32, height: f32, rect: [f32; 4]) -> Rect {
    let scale = stage.size() / Vec2::new(width, height);
    let min = stage.min + Vec2::new(rect[0], rect[1]) * scale;
    Rect::from_corners(min, min + Vec2::new(rect[2], rect[3]) * scale)
}

/// The same rectangle to within the pixel the layout rounds each edge to.
pub(crate) fn same_rect(a: Rect, b: Rect) -> bool {
    same_rect_within(a, b, 1.)
}

pub(crate) fn same_rect_within(a: Rect, b: Rect, pixels: f32) -> bool {
    (a.min - b.min).abs().max_element() <= pixels && (a.max - b.max).abs().max_element() <= pixels
}

/// The files of the pieces drawn, by their stems, in the order the
/// stage's children stack them: a later one is drawn over an earlier.
pub(crate) fn drawn(h: &UiHarness) -> Vec<String> {
    let world = h.world();
    let Some(root) = h.try_find(&by::test_id(diagram::STAGE_ID)) else {
        return Vec::new();
    };
    let children = |entity: Entity| -> Vec<Entity> {
        world
            .get::<Children>(entity)
            .map(|c| c.iter().collect())
            .unwrap_or_default()
    };
    let mut stems = Vec::new();
    for child in children(root) {
        // The body's image is the child itself; a mount's is its button's.
        for entity in std::iter::once(child).chain(children(child)) {
            if let Some(piece) = world.get::<DiagramPiece>(entity) {
                let stem = piece
                    .file
                    .rsplit('/')
                    .next()
                    .and_then(|name| name.strip_suffix(".png"))
                    .expect("a png under the chassis's directory");
                stems.push(stem.to_owned());
            }
        }
    }
    stems
}

/// Every drawn piece's file and tint, sorted by file.
pub(crate) fn tints(h: &mut UiHarness) -> Vec<(String, Color)> {
    let world = h.world_mut();
    let mut pieces = world.query::<(&DiagramPiece, &ImageNode)>();
    let mut tints: Vec<(String, Color)> = pieces
        .iter(world)
        .map(|(piece, image)| (piece.file.clone(), image.color))
        .collect();
    tints.sort_by(|a, b| a.0.cmp(&b.0));
    tints
}

pub(crate) fn shown(h: &UiHarness, id: &str) -> String {
    h.text_of(find(h, id)).expect("a text")
}

/// With pictures: a button a mount, named and found by its key, the body
/// drawn and not to be picked, the pieces where their rectangles say.
#[test]
fn the_weapon_diagram_has_a_button_for_every_mount_and_none_for_the_body() {
    let mut h = harness(temp_settings("diagram-buttons"));
    h.world_mut().insert_resource(carbine_pictures());
    let params = carbine_params();
    h.open(diagram_screen(&params));
    h.settle();

    let root = find(&h, diagram::DIAGRAM_ID);
    assert!(h.world().get::<Diagram>(root).unwrap().pictured);
    for mount in &params.mounts {
        let button = mount_button(&h, &mount.key);
        let world = h.world();
        assert_eq!(
            world.get::<SemanticRole>(button),
            Some(&SemanticRole::Button),
            "{}",
            mount.key
        );
        assert_eq!(
            world.get::<SemanticLabel>(button).map(|l| l.0.as_str()),
            Some(mount.label.as_str())
        );
        assert!(world.get::<Focusable>(button).is_some(), "{}", mount.key);
        assert!(h.is_visible(button), "{}", mount.key);
    }
    let buttons = h.find_all(&by::role(SemanticRole::Button).within(root));
    assert_eq!(buttons.len(), params.mounts.len(), "the body is no button");
    // A mount is a widget of its own kind, as a built-in button is of its,
    // and a marker's label is not a second node of the tree beside it.
    let kind = WidgetKind::new(diagram::MOUNT_WIDGET);
    assert_eq!(h.find_all(&by::widget_kind(kind).within(root)), buttons);
    assert!(
        h.find_all(&by::role(SemanticRole::Text).within(root))
            .is_empty(),
        "a marker's label is named by its button alone"
    );

    // The body first, then the mounts as the index stacks them, the sight
    // by the module the caller named.
    assert_eq!(
        drawn(&h),
        [
            "chassis",
            "sight.vigil_reflex",
            "stock.standard",
            "muzzle.standard"
        ]
    );
    let world = h.world_mut();
    let mut pieces = world.query::<(Entity, &DiagramPiece)>();
    let body = pieces
        .iter(world)
        .find(|(_, piece)| piece.mount.is_none())
        .map(|(entity, _)| entity)
        .expect("the body is drawn");
    assert!(world.get::<ButtonState>(body).is_none());
    let stage = stage(&h);
    let parent = |entity| h.world().get::<ChildOf>(entity).map(ChildOf::parent);
    assert_eq!(parent(body), Some(stage));
    assert_eq!(parent(stage), Some(root));

    // The frame's shape, and each piece in it where its rectangle says,
    // across and down.
    let frame = h.rect_of(stage);
    assert!(within(frame, h.rect_of(root)));
    assert!(
        (frame.width() / frame.height() - 2.5).abs() < 0.02,
        "{frame:?}"
    );
    assert!(same_rect(
        h.rect_of(body),
        in_stage(frame, 1000., 400., [250., 100., 600., 160.])
    ));
    for (key, rect) in [
        ("sight", [320., 40., 60., 50.]),
        ("stock", [20., 100., 230., 150.]),
        ("muzzle", [880., 130., 45., 40.]),
    ] {
        let placed = h.rect_of(mount_button(&h, key));
        let wanted = in_stage(frame, 1000., 400., rect);
        assert!(
            same_rect(placed, wanted),
            "{key}: {placed:?}, not {wanted:?}"
        );
    }
    let sight = h.rect_of(mount_button(&h, "sight"));
    let stock = h.rect_of(mount_button(&h, "stock"));
    let muzzle = h.rect_of(mount_button(&h, "muzzle"));
    assert!(stock.max.x < muzzle.min.x, "{stock:?} {muzzle:?}");
    assert!(sight.max.y < stock.min.y, "{sight:?} {stock:?}");
    for key in ["feed", "rail"] {
        let marker = h.rect_of(mount_button(&h, key));
        assert!(within(marker, frame), "{key}: {marker:?} outside {frame:?}");
    }

    // A click on the body where no mount lies over it picks nothing.
    let bare = in_stage(frame, 1000., 400., [700., 200., 0., 0.]).min;
    h.click_at(bare, PointerButton::Primary);
    h.settle();
    assert!(picks(&mut h).is_empty(), "the body is not to be picked");
}

/// The diagram is as wide as its parent until that would make it taller
/// than its share of the window; then it is as tall as that and the well
/// narrows to the weapon's own shape, in the middle of its parent. A
/// pistol's frame at a carbine's width is taller than the window.
#[test]
fn a_tall_frame_is_held_to_the_window_and_a_low_one_keeps_its_shape() {
    let mut h = harness(temp_settings("diagram-shapes"));
    h.world_mut().insert_resource(odd_pictures());

    // Ten to one: the stage is the frame's shape and nothing else's, so a
    // piece is scaled the same across and down.
    h.open(diagram_screen(&odd_params("heavy_cutter", &["edge"])));
    h.settle();
    let well = h.rect_of(find(&h, diagram::DIAGRAM_ID));
    let low = h.rect_of(stage(&h));
    assert!(within(low, well), "{low:?} outside {well:?}");
    assert!(
        well.width() > 850.,
        "as wide as the panel lets it: {well:?}"
    );
    assert!(low.width() > well.width() - 3., "{low:?} in {well:?}");
    assert!(
        (low.width() / low.height() / 10. - 1.).abs() < 0.012,
        "{low:?}"
    );
    // A piece that is the whole frame is the whole stage, edge on edge:
    // the stage has no border or padding for a percentage to be measured
    // inside of, which would scale a low frame's pieces more down than
    // across.
    let edge = h.rect_of(mount_button(&h, "edge"));
    assert!(same_rect_within(edge, low, 0.1), "{edge:?}, not {low:?}");
    let wide = well;
    h.key(ui::pause::PAUSE_KEY);
    h.settle();

    h.open(diagram_screen(&odd_params(
        "service_pistol",
        &["feed", "grip"],
    )));
    h.settle();
    let well = h.rect_of(find(&h, diagram::DIAGRAM_ID));
    let tall = h.rect_of(stage(&h));
    assert!(
        well.height() < 0.61 * 900.,
        "no taller than its share of the window: {well:?}"
    );
    assert!(well.height() > 0.5 * 900., "and no shorter: {well:?}");
    assert!(within(tall, well), "{tall:?} outside {well:?}");
    assert!(
        (tall.width() / tall.height() / 0.95 - 1.).abs() < 0.01,
        "{tall:?}"
    );
    // The well is the pistol's shape and not the panel's width with the
    // pistol small inside it, and it stands where the wide one's middle
    // was.
    assert!(
        well.width() < tall.width() + 4.,
        "the well narrows to the weapon: {well:?} round {tall:?}"
    );
    assert!(well.width() < 0.7 * wide.width(), "{well:?} of {wide:?}");
    assert!(
        (well.center().x - wide.center().x).abs() <= 1.,
        "{well:?} is not in the middle of {wide:?}"
    );
    let grip = h.rect_of(mount_button(&h, "grip"));
    let wanted = in_stage(tall, 950., 1000., [120., 300., 220., 540.]);
    assert!(same_rect(grip, wanted), "{grip:?}, not {wanted:?}");
    h.key(ui::pause::PAUSE_KEY);
    h.settle();

    // A screen with more to show under the drawing sets it a lower limit,
    // and a carbine's well narrows to it in the same way.
    h.world_mut().insert_resource(carbine_pictures());
    let limited = DiagramParams {
        max_height: Some(20.),
        ..carbine_params()
    };
    h.open(diagram_screen(&limited));
    h.settle();
    let well = h.rect_of(find(&h, diagram::DIAGRAM_ID));
    let stage = h.rect_of(stage(&h));
    assert!(well.height() <= 0.2 * 900. + 1., "{well:?}");
    assert!(well.height() > 0.19 * 900., "{well:?}");
    assert!(
        (stage.width() / stage.height() / 2.5 - 1.).abs() < 0.01,
        "{stage:?}"
    );
    // Its limit is the frame's shape at the height limit, border and all,
    // so it is wider than the stage by the border's share of that shape.
    assert!(
        well.width() < stage.width() + 8.,
        "{well:?} round {stage:?}"
    );
    assert!(
        (well.center().x - wide.center().x).abs() <= 1.,
        "{well:?} is not in the middle of {wide:?}"
    );
}

/// The mount in focus is marked by more than its tint: four corners stand
/// round its part, never closer than a cell of a grid however small the
/// part, with its name on a tag. The bracket is no target: a click inside
/// it lands on the part.
#[test]
fn the_focused_mount_is_bracketed_and_a_small_part_gets_a_large_bracket() {
    let mut h = harness(temp_settings("diagram-bracket"));
    h.world_mut().insert_resource(carbine_pictures());
    // The muzzle's piece is 45 by 40 of a frame 1000 across.
    let params = DiagramParams {
        focus: Some("muzzle".to_owned()),
        max_height: Some(20.),
        ..carbine_params()
    };
    h.open(diagram_screen(&params));
    h.settle();
    let muzzle = h.rect_of(mount_button(&h, "muzzle"));
    assert!(
        muzzle.width() < 30. && muzzle.height() < 30.,
        "a small part at this size: {muzzle:?}"
    );
    let bracket = h.rect_of(find(&h, diagram::BRACKET_ID));
    assert!(
        bracket.width() >= diagram::BRACKET_MIN - 0.5
            && bracket.height() >= diagram::BRACKET_MIN - 0.5,
        "{bracket:?}"
    );
    assert!(
        (bracket.center() - muzzle.center()).abs().max_element() <= 1.,
        "{bracket:?} is not about {muzzle:?}"
    );
    let label = find(&h, diagram::BRACKET_LABEL_ID);
    assert!(h.is_visible(label));
    let said = h
        .world()
        .get::<Children>(label)
        .and_then(|children| children.iter().find_map(|child| h.text_of(child)));
    assert_eq!(said.as_deref(), Some("\u{203a} MUZZLE"));
    // The tag stands in the frame's lower right corner, clear of the
    // bracket and of every part.
    let tag = h.rect_of(label);
    let frame = h.rect_of(stage(&h));
    assert!(
        frame.contains(tag.min) && frame.contains(tag.max),
        "{tag:?} leaves {frame:?}"
    );
    assert!(
        frame.max.x - tag.max.x < 10. && frame.max.y - tag.max.y < 10.,
        "{tag:?} is not in the corner of {frame:?}"
    );
    assert!(bracket.intersect(tag).is_empty(), "{tag:?} on {bracket:?}");
    for mount in ["stock", "sight", "muzzle"] {
        let part = h.rect_of(mount_button(&h, mount));
        assert!(part.intersect(tag).is_empty(), "{tag:?} on the {mount}");
    }
    // One bracket, and it takes no click from the part inside it.
    assert_eq!(h.find_all(&by::test_id(diagram::BRACKET_ID)).len(), 1);
    h.click_at(muzzle.center(), PointerButton::Primary);
    h.settle();
    assert_eq!(picks(&mut h), ["muzzle"]);
    h.key(ui::pause::PAUSE_KEY);
    h.settle();

    // A large part's bracket is the part's own size, and a mount the
    // index has no place for has none: its box in the row says it is in
    // focus by its frame and its mark.
    let stock = DiagramParams {
        focus: Some("stock".to_owned()),
        ..carbine_params()
    };
    h.open(diagram_screen(&stock));
    h.settle();
    let part = h.rect_of(mount_button(&h, "stock"));
    let bracket = h.rect_of(find(&h, diagram::BRACKET_ID));
    assert!(same_rect(bracket, part), "{bracket:?}, not {part:?}");
    h.key(ui::pause::PAUSE_KEY);
    h.settle();
    let rail = DiagramParams {
        focus: Some("rail".to_owned()),
        ..carbine_params()
    };
    h.open(diagram_screen(&rail));
    h.settle();
    assert!(h.try_find(&by::test_id(diagram::BRACKET_ID)).is_none());
    assert_eq!(box_words(&h, "rail"), "\u{203a} RAIL");
    assert_eq!(box_words(&h, "feed"), "FEED");
}

/// The words on a mount's box or marker.
pub(crate) fn box_words(h: &UiHarness, key: &str) -> String {
    h.world()
        .get::<Children>(mount_button(h, key))
        .and_then(|children| children.iter().find_map(|child| h.text_of(child)))
        .unwrap_or_default()
}

/// A piece has its state's tint from the moment it is spawned. A pick
/// replaces the screen, and a piece left for the tinting system to find
/// would be drawn white for a frame each time.
#[test]
fn a_piece_is_spawned_in_its_tint() {
    let mut h = harness(temp_settings("diagram-tint"));
    h.world_mut().insert_resource(carbine_pictures());
    let def = std::sync::Arc::new(diagram_screen(&carbine_params()));
    let world = h.world_mut();
    slotted::ui::push_screen(&mut world.commands(), def, None);
    world.flush();
    // No frame has run: this is what the first one draws.
    let spawned = tints(&mut h);
    assert_eq!(spawned.len(), 4, "the body and three mounts: {spawned:?}");
    for (file, tint) in &spawned {
        assert_ne!(*tint, Color::WHITE, "{file} is spawned untinted");
    }
    // The body, the focused sight and the two standard parts.
    let mut distinct: Vec<Color> = Vec::new();
    for (_, tint) in &spawned {
        if !distinct.contains(tint) {
            distinct.push(*tint);
        }
    }
    assert_eq!(distinct.len(), 3, "{spawned:?}");
    h.settle();
    assert_eq!(tints(&mut h), spawned, "the system agrees with the spawn");

    // The same across the rebuild a pick causes on a screen that swaps its
    // definition: the focus has moved, and nothing is white on the way.
    let moved = DiagramParams {
        focus: Some("muzzle".to_owned()),
        ..carbine_params()
    };
    let def = std::sync::Arc::new(diagram_screen(&moved));
    let world = h.world_mut();
    slotted::ui::replace_screen(&mut world.commands(), def, None);
    world.flush();
    let rebuilt = tints(&mut h);
    assert_ne!(rebuilt, spawned, "the focus moved, so the tints did");
    for (file, tint) in &rebuilt {
        assert_ne!(*tint, Color::WHITE, "{file} is white after the rebuild");
    }
    h.settle();
    assert_eq!(tints(&mut h), rebuilt);
}

/// The keyboard and the gamepad walk the mounts as they lie on the
/// picture, markers too, and Tab goes from one to the next.
#[test]
fn the_directions_and_tab_walk_the_mounts() {
    let mut h = harness(temp_settings("diagram-walk"));
    h.world_mut().insert_resource(carbine_pictures());
    h.open(diagram_screen(&carbine_params()));
    h.settle();
    let name = |h: &UiHarness| {
        h.focused()
            .and_then(|entity| h.world().get::<diagram::MountButton>(entity))
            .map(|mount| mount.mount.clone())
    };
    let stock = mount_button(&h, "stock");
    h.set_focus(Some(stock));
    h.step(1);
    let mut walk = Vec::new();
    for _ in 0..3 {
        h.action(UiAction::Right);
        h.settle();
        walk.push(name(&h).expect("a mount has the focus"));
    }
    // The hidden feed's marker lies between the sight and the muzzle.
    assert_eq!(walk, ["sight", "feed", "muzzle"]);
    h.action(UiAction::Accept);
    h.settle();
    assert_eq!(picks(&mut h), ["muzzle"]);
    h.action(UiAction::Left);
    h.settle();
    assert_eq!(name(&h).as_deref(), Some("feed"));

    // Tab takes them in the order they are stacked: the pieces as the
    // index lists them, then the markers.
    let sight = mount_button(&h, "sight");
    h.set_focus(Some(sight));
    h.step(1);
    let mut tabbed = Vec::new();
    for _ in 0..4 {
        h.focus_next();
        h.settle();
        tabbed.push(name(&h).expect("a mount has the focus"));
    }
    assert_eq!(tabbed, ["stock", "muzzle", "feed", "rail"]);
}

/// Picking is by rectangle: where two pieces' rectangles overlap, the one
/// later in the index, which is the one drawn on top, takes the click. So
/// a pistol's feed, which stands in its grip and is listed before it, is
/// picked only where it shows below the grip's rectangle. Ticket 0007's
/// fitting screen has to weigh that; this pins what the widget does.
#[test]
fn where_two_pieces_overlap_the_later_in_the_index_is_picked() {
    let mut h = harness(temp_settings("diagram-overlap"));
    h.world_mut().insert_resource(odd_pictures());
    h.open(diagram_screen(&odd_params(
        "service_pistol",
        &["feed", "grip"],
    )));
    h.settle();
    assert_eq!(drawn(&h), ["chassis", "feed.standard", "grip.standard"]);
    let frame = h.rect_of(stage(&h));
    let at = |x: f32, y: f32| in_stage(frame, 950., 1000., [x, y, 0., 0.]).min;
    // Inside both rectangles.
    h.click_at(at(225., 800.), PointerButton::Primary);
    h.settle();
    assert_eq!(picks_on(&mut h, "service_pistol"), ["grip"]);
    // Below the grip's rectangle, inside the feed's alone.
    h.click_at(at(225., 880.), PointerButton::Primary);
    h.settle();
    assert_eq!(picks_on(&mut h, "service_pistol"), ["feed"]);
}

/// A click on the muzzle's piece is the pick, and Escape closes the
/// screen it was on, as it does every screen.
#[test]
fn clicking_a_mount_picks_it() {
    let mut h = harness(temp_settings("diagram-click"));
    h.world_mut().insert_resource(carbine_pictures());
    h.open(diagram_screen(&carbine_params()));
    h.settle();
    let muzzle = mount_button(&h, "muzzle");
    h.click(muzzle);
    h.settle();
    assert_eq!(picks(&mut h), ["muzzle"]);
    h.key(ui::pause::PAUSE_KEY);
    h.settle();
    assert!(h.stack().is_empty());
}

/// The accept action on a focused mount is the same pick as the click.
#[test]
fn accept_on_a_focused_mount_picks_it() {
    let mut h = harness(temp_settings("diagram-accept"));
    h.world_mut().insert_resource(carbine_pictures());
    h.open(diagram_screen(&carbine_params()));
    h.settle();
    let stock = mount_button(&h, "stock");
    h.world_mut()
        .resource_mut::<bevy::input_focus::InputFocus>()
        .set(stock, bevy::input_focus::FocusCause::Navigated);
    h.step(1);
    h.action(UiAction::Accept);
    h.settle();
    assert_eq!(picks(&mut h), ["stock"]);
}

/// A part the body hides is no piece of the stack, and its mount is still
/// there to pick; so is a mount the index does not know.
#[test]
fn a_hidden_piece_draws_no_image_and_its_mount_is_still_picked() {
    let mut h = harness(temp_settings("diagram-hidden"));
    h.world_mut().insert_resource(carbine_pictures());
    h.open(diagram_screen(&carbine_params()));
    h.settle();
    assert!(
        !drawn(&h).iter().any(|stem| stem.starts_with("feed")),
        "the hidden feed is not stacked"
    );
    for key in ["feed", "rail"] {
        let marker = mount_button(&h, key);
        h.click(marker);
        h.settle();
        assert_eq!(picks(&mut h), [key]);
    }
}

/// With no pictures the widget is a row of boxes under the same ids, and
/// the box of the mount in focus has a mark before its name.
#[test]
fn without_pictures_the_mounts_are_boxes_with_the_same_ids() {
    let mut h = harness(temp_settings("diagram-boxes"));
    assert!(
        h.world().resource::<WeaponPictures>().0.chassis.is_empty(),
        "nothing fills the pictures headless"
    );
    let params = carbine_params();
    h.open(diagram_screen(&params));
    h.settle();
    let root = find(&h, diagram::DIAGRAM_ID);
    assert!(!h.world().get::<Diagram>(root).unwrap().pictured);
    assert!(drawn(&h).is_empty());
    let frame = h.rect_of(root);
    for mount in &params.mounts {
        let button = mount_button(&h, &mount.key);
        assert_eq!(
            h.world().get::<SemanticLabel>(button).map(|l| l.0.as_str()),
            Some(mount.label.as_str())
        );
        let rect = h.rect_of(button);
        assert!(within(rect, frame), "{}: {rect:?}", mount.key);
    }
    let muzzle = mount_button(&h, "muzzle");
    h.click(muzzle);
    h.settle();
    assert_eq!(picks(&mut h), ["muzzle"]);
    assert_eq!(box_words(&h, "sight"), "\u{203a} SIGHT");
    assert_eq!(box_words(&h, "muzzle"), "MUZZLE");
    assert!(
        h.try_find(&by::test_id(diagram::BRACKET_ID)).is_none(),
        "there is no part to bracket"
    );
}
