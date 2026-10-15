//! Faction kinds: who a world's factions can be (ticket 0012, step 1), as
//! `factions/*.faction.ron` spell them, one a file, with the authored
//! holders in `factions/anchors.ron` and the territory dial in
//! `factions/rules.ron`.
//!
//! A kind says what sort of body it is, which site kinds it takes and how
//! far a holding's ground reaches, how many a land mass makes, the colours
//! it draws from, its insignia and the words its names are built of. The
//! factions themselves are made by `strata_worldgen::factions` from the
//! seed, the placed sites and this data.
//!
//! Everything a file could get wrong is refused at load with the file's
//! name. A kind that holds nothing is legal: the outlaws hold no site until
//! they have camps.
//!
//! None of it is world state: it joins no world version and no handshake. A
//! data set without the directory has no factions, and that is legal.

use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};
use slotted_model::Namespaced;

use crate::icons::IconCatalogue;
use crate::registries::GameRegistries;
use crate::settlement::SettlementDef;
use crate::site::NamesDef;

/// The directory of `.faction.ron` files, relative to the asset root.
pub const FACTIONS_DIR: &str = "factions";
/// The suffix a kind's file must carry to be read.
pub const FACTION_SUFFIX: &str = ".faction.ron";
/// The authored holders.
pub const ANCHORS_FILE: &str = "factions/anchors.ron";
/// The territory dial.
pub const FACTION_RULES_FILE: &str = "factions/rules.ron";
/// Which deposits are claims, and how a lease and a commons work.
pub const CLAIM_RULES_FILE: &str = "factions/claims.ron";
/// What a deed weighs, how long it is remembered and what a stance does to a
/// board's prices.
pub const DEED_RULES_FILE: &str = "factions/deeds.ron";

/// What sort of body a kind is: what later steps' rules read.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub enum Nature {
    /// A company: concessions, boards, guards.
    Corporate,
    /// Settlers governing themselves.
    Assembly,
    /// A body that keeps ground from being worked.
    Conservation,
    /// People outside every law, hostile on sight.
    Outlaw,
}

/// One site kind a faction kind takes.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HoldDef {
    /// The site kind, `strata:hamlet`.
    pub kind: Namespaced,
    /// How much it wants this kind: a site goes to the faction whose seat
    /// is nearest once the distance is divided by the weight.
    pub weight: f64,
    /// How far a holding of this kind reaches, metres.
    pub reach_m: f64,
    /// Whether a site of this kind can be a faction's seat. A wreck is held
    /// but is nobody's home, and counting it would reseat every faction
    /// already made.
    #[serde(default = "seats")]
    pub seat: bool,
}

fn seats() -> bool {
    true
}

/// How many factions of a kind a land mass makes.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PerMassDef {
    /// How many of its sites a faction of the kind is made for: the count is
    /// the eligible sites over this, rounded.
    pub sites_each: f64,
    /// The fewest a mass with any eligible site gets.
    pub least: u32,
    /// The most a mass gets.
    pub most: u32,
}

/// A kind of faction, as its file spells it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FactionKindDef {
    /// The namespaced id, `strata:corporate`.
    pub id: Namespaced,
    /// The kind's word, `Corporate concern`.
    pub name: String,
    /// What sort of body it is.
    pub nature: Nature,
    /// The site kinds it takes. Empty holds nothing.
    pub holds: Vec<HoldDef>,
    /// How many a land mass makes.
    pub per_mass: PerMassDef,
    /// The colours a faction of the kind takes one of, `#RRGGBB`.
    pub palette: Vec<String>,
    /// The icon library's symbol for its insignia.
    pub symbol: String,
    /// How its factions are named.
    pub names: NamesDef,
}

