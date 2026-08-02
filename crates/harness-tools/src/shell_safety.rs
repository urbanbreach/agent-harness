// allow: SIZE_OK — shell safety checking (allowlist + command validation)
use std::path::{Path, PathBuf};

use harness_core::config::{ShellAllowlist, ShellAllowlistMode};
use harness_core::perm::shell::{scan_shell_command, ScannedShellCommand, ShellPathPattern};
use harness_core::tool::{ToolContext, ToolError};
use harness_core::ToolResultExt;

use crate::workspace_paths::normalize_workspace_target_path;
use crate::UnwrapOrAbort;
mod path_validation;
use path_validation::*;

#[derive(Debug, Clone)]
pub(crate) struct ShellSafety {
    allowlist: ShellAllowlist,
}

impl ShellSafety {
    pub(crate) fn new(allowlist: ShellAllowlist) -> Self {
        Self { allowlist }
    }

    pub(crate) fn ensure_executable_allowed(&self, executable: &str) -> Result<(), ToolError> {
        if self.allowlist.mode != ShellAllowlistMode::LegacyExecutables {
            return Ok(());
        }

        if self
            .allowlist
            .executables
            .iter()
            .any(|allowed| allowed == executable)
        {
            Ok(())
        } else {
            Err(ToolError::CommandBlocked(blocked_shell_command_message(
                executable,
            )))
        }
    }

    pub(crate) fn validate_direct_args(
        &self,
        executable: &str,
        args: &[String],
        cwd: &Path,
        workspace_root: &Path,
    ) -> Result<(), ToolError> {
        self.validate_direct_args_with_grants(executable, args, cwd, workspace_root, &[])
    }

    pub(crate) fn validate_direct_args_with_grants(
        &self,
        executable: &str,
        args: &[String],
        cwd: &Path,
        workspace_root: &Path,
        allow_prefixes: &[PathBuf],
    ) -> Result<(), ToolError> {
        validate_shell_executable_position(executable, cwd, workspace_root)?;
        reject_shell_wrapper_builtin(executable)?;
        reject_secret_dump_command(executable)?;
        reject_shell_interpreter_command_mode(executable, args)?;
        validate_shell_path_arguments_with_grants(
            &ShellSegmentCommand {
                executable: executable.to_string(),
                args: args.to_vec(),
            },
            cwd,
            workspace_root,
            allow_prefixes,
        )
    }

    pub(crate) fn resolve_cwd(
        &self,
        ctx: &ToolContext,
        cwd: Option<&str>,
    ) -> Result<PathBuf, ToolError> {
        let cwd = match cwd {
            Some(cwd) => ctx.resolve_workspace_path(Path::new(cwd))?,
            None => ctx.workspace_root.clone(),
        };

        if self.allowlist.cwd_roots.is_empty() {
            return Ok(cwd);
        }

        let canonical = cwd.canonicalize().tool_err("failed to resolve cwd")?;

        let allowed = self.allowlist.cwd_roots.iter().any(|root| {
            ctx.resolve_workspace_path(Path::new(root))
                .map(|allowed_root| canonical.starts_with(allowed_root))
                .unwrap_or(false)
        });

        if !allowed {
            return Err(ToolError::CommandBlocked(format!(
                "cwd {} is not in allowlist",
                canonical.display()
            )));
        }

        Ok(canonical)
    }

    pub(crate) fn validate_bash_command(
        &self,
        command: &str,
        cwd: &Path,
        workspace_root: &Path,
    ) -> Result<(), ToolError> {
        self.validate_bash_command_with_grants(command, cwd, workspace_root, &[])
    }

    pub(crate) fn validate_bash_command_with_grants(
        &self,
        command: &str,
        cwd: &Path,
        workspace_root: &Path,
        allow_prefixes: &[PathBuf],
    ) -> Result<(), ToolError> {
        if self.allowlist.mode == ShellAllowlistMode::LegacyExecutables {
            return self.validate_bash_command_legacy(command, cwd, workspace_root, allow_prefixes);
        }

        self.validate_bash_command_permission_patterns(command, cwd, workspace_root, allow_prefixes)
    }

    fn validate_bash_command_legacy(
        &self,
        command: &str,
        cwd: &Path,
        workspace_root: &Path,
        allow_prefixes: &[PathBuf],
    ) -> Result<(), ToolError> {
        reject_unsupported_bash_constructs(command)?;
        let segments = split_shell_segments(command)?;
        if segments.is_empty() {
            return Err(ToolError::InvalidArguments(
                "command must not be empty".to_string(),
            ));
        }

        let mut virtual_cwd = cwd.to_path_buf();
        for segment in segments {
            let Some(command) = parse_shell_segment_command(&segment)? else {
                continue;
            };
            let executable = command.executable.as_str();
            validate_shell_executable_position(executable, &virtual_cwd, workspace_root)?;
            if executable == "cd" {
                virtual_cwd = resolve_shell_cd_target(
                    &command,
                    &virtual_cwd,
                    workspace_root,
                    &self.allowlist,
                )?;
                continue;
            }
            if matches!(executable, "source" | ".") {
                return Err(ToolError::CommandBlocked(
                    "source and . are not allowed in bash".to_string(),
                ));
            }
            reject_shell_reserved_word(executable)?;
            reject_shell_wrapper_builtin(executable)?;
            reject_secret_dump_command(executable)?;
            if is_shell_builtin_allowed(executable) {
                if is_path_sensitive_shell_builtin(executable) {
                    validate_shell_path_arguments_with_grants(
                        &command,
                        &virtual_cwd,
                        workspace_root,
                        allow_prefixes,
                    )?;
                }
                continue;
            }
            reject_shell_interpreter_command_mode(executable, &command.args)?;
            self.ensure_executable_allowed(executable)?;
            validate_shell_path_arguments_with_grants(
                &command,
                &virtual_cwd,
                workspace_root,
                allow_prefixes,
            )?;
        }

        Ok(())
    }

