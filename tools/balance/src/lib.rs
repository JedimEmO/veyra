//! The duel harness of the progression design, section 7: bouts between
//! policies over the real `veyra-sim` and `strata-physics` combat, run
//! headless as fast as the CPU allows over a grid of combat table values,
//! and reported as CSV. The binary is `balance`; `README.md` has the CLI.
//!
//! The one model: nothing here computes damage, cadence, stamina or
//! motion. The policies write inputs, the sim's `CombatPlugin` and the
//! physics crate's `resolve_attacks`, bolts and character motor do the
//! rest, and the recorder watches. A number the harness predicts is the
//! number the game produces for the same inputs.

pub mod bout;
pub mod boxes;
pub mod contracts;
pub mod field;
pub mod grid;
pub mod loadout;
pub mod policy;
pub mod report;
pub mod worth;

use std::collections::BTreeMap;
use std::time::Instant;

use serde::{Deserialize, Serialize};
use veyra_rules::combat::Weapon;
use veyra_rules::combat_tables::CombatTables;

pub use bout::{BoutResult, Matchup, Profile, Side, SideMod, SideStats, Winner};
pub use contracts::{Band, Contracts, Metric, SideMetric, Verdict};
pub use grid::{Cell, Variation};
pub use policy::Policy;

/// A matchup in a grid spec: a preset's name, or one spelled out.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
#[allow(
    clippy::large_enum_variant,
    reason = "a spec holds a handful, built once"
)]
pub enum MatchupRef {
    /// A preset, by name (`balance list`).
    Preset(String),
    /// A matchup of the spec's own.
    Full(Matchup),
}

impl MatchupRef {
    /// The matchup itself.
    ///
    /// # Errors
    /// A preset name that is not one.
    pub fn resolve(&self) -> anyhow::Result<Matchup> {
        match self {
            Self::Preset(name) => preset(name),
            Self::Full(matchup) => Ok(matchup.clone()),
        }
    }
}

/// A whole run: what to vary, what to fight, how many times.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Grid {
    /// The varied paths and their values; the product is the cells.
    #[serde(default)]
    pub vary: Vec<(String, Vec<f64>)>,
    /// The matchups every cell runs.
    pub matchups: Vec<MatchupRef>,
    /// Bouts per matchup per cell, seeds `seed..seed + bouts`.
    pub bouts: u32,
    /// The first seed.
    #[serde(default)]
    pub seed: u64,
    /// Seconds a bout may last before it is a draw.
    #[serde(default = "default_cap")]
    pub cap: f32,
}

fn default_cap() -> f32 {
    60.
}

/// Where a player with a blade begins cutting, in metres from the
/// target: the blade curve's farthest sweep point (`melee.ron`, 1.65 m)
/// and a hand's slack. A policy's number, not a table's: the rusher's
/// `reach` is the rusher's, read by [`Policy::Rusher`] from its row.
pub const BLADE_RANGE: f32 = 1.7;

/// The reflex sight the sight's pair fits.
const REFLEX_SIGHT: &str = "veyra:module.vigil_reflex";
/// The hooked edge of the service blade, the melee part the harness fits.
const HOOKED_EDGE: &str = "veyra:module.blade_hooked_edge";

/// Metres of the sight's trio, whose presets are named by it. Near, both
/// land every bolt and the sight shows in nothing; far, even the sighted
/// one rarely lands; here the unsighted bought carbine lands between half
/// and four fifths of its bolts and a tighter cone is the difference.
const SIGHT_RANGE: u32 = 27;

/// Metres of the range presets: where each gun is read.
const RANGES: [f32; 3] = [10., 30., 60.];

/// Metres of the rifle's presets: close, mid, long and past the others' reach.
const RIFLE_RANGES: [f32; 4] = [10., 30., 60., 120.];

/// Metres the rifle is fired from the hip at, moving and unaimed: where an
/// aimed rifle lands every bolt, so the cost of not aiming is the whole
/// difference.
const RIFLE_HIP_RANGE: f32 = 30.;

/// Metres the held trigger is read at: arm's length and the carbine's mid.
const HOLD_RANGES: [f32; 2] = [8., 30.];

/// Seconds a guarding hand rests between cut openings with the guard up: a
/// figure of the policy and not of the game.
const GUARD_PAUSE: f32 = 0.8;

/// Seconds before a blow lands at which a parrying hand raises the guard,
/// the centre of the spread each blow draws from.
const PARRY_REACTION: f32 = 0.15;

/// Seconds a guard that only stands waits between openings: longer than any
/// bout, so it never opens.
const GUARD_STAND: f32 = 1000.;

/// Metres of the guard against a gun: far enough that the closer must cross.
const GUARD_RANGE: f32 = 25.;

/// Metres of the closing moves' own fight: inside what a thrust and a vault
/// carry plus the weapon's reach, so the ability is pressed, which at
/// `GUARD_RANGE` it never is (the carbine kills the walker first).
const CLOSING_RANGE: f32 = 6.;

/// Metres a marksman is met at: past the sentry's, inside its band.
const MARKSMAN_RANGE: f32 = 60.;

/// Metres a closer sprints to against a marksman before it fights.
const CLOSE_WITHIN: f32 = 20.;

/// Metres a cutting player begins from against a duelist: outside its wary
/// distance, so the duelist is met walking in.
const DUELIST_RANGE: f32 = 8.;

/// Metres a gun begins from against a duelist.
const DUELIST_GUN_RANGE: f32 = 15.;

/// Metres every preset against the bulwark begins from.
const BULWARK_RANGE: f32 = 15.;

/// Metres a punishing gun tries to keep from a bulwark: far enough that a
/// step back is a step, near enough that a bolt is not lost to its cone.
const PUNISH_RANGE: f32 = 8.;

/// The weapons of the worth page, in its order.
pub const WORTH_WEAPONS: [Weapon; 6] = [
    Weapon::Pistol,
    Weapon::Carbine,
    Weapon::Rifle,
    Weapon::Blade,
    Weapon::Greatblade,
    Weapon::Glaive,
];

/// Seconds a rifle hand walks across the line after a shot, and seconds it
/// holds the aim closed before the next: policy figures of the hand, not of
/// the rifle (the aim closes in the row's `aim_time`).
const RIFLE_WALK: f32 = 0.9;
const RIFLE_STAND: f32 = 0.9;

/// Metres of the opponent presets of the worth page: where the game puts
/// each opponent, or, for the marksman and the duelist, where the first half
/// of step 8 met them.
const SENTRY_RANGE: f32 = 27.;
const RUSHER_RANGE: f32 = 18.;

/// Metres a gun meets a duelist from inside its wary distance, so the
/// guard is the thing it breaks.
const DUELIST_CLOSE_RANGE: f32 = 6.;

/// Metres a strafing gun holds against the marksman, in the middle of its
/// band, where a slide has a line to leave.
const SLIDE_RANGE: f32 = 50.;

/// Metres the pistol holds against the rifle: the policy's `range`, so it
/// closes to a third of the way and strafes at it.
const PISTOL_DUEL_RANGE: f32 = 12.;

/// Metres of the closers' second pair: near enough that a plain walk
/// sometimes arrives, far enough that a thrust or a vault is the difference.
const STEP_RANGE: f32 = 4.;

/// Metres of the two guns' duels: the middle, and past the pistol.
const MIDDLE_RANGE: f32 = 30.;
const LONG_DUEL_RANGE: f32 = 80.;

/// The distance of the in-reach presets: a step outside the blade's cut,
/// the gun already drawn.
const REACH_RANGE: f32 = 2.;

/// The distance the trained kits are also measured from, a pressed move
/// and a sprint short of the gun.
const KIT_MID_RANGE: f32 = 12.;

fn stand_aim(cadence: f32) -> Policy {
    Policy::Aim { cadence }
}

/// Seconds between the trigger pulls that fire `weapon`'s row at its own
/// pace: its interval, and for a burst the burst itself.
pub(crate) fn tap_of(weapon: Weapon) -> f32 {
    use veyra_rules::combat::FireMode;
    let row = weapon.spec();
    match row.mode {
        FireMode::Burst { count, gap } => row.interval + gap * f32::from(count.saturating_sub(1)),
        FireMode::Single | FireMode::Auto => row.interval,
    }
}

/// How far a camp's people stand from the member when the fight opens,
/// metres: the brief's, a walk from the pad's edge where a camp's first
/// person sees the member.
const CAMP_RANGE: f32 = 30.;

