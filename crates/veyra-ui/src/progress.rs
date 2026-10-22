//! What the HUD says about progress: an XP toast when the ledger credits
//! the local player, and a hint when a box's rules come to pass.
//!
//! The ledger credits every hit, and a carbine lands four a second, so
//! the toasts are summed over a short window rather than raised one a
//! hit: a burst reads as one `+72 ranged`. The hint compares the boxes
//! the sheet could take (every rule but the trainer) before and after a
//! change to the sheet, and says so once for each box that newly can; a
//! swap or a lesson changes the learned set wholesale and only moves the
//! baseline, because the player did that themselves. A press the sheet
//! refused (the trigger on an uncertified weapon, a follow-up without Cuts 1)
//! is a short warning naming the box, since the sim drops the press
//! silently and a player would otherwise think the key is broken.
//!
//! [`ProgressCue`] is how a lesson and a switch reach the interface's
//! sounds, from the screens and from the game's own scripted path alike.

use std::collections::{BTreeMap, BTreeSet};
use std::time::Duration;

use bevy::ecs::prelude::*;
use bevy::time::Time;
use slotted::menu::{ToastLevel, ToastSpec, toast};
use strata_registry::progression::{Effect, SkillGraph, XpKind};
use veyra_sim::ability::{AbilityRefusal, Bar};
use veyra_sim::combat::{Combatant, Refusal};
use veyra_sim::progression::{CharacterSheet, XpGained};
use veyra_sim::view::WorldNotice;
use veyra_sim::{LocalPlayer, Registries};

use super::sheet::available;

/// Seconds XP is summed over before it is shown.
const XP_WINDOW: f32 = 0.5;

/// What a burst of credits has summed to so far.
#[derive(Debug, Default)]
pub struct PendingXp {
    since: f32,
    sums: BTreeMap<XpKind, (u32, bool)>,
}

pub use veyra_sim::view::ProgressCue;

/// The toast's words for a kind's sum: `+24 ranged`, `+74 blade · a kill`.
#[must_use]
pub fn xp_words(kind: XpKind, amount: u32, kill: bool) -> String {
    if kill {
        format!("+{amount} {kind}  ·  a kill")
    } else {
        format!("+{amount} {kind}")
    }
}

/// Whether a toast's words are [`xp_words`]: a `+`, a number and a space.
/// The interface's sounds tell an XP credit from any other info toast by
/// this, and no other toast starts that way.
#[must_use]
pub fn is_xp_words(words: &str) -> bool {
    words
        .strip_prefix('+')
        .and_then(|rest| rest.split_once(' '))
        .is_some_and(|(amount, _)| amount.parse::<u32>().is_ok())
}

/// `Update`: sums the local player's credits and toasts each kind's sum
/// when the window closes.
pub fn xp_toasts(
    time: Res<Time>,
    mut gained: MessageReader<XpGained>,
    players: Query<(), With<LocalPlayer>>,
    mut pending: Local<PendingXp>,
    mut commands: Commands,
) {
    for gain in gained.read() {
        if !players.contains(gain.entity) {
            continue;
        }
        let entry = pending.sums.entry(gain.kind).or_default();
        entry.0 += gain.amount;
        entry.1 |= gain.kill;
    }
    if pending.sums.is_empty() {
        pending.since = 0.;
        return;
    }
    pending.since += time.delta_secs();
    if pending.since < XP_WINDOW {
        return;
    }
    for (kind, (amount, kill)) in std::mem::take(&mut pending.sums) {
        toast(
            &mut commands,
            ToastSpec::new(xp_words(kind, amount, kill))
                .level(ToastLevel::Info)
                .duration(Duration::from_millis(1800)),
        );
    }
    pending.since = 0.;
}

/// What the hint last saw of the sheet.
#[derive(Debug, Default)]
pub struct Seen {
    learned: BTreeSet<String>,
    available: BTreeSet<String>,
}

/// `Update`: a box whose rules newly pass is announced, with who teaches
/// it.
pub fn box_hints(
    registries: Option<Res<Registries>>,
    sheets: Query<&CharacterSheet, (With<LocalPlayer>, Changed<CharacterSheet>)>,
    mut seen: Local<Option<Seen>>,
    mut commands: Commands,
) {
    let Some(registries) = registries else {
        return;
    };
    let Ok(sheet) = sheets.single() else {
        return;
    };
    let graph = &registries.0.progression;
    let now = Seen {
        learned: sheet.learned.clone(),
        available: available(graph, sheet),
    };
    if let Some(before) = seen.as_ref()
        && before.learned == now.learned
    {
        for id in now.available.difference(&before.available).take(2) {
            let Some(b) = graph.find(id) else {
                continue;
            };
            let tree = graph.tree_of(id);
            let teacher = if tree.is_some_and(|t| t.bench) {
                "go to a workbench".to_owned()
            } else {
                format!(
                    "see the {} trainer",
                    tree.map_or("a", |t| t.name.as_str()).to_lowercase()
                )
            };
            toast(
                &mut commands,
                ToastSpec::new(format!("{} can be learned: {teacher}", b.name))
                    .level(ToastLevel::Success)
                    .duration(Duration::from_secs(4)),
            );
        }
    }
    *seen = Some(now);
}

