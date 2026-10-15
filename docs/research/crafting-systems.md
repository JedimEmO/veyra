# Crafting systems: Star Wars Galaxies and what came after

Research note, 2026-10-10, written for [the crafting proposal](../design/crafting.md).
A mechanics-level reference on how Star Wars Galaxies (SWG) handled resources,
crafting and professions, with shorter notes on eight other games and a
table of the design levers they expose. Web research, not played
verification. Where sources disagree the disagreement is recorded, and the
last section lists what could not be confirmed.

## Sources and how far to trust them

Reused sources are cited by tag; single-use sources are linked inline.

- **[LUN-SHIFT]** Lunariel, "Guide to the Resource Shift", SWGCraft forums,
  Nov 2004, updated Feb 2005 (Patch 13.1). Contemporaneous player research.
  <https://www.swgemu.com/archive/scrapbookv51/data/20070126153930> (blocked
  to plain fetches; readable through the Wayback Machine)
- **[LUN-EXP]** Lunariel, "Experimentation Formula", Oct 2004.
  <https://www.swgemu.com/archive/scrapbookv51/data/20070204175514>
- **[ESTEI]** "Estei's Guide to experimentation", official Armorsmith forum,
  before the NGE. <https://www.swgemu.com/archive/scrapbookv51/data/20070204174832>
- **[FAQ]** Wuu Sarin, "The Ultimate Resource FAQ (updated to Pub 7)", early
  2004. <https://www.swgemu.com/archive/scrapbookv51/data/20070210182106>
- **[SOE-SCRIPT]** SOE server script `craftinglib.java`, NGE era, from the
  public SWG-Source mirror of the leaked code.
  <https://github.com/SWG-Source/dsrc/blob/master/sku.0/sys.server/compiled/game/script/library/craftinglib.java>
- **[SOE-TREE]** `resource_tree.tab`, same mirror, NGE era (includes
  Kashyyyk, Mustafar, recycled and space resources).
  <https://github.com/SWG-Source/dsrc/blob/master/sku.0/sys.shared/compiled/game/datatables/resource/resource_tree.tab>
- **[EMU]** SWGEmu Core3, a reconstruction of the game before the Combat
  Upgrade, not SOE code.
  <https://github.com/swgemu/Core3/tree/unstable/MMOCoreORB> (files named inline)
- **[WIKI]** swg.fandom.com, mostly NGE era.
- **[K1]** Raph Koster, "Designing a Living Society in SWG, part one" (2015).
  <https://www.raphkoster.com/?p=25717>
- **[K-FAIL]** Koster, "Did Star Wars Galaxies Fail?" (2015).
  <https://www.raphkoster.com/?p=25829>
- **[K-AH]** Koster, "Do auction houses suck?". <https://www.raphkoster.com/?p=4080>

The strongest cross-check here: Lunariel's 2004 empirical experimentation
formula matches the SOE script term for term (section 5). The core crafting
maths survived from launch into the NGE, and [SOE-SCRIPT] can be read as
close to authoritative for it.

"Pre-CU" is the game before the Combat Upgrade of April 2005. "NGE" is the
New Game Enhancements of November 2005, which replaced the professions with
classes.

## 1. Resource taxonomy

- **Shape.** Root, then Organic, Inorganic and Energy (plus Space Resource
  after Jump to Lightspeed, JTL). Up to 8 levels deep. Example chain:
  Inorganic > Mineral > Metal > Ferrous Metal > Steel > Duralloy Steel > a
  named spawn. [SOE-TREE]
- **Size.** The NGE-era table has 845 nodes and 738 leaves. Of those, 549
  are planet-specific (45 per planet for 11 planets, 54 for Mustafar), 31 are
  recycled classes and 13 are space classes. [SOE-TREE, parsed for this note]
- **Pre-CU size.** 10 planets with 42 native classes each (38 organic,
  fiberplast, water vapor, wind, solar) gives 420, plus roughly 145
  galaxy-wide leaf classes. [LUN-SHIFT; EMU `resource_manager.lua`]
- **Disagreement.** Koster gives "seventy-nine different subclasses ... a
  grand total of 474 distinct types", some never used [K1]. That matches
  neither count above and may be a launch-era figure.
