use std::fs;
use std::process::Command;

use async_trait::async_trait;
use harness_core::config::ShellAllowlist;
use harness_core::tool::{ArtifactRef, Tool, ToolCapability, ToolContext, ToolError, ToolResult};
use serde::Deserialize;
use serde_json::{json, Value};

use crate::native_tools::validate_bash_command;
use crate::{parse_tool_args, text_json_artifacts_tool_result, text_json_tool_result};

const TMUX_PREFIX: &str = "harness";
const CAPTURE_DEFAULT_START: &str = "-200";

#[derive(Debug)]
pub(crate) struct InteractiveBashTool {
    allowlist: ShellAllowlist,
}
#[derive(Debug)]
pub(crate) struct TerminalSpawnTool {
    allowlist: ShellAllowlist,
}
#[derive(Debug)]
pub(crate) struct TerminalWriteTool {
    allowlist: ShellAllowlist,
}
#[derive(Debug)]
pub(crate) struct TerminalScreenshotTool;
#[derive(Debug)]
pub(crate) struct TerminalResizeTool;
#[derive(Debug)]
pub(crate) struct TerminalKillTool;
#[derive(Debug)]
pub(crate) struct TerminalListTool;

#[derive(Debug, Deserialize)]
struct InteractiveBashArgs {
    session_id: Option<String>,
    command: Option<String>,
    input: Option<String>,
    cwd: Option<String>,
    #[serde(default = "default_enter")]
    enter: bool,
}

#[derive(Debug, Deserialize)]
struct TerminalSpawnArgs {
    session_id: Option<String>,
    command: String,
    cwd: Option<String>,
    rows: Option<u16>,
    cols: Option<u16>,
}

#[derive(Debug, Deserialize)]
struct TerminalWriteArgs {
    session_id: String,
    input: String,
    #[serde(default = "default_enter")]
    enter: bool,
}

#[derive(Debug, Deserialize)]
struct TerminalScreenshotArgs {
    session_id: String,
    start: Option<i32>,
}

#[derive(Debug, Deserialize)]
struct TerminalResizeArgs {
    session_id: String,
    rows: u16,
    cols: u16,
}

#[derive(Debug, Deserialize)]
struct TerminalKillArgs {
    session_id: String,
}

fn default_enter() -> bool {
    true
}

impl InteractiveBashTool {
    pub(crate) fn new(allowlist: ShellAllowlist) -> Self {
        Self { allowlist }
    }
}

impl TerminalSpawnTool {
    pub(crate) fn new(allowlist: ShellAllowlist) -> Self {
        Self { allowlist }
    }
}

impl TerminalWriteTool {
    pub(crate) fn new(allowlist: ShellAllowlist) -> Self {
        Self { allowlist }
    }
}

#[async_trait]
impl Tool for InteractiveBashTool {
    fn id(&self) -> &str {
        "interactive_bash"
    }

    fn description(&self) -> &str {
        "Progressive interactive terminal wrapper. Starts a tmux-backed session when command is supplied, sends input when input is supplied, and returns captured pane text as an artifact-backed screenshot. Requires tmux."
    }

    fn parameters_json_schema(&self) -> Value {
        interactive_bash_schema()
    }

    fn capability(&self) -> ToolCapability {
        ToolCapability::Shell
    }

    async fn call(&self, ctx: ToolContext, args_json: Value) -> Result<ToolResult, ToolError> {
        let args: InteractiveBashArgs = parse_tool_args(args_json)?;
        let session_id = args
            .session_id
            .unwrap_or_else(|| format!("term_{}", sanitize_fragment(&ctx.tool_call_id)));
        if let Some(command) = args.command {
            spawn_terminal(
                &ctx,
                TerminalSpawnArgs {
                    session_id: Some(session_id.clone()),
                    command,
                    cwd: args.cwd,
                    rows: None,
                    cols: None,
                },
                &self.allowlist,
            )?;
        }
        if let Some(input) = args.input {
            write_terminal(&ctx, &session_id, &input, args.enter, &self.allowlist)?;
        }
        capture_terminal(&ctx, &session_id, Some(-120))
    }
}

