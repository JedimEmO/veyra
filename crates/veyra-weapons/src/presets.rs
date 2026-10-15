//! Authored assembly variations expose component tradeoffs without random rolls.
use crate::{Assembly, AssemblyError, Finish, MODULES, Weapon};

/// A named, editable starting point; it never adds hidden bonuses.
#[derive(Clone, Copy, Debug, serde::Serialize)]
pub struct Preset {
    /// Stable command-line and content identifier.
    pub id: &'static str,
    /// Short fitting-bench label.
    pub name: &'static str,
    /// The receiver retained by this configuration.
    pub weapon: Weapon,
    /// Intended use, rather than an absolute power ranking.
    pub description: &'static str,
    /// Explicit fitted components; every other socket remains stock.
    pub modules: &'static [&'static str],
}

impl Preset {
    /// Resolve through the same fitting checks as individual component changes.
    ///
    /// # Errors
    /// Rejects unknown or incompatible catalogue components.
    pub fn assembly(&self, finish: Finish) -> Result<Assembly, AssemblyError> {
        let mut assembly = Assembly {
            weapon: self.weapon,
            finish,
            ..Assembly::default()
        };
        for id in self.modules {
            let module = MODULES
                .iter()
                .find(|module| module.id == *id)
                .ok_or_else(|| AssemblyError::UnknownModule((*id).into()))?;
            assembly.select(module.slot, Some(id))?;
        }
        Ok(assembly)
    }
}

macro_rules! preset {
    ($id:literal, $name:literal, $weapon:ident, $description:literal, [$($module:literal),*]) => {
        Preset { id:$id, name:$name, weapon:Weapon::$weapon, description:$description, modules:&[$($module),*] }
    };
}

