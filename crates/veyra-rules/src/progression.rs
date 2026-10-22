//! The character sheet, the rules of learning and forgetting, the merge of
//! learned boxes into an actor's combat numbers, and the XP ledger
//! (progression design, sections 2 to 4 and 6).
//!
//! The sheet knows box ids and XP counts and nothing else; every rule
//! reads the [`SkillGraph`] it is handed, so the trees can be rewritten
//! without touching this crate. Whether a teacher is within reach is the
//! world's to decide and the sim's to be told: [`CharacterSheet::learn`]
//! takes it as a bool. The same for being in combat when forgetting.
//!
//! The merge, [`apply`], is a pure function of the installed
//! [`CombatTables`], the graph and a sheet: the effective rows an actor
//! reads instead of the global ones. A [`Combatant`](crate::combat::Combatant) with no sheet reads
//! the global tables live, which is what the balance harness and every
//! existing test rely on; one that has been [`Combatant::fit`](crate::combat::Combatant::fit) reads its
//! own. Nothing in here is replicated: combat is local-only play in this
//! slice, and the protocol does not carry the sheet (`docs/design/progression.md`).

use std::collections::{BTreeMap, BTreeSet};

use std::sync::Arc;

use bevy_ecs::prelude::*;

use serde::{Deserialize, Serialize};

use strata_registry::Namespaced;

use strata_registry::progression::{Effect, Op, SheetDef, SkillGraph, XpKind};

use crate::ability::AbilityRow;

use crate::combat::{Action, ActionSpec, Weapon, WeaponSpec};

use crate::combat_tables::{ActorTable, CombatTables, GuardRow};

use crate::factions::FactionId;

/// What a character has learned and earned. Points spent and free are
/// derived from the graph, never stored, so a changed cost in the file
/// is a changed sheet with no migration.
#[derive(Component, Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[require(crate::ability::Bar)]
pub struct CharacterSheet {
    /// Box ids, as the graph spells them.
    pub learned: BTreeSet<String>,
    /// XP earned, by kind. Never spent, never lost.
    pub xp: BTreeMap<XpKind, u32>,
}

/// Why a box could not be learned.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum LearnRefused {
    /// No such box in the graph.
    #[error("`{0}` is no box")]
    UnknownBox(String),
    /// Already on the sheet.
    #[error("`{0}` is already learned")]
    AlreadyLearned(String),
    /// Nobody within reach to teach it.
    #[error("no teacher within reach")]
    NoTeacher,
    /// A box it needs is not learned.
    #[error("needs `{0}` first")]
    MissingPrerequisite(String),
    /// Not enough XP of the tree's kind.
    #[error("needs {need} {kind} XP, has {have}")]
    NotEnoughXp {
        /// The kind.
        kind: XpKind,
        /// What the sheet has.
        have: u32,
        /// What the box needs.
        need: u32,
    },
    /// Not enough free points.
    #[error("costs {cost} points, {free} free")]
    NotEnoughPoints {
        /// The box's cost.
        cost: u16,
        /// What is left of the budget.
        free: u32,
    },
}

/// Why a box could not be forgotten.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ForgetRefused {
    /// Not on the sheet.
    #[error("`{0}` is not learned")]
    NotLearned(String),
    /// A learned box needs it.
    #[error("`{0}` still needs it")]
    Dependent(String),
    /// Not while fighting.
    #[error("not in combat")]
    InCombat,
}

impl CharacterSheet {
    /// A sheet as a file authored it: the boxes and the XP, unchecked
    /// here because the loader checked it against the graph.
    #[must_use]
    pub fn authored(def: &SheetDef) -> Self {
        Self {
            learned: def.learned.iter().cloned().collect(),
            xp: def.xp.clone(),
        }
    }

    /// XP of `kind`.
    #[must_use]
    pub fn xp(&self, kind: XpKind) -> u32 {
        self.xp.get(&kind).copied().unwrap_or(0)
    }

    /// Earn `amount` of `kind`; saturating, never lost.
    pub fn credit(&mut self, kind: XpKind, amount: u32) {
        let entry = self.xp.entry(kind).or_insert(0);
        *entry = entry.saturating_add(amount);
    }

    /// Whether `id` is learned.
    #[must_use]
    pub fn has(&self, id: &str) -> bool {
        self.learned.contains(id)
    }

    /// The sum of the learned boxes' costs. A learned id the graph no
    /// longer has costs nothing.
    #[must_use]
    pub fn points_spent(&self, graph: &SkillGraph) -> u32 {
        self.learned
            .iter()
            .filter_map(|id| graph.find(id))
            .map(|b| u32::from(b.cost))
            .sum()
    }

    /// What is left of the budget.
    #[must_use]
    pub fn points_free(&self, graph: &SkillGraph) -> u32 {
        graph.budget.saturating_sub(self.points_spent(graph))
    }

    /// Whether `id` could be learned now, and why not if not: the box's
    /// prerequisites, the XP of its tree's kind, the free points and a
    /// teacher within reach, in that order. A UI greys a box by this.
    ///
    /// # Errors
    /// The first rule that refuses.
    pub fn can_learn(
        &self,
        graph: &SkillGraph,
        id: &str,
        teacher: bool,
    ) -> Result<(), LearnRefused> {
        let Some((tree, b)) = graph.find_with_tree(id) else {
            return Err(LearnRefused::UnknownBox(id.to_owned()));
        };
        if self.has(id) {
            return Err(LearnRefused::AlreadyLearned(id.to_owned()));
        }
        if let Some(missing) = graph.prerequisites(id).into_iter().find(|p| !self.has(p)) {
            return Err(LearnRefused::MissingPrerequisite(missing.to_owned()));
        }
        let kind = tree.xp;
        let have = self.xp(kind);
        if have < b.xp {
            return Err(LearnRefused::NotEnoughXp {
                kind,
                have,
                need: b.xp,
            });
        }
        let free = self.points_free(graph);
        if u32::from(b.cost) > free {
            return Err(LearnRefused::NotEnoughPoints { cost: b.cost, free });
        }
        if !teacher {
            return Err(LearnRefused::NoTeacher);
        }
        Ok(())
    }

    /// Learn `id`, if [`CharacterSheet::can_learn`] allows.
    ///
    /// # Errors
    /// See [`CharacterSheet::can_learn`].
    pub fn learn(
        &mut self,
        graph: &SkillGraph,
        id: &str,
        teacher: bool,
    ) -> Result<(), LearnRefused> {
        self.can_learn(graph, id, teacher)?;
        self.learned.insert(id.to_owned());
        Ok(())
    }

    /// Forget `id`: its points return, its XP stays. Refused while any
    /// learned box needs it, and while `in_combat`, which the caller
    /// reads off the actor ([`Combatant::in_combat`](crate::combat::Combatant::in_combat)). Returns the points
    /// returned.
    ///
    /// # Errors
    /// The first rule that refuses.
    pub fn forget(
        &mut self,
        graph: &SkillGraph,
        id: &str,
        in_combat: bool,
    ) -> Result<u16, ForgetRefused> {
        if !self.has(id) {
            return Err(ForgetRefused::NotLearned(id.to_owned()));
        }
        if in_combat {
            return Err(ForgetRefused::InCombat);
        }
        if let Some(dependent) = graph.dependents(id).into_iter().find(|d| self.has(d)) {
            return Err(ForgetRefused::Dependent(dependent.to_owned()));
        }
        self.learned.remove(id);
        Ok(graph.find(id).map_or(0, |b| b.cost))
    }
}

