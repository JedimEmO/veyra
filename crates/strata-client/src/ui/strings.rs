//! English for the game's own locale keys.
//!
//! A screen names a key and a catalogue answers it; the game has no `.ftl`
//! files yet, so this fallback is the catalogue, pushed after slotted's own
//! (which answers `slotted.menu.*`) and asked only when nothing else
//! defines a key. A translation lands later as a catalogue that wins over
//! this one, and nothing in the screens changes.

use bevy::ecs::prelude::*;
use slotted::ui::{LocArgs, LocKey, Localization, Localizer};

/// The main menu's title key (the pause screen does not show it, but
/// `MenuConfig` wants one).
pub const TITLE: &str = "strata.title";

/// The English fallback for `strata.*` keys.
#[derive(Debug, Default, Clone, Copy)]
pub struct StrataStrings;

/// The English for `key`, if it is one of ours.
#[must_use]
#[allow(clippy::too_many_lines, reason = "one flat table of words")]
pub fn english(key: &str) -> Option<&'static str> {
    if let Some(label) = super::render::english(key)
        .or_else(|| sheet_english(key))
        .or_else(|| combat_english(key))
        .or_else(|| guard_english(key))
        .or_else(|| scale_english(key))
        .or_else(|| world_english(key))
        .or_else(|| audio_english(key))
        .or_else(|| place_english(key))
        .or_else(|| fitting_english(key))
        .or_else(|| figure_english(key))
        .or_else(|| stores_english(key))
        .or_else(|| bike_english(key))
        .or_else(|| survey_english(key))
        .or_else(|| hand_survey_english(key))
        .or_else(|| crafting_english(key))
        .or_else(|| blueprint_english(key))
        .or_else(|| factory_english(key))
        .or_else(|| board_english(key))
        .or_else(|| harvester_english(key))
        .or_else(|| loot_english(key))
        .or_else(|| quest_english(key))
    {
        return Some(label);
    }
    Some(match key {
        TITLE => "sci-fi-scrolls",
        "strata.settings.display" => "Display",
        "strata.settings.graphics" => "Graphics",
        "strata.settings.graphics.low" => "Low",
        "strata.settings.graphics.medium" => "Medium",
        "strata.settings.graphics.high" => "High",
        "strata.settings.fov" => "Field of view",
        "strata.settings.foliage_distance" => "Foliage render distance",
        "strata.settings.show_hud" => "Show HUD",
        "strata.settings.camera_shake" => "Camera shake",
        "strata.settings.controls" => "Controls",
        "strata.settings.mouse_sensitivity" => "Mouse sensitivity",
        "strata.settings.audio" => "Audio",
        "strata.settings.master_volume" => "Master volume",
        "strata.quit.title" => "Quit?",
        "strata.quit.message" => "Leave the world? It keeps running without you.",
        "strata.quit.accept" => "Quit",
        "strata.menu.character" => "Character",
        "strata.menu.bench" => "Bench",
        "strata.menu.inventory" => "Inventory",
        "strata.menu.close" => "Close",
        "strata.sheet.subtitle" => "Character sheet",
        "strata.sheet.professions" => "PROFESSIONS",
        "strata.sheet.placeholders" => "NOT YET TAUGHT",
        "strata.sheet.learn" => "Learn",
        "strata.sheet.forget" => "Forget",
        "strata.sheet.keys" => "C or Esc closes",
        "strata.sheet.empty" => "No professions are known in this world.",
        "strata.bench.title" => "The bench",
        "strata.bench.out" => "OUT NOW",
        "strata.bench.playing" => "Out now",
        "strata.bench.switch" => "Switch",
        "strata.bench.join" => "Join",
        "strata.bench.recruits" => "WHO CAN JOIN",
        "strata.bench.nobody" => "Nobody is waiting to join.",
        "strata.bench.read_only" => {
            "Switching and joining happen at a terminal; this is the household as it stands."
        }
        "strata.bench.fighting" => "At the terminal, but not while fighting.",
        "strata.bench.here" => "At the terminal: bring someone else out, or take a recruit on.",
        "strata.sheet.not_yet" => "not yet in the game",
        "strata.sheet.more" => "not yet taught",
        "strata.sheet.fewer" => "Hide the rest",
        "strata.bench.nearest_terminal" => "the nearest terminal is",
        "strata.bench.not_certified" => "(not certified)",
        "strata.prompt.terminal" => "Use the terminal",
        "strata.prompt.train" => "Train with the {tree} trainer",
        "strata.prompt.locker" => "Open the locker",
        "strata.prompt.workbench" => "Use the workbench",
        "strata.prompt.fabricator" => "Use the fabricator",
        "strata.prompt.harvester" => "Read the field harvester",
        "strata.card.kind.harvester" => "Field harvester",
        "strata.prompt.board" => "Read the Pelagic procurement board",
        "strata.crew.strata:wreck" => "Scavenger",
        "strata.crew.strata:wreck_works" => "Security unit",
        "strata.post.guard" => "{faction} guard",
        "strata.post.warden" => "{faction} warden",
        "strata.post.guards" => "guards",
        "strata.post.wardens" => "wardens",
        "strata.post.warning" => "Leave the claim  {n}",
        "strata.post.turned" => "{faction} {people} turn on you",
        "strata.post.lease" => "Lease {faction} concessions  {price} cr",
        "strata.household.first_day" => {
            "A terminal, two trainers, a locker, a workbench and a fabricator stand behind you; the hostiles are ahead."
        }
        "strata.refused.fire" => "Not certified on the {weapon}",
        "strata.refused.draw" => "Cannot draw the {weapon}: not certified",
        "strata.refused.no_blade" => "No blade in hand",
        "strata.refused.ability.weapon" => "Not with this weapon",
        "strata.refused.ability.cooldown" => "On cooldown, {seconds} s",
        "strata.refused.ability.stamina" => "Not enough stamina",
        "strata.refused.ability.rounds" => "Magazine too low",
        "strata.refused.ability.nothing" => "Nothing to do",
        "strata.refused.ability.busy" => "Busy",
        "strata.refused.ability.disarmed" => "Disarmed",
        "strata.bar.key.0" => "Q",
        "strata.bar.key.1" => "E",
        "strata.bar.key.2" => "T",
        "strata.bar.key.3" => "3",
        "strata.bar.key.4" => "4",
        "strata.bar.key.weapon.0" => "1",
        "strata.bar.key.weapon.1" => "2",
        "strata.status.staggered" => "Staggered",
        "strata.controls.key.abilities" => "Q E T",
        "strata.controls.abilities" => "Abilities",
        "strata.controls.key.utility" => "3 4",
        "strata.controls.utility" => "Utility",
        "strata.refused.action" => "The {action} needs {box}",
        // The debug HUD's text before the first refresh writes over it.
        "strata.hud.stats" => "...",
        _ => return None,
    })
}

// ---- the character sheet's tree and the bench's points (sheet.rs, bench.rs) ----
/// The words of the sheet's Loadout tab, apart from [`sheet_english`] for
/// the same reason.
fn loadout_english(key: &str) -> Option<&'static str> {
    Some(match key {
        "strata.sheet.loadout" | "strata.menu.loadout" => "Loadout",
        "strata.sheet.tab.professions" => "Professions",
        "strata.loadout.utility" => "UTILITY",
        "strata.loadout.known" => "KNOWN",
        "strata.progress.learned_on" => "Learned {ability}. On {keys}.",
        "strata.progress.learned_free" => "Learned {ability}. Press L to give it a place.",
        "strata.bar.plus" => "[+]",
        "strata.bar.hint" => "L: abilities",
        "strata.loadout.not_learned" => "NOT LEARNED",
        "strata.loadout.locked_by" => "Taught by {tree}: {box}",
        "strata.loadout.none" => "Nothing learned yet that goes there.",
        "strata.loadout.empty" => "No class is certified, and nothing is learned.",
        "strata.loadout.hint" => {
            "Enter on a place picks it. Enter on an ability puts it there. Enter on that place again clears it."
        }
        "strata.loadout.pick_first" => "Pick a place first.",
        "strata.loadout.in" => "In {key} of {class}",
        "strata.loadout.in_utility" => "In {key}",
        "strata.loadout.set" => "{ability} is in {key} of {class}.",
        "strata.loadout.set_utility" => "{ability} is in {key}.",
        "strata.loadout.cleared" => "{key} of {class} is empty.",
        "strata.loadout.cleared_utility" => "{key} is empty.",
        "strata.loadout.refused.fight" => "Not during a fight",
        "strata.loadout.refused.nobody" => "There is no one to hold a bar",
        "strata.loadout.refused.place" => "There is no such place",
        "strata.loadout.refused.unknown" => "There is no such ability",
        "strata.loadout.refused.unlearned" => "Not learned",
        "strata.loadout.refused.kind" => "Goes in the other kind of place",
        "strata.loadout.refused.weapon" => "Does not work with {class}",
        "strata.loadout.refused.placed" => "Already in another place",
        "strata.loadout.works_with" => "Works with {classes}",
        "strata.loadout.works_any" => "Works with any weapon",
        "strata.loadout.utility_kind" => "A utility place",
        "strata.loadout.taught" => "Taught by {box}, {tree}",
        "strata.loadout.given" => "Given by hand",
        "strata.loadout.stamina" => "Stamina",
        "strata.loadout.cooldown" => "Cooldown",
        "strata.loadout.windup" => "Windup",
        "strata.loadout.commitment" => "Commits you for",
        "strata.loadout.effect.cut" => "Makes the {subject} cut",
        "strata.loadout.effect.shot" => "Fires the weapon in hand",
        "strata.loadout.effect.strike" => "Strikes all in reach",
        "strata.loadout.effect.hit" => "One attack that puts {subject} on",
        "strata.loadout.effect.move" => "Carries you",
        "strata.loadout.effect.reload" => "Fills the magazine",
        "strata.loadout.effect.shield" => "The shield recovers at once",
        "strata.loadout.effect.status" => "Gives you {subject}",
        "strata.loadout.figure.rounds" => "Bolts",
        "strata.loadout.figure.damage" => "Damage",
        "strata.loadout.figure.posture" => "Posture damage",
        "strata.loadout.figure.reach" => "Reach",
        "strata.loadout.figure.arc" => "Arc",
        "strata.loadout.figure.distance" => "Moves you",
        "strata.loadout.figure.lasts" => "Lasts",
        "strata.loadout.figure.takes" => "Takes",
        "strata.loadout.value.points" => "{n}",
        "strata.loadout.value.percent" => "{n} % of the weapon's",
        "strata.loadout.value.metres" => "{n} m",
        "strata.loadout.value.degrees" => "{n} degrees either side",
        "strata.loadout.value.seconds" => "{n} s",
        _ => return None,
    })
}

