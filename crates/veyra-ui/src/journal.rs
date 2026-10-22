//! The journal (ticket 0014, step 2): the questline's title, the line of the
//! stage it is at, that stage's objectives ticked or not, the choices made
//! so far and, once it is over, how it ended. J opens it, and the pause
//! menu has a button.
//!
//! The words are the quest data's, filled with the world's factions; no
//! rule lives here. [`target_of`] is also what the HUD's tracker and the
//! map mark: the first objective not yet met that has a place to go to.

use bevy::ecs::prelude::*;
use bevy::ecs::system::SystemParam;
use bevy::input::ButtonInput;
use bevy::input::keyboard::KeyCode;
use slotted::ui::def::{
    Align, ButtonOpts, Justify, Layout, Length, Padding, Presentation, PresentationMode, ScreenDef,
    ScreenKind, Tags, TextOpts, TextRole, Transition, UiNodeDef, WidgetKind,
};
use slotted::ui::{ScreenStack, pop_screen};
use strata_registry::GameRegistries;
use strata_registry::quest::{Beat, Objective, Place};
use veyra_sim::Registries;
use veyra_sim::deposits::{NearDeposits, Surveyed};
use veyra_sim::quests::{QuestCast, QuestLog, QuestPlaces, QuestState, fill as fill_names};

use super::crafting::{
    HEAD_ROLE, NAME_ROLE, NOTE_ROLE, column, key, line_in, panel, row, spacer, text_in, with_id,
    words,
};
use strata_ui::sim_screen::{self, ChangeSource, ScreenStep, SimScreen, SimScreenApp, moved};

/// The screen kind.
pub const KIND: &str = "strata:journal";
/// The `test_id` of the root panel.
pub const ROOT_ID: &str = "journal.root";
/// The `test_id` of the quest's title.
pub const TITLE_ID: &str = "journal.title";
/// The `test_id` of the stage's line.
pub const LINE_ID: &str = "journal.line";
/// The `test_id` prefix of an objective's row; its place follows.
pub const OBJECTIVE_TAG: &str = "journal.objective.";
/// The `test_id` prefix of a decision's row; its place follows.
pub const CHOICE_TAG: &str = "journal.choice.";
/// The `test_id` of the Close button.
pub const CLOSE_ID: &str = "journal.close";

/// The key that opens it.
pub const KEY: KeyCode = KeyCode::KeyJ;

/// How many decisions the list keeps, the latest.
const CHOICES_SHOWN: usize = 6;
const WIDTH: f32 = 700.;
const HEIGHT_PERCENT: f32 = 78.;

/// The screen kind.
#[must_use]
pub fn kind() -> ScreenKind {
    ScreenKind::new(KIND)
}

// ---- the view ----

/// One objective as a line of the journal.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Tick {
    /// What is to be done.
    pub text: String,
    /// Whether it is.
    pub done: bool,
}

/// Everything the screen shows.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct JournalView {
    /// The quest's name; empty with nothing begun.
    pub title: String,
    /// The stage's line, or the ending's once it is over.
    pub line: String,
    /// The stage's objectives.
    pub objectives: Vec<Tick>,
    /// The decisions made so far, one line each.
    pub choices: Vec<String>,
    /// Whether the quest is over.
    pub over: bool,
}

fn place_words(place: Place) -> &'static str {
    words(match place {
        Place::Quay => "strata.journal.place.quay",
        Place::Exchange => "strata.journal.place.exchange",
        Place::Board => "strata.journal.place.board",
        Place::Station => "strata.journal.place.station",
        Place::Channel => "strata.journal.place.channel",
        Place::Training => "strata.journal.place.training",
    })
}

fn item_name(data: &GameRegistries, id: &strata_registry::Namespaced) -> String {
    data.items
        .get(id)
        .map_or_else(|| id.to_string(), |def| def.name.clone())
}

fn person_name(data: &GameRegistries, id: &strata_registry::Namespaced) -> String {
    data.quests
        .person(id)
        .map_or_else(|| id.to_string(), |person| person.name.clone())
}

fn class_name(data: &GameRegistries, class: &strata_registry::Namespaced) -> String {
    data.resources
        .classes()
        .iter()
        .find(|each| each.id == *class)
        .map_or_else(|| class.to_string(), |each| each.name.clone())
}

