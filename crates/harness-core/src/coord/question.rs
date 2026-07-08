use crate::UnwrapOrAbort;
use std::time::Duration;

use serde::Deserialize;
use serde_json::Value;
use tokio::sync::oneshot;

use crate::config::HookLifecycleEvent;
use crate::event::{EventActor, PermissionDecision as EventPermissionDecision};
use crate::perm::{permission_kind_for_tool, PermissionKind, PermissionPolicy, PolicyDecision};
use crate::question_answers::{validate_question_answers, QuestionAnswerPrompt};
use crate::text::non_empty_trimmed;

use super::DEFAULT_QUESTION_TIMEOUT_MS;
use super::{
    append_permission_requested_event, append_permission_resolved_event, hooks,
    permission_request_digest, tool_request_correlation_id, Command, CoordinatorError,
    HookInvocationContext, PendingPermissionResolution, PendingPermissionState,
    PermissionRequestedEventArgs,
};

#[derive(Debug, Clone, Deserialize)]
pub(super) struct QuestionRequestSpec {
    questions: Vec<QuestionPromptSpec>,
}

#[derive(Debug, Clone, Deserialize)]
pub(super) struct QuestionPromptSpec {
    #[serde(rename = "question")]
    _question: String,
    header: String,
    options: Vec<QuestionOptionSpec>,
    #[serde(default)]
    multiple: Option<bool>,
}

#[derive(Debug, Clone, Deserialize)]
struct QuestionOptionSpec {
    label: String,
    #[serde(rename = "description")]
    _description: String,
}

impl QuestionAnswerPrompt for QuestionPromptSpec {
    fn header(&self) -> &str {
        &self.header
    }

    fn multiple(&self) -> bool {
        self.multiple.unwrap_or(false)
    }

    fn canonical_option_label<'a>(&'a self, answer: &str) -> Option<&'a str> {
        self.options
            .iter()
            .find(|option| option.label.eq_ignore_ascii_case(answer))
            .map(|option| option.label.as_str())
    }
}

fn parse_question_answers_reason(reason: Option<&str>) -> Result<Vec<Vec<String>>, String> {
    let Some(reason) = reason.and_then(non_empty_trimmed) else {
        return Err("question answers were not provided".to_string());
    };

    serde_json::from_str::<Vec<Vec<String>>>(reason)
        .map_err(|err| format!("invalid question answer payload: {err}"))
}

pub(super) fn validate_question_answers_reason(
    reason: Option<&str>,
    prompts: &[QuestionPromptSpec],
) -> Result<Vec<Vec<String>>, String> {
    let answers = parse_question_answers_reason(reason)?;
    validate_question_answers(prompts, answers)
}

pub(super) fn parse_question_request_prompts(
    request_json: &Value,
) -> Result<Vec<QuestionPromptSpec>, String> {
    let request = serde_json::from_value::<QuestionRequestSpec>(request_json.clone())
        .map_err(|err| format!("invalid question request payload: {err}"))?;
    validate_question_prompts(request.questions)
}

fn validate_question_prompts(
    prompts: Vec<QuestionPromptSpec>,
) -> Result<Vec<QuestionPromptSpec>, String> {
    if prompts.is_empty() {
        return Err("at least one question is required".to_string());
    }

    Ok(prompts)
}

pub(super) fn question_request_timeout_ms(permission_policy: &PermissionPolicy) -> u64 {
    match permission_policy.evaluate(None, PermissionKind::Question) {
        PolicyDecision::Ask { timeout_ms, .. } => timeout_ms,
        PolicyDecision::Allow | PolicyDecision::Deny => DEFAULT_QUESTION_TIMEOUT_MS,
    }
}

