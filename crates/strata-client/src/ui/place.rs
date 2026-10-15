//! The place's name: a small glass card at the top of the screen naming the
//! procedural site the player stands on, and what kind of place it is
//! (ticket 0005, step 4).
//!
//! A site's board is baked once for every site of its kind, so it cannot
//! carry a name; the name is read here instead, from the same site list
//! the buildings are stood from (`strata_sim::sites::NearSites`), with
//! nothing replicated. It shows while the player is on the site's laid
//! ground (its pad and the developed skirt round it) and hides with the
//! screens, like the interaction chip.
//!
//! Off a site's ground the card names a deposit the player stands at
//! (ticket 0007, step 3): the one the sim says they stand at
//! (within the dials' `place_m` of its centre, and only once the household
//! has found it), its place over its class and type.
//! A site comes first: a deposit is refused on a pad, so the two seldom
//! meet, and where they do the place with a name of its own is the one
//! said.
//!
//! Whose ground it is (ticket 0012, step 1) is read from the sim's
//! `HeldHere`: a held site gains a line, "Held by" its faction, and walking
//! onto another holder's ground, or off every holder's, shows the new
//! holder for [`NOTICE_S`] seconds when no site or deposit is named.

use bevy::ecs::prelude::*;
use bevy::math::Vec2;
use bevy::time::Time;
use bevy::ui::Node;
use bevy::ui::widget::Text;
use slotted::theme::Role;
use slotted::ui::def::{
    Align, Layout, LayoutDirection, LocKey, Padding, Tags, TextAlign, TextOpts, TextRole, UiNodeDef,
};
use slotted::ui::{HudAnchor, HudLayerDef, HudLayerId, HudLayers, NineAnchor};
use strata_core::WorldPos;
use strata_registry::{FactionSet, ResourceSet};
use strata_sim::deposits::{Deposit, NearDeposits, Surveyed};
use strata_sim::factions::{FactionId, HeldHere};
use strata_sim::sites::{NearSite, NearSites};
use strata_sim::{LocalPlayer, Observers, Registries, WorldPosition};

/// The layer id.
pub const LAYER: &str = "strata:place";

/// The `test_id` of the name's text node.
pub const NAME_ID: &str = "place.name";

/// The `test_id` of the kind's text node.
pub const KIND_ID: &str = "place.kind";

/// The `test_id` of the holder's text node, empty for an unheld place.
pub const HELD_ID: &str = "place.held";

/// How long the card names new ground after the member walks onto it,
/// seconds.
pub const NOTICE_S: f64 = 4.0;

/// The layer id.
#[must_use]
pub fn layer() -> HudLayerId {
    HudLayerId::new_static(LAYER)
}

/// The card: the name over its kind, centred.
#[must_use]
pub fn layer_def() -> HudLayerDef {
    let text = |key: &str, style: TextRole, role: &str, id: &str| UiNodeDef::Text {
        key: LocKey(key.to_owned()),
        style,
        opts: TextOpts {
            role: Some(Role::new(role)),
            align: TextAlign::Center,
            wrap: false,
            ..TextOpts::default()
        },
        tags: Tags::new().with(Tags::TEST_ID, id),
    };
    HudLayerDef {
        id: layer(),
        anchor: HudAnchor {
            anchor: NineAnchor::Top,
            offset: Vec2::new(0.0, 70.0),
            scale: 1.0,
        },
        tree: UiNodeDef::Panel {
            role: Role::new("tooltip"),
            layout: Layout {
                direction: LayoutDirection::Column,
                gap: 0.3,
                padding: Padding {
                    top: 1.0,
                    right: 3.0,
                    bottom: 1.0,
                    left: 3.0,
                },
                align: Some(Align::Center),
                ..Layout::default()
            },
            children: vec![
                text(
                    "strata.place.unnamed",
                    TextRole::Heading,
                    "toast.text",
                    NAME_ID,
                ),
                text(
                    "strata.place.kind",
                    TextRole::Caption,
                    "text.muted",
                    KIND_ID,
                ),
                text(
                    "strata.place.held",
                    TextRole::Caption,
                    "text.muted",
                    HELD_ID,
                ),
            ],
            tags: Tags::new().with(Tags::TEST_ID, "place"),
        },
        visible: false,
        hide_with_screen: true,
    }
}

