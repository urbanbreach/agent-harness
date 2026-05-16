use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use harness_core::config::{registered_skills_config, PermissionMode};
use harness_core::question_answers::{validate_question_answers, QuestionAnswerPrompt};
use harness_core::tool::{ToolContext, ToolError, ToolResult};
use schemars::JsonSchema;
use serde::de::Error as _;
use serde::{Deserialize, Deserializer, Serialize};
use serde_json::{json, Value};

use crate::question_env::question_answers_from_env_or_request;
use crate::text::has_trimmed_content;

const TODO_STATE_FILE: &str = "control-plane/todos.json";
const SKILL_MCP_STATE_FILE: &str = "control-plane/skill-mcp-sessions.json";
const QUESTION_STATE_DIR: &str = "control-plane/questions";
const SKILL_LOAD_CONFIRM_YES: &str = "Yes";
const SKILL_LOAD_CONFIRM_NO: &str = "No";
const TODO_STATUSES: &[&str] = &["pending", "in_progress", "completed", "cancelled"];
const TODO_PRIORITIES: &[&str] = &["high", "medium", "low"];

pub(crate) struct ControlPlaneExecutor;

impl ControlPlaneExecutor {
    pub(crate) fn new() -> Self {
        Self
    }

    pub(crate) fn write_todos(
        &self,
        ctx: &ToolContext,
        todos: Vec<TodoItem>,
    ) -> Result<ToolResult, ToolError> {
        validate_todo_items(&todos).map_err(ToolError::InvalidArguments)?;
        write_todo_state(ctx, &todos)?;
        render_todos_result(todos)
    }

    pub(crate) fn read_todos(&self, ctx: &ToolContext) -> Result<ToolResult, ToolError> {
        let todos = read_todo_state(ctx)?;
        render_todos_result(todos)
    }

    pub(crate) async fn load_skill(
        &self,
        ctx: &ToolContext,
        name: &str,
        user_message: Option<String>,
    ) -> Result<ToolResult, ToolError> {
        let mut catalog = discover_skill_catalog(ctx.workspace_root.as_path())?;
        let skill_not_found = skill_not_found_message(name, &catalog);
        let skill = match catalog
            .remove(name)
            .ok_or_else(|| ToolError::Execution(skill_not_found.clone()))?
            .discovered
        {
            DiscoveredSkill::Visible(skill) => match skill.permission {
                PermissionMode::Allow => skill,
                PermissionMode::Ask => {
                    request_skill_load_approval(ctx, name).await?;
                    skill
                }
                PermissionMode::Deny => {
                    return Err(ToolError::Execution(skill_not_found));
                }
            },
            DiscoveredSkill::Denied { .. } | DiscoveredSkill::Invalid { .. } => {
                return Err(ToolError::Execution(skill_not_found));
            }
        };
        let skill = TaskSkillContext::from(*skill);
        let mut output = render_task_skill_context(&skill);
        if let Some(user_message) = user_message {
            output.push_str(&format!(
                "\n\n<skill_user_message>{user_message}</skill_user_message>"
            ));
        }
        Ok(crate::text_json_tool_result(
            output,
            json!({
                "name": skill.name,
                "location": skill.location.display().to_string(),
                "policy": skill.policy,
            }),
        ))
    }

    pub(crate) fn list_skills(&self, ctx: &ToolContext) -> Result<ToolResult, ToolError> {
        let catalog = discover_skill_catalog(ctx.workspace_root.as_path())?;
        let config = registered_skills_config();
        let skills = catalog
            .iter()
            .map(|(name, skill)| skill_listing_json(name, skill))
            .collect::<Vec<_>>();
        let visible = catalog
            .iter()
            .filter_map(|(name, skill)| {
                matches!(skill.discovered, DiscoveredSkill::Visible(_)).then_some(name.clone())
            })
            .collect::<Vec<_>>();
        let denied = catalog
            .iter()
            .filter_map(|(name, skill)| {
                matches!(skill.discovered, DiscoveredSkill::Denied { .. }).then_some(name.clone())
            })
            .collect::<Vec<_>>();
        let invalid = catalog
            .iter()
            .filter_map(|(name, skill)| {
                matches!(skill.discovered, DiscoveredSkill::Invalid { .. }).then_some(name.clone())
            })
            .collect::<Vec<_>>();
        let shadowed = catalog
            .iter()
            .filter(|(_, skill)| !skill.shadowed.is_empty())
            .flat_map(|(name, skill)| {
                skill.shadowed.iter().map(move |shadow| {
                    json!({
                        "name": name,
                        "location": shadow.location,
                        "reason": shadow.reason,
                    })
                })
            })
            .collect::<Vec<_>>();
        let roots = skill_search_dirs(ctx.workspace_root.as_path(), &config)
            .into_iter()
            .map(|dir| dir.path.display().to_string())
            .collect::<Vec<_>>();
        let display_text = format!(
            "Skills: {} visible, {} denied, {} invalid, {} shadowed",
            visible.len(),
            denied.len(),
            invalid.len(),
            shadowed.len()
        );
        Ok(crate::text_json_tool_result(
            display_text,
            json!({
                "disabled": config.disabled,
                "roots": roots,
                "visible": visible,
                "denied": denied,
                "invalid": invalid,
                "shadowed": shadowed,
                "skills": skills,
            }),
        ))
    }

    pub(crate) fn invalid_tool(&self, tool: &str, error: &str) -> ToolResult {
        ToolResult::text(format!(
            "The arguments provided to the tool are invalid: {} ({})",
            error, tool
        ))
    }

    pub(crate) async fn skill_mcp(
        &self,
        ctx: &ToolContext,
        request: SkillMcpRequest,
    ) -> Result<ToolResult, ToolError> {
        let skill = resolve_single_skill_for_mcp(ctx, &request.skill).await?;
        let declarations = skill.policy.mcp_servers.clone();
        let selected = select_skill_mcp_declarations(&declarations, request.server.as_deref())?;
        let mut state = read_skill_mcp_state(ctx)?;
        let action = request.action.unwrap_or(SkillMcpAction::Status);

        match action {
            SkillMcpAction::List | SkillMcpAction::Status => {}
            SkillMcpAction::Start | SkillMcpAction::Stop => {
                let status = match action {
                    SkillMcpAction::Start => "started",
                    SkillMcpAction::Stop => "stopped",
                    SkillMcpAction::List | SkillMcpAction::Status => unreachable!(),
                };
                for server in &selected {
                    state.insert(
                        skill_mcp_state_key(&skill.name, &server.name),
                        SkillMcpSessionState {
                            skill: skill.name.clone(),
                            server: server.name.clone(),
                            status: status.to_string(),
                            scope: "run".to_string(),
                            transport: server.transport.clone(),
                        },
                    );
                }
                write_skill_mcp_state(ctx, &state)?;
            }
        }

        let servers = selected
            .iter()
            .map(|server| {
                let key = skill_mcp_state_key(&skill.name, &server.name);
                let status = state
                    .get(&key)
                    .map(|entry| entry.status.as_str())
                    .unwrap_or("declared");
                json!({
                    "skill": skill.name,
                    "server": server.name,
                    "status": status,
                    "scope": "run",
                    "transport": server.transport,
                    "command": server.command,
                    "endpoint": server.endpoint,
                    "env_keys": server.env_keys,
                    "env_values_redacted": true,
                })
            })
            .collect::<Vec<_>>();

        Ok(crate::text_json_tool_result(
            format!(
                "skill_mcp {}: {} server(s) for skill `{}`",
                action.as_str(),
                servers.len(),
                skill.name
            ),
            json!({
                "skill": skill.name,
                "action": action.as_str(),
                "servers": servers,
                "cleanup": "state is scoped to the run artifacts; external MCP process startup remains owned by the first-class MCP executor",
            }),
        ))
    }

