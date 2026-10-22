//! A fall in the world (ticket 0012, step 4a): the member who is brought
//! down at a camp or a post is carried home behind a fade, hurt and out of
//! play for played hours, and the bench puts the next member who is able
//! out. Nothing carried is lost and nobody walks back for anything.
//!
//! The rules are the sim's ([`strata::sim::fall`]) and the dials are
//! `assets/household/fall.ron`; this is the household's side: who is hurt
//! until when, which member goes out and where they arrive. The staging
//! fight keeps F5, as before.

use strata::registry::FallRules;
use strata::sim::encounters::{EncounterId, Fights, NearEncounters};
use strata::sim::fall::{self, Member};
use strata::sim::view::{FallFade, FallPhase, WorldNotice};
use veyra_words::fall::{FallCover, fall_words};

use super::bench::{Out, PlayerParts, put_out};
use super::props::{PropSites, terminal_view};
use super::*;

/// The frame `--fall` brings the member down on, after the world is up.
const FALL_FRAME: u32 = 200;

/// Where a fall is: nothing, or waiting for the curtain to be down.
#[derive(Resource, Debug, Default)]
pub struct FallState {
    pending: bool,
    forced: bool,
    frames: u32,
    injured_set: bool,
}

/// What a fall came to, for the toast.
#[derive(Debug, Clone, PartialEq)]
pub struct Report {
    /// Who fell.
    pub fallen: String,
    /// How long they are out, in played minutes.
    pub minutes: u32,
    /// Who is out in their place.
    pub cover: FallCover,
}

/// The minutes of injury `character` has left at played second `now`, rounded
/// up so a member never reads as hurt for nought minutes.
#[allow(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "a positive span of at most some hours, in whole minutes"
)]
pub fn injured_minutes(character: &Character, now: f64) -> Option<u32> {
    let member = Member {
        injured_until: character.injured_until,
        worker: false,
    };
    member.left_s(now).map(|left| (left / 60.).ceil() as u32)
}

/// Why a switch to `character` is refused while they are hurt, in words.
pub fn injured_refusal(character: &Character, now: f64) -> Option<String> {
    let minutes = injured_minutes(character, now)?;
    Some(
        veyra_words::english("strata.bench.refused.injured")
            .unwrap_or_default()
            .replace("{name}", &character.name)
            .replace("{time}", &veyra_words::fall::short_span(minutes)),
    )
}

/// Carry the member who is out home: hurt for the dial's hours, left at
/// `home`, and the next able member put out there with full pools; with none
/// able the soonest to heal is stood up early, and with nobody else on the
/// bench the fallen member is back at once, whole. What they carried stays
/// with them in every case.
#[allow(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "the dial's hours as whole minutes"
)]
pub fn carry_home(
    household: &mut Household,
    rules: &FallRules,
    now: f64,
    worker: Option<&str>,
    registries: &GameRegistries,
    mut player: Out<'_>,
    home: WorldPos,
) -> Report {
    let fallen = household.active;
    let members: Vec<Member> = household
        .characters
        .iter()
        .map(|character| Member {
            injured_until: character.injured_until,
            worker: worker == Some(character.name.as_str()),
        })
        .collect();
    let name = household.characters[fallen].name.clone();
    let (next, cover) = if let Some(index) = fall::able(&members, fallen, now) {
        (
            index,
            FallCover::Able(household.characters[index].name.clone()),
        )
    } else if let Some(index) = fall::soonest(&members, fallen, now) {
        household.characters[index].injured_until = Some(fall::healed_early_until(rules, now));
        (
            index,
            FallCover::Early(household.characters[index].name.clone()),
        )
    } else {
        (fallen, FallCover::Alone)
    };
    if next != fallen {
        household.characters[fallen].injured_until = Some(fall::injured_until(rules, now));
    }
    // They are left at the bench place, which is where the file finds them.
    player.position = home;
    put_out(household, next, registries, player);
    Report {
        fallen: name,
        minutes: (rules.out_s() / 60.).ceil() as u32,
        cover,
    }
}

/// Where the household's bench place is, `(x, z)`: the city's terminal when
/// the city has a layout, else the nearest terminal standing, else one
/// still waiting for its ground. `None` where there is no bench at all, and
/// the member is then left where they fell.
fn bench_spot(
    settlements: Option<&strata::sim::Settlements>,
    sites: &PropSites,
    terminals: &[(f64, f64)],
    from: (f64, f64),
) -> Option<(f64, f64)> {
    if let Some((spot, _)) = terminal_view(settlements) {
        return Some(spot);
    }
    let nearest = |points: &mut dyn Iterator<Item = (f64, f64)>| {
        points.min_by(|a, b| {
            let d = |p: &(f64, f64)| (p.0 - from.0).hypot(p.1 - from.1);
            d(a).total_cmp(&d(b))
        })
    };
    nearest(&mut terminals.iter().copied()).or_else(|| {
        nearest(
            &mut sites
                .0
                .iter()
                .filter(|site| matches!(site.kind, InteractionKind::Terminal))
                .map(|site| (site.x, site.z)),
        )
    })
}

/// Whether the member was brought down in the world: some camp or post is
/// engaged and the staging fight is not, so a death on the staging ground
/// keeps F5 even when a camp's wounded person is still engaged.
pub fn in_the_world(fights: &Fights, near: &NearEncounters) -> bool {
    let ids = near
        .camps
        .iter()
        .map(|camp| camp.id)
        .chain(near.posts.iter().map(|post| post.id));
    engaged_in_the_world(fights, ids)
}

