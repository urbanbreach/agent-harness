use harness_core::{
    event::EventEnvelopeV1,
    proj::TodoProjection,
    tool::{Tool, ToolCapability, ToolContext, ToolError, ToolResult},
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

pub(crate) enum TodoTool {
    Read,
    Write,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct WriteArgs {
    todos: Vec<Input>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Input {
    #[serde(alias = "text", alias = "title")]
    content: String,
    #[serde(default, alias = "state")]
    status: Option<String>,
    #[serde(default)]
    done: Option<bool>,
    #[serde(default)]
    priority: Option<String>,
    #[serde(default, rename = "id")]
    _id: Option<Value>,
}
#[derive(Serialize)]
struct Todo {
    content: String,
    status: String,
    priority: String,
}

#[async_trait::async_trait]
impl Tool for TodoTool {
    fn id(&self) -> &str {
        match self {
            Self::Read => "todoread",
            Self::Write => "todowrite",
        }
    }
    fn description(&self) -> &str {
        match self {
        Self::Read => "Read the current session's todo list.",
        Self::Write => "Replace the session's todo list. At most one item may be in_progress. An empty list clears it.",
    }
    }
    fn capability(&self) -> ToolCapability {
        ToolCapability::ReadFs
    }
    fn parameters_json_schema(&self) -> Value {
        match self {
            Self::Read => json!({"type":"object","properties":{},"additionalProperties":false}),
            Self::Write => {
                json!({"type":"object","properties":{"todos":{"type":"array","maxItems":100,"items":{"type":"object","properties":{"content":{"type":"string","minLength":1},"status":{"type":"string","enum":["pending","in_progress","completed","cancelled"]},"priority":{"type":"string","enum":["low","medium","high"]}},"required":["content","status","priority"],"additionalProperties":false}}},"required":["todos"],"additionalProperties":false})
            }
        }
    }
    fn permission_requests(&self, _: &Value) -> Vec<(String, String)> {
        let mut requests = vec![("task".into(), self.id().into())];
        if matches!(self, Self::Write) {
            requests.push((self.id().into(), self.id().into()));
        }
        requests
    }
    async fn call(&self, ctx: ToolContext, args: Value) -> Result<ToolResult, ToolError> {
        if ctx.cancellation.is_cancelled() {
            return Err(ToolError::Cancelled);
        }
        let todos = match self {
            Self::Write => {
                let input: WriteArgs = serde_json::from_value(args).map_err(invalid)?;
                let todos: Vec<_> = input
                    .todos
                    .into_iter()
                    .map(|t| Todo {
                        content: t.content,
                        status: t.status.unwrap_or_else(|| {
                            if t.done == Some(true) {
                                "completed"
                            } else {
                                "pending"
                            }
                            .into()
                        }),
                        priority: t.priority.unwrap_or_else(|| "medium".into()),
                    })
                    .collect();
                if todos.len() > 100
                    || todos.iter().filter(|t| t.status == "in_progress").count() > 1
                    || todos.iter().any(|t| {
                        t.content.trim().is_empty()
                            || !["pending", "in_progress", "completed", "cancelled"]
                                .contains(&t.status.as_str())
                            || !["low", "medium", "high"].contains(&t.priority.as_str())
                    })
                {
                    return Err(invalid("todos require valid content, status and priority, at most 100 items, and at most one in_progress item"));
                }
                let value = harness_core::redact::redact_value(
                    ctx.redactor.as_ref(),
                    &serde_json::to_value(todos).map_err(invalid)?,
                );
                if value.to_string().len() > 32 * 1024 {
                    return Err(invalid("todo list exceeds 32 KiB"));
                }
                value
            }
            Self::Read => {
                if args.as_object().is_none_or(|o| !o.is_empty()) {
                    return Err(invalid("todoread takes an empty object"));
                }
                let path = ctx
                    .coordinator
                    .run_info()
                    .await
                    .map_err(invalid)?
                    .events_path;
                tokio::task::spawn_blocking(move || {
                    let length = harness_core::store::open_private_file(&path)?
                        .metadata()?
                        .len();
                    let mut state = TodoProjection::default();
                    for event in
                        harness_core::store::JournalReader::open(&path, length).map_err(invalid)?
                    {
                        if ctx.cancellation.is_cancelled() {
                            return Err(ToolError::Cancelled);
                        }
                        state.apply(&event.map_err(invalid)?);
                    }
                    Ok(state.current())
                })
                .await
                .map_err(invalid)??
            }
        };
        let count = todos.as_array().map_or(0, Vec::len);
        Ok(ToolResult::structured(
            todos.to_string(),
            json!({"todos":todos,"title":format!("{count} todos")}),
        ))
    }
}

pub(crate) fn project(events: &[EventEnvelopeV1]) -> Value {
    TodoProjection::from_events(events).current()
}
fn invalid(error: impl std::fmt::Display) -> ToolError {
    ToolError::InvalidArguments(error.to_string())
}
