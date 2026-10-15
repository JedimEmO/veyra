//! Native studio controls keep edits explicit while leaving the model viewport clear.

use bevy::input::mouse::{MouseScrollUnit, MouseWheel};
use bevy::prelude::*;
use bevy::ui::FocusPolicy;
use bevy::window::PrimaryWindow;
use veyra_weapons::{Finish, MODULES, Maker, Weapon, modules, presets};

use crate::{Action, StudioActions, Workbench};

/// Width reserved for the catalogue and excluded from camera input.
pub const LEFT_WIDTH: f32 = 250.0;
/// Width reserved for the assembly panel and excluded from camera input.
pub const RIGHT_WIDTH: f32 = 320.0;
/// Height of the persistent studio identity strip.
pub const HEADER_HEIGHT: f32 = 70.0;
/// Height reserved for the viewport tools and interaction hint.
pub const FOOTER_HEIGHT: f32 = 110.0;

const PANEL: Color = Color::srgb(0.055, 0.075, 0.095);
const BUTTON: Color = Color::srgb(0.085, 0.115, 0.140);
const HOVER: Color = Color::srgb(0.145, 0.180, 0.200);
const ACTIVE: Color = Color::srgb(0.245, 0.190, 0.095);
const IVORY: Color = Color::srgb(0.94, 0.92, 0.86);
const MUTED: Color = Color::srgb(0.64, 0.71, 0.73);
const AMBER: Color = Color::srgb(0.98, 0.69, 0.31);
const LINE: Color = Color::srgb(0.20, 0.26, 0.29);

/// A retained native button action, also available to scripted interaction checks.
#[derive(Component)]
pub struct ButtonAction(pub Action);

#[derive(Component)]
struct HeaderLabel;
#[derive(Component)]
struct StatusLabel;
#[derive(Component)]
enum ScrollPanel {
    Catalogue,
    Build,
}
#[derive(Component)]
struct CatalogueContents;
#[derive(Component)]
struct BuildContents;
#[derive(Component)]
struct GeometryLabel;

/// Install retained panels and their interaction systems; the app owns actions.
pub fn plugin(app: &mut App) {
    app.add_systems(Startup, setup).add_systems(
        Update,
        (
            handle_buttons,
            sync,
            sync_catalogue,
            reset_scroll,
            apply_fonts,
            scroll_panel,
        )
            .chain(),
    );
}

fn label(
    commands: &mut Commands,
    parent: Entity,
    value: impl Into<String>,
    size: f32,
    color: Color,
) -> Entity {
    commands
        .spawn((
            Text::new(value),
            TextFont {
                font_size: FontSize::Px(size),
                ..default()
            },
            TextColor(color),
            ChildOf(parent),
        ))
        .id()
}

fn section(commands: &mut Commands, parent: Entity, title: &str) {
    let heading = label(commands, parent, title, 12.0, AMBER);
    commands.entity(heading).insert(Node {
        margin: UiRect {
            top: px(12),
            bottom: px(4),
            ..default()
        },
        ..default()
    });
}

fn button(
    commands: &mut Commands,
    parent: Entity,
    title: impl Into<String>,
    action: Action,
    selected: bool,
) -> Entity {
    let entity = commands
        .spawn((
            Button,
            ButtonAction(action),
            Node {
                width: percent(100),
                min_height: px(30),
                padding: UiRect::axes(px(10), px(7)),
                border: UiRect::all(px(1)),
                border_radius: BorderRadius::all(px(3)),
                align_items: AlignItems::Center,
                flex_shrink: 0.0,
                ..default()
            },
            BackgroundColor(if selected { ACTIVE } else { BUTTON }),
            BorderColor::all(if selected { AMBER } else { LINE }),
            ChildOf(parent),
        ))
        .id();
    label(commands, entity, title, 14.0, IVORY);
    entity
}

