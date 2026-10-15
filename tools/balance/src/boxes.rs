//! Boxes as a dimension of the harness. A side may carry a set of learned
//! boxes; at spawn the bout fits the sheet to its combatant through the
//! same `strata_sim::progression::apply` the game uses, so a box's
//! marginal effect here is what it is in play. A side with no boxes is
//! unfitted and reads the installed tables live, as every preset did
//! before the trees existed, so the older rows and bands keep their
//! numbers.
//!
//! The box sweep is the design's "every box's marginal effect" (section
//! 7): a baseline sheet, then one bout set per box that the baseline could
//! learn next, each added alone, and the change in win rate, time to kill
//! and damage taken against the baseline.

use std::sync::{Arc, LazyLock, PoisonError, RwLock};

use strata_registry::progression::{Effect, SheetDef, SkillGraph};
use strata_sim::CharacterSheet;

use crate::bout::{BoutResult, Matchup, Profile, Winner};

/// The graph the bouts fit sheets against: process-wide like the combat
/// tables, since a bout is built from a `Matchup` that names boxes and
/// nothing else. Shipped unless `install` is called.
static GRAPH: LazyLock<RwLock<Arc<SkillGraph>>> =
    LazyLock::new(|| RwLock::new(Arc::new(SkillGraph::shipped())));

/// Install the graph every later bout fits against.
pub fn install(graph: SkillGraph) {
    *GRAPH.write().unwrap_or_else(PoisonError::into_inner) = Arc::new(graph);
}

/// The installed graph.
#[must_use]
pub fn graph() -> Arc<SkillGraph> {
    GRAPH.read().unwrap_or_else(PoisonError::into_inner).clone()
}

/// The sheet that opens the combat slice's whole moveset and nothing
/// else: the two novices, the Cuts branch to the Heavy and the carbine.
/// One hand 2 is in it because One hand 3 needs it, so the fitted Follow-up
/// does 6 more than the unfitted one; this is the baseline a box sweep
/// adds to, since a sheet with less cannot cut a chain or fire.
pub const MOVESET: [&str; 6] = [
    "blade.novice",
    "blade.cuts.1",
    "blade.cuts.2",
    "blade.cuts.3",
    "marksman.novice",
    "marksman.carbine.1",
];

/// The sheet the game fits the opponent `name` from
/// (`assets/progression/opponents.ron`), so the harness fights the same
/// marksman and duelist: authored, never earned.
///
/// # Panics
/// The shipped opponents file does not parse, which the registry's own
/// test would have refused first.
#[must_use]
pub fn opponent(name: &str) -> Option<CharacterSheet> {
    static SHEETS: LazyLock<strata_registry::progression::OpponentSheets> = LazyLock::new(|| {
        ron::from_str(include_str!("../../../assets/progression/opponents.ron"))
            .expect("the shipped opponent sheets parse")
    });
    SHEETS.sheets.get(name).map(CharacterSheet::authored)
}

/// A box id as the graph spells it from the command line's spelling:
/// `blade:cuts-1` and `blade.cuts.1` are the same box.
#[must_use]
pub fn id(text: &str) -> String {
    text.trim().replace([':', '-'], ".")
}

/// A comma-separated list of box ids, in either spelling; empty text is
/// no boxes.
#[must_use]
pub fn parse(text: &str) -> Vec<String> {
    text.split(',')
        .map(id)
        .filter(|id| !id.is_empty())
        .collect()
}

/// The sheet of `boxes`, checked against the installed graph as an
/// authored sheet is: every box exists, every prerequisite is there,
/// the budget holds. The XP gates are paid in full on every kind; the
/// harness asks what a box is worth, not whether it has been earned.
///
/// # Errors
/// The first thing wrong, naming `name`.
pub fn sheet(name: &str, boxes: &[String]) -> anyhow::Result<CharacterSheet> {
    let def = SheetDef {
        learned: boxes.to_vec(),
        xp: strata_registry::progression::XpKind::ALL
            .into_iter()
            .map(|kind| (kind, u32::MAX))
            .collect(),
    };
    graph()
        .check_sheet(name, &def)
        .map_err(|error| anyhow::anyhow!("{error}"))?;
    Ok(CharacterSheet::authored(&def))
}

/// The boxes `sheet` could learn next, in graph order: every box of a
/// tree with effects (the placeholders have none to measure) that is not
/// learned and whose prerequisites all are.
#[must_use]
pub fn learnable(graph: &SkillGraph, sheet: &CharacterSheet) -> Vec<String> {
    graph
        .trees
        .iter()
        .filter(|tree| !tree.placeholder)
        .flat_map(strata_registry::progression::TreeDef::boxes)
        // A box that moves only survey, ride, bench, fabricator or wear
        // numbers moves no bout.
        .filter(|b| {
            b.effects.iter().any(|effect| {
                !matches!(
                    effect,
                    Effect::Survey { .. }
                        | Effect::Read { .. }
                        | Effect::Peak
                        | Effect::CraftPoints { .. }
                        | Effect::Ride { .. }
                        | Effect::Craft { .. }
                        | Effect::Factory { .. }
                        | Effect::Wear { .. }
                        | Effect::Station { .. }
                        | Effect::Grant { .. }
                        | Effect::Ability { .. }
                )
            })
        })
        .filter(|b| !sheet.has(&b.id))
        .filter(|b| {
            graph
                .prerequisites(&b.id)
                .iter()
                .all(|prerequisite| sheet.has(prerequisite))
        })
        .map(|b| b.id.clone())
        .collect()
}

