//! The bake on small synthetic domains and small windows of the real world:
//! never a whole large mass, so each test is quick in a debug build.

use super::*;
use crate::hydrology::field::Node;
use crate::hydrology::mass::{Domain, NEIGHBOURS, OutletKind};

fn world() -> ProceduralWorld {
    let registries =
        strata_registry::load_from_dir(concat!(env!("CARGO_MANIFEST_DIR"), "/../../assets"))
            .expect("the shipped assets load");
    ProceduralWorld::new(Seed(42), &registries)
}

/// The canaries' hashes, pinned: what the world's version folds in (the
/// island, its two cut parts, the window round Port Serein). A change here
/// is a change to rivers everywhere; if it is meant, paste the new values
/// and bump `GENERATOR_VERSION` (its doc says why).
/// Moved by ticket 0005's step 2, which sets every reach's surface onto the
/// sample's ground, grades its breaches and its mouth to the sample's shore
/// (and records the cut) and routes along a province's grain, from step
/// 1's `[0xa2ee_52e7_07b0_9abe, 0x7c8a_7817_cb0d_95fc,
/// 0x95f1_4f81_7b82_10c9, 0x26f7_9d74_dbc2_8398]`; and by its step 3b,
/// which settles every reach onto the sample's low ground, bends its
/// tributaries, gives it meanders and holds its outlets at their lakes'
/// levels to a vertex where each leaves its rim, from step 2's `[0x400f_2d09_0073_7faf, 0x9f1d_b497_a50b_1693,
/// 0x5be2_76b3_1ac2_7097, 0x0296_7f1a_a5d2_3805]`; and by its step 4, the
/// sites each network places on its mass and folds into its hash, from
/// step 3b's [`CANARIES_BEFORE_SITES`] (which a world without site kinds
/// still bakes); `[0xd11d_f0eb_f038_0d26, 0x7ac0_0a53_c236_043b,
/// 0xc4e1_d23f_dea5_3038, 0x29f9_2eed_77bc_7a60]` as step 4 was first built,
/// before its review moved the pads; and by its step 5, the roads between
/// the sites, folded into the hash after them (the ground did not move),
/// from step 4's `[0x1b5e_adb4_4535_ddf3, 0xa3b1_022a_5a87_4259,
/// 0x635f_c3f7_d8f2_e483, 0xa786_f91e_15a8_7112]`; and by its step 6, each
/// road's profile, from step 5's `[0xd399_40c2_2c0b_fbc3,
/// 0xf2c8_3696_2399_2d77, 0x4982_c8cd_90af_fa05, 0xc44e_76e5_b72a_391f]`
/// (`[0x28de_8e04_d0e1_8542, 0x64f5_9fc4_ea6f_4fb8, 0x5eec_203c_3db2_a67a,
/// 0xce98_0abd_c4d7_4206]` as steps 5 and 6 were first built, before their
/// review moved the lines, the decks and the junctions' holds); and by the
/// roads' alignment pass, whose search prices cut and fill and turns near a
/// river and whose lines run straight through their bridges (the rivers and
/// the sites did not move: [`CANARIES_BEFORE_SITES`] holds), from step 6's
/// `[0xdeb6_e6eb_f257_c640, 0xda36_6cc7_bada_9a34, 0x2354_d442_ee40_0b10,
/// 0x2b41_28ab_628d_b2ed]` (the island `0xf405_7b0f_2407_d274` as the pass
/// was first built, before its review held each bridge's shelf out of the
/// channel and kept the straights off held points and their ground); its
/// second round, whose search reads a crossing's banks as the record does
/// and closes one the kit cannot stand, and whose service roads search a
/// wider corridor where they cross a river, moved the roads alone again,
/// from the first round's `[0x97b7_f4cc_8189_6aa8, 0x1478_b1bc_245d_1ff1,
/// 0xaa74_ed40_4ed5_d6b9, 0xc4ef_4579_e517_2d5c]`; its third, the wall
/// search round a bridge whose searched line comes down past its grade,
/// moved the island's roads alone, from the second round's
/// `0x48eb_da47_c489_23cb`; and by ticket 0012's new site kinds (the outlaw
/// camp, the wreck and the derelict machine) and their tracks, folded in
/// after the old sites (the rivers did not move: [`CANARIES_BEFORE_SITES`]
/// holds), from `[0xc5ae_4916_d5c4_024a, 0x1478_b1bc_245d_1ff1,
/// 0x6d9e_85c8_c8ce_82e6, 0x9af2_264c_c2f8_4efb]`.
const PINNED_CANARIES: [u64; 4] = [
    0xffa6_b971_711a_d4d7,
    0x92b3_1d6a_f85f_00e8,
    0x7dd6_7cf1_9c70_ae36,
    0x9af2_264c_c2f8_4efb,
];

