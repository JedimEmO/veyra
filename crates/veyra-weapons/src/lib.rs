//! Compatible, serializable equipment choices kept independent of rendering.
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// Procedural assembly recipes for the selected equipment.
pub mod geometry;
mod military;
pub mod model;
pub mod picture;
pub mod presets;
/// Build the selected procedural assembly.
pub use geometry::build;

/// Stable weapon choices used by saved assemblies.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum Weapon {
    /// Compact service plasma sidearm.
    Pistol,
    /// Three-pulse service plasma rifle.
    BurstRifle,
    /// Conventional precision rifle with a narrow spine.
    PrecisionRifle,
    /// Close-range conventional cartridge weapon.
    Scattergun,
    /// Heavy plasma projector with a substantial cooling jacket.
    SupportProjector,
    /// Slim physical cutting edge.
    FieldBlade,
    /// Powered industrial cutting tool.
    HeavyCutter,
    /// Short physical impact tool.
    ImpactBaton,
    /// Restrained conventional military sidearm.
    CombatPistol,
    /// Compact military patrol weapon.
    PatrolCarbine,
    /// Military rifle with a long precision spine.
    MarksmanRifle,
    /// Short military scatter weapon.
    Breacher,
    /// Long polearm with a single edged head.
    ReachGlaive,
}

/// Stable maker choices used by saved assemblies.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum Maker {
    /// The Kestrel option.
    Kestrel,
    /// The Morrow option.
    Morrow,
    /// The Bracken option.
    Bracken,
    /// Dark military equipment with restrained field fittings.
    Vigil,
}

/// Stable slot choices used by saved assemblies.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum Slot {
    /// The Sight option.
    Sight,
    /// The Feed option.
    Feed,
    /// Front shroud, cooling insert and muzzle assembly.
    ForeEnd,
    /// The Grip option.
    Grip,
    /// The Muzzle option.
    Muzzle,
    /// The Stock option.
    Stock,
    /// The Cooling option.
    Cooling,
    /// The Support option.
    Support,
    /// The Edge option.
    Edge,
    /// The Head option.
    Head,
    /// The Power option.
    Power,
}

/// Stable finish choices used by saved assemblies.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum Finish {
    /// The Issued option.
    Issued,
    /// The Field option.
    Field,
    /// The Bare option.
    Bare,
}

