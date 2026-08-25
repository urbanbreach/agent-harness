// allow: SIZE_OK — coordinator state machine (turn lifecycle + scheduling)
use super::*;
use crate::event::BackgroundTaskNotificationEvent;
use crate::proj::BackgroundRequestProjectionError;

pub(in crate::coord) fn background_notification_status_for_cancel_reason(
    reason: &str,
) -> BackgroundTaskNotificationStatus {
    let lower = reason.to_ascii_lowercase();
    if lower.contains("cancel") || lower.contains("aborted") {
        BackgroundTaskNotificationStatus::Cancelled
    } else {
        BackgroundTaskNotificationStatus::Failed
    }
}

pub(in crate::coord) fn terminal_event_summary(event: &EventEnvelopeV1) -> String {
    match &event.payload {
        EventV1::TaskCompleted(payload) => payload.result_summary.clone(),
        EventV1::TaskCancelled(payload) => payload.reason.clone(),
        _ => String::new(),
    }
}

pub(in crate::coord) fn background_projection_error_to_coordinator_error(
    err: BackgroundRequestProjectionError,
) -> CoordinatorError {
    match err {
        BackgroundRequestProjectionError::Unauthorized => {
            CoordinatorError::PermissionDenied(err.to_string())
        }
        BackgroundRequestProjectionError::MissingSelector
        | BackgroundRequestProjectionError::UnknownRequest(_)
        | BackgroundRequestProjectionError::UnknownSelector(_)
        | BackgroundRequestProjectionError::MissingProjection(_) => {
            CoordinatorError::UnknownTask(err.to_string())
        }
    }
}

pub(in crate::coord) fn background_terminal_event_matches_task(
    event: &EventEnvelopeV1,
    scheduler_task_id: &str,
) -> bool {
    match &event.payload {
        EventV1::TaskCompleted(payload) => {
            payload.task_id.as_str() == scheduler_task_id
                || payload
                    .metadata
                    .as_ref()
                    .and_then(|metadata| metadata.task_scope)
                    == Some(TaskTerminalScope::AgentTurn)
        }
        EventV1::TaskCancelled(payload) => {
            payload.task_id.as_str() == scheduler_task_id
                || payload.task_scope == Some(TaskTerminalScope::AgentTurn)
        }
        _ => false,
    }
}

/// Multi-background wait mode for coordinator-owned wait-any / wait-all.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BackgroundWaitMode {
    /// Return when the first watched background request becomes terminal.
    Any,
    /// Return when every watched background request is terminal.
    All,
}

impl BackgroundWaitMode {
    /// Parse `any` / `all` (case-insensitive).
    #[must_use]
    pub fn parse(raw: &str) -> Option<Self> {
        match raw.trim().to_ascii_lowercase().as_str() {
            "any" => Some(Self::Any),
            "all" => Some(Self::All),
            _ => None,
        }
    }

    /// Stable wire / schema token for this mode.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Any => "any",
            Self::All => "all",
        }
    }
}

/// Outcome of a multi-request background wait.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BackgroundWaitOutcome {
    /// Whether the wait condition was satisfied before timeout.
    pub satisfied: bool,
    /// First request_id observed (or known) terminal when the condition became true.
    pub first_terminal_request_id: Option<String>,
}

/// Pure wait-condition predicate for multi-background synchronization.
///
/// Empty sets never satisfy either mode. Cancelled and completed both count as
/// terminal; late results do not change terminal membership.
#[must_use]
pub fn background_wait_condition_satisfied<S: AsRef<str>>(
    mode: BackgroundWaitMode,
    terminal_by_request: &[(S, bool)],
) -> bool {
    if terminal_by_request.is_empty() {
        return false;
    }
    match mode {
        BackgroundWaitMode::Any => terminal_by_request.iter().any(|(_, terminal)| *terminal),
        BackgroundWaitMode::All => terminal_by_request.iter().all(|(_, terminal)| *terminal),
    }
}

/// First terminal request id in caller order (stable for already-satisfied waits).
#[must_use]
pub fn first_terminal_request_id<S: AsRef<str>>(
    terminal_by_request: &[(S, bool)],
) -> Option<String> {
    terminal_by_request
        .iter()
        .find_map(|(request_id, terminal)| (*terminal).then(|| request_id.as_ref().to_string()))
}

fn background_task_notification_text(notification: &BackgroundTaskNotificationEvent) -> String {
    format!(
        "[BACKGROUND TASK {}]\nID: {}\nRequest ID: {}\nDescription: {}\nStatus: {}\n\n{}\n\nUse background_output(request_id=\"{}\") for full details or task(session_id=\"{}\") to continue analysis from the child session.",
        notification.status.as_str().to_ascii_uppercase(),
        notification.task_id,
        notification.child_request_id,
        notification.description,
        notification.status.as_str(),
        notification.summary,
        notification.child_request_id,
        notification.child_session_id,
    )
}

fn background_task_notification_reminder_text(
    notification: &BackgroundTaskNotificationEvent,
) -> String {
    format!(
        "<system-reminder>\n{}\n</system-reminder>",
        background_task_notification_text(notification)
    )
}