    fn validate_bash_command_permission_patterns(
        &self,
        command: &str,
        cwd: &Path,
        workspace_root: &Path,
        allow_prefixes: &[PathBuf],
    ) -> Result<(), ToolError> {
        reject_unsupported_bash_constructs(command)?;
        reject_background_execution(command)?;
        if command.trim().is_empty() {
            return Err(ToolError::InvalidArguments(
                "command must not be empty".to_string(),
            ));
        }

        let scanned = scan_shell_command(command).map_err(|err| {
            ToolError::InvalidArguments(format!("failed to parse command string: {err}"))
        })?;

        let mut virtual_cwd = cwd.to_path_buf();
        for command in &scanned.commands {
            reject_scanned_env_assignments(command)?;

            let executable = command.executable.as_str();
            validate_shell_executable_position(executable, &virtual_cwd, workspace_root)?;
            if executable == "cd" {
                virtual_cwd = resolve_shell_cd_target(
                    &scanned_to_segment_command(command),
                    &virtual_cwd,
                    workspace_root,
                    &self.allowlist,
                )?;
                continue;
            }
            if matches!(executable, "source" | ".") {
                return Err(ToolError::CommandBlocked(
                    "source and . are not allowed in bash".to_string(),
                ));
            }
            reject_shell_reserved_word(executable)?;
            reject_shell_wrapper_builtin(executable)?;
            reject_secret_dump_command(executable)?;

            reject_scanned_interpreter_input_redirection(command)?;
            validate_scanned_command_paths_with_grants(
                command,
                &virtual_cwd,
                workspace_root,
                allow_prefixes,
            )?;

            if is_shell_builtin_allowed(executable) {
                if is_path_sensitive_shell_builtin(executable) {
                    validate_shell_path_arguments_with_grants(
                        &scanned_to_segment_command(command),
                        &virtual_cwd,
                        workspace_root,
                        allow_prefixes,
                    )?;
                }
                continue;
            }

            let segment_command = scanned_to_segment_command(command);
            reject_shell_interpreter_command_mode(
                &segment_command.executable,
                &segment_command.args,
            )?;
            validate_shell_path_arguments_with_grants(
                &segment_command,
                &virtual_cwd,
                workspace_root,
                allow_prefixes,
            )?;
        }

        Ok(())
    }
}

fn blocked_shell_command_message(executable: &str) -> String {
    match executable {
        "find" => {
            "find is blocked; use glob for file discovery, list for directory trees, and run git status as a separate bash call"
                .to_string()
        }
        "grep" | "rg" => {
            "grep/rg are blocked; use the grep tool for content search instead of shell search"
                .to_string()
        }
        "cat" | "head" | "tail" | "sed" | "awk" => "text-processing shell commands are blocked; use read for file contents and edit for changes"
            .to_string(),
        _ => executable.to_string(),
    }
}

fn reject_unsupported_bash_constructs(command: &str) -> Result<(), ToolError> {
    if command.contains("$(")
        || command.contains('`')
        || command.contains("<(")
        || command.contains(">(")
    {
        return Err(ToolError::CommandBlocked(
            "command substitution and process substitution are not allowed in bash".to_string(),
        ));
    }

    if contains_unquoted_parameter_expansion(command) {
        return Err(ToolError::CommandBlocked(
            "shell parameter expansion is not allowed in bash".to_string(),
        ));
    }

    if contains_unquoted_brace_expansion(command) {
        return Err(ToolError::CommandBlocked(
            "brace expansion is not allowed in bash".to_string(),
        ));
    }

    if contains_unquoted_compound_shell_syntax(command) {
        return Err(ToolError::CommandBlocked(
            "compound shell syntax is not allowed in bash".to_string(),
        ));
    }

    Ok(())
}

fn contains_unquoted_parameter_expansion(command: &str) -> bool {
    let mut in_single = false;
    let mut in_double = false;
    let mut escaped = false;

    for ch in command.chars() {
        if escaped {
            escaped = false;
            continue;
        }

        match ch {
            '\\' if !in_single => escaped = true,
            '\'' if !in_double => in_single = !in_single,
            '"' if !in_single => in_double = !in_double,
            '$' if !in_single => return true,
            _ => {}
        }
    }

    false
}

fn contains_unquoted_compound_shell_syntax(command: &str) -> bool {
    let mut in_single = false;
    let mut in_double = false;
    let mut escaped = false;

    for ch in command.chars() {
        if escaped {
            escaped = false;
            continue;
        }

        match ch {
            '\\' if !in_single => escaped = true,
            '\'' if !in_double => in_single = !in_single,
            '"' if !in_single => in_double = !in_double,
            '(' | ')' | '{' | '}' if !in_single && !in_double => return true,
            _ => {}
        }
    }

    false
}

fn contains_unquoted_brace_expansion(command: &str) -> bool {
    let mut chars = command.chars().peekable();
    let mut in_single = false;
    let mut in_double = false;
    let mut escaped = false;

    while let Some(ch) = chars.next() {
        if escaped {
            escaped = false;
            continue;
        }

        match ch {
            '\\' if !in_single => escaped = true,
            '\'' if !in_double => in_single = !in_single,
            '"' if !in_single => in_double = !in_double,
            '{' if !in_single && !in_double => {
                if unquoted_brace_body_is_expansion(&mut chars) {
                    return true;
                }
            }
            _ => {}
        }
    }

    false
}

fn unquoted_brace_body_is_expansion(chars: &mut std::iter::Peekable<std::str::Chars<'_>>) -> bool {
    let mut in_single = false;
    let mut in_double = false;
    let mut escaped = false;
    let mut has_comma = false;
    let mut previous_was_dot = false;

    for ch in chars.by_ref() {
        if escaped {
            escaped = false;
            previous_was_dot = false;
            continue;
        }

        match ch {
            '\\' if !in_single => {
                escaped = true;
                previous_was_dot = false;
            }
            '\'' if !in_double => {
                in_single = !in_single;
                previous_was_dot = false;
            }
            '"' if !in_single => {
                in_double = !in_double;
                previous_was_dot = false;
            }
            '}' if !in_single && !in_double => return has_comma,
            ',' if !in_single && !in_double => {
                has_comma = true;
                previous_was_dot = false;
            }
            '.' if !in_single && !in_double && previous_was_dot => return true,
            '.' if !in_single && !in_double => previous_was_dot = true,
            _ => previous_was_dot = false,
        }
    }

    false
}

fn reject_background_execution(command: &str) -> Result<(), ToolError> {
    let mut chars = command.chars().peekable();
    let mut in_single = false;
    let mut in_double = false;
    let mut escaped = false;

    while let Some(ch) = chars.next() {
        if escaped {
            escaped = false;
            continue;
        }

        match ch {
            '\\' if !in_single => escaped = true,
            '\'' if !in_double => in_single = !in_single,
            '"' if !in_single => in_double = !in_double,
            '&' if !in_single && !in_double && chars.peek() == Some(&'&') => {
                chars.next();
            }
            '&' if !in_single && !in_double => {
                return Err(ToolError::CommandBlocked(
                    "background execution is not allowed in bash".to_string(),
                ));
            }
            _ => {}
        }
    }

    Ok(())
}

