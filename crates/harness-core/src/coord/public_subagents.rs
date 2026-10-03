//! Native public subagents are records inside the existing coordinator actor.
//! Provider workers, process jobs, policy decisions and journal appends retain
//! their existing owners. Watch receivers are observations, never state owners.
use super::{context::Context, runtime::*, *};
pub(super) use crate::config::SubagentModelPolicy;
use crate::{config::*, subagent::*, tool::ToolCapability};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeSet, VecDeque};
use std::time::Duration;
use tokio::sync::watch;

mod actor;
mod execution;
mod messages;
mod output;
mod persistence;
mod progress;
mod workspace;
pub use workspace::NativeWorkspaceReceipt;

pub use crate::subagent::GetCommandOrSubagentOutputResults as WaitCommandsOrSubagentsResult;
pub use crate::subagent::SendSubagentMessageOutput as SendSubagentMessageResult;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum SpawnSubagentResult {
    Background {
        subagent_id: String,
        description: String,
        auto_backgrounded: bool,
        continue_parent_work: bool,
        notified_on_completion: bool,
    },
    Completed(SpawnSubagentOutput),
}

impl From<SpawnSubagentResult> for ToolResult {
    fn from(result: SpawnSubagentResult) -> Self {
        match result {
            SpawnSubagentResult::Background {
                subagent_id,
                description,
                auto_backgrounded,
                continue_parent_work,
                notified_on_completion,
            } => {
                let result_line = format!("When you need its result, use get_command_or_subagent_output with task_ids=[\"{subagent_id}\"] and a positive timeout_ms.");
                let mut text = if auto_backgrounded {
                    let notification = if notified_on_completion {
                        " \u{2014} you will be notified when it completes"
                    } else {
                        ""
                    };
                    format!("Subagent took longer than the foreground budget and was moved to the background to keep the conversation responsive. It is still running{notification}.\nsubagent_id: {subagent_id}\ndescription: {description}\n\n{result_line}")
                } else {
                    format!("Subagent started in background.\nsubagent_id: {subagent_id}\ndescription: {description}\n\n{result_line}")
                };
                if continue_parent_work {
                    text.push_str(
                        "\n\nDo not only poll the child. Continue unfinished parent work now.",
                    );
                }
                ToolResult::structured(
                    text,
                    serde_json::json!({
                        "subagent_id": subagent_id, "description": description,
                        "auto_backgrounded": auto_backgrounded,
                    }),
                )
            }
            SpawnSubagentResult::Completed(output) => ToolResult::structured(
                persistence::completed_body(&output),
                serde_json::json!(output),
            ),
        }
    }
}