/// A crew's name for the journal: its leader's, else its hands' plate.
fn crew_name(data: &GameRegistries, quest: &str, crew: &str) -> String {
    let leader = format!("veyra:person.{crew}")
        .parse::<strata_registry::Namespaced>()
        .ok()
        .and_then(|id| data.quests.person(&id).map(|person| person.name.clone()));
    leader
        .or_else(|| {
            data.quests
                .quest(quest)?
                .crew(crew)?
                .hand
                .as_ref()
                .map(|hand| hand.name.clone())
        })
        .unwrap_or_else(|| crew.to_owned())
}

fn foe_words(kind: strata_registry::encounter::FoeKind) -> &'static str {
    use strata_registry::encounter::FoeKind;
    words(match kind {
        FoeKind::Sentry => "strata.journal.foe.sentry",
        FoeKind::Rusher => "strata.journal.foe.rusher",
        FoeKind::Marksman => "strata.journal.foe.marksman",
        FoeKind::Duelist => "strata.journal.foe.duelist",
        FoeKind::Bulwark => "strata.journal.foe.bulwark",
    })
}

/// What an objective asks, in the player's words.
#[must_use]
pub fn objective_words(
    data: &GameRegistries,
    quest: &str,
    objective: &Objective,
    state: &QuestState,
    index: usize,
) -> String {
    let said = |key: &'static str| words(key).to_owned();
    match objective {
        Objective::Talk(person) => {
            words("strata.journal.talk").replace("{name}", &person_name(data, person))
        }
        Objective::Reach(place, _) => {
            words("strata.journal.reach").replace("{place}", place_words(*place))
        }
        Objective::Defeat(Beat::Opponent(kind)) => {
            words("strata.journal.beat").replace("{name}", foe_words(*kind))
        }
        Objective::Defeat(Beat::Crew(crew)) => {
            words("strata.journal.beat_crew").replace("{name}", &crew_name(data, quest, crew))
        }
        Objective::Learn => said("strata.journal.learn"),
        Objective::Ping => said("strata.journal.ping"),
        Objective::Sample { class, units } => {
            let have = state.counts.get(&index).copied().unwrap_or(0).min(*units);
            words("strata.journal.sample")
                .replace("{class}", &class_name(data, class).to_lowercase())
                .replace("{have}", &have.to_string())
                .replace("{units}", &units.to_string())
        }
        Objective::Craft(item) => {
            words("strata.journal.craft").replace("{item}", &item_name(data, item).to_lowercase())
        }
        Objective::Sell(item) => {
            words("strata.journal.sell").replace("{item}", &item_name(data, item).to_lowercase())
        }
        Objective::Buy(item) => {
            words("strata.journal.buy").replace("{item}", &item_name(data, item).to_lowercase())
        }
        Objective::Stable => said("strata.journal.stable"),
        Objective::Recruit(name) => words("strata.journal.recruit").replace("{name}", name),
        Objective::Switch(name) => words("strata.journal.switch").replace("{name}", name),
        Objective::Deliver(item, person) => words("strata.journal.deliver")
            .replace("{item}", &item_name(data, item).to_lowercase())
            .replace("{name}", &person_name(data, person).to_lowercase()),
    }
}

impl JournalView {
    /// The journal of the questline the log has begun, the first in the data
    /// when more than one has.
    #[must_use]
    pub fn of(data: &GameRegistries, log: &QuestLog, cast: &QuestCast) -> Self {
        let Some((quest, state)) = data
            .quests
            .quests()
            .iter()
            .find_map(|quest| Some((quest, log.quests.get(&quest.id)?)))
        else {
            return Self {
                line: words("strata.journal.empty").to_owned(),
                ..Self::default()
            };
        };
        let over = state.done;
        let line = if over {
            log.ending
                .as_deref()
                .and_then(|id| quest.ending(id))
                .map(|ending| fill_names(&ending.journal, cast))
        } else {
            state
                .stage
                .as_deref()
                .and_then(|stage| quest.stage(stage))
                .map(|stage| fill_names(&stage.journal, cast))
        }
        .unwrap_or_default();
        let objectives = if over {
            Vec::new()
        } else {
            state
                .stage
                .as_deref()
                .and_then(|stage| quest.stage(stage))
                .map(|stage| {
                    stage
                        .objectives
                        .iter()
                        .enumerate()
                        .map(|(index, objective)| Tick {
                            text: objective_words(data, &quest.id, objective, state, index),
                            done: state.met.contains(&index),
                        })
                        .collect()
                })
                .unwrap_or_default()
        };
        // A reply that did something is a decision; the rest was talk.
        let mut choices: Vec<String> = log
            .choices
            .iter()
            .filter_map(|choice| {
                let reply = data.quests.node(&choice.node)?.replies.get(choice.reply)?;
                (!reply.effects.is_empty()).then(|| fill_names(&reply.text, cast))
            })
            .collect();
        let skip = choices.len().saturating_sub(CHOICES_SHOWN);
        choices.drain(..skip);
        Self {
            title: fill_names(&quest.title, cast),
            line,
            objectives,
            choices,
            over,
        }
    }
}

