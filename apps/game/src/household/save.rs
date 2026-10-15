//! The household file: the versioned shapes it has been saved in, the
//! upgrades from each, the copies kept beside it and `Household`'s load,
//! check and save. Separate from the model because it is the one part that
//! changes with every new shape of the file and is read nowhere else.

use super::*;

/// The save as it was before items (version 1), kept so that a played
/// household is read and given the first day's stock, not started over.
mod v1 {
    use super::{CharacterSheet, Deserialize, Weapon, WorldPos};

    #[derive(Deserialize)]
    pub(super) struct Character {
        pub name: String,
        pub sheet: CharacterSheet,
        pub weapon: Weapon,
        pub ammo: u16,
        pub holstered_ammo: u16,
        pub tint: u8,
        pub left_at: Option<WorldPos>,
    }

    #[derive(Deserialize)]
    pub(super) struct Household {
        pub characters: Vec<Character>,
        pub active: usize,
        pub recruits: Vec<Character>,
    }

    /// The version this module reads.
    pub(super) const VERSION: u32 = 1;
}

/// The save as it was before the purse and the survey (version 2), kept so
/// that a played household is read and given the first lots and the purse,
/// not started over.
mod v2 {
    use super::{Character, Deserialize, Record};

    #[derive(Deserialize)]
    pub(super) struct Household {
        pub characters: Vec<Character>,
        pub active: usize,
        pub recruits: Vec<Character>,
        pub store: Vec<Option<Record>>,
    }

    /// The version this module reads.
    pub(super) const VERSION: u32 = 2;
}

/// The save as it was before the bench made anything (version 3), kept so
/// that a played household is read and given the first day's bought parts,
/// not started over.
mod v3 {
    use super::{BTreeSet, Character, DepositId, Deserialize, Record, SurveyedIn};

    #[derive(Deserialize)]
    pub(super) struct Household {
        pub characters: Vec<Character>,
        pub active: usize,
        pub recruits: Vec<Character>,
        pub store: Vec<Option<Record>>,
        pub purse: u32,
        pub visited: BTreeSet<DepositId>,
        pub tracked: Option<DepositId>,
        #[serde(default)]
        pub surveyed_in: Option<SurveyedIn>,
    }

    /// The version this module reads.
    pub(super) const VERSION: u32 = 3;
}

/// The save as it was before blueprints and the fabricator (version 4),
/// kept so that a played household is read whole, with no blueprint and no
/// run, and its file kept beside the new one.
mod v4 {
    use super::{BTreeSet, Character, CraftState, DepositId, Deserialize, Record, SurveyedIn};

    #[derive(Deserialize)]
    pub(super) struct Household {
        pub characters: Vec<Character>,
        pub active: usize,
        pub recruits: Vec<Character>,
        pub store: Vec<Option<Record>>,
        pub purse: u32,
        pub visited: BTreeSet<DepositId>,
        pub tracked: Option<DepositId>,
        #[serde(default)]
        pub surveyed_in: Option<SurveyedIn>,
        #[serde(default)]
        pub crafting: CraftState,
    }

    /// The version this module reads.
    pub(super) const VERSION: u32 = 4;
}

/// Only a file's version, read before anything else so that an older
/// shape is told from a broken file.
#[derive(Deserialize)]
pub(super) struct Versioned {
    pub(super) version: u32,
}

/// The namespace the game's data ids had before the engine was renamed
/// `strata` (ticket 0013), which an old file is read through.
pub(super) const OLD_DATA_NAMESPACE: &str = "scrolls:";

/// What an older file's upgrade came to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct Upgrade {
    /// The shape it was upgraded from (`v1`, `v2`, `v3`, `v4`, or `ids` for
    /// a file of this shape under [`OLD_DATA_NAMESPACE`]).
    pub(super) shape: &'static str,
    /// How many of the first lots found no deposit ([`Grants::short`]).
    pub(super) short: usize,
    /// How many stacks it was owed, first lots and bought parts, found no
    /// cell in a store the player has filled.
    pub(super) left_out: usize,
}

/// Say what of a file's bench was mended to fit the data and keep the file
/// as it was beside itself (`household.ron.mended.bak`), since what was
/// dropped is only there. Whether the copy was made: where it was not, the
/// file must be left alone this run.
pub(super) fn keep_mended(path: &Path, mended: &[String]) -> bool {
    for said in mended {
        warn!("household: the bench: {said}");
    }
    match keep(path, "mended") {
        Ok(kept) => {
            warn!(
                "household: {} did not fit this build's data and its bench was mended; the file as it was is {}",
                path.display(),
                kept.display()
            );
            true
        }
        Err(error) => {
            error!(
                "household: cannot keep a copy of {} ({error}); it will not be written this run",
                path.display()
            );
            false
        }
    }
}

/// What reading the save came to.
pub(super) struct Start {
    /// The household to play.
    pub household: Household,
    /// Whether this is the first day (started, not loaded).
    pub first_day: bool,
    /// The file on disk could not be read or copied aside, so it must not
    /// be written over this run.
    pub hold: bool,
}

/// Copy the file at `path` beside itself as `<name>.<tag>.bak` before it is
/// written over. An earlier copy under that name is never replaced: the
/// same bytes are already kept, and other bytes get the next free number.
///
/// # Errors
/// The copy could not be made, in which case the file must be left alone.
pub(super) fn keep(path: &Path, tag: &str) -> Result<PathBuf, String> {
    let store = strata::sim::store::current();
    let bytes = store
        .read(&key(path))?
        .ok_or_else(|| format!("{}: not found", path.display()))?;
    let name = path.file_name().unwrap_or_default().to_string_lossy();
    let mut copy = 1_u32;
    loop {
        let numbered = if copy == 1 {
            format!("{name}.{tag}.bak")
        } else {
            format!("{name}.{tag}.{copy}.bak")
        };
        let kept = path.with_file_name(numbered);
        // A copy that cannot be read as text (not UTF-8) is different bytes
        // like any other: move on to the next number.
        match store.read(&key(&kept)) {
            Ok(Some(earlier)) if earlier == bytes => return Ok(kept),
            Ok(Some(_)) | Err(_) => copy += 1,
            Ok(None) => {
                store.write(&key(&kept), &bytes)?;
                return Ok(kept);
            }
        }
    }
}

