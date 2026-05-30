use std::collections::BTreeMap;
use std::fs;
use std::path::Path;
use std::sync::{Arc, OnceLock};

use async_trait::async_trait;
#[path = "mod.rs"]
mod common;

use common::{
    anonymous_supervisor_actor, find_finished, install_fake_mcp_server, read_events, repo_root,
    setup_workspace, wait_for_request_terminal, wait_for_tool_call_finish, worker_actor,
};
use harness_core::agent::{AgentModelSettings, AgentProfile};
use harness_core::clock::RealClock;
use harness_core::config::{
    refresh_skills_config_registry, registered_skills_config, CategoryPermissions, HarnessConfig,
    McpConfig, McpServerConfig, PermissionMode, ShellAllowlist, SkillsConfig,
};
use harness_core::coord::{
    spawn_coordinator, CoordinatorConfig, CoordinatorError, CoordinatorHandle, RunInfo,
};
use harness_core::event::{
    EventV1, PermissionDecision as EventPermissionDecision, TaskTerminalScope, ToolCallStatus,
};
use harness_core::perm::PermissionPolicy;
use harness_core::redact::DefaultRedactor;
use harness_providers::{
    CompletionRequest, CompletionUsage, Provider, ProviderEventStream, ProviderStreamEvent,
};
use harness_tools::{
    coordinator_registry, coordinator_registry_with_mcp, coordinator_registry_with_question_answers,
};
use serde_json::{json, Value};
use tokio::sync::Mutex;
use tokio_stream::StreamExt as _;

#[derive(Debug)]
struct StaticProvider;

#[async_trait]
impl Provider for StaticProvider {
    async fn stream_completion(&self, _req: CompletionRequest) -> ProviderEventStream {
        Box::pin(tokio_stream::iter(vec![
            ProviderStreamEvent::Start,
            ProviderStreamEvent::TextDelta("static child result".to_string()),
            ProviderStreamEvent::Done {
                usage: CompletionUsage {
                    prompt_tokens: 1,
                    completion_tokens: 1,
                    total_tokens: 2,
                },
            },
        ]))
    }
}

#[derive(Debug)]
struct BlockingProvider;

#[async_trait]
impl Provider for BlockingProvider {
    async fn stream_completion(&self, _req: CompletionRequest) -> ProviderEventStream {
        Box::pin(
            tokio_stream::iter(vec![ProviderStreamEvent::Start])
                .chain(tokio_stream::pending::<ProviderStreamEvent>()),
        )
    }
}

#[derive(Debug)]
struct DelayedProvider;

#[async_trait]
impl Provider for DelayedProvider {
    async fn stream_completion(&self, _req: CompletionRequest) -> ProviderEventStream {
        tokio::task::yield_now().await;
        Box::pin(tokio_stream::iter(vec![
            ProviderStreamEvent::Start,
            ProviderStreamEvent::TextDelta("delayed child result".to_string()),
            ProviderStreamEvent::Done {
                usage: CompletionUsage {
                    prompt_tokens: 1,
                    completion_tokens: 1,
                    total_tokens: 2,
                },
            },
        ]))
    }
}

#[derive(Debug)]
struct ChildToolThenFinalProvider {
    requests: Mutex<Vec<CompletionRequest>>,
}

impl ChildToolThenFinalProvider {
    fn new() -> Self {
        Self {
            requests: Mutex::new(Vec::new()),
        }
    }

    async fn requests(&self) -> Vec<CompletionRequest> {
        self.requests.lock().await.clone()
    }
}