/// Profile material is subordinate to the owning registration event. It permits
/// replay to restore a dynamic definition without rediscovering changed files.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NativeSubagentRegistration {
    pub payload_version: u16,
    pub child_id: String,
    pub spawner: String,
    pub root_agent: String,
    pub parent_tool: String,
    pub parent_request: Option<String>,
    pub subagent_type: String,
    #[serde(default)]
    pub persona: Option<String>,
    #[serde(default)]
    pub role: Option<String>,
    #[serde(default)]
    pub fork_context: bool,
    pub description: String,
    pub prompt: String,
    pub background: bool,
    pub isolation: SubagentIsolationMode,
    pub source: Option<String>,
    pub model: String,
    pub system_prompt: String,
    pub tools: Vec<String>,
    pub permission_rules: crate::perm::PermissionRuleset,
    pub max_iters: Option<usize>,
    pub allowed_types: Option<Vec<String>>,
    pub model_inherited: bool,
    pub messaging_granted: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NativeMessageReceipt {
    pub payload_version: u16,
    pub message_id: String,
    pub sender: String,
    pub sender_attempt: Option<String>,
    pub sender_generation: u64,
    pub recipient: String,
    pub recipient_attempt: Option<String>,
    pub recipient_generation: u64,
    pub delivery: SendSubagentMessageDelivery,
    pub text: String,
    pub status: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NativeSubagentReceipt {
    pub payload_version: u16,
    pub child_id: String,
    pub attempt_id: Option<String>,
    pub generation: u64,
    pub kind: String,
    pub waiter_id: Option<String>,
}

#[derive(Debug, Clone)]
pub(super) struct NativeSnapshot {
    result: GetCommandOrSubagentOutputResult,
    completed: Option<SpawnSubagentOutput>,
    error: Option<String>,
    terminal: bool,
    demoted: bool,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum NativePhase {
    Queued,
    Preparing,
    Running,
    Finalizing,
    Terminal,
}

pub(super) struct NativeSubagent {
    registration: NativeSubagentRegistration,
    resolved: Option<ResolvedSubagentDefinition>,
    phase: NativePhase,
    request: Option<String>,
    cancellation: CancellationToken,
    foreground_attached: bool,
    explicitly_killed: bool,
    terminal_published: bool,
    updates: watch::Sender<NativeSnapshot>,
    started_ms: u64,
    started: String,
    ended: Option<String>,
    cwd: PathBuf,
    worktree: Option<PathBuf>,
    snapshot_ref: Option<String>,
    preparation: Option<String>,
    source_state: Option<Box<RawFinalizedState>>,
    source_reference: Option<FinalizedAgentStateReferenceV1>,
    fork_context: Option<Context>,
    fork_read_state: Option<BTreeMap<PathBuf, String>>,
    messages: VecDeque<NativeMessageReceipt>,
    outbound: usize,
    sender_generation: u64,
    waiters: BTreeSet<String>,
    completion_age: u64,
    consumed: bool,
    wait_interrupt: watch::Sender<u64>,
    parked: VecDeque<messages::PendingAdmission>,
    displaced: Option<NativeDisplaced>,
    terminal_event: Option<EventEnvelopeV1>,
    pending_terminal: Option<NativeSnapshot>,
    buffered_for: Option<String>,
    routed_to_root: bool,
    creation_checkpoint: Option<WorkspaceCreationCheckpoint>,
    ownership: Option<workspace::WorkspaceOwnership>,
    cleanup_pending: bool,
    progress: Option<progress::Publisher>,
}

impl NativeSubagent {
    pub(super) fn preload_skills(&self) -> Vec<String> {
        self.resolved
            .as_ref()
            .map(|resolved| resolved.definition.skills.clone())
            .or_else(|| {
                self.source_state
                    .as_ref()
                    .map(|state| state.skill_preload_names.clone())
            })
            .unwrap_or_default()
    }

    pub(super) fn is_terminal(&self) -> bool {
        self.phase == NativePhase::Terminal
    }
}

struct NativeDisplaced {
    registration: NativeSubagentRegistration,
    snapshot: NativeSnapshot,
    request: Option<String>,
    started_ms: u64,
    started: String,
    ended: Option<String>,
}

pub(super) struct NativeSubscription {
    id: String,
    cancellation: CancellationToken,
    updates: watch::Receiver<NativeSnapshot>,
    continue_parent_work: bool,
    notified_on_completion: bool,
}

#[derive(Debug)]
pub(super) struct PreparedSubagent {
    pub(super) cwd: PathBuf,
    pub(super) worktree: Option<PathBuf>,
    pub(super) snapshot_ref: Option<String>,
    ownership: Option<workspace::WorkspaceOwnership>,
    pub(super) failure: Option<CoordinatorError>,
}

#[derive(Debug, Clone)]
pub(super) struct WorkspaceCreationCheckpoint {
    pub(super) entered: mpsc::UnboundedSender<PathBuf>,
    pub(super) proceed: Arc<tokio::sync::Notify>,
}

impl NativeSubagentRegistration {
    fn profile(&self) -> Arc<AgentProfile> {
        Arc::new(AgentProfile {
            name: self.subagent_type.clone(),
            model_ref: self.model.clone(),
            model_ref_explicit: true,
            system_prompt: self.system_prompt.clone(),
            max_iters: self.max_iters,
            toolset: self.tools.clone(),
            permission_ruleset: self.permission_rules.clone(),
            ..AgentProfile::fallback(&self.subagent_type)
        })
    }
}

fn optional(value: Option<&str>) -> Option<String> {
    value
        .map(str::trim)
        .filter(|value| {
            !value.is_empty()
                && !["null", "none", "undefined"]
                    .iter()
                    .any(|sentinel| value.eq_ignore_ascii_case(sentinel))
        })
        .map(str::to_owned)
}

fn native_id(injected: Option<&str>) -> Result<String, CoordinatorError> {
    let id = injected
        .map(uuid::Uuid::parse_str)
        .transpose()
        .map_err(|_| native_invalid("Injected task_id must be a UUIDv7.".into()))?
        .unwrap_or_else(uuid::Uuid::now_v7);
    if id.get_version_num() != 7 {
        return Err(native_invalid("Injected task_id must be a UUIDv7.".into()));
    }
    Ok(id.to_string())
}

fn milliseconds(name: &str, default: u64) -> u64 {
    std::env::var(name)
        .ok()
        .and_then(|value| value.parse().ok())
        .unwrap_or(default)
}

fn native_invalid(message: String) -> CoordinatorError {
    CoordinatorError::Native {
        code: "invalid_arguments".into(),
        message,
    }
}

fn native_timestamp(clock: &(dyn Clock + Send + Sync)) -> String {
    clock.system_time_rfc3339_millis().unwrap_or_default()
}

impl CoordinatorHandle {
    pub async fn spawn_subagent(
        &self,
        actor: EventActor,
        tool_call_id: String,
        input: SpawnSubagentInput,
    ) -> Result<SpawnSubagentResult, CoordinatorError> {
        self.spawn_native(actor, tool_call_id, input, false, None)
            .await
    }

    /// Host-only fork. Extra JSON fields never select this path.
    pub async fn spawn_subagent_from_current(
        &self,
        actor: EventActor,
        tool_call_id: String,
        input: SpawnSubagentInput,
    ) -> Result<SpawnSubagentResult, CoordinatorError> {
        self.spawn_native(actor, tool_call_id, input, true, None)
            .await
    }

    #[cfg(test)]
    pub(super) async fn spawn_subagent_at_creation_checkpoint(
        &self,
        actor: EventActor,
        tool_call_id: String,
        input: SpawnSubagentInput,
        checkpoint: WorkspaceCreationCheckpoint,
    ) -> Result<SpawnSubagentResult, CoordinatorError> {
        self.spawn_native(actor, tool_call_id, input, false, Some(checkpoint))
            .await
    }

    async fn spawn_native(
        &self,
        actor: EventActor,
        tool: String,
        input: SpawnSubagentInput,
        fork: bool,
        checkpoint: Option<WorkspaceCreationCheckpoint>,
    ) -> Result<SpawnSubagentResult, CoordinatorError> {
        let background = input.background;
        let description = input.description.clone();
        let waiter = tool.clone();
        let mut subscription = self
            .call(move |s| s.admit_native_subagent(actor, tool, input, fork, checkpoint))
            .await?;
        if background {
            return Ok(SpawnSubagentResult::Background {
                subagent_id: subscription.id,
                description,
                auto_backgrounded: false,
                continue_parent_work: subscription.continue_parent_work,
                notified_on_completion: subscription.notified_on_completion,
            });
        }
        let id = subscription.id.clone();
        let continue_parent_work = subscription.continue_parent_work;
        let notified_on_completion = subscription.notified_on_completion;
        let cancellation = subscription.cancellation.clone();
        let budget = Duration::from_millis(milliseconds("GROK_SUBAGENT_AWAIT_BUDGET_MS", 600_000));
        let terminal = async {
            loop {
                let snapshot = subscription.updates.borrow_and_update().clone();
                if snapshot.terminal {
                    return match snapshot.completed {
                        Some(output) => Ok(SpawnSubagentResult::Completed(output)),
                        None => Err(native_invalid(
                            snapshot
                                .error
                                .unwrap_or_else(|| "Unknown subagent error".into()),
                        )),
                    };
                }
                if snapshot.demoted {
                    return Ok(SpawnSubagentResult::Background {
                        subagent_id: id.clone(),
                        description: description.clone(),
                        auto_backgrounded: true,
                        continue_parent_work,
                        notified_on_completion,
                    });
                }
                subscription
                    .updates
                    .changed()
                    .await
                    .map_err(|_| CoordinatorError::Closed)?;
            }
        };
        let result = tokio::select! {
            biased;
            () = cancellation.cancelled() => {
                let cancelled_id = id.clone();
                self.call(move |s| s.cancel_native_foreground(&cancelled_id)).await?;
                Err(CoordinatorError::Cancelled("Subagent was cancelled".into()))
            }
            result = tokio::time::timeout(budget, terminal) => match result {
                Ok(result) => result,
                Err(_) => {
                    let demoted_id = id.clone();
                    self.call(move |s| s.demote_native_subagent(&demoted_id)).await?;
                    Ok(SpawnSubagentResult::Background {
                        subagent_id: id.clone(), description, auto_backgrounded: true,
                        continue_parent_work,
                        notified_on_completion,
                    })
                }
            }
        };
        self.call(move |s| s.detach_native_spawn_waiter(&id, &waiter))
            .await?;
        result
    }
}
