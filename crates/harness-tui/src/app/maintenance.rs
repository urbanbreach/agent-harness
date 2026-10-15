//! Maintenance lifecycle as the UI sees it: loader label while a pass runs, the outcome
//! branch when it ends, and the queue flush that follows the transition.
use super::context_status::format_tokens;
use super::notices::TranscriptNoticeKind;
use super::ActiveContextUsage;
use super::AppState;
use crate::{CompactionCommandKind, CompactionCommandOutcome};
use harness_core::event::{
    CompactionLifecycleEvent, CompactionMethod, CompactionStatusEvent, EventEnvelopeV1, EventV1,
};
use std::collections::BTreeMap;

/// One pass between its lifecycle start and end.
#[derive(Debug, Clone)]
pub(crate) struct ActiveMaintenance {
    pub(crate) operation_id: String,
    pub(crate) reason: String,
    pub(crate) action: String,
    /// Latest method the pass reported through its progress stages.
    pub(crate) method: Option<CompactionMethod>,
}

impl ActiveMaintenance {
    fn manual(&self) -> bool {
        self.reason == "manual"
    }
}

#[derive(Default)]
pub(crate) struct MaintenanceState {
    active: BTreeMap<String, ActiveMaintenance>,
    /// Last automatic-compaction status per agent, with the order it arrived in.
    status: BTreeMap<String, (u64, CompactionStatusEvent)>,
    arrivals: u64,
}

fn reason_prefix(reason: &str) -> &'static str {
    match reason {
        "overflow" => "Context overflow detected, ",
        "incomplete" => "Response incomplete, ",
        "idle" => "Idle ",
        _ => "",
    }
}

/// Label of an automatic pass: the method it is running wins over the generic action.
fn auto_label(action: &str, method: Option<CompactionMethod>) -> &'static str {
    match (action, method) {
        ("handoff", _) | (_, Some(CompactionMethod::Handoff)) => "Auto-handoff",
        ("shake", _) | (_, Some(CompactionMethod::Shake)) => "Auto-shake",
        (_, Some(CompactionMethod::Remote)) => "Auto server compaction",
        (_, Some(CompactionMethod::Archive)) => "Auto-archive",
        _ => "Auto context-full maintenance",
    }
}

/// Loader text of a pass; `None` for a pass that shows no loader (a manual shake).
pub(crate) fn loader_label(
    pass: &ActiveMaintenance,
    ascii: bool,
    cancel_hint: Option<&str>,
) -> Option<String> {
    let ellipsis = if ascii { "..." } else { "…" };
    let hint = cancel_hint.map_or_else(String::new, |key| format!(" ({key} to cancel)"));
    let body = if pass.manual() {
        match pass.action.as_str() {
            "shake" => return None,
            "handoff" => "Generating handoff".to_owned(),
            _ => "Compacting context".to_owned(),
        }
    } else {
        format!(
            "{}{}",
            reason_prefix(&pass.reason),
            auto_label(&pass.action, pass.method)
        )
    };
    Some(format!("{body}{ellipsis}{hint}"))
}

impl AppState {
    pub(crate) fn active_maintenance(&self) -> Option<&ActiveMaintenance> {
        self.maintenance
            .active
            .iter()
            .filter(|(agent, _)| !self.projection.is_child_agent(agent))
            .map(|(_, pass)| pass)
            .next_back()
    }

    /// A pass is running: its lifecycle started and has not ended, or its card is live.
    pub(crate) fn maintenance_running(&self) -> bool {
        self.active_maintenance().is_some() || self.active_compaction().is_some()
    }

    /// Agent whose pass a cancel request would stop.
    pub(crate) fn maintenance_agent(&self) -> Option<String> {
        self.active_compaction()
            .map(|card| card.agent_id.clone())
            .or_else(|| {
                self.maintenance
                    .active
                    .keys()
                    .find(|agent| !self.projection.is_child_agent(agent))
                    .cloned()
            })
    }

