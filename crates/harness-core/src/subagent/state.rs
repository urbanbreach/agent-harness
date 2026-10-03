use super::*;
use crate::redact::Redactor;
use crate::{agent::AgentModelSettings, attachment_transport::AttachmentMetadata};
use harness_providers::{CompletionMessage, CompletionUsage};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
};

pub const FINALIZED_STATE_VERSION: u16 = 1;
pub const MAX_FINALIZED_STATE_BYTES: u64 = 8 * 1024 * 1024;

/// A settled model-facing item, before any history/presentation reconstruction.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FinalizedConversationItem {
    pub message: CompletionMessage,
    pub event_seq: u64,
    pub turn_id: Option<String>,
    pub attachments: Vec<AttachmentMetadata>,
    pub settled_reasoning: Vec<String>,
    pub raw_tool_result: Option<FinalizedToolResult>,
}

/// Actual native tool output, captured before the presentation size/redaction pass.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FinalizedToolResult {
    pub tool_call_id: String,
    pub provider_tool_call_id: Option<String>,
    pub output: Option<crate::tool::ToolResult>,
    pub error: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FinalizedProviderUsage {
    pub request_id: String,
    pub attempt_id: String,
    pub model_ref: String,
    pub usage: Option<CompletionUsage>,
    pub usage_complete: bool,
    pub thinking: Option<harness_providers::ProviderStreamThinkingMetadata>,
    pub settled_reasoning: Option<Vec<String>>,
}

/// Immutable finalized core state. There are deliberately no plans or signals.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RawFinalizedState {
    pub payload_version: u16,
    pub owner_run_id: String,
    pub owner_agent_id: SubagentId,
    pub owner_session_id: String,
    pub attempt_id: String,
    pub generation: u64,
    pub conversation_items: Vec<FinalizedConversationItem>,
    /// Last post-compaction model input; system prompt/tool declarations are not copied.
    pub model_request: Option<Box<harness_providers::CompletionRequest>>,
    pub usage: Vec<FinalizedProviderUsage>,
    pub source_model: String,
    pub model_target: Option<crate::config::ResolvedModelTarget>,
    pub model_settings: AgentModelSettings,
    pub execution_context: ResolvedSubagentContext,
    pub read_state: BTreeMap<PathBuf, String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub skill_startup: Option<crate::config::SkillStartupSnapshot>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub skill_preload_names: Vec<String>,
    pub source_reference: Option<Box<FinalizedAgentStateReferenceV1>>,
    pub fidelity: FinalizedStateFidelity,
    pub availability: FinalizedStateAvailability,
    pub unavailable: Option<FinalizedStateUnavailable>,
}

/// Exact state is never reconstructed from redacted summaries or delta fragments.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FinalizedStateUnavailable {
    Missing,
    Active,
    LegacySummaryOnly,
    PolicyModified,
    UnsupportedReasoning,
    UnsupportedEncoding,
    UnsupportedVersion,
    Corrupt,
    OwnerMismatch,
    Incomplete,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum FinalizedStateResult {
    Available { state: Box<RawFinalizedState> },
    Unavailable { reason: FinalizedStateUnavailable },
}

/// Run-relative content-addressed reference; it contains no raw conversation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FinalizedAgentStateReferenceV1 {
    pub payload_version: u16,
    pub state: FinalizedSubagentStateReference,
    pub byte_length: u64,
    pub owner_run_id: String,
    pub owner_session_id: String,
    pub attempt_id: String,
    pub generation: u64,
    pub unavailable: Option<FinalizedStateUnavailable>,
}

impl FinalizedAgentStateReferenceV1 {
    pub fn artifact_path(&self) -> String {
        format!("artifacts/{}.finalized-v1.json", self.state.sha256)
    }
}

