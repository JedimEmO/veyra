//! `balance`: the duel harness on the command line. See `README.md`.
//!
//! ```text
//! cargo run --release -p balance -- duel --matchup carbine-vs-rusher --bouts 20
//! cargo run --release -p balance -- duel --vary weapons.Pistol.interval=0.12,0.15,0.18
//! cargo run --release -p balance -- grid --spec my-grid.ron --out bouts.csv
//! cargo run --release -p balance -- duel --matchup blade-vs-blade --boxes blade:cuts-4
//! cargo run --release -p balance -- duel --matchup carbine-vs-dummy --fit veyra:module.patrol_extended_feed
//! cargo run --release -p balance -- sweep --matchup blade-vs-blade
//! cargo run --release -p balance -- list
//! cargo run --release -p balance -- check
//! cargo run --release -p balance -- worth --write docs/balance/combat-worth.md
//! ```

use std::io::Write;
use std::path::PathBuf;

use balance::contracts::{Contracts, Verdict};
use balance::grid::Variation;
use balance::{Grid, MatchupRef, SideMod, boxes, loadout, report};
use clap::{Parser, Subcommand};
use strata_registry::progression::SkillGraph;
use veyra_rules::combat_tables::CombatTables;

#[derive(Parser)]
#[command(about = "Bouts between policies over the real combat sim, as CSV")]
struct Cli {
    /// The assets directory; its `combat/` tables are the base the grid
    /// varies. Missing, the compiled-in tables are used.
    #[arg(long, default_value = "assets", global = true)]
    assets: PathBuf,
    /// Bouts run at once; each is its own app, and the rows do not depend
    /// on it.
    #[arg(long, global = true)]
    jobs: Option<usize>,
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Run named matchups, optionally over a grid of table values.
    Duel {
        /// A preset matchup; repeatable. None runs every preset.
        #[arg(long = "matchup")]
        matchups: Vec<String>,
        /// Bouts per matchup per cell.
        #[arg(long, default_value_t = 20)]
        bouts: u32,
        /// The first seed; bout `i` uses `seed + i`.
        #[arg(long, default_value_t = 1)]
        seed: u64,
        /// Seconds before a bout is a draw.
        #[arg(long, default_value_t = 60.)]
        cap: f32,
        /// A table value to vary: `path=v1,v2,...`; repeatable, the grid is
        /// the product. `balance list` prints the paths.
        #[arg(long = "vary")]
        vary: Vec<Variation>,
        /// Write the bout rows here instead of stdout; the summary then
        /// goes to stdout.
        #[arg(long)]
        out: Option<PathBuf>,
        /// Write the summary rows here.
        #[arg(long)]
        summary_out: Option<PathBuf>,
        /// Boxes on side `a`'s sheet in every named matchup, comma
        /// separated (`blade:cuts-1,blade:footwork-2` or `blade.cuts.1`),
        /// fitted at spawn; the side's weapon must be certified and its
        /// cuts unlocked by them, or it does nothing.
        #[arg(long)]
        boxes: Option<String>,
        /// The same for side `b`.
        #[arg(long)]
        b_boxes: Option<String>,
        /// A module on side `a`'s weapon in every named matchup, equipped
        /// at spawn after the boxes; repeatable. An item id, then any of
        /// its quality lines after an `@`
        /// (`veyra:module.patrol_extended_feed@capacity=610,reload_time=433`);
        /// a line left out is store grade.
        #[arg(long = "fit", value_name = "MODULE")]
        fit: Vec<String>,
        /// The same for side `b`.
        #[arg(long = "b-fit", value_name = "MODULE")]
        b_fit: Vec<String>,
        /// Side `a`'s weapon condition, 0 to 1000, set before the loadout
        /// is built: below 250 the weapon's lines slide toward the worst.
        #[arg(long)]
        condition: Option<u16>,
        /// The same for side `b`.
        #[arg(long)]
        b_condition: Option<u16>,
        /// One of side `a`'s weapon's own quality lines, `id=value` (0 to
        /// 1000), on a bought example of it with the rest at store grade;
        /// repeatable (`--line interval=900`).
        #[arg(long = "line", value_name = "ID=N")]
        line: Vec<String>,
        /// The same for side `b`.
        #[arg(long = "b-line", value_name = "ID=N")]
        b_line: Vec<String>,
        /// Abilities side `a` knows, comma separated ids of
        /// `assets/combat/abilities.ron`, slotted in order and pressed
        /// whenever the rule allows and their condition holds.
        #[arg(long, value_delimiter = ',')]
        abilities: Vec<String>,
        /// The same for side `b`.
        #[arg(long = "b-abilities", value_delimiter = ',')]
        b_abilities: Vec<String>,
    },
    /// The first hour of a new household, worked from the shipped data
    /// (`assets/balance/first_hour.ron`): what it does, in minutes, to its
    /// first made carbine and the ones that open more mounts.
    Field {
        /// The world's seed; the file's own when left out.
        #[arg(long)]
        seed: Option<u64>,
        /// Write the timeline as Markdown here.
        #[arg(long)]
        write: Option<PathBuf>,
        /// Also print the document's two questions: what a Surveyor box
        /// buys, and a made carbine against a bought one.
        #[arg(long)]
        questions: bool,
    },
    /// Every box's marginal effect: a matchup's side `a` on a baseline
    /// sheet, then each box it could learn next added alone, one row per
    /// box with the change in win rate, time to kill and damage taken.
    Sweep {
        /// A preset matchup; repeatable.
        #[arg(long = "matchup", required = true)]
        matchups: Vec<String>,
        /// Side `a`'s baseline, comma separated; the default is the sheet
        /// that opens the whole moveset (the two novices, Cuts 1 to 3,
        /// Carbine 1), since a sheet with less cannot chain or fire.
        #[arg(long)]
        boxes: Option<String>,
        /// Side `b`'s sheet; the default is `a`'s baseline when `b` is a
        /// player, so the box under test is the only difference, and the
        /// game's own sheet when `b` is one of its opponents.
        #[arg(long)]
        b_boxes: Option<String>,
        /// Bouts per row.
        #[arg(long, default_value_t = 20)]
        bouts: u32,
        /// The first seed.
        #[arg(long, default_value_t = 1)]
        seed: u64,
        /// Seconds before a bout is a draw.
        #[arg(long, default_value_t = 60.)]
        cap: f32,
        /// A module on side `a`'s weapon in every row, as for `duel`;
        /// repeatable.
        #[arg(long = "fit", value_name = "MODULE")]
        fit: Vec<String>,
        /// The same for side `b`.
        #[arg(long = "b-fit", value_name = "MODULE")]
        b_fit: Vec<String>,
        /// Side `a`'s weapon condition, 0 to 1000, set before the loadout
        /// is built: below 250 the weapon's lines slide toward the worst.
        #[arg(long)]
        condition: Option<u16>,
        /// The same for side `b`.
        #[arg(long)]
        b_condition: Option<u16>,
        /// One of side `a`'s weapon's own quality lines, `id=value` (0 to
        /// 1000), on a bought example of it with the rest at store grade;
        /// repeatable (`--line interval=900`).
        #[arg(long = "line", value_name = "ID=N")]
        line: Vec<String>,
        /// The same for side `b`.
        #[arg(long = "b-line", value_name = "ID=N")]
        b_line: Vec<String>,
        /// Abilities side `a` knows, comma separated ids of
        /// `assets/combat/abilities.ron`, slotted in order and pressed
        /// whenever the rule allows and their condition holds.
        #[arg(long, value_delimiter = ',')]
        abilities: Vec<String>,
        /// The same for side `b`.
        #[arg(long = "b-abilities", value_delimiter = ',')]
        b_abilities: Vec<String>,
    },
    /// Run a grid from a RON file (the `balance::Grid` type).
    Grid {
        /// The spec.
        #[arg(long)]
        spec: PathBuf,
        /// As for `duel`.
        #[arg(long)]
        out: Option<PathBuf>,
        /// As for `duel`.
        #[arg(long)]
        summary_out: Option<PathBuf>,
    },
    /// Print the preset matchups and every path a grid may vary.
    List,
    /// Run the feel contracts of `combat/contracts.ron` on the tables and
    /// print each band with what it measured; exit 1 when one fails.
    Check,
    /// Write the page of what each weapon and ability is worth
    /// (`docs/balance/combat-worth.md`), from bouts of the presets it names.
    Worth {
        /// Bouts per preset.
        #[arg(long, default_value_t = 60)]
        bouts: u32,
        /// The first seed.
        #[arg(long, default_value_t = 1)]
        seed: u64,
        /// Seconds before a bout is a draw.
        #[arg(long, default_value_t = 60.)]
        cap: f32,
        /// Write the page here instead of stdout.
        #[arg(long)]
        write: Option<PathBuf>,
    },
}