/// An authored holder: a settlement kept by a named body.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AnchorDef {
    /// The settlement it holds, `strata:port_serein`.
    pub settlement: Namespaced,
    /// Its kind of faction.
    pub kind: Namespaced,
    /// Its name.
    pub name: String,
    /// Its colour, `#RRGGBB`.
    pub colour: String,
    /// How far its ground reaches from the settlement's centre, metres.
    pub reach_m: f64,
    /// How far from the settlement every site is the anchor's, whatever its
    /// kind and whoever would hold it otherwise, metres: the home ground a
    /// neighbouring body does not cut into.
    pub core_m: f64,
    /// The buyer whose board is this holder's, if it keeps one.
    #[serde(default)]
    pub buyer: Option<Namespaced>,
    /// Where the household's standing starts, on the scale the deed bands
    /// read: never shown, a score like any deed's sum.
    #[serde(default)]
    pub start: f64,
}

/// The anchors' file.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AnchorsFile {
    /// The authored holders.
    pub anchors: Vec<AnchorDef>,
}

/// The territory dial.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FactionRules {
    /// How close, in strength, the second holder must be to the first for the
    /// ground to read as contested.
    pub contested_margin: f64,
}

/// How a faction holds a rich deposit.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub enum ClaimKind {
    /// Fenced: working it needs a lease from its holder's board.
    Concession,
    /// Open to work, but not past a share in a played day.
    Commons,
    /// Kept: working it at all is a deed against its holder.
    Reserve,
}

/// A lease at a holder's board.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LeaseDef {
    /// Credits it costs.
    pub price: u32,
    /// Played hours it runs.
    pub hours: f64,
}

/// Which deposits are claims (ticket 0012, step 3).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ClaimRules {
    /// The top share of deposits by grade that a holder claims, `0..=1`.
    pub share: f64,
    /// The least strength a holder's ground has at a deposit for it to be a
    /// claim, `0..=1`: the thin edge of a reach is not guarded.
    pub min_strength: f64,
    /// The claim a nature makes; a nature missing here claims nothing.
    pub kinds: Vec<(Nature, ClaimKind)>,
    /// Units a member may take from one commons in a played day before it
    /// counts against the assembly.
    pub commons_share: u32,
    /// How long a played day is, hours.
    pub day_h: f64,
    /// A concession's lease.
    pub lease: LeaseDef,
    /// The countdown of a warning over a post, played seconds.
    pub warn_s: f64,
    /// How near a guard of a concession an F buys a lease, metres.
    pub guard_reach_m: f32,
}

impl ClaimRules {
    /// The claim a body of `nature` makes of a rich deposit.
    #[must_use]
    pub fn kind_of(&self, nature: Nature) -> Option<ClaimKind> {
        self.kinds
            .iter()
            .find(|(each, _)| *each == nature)
            .map(|(_, kind)| *kind)
    }
}

/// What each deed weighs: against a faction negative, for it positive.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DeedWeights {
    /// A unit worked on a concession without a lease.
    pub worked_concession: f64,
    /// A unit worked on a reserve.
    pub worked_reserve: f64,
    /// A unit taken from a commons past its share.
    pub took_past_share: f64,
    /// A harvester planted on a concession without a lease or on a reserve.
    pub planted: f64,
    /// A credit paid at its board.
    pub sold_per_credit: f64,
    /// An outlaw camp cleared near its ground.
    pub cleared_camp: f64,
    /// A posting of its board collected: read with a default so an older file
    /// reads.
    #[serde(default)]
    pub job_done: f64,
    /// A person of it hurt.
    pub hurt: f64,
    /// A person of it killed.
    pub killed: f64,
}

/// Where the stance bands fall on the score.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StanceBands {
    /// At or above this a faction is welcoming.
    pub welcoming_at: f64,
    /// At or below this it is hostile.
    pub hostile_at: f64,
}

/// What a board does to its prices in a stance.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PriceFactor {
    /// What it multiplies the pay of a sale by.
    pub pays: f64,
    /// What it multiplies the price of a purchase by.
    pub sells: f64,
}