    /// The loader row text for the running pass, with the configured interrupt key unless a
    /// child session is focused.
    pub(crate) fn maintenance_loader_label(&self, ascii: bool) -> Option<String> {
        let pass = self.active_maintenance()?;
        let hint = (!self.current_subagent_session_present()).then(|| self.interrupt_key_hint());
        loader_label(pass, ascii, hint.as_deref())
    }

    pub(crate) fn note_compaction_status(&mut self, status: CompactionStatusEvent) {
        self.maintenance.arrivals += 1;
        let order = self.maintenance.arrivals;
        self.maintenance
            .status
            .insert(status.agent_id.clone(), (order, status));
    }

    /// The published status of the agent the chrome describes: the focused child, else the
    /// newest status of an agent that is not a child.
    pub(crate) fn maintenance_status(&self) -> Option<&CompactionStatusEvent> {
        if self.current_subagent_session_present() {
            return self
                .current_session_id()
                .and_then(|id| self.maintenance.status.get(id))
                .map(|(_, status)| status);
        }
        self.maintenance
            .status
            .iter()
            .filter(|(agent, _)| !self.projection.is_child_agent(agent))
            .max_by_key(|(_, (order, _))| *order)
            .map(|(_, (_, status))| status)
    }

    pub(crate) fn note_maintenance_method(&mut self, agent_id: &str, method: CompactionMethod) {
        if let Some(pass) = self.maintenance.active.get_mut(agent_id) {
            pass.method = Some(method);
        }
    }

    pub(super) fn begin_maintenance(&mut self, event: &CompactionLifecycleEvent) {
        self.cancel_idle_compaction();
        self.composer_dismiss_suggestion();
        self.maintenance.active.insert(
            event.agent_id.clone(),
            ActiveMaintenance {
                operation_id: event.operation_id.clone(),
                reason: event.reason.clone(),
                action: event.action.clone(),
                method: None,
            },
        );
    }

    pub(super) fn end_maintenance(&mut self, event: &CompactionLifecycleEvent) {
        let pass = self
            .maintenance
            .active
            .remove(&event.agent_id)
            .filter(|pass| pass.operation_id == event.operation_id);
        self.cancel_idle_compaction();
        self.projection.finish_live_compaction(&event.agent_id);
        if self.projection.is_child_agent(&event.agent_id) {
            return;
        }
        let manual =
            pass.as_ref().is_some_and(ActiveMaintenance::manual) || event.reason == "manual";
        let method = event
            .result
            .as_ref()
            .and_then(|result| result.method)
            .or_else(|| pass.as_ref().and_then(|pass| pass.method));
        let label = auto_label(&event.action, method);
        if !manual {
            self.report_automatic_end(event, method, label);
        } else if event.result.is_some() {
            self.settle_after_maintenance(
                event.result.as_ref().and_then(|result| result.tokens_after),
                event.action == "handoff",
            );
        }
        self.rearm_idle_compaction();
        if !manual {
            self.flush_maintenance_queue(event.will_retry);
        }
    }

    /// Outcome branches of an automatic pass, in the order the end event is judged.
    fn report_automatic_end(
        &mut self,
        event: &CompactionLifecycleEvent,
        method: Option<CompactionMethod>,
        label: &str,
    ) {
        let tokens_after = event.result.as_ref().and_then(|result| result.tokens_after);
        if event.aborted {
            self.push_transcript_notice(TranscriptNoticeKind::Status, format!("{label} cancelled"));
        } else if event.action == "shake" {
            if !event.skipped {
                self.settle_after_maintenance(None, false);
            }
            match &event.error {
                Some(error) => {
                    self.push_transcript_notice(TranscriptNoticeKind::Warning, error.clone())
                }
                None if !event.skipped => {
                    self.push_transcript_notice(
                        TranscriptNoticeKind::Status,
                        "Auto-shake completed".into(),
                    );
                }
                None => {}
            }
        } else if event.result.is_some() {
            self.settle_after_maintenance(tokens_after, false);
        } else if let Some(error) = &event.error {
            self.push_transcript_notice(TranscriptNoticeKind::Warning, error.clone());
        } else if event.action == "handoff" {
            self.settle_after_maintenance(None, true);
            self.push_transcript_notice(
                TranscriptNoticeKind::Status,
                "Auto-handoff completed".into(),
            );
        } else if !event.skipped {
            let failure = match method {
                Some(CompactionMethod::Archive) => "Auto-archive maintenance failed",
                Some(CompactionMethod::Remote) => "Auto server compaction failed",
                _ => "Auto context-full maintenance failed",
            };
            self.push_transcript_notice(
                TranscriptNoticeKind::Warning,
                format!("{failure}; continuing without maintenance"),
            );
        }
    }

