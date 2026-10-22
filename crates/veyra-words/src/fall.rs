//! What a fall costs, in words (ticket 0012, step 4a): the span of played time
//! a member is out, the bench card's line for one still hurt and the toast on
//! arriving home. Here and not with the screens because the household, which
//! has no UI, words the refusal and the toast itself.

use crate::english;

fn words(key: &str) -> &'static str {
    english(key).unwrap_or("")
}

fn fill(text: &str, name: &str, with: &str) -> String {
    text.replace(&format!("{{{name}}}"), with)
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