fn reject_shell_interpreter_command_mode(
    executable: &str,
    args: &[String],
) -> Result<(), ToolError> {
    let executable_name = Path::new(executable)
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or(executable);
    if !is_interpreter_command_mode_executable(executable_name) {
        return Ok(());
    }

    if args
        .iter()
        .any(|arg| is_interpreter_command_option(executable_name, arg))
    {
        return Err(ToolError::CommandBlocked(
            "interpreter command-eval flags such as -c and -e are not allowed in bash; write a workspace script and run it only when the script path is intentionally allowed"
                .to_string(),
        ));
    }

    if args.iter().any(|arg| is_interpreter_stdin_arg(arg)) {
        return Err(ToolError::CommandBlocked(
            "interpreter stdin script mode is not allowed in bash; write a workspace script and run it only when the script path is intentionally allowed"
                .to_string(),
        ));
    }

    if args
        .iter()
        .any(|arg| is_interpreter_module_option(executable_name, arg))
    {
        return Err(ToolError::CommandBlocked(
            "interpreter module execution is not allowed in bash; write a workspace script and run it only when the script path is intentionally allowed"
                .to_string(),
        ));
    }

    if interpreter_script_operand(args).is_none() {
        return Err(ToolError::CommandBlocked(
            "interpreter script path is required in bash; stdin and interactive interpreter modes are not allowed"
                .to_string(),
        ));
    }

    Ok(())
}

fn validate_shell_executable_position(
    executable: &str,
    cwd: &Path,
    workspace_root: &Path,
) -> Result<(), ToolError> {
    if executable.contains('$') || executable.starts_with('~') {
        return Err(ToolError::CommandBlocked(
            "shell executable expansion is not allowed in bash".to_string(),
        ));
    }

    if executable.starts_with('/')
        || executable.starts_with("./")
        || executable.starts_with("../")
        || executable.contains('/')
    {
        let _ = normalize_shell_workspace_path(executable, cwd, workspace_root)?;
        // Folder trust is separate from operator permission allow: deny
        // repository-local executables before spawn when trust is missing/deny.
        ensure_folder_trust_allows_local_executable(executable, workspace_root)?;
    }

    Ok(())
}

fn ensure_folder_trust_allows_local_executable(
    executable: &str,
    workspace_root: &Path,
) -> Result<(), ToolError> {
    use harness_core::folder_trust::{
        gate_repository_local_executable_from_store, LocalExecutableGate,
    };

    match gate_repository_local_executable_from_store(executable, workspace_root) {
        Ok(LocalExecutableGate::Allowed | LocalExecutableGate::NotApplicable) => Ok(()),
        Ok(LocalExecutableGate::Denied { reason }) => Err(ToolError::CommandBlocked(reason)),
        Err(err) => Err(ToolError::CommandBlocked(format!(
            "folder trust check failed before spawn: {err}"
        ))),
    }
}

fn reject_shell_wrapper_builtin(executable: &str) -> Result<(), ToolError> {
    let executable_name = Path::new(executable)
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or(executable);
    if matches!(executable_name, "builtin" | "command" | "eval" | "exec") {
        Err(ToolError::CommandBlocked(
            "shell wrapper builtins are not allowed in bash".to_string(),
        ))
    } else {
        Ok(())
    }
}

fn is_interpreter_command_mode_executable(executable: &str) -> bool {
    matches!(
        executable,
        "bash" | "sh" | "zsh" | "fish" | "python" | "python3" | "node" | "ruby" | "perl"
    )
}

fn is_interpreter_command_option(executable: &str, arg: &str) -> bool {
    arg == "-c"
        || arg == "-e"
        || (arg.starts_with('-') && !arg.starts_with("--") && arg[1..].contains('c'))
        || (arg.starts_with('-') && !arg.starts_with("--") && arg[1..].contains('e'))
        || (executable == "node" && (arg == "-p" || arg.starts_with("-p")))
        || matches!(arg, "--eval" | "--print")
        || arg.starts_with("--eval=")
        || arg.starts_with("--print=")
}

fn is_interpreter_stdin_arg(arg: &str) -> bool {
    arg == "-" || arg.starts_with('<')
}

fn is_interpreter_module_option(executable: &str, arg: &str) -> bool {
    matches!(executable, "python" | "python3") && (arg == "-m" || arg.starts_with("-m"))
}

fn interpreter_script_operand(args: &[String]) -> Option<&str> {
    args.iter()
        .filter(|arg| !is_shell_redirection_token(arg))
        .find(|arg| !arg.starts_with('-'))
        .map(String::as_str)
}

fn reject_scanned_interpreter_input_redirection(
    command: &ScannedShellCommand,
) -> Result<(), ToolError> {
    let executable_name = Path::new(&command.executable)
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or(&command.executable);
    if !is_interpreter_command_mode_executable(executable_name) {
        return Ok(());
    }

    if command
        .tokens
        .iter()
        .skip(1)
        .any(|token| token.starts_with('<'))
    {
        return Err(ToolError::CommandBlocked(
            "interpreter stdin script mode is not allowed in bash; write a workspace script and run it only when the script path is intentionally allowed"
                .to_string(),
        ));
    }

    Ok(())
}

fn reject_secret_dump_command(executable: &str) -> Result<(), ToolError> {
    let executable_name = Path::new(executable)
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or(executable);
    if matches!(
        executable_name,
        "env" | "printenv" | "export" | "set" | "declare" | "typeset"
    ) {
        Err(ToolError::CommandBlocked(
            "environment dumping commands are not allowed in bash".to_string(),
        ))
    } else {
        Ok(())
    }
}

fn reject_shell_reserved_word(executable: &str) -> Result<(), ToolError> {
    if matches!(
        executable,
        "!" | "case"
            | "coproc"
            | "do"
            | "done"
            | "elif"
            | "else"
            | "esac"
            | "fi"
            | "for"
            | "function"
            | "if"
            | "in"
            | "select"
            | "then"
            | "time"
            | "until"
            | "while"
            | "{"
            | "}"
            | "[["
    ) {
        Err(ToolError::CommandBlocked(
            "reserved shell syntax is not allowed in bash".to_string(),
        ))
    } else {
        Ok(())
    }
}

fn split_shell_segments(command: &str) -> Result<Vec<String>, ToolError> {
    let mut segments = Vec::new();
    let mut current = String::new();
    let mut chars = command.chars().peekable();
    let mut in_single = false;
    let mut in_double = false;
    let mut escaped = false;

    while let Some(ch) = chars.next() {
        if escaped {
            current.push(ch);
            escaped = false;
            continue;
        }

        match ch {
            '\\' if !in_single => {
                current.push(ch);
                escaped = true;
            }
            '\'' if !in_double => {
                in_single = !in_single;
                current.push(ch);
            }
            '"' if !in_single => {
                in_double = !in_double;
                current.push(ch);
            }
            '&' if !in_single && !in_double && chars.peek() == Some(&'&') => {
                chars.next();
                push_shell_segment(&mut segments, &mut current);
            }
            '&' if !in_single && !in_double => {
                return Err(ToolError::CommandBlocked(
                    "background execution is not allowed in bash".to_string(),
                ));
            }
            '|' if !in_single && !in_double && chars.peek() == Some(&'|') => {
                chars.next();
                push_shell_segment(&mut segments, &mut current);
            }
            '<' | '>' if !in_single && !in_double => {
                return Err(ToolError::CommandBlocked(
                    "shell redirection is not allowed in bash".to_string(),
                ));
            }
            '|' | ';' | '\n' if !in_single && !in_double => {
                push_shell_segment(&mut segments, &mut current);
            }
            _ => current.push(ch),
        }
    }

    if in_single || in_double || escaped {
        return Err(ToolError::InvalidArguments(
            "failed to parse command string".to_string(),
        ));
    }

    push_shell_segment(&mut segments, &mut current);
    Ok(segments)
}

