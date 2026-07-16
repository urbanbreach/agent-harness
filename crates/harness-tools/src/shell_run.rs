// allow: SIZE_OK — shell execution (process spawning + output capture)
use crate::UnwrapOrAbort;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use harness_core::config::ShellAllowlist;
use harness_core::perm::shell::{
    direct_shell_command_request, scan_shell_command, ShellCommandRequest,
};
use harness_core::tool::{ArtifactRef, Tool, ToolCapability, ToolContext, ToolError, ToolResult};
use harness_core::tool_metadata;
use harness_core::ToolResultExt;
use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::json;

use crate::shell_safety::ShellSafety;
use crate::text::trimmed_non_empty;
use crate::{
    json_schema_for, parse_tool_args, text_json_artifacts_tool_result, text_json_tool_result,
};

const SHELL_OUTPUT_INLINE_LINE_LIMIT: usize = 2_000;
const SHELL_OUTPUT_INLINE_BYTE_LIMIT: usize = 51_200;
const SHELL_OUTPUT_PREVIEW_LIMITS: ShellOutputPreviewLimits = ShellOutputPreviewLimits {
    max_lines: SHELL_OUTPUT_INLINE_LINE_LIMIT,
    max_bytes: SHELL_OUTPUT_INLINE_BYTE_LIMIT,
};
const SAFE_SHELL_ENV_KEYS: &[&str] = &[
    "PATH", "TERM", "TMPDIR", "TEMP", "TMP", "LANG", "LC_ALL", "LC_CTYPE",
];

#[derive(Debug, Clone)]
struct ShellOutputPreview {
    inline_text: String,
    total_bytes: usize,
    total_lines: usize,
    inline_lines: usize,
}

#[derive(Debug, Clone, Copy)]
struct ShellOutputPreviewLimits {
    max_lines: usize,
    max_bytes: usize,
}

impl ShellOutputPreviewLimits {
    fn preview(self, text: &str) -> ShellOutputPreview {
        let total_bytes = text.len();
        let total_lines = count_shell_output_lines(text);
        let preview_end = self.preview_end(text);

        let inline_text = text[..preview_end].to_string();
        let inline_lines = count_shell_output_lines(&inline_text);

        ShellOutputPreview {
            inline_lines,
            inline_text,
            total_bytes,
            total_lines,
        }
    }

    fn preview_end(self, text: &str) -> usize {
        let byte_limited_end = self.byte_limited_end(text);
        let line_limited_end = self.line_limited_end(text);
        byte_limited_end.min(line_limited_end)
    }

    fn byte_limited_end(self, text: &str) -> usize {
        let mut preview_end = 0usize;

        for (idx, ch) in text.char_indices() {
            let next_end = idx + ch.len_utf8();
            if next_end > self.max_bytes {
                break;
            }
            preview_end = next_end;
        }

        preview_end
    }

    fn line_limited_end(self, text: &str) -> usize {
        if text.is_empty() || self.max_lines == 0 {
            return 0;
        }

        let mut line_end = 0usize;
        for segment in text.split_inclusive('\n').take(self.max_lines) {
            line_end += segment.len();
        }
        line_end
    }
}

impl ShellOutputPreview {
    fn is_truncated(&self) -> bool {
        self.inline_bytes() < self.total_bytes
    }

    fn inline_bytes(&self) -> usize {
        self.inline_text.len()
    }

    fn insert_truncation_metadata(
        &self,
        structured_json: &mut serde_json::Map<String, serde_json::Value>,
        artifact: &ArtifactRef,
    ) {
        structured_json.insert("truncated".to_string(), json!(true));
        structured_json.insert(
            "inline_output_bytes".to_string(),
            json!(self.inline_bytes()),
        );
        structured_json.insert("inline_output_lines".to_string(), json!(self.inline_lines));
        structured_json.insert("total_output_bytes".to_string(), json!(self.total_bytes));
        structured_json.insert("total_output_lines".to_string(), json!(self.total_lines));
        structured_json.insert(
            "output_artifact".to_string(),
            json!({
                "path": artifact.path,
                "digest": artifact.digest,
            }),
        );
    }

    fn truncation_marker(&self, artifact: &ArtifactRef) -> String {
        format!(
            "... [truncated: showing {} of {} lines and {} of {} bytes; full output: {}]",
            self.inline_lines,
            self.total_lines,
            self.inline_bytes(),
            self.total_bytes,
            artifact.path
        )
    }

    fn into_truncated_display_text(self, artifact: &ArtifactRef) -> String {
        let marker = self.truncation_marker(artifact);
        let mut display_text = self.inline_text;
        append_truncation_marker(&mut display_text, &marker);
        display_text
    }
}

fn count_shell_output_lines(text: &str) -> usize {
    if text.is_empty() {
        0
    } else {
        text.split_inclusive('\n').count()
    }
}

fn append_truncation_marker(display_text: &mut String, marker: &str) {
    if display_text.is_empty() {
        display_text.push_str(marker);
        return;
    }

    if !display_text.ends_with('\n') {
        display_text.push('\n');
    }
    display_text.push_str(marker);
}

fn resolve_bash_executable() -> String {
    for candidate in ["/bin/bash", "/usr/bin/bash"] {
        if is_existing_bash_file(Path::new(candidate)) {
            return candidate.to_string();
        }
    }

    if let Some(shell) = shell_env_bash_candidate(std::env::var("SHELL").ok().as_deref()) {
        return shell;
    }

    "bash".to_string()
}