/// The canary island's network, baked whole.
fn canary_mass(world: &ProceduralWorld) -> Arc<Network> {
    world
        .hydrology()
        .network_at(world, metres(CANARY_MASS[0]), metres(CANARY_MASS[1]))
        .expect("the canary island is land")
}

/// A cone island 15 nodes in radius with ridges on it, a pit 80 m deep
/// (a lake, its province keeping pits of 10 m) and one 4 m deep (breached),
/// the sea round it.
fn island() -> Domain {
    let mut nodes = Vec::new();
    let mut field = Vec::new();
    let inside = |i: i32, j: i32| i * i + j * j <= 15 * 15;
    for i in -16..=16 {
        for j in -16..=16 {
            if !inside(i, j) {
                continue;
            }
            let (x, z) = (f64::from(i), f64::from(j));
            let r = (x * x + z * z).sqrt();
            let mut h = 400.0 * (1.0 - r / 16.0) + 25.0 * ((x * 0.9).sin() * (z * 0.7).cos());
            let pit = |cx: f64, cz: f64, depth: f64| {
                let d2 = (x - cx) * (x - cx) + (z - cz) * (z - cz);
                if d2 < 4.0 {
                    depth * (1.0 - d2 / 4.0)
                } else {
                    0.0
                }
            };
            h -= pit(5.0, 5.0, 120.0) + pit(-6.0, -4.0, 4.0);
            nodes.push([i, j]);
            field.push(Node {
                height: h.max(1.0),
                rain: 0.8,
                keep: 10.0,
                sink: 0.0,
                grain: 0.0,
            });
        }
    }
    let outlet = nodes
        .iter()
        .map(|&[i, j]| {
            NEIGHBOURS
                .iter()
                .map(|d| [i + d[0], j + d[1]])
                .find(|&[a, b]| !inside(a, b))
                .map(|m| (m, OutletKind::Sea, -20.0))
        })
        .collect();
    Domain {
        nodes,
        field,
        outlet,
        capped: false,
    }
}

/// The island's network, as a bake would make it.
fn island_network() -> Network {
    Hydrology::new(Seed(7)).finish(None, &island(), Instant::now(), 0, 1)
}

/// Where each lake drains to: the reach that leaves it, if one does.
fn drains(network: &Network) -> NodeMap<u32> {
    let mut out = NodeMap::default();
    for (i, river) in network.rivers().iter().enumerate() {
        if let Some(lake) = river.from_lake {
            out.insert([i32::try_from(lake).expect("a lake"), 0], local(i));
        }
    }
    out
}

/// Every reach runs downhill and gathers water: no vertex's surface over
/// the one before, no discharge under it, and a reach that joins another
/// meets its first vertex at its own level or above, with no more water.
fn assert_rivers_run_down(network: &Network) {
    for (i, river) in network.rivers().iter().enumerate() {
        assert!(river.vertices.len() >= 2, "reach {i} is a line");
        for pair in river.vertices.windows(2) {
            assert!(
                pair[1].surface_m <= pair[0].surface_m,
                "reach {i} rises: {} to {}",
                pair[0].surface_m,
                pair[1].surface_m
            );
            assert!(
                pair[1].discharge_m3s >= pair[0].discharge_m3s,
                "reach {i} loses water: {} to {}",
                pair[0].discharge_m3s,
                pair[1].discharge_m3s
            );
        }
        let last = river.vertices[river.vertices.len() - 1];
        match river.end {
            RiverEnd::Joins(next) => {
                let first = network.rivers()[next as usize].vertices[0];
                assert_eq!(
                    (last.x, last.z),
                    (first.x, first.z),
                    "reach {i} meets {next}"
                );
                assert!(
                    first.surface_m <= last.surface_m,
                    "reach {i} climbs into {next}"
                );
                assert!(
                    first.discharge_m3s >= last.discharge_m3s,
                    "reach {i} into {next}"
                );
                assert!(network.rivers()[next as usize].order >= river.order);
            }
            RiverEnd::Lake(lake) => {
                let level = network.lakes()[lake as usize].level_m;
                assert!(
                    level <= last.surface_m + 1e-3,
                    "reach {i} into a lake above it"
                );
            }
            RiverEnd::Sea => assert!(
                last.surface_m.abs() < 1e-6,
                "a mouth at sea level: reach {i} ends at {} at ({}, {})",
                last.surface_m,
                last.x,
                last.z
            ),
            RiverEnd::Edge | RiverEnd::Sink => {}
        }
    }
}

