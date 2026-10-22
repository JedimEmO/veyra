//! `balance worth`: the page that says what each weapon and each ability is
//! worth, made from the harness alone. Every table is bouts of the presets
//! the page names, or a figure read from the combat tables, and the one
//! sentence under each table is written here from those figures by a rule
//! stated in the sentence's function, so the page can be made again after any
//! change and no word of it is typed by hand but the headings.
//!
//! The page holds no verdict a band does: the contracts gate the numbers,
//! this page shows them side by side.

use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::path::Path;

use serde::Deserialize;
use veyra_rules::combat::{Action, Weapon};
use veyra_rules::combat_ai::OpponentKind;
use veyra_rules::combat_tables;

use crate::bout::BoutResult;
use crate::contracts::{Metric, SideMetric};
use crate::{WORTH_WEAPONS, ability_cases, preset, run_matchup};

/// Where the tuning log is kept, under the assets directory.
pub const TUNING_FILE: &str = "balance/combat_tuning.ron";

/// The change, in percent, over which an ability is marked strong.
const STRONG_OVER: f64 = 40.;
/// The change from which an ability is marked as paying its place.
const PAYS_FROM: f64 = 10.;
/// The change from which an ability is marked slight, and under which it is
/// dead (the contracts' own floor of 1.03).
const SLIGHT_FROM: f64 = 3.;
/// How near the best gun a melee class must come, in seconds and in damage
/// taken, to be called within reach of it.
const WITHIN: f64 = 1.5;
/// The win share from which a weapon counts as winning an opponent's fight
/// for the marks and the sentences.
const WINNING: f64 = 0.5;

/// The tuning log: `assets/balance/combat_tuning.ron`.
#[derive(Debug, Clone, Deserialize)]
pub struct Tuning {
    /// Bumped when the shape changes.
    pub version: u32,
    /// The dials moved, in the order they were.
    pub changes: Vec<Change>,
}

/// One dial moved.
#[derive(Debug, Clone, Deserialize)]
pub struct Change {
    /// The file and path of the dial.
    pub dial: String,
    /// The figure as it was.
    pub from: String,
    /// The figure as it is.
    pub to: String,
    /// Why.
    pub why: String,
}

impl Tuning {
    /// Read the log under `assets`.
    ///
    /// # Errors
    /// The file cannot be read or parsed.
    pub fn load(assets: &Path) -> anyhow::Result<Self> {
        let path = assets.join(TUNING_FILE);
        let text = std::fs::read_to_string(&path)
            .map_err(|error| anyhow::anyhow!("{}: {error}", path.display()))?;
        ron::from_str(&text).map_err(|error| anyhow::anyhow!("{}: {error}", path.display()))
    }
}

const MELEE: [Weapon; 3] = [Weapon::Blade, Weapon::Greatblade, Weapon::Glaive];
const GUNS: [Weapon; 3] = [Weapon::Pistol, Weapon::Carbine, Weapon::Rifle];
const RANGES: [f32; 4] = [10., 30., 60., 120.];

fn lower(weapon: Weapon) -> String {
    weapon.name().to_lowercase()
}

/// Every preset the page reads, once each, in name order.
fn needed() -> Vec<String> {
    let mut names = Vec::new();
    for weapon in GUNS {
        // A row that does not reach a range has no bout to run there.
        for metres in RANGES.into_iter().filter(|m| weapon.spec().range >= *m) {
            names.push(gun_preset(weapon, metres));
        }
    }
    for weapon in MELEE {
        names.push(format!("{}-vs-dummy", lower(weapon)));
        names.push(format!("{}-vs-rusher", lower(weapon)));
    }
    for case in ability_cases() {
        names.push(case.with.to_owned());
        names.push(case.without.to_owned());
    }
    for kind in OpponentKind::ALL {
        for weapon in WORTH_WEAPONS {
            names.push(worth_preset(weapon, kind));
        }
    }
    names.sort();
    names.dedup();
    names.retain(|name| !name.is_empty());
    names
}

fn gun_preset(weapon: Weapon, metres: f32) -> String {
    format!("{}-vs-dummy-{metres}m", lower(weapon))
}

fn worth_preset(weapon: Weapon, kind: OpponentKind) -> String {
    format!("worth-{}-vs-{}", lower(weapon), kind.name())
}

