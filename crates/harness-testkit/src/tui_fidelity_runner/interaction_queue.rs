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
) -> io::Result<()> {
    let Some((event_class, receipt_count)) = expected_receipts(action) else {
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

fn expected_receipts(action: &ScenarioAction) -> Option<(InteractionEventClass, u16)> {
    match action {
        ScenarioAction::TimedKey(_) => Some((InteractionEventClass::Key, 1)),
        ScenarioAction::Paste(_) => Some((InteractionEventClass::Paste, 1)),
        ScenarioAction::TypeText(action) => Some((
            InteractionEventClass::Key,
            u16::try_from(action.text.len()).unwrap_or(u16::MAX),
        )),
        ScenarioAction::ClickText(_) => Some((InteractionEventClass::Mouse, 2)),
        ScenarioAction::WaitForText(_) => None,
        ScenarioAction::Mouse(_) => Some((InteractionEventClass::Mouse, 1)),
        ScenarioAction::Drag(_) => Some((InteractionEventClass::Mouse, 3)),
        ScenarioAction::Wheel(action) => Some((InteractionEventClass::Mouse, action.amount)),
        ScenarioAction::Resize(_) => Some((InteractionEventClass::Resize, 1)),
        ScenarioAction::TerminalReply(_) => Some((InteractionEventClass::Key, 1)),
        ScenarioAction::WaitForSemanticState(_) => None,
    }
}