/// The words of the sheet's tree and the bench's points, apart from
/// [`english`] so each screen's catalogue stays a readable length.
fn sheet_english(key: &str) -> Option<&'static str> {
    Some(match key {
        "strata.sheet.state.learned" => "Learned",
        "strata.sheet.state.learnable" => "Learn here",
        "strata.sheet.state.trainer" => "Trainer",
        "strata.sheet.state.xp" => "{has} / {needs} XP",
        "strata.sheet.state.locked" => "Locked",
        "strata.sheet.state.no_points" => "No points",
        "strata.sheet.state.not_yet" => "Not yet",
        "strata.sheet.cost" => "{cost} PTS",
        "strata.sheet.progress" => "{learned} of {total} boxes",
        "strata.sheet.legend.learned" => "LEARNED",
        "strata.sheet.legend.taught" => "TAUGHT HERE",
        "strata.sheet.legend.locked" => "LOCKED",
        "strata.sheet.effect.to" => "to",
        "strata.sheet.no_effect" => "No effect yet.",
        "strata.sheet.teaches" => "Teaches: {name}",
        "strata.sheet.learn.bench" => "A workbench teaches it for {points} points.",
        "strata.sheet.learn.foreman" => "The works foreman teaches it for {points} points.",
        "strata.sheet.need.bench" => "Needs a workbench or the works foreman.",
        "strata.sheet.effect.read.type" => "Reads a deposit's type and concentration",
        "strata.sheet.effect.read.bands" => "Reads each line of a deposit as a band",
        "strata.sheet.effect.read.exact" => "Reads each line of a deposit exactly",
        "strata.sheet.effect.peak" => "A ping also tells where each deposit's richest ground lies",
        "strata.sheet.effect.survey.range_m" => "Ping reach {change}",
        "strata.sheet.effect.survey.grid" => "Ping grid, points a side, {change}",
        "strata.sheet.effect.survey.channel_s" => "Ping time {change}",
        "strata.sheet.effect.survey.units" => "Units a sampling cycle {change}",
        "strata.sheet.effect.survey.cycle_s" => "Sampling cycle time {change}",
        "strata.sheet.effect.ride.call_s" => "Bike: call time {change}",
        "strata.sheet.effect.ride.speed_road" => "Bike: road speed {change}",
        "strata.sheet.effect.ride.speed_ground" => "Bike: off-road speed {change}",
        "strata.sheet.effect.ride.stop_deg" => "Bike: steepest wall it rides over {change}",
        "strata.sheet.effect.ride.grip" => "Bike: grip {change}",
        "strata.sheet.effect.craft.assembly" => "Bench: assembly {change}",
        "strata.sheet.effect.factory.seconds" => "Fabricator: a unit in {change}",
        "strata.sheet.effect.factory.seconds_less" => "Fabricator: a unit in {change} less time",
        "strata.sheet.effect.factory.seconds_more" => "Fabricator: a unit in {change} more time",
        "strata.sheet.effect.factory.runs" => "Fabricator: runs a blueprint starts with {change}",
        "strata.sheet.effect.factory.unattended" => "Fabricator: unattended share {change}",
        "strata.sheet.effect.grant" => "Learn the {ability} ability",
        "strata.sheet.effect.ability.stamina" => "{ability}: stamina cost {change}",
        "strata.sheet.effect.ability.cooldown" => "{ability}: cooldown {change}",
        "strata.sheet.effect.ability.windup" => "{ability}: windup {change}",
        "strata.sheet.effect.ability.length" => "{ability}: commitment {change}",
        "strata.sheet.effect.ability.damage" => "{ability}: damage {change}",
        "strata.sheet.effect.ability.posture" => "{ability}: posture damage {change}",
        "strata.sheet.effect.ability.seconds" => "{ability}: lasts {change}",
        "strata.sheet.effect.guard.share" => "Guard: damage taken off {change}",
        "strata.sheet.effect.guard.arc" => "Guard: arc {change}",
        "strata.sheet.effect.guard.stamina" => "Guard: stamina cost {change}",
        "strata.sheet.effect.guard.posture" => "Guard: posture through {change}",
        "strata.sheet.effect.guard.parry" => "Guard: parry window {change}",
        "strata.sheet.effect.guard.riposte" => "Guard: riposte {change}",
        "strata.sheet.effect.station.count" => "Field harvesters you may have planted {change}",
        "strata.sheet.effect.station.rate" => "Field harvester rate {change}",
        "strata.sheet.effect.station.hopper" => "Field harvester hopper {change}",
        "strata.sheet.effect.wear.field" => "Field repair restores {change}",
        "strata.sheet.effect.wear.field_more" => "Field repair restores {change} more",
        "strata.sheet.effect.wear.field_less" => "Field repair restores {change} less",
        "strata.sheet.effect.wear.cost" => "Wear from a shot or a cut {change}",
        "strata.sheet.effect.wear.cost_less" => "A shot or a cut wears the weapon {change} less",
        "strata.sheet.effect.wear.cost_more" => "A shot or a cut wears the weapon {change} more",
        "strata.sheet.effect.wear.bench_kit" => "A workbench repair uses no kit",
        "strata.bench.points" => "points",
        _ => return loadout_english(key),
    })
}
// ---- end of the sheet's tree and the bench's points ----

// ---- the combat HUD: vitals, controls card, floating text (combat_hud.rs, combat_text.rs) ----
/// The words of the combat HUD, apart from [`english`] for the same
/// reason as the sheet's: each catalogue stays a readable length.
#[allow(
    clippy::too_many_lines,
    reason = "one table of the HUD's words, which is how every layer here is kept"
)]
fn combat_english(key: &str) -> Option<&'static str> {
    Some(match key {
        "strata.controls.key.survey" => "G",
        "strata.controls.survey" => "Survey",
        "strata.controls.key.ride" => "V",
        "strata.controls.ride" => "Bike",
        "strata.ride.speed" => "{kmh} km/h",
        "strata.ride.calling" => "Calling the bike",
        "strata.ride.thrown.hit" => "A blow knocked you off the bike",
        "strata.ride.thrown.water" => "The bike stalled in deep water",
        "strata.ride.thrown.crash" => "You crashed",
        "strata.ride.thrown.crash.worn" => "You crashed. The bike is at {condition} of {full}",
        "strata.ride.cancelled.moved" => "You moved. The call is off",
        "strata.ride.cancelled" => "The call is off. {why}",
        "strata.ride.refused.no_bike" => "No bike to call",
        "strata.ride.refused.in_water" => "Not from the water",
        "strata.ride.refused.hostile" => "Not with a hostile this near",
        "strata.ride.refused.busy" => "Not in the middle of an action",
        "strata.ride.refused.already" => "The bike is already here",
        "strata.ride.refused.mounted" => "Get off the bike first",
        "strata.ride.refused.not_a_bike" => "That is not a bike",
        "strata.ride.refused.not_held" => "That bike is not to hand",
        "strata.ride.refused.saddle_loaded" => "Empty the saddle bags first",
        "strata.ride.refused.no_room" => "There is no room for it",
        "strata.vitals.fallback_name" => "Ranger",
        "strata.vitals.carbine" => "CARBINE",
        "strata.vitals.pistol" => "PISTOL",
        "strata.vitals.slot" => "{key}  {weapon}",
        "strata.vitals.slot.hand" => "> {key}  {weapon}",
        "strata.vitals.slot.empty" => "EMPTY",
        "strata.vitals.condition.sound" => "SOUND",
        "strata.vitals.condition.tired" => "TIRED",
        "strata.vitals.condition.worn" => "WORN",
        "strata.family.carbine" => "carbine",
        "strata.family.the.carbine" => "the carbine",
        "strata.family.in_hand.carbine" => "Carbine in hand",
        "strata.family.pistol" => "pistol",
        "strata.family.the.pistol" => "the pistol",
        "strata.family.in_hand.pistol" => "Pistol in hand",
        "strata.family.rifle" => "rifle",
        "strata.family.the.rifle" => "the rifle",
        "strata.family.in_hand.rifle" => "Rifle in hand",
        "strata.family.blade" => "blade",
        "strata.family.the.blade" => "the blade",
        "strata.family.in_hand.blade" => "Blade in hand",
        "strata.family.greatblade" => "greatblade",
        "strata.family.the.greatblade" => "the greatblade",
        "strata.family.in_hand.greatblade" => "Greatblade in hand",
        "strata.family.glaive" => "glaive",
        "strata.family.the.glaive" => "the glaive",
        "strata.family.in_hand.glaive" => "Glaive in hand",
        "strata.vitals.blade" => "BLADE",
        "strata.vitals.greatblade" => "GREATBLADE",
        "strata.vitals.glaive" => "GLAIVE",
        "strata.vitals.rifle" => "RIFLE",
        "strata.vitals.uncertified" => "NOT CERTIFIED",
        "strata.vitals.worn" => "{weapon} WORN",
        "strata.vitals.tired" => "{weapon} TIRED",
        "strata.vitals.knocked" => "Going down knocked {points} off everything you carry.",
        "strata.vitals.engaged" => "ENGAGED",
        "strata.vitals.clear" => "AREA CLEAR",
        "strata.vitals.ahead" => "HOSTILES AHEAD",
        "strata.vitals.streaming" => "STREAMING",
        "strata.vitals.health" => "HP",
        "strata.vitals.shield" => "SHIELD",
        "strata.vitals.stamina" => "STAMINA",
        "strata.vitals.magazine" => "/ {magazine}",
        "strata.vitals.reload" => "RELOAD",
        "strata.vitals.reloading" => "RELOADING",
        "strata.vitals.staggered" => "STAGGERED",
        "strata.vitals.dodge" => "DODGE",
        "strata.vitals.swapping" => "SWAPPING",
        "strata.controls.title" => "CONTROLS  ·  F1",
        "strata.controls.key.move" => "WASD",
        "strata.controls.move" => "Move",
        "strata.controls.key.sprint" => "Shift",
        "strata.controls.sprint" => "Sprint",
        "strata.controls.key.jump" => "Space",
        "strata.controls.jump" => "Jump",
        "strata.controls.key.dodge" => "Alt",
        "strata.controls.dodge" => "Dodge",
        "strata.controls.key.use" => "F",
        "strata.controls.use" => "Use",
        "strata.controls.key.fire" => "LMB",
        "strata.controls.fire" => "Fire",
        "strata.controls.fire.burst" => "Burst (hold: full auto)",
        "strata.controls.key.aim" => "RMB",
        "strata.controls.aim" => "Aim",
        "strata.vitals.reload_key" | "strata.controls.key.reload" => "R",
        "strata.controls.reload" => "Reload",
        "strata.controls.key.weapons" => "1 / 2",
        "strata.controls.weapons" => "Weapons",
        "strata.controls.cut" => "Cut",
        "strata.controls.key.heavy" => "Q",
        "strata.controls.heavy" => "Heavy cut",
        "strata.controls.key.sheet" => "C",
        "strata.controls.sheet" => "Character",
        "strata.controls.key.loadout" => "L",
        "strata.controls.loadout" => "Loadout",
        "strata.controls.key.inventory" => "I",
        "strata.controls.inventory" => "Carried",
        "strata.controls.key.menu" => "Esc",
        "strata.controls.menu" => "Menu",
        "strata.combat_text.hurt" => "-{amount}",
        _ => return None,
    })
}

