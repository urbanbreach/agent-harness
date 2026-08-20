use std::fs::File;
use std::io::{self, Write};

use serde::Serialize;

use crate::tui_fidelity::ScenarioAction;

#[derive(Serialize)]
#[serde(rename_all = "snake_case")]
enum InteractionEventClass {
    Key,
    Paste,
    Mouse,
    Wheel,
    Resize,
}

#[derive(Serialize)]
struct QueuedInteraction<'a> {
    interaction_id: &'a str,
    event_class: InteractionEventClass,
    receipt_count: u16,
}

pub(super) fn append(
    queue: &mut File,
    interaction_id: &str,
    action: &ScenarioAction,
    coalesced_type_text: bool,
) -> io::Result<()> {
    let Some((event_class, receipt_count)) = expected_receipts(action, coalesced_type_text) else {
        return Ok(());
    };
    serde_json::to_writer(
        &mut *queue,
        &QueuedInteraction {
            interaction_id,
            event_class,
            receipt_count,
        },
    )?;
    queue.write_all(b"\n")?;
    queue.flush()
}

pub(super) fn expected_receipt_count(
    action: &ScenarioAction,
    coalesced_type_text: bool,
) -> Option<u16> {
    expected_receipts(action, coalesced_type_text).map(|(_, count)| count)
}

fn expected_receipts(
    action: &ScenarioAction,
    coalesced_type_text: bool,
) -> Option<(InteractionEventClass, u16)> {
    match action {
        ScenarioAction::TimedKey(_) => Some((InteractionEventClass::Key, 1)),
        ScenarioAction::Paste(_) => Some((InteractionEventClass::Paste, 1)),
        ScenarioAction::TypeText(action) => Some((
            InteractionEventClass::Key,
            if coalesced_type_text {
                1
            } else {
                u16::try_from(action.text.chars().count()).unwrap_or(u16::MAX)
            },
        )),
        ScenarioAction::ClickText(_) => Some((InteractionEventClass::Mouse, 2)),
        ScenarioAction::WaitForText(_) => None,
        ScenarioAction::Mouse(_) => Some((InteractionEventClass::Mouse, 1)),
        ScenarioAction::Drag(_) => Some((InteractionEventClass::Mouse, 3)),
        ScenarioAction::Wheel(action) => Some((InteractionEventClass::Wheel, action.amount)),
        ScenarioAction::Resize(_) => Some((InteractionEventClass::Resize, 1)),
        ScenarioAction::TerminalReply(_) => Some((InteractionEventClass::Key, 1)),
        ScenarioAction::WaitForSemanticState(_) => None,
    }
}