/// `--line id=value` pairs as the lines of a weapon record.
fn own_lines(specs: &[String]) -> anyhow::Result<Vec<(String, u16)>> {
    specs
        .iter()
        .map(|spec| {
            let (id, value) = spec
                .split_once('=')
                .ok_or_else(|| anyhow::anyhow!("`{spec}`: a line is `id=value`"))?;
            let value = value
                .trim()
                .parse()
                .map_err(|_| anyhow::anyhow!("`{spec}`: a quality is 0 to 1000"))?;
            Ok((id.trim().to_owned(), value))
        })
        .collect()
}

/// Give `side` a record of its weapon with `lines` set, over any the
/// preset already gave it; no pairs leaves the side as it was.
fn set_lines(side: &mut balance::Side, lines: &[(String, u16)]) {
    if !lines.is_empty() {
        side.lines
            .get_or_insert_with(Default::default)
            .extend(lines.iter().cloned());
    }
}

#[allow(
    clippy::too_many_lines,
    reason = "the subcommands, the sinks and the run, in one place"
)]
fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();
    // Eight at most unless asked: sixteen bouts at once for minutes on end is
    // the load this project's tests keep under, as its `contracts` test does.
    let jobs = cli
        .jobs
        .unwrap_or_else(|| std::thread::available_parallelism().map_or(1, |n| n.get().min(8)));
    let combat = cli.assets.join("combat");
    let base = if combat.is_dir() {
        CombatTables::load(&combat)?
    } else {
        eprintln!(
            "balance: {} is not a directory; using the compiled-in tables",
            combat.display()
        );
        CombatTables::shipped()
    };
    // The graph the boxed sides are fitted against, from the same assets
    // directory, so a tree edit sweeps as it plays.
    let trees = cli.assets.join("progression").join("trees.ron");
    if trees.is_file() {
        let text = std::fs::read_to_string(&trees)
            .map_err(|error| anyhow::anyhow!("{}: {error}", trees.display()))?;
        let graph = SkillGraph::parse(&text)
            .map_err(|error| anyhow::anyhow!("{}: {error}", trees.display()))?;
        boxes::install(graph);
    }
    // The items the sides' modules are read against, from the same
    // directory. Only a run that fits something needs them, so a data set
    // that does not load is that run's error and no other's.
    let items = strata_registry::load_from_dir(&cli.assets).map(|registries| registries.items);
    let fits = |specs: &[String]| -> anyhow::Result<Vec<SideMod>> {
        if let (Err(error), false) = (&items, specs.is_empty()) {
            anyhow::bail!("{}: {error}", cli.assets.display());
        }
        specs.iter().map(|spec| loadout::parse(spec)).collect()
    };
    if let Ok(items) = &items {
        loadout::install(items.clone());
    }
    let (grid, out, summary_out) = match cli.command {
        Command::Field {
            seed,
            write,
            questions,
        } => {
            return run_field(&cli.assets, seed, write.as_deref(), questions);
        }
        Command::Worth {
            bouts,
            seed,
            cap,
            write,
        } => {
            veyra_rules::combat_tables::install(base);
            let page = balance::worth::page(&cli.assets, bouts, seed, cap, jobs)?;
            match write {
                Some(path) => std::fs::write(&path, page)
                    .map_err(|error| anyhow::anyhow!("{}: {error}", path.display()))?,
                None => print!("{page}"),
            }
            return Ok(());
        }
        Command::Check => {
            let contracts = if combat.is_dir() {
                Contracts::load(&combat)?
            } else {
                Contracts::shipped()
            };
            veyra_rules::combat_tables::install(base);
            let started = std::time::Instant::now();
            let verdicts = contracts.check(jobs)?;
            println!("{}", balance::contracts::table(&verdicts));
            let failed: Vec<&str> = verdicts
                .iter()
                .filter(|v| Verdict::fails(v))
                .map(|v| v.band.name.as_str())
                .collect();
            eprintln!(
                "balance: {} bands over {} bouts of {} matchups in {:.2} s, {} at a time",
                verdicts.len(),
                contracts.bouts,
                contracts.matchups().len(),
                started.elapsed().as_secs_f64(),
                jobs
            );
            if failed.is_empty() {
                return Ok(());
            }
            anyhow::bail!("{} band(s) failed: {}", failed.len(), failed.join(", "));
        }
        Command::List => {
            println!("matchups:");
            for matchup in balance::presets() {
                println!("  {}", matchup.name);
            }
            println!("paths:");
            for path in balance::grid::paths() {
                println!("  {path}");
            }
            return Ok(());
        }
        Command::Sweep {
            matchups,
            boxes: a_boxes,
            b_boxes,
            bouts,
            seed,
            cap,
            fit,
            b_fit,
            condition,
            b_condition,
            line,
            b_line,
            abilities,
            b_abilities,
        } => {
            let (a_lines, b_lines) = (own_lines(&line)?, own_lines(&b_line)?);
            let started = std::time::Instant::now();
            let mut rows = 0;
            let a_boxes = a_boxes.as_deref().map(boxes::parse);
            let b_boxes = b_boxes.as_deref().map(boxes::parse);
            let (a_fit, b_fit) = (fits(&fit)?, fits(&b_fit)?);
            for name in &matchups {
                let mut matchup = balance::preset(name)?;
                matchup.a.boxes = a_boxes
                    .clone()
                    .unwrap_or_else(|| boxes::MOVESET.map(str::to_owned).to_vec());
                matchup.b.boxes = b_boxes
                    .clone()
                    .unwrap_or_else(|| boxes::opponent_boxes(&matchup));
                matchup.a.fitted.extend(a_fit.iter().cloned());
                matchup.b.fitted.extend(b_fit.iter().cloned());
                matchup.a.condition = condition;
                matchup.b.condition = b_condition;
                set_lines(&mut matchup.a, &a_lines);
                set_lines(&mut matchup.b, &b_lines);
                matchup.a.abilities.extend(abilities.iter().cloned());
                matchup.b.abilities.extend(b_abilities.iter().cloned());
                println!(
                    "{name}: a on {}; b on {}",
                    matchup.a.boxes.join(", "),
                    if matchup.b.boxes.is_empty() {
                        "the installed tables, unfitted".to_owned()
                    } else {
                        matchup.b.boxes.join(", ")
                    }
                );
                println!("{}", boxes::header());
                let mut base = None;
                boxes::sweep(&matchup, seed, bouts, cap, jobs, |row| {
                    let reading = *base.get_or_insert(row.reading);
                    println!("{}", boxes::line(row, &reading));
                    rows += 1;
                })?;
                println!();
            }
            eprintln!(
                "balance: {rows} rows of {bouts} bouts in {:.2} s, {jobs} at a time",
                started.elapsed().as_secs_f64()
            );
            return Ok(());
        }
        Command::Duel {
            matchups,
            bouts,
            seed,
            cap,
            vary,
            out,
            summary_out,
            boxes: a_boxes,
            b_boxes,
            fit,
            b_fit,
            condition,
            b_condition,
            line,
            b_line,
            abilities,
            b_abilities,
        } => {
            let (a_lines, b_lines) = (own_lines(&line)?, own_lines(&b_line)?);
            let (a_fit, b_fit) = (fits(&fit)?, fits(&b_fit)?);
            let matchups = if matchups.is_empty() {
                balance::presets()
            } else {
                // Resolved here so an unknown name is refused before a run.
                matchups
                    .iter()
                    .map(|name| balance::preset(name))
                    .collect::<Result<Vec<_>, _>>()?
            };
            let a_boxes = a_boxes.as_deref().map(boxes::parse);
            let b_boxes = b_boxes.as_deref().map(boxes::parse);
            let matchups = matchups
                .into_iter()
                .map(|mut matchup| {
                    if let Some(boxes) = &a_boxes {
                        matchup.a.boxes.clone_from(boxes);
                    }
                    if let Some(boxes) = &b_boxes {
                        matchup.b.boxes.clone_from(boxes);
                    }
                    matchup.a.fitted.extend(a_fit.iter().cloned());
                    matchup.b.fitted.extend(b_fit.iter().cloned());
                    matchup.a.condition = condition;
                    matchup.b.condition = b_condition;
                    set_lines(&mut matchup.a, &a_lines);
                    set_lines(&mut matchup.b, &b_lines);
                    matchup.a.abilities.extend(abilities.iter().cloned());
                    matchup.b.abilities.extend(b_abilities.iter().cloned());
                    MatchupRef::Full(matchup)
                })
                .collect();
            (
                Grid {
                    vary: vary.into_iter().map(|v| (v.path, v.values)).collect(),
                    matchups,
                    bouts,
                    seed,
                    cap,
                },
                out,
                summary_out,
            )
        }
        Command::Grid {
            spec,
            out,
            summary_out,
        } => {
            let text = std::fs::read_to_string(&spec)
                .map_err(|error| anyhow::anyhow!("{}: {error}", spec.display()))?;
            let grid: Grid = ron::from_str(&text)
                .map_err(|error| anyhow::anyhow!("{}: {error}", spec.display()))?;
            (grid, out, summary_out)
        }
    };
    let paths: Vec<String> = grid.vary.iter().map(|(path, _)| path.clone()).collect();

    let stdout = std::io::stdout();
    let stderr = std::io::stderr();
    let mut bouts_sink: Box<dyn Write> = match &out {
        Some(path) => Box::new(std::io::BufWriter::new(std::fs::File::create(path)?)),
        None => Box::new(stdout.lock()),
    };
    // With the bouts on stdout the summary goes to stderr, so a pipe gets
    // one table; with them in a file stdout is free for the summary.
    let mut summary_sink: Box<dyn Write> = match (&summary_out, &out) {
        (Some(path), _) => Box::new(std::io::BufWriter::new(std::fs::File::create(path)?)),
        (None, Some(_)) => Box::new(stdout.lock()),
        (None, None) => Box::new(stderr.lock()),
    };
    writeln!(bouts_sink, "{}", report::bout_header(&paths))?;
    writeln!(summary_sink, "{}", report::summary_header(&paths))?;
    let mut failure = None;
    let mut total = (0_u32, 0_f64);
    balance::run_grid(&grid, &base, jobs, |cell| {
        if failure.is_some() {
            return;
        }
        let mut write = || -> anyhow::Result<()> {
            for (seed, result) in &cell.bouts {
                writeln!(
                    bouts_sink,
                    "{}",
                    report::bout_row(&cell.cell, &cell.matchup, *seed, result)
                )?;
            }
            let results: Vec<_> = cell.bouts.iter().map(|(_, r)| r.clone()).collect();
            writeln!(
                summary_sink,
                "{}",
                report::summary_row(&cell.cell, &cell.matchup, &results)
            )?;
            bouts_sink.flush()?;
            summary_sink.flush()?;
            Ok(())
        };
        if let Err(error) = write() {
            failure = Some(error);
        }
        #[allow(clippy::cast_possible_truncation, reason = "a bout count")]
        {
            total.0 += cell.bouts.len() as u32;
        }
        total.1 += cell.seconds;
    })?;
    if let Some(error) = failure {
        return Err(error);
    }
    if total.0 > 0 {
        eprintln!(
            "balance: {} bouts in {:.2} s, {:.1} ms per bout wall, {} at a time",
            total.0,
            total.1,
            total.1 * 1000. / f64::from(total.0),
            jobs
        );
    }
    Ok(())
}