/// Follows every reach down to where its water leaves the domain: the sea
/// (or a window's edge, which a window bake treats as a coast), through any
/// lakes on the way; never round in a circle and never into a sink.
fn assert_rivers_reach_the_sea(network: &Network) {
    let drains = drains(network);
    for start in 0..network.rivers().len() {
        let mut at = start;
        let mut steps = 0;
        loop {
            steps += 1;
            assert!(steps <= network.rivers().len() + 1, "reach {start} loops");
            match network.rivers()[at].end {
                RiverEnd::Sea | RiverEnd::Edge => break,
                RiverEnd::Joins(next) => at = next as usize,
                RiverEnd::Lake(lake) => {
                    let key = [i32::try_from(lake).expect("a lake"), 0];
                    // A lake whose outflow is under the river threshold
                    // drains by water too small to draw; its outlet is
                    // still downhill of it.
                    let Some(&next) = drains.get(&key) else { break };
                    at = next as usize;
                }
                RiverEnd::Sink => panic!("reach {start} ends in a sink"),
            }
        }
    }
}

#[test]
fn an_island_drains_to_the_sea_downhill() {
    let network = island_network();
    assert!(
        network.rivers().len() >= 4,
        "{} reaches",
        network.rivers().len()
    );
    assert_rivers_run_down(&network);
    assert_rivers_reach_the_sea(&network);
}

#[test]
fn a_deep_pit_is_kept_and_a_shallow_one_breached() {
    let network = island_network();
    assert_eq!(network.lakes().len(), 1, "{:?}", network.lakes());
    let lake = &network.lakes()[0];
    assert!(
        lake.nodes.contains(&[5, 5]),
        "the deep pit's floor is under it"
    );
    assert!(lake.depth_m > 10.0);
    assert!(
        !lake.nodes.iter().any(|n| n[0] < 0),
        "the shallow pit is not"
    );
    // Its water leaves it: by a river, or by water under the threshold.
    assert!(lake.discharge_m3s > 0.0);
}

#[test]
fn discharge_is_the_rain_upstream() {
    // Everything rains 0.8 of two metres a year on 512 m nodes; what
    // reaches the sea is all of it, less nothing.
    let domain = island();
    let routed = route::route(&domain, &RiverParams::DEFAULT, &[], (1.0, 0.0));
    let per_node = 0.8 * 2.0 * LATTICE_M * LATTICE_M / 31_557_600.0;
    let out: f64 = (0..domain.nodes.len())
        .filter(|&k| routed.receiver[k] == route::NONE)
        .map(|k| routed.discharge[k])
        .sum();
    let all = per_node * real(domain.nodes.len());
    assert!((out - all).abs() < 1e-9 * all, "{out} of {all}");
    for &k in &routed.order {
        let r = routed.receiver[k as usize];
        if r != route::NONE {
            assert!(routed.surface[r as usize] <= routed.surface[k as usize]);
        }
    }
}

#[test]
fn the_canaries_are_pinned_and_the_same_on_any_thread_count() {
    let world = world();
    let hashes = canary_hashes(&world, &canary_window(&world));
    assert_eq!(
        canary_hashes(&world, &canary_window(&world)),
        hashes,
        "a rebake"
    );
    let four = Hydrology::new(Seed(42));
    four.set_threads(4);
    let mass = four
        .network_at(&world, metres(CANARY_MASS[0]), metres(CANARY_MASS[1]))
        .expect("land");
    assert_eq!(mass.content_hash(), hashes[0], "the island on four threads");
    // On four threads for the field, the settling and the lakes' ground
    // (ticket 0005, step 3b): the same network as the canary's one.
    let window = four.bake_window(&world, CANARY_WINDOW.0, CANARY_WINDOW.1, 4);
    assert_eq!(
        window.content_hash(),
        hashes[3],
        "the window on four threads"
    );
    // What the canaries are there to reach.
    assert!(!mass.capped());
    assert!(mass.lakes().len() >= 10, "{} lakes", mass.lakes().len());
    assert!(!mass.falls().is_empty(), "no falls on the island");
    assert!(mass.rivers().iter().any(|r| r.end == RiverEnd::Sea));
    let cut = CANARY_CUT.2.map(|node| {
        four.bake_cut(&world, node, (CANARY_CUT.0, CANARY_CUT.1))
            .expect("land")
    });
    assert!(cut.iter().all(Network::capped));
    assert!(cut[0].rivers().iter().any(|r| r.end == RiverEnd::Sea));
    assert!(cut[1].rivers().iter().any(|r| r.end == RiverEnd::Edge));
    assert!(cut[1].rivers().iter().all(|r| r.end != RiverEnd::Sea));
    assert!(window.rivers().len() >= 3, "rivers round the city");
    assert_eq!(
        hashes, PINNED_CANARIES,
        "the canary networks moved ({hashes:#018x?}): rivers did; if on purpose, paste them into PINNED_CANARIES"
    );
}