/// The site whose laid ground `at` stands on: within its pad's radius and
/// the developed skirt past it.
#[must_use]
pub fn here(near: &NearSites, at: WorldPos) -> Option<&NearSite> {
    strata_sim::factions::site_at(near, at.x(), at.z())
}

/// The card's words for a site: its name, and its kind in the string
/// table's words, or the registry's when the table has none.
#[must_use]
pub fn words(near: &NearSite) -> (String, String) {
    let key = format!("strata.place.kind.{}", near.kind);
    let kind = super::strings::english(&key).map_or_else(|| near.kind_name.clone(), str::to_owned);
    (near.site.name.clone(), kind)
}

/// The card's third line for a site: "Held by" its faction, empty when
/// `held` was not made for this site or no faction holds its kind.
#[must_use]
pub fn held_words(near: &NearSite, held: &HeldHere) -> String {
    if held.site != Some(near.site.id) {
        return String::new();
    }
    held.site_holder
        .as_ref()
        .and_then(|faction| {
            super::strings::english("strata.place.held.by")
                .map(|line| line.replace("{faction}", &faction.name))
        })
        .unwrap_or_default()
}

/// Whether `site` is a camp cleared and still quiet, by the sim's one rule.
fn is_quiet(
    site: &NearSite,
    registries: &Registries,
    cleared: &strata_sim::encounters::Cleared,
    clock: strata_sim::encounters::PlayedClock,
) -> bool {
    strata_sim::encounters::is_cleared(
        &registries.0.encounters,
        cleared,
        clock.seconds,
        &site.kind,
        site.site.id.key(),
    )
}

/// The card's words for open ground: the holder over the kind of ground it
/// is, `Contested ground` where another is as strong, and `Unclaimed wilds`
/// alone where no holding reaches. `None` before the first reading.
#[must_use]
pub fn ground_words(held: &HeldHere, set: &FactionSet) -> Option<(String, String)> {
    let territory = held.territory?;
    let english = |key: &str| super::strings::english(key).unwrap_or_default();
    let Some(faction) = held.holder.as_ref() else {
        return Some((english("strata.place.unclaimed").to_owned(), String::new()));
    };
    let kind = if territory.contested {
        english("strata.place.contested").to_owned()
    } else {
        // A kind may word its ground whole ("Outlaw ground", not "Outlaw band ground").
        if let Some(whole) =
            super::strings::english(&format!("strata.place.ground.{}", faction.kind))
        {
            whole.to_owned()
        } else {
            let word = super::strings::english(&format!("strata.place.faction.{}", faction.kind))
                .map(str::to_owned)
                .or_else(|| set.kind(&faction.kind).map(|def| def.name.clone()))
                .unwrap_or_else(|| faction.kind.to_string());
            english("strata.place.ground").replace("{kind}", &word)
        }
    };
    // A name may open with "the"; a title line wants its capital.
    let mut name = faction.name.clone();
    if let Some(first) = name.get_mut(..1) {
        first.make_ascii_uppercase();
    }
    Some((name, kind))
}

/// The notice of new ground being shown, and what it last saw.
#[derive(Debug, Default)]
pub struct Notice {
    /// The holder of the last reading, once there is one.
    #[allow(
        clippy::option_option,
        reason = "never read, none yet, or read and held by no one"
    )]
    seen: Option<Option<FactionId>>,
    /// Whether the last reading was contested: a swing between two holders
    /// along a border is one piece of news, not a new notice each refresh.
    contested: bool,
    /// When the notice ends, seconds of the clock.
    until: f64,
    /// What it says: the holder, the kind of ground, and how the holder is
    /// towards the household.
    words: Option<(String, String, String)>,
}

