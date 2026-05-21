use harness_core::config::{load_config_from_str, AgentMode, PermissionMode, ProfileConfig};

use std::fs;
use std::path::{Path, PathBuf};

const PROJECTION_SOURCES: &[&str] = &[
    "src/goal_ledger.rs",
    "src/persistent_task.rs",
    "src/plan_consensus.rs",
    "src/proj.rs",
    "src/research_mission.rs",
    "src/run_dossier.rs",
    "src/transcript_projection.rs",
    "src/wiki.rs",
    "src/workflow.rs",
    "src/workflow_closeout.rs",
];

const REPLAY_FORBIDDEN_PATTERNS: &[ForbiddenPattern] = &[
    ForbiddenPattern::new(".append(", "replay must not append events"),
    ForbiddenPattern::new("append_payload_event(", "replay must not append events"),
    ForbiddenPattern::new(
        "append_tool_call_requested_event(",
        "replay must not synthesize tool lifecycle events",
    ),
    ForbiddenPattern::new("run_lifecycle_hooks(", "replay must not execute hooks"),
    ForbiddenPattern::new("execute_lifecycle_hook(", "replay must not execute hooks"),
    ForbiddenPattern::new(
        "execute_lifecycle_hook_command(",
        "replay must not execute hook commands",
    ),
    ForbiddenPattern::new(
        "start_tool_call_execution(",
        "replay must not execute native tools",
    ),
    ForbiddenPattern::new(
        "run_provider_stream_phase(",
        "replay must not call providers",
    ),
    ForbiddenPattern::new(
        "stream_assistant_response_once(",
        "replay must not call providers",
    ),
    ForbiddenPattern::new(
        ".execute_agent_tool_call(",
        "replay must not route tool execution",
    ),
    ForbiddenPattern::new(
        ".request_tool_call(",
        "replay must not route tool execution",
    ),
    ForbiddenPattern::new(".spawn_agent(", "replay must not schedule agents"),
    ForbiddenPattern::new(
        ".request_agent_turn(",
        "replay must not schedule provider turns",
    ),
    ForbiddenPattern::new(
        ".resolve_permission(",
        "replay must not resolve permissions",
    ),
    ForbiddenPattern::new(".cancel_task(", "replay must not mutate task state"),
];

const PROJECTION_FORBIDDEN_PATTERNS: &[ForbiddenPattern] = &[
    ForbiddenPattern::new("EventStore", "projections must not depend on event stores"),
    ForbiddenPattern::new(
        "JsonlFileEventStore",
        "projections must not depend on event stores",
    ),
    ForbiddenPattern::new(
        "InMemoryEventStore",
        "projections must not depend on event stores",
    ),
    ForbiddenPattern::new(
        "append_payload_event(",
        "projections must not append events",
    ),
    ForbiddenPattern::new(
        "append_tool_call_requested_event(",
        "projections must not synthesize tool lifecycle events",
    ),
    ForbiddenPattern::new("run_lifecycle_hooks(", "projections must not execute hooks"),
    ForbiddenPattern::new(
        "execute_lifecycle_hook(",
        "projections must not execute hooks",
    ),
    ForbiddenPattern::new(
        "start_tool_call_execution(",
        "projections must not execute native tools",
    ),
    ForbiddenPattern::new(
        "harness_providers::",
        "projections must not depend on provider adapters",
    ),
    ForbiddenPattern::new(
        "ProviderStreamEvent",
        "projections must not consume live provider streams",
    ),
    ForbiddenPattern::new("ToolContext", "projections must not execute tools"),
    ForbiddenPattern::new("ToolRegistry", "projections must not execute tools"),
];

const EVENT_APPEND_PATTERNS: &[ForbiddenPattern] = &[
    ForbiddenPattern::new(
        "event_store.append(",
        "event appends must stay coordinator-owned",
    ),
    ForbiddenPattern::new(
        "append_payload_event(",
        "event appends must stay coordinator-owned",
    ),
    ForbiddenPattern::new(
        "append_payload_event_with_correlation(",
        "event appends must stay coordinator-owned",
    ),
    ForbiddenPattern::new(
        "append_tool_call_requested_event(",
        "tool lifecycle event appends must stay coordinator-owned",
    ),
    ForbiddenPattern::new(
        "append_permission_resolved_event(",
        "permission event appends must stay coordinator-owned",
    ),
    ForbiddenPattern::new(
        "append_tool_call_rejection(",
        "tool rejection event appends must stay coordinator-owned",
    ),
    ForbiddenPattern::new(
        "append_agent_turn_task_scheduled_event(",
        "task scheduling events must stay coordinator-owned",
    ),
    ForbiddenPattern::new(
        "append_background_task_notification_and_schedule(",
        "background wakeup scheduling must stay coordinator-owned",
    ),
];