impl super::Coordinator {
    pub(in crate::coord) async fn request_question_internal(
        &mut self,
        actor: EventActor,
        tool_call_id: String,
        request_json: Value,
        respond_to: oneshot::Sender<Result<Vec<Vec<String>>, String>>,
    ) -> Result<(), CoordinatorError> {
        let mut respond_to = Some(respond_to);
        let result = async {
            let run_state = self
                .run_state
                .as_mut()
                .ok_or(CoordinatorError::RunNotStarted)?;
            let prompts = parse_question_request_prompts(&request_json)
                .map_err(CoordinatorError::PolicyViolation)?;

            let permission_id = format!("perm_{:06}", run_state.next_permission_id);
            run_state.next_permission_id += 1;
            let request_correlation_id = tool_request_correlation_id(run_state, &actor);
            let kind = permission_kind_for_tool("question").ok_or_else(|| {
                CoordinatorError::PolicyViolation(
                    "question must resolve to a formal permission kind".to_string(),
                )
            })?;
            let timeout_ms = question_request_timeout_ms(&self.config.permission_policy);
            let request_summary = serde_json::to_string(&request_json)?;
            let request_digest = permission_request_digest(kind.as_str(), &request_json);
            let hook_request_id = request_correlation_id
                .clone()
                .or_else(|| Some(tool_call_id.clone()));

            append_permission_requested_event(
                self.clock.as_ref(),
                self.redactor.as_ref(),
                run_state,
                PermissionRequestedEventArgs {
                    permission_id: &permission_id,
                    tool_call_id: &tool_call_id,
                    kind,
                    summary: request_summary.clone(),
                    request_digest,
                    timeout_ms,
                    default_decision: EventPermissionDecision::Deny,
                    request_correlation_id: request_correlation_id.as_deref(),
                },
            )?;

            let requested_hook_batch = hooks::run_lifecycle_hooks(
                self.clock.as_ref(),
                self.config.hook_command_executor.as_ref(),
                &self.config.hook_runtime_config,
                HookInvocationContext {
                    event: HookLifecycleEvent::PermissionRequested,
                    run_id: run_state.info.run_id.to_string(),
                    workspace_root: run_state.info.workspace_root.clone(),
                    artifacts_dir: run_state.info.artifacts_dir.clone(),
                    actor: Some(actor.clone()),
                    agent_id: actor.agent_id.clone(),
                    request_id: hook_request_id.clone(),
                    permission_id: Some(permission_id.clone()),
                    task_id: None,
                    tool_call_id: Some(tool_call_id.clone()),
                    tool_id: Some("user.question".to_string()),
                    provider_id: None,
                    model_id: None,
                    parent_agent_id: None,
                    category: None,
                    outcome: Some("requested".to_string()),
                    output_summary: Some(request_summary),
                    failure_reason: None,
                },
            )
            .await;

            let mut pending = PendingPermissionState {
                tool_call_id,
                request_correlation_id,
                hook_executions: requested_hook_batch.hook_executions.clone(),
                grant_request: None,
                resolution: PendingPermissionResolution::Question {
                    actor: actor.clone(),
                    prompts,
                    respond_to: respond_to.take().unwrap_or_abort(),
                },
            };

            if let Some(reason) = requested_hook_batch.critical_failure {
                let mut final_reason = format!("critical lifecycle hook failed: {reason}");
                append_permission_resolved_event(
                    self.clock.as_ref(),
                    self.redactor.as_ref(),
                    run_state,
                    permission_id.clone(),
                    EventPermissionDecision::Deny,
                    Some(final_reason.clone()),
                )?;

                let resolved_hook_batch = hooks::run_lifecycle_hooks(
                    self.clock.as_ref(),
                    self.config.hook_command_executor.as_ref(),
                    &self.config.hook_runtime_config,
                    HookInvocationContext {
                        event: HookLifecycleEvent::PermissionResolved,
                        run_id: run_state.info.run_id.to_string(),
                        workspace_root: run_state.info.workspace_root.clone(),
                        artifacts_dir: run_state.info.artifacts_dir.clone(),
                        actor: Some(actor.clone()),
                        agent_id: actor.agent_id.clone(),
                        request_id: hook_request_id,
                        permission_id: Some(permission_id),
                        task_id: None,
                        tool_call_id: Some(pending.tool_call_id.clone()),
                        tool_id: Some("user.question".to_string()),
                        provider_id: None,
                        model_id: None,
                        parent_agent_id: None,
                        category: None,
                        outcome: Some("deny".to_string()),
                        output_summary: Some(final_reason.clone()),
                        failure_reason: Some(final_reason.clone()),
                    },
                )
                .await;
                pending
                    .hook_executions
                    .extend(resolved_hook_batch.hook_executions.clone());
                if let Some(resolved_reason) = resolved_hook_batch.critical_failure {
                    final_reason = format!(
                        "{final_reason}; critical lifecycle hook failed: {resolved_reason}"
                    );
                }

                if let PendingPermissionResolution::Question { respond_to, .. } = pending.resolution
                {
                    let _ = respond_to.send(Err(final_reason.clone()));
                }
                return Err(CoordinatorError::LifecycleHookFailed(final_reason));
            }

            run_state.insert_pending_permission(permission_id.clone(), pending);

            let job_tx = self.job_tx.clone();
            if timeout_ms > 0 {
                tokio::spawn(async move {
                    tokio::time::sleep(Duration::from_millis(timeout_ms)).await;
                    let _ = job_tx
                        .send(Command::PermissionTimedOut { permission_id })
                        .await;
                });
            }

            Ok(())
        }
        .await;

        if let Err(err) = &result {
            if let Some(respond_to) = respond_to {
                let _ = respond_to.send(Err(err.to_string()));
            }
        }
        result
    }
}