/// The card's words for a deposit: its place, and its class with its type
/// (`Reactive · evaporite salt deposit`). `None` for a deposit of a type
/// the data set has not got.
#[must_use]
pub fn deposit_words(deposit: &Deposit, set: &ResourceSet) -> Option<(String, String)> {
    let kind = set.get(deposit.ty)?;
    Some((
        deposit.place.clone(),
        super::strings::english("strata.place.deposit")?
            .replace("{class}", &set.class_of(deposit.ty).name)
            .replace("{type}", &kind.name.to_lowercase()),
    ))
}

/// The deposit `from` stands at, of those the household has found: a ground
/// it has not pinged is not named, whatever lies under it.
#[must_use]
pub fn standing_at_found<'a>(
    near: &'a NearDeposits,
    set: &ResourceSet,
    surveyed: &Surveyed,
    from: [f64; 2],
) -> Option<&'a Deposit> {
    let within = set.rules().place.place_m;
    near.deposits
        .iter()
        .filter(|deposit| surveyed.found.contains_key(&deposit.id))
        .map(|deposit| (deposit.distance_m(from[0], from[1]), deposit))
        .filter(|(distance, _)| *distance <= within)
        .min_by(|a, b| a.0.total_cmp(&b.0).then_with(|| a.1.id.cmp(&b.1.id)))
        .map(|(_, deposit)| deposit)
}

/// `Startup`: the layer, hidden.
pub fn register_layer(mut layers: ResMut<HudLayers>) {
    layers.register(layer_def());
}

/// How far the player walks before the deposits near are gone through
/// again for the one stood at, metres. The list is every deposit of the
/// register's range, hundreds of them, and a card that comes up a step
/// late is not seen to.
const RESTAND_M: f64 = 1.0;

/// The deposit last found stood at, and where from.
#[derive(Debug, Default)]
pub struct Stood {
    from: Option<[f64; 2]>,
    words: Option<(String, String, String)>,
}

