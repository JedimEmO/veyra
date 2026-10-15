//! `--stance` and `--lease`: a still's ledger (ticket 0012, step 3). A stance
//! is set the only way the sim knows, by a deed written at the start whose
//! weight puts the standing in the band, so the rule that reads it is the one
//! the game runs; a lease is one bought for nothing.

use bevy::prelude::*;
use strata::prelude::*;
use strata::registry::DeedRules;
use strata::sim::deeds::{DeedKind, Deeds, Leases, Stance, anchor_faction, lease_buyer, start_of};
use strata::sim::encounters::PlayedClock;
use strata::sim::factions::Factions;

use super::Cli;

/// How far round the member a named faction is looked for, metres.
const SEARCH_M: f64 = 40_000.0;

/// The score a stance's band holds, one step inside its edge.
fn score_for(stance: Stance, rules: &DeedRules) -> f64 {
    match stance {
        Stance::Welcoming => rules.bands.welcoming_at + 1.0,
        Stance::Wary => 0.0,
        Stance::Hostile => rules.bands.hostile_at - 1.0,
    }
}

/// The stance a flag's word names.
fn parse(word: &str) -> Option<Stance> {
    match word {
        "welcoming" => Some(Stance::Welcoming),
        "wary" => Some(Stance::Wary),
        "hostile" => Some(Stance::Hostile),
        _ => None,
    }
}

/// Once, when the world is up: write the deeds that put each named faction
/// in its stance and buy the lease for nothing.
#[allow(
    clippy::too_many_arguments,
    reason = "the flags, the data, the map, the clock, the two books, the player and the once"
)]
pub(super) fn set_stance_and_lease(
    cli: Res<Cli>,
    registries: Res<strata::sim::Registries>,
    factions: Option<Res<Factions>>,
    clock: Res<PlayedClock>,
    mut deeds: ResMut<Deeds>,
    mut leases: ResMut<Leases>,
    player: Query<&WorldPosition, With<LocalPlayer>>,
    mut done: Local<bool>,
) {
    if *done || (cli.stance.is_empty() && !cli.lease) {
        return;
    }
    let (Some(factions), Ok(position), Some(rules), Some(claims)) = (
        factions,
        player.single(),
        registries.0.factions.deeds(),
        registries.0.factions.claims(),
    ) else {
        return;
    };
    *done = true;
    let (x, z) = (position.0.x(), position.0.z());
    for flag in &cli.stance {
        let Some((name, word)) = flag.split_once('=') else {
            error!("--stance {flag}: expected FACTION=STANCE");
            continue;
        };
        let (Some(stance), Some(faction)) = (
            parse(word),
            factions
                .0
                .factions_near(x, z, SEARCH_M)
                .into_iter()
                .find(|faction| faction.name == name),
        ) else {
            error!("--stance {flag}: no such stance or no such faction near");
            continue;
        };
        let gap = score_for(stance, rules) - start_of(&registries.0, &faction);
        let (kind, per) = if gap >= 0.0 {
            (DeedKind::ClearedCamp, rules.weights.cleared_camp)
        } else {
            (DeedKind::WorkedReserve, rules.weights.worked_reserve)
        };
        #[allow(
            clippy::cast_possible_truncation,
            clippy::cast_sign_loss,
            reason = "a small positive count of deeds for a still"
        )]
        let amount = (gap / per).ceil().max(0.0) as u32;
        deeds.write(faction.id, kind, amount, clock.seconds, rules);
    }
    if cli.lease {
        let pelagic = registries
            .0
            .factions
            .anchors()
            .iter()
            .find_map(|anchor| anchor.buyer.clone());
        let faction = pelagic.and_then(|buyer| {
            let faction = anchor_faction(&factions.0, &registries.0, &buyer)?;
            lease_buyer(&registries.0, &faction).map(|_| faction)
        });
        if let Some(faction) = faction {
            let until = clock.seconds + claims.lease.hours * 3600.0;
            leases.until.insert(faction.id.0, until);
        }
    }
}
