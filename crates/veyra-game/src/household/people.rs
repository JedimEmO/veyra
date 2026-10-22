//! The people of the first questline (ticket 0014) and the two places out of
//! town, stood in the world. What a person says, and when, is the sim's
//! (`veyra_sim::quests`) and the data's (`assets/people`); this is where they
//! stand: a body on the neutral team with a name plate and an F that opens
//! their conversation, a mast and a pump for the two things that answer like
//! one, Pell's crew and Wim as encounters that are neutral until a reply turns
//! them (and stay turned once turned), and the household's start on the quay.
//! Every distance is a dial of `assets/quests/places.ron`; a crew's plate is
//! its quest file's.

use std::collections::BTreeMap;

use strata::registry::Namespaced;
use strata::registry::quest::{Effect, Objective, Place, PlaceRules};
use strata::sim::ProceduralWorldRes;
use strata::sim::encounters::{Allegiance, EncounterId, Fights, NEUTRAL_TEAM};
use strata::sim::factions::Factions;
use strata::sim::quests::{OpenDialogue, QuestCast, QuestHostile, QuestLog, QuestPlaces, places};

use super::*;
use crate::hostiles::{Birth, spawn_hostile};

/// What a stand is, once it stands.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Body {
    /// A person with a conversation and nothing to fight with.
    Person,
    /// A person who may turn on the household: Pell, Wim.
    Fighter,
    /// A thing that answers F: the mast, the pump.
    Thing,
    /// One more of a crew, who is nobody in particular.
    Hand,
}

/// One body the questline stands somewhere: who, where, which way, and what
/// it is.
#[derive(Debug, Clone)]
pub struct Stand {
    /// Whose body it is.
    pub person: Namespaced,
    name: String,
    colour: [u8; 3],
    at: [f64; 2],
    yaw: f32,
    body: Body,
    /// The crew it is one of, and the kind it fights as.
    crew: Option<(String, strata::registry::FoeKind)>,
    /// The flag that sends it away.
    gone: Option<String>,
}

/// Everything the questline stands, found once the world is known.
#[derive(Resource, Debug, Clone, Default)]
pub struct Stands(pub Vec<Stand>);

/// Where the household starts when it is the questline's to place: the point,
/// and the heading that faces the first person.
#[derive(Resource, Debug, Clone, Copy)]
pub struct QuestStart {
    /// The point, world `x` and `z`.
    pub at: (f64, f64),
    /// The heading that faces the first person.
    pub yaw: f32,
}

/// A standing body's stand, for the fold.
#[derive(Component, Debug, Clone, PartialEq, Eq)]
pub struct Stood(pub usize);

/// The hash of a quest id, which names its encounter.
fn key(text: &str) -> u64 {
    text.bytes().fold(0xcbf2_9ce4_8422_2325, |hash, byte| {
        (hash ^ u64::from(byte)).wrapping_mul(0x0100_0000_01b3)
    })
}

/// The encounter a person or crew is, by the id the data names it with.
pub fn encounter_of(id: &str) -> EncounterId {
    EncounterId::Quest(key(id))
}

/// A point `along` metres on the settlement's row and `before` metres in
/// front of it, from the bench place's terminal.
pub fn offset(base: (f64, f64), yaw: f32, along: f64, before: f64) -> (f64, f64) {
    let (right, forward) = (PlayerIntent::right(yaw), PlayerIntent::forward(yaw));
    (
        base.0 + f64::from(right.x) * along - f64::from(forward.x) * before,
        base.1 + f64::from(right.z) * along - f64::from(forward.z) * before,
    )
}

/// The quay's water's edge, found from the Exchange's spawn down the
/// settlement's own downhill, then back from it and along the strip by the
/// dials of `places.ron`. `(x, z)` and the heading that looks out to sea.
/// `None` without a settlement with a layout, or a stair that reaches no
/// water within the dial's search.
pub fn quay_point(
    settlements: &strata::sim::Settlements,
    source: &dyn strata::worldgen::WorldSource,
    rules: &PlaceRules,
) -> Option<((f64, f64), f32)> {
    let settlement = settlements.0.first()?;
    let layout = settlement.layout.as_ref()?;
    let [sx, _, sz] = layout.spawn;
    let height = |z: f64| {
        let at = settlement.to_world([sx, 0., z]);
        f64::from(source.sample(at.x(), at.z()).height)
    };
    let sea = f64::from(source.sea_level());
    let mut z = sz;
    while height(z) > sea + rules.shore_m {
        z += 1.0;
        if z > sz + rules.quay_search_m {
            return None;
        }
    }
    let at = settlement.to_world([sx + rules.quay_along_m, 0., z - rules.quay_inset_m]);
    #[allow(
        clippy::cast_possible_truncation,
        reason = "a heading in radians, render-space f32"
    )]
    let yaw = settlement.world_yaw(std::f64::consts::PI) as f32;
    Some(((at.x(), at.z()), yaw))
}