// ---- the target ----

/// Where the current objective leads: what the tracker and the map mark.
#[derive(Debug, Clone, PartialEq)]
pub struct QuestTarget {
    /// `(x, z)`.
    pub at: [f64; 2],
    /// What is there: a person, a place, a deposit.
    pub name: String,
}

/// The first objective of the stage not yet met that has a place, where it
/// is. Talking and fitting lead to the person's stand, reaching and buying
/// to the place or the board, a crew to where it stands, sampling to the
/// nearest found deposit of the class that is listed near the household. Nothing
/// when the quest is over or the objective has no place (learning, pinging).
#[must_use]
pub fn target_of(
    data: &GameRegistries,
    log: &QuestLog,
    places: &QuestPlaces,
    sample: Option<(&NearDeposits, &Surveyed)>,
    from: [f64; 2],
) -> Option<QuestTarget> {
    let quest = data
        .quests
        .quests()
        .iter()
        .find(|quest| log.stage_of(&quest.id).is_some())?;
    let state = log.quests.get(&quest.id)?;
    let current = quest.stage(state.stage.as_deref()?)?;
    let at_place = |place: Place| {
        places.0.get(&place).map(|at| QuestTarget {
            at: *at,
            name: place_words(place).to_owned(),
        })
    };
    let at_person = |person: &strata_registry::Namespaced| {
        let def = data.quests.person(person)?;
        places.0.get(&def.stand).map(|at| QuestTarget {
            at: *at,
            name: def.name.clone(),
        })
    };
    current
        .objectives
        .iter()
        .enumerate()
        .filter(|(index, _)| !state.met.contains(index))
        .find_map(|(_, objective)| match objective {
            Objective::Talk(person) | Objective::Deliver(_, person) => at_person(person),
            Objective::Reach(place, _) => at_place(*place),
            Objective::Buy(_) | Objective::Sell(_) => at_place(Place::Board),
            Objective::Defeat(Beat::Crew(crew)) => {
                at_place(quest.crew(crew)?.at).map(|target| QuestTarget {
                    name: crew_name(data, &quest.id, crew),
                    ..target
                })
            }
            // The staging fight's opponents stand at the training ground.
            Objective::Defeat(Beat::Opponent(_)) => at_place(Place::Training),
            Objective::Sample { class, .. } => sample
                .map(|(near, _)| near)?
                .deposits
                .iter()
                .filter(|deposit| {
                    // Only a found deposit is marked (ticket 0011), so the ping
                    // the stage teaches is not skipped.
                    sample.is_some_and(|(_, surveyed)| surveyed.found.contains_key(&deposit.id))
                        && data.resources.class_of(deposit.ty).id == *class
                })
                .min_by(|a, b| {
                    a.distance_m(from[0], from[1])
                        .total_cmp(&b.distance_m(from[0], from[1]))
                })
                .map(|deposit| QuestTarget {
                    at: deposit.at,
                    name: deposit.place.clone(),
                }),
            _ => None,
        })
}

// ---- the definition ----

fn tick_row(place: usize, tick: &Tick) -> UiNodeDef {
    with_id(
        line_in(
            format!(
                "{} {}",
                words(if tick.done {
                    "strata.journal.tick.done"
                } else {
                    "strata.journal.tick.open"
                }),
                tick.text
            ),
            if tick.done { NOTE_ROLE } else { NAME_ROLE },
        ),
        &format!("{OBJECTIVE_TAG}{place}"),
    )
}