fn shell_env_bash_candidate(shell: Option<&str>) -> Option<String> {
    let shell = shell?.trim();
    if shell.is_empty() {
        return None;
    }

    let path = Path::new(shell);
    if !path.is_absolute() || !is_existing_bash_file(path) {
        return None;
    }

    Some(shell.to_string())
}

fn is_existing_bash_file(path: &Path) -> bool {
    path.file_name()
        .and_then(|name| name.to_str())
        .is_some_and(|name| name.eq_ignore_ascii_case("bash"))
        && std::fs::metadata(path)
            .map(|metadata| metadata.is_file())
            .unwrap_or(false)
}

#[derive(Debug, Clone)]
#[doc(hidden)]
pub struct ShellProcessOutput {
    pub stdout: String,
    pub stderr: String,
    pub status: i32,
    pub success: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
#[doc(hidden)]
pub struct ShellCommandInvocation {
    pub program: String,
    pub args: Vec<String>,
    pub cwd: PathBuf,
}

impl ShellCommandInvocation {
    fn new(program: impl Into<String>, cwd: PathBuf) -> Self {
        Self {
            program: program.into(),
            args: Vec::new(),
            cwd,
        }
    }

    fn args(mut self, args: impl IntoIterator<Item = impl Into<String>>) -> Self {
        self.args = args.into_iter().map(Into::into).collect();
        self
    }
}

#[async_trait]
#[doc(hidden)]
pub trait ShellCommandRunner: Send + Sync {
    async fn run(
        &self,
        invocation: ShellCommandInvocation,
        timeout_ms: u64,
    ) -> Result<ShellProcessOutput, ToolError>;
}

#[derive(Debug, Default)]
struct TokioShellCommandRunner;

#[async_trait]
impl ShellCommandRunner for TokioShellCommandRunner {
    async fn run(
        &self,
        invocation: ShellCommandInvocation,
        timeout_ms: u64,
    ) -> Result<ShellProcessOutput, ToolError> {
        #[cfg(target_os = "linux")]
        prevent_parent_process_environment_disclosure()?;
        let mut command = shell_process_command(invocation);
        apply_sanitized_shell_environment(&mut command);
        run_shell_process(command, timeout_ms).await
    }
}

#[cfg(target_os = "linux")]
fn prevent_parent_process_environment_disclosure() -> Result<(), ToolError> {
    rustix::process::set_dumpable_behavior(rustix::process::DumpableBehavior::NotDumpable)
        .tool_err("failed to harden shell process")
}

fn shell_process_command(invocation: ShellCommandInvocation) -> tokio::process::Command {
    if cfg!(unix) {
        let mut command = tokio::process::Command::new("/bin/sh");
        command
            .arg("-c")
            .arg(r#"("$@")"#)
            .arg("harness-shell-run")
            .arg(invocation.program)
            .args(invocation.args)
            .current_dir(invocation.cwd);
        return command;
    }

    let mut command = tokio::process::Command::new(&invocation.program);
    command.args(&invocation.args).current_dir(invocation.cwd);
    command
}

fn apply_sanitized_shell_environment(command: &mut tokio::process::Command) {
    command.env_clear();
    for key in SAFE_SHELL_ENV_KEYS {
        if let Some(value) = std::env::var_os(key) {
            command.env(key, value);
        }
    }
    if std::env::var_os("PATH").is_none() {
        command.env("PATH", default_shell_path());
    }
    if std::env::var_os("TERM").is_none() {
        command.env("TERM", "dumb");
    }
}

fn default_shell_path() -> &'static str {
    if cfg!(windows) {
        r"C:\Windows\System32;C:\Windows"
    } else {
        "/usr/local/bin:/usr/bin:/bin"
    }
}

impl From<std::process::Output> for ShellProcessOutput {
    fn from(output: std::process::Output) -> Self {
        Self {
            stdout: String::from_utf8_lossy(&output.stdout).to_string(),
            stderr: String::from_utf8_lossy(&output.stderr).to_string(),
            status: output.status.code().unwrap_or(-1),
            success: output.status.success(),
        }
    }
}

impl ShellProcessOutput {
    fn combined_output(&self) -> String {
        if self.stderr.is_empty() {
            self.stdout.clone()
        } else if self.stdout.is_empty() {
            self.stderr.clone()
        } else {
            format!("{}\n{}", self.stdout, self.stderr)
        }
    }

    fn insert_stream_byte_metadata(
        &self,
        metadata: &mut serde_json::Map<String, serde_json::Value>,
    ) {
        metadata.insert("stdout_bytes".to_string(), json!(self.stdout.len()));
        metadata.insert("stderr_bytes".to_string(), json!(self.stderr.len()));
    }

    fn insert_execution_status(&self, metadata: &mut serde_json::Map<String, serde_json::Value>) {
        metadata.insert("status".to_string(), json!(self.status));
        metadata.insert("success".to_string(), json!(self.success));
    }