/// The named matchups: the game's encounter opponents, the pairs the feel
/// contracts in `assets/combat/contracts.ron` are read from, and a
/// target dummy for the rates that no opponent should colour.
#[allow(clippy::too_many_lines, reason = "one list of every preset")]
#[must_use]
pub fn presets() -> Vec<Matchup> {
    let player = |policy: Policy, weapon: Weapon| Side {
        policy,
        weapon,
        profile: Profile::Player,
        count: 1,
        boxes: Vec::new(),
        fitted: Vec::new(),
        condition: None,
        lines: None,
        abilities: Vec::new(),
        with: Vec::new(),
        tier: 0,
    };
    let boxed = |policy: Policy, weapon: Weapon, boxes: Vec<String>| Side {
        boxes,
        ..player(policy, weapon)
    };
    // A pistol fires a press at a time and a carbine a burst a press, so
    // the presets that hold a trigger down tap it at the row's own pace:
    // the pistol's interval, and the carbine's burst and the pause after
    // it. Read from the tables the presets are built on.
    let pistol_tap = tap_of(Weapon::Pistol);
    let carbine_tap = tap_of(Weapon::Carbine);
    let rifle_tap = tap_of(Weapon::Rifle);
    let fire = Policy::HoldAndFire {
        range: 12.,
        cadence: carbine_tap,
        aim: true,
    };
    let fire_pistol = Policy::HoldAndFire {
        range: 12.,
        cadence: pistol_tap,
        aim: true,
    };
    // The game's opponents: the shipped AI on the installed row, read at
    // spawn and every tick, so every number of `opponents.ron` is live.
    let rusher = Side {
        policy: Policy::Rusher,
        weapon: Weapon::Blade,
        profile: Profile::Rusher,
        count: 1,
        boxes: Vec::new(),
        fitted: Vec::new(),
        condition: None,
        lines: None,
        abilities: Vec::new(),
        with: Vec::new(),
        tier: 0,
    };
    let sentry = Side {
        policy: Policy::Sentry,
        weapon: Weapon::Carbine,
        profile: Profile::Sentry,
        count: 1,
        boxes: Vec::new(),
        fitted: Vec::new(),
        condition: None,
        lines: None,
        abilities: Vec::new(),
        with: Vec::new(),
        tier: 0,
    };
    let blade = Policy::CloseAndCut {
        chain: true,
        heavy_every: 3,
        cooldown: 0.,
        reach: BLADE_RANGE,
        wait_stamina: false,
        sprint: false,
    };
    // A class cuts from the metres its own reach gives, and a dummy stands
    // there: the chain, a Heavy asked for every third opening (stamina
    // seldom covers it before the dummy falls), from the first step.
    let at_reach =
        |weapon: Weapon| veyra_rules::combat_tables::with_melee(|melee| melee.reach_m(weapon));
    let cutting = |weapon: Weapon| Policy::CloseAndCut {
        chain: true,
        heavy_every: 3,
        cooldown: 0.,
        reach: at_reach(weapon),
        wait_stamina: false,
        sprint: false,
    };
    // The same on a post that outlasts three openings, waiting at a Heavy
    // opening until stamina covers it, so the Heavy is swung and read.
    let cutting_waits = |weapon: Weapon| Policy::CloseAndCut {
        chain: true,
        heavy_every: 3,
        cooldown: 0.,
        reach: at_reach(weapon),
        wait_stamina: true,
        sprint: false,
    };
    // The guard's pairs: the same cuts behind a raised guard, and the
    // guard held down until a blow is a reaction from landing.
    let guarded_for = |weapon: Weapon, parry: bool, pause: f32| Policy::GuardAndCut {
        chain: true,
        heavy_every: 3,
        cooldown: pause,
        reach: at_reach(weapon),
        parry,
        reaction: PARRY_REACTION,
        sprint: false,
    };
    let guarded = |weapon: Weapon, parry: bool| guarded_for(weapon, parry, GUARD_PAUSE);
    // The guard's pair against the sentry, like the dodge's: the blade
    // closes and stands behind the guard without cutting, so the sentry
    // lives and both sides of the ratio are the same stretch of bolts.
    let standing = |parry: bool| guarded_for(Weapon::Blade, parry, GUARD_STAND);
    let kite = Policy::Kite {
        dodge_on_telegraph: true,
        range: 10.,
    };
    // The dodge's pair: the same stand, with and without the dodge, and
    // no gun in the number. A player who fires kills the game's sentry
    // before its first charge ends, and a strafing one is never hit by a
    // bolt aimed where the charge began; either way a ratio of nothing to
    // nothing says nothing.
    let dodge = Policy::Brace {
        dodge_on_telegraph: true,
    };
    let brace = Policy::Brace {
        dodge_on_telegraph: false,
    };
    // A chain that is exactly Light, its return cut, then a Heavy.
    let chain = Policy::CloseAndCut {
        chain: true,
        heavy_every: 2,
        cooldown: 0.,
        reach: BLADE_RANGE,
        wait_stamina: false,
        sprint: false,
    };
    let dummy = player(Policy::Stand, Weapon::Carbine);
    let post = Side {
        profile: Profile::Post,
        ..dummy.clone()
    };
    #[allow(clippy::cast_precision_loss, reason = "metres, a few tens")]
    let sight_range = SIGHT_RANGE as f32;
    let far = Policy::HoldAndFire {
        range: sight_range,
        cadence: carbine_tap,
        aim: true,
    };
    // A bought carbine: a record of its own with every line at store grade,
    // so it reads its own action line, 1.10 of the table's interval.
    let bought = |side: Side| Side {
        lines: Some(BTreeMap::new()),
        ..side
    };
    let edged = |edge: u16, fitted: Vec<SideMod>| Side {
        lines: Some(BTreeMap::from([("edge".to_owned(), edge)])),
        fitted,
        ..player(cutting_waits(Weapon::Blade), Weapon::Blade)
    };
    let hooked_edge = || loadout::parse(HOOKED_EDGE).expect("the shipped hooked edge");
    let sight = |lines: &str| {
        let text = format!("{REFLEX_SIGHT}{lines}");
        loadout::parse(&text).expect("the shipped sight")
    };
    // The boxed pairs: a Cuts 3 sheet (the novice and the Cuts branch to
    // the Heavy, the rusher's sheet) swinging only Heavies at the dummy,
    // and the whole Blade tree against that same sheet.
    let graph = boxes::graph();
    let cuts_3: Vec<String> = [
        "blade.novice",
        "blade.cuts.1",
        "blade.cuts.2",
        "blade.cuts.3",
    ]
    .map(str::to_owned)
    .to_vec();
    let heavy_only = Policy::CloseAndCut {
        chain: false,
        heavy_every: 1,
        cooldown: 0.,
        reach: BLADE_RANGE,
        wait_stamina: false,
        sprint: false,
    };
    let master = boxes::whole_tree(&graph, "blade");
    // A side that knows abilities presses them by the rule, whatever its
    // policy: the harness's wrapper, not a policy of its own.
    let knowing = |side: Side, ids: &[&str]| Side {
        abilities: ids.iter().map(|id| (*id).to_owned()).collect(),
        ..side
    };
    // A closer that sprints across open ground, as a player with the key
    // down does, and the trained kits that go with it.
    let sprinting = |weapon: Weapon| Policy::CloseAndCut {
        chain: true,
        heavy_every: 3,
        cooldown: 0.,
        reach: at_reach(weapon),
        wait_stamina: false,
        sprint: true,
    };
    let kit = |weapon: Weapon, ids: &[&str]| knowing(player(sprinting(weapon), weapon), ids);
    let kit_matchups = |metres: f32, suffix: &str| {
        let name = |kit: &str| format!("kit-{kit}-vs-carbine-{suffix}");
        [
            (
                name("blade"),
                Weapon::Blade,
                &["thrust", "reflect_shot", "heavy_cut"][..],
            ),
            (
                name("glaive"),
                Weapon::Glaive,
                &["vault", "thrust", "sweeping_cut"][..],
            ),
            (
                name("greatblade"),
                Weapon::Greatblade,
                &["kinetic_absorb", "armour_break", "sweeping_cut"][..],
            ),
        ]
        .into_iter()
        .map(|(name, weapon, ids)| Matchup {
            name,
            a: kit(weapon, ids),
            b: player(fire.clone(), Weapon::Carbine),
            distance: metres,
        })
        .collect::<Vec<_>>()
    };
    let matchup = |name: &str, a: Side, b: Side, distance: f32| Matchup {
        name: name.to_owned(),
        a,
        b,
        distance,
    };
    // The step 8 opponents, each fitted from its sheet as the game fits it.
    let marksman = Side {
        policy: Policy::Marksman,
        weapon: Weapon::Rifle,
        profile: Profile::Marksman,
        ..sentry.clone()
    };
    let duelist = Side {
        policy: Policy::Duelist,
        weapon: Weapon::Blade,
        profile: Profile::Duelist,
        ..sentry.clone()
    };
    let bulwark = Side {
        policy: Policy::Bulwark,
        weapon: Weapon::Blade,
        profile: Profile::Bulwark,
        ..sentry.clone()
    };
    // A gun that keeps its distance from a walker and fires at its own pace.
    let shoot = |weapon: Weapon| Policy::HoldAndFire {
        range: 12.,
        cadence: tap_of(weapon),
        aim: true,
    };
    let mut against_new = vec![
        // Standing in the open at 60 m, which the marksman punishes.
        matchup(
            "pistol-vs-marksman-60m",
            player(stand_aim(pistol_tap), Weapon::Pistol),
            marksman.clone(),
            MARKSMAN_RANGE,
        ),
        matchup(
            "carbine-vs-marksman-60m",
            player(stand_aim(carbine_tap), Weapon::Carbine),
            marksman.clone(),
            MARKSMAN_RANGE,
        ),
        matchup(
            "rifle-vs-marksman-60m",
            player(stand_aim(rifle_tap), Weapon::Rifle),
            marksman.clone(),
            MARKSMAN_RANGE,
        ),
        // Closing across the open: a carbine sprints to 20 m and then
        // fights, a blade with the Blade tree's abilities sprints to its
        // reach.
        matchup(
            "carbine-close-vs-marksman-60m",
            player(
                Policy::Close {
                    within: CLOSE_WITHIN,
                    then: Box::new(Policy::HoldAndFire {
                        range: CLOSE_WITHIN,
                        cadence: carbine_tap,
                        aim: true,
                    }),
                },
                Weapon::Carbine,
            ),
            marksman.clone(),
            MARKSMAN_RANGE,
        ),
        matchup(
            "blade-close-vs-marksman-60m",
            kit(Weapon::Blade, &["thrust", "reflect_shot", "heavy_cut"]),
            marksman.clone(),
            MARKSMAN_RANGE,
        ),
    ];
    for weapon in [Weapon::Blade, Weapon::Greatblade, Weapon::Glaive] {
        let class = weapon.name().to_lowercase();
        let lights = Policy::CloseAndCut {
            chain: false,
            heavy_every: 0,
            cooldown: 0.,
            reach: at_reach(weapon),
            wait_stamina: false,
            sprint: false,
        };
        let heavies = Policy::CloseAndCut {
            chain: false,
            heavy_every: 1,
            cooldown: 0.,
            reach: at_reach(weapon),
            wait_stamina: true,
            sprint: false,
        };
        for (name, policy) in [
            ("light", lights),
            ("heavy", heavies),
            ("guard", guarded(weapon, false)),
        ] {
            if weapon == Weapon::Greatblade && name == "light" {
                // Armour break against what holds a guard, where it is the
                // thing that opens it.
                against_new.push(matchup(
                    "break-greatblade-light-vs-duelist",
                    knowing(player(policy.clone(), weapon), &["armour_break"]),
                    duelist.clone(),
                    DUELIST_RANGE,
                ));
            }
            against_new.push(matchup(
                &format!("{class}-{name}-vs-bulwark"),
                player(policy.clone(), weapon),
                bulwark.clone(),
                BULWARK_RANGE,
            ));
            against_new.push(matchup(
                &format!("{class}-{name}-vs-duelist"),
                player(policy, weapon),
                duelist.clone(),
                DUELIST_RANGE,
            ));
        }
    }
    for weapon in [Weapon::Pistol, Weapon::Carbine, Weapon::Rifle] {
        against_new.push(matchup(
            &format!("{}-vs-duelist-15m", weapon.name().to_lowercase()),
            player(shoot(weapon), weapon),
            duelist.clone(),
            DUELIST_GUN_RANGE,
        ));
    }
    // Step 9: the shield bearer. A gun that fires at everything it sees, and
    // the hand that waits for the shield to come down.
    let punish = Policy::Punish {
        range: PUNISH_RANGE,
    };
    against_new.push(matchup(
        "standing-carbine-vs-bulwark",
        player(fire.clone(), Weapon::Carbine),
        bulwark.clone(),
        BULWARK_RANGE,
    ));
    for weapon in [Weapon::Pistol, Weapon::Carbine, Weapon::Rifle] {
        against_new.push(matchup(
            &format!("punish-{}-vs-bulwark", weapon.name().to_lowercase()),
            player(punish.clone(), weapon),
            bulwark.clone(),
            BULWARK_RANGE,
        ));
    }
    against_new.push(matchup(
        "blast-carbine-vs-bulwark",
        knowing(player(punish.clone(), Weapon::Carbine), &["explosive_shot"]),
        bulwark.clone(),
        BULWARK_RANGE,
    ));
    // Step 8, second half. The worth page's opponents against every weapon
    // with its plain policy: a gun fires as it does in the older presets (the
    // rifle stops to aim between steps), a class sprints in with its kit.
    // One scheme: `worth-<weapon>-vs-<opponent>`.
    let stop_and_shoot = Policy::StopAndShoot {
        walk: RIFLE_WALK,
        stand: RIFLE_STAND,
    };
    let kit_of = |weapon: Weapon| -> Side {
        match weapon {
            Weapon::Blade => kit(weapon, &["thrust", "reflect_shot", "heavy_cut"]),
            Weapon::Glaive => kit(weapon, &["vault", "thrust", "sweeping_cut"]),
            Weapon::Greatblade => kit(weapon, &["kinetic_absorb", "armour_break", "sweeping_cut"]),
            Weapon::Pistol => player(fire_pistol.clone(), weapon),
            Weapon::Rifle => player(stop_and_shoot.clone(), weapon),
            Weapon::Carbine => player(fire.clone(), weapon),
        }
    };
    for (opponent, side, metres) in [
        ("sentry", &sentry, SENTRY_RANGE),
        ("rusher", &rusher, RUSHER_RANGE),
        ("marksman", &marksman, MARKSMAN_RANGE),
        ("duelist", &duelist, DUELIST_GUN_RANGE),
        ("bulwark", &bulwark, BULWARK_RANGE),
    ] {
        for weapon in WORTH_WEAPONS {
            // A gun meets the marksman by closing to 20 m as a blade closes,
            // not by walking in at a gun's pace under its fire, so the table
            // compares two ways of closing and not a way and a stroll.
            let side_of =
                if opponent == "marksman" && matches!(weapon, Weapon::Pistol | Weapon::Carbine) {
                    let cadence = if weapon == Weapon::Pistol {
                        pistol_tap
                    } else {
                        carbine_tap
                    };
                    player(
                        Policy::Close {
                            within: CLOSE_WITHIN,
                            then: Box::new(Policy::HoldAndFire {
                                range: CLOSE_WITHIN,
                                cadence,
                                aim: true,
                            }),
                        },
                        weapon,
                    )
                } else if opponent == "bulwark" && !weapon.is_melee() {
                    // A gun plays the shield: it waits for it to come down.
                    player(punish.clone(), weapon)
                } else {
                    kit_of(weapon)
                };
            against_new.push(matchup(
                &format!("worth-{}-vs-{opponent}", weapon.name().to_lowercase()),
                side_of,
                side.clone(),
                metres,
            ));
        }
    }
    // The abilities that had no pair of their own. Each sits beside its
    // control and is read on the metric it is for (see `ability_cases`).
    let lights = Policy::CloseAndCut {
        chain: true,
        heavy_every: 0,
        cooldown: 0.,
        reach: at_reach(Weapon::Blade),
        wait_stamina: true,
        sprint: false,
    };
    let two = |side: Side| Side { count: 2, ..side };
    let rifle_walk = Policy::HoldAndFire {
        range: MIDDLE_RANGE,
        cadence: rifle_tap,
        aim: true,
    };
    // A gun that holds the middle of the marksman's band and strafes,
    // where a slide has a line to leave.
    let strafer = Policy::HoldAndFire {
        range: SLIDE_RANGE,
        cadence: carbine_tap,
        aim: true,
    };
    let dummy_two = Side {
        profile: Profile::Post,
        ..two(dummy.clone())
    };
    for (name, a, b, metres) in [
        (
            "lights-blade-vs-dummy",
            player(lights.clone(), Weapon::Blade),
            post.clone(),
            at_reach(Weapon::Blade),
        ),
        (
            "rifle-walk-vs-dummy-30m",
            player(rifle_walk.clone(), Weapon::Rifle),
            dummy.clone(),
            MIDDLE_RANGE,
        ),
        (
            "focus-rifle-walk-vs-dummy-30m",
            knowing(player(rifle_walk.clone(), Weapon::Rifle), &["focus"]),
            dummy.clone(),
            MIDDLE_RANGE,
        ),
        (
            "rapid-carbine-vs-marksman-60m",
            knowing(
                player(stand_aim(carbine_tap), Weapon::Carbine),
                &["rapid_reload"],
            ),
            marksman.clone(),
            MARKSMAN_RANGE,
        ),
        (
            "pistol-vs-dummies-30m",
            player(stand_aim(pistol_tap), Weapon::Pistol),
            dummy_two.clone(),
            MIDDLE_RANGE,
        ),
        (
            "ricochet-pistol-vs-dummies-30m",
            knowing(player(stand_aim(pistol_tap), Weapon::Pistol), &["ricochet"]),
            dummy_two.clone(),
            MIDDLE_RANGE,
        ),
        (
            "carbine-vs-dummies-30m",
            player(stand_aim(carbine_tap), Weapon::Carbine),
            dummy_two.clone(),
            MIDDLE_RANGE,
        ),
        (
            "explosive-carbine-vs-dummies-30m",
            knowing(
                player(stand_aim(carbine_tap), Weapon::Carbine),
                &["explosive_shot"],
            ),
            dummy_two.clone(),
            MIDDLE_RANGE,
        ),
        (
            "disarm-blade-vs-duelist",
            knowing(player(lights.clone(), Weapon::Blade), &["disarm"]),
            duelist.clone(),
            DUELIST_RANGE,
        ),
        (
            "suppress-carbine-vs-duelist-6m",
            knowing(
                player(shoot(Weapon::Carbine), Weapon::Carbine),
                &["suppressing_fire"],
            ),
            duelist.clone(),
            DUELIST_CLOSE_RANGE,
        ),
        (
            "carbine-vs-rusher-2m",
            player(fire.clone(), Weapon::Carbine),
            rusher.clone(),
            REACH_RANGE,
        ),
        (
            "butt-carbine-vs-rusher-2m",
            knowing(player(fire.clone(), Weapon::Carbine), &["butt_strike"]),
            rusher.clone(),
            REACH_RANGE,
        ),
        (
            "shield-pistol-vs-marksman-60m",
            knowing(
                player(stand_aim(pistol_tap), Weapon::Pistol),
                &["shield_recharge"],
            ),
            marksman.clone(),
            MARKSMAN_RANGE,
        ),
        (
            "blade-vs-carbine-4m",
            player(cutting(Weapon::Blade), Weapon::Blade),
            player(fire.clone(), Weapon::Carbine),
            STEP_RANGE,
        ),
        (
            "thrust-blade-vs-carbine-4m",
            knowing(player(cutting(Weapon::Blade), Weapon::Blade), &["thrust"]),
            player(fire.clone(), Weapon::Carbine),
            STEP_RANGE,
        ),
        (
            "glaive-vs-carbine-4m",
            player(cutting(Weapon::Glaive), Weapon::Glaive),
            player(fire.clone(), Weapon::Carbine),
            STEP_RANGE,
        ),
        (
            "vault-glaive-vs-carbine-4m",
            knowing(player(cutting(Weapon::Glaive), Weapon::Glaive), &["vault"]),
            player(fire.clone(), Weapon::Carbine),
            STEP_RANGE,
        ),
        (
            "glaive-vs-rushers",
            player(cutting(Weapon::Glaive), Weapon::Glaive),
            two(rusher.clone()),
            RUSHER_RANGE,
        ),
        (
            "sweep-glaive-vs-rushers",
            knowing(
                player(cutting(Weapon::Glaive), Weapon::Glaive),
                &["sweeping_cut"],
            ),
            two(rusher.clone()),
            RUSHER_RANGE,
        ),
        (
            "blade-guard-vs-sentry-12m",
            player(guarded(Weapon::Blade, false), Weapon::Blade),
            sentry.clone(),
            12.,
        ),
        (
            "reflect-blade-guard-vs-sentry-12m",
            knowing(
                player(guarded(Weapon::Blade, false), Weapon::Blade),
                &["reflect_shot"],
            ),
            sentry.clone(),
            12.,
        ),
        (
            "strafe-carbine-vs-marksman-60m",
            player(strafer.clone(), Weapon::Carbine),
            marksman.clone(),
            MARKSMAN_RANGE,
        ),
        (
            "slide-carbine-vs-marksman-60m",
            knowing(player(strafer.clone(), Weapon::Carbine), &["combat_slide"]),
            marksman.clone(),
            MARKSMAN_RANGE,
        ),
    ] {
        against_new.push(matchup(name, a, b, metres));
    }
    // Step 9c: each of the eleven beside its control in a fight it is for.
    // A kit is the class's three with and without the one read.
    let gb_kit = |ids: &[&str]| kit(Weapon::Greatblade, ids);
    let blade_kit = |ids: &[&str]| kit(Weapon::Blade, ids);
    let glaive_kit = |ids: &[&str]| kit(Weapon::Glaive, ids);
    let light_greatblade = Policy::CloseAndCut {
        chain: false,
        heavy_every: 0,
        cooldown: 0.,
        reach: at_reach(Weapon::Greatblade),
        wait_stamina: false,
        sprint: false,
    };
    let rifle_hand = player(stop_and_shoot.clone(), Weapon::Rifle);
    for (name, a, b, metres) in [
        (
            "absorb-kit-greatblade-vs-duelist",
            gb_kit(&["armour_break", "sweeping_cut", "kinetic_absorb"]),
            duelist.clone(),
            DUELIST_RANGE,
        ),
        (
            "kit-greatblade-vs-duelist",
            gb_kit(&["armour_break", "sweeping_cut"]),
            duelist.clone(),
            DUELIST_RANGE,
        ),
        (
            "absorb-kit-greatblade-vs-bulwark",
            gb_kit(&["armour_break", "sweeping_cut", "kinetic_absorb"]),
            bulwark.clone(),
            BULWARK_RANGE,
        ),
        (
            "kit-greatblade-vs-bulwark",
            gb_kit(&["armour_break", "sweeping_cut"]),
            bulwark.clone(),
            BULWARK_RANGE,
        ),
        (
            "break-greatblade-light-vs-bulwark",
            knowing(
                player(light_greatblade.clone(), Weapon::Greatblade),
                &["armour_break"],
            ),
            bulwark.clone(),
            BULWARK_RANGE,
        ),
        (
            "break-rifle-vs-marksman",
            knowing(rifle_hand.clone(), &["armour_break"]),
            marksman.clone(),
            MARKSMAN_RANGE,
        ),
        (
            "break-rifle-vs-sentry",
            knowing(rifle_hand.clone(), &["armour_break"]),
            sentry.clone(),
            SENTRY_RANGE,
        ),
        (
            "disarm-blade-vs-bulwark",
            knowing(
                player(
                    Policy::CloseAndCut {
                        chain: false,
                        heavy_every: 0,
                        cooldown: 0.,
                        reach: at_reach(Weapon::Blade),
                        wait_stamina: false,
                        sprint: false,
                    },
                    Weapon::Blade,
                ),
                &["disarm"],
            ),
            bulwark.clone(),
            BULWARK_RANGE,
        ),
        (
            "kit-blade-vs-marksman-60m",
            blade_kit(&["thrust", "reflect_shot", "heavy_cut"]),
            marksman.clone(),
            MARKSMAN_RANGE,
        ),
        (
            "noreflect-blade-vs-marksman-60m",
            blade_kit(&["thrust", "heavy_cut"]),
            marksman.clone(),
            MARKSMAN_RANGE,
        ),
        (
            "kit-blade-vs-sentry-27m",
            blade_kit(&["thrust", "reflect_shot", "heavy_cut"]),
            sentry.clone(),
            SENTRY_RANGE,
        ),
        (
            "noreflect-blade-vs-sentry-27m",
            blade_kit(&["thrust", "heavy_cut"]),
            sentry.clone(),
            SENTRY_RANGE,
        ),
        (
            "kit-glaive-vs-sentry-27m",
            glaive_kit(&["vault", "thrust", "sweeping_cut"]),
            sentry.clone(),
            SENTRY_RANGE,
        ),
        (
            "novault-glaive-vs-sentry-27m",
            glaive_kit(&["thrust", "sweeping_cut"]),
            sentry.clone(),
            SENTRY_RANGE,
        ),
        (
            "kit-glaive-vs-marksman-60m",
            glaive_kit(&["vault", "thrust", "sweeping_cut"]),
            marksman.clone(),
            MARKSMAN_RANGE,
        ),
        (
            "novault-glaive-vs-marksman-60m",
            glaive_kit(&["thrust", "sweeping_cut"]),
            marksman.clone(),
            MARKSMAN_RANGE,
        ),
        (
            "butt-punish-carbine-vs-bulwark",
            knowing(player(punish.clone(), Weapon::Carbine), &["butt_strike"]),
            bulwark.clone(),
            BULWARK_RANGE,
        ),
        (
            "butt-standing-carbine-vs-bulwark",
            knowing(player(fire.clone(), Weapon::Carbine), &["butt_strike"]),
            bulwark.clone(),
            BULWARK_RANGE,
        ),
        // Focus in a fight the game fields: the worth page's moving rifle
        // against the marksman, with the status on its bar.
        (
            "focus-stopshoot-rifle-vs-marksman",
            knowing(player(stop_and_shoot.clone(), Weapon::Rifle), &["focus"]),
            marksman.clone(),
            MARKSMAN_RANGE,
        ),
        (
            "rapid-carbine-vs-dummies-30m",
            knowing(
                player(stand_aim(carbine_tap), Weapon::Carbine),
                &["rapid_reload"],
            ),
            dummy_two.clone(),
            MIDDLE_RANGE,
        ),
    ] {
        against_new.push(matchup(name, a, b, metres));
    }
    // The guns inside the duelist's wary distance, where its guard is up.
    for weapon in [Weapon::Pistol, Weapon::Carbine, Weapon::Rifle] {
        against_new.push(matchup(
            &format!(
                "{}-vs-duelist-{DUELIST_CLOSE_RANGE}m",
                weapon.name().to_lowercase()
            ),
            player(shoot(weapon), weapon),
            duelist.clone(),
            DUELIST_CLOSE_RANGE,
        ));
    }
    // The two guns against each other, each with its best plain policy: the
    // pistol strafes and fires aimed, the rifle stops to aim between steps.
    against_new.push(matchup(
        &format!("pistol-vs-rifle-{MIDDLE_RANGE}m"),
        player(
            Policy::HoldAndFire {
                range: PISTOL_DUEL_RANGE,
                cadence: pistol_tap,
                aim: true,
            },
            Weapon::Pistol,
        ),
        player(stop_and_shoot.clone(), Weapon::Rifle),
        MIDDLE_RANGE,
    ));
    against_new.push(matchup(
        &format!("rifle-vs-pistol-{LONG_DUEL_RANGE}m"),
        player(stop_and_shoot.clone(), Weapon::Rifle),
        player(fire_pistol.clone(), Weapon::Pistol),
        LONG_DUEL_RANGE,
    ));
    // The Guard boxes, trained and not, behind the same parrying hand:
    // everything but the four boxes is the same sheet.
    let sheet_of = |extra: &[&str]| -> Vec<String> {
        ["blade.novice", "blade.cuts.1"]
            .iter()
            .chain(extra)
            .map(|id| (*id).to_owned())
            .collect()
    };
    let guard_boxes = [
        "blade.guard.1",
        "blade.guard.2",
        "blade.guard.3",
        "blade.guard.4",
    ];
    for (weapon, extra, opponent, side, metres) in [
        (Weapon::Blade, &[][..], "rusher", &rusher, RUSHER_RANGE),
        (Weapon::Blade, &[][..], "duelist", &duelist, DUELIST_RANGE),
        (
            Weapon::Greatblade,
            &["blade.breaking.1"][..],
            "rusher",
            &rusher,
            RUSHER_RANGE,
        ),
        (
            Weapon::Greatblade,
            &["blade.breaking.1"][..],
            "duelist",
            &duelist,
            DUELIST_RANGE,
        ),
    ] {
        let class = weapon.name().to_lowercase();
        for (kind, trained) in [("trained", true), ("untrained", false)] {
            let mut extra: Vec<&str> = extra.to_vec();
            if trained {
                extra.extend(guard_boxes);
            }
            against_new.push(matchup(
                &format!("guard-boxes-{kind}-{class}-vs-{opponent}"),
                boxed(guarded(weapon, true), weapon, sheet_of(&extra)),
                side.clone(),
                metres,
            ));
        }
    }
    // A camp's people at a tier, read from the shipped encounter file so the
    // harness fights the roster the game stands: one side of mixed members,
    // each its own profile, scaled by the tier's row.
    let camp_file: strata_registry::EncounterDef = ron::from_str(include_str!(
        "../../../assets/encounters/outlaw_camp.encounter.ron"
    ))
    .expect("the shipped outlaw camp parses");
    for tier in 1..=3_u8 {
        let mut members = camp_file.roster(tier).iter().map(|person| {
            let own = match person.kind {
                strata_registry::FoeKind::Sentry => &sentry,
                strata_registry::FoeKind::Rusher => &rusher,
                strata_registry::FoeKind::Marksman => &marksman,
                strata_registry::FoeKind::Duelist => &duelist,
                strata_registry::FoeKind::Bulwark => &bulwark,
            };
            Side {
                count: u32::from(person.count),
                tier,
                ..own.clone()
            }
        });
        let first = members.next().expect("a camp has people");
        // The player's usual policy: the bought carbine held at the
        // aimed range, as `bought-carbine-vs-dummy` plays it.
        against_new.push(matchup(
            &format!("camp-t{tier}-vs-carbine"),
            bought(player(fire.clone(), Weapon::Carbine)),
            Side {
                with: members.collect(),
                ..first
            },
            CAMP_RANGE,
        ));
    }
    vec![
        matchup(
            "carbine-vs-rusher",
            player(fire.clone(), Weapon::Carbine),
            rusher.clone(),
            18.,
        ),
        // The pressure abilities of ticket 0010 step 5b: suppressing fire
        // opens the rusher's fight with a slowing volley.
        matchup(
            "suppress-carbine-vs-rusher",
            knowing(player(fire.clone(), Weapon::Carbine), &["suppressing_fire"]),
            rusher.clone(),
            18.,
        ),
        // The butt strike, measured: the same carbine fight with the strike
        // pressed whenever the rusher is in reach.
        matchup(
            "butt-carbine-vs-rusher",
            knowing(player(fire.clone(), Weapon::Carbine), &["butt_strike"]),
            rusher.clone(),
            18.,
        ),
        matchup(
            "carbine-vs-sentry",
            player(fire.clone(), Weapon::Carbine),
            sentry.clone(),
            27.,
        ),
        matchup(
            "pistol-vs-rusher",
            player(fire_pistol.clone(), Weapon::Pistol),
            rusher.clone(),
            18.,
        ),
        matchup(
            "disarm-pistol-vs-rusher",
            knowing(player(fire_pistol.clone(), Weapon::Pistol), &["disarm"]),
            rusher.clone(),
            18.,
        ),
        matchup(
            "kite-vs-rusher",
            player(kite.clone(), Weapon::Carbine),
            rusher.clone(),
            18.,
        ),
        matchup(
            "kite-vs-sentry",
            player(kite.clone(), Weapon::Carbine),
            sentry.clone(),
            27.,
        ),
        matchup(
            "blade-vs-carbine",
            player(blade.clone(), Weapon::Blade),
            player(fire.clone(), Weapon::Carbine),
            15.,
        ),
        matchup(
            "blade-vs-blade",
            player(blade.clone(), Weapon::Blade),
            player(blade.clone(), Weapon::Blade),
            8.,
        ),
        // The contracts' pairs: a fair duel, the blade's closing problem
        // from the design's 12 m, the dodge against its control, and the
        // dummy for the two weapons' rates and the chain.
        matchup(
            "carbine-vs-carbine",
            player(fire.clone(), Weapon::Carbine),
            player(fire.clone(), Weapon::Carbine),
            12.,
        ),
        matchup(
            "blade-vs-carbine-12m",
            player(blade.clone(), Weapon::Blade),
            player(fire.clone(), Weapon::Carbine),
            12.,
        ),
        // The sentry's charge is aimed where the player stands when it
        // begins; a player who stands there takes the bolt, one who dodges
        // on the charge does not, and that is the dodge's worth.
        matchup(
            "dodge-vs-sentry-12m",
            player(dodge, Weapon::Carbine),
            sentry.clone(),
            12.,
        ),
        matchup(
            "brace-vs-sentry-12m",
            player(brace, Weapon::Carbine),
            sentry.clone(),
            12.,
        ),
        matchup(
            "carbine-vs-dummy",
            player(fire.clone(), Weapon::Carbine),
            dummy.clone(),
            12.,
        ),
        // The carbine's own action line: a bought one (every line 500)
        // against the table's row above, and one made to 900 against the
        // bought one, at the same ground.
        matchup(
            "bought-carbine-vs-dummy",
            bought(player(fire.clone(), Weapon::Carbine)),
            dummy.clone(),
            12.,
        ),
        matchup(
            "made-carbine-vs-dummy",
            Side {
                lines: Some(BTreeMap::from([("interval".to_owned(), 800)])),
                ..player(fire.clone(), Weapon::Carbine)
            },
            dummy.clone(),
            12.,
        ),
        // The sight's trio: the same aimed, held trigger on a bought
        // carbine at a distance where the cone is wider than the dummy,
        // without a sight, with the reflex sight at store grade, and with
        // one whose lines are 900.
        matchup(
            &format!("aimed-carbine-vs-dummy-{SIGHT_RANGE}m"),
            bought(player(far.clone(), Weapon::Carbine)),
            dummy.clone(),
            sight_range,
        ),
        matchup(
            &format!("sighted-carbine-vs-dummy-{SIGHT_RANGE}m"),
            Side {
                fitted: vec![sight("")],
                ..bought(player(far.clone(), Weapon::Carbine))
            },
            dummy.clone(),
            sight_range,
        ),
        matchup(
            &format!("fine-sighted-carbine-vs-dummy-{SIGHT_RANGE}m"),
            Side {
                fitted: vec![sight("@aim_spread=900,equip_time=900")],
                ..bought(player(far, Weapon::Carbine))
            },
            dummy.clone(),
            sight_range,
        ),
        matchup(
            "pistol-vs-dummy",
            player(fire_pistol.clone(), Weapon::Pistol),
            dummy.clone(),
            12.,
        ),
        // The ranges: a shooter standing and aiming at the passive dummy,
        // the pistol tapping and the carbine in bursts, at 10, 30 and 60 m,
        // and the carbine with the trigger held against its bursts.
        matchup(
            &format!("pistol-vs-dummy-{}m", RANGES[0]),
            player(stand_aim(pistol_tap), Weapon::Pistol),
            dummy.clone(),
            RANGES[0],
        ),
        matchup(
            &format!("pistol-vs-dummy-{}m", RANGES[1]),
            player(stand_aim(pistol_tap), Weapon::Pistol),
            dummy.clone(),
            RANGES[1],
        ),
        matchup(
            &format!("precise-pistol-vs-dummy-{}m", RANGES[1]),
            knowing(
                player(stand_aim(pistol_tap), Weapon::Pistol),
                &["precision_shot"],
            ),
            dummy.clone(),
            RANGES[1],
        ),
        matchup(
            &format!("pistol-vs-dummy-{}m", RANGES[2]),
            player(stand_aim(pistol_tap), Weapon::Pistol),
            dummy.clone(),
            RANGES[2],
        ),
        matchup(
            &format!("carbine-vs-dummy-{}m", RANGES[0]),
            player(stand_aim(carbine_tap), Weapon::Carbine),
            dummy.clone(),
            RANGES[0],
        ),
        matchup(
            &format!("carbine-vs-dummy-{}m", RANGES[1]),
            player(stand_aim(carbine_tap), Weapon::Carbine),
            dummy.clone(),
            RANGES[1],
        ),
        matchup(
            &format!("burst-carbine-vs-dummy-{}m", RANGES[1]),
            knowing(
                player(stand_aim(carbine_tap), Weapon::Carbine),
                &["aimed_burst"],
            ),
            dummy.clone(),
            RANGES[1],
        ),
        matchup(
            &format!("carbine-vs-dummy-{}m", RANGES[2]),
            player(stand_aim(carbine_tap), Weapon::Carbine),
            dummy.clone(),
            RANGES[2],
        ),
        matchup(
            &format!("carbine-hold-vs-dummy-{}m", HOLD_RANGES[0]),
            player(stand_aim(0.), Weapon::Carbine),
            dummy.clone(),
            HOLD_RANGES[0],
        ),
        matchup(
            &format!("carbine-hold-vs-dummy-{}m", HOLD_RANGES[1]),
            player(stand_aim(0.), Weapon::Carbine),
            dummy.clone(),
            HOLD_RANGES[1],
        ),
        matchup(
            &format!("carbine-burst-vs-dummy-{}m", HOLD_RANGES[0]),
            player(stand_aim(carbine_tap), Weapon::Carbine),
            dummy.clone(),
            HOLD_RANGES[0],
        ),
        // The rifle: standing and aiming at 10, 30, 60 and 120 m, one shot
        // at the row's own interval, and the same shooter moving about at
        // 30 m with no time to settle the aim.
        matchup(
            &format!("rifle-vs-dummy-{}m", RIFLE_RANGES[0]),
            player(stand_aim(rifle_tap), Weapon::Rifle),
            dummy.clone(),
            RIFLE_RANGES[0],
        ),
        matchup(
            &format!("rifle-vs-dummy-{}m", RIFLE_RANGES[1]),
            player(stand_aim(rifle_tap), Weapon::Rifle),
            dummy.clone(),
            RIFLE_RANGES[1],
        ),
        matchup(
            &format!("rifle-vs-dummy-{}m", RIFLE_RANGES[2]),
            player(stand_aim(rifle_tap), Weapon::Rifle),
            dummy.clone(),
            RIFLE_RANGES[2],
        ),
        // Focus on a rifle that stands to aim: the control a player would
        // actually play, where the aim is already closed when it fires.
        // Step 9c: the rifle that crosses the line and stops to aim, with
        // and without a focus that spares it the stop.
        matchup(
            &format!("stopshoot-rifle-vs-dummies-{}m", RIFLE_RANGES[2]),
            player(stop_and_shoot.clone(), Weapon::Rifle),
            dummy_two.clone(),
            RIFLE_RANGES[2],
        ),
        matchup(
            &format!("focus-stopshoot-rifle-vs-dummies-{}m", RIFLE_RANGES[2]),
            knowing(player(stop_and_shoot.clone(), Weapon::Rifle), &["focus"]),
            dummy_two.clone(),
            RIFLE_RANGES[2],
        ),
        matchup(
            &format!("focus-rifle-vs-dummy-{}m", RIFLE_RANGES[2]),
            knowing(player(stand_aim(rifle_tap), Weapon::Rifle), &["focus"]),
            dummy.clone(),
            RIFLE_RANGES[2],
        ),
        matchup(
            &format!("charged-rifle-vs-dummy-{}m", RIFLE_RANGES[2]),
            knowing(
                player(stand_aim(rifle_tap), Weapon::Rifle),
                &["charged_shot"],
            ),
            dummy.clone(),
            RIFLE_RANGES[2],
        ),
        matchup(
            &format!("rifle-vs-dummy-{}m", RIFLE_RANGES[3]),
            player(stand_aim(rifle_tap), Weapon::Rifle),
            dummy.clone(),
            RIFLE_RANGES[3],
        ),
        matchup(
            &format!("rifle-hip-vs-dummy-{RIFLE_HIP_RANGE}m"),
            player(
                Policy::HoldAndFire {
                    range: RIFLE_HIP_RANGE,
                    cadence: rifle_tap,
                    aim: false,
                },
                Weapon::Rifle,
            ),
            dummy.clone(),
            RIFLE_HIP_RANGE,
        ),
        matchup(
            &format!("focus-rifle-hip-vs-dummy-{RIFLE_HIP_RANGE}m"),
            knowing(
                player(
                    Policy::HoldAndFire {
                        range: RIFLE_HIP_RANGE,
                        cadence: rifle_tap,
                        aim: false,
                    },
                    Weapon::Rifle,
                ),
                &["focus"],
            ),
            dummy.clone(),
            RIFLE_HIP_RANGE,
        ),
        // The guard against the sentry: the blade closes from 12 m behind
        // a raised guard, or with the guard down until the bolt is a
        // reaction away, against the same stand that braces.
        matchup(
            "guard-vs-sentry-12m",
            player(standing(false), Weapon::Blade),
            sentry.clone(),
            12.,
        ),
        matchup(
            "parry-vs-sentry-12m",
            player(standing(true), Weapon::Blade),
            sentry.clone(),
            12.,
        ),
        matchup(
            "chain-vs-dummy",
            player(chain, Weapon::Blade),
            dummy.clone(),
            4.,
        ),
        matchup(
            "heavy-vs-dummy",
            boxed(heavy_only, Weapon::Blade, cuts_3.clone()),
            dummy.clone(),
            4.,
        ),
        // The classes on the dummy, each standing at its own reach, and
        // against the game's rusher from the encounter's 18 m.
        matchup(
            "blade-vs-dummy",
            player(cutting_waits(Weapon::Blade), Weapon::Blade),
            post.clone(),
            at_reach(Weapon::Blade),
        ),
        matchup(
            "greatblade-vs-dummy",
            player(cutting_waits(Weapon::Greatblade), Weapon::Greatblade),
            post.clone(),
            at_reach(Weapon::Greatblade),
        ),
        matchup(
            "glaive-vs-dummy",
            player(cutting_waits(Weapon::Glaive), Weapon::Glaive),
            post.clone(),
            at_reach(Weapon::Glaive),
        ),
        // A made blade: the service blade's own edge line at the store's
        // 500, at a smith's 900, and with the hooked edge fitted to a
        // bought one, on the post a class preset cuts at its reach.
        matchup(
            "bought-blade-vs-dummy",
            edged(500, Vec::new()),
            post.clone(),
            at_reach(Weapon::Blade),
        ),
        matchup(
            "made-blade-vs-dummy",
            edged(900, Vec::new()),
            post.clone(),
            at_reach(Weapon::Blade),
        ),
        matchup(
            "hooked-blade-vs-dummy",
            edged(500, vec![hooked_edge()]),
            post,
            at_reach(Weapon::Blade),
        ),
        matchup(
            "blade-vs-rusher",
            player(cutting(Weapon::Blade), Weapon::Blade),
            rusher.clone(),
            18.,
        ),
        matchup(
            "blade-guard-vs-rusher",
            player(guarded(Weapon::Blade, false), Weapon::Blade),
            rusher.clone(),
            18.,
        ),
        matchup(
            "blade-parry-vs-rusher",
            player(guarded(Weapon::Blade, true), Weapon::Blade),
            rusher.clone(),
            18.,
        ),
        matchup(
            "greatblade-guard-vs-rusher",
            player(guarded(Weapon::Greatblade, false), Weapon::Greatblade),
            rusher.clone(),
            18.,
        ),
        matchup(
            "blade-vs-carbine-25m",
            player(cutting(Weapon::Blade), Weapon::Blade),
            player(fire.clone(), Weapon::Carbine),
            GUARD_RANGE,
        ),
        matchup(
            "blade-guard-vs-carbine-25m",
            player(guarded(Weapon::Blade, false), Weapon::Blade),
            player(fire.clone(), Weapon::Carbine),
            GUARD_RANGE,
        ),
        // The closing moves across 25 m of open ground: the plain walkers
        // are the controls, the thrust and the vault the measured.
        matchup(
            "glaive-vs-carbine-25m",
            player(cutting(Weapon::Glaive), Weapon::Glaive),
            player(fire.clone(), Weapon::Carbine),
            GUARD_RANGE,
        ),
        matchup(
            "thrust-blade-vs-carbine-25m",
            knowing(player(cutting(Weapon::Blade), Weapon::Blade), &["thrust"]),
            player(fire.clone(), Weapon::Carbine),
            GUARD_RANGE,
        ),
        matchup(
            "vault-glaive-vs-carbine-25m",
            knowing(
                player(cutting(Weapon::Glaive), Weapon::Glaive),
                &["vault", "thrust"],
            ),
            player(fire.clone(), Weapon::Carbine),
            GUARD_RANGE,
        ),
        // The same closers where they are pressed: from 6 m, inside what the
        // move carries plus the reach.
        matchup(
            "blade-vs-carbine-6m",
            player(cutting(Weapon::Blade), Weapon::Blade),
            player(fire.clone(), Weapon::Carbine),
            CLOSING_RANGE,
        ),
        matchup(
            "thrust-blade-vs-carbine-6m",
            knowing(player(cutting(Weapon::Blade), Weapon::Blade), &["thrust"]),
            player(fire.clone(), Weapon::Carbine),
            CLOSING_RANGE,
        ),
        matchup(
            "glaive-vs-carbine-6m",
            player(cutting(Weapon::Glaive), Weapon::Glaive),
            player(fire.clone(), Weapon::Carbine),
            CLOSING_RANGE,
        ),
        matchup(
            "vault-glaive-vs-carbine-6m",
            knowing(
                player(cutting(Weapon::Glaive), Weapon::Glaive),
                &["vault", "thrust"],
            ),
            player(fire.clone(), Weapon::Carbine),
            CLOSING_RANGE,
        ),
        matchup(
            "reflect-blade-vs-carbine-25m",
            knowing(
                player(guarded(Weapon::Blade, false), Weapon::Blade),
                &["reflect_shot"],
            ),
            player(fire.clone(), Weapon::Carbine),
            GUARD_RANGE,
        ),
        matchup(
            "blade-vs-carbine-2m",
            player(cutting(Weapon::Blade), Weapon::Blade),
            player(fire.clone(), Weapon::Carbine),
            REACH_RANGE,
        ),
        matchup(
            "greatblade-vs-carbine-2m",
            player(cutting(Weapon::Greatblade), Weapon::Greatblade),
            player(fire.clone(), Weapon::Carbine),
            REACH_RANGE,
        ),
        matchup(
            "glaive-vs-carbine-2m",
            player(cutting(Weapon::Glaive), Weapon::Glaive),
            player(fire.clone(), Weapon::Carbine),
            REACH_RANGE,
        ),
        matchup(
            "blade-vs-pistol-2m",
            player(cutting(Weapon::Blade), Weapon::Blade),
            player(fire_pistol.clone(), Weapon::Pistol),
            REACH_RANGE,
        ),
        matchup(
            "sprint-blade-vs-carbine-25m",
            player(sprinting(Weapon::Blade), Weapon::Blade),
            player(fire.clone(), Weapon::Carbine),
            GUARD_RANGE,
        ),
        matchup(
            "kit-blade-vs-pistol-25m",
            kit(Weapon::Blade, &["thrust", "reflect_shot", "heavy_cut"]),
            player(fire_pistol.clone(), Weapon::Pistol),
            GUARD_RANGE,
        ),
        matchup(
            "greatblade-vs-rusher",
            player(cutting(Weapon::Greatblade), Weapon::Greatblade),
            rusher.clone(),
            18.,
        ),
        matchup(
            "break-greatblade-vs-rusher",
            knowing(
                player(cutting(Weapon::Greatblade), Weapon::Greatblade),
                &["armour_break"],
            ),
            rusher.clone(),
            18.,
        ),
        matchup(
            "absorb-greatblade-vs-rusher",
            knowing(
                player(cutting(Weapon::Greatblade), Weapon::Greatblade),
                &["kinetic_absorb"],
            ),
            rusher.clone(),
            18.,
        ),
        matchup(
            "glaive-vs-rusher",
            player(cutting(Weapon::Glaive), Weapon::Glaive),
            rusher.clone(),
            18.,
        ),
        matchup(
            "sweep-glaive-vs-rusher",
            knowing(
                player(cutting(Weapon::Glaive), Weapon::Glaive),
                &["sweeping_cut"],
            ),
            rusher,
            18.,
        ),
        matchup(
            "blade-master-vs-novice",
            boxed(blade.clone(), Weapon::Blade, master.clone()),
            boxed(blade.clone(), Weapon::Blade, cuts_3.clone()),
            8.,
        ),
        // Step 9c: the master as a master would fight: the guard and parry
        // boxes read by a hand that guards, the tree's abilities on its bar.
        matchup(
            "blade-master-guard-vs-novice",
            knowing(
                boxed(guarded(Weapon::Blade, true), Weapon::Blade, master.clone()),
                &["heavy_cut", "thrust", "disarm"],
            ),
            boxed(blade.clone(), Weapon::Blade, cuts_3.clone()),
            8.,
        ),
        matchup(
            "blade-master-guard-plain-vs-novice",
            boxed(guarded(Weapon::Blade, true), Weapon::Blade, master),
            boxed(blade, Weapon::Blade, cuts_3),
            8.,
        ),
    ]
    .into_iter()
    .chain(kit_matchups(GUARD_RANGE, "25m"))
    .chain(kit_matchups(KIT_MID_RANGE, "12m"))
    .chain(against_new)
    .collect()
}

