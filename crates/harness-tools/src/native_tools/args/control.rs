// allow: SIZE_OK — control tool arguments (background output timeout + cancel + task continuation params)
use crate::agent_ops::BatchCall;
use crate::control_plane::{QuestionPrompt, TodoItem};
use crate::text::trimmed_non_empty;
use schemars::JsonSchema;
use serde::de::Error as _;
use serde::{Deserialize, Deserializer};
use serde_json::Value;

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(in crate::native_tools) struct TodoWriteArgs {
    #[schemars(
        description = "Complete replacement todo list. Keep exactly one item in_progress and use pending, completed, or cancelled for the rest."
    )]
    pub(in crate::native_tools) todos: Vec<TodoItem>,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(in crate::native_tools) struct TaskArgs {
    #[schemars(
        description = "Optional short label for the delegated work, used in child session titles and status output. When omitted, a short label is auto-generated from the first few words of the prompt."
    )]
    #[serde(default)]
    pub(in crate::native_tools) description: Option<String>,
    #[schemars(
        description = "Task body delivered to the child. For non-trivial delegation, include context, goal, downstream use, request, required tools, must-do, and must-not-do sections."
    )]
    pub(in crate::native_tools) prompt: String,
    #[schemars(
        description = "Required named child profile. Must be one of `explore`, `general`, or `librarian`."
    )]
    pub(in crate::native_tools) subagent_type: TaskSubagentType,
    #[schemars(
        description = "Compatibility selector for continuing an existing child task/session when provided by prior task output."
    )]
    pub(in crate::native_tools) task_id: Option<String>,
    #[schemars(
        description = "Compatibility selector for continuing an existing child session when provided by prior task output."
    )]
    pub(in crate::native_tools) session_id: Option<String>,
    #[schemars(
        description = "Required execution choice. false waits synchronously; true returns request_id/task_id immediately. Use background_output for interim status checks, or cancel=true anytime, but wait for the coordinator/system completion notification before final result retrieval."
    )]
    pub(in crate::native_tools) run_in_background: bool,
    #[schemars(
        description = "Required list of skills to load before child spawn. Pass [] when no skills are needed."
    )]
    pub(in crate::native_tools) load_skills: Vec<String>,
    #[schemars(
        description = "Optional command/context string prepended to the child prompt as required delegation context."
    )]
    pub(in crate::native_tools) command: Option<String>,
}

#[derive(Debug, Clone, Copy, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub(in crate::native_tools) enum TaskSubagentType {
    Explore,
    General,
    Librarian,
}

impl TaskSubagentType {
    pub(in crate::native_tools) const fn as_str(self) -> &'static str {
        match self {
            Self::Explore => "explore",
            Self::General => "general",
            Self::Librarian => "librarian",
        }
    }
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(in crate::native_tools) struct BackgroundOutputArgs {
    #[schemars(
        description = "Compatibility selector from older task output. Prefer request_id for new calls."
    )]
    #[serde(default)]
    pub(in crate::native_tools) task_id: Option<String>,
    #[schemars(
        description = "Compatibility selector for the child session. Prefer request_id for background result retrieval."
    )]
    #[serde(default)]
    pub(in crate::native_tools) session_id: Option<String>,
    #[schemars(
        description = "Canonical background request identifier returned by task(run_in_background=true); use it for interim status checks and final result retrieval after the coordinator/system completion notification."
    )]
    #[serde(default)]
    pub(in crate::native_tools) request_id: Option<String>,
    #[schemars(
        description = "Optional list of background request_ids for multi-wait. When two or more ids are provided, wait_mode is required (any|all) and block waits until the wait condition is satisfied or timeout expires."
    )]
    #[serde(default)]
    pub(in crate::native_tools) request_ids: Vec<String>,
    #[schemars(
        description = "Multi-wait mode when request_ids has two or more entries: `any` returns when the first watched request is terminal; `all` returns when every watched request is terminal. Cancelled and completed both count as terminal."
    )]
    #[serde(default)]
    pub(in crate::native_tools) wait_mode: Option<String>,
    #[schemars(
        description = "When true, wait until the background request reaches a terminal state or timeout expires. Use only for interim status checks unless the coordinator/system completion notification has arrived."
    )]
    #[serde(default)]
    pub(in crate::native_tools) block: bool,
    #[schemars(description = "Maximum time to wait in milliseconds when block=true.")]
    #[serde(default = "default_background_output_timeout_ms", alias = "timeout_ms")]
    pub(in crate::native_tools) timeout: u64,
    #[schemars(
        description = "When true, request cancellation for a non-terminal child before returning status; cancel=true is allowed anytime."
    )]
    #[serde(default)]
    pub(in crate::native_tools) cancel: bool,
    #[schemars(description = "Optional cancellation reason recorded when cancel=true succeeds.")]
    #[serde(default)]
    pub(in crate::native_tools) reason: Option<String>,
    #[schemars(
        description = "When true, read the child session events.jsonl and return the complete event stream as structured JSON. Spills to artifact if output exceeds the inline char threshold."
    )]
    #[serde(default)]
    pub(in crate::native_tools) full_session: bool,
    #[schemars(
        description = "When true, extract ProviderReasoningDelta events from the child session. Thinking content is spilled to an artifact; only the artifact reference is returned inline."
    )]
    #[serde(default)]
    pub(in crate::native_tools) include_thinking: bool,
    #[schemars(
        description = "Cap the number of messages returned in the full_session payload. Maximum 200."
    )]
    #[serde(default)]
    pub(in crate::native_tools) message_limit: Option<u32>,
    #[schemars(
        description = "Filter full_session messages to those after this event_id (exclusive)."
    )]
    #[serde(default)]
    pub(in crate::native_tools) since_message_id: Option<String>,
    #[schemars(
        description = "When true, include ToolCallFinishedEvent.output_summary for each tool call in the full_session payload."
    )]
    #[serde(default)]
    pub(in crate::native_tools) include_tool_results: bool,
    #[schemars(
        description = "Cap each thinking block to this many characters when include_thinking is true. Defaults to 2000."
    )]
    #[serde(default)]
    pub(in crate::native_tools) thinking_max_chars: Option<u32>,
    #[schemars(
        description = "When true, reverse message order so the most recent messages appear first."
    )]
    #[serde(default)]
    pub(in crate::native_tools) from_end: bool,
}

