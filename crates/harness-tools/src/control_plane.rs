// allow: SIZE_OK — control plane tools (question + skill + todos)
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use harness_core::question_answers::{validate_question_answers, QuestionAnswerPrompt};
use harness_core::tool::{ToolContext, ToolError, ToolResult};
use harness_core::ToolResultExt;
use schemars::JsonSchema;
use serde::de::Error as _;
use serde::{Deserialize, Deserializer, Serialize};
use serde_json::{json, Value};

use crate::question_env::{
    coordinator_question_answer_source, question_answers_from_source_or_request,
    QuestionAnswerSource,
};
use crate::skill_catalog::{
    activate_skill, discover_skill_catalog, ActivatedSkill, SkillCatalog, SkillCatalogEntry,
    SkillCatalogStatus,
};
use crate::text::has_trimmed_content;

const TODO_STATE_FILE: &str = "control-plane/todos.json";
const QUESTION_STATE_DIR: &str = "control-plane/questions";
const SKILL_LOAD_CONFIRM_YES: &str = "Yes";
const SKILL_LOAD_CONFIRM_NO: &str = "No";
const TODO_STATUSES: &[&str] = &["pending", "in_progress", "completed", "cancelled"];
const TODO_PRIORITIES: &[&str] = &["high", "medium", "low"];

pub(crate) struct ControlPlaneExecutor {
    question_answer_source: std::sync::Arc<dyn QuestionAnswerSource>,
}

impl ControlPlaneExecutor {
    pub(crate) fn new() -> Self {
        Self::with_question_answer_source(coordinator_question_answer_source())
    }

    pub(crate) fn with_question_answer_source(
        question_answer_source: std::sync::Arc<dyn QuestionAnswerSource>,
    ) -> Self {
        Self {
            question_answer_source,
        }
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
    ) -> Result<ToolResult, ToolError> {
        let catalog = discover_skill_catalog(ctx.workspace_root.as_path())?;
        let metadata = resolve_loadable_skill_metadata(
            ctx,
            name,
            &catalog,
            self.question_answer_source.as_ref(),
        )
        .await?;
        let skill = TaskSkillContext::from(activate_skill(metadata)?);
        Ok(crate::text_json_tool_result(
            render_task_skill_context(&skill),
            json!({
                "name": skill.name,
                "location": skill.location.display().to_string(),
                "metadata": skill.metadata,
            }),
        ))
    }

    pub(crate) fn invalid_tool(&self, tool: &str, error: &str) -> ToolResult {
        ToolResult::text(format!(
            "The arguments provided to the tool are invalid: {} ({})",
            error, tool
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
        .tool_err("failed to write question state")?;

        let answers = question_answers_from_source_or_request(
            self.question_answer_source.as_ref(),
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

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct TaskSkillContext {
    pub(crate) name: String,
    pub(crate) description: String,
    pub(crate) content: String,
    pub(crate) location: PathBuf,
    pub(crate) metadata: SkillCatalogEntry,
}

impl From<ActivatedSkill> for TaskSkillContext {
    fn from(skill: ActivatedSkill) -> Self {
        Self {
            name: skill.metadata.name.clone(),
            description: skill.metadata.description.clone(),
            content: skill.content,
            location: skill.metadata.location.clone(),
            metadata: skill.metadata,
        }
    }
}

pub(crate) fn render_task_skill_context(skill: &TaskSkillContext) -> String {
    format!(
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
    )
}

fn todo_state_path(ctx: &ToolContext) -> Result<PathBuf, ToolError> {
    run_root(ctx).map(|root| root.join(TODO_STATE_FILE))
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
    .tool_err("failed to parse todo state")
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
    let non_completed = todos.iter().filter(|t| t.status != "completed").count();
    let display_text = serde_json::to_string_pretty(&todos).tool_err("failed to render todos")?;
    Ok(crate::text_json_tool_result(
        display_text,
        json!({ "todos": todos, "title": format!("{} todos", non_completed) }),
    ))
}

fn write_todo_state(ctx: &ToolContext, todos: &[TodoItem]) -> Result<(), ToolError> {
    let path = todo_state_path(ctx)?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|err| {
            ToolError::Execution(format!("failed to create todo state directory: {err}"))
        })?;
    }
    let bytes = serde_json::to_vec_pretty(todos).tool_err("failed to serialize todos")?;
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
    question_answer_source: &dyn QuestionAnswerSource,
) -> Result<Vec<TaskSkillContext>, ToolError> {
    if names.is_empty() {
        return Ok(Vec::new());
    }

    let catalog = discover_skill_catalog(ctx.workspace_root.as_path())?;
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

        let metadata =
            resolve_loadable_skill_metadata(ctx, name, &catalog, question_answer_source).await?;
        resolved.push(TaskSkillContext::from(activate_skill(metadata)?));
    }

    Ok(resolved)
}

async fn resolve_loadable_skill_metadata<'a>(
    ctx: &ToolContext,
    name: &str,
    catalog: &'a SkillCatalog,
    question_answer_source: &dyn QuestionAnswerSource,
) -> Result<&'a SkillCatalogEntry, ToolError> {
    let entry = catalog
        .active_entry(name)
        .ok_or_else(|| ToolError::Execution(skill_not_found_message(name, catalog)))?;
    if entry.status != SkillCatalogStatus::Loadable {
        return Err(ToolError::Execution(skill_unavailable_message(
            entry, catalog,
        )));
    }
    if entry.permission_mode == "ask" {
        request_skill_load_approval(ctx, name, question_answer_source).await?;
    }
    Ok(entry)
}

async fn request_skill_load_approval(
    ctx: &ToolContext,
    name: &str,
    question_answer_source: &dyn QuestionAnswerSource,
) -> Result<(), ToolError> {
    let questions = vec![skill_load_confirmation_question(name)];
    let answers = question_answers_from_source_or_request(
        question_answer_source,
        ctx,
        json!({ "questions": questions }),
        |err| {
            ToolError::Execution(format!(
                "Skill \"{name}\" approval failed before loading: {err}"
            ))
        },
    )
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

fn skill_not_found_message(name: &str, catalog: &SkillCatalog) -> String {
    let trimmed = name.trim();
    let mut message = format!("Skill \"{trimmed}\" not found");

    if let Some(agent_name) = known_agent_name(trimmed) {
        message.push_str(&format!(
            ". `{trimmed}` is an agent, not a skill; use task(category=\"{agent_name}\", ...) if you need a child session"
        ));
    }

    let visible = catalog.loadable_names();
    if visible.is_empty() {
        message.push_str(". No skills are currently available.");
    } else {
        message.push_str(&format!(". Available skills: {}", visible.join(", ")));
    }

    message
}

fn skill_unavailable_message(entry: &SkillCatalogEntry, catalog: &SkillCatalog) -> String {
    let mut message = format!(
        "Skill \"{}\" not found: skill is {}",
        entry.name,
        entry.status.as_str()
    );
    if let Some(reason) = &entry.reason {
        message.push_str(&format!(" ({reason})"));
    }
    let visible = catalog.loadable_names();
    if visible.is_empty() {
        message.push_str(". No skills are currently available.");
    } else {
        message.push_str(&format!(". Available skills: {}", visible.join(", ")));
    }
    message
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
