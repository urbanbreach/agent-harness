// allow: SIZE_OK — provider context data structures (compaction checkpoint + conversation turn + facts + serde validation)
use serde::{Deserialize, Deserializer, Serialize, Serializer};

use crate::attachment_transport::AttachmentMetadata;
use crate::conversation::ConversationMessage;
use crate::event::EventArtifactRef;
use crate::text::non_empty_trimmed;

pub(in crate::agent) const PROVIDER_TURN_FAILURE_REASON_MAX_CHARS: usize = 240;
const ALLOWED_PROVIDER_TURN_FAILURE_STAGES: &[&str] = &[
    "provider_error",
    "provider_abort",
    "tool_failure",
    "overflow_retry_failed",
    "hook_failure",
    "max_iters",
    "cancelled",
    "unknown",
];

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum ProviderConversationTurnStatus {
    #[default]
    Completed,
    Failed,
    Aborted,
}

impl ProviderConversationTurnStatus {
    pub fn is_completed(&self) -> bool {
        matches!(self, Self::Completed)
    }

    pub(in crate::agent) fn marker_label(self) -> &'static str {
        match self {
            Self::Completed => "completed",
            Self::Failed => "failed",
            Self::Aborted => "aborted",
        }
    }
}

pub(in crate::agent) fn is_allowed_provider_turn_failure_stage(stage: &str) -> bool {
    ALLOWED_PROVIDER_TURN_FAILURE_STAGES.contains(&stage)
}

fn serialize_provider_turn_failure_stage<S>(
    stage: &Option<String>,
    serializer: S,
) -> Result<S::Ok, S::Error>
where
    S: Serializer,
{
    match stage.as_deref() {
        Some(stage) if is_allowed_provider_turn_failure_stage(stage) => {
            serializer.serialize_some(stage)
        }
        Some(stage) => Err(serde::ser::Error::custom(format!(
            "unsupported provider turn failure stage `{stage}`"
        ))),
        None => serializer.serialize_none(),
    }
}

