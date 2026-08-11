// allow: SIZE_OK — agent operations executor (task spawn + background cancel + skill resolution + continuation)
use std::sync::Arc;

use harness_core::coord::{AgentRuntimeInfo, ChildTaskRequestMetadata, CoordinatorError};
use harness_core::event::{ActorKind, EventActor};
use harness_core::tool::{ToolContext, ToolError, ToolResult};
use serde_json::json;

use crate::control_plane::{
    render_task_skill_context, resolve_task_skill_context, TaskSkillContext,
};
use crate::question_env::QuestionAnswerSource;
use crate::text_json_tool_result;

mod background;
mod batch;
mod child_metadata;

pub(crate) use background::{BackgroundCancelRequest, BackgroundOutputRequest};
pub(crate) use batch::BatchCall;

use child_metadata::{
    child_permission_metadata, child_runtime_metadata, child_session_observability,
    spawn_result_json, wait_for_request_completion, ChildRequestObservability, ChildToolCallCounts,
};

pub(crate) struct AgentOpsExecutor {
    question_answer_source: Arc<dyn QuestionAnswerSource>,
}

impl AgentOpsExecutor {
    #[cfg(test)]
    pub(crate) fn new() -> Self {
        Self::with_question_answer_source(crate::question_env::coordinator_question_answer_source())
    }

    pub(crate) fn with_question_answer_source(
        question_answer_source: Arc<dyn QuestionAnswerSource>,
    ) -> Self {
        Self {
            question_answer_source,
        }
    }

    pub(crate) async fn spawn_agent(
        &self,
        ctx: &ToolContext,
        request: AgentSpawnRequest,
    ) -> Result<ToolResult, ToolError> {
        let loaded_skills = resolve_task_skill_context(
            ctx,
            &request.load_skills,
            self.question_answer_source.as_ref(),
        )
        .await?;

        let supervisor = EventActor::new(ActorKind::Supervisor, None);
        let existing_session_id = request.session_id.clone().or(request.task_id.clone());
        let resumed_existing_session = existing_session_id.is_some();
        let (agent_id, runtime) = if let Some(session_id) = existing_session_id {
            let target_info = ctx
                .coordinator
                .agent_runtime_info(session_id.clone())
                .await
                .map_err(|err| map_request_agent_turn_error(err, &request))?;
            authorize_existing_child_session(ctx, &request, &target_info)?;
            (session_id, target_info)
        } else {
            let agent_id = spawn_new_child_agent(ctx, &request, supervisor.clone()).await?;
            let runtime = ctx
                .coordinator
                .agent_runtime_info(agent_id.clone())
                .await
                .map_err(|err| map_request_agent_turn_error(err, &request))?;
            (agent_id, runtime)
        };
        let model_override = inherited_model_override(ctx, &runtime);
        let request_id = ctx
            .coordinator
            .request_child_agent_turn_with_model(
                supervisor,
                agent_id.clone(),
                build_child_prompt(&request, &loaded_skills),
                model_override
                    .as_ref()
                    .map(|(model_ref, _)| model_ref.clone()),
                model_override.map(|(_, settings)| settings),
                ChildTaskRequestMetadata {
                    parent_tool_call_id: ctx.tool_call_id.to_string(),
                    parent_session_id: ctx.run_id.as_str().into(),
                    parent_agent_id: ctx.actor.agent_id.clone(),
                    child_session_id: agent_id.clone().into(),
                    task_id: agent_id.clone().into(),
                    description: request.description.clone(),
                    run_in_background: request.run_in_background,
                },
            )
            .await
            .map_err(|err| map_request_agent_turn_error(err, &request))?;

        let lineage = json!({
            "parent_tool_call_id": ctx.tool_call_id.clone(),
            "parent_session_id": ctx.run_id.clone(),
            "child_session_id": agent_id.clone(),
            "child_request_id": request_id.clone(),
        });
        let permissions = child_permission_metadata(ctx, &runtime);

        if request.run_in_background {
            let child_session = child_session_observability(
                &agent_id,
                &request_id,
                &request,
                resumed_existing_session,
                &permissions,
                child_runtime_metadata(&runtime),
                ChildRequestObservability {
                    status: "scheduled",
                    duration_ms: None,
                    result_summary: None,
                    failure_summary: None,
                    child_summary: None,
                    tool_calls: ChildToolCallCounts::default(),
                },
            );
            return Ok(text_json_tool_result(
                format!(
                    "task_id: {agent_id} (for resuming to continue this task if needed)\nrequest_id: {request_id}\n\n<task_result>Background task scheduled.</task_result>"
                ),
                spawn_result_json(
                    &request,
                    &loaded_skills,
                    &agent_id,
                    &request_id,
                    lineage.clone(),
                    &child_session,
                ),
            ));
        }

        let child_observability = wait_for_request_completion(ctx, &request_id).await?;
        let child_session = child_session_observability(
            &agent_id,
            &request_id,
            &request,
            resumed_existing_session,
            &permissions,
            child_runtime_metadata(&runtime),
            child_observability,
        );
        let task_result = child_session
            .result_summary
            .clone()
            .or_else(|| child_session.failure_summary.clone())
            .unwrap_or_else(|| match child_session.status {
                "timed_out" => {
                    format!("timed out waiting for child session request {request_id}")
                }
                _ => "Child session finished without a summary.".to_string(),
            });

        Ok(text_json_tool_result(
            format!(
                "task_id: {agent_id} (for resuming to continue this task if needed)\nrequest_id: {request_id}\n\n<task_result>\n{}\n</task_result>",
                task_result
            ),
            spawn_result_json(
                &request,
                &loaded_skills,
                &agent_id,
                &request_id,
                lineage,
                &child_session,
            ),
        ))
    }

