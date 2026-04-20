use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use harness_core::config::{registered_skills_config, PermissionMode};
use harness_core::tool::{ToolContext, ToolError, ToolResult};
use schemars::JsonSchema;
use serde::de::Error as _;
use serde::{Deserialize, Deserializer, Serialize};
use serde_json::{json, Value};

const TODO_STATE_FILE: &str = "control-plane/todos.json";
const QUESTION_STATE_DIR: &str = "control-plane/questions";
const QUESTION_ANSWERS_ENV_VAR: &str = "HARNESS_QUESTION_ANSWERS";
const PLAN_EXIT_CONFIRM_YES: &str = "Yes";
const PLAN_EXIT_CONFIRM_NO: &str = "No";
const PLAN_EXIT_SYNTHETIC_PROMPT: &str =
    "The plan has been approved, you can now edit files. Execute the plan.";
const SKILL_LOAD_CONFIRM_YES: &str = "Yes";
const SKILL_LOAD_CONFIRM_NO: &str = "No";

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
        Ok(ToolResult {
            display_text: serde_json::to_string_pretty(&todos)
                .map_err(|err| ToolError::Execution(format!("failed to render todos: {err}")))?,
            structured_json: Some(json!({ "todos": todos })),
            artifacts: Vec::new(),
        })
    }

    pub(crate) fn read_todos(&self, ctx: &ToolContext) -> Result<ToolResult, ToolError> {
        let todos = read_todo_state(ctx)?;
        Ok(ToolResult {
            display_text: serde_json::to_string_pretty(&todos)
                .map_err(|err| ToolError::Execution(format!("failed to render todos: {err}")))?,
            structured_json: Some(json!({ "todos": todos })),
            artifacts: Vec::new(),
        })
    }

    pub(crate) async fn load_skill(
        &self,
        ctx: &ToolContext,
        name: &str,
        user_message: Option<String>,
    ) -> Result<ToolResult, ToolError> {
        let mut catalog = discover_skill_catalog()?;
        let skill_not_found = skill_not_found_message(name, &catalog);
        let skill = match catalog
            .remove(name)
            .ok_or_else(|| ToolError::Execution(skill_not_found.clone()))?
        {
            DiscoveredSkill::Visible(skill) => match skill.permission {
                PermissionMode::Allow => skill,
                PermissionMode::Ask => {
                    self.request_skill_load_approval(ctx, name).await?;
                    skill
                }
                PermissionMode::Deny => {
                    return Err(ToolError::Execution(skill_not_found));
                }
            },
            DiscoveredSkill::Denied | DiscoveredSkill::Invalid => {
                return Err(ToolError::Execution(skill_not_found));
            }
        };
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
        if let Some(user_message) = user_message {
            output.push_str(&format!(
                "\n\n<skill_user_message>{user_message}</skill_user_message>"
            ));
        }
        Ok(ToolResult {
            display_text: output,
            structured_json: Some(json!({
                "name": skill.name,
                "location": skill.location.display().to_string(),
            })),
            artifacts: Vec::new(),
        })
    }

    async fn request_skill_load_approval(
        &self,
        ctx: &ToolContext,
        name: &str,
    ) -> Result<(), ToolError> {
        let questions = vec![skill_load_confirmation_question(name)];
        let answers = match read_question_answers_from_env()? {
            Some(answers) => answers,
            None => ctx
                .coordinator
                .request_question(
                    ctx.actor.clone(),
                    ctx.tool_call_id.clone(),
                    json!({ "questions": questions }),
                )
                .await
                .map_err(|err| {
                    ToolError::Execution(format!(
                        "Skill \"{name}\" approval failed before loading: {err}"
                    ))
                })?,
        };
        let answers =
            validate_question_answers(&questions, answers).map_err(ToolError::Execution)?;
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
        .map_err(|err| ToolError::Execution(format!("failed to write question state: {err}")))?;

        let answers = match read_question_answers_from_env()? {
            Some(answers) => answers,
            None => ctx
                .coordinator
                .request_question(
                    ctx.actor.clone(),
                    ctx.tool_call_id.clone(),
                    json!({ "questions": questions }),
                )
                .await
                .map_err(ToolError::Execution)?,
        };
        let answers =
            validate_question_answers(&questions, answers).map_err(ToolError::Execution)?;
        let formatted = format_question_answers(&questions, &answers);
        let display_text = format!(
            "User has answered your questions: {formatted}. You can now continue with the user's answers in mind."
        );
        Ok(ToolResult {
            display_text: display_text.clone(),
            structured_json: Some(json!({
                "questions": questions,
                "answers": answers,
                "output": display_text,
                "state_path": state_path.display().to_string(),
            })),
            artifacts: Vec::new(),
        })
    }

    pub(crate) async fn plan_exit(&self, ctx: &ToolContext) -> Result<ToolResult, ToolError> {
        let source_profile = ctx
            .category
            .as_deref()
            .filter(|value| !value.trim().is_empty())
            .ok_or_else(|| {
                ToolError::Execution("plan.exit requires an active agent context".to_string())
            })?;
        if !ctx.plan_mode {
            return Err(ToolError::Execution(format!(
                "plan.exit is only available for plan-mode agents; `{source_profile}` is not plan-capable"
            )));
        }
        let target_profile = ctx
            .plan_exit_target_profile
            .as_deref()
            .filter(|value| !value.trim().is_empty())
            .ok_or_else(|| {
                ToolError::Execution(format!(
                    "plan.exit for `{source_profile}` requires a configured exit target agent"
                ))
            })?;

        let questions = vec![plan_exit_confirmation_question(target_profile)];
        let answers = match read_question_answers_from_env()? {
            Some(answers) => answers,
            None => ctx
                .coordinator
                .request_question(
                    ctx.actor.clone(),
                    ctx.tool_call_id.clone(),
                    json!({ "questions": questions }),
                )
                .await
                .map_err(ToolError::Execution)?,
        };
        let answers =
            validate_question_answers(&questions, answers).map_err(ToolError::Execution)?;
        let approved = answers
            .first()
            .and_then(|answer| answer.first())
            .is_some_and(|answer| answer == PLAN_EXIT_CONFIRM_YES);
        if !approved {
            return Err(ToolError::Execution(
                "plan.exit cancelled by user confirmation".to_string(),
            ));
        }

        Ok(ToolResult {
            display_text: format!(
                "User approved switching from `{source_profile}` to `{target_profile}`. The implementation handoff is ready."
            ),
            structured_json: Some(json!({
                "plan_exit_handoff": {
                    "source_profile": source_profile,
                    "target_profile": target_profile,
                    "prompt": PLAN_EXIT_SYNTHETIC_PROMPT,
                },
                "confirmed": true,
            })),
            artifacts: Vec::new(),
        })
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
    #[serde(default)]
    required: Option<bool>,
    #[serde(default)]
    id: Option<Value>,
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
    status: String,
    #[serde(default)]
    priority: Option<String>,
    #[serde(default)]
    id: Option<Value>,
}