/// One ability beside its control: the preset with the ability on the bar,
/// the same preset without it, the weapon it is for and the number it is
/// for. The worth page reads its rows from here and the contracts' ability
/// bands are written from the same list.
#[derive(Debug, Clone, PartialEq)]
pub struct AbilityCase {
    /// The ability's id in `abilities.ron`.
    pub id: &'static str,
    /// The weapon it is carried on in the pair.
    pub weapon: Weapon,
    /// The preset with the ability.
    pub with: &'static str,
    /// The preset without it.
    pub without: &'static str,
    /// The number the ability is for, read on both presets.
    pub metric: Metric,
    /// A smaller figure is the better one (damage taken, the opponent's
    /// posture left), so the ratio is read inverted.
    pub lower_is_better: bool,
}

/// The nineteen abilities, each with its pair, in the order of
/// `abilities.ron`. Every pair is read on the metric the ability is for:
/// damage a second for a damage ability, damage taken (smaller is better)
/// for a defensive one, the win share for a closing one.
#[allow(clippy::too_many_lines, reason = "one list of nineteen cases")]
#[must_use]
pub fn ability_cases() -> Vec<AbilityCase> {
    use SideMetric::{DealtPerSecond, Taken};
    let case = |id, weapon, with, without, metric, lower_is_better| AbilityCase {
        id,
        weapon,
        with,
        without,
        metric,
        lower_is_better,
    };
    let (rate, taken) = (Metric::A(DealtPerSecond), Metric::A(Taken));
    vec![
        case(
            "heavy_cut",
            Weapon::Blade,
            "blade-vs-dummy",
            "lights-blade-vs-dummy",
            rate.clone(),
            false,
        ),
        case(
            "butt_strike",
            Weapon::Carbine,
            "butt-punish-carbine-vs-bulwark",
            "punish-carbine-vs-bulwark",
            taken.clone(),
            true,
        ),
        case(
            "precision_shot",
            Weapon::Pistol,
            "precise-pistol-vs-dummy-30m",
            "pistol-vs-dummy-30m",
            rate.clone(),
            false,
        ),
        case(
            "aimed_burst",
            Weapon::Carbine,
            "burst-carbine-vs-dummy-30m",
            "carbine-vs-dummy-30m",
            rate.clone(),
            false,
        ),
        case(
            "focus",
            Weapon::Rifle,
            "focus-stopshoot-rifle-vs-marksman",
            "worth-rifle-vs-marksman",
            Metric::MeanTime,
            true,
        ),
        case(
            "kinetic_absorb",
            Weapon::Greatblade,
            "absorb-kit-greatblade-vs-duelist",
            "kit-greatblade-vs-duelist",
            taken.clone(),
            true,
        ),
        case(
            "rapid_reload",
            Weapon::Carbine,
            "rapid-carbine-vs-dummies-30m",
            "carbine-vs-dummies-30m",
            rate.clone(),
            false,
        ),
        case(
            "shield_recharge",
            Weapon::Pistol,
            "shield-pistol-vs-marksman-60m",
            "pistol-vs-marksman-60m",
            Metric::WinRate,
            false,
        ),
        case(
            "ricochet",
            Weapon::Pistol,
            "ricochet-pistol-vs-dummies-30m",
            "pistol-vs-dummies-30m",
            rate.clone(),
            false,
        ),
        case(
            "disarm",
            Weapon::Blade,
            "disarm-blade-vs-duelist",
            "blade-light-vs-duelist",
            taken.clone(),
            true,
        ),
        case(
            "suppressing_fire",
            Weapon::Carbine,
            "suppress-carbine-vs-duelist-6m",
            "carbine-vs-duelist-6m",
            rate.clone(),
            false,
        ),
        case(
            "explosive_shot",
            Weapon::Carbine,
            "blast-carbine-vs-bulwark",
            "punish-carbine-vs-bulwark",
            Metric::MeanTime,
            true,
        ),
        case(
            "charged_shot",
            Weapon::Rifle,
            "charged-rifle-vs-dummy-60m",
            "rifle-vs-dummy-60m",
            rate.clone(),
            false,
        ),
        case(
            "armour_break",
            Weapon::Rifle,
            "break-rifle-vs-marksman",
            "worth-rifle-vs-marksman",
            Metric::MeanTime,
            true,
        ),
        case(
            "thrust",
            Weapon::Blade,
            "thrust-blade-vs-carbine-4m",
            "blade-vs-carbine-4m",
            Metric::WinRate,
            false,
        ),
        case(
            "reflect_shot",
            Weapon::Blade,
            "reflect-blade-guard-vs-sentry-12m",
            "blade-guard-vs-sentry-12m",
            rate.clone(),
            false,
        ),
        case(
            "sweeping_cut",
            Weapon::Glaive,
            "sweep-glaive-vs-rushers",
            "glaive-vs-rushers",
            rate.clone(),
            false,
        ),
        case(
            "vault",
            Weapon::Glaive,
            "kit-glaive-vs-sentry-27m",
            "novault-glaive-vs-sentry-27m",
            taken.clone(),
            true,
        ),
        case(
            "combat_slide",
            Weapon::Carbine,
            "slide-carbine-vs-marksman-60m",
            "strafe-carbine-vs-marksman-60m",
            taken,
            true,
        ),
    ]
}

