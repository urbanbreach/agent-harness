// allow: SIZE_OK — coordinator state machine (turn lifecycle + scheduling)
use std::collections::BTreeSet;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use serde_json::Value;

use crate::config::HookLifecycleEvent;
use crate::digest::digest12;
use crate::event::PermissionDecision as EventPermissionDecision;
use crate::path_selector::workspace_relative_path_from_maybe_absolute;
use crate::perm::shell::{direct_shell_command_request, scan_shell_command, ShellCommandRequest};
use crate::perm::{
    always_external_path_prefix, PermissionDecision, PermissionGrant, PermissionGrantMatcher,
    PermissionGrantRequest, PermissionGrantScope, PermissionKind, PermissionPolicy,
    PermissionRuleRequest, PermissionToolSelector, PolicyDecision,
};
use crate::redact::Redactor;
use crate::tool::canonical_tool_id_for;

use super::question::validate_question_answers_reason;
use super::task_category::task_category_fallback_profile;
use super::tool_metadata::effective_mcp_tool_id;
use super::{
    append_permission_grant_recorded_event, append_permission_resolved_event, hooks,
    reject_pending_permission, tool_execution, CoordinatorError, HookInvocationContext,
    PendingPermissionResolution, PendingPermissionState, ToolCallExecutionArgs,
};

#[must_use]
pub(crate) fn permission_policy_denied_response_message(tool_id: &str) -> String {
    format!(
        "tool call denied: permission policy rejected `{tool_id}` \
(do not retry the same call; pick an allowed tool or adjust permission rules)"
    )
}

pub(super) fn event_permission_decision(decision: PermissionDecision) -> EventPermissionDecision {
    match decision {
        PermissionDecision::Allow => EventPermissionDecision::Allow,
        PermissionDecision::Deny => EventPermissionDecision::Deny,
    }
}

pub(super) fn permission_decision_label(decision: EventPermissionDecision) -> &'static str {
    match decision {
        EventPermissionDecision::Allow => "allow",
        EventPermissionDecision::Deny => "deny",
    }
}

pub(super) fn permission_summary(
    redactor: &dyn Redactor,
    tool_id: &str,
    args_json: &Value,
) -> String {
    if let Some(shell_request) = shell_request_from_args(args_json) {
        let command = redactor.redact_text(&shell_request.original);
        let patterns = shell_request
            .patterns
            .iter()
            .map(|pattern| redactor.redact_text(pattern))
            .collect::<Vec<_>>();
        let always_patterns = shell_request
            .always_patterns
            .iter()
            .map(|pattern| redactor.redact_text(pattern))
            .collect::<Vec<_>>();
        let patterns = serde_json::to_string(&patterns).unwrap_or_else(|_| "[]".to_string());
        let always_patterns =
            serde_json::to_string(&always_patterns).unwrap_or_else(|_| "[]".to_string());
        return format!(
            "tool={tool_id} command={} patterns={patterns} always_patterns={always_patterns}",
            command
        );
    }

    let redacted_args = crate::redact::redact_value(redactor, args_json);
    let args = serde_json::to_string(&redacted_args).unwrap_or_else(|_| "null".to_string());
    format!("tool={tool_id} args={args}")
}

// Doom-loop streak identity: digest12(tool_id || 0x1f || serde_json::to_vec(args)).
pub(super) fn permission_request_digest(tool_id: &str, args_json: &Value) -> String {
    let canonical = serde_json::to_vec(args_json).unwrap_or_else(|_| b"null".to_vec());
    let mut bytes = Vec::with_capacity(tool_id.len() + 1 + canonical.len());
    bytes.extend_from_slice(tool_id.as_bytes());
    bytes.push(0x1f);
    bytes.extend_from_slice(&canonical);
    digest12(&bytes)
}

pub(super) fn permission_grant_request(
    workspace_root: &Path,
    kind: PermissionKind,
    tool_id: &str,
    args_json: &Value,
    request_digest: &str,
) -> PermissionGrantRequest {
    PermissionGrantRequest {
        kind,
        tool: permission_tool_selector(tool_id, args_json),
        matcher: permission_grant_matcher(workspace_root, kind, args_json, request_digest),
    }
}

fn permission_tool_selector(tool_id: &str, args_json: &Value) -> PermissionToolSelector {
    let effective_tool_id = effective_mcp_tool_id(tool_id, args_json).unwrap_or_else(|| {
        canonical_tool_id_for(tool_id)
            .unwrap_or(tool_id)
            .to_string()
    });
    let canonical_tool_id = canonical_tool_id_for(tool_id).map(str::to_string);

    PermissionToolSelector {
        effective_tool_id,
        canonical_tool_id,
    }
}