/// An actor's own rows: the installed tables with its learned boxes'
/// effects merged in, and what it may do. Built by [`apply`], read by a
/// fitted [`Combatant`](crate::combat::Combatant).
#[derive(Debug, Clone, PartialEq)]
pub struct EffectiveTables {
    /// One row per action.
    pub actions: BTreeMap<Action, ActionSpec>,
    /// One row per weapon.
    pub weapons: BTreeMap<Weapon, WeaponSpec>,
    /// The pools, rates and windows.
    pub actor: ActorTable,
    /// The actions this actor may begin: every action no box in the graph
    /// gates, plus those its boxes unlocked.
    pub unlocked: BTreeSet<Action>,
    /// The weapons this actor may fire and draw, by the same rule.
    pub certified: BTreeSet<Weapon>,
    /// Every ability's row with the learned boxes' `Ability` effects
    /// applied, by id.
    pub abilities: BTreeMap<String, AbilityRow>,
    /// Every melee class's guard row with the learned boxes' `Guard`
    /// effects applied, so the guard is read from one place whatever
    /// trained it.
    pub guards: BTreeMap<Weapon, GuardRow>,
}

impl EffectiveTables {
    /// The row for `action`; every variant is present.
    #[must_use]
    pub fn spec(&self, action: Action) -> ActionSpec {
        self.actions
            .get(&action)
            .copied()
            .unwrap_or(ActionSpec::NONE)
    }

    /// The row for `weapon`; a melee class reads [`WeaponSpec::MELEE`].
    #[must_use]
    pub fn weapon_spec(&self, weapon: Weapon) -> WeaponSpec {
        self.weapons
            .get(&weapon)
            .copied()
            .unwrap_or(WeaponSpec::MELEE)
    }
}

/// Every effect of every box the sheet has learned, in graph order. The one
/// reader of a sheet's effects: the combat merge ([`apply`]), the crafting
/// points, the survey's skill and the ride's dials all go through it, so a
/// box counts the same everywhere. A box on the sheet the graph has not got
/// is ignored.
pub fn learned_effects<'a>(
    graph: &'a SkillGraph,
    sheet: &'a CharacterSheet,
) -> impl Iterator<Item = &'a Effect> {
    graph
        .boxes()
        .filter(|b| sheet.has(&b.id))
        .flat_map(|b| b.effects.iter())
}

/// `base` after the ops of one field, in the order Set, then Add, then Mul
/// (the last Set in graph order wins), so the result does not depend on the
/// order the boxes were learned in. `ops` is the field's effects as
/// `(op, value)`.
#[must_use]
pub fn fold_ops(base: f32, ops: impl IntoIterator<Item = (Op, f32)>) -> f32 {
    let ops: Vec<(Op, f32)> = ops.into_iter().collect();
    ops.iter()
        .filter(|(op, _)| *op == Op::Set)
        .chain(ops.iter().filter(|(op, _)| *op == Op::Add))
        .chain(ops.iter().filter(|(op, _)| *op == Op::Mul))
        .fold(base, |current, (op, value)| op.apply(current, *value))
}

/// `base` after the learned `Survey` effects on `field`.
#[must_use]
pub fn survey_field(graph: &SkillGraph, sheet: &CharacterSheet, field: &str, base: f32) -> f32 {
    fold_ops(
        base,
        learned_effects(graph, sheet).filter_map(|effect| match effect {
            Effect::Survey {
                field: f,
                op,
                value,
            } if f == field => Some((*op, *value)),
            _ => None,
        }),
    )
}

/// `base` after the learned `Ride` effects on `field`.
#[must_use]
pub fn ride_field(graph: &SkillGraph, sheet: &CharacterSheet, field: &str, base: f32) -> f32 {
    fold_ops(
        base,
        learned_effects(graph, sheet).filter_map(|effect| match effect {
            Effect::Ride {
                field: f,
                op,
                value,
            } if f == field => Some((*op, *value)),
            _ => None,
        }),
    )
}

/// `base` after the learned `Craft` effects on `field`.
#[must_use]
pub fn craft_field(graph: &SkillGraph, sheet: &CharacterSheet, field: &str, base: f32) -> f32 {
    fold_ops(
        base,
        learned_effects(graph, sheet).filter_map(|effect| match effect {
            Effect::Craft {
                field: f,
                op,
                value,
            } if f == field => Some((*op, *value)),
            _ => None,
        }),
    )
}

/// `base` after the learned `Factory` effects on `field`.
#[must_use]
pub fn factory_field(graph: &SkillGraph, sheet: &CharacterSheet, field: &str, base: f32) -> f32 {
    fold_ops(
        base,
        learned_effects(graph, sheet).filter_map(|effect| match effect {
            Effect::Factory {
                field: f,
                op,
                value,
            } if f == field => Some((*op, *value)),
            _ => None,
        }),
    )
}

/// `base` after the learned `Wear` effects on `field`.
#[must_use]
pub fn wear_field(graph: &SkillGraph, sheet: &CharacterSheet, field: &str, base: f32) -> f32 {
    fold_ops(
        base,
        learned_effects(graph, sheet).filter_map(|effect| match effect {
            Effect::Wear {
                field: f,
                op,
                value,
            } if f == field => Some((*op, *value)),
            _ => None,
        }),
    )
}

/// `base` after the learned `Station` effects on `field`.
#[must_use]
pub fn station_field(graph: &SkillGraph, sheet: &CharacterSheet, field: &str, base: f32) -> f32 {
    fold_ops(
        base,
        learned_effects(graph, sheet).filter_map(|effect| match effect {
            Effect::Station {
                field: f,
                op,
                value,
            } if f == field => Some((*op, *value)),
            _ => None,
        }),
    )
}

/// Whether the sheet has any box with an op on the `Craft`, `Factory` or
/// `Wear` field named: the test a caller makes to read its integer dial
/// untouched, not through `f32`, when nothing moves it.
#[must_use]
pub fn touches(graph: &SkillGraph, sheet: &CharacterSheet, kind: Touch, name: &str) -> bool {
    learned_effects(graph, sheet).any(|effect| match (kind, effect) {
        (Touch::Craft, Effect::Craft { field, .. })
        | (Touch::Factory, Effect::Factory { field, .. })
        | (Touch::Wear, Effect::Wear { field, .. }) => field == name,
        _ => false,
    })
}

/// Which family of field [`touches`] asks about.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Touch {
    /// `Effect::Craft`.
    Craft,
    /// `Effect::Factory`.
    Factory,
    /// `Effect::Wear`.
    Wear,
}

