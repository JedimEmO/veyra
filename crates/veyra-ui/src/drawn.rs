//! What a weapon screen reads of the combatant, as a value its inputs can
//! compare (kept apart from `strata_ui::sim_screen`, which knows no weapons).

/// Which weapons the local player's combatant holds and in which hand: all
/// a weapon screen reads of the combatant that its sheet and hands do not
/// already announce. The combatant itself is written most ticks (stamina,
/// age, the guard), so its change mark says nothing about a weapon.
pub type Drawn = (
    veyra_sim::combat::Weapon,
    usize,
    Option<veyra_sim::combat::Weapon>,
);

/// [`Drawn`] of `actor`.
#[must_use]
pub fn drawn(actor: &veyra_sim::combat::Combatant) -> Drawn {
    (actor.weapon, actor.hand(), actor.holstered())
}