    pub(crate) async fn user_question(
        &self,
        ctx: &ToolContext,
        questions: Vec<QuestionPrompt>,
    ) -> Result<ToolResult, ToolError> {
        validate_question_prompts(&questions).map_err(ToolError::Execution)?;
        let state_path = question_state_path(ctx)?;
        if let Some(parent) = state_path.parent() {
            std::fs::create_dir_all(parent).map_err(|err| {
                ToolError::Execution(format!("failed to create question state directory: {err}"))
            })?;
        }
        std::fs::write(
            &state_path,
            serde_json::to_vec_pretty(&questions).map_err(|err| {
                ToolError::Execution(format!("failed to serialize questions: {err}"))
            })?,
        )
        .map_err(|err| ToolError::Execution(format!("failed to write question state: {err}")))?;

        let answers = question_answers_from_env_or_request(
            ctx,
            json!({ "questions": questions }),
            ToolError::Execution,
        )
        .await?;
        let answers =
            validate_question_answers(&questions, answers).map_err(ToolError::Execution)?;
        let formatted = format_question_answers(&questions, &answers);
        let display_text = format!(
            "User has answered your questions: {formatted}. You can now continue with the user's answers in mind."
        );
        Ok(crate::text_json_tool_result(
            display_text.clone(),
            json!({
                "questions": questions,
                "answers": answers,
                "output": display_text,
                "state_path": state_path.display().to_string(),
            }),
        ))
    }
}

#[derive(Debug, Serialize, JsonSchema, Clone, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub(crate) struct TodoItem {
    pub(crate) content: String,
    pub(crate) status: String,
    pub(crate) priority: String,
}

#[derive(Debug, Serialize, JsonSchema, Clone, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub(crate) struct QuestionPrompt {
    pub(crate) question: String,
    pub(crate) header: String,
    pub(crate) options: Vec<QuestionOption>,
    #[serde(default)]
    pub(crate) multiple: Option<bool>,
}

#[derive(Debug, Serialize, JsonSchema, Clone, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub(crate) struct QuestionOption {
    pub(crate) label: String,
    pub(crate) description: String,
}

#[derive(Debug, Deserialize, JsonSchema, Clone, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub(crate) struct SkillMcpRequest {
    pub(crate) skill: String,
    #[serde(default)]
    pub(crate) server: Option<String>,
    #[serde(default)]
    pub(crate) action: Option<SkillMcpAction>,
}

#[derive(Debug, Deserialize, JsonSchema, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub(crate) enum SkillMcpAction {
    List,
    Status,
    Start,
    Stop,
}

impl SkillMcpAction {
    fn as_str(self) -> &'static str {
        match self {
            Self::List => "list",
            Self::Status => "status",
            Self::Start => "start",
            Self::Stop => "stop",
        }
    }
}