/// The whole screen for `view`.
#[must_use]
pub fn screen_def(view: &JournalView) -> ScreenDef {
    let mut body = vec![with_id(
        UiNodeDef::Text {
            key: key(view.line.clone()),
            style: TextRole::Body,
            opts: TextOpts::default(),
            tags: Tags::new(),
        },
        LINE_ID,
    )];
    if !view.objectives.is_empty() {
        body.push(line_in(words("strata.journal.objectives"), HEAD_ROLE));
        body.extend(
            view.objectives
                .iter()
                .enumerate()
                .map(|(place, tick)| tick_row(place, tick)),
        );
    }
    if !view.choices.is_empty() {
        body.push(line_in(words("strata.journal.choices"), HEAD_ROLE));
        body.extend(view.choices.iter().enumerate().map(|(place, said)| {
            with_id(
                line_in(said.clone(), NOTE_ROLE),
                &format!("{CHOICE_TAG}{place}"),
            )
        }));
    }
    ScreenDef {
        kind: kind(),
        inherits: None,
        root: UiNodeDef::Panel {
            role: slotted::theme::Role::new("panel"),
            layout: Layout {
                width: Some(Length::Px(WIDTH)),
                max_width: Some(Length::Percent(96.)),
                min_height: Some(Length::Percent(HEIGHT_PERCENT)),
                max_height: Some(Length::Percent(HEIGHT_PERCENT)),
                padding: Padding::all(3.),
                align: Some(Align::Stretch),
                justify: Justify::Start,
                ..column(1.4)
            },
            children: vec![
                with_id(
                    text_in(
                        if view.title.is_empty() {
                            words("strata.journal.title").to_owned()
                        } else {
                            view.title.clone()
                        },
                        TextRole::Title,
                        "panel.title",
                    ),
                    TITLE_ID,
                ),
                // A scroll panel, not a clipped one: the wheel and the bar move it.
                UiNodeDef::Scroll {
                    layout: Layout {
                        align: Some(Align::Stretch),
                        grow: 1.,
                        min_height: Some(Length::Px(0.)),
                        ..column(1.2)
                    },
                    scrollbar: true,
                    children: body,
                    tags: Tags::new(),
                },
                panel(
                    "invisible",
                    row(2.),
                    vec![
                        spacer(),
                        UiNodeDef::Button {
                            widget: Some(WidgetKind::new("slotted:close")),
                            opts: ButtonOpts {
                                label: Some(key("strata.menu.close")),
                                ..ButtonOpts::default()
                            },
                            tags: Tags::new().with(Tags::TEST_ID, CLOSE_ID),
                        },
                    ],
                ),
            ],
            tags: Tags::new().with(Tags::TEST_ID, ROOT_ID),
        },
        listring: Vec::new(),
        remove: Vec::new(),
        initial_focus: Some(CLOSE_ID.to_owned()),
        presentation: Presentation {
            mode: PresentationMode::Page,
            scrim: Some(true),
            transition: Transition::Fade,
            wildcards: Some(false),
            ..Presentation::default()
        },
    }
}

// ---- open, rebuild ----

/// What the open screen was last built from.
#[derive(Resource, Debug, Default)]
pub struct JournalUi {
    view: Option<JournalView>,
}

/// What the journal reads: the data, the log and the cast the names are
/// filled from. A stage met, a decision made or a name learnt moves one.
#[derive(SystemParam)]
pub struct JournalInputs<'w> {
    registries: Option<Res<'w, Registries>>,
    log: Option<Res<'w, QuestLog>>,
    cast: Option<Res<'w, QuestCast>>,
}

impl ChangeSource for JournalInputs<'static> {
    fn changed(inputs: &mut JournalInputs<'_>) -> bool {
        moved(&inputs.registries) || moved(&inputs.log) || moved(&inputs.cast)
    }
}

impl SimScreen for JournalUi {
    type View = JournalView;
    type Inputs = JournalInputs<'static>;
    type Answering = ();
    const KIND: &'static str = KIND;

    fn view(inputs: &JournalInputs<'_>, _: &Self) -> Option<JournalView> {
        let cast = inputs.cast.as_deref().cloned().unwrap_or_default();
        Some(JournalView::of(
            &inputs.registries.as_deref()?.0,
            inputs.log.as_deref()?,
            &cast,
        ))
    }

    fn def(view: &JournalView, _: &Self) -> ScreenDef {
        screen_def(view)
    }

    fn shown(state: &Self) -> Option<&JournalView> {
        state.view.as_ref()
    }

    fn marked(_: &Self) -> bool {
        false
    }

    fn built(state: &mut Self, view: JournalView) {
        state.view = Some(view);
    }
}

fn on_top(stack: &ScreenStack) -> bool {
    stack.top().is_some_and(|top| top.kind == kind())
}

/// Opens the journal, as J and the pause menu's button do.
pub fn open(commands: &mut Commands) {
    commands.queue(|world: &mut World| {
        let Some(def) = sim_screen::build::<JournalUi>(world) else {
            return;
        };
        sim_screen::show::<JournalUi>(world, def);
    });
}