const TASK_SCHEDULING_PATTERNS: &[ForbiddenPattern] = &[
    ForbiddenPattern::new(
        "schedule_agent_turn(",
        "agent turn scheduling must stay coordinator-owned",
    ),
    ForbiddenPattern::new(
        "append_agent_turn_task_scheduled_event(",
        "task scheduling events must stay coordinator-owned",
    ),
    ForbiddenPattern::new(
        "append_background_task_notification_and_schedule(",
        "background wakeup scheduling must stay coordinator-owned",
    ),
];

const DELEGATED_AGENT_SUPERVISOR_CONTROL_PATTERNS: &[ForbiddenPattern] = &[
    ForbiddenPattern::new(
        "background_output",
        "delegated/native agent prompts must not expose supervisor-owned background retrieval controls",
    ),
    ForbiddenPattern::new(
        "background_cancel",
        "delegated/native agent prompts must not expose supervisor-owned background cancellation controls",
    ),
    ForbiddenPattern::new(
        "run_in_background",
        "delegated/native agent prompts must not teach recursive background task control",
    ),
    ForbiddenPattern::new(
        "load_skills",
        "delegated/native agent prompts must not teach recursive task skill injection",
    ),
];

const LEGACY_TOOL_ALIAS_IDS: &[&str] = &[
    "agent.spawn",
    "code.lsp",
    "code.lsp.rename",
    "edit.hashline_apply",
    "edit.hashline_scan",
    "fs.glob",
    "fs.grep",
    "fs.ls",
    "fs.read",
    "fs.write",
    "search.code",
    "search.web",
    "skill.load",
    "todo.read",
    "todo.write",
    "tool.batch",
    "tool.invalid",
    "user.question",
    "web.fetch",
];

#[test]
fn replay_current_run_events_only_reads_the_event_log() {
    let coord = read_source("src/coord.rs");
    let replay_body = function_body(&coord, "async fn replay_current_run_events")
        .expect("replay_current_run_events function body");

    assert_no_forbidden_patterns(
        [(
            "src/coord.rs::replay_current_run_events",
            replay_body.as_str(),
        )],
        REPLAY_FORBIDDEN_PATTERNS,
    );
}

#[test]
fn replay_projection_sources_do_not_execute_runtime_side_effects() {
    let sources = projection_sources()
        .iter()
        .map(|relative_path| (relative_path.clone(), read_source(relative_path)))
        .collect::<Vec<_>>();
    let named_sources = sources
        .iter()
        .map(|(relative_path, source)| (relative_path.as_str(), source.as_str()));

    assert_no_forbidden_patterns(named_sources, PROJECTION_FORBIDDEN_PATTERNS);
}

#[test]
fn event_append_calls_remain_in_coordinator_source() {
    let violations = rust_sources()
        .into_iter()
        .filter(|path| !is_coordinator_runtime_source(path))
        .flat_map(|path| source_violations(&path, EVENT_APPEND_PATTERNS))
        .collect::<Vec<_>>();

    assert_no_violations(violations);
}

#[test]
fn task_scheduling_calls_remain_in_coordinator_source() {
    let violations = rust_sources()
        .into_iter()
        .filter(|path| !is_coordinator_runtime_source(path))
        .flat_map(|path| source_violations(&path, TASK_SCHEDULING_PATTERNS))
        .collect::<Vec<_>>();

    assert_no_violations(violations);
}

#[test]
fn delegated_agent_prompts_do_not_expose_supervisor_task_controls() {
    let prompt_sources = first_party_files(
        &[".agent-harness/native-agents", ".agent-harness/agents"],
        &["toml", "md"],
    )
    .into_iter()
    .map(|path| {
        let relative_path = workspace_relative_path(&path);
        let source = fs::read_to_string(&path).unwrap_or_else(|error| {
            panic!("failed to read {}: {error}", path.display());
        });
        (relative_path, source)
    })
    .collect::<Vec<_>>();

    let violations = prompt_sources
        .iter()
        .flat_map(|(relative_path, source)| {
            text_violations(
                relative_path,
                source,
                DELEGATED_AGENT_SUPERVISOR_CONTROL_PATTERNS,
            )
        })
        .collect::<Vec<_>>();

    assert_no_violations(violations);
}