fn push_shell_segment(segments: &mut Vec<String>, current: &mut String) {
    let trimmed = current.trim();
    if !trimmed.is_empty() {
        segments.push(trimmed.to_string());
    }
    current.clear();
}

struct ShellSegmentCommand {
    executable: String,
    args: Vec<String>,
}

fn parse_shell_segment_command(segment: &str) -> Result<Option<ShellSegmentCommand>, ToolError> {
    let mut tokens = parse_shell_segment_tokens(segment)?.into_iter();

    if let Some(token) = tokens.by_ref().next() {
        if is_shell_env_assignment(&token) {
            return Err(ToolError::CommandBlocked(
                "environment assignments are not allowed in bash".to_string(),
            ));
        }
        return Ok(Some(ShellSegmentCommand {
            executable: token,
            args: tokens.collect(),
        }));
    }

    Ok(None)
}

fn is_shell_env_assignment(token: &str) -> bool {
    let Some((name, _)) = token.split_once('=') else {
        return false;
    };
    !name.is_empty()
        && name
            .chars()
            .all(|ch| ch == '_' || ch.is_ascii_alphanumeric())
        && name
            .chars()
            .next()
            .is_some_and(|ch| ch == '_' || ch.is_ascii_alphabetic())
}

fn reject_scanned_env_assignments(command: &ScannedShellCommand) -> Result<(), ToolError> {
    if command
        .tokens
        .iter()
        .any(|token| is_shell_env_assignment(token.as_str()))
    {
        return Err(ToolError::CommandBlocked(
            "environment assignments are not allowed in bash".to_string(),
        ));
    }

    Ok(())
}

fn scanned_to_segment_command(command: &ScannedShellCommand) -> ShellSegmentCommand {
    ShellSegmentCommand {
        executable: command.executable.clone(),
        args: command
            .tokens
            .iter()
            .skip(1)
            .filter(|token| !is_shell_redirection_token(token))
            .cloned()
            .collect(),
    }
}

fn is_shell_redirection_token(token: &str) -> bool {
    token.contains('<') || token.contains('>')
}

const ALLOWED_SHELL_BUILTINS: &[&str] = &["echo", "false", "printf", "pwd", "test", "true", "["];

fn is_shell_builtin_allowed(executable: &str) -> bool {
    ALLOWED_SHELL_BUILTINS.contains(&executable)
}

fn is_path_sensitive_shell_builtin(executable: &str) -> bool {
    matches!(executable, "test" | "[")
}

fn parse_shell_segment_tokens(segment: &str) -> Result<Vec<String>, ToolError> {
    shell_words::split(segment).map_err(|err| {
        ToolError::InvalidArguments(format!("failed to parse command string: {err}"))
    })
}

#[cfg(test)]
mod tests {
    use harness_core::config::{ShellAllowlist, ShellAllowlistMode};
    use harness_core::tool::ToolError;

    use super::ShellSafety;
    use crate::test_support::tool_context;
    use crate::UnwrapOrAbort;

    #[test]
    fn ensure_executable_allowed_returns_recovery_hints_for_blocked_commands() {
        // arrange
        // act
        // assert
        let safety = ShellSafety::new(ShellAllowlist {
            mode: ShellAllowlistMode::LegacyExecutables,
            executables: vec!["git".to_string()],
            cwd_roots: Vec::new(),
            ..ShellAllowlist::default()
        });

        for (executable, expected_message) in [
            (
                "rg",
                "grep/rg are blocked; use the grep tool for content search instead of shell search",
            ),
            (
                "cat",
                "text-processing shell commands are blocked; use read for file contents and edit for changes",
            ),
            ("python", "python"),
        ] {
            let err = safety
                .ensure_executable_allowed(executable)
                .expect_err("executable should be blocked");
            assert!(matches!(
                err,
                ToolError::CommandBlocked(message) if message == expected_message
            ));
        }
    }

    #[test]
    fn validate_bash_denies_repo_local_executable_when_folder_trust_missing() {
        // arrange
        // act
        // assert
        // Given: workspace without folder trust; local script present (not executed)
        let tempdir = tempfile::tempdir().unwrap_or_abort();
        let workspace = tempdir.path();
        let scripts = workspace.join("scripts");
        std::fs::create_dir_all(&scripts).unwrap_or_abort();
        std::fs::write(scripts.join("tool.sh"), "#!/bin/sh\necho should-not-run\n")
            .unwrap_or_abort();
        let safety = ShellSafety::new(ShellAllowlist::default());

        // When: validation only (no spawn)
        let err = safety
            .validate_bash_command("./scripts/tool.sh", workspace, workspace)
            .expect_err("untrusted local executable must be denied before spawn");

        // Then
        match err {
            ToolError::CommandBlocked(message) => {
                assert!(
                    message.contains("folder trust"),
                    "expected folder trust denial, got: {message}"
                );
            }
            other => panic!("expected CommandBlocked, got {other:?}"),
        }
    }

    #[test]
    fn validate_bash_allows_repo_local_executable_when_folder_trust_allows() {
        // arrange
        // act
        // assert
        // Given: trusted workspace + local script
        let tempdir = tempfile::tempdir().unwrap_or_abort();
        let workspace = tempdir.path();
        let scripts = workspace.join("scripts");
        std::fs::create_dir_all(&scripts).unwrap_or_abort();
        std::fs::write(scripts.join("tool.sh"), "#!/bin/sh\necho ok\n").unwrap_or_abort();
        harness_core::folder_trust::FolderTrustStore::for_workspace(workspace)
            .set(
                workspace,
                harness_core::folder_trust::FolderTrustDecision::Allow,
            )
            .unwrap_or_abort();
        let safety = ShellSafety::new(ShellAllowlist {
            mode: ShellAllowlistMode::PermissionPatterns,
            ..ShellAllowlist::default()
        });

        // When / Then: validation succeeds (still no spawn in this unit test)
        safety
            .validate_bash_command("./scripts/tool.sh", workspace, workspace)
            .unwrap_or_abort();
    }