/// Every box of the tree called `tree`, in graph order; empty for an
/// unknown tree.
#[must_use]
pub fn whole_tree(graph: &SkillGraph, tree: &str) -> Vec<String> {
    graph
        .tree(tree)
        .map(|tree| tree.boxes().map(|b| b.id.clone()).collect())
        .unwrap_or_default()
}

/// What a sweep reads of one bout set: the three numbers the design asks
/// a box's marginal effect in.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Reading {
    /// Side `a`'s wins over all bouts, draws against it.
    pub win_rate: f64,
    /// Mean seconds of the decided bouts; `NaN` with none.
    pub time: f64,
    /// Mean health and shield side `a` lost.
    pub taken: f64,
}

impl Reading {
    /// The three numbers over `bouts`.
    #[allow(clippy::cast_precision_loss, reason = "bout counts")]
    #[must_use]
    pub fn of(bouts: &[BoutResult]) -> Self {
        let n = bouts.len().max(1) as f64;
        let decided: Vec<f64> = bouts
            .iter()
            .filter(|b| b.winner != Winner::Draw)
            .map(|b| f64::from(b.time))
            .collect();
        Self {
            win_rate: bouts.iter().filter(|b| b.winner == Winner::A).count() as f64 / n,
            time: if decided.is_empty() {
                f64::NAN
            } else {
                decided.iter().sum::<f64>() / decided.len() as f64
            },
            taken: bouts
                .iter()
                .map(|b| f64::from(b.sides[0].taken))
                .sum::<f64>()
                / n,
        }
    }
}

/// One row of a sweep: the baseline (`added` empty) or one box added to
/// it.
#[derive(Debug, Clone, PartialEq)]
pub struct Row {
    /// The box added, or nothing for the baseline.
    pub added: Option<String>,
    /// Its cost in points, zero for the baseline.
    pub cost: u16,
    /// What the bouts gave.
    pub reading: Reading,
}

/// A sweep: side `a`'s baseline, then each learnable box added alone.
#[derive(Debug, Clone, PartialEq)]
pub struct Sweep {
    /// The matchup, with its sides' baselines in their `boxes`.
    pub matchup: Matchup,
    /// The rows, the baseline first.
    pub rows: Vec<Row>,
}

/// Run a sweep of `matchup`: its side `a` as given is the baseline, and
/// every box its sheet could learn next is added to it alone for a bout
/// set of its own. `each` sees each row as it finishes, so a long sweep
/// prints as it goes.
///
/// # Errors
/// A side's boxes that are no sheet, or its modules that are no loadout.
pub fn sweep(
    matchup: &Matchup,
    seed: u64,
    bouts: u32,
    cap: f32,
    jobs: usize,
    mut each: impl FnMut(&Row),
) -> anyhow::Result<Sweep> {
    let graph = graph();
    let baseline = sheet("a", &matchup.a.boxes)?;
    sheet("b", &matchup.b.boxes)?;
    crate::loadout::check(matchup)?;
    let mut rows = Vec::new();
    let base = Row {
        added: None,
        cost: 0,
        reading: Reading::of(&results(matchup, seed, bouts, cap, jobs)),
    };
    each(&base);
    rows.push(base);
    for id in learnable(&graph, &baseline) {
        let mut varied = matchup.clone();
        varied.a.boxes.push(id.clone());
        let cost = graph.find(&id).map_or(0, |b| b.cost);
        let row = Row {
            added: Some(id),
            cost,
            reading: Reading::of(&results(&varied, seed, bouts, cap, jobs)),
        };
        each(&row);
        rows.push(row);
    }
    Ok(Sweep {
        matchup: matchup.clone(),
        rows,
    })
}

fn results(matchup: &Matchup, seed: u64, bouts: u32, cap: f32, jobs: usize) -> Vec<BoutResult> {
    crate::run_matchup(matchup, seed, bouts, cap, jobs)
        .into_iter()
        .map(|(_, result)| result)
        .collect()
}

/// A number and its change from the baseline, as the table prints them.
fn delta(value: f64, base: f64, places: usize) -> (String, String) {
    let change = value - base;
    let sign = if change > 0. { "+" } else { "" };
    let number = |x: f64, sign: &str| {
        if x.is_nan() {
            "nan".to_owned()
        } else {
            format!("{sign}{x:.places$}")
        }
    };
    (number(value, ""), number(change, sign))
}

