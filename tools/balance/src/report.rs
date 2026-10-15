//! The CSV: one row per bout and one summary row per cell. Plain text with
//! a fixed column order so Python reads it with no schema, and the cell's
//! varied values are leading columns named by their paths.

use crate::bout::{BoutResult, SideStats, Winner};
use crate::grid::Cell;

/// The per-side columns, each written for `a` and for `b`.
const SIDE_COLUMNS: [&str; 11] = [
    "dealt",
    "taken",
    "shots",
    "hits",
    "cuts",
    "cuts_landed",
    "stamina_min",
    "posture_min",
    "breaks",
    "dodges",
    "dodges_avoided",
];

/// The guard's columns, after both sides' columns so a row written before
/// the guard is a prefix of one written since.
const GUARD_COLUMNS: [&str; 3] = ["blocks", "parries", "guard_breaks"];

fn guard_header(columns: &mut Vec<String>) {
    for side in ["a", "b"] {
        columns.extend(GUARD_COLUMNS.iter().map(|c| format!("{side}_{c}")));
    }
    columns.extend(["a_abilities".to_owned(), "b_abilities".to_owned()]);
}

/// The header for bout rows: the varied paths, then the fixed columns.
#[must_use]
pub fn bout_header(paths: &[String]) -> String {
    let mut columns: Vec<String> = paths.to_vec();
    columns.extend(["matchup", "seed", "winner", "time"].map(str::to_owned));
    for side in ["a", "b"] {
        columns.extend(SIDE_COLUMNS.iter().map(|c| format!("{side}_{c}")));
    }
    guard_header(&mut columns);
    columns.join(",")
}

fn side_values(side: &SideStats) -> Vec<String> {
    vec![
        format!("{:.1}", side.dealt),
        format!("{:.1}", side.taken),
        side.shots.to_string(),
        side.hits.to_string(),
        side.cuts.to_string(),
        side.cuts_landed.to_string(),
        format!("{:.1}", side.stamina_min),
        format!("{:.1}", side.posture_min),
        side.breaks.to_string(),
        side.dodges.to_string(),
        side.dodges_avoided.to_string(),
    ]
}

fn guard_values(side: &SideStats) -> Vec<String> {
    vec![
        side.blocks.to_string(),
        side.parries.to_string(),
        side.guard_breaks.to_string(),
    ]
}

fn cell_values(cell: &Cell) -> Vec<String> {
    cell.values.iter().map(|(_, v)| trim_float(*v)).collect()
}

/// A float with no trailing zeros, so `0.4` reads as written.
fn trim_float(value: f64) -> String {
    let text = format!("{value:.6}");
    let text = text.trim_end_matches('0').trim_end_matches('.');
    if text.is_empty() || text == "-" {
        "0".to_owned()
    } else {
        text.to_owned()
    }
}

/// One bout as a row under [`bout_header`].
#[must_use]
pub fn bout_row(cell: &Cell, matchup: &str, seed: u64, result: &BoutResult) -> String {
    let mut columns = cell_values(cell);
    columns.push(matchup.to_owned());
    columns.push(seed.to_string());
    columns.push(result.winner.name().to_owned());
    columns.push(format!("{:.3}", result.time));
    for side in &result.sides {
        columns.extend(side_values(side));
    }
    for side in &result.sides {
        columns.extend(guard_values(side));
    }
    columns.extend(result.sides.iter().map(|side| side.abilities.to_string()));
    columns.join(",")
}

/// The header for summary rows: the varied paths, the matchup, the count,
/// side `a`'s win rate, the draw rate, the mean time of the decided bouts,
/// then the mean of every per-side column.
#[must_use]
pub fn summary_header(paths: &[String]) -> String {
    let mut columns: Vec<String> = paths.to_vec();
    columns.extend(["matchup", "bouts", "a_win_rate", "draw_rate", "mean_time"].map(str::to_owned));
    for side in ["a", "b"] {
        columns.extend(SIDE_COLUMNS.iter().map(|c| format!("{side}_{c}")));
    }
    guard_header(&mut columns);
    columns.join(",")
}