/// The bouts of every preset the page reads.
pub type Results = BTreeMap<String, Vec<BoutResult>>;

fn read(results: &Results, name: &str, metric: &Metric) -> anyhow::Result<f64> {
    metric.evaluate(name, results)
}

fn num(value: f64, places: usize) -> String {
    if value.is_nan() {
        "-".to_owned()
    } else {
        format!("{value:.places$}")
    }
}

/// The gun table's cell text and numbers for one weapon at one range, or
/// `None` when the row's range does not reach the target.
struct GunCell {
    rate: f64,
    share: f64,
}

fn guns(results: &Results) -> anyhow::Result<String> {
    let mut cells: BTreeMap<(usize, usize), Option<GunCell>> = BTreeMap::new();
    for (g, weapon) in GUNS.into_iter().enumerate() {
        for (r, metres) in RANGES.into_iter().enumerate() {
            let cell = if weapon.spec().range < metres {
                None
            } else {
                let name = gun_preset(weapon, metres);
                Some(GunCell {
                    rate: read(results, &name, &Metric::A(SideMetric::DealtPerSecond))?,
                    share: read(results, &name, &Metric::A(SideMetric::HitShare))?,
                })
            };
            cells.insert((g, r), cell);
        }
    }
    let best: Vec<Option<usize>> = (0..RANGES.len())
        .map(|r| {
            (0..GUNS.len())
                .filter_map(|g| cells[&(g, r)].as_ref().map(|c| (g, c.rate)))
                .max_by(|a, b| a.1.total_cmp(&b.1))
                .filter(|(_, rate)| *rate > 0.)
                .map(|(g, _)| g)
        })
        .collect();
    let mut out = String::from("## Guns by range\n\n");
    out.push_str(
        "Each gun standing and aiming at the passive dummy (the rifle one shot at its own \
         interval). Damage a second over the bout, and the share of shots that struck. The \
         best damage a second at each range is in bold; a gun whose row does not reach a \
         range says so.\n\n",
    );
    out.push_str("| Weapon | Range | Damage a second | Hit share |\n|---|---|---|---|\n");
    for (g, weapon) in GUNS.into_iter().enumerate() {
        for (r, metres) in RANGES.into_iter().enumerate() {
            match &cells[&(g, r)] {
                None => {
                    let _ = writeln!(
                        out,
                        "| {} | {metres} m | out of range ({} m) | - |",
                        weapon.name(),
                        weapon.spec().range
                    );
                }
                Some(cell) => {
                    let rate = num(cell.rate, 1);
                    let rate = if best[r] == Some(g) {
                        format!("**{rate}**")
                    } else {
                        rate
                    };
                    let _ = writeln!(
                        out,
                        "| {} | {metres} m | {rate} | {}% |",
                        weapon.name(),
                        num(cell.share * 100., 0)
                    );
                }
            }
        }
    }
    // Rule: name the gun with the most damage a second at each range.
    let words: Vec<String> = RANGES
        .into_iter()
        .zip(&best)
        .map(|(metres, best)| {
            best.map_or_else(
                || format!("no gun reaches {metres} m"),
                |g| format!("{} at {metres} m", GUNS[g].name()),
            )
        })
        .collect();
    let _ = write!(out, "\nMost damage a second: {}.", words.join(", "));
    let never: Vec<&str> = GUNS
        .into_iter()
        .enumerate()
        .filter(|(g, _)| !best.contains(&Some(*g)))
        .map(|(_, weapon)| weapon.name())
        .collect();
    if !never.is_empty() {
        let _ = write!(out, " Never first on the dummy: {}.", never.join(", "));
    }
    out.push('\n');
    Ok(out)
}

/// A class's hits and seconds to empty a full posture pool with a chain: the
/// Light, then the Followup for each next hit, the first hit at the Light's
/// contact start and each next one a Followup's duration later. Posture
/// regeneration is ignored (it waits `hurt_wait`).
fn time_to_stagger(weapon: Weapon) -> Option<(u32, f32)> {
    let class = combat_tables::with_melee(|melee| melee.class(weapon))?;
    let pool = combat_tables::current().actor.posture;
    let (light, follow) = (Action::Light.spec(), Action::Followup.spec());
    let mut posture = light.posture * class.light.posture;
    let mut time = light.contact_start * class.light.time;
    let mut hits = 1;
    while posture < pool && hits < 64 {
        posture += follow.posture * class.light.posture;
        time += follow.duration * class.light.time;
        hits += 1;
    }
    (posture >= pool).then_some((hits, time))
}

