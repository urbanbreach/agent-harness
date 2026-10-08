//! The session todo list, projected from durable `todowrite` results.
use crate::event::{EventEnvelopeV1, EventV1, ToolCallStatus};
use serde_json::{json, Value};
use std::collections::BTreeMap;

/// Folds events into the latest committed todo list.
///
/// Only successful `todowrite` results commit a version; a rewind drops versions and
/// pending calls at or after its target.
#[derive(Debug, Default, Clone)]
pub struct TodoProjection {
    pending: BTreeMap<String, u64>,
    // ponytail: retain todo versions for rewind; index on disk if long todo histories dominate RAM.
    versions: Vec<(u64, Value)>,
}

/// One todo entry as the model wrote it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TodoItem {
    pub content: String,
    pub status: String,
}

impl TodoItem {
    /// Pending and in-progress items still owe work.
    pub fn is_open(&self) -> bool {
        is_open_status(&self.status)
    }
}

fn is_open_status(status: &str) -> bool {
    matches!(status, "pending" | "in_progress")
}

impl TodoProjection {
    pub fn from_events<'a>(events: impl IntoIterator<Item = &'a EventEnvelopeV1>) -> Self {
        let mut state = Self::default();
        for event in events {
            state.apply(event);
        }
        state
    }

    pub fn apply(&mut self, event: &EventEnvelopeV1) {
        match &event.payload {
            EventV1::ToolCallRequested(e)
                if matches!(e.tool_id.as_str(), "todowrite" | "todo.write") =>
            {
                self.pending.insert(e.tool_call_id.to_string(), event.seq);
            }
            EventV1::ToolCallFinished(e)
                if self.pending.remove(e.tool_call_id.as_str()).is_some()
                    && e.status == ToolCallStatus::Succeeded =>
            {
                if let Some(value) = e
                    .output_json
                    .as_ref()
                    .and_then(|v| v.get("todos"))
                    .filter(|v| v.is_array())
                    && self
                        .versions
                        .last()
                        .is_none_or(|(_, previous)| previous != value)
                {
                    self.versions.push((event.seq, value.clone()));
                }
            }
            EventV1::ConversationRewound(e) => {
                self.versions.truncate(
                    self.versions
                        .partition_point(|(seq, _)| *seq < e.target_seq),
                );
                self.pending.retain(|_, seq| *seq < e.target_seq);
            }
            EventV1::TaskCancelled(e) => {
                self.pending.remove(e.task_id.as_str());
            }
            EventV1::RunFinished(_) | EventV1::RunFailed(_) => self.pending.clear(),
            _ => {}
        }
    }

    /// The latest committed list as written by the tool.
    pub fn current(&self) -> Value {
        self.versions
            .last()
            .map_or_else(|| json!([]), |(_, value)| value.clone())
    }

    /// Sequence of the event that committed the latest list, if any.
    pub fn version_seq(&self) -> Option<u64> {
        self.versions.last().map(|(seq, _)| *seq)
    }

    /// Whether the committed list contains an open item, without copying its text.
    pub fn has_open(&self) -> bool {
        self.versions
            .last()
            .and_then(|(_, value)| value.as_array())
            .is_some_and(|items| {
                items.iter().any(|item| {
                    item.get("content").is_some_and(Value::is_string)
                        && item
                            .get("status")
                            .and_then(Value::as_str)
                            .is_some_and(is_open_status)
                })
            })
    }

    pub fn items(&self) -> Vec<TodoItem> {
        self.versions
            .last()
            .and_then(|(_, value)| value.as_array())
            .map(|items| {
                items
                    .iter()
                    .filter_map(|item| {
                        Some(TodoItem {
                            content: item.get("content")?.as_str()?.to_owned(),
                            status: item.get("status")?.as_str()?.to_owned(),
                        })
                    })
                    .collect()
            })
            .unwrap_or_default()
    }
}