#[test]
fn resolved_delegated_profiles_do_not_expose_supervisor_task_controls() {
    let parsed = load_config_from_str(minimal_public_config())
        .expect("minimal public config should load shipped profiles");
    let supervisor_controls = [
        "task",
        "background_output",
        "background_cancel",
        "plan_enter",
        "plan_exit",
        "team_create",
        "team_delete",
        "team_send_message",
        "team_shutdown_approve",
        "team_shutdown_reject",
        "team_shutdown_request",
        "team_task_create",
        "team_task_update",
    ];

    let violations = parsed
        .agents
        .iter()
        .filter(|(_, profile)| {
            profile.mode == AgentMode::Subagent || profile_task_permission(profile) == Some(PermissionMode::Deny)
        })
        .flat_map(|(name, profile)| {
            supervisor_controls
                .iter()
                .filter(|tool_id| profile.tools.iter().any(|tool| tool == **tool_id))
                .map(move |tool_id| {
                    format!(
                        "agent `{name}` exposes supervisor-only control tool `{tool_id}` despite subagent/task-denied profile"
                    )
                })
        })
        .collect::<Vec<_>>();

    assert_no_violations(violations);
}

#[test]
fn native_registry_sources_do_not_register_legacy_alias_ids() {
    let registry_sources = [
        (
            "crates/harness-tools/src/lib.rs",
            read_workspace_source("crates/harness-tools/src/lib.rs"),
        ),
        (
            "crates/harness-tools/src/native_tools.rs",
            read_workspace_source("crates/harness-tools/src/native_tools.rs"),
        ),
    ];

    let violations = registry_sources
        .iter()
        .flat_map(|(relative_path, source)| {
            LEGACY_TOOL_ALIAS_IDS
                .iter()
                .filter(move |alias| source.contains(&format!("\"{alias}\"")))
                .map(move |alias| {
                    format!(
                        "{relative_path} registers or canonicalizes legacy alias `{alias}`; aliases may be accepted only as typed argument compatibility, not canonical permission/tool ids"
                    )
                })
        })
        .collect::<Vec<_>>();

    assert_no_violations(violations);
}

#[derive(Clone, Copy)]
struct ForbiddenPattern {
    token: &'static str,
    reason: &'static str,
}

impl ForbiddenPattern {
    const fn new(token: &'static str, reason: &'static str) -> Self {
        Self { token, reason }
    }
}

fn assert_no_forbidden_patterns<'a>(
    named_sources: impl IntoIterator<Item = (&'a str, &'a str)>,
    patterns: &[ForbiddenPattern],
) {
    let violations = named_sources
        .into_iter()
        .flat_map(|(name, source)| text_violations(name, source, patterns))
        .collect::<Vec<_>>();

    assert_no_violations(violations);
}

fn text_violations(name: &str, source: &str, patterns: &[ForbiddenPattern]) -> Vec<String> {
    source
        .lines()
        .enumerate()
        .filter(|(_, line)| !line.trim_start().starts_with("//"))
        .flat_map(|(index, line)| {
            patterns
                .iter()
                .filter(move |pattern| line.contains(pattern.token))
                .map(move |pattern| {
                    format!(
                        "{name}:{} contains `{}`: {}",
                        index + 1,
                        pattern.token,
                        pattern.reason
                    )
                })
        })
        .collect()
}

fn source_violations(path: &Path, patterns: &[ForbiddenPattern]) -> Vec<String> {
    let source = fs::read_to_string(path).unwrap_or_else(|error| {
        panic!("failed to read {}: {error}", path.display());
    });
    text_violations(&source_relative_path(path), &source, patterns)
}

fn assert_no_violations(violations: Vec<String>) {
    assert!(
        violations.is_empty(),
        "architecture audit violations:\n{}",
        violations.join("\n")
    );
}

fn projection_sources() -> Vec<String> {
    let declared_sources = PROJECTION_SOURCES
        .iter()
        .map(|source| (*source).to_string())
        .collect::<std::collections::BTreeSet<_>>();
    let mut discovered = rust_sources()
        .into_iter()
        .map(|path| source_relative_path(&path))
        .filter(|relative| is_projection_like_source(relative))
        .collect::<std::collections::BTreeSet<_>>();
    discovered.extend(declared_sources);
    discovered.into_iter().collect()
}

fn is_projection_like_source(relative_path: &str) -> bool {
    let Some(file_name) = Path::new(relative_path)
        .file_name()
        .and_then(|file_name| file_name.to_str())
    else {
        return false;
    };
    file_name.contains("projection")
        || file_name.contains("ledger")
        || file_name.contains("dossier")
        || file_name.contains("workflow")
        || file_name == "proj.rs"
        || file_name == "persistent_task.rs"
        || file_name == "plan_consensus.rs"
        || file_name == "research_mission.rs"
        || file_name == "wiki.rs"
}

