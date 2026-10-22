//! Lots, the survey register, the tracker and the place card
//! (ticket 0007, step 3).

use crate::ui;
use bevy::math::Rect;
use bevy::prelude::*;
use slotted::ui::InputMode;
use slotted::ui::UiAction;
use slotted::ui::def::Tags;
use slotted_test::prelude::*;
use strata_registry::GameRegistries;
use veyra_sim::WorldPosition;
use veyra_sim::deposits;
use veyra_sim::deposits::Deposit;
use veyra_sim::deposits::DepositId;
use veyra_sim::deposits::LastPing;
use veyra_sim::deposits::NearDeposits;
use veyra_sim::deposits::Pinging;
use veyra_sim::deposits::Purse;
use veyra_sim::deposits::SurveyInbox;
use veyra_sim::deposits::SurveyRequest;
use veyra_sim::deposits::Surveyed;
use veyra_sim::items::HouseholdStore;
use veyra_sim::progression::Interact;
use veyra_sim::progression::Interaction;
use veyra_sim::progression::InteractionKind;
use veyra_ui::bench;
use veyra_ui::card::BarRange;
use veyra_ui::card::CardParams;
use veyra_ui::card::ItemCard;
use veyra_ui::place;
use veyra_ui::stores;
use veyra_ui::stores::SlotDressing;
use veyra_ui::stores::StoresState;
use veyra_ui::survey;
use veyra_ui::tracker::TrackerMarker;

use crate::common::*;

/// A deposit of `ty` named `root` in `cell`, at `at` and with `figures` in its class's order: what the world's query
/// would give, made by hand so no world need be baked.
pub(crate) fn deposit_of(
    registries: &GameRegistries,
    ty: &str,
    root: &str,
    cell: [i32; 2],
    at: [f64; 2],
    figures: &[u16],
) -> Deposit {
    let set = &registries.resources;
    let ty = set.type_id(&ty.parse().unwrap()).expect("a shipped type");
    let def = set.get(ty).unwrap();
    let carries = &set.class_of(ty).carries;
    assert_eq!(carries.len(), figures.len(), "a figure a carried attribute");
    Deposit {
        id: DepositId(cell),
        ty,
        root: root.to_owned(),
        place: format!("{root} {}", def.grounds[0]),
        at,
        height_m: 10.0,
        figures: carries
            .iter()
            .copied()
            .zip(figures.iter().copied())
            .collect(),
        radius_m: 200.0,
        peak: 800,
        grade_pm: 500,
    }
}

/// The deposits of a survey day, by what they are for.
pub(crate) struct Ground {
    /// Evaporite salt its post within reach.
    pub(crate) calder: Deposit,
    /// Light alloy ore half a kilometre south.
    pub(crate) rake: Deposit,
    /// Evaporite salt again, on another deposit, to the north-west.
    pub(crate) harrow: Deposit,
    /// Sulphur far to the west.
    pub(crate) sorrel: Deposit,
}

/// [`first_day`] with 2,500 credits, four deposits published as near and
/// the survey post of the nearest within reach: what the game hands the
/// sim at a deposit, without a world.
pub(crate) fn survey_day(name: &str, width: f32, height: f32) -> (Day, Entity, Ground) {
    let mut day = first_day(name, width, height);
    let registries = day.registries.clone();
    let ground = Ground {
        calder: deposit_of(
            &registries,
            "strata:evaporite_salt",
            "Calder",
            [0, 0],
            [0.0, 1.5],
            &[610, 820, 240],
        ),
        rake: deposit_of(
            &registries,
            "strata:light_alloy_ore",
            "Rake",
            [0, -1],
            [0.0, -510.0],
            &[610, 540, 560, 410, 520, 700, 320, 760],
        ),
        harrow: deposit_of(
            &registries,
            "strata:evaporite_salt",
            "Harrow",
            [1, 1],
            [900.0, 900.0],
            &[300, 410, 660],
        ),
        sorrel: deposit_of(
            &registries,
            "strata:sulphur",
            "Sorrel",
            [-3, 0],
            [-2400.0, 0.0],
            &[640, 940, 300],
        ),
    };
    let mut deposits = vec![
        ground.calder.clone(),
        ground.rake.clone(),
        ground.harrow.clone(),
        ground.sorrel.clone(),
    ];
    deposits.sort_by_key(|deposit| deposit.id);
    day.h.world_mut().insert_resource(NearDeposits {
        deposits,
        around: vec![[0.0, 0.0]],
    });
    day.h.world_mut().insert_resource(Purse(2500));
    // Every deposit is found, and the first sampled: the household has
    // pinged them all and drawn from Calder flats.
    let found = {
        let near = day.h.world().resource::<NearDeposits>();
        near.deposits
            .iter()
            .map(|deposit| {
                let sampled = deposit.id == ground.calder.id;
                (
                    deposit.id,
                    deposits::Found {
                        reading: strata_registry::Reading::Exact,
                        sampled,
                        best_pm: deposit.peak,
                        best_at: deposit.at,
                    },
                )
            })
            .collect()
    };
    day.h.world_mut().resource_mut::<Surveyed>().found = found;
    // A locker beside the player, so the prompt chip has something to say.
    let post = day
        .h
        .world_mut()
        .spawn((
            WorldPosition::new(0.0, 10.0, 1.5),
            Interaction {
                kind: InteractionKind::Locker,
                radius: 2.5,
            },
        ))
        .id();
    // A tick, so the sim's reach has found it.
    day.h.advance(std::time::Duration::from_millis(120));
    day.h.settle();
    assert!(
        day.h
            .world()
            .resource::<veyra_sim::progression::Reach>()
            .nearest
            .is_some()
    );
    (day, post, ground)
}

