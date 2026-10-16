# 0014: The first questline

Status: done 2026-10-15; step 1 (rules, data, people) and step 2 (words and screens) built. The user's answers are marked (user); everything
else is a default of mine, listed at the end and easy to change.
Singleplayer: nothing here touches the protocol or `apps/server`.

## What the user said (2026-10-15)

> lets work on the new player experience. it's difficult to get started, can
> we make a questline with some NPCs to get started?

Asked and answered:

- **About thirty minutes** (user): arrival, a first fight, survey and sample,
  make something at the bench, sell to Pelagic, buy the bike. It ends with the
  player able to run the loop alone.
- **The monitoring station** (user): docs/SETTING.md's seed. A failed station
  whose repair turns up missing equipment, a settlement's unauthorised water
  diversion and an ecological change somebody would rather keep quiet.
- **Branching dialogue** (user): real branches with different outcomes and
  endings, not flavour replies.
- **Arrive in the city, nothing locked** (user): a new household starts on
  Port Serein's quay with the first person beside it. The questline guides; every
  system stays open and the questline can be skipped.

## The story

Three people in Port Serein, one for each kind of institution, and two out in
the basin. The factions are procedural (ticket 0012), so a line names them
through placeholders: `{holder}` is the city's holder (Pelagic Works on seed
42), `{assembly}` and `{keepers}` the nearest assembly and conservation
factions to the city. Names below are mine.

- **Maren Coyle**, arrivals warden for `{assembly}`, on the quay. Warm, busy,
  practical; she has a hundred settlers a week to place.
- **Dr Anouk Teague**, field officer for `{keepers}`, at the Exchange. Precise,
  short on patience, her station has gone dark and nobody will lend her hands.
- **Corin Hale**, works steward for `{holder}`, by Pelagic's board. Friendly,
  generous, always speaking for the company.
- **Pell**, who leads the scavengers stripping the station. Not an outlaw, not
  a friend: a living.
- **Wim Larrow**, a borough grower upstream whose orchard is dying, and who has
  cut a channel off the river to save it.

### Act 1: landfall (about 8 minutes)

1. **Landfall.** Maren meets the household on the quay: who they are, the
   bench (who goes out), the map. Replies can ask about work, the city or the
   factions. Her last line sends the player to Teague, who "needs someone
   with a steady hand". A reply here is **Skip**: "We've done this before".
   It ends the questline with the starting rewards and a journal line naming
   where everything is.
2. **A steady hand.** Teague will not send a stranger out past the outlaws on
   the road. Prove it at the training ground (the staging fight): beat the
   sentry, and learn one box from a trainer (the sheet, C). Teaches combat, the
   bar and learning. Replies decide whether the player asks about `{holder}`.
   If they do, Teague says the processor's intake is "drinking the river", and
   that opens a line later.

### Act 2: the quiet station (about 14 minutes)

3. **The quiet station.** The station is out past the road's end, too far to
   walk comfortably. Corin, unasked, offers a bike from the board at a
   newcomer's price, and the player can buy it there (V to call it). It is
   optional; walking works. At the station, Pell's crew is stripping the mast.
   **Branch A, Pell:**
   - *Fight*: the crew turns hostile; the data core is in the cache after.
     `{assembly}` hears of it (a Hurt or Killed deed against the unaligned).
   - *Pay* (200 credits): Pell hands over the core and leaves.
   - *Talk* (needs Teague's warning about the intake, or a second try at
     persuading): Pell hands over the core free and adds that the station's
     sensor array was gone before they came, "sold to `{holder}`'s yard".
     That is the evidence of the missing equipment.
4. **Mending the mast.** The core is whole but the mast's harness is burnt
   out. Find and sample 45 units of a conductor near the station (G: ping,
   track, sample; Ilse Marrow, a recruit at the terminal, is the surveyor),
   then make a harness at the workbench (the crafting tab; Tovan Reyes, the
   other recruit, is the maker). Fit it at the mast (F). Teaches the survey,
   recruiting and switching at the bench, and crafting. The
   restored station reads the river: the flow below a bend has fallen by a
   third, and the water downstream is turning brackish.

