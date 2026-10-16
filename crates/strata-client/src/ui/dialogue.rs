//! A person's conversation (ticket 0014, step 2): who is speaking and for
//! whom, what they say, and what can be said back, numbered, with a greyed
//! reply's reason under it.
//!
//! No rule lives here. The sim's `DialogueView` is the whole of what is
//! shown; a reply is a `DialogueRequest::Choose` put in its inbox, and the
//! screen goes away when the sim says the conversation is over or the
//! player leaves it (Escape or Leave), which sends `Close`. The line has a
//! box of its own and the replies another, both of a fixed height, so a
//! shorter line or fewer replies move nothing on the screen.

use std::sync::Arc;

use bevy::ecs::prelude::*;
use bevy::input::ButtonInput;
use bevy::input::keyboard::KeyCode;
use slotted::menu::MenuChoice;
use slotted::theme::Role;
use slotted::ui::def::{
    Align, ButtonOpts, Justify, Layout, Length, Padding, Presentation, PresentationMode, ScreenDef,
    ScreenKind, Tags, TextOpts, TextRole, Transition, UiNodeDef, WidgetKind,
};
use slotted::ui::{ScreenStack, pop_screen, push_screen, replace_screen};
use strata_registry::quest::Condition;
use strata_sim::Registries;
use strata_sim::quests::{
    DialogueInbox, DialogueRefusal, DialogueRefused, DialogueRequest, DialogueView, QuestCast,
    ReplyView, fill as fill_names,
};

use super::crafting::{
    HEAD_ROLE, NOTE_ROLE, WHY_ROLE, act, column, key, line_in, panel, row, spacer, text_in,
    with_id, words,
};
use super::strings::grouped;

/// The screen kind.
pub const KIND: &str = "strata:dialogue";
/// The `test_id` of the root panel.
pub const ROOT_ID: &str = "dialogue.root";
/// The `test_id` of the speaker's name.
pub const NAME_ID: &str = "dialogue.name";
/// The `test_id` of the speaker's faction line.
pub const FACTION_ID: &str = "dialogue.faction";
/// The `test_id` of the line.
pub const LINE_ID: &str = "dialogue.line";
/// The `test_id` prefix of a reply's button; the place follows.
pub const REPLY_TAG: &str = "dialogue.reply.";
/// The `test_id` prefix of a greyed reply's reason; the place follows.
pub const WHY_TAG: &str = "dialogue.why.";
/// The `test_id` of the Leave button.
pub const LEAVE_ID: &str = "dialogue.leave";

/// The most replies a node shows, which the keys 1 to 4 reach and the box
/// of replies is made tall enough for.
pub const MAX_REPLIES: usize = 4;

const WIDTH: f32 = 780.;
/// The line's box: three short sentences at the body size fit with room.
const LINE_HEIGHT: f32 = 118.;
/// A reply and, when it is greyed, its reason under it.
const REPLIES_HEIGHT: f32 = 236.;

const KEYS: [KeyCode; MAX_REPLIES] = [
    KeyCode::Digit1,
    KeyCode::Digit2,
    KeyCode::Digit3,
    KeyCode::Digit4,
];
const PAD_KEYS: [KeyCode; MAX_REPLIES] = [
    KeyCode::Numpad1,
    KeyCode::Numpad2,
    KeyCode::Numpad3,
    KeyCode::Numpad4,
];

/// The screen kind.
#[must_use]
pub fn kind() -> ScreenKind {
    ScreenKind::new(KIND)
}

/// Why a reply is grey, in the player's terms, when the data gives no words
/// of its own.
#[must_use]
pub fn condition_words(condition: &Condition) -> String {
    let said = |key: &'static str| words(key).to_owned();
    match condition {
        Condition::QuestAt(..) | Condition::QuestDone(_) | Condition::Flag(_) => {
            said("strata.dialogue.why.not_yet")
        }
        Condition::NotFlag(_) => said("strata.dialogue.why.done"),
        Condition::PurseAtLeast(credits) => {
            words("strata.dialogue.why.credits").replace("{n}", &grouped(*credits))
        }
        Condition::Holds(..) => said("strata.dialogue.why.holds"),
        Condition::Learned(_) => said("strata.dialogue.why.learned"),
        Condition::Room(..) => said("strata.dialogue.why.room"),
        Condition::CarriesMade(..) => said("strata.dialogue.why.made"),
    }
}

