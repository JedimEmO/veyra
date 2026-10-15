//! Deposits in the game (ticket 0007, step 3; ticket 0009, step 3): the
//! world's deposits handed to the sim.
//!
//! Where deposits are is `strata::worldgen::deposits`, a pure function of
//! the seed, the resource registry and the ground. [`init`] builds it over
//! the loader's world and publishes it as the sim's `Survey`, from which
//! the sim keeps `NearDeposits` and a ping reads. Nothing is stood in the
//! world: a deposit is found with the survey tool, and `--at deposit` only
//! puts a still or a test at the edge of one.

use std::sync::Arc;

use bevy::prelude::*;
use strata::prelude::*;
use strata::registry::Namespaced;
use strata::sim::ProceduralWorldRes;
use strata::sim::deposits::{Deposit, DepositField, Survey};

/// Publishes the world's deposits to the sim, once the world and its
/// registries exist. A data set with no resources has none, and neither
/// has a run with no household to draw from them unless it starts at one:
/// the sim lists the deposits round every observer it is given a survey
/// for, which a flight or a benchmark has no use for.
pub(super) fn init(
    mut commands: Commands,
    cli: Res<super::Cli>,
    world: Option<Res<ProceduralWorldRes>>,
    seed: Option<Res<WorldSeed>>,
    registries: Res<strata::sim::Registries>,
) {
    let (Some(world), Some(seed)) = (world, seed) else {
        return;
    };
    if registries.0.resources.is_empty()
        || !(cli.combat
            || super::at_deposit(&cli).is_some()
            || cli
                .at
                .as_deref()
                .and_then(super::camps::claim_spec)
                .is_some())
    {
        return;
    }
    let field = DepositField::new(Arc::clone(&world.0), seed.0, Arc::clone(&registries.0));
    info!(
        "deposits: {} types on a {:.0} m lattice",
        registries.0.resources.types().len(),
        registries.0.resources.rules().lattice.cell_m
    );
    commands.insert_resource(Survey(Arc::new(field)));
}

/// The type `--at deposit[:type]` asks for: `None` for any, else the id,
/// taken as the `strata` namespace's when it names none. `Err` for a word
/// that is no id.
fn wanted_type(spec: &str) -> Result<Option<Namespaced>, String> {
    let Some(name) = spec.strip_prefix("deposit") else {
        return Err(format!("`{spec}` is no deposit"));
    };
    let Some(name) = name.strip_prefix(':') else {
        return if name.is_empty() {
            Ok(None)
        } else {
            Err(format!("`{spec}` is no deposit"))
        };
    };
    let full = if name.contains(':') {
        name.to_owned()
    } else {
        format!("strata:{name}")
    };
    full.parse()
        .map(Some)
        .map_err(|_| format!("`{name}` is no resource type's id"))
}

/// Whether `--at`'s value asks for a deposit, and that it reads.
///
/// # Errors
/// A `deposit:` with a word that is no id.
pub(super) fn is_deposit(spec: &str) -> Result<bool, String> {
    if !spec.starts_with("deposit") {
        return Ok(false);
    }
    wanted_type(spec).map(|_| true)
}

/// How far from a deposit's centre `--at deposit` stands, as a share of its
/// radius: well inside the area's edge, where the concentration is a tenth
/// of the peak or so, so a ping from here reads it and a find can be
/// sampled, and the walk to the middle is still a search.
const START_SHARE: f64 = 0.8;

/// The nearest deposit to `from` within the register's range: of any
/// type for `None`, else of the type `wanted` names. `None`, said, for a
/// type the data set has not got and when there is no such deposit.
fn nearest(survey: &Survey, wanted: Option<&Namespaced>, from: (f64, f64)) -> Option<Deposit> {
    let field = &survey.0;
    let set = field.set();
    if let Some(name) = wanted
        && set.type_id(name).is_none()
    {
        warn!("deposits: the data set has no resource type `{name}`");
        return None;
    }
    let ty = wanted.and_then(|name| set.type_id(name));
    let range = set.rules().register.range_m;
    let found = field.nearest(from.0, from.1, range, |d| ty.is_none_or(|ty| d.ty == ty));
    if found.is_none() {
        warn!(
            "deposits: no such deposit within {:.0} km of the default start",
            range / 1000.0
        );
    }
    found
}

/// `--track <type>`: the nearest deposit of the type to `from`. The type
/// is an id, taken as the `strata` namespace's when it names none.
pub(super) fn nearest_of(survey: &Survey, ty: &str, from: (f64, f64)) -> Option<Deposit> {
    let wanted = wanted_type(&format!("deposit:{ty}")).ok()??;
    nearest(survey, Some(&wanted), from)
}