/// The canaries of step 3b, before sites existed: a world without site
/// kinds bakes the same networks to the bit (ADR 0012: a set with none is
/// the world as it was).
const CANARIES_BEFORE_SITES: [u64; 4] = [
    0x3809_1d37_e010_9538,
    0xb098_99e2_366c_6568,
    0x0d85_1ec1_feb0_1267,
    0xdf87_f3bd_734c_e1a3,
];

#[test]
fn without_site_kinds_the_networks_are_the_ones_before_sites() {
    let mut registries =
        strata_registry::load_from_dir(concat!(env!("CARGO_MANIFEST_DIR"), "/../../assets"))
            .expect("the shipped assets load");
    registries.sites = strata_registry::SiteKindStore::default();
    let bare = ProceduralWorld::new(Seed(42), &registries);
    assert_eq!(
        canary_hashes(&bare, &canary_window(&bare)),
        CANARIES_BEFORE_SITES
    );
}

/// Every pad on the canary island stands on ground that can take it, read
/// on the ground its network carves with no pad laid: no standing water, no
/// channel, no carved wall, no settlement's ground, nothing below the sea,
/// over its whole footprint, and no water or channel to the band's edge;
/// and its pad cuts and fills no deeper than its kind allows.
#[test]
fn every_pad_on_the_canary_island_stands_on_sound_ground() {
    let world = world();
    let mass = canary_mass(&world);
    assert!(mass.sites().len() >= 10, "{} sites", mass.sites().len());
    for site in mass.sites() {
        let kind = world
            .site_kinds()
            .iter()
            .find(|kind| kind.id == site.kind())
            .expect("a kind");
        assert!(site.cut_m <= kind.def.max_cut_m, "{site:?}");
        for (radius, ring) in [
            (0.0, 1),
            (site.radius_m * 0.5, 8),
            (site.radius_m, 16),
            (site.outer_m() * 0.95, 16),
        ] {
            for k in 0..ring {
                let a = std::f64::consts::TAU * f64::from(k) / f64::from(ring);
                let (x, z) = (
                    site.centre[0] + radius * a.cos(),
                    site.centre[1] + radius * a.sin(),
                );
                let s = world.sample_by(x, z, &mass);
                if radius <= site.radius_m {
                    assert!(
                        super::place::sound(&s) && s.height >= 0.5,
                        "{}'s pad at ({x:.0}, {z:.0}): {s:?}",
                        site.name
                    );
                } else {
                    // The band's outer edge may lie on a gorge's wall or
                    // the sea's edge; never in water or a channel.
                    let channel = s.water.is_some_and(|w| {
                        w.kind != WaterKind::Lake && w.signed_distance_m.abs() <= w.bank_m()
                    });
                    assert!(
                        s.standing_water().is_none() && !channel,
                        "{}'s band at ({x:.0}, {z:.0}): {s:?}",
                        site.name
                    );
                }
            }
        }
    }
}

#[test]
fn the_canaries_run_down_to_the_sea_or_their_edge() {
    let world = world();
    let mass = canary_mass(&world);
    assert_rivers_run_down(&mass);
    assert_rivers_reach_the_sea(&mass);
    for river in mass.rivers() {
        assert_ne!(river.end, RiverEnd::Edge, "a whole mass has no edge");
    }
    for node in CANARY_CUT.2 {
        let part = world
            .hydrology()
            .bake_cut(&world, node, (CANARY_CUT.0, CANARY_CUT.1))
            .expect("land");
        assert_rivers_run_down(&part);
        assert_rivers_reach_the_sea(&part);
    }
    let window = world
        .hydrology()
        .bake_window(&world, CANARY_WINDOW.0, CANARY_WINDOW.1, 1);
    assert_rivers_run_down(&window);
    assert_rivers_reach_the_sea(&window);
}