impl Day {
    pub(crate) fn purse(&self) -> u32 {
        self.h.world().resource::<Purse>().0
    }

    /// What each toast on screen says, oldest first.
    pub(crate) fn toasted(&mut self) -> Vec<String> {
        let toasts = self.h.toasts();
        toasts
            .into_iter()
            .map(|toast| {
                let world = self.h.world();
                let mut said = String::new();
                let mut below = vec![toast];
                while let Some(at) = below.pop() {
                    if let Some(text) = world.get::<Text>(at) {
                        said.push_str(&text.0);
                    }
                    if let Some(span) = world.get::<bevy::text::TextSpan>(at) {
                        said.push_str(&span.0);
                    }
                    if let Some(children) = world.get::<Children>(at) {
                        below.extend(children.iter().rev());
                    }
                }
                said
            })
            .collect()
    }

    /// The card under `id`, as it was last painted.
    pub(crate) fn card_at(&self, id: &str) -> CardParams {
        let card = find(&self.h, id);
        self.h.world().get::<ItemCard>(card).unwrap().0.clone()
    }

    /// The bands drawn under the card `id`: the floor and cap of each.
    pub(crate) fn bands(&self, id: &str) -> Vec<(u16, u16)> {
        let world = self.h.world();
        let mut found = Vec::new();
        let mut below = vec![find(&self.h, id)];
        while let Some(at) = below.pop() {
            if let Some(band) = world.get::<BarRange>(at) {
                found.push((band.0, band.1));
            }
            if let Some(children) = world.get::<Children>(at) {
                below.extend(children.iter().rev());
            }
        }
        found
    }

    /// Tab until the focus is on the control `id`.
    pub(crate) fn tab_to(&mut self, id: &str) {
        for _ in 0..90 {
            if self.h.focused().is_some() && self.h.focused() == self.h.try_find(&by::test_id(id)) {
                return;
            }
            self.h.focus_next();
            self.h.settle();
        }
        panic!(
            "Tab did not reach {id}, and is on {}",
            self.named(self.h.focused())
        );
    }

    /// Every control Tab stops on in one round of the screen.
    pub(crate) fn round(&mut self) -> Vec<String> {
        let from = self.h.focused();
        let mut stops = vec![self.named(from)];
        for _ in 0..120 {
            self.h.focus_next();
            self.h.settle();
            if self.h.focused() == from {
                return stops;
            }
            stops.push(self.named(self.h.focused()));
        }
        panic!("Tab does not come round: {stops:?}");
    }

    /// The family a button-like card is painted from.
    pub(crate) fn family_of(&self, id: &str) -> String {
        self.h
            .world()
            .get::<slotted::ui::ButtonRole>(find(&self.h, id))
            .expect("a card that is a button")
            .0
            .as_str()
            .to_owned()
    }
}

