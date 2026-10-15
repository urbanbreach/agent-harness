//! Notes-backed context windows.
//!
//! With the setting on and the note, rollover, read and search tools callable, a full
//! window is not summarized. The agent keeps a persistent notebook, a rollover commits an
//! ordinary compaction that selects a smaller live window, and the whole branch stays
//! recoverable through `history://current/full`.
use super::super::{turn::Worker, *};
use serde_json::{json, Value};

mod explicit;
pub(in crate::coord) mod handle;
mod reminder;
mod rollover;
mod window;

pub(in crate::coord) use window::{
    inject_notebook, insert_recovered_request, notebook_message, recover_authoritative_request,
    rollover_summary,
};

/// Largest notebook revision, in UTF-8 bytes.
pub(in crate::coord) const NOTEBOOK_LIMIT: usize = 16_384;
/// The only notebook revision format.
pub(in crate::coord) const NOTEBOOK_VERSION: u8 = 1;
/// `details.kind` of a compaction the notes rollover wrote.
pub(in crate::coord) const ROLLOVER_KIND: &str = "notes_rollover";
const ROLLOVER_VERSION: u64 = 1;
/// Tools that must all be callable before the experiment changes any behavior.
const REQUIRED_TOOLS: [&str; 4] = ["context_notes", "new_context", "read", "grep"];

/// One saved notebook revision.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(in crate::coord) struct NotebookRevision {
    /// Journal sequence of the revision, which is also its entry id.
    pub seq: u64,
    pub text: String,
    pub at_ms: Option<u64>,
}

/// A revision a reader may use: the supported format within the size bound. Anything
/// else never shadows an earlier valid revision.
pub(in crate::coord) fn is_valid(note: &ContextNotesEvent) -> bool {
    note.version == NOTEBOOK_VERSION && note.text.len() <= NOTEBOOK_LIMIT
}

/// The newest valid revision for `agent` on the given active fold. An empty revision is
/// returned as is, because it clears the notebook.
pub(in crate::coord) fn latest_notebook(
    events: &[EventEnvelopeV1],
    agent: &str,
) -> Option<NotebookRevision> {
    events.iter().rev().find_map(|event| match &event.payload {
        EventV1::ContextNotes(note) if note.agent_id == agent && is_valid(note) => {
            Some(NotebookRevision {
                seq: event.seq,
                text: note.text.clone(),
                at_ms: event_ms(event),
            })
        }
        _ => None,
    })
}

fn event_ms(event: &EventEnvelopeV1) -> Option<u64> {
    let time = humantime::parse_rfc3339(event.ts.as_deref()?).ok()?;
    u64::try_from(time.duration_since(std::time::UNIX_EPOCH).ok()?.as_millis()).ok()
}

/// The details a rollover records, wrapping what a before hook supplied.
pub(in crate::coord) fn rollover_details(hook_details: Option<Value>) -> Value {
    let mut details = json!({"kind": ROLLOVER_KIND, "version": ROLLOVER_VERSION});
    if let (Some(hook), Some(object)) = (hook_details, details.as_object_mut()) {
        object.insert("hook_details".into(), hook);
    }
    details
}

/// Whether compaction details mark a notes rollover.
pub(in crate::coord) fn is_rollover(details: Option<&Value>) -> bool {
    details
        .and_then(|details| details.get("kind"))
        .and_then(Value::as_str)
        == Some(ROLLOVER_KIND)
}

impl Worker {
    /// The setting is on and every tool the experiment relies on is callable by name for
    /// this agent. A missing capability leaves the legacy behavior in place.
    pub(in crate::coord) fn notes_usable(&self) -> bool {
        self.compaction.notes_windows && {
            let callable = self.registry.prompt_definitions(
                &self.profile,
                self.tool_scope.as_deref(),
                &self.turn.model,
                (&self.permissions.0, &self.permissions.1),
            );
            REQUIRED_TOOLS
                .iter()
                .all(|name| callable.iter().any(|tool| tool.tool_id == *name))
        }
    }
}

#[cfg(test)]
mod handle_tests;
#[cfg(test)]
mod rollover_tests;
#[cfg(test)]
mod tests;