impl QuestionAnswerPrompt for QuestionPrompt {
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

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct QuestionPromptCompat {
    #[serde(default)]
    question: Option<String>,
    #[serde(default)]
    prompt: Option<String>,
    #[serde(default)]
    text: Option<String>,
    #[serde(default)]
    header: Option<String>,
    #[serde(default)]
    title: Option<String>,
    #[serde(default)]
    options: Option<Vec<QuestionOptionInput>>,
    #[serde(default)]
    choices: Option<Vec<QuestionOptionInput>>,
    #[serde(default)]
    answers: Option<Vec<QuestionOptionInput>>,
    #[serde(default, alias = "allowMultiple", alias = "allow_multiple")]
    multiple: Option<bool>,
    #[serde(default, alias = "allowFreeform", alias = "allow_freeform")]
    allow_freeform: Option<bool>,
    #[serde(default)]
    custom: Option<bool>,
    #[serde(default, rename = "type")]
    question_type: Option<String>,
    #[serde(default, rename = "required")]
    _required: Option<bool>,
    #[serde(default, rename = "id")]
    _id: Option<Value>,
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum QuestionOptionInput {
    Label(String),
    Detailed(QuestionOptionCompat),
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct QuestionOptionCompat {
    label: String,
    #[serde(default)]
    description: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct TodoItemCompat {
    #[serde(default)]
    content: Option<String>,
    #[serde(default)]
    text: Option<String>,
    #[serde(default)]
    title: Option<String>,
    #[serde(default)]
    status: Option<String>,
    #[serde(default)]
    state: Option<String>,
    #[serde(default)]
    priority: Option<String>,
    #[serde(default)]
    done: Option<bool>,
    #[serde(default, rename = "id")]
    _id: Option<Value>,
}

impl<'de> Deserialize<'de> for TodoItem {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let compat = TodoItemCompat::deserialize(deserializer)?;
        let content = compat
            .content
            .or(compat.text)
            .or(compat.title)
            .ok_or_else(|| D::Error::custom("missing field `content`"))?;
        let status = compat
            .status
            .or(compat.state)
            .or_else(|| {
                compat.done.map(|done| {
                    if done {
                        "completed".to_string()
                    } else {
                        "pending".to_string()
                    }
                })
            })
            .unwrap_or_else(|| "pending".to_string());
        Ok(Self {
            content,
            status,
            priority: compat.priority.unwrap_or_else(|| "medium".to_string()),
        })
    }
}

impl<'de> Deserialize<'de> for QuestionPrompt {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let compat = QuestionPromptCompat::deserialize(deserializer)?;
        let question = compat
            .question
            .or(compat.prompt)
            .or(compat.text)
            .ok_or_else(|| D::Error::custom("missing field `question`"))?;
        let is_freeform = compat.allow_freeform.unwrap_or(false)
            || compat.custom.unwrap_or(false)
            || compat
                .question_type
                .as_deref()
                .is_some_and(|question_type| {
                    matches!(
                        question_type.trim().to_ascii_lowercase().as_str(),
                        "text" | "input" | "freeform" | "string"
                    )
                });
        let options = compat
            .options
            .or(compat.choices)
            .or(compat.answers)
            .unwrap_or_default();
        if options.is_empty() && !is_freeform {
            return Err(D::Error::custom("missing field `options`"));
        }
        let header = compat
            .header
            .or(compat.title)
            .filter(|value| has_trimmed_content(value))
            .unwrap_or_else(|| question.clone());

        Ok(Self {
            question,
            header,
            options: options.into_iter().map(Into::into).collect(),
            multiple: compat.multiple,
        })
    }
}

pub(crate) fn todo_write_parameters_json_schema() -> Value {
    json!({
        "type": "object",
        "additionalProperties": false,
        "required": ["todos"],
        "properties": {
            "todos": {
                "type": "array",
                "items": {
                    "type": "object",
                    "additionalProperties": false,
                    "properties": {
                        "content": { "type": "string" },
                        "text": { "type": "string" },
                        "title": { "type": "string" },
                        "status": {
                            "type": "string",
                            "enum": TODO_STATUSES
                        },
                        "state": {
                            "type": "string",
                            "enum": TODO_STATUSES
                        },
                        "priority": {
                            "type": "string",
                            "enum": TODO_PRIORITIES
                        },
                        "done": { "type": "boolean" },
                        "id": {}
                    },
                    "anyOf": [
                        { "required": ["content"] },
                        { "required": ["text"] },
                        { "required": ["title"] }
                    ]
                }
            }
        }
    })
}

pub(crate) fn question_parameters_json_schema() -> Value {
    let question_options_schema = json!({
        "type": "array",
        "items": {
            "oneOf": [
                { "type": "string" },
                {
                    "type": "object",
                    "additionalProperties": false,
                    "required": ["label"],
                    "properties": {
                        "label": { "type": "string" },
                        "description": { "type": "string" }
                    }
                }
            ]
        }
    });
    let question_prompt_schema = json!({
        "type": "object",
        "additionalProperties": false,
        "properties": {
            "question": { "type": "string" },
            "prompt": { "type": "string" },
            "text": { "type": "string" },
            "header": { "type": "string" },
            "title": { "type": "string" },
            "options": { "$ref": "#/definitions/question_options" },
            "choices": { "$ref": "#/definitions/question_options" },
            "answers": { "$ref": "#/definitions/question_options" },
            "multiple": { "type": "boolean" },
            "allowMultiple": { "type": "boolean" },
            "allow_multiple": { "type": "boolean" },
            "allowFreeform": { "type": "boolean" },
            "allow_freeform": { "type": "boolean" },
            "custom": { "type": "boolean" },
            "required": { "type": "boolean" },
            "type": { "type": "string" },
            "id": {}
        },
        "anyOf": [
            { "required": ["question"] },
            { "required": ["prompt"] },
            { "required": ["text"] }
        ]
    });

    json!({
        "type": "object",
        "additionalProperties": false,
        "required": ["questions"],
        "definitions": {
            "question_options": question_options_schema,
            "question_prompt": question_prompt_schema
        },
        "properties": {
            "questions": {
                "type": "array",
                "minItems": 1,
                "items": {
                    "$ref": "#/definitions/question_prompt"
                },
                "description": "Canonical provider-facing shape. Runtime compatibility still accepts top-level arrays and single-question payloads, but exported provider schemas use this wrapper so tool definitions stay provider-safe."
            }
        }
    })
}

impl<'de> Deserialize<'de> for QuestionOption {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        Ok(QuestionOptionInput::deserialize(deserializer)?.into())
    }
}

impl From<QuestionOptionInput> for QuestionOption {
    fn from(value: QuestionOptionInput) -> Self {
        match value {
            QuestionOptionInput::Label(label) => Self {
                description: label.clone(),
                label,
            },
            QuestionOptionInput::Detailed(option) => Self {
                description: option.description.unwrap_or_else(|| option.label.clone()),
                label: option.label,
            },
        }
    }
}

#[derive(Debug, Clone)]
struct SkillRecord {
    name: String,
    description: String,
    content: String,
    location: PathBuf,
    permission: PermissionMode,
    policy: SkillBundlePolicy,
}

#[derive(Debug, Clone)]
enum DiscoveredSkill {
    Visible(Box<SkillRecord>),
    Denied {
        location: Option<PathBuf>,
        reason: String,
    },
    Invalid {
        location: Option<PathBuf>,
        reason: String,
    },
}

#[derive(Debug, Clone)]
struct CatalogSkill {
    discovered: DiscoveredSkill,
    shadowed: Vec<SkillShadowRecord>,
}

#[derive(Debug, Clone, Serialize)]
struct SkillShadowRecord {
    location: String,
    reason: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct TaskSkillContext {
    pub(crate) name: String,
    pub(crate) description: String,
    pub(crate) content: String,
    pub(crate) location: PathBuf,
    pub(crate) policy: SkillBundlePolicy,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub(crate) struct SkillMcpServerPolicy {
    pub(crate) name: String,
    pub(crate) transport: String,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub(crate) command: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) endpoint: Option<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub(crate) env_keys: Vec<String>,
}

impl From<SkillRecord> for TaskSkillContext {
    fn from(skill: SkillRecord) -> Self {
        Self {
            name: skill.name,
            description: skill.description,
            content: skill.content,
            location: skill.location,
            policy: skill.policy,
        }
    }
}

pub(crate) fn render_task_skill_context(skill: &TaskSkillContext) -> String {
    let mut output = format!(
        "<skill_content name=\"{}\">\n# Skill: {}\n\n{}\n\n{}\n\nBase directory for this skill: file://{}\n</skill_content>",
        skill.name,
        skill.name,
        skill.description,
        skill.content.trim(),
        skill
            .location
            .parent()
            .unwrap_or(skill.location.as_path())
            .display(),
    );
    if !skill.policy.is_empty() {
        let policy =
            serde_json::to_string_pretty(&skill.policy).unwrap_or_else(|_| "{}".to_string());
        output.push_str(&format!(
            "\n<skill_policy name=\"{}\">\n{}\n</skill_policy>",
            skill.name, policy
        ));
    }
    output
}

#[derive(Debug, Default)]
struct SkillFrontmatter {
    name: Option<String>,
    description: Option<String>,
    policy: SkillBundlePolicy,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub(crate) struct SkillBundlePolicy {
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub(crate) mcp: Vec<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub(crate) mcp_servers: Vec<SkillMcpServerPolicy>,
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pub(crate) permissions: BTreeMap<String, String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub(crate) tools: Vec<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub(crate) commands: Vec<String>,
    #[serde(skip_serializing_if = "SkillEnvironmentPolicy::is_empty")]
    pub(crate) environment: SkillEnvironmentPolicy,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub(crate) verification: Vec<String>,
}

impl SkillBundlePolicy {
    fn is_empty(&self) -> bool {
        self.mcp.is_empty()
            && self.mcp_servers.is_empty()
            && self.permissions.is_empty()
            && self.tools.is_empty()
            && self.commands.is_empty()
            && self.environment.is_empty()
            && self.verification.is_empty()
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub(crate) struct SkillEnvironmentPolicy {
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub(crate) allow: Vec<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub(crate) deny: Vec<String>,
}

impl SkillEnvironmentPolicy {
    fn is_empty(&self) -> bool {
        self.allow.is_empty() && self.deny.is_empty()
    }
}

struct FrontmatterFieldLine<'a> {
    key: &'a str,
    raw_value: &'a str,
}

fn todo_state_path(ctx: &ToolContext) -> Result<PathBuf, ToolError> {
    run_root(ctx).map(|root| root.join(TODO_STATE_FILE))
}

fn skill_mcp_state_path(ctx: &ToolContext) -> Result<PathBuf, ToolError> {
    run_root(ctx).map(|root| root.join(SKILL_MCP_STATE_FILE))
}

fn read_todo_state(ctx: &ToolContext) -> Result<Vec<TodoItem>, ToolError> {
    let path = todo_state_path(ctx)?;
    if !path.exists() {
        return Ok(Vec::new());
    }
    serde_json::from_slice(&std::fs::read(&path).map_err(|err| {
        ToolError::Execution(format!(
            "failed to read todo state {}: {err}",
            path.display()
        ))
    })?)
    .map_err(|err| ToolError::Execution(format!("failed to parse todo state: {err}")))
}

fn validate_todo_items(todos: &[TodoItem]) -> Result<(), String> {
    let in_progress = todos
        .iter()
        .filter(|todo| todo.status == "in_progress")
        .count();
    if in_progress > 1 {
        return Err("todo.write accepts at most one item with status `in_progress`".to_string());
    }
    if let Some(todo) = todos
        .iter()
        .find(|todo| !TODO_STATUSES.contains(&todo.status.as_str()))
    {
        return Err(format!(
            "todo.write status must be one of {} (got `{}` for `{}`)",
            TODO_STATUSES.join(", "),
            todo.status,
            todo.content
        ));
    }
    if let Some(todo) = todos
        .iter()
        .find(|todo| !TODO_PRIORITIES.contains(&todo.priority.as_str()))
    {
        return Err(format!(
            "todo.write priority must be one of {} (got `{}` for `{}`)",
            TODO_PRIORITIES.join(", "),
            todo.priority,
            todo.content
        ));
    }
    Ok(())
}

fn render_todos_result(todos: Vec<TodoItem>) -> Result<ToolResult, ToolError> {
    let display_text = serde_json::to_string_pretty(&todos)
        .map_err(|err| ToolError::Execution(format!("failed to render todos: {err}")))?;
    Ok(crate::text_json_tool_result(
        display_text,
        json!({ "todos": todos }),
    ))
}

fn write_todo_state(ctx: &ToolContext, todos: &[TodoItem]) -> Result<(), ToolError> {
    let path = todo_state_path(ctx)?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|err| {
            ToolError::Execution(format!("failed to create todo state directory: {err}"))
        })?;
    }
    let bytes = serde_json::to_vec_pretty(todos)
        .map_err(|err| ToolError::Execution(format!("failed to serialize todos: {err}")))?;
    let temp_path = path.with_extension(format!("{}.tmp", ctx.tool_call_id));
    std::fs::write(&temp_path, bytes).map_err(|err| {
        ToolError::Execution(format!(
            "failed to write todo state temp file {}: {err}",
            temp_path.display()
        ))
    })?;
    std::fs::rename(&temp_path, &path).map_err(|err| {
        ToolError::Execution(format!(
            "failed to atomically replace todo state {}: {err}",
            path.display()
        ))
    })
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct SkillMcpSessionState {
    skill: String,
    server: String,
    status: String,
    scope: String,
    transport: String,
}

fn read_skill_mcp_state(
    ctx: &ToolContext,
) -> Result<BTreeMap<String, SkillMcpSessionState>, ToolError> {
    let path = skill_mcp_state_path(ctx)?;
    if !path.exists() {
        return Ok(BTreeMap::new());
    }
    serde_json::from_slice(&std::fs::read(&path).map_err(|err| {
        ToolError::Execution(format!(
            "failed to read skill MCP state {}: {err}",
            path.display()
        ))
    })?)
    .map_err(|err| ToolError::Execution(format!("failed to parse skill MCP state: {err}")))
}

fn write_skill_mcp_state(
    ctx: &ToolContext,
    state: &BTreeMap<String, SkillMcpSessionState>,
) -> Result<(), ToolError> {
    let path = skill_mcp_state_path(ctx)?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|err| {
            ToolError::Execution(format!("failed to create skill MCP state directory: {err}"))
        })?;
    }
    let bytes = serde_json::to_vec_pretty(state).map_err(|err| {
        ToolError::Execution(format!("failed to serialize skill MCP state: {err}"))
    })?;
    let temp_path = path.with_extension(format!("{}.tmp", ctx.tool_call_id));
    std::fs::write(&temp_path, bytes).map_err(|err| {
        ToolError::Execution(format!(
            "failed to write skill MCP state temp file {}: {err}",
            temp_path.display()
        ))
    })?;
    std::fs::rename(&temp_path, &path).map_err(|err| {
        ToolError::Execution(format!(
            "failed to atomically replace skill MCP state {}: {err}",
            path.display()
        ))
    })
}

fn skill_mcp_state_key(skill: &str, server: &str) -> String {
    format!("{skill}/{server}")
}

fn question_state_path(ctx: &ToolContext) -> Result<PathBuf, ToolError> {
    run_root(ctx).map(|root| {
        root.join(QUESTION_STATE_DIR)
            .join(format!("{}.json", ctx.tool_call_id))
    })
}

fn validate_question_prompts(questions: &[QuestionPrompt]) -> Result<(), String> {
    if questions.is_empty() {
        return Err("at least one question is required".to_string());
    }

    Ok(())
}

pub(crate) async fn resolve_task_skill_context(
    ctx: &ToolContext,
    names: &[String],
) -> Result<Vec<TaskSkillContext>, ToolError> {
    if names.is_empty() {
        return Ok(Vec::new());
    }

    let mut catalog = discover_skill_catalog(ctx.workspace_root.as_path())?;
    let mut seen = BTreeSet::new();
    let mut resolved = Vec::new();
    for raw_name in names {
        let name = raw_name.trim();
        if name.is_empty() {
            return Err(ToolError::InvalidArguments(
                "load_skills entries must not be empty".to_string(),
            ));
        }
        if !seen.insert(name.to_string()) {
            continue;
        }

        let skill_not_found = skill_not_found_message(name, &catalog);
        let skill = match catalog
            .remove(name)
            .ok_or_else(|| ToolError::Execution(skill_not_found.clone()))?
            .discovered
        {
            DiscoveredSkill::Visible(skill) => match skill.permission {
                PermissionMode::Allow => skill,
                PermissionMode::Ask => {
                    request_skill_load_approval(ctx, name).await?;
                    skill
                }
                PermissionMode::Deny => {
                    return Err(ToolError::Execution(skill_not_found));
                }
            },
            DiscoveredSkill::Denied { .. } | DiscoveredSkill::Invalid { .. } => {
                return Err(ToolError::Execution(skill_not_found));
            }
        };
        let skill = *skill;

        resolved.push(TaskSkillContext {
            name: skill.name,
            description: skill.description,
            content: skill.content,
            location: skill.location,
            policy: skill.policy,
        });
    }

    Ok(resolved)
}

async fn resolve_single_skill_for_mcp(
    ctx: &ToolContext,
    name: &str,
) -> Result<TaskSkillContext, ToolError> {
    let mut skills = resolve_task_skill_context(ctx, &[name.to_string()]).await?;
    let skill = skills
        .pop()
        .ok_or_else(|| ToolError::Execution(format!("Skill \"{name}\" not found")))?;
    if skill.policy.mcp_servers.is_empty() {
        return Err(ToolError::Execution(format!(
            "Skill \"{name}\" does not declare any MCP servers"
        )));
    }
    Ok(skill)
}

fn select_skill_mcp_declarations(
    declarations: &[SkillMcpServerPolicy],
    server: Option<&str>,
) -> Result<Vec<SkillMcpServerPolicy>, ToolError> {
    match server {
        Some(server) => declarations
            .iter()
            .find(|candidate| candidate.name == server)
            .cloned()
            .map(|server| vec![server])
            .ok_or_else(|| {
                ToolError::InvalidArguments(format!(
                    "skill MCP server `{server}` is not declared by the loaded skill"
                ))
            }),
        None => Ok(declarations.to_vec()),
    }
}

async fn request_skill_load_approval(ctx: &ToolContext, name: &str) -> Result<(), ToolError> {
    let questions = vec![skill_load_confirmation_question(name)];
    let answers =
        question_answers_from_env_or_request(ctx, json!({ "questions": questions }), |err| {
            ToolError::Execution(format!(
                "Skill \"{name}\" approval failed before loading: {err}"
            ))
        })
        .await?;
    let answers = validate_question_answers(&questions, answers).map_err(ToolError::Execution)?;
    let approved = answers
        .first()
        .and_then(|answer| answer.first())
        .is_some_and(|answer| answer == SKILL_LOAD_CONFIRM_YES);

    if approved {
        Ok(())
    } else {
        Err(ToolError::Execution(format!(
            "Skill \"{name}\" load cancelled by user confirmation"
        )))
    }
}

fn skill_not_found_message(name: &str, catalog: &BTreeMap<String, CatalogSkill>) -> String {
    let trimmed = name.trim();
    let mut message = format!("Skill \"{trimmed}\" not found");

    if let Some(agent_name) = known_agent_name(trimmed) {
        message.push_str(&format!(
            ". `{trimmed}` is an agent, not a skill; use task(category=\"{agent_name}\", ...) if you need a child session"
        ));
    }

    let visible = catalog
        .iter()
        .filter_map(|(name, skill)| {
            matches!(skill.discovered, DiscoveredSkill::Visible(_)).then_some(name.as_str())
        })
        .take(5)
        .collect::<Vec<_>>();
    if visible.is_empty() {
        message.push_str(". No skills are currently available.");
    } else {
        message.push_str(&format!(". Available skills: {}", visible.join(", ")));
    }

    message
}

fn skill_listing_json(name: &str, skill: &CatalogSkill) -> Value {
    let shadowed = skill
        .shadowed
        .iter()
        .map(|shadow| {
            json!({
                "location": shadow.location,
                "reason": shadow.reason,
            })
        })
        .collect::<Vec<_>>();
    match &skill.discovered {
        DiscoveredSkill::Visible(record) => json!({
            "name": name,
            "status": "visible",
            "description": record.description,
            "location": record.location.display().to_string(),
            "permission": format!("{:?}", record.permission).to_ascii_lowercase(),
            "policy": record.policy,
            "shadowed": shadowed,
        }),
        DiscoveredSkill::Denied { location, reason } => json!({
            "name": name,
            "status": "denied",
            "location": location.as_ref().map(|path| path.display().to_string()),
            "reason": reason,
            "shadowed": shadowed,
        }),
        DiscoveredSkill::Invalid { location, reason } => json!({
            "name": name,
            "status": "invalid",
            "location": location.as_ref().map(|path| path.display().to_string()),
            "reason": reason,
            "shadowed": shadowed,
        }),
    }
}

fn known_agent_name(name: &str) -> Option<&'static str> {
    match name.trim().to_ascii_lowercase().as_str() {
        "build" => Some("build"),
        "plan" => Some("plan"),
        _ => None,
    }
}

