use std::collections::BTreeMap;
use std::fs;
use std::path::Path;
use std::sync::{Arc, OnceLock};

use async_trait::async_trait;
mod common;

use common::{
    anonymous_supervisor_actor, find_finished, read_events, setup_workspace,
    wait_for_request_terminal, wait_for_tool_call_finish, worker_actor, EnvGuard,
};
use harness_core::agent::{AgentModelSettings, AgentProfile};
use harness_core::clock::RealClock;
use harness_core::config::{PermissionMode, ShellAllowlist};
use harness_core::coord::{
    spawn_coordinator, CoordinatorConfig, CoordinatorError, CoordinatorHandle, RunInfo,
};
use harness_core::event::{EventV1, PermissionDecision as EventPermissionDecision, ToolCallStatus};
use harness_core::perm::PermissionPolicy;
use harness_core::redact::DefaultRedactor;
use harness_providers::{
    CompletionRequest, CompletionUsage, Provider, ProviderEventStream, ProviderStreamEvent,
};
use harness_tools::coordinator_registry;
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

#[derive(Debug, Default)]
struct TaskCallingProvider {
    requests: Mutex<Vec<CompletionRequest>>,
}

impl TaskCallingProvider {
    async fn requests(&self) -> Vec<CompletionRequest> {
        self.requests.lock().await.clone()
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

fn env_lock() -> &'static Mutex<()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
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

async fn spawn_run(workspace: &Path) -> (CoordinatorHandle, RunInfo, String) {
    spawn_run_with_provider(workspace, Arc::new(StaticProvider)).await
}

async fn spawn_run_with_provider(
    workspace: &Path,
    provider: Arc<dyn Provider>,
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
    config.agent_profiles = BTreeMap::from([
        (
            "deep".to_string(),
            worker_profile(&["task", "background_output", "batch", "read", "bash"]),
        ),
        (
            "explore".to_string(),
            named_worker_profile("explore", &["read", "glob", "grep", "list"]),
        ),
        (
            "general".to_string(),
            named_worker_profile("general", &["read", "bash"]),
        ),
    ]);

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

#[tokio::test]
async fn task_subagent_inherits_parent_turn_model_when_profile_model_is_defaulted() {
    let temp_dir = setup_workspace();
    let workspace = temp_dir.path().join("workspace");
    let session_dir = workspace.join("sessions");
    fs::create_dir_all(&session_dir).expect("session dir");

    let provider = Arc::new(TaskCallingProvider::default());
    let mut config = CoordinatorConfig::new(session_dir);
    config.permission_policy = PermissionPolicy::new(
        PermissionMode::Allow,
        PermissionMode::Deny,
        PermissionMode::Allow,
    );
    config.provider = provider.clone();
    config.tool_registry = Arc::new(coordinator_registry(ShellAllowlist::default()));
    let mut general = named_worker_profile("general", &["read", "bash"]);
    general.model_ref_explicit = false;
    let mut general = named_worker_profile("general", &["read", "bash"]);
    general.model_ref_explicit = false;
    config.agent_profiles = BTreeMap::from([
        (
            "deep".to_string(),
            worker_profile(&["task", "background_output", "batch", "read", "bash"]),
        ),
        ("general".to_string(), general),
    ]);

    let handle = spawn_coordinator(
        config,
        Arc::new(RealClock::new()),
        Arc::new(DefaultRedactor::default()),
    );
    let run = handle
        .start_run("native_task_model_inheritance", &workspace)
        .await
        .expect("start run");
    let worker_id = handle
        .spawn_agent_idle(anonymous_supervisor_actor(), "deep", None)
        .await
        .expect("spawn worker");
    let request_id = handle
        .request_agent_turn_with_model(
            anonymous_supervisor_actor(),
            worker_id,
            "delegate to general",
            Some("default:parent-model".to_string()),
            Some(AgentModelSettings {
                variant: Some("parent-variant".to_string()),
                reasoning_effort: Some("high".to_string()),
                text_verbosity: Some("low".to_string()),
                reasoning_summary: Some("auto".to_string()),
            }),
        )
        .await
        .expect("request parent turn");

    wait_for_request_terminal(&run.events_path, &request_id).await;

    let requests = provider.requests().await;
    assert!(
        requests.len() >= 2,
        "expected parent and child provider requests, got {requests:#?}"
    );
    assert_eq!(requests[0].model_id, "parent-model");
    assert_eq!(requests[1].model_id, "parent-model");
    assert_eq!(requests[1].variant.as_deref(), Some("parent-variant"));
    assert_eq!(requests[1].reasoning_effort.as_deref(), Some("high"));
    assert_eq!(requests[1].text_verbosity.as_deref(), Some("low"));
    assert_eq!(requests[1].reasoning_summary.as_deref(), Some("auto"));
}

#[tokio::test]
async fn task_subagent_keeps_explicit_profile_model_over_parent_turn_model() {
    let temp_dir = setup_workspace();
    let workspace = temp_dir.path().join("workspace");
    let session_dir = workspace.join("sessions");
    fs::create_dir_all(&session_dir).expect("session dir");

    let provider = Arc::new(TaskCallingProvider::default());
    let mut config = CoordinatorConfig::new(session_dir);
    config.permission_policy = PermissionPolicy::new(
        PermissionMode::Allow,
        PermissionMode::Deny,
        PermissionMode::Allow,
    );
    config.provider = provider.clone();
    config.tool_registry = Arc::new(coordinator_registry(ShellAllowlist::default()));
    let mut general = named_worker_profile("general", &["read", "bash"]);
    general.model_ref = "default:general".to_string();
    config.agent_profiles = BTreeMap::from([
        (
            "deep".to_string(),
            worker_profile(&["task", "background_output", "batch", "read", "bash"]),
        ),
        ("general".to_string(), general),
    ]);

    let handle = spawn_coordinator(
        config,
        Arc::new(RealClock::new()),
        Arc::new(DefaultRedactor::default()),
    );
    let run = handle
        .start_run("native_task_model_override", &workspace)
        .await
        .expect("start run");
    let worker_id = handle
        .spawn_agent_idle(anonymous_supervisor_actor(), "deep", None)
        .await
        .expect("spawn worker");
    let request_id = handle
        .request_agent_turn_with_model(
            anonymous_supervisor_actor(),
            worker_id,
            "delegate to general",
            Some("default:parent-model".to_string()),
            Some(AgentModelSettings {
                variant: Some("parent-variant".to_string()),
                reasoning_effort: Some("high".to_string()),
                text_verbosity: Some("low".to_string()),
                reasoning_summary: Some("auto".to_string()),
            }),
        )
        .await
        .expect("request parent turn");

    wait_for_request_terminal(&run.events_path, &request_id).await;

    let requests = provider.requests().await;
    assert!(
        requests.len() >= 2,
        "expected parent and child provider requests, got {requests:#?}"
    );
    assert_eq!(requests[0].model_id, "parent-model");
    assert_eq!(requests[1].model_id, "general");
    assert_eq!(requests[1].variant, None);
    assert_eq!(requests[1].reasoning_effort, None);
    assert_eq!(requests[1].text_verbosity, None);
    assert_eq!(requests[1].reasoning_summary, None);
}

#[tokio::test]
async fn native_plan_exit_switches_to_build_agent_after_approval() {
    let _guard = env_lock().lock().await;
    let _answers = EnvGuard::set(&[("HARNESS_QUESTION_ANSWERS", Some(r#"[["Yes"]]"#))]);
    let temp_dir = setup_workspace();
    let workspace = temp_dir.path().join("workspace");
    let session_dir = workspace.join("sessions");
    fs::create_dir_all(&session_dir).expect("session dir");

    let mut config = CoordinatorConfig::new(session_dir);
    config.permission_policy = plan_mode_permission_policy();
    config.tool_registry = Arc::new(coordinator_registry(ShellAllowlist::default()));
    config.agent_profiles = BTreeMap::from([
        (
            "plan".to_string(),
            named_worker_profile("plan", &["plan_exit"]),
        ),
        ("build".to_string(), named_worker_profile("build", &[])),
    ]);

    let handle = spawn_coordinator(
        config,
        Arc::new(RealClock::new()),
        Arc::new(DefaultRedactor::default()),
    );
    let run = handle
        .start_run("native_plan_exit", &workspace)
        .await
        .expect("start run");
    let plan_agent_id = handle
        .spawn_agent_idle(anonymous_supervisor_actor(), "plan", None)
        .await
        .expect("spawn plan");

    let tool_call_id = handle
        .request_tool_call(
            worker_actor(&plan_agent_id),
            Some("plan".to_string()),
            "plan_exit",
            json!({}),
        )
        .await
        .expect("request plan_exit");
    wait_for_tool_call_finish(&run.events_path, &tool_call_id).await;

    let events = read_events(&run.events_path);
    let finished = find_finished(&events, &tool_call_id);
    assert_eq!(finished.status, ToolCallStatus::Succeeded);
    let output = finished.output_json.expect("plan_exit structured output");
    assert_eq!(output["agent"], "build");
    assert_eq!(
        output["plan_file"],
        format!(".agent-harness/plans/{}.md", run.run_id)
    );
    assert_eq!(output["approved"], true);
    let build_agent_id = output["build_agent_id"]
        .as_str()
        .expect("build agent id")
        .to_string();
    assert!(output["request_id"].as_str().is_some());

    assert!(events.iter().any(|event| matches!(
        &event.payload,
        EventV1::AgentSpawned(payload)
            if payload.agent_id == build_agent_id
                && payload.profile == "build"
                && payload.parent_agent_id.as_deref() == Some(plan_agent_id.as_str())
    )));
    assert!(events.iter().any(|event| matches!(
        &event.payload,
        EventV1::UserMessageSubmitted(payload)
            if payload.text.contains("Your operational mode has changed from plan to build")
                && payload.text.contains("has been approved, and you can now edit files")
                && payload.text.contains(".agent-harness/plans/")
    )));
}

#[tokio::test]
async fn native_plan_exit_decline_leaves_plan_agent_active_without_spawning_build() {
    let _guard = env_lock().lock().await;
    let _answers = EnvGuard::set(&[("HARNESS_QUESTION_ANSWERS", Some(r#"[["No"]]"#))]);
    let temp_dir = setup_workspace();
    let workspace = temp_dir.path().join("workspace");
    let session_dir = workspace.join("sessions");
    fs::create_dir_all(&session_dir).expect("session dir");

    let mut config = CoordinatorConfig::new(session_dir);
    config.permission_policy = plan_mode_permission_policy();
    config.tool_registry = Arc::new(coordinator_registry(ShellAllowlist::default()));
    config.agent_profiles = BTreeMap::from([
        (
            "plan".to_string(),
            named_worker_profile("plan", &["plan_exit"]),
        ),
        ("build".to_string(), named_worker_profile("build", &[])),
    ]);

    let handle = spawn_coordinator(
        config,
        Arc::new(RealClock::new()),
        Arc::new(DefaultRedactor::default()),
    );
    let run = handle
        .start_run("native_plan_exit_decline", &workspace)
        .await
        .expect("start run");
    let plan_agent_id = handle
        .spawn_agent_idle(anonymous_supervisor_actor(), "plan", None)
        .await
        .expect("spawn plan");

    let tool_call_id = handle
        .request_tool_call(
            worker_actor(&plan_agent_id),
            Some("plan".to_string()),
            "plan_exit",
            json!({}),
        )
        .await
        .expect("request plan_exit");
    wait_for_tool_call_finish(&run.events_path, &tool_call_id).await;

    let events = read_events(&run.events_path);
    let finished = find_finished(&events, &tool_call_id);
    assert_eq!(finished.status, ToolCallStatus::Succeeded);
    let output = finished.output_json.expect("plan_exit structured output");
    assert_eq!(output["agent"], "plan");
    assert_eq!(output["approved"], false);
    assert_eq!(
        output["plan_file"],
        format!(".agent-harness/plans/{}.md", run.run_id)
    );
    assert!(!events.iter().any(|event| matches!(
        &event.payload,
        EventV1::AgentSpawned(payload) if payload.profile == "build"
    )));
    assert!(!events.iter().any(|event| matches!(
        &event.payload,
        EventV1::UserMessageSubmitted(payload)
            if payload.text.contains("Your operational mode has changed from plan to build")
    )));
}

#[tokio::test]
async fn native_plan_enter_switches_to_plan_agent_after_approval() {
    let _guard = env_lock().lock().await;
    let _answers = EnvGuard::set(&[("HARNESS_QUESTION_ANSWERS", Some(r#"[["Yes"]]"#))]);
    let temp_dir = setup_workspace();
    let workspace = temp_dir.path().join("workspace");
    let session_dir = workspace.join("sessions");
    fs::create_dir_all(&session_dir).expect("session dir");

    let mut config = CoordinatorConfig::new(session_dir);
    config.permission_policy = plan_mode_permission_policy();
    config.tool_registry = Arc::new(coordinator_registry(ShellAllowlist::default()));
    config.agent_profiles = BTreeMap::from([
        (
            "build".to_string(),
            named_worker_profile("build", &["plan_enter"]),
        ),
        (
            "plan".to_string(),
            named_worker_profile("plan", &["plan_exit"]),
        ),
    ]);

    let handle = spawn_coordinator(
        config,
        Arc::new(RealClock::new()),
        Arc::new(DefaultRedactor::default()),
    );
    let run = handle
        .start_run("native_plan_enter", &workspace)
        .await
        .expect("start run");
    let build_agent_id = handle
        .spawn_agent_idle(anonymous_supervisor_actor(), "build", None)
        .await
        .expect("spawn build");

    let tool_call_id = handle
        .request_tool_call(
            worker_actor(&build_agent_id),
            Some("build".to_string()),
            "plan_enter",
            json!({"goal": "implement parity", "reason": "multi-file change"}),
        )
        .await
        .expect("request plan_enter");
    wait_for_tool_call_finish(&run.events_path, &tool_call_id).await;

    let events = read_events(&run.events_path);
    let finished = find_finished(&events, &tool_call_id);
    assert_eq!(finished.status, ToolCallStatus::Succeeded);
    let output = finished.output_json.expect("plan_enter structured output");
    assert_eq!(output["agent"], "plan");
    assert_eq!(output["goal"], "implement parity");
    assert_eq!(output["approved"], true);
    assert_eq!(
        output["plan_file"],
        format!(".agent-harness/plans/{}.md", run.run_id)
    );
    let plan_agent_id = output["plan_agent_id"]
        .as_str()
        .expect("plan agent id")
        .to_string();
    assert!(output["request_id"].as_str().is_some());

    assert!(events.iter().any(|event| matches!(
        &event.payload,
        EventV1::AgentSpawned(payload)
            if payload.agent_id == plan_agent_id
                && payload.profile == "plan"
                && payload.parent_agent_id.as_deref() == Some(build_agent_id.as_str())
    )));
    assert!(events.iter().any(|event| matches!(
        &event.payload,
        EventV1::UserMessageSubmitted(payload)
            if payload.text.contains("Your operational mode has changed from build to plan")
                && payload.text.contains("Original goal to plan: implement parity")
                && payload.text.contains(".agent-harness/plans/")
    )));
}

#[tokio::test]
async fn native_plan_enter_decline_leaves_build_agent_active_without_spawning_plan() {
    let _guard = env_lock().lock().await;
    let _answers = EnvGuard::set(&[("HARNESS_QUESTION_ANSWERS", Some(r#"[["No"]]"#))]);
    let temp_dir = setup_workspace();
    let workspace = temp_dir.path().join("workspace");
    let session_dir = workspace.join("sessions");
    fs::create_dir_all(&session_dir).expect("session dir");

    let mut config = CoordinatorConfig::new(session_dir);
    config.permission_policy = plan_mode_permission_policy();
    config.tool_registry = Arc::new(coordinator_registry(ShellAllowlist::default()));
    config.agent_profiles = BTreeMap::from([
        (
            "build".to_string(),
            named_worker_profile("build", &["plan_enter"]),
        ),
        (
            "plan".to_string(),
            named_worker_profile("plan", &["plan_exit"]),
        ),
    ]);

    let handle = spawn_coordinator(
        config,
        Arc::new(RealClock::new()),
        Arc::new(DefaultRedactor::default()),
    );
    let run = handle
        .start_run("native_plan_enter_decline", &workspace)
        .await
        .expect("start run");
    let build_agent_id = handle
        .spawn_agent_idle(anonymous_supervisor_actor(), "build", None)
        .await
        .expect("spawn build");

    let tool_call_id = handle
        .request_tool_call(
            worker_actor(&build_agent_id),
            Some("build".to_string()),
            "plan_enter",
            json!({"goal": "implement parity"}),
        )
        .await
        .expect("request plan_enter");
    wait_for_tool_call_finish(&run.events_path, &tool_call_id).await;

    let events = read_events(&run.events_path);
    let finished = find_finished(&events, &tool_call_id);
    assert_eq!(finished.status, ToolCallStatus::Succeeded);
    let output = finished.output_json.expect("plan_enter structured output");
    assert_eq!(output["agent"], "build");
    assert_eq!(output["approved"], false);
    assert_eq!(output["goal"], "implement parity");
    assert!(!events.iter().any(|event| matches!(
        &event.payload,
        EventV1::AgentSpawned(payload) if payload.profile == "plan"
    )));
    assert!(!events.iter().any(|event| matches!(
        &event.payload,
        EventV1::UserMessageSubmitted(payload)
            if payload.text.contains("Your operational mode has changed from build to plan")
    )));
}

#[tokio::test]
async fn plan_profile_can_spawn_explore_but_bash_is_permission_denied() {
    let temp_dir = setup_workspace();
    let workspace = temp_dir.path().join("workspace");
    let session_dir = workspace.join("sessions");
    fs::create_dir_all(&session_dir).expect("session dir");

    let mut config = CoordinatorConfig::new(session_dir);
    config.permission_policy = plan_mode_permission_policy();
    config.tool_registry = Arc::new(coordinator_registry(ShellAllowlist::default()));
    config.agent_profiles = plan_task_profiles();

    let handle = spawn_coordinator(
        config,
        Arc::new(RealClock::new()),
        Arc::new(DefaultRedactor::default()),
    );
    let run = handle
        .start_run("native_plan_task_boundary", &workspace)
        .await
        .expect("start run");
    let plan_agent_id = handle
        .spawn_agent_idle(anonymous_supervisor_actor(), "plan", None)
        .await
        .expect("spawn plan");

    let denied = handle
        .request_tool_call(
            worker_actor(&plan_agent_id),
            Some("plan".to_string()),
            "bash",
            json!({"command": "touch outside-plan", "description": "try mutating bash"}),
        )
        .await
        .expect_err("plan profile mutating bash must be permission denied");
    match denied {
        CoordinatorError::PermissionDenied(_) => {}
        other => panic!("expected PermissionDenied, got {other:?}"),
    }
    let events = read_events(&run.events_path);
    assert!(events.iter().any(|event| matches!(
        &event.payload,
        EventV1::PermissionResolved(data)
            if data.decision == EventPermissionDecision::Deny
                && data.reason.as_deref().is_some_and(|reason|
                    reason.contains("read-only inspection commands"))
    )));

    let task_tool_call_id = handle
        .request_tool_call(
            worker_actor(&plan_agent_id),
            Some("plan".to_string()),
            "task",
            json!({
                "subagent_type": "explore",
                "description": "Explore child",
                "prompt": "Inspect the fixture",
                "run_in_background": true,
                "load_skills": []
            }),
        )
        .await
        .expect("plan profile can call task");
    wait_for_tool_call_finish(&run.events_path, &task_tool_call_id).await;

    let events = read_events(&run.events_path);
    let finished = find_finished(&events, &task_tool_call_id);
    assert_eq!(finished.status, ToolCallStatus::Succeeded);
    let output = finished.output_json.expect("task structured output");
    assert_eq!(output["profile"], json!("explore"));
    let child_session_id = output["child_session_id"]
        .as_str()
        .expect("child session id");
    assert!(events.iter().any(|event| matches!(
        &event.payload,
        EventV1::AgentSpawned(payload)
            if payload.agent_id == child_session_id && payload.profile == "explore"
    )));

    let denied_general = handle
        .request_tool_call(
            worker_actor(&plan_agent_id),
            Some("plan".to_string()),
            "task",
            json!({
                "subagent_type": "general",
                "description": "General child",
                "prompt": "Try write-capable delegation",
                "run_in_background": true,
                "load_skills": []
            }),
        )
        .await
        .expect("plan profile can request task policy denial");
    wait_for_tool_call_finish(&run.events_path, &denied_general).await;

    let events = read_events(&run.events_path);
    let denied_finished = find_finished(&events, &denied_general);
    assert_eq!(denied_finished.status, ToolCallStatus::Failed);
    assert!(denied_finished
        .output_summary
        .as_deref()
        .unwrap_or_default()
        .contains("Plan mode may only delegate to the read-only `explore` profile"));
    assert!(!events.iter().any(|event| matches!(
        &event.payload,
        EventV1::AgentSpawned(payload) if payload.profile == "general"
    )));

    let denied_custom = handle
        .request_tool_call(
            worker_actor(&plan_agent_id),
            Some("plan".to_string()),
            "task",
            json!({
                "subagent_type": "custom_writer",
                "description": "Custom child",
                "prompt": "Try user-defined write-capable delegation",
                "run_in_background": true,
                "load_skills": []
            }),
        )
        .await
        .expect("plan profile can request custom task policy denial");
    wait_for_tool_call_finish(&run.events_path, &denied_custom).await;

    let events = read_events(&run.events_path);
    let denied_custom_finished = find_finished(&events, &denied_custom);
    assert_eq!(denied_custom_finished.status, ToolCallStatus::Failed);
    assert!(denied_custom_finished
        .output_summary
        .as_deref()
        .unwrap_or_default()
        .contains("Plan mode may only delegate to the read-only `explore` profile"));
    assert!(!events.iter().any(|event| matches!(
        &event.payload,
        EventV1::AgentSpawned(payload) if payload.profile == "custom_writer"
    )));
}

#[tokio::test]
async fn task_subagent_type_selects_explore_and_general_profiles() {
    let temp_dir = setup_workspace();
    let workspace = temp_dir.path().join("workspace");

    let (handle, run, worker_id) = spawn_run(&workspace).await;

    for profile in ["explore", "general"] {
        let tool_call_id = handle
            .request_tool_call(
                worker_actor(&worker_id),
                Some("deep".to_string()),
                "task",
                json!({
                    "subagent_type": profile,
                    "description": format!("{profile} child"),
                    "prompt": "Return a short answer",
                    "run_in_background": true,
                    "load_skills": []
                }),
            )
            .await
            .expect("request task");
        wait_for_tool_call_finish(&run.events_path, &tool_call_id).await;

        let events = read_events(&run.events_path);
        let finished = find_finished(&events, &tool_call_id);
        assert_eq!(finished.status, ToolCallStatus::Succeeded);
        let output = finished.output_json.expect("task structured output");
        assert_eq!(output["profile"], json!(profile));
        assert_eq!(output["runtime"]["profile"], json!(profile));
        assert_eq!(output["runtime"]["category"], json!(profile));
        assert_eq!(output["effective_model_ref"], json!("default:deep"));
        assert_eq!(output["can_redelegate"], json!(false));
        assert_eq!(output["has_background_output"], json!(false));
        assert!(output["child_toolset"].is_array());
        assert!(output["next_actions"]
            .as_array()
            .expect("next actions")
            .iter()
            .any(|action| action["action"] == json!("cancel")
                && action["tool"] == json!("background_output")));
        let child_session_id = output["child_session_id"]
            .as_str()
            .expect("child session id");
        assert!(events.iter().any(|event| matches!(
            &event.payload,
            EventV1::AgentSpawned(payload)
                if payload.agent_id == child_session_id && payload.profile == profile
        )));
    }
}

#[tokio::test]
async fn task_subagent_type_wins_when_category_hint_is_also_present() {
    let temp_dir = setup_workspace();
    let workspace = temp_dir.path().join("workspace");

    let (handle, run, worker_id) = spawn_run(&workspace).await;

    let tool_call_id = handle
        .request_tool_call(
            worker_actor(&worker_id),
            Some("deep".to_string()),
            "task",
            json!({
                "category": "quick",
                "subagent_type": "general",
                "description": "Direct subagent with category hint",
                "prompt": "Return a short answer",
                "run_in_background": true,
                "load_skills": []
            }),
        )
        .await
        .expect("request task");
    wait_for_tool_call_finish(&run.events_path, &tool_call_id).await;

    let events = read_events(&run.events_path);
    let finished = find_finished(&events, &tool_call_id);
    assert_eq!(finished.status, ToolCallStatus::Succeeded);
    let output = finished.output_json.expect("task structured output");
    assert_eq!(output["profile"], json!("general"));
    let child_session_id = output["child_session_id"]
        .as_str()
        .expect("child session id");
    assert!(events.iter().any(|event| matches!(
        &event.payload,
        EventV1::AgentSpawned(payload)
            if payload.agent_id == child_session_id && payload.profile == "general"
    )));
}

#[tokio::test]
async fn task_category_without_matching_profile_falls_back_to_general() {
    let temp_dir = setup_workspace();
    let workspace = temp_dir.path().join("workspace");

    let (handle, run, worker_id) = spawn_run(&workspace).await;

    let tool_call_id = handle
        .request_tool_call(
            worker_actor(&worker_id),
            Some("deep".to_string()),
            "task",
            json!({
                "category": "quick",
                "description": "Category fallback child",
                "prompt": "Return a short answer",
                "run_in_background": true,
                "load_skills": []
            }),
        )
        .await
        .expect("request task");
    wait_for_tool_call_finish(&run.events_path, &tool_call_id).await;

    let events = read_events(&run.events_path);
    let finished = find_finished(&events, &tool_call_id);
    assert_eq!(finished.status, ToolCallStatus::Succeeded);
    let output = finished.output_json.expect("task structured output");
    assert_eq!(output["profile"], json!("general"));
    let child_session_id = output["child_session_id"]
        .as_str()
        .expect("child session id");
    assert!(events.iter().any(|event| matches!(
        &event.payload,
        EventV1::AgentSpawned(payload)
            if payload.agent_id == child_session_id
                && payload.profile == "general"
    )));
}

#[tokio::test]
async fn background_output_retrieves_completed_child_result_by_request_id() {
    let temp_dir = setup_workspace();
    let workspace = temp_dir.path().join("workspace");

    let (handle, run, worker_id) = spawn_run(&workspace).await;

    let task_tool_call_id = handle
        .request_tool_call(
            worker_actor(&worker_id),
            Some("deep".to_string()),
            "task",
            json!({
                "category": "deep",
                "description": "Background child",
                "prompt": "Return a concise completed result",
                "run_in_background": true,
                "load_skills": []
            }),
        )
        .await
        .expect("request task");
    wait_for_tool_call_finish(&run.events_path, &task_tool_call_id).await;

    let task_events = read_events(&run.events_path);
    let task_finished = find_finished(&task_events, &task_tool_call_id);
    let task_output = task_finished.output_json.expect("task structured output");
    let request_id = task_output["child_request_id"]
        .as_str()
        .expect("child request id")
        .to_string();
    wait_for_request_terminal(&run.events_path, &request_id).await;

    let output_tool_call_id = handle
        .request_tool_call(
            worker_actor(&worker_id),
            Some("deep".to_string()),
            "background_output",
            json!({
                "request_id": request_id,
                "block": true,
                "timeout_ms": 1
            }),
        )
        .await
        .expect("request background output");
    wait_for_tool_call_finish(&run.events_path, &output_tool_call_id).await;

    let events = read_events(&run.events_path);
    let finished = find_finished(&events, &output_tool_call_id);
    assert_eq!(finished.status, ToolCallStatus::Succeeded);
    let output = finished
        .output_json
        .expect("background output structured json");
    assert_eq!(output["request_id"], json!(request_id));
    assert_eq!(output["status"], json!("completed"));
    assert_eq!(output["terminal"], json!(true));
    assert_eq!(output["timed_out"], json!(false));
    assert!(output["result_summary"]
        .as_str()
        .is_some_and(|value| !value.is_empty()));
    assert_eq!(output["runtime"]["profile"], json!("deep"));
    assert_eq!(output["runtime"]["model_ref"], json!("default:deep"));
    assert_eq!(output["runtime"]["can_redelegate"], json!(true));
    assert!(output["next_actions"]
        .as_array()
        .expect("next actions")
        .iter()
        .any(|action| action["action"] == json!("check_status")));
}

#[tokio::test]
async fn background_output_can_cancel_authorized_child_request() {
    let temp_dir = setup_workspace();
    let workspace = temp_dir.path().join("workspace");

    let (handle, run, worker_id) =
        spawn_run_with_provider(&workspace, Arc::new(BlockingProvider)).await;

    let task_tool_call_id = handle
        .request_tool_call(
            worker_actor(&worker_id),
            Some("deep".to_string()),
            "task",
            json!({
                "category": "deep",
                "description": "Cancellable child",
                "prompt": "Keep running until cancelled",
                "run_in_background": true,
                "load_skills": []
            }),
        )
        .await
        .expect("request task");
    wait_for_tool_call_finish(&run.events_path, &task_tool_call_id).await;

    let task_events = read_events(&run.events_path);
    let task_finished = find_finished(&task_events, &task_tool_call_id);
    let task_output = task_finished.output_json.expect("task structured output");
    let request_id = task_output["child_request_id"]
        .as_str()
        .expect("child request id")
        .to_string();

    let cancel_tool_call_id = handle
        .request_tool_call(
            worker_actor(&worker_id),
            Some("deep".to_string()),
            "background_output",
            json!({
                "request_id": request_id,
                "cancel": true,
                "reason": "test requested cancellation"
            }),
        )
        .await
        .expect("request background cancellation");
    wait_for_tool_call_finish(&run.events_path, &cancel_tool_call_id).await;

    let events = read_events(&run.events_path);
    let finished = find_finished(&events, &cancel_tool_call_id);
    assert_eq!(finished.status, ToolCallStatus::Succeeded);
    let output = finished
        .output_json
        .expect("background cancel structured json");
    assert_eq!(output["request_id"], json!(request_id));
    assert_eq!(output["status"], json!("cancelled"));
    assert_eq!(output["terminal"], json!(true));
    assert_eq!(output["cancel_requested"], json!(true));
    assert_eq!(output["cancel_performed"], json!(true));
    assert_eq!(
        output["cancel_reason"],
        json!("test requested cancellation")
    );
    assert_eq!(output["runtime"]["profile"], json!("deep"));
    assert!(events.iter().any(|event| matches!(
        &event.payload,
        EventV1::TaskCancelled(payload)
            if event.correlation_id.as_deref() == Some(request_id.as_str())
                && payload.reason == "test requested cancellation"
    )));
}

#[tokio::test]
async fn background_output_cancel_after_terminal_does_not_report_performed() {
    let temp_dir = setup_workspace();
    let workspace = temp_dir.path().join("workspace");

    let (handle, run, worker_id) = spawn_run(&workspace).await;

    let task_tool_call_id = handle
        .request_tool_call(
            worker_actor(&worker_id),
            Some("deep".to_string()),
            "task",
            json!({
                "category": "deep",
                "description": "Completed child",
                "prompt": "Return before cancellation",
                "run_in_background": true,
                "load_skills": []
            }),
        )
        .await
        .expect("request task");
    wait_for_tool_call_finish(&run.events_path, &task_tool_call_id).await;

    let task_events = read_events(&run.events_path);
    let task_finished = find_finished(&task_events, &task_tool_call_id);
    let task_output = task_finished.output_json.expect("task structured output");
    let request_id = task_output["child_request_id"]
        .as_str()
        .expect("child request id")
        .to_string();
    wait_for_request_terminal(&run.events_path, &request_id).await;

    let cancel_tool_call_id = handle
        .request_tool_call(
            worker_actor(&worker_id),
            Some("deep".to_string()),
            "background_output",
            json!({
                "request_id": request_id,
                "cancel": true,
                "reason": "too late to cancel"
            }),
        )
        .await
        .expect("request terminal cancellation status");
    wait_for_tool_call_finish(&run.events_path, &cancel_tool_call_id).await;

    let events = read_events(&run.events_path);
    let finished = find_finished(&events, &cancel_tool_call_id);
    assert_eq!(finished.status, ToolCallStatus::Succeeded);
    let output = finished
        .output_json
        .expect("terminal cancel structured json");
    assert_eq!(output["request_id"], json!(request_id));
    assert_eq!(output["status"], json!("completed"));
    assert_eq!(output["terminal"], json!(true));
    assert_eq!(output["cancel_requested"], json!(true));
    assert_eq!(output["cancel_performed"], json!(false));
    assert!(output["cancel_reason"].is_null());
    assert!(!events.iter().any(|event| matches!(
        &event.payload,
        EventV1::TaskCancelled(_) if event.correlation_id.as_deref() == Some(request_id.as_str())
    )));
}

#[tokio::test]
async fn background_output_rejects_sibling_request_ids() {
    let temp_dir = setup_workspace();
    let workspace = temp_dir.path().join("workspace");

    let (handle, run, worker_id) = spawn_run(&workspace).await;
    let sibling_worker_id = handle
        .spawn_agent(anonymous_supervisor_actor(), "deep", None)
        .await
        .expect("spawn sibling worker");

    let task_tool_call_id = handle
        .request_tool_call(
            worker_actor(&worker_id),
            Some("deep".to_string()),
            "task",
            json!({
                "category": "deep",
                "description": "Private child",
                "prompt": "Return a concise completed result",
                "run_in_background": true,
                "load_skills": []
            }),
        )
        .await
        .expect("request task");
    wait_for_tool_call_finish(&run.events_path, &task_tool_call_id).await;

    let task_events = read_events(&run.events_path);
    let task_finished = find_finished(&task_events, &task_tool_call_id);
    let task_output = task_finished.output_json.expect("task structured output");
    let request_id = task_output["child_request_id"]
        .as_str()
        .expect("child request id")
        .to_string();

    let output_tool_call_id = handle
        .request_tool_call(
            worker_actor(&sibling_worker_id),
            Some("deep".to_string()),
            "background_output",
            json!({
                "request_id": request_id,
                "block": false
            }),
        )
        .await
        .expect("request unauthorized background output");
    wait_for_tool_call_finish(&run.events_path, &output_tool_call_id).await;

    let events = read_events(&run.events_path);
    let finished = find_finished(&events, &output_tool_call_id);
    assert_eq!(finished.status, ToolCallStatus::Failed);
    assert!(finished
        .output_summary
        .as_deref()
        .is_some_and(|summary| summary.contains("not in the caller's task lineage")));
}

#[tokio::test]
async fn background_output_rejects_excessive_block_timeout() {
    let temp_dir = setup_workspace();
    let workspace = temp_dir.path().join("workspace");

    let (handle, run, worker_id) = spawn_run(&workspace).await;

    let output_tool_call_id = handle
        .request_tool_call(
            worker_actor(&worker_id),
            Some("deep".to_string()),
            "background_output",
            json!({
                "request_id": "req_missing",
                "block": true,
                "timeout_ms": 300_001
            }),
        )
        .await
        .expect("request background output");
    wait_for_tool_call_finish(&run.events_path, &output_tool_call_id).await;

    let events = read_events(&run.events_path);
    let finished = find_finished(&events, &output_tool_call_id);
    assert_eq!(finished.status, ToolCallStatus::Failed);
    assert!(finished
        .output_summary
        .as_deref()
        .is_some_and(|summary| summary.contains("timeout must be <= 300000 ms")));
}

#[tokio::test]
async fn child_agent_toolset_boundary_is_enforced() {
    let temp_dir = setup_workspace();
    let workspace = temp_dir.path().join("workspace");

    let (handle, run, worker_id) = spawn_run(&workspace).await;

    let task_tool_call_id = handle
        .request_tool_call(
            worker_actor(&worker_id),
            Some("deep".to_string()),
            "task",
            json!({
                "subagent_type": "explore",
                "description": "Restricted child",
                "prompt": "Stay read-only",
                "run_in_background": true,
                "load_skills": []
            }),
        )
        .await
        .expect("spawn restricted child");

    wait_for_tool_call_finish(&run.events_path, &task_tool_call_id).await;
    let events = read_events(&run.events_path);
    let finished = find_finished(&events, &task_tool_call_id);
    let output = finished.output_json.expect("task structured output");
    let child_session_id = output["child_session_id"]
        .as_str()
        .expect("child session id");

    let denied = handle
        .request_tool_call(
            worker_actor(child_session_id),
            Some("explore".to_string()),
            "bash",
            json!({"command": "true", "description": "try child shell"}),
        )
        .await
        .expect_err("explore child must not be able to call bash");
    assert!(denied
        .to_string()
        .contains("tool `bash` is not in worker toolset"));
}

#[tokio::test]
async fn native_batch_and_agent_spawn_preserve_child_lineage_permissions_and_order() {
    let temp_dir = setup_workspace();
    let workspace = temp_dir.path().join("workspace");
    write_numbered_fixture(&workspace);

    let (handle, run, worker_id) = spawn_run(&workspace).await;

    let native_spawn_tool_call_id = handle
        .request_tool_call(
            worker_actor(&worker_id),
            Some("deep".to_string()),
            "task",
            json!({
                "category": "deep",
                "description": "Native background child",
                "prompt": "Say hello from native child",
                "run_in_background": true,
                "load_skills": ["rust-best-practices"],
                "command": "delegate-native",
            }),
        )
        .await
        .expect("request native task");
    wait_for_tool_call_finish(&run.events_path, &native_spawn_tool_call_id).await;

    let compat_task_tool_call_id = handle
        .request_tool_call(
            worker_actor(&worker_id),
            Some("deep".to_string()),
            "task",
            json!({
                "category": "deep",
                "description": "Compat background child",
                "prompt": "Say hello from compat child",
                "run_in_background": true,
                "load_skills": ["rust-best-practices"],
            }),
        )
        .await
        .expect("request compat task");
    wait_for_tool_call_finish(&run.events_path, &compat_task_tool_call_id).await;

    let native_batch_tool_call_id = handle
        .request_tool_call(
            worker_actor(&worker_id),
            Some("deep".to_string()),
            "batch",
            json!({
                "tool_calls": [
                    {"tool": "read", "parameters": {"filePath": "fixture.txt"}},
                    {"tool": "bash", "parameters": {"command": "ls", "workdir": ".", "description": "List workspace"}},
                    {
                        "tool": "batch",
                        "parameters": {
                            "tool_calls": [{"tool": "read", "parameters": {"filePath": "fixture.txt"}}]
                        }
                    },
                    {"tool": "read", "parameters": {"filePath": "fixture.txt", "offset": 1, "limit": 1}}
                ]
            }),
        )
        .await
        .expect("request native batch");
    wait_for_tool_call_finish(&run.events_path, &native_batch_tool_call_id).await;

    let compat_batch_tool_call_id = handle
        .request_tool_call(
            worker_actor(&worker_id),
            Some("deep".to_string()),
            "batch",
            json!({
                "tool_calls": [
                    {"tool": "read", "parameters": {"filePath": "fixture.txt", "offset": 2, "limit": 1}},
                    {"tool": "read", "parameters": {"filePath": "fixture.txt", "offset": 1, "limit": 1}}
                ]
            }),
        )
        .await
        .expect("request compat batch");
    wait_for_tool_call_finish(&run.events_path, &compat_batch_tool_call_id).await;

    handle.stop_run().await.expect("stop run");
    let events = read_events(&run.events_path);

    let native_spawn_finished = find_finished(&events, &native_spawn_tool_call_id);
    assert_eq!(native_spawn_finished.status, ToolCallStatus::Succeeded);
    let native_spawn_metadata = native_spawn_finished
        .metadata
        .as_ref()
        .expect("native spawn metadata");
    assert_eq!(
        native_spawn_metadata.canonical_tool_id.as_deref(),
        Some("task")
    );
    assert_eq!(native_spawn_metadata.alias_source_tool_id.as_deref(), None);
    let native_spawn_output = native_spawn_finished
        .output_json
        .as_ref()
        .expect("native spawn output json");
    assert_eq!(native_spawn_output.get("mode"), Some(&json!("background")));
    assert_eq!(native_spawn_output.get("status"), Some(&json!("scheduled")));
    let native_child_session = native_spawn_output
        .get("child_session_id")
        .and_then(Value::as_str)
        .expect("native child session id");
    let native_child_request = native_spawn_output
        .get("child_request_id")
        .and_then(Value::as_str)
        .expect("native child request id");
    assert_eq!(
        native_spawn_metadata
            .lineage
            .as_ref()
            .and_then(|lineage| lineage.child_session_id.as_deref()),
        Some(native_child_session)
    );
    assert_eq!(
        native_spawn_metadata
            .lineage
            .as_ref()
            .and_then(|lineage| lineage.child_request_id.as_deref()),
        Some(native_child_request)
    );

    let native_completed = events
        .iter()
        .find_map(|event| match &event.payload {
            EventV1::TaskCompleted(payload)
                if payload
                    .metadata
                    .as_ref()
                    .and_then(|metadata| metadata.lineage.as_ref())
                    .and_then(|lineage| lineage.parent_tool_call_id.as_deref())
                    == Some(native_spawn_tool_call_id.as_str()) =>
            {
                Some(payload)
            }
            _ => None,
        })
        .expect("native spawn task completion");
    assert_eq!(
        native_completed
            .metadata
            .as_ref()
            .and_then(|metadata| metadata.lineage.as_ref())
            .and_then(|lineage| lineage.child_session_id.as_deref()),
        Some(native_child_session)
    );
    assert_eq!(
        native_completed
            .metadata
            .as_ref()
            .and_then(|metadata| metadata.lineage.as_ref())
            .and_then(|lineage| lineage.child_request_id.as_deref()),
        Some(native_child_request)
    );

    let compat_task_finished = find_finished(&events, &compat_task_tool_call_id);
    let compat_task_metadata = compat_task_finished
        .metadata
        .as_ref()
        .expect("compat task metadata");
    assert_eq!(
        compat_task_metadata.canonical_tool_id.as_deref(),
        Some("task")
    );
    assert_eq!(compat_task_metadata.alias_source_tool_id.as_deref(), None);
    let compat_task_output = compat_task_finished
        .output_json
        .as_ref()
        .expect("compat task output json");
    assert_eq!(compat_task_output.get("mode"), Some(&json!("background")));
    assert_eq!(
        compat_task_output
            .get("child_session_id")
            .and_then(Value::as_str),
        compat_task_metadata
            .lineage
            .as_ref()
            .and_then(|lineage| lineage.child_session_id.as_deref())
    );

    let native_batch_finished = find_finished(&events, &native_batch_tool_call_id);
    let native_batch_metadata = native_batch_finished
        .metadata
        .as_ref()
        .expect("native batch metadata");
    assert_eq!(
        native_batch_metadata.canonical_tool_id.as_deref(),
        Some("batch")
    );
    assert_eq!(native_batch_metadata.alias_source_tool_id.as_deref(), None);
    let native_batch_output = native_batch_finished
        .output_json
        .as_ref()
        .expect("native batch output json");
    assert_eq!(
        native_batch_output.pointer("/execution/concurrency"),
        Some(&json!("parallel"))
    );
    assert_eq!(
        native_batch_output.pointer("/execution/result_order"),
        Some(&json!("input"))
    );
    assert_eq!(
        native_batch_output.pointer("/execution/nested_batch_disallowed"),
        Some(&json!(true))
    );
    assert_eq!(
        native_batch_output.pointer("/audit/successful"),
        Some(&json!(2))
    );
    assert_eq!(
        native_batch_output.pointer("/audit/failed"),
        Some(&json!(2))
    );
    let native_details = native_batch_output
        .get("details")
        .and_then(Value::as_array)
        .expect("native batch details array");
    assert_eq!(native_details.len(), 4);
    assert_eq!(native_details[0].get("index"), Some(&json!(0)));
    assert_eq!(native_details[0].get("tool_id"), Some(&json!("read")));
    assert_eq!(
        native_details[0].pointer("/request/parameter_keys/0"),
        Some(&json!("filePath"))
    );
    assert_eq!(
        native_details[0].pointer("/request/parameters_redacted"),
        Some(&json!(true))
    );
    assert!(native_details[0].get("parameters").is_none());
    assert_eq!(native_details[0].get("success"), Some(&json!(true)));

    assert_eq!(native_details[1].get("index"), Some(&json!(1)));
    assert_eq!(native_details[1].get("tool_id"), Some(&json!("bash")));
    assert_eq!(native_details[1].get("success"), Some(&json!(false)));
    assert!(native_details[1]
        .get("error")
        .and_then(Value::as_str)
        .expect("shell error")
        .contains("tool call denied"));

    assert_eq!(native_details[2].get("index"), Some(&json!(2)));
    assert_eq!(native_details[2].get("tool_id"), Some(&json!("batch")));
    assert_eq!(native_details[2].get("success"), Some(&json!(false)));
    assert!(native_details[2]
        .get("error")
        .and_then(Value::as_str)
        .expect("nested batch error")
        .contains("cannot be nested"));

    assert_eq!(native_details[3].get("index"), Some(&json!(3)));
    assert_eq!(native_details[3].get("tool_id"), Some(&json!("read")));
    assert_eq!(native_details[3].get("success"), Some(&json!(true)));

    assert!(events.iter().any(|event| {
        matches!(
            &event.payload,
            EventV1::PermissionResolved(data)
                if data.decision == EventPermissionDecision::Deny
                    && data.reason.as_deref() == Some("policy denied request (shell)")
        )
    }));

    let compat_batch_finished = find_finished(&events, &compat_batch_tool_call_id);
    let compat_batch_metadata = compat_batch_finished
        .metadata
        .as_ref()
        .expect("compat batch metadata");
    assert_eq!(
        compat_batch_metadata.canonical_tool_id.as_deref(),
        Some("batch")
    );
    assert_eq!(compat_batch_metadata.alias_source_tool_id.as_deref(), None);

    let compat_batch_output = compat_batch_finished
        .output_json
        .as_ref()
        .expect("compat batch output json");
    let compat_details = compat_batch_output
        .get("details")
        .and_then(Value::as_array)
        .expect("compat batch details");
    assert_eq!(compat_details.len(), 2);
    assert_eq!(compat_details[0].get("index"), Some(&json!(0)));
    assert_eq!(compat_details[1].get("index"), Some(&json!(1)));
    assert!(compat_details[0]
        .get("summary")
        .and_then(Value::as_str)
        .expect("first compat summary")
        .contains("|line-02"));
    assert!(compat_details[1]
        .get("summary")
        .and_then(Value::as_str)
        .expect("second compat summary")
        .contains("|line-01"));
}

#[tokio::test]
async fn compat_task_and_batch_delegate_to_native_orchestration() {
    let temp_dir = setup_workspace();
    let workspace = temp_dir.path().join("workspace");
    write_numbered_fixture(&workspace);

    let (handle, run, worker_id) = spawn_run(&workspace).await;

    let compat_task_tool_call_id = handle
        .request_tool_call(
            worker_actor(&worker_id),
            Some("deep".to_string()),
            "task",
            json!({
                "category": "deep",
                "description": "Compat child",
                "prompt": "Say hello from compat child",
                "run_in_background": true,
                "load_skills": ["rust-best-practices"],
            }),
        )
        .await
        .expect("request compat task");
    wait_for_tool_call_finish(&run.events_path, &compat_task_tool_call_id).await;

    let compat_batch_tool_call_id = handle
        .request_tool_call(
            worker_actor(&worker_id),
            Some("deep".to_string()),
            "batch",
            json!({
                "tool_calls": [
                    {"tool": "read", "parameters": {"filePath": "fixture.txt", "offset": 2, "limit": 1}},
                    {
                        "tool": "batch",
                        "parameters": {
                            "tool_calls": [
                                {"tool": "read", "parameters": {"filePath": "fixture.txt"}}
                            ]
                        }
                    },
                    {"tool": "read", "parameters": {"filePath": "fixture.txt", "offset": 1, "limit": 1}}
                ]
            }),
        )
        .await
        .expect("request compat batch");
    wait_for_tool_call_finish(&run.events_path, &compat_batch_tool_call_id).await;

    handle.stop_run().await.expect("stop run");
    let events = read_events(&run.events_path);

    let compat_task_finished = find_finished(&events, &compat_task_tool_call_id);
    assert_eq!(compat_task_finished.status, ToolCallStatus::Succeeded);
    let compat_task_metadata = compat_task_finished
        .metadata
        .as_ref()
        .expect("compat task metadata");
    assert_eq!(
        compat_task_metadata.canonical_tool_id.as_deref(),
        Some("task")
    );
    assert_eq!(compat_task_metadata.alias_source_tool_id.as_deref(), None);
    let compat_task_output = compat_task_finished
        .output_json
        .as_ref()
        .expect("compat task output json");
    assert_eq!(compat_task_output.get("mode"), Some(&json!("background")));
    assert_eq!(compat_task_output.get("status"), Some(&json!("scheduled")));
    assert_eq!(
        compat_task_output
            .get("child_session_id")
            .and_then(Value::as_str),
        compat_task_metadata
            .lineage
            .as_ref()
            .and_then(|lineage| lineage.child_session_id.as_deref())
    );
    assert_eq!(
        compat_task_output
            .get("child_request_id")
            .and_then(Value::as_str),
        compat_task_metadata
            .lineage
            .as_ref()
            .and_then(|lineage| lineage.child_request_id.as_deref())
    );

    let compat_batch_finished = find_finished(&events, &compat_batch_tool_call_id);
    assert_eq!(compat_batch_finished.status, ToolCallStatus::Succeeded);
    let compat_batch_metadata = compat_batch_finished
        .metadata
        .as_ref()
        .expect("compat batch metadata");
    assert_eq!(
        compat_batch_metadata.canonical_tool_id.as_deref(),
        Some("batch")
    );
    assert_eq!(compat_batch_metadata.alias_source_tool_id.as_deref(), None);
    let compat_batch_output = compat_batch_finished
        .output_json
        .as_ref()
        .expect("compat batch output json");
    assert_eq!(
        compat_batch_output.pointer("/execution/concurrency"),
        Some(&json!("parallel"))
    );
    assert_eq!(
        compat_batch_output.pointer("/execution/result_order"),
        Some(&json!("input"))
    );
    assert_eq!(
        compat_batch_output.pointer("/execution/nested_batch_disallowed"),
        Some(&json!(true))
    );
    assert_eq!(
        compat_batch_output.pointer("/audit/successful"),
        Some(&json!(2))
    );
    assert_eq!(
        compat_batch_output.pointer("/audit/failed"),
        Some(&json!(1))
    );
    let compat_details = compat_batch_output
        .get("details")
        .and_then(Value::as_array)
        .expect("compat batch details");
    assert_eq!(compat_details.len(), 3);
    assert_eq!(compat_details[0].get("index"), Some(&json!(0)));
    assert_eq!(compat_details[0].get("tool_id"), Some(&json!("read")));
    assert_eq!(
        compat_details[0].pointer("/request/parameter_keys/0"),
        Some(&json!("filePath"))
    );
    assert_eq!(
        compat_details[0].pointer("/request/parameters_redacted"),
        Some(&json!(true))
    );
    assert_eq!(
        compat_details[0].get("canonical_tool_id"),
        Some(&json!("read"))
    );
    assert_eq!(compat_details[0].get("success"), Some(&json!(true)));
    assert!(compat_details[0]
        .get("summary")
        .and_then(Value::as_str)
        .expect("first compat summary")
        .contains("|line-02"));
    assert_eq!(compat_details[1].get("index"), Some(&json!(1)));
    assert_eq!(compat_details[1].get("tool_id"), Some(&json!("batch")));
    assert_eq!(
        compat_details[1].get("canonical_tool_id"),
        Some(&json!("batch"))
    );
    assert_eq!(compat_details[1].get("success"), Some(&json!(false)));
    assert!(compat_details[1]
        .get("error")
        .and_then(Value::as_str)
        .expect("nested compat batch error")
        .contains("cannot be nested"));
    assert_eq!(compat_details[2].get("index"), Some(&json!(2)));
    assert_eq!(compat_details[2].get("tool_id"), Some(&json!("read")));
    assert_eq!(
        compat_details[2].get("canonical_tool_id"),
        Some(&json!("read"))
    );
    assert_eq!(compat_details[2].get("success"), Some(&json!(true)));
    assert!(compat_details[2]
        .get("summary")
        .and_then(Value::as_str)
        .expect("second compat summary")
        .contains("|line-01"));
}

#[tokio::test]
async fn task_tool_rejects_unknown_child_profile_before_spawning_fallback_model() {
    let temp_dir = setup_workspace();
    let workspace = temp_dir.path().join("workspace");

    let (handle, run, worker_id) = spawn_run(&workspace).await;

    let task_tool_call_id = handle
        .request_tool_call(
            worker_actor(&worker_id),
            Some("deep".to_string()),
            "task",
            json!({
                "description": "Missing child profile",
                "prompt": "Try to inspect the repo",
                "subagent_type": "missing_profile",
                "run_in_background": false,
                "load_skills": []
            }),
        )
        .await
        .expect("request task tool");
    wait_for_tool_call_finish(&run.events_path, &task_tool_call_id).await;

    handle.stop_run().await.expect("stop run");
    let events = read_events(&run.events_path);
    let finished = find_finished(&events, &task_tool_call_id);

    assert_eq!(finished.status, ToolCallStatus::Failed);
    assert!(finished
        .output_summary
        .as_deref()
        .expect("output summary")
        .contains("Unknown child profile `missing_profile`"));
}

#[tokio::test]
async fn batch_tool_accepts_args_alias_on_real_tool_path() {
    let temp_dir = setup_workspace();
    let workspace = temp_dir.path().join("workspace");
    write_fixture(&workspace);

    let (handle, run, worker_id) = spawn_run(&workspace).await;

    let batch_tool_call_id = handle
        .request_tool_call(
            worker_actor(&worker_id),
            Some("deep".to_string()),
            "batch",
            json!({
                "tool_calls": [
                    {"tool": "read", "args": {"filePath": "fixture.txt", "offset": 2, "limit": 1}},
                    {"tool": "read", "args": {"filePath": "fixture.txt", "offset": 1, "limit": 1}}
                ]
            }),
        )
        .await
        .expect("request batch tool");
    wait_for_tool_call_finish(&run.events_path, &batch_tool_call_id).await;

    handle.stop_run().await.expect("stop run");
    let events = read_events(&run.events_path);
    let finished = find_finished(&events, &batch_tool_call_id);

    assert_eq!(finished.status, ToolCallStatus::Succeeded);
    let output = finished.output_json.as_ref().expect("batch output json");
    assert_eq!(output.pointer("/audit/successful"), Some(&json!(2)));
    let details = output
        .get("details")
        .and_then(Value::as_array)
        .expect("batch details");
    assert_eq!(
        details[0].pointer("/request/parameter_keys/0"),
        Some(&json!("filePath"))
    );
    assert_eq!(
        details[0].pointer("/request/parameters_redacted"),
        Some(&json!(true))
    );
    assert!(details[0]
        .get("summary")
        .and_then(Value::as_str)
        .expect("first summary")
        .contains("|beta"));
    assert!(details[1]
        .get("summary")
        .and_then(Value::as_str)
        .expect("second summary")
        .contains("|alpha"));
}

#[tokio::test]
async fn batch_tool_accepts_wrapper_calls_inside_tool_calls_on_real_path() {
    let temp_dir = setup_workspace();
    let workspace = temp_dir.path().join("workspace");
    write_fixture(&workspace);

    let (handle, run, worker_id) = spawn_run(&workspace).await;

    let batch_tool_call_id = handle
        .request_tool_call(
            worker_actor(&worker_id),
            Some("deep".to_string()),
            "batch",
            json!({
                "tool_calls": [
                    {"recipient_name": "functions.read", "parameters": {"filePath": "fixture.txt", "offset": 2, "limit": 1}},
                    {"recipient_name": "functions.read", "parameters": {"filePath": "fixture.txt", "offset": 1, "limit": 1}}
                ]
            }),
        )
        .await
        .expect("request batch tool");
    wait_for_tool_call_finish(&run.events_path, &batch_tool_call_id).await;

    handle.stop_run().await.expect("stop run");
    let events = read_events(&run.events_path);
    let finished = find_finished(&events, &batch_tool_call_id);

    assert_eq!(finished.status, ToolCallStatus::Succeeded);
    let output = finished.output_json.as_ref().expect("batch output json");
    assert_eq!(output.pointer("/audit/successful"), Some(&json!(2)));
    let details = output
        .get("details")
        .and_then(Value::as_array)
        .expect("batch details");
    assert_eq!(details[0].get("tool_id"), Some(&json!("read")));
    assert!(details[0]
        .get("summary")
        .and_then(Value::as_str)
        .expect("first summary")
        .contains("|beta"));
    assert!(details[1]
        .get("summary")
        .and_then(Value::as_str)
        .expect("second summary")
        .contains("|alpha"));
}

#[tokio::test]
async fn task_tool_reenters_existing_child_session_by_session_id() {
    let temp_dir = setup_workspace();
    let workspace = temp_dir.path().join("workspace");

    let (handle, run, worker_id) = spawn_run(&workspace).await;

    let first_tool_call_id = handle
        .request_tool_call(
            worker_actor(&worker_id),
            Some("deep".to_string()),
            "task",
            json!({
                "category": "deep",
                "description": "Initial child",
                "prompt": "First child turn",
                "run_in_background": true,
                "load_skills": []
            }),
        )
        .await
        .expect("request initial task");
    wait_for_tool_call_finish(&run.events_path, &first_tool_call_id).await;

    let first_events = read_events(&run.events_path);
    let first_finished = find_finished(&first_events, &first_tool_call_id);
    let first_output = first_finished
        .output_json
        .as_ref()
        .expect("initial task output json");
    let child_session_id = first_output
        .get("child_session_id")
        .and_then(Value::as_str)
        .expect("child session id")
        .to_string();
    let first_request_id = first_output
        .get("child_request_id")
        .and_then(Value::as_str)
        .expect("child request id")
        .to_string();
    wait_for_request_terminal(&run.events_path, &first_request_id).await;

    let reentry_tool_call_id = handle
        .request_tool_call(
            worker_actor(&worker_id),
            Some("deep".to_string()),
            "task",
            json!({
                "category": "deep",
                "description": "Resume child by session id",
                "prompt": "Second child turn by session_id",
                "session_id": child_session_id,
                "run_in_background": true,
                "load_skills": []
            }),
        )
        .await
        .expect("request task reentry by session_id");
    wait_for_tool_call_finish(&run.events_path, &reentry_tool_call_id).await;

    let reentry_events = read_events(&run.events_path);
    let reentry_finished = find_finished(&reentry_events, &reentry_tool_call_id);
    assert_eq!(reentry_finished.status, ToolCallStatus::Succeeded);
    let reentry_output = reentry_finished
        .output_json
        .as_ref()
        .expect("reentry task output json");
    assert_eq!(
        reentry_output
            .get("child_session_id")
            .and_then(Value::as_str),
        Some(child_session_id.as_str())
    );
    assert_eq!(
        reentry_output.get("resumed_existing_session"),
        Some(&json!(true))
    );
    assert_eq!(
        reentry_output.pointer("/child_session/resumed_existing_session"),
        Some(&json!(true))
    );
    let second_request_id = reentry_output
        .get("child_request_id")
        .and_then(Value::as_str)
        .expect("second child request id")
        .to_string();
    wait_for_request_terminal(&run.events_path, &second_request_id).await;

    handle.stop_run().await.expect("stop run");
    let events = read_events(&run.events_path);
    let child_spawn_count = events
        .iter()
        .filter(|event| {
            matches!(
                &event.payload,
                EventV1::AgentSpawned(payload) if payload.agent_id == child_session_id
            )
        })
        .count();
    assert_eq!(child_spawn_count, 1, "reentry must not spawn a new child");
}

#[tokio::test]
async fn task_tool_reenters_existing_child_session_by_task_id() {
    let temp_dir = setup_workspace();
    let workspace = temp_dir.path().join("workspace");

    let (handle, run, worker_id) = spawn_run(&workspace).await;

    let first_tool_call_id = handle
        .request_tool_call(
            worker_actor(&worker_id),
            Some("deep".to_string()),
            "task",
            json!({
                "category": "deep",
                "description": "Initial child",
                "prompt": "First child turn",
                "run_in_background": true,
                "load_skills": []
            }),
        )
        .await
        .expect("request initial task");
    wait_for_tool_call_finish(&run.events_path, &first_tool_call_id).await;

    let first_events = read_events(&run.events_path);
    let first_finished = find_finished(&first_events, &first_tool_call_id);
    let first_output = first_finished
        .output_json
        .as_ref()
        .expect("initial task output json");
    let child_task_id = first_output
        .get("task_id")
        .and_then(Value::as_str)
        .expect("child task id")
        .to_string();
    let first_request_id = first_output
        .get("child_request_id")
        .and_then(Value::as_str)
        .expect("child request id")
        .to_string();
    wait_for_request_terminal(&run.events_path, &first_request_id).await;

    let reentry_tool_call_id = handle
        .request_tool_call(
            worker_actor(&worker_id),
            Some("deep".to_string()),
            "task",
            json!({
                "category": "deep",
                "description": "Resume child by task id",
                "prompt": "Second child turn by task_id",
                "task_id": child_task_id,
                "run_in_background": true,
                "load_skills": []
            }),
        )
        .await
        .expect("request task reentry by task_id");
    wait_for_tool_call_finish(&run.events_path, &reentry_tool_call_id).await;

    let reentry_events = read_events(&run.events_path);
    let reentry_finished = find_finished(&reentry_events, &reentry_tool_call_id);
    assert_eq!(reentry_finished.status, ToolCallStatus::Succeeded);
    let reentry_output = reentry_finished
        .output_json
        .as_ref()
        .expect("reentry task output json");
    assert_eq!(
        reentry_output
            .get("child_session_id")
            .and_then(Value::as_str),
        Some(child_task_id.as_str())
    );
    assert_eq!(
        reentry_output.get("resumed_existing_session"),
        Some(&json!(true))
    );
    assert_eq!(
        reentry_output.pointer("/child_session/resumed_existing_session"),
        Some(&json!(true))
    );
    let second_request_id = reentry_output
        .get("child_request_id")
        .and_then(Value::as_str)
        .expect("second child request id")
        .to_string();
    wait_for_request_terminal(&run.events_path, &second_request_id).await;

    handle.stop_run().await.expect("stop run");
    let events = read_events(&run.events_path);
    let child_spawn_count = events
        .iter()
        .filter(|event| {
            matches!(
                &event.payload,
                EventV1::AgentSpawned(payload) if payload.agent_id == child_task_id
            )
        })
        .count();
    assert_eq!(child_spawn_count, 1, "reentry must not spawn a new child");
}

#[tokio::test]
async fn task_tool_rejects_reentry_to_sibling_child_session() {
    let temp_dir = setup_workspace();
    let workspace = temp_dir.path().join("workspace");

    let (handle, run, worker_id) = spawn_run(&workspace).await;
    let sibling_parent = handle
        .spawn_agent_idle(anonymous_supervisor_actor(), "deep", None)
        .await
        .expect("spawn sibling parent");
    let sibling_child = handle
        .spawn_agent_idle(
            anonymous_supervisor_actor(),
            "general",
            Some(sibling_parent),
        )
        .await
        .expect("spawn sibling child");

    let reentry_tool_call_id = handle
        .request_tool_call(
            worker_actor(&worker_id),
            Some("deep".to_string()),
            "task",
            json!({
                "category": "general",
                "description": "Forbidden sibling reentry",
                "prompt": "Try to drive another parent's child",
                "session_id": sibling_child,
                "run_in_background": true,
                "load_skills": []
            }),
        )
        .await
        .expect("request sibling reentry task");
    wait_for_tool_call_finish(&run.events_path, &reentry_tool_call_id).await;

    let events = read_events(&run.events_path);
    let finished = find_finished(&events, &reentry_tool_call_id);
    assert_eq!(finished.status, ToolCallStatus::Failed);
    assert!(finished
        .output_summary
        .as_deref()
        .expect("failure summary")
        .contains("is not a direct child of the calling agent"));
}

#[tokio::test]
async fn plan_task_reentry_rejects_non_explore_existing_child_profile() {
    let temp_dir = setup_workspace();
    let workspace = temp_dir.path().join("workspace");
    let session_dir = workspace.join("sessions");
    fs::create_dir_all(&session_dir).expect("session dir");

    let mut config = CoordinatorConfig::new(session_dir);
    config.permission_policy = plan_mode_permission_policy();
    config.tool_registry = Arc::new(coordinator_registry(ShellAllowlist::default()));
    config.agent_profiles = plan_task_profiles();

    let handle = spawn_coordinator(
        config,
        Arc::new(RealClock::new()),
        Arc::new(DefaultRedactor::default()),
    );
    let run = handle
        .start_run("native_plan_task_reentry_boundary", &workspace)
        .await
        .expect("start run");
    let plan_agent_id = handle
        .spawn_agent_idle(anonymous_supervisor_actor(), "plan", None)
        .await
        .expect("spawn plan");
    let general_child = handle
        .spawn_agent_idle(
            anonymous_supervisor_actor(),
            "general",
            Some(plan_agent_id.clone()),
        )
        .await
        .expect("spawn general child");

    let reentry_tool_call_id = handle
        .request_tool_call(
            worker_actor(&plan_agent_id),
            Some("plan".to_string()),
            "task",
            json!({
                "category": "explore",
                "description": "Forbidden profile reentry",
                "prompt": "Try to drive a write-capable existing child",
                "session_id": general_child,
                "run_in_background": true,
                "load_skills": []
            }),
        )
        .await
        .expect("request plan reentry task");
    wait_for_tool_call_finish(&run.events_path, &reentry_tool_call_id).await;

    let events = read_events(&run.events_path);
    let finished = find_finished(&events, &reentry_tool_call_id);
    assert_eq!(finished.status, ToolCallStatus::Failed);
    assert!(finished
        .output_summary
        .as_deref()
        .expect("failure summary")
        .contains("uses profile `general`, but the request selected `explore`"));
}

#[tokio::test]
async fn batch_rejects_more_than_25_calls_preserving_input_order() {
    let temp_dir = setup_workspace();
    let workspace = temp_dir.path().join("workspace");
    fs::write(workspace.join("fixture.txt"), "alpha\nbeta\n").expect("fixture file");

    let (handle, run, worker_id) = spawn_run(&workspace).await;
    let tool_calls = (0..26)
        .map(|index| {
            json!({
                "tool": "read",
                "parameters": {
                    "filePath": "fixture.txt",
                    "offset": index + 1,
                    "limit": 1
                }
            })
        })
        .collect::<Vec<_>>();

    let batch_tool_call_id = handle
        .request_tool_call(
            worker_actor(&worker_id),
            Some("deep".to_string()),
            "batch",
            json!({ "tool_calls": tool_calls }),
        )
        .await
        .expect("request over-limit batch");
    wait_for_tool_call_finish(&run.events_path, &batch_tool_call_id).await;

    handle.stop_run().await.expect("stop run");
    let events = read_events(&run.events_path);
    let finished = find_finished(&events, &batch_tool_call_id);
    assert_eq!(finished.status, ToolCallStatus::Succeeded);
    let output = finished.output_json.as_ref().expect("batch output json");
    assert_eq!(output.get("requested_call_count"), Some(&json!(26)));
    assert_eq!(output.get("max_calls"), Some(&json!(25)));
    assert_eq!(output.pointer("/audit/successful"), Some(&json!(25)));
    assert_eq!(output.pointer("/audit/failed"), Some(&json!(1)));
    assert_eq!(
        output.pointer("/audit/discarded_call_count"),
        Some(&json!(1))
    );

    let details = output
        .get("details")
        .and_then(Value::as_array)
        .expect("batch details");
    assert_eq!(details.len(), 26);
    for (index, detail) in details.iter().enumerate() {
        assert_eq!(detail.get("index"), Some(&json!(index)));
        assert_eq!(
            detail.pointer("/request/parameter_shape"),
            Some(&json!("object"))
        );
        assert_eq!(
            detail.pointer("/request/parameters_redacted"),
            Some(&json!(true))
        );
        assert!(detail.get("parameters").is_none());
    }
    for detail in &details[..25] {
        assert_eq!(detail.get("success"), Some(&json!(true)));
    }
    assert_eq!(details[25].get("success"), Some(&json!(false)));
    assert!(details[25]
        .get("error")
        .and_then(Value::as_str)
        .expect("over-limit batch error")
        .contains("Maximum of 25 tools allowed in batch"));
}