/// A getter/decoder only: it never creates artifacts, journals, or worktrees.
pub fn resolve_finalized_state(
    run_dir: &Path,
    reference: &FinalizedAgentStateReferenceV1,
    owner_run: &str,
    owner: &SubagentId,
    attempt: &str,
) -> FinalizedStateResult {
    let unavailable = |reason| FinalizedStateResult::Unavailable { reason };
    if reference.owner_run_id != owner_run
        || &reference.state.owner != owner
        || reference.attempt_id != attempt
    {
        return unavailable(FinalizedStateUnavailable::OwnerMismatch);
    }
    if reference.payload_version != FINALIZED_STATE_VERSION
        || reference.state.payload_version != FINALIZED_STATE_VERSION
    {
        return unavailable(FinalizedStateUnavailable::UnsupportedVersion);
    }
    if let Some(reason) = reference.unavailable {
        return unavailable(reason);
    }
    if reference.state.fidelity != FinalizedStateFidelity::Exact {
        return unavailable(FinalizedStateUnavailable::PolicyModified);
    }
    if reference.state.availability != FinalizedStateAvailability::Available {
        return unavailable(FinalizedStateUnavailable::Missing);
    }
    match read_finalized_payload(run_dir, reference, owner_run, owner, attempt) {
        Ok(mut state) => match hydrate_state_attachments(&mut state, run_dir) {
            Ok(()) => FinalizedStateResult::Available {
                state: Box::new(state),
            },
            Err(reason) => unavailable(reason),
        },
        Err(reason) => unavailable(reason),
    }
}

/// Internal verified payload access for an authorized immutable session copy.
/// Availability remains checked by the public getter, never upgraded by a copy.
pub(crate) fn read_finalized_payload(
    run_dir: &Path,
    reference: &FinalizedAgentStateReferenceV1,
    owner_run: &str,
    owner: &SubagentId,
    attempt: &str,
) -> Result<RawFinalizedState, FinalizedStateUnavailable> {
    let unavailable = Err;
    if reference.payload_version != FINALIZED_STATE_VERSION
        || reference.state.payload_version != FINALIZED_STATE_VERSION
    {
        return unavailable(FinalizedStateUnavailable::UnsupportedVersion);
    }
    if reference.owner_run_id != owner_run
        || &reference.state.owner != owner
        || reference.attempt_id != attempt
    {
        return unavailable(FinalizedStateUnavailable::OwnerMismatch);
    }
    if reference.state.sha256.len() != 64
        || !reference
            .state
            .sha256
            .bytes()
            .all(|c| c.is_ascii_digit() || (b'a'..=b'f').contains(&c))
        || reference.byte_length > MAX_FINALIZED_STATE_BYTES
    {
        return unavailable(FinalizedStateUnavailable::Corrupt);
    }
    let bytes = match crate::store::read_private_bytes(
        &run_dir.join(reference.artifact_path()),
        MAX_FINALIZED_STATE_BYTES,
    ) {
        Ok(Some(bytes)) => bytes,
        Ok(None) => return unavailable(FinalizedStateUnavailable::Missing),
        Err(_) => return unavailable(FinalizedStateUnavailable::Corrupt),
    };
    if bytes.len() as u64 != reference.byte_length
        || format!("{:x}", Sha256::digest(&bytes)) != reference.state.sha256
    {
        return unavailable(FinalizedStateUnavailable::Corrupt);
    }
    let Ok(state) = serde_json::from_slice::<RawFinalizedState>(&bytes) else {
        return unavailable(FinalizedStateUnavailable::Corrupt);
    };
    if state.payload_version != FINALIZED_STATE_VERSION {
        return unavailable(FinalizedStateUnavailable::UnsupportedVersion);
    }
    if state.owner_run_id != reference.owner_run_id
        || state.owner_agent_id != reference.state.owner
        || state.owner_session_id != reference.owner_session_id
        || state.attempt_id != reference.attempt_id
        || state.generation != reference.generation
        || state.fidelity != reference.state.fidelity
    {
        return unavailable(FinalizedStateUnavailable::OwnerMismatch);
    }
    if state
        .conversation_items
        .iter()
        .any(|item| item.message.role == harness_providers::MessageRole::System)
        || state.model_request.iter().any(|request| {
            request
                .messages
                .iter()
                .any(|message| message.role == harness_providers::MessageRole::System)
        })
        || state.read_state.len() > 1024
        || state.availability != reference.state.availability
        || state.unavailable != reference.unavailable
        || (state.availability == FinalizedStateAvailability::Available
            && state.usage.iter().any(|entry| {
                (entry.usage_complete && entry.usage.is_none()) || entry.settled_reasoning.is_none()
            }))
        || state
            .conversation_items
            .iter()
            .filter_map(|item| item.raw_tool_result.as_ref())
            .any(|result| result.output.is_some() == result.error.is_some())
    {
        return unavailable(FinalizedStateUnavailable::Corrupt);
    }
    if crate::redact::redact_artifact_text(&String::from_utf8_lossy(&bytes))
        != String::from_utf8_lossy(&bytes)
    {
        return unavailable(FinalizedStateUnavailable::PolicyModified);
    }
    Ok(state)
}

