use std::collections::BTreeMap;
use std::env;
use std::ffi::OsString;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use harness_core::agent::AgentProfile;
use harness_core::clock::RealClock;
use harness_core::config::{
    refresh_skills_config_registry, registered_skills_config, HarnessConfig, PermissionMode,
    ShellAllowlist, SkillsConfig, ToolFailureMode,
};
use harness_core::coord::{spawn_coordinator, CoordinatorConfig, PlanProfileConfig, RunInfo};
use harness_core::event::{ActorKind, EventActor, EventEnvelopeV1, EventV1};
use harness_core::perm::{PermissionDecision, PermissionPolicy};
use harness_core::redact::DefaultRedactor;
use harness_core::tool::{ToolContext, ToolSurface};
use harness_tools::coordinator_registry;
use serde_json::json;
use tokio::time::{sleep, Duration, Instant};

static SKILL_DISCOVERY_ENV_LOCK: Mutex<()> = Mutex::new(());

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

struct ScopedEnvVar {
    key: &'static str,
    previous: Option<OsString>,
}

impl ScopedEnvVar {
    fn set(key: &'static str, value: &str) -> Self {
        let previous = env::var_os(key);
        env::set_var(key, value);
        Self { key, previous }
    }
}

impl Drop for ScopedEnvVar {
    fn drop(&mut self) {
        match &self.previous {
            Some(value) => env::set_var(self.key, value),
            None => env::remove_var(self.key),
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
        actor: EventActor::new(ActorKind::Supervisor, None),
        category: Some("deep".to_string()),
        plan_mode: false,
        plan_exit_target_profile: None,
        tool_call_id: tool_call_id.to_string(),
        coordinator,
    }
}

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .canonicalize()
        .expect("canonical repo root")
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

fn actor(agent_id: &str) -> EventActor {
    EventActor::new(ActorKind::Worker, Some(agent_id.to_string()))
}

fn worker_profile(name: &str, toolset: &[&str]) -> AgentProfile {
    AgentProfile {
        name: name.to_string(),
        category: name.to_string(),
        model_ref: format!("default:{name}"),
        system_prompt: format!("{name} prompt"),
        tool_failure_mode: ToolFailureMode::FailTurn,
        tool_surface: ToolSurface::Native,
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
    config.permission_policy = PermissionPolicy::new(
        PermissionMode::Allow,
        PermissionMode::Allow,
        PermissionMode::Allow,
    );
    config.tool_registry = Arc::new(coordinator_registry(ShellAllowlist::default()));
    config.agent_profiles = agent_profiles;
    config.plan_profiles =
        BTreeMap::from([(profile_name.to_string(), PlanProfileConfig::default())]);

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
        .spawn_agent(
            EventActor::new(ActorKind::Supervisor, None),
            profile_name,
            None,
        )
        .await
        .expect("spawn worker");
    (handle, run, worker_id)
}

fn read_events(path: &Path) -> Vec<EventEnvelopeV1> {
    fs::read_to_string(path)
        .expect("read events")
        .lines()
        .map(|line| serde_json::from_str(line).expect("parse event"))
        .collect()
}

async fn wait_for_question_permission(path: &Path, previous: Option<&str>) -> String {
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        if let Some(permission_id) =
            read_events(path)
                .into_iter()
                .rev()
                .find_map(|event| match event.payload {
                    EventV1::PermissionRequested(data)
                        if data.kind == "question"
                            && previous.is_none_or(|value| value != data.permission_id) =>
                    {
                        Some(data.permission_id)
                    }
                    _ => None,
                })
        {
            return permission_id;
        }

        assert!(
            Instant::now() < deadline,
            "timed out waiting for question permission"
        );
        sleep(Duration::from_millis(20)).await;
    }
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
        "profiles": {
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
    let _guard = SKILL_DISCOVERY_ENV_LOCK.lock().expect("env test lock");
    let temp_dir = tempfile::tempdir().expect("tempdir");
    let home = temp_dir.path().join("home");
    let repo = temp_dir.path().join("repo");
    let app = repo.join("packages/app");
    fs::create_dir_all(&home).expect("home dir");
    fs::create_dir_all(&app).expect("app dir");
    fs::create_dir_all(repo.join(".git")).expect("git dir");
    let _env = EnvTestContext::new(&app, &home);

    write_skill(
        &home.join(".config/opencode/skills"),
        "shared-skill",
        "Global shared description",
        "Global body",
    );
    write_skill(
        &repo.join(".opencode/skills"),
        "shared-skill",
        "Repo shared description",
        "Repo body",
    );
    write_skill(
        &app.join(".opencode/skills"),
        "shared-skill",
        "App shared description",
        "App body",
    );
    write_skill(
        &repo.join(".agents/skills"),
        "repo-only",
        "Repo only description",
        "Repo only body",
    );
    write_skill(
        &home.join(".agents/skills"),
        "global-only",
        "Global only description",
        "Global only body",
    );

    let registry = coordinator_registry(ShellAllowlist::default());
    let native = registry.get("skill.load").expect("skill.load tool");
    let compat = registry.get("skill").expect("skill tool");

    let native_shared = native
        .call(
            tool_context(&repo, "toolcall-native-shared"),
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
            .join(".opencode/skills/shared-skill/SKILL.md")
            .display()
            .to_string()))
    );

