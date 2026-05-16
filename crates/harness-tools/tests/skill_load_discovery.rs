use std::collections::{BTreeMap, BTreeSet};
use std::env;
use std::ffi::OsString;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use harness_core::agent::AgentProfile;
use harness_core::clock::RealClock;
use harness_core::config::{
    load_config_from_str, refresh_skills_config_registry, registered_skills_config, HarnessConfig,
    PermissionMode, ShellAllowlist, SkillsConfig, ToolFailureMode,
};
use harness_core::coord::{spawn_coordinator, CoordinatorConfig, RunInfo};
use harness_core::perm::PermissionDecision;
use harness_core::redact::DefaultRedactor;
use harness_core::tool::ToolContext;
use harness_tools::coordinator_registry;
use serde_json::json;
use tokio::time::Duration;

mod common;

use common::{
    allow_all_permission_policy, anonymous_supervisor_actor, env_test_lock, repo_root,
    wait_for_question_permission, worker_actor, EnvGuard,
};

struct EnvTestContext {
    previous_cwd: PathBuf,
    previous_home: Option<OsString>,
}

impl EnvTestContext {
    fn new(cwd: &Path, home: &Path) -> Self {
        let previous_cwd = env::current_dir().expect("capture current dir");
        let previous_home = env::var_os("HOME");

        env::set_current_dir(cwd).expect("set test current dir");
        env::set_var("HOME", home);

        Self {
            previous_cwd,
            previous_home,
        }
    }
}

impl Drop for EnvTestContext {
    fn drop(&mut self) {
        env::set_current_dir(&self.previous_cwd).expect("restore current dir");
        match &self.previous_home {
            Some(value) => env::set_var("HOME", value),
            None => env::remove_var("HOME"),
        }
    }
}

fn tool_context(workspace_root: &Path, tool_call_id: &str) -> ToolContext {
    let coordinator = spawn_coordinator(
        CoordinatorConfig::default(),
        Arc::new(RealClock::new()),
        Arc::new(DefaultRedactor::default()),
    );

    ToolContext {
        run_id: "run-skill-load-tests".to_string(),
        workspace_root: workspace_root.to_path_buf(),
        artifacts_dir: workspace_root.join(".artifacts"),
        actor: anonymous_supervisor_actor(),
        category: Some("deep".to_string()),
        tool_call_id: tool_call_id.to_string(),
        current_model_ref: None,
        current_model_settings: None,
        coordinator,
    }
}

struct CurrentDirGuard {
    previous_cwd: PathBuf,
}

impl CurrentDirGuard {
    fn set(cwd: &Path) -> Self {
        let previous_cwd = env::current_dir().expect("capture cwd");
        env::set_current_dir(cwd).expect("set cwd");
        Self { previous_cwd }
    }
}

impl Drop for CurrentDirGuard {
    fn drop(&mut self) {
        env::set_current_dir(&self.previous_cwd).expect("restore cwd");
    }
}

fn write_skill(root: &Path, name: &str, description: &str, body: &str) {
    let skill_dir = root.join(name);
    fs::create_dir_all(&skill_dir).expect("create skill dir");
    fs::write(
        skill_dir.join("SKILL.md"),
        format!("---\nname: {name}\ndescription: {description}\n---\n\n{body}\n"),
    )
    .expect("write skill file");
}

fn write_skill_with_extra_frontmatter(
    root: &Path,
    name: &str,
    description: &str,
    extra_frontmatter: &str,
    body: &str,
) {
    let skill_dir = root.join(name);
    fs::create_dir_all(&skill_dir).expect("create skill dir");
    fs::write(
        skill_dir.join("SKILL.md"),
        format!(
            "---\nname: {name}\ndescription: {description}\n{extra_frontmatter}---\n\n{body}\n"
        ),
    )
    .expect("write skill file");
}

fn write_invalid_skill(root: &Path, name: &str) {
    let skill_dir = root.join(name);
    fs::create_dir_all(&skill_dir).expect("create invalid skill dir");
    fs::write(
        skill_dir.join("SKILL.md"),
        format!(
            "---\nname: {name}\ndescription: Broken precedence target\nmetadata: nope\n---\n\nThis skill should be rejected.\n"
        ),
    )
    .expect("write invalid skill file");
}

fn worker_profile(name: &str, toolset: &[&str]) -> AgentProfile {
    AgentProfile {
        name: name.to_string(),
        category: name.to_string(),
        model_ref: format!("default:{name}"),
        model_ref_explicit: true,
        fallback_model_refs: Vec::new(),
        fallback_model_settings: Vec::new(),
        system_prompt: format!("{name} prompt"),
        temperature: None,
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
    fs::create_dir_all(&session_dir).expect("session dir");

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
        .expect("start run");
    let worker_id = handle
        .spawn_agent(anonymous_supervisor_actor(), profile_name, None)
        .await
        .expect("spawn worker");
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
        "skills": serde_json::to_value(skills).expect("serialize skills config")
    }))
    .expect("config shape should deserialize")
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