    pub(crate) async fn execute_batch(
        &self,
        ctx: &ToolContext,
        calls: Vec<BatchCall>,
    ) -> Result<ToolResult, ToolError> {
        batch::execute_batch(ctx, calls).await
    }

    pub(crate) async fn background_output(
        &self,
        ctx: &ToolContext,
        request: BackgroundOutputRequest,
    ) -> Result<ToolResult, ToolError> {
        background::background_output(ctx, request).await
    }

    pub(crate) async fn background_cancel(
        &self,
        ctx: &ToolContext,
        request: BackgroundCancelRequest,
    ) -> Result<ToolResult, ToolError> {
        background::background_cancel(ctx, request).await
    }

    pub(crate) async fn cancel_all_background_tasks(
        &self,
        ctx: &ToolContext,
        reason: Option<String>,
    ) -> Result<ToolResult, ToolError> {
        background::cancel_all_background_tasks(ctx, reason).await
    }
}

#[derive(Debug, Clone)]
pub(crate) struct AgentSpawnRequest {
    pub(crate) description: String,
    pub(crate) profile_name: String,
    pub(crate) prompt: String,
    pub(crate) task_id: Option<String>,
    pub(crate) session_id: Option<String>,
    pub(crate) run_in_background: bool,
    pub(crate) load_skills: Vec<String>,
    pub(crate) command: Option<String>,
}

fn build_child_prompt(request: &AgentSpawnRequest, loaded_skills: &[TaskSkillContext]) -> String {
    if loaded_skills.is_empty() && request.command.is_none() {
        return request.prompt.clone();
    }

    let mut prompt = String::from("Delegation context from parent:\n");
    if !loaded_skills.is_empty() {
        prompt.push_str("- Loaded skills: ");
        prompt.push_str(
            &loaded_skills
                .iter()
                .map(|skill| skill.name.as_str())
                .collect::<Vec<_>>()
                .join(", "),
        );
        prompt.push_str("\n\n");
        for skill in loaded_skills {
            prompt.push_str(&render_task_skill_context(skill));
            prompt.push_str("\n\n");
        }
    }
    if let Some(command) = request.command.as_deref() {
        prompt.push_str("- Treat this command as required execution context: ");
        prompt.push_str(command);
        prompt.push('\n');
    }
    prompt.push_str("\nTask:\n");
    prompt.push_str(&request.prompt);
    prompt
}

fn authorize_existing_child_session(
    ctx: &ToolContext,
    request: &AgentSpawnRequest,
    target_info: &AgentRuntimeInfo,
) -> Result<(), ToolError> {
    let parent_agent_id = ctx.actor.agent_id.as_deref().ok_or_else(|| {
        ToolError::InvalidArguments(
            "task session re-entry requires a worker-owned parent agent".to_string(),
        )
    })?;

    if target_info.parent_agent_id.as_deref() != Some(parent_agent_id) {
        return Err(ToolError::InvalidArguments(format!(
            "task session `{}` is not a direct child of the calling agent",
            target_info.agent_id
        )));
    }

    if target_info.profile_name != request.profile_name {
        return Err(ToolError::InvalidArguments(format!(
            "task session `{}` uses profile `{}`, but the request selected `{}`",
            target_info.agent_id, target_info.profile_name, request.profile_name
        )));
    }

    Ok(())
}

async fn spawn_new_child_agent(
    ctx: &ToolContext,
    request: &AgentSpawnRequest,
    supervisor: EventActor,
) -> Result<String, ToolError> {
    spawn_child_agent_once(ctx, request, supervisor)
        .await
        .map_err(map_spawn_agent_error)
}

async fn spawn_child_agent_once(
    ctx: &ToolContext,
    request: &AgentSpawnRequest,
    supervisor: EventActor,
) -> Result<String, CoordinatorError> {
    ctx.coordinator
        .spawn_agent_idle_with_child_title(
            supervisor,
            request.profile_name.clone(),
            ctx.actor.agent_id.clone(),
            format!(
                "{} (@{} subagent)",
                request.description, request.profile_name
            ),
        )
        .await
}

fn inherited_model_override(
    ctx: &ToolContext,
    runtime: &AgentRuntimeInfo,
) -> Option<(String, harness_core::agent::AgentModelSettings)> {
    if runtime.model_ref_explicit {
        return None;
    }

    Some((
        ctx.current_model_ref.clone()?,
        ctx.current_model_settings.clone().unwrap_or_default(),
    ))
}