/// Where a new household arrives and where Maren stands to meet it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Arrival {
    /// The household's `(x, z)`.
    pub at: (f64, f64),
    /// Its heading: out to sea on the quay.
    pub yaw: f32,
    /// Maren's `(x, z)`: ahead and to the right, past the household's
    /// shoulder from the camera behind it.
    pub greeter: (f64, f64),
    /// Her heading, toward the household.
    pub greeter_yaw: f32,
}

/// The heading that looks from `from` to `to`.
fn facing(from: (f64, f64), to: (f64, f64)) -> f32 {
    let (dx, dz) = (to.0 - from.0, to.1 - from.1);
    #[allow(
        clippy::cast_possible_truncation,
        reason = "a heading in radians, render-space f32"
    )]
    let yaw = (-dx).atan2(-dz) as f32;
    yaw
}

/// The arrival: on the quay ([`quay_point`]), or at the Exchange beside
/// Teague where the quay is not found, so a world whose stair finds no water
/// still starts the questline. `None` without a settlement with a layout.
pub fn arrival(
    settlements: &strata::sim::Settlements,
    source: &dyn strata::worldgen::WorldSource,
    rules: &PlaceRules,
) -> Option<Arrival> {
    let (at, yaw) = quay_point(settlements, source, rules).or_else(|| {
        let (place, yaw) = city_place(Some(settlements))?;
        Some((
            offset(place, yaw, rules.spread_m, rules.exchange_before_m),
            yaw,
        ))
    })?;
    let greeter = offset(at, yaw, rules.greeter_aside_m, -rules.greeter_ahead_m);
    Some(Arrival {
        at,
        yaw,
        greeter,
        greeter_yaw: facing(greeter, at),
    })
}

/// The stand a person's file names, once the places are known.
fn spot(place: Place, found: &BTreeMap<Place, (f64, f64)>) -> Option<(f64, f64)> {
    found.get(&place).copied()
}

/// The place a still's `--quest-stage` starts near: the last of the stage's
/// objectives that names a person or a place, else the quay.
fn stage_place(registries: &GameRegistries, quest: &str, stage: &str) -> Place {
    let data = &registries.quests;
    let Some(def) = data.quest(quest).and_then(|quest| quest.stage(stage)) else {
        return Place::Quay;
    };
    def.objectives
        .iter()
        .rev()
        .find_map(|objective| match objective {
            Objective::Talk(person) | Objective::Deliver(_, person) => {
                data.person(person).map(|person| person.stand)
            }
            Objective::Reach(place, _) => Some(*place),
            _ => None,
        })
        .unwrap_or(Place::Quay)
}

/// Whether a run is a play and not a still, a test, a headless run or a
/// script: the one start the questline takes over.
fn is_play(cli: &crate::Launch) -> bool {
    !cli.headless
        && cli.at.is_none()
        && cli.spawn.is_none()
        && cli.frames.is_none()
        && !cli.combat_script
        && cli.script_survey.is_none()
        && !cli.script_ride
}

/// The persons and crews of the data whose id a `Hostile` effect names: the
/// ones who may turn on the household.
fn fighters(registries: &GameRegistries) -> Vec<String> {
    fn walk(effects: &[Effect], found: &mut Vec<String>) {
        for effect in effects {
            match effect {
                Effect::Hostile(target) => found.push(target.clone()),
                Effect::If(_, inner) => walk(inner, found),
                _ => {}
            }
        }
    }
    let mut found = Vec::new();
    for node in registries.quests.nodes() {
        for reply in &node.replies {
            walk(&reply.effects, &mut found);
        }
    }
    found.sort();
    found.dedup();
    found
}

