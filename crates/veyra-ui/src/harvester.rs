//! A planted field harvester's screen (ticket 0009, step 6): where it
//! stands and what it holds, how fast it fills, and Collect and Pick up.
//!
//! No rule lives here and nothing is worked out beyond words: the figures
//! are the machine's own (`Harvester::per_hour`, `minutes_to_full`) and the
//! two buttons are grey exactly when `veyra_sim::harvest::can_collect`
//! and `can_pick_up` would refuse the request, which is put in the sim's
//! inbox for the tick to judge. [`HarvesterView::of`] gathers the answers
//! into words and [`screen_def`] lays them out, so what the screen says
//! can be checked without one. Local-only: nothing here is replicated.

use bevy::ecs::prelude::*;
use bevy::ecs::system::SystemParam;
use slotted::menu::MenuChoice;
use slotted::theme::Role;
use slotted::ui::def::{
    Align, ButtonOpts, Justify, Layout, Length, Padding, Presentation, PresentationMode, ScreenDef,
    ScreenKind, Tags, TextRole, Transition, UiNodeDef, WidgetKind,
};
use strata_registry::GameRegistries;
use veyra_sim::deposits::{Deposit, Survey};
use veyra_sim::harvest::{
    self, HarvestAnswer, HarvestDone, HarvestInbox, HarvestNotice, HarvestRefusal, HarvestRequest,
    Harvester, Harvesters,
};
use veyra_sim::items::Pack;
use veyra_sim::{LocalPlayer, Registries, WorldPosition};

use super::card;
use super::crafting::{
    FIGURE_ROLE, HEAD_ROLE, NAME_ROLE, NOTE_ROLE, WHY_ROLE, act, column, fill, line_in, panel, row,
    spacer, text_in, with_id, words,
};
use super::factory::{PROGRESS_FILL_ROLE, PROGRESS_ROLE};
use super::strings::grouped;
use strata_ui::sim_screen::{self, ChangeSource, ScreenStep, Seen, SimScreen, SimScreenApp, moved};

/// The screen kind.
pub const KIND: &str = "strata:harvester";
/// The `test_id` of the root panel.
pub const ROOT_ID: &str = "harvester.root";
/// The `test_id` of the title.
pub const TITLE_ID: &str = "harvester.title";
/// The `test_id` of the deposit's type and place.
pub const ON_ID: &str = "harvester.on";
/// The `test_id` of the concentration line.
pub const READS_ID: &str = "harvester.reads";
/// The `test_id` of the rate line.
pub const RATE_ID: &str = "harvester.rate";
/// The `test_id` of the time-to-full line.
pub const TIME_ID: &str = "harvester.time";
/// The `test_id` of the hopper's track.
pub const HOPPER_ID: &str = "harvester.hopper";
/// The `test_id` of the held-of-hopper text.
pub const HELD_ID: &str = "harvester.held";
/// The `test_id` of the planter's line.
pub const BY_ID: &str = "harvester.by";
/// The `test_id` of the Collect button.
pub const COLLECT: &str = "harvester.collect";
/// The `test_id` of the Pick up button.
pub const PICK_UP: &str = "harvester.pick_up";
/// The `test_id` of the Track button, which points the HUD's lead at the
/// machine from anywhere.
pub const TRACK: &str = "harvester.track";
/// The `test_id` of the line of reasons under the buttons.
pub const WHY_ID: &str = "harvester.why";
/// The `test_id` of the Close button.
pub const CLOSE_ID: &str = "harvester.close";

const WIDTH: f32 = 560.;
const BAR_WIDTH: f32 = 320.;

/// No role of its own: the screen is drawn in the families the board and the
/// factory already own.
pub const ROLES: [&str; 0] = [];

/// The screen kind.
#[must_use]
pub fn kind() -> ScreenKind {
    ScreenKind::new(KIND)
}

