use async_trait::async_trait;
use harness_core::event::{ActorKind, EventActor};
use harness_core::plan::{plan_file_display_path, BUILD_AGENT_NAME};
use harness_core::tool::{Tool, ToolCapability, ToolContext, ToolError, ToolResult};
use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::{json, Value};

use crate::question_env::question_answers_from_env_or_request;

const PLAN_EXIT_YES: &str = "Yes";
const PLAN_EXIT_NO: &str = "No";

pub(crate) struct PlanExitTool;

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct PlanExitArgs {}

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
        execute_plan_exit(ctx).await
    }
}

async fn execute_plan_exit(ctx: ToolContext) -> Result<ToolResult, ToolError> {
    let plan_file = plan_file_display_path(&ctx.run_id);
    let approved = request_plan_exit_approval(&ctx, &plan_file).await?;
    if !approved {
        return Err(ToolError::Execution(
            "Plan exit cancelled by user confirmation".to_string(),
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
        "<system-reminder>\nYour operational mode has changed from plan to build.\nYou are no longer in plan-mode read-only restrictions.\nUse the active build profile's available tools and permission policy to execute the approved plan; file edits and shell commands are allowed when that profile exposes and permits those tools.\n</system-reminder>\n\nA plan file exists at {plan_file}. You should execute on the plan defined within it"
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

async fn request_plan_exit_approval(ctx: &ToolContext, plan_file: &str) -> Result<bool, ToolError> {
    let questions = json!({
        "questions": [{
            "question": format!("Plan at {plan_file} is complete. Would you like to switch to the build agent and start implementing?"),
            "header": "Build Agent",
            "options": [
                { "label": PLAN_EXIT_YES, "description": "Switch to build agent and start implementing the plan" },
                { "label": PLAN_EXIT_NO, "description": "Stay in plan mode" }
            ]
        }]
    });

    let answers =
        question_answers_from_env_or_request(ctx, questions, ToolError::Execution).await?;
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