    #[tokio::test]
    async fn resolve_cwd_rejects_workspace_cwd_outside_allowlist_roots() {
        // arrange
        // act
        // assert
        let tempdir = tempfile::tempdir().unwrap_or_abort();
        std::fs::create_dir(tempdir.path().join("allowed")).unwrap_or_abort();
        std::fs::create_dir(tempdir.path().join("blocked")).unwrap_or_abort();
        let safety = ShellSafety::new(ShellAllowlist {
            executables: Vec::new(),
            cwd_roots: vec!["allowed".to_string()],
            ..ShellAllowlist::default()
        });
        let ctx = tool_context(tempdir.path(), "shell-safety-cwd");

        let allowed = safety.resolve_cwd(&ctx, Some("allowed")).unwrap_or_abort();
        assert_eq!(allowed, tempdir.path().join("allowed"));

        let err = safety
            .resolve_cwd(&ctx, Some("blocked"))
            .expect_err("blocked cwd should be rejected");
        assert!(matches!(
            err,
            ToolError::CommandBlocked(message) if message.contains(" is not in allowlist")
        ));
    }

    fn is_external_directory_denial(err: &ToolError) -> bool {
        match err {
            ToolError::PathEscapesWorkspace { .. } => true,
            ToolError::CommandBlocked(message) => message.contains("external_directory"),
            _ => false,
        }
    }

    #[test]
    fn validate_bash_command_rejects_command_substitution() {
        // arrange
        // act
        // assert
        let tempdir = tempfile::tempdir().unwrap_or_abort();
        let safety = ShellSafety::new(ShellAllowlist {
            executables: vec!["ls".to_string()],
            cwd_roots: vec![".".to_string()],
            ..ShellAllowlist::default()
        });
        let err = safety
            .validate_bash_command("ls $(pwd)", tempdir.path(), tempdir.path())
            .expect_err("command substitution should be blocked");
        assert!(matches!(err, ToolError::CommandBlocked(_)));
    }

    #[test]
    fn validate_bash_command_rejects_external_path_arguments() {
        // arrange
        // act
        // assert
        let tempdir = tempfile::tempdir().unwrap_or_abort();
        let safety = ShellSafety::new(ShellAllowlist {
            executables: vec!["ls".to_string()],
            cwd_roots: vec![".".to_string()],
            ..ShellAllowlist::default()
        });
        let err = safety
            .validate_bash_command("ls /tmp", tempdir.path(), tempdir.path())
            .expect_err("external path should be blocked");
        assert!(is_external_directory_denial(&err), "{:?}", err);

        let err2 = safety
            .validate_bash_command("ls foo/../../../etc/passwd", tempdir.path(), tempdir.path())
            .expect_err("external relative path should be blocked");
        assert!(is_external_directory_denial(&err2), "{:?}", err2);

        let err3 = safety
            .validate_bash_command(
                "ls --files-from=foo/../../../etc/passwd",
                tempdir.path(),
                tempdir.path(),
            )
            .expect_err("external relative path inside option should be blocked");
        assert!(is_external_directory_denial(&err3), "{:?}", err3);

        let err4 = safety
            .validate_bash_command("ls foo/../../../etc/pas*", tempdir.path(), tempdir.path())
            .expect_err("external relative path with glob should be blocked");
        assert!(is_external_directory_denial(&err4), "{:?}", err4);
    }

    #[test]
    fn validate_bash_command_allows_find_in_permission_patterns() {
        // arrange
        // act
        // assert
        let tempdir = tempfile::tempdir().unwrap_or_abort();
        let safety = ShellSafety::new(ShellAllowlist {
            executables: vec!["git".to_string(), "ls".to_string()],
            cwd_roots: vec![".".to_string()],
            ..ShellAllowlist::default()
        });
        safety
            .validate_bash_command(
                "find docs -maxdepth 1 -type f | sort",
                tempdir.path(),
                tempdir.path(),
            )
            .unwrap_or_abort();
    }

    #[test]
    fn validate_bash_command_rejects_source_builtins() {
        // arrange
        // act
        // assert
        let tempdir = tempfile::tempdir().unwrap_or_abort();
        let safety = ShellSafety::new(ShellAllowlist {
            executables: vec!["ls".to_string()],
            cwd_roots: vec![".".to_string()],
            ..ShellAllowlist::default()
        });

        for command in ["source env.sh", ". env.sh"] {
            let err = safety
                .validate_bash_command(command, tempdir.path(), tempdir.path())
                .expect_err("source-style builtins should be blocked");
            assert!(
                matches!(err, ToolError::CommandBlocked(message) if message == "source and . are not allowed in bash")
            );
        }
    }

    #[test]
    fn validate_bash_command_allows_redirection_and_cat() {
        // arrange
        // act
        // assert
        let tempdir = tempfile::tempdir().unwrap_or_abort();
        let safety = ShellSafety::new(ShellAllowlist {
            executables: vec!["ls".to_string()],
            cwd_roots: vec![".".to_string()],
            ..ShellAllowlist::default()
        });

        safety
            .validate_bash_command(
                "printf hi > out.txt && cat out.txt",
                tempdir.path(),
                tempdir.path(),
            )
            .unwrap_or_abort();
    }

    #[test]
    fn validate_bash_command_rejects_background_execution() {
        // arrange
        // act
        // assert
        let tempdir = tempfile::tempdir().unwrap_or_abort();
        let safety = ShellSafety::new(ShellAllowlist {
            executables: vec!["ls".to_string()],
            cwd_roots: vec![".".to_string()],
            ..ShellAllowlist::default()
        });

        let err = safety
            .validate_bash_command("ls . & python -c pass", tempdir.path(), tempdir.path())
            .expect_err("background execution should be blocked");
        assert!(matches!(err, ToolError::CommandBlocked(_)));
    }

    #[test]
    fn validate_bash_command_allows_pipeline_with_grep() {
        // arrange
        // act
        // assert
        let tempdir = tempfile::tempdir().unwrap_or_abort();
        let safety = ShellSafety::new(ShellAllowlist {
            executables: Vec::new(),
            cwd_roots: vec![".".to_string()],
            ..ShellAllowlist::default()
        });

        safety
            .validate_bash_command("printf 'a\\nb\\n' | grep b", tempdir.path(), tempdir.path())
            .unwrap_or_abort();
    }

    #[test]
    fn validate_bash_command_allows_touch_and_rm() {
        // arrange
        // act
        // assert
        let tempdir = tempfile::tempdir().unwrap_or_abort();
        let safety = ShellSafety::new(ShellAllowlist {
            executables: Vec::new(),
            cwd_roots: vec![".".to_string()],
            ..ShellAllowlist::default()
        });

        safety
            .validate_bash_command(
                "touch tmp.txt && rm tmp.txt",
                tempdir.path(),
                tempdir.path(),
            )
            .unwrap_or_abort();
    }