- **Mineral families.** Steel has 10 ordinary subtypes plus 2 from JTL. Iron
  has 8, copper 9, aluminum 6. There are also three ore families, two
  gemstone families, radioactive classes 1 to 7, and solid and liquid petro
  fuel classes 1 to 7. [SOE-TREE]
- **Planet classes.** "Corellian Fiberplast", "Corellian Wooly Hide" and the
  like are ordinary leaves restricted to one zone. Each planet always has
  exactly one live spawn of each. [LUN-SHIFT]
- **Slot breadth.** A schematic slot names any node in the tree, and any
  descendant spawn qualifies. The T21 Rifle mixes specific and broad slots:
  150 Ditanium Steel, 85 Polonium Iron, 20 Metal, 15 Gallinorian crystalline
  gem, 30 Alantium Carbonate Ore, 10 identical power handlers, 1 barrel, 1
  optional stock. [EMU `rifle_t21.lua`]
- "Generic, simple items might only require ... 10 units of 'Inorganic' ...
  Power Hammers ... not only require Iron, but Kammris Iron." [FAQ]

## 2. Resource stats

- **The 11 attributes** [WIKI; SOE-TREE]: OQ Overall Quality, CR Cold
  Resistance, CD Conductivity, DR Decay Resistance, FL Flavor, HR Heat
  Resistance, MA Malleability, PE Potential Energy, SR Shock Resistance, UT
  Unit Toughness, ER Entangle Resistance.
- **No intrinsic meaning.** The names only matter through schematic weights.
  "Resources with low Decay Resistance do not rot." The one exception is PE
  on energy: a PE 1000 unit counts as two PE 500 units. [FAQ]
- **Range.** 1 to 1000. Every unit of a named spawn is identical everywhere.
  [FAQ]
- **Which stats a class has** [SOE-TREE]:
  - Metals: DR, OQ, CR, HR, MA, UT, SR, CD.
  - Ores: the same without CD.
  - Gemstones: add ER.
  - Hide and bone: DR, OQ, MA, UT, SR.
  - Meat, milk and flora food: DR, OQ, FL, PE.
  - Gas, lubricating oil, water: DR and OQ only.
  - Energy: OQ and PE.
- **Per-class caps** [SOE-TREE]:
  - Duralloy Steel: DR 935-1000, OQ 1-1000, CR 696-800, HR 948-1000, MA
    348-400, UT 922-1000, SR 935-1000, CD 566-650.
  - Rhodium Steel (the bottom steel): DR 500-565, UT 400-478, CD 1-85.
  - Polysteel Copper: CD 928-1000, MA 928-1000, UT 728-800.
  - OQ is 1-1000 on almost every class. It is the universal lottery; the
    other stats mostly encode which subclass was found.
  - JTL classes are 1-1000 on everything.
    <https://swgaide.com/lunariels_condensed_resource_guide.html>
- **Rolls.** Uniform between the class minimum and maximum in the emulator
  [EMU `ResourceSpawner.cpp`]. The live distribution is unverified.