fn map_request_agent_turn_error(err: CoordinatorError, request: &AgentSpawnRequest) -> ToolError {
    match err {
        CoordinatorError::UnknownAgent(agent_id)
            if request.session_id.is_some() || request.task_id.is_some() =>
        {
            let message = format!(
                "Unknown child session `{agent_id}`. Provide a `session_id`/`task_id` returned by a prior `task` call, or omit it to start a new child session."
            );
            ToolError::InvalidArguments(message)
        }
        other => ToolError::Execution(format!("failed to request agent turn: {other}")),
    }
}

fn map_spawn_agent_error(err: CoordinatorError) -> ToolError {
    match err {
        CoordinatorError::UnknownAgent(profile_name) => ToolError::InvalidArguments(format!(
            "Unknown child profile `{profile_name}`. Use one of the configured task subagents: `explore`, `general`, or `librarian`."
        )),
        other => ToolError::Execution(format!("failed to spawn agent: {other}")),
    }
}

#[cfg(test)]
mod tests {
    use super::{
        build_child_prompt, map_request_agent_turn_error, map_spawn_agent_error, AgentSpawnRequest,
    };
    use crate::UnwrapOrAbort;
    use harness_core::coord::CoordinatorError;
    use harness_core::tool::ToolError;
    use std::path::PathBuf;

    use crate::control_plane::TaskSkillContext;
    use crate::skill_catalog::{SkillCatalogEntry, SkillCatalogStatus};

    #[test]
    fn child_prompt_preserves_v1_delegation_skill_command_task_precedence() {
        // arrange
        let request = AgentSpawnRequest {
            description: "spawn child".to_string(),
            profile_name: "general".to_string(),
            prompt: "Task body.".to_string(),
            task_id: None,
            session_id: None,
            run_in_background: false,
            load_skills: vec!["rust-best-practices".to_string()],
            command: Some("/issue-delivery".to_string()),
        };
        let loaded_skills = vec![TaskSkillContext {
            name: "rust-best-practices".to_string(),
            description: "Rust guidance".to_string(),
            content: "Use focused diffs.".to_string(),
            location: PathBuf::from(".agents/skills/rust-best-practices/SKILL.md"),
            metadata: SkillCatalogEntry {
                stable_id: "skill:project:rust-best-practices".to_string(),
                name: "rust-best-practices".to_string(),
                description: "Rust guidance".to_string(),
                source_scope: "project".to_string(),
                root_path: PathBuf::from(".agents/skills"),
                location: PathBuf::from(".agents/skills/rust-best-practices/SKILL.md"),
                loadable: true,
                permission_mode: "allow".to_string(),
                status: SkillCatalogStatus::Loadable,
                reason: None,
                argument_hint: None,
                allowed_tools: Vec::new(),
                deferred_mcp: None,
                deferred_resources: None,
                body_loaded: false,
                body_digest: None,
            },
        }];

        // act
        let prompt = build_child_prompt(&request, &loaded_skills);

        // assert
        assert_prompt_order(
            &prompt,
            "Delegation context from parent:",
            "- Loaded skills:",
        );
        assert_prompt_order(
            &prompt,
            "- Loaded skills:",
            "<skill_content name=\"rust-best-practices\">",
        );
        assert_prompt_order(
            &prompt,
            "<skill_content name=\"rust-best-practices\">",
            "- Treat this command as required execution context: /issue-delivery",
        );
        assert_prompt_order(
            &prompt,
            "- Treat this command as required execution context: /issue-delivery",
            "\nTask:\nTask body.",
        );
    }

    #[test]
    fn unknown_existing_session_returns_guidance() {
        // arrange
        // act
        // assert
        let err = map_request_agent_turn_error(
            CoordinatorError::UnknownAgent("missing-session".to_string()),
            &AgentSpawnRequest {
                description: "resume child".to_string(),
                profile_name: "general".to_string(),
                prompt: "resume".to_string(),
                task_id: Some("missing-session".to_string()),
                session_id: None,
                run_in_background: false,
                load_skills: Vec::new(),
                command: None,
            },
        );
        assert!(
            matches!(err, ToolError::InvalidArguments(message) if message.contains("Unknown child session `missing-session`") && message.contains("start a new child session"))
        );
    }

    #[test]
    fn unknown_child_profile_returns_guidance() {
        // arrange
        // act
        // assert
        let err = map_spawn_agent_error(CoordinatorError::UnknownAgent("general".to_string()));
        assert!(
            matches!(err, ToolError::InvalidArguments(message) if message.contains("Unknown child profile `general`") && message.contains("`explore`, `general`, or `librarian`"))
        );
    }

    fn assert_prompt_order(prompt: &str, before: &str, after: &str) {
        let before_index = prompt
            .find(before)
            .unwrap_or_else(|| panic!("child prompt missing {before:?}"));
        let after_index = prompt
            .find(after)
            .unwrap_or_else(|| panic!("child prompt missing {after:?}"));
        assert!(
            before_index < after_index,
            "expected {before:?} before {after:?} in child prompt"
        );
    }
}