#[tokio::test]
#[expect(
    clippy::await_holding_lock,
    reason = "the global env lock intentionally serializes process-wide HOME/cwd mutation across awaits"
)]
async fn skill_load_discovers_project_and_global_roots_with_precedence() {
    let _guard = env_test_lock();
    let temp_dir = tempfile::tempdir().expect("tempdir");
    let home = temp_dir.path().join("home");
    let repo = temp_dir.path().join("repo");
    let app = repo.join("packages/app");
    fs::create_dir_all(&home).expect("home dir");
    fs::create_dir_all(&app).expect("app dir");
    fs::create_dir_all(repo.join(".git")).expect("git dir");
    let _env = EnvTestContext::new(&app, &home);

    write_skill(
        &home.join(".config/agent-harness/skills"),
        "shared-skill",
        "Global shared description",
        "Global body",
    );
    write_skill(
        &repo.join(".agent-harness/skills"),
        "shared-skill",
        "Repo shared description",
        "Repo body",
    );
    write_skill(
        &app.join(".agent-harness/skills"),
        "shared-skill",
        "App shared description",
        "App body",
    );
    write_skill(
        &repo.join(".agent-harness/skills"),
        "repo-only",
        "Repo only description",
        "Repo only body",
    );
    write_skill(
        &home.join(".config/agent-harness/skills"),
        "global-only",
        "Global only description",
        "Global only body",
    );
    write_skill(
        &repo.join(".opencode/skills"),
        "upstream-only",
        "Upstream only description",
        "Upstream only body",
    );
    write_skill(
        &repo.join(".claude/skills"),
        "claude-only",
        "Claude only description",
        "Claude only body",
    );
    write_skill(
        &repo.join(".agents/skills"),
        "agents-only",
        "Agents only description",
        "Agents only body",
    );
    write_skill(
        &home.join(".config/opencode/skills"),
        "global-upstream",
        "Global upstream description",
        "Global upstream body",
    );
    write_skill(
        &home.join(".claude/skills"),
        "global-claude",
        "Global Claude description",
        "Global Claude body",
    );
    write_skill(
        &home.join(".agents/skills"),
        "global-agents",
        "Global Agents description",
        "Global Agents body",
    );

    let registry = coordinator_registry(ShellAllowlist::default());
    let skill_tool = registry.get("skill").expect("skill tool");

    let native_shared = skill_tool
        .call(
            tool_context(&app, "toolcall-native-shared"),
            json!({"name": "shared-skill"}),
        )
        .await
        .expect("native shared skill");
    assert!(native_shared
        .display_text
        .contains("App shared description"));
    assert!(native_shared.display_text.contains("App body"));
    assert!(!native_shared.display_text.contains("Repo body"));
    assert!(!native_shared.display_text.contains("Global body"));
    assert_eq!(
        native_shared
            .structured_json
            .as_ref()
            .and_then(|value| value.get("location")),
        Some(&json!(app
            .join(".agent-harness/skills/shared-skill/SKILL.md")
            .display()
            .to_string()))
    );

    let repo_only = skill_tool
        .call(
            tool_context(&app, "toolcall-repo-only"),
            json!({"name": "repo-only"}),
        )
        .await
        .expect("repo-only skill");
    assert!(repo_only.display_text.contains("Repo only description"));

    let global_only = skill_tool
        .call(
            tool_context(&app, "toolcall-global-only"),
            json!({"name": "global-only"}),
        )
        .await
        .expect("global-only skill");
    assert!(global_only.display_text.contains("Global only description"));

    for (name, description) in [
        ("upstream-only", "Upstream only description"),
        ("claude-only", "Claude only description"),
        ("agents-only", "Agents only description"),
        ("global-upstream", "Global upstream description"),
        ("global-claude", "Global Claude description"),
        ("global-agents", "Global Agents description"),
    ] {
        let skill = skill_tool
            .call(
                tool_context(&app, &format!("toolcall-{name}")),
                json!({ "name": name }),
            )
            .await
            .unwrap_or_else(|err| panic!("{name} should load from compatibility roots: {err}"));
        assert!(
            skill.display_text.contains(description),
            "{name} should include {description}"
        );
    }
}

#[test]
fn skill_discovery_walks_project_and_global_roots() {
    skill_load_discovers_project_and_global_roots_with_precedence();
}

