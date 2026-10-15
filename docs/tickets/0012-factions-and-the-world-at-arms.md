# 0012: Factions and the world at arms

Status: designed 2026-10-14; step 1 (factions and territory) built; step 2a (the outlaw camp site) built; step 2b (encounters at the camps) built; step 3a (claims and deeds) built; step 3b (guards, warnings and crossing) built; step 4a (caches and the fall) built; step 4b (postings and the heard-of map) built; step 5a (the wreck and the derelict machine) built; step 5b (people, salvage and work there) built, so the ticket is built through step 5
2026-10-14. The user's answers are marked
(user); everything else is a default of mine, listed at the end and easy to
change. Singleplayer: nothing here touches the protocol or `apps/server`.

## What the user said (2026-10-14)

> yes, world combat sounds good. needs some design work. I want things to be
> procedural with factions, settlements etc. having political affiliations
> etc. so factions need to be considered. unsure if they would naturally sit
> on top of deposits? maybe some high value ones. we can also consider other
> POIs for the player to explore

Asked and answered:

- **Factions are fully procedural** (user). A world makes its own factions
  with their own names and aims; the setting's three institutions become
  kinds of faction, not fixed players.
- **Outlaws and wildlife are always hostile; a faction only when crossed**
  (user): trespass, taking from its claim, or attacking its people. Working
  for one does not make the others enemies (docs/SETTING.md).
- **A faction remembers deeds, hidden** (user): no bar on screen. What a
  character did for or against a faction shows as how its guards greet them,
  what its boards pay and who will teach them.