#[async_trait]
impl Tool for TerminalSpawnTool {
    fn id(&self) -> &str {
        "terminal_spawn"
    }

    fn description(&self) -> &str {
        "Starts a persistent tmux-backed terminal session for this run. Missing tmux returns an actionable dependency error instead of falling back to one-shot bash."
    }

    fn parameters_json_schema(&self) -> Value {
        terminal_spawn_schema()
    }

    fn capability(&self) -> ToolCapability {
        ToolCapability::Shell
    }

    async fn call(&self, ctx: ToolContext, args_json: Value) -> Result<ToolResult, ToolError> {
        let args: TerminalSpawnArgs = parse_tool_args(args_json)?;
        spawn_terminal(&ctx, args, &self.allowlist)
    }
}

#[async_trait]
impl Tool for TerminalWriteTool {
    fn id(&self) -> &str {
        "terminal_write"
    }

    fn description(&self) -> &str {
        "Sends literal input to a persistent terminal session and optionally presses Enter. Requires tmux."
    }

    fn parameters_json_schema(&self) -> Value {
        terminal_write_schema()
    }

    fn capability(&self) -> ToolCapability {
        ToolCapability::Shell
    }

    async fn call(&self, ctx: ToolContext, args_json: Value) -> Result<ToolResult, ToolError> {
        let args: TerminalWriteArgs = parse_tool_args(args_json)?;
        write_terminal(
            &ctx,
            &args.session_id,
            &args.input,
            args.enter,
            &self.allowlist,
        )?;
        Ok(text_json_tool_result(
            format!("terminal input sent: {}", args.session_id),
            json!({ "session_id": args.session_id, "entered": args.enter }),
        ))
    }
}

#[async_trait]
impl Tool for TerminalScreenshotTool {
    fn id(&self) -> &str {
        "terminal_screenshot"
    }

    fn description(&self) -> &str {
        "Captures text from a persistent terminal pane and stores the capture as a session artifact. Requires tmux."
    }

    fn parameters_json_schema(&self) -> Value {
        terminal_screenshot_schema()
    }

    fn capability(&self) -> ToolCapability {
        ToolCapability::Shell
    }

    async fn call(&self, ctx: ToolContext, args_json: Value) -> Result<ToolResult, ToolError> {
        let args: TerminalScreenshotArgs = parse_tool_args(args_json)?;
        capture_terminal(&ctx, &args.session_id, args.start)
    }
}

#[async_trait]
impl Tool for TerminalResizeTool {
    fn id(&self) -> &str {
        "terminal_resize"
    }

    fn description(&self) -> &str {
        "Resizes a persistent terminal session. Requires tmux."
    }

    fn parameters_json_schema(&self) -> Value {
        terminal_resize_schema()
    }

    fn capability(&self) -> ToolCapability {
        ToolCapability::Shell
    }

    async fn call(&self, _ctx: ToolContext, args_json: Value) -> Result<ToolResult, ToolError> {
        require_tmux()?;
        let args: TerminalResizeArgs = parse_tool_args(args_json)?;
        validate_terminal_id(&args.session_id)?;
        validate_size(args.rows, args.cols)?;
        let target = tmux_name(&_ctx.run_id, &args.session_id)?;
        run_tmux([
            "resize-window".to_string(),
            "-t".to_string(),
            target,
            "-x".to_string(),
            args.cols.to_string(),
            "-y".to_string(),
            args.rows.to_string(),
        ])?;
        Ok(text_json_tool_result(
            format!("terminal resized: {}", args.session_id),
            json!({ "session_id": args.session_id, "rows": args.rows, "cols": args.cols }),
        ))
    }
}

#[async_trait]
impl Tool for TerminalKillTool {
    fn id(&self) -> &str {
        "terminal_kill"
    }

    fn description(&self) -> &str {
        "Kills a persistent terminal session for this run. Requires tmux."
    }

    fn parameters_json_schema(&self) -> Value {
        terminal_kill_schema()
    }

    fn capability(&self) -> ToolCapability {
        ToolCapability::Shell
    }