#[tokio::test]
#[expect(
    clippy::await_holding_lock,
    reason = "the global env lock intentionally serializes process-wide HOME/cwd mutation across awaits"
)]
async fn skill_discovery_uses_workspace_root_not_process_cwd() {
    let _guard = env_test_lock();
    let temp_dir = tempfile::tempdir().expect("tempdir");
    let home = temp_dir.path().join("home");
    let repo = temp_dir.path().join("repo");
    let outside = temp_dir.path().join("outside-cwd");
    fs::create_dir_all(&home).expect("home dir");
    fs::create_dir_all(&repo).expect("repo dir");
    fs::create_dir_all(&outside).expect("outside cwd");
    fs::create_dir_all(repo.join(".git")).expect("git dir");
    let _env = EnvTestContext::new(&outside, &home);

    write_skill(
        &repo.join(".agent-harness/skills"),
        "workspace-skill",
        "Workspace description",
        "Workspace body",
    );
    write_skill(
        &outside.join(".agent-harness/skills"),
        "cwd-only",
        "Cwd description",
        "Cwd body",
    );

    let registry = coordinator_registry(ShellAllowlist::default());
    let skill_tool = registry.get("skill").expect("skill tool");
    let workspace_skill = skill_tool
        .call(
            tool_context(&repo, "toolcall-workspace-root-skill"),
            json!({"name": "workspace-skill"}),
        )
        .await
        .expect("workspace skill");
    assert!(workspace_skill
        .display_text
        .contains("Workspace description"));

    let cwd_only = skill_tool
        .call(
            tool_context(&repo, "toolcall-cwd-only-skill"),
            json!({"name": "cwd-only"}),
        )
        .await
        .expect_err("cwd skill should not be loaded from process cwd");
    assert!(cwd_only
        .to_string()
        .contains("Skill \"cwd-only\" not found"));
}

#[cfg(unix)]
#[tokio::test]
#[expect(
    clippy::await_holding_lock,
    reason = "the global env lock intentionally serializes process-wide HOME/cwd mutation across awaits"
)]
async fn skill_discovery_rejects_symlinked_skill_directories() {
    let _guard = env_test_lock();
    let temp_dir = tempfile::tempdir().expect("tempdir");
    let home = temp_dir.path().join("home");
    let repo = temp_dir.path().join("repo");
    let outside = temp_dir.path().join("outside-skill");
    fs::create_dir_all(&home).expect("home dir");
    fs::create_dir_all(&repo).expect("repo dir");
    fs::create_dir_all(repo.join(".git")).expect("git dir");
    let _env = EnvTestContext::new(&repo, &home);

    write_skill(
        temp_dir.path(),
        "outside-skill",
        "Outside description",
        "Outside body",
    );
    let skill_root = repo.join(".agent-harness/skills");
    fs::create_dir_all(&skill_root).expect("skill root");
    std::os::unix::fs::symlink(&outside, skill_root.join("evil")).expect("symlink evil skill dir");

    let registry = coordinator_registry(ShellAllowlist::default());
    let skill_tool = registry.get("skill").expect("skill tool");
    let err = skill_tool
        .call(
            tool_context(&repo, "toolcall-symlink-skill"),
            json!({"name": "evil"}),
        )
        .await
        .expect_err("symlinked skill should be rejected");
    assert!(err.to_string().contains("Skill \"evil\" not found"));
}

#[cfg(unix)]
#[tokio::test]
#[expect(
    clippy::await_holding_lock,
    reason = "the global env lock intentionally serializes process-wide HOME/cwd mutation across awaits"
)]
async fn skill_discovery_rejects_symlinked_project_skill_root() {
    let _guard = env_test_lock();
    let temp_dir = tempfile::tempdir().expect("tempdir");
    let home = temp_dir.path().join("home");
    let repo = temp_dir.path().join("repo");
    let outside = temp_dir.path().join("outside-root");
    fs::create_dir_all(&home).expect("home dir");
    fs::create_dir_all(&repo).expect("repo dir");
    fs::create_dir_all(repo.join(".git")).expect("git dir");
    let _env = EnvTestContext::new(&repo, &home);

    write_skill(
        &outside,
        "evil-root-skill",
        "Outside root description",
        "Outside root body",
    );
    let agent_harness_dir = repo.join(".agent-harness");
    fs::create_dir_all(&agent_harness_dir).expect("agent harness dir");
    std::os::unix::fs::symlink(&outside, agent_harness_dir.join("skills"))
        .expect("symlink project skill root");

    let registry = coordinator_registry(ShellAllowlist::default());
    let skill_tool = registry.get("skill").expect("skill tool");
    let err = skill_tool
        .call(
            tool_context(&repo, "toolcall-symlink-root-skill"),
            json!({"name": "evil-root-skill"}),
        )
        .await
        .expect_err("symlinked project skill root should be ignored");
    assert!(err
        .to_string()
        .contains("Skill \"evil-root-skill\" not found"));
}

