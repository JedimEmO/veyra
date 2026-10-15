//! The icons the client names itself: the art library's ids that no table
//! carries, and the handles to draw any icon by.
//!
//! An ability's or a status's square is named by its row in
//! `abilities.ron`; everything else the HUD and the screens draw (a weapon
//! class's mark, the empty place, the staggered square) is named here, in
//! one place, so a test can hold every name to the catalogue. A painted
//! square is drawn as it is and only ever dimmed; a symbol is white on clear
//! and takes its colour from a theme role.

use std::collections::{BTreeMap, HashMap};

use bevy::asset::{AssetServer, Assets, Handle};
use bevy::color::{Alpha, Color};
use bevy::ecs::prelude::{Entity, ResMut};
use bevy::ecs::resource::Resource;
use bevy::image::Image;
use bevy::picking::Pickable;
use bevy::ui::widget::{ImageNode, NodeImageMode};
use bevy::ui::{Node, Val};
use slotted::model::Value as Model;
use slotted::registry::Value;
use slotted::theme::{ActiveTheme, Paint, Role, Theme};
use slotted::ui::def::{Tags, UiNodeDef, WidgetKind};
use slotted::ui::screen::{SpawnCtx, Widget, WidgetRegistry};
use strata_registry::IconCatalogue;
use strata_sim::Registries;
use strata_sim::combat::Weapon;

/// The mark of an empty place.
pub const EMPTY_PLACE: &str = "veyra:icon.ui.add";

/// The square of the stagger the sim already has, shown with the statuses.
pub const STAGGERED: &str = "veyra:icon.ability.staggered";

/// The weapon classes, for a test that holds every one to a symbol.
pub const CLASSES: [Weapon; 6] = [
    Weapon::Pistol,
    Weapon::Carbine,
    Weapon::Rifle,
    Weapon::Blade,
    Weapon::Greatblade,
    Weapon::Glaive,
];

/// The symbol of a weapon class. The greatblade and the glaive have no mark
/// of their own in the library and borrow the blade's: the nearest other
/// mark, the heavy tool, reads as a gun at the bar's size.
#[must_use]
pub const fn symbol_of(weapon: Weapon) -> &'static str {
    match weapon {
        Weapon::Pistol => "veyra:icon.weapon.pistol",
        Weapon::Carbine => "veyra:icon.weapon.carbine",
        Weapon::Rifle => "veyra:icon.weapon.rifle",
        Weapon::Blade | Weapon::Greatblade | Weapon::Glaive => "veyra:icon.weapon.blade",
    }
}

/// Every symbol this module names.
#[must_use]
pub fn symbols() -> Vec<&'static str> {
    let mut all: Vec<&'static str> = CLASSES.into_iter().map(symbol_of).collect();
    all.push(EMPTY_PLACE);
    all.sort_unstable();
    all.dedup();
    all
}

/// The file of `id` at `pixels` or more, under the asset root, if the
/// catalogue has it.
#[must_use]
pub fn file_of(catalogue: &IconCatalogue, id: &str, pixels: u32) -> Option<String> {
    catalogue.file(id, pixels).map(str::to_owned)
}

/// The fill of `role` in `theme`: a symbol's tint. A role the theme lacks
/// tints nothing, so the symbol shows white.
#[must_use]
pub fn tint_of(theme: Option<&Theme>, role: &'static str) -> Color {
    theme
        .and_then(|theme| Paint::for_role(theme, &Role::new_static(role)))
        .and_then(|paint| paint.background)
        .unwrap_or(Color::WHITE)
}

/// Icon files loaded once, by their path under the asset root.
#[derive(Resource, Default)]
pub struct IconHandles {
    loaded: HashMap<String, Handle<Image>>,
}

impl IconHandles {
    /// The handle of `file`, loading it on the first ask.
    pub fn handle(&mut self, assets: &AssetServer, file: &str) -> Handle<Image> {
        self.loaded
            .entry(file.to_owned())
            .or_insert_with(|| assets.load(file.to_owned()))
            .clone()
    }
}

/// The widget kind of an icon on a screen's definition.
pub const WIDGET: &str = "strata:icon";

/// The share of its colour a dimmed square keeps, as the bar dims one that
/// cannot be pressed.
const DIMMED: f32 = 0.4;

