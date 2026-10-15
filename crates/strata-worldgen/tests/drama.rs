//! How dramatic seed 42 is, held (ticket 0004, step 1).
//!
//! The snapshot pins the measures of [`strata_worldgen::measure`] over the
//! standard point set, rounded so that only a change to the ground shows
//! as a diff: a change that moves the world is reviewed as numbers, beside
//! the worldmap's `-shade` and `-view` layers. Bless an intended change
//! with `INSTA_UPDATE=always cargo test -p strata-worldgen --test drama`
//! and read the diff.
//!
//! The gates are the floor the ticket's plan may not fall through. Step 1
//! set them a little under the world as it first measured; step 2, which
//! brought the ranges near, raised them to its targets (ticket 0004): ten
//! degrees in view from 60 % of the land, the mountains between a tenth and
//! a fifth of it, nine tenths of it walkable, and foothills round the
//! ranges that are a tenth of the land and hills in their own right. Step
//! 2b added that ranges are long: the patches of mountain ground are
//! measured for their shape ([`measure::patches`]), and their median
//! elongation and the roundest large one are gated, over the patches the
//! 100 km disc does not cut (a cut patch's shape is only the part inside). The gates the steps did
//! not name are held a little under what the world measured when they were
//! set.
//!
//! Step 2b's other measure, the profiles across ten range flanks
//! ([`measure::flank_profiles`]), was reported and not gated until a world
//! passed it: a shelf or a step at the foot flagged every profile of the
//! world before step 2b and every one after it. Step 4 gave every range a
//! foot (the range's own height read from the distance to its crest line)
//! and gates it: at most three of the ten flagged, and the median largest
//! rise in 250 m below half-height under 250 m (ten flagged and 486 m
//! before). Its review added to the snapshot, reported and not gated, the
//! other flank of each of those crests ([`measure::opposite_profiles`]) and
//! the flank facing the sea of each large patch
//! ([`measure::seaward_profiles`]), which the gated ten never are.
//!
//! Step 4 also gave each geological province its relief shapes, and gates
//! what can be stated of them so that the world before it fails and the
//! world after passes ([`measure::ProvinceLand`], [`measure::province_tiles`]):
//! sandstone keeps flat ground and gains steep ground, karst is pocked with
//! closed hollows, the folded belt's ridges run along one grain, and the
//! badlands are a maze of steep ground.
//!
//! Step 5 gave the provinces beds, hard and soft, and gates the two whose
//! beds are strong and flat, sandstone and basalt: their steep ground is
//! cliff bands, a share of it risers ([`measure::RISER_DEG`]), and the
//! risers are a bed's height and not a range's flank.

mod common;

use std::fmt::Write as _;
use std::sync::OnceLock;

use strata_registry::GameRegistries;
use strata_worldgen::measure::{
    self, FLAT_DEG, HOLLOW_DEPTH_M, Measures, PATCH_CELL_M, PATCH_LARGE_AREA_KM2,
    PATCH_MIN_AREA_KM2, PATCH_RADIUS_M, Patch, PointSet, Profile, ProvinceTiles, RELIEF_FAR,
    RELIEF_NEAR, SLOPE_CLASSES_DEG, STEEP_DEG, VIEW_THRESHOLDS_DEG, WALKABLE_SLOPE_DEG,
};

/// The median tallest thing in view may not fall under this, in degrees.
/// 2.9 when first measured (step 1); 13.8 after step 2, 14.0 after its
/// review, 13.4 after step 2b, 15.1 after step 4, 15.0 after its review, 16.0
/// after step 5 (its crests).
const MIN_VIEW_MEDIAN_DEG: f64 = 12.0;

/// The share of land seeing ten degrees or more may not fall under this.
/// 15.2 % when first measured; step 2's target, 65.2 % after it, 65.3 %
/// after its review, 63.0 % after step 2b, 67.4 % after step 4 and 66.8 %
/// after its review, 67.7 % after step 5.
const MIN_SEEING_TEN_DEG: f64 = 0.60;

