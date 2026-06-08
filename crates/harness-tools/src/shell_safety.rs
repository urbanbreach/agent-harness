use std::path::{Path, PathBuf};

use harness_core::config::ShellAllowlist;
use harness_core::tool::{ToolContext, ToolError};

use crate::workspace_paths::normalize_workspace_target_path;

#[derive(Debug, Clone)]
pub(crate) struct ShellSafety {
    allowlist: ShellAllowlist,
}

impl ShellSafety {
    pub(crate) fn new(allowlist: ShellAllowlist) -> Self {
        Self { allowlist }
    }

    pub(crate) fn ensure_executable_allowed(&self, executable: &str) -> Result<(), ToolError> {
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
        reject_shell_interpreter_command_mode(executable, args)?;
        validate_shell_path_arguments(
            &ShellSegmentCommand {
                executable: executable.to_string(),
                args: args.to_vec(),
            },
            cwd,
            workspace_root,
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

        let canonical = cwd
            .canonicalize()
            .map_err(|err| ToolError::Execution(format!("failed to resolve cwd: {err}")))?;

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
            if is_shell_builtin_allowed(executable) {
                if is_path_sensitive_shell_builtin(executable) {
                    validate_shell_path_arguments(&command, &virtual_cwd, workspace_root)?;
                }
                continue;
            }
            self.ensure_executable_allowed(executable)?;
            validate_shell_path_arguments(&command, &virtual_cwd, workspace_root)?;
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
    if !matches!(executable_name, "bash" | "sh" | "zsh" | "fish") {
        return Ok(());
    }

    if args.iter().any(|arg| is_shell_command_option(arg)) {
        Err(ToolError::CommandBlocked(
            "shell interpreters with -c are not allowed in direct shell.run; use the bash command wrapper so shell safety validation applies"
                .to_string(),
        ))
    } else {
        Ok(())
    }
}

fn is_shell_command_option(arg: &str) -> bool {
    arg == "-c" || (arg.starts_with('-') && !arg.starts_with("--") && arg[1..].contains('c'))
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

const ALLOWED_SHELL_BUILTINS: &[&str] = &["echo", "false", "printf", "pwd", "test", "true", "["];

fn is_shell_builtin_allowed(executable: &str) -> bool {
    ALLOWED_SHELL_BUILTINS.contains(&executable)
}

fn is_path_sensitive_shell_builtin(executable: &str) -> bool {
    matches!(executable, "test" | "[")
}

fn resolve_shell_cd_target(
    command: &ShellSegmentCommand,
    cwd: &Path,
    workspace_root: &Path,
    allowlist: &ShellAllowlist,
) -> Result<PathBuf, ToolError> {
    let Some(target) = command
        .args
        .iter()
        .find(|token| !is_shell_env_assignment(token.as_str()))
        .map(String::as_str)
    else {
        return Err(ToolError::CommandBlocked(
            "cd without an explicit target is not allowed".to_string(),
        ));
    };
    if command.args.iter().any(|token| token.starts_with('-')) {
        return Err(ToolError::CommandBlocked(
            "cd options are not allowed in bash".to_string(),
        ));
    }
    if target == "-" || target.starts_with('~') || target.contains('$') {
        return Err(ToolError::CommandBlocked(
            "cd target must be an explicit workspace path".to_string(),
        ));
    }

    let candidate = normalize_shell_workspace_path(target, cwd, workspace_root)?;
    if !candidate.is_dir() {
        return Err(ToolError::CommandBlocked(
            "cd target must be an existing workspace directory".to_string(),
        ));
    }
    if allowlist.cwd_roots.is_empty() {
        return Ok(candidate);
    }

    let canonical_workspace = workspace_root
        .canonicalize()
        .unwrap_or_else(|_| workspace_root.to_path_buf());
    let allowed = allowlist.cwd_roots.iter().any(|root| {
        normalize_workspace_target_path(&canonical_workspace, Path::new(root))
            .map(|allowed_root| candidate.starts_with(allowed_root))
            .unwrap_or(false)
    });
    if allowed {
        Ok(candidate)
    } else {
        Err(ToolError::CommandBlocked(format!(
            "cd target {} is not in allowlist",
            candidate.display()
        )))
    }
}

fn validate_shell_path_arguments(
    command: &ShellSegmentCommand,
    cwd: &Path,
    workspace_root: &Path,
) -> Result<(), ToolError> {
    for token in &command.args {
        let candidate = if token.starts_with('-') {
            if let Some((_, value)) = token.split_once('=') {
                value
            } else if !token.starts_with("--") && token.len() > 2 {
                let mut chars = token.chars();
                chars.next(); // Skip '-'
                chars.next(); // Skip flag character
                chars.as_str()
            } else {
                continue;
            }
        } else {
            token
        };

        if candidate.contains('$') || candidate.starts_with('~') {
            return Err(ToolError::CommandBlocked(
                "shell path expansion is not allowed in bash".to_string(),
            ));
        }

        let mut extracted_path = candidate;
        if let Some(prefix_end) = candidate.find(['*', '?', '[']) {
            extracted_path = &candidate[..prefix_end];
        }
        if extracted_path.is_empty() {
            continue;
        }

        if extracted_path.starts_with('/')
            || extracted_path.starts_with("./")
            || extracted_path.starts_with("../")
            || extracted_path == "."
            || extracted_path == ".."
            || extracted_path.contains('/')
        {
            let _ = normalize_shell_workspace_path(extracted_path, cwd, workspace_root)?;
        }
    }

    Ok(())
}

fn normalize_shell_workspace_path(
    token: &str,
    cwd: &Path,
    workspace_root: &Path,
) -> Result<PathBuf, ToolError> {
    let workspace = workspace_root
        .canonicalize()
        .unwrap_or_else(|_| workspace_root.to_path_buf());
    let candidate = if Path::new(token).is_absolute() {
        PathBuf::from(token)
    } else {
        cwd.join(token)
    };
    let normalized = normalize_workspace_target_path(&workspace, &candidate)?;
    ensure_existing_shell_path_stays_in_workspace(&normalized, &workspace)?;
    Ok(normalized)
}

fn ensure_existing_shell_path_stays_in_workspace(
    candidate: &Path,
    workspace: &Path,
) -> Result<(), ToolError> {
    let Some(existing) = deepest_existing_ancestor(candidate) else {
        return Ok(());
    };

    let canonical = existing
        .canonicalize()
        .map_err(|err| ToolError::Execution(format!("failed to resolve shell path: {err}")))?;
    if canonical.starts_with(workspace) {
        Ok(())
    } else {
        Err(ToolError::PathEscapesWorkspace {
            workspace_root: workspace.display().to_string(),
            path: canonical.display().to_string(),
        })
    }
}

fn deepest_existing_ancestor(path: &Path) -> Option<&Path> {
    let mut current = Some(path);
    while let Some(candidate) = current {
        if candidate.exists() {
            return Some(candidate);
        }
        current = candidate.parent();
    }
    None
}

fn parse_shell_segment_tokens(segment: &str) -> Result<Vec<String>, ToolError> {
    shell_words::split(segment).map_err(|err| {
        ToolError::InvalidArguments(format!("failed to parse command string: {err}"))
    })
}

#[cfg(test)]
mod tests {
    use super::{blocked_shell_command_message, ShellSafety};

    use harness_core::config::ShellAllowlist;
    use harness_core::tool::ToolError;

    use crate::test_support::tool_context;

    #[test]
    fn ensure_executable_allowed_returns_recovery_hints_for_blocked_commands() {
        let safety = ShellSafety::new(ShellAllowlist {
            executables: vec!["git".to_string()],
            cwd_roots: Vec::new(),
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

    #[tokio::test]
    async fn resolve_cwd_rejects_workspace_cwd_outside_allowlist_roots() {
        let tempdir = tempfile::tempdir().expect("tempdir");
        std::fs::create_dir(tempdir.path().join("allowed")).expect("allowed dir");
        std::fs::create_dir(tempdir.path().join("blocked")).expect("blocked dir");
        let safety = ShellSafety::new(ShellAllowlist {
            executables: Vec::new(),
            cwd_roots: vec!["allowed".to_string()],
        });
        let ctx = tool_context(tempdir.path(), "shell-safety-cwd");

        let allowed = safety
            .resolve_cwd(&ctx, Some("allowed"))
            .expect("allowed cwd should resolve");
        assert_eq!(allowed, tempdir.path().join("allowed"));

        let err = safety
            .resolve_cwd(&ctx, Some("blocked"))
            .expect_err("blocked cwd should be rejected");
        assert!(matches!(
            err,
            ToolError::CommandBlocked(message) if message.contains(" is not in allowlist")
        ));
    }

    #[test]
    fn validate_bash_command_rejects_command_substitution() {
        let tempdir = tempfile::tempdir().expect("tempdir");
        let safety = ShellSafety::new(ShellAllowlist {
            executables: vec!["ls".to_string()],
            cwd_roots: vec![".".to_string()],
        });
        let err = safety
            .validate_bash_command("ls $(pwd)", tempdir.path(), tempdir.path())
            .expect_err("command substitution should be blocked");
        assert!(matches!(err, ToolError::CommandBlocked(_)));
    }

    #[test]
    fn validate_bash_command_rejects_external_path_arguments() {
        let tempdir = tempfile::tempdir().expect("tempdir");
        let safety = ShellSafety::new(ShellAllowlist {
            executables: vec!["ls".to_string()],
            cwd_roots: vec![".".to_string()],
        });
        let err = safety
            .validate_bash_command("ls /tmp", tempdir.path(), tempdir.path())
            .expect_err("external path should be blocked");
        assert!(matches!(err, ToolError::PathEscapesWorkspace { .. }));

        let err2 = safety
            .validate_bash_command("ls foo/../../../etc/passwd", tempdir.path(), tempdir.path())
            .expect_err("external relative path should be blocked");
        assert!(matches!(err2, ToolError::PathEscapesWorkspace { .. }));

        let err3 = safety
            .validate_bash_command(
                "ls --files-from=foo/../../../etc/passwd",
                tempdir.path(),
                tempdir.path(),
            )
            .expect_err("external relative path inside option should be blocked");
        assert!(matches!(err3, ToolError::PathEscapesWorkspace { .. }));

        let err4 = safety
            .validate_bash_command("ls foo/../../../etc/pas*", tempdir.path(), tempdir.path())
            .expect_err("external relative path with glob should be blocked");
        assert!(matches!(err4, ToolError::PathEscapesWorkspace { .. }));

        let err5 = safety
            .validate_bash_command(
                "ls -Ifoo/../../../etc/passwd",
                tempdir.path(),
                tempdir.path(),
            )
            .expect_err("external relative path inside short option should be blocked");
        assert!(matches!(err5, ToolError::PathEscapesWorkspace { .. }));
    }

    #[test]
    fn validate_bash_command_returns_recovery_hint_for_find() {
        let tempdir = tempfile::tempdir().expect("tempdir");
        let safety = ShellSafety::new(ShellAllowlist {
            executables: vec!["git".to_string(), "ls".to_string()],
            cwd_roots: vec![".".to_string()],
        });
        let err = safety
            .validate_bash_command(
                "find docs -maxdepth 1 -type f | sort",
                tempdir.path(),
                tempdir.path(),
            )
            .expect_err("find should be blocked with a recovery hint");
        match err {
            ToolError::CommandBlocked(message) => {
                assert_eq!(message, blocked_shell_command_message("find"));
                assert!(message.contains("glob"));
                assert!(message.contains("git status"));
            }
            other => panic!("expected command blocked error, got {other:?}"),
        }
    }

    #[test]
    fn validate_bash_command_rejects_source_builtins() {
        let tempdir = tempfile::tempdir().expect("tempdir");
        let safety = ShellSafety::new(ShellAllowlist {
            executables: vec!["ls".to_string()],
            cwd_roots: vec![".".to_string()],
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
    fn validate_bash_command_rejects_redirection_and_background_execution() {
        let tempdir = tempfile::tempdir().expect("tempdir");
        let safety = ShellSafety::new(ShellAllowlist {
            executables: vec!["ls".to_string()],
            cwd_roots: vec![".".to_string()],
        });

        for command in ["printf hi >/tmp/out", "ls . & python -c pass"] {
            let err = safety
                .validate_bash_command(command, tempdir.path(), tempdir.path())
                .expect_err("redirection and background execution should be blocked");
            assert!(matches!(err, ToolError::CommandBlocked(_)));
        }
    }

    #[test]
    fn validate_bash_command_rejects_expansion_and_env_assignment_bypasses() {
        let tempdir = tempfile::tempdir().expect("tempdir");
        let safety = ShellSafety::new(ShellAllowlist {
            executables: vec!["git".to_string(), "ls".to_string()],
            cwd_roots: vec![".".to_string()],
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
        let tempdir = tempfile::tempdir().expect("tempdir");
        let safety = ShellSafety::new(ShellAllowlist {
            executables: Vec::new(),
            cwd_roots: vec![".".to_string()],
        });

        for command in ["test -e /tmp/outside", "[ -e /tmp/outside ]"] {
            let err = safety
                .validate_bash_command(command, tempdir.path(), tempdir.path())
                .expect_err("path-sensitive builtins should not probe outside workspace");
            assert!(matches!(err, ToolError::PathEscapesWorkspace { .. }));
        }
    }

    #[test]
    fn validate_direct_args_rejects_external_paths_and_shell_command_mode() {
        let tempdir = tempfile::tempdir().expect("tempdir");
        let safety = ShellSafety::new(ShellAllowlist {
            executables: vec!["bash".to_string(), "ls".to_string()],
            cwd_roots: vec![".".to_string()],
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
        let tempdir = tempfile::tempdir().expect("tempdir");
        std::fs::create_dir(tempdir.path().join("subdir")).expect("subdir");
        let safety = ShellSafety::new(ShellAllowlist {
            executables: vec!["ls".to_string()],
            cwd_roots: vec![".".to_string()],
        });

        safety
            .validate_bash_command("cd subdir && ls ../sibling", tempdir.path(), tempdir.path())
            .expect("relative path should be checked from virtual cd cwd");
    }

    #[test]
    fn validate_bash_command_rejects_cd_options_and_missing_targets() {
        let tempdir = tempfile::tempdir().expect("tempdir");
        let safety = ShellSafety::new(ShellAllowlist {
            executables: vec!["ls".to_string()],
            cwd_roots: vec![".".to_string()],
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
        let tempdir = tempfile::tempdir().expect("tempdir");
        let external = tempfile::tempdir().expect("external tempdir");
        std::os::unix::fs::symlink(external.path(), tempdir.path().join("outside"))
            .expect("symlink outside workspace");

        let safety = ShellSafety::new(ShellAllowlist {
            executables: vec!["ls".to_string()],
            cwd_roots: vec![".".to_string()],
        });

        let path_arg_err = safety
            .validate_bash_command("ls outside/missing", tempdir.path(), tempdir.path())
            .expect_err("path arguments through symlinks must not escape workspace");
        assert!(matches!(
            path_arg_err,
            ToolError::PathEscapesWorkspace { .. }
        ));

        let cd_err = safety
            .validate_bash_command("cd outside && ls .", tempdir.path(), tempdir.path())
            .expect_err("cd through symlink must not escape workspace");
        assert!(matches!(cd_err, ToolError::PathEscapesWorkspace { .. }));
    }
}