/// A reply's button label: its number and what it says.
#[must_use]
pub fn reply_label(place: usize, reply: &ReplyView) -> String {
    format!("{}. {}", place + 1, reply.text)
}

/// A greyed reply's reason: the data's words, else the condition's.
#[must_use]
pub fn reply_reason(reply: &ReplyView) -> Option<String> {
    reply.blocked.as_ref().map(|condition| {
        reply
            .why
            .clone()
            .filter(|why| !why.is_empty())
            .unwrap_or_else(|| condition_words(condition))
    })
}

// ---- the state ----

/// The conversation screen's state.
#[derive(Resource, Debug, Default)]
#[allow(
    clippy::struct_excessive_bools,
    reason = "four independent latches in one screen's hand-off with the sim"
)]
pub struct DialogueUi {
    /// What the open screen shows.
    pub view: Option<DialogueView>,
    /// The faction line under the name, filled.
    pub faction: String,
    /// The sim has been told to open or to choose and has not answered.
    asked: bool,
    /// The newest view the sim gave, not yet drawn.
    pending: Option<DialogueView>,
    /// The sim refused something that ends the conversation.
    ended: bool,
    /// Whether the screen is thought open: leaving it must send `Close`.
    shown: bool,
    /// Whether the screen was on top last frame.
    was_on_top: bool,
}

// ---- the definition ----

/// The speaker's title with the factions named, `Field officer, <name>`.
#[must_use]
pub fn faction_line(title: &str, cast: &QuestCast) -> String {
    fill_names(title, cast)
}

fn reply_rows(view: &DialogueView) -> Vec<UiNodeDef> {
    view.replies
        .iter()
        .take(MAX_REPLIES)
        .enumerate()
        .flat_map(|(place, reply)| {
            let id = format!("{REPLY_TAG}{place}");
            let reason = reply_reason(reply);
            let mut rows = vec![act(&reply_label(place, reply), &id, true, reason.is_none())];
            if let Some(reason) = reason {
                rows.push(with_id(
                    text_in(reason, TextRole::Caption, WHY_ROLE),
                    &format!("{WHY_TAG}{place}"),
                ));
            }
            rows
        })
        .collect()
}

/// The whole screen for `view`.
#[must_use]
pub fn screen_def(view: &DialogueView, faction: &str) -> ScreenDef {
    let focus = view
        .replies
        .iter()
        .take(MAX_REPLIES)
        .position(|reply| reply.blocked.is_none())
        .map_or_else(
            || LEAVE_ID.to_owned(),
            |place| format!("{REPLY_TAG}{place}"),
        );
    let line = UiNodeDef::Text {
        key: key(view.line.clone()),
        style: TextRole::Body,
        opts: TextOpts::default(),
        tags: Tags::new().with(Tags::TEST_ID, LINE_ID),
    };
    ScreenDef {
        kind: kind(),
        inherits: None,
        root: UiNodeDef::Panel {
            role: Role::new("panel"),
            layout: Layout {
                width: Some(Length::Px(WIDTH)),
                max_width: Some(Length::Percent(96.)),
                padding: Padding::all(3.),
                align: Some(Align::Stretch),
                justify: Justify::Start,
                ..column(1.4)
            },
            children: vec![
                with_id(
                    text_in(view.name.clone(), TextRole::Title, "panel.title"),
                    NAME_ID,
                ),
                with_id(line_in(faction.to_owned(), HEAD_ROLE), FACTION_ID),
                UiNodeDef::Scroll {
                    layout: Layout {
                        height: Some(Length::Px(LINE_HEIGHT)),
                        min_height: Some(Length::Px(LINE_HEIGHT)),
                        max_height: Some(Length::Px(LINE_HEIGHT)),
                        align: Some(Align::Stretch),
                        ..column(0.)
                    },
                    scrollbar: true,
                    children: vec![line],
                    tags: Tags::new(),
                },
                UiNodeDef::Scroll {
                    layout: Layout {
                        height: Some(Length::Px(REPLIES_HEIGHT)),
                        min_height: Some(Length::Px(REPLIES_HEIGHT)),
                        max_height: Some(Length::Px(REPLIES_HEIGHT)),
                        align: Some(Align::Stretch),
                        ..column(0.8)
                    },
                    scrollbar: true,
                    children: reply_rows(view),
                    tags: Tags::new(),
                },
                panel(
                    "invisible",
                    row(2.),
                    vec![
                        line_in(words("strata.dialogue.keys"), NOTE_ROLE),
                        spacer(),
                        UiNodeDef::Button {
                            widget: Some(WidgetKind::new("slotted:close")),
                            opts: ButtonOpts {
                                label: Some(key("strata.dialogue.leave")),
                                ..ButtonOpts::default()
                            },
                            tags: Tags::new().with(Tags::TEST_ID, LEAVE_ID),
                        },
                    ],
                ),
            ],
            tags: Tags::new().with(Tags::TEST_ID, ROOT_ID),
        },
        listring: Vec::new(),
        remove: Vec::new(),
        initial_focus: Some(focus),
        presentation: Presentation {
            mode: PresentationMode::Modal,
            scrim: Some(true),
            transition: Transition::Fade,
            wildcards: Some(false),
            ..Presentation::default()
        },
    }
}

