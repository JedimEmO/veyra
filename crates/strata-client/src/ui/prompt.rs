//! The interaction prompt: a small glass chip near the bottom centre that
//! names what F does beside the prop the sim says is within reach.
//!
//! A HUD layer rather than a line in the combat text, so it sits where
//! the eye rests, hides with the screens like the reticle, and a mod can
//! move it. The text and the visibility follow [`Reach`], which the sim
//! writes only on a change, so this costs nothing while nothing is near.

use bevy::ecs::message::MessageReader;
use bevy::ecs::prelude::*;
use bevy::math::Vec2;
use bevy::ui::widget::Text;
use slotted::theme::Role;
use slotted::ui::def::{
    Align, Justify, Layout, LayoutDirection, Length, LocKey, Padding, Tags, TextAlign, TextOpts,
    TextRole, UiNodeDef,
};
use slotted::ui::{HudAnchor, HudLayerDef, HudLayerId, HudLayers, NineAnchor};
use strata_registry::progression::SkillGraph;
use strata_sim::Registries;
use strata_sim::harvest::Harvesters;
use strata_sim::progression::{InteractionKind, Reach};

/// The layer id.
pub const LAYER: &str = "strata:prompt";

/// The `test_id` of the text node the update writes.
pub const TEXT_ID: &str = "prompt.text";

/// The layer id.
#[must_use]
pub fn layer() -> HudLayerId {
    HudLayerId::new_static(LAYER)
}

/// The chip: the key as an accent pill, the verb beside it.
#[must_use]
pub fn layer_def() -> HudLayerDef {
    HudLayerDef {
        id: layer(),
        anchor: HudAnchor {
            anchor: NineAnchor::Bottom,
            offset: Vec2::new(0.0, -150.0),
            scale: 1.0,
        },
        tree: UiNodeDef::Panel {
            role: Role::new("tooltip"),
            layout: Layout {
                direction: LayoutDirection::Row,
                gap: 1.5,
                padding: Padding {
                    top: 1.0,
                    right: 2.5,
                    bottom: 1.0,
                    left: 1.5,
                },
                align: Some(Align::Center),
                ..Layout::default()
            },
            children: vec![
                UiNodeDef::Panel {
                    role: Role::new("hint.glyph"),
                    layout: Layout {
                        min_width: Some(Length::Px(26.0)),
                        padding: Padding {
                            top: 0.3,
                            right: 1.2,
                            bottom: 0.3,
                            left: 1.2,
                        },
                        align: Some(Align::Center),
                        justify: Justify::Center,
                        ..Layout::default()
                    },
                    children: vec![UiNodeDef::Text {
                        key: LocKey("F".to_owned()),
                        style: TextRole::Caption,
                        opts: TextOpts {
                            role: Some(Role::new("hint.glyph.text")),
                            align: TextAlign::Center,
                            ..TextOpts::default()
                        },
                        tags: Tags::new(),
                    }],
                    tags: Tags::new(),
                },
                UiNodeDef::Text {
                    key: LocKey("strata.prompt.terminal".to_owned()),
                    style: TextRole::Body,
                    opts: TextOpts {
                        role: Some(Role::new("toast.text")),
                        wrap: false,
                        ..TextOpts::default()
                    },
                    tags: Tags::new().with(Tags::TEST_ID, TEXT_ID),
                },
            ],
            tags: Tags::new().with(Tags::TEST_ID, "prompt"),
        },
        visible: false,
        hide_with_screen: true,
    }
}

/// The chip's words for what is within reach: the sim says what it is
/// and the graph what its tree is called; the English is
/// [`super::strings`]'.
#[must_use]
pub fn words(
    reach: &Reach,
    graph: Option<&SkillGraph>,
    harvesters: Option<&Harvesters>,
) -> Option<String> {
    let (_, kind) = reach.nearest.as_ref()?;
    let english = |key: &str| super::strings::english(key).unwrap_or(key).to_owned();
    Some(match kind {
        InteractionKind::Terminal => english("strata.prompt.terminal"),
        InteractionKind::Trainer { trees } => {
            let text = english("strata.prompt.train");
            match trees.first().and_then(|id| graph.and_then(|g| g.tree(id))) {
                Some(tree) => text.replace("{tree}", &tree.name.to_lowercase()),
                None => text.replace("{tree} ", ""),
            }
        }
        InteractionKind::Locker => english("strata.prompt.locker"),
        InteractionKind::Workbench => english("strata.prompt.workbench"),
        InteractionKind::Fabricator => english("strata.prompt.fabricator"),
        InteractionKind::Board { .. } => english("strata.prompt.board"),
        // The chip of a guard needs the books, so `guard_words` words it.
        InteractionKind::Guard { .. } => String::new(),
        InteractionKind::Cache { .. } => english("strata.prompt.cache"),
        InteractionKind::Harvester { id } => match harvesters.and_then(|all| all.get(*id)) {
            Some(machine) => english(if machine.is_full() {
                "strata.harvester.chip.full"
            } else {
                "strata.harvester.chip"
            })
            .replace("{held}", &super::strings::grouped(machine.held))
            .replace("{hopper}", &super::strings::grouped(machine.hopper)),
            None => english("strata.prompt.harvester"),
        },
    })
}