/// A path as the store's key: natively the file itself.
fn key(path: &Path) -> String {
    path.to_string_lossy().into_owned()
}

impl Household {
    /// The file's current shape: 2 since each character has a pack and
    /// weapons and the household a store, 3 since it has a purse and a
    /// survey record, 4 since it has a bench's work and the parts a bench
    /// buys, 5 since it has blueprints and a fabricator's run (ticket
    /// 0007).
    pub const VERSION: u32 = 5;

    /// The first day: the household file's arrival and recruits, or, for
    /// a data set without one, `player.ron` alone under the ranger's name;
    /// the items' first store with `grants`' lots after it, and its purse.
    pub(super) fn starting(registries: &GameRegistries, grants: &Grants) -> Self {
        let (arrival, recruits) = match &registries.sheets.household {
            Some(HouseholdDef {
                arrival, recruits, ..
            }) => (
                Character::fresh(arrival, registries),
                recruits
                    .iter()
                    .map(|m| Character::fresh(m, registries))
                    .collect(),
            ),
            None => (
                Character::fresh(
                    &MemberDef {
                        name: "Frontier ranger".to_owned(),
                        tint: 0,
                        sheet: registries.sheets.player.clone().unwrap_or_default(),
                    },
                    registries,
                ),
                Vec::new(),
            ),
        };
        let mut household = Self {
            version: Self::VERSION,
            characters: vec![arrival],
            active: 0,
            recruits,
            store: starting_store(&registries.items),
            purse: 0,
            visited: BTreeSet::new(),
            found: BTreeMap::new(),
            // The first day's packs carry the tool already.
            survey_tool_given: true,
            tracked: None,
            surveyed_in: None,
            crafting: CraftState::default(),
            blueprints: Blueprints::default(),
            factory: FactoryState::default(),
            procurement: ProcurementState::default(),
            bike: None,
            stable: strata::sim::ride::SavedStable::default(),
            harvesters: Harvesters::default(),
            played: PlayedClock::default(),
            cleared: Cleared::default(),
            deeds: Deeds::default(),
            commons: CommonsTaken::default(),
            leases: Leases::default(),
            caches: Caches::default(),
            jobs: Jobs::default(),
            known: Known::default(),
        };
        household.give(grants, &registries.items);
        household
    }

    /// Give the household `grants`: the purse set, and each lot put in the
    /// store's first empty cell. A lot the store has no cell for is said
    /// and left out, since the store is the player's own by then; how many
    /// were is the answer.
    pub(super) fn give(&mut self, grants: &Grants, items: &ItemCatalogue) -> usize {
        self.purse = grants.purse;
        // The first lots were sampled from their deposits, so the register
        // is not empty on the first day.
        for deposit in &grants.deposits {
            self.found.insert(
                deposit.id,
                Found {
                    reading: Reading::Exact,
                    sampled: true,
                    best_pm: deposit.peak,
                    best_at: deposit.at,
                },
            );
        }
        self.put_in_store(&grants.lots, items)
    }

    /// Give a household saved before a bench made anything the parts the
    /// first day's store has for it: every component and commodity stack
    /// of the starting file, each put in the store's first empty cell. How
    /// many found none is the answer.
    pub(super) fn give_parts(&mut self, items: &ItemCatalogue) -> usize {
        let parts: Vec<Record> = items
            .starting()
            .into_iter()
            .flat_map(|starting| &starting.store)
            .filter(|record| {
                matches!(
                    items.get(&record.item).map(|def| &def.kind),
                    Some(ItemKind::Component | ItemKind::Commodity)
                )
            })
            .cloned()
            .collect();
        self.put_in_store(&parts, items)
    }

    /// Give a household saved before the survey tool every character a tool
    /// of the first day's, in the first empty cell of the pack, unless they
    /// carry one already. How many found no cell is the answer. Once: the
    /// flag is set, whatever the answer.
    pub(super) fn give_survey_tool(&mut self, items: &ItemCatalogue) -> usize {
        self.survey_tool_given = true;
        let Some(tool) = starting_tool(items) else {
            return 0;
        };
        let room = usize::from(items.carry().pack.cells());
        let mut left_out = 0;
        for character in self.characters.iter_mut().chain(&mut self.recruits) {
            let pack = &mut character.pack;
            let carried = pack
                .cells
                .iter()
                .chain(pack.bags.iter().flatten())
                .flatten()
                .any(|record| record.item == tool.item);
            if carried {
                continue;
            }
            let cell = pack.cells.iter().position(Option::is_none).or_else(|| {
                (pack.cells.len() < room).then(|| {
                    pack.cells.push(None);
                    pack.cells.len() - 1
                })
            });
            if let Some(cell) = cell {
                pack.cells[cell] = Some(tool.clone());
            } else {
                warn!(
                    "household: {}'s pack is full; the survey tool is left out",
                    character.name
                );
                left_out += 1;
            }
        }
        left_out
    }

