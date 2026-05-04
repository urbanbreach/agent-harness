use async_trait::async_trait;
use harness_core::event::{ActorKind, EventActor};
use harness_core::plan::{plan_file_display_path, BUILD_AGENT_NAME};
use harness_core::tool::{Tool, ToolCapability, ToolContext, ToolError, ToolResult};
use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::{json, Value};

const QUESTION_ANSWERS_ENV_VAR: &str = "HARNESS_QUESTION_ANSWERS";
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
        let _args: PlanExitArgs = serde_json::from_value(args_json)
            .map_err(|err| ToolError::InvalidArguments(err.to_string()))?;
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
        "The plan at {plan_file} has been approved, you can now edit files. Execute the plan"
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

    Ok(ToolResult {
        display_text: "Switching to build agent".to_string(),
        structured_json: Some(json!({
            "agent": BUILD_AGENT_NAME,
            "build_agent_id": build_agent_id,
            "request_id": request_id,
            "plan_file": plan_file,
            "approved": true,
        })),
        artifacts: Vec::new(),
    })
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

    let answers = match read_question_answers_from_env()? {
        Some(answers) => answers,
        None => ctx
            .coordinator
            .request_question(ctx.actor.clone(), ctx.tool_call_id.clone(), questions)
            .await
            .map_err(ToolError::Execution)?,
    };
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

fn read_question_answers_from_env() -> Result<Option<Vec<Vec<String>>>, ToolError> {
    std::env::var(QUESTION_ANSWERS_ENV_VAR)
        .ok()
        .map(|value| serde_json::from_str::<Vec<Vec<String>>>(&value))
        .transpose()
        .map_err(|err| {
            ToolError::Execution(format!("failed to parse {QUESTION_ANSWERS_ENV_VAR}: {err}"))
        })
}
