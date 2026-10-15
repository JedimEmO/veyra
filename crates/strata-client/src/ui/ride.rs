//! The bike's key and its refusals (ticket 0009, step 2): V calls the bike
//! or gets off it, and a call the sim refuses says why as a toast.
//!
//! The client decides nothing here: V puts a [`RideRequest`] in the sim's
//! inbox, the sim answers with a [`RideAnswer`], and the words of a refusal
//! are [`super::strings`]'. What is asked is a function of whether the
//! player is on the bike or calling it, which is the sim's component, not a
//! flag of ours.

use std::time::Duration;

use bevy::ecs::prelude::*;
use bevy::input::ButtonInput;
use bevy::input::keyboard::KeyCode;
use slotted::menu::{ToastLevel, ToastSpec, toast};
use strata_sim::ride::{
    Calling, CancelWhy, RideAnswer, RideInbox, RideNotice, RideRefusal, RideRequest, Riding,
    ThrownWhy,
};
use strata_sim::{LocalPlayer, Registries};

/// The key that calls the bike and gets off it.
pub const RIDE_KEY: KeyCode = KeyCode::KeyV;

/// What V asks for: the dismount while mounted or calling, the call
/// otherwise.
#[must_use]
pub const fn request_for(riding: bool, calling: bool) -> RideRequest {
    if riding || calling {
        RideRequest::Dismount
    } else {
        RideRequest::Call
    }
}

/// The words of a refusal.
#[must_use]
pub fn refusal_words(refusal: RideRefusal) -> &'static str {
    let key = match refusal {
        RideRefusal::NoBike => "strata.ride.refused.no_bike",
        RideRefusal::InWater => "strata.ride.refused.in_water",
        RideRefusal::Hostile => "strata.ride.refused.hostile",
        RideRefusal::Busy => "strata.ride.refused.busy",
        RideRefusal::AlreadyRiding => "strata.ride.refused.already",
        RideRefusal::NotABike => "strata.ride.refused.not_a_bike",
        RideRefusal::NotHeld => "strata.ride.refused.not_held",
        RideRefusal::SaddleLoaded => "strata.ride.refused.saddle_loaded",
        RideRefusal::NoRoom => "strata.ride.refused.no_room",
    };
    super::strings::english(key).unwrap_or(key)
}

/// The words of a refusal of taking a bike as the ride or putting it away,
/// where a rider on the bike is told to get off and not that it is here.
#[must_use]
pub fn stable_words(refusal: RideRefusal) -> &'static str {
    match refusal {
        RideRefusal::AlreadyRiding => super::strings::english("strata.ride.refused.mounted")
            .unwrap_or("strata.ride.refused.mounted"),
        other => refusal_words(other),
    }
}

/// `Update`, with the game's input: V is the request.
pub fn ride_key(
    keys: Res<ButtonInput<KeyCode>>,
    mut inbox: ResMut<RideInbox>,
    players: Query<(Has<Riding>, Has<Calling>), With<LocalPlayer>>,
) {
    if !keys.just_pressed(RIDE_KEY) {
        return;
    }
    let Ok((riding, calling)) = players.single() else {
        return;
    };
    inbox.0.push(request_for(riding, calling));
}

/// `Update`: a refused call is a toast with its reason.
pub fn refusal_toasts(mut answers: MessageReader<RideAnswer>, mut commands: Commands) {
    for answer in answers.read() {
        if let Err(refusal) = answer.result {
            toast(
                &mut commands,
                ToastSpec::new(refusal_words(refusal))
                    .level(ToastLevel::Warning)
                    .duration(Duration::from_millis(1600)),
            );
        }
    }
}

/// The words of what happened to the ride, for a notice that is said:
/// a throw (with the bike's new condition after a crash) and a call that
/// ended without a rider. A mount and a dismount are said by the screen
/// itself. `full` is the wear rules' full condition, which a crash line
/// reads the bike's against.
#[must_use]
pub fn notice_words(notice: &RideNotice, full: u16) -> Option<String> {
    use super::crafting::{fill, words};
    match notice {
        RideNotice::Thrown {
            why,
            condition,
            speed: _,
        } => Some(match (why, condition) {
            (ThrownWhy::Hit, _) => words("strata.ride.thrown.hit").to_owned(),
            (ThrownWhy::Water, _) => words("strata.ride.thrown.water").to_owned(),
            (ThrownWhy::Crash, None) => words("strata.ride.thrown.crash").to_owned(),
            (ThrownWhy::Crash, Some(condition)) => fill(
                &fill(
                    words("strata.ride.thrown.crash.worn"),
                    "condition",
                    &condition.to_string(),
                ),
                "full",
                &full.to_string(),
            ),
        }),
        RideNotice::Cancelled { why } => Some(match why {
            CancelWhy::Moved => words("strata.ride.cancelled.moved").to_owned(),
            CancelWhy::Refused(refusal) => fill(
                words("strata.ride.cancelled"),
                "why",
                refusal_words(*refusal),
            ),
        }),
        RideNotice::Mounted | RideNotice::Dismounted => None,
    }
}