/// The preset called `name`.
///
/// # Errors
/// An unknown name, with the known ones.
pub fn preset(name: &str) -> anyhow::Result<Matchup> {
    presets()
        .into_iter()
        .find(|m| m.name == name)
        .ok_or_else(|| {
            anyhow::anyhow!(
                "no matchup `{name}`; the presets are {}",
                presets()
                    .iter()
                    .map(|m| m.name.as_str())
                    .collect::<Vec<_>>()
                    .join(", ")
            )
        })
}

/// The bouts of one matchup in one cell, in seed order.
#[derive(Debug, Clone, PartialEq)]
pub struct CellResult {
    /// The cell.
    pub cell: Cell,
    /// The matchup's name.
    pub matchup: String,
    /// `(seed, result)` in seed order.
    pub bouts: Vec<(u64, BoutResult)>,
    /// Wall seconds the cell's bouts took.
    pub seconds: f64,
}

/// Run `bouts` bouts of `matchup` on the installed tables, `jobs` at a
/// time. Bouts are independent apps; the seed decides every choice, so
/// the thread a bout ran on does not show in its row.
#[must_use]
pub fn run_matchup(
    matchup: &Matchup,
    seed: u64,
    bouts: u32,
    cap: f32,
    jobs: usize,
) -> Vec<(u64, BoutResult)> {
    let seeds: Vec<u64> = (0..u64::from(bouts)).map(|i| seed + i).collect();
    let jobs = jobs.max(1).min(seeds.len().max(1));
    let mut results: Vec<Option<BoutResult>> = vec![None; seeds.len()];
    std::thread::scope(|scope| {
        let mut slots: Vec<&mut [Option<BoutResult>]> =
            results.chunks_mut(seeds.len().div_ceil(jobs)).collect();
        let mut chunks = seeds.chunks(seeds.len().div_ceil(jobs));
        let mut handles = Vec::new();
        for slot in slots.drain(..) {
            let Some(chunk) = chunks.next() else { break };
            handles.push(scope.spawn(move || {
                for (out, seed) in slot.iter_mut().zip(chunk) {
                    *out = Some(bout::run(matchup, *seed, cap));
                }
            }));
        }
        for handle in handles {
            handle.join().expect("a bout thread panicked");
        }
    });
    seeds
        .into_iter()
        .zip(results)
        .map(|(seed, result)| (seed, result.expect("every seed ran")))
        .collect()
}

