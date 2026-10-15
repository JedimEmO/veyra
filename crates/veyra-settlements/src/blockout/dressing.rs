//! Street detail follows shops, courts and usable lane edges. Assemblies
//! stay shared, and colour comes from the same finite palette as the town.
use veyra_buildings::port::Fitting;

use super::{
    City, Dice, Ground, Kit, PlannedLight, PlannedPlant, TreeKind, owned, place_kit, plant_at,
    reserved,
};

pub(super) fn dress(city: &mut City, ground: &Ground<'_>) {
    facades(city);
    exchange(city, ground);
    courts(city);
    banks(city, ground);
}

/// Mount in a facade's frame, keeping the town's fronts pointing +Z.
fn mount(
    city: &mut City,
    fitting: Fitting,
    origin: [f64; 3],
    yaw: f64,
    local: [f64; 3],
    cold: bool,
) {
    let (sin, cos) = yaw.sin_cos();
    let [x, y, z] = local;
    place_kit(
        city,
        "port-fittings",
        Kit::Dressing(fitting),
        [origin[0] + cos * x + sin * z, origin[2] - sin * x + cos * z],
        origin[1] + y,
        yaw,
        0.5,
        if cold {
            vec![("lantern", "serein:rail-lamp")]
        } else {
            Vec::new()
        },
    );
}

#[allow(clippy::too_many_lines, reason = "one fitting rule per facade type")]
fn facades(city: &mut City) {
    let mut dice = Dice(0x706f_7274_5f66_6974);
    let kits: Vec<_> = city
        .kits
        .values()
        .flatten()
        .map(|k| (k.kit, k.at, k.yaw))
        .collect();
    for (kit, at, yaw) in kits {
        match kit {
            Kit::Module(module) => {
                let front = module.depth * 0.5;
                if module.shop {
                    mount(
                        city,
                        Fitting::Fascia,
                        at,
                        yaw,
                        [0.0, 2.85, front - 0.02],
                        false,
                    );
                }
                mount(
                    city,
                    Fitting::Services,
                    at,
                    yaw + std::f64::consts::PI,
                    [module.width * 0.5 - 1.6, 0.0, front - 0.02],
                    false,
                );
                if dice.unit() < 0.4 {
                    mount(
                        city,
                        Fitting::Corner,
                        at,
                        yaw,
                        [-module.width * 0.5 + 0.9, 0.35, front - 0.02],
                        false,
                    );
                }
            }
            Kit::House(house) if house.workshop || dice.unit() < 0.18 => {
                let shift = house.shed.map_or(0.0, |right| {
                    if right {
                        -super::types::house::SHED * 0.5
                    } else {
                        super::types::house::SHED * 0.5
                    }
                });
                mount(
                    city,
                    Fitting::Services,
                    at,
                    yaw + std::f64::consts::PI,
                    [
                        -shift + house.width * 0.5 - 1.0,
                        0.0,
                        house.depth * 0.5 - 0.02,
                    ],
                    false,
                );
                if house.workshop {
                    mount(
                        city,
                        Fitting::Fascia,
                        at,
                        yaw,
                        [shift, 2.9, house.depth * 0.5 - 0.02],
                        false,
                    );
                } else if house.width >= 8.0 {
                    let module = super::types::house::Module {
                        width: house.width,
                        depth: house.depth,
                        storeys: house.lower,
                        style: house.style,
                        door: true,
                    };
                    if let Some(window) = module
                        .bays()
                        .into_iter()
                        .find(|x| (*x - module.door_at()).abs() > 0.1)
                    {
                        mount(
                            city,
                            Fitting::Window,
                            at,
                            yaw,
                            [shift + window, 1.05, house.depth * 0.5 + 0.02],
                            false,
                        );
                    }
                }
            }
            Kit::Stall(stall) => {
                mount(
                    city,
                    Fitting::MarketFascia,
                    at,
                    yaw,
                    [0.0, 1.9 + stall.lift, stall.depth * 0.5 + 0.3],
                    false,
                );
            }
            Kit::Furniture(super::types::Furniture::Kiosk) => {
                mount(city, Fitting::WaterSign, at, yaw, [-0.7, 2.5, 1.16], false);
            }
            _ => {}
        }
    }
}

