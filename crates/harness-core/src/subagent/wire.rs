use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

const fn default_true() -> bool {
    true
}

fn is_not_sentinel(value: &str) -> bool {
    let value = value.trim();
    !value.is_empty()
        && !value.eq_ignore_ascii_case("null")
        && !value.eq_ignore_ascii_case("none")
        && !value.eq_ignore_ascii_case("undefined")
}

/// Isolation mode for a subagent.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum SubagentIsolationMode {
    /// Use the shared workspace.
    #[default]
    #[serde(alias = "None")]
    None,
    /// Use an isolated worktree.
    #[serde(alias = "Worktree", alias = "work_tree", alias = "work-tree")]
    Worktree,
}

#[derive(Debug, Clone, JsonSchema)]
pub struct SpawnSubagentInput {
    /// Prompt for the child agent.
    pub prompt: String,
    /// Short task description.
    pub description: String,
    /// Requested subagent type. Omission and sentinel strings use the default.
    #[schemars(skip)]
    pub subagent_type: String,
    /// Whether `subagent_type` was explicitly supplied as a non-sentinel.
    #[schemars(skip)]
    pub subagent_type_specified: bool,
    /// Whether to run the child in the background; defaults to true.
    #[schemars(description = "Returns immediately with a subagent_id. Defaults to true.")]
    #[serde(
        default = "default_true",
        deserialize_with = "deserialize_lenient_bool"
    )]
    pub background: bool,
    /// Child workspace isolation.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub isolation: Option<SubagentIsolationMode>,
    /// Completed child whose conversation should be resumed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resume_from: Option<String>,
    /// Explicit child working directory.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cwd: Option<String>,
    /// Optional model slug.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    /// Accepted wire-only workspace identifier.
    #[schemars(skip)]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub workspace: Option<String>,
    /// Server-injected subagent identity.
    #[schemars(skip)]
    #[serde(default)]
    pub task_id: Option<String>,
}

#[derive(Deserialize)]
struct SpawnSubagentInputDe {
    prompt: String,
    description: String,
    #[serde(default)]
    subagent_type: Option<String>,
    #[serde(
        default = "default_true",
        deserialize_with = "deserialize_lenient_bool"
    )]
    background: bool,
    #[serde(default)]
    isolation: Option<SubagentIsolationMode>,
    #[serde(default)]
    resume_from: Option<String>,
    #[serde(default)]
    cwd: Option<String>,
    #[serde(default)]
    model: Option<String>,
    #[serde(default)]
    workspace: Option<String>,
    #[serde(default)]
    task_id: Option<String>,
}

impl<'de> Deserialize<'de> for SpawnSubagentInput {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let raw = SpawnSubagentInputDe::deserialize(deserializer)?;
        let subagent_type_specified = raw.subagent_type.as_deref().is_some_and(is_not_sentinel);
        let subagent_type = raw
            .subagent_type
            .filter(|value| is_not_sentinel(value))
            .unwrap_or_else(|| "general-purpose".to_owned());
        Ok(Self {
            prompt: raw.prompt,
            description: raw.description,
            subagent_type,
            subagent_type_specified,
            background: raw.background,
            isolation: raw.isolation,
            resume_from: raw.resume_from,
            cwd: raw.cwd,
            model: raw.model,
            workspace: raw.workspace,
            task_id: raw.task_id,
        })
    }
}

impl Serialize for SpawnSubagentInput {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        #[derive(Serialize)]
        struct Wire<'a> {
            prompt: &'a str,
            description: &'a str,
            #[serde(skip_serializing_if = "Option::is_none")]
            subagent_type: Option<&'a str>,
            background: bool,
            #[serde(skip_serializing_if = "Option::is_none")]
            isolation: Option<&'a SubagentIsolationMode>,
            #[serde(skip_serializing_if = "Option::is_none")]
            resume_from: Option<&'a str>,
            #[serde(skip_serializing_if = "Option::is_none")]
            cwd: Option<&'a str>,
            #[serde(skip_serializing_if = "Option::is_none")]
            model: Option<&'a str>,
            #[serde(skip_serializing_if = "Option::is_none")]
            workspace: Option<&'a str>,
            task_id: Option<&'a str>,
        }

        Wire {
            prompt: &self.prompt,
            description: &self.description,
            subagent_type: self
                .subagent_type_specified
                .then_some(self.subagent_type.as_str()),
            background: self.background,
            isolation: self.isolation.as_ref(),
            resume_from: self.resume_from.as_deref(),
            cwd: self.cwd.as_deref(),
            model: self.model.as_deref(),
            workspace: self.workspace.as_deref(),
            task_id: self.task_id.as_deref(),
        }
        .serialize(serializer)
    }
}

fn deserialize_lenient_bool<'de, D>(deserializer: D) -> Result<bool, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let value = serde_json::Value::deserialize(deserializer)?;
    match &value {
        serde_json::Value::Bool(value) => Ok(*value),
        serde_json::Value::Null => Ok(false),
        serde_json::Value::String(value) if value.trim().eq_ignore_ascii_case("true")
            || value.trim().eq_ignore_ascii_case("yes")
            || value.trim() == "1" =>
        {
            Ok(true)
        }
        serde_json::Value::String(value) if value.trim().eq_ignore_ascii_case("false")
            || value.trim().eq_ignore_ascii_case("no")
            || value.trim() == "0" =>
        {
            Ok(false)
        }
        serde_json::Value::Number(value) if value.as_i64() == Some(1) => Ok(true),
        serde_json::Value::Number(value) if value.as_i64() == Some(0) => Ok(false),
        _ => Err(serde::de::Error::custom(format!(
            "expected a boolean (true/false, \"true\"/\"false\", \"yes\"/\"no\", \"1\"/\"0\", 1/0), got {value}"
        ))),
    }
}