    fn insert_full_streams(self, metadata: &mut serde_json::Map<String, serde_json::Value>) {
        metadata.insert("stdout".to_string(), json!(self.stdout));
        metadata.insert("stderr".to_string(), json!(self.stderr));
    }
}

async fn run_shell_process(
    mut command: tokio::process::Command,
    timeout_ms: u64,
) -> Result<ShellProcessOutput, ToolError> {
    let output = tokio::time::timeout(Duration::from_millis(timeout_ms), command.output())
        .await
        .map_err(|_| ToolError::Execution(format!("command timed out after {timeout_ms} ms")))?
        .tool_err("failed to execute command")?;

    Ok(output.into())
}

fn build_shell_run_result(
    ctx: &ToolContext,
    execution: ShellRunExecution,
) -> Result<ToolResult, ToolError> {
    let ShellRunExecution {
        output,
        mut structured_json,
    } = execution;

    output.insert_stream_byte_metadata(&mut structured_json);
    let full_output = output.combined_output();
    let preview = SHELL_OUTPUT_PREVIEW_LIMITS.preview(&full_output);

    if !preview.is_truncated() {
        return Ok(build_inline_shell_run_result(
            output,
            structured_json,
            full_output,
        ));
    }

    build_truncated_shell_run_result(ctx, structured_json, &full_output, preview)
}

fn build_inline_shell_run_result(
    output: ShellProcessOutput,
    mut structured_json: serde_json::Map<String, serde_json::Value>,
    full_output: String,
) -> ToolResult {
    output.insert_full_streams(&mut structured_json);
    structured_json.insert("truncated".to_string(), json!(false));
    text_json_tool_result(full_output, serde_json::Value::Object(structured_json))
}

fn build_truncated_shell_run_result(
    ctx: &ToolContext,
    mut structured_json: serde_json::Map<String, serde_json::Value>,
    full_output: &str,
    preview: ShellOutputPreview,
) -> Result<ToolResult, ToolError> {
    let artifact = write_shell_output_artifact(ctx, full_output)?;
    preview.insert_truncation_metadata(&mut structured_json, &artifact);
    let display_text = preview.into_truncated_display_text(&artifact);

    Ok(text_json_artifacts_tool_result(
        display_text,
        serde_json::Value::Object(structured_json),
        vec![artifact],
    ))
}

fn write_shell_output_artifact(
    ctx: &ToolContext,
    full_output: &str,
) -> Result<ArtifactRef, ToolError> {
    ctx.artifact_store()
        .tool_err("failed to access artifact store")?
        .write_text(
            &format!("toolcalls/{}/shell.output.txt", ctx.tool_call_id),
            full_output,
        )
        .map_err(|err| {
            ToolError::Execution(format!("failed to write shell output artifact: {err}"))
        })
}

pub(crate) struct ShellRunTool {
    safety: ShellSafety,
    runner: Arc<dyn ShellCommandRunner>,
}

impl ShellRunTool {
    pub(crate) fn default_runner() -> Arc<dyn ShellCommandRunner> {
        Arc::new(TokioShellCommandRunner)
    }

    pub(crate) fn with_runner(
        allowlist: ShellAllowlist,
        runner: Arc<dyn ShellCommandRunner>,
    ) -> Self {
        Self {
            safety: ShellSafety::new(allowlist),
            runner,
        }
    }
}

#[derive(Deserialize, JsonSchema)]
struct ShellRunArgs {
    #[serde(default)]
    cmd: Option<String>,
    #[serde(default)]
    args: Vec<String>,
    #[serde(default)]
    cwd: Option<String>,
    #[serde(default)]
    command: Option<String>,
    #[serde(default)]
    workdir: Option<String>,
    #[serde(default)]
    timeout: Option<u64>,
    #[serde(default)]
    description: Option<String>,
}

const DEFAULT_SHELL_TIMEOUT_MS: u64 = 120_000;

struct ShellRunRequest {
    invocation: ShellInvocation,
    timeout_ms: u64,
}

enum ShellInvocation {
    Direct(DirectShellInvocation),
    Wrapper(WrapperShellInvocation),
}

struct DirectShellInvocation {
    cmd: String,
    args: Vec<String>,
    cwd: Option<String>,
}

struct WrapperShellInvocation {
    command: String,
    workdir: Option<String>,
    description: Option<String>,
}

struct ShellRunExecution {
    output: ShellProcessOutput,
    structured_json: serde_json::Map<String, serde_json::Value>,
}

impl DirectShellInvocation {
    fn into_metadata(
        self,
        output: &ShellProcessOutput,
    ) -> serde_json::Map<String, serde_json::Value> {
        let permission_request = direct_shell_command_request(&self.cmd, &self.args);
        let mut metadata = serde_json::Map::new();
        metadata.insert("cmd".to_string(), json!(self.cmd));
        metadata.insert("args".to_string(), json!(self.args));
        metadata.insert("cwd".to_string(), json!(self.cwd));
        insert_permission_pattern_metadata(&mut metadata, &permission_request);
        output.insert_execution_status(&mut metadata);
        metadata
    }
}

impl WrapperShellInvocation {
    fn into_metadata(
        self,
        output: &ShellProcessOutput,
    ) -> serde_json::Map<String, serde_json::Value> {
        let permission_request = scan_shell_command(&self.command).ok();
        let mut metadata = serde_json::Map::new();
        metadata.insert("description".to_string(), json!(self.description));
        metadata.insert("command".to_string(), json!(self.command));
        metadata.insert("workdir".to_string(), json!(self.workdir));
        insert_optional_permission_pattern_metadata(&mut metadata, permission_request.as_ref());
        output.insert_execution_status(&mut metadata);
        metadata
    }
}

fn insert_permission_pattern_metadata(
    metadata: &mut serde_json::Map<String, serde_json::Value>,
    permission_request: &ShellCommandRequest,
) {
    metadata.insert(
        "permission_patterns".to_string(),
        json!(permission_request.patterns),
    );
    metadata.insert(
        "permission_always_patterns".to_string(),
        json!(permission_request.always_patterns),
    );
}

fn insert_optional_permission_pattern_metadata(
    metadata: &mut serde_json::Map<String, serde_json::Value>,
    permission_request: Option<&ShellCommandRequest>,
) {
    match permission_request {
        Some(request) => insert_permission_pattern_metadata(metadata, request),
        None => {
            metadata.insert("permission_patterns".to_string(), json!([]));
            metadata.insert("permission_always_patterns".to_string(), json!([]));
        }
    }
}

impl ShellRunArgs {
    fn into_request(self) -> Result<ShellRunRequest, ToolError> {
        let timeout_ms = self.timeout.unwrap_or(DEFAULT_SHELL_TIMEOUT_MS);
        let invocation = self.into_invocation()?;

        Ok(ShellRunRequest {
            invocation,
            timeout_ms,
        })
    }

