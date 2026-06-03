use serde::Deserialize;
use serde_json::Value;

use crate::perm::{PermissionKind, PermissionPolicy, PolicyDecision};
use crate::question_answers::{validate_question_answers, QuestionAnswerPrompt};
use crate::text::non_empty_trimmed;

use super::DEFAULT_QUESTION_TIMEOUT_MS;

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
