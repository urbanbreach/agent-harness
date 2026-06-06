use crate::agent_ops::BatchCall;
use crate::control_plane::{QuestionPrompt, TodoItem};
use crate::network::WebFetchFormat;
use crate::read_window::READ_DEFAULT_LIMIT;
use crate::text::trimmed_non_empty;
use schemars::JsonSchema;
use serde::de::Error as _;
use serde::{Deserialize, Deserializer};
use serde_json::{json, Value};

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(super) struct ReadArgs {
    #[serde(rename = "filePath", alias = "path")]
    pub(super) file_path: String,
    #[serde(default)]
    pub(super) offset: Option<u32>,
    #[serde(default)]
    pub(super) limit: Option<u32>,
    #[serde(default, rename = "hashlineAnchors", alias = "hashline_anchors")]
    pub(super) hashline_anchors: Option<bool>,
}

pub(super) fn read_parameters_json_schema(default_hashline_anchors: bool) -> Value {
    json!({
        "type": "object",
        "properties": {
            "filePath": {
                "type": "string",
                "description": "The path to the file or directory to read"
            },
            "offset": {
                "type": "integer",
                "minimum": 1,
                "description": "The line number to start reading from (1-indexed)"
            },
            "limit": {
                "type": "integer",
                "minimum": 1,
                "description": format!("The maximum number of lines to read (defaults to {READ_DEFAULT_LIMIT})")
            },
            "hashlineAnchors": {
                "type": "boolean",
                "default": default_hashline_anchors,
                "description": "When true, render lines as LINE#HASH|text and return anchor metadata for robust hashline edits"
            }
        },
        "required": ["filePath"],
        "additionalProperties": false
    })
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(super) struct ListArgs {
    #[serde(default)]
    pub(super) path: Option<String>,
    #[serde(default)]
    pub(super) ignore: Option<Vec<String>>,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(super) struct GlobArgs {
    pub(super) pattern: String,
    #[serde(default)]
    pub(super) path: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(super) struct GrepArgs {
    pub(super) pattern: String,
    #[serde(default)]
    pub(super) path: Option<String>,
    #[serde(default)]
    pub(super) include: Option<String>,
    #[serde(default)]
    pub(super) literal: bool,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(super) struct BashArgs {
    pub(super) command: String,
    #[serde(default)]
    pub(super) timeout: Option<u64>,
    #[serde(default)]
    pub(super) workdir: Option<String>,
    pub(super) description: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(super) struct WebFetchArgs {
    pub(super) url: String,
    #[serde(default = "default_webfetch_format")]
    pub(super) format: WebFetchFormat,
    #[serde(default)]
    pub(super) timeout: Option<u64>,
}

fn default_webfetch_format() -> WebFetchFormat {
    WebFetchFormat::Markdown
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(super) struct TodoWriteArgs {
    pub(super) todos: Vec<TodoItem>,
}

#[derive(Debug, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(super) struct TaskArgs {
    pub(super) description: String,
    /// Task body delivered to the child. For non-trivial delegation, use a structured body with sections: context, goal, downstream use, request, required tools, must-do, must-not-do.
    pub(super) prompt: String,
    pub(super) subagent_type: Option<String>,
    pub(super) category: Option<String>,
    pub(super) task_id: Option<String>,
    pub(super) session_id: Option<String>,
    pub(super) run_in_background: bool,
    pub(super) load_skills: Vec<String>,
    pub(super) command: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(super) struct BackgroundOutputArgs {
    #[serde(default)]
    pub(super) task_id: Option<String>,
    #[serde(default)]
    pub(super) session_id: Option<String>,
    #[serde(default)]
    pub(super) request_id: Option<String>,
    #[serde(default)]
    pub(super) block: bool,
    #[serde(default = "default_background_output_timeout_ms", alias = "timeout_ms")]
    pub(super) timeout: u64,
    #[serde(default)]
    pub(super) cancel: bool,
    #[serde(default)]
    pub(super) reason: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(super) struct BackgroundCancelArgs {
    pub(super) request_id: String,
    #[serde(default)]
    pub(super) reason: Option<String>,
}

fn default_background_output_timeout_ms() -> u64 {
    120_000
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct TaskArgsCompat {
    pub(super) description: String,
    pub(super) prompt: String,
    #[serde(default)]
    pub(super) subagent_type: Option<String>,
    #[serde(default)]
    pub(super) category: Option<String>,
    #[serde(default)]
    agent: Option<String>,
    #[serde(default)]
    profile: Option<String>,
    #[serde(default, alias = "profileName")]
    profile_name: Option<String>,
    #[serde(default)]
    pub(super) task_id: Option<String>,
    #[serde(default)]
    pub(super) session_id: Option<String>,
    #[serde(default, alias = "background")]
    pub(super) run_in_background: Option<bool>,
    #[serde(default, alias = "skills")]
    pub(super) load_skills: Option<Vec<String>>,
    #[serde(default)]
    pub(super) command: Option<String>,
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
pub(super) struct BatchArgs {
    pub(super) tool_calls: Vec<BatchCall>,
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
        use serde::de::Error as _;

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
pub(super) struct SkillArgs {
    #[schemars(description = "The name of the skill from available_skills")]
    pub(super) name: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(super) struct InvalidArgs {
    pub(super) tool: String,
    pub(super) error: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(super) struct WebSearchArgs {
    pub(super) query: String,
    #[serde(default)]
    #[serde(rename = "numResults", alias = "num_results")]
    pub(super) num_results: Option<u32>,
    #[serde(default)]
    pub(super) livecrawl: Option<String>,
    #[serde(default)]
    pub(super) r#type: Option<String>,
    #[serde(default)]
    #[serde(rename = "contextMaxCharacters", alias = "context_max_characters")]
    pub(super) context_max_characters: Option<u32>,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(super) struct CodeSearchArgs {
    pub(super) query: String,
    #[serde(default)]
    #[serde(rename = "tokensNum", alias = "tokens_num")]
    pub(super) tokens_num: Option<u32>,
}

#[derive(Debug, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(super) struct QuestionArgs {
    pub(super) questions: Vec<QuestionPrompt>,
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