    fn into_invocation(self) -> Result<ShellInvocation, ToolError> {
        let cmd = normalized_shell_command(self.cmd);
        let command = normalized_shell_command(self.command);

        if let Some(cmd) = cmd {
            if command.as_ref().is_some_and(|command| command != &cmd) {
                return Err(shell_invocation_selection_error());
            }

            return Ok(ShellInvocation::Direct(DirectShellInvocation {
                cmd,
                args: self.args,
                cwd: self.cwd.or(self.workdir),
            }));
        }

        if let Some(command) = command {
            Ok(ShellInvocation::Wrapper(WrapperShellInvocation {
                command,
                workdir: self.workdir.or(self.cwd),
                description: self.description,
            }))
        } else {
            Err(shell_invocation_selection_error())
        }
    }
}

fn shell_invocation_selection_error() -> ToolError {
    ToolError::InvalidArguments("provide exactly one of cmd or command".to_string())
}

fn normalized_shell_command(command: Option<String>) -> Option<String> {
    command.and_then(|command| trimmed_non_empty(&command).map(str::to_string))
}

impl ShellRunTool {
    async fn run_request(
        &self,
        ctx: &ToolContext,
        request: ShellRunRequest,
    ) -> Result<ShellRunExecution, ToolError> {
        match request.invocation {
            ShellInvocation::Direct(invocation) => {
                self.run_direct_invocation(ctx, invocation, request.timeout_ms)
                    .await
            }
            ShellInvocation::Wrapper(invocation) => {
                self.run_wrapper_invocation(ctx, invocation, request.timeout_ms)
                    .await
            }
        }
    }

    async fn run_direct_invocation(
        &self,
        ctx: &ToolContext,
        invocation: DirectShellInvocation,
        timeout_ms: u64,
    ) -> Result<ShellRunExecution, ToolError> {
        self.safety.ensure_executable_allowed(&invocation.cmd)?;

        let resolved_cwd = self.safety.resolve_cwd(ctx, invocation.cwd.as_deref())?;
        self.safety.validate_direct_args_with_grants(
            &invocation.cmd,
            &invocation.args,
            &resolved_cwd,
            &ctx.workspace_root,
            &ctx.external_directory_allow_prefixes,
        )?;
        let output = self
            .runner
            .run(
                ShellCommandInvocation::new(&invocation.cmd, resolved_cwd).args(&invocation.args),
                timeout_ms,
            )
            .await?;
        let structured_json = invocation.into_metadata(&output);

        Ok(ShellRunExecution {
            output,
            structured_json,
        })
    }

    async fn run_wrapper_invocation(
        &self,
        ctx: &ToolContext,
        invocation: WrapperShellInvocation,
        timeout_ms: u64,
    ) -> Result<ShellRunExecution, ToolError> {
        let cwd = self
            .safety
            .resolve_cwd(ctx, invocation.workdir.as_deref())?;
        self.safety.validate_bash_command_with_grants(
            &invocation.command,
            &cwd,
            &ctx.workspace_root,
            &ctx.external_directory_allow_prefixes,
        )?;
        let shell = resolve_bash_executable();
        let output = self
            .runner
            .run(
                ShellCommandInvocation::new(shell, cwd)
                    .args(["-c".to_string(), invocation.command.clone()]),
                timeout_ms,
            )
            .await?;
        let structured_json = invocation.into_metadata(&output);

        Ok(ShellRunExecution {
            output,
            structured_json,
        })
    }
}

#[async_trait]
impl Tool for ShellRunTool {
    tool_metadata!(
        "shell.run",
        "Runs a shell command inside the workspace and returns stdout/stderr. Use bash for real shell work like git, cargo, or npm—not for file discovery, file search, reading, or editing. Commands such as find, grep/rg, cat, head, tail, sed, and awk are discouraged; use glob, grep, list, read, or edit instead when possible. Shell commands are controlled by permission patterns and workspace path safety.",
        ToolCapability::Shell,
        json_schema_for::<ShellRunArgs>()
    );

    async fn call(
        &self,
        ctx: ToolContext,
        args_json: serde_json::Value,
    ) -> Result<ToolResult, ToolError> {
        let args: ShellRunArgs = parse_tool_args(args_json)?;
        let request = args.into_request()?;
        let executed = self.run_request(&ctx, request).await?;

        build_shell_run_result(&ctx, executed)
    }
}

#[cfg(test)]
mod tests {
    use crate::UnwrapOrAbort;
    use std::sync::{Arc, Mutex};