#[test]
fn the_canary_island_routes_without_loops_and_keeps_its_water() {
    let world = world();
    let hydrology = world.hydrology();
    let (domain, _) = mass::find(hydrology, &world, CANARY_MASS, false, hydrology.cap, 1);
    let routed = route::route(
        &domain,
        &RiverParams::DEFAULT,
        world.sites(),
        world.range_axis(),
    );
    let n = domain.nodes.len();
    // Every routed node once in the order, after its receiver: no loop.
    let mut place = vec![usize::MAX; n];
    for (at, &k) in routed.order.iter().enumerate() {
        assert_eq!(place[k as usize], usize::MAX, "node {k} twice");
        place[k as usize] = at;
    }
    for k in 0..n {
        if routed.blocked[k] {
            continue;
        }
        assert_ne!(place[k], usize::MAX, "node {k} on a loop");
        let r = routed.receiver[k];
        if r == route::NONE {
            assert!(matches!(routed.root[k], Some(route::Root::Out(..))));
        } else {
            assert!(place[r as usize] < place[k]);
            assert!(routed.surface[r as usize] <= routed.surface[k]);
        }
    }
    // A lake's outlet leads out of it for good.
    for (id, lake) in routed.lakes.iter().enumerate() {
        let mut p = routed.receiver[lake.outlet as usize];
        while p != route::NONE {
            assert_ne!(routed.lake[p as usize], local(id), "lake {id} drains back");
            p = routed.receiver[p as usize];
        }
    }
    // The rain in is the water out.
    let per_node = 2.0 * LATTICE_M * LATTICE_M / 31_557_600.0;
    let rain: f64 = domain
        .field
        .iter()
        .zip(&routed.blocked)
        .filter(|(_, blocked)| !**blocked)
        .map(|(f, _)| f.rain * (1.0 - f.sink) * per_node)
        .sum();
    let out: f64 = (0..n)
        .filter(|&k| !routed.blocked[k] && routed.receiver[k] == route::NONE)
        .map(|k| routed.discharge[k])
        .sum();
    assert!((out - rain).abs() < 1e-9 * rain, "{out} of {rain}");
}

#[test]
fn no_river_runs_through_port_serein() {
    let world = world();
    let city = &world.sites()[0];
    // About 11 km round the city, its edge a coast.
    let network = world
        .hydrology()
        .bake_window(&world, CANARY_WINDOW.0, CANARY_WINDOW.1, 1);
    assert!(network.rivers().len() >= 3, "rivers near the city");
    for river in network.rivers() {
        for pair in river.vertices.windows(2) {
            let (from, to) = (pair[0], pair[1]);
            let mut along = 0.0;
            while along <= 1.0 {
                let (x, z) = (
                    from.x + (to.x - from.x) * along,
                    from.z + (to.z - from.z) * along,
                );
                let clear = city.extent_distance_at(x, z);
                assert!(
                    clear > f64::from(from.width_m.max(to.width_m)) * 0.5,
                    "a river {clear:.0} m from the extent at ({x:.0}, {z:.0})"
                );
                along += 0.05;
            }
        }
    }
}

#[test]
fn the_query_finds_the_polylines() {
    let world = world();
    let network = canary_mass(&world);
    let params = RiverParams::DEFAULT;
    let mut checked = 0;
    for river in network.rivers() {
        for pair in river.vertices.windows(2).step_by(3) {
            let (a, b) = (pair[0], pair[1]);
            let (dx, dz) = (b.x - a.x, b.z - a.z);
            let length = (dx * dx + dz * dz).sqrt();
            if length < 1.0 || a.fall {
                continue;
            }
            let (mx, mz) = (f64::midpoint(a.x, b.x), f64::midpoint(a.z, b.z));
            let on = network.water_at(mx, mz).expect("a centreline is water");
            // A reach's stretch into a lake lies over it, and the lake wins.
            if on.kind == WaterKind::Lake {
                continue;
            }
            assert!(on.signed_distance_m.abs() < 1e-3, "{on:?}");
            // Another reach can pass this close only at a confluence.
            let q = f64::midpoint(f64::from(a.discharge_m3s), f64::from(b.discharge_m3s));
            let near_q = (f64::from(on.discharge_m3s) - q).abs() < 1e-3 * q.max(1.0);
            if near_q {
                assert!((f64::from(on.width_m) - params.width(q)).abs() < 1e-3);
                let s = f64::midpoint(f64::from(a.surface_m), f64::from(b.surface_m));
                assert!((f64::from(on.surface_m) - s).abs() < 1e-2, "{on:?}");
                let flow = [dx / length, dz / length];
                assert!(
                    (f64::from(on.flow_dir[0]) - flow[0]).abs() < 1e-3
                        && (f64::from(on.flow_dir[1]) - flow[1]).abs() < 1e-3
                );
                // A metre to the side of the flow's left normal.
                let (sx, sz) = (mx - flow[1], mz + flow[0]);
                let side = network.water_at(sx, sz).expect("beside it");
                // Unless a sharp bend puts another segment nearer: the routing
                // turns along a province's grain and back across it, and a
                // short segment's neighbour can come within a metre.
                let same = (f64::from(side.flow_dir[0]) - flow[0]).abs() < 1e-3
                    && (f64::from(side.flow_dir[1]) - flow[1]).abs() < 1e-3;
                assert!(
                    !same || (side.signed_distance_m > 0.0 && side.signed_distance_m <= 1.0 + 1e-6),
                    "{side:?}"
                );
                checked += 1;
            }
        }
    }
    assert!(checked > 50, "{checked} points checked");
    // On each lake's nodes the point is in that lake, or in one deeper
    // there.
    for lake in network.lakes() {
        for &[i, j] in &lake.nodes {
            let water = network
                .water_at(metres(i), metres(j))
                .expect("a lake node is wet");
            assert_eq!(water.kind, WaterKind::Lake);
            assert!(water.depth_m > 0.0 && water.signed_distance_m < 0.0);
        }
    }
    // Out at sea, far from any river: dry.
    assert!(network.water_at(-30_000.0, 40_000.0).is_none());
}

