use std::sync::Arc;

use async_trait::async_trait;
use harness_core::event::{ActorKind, EventActor};
use harness_core::plan::{plan_file_display_path, BUILD_AGENT_NAME, PLAN_AGENT_NAME};
use harness_core::tool::{Tool, ToolCapability, ToolContext, ToolError, ToolResult};
use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::{json, Value};

use crate::question_env::{
    coordinator_question_answer_source, question_answers_from_source_or_request,
    QuestionAnswerSource,
};

const PLAN_EXIT_YES: &str = "Yes";
const PLAN_EXIT_NO: &str = "No";

pub(crate) struct PlanExitTool {
    question_answer_source: Arc<dyn QuestionAnswerSource>,
}
pub(crate) struct PlanEnterTool {
    question_answer_source: Arc<dyn QuestionAnswerSource>,
}

impl PlanEnterTool {
    pub(crate) fn new(question_answer_source: Arc<dyn QuestionAnswerSource>) -> Self {
        Self {
            question_answer_source,
        }
    }
}

impl PlanExitTool {
    pub(crate) fn new(question_answer_source: Arc<dyn QuestionAnswerSource>) -> Self {
        Self {
            question_answer_source,
        }
    }
}

impl Default for PlanEnterTool {
    fn default() -> Self {
        Self::new(coordinator_question_answer_source())
    }
}

impl Default for PlanExitTool {
    fn default() -> Self {
        Self::new(coordinator_question_answer_source())
    }
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct PlanEnterArgs {
    goal: Option<String>,
    reason: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
#[allow(
    clippy::empty_structs_with_brackets,
    reason = "unit struct changes JSON schema representation"
)]
struct PlanExitArgs {}

#[async_trait]
impl Tool for PlanEnterTool {
    fn id(&self) -> &str {
        harness_core::plan::PLAN_ENTER_TOOL_ID
    }

    fn description(&self) -> &str {
        "Suggest switching to the plan agent when the user's request would benefit from planning before implementation. Optionally pass the original user goal."
    }

    fn parameters_json_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "goal": {
                    "type": "string",
                    "description": "The user's original request or goal to plan before implementation."
                },
                "reason": {
                    "type": "string",
                    "description": "Why this request would benefit from Plan mode."
                }
            },
            "required": [],
            "additionalProperties": false
        })
    }

    fn capability(&self) -> ToolCapability {
        ToolCapability::SpawnAgent
    }

    async fn call(&self, ctx: ToolContext, args_json: Value) -> Result<ToolResult, ToolError> {
        let args: PlanEnterArgs = crate::parse_tool_args(args_json)?;
        execute_plan_enter(ctx, args, self.question_answer_source.as_ref()).await
    }
}

#[async_trait]
impl Tool for PlanExitTool {
    fn id(&self) -> &str {
        harness_core::plan::PLAN_EXIT_TOOL_ID
    }

    fn description(&self) -> &str {
        "Completes plan mode by asking the user whether to switch to the build agent and execute the active plan. Takes no arguments."
    }

    fn parameters_json_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {},
            "required": [],
            "additionalProperties": false
        })
    }

    fn capability(&self) -> ToolCapability {
        ToolCapability::SpawnAgent
    }

    async fn call(&self, ctx: ToolContext, args_json: Value) -> Result<ToolResult, ToolError> {
        let _args: PlanExitArgs = crate::parse_tool_args(args_json)?;
        execute_plan_exit(ctx, self.question_answer_source.as_ref()).await
    }
}

async fn execute_plan_enter(
    ctx: ToolContext,
    args: PlanEnterArgs,
    question_answer_source: &dyn QuestionAnswerSource,
) -> Result<ToolResult, ToolError> {
    let goal = args
        .goal
        .as_deref()
        .map(str::trim)
        .filter(|goal| !goal.is_empty())
        .unwrap_or("the current user request");
    let approved =
        request_plan_enter_approval(&ctx, goal, args.reason.as_deref(), question_answer_source)
            .await?;
    if !approved {
        return Ok(crate::text_json_tool_result(
            "Staying with build agent",
            json!({
                "agent": BUILD_AGENT_NAME,
                "goal": goal,
                "approved": false,
            }),
        ));
    }

    let supervisor = EventActor::new(ActorKind::Supervisor, None);
    let plan_agent_id = ctx
        .coordinator
        .spawn_agent_idle(
            supervisor.clone(),
            PLAN_AGENT_NAME,
            ctx.actor.agent_id.clone(),
        )
        .await
        .map_err(|err| ToolError::Execution(format!("failed to switch to plan agent: {err}")))?;
    let plan_file = plan_file_display_path(&ctx.run_id);
    let prompt = format!(
        "<system-reminder>\nYour operational mode has changed from build to plan.\nThe user approved switching to the plan agent before implementation. Do not edit implementation files yet.\nOriginal goal to plan: {goal}\nCreate or update the active plan file at {plan_file}, then call `plan_exit` when the plan is ready for approval.\n</system-reminder>"
    );
    let request_id = ctx
        .coordinator
        .request_agent_turn(supervisor, plan_agent_id.clone(), prompt)
        .await
        .map_err(|err| {
            ToolError::Execution(format!("failed to schedule plan agent continuation: {err}"))
        })?;

    Ok(crate::text_json_tool_result(
        "Switching to plan agent",
        json!({
            "agent": PLAN_AGENT_NAME,
            "plan_agent_id": plan_agent_id,
            "request_id": request_id,
            "plan_file": plan_file,
            "goal": goal,
            "approved": true,
        }),
    ))
}