#[derive(Debug, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(in crate::native_tools) struct BackgroundCancelArgs {
    #[schemars(
        description = "Canonical background request identifier returned by task(run_in_background=true). Required when all is false or omitted."
    )]
    #[serde(default)]
    pub(in crate::native_tools) request_id: Option<String>,
    #[schemars(
        description = "When true, cancel all non-terminal background tasks for the current session. When false (default), request_id is required."
    )]
    #[serde(default)]
    pub(in crate::native_tools) all: bool,
    #[schemars(
        description = "Optional human-readable reason to record with the cancellation request."
    )]
    #[serde(default)]
    pub(in crate::native_tools) reason: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct BackgroundCancelArgsCompat {
    #[serde(default)]
    request_id: Option<String>,
    #[serde(default)]
    all: bool,
    #[serde(default)]
    reason: Option<String>,
}

impl<'de> Deserialize<'de> for BackgroundCancelArgs {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let compat = BackgroundCancelArgsCompat::deserialize(deserializer)?;
        if !compat.all && compat.request_id.is_none() {
            return Err(D::Error::custom("request_id is required when all is false"));
        }
        Ok(Self {
            request_id: compat.request_id,
            all: compat.all,
            reason: compat.reason,
        })
    }
}

fn default_background_output_timeout_ms() -> u64 {
    120_000
}

#[derive(Debug, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(in crate::native_tools) struct BatchArgs {
    #[schemars(
        description = "Ordered child tool calls to execute through the coordinator. Each entry has a tool id and parameters object; batch cannot be nested."
    )]
    pub(in crate::native_tools) tool_calls: Vec<BatchCall>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct BatchWrapperCall {
    recipient_name: String,
    #[serde(default)]
    parameters: Option<Value>,
    #[serde(default)]
    arguments: Option<Value>,
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum BatchArgsCompat {
    Wrapped { tool_calls: Vec<BatchCall> },
    WrappedWrapper { tool_calls: Vec<BatchWrapperCall> },
    Wrapper { tool_uses: Vec<BatchWrapperCall> },
    Calls { calls: Vec<BatchCall> },
    List(Vec<BatchCall>),
}

impl<'de> Deserialize<'de> for BatchArgs {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let tool_calls = match BatchArgsCompat::deserialize(deserializer)? {
            BatchArgsCompat::Wrapped { tool_calls } => tool_calls,
            BatchArgsCompat::WrappedWrapper { tool_calls } => tool_calls
                .into_iter()
                .map(BatchCall::try_from)
                .collect::<Result<Vec<_>, _>>()
                .map_err(D::Error::custom)?,
            BatchArgsCompat::Calls { calls } => calls,
            BatchArgsCompat::List(calls) => calls,
            BatchArgsCompat::Wrapper { tool_uses } => tool_uses
                .into_iter()
                .map(BatchCall::try_from)
                .collect::<Result<Vec<_>, _>>()
                .map_err(D::Error::custom)?,
        };

        Ok(Self { tool_calls })
    }
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(in crate::native_tools) struct SkillArgs {
    #[schemars(
        description = "Skill name to load from the configured skill catalog; use exactly the name shown in available_skills."
    )]
    pub(in crate::native_tools) name: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(in crate::native_tools) struct InvalidArgs {
    #[schemars(
        description = "Name of the invalid or unsupported tool the provider attempted to call."
    )]
    pub(in crate::native_tools) tool: String,
    #[schemars(
        description = "Provider or argument error text to return as deterministic model-facing recovery guidance."
    )]
    pub(in crate::native_tools) error: String,
}

#[derive(Debug, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(in crate::native_tools) struct QuestionArgs {
    pub(in crate::native_tools) questions: Vec<QuestionPrompt>,
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum QuestionArgsCompat {
    Wrapped { questions: Vec<QuestionPrompt> },
    List(Vec<QuestionPrompt>),
    Single(QuestionPrompt),
}

impl<'de> Deserialize<'de> for QuestionArgs {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        Ok(match QuestionArgsCompat::deserialize(deserializer)? {
            QuestionArgsCompat::Wrapped { questions } => Self { questions },
            QuestionArgsCompat::List(questions) => Self { questions },
            QuestionArgsCompat::Single(question) => Self {
                questions: vec![question],
            },
        })
    }
}

impl TryFrom<BatchWrapperCall> for BatchCall {
    type Error = String;

    fn try_from(value: BatchWrapperCall) -> Result<Self, Self::Error> {
        let tool = value
            .recipient_name
            .rsplit('.')
            .next()
            .and_then(trimmed_non_empty)
            .ok_or_else(|| "batch wrapper call requires a non-empty recipient_name".to_string())?
            .to_string();
        let parameters = value.parameters.or(value.arguments).unwrap_or(Value::Null);
        Ok(Self { tool, parameters })
    }
}
