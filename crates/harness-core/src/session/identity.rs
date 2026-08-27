use crate::digest::digest32;
use crate::event::{EventEnvelopeV1, EventV1};
use crate::ids::{EntryId, ProviderRequestId, RunId, SessionId, TurnId};

#[derive(Debug, Clone)]
pub struct EventIdentityNamespace<'a> {
    run_id: &'a RunId,
}

impl<'a> EventIdentityNamespace<'a> {
    pub const fn new(run_id: &'a RunId) -> Self {
        Self { run_id }
    }

    pub fn session_id(&self) -> SessionId {
        SessionId::new(format!(
            "legacy-session-{}",
            digest32(format!("session\0{}", self.run_id).as_bytes())
        ))
    }

    pub fn entry_id(&self, sequence: u64, event_id: &str, semantic_kind: &str) -> EntryId {
        EntryId::new(format!(
            "legacy-entry-{}",
            digest32(
                format!(
                    "entry\0{}\0{sequence}\0{event_id}\0{semantic_kind}",
                    self.run_id
                )
                .as_bytes()
            )
        ))
    }

    pub fn source_entry_id(&self, event: &EventEnvelopeV1) -> Option<EntryId> {
        let semantic_kind = match event.payload {
            EventV1::SessionTitleUpdated(_) => "session_metadata",
            EventV1::UserMessageSubmitted(_) => "user_message",
            EventV1::ProviderRequestStarted(_) => "assistant_message",
            EventV1::SessionCompaction(_) => "compaction_summary",
            EventV1::BranchSummary(_) => "branch_summary",
            EventV1::ToolCallFinished(_) => "tool_result",
            _ => return None,
        };
        Some(self.entry_id(event.seq, &event.event_id, semantic_kind))
    }

    pub fn turn_id(&self, correlation_id: &str) -> TurnId {
        TurnId::new(format!(
            "legacy-turn-{}",
            digest32(format!("turn\0{}\0{correlation_id}", self.run_id).as_bytes())
        ))
    }

    pub fn provider_request_id(&self, request_id: &str) -> ProviderRequestId {
        ProviderRequestId::new(format!(
            "legacy-provider-request-{}",
            digest32(format!("provider-request\0{}\0{request_id}", self.run_id).as_bytes())
        ))
    }
}