/// What a guard of `weapon` lets through a blow of `damage` and `posture`:
/// the damage, the posture and the stamina the guard pays.
fn through(weapon: Weapon, damage: f32, posture: f32) -> Option<(f32, f32, f32)> {
    let row = combat_tables::with_melee(|melee| melee.class(weapon))?.guard;
    Some((
        damage * (1. - row.share),
        posture * row.posture,
        damage * row.share * row.stamina,
    ))
}

fn melee(results: &Results) -> anyhow::Result<String> {
    let rate = Metric::A(SideMetric::DealtPerSecond);
    let carbine = Weapon::Carbine.spec();
    let heavy = Action::Heavy.spec();
    let mut out = String::from("## Melee\n\n");
    out.push_str(
        "Each class with the cutting policy of its dummy and rusher presets. Damage a second is \
         measured. Time to stagger is worked from the tables: a Light then Followups, until the \
         posture pool is gone (hits, then seconds). The last two columns are what the class's \
         guard lets through of one carbine round and of the table's Heavy: damage, posture and \
         the stamina the guard pays.\n\n",
    );
    out.push_str(
        "| Class | Damage/s on the dummy | Damage/s on the rusher | Time to stagger | Guard, carbine round | Guard, Heavy |\n|---|---|---|---|---|---|\n",
    );
    let mut staggers = Vec::new();
    let mut posture_through = Vec::new();
    for weapon in MELEE {
        let dummy = read(results, &format!("{}-vs-dummy", lower(weapon)), &rate)?;
        let rusher = read(results, &format!("{}-vs-rusher", lower(weapon)), &rate)?;
        let stagger = time_to_stagger(weapon);
        let (round, hit) = (
            through(weapon, carbine.damage, carbine.posture),
            through(weapon, heavy.damage, heavy.posture),
        );
        let fmt3 = |v: Option<(f32, f32, f32)>| {
            v.map_or_else(
                || "-".to_owned(),
                |(d, p, s)| format!("{d:.1} / {p:.1} / {s:.1}"),
            )
        };
        let stagger_text = stagger.map_or_else(
            || "never".to_owned(),
            |(hits, seconds)| format!("{hits} hits, {seconds:.2} s"),
        );
        staggers.push((weapon, stagger.map(|s| s.1)));
        posture_through.push((weapon, hit.map(|h| h.1)));
        let _ = writeln!(
            out,
            "| {} | {} | {} | {stagger_text} | {} | {} |",
            weapon.name(),
            num(dummy, 1),
            num(rusher, 1),
            fmt3(round),
            fmt3(hit)
        );
    }
    // Rule: the classes within a hundredth of the least seconds to stagger,
    // and of the least posture a Heavy gets through a guard.
    let least = |values: &[(Weapon, Option<f32>)]| -> Option<(String, f32)> {
        let floor = values
            .iter()
            .filter_map(|(_, v)| *v)
            .min_by(f32::total_cmp)?;
        let names: Vec<&str> = values
            .iter()
            .filter(|(_, v)| v.is_some_and(|v| v - floor < 0.01))
            .map(|(w, _)| w.name())
            .collect();
        Some((names.join(" and "), floor))
    };
    if let (Some(soonest), Some(sturdiest)) = (least(&staggers), least(&posture_through)) {
        let _ = write!(
            out,
            "\n{} stagger soonest ({:.2} s) and the {} guard lets the least posture of a Heavy \
             through ({:.1}).\n",
            soonest.0, soonest.1, sturdiest.0, sturdiest.1
        );
    }
    Ok(out)
}

/// The word for a change in percent. A `None` change is a control that
/// measured nothing against an ability that did.
fn word(change: Option<f64>) -> &'static str {
    match change {
        None => "unread",
        Some(c) if c > STRONG_OVER => "strong",
        Some(c) if c >= PAYS_FROM => "pays",
        Some(c) if c >= SLIGHT_FROM => "slight",
        Some(_) => "dead",
    }
}