- **A fall in the world carries the member home injured** (user): out of play
  for a stretch of played time while another member takes over, the gear
  worn. Nothing is dropped (user, later the same day: "don't like the item
  drop mechanics; not fun, needless corpse run").

## What is there today (read 2026-10-14, nothing run)

- **Fights** happen once, at the staging spawn west of Port Serein
  (`apps/game/src/combat.rs` `setup`, the default five or `--hostiles`). A
  hostile is a `Combatant` on `team` 1 with an `OpponentBrain` of five kinds
  (sentry, rusher, marksman, duelist, bulwark); there is no faction beyond
  that `u8`. Nothing respawns; F5 resets the whole encounter.
- **A fall** is `Action::Dead`, terminal until F5; the HUD shows DOWN and
  Retry. Wear knocks everything carried once (`wear.rs`, `down: 150`). The
  household has no notion of an injured member.
- **Sites** are six kinds (`assets/sites/`), placed per land mass in the
  river bake, named from the seed and keyed by a stable `SiteId::key()`.
  Site kinds and placed sites are hashed into `world_version`. Port Serein is
  the only settlement, authored. Nothing carries an owner or a population.
- **Deposits** are a pure function of the seed and data, in no version. A
  deposit's quality is one shared roll (`grade`) plus a spike per attribute;
  the shared roll is not kept on `Deposit`.
- **Institution names** live in data and comments: Pelagic Works is the one
  buyer (`assets/procurement/pelagic_works.buyer.ron`, hardcoded by id in
  `household/props.rs` and in one prompt string) and the maker of five items;
  the Borough Assembly and the Continuity Institute appear only in the elite
  trees' descriptions.
- **The map** (`strata_sim::chart`, `ui::map`) draws sites, found deposits,
  harvesters and the lead; it has no area overlay.

## The design

### 1. Factions are made from kinds

A faction kind is data, `assets/factions/*.faction.ron`: what it is
(corporate concessionaire, settlers' assembly, conservation body, outlaw
band; more later), the words its names are built from, its colours and
insignia symbol, what ground and site kinds it favours, how it treats a
deposit it holds (concession, commons or reserve), who its guards are
(opponent kinds and their weights), and its temper (how quickly it turns
hostile, how long it remembers).

`strata_worldgen::factions` (no Bevy, a pure function of the seed, the
sites, the settlements and that data, in no version, like the deposits)
makes a world's factions: a handful per land mass, each a kind, a generated
name ("Halvorn Extraction", "the Kessel Commons", "Ninefold Trust"), colours
from its kind's palette and a stable id. The authored settlement keeps its
authored holder: Port Serein's corporate faction is Pelagic Works, so the
board, the makers' names and the elite trees stay true.

### 2. Every place has a holder, and the ground has territory

Each settlement and site is given a holder by its kind's preferences and by
neighbourhood: towns and hamlets cluster under assemblies, the landing and
the big works under corporates, monitoring stations under conservation
bodies, and an outlaw band holds a camp in ground no one else reaches.
Territory is a field read at any point: the strongest holding within reach,
weighted by the site's rank and falling off with distance, and beyond every
reach the ground is unclaimed wilderness. The place card names the holder as
you enter its ground; the map gains a territory layer (a chip, off by
default); `just worldmap` gains `-factions`.

### 3. Rich deposits are claimed

A deposit whose shared grade is in the top share (data, a fifth to start)
and lies in held territory is that holder's claim, of its kind:

- **Concession** (corporate): fenced and guarded. A lease from its board
  makes working it lawful; without one, sampling or planting is a deed
  against it and the guards act.
- **Commons** (assembly): open to work, but taking past a share in a played
  day is a deed against it; the locals warn, then turn.
- **Reserve** (conservation): working it at all is a deed against it.

A rich deposit in wilderness is unclaimed and may draw an outlaw camp to
it. Ordinary deposits stay open ground. The register, the card and the map
show a claim and its holder once the deposit is found.

### 4. Places worth finding, part of the world

The user asked that the new places be meaningfully integrated with the
world (2026-10-14). So they are site kinds like the hamlet and the station,
not a layer beside them: `assets/sites/*.site.ron`, placed per land mass by
the same bake (`hydrology/place.rs`) after every existing kind (a higher
rank, so the sites already there keep their places), each on a levelled pad
with its own building group baked by `just content`, named by the site
namer, reached by the road network, drawn on the map and the worldmap. Each
is in the world for a reason the ground gives it, and each is tied to the
people around it by the faction layer:

- **An outlaw camp** wants remote, sheltered ground: far from towns and
  service roads, near water, under cover or behind a shoulder with a view
  of the approach. A trail, not a track, leads in: people walk there. Tents,
  a fire, crates, a lookout. Who sits there is an outlaw band of the faction
  layer; what it preys on is the nearest held road and its settlements, and
  the rich unclaimed deposits around it.
- **A wreck** lies where a freighter came down: on open ground under the
  approach of a landing, or on a coast or a plain within reach of one. Hull
  sections, a scatter of cargo, a scar in the ground (the pad's cut). Its
  cargo belonged to a faction (a corporate one by default), so what is
  carried back to that faction's board is a deed for it; scavengers may
  already be there.
- **A derelict machine** is an old terraforming project (docs/SETTING.md):
  a pump on a river's bank, a processor's broken mast on a ridge, standing
  where its job was. It wants the feature its job needed (a river, a high
  shoulder), far enough from the living towns to have been left. Its broken
  security guards it; its parts are the rarest salvage. The faction kind
  that built it (corporate or conservation) still claims it on paper.

What makes them part of the world, not dots on it:

- **The roads know them.** The network links each with a trail or a track,
  so a road walked far enough leads to one, and a camp's trail is a sign.
- **The factions know them.** A holder's board posts work about the places
  in or near its ground: clear the camp preying on its road, recover a
  wreck's cargo, secure a derelict's parts. A posting is a board line (the
  procurement board's Project lines grow a target place) that pays credits
  and a deed. A posting names the place, so the map marks it as heard of
  (its area, not its point) until it is found.
- **The ground knows them.** A wreck's scar, a camp's trodden yard and a
  machine's cut are in the samples, so they look like they have been there.
- **They change with what is done.** A cleared camp falls quiet and its
  holder's road is safe for a while; when it is taken again the board says
  so. A recovered wreck is empty; a secured machine is the holder's.

The cost: new site kinds and their trails are part of the world, so
`world_version` and the canary networks change once with the first of them
(a `GENERATOR_VERSION` bump: roads gain links to the new places and some
reaches of road move). Saves carry over: the household's survey record is
stamped with the seed and the deposit lattice, not the world version, so
found deposits stay found; a deposit or a harvester on ground that a new
pad now covers is the rare loss. Later kinds (an overgrown cultivation
project, a den, a cache, a ruined station) follow the same file.

### 5. Encounters stream with the observer

An encounter is data, `assets/encounters/*.encounter.ron`: who stands there
(opponent kinds and counts), how they stand (guard a point, patrol a loop,
sit a camp) and a tier. The tier rises with distance from the arrival and
with the place's kind, and scales the opponents' rows (data, never a code
literal). `strata_sim::encounters` keeps `NearEncounters` like
`NearSites`: an encounter is stood up when an observer comes within its
reach, its people get a faction tint and their kind's brain, and it is
folded away when the observer leaves and nobody is fighting. A cleared
encounter stays cleared for a stretch of played time (data, a few hours to
start), then its holder sends people back. The staging fight stays as the
training ground, with F5.

### 6. Who is hostile

A combatant carries an `Allegiance` (a faction id, or wildlife). Outlaws
and wildlife are hostile on sight. A faction's people are neutral until
crossed: trespass on a guarded claim draws a warning (a word over the
guard, a countdown), then hostility; attacking one turns that encounter at
once. Hostility is written into the existing `team` so the sim's rules
need no second idea of an enemy.

### 7. Deeds, not a bar

`Deeds` (saved with the household) is a short ledger per faction: its
people hurt and killed, its claims worked without leave, its enemies
cleared from its ground, its boards sold to. One rule turns it into a
stance (welcoming, wary, hostile) with a decay over played time. A stance
sets how its guards greet the member, its boards' prices, and whether its
trainers teach. Nothing shows a number; the place card and the guards' words
say how a faction feels.

### 8. Rewards feed the bench

A fallen opponent or a cleared place leaves a container to open with F:
credits, lots, salvage (new component items with low, wide quality lines
that a smith can rework) and, rarely, a module or a schematic. Tables are
data by place kind and tier.

### 9. A fall in the world

The member is carried home: a fade, then they are at the household's bench
place, out of play for a stretch of played time (data), and the bench
switches to the next member able to go out. If none can, the soonest well
wakes early. Everything carried comes home with them: no drop and no run
back to fetch it. The cost of a fall is the time out and wear's fall rule,
which runs as now; the encounter that won keeps its ground.

## The plan, a step a workflow

1. **Factions and territory.** Faction kinds, the generated factions, the
   holders and the territory field; the place card, the map's layer and
   `-factions` on the worldmap. Nothing hostile yet.
2. **Encounters in the world.** Encounter data and tiers, streaming,
   allegiance and the hostility rule, the outlaw camp as the first new site
   kind (its building group, placement, trail and the one world version
   change), cleared and returning. The harness gains a two and three against one fight
   so tiers are measured, not guessed.
3. **Claims and deeds.** Claims on rich deposits, guards at them, warnings,
   the ledger and its stance, a lease at a board.
4. **Rewards, postings and the fall.** Loot containers and tables, salvage
   items, a board's postings about places and the heard-of mark, the member
   carried home injured.
5. **More places.** The wreck and the derelict machine as site kinds, their
   building groups, postings and rewards.

Wildlife is a ticket of its own: it needs creature bodies and motion that
the forge pipeline has not made yet.

## Decided for the user and easy to change

- Faction kinds as data with generated names; Port Serein's holder authored
  as Pelagic Works.
- A handful of factions per land mass; territory falls off with distance and
  the far wild is unclaimed.
- The top fifth of deposits by grade, inside held ground, are claims.
- The new places rank after every existing site kind, so the old sites keep
  their places; roads gain trails to them and the world version changes once.
- An outlaw camp is reached by a trail, a wreck and a derelict machine by a
  track.
- A wreck's cargo belongs to a corporate faction by default.
- Encounters stream within a reach of the observer and a cleared one returns
  after a few played hours.
- Hostility rides on the existing `team`; trespass warns before it turns.
- Deeds decay over played time; a stance has three words.
- A fall costs time out and wear, nothing more.
- Wildlife waits for its own ticket.

## As built

### Step 1: factions and territory

- Data: `assets/factions/` has four kinds (corporate, assembly,
  conservation, outlaw, which holds nothing yet), `anchors.ron` (Port Serein
  held by Pelagic Works, reach 9 km) and `rules.ron` (`contested_margin`).
  `strata_registry::faction` loads and checks them into
  `GameRegistries.factions`, refusing a bad site kind, settlement, symbol,
  palette or name set by name. In no version, no handshake.
- `strata_worldgen::factions::FactionMap` (no Bevy): `faction`,
  `factions_near`, `holder_of`, `holdings_near`, `territory`,
  `territory_among`. A mass makes its factions once from its own sites: a
  count of round(eligible / `sites_each`) in `least`..`most`; the first seat
  the best site, each next the farthest from the seats; names from the
  site namer, unique on the mass; colours from the palette, seeded order.
- A site goes to the faction whose seat is nearest after dividing distance
  by the kind's weight. Strength at a point is 1 - distance / reach; the
  strongest holds it, contested when the next is within `contested_margin`,
  unclaimed when none reaches.
- Sim: `Factions`, `HeldHere` (refreshed on the pool after 100 m or a site
  entered or left), `site_at` shared with the place card. Client: the card
  says "Held by" on a site and names new ground for 4 s; the map's Holders
  chip (off) tints, hatches contested ground, draws borders, names factions
  in the legend and a site's holder in its panel. `just worldmap` draws
  `-factions` (`--no-factions`).
- Seed 42's home mass has 242 factions; Krot Station, 1 km from Port
  Serein, is a conservation disc cut into Pelagic's ground. Stills:
  `docs/design/factions/f1-*.png` (map region, panel, far; cards; worldmap).

Decided for the user and easy to change: `sites_each` 25, 25, 12 and `most`
500; an anchor is strength 1 only at its centre, so a site beside it can
outrank it; palettes; Pelagic's `#E8365D`; notice 4 s; legend cap of 4.

Left rough: the legend names 4 of about 15 Far-map factions; the picture
finds the sea by ground colour; `chart::territory` has no unit test; no
still of a walked border or "Unclaimed wilds"; the worldmap tints sea; name
roots read as fantasy (seat place names would fit); 242 factions may be many.

### Step 2a: the outlaw camp

- Follow ups: an anchor holds its core (`core_m` 3500, so Krot Station is Pelagic Works'); a faction's name is rooted in its seat's place ("Skan Cut" gives "the Skan Gang") and never equals its seat's name; palettes spread, corporate in ambers and ochres, outlaw in purples and rusts, assembly's navy lightened off the sea.
- Site kind `assets/sites/wild_camp.site.ron` (`strata:wild_camp`, rank 6, trail, footprint 26 m): near a river or lake, off the sea, beyond 2.2 km of a hamlet, 2.5 km of a landing, 5 km of a town or settlement (fades of 0.4 to 1 km, which the scoring turns into real floors), eyes on a view, within 12 km of a settlement preferred; `per_1000_km2` 30, own spacing 3 km. Names are rough ("Snogirk Cut", "Mecark's Rest", "X's Wade"; "The X" capitalised since a name must start upper case).
- Group `veyra:outlaw_camp`: three designs, each mirrored, of tents, a fire pit and a lookout (new `Furniture::{Tent, FirePit, Lookout}` in veyra-settlements) with containers and crates as the barricade. Outlaw faction holds camps (reach 1200 m, about two a band).
- Version: PINNED is now `0x7e0f_4a47_794f_227d` (was `0x0a24_b0e8_ffa4_563b`), a data file alone, no `GENERATOR_VERSION` bump; seed 42 snapshot unchanged. Saves: `SurveyedIn` is the seed and the lattice cell only, so surveyed deposits and harvesters survive (read, not tested anew).
- Old world: within 40 km of the city, kind, name, centre and key of all 162 old sites are identical (hamlet 69, landing 8, landmark 20, pass shelter 2, station 53, town 10); 37 camps added (`scratch/combat/sites-before.txt`, `sites-after.txt`; the dump predates the capital "The" in camp names). Camps within 20 km: Snogirk Cut 5.9 km, Skan Cut 6.0, Mecark's Rest 9.0, Gakork Hide 9.9, Bocal Bend 9.7, Diderk Gully 10.4 (all the Skan Gang), Rogrut's Wade 12.9, Skor Hide 17.6 (the Gratet Band), Jerk Hide 17.9 (the Jerk Gang). None within 3 km. Trails moved: not measured per road; the camps' own trails are the added length.
- Worldgen `roads` and `sites` suites pass with camps in (no pad in water, every camp reached, no road across a pad).
- Stills (`docs/design/factions/c2-*.png`): the card on a camp, the camp, `--at camp` (stands on the trail 120 m out from the camp nearest Port Serein, facing it; the camp lies in a dip so only the trail shows at 120 m), map, worldmap layers.
- Decided for the user and easy to change: camp count and floors (the site file), the map's mission symbol for stations, landmarks and pass shelters (camp symbol is now the outlaw camp's alone), a core site gets no second holding.
- Left rough: some camps sit 0.4 to 2 km from a service road or track (placement runs before roads); the camp is sparse; no still of the "Outlaw ground" notice or of the map panel naming a camp's band.

### Step 2b: encounters at the camps

- Data: `assets/encounters/outlaw_camp.encounter.ron` (`strata_registry::encounter`, `GameRegistries.encounters`, in no version): `stand_m` 260, `fold_m` 420, `returns_h` 3, a roster per tier, and where people stand (`ring`, `nudges`, `clear_m`). An unknown opponent kind or site kind is refused by name. `assets/combat/tiers.ron` is the seventh combat table (own version, in the handshake hash): bands 6 and 15 km from Port Serein's centre, health and shield 1.0 / 1.3 / 1.7, damage 1.0 / 1.15 / 1.3; `combat_tiers::{tier_at, opponent_pools}` are the only readers, the game and the harness both.
- Sim: `strata_sim::encounters`: `plan` (a pure ring of slots round the pad, moved off the group's buildings), `NearEncounters` (camps within `stand_m`, not quiet, refreshed on Update from `NearSites` by distance, never sampling ground), `Allegiance` and `hostility` (the one rule step 3 adds an arm to), `EncounterId` on every opponent, `PlayedClock` (nominal seconds from the tick), `Cleared` (camp key to the played second), `EncounterChanged`.
- Engagement is per encounter (`Fights`): a person hurt or the member within `engage_distance` of one engages only its own camp; `CombatEncounter` reads `Fights::headline` (a camp before the staging fight). A camp folds beyond `fold_m` only when nobody fights it. F5 lets every camp's fight go (`abandon_camps`): hurt people no longer re-engage it, so a camp left behind by a fall folds and the HUD and UI locks release. The staging fight and F5 are as before.
- Clearing: `mark_cleared` clears a camp only after one of its own people was seen alive and all its slots have stood, so old bodies or people still waiting for ground never clear a camp that has just returned. After `returns_h` played hours a camp is stood down and, with the member still near, stood again fresh at its tier of then. `Household.played` and `Household.cleared` are saved (serde default, no version change, stamped with `SurveyedIn`); the clock alone is written once a played minute. The card says "Cleared" and the map's camp panel says it.
- Game: `camps.rs` turns changes into people through `combat::spawn_hostile`, which setup shares (no `ResetPosition`, `BodyTint`), and folds them with their bolts. `--camp-tier n` and `--cleared` (refused on the default household) are for stills.
- Harness: `Side.with` mixes profiles in a line, `Side.tier` scales through the sim; presets `camp-t1/t2/t3-vs-carbine`, a bought carbine with the usual `HoldAndFire` policy (the one a first-hour household plays) at 12 m against the camp at 30 m. Measured 20 bouts, seed 1: tier 1 1.00, tier 3 0.25 (0.60 of bouts drawn at the cap, so a `DrawRate` band at most 0.75 sits beside it). Tier 2 with the briefed roster stayed 0.85 to 0.95 through three rounds of `tiers.ron` (to 2.0 health, 2.6 damage); rosters then measured 1.00 (adding a duelist), 0.90, 0.80, 0.55, 0.70; shipped: two duelists, three rushers, two marksmen, 0.70 at 20 bouts and 0.525 at 40. Tier 3 was then made tier 2's seven plus the bulwark, so a far camp is never smaller than a middle one: 0.10 won, 0.25 drawn at 20 bouts.
- Stills (`docs/design/factions/c2-*`): `c2-camp-people` (tier 1 people on the pad, HOSTILES AHEAD), `c2-camp-fight`, `c2-camp-tier3`, `c2-card-cleared`, `c2-outlaw-ground`. A camp's people closing on the member is proved by a game test (a hurt person engages, the nearest closes), as xvfb runs the sim too slowly to see it.
- Decided for the user and easy to change: the tier 2 roster (it departs from the briefed rusher, sentry, marksman, which a shooter wins nearly every time); the bands and factors; `returns_h` 3 (the file); the played clock saved once a minute.
- Left rough: a band's colour does not read (`BodyTint` is the household's four-colour palette, so every outlaw band tints alike and faintly on the dark body); a camp with a slot that never finds ground never clears; plates of a crowd overlap; `Cleared` does not forget a camp whose world was left.

### Step 3a: claims and deeds

- Data (in no version): `assets/factions/claims.ron` (share 0.2, `min_strength` 0.25, a claim kind per nature, `commons_share` 30 units a played day, `day_h`, the lease at 400 cr for 6 h) and `deeds.ron` (a weight per deed kind, `camp_reach_m` 6000, half life 24 h, stance bands, a price factor per stance). `anchors.ron` gains `buyer` and `start`; Pelagic Works keeps `veyra:buyer.pelagic_works` and starts welcoming.
- Claim: `Deposit::grade_pm` is the shared roll kept (no figure moved); `FactionMap::claim_of` is one pure function (top share, held centre, strength at least `min_strength`, a kind for the nature). Round Port Serein (6 km, seed 42): 304 deposits, 243 open, 45 Pelagic concessions, 3 Moment Works, 5 commons, 8 reserves.
- Ledger: `strata_sim::deeds` holds `Deeds`, `score` (half life), `stance`, `Terms` (price factors, `refuses`), `board_terms` (the one rule from a board to its faction's terms, asked by sim and client), `Leases`, `CommonsTaken` and the `Ledger` param. Deeds are written by sampling a cycle, planting, collecting, selling at an anchor's board and clearing a camp (once, for each non-outlaw faction within `camp_reach_m`). Hurt and killed arms are left for 3b.
- Stance and words: no score reaches a screen. The register row and card, the deposit card, the place card's third line, the map's deposit panel and the survey screen's line under Sample say whose claim a found deposit is (never an unfound one); the entering notice's third line and the board's line under its title give the stance as words.
- Leases and prices: `ProcurementRequest::Lease` at the anchor's board (not when hostile or short); sales and purchases take the stance factor (welcoming pays 1.05, sells 0.95), a hostile board refuses. The balance first-hour model uses neutral prices, so live sales at Pelagic pay 5% over its page.
- Save: `Household` gains `deeds`, `commons`, `leases` (serde default, no version change, stamped with the world, copied on a real change with the played clock). `--stance <faction>=<stance>` and `--lease` are for stills (refused on the default household).
- Stills (`docs/design/factions/c3-*`): register, survey claim, place card, board (lease, leased, hostile), entering notice. Seen only by test, not in a still: the map's panel and the register card's notes (`claims::tests`).
- Decided for the user and easy to change: weights, bands (welcoming at +5, hostile at -10) and Pelagic's start 8; the lease price and hours; deeds of a kind within a minute merge into one entry; the stance is read live, so a hostile board refuses Sell and Lease alike.
- Left rough: the hooks into advance_sample, planting, collecting and mark_cleared are tested through the `Ledger` rather than each system; the claim_of test's min_strength case is conditional and only the Concession kind is asserted there; the possessive is built in `claims.rs`; "leased" words go stale after a lease expires until the card rebuilds; the merge window is a literal (a merged entry keeps its first date).

### Step 3b: guards, warnings and crossing

- Data: `EncounterDef.at` is `Site(kind)` or `Claim(kind)`; a claim file needs `post_m` (14). `concession_post` (guards) and `reserve_post` (wardens) ship; `claims.ron` gains `warn_s` (12) and `guard_reach_m` (3). Commons have no post.
- Posts: `NearEncounters.posts` (`NearPost`, `refresh_posts`, `plan_post`), at most two (`MAX_POSTS`), people on a ring round the deposit's centre; `EncounterId::Post(post_key)`, `Allegiance::Faction`. A cleared post is quiet for `returns_h`, writes no camp deed.
- Teams: `NEUTRAL_TEAM` (2) and `enemies(a, b)` are the one enemy rule (ride, ability, script, reticle ask it); `hostility(allegiance, crossed)`. Physics is unchanged, so a neutral guard can be struck.
- Crossing: `Fights` keeps posts apart from camps; an uncrossed post is never engaged by distance or by another's blow, and `headline` skips it. A post is crossed by a member's blow (`strike_posts`), by the warning running out, or by a hostile stance with the member in engage distance (`watch_posts`); it stays crossed until it folds. `PostTurned` carries why.
- Warning: `Ledger::trespass` starts `Warning { post, faction, until }` on a sample or plant without leave; it lapses on leaving the radius or a live lease.
- Deeds: a member's first blow on a person writes Hurt once, a lethal one Killed; outlaws write none.
- Lease: `tend_guards` gives each uncrossed guard `InteractionKind::Guard`; `check_lease` accepts a guard in reach as well as the board; a warden sells nothing.
- Client: `BodyColour` beside `BodyTint` (faction and band colours, outlaws included); a steel neutral plate (`strata.nameplate.neutral`) saying "<Faction> guard"; the warning word counts whole seconds; a toast on turning; the guard's F chip. No number of the ledger shows.
- Stills: `docs/design/factions/c3-{post,warning,crossed,guard-lease,hostile-sight,band-tint}.png`. `--at claim[:faction]`.
- Review fix: a post that folds and stands in one pass is registered again (folds apply before stands).
- After the workflow: the two standing posts are the nearest (a farther post nobody has crossed, fights or is warned by gives way to a nearer claim), and a wet slot of a post's ring is drawn in towards the dry deposit centre.

Decided for the user and easy to change: `--at claim` stands 40 m away; own colour is lifted and glows because dark armour hid it (constants in `combat_art.rs`); `MAX_POSTS` is a constant.

Left rough: post rings check no buildings or roads; Pelagic red and Skan plum look alike, only the plate tells them apart; guard-lease and hostile-sight stills predate the final glow; no test drives a Sample or Plant to the warning, nor F to the lease toast.

### Step 4a: caches and the fall

- Data: `assets/loot/outlaw_camp.loot.ron` (`strata_registry::loot`, `GameRegistries.loot`): a table a tier of credits, picks (item, count, chance, lines low and wide), lots (class, units, chance) and rare modules; `lots_within_m` is 4000. `assets/household/fall.ron`: `out_h` 2, `fade_s` 1.2, `early_min` 0. In no version, no handshake.
- Worth at Pelagic's board (average of 600 seeds, held by a test): tier 1 about 235 credits, tier 2 about 640, tier 3 about 1360, parts the board does not buy extra.
- Roll: `strata_sim::loot::roll` is pure in its seed (world seed, camp key, played second of the clearing). Salvage is existing components with maker "salvage"; a lot comes from the nearest deposit of its class within 4 km, read from the survey's `NearDeposits` (8 km round the member) when it holds the whole circle, else one search of the field (`loot::lot_candidates`); a test holds the two equal.
- Caches: `Caches` is written by `mark_cleared` for a camp only (never a post) and pruned when the camp returns. `CacheRequest::Take` in `CacheInbox` moves credits to the `Purse` and what fits into the pack; the rest stays. The game stands a chest (`Prop::Cache`) and saves `Household.caches` (serde default, stamped with the world).
- Fall: a member Dead while a camp or post is engaged and the staging fight is not is carried home behind `FallFade` after `fade_s`: `injured_until` on the played clock, left at the bench place, the next able member out (`strata_sim::fall::able`), else the soonest to heal early. Carried items stay; wear runs as before. F5 is unchanged for the staging fight.
- Words: the arrival toast, the bench card's "Injured: back in 1 h 40 min" with Switch greyed, "a cache" on the place card, the F chip; all in `ui/strings.rs`.
- Stills (`docs/design/factions/c4-*`): cache, cache taken, fall home, bench injured. `--fall`, `--injured <member>`, `--cache`.
- Review fixes: one deposit search instead of one a class, then (mine) none at all when the kept list covers the camp, at 4 km; `in_the_world` is false while staging is engaged; the clock is saved with the injury; a record the catalogue refuses is dropped from a cache.

Decided for the user and easy to change: tier worths, 2 hours out, 1.2 s fade; alone on the bench the fallen member stands up whole; a death with nothing engaged keeps F5.

Left rough: the Dead-at-a-camp trigger through the schedule and the fade phases is proven by stills and pure tests only; the chest's Name is English in the game crate (as the harvester's); `--cache` and a clearing before the survey's list is made still search the field on the main thread (about 400 cells, once).

### Step 4b: postings and the heard-of map

- Data: `assets/procurement/postings.ron` (open 3, reach 20 km, a 24 h window, pay by tier, taken_max 3, heard 900 m with offset 0.6, seen 250 m), `strata_registry::posting`; `deeds.ron` gains `job_done` (no default in code: an older file writes no job deed). In no version, no handshake.
- The draw: `strata_sim::postings::offered` is a pure function of the seat, the window, the seed and the taken and cleared books, so every machine shows the same three. Camps are gathered once at `OnEnter(InWorld)` from the baked masses. First window on seed 42: Diderk Gully 900, Gakork Hide 900, Skan Cut 400 cr.
- Jobs: `TakeJob` and `Collect` in `ProcurementInbox` (`postings::consume`, before the procurement system), refusals as `JobRefusal`. A clearing at or after the take sets done; Collect pays the purse, writes `JobDone` for the board's anchor faction and removes the job.
- Found camps: `Known` is set when the member comes within `seen_m` or the camp's people are engaged (`find_camps`).
- Board: a WORK section first, rows with band, distance and direction (to the heard-of centre), pay and Take, Cleared with Collect; toasts on both; all words in `strings.rs`.
- Map: a camp shows only when found; a taken, unfound job shows a disc at the offset centre labelled heard of. The Holders tint leaves outlaw holdings out, so it cannot give a camp away.
- Saved in the household file with serde defaults, stamped with the world. Stills: `--take-job`, `--job-done`, `--found`; `docs/design/factions/c4-*.png`.
- Decided for the user and easy to change: pay by tier with no stance factor, TakeJob needs no stance, the heard offset is per camp not per window, the WORK section sits above the orders.
- Fixed after review (mine): a camp returns after 3 h, so `Jobs.collected_in` keeps the window each camp was paid in and the board does not post it again until the next window.
- Left rough: focus on opening the board starts on the first Sell.

### Step 5a: the wreck and the derelict machine

- Kinds: `assets/sites/wreck.site.ron` (`strata:wreck`, rank 7, track, 3.0 per 1000 km2) and `wreck_works.site.ron` (`strata:wreck_works`, rank 8, track, 1.5, spacing 16 km; the player words are Wreck and Derelict machine). Both ids sort after `strata:wild_camp` (a registry test holds the order), so no dense id moved.
- Groups (`veyra-settlements`): `veyra:wreck` has 4 designs of `HullSection` (rolled 24 degrees, pitched, axis 0.8 m under ground, rust plating), `HullPlate` (a slanted torn slab and a bent strip), containers, crates and a hut; `veyra:derelict_machine` has 3 designs of `BrokenMast` (a drum with a canted shaft cut off at a slant, no lamp or aerial), `Pipes`, hut or workshop, bollards. Both pass `tools/content/tests/sites.rs`.
- Holders: corporate holds wrecks (weight, 600 m) and machines; conservation holds machines (800 m). The old code let any held kind be a seat, which would have made wrecks seats and renamed old factions, so `HoldDef.seat` (default true, false on the three new holds) keeps them holders only; a factions test holds it.
- Version: `world_version` is `0xe3a4_b4f5_be38_2801` (was `0x7e0f_4a47_794f_227d`), two data files, no `GENERATOR_VERSION` bump, the seed 42 snapshot did not move. Within 40 km of Port Serein the 162 old sites and 37 camps are identical in kind, name, centre, key and holder (`scratch/combat/sites-before5.txt` against `sites-after5.txt`); 12 wrecks and 8 machines were added. Worldgen `sites` (6) and `roads` (8) suites pass with the new kinds in.
- Near the city: wrecks Hull of the Guco 14.2 km and The Kemon 17.9 km; machines Zesot Processor 20.7 km and Ardent Arotor 23.1 km; none within 3 km.
- Client: card "{name} / Wreck / Held by ...", map marks (wreck keeps the landmark's flag, machine the water mark, own legend row), worldmap `-sites` colour per kind (a ninth colour), `--at wreck` and `--at machine`. Stills `docs/design/factions/c5-*.png` (wreck, machine, cards, map, worldmap).
- Decided for the user and easy to change: wreck and machine show on the map from the first minute (the hide rule is "kinds an encounter names", so 5b hides them once they have encounters); no icon fits a wreck.
- Left rough: whether each machine stands on a river bank or a high shoulder was not measured (the job term is an any-of, so a machine on a coastal plain such as Zesot is allowed); the wreck still reads as a modest scatter of rust plates and containers; no wreck cache or people yet (5b).

### Step 5b: people, salvage and work at the wreck and the machine

- Crews: `EncounterDef.crew` (`strata_registry::Crew`, serde default `Holder`, else `Unaligned`). `encounters::refresh` gives an unaligned crew no band whoever holds the ground, so the game stands them as `Allegiance::Outlaw(None)` with no holder colour; no Hurt or Killed deed is written for them, clearing one writes the usual `ClearedCamp` deed. A test holds it.
- Encounter files: `wreck_scavengers` (`strata:wreck`, 280/440 m, returns 6 h; tier 1 two rushers, tier 2 a duelist, two rushers, a marksman, tier 3 two marksmen) and `machine_security` (`strata:wreck_works`, 260/420 m, returns 12 h; sentries and bulwarks only). Plates say "Scavenger" and "Security unit" (`strata.crew.<site kind>`); the machine's people wear `combat_art::MACHINE_STEEL`.
- Loot: `wreck.loot.ron` (cargo and components) and `machine.loot.ron` (few credits, blanks and coils on lines 400-950, modules at about twice a camp's chance). Worth at Pelagic's board, 600 seeds: camp 235/636/1364, wreck 325/824/1739, machine 102/267/542; the `loot` test holds bands for each and their order by tier. The chest is "Outlaw cache", "Salvage" or "Machine parts" by kind.
- Board: one candidate list, `Candidate.kind` and `Posting.kind` (serde default camp), an empty band for an unaligned crew. The head is "WORK"; the verb is "Clear {camp}", "Recover the cargo at {wreck}" or "Secure {machine}", with "held by {band}" for a camp and "unclaimed salvage" for the others, never the holder. The take toast is per kind; the refusal and the map's heard-of lines carry no kind word.
- Map: the hide rule was already "kinds an encounter names", so wrecks and machines are hidden until heard of or found, like camps. `--cache` and `clear_nearest` use the nearest place of any encounter file.
- Stills: `docs/design/factions/c5-scavengers.png`, `c5-security.png` (reshot with a steel-green body), `c5-salvage.png`, `c5-board-work.png` (seed 3).
- Decided for the user and easy to change: a machine's cache is worth under half a camp's at the board though its tier 3 roster is the heaviest and its job pays a camp's rate (the brief said fewer credits, fine parts; the bench worth of its high-line blanks was not measured); `--no-hostiles` removes only the staging fight, so camp, wreck and machine people stand anyway.
- Left rough: no `Secure {machine}` row in any still (no machine lies within Pelagic's 20 km on the seeds tried; a unit test holds the wording); no still shows the map hiding the new kinds, and 5a's `c5-map.png` is stale for that; the F chip says "Open the cache" for every kind; the security tint is subtle by the dark armour model; overlapping plates.