#[tokio::test]
#[expect(
    clippy::await_holding_lock,
    reason = "the global env lock intentionally serializes process-wide HOME/cwd mutation across awaits"
)]
async fn skill_load_hides_denied_or_invalid_skills() {
    let _guard = env_test_lock();
    let temp_dir = tempfile::tempdir().expect("tempdir");
    let home = temp_dir.path().join("home");
    let repo = temp_dir.path().join("repo");
    fs::create_dir_all(&home).expect("home dir");
    fs::create_dir_all(&repo).expect("repo dir");
    fs::create_dir_all(repo.join(".git")).expect("git dir");
    let _env = EnvTestContext::new(&repo, &home);

    write_skill(
        &repo.join(".agent-harness/skills"),
        "visible-skill",
        "Visible description",
        "Visible body",
    );
    write_skill(
        &repo.join(".agent-harness/skills"),
        "internal-secret",
        "Denied description",
        "Denied body",
    );
    write_skill(
        &repo.join(".agent-harness/skills"),
        "experimental-preview",
        "Ask description",
        "Ask body",
    );
    write_invalid_skill(&repo.join(".agent-harness/skills"), "broken-skill");
    write_skill(
        &home.join(".config/agent-harness/skills"),
        "broken-skill",
        "Global description",
        "Global body",
    );

    let registry = coordinator_registry(ShellAllowlist::default());
    let skill_tool = registry.get("skill").expect("skill tool");
    let visible = skill_tool
        .call(
            tool_context(&repo, "toolcall-visible-skill"),
            json!({"name": "visible-skill"}),
        )
        .await
        .expect("visible skill");
    assert!(visible.display_text.contains("Visible description"));

    let denied = skill_tool
        .call(
            tool_context(&repo, "toolcall-denied-skill"),
            json!({"name": "internal-secret"}),
        )
        .await
        .expect_err("denied skill should be hidden");
    assert!(denied
        .to_string()
        .contains("Skill \"internal-secret\" not found"));

    let _answers = EnvGuard::set(&[("HARNESS_QUESTION_ANSWERS", Some(r#"[["Yes"]]"#))]);
    let approved = skill_tool
        .call(
            tool_context(&repo, "toolcall-ask-skill"),
            json!({"name": "experimental-preview"}),
        )
        .await
        .expect("ask skill should load after approval");
    assert!(approved.display_text.contains("Ask description"));
    assert!(approved.display_text.contains("Ask body"));

    let invalid = skill_tool
        .call(
            tool_context(&repo, "toolcall-invalid-skill"),
            json!({"name": "broken-skill"}),
        )
        .await
        .expect_err("invalid higher-precedence skill should hide lower-precedence skill");
    assert!(invalid
        .to_string()
        .contains("Skill \"broken-skill\" not found"));
}

#[tokio::test]
#[expect(
    clippy::await_holding_lock,
    reason = "the global env lock intentionally serializes process-wide HOME/cwd mutation across awaits"
)]
async fn compatibility_disabled_skills_are_hidden_from_skill_tool() {
    let _guard = env_test_lock();
    let temp_dir = tempfile::tempdir().expect("tempdir");
    let home = temp_dir.path().join("home");
    let repo = temp_dir.path().join("repo");
    fs::create_dir_all(&home).expect("home dir");
    fs::create_dir_all(&repo).expect("repo dir");
    fs::create_dir_all(repo.join(".git")).expect("git dir");
    let _env = EnvTestContext::new(&repo, &home);

    write_skill(
        &repo.join(".opencode/skills"),
        "legacy-disabled",
        "Disabled compatibility skill",
        "Compatibility body",
    );
    let previous_skills = registered_skills_config();
    let config = load_config_from_str(
        r#"
        {
          provider: {
            default: {
              type: "openai_compatible",
              baseURL: "http://127.0.0.1:1/v1",
              apiKey: "DUMMY",
              models: { "gpt-5.4-mini": { name: "GPT-5.4 Mini" } }
            }
          },
          model: "default:gpt-5.4-mini",
          compatibility: { disabledSkills: ["legacy-disabled"] }
        }
        "#,
    )
    .expect("config parses");
    assert!(config.skills.disabled_skills.contains("legacy-disabled"));
    let _skills_guard = SkillsConfigGuard {
        previous: previous_skills,
    };
    refresh_skills_config_registry(&harness_config_with_skills(config.skills.clone()));

    let registry = coordinator_registry(ShellAllowlist::default());
    let skill_tool = registry.get("skill").expect("skill tool");
    let err = skill_tool
        .call(
            tool_context(&repo, "toolcall-compat-disabled-skill"),
            json!({"name": "legacy-disabled"}),
        )
        .await
        .expect_err("compatibility-disabled skill should not load");
    assert!(err
        .to_string()
        .contains("Skill \"legacy-disabled\" not found"));
}

#[test]
fn skill_permissions_hide_denied_and_reject_invalid_frontmatter() {
    skill_load_hides_denied_or_invalid_skills();
}

#[tokio::test]
#[expect(
    clippy::await_holding_lock,
    reason = "the global env lock intentionally serializes process-wide HOME/cwd mutation across awaits"
)]
async fn skill_list_reports_status_shadowing_and_frontmatter_policy() {
    let _guard = env_test_lock();
    let temp_dir = tempfile::tempdir().expect("tempdir");
    let home = temp_dir.path().join("home");
    let repo = temp_dir.path().join("repo");
    fs::create_dir_all(&home).expect("home dir");
    fs::create_dir_all(&repo).expect("repo dir");
    fs::create_dir_all(repo.join(".git")).expect("git dir");
    let _env = EnvTestContext::new(&repo, &home);

    write_skill_with_extra_frontmatter(
        &repo.join(".agent-harness/skills"),
        "browser-skill",
        "Browser skill description",
        "tools: [read, webfetch]\ncommands:\n  - npm test\npermissions:\n  webfetch: allow\nenvironment:\n  allow:\n    - PLAYWRIGHT_*\nmcp:\n  playwright:\n    command: playwright-mcp\n",
        "Browser body",
    );
    write_skill(
        &home.join(".config/agent-harness/skills"),
        "browser-skill",
        "Shadowed browser description",
        "Shadowed body",
    );
    write_skill(
        &repo.join(".agent-harness/skills"),
        "internal-secret",
        "Denied description",
        "Denied body",
    );
    write_invalid_skill(&repo.join(".agent-harness/skills"), "broken-skill");

    let registry = coordinator_registry(ShellAllowlist::default());
    let skill_tool = registry.get("skill").expect("skill tool");
    let list = skill_tool
        .call(
            tool_context(&repo, "toolcall-skill-list"),
            json!({"list": true}),
        )
        .await
        .expect("skill list");
    let output = list.structured_json.expect("skill list json");
    assert_eq!(output["visible"], json!(["browser-skill"]));
    assert_eq!(output["denied"], json!(["internal-secret"]));
    assert_eq!(output["invalid"], json!(["broken-skill"]));
    assert_eq!(output["shadowed"][0]["name"], json!("browser-skill"));

    let browser = output["skills"]
        .as_array()
        .expect("skills array")
        .iter()
        .find(|skill| skill["name"] == json!("browser-skill"))
        .expect("browser skill listing");
    assert_eq!(browser["status"], json!("visible"));
    assert_eq!(browser["policy"]["tools"], json!(["read", "webfetch"]));
    assert_eq!(browser["policy"]["commands"], json!(["npm test"]));
    assert_eq!(browser["policy"]["permissions"]["webfetch"], json!("allow"));
    assert_eq!(
        browser["policy"]["environment"]["allow"],
        json!(["PLAYWRIGHT_*"])
    );
    assert_eq!(browser["policy"]["mcp"], json!(["playwright"]));

    let loaded = skill_tool
        .call(
            tool_context(&repo, "toolcall-skill-load-with-policy"),
            json!({"name": "browser-skill"}),
        )
        .await
        .expect("load browser skill");
    assert!(loaded
        .display_text
        .contains("<skill_policy name=\"browser-skill\">"));
    assert_eq!(
        loaded.structured_json.expect("load json")["policy"]["tools"],
        json!(["read", "webfetch"])
    );
}