/// The survey register, the terminal's second tab: every deposit near,
/// nearest first, each with its class and type, its best line
/// and where it lies, a disc on the ones sampled, a square on those read and a ring on the rest.
/// The row in focus shows the card of a lot drawn there beside the list;
/// Accept on it tracks it, which the row then says, and the marker on the
/// HUD is there when the screen has closed. Accept on the tracked row lets
/// it go.
#[test]
#[allow(clippy::too_many_lines)]
fn the_register_tracks_by_focus_and_accept() {
    let (mut day, _, ground) = survey_day("register", 1600.0, 900.0);
    let (calder, rake, harrow, sorrel) = (
        ground.calder.id,
        ground.rake.id,
        ground.harrow.id,
        ground.sorrel.id,
    );
    let terminal = day
        .h
        .world_mut()
        .spawn((
            WorldPosition::new(-1.0, 10.0, 0.0),
            Interaction {
                kind: InteractionKind::Terminal,
                radius: 3.0,
            },
        ))
        .id();
    day.h.world_mut().write_message(Interact {
        entity: terminal,
        kind: InteractionKind::Terminal,
    });
    day.h.step(2);
    day.h.settle();
    assert_eq!(day.h.stack_top(), Some(bench::kind()));
    assert_eq!(day.says(bench::TITLE_ID), "The bench");
    assert_eq!(
        texts_under(&day.h, find(&day.h, &bench::BenchTab::Bench.tab_id())),
        ["\u{203a} BENCH"],
        "the tab shown is marked, not only lit"
    );
    // The tab is reached and shown by the keys alone, and keeps the
    // keyboard across the rebuild it caused.
    day.h.set_input_mode(InputMode::Keyboard);
    day.h.settle();
    day.tab_to(&bench::BenchTab::Register.tab_id());
    day.h.action(UiAction::Accept);
    day.h.settle();
    assert_eq!(day.h.stack_top(), Some(bench::kind()));
    assert_eq!(day.says(bench::TITLE_ID), "The survey register");
    assert_eq!(
        texts_under(&day.h, find(&day.h, &bench::BenchTab::Register.tab_id())),
        ["\u{203a} SURVEY REGISTER"]
    );
    assert_eq!(
        day.h.focused(),
        Some(find(&day.h, &bench::BenchTab::Register.tab_id())),
        "the tab keeps the keyboard"
    );
    assert_eq!(day.says(bench::CAPTION_ID), "ALL 4 WITHIN 8 KM");
    assert_eq!(day.says(bench::COUNT_ID), "4 found · 1 sampled");

    // Nearest first, down the list.
    let order = [calder, rake, harrow, sorrel];
    let tops: Vec<f32> = order
        .iter()
        .map(|id| day.h.rect_of(find(&day.h, &bench::row_id(*id))).min.y)
        .collect();
    assert!(tops.windows(2).all(|pair| pair[0] < pair[1]), "{tops:?}");
    assert_eq!(
        day.row(&bench::row_id(rake)),
        [
            "Rake scar",
            "STRUCTURAL METAL · Light alloy ore · Read",
            "ST 540",
            "510 m south",
            ""
        ]
    );
    assert_eq!(
        day.row(&bench::row_id(sorrel)),
        [
            "Sorrel vents",
            "REACTIVE · Sulphur · Read",
            "PT 940",
            "2.4 km east",
            ""
        ]
    );
    assert_eq!(day.row(&bench::row_id(harrow))[3], "1.3 km north-west");
    // The one sampled has a disc, one read a square: a shape, and a word
    // on the node for a reader that cannot see either.
    let mark = |day: &Day, id: DepositId| {
        let mark = find(
            &day.h,
            &format!("{}{}", bench::MARK_TAG, bench::deposit_tag(id)),
        );
        let world = day.h.world();
        (
            world
                .get::<slotted::theme::Themed>(mark)
                .unwrap()
                .0
                .as_str()
                .to_owned(),
            world
                .get::<Tags>(mark)
                .and_then(|tags| tags.get("state").map(str::to_owned)),
        )
    };
    assert_eq!(
        mark(&day, calder),
        (
            "strata.register.mark".to_owned(),
            Some("Sampled".to_owned())
        )
    );
    assert_eq!(
        mark(&day, rake),
        (
            "strata.register.mark.read".to_owned(),
            Some("Read".to_owned())
        )
    );

    // With nothing looked at yet the card is the nearest row's, and that
    // row is the one marked.
    assert_eq!(day.card_at(bench::REGISTER_CARD_ID).name, "Calder salt");
    assert_eq!(day.row(&bench::row_id(calder))[0], "\u{203a} Calder flats");

    // By the keys: the focus on a row shows its card beside the list.
    day.h.set_input_mode(InputMode::Keyboard);
    day.h.settle();
    let stops = day.round();
    for id in order.map(bench::row_id).into_iter().chain([
        bench::BenchTab::Bench.tab_id(),
        bench::BenchTab::Register.tab_id(),
        bench::class_id(None),
        bench::CLOSE_ID.to_owned(),
    ]) {
        assert!(stops.contains(&id), "Tab misses {id}: {stops:?}");
    }
    day.tab_to(&bench::row_id(harrow));
    let card = day.card_at(bench::REGISTER_CARD_ID);
    assert_eq!(card.name, "Harrow salt");
    assert_eq!(card.from, "Harrow flats");
    assert_eq!(
        day.bands(bench::REGISTER_CARD_ID),
        [(200, 900), (300, 950), (100, 700)]
    );
    assert_eq!(day.row(&bench::row_id(harrow))[0], "\u{203a} Harrow flats");
    assert_eq!(day.row(&bench::row_id(calder))[0], "Calder flats");
    // Two deposits of one type differ on the card: the same ranges, their
    // own figures.
    let harrow_lines: Vec<u16> = card.ranges.iter().map(|range| range.value).collect();
    assert_eq!(harrow_lines, [300, 410, 660]);
    day.tab_to(&bench::row_id(rake));
    let card = day.card_at(bench::REGISTER_CARD_ID);
    assert_eq!(card.name, "Rake light alloy");
    assert_eq!(card.ranges.len(), 8, "the long card: every line it carries");
    assert_eq!(day.bands(bench::REGISTER_CARD_ID).len(), 8);

    // Accept tracks the row in focus.
    day.tab_to(&bench::row_id(harrow));
    assert_eq!(day.h.world().resource::<Surveyed>().tracked, None);
    day.h.action(UiAction::Accept);
    day.tick();
    assert_eq!(
        day.h
            .world()
            .resource::<Surveyed>()
            .tracked
            .as_ref()
            .map(|deposit| deposit.id),
        Some(harrow),
        "what is tracked is the sim's"
    );
    assert_eq!(day.row(&bench::row_id(harrow))[4], "TRACKED");
    assert_eq!(
        day.family_of(&bench::row_id(harrow)),
        "strata.register.row.on"
    );
    assert_eq!(day.family_of(&bench::row_id(rake)), "strata.register.row");
    assert_eq!(
        day.h.focused(),
        Some(find(&day.h, &bench::row_id(harrow))),
        "the row keeps the keyboard across the rebuild"
    );
    assert_eq!(day.card_at(bench::REGISTER_CARD_ID).name, "Harrow salt");

    // The marker: hidden under the screen, and there with the deposit's
    // name and its distance once the screen has closed.
    let marker = |day: &mut Day| {
        let world = day.h.world_mut();
        let mut markers = world.query::<(Entity, &TrackerMarker)>();
        let (entity, marker) = markers
            .iter(world)
            .find(|(_, marker)| !marker.lead)
            .expect("the tracked marker");
        let words = world.get::<Text>(marker.text).unwrap().0.clone();
        (entity, words)
    };
    let (entity, _) = marker(&mut day);
    assert!(!day.h.is_visible(entity), "no marker over a screen");
    day.close();
    assert_eq!(day.h.stack_top(), None);
    day.h.step(2);
    let (entity, words) = marker(&mut day);
    assert!(day.h.is_visible(entity), "tracking survives the screen");
    assert_eq!(words, "Harrow flats · 1.3 km");
    let rect = day.h.rect_of(entity);
    assert!(
        within(rect, Rect::new(0.0, 0.0, 1600.0, 900.0)),
        "the marker is held inside the screen: {rect:?}"
    );

    // From the pause the register reads the same, and Accept on the
    // tracked row lets it go.
    let mut commands = day.h.world_mut().commands();
    bench::open_on(&mut commands, false, bench::BenchTab::Register);
    day.h.world_mut().flush();
    day.h.settle();
    assert_eq!(day.says(bench::TITLE_ID), "The survey register");
    assert_eq!(
        day.card_at(bench::REGISTER_CARD_ID).name,
        "Harrow salt",
        "the tracked row's card as the tab opens"
    );
    assert_eq!(
        day.h.focused(),
        Some(find(&day.h, &bench::row_id(harrow))),
        "and the keyboard on it"
    );
    day.h.action(UiAction::Accept);
    day.tick();
    assert_eq!(day.h.world().resource::<Surveyed>().tracked, None);
    assert_eq!(day.row(&bench::row_id(harrow))[4], "");
    assert_eq!(day.family_of(&bench::row_id(harrow)), "strata.register.row");

    // Two presses before the screen has heard of the first are a track
    // and a release: which a press is, is the sim's to say.
    let row = find(&day.h, &bench::row_id(harrow));
    for _ in 0..2 {
        day.h.world_mut().write_message(slotted::menu::MenuChoice {
            id: bench::row_id(harrow),
            screen: bench::kind(),
            entity: row,
        });
    }
    day.tick();
    assert_eq!(day.h.world().resource::<Surveyed>().tracked, None);
    assert_eq!(day.row(&bench::row_id(harrow))[4], "");

    // A class chip lists that class alone. By the keys: Tab reaches it,
    // Accept sets it, and it keeps the keyboard and takes the mark.
    let metal = bench::class_id(Some(&"strata:structural_metal".parse().unwrap()));
    day.tab_to(&metal);
    day.h.action(UiAction::Accept);
    day.h.settle();
    assert_eq!(day.h.focused(), Some(find(&day.h, &metal)));
    assert_eq!(
        texts_under(&day.h, find(&day.h, &metal)),
        ["\u{203a} STRUCTURAL METAL"]
    );
    assert_eq!(
        texts_under(&day.h, find(&day.h, &bench::class_id(None))),
        ["ALL"]
    );
    assert_eq!(day.says(bench::CAPTION_ID), "ALL 1 WITHIN 8 KM");
    assert_eq!(day.says(bench::COUNT_ID), "1 found · 0 sampled");
    assert!(
        day.h
            .try_find(&by::test_id(&bench::row_id(harrow)))
            .is_none()
    );
    assert_eq!(
        day.card_at(bench::REGISTER_CARD_ID).name,
        "Rake light alloy"
    );
    day.close();
    day.h.step(2);
    let (entity, _) = marker(&mut day);
    assert!(!day.h.is_visible(entity), "nothing tracked, no marker");
}

