use super::notices::TranscriptNoticeKind;
use super::{AppState, UiIntent};
use harness_compaction::ManualMode;
use harness_core::event::EventV1;

impl AppState {
    pub(in crate::app) fn has_compaction_messages(&self) -> bool {
        let hidden = self.hidden_delegated_child_request_ids_in_current_view();
        self.events()
            .filter(|event| {
                event
                    .correlation_id
                    .as_deref()
                    .is_none_or(|id| !hidden.contains(id))
            })
            .filter(|event| {
                // Every journal message counts: tool results and steering are messages too.
                matches!(
                    event.payload,
                    EventV1::UserMessageSubmitted(_)
                        | EventV1::AssistantMessageFinished(_)
                        | EventV1::ToolCallFinished(_)
                        | EventV1::SteeringAccepted(_)
                        | EventV1::RuntimeReminder(_)
                        | EventV1::BackgroundTaskNotification(_)
                )
            })
            .take(2)
            .count()
            == 2
    }

    pub(in crate::app) fn request_manual_compaction(
        &mut self,
        instructions: Option<String>,
        mode: Option<ManualMode>,
    ) {
        if !self.has_compaction_messages() {
            self.push_transcript_notice(
                TranscriptNoticeKind::Warning,
                "Nothing to compact (no messages yet)".into(),
            );
            return;
        }
        self.emit_ui_intent(UiIntent::CompactSession { instructions, mode });
    }

    /// `/handoff [focus]`: refuses while a turn or a compaction runs and when the session
    /// has nothing to hand off; each refusal is a warning.
    pub(in crate::app) fn request_handoff(&mut self, args: String) {
        let refusal = if self.active_turn_in_progress() {
            Some("Wait for the current response to finish or abort it before handing off.")
        } else if self.maintenance_running() {
            Some("Wait for context compaction to finish or cancel it before handing off.")
        } else if !self.has_compaction_messages() {
            Some("Nothing to hand off (no messages yet)")
        } else {
            None
        };
        match refusal {
            Some(text) => self.push_transcript_notice(TranscriptNoticeKind::Warning, text.into()),
            None => self.emit_ui_intent(UiIntent::Handoff {
                focus: (!args.is_empty()).then_some(args),
            }),
        }
    }

    /// Rounded share of the context window the request occupies; `None` without a
    /// measured estimate and a known window.
    pub(in crate::app) fn context_used_percent(&self) -> Option<u64> {
        let snapshot = self.current_request_budget_snapshot().filter(|snapshot| {
            snapshot.status == harness_core::context_budget::BudgetStatus::Estimated
        })?;
        let window = snapshot
            .maximum_input_tokens?
            .checked_add(snapshot.reserved_output_tokens.unwrap_or(0))
            .filter(|window| *window > 0)?;
        Some(
            (u64::from(snapshot.occupied_input_tokens) * 100 + u64::from(window) / 2)
                / u64::from(window),
        )
    }
}