/// Find where everything stands and publish it: the cast of the three
/// roles, the quest places, every body's stand and, where the questline
/// places the household, its start. Once the world and its factions exist,
/// before the player spawns.
#[allow(
    clippy::too_many_arguments,
    clippy::too_many_lines,
    reason = "the world, the data and the flags in; the cast, the places, the stands and the start out"
)]
pub fn locate(
    mut commands: Commands,
    cli: Res<crate::Launch>,
    registries: Res<strata::sim::Registries>,
    world: Option<Res<ProceduralWorldRes>>,
    source: Option<Res<WorldSourceRes>>,
    settlements: Option<Res<strata::sim::Settlements>>,
    factions: Option<Res<Factions>>,
    first_day: Option<Res<FirstDay>>,
    household: Option<Res<Household>>,
) {
    let registries = &registries.0;
    let data = &registries.quests;
    if data.is_empty() {
        return;
    }
    let (Some(world), Some(source), Some(settlements)) = (world, source, settlements) else {
        return;
    };
    let Some(city) = settlements
        .0
        .first()
        .map(|settlement| settlement.site.centre)
    else {
        return;
    };
    if let Some(factions) = &factions {
        commands.insert_resource(QuestCast::of(
            &factions.0,
            city,
            data.rules().map_or(0.0, |rules| rules.cast_reach_m),
        ));
    }
    let Some(rules) = data.rules() else {
        return;
    };
    let quay = arrival(&settlements, source.source(), rules);
    let mut found: BTreeMap<Place, (f64, f64)> = BTreeMap::new();
    let mut yaw_of: BTreeMap<Place, f32> = BTreeMap::new();
    if let Some(quay) = quay {
        found.insert(Place::Quay, quay.greeter);
        yaw_of.insert(Place::Quay, quay.greeter_yaw);
    }
    if let Some(((x, z), yaw)) = city_place(Some(&settlements)) {
        let exchange = offset((x, z), yaw, 0.0, rules.exchange_before_m);
        found.insert(Place::Exchange, exchange);
        yaw_of.insert(Place::Exchange, yaw);
        if let Some(((bx, bz), byaw)) = board_view(Some(&settlements)) {
            found.insert(
                Place::Board,
                offset((bx, bz), byaw, rules.board_along_m, 0.0),
            );
            yaw_of.insert(Place::Board, byaw + std::f32::consts::PI);
        }
    }
    let sites = places(&world.0, rules, city);
    if let Some(at) = sites.station {
        found.insert(Place::Station, (at[0], at[1]));
    }
    if let Some(at) = sites.channel {
        found.insert(Place::Channel, (at[0], at[1]));
    }
    if let Some((x, _, z)) = crate::hostiles::default_spawn(&source, Some(&settlements)) {
        found.insert(Place::Training, (x, z));
    }
    commands.insert_resource(QuestPlaces(
        found
            .iter()
            .map(|(place, (x, z))| (*place, [*x, *z]))
            .collect(),
    ));
    info!(
        "quest: places {}",
        found
            .iter()
            .map(|(place, (x, z))| format!("{place:?} ({x:.0}, {z:.0})"))
            .collect::<Vec<_>>()
            .join(", ")
    );

    // The bodies, by place in the order the data lists them so two at one
    // place stand beside each other and never on each other.
    let fighting = fighters(registries);
    let mut stands = Vec::new();
    let mut count: BTreeMap<Place, f64> = BTreeMap::new();
    for person in data.people() {
        let Some((x, z)) = spot(person.stand, &found) else {
            continue;
        };
        let slot = count.entry(person.stand).or_default();
        let beside = *slot * rules.spread_m;
        *slot += 1.0;
        let yaw = yaw_of.get(&person.stand).copied().unwrap_or(person.facing);
        let (x, z) = if matches!(person.stand, Place::Station | Place::Channel) {
            (x + beside, z)
        } else {
            (x, z)
        };
        let leads = data
            .quests()
            .iter()
            .flat_map(|quest| &quest.crews)
            .find(|crew| {
                person.id.to_string() == format!("veyra:person.{}", crew.id)
                    && crew.at == person.stand
            });
        let fights = fighting.contains(&person.id.to_string()) || leads.is_some();
        stands.push(Stand {
            person: person.id.clone(),
            name: person.name.clone(),
            colour: person.tint,
            at: [x, z],
            yaw,
            body: if person.prop {
                Body::Thing
            } else if fights {
                Body::Fighter
            } else {
                Body::Person
            },
            crew: leads.and_then(|crew| Some((crew.id.clone(), crew.roster.first()?.kind))),
            gone: leads.and_then(|crew| crew.gone.clone()),
        });
    }
    for quest in data.quests() {
        for crew in &quest.crews {
            let Some((x, z)) = spot(crew.at, &found) else {
                continue;
            };
            let led = stands
                .iter()
                .any(|stand| stand.crew.as_ref().is_some_and(|(id, _)| *id == crew.id));
            let mut hands: Vec<_> = crew
                .roster
                .iter()
                .flat_map(|foe| std::iter::repeat_n(foe.kind, usize::from(foe.count)))
                .collect();
            if led {
                hands.remove(0);
            }
            #[allow(clippy::cast_precision_loss, reason = "a few crew members")]
            let n = hands.len().max(1) as f64;
            let Ok(crew_id) = Namespaced::parse(&format!("veyra:person.{}", crew.id)) else {
                continue;
            };
            let Some(hand) = crew.hand.as_ref().filter(|_| !hands.is_empty()) else {
                continue;
            };
            for (index, kind) in hands.into_iter().enumerate() {
                #[allow(clippy::cast_precision_loss, reason = "a few crew members")]
                let angle = std::f64::consts::TAU * (index as f64 + 0.5) / n;
                stands.push(Stand {
                    person: crew_id.clone(),
                    name: hand.name.clone(),
                    colour: hand.tint,
                    at: [
                        x + rules.crew_ring_m * angle.cos(),
                        z + rules.crew_ring_m * angle.sin() + rules.spread_m,
                    ],
                    yaw: 0.0,
                    body: Body::Hand,
                    crew: Some((crew.id.clone(), kind)),
                    gone: crew.gone.clone(),
                });
            }
        }
    }
    let stands = Stands(stands);

    // Where the household starts: a first day takes the quay; a still says
    // which stage it wants to see.
    let log = household.as_deref().map(|household| &household.quests);
    let person_place = cli.open_dialogue.as_deref().and_then(|name| {
        let id = format!("veyra:person.{name}").parse().ok()?;
        data.person(&id).map(|person| person.stand)
    });
    let place = if let Some(place) = person_place {
        Some(place)
    } else if let Some((quest, stage)) = &cli.quest_stage {
        Some(stage_place(registries, quest, stage))
    } else if (first_day.is_some() || cli.script_quest.is_some())
        && is_play_or_script(&cli)
        && log.is_none_or(|log| log.quests.is_empty())
    {
        Some(Place::Quay)
    } else {
        None
    };
    if cli.at.is_some() || cli.spawn.is_some() {
        commands.insert_resource(stands);
        return;
    }
    let Some(place) = place else {
        commands.insert_resource(stands);
        return;
    };
    // A still of a conversation starts by the person's own body, which stands
    // a spread off its place when others share it.
    let person_at = cli.open_dialogue.as_deref().and_then(|name| {
        let id: Namespaced = format!("veyra:person.{name}").parse().ok()?;
        standing_at(&stands, &id).map(|[x, z]| (x, z))
    });
    let start = match place {
        Place::Quay => quay.map(|quay| QuestStart {
            at: quay.at,
            yaw: quay.yaw,
        }),
        other => person_at
            .or(found.get(&other).copied())
            .map(|(x, z)| QuestStart {
                // North of the place, looking south at it (yaw 0 looks down
                // -z, so a start on +z looks at the place).
                at: (x, z + rules.still_m),
                yaw: 0.0,
            }),
    };
    if let Some(start) = start {
        commands.insert_resource(start);
    }
    commands.insert_resource(stands);
}