/// A refusal as a player reads it beside a button.
#[must_use]
pub fn refusal_words(refusal: HarvestRefusal) -> String {
    let key = match refusal {
        HarvestRefusal::NoPlayer => "strata.harvester.why.no_player",
        HarvestRefusal::Mounted => "strata.harvester.why.mounted",
        HarvestRefusal::InWater => "strata.harvester.why.in_water",
        HarvestRefusal::NoHarvester => "strata.harvester.why.no_harvester",
        HarvestRefusal::NotTaught => "strata.harvester.why.not_taught",
        HarvestRefusal::AtLimit { count } => {
            return fill(
                words("strata.harvester.why.at_limit"),
                "count",
                &count.to_string(),
            );
        }
        HarvestRefusal::NothingHere => "strata.harvester.why.nothing_here",
        HarvestRefusal::TooNear => "strata.harvester.why.too_near",
        HarvestRefusal::NotInReach => "strata.harvester.why.not_in_reach",
        HarvestRefusal::Empty => "strata.harvester.why.empty",
        HarvestRefusal::NotEmpty => "strata.harvester.why.not_empty",
        HarvestRefusal::NoRoom => "strata.harvester.why.no_room",
    };
    words(key).to_owned()
}

/// `{held} of {hopper}`, grouped.
#[must_use]
pub fn held_words(held: u32, hopper: u32) -> String {
    fill(
        &fill(words("strata.harvester.held"), "held", &grouped(held)),
        "hopper",
        &grouped(hopper),
    )
}

/// Played minutes as `1 h 20 min` or `35 min`.
fn time_words(minutes: u32) -> String {
    if minutes >= 60 {
        fill(
            &fill(
                words("strata.harvester.hours"),
                "h",
                &(minutes / 60).to_string(),
            ),
            "m",
            &(minutes % 60).to_string(),
        )
    } else {
        fill(words("strata.harvester.minutes"), "m", &minutes.to_string())
    }
}

// ---- the state ----

/// The harvester screen's state: which machine it is open on and what it
/// was last built from.
#[derive(Resource, Debug, Default)]
pub struct HarvesterUi {
    /// The machine whose screen is open.
    pub id: Option<u32>,
    /// A rebuild is due whatever the view says.
    dirty: bool,
    /// What the open screen was built from.
    view: Option<HarvesterView>,
    /// The requests sent that the sim has not answered yet.
    asked: usize,
    /// The control the focus goes to at the next build.
    last: Option<String>,
}

impl HarvesterUi {
    /// What the open screen was last built from.
    #[must_use]
    pub const fn view(&self) -> Option<&HarvesterView> {
        self.view.as_ref()
    }
}

// ---- the view ----

/// Everything the screen shows, in display words.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HarvesterView {
    /// The machine.
    pub id: u32,
    /// The item's name.
    pub title: String,
    /// The deposit's type and place; empty when the world has no survey.
    pub on: String,
    /// The concentration where it stands.
    pub reads: String,
    /// Units an hour it makes there.
    pub rate: String,
    /// Time to full, or that it is full.
    pub time: String,
    /// The hopper's fill, per-mille.
    pub share: u16,
    /// `120 of 300`.
    pub held: String,
    /// Who planted it.
    pub by: String,
    /// Whether Collect can act.
    pub can_collect: bool,
    /// Collect's label: with the units it would take when it can act.
    pub collect_label: String,
    /// Whether Pick up can act.
    pub can_pick_up: bool,
    /// The reasons a button is grey; empty when both act.
    pub why: String,
}

/// What the view is made from.
pub struct Floor<'a> {
    /// The data sets.
    pub registries: &'a GameRegistries,
    /// The household's machines.
    pub harvesters: &'a Harvesters,
    /// The pack.
    pub pack: &'a Pack,
    /// Where the player stands, `(x, z)`.
    pub at: [f64; 2],
    /// The deposit the machine stands on, when the world has a survey.
    pub deposit: Option<&'a Deposit>,
}