    #[test]
    fn validate_bash_command_rejects_python3_c() {
        // arrange
        let tempdir = tempfile::tempdir().unwrap_or_abort();
        let safety = ShellSafety::new(ShellAllowlist {
            executables: Vec::new(),
            cwd_roots: vec![".".to_string()],
            ..ShellAllowlist::default()
        });

        // act
        let err = safety
            .validate_bash_command("python3 -c \"print('ok')\"", tempdir.path(), tempdir.path())
            .expect_err("python3 -c should be blocked by shell safety");

        // assert
        assert!(matches!(err, ToolError::CommandBlocked(_)));
    }

    #[test]
    fn validate_bash_command_rejects_node_long_eval_modes() {
        // arrange
        let tempdir = tempfile::tempdir().unwrap_or_abort();
        let safety = ShellSafety::new(ShellAllowlist {
            executables: Vec::new(),
            cwd_roots: vec![".".to_string()],
            ..ShellAllowlist::default()
        });

        // act
        for command in [
            "node --eval \"console.log('ok')\"",
            "node --eval=console.log('ok')",
            "node --print \"1 + 1\"",
            "node -p \"1 + 1\"",
        ] {
            let err = safety
                .validate_bash_command(command, tempdir.path(), tempdir.path())
                .expect_err("node eval/print modes should be blocked");

            // assert
            assert!(matches!(err, ToolError::CommandBlocked(_)));
        }
    }

    #[test]
    fn validate_bash_command_rejects_python3_stdin_script_mode() {
        // arrange
        let tempdir = tempfile::tempdir().unwrap_or_abort();
        let safety = ShellSafety::new(ShellAllowlist {
            executables: Vec::new(),
            cwd_roots: vec![".".to_string()],
            ..ShellAllowlist::default()
        });

        // act
        let heredoc_err = safety
            .validate_bash_command(
                "python3 - <<'PY'\nprint('ok')\nPY",
                tempdir.path(),
                tempdir.path(),
            )
            .expect_err("python3 stdin heredoc should be blocked by shell safety");
        let heredoc_without_dash_err = safety
            .validate_bash_command(
                "python3 <<'PY'\nprint('ok')\nPY",
                tempdir.path(),
                tempdir.path(),
            )
            .expect_err("python3 heredoc without dash should be blocked by shell safety");
        let stdin_arg_err = safety
            .validate_direct_args(
                "python3",
                &["-".to_string()],
                tempdir.path(),
                tempdir.path(),
            )
            .expect_err("direct python3 stdin mode should be blocked by shell safety");

        // assert
        assert!(matches!(heredoc_err, ToolError::CommandBlocked(_)));
        assert!(matches!(
            heredoc_without_dash_err,
            ToolError::CommandBlocked(_)
        ));
        assert!(matches!(stdin_arg_err, ToolError::CommandBlocked(_)));
    }

    #[test]
    fn validate_bash_command_rejects_interpreter_input_redirection() {
        // arrange
        let tempdir = tempfile::tempdir().unwrap_or_abort();
        std::fs::write(tempdir.path().join("script.sh"), "printf ok\n").unwrap_or_abort();
        let safety = ShellSafety::new(ShellAllowlist {
            executables: Vec::new(),
            cwd_roots: vec![".".to_string()],
            ..ShellAllowlist::default()
        });

        // act
        let err = safety
            .validate_bash_command("bash < script.sh", tempdir.path(), tempdir.path())
            .expect_err("interpreter input redirection should be blocked");

        // assert
        assert!(matches!(err, ToolError::CommandBlocked(_)));
    }

    #[test]
    fn validate_bash_command_allows_interpreter_script_output_redirection() {
        // arrange
        let tempdir = tempfile::tempdir().unwrap_or_abort();
        std::fs::write(tempdir.path().join("script.py"), "print('ok')\n").unwrap_or_abort();
        let safety = ShellSafety::new(ShellAllowlist {
            executables: Vec::new(),
            cwd_roots: vec![".".to_string()],
            ..ShellAllowlist::default()
        });

        // act
        let result = safety.validate_bash_command(
            "python3 script.py > out.txt",
            tempdir.path(),
            tempdir.path(),
        );

        // assert
        result.unwrap_or_abort();
    }

    #[test]
    fn validate_bash_command_rejects_secret_dump_commands_in_permission_patterns() {
        // arrange
        let tempdir = tempfile::tempdir().unwrap_or_abort();
        let safety = ShellSafety::new(ShellAllowlist {
            executables: Vec::new(),
            cwd_roots: vec![".".to_string()],
            ..ShellAllowlist::default()
        });

        // act
        for command in ["env", "printenv"] {
            let err = safety
                .validate_bash_command(command, tempdir.path(), tempdir.path())
                .expect_err("environment dump command should be blocked");

            // assert
            assert!(matches!(err, ToolError::CommandBlocked(_)));
        }
    }

    #[test]
    fn validate_bash_command_rejects_environment_inspection_builtins() {
        // arrange
        let tempdir = tempfile::tempdir().unwrap_or_abort();
        let safety = ShellSafety::new(ShellAllowlist::default());

        // act
        for command in ["export", "set", "declare -px", "typeset -px"] {
            let err = safety
                .validate_bash_command(command, tempdir.path(), tempdir.path())
                .expect_err("environment inspection builtins should be blocked");

            // assert
            assert!(matches!(err, ToolError::CommandBlocked(_)));
        }
    }

    #[test]
    fn validate_bash_command_rejects_shell_wrapper_bypasses() {
        // arrange
        let tempdir = tempfile::tempdir().unwrap_or_abort();
        let safety = ShellSafety::new(ShellAllowlist {
            executables: Vec::new(),
            cwd_roots: vec![".".to_string()],
            ..ShellAllowlist::default()
        });

        // act
        for command in [
            "command bash -c 'printf bypass'",
            "eval \"python3 -c 'print(1)'\"",
            "command printenv",
        ] {
            let err = safety
                .validate_bash_command(command, tempdir.path(), tempdir.path())
                .expect_err("shell wrapper builtins must not bypass safety checks");

            // assert
            assert!(matches!(err, ToolError::CommandBlocked(_)));
        }
    }

    #[test]
    fn validate_bash_command_rejects_brace_expansion_bypasses() {
        // arrange
        let tempdir = tempfile::tempdir().unwrap_or_abort();
        let safety = ShellSafety::new(ShellAllowlist {
            executables: Vec::new(),
            cwd_roots: vec![".".to_string()],
            ..ShellAllowlist::default()
        });

        // act
        for command in [
            "{/bin/printf,%s\\n,ok}",
            "{python3,-c,print(1)}",
            "{printenv,PATH}",
            "ls {~/secret,}",
        ] {
            let err = safety
                .validate_bash_command(command, tempdir.path(), tempdir.path())
                .expect_err("brace expansion must not bypass shell safety");

            // assert
            assert!(matches!(err, ToolError::CommandBlocked(_)));
        }
    }