/// Teague's field camp (ticket 0015): once the player has spawned and the
/// places are known, a terminal and a workbench join the props to place back
/// from the mast, once. Not tied to the stage, so the camp stays after the
/// questline as a place to work in the basin.
pub fn stand_camp(
    registries: Res<strata::sim::Registries>,
    source: Option<Res<WorldSourceRes>>,
    places: Option<Res<QuestPlaces>>,
    settlements: Option<Res<strata::sim::Settlements>>,
    sites: Option<ResMut<PropSites>>,
    players: Query<(), With<LocalPlayer>>,
    mut done: Local<bool>,
) {
    if *done || players.is_empty() {
        return;
    }
    let (Some(places), Some(settlements), Some(mut sites), Some(source)) =
        (places, settlements, sites, source)
    else {
        return;
    };
    let Some(rules) = registries.0.quests.rules() else {
        return;
    };
    let (Some(station), Some(city)) = (
        places.0.get(&Place::Station),
        settlements
            .0
            .first()
            .map(|settlement| settlement.site.centre),
    ) else {
        return;
    };
    sites.0.extend(camp_sites(strata::sim::quests::camp(
        source.source(),
        rules,
        *station,
        city,
    )));
    *done = true;
}

/// Until the dialogue screen exists (step 2), a conversation is read from the
/// log: the speaker, the line and the numbered replies with why one is greyed.
pub fn log_dialogue(mut views: MessageReader<strata::sim::quests::DialogueView>) {
    for view in views.read() {
        info!("{}: {}", view.name, view.line);
        for (number, reply) in view.replies.iter().enumerate() {
            let why = reply
                .blocked
                .as_ref()
                .map_or_else(String::new, |why| format!(" (not now: {why:?})"));
            info!("  {}. {}{why}", number + 1, reply.text);
        }
    }
}