    /// Put each of `stacks` in the store's first empty cell. One the store
    /// has no cell for is said and left out, since the store is the
    /// player's own by then; how many were is the answer.
    pub(super) fn put_in_store(&mut self, stacks: &[Record], items: &ItemCatalogue) -> usize {
        let room = usize::from(items.carry().store.cells());
        let mut left_out = 0;
        for stack in stacks {
            let cell = self.store.iter().position(Option::is_none).or_else(|| {
                (self.store.len() < room).then(|| {
                    self.store.push(None);
                    self.store.len() - 1
                })
            });
            if let Some(cell) = cell {
                self.store[cell] = Some(stack.clone());
            } else {
                warn!(
                    "household: the store is full; {} of `{}` are left out",
                    stack.count, stack.item
                );
                left_out += 1;
            }
        }
        left_out
    }

    /// What is wrong with a parsed file, if anything: another shape, an
    /// empty bench, an `active` off the bench, a sheet the graph refuses
    /// (a box the graph no longer has would silently cost nothing; one
    /// under its gate could never be learned back), or stock the items
    /// refuse (an item that no longer exists, a stack over its size, a
    /// pack past its grid), or a bench's work the schematics refuse (a
    /// session of one that no longer exists). Every index into
    /// `characters[active]` and every stack and session handed to the sim
    /// relies on this having passed. What of the bench's work a retuned
    /// dial or a changed item can put out of line is mended before it is
    /// asked ([`Household::read`]), not refused here.
    pub(super) fn check(&self, registries: &GameRegistries) -> Result<(), String> {
        let graph = &registries.progression;
        let items = &registries.items;
        if self.version != Self::VERSION {
            return Err(format!(
                "version {} is not this build's {}",
                self.version,
                Self::VERSION
            ));
        }
        if self.characters.is_empty() {
            return Err("nobody is on the bench".to_owned());
        }
        if self.active >= self.characters.len() {
            return Err(format!(
                "active {} is off a bench of {}",
                self.active,
                self.characters.len()
            ));
        }
        for c in self.characters.iter().chain(&self.recruits) {
            let def = SheetDef {
                learned: c.sheet.learned.iter().cloned().collect(),
                xp: c.sheet.xp.clone(),
            };
            graph
                .check_sheet(&c.name, &def)
                .map_err(|e| e.to_string())?;
            items
                .check_pack(&c.pack)
                .and_then(|()| items.check_hands(&c.hands))
                .map_err(|e| format!("{}: {e}", c.name))?;
        }
        items
            .check_store(&self.store)
            .map_err(|e| format!("the store: {e}"))?;
        self.stable
            .check(items, &registries.vehicles)
            .map_err(|e| format!("the stable: {e}"))?;
        self.crafting
            .check(registries)
            .map_err(|e| format!("the bench: {e}"))
    }