/// The `custom` node that draws `id` at `size` logical pixels: a painted
/// square as it is, a symbol tinted by `tint`'s fill, either dimmed when
/// `dim`. A screen's definition cannot hold an image node, so this is the
/// one way it names a picture.
#[must_use]
pub fn node(
    id: &str,
    size: f32,
    tint: Option<&str>,
    dim: bool,
    test_id: Option<&str>,
) -> UiNodeDef {
    let mut map = BTreeMap::from([
        ("id".to_owned(), Model::Str(id.to_owned())),
        ("size".to_owned(), Model::Float(f64::from(size))),
        ("dim".to_owned(), Model::Bool(dim)),
    ]);
    if let Some(role) = tint {
        map.insert("tint".to_owned(), Model::Str(role.to_owned()));
    }
    UiNodeDef::Custom {
        kind: WidgetKind::new(WIDGET),
        params: slotted::registry::from_model(&Model::Map(map)),
        children: Vec::new(),
        tags: test_id.map_or_else(Tags::new, |id| Tags::new().with(Tags::TEST_ID, id)),
    }
}

/// `strata:icon`.
#[derive(Debug, Default, Clone, Copy)]
pub struct IconWidget;

impl Widget for IconWidget {
    fn spawn(&self, ctx: &mut SpawnCtx<'_>, params: &Value, _children: &[UiNodeDef]) -> Entity {
        let model = slotted::registry::to_model(params).unwrap_or(Model::Null);
        let text = |key: &str| match &model {
            Model::Map(map) => match map.get(key) {
                Some(Model::Str(s)) => Some(s.clone()),
                _ => None,
            },
            _ => None,
        };
        let size = match &model {
            Model::Map(map) => match map.get("size") {
                #[allow(clippy::cast_possible_truncation, reason = "a few hundred pixels")]
                Some(Model::Float(f)) => *f as f32,
                _ => 0.,
            },
            _ => 0.,
        };
        let dim = matches!(&model, Model::Map(map) if map.get("dim") == Some(&Model::Bool(true)));
        let root = ctx.spawn_node((
            Node {
                width: Val::Px(size),
                height: Val::Px(size),
                flex_shrink: 0.,
                ..Node::default()
            },
            Pickable::IGNORE,
        ));
        let (Some(id), Some(registries)) = (text("id"), ctx.world.get_resource::<Registries>())
        else {
            return root;
        };
        // The 64 export serves up to its own size, the 256 beyond it.
        let file = registries
            .0
            .icons
            .file(
                &id,
                size_pixels(
                    size * ctx
                        .world
                        .get_resource::<bevy::ui::UiScale>()
                        .map_or(1.0, |s| s.0)
                        .max(1.0),
                ),
            )
            .map(str::to_owned);
        let (Some(file), Some(assets)) = (file, ctx.world.get_resource::<AssetServer>()) else {
            return root;
        };
        let handle = assets.load(file);
        let mut color = text("tint")
            .and_then(|role| {
                let active = ctx.world.get_resource::<ActiveTheme>()?;
                let themes = ctx.world.get_resource::<Assets<Theme>>()?;
                Paint::for_role(themes.get(&active.0)?, &Role::new(role))?.background
            })
            .unwrap_or(Color::WHITE);
        if dim {
            color = color.with_alpha(DIMMED);
        }
        ctx.world.entity_mut(root).insert(ImageNode {
            image_mode: NodeImageMode::Stretch,
            color,
            ..ImageNode::new(handle)
        });
        root
    }
}

/// The pixels an icon drawn at `size` logical pixels wants.
#[allow(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "a positive size of a few hundred pixels"
)]
fn size_pixels(size: f32) -> u32 {
    size.max(0.).ceil() as u32
}

/// `Startup`: the icon widget, under its kind.
pub(crate) fn register_widget(mut widgets: ResMut<WidgetRegistry>) {
    widgets.register(WidgetKind::new(WIDGET), IconWidget);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn shipped() -> IconCatalogue {
        let text = std::fs::read_to_string(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../assets/icons/catalogue.json"
        ))
        .expect("the catalogue is committed");
        IconCatalogue::parse(&text).expect("it loads")
    }

    #[test]
    fn every_symbol_and_square_the_client_names_is_in_the_catalogue() {
        let catalogue = shipped();
        for symbol in symbols() {
            assert!(catalogue.is_symbol(symbol), "{symbol} is a tintable symbol");
        }
        assert!(catalogue.is_square(STAGGERED), "the stagger is a square");
        for class in CLASSES {
            assert!(
                catalogue.file(symbol_of(class), 24).is_some(),
                "{} has a file",
                class.name()
            );
        }
    }
}