#[tokio::test]
#[expect(
    clippy::await_holding_lock,
    reason = "the global env lock intentionally serializes process-wide HOME/cwd mutation across awaits"
)]
async fn skills_config_can_disable_all_or_individual_skills() {
    let _guard = env_test_lock();
    let temp_dir = tempfile::tempdir().expect("tempdir");
    let home = temp_dir.path().join("home");
    let repo = temp_dir.path().join("repo");
    fs::create_dir_all(&home).expect("home dir");
    fs::create_dir_all(&repo).expect("repo dir");
    fs::create_dir_all(repo.join(".git")).expect("git dir");
    let _env = EnvTestContext::new(&repo, &home);

    write_skill(
        &repo.join(".agent-harness/skills"),
        "visible-skill",
        "Visible description",
        "Visible body",
    );
    write_skill(
        &repo.join(".agent-harness/skills"),
        "disabled-skill",
        "Disabled description",
        "Disabled body",
    );
    let registry = coordinator_registry(ShellAllowlist::default());
    let skill_tool = registry.get("skill").expect("skill tool");

    let skills_guard = SkillsConfigGuard::install(SkillsConfig {
        disabled_skills: BTreeSet::from(["disabled-skill".to_string()]),
        ..SkillsConfig::default()
    });
    let list = skill_tool
        .call(
            tool_context(&repo, "toolcall-skill-disable-list"),
            json!({}),
        )
        .await
        .expect("skill list");
    let output = list.structured_json.expect("skill list json");
    assert_eq!(output["visible"], json!(["visible-skill"]));
    assert_eq!(output["denied"], json!(["disabled-skill"]));
    let err = skill_tool
        .call(
            tool_context(&repo, "toolcall-disabled-skill"),
            json!({"name": "disabled-skill"}),
        )
        .await
        .expect_err("disabled skill should not load");
    assert!(err
        .to_string()
        .contains("Skill \"disabled-skill\" not found"));
    drop(skills_guard);

    let _skills_guard = SkillsConfigGuard::install(SkillsConfig {
        disabled: true,
        ..SkillsConfig::default()
    });
    let list = skill_tool
        .call(tool_context(&repo, "toolcall-skills-disabled"), json!({}))
        .await
        .expect("disabled skill list");
    let output = list.structured_json.expect("disabled skill list json");
    assert_eq!(output["disabled"], json!(true));
    assert_eq!(output["visible"], json!([]));
}

