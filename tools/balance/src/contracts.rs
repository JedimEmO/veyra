//! The feel contracts as gates. A band is a named range over one of the
//! numbers the harness measures for one preset matchup, read from
//! `assets/combat/contracts.ron` so a designer moves a band without
//! touching Rust; `balance check` and the crate's `contracts` test run the
//! bands' matchups on the installed tables and say which held. A band that
//! fails is a table change that broke a feel contract, as a perf gate
//! failing is a frame that got slower; neither says which side is wrong,
//! and the ledger (`docs/balance/ledger.md`) is where that is decided.

use std::collections::BTreeMap;
use std::fmt;
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::bout::{BoutResult, SideStats, Winner};

/// The bands and how many bouts they are read from.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Contracts {
    /// Bumped when a band's meaning changes, not when a number moves.
    pub version: u32,
    /// Bouts per matchup; the seeds are `seed..seed + bouts`.
    pub bouts: u32,
    /// The first seed, so the check is the same bouts every run.
    #[serde(default)]
    pub seed: u64,
    /// Seconds before a bout is a draw.
    #[serde(default = "default_cap")]
    pub cap: f32,
    /// The bands, in the order they are reported.
    pub bands: Vec<Band>,
}

fn default_cap() -> f32 {
    60.
}

/// One contract: `metric` of `matchup` lies in `low..=high`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Band {
    /// Its name in the report and the ledger.
    pub name: String,
    /// A preset matchup (`balance list`).
    pub matchup: String,
    /// What is measured.
    pub metric: Metric,
    /// The lowest value that holds.
    pub low: f64,
    /// The highest value that holds.
    pub high: f64,
    /// The range is the design's: a miss fails the check. Unchosen, the
    /// range is only where the harness measured the number on the day the
    /// band was written, and a miss is printed, not failed, so a motor or
    /// physics change does not fail CI on a number nobody chose.
    pub chosen: bool,
    /// Why the band is where it is, or that it is only where the harness
    /// measured it today.
    pub rationale: String,
    /// Bouts this band is read from when it needs more than the file's:
    /// a ratio of two small counts spreads with its seed, and a larger
    /// sample is the honest way to hold it, not a range too wide to fail.
    /// The run reads the first `bouts` of the seeds, so a band that asks
    /// for fewer or none reads what it always did.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bouts: Option<u32>,
    /// Seconds after which this band counts a bout as a draw, when it asks
    /// for a shorter wait than the file's: a fight that is simply long is
    /// held at the length the design wants, not at the length the check
    /// happens to run to. A bout that ended past it is no win and not
    /// decided for the seconds metrics.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cap: Option<f32>,
}

/// A bout that ended after `cap` seconds, read as a draw.
fn late_is_draw(bout: &BoutResult, cap: f32) -> BoutResult {
    let mut bout = bout.clone();
    if bout.time > cap {
        bout.winner = Winner::Draw;
    }
    bout
}

/// A number read from one side's book, averaged over the bouts.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SideMetric {
    /// Health and shield taken off the other side.
    Dealt,
    /// Health and shield lost.
    Taken,
    /// `Dealt` over the bout's seconds, so a weapon's rate reads the same
    /// whatever the target's pools.
    DealtPerSecond,
    /// Bolts fired.
    Shots,
    /// Bolts that struck.
    Hits,
    /// Hits over shots; a bout with no shots counts as zero.
    HitShare,
    /// Cuts begun.
    Cuts,
    /// Cuts that struck.
    CutsLanded,
    /// Cuts landed over cuts begun; a bout with no cuts counts as zero.
    CutShare,
    /// The lowest stamina any member reached.
    StaminaMin,
    /// The lowest posture any member reached.
    PostureMin,
    /// Posture breaks suffered.
    Breaks,
    /// Dodges begun.
    Dodges,
    /// Dodges an enemy attack would otherwise have landed through.
    DodgesAvoided,
    /// Hits a raised guard took a share of.
    Blocks,
    /// Hits a guard raised in time turned aside whole.
    Parries,
    /// Guards that ran out of stamina and broke.
    GuardBreaks,
    /// Abilities begun.
    Abilities,
    /// The bout second of the first cut that struck, the whole bout's
    /// length when none did, so a fight that never closes reads as long.
    FirstCut,
}