/// The guard's words (the right button, the stamina row's mark and the
/// floating text), apart from [`combat_english`] to keep it a readable length.
fn guard_english(key: &str) -> Option<&'static str> {
    Some(match key {
        "strata.controls.guard" => "Guard",
        "strata.vitals.guarding" => "GUARD",
        "strata.combat_text.blocked" => "Blocked",
        "strata.combat_text.parried" => "Parried",
        "strata.combat_text.guard_broken" => "Guard broken",
        _ => return None,
    })
}
// ---- end of the combat HUD ----

// ---- the world layer: nameplates and the down card (nameplates.rs, combat_hud.rs) ----
/// The words over the world and on the down card, apart from [`english`]
/// for the same reason as the HUD's.
fn world_english(key: &str) -> Option<&'static str> {
    Some(match key {
        "strata.nameplate.fallback" => "Hostile",
        "strata.down.title" => "DOWN",
        "strata.down.key" => "F5",
        "strata.down.retry" => "Retry",
        _ => return None,
    })
}
// ---- end of the world layer ----

// ---- the place card (place.rs) ----
/// The words of the place card: what each site kind is called on it. The
/// name itself is the site's, made by the world, and is no key.
fn place_english(key: &str) -> Option<&'static str> {
    Some(match key {
        "strata.place.unnamed" | "strata.place.kind" | "strata.place.held" => "",
        "strata.place.faction.strata:corporate" => "Corporate concern",
        "strata.place.faction.strata:assembly" => "Settlers' assembly",
        "strata.place.faction.strata:conservation" => "Conservation body",
        "strata.place.faction.strata:outlaw" => "Outlaw band",
        "strata.place.held.by" => "Held by {faction}",
        "strata.place.cleared" => "{kind} · Cleared",
        "strata.place.cache" => "{kind} · a cache",
        "strata.place.ground" => "{kind} ground",
        "strata.place.ground.strata:outlaw" => "Outlaw ground",
        "strata.place.unclaimed" => "Unclaimed wilds",
        "strata.place.contested" => "Contested ground",
        "strata.claim.concession" => "Concession of {faction}",
        "strata.claim.concession.leased" => "Concession of {faction} \u{b7} leased",
        "strata.claim.commons" => "Commons of {faction}",
        "strata.claim.reserve" => "Reserve of {faction}",
        "strata.claim.warn.concession" => "{faction} concession: work it under a lease",
        "strata.claim.warn.reserve" => "{faction} reserve: they will hear of it",
        "strata.claim.warn.commons" => "A commons: take a fair share",
        "strata.stance.welcoming" => "They know you well",
        "strata.stance.wary" => "They are wary of strangers",
        "strata.stance.hostile" => "They want you gone",
        "strata.place.kind.strata:town" => "Town site",
        "strata.place.kind.strata:hamlet" => "Hamlet",
        "strata.place.kind.strata:station" => "Monitoring station",
        "strata.place.kind.strata:landing" => "Landing",
        "strata.place.kind.strata:pass_shelter" => "Pass shelter",
        "strata.place.kind.strata:landmark" => "Landmark",
        "strata.place.kind.strata:wild_camp" => "Outlaw camp",
        "strata.place.kind.strata:wreck" => "Wreck",
        "strata.place.kind.strata:wreck_works" => "Derelict machine",
        _ => return None,
    })
}
// ---- end of the place card ----

// ---- the fitting screen (fitting.rs) ----
/// The words of the fitting screen: its headings, what a mount and a part
/// say of themselves, the two buttons and why either is grey, and the
/// sim's refusals as a player reads them. A phrase with a `{name}` in it is
/// filled by the screen from the weapon's and the item's own data.
fn fitting_english(key: &str) -> Option<&'static str> {
    Some(match key {
        "strata.fitting.sub" => "FITTING · {maker}",
        "strata.fitting.tab.in_hand" => "{weapon} · IN HAND",
        "strata.fitting.tab.holstered" => "{weapon} · HOLSTERED",
        "strata.fitting.condition" => "CONDITION",
        "strata.fitting.condition.value" => "{condition} of {full}",
        "strata.fitting.mounts" => "MOUNTS",
        "strata.fitting.mounts.open" => "{open} of {all} open",
        "strata.fitting.mount.standard" => "Standard",
        "strata.fitting.mount.standard.sub" => "The part it was built with",
        "strata.fitting.mount.shut" => "Shut",
        "strata.fitting.mount.shut.sub" | "strata.fitting.why.shut" => {
            "Opens at moddability {threshold}"
        }
        "strata.fitting.grade.store" => "Store grade",
        "strata.fitting.grade.batch" => "Batch {batch}",
        "strata.fitting.fits" => "FITS THE {mount}, FROM THE PACK AND THE STORE",
        "strata.fitting.fits.pack" => "FITS THE {mount}, FROM THE PACK",
        "strata.fitting.pick.standard" => "Standard {mount}",
        "strata.fitting.pick.fitted" | "strata.fitting.pick.now" => "Fitted now",
        "strata.fitting.pick.put_back" => "Remove puts it back",
        "strata.fitting.pick.carried" => "{count} carried",
        "strata.fitting.pick.stored" => "{count} in store",
        "strata.fitting.pick.quality" => "{line} {quality}",
        "strata.fitting.pick.none" => "No {mount} mod in the pack or the store.",
        "strata.fitting.pick.none.pack" => "No {mount} mod in the pack.",
        "strata.fitting.pick.picked" => "\u{203a} {name}",
        "strata.fitting.fit" => "Fit",
        "strata.fitting.fit.standard" => "Refit the standard part",
        "strata.fitting.remove" => "Remove",
        "strata.fitting.well.diagram" => "Diagram",
        "strata.fitting.well.model" => "Model",
        "strata.fitting.why.already" => "Already fitted",
        "strata.fitting.why.nothing" => "Nothing fitted here",
        "strata.fitting.why.pick" => "Pick a part to fit",
        "strata.fitting.why.no_weapon" | "strata.fitting.repair.empty" => "Nothing in this hand",
        "strata.fitting.table.now" => "NOW",
        "strata.fitting.table.after" => "AFTER",
        "strata.fitting.table.bare" => "BARE",
        "strata.fitting.table.in_hand" => "IN HAND",
        "strata.fitting.table.holstered" => "HOLSTERED",
        "strata.fitting.later" => "NOT IN THE FIGHT YET",
        "strata.fitting.moddability" => "MODDABILITY",
        "strata.fitting.moddability.opens" => "A mount opens at {thresholds}.",
        "strata.fitting.moddability.and" => " and ",
        "strata.fitting.moddability.all" => "Every mount is open.",
        "strata.fitting.status.bench" => {
            "At the workbench: pick a mount, then a part. Enter picks, Enter again fits."
        }
        "strata.fitting.status.cuts" => "At the workbench: a worn weapon is repaired here.",
        "strata.fitting.status.reading" => "A reading. Fitting is done at a workbench.",
        "strata.fitting.status.fitted" => "Fitted the {name}.",
        "strata.fitting.status.removed" => "Removed. The {mount} is standard again.",
        "strata.fitting.toast.fitted" => "Fitted. {line}.",
        "strata.fitting.tired" => "TIRED",
        "strata.fitting.worn.word" => "WORN",
        "strata.fitting.worn" => "Worn. Its lines are sliding until it is repaired.",
        "strata.fitting.repair" => "Repair",
        "strata.fitting.repair.plan" => "Repair \u{b7} 1 kit \u{b7} +{gain}",
        "strata.fitting.repair.plan_kitless" => "Repair \u{b7} no kit \u{b7} +{gain}",
        "strata.fitting.repair.nothing" => "Nothing to repair.",
        "strata.fitting.repair.no_kit" => "No repair kit carried or in the store.",
        "strata.fitting.toast.repaired" => "Repaired. Condition +{gain}.",
        "strata.fitting.rework" => "Rework",
        "strata.fitting.rework.buy" => "Buy blank \u{b7} {cost}",
        "strata.fitting.rework.plan" => {
            "Mount blank, temper {temper}. Most often to {often}, between {least} and {most}."
        }
        "strata.fitting.rework.needs" => "Needs {needs}.",
        "strata.fitting.rework.no_blank" => "No mount blank to hand.",
        "strata.fitting.rework.no_gain" => {
            "This blank is no harder than the mounts: temper {temper}, moddability {moddability}."
        }
        "strata.fitting.toast.reworked" => "Reworked: moddability {from} to {to}.",
        "strata.fitting.toast.reworked.opened" => " The {mount} mount is open.",
        "strata.fitting.empty" => "Nothing in this hand. A weapon is taken in hand at a locker.",
        "strata.fitting.refused.no_workbench" | "strata.fitting.why.no_workbench" => {
            "Fitting is done at a workbench"
        }
        "strata.fitting.refused.busy" => "Finish what you are doing first",
        "strata.fitting.refused.no_room" => "No room",
        "strata.fitting.refused.not_in_stock" => "That part is no longer in stock",
        "strata.fitting.refused.does_not_fit" => "It does not fit this weapon",
        "strata.fitting.refused.no_player" => "Nobody is holding a weapon",
        _ => return None,
    })
}
// ---- end of the fitting screen ----