// ---- open, answer, rebuild ----

fn on_top(stack: &ScreenStack) -> bool {
    stack.top().is_some_and(|top| top.kind == kind())
}

/// `Update`, always: what the sim answered, kept for the rebuild.
pub fn hear(
    mut views: MessageReader<DialogueView>,
    mut refusals: MessageReader<DialogueRefused>,
    mut ui: ResMut<DialogueUi>,
) {
    for view in views.read() {
        ui.pending = Some(view.clone());
        ui.asked = false;
    }
    for refused in refusals.read() {
        ui.asked = false;
        if matches!(
            refused.why,
            DialogueRefusal::NotNear | DialogueRefusal::NobodyHome | DialogueRefusal::NothingOpen
        ) {
            ui.ended = true;
        }
    }
}

/// `Update`: a reply's button or its number key becomes the sim's request,
/// one at a time until the sim has answered.
pub fn answer(
    mut choices: MessageReader<MenuChoice>,
    keys: Res<ButtonInput<KeyCode>>,
    stack: Res<ScreenStack>,
    mut ui: ResMut<DialogueUi>,
    mut inbox: ResMut<DialogueInbox>,
) {
    if !on_top(&stack) {
        choices.clear();
        return;
    }
    let mut chosen: Option<usize> = None;
    for choice in choices.read() {
        if choice.screen != kind() {
            continue;
        }
        if let Some(place) = choice
            .id
            .strip_prefix(REPLY_TAG)
            .and_then(|place| place.parse().ok())
        {
            chosen = Some(place);
        }
    }
    if chosen.is_none() {
        chosen = (0..MAX_REPLIES)
            .find(|&place| keys.just_pressed(KEYS[place]) || keys.just_pressed(PAD_KEYS[place]));
    }
    let Some(place) = chosen else {
        return;
    };
    let open = ui.view.as_ref().is_some_and(|view| {
        view.replies
            .get(place)
            .is_some_and(|reply| reply.blocked.is_none())
    });
    if open && !ui.asked {
        inbox.0.push(DialogueRequest::Choose(place));
        ui.asked = true;
    }
}

/// Opens the conversation screen on `view`, as the sim's answer to an open.
fn show(world: &mut World, view: DialogueView) {
    let faction = {
        let registries = world.get_resource::<Registries>().map(|r| r.0.clone());
        let cast = world
            .get_resource::<QuestCast>()
            .cloned()
            .unwrap_or_default();
        registries
            .and_then(|registries| {
                registries
                    .quests
                    .person(&view.speaker)
                    .map(|person| faction_line(&person.title, &cast))
            })
            .unwrap_or_default()
    };
    let def = screen_def(&view, &faction);
    let on = on_top(world.resource::<ScreenStack>());
    {
        let mut ui = world.resource_mut::<DialogueUi>();
        ui.view = Some(view);
        ui.faction = faction;
        ui.shown = true;
    }
    if on {
        replace_screen(&mut world.commands(), Arc::new(def), None);
    } else {
        push_screen(&mut world.commands(), Arc::new(def), None);
    }
}