/// Run the whole grid: every cell installed in turn, every matchup, the
/// bouts. `base` is the tables the cells are written over. The callback
/// sees each cell's results as they finish, so the CSV streams.
///
/// # Errors
/// A path in `vary` that names no field, a matchup named that is no
/// preset, a side's boxes that are no sheet, or its modules that are no
/// loadout.
pub fn run_grid(
    grid: &Grid,
    base: &CombatTables,
    jobs: usize,
    mut each: impl FnMut(&CellResult),
) -> anyhow::Result<()> {
    let variations: Vec<Variation> = grid
        .vary
        .iter()
        .map(|(path, values)| Variation {
            path: path.clone(),
            values: values.clone(),
        })
        .collect();
    let matchups = grid
        .matchups
        .iter()
        .map(MatchupRef::resolve)
        .collect::<anyhow::Result<Vec<_>>>()?;
    // A side's boxes and modules are checked before any bout runs, so a
    // wrong id is refused with its name and not a panic mid-grid.
    for matchup in &matchups {
        boxes::sheet(&format!("{} a", matchup.name), &matchup.a.boxes)?;
        boxes::sheet(&format!("{} b", matchup.name), &matchup.b.boxes)?;
        loadout::check(matchup)?;
    }
    for cell in grid::cells(&variations) {
        // One grid point at a time: the tables are process-wide.
        veyra_rules::combat_tables::install(cell.apply(base)?);
        for matchup in &matchups {
            bout::check_abilities(matchup)?;
            let started = Instant::now();
            let bouts = run_matchup(matchup, grid.seed, grid.bouts, grid.cap, jobs);
            each(&CellResult {
                cell: cell.clone(),
                matchup: matchup.name.clone(),
                bouts,
                seconds: started.elapsed().as_secs_f64(),
            });
        }
    }
    veyra_rules::combat_tables::install(base.clone());
    Ok(())
}
