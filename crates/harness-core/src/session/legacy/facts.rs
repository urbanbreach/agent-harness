use std::collections::BTreeMap;

use harness_providers::CompletionUsage;
use serde_json::Value;

use super::LegacyIdentityNamespace;
use crate::attachment_transport::AttachmentMetadata;
use crate::ids::{EntryId, ToolCallId};
use crate::session::{AssistantPart, RunStatus, SessionStatus, ToolResultStatus};

#[derive(Debug, Clone)]
pub(super) struct LegacyFact {
    pub sequence: u64,
    pub event_id: String,
    pub kind: LegacyFactKind,
}

#[derive(Debug, Clone)]
pub(super) enum LegacyFactKind {
    RunStarted,
    Title(String),
    RunTerminal {
        run: RunStatus,
        session: SessionStatus,
    },
    User {
        request_id: String,
        text: String,
    },
    Attachments {
        request_id: String,
        values: Vec<AttachmentMetadata>,
    },
    ProviderStarted(ProviderStartFact),
    AssistantPart {
        request_id: String,
        part: AssistantPart,
    },
    ProviderFinished(ProviderFinishFact),
    AssistantFinished {
        request_id: String,
    },
    ToolFinished(ToolFinishFact),
    Compaction {
        summary: String,
        first_kept_event_seq: u64,
    },
    BranchSummary(String),
    Noop,
}

#[derive(Debug, Clone)]
pub(super) struct ProviderStartFact {
    pub request_id: String,
    pub turn_key: String,
    pub provider_id: String,
    pub model_id: String,
}

#[derive(Debug, Clone)]
pub(super) struct ProviderFinishFact {
    pub request_id: String,
    pub response_id: Option<String>,
    pub stop_reason: String,
    pub usage: Option<CompletionUsage>,
}

#[derive(Debug, Clone)]
pub(super) struct ToolFinishFact {
    pub request_id: String,
    pub tool_call_id: ToolCallId,
    pub status: ToolResultStatus,
    pub output_summary: Option<String>,
    pub output_digest: Option<String>,
    pub output_json: Option<Value>,
}

#[derive(Debug)]
pub(super) struct AssistantAggregate {
    pub entry_id: EntryId,
    pub turn_key: String,
    pub provider_id: String,
    pub model_id: String,
    pub parts: Vec<(u64, AssistantPart)>,
    pub response_id: Option<String>,
    pub stop_reason: Option<String>,
    pub usage: Option<CompletionUsage>,
    pub finished: bool,
}

#[derive(Debug, Default)]
pub(super) struct ProjectionIndex {
    pub attachments: BTreeMap<String, Vec<AttachmentMetadata>>,
    pub assistants: BTreeMap<String, AssistantAggregate>,
    pub source_entries: BTreeMap<u64, EntryId>,
    pub provider_entries: BTreeMap<String, EntryId>,
    pub provider_turns: BTreeMap<String, String>,
}

impl ProjectionIndex {
    pub fn build(facts: &[LegacyFact], namespace: &LegacyIdentityNamespace<'_>) -> Self {
        let mut index = Self::default();
        index.collect_attachments_and_assistants(facts, namespace);
        index.collect_source_entries(facts, namespace);
        index
    }

    fn collect_attachments_and_assistants(
        &mut self,
        facts: &[LegacyFact],
        namespace: &LegacyIdentityNamespace<'_>,
    ) {
        for fact in facts {
            match &fact.kind {
                LegacyFactKind::Attachments { request_id, values } => self
                    .attachments
                    .entry(request_id.clone())
                    .or_default()
                    .extend(values.clone()),
                LegacyFactKind::ProviderStarted(start) => {
                    let entry_id =
                        namespace.entry_id(fact.sequence, &fact.event_id, "assistant_message");
                    self.provider_entries
                        .insert(start.request_id.clone(), entry_id.clone());
                    self.provider_turns
                        .insert(start.request_id.clone(), start.turn_key.clone());
                    self.assistants.insert(
                        start.request_id.clone(),
                        AssistantAggregate {
                            entry_id,
                            turn_key: start.turn_key.clone(),
                            provider_id: start.provider_id.clone(),
                            model_id: start.model_id.clone(),
                            parts: Vec::new(),
                            response_id: None,
                            stop_reason: None,
                            usage: None,
                            finished: false,
                        },
                    );
                }
                LegacyFactKind::AssistantPart { request_id, part } => {
                    if let Some(assistant) = self.assistants.get_mut(request_id) {
                        assistant.parts.push((fact.sequence, part.clone()));
                    }
                }
                LegacyFactKind::ProviderFinished(finish) => {
                    if let Some(assistant) = self.assistants.get_mut(&finish.request_id) {
                        assistant.response_id.clone_from(&finish.response_id);
                        assistant.stop_reason = Some(finish.stop_reason.clone());
                        assistant.usage.clone_from(&finish.usage);
                    }
                }
                LegacyFactKind::AssistantFinished { request_id } => {
                    if let Some(assistant) = self.assistants.get_mut(request_id) {
                        assistant.finished = true;
                    }
                }
                LegacyFactKind::RunStarted
                | LegacyFactKind::Title(_)
                | LegacyFactKind::RunTerminal { .. }
                | LegacyFactKind::User { .. }
                | LegacyFactKind::ToolFinished(_)
                | LegacyFactKind::Compaction { .. }
                | LegacyFactKind::BranchSummary(_)
                | LegacyFactKind::Noop => {}
            }
        }
        for assistant in self.assistants.values_mut() {
            assistant.parts.sort_by_key(|(sequence, _)| *sequence);
        }
    }

    fn collect_source_entries(
        &mut self,
        facts: &[LegacyFact],
        namespace: &LegacyIdentityNamespace<'_>,
    ) {
        let users = facts
            .iter()
            .filter_map(|fact| match &fact.kind {
                LegacyFactKind::User { request_id, .. } => Some((
                    request_id.clone(),
                    namespace.entry_id(fact.sequence, &fact.event_id, "user_message"),
                )),
                _ => None,
            })
            .collect::<BTreeMap<_, _>>();

        for fact in facts {
            let entry_id = match &fact.kind {
                LegacyFactKind::User { request_id, .. }
                | LegacyFactKind::Attachments { request_id, .. } => users.get(request_id).cloned(),
                LegacyFactKind::ProviderStarted(start) => {
                    self.provider_entries.get(&start.request_id).cloned()
                }
                LegacyFactKind::AssistantPart { request_id, .. }
                | LegacyFactKind::AssistantFinished { request_id } => {
                    self.provider_entries.get(request_id).cloned()
                }
                LegacyFactKind::ProviderFinished(finish) => {
                    self.provider_entries.get(&finish.request_id).cloned()
                }
                LegacyFactKind::ToolFinished(_) => {
                    Some(namespace.entry_id(fact.sequence, &fact.event_id, "tool_result"))
                }
                LegacyFactKind::Title(_) => {
                    Some(namespace.entry_id(fact.sequence, &fact.event_id, "session_metadata"))
                }
                LegacyFactKind::Compaction { .. } => {
                    Some(namespace.entry_id(fact.sequence, &fact.event_id, "compaction_summary"))
                }
                LegacyFactKind::BranchSummary(_) => {
                    Some(namespace.entry_id(fact.sequence, &fact.event_id, "branch_summary"))
                }
                LegacyFactKind::RunStarted
                | LegacyFactKind::RunTerminal { .. }
                | LegacyFactKind::Noop => None,
            };
            if let Some(entry_id) = entry_id {
                self.source_entries.insert(fact.sequence, entry_id);
            }
        }
    }
}