/// Lots on the stores screen: a lot's cell counts its units and its pip
/// says where it stands in its type's ranges, the Lots chip dims the rest,
/// the pinned card is the lot's own, and the head shows the household's
/// credits and follows the purse.
#[test]
fn the_stores_show_lots_the_lots_chip_and_the_purse() {
    let (mut day, _, ground) = survey_day("stores-lots", 1600.0, 900.0);
    let registries = day.registries.clone();
    let items = &registries.items;
    let salt = deposits::lot(&ground.calder, &registries.resources, 160).unwrap();
    let other = deposits::lot(&ground.harrow, &registries.resources, 160).unwrap();
    // After the first day's stock, in the store's next two cells.
    let first = day
        .h
        .world()
        .resource::<HouseholdStore>()
        .0
        .slots()
        .iter()
        .flatten()
        .count();
    {
        let mut store = day.h.world_mut().resource_mut::<HouseholdStore>();
        for (offset, lot) in [&salt, &other].into_iter().enumerate() {
            let stack = veyra_sim::items::write(lot, items).unwrap();
            store.0.set(first + offset, Some(stack));
        }
        store.0.clear_changed();
    }
    day.use_locker();
    day.h.step(3);
    day.h.settle();
    assert_eq!(day.says(stores::PURSE_ID), "2,500");
    assert!(
        texts_under(&day.h, find(&day.h, stores::ROOT_ID))
            .iter()
            .any(|text| text == "CREDITS")
    );
    let (calder, harrow) = (day.cell("store", first), day.cell("store", first + 1));
    let lot_id = day.id("strata:lot.evaporite_salt");
    assert_eq!(day.shows(calder), Some((lot_id, 160)), "its count is units");
    assert_eq!(day.shows(harrow), Some((lot_id, 160)));
    // Two lots of one type tell apart by their pips: where each stands in
    // the type's ranges, which is the sim's figure.
    let grade = |day: &Day, slot: Entity| day.h.world().get::<SlotDressing>(slot).unwrap().grade;
    assert_eq!(
        grade(&day, calder),
        Some(veyra_sim::items::grade_of(items, &salt))
    );
    assert_eq!(
        grade(&day, harrow),
        Some(veyra_sim::items::grade_of(items, &other))
    );
    assert_ne!(grade(&day, calder), grade(&day, harrow));

    // Focus on the lot pins its card: the lot's form, with its ranges.
    day.h.set_input_mode(InputMode::Keyboard);
    day.h.settle();
    // The first day's stock fills a row of the store, so the lots stand
    // under its first cell: in along the row, then down.
    let first_cell = day.cell("store", 0);
    day.walk_to(UiAction::Right, first_cell, 12);
    day.walk_to(UiAction::Down, calder, 2);
    let card = day.card();
    assert_eq!(card.name, "Calder salt");
    assert_eq!(card.from, "Calder flats · 160 units");
    assert_eq!(
        day.bands(stores::CARD_ID),
        [(200, 900), (300, 950), (100, 700)]
    );
    assert_eq!(day.says(stores::SENDS_ID), "Sends Calder salt x160");

    // The Lots chip leaves the lots at full strength and dims the rest.
    let dimmed = |day: &Day, slot: Entity| day.h.world().get::<SlotDressing>(slot).unwrap().dimmed;
    assert!(!dimmed(&day, day.cell("store", 0)));
    let chip = find(&day.h, &stores::Filter::Lots.chip_id());
    day.h.click(chip);
    day.h.step(2);
    day.h.settle();
    assert_eq!(
        day.h.world().resource::<StoresState>().filter,
        stores::Filter::Lots
    );
    assert!(dimmed(&day, day.cell("store", 0)), "a module is no lot");
    assert!(!dimmed(&day, calder) && !dimmed(&day, harrow));
    assert_eq!(
        day.family_of(&stores::Filter::Lots.chip_id()),
        "strata.stores.chip.on"
    );
    // The chip in force is marked in a word's place, not by its fill
    // alone, and the one it took over from has lost the mark.
    let chip_says = |day: &Day, filter: stores::Filter| {
        texts_under(&day.h, find(&day.h, &filter.chip_id())).concat()
    };
    assert_eq!(chip_says(&day, stores::Filter::Lots), "\u{203a} LOTS");
    assert_eq!(chip_says(&day, stores::Filter::All), "ALL");

    // The head follows the purse while the screen is up.
    day.h.world_mut().resource_mut::<Purse>().0 = 2470;
    day.h.step(2);
    day.h.settle();
    assert_eq!(day.says(stores::PURSE_ID), "2,470");
    // The pack alone shows it too.
    day.close();
    day.h.key(stores::KEY);
    day.h.settle();
    assert_eq!(day.h.stack_top(), Some(stores::kind()));
    assert_eq!(day.says(stores::PURSE_ID), "2,470");
}