/// [`in_the_world`] over the ids of the camps and posts that stand.
pub fn engaged_in_the_world(fights: &Fights, mut ids: impl Iterator<Item = EncounterId>) -> bool {
    !fights.engaged(EncounterId::Staging) && ids.any(|id| fights.engaged(id))
}

/// `Update`: the fall. The member out is down in the world, or `--fall`
/// says so at its frame: the curtain comes down over the dial's seconds, the
/// household is carried home behind it and the curtain goes up on the bench
/// place. Without a window there is no curtain and it is done at once.
#[allow(
    clippy::too_many_arguments,
    clippy::type_complexity,
    reason = "the household, the world's bench and the player's parts, which a fall touches together"
)]
pub fn carried_home(
    mut commands: Commands,
    mut notices: MessageWriter<WorldNotice>,
    cli: Res<crate::Launch>,
    registries: Res<strata::sim::Registries>,
    clock: Option<Res<PlayedClock>>,
    (fights, near): (Option<ResMut<Fights>>, Option<Res<NearEncounters>>),
    run: Option<Res<FactoryState>>,
    (settlements, sites, source): (
        Option<Res<strata::sim::Settlements>>,
        Res<PropSites>,
        Res<WorldSourceRes>,
    ),
    terminals: Query<(&WorldPosition, &Interaction), Without<LocalPlayer>>,
    mut fade: Option<ResMut<FallFade>>,
    mut state: ResMut<FallState>,
    mut household: ResMut<Household>,
    mut players: ParamSet<(
        Query<PlayerParts, With<LocalPlayer>>,
        Query<
            (
                Entity,
                &mut WorldPosition,
                &mut PreviousWorldPosition,
                &mut CharacterController,
            ),
            With<LocalPlayer>,
        >,
    )>,
) {
    let (Some(rules), Some(clock)) = (registries.0.fall.as_ref(), clock) else {
        return;
    };
    state.frames += 1;
    if !state.pending {
        let down = players
            .p0()
            .single()
            .is_ok_and(|player| player.1.action == Action::Dead);
        let world = fights
            .as_deref()
            .zip(near.as_deref())
            .is_some_and(|(fights, near)| in_the_world(fights, near));
        let forced = cli.fall && !state.forced && state.frames >= FALL_FRAME;
        if !((down && world) || forced) {
            return;
        }
        state.pending = true;
        state.forced |= forced;
        if let Some(fade) = fade.as_deref_mut() {
            fade.begin(rules.fade_s);
            return;
        }
    } else if fade
        .as_deref()
        .is_some_and(|fade| fade.phase != FallPhase::Black)
    {
        return;
    }
    state.pending = false;
    let worker = run
        .as_deref()
        .and_then(|run| run.job.as_ref())
        .and_then(|job| job.worker.clone());
    let from = players
        .p0()
        .single()
        .map(|player| (player.0.0.x(), player.0.0.z()))
        .unwrap_or_default();
    let standing: Vec<(f64, f64)> = terminals
        .iter()
        .filter(|(_, interaction)| matches!(interaction.kind, InteractionKind::Terminal))
        .map(|(at, _)| (at.0.x(), at.0.z()))
        .collect();
    let spot = bench_spot(settlements.as_deref(), &sites, &standing, from);
    let mut parts = players.p0();
    let Ok(mut player) = parts.single_mut() else {
        return;
    };
    let here = player.0.0;
    // Above the ground the world's own sample gives; the character settles
    // onto the streamed ground, or waits for it, as at a start.
    let home = spot.map_or(here, |(x, z)| {
        WorldPos::new(x, f64::from(source.source().sample(x, z).height) + 1., z)
    });
    // The injury ends on the clock's second; the saved clock must not lag it.
    household.played = PlayedClock {
        seconds: clock.seconds,
    };
    let report = carry_home(
        &mut household,
        rules,
        clock.seconds,
        worker.as_deref(),
        &registries.0,
        out!(player),
        home,
    );
    if let Ok((rider, mut at, mut before, mut controller)) = players.p1().single_mut() {
        at.0 = home;
        before.0 = None;
        controller.vertical_velocity = 0.;
        controller.horizontal_velocity = Vec3::ZERO;
        controller.action_velocity = None;
        commands
            .entity(rider)
            .remove::<(strata::sim::ride::Riding, strata::sim::ride::Calling)>();
    }
    // The camps keep their ground and the fight is let go, so the HUD and
    // the locks do not hold on a fight the member is no longer in.
    if let Some(mut fights) = fights {
        fights.abandon_camps();
    }
    let said = fall_words(&report.fallen, report.minutes, &report.cover);
    info!("household: {said}");
    if let Some(fade) = fade.as_deref_mut() {
        fade.wake();
        notices.write(WorldNotice(said));
    }
}

/// `Update`, `--injured`: the named member starts hurt for the dial's hours,
/// once, so a still can show the bench card's line.
pub fn injured_at_start(
    cli: Res<crate::Launch>,
    registries: Res<strata::sim::Registries>,
    clock: Option<Res<PlayedClock>>,
    mut state: ResMut<FallState>,
    mut household: ResMut<Household>,
) {
    let (Some(name), Some(rules), Some(clock)) =
        (cli.injured.as_deref(), registries.0.fall.as_ref(), clock)
    else {
        return;
    };
    if state.injured_set {
        return;
    }
    state.injured_set = true;
    let until = Some(fall::injured_until(rules, clock.seconds));
    let household = &mut *household;
    let found = household
        .characters
        .iter_mut()
        .chain(household.recruits.iter_mut())
        .find(|character| character.name.eq_ignore_ascii_case(name));
    if let Some(character) = found {
        character.injured_until = until;
    } else {
        warn!("--injured: nobody on the bench is called {name}");
    }
}
