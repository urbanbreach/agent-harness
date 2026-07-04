use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use serde_json::Value;
use tokio::sync::{mpsc, oneshot};
use tokio::time::sleep;
use tokio_stream::StreamExt;

use super::*;

#[derive(Debug, Clone)]
pub struct CoordinatorHandle {
    pub(in crate::coord) tx: mpsc::Sender<Command>,
}

impl CoordinatorHandle {
    async fn request<T>(
        &self,
        build_command: impl FnOnce(oneshot::Sender<Result<T, CoordinatorError>>) -> Command,
    ) -> Result<T, CoordinatorError> {
        let (respond_to, response_rx) = oneshot::channel();
        self.tx
            .send(build_command(respond_to))
            .await
            .map_err(|_| CoordinatorError::CommandChannelClosed)?;

        response_rx
            .await
            .map_err(|_| CoordinatorError::ResponseChannelClosed)?
    }

    async fn request_string_error<T>(
        &self,
        build_command: impl FnOnce(oneshot::Sender<Result<T, String>>) -> Command,
    ) -> Result<T, String> {
        let (respond_to, response_rx) = oneshot::channel();
        self.tx
            .send(build_command(respond_to))
            .await
            .map_err(|_| CoordinatorError::CommandChannelClosed.to_string())?;

        response_rx
            .await
            .map_err(|_| CoordinatorError::ResponseChannelClosed.to_string())?
    }

    async fn send_command(&self, command: Command) -> Result<(), CoordinatorError> {
        self.tx
            .send(command)
            .await
            .map_err(|_| CoordinatorError::CommandChannelClosed)
    }

    pub async fn start_run(
        &self,
        run_name: impl Into<String>,
        workspace_root: impl Into<PathBuf>,
    ) -> Result<RunInfo, CoordinatorError> {
        self.request(|respond_to| Command::StartRun {
            run_name: run_name.into(),
            workspace_root: workspace_root.into(),
            respond_to,
        })
        .await
    }

    pub async fn resume_run(
        &self,
        run_id: impl Into<String>,
        run_name: impl Into<String>,
    ) -> Result<RunInfo, CoordinatorError> {
        self.request(|respond_to| Command::ResumeRun {
            run_id: run_id.into(),
            run_name: run_name.into(),
            respond_to,
        })
        .await
    }

    pub async fn stop_run(&self) -> Result<(), CoordinatorError> {
        self.request(|respond_to| Command::StopRun { respond_to })
            .await
    }

    pub async fn fail_run(&self, error: impl Into<String>) -> Result<(), CoordinatorError> {
        let error = error.into();
        self.request(|respond_to| Command::FailRun { error, respond_to })
            .await
    }

    pub async fn event_store(&self) -> Result<Arc<dyn EventStore>, CoordinatorError> {
        let store = self
            .request(|respond_to| Command::GetEventStore { respond_to })
            .await?;
        let store: Arc<dyn EventStore> = store;
        Ok(store)
    }

    pub async fn run_info(&self) -> Result<RunInfo, CoordinatorError> {
        self.request(|respond_to| Command::GetRunInfo { respond_to })
            .await
    }

    pub async fn update_session_title(
        &self,
        title: impl Into<String>,
    ) -> Result<RunInfo, CoordinatorError> {
        self.request(|respond_to| Command::UpdateSessionTitle {
            title: title.into(),
            respond_to,
        })
        .await
    }

    pub async fn agent_runtime_info(
        &self,
        agent_id: impl Into<String>,
    ) -> Result<AgentRuntimeInfo, CoordinatorError> {
        self.request(|respond_to| Command::GetAgentRuntimeInfo {
            agent_id: agent_id.into(),
            respond_to,
        })
        .await
    }

    pub async fn spawn_agent(
        &self,
        actor: EventActor,
        profile: impl Into<String>,
        parent_agent_id: Option<String>,
    ) -> Result<String, CoordinatorError> {
        self.request(|respond_to| Command::SpawnAgent {
            actor,
            profile: profile.into(),
            parent_agent_id,
            child_session_title: None,
            respond_to,
        })
        .await
    }

    pub async fn spawn_agent_idle(
        &self,
        actor: EventActor,
        profile: impl Into<String>,
        parent_agent_id: Option<String>,
    ) -> Result<String, CoordinatorError> {
        self.request(|respond_to| Command::SpawnAgentIdle {
            actor,
            profile: profile.into(),
            parent_agent_id,
            child_session_title: None,
            respond_to,
        })
        .await
    }