/// `--at deposit` and `--at deposit:<type>`: where to start and which way
/// to look. The nearest deposit to `from` (of the type, when one is named)
/// within the register's range, south of its centre inside the edge of its
/// area ([`START_SHARE`] of the radius, or nearer the centre where that is
/// water), facing north to it. `None`, said, when there is none.
pub(super) fn start_at(
    spec: &str,
    survey: Option<&Survey>,
    from: (f64, f64),
) -> Option<((f64, f64), f32)> {
    use strata::prelude::WorldSource;
    let survey = survey?;
    let set = survey.0.set();
    let wanted = wanted_type(spec).ok()?;
    let deposit = nearest(survey, wanted.as_ref(), from)?;
    info!(
        "deposits: starting at {} ({}), {:.0} m from the default start",
        deposit.place,
        deposit.lot_name(set),
        deposit.distance_m(from.0, from.1)
    );
    let world = survey.0.world();
    let sea = f64::from(world.sea_level());
    let shore = set.rules().refuse.shore_m;
    let radius = f64::from(deposit.radius_m);
    // North is `+z`, so due south of the centre looks at it with a heading
    // of half a turn. Nearer the centre until the ground is dry.
    let spot = (0..=8)
        .map(|step| deposit.at[1] - radius * START_SHARE * (1.0 - f64::from(step) / 8.0))
        .map(|z| (deposit.at[0], z))
        .find(|&(x, z)| {
            let sample = world.sample(x, z);
            !sample.in_water() && f64::from(sample.height) >= sea + shore
        })
        .unwrap_or((deposit.at[0], deposit.at[1]));
    Some((spot, std::f32::consts::PI))
}

/// How far from the nearest deposit's centre `--script-survey` starts,
/// metres: past the ping's reach, so the first pings meet nothing, and short
/// enough to be found in a few legs.
const SURVEY_START_M: f64 = 400.0;

/// How far east of the centre's meridian `--script-survey` starts, metres,
/// so the heading north meets the area off its centre and the script must
/// turn to the lead to reach the peak.
const SURVEY_SIDE_M: f64 = 60.0;

/// Where `--script-survey` starts: due south of the centre of the nearest
/// deposit of the class, [`SURVEY_START_M`] off and [`SURVEY_SIDE_M`] east, or
/// the nearest metres to it that are dry, so a heading north passes beside
/// it. The script does not know this: it walks the heading it starts with.
pub(super) fn survey_start(
    class: &str,
    survey: Option<&Survey>,
    from: (f64, f64),
) -> Option<(f64, f64)> {
    use strata::prelude::WorldSource;
    let survey = survey?;
    let set = survey.0.set();
    let class = crate::survey_script::class_of(class).ok()?;
    let range = set.rules().register.range_m;
    let deposit = survey.0.nearest(from.0, from.1, range, |deposit| {
        set.get(deposit.ty).is_some() && set.class_of(deposit.ty).id == class
    });
    let Some(deposit) = deposit else {
        warn!("deposits: no deposit of class {class} within {range:.0} m of the default start");
        return None;
    };
    info!(
        "deposits: --script-survey starts near {} ({}), {:.0} m from the default start",
        deposit.place,
        deposit.lot_name(set),
        deposit.distance_m(from.0, from.1)
    );
    let world = survey.0.world();
    let sea = f64::from(world.sea_level());
    let shore = set.rules().refuse.shore_m;
    [0.0, 20.0, -20.0, 40.0, -40.0, 60.0, -60.0, 80.0, -80.0]
        .into_iter()
        .map(|off| {
            (
                deposit.at[0] + SURVEY_SIDE_M,
                deposit.at[1] - SURVEY_START_M - off,
            )
        })
        .find(|&(x, z)| {
            let sample = world.sample(x, z);
            !sample.in_water() && f64::from(sample.height) >= sea + shore
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn at_deposit_reads_any_or_a_type() {
        assert_eq!(wanted_type("deposit"), Ok(None));
        assert_eq!(
            wanted_type("deposit:sulphur"),
            Ok(Some("strata:sulphur".parse().expect("valid")))
        );
        assert_eq!(
            wanted_type("deposit:veyra:glass"),
            Ok(Some("veyra:glass".parse().expect("valid")))
        );
        assert!(wanted_type("deposit:Not An Id").is_err());
        assert!(wanted_type("deposits").is_err());
        assert_eq!(is_deposit("city"), Ok(false));
        assert_eq!(is_deposit("deposit"), Ok(true));
        assert_eq!(is_deposit("deposit:sulphur"), Ok(true));
        assert!(is_deposit("deposit:").is_err());
        assert!(start_at("deposit", None, (0., 0.)).is_none());
    }
}