/// `Update`: a throw and a cancelled call are toasts.
pub fn notice_toasts(
    mut notices: MessageReader<RideNotice>,
    registries: Option<Res<Registries>>,
    mut commands: Commands,
) {
    let full = registries.map_or(strata_registry::item::QUALITY_MAX, |registries| {
        registries.0.items.wear().full
    });
    for notice in notices.read() {
        if let Some(said) = notice_words(notice, full) {
            toast(
                &mut commands,
                ToastSpec::new(said)
                    .level(ToastLevel::Warning)
                    .duration(Duration::from_millis(2400)),
            );
        }
    }
}

/// Registers the key and the toasts. Only called once the UI is wired.
pub(crate) fn register(app: &mut bevy::app::App) {
    app.add_systems(
        bevy::app::Update,
        (
            ride_key.run_if(super::game_input_allowed),
            refusal_toasts,
            notice_toasts,
        ),
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use bevy::ecs::system::RunSystemOnce;

    fn world() -> World {
        let mut world = World::new();
        world.init_resource::<ButtonInput<KeyCode>>();
        world.init_resource::<RideInbox>();
        world
    }

    fn press(world: &mut World) {
        let mut keys = world.resource_mut::<ButtonInput<KeyCode>>();
        keys.reset_all();
        keys.press(RIDE_KEY);
    }

    #[test]
    fn v_calls_and_a_second_v_gets_off() {
        let mut world = world();
        let player = world.spawn(LocalPlayer).id();
        press(&mut world);
        world.run_system_once(ride_key).expect("runs");
        assert_eq!(world.resource::<RideInbox>().0, vec![RideRequest::Call]);
        // The sim mounts the rider; the next V is the dismount.
        world.resource_mut::<RideInbox>().0.clear();
        let registries = super::super::testing::shipped();
        let id = "veyra:vehicle.courier_bike".parse().expect("an id");
        let def = registries.vehicles.get(&id).expect("the shipped bike");
        world.entity_mut(player).insert(Riding::new(def, 0.0));
        press(&mut world);
        world.run_system_once(ride_key).expect("runs");
        assert_eq!(world.resource::<RideInbox>().0, vec![RideRequest::Dismount]);
        // A call under way is cancelled by V too.
        world.resource_mut::<RideInbox>().0.clear();
        world.entity_mut(player).remove::<Riding>();
        world.entity_mut(player).insert(Calling { left: 1.0 });
        press(&mut world);
        world.run_system_once(ride_key).expect("runs");
        assert_eq!(world.resource::<RideInbox>().0, vec![RideRequest::Dismount]);
    }

    #[test]
    fn a_throw_and_a_cancelled_call_are_worded() {
        let thrown = |why, condition| RideNotice::Thrown {
            why,
            speed: 20.0,
            condition,
        };
        assert_eq!(
            notice_words(&thrown(ThrownWhy::Crash, Some(750)), 1000).as_deref(),
            Some("You crashed. The bike is at 750 of 1000")
        );
        assert_eq!(
            notice_words(&thrown(ThrownWhy::Crash, None), 1000).as_deref(),
            Some("You crashed")
        );
        assert_eq!(
            notice_words(&thrown(ThrownWhy::Crash, Some(750)), 800).as_deref(),
            Some("You crashed. The bike is at 750 of 800"),
            "the full figure is the wear rules', not a constant"
        );
        assert_eq!(
            notice_words(&thrown(ThrownWhy::Hit, None), 1000).as_deref(),
            Some("A blow knocked you off the bike")
        );
        assert_eq!(
            notice_words(&thrown(ThrownWhy::Water, None), 1000).as_deref(),
            Some("The bike stalled in deep water")
        );
        assert_eq!(
            notice_words(
                &RideNotice::Cancelled {
                    why: CancelWhy::Moved
                },
                1000
            )
            .as_deref(),
            Some("You moved. The call is off")
        );
        assert_eq!(
            notice_words(
                &RideNotice::Cancelled {
                    why: CancelWhy::Refused(RideRefusal::Hostile)
                },
                1000
            )
            .as_deref(),
            Some("The call is off. Not with a hostile this near")
        );
        assert_eq!(notice_words(&RideNotice::Mounted, 1000), None);
        assert_eq!(notice_words(&RideNotice::Dismounted, 1000), None);
    }

    #[test]
    fn every_refusal_has_its_own_words() {
        let all = [
            RideRefusal::NoBike,
            RideRefusal::InWater,
            RideRefusal::Hostile,
            RideRefusal::Busy,
            RideRefusal::AlreadyRiding,
        ];
        let words: std::collections::BTreeSet<_> = all.into_iter().map(refusal_words).collect();
        assert_eq!(words.len(), all.len());
        assert!(words.iter().all(|w| !w.starts_with("strata.")), "{words:?}");
        assert_eq!(
            refusal_words(RideRefusal::Hostile),
            "Not with a hostile this near"
        );
    }

    #[test]
    fn a_refusal_is_toasted_and_a_mount_is_not() {
        use slotted::menu::Toast;
        let mut world = world();
        world.init_resource::<bevy::ecs::message::Messages<RideAnswer>>();
        world.write_message(RideAnswer {
            result: Ok(strata_sim::ride::RideDone::Mounted),
        });
        world.run_system_once(refusal_toasts).expect("runs");
        assert_eq!(world.query::<&Toast>().iter(&world).count(), 0);
        world.write_message(RideAnswer {
            result: Err(RideRefusal::Hostile),
        });
        world.run_system_once(refusal_toasts).expect("runs");
        assert_eq!(world.query::<&Toast>().iter(&world).count(), 1);
    }
}
