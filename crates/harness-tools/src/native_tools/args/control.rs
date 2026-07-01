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

#[derive(Debug, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(in crate::native_tools) struct TaskArgs {
    #[schemars(
        description = "Short label for the delegated work, used in child session titles and status output."
    )]
    pub(in crate::native_tools) description: String,
    #[schemars(
        description = "Task body delivered to the child. For non-trivial delegation, include context, goal, downstream use, request, required tools, must-do, and must-not-do sections."
    )]
    pub(in crate::native_tools) prompt: String,
    #[schemars(
        description = "Specific configured subagent/profile to run, such as `explore` or `build`. Required when category is omitted; use either subagent_type or category, not both."
    )]
    pub(in crate::native_tools) subagent_type: Option<String>,
    #[schemars(
        description = "Category selector for category-routed delegation. Required when subagent_type is omitted; use either category or subagent_type, not both."
    )]
    pub(in crate::native_tools) category: Option<String>,
    #[schemars(
        description = "Compatibility selector for continuing an existing child task/session when provided by prior task output."
    )]
    pub(in crate::native_tools) task_id: Option<String>,
    #[schemars(
        description = "Compatibility selector for continuing an existing child session when provided by prior task output."
    )]
    pub(in crate::native_tools) session_id: Option<String>,
    #[schemars(
        description = "Required. false waits synchronously; true returns request_id/task_id immediately for background_output retrieval."
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
        description = "Canonical background request identifier returned by task(run_in_background=true)."
    )]
    #[serde(default)]
    pub(in crate::native_tools) request_id: Option<String>,
    #[schemars(
        description = "When true, wait until the background request reaches a terminal state or timeout expires."
    )]
    #[serde(default)]
    pub(in crate::native_tools) block: bool,
    #[schemars(description = "Maximum time to wait in milliseconds when block=true.")]
    #[serde(default = "default_background_output_timeout_ms", alias = "timeout_ms")]
    pub(in crate::native_tools) timeout: u64,
    #[schemars(
        description = "When true, request cancellation for a non-terminal child before returning status."
    )]
    #[serde(default)]
    pub(in crate::native_tools) cancel: bool,
    #[schemars(description = "Optional cancellation reason recorded when cancel=true succeeds.")]
    #[serde(default)]
    pub(in crate::native_tools) reason: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(in crate::native_tools) struct BackgroundCancelArgs {
    #[schemars(
        description = "Canonical background request identifier returned by task(run_in_background=true)."
    )]
    pub(in crate::native_tools) request_id: String,
    #[schemars(
        description = "Optional human-readable reason to record with the cancellation request."
    )]
    #[serde(default)]
    pub(in crate::native_tools) reason: Option<String>,
}

fn default_background_output_timeout_ms() -> u64 {
    120_000
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct TaskArgsCompat {
    description: String,
    prompt: String,
    #[serde(default)]
    subagent_type: Option<String>,
    #[serde(default)]
    category: Option<String>,
    #[serde(default)]
    agent: Option<String>,
    #[serde(default)]
    profile: Option<String>,
    #[serde(default, alias = "profileName")]
    profile_name: Option<String>,
    #[serde(default)]
    task_id: Option<String>,
    #[serde(default)]
    session_id: Option<String>,
    #[serde(default, alias = "background")]
    run_in_background: Option<bool>,
    #[serde(default, alias = "skills")]
    load_skills: Option<Vec<String>>,
    #[serde(default)]
    command: Option<String>,
}

impl<'de> Deserialize<'de> for TaskArgs {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let compat = TaskArgsCompat::deserialize(deserializer)?;
        let run_in_background = compat
            .run_in_background
            .ok_or_else(|| D::Error::custom("missing required field `run_in_background`"))?;
        let load_skills = compat
            .load_skills
            .ok_or_else(|| D::Error::custom("missing required field `load_skills`"))?;

        Ok(Self {
            description: compat.description,
            prompt: compat.prompt,
            subagent_type: compat
                .subagent_type
                .or(compat.agent)
                .or(compat.profile)
                .or(compat.profile_name),
            category: compat.category,
            task_id: compat.task_id,
            session_id: compat.session_id,
            run_in_background,
            load_skills,
            command: compat.command,
        })
    }
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