#[tokio::test]
#[expect(
    clippy::await_holding_lock,
    reason = "the global env lock intentionally serializes process-wide HOME/cwd mutation across awaits"
)]
async fn skill_mcp_records_run_scoped_state_without_exposing_env_values() {
    let _guard = env_test_lock();
    let temp_dir = tempfile::tempdir().expect("tempdir");
    let home = temp_dir.path().join("home");
    let repo = temp_dir.path().join("repo");
    fs::create_dir_all(&home).expect("home dir");
    fs::create_dir_all(&repo).expect("repo dir");
    fs::create_dir_all(repo.join(".git")).expect("git dir");
    let _env = EnvTestContext::new(&repo, &home);

    write_skill_with_extra_frontmatter(
        &repo.join(".agent-harness/skills"),
        "mcp-skill",
        "MCP skill description",
        "mcp:\n  docs:\n    command: docs-mcp\n    args:\n      - serve\n    env:\n      DOCS_TOKEN: super-secret-token\n",
        "MCP body",
    );

    let registry = coordinator_registry(ShellAllowlist::default());
    let skill_mcp_tool = registry.get("skill_mcp").expect("skill_mcp tool");
    let status = skill_mcp_tool
        .call(
            tool_context(&repo, "toolcall-skill-mcp-status"),
            json!({"skill": "mcp-skill", "action": "status"}),
        )
        .await
        .expect("skill_mcp status");
    let output = status.structured_json.expect("skill_mcp status json");
    assert_eq!(output["servers"][0]["server"], json!("docs"));
    assert_eq!(output["servers"][0]["status"], json!("declared"));
    assert_eq!(
        output["servers"][0]["command"],
        json!(["docs-mcp", "serve"])
    );
    assert_eq!(output["servers"][0]["env_keys"], json!(["DOCS_TOKEN"]));
    assert_eq!(output["servers"][0]["env_values_redacted"], json!(true));
    assert!(!serde_json::to_string(&output)
        .expect("json")
        .contains("super-secret-token"));

    let started = skill_mcp_tool
        .call(
            tool_context(&repo, "toolcall-skill-mcp-start"),
            json!({"skill": "mcp-skill", "server": "docs", "action": "start"}),
        )
        .await
        .expect("skill_mcp start");
    assert_eq!(
        started.structured_json.expect("start json")["servers"][0]["status"],
        json!("started")
    );

    let stopped = skill_mcp_tool
        .call(
            tool_context(&repo, "toolcall-skill-mcp-stop"),
            json!({"skill": "mcp-skill", "server": "docs", "action": "stop"}),
        )
        .await
        .expect("skill_mcp stop");
    assert_eq!(
        stopped.structured_json.expect("stop json")["servers"][0]["status"],
        json!("stopped")
    );
}

#[tokio::test]
#[expect(
    clippy::await_holding_lock,
    reason = "the global env lock intentionally serializes process-wide HOME/cwd mutation across awaits"
)]
async fn skill_mcp_accepts_yaml_block_list_declarations() {
    let _guard = env_test_lock();
    let temp_dir = tempfile::tempdir().expect("tempdir");
    let home = temp_dir.path().join("home");
    let repo = temp_dir.path().join("repo");
    fs::create_dir_all(&home).expect("home dir");
    fs::create_dir_all(&repo).expect("repo dir");
    fs::create_dir_all(repo.join(".git")).expect("git dir");
    let _env = EnvTestContext::new(&repo, &home);

    write_skill_with_extra_frontmatter(
        &repo.join(".agent-harness/skills"),
        "block-mcp-skill",
        "Block MCP skill description",
        "mcp:\n  - docs\n",
        "MCP body",
    );

    let registry = coordinator_registry(ShellAllowlist::default());
    let skill_mcp_tool = registry.get("skill_mcp").expect("skill_mcp tool");
    let status = skill_mcp_tool
        .call(
            tool_context(&repo, "toolcall-skill-mcp-block-list"),
            json!({"skill": "block-mcp-skill", "action": "status"}),
        )
        .await
        .expect("skill_mcp status");
    let output = status.structured_json.expect("skill_mcp status json");
    assert_eq!(output["servers"][0]["server"], json!("docs"));
    assert_eq!(output["servers"][0]["status"], json!("declared"));
    assert_eq!(output["servers"][0]["transport"], json!("declared"));
}