#[async_trait]
impl Provider for ChildToolThenFinalProvider {
    async fn stream_completion(&self, req: CompletionRequest) -> ProviderEventStream {
        let mut requests = self.requests.lock().await;
        requests.push(req);
        let call_count = requests.len();
        drop(requests);

        if call_count == 1 {
            return Box::pin(tokio_stream::iter(vec![
                ProviderStreamEvent::Start,
                ProviderStreamEvent::ToolCallComplete {
                    tool_call_id: "call_read_fixture".to_string(),
                    function_name: "read".to_string(),
                    arguments_json: json!({
                        "filePath": "fixture.txt",
                        "offset": 1,
                        "limit": 1,
                    })
                    .to_string(),
                },
                ProviderStreamEvent::Done {
                    usage: CompletionUsage {
                        prompt_tokens: 1,
                        completion_tokens: 1,
                        total_tokens: 2,
                    },
                },
            ]));
        }

        tokio::task::yield_now().await;
        Box::pin(tokio_stream::iter(vec![
            ProviderStreamEvent::Start,
            ProviderStreamEvent::TextDelta("child final after read".to_string()),
            ProviderStreamEvent::Done {
                usage: CompletionUsage {
                    prompt_tokens: 1,
                    completion_tokens: 1,
                    total_tokens: 2,
                },
            },
        ]))
    }
}

#[derive(Debug, Default)]
struct TaskCallingProvider {
    requests: Mutex<Vec<CompletionRequest>>,
}

impl TaskCallingProvider {
    async fn requests(&self) -> Vec<CompletionRequest> {
        self.requests.lock().await.clone()
    }
}

#[derive(Debug)]
struct DelegationContractProvider {
    requests: Mutex<Vec<CompletionRequest>>,
}

impl DelegationContractProvider {
    fn new() -> Self {
        Self {
            requests: Mutex::new(Vec::new()),
        }
    }

    async fn requests(&self) -> Vec<CompletionRequest> {
        self.requests.lock().await.clone()
    }
}

#[async_trait]
impl Provider for DelegationContractProvider {
    async fn stream_completion(&self, req: CompletionRequest) -> ProviderEventStream {
        let mut requests = self.requests.lock().await;
        requests.push(req);
        let call_count = requests.len();
        drop(requests);

        let prefix = if call_count == 1 {
            "sync child summary"
        } else {
            "background child summary"
        };
        let body = format!("{prefix}: {}", "0123456789abcdef".repeat(220));
        Box::pin(tokio_stream::iter(vec![
            ProviderStreamEvent::Start,
            ProviderStreamEvent::TextDelta(body),
            ProviderStreamEvent::Done {
                usage: CompletionUsage {
                    prompt_tokens: 1,
                    completion_tokens: 1,
                    total_tokens: 2,
                },
            },
        ]))
    }
}

#[async_trait]
impl Provider for TaskCallingProvider {
    async fn stream_completion(&self, req: CompletionRequest) -> ProviderEventStream {
        let mut requests = self.requests.lock().await;
        requests.push(req);
        let call_count = requests.len();
        drop(requests);

        let events = if call_count == 1 {
            vec![
                ProviderStreamEvent::Start,
                ProviderStreamEvent::ToolCallComplete {
                    tool_call_id: "call_task".to_string(),
                    function_name: "task".to_string(),
                    arguments_json: json!({
                        "description": "inherit model",
                        "prompt": "report child model",
                        "subagent_type": "general",
                        "run_in_background": false,
                        "load_skills": []
                    })
                    .to_string(),
                },
                ProviderStreamEvent::Done {
                    usage: CompletionUsage {
                        prompt_tokens: 1,
                        completion_tokens: 1,
                        total_tokens: 2,
                    },
                },
            ]
        } else {
            vec![
                ProviderStreamEvent::Start,
                ProviderStreamEvent::TextDelta("done".to_string()),
                ProviderStreamEvent::Done {
                    usage: CompletionUsage {
                        prompt_tokens: 1,
                        completion_tokens: 1,
                        total_tokens: 2,
                    },
                },
            ]
        };

        Box::pin(tokio_stream::iter(events))
    }
}

fn worker_profile(toolset: &[&str]) -> AgentProfile {
    AgentProfile {
        name: "deep".to_string(),
        category: "deep".to_string(),
        model_ref: "default:deep".to_string(),
        model_ref_explicit: true,
        system_prompt: "deep prompt".to_string(),
        max_iters: Some(12),
        temperature: Some(0.0),
        tool_failure_mode: harness_core::config::ToolFailureMode::FailTurn,
        toolset: toolset.iter().map(|tool| (*tool).to_string()).collect(),
    }
}