#[allow(
    clippy::too_many_lines,
    reason = "declarative layout of the four persistent studio panels"
)]
fn setup(mut commands: Commands, state: Res<Workbench>) {
    let header = commands
        .spawn((
            Node {
                position_type: PositionType::Absolute,
                left: px(0),
                top: px(0),
                width: percent(100),
                height: px(HEADER_HEIGHT),
                padding: UiRect::axes(px(24), px(12)),
                flex_direction: FlexDirection::Column,
                row_gap: px(5),
                border: UiRect::bottom(px(1)),
                ..default()
            },
            BackgroundColor(PANEL),
            BorderColor::all(LINE),
            FocusPolicy::Block,
            GlobalZIndex(10),
        ))
        .id();
    label(
        &mut commands,
        header,
        "VEYRA  /  ARMORY WORKBENCH",
        13.0,
        AMBER,
    );
    let title = label(&mut commands, header, "", 24.0, IVORY);
    commands.entity(title).insert(HeaderLabel);

    let catalogue = commands
        .spawn((
            ScrollPanel::Catalogue,
            ScrollPosition::default(),
            Node {
                position_type: PositionType::Absolute,
                left: px(0),
                top: px(HEADER_HEIGHT),
                bottom: px(0),
                width: px(LEFT_WIDTH),
                overflow: Overflow::scroll_y(),
                border: UiRect::right(px(1)),
                ..default()
            },
            BackgroundColor(PANEL),
            BorderColor::all(LINE),
            FocusPolicy::Block,
            GlobalZIndex(10),
        ))
        .id();
    let content = commands
        .spawn((
            CatalogueContents,
            Node {
                width: percent(100),
                padding: UiRect::all(px(16)),
                flex_direction: FlexDirection::Column,
                row_gap: px(8),
                flex_shrink: 0.0,
                ..default()
            },
            ChildOf(catalogue),
        ))
        .id();
    populate_catalogue(&mut commands, content, &state);

    let right = commands
        .spawn((
            ScrollPanel::Build,
            ScrollPosition::default(),
            Node {
                position_type: PositionType::Absolute,
                right: px(0),
                top: px(HEADER_HEIGHT),
                bottom: px(88),
                width: px(RIGHT_WIDTH),
                overflow: Overflow::scroll_y(),
                border: UiRect::left(px(1)),
                ..default()
            },
            BackgroundColor(PANEL),
            BorderColor::all(LINE),
            FocusPolicy::Block,
            GlobalZIndex(10),
        ))
        .id();
    let content = commands
        .spawn((
            BuildContents,
            Node {
                width: percent(100),
                padding: UiRect::all(px(16)),
                flex_direction: FlexDirection::Column,
                row_gap: px(6),
                flex_shrink: 0.0,
                ..default()
            },
            ChildOf(right),
        ))
        .id();
    populate_build(&mut commands, content, &state);

    let files = commands
        .spawn((
            Node {
                position_type: PositionType::Absolute,
                right: px(0),
                bottom: px(0),
                width: px(RIGHT_WIDTH),
                height: px(88),
                padding: UiRect::all(px(12)),
                flex_direction: FlexDirection::Column,
                row_gap: px(7),
                border: UiRect::left(px(1)),
                ..default()
            },
            BackgroundColor(PANEL),
            BorderColor::all(LINE),
            FocusPolicy::Block,
            GlobalZIndex(10),
        ))
        .id();
    let actions = row(&mut commands, files);
    for (title, action) in [
        ("Save", Action::Save),
        ("Load", Action::Load),
        ("Export", Action::Export),
    ] {
        let entity = button(&mut commands, actions, title, action, false);
        commands.entity(entity).insert(Node {
            flex_grow: 1.0,
            flex_basis: px(0),
            padding: UiRect::all(px(8)),
            justify_content: JustifyContent::Center,
            ..default()
        });
    }
    let status = label(&mut commands, files, &state.status, 10.0, MUTED);
    commands.entity(status).insert(StatusLabel);

    let footer = commands
        .spawn((
            Node {
                position_type: PositionType::Absolute,
                left: px(LEFT_WIDTH + 18.0),
                right: px(RIGHT_WIDTH + 18.0),
                bottom: px(12),
                height: px(FOOTER_HEIGHT - 14.0),
                flex_direction: FlexDirection::Column,
                align_items: AlignItems::Center,
                row_gap: px(8),
                ..default()
            },
            FocusPolicy::Block,
            GlobalZIndex(10),
        ))
        .id();
    let controls = row(&mut commands, footer);
    commands.entity(controls).insert(Node {
        flex_direction: FlexDirection::Row,
        flex_wrap: FlexWrap::Wrap,
        justify_content: JustifyContent::Center,
        column_gap: px(6),
        row_gap: px(6),
        ..default()
    });
    for (title, action) in [
        ("Exploded", Action::ToggleExploded),
        ("Auto orbit", Action::ToggleRotate),
        ("Clay", Action::ToggleClay),
        ("Stock reference", Action::ToggleCompare),
        ("Side", Action::ViewSide),
        ("Top", Action::ViewTop),
        ("Perspective", Action::ResetView),
        ("Stock build", Action::ResetBuild),
    ] {
        let entity = button(&mut commands, controls, title, action, false);
        commands.entity(entity).insert(Node {
            padding: UiRect::axes(px(10), px(7)),
            border: UiRect::all(px(1)),
            border_radius: BorderRadius::all(px(3)),
            ..default()
        });
    }
    label(
        &mut commands,
        footer,
        "DRAG TO ORBIT  /  SCROLL TO ZOOM  /  SCROLL EITHER PANEL",
        10.0,
        MUTED,
    );
    let statistics = commands
        .spawn((
            Node {
                position_type: PositionType::Absolute,
                left: px(LEFT_WIDTH + 24.0),
                top: px(HEADER_HEIGHT + 18.0),
                ..default()
            },
            GlobalZIndex(10),
        ))
        .id();
    let text = label(&mut commands, statistics, "", 11.0, MUTED);
    commands.entity(text).insert(GeometryLabel);
}