#[tokio::test]
#[expect(
    clippy::await_holding_lock,
    reason = "the global env lock intentionally serializes process-wide HOME/cwd mutation across awaits"
)]
async fn shipped_starter_skill_pack_is_discoverable_from_repo_checkout() {
    let _guard = env_test_lock();
    let repo = repo_root();
    let _cwd = CurrentDirGuard::set(&repo);
    let registry = coordinator_registry(ShellAllowlist::default());
    let skill_tool = registry.get("skill").expect("skill tool");
    let skill = skill_tool
        .call(
            tool_context(&repo, "toolcall-shipped-rust-best-practices"),
            json!({"name": "rust-best-practices"}),
        )
        .await
        .expect("shipped rust-best-practices skill");
    assert!(skill.display_text.contains("# Skill: rust-best-practices"));
    assert!(skill.display_text.contains("cargo fmt --all -- --check"));
    assert_eq!(
        skill
            .structured_json
            .as_ref()
            .and_then(|value| value.get("location")),
        Some(&json!(repo
            .join(".agent-harness/skills/rust-best-practices/SKILL.md")
            .display()
            .to_string()))
    );
}

#[tokio::test]
#[expect(
    clippy::await_holding_lock,
    reason = "the global env lock intentionally serializes process-wide HOME/cwd mutation across awaits"
)]
async fn harness_skill_pack_is_discoverable_from_repo_checkout() {
    let _guard = env_test_lock();
    let repo = repo_root();
    let _cwd = CurrentDirGuard::set(&repo);
    let registry = coordinator_registry(ShellAllowlist::default());
    let skill_tool = registry.get("skill").expect("skill tool");
    let skill = skill_tool
        .call(
            tool_context(&repo, "toolcall-shipped-analyze"),
            json!({"name": "rust-best-practices"}),
        )
        .await
        .expect("shipped rust best practices skill");
    assert!(skill.display_text.contains("# Rust best practices"));
    assert!(skill.display_text.contains("harness workspace"));
    assert_eq!(
        skill
            .structured_json
            .as_ref()
            .and_then(|value| value.get("location")),
        Some(&json!(repo
            .join(".agent-harness/skills/rust-best-practices/SKILL.md")
            .display()
            .to_string()))
    );

    for (name, expected) in [
        ("git-master", "# Git master"),
        ("frontend-ui-ux", "# Frontend UI UX"),
        ("review-work", "# Review work"),
        ("playwright", "# Playwright browser automation"),
        ("agent-browser", "# Agent browser"),
        ("dev-browser", "# Dev browser"),
        ("ai-slop-remover", "# AI slop remover"),
        ("team-mode", "# Team mode"),
    ] {
        let skill = skill_tool
            .call(
                tool_context(&repo, &format!("toolcall-shipped-{name}")),
                json!({ "name": name }),
            )
            .await
            .unwrap_or_else(|err| panic!("shipped {name} skill should load: {err}"));
        assert!(skill.display_text.contains(expected));
        assert_eq!(
            skill
                .structured_json
                .as_ref()
                .and_then(|value| value.get("location")),
            Some(&json!(repo
                .join(format!(".agent-harness/skills/{name}/SKILL.md"))
                .display()
                .to_string()))
        );
    }
}

#[tokio::test]
#[expect(
    clippy::await_holding_lock,
    reason = "the global env lock intentionally serializes process-wide HOME/cwd mutation across awaits"
)]
async fn skill_load_reports_agent_hint_for_build() {
    let _guard = env_test_lock();
    let repo = repo_root();
    let _cwd = CurrentDirGuard::set(&repo);
    let registry = coordinator_registry(ShellAllowlist::default());
    let skill_tool = registry.get("skill").expect("skill tool");

    let err = skill_tool
        .call(
            tool_context(&repo, "toolcall-build-agent-hint"),
            json!({"name": "build"}),
        )
        .await
        .expect_err("build is an agent, not a skill");

    let message = err.to_string();
    assert!(message.contains("Skill \"build\" not found"));
    assert!(message.contains("`build` is an agent, not a skill"));
    assert!(message.contains("task"));
}