#[test]
fn a_cut_mass_is_cut_the_same_from_any_of_its_nodes() {
    // The 1 600 km2 island, under a cap of 2 000 nodes in cells of 32: its
    // part round one node is the same mass whichever node asks first.
    let world = world();
    let (a, b) = ([-74, 120], [-70, 118]);
    let first = Hydrology::with_cap(Seed(42), 2_000, 32);
    let from_a = first
        .network_at(&world, metres(a[0]), metres(a[1]))
        .expect("land");
    let second = Hydrology::with_cap(Seed(42), 2_000, 32);
    let from_b = second
        .network_at(&world, metres(b[0]), metres(b[1]))
        .expect("land");
    assert!(from_a.capped() && from_b.capped());
    let cell = |n: [i32; 2]| [n[0].div_euclid(32), n[1].div_euclid(32)];
    assert_eq!(cell(a), cell(b), "the two nodes share a cell");
    assert_eq!(from_a.key(), from_b.key());
    assert_eq!(from_a.content_hash(), from_b.content_hash());
    // A node in another cell that the first count marked as over the cap
    // is baked without a count; it is the part a fresh cache counts to.
    let marked = (-90..-40)
        .flat_map(|i| (100..140).map(move |j| [i, j]))
        .find(|&n| {
            let (key, k) = field::locate(n);
            cell(n) != cell(a)
                && first.tile(&world, key, 1).label[k].load(Ordering::Acquire) == LARGE
        })
        .expect("a marked node in another cell");
    let known = first
        .network_at(&world, metres(marked[0]), metres(marked[1]))
        .expect("land");
    let counted = Hydrology::with_cap(Seed(42), 2_000, 32)
        .network_at(&world, metres(marked[0]), metres(marked[1]))
        .expect("land");
    assert!(known.capped());
    assert_eq!(known.key(), counted.key());
    assert_eq!(known.content_hash(), counted.content_hash());
    // Every vertex lies on the part's cell, give or take the jitter and the
    // sway, but on a reach's last lattice step, which runs out to the sea
    // node (or the cut edge's) it leaves by: the divide holds.
    let home = cell(from_a.key());
    let p = RiverParams::DEFAULT;
    let slack = p.jitter * LATTICE_M + p.meander_m;
    let span = |c: i32| (metres(c * 32) - slack, metres(c * 32 + 31) + slack);
    let ((x0, x1), (z0, z1)) = (span(home[0]), span(home[1]));
    for river in from_a.rivers() {
        let last = river.vertices[river.vertices.len() - 1];
        for v in &river.vertices {
            if (v.x - last.x).hypot(v.z - last.z) < LATTICE_M * 1.5 {
                continue;
            }
            assert!(
                v.x >= x0 && v.x <= x1 && v.z >= z0 && v.z <= z1,
                "a vertex off its cell at ({:.0}, {:.0})",
                v.x,
                v.z
            );
        }
    }
    assert_rivers_run_down(&from_a);
}

#[test]
fn many_threads_asking_at_once_bake_each_mass_once() {
    let world = world();
    let points: Vec<(f64, f64)> = (0..64)
        .map(|k| {
            (
                -37_888.0 + f64::from(k % 8) * 900.0,
                61_440.0 + f64::from(k / 8) * 900.0,
            )
        })
        .collect();
    let answers: Vec<Vec<Option<Water>>> = std::thread::scope(|scope| {
        let workers: Vec<_> = (0..4)
            .map(|_| scope.spawn(|| points.iter().map(|&(x, z)| world.water_at(x, z)).collect()))
            .collect();
        workers
            .into_iter()
            .map(|w| w.join().expect("a query thread"))
            .collect()
    });
    for other in &answers[1..] {
        assert_eq!(&answers[0], other);
    }
    let keys: Vec<_> = world
        .hydrology()
        .networks()
        .iter()
        .map(|n| n.key())
        .collect();
    let mut unique = keys.clone();
    unique.dedup();
    assert_eq!(keys, unique, "a mass baked twice");
}

