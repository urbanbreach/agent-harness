//! Tools for notes-backed context windows: the persistent notebook and the request for a
//! fresh window. The coordinator owns eligibility, redaction and the durable append.
use harness_core::tool::{Tool, ToolCapability, ToolContext, ToolError, ToolResult};
use serde_json::{json, Value};

const NOTES_DESCRIPTION: &str = include_str!("context_notes/context-notes.md");
const NEW_CONTEXT_DESCRIPTION: &str = include_str!("context_notes/new-context.md");
const MISSING: &str = "No context notes are stored for this session branch.";
const SAVED: &str = "Context notes saved.";
const REQUESTED: &str = "New context window requested.";
const NOT_A_STRING: &str = "context_notes text must be a string.";

#[derive(Clone, Copy)]
pub(crate) enum NotesTool {
    Notes,
    NewContext,
}

impl NotesTool {
    fn call_notes(args: &Value) -> Result<Option<&Value>, ToolError> {
        let object = args
            .as_object()
            .ok_or_else(|| invalid("context_notes takes an object"))?;
        if let Some(key) = object.keys().find(|key| *key != "text") {
            return Err(invalid(format!("unknown field `{key}`, expected `text`")));
        }
        Ok(object.get("text"))
    }
}

#[async_trait::async_trait]
impl Tool for NotesTool {
    fn id(&self) -> &str {
        match self {
            Self::Notes => "context_notes",
            Self::NewContext => "new_context",
        }
    }
    fn description(&self) -> &str {
        match self {
            Self::Notes => NOTES_DESCRIPTION.trim_end(),
            Self::NewContext => NEW_CONTEXT_DESCRIPTION.trim_end(),
        }
    }
    fn capability(&self) -> ToolCapability {
        ToolCapability::ReadFs
    }
    fn parameters_json_schema(&self) -> Value {
        match self {
            Self::Notes => json!({
                "type": "object",
                "properties": {"text": {"type": "string"}},
                "additionalProperties": false,
            }),
            Self::NewContext => {
                json!({"type": "object", "properties": {}, "additionalProperties": false})
            }
        }
    }
    /// Replacing or clearing the notebook, and asking for a new window, change the
    /// session and need write approval; reading the notebook does not.
    fn permission_requests(&self, args: &Value) -> Vec<(String, String)> {
        let writes = match self {
            Self::Notes => args.as_object().is_some_and(|o| o.contains_key("text")),
            Self::NewContext => true,
        };
        vec![(
            if writes { "edit" } else { "read" }.into(),
            self.id().into(),
        )]
    }
    async fn call(&self, ctx: ToolContext, args: Value) -> Result<ToolResult, ToolError> {
        if ctx.cancellation.is_cancelled() {
            return Err(ToolError::Cancelled);
        }
        let task = ctx.tool_call_id.to_string();
        match self {
            Self::Notes => match Self::call_notes(&args)? {
                None => {
                    let revision = ctx.coordinator.context_notes_read(task).await?;
                    let text = if revision.entry_id.is_none() {
                        MISSING.to_owned()
                    } else {
                        revision.text.clone()
                    };
                    let mut details = json!({"text": revision.text});
                    if let Some(id) = revision.entry_id {
                        details["entry_id"] = json!(id);
                    }
                    Ok(ToolResult::structured(text, details))
                }
                Some(text) => {
                    // Eligibility comes first, then the type and size of what was sent.
                    let ticket = ctx.coordinator.context_notes_ticket(task.clone()).await?;
                    let Some(text) = text.as_str() else {
                        return Err(ToolError::Execution(NOT_A_STRING.into()));
                    };
                    if ctx.cancellation.is_cancelled() {
                        return Err(ToolError::Cancelled);
                    }
                    let saved = ctx
                        .coordinator
                        .context_notes_save(task, ticket, text.to_owned())
                        .await?;
                    Ok(ToolResult::structured(
                        SAVED,
                        json!({"entry_id": saved.entry_id, "text": saved.text, "bytes": saved.bytes}),
                    ))
                }
            },
            Self::NewContext => {
                if args.as_object().is_none_or(|object| !object.is_empty()) {
                    return Err(invalid("new_context takes an empty object"));
                }
                // Nothing is recorded before eligibility and a last abort check pass.
                if ctx.cancellation.is_cancelled() {
                    return Err(ToolError::Cancelled);
                }
                ctx.coordinator.new_context_request(task).await?;
                Ok(ToolResult::structured(
                    REQUESTED,
                    json!({"requested": true}),
                ))
            }
        }
    }
}

fn invalid(message: impl Into<String>) -> ToolError {
    ToolError::InvalidArguments(message.into())
}