/// Furnish the arrival foreground as well as the market aisles. Coordinates
/// are relative to the Exchange, with the spawn at (+20, +42).
#[allow(
    clippy::too_many_lines,
    reason = "one square, laid out around its arrival corridor"
)]
fn exchange(city: &mut City, ground: &Ground<'_>) {
    let ([cx, cz], _, level) = ground.exchange;
    let at = |x, z| [cx + x, level, cz + z];
    let mut dice = Dice(0x7371_7561_7265_6564);
    // The camera arrives between the canteen and clinic. Dark setts give
    // their frontages an edge without covering the central stair approach.
    for (start, end, z) in [(-30, 6, 31.0), (30, 48, 30.0), (-44, -32, 20.0)] {
        for x in (start..end).step_by(6) {
            let x = f64::from(x) + 3.0;
            place_kit(
                city,
                "exchange-dressing",
                Kit::Dressing(Fitting::Paving),
                [cx + x, cz + z],
                level,
                0.0,
                3.1,
                vec![("trim", "serein:terrace-masonry")],
            );
            mount(city, Fitting::Kerb, at(x, z + 1.3), 0.0, [0.0; 3], false);
        }
    }
    for (x, z, yaw) in [
        (12.0, 32.0, 0.2),
        (29.0, 28.0, -0.2),
        (-28.0, 24.0, 0.0),
        (47.0, 22.0, -1.0),
    ] {
        place_kit(
            city,
            "exchange-dressing",
            Kit::Furniture(super::types::Furniture::Lamp),
            [cx + x, cz + z],
            level,
            yaw,
            0.6,
            Vec::new(),
        );
        super::lamp_light(city, [cx + x, cz + z], level, yaw);
    }
    // Warm overhead spans frame the nearest market aisle in the spawn view.
    for (x, z, yaw) in [
        (-27.0, 3.0, 0.13),
        (10.0, 8.0, -0.12),
        (-20.0, 22.0, 0.0),
        (-5.0, 22.0, 0.0),
        (20.0, 23.0, 0.0),
    ] {
        mount(city, Fitting::Lanterns, at(x, z), yaw, [0.0; 3], false);
        city.lights.push(PlannedLight {
            position: [cx + x, level + 3.5, cz + z],
            color: [1.0, 0.66, 0.34],
            lumens: 3600.0,
            range: 16.0,
        });
    }
    // Pair of beds and seats flanking the arrival, at eye level rather than
    // in the distant courts. The eight-metre central corridor stays clear.
    for (x, z, yaw) in [(10.0, 28.0, -0.25), (31.0, 27.0, 0.2)] {
        mount(city, Fitting::Bed, at(x, z), yaw, [0.0; 3], false);
        place_kit(
            city,
            "exchange-dressing",
            Kit::Furniture(super::types::Furniture::Bench),
            [cx + if x < 20.0 { 12.0 } else { 28.0 }, cz + 35.0],
            level,
            std::f64::consts::PI,
            1.0,
            Vec::new(),
        );
        // Actual flora catches wind, unlike the bed's low permanent hedge.
        super::plant_at(
            city,
            &mut dice,
            [cx + x, cz + z],
            level + 0.35,
            TreeKind::Shrub,
        );
    }
    for (x, z) in [
        (14.0, 37.0),
        (26.0, 37.0),
        (14.0, 30.0),
        (26.0, 30.0),
        (-22.0, 33.5),
        (-10.0, 33.5),
    ] {
        mount(city, Fitting::Bollard, at(x, z), 0.0, [0.0; 3], false);
    }
    mount(
        city,
        Fitting::Terminal,
        at(14.0, 35.0),
        -0.25,
        [0.0; 3],
        true,
    );
    mount(
        city,
        Fitting::Shelter,
        at(43.0, 16.0),
        -std::f64::consts::FRAC_PI_2,
        [0.0; 3],
        true,
    );
    mount(
        city,
        Fitting::Terminal,
        at(36.0, 21.0),
        -0.3,
        [0.0; 3],
        true,
    );
    for (x, z) in [(-12.0, 29.0), (-31.0, 25.0), (39.0, 29.0)] {
        place_kit(
            city,
            "exchange-dressing",
            Kit::Furniture(super::types::Furniture::Crates),
            [cx + x, cz + z],
            level,
            0.15,
            0.7,
            Vec::new(),
        );
    }
    // The clinic presents its side to the spawn. Give that visible face a
    // service sign and downlight, instead of relying on its uphill doorway.
    mount(
        city,
        Fitting::WaterSign,
        at(35.0, 40.0),
        -std::f64::consts::FRAC_PI_2,
        [0.0, 2.5, 0.0],
        true,
    );
    mount(
        city,
        Fitting::Fascia,
        at(35.0, 37.0),
        -std::f64::consts::FRAC_PI_2,
        [0.0, 3.0, 0.0],
        true,
    );
    city.lights.push(PlannedLight {
        position: [cx + 33.0, level + 3.1, cz + 37.0],
        color: [0.66, 0.82, 1.0],
        lumens: 2800.0,
        range: 13.0,
    });
}