/// `Update`: the card follows the player onto and off a site's ground, and
/// up to and away from a deposit's survey post. The words are written
/// straight onto the text nodes, as the prompt's are, so a layer respawned
/// under a closing screen gets them too.
#[allow(
    clippy::too_many_arguments,
    clippy::too_many_lines,
    reason = "the sites, the deposits and their data, the player, the layer and its words"
)]
pub fn follow_place(
    near: Option<Res<NearSites>>,
    deposits: Option<Res<NearDeposits>>,
    surveyed: Option<Res<Surveyed>>,
    registries: Option<Res<Registries>>,
    observers: Res<Observers>,
    players: Query<&WorldPosition, With<LocalPlayer>>,
    mut layers: ResMut<HudLayers>,
    held: Option<Res<HeldHere>>,
    (cleared, clock, caches): (
        Option<Res<strata_sim::encounters::Cleared>>,
        Option<Res<strata_sim::encounters::PlayedClock>>,
        Option<Res<strata_sim::loot::Caches>>,
    ),
    controllers: Query<&strata_physics::character::CharacterController, With<LocalPlayer>>,
    mut landed: Local<bool>,
    (time, standing): (Res<Time>, super::claims::Standing),
    mut wanted: Local<Option<(String, String, String)>>,
    mut stood: Local<Stood>,
    mut notice: Local<Notice>,
    mut texts: Query<(&Tags, &mut Text, Option<&mut Node>)>,
) {
    let at = players.iter().next().map(|p| p.0).or(observers.primary);
    let site = near
        .as_deref()
        .zip(at)
        .and_then(|(near, at)| here(near, at))
        .map(|site| {
            let (name, mut kind) = words(site);
            let quiet = registries
                .as_deref()
                .zip(cleared.as_deref().zip(clock.as_deref()))
                .is_some_and(|(registries, (cleared, clock))| {
                    is_quiet(site, registries, cleared, *clock)
                });
            if quiet && let Some(line) = super::strings::english("strata.place.cleared") {
                kind = line.replace("{kind}", &kind);
                let holds_one = caches
                    .as_deref()
                    .is_some_and(|caches| caches.by.contains_key(&site.site.id.key()));
                if holds_one && let Some(line) = super::strings::english("strata.place.cache") {
                    kind = line.replace("{kind}", &kind);
                }
            }
            let line = held.as_deref().map(|held| held_words(site, held));
            (name, kind, line.unwrap_or_default())
        });
    let now = site.or_else(|| {
        let set = &registries.as_ref()?.0.resources;
        let list = deposits.as_ref()?;
        let from = at.map(|at| [at.x(), at.z()])?;
        let moved = stood.from.is_none_or(|[x, z]| {
            let (dx, dz) = (from[0] - x, from[1] - z);
            dx * dx + dz * dz >= RESTAND_M * RESTAND_M
        });
        let learned = surveyed.as_ref().is_some_and(Res::is_changed);
        if moved || list.is_changed() || learned || standing.leases_changed() {
            stood.from = Some(from);
            stood.words = surveyed.as_ref().and_then(|surveyed| {
                let deposit = standing_at_found(list, set, surveyed, from)?;
                let (name, kind) = deposit_words(deposit, set)?;
                // Whose it is, on the line a site keeps for its holder.
                let claim = standing
                    .books()
                    .and_then(|books| books.claim_words(deposit))
                    .unwrap_or_default();
                Some((name, kind, claim))
            });
        }
        stood.words.clone()
    });
    // Walking onto other ground names it for a while, unless a place with a
    // name of its own is already on the card.
    // The first reading comes in the first frames, while the ground is still
    // streaming and the screen shows none of it: a notice counted from then
    // is over before the world is, so a member who spawns on a band's ground
    // never saw it. It starts once the member first stands on ground or
    // floats, which holds whatever hostiles there are.
    *landed |= controllers
        .iter()
        .any(|controller| controller.grounded || controller.swimming);
    let ready = *landed || controllers.is_empty();
    if let (Some(held), Some(registries)) = (held.as_deref(), registries.as_deref())
        && held.at.is_some()
        && ready
    {
        let holder = held.holder.as_ref().map(|faction| faction.id);
        let contested = held.territory.is_some_and(|t| t.contested);
        let changed = notice.seen != Some(holder) && !(contested && notice.contested);
        notice.contested = contested;
        if changed {
            notice.seen = Some(holder);
            notice.until = time.elapsed_secs_f64() + NOTICE_S;
            let stance = held
                .holder
                .as_ref()
                // An outlaw band has no stance: its camps answer for it.
                .filter(|faction| faction.nature != strata_registry::Nature::Outlaw)
                .zip(standing.books())
                .and_then(|(faction, books)| books.stance_words(faction))
                .unwrap_or_default();
            notice.words =
                ground_words(held, &registries.0.factions).map(|(name, kind)| (name, kind, stance));
        }
    }
    let now = now.or_else(|| {
        if time.elapsed_secs_f64() < notice.until {
            notice.words.clone()
        } else {
            None
        }
    });
    if now != *wanted {
        layers.set_visible(&layer(), now.is_some());
        *wanted = now;
    }
    let Some((name, kind, line)) = wanted.as_ref() else {
        return;
    };
    for (tags, mut node, shape) in &mut texts {
        let text = match tags.get(Tags::TEST_ID) {
            Some(NAME_ID) => name,
            Some(KIND_ID) => kind,
            Some(HELD_ID) => line,
            _ => continue,
        };
        if node.0 != *text {
            node.0.clone_from(text);
        }
        // An empty line takes no room, so a deposit's card is as it was.
        if tags.get(Tags::TEST_ID) != Some(NAME_ID)
            && let Some(mut shape) = shape
        {
            let display = if text.is_empty() {
                bevy::ui::Display::None
            } else {
                bevy::ui::Display::DEFAULT
            };
            if shape.display != display {
                shape.display = display;
            }
        }
    }
}

/// Registers the layer and its system. Only called once the UI is wired.
pub(crate) fn register(app: &mut bevy::app::App) {
    app.add_systems(bevy::app::Startup, register_layer)
        .add_systems(bevy::app::Update, follow_place);
}