/// How deeds weigh, fade and set prices (ticket 0012, step 3).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DeedRules {
    /// What each deed weighs.
    pub weights: DeedWeights,
    /// How near a faction's ground a camp must be for clearing it to count
    /// for that faction, metres.
    pub camp_reach_m: f64,
    /// Played hours in which a deed's weight halves.
    pub half_life_h: f64,
    /// The stance bands.
    pub bands: StanceBands,
    /// A welcoming board's prices.
    pub welcoming: PriceFactor,
    /// A wary board's prices. A hostile board does not trade.
    pub wary: PriceFactor,
}

/// Why a faction file was refused.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum FactionError {
    /// Not valid RON for the file.
    #[error("{0}")]
    Ron(String),
    /// Two files define one kind.
    #[error("`{0}` is defined twice")]
    Duplicate(Namespaced),
    /// A kind or an anchor breaks a rule.
    #[error("`{subject}`: {reason}")]
    Faction {
        /// The kind's id or the anchor's name.
        subject: String,
        /// What is wrong.
        reason: String,
    },
    /// Kinds without the rules file.
    #[error("kinds are defined but `{FACTION_RULES_FILE}` is missing")]
    NoRules,
}

/// A colour `#RRGGBB` as bytes, or `None` for any other spelling.
#[must_use]
pub fn parse_colour(text: &str) -> Option<[u8; 3]> {
    let hex = text.strip_prefix('#')?;
    if hex.len() != 6 || !hex.bytes().all(|b| b.is_ascii_hexdigit()) {
        return None;
    }
    let byte = |i: usize| u8::from_str_radix(&hex[i..i + 2], 16).ok();
    Some([byte(0)?, byte(2)?, byte(4)?])
}

/// Every faction kind, the anchors and the rules, checked.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct FactionSet {
    kinds: Vec<FactionKindDef>,
    anchors: Vec<AnchorDef>,
    rules: Option<FactionRules>,
    claims: Option<ClaimRules>,
    deeds: Option<DeedRules>,
}

impl FactionSet {
    /// Check the files against the site kinds and the settlements.
    ///
    /// # Errors
    /// The path of the first file that breaks a rule, and which.
    pub fn build(
        mut kinds: Vec<(String, FactionKindDef)>,
        anchors: Option<AnchorsFile>,
        rules: Option<FactionRules>,
        claims: Option<ClaimRules>,
        deeds: Option<DeedRules>,
        registries: &GameRegistries,
    ) -> Result<Self, (String, FactionError)> {
        kinds.sort_by(|(_, a), (_, b)| a.id.cmp(&b.id));
        for pair in kinds.windows(2) {
            if pair[0].1.id == pair[1].1.id {
                return Err((
                    pair[1].0.clone(),
                    FactionError::Duplicate(pair[1].1.id.clone()),
                ));
            }
        }
        for (path, def) in &kinds {
            check_kind(def, registries).map_err(|reason| {
                (
                    path.clone(),
                    FactionError::Faction {
                        subject: def.id.to_string(),
                        reason,
                    },
                )
            })?;
        }
        let anchors = anchors.map_or_else(Vec::new, |file| file.anchors);
        let known: BTreeSet<&Namespaced> = kinds.iter().map(|(_, def)| &def.id).collect();
        let mut seen = BTreeSet::new();
        for anchor in &anchors {
            check_anchor(anchor, &known, registries, &mut seen).map_err(|reason| {
                (
                    ANCHORS_FILE.to_owned(),
                    FactionError::Faction {
                        subject: anchor.name.clone(),
                        reason,
                    },
                )
            })?;
        }
        if let Some(rules) = &rules
            && !(rules.contested_margin.is_finite()
                && (0.0..=1.0).contains(&rules.contested_margin))
        {
            return Err((
                FACTION_RULES_FILE.to_owned(),
                FactionError::Faction {
                    subject: "contested_margin".to_owned(),
                    reason: "must be within 0 to 1".to_owned(),
                },
            ));
        }
        if rules.is_none() && !kinds.is_empty() {
            return Err((FACTION_RULES_FILE.to_owned(), FactionError::NoRules));
        }
        let refuse = |path: &str, subject: &str, reason: String| {
            (
                path.to_owned(),
                FactionError::Faction {
                    subject: subject.to_owned(),
                    reason,
                },
            )
        };
        if let Some(claims) = &claims {
            check_claims(claims)
                .map_err(|(subject, reason)| refuse(CLAIM_RULES_FILE, subject, reason))?;
        }
        if let Some(deeds) = &deeds {
            check_deeds(deeds)
                .map_err(|(subject, reason)| refuse(DEED_RULES_FILE, subject, reason))?;
        }
        Ok(Self {
            kinds: kinds.into_iter().map(|(_, def)| def).collect(),
            anchors,
            rules,
            claims,
            deeds,
        })
    }