impl Weapon {
    /// Enumerate every supported chassis in display order.
    pub const ALL: [Self; 13] = [
        Self::Pistol,
        Self::BurstRifle,
        Self::PrecisionRifle,
        Self::Scattergun,
        Self::SupportProjector,
        Self::FieldBlade,
        Self::HeavyCutter,
        Self::ImpactBaton,
        Self::ReachGlaive,
        Self::CombatPistol,
        Self::PatrolCarbine,
        Self::MarksmanRifle,
        Self::Breacher,
    ];
    /// Stable key for authored equipment.
    #[must_use]
    pub const fn key(self) -> &'static str {
        match self {
            Self::Pistol => "service_pistol",
            Self::BurstRifle => "burst_rifle",
            Self::PrecisionRifle => "precision_rifle",
            Self::Scattergun => "scattergun",
            Self::SupportProjector => "support_projector",
            Self::FieldBlade => "field_blade",
            Self::HeavyCutter => "heavy_cutter",
            Self::ImpactBaton => "impact_baton",
            Self::ReachGlaive => "reach_glaive",
            Self::CombatPistol => "combat_pistol",
            Self::PatrolCarbine => "patrol_carbine",
            Self::MarksmanRifle => "marksman_rifle",
            Self::Breacher => "breacher",
        }
    }
    /// Readable chassis name.
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Pistol => "Service pistol",
            Self::BurstRifle => "Burst rifle",
            Self::PrecisionRifle => "Precision rifle",
            Self::Scattergun => "Scattergun",
            Self::SupportProjector => "Support projector",
            Self::FieldBlade => "Field blade",
            Self::HeavyCutter => "Heavy cutter",
            Self::ImpactBaton => "Impact baton",
            Self::ReachGlaive => "Reach glaive",
            Self::CombatPistol => "Combat pistol",
            Self::PatrolCarbine => "Patrol carbine",
            Self::MarksmanRifle => "Marksman rifle",
            Self::Breacher => "Breacher",
        }
    }
    /// Intended handling role, not a balance measurement.
    #[must_use]
    pub const fn description(self) -> &'static str {
        match self {
            Self::Pistol => {
                "Compact plasma weapon for mobile, close-range pressure. Fast handling and a replaceable power cell."
            }
            Self::BurstRifle => {
                "Three-pulse plasma rifle for deliberate medium-range fire. Modular sights, power feed and cooling shroud."
            }
            Self::PrecisionRifle => {
                "Long-range deliberate shots with commitment and poor close handling."
            }
            Self::Scattergun => {
                "Close-range spread and front-loaded damage, limited posture and costly reload openings."
            }
            Self::SupportProjector => {
                "Direct-fire lane pressure; heat and bracing create a commando playstyle without splash."
            }
            Self::FieldBlade => {
                "Stamina-efficient close pressure, limited reach and modest posture per contact."
            }
            Self::HeavyCutter => "Committed posture-breaking cut with costly misses.",
            Self::ImpactBaton => {
                "Compact impact tool for close-range pressure. A weighted head trades recovery speed for a harder strike."
            }
            Self::ReachGlaive => {
                "Long polearm with a single edged head: reach over speed, held by the leading hand with a counterweight behind."
            }
            Self::CombatPistol => {
                "Compact Vigil sidearm with dark service panels and interchangeable field fittings."
            }
            Self::PatrolCarbine => {
                "Lean Vigil carbine for mobile patrols, with a vented fore-end and a restrained modular stock."
            }
            Self::MarksmanRifle => {
                "Long Vigil precision chassis with a narrow receiver, protected sight line and optional field rest."
            }
            Self::Breacher => {
                "Short Vigil scatter weapon with a broad muzzle, compact feed and close handling."
            }
        }
    }
    /// Identify the shared restrained military material language.
    #[must_use]
    pub const fn military(self) -> bool {
        matches!(
            self,
            Self::CombatPistol | Self::PatrolCarbine | Self::MarksmanRifle | Self::Breacher
        )
    }
    /// Nominal chassis length in metres.
    #[must_use]
    pub const fn length(self) -> f64 {
        match self {
            Self::Pistol => 0.24,
            Self::BurstRifle => 0.76,
            Self::PrecisionRifle => 1.05,
            Self::Scattergun => 0.69,
            Self::SupportProjector => 0.98,
            Self::FieldBlade => 0.72,
            Self::HeavyCutter => 1.02,
            Self::ImpactBaton => 0.52,
            Self::ReachGlaive => 2.14,
            Self::CombatPistol => 0.22,
            Self::PatrolCarbine => 0.78,
            Self::MarksmanRifle => 1.03,
            Self::Breacher => 0.68,
        }
    }
    /// Supplier defining the stock visual language.
    #[must_use]
    pub const fn maker(self) -> Maker {
        match self {
            Self::Pistol | Self::BurstRifle | Self::SupportProjector => Maker::Kestrel,
            Self::PrecisionRifle | Self::FieldBlade | Self::ReachGlaive => Maker::Morrow,
            Self::Scattergun | Self::HeavyCutter | Self::ImpactBaton => Maker::Bracken,
            Self::CombatPistol | Self::PatrolCarbine | Self::MarksmanRifle | Self::Breacher => {
                Maker::Vigil
            }
        }
    }
    /// Only these sockets may contain functional replacements.
    #[must_use]
    pub const fn slots(self) -> &'static [Slot] {
        match self {
            Self::Pistol => &[Slot::Sight, Slot::Feed, Slot::Grip],
            Self::BurstRifle => &[Slot::Sight, Slot::Feed, Slot::ForeEnd],
            Self::PrecisionRifle => &[Slot::Sight, Slot::Feed, Slot::Stock],
            Self::Scattergun => &[Slot::Muzzle, Slot::Feed, Slot::Grip],
            Self::SupportProjector => &[Slot::Cooling, Slot::Feed, Slot::Support],
            Self::FieldBlade | Self::ReachGlaive => &[Slot::Edge, Slot::Grip],
            Self::HeavyCutter => &[Slot::Head, Slot::Power, Slot::Grip],
            Self::ImpactBaton => &[Slot::Head, Slot::Grip],
            Self::CombatPistol => &[Slot::Sight, Slot::Feed, Slot::Grip, Slot::Muzzle],
            Self::PatrolCarbine => &[
                Slot::Sight,
                Slot::Feed,
                Slot::ForeEnd,
                Slot::Stock,
                Slot::Muzzle,
                Slot::Grip,
            ],
            Self::MarksmanRifle => &[
                Slot::Sight,
                Slot::Feed,
                Slot::Stock,
                Slot::Muzzle,
                Slot::Support,
            ],
            Self::Breacher => &[
                Slot::Sight,
                Slot::Feed,
                Slot::Grip,
                Slot::Muzzle,
                Slot::Stock,
            ],
        }
    }
}
impl Maker {
    /// Stable supplier display order.
    pub const ALL: [Self; 4] = [Self::Kestrel, Self::Morrow, Self::Bracken, Self::Vigil];
    /// Shared design language for filtering and comparing equipment.
    #[must_use]
    pub const fn description(self) -> &'static str {
        match self {
            Self::Kestrel => "Service plasma equipment with pale shells and inset energy cores.",
            Self::Morrow => "Slender precision equipment with quiet surfaces and exposed metal.",
            Self::Bracken => "Industrial field tools with worn casing and tactile fittings.",
            Self::Vigil => {
                "Dark military equipment with lean receivers, recessed panels and restrained field fittings."
            }
        }
    }

    /// Readable display name.
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Kestrel => "Kestrel",
            Self::Morrow => "Morrow",
            Self::Bracken => "Bracken",
            Self::Vigil => "Vigil",
        }
    }
}
impl Slot {
    /// Stable compatibility key.
    #[must_use]
    pub const fn key(self) -> &'static str {
        match self {
            Self::Sight => "sight",
            Self::Feed => "feed",
            Self::ForeEnd => "fore_end",
            Self::Grip => "grip",
            Self::Muzzle => "muzzle",
            Self::Stock => "stock",
            Self::Cooling => "cooling",
            Self::Support => "support",
            Self::Edge => "edge",
            Self::Head => "head",
            Self::Power => "power",
        }
    }
    /// Readable display name.
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Sight => "Sight",
            Self::Feed => "Feed",
            Self::ForeEnd => "Fore End",
            Self::Grip => "Grip",
            Self::Muzzle => "Muzzle",
            Self::Stock => "Stock",
            Self::Cooling => "Cooling",
            Self::Support => "Support",
            Self::Edge => "Edge",
            Self::Head => "Head",
            Self::Power => "Power",
        }
    }
}
impl Finish {
    /// Available bounded material treatments.
    pub const ALL: [Self; 3] = [Self::Issued, Self::Field, Self::Bare];
    /// Readable display name.
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Issued => "Issued",
            Self::Field => "Field",
            Self::Bare => "Bare",
        }
    }
}