/// The header of the sweep's table, for a run that prints rows as they
/// finish.
#[must_use]
pub fn header() -> String {
    "| Box | Cost | Win rate | Change | Time to kill | Change | Taken | Change |\n\
     | --- | --- | --- | --- | --- | --- | --- | --- |"
        .to_owned()
}

/// One row of the sweep's table under [`header`], against `base`.
#[must_use]
pub fn line(row: &Row, base: &Reading) -> String {
    let (win, win_change) = delta(row.reading.win_rate, base.win_rate, 2);
    let (time, time_change) = delta(row.reading.time, base.time, 2);
    let (taken, taken_change) = delta(row.reading.taken, base.taken, 1);
    match &row.added {
        None => format!("| baseline | | {win} | | {time} | | {taken} | |"),
        Some(id) => format!(
            "| {id} | {} | {win} | {win_change} | {time} | {time_change} | {taken} | {taken_change} |",
            row.cost
        ),
    }
}

/// The whole sweep as a Markdown table, the ledger's form.
#[must_use]
pub fn table(sweep: &Sweep) -> String {
    let base = sweep.rows.first().map_or(
        Reading {
            win_rate: f64::NAN,
            time: f64::NAN,
            taken: f64::NAN,
        },
        |row| row.reading,
    );
    std::iter::once(header())
        .chain(sweep.rows.iter().map(|row| line(row, &base)))
        .collect::<Vec<_>>()
        .join("\n")
}

/// A sweep's side `b` as the baseline gives it: the same sheet when the
/// opponent is a player too, so the only difference between the sides is
/// the box under test; an opponent of the game keeps its own sheet.
#[must_use]
pub fn opponent_boxes(matchup: &Matchup) -> Vec<String> {
    if matchup.b.profile == Profile::Player && matchup.b.boxes.is_empty() {
        matchup.a.boxes.clone()
    } else {
        matchup.b.boxes.clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spellings_and_sheets() {
        assert_eq!(id("blade:cuts-1"), "blade.cuts.1");
        assert_eq!(
            parse("blade:cuts-1, marksman.field_craft.2,"),
            vec!["blade.cuts.1", "marksman.field_craft.2"]
        );
        let moveset: Vec<String> = MOVESET.iter().map(|s| (*s).to_owned()).collect();
        let base = sheet("a", &moveset).expect("the moveset is a sheet");
        assert_eq!(base.points_spent(&graph()), 26);
        // A chain with a step missing is no sheet, nor an unknown box.
        assert!(sheet("a", &["blade.cuts.2".to_owned()]).is_err());
        assert!(sheet("a", &["blade.cuts.9".to_owned()]).is_err());
    }

    #[test]
    fn the_learnable_boxes_are_the_next_step_of_every_open_branch() {
        let graph = graph();
        let moveset: Vec<String> = MOVESET.iter().map(|s| (*s).to_owned()).collect();
        let base = sheet("a", &moveset).expect("sheet");
        let next = learnable(&graph, &base);
        // Blade: Cuts 4 and the first of the other three branches;
        // Marksman: Carbine 2 and the first of the other three. No
        // placeholder tree's novice, and no master.
        assert_eq!(
            next,
            [
                "marksman.carbine.2",
                "marksman.pistol.1",
                "marksman.steadiness.1",
                "marksman.field_craft.1",
                "blade.cuts.4",
                "blade.guard.1",
                "blade.footwork.1",
                "blade.breaking.1",
            ]
        );
        let empty = CharacterSheet::default();
        assert_eq!(
            learnable(&graph, &empty),
            ["marksman.novice", "blade.novice"]
        );
        let whole = whole_tree(&graph, "blade");
        assert_eq!(whole.len(), 18);
        let master = sheet("a", &whole).expect("a whole tree is a sheet");
        assert_eq!(master.points_spent(&graph), 98);
        assert!(!learnable(&graph, &master).contains(&"blade.master".to_owned()));
    }

    #[test]
    fn the_table_prints_changes_against_the_baseline() {
        let reading = |win_rate, time, taken| Reading {
            win_rate,
            time,
            taken,
        };
        let sweep = Sweep {
            matchup: crate::preset("blade-vs-blade").expect("preset"),
            rows: vec![
                Row {
                    added: None,
                    cost: 0,
                    reading: reading(0.5, 2., 100.),
                },
                Row {
                    added: Some("blade.cuts.4".into()),
                    cost: 8,
                    reading: reading(0.65, 1.5, 90.),
                },
            ],
        };
        let text = table(&sweep);
        assert!(
            text.contains("| baseline | | 0.50 | | 2.00 | | 100.0 | |"),
            "{text}"
        );
        assert!(
            text.contains("| blade.cuts.4 | 8 | 0.65 | +0.15 | 1.50 | -0.50 | 90.0 | -10.0 |"),
            "{text}"
        );
    }
}
