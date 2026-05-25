use std::sync::Arc;

use harness_core::tool::{ToolContext, ToolError};
use serde_json::Value;

pub trait QuestionAnswerSource: Send + Sync {
    fn answers(&self) -> Option<Vec<Vec<String>>>;
}

#[derive(Debug, Default)]
pub struct CoordinatorQuestionAnswerSource;

impl QuestionAnswerSource for CoordinatorQuestionAnswerSource {
    fn answers(&self) -> Option<Vec<Vec<String>>> {
        None
    }
}

#[derive(Debug, Clone)]
pub struct ScriptedQuestionAnswerSource {
    answers: Vec<Vec<String>>,
}

impl ScriptedQuestionAnswerSource {
    pub fn new(answers: Vec<Vec<String>>) -> Self {
        Self { answers }
    }
}

impl QuestionAnswerSource for ScriptedQuestionAnswerSource {
    fn answers(&self) -> Option<Vec<Vec<String>>> {
        Some(self.answers.clone())
    }
}

pub(crate) fn coordinator_question_answer_source() -> Arc<dyn QuestionAnswerSource> {
    Arc::new(CoordinatorQuestionAnswerSource)
}

pub(crate) async fn question_answers_from_source_or_request(
    source: &dyn QuestionAnswerSource,
    ctx: &ToolContext,
    questions: Value,
    coordinator_error: impl FnOnce(String) -> ToolError,
) -> Result<Vec<Vec<String>>, ToolError> {
    if let Some(answers) = source.answers() {
        return Ok(answers);
    }

    ctx.coordinator
        .request_question(ctx.actor.clone(), ctx.tool_call_id.clone(), questions)
        .await
        .map_err(coordinator_error)
}