    /// Rebuild what a finished pass changed: the cached context usage, the transcript, and
    /// for a handoff the todo list.
    pub(crate) fn settle_after_maintenance(
        &mut self,
        tokens_after: Option<u32>,
        reload_todos: bool,
    ) {
        self.projection.active_context_usage = Some(tokens_after.map_or_else(
            ActiveContextUsage::compacted_pending_refresh,
            ActiveContextUsage::estimate,
        ));
        if reload_todos {
            self.refresh_todo_items();
        }
        self.bump_transcript_render_epoch();
        self.sync_transcript_state(false);
    }

    /// Claude Code compacts a resident session itself. The journal records it on the
    /// finished request; without this row only a two-second toast would show it.
    pub(crate) fn note_native_compactions(&mut self, event: &EventEnvelopeV1) {
        let EventV1::ProviderRequestFinished(finish) = &event.payload else {
            return;
        };
        for compaction in finish.metadata.iter().flat_map(|m| &m.native_compactions) {
            let tokens = match (compaction.pre_tokens, compaction.post_tokens) {
                (Some(before), Some(after)) => {
                    format!(
                        " ({} → {} tokens)",
                        format_tokens(before),
                        format_tokens(after)
                    )
                }
                (Some(before), None) => format!(" ({} tokens before)", format_tokens(before)),
                _ => String::new(),
            };
            self.push_transcript_notice(
                TranscriptNoticeKind::Status,
                format!("Claude Code compacted this conversation{tokens}"),
            );
        }
    }
}

impl AppState {
    /// A manual command settled: show its outcome, then let queued input go, so queued
    /// turns start on the transitioned session.
    pub fn apply_compaction_command_outcome(
        &mut self,
        kind: CompactionCommandKind,
        outcome: CompactionCommandOutcome,
    ) {
        let noun = match kind {
            CompactionCommandKind::Handoff => "Handoff",
            CompactionCommandKind::Shake => "Shake",
            CompactionCommandKind::Compact | CompactionCommandKind::Idle => "Compaction",
        };
        match outcome {
            CompactionCommandOutcome::Done {
                message,
                tokens_after,
                saved_path,
                rebuild,
            } => {
                if rebuild {
                    self.settle_after_maintenance(
                        tokens_after,
                        kind == CompactionCommandKind::Handoff,
                    );
                }
                if kind == CompactionCommandKind::Handoff {
                    self.push_transcript_notice(
                        TranscriptNoticeKind::Success,
                        "Context handed off and compacted in place".into(),
                    );
                }
                if let Some(path) = saved_path {
                    self.push_transcript_notice(
                        TranscriptNoticeKind::Status,
                        format!("Handoff document saved to: {path}"),
                    );
                }
                if let Some(message) = message {
                    self.push_transcript_notice(TranscriptNoticeKind::Status, message);
                }
            }
            CompactionCommandOutcome::Warning(text) => {
                self.push_transcript_notice(TranscriptNoticeKind::Warning, text);
            }
            CompactionCommandOutcome::Cancelled => {
                self.push_transcript_notice(
                    TranscriptNoticeKind::Error,
                    format!("{noun} cancelled"),
                );
            }
            CompactionCommandOutcome::Failed(error) => {
                self.push_transcript_notice(
                    TranscriptNoticeKind::Error,
                    format!("{noun} failed: {error}"),
                );
            }
        }
        if kind != CompactionCommandKind::Idle {
            self.flush_maintenance_queue(false);
        }
    }
}