/// Two purpose-built variations per chassis, with no hidden statistical effects.
pub const PRESETS: [Preset; 26] = [
    preset!(
        "service_courier",
        "Courier",
        Pistol,
        "Low sight and slim grip keep the sidearm unobtrusive.",
        ["pistol_low_optic", "pistol_slim_grip"]
    ),
    preset!(
        "service_watch",
        "Long watch",
        Pistol,
        "Extra cell capacity and a firm grip trade speed for steadiness.",
        [
            "compact_optic",
            "pistol_extended_cell",
            "pistol_stable_grip"
        ]
    ),
    preset!(
        "service_scout",
        "Scout",
        BurstRifle,
        "Open fore-end and a compact sight reduce the forward burden.",
        ["compact_optic", "rifle_open_fore_end"]
    ),
    preset!(
        "service_overwatch",
        "Workshop overwatch",
        BurstRifle,
        "Adapted Morrow optic with the larger cell and cooled shroud.",
        [
            "adapted_morrow_optic",
            "rifle_extended_cell",
            "rifle_cooled_fore_end"
        ]
    ),
    preset!(
        "morrow_rover",
        "Rover",
        PrecisionRifle,
        "Compact feed keeps the long receiver easy to carry.",
        ["precision_compact_feed"]
    ),
    preset!(
        "morrow_lookout",
        "Lookout",
        PrecisionRifle,
        "Long optic and a supported stock suit deliberate shots.",
        ["precision_long_optic", "precision_braced_stock"]
    ),
    preset!(
        "bracken_trail",
        "Trail gun",
        Scattergun,
        "Wrapped forward grip with the standard muzzle and feed.",
        ["scatter_wrapped_grip"]
    ),
    preset!(
        "bracken_entry",
        "Entry gun",
        Scattergun,
        "Wide muzzle and extended feed emphasize short engagements.",
        ["scatter_wide_muzzle", "scatter_extended_feed"]
    ),
    preset!(
        "kestrel_convoy",
        "Convoy",
        SupportProjector,
        "Reserve cell extends time away from resupply.",
        ["support_reserve_cell"]
    ),
    preset!(
        "kestrel_emplacement",
        "Field support",
        SupportProjector,
        "Additional cooling and a wider rest favor a fixed position.",
        ["support_cooling_jacket", "support_stable_rest"]
    ),
    preset!(
        "morrow_duelist",
        "Balanced blade",
        FieldBlade,
        "A balanced grip retains the straight physical cutting edge.",
        ["blade_balanced_grip"]
    ),
    preset!(
        "morrow_salvager",
        "Salvage blade",
        FieldBlade,
        "Hooked tip and balanced grip make a distinct working tool.",
        ["blade_hooked_edge", "blade_balanced_grip"]
    ),
    preset!(
        "bracken_pack_cutter",
        "Pack cutter",
        HeavyCutter,
        "Compact power pack and wrapped handle reduce carrying burden.",
        ["cutter_compact_power", "cutter_wrapped_grip"]
    ),
    preset!(
        "bracken_work_cutter",
        "Heavy work",
        HeavyCutter,
        "Reinforced cutting head keeps the industrial silhouette.",
        ["cutter_reinforced_head", "cutter_wrapped_grip"]
    ),
    preset!(
        "morrow_reach_broad",
        "Broad glaive",
        ReachGlaive,
        "A wider head trades stamina for the harder cut.",
        ["glaive_broad_edge"]
    ),
    preset!(
        "morrow_reach_weighted",
        "Weighted glaive",
        ReachGlaive,
        "A heavy butt shortens the recovery and slows the draw.",
        ["glaive_counterweight"]
    ),
    preset!(
        "bracken_watch_baton",
        "Watch baton",
        ImpactBaton,
        "Wrapped grip improves purchase while retaining the stock head.",
        ["baton_wrapped_grip"]
    ),
    preset!(
        "bracken_impact_baton",
        "Weighted baton",
        ImpactBaton,
        "More head mass trades recovery for impact.",
        ["baton_weighted_head", "baton_wrapped_grip"]
    ),
    preset!(
        "vigil_sidearm_patrol",
        "Patrol sidearm",
        CombatPistol,
        "Low reflex sight and firm grip for controlled handling.",
        ["vigil_reflex", "vigil_firm_grip", "vigil_compensator"]
    ),
    preset!(
        "vigil_sidearm_boarding",
        "Boarding sidearm",
        CombatPistol,
        "Extended feed and shrouded muzzle on the compact receiver.",
        [
            "combat_extended_feed",
            "vigil_angled_grip",
            "vigil_shrouded_muzzle"
        ]
    ),
    preset!(
        "vigil_patrol",
        "Patrol kit",
        PatrolCarbine,
        "Open stock, reflex sight and ventilated forward housing.",
        [
            "vigil_reflex",
            "patrol_vented_fore_end",
            "vigil_wire_stock",
            "vigil_angled_grip",
            "vigil_compensator"
        ]
    ),
    preset!(
        "vigil_recon",
        "Recon kit",
        PatrolCarbine,
        "Prism sight, supported stock and guarded fore-end.",
        [
            "vigil_prism",
            "patrol_extended_feed",
            "patrol_guarded_fore_end",
            "vigil_precision_stock",
            "vigil_shrouded_muzzle"
        ]
    ),
    preset!(
        "vigil_marksman_mobile",
        "Mobile marksman",
        MarksmanRifle,
        "Prism sight and compact feed keep the long weapon manageable.",
        ["vigil_prism", "marksman_compact_feed", "vigil_compensator"]
    ),
    preset!(
        "vigil_marksman_watch",
        "Long watch",
        MarksmanRifle,
        "Long optic, field rest and supported stock for patient fire.",
        [
            "vigil_long_optic",
            "vigil_precision_stock",
            "marksman_field_rest",
            "vigil_shrouded_muzzle"
        ]
    ),
    preset!(
        "vigil_breacher_entry",
        "Entry kit",
        Breacher,
        "Open stock and angled grip on a compact heavy receiver.",
        [
            "vigil_reflex",
            "vigil_wire_stock",
            "vigil_angled_grip",
            "vigil_compensator"
        ]
    ),
    preset!(
        "vigil_breacher_hold",
        "Hold kit",
        Breacher,
        "Extended feed and supported stock trade speed for endurance.",
        [
            "breacher_extended_feed",
            "vigil_precision_stock",
            "vigil_firm_grip",
            "vigil_shrouded_muzzle"
        ]
    ),
];

/// Catalogue order is stable so the UI and review gallery show the same choices.
pub fn for_weapon(weapon: Weapon) -> impl Iterator<Item = &'static Preset> {
    PRESETS.iter().filter(move |preset| preset.weapon == weapon)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn authored_presets_fit_without_duplicate_slots_or_hidden_effects() {
        let mut ids = std::collections::BTreeSet::new();
        for preset in PRESETS {
            assert!(ids.insert(preset.id), "preset ids are stable and unique");
            for finish in Finish::ALL {
                let assembly = preset.assembly(finish).expect("authored preset fits");
                assert_eq!(
                    assembly.modules.len(),
                    preset.modules.len(),
                    "no silent same-slot replacement"
                );
                assert_eq!(assembly.finish, finish);
            }
        }
        for weapon in Weapon::ALL {
            assert_eq!(for_weapon(weapon).count(), 2);
        }
    }
}