/// The median relief within a kilometre may not fall under this, in metres.
/// 73 m when first measured; 128 m after step 2, 133 m after its review,
/// 143 m after step 2b, 150 m after step 4, 149 m after its review and
/// after step 5.
const MIN_RELIEF_NEAR_M: f64 = 110.0;

/// The median relief within five kilometres may not fall under this, in
/// metres. 238 m when first measured; 972 m after step 2, of which about
/// 300 was the scarp its review took out of the foot of every massif (the
/// mountains' relief rose from a quarter of its height where they entered
/// the blend); 668 m after that review. **Lowered by step 2b**, whose
/// ranges are lines tens of kilometres apart rather than massifs, so the
/// median point has less of one within 5 km: 580 m on the standard set and
/// 551 m at the probe's size ([`seed_42_measures_at_the_probes_size`], ten
/// times the points), and 580 and 564 m after its review. The gate is 530
/// m, under the larger sample's reading with room for the spread between
/// the two, since a gate that holds only because of which 1 200 points the
/// standard set has holds nothing. It was never a target (it was set a
/// little under what the review of step 2 measured), and the alternatives
/// that held 600 m were taller foothills, which lifted the plains over
/// their 400 m bound (`tests/coverage.rs`), or a highlands window reaching
/// lower, which brought that biome into the blend over most of the lowland
/// at a negligible weight and cost 0.7 us a sample there (ticket 0004,
/// "Step 2b, as built"). Step 4, whose ranges rise over a foot of
/// kilometres, reads 728 m, and 712 m since its review (and after step
/// 5).
const MIN_RELIEF_FAR_M: f64 = 530.0;

/// The share of land the character can walk on may not fall under this.
/// 99.1 % when first measured, 95.0 % after step 2, 95.1 % after its
/// review, 95.4 % after step 2b, 98.2 % after step 4 and 98.6 % after its
/// review, 97.4 % after step 5 (its cliff bands and crests); the
/// ticket's risks name steep ground
/// nobody walks as the price of drama, and this is step 2's bound on that
/// price.
const MIN_WALKABLE: f64 = 0.90;

/// The mountains must take less of the land than this. 4.9 % when first
/// measured, 13.7 % after step 2 (16.6 % after its review, 16.8 % after
/// step 2b, 11.9 % after step 4 and after step 5), whose target this is: nearer ranges are
/// wider ranges, and past a fifth the land is mostly rock and snow.
const MAX_MOUNTAINS: f64 = 0.20;

/// The mountains may not shrink back either: they are what is in view.
/// 13.7 % after step 2, 16.6 % after its review, 16.8 % after step 2b,
/// 11.9 % after step 4 (whose ranges are as wide in metres everywhere and
/// no longer spread into plateaus where the range field is flat).
const MIN_MOUNTAINS: f64 = 0.10;

/// The highlands, the foothills a range stands in, may not take less of
/// the land than this. 3.9 % when first measured, a thin ring round each
/// massif; 12.2 % after step 2, whose target this is, 10.8 % after its
/// review, which gave the top of their window to the mountains, and 13.1 %
/// after step 2b, whose ranges stand on a long foot, and 12.8 % after step
/// 4 (and after its review, and after step 5).
const MIN_HIGHLANDS: f64 = 0.10;

/// The highlands' own relief may not be gentler than this mean slope, in
/// degrees ([`Measures::biome_relief_slope`], which leaves the range's flank
/// out, so that foothills are hills and not only a hillside). 2.8 when
/// first measured; 11.7 after step 2, whose target this is, 10.7 after its
/// review, 10.6 after step 2b, 17.3 after step 4 (the provinces' shapes,
/// and a range's own slope under them), 16.2 after its review and after
/// step 5.
const MIN_HIGHLANDS_RELIEF_SLOPE_DEG: f64 = 5.0;