    async fn call(&self, ctx: ToolContext, args_json: Value) -> Result<ToolResult, ToolError> {
        require_tmux()?;
        let args: TerminalKillArgs = parse_tool_args(args_json)?;
        validate_terminal_id(&args.session_id)?;
        let target = tmux_name(&ctx.run_id, &args.session_id)?;
        run_tmux(["kill-session".to_string(), "-t".to_string(), target])?;
        Ok(text_json_tool_result(
            format!("terminal killed: {}", args.session_id),
            json!({ "session_id": args.session_id, "status": "killed" }),
        ))
    }
}

#[async_trait]
impl Tool for TerminalListTool {
    fn id(&self) -> &str {
        "terminal_list"
    }

    fn description(&self) -> &str {
        "Lists tmux-backed terminal sessions for this run. If tmux is missing, reports availability=false without side effects."
    }

    fn parameters_json_schema(&self) -> Value {
        terminal_list_schema()
    }

    fn capability(&self) -> ToolCapability {
        ToolCapability::Shell
    }

    async fn call(&self, ctx: ToolContext, _args_json: Value) -> Result<ToolResult, ToolError> {
        if !tmux_available() {
            return Ok(text_json_tool_result(
                "tmux is not available; terminal sessions cannot be listed",
                json!({ "tmux_available": false, "sessions": [] }),
            ));
        }
        let prefix = tmux_run_prefix(&ctx.run_id);
        let output = Command::new("tmux")
            .args(["list-sessions", "-F", "#{session_name}"])
            .output()
            .map_err(|err| ToolError::Execution(format!("failed to list tmux sessions: {err}")))?;
        let stdout = String::from_utf8_lossy(&output.stdout);
        let sessions = stdout
            .lines()
            .filter_map(|line| line.strip_prefix(&prefix))
            .map(|session_id| json!({ "session_id": session_id, "backend": "tmux" }))
            .collect::<Vec<_>>();
        Ok(text_json_tool_result(
            format!("{} terminal session(s)", sessions.len()),
            json!({ "tmux_available": true, "sessions": sessions }),
        ))
    }
}

fn spawn_terminal(
    ctx: &ToolContext,
    args: TerminalSpawnArgs,
    allowlist: &ShellAllowlist,
) -> Result<ToolResult, ToolError> {
    if args.command.trim().is_empty() {
        return Err(ToolError::InvalidArguments(
            "terminal command cannot be empty".to_string(),
        ));
    }
    let session_id = args
        .session_id
        .unwrap_or_else(|| format!("term_{}", sanitize_fragment(&ctx.tool_call_id)));
    validate_terminal_id(&session_id)?;
    let target = tmux_name(&ctx.run_id, &session_id)?;
    let cwd = match args.cwd.as_deref() {
        Some(cwd) => ctx.resolve_workspace_path(std::path::Path::new(cwd))?,
        None => ctx.workspace_root.clone(),
    };
    validate_terminal_shell_input(&args.command, &cwd, ctx, allowlist)?;
    require_tmux()?;
    let command = format!("sh -lc {}", shell_words::quote(&args.command));
    let mut tmux_args = vec![
        "new-session".to_string(),
        "-d".to_string(),
        "-s".to_string(),
        target.clone(),
        "-c".to_string(),
        cwd.display().to_string(),
    ];
    if let Some(cols) = args.cols {
        tmux_args.push("-x".to_string());
        tmux_args.push(cols.to_string());
    }
    if let Some(rows) = args.rows {
        tmux_args.push("-y".to_string());
        tmux_args.push(rows.to_string());
    }
    tmux_args.push(command);
    run_tmux(tmux_args)?;
    Ok(text_json_tool_result(
        format!("terminal spawned: {session_id}"),
        json!({
            "session_id": session_id,
            "backend": "tmux",
            "tmux_session": target,
            "cwd": cwd,
            "rows": args.rows,
            "cols": args.cols
        }),
    ))
}