fn skill_load_confirmation_question(skill_name: &str) -> QuestionPrompt {
    QuestionPrompt {
        question: format!("Would you like to load the `{skill_name}` skill?"),
        header: "Load Skill".to_string(),
        options: vec![
            QuestionOption {
                label: SKILL_LOAD_CONFIRM_YES.to_string(),
                description: format!(
                    "Load the `{skill_name}` skill and make its instructions available to the agent"
                ),
            },
            QuestionOption {
                label: SKILL_LOAD_CONFIRM_NO.to_string(),
                description: format!("Do not load the `{skill_name}` skill"),
            },
        ],
        multiple: Some(false),
    }
}

fn format_question_answers(questions: &[QuestionPrompt], answers: &[Vec<String>]) -> String {
    questions
        .iter()
        .zip(answers)
        .map(|(question, answers)| {
            let answer = if answers.is_empty() {
                "Unanswered".to_string()
            } else {
                answers.join(", ")
            };
            format!("\"{}\"=\"{}\"", question.question, answer)
        })
        .collect::<Vec<_>>()
        .join(", ")
}

fn run_root(ctx: &ToolContext) -> Result<PathBuf, ToolError> {
    ctx.artifacts_dir
        .parent()
        .map(Path::to_path_buf)
        .ok_or_else(|| {
            ToolError::Execution(
                "failed to determine run root from artifacts directory".to_string(),
            )
        })
}