/// The median elongation of the patches of mountain ground over
/// [`PATCH_MIN_AREA_KM2`] may not fall under this ([`Patch::elongation`]: a
/// disc is one). Step 2b's target: 1.94 before it, when the ranges were
/// massifs round the extremes of a ridged noise, and 2.77 after (2.78 over
/// the uncut patches since its review, which leaves the patches the disc
/// cuts out of both elongation gates); 3.78 after step 4 and its review.
const MIN_MEDIAN_ELONGATION: f64 = 2.5;

/// No patch of mountain ground over [`PATCH_LARGE_AREA_KM2`] may be rounder
/// than this. Step 2b's target: the roundest was 1.43 before it and is 2.06
/// after (2.05 since its review); 5.68 after step 4 and 5.69 after its
/// review. **Since step 4 this gate rests on one patch**: its ranges are as
/// wide in metres everywhere and no longer spread into plateaus, so one
/// patch over [`PATCH_LARGE_AREA_KM2`] is left uncut (five before), and the
/// gate says only that that one is long (the review of step 4).
const MIN_LARGE_ELONGATION: f64 = 1.5;

/// How many flank profiles the snapshot reports.
const PROFILES: usize = 10;

/// At most this many of the [`PROFILES`] may be flagged for a shelf or a
/// step at the foot ([`Profile::flagged`]): ten of ten before step 4 (nine
/// with a shelf, nine with a step), three after it, two after its review,
/// three after step 5 (three with a shelf, one with a step: the cols
/// between the crests' taller peaks read as shelves).
/// The bound is step 4's own, set at what it measured; step 2b's target, a
/// foot with no shelf and no step, is not met. It holds on the ten flanks
/// the measure chooses (the steepest land line from each large patch's
/// crest), not in general: the other flank of the same crests flags three
/// of nine, and the flanks facing the sea six of seven (reported in the
/// snapshot, not gated). Nor does it hold whatever the noise: step 4's
/// review drew the provinces' relief variants from other lattices once and
/// seven of ten were flagged, with a median wall of 225 m.
const MAX_FLAGGED_PROFILES: usize = 3;

/// The median of the profiles' largest rise in 250 m below half-height
/// ([`measure::median_wall_m`]) must be under this, in metres. Step 4's
/// target: 486 m before it, 147 m after (and after its review), 181 m after
/// step 5; 184 m over the other flanks, 380 m over those facing the sea
/// (reported), and 229 and 479 m after step 5.
const MAX_MEDIAN_WALL_M: f64 = 250.0;

/// Sandstone's own ground keeps at least this share flat ([`FLAT_DEG`])
/// and gains at least [`MIN_SANDSTONE_STEEP`] steep ([`STEEP_DEG`]): benches
/// and the bluffs between them (step 4). The world before step 4 read 25.5 %
/// and 3.3 %, and fails both; after it 29.8 % and 5.3 % (30.1 and 5.2 after
/// its review, 30.3 and 4.6 after step 5, whose beds turn part of its steep
/// slopes into benches and cliffs). An fbm damped by its own slope lays
/// benches down but cannot stand a cliff; the beds of step 5 stand them
/// ([`MIN_SANDSTONE_RISERS`]).
const MIN_SANDSTONE_FLAT: f64 = 0.26;

/// See [`MIN_SANDSTONE_FLAT`].
const MIN_SANDSTONE_STEEP: f64 = 0.04;

/// Closed hollows at least [`HOLLOW_DEPTH_M`] deep a square kilometre on
/// karst's tiles, at least ([`measure::province_tiles`]). 0.53 before step 4
/// (the lows of fbm and the erosion's pits), 1.98 after (the cellular
/// field's dolines, about 600 m apart on its plains) and after its review
/// and step 5.
const MIN_KARST_HOLLOWS_PER_KM2: f64 = 1.5;

/// The elongation of the folded belt's relief on its tiles, at least
/// ([`ProvinceTiles::elongation`]): 1.02 before step 4, 2.48 after (ridged
/// relief stretched four times along the seed's range axis), 2.35 after its
/// review, which lowered the folded hills, 2.34 after step 5.
const MIN_FOLDED_ELONGATION: f64 = 2.0;