fn write_terminal(
    ctx: &ToolContext,
    session_id: &str,
    input: &str,
    enter: bool,
    allowlist: &ShellAllowlist,
) -> Result<(), ToolError> {
    validate_terminal_id(session_id)?;
    validate_terminal_write_input(input, enter)?;
    if enter {
        validate_terminal_shell_input(input, &ctx.workspace_root, ctx, allowlist)?;
    }
    require_tmux()?;
    let target = tmux_name(&ctx.run_id, session_id)?;
    if enter {
        run_tmux([
            "send-keys".to_string(),
            "-t".to_string(),
            target.clone(),
            "C-u".to_string(),
        ])?;
    }
    run_tmux([
        "send-keys".to_string(),
        "-l".to_string(),
        "-t".to_string(),
        target.clone(),
        input.to_string(),
    ])?;
    if enter {
        run_tmux([
            "send-keys".to_string(),
            "-t".to_string(),
            target,
            "Enter".to_string(),
        ])?;
    }
    Ok(())
}

fn validate_terminal_write_input(input: &str, enter: bool) -> Result<(), ToolError> {
    if input.contains('\n') || input.contains('\r') {
        return Err(ToolError::InvalidArguments(
            "terminal_write input must not contain newlines; use enter=true for a single shell submission"
                .to_string(),
        ));
    }
    if enter && input.trim().is_empty() {
        return Err(ToolError::InvalidArguments(
            "terminal_write enter=true requires a non-empty shell command".to_string(),
        ));
    }
    Ok(())
}

fn validate_terminal_shell_input(
    command: &str,
    cwd: &std::path::Path,
    ctx: &ToolContext,
    allowlist: &ShellAllowlist,
) -> Result<(), ToolError> {
    validate_bash_command(command, cwd, &ctx.workspace_root, allowlist).map_err(|err| match err {
        ToolError::CommandBlocked(reason) => ToolError::CommandBlocked(format!(
            "terminal shell command blocked by bash policy: {reason}"
        )),
        other => other,
    })
}

fn capture_terminal(
    ctx: &ToolContext,
    session_id: &str,
    start: Option<i32>,
) -> Result<ToolResult, ToolError> {
    require_tmux()?;
    validate_terminal_id(session_id)?;
    let target = tmux_name(&ctx.run_id, session_id)?;
    let start = start
        .map(|value| value.to_string())
        .unwrap_or_else(|| CAPTURE_DEFAULT_START.to_string());
    let output = run_tmux_capture([
        "capture-pane".to_string(),
        "-p".to_string(),
        "-t".to_string(),
        target,
        "-S".to_string(),
        start,
    ])?;
    let artifact = write_capture_artifact(ctx, session_id, &output)?;
    Ok(text_json_artifacts_tool_result(
        format!("terminal captured: {session_id}"),
        json!({
            "session_id": session_id,
            "backend": "tmux",
            "captured_text": truncate_chars(&output, 4000),
            "artifact_path": artifact.path
        }),
        vec![artifact],
    ))
}

fn write_capture_artifact(
    ctx: &ToolContext,
    session_id: &str,
    output: &str,
) -> Result<ArtifactRef, ToolError> {
    let relative = format!(
        "toolcalls/{}/terminal_{}.txt",
        sanitize_fragment(&ctx.tool_call_id),
        sanitize_fragment(session_id)
    );
    let target = ctx.artifacts_dir.join(&relative);
    if let Some(parent) = target.parent() {
        fs::create_dir_all(parent).map_err(|err| {
            ToolError::Execution(format!("failed to create terminal artifact dir: {err}"))
        })?;
    }
    fs::write(&target, output).map_err(|err| {
        ToolError::Execution(format!("failed to write terminal capture artifact: {err}"))
    })?;
    Ok(ArtifactRef {
        path: format!("artifacts/{relative}"),
        digest: None,
    })
}

fn require_tmux() -> Result<(), ToolError> {
    if tmux_available() {
        Ok(())
    } else {
        Err(ToolError::Execution(
            "tmux is not installed or not runnable; install tmux or use one-shot bash. Persistent terminal sessions intentionally do not fall back to non-interactive bash.".to_string(),
        ))
    }
}

fn tmux_available() -> bool {
    Command::new("tmux")
        .arg("-V")
        .output()
        .is_ok_and(|output| output.status.success())
}

fn run_tmux(args: impl IntoIterator<Item = String>) -> Result<(), ToolError> {
    let output = Command::new("tmux")
        .args(args)
        .output()
        .map_err(|err| ToolError::Execution(format!("failed to run tmux: {err}")))?;
    if output.status.success() {
        Ok(())
    } else {
        Err(ToolError::Execution(format!(
            "tmux command failed: {}{}",
            String::from_utf8_lossy(&output.stderr).trim(),
            format_stdout(&output.stdout)
        )))
    }
}