impl HarvesterView {
    /// The screen for machine `id`; `None` when there is no such machine.
    #[must_use]
    pub fn of(floor: &Floor<'_>, id: u32) -> Option<Self> {
        let registries = floor.registries;
        let machine: &Harvester = floor.harvesters.get(id)?;
        let collect = harvest::can_collect(registries, floor.harvesters, floor.pack, id, floor.at);
        let pick_up = harvest::can_pick_up(registries, floor.harvesters, floor.pack, id, floor.at);
        let set = &registries.resources;
        let mut reasons: Vec<String> = Vec::new();
        for refusal in [collect.err(), pick_up.err()].into_iter().flatten() {
            let said = refusal_words(refusal);
            if !reasons.contains(&said) {
                reasons.push(said);
            }
        }
        let time = if machine.is_full() {
            words("strata.harvester.time.full").to_owned()
        } else {
            machine.minutes_to_full().map_or_else(
                || words("strata.harvester.time.none").to_owned(),
                |minutes| fill(words("strata.harvester.time"), "time", &time_words(minutes)),
            )
        };
        let share = if machine.hopper == 0 {
            0
        } else {
            u16::try_from(
                u64::from(machine.held.min(machine.hopper)) * 1000 / u64::from(machine.hopper),
            )
            .unwrap_or(1000)
        };
        Some(Self {
            id,
            title: registries.items.get(&machine.item.item).map_or_else(
                || words("strata.harvester.title").to_owned(),
                |def| def.name.clone(),
            ),
            on: floor.deposit.map_or_else(String::new, |deposit| {
                fill(
                    &fill(
                        words("strata.harvester.on"),
                        "type",
                        set.get(deposit.ty).map_or("", |def| def.name.as_str()),
                    ),
                    "place",
                    &deposit.place,
                )
            }),
            reads: fill(
                words("strata.harvester.reads"),
                "pct",
                &((u32::from(machine.pm) + 5) / 10).to_string(),
            ),
            rate: fill(
                words("strata.harvester.rate"),
                "n",
                &grouped(machine.per_hour()),
            ),
            time,
            share,
            held: held_words(machine.held, machine.hopper),
            by: if machine.by.is_empty() {
                words("strata.harvester.by.nobody").to_owned()
            } else {
                fill(words("strata.harvester.by"), "name", &machine.by)
            },
            can_collect: collect.is_ok(),
            collect_label: match collect {
                Ok(units) => fill(words("strata.harvester.collect.n"), "n", &grouped(units)),
                Err(_) => words("strata.harvester.collect").to_owned(),
            },
            can_pick_up: pick_up.is_ok(),
            why: reasons.join(" \u{b7} "),
        })
    }

    /// The request a button makes, if the id is one and it can act.
    #[must_use]
    pub fn request_of(&self, id: &str) -> Option<HarvestRequest> {
        match id {
            COLLECT if self.can_collect => Some(HarvestRequest::Collect { id: self.id }),
            PICK_UP if self.can_pick_up => Some(HarvestRequest::PickUp { id: self.id }),
            // The register's Enter does the same, and from any distance.
            TRACK => Some(HarvestRequest::Lead { id: self.id }),
            _ => None,
        }
    }

    /// The control the focus starts on: the first that can act.
    fn focus(&self, last: Option<&str>) -> String {
        let there = |id: &str| match id {
            COLLECT => self.can_collect,
            PICK_UP => self.can_pick_up,
            CLOSE_ID | TRACK => true,
            _ => false,
        };
        last.filter(|id| there(id))
            .map(str::to_owned)
            .or_else(|| {
                [COLLECT, PICK_UP]
                    .into_iter()
                    .find(|id| there(id))
                    .map(str::to_owned)
            })
            .unwrap_or_else(|| CLOSE_ID.to_owned())
    }
}

// ---- the definition ----

fn bar(share: u16) -> UiNodeDef {
    with_id(
        panel(
            PROGRESS_ROLE,
            Layout {
                width: Some(Length::Px(BAR_WIDTH)),
                height: Some(Length::Px(10.)),
                ..row(0.)
            },
            vec![panel(
                PROGRESS_FILL_ROLE,
                Layout {
                    width: Some(Length::Percent(card::track_percent(share))),
                    height: Some(Length::Px(10.)),
                    ..row(0.)
                },
                Vec::new(),
            )],
        ),
        HOPPER_ID,
    )
}