// ---- figures in the game's units (card.rs, fitting.rs) ----
/// What a weapon's properties are called and how a figure of each reads:
/// the unit after a number, and for a property the fight has no number for
/// yet, the word for less of it and for more. A change for the worse says
/// so in a word, since its colour may not be seen.
fn figure_english(key: &str) -> Option<&'static str> {
    Some(match key {
        "strata.crafting.weapon.range" => "{low} to {high}",
        "strata.crafting.weapon.mounts" => "{figure} of {of} mounts open",
        "strata.crafting.weapon.shots" => "{figure} shots",
        "strata.crafting.weapon.cuts" => "{figure} cuts",
        "strata.crafting.weapon.row" => "{figure} between bursts",
        "strata.crafting.weapon.row.single" => "{figure} between shots",
        "strata.crafting.weapon.row.aim" => "{figure} aimed spread",
        "strata.crafting.weapon.row.cut" => "{figure} of the class's cut",
        "strata.weapon.fact.mode" => "Fire",
        "strata.weapon.fact.range" => "Falloff",
        "strata.weapon.fact.reach" => "Reach",
        "strata.weapon.fact.aim" => "Aim",
        "strata.weapon.aim.settles" => "Settles in {time} s",
        "strata.weapon.mode.single" => "Single shot",
        "strata.weapon.mode.burst" => "Burst of {count}",
        "strata.weapon.mode.auto" => "Full auto",
        "strata.weapon.mode.hold" => "{mode}, Hold for full auto",
        "strata.weapon.range.half" => "Full to {full} m, half at {far} m",
        "strata.weapon.range.share" => "Full to {full} m, {share}% at {far} m",
        "strata.weapon.reach" => "{range} m",
        "strata.property.capacity" => "Magazine",
        "strata.property.reload_time" => "Reload",
        "strata.property.repeat_interval" => "Shot or burst pause",
        "strata.property.repeat_interval.burst" => "Pause between bursts",
        "strata.property.repeat_interval.single" => "Time between shots",
        "strata.property.equip_time" => "Draw",
        "strata.property.aim_move_speed" => "Aimed pace",
        "strata.property.hit_damage" => "Damage",
        "strata.property.hit_posture" => "Posture damage",
        "strata.property.attack_stamina" => "Cut stamina",
        "strata.property.aim_spread" => "Aimed spread",
        "strata.property.recoil_recovery" => "Recoil",
        "strata.property.cooling_rate" => "Cooling",
        "strata.property.brace_time" => "Brace",
        "strata.property.recovery_time" => "Recovery",
        "strata.property.capacity.less" => "smaller",
        "strata.property.capacity.more" => "larger",
        "strata.property.aim_spread.less" => "tighter",
        "strata.property.aim_spread.more" => "looser",
        "strata.property.recoil_recovery.less"
        | "strata.property.brace_time.less"
        | "strata.property.recovery_time.less" => "quicker",
        "strata.property.recoil_recovery.more"
        | "strata.property.brace_time.more"
        | "strata.property.recovery_time.more"
        | "strata.property.cooling_rate.less" => "slower",
        "strata.property.cooling_rate.more" => "faster",
        "strata.figure.rounds" => "{n} rounds",
        "strata.figure.seconds" => "{n} s",
        "strata.figure.degrees" => "{n}\u{b0}",
        "strata.figure.percent" => "{n}%",
        "strata.figure.points" => "{n}",
        "strata.figure.metres" => "{n} m",
        "strata.cut.light_damage" => "Light cut damage",
        "strata.cut.light_time" => "Light cut time",
        "strata.cut.light_recovery" => "Light cut recovery",
        "strata.cut.light_stamina" => "Light cut stamina",
        "strata.cut.heavy_damage" => "Heavy cut damage",
        "strata.cut.heavy_posture" => "Heavy cut posture damage",
        "strata.cut.guard_share" => "Guard takes",
        "strata.cut.parry_window" => "Parry window",
        "strata.fitting.nomounts" => "No mounts. No parts fit this weapon yet.",
        "strata.figure.standard" => "Standard",
        "strata.figure.of_standard" => "{n}% {word}",
        "strata.figure.change.worse" => "{change} worse",
        "strata.figure.better" => "better",
        "strata.figure.worse" => "worse",
        "strata.figure.later" => "NOT IN THE FIGHT YET",
        _ => return None,
    })
}
// ---- end of the figures ----

// ---- the stores and the item card (stores.rs, card.rs) ----
/// The RIDE pane of the stores screens: the bike, its repair and its
/// saddle bags.
fn bike_english(key: &str) -> Option<&'static str> {
    Some(match key {
        "strata.stores.ride" => "RIDE",
        "strata.stores.ride.repair" => "Repair",
        "strata.stores.ride.put" => "Put away",
        "strata.stores.saddle" => "SADDLE BAGS",
        "strata.stores.saddle.shut" => "Mount up or stand at a locker",
        "strata.stores.take.ride" => "Take as ride",
        "strata.card.bike.road" => "Top speed, road",
        "strata.card.bike.ground" => "Top speed, off road",
        "strata.card.bike.turn" => "Turn",
        "strata.card.bike.cargo" => "Saddle bags",
        "strata.card.bike.kmh" => "{n} km/h",
        "strata.card.bike.turn.value" => "{slow} to {fast}\u{b0}/s",
        "strata.card.bike.turn.change" => "{n}\u{b0}/s",
        "strata.card.bike.cells" => "{n} cells ({columns} x {rows})",
        _ => return None,
    })
}

/// The words of the stores screen and of the card every item shows, apart
/// from [`english`] so the screen's catalogue reads in one place. A phrase
/// with a `{name}` in it is filled by the screen from the item's own data.
#[allow(
    clippy::too_many_lines,
    reason = "one table of the screen's words, which is how every layer here is kept"
)]
fn stores_english(key: &str) -> Option<&'static str> {
    Some(match key {
        "strata.stores.title" => "Stores",
        "strata.stores.title.pack" => "Carried",
        "strata.stores.sub" => "The household's locker",
        "strata.stores.sub.pack" => {
            "What {name} has on them. The household's store is at the locker."
        }
        "strata.stores.filter.all" => "ALL",
        "strata.stores.filter.lots" => "LOTS",
        "strata.stores.filter.parts" => "PARTS",
        "strata.stores.filter.mods" => "MODS",
        "strata.stores.filter.gear" => "GEAR",
        "strata.stores.filter.blueprints" => "BLUEPRINTS",
        "strata.stores.filter.kits" => "KITS",
        "strata.stores.store" => "THE HOUSEHOLD'S STORE",
        "strata.stores.pack" => "CARRIED",
        "strata.stores.filter.bags" | "strata.stores.bags" => "BAGS",
        "strata.stores.hands" | "strata.stores.hand.in_hand" => "IN HAND",
        "strata.stores.hand.holstered" => "HOLSTERED",
        "strata.stores.hand.empty" => "Nothing in this hand",
        "strata.stores.hand.condition" => "condition {condition}",
        "strata.stores.bag.empty" => "Empty bag slot",
        "strata.stores.bag.open" => "{name} · {cells} slots",
        "strata.stores.bag.holds" => "{name} · holds {stacks} · empty it to take it out",
        "strata.stores.send.pack" => "Send to carried",
        "strata.stores.send.store" => "Send to the store",
        "strata.stores.take" => "Take in hand",
        "strata.stores.take.hand" => "Take in hand {hand}",
        "strata.stores.take.hand.away" => "Swap out {weapon}",
        "strata.stores.take.done" => "{name} in hand.",
        "strata.stores.take.why" | "strata.stores.take.not_a_weapon" => "That is not a weapon.",
        "strata.stores.take.same_family" => "The other hand holds one of those.",
        "strata.stores.take.busy" => "Put down what is on the cursor first.",
        "strata.stores.take.not_held" => "That weapon is not to hand.",
        "strata.stores.take.mid_action" => "Finish what you are doing first.",
        "strata.stores.sends" => "Sends {name} x{count}",
        "strata.stores.carrying" => "Carrying: {name} x{count}",
        "strata.stores.hint" => "T or Shift click sends the stack across · right click takes half",
        "strata.stores.hint.pack" => {
            "T or Shift click sends between the pack and a bag · T on a weapon takes it in hand · right click takes half"
        }
        "strata.stores.hint.saddle" => {
            "T or Shift click sends between the pack and the saddle bags · right click takes half"
        }
        "strata.stores.hand.note" => "Click a hand, or Enter on it, opens its fitting.",
        "strata.stores.hand.note.pack" => "T takes it in hand, or carry it onto a hand.",
        "strata.stores.refused.bag" => "Holds {stacks}: empty the bag to take it out",
        "strata.stores.refused.hand" => "A hand takes a weapon. Carry one here, or press T on it.",
        "strata.stores.refused.not_a_bag" => "A bag slot takes a bag",
        "strata.stores.refused.no_locker" => "The household's store is reached at a locker",
        "strata.stores.refused.saddle_shut" => "The saddle bags are reached mounted or at a locker",
        "strata.stores.refused.no_room.pack" => "No room in the pack",
        "strata.stores.refused.no_room.bags" => "No room in a bag",
        "strata.stores.refused.no_room.saddle" => "No room in the saddle bags",
        "strata.stores.refused.no_room.store" => "No room in the store",
        "strata.stores.refused.no_dropping" => "A stack is put down in a slot, not dropped",
        "strata.card.empty" => "An empty slot. Pick a stack to read its card here.",
        "strata.card.kind.weapon" => "Weapon",
        "strata.card.kind.module" => "Mod",
        "strata.card.kind.bag" => "Bag",
        "strata.card.kind.lot" => "Lot",
        "strata.card.kind.component" => "Part",
        "strata.card.kind.commodity" => "Part · bought in",
        "strata.card.kind.kit" => "Repair kit",
        "strata.card.kind.tool" => "Survey tool",
        "strata.card.kind.vehicle" => "Bike",
        "strata.card.kit" => "Restores {points} points in the field, everything at a workbench.",
        "strata.card.kind.blueprint" => "Blueprint",
        "strata.card.from.store" => "Store grade · {maker}",
        "strata.card.from.crafted" => "Crafted by {by} · batch {batch}",
        "strata.card.from.unnamed" => "the household",
        "strata.card.from.stack" => "{count} in the stack",
        "strata.card.condition" => "CONDITION",
        "strata.card.condition.value" => "{condition} of {full}",
        "strata.card.fitted.none" => "Nothing fitted",
        "strata.card.fitted" => "Fitted: {names}",
        "strata.card.fits" => "Fits the {weapons}'s {mount}",
        "strata.card.fits.and" => " and the ",
        "strata.card.against.standard" => "Against the standard {mount} on the {weapon} {where}.",
        "strata.card.against.fitted" => "Against the {name} fitted now.",
        "strata.card.against.this" => "Fitted now. Against the standard {mount}.",
        "strata.card.against.shut" => "The {weapon} {where} has no open {mount} mount yet.",
        "strata.card.against.none" => "No weapon in hand takes it.",
        "strata.card.where.in_hand" => "in hand",
        "strata.card.where.holstered" => "holstered",
        "strata.card.mounts" => "{open} of {all} mounts open",
        "strata.card.bag" => "In a bag slot it opens {cells} more slots.",
        "strata.card.store_grade" => {
            "Every line of a bought item is {grade}, the tick on each bar."
        }
        "strata.card.shift" => "Hold Shift for the long card",
        "strata.card.commodity" => {
            "A certified part. It has no quality of its own and every one is the same."
        }
        "strata.card.goes_into" => "Goes into: {line} of {product}",
        "strata.card.goes_nowhere" => "Goes into nothing yet",
        "strata.card.made_from" => "Made from {list}.",
        "strata.card.made_from.batch" => "{name} batch {batch}",
        "strata.card.made_from.store" => "{name}, store grade",
        "strata.article" => "a",
        "strata.article.phrase" => "{article} {name}",
        _ => return None,
    })
}
// ---- end of the stores and the item card ----

