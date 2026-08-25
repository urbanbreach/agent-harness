use std::collections::BTreeMap;

use super::facts::{LegacyFact, LegacyFactKind, ProjectionIndex};
use super::{
    LegacyAdapterError, LegacyAuditReference, LegacyCompactionFact, LegacyIdentityNamespace,
    LegacyProvenance, LegacySessionSnapshot, LegacyWarning,
};
use crate::event::SCHEMA_VERSION;
use crate::ids::{EntryId, RunId, SessionId};
use crate::session::{
    reducer, CanonicalRecord, CanonicalRecordKind, ProviderProvenance, RecordSequence, RunAttempt,
    RunStatus, SessionEntry, SessionEntryPayload, SessionMetadata,
};

mod helpers;

pub(super) fn project_facts(
    run_id: RunId,
    facts: &[LegacyFact],
    warnings: Vec<LegacyWarning>,
) -> Result<LegacySessionSnapshot, LegacyAdapterError> {
    let namespace = LegacyIdentityNamespace::new(&run_id);
    let index = ProjectionIndex::build(facts, &namespace);
    let mut projector = SessionProjector::new(&run_id, index, warnings);
    for fact in facts {
        projector.apply(fact)?;
    }
    projector.finish(facts)
}
struct SessionProjector<'a> {
    run_id: &'a RunId,
    namespace: LegacyIdentityNamespace<'a>,
    session_id: SessionId,
    index: ProjectionIndex,
    warnings: Vec<LegacyWarning>,
    records: Vec<CanonicalRecord>,
    active_leaf: Option<EntryId>,
}

impl<'a> SessionProjector<'a> {
    fn new(run_id: &'a RunId, index: ProjectionIndex, warnings: Vec<LegacyWarning>) -> Self {
        let namespace = LegacyIdentityNamespace::new(run_id);
        let session_id = namespace.session_id();
        let mut projector = Self {
            run_id,
            namespace,
            session_id,
            index,
            warnings,
            records: Vec::new(),
            active_leaf: None,
        };
        projector.push_record(CanonicalRecordKind::RunStarted {
            attempt: RunAttempt {
                run_id: run_id.clone(),
                status: RunStatus::Active,
                legacy_run_id: Some(run_id.to_string()),
            },
        });
        projector
    }

    fn apply(&mut self, fact: &LegacyFact) -> Result<(), LegacyAdapterError> {
        match &fact.kind {
            LegacyFactKind::Title(title) => self.apply_title(fact, title),
            LegacyFactKind::RunTerminal { run, session } => {
                self.push_record(CanonicalRecordKind::RunStatusChanged {
                    run_id: self.run_id.clone(),
                    status: *run,
                });
                self.push_record(CanonicalRecordKind::SessionStatusChanged { status: *session });
            }
            LegacyFactKind::User { request_id, text } => {
                self.push_entry(SessionEntry {
                    id: self
                        .namespace
                        .entry_id(fact.sequence, &fact.event_id, "user_message"),
                    parent_id: None,
                    turn_id: Some(self.namespace.turn_id(request_id)),
                    run_id: self.run_id.clone(),
                    payload: SessionEntryPayload::UserMessage {
                        text: text.clone(),
                        attachments: self
                            .index
                            .attachments
                            .get(request_id)
                            .cloned()
                            .unwrap_or_default(),
                    },
                });
            }
            LegacyFactKind::ProviderStarted(start) => self.apply_assistant(start)?,
            LegacyFactKind::ToolFinished(tool) => {
                let Some(assistant_id) = self.index.provider_entries.get(&tool.request_id) else {
                    return Err(LegacyAdapterError::InvalidIdentityRelationship {
                        event_id: fact.event_id.clone(),
                    });
                };
                let turn_id = self
                    .index
                    .provider_turns
                    .get(&tool.request_id)
                    .map(|turn| self.namespace.turn_id(turn));
                self.push_entry(SessionEntry {
                    id: self
                        .namespace
                        .entry_id(fact.sequence, &fact.event_id, "tool_result"),
                    parent_id: None,
                    turn_id,
                    run_id: self.run_id.clone(),
                    payload: SessionEntryPayload::ToolResult {
                        tool_call_id: tool.tool_call_id.clone(),
                        requesting_assistant_entry_id: assistant_id.clone(),
                        status: tool.status,
                        output_summary: tool.output_summary.clone(),
                        output_digest: tool.output_digest.clone(),
                        output_json: tool.output_json.clone(),
                    },
                });
            }
            LegacyFactKind::Compaction(compaction) => self.apply_compaction(fact, compaction),
            LegacyFactKind::CurrentIntent => {}
            LegacyFactKind::BranchSummary(summary) => self.push_entry(SessionEntry {
                id: self
                    .namespace
                    .entry_id(fact.sequence, &fact.event_id, "branch_summary"),
                parent_id: None,
                turn_id: None,
                run_id: self.run_id.clone(),
                payload: SessionEntryPayload::BranchSummary {
                    summary: summary.clone(),
                },
            }),
            LegacyFactKind::RunStarted
            | LegacyFactKind::Attachments { .. }
            | LegacyFactKind::AssistantPart { .. }
            | LegacyFactKind::ProviderFinished(_)
            | LegacyFactKind::AssistantFinished { .. }
            | LegacyFactKind::Noop => {}
        }
        Ok(())
    }
}