    let compat_shared = compat
        .call(
            tool_context(&repo, "toolcall-compat-shared"),
            json!({"name": "shared-skill"}),
        )
        .await
        .expect("compat shared skill");
    assert_eq!(native_shared.display_text, compat_shared.display_text);
    assert_eq!(native_shared.structured_json, compat_shared.structured_json);

    let repo_only = native
        .call(
            tool_context(&repo, "toolcall-repo-only"),
            json!({"name": "repo-only"}),
        )
        .await
        .expect("repo-only skill");
    assert!(repo_only.display_text.contains("Repo only description"));

    let global_only = native
        .call(
            tool_context(&repo, "toolcall-global-only"),
            json!({"name": "global-only"}),
        )
        .await
        .expect("global-only skill");
    assert!(global_only.display_text.contains("Global only description"));
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
async fn skill_load_hides_denied_or_invalid_skills() {
    let _guard = SKILL_DISCOVERY_ENV_LOCK.lock().expect("env test lock");
    let temp_dir = tempfile::tempdir().expect("tempdir");
    let home = temp_dir.path().join("home");
    let repo = temp_dir.path().join("repo");
    fs::create_dir_all(&home).expect("home dir");
    fs::create_dir_all(&repo).expect("repo dir");
    fs::create_dir_all(repo.join(".git")).expect("git dir");
    let _env = EnvTestContext::new(&repo, &home);

    write_skill(
        &repo.join(".opencode/skills"),
        "visible-skill",
        "Visible description",
        "Visible body",
    );
    write_skill(
        &repo.join(".opencode/skills"),
        "internal-secret",
        "Denied description",
        "Denied body",
    );
    write_skill(
        &repo.join(".opencode/skills"),
        "experimental-preview",
        "Ask description",
        "Ask body",
    );
    write_invalid_skill(&repo.join(".opencode/skills"), "broken-skill");
    write_skill(
        &home.join(".config/opencode/skills"),
        "broken-skill",
        "Global fallback description",
        "Global fallback body",
    );

    let registry = coordinator_registry(ShellAllowlist::default());
    let native = registry.get("skill.load").expect("skill.load tool");
    let compat = registry.get("skill").expect("skill tool");

    let visible = native
        .call(
            tool_context(&repo, "toolcall-visible-skill"),
            json!({"name": "visible-skill"}),
        )
        .await
        .expect("visible skill");
    assert!(visible.display_text.contains("Visible description"));

    let denied = native
        .call(
            tool_context(&repo, "toolcall-denied-skill"),
            json!({"name": "internal-secret"}),
        )
        .await
        .expect_err("denied skill should be hidden");
    assert!(denied
        .to_string()
        .contains("Skill \"internal-secret\" not found"));

    let compat_denied = compat
        .call(
            tool_context(&repo, "toolcall-compat-denied-skill"),
            json!({"name": "internal-secret"}),
        )
        .await
        .expect_err("compat denied skill should be hidden");
    assert_eq!(denied.to_string(), compat_denied.to_string());

    let _answers = ScopedEnvVar::set("HARNESS_QUESTION_ANSWERS", r#"[["Yes"]]"#);
    let approved = native
        .call(
            tool_context(&repo, "toolcall-ask-skill"),
            json!({"name": "experimental-preview"}),
        )
        .await
        .expect("ask skill should load after approval");
    assert!(approved.display_text.contains("Ask description"));
    assert!(approved.display_text.contains("Ask body"));

    let invalid = native
        .call(
            tool_context(&repo, "toolcall-invalid-skill"),
            json!({"name": "broken-skill"}),
        )
        .await
        .expect_err("invalid higher-precedence skill should hide fallback");
    assert!(invalid
        .to_string()
        .contains("Skill \"broken-skill\" not found"));
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
async fn shipped_starter_skill_pack_is_discoverable_from_repo_checkout() {
    let _guard = SKILL_DISCOVERY_ENV_LOCK.lock().expect("env test lock");
    let repo = repo_root();
    let _cwd = CurrentDirGuard::set(&repo);
    let registry = coordinator_registry(ShellAllowlist::default());
    let native = registry.get("skill.load").expect("skill.load tool");
    let skill = native
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
            .join(".agents/skills/rust-best-practices/SKILL.md")
            .display()
            .to_string()))
    );
}