/// How many times an ability can be used in a minute at most: the lesser of
/// what its cooldown and commitment allow and what the stamina pool and its
/// regeneration pay for.
fn uses_a_minute(stamina: f32, cooldown: f32, length: f32) -> f32 {
    let actor = combat_tables::current().actor.clone();
    let period = cooldown.max(length);
    let by_time = if period > 0. { 60. / period } else { f32::MAX };
    let by_stamina = if stamina > 0. {
        (actor.stamina + actor.stamina_regen * 60.) / stamina
    } else {
        f32::MAX
    };
    by_time.min(by_stamina)
}

fn abilities(results: &Results) -> anyhow::Result<String> {
    let tables = combat_tables::current();
    let mut out = String::from("## Abilities\n\n");
    out.push_str(
        "Each ability beside a control without it, on the weapon and at the range it is for, \
         read on the number it is for (damage a second, the win share for a closer, damage \
         taken, posture left or seconds to win for a defensive one, where a smaller figure is the better). \
         The two seconds columns show what the ability did to the length of the fight, which a gain on \
         another number can hide. \
         The change is the ability's side over the control's, as a gain (inverted where smaller \
         is better). Uses a minute is the lesser of what the cooldown with the commitment and \
         the stamina pool with its regeneration allow. The word: strong over +40 percent, pays \
         +10 to +40, slight +3 to +10, dead under +3; a closer whose control wins nothing is \
         marked opens when it wins something and unread when it wins nothing either.\n\n",
    );
    out.push_str(
        "| Ability | Weapon | Reads | Control | With | Change | Seconds control | Seconds with | Stamina | Cooldown | Uses a minute | Word |\n|---|---|---|---|---|---|---|---|---|---|---|---|\n",
    );
    let mut counts: BTreeMap<&str, Vec<String>> = BTreeMap::new();
    for case in ability_cases() {
        let row = tables
            .abilities
            .row(case.id)
            .ok_or_else(|| anyhow::anyhow!("no ability `{}` in abilities.ron", case.id))?;
        let control = read(results, case.without, &case.metric)?;
        let with = read(results, case.with, &case.metric)?;
        // The gain: with over control, or control over with where smaller is
        // better. Nothing to divide by reads as unread, unless the ability
        // made something from a control of nothing.
        let change = if case.lower_is_better {
            if with == 0. && control == 0. {
                None
            } else if with == 0. {
                Some(f64::INFINITY)
            } else {
                Some((control / with - 1.) * 100.)
            }
        } else if control == 0. {
            None
        } else {
            Some((with / control - 1.) * 100.)
        };
        let opened = !case.lower_is_better && control == 0. && with > 0.;
        let (stamina, length) = match &row.effect {
            veyra_rules::ability::AbilityEffect::Cut(action) => {
                let spec = action.spec();
                (spec.stamina, spec.duration)
            }
            _ => (row.stamina, row.length),
        };
        // A press that never happened in the fight proves nothing about the
        // ability, whatever the two columns say. An instant press and a cut
        // (counted under cuts) are not in the book's count, so only the rest
        // can be told unpressed.
        let pressed = row.is_instant()
            || matches!(row.effect, veyra_rules::ability::AbilityEffect::Cut(_))
            || read(results, case.with, &Metric::A(SideMetric::Abilities))? > 0.;
        let word = if opened {
            "opens"
        } else if !pressed {
            "unread"
        } else {
            word(change)
        };
        counts.entry(word).or_default().push(case.id.to_owned());
        let change_text = match change {
            Some(c) if c.is_finite() => format!("{c:+.0}%"),
            Some(_) => "no damage taken".to_owned(),
            None if opened => "from nothing".to_owned(),
            None => "-".to_owned(),
        };
        let _ = writeln!(
            out,
            "| {} | {} | {} | {} | {} | {change_text} | {} | {} | {stamina:.0} | {:.1} s | {:.0} | {word} |",
            case.id,
            case.weapon.name(),
            metric_name(&case.metric, case.lower_is_better),
            num(control, 2),
            num(with, 2),
            num(read(results, case.without, &Metric::MeanTime)?, 1),
            num(read(results, case.with, &Metric::MeanTime)?, 1),
            row.cooldown,
            uses_a_minute(stamina, row.cooldown, length).min(999.),
        );
    }
    // Rule: count the words and list the ones that are not paying.
    let count = |w: &str| counts.get(w).map_or(0, Vec::len);
    let _ = write!(
        out,
        "\n{} abilities: {} strong, {} pay, {} slight, {} dead, {} open a fight the control cannot, {} unread.",
        ability_cases().len(),
        count("strong"),
        count("pays"),
        count("slight"),
        count("dead"),
        count("opens"),
        count("unread")
    );
    for w in ["dead", "unread"] {
        if let Some(ids) = counts.get(w) {
            let _ = write!(out, " The {w} ones: {}.", ids.join(", "));
        }
    }
    out.push('\n');
    Ok(out)
}

