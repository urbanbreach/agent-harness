use harness_tools::UnwrapOrAbort;
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard, OnceLock};

use harness_core::agent::AgentProfile;
use harness_core::clock::RealClock;
use harness_core::config::{
    refresh_skills_config_registry, registered_skills_config, HarnessConfig, PermissionMode,
    ShellAllowlist, SkillsConfig, ToolFailureMode,
};
use harness_core::coord::{spawn_coordinator, CoordinatorConfig, RunInfo};
use harness_core::perm::PermissionDecision;
use harness_core::redact::DefaultRedactor;
use harness_core::tool::{ToolContext, ToolRunState};
use harness_tools::coordinator_registry;
use serde_json::json;
use tokio::time::Duration;

#[path = "mod.rs"]
mod common;

use common::{
    allow_all_permission_policy, anonymous_supervisor_actor, repo_root,
    wait_for_question_permission, worker_actor,
};

fn skills_registry_test_lock() -> MutexGuard<'static, ()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
        .lock()
        .unwrap_or_abort()
}

fn tool_context(workspace_root: &Path, tool_call_id: &str) -> ToolContext {
    let coordinator = spawn_coordinator(
        CoordinatorConfig::default(),
        Arc::new(RealClock::new()),
        Arc::new(DefaultRedactor::default()),
    );

    ToolContext {
        run_id: "run-skill-load-tests".into(),
        workspace_root: workspace_root.to_path_buf(),
        artifacts_dir: workspace_root.join(".artifacts"),
        actor: anonymous_supervisor_actor(),
        category: Some("deep".to_string()),
        tool_call_id: tool_call_id.into(),
        current_model_ref: None,
        current_model_settings: None,
        tool_state: ToolRunState::default(),
        coordinator,
    }
}

fn write_skill(root: &Path, name: &str, description: &str, body: &str) {
    let skill_dir = root.join(name);
    fs::create_dir_all(&skill_dir).unwrap_or_abort();
    fs::write(
        skill_dir.join("SKILL.md"),
        format!("---\nname: {name}\ndescription: {description}\n---\n\n{body}\n"),
    )
    .unwrap_or_abort();
}

fn write_invalid_skill(root: &Path, name: &str) {
    let skill_dir = root.join(name);
    fs::create_dir_all(&skill_dir).unwrap_or_abort();
    fs::write(
        skill_dir.join("SKILL.md"),
        format!(
            "---\nname: {name}\ndescription: Broken precedence target\nmetadata: nope\n---\n\nThis skill should be rejected.\n"
        ),
    )
    .unwrap_or_abort();
}

fn write_v1_skill(root: &Path, name: &str, frontmatter: &str, body: &str) {
    let skill_dir = root.join(name);
    fs::create_dir_all(&skill_dir).unwrap_or_abort();
    fs::write(
        skill_dir.join("SKILL.md"),
        format!("---\n{frontmatter}\n---\n\n{body}\n"),
    )
    .unwrap_or_abort();
}

#[allow(clippy::panic, reason = "test fixture code must panic gracefully")]
fn section_body<'a>(body: &'a str, section: &str) -> &'a str {
    let section_start = body
        .find(section)
        .unwrap_or_else(|| panic!("abort"));
    let after_heading = &body[section_start + section.len()..];
    after_heading
        .split("\n## ")
        .next()
        .unwrap_or_abort()
}

fn assert_section_has_content(body: &str, section: &str, skill_name: &str) {
    let content = section_body(body, section)
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .collect::<Vec<_>>()
        .join("\n");
    assert!(
        !content.is_empty(),
        "{skill_name} section `{section}` must contain non-empty guidance"
    );
}

fn quoted_values_after(body: &str, token: &str) -> Vec<String> {
    body.match_indices(token)
        .filter_map(|(index, _)| {
            let after_token = &body[index + token.len()..];
            let quote = after_token.chars().next()?;
            if quote != '"' && quote != '\'' {
                return None;
            }
            let after_quote = &after_token[quote.len_utf8()..];
            let end = after_quote.find(quote)?;
            Some(after_quote[..end].to_string())
        })
        .collect()
}