/// The means over a cell's bouts.
#[must_use]
pub fn summary_row(cell: &Cell, matchup: &str, results: &[BoutResult]) -> String {
    let mut columns = cell_values(cell);
    columns.push(matchup.to_owned());
    columns.push(results.len().to_string());
    let n = results.len().max(1);
    #[allow(clippy::cast_precision_loss, reason = "bout counts")]
    let rate = |count: usize| format!("{:.3}", count as f64 / n as f64);
    let wins = results.iter().filter(|r| r.winner == Winner::A).count();
    let draws = results.iter().filter(|r| r.winner == Winner::Draw).count();
    columns.push(rate(wins));
    columns.push(rate(draws));
    let decided: Vec<f32> = results
        .iter()
        .filter(|r| r.winner != Winner::Draw)
        .map(|r| r.time)
        .collect();
    columns.push(mean(&decided));
    for side in 0..2 {
        let stat = |pick: fn(&SideStats) -> f64| {
            let values: Vec<f32> = results
                .iter()
                .map(|r| pick(&r.sides[side]))
                .map(|v| {
                    #[allow(clippy::cast_possible_truncation, reason = "a mean for a CSV")]
                    let v = v as f32;
                    v
                })
                .collect();
            mean(&values)
        };
        columns.push(stat(|s| f64::from(s.dealt)));
        columns.push(stat(|s| f64::from(s.taken)));
        columns.push(stat(|s| count(s.shots)));
        columns.push(stat(|s| count(s.hits)));
        columns.push(stat(|s| count(s.cuts)));
        columns.push(stat(|s| count(s.cuts_landed)));
        columns.push(stat(|s| f64::from(s.stamina_min)));
        columns.push(stat(|s| f64::from(s.posture_min)));
        columns.push(stat(|s| count(s.breaks)));
        columns.push(stat(|s| count(s.dodges)));
        columns.push(stat(|s| count(s.dodges_avoided)));
    }
    for side in 0..2 {
        let stat = |pick: fn(&SideStats) -> u64| {
            #[allow(clippy::cast_precision_loss, reason = "a mean for a CSV")]
            let values: Vec<f32> = results
                .iter()
                .map(|r| pick(&r.sides[side]) as f32)
                .collect();
            mean(&values)
        };
        columns.push(stat(|s| s.blocks));
        columns.push(stat(|s| s.parries));
        columns.push(stat(|s| s.guard_breaks));
    }
    for side in 0..2 {
        #[allow(clippy::cast_precision_loss, reason = "a mean for a CSV")]
        let values: Vec<f32> = results
            .iter()
            .map(|r| r.sides[side].abilities as f32)
            .collect();
        columns.push(mean(&values));
    }
    columns.join(",")
}

#[allow(clippy::cast_precision_loss, reason = "small counters")]
fn count(value: u64) -> f64 {
    value as f64
}

fn mean(values: &[f32]) -> String {
    if values.is_empty() {
        return "nan".to_owned();
    }
    #[allow(clippy::cast_precision_loss, reason = "bout counts")]
    let mean = values.iter().map(|v| f64::from(*v)).sum::<f64>() / values.len() as f64;
    format!("{mean:.2}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_rows_line_up_with_their_headers() {
        let cell = Cell {
            values: vec![("weapons.Pistol.interval".to_owned(), 0.15)],
        };
        let paths = vec!["weapons.Pistol.interval".to_owned()];
        let result = BoutResult {
            winner: Winner::A,
            time: 9.5,
            sides: [SideStats::default(), SideStats::default()],
        };
        let header = bout_header(&paths);
        let row = bout_row(&cell, "m", 3, &result);
        assert_eq!(header.split(',').count(), row.split(',').count());
        assert!(row.starts_with("0.15,m,3,a,9.500,"));
        let header = summary_header(&paths);
        let row = summary_row(
            &cell,
            "m",
            &[
                result.clone(),
                BoutResult {
                    winner: Winner::Draw,
                    ..result
                },
            ],
        );
        assert_eq!(header.split(',').count(), row.split(',').count());
        assert!(row.starts_with("0.15,m,2,0.500,0.500,9.50,"), "{row}");
        assert_eq!(trim_float(0.4), "0.4");
        assert_eq!(trim_float(12.), "12");
    }
}