fn apply_fonts(server: Option<Res<AssetServer>>, mut texts: Query<&mut TextFont, Added<Text>>) {
    if let Some(server) = server {
        let font = server.load("fonts/manrope/Manrope-Variable.ttf");
        for mut text in &mut texts {
            text.font = FontSource::Handle(font.clone());
        }
    }
}

fn row(commands: &mut Commands, parent: Entity) -> Entity {
    commands
        .spawn((
            Node {
                column_gap: px(6),
                flex_direction: FlexDirection::Row,
                ..default()
            },
            ChildOf(parent),
        ))
        .id()
}

fn populate_catalogue(commands: &mut Commands, parent: Entity, state: &Workbench) {
    section(commands, parent, "01  /  CHASSIS CATALOGUE");
    label(
        commands,
        parent,
        format!(
            "{} chassis / {} components",
            Weapon::ALL.len(),
            MODULES.len()
        ),
        12.0,
        MUTED,
    );
    let filters = row(commands, parent);
    commands.entity(filters).insert(Node {
        flex_direction: FlexDirection::Row,
        flex_wrap: FlexWrap::Wrap,
        column_gap: px(5),
        row_gap: px(5),
        flex_shrink: 0.0,
        ..default()
    });
    for maker in std::iter::once(None).chain(Maker::ALL.into_iter().map(Some)) {
        let entity = button(
            commands,
            filters,
            maker.map_or("All", Maker::label),
            Action::Maker(maker),
            state.maker_filter == maker,
        );
        commands.entity(entity).insert(Node {
            padding: UiRect::axes(px(8), px(6)),
            border: UiRect::all(px(1)),
            border_radius: BorderRadius::all(px(3)),
            flex_shrink: 0.0,
            ..default()
        });
    }
    if let Some(maker) = state.maker_filter {
        label(commands, parent, maker.description(), 11.0, MUTED);
    }
    for weapon in Weapon::ALL.into_iter().filter(|weapon| {
        state
            .maker_filter
            .is_none_or(|maker| weapon.maker() == maker)
    }) {
        let entity = button(
            commands,
            parent,
            weapon.label(),
            Action::Weapon(weapon),
            weapon == state.assembly.weapon,
        );
        commands.entity(entity).insert(Node {
            width: percent(100),
            min_height: px(58),
            padding: UiRect::axes(px(11), px(8)),
            border: UiRect::all(px(1)),
            border_radius: BorderRadius::all(px(3)),
            flex_direction: FlexDirection::Column,
            align_items: AlignItems::Start,
            justify_content: JustifyContent::Center,
            row_gap: px(4),
            flex_shrink: 0.0,
            ..default()
        });
        label(commands, entity, weapon.maker().label(), 10.0, MUTED);
    }
    label(
        commands,
        parent,
        "Choose a chassis, then try a configuration or fit individual components. Scroll for more.",
        11.0,
        MUTED,
    );
}