fn hydrate_state_attachments(
    state: &mut RawFinalizedState,
    run_dir: &Path,
) -> Result<(), FinalizedStateUnavailable> {
    use harness_providers::attachment_protocol::{
        MAX_ATTACHMENT_BYTES, MAX_REQUEST_ATTACHMENT_BYTES,
    };
    let mut cached: BTreeMap<String, AttachmentMetadata> = BTreeMap::new();
    let mut bytes_read = 0usize;
    for attachment in state
        .conversation_items
        .iter_mut()
        .flat_map(|item| {
            item.attachments
                .iter_mut()
                .chain(item.raw_tool_result.iter_mut().flat_map(|result| {
                    result
                        .output
                        .iter_mut()
                        .flat_map(|output| &mut output.attachments)
                }))
        })
        .chain(
            state
                .model_request
                .iter_mut()
                .flat_map(|request| request.attachments.values_mut().flatten()),
        )
    {
        if let Some(loaded) = cached.get(&attachment.content_ref) {
            let mut copy = loaded.clone();
            copy.id.clone_from(&attachment.id);
            copy.mime.clone_from(&attachment.mime);
            copy.size = attachment.size;
            copy.dimensions = attachment.dimensions;
            copy.bytes()
                .map_err(|_| FinalizedStateUnavailable::Corrupt)?;
            *attachment = copy;
            continue;
        }
        let digest = attachment
            .content_ref
            .strip_prefix("attachment:blake3:")
            .filter(|s| s.len() == 64 && s.bytes().all(|c| c.is_ascii_hexdigit()))
            .ok_or(FinalizedStateUnavailable::Corrupt)?;
        let bytes = crate::store::read_private_bytes(
            &run_dir.join(format!("artifacts/{digest}.attachment")),
            MAX_ATTACHMENT_BYTES as u64,
        )
        .map_err(|_| FinalizedStateUnavailable::Corrupt)?
        .ok_or(FinalizedStateUnavailable::Missing)?;
        bytes_read = bytes_read.saturating_add(bytes.len());
        if bytes_read > MAX_REQUEST_ATTACHMENT_BYTES {
            return Err(FinalizedStateUnavailable::Incomplete);
        }
        let text = String::from_utf8_lossy(&bytes);
        if crate::redact::DefaultRedactor::default().redact_text(&text) != text {
            return Err(FinalizedStateUnavailable::PolicyModified);
        }
        *attachment = attachment
            .clone()
            .with_bytes(bytes)
            .map_err(|_| FinalizedStateUnavailable::Corrupt)?;
        cached.insert(attachment.content_ref.clone(), attachment.clone());
    }
    Ok(())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FinalizedContextCopy {
    Fork,
    Resume,
    Wake,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentContextInitializedV1 {
    pub payload_version: u16,
    pub agent_id: SubagentId,
    pub mode: FinalizedContextCopy,
    pub source: FinalizedAgentStateReferenceV1,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentExecutionContextChangedV1 {
    pub payload_version: u16,
    pub agent_id: SubagentId,
    pub context: ResolvedSubagentContext,
}

/// Explicit metadata, independent of generic task/job edges.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SubagentTransitionMetadataV1 {
    pub ancestry: SubagentAncestry,
    pub execution_owner: SubagentExecutionOwner,
    pub display_route: SubagentDisplayRoute,
    pub notification_route: SubagentNotificationRoute,
    pub injected_depth: InjectedSubagentDepth,
    pub isolation_requested: SubagentIsolationMode,
    pub context: ResolvedSubagentContext,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SubagentTransitionKind {
    Spawned,
    Progress,
    Finished,
    Retired,
    Routed,
}

/// Internally versioned payload inside the unchanged V1 event envelope.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SubagentTransitionV1 {
    pub payload_version: u16,
    pub child_id: SubagentId,
    pub attempt_id: Option<String>,
    pub generation: u64,
    pub notification_seq: Option<u64>,
    pub origin: LifecycleOrigin,
    pub transition: SubagentTransitionKind,
    pub metadata: SubagentTransitionMetadataV1,
    pub outcome: Option<SubagentTerminalOutcome>,
    pub accounting: Option<SubagentTerminalAccounting>,
    pub finalized_state: Option<FinalizedAgentStateReferenceV1>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SubagentCancelIntentV1 {
    pub payload_version: u16,
    pub command: SubagentCommandRequest,
    pub targets: Vec<SubagentId>,
}