    /// Check every kind's symbol against the icon library, which loads
    /// after the kinds. An empty library checks nothing, as for items.
    ///
    /// # Errors
    /// The first kind whose symbol the library lacks.
    pub fn check_symbols(&self, icons: &IconCatalogue) -> Result<(), (String, FactionError)> {
        if icons.is_empty() {
            return Ok(());
        }
        for def in &self.kinds {
            if !icons.is_symbol(&def.symbol) {
                return Err((
                    FACTIONS_DIR.to_owned(),
                    FactionError::Faction {
                        subject: def.id.to_string(),
                        reason: format!(
                            "names the symbol `{}`, which the library does not hold as a \
                             tintable symbol",
                            def.symbol
                        ),
                    },
                ));
            }
        }
        Ok(())
    }

    /// Every kind, in id order.
    #[must_use]
    pub fn kinds(&self) -> &[FactionKindDef] {
        &self.kinds
    }

    /// A kind by id.
    #[must_use]
    pub fn kind(&self, id: &Namespaced) -> Option<&FactionKindDef> {
        self.kinds.iter().find(|def| &def.id == id)
    }

    /// The authored holders, in the file's order.
    #[must_use]
    pub fn anchors(&self) -> &[AnchorDef] {
        &self.anchors
    }

    /// The territory dial; `None` for a data set without factions.
    #[must_use]
    pub fn rules(&self) -> Option<&FactionRules> {
        self.rules.as_ref()
    }

    /// Which deposits are claims; `None` for a data set without the file,
    /// where nothing is claimed.
    #[must_use]
    pub fn claims(&self) -> Option<&ClaimRules> {
        self.claims.as_ref()
    }

    /// How deeds weigh and what a stance does to prices; `None` without the
    /// file.
    #[must_use]
    pub fn deeds(&self) -> Option<&DeedRules> {
        self.deeds.as_ref()
    }

    /// The authored holder whose board is `buyer`'s.
    #[must_use]
    pub fn anchor_of_buyer(&self, buyer: &Namespaced) -> Option<&AnchorDef> {
        self.anchors
            .iter()
            .find(|anchor| anchor.buyer.as_ref() == Some(buyer))
    }

    /// Whether the set holds no kind.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.kinds.is_empty()
    }
}

fn check_kind(def: &FactionKindDef, registries: &GameRegistries) -> Result<(), String> {
    if def.name.trim().is_empty() {
        return Err("needs a name".to_owned());
    }
    let mut held = BTreeSet::new();
    for hold in &def.holds {
        if registries.sites.id_of(&hold.kind).is_none() {
            return Err(format!("holds `{}`, which is not a site kind", hold.kind));
        }
        if !held.insert(&hold.kind) {
            return Err(format!("holds `{}` twice", hold.kind));
        }
        if !(hold.weight.is_finite() && hold.weight > 0.0) {
            return Err(format!(
                "holds `{}` at a weight that is not positive",
                hold.kind
            ));
        }
        if !(hold.reach_m.is_finite() && hold.reach_m > 0.0) {
            return Err(format!(
                "holds `{}` to a reach that is not positive",
                hold.kind
            ));
        }
    }
    let per = def.per_mass;
    if !(per.sites_each.is_finite() && per.sites_each > 0.0) {
        return Err("per_mass.sites_each must be positive".to_owned());
    }
    if per.least > per.most {
        return Err("per_mass.least is over per_mass.most".to_owned());
    }
    if def.palette.is_empty() {
        return Err("needs a palette of at least one colour".to_owned());
    }
    if let Some(bad) = def.palette.iter().find(|c| parse_colour(c).is_none()) {
        return Err(format!("palette colour `{bad}` is not `#RRGGBB`"));
    }
    if def.symbol.trim().is_empty() {
        return Err("needs a symbol".to_owned());
    }
    def.names
        .check()
        .map_err(|(field, reason)| format!("{field}: {reason}"))
}