fn named_worker_profile(name: &str, toolset: &[&str]) -> AgentProfile {
    AgentProfile {
        name: name.to_string(),
        category: name.to_string(),
        model_ref: "default:deep".to_string(),
        model_ref_explicit: true,
        system_prompt: format!("{name} prompt"),
        max_iters: Some(12),
        temperature: Some(0.0),
        tool_failure_mode: harness_core::config::ToolFailureMode::FailTurn,
        toolset: toolset.iter().map(|tool| (*tool).to_string()).collect(),
    }
}

fn named_worker_profile_with_prompt(name: &str, toolset: &[&str], system_prompt: &str) -> AgentProfile {
    AgentProfile {
        name: name.to_string(),
        category: name.to_string(),
        model_ref: "default:deep".to_string(),
        model_ref_explicit: true,
        system_prompt: system_prompt.to_string(),
        max_iters: Some(12),
        temperature: Some(0.0),
        tool_failure_mode: harness_core::config::ToolFailureMode::FailTurn,
        toolset: toolset.iter().map(|tool| (*tool).to_string()).collect(),
    }
}

fn write_fixture(workspace: &Path) {
    fs::write(workspace.join("fixture.txt"), "alpha\nbeta\n").expect("fixture file");
}

fn write_numbered_fixture(workspace: &Path) {
    let fixture_body = (1..=30)
        .map(|index| format!("line-{index:02}"))
        .collect::<Vec<_>>()
        .join("\n");
    fs::write(workspace.join("fixture.txt"), format!("{fixture_body}\n")).expect("fixture file");
}

fn write_skill_fixture(workspace: &Path, name: &str) {
    let skill_dir = workspace.join(".agent-harness/skills").join(name);
    fs::create_dir_all(&skill_dir).expect("skill dir");
    fs::write(
        skill_dir.join("SKILL.md"),
        format!("---\nname: {name}\ndescription: {name} description\n---\n\n{name} body.\n"),
    )
    .expect("skill file");
}

fn write_skill_fixture_with_frontmatter(workspace: &Path, name: &str, frontmatter: &str, body: &str) {
    let skill_dir = workspace.join(".agent-harness/skills").join(name);
    fs::create_dir_all(&skill_dir).expect("skill dir");
    fs::write(
        skill_dir.join("SKILL.md"),
        format!("---\n{frontmatter}\n---\n\n{body}\n"),
    )
    .expect("skill file");
}

fn skills_registry_test_lock() -> &'static Mutex<()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
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

