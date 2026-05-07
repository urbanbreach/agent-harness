use harness_core::tool::{ToolContext, ToolError};
use serde_json::Value;

const QUESTION_ANSWERS_ENV_VAR: &str = "HARNESS_QUESTION_ANSWERS";

pub(crate) fn read_question_answers_from_env() -> Result<Option<Vec<Vec<String>>>, ToolError> {
    std::env::var(QUESTION_ANSWERS_ENV_VAR)
        .ok()
        .map(|value| serde_json::from_str::<Vec<Vec<String>>>(&value))
        .transpose()
        .map_err(|err| {
            ToolError::Execution(format!("failed to parse {QUESTION_ANSWERS_ENV_VAR}: {err}"))
        })
}

pub(crate) async fn question_answers_from_env_or_request(
    ctx: &ToolContext,
    questions: Value,
    coordinator_error: impl FnOnce(String) -> ToolError,
) -> Result<Vec<Vec<String>>, ToolError> {
    if let Some(answers) = read_question_answers_from_env()? {
        return Ok(answers);
    }

    ctx.coordinator
        .request_question(ctx.actor.clone(), ctx.tool_call_id.clone(), questions)
        .await
        .map_err(coordinator_error)
}