### Act 3: upstream (about 8 minutes)

5. **Upstream.** The record points to a cut in the bank upstream. Wim Larrow
   is there with a pump and a dying orchard. He explains: the `{assembly}`
   allotment shrank when the processor's intake opened, so he takes what he
   needs. The station's readings show his channel takes about a fifth of what
   went missing; the rest goes into the intake.
   **Branch B, Wim:**
   - *Shut the pump* (F at the pump): Wim's channel closes. `{assembly}`
     resents it, `{keepers}` approves.
   - *Leave it*: nothing changes at the channel.
   - *Threaten him*: he fights; a Killed deed against `{assembly}`.
6. **Who hears it.** The player carries the station's record back to the city
   and chooses who gets it. This is the ending:
   - **Teague (`{keepers}`)**: the record is published. The diversion must
     close, and if Pell said where the array went, `{holder}` is named for
     taking it. Reward: Survey XP and an improved survey tool. Standing
     rises with `{keepers}` and falls with `{holder}`; if Wim's pump still runs,
     it falls a little with `{assembly}`.
   - **Maren (`{assembly}`)**: the record goes to the borough first. Wim's
     channel is put before the Assembly and made lawful. Reward: a field
     harvester (teaches planting). Standing rises with `{assembly}`; Teague is
     cool afterwards.
   - **Corin (`{holder}`)**: the record is sold to the company. They pay well
     (sold at the board, which teaches selling) and the station "keeps
     reporting" on Pelagic's firmware. Reward: credits and better board
     terms. Standing rises with `{holder}` and falls with `{keepers}`; Teague
     will not speak to the household about the station again.
   Each ending has its own closing scene with the person chosen and a line
   from each of the other two the next time the player meets them. The last
   journal entry points at the board's postings, the deposits and the
   fabricator: the loop to run alone.

The deeds are the ledger of ticket 0012 and stay hidden; what the player sees
is how people speak to them, the board's terms and the guards.

## The design

1. **Data, no code per quest.** `assets/people/*.person.ron`: a person's name,
   faction role (`holder`, `assembly`, `keepers`, `unaligned`), body tint,
   where they stand (a named place: `quay`, `exchange`, `board`, or a quest
   place) and which conversation they open by condition.
   `assets/dialogue/*.dialogue.ron`: a graph of nodes, each a speaker and a
   line, with replies that carry conditions (a quest's stage, a flag, the
   purse, an item held, a learned box) and effects (set a flag, start or
   advance a quest, give or take credits or items, a deed, turn an encounter
   hostile, end). `assets/quests/*.quest.ron`: stages, each with a journal
   line, objectives and what completes it. Loaded and checked by
   `strata_registry::quest` (no Bevy): every node reachable, every reply's
   target exists, every stage reachable, every ending reachable, every
   placeholder known. In no world version and no handshake.
2. **Objectives are sim events.** `strata_sim::quests` holds the `QuestLog`
   (stages, flags, the choices made), consumes a `QuestEvent` the existing
   answers raise (talked, defeated, learned, pinged, sampled, crafted, fitted,
   sold, bought, stabled, reached a place, delivered, cleared) and advances
   stages. A conversation is `DialogueRequest::{Open, Choose}` in
   `DialogueInbox`, consumed in `SimSet::Input`. `replies` (which are shown,
   which are greyed and why) and `apply` are the one rule for the screen and
   the request. The game saves the log in the household file with a serde
   default, as ticket 0012 did, so an old file reads with an empty log and no
   version changes; an old household is offered the questline at Maren's
   desk on the quay.
