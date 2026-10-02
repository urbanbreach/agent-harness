//! Native subagent wire contracts and attempt-aware lifecycle reduction.
//!
//! This module contains no executor or persistence authority. Its lifecycle
//! fold returns effects for the coordinator to commit and project.

use serde::{Deserialize, Serialize};

mod history;
mod lifecycle;
mod state;
mod wire;
pub use history::*;
pub(crate) use state::read_finalized_payload;
pub use state::*;

pub use lifecycle::{
    AcceptedLifecycle, LifecycleEffect, LifecyclePhase, LifecycleReduction, LifecycleState,
    LifecycleTransition, SubagentAttemptKey,
};
pub use wire::{
    GetCommandOrSubagentOutputInput, GetCommandOrSubagentOutputResult,
    GetCommandOrSubagentOutputResults, GetCommandOrSubagentOutputValue, KillCommandOrSubagentInput,
    KillCommandOrSubagentResult, KillCommandOrSubagentValue, SendSubagentMessageDelivery,
    SendSubagentMessageInput, SendSubagentMessageOutput, SendSubagentMessageQuotaKind,
    SpawnSubagentInput, SpawnSubagentOutput, SubagentIsolationMode, WaitCommandsOrSubagentsInput,
    WaitCommandsOrSubagentsMode,
};

/// Child identity used by ancestry and command contracts.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct SubagentId(pub String);

/// Immutable creation ancestry. Missing values preserve legacy records.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SubagentAncestry {
    spawner: Option<SubagentId>,
    origin: Option<SubagentId>,
}

impl SubagentAncestry {
    /// Create immutable ancestry; absent links represent legacy records.
    pub fn new(spawner: Option<SubagentId>, origin: Option<SubagentId>) -> Self {
        Self { spawner, origin }
    }

    /// Return the direct spawner, when recorded.
    pub fn spawner(&self) -> Option<&SubagentId> {
        self.spawner.as_ref()
    }

    /// Return the root origin, when recorded.
    pub fn origin(&self) -> Option<&SubagentId> {
        self.origin.as_ref()
    }
}

/// Source of a lifecycle transition.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LifecycleOrigin {
    Stream,
    Reconciliation,
}

/// Session that owns execution, independent of notification and display routes.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum SubagentExecutionOwner {
    RootSession { session_id: String },
    ChildSession { child_id: SubagentId },
}

/// Mutable policy for delivering completion to the parent.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SubagentNotificationRoute {
    pub parent_prompt_id: Option<String>,
    pub background: bool,
    pub await_to_completion: bool,
    pub surface_completion: bool,
}

/// Presentation destination; deliberately separate from immutable ancestry.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SubagentDisplayRoute {
    pub root_session_id: String,
    pub parent_session_id: Option<String>,
    pub child_session_id: String,
}

/// Depth injected by the public caller. This is not derived from ancestry.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct InjectedSubagentDepth(pub u32);

/// Isolation that was actually resolved for execution.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ResolvedSubagentIsolation {
    SharedWorkspace,
    Worktree { path: String },
}

/// Resolved child context, distinct from user-requested cwd and isolation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ResolvedSubagentContext {
    pub effective_cwd: String,
    pub policy_roots: Vec<String>,
    pub isolation: ResolvedSubagentIsolation,
}

/// Whether finalized state is exact, presentation-only, or altered by redaction.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FinalizedStateFidelity {
    Exact,
    SummaryOnly,
    Redacted,
}

/// Availability is separate from fidelity; absence is never reconstructed as data.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FinalizedStateAvailability {
    Available,
    Missing,
    Unsupported,
}

/// Content-addressed finalized-state contract for a child owner.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FinalizedSubagentStateReference {
    pub payload_version: u16,
    pub sha256: String,
    pub owner: SubagentId,
    pub fidelity: FinalizedStateFidelity,
    pub availability: FinalizedStateAvailability,
}

/// Terminal accounting retained with a completed attempt.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct SubagentTerminalAccounting {
    pub tool_calls: u32,
    pub turns: u32,
    pub duration_ms: u64,
    pub tokens_used: Option<u64>,
    pub output_tokens_used: Option<u64>,
    pub total_tokens_used: Option<u64>,
    pub output_usage_incomplete: bool,
}

/// Source-backed terminal turn outcomes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SubagentTerminalOutcome {
    Completed,
    StationarityEnded,
    MaxTokens,
    Cancelled,
    MaxTurnsReached,
    Rewound,
    RemovedFromQueue,
    SessionError,
    DroppedResultChannel,
}

/// Typed cancellation and shutdown requests; ownership scopes are not aliases.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "command", rename_all = "snake_case")]
pub enum SubagentCommandRequest {
    ExplicitChildKill {
        child_id: SubagentId,
    },
    ChildSessionCancel {
        session_id: String,
        descendants: Vec<SubagentId>,
    },
    ParentPromptCancel {
        prompt_id: String,
    },
    ParentSessionStop {
        session_id: String,
    },
    WaiterCancel {
        waiter_id: String,
    },
    RootShutdown,
}