    #[test]
    fn validate_bash_command_rejects_executable_path_escapes() {
        // arrange
        let tempdir = tempfile::tempdir().unwrap_or_abort();
        let safety = ShellSafety::new(ShellAllowlist {
            executables: Vec::new(),
            cwd_roots: vec![".".to_string()],
            ..ShellAllowlist::default()
        });

        // act
        for command in ["/tmp/evil", "../outside/tool", "$SHELL -c 'printf bypass'"] {
            let err = safety
                .validate_bash_command(command, tempdir.path(), tempdir.path())
                .expect_err(
                    "executable-position paths and expansions must stay inside safety checks",
                );

            // assert
            assert!(matches!(
                err,
                ToolError::CommandBlocked(_) | ToolError::PathEscapesWorkspace { .. }
            ));
        }
    }

    #[test]
    fn validate_bash_command_rejects_parameter_expanded_executables() {
        // arrange
        let tempdir = tempfile::tempdir().unwrap_or_abort();
        let safety = ShellSafety::new(ShellAllowlist {
            executables: Vec::new(),
            cwd_roots: vec![".".to_string()],
            ..ShellAllowlist::default()
        });

        // act
        for command in [
            "p${UNSET}ython3 -c 'print(1)'",
            "py${UNSET}thon3 -c 'print(1)'",
        ] {
            let err = safety
                .validate_bash_command(command, tempdir.path(), tempdir.path())
                .expect_err("expanded executable words must not bypass shell safety");

            // assert
            assert!(matches!(err, ToolError::CommandBlocked(_)));
        }
    }

    #[test]
    fn validate_bash_command_rejects_parameter_expansion_secret_reads() {
        // arrange
        let tempdir = tempfile::tempdir().unwrap_or_abort();
        let safety = ShellSafety::new(ShellAllowlist::default());

        // act
        let err = safety
            .validate_bash_command("printf ${OPENAI_API_KEY}", tempdir.path(), tempdir.path())
            .expect_err("parameter expansion must not expose secrets");

        // assert
        assert!(matches!(err, ToolError::CommandBlocked(_)));
    }

    #[test]
    fn validate_bash_command_rejects_compound_and_reserved_shell_syntax() {
        // arrange
        let tempdir = tempfile::tempdir().unwrap_or_abort();
        let safety = ShellSafety::new(ShellAllowlist::default());

        // act
        for command in ["if true; then pwd; fi", "while true; do pwd; done"] {
            let err = safety
                .validate_bash_command(command, tempdir.path(), tempdir.path())
                .expect_err("reserved compound shell syntax should be blocked");

            // assert
            assert!(matches!(err, ToolError::CommandBlocked(_)));
        }
    }

    #[test]
    fn validate_bash_command_rejects_interpreter_pipeline_stdin_and_no_script() {
        // arrange
        let tempdir = tempfile::tempdir().unwrap_or_abort();
        let safety = ShellSafety::new(ShellAllowlist::default());

        // act
        for command in ["printf 'print(1)' | python3", "python3"] {
            let err = safety
                .validate_bash_command(command, tempdir.path(), tempdir.path())
                .expect_err("interpreter stdin and no-script modes should be blocked");

            // assert
            assert!(matches!(err, ToolError::CommandBlocked(_)));
        }
    }

    #[test]
    fn validate_bash_command_allows_shell_globs_and_dev_null_redirect() {
        // arrange
        // act
        // assert
        let tempdir = tempfile::tempdir().unwrap_or_abort();
        let safety = ShellSafety::new(ShellAllowlist {
            executables: vec!["git".to_string(), "find".to_string(), "wc".to_string()],
            cwd_roots: vec![".".to_string()],
            ..ShellAllowlist::default()
        });

        safety
            .validate_bash_command(
                "git log --oneline -5 2>/dev/null",
                tempdir.path(),
                tempdir.path(),
            )
            .unwrap_or_abort();
        safety
            .validate_bash_command(
                "find crates -name '*.rs' | wc -l",
                tempdir.path(),
                tempdir.path(),
            )
            .unwrap_or_abort();
        safety
            .validate_bash_command("ls le*", tempdir.path(), tempdir.path())
            .unwrap_or_abort();
    }

    #[test]
    fn validate_bash_command_legacy_mode_allows_workspace_globs() {
        // arrange
        // act
        // assert
        let tempdir = tempfile::tempdir().unwrap_or_abort();
        let safety = ShellSafety::new(ShellAllowlist {
            mode: ShellAllowlistMode::LegacyExecutables,
            executables: vec!["ls".to_string()],
            cwd_roots: vec![".".to_string()],
        });

        safety
            .validate_bash_command("ls le*", tempdir.path(), tempdir.path())
            .unwrap_or_abort();
    }

    #[test]
    fn validate_direct_args_rejects_secret_dump_commands() {
        // arrange
        let tempdir = tempfile::tempdir().unwrap_or_abort();
        let safety = ShellSafety::new(ShellAllowlist::default());

        // act
        for command in ["env", "printenv"] {
            let err = safety
                .validate_direct_args(command, &[], tempdir.path(), tempdir.path())
                .expect_err("direct environment dump command should be blocked");

            // assert
            assert!(matches!(err, ToolError::CommandBlocked(_)));
        }
    }

    #[test]
    fn validate_bash_command_rejects_redirection_workspace_escape() {
        // arrange
        // act
        // assert
        let tempdir = tempfile::tempdir().unwrap_or_abort();
        let safety = ShellSafety::new(ShellAllowlist {
            executables: Vec::new(),
            cwd_roots: vec![".".to_string()],
            ..ShellAllowlist::default()
        });

        let err = safety
            .validate_bash_command("printf hi > ../outside.txt", tempdir.path(), tempdir.path())
            .expect_err("redirection outside workspace should be blocked");
        assert!(is_external_directory_denial(&err), "{:?}", err);
    }

    #[test]
    fn validate_bash_command_rejects_env_assignment_in_permission_patterns() {
        // arrange
        // act
        // assert
        let tempdir = tempfile::tempdir().unwrap_or_abort();
        let safety = ShellSafety::new(ShellAllowlist {
            executables: Vec::new(),
            cwd_roots: vec![".".to_string()],
            ..ShellAllowlist::default()
        });

        let err = safety
            .validate_bash_command("PATH=. git status", tempdir.path(), tempdir.path())
            .expect_err("environment assignment should be blocked");
        assert!(matches!(err, ToolError::CommandBlocked(_)));
    }