/// The merge: `tables` with every learned box's effects applied, in a
/// fixed order so the result does not depend on the order the boxes were
/// learned in: all Sets, then all Adds, then all Muls, each in graph
/// order. A box on the sheet the graph has not got is ignored.
#[must_use]
pub fn apply(tables: &CombatTables, graph: &SkillGraph, sheet: &CharacterSheet) -> EffectiveTables {
    let gated_actions = graph.gated_actions();
    let gated_weapons = graph.gated_weapons();
    let mut effective = EffectiveTables {
        actions: tables.actions.actions.clone(),
        weapons: tables.weapons.weapons.clone(),
        actor: tables.actor.clone(),
        unlocked: Action::ALL
            .into_iter()
            .filter(|a| !gated_actions.contains(a.name()))
            .collect(),
        certified: Weapon::ALL
            .into_iter()
            .filter(|w| !gated_weapons.contains(w.name()))
            .collect(),
        abilities: tables
            .abilities
            .abilities
            .iter()
            .map(|row| (row.id.clone(), row.clone()))
            .collect(),
        guards: tables
            .melee
            .classes
            .iter()
            .map(|(weapon, class)| (*weapon, class.guard))
            .collect(),
    };
    let learned: Vec<&Effect> = learned_effects(graph, sheet).collect();
    for pass in [Op::Set, Op::Add, Op::Mul] {
        for effect in &learned {
            match effect {
                Effect::Action {
                    action,
                    field,
                    op,
                    value,
                } if *op == pass => {
                    if let Some(row) = Action::from_name(action)
                        .and_then(|action| effective.actions.get_mut(&action))
                        && let Some(slot) = row.field_mut(field)
                    {
                        *slot = op.apply(*slot, *value);
                    }
                }
                Effect::Weapon {
                    weapon,
                    field,
                    op,
                    value,
                } if *op == pass => {
                    if let Some(row) = Weapon::from_name(weapon)
                        .and_then(|weapon| effective.weapons.get_mut(&weapon))
                    {
                        row.modify(field, *op, *value);
                    }
                }
                Effect::Actor { field, op, value } if *op == pass => {
                    if let Some(slot) = effective.actor.field_mut(field) {
                        *slot = op.apply(*slot, *value);
                    }
                }
                Effect::Ability {
                    id,
                    field,
                    op,
                    value,
                } if *op == pass => {
                    if let Some(row) = effective.abilities.get_mut(id) {
                        row.modify(field, *op, *value);
                    }
                }
                Effect::Guard { field, op, value } if *op == pass => {
                    for row in effective.guards.values_mut() {
                        if let Some(slot) = row.field_mut(field) {
                            *slot = op.apply(*slot, *value);
                        }
                    }
                }
                _ => {}
            }
        }
    }
    for effect in &learned {
        match effect {
            Effect::Unlock { action } => {
                if let Some(action) = Action::from_name(action) {
                    effective.unlocked.insert(action);
                }
            }
            Effect::Certify { weapon } => {
                if let Some(weapon) = Weapon::from_name(weapon) {
                    effective.certified.insert(weapon);
                }
            }
            _ => {}
        }
    }
    effective
}

/// XP credited to a sheet this tick, for a HUD line.
#[derive(Message, Debug, Clone, Copy, PartialEq, Eq)]
pub struct XpGained {
    /// Whose sheet.
    pub entity: Entity,
    /// What kind.
    pub kind: XpKind,
    /// How much.
    pub amount: u32,
    /// The hit killed.
    pub kill: bool,
}

/// What a thing in the world offers a character standing beside it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InteractionKind {
    /// A bench place: the household switches who is out here, and a
    /// recruit joins here. Teaches nothing.
    Terminal,
    /// A teacher of the named trees (graph ids, `blade`). A box is learned
    /// only within reach of a trainer whose trees include it.
    Trainer {
        /// Tree ids.
        trees: Vec<String>,
    },
    /// A locker: the household's store is reached here, and a stack moves
    /// between it and what the character carries. Teaches nothing.
    Locker,
    /// A workbench: a module is fitted to a weapon's mount here, and taken
    /// off again, with the household's store to hand. Teaches nothing.
    Workbench,
    /// A fabricator: a production blueprint is run here, and a run under
    /// way is read and stopped, with the household's store to hand (ticket
    /// 0007, step 5). Teaches nothing.
    Fabricator,
    /// A procurement board: a buyer's lines are read and sold to here, with
    /// the household's store to hand (ticket 0007, step 7). Teaches nothing.
    Board {
        /// The buyer whose board it is.
        buyer: Namespaced,
    },
    /// A planted field harvester: its hopper is read and emptied here
    /// (ticket 0009, step 6). Teaches nothing.
    Harvester {
        /// The machine's number in `Harvesters`.
        id: u32,
    },
    /// A guard of a concession, who sells its faction's lease (ticket 0012,
    /// step 3b). Teaches nothing.
    Guard {
        /// Whose guard it is.
        faction: FactionId,
    },
    /// What a cleared camp left, emptied into the pack with F (ticket 0012,
    /// step 4a). Teaches nothing.
    Cache {
        /// The camp's key in `Caches`.
        key: u64,
    },
    /// A person of the questline (ticket 0014), or a thing that answers like
    /// one: F opens their conversation. Teaches nothing.
    Person {
        /// The person's id, `veyra:person.maren_coyle`.
        id: Namespaced,
    },
}

/// A prop a character can use: a terminal, a trainer, a locker, a
/// workbench or a fabricator, with the reach within which the
/// prompt shows and the use is allowed. The world places these; the sim only measures distance to them.
#[derive(Component, Debug, Clone, PartialEq)]
pub struct Interaction {
    /// What it is.
    pub kind: InteractionKind,
    /// Metres from the character's feet, horizontally.
    pub radius: f32,
}

/// What the local player can use right now: the nearest [`Interaction`]
/// within its reach, the trees taught by every trainer within reach,
/// which is the `teacher` [`CharacterSheet::learn`] takes, and whether a
/// workbench is within reach, which is the fitting rule. Rebuilt every
/// tick by `find_reach`; a UI reads it for the prompt and words it
/// itself, since the sim carries what is there and not what to call it.
#[derive(Resource, Debug, Clone, Default, PartialEq)]
pub struct Reach {
    /// The nearest usable prop and what it is.
    pub nearest: Option<(Entity, InteractionKind)>,
    /// Every tree a trainer within reach teaches.
    pub trees: BTreeSet<String>,
    /// Whether a workbench is within its reach, nearest or not: a bench
    /// stands beside a locker, and the rule that a weapon is worked on at
    /// one must not turn on which of the two is a step closer.
    pub workbench: bool,
    /// Whether a fabricator is within its reach, nearest or not: the rule a
    /// run is started, worked and stopped by, as the workbench's is for a
    /// fit.
    pub fabricator: bool,
    /// The buyer whose board is within its reach, nearest or not: the rule
    /// a sale is held to.
    pub board: Option<Namespaced>,
    /// The factions whose guard is within reach: the rule a lease bought
    /// from a guard is held to.
    pub guards: BTreeSet<FactionId>,
    /// The people of the questline within reach: the rule a conversation is
    /// opened by.
    pub people: BTreeSet<Namespaced>,
}

impl Reach {
    /// Whether a trainer within reach teaches `id`'s tree, or a workbench
    /// does for a tree taught at any bench: the teacher argument of
    /// [`CharacterSheet::learn`].
    #[must_use]
    pub fn teaches(&self, graph: &SkillGraph, id: &str) -> bool {
        graph
            .tree_of(id)
            .is_some_and(|tree| self.trees.contains(&tree.id) || (self.workbench && tree.bench))
    }
}

/// The local player used what it stood beside (the F key, a script).
/// The world, and later a screen, answers it; the sim only carries it.
#[derive(Message, Debug, Clone, PartialEq)]
pub struct Interact {
    /// The prop.
    pub entity: Entity,
    /// What it is, so a reader need not look it up.
    pub kind: InteractionKind,
}

/// A handle a [`Combatant`](crate::combat::Combatant) holds so a clone is a pointer copy.
pub type Effective = Arc<EffectiveTables>;

