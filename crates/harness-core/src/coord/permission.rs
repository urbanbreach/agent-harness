use std::collections::BTreeSet;
use std::fs;
use std::io;
use std::path::Path;

use serde_json::Value;

use crate::digest::digest12;
use crate::event::PermissionDecision as EventPermissionDecision;
use crate::path_selector::workspace_relative_path_from_maybe_absolute;
use crate::perm::{
    PermissionDecision, PermissionGrantMatcher, PermissionGrantRequest, PermissionKind,
    PermissionPolicy, PermissionRuleRequest, PermissionToolSelector, PolicyDecision,
};
use crate::redact::Redactor;
use crate::tool::canonical_tool_id_for;

use super::task_category::task_category_fallback_profile;
use super::tool_metadata::effective_mcp_tool_id;

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
    let redacted_args = crate::redact::redact_value(redactor, args_json);
    let args = serde_json::to_string(&redacted_args).unwrap_or_else(|_| "null".to_string());
    format!("tool={tool_id} args={args}")
}

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
        PermissionKind::EditFs => {
            let paths = workspace_path_selector_paths(workspace_root, args_json);
            if paths.len() == 1 {
                PermissionGrantMatcher::WorkspacePath {
                    path: paths.into_iter().next().expect("single path exists"),
                    request_digest: request_digest.to_string(),
                }
            } else {
                request_digest_selector(request_digest)
            }
        }
        _ => request_digest_selector(request_digest),
    }
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
        PermissionKind::Shell => shell_command_rule_selector(args_json).into_iter().collect(),
        PermissionKind::EditFs => workspace_path_rule_selectors(workspace_root, args_json),
        PermissionKind::Task => task_agent_rule_selectors(args_json),
        PermissionKind::Network
        | PermissionKind::Question
        | PermissionKind::WebFetch
        | PermissionKind::WebSearch
        | PermissionKind::CodeSearch
        | PermissionKind::Lsp => Vec::new(),
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
    let mut team_selectors = Vec::new();
    if let Some(members) = args_json.get("members").and_then(Value::as_array) {
        for member in members {
            team_selectors.extend(task_agent_rule_selectors(member));
        }
    }
    if let Some(lead) = args_json.get("lead") {
        team_selectors.extend(task_agent_rule_selectors(lead));
    }
    if !team_selectors.is_empty() {
        team_selectors.sort_by(|left, right| {
            permission_rule_request_key(left).cmp(permission_rule_request_key(right))
        });
        team_selectors.dedup();
        return team_selectors;
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
        PermissionRuleRequest::ShellCommand(value)
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

fn shell_command_rule_selector(args_json: &Value) -> Option<PermissionRuleRequest> {
    args_json
        .get("command")
        .or_else(|| args_json.get("cmd"))
        .and_then(Value::as_str)
        .map(|command| PermissionRuleRequest::ShellCommand(command.to_string()))
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
    Some(PermissionGrantMatcher::ShellCommand {
        command_digest: digest12(&command_identity),
        request_digest: request_digest.to_string(),
    })
}

fn workspace_path_selector_paths(workspace_root: &Path, args_json: &Value) -> Vec<String> {
    let mut paths = BTreeSet::new();
    for key in WORKSPACE_PATH_SELECTOR_KEYS {
        collect_workspace_path_selector(workspace_root, args_json.get(key), &mut paths);
    }
    paths.into_iter().collect()
}

const WORKSPACE_PATH_SELECTOR_KEYS: &[&str] = &[
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
