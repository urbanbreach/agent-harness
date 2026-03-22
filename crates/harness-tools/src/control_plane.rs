use std::path::{Path, PathBuf};
use std::sync::Arc;

use async_trait::async_trait;
use harness_core::tool::{Tool, ToolCapability, ToolContext, ToolError, ToolResult};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

// Native control-plane tools intentionally keep the legacy compat path contract so
// canonical native IDs and compat aliases share one persisted state layout during
// migration, replay, and resume.
const TODO_STATE_FILE: &str = "opencode-compat/todos.json";
const QUESTION_STATE_DIR: &str = "opencode-compat/questions";
const QUESTION_ANSWERS_ENV_VAR: &str = "HARNESS_QUESTION_ANSWERS";
const PLAN_EXIT_BUILD_FALLBACK_PROFILE: &str = "build";
const PLAN_EXIT_CONFIRM_YES: &str = "Yes";
const PLAN_EXIT_CONFIRM_NO: &str = "No";
const PLAN_EXIT_SYNTHETIC_PROMPT: &str =
    "The plan has been approved, you can now edit files. Execute the plan.";

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

    pub(crate) fn load_skill(
        &self,
        name: &str,
        user_message: Option<String>,
    ) -> Result<ToolResult, ToolError> {
        let skill = discover_skills()?
            .into_iter()
            .find(|skill| skill.name == name)
            .ok_or_else(|| ToolError::Execution(format!("Skill \"{name}\" not found")))?;
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
                ToolError::Execution(
                    "plan.exit requires an active category/profile context".to_string(),
                )
            })?;
        if !ctx.plan_mode {
            return Err(ToolError::Execution(format!(
                "plan.exit is only available for plan-mode profiles; `{source_profile}` is not plan-capable"
            )));
        }
        let target_profile = ctx
            .plan_exit_target_profile
            .as_deref()
            .filter(|value| !value.trim().is_empty())
            .ok_or_else(|| {
                ToolError::Execution(format!(
                    "plan.exit for `{source_profile}` requires a configured exit target profile or an available `{PLAN_EXIT_BUILD_FALLBACK_PROFILE}` profile"
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

pub(crate) struct TodoWriteTool {
    executor: Arc<ControlPlaneExecutor>,
}

impl TodoWriteTool {
    pub(crate) fn new(executor: Arc<ControlPlaneExecutor>) -> Self {
        Self { executor }
    }
}

pub(crate) struct TodoReadTool {
    executor: Arc<ControlPlaneExecutor>,
}

impl TodoReadTool {
    pub(crate) fn new(executor: Arc<ControlPlaneExecutor>) -> Self {
        Self { executor }
    }
}

pub(crate) struct SkillLoadTool {
    executor: Arc<ControlPlaneExecutor>,
}

impl SkillLoadTool {
    pub(crate) fn new(executor: Arc<ControlPlaneExecutor>) -> Self {
        Self { executor }
    }
}

pub(crate) struct InvalidTool {
    executor: Arc<ControlPlaneExecutor>,
}

impl InvalidTool {
    pub(crate) fn new(executor: Arc<ControlPlaneExecutor>) -> Self {
        Self { executor }
    }
}

pub(crate) struct UserQuestionTool {
    executor: Arc<ControlPlaneExecutor>,
}

impl UserQuestionTool {
    pub(crate) fn new(executor: Arc<ControlPlaneExecutor>) -> Self {
        Self { executor }
    }
}

pub(crate) struct PlanExitTool {
    executor: Arc<ControlPlaneExecutor>,
}

impl PlanExitTool {
    pub(crate) fn new(executor: Arc<ControlPlaneExecutor>) -> Self {
        Self { executor }
    }
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct TodoWriteArgs {
    todos: Vec<TodoItem>,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct SkillLoadArgs {
    name: String,
    #[serde(default)]
    user_message: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct InvalidArgs {
    tool: String,
    error: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct UserQuestionArgs {
    questions: Vec<QuestionPrompt>,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct PlanExitArgs {}

#[async_trait]
impl Tool for TodoWriteTool {
    fn id(&self) -> &str {
        "todo.write"
    }

    fn description(&self) -> &str {
        "Stores a per-run todo list."
    }

    fn parameters_json_schema(&self) -> Value {
        super::json_schema_for::<TodoWriteArgs>()
    }

    fn capability(&self) -> ToolCapability {
        ToolCapability::ReadFs
    }

    async fn call(&self, ctx: ToolContext, args_json: Value) -> Result<ToolResult, ToolError> {
        let args: TodoWriteArgs = serde_json::from_value(args_json)
            .map_err(|err| ToolError::InvalidArguments(err.to_string()))?;
        self.executor.write_todos(&ctx, args.todos)
    }
}

#[async_trait]
impl Tool for TodoReadTool {
    fn id(&self) -> &str {
        "todo.read"
    }

    fn description(&self) -> &str {
        "Reads the per-run todo list."
    }

    fn parameters_json_schema(&self) -> Value {
        json!({"type": "object", "additionalProperties": false})
    }

    fn capability(&self) -> ToolCapability {
        ToolCapability::ReadFs
    }

    async fn call(&self, ctx: ToolContext, _args_json: Value) -> Result<ToolResult, ToolError> {
        self.executor.read_todos(&ctx)
    }
}

#[async_trait]
impl Tool for SkillLoadTool {
    fn id(&self) -> &str {
        "skill.load"
    }

    fn description(&self) -> &str {
        "Loads user-installed skills from configured skill directories."
    }

    fn parameters_json_schema(&self) -> Value {
        super::json_schema_for::<SkillLoadArgs>()
    }

    fn capability(&self) -> ToolCapability {
        ToolCapability::ReadFs
    }

    async fn call(&self, _ctx: ToolContext, args_json: Value) -> Result<ToolResult, ToolError> {
        let args: SkillLoadArgs = serde_json::from_value(args_json)
            .map_err(|err| ToolError::InvalidArguments(err.to_string()))?;
        self.executor.load_skill(&args.name, args.user_message)
    }
}

#[async_trait]
impl Tool for InvalidTool {
    fn id(&self) -> &str {
        "tool.invalid"
    }

    fn description(&self) -> &str {
        "Builds a deterministic invalid-tool response."
    }

    fn parameters_json_schema(&self) -> Value {
        super::json_schema_for::<InvalidArgs>()
    }

    fn capability(&self) -> ToolCapability {
        ToolCapability::ReadFs
    }

    async fn call(&self, _ctx: ToolContext, args_json: Value) -> Result<ToolResult, ToolError> {
        let args: InvalidArgs = serde_json::from_value(args_json)
            .map_err(|err| ToolError::InvalidArguments(err.to_string()))?;
        Ok(self.executor.invalid_tool(&args.tool, &args.error))
    }
}

#[async_trait]
impl Tool for UserQuestionTool {
    fn id(&self) -> &str {
        "user.question"
    }

    fn description(&self) -> &str {
        "Asks structured questions and waits for answers from the coordinator."
    }

    fn parameters_json_schema(&self) -> Value {
        super::json_schema_for::<UserQuestionArgs>()
    }

    fn capability(&self) -> ToolCapability {
        ToolCapability::ReadFs
    }

    async fn call(&self, ctx: ToolContext, args_json: Value) -> Result<ToolResult, ToolError> {
        let args: UserQuestionArgs = serde_json::from_value(args_json)
            .map_err(|err| ToolError::InvalidArguments(err.to_string()))?;
        self.executor.user_question(&ctx, args.questions).await
    }
}

#[async_trait]
impl Tool for PlanExitTool {
    fn id(&self) -> &str {
        "plan.exit"
    }

    fn description(&self) -> &str {
        "Requests approval to leave plan mode and hand off to the configured build profile."
    }

    fn parameters_json_schema(&self) -> Value {
        super::json_schema_for::<PlanExitArgs>()
    }

    fn capability(&self) -> ToolCapability {
        ToolCapability::ReadFs
    }

    async fn call(&self, ctx: ToolContext, _args_json: Value) -> Result<ToolResult, ToolError> {
        self.executor.plan_exit(&ctx).await
    }
}

#[derive(Debug, Deserialize, Serialize, JsonSchema, Clone, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub(crate) struct TodoItem {
    pub(crate) content: String,
    pub(crate) status: String,
    pub(crate) priority: String,
}

#[derive(Debug, Deserialize, Serialize, JsonSchema, Clone, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub(crate) struct QuestionPrompt {
    pub(crate) question: String,
    pub(crate) header: String,
    pub(crate) options: Vec<QuestionOption>,
    #[serde(default)]
    pub(crate) multiple: Option<bool>,
}

#[derive(Debug, Deserialize, Serialize, JsonSchema, Clone, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub(crate) struct QuestionOption {
    pub(crate) label: String,
    pub(crate) description: String,
}

#[derive(Debug, Clone)]
struct SkillRecord {
    name: String,
    description: String,
    content: String,
    location: PathBuf,
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

fn discover_skills() -> Result<Vec<SkillRecord>, ToolError> {
    let mut dirs = Vec::new();
    if let Some(home) = std::env::var_os("HOME") {
        let home = PathBuf::from(home);
        dirs.push(home.join(".config/opencode/skills"));
        dirs.push(home.join(".agents/skills"));
    }
    let mut skills = Vec::new();
    for dir in dirs {
        if !dir.exists() {
            continue;
        }
        for entry in std::fs::read_dir(&dir).map_err(|err| {
            ToolError::Execution(format!(
                "failed to read skill directory {}: {err}",
                dir.display()
            ))
        })? {
            let entry = entry.map_err(|err| {
                ToolError::Execution(format!("failed to read skill entry: {err}"))
            })?;
            let skill_dir = entry.path();
            let skill_file = skill_dir.join("SKILL.md");
            if !skill_file.exists() {
                continue;
            }
            let content = std::fs::read_to_string(&skill_file).map_err(|err| {
                ToolError::Execution(format!(
                    "failed to read skill file {}: {err}",
                    skill_file.display()
                ))
            })?;
            let description = content
                .lines()
                .find(|line| !line.trim().is_empty() && !line.starts_with('#'))
                .unwrap_or("No description available.")
                .trim()
                .to_string();
            skills.push(SkillRecord {
                name: entry.file_name().to_string_lossy().to_string(),
                description,
                content,
                location: skill_file,
            });
        }
    }
    skills.sort_by(|left, right| left.name.cmp(&right.name));
    Ok(skills)
}