- **Percent of cap.** Galaxy Harvester defines it as (value - min) / (max -
  min) (<https://galaxyharvester.net/help.py>). Crafting maths uses the
  absolute value over 1000, so a perfect Duralloy still contributes only 0.40
  on an MA-weighted line. Some schematic lines could never reach 100 percent
  until the JTL resources arrived.
- **Missing stats: sources disagree.** Lunariel measured a missing MA as 0 in
  the success formula [LUN-EXP]; the wiki says a missing stat "seems" to
  count as 1000; the emulator leaves that resource out of the weighted
  average.

## 3. Spawn lifecycle

- **Pools** (per server, Patch 13.1, Feb 2005) [LUN-SHIFT; EMU agrees]:
  - Minimum pool, 15: guarantees one each of steel, copper, aluminum, three
    ores, both gemstones, known radioactive, solid and liquid petro, plus 2
    polymer and 2 lubricating oil.
  - Random pool, 27: a weighted draw (emulator weights: metal 32, gas 16, ore
    13, gemstone 12, petro 21 in three kinds, radioactive 4, water 2).
  - Fixed pool, 22: 14 iron and 8 JTL.
  - Native pool, 420.
  - About 484 live named resources, 64 of them not planet-native.
- **History.** The non-native count was retuned repeatedly: about 55 at
  launch, 53 at Publish 10, 45 at Publish 12 (which produced gas droughts),
  then 64. [LUN-SHIFT]
- **Planets.** Natives sit on their one planet. Most common mineral and gas
  classes appear on 8 of 10 planets; iron, some ores and petro fuels on 1 to
  3; JTL classes on 1. [LUN-SHIFT; SOE-TREE]
- **Lifetime.** Uniform by class: inorganic 6 to 11 days, organic 6 to 22,
  JTL 13 to 22. It does not depend on how much is harvested. [LUN-SHIFT;
  EMU] The wiki says 6-10 and 6-21; the 2004 FAQ says 5-15.
- **Names.** Random and unique. "Once a resource has despawned no more of it
  will ever be created in the galaxy, except by bugs or CSR intervention."
  [LUN-SHIFT]
- **Disagreement.** Koster describes resources as "mined out" and finite
  [K1, K-AH]. Player measurement and the emulator both show despawn on a
  timer; the emulator marks a quantity cap as "NOT standard behavior".
- **Density.** Perlin-noise maps per spawn per planet [K1]. Peak
  concentration by class: chemicals, inert gas and JTL up to 99 percent;
  non-ore minerals, reactive gas and flora in the 80s and 90s; ore, water,
  wind and solar in the 50s to 70s. [LUN-SHIFT]
- **Economic effects.**
  - Obsolescence: a manufacturing schematic binds to the named resource, so
    "that recipe was literally never craftable again" once stock ran out.
    [K1]
  - Hoarding and arbitrage: players held a great resource "until there were
    no ferrous metals available in the entire galaxy" and then released it.
    [K1]
  - Third-party trackers (SWGCraft, later Galaxy Harvester and SWGAide) were
    used "much like a Bloomberg terminal". [K-AH]
  - The resource database eventually ran out of space and needed manual
    cleanup. [K1]

## 4. Gathering

- **Survey and sample.** A hot and cold loop inspired by M.U.L.E. [K1]. Each
  resource family has its own tool; range grows with Surveying skill; nothing
  below 10 percent is shown. [WIKI; LUN-SHIFT]
- **Sampling** (emulator reconstruction): no sample below a density threshold
  that falls with skill; units = density x (25 to 28) x skill/100; occasional
  events multiply the yield by 5, which the FAQ says exist "to encourage
  manual sampling and discourage AFK sampling". Sampling is the only source
  of Surveying XP. [EMU; FAQ]
- **Harvesters.** Yield per minute = base extraction rate (BER) x
  concentration. [FAQ]

| Class | Max BER 2004 [FAQ] | Max BER NGE [WIKI] | Maintenance cr/h (2004 / NGE) | Power/h |
|---|---|---|---|---|
| Personal | 4 | 5 | 30 / 16 | 25 |
| Medium | 10 | 11 | 60 / 30 | 50 |
| Heavy | 13 | 14 | 90 / 90 | 75 |
| Elite (3 lots) | not in game | 44 | 126 | 206 |

  - Hopper size and BER are themselves experimented stats of the crafted
    harvester. [FAQ]
  - Unpaid maintenance damages the structure until it is destroyed. [FAQ]
  - 10 lots per character; harvesters and factories take 1 each. [FAQ]
- **Creature resources.** Hide, bone and meat come from corpses, for Scouts
  and Rangers only; milk comes from live animals. [FAQ]
- **Active versus offline.** Deliberately asynchronous. Koster compares
  harvesters to Farmville check-ins and says short sessions through "offline
  crafting and harvesting" were a design goal. [K1, K-FAIL] In practice the
  top crafters were supplied by harvester farms and hired hunters; sampling
  was mostly for XP and small needs.
- **Koster's regrets.** The cut Miner profession "made harvesting far too
  widespread", and harvesters should have had to be rebuilt per spawn because
  there "were not anywhere near enough economic sinks". [K1]

## 5. The crafting session

- **Tools and stations** [WIKI]: a generic tool up to complexity 15 with no
  experimentation; a specialised tool above that; a station at complexity 20
  and for any experimentation or factory schematic; a private station (in a
  house or a crafting droid) above complexity 25. Private stations have a
  quality rating up to 45, applied as rating/10 skill points. [SOE-SCRIPT]
- **Schematic.** Resource slots, component slots (identical or "similar"),
  optional slots, and experimental lines with weights. Two real examples
  [WIKI]:
  - Blaster Power Handler: attack speed and max damage weight CD 66 percent
    and OQ 33; min damage weights CD 50 and OQ 50.
  - E11 Carbine: damage weights OQ 33 and SR 66.
- **Weighted average.** Per line, X = sum over stats of weight x (the
  quantity-weighted mean of that stat across slots). Estei's worked example,
  with OQ 900 and SR 800 at 50/50, gives X = 850. [ESTEI; EMU]
- **Cap from resources.** Maximum experimentable percentage = X/10. [ESTEI;
  SOE-SCRIPT]
- **Starting percentage: two versions.**
  - Pre-CU, measured: Y = X x (0.000015 X + 0.015). That is 23.59 percent at
    X = 850 and 30 percent at X = 1000. [ESTEI; EMU]
  - NGE script: X x 15/1000 from resources, plus an assembly-result bonus of
    15, 10, 5 or 1 points. [SOE-SCRIPT]
- **Assembly roll** [SOE-SCRIPT]: d10000 + 100 x (assembly skill + station/10
  + (OQ-500)/40 + (MA-500)/40 - 3 per point of added complexity). Criticals
  are read from the raw die and narrow with skill. A Manufacturing Center
  city and some foods add to the roll.
- **Experimentation points.** Skill/10. A master has 10; clothing attachments
  cap at +25 skill, giving 12. [ESTEI; WIKI]
- **Value of a point.** 7 percent of the attribute's range. So 30 percent
  from assembly plus 10 points at 7 percent reaches 100 percent only with
  perfect resources. [SOE-SCRIPT]
- **Result tiers per point** (sources disagree on names and low-end values):

| Result | SOE script | Emulator | Wiki |
|---|---|---|---|
| Lucky (luck proc) | 8.4% | n/a | "Critical" 8.40% |
| Critical / amazing | 8.05% | 8% | 8.05% |
| Great | 7% | 7% | 7.00% |
| Good | 3.5% | 5.5% | 3.50% |
| Moderate | 1.75% | 1.5% | listed as "Success" 1.75% |
| Success | 1.05% | 1% | listed as "Moderate" 1.05% |
| Failure | 0 | 0 | not listed |
| Moderate failure | -3.5% | -4% | "?" |
| Big failure | -7% | -7% | "Failure" -7.00% |
| Critical failure | -7%, and one other attribute loses a point's worth | -8% | -14.00% |

- **Success formula.**
  - Lunariel, 2004, from the red failure bar: chance = 50 + (average MA -
    500)/40 + experimentation skill - 5 x points used at once. [LUN-EXP]
  - The script is the same shape: threshold = 5 x (10 + points) x (1 -
    skill/4000); modifier = skill + (MA-500)/40 + station/10 - 5 x (lines
    experimented at once - 1) - a complexity term. [SOE-SCRIPT]
  - Worked from the script: skill 100, MA 500, 10 points in one roll gives
    about 59 percent great or better, 7.5 percent critical success and 2.5
    percent critical failure. One point at a time is always at least great.
- **Modifiers** [SOE-SCRIPT]: a Research Center city multiplies the die by
  1.15; Bespin Port, entertainer inspiration and force skill add to the
  roll. The NGE added an expertise bonus of flat points on the resource cap,
  the source of the "960 is enough" rule on NGE servers.
  <https://swgr.org/wiki/crafting>
- **Risk elements.** More points per roll raises the threshold, and one big
  amazing roll frees a point (7 amazings equal 8 greats). Each press of
  Experiment adds 1 complexity, which is 8 seconds per unit in a factory.
  Resources are committed at assembly. [ESTEI]
- **Hidden breakpoints.** Estei documents armour-layer cutoffs every 25
  percent and fractional carry-over between sub-assemblies. Small percentage
  gains could flip a visible stat, which is what drove min-maxing. [ESTEI]

## 6. Subcomponents and the component tree

- **Structure.** Weapons are frame resources plus power handlers, a barrel,
  an optional scope and stock, and for melee reinforcement cores or vibro
  units. Armour is layers into segments into pieces. [WIKI; ESTEI]
- **How component stats flow.** The emulator adds component value x slot
  contribution to the item's current, minimum and maximum for that
  attribute. Components shift the whole experimentation window instead of
  being averaged in. [EMU `ResourceLabratory.cpp`]
- **Components are schematics too**, with their own weights and
  experimentation (a scope is 100 percent OQ; a stock is 100 percent SR on
  most lines), so quality compounds up the tree. [WIKI]
- **Identical slots and serials.** The T21 needs 10 identical power handlers,
  meaning the same serial number, which only a factory run produces. [EMU]
- **Looted enhancers.**
  - Krayt Dragon Tissue goes in the handler's optional slot: a standard
    tissue gives +30 max damage and -0.3 speed, better ones +100 and more.
    [WIKI]
  - Acklay bones go in melee reinforcement cores; Geonosian power cubes in
    Geonosian weapons and armour. [WIKI]
  - Bio-Engineer tissues add skill mods to tailored clothing, capped at +25.
    [EMU]
- **The loop this created.** Combat players farmed what crafters could not
  make, and crafters turned it into gear nobody could loot whole.

## 7. Factories and scale

- **Manufacturing schematic.** Made at a station from a finished,
  experimented prototype. It carries a run limit of at most 1000 and binds to
  the exact named resources and component serials used. [EMU; WIKI; K1]
- **Factory types.** Equipment, Food, Structure, Wearables. An Equipment
  Factory uses 1 lot, 50 power and 50 credits per hour. [WIKI]
- **Time.** 8 seconds x complexity per unit in a factory, against 2 seconds
  x complexity by hand. [EMU; ESTEI]
- **Crates and serials.** Output stacks into crates of identical items with
  one serial. Serials are what makes components stackable and what
  "identical" slots check. [WIKI; EMU]
- **Consequence.** A factory run is a commitment of a finite named resource.
  A player in Koster's comments: "limited runs with some resources and burn
  the 'junk' resources on mass production". [K1]

## 8. Professions

- **List.** Artisan is the starter. Elite from Artisan: Architect,
  Armorsmith, Chef, Droid Engineer, Merchant, Shipwright (JTL), Tailor,
  Weaponsmith. Bio-Engineer is a hybrid. Doctor and Combat Medic craft
  medicine. [WIKI]
- **Boxes.** Each profession is Novice, 4 branches of 4, and Master, 18
  boxes. A box needs XP of a specific type and skill points. [WIKI; K1]
- **Skill points.** 250 per character. Any box can be surrendered at any
  time. One character per server. [WIKI; K1]
  - Weaponsmith costs 92 points, 29 of them Artisan prerequisites, and needs
    896,000 Weapon Crafting XP. [WIKI]
  - Merchant costs 42 points and needs 405,000 Merchant XP, earned at 50 per
    vendor-hour and 50 per customer interaction. [WIKI]
  - A full crafter-merchant fits two or three crafting elites, never all
    eight.
- **Skill mods.** Assembly and experimentation mods come from the boxes.
  Armorsmith experimentation: 10 at Novice, then +10, 15, 20, 20 down one
  branch, and +25 at Master. [WIKI]
- **XP.** Fixed per schematic (T21: 650) [EMU]. Practice mode gives +5
  percent and no item [WIKI; EMU]. Any resource quality works. At 650 XP
  each, 896,000 XP is about 1,380 T21s. That is the grind, in practice
  driven by macros across several tools. [WIKI]
- **Koster's verdict.** Usage-based XP (crafters earn when their items are
  used) was cut for database load, and "crafting ... therefore fell victim to
  the classic grinding problems anyway". A cap of ten known recipes per
  crafter was discussed and "never happened", so masters also owned the
  component and low-end markets. [K1]

