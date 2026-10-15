//! What a rolled-over window contains: the notebook note, the fixed rollover notice and
//! the latest authoritative request. Live commits and journal replay build the same
//! context from these helpers.
use super::NotebookRevision;
use crate::coord::context::{Context, Entry};
use harness_compaction::{NoteKind, Origin};
use harness_providers::{CompletionMessage, MessageRole};

const NOTEBOOK: &str = include_str!("../../../../prompts/notes/notebook.md");
const ROLLOVER: &str = include_str!("../../../../prompts/notes/rollover-summary.md");

/// The wrapper tag the notebook text must not be able to close early.
const NOTEBOOK_TAG: &str = "experimental-context-notes";

/// The model-only text that carries a notebook revision.
pub(in crate::coord) fn notebook_message(revision: &NotebookRevision) -> String {
    let text = revision
        .text
        .replace(&format!("</{NOTEBOOK_TAG}"), &format!("<\\/{NOTEBOOK_TAG}"))
        .replace(&format!("<{NOTEBOOK_TAG}"), &format!("<\\{NOTEBOOK_TAG}"));
    NOTEBOOK.trim_end().replacen("{notes}", &text, 1)
}

/// The fixed summary of a rollover; no model writes it.
pub(in crate::coord) fn rollover_summary() -> String {
    ROLLOVER.trim_end().to_owned()
}

fn is_notebook(entry: &Entry) -> bool {
    entry.note_kind == Some(NoteKind::NotesWindow) && entry.turn.is_none()
}

fn notebook_entry(revision: &NotebookRevision) -> Entry {
    let mut entry = Entry::new(
        CompletionMessage::text(MessageRole::User, notebook_message(revision)),
        revision.seq,
        None,
    );
    entry.at_ms = revision.at_ms;
    entry.origin = Origin::Host;
    entry.note_kind = Some(NoteKind::NotesWindow);
    entry
}

/// Index just past a native Anthropic head summary, its immediate assistant message and
/// the tool results that follow it. A note placed anywhere earlier would split the
/// provider block from the signed assistant fold it belongs to.
fn after_native_fold(context: &Context, body: usize) -> Option<usize> {
    let native = context
        .entries
        .get(body)?
        .compaction
        .as_ref()?
        .native
        .as_ref()?;
    if !native.lane.starts_with("anthropic") {
        return None;
    }
    let mut end = body + 1;
    if context
        .entries
        .get(end)
        .is_some_and(|entry| entry.message.role == MessageRole::Assistant)
    {
        end += 1;
    }
    while context
        .entries
        .get(end)
        .is_some_and(|entry| entry.message.role == MessageRole::Tool)
    {
        end += 1;
    }
    Some(end)
}

/// Puts the latest notebook into a rebuilt context. It leads the conversation, before the
/// summary, except after a native Anthropic summary and the fold that follows it. An empty
/// revision injects nothing; a notebook already present is replaced.
pub(in crate::coord) fn inject_notebook(context: &mut Context, revision: &NotebookRevision) {
    context.entries.retain(|entry| !is_notebook(entry));
    if revision.text.is_empty() {
        return;
    }
    let body = context.body_start();
    let at = after_native_fold(context, body).unwrap_or(body);
    context.entries.insert(at, notebook_entry(revision));
}

/// A message that can authorize the work: typed by the user, or steered in by the parent.
/// Harness notes and peer messages are not authority.
fn is_authority(entry: &Entry) -> bool {
    entry.message.role == MessageRole::User
        && entry.compaction.is_none()
        && entry.branch_summary.is_none()
        && match entry.origin {
            Origin::User => true,
            Origin::Agent => entry.note_kind != Some(NoteKind::Peer),
            Origin::Host => false,
        }
}

/// Index of the newest authoritative request among the original entries that precede a
/// compaction, when it falls before `first_kept`. A request inside the kept suffix is
/// already part of the window and is never copied.
pub(in crate::coord) fn recover_authoritative_request(
    originals: &[Entry],
    first_kept: usize,
) -> Option<usize> {
    let index = originals.iter().rposition(is_authority)?;
    (index < first_kept).then_some(index)
}

/// Places a recovered request immediately after the head summary.
pub(in crate::coord) fn insert_recovered_request(context: &mut Context, request: Entry) {
    let body = context.body_start();
    let at = if context
        .entries
        .get(body)
        .is_some_and(|entry| entry.compaction.is_some())
    {
        body + 1
    } else {
        body
    };
    context
        .entries
        .insert(at.min(context.entries.len()), request);
}