// ---- lots, the register and the tracker (card.rs, bench.rs, tracker.rs, place.rs) ----
/// The words of a lot's card, of the survey
/// register on the terminal and of the marker that points at a tracked
/// deposit. A phrase with a `{name}` in it is filled by the screen from the
/// sim's own figures; a deposit's name is the world's and is no key.
fn survey_english(key: &str) -> Option<&'static str> {
    Some(match key {
        "strata.card.lot.from" => "{place} · {units}",
        "strata.card.lot.commodity" => "Commodity grade · {units}",
        "strata.card.lot.units" => "{units} units",
        "strata.card.lot.unit" => "1 unit",
        "strata.card.lot.of" => "of {floor} to {cap}",
        "strata.card.lot.more" => "{more} more with Shift",
        "strata.card.lot.long" => {
            "Sampled at {place}. The band under each bar is the range {type} can have anywhere on Veyra."
        }
        "strata.card.lot.long.commodity" => {
            "Certified stock: every line is {grade}, whatever the ground gives."
        }
        "strata.card.deposit.from" => "{place}",
        "strata.card.deposit.long" => {
            "What a lot sampled here reads. The band under each bar is the range {type} can have anywhere on Veyra."
        }
        "strata.purse.label" => "CREDITS",
        "strata.purse.credits" => "{credits} credits",
        "strata.purse.credit" => "1 credit",
        "strata.bench.tab.bench" => "BENCH",
        "strata.bench.tab.register" => "SURVEY REGISTER",
        "strata.register.title" => "The survey register",
        "strata.register.all" => "ALL",
        "strata.register.caption" => "NEAREST {rows} OF {total} WITHIN {range} KM",
        "strata.register.caption.all" => "ALL {total} WITHIN {range} KM",
        "strata.register.count" => "{shown} found · {visited} sampled",
        "strata.register.none" => {
            "Nothing found within {range} km yet. Open the survey tool with G and ping the ground."
        }
        "strata.register.none.class" => "No deposit of that class found within {range} km.",
        "strata.register.no_survey" => "No survey is published in this world.",
        "strata.register.kind" => "{class} · {type}",
        "strata.register.best" => "{code} {value}",
        "strata.register.best.band" => "{code} {band}",
        "strata.register.where.m" => "{distance} m {bearing}",
        "strata.register.where.km" => "{distance} km {bearing}",
        "strata.register.here" => "Here",
        "strata.register.tracked" => "TRACKED",
        "strata.register.sampled" => "Sampled",
        "strata.register.read" => "Read",
        "strata.register.type_only" => "Type only",
        "strata.register.keys" => {
            "Enter tracks the deposit in focus, or lets the tracked one go · a disc is a deposit you have sampled, a square one read by a ping, a ring one known by its type only"
        }
        "strata.register.card.none" => {
            "Put the focus on a row to read what the household knows of the deposit."
        }
        "strata.band.0" => "poor",
        "strata.band.1" => "fair",
        "strata.band.2" => "good",
        "strata.band.3" => "fine",
        "strata.card.deposit.unread" => "Not read",
        "strata.card.deposit.unread.long" => {
            "Not read. A ping told its type and how strong it is where you stood, and no more. Sample it to read its lines."
        }
        "strata.card.deposit.banded" => {
            "A ping read each line to a band of the range {type} can have anywhere on Veyra. Sample it to read the figures."
        }
        "strata.tracker.lead.m" => "Strongest reading · {distance} m",
        "strata.tracker.lead.km" => "Strongest reading · {distance} km",
        "strata.compass.north" => "north",
        "strata.compass.north_east" => "north-east",
        "strata.compass.east" => "east",
        "strata.compass.south_east" => "south-east",
        "strata.compass.south" => "south",
        "strata.compass.south_west" => "south-west",
        "strata.compass.west" => "west",
        "strata.compass.north_west" => "north-west",
        "strata.tracker.m" => "{place} · {distance} m",
        "strata.tracker.km" => "{place} · {distance} km",
        "strata.place.deposit" => "{class} · {type} deposit",
        _ => return None,
    })
}

/// A count with its thousands apart, as a purse reads: `2,500`.
#[must_use]
pub fn grouped(count: u32) -> String {
    let digits = count.to_string();
    let mut out = String::with_capacity(digits.len() + digits.len() / 3);
    for (index, digit) in digits.chars().enumerate() {
        if index > 0 && (digits.len() - index).is_multiple_of(3) {
            out.push(',');
        }
        out.push(digit);
    }
    out
}

/// A sum of credits as a sentence names it: `150 credits`, `1 credit`.
#[must_use]
pub fn credits(count: u32) -> String {
    if count == 1 {
        english("strata.purse.credit")
            .unwrap_or_default()
            .to_owned()
    } else {
        english("strata.purse.credits")
            .unwrap_or_default()
            .replace("{credits}", &grouped(count))
    }
}
// ---- end of lots, the register and the tracker ----

// ---- the survey tool (survey.rs) ----
/// The words of the survey screen, the HUD's survey bar and the toasts of a
/// ping or a sample that ended. A phrase with a `{name}` in it is filled by
/// the screen from the sim's own figures.
fn hand_survey_english(key: &str) -> Option<&'static str> {
    Some(match key {
        "strata.survey.title" => "Survey tool",
        "strata.survey.sub" => "PING THE GROUND · SAMPLE WHAT IT HOLDS",
        "strata.survey.ping" => "Ping",
        "strata.survey.sample" => "Sample here",
        "strata.survey.you" => "YOU",
        "strata.survey.cell" => "{pct}%",
        "strata.survey.edge.ahead" => "AHEAD",
        "strata.survey.edge.behind" => "BEHIND",
        "strata.survey.edge.left" => "LEFT",
        "strata.survey.edge.right" => "RIGHT",
        "strata.survey.stop" => "Stop",
        "strata.survey.stop_sampling" => "Stop sampling",
        "strata.survey.status.sampling" => "Sampling · {units} units",
        "strata.survey.skill" => {
            "{who}: reach {metres} m · {grid} by {grid} grid · {units} units a cycle · {reads}"
        }
        "strata.survey.skill.peak" => "also tells where the richest ground lies",
        "strata.survey.reads.type" => "reads the type only",
        "strata.survey.reads.bands" => "reads each line in bands",
        "strata.survey.reads.exact" => "reads each line exactly",
        "strata.survey.met.richest" => "Richest ground {pct}% · {where}",
        "strata.survey.span" => "{metres} m between readings",
        "strata.survey.back" => "pinged {metres} m back",
        "strata.survey.met.head" => "MET BY THE PING",
        "strata.survey.ranger" => "Ranger",
        "strata.survey.line.band" => "{code} {band}",
        "strata.survey.line.exact" => "{code} {value}",
        "strata.survey.met.row" => "{type} · strongest {pct}% · {where}",
        "strata.survey.known.type" => "Type only",
        "strata.survey.known.bands" => "Read in bands",
        "strata.survey.known.exact" => "Read exactly",
        "strata.survey.known.sampled" => "Sampled",
        "strata.survey.never" => "No ping yet. Pick a class and ping.",
        "strata.survey.nothing" => "The ping met no deposit of that class within {metres} m.",
        "strata.survey.here" => "Here the ground reads {pct}% of {place}.",
        "strata.survey.here.none" => "Nothing found reads here. Ping, then walk to a reading.",
        "strata.survey.keys" => "Enter on a class picks it · Enter on Ping pings · G or Esc closes",
        "strata.survey.why.no_player" => "Nobody is here to survey",
        "strata.survey.why.mounted" => "Not from the saddle",
        "strata.survey.why.no_tool" => "No survey tool in the pack",
        "strata.survey.why.in_water" => "Not from the water",
        "strata.survey.why.busy" => "Not in the middle of something else",
        "strata.survey.why.unknown_class" => "No such class of resource",
        "strata.survey.why.nothing" => "Ping first: nothing found reads here",
        "strata.survey.why.no_room" => "No room in the pack",
        "strata.survey.hud.pinging" => "Surveying",
        "strata.survey.hud.sampling_short" => "Sampling",
        "strata.survey.hud.lot" => "the lot",
        "strata.survey.hud.sampling" => "Sampling {lot} · {units} units",
        "strata.survey.notice.ping_moved" => "Ping lost: you moved",
        "strata.survey.notice.ping_stopped" => "Ping stopped",
        "strata.survey.notice.ping_no_tool" => "Ping lost: the survey tool is gone",
        "strata.survey.notice.sample.stopped" => "Sampling stopped",
        "strata.survey.notice.sample.moved" => "Sampling stopped: you moved off the spot",
        "strata.survey.notice.sample.hit" => "Sampling stopped: you were hit",
        "strata.survey.notice.sample.swimming" => "Sampling stopped: you are in the water",
        "strata.survey.notice.sample.no_room" => "Sampling stopped: no room in the pack",
        "strata.survey.notice.sample.no_tool" => "Sampling stopped: the survey tool is gone",
        _ => return None,
    })
}
// ---- end of the survey tool ----