## 9. Decay and item sinks

- **Design intent.** "All items ... have a decay rate"; the item is
  destroyed at zero condition (game manual). "Everything in the game was set
  up to take damage, to decay ... even the repair tools ... got used up."
  [K1]
- **On death.** Insured items lose 1 percent on cloning, uninsured 5
  percent. [WIKI]
- **Repair.** Consumable repair kits. Player recollection only: each repair
  lowers maximum condition by about 10 to 20 percent, and weapons lose
  damage below 25 percent condition.
- **NGE** (15 Nov 2005) [WIKI]: decay and insurance were removed; 32
  professions became 9 classes with four Trader types; weapons moved to
  level-banded cores with barrels and scopes worth 1 percent; Krayt tissues
  became unusable; quest and loot rewards competed with crafted gear.
- **Player accounts, June 2006** (disputed in the same thread): "the most
  devastating change was DECAY was taken out"; "no resource market anymore";
  an active crafter replied that he still had repeat business.
  <https://forums.mmorpg.com/discussion/81195/crafting-post-nge-question>
- **Koster on metrics.** Adding server-wide vendor search "caused one of the
  single largest single-week drops of subs in SWG's history". [K-FAIL]

## 10. Merchant layer

- **Vendors.** NPCs, droids or terminals inside player structures, with
  maintenance fees. The Merchant tree unlocks more vendors, more stock,
  advertising and uniforms. [WIKI]