    pub async fn spawn_agent_idle_with_child_title(
        &self,
        actor: EventActor,
        profile: impl Into<String>,
        parent_agent_id: Option<String>,
        child_session_title: impl Into<String>,
    ) -> Result<String, CoordinatorError> {
        self.request(|respond_to| Command::SpawnAgentIdle {
            actor,
            profile: profile.into(),
            parent_agent_id,
            child_session_title: Some(child_session_title.into()),
            respond_to,
        })
        .await
    }

    pub async fn request_tool_call(
        &self,
        actor: EventActor,
        category: Option<String>,
        tool_id: impl Into<String>,
        args_json: Value,
    ) -> Result<String, CoordinatorError> {
        self.request(|respond_to| Command::RequestToolCall {
            actor,
            category,
            tool_id: tool_id.into(),
            args_json,
            respond_to,
        })
        .await
    }

    pub async fn execute_agent_tool_call(
        &self,
        actor: EventActor,
        category: Option<String>,
        tool_id: impl Into<String>,
        args_json: Value,
    ) -> Result<ToolResult, String> {
        self.request_string_error(|respond_to| Command::ExecuteAgentToolCall {
            actor,
            category,
            tool_id: tool_id.into(),
            args_json,
            respond_to,
        })
        .await
    }

    pub async fn request_agent_turn(
        &self,
        actor: EventActor,
        agent_id: impl Into<String>,
        prompt: impl Into<String>,
    ) -> Result<String, CoordinatorError> {
        self.request_agent_turn_with_model(actor, agent_id, prompt, None, None)
            .await
    }

    pub async fn request_agent_turn_with_model(
        &self,
        actor: EventActor,
        agent_id: impl Into<String>,
        prompt: impl Into<String>,
        model_ref_override: Option<String>,
        model_settings_override: Option<AgentModelSettings>,
    ) -> Result<String, CoordinatorError> {
        self.request_agent_turn_with_model_and_selected_tags(
            actor,
            agent_id,
            prompt,
            crate::file_tag::SelectedPromptTags::default(),
            model_ref_override,
            model_settings_override,
        )
        .await
    }

    pub async fn request_agent_turn_with_model_and_selected_tags(
        &self,
        actor: EventActor,
        agent_id: impl Into<String>,
        prompt: impl Into<String>,
        selected_tags: crate::file_tag::SelectedPromptTags,
        model_ref_override: Option<String>,
        model_settings_override: Option<AgentModelSettings>,
    ) -> Result<String, CoordinatorError> {
        self.request(|respond_to| Command::RequestAgentTurn {
            actor,
            agent_id: agent_id.into(),
            prompt: prompt.into(),
            selected_file_tags: selected_tags.files,
            selected_agent_tags: selected_tags.agents,
            selected_resource_tags: selected_tags.resources,
            model_ref_override,
            model_settings_override,
            child_task_metadata: None,
            respond_to,
        })
        .await
    }

    pub async fn request_child_agent_turn_with_model(
        &self,
        actor: EventActor,
        agent_id: impl Into<String>,
        prompt: impl Into<String>,
        model_ref_override: Option<String>,
        model_settings_override: Option<AgentModelSettings>,
        child_task_metadata: ChildTaskRequestMetadata,
    ) -> Result<String, CoordinatorError> {
        self.request(|respond_to| Command::RequestAgentTurn {
            actor,
            agent_id: agent_id.into(),
            prompt: prompt.into(),
            selected_file_tags: Vec::new(),
            selected_agent_tags: Vec::new(),
            selected_resource_tags: Vec::new(),
            model_ref_override,
            model_settings_override,
            child_task_metadata: Some(child_task_metadata),
            respond_to,
        })
        .await
    }

    pub async fn compact_agent_context(
        &self,
        agent_id: impl Into<String>,
        through_request_id: Option<String>,
        trigger_reason: impl Into<String>,
    ) -> Result<ManualCompactionOutcome, CoordinatorError> {
        self.request(|respond_to| Command::ManualCompactAgentContext {
            agent_id: agent_id.into(),
            through_request_id,
            trigger_reason: trigger_reason.into(),
            respond_to,
        })
        .await
    }

    pub async fn resolve_permission(
        &self,
        permission_id: impl Into<String>,
        decision: PermissionDecision,
        reason: Option<String>,
    ) -> Result<(), CoordinatorError> {
        self.resolve_permission_with_grant_scope(permission_id, decision, reason, None)
            .await
    }