fn metric_name(metric: &Metric, lower_is_better: bool) -> &'static str {
    match (metric, lower_is_better) {
        (Metric::WinRate, _) => "win share",
        (Metric::A(SideMetric::DealtPerSecond), _) => "damage/s",
        (Metric::A(SideMetric::Taken), _) => "damage taken",
        (Metric::A(SideMetric::PostureMin), _) => "posture left",
        (Metric::A(SideMetric::Dealt), _) => "damage dealt",
        (Metric::B(SideMetric::PostureMin), _) => "their posture left",
        (Metric::MeanTime, _) => "seconds to win",
        (_, true) => "lower is better",
        (_, false) => "figure",
    }
}

/// One cell of the opponents table.
struct Fight {
    weapon: Weapon,
    win: f64,
    seconds: f64,
    taken: f64,
}

fn opponents(results: &Results) -> anyhow::Result<String> {
    let mut out = String::from("## Opponents\n\n");
    out.push_str(
        "Every weapon against each opponent kind with its plain policy: a gun fires as its \
         presets do, a melee class sprints in with its kit on the bar. Win share, mean seconds \
         of the bouts it won, and damage taken. Among the weapons that win at least half their \
         bouts, the quickest is marked fastest and the one that takes least is marked safest.\n\n",
    );
    out.push_str("| Opponent | Weapon | Win share | Seconds to win | Damage taken | Mark |\n|---|---|---|---|---|---|\n");
    let mut sentences = String::new();
    for kind in OpponentKind::ALL {
        let mut fights = Vec::new();
        for weapon in WORTH_WEAPONS {
            let name = worth_preset(weapon, kind);
            fights.push(Fight {
                weapon,
                win: read(results, &name, &Metric::WinRate)?,
                seconds: read(results, &name, &Metric::MeanTime)?,
                taken: read(results, &name, &Metric::A(SideMetric::Taken))?,
            });
        }
        let winners = || {
            fights
                .iter()
                .filter(|f| f.win >= WINNING && !f.seconds.is_nan())
        };
        // A tie to the figure the table prints shares the mark.
        let shown = |v: f64| (v * 10.).round();
        let quickest = winners().map(|f| shown(f.seconds)).min_by(f64::total_cmp);
        let safest = winners().map(|f| shown(f.taken)).min_by(f64::total_cmp);
        for fight in &fights {
            let mut marks = Vec::new();
            let won = fight.win >= WINNING && !fight.seconds.is_nan();
            if won && Some(shown(fight.seconds)) == quickest {
                marks.push("fastest");
            }
            if won && Some(shown(fight.taken)) == safest {
                marks.push("safest");
            }
            let _ = writeln!(
                out,
                "| {} | {} | {}% | {} | {} | {} |",
                kind.name(),
                fight.weapon.name(),
                num(fight.win * 100., 0),
                num(fight.seconds, 1),
                num(fight.taken, 1),
                marks.join(", ")
            );
        }
        sentences.push_str(&opponent_sentence(kind, &fights));
    }
    out.push('\n');
    out.push_str(&sentences);
    Ok(out)
}