- **Bazaar.** A deliberately limited "commodities market": "very carefully
  not a full auction house", with no remote delivery and a low price cap
  (3000 credits in early 2004). [K1; FAQ]
- **Placement.** No player housing in NPC cities, so shops clustered in
  player cities and outside starports. Buyers found this "a massive
  inconvenience". [K1]
- **Scale.** "At peak, fully half the players in Star Wars Galaxies ran a
  shop." [K-AH]
- **Reputation.** Items carry the crafter's name. "Crafters and brands ...
  could effectively build up very real fame on the server." [K1]
- **Outcome.** A "Pareto-distributed economy where the richest were insanely
  rich", driven by one player integrating harvester, crafter and merchant.
  [K1]

## 11. What worked and what failed

**Worked** (Koster and broad community agreement): variable resources plus
obsolescence created perpetual exploration and a living market; crafter
fame and interdependence; shops as a play style; short sessions with the
highest weekly hours at SOE. [K1, K-FAIL, K-AH]

**Failed or cost something:**

- **Grind.** XP per craft plus practice mode produced macro loops; the cut
  usage-based XP was the intended fix. [K1]
- **Idle accumulation and sprawl.** Permanent harvester networks and too few
  sinks. [K1]
- **Hoarding and "server best" lock-in.** An uncapped 1-1000 OQ and permanent
  despawn meant a veteran's stockpile could not be matched until chance
  produced another top spawn. Lunariel notes holding OQ 960 or better of
  every iron subtype by late 2004. [LUN-SHIFT] The newcomer gap follows
  directly.