fn sync_catalogue(
    mut commands: Commands,
    state: Res<Workbench>,
    mut previous: Local<(bool, Option<Maker>)>,
    catalogue: Query<Entity, With<CatalogueContents>>,
) {
    if previous.0
        && previous.1 != state.maker_filter
        && let Ok(parent) = catalogue.single()
    {
        commands.entity(parent).despawn_children();
        populate_catalogue(&mut commands, parent, &state);
    }
    *previous = (true, state.maker_filter);
}

fn populate_build(commands: &mut Commands, parent: Entity, state: &Workbench) {
    section(commands, parent, "02  /  FITTING BENCH");
    label(
        commands,
        parent,
        state.assembly.weapon.description(),
        12.0,
        MUTED,
    );
    section(commands, parent, "CONFIGURATIONS");
    button(
        commands,
        parent,
        "Stock build",
        Action::ResetBuild,
        state.assembly.modules.is_empty(),
    );
    for preset in presets::for_weapon(state.assembly.weapon) {
        let action = Action::Preset(preset.id.to_owned());
        let selected = active(&action, state);
        button(commands, parent, preset.name, action, selected);
        if selected {
            label(commands, parent, preset.description, 10.0, MUTED);
        }
    }
    section(commands, parent, "MATERIAL FINISH");
    for finish in Finish::ALL {
        button(
            commands,
            parent,
            finish.label(),
            Action::Finish(finish),
            finish == state.assembly.finish,
        );
    }
    section(commands, parent, "COMPONENTS");
    for slot in state.assembly.weapon.slots() {
        section(commands, parent, slot.label());
        let selected = state.assembly.selected(*slot);
        button(
            commands,
            parent,
            "Stock assembly",
            Action::Module(*slot, None),
            selected.is_none(),
        );
        for module in modules(state.assembly.weapon, *slot) {
            button(
                commands,
                parent,
                module.name,
                Action::Module(*slot, Some(module.id.to_owned())),
                selected.is_some_and(|current| current.id == module.id),
            );
            if selected.is_some_and(|current| current.id == module.id) {
                label(commands, parent, module.description, 10.0, MUTED);
            }
        }
    }
    section(commands, parent, "CONFIGURATION TRADEOFFS");
    let factors = state.assembly.factor_summary();
    if factors.is_empty() {
        label(commands, parent, "Stock properties / 1.00×", 11.0, IVORY);
    } else {
        for (name, factor) in factors {
            label(
                commands,
                parent,
                format!("{}  {factor:.2}×", name.replace('_', " ")),
                11.0,
                IVORY,
            );
        }
    }
    label(
        commands,
        parent,
        "Relative to this chassis stock.\nHandling tradeoffs, not a power rating.",
        10.0,
        MUTED,
    );
}

fn active(action: &Action, state: &Workbench) -> bool {
    match action {
        Action::Weapon(weapon) => *weapon == state.assembly.weapon,
        Action::Maker(maker) => *maker == state.maker_filter,
        Action::Preset(id) => presets::for_weapon(state.assembly.weapon).any(|preset| {
            preset.id == id
                && preset
                    .assembly(state.assembly.finish)
                    .is_ok_and(|assembly| assembly == state.assembly)
        }),
        Action::ResetBuild => state.assembly.modules.is_empty(),
        Action::ToggleCompare => state.compare_stock,
        Action::Module(slot, id) => state.assembly.selected(*slot).map(|m| m.id) == id.as_deref(),
        Action::Finish(finish) => *finish == state.assembly.finish,
        Action::ToggleExploded => state.exploded,
        Action::ToggleRotate => state.auto_rotate,
        Action::ToggleClay => state.clay,
        _ => false,
    }
}

fn handle_buttons(
    mut actions: ResMut<StudioActions>,
    buttons: Query<(&Interaction, &ButtonAction), Changed<Interaction>>,
) {
    for (interaction, action) in &buttons {
        if *interaction == Interaction::Pressed {
            actions.0.push(action.0.clone());
        }
    }
}