#[test]
fn hydraulic_geometry_multiplies_to_the_discharge() {
    let p = RiverParams::DEFAULT;
    for q in [0.1, 1.0, 7.5, 40.0] {
        let product = p.width(q) * p.depth(q) * p.speed(q);
        assert!((product - q).abs() < 1e-9 * q, "{q}: {product}");
    }
    assert!((p.depth(16.0) / p.depth(1.0) - 16.0_f64.powf(0.375)).abs() < 1e-9);
}

#[test]
#[ignore = "a probe: run in release"]
#[allow(
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    clippy::cast_possible_wrap,
    clippy::cast_sign_loss,
    clippy::cast_lossless,
    clippy::too_many_lines,
    clippy::many_single_char_names,
    reason = "a measure's arithmetic on grid indices far under 2^31"
)]
fn probe_masses() {
    let world = world();
    let step = 2_000.0;
    let half = 1_500;
    let side = (2 * half) as usize;
    let rows = field::parallel_rows(side, 16, |row| {
        (0..side)
            .map(|col| {
                let x = (col as f64 - half as f64) * step;
                let z = (row as f64 - half as f64) * step;
                world.coarse_ground(x, z).height >= 0.0
            })
            .collect::<Vec<_>>()
    });
    let land: Vec<bool> = rows.into_iter().flatten().collect();
    let mut label = vec![u32::MAX; side * side];
    let mut comps = Vec::new();
    for s in 0..side * side {
        if !land[s] || label[s] != u32::MAX {
            continue;
        }
        let id = comps.len() as u32;
        let mut count = 0usize;
        let mut bb = [usize::MAX, 0, usize::MAX, 0];
        let mut stack = vec![s];
        label[s] = id;
        while let Some(p) = stack.pop() {
            count += 1;
            let (i, j) = (p % side, p / side);
            bb = [bb[0].min(i), bb[1].max(i), bb[2].min(j), bb[3].max(j)];
            for (di, dj) in [
                (-1i64, 0i64),
                (1, 0),
                (0, -1),
                (0, 1),
                (-1, -1),
                (1, -1),
                (-1, 1),
                (1, 1),
            ] {
                let (a, b) = (i as i64 + di, j as i64 + dj);
                if a < 0 || b < 0 || a >= side as i64 || b >= side as i64 {
                    continue;
                }
                let q = b as usize * side + a as usize;
                if land[q] && label[q] == u32::MAX {
                    label[q] = id;
                    stack.push(q);
                }
            }
        }
        comps.push((count, bb, s));
    }
    comps.sort_by_key(|c| std::cmp::Reverse(c.0));
    for (count, bb, s) in comps.iter().take(6) {
        let (i, j) = (s % side, s / side);
        println!(
            "area {:.0} km2, extent {:.0} x {:.0} km, touches edge {}, a node at ({:.0}, {:.0})",
            *count as f64 * 4.0,
            (bb[1] - bb[0] + 1) as f64 * 2.0,
            (bb[3] - bb[2] + 1) as f64 * 2.0,
            bb[0] == 0 || bb[2] == 0 || bb[1] == side - 1 || bb[3] == side - 1,
            (i as f64 - half as f64) * step,
            (j as f64 - half as f64) * step
        );
    }
    let (_, _, s) = comps[0];
    let (x, z) = (
        ((s % side) as f64 - half as f64) * step,
        ((s / side) as f64 - half as f64) * step,
    );
    let hydrology = world.hydrology();
    hydrology.set_threads(16);
    let started = std::time::Instant::now();
    let network = hydrology.network_at(&world, x, z).expect("land");
    println!(
        "largest: baked {:?}, {} nodes, capped {}, {:.2} s ({:.2} s wall), {} tiles, {} reaches, {} lakes",
        network.key(),
        network.stats().nodes,
        network.capped(),
        network.stats().seconds,
        started.elapsed().as_secs_f64(),
        network.stats().tiles,
        network.rivers().len(),
        network.lakes().len()
    );
    // A second part of the same mass, in another cell: the count is skipped.
    let started = std::time::Instant::now();
    let other = (0..side * side)
        .filter(|&p| label[p] == 0)
        .map(|p| {
            (
                ((p % side) as f64 - half as f64) * step,
                ((p / side) as f64 - half as f64) * step,
            )
        })
        .find(|&(px, pz)| {
            ((px - x).abs() > 300_000.0 || (pz - z).abs() > 300_000.0)
                && world.coarse_ground(px, pz).height > 50.0
        });
    if let Some((px, pz)) = other {
        let second = hydrology.network_at(&world, px, pz).expect("land");
        println!(
            "another part at ({px:.0}, {pz:.0}): {:?}, {} nodes, capped {}, {:.2} s ({:.2} s wall)",
            second.key(),
            second.stats().nodes,
            second.capped(),
            second.stats().seconds,
            started.elapsed().as_secs_f64()
        );
    }
}