fn check_anchor<'a>(
    anchor: &'a AnchorDef,
    kinds: &BTreeSet<&Namespaced>,
    registries: &GameRegistries,
    seen: &mut BTreeSet<&'a Namespaced>,
) -> Result<(), String> {
    let settlements: &[SettlementDef] = &registries.settlements;
    if anchor.name.trim().is_empty() {
        return Err("needs a name".to_owned());
    }
    if !settlements.iter().any(|s| s.id == anchor.settlement) {
        return Err(format!(
            "holds `{}`, which is not a settlement",
            anchor.settlement
        ));
    }
    if !seen.insert(&anchor.settlement) {
        return Err(format!("`{}` has two holders", anchor.settlement));
    }
    if !kinds.contains(&anchor.kind) {
        return Err(format!(
            "is of `{}`, which is not a faction kind",
            anchor.kind
        ));
    }
    if parse_colour(&anchor.colour).is_none() {
        return Err(format!("colour `{}` is not `#RRGGBB`", anchor.colour));
    }
    if !(anchor.reach_m.is_finite() && anchor.reach_m > 0.0) {
        return Err("reach_m must be positive".to_owned());
    }
    if !(anchor.core_m.is_finite() && (0.0..=anchor.reach_m).contains(&anchor.core_m)) {
        return Err("core_m must lie within 0 and reach_m".to_owned());
    }
    if !anchor.start.is_finite() {
        return Err("start must be a finite score".to_owned());
    }
    if let Some(buyer) = &anchor.buyer
        && registries.buyers.get(buyer).is_none()
    {
        return Err(format!(
            "keeps the board of `{buyer}`, which is not a buyer"
        ));
    }
    Ok(())
}

fn check_claims(claims: &ClaimRules) -> Result<(), (&'static str, String)> {
    if !(claims.share.is_finite() && claims.share > 0.0 && claims.share <= 1.0) {
        return Err(("share", "must be above 0 and at most 1".to_owned()));
    }
    if !(claims.min_strength.is_finite() && (0.0..=1.0).contains(&claims.min_strength)) {
        return Err(("min_strength", "must be within 0 to 1".to_owned()));
    }
    let mut seen = BTreeSet::new();
    for (nature, _) in &claims.kinds {
        if *nature == Nature::Outlaw {
            return Err(("kinds", "an outlaw band makes no claim".to_owned()));
        }
        if !seen.insert(*nature) {
            return Err(("kinds", format!("{nature:?} is named twice")));
        }
    }
    if !(claims.day_h.is_finite() && claims.day_h > 0.0) {
        return Err(("day_h", "must be positive".to_owned()));
    }
    if !(claims.lease.hours.is_finite() && claims.lease.hours > 0.0) {
        return Err(("lease.hours", "must be positive".to_owned()));
    }
    if !(claims.warn_s.is_finite() && claims.warn_s > 0.0) {
        return Err(("warn_s", "must be positive".to_owned()));
    }
    if !(claims.guard_reach_m.is_finite() && claims.guard_reach_m > 0.0) {
        return Err(("guard_reach_m", "must be positive".to_owned()));
    }
    Ok(())
}