fn discover_skill_catalog(
    workspace_root: &Path,
) -> Result<BTreeMap<String, CatalogSkill>, ToolError> {
    let config = registered_skills_config();
    let mut catalog = BTreeMap::new();
    if config.disabled {
        return Ok(catalog);
    }
    for search_dir in skill_search_dirs(workspace_root, &config) {
        if !search_dir.path.exists() {
            continue;
        }
        let canonical_dir = search_dir.path.canonicalize().map_err(|err| {
            ToolError::Execution(format!(
                "failed to resolve skill directory {}: {err}",
                search_dir.path.display()
            ))
        })?;
        if let Some(base_dir) = search_dir.project_base.as_deref() {
            let canonical_base = base_dir.canonicalize().map_err(|err| {
                ToolError::Execution(format!(
                    "failed to resolve skill search base {}: {err}",
                    base_dir.display()
                ))
            })?;
            if !canonical_dir.starts_with(canonical_base) {
                continue;
            }
        }

        for entry in sorted_skill_entries(&canonical_dir)? {
            let name = entry.file_name().to_string_lossy().to_string();
            if let Some(existing) = catalog.get_mut(&name) {
                existing.shadowed.push(SkillShadowRecord {
                    location: entry.path().join("SKILL.md").display().to_string(),
                    reason: format!("shadowed by higher-precedence skill `{name}`"),
                });
                continue;
            }

            let permission = resolve_skill_permission(&name, &config.permissions);
            if config.disabled_skills.contains(&name) {
                catalog.insert(
                    name,
                    CatalogSkill {
                        discovered: DiscoveredSkill::Denied {
                            location: Some(entry.path().join("SKILL.md")),
                            reason: "disabled by skills.disabled_skills".to_string(),
                        },
                        shadowed: Vec::new(),
                    },
                );
                continue;
            }
            if permission == PermissionMode::Deny {
                catalog.insert(
                    name,
                    CatalogSkill {
                        discovered: DiscoveredSkill::Denied {
                            location: Some(entry.path().join("SKILL.md")),
                            reason: "denied by skills.permissions".to_string(),
                        },
                        shadowed: Vec::new(),
                    },
                );
                continue;
            }

            if entry
                .file_type()
                .map_err(|err| {
                    ToolError::Execution(format!("failed to inspect skill entry: {err}"))
                })?
                .is_symlink()
            {
                catalog.insert(
                    name,
                    CatalogSkill {
                        discovered: DiscoveredSkill::Invalid {
                            location: Some(entry.path()),
                            reason: "skill directory must not be a symlink".to_string(),
                        },
                        shadowed: Vec::new(),
                    },
                );
                continue;
            }

            let skill_file = entry.path().join("SKILL.md");
            if !skill_file.exists() {
                continue;
            }
            let canonical_skill_file = skill_file.canonicalize().map_err(|err| {
                ToolError::Execution(format!(
                    "failed to resolve skill file {}: {err}",
                    skill_file.display()
                ))
            })?;
            if !canonical_skill_file.starts_with(&canonical_dir) {
                catalog.insert(
                    name,
                    CatalogSkill {
                        discovered: DiscoveredSkill::Invalid {
                            location: Some(skill_file),
                            reason: "skill file resolved outside its skill root".to_string(),
                        },
                        shadowed: Vec::new(),
                    },
                );
                continue;
            }

            let content = std::fs::read_to_string(&canonical_skill_file).map_err(|err| {
                ToolError::Execution(format!(
                    "failed to read skill file {}: {err}",
                    canonical_skill_file.display()
                ))
            })?;

            match build_skill_record(&name, &canonical_skill_file, &content, permission.clone()) {
                Ok(skill) => {
                    catalog.insert(
                        name,
                        CatalogSkill {
                            discovered: DiscoveredSkill::Visible(Box::new(skill)),
                            shadowed: Vec::new(),
                        },
                    );
                }
                Err(reason) => {
                    catalog.insert(
                        name,
                        CatalogSkill {
                            discovered: DiscoveredSkill::Invalid {
                                location: Some(canonical_skill_file),
                                reason,
                            },
                            shadowed: Vec::new(),
                        },
                    );
                }
            }
        }
    }

    Ok(catalog)
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct SkillSearchDir {
    path: PathBuf,
    project_base: Option<PathBuf>,
}

fn skill_search_dirs(
    current_dir: &Path,
    config: &harness_core::config::SkillsConfig,
) -> Vec<SkillSearchDir> {
    let mut dirs = Vec::new();
    for base_dir in project_search_bases(current_dir, config.walk_to_git_root) {
        for root in &config.project_roots {
            push_unique_search_dir(
                &mut dirs,
                SkillSearchDir {
                    path: resolve_skill_root(&base_dir, root),
                    project_base: Some(base_dir.clone()),
                },
            );
        }
    }
    for root in &config.global_roots {
        push_unique_search_dir(
            &mut dirs,
            SkillSearchDir {
                path: resolve_skill_root(current_dir, root),
                project_base: None,
            },
        );
    }
    dirs
}

fn project_search_bases(current_dir: &Path, walk_to_git_root: bool) -> Vec<PathBuf> {
    if !walk_to_git_root {
        return vec![current_dir.to_path_buf()];
    }

    let ancestors = current_dir
        .ancestors()
        .map(Path::to_path_buf)
        .collect::<Vec<_>>();
    if let Some(git_root_index) = ancestors.iter().position(|path| path.join(".git").exists()) {
        return ancestors.into_iter().take(git_root_index + 1).collect();
    }

    vec![current_dir.to_path_buf()]
}

fn push_unique_search_dir(paths: &mut Vec<SkillSearchDir>, candidate: SkillSearchDir) {
    if !paths.iter().any(|existing| existing.path == candidate.path) {
        paths.push(candidate);
    }
}

fn resolve_skill_root(base_dir: &Path, root: &Path) -> PathBuf {
    let expanded = expand_home(root);
    if expanded.is_absolute() {
        expanded
    } else {
        base_dir.join(expanded)
    }
}

fn expand_home(path: &Path) -> PathBuf {
    let text = path.to_string_lossy();
    if text == "~" {
        return std::env::var_os("HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|| path.to_path_buf());
    }
    if let Some(stripped) = text.strip_prefix("~/") {
        if let Some(home) = std::env::var_os("HOME") {
            return PathBuf::from(home).join(stripped);
        }
    }
    path.to_path_buf()
}

fn sorted_skill_entries(dir: &Path) -> Result<Vec<std::fs::DirEntry>, ToolError> {
    let mut entries = std::fs::read_dir(dir)
        .map_err(|err| {
            ToolError::Execution(format!(
                "failed to read skill directory {}: {err}",
                dir.display()
            ))
        })?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|err| ToolError::Execution(format!("failed to read skill entry: {err}")))?;
    entries.sort_by_key(|entry| entry.file_name());
    Ok(entries)
}

fn resolve_skill_permission(
    name: &str,
    permissions: &BTreeMap<String, PermissionMode>,
) -> PermissionMode {
    permissions
        .iter()
        .filter(|(pattern, _)| skill_name_matches_pattern(name, pattern))
        .max_by(|(left, _), (right, _)| compare_permission_patterns(left, right))
        .map(|(_, mode)| mode.clone())
        .unwrap_or(PermissionMode::Allow)
}

fn compare_permission_patterns(left: &str, right: &str) -> std::cmp::Ordering {
    skill_pattern_specificity(left)
        .cmp(&skill_pattern_specificity(right))
        .then_with(|| left.cmp(right))
}

fn skill_pattern_specificity(pattern: &str) -> (bool, usize, usize) {
    let non_wildcard_len = pattern.chars().filter(|ch| *ch != '*').count();
    (!pattern.contains('*'), non_wildcard_len, pattern.len())
}

fn skill_name_matches_pattern(name: &str, pattern: &str) -> bool {
    if pattern == "*" {
        return true;
    }

    let segments = pattern.split('*').collect::<Vec<_>>();
    let anchored_start = !pattern.starts_with('*');
    let anchored_end = !pattern.ends_with('*');
    let mut remainder = name;

    for (index, segment) in segments.iter().enumerate() {
        if segment.is_empty() {
            continue;
        }

        if index == 0 && anchored_start {
            let Some(stripped) = remainder.strip_prefix(segment) else {
                return false;
            };
            remainder = stripped;
            continue;
        }

        if index == segments.len() - 1 && anchored_end {
            return remainder.ends_with(segment);
        }

        let Some(position) = remainder.find(segment) else {
            return false;
        };
        remainder = &remainder[position + segment.len()..];
    }

    !anchored_end
        || segments.last().is_some_and(|segment| segment.is_empty())
        || remainder.is_empty()
}

fn build_skill_record(
    directory_name: &str,
    skill_file: &Path,
    content: &str,
    permission: PermissionMode,
) -> Result<SkillRecord, String> {
    let frontmatter = parse_skill_frontmatter(content)?;
    let name = frontmatter.name.ok_or_else(|| {
        format!(
            "skill {} is missing required frontmatter `name`",
            skill_file.display()
        )
    })?;
    validate_skill_name(&name, skill_file)?;
    if name != directory_name {
        return Err(format!(
            "skill {} frontmatter `name` `{name}` must match directory `{directory_name}`",
            skill_file.display()
        ));
    }

    let description = frontmatter.description.ok_or_else(|| {
        format!(
            "skill {} is missing required frontmatter `description`",
            skill_file.display()
        )
    })?;
    let description_len = description.chars().count();
    if !(1..=1024).contains(&description_len) {
        return Err(format!(
            "skill {} frontmatter `description` must be 1-1024 characters",
            skill_file.display()
        ));
    }

    Ok(SkillRecord {
        name,
        description,
        content: content.to_string(),
        location: skill_file.to_path_buf(),
        permission,
        policy: frontmatter.policy,
    })
}

fn parse_skill_frontmatter(content: &str) -> Result<SkillFrontmatter, String> {
    let mut lines = content.lines();
    if lines.next() != Some("---") {
        return Err("skill frontmatter must start with `---`".to_string());
    }

    let mut frontmatter_lines = Vec::new();
    let mut found_closing = false;
    for line in lines {
        if line == "---" {
            found_closing = true;
            break;
        }
        frontmatter_lines.push(line);
    }

    if !found_closing {
        return Err("skill frontmatter must end with `---`".to_string());
    }

    parse_frontmatter_fields(&frontmatter_lines)
}

fn parse_frontmatter_fields(lines: &[&str]) -> Result<SkillFrontmatter, String> {
    let mut frontmatter = SkillFrontmatter::default();
    let mut index = 0;

    while index < lines.len() {
        let line = lines[index];
        if !has_trimmed_content(line) {
            index += 1;
            continue;
        }
        if leading_indent(line)? != 0 {
            return Err(format!(
                "frontmatter line `{}` must not be indented",
                line.trim()
            ));
        }

        let field = parse_frontmatter_field_line(line, |trimmed| {
            format!("frontmatter line `{trimmed}` must use `key: value` syntax")
        })?;

        match field.key {
            "name" => {
                let (value, next_index) = parse_scalar_field(lines, index, field.raw_value)?;
                frontmatter.name = Some(value);
                index = next_index;
            }
            "description" => {
                let (value, next_index) = parse_scalar_field(lines, index, field.raw_value)?;
                frontmatter.description = Some(value);
                index = next_index;
            }
            "license" | "compatibility" => {
                let (_, next_index) = parse_scalar_field(lines, index, field.raw_value)?;
                index = next_index;
            }
            "metadata" => {
                index = parse_metadata_field(lines, index, field.raw_value)?;
            }
            "mcp" | "mcps" => {
                let (values, next_index) =
                    parse_string_list_or_keys(lines, index, field.raw_value)?;
                frontmatter.policy.mcp = values;
                frontmatter.policy.mcp_servers =
                    parse_mcp_server_policies(lines, index, field.raw_value)?;
                index = next_index;
            }
            "tools" => {
                let (values, next_index) =
                    parse_string_list_or_keys(lines, index, field.raw_value)?;
                frontmatter.policy.tools = values;
                index = next_index;
            }
            "commands" | "command_templates" | "commandTemplates" => {
                let (values, next_index) =
                    parse_string_list_or_keys(lines, index, field.raw_value)?;
                frontmatter.policy.commands = values;
                index = next_index;
            }
            "verification" | "verification_hooks" | "verificationHooks" => {
                let (values, next_index) =
                    parse_string_list_or_keys(lines, index, field.raw_value)?;
                frontmatter.policy.verification = values;
                index = next_index;
            }
            "permissions" => {
                let (values, next_index) = parse_string_map(lines, index, field.raw_value)?;
                frontmatter.policy.permissions = values;
                index = next_index;
            }
            "env" | "environment" => {
                let (policy, next_index) = parse_environment_policy(lines, index, field.raw_value)?;
                frontmatter.policy.environment = policy;
                index = next_index;
            }
            _ => {
                index = skip_unknown_field(lines, index, field.raw_value)?;
            }
        }
    }

    Ok(frontmatter)
}

fn parse_string_list_or_keys(
    lines: &[&str],
    index: usize,
    raw_value: &str,
) -> Result<(Vec<String>, usize), String> {
    parse_string_list_or_keys_with_parent(lines, index, raw_value, 0)
}

fn parse_string_list_or_keys_with_parent(
    lines: &[&str],
    index: usize,
    raw_value: &str,
    parent_indent: usize,
) -> Result<(Vec<String>, usize), String> {
    if !raw_value.is_empty() {
        return Ok((parse_inline_list_or_scalar(raw_value), index + 1));
    }

    let mut cursor = index + 1;
    let mut values = Vec::new();
    let mut item_indent = None;
    while cursor < lines.len() {
        let line = lines[cursor];
        if !has_trimmed_content(line) {
            cursor += 1;
            continue;
        }
        let indent = leading_indent(line)?;
        if indent <= parent_indent {
            break;
        }
        let item_indent = *item_indent.get_or_insert(indent);
        if indent > item_indent {
            cursor += 1;
            continue;
        }
        if indent < item_indent {
            break;
        }
        let trimmed = line.trim();
        if let Some(item) = trimmed.strip_prefix("- ") {
            values.push(parse_inline_scalar(item));
        } else if let Some((key, _)) = trimmed.split_once(':') {
            if !key.trim().is_empty() {
                values.push(key.trim().to_string());
            }
        }
        cursor += 1;
    }
    Ok((dedup_non_empty(values), cursor))
}

fn parse_string_map(
    lines: &[&str],
    index: usize,
    raw_value: &str,
) -> Result<(BTreeMap<String, String>, usize), String> {
    if !raw_value.is_empty() {
        return Err("frontmatter map fields must use nested key/value entries".to_string());
    }

    let mut cursor = index + 1;
    let mut values = BTreeMap::new();
    while cursor < lines.len() {
        let line = lines[cursor];
        if !has_trimmed_content(line) {
            cursor += 1;
            continue;
        }
        let indent = leading_indent(line)?;
        if indent == 0 {
            break;
        }
        let field = parse_frontmatter_field_line(line.trim(), |trimmed| {
            format!("frontmatter map entry `{trimmed}` must use `key: value` syntax")
        })?;
        if field.key.trim().is_empty() {
            return Err("frontmatter map keys must not be empty".to_string());
        }
        values.insert(
            field.key.trim().to_string(),
            parse_inline_scalar(field.raw_value),
        );
        cursor += 1;
    }
    Ok((values, cursor))
}

fn parse_mcp_server_policies(
    lines: &[&str],
    index: usize,
    raw_value: &str,
) -> Result<Vec<SkillMcpServerPolicy>, String> {
    if !raw_value.is_empty() {
        return Ok(parse_inline_list_or_scalar(raw_value)
            .into_iter()
            .map(default_mcp_server_policy)
            .collect());
    }

    let mut cursor = index + 1;
    let mut policies = Vec::new();
    let mut server_indent = None;
    while cursor < lines.len() {
        let line = lines[cursor];
        if !has_trimmed_content(line) {
            cursor += 1;
            continue;
        }
        let indent = leading_indent(line)?;
        if indent == 0 {
            break;
        }
        let current_server_indent = *server_indent.get_or_insert(indent);
        if indent > current_server_indent {
            cursor += 1;
            continue;
        }
        if indent < current_server_indent {
            break;
        }
        let trimmed = line.trim();
        if let Some(item) = trimmed.strip_prefix("- ") {
            let name = parse_inline_scalar(item);
            if !name.is_empty() {
                policies.push(default_mcp_server_policy(name));
            }
            cursor += 1;
            continue;
        }
        let Some((raw_name, raw_tail)) = trimmed.split_once(':') else {
            cursor += 1;
            continue;
        };
        let name = raw_name.trim();
        if name.is_empty() || name.starts_with('-') {
            cursor += 1;
            continue;
        }
        let mut policy = default_mcp_server_policy(name.to_string());
        if !raw_tail.trim().is_empty() {
            policy.transport = raw_tail.trim().to_string();
            policies.push(policy);
            cursor += 1;
            continue;
        }

        cursor += 1;
        while cursor < lines.len() {
            let child_line = lines[cursor];
            if !has_trimmed_content(child_line) {
                cursor += 1;
                continue;
            }
            let child_indent = leading_indent(child_line)?;
            if child_indent <= current_server_indent {
                break;
            }
            let field = parse_frontmatter_field_line(child_line.trim(), |trimmed| {
                format!("frontmatter mcp entry `{trimmed}` must use `key: value` syntax")
            })?;
            match field.key {
                "transport" | "type" => {
                    policy.transport = parse_inline_scalar(field.raw_value);
                    cursor += 1;
                }
                "command" => {
                    policy.command = parse_inline_list_or_scalar(field.raw_value);
                    policy.transport = "stdio".to_string();
                    cursor += 1;
                }
                "args" => {
                    let (args, next_index) = parse_string_list_or_keys_with_parent(
                        lines,
                        cursor,
                        field.raw_value,
                        child_indent,
                    )?;
                    policy.command.extend(args);
                    cursor = next_index;
                }
                "endpoint" | "url" => {
                    policy.endpoint = Some(parse_inline_scalar(field.raw_value));
                    policy.transport = "http".to_string();
                    cursor += 1;
                }
                "env" | "environment" => {
                    let (env, next_index) = parse_string_map(lines, cursor, field.raw_value)?;
                    policy.env_keys = env.keys().cloned().collect();
                    cursor = next_index;
                }
                _ => {
                    cursor = skip_unknown_field(lines, cursor, field.raw_value)?;
                }
            }
        }
        policies.push(policy);
    }
    Ok(policies)
}

fn default_mcp_server_policy(name: String) -> SkillMcpServerPolicy {
    SkillMcpServerPolicy {
        name,
        transport: "declared".to_string(),
        command: Vec::new(),
        endpoint: None,
        env_keys: Vec::new(),
    }
}

fn parse_environment_policy(
    lines: &[&str],
    index: usize,
    raw_value: &str,
) -> Result<(SkillEnvironmentPolicy, usize), String> {
    if !raw_value.is_empty() {
        return Ok((
            SkillEnvironmentPolicy {
                allow: parse_inline_list_or_scalar(raw_value),
                deny: Vec::new(),
            },
            index + 1,
        ));
    }

    let mut cursor = index + 1;
    let mut policy = SkillEnvironmentPolicy::default();
    while cursor < lines.len() {
        let line = lines[cursor];
        if !has_trimmed_content(line) {
            cursor += 1;
            continue;
        }
        let indent = leading_indent(line)?;
        if indent == 0 {
            break;
        }
        let field = parse_frontmatter_field_line(line.trim(), |trimmed| {
            format!("frontmatter environment entry `{trimmed}` must use `key: value` syntax")
        })?;
        let (values, next_index) =
            parse_string_list_or_keys_with_parent(lines, cursor, field.raw_value, indent)?;
        match field.key {
            "allow" | "allowlist" | "allowed" => policy.allow = values,
            "deny" | "denylist" | "denied" => policy.deny = values,
            _ => {}
        }
        cursor = next_index;
    }
    Ok((policy, cursor))
}

fn parse_frontmatter_field_line<'a>(
    line: &'a str,
    syntax_error: impl FnOnce(&str) -> String,
) -> Result<FrontmatterFieldLine<'a>, String> {
    let trimmed = line.trim_end();
    let (key, raw_value) = trimmed
        .split_once(':')
        .ok_or_else(|| syntax_error(trimmed))?;
    Ok(FrontmatterFieldLine {
        key: key.trim(),
        raw_value: raw_value.trim_start(),
    })
}

