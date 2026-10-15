//! Compaction hooks: the shell lifecycle handlers of the three compaction events.
//!
//! A handler that exits 0 and prints one JSON object on stdout returns a result; any other
//! stdout stays a receipt. Handlers of an event run in configuration order and each result
//! replaces the previous one as a whole. A handler that fails, times out or prints something
//! else contributes nothing and the chain goes on; only a hook marked `critical` stops it.
//! Hooks run off the coordinator thread so a slow handler never blocks other agents, and the
//! pass's cancellation token ends them early.
//!
//! * `compaction_requested` (before): `{"cancel": bool, "compaction": {...}}`
//! * `compaction_summarizing`: `{"context": [string], "prompt": string, "preserve_data": {}}`
#[cfg(test)]
mod tests;
use super::super::{
    hooks::{Batch, Executed, Invocation, MESSAGES_TEXT_LIMIT},
    turn::Worker,
    *,
};
use super::manual::translate_cancellation;
use crate::config::{HookLifecycleEvent as Hook, LifecycleHookConfig};
use harness_compaction::CompactionMethod;
use serde_json::{json, Map, Value};
use std::{collections::BTreeMap, ops::ControlFlow, time::Instant};

/// Paths listed per file set in the hook input; the rest is counted, not listed.
const LISTED_FILES: usize = 200;

/// What the before-compaction hooks decided.
#[derive(Debug, Clone, Default, PartialEq)]
pub(in crate::coord) struct BeforeResult {
    /// Stop: no summary, no entry. Beats a custom compaction in the same result.
    pub cancel: bool,
    /// A trusted replacement the pass commits as supplied.
    pub compaction: Option<HookCompaction>,
}

/// A compaction a hook supplied. Its cut point and token count are trusted as given.
#[derive(Debug, Clone, PartialEq)]
pub(in crate::coord) struct HookCompaction {
    pub summary: String,
    pub short_summary: Option<String>,
    pub first_kept_seq: u64,
    pub tokens_before: u32,
    pub details: Option<Value>,
    pub preserve_data: BTreeMap<String, Value>,
}

/// What the summarizing hooks add to the summary request.
#[derive(Debug, Clone, Default, PartialEq)]
pub(in crate::coord) struct SummarizingResult {
    /// Extra context lines, in hook order, rendered unescaped before the instructions.
    pub context: Vec<String>,
    /// Replaces the base summary prompt; the public focus is still appended.
    pub prompt: Option<String>,
    /// Keys to carry on the entry; the engine's own keys win a conflict.
    pub preserve_data: BTreeMap<String, Value>,
}

/// Facts about the pass's preparation that a before hook may inspect.
#[derive(Debug, Clone, Default)]
pub(in crate::coord) struct PreparationFacts {
    /// The trigger label (`manual`, `threshold`, ...).
    pub reason: String,
    pub history_messages: usize,
    pub prefix_messages: usize,
    pub retained_messages: usize,
    pub is_split_turn: bool,
    pub tokens_before: u32,
    pub keep_recent_tokens: u32,
    pub reserve_tokens: u32,
    pub window_tokens: Option<u32>,
    pub first_kept_seq: u64,
    /// Journal sequence range the summarized branch covers.
    pub branch_from_seq: u64,
    pub branch_through_seq: u64,
    pub previous_summary: Option<String>,
    pub read_files: Vec<String>,
    pub modified_files: Vec<String>,
}

/// The entry a pass saved, as the after hook sees it.
#[derive(Debug, Clone)]
pub(in crate::coord) struct SavedCompaction {
    pub seq: u64,
    pub first_kept_seq: u64,
    pub summary: String,
    pub short_summary: Option<String>,
    pub method: Option<CompactionMethod>,
    pub tokens_before: u32,
    pub tokens_after: u32,
    pub from_hook: bool,
}

/// JavaScript-style truthiness, which decides whether a handler's result displaces the
/// previous one and whether `cancel` stops the chain.
fn truthy(value: &Value) -> bool {
    match value {
        Value::Null => false,
        Value::Bool(flag) => *flag,
        Value::Number(number) => number.as_f64().is_some_and(|n| n != 0.0),
        Value::String(text) => !text.is_empty(),
        Value::Array(_) | Value::Object(_) => true,
    }
}