fn worker_profile(name: &str, toolset: &[&str]) -> AgentProfile {
    AgentProfile {
        name: name.to_string(),
        category: name.to_string(),
        model_ref: format!("default:{name}"),
        model_ref_explicit: true,
        system_prompt: format!("{name} prompt"),
        temperature: None,
        cache_retention: Default::default(),
        max_iters: Some(12),
        tool_failure_mode: ToolFailureMode::FailTurn,
        toolset: toolset.iter().map(|tool| (*tool).to_string()).collect(),
    }
}

async fn spawn_worker_run(
    workspace: &Path,
    profile_name: &str,
    agent_profiles: BTreeMap<String, AgentProfile>,
) -> (harness_core::coord::CoordinatorHandle, RunInfo, String) {
    let session_dir = workspace.join("session-dir");
    fs::create_dir_all(&session_dir).unwrap_or_abort();

    let mut config = CoordinatorConfig::new(session_dir);
    config.permission_policy = allow_all_permission_policy();
    config.tool_registry = Arc::new(coordinator_registry(ShellAllowlist::default()));
    config.agent_profiles = agent_profiles;
    let handle = spawn_coordinator(
        config,
        Arc::new(RealClock::new()),
        Arc::new(DefaultRedactor::default()),
    );
    let run = handle
        .start_run("skill_load_discovery", workspace)
        .await
        .unwrap_or_abort();
    let worker_id = handle
        .spawn_agent(anonymous_supervisor_actor(), profile_name, None)
        .await
        .unwrap_or_abort();
    (handle, run, worker_id)
}

fn harness_config_with_skills(skills: SkillsConfig) -> HarnessConfig {
    serde_json::from_value(json!({
        "providers": {
            "default": {
                "type": "openai_compatible",
                "base_url": "http://127.0.0.1:1/v1",
                "api_key": "DUMMY",
                "api_mode": "responses",
                "models": {
                    "gpt-5.4-mini": {
                        "display_name": "GPT-5.4 Mini"
                    }
                }
            }
        },
        "agents": {
            "deep": {
                "description": "Deep profile",
                "model_ref": "default:gpt-5.4-mini",
                "tools": []
            }
        },
        "permissions": {
            "defaults": {
                "edit": "allow",
                "shell": "allow",
                "network": "allow"
            }
        },
        "runtime": {
            "background_tasks": {
                "default_concurrency": 2,
                "provider_concurrency": 2,
                "model_concurrency": 2,
                "stale_timeout_ms": 30000,
                "message_staleness_timeout_ms": 10000
            },
            "session_dir": ".agent-harness/sessions"
        },
        "integrations": {
            "remote_search": {
                "endpoint": "https://mcp.exa.ai/mcp"
            }
        },
        "skills": serde_json::to_value(skills).unwrap_or_abort()
    }))
    .unwrap_or_abort()
}

struct SkillsConfigGuard {
    previous: SkillsConfig,
}

impl SkillsConfigGuard {
    fn install(skills: SkillsConfig) -> Self {
        let previous = registered_skills_config();
        refresh_skills_config_registry(&harness_config_with_skills(skills));
        Self { previous }
    }
}

impl Drop for SkillsConfigGuard {
    fn drop(&mut self) {
        refresh_skills_config_registry(&harness_config_with_skills(self.previous.clone()));
    }
}

fn skills_config_with_global_root(global_root: PathBuf) -> SkillsConfig {
    SkillsConfig {
        global_roots: vec![global_root],
        ..SkillsConfig::default()
    }
}

fn skills_config_without_global_roots() -> SkillsConfig {
    SkillsConfig {
        global_roots: Vec::new(),
        ..SkillsConfig::default()
    }
}
