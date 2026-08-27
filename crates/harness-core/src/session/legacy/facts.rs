use std::collections::BTreeMap;

use harness_providers::CompletionUsage;
use serde_json::Value;

use super::{LegacyCompactionFact, LegacyIdentityNamespace};
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
        parts: Vec<AssistantPart>,
        provenance: Option<crate::session::ProviderProvenance>,
    },
    ToolFinished(ToolFinishFact),
    TurnCancelled(TurnCancelledFact),
    Compaction(LegacyCompactionFact),
    CurrentIntent,
    BranchSummary(String),
    Noop,
}

#[derive(Debug, Clone)]
pub(super) struct ProviderStartFact {
    pub request_id: String,
    pub turn_key: String,
    pub inferred_user_text: Option<String>,
    pub provider_id: String,
    pub model_id: String,
    pub runtime_selection: Option<Box<crate::session::CanonicalRuntimeSelection>>,
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

#[derive(Debug, Clone)]
pub(super) struct TurnCancelledFact {
    pub turn_key: String,
    pub status: String,
    pub stage: String,
    pub reason: String,
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
    pub semantic_parts_authoritative: bool,
    pub semantic_tool_requests_seen: usize,
    pub provenance: Option<crate::session::ProviderProvenance>,
}

#[derive(Debug, Default)]
pub(super) struct ProjectionIndex {
    pub attachments: BTreeMap<String, Vec<AttachmentMetadata>>,
    pub assistants: BTreeMap<String, AssistantAggregate>,
    pub source_entries: BTreeMap<u64, EntryId>,
    pub provider_entries: BTreeMap<String, EntryId>,
    pub provider_turns: BTreeMap<String, String>,
    pub last_provider_by_turn: BTreeMap<String, String>,
    pub partial_text_by_turn: BTreeMap<String, String>,
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
                        .entry(start.request_id.clone())
                        .or_insert_with(|| entry_id.clone());
                    self.provider_turns
                        .entry(start.request_id.clone())
                        .or_insert_with(|| start.turn_key.clone());
                    self.last_provider_by_turn
                        .insert(start.turn_key.clone(), start.request_id.clone());
                    self.assistants
                        .entry(start.request_id.clone())
                        .and_modify(|assistant| {
                            assistant.provider_id.clone_from(&start.provider_id);
                            assistant.model_id.clone_from(&start.model_id);
                        })
                        .or_insert_with(|| AssistantAggregate {
                            entry_id,
                            turn_key: start.turn_key.clone(),
                            provider_id: start.provider_id.clone(),
                            model_id: start.model_id.clone(),
                            parts: Vec::new(),
                            response_id: None,
                            stop_reason: None,
                            usage: None,
                            finished: false,
                            semantic_parts_authoritative: false,
                            semantic_tool_requests_seen: 0,
                            provenance: None,
                        });
                }
                LegacyFactKind::AssistantPart { request_id, part } => {
                    if let Some(assistant) = self.assistants.get_mut(request_id) {
                        if !assistant.semantic_parts_authoritative {
                            assistant.parts.push((fact.sequence, part.clone()));
                        } else if let AssistantPart::ToolCall(materialized) = part {
                            let committed = assistant
                                .parts
                                .iter_mut()
                                .filter_map(|(_, part)| match part {
                                    AssistantPart::ToolCall(tool_call) => Some(tool_call),
                                    AssistantPart::Text { .. }
                                    | AssistantPart::Reasoning { .. } => None,
                                })
                                .find(|committed| {
                                    committed.tool_call_id == materialized.tool_call_id
                                });
                            if let Some(committed) = committed {
                                committed.tool_call_id = materialized.tool_call_id.clone();
                                assistant.semantic_tool_requests_seen =
                                    assistant.semantic_tool_requests_seen.saturating_add(1);
                            }
                        }
                    }
                }
                LegacyFactKind::ProviderFinished(finish) => {
                    if let Some(assistant) = self.assistants.get_mut(&finish.request_id) {
                        assistant.response_id.clone_from(&finish.response_id);
                        assistant.stop_reason = Some(finish.stop_reason.clone());
                        assistant.usage.clone_from(&finish.usage);
                        assistant.finished = finish.stop_reason != "error";
                    }
                }
                LegacyFactKind::AssistantFinished {
                    request_id,
                    parts,
                    provenance,
                } => {
                    if let Some(assistant) = self.assistants.get_mut(request_id) {
                        assistant.finished = true;
                        if !parts.is_empty() {
                            assistant.parts = parts
                                .iter()
                                .cloned()
                                .map(|part| (fact.sequence, part))
                                .collect();
                            assistant.semantic_parts_authoritative = true;
                            assistant.semantic_tool_requests_seen = 0;
                            assistant.provenance.clone_from(provenance);
                        }
                    }
                }
                LegacyFactKind::RunStarted
                | LegacyFactKind::Title(_)
                | LegacyFactKind::RunTerminal { .. }
                | LegacyFactKind::User { .. }
                | LegacyFactKind::ToolFinished(_)
                | LegacyFactKind::TurnCancelled(_)
                | LegacyFactKind::Compaction(_)
                | LegacyFactKind::CurrentIntent
                | LegacyFactKind::BranchSummary(_)
                | LegacyFactKind::Noop => {}
            }
        }
        for assistant in self.assistants.values_mut() {
            assistant.parts.sort_by_key(|(sequence, _)| *sequence);
        }
        for (turn_key, request_id) in &self.last_provider_by_turn {
            let Some(assistant) = self.assistants.get(request_id) else {
                continue;
            };
            let text = assistant
                .parts
                .iter()
                .filter_map(|(_, part)| match part {
                    AssistantPart::Text { text } => Some(text.as_str()),
                    AssistantPart::Reasoning { .. } | AssistantPart::ToolCall(_) => None,
                })
                .collect::<String>();
            self.partial_text_by_turn.insert(turn_key.clone(), text);
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
                    start.inferred_user_text.as_ref().map_or_else(
                        || self.provider_entries.get(&start.request_id).cloned(),
                        |_| {
                            Some(namespace.entry_id(
                                fact.sequence,
                                &fact.event_id,
                                "inferred_user_message",
                            ))
                        },
                    )
                }
                LegacyFactKind::AssistantPart { request_id, .. }
                | LegacyFactKind::AssistantFinished { request_id, .. } => {
                    self.provider_entries.get(request_id).cloned()
                }
                LegacyFactKind::ProviderFinished(finish) => {
                    self.provider_entries.get(&finish.request_id).cloned()
                }
                LegacyFactKind::ToolFinished(_) => {
                    Some(namespace.entry_id(fact.sequence, &fact.event_id, "tool_result"))
                }
                LegacyFactKind::TurnCancelled(_) => {
                    Some(namespace.entry_id(fact.sequence, &fact.event_id, "turn_cancelled"))
                }
                LegacyFactKind::Title(_) => {
                    Some(namespace.entry_id(fact.sequence, &fact.event_id, "session_metadata"))
                }
                LegacyFactKind::Compaction(_) => {
                    Some(namespace.entry_id(fact.sequence, &fact.event_id, "compaction_summary"))
                }
                LegacyFactKind::CurrentIntent => None,
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
