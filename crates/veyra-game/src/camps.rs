//! The outlaw camps' and the posts' people: what the sim stands up
//! (`veyra_sim::encounters`) turned into bodies and back as the household
//! nears them, and the words on their plates. The stills that put a member
//! at a camp (`--at camp`, `--cleared`, `--cache`) are the game binary's.

use bevy::prelude::*;
use strata::physics::avian3d::prelude::{SpatialQuery, SpatialQueryFilter};
use strata::prelude::*;
use strata::registry::faction::ClaimKind;
use strata::sim::combat::{BlasterBolt, Combatant};
use strata::sim::encounters::{
    Allegiance, EncounterChanged, EncounterId, Fights, NearEncounters, Slot, hostility,
};
use strata::sim::factions::Factions;
use strata::veyra_world::factions::FactionId;

use crate::hostiles::{Birth, spawn_hostile};

/// The outlaw camp's site kind.
pub const CAMP_KIND: &str = "strata:wild_camp";

/// The derelict machine's site kind, whose security wears grey metal.
pub const MACHINE_KIND: &str = "strata:wreck_works";

/// The plate's words for a post's person: `Pelagic Works guard`.
fn plate_name(kind: ClaimKind, faction: &str) -> String {
    let key = if kind == ClaimKind::Reserve {
        "strata.post.warden"
    } else {
        "strata.post.guard"
    };
    let mut name = faction.to_owned();
    if let Some(first) = name.get_mut(..1) {
        first.make_ascii_uppercase();
    }
    veyra_words::english(key)
        .unwrap_or(key)
        .replace("{faction}", &name)
}

/// The camps whose people have not all been given ground yet, each as its
/// camp and the index of a slot in its plan. A camp stands up 260 m or so
/// out, where the ground may not have streamed, so a slot waits here until
/// a ray finds it, as the staging fight's places do.
#[derive(Resource, Default)]
pub struct Waiting(Vec<(EncounterId, usize)>);

/// What a standing encounter asks of the game: where its people stand, at
/// what tier, under which allegiance and in whose colour.
struct Stand<'a> {
    slots: &'a [Slot],
    tier: u8,
    allegiance: Allegiance,
    holder: Option<FactionId>,
    /// The words a plate says of a post's person, `None` for a camp's.
    post: Option<ClaimKind>,
    /// A post's deposit centre, which is never in water: a slot of its ring
    /// that is wet is drawn in towards it. `None` for a camp, whose pad is dry.
    centre: Option<[f64; 2]>,
}

/// The camp or post `id` names, as it now stands.
fn stand_of(near: &NearEncounters, id: EncounterId) -> Option<Stand<'_>> {
    if let Some(camp) = near.camps.iter().find(|camp| camp.id == id) {
        return Some(Stand {
            slots: &camp.slots,
            tier: camp.tier,
            allegiance: Allegiance::Outlaw(camp.band),
            holder: camp.band,
            post: None,
            centre: None,
        });
    }
    let post = near.posts.iter().find(|post| post.id == id)?;
    Some(Stand {
        slots: &post.slots,
        tier: post.tier,
        allegiance: Allegiance::Faction(post.faction),
        holder: Some(post.faction),
        post: Some(post.kind),
        centre: Some(post.centre),
    })
}

/// A post's person is the team its post's state says: neutral until the post
/// is crossed, then the hostile team, so a hit lands the same either way and
/// the enemy rule leaves an uncrossed guard out of every "hostile near" test.
pub fn sync_post_teams(
    fights: Res<Fights>,
    mut people: Query<(&EncounterId, &Allegiance, &mut Combatant)>,
) {
    for (id, allegiance, mut actor) in &mut people {
        if !matches!(id, EncounterId::Post(_)) {
            continue;
        }
        let team = hostility(*allegiance, fights.crossed(*id));
        if actor.team != team {
            actor.team = team;
        }
    }
}