/// Badlands' own ground steeper than [`STEEP_DEG`], at least: 13.3 % before
/// step 4, 44.0 % after (short ridged relief, cut twice as deep by the
/// gullies), 39.4 % after its review, which lowered the badlands' hills to
/// keep them under 8 m in a metre, 39.2 % after step 5.
const MIN_BADLANDS_STEEP: f64 = 0.35;

/// Of sandstone's steep ground ([`STEEP_DEG`]), at least this share is a
/// riser ([`measure::RISER_DEG`]): its beds stand as cliff bands (ticket
/// 0004, step 5). 1.7 % before step 5, whose steep sandstone was slopes;
/// 9.5 % after it.
const MIN_SANDSTONE_RISERS: f64 = 0.06;

/// Of basalt's steep ground, at least this share is a riser: a step at
/// each flow. 3.4 % before step 5, 26.9 % after it.
const MIN_BASALT_RISERS: f64 = 0.15;

/// The mean height of sandstone's and basalt's risers
/// ([`measure::riser_height_m`]) must be under this, in metres: a bed's
/// cliff, tens of metres, and not a range's flank. Before step 5 the few
/// risers on their ground were flanks, 300 and 458 m; after it 39 and 64 m
/// (the beds are 8 to 40 and 25 to 70 m thick, and a riser can run on
/// through the soft bed under it).
const MAX_BEDDED_RISER_MEAN_M: f64 = 100.0;

/// Tiles a province [`measure::province_tiles`] measures.
const TILES_PER_PROVINCE: usize = 8;

/// Threads the measures run on. Not every core: on the development machine
/// (an i9-13900KF) a debug build with all 32 threads busy in the sampler
/// dies with SIGSEGV at a program counter in the middle of an instruction,
/// every run, and passes on any 16. The flora placement test's binary
/// crashes the same way on all cores and passes on 16; the cause is not
/// known (a hardware instability of that CPU family is a guess, not a
/// finding). The result does not depend on the count.
const THREADS: usize = 16;

/// Seed 42's measures, taken once for both tests: the standard set, the
/// patches of mountain ground and the profiles across the largest ones'
/// flanks.
struct Taken {
    measures: Measures,
    patches: Vec<Patch>,
    profiles: Vec<Profile>,
    /// The other flank of each profile's range, and the flank facing the
    /// sea of each of the ten largest patches: reported, not gated.
    opposite: Vec<Option<Profile>>,
    seaward: Vec<Option<Profile>>,
    tiles: Vec<ProvinceTiles>,
    registries: GameRegistries,
}

fn taken() -> &'static Taken {
    static TAKEN: OnceLock<Taken> = OnceLock::new();
    TAKEN.get_or_init(|| {
        let registries = common::registries();
        let world = common::world(42);
        let points = PointSet::standard(&world);
        let mountains = mountains(&registries);
        let patches = measure::patches(
            &world,
            |s| f64::from(s.height) > measure::LAND_MIN_M && s.dominant_biome() == mountains,
            PATCH_RADIUS_M,
            PATCH_CELL_M,
            PATCH_MIN_AREA_KM2,
            THREADS,
        );
        let profiles = measure::flank_profiles(&world, &patches, PROFILES);
        let opposite = measure::opposite_profiles(&world, &profiles);
        let seaward = measure::seaward_profiles(&world, &patches, PROFILES);
        // Tiles away from the mountains, whose flanks would read as grain.
        let tiles = measure::province_tiles(
            &world,
            |s| f64::from(s.height) > measure::LAND_MIN_M && s.dominant_biome() != mountains,
            PATCH_RADIUS_M,
            TILES_PER_PROVINCE,
            THREADS,
        );
        Taken {
            measures: measure::measure_on(&world, &points, THREADS),
            patches,
            profiles,
            opposite,
            seaward,
            tiles,
            registries,
        }
    })
}

