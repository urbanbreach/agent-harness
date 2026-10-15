//! Append-only journal of the text tokens inline imaging avoided.
use crate::coord::context::archive::inline_cache::SavingsState;
use serde::Serialize;
use serde_json::Value;
use std::{
    collections::HashSet,
    io::{ErrorKind, Write},
    path::{Path, PathBuf},
};

/// File name under the Harness data directory.
pub(in crate::coord) const FILE_NAME: &str = "inline-savings.jsonl";

pub(in crate::coord) fn path(data_dir: &Path) -> PathBuf {
    data_dir.join(FILE_NAME)
}

/// Field order is the journal's wire order.
#[derive(Serialize)]
struct Line<'a> {
    ts: u64,
    session: &'a str,
    provider: &'a str,
    model: &'a str,
    #[serde(rename = "toolCallId")]
    tool_call_id: &'a str,
    #[serde(rename = "savedTokens")]
    saved_tokens: u64,
}

/// One applied tool swap and the tokens it saved.
pub(super) struct Saving {
    pub tool_call_id: String,
    pub saved_tokens: u64,
}

/// Appends one batch of savings; each tool call id is recorded once for the lifetime of
/// `state`. A missing session or a nonpositive saving is skipped without marking the id,
/// every other id is marked before any file work so a failed write is never retried.
/// I/O failures are debug logged and never reach the request.
pub(super) fn record(
    state: &SavingsState,
    data_dir: &Path,
    session: Option<&str>,
    provider: &str,
    model: &str,
    savings: &[Saving],
    now_ms: u64,
) {
    let Some(session) = session.filter(|session| !session.is_empty()) else {
        return;
    };
    let (batch, dir_ready) = {
        let Ok(mut inner) = state.inner.lock() else {
            return;
        };
        let mut batch = String::new();
        for saving in savings.iter().filter(|saving| saving.saved_tokens > 0) {
            if !inner.seen.insert(saving.tool_call_id.clone()) {
                continue;
            }
            let line = Line {
                ts: now_ms,
                session,
                provider,
                model,
                tool_call_id: &saving.tool_call_id,
                saved_tokens: saving.saved_tokens,
            };
            if let Ok(json) = serde_json::to_string(&line) {
                batch.push_str(&json);
                batch.push('\n');
            }
        }
        (batch, inner.dir_ready)
    };
    if batch.is_empty() {
        return;
    }
    if !dir_ready {
        if let Err(error) = std::fs::create_dir_all(data_dir) {
            tracing::debug!(%error, "inline savings directory unavailable");
            return;
        }
        if let Ok(mut inner) = state.inner.lock() {
            inner.dir_ready = true;
        }
    }
    let appended = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path(data_dir))
        .and_then(|mut file| file.write_all(batch.as_bytes()));
    if let Err(error) = appended {
        tracing::debug!(%error, "inline savings append failed");
    }
}

/// Every valid JSON line of the journal; a missing file is empty, malformed and blank
/// lines are skipped, any other read error propagates. Values are not schema checked.
pub(in crate::coord) fn read(data_dir: &Path) -> std::io::Result<Vec<Value>> {
    let text = match std::fs::read_to_string(path(data_dir)) {
        Ok(text) => text,
        Err(error) if error.kind() == ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => return Err(error),
    };
    Ok(text
        .lines()
        .filter(|line| !line.trim().is_empty())
        .filter_map(|line| serde_json::from_str(line).ok())
        .collect())
}

/// Entries of separate recorders can repeat an id; aggregates count `(session,
/// toolCallId)` once.
pub(in crate::coord) fn dedup(entries: Vec<Value>) -> Vec<Value> {
    let mut seen = HashSet::new();
    entries
        .into_iter()
        .filter(|entry| {
            seen.insert((
                entry.get("session").map(Value::to_string),
                entry.get("toolCallId").map(Value::to_string),
            ))
        })
        .collect()
}

/// Total saved tokens of deduplicated entries.
pub(in crate::coord) fn total_saved(entries: &[Value]) -> u64 {
    entries
        .iter()
        .filter_map(|entry| entry.get("savedTokens").and_then(Value::as_u64))
        .fold(0, u64::saturating_add)
}
