// allow: SIZE_OK — coordinator state machine (turn lifecycle + scheduling)
use super::*;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AgentTurnTaskOutcome {
    Succeeded {
        output: String,
        messages: Vec<ConversationMessage>,
    },
    Failed {
        reason: String,
        memory: Option<AgentTurnFailureMemory>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentTurnFailureMemory {
    pub status: ProviderConversationTurnStatus,
    pub failure_stage: String,
    pub failure_reason: String,
    pub partial_assistant_output: String,
    pub provider_request_id: Option<String>,
}

impl AgentTurnFailureMemory {
    pub(in crate::coord) fn new(
        status: ProviderConversationTurnStatus,
        failure_stage: impl Into<String>,
        failure_reason: impl Into<String>,
        partial_assistant_output: impl Into<String>,
        provider_request_id: Option<String>,
    ) -> Self {
        Self {
            status,
            failure_stage: failure_stage.into(),
            failure_reason: failure_reason.into(),
            partial_assistant_output: partial_assistant_output.into(),
            provider_request_id,
        }
    }

    pub(in crate::coord) fn failed(
        failure_stage: impl Into<String>,
        failure_reason: impl Into<String>,
        partial_assistant_output: impl Into<String>,
        provider_request_id: Option<String>,
    ) -> Self {
        Self::new(
            ProviderConversationTurnStatus::Failed,
            failure_stage,
            failure_reason,
            partial_assistant_output,
            provider_request_id,
        )
    }

    pub(in crate::coord) fn aborted(
        failure_stage: impl Into<String>,
        failure_reason: impl Into<String>,
        partial_assistant_output: impl Into<String>,
        provider_request_id: Option<String>,
    ) -> Self {
        Self::new(
            ProviderConversationTurnStatus::Aborted,
            failure_stage,
            failure_reason,
            partial_assistant_output,
            provider_request_id,
        )
    }
}

impl From<AgentTurnFailure> for AgentTurnFailureMemory {
    fn from(failure: AgentTurnFailure) -> Self {
        let failure = failure.into_details();
        Self::new(
            failure.status,
            failure.failure_stage,
            failure.reason,
            failure.partial_assistant_output,
            failure.provider_request_id,
        )
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::coord) struct CompactionGenerationToken(u64);

#[derive(Debug, Clone, PartialEq, Eq)]
pub(in crate::coord) struct CompactionGenerationBase {
    run_id: crate::ids::RunId,
    durable_agent_tail_seq: Option<u64>,
}

impl CompactionGenerationBase {
    pub(in crate::coord) fn capture(
        run_state: &RunState,
        durable_agent_tail_seq: Option<u64>,
    ) -> Self {
        Self {
            run_id: run_state.info.run_id.clone(),
            durable_agent_tail_seq,
        }
    }

    pub(in crate::coord) fn is_current(
        &self,
        run_state: &RunState,
        durable_agent_tail_seq: Option<u64>,
    ) -> bool {
        self.run_id == run_state.info.run_id
            && self.durable_agent_tail_seq == durable_agent_tail_seq
    }
}

pub(in crate::coord) enum PendingCompactionResponse {
    Agent(
        oneshot::Sender<
            Result<
                (
                    ProviderContext,
                    Option<crate::session::CanonicalProviderView>,
                    bool,
                ),
                CoordinatorError,
            >,
        >,
    ),
    Manual(oneshot::Sender<Result<ManualCompactionOutcome, CoordinatorError>>),
    Internal {
        trigger_reason: String,
    },
}

impl PendingCompactionResponse {
    pub(in crate::coord) fn finish(
        self,
        result: Result<CompactAgentContextResult, CoordinatorError>,
    ) {
        match self {
            Self::Agent(respond_to) => warn_oneshot_send_failure(
                respond_to.send(result.map(|outcome| match outcome {
                    CompactAgentContextResult::Compacted { context, view, .. } => {
                        (context, Some(*view), true)
                    }
                    CompactAgentContextResult::NoOp { context } => (context, None, false),
                })),
                "compact_agent_context",
            ),
            Self::Manual(respond_to) => warn_oneshot_send_failure(
                respond_to.send(result.map(CompactAgentContextResult::into_manual_outcome)),
                "manual_compact_agent_context",
            ),
            Self::Internal { trigger_reason } => {
                if let Err(error) = result {
                    tracing::warn!(
                        trigger_reason,
                        error = %error,
                        "internal compaction generation did not complete"
                    );
                }
            }
        }
    }
}

pub(in crate::coord) struct PendingCompactionState {
    pub(in crate::coord) agent_id: String,
    pub(in crate::coord) task_id: Option<String>,
    pub(in crate::coord) generation: CompactionGenerationToken,
    pub(in crate::coord) base: CompactionGenerationBase,
    pub(in crate::coord) cancellation_token: CancellationToken,
    pub(in crate::coord) trigger: ProviderCompactionTrigger,
    pub(in crate::coord) response: PendingCompactionResponse,
}

impl PendingCompactionState {
    fn cancel(self, reason: &str) {
        self.cancellation_token.cancel();
        self.response
            .finish(Err(CoordinatorError::CompactionCancelled {
                agent_id: self.agent_id,
                reason: reason.to_string(),
            }));
    }
}

pub(in crate::coord) struct RunState {
    pub(in crate::coord) info: RunInfo,
    pub(in crate::coord) event_store: Arc<JsonlFileEventStore>,
    pub(in crate::coord) canonical_event_history: Vec<EventEnvelopeV1>,
    pub(in crate::coord) next_event_seq: u64,
    pub(in crate::coord) next_live_event_id: u64,
    pub(in crate::coord) next_agent_id: u64,
    pub(in crate::coord) next_tool_call_id: u64,
    pub(in crate::coord) next_task_id: u64,
    pub(in crate::coord) next_provider_request_id: u64,
    pub(in crate::coord) next_permission_id: u64,
    pub(in crate::coord) next_compaction_generation: u64,
    pub(in crate::coord) compaction_boundary_watermark: u64,
    pub(in crate::coord) agents: BTreeMap<String, AgentProfile>,
    pub(in crate::coord) provider_context_by_agent: BTreeMap<String, ProviderContext>,
    pub(in crate::coord) canonical_provider_view_by_agent:
        BTreeMap<String, crate::session::CanonicalProviderView>,
    pub(in crate::coord) provider_context_cache_key_by_agent:
        BTreeMap<String, ProviderContextCacheKey>,
    pub(in crate::coord) live_incomplete_provider_turns_by_agent:
        BTreeMap<String, Vec<ProviderConversationTurn>>,
    pub(in crate::coord) explicit_runtime_selection_request_ids: BTreeSet<String>,
    pub(in crate::coord) tasks: BTreeMap<String, TaskState>,
    pub(in crate::coord) task_hook_state: BTreeMap<String, TaskHookState>,
    pub(in crate::coord) agent_hook_state: BTreeMap<String, Vec<HookExecutionMetadata>>,
    pub(in crate::coord) subagent_parent_by_id: BTreeMap<String, String>,
    pub(in crate::coord) child_session_mirrors: BTreeMap<String, ChildSessionMirror>,
    pub(in crate::coord) child_request_session_by_id: BTreeMap<String, String>,
    pub(in crate::coord) background_notification_child_requests: BTreeSet<String>,
    pub(in crate::coord) pending_agent_wakeups: BTreeMap<String, Vec<PendingAgentWakeup>>,
    pub(in crate::coord) pending_permissions: BTreeMap<String, PendingPermissionState>,
    pub(in crate::coord) tool_call_request_event_ids: BTreeMap<String, String>,
    pub(in crate::coord) active_permission_grants: PermissionGrantSet,
    pub(in crate::coord) cancelled_running_tasks: BTreeSet<String>,
    pub(in crate::coord) queued_agent_turns: BTreeMap<String, QueuedAgentTurn>,
    pub(in crate::coord) running_agent_turns: BTreeMap<String, RunningAgentTurn>,
    pub(in crate::coord) pending_compactions: BTreeMap<String, PendingCompactionState>,
    pub(in crate::coord) failed_terminal_compaction_attempts: BTreeSet<(String, String)>,
    pub(in crate::coord) overflow_retry_compacted_context_by_attempt:
        BTreeMap<(String, String), ProviderContext>,
    pub(in crate::coord) scheduler: Scheduler,
    pub(in crate::coord) recorded_runtime_context: Option<RecordedRuntimeContext>,
    pub(in crate::coord) allow_initial_runtime_context_recording: bool,
    pub(in crate::coord) shutdown_token: CancellationToken,
    pub(in crate::coord) tool_state: ToolRunState,
    // (tool_id, permission_request_digest) for consecutive-identical doom_loop streak.
    pub(in crate::coord) last_identical_tool_key: Option<(String, String)>,
    pub(in crate::coord) identical_tool_call_streak: u32,
    pub(in crate::coord) doom_loop_always_granted: bool,
    /// Workspace-durable agent vs external edit attribution journal.
    pub(in crate::coord) edit_attribution: crate::edit_attribution::EditAttributionJournal,
    /// Session-local multi-agent team registry + in-memory mailbox (not Team Mode product).
    pub(in crate::coord) team_registry: crate::team_registry::TeamRegistry,
    /// Session-local cron schedule definitions only (`executor_available() == false`).
    pub(in crate::coord) cron_schedules: crate::cron_schedule::CronScheduleRegistry,
    /// Coordinator-owned plugin runtime contract (lifecycle + execution surface + event recording).
    pub(in crate::coord) plugin_lifecycle: crate::integrations::PluginRuntimeContract,
}

impl RunState {
    pub(in crate::coord) fn canonical_provider_context(
        &self,
        key: &ProviderContextCacheKey,
    ) -> Option<&ProviderContext> {
        (self
            .provider_context_cache_key_by_agent
            .get(&key.owner.agent_id)
            == Some(key))
        .then(|| self.provider_context_by_agent.get(&key.owner.agent_id))
        .flatten()
    }

    pub(in crate::coord) fn install_canonical_provider_context(
        &mut self,
        key: ProviderContextCacheKey,
        context: ProviderContext,
    ) {
        let agent_id = key.owner.agent_id.clone();
        self.provider_context_by_agent
            .insert(agent_id.clone(), context);
        self.provider_context_cache_key_by_agent
            .insert(agent_id, key);
    }

    pub(in crate::coord) fn install_canonical_provider_recovery(
        &mut self,
        view: crate::session::CanonicalProviderView,
        context: ProviderContext,
    ) {
        let agent_id = view.owner.agent_id.clone();
        self.install_canonical_provider_context(ProviderContextCacheKey::from(&view), context);
        self.canonical_provider_view_by_agent
            .insert(agent_id.clone(), view);
        if let Some(turns) = self
            .live_incomplete_provider_turns_by_agent
            .get_mut(&agent_id)
        {
            turns.retain(|turn| !turn.status.is_completed());
            if turns.is_empty() {
                self.live_incomplete_provider_turns_by_agent
                    .remove(&agent_id);
            }
        }
    }

    pub(in crate::coord) fn invalidate_provider_context(&mut self, agent_id: &str) {
        self.provider_context_by_agent.remove(agent_id);
        self.canonical_provider_view_by_agent.remove(agent_id);
        self.provider_context_cache_key_by_agent.remove(agent_id);
    }

    pub(in crate::coord) fn cached_canonical_provider_view(
        &self,
        agent_id: &str,
    ) -> Option<&crate::session::CanonicalProviderView> {
        self.canonical_provider_view_by_agent.get(agent_id)
    }

    pub(in crate::coord) fn record_completed_provider_turn(
        &mut self,
        agent_id: &str,
        turn: ProviderConversationTurn,
    ) {
        let turns = self
            .live_incomplete_provider_turns_by_agent
            .entry(agent_id.to_string())
            .or_default();
        turns.retain(|existing| existing.request_id != turn.request_id);
        turns.push(turn.clone());
        let context = self
            .provider_context_by_agent
            .entry(agent_id.to_string())
            .or_default();
        context
            .preserved_turns
            .retain(|existing| existing.request_id != turn.request_id);
        context.preserved_turns.push(turn);
    }

    pub(in crate::coord) fn refresh_canonical_provider_cache(
        &mut self,
    ) -> Result<(), CoordinatorError> {
        let run_id = self.info.run_id.to_string();
        let recovery = super::provider_context::recover_canonical_provider_context_from_events(
            &self.canonical_event_history,
            Vec::new(),
            &run_id,
            &BTreeMap::new(),
        )?;
        for recovered in recovery.by_agent.into_values() {
            self.install_canonical_provider_recovery(recovered.view, recovered.context);
        }
        Ok(())
    }

    pub(in crate::coord) fn next_compaction_generation(&mut self) -> CompactionGenerationToken {
        let generation = CompactionGenerationToken(self.next_compaction_generation);
        self.next_compaction_generation += 1;
        generation
    }

    pub(in crate::coord) fn advance_compaction_boundary(&mut self) {
        self.compaction_boundary_watermark += 1;
    }

    pub(in crate::coord) fn cancel_pending_compaction_for_task(
        &mut self,
        task_id: &str,
        reason: &str,
    ) {
        let agent_id = self
            .pending_compactions
            .iter()
            .find_map(|(agent_id, pending)| {
                (pending.task_id.as_deref() == Some(task_id)).then(|| agent_id.clone())
            });
        if let Some(agent_id) = agent_id {
            if let Some(pending) = self.pending_compactions.remove(&agent_id) {
                pending.cancel(reason);
            }
        }
    }

    pub(in crate::coord) fn cancel_all_pending_compactions(&mut self, reason: &str) {
        for (_, pending) in std::mem::take(&mut self.pending_compactions) {
            pending.cancel(reason);
        }
    }

    pub(in crate::coord) fn agent_has_active_or_queued_turn(&self, agent_id: &str) -> bool {
        self.running_agent_turns
            .values()
            .any(|running| running.agent_id == agent_id)
            || self
                .queued_agent_turns
                .values()
                .any(|queued| queued.agent_id == agent_id)
    }

    pub(in crate::coord) fn agent_has_running_turn(&self, agent_id: &str) -> bool {
        self.running_agent_turns
            .values()
            .any(|running| running.agent_id == agent_id)
    }

    pub(in crate::coord) fn next_agent_blocked_turn_id(&self, agent_id: &str) -> Option<String> {
        self.queued_agent_turns
            .values()
            .filter(|queued| queued.agent_id == agent_id && !queued.scheduler_queued)
            .min_by(|left, right| left.task_id.cmp(&right.task_id))
            .map(|queued| queued.task_id.clone())
    }

    pub(in crate::coord) fn queue_agent_turn(&mut self, queued: QueuedAgentTurn) {
        self.queued_agent_turns
            .insert(queued.task_id.clone(), queued);
    }

    pub(in crate::coord) fn mark_queued_agent_turn_scheduler_queued(&mut self, task_id: &str) {
        if let Some(queued) = self.queued_agent_turns.get_mut(task_id) {
            queued.scheduler_queued = true;
        }
    }

    pub(in crate::coord) fn begin_running_agent_turn<C>(
        &mut self,
        clock: &C,
        task: &QueuedAgentTurn,
        hook_executions: Vec<HookExecutionMetadata>,
        cancellation_token: CancellationToken,
    ) where
        C: Clock + ?Sized,
    {
        self.running_agent_turns.insert(
            task.task_id.clone(),
            RunningAgentTurn {
                agent_id: task.agent_id.clone(),
                request_id: task.request_id.clone(),
                request_prompt: task.request.prompt.clone(),
                attachments: task.request.attachments.clone(),
                profile_name: task.profile.name.clone(),
                model_ref: task.request.model_ref.clone(),
                model_settings: task.request.model_settings.clone(),
                profile: Some(task.profile.name.clone()),
                queue_key: task.queue_key.clone(),
                cancellation_token,
                started_mono_ms: clock.mono_ms(),
                hook_executions,
                latest_provider_usage: None,
                latest_provider_request_id: None,
                latest_assistant_output: None,
                latest_provider_id: None,
                latest_model_id: None,
                child_task: task.child_task.clone(),
            },
        );
    }

    pub(in crate::coord) fn pending_permission(
        &self,
        permission_id: &str,
    ) -> Option<&PendingPermissionState> {
        self.pending_permissions.get(permission_id)
    }

    pub(in crate::coord) fn insert_pending_permission(
        &mut self,
        permission_id: String,
        pending: PendingPermissionState,
    ) {
        self.pending_permissions.insert(permission_id, pending);
    }

    pub(in crate::coord) fn take_pending_permission(
        &mut self,
        permission_id: &str,
    ) -> Option<PendingPermissionState> {
        self.pending_permissions.remove(permission_id)
    }

    pub(in crate::coord) fn record_permission_grant(&mut self, grant: PermissionGrant) {
        self.active_permission_grants.record(grant);
    }

    pub(in crate::coord) fn permission_grant_authorizes(
        &self,
        grant_request: &PermissionGrantRequest,
    ) -> bool {
        self.active_permission_grants.authorizes(grant_request)
    }

    pub(in crate::coord) fn note_identical_tool_call(
        &mut self,
        tool_id: &str,
        args_digest: &str,
    ) -> u32 {
        match &self.last_identical_tool_key {
            Some((prev_tool, prev_digest))
                if prev_tool == tool_id && prev_digest == args_digest =>
            {
                self.identical_tool_call_streak = self.identical_tool_call_streak.saturating_add(1);
            }
            _ => {
                self.last_identical_tool_key = Some((tool_id.to_string(), args_digest.to_string()));
                self.identical_tool_call_streak = 1;
            }
        }
        self.identical_tool_call_streak
    }

    pub(in crate::coord) fn reset_identical_tool_call_streak(&mut self) {
        self.last_identical_tool_key = None;
        self.identical_tool_call_streak = 0;
    }

    pub(in crate::coord) fn record_overflow_retry_compacted_context(
        &mut self,
        task_id: &str,
        request_id: &str,
        context: ProviderContext,
    ) {
        self.overflow_retry_compacted_context_by_attempt
            .insert((task_id.to_string(), request_id.to_string()), context);
    }

    pub(in crate::coord) fn failed_terminal_compaction_attempt_should_run(
        &mut self,
        request: &FailedTerminalCompactionRequest,
    ) -> bool {
        let key = request.attempt_key();
        if !self.failed_terminal_compaction_attempts.insert(key.clone()) {
            return false;
        }

        if let Some(overflow_context) = self.overflow_retry_compacted_context_by_attempt.get(&key) {
            let current_context = self
                .provider_context_by_agent
                .get(&request.agent_id)
                .cloned()
                .unwrap_or_default();
            if &current_context == overflow_context {
                return false;
            }
        }

        true
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(in crate::coord) struct ProviderContextCacheKey {
    pub(in crate::coord) owner: crate::session::ProviderViewOwner,
    pub(in crate::coord) watermark: Option<crate::session::RecordSequence>,
    pub(in crate::coord) selected_leaf: crate::ids::EntryId,
    pub(in crate::coord) runtime_selection: crate::session::CanonicalRuntimeSelection,
}

impl From<&crate::session::CanonicalProviderView> for ProviderContextCacheKey {
    fn from(view: &crate::session::CanonicalProviderView) -> Self {
        Self {
            owner: view.owner.clone(),
            watermark: view.watermark,
            selected_leaf: view.selected_leaf.clone(),
            runtime_selection: view.runtime_selection.clone(),
        }
    }
}

#[derive(Debug, Clone)]
pub(in crate::coord) struct QueuedAgentTurn {
    pub(in crate::coord) task_id: String,
    pub(in crate::coord) agent_id: String,
    pub(in crate::coord) session_id: crate::ids::SessionId,
    pub(in crate::coord) request_id: String,
    pub(in crate::coord) profile: AgentProfile,
    pub(in crate::coord) request: AgentRequest,
    pub(in crate::coord) queue_key: ConcurrencyKey,
    pub(in crate::coord) scheduler_queued: bool,
    pub(in crate::coord) child_task: Option<ChildTaskTurnState>,
    /// Remaining model refs to try after the current `request.model_ref` fails.
    pub(in crate::coord) model_fallback_chain: Vec<crate::config::ResolvedModelTarget>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(in crate::coord) struct ChildTaskTurnState {
    pub(in crate::coord) parent_tool_call_id: String,
    pub(in crate::coord) parent_session_id: crate::ids::SessionId,
    pub(in crate::coord) parent_agent_id: Option<String>,
    pub(in crate::coord) child_session_id: crate::ids::SessionId,
    pub(in crate::coord) child_request_id: String,
    pub(in crate::coord) task_id: String,
    pub(in crate::coord) description: String,
    pub(in crate::coord) run_in_background: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(in crate::coord) struct PendingAgentWakeup {
    pub(in crate::coord) request_id: String,
    pub(in crate::coord) notification_text: String,
}

#[derive(Debug, Clone)]
pub(in crate::coord) struct RunningAgentTurn {
    pub(in crate::coord) agent_id: String,
    pub(in crate::coord) request_id: String,
    pub(in crate::coord) request_prompt: String,
    pub(in crate::coord) attachments: Vec<crate::attachment_transport::AttachmentMetadata>,
    pub(in crate::coord) profile_name: String,
    pub(in crate::coord) model_ref: String,
    pub(in crate::coord) model_settings: AgentModelSettings,
    pub(in crate::coord) profile: Option<String>,
    pub(in crate::coord) queue_key: ConcurrencyKey,
    pub(in crate::coord) cancellation_token: CancellationToken,
    pub(in crate::coord) started_mono_ms: u64,
    pub(in crate::coord) hook_executions: Vec<HookExecutionMetadata>,
    pub(in crate::coord) latest_provider_usage: Option<harness_providers::CompletionUsage>,
    pub(in crate::coord) latest_provider_request_id: Option<String>,
    pub(in crate::coord) latest_assistant_output: Option<String>,
    pub(in crate::coord) latest_provider_id: Option<String>,
    pub(in crate::coord) latest_model_id: Option<String>,
    pub(in crate::coord) child_task: Option<ChildTaskTurnState>,
}

pub(in crate::coord) fn cancelled_failure_memory_from_running(
    running: &RunningAgentTurn,
    reason: &str,
) -> Option<AgentTurnFailureMemory> {
    let provider_request_id = running.latest_provider_request_id.clone()?;
    Some(AgentTurnFailureMemory::aborted(
        "cancelled",
        reason.to_string(),
        running.latest_assistant_output.clone().unwrap_or_default(),
        Some(provider_request_id),
    ))
}

pub(in crate::coord) fn push_incomplete_provider_turn(
    run_state: &mut RunState,
    running: &RunningAgentTurn,
    fallback_request_id: &str,
    memory: AgentTurnFailureMemory,
) {
    let request_id = memory
        .provider_request_id
        .clone()
        .or_else(|| running.latest_provider_request_id.clone())
        .unwrap_or_else(|| fallback_request_id.to_string());
    let turns = run_state
        .live_incomplete_provider_turns_by_agent
        .entry(running.agent_id.clone())
        .or_default();
    if turns.iter().any(|turn| {
        turn.request_id.as_ref().map(|value| value.as_str()) == Some(request_id.as_str())
            && !turn.status.is_completed()
    }) {
        return;
    }
    turns.push(ProviderConversationTurn {
        user_prompt: running.request_prompt.clone(),
        assistant_response: memory.partial_assistant_output,
        status: memory.status,
        failure_stage: Some(memory.failure_stage),
        failure_reason: truncated_failure_reason(&memory.failure_reason),
        request_id: Some(request_id.into()),
        attachments: running.attachments.clone(),
        ..ProviderConversationTurn::default()
    });
}

pub(in crate::coord) fn agent_turn_child_lineage(
    run_state: &RunState,
    running: &RunningAgentTurn,
    request_id: &str,
) -> Option<TaskLineageMetadata> {
    if let Some(child_task) = running.child_task.as_ref() {
        return Some(TaskLineageMetadata {
            parent_tool_call_id: Some(child_task.parent_tool_call_id.clone()),
            parent_task_id: None,
            parent_request_id: None,
            parent_session_id: Some(child_task.parent_session_id.to_string()),
            child_session_id: Some(child_task.child_session_id.to_string()),
            child_request_id: Some(child_task.child_request_id.clone()),
            child_provider_id: running.latest_provider_id.clone(),
            child_model_id: running.latest_model_id.clone(),
        });
    }

    run_state
        .child_session_mirrors
        .contains_key(&running.agent_id)
        .then(|| TaskLineageMetadata {
            parent_session_id: Some(run_state.info.run_id.to_string()),
            child_session_id: Some(running.agent_id.clone()),
            child_request_id: Some(request_id.to_string()),
            ..TaskLineageMetadata::default()
        })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::coord) enum TaskExecutionState {
    Running,
}

pub(in crate::coord) struct TaskState {
    pub(in crate::coord) tool_call_id: String,
    pub(in crate::coord) tool_metadata: Option<ToolIdentityMetadata>,
    pub(in crate::coord) owner_actor: EventActor,
    pub(in crate::coord) request_correlation_id: Option<String>,
    pub(in crate::coord) queue_key: ConcurrencyKey,
    pub(in crate::coord) state: TaskExecutionState,
    pub(in crate::coord) cancellation_token: CancellationToken,
    pub(in crate::coord) started_mono_ms: u64,
    pub(in crate::coord) last_progress_mono_ms: u64,
    pub(in crate::coord) last_progress_kind: JobProgressKind,
    pub(in crate::coord) hashline_edit: Option<HashlineEditMetadata>,
    pub(in crate::coord) respond_to: Option<oneshot::Sender<Result<ToolResult, String>>>,
}

#[derive(Debug, Clone, Default)]
pub(in crate::coord) struct TaskHookState {
    pub(in crate::coord) tool_id: String,
    pub(in crate::coord) profile: Option<String>,
    pub(in crate::coord) hook_executions: Vec<HookExecutionMetadata>,
}

pub(in crate::coord) struct PendingPermissionState {
    pub(in crate::coord) tool_call_id: String,
    pub(in crate::coord) request_correlation_id: Option<String>,
    pub(in crate::coord) hook_executions: Vec<HookExecutionMetadata>,
    pub(in crate::coord) grant_request: Option<PermissionGrantRequest>,
    pub(in crate::coord) resolution: PendingPermissionResolution,
}

pub(in crate::coord) enum PendingPermissionResolution {
    ToolCall {
        tool_id: String,
        args_json: Value,
        actor: EventActor,
        profile: Option<String>,
        respond_to: Option<oneshot::Sender<Result<ToolResult, String>>>,
    },
    Question {
        actor: EventActor,
        prompts: Vec<QuestionPromptSpec>,
        respond_to: oneshot::Sender<Result<Vec<Vec<String>>, String>>,
    },
}

#[derive(Debug, Clone, Default)]
pub(in crate::coord) struct HookExecutionBatch {
    pub(in crate::coord) hook_executions: Vec<HookExecutionMetadata>,
    pub(in crate::coord) critical_failure: Option<String>,
}

#[derive(Debug, Clone)]
pub(in crate::coord) struct HookInvocationContext {
    pub(in crate::coord) event: HookLifecycleEvent,
    pub(in crate::coord) run_id: String,
    pub(in crate::coord) workspace_root: PathBuf,
    pub(in crate::coord) artifacts_dir: PathBuf,
    pub(in crate::coord) actor: Option<EventActor>,
    pub(in crate::coord) agent_id: Option<String>,
    pub(in crate::coord) request_id: Option<String>,
    pub(in crate::coord) permission_id: Option<String>,
    pub(in crate::coord) task_id: Option<String>,
    pub(in crate::coord) tool_call_id: Option<String>,
    pub(in crate::coord) tool_id: Option<String>,
    pub(in crate::coord) provider_id: Option<String>,
    pub(in crate::coord) model_id: Option<String>,
    pub(in crate::coord) parent_agent_id: Option<String>,
    pub(in crate::coord) profile: Option<String>,
    pub(in crate::coord) outcome: Option<String>,
    pub(in crate::coord) output_summary: Option<String>,
    pub(in crate::coord) failure_reason: Option<String>,
}

pub(in crate::coord) struct PermissionDeniedArgs<'a> {
    pub(in crate::coord) actor: EventActor,
    pub(in crate::coord) profile: Option<String>,
    pub(in crate::coord) tool_id: &'a str,
    pub(in crate::coord) args_json: &'a Value,
    pub(in crate::coord) tool_call_id: &'a str,
    pub(in crate::coord) hashline_edit: Option<&'a HashlineEditMetadata>,
    pub(in crate::coord) kind: PermissionKind,
    pub(in crate::coord) reason: &'a str,
    pub(in crate::coord) request_correlation_id: Option<&'a str>,
}

pub(in crate::coord) struct ToolCallRequestedEventArgs<'a> {
    pub(in crate::coord) actor: EventActor,
    pub(in crate::coord) tool_call_id: &'a str,
    pub(in crate::coord) tool_id: &'a str,
    pub(in crate::coord) args_json: &'a Value,
    pub(in crate::coord) tool_metadata: Option<ToolCallMetadata>,
    pub(in crate::coord) request_correlation_id: Option<&'a str>,
}

pub(in crate::coord) struct AgentProviderRequestStartedArgs {
    pub(in crate::coord) task_id: String,
    pub(in crate::coord) agent_id: String,
    pub(in crate::coord) request_id: String,
    pub(in crate::coord) provider_id: String,
    pub(in crate::coord) model_id: String,
    pub(in crate::coord) prompt_summary: String,
    pub(in crate::coord) request_digest: String,
    pub(in crate::coord) metadata: Option<ProviderRequestStartedMetadata>,
    pub(in crate::coord) model_target: Option<crate::config::ResolvedModelTarget>,
}

pub(in crate::coord) struct AgentProviderRequestFinishedArgs {
    pub(in crate::coord) task_id: String,
    pub(in crate::coord) agent_id: String,
    pub(in crate::coord) request_id: String,
    pub(in crate::coord) finish_reason: String,
    pub(in crate::coord) output_digest: Option<String>,
    pub(in crate::coord) usage: Option<harness_providers::CompletionUsage>,
    pub(in crate::coord) metadata: Option<ProviderRequestFinishedMetadata>,
}

pub(in crate::coord) struct ToolCallExecutionArgs {
    pub(in crate::coord) tool_call_id: String,
    pub(in crate::coord) tool_id: String,
    pub(in crate::coord) args_json: Value,
    pub(in crate::coord) actor: EventActor,
    pub(in crate::coord) profile: Option<String>,
    pub(in crate::coord) permission_ruleset: Vec<crate::perm::PermissionRule>,
    pub(in crate::coord) hook_executions: Vec<HookExecutionMetadata>,
    pub(in crate::coord) tool_registry: Arc<ToolRegistry>,
    pub(in crate::coord) request_correlation_id: Option<String>,
    pub(in crate::coord) respond_to: Option<oneshot::Sender<Result<ToolResult, String>>>,
    pub(in crate::coord) external_directory_allow_prefixes: Vec<PathBuf>,
}

pub(in crate::coord) struct PermissionRequestedEventArgs<'a> {
    pub(in crate::coord) permission_id: &'a str,
    pub(in crate::coord) tool_call_id: &'a str,
    pub(in crate::coord) kind: PermissionKind,
    pub(in crate::coord) summary: String,
    pub(in crate::coord) request_digest: String,
    pub(in crate::coord) timeout_ms: u64,
    pub(in crate::coord) default_decision: EventPermissionDecision,
    pub(in crate::coord) request_correlation_id: Option<&'a str>,
}

pub(in crate::coord) struct ToolCallFinishedEventArgs<'a> {
    pub(in crate::coord) tool_call_id: &'a str,
    pub(in crate::coord) status: ToolCallStatus,
    pub(in crate::coord) output_summary: Option<String>,
    pub(in crate::coord) output_json: Option<Value>,
    pub(in crate::coord) metadata: Option<ToolCallMetadata>,
    pub(in crate::coord) request_correlation_id: Option<&'a str>,
    pub(in crate::coord) causation_id: Option<&'a str>,
}

pub(in crate::coord) struct EditAppliedEventArgs<'a> {
    pub(in crate::coord) tool_call_id: &'a str,
    pub(in crate::coord) metadata: &'a HashlineEditMetadata,
    pub(in crate::coord) new_file_digest: String,
    pub(in crate::coord) diff_rel_path: Option<String>,
    pub(in crate::coord) diff_digest: Option<String>,
    pub(in crate::coord) request_correlation_id: Option<&'a str>,
}