fn parse_scalar_field(
    lines: &[&str],
    index: usize,
    raw_value: &str,
) -> Result<(String, usize), String> {
    match raw_value {
        ">" => {
            let (value, next_index) = collect_block_scalar(lines, index + 1, 0)?;
            Ok((
                value
                    .lines()
                    .map(str::trim)
                    .filter(|line| !line.is_empty())
                    .collect::<Vec<_>>()
                    .join(" "),
                next_index,
            ))
        }
        "|" => collect_block_scalar(lines, index + 1, 0),
        "" => {
            if next_line_is_indented(lines, index + 1)? {
                return Err("frontmatter scalar fields must not use nested mappings".to_string());
            }
            Ok((String::new(), index + 1))
        }
        value => Ok((parse_inline_scalar(value), index + 1)),
    }
}

fn parse_metadata_field(lines: &[&str], index: usize, raw_value: &str) -> Result<usize, String> {
    if !raw_value.is_empty() {
        return Err("frontmatter `metadata` must be a string-to-string map".to_string());
    }

    let mut cursor = index + 1;
    while cursor < lines.len() {
        let line = lines[cursor];
        if !has_trimmed_content(line) {
            cursor += 1;
            continue;
        }

        let indent = leading_indent(line)?;
        if indent == 0 {
            break;
        }

        let field = parse_frontmatter_field_line(line.trim(), |trimmed| {
            format!("frontmatter `metadata` entry `{trimmed}` must use `key: value` syntax")
        })?;
        if !has_trimmed_content(field.key) {
            return Err("frontmatter `metadata` keys must not be empty".to_string());
        }

        match field.raw_value {
            ">" | "|" => {
                let (_, next_index) = collect_block_scalar(lines, cursor + 1, indent)?;
                cursor = next_index;
            }
            "" => {
                if next_line_has_deeper_indent(lines, cursor + 1, indent)? {
                    return Err("frontmatter `metadata` must be a string-to-string map".to_string());
                }
                cursor += 1;
            }
            _ => {
                cursor += 1;
            }
        }
    }

    Ok(cursor)
}