fn sync(
    mut commands: Commands,
    state: Res<Workbench>,
    mut revision: Local<Option<u64>>,
    build: Query<Entity, With<BuildContents>>,
    mut texts: Query<(
        &mut Text,
        Option<&HeaderLabel>,
        Option<&StatusLabel>,
        Option<&GeometryLabel>,
    )>,
    mut buttons: Query<(
        &Interaction,
        &ButtonAction,
        &mut BackgroundColor,
        &mut BorderColor,
    )>,
) {
    if revision.is_none() {
        *revision = Some(state.revision);
    } else if *revision != Some(state.revision) {
        if let Ok(parent) = build.single() {
            commands.entity(parent).despawn_children();
            populate_build(&mut commands, parent, &state);
        }
        *revision = Some(state.revision);
    }
    for (mut text, header, status, geometry) in &mut texts {
        let value = if header.is_some() {
            format!(
                "{}  /  {}",
                state.assembly.weapon.maker().label(),
                state.assembly.weapon.label()
            )
        } else if status.is_some() {
            state.status.clone()
        } else if geometry.is_some() {
            format!(
                "{}   {} pieces / {} triangles",
                if state.compare_stock {
                    "STOCK REFERENCE"
                } else {
                    "ASSEMBLY"
                },
                state.mesh_count,
                state.triangle_count
            )
        } else {
            continue;
        };
        if **text != value {
            **text = value;
        }
    }
    for (interaction, action, mut color, mut border) in &mut buttons {
        let selected = active(&action.0, &state);
        let next = if *interaction == Interaction::Pressed || selected {
            ACTIVE
        } else if *interaction == Interaction::Hovered {
            HOVER
        } else {
            BUTTON
        };
        if color.0 != next {
            color.0 = next;
        }
        let next_border = BorderColor::all(if selected { AMBER } else { LINE });
        if *border != next_border {
            *border = next_border;
        }
    }
}

fn reset_scroll(
    state: Res<Workbench>,
    mut previous: Local<Option<(Option<Maker>, Weapon)>>,
    mut panels: Query<(&ScrollPanel, &mut ScrollPosition)>,
) {
    let current = (state.maker_filter, state.assembly.weapon);
    if let Some(previous) = *previous {
        for (panel, mut position) in &mut panels {
            let changed = match panel {
                ScrollPanel::Catalogue => previous.0 != current.0,
                ScrollPanel::Build => previous.1 != current.1,
            };
            if changed {
                position.0.y = 0.0;
            }
        }
    }
    *previous = Some(current);
}