/// At a deposit the HUD's place card names it and its class, and away from
/// it says nothing.
#[test]
fn the_place_card_names_the_deposit_stood_at() {
    let (mut day, _, _) = survey_day("place-deposit", 1600.0, 900.0);
    day.h.step(3);
    day.h.settle();
    assert_eq!(day.says(place::NAME_ID), "Calder flats");
    assert_eq!(
        day.says(place::KIND_ID),
        "Reactive · evaporite salt deposit"
    );
    let layer = day.h.hud_layer(place::LAYER).expect("the place card");
    assert!(day.h.is_visible(layer));
    // Away from every deposit it goes.
    day.h
        .world_mut()
        .get_mut::<WorldPosition>(day.player)
        .unwrap()
        .0 = strata_core::WorldPos::new(200.0, 10.0, 200.0);
    day.h.step(3);
    day.h.settle();
    let layer = day.h.hud_layer(place::LAYER).expect("the place card");
    assert!(!day.h.is_visible(layer));
}

/// Walking onto a holder's ground puts its name and the kind of ground on the
/// card, "Contested ground" in the kind's place where two are as strong, and
/// leaving every holding's reach says "Unclaimed wilds" alone.
#[test]
fn the_place_card_names_the_ground_entered_and_the_wilds_left() {
    use veyra_sim::factions::{Faction, FactionId, HeldHere, Territory};
    let (mut day, _, _) = survey_day("place-ground", 1600.0, 900.0);
    day.h
        .world_mut()
        .get_mut::<WorldPosition>(day.player)
        .unwrap()
        .0 = strata_core::WorldPos::new(200.0, 10.0, 200.0);
    let kind = day
        .registries
        .factions
        .kinds()
        .iter()
        .find(|kind| kind.nature == strata_registry::Nature::Conservation)
        .expect("a conservation kind");
    let faction = Faction {
        id: FactionId(7),
        kind: kind.id.clone(),
        name: "Nothath Trust".to_owned(),
        colour: [24, 200, 176],
        nature: kind.nature,
        seat: [0.0, 0.0],
    };
    let held = |holder: Option<Faction>, contested: bool| HeldHere {
        at: Some([200.0, 200.0]),
        territory: Some(Territory {
            holder: holder.as_ref().map(|faction| faction.id),
            strength: 0.6,
            contested,
        }),
        holder,
        ..HeldHere::default()
    };
    day.h
        .world_mut()
        .insert_resource(held(Some(faction.clone()), false));
    day.h.step(3);
    day.h.settle();
    assert_eq!(day.says(place::NAME_ID), "Nothath Trust");
    assert_eq!(day.says(place::KIND_ID), format!("{} ground", kind.name));
    let layer = day.h.hud_layer(place::LAYER).expect("the place card");
    assert!(day.h.is_visible(layer));

    // Onto other ground, where another is as strong.
    let other = Faction {
        id: FactionId(9),
        name: "Cethand Cooperative".to_owned(),
        ..faction
    };
    day.h.world_mut().insert_resource(held(Some(other), true));
    day.h.step(3);
    day.h.settle();
    assert_eq!(day.says(place::NAME_ID), "Cethand Cooperative");
    assert_eq!(day.says(place::KIND_ID), "Contested ground");

    // Out of every reach.
    day.h.world_mut().insert_resource(held(None, false));
    day.h.step(3);
    day.h.settle();
    assert_eq!(day.says(place::NAME_ID), "Unclaimed wilds");
    assert_eq!(day.says(place::KIND_ID), "");
}

