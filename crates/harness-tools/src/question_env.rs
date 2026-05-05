use harness_core::tool::ToolError;

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