fn scroll_panel(
    mut wheel: MessageReader<MouseWheel>,
    windows: Query<&Window, With<PrimaryWindow>>,
    mut panels: Query<(&ScrollPanel, &mut ScrollPosition, &ComputedNode)>,
) {
    let Ok(window) = windows.single() else {
        wheel.clear();
        return;
    };
    let Some(pointer) = window.cursor_position() else {
        wheel.clear();
        return;
    };
    for event in wheel.read() {
        let scale = if event.unit == MouseScrollUnit::Line {
            28.0
        } else {
            1.0
        };
        for (panel, mut position, computed) in &mut panels {
            let over = pointer.y >= HEADER_HEIGHT
                && match panel {
                    ScrollPanel::Catalogue => pointer.x < LEFT_WIDTH,
                    ScrollPanel::Build => {
                        pointer.x >= window.width() - RIGHT_WIDTH
                            && pointer.y < window.height() - 88.0
                    }
                };
            if !over {
                continue;
            }
            let maximum = ((computed.content_size().y - computed.size().y)
                * computed.inverse_scale_factor())
            .max(0.0);
            position.0.y = (position.0.y - event.y * scale).clamp(0.0, maximum);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pressed_native_button_queues_its_action_once() {
        let mut app = App::new();
        app.insert_resource(StudioActions(Vec::new()));
        app.add_systems(Update, handle_buttons);
        let entity = app
            .world_mut()
            .spawn((Button, ButtonAction(Action::Save), Interaction::Pressed))
            .id();
        app.update();
        assert!(matches!(
            app.world().resource::<StudioActions>().0.as_slice(),
            [Action::Save]
        ));
        app.update();
        assert_eq!(app.world().resource::<StudioActions>().0.len(), 1);
        app.world_mut().entity_mut(entity).insert(Interaction::None);
        app.update();
        app.world_mut()
            .entity_mut(entity)
            .insert(Interaction::Pressed);
        app.update();
        assert_eq!(app.world().resource::<StudioActions>().0.len(), 2);
    }

    #[test]
    fn chassis_change_replaces_compatible_options_without_rebuilding_each_frame() {
        let mut app = App::new();
        app.insert_resource(Workbench {
            assembly: veyra_weapons::Assembly::default(),
            maker_filter: None,
            compare_stock: false,
            exploded: false,
            auto_rotate: false,
            clay: false,
            status: String::new(),
            mesh_count: 0,
            triangle_count: 0,
            revision: 0,
        });
        app.add_systems(Startup, setup);
        app.add_systems(Update, sync);
        app.update();
        let before: Vec<Entity> = {
            let world = app.world_mut();
            let mut query = world.query_filtered::<Entity, With<ButtonAction>>();
            query.iter(world).collect()
        };
        app.update();
        assert!(
            before
                .iter()
                .all(|entity| app.world().get_entity(*entity).is_ok())
        );
        {
            let mut state = app.world_mut().resource_mut::<Workbench>();
            state.assembly.weapon = Weapon::FieldBlade;
            state.assembly.reset_modules();
            state.revision += 1;
        }
        app.update();
        let world = app.world_mut();
        let mut query = world.query::<&ButtonAction>();
        let options: Vec<_> = query
            .iter(world)
            .filter_map(|button| {
                if let Action::Module(slot, id) = &button.0 {
                    Some((*slot, id.clone()))
                } else {
                    None
                }
            })
            .collect();
        assert!(!options.is_empty());
        for (slot, id) in options {
            assert!(Weapon::FieldBlade.slots().contains(&slot));
            if let Some(id) = id {
                assert!(
                    modules(Weapon::FieldBlade, slot)
                        .iter()
                        .any(|module| module.id == id)
                );
            }
        }
    }

    #[test]
    fn maker_filter_preserves_fittings_and_presets_match_the_whole_build() {
        let mut app = App::new();
        app.insert_resource(Workbench {
            assembly: veyra_weapons::Assembly {
                weapon: Weapon::PatrolCarbine,
                ..default()
            },
            maker_filter: None,
            compare_stock: false,
            exploded: false,
            auto_rotate: false,
            clay: false,
            status: String::new(),
            mesh_count: 0,
            triangle_count: 0,
            revision: 0,
        });
        app.add_systems(Startup, setup);
        app.add_systems(Update, (sync, sync_catalogue).chain());
        app.update();
        let fitting_entities: Vec<Entity> = {
            let world = app.world_mut();
            let mut query = world.query::<(Entity, &ButtonAction)>();
            query
                .iter(world)
                .filter_map(|(entity, action)| {
                    matches!(action.0, Action::Module(..)).then_some(entity)
                })
                .collect()
        };
        app.world_mut().resource_mut::<Workbench>().maker_filter = Some(Maker::Vigil);
        app.update();
        assert!(
            fitting_entities
                .iter()
                .all(|entity| app.world().get_entity(*entity).is_ok())
        );
        let weapons: Vec<Weapon> = {
            let world = app.world_mut();
            let mut query = world.query::<&ButtonAction>();
            query
                .iter(world)
                .filter_map(|action| match action.0 {
                    Action::Weapon(weapon) => Some(weapon),
                    _ => None,
                })
                .collect()
        };
        assert_eq!(weapons.len(), 4);
        assert!(weapons.iter().all(|weapon| weapon.military()));
        let preset = presets::for_weapon(Weapon::PatrolCarbine)
            .next()
            .expect("patrol presets exist");
        let action = Action::Preset(preset.id.to_owned());
        {
            let mut state = app.world_mut().resource_mut::<Workbench>();
            assert!(!active(&action, &state));
            state.assembly = preset.assembly(Finish::Field).expect("valid preset");
            state.revision += 1;
            assert!(active(&action, &state));
            state.assembly.modules.pop_first();
            assert!(!active(&action, &state));
            state.maker_filter = None;
        }
        app.update();
        let world = app.world_mut();
        let mut query = world.query::<&ButtonAction>();
        assert_eq!(
            query
                .iter(world)
                .filter(|action| matches!(action.0, Action::Weapon(_)))
                .count(),
            Weapon::ALL.len()
        );
    }
}