impl SideMetric {
    #[allow(clippy::cast_precision_loss, reason = "small counters")]
    fn of(self, side: &SideStats, time: f32) -> f64 {
        let share = |part: u64, whole: u64| {
            if whole == 0 {
                0.
            } else {
                part as f64 / whole as f64
            }
        };
        match self {
            Self::Dealt => f64::from(side.dealt),
            Self::Taken => f64::from(side.taken),
            Self::DealtPerSecond => {
                if time > 0. {
                    f64::from(side.dealt) / f64::from(time)
                } else {
                    0.
                }
            }
            Self::Shots => side.shots as f64,
            Self::Hits => side.hits as f64,
            Self::HitShare => share(side.hits, side.shots),
            Self::Cuts => side.cuts as f64,
            Self::CutsLanded => side.cuts_landed as f64,
            Self::CutShare => share(side.cuts_landed, side.cuts),
            Self::StaminaMin => f64::from(side.stamina_min),
            Self::PostureMin => f64::from(side.posture_min),
            Self::Breaks => side.breaks as f64,
            Self::Dodges => side.dodges as f64,
            Self::DodgesAvoided => side.dodges_avoided as f64,
            Self::Blocks => side.blocks as f64,
            Self::Parries => side.parries as f64,
            Self::GuardBreaks => side.guard_breaks as f64,
            Self::Abilities => side.abilities as f64,
            Self::FirstCut => f64::from(side.first_cut.unwrap_or(time)),
        }
    }

    fn name(self) -> &'static str {
        match self {
            Self::Dealt => "dealt",
            Self::Taken => "taken",
            Self::DealtPerSecond => "dealt_per_second",
            Self::Shots => "shots",
            Self::Hits => "hits",
            Self::HitShare => "hit_share",
            Self::Cuts => "cuts",
            Self::CutsLanded => "cuts_landed",
            Self::CutShare => "cut_share",
            Self::StaminaMin => "stamina_min",
            Self::PostureMin => "posture_min",
            Self::Breaks => "breaks",
            Self::Dodges => "dodges",
            Self::DodgesAvoided => "dodges_avoided",
            Self::Blocks => "blocks",
            Self::Parries => "parries",
            Self::GuardBreaks => "guard_breaks",
            Self::Abilities => "abilities",
            Self::FirstCut => "first_cut",
        }
    }
}

/// What a band measures.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Metric {
    /// Side `a`'s wins over all bouts, draws counted against it.
    WinRate,
    /// Bouts that reached the cap.
    DrawRate,
    /// Mean seconds of the decided bouts.
    MeanTime,
    /// A number from side `a`'s book.
    A(SideMetric),
    /// A number from side `b`'s book.
    B(SideMetric),
    /// Side `a`'s parries over the cuts side `b` began, summed over the
    /// bouts: the share of an opponent's cuts a guard turned aside whole.
    ParryShare,
    /// `metric` of the band's matchup over the same `metric` of `control`,
    /// a preset too: how much better or worse a change of one thing is
    /// against the matchup without it. A control that measures zero makes
    /// the ratio undefined, and the band fails with that said.
    Relative {
        /// The number compared.
        metric: Box<Metric>,
        /// The matchup it is compared against.
        control: String,
    },
}

impl Metric {
    /// The preset matchups this metric needs run.
    fn controls(&self) -> Vec<&str> {
        match self {
            Self::Relative { metric, control } => {
                let mut names = metric.controls();
                names.push(control);
                names
            }
            _ => Vec::new(),
        }
    }