/// The measures as the snapshot holds them: degrees to a tenth, metres to
/// the metre, shares to a tenth of a percent, every shipped biome named.
fn report(m: &Measures, registries: &GameRegistries) -> String {
    let pct = |share: f64| format!("{:.1} %", share * 100.0);
    let mut out = String::new();
    let _ = writeln!(
        out,
        "points: {} views, {} ground",
        m.view_points, m.ground_points
    );
    let _ = writeln!(
        out,
        "tallest thing in view: median {:.1} deg",
        m.view_median_deg
    );
    for (t, share) in VIEW_THRESHOLDS_DEG.iter().zip(m.view_shares) {
        let _ = writeln!(out, "  land seeing {t:>2} deg or more: {}", pct(share));
    }
    let _ = writeln!(
        out,
        "relief: median {:.0} m within {:.0} km, {:.0} m within {:.0} km",
        m.relief_near_median_m,
        RELIEF_NEAR.0 / 1_000.0,
        m.relief_far_median_m,
        RELIEF_FAR.0 / 1_000.0
    );
    let _ = writeln!(out, "slope classes:");
    let mut lower = 0.0;
    for (k, share) in m.slope_shares.iter().enumerate() {
        let label = SLOPE_CLASSES_DEG.get(k).map_or_else(
            || format!("over {lower:.0} deg"),
            |upper| format!("{lower:.0} to {upper:.0} deg"),
        );
        let _ = writeln!(out, "  {label:<16} {}", pct(*share));
        lower = SLOPE_CLASSES_DEG.get(k).copied().unwrap_or(lower);
    }
    let _ = writeln!(
        out,
        "walkable (at most {WALKABLE_SLOPE_DEG:.0} deg): {}",
        pct(m.walkable_share)
    );
    let _ = writeln!(
        out,
        "biomes: share of land, mean slope at 1 m, mean slope of the relief"
    );
    for (id, name, _) in registries.biomes.iter() {
        let _ = writeln!(
            out,
            "  {:<20} {:>6}  {:>4.1} deg  {:>4.1} deg",
            name.to_string(),
            pct(m.biome_share(id)),
            m.biome_slope(id),
            m.biome_relief_slope(id)
        );
    }
    out
}

/// The shape of the ranges as the snapshot holds it: every patch of
/// mountain ground over [`PATCH_MIN_AREA_KM2`] with its area and elongation,
/// and each flank profile's heights every 250 m from the plain to the crest
/// with its flags, so a change to the ranges reads as a diff of numbers.
fn shape_report(t: &Taken) -> String {
    let (patches, profiles) = (&t.patches, &t.profiles);
    let mut out = String::new();
    let large: Vec<&Patch> = patches
        .iter()
        .filter(|p| p.area_km2 > PATCH_LARGE_AREA_KM2)
        .collect();
    let _ = writeln!(
        out,
        "mountain patches over {PATCH_MIN_AREA_KM2:.0} km2 within {:.0} km: {} ({} cut by the disc), median elongation of the uncut {:.2}; over {PATCH_LARGE_AREA_KM2:.0} km2: {}, least elongation of the uncut {:.2}",
        PATCH_RADIUS_M / 1_000.0,
        patches.len(),
        patches.iter().filter(|p| p.cut).count(),
        measure::median_elongation(patches),
        large.len(),
        least_large_elongation(patches)
    );
    for p in patches {
        let _ = writeln!(
            out,
            "  {:>6.1} km2 at ({:>6.0}, {:>6.0}) elongation {:>5.2}{}",
            p.area_km2,
            p.centroid[0],
            p.centroid[1],
            p.elongation,
            if p.cut { ", cut by the disc" } else { "" }
        );
    }
    let flagged = profiles.iter().filter(|p| p.flagged()).count();
    let _ = writeln!(
        out,
        "flank profiles: {} ({flagged} flagged: {} with a shelf, {} with a step at the foot), median largest rise in 250 m below half-height {:.0} m",
        profiles.len(),
        profiles.iter().filter(|p| p.shelf.is_some()).count(),
        profiles.iter().filter(|p| p.foot_step.is_some()).count(),
        measure::median_wall_m(profiles)
    );
    for p in profiles {
        profile_line(&mut out, p);
    }
    for (label, set) in [
        ("the other flank of each", &t.opposite),
        ("facing the sea, of the largest patches", &t.seaward),
    ] {
        let found: Vec<&Profile> = set.iter().flatten().collect();
        let walls: Vec<Profile> = found.iter().map(|p| (*p).clone()).collect();
        let _ = writeln!(
            out,
            "flank profiles {label}, reported and not gated: {} ({} flagged: {} with a shelf, {} with a step at the foot), median largest rise in 250 m below half-height {:.0} m",
            found.len(),
            found.iter().filter(|p| p.flagged()).count(),
            found.iter().filter(|p| p.shelf.is_some()).count(),
            found.iter().filter(|p| p.foot_step.is_some()).count(),
            measure::median_wall_m(&walls)
        );
        for p in found {
            profile_line(&mut out, p);
        }
    }
    out
}

