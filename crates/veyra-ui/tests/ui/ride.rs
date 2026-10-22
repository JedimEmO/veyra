//! The bike's readout (ticket 0009, step 2): the speed by the vitals only
//! while the player rides, and the call's words while the bike is coming.

use veyra_sim::ride::{Calling, Riding};
use veyra_view::combat_hud::ids;

use crate::common::*;

impl Day {
    /// The shipped bike's rider state at `speed` metres a second.
    fn riding_at(&self, speed: f32) -> Riding {
        let id = "veyra:vehicle.courier_bike".parse().unwrap();
        let def = self.registries.vehicles.get(&id).unwrap();
        let mut riding = Riding::new(def, 0.0);
        riding.speed = speed;
        riding
    }

    /// Lets the HUD follow the world.
    fn follow(&mut self) {
        self.h.step(2);
        self.h.settle();
    }
}

#[test]
fn the_speed_reads_only_while_riding() {
    let mut day = first_day("ride-readout", 1600.0, 900.0);
    assert!(!day.laid_out(find(&day.h, ids::RIDE)), "no readout on foot");
    let riding = day.riding_at(20.0);
    let player = day.player;
    day.h.world_mut().entity_mut(player).insert(riding);
    day.follow();
    assert!(day.laid_out(find(&day.h, ids::RIDE)));
    assert_eq!(day.says(ids::SPEED), "72 km/h");
    day.h.world_mut().get_mut::<Riding>(player).unwrap().speed = -4.0;
    day.follow();
    assert_eq!(day.says(ids::SPEED), "14 km/h", "reversing reads its size");
    day.h.world_mut().entity_mut(player).remove::<Riding>();
    day.follow();
    assert!(!day.laid_out(find(&day.h, ids::RIDE)), "gone with the bike");
}

#[test]
fn a_call_under_way_says_so_in_the_same_place() {
    let mut day = first_day("ride-calling", 1600.0, 900.0);
    let player = day.player;
    day.h
        .world_mut()
        .entity_mut(player)
        .insert(Calling { left: 1.0 });
    day.follow();
    assert!(day.laid_out(find(&day.h, ids::RIDE)));
    assert_eq!(day.says(ids::SPEED), "Calling the bike");
}