3. **Quest places are found, not baked.** The station and Wim's channel are
   placed by a pure function of the seed and the world when the quest starts:
   the station on dry ground past the end of Port Serein's road, the channel
   on the nearest river bank upstream of it, within the 30 minute budget's
   distances (station about 900 m from the quay, the channel 400 to 800 m
   beyond). Not a site kind, so `world_version` does not move. The station
   stands an existing baked group (the derelict machine, or a mast built from
   the roadside kit) and Pell's crew is an encounter with
   `EncounterId::Quest`.
4. **People are bodies.** The game stands each person as the humanoid body
   the hostiles use, on the neutral team, with a name plate and
   `InteractionKind::Person { id }` (F talks). They idle; they do not walk
   yet. A new household (`--fresh` or no save) spawns on the quay with Maren
   two metres away. `--at` and an old save start where they did.
5. **Screens.** `ui::dialogue` (kind `strata:dialogue`): the speaker, the
   line, the replies keyed 1 to 4, a greyed reply with its reason.
   `ui::journal` (J, and a pause button): the active stage, its objectives
   ticked, past choices in a line each. The HUD tracker shows the current
   objective's marker and distance (`ui::tracker`); the map marks it.
6. **Words live in data.** All the questline's English is in the dialogue
   and quest files; the client's English is only the screens' chrome.
7. **Proof.** `--script-quest <ending>` walks the whole questline headless for
   each ending and each Pell branch, and a test in `apps/game` runs them.
   `--quest-stage <quest>:<stage>` starts at a stage, `--open-dialogue
   <person>` and `--open-journal` are for stills.

## The plan

1. **Rules, data and people** (one workflow): (a) the three file kinds and
   their checks, `QuestLog`, `QuestEvent`, the dialogue requests, the effects,
   the quest places and the questline's structure with draft lines; (b) the
   people in the world, F to talk, the quay start, the station and the
   channel, Pell's encounter, the events raised from every existing answer,
   the save, and `--script-quest`.
2. **Words and screens** (one workflow): (a) the questline written in full and
   every branch walked by the script, the timings checked against thirty
   minutes; (b) dialogue, journal, tracker and map marks, stills of each
   scene.

## Defaults of mine

- The names, the cast and the three endings above.
- People stand still; walking NPCs are later.
- Skip gives the starting rewards (no bike, no harvester).
- Pay costs 200 credits; the endings' rewards are sized in step 4 against the
  first-hour model.
- No voice; sounds only for opening and closing a conversation.

## As built

### Step 1: rules, data and people

- Files: `assets/quests/arrival.quest.ron` (one file, eight stages, four endings), `places.ron` (the dials),
  `assets/dialogue/*.dialogue.ron` (a file a person, plus the props), `assets/people/*.person.ron` (five people and
  two thing-people, the mast and Wim's pump). `strata_registry::quest` loads and checks them into
  `GameRegistries.quests`; in no world version and no handshake.
- Sim: `strata_sim::quests` has `QuestLog`, `QuestEvent`, `advance`, `DialogueRequest` in `DialogueInbox` (reach of
  the person required), `DialogueView`, `replies`, `apply`, `fill` and `QuestCast`. `Reach.people` and
  `InteractionKind::Person { id }` carry F. `EncounterId::Quest` stands Pell's crew neutral until `Hostile`.
- Places (`quests::places`, a pure function of the world, 60 ms on seed 42): the station stands 2250 m from Port
  Serein and Wim's channel 778 m from it, upstream where the river's flow allows. The ticket's 900 m station and
  400 to 800 m channel cannot both hold: the only river lies about 2.4 km from the city.
- Game: `household/people.rs` stands each person as a body with a plate and F; a new household starts on the quay
  facing Maren; `QuestLog` is saved in the household file as a serde default. F at a person logs the `DialogueView`
  until step 2. A stage asking for Recruit or Switch counts a member already on the bench.
- Script: `--script-quest <keepers|assembly|holder>`, `--script-pell <fight|pay|talk>` and `--quest-stage
  <quest>:<stage>`; a test in `apps/game` runs all three endings with the three branches spread over them. Played
  minutes per ending (keepers, assembly, holder), walking / sprinting / on the bike: 53.7/35.3/17.6,
  53.3/34.9/17.2, 53.4/35.1/17.3 over about 17 km. The 30 minute budget fails on foot because the station is far.
- Left rough: the station and pump are box props, not baked groups; the crew's literals (`STAND_M` and kin, the
  "Salvager" plate) are in code, not data; a crew that was crossed stands neutral again after folding; a Give into a
  full pack is dropped; the `Fit`, `Buy`, `Sell` and `Stable` objectives are used by no stage; the script writes the
  Learned event and kills itself; `quay_point` has no fallback or test; the keepers' reward is a store-grade tool.