fn skip_unknown_field(lines: &[&str], index: usize, raw_value: &str) -> Result<usize, String> {
    match raw_value {
        ">" | "|" => {
            let (_, next_index) = collect_block_scalar(lines, index + 1, 0)?;
            Ok(next_index)
        }
        "" => {
            let mut cursor = index + 1;
            while cursor < lines.len() {
                let line = lines[cursor];
                if !has_trimmed_content(line) {
                    cursor += 1;
                    continue;
                }
                if leading_indent(line)? == 0 {
                    break;
                }
                cursor += 1;
            }
            Ok(cursor)
        }
        _ => Ok(index + 1),
    }
}

fn collect_block_scalar(
    lines: &[&str],
    start_index: usize,
    parent_indent: usize,
) -> Result<(String, usize), String> {
    let mut cursor = start_index;
    let mut block_indent = None;
    let mut values = Vec::new();

    while cursor < lines.len() {
        let line = lines[cursor];
        if !has_trimmed_content(line) {
            values.push(String::new());
            cursor += 1;
            continue;
        }

        let indent = leading_indent(line)?;
        if indent <= parent_indent {
            break;
        }

        let block_indent = *block_indent.get_or_insert(indent);
        if indent < block_indent {
            break;
        }
        values.push(line[block_indent..].to_string());
        cursor += 1;
    }

    Ok((values.join("\n"), cursor))
}