// ---- the crafting session (crafting.rs, fitting.rs) ----
/// The words of the workbench's second tab: the schematics, a slot and
/// what the stock has for it, what comes out, the bench hand with its two
/// hands and their odds, and the sim's refusals as a player reads them. A
/// phrase with a `{name}` in it is filled by the screen from the sim's own
/// figures.
#[allow(
    clippy::too_many_lines,
    reason = "one table of the tab's words, which is how every layer here is kept"
)]
fn crafting_english(key: &str) -> Option<&'static str> {
    Some(match key {
        "strata.workbench.tab.fitting" => "FITTING",
        "strata.workbench.tab.crafting" => "CRAFTING",
        "strata.crafting.title" => "Workbench",
        "strata.crafting.worker" => "WORKED BY {name}",
        "strata.crafting.schematics" => "SCHEMATICS",
        "strata.crafting.legend" => "can be made now",
        "strata.crafting.group.gun_parts" => "GUN PARTS",
        "strata.crafting.group.blade_parts" => "BLADE PARTS",
        "strata.crafting.group.components" => "COMPONENTS",
        "strata.crafting.filter.known" => "Known",
        "strata.crafting.filter.ready" => "Ready",
        "strata.crafting.filter.all" => "All",
        "strata.crafting.none.known" => "Nothing taught yet. All shows what the trades make.",
        "strata.crafting.none.ready" => "Nothing can be made from what is in reach.",
        "strata.crafting.group.weapons" => "WEAPONS",
        "strata.crafting.group.tools" => "TOOLS AND VEHICLES",
        "strata.crafting.group.kits" => "KITS",
        "strata.crafting.schem.can" => "Can be made now",
        "strata.crafting.schem.cannot" => "Stock is short",
        "strata.crafting.schem.busy" | "strata.crafting.refused.session" => {
            "Finish what is on the bench first"
        }
        "strata.crafting.none" => "No schematics are known in this world.",
        "strata.crafting.wants.lot" => "any {kind} · {units} units",
        "strata.crafting.wants.one" => "one {name}",
        "strata.crafting.wants.many" => "{count} of {name}",
        "strata.crafting.slot.fit" => "{count} fit",
        "strata.crafting.slot.fits" => "1 fits",
        "strata.crafting.slot.close" => "Close",
        "strata.crafting.slot.empty" => "Nothing picked",
        "strata.crafting.slot.flat" => "Bought in, nothing to weigh",
        "strata.crafting.slot.none" => "Nothing in the pack or the store fits",
        "strata.crafting.slot.none.pack" => "Nothing in the pack fits",
        "strata.crafting.slot.short" => "Nothing in the pack or the store is enough",
        "strata.crafting.slot.short.pack" => "Nothing in the pack is enough",
        "strata.crafting.pick.best" => "BEST",
        "strata.crafting.pick.batch" => "batch {batch}",
        "strata.crafting.pick.lighter" => "lower is better",
        "strata.crafting.pick.odds" => "moves the odds",
        "strata.crafting.pick.short.units" => "{have} units, needs {needs}",
        "strata.crafting.pick.short.count" => "{have}, needs {needs}",
        "strata.crafting.pick.units" => "{units} u",
        "strata.crafting.buy" => "Buy 1 · {cost}",
        "strata.crafting.buy.short" => "Needs {needs}, you have {have}",
        "strata.crafting.out" => "WHAT COMES OUT",
        "strata.crafting.line.unfilled" => "Fill the slots to see this.",
        "strata.crafting.line.range" => "Assembles at {floor}, can be worked up to {ceiling}.",
        "strata.crafting.range" => "{low} to {high}",
        "strata.crafting.downstream" => "In {product}: {range}",
        "strata.crafting.uses" => "Uses {list}.",
        "strata.crafting.uses.units" => "{units} units of {name}",
        "strata.crafting.uses.count" => "{count} {name}",
        "strata.crafting.best" => "Best stock",
        "strata.crafting.assemble" => "Assemble",
        "strata.crafting.why.slot.empty" => "{slot}: nothing is picked.",
        "strata.crafting.why.slot.short" => "{slot}: {have} in stock, needs {needs}.",
        "strata.crafting.why.slot.wrong" => "{slot}: it does not take that.",
        "strata.crafting.refused.no_workbench" => "Step up to a workbench",
        "strata.crafting.refused.no_player" => "Nobody is at the bench",
        "strata.crafting.refused.unknown" => "The bench has no such schematic",
        "strata.crafting.refused.no_session" => "Nothing is on the bench",
        "strata.crafting.refused.no_points" | "strata.crafting.hand.spent" => "No points left.",
        "strata.crafting.refused.at_ceiling" => "That line is at its ceiling",
        "strata.crafting.refused.no_room" => "No room",
        "strata.crafting.refused.not_for_sale" => "That is not for sale here",
        "strata.crafting.went" => "WENT IN",
        "strata.crafting.went.units" => "{units} units",
        "strata.crafting.went.note" => {
            "The stock is spent at assembly. A setback costs quality, never the materials."
        }
        "strata.crafting.hand" => "THE BENCH HAND",
        "strata.crafting.points" => "{points} points left",
        "strata.crafting.point" => "1 point left",
        "strata.crafting.stands" => "{figure} of {ceiling}",
        "strata.crafting.stands.full" => "{figure} of {ceiling} · at its ceiling",
        "strata.crafting.steady" => "Steady",
        "strata.crafting.bold" => "Bold",
        "strata.crafting.steady.offer" => "most often {gain}",
        "strata.crafting.bold.offer" => "up to {gain}",
        "strata.crafting.bold.offer.slip" => "up to {gain}, can slip {slip}",
        "strata.crafting.outcome.exceptional" => "Exceptional",
        "strata.crafting.outcome.good" => "Good",
        "strata.crafting.outcome.fair" => "Fair",
        "strata.crafting.outcome.poor" => "Poor",
        "strata.crafting.outcome.setback" => "Setback",
        "strata.crafting.word" => "{outcome} {moved}",
        "strata.crafting.odds.note" => {
            "Bars read left to right: exceptional, good, fair, poor, setback."
        }
        "strata.crafting.odds.workable" => "Workable stock. The odds are a row better.",
        "strata.crafting.finish" => "Finish",
        "strata.crafting.again" => "Make another",
        "strata.crafting.status.bench" => {
            "At the workbench: pick a schematic, fill its slots, assemble. {prev} and {next} change the tab."
        }
        "strata.crafting.status.hand" => {
            "Spend the points a line at a time. Hold a hand to keep rolling. Finish makes it."
        }
        "strata.crafting.status.reading" => "A reading. Things are made at a workbench.",
        "strata.crafting.status.assembled" => "Assembled. {points} to spend.",
        "strata.crafting.status.rolled" => "{outcome}. {line} {from} to {to}.",
        "strata.crafting.status.finished" => "{name} batch {batch} is in your pack.",
        "strata.crafting.status.finished.store" => "{name} batch {batch} is in the store.",
        "strata.crafting.status.bought" => "Bought {name}. {left} left.",
        "strata.crafting.points.count" => "{points} points",
        "strata.crafting.points.one" => "1 point",
        _ => return None,
    })
}
// ---- end of the crafting session ----

// ---- blueprints (crafting.rs, card.rs, stores.rs) ----
/// The words of recording a blueprint at the workbench: the box that opens
/// on the finished card, what the bench says when it is done, the sim's
/// refusals as a player reads them, and a blueprint's own card. Apart from
/// the session's so each catalogue stays a readable length.
fn blueprint_english(key: &str) -> Option<&'static str> {
    Some(match key {
        "strata.crafting.status.recorded" => "Recorded as blueprint {blueprint}.",
        "strata.crafting.refused.no_blueprints" => "Nothing is recorded as a blueprint here",
        "strata.crafting.refused.no_prototype" => "It is not in your pack or the store",
        "strata.crafting.refused.no_ledger" => "Nobody remembers what went into it",
        "strata.crafting.refused.run_off" => {
            "Run off blueprint {blueprint}: a copy is no prototype"
        }
        "strata.crafting.refused.not_taught" => "Needs {tree}: {box}.",
        "strata.crafting.learn_here" => {
            "It can be learned at this bench: close this screen and press C."
        }
        "strata.crafting.refused.recorded" => "Recorded already as blueprint {blueprint}",
        "strata.crafting.refused.derate" => "A blueprint is not recorded at that",
        "strata.crafting.record.open" => "Record as blueprint",
        "strata.crafting.record.at" => "RECORD AT",
        "strata.crafting.record.share" => "{percent}% of this one",
        "strata.crafting.record.makes" => "MAKES",
        "strata.crafting.record.note" => "A run will take any stock that meets these:",
        "strata.crafting.record.least" => "{stat} at least {n}",
        "strata.crafting.record.most" => "{stat} at most {n}",
        "strata.crafting.record.runs" => "RUNS",
        "strata.crafting.record.go" => "Record",
        "strata.crafting.record.done" => "Blueprint recorded at {percent}%.",
        "strata.crafting.record.gone" => "It has left your pack and the store",
        "strata.crafting.earlier" => "MADE EARLIER",
        "strata.crafting.earlier.row" => "{name} · batch {batch} · {lead}",
        "strata.card.blueprint.name" => "{product} blueprint",
        "strata.card.blueprint.from" => "Recorded by {by} · blueprint {id}",
        "strata.card.blueprint.runs" => "{left} of {runs} runs left",
        "strata.card.blueprint.makes" => "Makes {names}, every one the same.",
        "strata.card.blueprint.slot.units" => "{slot}, {units} units: {needs}",
        "strata.card.blueprint.slot.count" => "{slot}, {count}: {needs}",
        "strata.card.blueprint.any" => "any will do",
        "strata.card.blueprint.recorded" => "Recorded at {percent}% of batch {batch}.",
        "strata.card.blueprint.lost" => "Nobody remembers what it records.",
        "strata.card.from.run" => "Run off blueprint {blueprint} · batch {batch}",
        _ => return None,
    })
}
// ---- end of blueprints ----

/// The field harvester: the survey screen's Plant button, its own screen,
/// the chip, the toasts and the register's holding. A `{name}` is filled by
/// the screen from the sim's own figures.
fn harvester_english(key: &str) -> Option<&'static str> {
    Some(match key {
        "strata.harvester.plant" => "Plant harvester here",
        "strata.harvester.plant.ok" => "A field harvester can stand here, on {place} at {pct}%.",
        "strata.harvester.why.no_player" => "Nobody is out to plant it",
        "strata.harvester.why.mounted" => "Get off the bike to plant it",
        "strata.harvester.why.in_water" => "It cannot be planted in the water",
        "strata.harvester.why.no_harvester" => "No field harvester is carried",
        "strata.harvester.why.not_taught" => "No box you know teaches planting",
        "strata.harvester.why.at_limit" => "{count} planted already, as many as you may have",
        "strata.harvester.why.nothing_here" => {
            "No found deposit reads strongly enough here to plant on"
        }
        "strata.harvester.why.too_near" => "Another field harvester stands too near",
        "strata.harvester.why.not_in_reach" => "Out of reach of the harvester",
        "strata.harvester.why.empty" => "It holds nothing",
        "strata.harvester.why.not_empty" => "Collect what it holds first",
        "strata.harvester.why.no_room" => "No room in the pack",
        "strata.harvester.toast.planted" => "Planted a field harvester on {place}",
        "strata.harvester.toast.collected" => "Collected {n} units from the field harvester",
        "strata.harvester.toast.collected.left" => {
            "Collected {n} units; {left} are left in the hopper"
        }
        "strata.harvester.toast.picked_up" => "Picked up the field harvester",
        "strata.harvester.toast.full" => "A field harvester on {place} is full",
        "strata.harvester.title" => "Field harvester",
        "strata.harvester.on" => "{type} \u{b7} {place}",
        "strata.harvester.reads" => "The ground reads {pct}% here",
        "strata.harvester.rate" => "Makes {n} units an hour",
        "strata.harvester.time" => "Full in {time}",
        "strata.harvester.time.full" => "Full",
        "strata.harvester.time.none" => "Makes nothing here",
        "strata.harvester.hours" => "{h} h {m} min",
        "strata.harvester.minutes" => "{m} min",
        "strata.harvester.hopper" => "Hopper",
        "strata.harvester.held" => "{held} of {hopper}",
        "strata.harvester.by" => "Planted by {name}",
        "strata.harvester.by.nobody" => "Planter not recorded",
        "strata.harvester.collect" => "Collect",
        "strata.harvester.collect.n" => "Collect {n}",
        "strata.harvester.pick_up" => "Pick up",
        "strata.harvester.track" => "Track",
        "strata.harvester.chip" => "Field harvester \u{b7} {held} of {hopper}",
        "strata.harvester.chip.full" => "Field harvester \u{b7} {held} of {hopper}, full",
        "strata.harvester.register" => "Harvester {held} of {hopper}",
        "strata.harvester.register.n" => "{count} harvesters {held} of {hopper}",
        "strata.harvester.register.full" => ", full",
        _ => return None,
    })
}