/// One flank profile as the snapshot holds it.
fn profile_line(out: &mut String, p: &Profile) {
    let heights: Vec<String> = p.heights.iter().map(|h| format!("{h:.0}")).collect();
    let _ = writeln!(
        out,
        "  ({:>6.0}, {:>6.0}) to ({:>6.0}, {:>6.0}) shelf {:?} step {:?}: {}",
        p.plain[0],
        p.plain[1],
        p.crest[0],
        p.crest[1],
        p.shelf,
        p.foot_step,
        heights.join(" ")
    );
}

/// Each province's landform as the snapshot holds it: its own ground's
/// flat and steep shares and relief within 1 km, and on its tiles the
/// closed hollows a square kilometre and the elongation of its grain.
fn province_report(m: &Measures, tiles: &[ProvinceTiles], registries: &GameRegistries) -> String {
    let mut out = String::new();
    let _ = writeln!(
        out,
        "provinces (own ground): points, flat under {FLAT_DEG:.0} deg, steep over {STEEP_DEG:.0} deg, relief within {:.0} km, risers over {:.0} deg as a share of the steep and their mean height; on tiles: tiles, hollows over {HOLLOW_DEPTH_M:.0} m a km2, elongation",
        RELIEF_NEAR.0 / 1_000.0,
        measure::RISER_DEG
    );
    for (id, name, _) in registries.provinces.iter() {
        let land = m.province(id);
        let tile = tiles.iter().find(|t| t.province == id);
        let _ = writeln!(
            out,
            "  {:<18} {:>5} {:>5.1} % {:>5.1} % {:>4.0} m {:>5.1} % {:>4.0} m | {:>2} {:>5.2} {:>5.2}",
            name.to_string(),
            land.map_or(0, |l| l.ground_points),
            land.map_or(f64::NAN, |l| l.flat_share * 100.0),
            land.map_or(f64::NAN, |l| l.steep_share * 100.0),
            land.map_or(f64::NAN, |l| l.relief_near_median_m),
            land.map_or(f64::NAN, |l| l.riser_share * 100.0),
            land.map_or(f64::NAN, |l| l.riser_mean_m),
            tile.map_or(0, |t| t.tiles),
            tile.map_or(f64::NAN, |t| t.hollows_per_km2),
            tile.map_or(f64::NAN, |t| t.elongation),
        );
    }
    out
}

fn province(registries: &GameRegistries, name: &str) -> strata_registry::ProvinceId {
    registries
        .province_id(&strata_registry::Namespaced::parse(name).expect("valid"))
        .unwrap_or_else(|| panic!("{name} ships"))
}

/// The least elongation of the patches over [`PATCH_LARGE_AREA_KM2`] the
/// disc does not cut, infinite if there are none.
fn least_large_elongation(patches: &[Patch]) -> f64 {
    measure::least_elongation_over(patches, PATCH_LARGE_AREA_KM2)
}