/// Successful foreground spawn result.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct SpawnSubagentOutput {
    pub output: String,
    pub subagent_id: String,
    pub subagent_type: String,
    pub tool_calls: u32,
    pub turns: u32,
    pub duration_ms: u64,
    pub worktree_path: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub persona: Option<String>,
    pub resume_from_hint: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub persona_hint: Option<String>,
}

/// Input to `get_command_or_subagent_output`.
#[derive(Debug, Clone, Default, Deserialize, Serialize, JsonSchema)]
pub struct GetCommandOrSubagentOutputInput {
    /// Task identifiers; hidden wire aliases also accept `task_id` and scalars.
    #[serde(
        default,
        alias = "task_id",
        deserialize_with = "deserialize_lenient_string_list"
    )]
    pub task_ids: Vec<String>,
    #[serde(default)]
    #[schemars(range(max = 3_600_000))]
    pub timeout_ms: Option<u64>,
}

fn deserialize_lenient_string_list<'de, D>(deserializer: D) -> Result<Vec<String>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let value = serde_json::Value::deserialize(deserializer)?;
    fn item(value: &serde_json::Value) -> Option<String> {
        match value {
            serde_json::Value::String(value) => Some(value.clone()),
            serde_json::Value::Number(value) => Some(value.to_string()),
            _ => None,
        }
    }
    let parsed = match &value {
        serde_json::Value::Array(values) => values.iter().map(item).collect::<Option<Vec<_>>>(),
        serde_json::Value::Null => Some(Vec::new()),
        value => item(value).map(|value| vec![value]),
    };
    parsed.ok_or_else(|| {
        serde::de::Error::custom(format!(
            "expected a list of string ids (or a single string), got {value}"
        ))
    })
}

/// One command or subagent result.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct GetCommandOrSubagentOutputResult {
    pub task_id: String,
    pub command: String,
    pub status: String,
    pub exit_code: Option<i32>,
    pub started: String,
    pub ended: Option<String>,
    pub duration_secs: f64,
    pub output: String,
    pub output_file: String,
    pub truncated: bool,
    #[serde(default)]
    pub truncation_hint: String,
    #[serde(default)]
    pub raw_output_bytes: usize,
}

/// Aggregate response for a multi-ID output or wait request.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct GetCommandOrSubagentOutputResults {
    pub mode: String,
    pub results: Vec<GetCommandOrSubagentOutputResult>,
    pub summary: String,
}

/// Result variants returned by `get_command_or_subagent_output`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub enum GetCommandOrSubagentOutputValue {
    Result(GetCommandOrSubagentOutputResult),
    TaskNotFound(String),
    MultiResult(GetCommandOrSubagentOutputResults),
}

/// Wait mode for `wait_commands_or_subagents`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum WaitCommandsOrSubagentsMode {
    WaitAny,
    WaitAll,
}

/// Input to `wait_commands_or_subagents`.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct WaitCommandsOrSubagentsInput {
    pub task_ids: Vec<String>,
    pub mode: WaitCommandsOrSubagentsMode,
    #[serde(default)]
    pub timeout_ms: Option<u64>,
}

/// Input to `kill_command_or_subagent`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct KillCommandOrSubagentInput {
    pub task_id: String,
}

/// Successful kill response.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct KillCommandOrSubagentResult {
    pub task_id: String,
    pub outcome: String,
    pub message: String,
}

/// Result variants returned by `kill_command_or_subagent`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub enum KillCommandOrSubagentValue {
    Result(KillCommandOrSubagentResult),
    TaskNotFound(String),
}

/// Message delivery mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum SendSubagentMessageDelivery {
    Steer,
    Queue,
    Interject,
}

/// Input to `send_subagent_message`.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct SendSubagentMessageInput {
    pub subagent_id: String,
    pub text: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub delivery: Option<SendSubagentMessageDelivery>,
    #[schemars(skip)]
    #[serde(default)]
    pub queue: bool,
}

impl SendSubagentMessageInput {
    /// Resolve the hidden legacy `queue` flag after the public delivery field.
    pub fn delivery(&self) -> SendSubagentMessageDelivery {
        self.delivery.unwrap_or(if self.queue {
            SendSubagentMessageDelivery::Queue
        } else {
            SendSubagentMessageDelivery::Steer
        })
    }
}

/// Message quota category.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum SendSubagentMessageQuotaKind {
    SenderTargetInFlight,
    AttemptOutbound,
}

/// Tagged response from `send_subagent_message`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[non_exhaustive]
#[serde(tag = "outcome", rename_all = "snake_case")]
pub enum SendSubagentMessageOutput {
    Accepted {
        message_id: String,
    },
    NotFoundOrNotOwned,
    NotActiveOrFinalizing,
    Saturated {
        max_in_flight: usize,
    },
    QuotaExceeded {
        kind: SendSubagentMessageQuotaKind,
        limit: usize,
    },
    AdmissionUncertain,
    NotAcceptedBeforeDeadline,
    Unsupported,
    Limit {
        max_bytes: usize,
        observed_bytes: usize,
    },
    ChannelClosed,
}
