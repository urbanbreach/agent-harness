use super::*;

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct QuestionRequest {
    questions: Vec<Question>,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Question {
    question: String,
    #[serde(default)]
    header: String,
    options: Vec<QuestionOption>,
    #[serde(default, alias = "multiSelect", alias = "multi_select")]
    multiple: bool,
    #[serde(default = "yes")]
    custom: bool,
}
fn yes() -> bool {
    true
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct QuestionOption {
    label: String,
    #[serde(default)]
    description: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    preview: Option<String>,
}
impl QuestionRequest {
    pub(crate) fn parse(value: Value) -> Result<Self, ToolError> {
        if value.to_string().len() > 65_536 {
            return Err(ToolError::InvalidArguments(
                "questions exceed 64 KiB".into(),
            ));
        }
        let request: Self = serde_json::from_value(value)
            .map_err(|_| ToolError::InvalidArguments("invalid question request".into()))?;
        if request.questions.is_empty()
            || request.questions.len() > 8
            || request.questions.iter().any(|q| {
                q.question.trim().is_empty()
                    || q.question.len() > 4096
                    || q.header.len() > 128
                    || q.options.len() > 32
                    || (q.options.is_empty() && !q.custom)
                    || q.options.iter().enumerate().any(|(i, o)| {
                        o.label.trim().is_empty()
                            || o.label.len() > 256
                            || o.description.len() > 2048
                            || o.preview.as_ref().is_some_and(|p| p.len() > 4096)
                            || q.options[..i].iter().any(|other| other.label == o.label)
                    })
            })
        {
            return Err(ToolError::InvalidArguments(
                "expected 1–8 bounded questions with unique options".into(),
            ));
        }
        Ok(request)
    }
    pub(crate) fn answers(
        &self,
        value: Option<&str>,
    ) -> Result<Value, crate::coord::CoordinatorError> {
        let invalid = || {
            crate::coord::CoordinatorError::Invalid(
                "answers must match the question count, choices, and selection limits".into(),
            )
        };
        let value = value.filter(|s| s.len() <= 65_536).ok_or_else(invalid)?;
        let answers: Vec<Vec<String>> = serde_json::from_str(value).map_err(|_| invalid())?;
        if answers.len() != self.questions.len()
            || answers.iter().zip(&self.questions).any(|(answers, q)| {
                answers.len() > 32
                    || (!q.multiple && answers.len() > 1)
                    || answers.iter().enumerate().any(|(i, a)| {
                        a.trim().is_empty()
                            || a.len() > 4096
                            || answers[..i].contains(a)
                            || (!q.custom && !q.options.iter().any(|o| &o.label == a))
                    })
            })
        {
            return Err(invalid());
        }
        Ok(serde_json::json!({"answers":answers}))
    }
}

pub struct QuestionTool;
#[async_trait::async_trait]
impl Tool for QuestionTool {
    fn id(&self) -> &str {
        "question"
    }
    fn description(&self) -> &str {
        "Ask the user for choices or free-text answers and wait for their response."
    }
    fn capability(&self) -> ToolCapability {
        ToolCapability::ReadFs
    }
    fn parameters_json_schema(&self) -> Value {
        serde_json::json!({"type":"object", "required":["questions"], "additionalProperties":false, "properties":{
            "questions":{"type":"array","minItems":1,"maxItems":8,"items":{
                "type":"object","required":["question","options"],"additionalProperties":false,"properties":{
                    "question":{"type":"string"},"header":{"type":"string"},"multiple":{"type":"boolean"},"custom":{"type":"boolean"},
                    "options":{"type":"array","maxItems":32,"items":{"type":"object","required":["label"],"properties":{
                        "label":{"type":"string"},"description":{"type":"string"},"preview":{"type":"string"}
                    }}}
                }
            }}
        }})
    }
    async fn call(&self, ctx: ToolContext, args: Value) -> Result<ToolResult, ToolError> {
        let request = QuestionRequest::parse(args)?;
        let answers = ctx
            .coordinator
            .wait_for_question(ctx.tool_call_id.to_string(), request)
            .await
            .map_err(|e| ToolError::Execution(e.to_string()))?;
        Ok(ToolResult::structured(answers.to_string(), answers))
    }
}