fn profile_task_permission(profile: &ProfileConfig) -> Option<PermissionMode> {
    profile
        .permissions
        .as_ref()
        .and_then(|permissions| permissions.task.clone())
}

fn minimal_public_config() -> &'static str {
    r#"
    {
      provider: {
        default: {
          type: "openai_compatible",
          options: {
            baseURL: "http://127.0.0.1:8317/v1",
            apiKey: "test-key",
          },
          models: {
            "gpt-4o-mini": { name: "GPT-4o mini" }
          }
        }
      },
      model: "default/gpt-4o-mini",
      small_model: "default/gpt-4o-mini",
      agent: {
        build: { system_prompt: "Build work" }
      },
      default_agent: "build",
      permission: "allow"
    }
    "#
}

fn function_body(source: &str, signature: &str) -> Option<String> {
    let signature_start = source.find(signature)?;
    let body_start = source[signature_start..].find('{')? + signature_start;
    let mut depth = 0usize;

    for (index, ch) in source[body_start..].char_indices() {
        match ch {
            '{' => depth += 1,
            '}' => {
                depth = depth.saturating_sub(1);
                if depth == 0 {
                    let body_end = body_start + index + ch.len_utf8();
                    return Some(source[body_start..body_end].to_string());
                }
            }
            _ => {}
        }
    }

    None
}

fn is_coordinator_runtime_source(path: &Path) -> bool {
    let relative = source_relative_path(path);
    relative == "src/coord.rs" || relative.starts_with("src/coord/")
}

fn rust_sources() -> Vec<PathBuf> {
    let src_dir = manifest_dir().join("src");
    let mut sources = Vec::new();
    collect_rust_sources(&src_dir, &mut sources);
    sources.sort();
    sources
}

fn collect_rust_sources(dir: &Path, sources: &mut Vec<PathBuf>) {
    for entry in fs::read_dir(dir).unwrap_or_else(|error| {
        panic!("failed to read source directory {}: {error}", dir.display());
    }) {
        let path = entry.expect("source directory entry").path();
        if path.is_dir() {
            collect_rust_sources(&path, sources);
        } else if path.extension().and_then(|ext| ext.to_str()) == Some("rs")
            && path.file_name().and_then(|name| name.to_str()) != Some("tests.rs")
        {
            sources.push(path);
        }
    }
}

fn first_party_files(roots: &[&str], extensions: &[&str]) -> Vec<PathBuf> {
    let mut files = Vec::new();
    for root in roots {
        collect_files_with_extensions(&workspace_root().join(root), extensions, &mut files);
    }
    files
}

fn collect_files_with_extensions(path: &Path, extensions: &[&str], files: &mut Vec<PathBuf>) {
    if !path.exists() {
        return;
    }

    if path.is_file() {
        if path
            .extension()
            .and_then(|extension| extension.to_str())
            .is_some_and(|extension| extensions.contains(&extension))
        {
            files.push(path.to_path_buf());
        }
        return;
    }

    let mut entries = fs::read_dir(path)
        .unwrap_or_else(|error| panic!("failed to read directory {}: {error}", path.display()))
        .map(|entry| entry.expect("directory entry").path())
        .collect::<Vec<_>>();
    entries.sort();

    for entry in entries {
        collect_files_with_extensions(&entry, extensions, files);
    }
}

fn read_source(relative_path: &str) -> String {
    let path = manifest_dir().join(relative_path);
    fs::read_to_string(&path).unwrap_or_else(|error| {
        panic!("failed to read {}: {error}", path.display());
    })
}

fn read_workspace_source(relative_path: &str) -> String {
    let path = workspace_root().join(relative_path);
    fs::read_to_string(&path).unwrap_or_else(|error| {
        panic!("failed to read {}: {error}", path.display());
    })
}

fn source_relative_path(path: &Path) -> String {
    path.strip_prefix(manifest_dir())
        .expect("source path inside crate")
        .to_string_lossy()
        .replace('\\', "/")
}

fn workspace_relative_path(path: &Path) -> String {
    path.strip_prefix(workspace_root())
        .unwrap_or(path)
        .to_string_lossy()
        .replace('\\', "/")
}

fn manifest_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn workspace_root() -> PathBuf {
    manifest_dir()
        .parent()
        .and_then(Path::parent)
        .expect("workspace root")
        .to_path_buf()
}