fn biome(registries: &GameRegistries, name: &str) -> strata_registry::BiomeId {
    registries
        .biome_id(&strata_registry::Namespaced::parse(name).expect("valid"))
        .unwrap_or_else(|| panic!("{name} ships"))
}

fn mountains(registries: &GameRegistries) -> strata_registry::BiomeId {
    biome(registries, "strata:mountains")
}

#[test]
fn seed_42_measures() {
    let t = taken();
    insta::assert_snapshot!(format!(
        "{}{}{}",
        report(&t.measures, &t.registries),
        shape_report(t),
        province_report(&t.measures, &t.tiles, &t.registries)
    ));
}

#[test]
#[allow(
    clippy::too_many_lines,
    reason = "one gate a line, in the ticket's order"
)]
fn seed_42_holds_the_drama_gates() {
    let t = taken();
    let (m, registries) = (&t.measures, &t.registries);
    let elongation = measure::median_elongation(&t.patches);
    let least_large = least_large_elongation(&t.patches);
    let seeing_ten = m.view_shares[1];
    let mountains = m.biome_share(mountains(registries));
    let highlands_id = biome(registries, "strata:highlands");
    let highlands = m.biome_share(highlands_id);
    let highlands_slope = m.biome_relief_slope(highlands_id);
    let flagged = t.profiles.iter().filter(|p| p.flagged()).count();
    let wall = measure::median_wall_m(&t.profiles);
    let land = |name: &str| {
        m.province(province(registries, name))
            .unwrap_or_else(|| panic!("{name} has ground in the standard set"))
    };
    let tile = |name: &str| {
        let id = province(registries, name);
        t.tiles
            .iter()
            .find(|t| t.province == id)
            .unwrap_or_else(|| panic!("{name} has tiles"))
    };
    let (sandstone, badlands) = (land("strata:sandstone"), land("strata:badlands"));
    let basalt = land("strata:basalt");
    let (karst, folded) = (tile("strata:karst"), tile("strata:folded"));
    let failures: Vec<String> = [
        (
            m.view_median_deg >= MIN_VIEW_MEDIAN_DEG,
            format!(
                "median view {:.2} deg < {MIN_VIEW_MEDIAN_DEG}",
                m.view_median_deg
            ),
        ),
        (
            seeing_ten >= MIN_SEEING_TEN_DEG,
            format!("land seeing 10 deg {seeing_ten:.3} < {MIN_SEEING_TEN_DEG}"),
        ),
        (
            m.relief_near_median_m >= MIN_RELIEF_NEAR_M,
            format!(
                "relief within 1 km {:.0} m < {MIN_RELIEF_NEAR_M}",
                m.relief_near_median_m
            ),
        ),
        (
            m.relief_far_median_m >= MIN_RELIEF_FAR_M,
            format!(
                "relief within 5 km {:.0} m < {MIN_RELIEF_FAR_M}",
                m.relief_far_median_m
            ),
        ),
        (
            m.walkable_share >= MIN_WALKABLE,
            format!("walkable {:.3} < {MIN_WALKABLE}", m.walkable_share),
        ),
        (
            (MIN_MOUNTAINS..MAX_MOUNTAINS).contains(&mountains),
            format!("mountains {mountains:.3} outside {MIN_MOUNTAINS}..{MAX_MOUNTAINS}"),
        ),
        (
            highlands >= MIN_HIGHLANDS,
            format!("highlands {highlands:.3} < {MIN_HIGHLANDS}"),
        ),
        (
            highlands_slope >= MIN_HIGHLANDS_RELIEF_SLOPE_DEG,
            format!(
                "highlands' relief slope {highlands_slope:.2} deg < {MIN_HIGHLANDS_RELIEF_SLOPE_DEG}"
            ),
        ),
        (
            elongation >= MIN_MEDIAN_ELONGATION,
            format!("median elongation {elongation:.2} < {MIN_MEDIAN_ELONGATION}"),
        ),
        (
            least_large >= MIN_LARGE_ELONGATION,
            format!(
                "a patch over {PATCH_LARGE_AREA_KM2} km2 has elongation {least_large:.2} < {MIN_LARGE_ELONGATION}"
            ),
        ),
        (
            flagged <= MAX_FLAGGED_PROFILES,
            format!("{flagged} flank profiles flagged > {MAX_FLAGGED_PROFILES}"),
        ),
        (
            wall < MAX_MEDIAN_WALL_M,
            format!("median wall {wall:.0} m >= {MAX_MEDIAN_WALL_M}"),
        ),
        (
            sandstone.flat_share >= MIN_SANDSTONE_FLAT
                && sandstone.steep_share >= MIN_SANDSTONE_STEEP,
            format!(
                "sandstone flat {:.3} (at least {MIN_SANDSTONE_FLAT}), steep {:.3} (at least {MIN_SANDSTONE_STEEP})",
                sandstone.flat_share, sandstone.steep_share
            ),
        ),
        (
            sandstone.riser_share >= MIN_SANDSTONE_RISERS
                && sandstone.riser_mean_m < MAX_BEDDED_RISER_MEAN_M,
            format!(
                "sandstone risers {:.3} of its steep ground (at least {MIN_SANDSTONE_RISERS}), {:.0} m tall (under {MAX_BEDDED_RISER_MEAN_M})",
                sandstone.riser_share, sandstone.riser_mean_m
            ),
        ),
        (
            basalt.riser_share >= MIN_BASALT_RISERS
                && basalt.riser_mean_m < MAX_BEDDED_RISER_MEAN_M,
            format!(
                "basalt risers {:.3} of its steep ground (at least {MIN_BASALT_RISERS}), {:.0} m tall (under {MAX_BEDDED_RISER_MEAN_M})",
                basalt.riser_share, basalt.riser_mean_m
            ),
        ),
        (
            karst.hollows_per_km2 >= MIN_KARST_HOLLOWS_PER_KM2,
            format!(
                "karst {:.2} hollows a km2 < {MIN_KARST_HOLLOWS_PER_KM2}",
                karst.hollows_per_km2
            ),
        ),
        (
            folded.elongation >= MIN_FOLDED_ELONGATION,
            format!(
                "folded elongation {:.2} < {MIN_FOLDED_ELONGATION}",
                folded.elongation
            ),
        ),
        (
            badlands.steep_share >= MIN_BADLANDS_STEEP,
            format!(
                "badlands steep {:.3} < {MIN_BADLANDS_STEEP}",
                badlands.steep_share
            ),
        ),
    ]
    .into_iter()
    .filter(|(ok, _)| !ok)
    .map(|(_, why)| why)
    .collect();
    assert!(
        failures.is_empty(),
        "seed 42 fails its drama gates:\n  {}\n{}{}{}",
        failures.join("\n  "),
        report(m, registries),
        shape_report(t),
        province_report(m, &t.tiles, registries)
    );
}

/// The research probe's own size (4 000 view points; the ground points
/// raised to 200 000 from the probe's 600 000 or so, for time), for numbers
/// to set beside the ticket's baseline. Minutes in a debug build; run it in
/// release:
/// `cargo test -p strata-worldgen --release --test drama -- --ignored --nocapture`.
#[test]
#[ignore = "the probe's full size; run by hand in release"]
fn seed_42_measures_at_the_probes_size() {
    let registries = common::registries();
    let world = common::world(42);
    let started = std::time::Instant::now();
    let points = PointSet::scatter(&world, measure::STANDARD_HALF_EXTENT_M, 4_000, 200_000);
    let m = measure::measure_on(&world, &points, THREADS);
    // The probe counted biomes over its view points, not a ground set.
    let on_views = measure::measure_on(
        &world,
        &PointSet {
            view: Vec::new(),
            ground: points.view.clone(),
        },
        THREADS,
    );
    println!(
        "{}mountains over the {} view points: {:.1} %\n(in {:.1} s)",
        report(&m, &registries),
        points.view.len(),
        on_views.biome_share(mountains(&registries)) * 100.0,
        started.elapsed().as_secs_f64()
    );
}