    /// The metric over the bouts of `matchup`, with every preset's bouts
    /// in `results` for the controls.
    ///
    /// # Errors
    /// A matchup or control with no bouts in `results`.
    #[allow(clippy::cast_precision_loss, reason = "bout counts")]
    pub fn evaluate(
        &self,
        matchup: &str,
        results: &BTreeMap<String, Vec<BoutResult>>,
    ) -> anyhow::Result<f64> {
        let bouts = results
            .get(matchup)
            .ok_or_else(|| anyhow::anyhow!("no bouts of `{matchup}` were run"))?;
        let n = bouts.len().max(1) as f64;
        let rate =
            |pick: fn(&BoutResult) -> bool| bouts.iter().filter(|b| pick(b)).count() as f64 / n;
        let side = |index: usize, metric: SideMetric| {
            bouts
                .iter()
                .map(|b| metric.of(&b.sides[index], b.time))
                .sum::<f64>()
                / n
        };
        Ok(match self {
            Self::WinRate => rate(|b| b.winner == Winner::A),
            Self::DrawRate => rate(|b| b.winner == Winner::Draw),
            Self::MeanTime => {
                let decided: Vec<f64> = bouts
                    .iter()
                    .filter(|b| b.winner != Winner::Draw)
                    .map(|b| f64::from(b.time))
                    .collect();
                if decided.is_empty() {
                    f64::NAN
                } else {
                    decided.iter().sum::<f64>() / decided.len() as f64
                }
            }
            Self::A(metric) => side(0, *metric),
            Self::B(metric) => side(1, *metric),
            Self::ParryShare => {
                let parries: u64 = bouts.iter().map(|b| b.sides[0].parries).sum();
                let cuts: u64 = bouts.iter().map(|b| b.sides[1].cuts).sum();
                if cuts == 0 {
                    f64::NAN
                } else {
                    parries as f64 / cuts as f64
                }
            }
            Self::Relative { metric, control } => {
                let value = metric.evaluate(matchup, results)?;
                let base = metric.evaluate(control, results)?;
                if base == 0. { f64::NAN } else { value / base }
            }
        })
    }
}

impl fmt::Display for Metric {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::WinRate => f.write_str("a_win_rate"),
            Self::DrawRate => f.write_str("draw_rate"),
            Self::MeanTime => f.write_str("mean_time"),
            Self::A(metric) => write!(f, "a_{}", metric.name()),
            Self::B(metric) => write!(f, "b_{}", metric.name()),
            Self::ParryShare => f.write_str("parry_share"),
            Self::Relative { metric, control } => write!(f, "{metric} / {control}"),
        }
    }
}

/// What a band measured on one run.
#[derive(Debug, Clone, PartialEq)]
pub struct Verdict {
    /// The band.
    pub band: Band,
    /// What the harness measured; `NaN` when undefined.
    pub value: f64,
}

impl Verdict {
    /// Whether the value is inside the band; an undefined value is not.
    #[must_use]
    pub fn held(&self) -> bool {
        self.value >= self.band.low && self.value <= self.band.high
    }

    /// Whether this verdict fails the check: a chosen band that did not
    /// hold. A measured band that moved is reported and not failed.
    #[must_use]
    pub fn fails(&self) -> bool {
        self.band.chosen && !self.held()
    }
}

impl Contracts {
    /// The file beside the combat tables, as shipped.
    pub const FILE: &'static str = "contracts.ron";

    /// The contracts compiled in from `assets/combat/contracts.ron`, for a
    /// run with no assets directory at hand.
    ///
    /// # Panics
    /// The shipped file does not parse; the crate's tests catch that.
    #[must_use]
    pub fn shipped() -> Self {
        Self::parse(include_str!("../../../assets/combat/contracts.ron"))
            .expect("the shipped contracts parse")
    }

    /// Parse the RON text and check what the parser cannot: every band's
    /// matchup and control is a preset, and `low` is at most `high`.
    ///
    /// # Errors
    /// Any of those.
    pub fn parse(text: &str) -> anyhow::Result<Self> {
        let contracts: Self = ron::from_str(text)?;
        let presets = crate::presets();
        let known = |name: &str| presets.iter().any(|m| m.name == name);
        for band in &contracts.bands {
            anyhow::ensure!(
                band.low <= band.high,
                "band `{}`: low {} is above high {}",
                band.name,
                band.low,
                band.high
            );
            for name in std::iter::once(band.matchup.as_str()).chain(band.metric.controls()) {
                anyhow::ensure!(
                    known(name),
                    "band `{}`: `{name}` is not a preset matchup; `balance list` prints them",
                    band.name
                );
            }
        }
        Ok(contracts)
    }

    /// Read `dir/contracts.ron`.
    ///
    /// # Errors
    /// The file cannot be read or does not pass [`Contracts::parse`].
    pub fn load(dir: impl AsRef<Path>) -> anyhow::Result<Self> {
        let path = dir.as_ref().join(Self::FILE);
        let text = std::fs::read_to_string(&path)
            .map_err(|error| anyhow::anyhow!("{}: {error}", path.display()))?;
        Self::parse(&text).map_err(|error| anyhow::anyhow!("{}: {error}", path.display()))
    }