#[cfg(test)]
mod tests {
    use super::*;
    use strata_registry::SiteKindId;
    use strata_sim::deposits::standing_at;
    use strata_sim::sites::{ProceduralSite, SiteId};

    fn near(kind: &str, x: f64) -> NearSite {
        NearSite {
            site: ProceduralSite {
                id: SiteId {
                    kind: SiteKindId(0),
                    mass: [0, 0],
                    node: [0, 0],
                },
                name: "Beth's Ford".to_owned(),
                centre: [x, 0.0],
                level_m: 5.0,
                radius_m: 40.0,
                blend_m: 40.0,
                crown: 0.02,
                facing: [0.0, 1.0],
                arrival: [x, -40.0],
                by: None,
                score: 1.0,
                cut_m: 0.0,
            },
            kind: kind.to_owned(),
            kind_name: "Registry name".to_owned(),
            placement: None,
        }
    }

    #[test]
    fn the_card_names_the_site_underfoot() {
        let list = NearSites {
            sites: vec![near("strata:hamlet", 0.0), near("test:other", 500.0)],
            around: Vec::new(),
        };
        let on = here(&list, WorldPos::new(30.0, 5.0, 25.0)).expect("on the pad's ground");
        assert_eq!(words(on), ("Beth's Ford".to_owned(), "Hamlet".to_owned()));
        // On the skirt still; past it, nowhere.
        assert!(here(&list, WorldPos::new(45.0, 5.0, 0.0)).is_some());
        assert!(here(&list, WorldPos::new(47.0, 5.0, 0.0)).is_none());
        // A kind the table does not know is called what the registry calls it.
        let other = here(&list, WorldPos::new(500.0, 5.0, 0.0)).expect("the other");
        assert_eq!(words(other).1, "Registry name");
        assert!(layer_def().hide_with_screen);
        assert!(!layer_def().visible);
    }

    /// At a deposit the card names its place over its class and type, the
    /// nearer of two, and nothing a few dozen metres from either.
    #[test]
    fn the_card_names_the_deposit_stood_at() {
        use crate::ui::testing::{deposit, shipped};
        let registries = shipped();
        let set = &registries.resources;
        let calder = deposit(
            &registries,
            "strata:evaporite_salt",
            "Calder",
            [3, -2],
            [2_600., -1_300.],
        );
        let harrow = deposit(
            &registries,
            "strata:light_alloy_ore",
            "Harrow",
            [3, -1],
            [2_640., -1_300.],
        );
        let list = NearDeposits {
            deposits: vec![calder.clone(), harrow],
            around: Vec::new(),
        };
        let within = set.rules().place.place_m;
        let at = |x: f64| [x, -1_300.];
        let found = standing_at(&list, set, at(2_600. - within + 1.)).expect("at Calder");
        assert_eq!(found.id, calder.id);
        assert_eq!(
            deposit_words(found, set),
            Some((
                "Calder flats".to_owned(),
                "Reactive · evaporite salt deposit".to_owned()
            ))
        );
        // Between the two, the nearer is the one named.
        let found = standing_at(&list, set, at(2_625.)).expect("nearer Harrow");
        assert_eq!(
            deposit_words(found, set).map(|(_, kind)| kind).as_deref(),
            Some("Structural metal · light alloy ore deposit")
        );
        assert!(standing_at(&list, set, at(2_600. - within - 1.)).is_none());
        assert!(standing_at(&NearDeposits::default(), set, at(2_600.)).is_none());

        // Only a found deposit is named.
        let mut surveyed = Surveyed::default();
        assert!(standing_at_found(&list, set, &surveyed, at(2_600.)).is_none());
        surveyed.found.insert(
            calder.id,
            strata_sim::deposits::Found {
                reading: strata_registry::Reading::Type,
                sampled: false,
                best_pm: 100,
                best_at: calder.at,
            },
        );
        let found = standing_at_found(&list, set, &surveyed, at(2_600.)).expect("found");
        assert_eq!(found.id, calder.id);
    }
}
