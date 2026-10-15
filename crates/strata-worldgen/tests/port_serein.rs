//! Port Serein's surroundings on seed 42 as the ticket 0004 steps left
//! them: where a player starts stands on ground, and the ranges brought
//! near stay out of the city's neighbourhood.
//!
//! The city's own ground and layout are ADR 0009's and pinned by its site
//! fingerprint; these are the natural ground round it, which a change to
//! the ranges moves.

mod common;

use strata_worldgen::{WorldSource, find_land_near};

/// The mountains' ground may come no nearer the city's centre than this.
/// 24.3 km before ticket 0004's step 2, 15.0 km after it and 14.5 km after
/// its review.
const MOUNTAINS_CLEAR_M: f64 = 3_000.0;

/// Where the city stands (the fog test's `PORT_SEREIN`, its lower
/// platform above the bay).
const CITY: (f64, f64) = (-400.0, -500.0);

#[test]
fn a_player_starts_on_ground() {
    let world = common::world(42);
    let sea = world.sea_level();
    // The default spawn, as the game and the server look for it.
    let (x, z) = find_land_near(&world, 0.0, 0.0, 5.0, 250.0, 30_000.0).expect("land near");
    let spawn = world.sample(x, z);
    assert!(
        spawn.height > sea,
        "the default spawn ({x}, {z}) is under the sea: {spawn:?}"
    );
    // `--at road`: the far end of the city's road.
    let site = &world.sites()[0];
    let end = *site.road.points.last().expect("the road has points");
    let road = world.sample(end[0], end[1]);
    assert!(
        road.height > sea + 2.0 && road.landform.slope < 0.2,
        "the road ends at ({:.0}, {:.0}) on {road:?}",
        end[0],
        end[1]
    );
}

#[test]
fn the_mountains_keep_their_distance_from_the_city() {
    let registries = common::registries();
    let world = common::world(42);
    let mountains = registries
        .biome_id(&strata_registry::Namespaced::parse("strata:mountains").expect("valid"))
        .expect("ships");
    let step = 250.0;
    // A count of steps; exact.
    #[allow(clippy::cast_possible_truncation)]
    let reach = (MOUNTAINS_CLEAR_M / step) as i32;
    for j in -reach..=reach {
        for i in -reach..=reach {
            let (dx, dz) = (f64::from(i) * step, f64::from(j) * step);
            if dx.hypot(dz) > MOUNTAINS_CLEAR_M {
                continue;
            }
            let s = world.sample(CITY.0 + dx, CITY.1 + dz);
            let weight = s
                .biomes
                .iter()
                .find(|(id, _)| *id == mountains)
                .map_or(0.0, |(_, w)| *w);
            assert!(
                weight == 0.0,
                "mountains at ({:.0}, {:.0}), within {MOUNTAINS_CLEAR_M} m of the city: {s:?}",
                CITY.0 + dx,
                CITY.1 + dz
            );
        }
    }
}

/// The tallest thing in view from the city, of the world's ground, may not
/// fall under this, in degrees (ticket 0004, step 2b): the city has
/// something to look at. The city's own hillside is left out of the view,
/// since from the quay it stands higher than anything the world has.
const CITY_VIEW_MIN_DEG: f64 = 6.0;

/// The city's waterfront and its upper terrace, in the site's frame (`+z`
/// downhill to the bay): the quay below the Exchange, and the top of the
/// hillside under the processor's shelf.
const CITY_EYES: [(&str, [f64; 2]); 2] = [
    ("waterfront", [-100.0, 150.0]),
    ("upper terrace", [-100.0, -340.0]),
];

/// From the waterfront and the upper terrace the world stands at least
/// [`CITY_VIEW_MIN_DEG`] over the horizon. 7.2 and 0.8 degrees before step
/// 2b, whose nearest range stands 9 km from the city towards `-z`; 7.1 and
/// 6.9 after, 7.2 and 6.7 since its review; 6.20 and 6.06 after step 4,
/// whose ranges carry most of their height as a profile of their own, and
/// 6.17 and 6.03 since its review: met, by a few hundredths of a degree at
/// the terrace.
#[test]
fn the_city_has_something_to_look_at() {
    let world = common::world(42);
    let site = &world.sites()[0];
    for (name, local) in CITY_EYES {
        let [x, z] = site.to_world_xz(local);
        let view =
            strata_worldgen::measure::view_deg_where(&world, x, z, |px, pz| !site.touches(px, pz));
        assert!(
            view >= CITY_VIEW_MIN_DEG,
            "from the {name} at ({x:.0}, {z:.0}) the world stands {view:.1} deg over the horizon"
        );
    }
}