fn run_tmux_capture(args: impl IntoIterator<Item = String>) -> Result<String, ToolError> {
    let output = Command::new("tmux")
        .args(args)
        .output()
        .map_err(|err| ToolError::Execution(format!("failed to run tmux capture: {err}")))?;
    if output.status.success() {
        Ok(String::from_utf8_lossy(&output.stdout).to_string())
    } else {
        Err(ToolError::Execution(format!(
            "tmux capture failed: {}{}",
            String::from_utf8_lossy(&output.stderr).trim(),
            format_stdout(&output.stdout)
        )))
    }
}

fn format_stdout(stdout: &[u8]) -> String {
    let stdout = String::from_utf8_lossy(stdout);
    let stdout = stdout.trim();
    if stdout.is_empty() {
        String::new()
    } else {
        format!("; stdout: {stdout}")
    }
}

fn tmux_name(run_id: &str, session_id: &str) -> Result<String, ToolError> {
    validate_terminal_id(session_id)?;
    Ok(format!("{}{}", tmux_run_prefix(run_id), session_id))
}

fn tmux_run_prefix(run_id: &str) -> String {
    format!("{TMUX_PREFIX}_{}_", sanitize_fragment(run_id))
}

fn validate_terminal_id(session_id: &str) -> Result<(), ToolError> {
    let valid = !session_id.trim().is_empty()
        && session_id
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '_' | '-' | '.'));
    if valid {
        Ok(())
    } else {
        Err(ToolError::InvalidArguments(
            "terminal session_id must contain only ASCII letters, digits, '.', '_' or '-'"
                .to_string(),
        ))
    }
}

fn validate_size(rows: u16, cols: u16) -> Result<(), ToolError> {
    if rows == 0 || cols == 0 {
        return Err(ToolError::InvalidArguments(
            "terminal rows and cols must be non-zero".to_string(),
        ));
    }
    Ok(())
}

fn sanitize_fragment(value: &str) -> String {
    let sanitized = value
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || matches!(ch, '_' | '-' | '.') {
                ch
            } else {
                '_'
            }
        })
        .collect::<String>();
    if sanitized.is_empty() {
        "terminal".to_string()
    } else {
        sanitized
    }
}

fn truncate_chars(value: &str, max_chars: usize) -> String {
    if value.chars().count() <= max_chars {
        value.to_string()
    } else {
        let mut out = value
            .chars()
            .take(max_chars.saturating_sub(1))
            .collect::<String>();
        out.push('…');
        out
    }
}

pub(crate) fn interactive_bash_schema() -> Value {
    json!({
        "type": "object",
        "additionalProperties": false,
        "properties": {
            "session_id": { "type": "string" },
            "command": { "type": "string" },
            "input": { "type": "string" },
            "cwd": { "type": "string" },
            "enter": { "type": "boolean" }
        },
        "required": []
    })
}

pub(crate) fn terminal_spawn_schema() -> Value {
    json!({
        "type": "object",
        "additionalProperties": false,
        "properties": {
            "session_id": { "type": "string" },
            "command": { "type": "string" },
            "cwd": { "type": "string" },
            "rows": { "type": "integer", "minimum": 1 },
            "cols": { "type": "integer", "minimum": 1 }
        },
        "required": ["command"]
    })
}

pub(crate) fn terminal_write_schema() -> Value {
    json!({
        "type": "object",
        "additionalProperties": false,
        "properties": {
            "session_id": { "type": "string" },
            "input": { "type": "string" },
            "enter": { "type": "boolean" }
        },
        "required": ["session_id", "input"]
    })
}

pub(crate) fn terminal_screenshot_schema() -> Value {
    json!({
        "type": "object",
        "additionalProperties": false,
        "properties": {
            "session_id": { "type": "string" },
            "start": { "type": "integer" }
        },
        "required": ["session_id"]
    })
}