fn deserialize_provider_turn_failure_stage<'de, D>(
    deserializer: D,
) -> Result<Option<String>, D::Error>
where
    D: Deserializer<'de>,
{
    let stage = Option::<String>::deserialize(deserializer)?;
    if let Some(stage) = stage.as_deref() {
        if !is_allowed_provider_turn_failure_stage(stage) {
            return Err(serde::de::Error::custom(format!(
                "unsupported provider turn failure stage `{stage}`"
            )));
        }
    }
    Ok(stage)
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct ProviderConversationTurn {
    pub user_prompt: String,
    pub assistant_response: String,
    #[serde(
        default,
        skip_serializing_if = "ProviderConversationTurnStatus::is_completed"
    )]
    pub status: ProviderConversationTurnStatus,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        serialize_with = "serialize_provider_turn_failure_stage",
        deserialize_with = "deserialize_provider_turn_failure_stage"
    )]
    pub failure_stage: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub failure_reason: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub request_id: Option<crate::ids::RequestId>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub first_seq: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_seq: Option<u64>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub artifacts: Vec<EventArtifactRef>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub messages: Vec<ConversationMessage>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub attachments: Vec<AttachmentMetadata>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProviderContextCheckpointMetadata {
    pub checkpoint_id: String,
    pub agent_id: String,
    pub run_id: String,
    pub through_seq: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub through_request_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tokens_before: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tokens_before_estimate: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tokens_after_estimate: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub summary_tokens_estimate: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub compacted_turns: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub preserved_turns: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reduction_tokens_estimate: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reduction_percent_estimate: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub trigger_reason: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct ProviderCompactionTurnFact {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub request_id: Option<crate::ids::RequestId>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub first_seq: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_seq: Option<u64>,
    pub user_excerpt: String,
    pub assistant_excerpt: String,
    #[serde(
        default,
        skip_serializing_if = "ProviderConversationTurnStatus::is_completed"
    )]
    pub status: ProviderConversationTurnStatus,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        serialize_with = "serialize_provider_turn_failure_stage",
        deserialize_with = "deserialize_provider_turn_failure_stage"
    )]
    pub failure_stage: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub failure_reason: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub artifacts: Vec<EventArtifactRef>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct ProviderFileOperationFact {
    pub path: String,
    pub operation: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub first_seq: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_seq: Option<u64>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub sources: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub summary: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct ProviderCompactionFacts {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub previous_checkpoint_id: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub compacted_turns: Vec<ProviderCompactionTurnFact>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub relevant_artifacts: Vec<EventArtifactRef>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub read_files: Vec<ProviderFileOperationFact>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub modified_files: Vec<ProviderFileOperationFact>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub operation_facts: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub touched_files: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub pending_work: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub blockers: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProviderCompactionTailBoundary {
    pub mode: String,
    pub preserved_turns: u32,
    pub preserved_tokens_estimate: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub preserved_from_request_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub preserved_from_seq: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub split_prefix_summary: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProviderCompactionSummarySource {
    pub strategy: String,
    pub model_ref: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reasoning_effort: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub text_verbosity: Option<String>,
    pub previous_summary_used: bool,
    pub model_backed: bool,
    pub deterministic_fallback: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub summary_contract_version: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub summary_contract_enforced: Option<bool>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProviderCompactionTimelineEntry {
    pub entry_type: String,
    pub summary: String,
    pub first_kept_request_id: Option<String>,
    pub compacted_turns: u32,
    pub preserved_turns: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tokens_before_estimate: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tokens_after_estimate: Option<u32>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProviderContextCheckpoint {
    #[serde(flatten)]
    pub metadata: ProviderContextCheckpointMetadata,
    pub summary: String,
    #[serde(default)]
    pub recent_turns: Vec<ProviderConversationTurn>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub pruned_tool_artifacts: Vec<EventArtifactRef>,
    #[serde(default, skip_serializing_if = "ProviderCompactionFacts::is_empty")]
    pub facts: ProviderCompactionFacts,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tail_boundary: Option<ProviderCompactionTailBoundary>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub summary_source: Option<ProviderCompactionSummarySource>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub timeline_entry: Option<ProviderCompactionTimelineEntry>,
}

impl ProviderCompactionFacts {
    pub fn is_empty(&self) -> bool {
        self.previous_checkpoint_id.is_none()
            && self.compacted_turns.is_empty()
            && self.relevant_artifacts.is_empty()
            && self.read_files.is_empty()
            && self.modified_files.is_empty()
            && self.operation_facts.is_empty()
            && self.touched_files.is_empty()
            && self.pending_work.is_empty()
            && self.blockers.is_empty()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ProviderContext {
    pub compacted_summary: Option<String>,
    pub preserved_turns: Vec<ProviderConversationTurn>,
    pub checkpoint: Option<ProviderContextCheckpointMetadata>,
}

impl ProviderContext {
    pub fn from_turns(turns: Vec<ProviderConversationTurn>) -> Self {
        Self {
            compacted_summary: None,
            preserved_turns: turns,
            checkpoint: None,
        }
    }

    pub fn from_checkpoint(checkpoint: ProviderContextCheckpoint) -> Self {
        let summary =
            checkpoint_summary_with_operational_memory(&checkpoint.summary, &checkpoint.facts);
        Self {
            compacted_summary: Some(summary),
            preserved_turns: checkpoint.recent_turns,
            checkpoint: Some(checkpoint.metadata),
        }
    }

    pub fn push_turn(&mut self, turn: ProviderConversationTurn) {
        self.preserved_turns.push(turn);
    }

    pub fn is_empty(&self) -> bool {
        self.compacted_summary
            .as_deref()
            .and_then(non_empty_trimmed)
            .is_none()
            && self.preserved_turns.is_empty()
    }
}

fn checkpoint_summary_with_operational_memory(
    summary: &str,
    facts: &ProviderCompactionFacts,
) -> String {
    if summary.contains("## Operational Memory")
        || (facts.read_files.is_empty()
            && facts.modified_files.is_empty()
            && facts.operation_facts.is_empty())
    {
        return summary.to_string();
    }

    let mut lines = vec![summary.trim_end().to_string(), String::new()];
    lines.push("## Operational Memory".to_string());
    lines.push("Read files:".to_string());
    if facts.read_files.is_empty() {
        lines.push("- (none recorded)".to_string());
    } else {
        lines.extend(
            facts
                .read_files
                .iter()
                .take(12)
                .map(|fact| format!("- {}", fact.path)),
        );
    }
    lines.push("Modified files:".to_string());
    if facts.modified_files.is_empty() {
        lines.push("- (none recorded)".to_string());
    } else {
        lines.extend(
            facts
                .modified_files
                .iter()
                .take(12)
                .map(|fact| format!("- {}", fact.path)),
        );
    }
    for fact in facts.operation_facts.iter().take(20) {
        lines.push(format!("- {fact}"));
    }
    lines.join("\n")
}
