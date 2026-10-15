//! What a cleared camp leaves and what a fall costs, in words (ticket 0012,
//! step 4a). The sim decides both: [`strata_sim::loot`] what a take moved,
//! and the game's fall what the bench does. Here the answers become the
//! toasts, and F beside a cache becomes the request.
//!
//! Nothing here is a rule. Local-only: nothing is replicated.

use bevy::ecs::prelude::*;
use strata_registry::GameRegistries;
use strata_registry::item::Record;
use strata_sim::Registries;
use strata_sim::loot::{CacheAnswer, CacheRefusal, CacheTaken};

use super::strings::{counted, english, grouped};

fn words(key: &str) -> &'static str {
    english(key).unwrap_or("")
}

fn fill(text: &str, name: &str, with: &str) -> String {
    text.replace(&format!("{{{name}}}"), with)
}

/// What a take moved, as one line: `Cache: 340 cr, 2 drive coils, 30 units
/// of salt`, with a second sentence when part of it had no room.
#[must_use]
pub fn cache_words(registries: &GameRegistries, taken: &CacheTaken) -> String {
    let mut parts = Vec::new();
    if taken.credits > 0 {
        parts.push(fill(words("strata.board.cr"), "n", &grouped(taken.credits)));
    }
    // A lot is split into stacks of the type's size: the line names each
    // kind once, with the sum.
    let mut order: Vec<(&Record, u32)> = Vec::new();
    for record in &taken.took {
        match order.iter_mut().find(|(seen, _)| seen.item == record.item) {
            Some((_, count)) => *count += record.count,
            None => order.push((record, record.count)),
        }
    }
    for (record, count) in order {
        let def = registries.items.get(&record.item);
        let name = def.map_or_else(|| record.item.to_string(), |def| def.name.clone());
        let lot =
            def.is_some_and(|def| matches!(def.kind, strata_registry::item::ItemKind::Lot { .. }));
        parts.push(if lot {
            fill(
                &fill(words("strata.board.units.of"), "n", &grouped(count)),
                "name",
                &name,
            )
        } else {
            format!("{} {}", grouped(count), counted(&name, count))
        });
    }
    let mut said = fill(words("strata.cache.toast"), "what", &parts.join(", "));
    if taken.left > 0 {
        said.push(' ');
        said.push_str(&fill(
            words("strata.cache.left"),
            "n",
            &taken.left.to_string(),
        ));
    }
    said
}

/// Why a take did nothing.
#[must_use]
pub fn refusal_words(refusal: CacheRefusal) -> String {
    words(match refusal {
        CacheRefusal::NoPlayer => "strata.cache.why.player",
        CacheRefusal::NoCache => "strata.cache.why.gone",
        CacheRefusal::NotInReach => "strata.cache.why.reach",
        CacheRefusal::NoRoom => "strata.cache.why.room",
    })
    .to_owned()
}

/// A span of played minutes in the long form a sentence uses: `2 hours`,
/// `1 hour 40 minutes`, `35 minutes`.
#[must_use]
pub fn span_words(minutes: u32) -> String {
    let (h, m) = (minutes / 60, minutes % 60);
    let said = |one: &str, many: &str, n: u32| {
        if n == 1 {
            words(one).to_owned()
        } else {
            fill(words(many), "n", &n.to_string())
        }
    };
    let hours = said("strata.fall.hour.one", "strata.fall.hour.many", h);
    let mins = said("strata.fall.minute.one", "strata.fall.minute.many", m);
    match (h, m) {
        (0, _) => mins,
        (_, 0) => hours,
        _ => format!("{hours} {mins}"),
    }
}

/// The same in the short form the bench card uses: `1 h 40 min`, `2 h`,
/// `35 min`.
#[must_use]
pub fn short_span(minutes: u32) -> String {
    let (h, m) = (minutes / 60, minutes % 60);
    let hours = fill(words("strata.fall.short.h"), "n", &h.to_string());
    let mins = fill(words("strata.fall.short.m"), "n", &m.to_string());
    match (h, m) {
        (0, _) => mins,
        (_, 0) => hours,
        _ => format!("{hours} {mins}"),
    }
}

/// The bench card's line for a member still hurt: `Injured: back in 1 h
/// 40 min`.
#[must_use]
pub fn injured_words(minutes: u32) -> String {
    fill(words("strata.bench.injured"), "time", &short_span(minutes))
}

/// Who is out in place of the one who fell, as the toast says it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FallCover {
    /// The next able member went out.
    Able(String),
    /// Nobody was able and the soonest to heal got up early.
    Early(String),
    /// Nobody else is on the bench: the fallen member is back, whole.
    Alone,
}

/// The toast on arriving home after a fall: who is out of it for how long,
/// and who is out now. Nothing is said of what was carried because nothing
/// is lost, which the first words say.
#[must_use]
pub fn fall_words(fallen: &str, minutes: u32, cover: &FallCover) -> String {
    let home = words("strata.fall.home");
    let rest = match cover {
        FallCover::Able(name) => fill(
            &fill(words("strata.fall.out"), "name", fallen),
            "time",
            &span_words(minutes),
        )
        .replace("{other}", name),
        FallCover::Early(name) => fill(
            &fill(words("strata.fall.early"), "name", fallen),
            "time",
            &span_words(minutes),
        )
        .replace("{other}", name),
        FallCover::Alone => fill(words("strata.fall.alone"), "name", fallen),
    };
    format!("{home} {rest}")
}

/// `Update`, always: the sim's answer to a take as a toast.
pub fn hear_caches(
    mut answers: MessageReader<CacheAnswer>,
    registries: Option<Res<Registries>>,
    mut commands: Commands,
) {
    for answer in answers.read() {
        let said = match (&answer.outcome, registries.as_deref()) {
            (Ok(taken), Some(registries)) => cache_words(&registries.0, taken),
            (Ok(_), None) => continue,
            (Err(refusal), _) => refusal_words(*refusal),
        };
        tracing::info!("cache: {said}");
        super::progress::notice(&mut commands, said);
    }
}

/// Registers the toast. Only called once the UI is wired.
pub(crate) fn register(app: &mut bevy::app::App) {
    app.add_message::<CacheAnswer>()
        .add_systems(bevy::app::Update, hear_caches);
}
