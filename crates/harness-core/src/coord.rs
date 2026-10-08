//! A serial authority for session changes. Workers only submit results and intents.
use crate::{
    agent::{AgentModelSettings, AgentProfile},
    clock::Clock,
    config::ResolvedModelTarget,
    event::*,
    ids::{RunId, RunName},
    perm::{PermissionDecision, PermissionPolicy},
    redact::Redactor,
    store::{EventStore, EventStoreError},
    tool::{ToolError, ToolRegistry, ToolResult},
};
use serde_json::Value;
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
    sync::Arc,
};
use tokio::sync::{mpsc, oneshot};
use tokio_util::sync::{CancellationToken, DropGuard};
mod child_journals;
mod children;
mod commands;
mod compaction;
pub use public_subagents::{
    NativeMessageReceipt, NativeSubagentReceipt, NativeSubagentRegistration,
    NativeWorkspaceReceipt, SendSubagentMessageResult, SpawnSubagentResult,
    WaitCommandsOrSubagentsResult,
};
#[cfg(test)]
mod children_tests;
mod edit_paths;
mod edits;
mod eval;
#[cfg(test)]
mod failure_tests;
#[cfg(test)]
mod fallback_tests;
pub use compaction::ManualCompactionOutcome;
#[cfg(test)]
mod compaction_tests;
mod completion;
mod context;
mod grants;
mod handle;
pub use edits::EditReceipt;
#[cfg(test)]
mod grant_tests;
#[cfg(test)]
mod guidance_tests;
mod history;
#[cfg(test)]
mod history_tests;
mod hooks;
#[cfg(all(test, unix))]
mod hooks_tests;
mod instructions;
mod lifecycle;
#[cfg(test)]
mod media_tests;
mod metadata;
mod nested_tools;
mod output;
#[cfg(test)]
mod output_tests;
mod permissions;
mod prompt;
mod public_subagent_hooks;
mod public_subagents;
#[cfg(test)]
mod question_tests;
mod questions;
mod reminders;
#[cfg(test)]
mod replay_order_tests;
mod resume;
mod runtime;
mod steering;
mod streaming;
mod subagents;
pub use steering::SteerOutcome;
#[cfg(test)]
mod subagents_tests;
#[cfg(test)]
mod tests;
mod tools;
mod turn;
mod workspace;
pub use commands::{CommandExit, CommandSnapshot, CommandSubscription, PreparedCommand};
#[cfg(test)]
mod workspace_tests;
use runtime::Runtime;
pub use workspace::{
    plan_saved_session_rewind, FileSnapshotEntry, SavedSessionRewindReport, WorkspaceRevertSummary,
    WorkspaceSnapshotSummary,
};

type Reply<T> = oneshot::Sender<Result<T, CoordinatorError>>;
type Command = Box<dyn FnOnce(&mut Runtime) + Send>;

#[derive(Clone)]
pub struct CoordinatorHandle {
    tx: mpsc::Sender<Command>,
    // Worker handles do not keep the public owner's lifetime alive.
    _owner: Option<Arc<DropGuard>>,
}
#[derive(Clone)]
pub struct CoordinatorConfig {
    pub session_dir: PathBuf,
    pub run_id_override: Option<String>,
    pub deterministic_store: bool,
    /// Interactive callers can receive detached eval completions after the foreground receipt.
    pub interactive: bool,
    pub command_buffer: usize,
    pub permission_policy: PermissionPolicy,
    pub yolo_on_start: bool,
    pub tool_concurrency: usize,
    pub provider_model_concurrency: usize,
    pub tool_registry: Arc<ToolRegistry>,
    pub formatter: Arc<crate::config::FormatterConfig>,
    pub hook_runtime_config: crate::config::HookRuntimeConfig,
    pub provider: Arc<dyn harness_providers::Provider>,
    pub agent_profiles: BTreeMap<String, AgentProfile>,
    pub subagents: crate::config::SubagentRuntimeConfig,
    pub subagent_definitions: Option<crate::config::SubagentDefinitionSnapshot>,
    pub subagent_discovery: Option<crate::config::SubagentDiscoveryContext>,
    pub skills: crate::config::SkillsConfig,
    pub skill_catalog_discovery: Option<Arc<dyn crate::config::SkillCatalogDiscovery>>,
    pub subagent_model_catalog: Option<crate::config::SubagentModelCatalog>,
    pub agent_prompt_sources: BTreeMap<String, Arc<crate::system_prompt::PromptSource>>,
    pub agent_model_targets: BTreeMap<String, ResolvedModelTarget>,
    pub agent_model_fallbacks: BTreeMap<String, Vec<ResolvedModelTarget>>,
    pub model_catalog: Arc<[crate::config::ResolvedModelCatalogEntry]>,
    pub provider_retry: crate::config::ProviderRetryRuntimeConfig,
    pub compaction: crate::config::CompactionSettings,
    /// Runtime guidance; `new` starts with every guard off so embedders opt in.
    pub behavior: crate::config::BehaviorSettings,
    /// Instruction files already in the startup prompt, excluded from directory instructions.
    pub instruction_paths: Vec<PathBuf>,
    pub config_digest: String,
    pub secret_values: Vec<String>,
    pub secret_registry: Arc<crate::redact::SecretRegistry>,
    pub harness_version: String,
    pub session_mode_source: Option<crate::proj::SessionModeSource>,
}
impl CoordinatorConfig {
    pub fn new(session_dir: impl Into<PathBuf>) -> Self {
        Self {
            session_dir: session_dir.into(),
            run_id_override: None,
            deterministic_store: false,
            interactive: false,
            command_buffer: 64,
            permission_policy: PermissionPolicy::default(),
            yolo_on_start: false,
            tool_concurrency: 4,
            provider_model_concurrency: 2,
            tool_registry: Arc::new(ToolRegistry::new()),
            formatter: Arc::new(Default::default()),
            hook_runtime_config: Default::default(),
            provider: Arc::new(harness_providers::mock::MockProvider::default()),
            agent_profiles: BTreeMap::from([("default".into(), AgentProfile::fallback("default"))]),
            subagents: Default::default(),
            subagent_definitions: None,
            subagent_discovery: None,
            skills: Default::default(),
            skill_catalog_discovery: None,
            subagent_model_catalog: None,
            agent_prompt_sources: BTreeMap::new(),
            agent_model_targets: BTreeMap::new(),
            agent_model_fallbacks: BTreeMap::new(),
            model_catalog: Arc::from([]),
            provider_retry: Default::default(),
            compaction: Default::default(),
            behavior: crate::config::BehaviorSettings::off(),
            instruction_paths: Vec::new(),
            config_digest: String::new(),
            secret_values: Vec::new(),
            secret_registry: Arc::new(crate::redact::SecretRegistry::default()),
            harness_version: env!("CARGO_PKG_VERSION").into(),
            session_mode_source: None,
        }
    }
}
#[derive(Debug, Clone)]
pub struct RunInfo {
    pub run_id: RunId,
    pub run_name: RunName,
    pub workspace_root: PathBuf,
    pub run_dir: PathBuf,
    pub artifacts_dir: PathBuf,
    pub events_path: PathBuf,
}
#[derive(Debug, Clone)]
pub struct AgentRuntimeInfo {
    pub agent_id: String,
    pub profile_name: String,
    pub model_ref: String,
    pub model_ref_explicit: bool,
    pub toolset: Vec<String>,
    pub parent_agent_id: Option<String>,
}
/// A guarded request and its settled billing, retained even when the worker retries.
#[derive(Debug, thiserror::Error)]
#[error("{reason}")]
pub struct StreamGuardFailure {
    pub reason: String,
    pub request_id: String,
    pub usage: Option<harness_providers::CompletionUsage>,
    pub usage_complete: bool,
}