#[cfg(any(test, feature = "test-support"))]
pub mod test_support {
    #![allow(
        missing_docs,
        clippy::new_without_default,
        clippy::wildcard_imports,
        reason = "test helpers the sim's tests share"
    )]
    #![cfg_attr(
        not(test),
        allow(
            unused_imports,
            reason = "the #[test] fns that use some imports compile under cfg(test) only, where the lint stays on"
        )
    )]
    #![allow(
        clippy::float_cmp,
        reason = "the design's numbers are set exactly; a bit off is a wrong table"
    )]

    use super::*;

    use crate::combat::Action;

    use crate::combat::CombatIntent;
    use crate::combat::CombatMotion;
    use crate::combat::Combatant;
    use crate::combat::Weapon;

    use crate::combat_tables::CombatTables;
    use crate::combat_tables::GuardRow;

    use strata_registry::progression::ACTION_FIELDS;
    use strata_registry::progression::ACTIONS;
    use strata_registry::progression::ACTOR_FIELDS;
    use strata_registry::progression::Effect;
    use strata_registry::progression::Op;

    use strata_registry::progression::SkillGraph;
    use strata_registry::progression::WEAPON_FIELDS;
    use strata_registry::progression::WEAPONS;
    use strata_registry::progression::XpKind;
    use strata_types::PlayerIntent;

    pub fn graph() -> SkillGraph {
        SkillGraph::shipped()
    }

    /// Effects of one field apply Set, then Add, then Mul whatever order
    /// they come in, and the last Set wins.
    #[test]
    fn a_field_folds_set_then_add_then_mul_in_any_order() {
        let ops = [(Op::Mul, 2.0), (Op::Add, 3.0), (Op::Set, 10.0)];
        let want = (10.0 + 3.0) * 2.0;
        for order in [
            [0, 1, 2],
            [0, 2, 1],
            [1, 0, 2],
            [1, 2, 0],
            [2, 0, 1],
            [2, 1, 0],
        ] {
            let got = fold_ops(100.0, order.map(|i| ops[i]));
            assert!((got - want).abs() < 1e-6, "{order:?}: {got}");
        }
        assert!(
            (fold_ops(7.0, []) - 7.0).abs() < 1e-6,
            "no effect, the base"
        );
        // The shipped graph: the surveyor's range is the dials' plus each box.
        let graph = graph();
        let boxes = [
            "surveyor.novice",
            "surveyor.deposits.1",
            "surveyor.deposits.3",
        ];
        let learned = sheet(&boxes);
        let mut backwards = CharacterSheet::default();
        for id in boxes.iter().rev() {
            backwards.learned.insert((*id).to_owned());
        }
        for each in [&learned, &backwards] {
            assert!((survey_field(&graph, each, "range_m", 128.0) - 320.0).abs() < 1e-4);
            assert!((survey_field(&graph, each, "channel_s", 2.5) - 2.0).abs() < 1e-4);
            assert!((ride_field(&graph, each, "call_s", 1.5) - 1.5).abs() < 1e-4);
        }
    }

    /// The guard row of a class after a graph's `Guard` effects on a sheet.
    pub fn guards_after(effects: &[(&str, Op, f32)], weapon: Weapon) -> GuardRow {
        let mut graph = graph();
        for (field, op, value) in effects {
            graph.trees[0].novice.effects.push(Effect::Guard {
                field: (*field).to_owned(),
                op: *op,
                value: *value,
            });
        }
        let tables = CombatTables::shipped();
        *apply(&tables, &graph, &sheet(&["marksman.novice"]))
            .guards
            .get(&weapon)
            .expect("a melee class")
    }

    #[test]
    #[allow(clippy::float_cmp, reason = "bit for bit is the claim")]
    fn a_guard_effect_moves_each_field_of_every_class_and_an_unfitted_actor_reads_the_table() {
        let tables = CombatTables::shipped();
        for weapon in Weapon::MELEE {
            let base = tables.melee.class(weapon).expect("class").guard;
            assert_eq!(guards_after(&[], weapon), base, "no effect, no change");
            for (field, add) in [
                ("share", 0.05),
                ("arc", 5.),
                ("stamina", -0.1),
                ("posture", 0.05),
                ("parry", 0.03),
                ("riposte", 5.),
            ] {
                let mut want = base;
                *want.field_mut(field).expect("field") += add;
                assert_eq!(guards_after(&[(field, Op::Add, add)], weapon), want);
                let mut want = base;
                *want.field_mut(field).expect("field") *= 1.25;
                assert_eq!(guards_after(&[(field, Op::Mul, 1.25)], weapon), want);
                let mut want = base;
                *want.field_mut(field).expect("field") = 0.5;
                assert_eq!(guards_after(&[(field, Op::Set, 0.5)], weapon), want);
            }
            // Set, then Add, then Mul, whatever the order they were written.
            let mixed = guards_after(
                &[
                    ("parry", Op::Mul, 2.),
                    ("parry", Op::Add, 0.1),
                    ("parry", Op::Set, 0.3),
                ],
                weapon,
            );
            assert!((mixed.parry - 0.8).abs() < 1e-6);
        }
        // A fitted actor with no Guard effect reads the table's bits, and
        // one never fitted reads the table.
        let unfitted = Combatant::default();
        let mut fitted = Combatant::default();
        fitted.fit(&graph(), &sheet(&["blade.novice"]));
        for weapon in Weapon::MELEE {
            let base = tables.melee.class(weapon).expect("class").guard;
            assert_eq!(unfitted.guard_row(weapon), Some(base));
            assert_eq!(fitted.guard_row(weapon), Some(base));
        }
        assert_eq!(unfitted.guard_row(Weapon::Carbine), None);
    }

    #[test]
    fn the_blade_guard_boxes_train_the_guard_row() {
        let graph = graph();
        let mut learned = vec!["blade.novice"];
        let base = CombatTables::shipped()
            .melee
            .class(Weapon::Blade)
            .expect("class")
            .guard;
        let read = |ids: &[&str]| {
            let mut actor = Combatant::default();
            actor.fit(&graph, &sheet(ids));
            actor.guard_row(Weapon::Blade).expect("row")
        };
        learned.push("blade.guard.1");
        assert!((read(&learned).stamina - base.stamina * 0.9).abs() < 1e-6);
        learned.push("blade.guard.2");
        assert!((read(&learned).parry - (base.parry + 0.03)).abs() < 1e-6);
        learned.push("blade.guard.3");
        assert!((read(&learned).share - (base.share + 0.05)).abs() < 1e-6);
        learned.push("blade.guard.4");
        assert!((read(&learned).riposte - base.riposte * 1.25).abs() < 1e-4);
    }

    #[test]
    #[allow(clippy::float_cmp, reason = "bit for bit is the claim")]
    fn the_heavy_cut_is_the_novice_s_and_cuts_three_gives_it_no_more() {
        let graph = graph();
        let tables = CombatTables::shipped();
        let novice = apply(&tables, &graph, &sheet(&["blade.novice"]));
        assert!(novice.unlocked.contains(&Action::Heavy));
        assert!(novice.abilities.contains_key("heavy_cut"));
        assert!(crate::ability::known(&graph, &sheet(&["blade.novice"])).contains("heavy_cut"));
        assert_eq!(
            novice.spec(Action::Heavy),
            tables.actions.actions[&Action::Heavy],
            "the heavy row is the table's, bit for bit"
        );
        let cuts3 = apply(
            &tables,
            &graph,
            &sheet(&[
                "blade.novice",
                "blade.cuts.1",
                "blade.cuts.2",
                "blade.cuts.3",
            ]),
        );
        let heavy = tables.actions.actions[&Action::Heavy];
        assert_eq!(cuts3.spec(Action::Heavy).damage, heavy.damage);
        assert_eq!(cuts3.spec(Action::Heavy).posture, heavy.posture * 1.15);
        let opens = graph
            .find("blade.cuts.3")
            .expect("box")
            .effects
            .iter()
            .any(|e| {
                matches!(e, Effect::Unlock { .. })
                    || matches!(e, Effect::Grant { ability } if ability == "heavy_cut")
            });
        assert!(!opens, "cuts 3 opens no heavy any more");
    }

    #[test]
    fn every_ability_effect_of_the_trees_moves_the_figure_it_names() {
        let graph = graph();
        let tables = CombatTables::shipped();
        let mut seen = 0;
        for b in graph.boxes() {
            for effect in &b.effects {
                let Effect::Ability {
                    id,
                    field,
                    op,
                    value,
                } = effect
                else {
                    continue;
                };
                let base = tables.abilities.row(id).expect("row");
                let mut moved = base.clone();
                assert!(moved.modify(field, *op, *value), "{} {id} {field}", b.id);
                assert_ne!(&moved, base, "{} {id} {field}", b.id);
                // Through the merge: `apply` reads effects, not chains.
                let effective = apply(&tables, &graph, &sheet(&[b.id.as_str()]));
                assert_eq!(effective.abilities[id.as_str()], moved, "{}", b.id);
                seen += 1;
            }
        }
        assert_eq!(seen, 9, "the trees sharpen their abilities: {seen}");
    }

    pub fn sorted(names: &[&str]) -> Vec<String> {
        let mut names: Vec<String> = names.iter().map(|n| (*n).to_owned()).collect();
        names.sort_unstable();
        names
    }

    /// A sheet with `ids` learned and plenty of XP of every kind.
    pub fn sheet(ids: &[&str]) -> CharacterSheet {
        let mut sheet = CharacterSheet::default();
        for kind in XpKind::ALL {
            sheet.credit(kind, 100_000);
        }
        for id in ids {
            sheet.learned.insert((*id).to_owned());
        }
        sheet
    }

    #[test]
    fn the_registry_vocabulary_is_the_sim_s_structs() {
        let names: Vec<&str> = Action::ALL.iter().map(|a| a.name()).collect();
        assert_eq!(names, ACTIONS);
        let names: Vec<&str> = Weapon::ALL.iter().map(|w| w.name()).collect();
        assert_eq!(names, WEAPONS);
        // The RON map sorts its keys, so both sides are compared sorted.
        let keys = |text: String| -> Vec<String> {
            let value: ron::Value = ron::from_str(&text).expect("ron");
            let ron::Value::Map(map) = value else {
                panic!("a struct")
            };
            let mut keys: Vec<String> = map
                .keys()
                .map(|k| match k {
                    ron::Value::String(s) => s.clone(),
                    other => panic!("{other:?}"),
                })
                .collect();
            keys.sort_unstable();
            keys
        };
        let tables = CombatTables::shipped();
        let mut spec = tables.actions.spec(Action::Light);
        let fields = keys(ron::to_string(&spec).expect("ron"));
        assert_eq!(fields, sorted(ACTION_FIELDS));
        for field in ACTION_FIELDS {
            assert!(spec.field_mut(field).is_some(), "{field}");
        }
        assert!(spec.field_mut("nothing").is_none());
        let mut weapon = tables.weapons.spec(Weapon::Carbine);
        let mut fields = keys(ron::to_string(&weapon).expect("ron"));
        // How a press becomes rounds, the bloom and the falloff are
        // tables of their own, not numbers a box scales; zoom is the client's.
        fields.retain(|f| !["mode", "hold", "bloom", "falloff", "zoom"].contains(&f.as_str()));
        assert_eq!(fields, sorted(WEAPON_FIELDS));
        for field in WEAPON_FIELDS {
            assert!(weapon.modify(field, Op::Add, 0.), "{field}");
        }
        assert!(!weapon.modify("nothing", Op::Add, 0.));
        let mut actor = tables.actor.clone();
        let mut fields = keys(ron::to_string(&actor).expect("ron"));
        fields.retain(|f| f != "version");
        assert_eq!(fields, sorted(ACTOR_FIELDS));
        for field in ACTOR_FIELDS {
            assert!(actor.field_mut(field).is_some(), "{field}");
        }
        assert!(actor.field_mut("version").is_none());
    }

    #[test]
    fn points_are_derived_and_the_budget_is_the_graph_s() {
        let graph = graph();
        let mut sheet = sheet(&["blade.novice", "blade.cuts.1"]);
        assert_eq!(sheet.points_spent(&graph), 8);
        assert_eq!(sheet.points_free(&graph), 242);
        sheet.learned.insert("not.a.box".into());
        assert_eq!(sheet.points_spent(&graph), 8, "an unknown id costs nothing");
    }

    #[test]
    fn learning_needs_the_chain_the_xp_the_points_and_a_teacher() {
        let graph = graph();
        let mut fresh = CharacterSheet::default();
        assert_eq!(
            fresh.learn(&graph, "blade.cuts.1", true),
            Err(LearnRefused::MissingPrerequisite("blade.novice".into()))
        );
        assert_eq!(
            fresh.learn(&graph, "blade.novice", false),
            Err(LearnRefused::NoTeacher)
        );
        assert_eq!(fresh.learn(&graph, "blade.novice", true), Ok(()));
        assert_eq!(
            fresh.learn(&graph, "blade.novice", true),
            Err(LearnRefused::AlreadyLearned("blade.novice".into()))
        );
        assert_eq!(
            fresh.learn(&graph, "blade.cuts.1", true),
            Err(LearnRefused::NotEnoughXp {
                kind: XpKind::Blade,
                have: 0,
                need: 150
            })
        );
        fresh.credit(XpKind::Ranged, 1000);
        assert!(
            matches!(
                fresh.learn(&graph, "blade.cuts.1", true),
                Err(LearnRefused::NotEnoughXp { .. })
            ),
            "the wrong kind"
        );
        fresh.credit(XpKind::Blade, 150);
        assert_eq!(fresh.learn(&graph, "blade.cuts.1", true), Ok(()));
        assert_eq!(
            fresh.learn(&graph, "blade.cuts.3", true),
            Err(LearnRefused::MissingPrerequisite("blade.cuts.2".into()))
        );
        assert_eq!(
            fresh.learn(&graph, "nothing", true),
            Err(LearnRefused::UnknownBox("nothing".into()))
        );
        // The budget: two full trees and a half is 245, and the next
        // tree's novice (6) does not fit.
        let mut full = sheet(&[]);
        for tree in ["blade", "marksman"] {
            for b in graph.tree(tree).expect(tree).boxes() {
                full.learn(&graph, &b.id, true).expect(&b.id);
            }
        }
        assert_eq!(full.points_spent(&graph), 196);
        let surveyor = graph.tree("surveyor").expect("ships");
        full.learn(&graph, &surveyor.novice.id, true)
            .expect("54 free");
        for branch in &surveyor.branches[..2] {
            for b in &branch.boxes {
                full.learn(&graph, &b.id, true).expect(&b.id);
            }
        }
        assert_eq!(full.points_free(&graph), 8);
        let third = &surveyor.branches[2];
        full.learn(&graph, &third.boxes[0].id, true)
            .expect("2 of 8");
        full.learn(&graph, &third.boxes[1].id, true)
            .expect("4 of 6");
        assert_eq!(
            full.learn(&graph, &third.boxes[2].id, true),
            Err(LearnRefused::NotEnoughPoints { cost: 6, free: 2 })
        );
    }

    #[test]
    fn forgetting_returns_points_keeps_xp_and_refuses_dependents_and_combat() {
        let graph = graph();
        let mut sheet = sheet(&["blade.novice", "blade.cuts.1", "blade.cuts.2"]);
        let xp = sheet.xp(XpKind::Blade);
        assert_eq!(
            sheet.forget(&graph, "blade.cuts.1", false),
            Err(ForgetRefused::Dependent("blade.cuts.2".into()))
        );
        assert_eq!(
            sheet.forget(&graph, "blade.cuts.2", true),
            Err(ForgetRefused::InCombat)
        );
        assert_eq!(sheet.forget(&graph, "blade.cuts.2", false), Ok(4));
        assert_eq!(sheet.forget(&graph, "blade.cuts.1", false), Ok(2));
        assert_eq!(
            sheet.forget(&graph, "blade.cuts.1", false),
            Err(ForgetRefused::NotLearned("blade.cuts.1".into()))
        );
        assert_eq!(sheet.points_spent(&graph), 6);
        assert_eq!(sheet.xp(XpKind::Blade), xp, "XP is kept");
        let actor = Combatant::default();
        assert!(!actor.in_combat(false));
        assert!(actor.in_combat(true));
        let mut swinging = Combatant::default();
        swinging.action = Action::Light;
        assert!(swinging.in_combat(false));
    }

    #[test]
    fn field_craft_one_lengthens_both_weapons_ranges() {
        let tables = CombatTables::shipped();
        let learned = apply(&tables, &graph(), &sheet(&["marksman.field_craft.1"]));
        for weapon in Weapon::GUNS {
            let was = tables.weapons.spec(weapon).range;
            let now = learned.weapon_spec(weapon).range;
            // The rifle's range is Long shots 4's alone.
            let by = if weapon == Weapon::Rifle { 1. } else { 1.25 };
            assert!((now - was * by).abs() < 1e-4, "{weapon:?}: {was} to {now}");
        }
    }

    #[test]
    #[allow(clippy::too_many_lines, reason = "one box after another")]
    fn the_blade_and_marksman_boxes_change_exactly_what_the_design_says() {
        let graph = graph();
        let tables = CombatTables::shipped();
        let base = apply(&tables, &graph, &CharacterSheet::default());
        // No boxes: the tables as installed, with the gated things closed
        // and everything else open.
        assert_eq!(base.actions, tables.actions.actions);
        assert_eq!(base.weapons, tables.weapons.weapons);
        assert_eq!(base.actor, tables.actor);
        assert!(!base.unlocked.contains(&Action::Light));
        assert!(!base.unlocked.contains(&Action::Followup));
        assert!(!base.unlocked.contains(&Action::Heavy));
        assert!(base.unlocked.contains(&Action::Dodge));
        assert!(base.certified.is_empty());

        let novice = apply(&tables, &graph, &sheet(&["blade.novice"]));
        assert!(novice.unlocked.contains(&Action::Light));
        assert!(!novice.unlocked.contains(&Action::Followup));
        assert!(
            novice.unlocked.contains(&Action::Heavy),
            "every cut's second"
        );
        assert_eq!(novice.actions, base.actions, "the novice changes no number");

        let cuts1 = apply(&tables, &graph, &sheet(&["blade.novice", "blade.cuts.1"]));
        assert!(cuts1.unlocked.contains(&Action::Followup));
        assert_eq!(cuts1.actions, base.actions);

        let cuts3 = apply(
            &tables,
            &graph,
            &sheet(&[
                "blade.novice",
                "blade.cuts.1",
                "blade.cuts.2",
                "blade.cuts.3",
            ]),
        );
        assert!(cuts3.unlocked.contains(&Action::Heavy));
        assert_eq!(cuts3.spec(Action::Followup).damage, 40., "Cuts 2");
        let mut expected = base.actions.clone();
        expected.get_mut(&Action::Followup).expect("row").damage = 40.;
        expected.get_mut(&Action::Heavy).expect("row").posture *= 1.15;
        assert_eq!(
            cuts3.actions, expected,
            "only the return cut's damage and the heavy's posture moved"
        );

        let footwork2 = apply(
            &tables,
            &graph,
            &sheet(&["blade.novice", "blade.footwork.1", "blade.footwork.2"]),
        );
        assert_eq!(footwork2.spec(Action::Light).dodge_cancel, 0.26);
        assert_eq!(footwork2.spec(Action::Followup).dodge_cancel, 0.26);
        assert_eq!(footwork2.spec(Action::Dodge).stamina, 22., "Footwork 1");
        assert_eq!(
            footwork2.spec(Action::Heavy).dodge_cancel,
            0.69,
            "the heavy's is not moved"
        );

        let footwork4 = apply(
            &tables,
            &graph,
            &sheet(&[
                "blade.novice",
                "blade.footwork.1",
                "blade.footwork.2",
                "blade.footwork.3",
                "blade.footwork.4",
            ]),
        );
        assert_eq!(footwork4.spec(Action::Dodge).contact_end, 0.28);
        assert_eq!(footwork4.spec(Action::Dodge).contact_start, 0.06);
        assert_eq!(footwork4.spec(Action::Dodge).travel, 4.0);

        let guard1 = apply(&tables, &graph, &sheet(&["blade.novice", "blade.guard.1"]));
        assert_eq!(guard1.actor.posture_regen, 48.);

        let breaking4 = apply(
            &tables,
            &graph,
            &sheet(&[
                "blade.novice",
                "blade.breaking.1",
                "blade.breaking.2",
                "blade.breaking.3",
                "blade.breaking.4",
            ]),
        );
        assert_eq!(breaking4.spec(Action::Heavy).posture, 110.);
        assert!((breaking4.spec(Action::Heavy).contact_start - 0.28).abs() < 1e-6);
        assert_eq!(breaking4.spec(Action::Light).posture, 40.);
        assert_eq!(breaking4.spec(Action::Followup).posture, 50.);

        let marksman = apply(&tables, &graph, &sheet(&["marksman.novice"]));
        assert!(marksman.certified.contains(&Weapon::Pistol));
        assert!(!marksman.certified.contains(&Weapon::Carbine));
        let carbine3 = apply(
            &tables,
            &graph,
            &sheet(&[
                "marksman.novice",
                "marksman.carbine.1",
                "marksman.carbine.2",
                "marksman.carbine.3",
            ]),
        );
        assert!(carbine3.certified.contains(&Weapon::Carbine));
        assert!(
            (carbine3.weapon_spec(Weapon::Carbine).interval - 0.42 * 0.85).abs() < 1e-6,
            "Carbine 3"
        );
        assert!(
            (carbine3.weapon_spec(Weapon::Carbine).reload - 1.1475).abs() < 1e-6,
            "Carbine 2"
        );
        assert_eq!(
            carbine3.weapon_spec(Weapon::Pistol),
            base.weapon_spec(Weapon::Pistol)
        );

        let steadiness2 = apply(
            &tables,
            &graph,
            &sheet(&[
                "marksman.novice",
                "marksman.steadiness.1",
                "marksman.steadiness.2",
            ]),
        );
        assert_eq!(steadiness2.actor.aim_speed_scale, 0.8);
        assert!((steadiness2.spec(Action::Swap).duration - 0.36).abs() < 1e-6);

        // The pistol's magazine is a count: Pistol 2 adds four whole
        // rounds, and the master's quarter rounds to eighteen.
        let pistol2 = apply(
            &tables,
            &graph,
            &sheet(&["marksman.novice", "marksman.pistol.1", "marksman.pistol.2"]),
        );
        assert_eq!(pistol2.weapon_spec(Weapon::Pistol).magazine, 14);
        let mut master = sheet(&[]);
        for b in graph.tree("marksman").expect("ships").boxes() {
            master.learned.insert(b.id.clone());
        }
        let master = apply(&tables, &graph, &master);
        assert_eq!(master.weapon_spec(Weapon::Pistol).magazine, 18);
        assert_eq!(master.weapon_spec(Weapon::Carbine).magazine, 30);
        assert!((master.actor.hurt_wait - 2.5).abs() < 1e-6);
    }

    #[test]
    fn sets_come_before_adds_before_muls_whatever_the_order_learned() {
        // Footwork 2 sets the Light cancel, the Blade master takes three
        // stamina off and multiplies the regen: the result is the same
        // whichever order the ids sit in the set (it is a BTreeSet, so
        // this is really a check that the passes are by op, not by box).
        let graph = graph();
        let tables = CombatTables::shipped();
        let mut all = sheet(&[]);
        for b in graph.tree("blade").expect("ships").boxes() {
            all.learned.insert(b.id.clone());
        }
        let effective = apply(&tables, &graph, &all);
        assert_eq!(effective.spec(Action::Light).stamina, 15.);
        assert_eq!(effective.actor.stamina_regen, 30.);
        assert_eq!(effective.actor.posture_regen, 48.);
    }

    #[test]
    fn a_combatant_without_cuts_1_ignores_a_follow_up_and_the_novice_has_the_heavy() {
        let graph = graph();
        let movement = PlayerIntent::default();
        let mut motion = CombatMotion::default();
        let dt = 1. / 60.;
        // The novice alone: a light, a light pressed in contact, and no
        // return cut comes of it.
        let mut actor =
            Combatant::default().carrying([Some(Weapon::Blade), Some(Weapon::Pistol)], 0);
        actor.fit(&graph, &sheet(&["blade.novice"]));
        let mut input = CombatIntent {
            light: true,
            ..Default::default()
        };
        actor.step(&mut input, &movement, dt, &mut motion);
        assert_eq!(actor.action, Action::Light);
        let mut saw_followup = false;
        for _ in 0..60 {
            if actor.action == Action::Light && actor.age >= 0.14 && actor.age < 0.16 {
                input.light = true;
            }
            actor.step(&mut input, &movement, dt, &mut motion);
            actor.finish_contacts(&movement);
            saw_followup |= actor.action == Action::Followup;
        }
        assert!(!saw_followup);
        assert_eq!(actor.sequence, 1, "one cut begun");
        // The heavy is the novice's since step 7 of ticket 0010.
        input.heavy = true;
        actor.step(&mut input, &movement, dt, &mut motion);
        assert_eq!(actor.action, Action::Heavy);
        // With Cuts 1 the same presses chain.
        let mut actor =
            Combatant::default().carrying([Some(Weapon::Blade), Some(Weapon::Pistol)], 0);
        actor.fit(&graph, &sheet(&["blade.novice", "blade.cuts.1"]));
        let mut input = CombatIntent {
            light: true,
            ..Default::default()
        };
        actor.step(&mut input, &movement, dt, &mut motion);
        let mut saw_followup = false;
        for _ in 0..60 {
            if actor.action == Action::Light && actor.age >= 0.14 && actor.age < 0.16 {
                input.light = true;
            }
            actor.step(&mut input, &movement, dt, &mut motion);
            actor.finish_contacts(&movement);
            saw_followup |= actor.action == Action::Followup;
        }
        assert!(saw_followup);
        // No sheet at all: everything open, as the harness expects.
        let mut actor =
            Combatant::default().carrying([Some(Weapon::Blade), Some(Weapon::Pistol)], 0);
        let mut input = CombatIntent {
            heavy: true,
            ..Default::default()
        };
        actor.step(&mut input, &movement, dt, &mut motion);
        assert_eq!(actor.action, Action::Heavy);
    }

    #[test]
    fn an_uncertified_weapon_neither_fires_nor_draws() {
        let graph = graph();
        let movement = PlayerIntent::default();
        let mut motion = CombatMotion::default();
        let dt = 1. / 60.;
        // The pistol only: the carbine in hand will not fire, the pistol
        // can be drawn, the carbine cannot be drawn back.
        let mut actor = Combatant::default();
        actor.fit(&graph, &sheet(&["marksman.novice"]));
        let mut input = CombatIntent {
            fire: true,
            ..Default::default()
        };
        for _ in 0..30 {
            actor.step(&mut input, &movement, dt, &mut motion);
        }
        assert_eq!(actor.shots, 0, "the carbine is not certified");
        input.weapon = Some(1);
        for tick in 0..60 {
            // A pistol fires a press at a time.
            input.fire = tick % 2 == 0;
            actor.step(&mut input, &movement, dt, &mut motion);
        }
        input.fire = true;
        assert_eq!(actor.weapon, Weapon::Pistol);
        assert!(actor.shots > 0, "the pistol fires");
        input.weapon = Some(0);
        for _ in 0..60 {
            actor.step(&mut input, &movement, dt, &mut motion);
        }
        assert_eq!(
            actor.weapon,
            Weapon::Pistol,
            "no drawing an uncertified weapon"
        );
        assert!(input.weapon.is_none(), "the refused draw does not linger");
    }

    #[test]
    fn a_fitted_actor_reads_its_own_rows_and_a_refit_keeps_full_pools_full() {
        let graph = graph();
        let mut actor = Combatant::default();
        assert_eq!(actor.spec(Action::Light).dodge_cancel, 0.31);
        actor.fit(
            &graph,
            &sheet(&[
                "blade.novice",
                "blade.footwork.1",
                "blade.footwork.2",
                "blade.guard.1",
                "blade.guard.2",
                "blade.guard.3",
            ]),
        );
        assert_eq!(actor.spec(Action::Light).dodge_cancel, 0.26);
        assert_eq!(actor.posture, 120., "a full pool follows its new cap");
        actor.posture = 50.;
        actor.fit(&graph, &sheet(&["blade.novice"]));
        assert_eq!(actor.posture, 50., "a part pool is kept");
        assert_eq!(actor.spec(Action::Light).dodge_cancel, 0.31);
        assert!(actor.effective().is_some());
    }

    #[test]
    fn a_shield_box_moves_the_capacity_and_an_opponent_s_own_is_kept() {
        let tables = CombatTables::shipped();
        let mut graph = graph();
        // A box of the shipped graph given a shield effect, so the test
        // does not depend on which box ships one.
        let b = &mut graph.trees[0].branches[0].boxes[0];
        b.effects.push(Effect::Actor {
            field: "shield".into(),
            op: Op::Add,
            value: 20.,
        });
        b.effects.push(Effect::Actor {
            field: "health".into(),
            op: Op::Add,
            value: 50.,
        });
        let id = b.id.clone();
        let mut actor = Combatant::default();
        actor.fit(&graph, &sheet(&["marksman.novice", &id]));
        assert_eq!(actor.shield_capacity, tables.actor.shield + 20.);
        assert_eq!(
            actor.shield,
            tables.actor.shield + 20.,
            "a full pool follows"
        );
        assert_eq!(actor.health, tables.actor.health + 50.);
        // Regeneration fills to the new capacity.
        actor.shield = 10.;
        let movement = PlayerIntent::default();
        let mut motion = CombatMotion::default();
        for _ in 0..60 * 60 {
            actor.step(
                &mut CombatIntent::default(),
                &movement,
                1. / 60.,
                &mut motion,
            );
        }
        assert_eq!(actor.shield, tables.actor.shield + 20.);
        // Forgetting the box clamps a full pool back down.
        actor.fit(&graph, &sheet(&["marksman.novice"]));
        assert_eq!(actor.shield_capacity, tables.actor.shield);
        assert_eq!(actor.shield, tables.actor.shield);
        // An opponent on its own capacity, fitted after: the capacity is
        // not the table's, so it is kept.
        let mut opponent = Combatant::on_team(1, 85.);
        opponent.shield = 20.;
        opponent.shield_capacity = 20.;
        opponent.fit(&graph, &sheet(&["marksman.novice", &id]));
        assert_eq!(opponent.shield_capacity, 20.);
        assert_eq!(opponent.health, 85.);
    }

    #[test]
    fn a_refused_press_pulses_once_and_says_why() {
        use crate::combat::Refusal;
        let graph = graph();
        let movement = PlayerIntent::default();
        let mut motion = CombatMotion::default();
        let dt = 1. / 60.;
        let mut actor = Combatant::default();
        actor.fit(&graph, &sheet(&["blade.novice"]));
        // The carbine is not certified: a held trigger is one refusal.
        let mut input = CombatIntent {
            fire: true,
            ..Default::default()
        };
        for _ in 0..30 {
            actor.step(&mut input, &movement, dt, &mut motion);
        }
        assert_eq!(actor.refusals, 1);
        assert_eq!(actor.refused, Some(Refusal::Fire(Weapon::Carbine)));
        input.fire = false;
        actor.step(&mut input, &movement, dt, &mut motion);
        input.fire = true;
        actor.step(&mut input, &movement, dt, &mut motion);
        assert_eq!(actor.refusals, 2, "a new press is a new refusal");
        // A cut with an uncertified blade in hand (the sheet has no blade
        // box now), a draw of the uncertified pistol.
        actor.fit(&graph, &sheet(&["medic.novice"]));
        actor.carry([Some(Weapon::Blade), Some(Weapon::Pistol)]);
        input.fire = false;
        input.light = true;
        actor.step(&mut input, &movement, dt, &mut motion);
        assert_eq!(actor.refused, Some(Refusal::Fire(Weapon::Blade)));
        input.light = false;
        input.weapon = Some(1);
        actor.step(&mut input, &movement, dt, &mut motion);
        assert_eq!(actor.refused, Some(Refusal::Draw(Weapon::Pistol)));
        assert_eq!(actor.refusals, 4);
        // A locked follow-up is the action's refusal: fitted with the novice
        // only, a second press inside the first cut's contact asks for it.
        let mut cutter = Combatant::default().carrying([Some(Weapon::Blade), None], 0);
        cutter.fit(&graph, &sheet(&["blade.novice"]));
        let mut press = CombatIntent {
            light: true,
            ..Default::default()
        };
        cutter.step(&mut press, &movement, dt, &mut motion);
        let mut refused = None;
        for _ in 0..60 {
            if cutter.action == Action::Light && cutter.age >= 0.14 && cutter.age < 0.16 {
                press.light = true;
            }
            cutter.step(&mut press, &movement, dt, &mut motion);
            cutter.finish_contacts(&movement);
            refused = refused.or(cutter.refused);
        }
        assert_eq!(refused, Some(Refusal::Action(Action::Followup)));
        // An actor with no sheet refuses nothing.
        let mut free =
            Combatant::default().carrying([Some(Weapon::Blade), Some(Weapon::Pistol)], 0);
        let mut input = CombatIntent {
            heavy: true,
            fire: true,
            ..Default::default()
        };
        free.step(&mut input, &movement, dt, &mut motion);
        assert_eq!(free.refusals, 0);
    }

    #[test]
    fn long_shots_certify_the_rifle_and_improve_it() {
        let graph = graph();
        let tables = CombatTables::shipped();
        let none = apply(&tables, &graph, &CharacterSheet::default());
        let table = tables.weapons.spec(Weapon::Rifle);
        assert!(
            !none.certified.contains(&Weapon::Rifle),
            "closed until learned"
        );
        let learned = |boxes: &[&str]| apply(&tables, &graph, &sheet(boxes));
        let one = learned(&["ranger.long_shots.1"]);
        assert!(one.certified.contains(&Weapon::Rifle));
        assert_eq!(
            one.weapon_spec(Weapon::Rifle),
            table,
            "a certificate moves no number"
        );
        let two = learned(&["ranger.long_shots.1", "ranger.long_shots.2"]);
        let row = two.weapon_spec(Weapon::Rifle);
        assert!((row.aim_time - table.aim_time * 0.75).abs() < 1e-6);
        assert_eq!(row.reload.to_bits(), table.reload.to_bits());
        let three = learned(&["ranger.long_shots.3"]).weapon_spec(Weapon::Rifle);
        assert!((three.reload - table.reload * 0.85).abs() < 1e-6);
        assert_eq!(three.aim_time.to_bits(), table.aim_time.to_bits());
        let four = learned(&["ranger.long_shots.4"]).weapon_spec(Weapon::Rifle);
        assert!((four.range - table.range * 1.15).abs() < 1e-4);
        assert_eq!(four.falloff, table.falloff);
        // No other weapon's row moves, and the other branches teach nothing.
        let all = learned(&[
            "ranger.novice",
            "ranger.long_shots.1",
            "ranger.long_shots.2",
            "ranger.long_shots.3",
            "ranger.long_shots.4",
            "ranger.tracking.1",
            "ranger.wilderness.1",
            "ranger.native_life.1",
        ]);
        for weapon in [Weapon::Carbine, Weapon::Pistol] {
            assert_eq!(all.weapon_spec(weapon), none.weapon_spec(weapon));
        }
        let lonely = learned(&["ranger.novice", "ranger.tracking.1", "ranger.wilderness.1"]);
        assert_eq!(lonely.weapons, none.weapons);
        assert_eq!(lonely.certified, none.certified);
        // The actor's own row reads them, and a draw of the rifle is refused
        // without the certificate.
        let mut actor = Combatant::default();
        actor.fit(&graph, &sheet(&["marksman.novice", "marksman.carbine.1"]));
        actor.carry([Some(Weapon::Carbine), Some(Weapon::Rifle)]);
        let mut input = CombatIntent {
            weapon: Some(1),
            ..Default::default()
        };
        let mut motion = CombatMotion::default();
        actor.step(&mut input, &PlayerIntent::default(), 1. / 60., &mut motion);
        assert_eq!(
            actor.refused,
            Some(crate::combat::Refusal::Draw(Weapon::Rifle))
        );
        assert_eq!(actor.weapon, Weapon::Carbine);
        actor.fit(
            &graph,
            &sheet(&[
                "marksman.novice",
                "marksman.carbine.1",
                "ranger.long_shots.1",
                "ranger.long_shots.4",
            ]),
        );
        assert!(actor.certified(Weapon::Rifle));
        assert!((actor.spec_of(Weapon::Rifle).range - table.range * 1.15).abs() < 1e-4);
        let mut input = CombatIntent {
            weapon: Some(1),
            ..Default::default()
        };
        actor.step(&mut input, &PlayerIntent::default(), 1. / 60., &mut motion);
        for _ in 0..60 {
            let mut idle = CombatIntent::default();
            actor.step(&mut idle, &PlayerIntent::default(), 1. / 60., &mut motion);
        }
        assert_eq!(actor.weapon, Weapon::Rifle, "drawn once certified");
    }

    /// Who of the first household can walk to Ranger, and what it costs:
    /// the Surveyor, who has the Marksman novice, the Surveyor novice and
    /// Reading 1 and lacks Carbine 1 (a Marksman trainer's) and Routes 1,
    /// the Ranger novice and Long shots 1 (the survey trainer's).
    #[test]
    fn the_first_households_surveyor_can_learn_long_shots_at_twelve_points() {
        let registries =
            strata_registry::load_from_dir(concat!(env!("CARGO_MANIFEST_DIR"), "/../../assets"))
                .expect("assets/ loads");
        let graph = registries.progression.clone();
        let household = registries
            .sheets
            .household
            .as_ref()
            .expect("the household ships");
        let chain = [
            "marksman.carbine.1",
            "surveyor.routes.1",
            "ranger.novice",
            "ranger.long_shots.1",
        ];
        let reaches = |member: &strata_registry::progression::MemberDef| {
            let mut sheet = CharacterSheet::authored(&member.sheet);
            let before = sheet.points_spent(&graph);
            let walked = chain.iter().all(|id| sheet.learn(&graph, id, true).is_ok());
            walked.then(|| sheet.points_spent(&graph) - before)
        };
        let costs: Vec<(&str, Option<u32>)> = std::iter::once(&household.arrival)
            .chain(household.recruits.iter())
            .map(|member| (member.name.as_str(), reaches(member)))
            .collect();
        assert_eq!(
            costs
                .iter()
                .filter(|(_, cost)| cost.is_some())
                .collect::<Vec<_>>(),
            [&("Ilse Marrow", Some(12))],
            "{costs:?}"
        );
    }
}