/// The words for a refused press: the weapon the sheet has not certified,
/// or the action and the box of the graph that unlocks it.
#[must_use]
pub fn refusal_words(graph: &SkillGraph, refusal: Refusal) -> String {
    let english = |key: &str| super::strings::english(key).unwrap_or(key).to_owned();
    match refusal {
        Refusal::Fire(weapon) => english("strata.refused.fire")
            .replace("{weapon}", weapon.name().to_lowercase().as_str()),
        Refusal::Draw(weapon) => english("strata.refused.draw")
            .replace("{weapon}", weapon.name().to_lowercase().as_str()),
        Refusal::NoBlade => english("strata.refused.no_blade"),
        Refusal::Action(action) => {
            let unlocks = graph.boxes().find(|b| {
                b.effects
                    .iter()
                    .any(|e| matches!(e, Effect::Unlock { action: a } if a == action.name()))
            });
            english("strata.refused.action")
                .replace("{action}", &super::sheet::action_words(action.name()))
                .replace(
                    "{box}",
                    unlocks.map_or("a box not in the game", |b| b.name.as_str()),
                )
        }
        Refusal::Ability(why) => ability_words(why),
    }
}

/// The word for an ability press the rule of `can_use` refused. A press of
/// an empty place is said by nothing, so it has no word.
pub fn ability_words(why: AbilityRefusal) -> String {
    let english = |key: &str| super::strings::english(key).unwrap_or(key).to_owned();
    match why {
        AbilityRefusal::Empty => String::new(),
        AbilityRefusal::WrongWeapon => english("strata.refused.ability.weapon"),
        AbilityRefusal::Cooldown(left) => english("strata.refused.ability.cooldown")
            .replace("{seconds}", &format!("{:.0}", left.ceil())),
        AbilityRefusal::Stamina(_) => english("strata.refused.ability.stamina"),
        AbilityRefusal::NoRounds => english("strata.refused.ability.rounds"),
        AbilityRefusal::Nothing => english("strata.refused.ability.nothing"),
        AbilityRefusal::Busy => english("strata.refused.ability.busy"),
        AbilityRefusal::Disarmed => english("strata.refused.ability.disarmed"),
    }
}

/// `Update`: the local player's refused presses, one toast a press.
pub fn refusal_toasts(
    registries: Option<Res<Registries>>,
    actors: Query<&Combatant, (With<LocalPlayer>, Changed<Combatant>)>,
    mut seen: Local<Option<u64>>,
    mut commands: Commands,
) {
    let Ok(actor) = actors.single() else {
        return;
    };
    let last = seen.replace(actor.refusals);
    // The first sight of an actor (a spawn, a swap) is a baseline, not a
    // burst of everything it was ever refused.
    if last.is_none_or(|last| last == actor.refusals) {
        return;
    }
    let (Some(registries), Some(refusal)) = (registries, actor.refused) else {
        return;
    };
    // The action bar flashes the square and says this one itself.
    if matches!(refusal, Refusal::Ability(_)) {
        return;
    }
    toast(
        &mut commands,
        ToastSpec::new(refusal_words(&registries.0.progression, refusal))
            .level(ToastLevel::Warning)
            .duration(Duration::from_millis(1600)),
    );
}

/// A plain notice from the world (the game's first-day line about where
/// the props stand), toasted the way the ledger's credits are.
pub fn notice(commands: &mut Commands, words: impl Into<String>) {
    toast(
        commands,
        ToastSpec::new(words)
            .level(ToastLevel::Info)
            .duration(Duration::from_secs(6)),
    );
}

/// The household's plain lines as toasts: it has no interface of its own, so
/// it writes a [`WorldNotice`] and the screens say it.
pub fn hear_notices(mut notices: MessageReader<WorldNotice>, mut commands: Commands) {
    for WorldNotice(words) in notices.read() {
        notice(&mut commands, words.clone());
    }
}