#[tokio::test]
#[expect(
    clippy::await_holding_lock,
    reason = "the global env lock intentionally serializes process-wide HOME/cwd mutation across awaits"
)]
async fn skill_load_uses_registered_custom_roots_and_permission_precedence() {
    let _guard = env_test_lock();
    let temp_dir = tempfile::tempdir().expect("tempdir");
    let home = temp_dir.path().join("home");
    let repo = temp_dir.path().join("repo");
    let app = repo.join("packages/app");
    fs::create_dir_all(&home).expect("home dir");
    fs::create_dir_all(&app).expect("app dir");
    fs::create_dir_all(repo.join(".git")).expect("git dir");
    let _env = EnvTestContext::new(&app, &home);
    let _skills_guard = SkillsConfigGuard::install(SkillsConfig {
        disabled: false,
        project_roots: vec![PathBuf::from(".custom/skills")],
        global_roots: vec![PathBuf::from("~/.company/skills")],
        urls: Vec::new(),
        walk_to_git_root: false,
        disabled_skills: BTreeSet::new(),
        permissions: std::collections::BTreeMap::from([
            ("*".to_string(), PermissionMode::Allow),
            ("team-*".to_string(), PermissionMode::Allow),
            ("team-secret".to_string(), PermissionMode::Ask),
        ]),
    });

    write_skill(
        &app.join(".custom/skills"),
        "team-visible",
        "Team visible description",
        "Team visible body",
    );
    write_skill(
        &app.join(".custom/skills"),
        "team-secret",
        "Team secret description",
        "Team secret body",
    );
    write_skill(
        &repo.join(".custom/skills"),
        "team-repo",
        "Repo-only description",
        "Repo-only body",
    );
    write_skill(
        &home.join(".company/skills"),
        "global-visible",
        "Global visible description",
        "Global visible body",
    );

    let registry = coordinator_registry(ShellAllowlist::default());
    let skill_tool = registry.get("skill").expect("skill tool");

    let visible = skill_tool
        .call(
            tool_context(&app, "toolcall-custom-visible"),
            json!({"name": "team-visible"}),
        )
        .await
        .expect("team-visible skill");
    assert!(visible.display_text.contains("Team visible description"));
    assert_eq!(
        visible
            .structured_json
            .as_ref()
            .and_then(|value| value.get("location")),
        Some(&json!(app
            .join(".custom/skills/team-visible/SKILL.md")
            .display()
            .to_string()))
    );

    let _answers = EnvGuard::set(&[("HARNESS_QUESTION_ANSWERS", Some(r#"[["Yes"]]"#))]);
    let gated = skill_tool
        .call(
            tool_context(&app, "toolcall-custom-ask"),
            json!({"name": "team-secret"}),
        )
        .await
        .expect("exact permission override should load after approval");
    assert!(gated.display_text.contains("Team secret description"));

    let repo_hidden = skill_tool
        .call(
            tool_context(&app, "toolcall-custom-repo-hidden"),
            json!({"name": "team-repo"}),
        )
        .await
        .expect_err("walk_to_git_root=false should skip repo-root skills from app cwd");
    assert!(repo_hidden
        .to_string()
        .contains("Skill \"team-repo\" not found"));

    let global_visible = skill_tool
        .call(
            tool_context(&app, "toolcall-custom-global"),
            json!({"name": "global-visible"}),
        )
        .await
        .expect("global-visible skill");
    assert!(global_visible
        .display_text
        .contains("Global visible description"));
}

#[tokio::test]
#[expect(
    clippy::await_holding_lock,
    reason = "the global env lock intentionally serializes process-wide HOME/cwd mutation across awaits"
)]
async fn skill_load_ask_permissions_use_question_approval_flow() {
    let _guard = env_test_lock();
    let temp_dir = tempfile::tempdir().expect("tempdir");
    let home = temp_dir.path().join("home");
    let repo = temp_dir.path().join("repo");
    fs::create_dir_all(&home).expect("home dir");
    fs::create_dir_all(&repo).expect("repo dir");
    fs::create_dir_all(repo.join(".git")).expect("git dir");
    let _env = EnvTestContext::new(&repo, &home);
    let _skills_guard = SkillsConfigGuard::install(SkillsConfig {
        permissions: BTreeMap::from([
            ("*".to_string(), PermissionMode::Allow),
            ("experimental-*".to_string(), PermissionMode::Ask),
        ]),
        ..SkillsConfig::default()
    });

    write_skill(
        &repo.join(".agent-harness/skills"),
        "experimental-preview",
        "Ask description",
        "Ask body",
    );

    let agent_profiles = BTreeMap::from([("deep".to_string(), worker_profile("deep", &["skill"]))]);
    let (handle, run, worker_id) = spawn_worker_run(&repo, "deep", agent_profiles).await;

    let skill_task = {
        let handle = handle.clone();
        let worker_id = worker_id.clone();
        tokio::spawn(async move {
            handle
                .execute_agent_tool_call(
                    worker_actor(&worker_id),
                    Some("deep".to_string()),
                    "skill",
                    json!({"name": "experimental-preview"}),
                )
                .await
        })
    };
    let permission_id =
        wait_for_question_permission(&run.events_path, None, Duration::from_secs(5)).await;
    handle
        .resolve_permission(
            permission_id,
            PermissionDecision::Allow,
            Some(r#"[["Yes"]]"#.to_string()),
        )
        .await
        .expect("approve skill tool");
    let skill = skill_task
        .await
        .expect("join skill tool")
        .expect("skill tool result");

    assert!(skill.display_text.contains("Ask description"));
    assert!(skill.display_text.contains("Ask body"));
    assert!(skill.display_text.contains("# Skill: experimental-preview"));

    handle.stop_run().await.expect("stop run");
}
