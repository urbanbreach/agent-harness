use super::*;
use crate::{event::*, ids::EntryId};
use std::collections::{BTreeMap, HashMap};

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CanonicalSession {
    entries: BTreeMap<EntryId, SessionEntry>,
    active: Vec<(u64, EntryId)>,
    users: HashMap<String, EntryId>,
    tool_owners: HashMap<String, EntryId>,
}

impl CanonicalSession {
    pub fn entries(&self) -> &BTreeMap<EntryId, SessionEntry> {
        &self.entries
    }
    pub fn active_entries(&self) -> impl Iterator<Item = &SessionEntry> {
        self.active
            .iter()
            .filter_map(|(_, id)| self.entries.get(id))
    }
    pub(crate) fn apply(&mut self, event: &EventEnvelopeV1) {
        let id = EntryId::new(format!("entry-{}", event.seq));
        let payload = match &event.payload {
            EventV1::UserMessageSubmitted(data) => {
                self.users.insert(data.request_id.to_string(), id.clone());
                SessionEntryPayload::UserMessage {
                    text: data.text.clone(),
                    attachments: Vec::new(),
                }
            }
            EventV1::AssistantMessageFinished(data) => {
                for part in &data.parts {
                    if let AssistantPart::ToolCall(tool) = part {
                        self.tool_owners
                            .insert(tool.tool_call_id.to_string(), id.clone());
                    }
                }
                SessionEntryPayload::AssistantMessage {
                    parts: data.parts.clone(),
                    provenance: data.provenance.clone().map(Box::new),
                }
            }
            EventV1::ToolCallFinished(data) => {
                let Some(owner) = self.tool_owners.get(data.tool_call_id.as_str()) else {
                    return;
                };
                SessionEntryPayload::ToolResult {
                    tool_call_id: data.tool_call_id.clone(),
                    requesting_assistant_entry_id: owner.clone(),
                    status: match data.status {
                        ToolCallStatus::Succeeded => ToolResultStatus::Succeeded,
                        ToolCallStatus::Failed => ToolResultStatus::Failed,
                    },
                    output_summary: data.output_summary.clone(),
                    output_digest: data.output_digest.clone(),
                    output_json: data.output_json.clone(),
                }
            }
            EventV1::SessionCompaction(data) => {
                let kept = data.first_kept_entry_id.clone().or_else(|| {
                    self.active
                        .iter()
                        .find(|(seq, _)| *seq >= data.first_kept_event_seq)
                        .map(|(_, id)| id.clone())
                });
                let Some(kept) = kept else { return };
                SessionEntryPayload::CompactionSummary {
                    summary: data.summary.clone(),
                    first_kept_entry_id: kept,
                    tokens_after: data.tokens_after,
                    summary_usage: data.summary_usage.clone(),
                    summary_provider_id: data.summary_provider_id.clone(),
                    summary_model_id: data.summary_model_id.clone(),
                    preserved_state: Some(Box::new(CompactionPreservedState {
                        read_files: data.read_files.clone(),
                        modified_files: data.modified_files.clone(),
                        current_intent: data.current_intent.clone(),
                    })),
                }
            }
            EventV1::BranchSummary(data) => SessionEntryPayload::BranchSummary {
                summary: data.summary.clone(),
            },
            EventV1::SessionTitleUpdated(data) => SessionEntryPayload::SessionMetadata {
                title: Some(data.title.clone()),
            },
            EventV1::ConversationRewound(data) => {
                self.active.truncate(
                    self.active
                        .partition_point(|(seq, _)| *seq < data.target_seq),
                );
                return;
            }
            EventV1::PromptAttachmentsSubmitted(data) => {
                if let Some(entry) = self
                    .users
                    .get(data.request_id.as_str())
                    .and_then(|id| self.entries.get_mut(id))
                {
                    if let SessionEntryPayload::UserMessage { attachments, .. } = &mut entry.payload
                    {
                        attachments.clone_from(&data.attachments);
                    }
                }
                return;
            }
            _ => return,
        };
        let entry = SessionEntry {
            id: id.clone(),
            parent_id: self.active.last().map(|(_, id)| id.clone()),
            turn_id: event.correlation_id.as_deref().map(Into::into),
            run_id: event.run_id.clone(),
            payload,
        };
        self.entries.insert(id.clone(), entry);
        self.active.push((event.seq, id));
    }
}