/// A stage that asks for a recruit, a switch, a purchase or a stabled bike
/// counts the ones already made: a join is one way and a bike is bought
/// once, so a household that took Ilse on the bench, or rode its own bike,
/// before the questline asked would otherwise never meet the objective.
#[allow(
    clippy::too_many_arguments,
    reason = "the log and the data, and the four things an objective can already be met by"
)]
pub fn catch_up(
    log: Res<QuestLog>,
    household: Res<Household>,
    registries: Res<strata::sim::Registries>,
    stable: Option<Res<strata::sim::ride::Stable>>,
    store: Option<Res<HouseholdStore>>,
    packs: Query<&Pack, With<LocalPlayer>>,
    mut seen: Local<Option<(String, String)>>,
    mut events: MessageWriter<strata::sim::quests::QuestEvent>,
) {
    use strata::sim::quests::QuestEvent;
    let mut now = None;
    for quest in registries.0.quests.quests() {
        if let Some(stage) = log.stage_of(&quest.id) {
            now = Some((quest.id.clone(), stage.to_owned()));
        }
    }
    if *seen == now {
        return;
    }
    seen.clone_from(&now);
    let Some((quest, stage)) = now else {
        return;
    };
    let Some(def) = registries
        .0
        .quests
        .quests()
        .iter()
        .find(|def| def.id == quest)
        .and_then(|def| def.stage(&stage))
    else {
        return;
    };
    for objective in &def.objectives {
        match objective {
            Objective::Recruit(name) if household.characters.iter().any(|c| &c.name == name) => {
                events.write(QuestEvent::Recruited(name.clone()));
            }
            Objective::Switch(name) if &household.active().name == name => {
                events.write(QuestEvent::Switched(name.clone()));
            }
            Objective::Buy(item)
                if stable
                    .as_ref()
                    .and_then(|stable| stable.bike.as_ref())
                    .is_some_and(|bike| bike.item == *item)
                    || owns(&registries.0, packs.single().ok(), store.as_deref(), item) =>
            {
                events.write(QuestEvent::Bought(item.clone()));
            }
            Objective::Stable if stable.as_ref().is_some_and(|stable| stable.bike.is_some()) => {
                events.write(QuestEvent::Stabled);
            }
            _ => {}
        }
    }
}

/// Whether a record of `item` is in the pack, its bags or the store.
fn owns(
    registries: &GameRegistries,
    pack: Option<&Pack>,
    store: Option<&HouseholdStore>,
    item: &Namespaced,
) -> bool {
    let carried = pack
        .into_iter()
        .flat_map(|pack| std::iter::once(&pack.cells).chain(pack.bags.iter()));
    carried
        .chain(store.map(|store| &store.0))
        .flat_map(|inventory| inventory.slots().iter().flatten())
        .filter_map(|stack| strata::sim::items::read(stack, &registries.items).ok())
        .any(|record| record.item == *item)
}

/// A play, or the questline's own script.
fn is_play_or_script(cli: &crate::Launch) -> bool {
    is_play(cli) || (cli.script_quest.is_some() && cli.spawn.is_none() && cli.at.is_none())
}