### Step 2: words and screens

- The words: every line, reply and journal line is rewritten in `assets/dialogue/*.dialogue.ron`, `assets/people/*` and
  `assets/quests/arrival.quest.ron` (Maren warm and tired, Teague dry, Corin easy, Pell blunt, Wim worn). Lines are at
  most three sentences and 240 characters, replies at most 70, factions are `{holder}`, `{assembly}`, `{keepers}`, the
  player speaks as "we". Each branch and ending has a closing scene and an after line for the two not chosen, with
  variants for a dead Wim (`wim_fallen`) and a shut pump. Skip names where the board, bench and trainers are.
- Route: the bike is the way out. Quiet station asks Talk, Buy (the courier bike at the board), Stable and Reach. The
  newcomer price is a one-off 150 credit from Corin (`Earn`, flag `newcomer_credit`), not a board term: the bike
  costs 300 net. Keepers: a survey tool at Range 700 (store grade 500) and 300 Survey XP. Assembly: a field harvester.
  Company: `veyra:item.station_record`, sold once at the board for 350 by a Project line on Pelagic's file.
- Sizing: runs end at 2423 credits and a stabled bike (keepers, assembly) and 2573 (company), against 2390, a bike and
  a harvester at minute 25 of `docs/balance/first-hour.md`: level, plus one reward worth about 300.
- Roughness fixed: the mast takes only a pack harness with a batch and a maker, once (`harness_fitted`); a Give checks
  room and its reply is greyed, never hidden, when full; crossings are kept in `QuestLog.hostile` and stand again;
  the crew literals are in `places.ron` and the crew file; `quay_point` falls back to the Exchange and a seed 42 test
  holds it dry, paved and clear; the Fit objective is removed (the mast keeps Deliver); Learned, Recruited and
  Switched have unit tests.
- Screens: `ui::dialogue` (F at a person, keys 1 to 4, greyed reasons, Leave and Escape send Close), `ui::journal` (J and
  the pause), a tracker marker with the distance and a map mark (`MarkKey::Quest`, tested) for the first unmet
  objective with a place; a Sample leads only to a found deposit. `--open-dialogue <person>` starts by that body,
  `--open-journal` with `--quest-stage`.
- Played minutes walking / sprinting / bike over about 17.2 km: keepers 53.8/35.4/17.7, assembly 53.6/35.3/17.5,
  company 53.6/35.2/17.4. The three endings run in one game test (about 430 s).
- Stills: `docs/design/quests/` (arrival, Maren, Teague's test and mission, Corin's bike, Pell, the mast, Wim, the pump,
  the board, the map, the journal, the tracker, the three endings).
- Left rough: the three ending stills predate the last edits to the closing lines; the skip has no still of its own
  (Maren's hub shows its reply); the board still shows the record row cut at the fold; the map mark is the objective
  icon without a legend row; the board lists the station record Project line from the first minute; a step 1 save
  mid `quiet_station` or with Wim killed should be restarted with `--quest-stage`; the keyboard focus ring at 1080p
  is slotted's.
- Ticket 0015 moved the mending stage to a field camp beside the mast and brought the station and channel nearer the city.