/// Turns what the sim stood up and folded into people and back. A folded
/// camp's or post's people and their bolts go; a standing camp's slots get their
/// ground as it streams, and a person is made with its tier's scaled stats,
/// its kind's brain, its holder's allegiance and colour, and no
/// `ResetPosition`, so F5 leaves a camp as it is. A post's people are named
/// for their faction and their post, and stand neutral.
#[allow(
    clippy::too_many_arguments,
    reason = "the sim's changes, the world's ground, and what is made"
)]
pub fn stage_camps(
    mut commands: Commands,
    mut changes: MessageReader<EncounterChanged>,
    near: Res<NearEncounters>,
    mut waiting: ResMut<Waiting>,
    people: Query<(Entity, &EncounterId)>,
    bolts: Query<(Entity, &BlasterBolt)>,
    actors: Query<Entity, With<Combatant>>,
    source: Res<WorldSourceRes>,
    origin: Res<RenderOrigin>,
    spatial: SpatialQuery,
    registries: Res<strata::sim::Registries>,
    factions: Option<Res<Factions>>,
) {
    for change in changes.read() {
        for id in &change.0.folded {
            waiting.0.retain(|(waiting, _)| waiting != id);
            let gone: Vec<Entity> = people
                .iter()
                .filter(|(_, person)| *person == id)
                .map(|(entity, _)| entity)
                .collect();
            for (bolt, shot) in &bolts {
                if gone.contains(&shot.owner) {
                    commands.entity(bolt).despawn();
                }
            }
            for entity in gone {
                commands.entity(entity).despawn();
            }
        }
        for id in &change.0.stood {
            if let Some(stand) = stand_of(&near, *id) {
                waiting
                    .0
                    .extend((0..stand.slots.len()).map(|index| (*id, index)));
            }
        }
    }
    if waiting.0.is_empty() {
        return;
    }
    let filter = SpatialQueryFilter::from_excluded_entities(actors.iter());
    let mut pending = Vec::new();
    for (id, index) in waiting.0.drain(..) {
        let Some(stand) = stand_of(&near, id) else {
            continue;
        };
        let Slot { kind, at: [x, z] } = stand.slots[index];
        let (x, z) = stand
            .centre
            .map_or((x, z), |centre| dry_towards(&source, [x, z], centre));
        let Some(ground) = crate::household::ground_at(x, z, &source, &origin, &spatial, &filter)
        else {
            pending.push((id, index));
            continue;
        };
        let birth = Birth {
            passive: false,
            kind,
            tier: stand.tier,
            id,
            allegiance: stand.allegiance,
        };
        let at = WorldPos::new(x, ground + crate::PLAYER_DROP + 0.9, z);
        info!(
            "camp: {} stands at ({x:.0}, {z:.0}), tier {}",
            kind.name(),
            stand.tier
        );
        let mut person = spawn_hostile(&mut commands, at, birth, &registries.0);
        let holder = stand
            .holder
            .zip(factions.as_deref())
            .and_then(|(holder, factions)| factions.0.faction(holder));
        if let Some(holder) = holder {
            person.insert(strata::sim::view::BodyColour(holder.colour));
            if let Some(kind) = stand.post {
                person.insert(Name::new(plate_name(kind, &holder.name)));
            }
        } else {
            // An unbanded outlaw keeps the second household look, as before.
            person.insert(strata::sim::view::BodyTint(1));
            // Scavengers and security units are of no faction: the place's
            // kind words their plate, and a machine's people wear grey.
            let place = near
                .camps
                .iter()
                .find(|camp| camp.id == id)
                .and_then(|camp| registries.0.sites.get(camp.site.kind))
                .map(|def| def.id.to_string());
            if let Some(place) = place.as_deref() {
                if let Some(name) = crew_name(place) {
                    person.insert(Name::new(name));
                }
                if place == MACHINE_KIND {
                    person.insert(strata::sim::view::BodyColour(
                        strata::sim::view::MACHINE_STEEL,
                    ));
                }
            }
        }
    }
    waiting.0 = pending;
}

/// What a plate says of the unaligned crew of a place of `kind`, if the
/// kind has one (a camp's people keep their kind's name).
pub fn crew_name(kind: &str) -> Option<&'static str> {
    veyra_words::english(&format!("strata.crew.{kind}"))
}

/// The first dry point from `slot` in towards `centre`, in quarters of the
/// way; the centre itself when every step is wet.
fn dry_towards(source: &WorldSourceRes, slot: [f64; 2], centre: [f64; 2]) -> (f64, f64) {
    let world = source.source();
    [1.0, 0.75, 0.5, 0.25]
        .into_iter()
        .map(|share| {
            (
                centre[0] + (slot[0] - centre[0]) * share,
                centre[1] + (slot[1] - centre[1]) * share,
            )
        })
        .find(|&(x, z)| !world.sample(x, z).in_water())
        .unwrap_or((centre[0], centre[1]))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The unaligned crews are worded by their place; a camp's are not.
    #[test]
    fn the_crew_of_a_wreck_and_a_machine_have_plates_of_their_own() {
        assert_eq!(crew_name("strata:wreck"), Some("Scavenger"));
        assert_eq!(crew_name("strata:wreck_works"), Some("Security unit"));
        assert_eq!(crew_name(CAMP_KIND), None);
    }
}