fn harness_config_with_skills(skills: SkillsConfig) -> HarnessConfig {
    serde_json::from_value(json!({
        "providers": {
            "default": {
                "type": "openai_compatible",
                "base_url": "http://127.0.0.1:1/v1",
                "api_key": "DUMMY",
                "api_mode": "responses",
                "models": {
                    "deep": {
                        "display_name": "Deep model"
                    }
                }
            }
        },
        "agents": {
            "deep": {
                "description": "Deep profile",
                "model_ref": "default:deep",
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

fn plan_mode_permission_policy() -> PermissionPolicy {
    PermissionPolicy::new(
        PermissionMode::Deny,
        PermissionMode::Deny,
        PermissionMode::Allow,
    )
}

fn plan_task_profiles() -> BTreeMap<String, AgentProfile> {
    BTreeMap::from([
        (
            "plan".to_string(),
            named_worker_profile("plan", &["task", "background_output", "bash"]),
        ),
        (
            "explore".to_string(),
            named_worker_profile("explore", &["read", "grep", "glob", "list"]),
        ),
        (
            "general".to_string(),
            named_worker_profile("general", &["read", "bash"]),
        ),
        (
            "custom_writer".to_string(),
            named_worker_profile("custom_writer", &["read", "edit"]),
        ),
    ])
}

fn restricted_subagent_permission_policy() -> PermissionPolicy {
    PermissionPolicy::new(
        PermissionMode::Allow,
        PermissionMode::Allow,
        PermissionMode::Allow,
    )
    .with_category_override(
        "explore",
        CategoryPermissions {
            edit: Some(PermissionMode::Deny),
            shell: Some(PermissionMode::Deny),
            network: Some(PermissionMode::Deny),
            question: Some(PermissionMode::Allow),
            task: Some(PermissionMode::Deny),
            webfetch: Some(PermissionMode::Deny),
            websearch: Some(PermissionMode::Deny),
            codesearch: Some(PermissionMode::Deny),
            lsp: Some(PermissionMode::Allow),
            ..CategoryPermissions::default()
        },
    )
    .with_category_override(
        "quick",
        CategoryPermissions {
            task: Some(PermissionMode::Deny),
            ..CategoryPermissions::default()
        },
    )
}

fn restricted_subagent_profiles() -> BTreeMap<String, AgentProfile> {
    BTreeMap::from([
        (
            "explore".to_string(),
            named_worker_profile(
                "explore",
                &["read", "grep", "glob", "list", "mcp.fixture.tool.call"],
            ),
        ),
        (
            "quick".to_string(),
            named_worker_profile("quick", &["read", "edit", "bash", "task"]),
        ),
        (
            "general".to_string(),
            named_worker_profile("general", &["read", "bash"]),
        ),
    ])
}

fn fixture_mcp_config(script_path: &Path) -> McpConfig {
    McpConfig {
        servers: BTreeMap::from([(
            "fixture".to_string(),
            McpServerConfig::Stdio {
                command: vec![script_path.to_string_lossy().into_owned()],
                env: BTreeMap::new(),
                cwd: None,
                timeout_secs: 5,
                enabled: true,
            },
        )]),
    }
}

async fn spawn_run(workspace: &Path) -> (CoordinatorHandle, RunInfo, String) {
    spawn_run_with_provider(workspace, Arc::new(StaticProvider)).await
}

async fn spawn_run_with_provider(
    workspace: &Path,
    provider: Arc<dyn Provider>,
) -> (CoordinatorHandle, RunInfo, String) {
    spawn_run_with_provider_and_profiles(
        workspace,
        provider,
        BTreeMap::from([
            (
                "deep".to_string(),
                worker_profile(&[
                    "task",
                    "background_output",
                    "background_cancel",
                    "batch",
                    "read",
                    "bash",
                ]),
            ),
            (
                "explore".to_string(),
                named_worker_profile("explore", &["read", "glob", "grep", "list"]),
            ),
            (
                "general".to_string(),
                named_worker_profile("general", &["read", "bash"]),
            ),
        ]),
    )
    .await
}

async fn spawn_run_with_provider_and_profiles(
    workspace: &Path,
    provider: Arc<dyn Provider>,
    agent_profiles: BTreeMap<String, AgentProfile>,
) -> (CoordinatorHandle, RunInfo, String) {
    let session_dir = workspace.join("sessions");
    fs::create_dir_all(&session_dir).expect("session dir");

    let mut config = CoordinatorConfig::new(session_dir);
    config.permission_policy = PermissionPolicy::new(
        PermissionMode::Allow,
        PermissionMode::Deny,
        PermissionMode::Allow,
    );
    config.provider = provider;
    config.tool_registry = Arc::new(coordinator_registry(ShellAllowlist::default()));
    config.agent_profiles = agent_profiles;

    let handle = spawn_coordinator(
        config,
        Arc::new(RealClock::new()),
        Arc::new(DefaultRedactor::default()),
    );
    let run = handle
        .start_run("native_agent_spawn_batch", workspace)
        .await
        .expect("start run");
    let worker_id = handle
        .spawn_agent_idle(anonymous_supervisor_actor(), "deep", None)
        .await
        .expect("spawn worker");

    (handle, run, worker_id)
}