/// Rule: the best weapon is the quickest of those that win at least half
/// their bouts. The best gun is the quickest gun that wins; a melee class is
/// within reach of it when its seconds and its damage taken are both at most
/// 1.5 times that gun's (so a gun that takes nothing is matched only by a
/// class that takes nothing). When none is, the nearest is the class with
/// the lowest ratio of seconds.
fn opponent_sentence(kind: OpponentKind, fights: &[Fight]) -> String {
    let winning = |f: &&Fight| f.win >= WINNING && !f.seconds.is_nan();
    let best = fights
        .iter()
        .filter(winning)
        .min_by(|a, b| a.seconds.total_cmp(&b.seconds));
    let Some(best) = best else {
        return format!(
            "No weapon wins half its bouts against the {} inside the cap.\n\n",
            kind.name()
        );
    };
    let gun = fights
        .iter()
        .filter(|f| GUNS.contains(&f.weapon))
        .filter(winning)
        .min_by(|a, b| a.seconds.total_cmp(&b.seconds));
    let mut text = format!(
        "Against the {} the quickest winner is the {} ({:.1} s, taking {:.1}).",
        kind.name(),
        best.weapon.name(),
        best.seconds,
        best.taken
    );
    let Some(gun) = gun else {
        text.push_str(" No gun wins half its bouts.");
        text.push_str("\n\n");
        return text;
    };
    let near: Vec<&Fight> = fights
        .iter()
        .filter(|f| MELEE.contains(&f.weapon))
        .filter(winning)
        .filter(|f| f.seconds <= WITHIN * gun.seconds && f.taken <= WITHIN * gun.taken)
        .collect();
    if near.is_empty() {
        let nearest = fights
            .iter()
            .filter(|f| MELEE.contains(&f.weapon))
            .filter(winning)
            .min_by(|a, b| a.seconds.total_cmp(&b.seconds));
        let _ = write!(
            text,
            " No melee class is within {WITHIN} times the best gun's ({}, {:.1} s, taking {:.1}) \
             in both seconds and damage taken",
            gun.weapon.name(),
            gun.seconds,
            gun.taken
        );
        match nearest {
            Some(n) => {
                let _ = write!(
                    text,
                    "; the nearest in seconds is the {} at {:.1} s, {:.1} times as long, taking {:.1}.",
                    n.weapon.name(),
                    n.seconds,
                    n.seconds / gun.seconds,
                    n.taken
                );
            }
            None => text.push_str("; no melee class wins half its bouts."),
        }
    } else {
        let names: Vec<&str> = near.iter().map(|f| f.weapon.name()).collect();
        let _ = write!(
            text,
            " Within {WITHIN} times the best gun's ({}, {:.1} s, taking {:.1}) in both seconds and \
             damage taken: {}.",
            gun.weapon.name(),
            gun.seconds,
            gun.taken,
            names.join(", ")
        );
    }
    text.push_str("\n\n");
    text
}

fn changes(tuning: &Tuning) -> String {
    let mut out = String::from("## What changed in this tuning\n\n");
    out.push_str(
        "Written from `assets/balance/combat_tuning.ron`, the log kept while the dials were \
         moved.\n\n| Dial | From | To | Why |\n|---|---|---|---|\n",
    );
    for change in &tuning.changes {
        let cell = |s: &str| s.replace('|', "/").replace('\n', " ");
        let _ = writeln!(
            out,
            "| {} | {} | {} | {} |",
            cell(&change.dial),
            cell(&change.from),
            cell(&change.to),
            cell(&change.why)
        );
    }
    let _ = write!(out, "\n{} dials moved.\n", tuning.changes.len());
    out
}

/// Run the presets the page reads on the installed tables and write the
/// page.
///
/// # Errors
/// A preset that does not exist, an ability the table lacks, or a tuning log
/// that does not read.
pub fn page(assets: &Path, bouts: u32, seed: u64, cap: f32, jobs: usize) -> anyhow::Result<String> {
    let tuning = Tuning::load(assets)?;
    let mut results = Results::new();
    for name in needed() {
        let matchup = preset(&name)?;
        let run = run_matchup(&matchup, seed, bouts, cap, jobs);
        results.insert(name, run.into_iter().map(|(_, r)| r).collect());
    }
    page_from(&results, &tuning, bouts)
}

/// The page from bouts already run.
///
/// # Errors
/// A preset with no bouts in `results`.
pub fn page_from(results: &Results, tuning: &Tuning, bouts: u32) -> anyhow::Result<String> {
    let mut out = format!(
        "# What the weapons and abilities are worth\n\nMade by `just balance worth --write \
         docs/balance/combat-worth.md` from the duel harness over the shipped tables, {bouts} \
         bouts a preset. Do not edit it: make it again after a change to a table. Every \
         sentence under a table is written by the command from the figures above it.\n\n"
    );
    for section in [
        guns(results)?,
        melee(results)?,
        abilities(results)?,
        opponents(results)?,
        changes(tuning),
    ] {
        out.push_str(&section);
        out.push('\n');
    }
    Ok(out)
}