    #[test]
    fn validate_bash_command_rejects_expansion_and_env_assignment_bypasses() {
        // arrange
        // act
        // assert
        let tempdir = tempfile::tempdir().unwrap_or_abort();
        let safety = ShellSafety::new(ShellAllowlist {
            executables: vec!["git".to_string(), "ls".to_string()],
            cwd_roots: vec![".".to_string()],
            ..ShellAllowlist::default()
        });

        for command in [
            "ls ~/../../../etc/passwd",
            "ls ~/.ssh/id_rsa",
            "ls --files-from=~/../../../etc/passwd",
            "ls $HOME/.ssh",
            "PATH=. git status",
            "export PATH=. ; git status",
        ] {
            let err = safety
                .validate_bash_command(command, tempdir.path(), tempdir.path())
                .expect_err("expansion and env assignment bypasses should be blocked");
            assert!(matches!(err, ToolError::CommandBlocked(_)));
        }
    }

    #[test]
    fn validate_bash_command_checks_path_sensitive_builtins() {
        // arrange
        // act
        // assert
        let tempdir = tempfile::tempdir().unwrap_or_abort();
        let safety = ShellSafety::new(ShellAllowlist {
            executables: Vec::new(),
            cwd_roots: vec![".".to_string()],
            ..ShellAllowlist::default()
        });

        for command in ["test -e /tmp/outside", "[ -e /tmp/outside ]"] {
            let err = safety
                .validate_bash_command(command, tempdir.path(), tempdir.path())
                .expect_err("path-sensitive builtins should not probe outside workspace");
            assert!(is_external_directory_denial(&err), "{:?}", err);
        }
    }

    #[test]
    fn validate_direct_args_rejects_external_paths_and_shell_command_mode() {
        // arrange
        // act
        // assert
        let tempdir = tempfile::tempdir().unwrap_or_abort();
        let safety = ShellSafety::new(ShellAllowlist {
            executables: vec!["bash".to_string(), "ls".to_string()],
            cwd_roots: vec![".".to_string()],
            ..ShellAllowlist::default()
        });

        let external_path_err = safety
            .validate_direct_args("ls", &["/tmp".to_string()], tempdir.path(), tempdir.path())
            .expect_err("direct shell.run path args should stay inside workspace");
        assert!(matches!(
            external_path_err,
            ToolError::PathEscapesWorkspace { .. }
        ));

        let shell_command_err = safety
            .validate_direct_args(
                "bash",
                &["-lc".to_string(), "printf bypass".to_string()],
                tempdir.path(),
                tempdir.path(),
            )
            .expect_err("direct shell.run bash -lc should be blocked");
        assert!(matches!(shell_command_err, ToolError::CommandBlocked(_)));
    }

    #[test]
    fn validate_bash_command_checks_relative_paths_after_cd() {
        // arrange
        // act
        // assert
        let tempdir = tempfile::tempdir().unwrap_or_abort();
        std::fs::create_dir(tempdir.path().join("subdir")).unwrap_or_abort();
        let safety = ShellSafety::new(ShellAllowlist {
            executables: vec!["ls".to_string()],
            cwd_roots: vec![".".to_string()],
            ..ShellAllowlist::default()
        });

        safety
            .validate_bash_command("cd subdir && ls ../sibling", tempdir.path(), tempdir.path())
            .unwrap_or_abort();
    }

    #[test]
    fn validate_bash_command_rejects_cd_options_and_missing_targets() {
        // arrange
        // act
        // assert
        let tempdir = tempfile::tempdir().unwrap_or_abort();
        let safety = ShellSafety::new(ShellAllowlist {
            executables: vec!["ls".to_string()],
            cwd_roots: vec![".".to_string()],
            ..ShellAllowlist::default()
        });

        for command in ["cd -P /tmp && pwd", "cd missing && ls ../sibling"] {
            let err = safety
                .validate_bash_command(command, tempdir.path(), tempdir.path())
                .expect_err("unsafe cd forms should be blocked");
            assert!(matches!(err, ToolError::CommandBlocked(_)));
        }
    }

    #[cfg(unix)]
    #[test]
    fn validate_bash_command_rejects_symlink_path_escapes() {
        // arrange
        // act
        // assert
        let tempdir = tempfile::tempdir().unwrap_or_abort();
        let external = tempfile::tempdir().unwrap_or_abort();
        std::os::unix::fs::symlink(external.path(), tempdir.path().join("outside"))
            .unwrap_or_abort();

        let safety = ShellSafety::new(ShellAllowlist {
            executables: vec!["ls".to_string()],
            cwd_roots: vec![".".to_string()],
            ..ShellAllowlist::default()
        });

        let path_arg_err = safety
            .validate_bash_command("ls outside/missing", tempdir.path(), tempdir.path())
            .expect_err("path arguments through symlinks must not escape workspace");
        assert!(
            is_external_directory_denial(&path_arg_err),
            "{path_arg_err:?}"
        );

        let cd_err = safety
            .validate_bash_command("cd outside && ls .", tempdir.path(), tempdir.path())
            .expect_err("cd through symlink must not escape workspace");
        assert!(is_external_directory_denial(&cd_err), "{cd_err:?}");
    }

    #[cfg(unix)]
    #[test]
    fn validate_bash_command_rejects_bare_symlink_path_escapes() {
        // arrange
        let tempdir = tempfile::tempdir().unwrap_or_abort();
        let external = tempfile::tempdir().unwrap_or_abort();
        std::os::unix::fs::symlink(external.path(), tempdir.path().join("leak")).unwrap_or_abort();
        let safety = ShellSafety::new(ShellAllowlist {
            executables: Vec::new(),
            cwd_roots: vec![".".to_string()],
            ..ShellAllowlist::default()
        });

        // act
        for command in ["cat leak", "bash leak", "node leak"] {
            let err = safety
                .validate_bash_command(command, tempdir.path(), tempdir.path())
                .expect_err("bare symlink file operands must not escape workspace");

            // assert
            assert!(is_external_directory_denial(&err), "{:?}", err);
        }
    }

    #[cfg(unix)]
    #[test]
    fn validate_bash_command_allows_workspace_globs_even_when_symlink_exists() {
        // arrange
        // act
        // assert
        let tempdir = tempfile::tempdir().unwrap_or_abort();
        let external = tempfile::tempdir().unwrap_or_abort();
        std::os::unix::fs::symlink(external.path(), tempdir.path().join("leak")).unwrap_or_abort();
        let safety = ShellSafety::new(ShellAllowlist {
            executables: Vec::new(),
            cwd_roots: vec![".".to_string()],
            ..ShellAllowlist::default()
        });
        for command in ["cat l*", "ls le*", "bash le*"] {
            safety
                .validate_bash_command(command, tempdir.path(), tempdir.path())
                .unwrap_or_abort();
        }
        let err = safety
            .validate_bash_command("cat leak", tempdir.path(), tempdir.path())
            .expect_err("explicit symlink operand must still escape-check");
        assert!(is_external_directory_denial(&err), "{:?}", err);
    }
}