/// The whole screen for `view`.
#[must_use]
pub fn screen_def(view: &HarvesterView, focus: &str) -> ScreenDef {
    let body = vec![
        with_id(line_in(view.on.clone(), NAME_ROLE), ON_ID),
        with_id(line_in(view.reads.clone(), NOTE_ROLE), READS_ID),
        with_id(line_in(view.rate.clone(), FIGURE_ROLE), RATE_ID),
        with_id(line_in(view.time.clone(), FIGURE_ROLE), TIME_ID),
        line_in(words("strata.harvester.hopper"), HEAD_ROLE),
        panel(
            "invisible",
            row(2.),
            vec![
                bar(view.share),
                with_id(line_in(view.held.clone(), FIGURE_ROLE), HELD_ID),
            ],
        ),
        with_id(line_in(view.by.clone(), NOTE_ROLE), BY_ID),
        panel(
            "invisible",
            row(1.6),
            vec![
                act(&view.collect_label, COLLECT, true, view.can_collect),
                act(
                    words("strata.harvester.pick_up"),
                    PICK_UP,
                    false,
                    view.can_pick_up,
                ),
                act(words("strata.harvester.track"), TRACK, false, true),
            ],
        ),
        with_id(line_in(view.why.clone(), WHY_ROLE), WHY_ID),
    ];
    ScreenDef {
        kind: kind(),
        inherits: None,
        root: UiNodeDef::Panel {
            role: Role::new("panel"),
            layout: Layout {
                width: Some(Length::Px(WIDTH)),
                max_width: Some(Length::Percent(96.)),
                min_height: Some(Length::Percent(94.)),
                max_height: Some(Length::Percent(94.)),
                padding: Padding::all(3.),
                align: Some(Align::Stretch),
                justify: Justify::Start,
                ..column(1.4)
            },
            children: vec![
                with_id(
                    text_in(view.title.clone(), TextRole::Title, "panel.title"),
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
                        UiNodeDef::Custom {
                            kind: WidgetKind::new("slotted:hint_bar"),
                            params: slotted::registry::Value::Unit,
                            children: Vec::new(),
                            tags: Tags::new().with(Tags::TEST_ID, "harvester.hints"),
                        },
                        UiNodeDef::Button {
                            widget: Some(WidgetKind::new("slotted:close")),
                            opts: ButtonOpts {
                                label: Some(super::crafting::key("strata.menu.close")),
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
        initial_focus: Some(focus.to_owned()),
        presentation: Presentation {
            mode: PresentationMode::Page,
            scrim: Some(true),
            transition: Transition::Fade,
            wildcards: Some(false),
            ..Presentation::default()
        },
    }
}

// ---- open, answer, rebuild ----

/// What the harvester screen reads: the data, the machines, the survey for
/// the deposit under the machine and the local player's pack and place.
/// Reach is a distance, so the player's place counts by value: a character
/// standing still is not a change.
#[derive(SystemParam)]
pub struct HarvesterInputs<'w, 's> {
    registries: Option<Res<'w, Registries>>,
    harvesters: Option<Res<'w, Harvesters>>,
    survey: Option<Res<'w, Survey>>,
    players: Query<'w, 's, (Ref<'static, Pack>, &'static WorldPosition), With<LocalPlayer>>,
    at: Local<'s, Seen<[u64; 2]>>,
}

impl ChangeSource for HarvesterInputs<'static, 'static> {
    fn changed(inputs: &mut HarvesterInputs<'_, '_>) -> bool {
        let place = inputs
            .players
            .single()
            .ok()
            .map(|(pack, at)| (pack.is_changed(), [at.0.x().to_bits(), at.0.z().to_bits()]));
        let stepped = place.is_some_and(|(_, at)| inputs.at.differs(at));
        moved(&inputs.registries)
            || moved(&inputs.harvesters)
            || moved(&inputs.survey)
            || stepped
            || place.is_some_and(|(pack, _)| pack)
    }
}

impl SimScreen for HarvesterUi {
    type View = HarvesterView;
    type Inputs = HarvesterInputs<'static, 'static>;
    type Answering = ResMut<'static, HarvestInbox>;
    const KIND: &'static str = KIND;
    const CLOSES_WHEN_GONE: bool = true;

    fn view(inputs: &HarvesterInputs<'_, '_>, state: &Self) -> Option<HarvesterView> {
        let registries = &inputs.registries.as_deref()?.0;
        let id = state.id?;
        let (pack, at) = inputs.players.single().ok()?;
        let harvesters = inputs.harvesters.as_deref()?;
        let deposit = harvesters
            .get(id)
            .and_then(|machine| inputs.survey.as_deref()?.0.get(machine.deposit));
        HarvesterView::of(
            &Floor {
                registries,
                harvesters,
                pack: &pack,
                at: [at.0.x(), at.0.z()],
                deposit: deposit.as_ref(),
            },
            id,
        )
    }

    fn def(view: &HarvesterView, state: &Self) -> ScreenDef {
        screen_def(view, &view.focus(state.last.as_deref()))
    }

    fn shown(state: &Self) -> Option<&HarvesterView> {
        state.view.as_ref()
    }

    fn marked(state: &Self) -> bool {
        state.dirty
    }

    fn built(state: &mut Self, view: HarvesterView) {
        state.dirty = false;
        state.view = Some(view);
    }

    /// A Collect or Pick up button: the request the view worked out goes to
    /// the sim's inbox, one at a time.
    fn answer(ui: &mut Self, choice: &MenuChoice, inbox: &mut ResMut<HarvestInbox>) {
        let Some(request) = ui
            .view
            .as_ref()
            .and_then(|view| view.request_of(&choice.id))
        else {
            return;
        };
        if ui.asked == 0 {
            inbox.push(request);
            ui.asked += 1;
        }
        ui.last = Some(choice.id.clone());
        ui.dirty = true;
    }
}

/// Opens the screen of machine `id`, as a harvester's F does.
pub fn open(commands: &mut Commands, id: u32) {
    commands.queue(move |world: &mut World| {
        {
            let mut ui = world.resource_mut::<HarvesterUi>();
            ui.id = Some(id);
            ui.last = None;
            ui.asked = 0;
        }
        let Some(def) = sim_screen::build::<HarvesterUi>(world) else {
            tracing::warn!("no local player or no such harvester: nothing to read");
            return;
        };
        sim_screen::show::<HarvesterUi>(world, def);
    });
}

/// `Update`, always: the sim's answer as a toast wherever the player is.
pub fn hear(
    mut answers: MessageReader<HarvestAnswer>,
    survey: Option<Res<Survey>>,
    mut ui: ResMut<HarvesterUi>,
    mut commands: Commands,
) {
    for answer in answers.read() {
        ui.asked = ui.asked.saturating_sub(1);
        ui.dirty = true;
        let said = match &answer.outcome {
            Ok(HarvestDone::Planted { deposit, .. }) => fill(
                words("strata.harvester.toast.planted"),
                "place",
                &place_of(survey.as_deref(), *deposit),
            ),
            Ok(HarvestDone::Collected { units, left, .. }) => {
                if *left == 0 {
                    fill(
                        words("strata.harvester.toast.collected"),
                        "n",
                        &grouped(*units),
                    )
                } else {
                    fill(
                        &fill(
                            words("strata.harvester.toast.collected.left"),
                            "n",
                            &grouped(*units),
                        ),
                        "left",
                        &grouped(*left),
                    )
                }
            }
            Ok(HarvestDone::PickedUp { .. }) => {
                words("strata.harvester.toast.picked_up").to_owned()
            }
            // The tracker's marker says it.
            Ok(HarvestDone::Led { .. }) => continue,
            Err(refusal) => refusal_words(*refusal),
        };
        tracing::info!("harvester: {said}");
        super::progress::notice(&mut commands, said);
    }
}

fn place_of(survey: Option<&Survey>, deposit: veyra_sim::deposits::DepositId) -> String {
    survey
        .and_then(|survey| survey.0.get(deposit))
        .map_or_else(String::new, |deposit| deposit.place)
}

/// `Update`, always: a toast when a hopper fills.
pub fn hear_full(
    mut notices: MessageReader<HarvestNotice>,
    survey: Option<Res<Survey>>,
    mut commands: Commands,
) {
    for notice in notices.read() {
        let HarvestNotice::Full { deposit, .. } = notice;
        let said = fill(
            words("strata.harvester.toast.full"),
            "place",
            &place_of(survey.as_deref(), *deposit),
        );
        tracing::info!("harvester: {said}");
        super::progress::notice(&mut commands, said);
    }
}

/// Registers the state and the systems. Only called once the UI is wired.
pub(crate) fn register(app: &mut bevy::app::App) {
    app.add_sim_screen::<HarvesterUi>()
        .init_resource::<Harvesters>()
        .init_resource::<HarvestInbox>()
        .add_message::<HarvestAnswer>()
        .add_message::<HarvestNotice>()
        .add_systems(
            bevy::app::Update,
            (
                // The sim's answer marks the screen before the comparison.
                hear.after(ScreenStep::answer::<HarvesterUi>())
                    .before(ScreenStep::rebuild::<HarvesterUi>()),
                hear_full,
            ),
        );
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::{deposit, harvester_record, pack_with, planted, shipped};

    fn view_with(held: u32, carried: bool, at: [f64; 2]) -> HarvesterView {
        let registries = shipped();
        let calder = deposit(
            &registries,
            "strata:evaporite_salt",
            "Calder",
            [0, 0],
            [0., 0.],
        );
        let harvesters = Harvesters {
            list: vec![planted(&registries, &calder, [0., 0.], 600, held)],
            next: 1,
        };
        let record = harvester_record();
        let pack = pack_with(&registries, carried.then_some(&record));
        HarvesterView::of(
            &Floor {
                registries: &registries,
                harvesters: &harvesters,
                pack: &pack,
                at,
                deposit: Some(&calder),
            },
            0,
        )
        .expect("a view")
    }

    #[test]
    fn the_screen_at_empty_part_and_full() {
        let empty = view_with(0, false, [0., 0.]);
        assert_eq!(empty.held, "0 of 1,000");
        assert_eq!(empty.share, 0);
        assert_eq!(empty.reads, "The ground reads 60% here");
        assert_eq!(empty.rate, "Makes 720 units an hour");
        assert_eq!(empty.time, "Full in 1 h 24 min");
        assert!(empty.on.contains("Calder"), "{}", empty.on);
        assert_eq!(empty.by, "Planted by Ilse Marrow");
        // Nothing to collect, but it can be picked up.
        assert!(!empty.can_collect && empty.can_pick_up);
        assert_eq!(empty.collect_label, "Collect");
        assert_eq!(empty.why, refusal_words(HarvestRefusal::Empty));
        assert_eq!(empty.request_of(COLLECT), None);
        assert_eq!(
            empty.request_of(TRACK),
            Some(HarvestRequest::Lead { id: 0 }),
            "Track works from anywhere"
        );
        assert_eq!(
            empty.request_of(PICK_UP),
            Some(HarvestRequest::PickUp { id: 0 })
        );

        let part = view_with(120, false, [0., 0.]);
        assert_eq!(part.held, "120 of 1,000");
        assert_eq!(part.share, 120);
        assert_eq!(part.time, "Full in 1 h 14 min");
        assert_eq!(part.collect_label, "Collect 120");
        assert!(part.can_collect && !part.can_pick_up);
        assert_eq!(part.why, refusal_words(HarvestRefusal::NotEmpty));
        assert_eq!(
            part.request_of(COLLECT),
            Some(HarvestRequest::Collect { id: 0 })
        );
        assert_eq!(part.request_of(PICK_UP), None);
        assert_eq!(part.focus(None), COLLECT);

        let full = view_with(1000, false, [0., 0.]);
        assert_eq!(full.held, "1,000 of 1,000");
        assert_eq!(full.share, 1000);
        assert_eq!(full.time, "Full");
        // The starting pack has room for only part of a full hopper, which the
        // label says rather than promising all of it.
        assert!(full.can_collect && full.collect_label.starts_with("Collect "));

        // Out of reach, both are grey for one reason said once.
        let away = view_with(120, false, [50., 0.]);
        assert!(!away.can_collect && !away.can_pick_up);
        assert_eq!(away.why, refusal_words(HarvestRefusal::NotInReach));
        assert_eq!(away.focus(None), CLOSE_ID);

        // The definition lays out for any of them.
        let def = screen_def(&part, &part.focus(None));
        assert_eq!(def.kind, kind());
    }
}