/// A relative paper factor; it is never an absolute combat statistic.
#[derive(Clone, Copy, Debug, Serialize)]
pub struct Effect {
    /// Human-readable property name, shared across modules for composition.
    pub label: &'static str,
    /// Multiplicative change relative to this chassis' stock property.
    pub factor: f32,
}
/// A finite compatible replacement, not a free attachment point.
#[derive(Clone, Copy, Debug, Serialize)]
pub struct Module {
    /// Stable short item key.
    pub id: &'static str,
    /// Display name.
    pub name: &'static str,
    /// The occupied functional socket.
    pub slot: Slot,
    /// Explicitly permitted chassis.
    pub weapons: &'static [Weapon],
    /// Visible construction and handling brief.
    pub description: &'static str,
    /// Proposed dimensionless factors for the build report.
    pub effects: &'static [Effect],
}
/// The complete finite replacement catalogue.
pub const MODULES: [Module; 43] = [
    Module {
        id: "compact_optic",
        name: "Service compact optic",
        description: "Low enclosed optical hood on the shared service sight interface.",
        slot: Slot::Sight,
        weapons: &[Weapon::Pistol, Weapon::BurstRifle],
        effects: &[
            Effect {
                label: "aim spread",
                factor: 0.9,
            },
            Effect {
                label: "equip time",
                factor: 1.06,
            },
        ],
    },
    Module {
        id: "pistol_extended_cell",
        name: "Pistol extended cell",
        description: "Deeper cell heel with unchanged key and withdrawal axis.",
        slot: Slot::Feed,
        weapons: &[Weapon::Pistol],
        effects: &[
            Effect {
                label: "capacity",
                factor: 1.25,
            },
            Effect {
                label: "reload time",
                factor: 1.15,
            },
        ],
    },
    Module {
        id: "pistol_stable_grip",
        name: "Pistol stability grip",
        description: "Broader rear grip panel contained inside the original hand envelope.",
        slot: Slot::Grip,
        weapons: &[Weapon::Pistol],
        effects: &[
            Effect {
                label: "recoil recovery",
                factor: 0.9,
            },
            Effect {
                label: "equip time",
                factor: 1.08,
            },
        ],
    },
    Module {
        id: "rifle_extended_cell",
        name: "Rifle extended cell",
        description: "Downward extension of the stock power pack, with enlarged reload clearance.",
        slot: Slot::Feed,
        weapons: &[Weapon::BurstRifle],
        effects: &[
            Effect {
                label: "capacity",
                factor: 1.25,
            },
            Effect {
                label: "reload time",
                factor: 1.15,
            },
        ],
    },
    Module {
        id: "rifle_cooled_fore_end",
        name: "Rifle cooled fore-end",
        description: "Longer ribbed outer cooling sleeve; support hand position unchanged.",
        slot: Slot::ForeEnd,
        weapons: &[Weapon::BurstRifle],
        effects: &[
            Effect {
                label: "repeat interval",
                factor: 0.92,
            },
            Effect {
                label: "aim move speed",
                factor: 0.92,
            },
        ],
    },
    Module {
        id: "adapted_morrow_optic",
        name: "Workshop adapted Morrow optic",
        description: "Morrow narrow optic on a visible workshop adapter shoe; one assembled sight module.",
        slot: Slot::Sight,
        weapons: &[Weapon::BurstRifle],
        effects: &[
            Effect {
                label: "aim spread",
                factor: 0.82,
            },
            Effect {
                label: "equip time",
                factor: 1.15,
            },
            Effect {
                label: "aim move speed",
                factor: 0.95,
            },
        ],
    },
    Module {
        id: "precision_long_optic",
        name: "Precision long optic",
        description: "Long protected optic aligned with the stock sight centreline.",
        slot: Slot::Sight,
        weapons: &[Weapon::PrecisionRifle],
        effects: &[
            Effect {
                label: "aim spread",
                factor: 0.85,
            },
            Effect {
                label: "equip time",
                factor: 1.12,
            },
        ],
    },
    Module {
        id: "precision_compact_feed",
        name: "Precision compact magazine",
        description: "Shorter box with identical top interface and latch.",
        slot: Slot::Feed,
        weapons: &[Weapon::PrecisionRifle],
        effects: &[
            Effect {
                label: "capacity",
                factor: 0.8,
            },
            Effect {
                label: "reload time",
                factor: 0.9,
            },
        ],
    },
    Module {
        id: "scatter_wide_muzzle",
        name: "Scatter spread muzzle",
        description: "Wide vented muzzle assembly; wider pellet pattern buys faster discharge recovery.",
        slot: Slot::Muzzle,
        weapons: &[Weapon::Scattergun],
        effects: &[
            Effect {
                label: "aim spread",
                factor: 1.15,
            },
            Effect {
                label: "repeat interval",
                factor: 0.94,
            },
        ],
    },
    Module {
        id: "scatter_extended_feed",
        name: "Scatter extended magazine",
        description: "Deeper detachable box; tube-fed and per-shell reloads are outside this first chassis.",
        slot: Slot::Feed,
        weapons: &[Weapon::Scattergun],
        effects: &[
            Effect {
                label: "capacity",
                factor: 1.25,
            },
            Effect {
                label: "reload time",
                factor: 1.15,
            },
        ],
    },
    Module {
        id: "support_cooling_jacket",
        name: "Support cooling jacket",
        description: "Larger finned jacket contained away from support grip and exhaust path.",
        slot: Slot::Cooling,
        weapons: &[Weapon::SupportProjector],
        effects: &[
            Effect {
                label: "cooling rate",
                factor: 1.15,
            },
            Effect {
                label: "brace time",
                factor: 1.1,
            },
        ],
    },
    Module {
        id: "support_stable_rest",
        name: "Support stabilizing rest",
        description: "Broad fixed underside rest, no folding bipod or terrain lock in the first proof.",
        slot: Slot::Support,
        weapons: &[Weapon::SupportProjector],
        effects: &[
            Effect {
                label: "recoil recovery",
                factor: 0.9,
            },
            Effect {
                label: "brace time",
                factor: 1.15,
            },
        ],
    },
    Module {
        id: "blade_balanced_grip",
        name: "Blade balanced grip",
        description: "Inset grip weights and restrained end-cap change; blade profile and palm frame stay fixed.",
        slot: Slot::Grip,
        weapons: &[Weapon::FieldBlade],
        effects: &[
            Effect {
                label: "attack stamina",
                factor: 0.9,
            },
            Effect {
                label: "hit posture",
                factor: 0.94,
            },
        ],
    },
    Module {
        id: "cutter_reinforced_head",
        name: "Cutter reinforced head",
        description: "Thicker rear head spine; cutting edge endpoints remain fixed.",
        slot: Slot::Head,
        weapons: &[Weapon::HeavyCutter],
        effects: &[
            Effect {
                label: "hit posture",
                factor: 1.12,
            },
            Effect {
                label: "recovery time",
                factor: 1.1,
            },
        ],
    },
    Module {
        id: "glaive_broad_edge",
        name: "Glaive broad edge",
        description: "A wider leaf head with a second inlay along the belly; the shaft and collar stay fixed.",
        slot: Slot::Edge,
        weapons: &[Weapon::ReachGlaive],
        effects: &[
            Effect {
                label: "hit damage",
                factor: 1.08,
            },
            Effect {
                label: "attack stamina",
                factor: 1.08,
            },
        ],
    },
    Module {
        id: "glaive_counterweight",
        name: "Glaive counterweight",
        description: "A fat weighted butt behind the hands; the grip and the head stay fixed.",
        slot: Slot::Grip,
        weapons: &[Weapon::ReachGlaive],
        effects: &[
            Effect {
                label: "recovery time",
                factor: 0.92,
            },
            Effect {
                label: "equip time",
                factor: 1.06,
            },
        ],
    },
    Module {
        id: "baton_weighted_head",
        name: "Baton weighted head",
        description: "Replaceable blunt head collar inside the approved reach envelope.",
        slot: Slot::Head,
        weapons: &[Weapon::ImpactBaton],
        effects: &[
            Effect {
                label: "hit posture",
                factor: 1.1,
            },
            Effect {
                label: "hit damage",
                factor: 0.9,
            },
        ],
    },
    Module {
        id: "pistol_low_optic",
        name: "Pistol open sight",
        description: "Shallow open hood with a clear sight line and little added height.",
        slot: Slot::Sight,
        weapons: &[Weapon::Pistol],
        effects: &[
            Effect {
                label: "aim spread",
                factor: 0.96,
            },
            Effect {
                label: "equip time",
                factor: 1.02,
            },
        ],
    },
    Module {
        id: "pistol_slim_grip",
        name: "Pistol slim grip",
        description: "Thin inset panels keep the grip light and compact.",
        slot: Slot::Grip,
        weapons: &[Weapon::Pistol],
        effects: &[
            Effect {
                label: "equip time",
                factor: 0.94,
            },
            Effect {
                label: "recoil recovery",
                factor: 1.06,
            },
        ],
    },
    Module {
        id: "rifle_open_fore_end",
        name: "Rifle open fore-end",
        description: "Short skeletal sleeve leaves the cooling core visible.",
        slot: Slot::ForeEnd,
        weapons: &[Weapon::BurstRifle],
        effects: &[
            Effect {
                label: "equip time",
                factor: 0.95,
            },
            Effect {
                label: "cooling rate",
                factor: 0.94,
            },
        ],
    },
    Module {
        id: "precision_braced_stock",
        name: "Precision braced stock",
        description: "Narrow triangular rear brace supports a restrained cheek saddle.",
        slot: Slot::Stock,
        weapons: &[Weapon::PrecisionRifle],
        effects: &[
            Effect {
                label: "aim spread",
                factor: 0.96,
            },
            Effect {
                label: "equip time",
                factor: 1.05,
            },
        ],
    },
    Module {
        id: "scatter_wrapped_grip",
        name: "Scatter wrapped grip",
        description: "Segmented rubber bands provide a readable tactile surface.",
        slot: Slot::Grip,
        weapons: &[Weapon::Scattergun],
        effects: &[
            Effect {
                label: "recoil recovery",
                factor: 0.95,
            },
            Effect {
                label: "equip time",
                factor: 1.04,
            },
        ],
    },
    Module {
        id: "support_reserve_cell",
        name: "Support reserve cell",
        description: "Deeper power pack adds endurance and a slower exchange.",
        slot: Slot::Feed,
        weapons: &[Weapon::SupportProjector],
        effects: &[
            Effect {
                label: "capacity",
                factor: 1.2,
            },
            Effect {
                label: "reload time",
                factor: 1.12,
            },
        ],
    },
    Module {
        id: "blade_hooked_edge",
        name: "Blade hooked edge",
        description: "Subtle recurved tip stays inside the original reach envelope.",
        slot: Slot::Edge,
        weapons: &[Weapon::FieldBlade],
        effects: &[
            Effect {
                label: "hit posture",
                factor: 1.08,
            },
            Effect {
                label: "recovery time",
                factor: 1.06,
            },
        ],
    },
    Module {
        id: "cutter_compact_power",
        name: "Cutter compact cell",
        description: "Shorter rear cell reduces the silhouette and reserve.",
        slot: Slot::Power,
        weapons: &[Weapon::HeavyCutter],
        effects: &[
            Effect {
                label: "capacity",
                factor: 0.85,
            },
            Effect {
                label: "equip time",
                factor: 0.95,
            },
        ],
    },
    Module {
        id: "cutter_wrapped_grip",
        name: "Cutter wrapped grip",
        description: "Fine rubber bands break up the long handle.",
        slot: Slot::Grip,
        weapons: &[Weapon::HeavyCutter],
        effects: &[
            Effect {
                label: "attack stamina",
                factor: 0.96,
            },
            Effect {
                label: "equip time",
                factor: 1.03,
            },
        ],
    },
    Module {
        id: "baton_wrapped_grip",
        name: "Baton wrapped grip",
        description: "Fine rubber bands and a small inset end cap.",
        slot: Slot::Grip,
        weapons: &[Weapon::ImpactBaton],
        effects: &[
            Effect {
                label: "recovery time",
                factor: 0.95,
            },
            Effect {
                label: "hit posture",
                factor: 0.96,
            },
        ],
    },
    Module {
        id: "vigil_reflex",
        name: "Vigil reflex sight",
        description: "Low enclosed hood with a narrow protected lens.",
        slot: Slot::Sight,
        weapons: &[
            Weapon::CombatPistol,
            Weapon::PatrolCarbine,
            Weapon::MarksmanRifle,
            Weapon::Breacher,
        ],
        effects: &[
            Effect {
                label: "aim spread",
                factor: 0.70,
            },
            Effect {
                label: "equip time",
                factor: 1.03,
            },
        ],
    },
    Module {
        id: "vigil_prism",
        name: "Vigil prism sight",
        description: "Compact protected optic on a short saddle.",
        slot: Slot::Sight,
        weapons: &[Weapon::PatrolCarbine, Weapon::MarksmanRifle],
        effects: &[
            Effect {
                label: "aim spread",
                factor: 0.9,
            },
            Effect {
                label: "equip time",
                factor: 1.07,
            },
        ],
    },
    Module {
        id: "vigil_long_optic",
        name: "Vigil long optic",
        description: "Long narrow optic with separate mounting rings.",
        slot: Slot::Sight,
        weapons: &[Weapon::MarksmanRifle],
        effects: &[
            Effect {
                label: "aim spread",
                factor: 0.86,
            },
            Effect {
                label: "equip time",
                factor: 1.1,
            },
        ],
    },
    Module {
        id: "combat_extended_feed",
        name: "Combat extended magazine",
        description: "Deeper magazine heel below the main grip.",
        slot: Slot::Feed,
        weapons: &[Weapon::CombatPistol],
        effects: &[
            Effect {
                label: "capacity",
                factor: 1.2,
            },
            Effect {
                label: "reload time",
                factor: 1.1,
            },
        ],
    },
    Module {
        id: "patrol_extended_feed",
        name: "Patrol extended magazine",
        description: "Longer box with restrained ribbing and a narrow base plate.",
        slot: Slot::Feed,
        weapons: &[Weapon::PatrolCarbine],
        effects: &[
            Effect {
                label: "capacity",
                factor: 1.2,
            },
            Effect {
                label: "reload time",
                factor: 1.1,
            },
        ],
    },
    Module {
        id: "marksman_compact_feed",
        name: "Marksman compact magazine",
        description: "Short low-profile box leaves more space beneath the receiver.",
        slot: Slot::Feed,
        weapons: &[Weapon::MarksmanRifle],
        effects: &[
            Effect {
                label: "capacity",
                factor: 0.85,
            },
            Effect {
                label: "reload time",
                factor: 0.94,
            },
        ],
    },
    Module {
        id: "breacher_extended_feed",
        name: "Breacher extended magazine",
        description: "Deeper detachable box with broad recessed side panels.",
        slot: Slot::Feed,
        weapons: &[Weapon::Breacher],
        effects: &[
            Effect {
                label: "capacity",
                factor: 1.2,
            },
            Effect {
                label: "reload time",
                factor: 1.12,
            },
        ],
    },
    Module {
        id: "vigil_angled_grip",
        name: "Vigil angled grip",
        description: "Slim swept grip with recessed side panels.",
        slot: Slot::Grip,
        weapons: &[
            Weapon::CombatPistol,
            Weapon::PatrolCarbine,
            Weapon::Breacher,
        ],
        effects: &[
            Effect {
                label: "equip time",
                factor: 0.95,
            },
            Effect {
                label: "recoil recovery",
                factor: 1.05,
            },
        ],
    },
    Module {
        id: "vigil_firm_grip",
        name: "Vigil firm grip",
        description: "Slightly fuller rear insert and fine horizontal rubber ribs.",
        slot: Slot::Grip,
        weapons: &[
            Weapon::CombatPistol,
            Weapon::PatrolCarbine,
            Weapon::Breacher,
        ],
        effects: &[
            Effect {
                label: "recoil recovery",
                factor: 0.95,
            },
            Effect {
                label: "equip time",
                factor: 1.03,
            },
        ],
    },
    Module {
        id: "vigil_shrouded_muzzle",
        name: "Vigil shrouded muzzle",
        description: "Short cylindrical sleeve with a restrained stepped front collar.",
        slot: Slot::Muzzle,
        weapons: &[
            Weapon::CombatPistol,
            Weapon::PatrolCarbine,
            Weapon::MarksmanRifle,
            Weapon::Breacher,
        ],
        effects: &[
            Effect {
                label: "aim spread",
                factor: 0.97,
            },
            Effect {
                label: "equip time",
                factor: 1.04,
            },
        ],
    },
    Module {
        id: "vigil_compensator",
        name: "Vigil vented muzzle",
        description: "Short dark collar with visible side vents.",
        slot: Slot::Muzzle,
        weapons: &[
            Weapon::CombatPistol,
            Weapon::PatrolCarbine,
            Weapon::MarksmanRifle,
            Weapon::Breacher,
        ],
        effects: &[
            Effect {
                label: "recoil recovery",
                factor: 0.94,
            },
            Effect {
                label: "aim move speed",
                factor: 0.97,
            },
        ],
    },
    Module {
        id: "patrol_vented_fore_end",
        name: "Patrol vented fore-end",
        description: "Long lean side panels with repeated recessed vents.",
        slot: Slot::ForeEnd,
        weapons: &[Weapon::PatrolCarbine],
        effects: &[
            Effect {
                label: "cooling rate",
                factor: 1.08,
            },
            Effect {
                label: "equip time",
                factor: 1.03,
            },
        ],
    },
    Module {
        id: "patrol_guarded_fore_end",
        name: "Patrol guarded fore-end",
        description: "Short covered sleeve with a fine underside hand guard.",
        slot: Slot::ForeEnd,
        weapons: &[Weapon::PatrolCarbine],
        effects: &[
            Effect {
                label: "equip time",
                factor: 0.97,
            },
            Effect {
                label: "cooling rate",
                factor: 0.95,
            },
        ],
    },
    Module {
        id: "vigil_wire_stock",
        name: "Vigil open stock",
        description: "Two narrow open struts lead to a compact shoulder pad.",
        slot: Slot::Stock,
        weapons: &[Weapon::PatrolCarbine, Weapon::Breacher],
        effects: &[
            Effect {
                label: "equip time",
                factor: 0.96,
            },
            Effect {
                label: "recoil recovery",
                factor: 1.04,
            },
        ],
    },
    Module {
        id: "vigil_precision_stock",
        name: "Vigil supported stock",
        description: "Narrow cheek saddle and lower brace add rear support.",
        slot: Slot::Stock,
        weapons: &[
            Weapon::PatrolCarbine,
            Weapon::MarksmanRifle,
            Weapon::Breacher,
        ],
        effects: &[
            Effect {
                label: "aim spread",
                factor: 0.97,
            },
            Effect {
                label: "equip time",
                factor: 1.04,
            },
        ],
    },
    Module {
        id: "marksman_field_rest",
        name: "Marksman field rest",
        description: "Paired fixed support legs keep the long silhouette grounded.",
        slot: Slot::Support,
        weapons: &[Weapon::MarksmanRifle],
        effects: &[
            Effect {
                label: "aim spread",
                factor: 0.97,
            },
            Effect {
                label: "brace time",
                factor: 1.1,
            },
        ],
    },
];