    use super::{
        ShellCommandInvocation, ShellCommandRunner, ShellOutputPreviewLimits, ShellProcessOutput,
        ShellRunTool, SHELL_OUTPUT_INLINE_LINE_LIMIT,
    };
    use crate::coordinator_registry;
    use crate::test_support::{read_spilled_artifact, tool_context as shell_test_context};
    use harness_core::config::ShellAllowlist;
    use harness_core::tool::{Tool, ToolError};
    use serde_json::json;

    #[derive(Debug)]
    struct FakeShellCommandRunner {
        output: ShellProcessOutput,
        calls: Mutex<Vec<(ShellCommandInvocation, u64)>>,
    }

    impl FakeShellCommandRunner {
        fn success(stdout: &str) -> std::sync::Arc<Self> {
            std::sync::Arc::new(Self {
                output: ShellProcessOutput {
                    stdout: stdout.to_string(),
                    stderr: String::new(),
                    status: 0,
                    success: true,
                },
                calls: Mutex::new(Vec::new()),
            })
        }

        fn calls(&self) -> Vec<(ShellCommandInvocation, u64)> {
            self.calls.lock().unwrap_or_abort().clone()
        }
    }

    #[async_trait::async_trait]
    impl ShellCommandRunner for FakeShellCommandRunner {
        async fn run(
            &self,
            invocation: ShellCommandInvocation,
            timeout_ms: u64,
        ) -> Result<ShellProcessOutput, ToolError> {
            self.calls
                .lock()
                .unwrap_or_abort()
                .push((invocation, timeout_ms));
            Ok(self.output.clone())
        }
    }

    #[test]
    fn shell_output_preview_enforces_line_limit_without_trailing_newline() {
        let output = (1..=2_001)
            .map(|idx| format!("line {idx}"))
            .collect::<Vec<_>>()
            .join("\n");

        let preview = ShellOutputPreviewLimits {
            max_lines: SHELL_OUTPUT_INLINE_LINE_LIMIT,
            max_bytes: usize::MAX,
        }
        .preview(&output);

        assert!(preview.is_truncated());
        assert_eq!(preview.inline_lines, SHELL_OUTPUT_INLINE_LINE_LIMIT);
        assert_eq!(preview.total_lines, 2_001);
        assert!(preview
            .inline_text
            .trim_end_matches('\n')
            .ends_with("line 2000"));
        assert!(!preview.inline_text.contains("line 2001"));
    }

    #[test]
    fn shell_output_preview_allows_exact_line_limit_without_trailing_newline() {
        let output = (1..=SHELL_OUTPUT_INLINE_LINE_LIMIT)
            .map(|idx| format!("line {idx}"))
            .collect::<Vec<_>>()
            .join("\n");

        let preview = ShellOutputPreviewLimits {
            max_lines: SHELL_OUTPUT_INLINE_LINE_LIMIT,
            max_bytes: usize::MAX,
        }
        .preview(&output);

        assert!(!preview.is_truncated());
        assert_eq!(preview.inline_lines, SHELL_OUTPUT_INLINE_LINE_LIMIT);
        assert_eq!(preview.total_lines, SHELL_OUTPUT_INLINE_LINE_LIMIT);
        assert_eq!(preview.inline_text, output);
    }

    #[test]
    fn shell_output_preview_byte_limit_keeps_utf8_boundaries() {
        let preview = ShellOutputPreviewLimits {
            max_lines: usize::MAX,
            max_bytes: 3,
        }
        .preview("ééx");

        assert!(preview.is_truncated());
        assert_eq!(preview.inline_text, "é");
        assert_eq!(preview.inline_bytes(), "é".len());
        assert_eq!(preview.total_bytes, "ééx".len());
    }

    #[test]
    fn shell_env_bash_candidate_rejects_relative_and_missing_paths() {
        assert_eq!(super::shell_env_bash_candidate(Some("bash")), None);
        assert_eq!(
            super::shell_env_bash_candidate(Some("/tmp/definitely-not-a-real-bash")),
            None
        );
    }

    #[test]
    fn shell_env_bash_candidate_accepts_existing_absolute_bash_path() {
        let temp = tempfile::tempdir().unwrap_or_abort();
        let bash_path = temp.path().join("bash");
        std::fs::write(&bash_path, "#!/usr/bin/env bash\n").unwrap_or_abort();

        let candidate = super::shell_env_bash_candidate(bash_path.to_str());
        assert_eq!(candidate.as_deref(), bash_path.to_str());
    }
    #[tokio::test]
    async fn bash_spills_large_output_to_artifact_for_event_stability() {
        let temp = tempfile::tempdir().unwrap_or_abort();
        let registry = coordinator_registry(ShellAllowlist::default());
        let bash = registry.get("bash").unwrap_or_abort();

        let context = shell_test_context(temp.path(), "toolcall-bash-truncated");
        let result = bash
            .call(
                context.clone(),
                json!({
                    "command": "yes alpha | tr -d '\\n' | head -c 55000",
                    "description": "emit many lines"
                }),
            )
            .await
            .unwrap_or_abort();

        assert!(result.display_text.contains("[truncated:"));
        assert_eq!(result.artifacts.len(), 1);

        let metadata = result.structured_json.unwrap_or_abort();
        assert_eq!(metadata["truncated"], json!(true));
        assert!(metadata.get("stdout").is_none());
        assert!(metadata.get("stderr").is_none());
        assert_eq!(metadata["total_output_bytes"], json!(55_000));

        let spilled = read_spilled_artifact(&context, &result.artifacts[0].path);
        assert_eq!(spilled.len(), 55_000);
        assert!(spilled.starts_with("alphaalphaalpha"));
    }
    #[tokio::test]
    async fn bash_direct_exec_allows_find_in_permission_patterns() {
        let temp = tempfile::tempdir().unwrap_or_abort();
        std::fs::create_dir(temp.path().join("docs")).unwrap_or_abort();
        std::fs::write(temp.path().join("docs/example.txt"), "hello\n").unwrap_or_abort();
        let bash =
            ShellRunTool::with_runner(ShellAllowlist::default(), ShellRunTool::default_runner());

        let result = bash
            .call(
                shell_test_context(temp.path(), "toolcall-bash-find-allowed"),
                json!({
                    "cmd": "find",
                    "args": ["docs", "-maxdepth", "1", "-type", "f"],
                }),
            )
            .await
            .unwrap_or_abort();

        assert!(result.display_text.contains("docs/example.txt"));
        let structured = result.structured_json.unwrap_or_abort();
        assert_eq!(structured.get("status"), Some(&json!(0)));
    }