pub(crate) fn terminal_resize_schema() -> Value {
    json!({
        "type": "object",
        "additionalProperties": false,
        "properties": {
            "session_id": { "type": "string" },
            "rows": { "type": "integer", "minimum": 1 },
            "cols": { "type": "integer", "minimum": 1 }
        },
        "required": ["session_id", "rows", "cols"]
    })
}

pub(crate) fn terminal_kill_schema() -> Value {
    json!({
        "type": "object",
        "additionalProperties": false,
        "properties": {
            "session_id": { "type": "string" }
        },
        "required": ["session_id"]
    })
}

pub(crate) fn terminal_list_schema() -> Value {
    json!({
        "type": "object",
        "additionalProperties": false,
        "properties": {},
        "required": []
    })
}

#[cfg(test)]
mod tests {
    use harness_core::config::ShellAllowlist;
    use harness_core::tool::{Tool, ToolError};
    use serde_json::json;
    use tempfile::tempdir;

    use super::{InteractiveBashTool, TerminalSpawnTool, TerminalWriteTool};
    use crate::test_support::tool_context;

    fn allow_echo_only() -> ShellAllowlist {
        ShellAllowlist {
            executables: vec!["echo".to_string()],
            cwd_roots: Vec::new(),
        }
    }

    fn assert_command_blocked(err: ToolError) {
        let message = err.to_string();
        assert!(
            message.contains("terminal shell command blocked by bash policy"),
            "{message}"
        );
    }

    #[tokio::test]
    async fn terminal_spawn_enforces_bash_allowlist_before_tmux() {
        let workspace = tempdir().expect("workspace");
        let tool = TerminalSpawnTool::new(allow_echo_only());
        let err = tool
            .call(
                tool_context(workspace.path(), "terminal-spawn-policy"),
                json!({"session_id": "policy", "command": "find ."}),
            )
            .await
            .expect_err("terminal_spawn must reject commands blocked by bash policy");

        assert_command_blocked(err);
    }

    #[tokio::test]
    async fn interactive_bash_command_enforces_bash_allowlist_before_tmux() {
        let workspace = tempdir().expect("workspace");
        let tool = InteractiveBashTool::new(allow_echo_only());
        let err = tool
            .call(
                tool_context(workspace.path(), "interactive-bash-policy"),
                json!({"session_id": "policy", "command": "rm -rf ."}),
            )
            .await
            .expect_err("interactive_bash command must reject dangerous shell commands");

        assert_command_blocked(err);
    }

    #[tokio::test]
    async fn terminal_write_treats_input_as_literal_tmux_payload_without_enter() {
        let workspace = tempdir().expect("workspace");
        let tool = TerminalWriteTool::new(allow_echo_only());
        let result = tool
            .call(
                tool_context(workspace.path(), "terminal-write-literal"),
                json!({"session_id": "policy", "input": "grep needle file", "enter": false}),
            )
            .await;

        if let Err(err) = result {
            assert!(
                !matches!(err, ToolError::CommandBlocked(_)),
                "terminal_write enter=false should send literal input instead of applying bash allowlist: {err}"
            );
        }
    }

    #[tokio::test]
    async fn terminal_write_enter_enforces_bash_allowlist_before_tmux() {
        let workspace = tempdir().expect("workspace");
        let tool = TerminalWriteTool::new(allow_echo_only());
        let err = tool
            .call(
                tool_context(workspace.path(), "terminal-write-policy"),
                json!({"session_id": "policy", "input": "rm -rf .", "enter": true}),
            )
            .await
            .expect_err("terminal_write enter=true must reject dangerous shell commands");

        assert_command_blocked(err);
    }

    #[tokio::test]
    async fn terminal_write_rejects_newline_payloads_before_tmux() {
        let workspace = tempdir().expect("workspace");
        let tool = TerminalWriteTool::new(allow_echo_only());
        let err = tool
            .call(
                tool_context(workspace.path(), "terminal-write-newline"),
                json!({"session_id": "policy", "input": "echo ok\nrm -rf .", "enter": false}),
            )
            .await
            .expect_err("terminal_write must not send newline payloads");

        assert!(
            matches!(err, ToolError::InvalidArguments(_)),
            "newline payload should fail argument validation before tmux: {err}"
        );
    }
}