/// `Update`: J opens the journal over nothing and closes it from the top.
pub fn toggle(keys: Res<ButtonInput<KeyCode>>, stack: Res<ScreenStack>, mut commands: Commands) {
    if !keys.just_pressed(KEY) {
        return;
    }
    if on_top(&stack) {
        pop_screen(&mut commands);
    } else if stack.top().is_none() {
        open(&mut commands);
    }
}

/// Registers the state and the systems. Only called once the UI is wired.
/// The screen rebuilds when the log has moved under it (an objective met
/// while it is up, a stage over).
pub(crate) fn register(app: &mut bevy::app::App) {
    app.add_sim_screen::<JournalUi>().add_systems(
        bevy::app::Update,
        toggle
            .before(ScreenStep::rebuild::<JournalUi>())
            .in_set(super::ScreenWork),
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::shipped;
    use veyra_sim::quests::Choice;

    #[test]
    fn the_journal_ticks_what_is_met_and_lists_the_decisions() {
        let data = shipped();
        let mut log = QuestLog::default();
        log.quests.insert(
            "arrival".to_owned(),
            QuestState {
                stage: Some("mending".to_owned()),
                met: [1, 2].into_iter().collect(),
                counts: [(0, 20)].into_iter().collect(),
                ..QuestState::default()
            },
        );
        // The Pay reply did something; "All right. Listen again." was talk.
        log.choices.push(Choice {
            node: "pell.hub".to_owned(),
            reply: 1,
        });
        log.choices.push(Choice {
            node: "pell.refuse".to_owned(),
            reply: 0,
        });
        let view = JournalView::of(&data, &log, &QuestCast::default());
        assert_eq!(
            view.choices,
            ["Two hundred credits for the core, and you go."]
        );
        assert_eq!(view.title, "The quiet station");
        assert!(view.line.contains("harness"), "{}", view.line);
        assert_eq!(view.objectives.len(), 5);
        assert!(!view.objectives[0].done && view.objectives[1].done);
        assert!(
            view.objectives[0].text.contains("20 of 45"),
            "{}",
            view.objectives[0].text
        );
        assert!(!view.over);
        let def = screen_def(&view);
        assert_eq!(def.kind, kind());
    }

    #[test]
    fn nothing_begun_is_said_and_a_finished_quest_gives_its_ending() {
        let data = shipped();
        let empty = JournalView::of(&data, &QuestLog::default(), &QuestCast::default());
        assert!(empty.title.is_empty() && !empty.line.is_empty());
        let mut log = QuestLog::default();
        log.quests.insert(
            "arrival".to_owned(),
            QuestState {
                done: true,
                ..QuestState::default()
            },
        );
        log.ending = Some("keepers".to_owned());
        let done = JournalView::of(&data, &log, &QuestCast::default());
        assert!(done.over && done.objectives.is_empty());
        assert!(done.line.contains("published"), "{}", done.line);
    }

    #[test]
    fn the_target_is_the_first_objective_with_a_place() {
        let data = shipped();
        let mut log = QuestLog::default();
        log.quests.insert(
            "arrival".to_owned(),
            QuestState {
                stage: Some("quiet_station".to_owned()),
                met: [0, 1, 2].into_iter().collect(),
                ..QuestState::default()
            },
        );
        let places = QuestPlaces(
            [(Place::Station, [100., 200.]), (Place::Board, [1., 2.])]
                .into_iter()
                .collect(),
        );
        let target = target_of(&data, &log, &places, None, [0., 0.]).expect("the station");
        assert!((target.at[0] - 100.).abs() < 1e-9 && (target.at[1] - 200.).abs() < 1e-9);
        log.quests.get_mut("arrival").expect("it").stage = Some("who_hears".to_owned());
        assert_eq!(target_of(&data, &log, &places, None, [0., 0.]), None);
    }

    /// A household that arrives on the quay is led to the staging fight.
    #[test]
    fn the_sentry_leads_to_the_training_ground() {
        let data = shipped();
        let mut log = QuestLog::default();
        log.quests.insert(
            "arrival".to_owned(),
            QuestState {
                stage: Some("steady_hand".to_owned()),
                met: [0].into_iter().collect(),
                ..QuestState::default()
            },
        );
        let places = QuestPlaces([(Place::Training, [-684., -270.])].into_iter().collect());
        let target = target_of(&data, &log, &places, None, [0., 0.]).expect("the ground");
        assert!((target.at[0] + 684.).abs() < 1e-9 && (target.at[1] + 270.).abs() < 1e-9);
        assert_eq!(target.name, "the training ground");
    }
}