async fn execute_plan_exit(
    ctx: ToolContext,
    question_answer_source: &dyn QuestionAnswerSource,
) -> Result<ToolResult, ToolError> {
    let plan_file = plan_file_display_path(&ctx.run_id);
    let approved = request_plan_exit_approval(&ctx, &plan_file, question_answer_source).await?;
    if !approved {
        return Ok(crate::text_json_tool_result(
            "Staying in plan mode",
            json!({
                "agent": harness_core::plan::PLAN_AGENT_NAME,
                "plan_file": plan_file,
                "approved": false,
            }),
        ));
    }

    let supervisor = EventActor::new(ActorKind::Supervisor, None);
    let build_agent_id = ctx
        .coordinator
        .spawn_agent_idle(
            supervisor.clone(),
            BUILD_AGENT_NAME,
            ctx.actor.agent_id.clone(),
        )
        .await
        .map_err(|err| ToolError::Execution(format!("failed to switch to build agent: {err}")))?;
    let prompt = format!(
        "<system-reminder>\nYour operational mode has changed from plan to build.\nThe plan at {plan_file} has been approved, and you can now edit files. Execute the plan defined within it.\nYou are no longer in plan-mode read-only restrictions.\nUse the active build profile's available tools and permission policy; file edits and shell commands are allowed only when that profile exposes and permits those tools.\n</system-reminder>"
    );
    let request_id = ctx
        .coordinator
        .request_agent_turn(supervisor, build_agent_id.clone(), prompt)
        .await
        .map_err(|err| {
            ToolError::Execution(format!(
                "failed to schedule build agent continuation: {err}"
            ))
        })?;

    Ok(crate::text_json_tool_result(
        "Switching to build agent",
        json!({
            "agent": BUILD_AGENT_NAME,
            "build_agent_id": build_agent_id,
            "request_id": request_id,
            "plan_file": plan_file,
            "approved": true,
        }),
    ))
}

async fn request_plan_enter_approval(
    ctx: &ToolContext,
    goal: &str,
    reason: Option<&str>,
    question_answer_source: &dyn QuestionAnswerSource,
) -> Result<bool, ToolError> {
    let reason_text = reason
        .map(str::trim)
        .filter(|reason| !reason.is_empty())
        .map(|reason| format!(" Reason: {reason}"))
        .unwrap_or_default();
    let questions = json!({
        "questions": [{
            "question": format!("This request may benefit from Plan mode before implementation.{reason_text} Switch to the plan agent to plan `{goal}`?"),
            "header": "Plan Agent",
            "options": [
                { "label": PLAN_EXIT_YES, "description": "Switch to plan agent and create a plan before implementing" },
                { "label": PLAN_EXIT_NO, "description": "Stay with build agent and continue without switching" }
            ]
        }]
    });

    let answers = question_answers_from_source_or_request(
        question_answer_source,
        ctx,
        questions,
        ToolError::Execution,
    )
    .await?;
    let answer = answers
        .first()
        .and_then(|group| group.first())
        .map(|answer| answer.trim())
        .unwrap_or("");

    match answer {
        PLAN_EXIT_YES => Ok(true),
        PLAN_EXIT_NO | "" => Ok(false),
        other => Err(ToolError::Execution(format!(
            "invalid plan_enter answer `{other}`; expected `{PLAN_EXIT_YES}` or `{PLAN_EXIT_NO}`"
        ))),
    }
}

async fn request_plan_exit_approval(
    ctx: &ToolContext,
    plan_file: &str,
    question_answer_source: &dyn QuestionAnswerSource,
) -> Result<bool, ToolError> {
    let questions = json!({
        "questions": [{
            "question": format!("Plan at {plan_file} is complete. Would you like to switch to the build agent and start implementing?"),
            "header": "Build Agent",
            "options": [
                { "label": PLAN_EXIT_YES, "description": "Switch to build agent and start implementing the plan" },
                { "label": PLAN_EXIT_NO, "description": "Stay with plan agent to continue refining the plan" }
            ]
        }]
    });

    let answers = question_answers_from_source_or_request(
        question_answer_source,
        ctx,
        questions,
        ToolError::Execution,
    )
    .await?;
    let answer = answers
        .first()
        .and_then(|group| group.first())
        .map(|answer| answer.trim())
        .unwrap_or("");

    match answer {
        PLAN_EXIT_YES => Ok(true),
        PLAN_EXIT_NO | "" => Ok(false),
        other => Err(ToolError::Execution(format!(
            "invalid plan_exit answer `{other}`; expected `{PLAN_EXIT_YES}` or `{PLAN_EXIT_NO}`"
        ))),
    }
}