/// `balance field`: the first hour, printed and optionally written.
fn run_field(
    assets: &std::path::Path,
    seed: Option<u64>,
    write: Option<&std::path::Path>,
    questions: bool,
) -> anyhow::Result<()> {
    use balance::field;
    let registries = strata_registry::load_from_dir(assets)?;
    let file = field::FirstHour::load(assets, &registries)?;
    let shipped = field::Shipped::open(assets, seed.unwrap_or(file.seed))?;
    let timeline = field::run(
        &shipped.registries,
        &shipped.field,
        &file,
        shipped.home,
        shipped.foot,
        &field::Variant::default(),
    );
    let harvest = field::harvest_numbers(&shipped.registries, &file, "Ilse Marrow")?;
    println!("{}", field::table(&timeline));
    println!("{}", field::bands_table(&file, &timeline));
    println!(
        "harvester: {} units an hour, worth {:.1} min of a hand ({:.0} a minute), hopper fills in {:.2} h",
        harvest.units_hour, harvest.worth_min, harvest.hand_per_min, harvest.hopper_hours
    );
    let (failed, waived) = field::judge(&file, &timeline, Some(harvest));
    for line in &failed {
        println!("FAILS {line}");
    }
    for line in &waived {
        println!("WAIVED {line}");
    }
    let extra = if questions {
        field::questions(&shipped, &file)
    } else {
        String::new()
    };
    if questions {
        println!("{extra}");
    }
    if let Some(path) = write {
        let mut text = format!(
            "# The first hour (seed {})\n\n{}\n{}\n",
            seed.unwrap_or(file.seed),
            field::markdown(&timeline),
            field::bands_table(&file, &timeline)
        );
        {
            use std::fmt::Write as _;
            write!(
                text,
                "Harvester: {} units an hour, worth {:.1} min of a hand, hopper fills in {:.2} h.\n\n",
                harvest.units_hour, harvest.worth_min, harvest.hopper_hours
            )?;
        }
        text.push_str(&extra);
        std::fs::write(path, text)?;
    }
    Ok(())
}