fn preserve_map(value: Option<&Value>) -> BTreeMap<String, Value> {
    match value {
        Some(Value::Object(map)) => map.iter().map(|(k, v)| (k.clone(), v.clone())).collect(),
        _ => BTreeMap::new(),
    }
}

fn optional_text(object: &Map<String, Value>, key: &str) -> Option<String> {
    object.get(key).and_then(Value::as_str).map(str::to_owned)
}

/// A custom compaction needs a summary, a cut point and a token count; a result without
/// them is an ordinary result with nothing to commit.
fn parse_compaction(value: &Value) -> Option<HookCompaction> {
    let object = value.as_object()?;
    let summary = object.get("summary")?.as_str()?;
    if summary.trim().is_empty() {
        return None;
    }
    Some(HookCompaction {
        summary: summary.to_owned(),
        short_summary: optional_text(object, "short_summary"),
        first_kept_seq: object.get("first_kept_seq")?.as_u64()?,
        tokens_before: u32::try_from(object.get("tokens_before")?.as_u64()?).unwrap_or(u32::MAX),
        details: object.get("details").filter(|d| !d.is_null()).cloned(),
        preserve_data: preserve_map(object.get("preserve_data")),
    })
}

fn parse_before(object: &Map<String, Value>) -> BeforeResult {
    if object.get("cancel").is_some_and(truthy) {
        return BeforeResult {
            cancel: true,
            compaction: None,
        };
    }
    BeforeResult {
        cancel: false,
        compaction: object.get("compaction").and_then(parse_compaction),
    }
}

fn parse_summarizing(object: &Map<String, Value>) -> SummarizingResult {
    SummarizingResult {
        context: object
            .get("context")
            .and_then(Value::as_array)
            .map(|lines| {
                lines
                    .iter()
                    .filter_map(|line| line.as_str().map(str::to_owned))
                    .collect()
            })
            .unwrap_or_default(),
        prompt: optional_text(object, "prompt"),
        preserve_data: preserve_map(object.get("preserve_data")),
    }
}

fn capped(files: &[String]) -> Value {
    json!({
        "paths": files.iter().take(LISTED_FILES).collect::<Vec<_>>(),
        "count": files.len(),
    })
}

fn before_fields(prep: &PreparationFacts, public_focus: Option<&str>, session: &str) -> Value {
    json!({
        "outcome": "requested",
        "output_summary": prep.reason,
        "session_id": session,
        "reason": prep.reason,
        "preparation": {
            "history_messages": prep.history_messages,
            "prefix_messages": prep.prefix_messages,
            "retained_messages": prep.retained_messages,
            "is_split_turn": prep.is_split_turn,
            "tokens_before": prep.tokens_before,
            "keep_recent_tokens": prep.keep_recent_tokens,
            "reserve_tokens": prep.reserve_tokens,
            "window_tokens": prep.window_tokens,
            "first_kept_seq": prep.first_kept_seq,
            "read_files": capped(&prep.read_files),
            "modified_files": capped(&prep.modified_files),
        },
        "branch_from_seq": prep.branch_from_seq,
        "branch_through_seq": prep.branch_through_seq,
        "previous_summary": prep.previous_summary,
        "custom_instructions": public_focus,
    })
}

impl Worker {
    /// Runs the before-compaction handlers. `Err` means a critical hook failed or `cancel`
    /// fired; a veto is a successful `BeforeResult { cancel: true, .. }`.
    pub(in crate::coord) async fn run_before_hook(
        &self,
        cancel: &CancellationToken,
        prep: &PreparationFacts,
        public_focus: Option<&str>,
    ) -> Result<BeforeResult, CoordinatorError> {
        let mut result = BeforeResult::default();
        self.hook_chain(
            cancel,
            Hook::CompactionRequested,
            before_fields(prep, public_focus, &self.session),
            |object| {
                result = parse_before(&object);
                if result.cancel {
                    ControlFlow::Break(())
                } else {
                    ControlFlow::Continue(())
                }
            },
        )
        .await?;
        Ok(result)
    }