fn permission_grant_matcher(
    workspace_root: &Path,
    kind: PermissionKind,
    args_json: &Value,
    request_digest: &str,
) -> PermissionGrantMatcher {
    match kind {
        PermissionKind::Shell => shell_command_selector(args_json, request_digest)
            .unwrap_or_else(|| request_digest_selector(request_digest)),
        PermissionKind::EditFs | PermissionKind::Read => {
            let paths = workspace_path_selector_paths(workspace_root, args_json);
            if paths.len() == 1 {
                PermissionGrantMatcher::WorkspacePath {
                    path: paths.into_iter().next().unwrap_or_default(),
                    request_digest: request_digest.to_string(),
                }
            } else {
                request_digest_selector(request_digest)
            }
        }
        PermissionKind::ExternalDirectory => {
            let collection = collect_external_directory_paths(workspace_root, "", args_json);
            let path_prefix = collection
                .paths
                .first()
                .map(|path| path.display().to_string())
                .unwrap_or_default();
            PermissionGrantMatcher::ExternalPath {
                path_prefix,
                request_digest: request_digest.to_string(),
            }
        }
        PermissionKind::DoomLoop
        | PermissionKind::Network
        | PermissionKind::Question
        | PermissionKind::Task
        | PermissionKind::WebFetch
        | PermissionKind::WebSearch
        | PermissionKind::CodeSearch
        | PermissionKind::Lsp => request_digest_selector(request_digest),
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(super) struct ExternalDirectoryPathCollection {
    pub(super) paths: Vec<PathBuf>,
    pub(super) hard_deny: Option<String>,
}

pub(super) fn collect_external_directory_paths(
    workspace_root: &Path,
    tool_id: &str,
    args_json: &Value,
) -> ExternalDirectoryPathCollection {
    let canonical_tool = canonical_tool_id_for(tool_id).unwrap_or(tool_id);
    let workspace = workspace_root
        .canonicalize()
        .unwrap_or_else(|_| workspace_root.to_path_buf());

    if matches!(
        canonical_tool,
        "bash" | "shell.run" | "shell.exec" | "shell.command"
    ) || canonical_tool.starts_with("shell.")
    {
        return collect_external_paths_from_bash(&workspace, args_json);
    }

    let paths = collect_external_paths_from_path_args(&workspace, args_json);
    ExternalDirectoryPathCollection {
        paths,
        hard_deny: None,
    }
}

fn collect_external_paths_from_path_args(workspace: &Path, args_json: &Value) -> Vec<PathBuf> {
    let mut raw_paths = BTreeSet::new();
    for key in WORKSPACE_PATH_SELECTOR_KEYS {
        collect_raw_path_strings(args_json.get(key), &mut raw_paths);
    }
    collect_apply_patch_raw_paths(args_json, &mut raw_paths);

    let mut external = BTreeSet::new();
    for raw in raw_paths {
        if let Some(path) = resolve_outside_workspace_path(workspace, &raw) {
            external.insert(path);
        }
    }
    external.into_iter().collect()
}

fn collect_raw_path_strings(value: Option<&Value>, paths: &mut BTreeSet<String>) {
    match value {
        Some(Value::String(raw_path)) => {
            if !raw_path.is_empty() {
                paths.insert(raw_path.clone());
            }
        }
        Some(Value::Array(raw_paths)) => {
            for raw_path in raw_paths.iter().filter_map(Value::as_str) {
                if !raw_path.is_empty() {
                    paths.insert(raw_path.to_string());
                }
            }
        }
        Some(_) | None => {}
    }
}

fn collect_apply_patch_raw_paths(args_json: &Value, paths: &mut BTreeSet<String>) {
    let Some(patch_text) = args_json
        .get("patchText")
        .or_else(|| args_json.get("patch_text"))
        .and_then(Value::as_str)
    else {
        return;
    };
    for line in patch_text.lines() {
        for prefix in APPLY_PATCH_PATH_PREFIXES {
            if let Some(path) = line.strip_prefix(prefix) {
                let trimmed = path.trim();
                if !trimmed.is_empty() {
                    paths.insert(trimmed.to_string());
                }
            }
        }
    }
}

fn collect_external_paths_from_bash(
    workspace: &Path,
    args_json: &Value,
) -> ExternalDirectoryPathCollection {
    let Some(shell_request) = shell_request_from_args(args_json) else {
        return ExternalDirectoryPathCollection::default();
    };

    let mut scanned_tokens = BTreeSet::new();
    let mut external = BTreeSet::new();
    for command in &shell_request.commands {
        for pattern in &command.path_patterns {
            scanned_tokens.insert(pattern.path.clone());
            if is_safe_shell_device_path(&pattern.path) {
                continue;
            }
            if let Some(path) = resolve_outside_workspace_path(workspace, &pattern.path) {
                external.insert(path);
            }
        }
    }

    if let Some(reason) = bash_unscanned_path_hard_deny(&shell_request, &scanned_tokens) {
        return ExternalDirectoryPathCollection {
            paths: external.into_iter().collect(),
            hard_deny: Some(reason),
        };
    }

    ExternalDirectoryPathCollection {
        paths: external.into_iter().collect(),
        hard_deny: None,
    }
}

fn bash_unscanned_path_hard_deny(
    shell_request: &ShellCommandRequest,
    scanned_tokens: &BTreeSet<String>,
) -> Option<String> {
    for command in &shell_request.commands {
        for token in command.tokens.iter().skip(1) {
            if token.starts_with('-') || is_safe_shell_device_path(token) {
                continue;
            }
            if !looks_like_path_token(token) {
                continue;
            }
            if scanned_tokens.contains(token) {
                continue;
            }
            return Some(format!(
                "external_directory: shell path-like token `{token}` was not scanned; \
refusing fail-open outside-workspace access"
            ));
        }
    }
    None
}

fn looks_like_path_token(token: &str) -> bool {
    token.starts_with('/')
        || token.starts_with("./")
        || token.starts_with("../")
        || token == ".."
        || (token.contains('/') && !token.contains('='))
}

fn is_safe_shell_device_path(path: &str) -> bool {
    matches!(
        path,
        "/dev/null" | "/dev/zero" | "/dev/urandom" | "/dev/random" | "NUL" | "nul"
    )
}

fn resolve_outside_workspace_path(workspace: &Path, raw: &str) -> Option<PathBuf> {
    if is_safe_shell_device_path(raw) {
        return None;
    }
    let candidate = if Path::new(raw).is_absolute() {
        PathBuf::from(raw)
    } else {
        workspace.join(raw)
    };
    let normalized = normalize_path_components(&candidate);
    let workspace_canonical = workspace
        .canonicalize()
        .unwrap_or_else(|_| workspace.to_path_buf());
    let normalized_canonical = normalized
        .canonicalize()
        .unwrap_or_else(|_| normalized.clone());

    if normalized_canonical == workspace_canonical
        || normalized_canonical.starts_with(&workspace_canonical)
        || normalized.starts_with(workspace)
    {
        return None;
    }
    Some(normalized_canonical)
}

fn normalize_path_components(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for component in path.components() {
        match component {
            std::path::Component::ParentDir => {
                let _ = out.pop();
            }
            std::path::Component::CurDir => {}
            other => out.push(other.as_os_str()),
        }
    }
    if out.as_os_str().is_empty() {
        path.to_path_buf()
    } else {
        out
    }
}

pub(super) fn call_scoped_external_allow_prefixes(paths: &[PathBuf]) -> Vec<PathBuf> {
    paths.to_vec()
}

pub(super) fn external_directory_grants_authorize(
    run_state: &super::RunState,
    workspace_root: &Path,
    tool_id: &str,
    args_json: &Value,
    external_paths: &[PathBuf],
    request_digest: &str,
) -> bool {
    if external_paths.is_empty() {
        return true;
    }
    let grant_request = permission_grant_request(
        workspace_root,
        PermissionKind::ExternalDirectory,
        tool_id,
        args_json,
        request_digest,
    );
    if run_state.permission_grant_authorizes(&grant_request) {
        return true;
    }
    external_paths.iter().all(|path| {
        let path_request = PermissionGrantRequest {
            kind: PermissionKind::ExternalDirectory,
            tool: grant_request.tool.clone(),
            matcher: PermissionGrantMatcher::ExternalPath {
                path_prefix: path.display().to_string(),
                request_digest: request_digest.to_string(),
            },
        };
        run_state.permission_grant_authorizes(&path_request)
    })
}

pub(super) fn record_external_directory_always_grants(
    run_state: &mut super::RunState,
    permission_id: &str,
    scope: PermissionGrantScope,
    grant_request: &PermissionGrantRequest,
    external_paths: &[PathBuf],
    request_digest: &str,
) -> Vec<PermissionGrant> {
    let mut recorded = Vec::new();
    for (index, path) in external_paths.iter().enumerate() {
        let prefix = always_external_path_prefix(path);
        if prefix == Path::new("/") || prefix.as_os_str().is_empty() {
            continue;
        }
        let grant = PermissionGrant {
            grant_id: format!("grant_{permission_id}_{index}"),
            permission_id: permission_id.to_string(),
            scope,
            expires_at: None,
            kind: PermissionKind::ExternalDirectory,
            tool: grant_request.tool.clone(),
            matcher: PermissionGrantMatcher::ExternalPath {
                path_prefix: prefix.display().to_string(),
                request_digest: request_digest.to_string(),
            },
        };
        run_state.record_permission_grant(grant.clone());
        recorded.push(grant);
    }
    if recorded.is_empty() && !external_paths.is_empty() {
        for (index, path) in external_paths.iter().enumerate() {
            let grant = PermissionGrant {
                grant_id: format!("grant_{permission_id}_exact_{index}"),
                permission_id: permission_id.to_string(),
                scope,
                expires_at: None,
                kind: PermissionKind::ExternalDirectory,
                tool: grant_request.tool.clone(),
                matcher: PermissionGrantMatcher::ExternalPath {
                    path_prefix: path.display().to_string(),
                    request_digest: request_digest.to_string(),
                },
            };
            run_state.record_permission_grant(grant.clone());
            recorded.push(grant);
        }
    }
    recorded
}

pub(super) fn external_directory_summary(paths: &[PathBuf]) -> String {
    let joined = paths
        .iter()
        .map(|path| path.display().to_string())
        .collect::<Vec<_>>()
        .join(", ");
    format!("external_directory paths=[{joined}]")
}

pub(super) fn evaluate_permission_rule_requests(
    policy: &PermissionPolicy,
    category: Option<&str>,
    kind: PermissionKind,
    selectors: &[PermissionRuleRequest],
) -> PolicyDecision {
    if selectors.is_empty() {
        return policy.evaluate_request(category, kind, None);
    }

    let mut ask_decision = None;
    for selector in selectors {
        match policy.evaluate_request(category, kind, Some(selector)) {
            PolicyDecision::Deny => return PolicyDecision::Deny,
            PolicyDecision::Ask {
                timeout_ms,
                default_decision,
            } => {
                ask_decision = Some(PolicyDecision::Ask {
                    timeout_ms,
                    default_decision,
                });
            }
            PolicyDecision::Allow => {}
        }
    }

    ask_decision.unwrap_or(PolicyDecision::Allow)
}

pub(super) fn permission_rule_request_selectors(
    workspace_root: &Path,
    kind: PermissionKind,
    args_json: &Value,
) -> Vec<PermissionRuleRequest> {
    match kind {
        PermissionKind::Shell => shell_command_rule_selector(args_json),
        PermissionKind::EditFs | PermissionKind::Read => {
            workspace_path_rule_selectors(workspace_root, args_json)
        }
        PermissionKind::ExternalDirectory => {
            workspace_path_rule_selectors(workspace_root, args_json)
        }
        PermissionKind::Task => task_agent_rule_selectors(args_json),
        PermissionKind::Network
        | PermissionKind::Question
        | PermissionKind::WebFetch
        | PermissionKind::WebSearch
        | PermissionKind::CodeSearch
        | PermissionKind::Lsp
        | PermissionKind::DoomLoop => Vec::new(),
    }
}

pub(super) fn plan_mode_edit_boundary_denial(
    category: Option<&str>,
    kind: Option<PermissionKind>,
    run_id: &str,
    workspace_root: &Path,
    args_json: &Value,
) -> Option<String> {
    if category != Some(crate::plan::PLAN_AGENT_NAME) || kind != Some(PermissionKind::EditFs) {
        return None;
    }

    let active_plan = crate::plan::plan_file_relative_path(run_id)
        .to_string_lossy()
        .to_string();
    let paths = workspace_path_selector_paths(workspace_root, args_json);
    if !paths.is_empty() && paths.iter().all(|path| path == &active_plan) {
        return active_plan_symlink_denial(workspace_root, &active_plan);
    }

    let requested = if paths.is_empty() {
        "<unresolved path>".to_string()
    } else {
        paths.join(", ")
    };
    Some(format!(
        "plan mode may edit only the active plan file `{active_plan}`; requested `{requested}`"
    ))
}

pub(super) fn plan_mode_shell_boundary_denial(
    category: Option<&str>,
    kind: Option<PermissionKind>,
    args_json: &Value,
) -> Option<String> {
    if category != Some(crate::plan::PLAN_AGENT_NAME) || kind != Some(PermissionKind::Shell) {
        return None;
    }

    let command = args_json
        .get("command")
        .or_else(|| args_json.get("cmd"))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|command| !command.is_empty());
    let Some(command) = command else {
        return Some("plan mode bash requires a read-only inspection command".to_string());
    };

    if is_plan_mode_read_only_shell_command(command) {
        None
    } else {
        Some(format!(
            "plan mode bash may only run read-only inspection commands; requested `{command}`"
        ))
    }
}

fn is_plan_mode_read_only_shell_command(command: &str) -> bool {
    let trimmed = command.trim();
    if trimmed.is_empty()
        || contains_shell_control_operator(trimmed)
        || contains_shell_quote_or_escape(trimmed)
    {
        return false;
    }

    let tokens = trimmed.split_whitespace().collect::<Vec<_>>();
    match tokens.as_slice() {
        ["pwd"] => true,
        ["ls", ..] => true,
        ["git", subcommand, args @ ..] => is_plan_mode_read_only_git_command(subcommand, args),
        _ => false,
    }
}

fn is_plan_mode_read_only_git_command(subcommand: &str, args: &[&str]) -> bool {
    match subcommand {
        "status" | "diff" | "log" | "show" | "rev-parse" | "merge-base" => {
            !contains_git_write_output_arg(args) && !contains_git_exec_capable_arg(args)
        }
        "branch" => is_plan_mode_read_only_git_branch(args),
        _ => false,
    }
}

fn contains_git_write_output_arg(args: &[&str]) -> bool {
    args.iter()
        .any(|arg| *arg == "-o" || *arg == "--output" || arg.starts_with("--output="))
}

fn contains_git_exec_capable_arg(args: &[&str]) -> bool {
    args.iter().any(|arg| {
        matches!(*arg, "--ext-diff" | "--textconv")
            || arg.starts_with("--ext-diff=")
            || arg.starts_with("--textconv=")
    })
}

fn is_plan_mode_read_only_git_branch(args: &[&str]) -> bool {
    const MUTATING_FLAGS: &[&str] = &[
        "-d",
        "-D",
        "-m",
        "-M",
        "-c",
        "-C",
        "--copy",
        "--create-reflog",
        "--delete",
        "--edit-description",
        "--move",
        "--no-track",
        "--set-upstream-to",
        "--track",
        "--unset-upstream",
    ];

    !args.iter().any(|arg| {
        MUTATING_FLAGS.contains(arg)
            || arg.starts_with("--set-upstream-to=")
            || !arg.starts_with('-')
    })
}

fn contains_shell_control_operator(command: &str) -> bool {
    command
        .chars()
        .any(|ch| matches!(ch, '>' | '<' | '|' | '&' | ';' | '`'))
        || command.contains("$(")
}

fn contains_shell_quote_or_escape(command: &str) -> bool {
    command.chars().any(|ch| matches!(ch, '\'' | '"' | '\\'))
}

fn active_plan_symlink_denial(workspace_root: &Path, active_plan: &str) -> Option<String> {
    let mut current = workspace_root.to_path_buf();
    for component in Path::new(active_plan).components() {
        match component {
            std::path::Component::Normal(segment) => current.push(segment),
            std::path::Component::CurDir => continue,
            std::path::Component::ParentDir
            | std::path::Component::RootDir
            | std::path::Component::Prefix(_) => {
                return Some(format!(
                    "plan mode active plan path `{active_plan}` contains an invalid component"
                ));
            }
        }

        match fs::symlink_metadata(&current) {
            Ok(metadata) if metadata.file_type().is_symlink() => {
                return Some(format!(
                    "plan mode active plan path `{active_plan}` must not contain symlink component `{}`",
                    current.display()
                ));
            }
            Ok(_) => {}
            Err(err) if err.kind() == io::ErrorKind::NotFound => return None,
            Err(err) => {
                return Some(format!(
                    "plan mode could not verify active plan path `{active_plan}`: {err}"
                ));
            }
        }
    }
    None
}

fn task_agent_rule_selectors(args_json: &Value) -> Vec<PermissionRuleRequest> {
    let mut nested_selectors = Vec::new();
    if let Some(members) = args_json.get("members").and_then(Value::as_array) {
        for member in members {
            nested_selectors.extend(task_agent_rule_selectors(member));
        }
    }
    if let Some(lead) = args_json.get("lead") {
        nested_selectors.extend(task_agent_rule_selectors(lead));
    }
    if !nested_selectors.is_empty() {
        nested_selectors.sort_by(|left, right| {
            permission_rule_request_key(left).cmp(permission_rule_request_key(right))
        });
        nested_selectors.dedup();
        return nested_selectors;
    }

    let category = trimmed_arg(args_json, "category");
    let subagent_type = ["subagent_type", "agent", "profile", "profileName"]
        .into_iter()
        .find_map(|key| trimmed_arg(args_json, key));

    match (category, subagent_type) {
        (Some(category), Some(subagent_type)) if category == subagent_type => {
            vec![PermissionRuleRequest::TaskAgent(category)]
        }
        (Some(_), Some(subagent_type)) | (None, Some(subagent_type)) => {
            vec![PermissionRuleRequest::TaskAgent(subagent_type)]
        }
        (Some(category), None) => {
            let mut selectors = vec![PermissionRuleRequest::TaskAgent(category.clone())];
            if let Some(fallback) = task_category_fallback_profile(&category) {
                selectors.push(PermissionRuleRequest::TaskAgent(fallback.to_string()));
            }
            selectors
        }
        (None, None) => Vec::new(),
    }
}

fn permission_rule_request_key(selector: &PermissionRuleRequest) -> &str {
    match selector {
        PermissionRuleRequest::ShellCommand { pattern: value }
        | PermissionRuleRequest::WorkspacePath(value)
        | PermissionRuleRequest::TaskAgent(value) => value,
    }
}

fn trimmed_arg(args_json: &Value, key: &str) -> Option<String> {
    args_json
        .get(key)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
}

fn shell_command_rule_selector(args_json: &Value) -> Vec<PermissionRuleRequest> {
    let Some(command) = args_json
        .get("command")
        .or_else(|| args_json.get("cmd"))
        .and_then(Value::as_str)
    else {
        return Vec::new();
    };
    shell_request_from_args(args_json)
        .map(|request| {
            request
                .commands
                .into_iter()
                .map(|command| PermissionRuleRequest::ShellCommand {
                    pattern: command.pattern,
                })
                .collect()
        })
        .unwrap_or_else(|| {
            vec![PermissionRuleRequest::ShellCommand {
                pattern: command.to_string(),
            }]
        })
}

fn workspace_path_rule_selectors(
    workspace_root: &Path,
    args_json: &Value,
) -> Vec<PermissionRuleRequest> {
    workspace_path_selector_paths(workspace_root, args_json)
        .into_iter()
        .map(PermissionRuleRequest::WorkspacePath)
        .collect()
}

fn request_digest_selector(request_digest: &str) -> PermissionGrantMatcher {
    PermissionGrantMatcher::RequestDigest {
        request_digest: request_digest.to_string(),
    }
}

fn shell_command_selector(
    args_json: &Value,
    request_digest: &str,
) -> Option<PermissionGrantMatcher> {
    let command = args_json
        .get("command")
        .or_else(|| args_json.get("cmd"))
        .and_then(Value::as_str)?;
    let mut command_identity = Vec::new();
    command_identity.extend_from_slice(command.as_bytes());
    if let Some(args) = args_json.get("args").and_then(Value::as_array) {
        command_identity.push(0x1f);
        command_identity.extend_from_slice(&serde_json::to_vec(args).ok()?);
    }
    let shell_request = shell_request_from_args(args_json);
    let (patterns, always_patterns) = shell_request
        .map(|request| (request.patterns, request.always_patterns))
        .unwrap_or_else(|| (Vec::new(), Vec::new()));
    Some(PermissionGrantMatcher::ShellCommand {
        command_digest: digest12(&command_identity),
        request_digest: request_digest.to_string(),
        patterns,
        always_patterns,
    })
}

fn shell_request_from_args(args_json: &Value) -> Option<ShellCommandRequest> {
    if let Some(command) = args_json.get("command").and_then(Value::as_str) {
        return scan_shell_command(command).ok();
    }
    let cmd = args_json.get("cmd").and_then(Value::as_str)?;
    let Some(args) = args_json.get("args").and_then(Value::as_array) else {
        return scan_shell_command(cmd).ok();
    };
    let args = args
        .iter()
        .filter_map(Value::as_str)
        .map(str::to_string)
        .collect::<Vec<_>>();
    Some(direct_shell_command_request(cmd, &args))
}

fn workspace_path_selector_paths(workspace_root: &Path, args_json: &Value) -> Vec<String> {
    let mut paths = BTreeSet::new();
    for key in WORKSPACE_PATH_SELECTOR_KEYS {
        collect_workspace_path_selector(workspace_root, args_json.get(key), &mut paths);
    }
    collect_apply_patch_path_selectors(workspace_root, args_json, &mut paths);
    paths.into_iter().collect()
}

const WORKSPACE_PATH_SELECTOR_KEYS: &[&str] = &[
    "file",
    "path",
    "paths",
    "filePath",
    "from_path",
    "fromPath",
    "rename",
    "to_path",
    "toPath",
];

fn collect_workspace_path_selector(
    workspace_root: &Path,
    value: Option<&Value>,
    paths: &mut BTreeSet<String>,
) {
    match value {
        Some(Value::String(raw_path)) => {
            insert_workspace_path_selector(workspace_root, raw_path, paths);
        }
        Some(Value::Array(raw_paths)) => {
            for raw_path in raw_paths.iter().filter_map(Value::as_str) {
                insert_workspace_path_selector(workspace_root, raw_path, paths);
            }
        }
        Some(_) | None => {}
    }
}

fn insert_workspace_path_selector(
    workspace_root: &Path,
    raw_path: &str,
    paths: &mut BTreeSet<String>,
) {
    if let Some(path) =
        workspace_relative_path_from_maybe_absolute(workspace_root, Path::new(raw_path))
    {
        paths.insert(path);
    }
}

fn collect_apply_patch_path_selectors(
    workspace_root: &Path,
    args_json: &Value,
    paths: &mut BTreeSet<String>,
) {
    let Some(patch_text) = args_json
        .get("patchText")
        .or_else(|| args_json.get("patch_text"))
        .and_then(Value::as_str)
    else {
        return;
    };

    for line in patch_text.lines() {
        for prefix in APPLY_PATCH_PATH_PREFIXES {
            if let Some(path) = line.strip_prefix(prefix) {
                insert_workspace_path_selector(workspace_root, path.trim(), paths);
            }
        }
    }
}

const APPLY_PATCH_PATH_PREFIXES: &[&str] = &[
    "*** Add File:",
    "*** Delete File:",
    "*** Update File:",
    "*** Move to:",
];

impl super::Coordinator {
    pub(in crate::coord) async fn resolve_permission_internal(
        &mut self,
        permission_id: String,
        decision: PermissionDecision,
        reason: Option<String>,
        grant_scope: Option<PermissionGrantScope>,
    ) -> Result<(), CoordinatorError> {
        let clock = Arc::clone(&self.clock);
        let redactor = Arc::clone(&self.redactor);
        let job_tx = self.job_tx.clone();

        let run_state = self
            .run_state
            .as_mut()
            .ok_or(CoordinatorError::RunNotStarted)?;

        let Some(existing) = run_state.pending_permission(&permission_id) else {
            return Err(CoordinatorError::UnknownPermission(permission_id));
        };

        let validated_question_answers = if decision == PermissionDecision::Allow {
            match &existing.resolution {
                PendingPermissionResolution::Question { prompts, .. } => Some(
                    validate_question_answers_reason(reason.as_deref(), prompts)
                        .map_err(CoordinatorError::PolicyViolation)?,
                ),
                PendingPermissionResolution::ToolCall { .. } => None,
            }
        } else {
            None
        };

        let Some(pending) = run_state.take_pending_permission(&permission_id) else {
            return Err(CoordinatorError::PolicyViolation(
                "pending permission not found after validation".to_string(),
            ));
        };

        let hook_request_id = pending
            .request_correlation_id
            .clone()
            .or_else(|| Some(pending.tool_call_id.clone()));
        let hook_tool_call_id = pending.tool_call_id.clone();
        let (hook_actor, hook_agent_id, hook_tool_id, hook_category) = match &pending.resolution {
            PendingPermissionResolution::ToolCall {
                tool_id,
                actor,
                category,
                ..
            } => (
                actor.clone(),
                actor.agent_id.clone(),
                Some(tool_id.clone()),
                category.clone(),
            ),
            PendingPermissionResolution::Question { actor, .. } => (
                actor.clone(),
                actor.agent_id.clone(),
                Some("question".to_string()),
                None,
            ),
        };
        let mut permission_hook_executions = pending.hook_executions.clone();
        let permission_decision = event_permission_decision(decision);

        append_permission_resolved_event(
            self.clock.as_ref(),
            self.redactor.as_ref(),
            run_state,
            permission_id.clone(),
            permission_decision,
            reason.clone(),
        )?;

        let resolved_hook_batch = hooks::run_lifecycle_hooks(
            self.clock.as_ref(),
            self.config.hook_command_executor.as_ref(),
            &self.config.hook_runtime_config,
            HookInvocationContext {
                event: HookLifecycleEvent::PermissionResolved,
                run_id: run_state.info.run_id.to_string(),
                workspace_root: run_state.info.workspace_root.clone(),
                artifacts_dir: run_state.info.artifacts_dir.clone(),
                actor: Some(hook_actor),
                agent_id: hook_agent_id,
                request_id: hook_request_id,
                permission_id: Some(permission_id.clone()),
                task_id: None,
                tool_call_id: Some(hook_tool_call_id),
                tool_id: hook_tool_id,
                provider_id: None,
                model_id: None,
                parent_agent_id: None,
                category: hook_category,
                outcome: Some(permission_decision_label(permission_decision).to_string()),
                output_summary: reason.clone(),
                failure_reason: reason.clone(),
            },
        )
        .await;
        permission_hook_executions.extend(resolved_hook_batch.hook_executions.clone());
        let permission_hook_failure = resolved_hook_batch.critical_failure.clone();

        match pending {
            PendingPermissionState {
                tool_call_id,
                request_correlation_id,
                grant_request,
                resolution:
                    PendingPermissionResolution::ToolCall {
                        tool_id,
                        args_json,
                        actor,
                        category,
                        respond_to,
                    },
                ..
            } => {
                let caller_cancelled = respond_to.as_ref().is_some_and(|sender| sender.is_closed());
                if decision == PermissionDecision::Allow
                    && permission_hook_failure.is_none()
                    && !caller_cancelled
                {
                    if let (Some(scope), Some(grant_request)) =
                        (grant_scope, grant_request.as_ref())
                    {
                        if grant_request.kind == PermissionKind::ExternalDirectory {
                            let collection = collect_external_directory_paths(
                                &run_state.info.workspace_root,
                                &tool_id,
                                &args_json,
                            );
                            let recorded = record_external_directory_always_grants(
                                run_state,
                                &permission_id,
                                scope,
                                grant_request,
                                &collection.paths,
                                grant_request.matcher.request_digest(),
                            );
                            for grant in recorded {
                                append_permission_grant_recorded_event(
                                    clock.as_ref(),
                                    redactor.as_ref(),
                                    run_state,
                                    &permission_id,
                                    request_correlation_id.as_deref(),
                                    grant,
                                )?;
                            }
                        } else {
                            if grant_request.kind == PermissionKind::DoomLoop {
                                run_state.doom_loop_always_granted = true;
                            }
                            let grant = PermissionGrant {
                                grant_id: format!("grant_{permission_id}"),
                                permission_id: permission_id.clone(),
                                scope,
                                expires_at: None,
                                kind: grant_request.kind,
                                tool: grant_request.tool.clone(),
                                matcher: grant_request.matcher.clone(),
                            };
                            append_permission_grant_recorded_event(
                                clock.as_ref(),
                                redactor.as_ref(),
                                run_state,
                                &permission_id,
                                request_correlation_id.as_deref(),
                                grant.clone(),
                            )?;
                            run_state.record_permission_grant(grant);
                        }
                    } else if grant_request
                        .as_ref()
                        .is_some_and(|g| g.kind == PermissionKind::DoomLoop)
                    {
                        run_state.reset_identical_tool_call_streak();
                    }

                    let resolved_kind = grant_request.as_ref().map(|g| g.kind);
                    match resolved_kind {
                        Some(PermissionKind::ExternalDirectory) => {
                            let collection = collect_external_directory_paths(
                                &run_state.info.workspace_root,
                                &tool_id,
                                &args_json,
                            );
                            tool_execution::start_tool_call_execution(
                                clock.as_ref(),
                                redactor.as_ref(),
                                Arc::clone(&self.config.hook_command_executor),
                                job_tx,
                                run_state,
                                self.config.hook_runtime_config.clone(),
                                ToolCallExecutionArgs {
                                    tool_call_id,
                                    tool_id,
                                    args_json,
                                    actor,
                                    category,
                                    hook_executions: permission_hook_executions,
                                    tool_registry: Arc::clone(&self.config.tool_registry),
                                    request_correlation_id,
                                    respond_to,
                                    external_directory_allow_prefixes:
                                        call_scoped_external_allow_prefixes(&collection.paths),
                                },
                            )
                            .await?;
                        }
                        Some(PermissionKind::DoomLoop) => {
                            tool_execution::gate_external_directory_and_start(
                                clock.as_ref(),
                                redactor.as_ref(),
                                Arc::clone(&self.config.hook_command_executor),
                                job_tx,
                                run_state,
                                self.config.hook_runtime_config.clone(),
                                &self.config.permission_policy,
                                ToolCallExecutionArgs {
                                    tool_call_id,
                                    tool_id,
                                    args_json,
                                    actor,
                                    category,
                                    hook_executions: permission_hook_executions,
                                    tool_registry: Arc::clone(&self.config.tool_registry),
                                    request_correlation_id,
                                    respond_to,
                                    external_directory_allow_prefixes: Vec::new(),
                                },
                            )
                            .await?;
                        }
                        Some(_) | None => {
                            tool_execution::gate_doom_loop_and_start(
                                clock.as_ref(),
                                redactor.as_ref(),
                                Arc::clone(&self.config.hook_command_executor),
                                job_tx,
                                run_state,
                                self.config.hook_runtime_config.clone(),
                                &self.config.permission_policy,
                                ToolCallExecutionArgs {
                                    tool_call_id,
                                    tool_id,
                                    args_json,
                                    actor,
                                    category,
                                    hook_executions: permission_hook_executions,
                                    tool_registry: Arc::clone(&self.config.tool_registry),
                                    request_correlation_id,
                                    respond_to,
                                    external_directory_allow_prefixes: Vec::new(),
                                },
                            )
                            .await?;
                        }
                    }
                } else {
                    let (rejection_reason, response_message) =
                        if let Some(hook_reason) = permission_hook_failure.as_ref() {
                            (
                                format!("permission denied by lifecycle hook: {hook_reason}"),
                                format!(
                                "tool call denied: critical lifecycle hook failed: {hook_reason}"
                            ),
                            )
                        } else if caller_cancelled {
                            (
                                "tool caller cancelled before permission resolution".to_string(),
                                "tool call cancelled before permission resolution".to_string(),
                            )
                        } else {
                            (
                                "permission denied".to_string(),
                                permission_policy_denied_response_message(&tool_id),
                            )
                        };
                    reject_pending_permission(
                        self.clock.as_ref(),
                        self.redactor.as_ref(),
                        run_state,
                        &rejection_reason,
                        &response_message,
                        PendingPermissionState {
                            tool_call_id,
                            request_correlation_id,
                            hook_executions: permission_hook_executions.clone(),
                            grant_request,
                            resolution: PendingPermissionResolution::ToolCall {
                                tool_id,
                                args_json,
                                actor,
                                category,
                                respond_to,
                            },
                        },
                        &permission_hook_executions,
                    )?;
                }
            }
            PendingPermissionState {
                resolution: PendingPermissionResolution::Question { respond_to, .. },
                ..
            } => {
                if decision == PermissionDecision::Allow && permission_hook_failure.is_none() {
                    let answers = validated_question_answers.unwrap_or_default();
                    let _ = respond_to.send(Ok(answers));
                } else if let Some(hook_reason) = permission_hook_failure.as_ref() {
                    let _ = respond_to.send(Err(format!(
                        "question denied: critical lifecycle hook failed: {hook_reason}"
                    )));
                } else {
                    let _ = respond_to.send(Err(
                        reason.unwrap_or_else(|| "question rejected by user".to_string())
                    ));
                }
            }
        }

        if let Some(reason) = permission_hook_failure {
            return Err(CoordinatorError::LifecycleHookFailed(reason));
        }

        Ok(())
    }

    pub(in crate::coord) async fn resolve_permission_timeout_internal(
        &mut self,
        permission_id: String,
    ) {
        let Some(run_state) = self.run_state.as_mut() else {
            return;
        };

        let Some(pending) = run_state.take_pending_permission(&permission_id) else {
            return;
        };

        let timeout_reason = "permission request timed out".to_string();
        let hook_request_id = pending
            .request_correlation_id
            .clone()
            .or_else(|| Some(pending.tool_call_id.clone()));
        let hook_tool_call_id = pending.tool_call_id.clone();
        let (hook_actor, hook_agent_id, hook_tool_id, hook_category) = match &pending.resolution {
            PendingPermissionResolution::ToolCall {
                tool_id,
                actor,
                category,
                ..
            } => (
                actor.clone(),
                actor.agent_id.clone(),
                Some(tool_id.clone()),
                category.clone(),
            ),
            PendingPermissionResolution::Question { actor, .. } => (
                actor.clone(),
                actor.agent_id.clone(),
                Some("question".to_string()),
                None,
            ),
        };

        let _ = append_permission_resolved_event(
            self.clock.as_ref(),
            self.redactor.as_ref(),
            run_state,
            permission_id.clone(),
            EventPermissionDecision::Deny,
            Some(timeout_reason.clone()),
        );

        let resolved_hook_batch = hooks::run_lifecycle_hooks(
            self.clock.as_ref(),
            self.config.hook_command_executor.as_ref(),
            &self.config.hook_runtime_config,
            HookInvocationContext {
                event: HookLifecycleEvent::PermissionResolved,
                run_id: run_state.info.run_id.to_string(),
                workspace_root: run_state.info.workspace_root.clone(),
                artifacts_dir: run_state.info.artifacts_dir.clone(),
                actor: Some(hook_actor),
                agent_id: hook_agent_id,
                request_id: hook_request_id,
                permission_id: Some(permission_id),
                task_id: None,
                tool_call_id: Some(hook_tool_call_id),
                tool_id: hook_tool_id,
                provider_id: None,
                model_id: None,
                parent_agent_id: None,
                category: hook_category,
                outcome: Some("deny".to_string()),
                output_summary: Some(timeout_reason.clone()),
                failure_reason: Some(timeout_reason.clone()),
            },
        )
        .await;
        let mut permission_hook_executions = pending.hook_executions.clone();
        permission_hook_executions.extend(resolved_hook_batch.hook_executions.clone());
        let permission_hook_failure = resolved_hook_batch.critical_failure.clone();

        let _ = match pending {
            PendingPermissionState {
                tool_call_id,
                request_correlation_id,
                grant_request,
                resolution:
                    PendingPermissionResolution::ToolCall {
                        tool_id,
                        args_json,
                        actor,
                        category,
                        respond_to,
                    },
                ..
            } => {
                let (rejection_reason, response_message) =
                    if let Some(hook_reason) = permission_hook_failure.as_ref() {
                        (
                            format!("permission denied by timeout hook: {hook_reason}"),
                            format!(
                                "tool call timed out: critical lifecycle hook failed: {hook_reason}"
                            ),
                        )
                    } else {
                        (
                            "permission denied by timeout".to_string(),
                            "tool call timed out: permission request timed out".to_string(),
                        )
                    };
                reject_pending_permission(
                    self.clock.as_ref(),
                    self.redactor.as_ref(),
                    run_state,
                    &rejection_reason,
                    &response_message,
                    PendingPermissionState {
                        tool_call_id,
                        request_correlation_id,
                        hook_executions: permission_hook_executions.clone(),
                        grant_request,
                        resolution: PendingPermissionResolution::ToolCall {
                            tool_id,
                            args_json,
                            actor,
                            category,
                            respond_to,
                        },
                    },
                    &permission_hook_executions,
                )
            }
            PendingPermissionState {
                resolution: PendingPermissionResolution::Question { respond_to, .. },
                ..
            } => {
                let reason = if let Some(hook_reason) = permission_hook_failure.as_ref() {
                    format!("question timed out: critical lifecycle hook failed: {hook_reason}")
                } else {
                    "question timed out awaiting user input".to_string()
                };
                let _ = respond_to.send(Err(reason));
                Ok(())
            }
        };
    }
}

#[cfg(test)]
mod permission_deny_message_contract_tests {
    use super::permission_policy_denied_response_message;

    #[test]
    fn permission_policy_deny_message_is_actionable_and_anti_thrash() {
        // arrange
        // act
        let message = permission_policy_denied_response_message("edit");

        // assert
        assert!(
            message.contains("tool call denied: permission policy rejected `edit`"),
            "deny message must name the rejected tool: {message}"
        );
        assert!(
            message.contains("do not retry the same call"),
            "deny message must discourage thrash retries: {message}"
        );
        assert!(
            message.contains("pick an allowed tool or adjust permission rules"),
            "deny message must suggest recovery: {message}"
        );
    }
}

#[cfg(test)]
mod external_directory_path_collect_tests {
    use super::collect_external_directory_paths;
    use serde_json::json;
    use std::path::Path;

    #[test]
    fn collect_external_read_path_includes_absolute_outside() {
        // arrange
        // act
        // assert
        let workspace = Path::new("/workspace/project");
        let collection = collect_external_directory_paths(
            workspace,
            "read",
            &json!({"filePath": "/tmp/outside.txt"}),
        );
        assert!(collection.hard_deny.is_none());
        assert!(
            collection
                .paths
                .iter()
                .any(|p| p.ends_with("outside.txt") || p == Path::new("/tmp/outside.txt")),
            "got {:?}",
            collection.paths
        );
    }

    #[test]
    fn collect_external_skips_in_workspace_relative() {
        // arrange
        // act
        // assert
        let workspace = Path::new("/workspace/project");
        let collection = collect_external_directory_paths(
            workspace,
            "read",
            &json!({"filePath": "src/main.rs"}),
        );
        assert!(collection.paths.is_empty(), "got {:?}", collection.paths);
    }

    #[test]
    fn collect_external_bash_two_absolute_paths() {
        // arrange
        // act
        // assert
        let workspace = Path::new("/workspace/project");
        let collection = collect_external_directory_paths(
            workspace,
            "bash",
            &json!({"command": "cat /tmp/a.txt /tmp/b.txt"}),
        );
        assert!(collection.hard_deny.is_none());
        assert!(
            collection.paths.len() >= 2,
            "expected both external paths; got {:?}",
            collection.paths
        );
    }
}