/// `Update`: draws the newest view, closes the screen when the sim ended the
/// conversation, and tells the sim when the player left it by Escape.
pub fn rebuild(world: &mut World) {
    let now = on_top(world.resource::<ScreenStack>());
    let (pending, ended, left) = {
        let mut ui = world.resource_mut::<DialogueUi>();
        let left = ui.was_on_top && !now && ui.shown;
        ui.was_on_top = now;
        (ui.pending.take(), std::mem::take(&mut ui.ended), left)
    };
    if left {
        world.resource_mut::<DialogueUi>().shown = false;
        world
            .resource_mut::<DialogueInbox>()
            .0
            .push(DialogueRequest::Close);
        return;
    }
    if let Some(view) = pending {
        if view.open {
            show(world, view);
            return;
        }
        let was = std::mem::replace(&mut world.resource_mut::<DialogueUi>().shown, false);
        if was && now {
            pop_screen(&mut world.commands());
        }
        return;
    }
    if ended {
        let was = std::mem::replace(&mut world.resource_mut::<DialogueUi>().shown, false);
        if was && now {
            pop_screen(&mut world.commands());
        }
    }
}

/// Registers the state and the systems. Only called once the UI is wired.
pub(crate) fn register(app: &mut bevy::app::App) {
    app.init_resource::<DialogueUi>()
        .init_resource::<DialogueInbox>()
        .add_message::<DialogueView>()
        .add_message::<DialogueRefused>()
        .add_systems(
            bevy::app::Update,
            (hear, answer, rebuild).chain().in_set(super::ScreenWork),
        );
}

#[cfg(test)]
mod tests {
    use super::*;
    use strata_registry::Namespaced;

    fn reply(text: &str, blocked: Option<Condition>, why: Option<&str>) -> ReplyView {
        ReplyView {
            text: text.to_owned(),
            blocked,
            why: why.map(str::to_owned),
        }
    }

    fn view(replies: Vec<ReplyView>) -> DialogueView {
        DialogueView {
            speaker: "veyra:person.pell".parse::<Namespaced>().expect("an id"),
            name: "Pell".to_owned(),
            line: "Words.".to_owned(),
            replies,
            open: true,
        }
    }

    #[test]
    fn a_reply_is_numbered_and_a_greyed_one_says_why() {
        let open = reply("Pay him.", None, None);
        assert_eq!(reply_label(0, &open), "1. Pay him.");
        assert_eq!(reply_reason(&open), None);
        let data = reply(
            "Talk.",
            Some(Condition::Flag("asked_intake".into())),
            Some("You have not asked Teague about the intake."),
        );
        assert_eq!(
            reply_reason(&data).as_deref(),
            Some("You have not asked Teague about the intake.")
        );
        let bare = reply("Pay.", Some(Condition::PurseAtLeast(1200)), None);
        assert_eq!(
            reply_reason(&bare).as_deref(),
            Some("You need 1,200 credits.")
        );
    }

    #[test]
    fn the_screen_focuses_the_first_reply_that_can_be_chosen() {
        let grey = reply("No.", Some(Condition::Flag("x".into())), None);
        let ok = reply("Yes.", None, None);
        let def = screen_def(&view(vec![grey.clone(), ok.clone()]), "Salvager");
        assert_eq!(def.kind, kind());
        assert_eq!(def.initial_focus.as_deref(), Some("dialogue.reply.1"));
        let none = screen_def(&view(vec![grey]), "");
        assert_eq!(none.initial_focus.as_deref(), Some(LEAVE_ID));
        let empty = screen_def(&view(Vec::new()), "");
        assert_eq!(empty.initial_focus.as_deref(), Some(LEAVE_ID));
    }
}
