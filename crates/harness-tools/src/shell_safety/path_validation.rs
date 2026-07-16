use super::*;

pub(super) fn resolve_shell_cd_target(
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

pub(super) fn validate_shell_path_arguments(
    command: &ShellSegmentCommand,
    cwd: &Path,
    workspace_root: &Path,
) -> Result<(), ToolError> {
    validate_shell_path_arguments_with_grants(command, cwd, workspace_root, &[])
}

pub(super) fn validate_shell_path_arguments_with_grants(
    command: &ShellSegmentCommand,
    cwd: &Path,
    workspace_root: &Path,
    allow_prefixes: &[PathBuf],
) -> Result<(), ToolError> {
    for (index, token) in command.args.iter().enumerate() {
        let candidate = if token.starts_with('-') {
            if let Some((_, value)) = token.split_once('=') {
                value
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

        if contains_shell_glob_metachar(candidate)
            && should_block_glob_path_argument(command, index, candidate, cwd)
        {
            return Err(ToolError::CommandBlocked(
                "shell glob path expansion is not allowed in bash; use the glob tool instead"
                    .to_string(),
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
            || should_validate_bare_path_argument(command, index, extracted_path, cwd)
        {
            let _ = normalize_shell_workspace_path_with_grants(
                extracted_path,
                cwd,
                workspace_root,
                allow_prefixes,
            )?;
        }
    }

    Ok(())
}

pub(super) fn should_validate_bare_path_argument(
    command: &ShellSegmentCommand,
    index: usize,
    path: &str,
    cwd: &Path,
) -> bool {
    if !tracks_shell_bare_path_arguments(&command.executable) {
        return false;
    }
    if is_grep_pattern_argument(&command.executable, &command.args, index) {
        return false;
    }
    cwd.join(path).exists()
}

pub(super) fn should_block_glob_path_argument(
    command: &ShellSegmentCommand,
    index: usize,
    candidate: &str,
    cwd: &Path,
) -> bool {
    if is_grep_pattern_argument(&command.executable, &command.args, index) {
        return false;
    }
    candidate.starts_with('/')
        || candidate.starts_with("./")
        || candidate.starts_with("../")
        || candidate.contains('/')
        || tracks_shell_bare_path_arguments(&command.executable)
        || cwd.join(candidate).exists()
}

pub(super) fn tracks_shell_bare_path_arguments(executable: &str) -> bool {
    matches!(
        executable,
        "cat"
            | "bash"
            | "cp"
            | "find"
            | "fish"
            | "grep"
            | "head"
            | "ls"
            | "mv"
            | "node"
            | "perl"
            | "python"
            | "python3"
            | "rg"
            | "rm"
            | "ruby"
            | "sed"
            | "sh"
            | "tail"
            | "touch"
            | "zsh"
    )
}

pub(super) fn is_grep_pattern_argument(executable: &str, args: &[String], index: usize) -> bool {
    if !matches!(executable, "grep" | "rg") {
        return false;
    }

    args.iter()
        .enumerate()
        .filter(|(_, token)| !token.starts_with('-'))
        .next()
        .is_some_and(|(pattern_index, _)| pattern_index == index)
}

pub(super) fn validate_scanned_command_paths(
    command: &ScannedShellCommand,
    cwd: &Path,
    workspace_root: &Path,
) -> Result<(), ToolError> {
    validate_scanned_command_paths_with_grants(command, cwd, workspace_root, &[])
}

pub(super) fn validate_scanned_command_paths_with_grants(
    command: &ScannedShellCommand,
    cwd: &Path,
    workspace_root: &Path,
    allow_prefixes: &[PathBuf],
) -> Result<(), ToolError> {
    for pattern in &command.path_patterns {
        validate_scanned_path_pattern_with_grants(pattern, cwd, workspace_root, allow_prefixes)?;
    }

    Ok(())
}

pub(super) fn validate_scanned_path_pattern(
    pattern: &ShellPathPattern,
    cwd: &Path,
    workspace_root: &Path,
) -> Result<(), ToolError> {
    validate_scanned_path_pattern_with_grants(pattern, cwd, workspace_root, &[])
}

pub(super) fn validate_scanned_path_pattern_with_grants(
    pattern: &ShellPathPattern,
    cwd: &Path,
    workspace_root: &Path,
    allow_prefixes: &[PathBuf],
) -> Result<(), ToolError> {
    if pattern.path.contains('$') || pattern.path.starts_with('~') {
        return Err(ToolError::CommandBlocked(
            "shell path expansion is not allowed in bash".to_string(),
        ));
    }

    if contains_shell_glob_metachar(&pattern.path) {
        return Err(ToolError::CommandBlocked(
            "shell glob path expansion is not allowed in bash; use the glob tool instead"
                .to_string(),
        ));
    }

    let _ = normalize_shell_workspace_path_with_grants(
        &pattern.path,
        cwd,
        workspace_root,
        allow_prefixes,
    )?;
    Ok(())
}

pub(super) fn contains_shell_glob_metachar(path: &str) -> bool {
    path.contains('*') || path.contains('?') || path.contains('[')
}

pub(super) fn normalize_shell_workspace_path(
    token: &str,
    cwd: &Path,
    workspace_root: &Path,
) -> Result<PathBuf, ToolError> {
    normalize_shell_workspace_path_with_grants(token, cwd, workspace_root, &[])
}

pub(super) fn normalize_shell_workspace_path_with_grants(
    token: &str,
    cwd: &Path,
    workspace_root: &Path,
    allow_prefixes: &[PathBuf],
) -> Result<PathBuf, ToolError> {
    let workspace = workspace_root
        .canonicalize()
        .unwrap_or_else(|_| workspace_root.to_path_buf());
    let candidate = if Path::new(token).is_absolute() {
        PathBuf::from(token)
    } else {
        cwd.join(token)
    };
    match normalize_workspace_target_path(&workspace, &candidate) {
        Ok(normalized) => {
            ensure_existing_shell_path_stays_in_workspace_with_grants(
                &normalized,
                &workspace,
                allow_prefixes,
            )?;
            Ok(normalized)
        }
        Err(ToolError::PathEscapesWorkspace { path, .. }) => {
            let external = PathBuf::from(&path);
            if external_shell_path_authorized(&external, allow_prefixes) {
                ensure_existing_shell_path_stays_in_workspace_with_grants(
                    &external,
                    &workspace,
                    allow_prefixes,
                )?;
                Ok(external)
            } else {
                Err(ToolError::PathEscapesWorkspace {
                    workspace_root: workspace.display().to_string(),
                    path,
                })
            }
        }
        Err(err) => Err(err),
    }
}

pub(super) fn ensure_existing_shell_path_stays_in_workspace(
    candidate: &Path,
    workspace: &Path,
) -> Result<(), ToolError> {
    ensure_existing_shell_path_stays_in_workspace_with_grants(candidate, workspace, &[])
}

pub(super) fn ensure_existing_shell_path_stays_in_workspace_with_grants(
    candidate: &Path,
    workspace: &Path,
    allow_prefixes: &[PathBuf],
) -> Result<(), ToolError> {
    let Some(existing) = deepest_existing_ancestor(candidate) else {
        return Ok(());
    };

    let canonical = existing
        .canonicalize()
        .tool_err("failed to resolve shell path")?;
    if canonical.starts_with(workspace) || external_shell_path_authorized(&canonical, allow_prefixes)
    {
        Ok(())
    } else {
        Err(ToolError::PathEscapesWorkspace {
            workspace_root: workspace.display().to_string(),
            path: canonical.display().to_string(),
        })
    }
}

fn external_shell_path_authorized(path: &Path, allow_prefixes: &[PathBuf]) -> bool {
    !allow_prefixes.is_empty()
        && allow_prefixes
            .iter()
            .any(|prefix| path == prefix.as_path() || path.starts_with(prefix))
}

pub(super) fn deepest_existing_ancestor(path: &Path) -> Option<&Path> {
    let mut current = Some(path);
    while let Some(candidate) = current {
        if candidate.exists() {
            return Some(candidate);
        }
        current = candidate.parent();
    }
    None
}