/// Stand and fold the bodies by how near the household is. A person stands
/// once the ground under them has streamed; a crew stands until its flag
/// sends it away. Bodies of the neutral team, with the plate the actor's name
/// gives them and an F that opens their conversation.
#[allow(
    clippy::too_many_arguments,
    clippy::type_complexity,
    reason = "the stands, the log and the world's ground; what stands and what is placed"
)]
pub fn stand_people(
    mut commands: Commands,
    stands: Option<Res<Stands>>,
    log: Option<Res<QuestLog>>,
    open: Option<Res<OpenDialogue>>,
    registries: Res<strata::sim::Registries>,
    mut fights: ResMut<Fights>,
    source: Res<WorldSourceRes>,
    origin: Res<RenderOrigin>,
    spatial: SpatialQuery,
    players: Query<&WorldPosition, With<LocalPlayer>>,
    standing: Query<(Entity, &Stood)>,
    actors: Query<Entity, With<Combatant>>,
) {
    let (Some(stands), Some(log)) = (stands, log) else {
        return;
    };
    let Ok(player) = players.single() else {
        return;
    };
    let Some(rules) = registries.0.quests.rules() else {
        return;
    };
    let here = [player.0.x(), player.0.z()];
    let near = |at: [f64; 2]| (at[0] - here[0]).hypot(at[1] - here[1]);
    // Whoever is being spoken to stays, so a reply that sends the crew away
    // never strands its closing line.
    let speaking = open
        .as_deref()
        .and_then(|open| open.0.as_ref())
        .map(|(who, _)| who.to_string());
    let wanted = |stand: &Stand| {
        stand.gone.as_ref().is_none_or(|flag| !log.flag(flag))
            || speaking.as_deref() == Some(stand.person.as_ref())
    };
    for (entity, stood) in &standing {
        let Some(stand) = stands.0.get(stood.0) else {
            continue;
        };
        if !wanted(stand) || near(stand.at) > rules.fold_m {
            if let Some(id) = fight_id(stand) {
                fights.forget_post(id);
            }
            commands.entity(entity).despawn();
        }
    }
    let filter = SpatialQueryFilter::from_excluded_entities(actors.iter());
    for (index, stand) in stands.0.iter().enumerate() {
        if standing.iter().any(|(_, stood)| stood.0 == index)
            || !wanted(stand)
            || near(stand.at) > rules.stand_m
        {
            continue;
        }
        let Some(ground) = ground_at(
            stand.at[0],
            stand.at[1],
            &source,
            &origin,
            &spatial,
            &filter,
        ) else {
            continue;
        };
        // A crew or a person a reply turned stands turned: the log keeps
        // the crossing, the post forgets it when it folds.
        let crossed = is_crossed(stand, &log);
        stand_one(
            &mut commands,
            index,
            stand,
            ground,
            &registries.0,
            &mut fights,
            crossed,
        );
    }
}

/// Whether the log keeps this body's crew or person as turned on the
/// household.
fn is_crossed(stand: &Stand, log: &QuestLog) -> bool {
    fight_id(stand).is_some_and(|id| log.hostile.iter().any(|target| encounter_of(target) == id))
}

/// The encounter a fighting body is of, by its crew or its own id.
fn fight_id(stand: &Stand) -> Option<EncounterId> {
    match stand.body {
        Body::Fighter | Body::Hand => Some(encounter_of(
            stand
                .crew
                .as_ref()
                .map_or_else(|| stand.person.to_string(), |(crew, _)| crew.clone())
                .as_str(),
        )),
        Body::Person | Body::Thing => None,
    }
}