/// The register fits a small window: the frame inside the window, Close and
/// the card inside the frame, nothing spilling.
#[test]
fn the_register_fits_small_windows() {
    let window = Rect::new(0.0, 0.0, 1280.0, 720.0);
    let (mut day, _, ground) = survey_day("register-small", 1280.0, 720.0);

    // The register, on the long card of an ore with eight lines.
    let mut commands = day.h.world_mut().commands();
    bench::open_on(&mut commands, false, bench::BenchTab::Register);
    day.h.world_mut().flush();
    day.h.settle();
    day.h.set_input_mode(InputMode::Keyboard);
    day.h.settle();
    day.tab_to(&bench::row_id(ground.rake.id));
    day.h.step(2);
    day.h.settle();
    assert_eq!(day.card_at(bench::REGISTER_CARD_ID).ranges.len(), 8);
    let root = day.h.rect_of(find(&day.h, bench::ROOT_ID));
    assert!(within(root, window), "{root:?}");
    for id in [
        bench::CLOSE_ID.to_owned(),
        bench::REGISTER_CARD_ID.to_owned(),
        bench::COUNT_ID.to_owned(),
        bench::CAPTION_ID.to_owned(),
        bench::row_id(ground.rake.id),
    ] {
        let rect = day.h.rect_of(find(&day.h, &id));
        assert!(within(rect, root), "{id} {rect:?} outside {root:?}");
    }
    // A row is one line of columns: its last column ends inside it.
    let row = day
        .h
        .rect_of(find(&day.h, &bench::row_id(ground.sorrel.id)));
    let card = day.h.rect_of(find(&day.h, bench::REGISTER_CARD_ID));
    assert!(row.max.x <= card.min.x, "{row:?} under {card:?}");
}

/// The prompt chip and the place card stand at the middle of the window at
/// any UI scale. Slotted anchors a centred layer at half the window's
/// logical pixels, which the scale then multiplies: at 1080p that is five
/// eighths of the way across, and the client holds them to the middle.
#[test]
fn the_prompt_and_the_place_card_are_centred_at_any_scale() {
    for (width, height, scale) in [(1600.0, 900.0, 1.0), (1920.0, 1080.0, 1.25)] {
        let (mut day, _, _) = survey_day("hud-centred", width, height);
        day.h.step(3);
        day.h.settle();
        assert_eq!(day.h.world().resource::<bevy::ui::UiScale>().0, scale);
        for (layer, text) in [
            (ui::prompt::LAYER, ui::prompt::TEXT_ID),
            (place::LAYER, place::NAME_ID),
        ] {
            let root = day.h.hud_layer(layer).expect("the layer");
            assert!(day.h.is_visible(root), "{layer} is up");
            // The card is the panel round the words.
            let words = find(&day.h, text);
            let card = day.h.world().get::<ChildOf>(words).unwrap().parent();
            // A node's rectangle is in UI units, which the scale puts on
            // the screen.
            let rect = day.h.rect_of(card);
            assert!(
                (rect.center().x * scale - width / 2.0).abs() < 1.5,
                "{layer} at {width} wide stands at {rect:?}"
            );
        }
    }
}

/// G opens the survey screen over nothing and closes it again, so does
/// Escape, and the tool is not an assay: no screen of that kind exists.
#[test]
fn g_opens_and_closes_the_survey_screen() {
    let mut day = first_day("survey-key", 1280.0, 720.0);
    day.h.key(survey::KEY);
    day.h.settle();
    assert_eq!(day.h.stack(), vec![survey::kind()]);
    assert_eq!(day.says(survey::PING_WHY_ID), "", "a hand that can ping");
    assert!(!button_disabled(&day.h, survey::PING));
    // Nothing found reads here, so Sample here is grey and says so.
    assert!(button_disabled(&day.h, survey::SAMPLE));
    assert_eq!(
        day.says(survey::SAMPLE_WHY_ID),
        "Nothing found reads here. Ping, then walk to a reading."
    );
    day.h.key(survey::KEY);
    day.h.advance(std::time::Duration::from_millis(120));
    day.h.settle();
    assert_eq!(day.h.stack_top(), None);
    day.h.key(survey::KEY);
    day.h.settle();
    assert_eq!(day.h.stack_top(), Some(survey::kind()));
    day.close();
    assert_eq!(day.h.stack_top(), None, "Escape closes it too");
    // A screen on top keeps G from opening a second.
    day.use_locker();
    day.h.key(survey::KEY);
    day.h.settle();
    assert_eq!(day.h.stack(), vec![stores::kind()]);
    let screens = day.h.world().resource::<slotted::ui::Screens>();
    assert!(
        screens
            .kinds()
            .all(|kind| !kind.0.as_str().contains("assay")),
        "the assay is gone"
    );
}

