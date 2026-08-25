mod projection;
mod tool_pairs;

use harness_providers::CompletionUsage;
use serde::{Deserialize, Serialize};

use crate::agent::AgentModelSettings;
use crate::attachment_transport::AttachmentMetadata;
use crate::config::ResolvedModelLimits;
use crate::ids::{EntryId, SessionId, ToolCallId, TurnId};

use super::{CanonicalSession, RecordSequence, SessionEntry, SessionError};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProviderViewOwner {
    pub agent_id: String,
    pub session: OwnedSession,
}

impl ProviderViewOwner {
    pub fn root(agent_id: impl Into<String>, session_id: SessionId) -> Self {
        Self {
            agent_id: agent_id.into(),
            session: OwnedSession::Root { session_id },
        }
    }

    pub fn child(
        agent_id: impl Into<String>,
        session_id: SessionId,
        root_session_id: SessionId,
    ) -> Self {
        Self {
            agent_id: agent_id.into(),
            session: OwnedSession::Child {
                session_id,
                root_session_id,
            },
        }
    }

    pub fn session_id(&self) -> &SessionId {
        match &self.session {
            OwnedSession::Root { session_id } | OwnedSession::Child { session_id, .. } => {
                session_id
            }
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OwnedSession {
    Root {
        session_id: SessionId,
    },
    Child {
        session_id: SessionId,
        root_session_id: SessionId,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CanonicalRuntimeSelection {
    pub profile: Option<String>,
    pub provider_id: String,
    pub model_id: String,
    pub variant: Option<String>,
    pub reasoning_effort: Option<String>,
    pub text_verbosity: Option<String>,
    pub reasoning_summary: Option<String>,
    pub thinking: Option<serde_json::Value>,
    pub resolved_limits: ResolvedModelLimits,
    pub profile_tool_shape_digest: String,
}

impl CanonicalRuntimeSelection {
    pub fn new(
        profile: Option<String>,
        provider_id: impl Into<String>,
        model_id: impl Into<String>,
        settings: AgentModelSettings,
        resolved_limits: ResolvedModelLimits,
        profile_tool_shape_digest: impl Into<String>,
    ) -> Result<Self, ProviderViewError> {
        let selection = Self {
            profile,
            provider_id: provider_id.into(),
            model_id: model_id.into(),
            variant: settings.variant,
            reasoning_effort: settings.reasoning_effort,
            text_verbosity: settings.text_verbosity,
            reasoning_summary: settings.reasoning_summary,
            thinking: settings.thinking,
            resolved_limits,
            profile_tool_shape_digest: profile_tool_shape_digest.into(),
        };
        selection.validate()?;
        Ok(selection)
    }

    fn validate(&self) -> Result<(), ProviderViewError> {
        if self.provider_id.trim().is_empty() {
            return Err(ProviderViewError::InvalidRuntimeSelection {
                field: "provider_id",
            });
        }
        if self.model_id.trim().is_empty() {
            return Err(ProviderViewError::InvalidRuntimeSelection { field: "model_id" });
        }
        if self.profile_tool_shape_digest.len() != 64
            || !self
                .profile_tool_shape_digest
                .bytes()
                .all(|byte| byte.is_ascii_hexdigit())
        {
            return Err(ProviderViewError::InvalidRuntimeSelection {
                field: "profile_tool_shape_digest",
            });
        }
        self.resolved_limits
            .validate(&format!("{}:{}", self.provider_id, self.model_id))
            .map_err(|_| ProviderViewError::InvalidRuntimeSelection {
                field: "resolved_limits",
            })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CanonicalPendingPrompt {
    pub turn_id: TurnId,
    pub text: String,
    pub attachments: Vec<AttachmentMetadata>,
}

pub struct ProviderViewInput {
    pub owner: ProviderViewOwner,
    pub selected_leaf: Option<EntryId>,
    pub pending_prompt: Option<CanonicalPendingPrompt>,
    pub runtime_selection: CanonicalRuntimeSelection,
}

#[derive(Debug, Clone, PartialEq)]
pub struct CanonicalProviderView {
    pub owner: ProviderViewOwner,
    pub selected_leaf: EntryId,
    pub active_entry_ids: Vec<EntryId>,
    pub entries: Vec<SessionEntry>,
    pub pending_prompt: Option<CanonicalPendingPrompt>,
    pub latest_compaction_summary: Option<CanonicalCompactionSummary>,
    pub tool_pairs: Vec<CanonicalToolPair>,
    pub attachments: Vec<CanonicalAttachment>,
    pub usage_boundaries: Vec<CanonicalUsageBoundary>,
    pub watermark: Option<RecordSequence>,
    pub runtime_selection: CanonicalRuntimeSelection,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CanonicalToolPair {
    pub tool_call_id: ToolCallId,
    pub assistant_entry_id: EntryId,
    pub result_entry_id: EntryId,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CanonicalAttachment {
    pub entry_id: EntryId,
    pub attachment: AttachmentMetadata,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CanonicalCompactionSummary {
    pub entry_id: EntryId,
    pub summary: String,
    pub first_kept_entry_id: EntryId,
    pub tokens_after: Option<u32>,
    pub usage: Option<CompletionUsage>,
    pub provider_id: Option<String>,
    pub model_id: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UsageBoundaryKind {
    Provider,
    Compaction,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CanonicalUsageBoundary {
    pub entry_id: EntryId,
    pub kind: UsageBoundaryKind,
    pub usage: CompletionUsage,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ProviderViewError {
    #[error(transparent)]
    InvalidSession(#[from] SessionError),
    #[error("provider-view owner session {actual} does not match canonical session {expected}")]
    OwnerSessionMismatch {
        expected: SessionId,
        actual: SessionId,
    },
    #[error("canonical session has no persisted active leaf")]
    MissingActiveLeaf,
    #[error("selected leaf {selected} does not match persisted active leaf {persisted}")]
    SelectedLeafMismatch {
        selected: EntryId,
        persisted: EntryId,
    },
    #[error("canonical runtime selection has invalid {field}")]
    InvalidRuntimeSelection { field: &'static str },
}

pub(crate) fn build(
    session: &CanonicalSession,
    input: ProviderViewInput,
) -> Result<CanonicalProviderView, ProviderViewError> {
    projection::build(session, input)
}

pub(crate) struct CanonicalActivePathSelection {
    pub selected_leaf: EntryId,
    pub active_entry_ids: Vec<EntryId>,
    pub entries: Vec<SessionEntry>,
    pub latest_compaction_summary: Option<CanonicalCompactionSummary>,
    pub tool_pairs: Vec<CanonicalToolPair>,
    pub attachments: Vec<CanonicalAttachment>,
    pub usage_boundaries: Vec<CanonicalUsageBoundary>,
}

pub(crate) fn select_active_path(
    session: &CanonicalSession,
    owner: &ProviderViewOwner,
    selected_leaf: Option<&EntryId>,
) -> Result<CanonicalActivePathSelection, ProviderViewError> {
    projection::select_active_path(session, owner, selected_leaf)
}