fn stand_one(
    commands: &mut Commands,
    index: usize,
    stand: &Stand,
    ground: f64,
    registries: &GameRegistries,
    fights: &mut Fights,
    crossed: bool,
) {
    let [x, z] = stand.at;
    let interaction = Interaction {
        kind: InteractionKind::Person {
            id: stand.person.clone(),
        },
        radius: REACH_M,
    };
    if stand.body == Body::Thing {
        // A thing is a prop with the person's name: a mast, a pump.
        let prop = if stand.person.to_string().ends_with("pump") {
            Prop::Pump
        } else {
            Prop::Mast
        };
        commands.spawn((
            Name::new(stand.name.clone()),
            prop,
            Stood(index),
            interaction,
            WorldPosition(WorldPos::new(x, ground, z)),
            Transform::from_rotation(Quat::from_rotation_y(stand.yaw)),
        ));
        info!("quest: {} stands at ({x:.0}, {z:.0})", stand.name);
        return;
    }
    let at = WorldPos::new(x, ground + crate::PLAYER_DROP + 0.9, z);
    let colour = strata::sim::view::BodyColour(stand.colour);
    let mut body = if let Some(id) = fight_id(stand) {
        // A fighter has the brain of its kind and the neutral team until a
        // reply turns it: the post's rule, which `Fights` keeps.
        let kind = stand
            .crew
            .as_ref()
            .map_or(strata::sim::combat_ai::OpponentKind::Rusher, |(_, kind)| {
                (*kind).into()
            });
        fights.stand_post(id);
        if crossed {
            fights.cross(id);
        }
        let birth = Birth {
            passive: false,
            kind,
            tier: 0,
            id,
            allegiance: Allegiance::Outlaw(None),
        };
        let mut body = spawn_hostile(commands, at, birth, registries);
        body.entry::<Combatant>()
            .and_modify(|mut actor| actor.team = NEUTRAL_TEAM);
        body.insert(strata::sim::quests::QuestFoe {
            kind: strata::sim::quests::foe_kind(kind),
            crew: stand.crew.as_ref().map(|(crew, _)| crew.clone()),
        });
        body
    } else {
        let health = strata::sim::combat_tables::current()
            .opponents
            .passive_health;
        commands.spawn((
            Combatant::on_team(NEUTRAL_TEAM, health),
            CharacterController {
                height: crate::PLAYER_HEIGHT_M,
                responsive: true,
                ..default()
            },
            WorldPosition(at),
            PreviousWorldPosition::default(),
            PlayerCommand::default(),
        ))
    };
    body.insert((
        Name::new(stand.name.clone()),
        Stood(index),
        colour,
        PlayerIntent {
            yaw: stand.yaw,
            ..default()
        },
    ));
    if stand.body != Body::Hand {
        body.insert(interaction);
    }
    info!("quest: {} stands at ({x:.0}, {z:.0})", stand.name);
}

/// A reply turned a crew or a person on the household: their post is
/// crossed, so they fight as a camp does.
pub fn turn_hostile(mut heard: MessageReader<QuestHostile>, mut fights: ResMut<Fights>) {
    for QuestHostile(target) in heard.read() {
        let id = encounter_of(target);
        if fights.cross(id) {
            info!("quest: {target} turns on the household");
        }
    }
}

/// A quest body's team is the post's: neutral until crossed, then the
/// hostile team, so a hit lands the same either way.
pub fn sync_quest_teams(
    fights: Res<Fights>,
    mut people: Query<(&EncounterId, &mut Combatant), With<Stood>>,
) {
    for (id, mut actor) in &mut people {
        if !matches!(id, EncounterId::Quest(_)) {
            continue;
        }
        let team = if fights.crossed(*id) {
            strata::sim::encounters::hostility(Allegiance::Outlaw(None), true)
        } else {
            NEUTRAL_TEAM
        };
        if actor.team != team {
            actor.team = team;
        }
    }
}

/// The people a `--script-quest` walks to: where each stands, by id.
pub fn standing_at(stands: &Stands, id: &Namespaced) -> Option<[f64; 2]> {
    stands
        .0
        .iter()
        .find(|stand| stand.person == *id && stand.body != Body::Hand)
        .map(|stand| stand.at)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hand(crew: &str) -> Stand {
        Stand {
            person: "veyra:person.pell".parse().expect("an id"),
            name: "Salvager".to_owned(),
            colour: [0; 3],
            at: [0.0; 2],
            yaw: 0.0,
            body: Body::Hand,
            crew: Some((crew.to_owned(), strata::registry::FoeKind::Rusher)),
            gone: None,
        }
    }

    #[test]
    fn a_crew_the_log_keeps_crossed_stands_crossed_again() {
        let mut log = QuestLog::default();
        assert!(!is_crossed(&hand("pell"), &log));
        log.hostile.insert("pell".to_owned());
        assert!(is_crossed(&hand("pell"), &log));
        assert!(!is_crossed(&hand("wim_larrow"), &log));
    }
}