fn build_background_task_notification<R>(
    redactor: &R,
    child_task: &ChildTaskTurnState,
    parent_agent_id: Option<String>,
    delivered_turn_request_id: Option<String>,
    terminal_event: &EventEnvelopeV1,
    status: BackgroundTaskNotificationStatus,
    summary: &str,
) -> BackgroundTaskNotificationEvent
where
    R: Redactor + ?Sized,
{
    let capped_description = truncate_with_ellipsis(
        &redactor.redact_text(&child_task.description),
        BACKGROUND_TASK_NOTIFICATION_DESCRIPTION_MAX_CHARS,
    );
    let capped_summary = truncate_with_ellipsis(
        &redactor.redact_text(summary),
        BACKGROUND_TASK_NOTIFICATION_SUMMARY_MAX_CHARS,
    );

    BackgroundTaskNotificationEvent {
        parent_session_id: child_task.parent_session_id.clone(),
        parent_agent_id,
        child_session_id: child_task.child_session_id.clone(),
        child_request_id: child_task.child_request_id.clone(),
        task_id: child_task.task_id.clone().into(),
        description: capped_description,
        status,
        summary: capped_summary,
        terminal_event_id: terminal_event.event_id.clone(),
        terminal_task_id: terminal_terminal_task_id(terminal_event),
        delivered_turn_request_id,
    }
}

#[expect(
    clippy::too_many_arguments,
    reason = "background notification scheduling needs explicit coordinator dependencies"
)]
pub(in crate::coord) async fn append_background_task_notification_and_schedule<C, R>(
    clock: &C,
    redactor: &R,
    hook_command_executor: Arc<dyn LifecycleHookCommandExecutor + Send + Sync>,
    job_tx: mpsc::Sender<Command>,
    run_state: &mut RunState,
    hook_runtime_config: HookRuntimeConfig,
    compaction_config: CompactionSettings,
    provider_retry_config: ProviderRetryRuntimeConfig,
    provider: Arc<dyn Provider>,
    tool_registry: Arc<ToolRegistry>,
    child_task: Option<ChildTaskTurnState>,
    terminal_event: &EventEnvelopeV1,
    status: BackgroundTaskNotificationStatus,
    summary: &str,
) -> Result<(), CoordinatorError>
where
    C: Clock + ?Sized,
    R: Redactor + ?Sized,
{
    let Some(child_task) = child_task.filter(|metadata| metadata.run_in_background) else {
        return Ok(());
    };

    if !run_state
        .background_notification_child_requests
        .insert(child_task.child_request_id.clone())
    {
        return Ok(());
    }

    let parent_agent_id = child_task.parent_agent_id.clone();
    let parent_profile = parent_agent_id
        .as_deref()
        .and_then(|agent_id| run_state.agents.get(agent_id))
        .cloned();
    let delivered_turn_request_id = parent_profile
        .as_ref()
        .map(|_| allocate_provider_request_id(run_state));
    let notification = build_background_task_notification(
        redactor,
        &child_task,
        parent_agent_id.clone(),
        delivered_turn_request_id.clone(),
        terminal_event,
        status,
        summary,
    );
    let notification_text = background_task_notification_reminder_text(&notification);

    append_payload_event_with_correlation(
        clock,
        redactor,
        run_state,
        system_actor(),
        Some(format!(
            "background_task_notification:{}",
            notification.child_request_id
        )),
        Some(notification.child_request_id.clone()),
        EventV1::BackgroundTaskNotification(notification),
    )?;

    let (Some(parent_agent_id), Some(parent_profile), Some(delivered_turn_request_id)) =
        (parent_agent_id, parent_profile, delivered_turn_request_id)
    else {
        return Ok(());
    };

    append_payload_event_with_correlation(
        clock,
        redactor,
        run_state,
        system_actor(),
        Some(format!("agent:{parent_agent_id}")),
        Some(delivered_turn_request_id.clone()),
        EventV1::UserMessageSubmitted(UserMessageSubmittedEvent {
            request_id: delivered_turn_request_id.clone().into(),
            text: notification_text.clone(),
        }),
    )?;

    if run_state.agent_has_running_turn(&parent_agent_id) {
        run_state
            .pending_agent_wakeups
            .entry(parent_agent_id)
            .or_default()
            .push(PendingAgentWakeup {
                request_id: delivered_turn_request_id,
                notification_text,
            });
        return Ok(());
    }

    schedule_agent_turn(
        clock,
        redactor,
        hook_command_executor,
        job_tx,
        run_state,
        hook_runtime_config,
        compaction_config,
        provider_retry_config,
        ScheduleAgentTurnArgs {
            provider,
            tool_registry,
            profile: parent_profile.clone(),
            request: AgentRequest {
                agent_id: parent_agent_id,
                prompt: notification_text,
                attachments: Vec::new(),
                prompt_context: None,
                selected_file_tags: Vec::new(),
                selected_agent_tags: Vec::new(),
                selected_resource_tags: Vec::new(),
                model_ref: parent_profile.model_ref.clone(),
                model_target: None,
                model_settings: default_model_settings_for_profile(&parent_profile.name),
            },
            request_id: delivered_turn_request_id,
            child_task: None,
            model_fallback_chain: Vec::new(),
        },
    )
    .await
}