fn check_deeds(deeds: &DeedRules) -> Result<(), (&'static str, String)> {
    let w = deeds.weights;
    let weights = [
        w.worked_concession,
        w.worked_reserve,
        w.took_past_share,
        w.planted,
        w.sold_per_credit,
        w.cleared_camp,
        w.job_done,
        w.hurt,
        w.killed,
    ];
    if weights.iter().any(|weight| !weight.is_finite()) {
        return Err(("weights", "must all be finite".to_owned()));
    }
    if !(deeds.camp_reach_m.is_finite() && deeds.camp_reach_m > 0.0) {
        return Err(("camp_reach_m", "must be positive".to_owned()));
    }
    if !(deeds.half_life_h.is_finite() && deeds.half_life_h > 0.0) {
        return Err(("half_life_h", "must be positive".to_owned()));
    }
    let bands = deeds.bands;
    if !(bands.welcoming_at.is_finite()
        && bands.hostile_at.is_finite()
        && bands.hostile_at < bands.welcoming_at)
    {
        return Err(("bands", "hostile_at must lie below welcoming_at".to_owned()));
    }
    for factor in [deeds.welcoming, deeds.wary] {
        if !(factor.pays.is_finite()
            && factor.pays > 0.0
            && factor.sells.is_finite()
            && factor.sells > 0.0)
        {
            return Err(("prices", "factors must be positive".to_owned()));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    #![allow(
        clippy::unwrap_used,
        reason = "a test that unwraps a None fails, which is the point"
    )]

    use super::*;
    use crate::load::{LoadError, load_from_dir, load_from_source};
    use slotted_registry::loader::{AssetSource, DirSource, SourceError};

    const ROOT: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../assets");

    /// Site kind ids are dense in alphabetical order, so a kind added later
    /// must sort after every older one or it shifts theirs (ticket 0012).
    #[test]
    fn the_newer_site_kinds_sort_after_the_older() {
        let registries = load_from_dir(ROOT).expect("assets/ loads");
        let id = |name: &str| registries.sites.id_of(&name.parse().unwrap()).unwrap();
        for old in [
            "town",
            "hamlet",
            "station",
            "landing",
            "pass_shelter",
            "landmark",
        ] {
            assert!(
                id(&format!("strata:{old}")).0 < id("strata:wild_camp").0,
                "{old}"
            );
        }
        assert!(id("strata:wild_camp").0 < id("strata:wreck").0);
        assert!(id("strata:wreck").0 < id("strata:wreck_works").0);
    }

    #[test]
    fn the_shipped_kinds_load() {
        let registries = load_from_dir(ROOT).expect("assets/ loads");
        let set = &registries.factions;
        assert_eq!(set.kinds().len(), 4);
        let natures: Vec<Nature> = set.kinds().iter().map(|k| k.nature).collect();
        for nature in [
            Nature::Corporate,
            Nature::Assembly,
            Nature::Conservation,
            Nature::Outlaw,
        ] {
            assert!(natures.contains(&nature), "{nature:?}");
        }
        let outlaw = set
            .kinds()
            .iter()
            .find(|k| k.nature == Nature::Outlaw)
            .unwrap();
        assert_eq!(outlaw.holds.len(), 1, "the outlaws hold the camps");
        assert_eq!(outlaw.holds[0].kind.to_string(), "strata:wild_camp");
        assert_eq!(set.anchors().len(), 1);
        assert_eq!(set.anchors()[0].name, "Pelagic Works");
        assert_eq!(
            set.anchors()[0].settlement.to_string(),
            "strata:port_serein"
        );
        assert!(set.rules().is_some());
        let palettes: BTreeSet<&String> = set.kinds().iter().flat_map(|k| &k.palette).collect();
        let total: usize = set.kinds().iter().map(|k| k.palette.len()).sum();
        assert_eq!(palettes.len(), total, "no two kinds share a colour");
    }

    /// The shipped files with one text replaced, loaded from memory.
    struct Edited {
        path: &'static str,
        from: &'static str,
        to: &'static str,
    }

    impl AssetSource for Edited {
        fn read(&self, path: &str) -> Result<Vec<u8>, SourceError> {
            let bytes = DirSource::new(std::path::Path::new(ROOT)).read(path)?;
            if path != self.path {
                return Ok(bytes);
            }
            let text = String::from_utf8(bytes).unwrap();
            assert!(text.contains(self.from), "`{}` is in {path}", self.from);
            Ok(text.replace(self.from, self.to).into_bytes())
        }

        fn list(&self, dir: &str) -> Result<Vec<String>, SourceError> {
            DirSource::new(std::path::Path::new(ROOT)).list(dir)
        }
    }

    #[track_caller]
    fn refused(path: &'static str, from: &'static str, to: &'static str, reason: &str) {
        let error = load_from_source(&Edited { path, from, to })
            .expect_err("the edit is refused")
            .to_string();
        assert!(error.contains(reason), "`{error}` should say `{reason}`");
        assert!(matches!(
            load_from_source(&Edited { path, from, to }),
            Err(LoadError::Factions { .. })
        ));
    }

    #[test]
    fn what_names_nothing_is_refused() {
        refused(
            "factions/corporate.faction.ron",
            "strata:landing",
            "strata:nowhere",
            "not a site kind",
        );
        refused(
            "factions/anchors.ron",
            "strata:port_serein",
            "strata:nowhere",
            "not a settlement",
        );
        refused(
            "factions/anchors.ron",
            "kind: \"strata:corporate\"",
            "kind: \"strata:nothing\"",
            "not a faction kind",
        );
    }

    #[test]
    fn an_empty_palette_names_or_a_missing_symbol_is_refused() {
        refused(
            "factions/outlaw.faction.ron",
            "symbol: \"veyra:icon.map.camp\"",
            "symbol: \"veyra:icon.nope\"",
            "does not hold",
        );
        refused(
            "factions/outlaw.faction.ron",
            "palette: [\"#4B1F6B\", \"#8A3A1F\", \"#6A2A8A\", \"#B5502A\", \"#2F1A45\", \"#7A3F5F\"]",
            "palette: []",
            "needs a palette",
        );
        refused(
            "factions/outlaw.faction.ron",
            "patterns: [\"the {root} Band\", \"{root}'s Crew\", \"the {root} Gang\"]",
            "patterns: []",
            "names.patterns",
        );
    }

    #[test]
    fn a_colour_that_is_not_hex_is_refused() {
        refused(
            "factions/anchors.ron",
            "colour: \"#",
            "colour: \"red#",
            "not `#RRGGBB`",
        );
        assert_eq!(parse_colour("#FF8000"), Some([255, 128, 0]));
        assert_eq!(parse_colour("FF8000"), None);
        assert_eq!(parse_colour("#FF80"), None);
    }

    #[test]
    fn the_claims_and_deeds_load_and_a_bad_one_is_refused() {
        let registries = load_from_dir(ROOT).expect("assets/ loads");
        let set = &registries.factions;
        let claims = set.claims().unwrap();
        assert_eq!(
            claims.kind_of(Nature::Corporate),
            Some(ClaimKind::Concession)
        );
        assert_eq!(claims.kind_of(Nature::Outlaw), None);
        let deeds = set.deeds().unwrap();
        assert!(deeds.bands.hostile_at < deeds.bands.welcoming_at);
        let pelagic = &set.anchors()[0];
        assert_eq!(
            set.anchor_of_buyer(pelagic.buyer.as_ref().unwrap())
                .unwrap()
                .name,
            "Pelagic Works"
        );
        refused("factions/claims.ron", "share: 0.2", "share: 0.0", "share");
        refused(
            "factions/claims.ron",
            "(Conservation, Reserve),",
            "(Conservation, Reserve), (Outlaw, Reserve),",
            "outlaw band makes no claim",
        );
        refused(
            "factions/deeds.ron",
            "hostile_at: -10.0",
            "hostile_at: 9.0",
            "hostile_at must lie below",
        );
        refused(
            "factions/anchors.ron",
            "veyra:buyer.pelagic_works",
            "veyra:buyer.nobody",
            "not a buyer",
        );
    }
}