// ---- the procurement board (board.rs) ----
/// The words of a buyer's board: the lines it wants, the household's side
/// of each, the ledger and the sim's refusals as a player reads them. A
/// phrase with a `{name}` in it is filled by the screen from the sim's own
/// figures.
fn board_english(key: &str) -> Option<&'static str> {
    Some(match key {
        "strata.board.title" => "{buyer} \u{b7} procurement",
        "strata.board.project" => "PROJECT \u{b7} {name}",
        "strata.board.wants.more" => "wants {n} more",
        "strata.board.wants.units" => "wants {n} units",
        "strata.board.min" => "min {grade} {min}",
        "strata.board.price.each" => "{n} cr each",
        "strata.board.price.unit" => "{n} cr a unit",
        "strata.board.filled" => "filled",
        "strata.board.refills" => "refills with time",
        "strata.board.yours" => "yours: {held} at {grade} \u{b7} pays {each} each",
        "strata.board.yours.lot" => "yours: {name} {grade}, {units} units",
        "strata.board.pays" => "pays {each} a unit",
        "strata.board.more" => "and {n} more",
        "strata.board.short" => "not met: {has} \u{b7} needs {needs}",
        "strata.board.none" => "none held",
        "strata.board.sell" => "Sell",
        "strata.board.sell.n" => "Sell {n} \u{b7} {paid} cr",
        "strata.board.why.short" => "not met: {has}",
        "strata.board.why.under" => "not met: {has}, needs {needs}",
        "strata.board.why.reach" => "Out of reach of the board",
        "strata.board.why.unknown" => "The buyer has no such line",
        "strata.board.why.unwanted" => "The buyer does not take that",
        "strata.board.sold" => "SOLD HERE",
        "strata.board.sold.none" => "Nothing sold here yet.",
        "strata.board.sold.batch" => "batch {batch}",
        "strata.board.sold.store" => "store grade",
        "strata.board.cr" => "{n} cr",
        "strata.board.units.of" => "{n} units of {name}",
        "strata.board.sale" => "FOR SALE",
        "strata.board.buy" => "Buy",
        "strata.board.purse" => "purse {n} cr",
        "strata.board.why.unsold" => "The buyer does not sell that",
        "strata.board.why.broke" => "Short by {short} cr \u{b7} costs {needs} cr",
        "strata.board.why.hostile" => "They will not deal with you",
        "strata.board.lease" => "LEASE \u{b7} {faction} concessions",
        "strata.board.lease.button" => "Lease",
        "strata.board.lease.renew" => "Extend",
        "strata.board.lease.price" => "{n} cr for {hours} hours",
        "strata.board.lease.left" => "Leased \u{b7} {left} left",
        "strata.board.lease.time" => "{h} h {m} min",
        "strata.board.why.lease" => "Nothing to lease here",
        "strata.board.toast.leased" => "Leased {faction} concessions. -{paid} cr.",
        "strata.board.why.room" => "No room in the pack or the store",
        "strata.board.toast.bought" => "Bought {what} from {buyer}. -{paid} cr.",
        "strata.board.work" => "WORK",
        "strata.board.work.none" => "No work posted just now.",
        "strata.board.work.head" => "Clear {camp}",
        "strata.board.work.head.strata:wreck" => "Recover the cargo at {camp}",
        "strata.board.work.head.strata:wreck_works" => "Secure {camp}",
        "strata.board.work.unheld.strata:wreck" | "strata.board.work.unheld.strata:wreck_works" => {
            "unclaimed salvage"
        }
        "strata.board.work.take" => "Take",
        "strata.board.work.collect" => "Collect",
        "strata.board.work.taken" => "Taken \u{b7} heard of {lies}",
        "strata.board.work.cleared" => "Cleared \u{b7} collect the pay",
        "strata.board.work.who" => "held by {band}",
        "strata.board.why.work.offered" => "That work is no longer posted",
        "strata.board.why.work.many" => "You hold as much work as you can",
        "strata.board.why.work.undone" => "Not done yet",
        "strata.board.why.work.taken" => "No such work is taken here",
        "strata.board.toast.work.taken" => {
            "Taken: clear {camp}. It is marked on the map as heard of."
        }
        "strata.board.toast.work.taken.strata:wreck" => {
            "Taken: recover the cargo at {camp}. It is marked on the map as heard of."
        }
        "strata.board.toast.work.taken.strata:wreck_works" => {
            "Taken: secure {camp}. It is marked on the map as heard of."
        }
        "strata.board.toast.work.paid" => "{buyer} pays {paid} cr for {camp}.",
        "strata.board.toast.sold" => "Sold {what} to {buyer}. +{paid} cr.",
        _ => return map_english(key),
    })
}
// ---- end of the procurement board ----

// ---- the factory (factory.rs, bench.rs) ----
/// The words of the fabricator's screen: the blueprints, a slot with what
/// it needs and what the stock has for it, the batch and what holds it,
/// the run, the sim's refusals as a player reads them, and what the run
/// says of itself wherever the player is. A phrase with a `{name}` in it
/// is filled by the screen from the sim's own figures.
fn factory_english(key: &str) -> Option<&'static str> {
    Some(match key {
        "strata.factory.title" => "Fabricator",
        "strata.factory.worker" => "WORKED BY {name}",
        "strata.factory.speed.full" => "full speed",
        "strata.factory.speed.half" => "half speed",
        "strata.factory.speed.share" => "{percent}% speed",
        "strata.factory.blueprints" => "BLUEPRINTS",
        "strata.factory.none" => {
            "No blueprint in your pack or the store. One is recorded at the workbench, from a thing just finished."
        }
        "strata.factory.none.pack" => "No blueprint in your pack. The store is out of reach here.",
        "strata.factory.runs" => "{left} runs left",
        "strata.factory.runs.one" => "1 run left",
        "strata.factory.head" => "{name} · BLUEPRINT {id}",
        "strata.factory.head.run" => "{name} · BLUEPRINT {id} · BATCH {batch}",
        "strata.factory.batch" => "BATCH OF",
        "strata.factory.batch.most" => "Most",
        "strata.factory.batch.runs" => "Up to {most} · {runs}",
        "strata.factory.batch.stock" => "Up to {most} · the {slot} in stock feeds {feeds}",
        "strata.factory.batch.none" => "The {slot} in stock does not feed one",
        "strata.factory.slot.units" => "{units} units each",
        "strata.factory.slot.count" => "{count} each",
        "strata.factory.needs" => "needs {list}",
        "strata.factory.needs.none" => "takes any",
        "strata.factory.need.least" => "{stat} {n}",
        "strata.factory.need.most" => "{stat} at most {n}",
        "strata.factory.feed.meets" => "MEETS",
        "strata.factory.feed.feeds" => "FEEDS",
        "strata.factory.feed.under" => "{stat} {has}, needs {needs}",
        "strata.factory.feed.over" => "{stat} {has}, at most {most}",
        "strata.factory.feed.wrong" => "Not what the slot takes",
        "strata.factory.feed.pack" => "{n} in pack",
        "strata.factory.feed.store" => "{n} in store",
        "strata.factory.slot.none" => "Nothing in stock fits",
        "strata.factory.slot.none.pack" => "Nothing in your pack fits",
        "strata.factory.slot.unmet" => "Nothing in stock meets it",
        "strata.factory.cheapest" => "Cheapest stock",
        "strata.factory.makes" => "Makes {count} {names} at {lead} · ready in {time}",
        "strata.factory.time.seconds" => "{n} s",
        "strata.factory.time.minutes" => "{n} min",
        "strata.factory.workers" => "WORKED BY",
        "strata.factory.worker.none" => "Nobody · {speed}",
        "strata.factory.worker.one" => "{name} · {speed}",
        "strata.factory.worker.empty" => "Nobody is on the bench to work it.",
        "strata.factory.run" => "Run",
        "strata.factory.stop" => "Stop",
        "strata.factory.made.head" => "MADE",
        "strata.factory.made" => "{made} of {size}",
        "strata.factory.left" => "Ready in {time}",
        "strata.factory.paused" => "PAUSED · no room in the store",
        "strata.factory.hopper" => "IN THE HOPPER",
        "strata.factory.hopper.units" => "{units} units",
        "strata.factory.hopper.empty" => "Nothing is left in it.",
        "strata.factory.status.idle" => {
            "Feed a blueprint's slots and run it. Every unit comes out the same, and better stock gains nothing."
        }
        "strata.factory.status.reading" => "A reading. A blueprint is run at a fabricator.",
        "strata.factory.status.running" => {
            "The run goes on while you are away. Stop keeps what is made and gives back the rest."
        }
        "strata.factory.status.started" => "Batch {batch} is running · ready in {time}.",
        "strata.factory.status.stopped" => {
            "Stopped at {made}. What it had not used is back in the store."
        }
        "strata.factory.status.assigned" => "{name} is working the run.",
        "strata.factory.status.unassigned" => "Nobody is working the run.",
        "strata.factory.refused.no_player" => "Nobody is at the fabricator",
        "strata.factory.refused.no_fabricator" => "Step up to a fabricator",
        "strata.factory.refused.busy" => "The fabricator is running",
        "strata.factory.refused.no_blueprint" => "That blueprint is not in your pack or the store",
        "strata.factory.refused.stale" => "The blueprint no longer fits what it makes",
        "strata.factory.refused.no_batch" => "A batch is one or more",
        "strata.factory.refused.over_runs" => "The blueprint has {runs}",
        "strata.factory.refused.slot" => "{slot}: {why}",
        "strata.factory.refused.slot.empty" => "nothing feeds it",
        "strata.factory.refused.slot.wrong" => "it does not take that",
        "strata.factory.refused.slot.short" => "{have} in stock, the batch takes {needs}",
        "strata.factory.refused.no_job" => "The fabricator is not running",
        "strata.factory.refused.no_room" => "No room in the store for what the run holds",
        "strata.factory.refused.not_on_bench" => "Only somebody on the bench works a run",
        "strata.factory.toast.finished" => "{count} {names} are in the store · batch {batch}",
        "strata.factory.toast.finished.one" => "1 {name} is in the store · batch {batch}",
        "strata.factory.toast.spent" => " · the blueprint is spent",
        "strata.factory.toast.paused" => "The fabricator has stopped: the store is full",
        "strata.factory.toast.resumed" => "The fabricator is running again",
        "strata.bench.work" => "AT THE FABRICATOR · {made} OF {size}",
        "strata.bench.work.why" => "Working until the run ends or is stopped.",
        // The game's answer to a switch asked for all the same; it fills
        // the names in and the bench toasts it.
        "strata.bench.refused.working" => {
            "{name} is working the fabricator: {made} of {size} made. Stop the run to switch."
        }
        _ => return None,
    })
}