#[test]
#[ignore = "a probe: run in release"]
#[allow(
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    clippy::cast_possible_wrap,
    clippy::cast_sign_loss,
    clippy::cast_lossless,
    clippy::too_many_lines,
    clippy::many_single_char_names,
    reason = "a measure's arithmetic on grid indices far under 2^31"
)]
fn probe_cap() {
    let world = world();
    let whole = world.hydrology();
    whole.set_threads(16);
    let started = std::time::Instant::now();
    let home = whole.network_at(&world, -700.0, -1_200.0).expect("land");
    println!(
        "whole: {} nodes in {:.2} s",
        home.stats().nodes,
        started.elapsed().as_secs_f64()
    );
    for cell in [128, 256] {
        let cut = Hydrology::with_cap(Seed(42), 100_000, cell);
        cut.set_threads(16);
        let started = std::time::Instant::now();
        let part = cut.network_at(&world, -700.0, -1_200.0).expect("land");
        println!(
            "cell {} km: the part under Port Serein {:?}, {} nodes, capped {}, {:.2} s ({:.2} s wall, the count included)",
            f64::from(cell) * LATTICE_M / 1000.0,
            part.key(),
            part.stats().nodes,
            part.capped(),
            part.stats().seconds,
            started.elapsed().as_secs_f64()
        );
        let (mut same, mut total) = (0usize, 0usize);
        let (mut km_same, mut km_total) = (0.0, 0.0);
        for river in home.rivers() {
            for pair in river.vertices.windows(2) {
                let v = pair[0];
                let step = ((pair[1].x - v.x).powi(2) + (pair[1].z - v.z).powi(2)).sqrt() / 1000.0;
                total += 1;
                km_total += step;
                if let Some(w) = cut.water_at(&world, v.x, v.z) {
                    let rel = (w.discharge_m3s - v.discharge_m3s).abs() / v.discharge_m3s;
                    if w.signed_distance_m.abs() < 50.0 && rel < 0.1 {
                        same += 1;
                        km_same += step;
                    }
                }
            }
        }
        println!(
            "  of the whole mass's rivers, {:.1}% of vertices ({:.1}% of length) are the same river within 10% of its discharge cut",
            same as f64 / total as f64 * 100.0,
            km_same / km_total * 100.0
        );
    }
    let status = std::fs::read_to_string("/proc/self/status").unwrap_or_default();
    for line in status.lines().filter(|l| l.starts_with("VmHWM")) {
        println!("{line}");
    }
}

#[test]
#[ignore = "a probe: run in release"]
#[allow(clippy::cast_precision_loss, reason = "a measure's counts")]
fn probe_node_cost() {
    let world = world();
    let points: Vec<(f64, f64)> = (0..200_000_u32)
        .map(|k| {
            (
                f64::from(k % 500) * 97.0 - 30_000.0,
                f64::from(k / 500) * 97.0 - 30_000.0,
            )
        })
        .filter(|&(x, z)| world.coarse_ground(x, z).height >= 0.0)
        .collect();
    for _ in 0..2 {
        let started = Instant::now();
        let mut sum = 0.0;
        for &(x, z) in &points {
            sum += world.coarse_ground(x, z).height;
        }
        let node = started.elapsed().as_secs_f64() * 1e9 / points.len() as f64;
        let started = Instant::now();
        for &(x, z) in &points {
            sum += f64::from(crate::WorldSource::sample(&world, x, z).height);
        }
        let sample = started.elapsed().as_secs_f64() * 1e9 / points.len() as f64;
        println!(
            "{} land points: a node {node:.0} ns, a sample {sample:.0} ns ({sum:.0})",
            points.len()
        );
    }
}

#[test]
#[ignore = "a probe: run in release"]
fn probe_version_cost() {
    let registries =
        strata_registry::load_from_dir(concat!(env!("CARGO_MANIFEST_DIR"), "/../../assets"))
            .expect("the shipped assets load");
    let params = crate::WorldParams::default();
    for _ in 0..3 {
        let started = Instant::now();
        let v = crate::world_version(&params, &registries);
        let all = started.elapsed().as_secs_f64();
        let canary = ProceduralWorld::new(Seed(42), &registries);
        let started = Instant::now();
        let _ = canary_hashes(&canary, &canary_window(&canary));
        println!(
            "world_version {v:#018x} {:.1} ms, of which the canaries {:.1} ms",
            all * 1e3,
            started.elapsed().as_secs_f64() * 1e3
        );
    }
}