/// A guard's chip: the lease it sells at the price its faction's stance sets,
/// or the sim's own refusal in the board's words. The price and the refusal
/// are `check_lease`'s, so the chip decides nothing.
#[must_use]
pub fn guard_words(
    reach: &Reach,
    books: &super::claims::Books<'_>,
    purse: &strata_sim::deposits::Purse,
) -> Option<String> {
    let (_, InteractionKind::Guard { faction }) = reach.nearest.as_ref()? else {
        return None;
    };
    let faction = books.map?.faction(*faction)?;
    let rules = books.registries.factions.deeds()?;
    let terms = strata_sim::deeds::Terms::of(
        strata_sim::deeds::stance_of(books.registries, books.deeds, &faction, books.now),
        rules,
    );
    let name = super::claims::possessive(&faction.name);
    let english = |key: &str| super::strings::english(key).unwrap_or(key).to_owned();
    Some(
        match strata_sim::deeds::check_lease(books.registries, true, terms, purse, &faction) {
            Ok(quote) => english("strata.post.lease")
                .replace("{faction}", &name)
                .replace("{price}", &super::strings::grouped(quote.price)),
            Err(refusal) => super::board::lease_words(&refusal),
        },
    )
}

/// `Startup`: the layer, hidden.
pub fn register_layer(mut layers: ResMut<HudLayers>) {
    layers.register(layer_def());
}

/// `Update`: the chip follows the reach. The words are written straight
/// onto the text node whenever it exists and says something else, rather
/// than sent as a `HudUpdate`: the layer is respawned each time a screen
/// closes over it, and an update in flight across that respawn lands on
/// a node that is gone.
#[allow(
    clippy::too_many_arguments,
    reason = "the reach, the data, the books a guard's chip reads and the layer's nodes"
)]
pub fn follow_reach(
    reach: Res<Reach>,
    registries: Option<Res<Registries>>,
    harvesters: Option<Res<Harvesters>>,
    standing: super::claims::Standing,
    purse: Option<Res<strata_sim::deposits::Purse>>,
    mut layers: ResMut<HudLayers>,
    mut wanted: Local<Option<String>>,
    mut texts: Query<(&Tags, &mut Text)>,
) {
    // The chip of a harvester carries what it holds, which moves with time.
    let guard = matches!(&reach.nearest, Some((_, InteractionKind::Guard { .. })));
    if guard || reach.is_changed() || harvesters.as_ref().is_some_and(DetectChanges::is_changed) {
        *wanted = if guard {
            standing
                .books()
                .zip(purse.as_deref())
                .and_then(|(books, purse)| guard_words(&reach, &books, purse))
        } else {
            words(
                &reach,
                registries.as_ref().map(|r| &r.0.progression),
                harvesters.as_deref(),
            )
        };
        layers.set_visible(&layer(), wanted.is_some());
    }
    let Some(text) = wanted.as_ref() else {
        return;
    };
    for (tags, mut node) in &mut texts {
        if tags.get(Tags::TEST_ID) == Some(TEXT_ID) && node.0 != *text {
            node.0.clone_from(text);
        }
    }
}

/// `Update`: a post turning on the member is a short toast naming whose
/// people they are, never why in numbers.
pub fn hear_turns(
    mut turns: MessageReader<strata_sim::encounters::PostTurned>,
    near: Res<strata_sim::encounters::NearEncounters>,
    factions: Option<Res<strata_sim::factions::Factions>>,
    mut commands: Commands,
) {
    for turn in turns.read() {
        let Some(faction) = factions
            .as_ref()
            .and_then(|factions| factions.0.faction(turn.faction))
        else {
            continue;
        };
        let wardens = near.posts.iter().any(|post| {
            post.id == turn.post && post.kind == strata_registry::faction::ClaimKind::Reserve
        });
        let english = |key: &'static str| super::strings::english(key).unwrap_or(key);
        let said = english("strata.post.turned")
            .replace("{faction}", &super::claims::possessive(&faction.name))
            .replace(
                "{people}",
                english(if wardens {
                    "strata.post.wardens"
                } else {
                    "strata.post.guards"
                }),
            );
        super::progress::notice(&mut commands, said);
    }
}