/// The land within 10 km of the city, outside its own ground, by dominant
/// biome: shares of the land points on a 100 m lattice, the biome's name
/// with each.
fn country(world: &strata_worldgen::ProceduralWorld) -> Vec<(String, f64)> {
    let registries = common::registries();
    let step = 100.0;
    let radius = 10_000.0;
    let mut counts = vec![0_u32; registries.biomes.len()];
    let mut land = 0_u32;
    for j in -100..=100 {
        for i in -100..=100 {
            let (dx, dz) = (f64::from(i) * step, f64::from(j) * step);
            if dx.hypot(dz) > radius {
                continue;
            }
            let s = world.sample(CITY.0 + dx, CITY.1 + dz);
            // The measures' land, and not the city's own ground or its
            // road: those are the settlement's, whatever the climate.
            if f64::from(s.height) <= strata_worldgen::measure::LAND_MIN_M
                || s.landform.developed > 0.0
            {
                continue;
            }
            counts[s.dominant_biome().0 as usize] += 1;
            land += 1;
        }
    }
    registries
        .biomes
        .iter()
        .map(|(id, name, _)| {
            (
                name.to_string(),
                f64::from(counts[id.0 as usize]) / f64::from(land),
            )
        })
        .collect()
}

/// Port Serein stands in a temperate country (ticket 0004's decision 4 of
/// 2026-09-30, step 2b): the climate moved, not the city. Of the land within
/// 10 km, plains and forest are at least three fifths, desert and savanna at
/// most a twentieth, tundra none; the rest may be the coast's beach, the
/// highlands and the mountains. Before the step it was 96 % coast (hot, wet
/// ground that fits no lowland biome) and 3 % savanna; after, 64 % forest
/// and 0.5 % plains (63 % and 0.5 % since its review). The step's target that plains and forest each be at
/// least 15 % is not met, and not held: the rain march saturates the air
/// over the sea upwind (`-x`), and every point within 10 km is wetter than
/// the plains' window allows.
#[test]
fn the_city_stands_in_a_temperate_country() {
    let world = common::world(42);
    let shares = country(&world);
    let share = |name: &str| {
        shares
            .iter()
            .find(|(n, _)| n == name)
            .map_or(0.0, |(_, s)| *s)
    };
    let temperate = share("strata:plains") + share("strata:forest");
    let hot = share("strata:desert") + share("strata:savanna");
    let cold = share("strata:tundra");
    assert!(
        temperate >= 0.6 && hot <= 0.05 && cold == 0.0,
        "the land within 10 km of the city: plains and forest {temperate:.3}, desert and savanna {hot:.3}, tundra {cold:.3}: {shares:?}"
    );
}

/// The temperature at the city lies in the middle third of both the
/// plains' and the forest's windows, so the country stays temperate some
/// way either side of it in `z` rather than at the edge of a desert or a
/// tundra. 0.98 before step 2b, 0.485 after.
#[test]
fn the_city_is_in_the_middle_of_the_temperate_windows() {
    let registries = common::registries();
    let world = common::world(42);
    let continent = world.continent(CITY.0, CITY.1);
    let t = world.climate(CITY.0, CITY.1, &continent).temperature;
    for name in ["strata:plains", "strata:forest"] {
        let id = registries
            .biome_id(&strata_registry::Namespaced::parse(name).expect("valid"))
            .expect("ships");
        let def = registries.biomes.get(id).expect("frozen");
        let (lo, hi) = (
            f64::from(def.climate.temperature.min),
            f64::from(def.climate.temperature.max),
        );
        let third = (hi - lo) / 3.0;
        assert!(
            t >= lo + third && t <= hi - third,
            "the city's temperature {t:.3} is outside the middle third of {name}'s {lo}..{hi}"
        );
    }
}