impl<'de> Deserialize<'de> for TodoItem {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let compat = TodoItemCompat::deserialize(deserializer)?;
        let _ = compat.id;
        let content = compat
            .content
            .or(compat.text)
            .or(compat.title)
            .ok_or_else(|| D::Error::custom("missing field `content`"))?;
        Ok(Self {
            content,
            status: compat.status,
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
        let _ = compat.required;
        let _ = compat.id;
        let question = compat
            .question
            .or(compat.prompt)
            .or(compat.text)
            .ok_or_else(|| D::Error::custom("missing field `question`"))?;
        let options = compat
            .options
            .or(compat.choices)
            .or(compat.answers)
            .ok_or_else(|| D::Error::custom("missing field `options`"))?;
        let header = compat
            .header
            .or(compat.title)
            .filter(|value| !value.trim().is_empty())
            .unwrap_or_else(|| question.clone());

        Ok(Self {
            question,
            header,
            options: options.into_iter().map(Into::into).collect(),
            multiple: compat.multiple,
        })
    }
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
}

#[derive(Debug, Clone)]
enum DiscoveredSkill {
    Visible(SkillRecord),
    Denied,
    Invalid,
}

#[derive(Debug, Default)]
struct SkillFrontmatter {
    name: Option<String>,
    description: Option<String>,
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
    Ok(())
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

fn question_state_path(ctx: &ToolContext) -> Result<PathBuf, ToolError> {
    run_root(ctx).map(|root| {
        root.join(QUESTION_STATE_DIR)
            .join(format!("{}.json", ctx.tool_call_id))
    })
}

fn read_question_answers_from_env() -> Result<Option<Vec<Vec<String>>>, ToolError> {
    std::env::var(QUESTION_ANSWERS_ENV_VAR)
        .ok()
        .map(|value| serde_json::from_str::<Vec<Vec<String>>>(&value))
        .transpose()
        .map_err(|err| {
            ToolError::Execution(format!("failed to parse {QUESTION_ANSWERS_ENV_VAR}: {err}"))
        })
}

fn validate_question_prompts(questions: &[QuestionPrompt]) -> Result<(), String> {
    if questions.is_empty() {
        return Err("at least one question is required".to_string());
    }

    Ok(())
}

fn skill_not_found_message(name: &str, catalog: &BTreeMap<String, DiscoveredSkill>) -> String {
    let trimmed = name.trim();
    let mut message = format!("Skill \"{trimmed}\" not found");

    if let Some(agent_name) = known_agent_name(trimmed) {
        message.push_str(&format!(
            ". `{trimmed}` is an agent, not a skill; use task(subagent_type=\"{agent_name}\", ...) or @{agent_name} instead"
        ));
    }

    let visible = catalog
        .iter()
        .filter_map(|(name, skill)| {
            matches!(skill, DiscoveredSkill::Visible(_)).then_some(name.as_str())
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

fn known_agent_name(name: &str) -> Option<&'static str> {
    match name.trim().to_ascii_lowercase().as_str() {
        "explore" | "explorer" => Some("explore"),
        "general" => Some("general"),
        "librarian" => Some("librarian"),
        "oracle" => Some("oracle"),
        "build" => Some("build"),
        "plan" => Some("plan"),
        _ => None,
    }
}

fn validate_question_answers(
    questions: &[QuestionPrompt],
    answers: Vec<Vec<String>>,
) -> Result<Vec<Vec<String>>, String> {
    if answers.len() != questions.len() {
        return Err(format!(
            "Expected {} answer group(s) for {} question(s); received {}.",
            questions.len(),
            questions.len(),
            answers.len()
        ));
    }

    questions
        .iter()
        .zip(answers)
        .enumerate()
        .map(|(index, (question, answers))| normalize_question_answers(index, question, answers))
        .collect()
}

fn normalize_question_answers(
    index: usize,
    question: &QuestionPrompt,
    answers: Vec<String>,
) -> Result<Vec<String>, String> {
    let answers = answers
        .into_iter()
        .map(|answer| answer.trim().to_string())
        .filter(|answer| !answer.is_empty())
        .collect::<Vec<_>>();
    if answers.is_empty() {
        return Ok(Vec::new());
    }

    if !question.multiple.unwrap_or(false) && answers.len() != 1 {
        return Err(format!(
            "Question {} ({}) accepts only one answer.",
            index + 1,
            question.header
        ));
    }

    Ok(answers
        .into_iter()
        .map(|answer| canonicalize_question_answer(question, answer))
        .collect())
}

fn canonicalize_question_answer(question: &QuestionPrompt, answer: String) -> String {
    question
        .options
        .iter()
        .find(|option| option.label.eq_ignore_ascii_case(&answer))
        .map(|option| option.label.clone())
        .unwrap_or(answer)
}

fn plan_exit_confirmation_question(target_profile: &str) -> QuestionPrompt {
    QuestionPrompt {
        question: format!(
            "Planning is complete. Would you like to switch to the {target_profile} agent and start implementing?"
        ),
        header: format!("{} Agent", title_case_label(target_profile)),
        options: vec![
            QuestionOption {
                label: PLAN_EXIT_CONFIRM_YES.to_string(),
                description: format!(
                    "Switch to the {target_profile} agent and start implementing the approved plan"
                ),
            },
            QuestionOption {
                label: PLAN_EXIT_CONFIRM_NO.to_string(),
                description: "Stay in the current planning session".to_string(),
            },
        ],
        multiple: Some(false),
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

fn title_case_label(label: &str) -> String {
    let mut chars = label.chars();
    let Some(first) = chars.next() else {
        return String::new();
    };
    first.to_uppercase().collect::<String>() + chars.as_str()
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

fn discover_skill_catalog() -> Result<BTreeMap<String, DiscoveredSkill>, ToolError> {
    let config = registered_skills_config();
    let current_dir = std::env::current_dir().map_err(|err| {
        ToolError::Execution(format!("failed to determine current directory: {err}"))
    })?;
    let mut catalog = BTreeMap::new();
    for dir in skill_search_dirs(&current_dir, &config) {
        if !dir.exists() {
            continue;
        }

        for entry in sorted_skill_entries(&dir)? {
            let name = entry.file_name().to_string_lossy().to_string();
            if catalog.contains_key(&name) {
                continue;
            }

            let permission = resolve_skill_permission(&name, &config.permissions);
            if permission == PermissionMode::Deny {
                catalog.insert(name, DiscoveredSkill::Denied);
                continue;
            }

            let skill_file = entry.path().join("SKILL.md");
            if !skill_file.exists() {
                continue;
            }

            let content = std::fs::read_to_string(&skill_file).map_err(|err| {
                ToolError::Execution(format!(
                    "failed to read skill file {}: {err}",
                    skill_file.display()
                ))
            })?;

            match build_skill_record(&name, &skill_file, &content, permission.clone()) {
                Ok(skill) => {
                    catalog.insert(name, DiscoveredSkill::Visible(skill));
                }
                Err(_) => {
                    catalog.insert(name, DiscoveredSkill::Invalid);
                }
            }
        }
    }

    Ok(catalog)
}

fn skill_search_dirs(
    current_dir: &Path,
    config: &harness_core::config::SkillsConfig,
) -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    for base_dir in project_search_bases(current_dir, config.walk_to_git_root) {
        for root in &config.project_roots {
            push_unique_path(&mut dirs, resolve_skill_root(&base_dir, root));
        }
    }
    for root in &config.global_roots {
        push_unique_path(&mut dirs, resolve_skill_root(current_dir, root));
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

fn push_unique_path(paths: &mut Vec<PathBuf>, candidate: PathBuf) {
    if !paths.iter().any(|existing| existing == &candidate) {
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
        if line.trim().is_empty() {
            index += 1;
            continue;
        }
        if leading_indent(line)? != 0 {
            return Err(format!(
                "frontmatter line `{}` must not be indented",
                line.trim()
            ));
        }

        let trimmed = line.trim_end();
        let (key, raw_value) = trimmed
            .split_once(':')
            .ok_or_else(|| format!("frontmatter line `{trimmed}` must use `key: value` syntax"))?;
        let key = key.trim();
        let raw_value = raw_value.trim_start();

        match key {
            "name" => {
                let (value, next_index) = parse_scalar_field(lines, index, raw_value)?;
                frontmatter.name = Some(value);
                index = next_index;
            }
            "description" => {
                let (value, next_index) = parse_scalar_field(lines, index, raw_value)?;
                frontmatter.description = Some(value);
                index = next_index;
            }
            "license" | "compatibility" => {
                let (_, next_index) = parse_scalar_field(lines, index, raw_value)?;
                index = next_index;
            }
            "metadata" => {
                index = parse_metadata_field(lines, index, raw_value)?;
            }
            _ => {
                index = skip_unknown_field(lines, index, raw_value)?;
            }
        }
    }

    Ok(frontmatter)
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
        if line.trim().is_empty() {
            cursor += 1;
            continue;
        }

        let indent = leading_indent(line)?;
        if indent == 0 {
            break;
        }

        let trimmed = line.trim();
        let (key, raw_nested_value) = trimmed.split_once(':').ok_or_else(|| {
            format!("frontmatter `metadata` entry `{trimmed}` must use `key: value` syntax")
        })?;
        if key.trim().is_empty() {
            return Err("frontmatter `metadata` keys must not be empty".to_string());
        }

        let raw_nested_value = raw_nested_value.trim_start();
        match raw_nested_value {
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
                if line.trim().is_empty() {
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
        if line.trim().is_empty() {
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
    for line in &lines[start_index..] {
        if line.trim().is_empty() {
            continue;
        }
        return Ok(leading_indent(line)? > 0);
    }
    Ok(false)
}

fn next_line_has_deeper_indent(
    lines: &[&str],
    start_index: usize,
    current_indent: usize,
) -> Result<bool, String> {
    for line in &lines[start_index..] {
        if line.trim().is_empty() {
            continue;
        }
        return Ok(leading_indent(line)? > current_indent);
    }
    Ok(false)
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