    /// Every preset the bands need run, each once, in name order.
    #[must_use]
    pub fn matchups(&self) -> Vec<String> {
        let mut names: Vec<String> = self
            .bands
            .iter()
            .flat_map(|band| {
                std::iter::once(band.matchup.clone())
                    .chain(band.metric.controls().into_iter().map(str::to_owned))
            })
            .collect();
        names.sort();
        names.dedup();
        names
    }

    /// The bouts a preset is run for: the most any band reading it asks.
    #[must_use]
    pub fn bouts_of(&self, name: &str) -> u32 {
        self.bands
            .iter()
            .filter(|band| {
                band.matchup == name || band.metric.controls().into_iter().any(|c| c == name)
            })
            .map(|band| band.bouts.unwrap_or(self.bouts))
            .max()
            .unwrap_or(self.bouts)
    }

    /// The cap a preset is run to: the longest any band reading it asks,
    /// so a band with a shorter cap can count the later ends as draws.
    #[must_use]
    pub fn cap_of(&self, name: &str) -> f32 {
        self.bands
            .iter()
            .filter(|band| {
                band.matchup == name || band.metric.controls().into_iter().any(|c| c == name)
            })
            .map(|band| band.cap.unwrap_or(self.cap))
            .fold(self.cap, f32::max)
    }

    /// Run the bands' matchups on the installed tables, `jobs` bouts at a
    /// time, and read every band.
    ///
    /// # Errors
    /// A matchup that is not a preset.
    pub fn check(&self, jobs: usize) -> anyhow::Result<Vec<Verdict>> {
        let mut results = BTreeMap::new();
        for name in self.matchups() {
            let matchup = crate::preset(&name)?;
            let bouts = crate::run_matchup(
                &matchup,
                self.seed,
                self.bouts_of(&name),
                self.cap_of(&name),
                jobs,
            );
            results.insert(name, bouts.into_iter().map(|(_, r)| r).collect());
        }
        self.read(&results)
    }

    /// Read every band from bouts already run, keyed by preset name.
    ///
    /// # Errors
    /// A band whose matchup or control has no bouts in `results`.
    pub fn read(
        &self,
        results: &BTreeMap<String, Vec<BoutResult>>,
    ) -> anyhow::Result<Vec<Verdict>> {
        self.bands
            .iter()
            .map(|band| {
                // A preset was run for the most any band asks; each band
                // reads the first of those bouts it asks for.
                let own = band.bouts.unwrap_or(self.bouts) as usize;
                let cap = band.cap.unwrap_or(self.cap);
                let clipped: BTreeMap<String, Vec<BoutResult>> = results
                    .iter()
                    .map(|(name, bouts)| {
                        (
                            name.clone(),
                            bouts
                                .iter()
                                .take(own)
                                .map(|b| late_is_draw(b, cap))
                                .collect(),
                        )
                    })
                    .collect();
                Ok(Verdict {
                    band: band.clone(),
                    value: band.metric.evaluate(&band.matchup, &clipped)?,
                })
            })
            .collect()
    }
}