    #[tokio::test]
    async fn shell_run_accepts_duplicate_wrapper_command_when_it_matches_cmd() {
        let temp = tempfile::tempdir().unwrap_or_abort();
        let shell = ShellRunTool::with_runner(
            ShellAllowlist {
                executables: vec!["printf".to_string()],
                cwd_roots: Vec::new(),
                ..ShellAllowlist::default()
            },
            ShellRunTool::default_runner(),
        );

        let result = shell
            .call(
                shell_test_context(temp.path(), "toolcall-shell-run-duplicate"),
                json!({
                    "cmd": "printf",
                    "command": "printf",
                    "args": ["shell-ok"],
                    "cwd": ".",
                    "workdir": ".",
                }),
            )
            .await
            .unwrap_or_abort();

        assert_eq!(result.display_text, "shell-ok");
        let structured = result.structured_json.unwrap_or_abort();
        assert_eq!(structured.get("cmd"), Some(&json!("printf")));
        assert_eq!(structured.get("stdout"), Some(&json!("shell-ok")));
        assert_eq!(structured.get("success"), Some(&json!(true)));
    }

    #[tokio::test]
    async fn shell_run_direct_invocation_uses_injected_runner_without_spawning() {
        let temp = tempfile::tempdir().unwrap_or_abort();
        let runner = FakeShellCommandRunner::success("direct-ok");
        let shell = ShellRunTool::with_runner(
            ShellAllowlist {
                executables: vec!["printf".to_string()],
                cwd_roots: Vec::new(),
                ..ShellAllowlist::default()
            },
            std::sync::Arc::<FakeShellCommandRunner>::clone(&runner),
        );

        let result = shell
            .call(
                shell_test_context(temp.path(), "toolcall-shell-run-fake-direct"),
                json!({
                    "cmd": "printf",
                    "args": ["direct-ok"],
                    "cwd": ".",
                    "timeout": 1234,
                }),
            )
            .await
            .unwrap_or_abort();

        assert_eq!(result.display_text, "direct-ok");
        let calls = runner.calls();
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].0.program, "printf");
        assert_eq!(calls[0].0.args, vec!["direct-ok"]);
        assert_eq!(calls[0].0.cwd, temp.path());
        assert_eq!(calls[0].1, 1234);
    }

    #[tokio::test]
    async fn shell_run_wrapper_invocation_uses_injected_runner_without_spawning_bash() {
        let temp = tempfile::tempdir().unwrap_or_abort();
        let runner = FakeShellCommandRunner::success("wrapper-ok");
        let shell = ShellRunTool::with_runner(
            ShellAllowlist::default(),
            std::sync::Arc::<FakeShellCommandRunner>::clone(&runner),
        );

        let result = shell
            .call(
                shell_test_context(temp.path(), "toolcall-shell-run-fake-wrapper"),
                json!({
                    "command": "printf wrapper-ok",
                    "workdir": ".",
                    "timeout": 4321,
                }),
            )
            .await
            .unwrap_or_abort();

        assert_eq!(result.display_text, "wrapper-ok");
        let calls = runner.calls();
        assert_eq!(calls.len(), 1);
        assert!(calls[0].0.program.ends_with("bash"));
        assert_eq!(
            calls[0].0.args,
            vec!["-c".to_string(), "printf wrapper-ok".to_string()]
        );
        assert_eq!(calls[0].0.cwd, temp.path());
        assert_eq!(calls[0].1, 4321);
    }

    #[tokio::test]
    async fn shell_run_wrapper_records_permission_patterns_in_metadata() {
        let temp = tempfile::tempdir().unwrap_or_abort();
        let runner = FakeShellCommandRunner::success("hello");
        let shell = ShellRunTool::with_runner(ShellAllowlist::default(), runner);

        let result = shell
            .call(
                shell_test_context(temp.path(), "toolcall-shell-run-pattern-metadata"),
                json!({
                    "command": "printf hello",
                    "workdir": ".",
                }),
            )
            .await
            .unwrap_or_abort();

        let structured = result.structured_json.unwrap_or_abort();
        assert_eq!(
            structured.get("permission_always_patterns"),
            Some(&json!(["printf *"]))
        );
    }

    #[tokio::test]
    async fn shell_run_rejects_direct_bash_command_mode_bypass() {
        let temp = tempfile::tempdir().unwrap_or_abort();
        let shell = ShellRunTool::with_runner(
            ShellAllowlist {
                executables: vec!["bash".to_string()],
                cwd_roots: Vec::new(),
                ..ShellAllowlist::default()
            },
            ShellRunTool::default_runner(),
        );

        let error = shell
            .call(
                shell_test_context(temp.path(), "toolcall-shell-run-bash-c-blocked"),
                json!({
                    "cmd": "bash",
                    "args": ["-lc", "printf bypass"],
                    "cwd": ".",
                }),
            )
            .await
            .expect_err("direct bash -lc should be blocked");

        match error {
            ToolError::CommandBlocked(message) => {
                assert!(message.contains("interpreter command-eval flags"));
            }
            other => panic!("unexpected error variant: {other}"),
        }
    }

    #[tokio::test]
    async fn shell_run_rejects_direct_secret_dump_command() {
        // arrange
        let temp = tempfile::tempdir().unwrap_or_abort();
        let shell =
            ShellRunTool::with_runner(ShellAllowlist::default(), ShellRunTool::default_runner());

        // act
        let error = shell
            .call(
                shell_test_context(temp.path(), "toolcall-shell-run-env-blocked"),
                json!({
                    "cmd": "env",
                    "cwd": ".",
                }),
            )
            .await
            .expect_err("direct env should be blocked");

        // assert
        assert!(matches!(error, ToolError::CommandBlocked(_)));
    }

    #[tokio::test]
    async fn shell_run_rejects_wrapper_environment_inspection_before_secret_output() {
        // arrange
        let temp = tempfile::tempdir().unwrap_or_abort();
        let fake_secret = "SECRET_TOKEN=redacted-test-secret";

        // act
        for command in ["export", "set", "declare -px", "typeset -px"] {
            let runner = FakeShellCommandRunner::success(fake_secret);
            let shell = ShellRunTool::with_runner(
                ShellAllowlist::default(),
                std::sync::Arc::<FakeShellCommandRunner>::clone(&runner),
            );
            let result = shell
                .call(
                    shell_test_context(temp.path(), "toolcall-shell-run-env-inspect-blocked"),
                    json!({
                        "command": command,
                        "workdir": ".",
                    }),
                )
                .await;

            // assert
            assert_shell_result_blocks_fake_secret(result, fake_secret);
            assert!(runner.calls().is_empty());
        }
    }

    #[tokio::test]
    async fn shell_run_rejects_direct_environment_inspection_before_secret_output() {
        // arrange
        let temp = tempfile::tempdir().unwrap_or_abort();
        let fake_secret = "SECRET_TOKEN=redacted-test-secret";

        // act
        for (cmd, args) in [
            ("export", Vec::<String>::new()),
            ("set", Vec::<String>::new()),
            ("declare", vec!["-px".to_string()]),
            ("typeset", vec!["-px".to_string()]),
        ] {
            let runner = FakeShellCommandRunner::success(fake_secret);
            let shell = ShellRunTool::with_runner(
                ShellAllowlist::default(),
                std::sync::Arc::<FakeShellCommandRunner>::clone(&runner),
            );
            let result = shell
                .call(
                    shell_test_context(
                        temp.path(),
                        "toolcall-shell-run-direct-env-inspect-blocked",
                    ),
                    json!({
                        "cmd": cmd,
                        "args": args,
                        "cwd": ".",
                    }),
                )
                .await;

            // assert
            assert_shell_result_blocks_fake_secret(result, fake_secret);
            assert!(runner.calls().is_empty());
        }
    }

    #[tokio::test]
    async fn shell_run_direct_invocation_scrubs_inherited_environment() {
        // arrange
        let temp = tempfile::tempdir().unwrap_or_abort();
        let shell =
            ShellRunTool::with_runner(ShellAllowlist::default(), ShellRunTool::default_runner());

        // act
        let result = shell
            .call(
                shell_test_context(temp.path(), "toolcall-shell-run-direct-env-scrubbed"),
                json!({
                    "cmd": "awk",
                    "args": [awk_environ_program()],
                    "cwd": ".",
                }),
            )
            .await
            .unwrap_or_abort();

        // assert
        assert_shell_result_scrubs_inherited_environment(&result);
    }

    #[tokio::test]
    async fn shell_run_wrapper_invocation_scrubs_inherited_environment() {
        // arrange
        let temp = tempfile::tempdir().unwrap_or_abort();
        let shell =
            ShellRunTool::with_runner(ShellAllowlist::default(), ShellRunTool::default_runner());

        // act
        let result = shell
            .call(
                shell_test_context(temp.path(), "toolcall-shell-run-wrapper-env-scrubbed"),
                json!({
                    "command": format!("awk '{}'", awk_environ_program()),
                    "workdir": ".",
                }),
            )
            .await
            .unwrap_or_abort();

        // assert
        assert_shell_result_scrubs_inherited_environment(&result);
    }

    #[cfg(target_os = "linux")]
    #[tokio::test]
    async fn shell_run_direct_invocation_hides_parent_environment() {
        // arrange
        let temp = tempfile::tempdir().unwrap_or_abort();
        let shell =
            ShellRunTool::with_runner(ShellAllowlist::default(), ShellRunTool::default_runner());

        // act
        let result = shell
            .call(
                shell_test_context(temp.path(), "toolcall-shell-run-direct-parent-env-hidden"),
                json!({
                    "cmd": "awk",
                    "args": [awk_parent_environ_program()],
                    "cwd": ".",
                }),
            )
            .await
            .unwrap_or_abort();

        // assert
        assert_shell_result_hides_parent_environment(&result);
    }

    #[cfg(target_os = "linux")]
    #[tokio::test]
    async fn shell_run_wrapper_invocation_hides_parent_environment() {
        // arrange
        let temp = tempfile::tempdir().unwrap_or_abort();
        let shell =
            ShellRunTool::with_runner(ShellAllowlist::default(), ShellRunTool::default_runner());

        // act
        let result = shell
            .call(
                shell_test_context(temp.path(), "toolcall-shell-run-wrapper-parent-env-hidden"),
                json!({
                    "command": format!("awk '{}'", awk_parent_environ_program()),
                    "workdir": ".",
                }),
            )
            .await
            .unwrap_or_abort();

        // assert
        assert_shell_result_hides_parent_environment(&result);
    }

    #[cfg(target_os = "linux")]
    #[tokio::test]
    async fn shell_run_direct_invocation_hides_grandparent_environment() {
        // arrange
        let temp = tempfile::tempdir().unwrap_or_abort();
        let shell =
            ShellRunTool::with_runner(ShellAllowlist::default(), ShellRunTool::default_runner());

        // act
        let result = shell
            .call(
                shell_test_context(
                    temp.path(),
                    "toolcall-shell-run-direct-grandparent-env-hidden",
                ),
                json!({
                    "cmd": "awk",
                    "args": [awk_grandparent_environ_program()],
                    "cwd": ".",
                }),
            )
            .await
            .expect(
                "direct awk grandparent env inspection should execute without inherited secrets",
            );

        // assert
        assert_shell_result_hides_parent_environment(&result);
    }

    #[cfg(target_os = "linux")]
    #[tokio::test]
    async fn shell_run_wrapper_invocation_hides_grandparent_environment() {
        // arrange
        let temp = tempfile::tempdir().unwrap_or_abort();
        let shell =
            ShellRunTool::with_runner(ShellAllowlist::default(), ShellRunTool::default_runner());

        // act
        let result = shell
            .call(
                shell_test_context(
                    temp.path(),
                    "toolcall-shell-run-wrapper-grandparent-env-hidden",
                ),
                json!({
                    "command": format!("awk '{}'", awk_grandparent_environ_program()),
                    "workdir": ".",
                }),
            )
            .await
            .expect(
                "wrapper awk grandparent env inspection should execute without inherited secrets",
            );

        // assert
        assert_shell_result_hides_parent_environment(&result);
    }

    fn assert_shell_result_blocks_fake_secret(
        result: Result<harness_core::tool::ToolResult, ToolError>,
        fake_secret: &str,
    ) {
        match result {
            Ok(result) => {
                assert!(!result.display_text.contains(fake_secret));
                if let Some(structured) = &result.structured_json {
                    assert!(!structured.to_string().contains(fake_secret));
                }
                assert!(result.artifacts.is_empty());
                panic!("environment inspection command should be blocked before execution");
            }
            Err(error) => {
                assert!(matches!(error, ToolError::CommandBlocked(_)));
                assert!(!error.to_string().contains(fake_secret));
            }
        }
    }

    fn awk_environ_program() -> &'static str {
        r#"BEGIN { for (k in ENVIRON) print k "=" ENVIRON[k] }"#
    }

    fn awk_parent_environ_program() -> &'static str {
        r#"BEGIN { RS="\0"; slash=sprintf("%c",47); path=slash "proc" slash PROCINFO["ppid"] slash "environ"; while ((getline line < path) > 0) print line }"#
    }

    fn awk_grandparent_environ_program() -> &'static str {
        r#"BEGIN { slash=sprintf("%c",47); status=slash "proc" slash PROCINFO["ppid"] slash "status"; while ((getline line < status) > 0) if (index(line,"PPid:")==1) { sub("^PPid:[ \t]*","",line); gppid=line } close(status); RS="\0"; env=slash "proc" slash gppid slash "environ"; while ((getline line < env) > 0) print line }"#
    }

    fn assert_shell_result_scrubs_inherited_environment(result: &harness_core::tool::ToolResult) {
        for leaked in ["HOME=", "SECRET_TOKEN="] {
            assert!(!result.display_text.contains(leaked));
            if let Some(structured) = &result.structured_json {
                assert!(!structured.to_string().contains(leaked));
            }
        }
        assert!(result.artifacts.is_empty());
    }

    #[cfg(target_os = "linux")]
    fn assert_shell_result_hides_parent_environment(result: &harness_core::tool::ToolResult) {
        for leaked in ["HOME=", "SECRET_TOKEN="] {
            assert!(!result.display_text.contains(leaked));
            if let Some(structured) = &result.structured_json {
                assert!(!structured.to_string().contains(leaked));
            }
        }
        assert!(result.artifacts.is_empty());
    }

    #[tokio::test]
    async fn shell_run_rejects_conflicting_cmd_and_command() {
        let temp = tempfile::tempdir().unwrap_or_abort();
        let shell =
            ShellRunTool::with_runner(ShellAllowlist::default(), ShellRunTool::default_runner());

        let error = shell
            .call(
                shell_test_context(temp.path(), "toolcall-shell-run-conflicting"),
                json!({
                    "cmd": "printf",
                    "command": "echo shell-ok",
                }),
            )
            .await
            .expect_err("conflicting cmd and command should be rejected");

        match error {
            ToolError::InvalidArguments(message) => {
                assert_eq!(message, "provide exactly one of cmd or command");
            }
            other => panic!("unexpected error variant: {other}"),
        }
    }
}