- **Spawn-count tuning was fragile.** Publish 12 produced weeks without any
  gas. [LUN-SHIFT]
- **Complexity barrier.** The real formulas were hidden and
  reverse-engineered by players; third-party databases were effectively
  mandatory. [LUN-EXP; K-AH]
- **Shopping friction.** [K1]
- **Koster's general lesson.** "You build your game around it, not the other
  way around." [K1]
- **A dissenting player view.** SWG crafting became "mass production-focused
  competitive play", not craftsmanship. [K1 comments]

## Other games

**EVE Online.** No quality variation: every Tech 1 item of a type is
identical. Variance lives in the blueprint (material efficiency research to
-10 percent materials, time efficiency to -20 percent; copies have limited
runs) and in location (structure and rig bonuses, a system cost index).
Demand comes from destruction, not decay. Margin, logistics and scale carry
the depth that SWG put into item stats.
<https://wiki.eveuniversity.org/Manufacturing>

**Fallen Earth.** Nearly every item is player-craftable. Crafting runs on
real-time timers that continue offline, with a queue. Bosses drop parts, not
finished items. No input-quality system was found; time is the scarce input.

**Vanguard: Saga of Heroes.** Crafting is an encounter: a pool of action
points spent over four stages on actions that add progress or quality, with
random complications to fix or absorb, and a result graded A to D.
Higher-grade refined inputs pre-fill the quality bar. XP comes mainly from
NPC work orders that supply the materials, explicitly to avoid flooding the
market. Quality comes from play during the session, not from resource stats.
<https://www.tentonhammer.com/guides/11-08-06-crafting-preview-pg-2>

**Star Citizen** (information dated March to September 2026). Crafting
shipped in Alpha 4.7 with a placeable fabricator. Every resource has a
quality of 0 to 1000; mixed stacks average by quantity; stats preview live in
the interface. Refining passes the input's quality through and material
quality cannot be upgraded. Blueprints are earned from missions. Planned and
not final: wear, with repairs lowering maximum function. No official
quality-to-stat formula is published.
<https://theimpound.com/blogs/guides/star-citizen-alpha-4-7-introduces-crafting>

**Dune: Awakening** (launched June 2025; figures as of February 2026). No
resource quality. Progression is material tiers, researched schematics and
fabricators. The sink is durability: each repair cuts maximum durability by
15 percent. The Deep Desert is wiped and reshuffled by a weekly storm, so
churn is applied to the map, not to the materials.