/// The verdicts as an aligned table, one band a line, for the terminal and
/// for pasting into the ledger as its predicted column.
#[must_use]
pub fn table(verdicts: &[Verdict]) -> String {
    let rows: Vec<[String; 7]> = std::iter::once([
        "band".into(),
        "matchup".into(),
        "metric".into(),
        "low".into(),
        "high".into(),
        "measured".into(),
        "held".into(),
    ])
    .chain(verdicts.iter().map(|v| {
        [
            v.band.name.clone(),
            v.band.matchup.clone(),
            v.band.metric.to_string(),
            format!("{:.2}", v.band.low),
            format!("{:.2}", v.band.high),
            format!("{:.2}", v.value),
            match (v.held(), v.band.chosen) {
                (true, _) => "yes".into(),
                (false, true) => "NO".into(),
                (false, false) => "no (measured, not chosen)".into(),
            },
        ]
    }))
    .collect();
    let widths: Vec<usize> = (0..7)
        .map(|column| rows.iter().map(|row| row[column].len()).max().unwrap_or(0))
        .collect();
    rows.iter()
        .map(|row| {
            row.iter()
                .zip(&widths)
                .map(|(cell, width)| format!("{cell:<width$}"))
                .collect::<Vec<_>>()
                .join("  ")
                .trim_end()
                .to_owned()
        })
        .collect::<Vec<_>>()
        .join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bout(winner: Winner, time: f32, a_dealt: f32, b_taken: f32) -> BoutResult {
        BoutResult {
            winner,
            time,
            sides: [
                SideStats {
                    dealt: a_dealt,
                    cuts: 4,
                    cuts_landed: 3,
                    ..Default::default()
                },
                SideStats {
                    taken: b_taken,
                    ..Default::default()
                },
            ],
        }
    }

    #[test]
    fn the_shipped_contracts_name_presets_and_ordered_bands() {
        let contracts = Contracts::shipped();
        assert!(!contracts.bands.is_empty());
        assert!(contracts.bouts > 0);
        let text = "Contracts(version: 1, bouts: 2, bands: [Band(name: \"x\", matchup: \"nowhere\", metric: WinRate, low: 0.0, high: 1.0, chosen: true, rationale: \"\")])";
        let error = Contracts::parse(text).expect_err("an unknown matchup is refused");
        assert!(error.to_string().contains("nowhere"), "{error}");
        let text = "Contracts(version: 1, bouts: 2, bands: [Band(name: \"x\", matchup: \"blade-vs-blade\", metric: WinRate, low: 2.0, high: 1.0, chosen: true, rationale: \"\")])";
        assert!(
            Contracts::parse(text).is_err(),
            "an inverted band is refused"
        );
    }

    #[test]
    fn the_metrics_read_the_books_and_a_zero_control_is_undefined() {
        let mut results = BTreeMap::new();
        results.insert(
            "m".to_owned(),
            vec![
                bout(Winner::A, 2., 100., 100.),
                bout(Winner::Draw, 60., 40., 40.),
            ],
        );
        results.insert("zero".to_owned(), vec![bout(Winner::B, 1., 0., 0.)]);
        let read = |metric: Metric| metric.evaluate("m", &results).expect("evaluates");
        assert!((read(Metric::WinRate) - 0.5).abs() < 1e-9);
        assert!((read(Metric::DrawRate) - 0.5).abs() < 1e-9);
        assert!(
            (read(Metric::MeanTime) - 2.).abs() < 1e-9,
            "draws are left out"
        );
        assert!((read(Metric::A(SideMetric::Dealt)) - 70.).abs() < 1e-9);
        assert!((read(Metric::B(SideMetric::Taken)) - 70.).abs() < 1e-9);
        // The mean of each bout's own rate: 100 over 2 s and 40 over 60 s.
        let dps = read(Metric::A(SideMetric::DealtPerSecond));
        assert!((dps - f64::midpoint(50., 40. / 60.)).abs() < 1e-9, "{dps}");
        assert!((read(Metric::A(SideMetric::CutShare)) - 0.75).abs() < 1e-9);
        let relative = read(Metric::Relative {
            metric: Box::new(Metric::A(SideMetric::Dealt)),
            control: "zero".into(),
        });
        assert!(relative.is_nan());
        let mut verdict = Verdict {
            band: Band {
                name: "x".into(),
                matchup: "m".into(),
                metric: Metric::WinRate,
                low: 0.,
                high: 1.,
                chosen: true,
                rationale: String::new(),
                bouts: None,
                cap: None,
            },
            value: relative,
        };
        assert!(!verdict.held(), "an undefined value holds no band");
        assert!(verdict.fails());
        assert!(table(std::slice::from_ref(&verdict)).contains("NO"));
        verdict.band.chosen = false;
        assert!(
            !verdict.fails(),
            "a measured band reports and does not fail"
        );
        assert!(table(&[verdict]).contains("not chosen"));
    }

    #[test]
    fn a_band_with_a_shorter_cap_counts_a_late_end_as_a_draw() {
        let bouts = [bout(Winner::A, 20., 0., 0.), bout(Winner::A, 40., 0., 0.)];
        let mut results = BTreeMap::new();
        for cap in [60., 30.] {
            let clipped: Vec<BoutResult> = bouts.iter().map(|b| late_is_draw(b, cap)).collect();
            results.insert(cap.to_string(), clipped);
        }
        let read = |key: &str, metric: Metric| metric.evaluate(key, &results).expect("evaluates");
        assert!((read("60", Metric::WinRate) - 1.).abs() < 1e-9);
        assert!((read("30", Metric::WinRate) - 0.5).abs() < 1e-9);
        assert!((read("30", Metric::MeanTime) - 20.).abs() < 1e-9);
        assert!((read("30", Metric::DrawRate) - 0.5).abs() < 1e-9);
    }
}