    /// Runs the summarizing handlers on the text of the summarized and prefix messages.
    pub(in crate::coord) async fn run_summarizing_hook(
        &self,
        cancel: &CancellationToken,
        messages_text: &str,
    ) -> Result<SummarizingResult, CoordinatorError> {
        let mut result = SummarizingResult::default();
        let fields = json!({
            "outcome": "summarizing",
            "session_id": self.session,
            "messages_text": messages_text,
            "messages_truncated": messages_text.len() > MESSAGES_TEXT_LIMIT,
        });
        self.hook_chain(cancel, Hook::CompactionSummarizing, fields, |object| {
            result = parse_summarizing(&object);
            ControlFlow::Continue(())
        })
        .await?;
        Ok(result)
    }

    /// Tells the after-compaction handlers what was saved. It runs after the append, cannot
    /// undo it and never fails the pass.
    pub(in crate::coord) async fn fire_after_hook(&self, saved: &SavedCompaction) {
        let fields = json!({
            "outcome": "applied",
            "session_id": self.session,
            "seq": saved.seq,
            "first_kept_seq": saved.first_kept_seq,
            "summary": saved.summary,
            "short_summary": saved.short_summary,
            "method": saved.method.map(|m| m.as_str()),
            "tokens_before": saved.tokens_before,
            "tokens_after": saved.tokens_after,
            "from_hook": saved.from_hook,
        });
        let outcome = self
            .hook_chain(&self.cancellation, Hook::CompactionApplied, fields, |_| {
                ControlFlow::Continue(())
            })
            .await;
        // The entry is saved. Handler failures already reached the receipts and the live
        // warning; what is left is a stopped or closed coordinator, which no caller can act on.
        drop(outcome);
    }

    /// Runs `event`'s handlers in order, feeding every JSON result to `visit`.
    async fn hook_chain(
        &self,
        cancel: &CancellationToken,
        event: Hook,
        fields: Value,
        mut visit: impl FnMut(Map<String, Value>) -> ControlFlow<()>,
    ) -> Result<(), CoordinatorError> {
        let (actor, task) = (self.actor.clone(), self.turn.id.clone());
        let invocations = {
            let (actor, task) = (actor.clone(), task.clone());
            self.handle
                .call(move |s| Ok(s.hook_invocations(event, &actor, Some(&task), &fields)))
                .await?
        };
        let mut reports = Vec::with_capacity(invocations.len());
        let mut cancelled = false;
        for invocation in invocations {
            let started = Instant::now();
            let Invocation {
                index,
                name,
                hook,
                prepared,
            } = invocation;
            let Some(mut executed) = prepared.run(cancel).await else {
                cancelled = true;
                break;
            };
            let object = executed.object.take();
            let failed = matches!(
                executed.status,
                HookExecutionStatus::Failed | HookExecutionStatus::Blocked
            );
            let critical = failed && hook.critical;
            reports.push(Report {
                index,
                name,
                hook,
                elapsed: started.elapsed(),
                executed,
            });
            if critical {
                break;
            }
            if let Some(object) = object
                && visit(object).is_break()
            {
                break;
            }
        }
        let agent = actor.agent_id.clone().unwrap_or_default();
        let recorded = self
            .handle
            .call(move |s| {
                let mut batch = Batch::default();
                for report in reports {
                    s.account_hook(
                        event,
                        report.index,
                        &report.name,
                        &report.hook,
                        &actor,
                        Some(&task),
                        report.elapsed,
                        report.executed,
                        &mut batch,
                    );
                }
                let failure = batch.failure.take();
                s.record_hook_batch(Some(&task), batch)?;
                Ok(failure)
            })
            .await?;
        if cancelled {
            return Err(translate_cancellation(
                CoordinatorError::Cancelled("compaction cancelled".into()),
                true,
                &agent,
            ));
        }
        match recorded {
            Some(message) => Err(CoordinatorError::Compaction(CompactionError::new(
                CompactionErrorKind::Hook,
                message,
            ))),
            None => Ok(()),
        }
    }
}

struct Report {
    index: usize,
    name: String,
    hook: LifecycleHookConfig,
    elapsed: std::time::Duration,
    executed: Executed,
}