/// Return only explicit replacements for this chassis and socket.
#[must_use]
pub fn modules(weapon: Weapon, slot: Slot) -> Vec<&'static Module> {
    MODULES
        .iter()
        .filter(|module| module.slot == slot && module.weapons.contains(&weapon))
        .collect()
}

/// A saved selection independent of meshes and GPU state.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Assembly {
    /// Stock chassis identity.
    pub weapon: Weapon,
    /// One replacement per occupied socket; absence selects the stock assembly.
    pub modules: BTreeMap<Slot, String>,
    /// Finite material treatment that does not alter combat properties.
    pub finish: Finish,
}
impl Default for Assembly {
    fn default() -> Self {
        Self {
            weapon: Weapon::BurstRifle,
            modules: BTreeMap::new(),
            finish: Finish::Issued,
        }
    }
}
/// Invalid input is reported without silently substituting stock parts.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum AssemblyError {
    /// The chassis does not expose the requested slot.
    #[error("{weapon:?} has no {slot:?} slot")]
    UnsupportedSlot {
        /// Selected chassis.
        weapon: Weapon,
        /// Requested socket.
        slot: Slot,
    },
    /// The module ID is not in the catalogue.
    #[error("unknown module {0}")]
    UnknownModule(String),
    /// A known module cannot occupy this chassis/socket pair.
    #[error("module {id} is incompatible with {weapon:?}/{slot:?}")]
    IncompatibleModule {
        /// Requested item key.
        id: String,
        /// Selected chassis.
        weapon: Weapon,
        /// Requested socket.
        slot: Slot,
    },
}
impl Assembly {
    /// Reject unknown, misplaced or incompatible saved selections before building.
    ///
    /// # Errors
    /// Returns the first unsupported socket, unknown item or incompatible item.
    pub fn validate(&self) -> Result<(), AssemblyError> {
        for (&slot, id) in &self.modules {
            if !self.weapon.slots().contains(&slot) {
                return Err(AssemblyError::UnsupportedSlot {
                    weapon: self.weapon,
                    slot,
                });
            }
            let module = MODULES
                .iter()
                .find(|module| module.id == id)
                .ok_or_else(|| AssemblyError::UnknownModule(id.clone()))?;
            if module.slot != slot || !module.weapons.contains(&self.weapon) {
                return Err(AssemblyError::IncompatibleModule {
                    id: id.clone(),
                    weapon: self.weapon,
                    slot,
                });
            }
        }
        Ok(())
    }
    /// Apply a selection atomically, preserving the previous build on failure.
    ///
    /// # Errors
    /// Refuses unsupported sockets, unknown IDs and incompatible combinations.
    pub fn select(&mut self, slot: Slot, id: Option<&str>) -> Result<(), AssemblyError> {
        if !self.weapon.slots().contains(&slot) {
            return Err(AssemblyError::UnsupportedSlot {
                weapon: self.weapon,
                slot,
            });
        }
        let mut candidate = self.clone();
        if let Some(id) = id {
            candidate.modules.insert(slot, id.to_owned());
        } else {
            candidate.modules.remove(&slot);
        }
        candidate.validate()?;
        *self = candidate;
        Ok(())
    }
    /// Resolve a validated replacement; invalid saved input must still be validated.
    #[must_use]
    pub fn selected(&self, slot: Slot) -> Option<&'static Module> {
        let id = self.modules.get(&slot)?;
        MODULES.iter().find(|module| {
            module.id == id && module.slot == slot && module.weapons.contains(&self.weapon)
        })
    }
    /// Restore the complete stock build while retaining chassis and finish.
    pub fn reset_modules(&mut self) {
        self.modules.clear();
    }
    /// Compose paper factors from immutable module definitions, once per socket.
    /// Invalid selections are omitted; call `validate` before displaying a saved build.
    #[must_use]
    pub fn factor_summary(&self) -> Vec<(&'static str, f32)> {
        let mut factors = BTreeMap::new();
        for &slot in self.weapon.slots() {
            if let Some(module) = self.selected(slot) {
                for effect in module.effects {
                    *factors.entry(effect.label).or_insert(1.0) *= effect.factor;
                }
            }
        }
        factors.into_iter().collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn every_legal_configuration_validates_and_has_bounded_factors() {
        let mut count = 0;
        for weapon in Weapon::ALL {
            let mut builds = vec![Assembly {
                weapon,
                ..Default::default()
            }];
            for &slot in weapon.slots() {
                let mut expanded = Vec::new();
                for build in builds {
                    expanded.push(build.clone());
                    for module in modules(weapon, slot) {
                        let mut variant = build.clone();
                        variant
                            .select(slot, Some(module.id))
                            .expect("catalogue option fits its declared slot");
                        expanded.push(variant);
                    }
                }
                builds = expanded;
            }
            for mut build in builds {
                build
                    .validate()
                    .expect("enumerated compatible assembly validates");
                for (_, factor) in build.factor_summary() {
                    // Lower bound 0.60 (was 0.65) on purpose: the reflex sight's 0.70 composes.
                    assert!(
                        factor.is_finite() && (0.60..=1.35).contains(&factor),
                        "{factor}"
                    );
                }
                for finish in Finish::ALL {
                    build.finish = finish;
                    let encoded = ron::to_string(&build).expect("legal assembly serializes");
                    let decoded: Assembly =
                        ron::from_str(&encoded).expect("legal assembly decodes");
                    assert_eq!(decoded, build);
                    decoded.validate().expect("round trip remains compatible");
                }
                count += 1;
            }
        }
        assert_eq!(count, 806);
    }

    #[test]
    fn catalogue_keys_are_unique_and_every_socket_has_a_replacement() {
        let ids: std::collections::BTreeSet<_> = MODULES.iter().map(|module| module.id).collect();
        assert_eq!(ids.len(), MODULES.len());
        let keys: std::collections::BTreeSet<_> =
            Weapon::ALL.iter().map(|weapon| weapon.key()).collect();
        assert_eq!(keys.len(), Weapon::ALL.len());
        for weapon in Weapon::ALL {
            assert_eq!(weapon.military(), weapon.maker() == Maker::Vigil);
            for &slot in weapon.slots() {
                assert!(
                    !modules(weapon, slot).is_empty(),
                    "{weapon:?}/{slot:?} has no replacement"
                );
                for module in &MODULES {
                    let mut assembly = Assembly {
                        weapon,
                        ..Default::default()
                    };
                    let compatible = module.slot == slot && module.weapons.contains(&weapon);
                    assert_eq!(assembly.select(slot, Some(module.id)).is_ok(), compatible);
                    if !compatible {
                        assert!(
                            assembly.modules.is_empty(),
                            "failed selection must remain atomic"
                        );
                    }
                }
            }
        }
        for module in &MODULES {
            assert!(!module.weapons.is_empty());
            assert!(!module.effects.is_empty());
            let labels: std::collections::BTreeSet<_> =
                module.effects.iter().map(|effect| effect.label).collect();
            assert_eq!(
                labels.len(),
                module.effects.len(),
                "duplicate effect in {}",
                module.id
            );
        }
    }

    #[test]
    fn original_saved_variant_names_still_decode() {
        for (variant, weapon) in [
            ("Pistol", Weapon::Pistol),
            ("BurstRifle", Weapon::BurstRifle),
            ("PrecisionRifle", Weapon::PrecisionRifle),
            ("Scattergun", Weapon::Scattergun),
            ("SupportProjector", Weapon::SupportProjector),
            ("FieldBlade", Weapon::FieldBlade),
            ("HeavyCutter", Weapon::HeavyCutter),
            ("ImpactBaton", Weapon::ImpactBaton),
            ("ReachGlaive", Weapon::ReachGlaive),
        ] {
            let encoded = format!("(weapon:{variant},modules:{{}},finish:Issued)");
            let decoded: Assembly =
                ron::from_str(&encoded).expect("original variant remains stable");
            assert_eq!(decoded.weapon, weapon);
            decoded
                .validate()
                .expect("original stock save remains valid");
        }
    }

    #[test]
    fn catalogue_compatibility_and_transactional_selection() {
        for module in &MODULES {
            for &weapon in module.weapons {
                let mut assembly = Assembly {
                    weapon,
                    ..Default::default()
                };
                assembly
                    .select(module.slot, Some(module.id))
                    .expect("module fits each declared chassis");
                assert_eq!(
                    assembly
                        .selected(module.slot)
                        .expect("selected module resolves")
                        .id,
                    module.id
                );
                assembly
                    .validate()
                    .expect("legal selection preserves a valid assembly");
            }
        }
        let mut assembly = Assembly::default();
        assembly
            .select(Slot::Sight, Some("compact_optic"))
            .expect("compact optic fits the burst rifle");
        let previous = assembly.clone();
        assert!(
            assembly
                .select(Slot::Sight, Some("precision_long_optic"))
                .is_err()
        );
        assert_eq!(assembly, previous);
        assert!(assembly.select(Slot::Feed, Some("unknown")).is_err());
        assert_eq!(assembly, previous);
        assert!(assembly.select(Slot::Edge, None).is_err());
        assert_eq!(assembly, previous);
    }
    #[test]
    fn serialization_requires_validation_and_rejects_unknown_fields() {
        let mut assembly = Assembly::default();
        assembly
            .select(Slot::Sight, Some("adapted_morrow_optic"))
            .expect("adapted optic fits the burst rifle");
        assembly.finish = Finish::Field;
        let encoded = ron::to_string(&assembly).expect("assembly serializes to RON");
        let decoded: Assembly = ron::from_str(&encoded).expect("saved assembly decodes");
        assert_eq!(decoded, assembly);
        decoded
            .validate()
            .expect("round trip preserves a valid selection");
        assert!(
            ron::from_str::<Assembly>("(weapon:BurstRifle,modules:{},finish:Issued,extra:1)")
                .is_err()
        );
        let invalid: Assembly =
            ron::from_str("(weapon:BurstRifle,modules:{Sight:\"missing\"},finish:Issued)")
                .expect("unknown module is syntactically valid RON");
        assert!(matches!(
            invalid.validate(),
            Err(AssemblyError::UnknownModule(_))
        ));
    }
    #[test]
    fn factors_rebuild_without_accumulating_and_reset_restores_stock() {
        let mut assembly = Assembly::default();
        assembly
            .select(Slot::Feed, Some("rifle_extended_cell"))
            .expect("extended cell fits the burst rifle");
        assert_eq!(assembly.factor_summary(), assembly.factor_summary());
        assert_eq!(
            assembly.factor_summary(),
            vec![("capacity", 1.25), ("reload time", 1.15)]
        );
        assembly.reset_modules();
        assert!(assembly.factor_summary().is_empty());
        assert_eq!(assembly.weapon, Weapon::BurstRifle);
    }
}
