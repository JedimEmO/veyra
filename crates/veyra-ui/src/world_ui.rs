//! The one world-anchored UI rule that needs the combat rules: whether the
//! local player is down. The rest (`ui_point`, `ui_scale`, `world_ui_shown`)
//! is `strata_ui::world_ui`.

use veyra_sim::combat::{Action, Combatant};

/// Whether the local player is down.
#[must_use]
pub fn is_down(actor: Option<&Combatant>) -> bool {
    actor.is_some_and(|actor| actor.action == Action::Dead)
}