#[expect(
    clippy::too_many_arguments,
    reason = "pending wakeup scheduling needs explicit coordinator dependencies"
)]
pub(in crate::coord) async fn schedule_pending_agent_wakeups_for_idle_agent<C, R>(
    clock: &C,
    redactor: &R,
    hook_command_executor: Arc<dyn LifecycleHookCommandExecutor + Send + Sync>,
    job_tx: mpsc::Sender<Command>,
    run_state: &mut RunState,
    hook_runtime_config: HookRuntimeConfig,
    compaction_config: CompactionSettings,
    provider_retry_config: ProviderRetryRuntimeConfig,
    provider: Arc<dyn Provider>,
    tool_registry: Arc<ToolRegistry>,
    agent_id: &str,
) -> Result<(), CoordinatorError>
where
    C: Clock + ?Sized,
    R: Redactor + ?Sized,
{
    if run_state.agent_has_running_turn(agent_id) {
        return Ok(());
    }

    let Some(wakeups) = run_state.pending_agent_wakeups.remove(agent_id) else {
        return Ok(());
    };
    let Some(parent_profile) = run_state.agents.get(agent_id).cloned() else {
        return Ok(());
    };

    for wakeup in wakeups {
        schedule_agent_turn(
            clock,
            redactor,
            Arc::clone(&hook_command_executor),
            job_tx.clone(),
            run_state,
            hook_runtime_config.clone(),
            compaction_config.clone(),
            provider_retry_config,
            ScheduleAgentTurnArgs {
                provider: Arc::clone(&provider),
                tool_registry: Arc::clone(&tool_registry),
                profile: parent_profile.clone(),
                request: AgentRequest {
                    agent_id: agent_id.to_string(),
                    prompt: wakeup.notification_text,
                    attachments: Vec::new(),
                    prompt_context: None,
                    selected_file_tags: Vec::new(),
                    selected_agent_tags: Vec::new(),
                    selected_resource_tags: Vec::new(),
                    model_ref: parent_profile.model_ref.clone(),
                    model_target: None,
                    model_settings: default_model_settings_for_profile(&parent_profile.name),
                },
                request_id: wakeup.request_id,
                child_task: None,
                model_fallback_chain: Vec::new(),
            },
        )
        .await?;
    }

    Ok(())
}

fn terminal_terminal_task_id(event: &EventEnvelopeV1) -> String {
    match &event.payload {
        EventV1::TaskCompleted(payload) => payload.task_id.to_string(),
        EventV1::TaskCancelled(payload) => payload.task_id.to_string(),
        EventV1::TaskResultLate(payload) => payload.task_id.to_string(),
        _ => String::new(),
    }
}

#[cfg(test)]
mod wait_mode_tests {
    use super::{
        background_wait_condition_satisfied, first_terminal_request_id, BackgroundWaitMode,
    };

    #[test]
    fn wait_any_is_true_when_first_of_n_is_terminal() {
        // arrange
        // act
        // assert
        let flags = [("req_a", false), ("req_b", true), ("req_c", false)];
        assert!(background_wait_condition_satisfied(
            BackgroundWaitMode::Any,
            &flags
        ));
        assert_eq!(first_terminal_request_id(&flags).as_deref(), Some("req_b"));
    }

    #[test]
    fn wait_any_is_false_when_none_terminal() {
        // arrange
        // act
        // assert
        let flags = [("req_a", false), ("req_b", false)];
        assert!(!background_wait_condition_satisfied(
            BackgroundWaitMode::Any,
            &flags
        ));
        assert_eq!(first_terminal_request_id(&flags), None);
    }

    #[test]
    fn wait_all_is_true_only_when_every_request_is_terminal() {
        // arrange
        // act
        // assert
        let partial = [("req_a", true), ("req_b", false)];
        let complete = [("req_a", true), ("req_b", true)];
        assert!(!background_wait_condition_satisfied(
            BackgroundWaitMode::All,
            &partial
        ));
        assert!(background_wait_condition_satisfied(
            BackgroundWaitMode::All,
            &complete
        ));
    }

    #[test]
    fn wait_condition_rejects_empty_request_set() {
        // arrange
        // act
        // assert
        let empty: [(&str, bool); 0] = [];
        assert!(!background_wait_condition_satisfied(
            BackgroundWaitMode::Any,
            &empty
        ));
        assert!(!background_wait_condition_satisfied(
            BackgroundWaitMode::All,
            &empty
        ));
    }

    #[test]
    fn wait_mode_parse_accepts_any_and_all_case_insensitively() {
        // arrange
        // act
        // assert
        assert_eq!(
            BackgroundWaitMode::parse("any"),
            Some(BackgroundWaitMode::Any)
        );
        assert_eq!(
            BackgroundWaitMode::parse("ALL"),
            Some(BackgroundWaitMode::All)
        );
        assert_eq!(BackgroundWaitMode::parse("maybe"), None);
    }
}