/// Registers the layer and its system. Only called once the UI is wired.
pub(crate) fn register(app: &mut bevy::app::App) {
    app.add_systems(bevy::app::Startup, register_layer)
        .add_systems(bevy::app::Update, (follow_reach, hear_turns));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_words_name_the_prop() {
        let graph = SkillGraph::shipped();
        assert_eq!(words(&Reach::default(), Some(&graph), None), None);
        let reach = Reach {
            nearest: Some((Entity::PLACEHOLDER, InteractionKind::Terminal)),
            trees: std::collections::BTreeSet::default(),
            workbench: false,
            fabricator: false,
            board: None,
            guards: std::collections::BTreeSet::default(),
        };
        assert_eq!(
            words(&reach, Some(&graph), None).as_deref(),
            Some("Use the terminal")
        );
        let reach = Reach {
            nearest: Some((
                Entity::PLACEHOLDER,
                InteractionKind::Trainer {
                    trees: vec!["blade".to_owned()],
                },
            )),
            trees: std::collections::BTreeSet::default(),
            workbench: false,
            fabricator: false,
            board: None,
            guards: std::collections::BTreeSet::default(),
        };
        assert_eq!(
            words(&reach, Some(&graph), None).as_deref(),
            Some("Train with the blade trainer")
        );
        assert_eq!(
            words(&reach, None, None).as_deref(),
            Some("Train with the trainer"),
            "no graph: the id is not guessed at"
        );
        let reach = Reach {
            nearest: Some((Entity::PLACEHOLDER, InteractionKind::Locker)),
            trees: std::collections::BTreeSet::default(),
            workbench: false,
            fabricator: false,
            board: None,
            guards: std::collections::BTreeSet::default(),
        };
        assert_eq!(
            words(&reach, Some(&graph), None).as_deref(),
            Some("Open the locker")
        );
        let reach = Reach {
            nearest: Some((Entity::PLACEHOLDER, InteractionKind::Workbench)),
            trees: std::collections::BTreeSet::default(),
            workbench: true,
            fabricator: false,
            board: None,
            guards: std::collections::BTreeSet::default(),
        };
        assert_eq!(
            words(&reach, Some(&graph), None).as_deref(),
            Some("Use the workbench")
        );
        let reach = Reach {
            nearest: Some((Entity::PLACEHOLDER, InteractionKind::Fabricator)),
            fabricator: true,
            ..Reach::default()
        };
        assert_eq!(
            words(&reach, Some(&graph), None).as_deref(),
            Some("Use the fabricator")
        );
        assert!(layer_def().hide_with_screen);
        assert!(!layer_def().visible);
    }

    #[test]
    fn a_harvesters_chip_says_what_it_holds() {
        use strata_registry::item::Record;
        use strata_sim::deposits::DepositId;
        use strata_sim::harvest::Harvester;

        let record = |item: &str| Record {
            item: item.parse().expect("an id"),
            count: 1,
            batch: 0,
            by: None,
            lines: std::collections::BTreeMap::new(),
            condition: None,
            fitted: std::collections::BTreeMap::new(),
            origin: None,
        };
        let machine = |held: u32| Harvester {
            id: 3,
            deposit: DepositId([0, 0]),
            at: [0., 0.],
            pm: 500,
            item: record("veyra:station.field_harvester"),
            by: String::new(),
            rate: 360,
            hopper: 300,
            lot: record("strata:lot.salt"),
            held,
            carry: 0,
            full: false,
        };
        let reach = Reach {
            nearest: Some((Entity::PLACEHOLDER, InteractionKind::Harvester { id: 3 })),
            ..Reach::default()
        };
        let mut all = Harvesters::default();
        all.list.push(machine(120));
        assert_eq!(
            words(&reach, None, Some(&all)).as_deref(),
            Some("Field harvester \u{b7} 120 of 300")
        );
        all.list[0] = machine(300);
        assert_eq!(
            words(&reach, None, Some(&all)).as_deref(),
            Some("Field harvester \u{b7} 300 of 300, full")
        );
        assert_eq!(
            words(&reach, None, None).as_deref(),
            Some("Read the field harvester")
        );
    }
}