**Albion Online** (wiki snapshots from 2022 and 2023). Power is tier,
enchantment and a quality roll on the finished item (Normal 68.8 percent,
Good 25, Outstanding 5, Excellent 1, Masterpiece 0.1). Inputs have no
quality. Economic variance comes from where an item is crafted (a resource
return rate that depends on the city) and from focus, a currency gated by
time. Full loot is the sink.

**Wurm Online.** Everything has a quality level of 1 to 100. A gathered
item's quality is random up to the lower of the gatherer's skill and the
tile's inherent quality. Improving is limited by skill and material quality.
Effective quality falls with damage. One scalar, capped by character skill,
so the lock-in is skill time, not stockpile.
<https://www.wurmpedia.com/index.php/Quality_Level>

**Stars Reach** (Playable Worlds, Koster; Steam Early Access from 18 August
2026). Each resource has four stats: Potential, Quality, Resilience,
Versatility. The combining rule in the August 2025 notes: per stat, throw
out the lowest input and average the rest. Stats map to concrete traits
(battery life from Potential and Resilience, beam heat from Quality). The
same material differs by planet, and deposits deplete physically. An
unofficial wiki lists usage-based crafter XP and limited recipe space, the
two things Koster says SWG should have had; this is unverified against the
Early Access build. <https://starsreach.com/twilight-update-notes/>

## The levers and what each is known to do

| Lever | SWG's setting | Observed effect |
|---|---|---|
| Stat count | 11, with 2 to 8 per class | Many attributes plus per-schematic weights make "best" depend on the recipe. That sustains specialised markets and trackers, and raises the complexity barrier. Stars Reach uses 4; Star Citizen and Wurm use 1. |
| Cap structure | Per-class minimum and maximum on most stats; OQ uncapped; the absolute value used in the maths | Subclass identity matters and some lines can never max. Uncapped OQ is a pure lottery and the main hoarding driver. Removing caps (JTL) devalued the old ladders. |
| Spawn churn | 6 to 11 days inorganic, 6 to 22 organic; about 64 non-native live; never repeats | Perpetual surveying, obsolescence and speculation. Too few live spawns causes droughts; permanence causes stockpile lock-in and a newcomer gap. Dune churns the map; Albion and EVE do not churn materials. |
| Weighting formula | Quantity-weighted average, X/10 as a hard cap, about 30 percent from assembly, 7 percent per point | Resources set the ceiling and skill sets how reliably it is reached. Linear caps make 960 against 990 visible, so tiny margins command large prices. Stars Reach's drop-the-lowest rule softens one bad input. |
| Experimentation risk | 50 + skill + MA/40 - 5 per point; criticals on the raw die | A real gamble only at low skill. At master level it is a mild optimisation about batching points, with a small critical tail. City, food and buff modifiers create service demand. Vanguard moves risk into session play; Albion into a quality roll. |
| Decay rate | Use decay, 1 or 5 percent on death, repairs cut the maximum | Repeat business. Removing it (NGE) was widely blamed for collapsing demand. Star Citizen and Dune use the same repair-lowers-maximum pattern. |
| Component depth | 2 to 3 tiers, identical-serial slots, looted optional enhancers | Factories become mandatory, hunters and crafters depend on each other, and quality compounds. Deeper trees favour vertically integrated groups. |
| Recipe breadth and skill cap | 250 points, no recipe cap | Forces a choice of profession, but not interdependence between crafter tiers. Koster's unshipped fixes were a recipe cap and usage-based XP. |
| Market information | Local vendors, capped bazaar | Shop-running as play and local price variation, at the cost of buyer friction. |
| Offline accumulation | Harvesters and factories run unattended | Short sessions and high retention, but unbounded scaling without sinks. |

## Could not verify

- Bazaar price caps between 3000 credits (2004) and the NGE's 10 million.
- When the Elite harvester was introduced.
- Repair-kit effects on maximum condition (player recollection only).
- The live stat-roll distribution (the emulator assumes uniform).
- Koster's 474 and 79 class counts against the datatable.
- EVE's module drop chance from an official page.
- A general creation formula for Wurm.
- Albion's numbers after 2023.
- Stars Reach's combining rule in the Early Access build.
- Star Citizen's quality-to-stat formula and what shipped after Alpha 4.7.