/// Ping asks the sim for the class picked, and is grey with the sim's
/// reason while the player is busy with a channel.
#[test]
fn ping_asks_the_sim_and_reads_stop_while_a_ping_runs() {
    let mut day = first_day("survey-ping", 1280.0, 720.0);
    day.h.key(survey::KEY);
    day.h.settle();
    let classes = day.registries.resources.classes().to_vec();
    let brine = classes
        .iter()
        .find(|class| class.name == "Brine")
        .expect("brine");
    day.h.click(find(&day.h, &survey::class_id(&brine.id)));
    day.h.settle();
    day.h.click(find(&day.h, survey::PING));
    day.h.step(1);
    let asked = day.h.world().resource::<SurveyInbox>().to_vec();
    assert!(
        asked.contains(&SurveyRequest::Ping {
            class: brine.id.clone()
        }) || day.h.world().resource::<LastPing>().0.is_some(),
        "the request went to the sim or was already consumed: {asked:?}"
    );
    // A channel on the player makes Ping grey with the sim's own reason.
    day.h.world_mut().entity_mut(day.player).insert(Pinging {
        class: brine.id.clone(),
        left: 2.0,
    });
    day.h.step(2);
    day.h.settle();
    // The Ping button is Stop while the ping runs: live, with no reason.
    assert!(!button_disabled(&day.h, survey::PING));
    assert_eq!(
        texts_under(&day.h, find(&day.h, survey::PING)).join(" "),
        "Stop"
    );
    assert_eq!(day.says(survey::PING_WHY_ID), "");
    assert_eq!(day.says(survey::STATUS_ID), "Surveying");
}

/// Stop is on the screen for both channels: the Ping button while pinging
/// and Sample here, as Stop sampling, while sampling, each sending the
/// sim's `Stop`, and the status line counts the units taken so far.
#[test]
fn the_screen_can_stop_what_it_started() {
    let (mut day, _, ground) = survey_day("survey-stop", 1280.0, 720.0);
    let class = day
        .registries
        .resources
        .class_of(ground.calder.ty)
        .id
        .clone();
    day.h.key(survey::KEY);
    day.h.settle();
    day.h
        .world_mut()
        .entity_mut(day.player)
        .insert(Pinging { class, left: 2.0 });
    day.h.step(2);
    day.h.settle();
    // The tick hears Stop and ends the channel; another Ping would have
    // been refused as Busy and left it running.
    day.h.click(find(&day.h, survey::PING));
    day.h.step(2);
    assert!(
        !day.h.world().entity(day.player).contains::<Pinging>(),
        "Stop ended the ping"
    );
    day.h
        .world_mut()
        .entity_mut(day.player)
        .insert(deposits::Sampling {
            deposit: ground.calder.id,
            from: [0.0, 0.0],
            left: 2.0,
            got: 16,
        });
    day.h.step(2);
    day.h.settle();
    assert_eq!(
        texts_under(&day.h, find(&day.h, survey::SAMPLE)).join(" "),
        "Stop sampling"
    );
    assert!(!button_disabled(&day.h, survey::SAMPLE));
    assert_eq!(day.says(survey::STATUS_ID), "Sampling · 16 units");
    day.h.click(find(&day.h, survey::SAMPLE));
    day.h.step(2);
    assert!(
        !day.h
            .world()
            .entity(day.player)
            .contains::<deposits::Sampling>(),
        "Stop ended the sampling"
    );
}

/// The last ping's grid is drawn from the sim's result: every point a cell
/// with a number, the middle one the player's.
#[test]
fn the_last_pings_grid_is_drawn_cell_by_cell() {
    let cell_text = |day: &Day, id: &str| texts_under(&day.h, find(&day.h, id)).join(" ");
    let (mut day, _, ground) = survey_day("survey-grid", 1280.0, 720.0);
    let set = &day.registries.resources;
    let class = set.class_of(ground.calder.ty).id.clone();
    let skill = deposits::SurveySkill {
        channel_s: 2.5,
        cycle_s: 4.0,
        peak: false,
        range_m: 128.0,
        grid: 5,
        units: 8,
        reading: strata_registry::Reading::Type,
    };
    let calder = ground.calder.clone();
    let result = deposits::ping_with(
        &|x, z| {
            let pm = calder.concentration(x, z);
            if pm == 0 {
                Vec::new()
            } else {
                vec![(calder.clone(), pm)]
            }
        },
        set,
        &skill,
        [0.0, 0.0],
        &class,
    );
    assert!(!result.found.is_empty(), "the hand-built deposit is met");
    day.h
        .world_mut()
        .insert_resource(LastPing(Some(result.clone())));
    day.h.key(survey::KEY);
    day.h.settle();
    let mut cells = 0;
    for row in 0..5 {
        for column in 0..5 {
            let text = cell_text(&day, &survey::cell_id(row, column));
            assert!(text.contains('%'), "a number in every cell: {text:?}");
            cells += 1;
        }
    }
    assert_eq!(cells, 25);
    let middle = cell_text(&day, &survey::cell_id(2, 2));
    assert!(middle.contains("YOU"), "the player's own point: {middle:?}");
    let others = (0..5)
        .flat_map(|r| (0..5).map(move |c| (r, c)))
        .filter(|&(r, c)| (r, c) != (2, 2))
        .filter(|&(r, c)| cell_text(&day, &survey::cell_id(r, c)).contains("YOU"))
        .count();
    assert_eq!(others, 0, "one mark and only one");
    // The deposit lies at the player's feet to the north by 1.5 m: the
    // northern rows read as much as the southern ones, no more.
    let met = &day
        .h
        .world()
        .resource::<LastPing>()
        .0
        .as_ref()
        .unwrap()
        .found;
    assert_eq!(met[0].deposit.id, ground.calder.id);
    let list = texts_under(&day.h, find(&day.h, survey::ROOT_ID)).join("\n");
    assert!(list.contains("Calder flats"), "{list}");
    assert!(
        list.contains("Sampled"),
        "what the household knows of it: {list}"
    );
}