    /// The household a file's text holds, what its upgrade came to when it
    /// was upgraded, and what of the bench's work was mended to fit the
    /// data (`CraftState::repair`), in words. One from before items has its characters kept
    /// whole and given the first day's pack, weapons and store; that one
    /// and one from before the purse are given `grants`, once, since what
    /// is written afterwards is this build's shape. `grants` is asked only
    /// by those two: it reads the world's deposits, which a later shape has
    /// no use for. One from before the bench made anything is given the
    /// first day's bought parts, as the second is too; the first has them
    /// in the store it is given whole. One from before blueprints is owed
    /// nothing and is read as it is, with no blueprint and no run. What of
    /// the blueprints and the fabricator's run does not fit the data or
    /// the bench is mended with the bench's work
    /// ([`Household::mend_factory`]).
    #[allow(
        clippy::too_many_lines,
        reason = "one arm for each shape the file has been saved in, then what every shape is owed"
    )]
    pub(super) fn read(
        text: &str,
        registries: &GameRegistries,
        grants: &dyn Fn() -> Grants,
    ) -> Result<(Self, Option<Upgrade>, Vec<String>), String> {
        let parse = |error: ron::error::SpannedError| format!("does not parse ({error})");
        // A file written before the engine was renamed (ticket 0013) names
        // its data under the old namespace; it reads as the same file under
        // the new one, and is kept beside as it was like any upgrade.
        let renamed = text.replace(OLD_DATA_NAMESPACE, "strata:");
        let ids_renamed = renamed != text;
        let text = renamed.as_str();
        let version = ron::from_str::<Versioned>(text).map_err(parse)?.version;
        let (mut household, upgraded) = if version == v1::VERSION {
            let old: v1::Household = ron::from_str(text).map_err(parse)?;
            let upgrade = |c: v1::Character| {
                let (pack, hands) = starting_stock(&c.name, &registries.items);
                Character {
                    name: c.name,
                    sheet: c.sheet,
                    weapon: c.weapon,
                    ammo: c.ammo,
                    holstered_ammo: c.holstered_ammo,
                    tint: c.tint,
                    left_at: c.left_at,
                    pack,
                    hands,
                    bar: Bar::default(),
                    injured_until: None,
                }
            };
            let mut household = Self::before_the_purse(
                old.characters.into_iter().map(upgrade).collect(),
                old.active,
                old.recruits.into_iter().map(upgrade).collect(),
                starting_store(&registries.items),
            );
            let grants = grants();
            let left_out = household.give(&grants, &registries.items);
            let upgrade = Upgrade {
                shape: "v1",
                short: grants.short,
                left_out,
            };
            (household, Some(upgrade))
        } else if version == v2::VERSION {
            let old: v2::Household = ron::from_str(text).map_err(parse)?;
            let mut household =
                Self::before_the_purse(old.characters, old.active, old.recruits, old.store);
            let grants = grants();
            let left_out = household.give(&grants, &registries.items)
                + household.give_parts(&registries.items);
            let upgrade = Upgrade {
                shape: "v2",
                short: grants.short,
                left_out,
            };
            (household, Some(upgrade))
        } else if version == v3::VERSION {
            let old: v3::Household = ron::from_str(text).map_err(parse)?;
            let mut household = Self::from_v4(v4::Household {
                characters: old.characters,
                active: old.active,
                recruits: old.recruits,
                store: old.store,
                purse: old.purse,
                visited: old.visited,
                tracked: old.tracked,
                surveyed_in: old.surveyed_in,
                crafting: CraftState::default(),
            });
            let left_out = household.give_parts(&registries.items);
            let upgrade = Upgrade {
                shape: "v3",
                short: 0,
                left_out,
            };
            (household, Some(upgrade))
        } else if version == v4::VERSION {
            // Nothing is owed: that shape lacked only what nobody had yet.
            let household = Self::from_v4(ron::from_str(text).map_err(parse)?);
            let upgrade = Upgrade {
                shape: "v4",
                short: 0,
                left_out: 0,
            };
            (household, Some(upgrade))
        } else if version == Self::VERSION {
            (ron::from_str(text).map_err(parse)?, None)
        } else {
            return Err(format!(
                "version {version} is not this build's {}",
                Self::VERSION
            ));
        };
        let mut upgraded = upgraded;
        if ids_renamed && upgraded.is_none() {
            upgraded = Some(Upgrade {
                shape: "ids",
                short: 0,
                left_out: 0,
            });
        }
        // A file saved before the survey tool is given one, once, whatever
        // its shape; the upgrade is kept beside it like any other.
        if !household.survey_tool_given {
            let left_out = household.give_survey_tool(&registries.items);
            upgraded = Some(match upgraded {
                Some(was) => Upgrade {
                    left_out: was.left_out + left_out,
                    ..was
                },
                None => Upgrade {
                    shape: "v5",
                    short: 0,
                    left_out,
                },
            });
        }
        // The bike of a file from before it was an item is a store grade
        // one, stabled.
        if let Some(vehicle) = household.bike.take()
            && household.stable.bike.is_none()
        {
            household.stable.bike =
                strata::sim::ride::store_grade_bike(&registries.items, &vehicle);
        }
        // An edit to the data since the file was written (fewer points a
        // session, an item's lines) is no fault of the file's: the bench's
        // work is held to the data as it now is, and only what cannot be
        // worked on is refused.
        let mut mended = household.crafting.repair(registries);
        mended.extend(household.mend_factory(registries));
        mended.extend(household.procurement.repair(registries));
        household
            .check(registries)
            .map_err(|reason| format!("is refused ({reason})"))?;
        Ok((household, upgraded, mended))
    }

    /// The saved household at `path`, or the first day when there is none
    /// or `fresh` asks to ignore it. A file that does not parse or
    /// [`Household::check`] is a warning and the first day, never a crash
    /// at startup, and never a loss: it is copied aside first
    /// (`household.ron.refused.bak`), as a file of an older shape is
    /// (`household.ron.v1.bak`, `household.ron.v2.bak`,
    /// `household.ron.v3.bak`, `household.ron.v4.bak`) before its upgrade
    /// is written, and one whose
    /// bench was mended to fit the data (`household.ron.mended.bak`). Where no copy can be made, [`Start::hold`] says to leave
    /// the file alone, and so it does for an upgrade that could not give
    /// every first lot the starting file names (a world with no deposit of
    /// one in range): the file stays in its older shape, and a later start
    /// that finds them finishes it.
    ///
    /// `grants` is the first day's lots and purse, asked only on a first
    /// day and by an upgrade.
    pub(super) fn load_or_start(
        path: &Path,
        fresh: bool,
        registries: &GameRegistries,
        grants: &dyn Fn() -> Grants,
    ) -> Start {
        let mut hold = false;
        if !fresh {
            match strata::sim::store::current().read(&key(path)) {
                Ok(Some(text)) => match Self::read(&text, registries, grants) {
                    Ok((household, upgraded, mended)) => {
                        info!(
                            "household: loaded {} from {} ({} on the bench, {} to recruit)",
                            household.active().name,
                            path.display(),
                            household.characters.len(),
                            household.recruits.len()
                        );
                        match upgraded {
                            Some(Upgrade { shape, short, .. }) if short > 0 => {
                                error!(
                                    "household: {} was written in an older shape ({shape}) and {short} of the first lots it is owed found no deposit in this world; it is left as it is and not written this run, so a later start can finish it",
                                    path.display()
                                );
                                hold = true;
                            }
                            Some(Upgrade {
                                shape, left_out, ..
                            }) => match keep(path, shape) {
                                Ok(kept) => {
                                    info!(
                                        "household: {} was written in an older shape ({shape}); it is kept as {} and the household has what that shape lacked",
                                        path.display(),
                                        kept.display()
                                    );
                                    if left_out > 0 {
                                        warn!(
                                            "household: its store had no cell for {left_out} of the first lots and bought parts, which are left out; the file as it was is {}",
                                            kept.display()
                                        );
                                    }
                                }
                                Err(error) => {
                                    error!(
                                        "household: cannot keep a copy of {} ({error}); it will not be written this run",
                                        path.display()
                                    );
                                    hold = true;
                                }
                            },
                            None => {}
                        }
                        if !mended.is_empty() && !keep_mended(path, &mended) {
                            hold = true;
                        }
                        return Start {
                            household,
                            first_day: false,
                            hold,
                        };
                    }
                    Err(reason) => match keep(path, "refused") {
                        Ok(kept) => warn!(
                            "household: {} {reason}; it is kept as {} and the first day starts over",
                            path.display(),
                            kept.display()
                        ),
                        Err(error) => {
                            error!(
                                "household: {} {reason} and cannot be copied aside ({error}); starting over without writing it",
                                path.display()
                            );
                            hold = true;
                        }
                    },
                },
                Ok(None) => {}
                Err(error) => {
                    warn!(
                        "household: cannot read {} ({error}); starting over without writing it",
                        path.display()
                    );
                    hold = true;
                }
            }
        }
        let household = Self::starting(registries, &grants());
        info!(
            "household: the first day, {} with {} to recruit",
            household.active().name,
            household.recruits.len()
        );
        Start {
            household,
            first_day: true,
            hold,
        }
    }

    /// A household of a shape before the purse, in this build's: with no
    /// credits, no survey record and nothing made, each of which the
    /// upgrade then gives or leaves empty.
    pub(super) fn before_the_purse(
        characters: Vec<Character>,
        active: usize,
        recruits: Vec<Character>,
        store: Vec<Option<Record>>,
    ) -> Self {
        Self::from_v4(v4::Household {
            characters,
            active,
            recruits,
            store,
            purse: 0,
            visited: BTreeSet::new(),
            tracked: None,
            surveyed_in: None,
            crafting: CraftState::default(),
        })
    }

    /// A household of the shape before blueprints, in this build's: with
    /// no blueprint and no run.
    fn from_v4(old: v4::Household) -> Self {
        Self {
            version: Self::VERSION,
            characters: old.characters,
            active: old.active,
            recruits: old.recruits,
            store: old.store,
            purse: old.purse,
            visited: old.visited,
            found: BTreeMap::new(),
            survey_tool_given: false,
            tracked: old.tracked,
            surveyed_in: old.surveyed_in,
            crafting: old.crafting,
            blueprints: Blueprints::default(),
            factory: FactoryState::default(),
            procurement: ProcurementState::default(),
            bike: None,
            stable: strata::sim::ride::SavedStable::default(),
            harvesters: Harvesters::default(),
            played: PlayedClock::default(),
            cleared: Cleared::default(),
            deeds: Deeds::default(),
            commons: CommonsTaken::default(),
            leases: Leases::default(),
            caches: Caches::default(),
            jobs: Jobs::default(),
            known: Known::default(),
        }
    }

    /// Hold the blueprints and the fabricator's run to the data and to the
    /// bench, as [`Household::read`] holds the bench's work: what
    /// `Blueprints::repair` and `FactoryState::repair` mend, a dropped
    /// run's inputs going back to the store; and a run's worker who is the
    /// one out, or is nobody on the bench, is taken off it, since only a
    /// benched member works one and the run goes on unattended. In words,
    /// one for each thing changed.
    pub(super) fn mend_factory(&mut self, registries: &GameRegistries) -> Vec<String> {
        let mut said = self.blueprints.repair(registries);
        // A store the items refuse is the check's to say; nothing can be
        // put in it either way.
        if self.factory.job.is_some()
            && let Ok(mut store) = HouseholdStore::from_saved(&self.store, &registries.items)
        {
            let before = said.len();
            said.extend(
                self.factory
                    .repair(registries, &self.blueprints, &mut store),
            );
            if said.len() > before
                && let Ok(saved) = store.saved(&registries.items)
            {
                self.store = saved;
            }
        }
        let out = self.characters.get(self.active).map(|c| c.name.clone());
        if let Some(job) = self.factory.job.as_mut()
            && let Some(worker) = job.worker.clone()
        {
            let benched = self.characters.iter().any(|c| c.name == worker);
            if !benched || out.as_deref() == Some(worker.as_str()) {
                said.push(format!(
                    "{worker} was working the fabricator and is {}; nobody is working it now",
                    if benched { "out" } else { "not on the bench" }
                ));
                job.worker = None;
            }
        }
        said
    }

    /// Write the household to `path`, making its directory.
    ///
    /// # Errors
    /// The file could not be written.
    pub(super) fn save(&self, path: &Path) -> Result<(), String> {
        let text = ron::ser::to_string_pretty(self, ron::ser::PrettyConfig::default())
            .map_err(|e| e.to_string())?;
        strata::sim::store::current().write(&key(path), &text)
    }

    /// Who is out. `active` is on the bench by [`Household::check`] at
    /// load and by every write to it since.
    pub(crate) fn active(&self) -> &Character {
        &self.characters[self.active]
    }

    /// What the local player spawns with: the active character's actor,
    /// sheet, name and tint, and what they carry and hold.
    pub(crate) fn stats(
        &self,
        registries: &GameRegistries,
    ) -> (Combatant, CharacterSheet, Name, BodyTint, Pack, Hands) {
        let character = self.active();
        let (pack, hands) = character.carried(&registries.items);
        (
            character.actor(&registries.progression, &registries.items),
            character.sheet.clone(),
            Name::new(character.name.clone()),
            BodyTint(character.tint),
            pack,
            hands,
        )
    }

    /// The active character's bar, which the player spawns with.
    pub(crate) fn bar(&self) -> Bar {
        self.active().bar.clone()
    }

    /// `--grant`: the abilities `wanted` are known to whoever starts out
    /// and slotted where the sim puts a newly known ability, before the
    /// player spawns. There is no box that grants most of them yet, so the
    /// bar keeps them as granted by hand.
    ///
    /// # Errors
    /// An ability that is no row of the table; nothing changes.
    pub(super) fn grant_at_start(
        &mut self,
        wanted: &[String],
        registries: &GameRegistries,
    ) -> Result<(), String> {
        let tables = strata::sim::combat_tables::current();
        if let Some(unknown) = wanted.iter().find(|id| tables.abilities.row(id).is_none()) {
            return Err(format!("`{unknown}` is no ability"));
        }
        let character = self
            .characters
            .get_mut(self.active)
            .ok_or("nobody on the bench")?;
        character.bar.granted.extend(wanted.iter().cloned());
        let known = strata::sim::ability::known_to(
            &registries.progression,
            &character.sheet,
            &character.bar,
        );
        character.bar.follow(&tables.abilities, &known);
        info!("household: {} starts knowing {wanted:?}", character.name);
        Ok(())
    }

    /// `--fitted`: fit a bought example of the module `id` to the weapon of
    /// the active character it is made for, before the player spawns. It
    /// is the transaction a workbench runs (`fitting::fit`) with the store
    /// to hand, so the unit comes out of the stock. A part the store holds
    /// none of is stood there at store grade first (the melee parts are
    /// made, never sold), so a still can show one.
    ///
    /// # Errors
    /// No such module, no weapon in hand that takes it, or the sim's
    /// refusal: none in stock, a shut mount.
    pub(super) fn fit_at_start(
        &mut self,
        id: &str,
        registries: &GameRegistries,
    ) -> Result<(), String> {
        let items = &registries.items;
        let (module, mount) = bought_module(items, id).ok_or("no such module")?;
        let character = &self.characters[self.active];
        let fits = |record: &Option<Record>| {
            record.as_ref().is_some_and(|record| {
                matches!(
                    items.get(&module.item).map(|def| &def.kind),
                    Some(ItemKind::Module { fits, .. }) if fits.contains(&record.item)
                )
            })
        };
        let hand = character
            .hands
            .iter()
            .position(fits)
            .ok_or("no weapon in hand takes it")?;
        let (mut pack, mut hands) = character.carried(items);
        let mut store =
            HouseholdStore::from_saved(&self.store, items).map_err(|error| error.to_string())?;
        // A part no store stocks (the melee ones are made, none is sold) is
        // stood in the store at store grade first, so a still can show it.
        let record = Record {
            item: module.item.clone(),
            count: 1,
            batch: 0,
            by: None,
            lines: module.lines.clone(),
            condition: None,
            fitted: BTreeMap::new(),
            origin: None,
        };
        if let Ok(unit) = items::write(&record, items) {
            let held = (0..store.0.len()).any(|cell| {
                store
                    .0
                    .get(cell)
                    .is_some_and(|present| present.same_kind(&unit))
            });
            if !held && let Some(cell) = store.0.first_empty() {
                store.0.set(cell, Some(unit));
            }
        }
        let mut stock = Stock {
            saddle: None,
            pack: &mut pack,
            hands: &mut hands,
            store: Some(&mut store),
        };
        fitting::fit(items, &mut stock, hand, &mount, &module).map_err(|why| why.to_string())?;
        let stock = character
            .stock_of(&pack, &hands, items)
            .ok_or("the stock does not read")?;
        let store = store.saved(items).map_err(|error| error.to_string())?;
        let character = &mut self.characters[self.active];
        (character.pack, character.hands) = stock;
        self.store = store;
        info!("household: {id} is fitted to {}'s weapon", character.name);
        Ok(())
    }

    /// `--learn`: the boxes `wanted` are on the sheet of whoever starts
    /// out, past their prerequisites and at no cost, before the player
    /// spawns.
    ///
    /// # Errors
    /// A box the graph does not have; nothing changes.
    pub(super) fn learn_at_start(
        &mut self,
        wanted: &[String],
        registries: &GameRegistries,
    ) -> Result<(), String> {
        if let Some(unknown) = wanted
            .iter()
            .find(|id| registries.progression.find(id).is_none())
        {
            return Err(format!("`{unknown}` is no box"));
        }
        let character = self
            .characters
            .get_mut(self.active)
            .ok_or("nobody on the bench")?;
        character.sheet.learned.extend(wanted.iter().cloned());
        info!("household: {} starts knowing {wanted:?}", character.name);
        Ok(())
    }

    /// `--in-hand`: store grade weapons of `wanted` (an item id each, `-`
    /// leaving the slot be) in the slots of whoever starts out, before the
    /// player spawns. A weapon that leaves goes to a free place in the
    /// household's store, so a scripted run on a played file keeps it; with
    /// the store full it is dropped.
    ///
    /// # Errors
    /// An item that is no weapon, a character the boxes do not certify on
    /// it, or two of a family; nothing changes.
    pub(super) fn in_hand_at_start(
        &mut self,
        wanted: &[String],
        registries: &GameRegistries,
    ) -> Result<(), String> {
        let items = &registries.items;
        let character = self
            .characters
            .get(self.active)
            .ok_or("nobody on the bench")?;
        let mut hands = character.hands.clone();
        let mut displaced = Vec::new();
        let effective = strata::sim::progression::apply(
            &strata::sim::combat_tables::current(),
            &registries.progression,
            &character.sheet,
        );
        for (slot, id) in wanted.iter().enumerate().take(HANDS) {
            if id == "-" {
                continue;
            }
            let id = strata::registry::Namespaced::parse(id).map_err(|e| e.to_string())?;
            let def = items.get(&id).ok_or_else(|| format!("no item `{id}`"))?;
            let ItemKind::Weapon { family, .. } = &def.kind else {
                return Err(format!("`{id}` is no weapon"));
            };
            let weapon =
                Weapon::from_name(family).ok_or_else(|| format!("`{id}` has no family"))?;
            if !effective.certified.contains(&weapon) {
                return Err(format!(
                    "{} is not certified on the {}; a member who has learned its box (the blade novice for the blade, Greatblade 1 for the greatblade, Glaive 1 for the glaive) is needed: start with --as",
                    character.name,
                    weapon.name().to_lowercase()
                ));
            }
            displaced.extend(hands[slot].take());
            hands[slot] = Some(strata::sim::crafting::bought(def));
        }
        items
            .check_hands(&hands)
            .map_err(|error| error.to_string())?;
        for record in displaced {
            if let Some(place) = self.store.iter_mut().find(|place| place.is_none()) {
                *place = Some(record);
            } else {
                warn!("household: the store is full, a displaced weapon is dropped");
            }
        }
        let character = &mut self.characters[self.active];
        character.hands = hands;
        info!(
            "household: {} starts with the weapons asked for",
            character.name
        );
        Ok(())
    }

    /// `--condition`: the carbine of the active character, the weapon in
    /// hand at the start, worn down to `condition`. It edits the saved
    /// record before the player spawns, when no stack exists to write
    /// through.
    ///
    /// # Errors
    /// No weapon in the first hand or a record the catalogue refuses.
    pub(super) fn wear_at_start(
        &mut self,
        condition: u16,
        registries: &GameRegistries,
    ) -> Result<(), String> {
        let items = &registries.items;
        let character = &mut self.characters[self.active];
        let record = character
            .hands
            .first_mut()
            .and_then(Option::as_mut)
            .filter(|record| record.condition.is_some())
            .ok_or("no weapon in the first hand")?;
        let condition = condition.min(items.wear().full);
        record.condition = Some(condition);
        info!("household: {}'s weapon is at {condition}", character.name);
        Ok(())
    }

    /// The first recruit joins the bench, if there is room; the joined
    /// character's index.
    pub(super) fn join_next(&mut self) -> Option<usize> {
        if self.characters.len() >= BENCH || self.recruits.is_empty() {
            return None;
        }
        self.characters.push(self.recruits.remove(0));
        Some(self.characters.len() - 1)
    }

    /// `--as`: the recruit called `name` joins the bench and is out, before
    /// the player spawns.
    ///
    /// # Errors
    /// No such recruit, or a bench with no room.
    pub(super) fn join_named_at_start(&mut self, name: &str) -> Result<(), String> {
        if let Some(index) = self.characters.iter().position(|c| c.name == name) {
            self.active = index;
            return Ok(());
        }
        if self.characters.len() >= BENCH {
            return Err("the bench is full".to_owned());
        }
        let at = self
            .recruits
            .iter()
            .position(|c| c.name == name)
            .ok_or_else(|| format!("no recruit called {name}"))?;
        self.characters.push(self.recruits.remove(at));
        self.active = self.characters.len() - 1;
        info!("household: {name} joined the bench and is out");
        Ok(())
    }

    /// `--crafted-carbine`: a patrol carbine made by the household's smith
    /// (batch 7, `moddability` on that line and 650 on the rest, whole)
    /// goes in the arrival's pack, or with `in_hand` into the hand, the
    /// bought one into the pack in its place. It edits the saved stock
    /// before the player spawns, when no stack exists to write through.
    ///
    /// # Errors
    /// A catalogue without the carbine, or a stock that does not read.
    pub(super) fn crafted_carbine_at_start(
        &mut self,
        moddability: u16,
        in_hand: bool,
        registries: &GameRegistries,
    ) -> Result<(), String> {
        let items = &registries.items;
        let id = strata::registry::Namespaced::parse("veyra:weapon.patrol_carbine")
            .map_err(|e| e.to_string())?;
        let def = items.get(&id).ok_or("no patrol carbine in the catalogue")?;
        let record = Record {
            item: id,
            count: 1,
            batch: 7,
            by: Some("Tovan Reyes".to_owned()),
            lines: def
                .lines
                .iter()
                .map(|line| {
                    let figure = if line.id == "moddability" {
                        moddability
                    } else {
                        650
                    };
                    (line.id.clone(), figure)
                })
                .collect(),
            condition: Some(items.wear().full),
            fitted: BTreeMap::new(),
            origin: None,
        };
        // Whoever starts out: the arrival, or with `--as` the recruit, whose
        // sheet is the one that can rework it.
        let arrival = self
            .characters
            .get_mut(self.active)
            .ok_or("nobody on the bench")?;
        let (mut pack, mut hands) = arrival.carried(items);
        let made = items::write(&record, items).map_err(|e| e.to_string())?;
        let made = if in_hand {
            let old = hands.0.get(0).cloned();
            hands.0.set(0, Some(made));
            old
        } else {
            Some(made)
        };
        if let Some(stack) = made
            && items::place_in(items, &mut pack.cells, stack).is_some()
        {
            return Err("no room in the pack".to_owned());
        }
        pack.cells.clear_changed();
        hands.0.clear_changed();
        let stock = arrival
            .stock_of(&pack, &hands, items)
            .ok_or("the stock does not read")?;
        (arrival.pack, arrival.hands) = stock;
        info!(
            "household: a carbine made by Tovan Reyes is with {}",
            arrival.name
        );
        Ok(())
    }

    /// `--blanks`: `count` store-grade mount blanks go in the pack of the
    /// character who starts out, before the player spawns.
    ///
    /// # Errors
    /// A catalogue without the blank, or no room in the pack.
    pub(super) fn blanks_at_start(
        &mut self,
        count: u32,
        registries: &GameRegistries,
    ) -> Result<(), String> {
        let items = &registries.items;
        let rules = registries.rework.as_ref().ok_or("no rework data")?;
        let def = items
            .get(&rules.blank)
            .ok_or("no mount blank in the catalogue")?;
        let record = Record {
            item: rules.blank.clone(),
            count,
            batch: 0,
            by: None,
            lines: def
                .lines
                .iter()
                .map(|line| (line.id.clone(), strata::registry::item::STORE_GRADE))
                .collect(),
            condition: None,
            fitted: BTreeMap::new(),
            origin: None,
        };
        let who = &mut self.characters[self.active];
        let (mut pack, hands) = who.carried(items);
        let blanks = items::write(&record, items).map_err(|e| e.to_string())?;
        if items::place_in(items, &mut pack.cells, blanks).is_some() {
            return Err("no room in the pack".to_owned());
        }
        pack.cells.clear_changed();
        let kept = who
            .stock_of(&pack, &hands, items)
            .ok_or("the stock does not read")?;
        (who.pack, who.hands) = kept;
        info!("household: {count} mount blanks are with {}", who.name);
        Ok(())
    }

    /// `--give`, for a still: one of each item in the pack of the character
    /// who starts out. A lot is the store's own copy (capped at twenty
    /// units) where the store holds one, since a lot is made from a
    /// deposit; anything else is a store grade example.
    ///
    /// # Errors
    /// An id that is no item, or a pack with no room for them all.
    pub(super) fn give_at_start(
        &mut self,
        wanted: &[String],
        registries: &GameRegistries,
    ) -> Result<(), String> {
        let items = &registries.items;
        let who = self
            .characters
            .get(self.active)
            .ok_or("nobody on the bench")?;
        let (mut pack, hands) = who.carried(items);
        for id in wanted {
            let id = strata::registry::Namespaced::parse(id).map_err(|e| e.to_string())?;
            let def = items.get(&id).ok_or_else(|| format!("no item `{id}`"))?;
            let stored = self.store.iter().flatten().find(|record| record.item == id);
            let record = match stored {
                Some(record) if matches!(def.kind, ItemKind::Lot { .. }) => Record {
                    count: record.count.min(20),
                    ..record.clone()
                },
                // A blueprint is a recording and never bought: a stand-in
                // number whose body the ledger has not got is enough to
                // draw its cell.
                _ if matches!(def.kind, ItemKind::Blueprint) => Record {
                    batch: 1,
                    by: Some(who.name.clone()),
                    ..strata::sim::crafting::bought(def)
                },
                _ => strata::sim::crafting::bought(def),
            };
            let stack = items::write(&record, items).map_err(|e| e.to_string())?;
            if items::place_in(items, &mut pack.cells, stack).is_some() {
                return Err(format!("no room in the pack for `{id}`"));
            }
        }
        pack.cells.clear_changed();
        let who = &mut self.characters[self.active];
        let kept = who
            .stock_of(&pack, &hands, items)
            .ok_or("the stock does not read")?;
        (who.pack, who.hands) = kept;
        info!(
            "household: {} starts with {} given items",
            who.name,
            wanted.len()
        );
        Ok(())
    }

    /// What the command line asks of the ride, the purse and the saddle for
    /// a still: a store grade bike for a household with none, the bike worn
    /// to a condition, the purse set, three lots and a part in the saddle.
    pub(super) fn ride_at_start(&mut self, file: &HouseholdFile, registries: &GameRegistries) {
        // A bike put away in the pack or the store is the household's: no
        // second one is written into a save that already owns one.
        let owns = {
            let is_bike = |record: &Option<Record>| {
                record.as_ref().is_some_and(|record| {
                    matches!(
                        registries.items.get(&record.item).map(|def| &def.kind),
                        Some(strata::registry::item::ItemKind::Vehicle { .. })
                    )
                })
            };
            let pack = &self.active().pack;
            pack.cells.iter().any(is_bike)
                || pack.bags.iter().flatten().any(is_bike)
                || self.store.iter().any(is_bike)
        };
        if file.bike
            && !owns
            && self.stable.bike.is_none()
            && let Some(bike) = registries.vehicles.iter().find_map(|vehicle| {
                strata::sim::ride::store_grade_bike(&registries.items, &vehicle.id)
            })
        {
            self.stable.bike = Some(bike);
        }
        if let Some(condition) = file.bike_condition
            && let Some(bike) = self.stable.bike.as_mut()
        {
            bike.condition = Some(condition.min(strata::registry::item::QUALITY_MAX));
        }
        if let Some(purse) = file.purse {
            self.purse = purse;
        }
        if file.saddle_stock
            && let Err(reason) = self.saddle_stock_at_start(registries)
        {
            error!("household: --saddle-stock: {reason}");
        }
    }

    /// `--saddle-stock`, for a still: three lots and a part copied from the
    /// store into the saddle bags, the lots at most twenty a stack. The
    /// store keeps its own.
    ///
    /// # Errors
    /// No bike, or a store with no lots in it.
    pub(super) fn saddle_stock_at_start(
        &mut self,
        registries: &GameRegistries,
    ) -> Result<(), String> {
        let items = &registries.items;
        if self.stable.bike.is_none() {
            return Err("the household has no bike".to_owned());
        }
        let kind = |record: &Record| items.get(&record.item).map(|def| &def.kind);
        let stored: Vec<&Record> = self.store.iter().flatten().collect();
        let lots = stored
            .iter()
            .filter(|record| matches!(kind(record), Some(ItemKind::Lot { .. })))
            .take(3);
        let part = stored.iter().find(|record| {
            matches!(
                kind(record),
                Some(ItemKind::Component | ItemKind::Module { .. })
            )
        });
        let mut put: Vec<Record> = lots
            .map(|record| Record {
                count: record.count.min(20),
                ..(*record).clone()
            })
            .collect();
        put.extend(part.map(|record| (*record).clone()));
        if put.is_empty() {
            return Err("the store has nothing to copy".to_owned());
        }
        self.stable.saddle = put.into_iter().map(Some).collect();
        info!(
            "household: {} stacks are in the saddle",
            self.stable.saddle.len()
        );
        Ok(())
    }

    /// The index of the character whose sheet has `id` learned.
    pub(super) fn index_with(&self, id: &str) -> Option<usize> {
        self.characters.iter().position(|c| c.sheet.has(id))
    }
}