#[derive(Debug, thiserror::Error)]
pub enum CoordinatorError {
    #[error("{code}: {message}")]
    Native { code: String, message: String },
    #[error("coordinator is closed")]
    Closed,
    #[error("run has not started")]
    RunNotStarted,
    #[error("run has already started")]
    RunAlreadyStarted,
    #[error("run is stopping")]
    Stopping,
    #[error("unknown agent: {0}")]
    UnknownAgent(String),
    #[error("unknown profile: {0}")]
    UnknownProfile(String),
    #[error("unknown tool: {0}")]
    UnknownTool(String),
    #[error("permission denied: {0}")]
    PermissionDenied(String),
    #[error("unknown permission: {0}")]
    UnknownPermission(String),
    #[error("unknown task: {0}")]
    UnknownTask(String),
    #[error("cancelled: {0}")]
    Cancelled(String),
    #[error("compaction cancelled for {agent_id}")]
    CompactionCancelled { agent_id: String },
    /// The stream guard stopped a degenerate response; the worker may retry with a correction.
    #[error("response stopped by the stream guard: {0}")]
    StreamGuard(StreamGuardFailure),
    #[error("{message}")]
    Provider {
        message: String,
        category: Option<harness_providers::ProviderErrorCategory>,
        remediation: Option<String>,
        retry_after_ms: Option<u64>,
    },
    #[error("{0}")]
    Invalid(String),
    #[error(transparent)]
    Store(#[from] EventStoreError),
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error(transparent)]
    Json(#[from] serde_json::Error),
}

impl From<ToolError> for CoordinatorError {
    fn from(error: ToolError) -> Self {
        match error {
            ToolError::Custom { code, message } => Self::Native { code, message },
            ToolError::InvalidArguments(message) => Self::Native {
                code: "invalid_arguments".into(),
                message,
            },
            ToolError::Cancelled => Self::Cancelled("tool cancelled".into()),
            ToolError::Io(error) => Self::Io(error),
            error => Self::Invalid(error.to_string()),
        }
    }
}

impl From<CoordinatorError> for ToolError {
    fn from(error: CoordinatorError) -> Self {
        match error {
            CoordinatorError::Native { code, message } => Self::Custom { code, message },
            CoordinatorError::PermissionDenied(message) => Self::Custom {
                code: "permission_denied".into(),
                message,
            },
            CoordinatorError::Cancelled(_) => Self::Cancelled,
            CoordinatorError::Io(error) => Self::Io(error),
            error => Self::Execution(error.to_string()),
        }
    }
}

pub fn spawn_coordinator(
    config: CoordinatorConfig,
    clock: Arc<dyn Clock + Send + Sync>,
    redactor: Arc<dyn Redactor + Send + Sync>,
) -> CoordinatorHandle {
    let shutdown = CancellationToken::new();
    let (tx, rx) = mpsc::channel(config.command_buffer.max(1));
    let runtime = Runtime::new(config, clock, redactor, tx.downgrade());
    let executor = tokio::runtime::Handle::current();
    let owner = Arc::new(shutdown.clone().drop_guard());
    let run = runtime.run(rx, shutdown);
    if executor.runtime_flavor() == tokio::runtime::RuntimeFlavor::CurrentThread {
        // ponytail: single-thread callers commit inline; interactive runtimes use the I/O thread below.
        tokio::spawn(run);
    } else {
        // Journal syncs stay off the UI executor; the actor sleeps when idle.
        tokio::task::spawn_blocking(move || executor.block_on(run));
    }
    CoordinatorHandle {
        tx,
        _owner: Some(owner),
    }
}