    pub async fn resolve_permission_with_grant_scope(
        &self,
        permission_id: impl Into<String>,
        decision: PermissionDecision,
        reason: Option<String>,
        grant_scope: Option<PermissionGrantScope>,
    ) -> Result<(), CoordinatorError> {
        self.request(|respond_to| Command::ResolvePermission {
            permission_id: permission_id.into(),
            decision,
            reason,
            grant_scope,
            respond_to,
        })
        .await
    }

    pub async fn request_question(
        &self,
        actor: EventActor,
        tool_call_id: impl Into<String>,
        request_json: Value,
    ) -> Result<Vec<Vec<String>>, String> {
        self.request_string_error(|respond_to| Command::RequestQuestion {
            actor,
            tool_call_id: tool_call_id.into(),
            request_json,
            respond_to,
        })
        .await
    }

    pub async fn job_progress(
        &self,
        task_id: impl Into<String>,
        kind: JobProgressKind,
    ) -> Result<(), CoordinatorError> {
        self.send_command(Command::JobProgress {
            task_id: task_id.into(),
            kind,
        })
        .await
    }

    pub async fn cancel_task(
        &self,
        task_id: impl Into<String>,
        reason: impl Into<String>,
    ) -> Result<(), CoordinatorError> {
        self.request(|respond_to| Command::CancelTask {
            task_id: task_id.into(),
            reason: reason.into(),
            respond_to,
        })
        .await
    }

    pub async fn background_request_projection(
        &self,
        actor: EventActor,
        request_id: Option<String>,
        selector_hint: Option<String>,
    ) -> Result<BackgroundRequestProjection, CoordinatorError> {
        self.request(|respond_to| Command::GetBackgroundRequestProjection {
            actor,
            request_id,
            selector_hint,
            respond_to,
        })
        .await
    }

    pub async fn cancel_background_request(
        &self,
        actor: EventActor,
        request_id: Option<String>,
        selector_hint: Option<String>,
        reason: impl Into<String>,
    ) -> Result<BackgroundRequestProjection, CoordinatorError> {
        self.request(|respond_to| Command::CancelBackgroundRequest {
            actor,
            request_id,
            selector_hint,
            reason: reason.into(),
            respond_to,
        })
        .await
    }

    pub async fn background_foreground_child_tasks(&self) -> Result<usize, CoordinatorError> {
        self.request(|respond_to| Command::BackgroundForegroundChildTasks { respond_to })
            .await
    }

    pub async fn wait_background_request_terminal(
        &self,
        request_id: impl Into<String>,
        scheduler_task_id: impl Into<String>,
        timeout_ms: u64,
    ) -> Result<bool, CoordinatorError> {
        let request_id = request_id.into();
        let scheduler_task_id = scheduler_task_id.into();
        let store = self.event_store().await?;
        let mut stream = store.subscribe(1)?;
        let deadline = Instant::now() + Duration::from_millis(timeout_ms);

        while Instant::now() < deadline {
            let remaining = deadline.saturating_duration_since(Instant::now());
            let next =
                tokio::time::timeout(remaining.min(Duration::from_millis(250)), stream.next())
                    .await;
            match next {
                Ok(Some(Ok(event))) => {
                    if event.correlation_id.as_deref() == Some(request_id.as_str())
                        && background_terminal_event_matches_task(&event, &scheduler_task_id)
                    {
                        return Ok(true);
                    }
                }
                Ok(Some(Err(err))) => return Err(CoordinatorError::EventStore(err)),
                Ok(None) | Err(_) => sleep(Duration::from_millis(10)).await,
            }
        }

        Ok(false)
    }

    pub async fn job_finished(
        &self,
        task_id: impl Into<String>,
        outcome: JobOutcome,
    ) -> Result<(), CoordinatorError> {
        self.send_command(Command::JobFinished {
            task_id: task_id.into(),
            outcome,
        })
        .await
    }

    pub async fn snapshot_workspace(
        &self,
        request_id: impl Into<String>,
    ) -> Result<WorkspaceSnapshotSummary, CoordinatorError> {
        self.request(|respond_to| Command::SnapshotWorkspace {
            request_id: request_id.into(),
            respond_to,
        })
        .await
    }

    pub async fn revert_workspace(
        &self,
        snapshot_request_id: impl Into<String>,
    ) -> Result<WorkspaceRevertSummary, CoordinatorError> {
        self.request(|respond_to| Command::RevertWorkspace {
            snapshot_request_id: snapshot_request_id.into(),
            respond_to,
        })
        .await
    }
}