/// Courts have a reason to exist: a shade tree, seats, a water marker or
/// a public terminal. The middle and the cross-lane remain open.
fn courts(city: &mut City) {
    let mut dice = Dice(0x636f_7572_745f_7472);
    let courts: Vec<_> = city
        .courts
        .iter()
        .map(|(l, y)| ([l.centre[0], *y, l.centre[1]], l.yaw))
        .collect();
    for (index, (at, yaw)) in courts.into_iter().enumerate() {
        let (sin, cos) = yaw.sin_cos();
        for side in [-5.2, 5.2] {
            mount(
                city,
                Fitting::Kerb,
                [at[0] + cos * side, at[1], at[2] - sin * side],
                yaw + std::f64::consts::FRAC_PI_2,
                [0.0; 3],
                false,
            );
        }
        plant_at(
            city,
            &mut dice,
            [at[0] - cos * 3.5, at[2] + sin * 3.5],
            at[1],
            TreeKind::Shade,
        );
        if index % 3 == 0 {
            mount(city, Fitting::Terminal, at, yaw, [4.2, 0.0, -2.0], false);
        }
        {
            super::place_kit(
                city,
                "street",
                Kit::Furniture(super::types::Furniture::Bench),
                [at[0] - cos * 3.7 + sin * 2.0, at[2] + sin * 3.7 + cos * 2.0],
                at[1],
                yaw,
                1.0,
                Vec::new(),
            );
        }
    }
}

/// Shoulder-high clumps at every clear bank foot and crest, plus larger
/// rocks on unbuilt slopes, soften the bench ends and
/// gives the gullies a scree toe. It uses the existing flora/rock library.
#[allow(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "unit turns, scales and sixteen variants"
)]
fn banks(city: &mut City, ground: &Ground<'_>) {
    let mut dice = Dice(0x7363_7265_655f_746f);
    for terrace in super::terraces_between(ground, -315.0, 315.0) {
        let mut along = 1.5;
        while along < terrace.length() - 1.0 {
            let ([x, z], depth) = terrace.at(along);
            for bank_z in [z - 0.6, z + depth + 0.6] {
                let footprint = [x - 0.8, bank_z - 0.8, x + 0.8, bank_z + 0.8];
                if owned(ground, footprint)
                    && !reserved(ground, footprint)
                    && city.clear(footprint, 0.8)
                {
                    city.plants.push(PlannedPlant {
                        species: if dice.unit() < 0.22 {
                            "strata:granite_stone"
                        } else {
                            "strata:scrub"
                        },
                        position: [x, (ground.height)(x, bank_z) - 0.12, bank_z],
                        yaw: dice.unit() as f32,
                        scale: dice.range(1.4, 2.15) as f32,
                        variant: (dice.unit() * 16.0) as u8,
                    });
                }
            }
            along += dice.range(2.7, 4.3);
        }
    }
    for ix in -43..=43 {
        for iz in -64..=26 {
            let (x, z) = (
                f64::from(ix) * 7.0 + dice.range(-2.0, 2.0),
                f64::from(iz) * 7.0 + dice.range(-2.0, 2.0),
            );
            let level = (ground.height)(x, z);
            let dx = ((ground.height)(x + 1.0, z) - (ground.height)(x - 1.0, z)) * 0.5;
            let dz = ((ground.height)(x, z + 1.0) - (ground.height)(x, z - 1.0)) * 0.5;
            let grade = dx * dx + dz * dz;
            let footprint = [x - 1.6, z - 1.6, x + 1.6, z + 1.6];
            if level < ground.quay + 1.0
                || !(0.003..4.0).contains(&grade)
                || !owned(ground, footprint)
                || reserved(ground, footprint)
                || !city.clear(footprint, 1.0)
            {
                continue;
            }
            let species = if dice.unit() < 0.28 {
                "strata:granite_stone"
            } else {
                "strata:scrub"
            };
            city.plants.push(PlannedPlant {
                species,
                position: [x, level - 0.10, z],
                yaw: dice.unit() as f32,
                scale: dice.range(1.35, 2.1) as f32,
                variant: (dice.unit() * 16.0) as u8,
            });
        }
    }
}