fn next_line_is_indented(lines: &[&str], start_index: usize) -> Result<bool, String> {
    Ok(next_non_empty_line_indent(lines, start_index)?.is_some_and(|indent| indent > 0))
}

fn next_line_has_deeper_indent(
    lines: &[&str],
    start_index: usize,
    current_indent: usize,
) -> Result<bool, String> {
    Ok(next_non_empty_line_indent(lines, start_index)?
        .is_some_and(|indent| indent > current_indent))
}

fn next_non_empty_line_indent(lines: &[&str], start_index: usize) -> Result<Option<usize>, String> {
    for line in &lines[start_index..] {
        if !has_trimmed_content(line) {
            continue;
        }
        return leading_indent(line).map(Some);
    }
    Ok(None)
}

fn leading_indent(line: &str) -> Result<usize, String> {
    let indent = line
        .chars()
        .take_while(|ch| matches!(ch, ' ' | '\t'))
        .count();
    if line[..indent].contains('\t') {
        return Err("frontmatter indentation must use spaces".to_string());
    }
    Ok(indent)
}

fn parse_inline_scalar(value: &str) -> String {
    let trimmed = value.trim();
    if trimmed.len() >= 2 {
        let bytes = trimmed.as_bytes();
        if (bytes[0] == b'"' && bytes[trimmed.len() - 1] == b'"')
            || (bytes[0] == b'\'' && bytes[trimmed.len() - 1] == b'\'')
        {
            return trimmed[1..trimmed.len() - 1].to_string();
        }
    }
    trimmed.to_string()
}

fn parse_inline_list_or_scalar(value: &str) -> Vec<String> {
    let trimmed = value.trim();
    if trimmed.starts_with('[') && trimmed.ends_with(']') {
        return dedup_non_empty(
            trimmed[1..trimmed.len() - 1]
                .split(',')
                .map(parse_inline_scalar)
                .collect(),
        );
    }
    dedup_non_empty(vec![parse_inline_scalar(trimmed)])
}

fn dedup_non_empty(values: Vec<String>) -> Vec<String> {
    let mut seen = BTreeSet::new();
    let mut deduped = Vec::new();
    for value in values {
        let trimmed = value.trim();
        if trimmed.is_empty() {
            continue;
        }
        if seen.insert(trimmed.to_string()) {
            deduped.push(trimmed.to_string());
        }
    }
    deduped
}

fn validate_skill_name(name: &str, skill_file: &Path) -> Result<(), String> {
    let len = name.chars().count();
    if !(1..=64).contains(&len) {
        return Err(format!(
            "skill {} frontmatter `name` must be 1-64 characters",
            skill_file.display()
        ));
    }
    if name.starts_with('-') || name.ends_with('-') || name.contains("--") {
        return Err(format!(
            "skill {} frontmatter `name` must use single hyphen separators",
            skill_file.display()
        ));
    }
    if !name
        .chars()
        .all(|ch| ch.is_ascii_lowercase() || ch.is_ascii_digit() || ch == '-')
    {
        return Err(format!(
            "skill {} frontmatter `name` must match `^[a-z0-9]+(-[a-z0-9]+)*$`",
            skill_file.display()
        ));
    }
    Ok(())
}