/// What a lesson that granted an ability says of where the bar put it:
/// the keys of its places, or the way to give it one when it found none.
#[must_use]
pub fn placed_words(
    tables: &veyra_sim::combat_tables::CombatTables,
    bar: &Bar,
    id: &str,
) -> Option<String> {
    let row = tables.abilities.row(id)?;
    let mut keys: Vec<&str> = Vec::new();
    for place in bar.places_of(id) {
        let key = super::sheet::loadout::key_of(place, tables);
        if !keys.contains(&key) {
            keys.push(key);
        }
    }
    let english = |key: &'static str| super::strings::english(key).unwrap_or(key);
    Some(if keys.is_empty() {
        english("strata.progress.learned_free").replace("{ability}", &row.name)
    } else {
        english("strata.progress.learned_on")
            .replace("{ability}", &row.name)
            .replace("{keys}", &keys.join(", "))
    })
}

/// `Update`: a lesson is followed one tick later by the bar the sim held to
/// the new sheet, and each ability that came with it is toasted with the
/// place it was given. The known set is followed every frame, so the answer
/// does not depend on whether this system or the sheet's ran first in the
/// frame the cue was written; the first bar change after a cue is the one
/// read, and a bar that changes with no cue says nothing.
pub fn ability_toasts(
    mut cues: MessageReader<ProgressCue>,
    bars: Query<Ref<Bar>, With<LocalPlayer>>,
    mut last: Local<BTreeSet<String>>,
    mut pending: Local<bool>,
    mut commands: Commands,
) {
    if cues.read().any(|cue| *cue == ProgressCue::Learned) {
        *pending = true;
    }
    let Ok(bar) = bars.single() else {
        return;
    };
    if *pending && bar.is_changed() {
        *pending = false;
        let tables = veyra_sim::combat_tables::current();
        for row in &tables.abilities.abilities {
            if bar.known.contains(&row.id)
                && !last.contains(&row.id)
                && let Some(words) = placed_words(&tables, &bar, &row.id)
            {
                toast(
                    &mut commands,
                    ToastSpec::new(words)
                        .level(ToastLevel::Success)
                        .duration(Duration::from_secs(4)),
                );
            }
        }
    }
    if bar.is_changed() {
        last.clone_from(&bar.known);
    }
}

/// Registers the cue and the three systems. Only called once the UI is
/// wired.
pub(crate) fn register(app: &mut bevy::app::App) {
    app.add_message::<ProgressCue>()
        .add_message::<WorldNotice>()
        .add_systems(
            bevy::app::Update,
            (
                xp_toasts,
                box_hints,
                refusal_toasts,
                ability_toasts,
                hear_notices,
            ),
        );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_words_are_short() {
        assert_eq!(xp_words(XpKind::Ranged, 24, false), "+24 ranged");
        assert_eq!(xp_words(XpKind::Blade, 74, true), "+74 blade  ·  a kill");
        assert!(is_xp_words(&xp_words(XpKind::Ranged, 24, false)));
        assert!(is_xp_words(&xp_words(XpKind::Blade, 74, true)));
        assert!(!is_xp_words("+ a terminal"));
        assert!(!is_xp_words("Cuts 1 can be learned: see the blade trainer"));
    }

    #[test]
    fn a_learned_ability_says_where_the_bar_put_it() {
        use veyra_sim::ability::Place;
        use veyra_sim::combat::Weapon;
        let tables = veyra_sim::combat_tables::current();
        let mut bar = Bar::default();
        assert_eq!(
            placed_words(&tables, &bar, "aimed_burst").as_deref(),
            Some("Learned Aimed burst. Press L to give it a place.")
        );
        bar.set(Place::Weapon(Weapon::Carbine, 0), "aimed_burst");
        assert_eq!(
            placed_words(&tables, &bar, "aimed_burst").as_deref(),
            Some("Learned Aimed burst. On Q.")
        );
        assert_eq!(placed_words(&tables, &bar, "no_such_ability"), None);
    }

    #[test]
    fn a_refusal_names_the_weapon_or_the_box() {
        use veyra_sim::combat::{Action, Weapon};
        let graph = SkillGraph::shipped();
        assert_eq!(
            refusal_words(&graph, Refusal::Fire(Weapon::Carbine)),
            "Not certified on the carbine"
        );
        assert_eq!(
            refusal_words(&graph, Refusal::Draw(Weapon::Pistol)),
            "Cannot draw the pistol: not certified"
        );
        assert_eq!(
            refusal_words(&graph, Refusal::NoBlade),
            "No blade in hand",
            "a cut with a gun in hand says why"
        );
        assert_eq!(
            refusal_words(&graph, Refusal::Action(Action::Heavy)),
            "The heavy cut needs Blade novice"
        );
        assert_eq!(
            refusal_words(&graph, Refusal::Action(Action::Followup)),
            "The follow-up cut needs One hand 1"
        );
    }
}