/// A cleared camp's cache and a fall in the world (ticket 0012, step 4a):
/// the chip, the toasts and the bench card's line, filled by `ui::loot`.
fn loot_english(key: &str) -> Option<&'static str> {
    Some(match key {
        "strata.cache.name.strata:wild_camp" => "Outlaw cache",
        "strata.cache.name.strata:wreck" => "Salvage",
        "strata.cache.name.strata:wreck_works" => "Machine parts",
        "strata.prompt.cache" => "Open the cache",
        "strata.prompt.person" => "Talk",
        "strata.cache.toast" => "Cache: {what}",
        "strata.cache.left" => "The pack is full: {n} stacks stay in the cache.",
        "strata.cache.why.player" => "Nobody is there to take it",
        "strata.cache.why.gone" => "The cache is empty",
        "strata.cache.why.reach" => "Out of reach of the cache",
        "strata.cache.why.room" => "No room in the pack",
        "strata.fall.home" => "Carried home.",
        "strata.fall.out" => "{name} is out of it for {time}; {other} is out.",
        "strata.fall.early" => {
            "{name} is out of it for {time}; nobody else is able, so {other} got up early."
        }
        "strata.fall.alone" => "{name} is back at the bench, with everything carried.",
        "strata.fall.hour.one" => "1 hour",
        "strata.fall.hour.many" => "{n} hours",
        "strata.fall.minute.one" => "1 minute",
        "strata.fall.minute.many" => "{n} minutes",
        "strata.fall.short.h" => "{n} h",
        "strata.fall.short.m" => "{n} min",
        "strata.bench.injured" => "Injured: back in {time}",
        "strata.bench.refused.injured" => "{name} is injured: back in {time}.",
        "strata.bench.injured.why" => "Hurt in a fall. Everything they carry is with them.",
        _ => return None,
    })
}

/// A thing's name for `count` of it, as a sentence counts them: `cell
/// cores`, `1 cell core`'s `cell core`. The English rule only, which is
/// what this catalogue is: a translation words its own.
#[must_use]
pub fn counted(name: &str, count: u32) -> String {
    if count == 1 {
        name.to_owned()
    } else if ["s", "x", "ch", "sh"].iter().any(|end| name.ends_with(end)) {
        format!("{name}es")
    } else {
        format!("{name}s")
    }
}
// ---- end of the factory ----

// ---- the audio tab (settings.rs, combat_audio.rs) ----
/// The words of the Audio tab, apart from [`english`] so the two volumes
/// and the note that tells them apart read together.
fn audio_english(key: &str) -> Option<&'static str> {
    Some(match key {
        "strata.settings.ui_volume" => "Interface volume",
        "strata.settings.audio_note" => {
            "The master volume covers the fight and the interface; the interface volume scales the screens' own sounds under it."
        }
        _ => return None,
    })
}
// ---- end of the audio tab ----

// ---- the UI scale setting (settings.rs, scale.rs) ----
/// The words of the Display tab's UI scale row, apart from [`english`] so
/// the setting's label and its choices read together.
fn scale_english(key: &str) -> Option<&'static str> {
    Some(match key {
        "strata.settings.ui_scale" => "Interface scale",
        "strata.settings.ui_scale.auto" => "Auto",
        "strata.settings.ui_scale.100" => "100 %",
        "strata.settings.ui_scale.125" => "125 %",
        "strata.settings.ui_scale.150" => "150 %",
        "strata.settings.ui_scale.200" => "200 %",
        _ => return None,
    })
}
// ---- end of the UI scale setting ----

impl Localizer for StrataStrings {
    fn resolve(&self, key: &LocKey, _args: &LocArgs) -> Option<String> {
        english(&key.0).map(str::to_owned)
    }
}

/// `Startup`: pushes the fallback.
pub fn install(mut localization: ResMut<Localization>) {
    localization.push_fallback(StrataStrings);
}

// ---- the questline's screens (dialogue.rs, journal.rs) ----

/// The words of the conversation and the journal. The quest's own lines are
/// data; these are the screen's.
fn quest_english(key: &str) -> Option<&'static str> {
    Some(match key {
        "strata.dialogue.why.not_yet" => "Not yet.",
        "strata.dialogue.why.done" => "That is already settled.",
        "strata.dialogue.why.credits" => "You need {n} credits.",
        "strata.dialogue.why.holds" => "You are not carrying what that needs.",
        "strata.dialogue.why.learned" => "You have not learned that yet.",
        "strata.dialogue.why.room" => "There is no room to carry it.",
        "strata.dialogue.why.made" => "It needs something made at a bench, in your pack.",
        "strata.dialogue.keys" => "1 to 4 choose",
        "strata.dialogue.leave" => "Leave",
        "strata.menu.journal" | "strata.journal.title" => "Journal",
        "strata.journal.empty" => "Nothing in your journal yet.",
        "strata.journal.objectives" => "WHAT IS ASKED",
        "strata.journal.choices" => "WHAT YOU DECIDED",
        "strata.journal.tick.done" => "[x]",
        "strata.journal.tick.open" => "[ ]",
        "strata.journal.place.quay" => "the quay",
        "strata.journal.place.exchange" => "the Exchange",
        "strata.journal.place.board" => "the board",
        "strata.journal.place.station" => "the monitoring station",
        "strata.journal.place.channel" => "Wim's channel",
        "strata.journal.place.training" => "the training ground",
        "strata.journal.foe.sentry" => "the training sentry",
        "strata.journal.foe.rusher" => "a rusher",
        "strata.journal.foe.marksman" => "a marksman",
        "strata.journal.foe.duelist" => "a duelist",
        "strata.journal.foe.bulwark" => "a bulwark",
        "strata.journal.talk" => "Talk to {name}",
        "strata.journal.reach" => "Go to {place}",
        "strata.journal.beat" => "Beat {name}",
        "strata.journal.beat_crew" => "Beat {name}'s crew",
        "strata.journal.learn" => "Learn a box from a trainer",
        "strata.journal.ping" => "Ping the ground",
        "strata.journal.sample" => "Sample {class}: {have} of {units}",
        "strata.journal.craft" => "Make a {item} at a workbench",
        "strata.journal.sell" => "Sell the {item} at the board",
        "strata.journal.buy" => "Buy a {item} at the board",
        "strata.journal.stable" => "Take the bike as your ride",
        "strata.journal.recruit" => "Take {name} onto the bench",
        "strata.journal.switch" => "Switch to {name} at the terminal",
        "strata.journal.deliver" => "Fit the {item} to the {name}",
        "strata.map.panel.quest" => "Objective",
        "strata.map.panel.quest.kind" => "Where your journal points",
        _ => return None,
    })
}

// ---- the map (map.rs) ----

/// The words of the map screen.
fn map_english(key: &str) -> Option<&'static str> {
    Some(match key {
        "strata.map.title" | "strata.menu.map" | "strata.controls.map" => "Map",
        "strata.controls.key.map" => "M",
        "strata.map.sub" => "North is up",
        "strata.map.keys" => {
            "Arrows move between marks · Enter tracks a found deposit, or lets the tracked one go · M or Esc closes"
        }
        "strata.map.scale.near" => "Near · 3 km",
        "strata.map.scale.region" => "Region · 12 km",
        "strata.map.scale.far" => "Far · 48 km",
        "strata.map.charting" => "Charting",
        "strata.map.north" => "N",
        "strata.map.legend" => "LEGEND",
        "strata.map.legend.you" => "The arrow is you, facing that way",
        "strata.map.legend.site" => "Site or wreck",
        "strata.map.legend.machine" => "Derelict machine",
        "strata.map.legend.camp" => "Outlaw camp",
        "strata.map.legend.deposit" => "Found deposit and its area, ringed when tracked",
        "strata.map.legend.lead" => "Strongest reading of the last ping",
        "strata.map.legend.service" => "Service road",
        "strata.map.legend.track" => "Track",
        "strata.map.legend.trail" => "Trail",
        "strata.map.legend.water" => "River",
        "strata.map.panel.none" => "Point at a mark, or move the focus onto one.",
        "strata.map.legend.settlement" | "strata.map.panel.settlement" => "Settlement",
        "strata.map.legend.harvester" | "strata.map.panel.harvester" => "Harvester",
        "strata.map.panel.harvester.on" => "On {place}",
        "strata.map.legend.bench" | "strata.map.panel.bench" => "Workbench",
        "strata.map.panel.bench.kind" => "The household's bench place",
        "strata.map.panel.lead" => "Strongest reading",
        "strata.map.panel.lead.kind" => "Where the last ping read most",
        "strata.map.panel.lead.best" => "{pct}% of {place}",
        "strata.map.label" => "{place} · {reading}",
        "strata.map.panel.track" => "Enter tracks it",
        "strata.map.panel.untrack" => "Tracked. Enter lets it go",
        "strata.map.panel.off" => "{lies}, past the edge of the map",
        "strata.map.holders" => "Holders",
        "strata.map.legend.holders.none" => "No one's ground in view",
        "strata.map.panel.held" => "Held by {faction}",
        "strata.map.panel.cleared" => "Cleared",
        "strata.map.legend.heard" => "Heard of: somewhere in this area",
        "strata.map.heard.label" => "{camp} \u{b7} heard of",
        "strata.map.heard.kind" => "{kind} \u{b7} heard of",
        "strata.map.heard.job" => "Taken work: {pay} cr",
        "strata.map.heard.unfound" => "Its place is not found yet",
        _ => return None,
    })
}
// ---- end of the map ----

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_purse_reads_with_its_thousands_apart() {
        assert_eq!(grouped(0), "0");
        assert_eq!(grouped(80), "80");
        assert_eq!(grouped(2500), "2,500");
        assert_eq!(grouped(1_234_567), "1,234,567");
        assert_eq!(credits(1), "1 credit");
        assert_eq!(credits(2350), "2,350 credits");
    }

    #[test]
    fn every_settings_label_has_english() {
        let spec = crate::ui::settings::spec();
        for tab in &spec.tabs {
            assert!(english(&tab.label.0).is_some(), "{}", tab.label.0);
            for row in &tab.rows {
                if let slotted::menu::SettingsRow::Select { options, .. }
                | slotted::menu::SettingsRow::Radio { options, .. } = row
                {
                    for option in options {
                        assert!(english(&option.label.0).is_some(), "{}", option.label.0);
                    }
                }
                let label = match row {
                    slotted::menu::SettingsRow::Toggle { label, .. }
                    | slotted::menu::SettingsRow::Slider { label, .. }
                    | slotted::menu::SettingsRow::Select { label, .. }
                    | slotted::menu::SettingsRow::Radio { label, .. }
                    | slotted::menu::SettingsRow::Text { label, .. }
                    | slotted::menu::SettingsRow::Heading(label) => label.0.as_str(),
                    _ => continue,
                };
                assert!(english(label).is_some(), "{label}");
            }
        }
    }
}