#[tokio::test]
#[expect(
    clippy::await_holding_lock,
    reason = "the global env lock intentionally serializes process-wide HOME/cwd mutation across awaits"
)]
async fn skill_load_uses_registered_custom_roots_and_permission_precedence() {
    let _guard = SKILL_DISCOVERY_ENV_LOCK.lock().expect("env test lock");
    let temp_dir = tempfile::tempdir().expect("tempdir");
    let home = temp_dir.path().join("home");
    let repo = temp_dir.path().join("repo");
    let app = repo.join("packages/app");
    fs::create_dir_all(&home).expect("home dir");
    fs::create_dir_all(&app).expect("app dir");
    fs::create_dir_all(repo.join(".git")).expect("git dir");
    let _env = EnvTestContext::new(&app, &home);
    let _skills_guard = SkillsConfigGuard::install(SkillsConfig {
        project_roots: vec![PathBuf::from(".custom/skills")],
        global_roots: vec![PathBuf::from("~/.company/skills")],
        walk_to_git_root: false,
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
    let native = registry.get("skill.load").expect("skill.load tool");

    let visible = native
        .call(
            tool_context(&repo, "toolcall-custom-visible"),
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

    let _answers = ScopedEnvVar::set("HARNESS_QUESTION_ANSWERS", r#"[["Yes"]]"#);
    let gated = native
        .call(
            tool_context(&repo, "toolcall-custom-ask"),
            json!({"name": "team-secret"}),
        )
        .await
        .expect("exact permission override should load after approval");
    assert!(gated.display_text.contains("Team secret description"));

    let repo_hidden = native
        .call(
            tool_context(&repo, "toolcall-custom-repo-hidden"),
            json!({"name": "team-repo"}),
        )
        .await
        .expect_err("walk_to_git_root=false should skip repo-root skills from app cwd");
    assert!(repo_hidden
        .to_string()
        .contains("Skill \"team-repo\" not found"));

    let global_visible = native
        .call(
            tool_context(&repo, "toolcall-custom-global"),
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
async fn skill_load_ask_permissions_use_question_approval_flow_for_native_and_compat() {
    let _guard = SKILL_DISCOVERY_ENV_LOCK.lock().expect("env test lock");
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
        &repo.join(".opencode/skills"),
        "experimental-preview",
        "Ask description",
        "Ask body",
    );

    let toolset = ["skill.load", "skill"];
    let agent_profiles = BTreeMap::from([("deep".to_string(), worker_profile("deep", &toolset))]);
    let (handle, run, worker_id) = spawn_worker_run(&repo, "deep", agent_profiles).await;

    let native_task = {
        let handle = handle.clone();
        let worker_id = worker_id.clone();
        tokio::spawn(async move {
            handle
                .execute_agent_tool_call(
                    actor(&worker_id),
                    Some("deep".to_string()),
                    "skill.load",
                    json!({"name": "experimental-preview"}),
                )
                .await
        })
    };
    let native_permission_id = wait_for_question_permission(&run.events_path, None).await;
    handle
        .resolve_permission(
            native_permission_id.clone(),
            PermissionDecision::Allow,
            Some(r#"[["Yes"]]"#.to_string()),
        )
        .await
        .expect("approve native skill.load");
    let native = native_task
        .await
        .expect("join native skill.load")
        .expect("native skill.load result");

    let compat_task = {
        let handle = handle.clone();
        let worker_id = worker_id.clone();
        tokio::spawn(async move {
            handle
                .execute_agent_tool_call(
                    actor(&worker_id),
                    Some("deep".to_string()),
                    "skill",
                    json!({"name": "experimental-preview"}),
                )
                .await
        })
    };
    let compat_permission_id =
        wait_for_question_permission(&run.events_path, Some(&native_permission_id)).await;
    handle
        .resolve_permission(
            compat_permission_id,
            PermissionDecision::Allow,
            Some(r#"[["Yes"]]"#.to_string()),
        )
        .await
        .expect("approve compat skill");
    let compat = compat_task
        .await
        .expect("join compat skill")
        .expect("compat skill result");

    assert_eq!(native.display_text, compat.display_text);
    assert_eq!(native.structured_json, compat.structured_json);
    assert!(native.display_text.contains("Ask description"));
    assert!(native.display_text.contains("Ask body"));
    assert!(native
        .display_text
        .contains("# Skill: experimental-preview"));

    handle.stop_run().await.expect("stop run");
}