/// The lead is a second marker with its own words, and goes when the
/// household stands at it.
#[test]
fn the_lead_is_a_marker_that_goes_when_the_household_arrives() {
    let (mut day, _, _) = survey_day("survey-lead", 1600.0, 900.0);
    let lead = |day: &mut Day| {
        let world = day.h.world_mut();
        let mut markers = world.query::<(Entity, &TrackerMarker)>();
        let (entity, marker) = markers
            .iter(world)
            .find(|(_, marker)| marker.lead)
            .expect("the lead marker");
        let words = world.get::<Text>(marker.text).unwrap().0.clone();
        (entity, words)
    };
    let (entity, _) = lead(&mut day);
    assert!(!day.h.is_visible(entity), "no ping, no lead");
    day.h.world_mut().resource_mut::<Surveyed>().lead = Some(deposits::Lead {
        deposit: DepositId([0, 0]),
        at: [60.0, 0.0],
        pm: 400,
    });
    day.h.step(3);
    day.h.settle();
    let (entity, words) = lead(&mut day);
    assert!(day.h.is_visible(entity));
    assert_eq!(words, "Strongest reading · 60 m");
    day.h.world_mut().resource_mut::<Surveyed>().lead = Some(deposits::Lead {
        deposit: DepositId([0, 0]),
        at: [4.0, 0.0],
        pm: 400,
    });
    day.h.step(3);
    day.h.settle();
    let (entity, _) = lead(&mut day);
    assert!(
        !day.h.is_visible(entity),
        "within the clear distance it goes"
    );
}

/// A survey screen left alone builds its view once: ten idle frames ask the
/// world the same questions and, finding the answers the same, make no new
/// view; a changed answer (a pack cell filled) makes one.
#[test]
fn an_idle_survey_screen_builds_its_view_once() {
    let mut day = first_day("survey-idle", 1280.0, 720.0);
    day.h.key(survey::KEY);
    day.h.settle();
    assert_eq!(day.h.stack(), vec![survey::kind()]);
    let built = day.h.world().resource::<survey::SurveyState>().views();
    assert!(built >= 1, "the open built a view");
    for _ in 0..10 {
        day.h.step(1);
    }
    assert_eq!(
        day.h.world().resource::<survey::SurveyState>().views(),
        built,
        "ten idle frames built no view"
    );
    let feed = day.id(FEED);
    let player = day.player;
    day.h
        .world_mut()
        .get_mut::<veyra_sim::items::Pack>(player)
        .unwrap()
        .cells
        .set(17, Some(ItemStack::new(feed, 2)));
    day.h.step(2);
    assert!(
        day.h.world().resource::<survey::SurveyState>().views() > built,
        "a changed pack built a view"
    );
}

/// A machine at work moves its carry every tick and the screen draws only
/// where it stands (the ticket 0011 bug, which rebuilt the screen a tick):
/// the carry builds no view, a machine planted does.
#[test]
fn a_working_machines_carry_builds_no_survey_view() {
    use veyra_sim::harvest::{Harvester, Harvesters};

    let (mut day, _, ground) = survey_day("survey-carry", 1280.0, 720.0);
    let stack = deposits::lot(&ground.harrow, &day.registries.resources, 1).unwrap();
    let machine = |id, at: [f64; 2]| Harvester {
        id,
        deposit: ground.harrow.id,
        at,
        pm: 500,
        item: stack.clone(),
        by: String::new(),
        rate: 1,
        hopper: 10,
        lot: stack.clone(),
        held: 0,
        carry: 0,
        full: false,
    };
    day.h.world_mut().insert_resource(Harvesters {
        list: vec![machine(7, [800.0, 800.0])],
        next: 8,
    });
    day.h.key(survey::KEY);
    day.h.settle();
    day.h.step(6);
    let built = day.h.world().resource::<survey::SurveyState>().views();
    for step in 1..=5 {
        day.h.world_mut().resource_mut::<Harvesters>().list[0].carry = step;
        day.h.step(2);
    }
    assert_eq!(
        day.h.world().resource::<survey::SurveyState>().views(),
        built,
        "the carry built no view"
    );
    day.h
        .world_mut()
        .resource_mut::<Harvesters>()
        .list
        .push(machine(8, [900.0, 900.0]));
    day.h.step(2);
    assert!(
        day.h.world().resource::<survey::SurveyState>().views() > built,
        "a machine planted built one"
    );
}
