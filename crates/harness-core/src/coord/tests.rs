use std::fs;
use std::path::Path;
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Duration;

use async_trait::async_trait;
use serde_json::json;
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

use crate::agent::{
    ProviderCompactionFacts, ProviderCompactionSummarySource, ProviderCompactionTailBoundary,
    ProviderContext, ProviderContextCheckpoint, ProviderContextCheckpointMetadata,
    ProviderConversationTurn,
};
use crate::clock::{FakeClock, RealClock};
use crate::config::{
    clear_registered_mcp_server_first_class_tool_ids, load_config_from_str,
    resolve_profile_model_metadata, set_registered_mcp_server_first_class_tool_ids,
    CategoryPermissions, CompactionRuntimeConfig, PermissionMode,
};
use crate::event::{
    ActorKind, AgentSpawnedEvent, ArtifactWrittenEvent, CompactionAppliedEvent,
    CompactionWrittenEvent, EventActor, EventEnvelopeV1, EventV1, HookExecutionMetadata,
    HookExecutionStatus, ProviderRequestStartedEvent, RunFinishedEvent, RunStartedEvent,
    TaskCompletedEvent, ToolCallStatus, UserMessageSubmittedEvent, SCHEMA_VERSION,
};
use crate::perm::{PermissionDecision, PermissionGrantScope, PermissionPolicy};
use crate::proj::{inspect_resume_plan, RecordedRuntimeContext};
use crate::redact::DefaultRedactor;
use crate::sched::{ConcurrencyKey, ScheduleDecision, Scheduler, SchedulerLimits};
use crate::store::JsonlFileEventStore;
use crate::tool::{Tool, ToolCapability, ToolContext, ToolError, ToolRegistry, ToolResult};

use super::{
    build_provider_context_summary, compact_provider_context,
    compaction_summary_override_from_hooks, restore_provider_context_from_history,
    spawn_coordinator, Coordinator, CoordinatorConfig, CoordinatorError, HookExecutionBatch,
    JobOutcome, JobProgressKind, ProviderCompactionTrigger, RunInfo, RunState, TaskExecutionState,
    TaskState,
};

struct TestShellTool;

struct TestMcpEchoTool;

struct TestMcpWrapperTool;

fn mcp_identity_registry_test_lock() -> &'static Mutex<()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
}

#[async_trait]
impl Tool for TestShellTool {
    fn id(&self) -> &str {
        "shell.run"
    }

    fn capability(&self) -> ToolCapability {
        ToolCapability::Shell
    }

    async fn call(
        &self,
        _ctx: ToolContext,
        args_json: serde_json::Value,
    ) -> Result<ToolResult, ToolError> {
        Ok(ToolResult::text(format!("ok {args_json}")))
    }
}

#[async_trait]
impl Tool for TestMcpEchoTool {
    fn id(&self) -> &str {
        "mcp.fixture.echo"
    }

    fn capability(&self) -> ToolCapability {
        ToolCapability::Shell
    }

    async fn call(
        &self,
        _ctx: ToolContext,
        args_json: serde_json::Value,
    ) -> Result<ToolResult, ToolError> {
        let text = args_json
            .get("text")
            .and_then(serde_json::Value::as_str)
            .unwrap_or_default();
        Ok(fake_mcp_echo_result(text))
    }
}

#[async_trait]
impl Tool for TestMcpWrapperTool {
    fn id(&self) -> &str {
        "mcp.fixture.tool.call"
    }

    fn capability(&self) -> ToolCapability {
        ToolCapability::Shell
    }

    async fn call(
        &self,
        _ctx: ToolContext,
        args_json: serde_json::Value,
    ) -> Result<ToolResult, ToolError> {
        let tool_name = args_json
            .get("tool")
            .and_then(serde_json::Value::as_str)
            .unwrap_or_default();
        let text = args_json
            .get("arguments")
            .and_then(|value| value.get("text"))
            .and_then(serde_json::Value::as_str)
            .unwrap_or_default();
        Ok(ToolResult {
            display_text: text.to_string(),
            structured_json: Some(serde_json::json!({
                "server": {
                    "id": "fixture",
                    "transport": "stdio",
                },
                "protocolVersion": "2025-06-18",
                "serverInfo": {
                    "name": "fixture",
                    "version": "1.0.0",
                },
                "payload": {
                    "tool": tool_name,
                    "arguments": { "text": text },
                    "result": {
                        "content": [{ "type": "text", "text": text }],
                        "isError": false,
                    },
                },
            })),
            artifacts: Vec::new(),
        })
    }
}

fn fake_mcp_echo_result(text: &str) -> ToolResult {
    ToolResult {
        display_text: text.to_string(),
        structured_json: Some(serde_json::json!({
            "server": {
                "id": "fixture",
                "transport": "stdio",
            },
            "protocolVersion": "2025-06-18",
            "serverInfo": {
                "name": "fixture",
                "version": "1.0.0",
            },
            "payload": {
                "tool": "echo",
                "arguments": { "text": text },
                "result": {
                    "content": [{ "type": "text", "text": text }],
                    "isError": false,
                },
            },
        })),
        artifacts: Vec::new(),
    }
}

fn test_tool_registry() -> Arc<ToolRegistry> {
    let mut registry = ToolRegistry::new();
    registry.register(Arc::new(TestShellTool));
    registry.register(Arc::new(TestMcpEchoTool));
    registry.register(Arc::new(TestMcpWrapperTool));
    Arc::new(registry)
}

fn test_config(session_dir: &Path) -> CoordinatorConfig {
    let mut config = CoordinatorConfig::new(session_dir);
    config.deterministic_store = true;
    config.tool_registry = test_tool_registry();
    config
}

#[tokio::test]
async fn perm_allow_path_proceeds() {
    let temp_dir = tempfile::tempdir().expect("tempdir");
    let mut config = test_config(temp_dir.path());
    config.permission_policy = PermissionPolicy::new(
        PermissionMode::Deny,
        PermissionMode::Allow,
        PermissionMode::Deny,
    );

    let handle = spawn_coordinator(
        config,
        Arc::new(FakeClock::new()),
        Arc::new(DefaultRedactor::default()),
    );

    let run = handle
        .start_run("perm_allow", temp_dir.path())
        .await
        .expect("start run");

    let tool_call_id = handle
        .request_tool_call(
            EventActor::new(ActorKind::Supervisor, Some("agent-supervisor".to_string())),
            Some("deep".to_string()),
            "shell.run",
            json!({"cmd": "true"}),
        )
        .await
        .expect("request tool call");

    tokio::time::sleep(Duration::from_millis(80)).await;
    handle.stop_run().await.expect("stop run");

    let events = read_events(&run.events_path);
    assert!(events.iter().any(|event| {
        matches!(
            &event.payload,
            EventV1::ToolCallRequested(data)
                if data.tool_call_id == tool_call_id
                    && event.correlation_id.as_deref() == Some(tool_call_id.as_str())
        )
    }));
    assert!(events.iter().any(|event| {
        matches!(
            &event.payload,
            EventV1::ToolCallStarted(data)
                if data.tool_call_id == tool_call_id
                    && event.correlation_id.as_deref() == Some(tool_call_id.as_str())
        )
    }));
    assert!(events.iter().any(|event| {
        matches!(
            &event.payload,
            EventV1::ToolCallFinished(data)
                if data.tool_call_id == tool_call_id
                    && data.status == ToolCallStatus::Succeeded
                    && event.correlation_id.as_deref() == Some(tool_call_id.as_str())
        )
    }));
}

#[tokio::test]
async fn perm_ask_path_blocks_until_resolved() {
    let temp_dir = tempfile::tempdir().expect("tempdir");
    let mut config = test_config(temp_dir.path());
    config.permission_policy = PermissionPolicy::new(
        PermissionMode::Deny,
        PermissionMode::Ask,
        PermissionMode::Deny,
    )
    .with_ask_timeout_ms(1_000);

    let handle = spawn_coordinator(
        config,
        Arc::new(FakeClock::new()),
        Arc::new(DefaultRedactor::default()),
    );

    let run = handle
        .start_run("perm_ask", temp_dir.path())
        .await
        .expect("start run");

    let tool_call_id = handle
        .request_tool_call(
            EventActor::new(ActorKind::Supervisor, Some("agent-supervisor".to_string())),
            Some("deep".to_string()),
            "shell.run",
            json!({"cmd": "echo blocked"}),
        )
        .await
        .expect("request tool call");

    tokio::time::sleep(Duration::from_millis(40)).await;
    let before_resolve = read_events(&run.events_path);
    assert!(
        !before_resolve.iter().any(|event| {
            matches!(
                &event.payload,
                EventV1::ToolCallStarted(data) if data.tool_call_id == tool_call_id
            )
        }),
        "tool call must not start before permission resolution"
    );

    let permission_id = before_resolve
        .iter()
        .find_map(|event| match &event.payload {
            EventV1::PermissionRequested(data)
                if data.tool_call_id.as_deref() == Some(tool_call_id.as_str()) =>
            {
                Some(data.permission_id.clone())
            }
            _ => None,
        })
        .expect("permission requested event");

    handle
        .resolve_permission(permission_id, PermissionDecision::Allow, None)
        .await
        .expect("resolve permission");

    tokio::time::sleep(Duration::from_millis(80)).await;
    handle.stop_run().await.expect("stop run");

    let events = read_events(&run.events_path);
    let requested_idx = events
        .iter()
        .position(|event| {
            matches!(
                &event.payload,
                EventV1::PermissionRequested(data)
                    if data.tool_call_id.as_deref() == Some(tool_call_id.as_str())
            )
        })
        .expect("permission requested index");
    let resolved_idx = events
        .iter()
        .position(|event| {
            matches!(
                &event.payload,
                EventV1::PermissionResolved(data)
                    if data.decision == crate::event::PermissionDecision::Allow
            )
        })
        .expect("permission resolved index");
    let started_idx = events
        .iter()
        .position(|event| {
            matches!(
                &event.payload,
                EventV1::ToolCallStarted(data) if data.tool_call_id == tool_call_id
            )
        })
        .expect("tool started index");

    assert!(requested_idx < resolved_idx);
    assert!(resolved_idx < started_idx);
}

#[tokio::test]
async fn allow_always_records_grant_and_authorizes_matching_future_shell_call() {
    let temp_dir = tempfile::tempdir().expect("tempdir");
    let mut config = test_config(temp_dir.path());
    config.permission_policy = PermissionPolicy::new(
        PermissionMode::Deny,
        PermissionMode::Ask,
        PermissionMode::Deny,
    )
    .with_ask_timeout_ms(1_000);

    let handle = spawn_coordinator(
        config,
        Arc::new(FakeClock::new()),
        Arc::new(DefaultRedactor::default()),
    );

    let run = handle
        .start_run("perm_allow_always", temp_dir.path())
        .await
        .expect("start run");

    let actor = EventActor::new(ActorKind::Supervisor, Some("agent-supervisor".to_string()));
    let first_tool_call_id = handle
        .request_tool_call(
            actor.clone(),
            Some("deep".to_string()),
            "shell.run",
            json!({"cmd": "echo durable"}),
        )
        .await
        .expect("request first tool call");

    tokio::time::sleep(Duration::from_millis(40)).await;
    let before_resolve = read_events(&run.events_path);
    let permission_id = before_resolve
        .iter()
        .find_map(|event| match &event.payload {
            EventV1::PermissionRequested(data)
                if data.tool_call_id.as_deref() == Some(first_tool_call_id.as_str()) =>
            {
                Some(data.permission_id.clone())
            }
            _ => None,
        })
        .expect("permission requested");

    handle
        .resolve_permission_with_grant_scope(
            permission_id,
            PermissionDecision::Allow,
            None,
            Some(PermissionGrantScope::Run),
        )
        .await
        .expect("resolve with durable grant");
    tokio::time::sleep(Duration::from_millis(80)).await;

    let second_tool_call_id = handle
        .request_tool_call(
            actor,
            Some("deep".to_string()),
            "shell.run",
            json!({"cmd": "echo durable", "note": "different digest"}),
        )
        .await
        .expect("matching grant starts without ask");

    tokio::time::sleep(Duration::from_millis(80)).await;
    handle.stop_run().await.expect("stop run");

    let events = read_events(&run.events_path);
    let requested_count = events
        .iter()
        .filter(|event| matches!(event.payload, EventV1::PermissionRequested(_)))
        .count();
    assert_eq!(requested_count, 1, "second matching call should not ask");
    assert!(events
        .iter()
        .any(|event| matches!(event.payload, EventV1::PermissionGrantRecorded(_))));
    assert!(events.iter().any(|event| {
        matches!(
            &event.payload,
            EventV1::ToolCallStarted(data) if data.tool_call_id == first_tool_call_id
        )
    }));
    assert!(events.iter().any(|event| {
        matches!(
            &event.payload,
            EventV1::ToolCallStarted(data) if data.tool_call_id == second_tool_call_id
        )
    }));
}

#[tokio::test]
async fn allow_always_shell_run_grant_does_not_authorize_changed_args() {
    let temp_dir = tempfile::tempdir().expect("tempdir");
    let mut config = test_config(temp_dir.path());
    config.permission_policy = PermissionPolicy::new(
        PermissionMode::Deny,
        PermissionMode::Ask,
        PermissionMode::Deny,
    )
    .with_ask_timeout_ms(1_000);

    let handle = spawn_coordinator(
        config,
        Arc::new(FakeClock::new()),
        Arc::new(DefaultRedactor::default()),
    );

    let run = handle
        .start_run("perm_allow_always_args", temp_dir.path())
        .await
        .expect("start run");

    let actor = EventActor::new(ActorKind::Supervisor, Some("agent-supervisor".to_string()));
    let first_tool_call_id = handle
        .request_tool_call(
            actor.clone(),
            Some("deep".to_string()),
            "shell.run",
            json!({"cmd": "bash", "args": ["-lc", "echo durable"]}),
        )
        .await
        .expect("request first tool call");

    tokio::time::sleep(Duration::from_millis(40)).await;
    let permission_id = read_events(&run.events_path)
        .iter()
        .find_map(|event| match &event.payload {
            EventV1::PermissionRequested(data)
                if data.tool_call_id.as_deref() == Some(first_tool_call_id.as_str()) =>
            {
                Some(data.permission_id.clone())
            }
            _ => None,
        })
        .expect("permission requested");

    handle
        .resolve_permission_with_grant_scope(
            permission_id,
            PermissionDecision::Allow,
            None,
            Some(PermissionGrantScope::Run),
        )
        .await
        .expect("resolve with durable grant");
    tokio::time::sleep(Duration::from_millis(80)).await;

    let second_tool_call_id = handle
        .request_tool_call(
            actor,
            Some("deep".to_string()),
            "shell.run",
            json!({"cmd": "bash", "args": ["-lc", "echo changed"]}),
        )
        .await
        .expect("request changed args tool call");

    tokio::time::sleep(Duration::from_millis(80)).await;
    handle.stop_run().await.expect("stop run");

    let events = read_events(&run.events_path);
    let requested_count = events
        .iter()
        .filter(|event| matches!(event.payload, EventV1::PermissionRequested(_)))
        .count();
    assert_eq!(requested_count, 2, "changed args should ask again");
    assert!(!events.iter().any(|event| {
        matches!(
            &event.payload,
            EventV1::ToolCallStarted(data) if data.tool_call_id == second_tool_call_id
        )
    }));
}

#[tokio::test]
async fn static_deny_overrides_permission_grant() {
    let temp_dir = tempfile::tempdir().expect("tempdir");
    let mut config = test_config(temp_dir.path());
    config.permission_policy = PermissionPolicy::new(
        PermissionMode::Deny,
        PermissionMode::Ask,
        PermissionMode::Deny,
    )
    .with_category_override(
        "locked",
        CategoryPermissions {
            shell: Some(PermissionMode::Deny),
            ..CategoryPermissions::default()
        },
    )
    .with_ask_timeout_ms(1_000);

    let handle = spawn_coordinator(
        config,
        Arc::new(FakeClock::new()),
        Arc::new(DefaultRedactor::default()),
    );
    let run = handle
        .start_run("static_deny", temp_dir.path())
        .await
        .expect("start run");

    let actor = EventActor::new(ActorKind::Supervisor, Some("agent-supervisor".to_string()));
    let granted_tool_call_id = handle
        .request_tool_call(
            actor.clone(),
            Some("deep".to_string()),
            "shell.run",
            json!({"cmd": "echo durable"}),
        )
        .await
        .expect("request grantable call");
    tokio::time::sleep(Duration::from_millis(40)).await;
    let permission_id = read_events(&run.events_path)
        .iter()
        .find_map(|event| match &event.payload {
            EventV1::PermissionRequested(data)
                if data.tool_call_id.as_deref() == Some(granted_tool_call_id.as_str()) =>
            {
                Some(data.permission_id.clone())
            }
            _ => None,
        })
        .expect("permission requested");
    handle
        .resolve_permission_with_grant_scope(
            permission_id,
            PermissionDecision::Allow,
            None,
            Some(PermissionGrantScope::Run),
        )
        .await
        .expect("record durable grant");
    tokio::time::sleep(Duration::from_millis(80)).await;

    let denied = handle
        .request_tool_call(
            actor,
            Some("locked".to_string()),
            "shell.run",
            json!({"cmd": "echo durable"}),
        )
        .await
        .expect_err("static deny must override durable grant");
    assert!(matches!(denied, CoordinatorError::PermissionDenied(_)));

    handle.stop_run().await.expect("stop run");
    let events = read_events(&run.events_path);
    assert!(events.iter().any(|event| {
        matches!(
            &event.payload,
            EventV1::PermissionResolved(data)
                if data.decision == crate::event::PermissionDecision::Deny
        )
    }));
    let denied_tool_started = events.iter().any(|event| {
        matches!(
            &event.payload,
            EventV1::ToolCallStarted(data) if data.tool_call_id == "toolcall_000002"
        )
    });
    assert!(!denied_tool_started);
}

#[tokio::test]
async fn permission_grant_event_does_not_persist_raw_shell_command_secret() {
    let temp_dir = tempfile::tempdir().expect("tempdir");
    let mut config = test_config(temp_dir.path());
    config.permission_policy = PermissionPolicy::new(
        PermissionMode::Deny,
        PermissionMode::Ask,
        PermissionMode::Deny,
    )
    .with_ask_timeout_ms(1_000);

    let handle = spawn_coordinator(
        config,
        Arc::new(FakeClock::new()),
        Arc::new(DefaultRedactor::default()),
    );
    let run = handle
        .start_run("perm_grant_redaction", temp_dir.path())
        .await
        .expect("start run");

    let tool_call_id = handle
        .request_tool_call(
            EventActor::new(ActorKind::Supervisor, Some("agent-supervisor".to_string())),
            Some("deep".to_string()),
            "shell.run",
            json!({"cmd": "curl -H 'Authorization: Bearer secret.value' https://example.invalid"}),
        )
        .await
        .expect("request shell call");
    tokio::time::sleep(Duration::from_millis(40)).await;
    let permission_id = read_events(&run.events_path)
        .iter()
        .find_map(|event| match &event.payload {
            EventV1::PermissionRequested(data)
                if data.tool_call_id.as_deref() == Some(tool_call_id.as_str()) =>
            {
                Some(data.permission_id.clone())
            }
            _ => None,
        })
        .expect("permission requested");

    handle
        .resolve_permission_with_grant_scope(
            permission_id,
            PermissionDecision::Allow,
            None,
            Some(PermissionGrantScope::Run),
        )
        .await
        .expect("resolve durable grant");
    tokio::time::sleep(Duration::from_millis(80)).await;
    handle.stop_run().await.expect("stop run");

    let events_body = fs::read_to_string(&run.events_path).expect("read events body");
    let grant_line = events_body
        .lines()
        .find(|line| line.contains("permission_grant_recorded"))
        .expect("grant event line");
    assert!(!grant_line.contains("secret.value"));
    assert!(!grant_line.contains("Authorization"));
    assert!(!grant_line.contains("Bearer"));
}

#[tokio::test]
async fn perm_timeout_path_denies_deterministically() {
    let temp_dir = tempfile::tempdir().expect("tempdir");
    let mut config = test_config(temp_dir.path());
    config.permission_policy = PermissionPolicy::new(
        PermissionMode::Deny,
        PermissionMode::Ask,
        PermissionMode::Deny,
    )
    .with_ask_timeout_ms(25);

    let handle = spawn_coordinator(
        config,
        Arc::new(FakeClock::new()),
        Arc::new(DefaultRedactor::default()),
    );

    let run = handle
        .start_run("perm_timeout", temp_dir.path())
        .await
        .expect("start run");

    let tool_call_id = handle
        .request_tool_call(
            EventActor::new(ActorKind::Supervisor, Some("agent-supervisor".to_string())),
            Some("deep".to_string()),
            "shell.run",
            json!({"cmd": "sleep 1"}),
        )
        .await
        .expect("request tool call");

    tokio::time::sleep(Duration::from_millis(90)).await;
    handle.stop_run().await.expect("stop run");

    let events = read_events(&run.events_path);
    assert!(events.iter().any(|event| {
        matches!(
            &event.payload,
            EventV1::PermissionResolved(data)
                if data.decision == crate::event::PermissionDecision::Deny
                    && data.reason.as_deref() == Some("permission request timed out")
        )
    }));
    assert!(events.iter().any(|event| {
        matches!(
            &event.payload,
            EventV1::ToolCallFinished(data)
                if data.tool_call_id == tool_call_id && data.status == ToolCallStatus::Failed
        )
    }));
    assert!(!events.iter().any(|event| {
        matches!(
            &event.payload,
            EventV1::ToolCallStarted(data) if data.tool_call_id == tool_call_id
        )
    }));
}

#[tokio::test]
async fn malformed_question_answer_does_not_resolve_permission() {
    let temp_dir = tempfile::tempdir().expect("tempdir");
    let config = test_config(temp_dir.path());
    let handle = spawn_coordinator(
        config,
        Arc::new(RealClock::new()),
        Arc::new(DefaultRedactor::default()),
    );
    let run = handle
        .start_run("question_validation", temp_dir.path())
        .await
        .expect("start run");

    let question_handle = handle.clone();
    let request = tokio::spawn(async move {
        question_handle
            .request_question(
                EventActor::new(ActorKind::Worker, Some("agent-worker".to_string())),
                "toolcall_question_validation",
                json!({
                    "questions": [{
                        "question": "Pick one",
                        "header": "Choice",
                        "options": [{"label": "A", "description": "Option A"}],
                    }]
                }),
            )
            .await
    });

    tokio::time::sleep(Duration::from_millis(40)).await;
    let before = read_events(&run.events_path);
    let permission_id = before
        .iter()
        .find_map(|event| match &event.payload {
            EventV1::PermissionRequested(data) if data.kind == "question" => {
                Some(data.permission_id.clone())
            }
            _ => None,
        })
        .expect("question permission requested");

    let err = handle
        .resolve_permission(
            permission_id.clone(),
            PermissionDecision::Allow,
            Some("not-json".to_string()),
        )
        .await
        .expect_err("malformed answers must be rejected");
    assert!(err.to_string().contains("invalid question answer payload"));

    assert!(
        read_events(&run.events_path).iter().all(|event| {
            !matches!(
                &event.payload,
                EventV1::PermissionResolved(data) if data.permission_id == permission_id
            )
        }),
        "permission should remain pending when answer payload is invalid"
    );

    request.abort();
    handle.stop_run().await.expect("stop run");
}

#[tokio::test]
async fn mcp_effective_identity_persists_for_direct_and_wrapper_calls() {
    let temp_dir = tempfile::tempdir().expect("tempdir");
    let mut config = test_config(temp_dir.path());
    config.permission_policy = PermissionPolicy::new(
        PermissionMode::Deny,
        PermissionMode::Allow,
        PermissionMode::Deny,
    );

    let handle = spawn_coordinator(
        config,
        Arc::new(FakeClock::new()),
        Arc::new(DefaultRedactor::default()),
    );

    let run = handle
        .start_run("mcp_identity", temp_dir.path())
        .await
        .expect("start run");

    let actor = EventActor::new(ActorKind::Supervisor, Some("agent-supervisor".to_string()));
    let direct_call_id = handle
        .request_tool_call(
            actor.clone(),
            Some("deep".to_string()),
            "mcp.fixture.echo",
            json!({"text": "hello direct"}),
        )
        .await
        .expect("request direct MCP tool call");
    let wrapper_call_id = handle
        .request_tool_call(
            actor,
            Some("deep".to_string()),
            "mcp.fixture.tool.call",
            json!({
                "tool": "echo",
                "arguments": { "text": "hello wrapper" },
            }),
        )
        .await
        .expect("request wrapper MCP tool call");

    tokio::time::sleep(Duration::from_millis(80)).await;
    handle.stop_run().await.expect("stop run");

    let events = read_events(&run.events_path);
    let direct_requested = events
        .iter()
        .find_map(|event| match &event.payload {
            EventV1::ToolCallRequested(data) if data.tool_call_id == direct_call_id => Some(data),
            _ => None,
        })
        .expect("direct requested event");
    assert_eq!(direct_requested.tool_id, "mcp.fixture.echo");
    assert_eq!(
        direct_requested
            .metadata
            .as_ref()
            .and_then(|metadata| metadata.canonical_tool_id.as_deref()),
        Some("mcp.fixture.echo")
    );
    assert_eq!(
        direct_requested
            .metadata
            .as_ref()
            .and_then(|metadata| metadata.alias_source_tool_id.as_deref()),
        None
    );

    let wrapper_requested = events
        .iter()
        .find_map(|event| match &event.payload {
            EventV1::ToolCallRequested(data) if data.tool_call_id == wrapper_call_id => Some(data),
            _ => None,
        })
        .expect("wrapper requested event");
    assert_eq!(wrapper_requested.tool_id, "mcp.fixture.tool.call");
    assert_eq!(
        wrapper_requested
            .metadata
            .as_ref()
            .and_then(|metadata| metadata.canonical_tool_id.as_deref()),
        Some("mcp.fixture.echo")
    );
    assert_eq!(
        wrapper_requested
            .metadata
            .as_ref()
            .and_then(|metadata| metadata.alias_source_tool_id.as_deref()),
        None
    );

    let wrapper_finished = events
        .iter()
        .find_map(|event| match &event.payload {
            EventV1::ToolCallFinished(data) if data.tool_call_id == wrapper_call_id => Some(data),
            _ => None,
        })
        .expect("wrapper finished event");
    assert_eq!(wrapper_finished.status, ToolCallStatus::Succeeded);
    assert_eq!(
        wrapper_finished
            .metadata
            .as_ref()
            .and_then(|metadata| metadata.canonical_tool_id.as_deref()),
        Some("mcp.fixture.echo")
    );
    assert_eq!(
        wrapper_finished
            .output_json
            .as_ref()
            .and_then(|value| value.get("payload"))
            .and_then(|value| value.get("tool"))
            .and_then(|value| value.as_str()),
        Some("echo")
    );

    let resume_plan = inspect_resume_plan(&run.run_dir);
    let direct_snapshot = resume_plan
        .tool_calls
        .get(&direct_call_id)
        .expect("direct tool snapshot");
    assert_eq!(
        direct_snapshot
            .resolved_tool_identity
            .as_ref()
            .and_then(|identity| identity.invoked_tool_id.as_deref()),
        Some("mcp.fixture.echo")
    );
    assert_eq!(
        direct_snapshot
            .resolved_tool_identity
            .as_ref()
            .and_then(|identity| identity.effective_tool_id.as_deref()),
        Some("mcp.fixture.echo")
    );
    assert_eq!(
        direct_snapshot
            .resolved_tool_identity
            .as_ref()
            .and_then(|identity| identity.canonical_tool_id.as_deref()),
        None
    );
    assert_eq!(
        direct_snapshot.lifecycle_state,
        Some(crate::event::ToolCallLifecycleState::Completed)
    );
    assert_eq!(
        direct_snapshot
            .metadata
            .as_ref()
            .and_then(|metadata| metadata.canonical_tool_id.as_deref()),
        Some("mcp.fixture.echo")
    );
    let wrapper_snapshot = resume_plan
        .tool_calls
        .get(&wrapper_call_id)
        .expect("wrapper tool snapshot");
    assert_eq!(
        wrapper_snapshot.tool_id.as_deref(),
        Some("mcp.fixture.tool.call")
    );
    assert_eq!(
        wrapper_snapshot
            .resolved_tool_identity
            .as_ref()
            .and_then(|identity| identity.invoked_tool_id.as_deref()),
        Some("mcp.fixture.tool.call")
    );
    assert_eq!(
        wrapper_snapshot
            .resolved_tool_identity
            .as_ref()
            .and_then(|identity| identity.effective_tool_id.as_deref()),
        Some("mcp.fixture.echo")
    );
    assert_eq!(
        wrapper_snapshot
            .resolved_tool_identity
            .as_ref()
            .and_then(|identity| identity.canonical_tool_id.as_deref()),
        None
    );
    assert_eq!(
        wrapper_snapshot
            .resolved_tool_identity
            .as_ref()
            .and_then(|identity| identity.alias_source_tool_id.as_deref()),
        None
    );
    assert_eq!(
        wrapper_snapshot.lifecycle_state,
        Some(crate::event::ToolCallLifecycleState::Completed)
    );
    assert_eq!(
        wrapper_snapshot
            .metadata
            .as_ref()
            .and_then(|metadata| metadata.canonical_tool_id.as_deref()),
        Some("mcp.fixture.echo")
    );
    assert_eq!(
        wrapper_snapshot
            .metadata
            .as_ref()
            .and_then(|metadata| metadata.alias_source_tool_id.as_deref()),
        None
    );
}

#[test]
fn mcp_effective_identity_uses_registered_first_class_ids_for_reserved_wrapper_names() {
    let _guard = mcp_identity_registry_test_lock()
        .lock()
        .expect("mcp identity registry test lock");
    clear_registered_mcp_server_first_class_tool_ids();
    set_registered_mcp_server_first_class_tool_ids(std::collections::BTreeMap::from([(
        "fixture".to_string(),
        std::collections::BTreeMap::from([(
            "tool.call".to_string(),
            "mcp.fixture.tool_call_2".to_string(),
        )]),
    )]));

    let wrapper_metadata = super::tool_identity_metadata(
        "mcp.fixture.tool.call",
        &json!({
            "tool": "tool.call",
            "arguments": { "text": "hello wrapper" },
        }),
    )
    .expect("wrapper MCP identity metadata");
    assert_eq!(
        wrapper_metadata.canonical_tool_id.as_deref(),
        Some("mcp.fixture.tool_call_2")
    );
    assert_eq!(wrapper_metadata.alias_source_tool_id.as_deref(), None);

    let direct_metadata = super::tool_identity_metadata(
        "mcp.fixture.tool_call_2",
        &json!({ "text": "hello direct" }),
    )
    .expect("direct MCP identity metadata");
    assert_eq!(
        direct_metadata.canonical_tool_id.as_deref(),
        Some("mcp.fixture.tool_call_2")
    );
    assert_eq!(direct_metadata.alias_source_tool_id.as_deref(), None);

    clear_registered_mcp_server_first_class_tool_ids();
}

#[test]
fn stale_tool_task_late_result_preserves_owner_actor() {
    let temp_dir = tempfile::tempdir().expect("tempdir");
    let mut config = test_config(temp_dir.path());
    config.stale_timeout_ms = 20;
    let clock = Arc::new(FakeClock::new());
    let redactor = Arc::new(DefaultRedactor::default());
    let (_command_tx, command_rx) = mpsc::channel(1);
    let (job_tx, job_rx) = mpsc::channel(1);
    let mut coordinator =
        Coordinator::new(config, clock.clone(), redactor, command_rx, job_tx, job_rx);

    let run = coordinator
        .start_run_internal("stale_owner".to_string(), temp_dir.path().to_path_buf())
        .expect("start run");
    let task_id = "task_000001".to_string();
    let queue_key = ConcurrencyKey::Tool {
        tool_id: "shell.run".to_string(),
    };
    let owner_actor = EventActor::new(ActorKind::Worker, Some("agent_000001".to_string()));
    let request_correlation_id = Some("req_000001".to_string());

    {
        let run_state = coordinator.run_state.as_mut().expect("run state");
        assert!(matches!(
            run_state
                .scheduler
                .schedule(task_id.clone(), queue_key.clone()),
            ScheduleDecision::Started(_)
        ));
        run_state.tasks.insert(
            task_id.clone(),
            TaskState {
                tool_call_id: "toolcall_000001".to_string(),
                tool_metadata: None,
                owner_actor: owner_actor.clone(),
                request_correlation_id: request_correlation_id.clone(),
                queue_key,
                state: TaskExecutionState::Running,
                cancellation_token: CancellationToken::new(),
                started_mono_ms: 0,
                last_progress_mono_ms: 0,
                last_progress_kind: JobProgressKind::Heartbeat,
                hashline_edit: None,
                respond_to: None,
            },
        );
    }

    clock.advance(25);
    coordinator
        .watchdog_tick_internal()
        .expect("detect stale tool task");
    coordinator
        .job_finished_internal(
            task_id.clone(),
            JobOutcome::Cancelled {
                reason: "job cancelled".to_string(),
            },
        )
        .expect("record late result");

    let events = read_events(&run.events_path);
    assert!(events.iter().any(|event| {
        matches!(
            &event.payload,
            EventV1::StaleDetected(data)
                if data.task_id == task_id
                    && event.actor == owner_actor
                    && event.correlation_id.as_deref() == request_correlation_id.as_deref()
        )
    }));
    assert!(events.iter().any(|event| {
        matches!(
            &event.payload,
            EventV1::TaskResultLate(data)
                if data.task_id == task_id
                    && event.actor == owner_actor
                    && event.correlation_id.as_deref() == request_correlation_id.as_deref()
        )
    }));
}

#[test]
fn restore_provider_context_uses_task_completed_summary_for_iterative_history() {
    let temp_dir = tempfile::tempdir().expect("tempdir");
    let run_id = "run_iterative_restore";
    write_restore_history_fixture(
        temp_dir.path(),
        run_id,
        &[
            restore_fixture_event(
                run_id,
                1,
                EventActor::new(ActorKind::System, Some("coordinator".to_string())),
                None,
                EventV1::RunStarted(RunStartedEvent {
                    run_name: "interactive".to_string(),
                    workspace_root: "/workspace/project".to_string(),
                }),
            ),
            restore_fixture_event(
                run_id,
                2,
                EventActor::new(ActorKind::System, Some("coordinator".to_string())),
                None,
                EventV1::AgentSpawned(AgentSpawnedEvent {
                    agent_id: "agent_000001".to_string(),
                    profile: "alpha".to_string(),
                    parent_agent_id: None,
                }),
            ),
            restore_fixture_event(
                run_id,
                3,
                EventActor::new(ActorKind::Worker, Some("agent_000001".to_string())),
                Some("req_000001"),
                EventV1::ProviderRequestStarted(ProviderRequestStartedEvent {
                    request_id: "req_000001".to_string(),
                    provider_id: "mock".to_string(),
                    model_id: "model-1".to_string(),
                    prompt_summary: "first question".to_string(),
                    request_digest: "digest-1".to_string(),
                    metadata: None,
                }),
            ),
            restore_fixture_event(
                run_id,
                4,
                EventActor::new(ActorKind::Worker, Some("agent_000001".to_string())),
                Some("req_000001"),
                EventV1::ProviderStreamDelta(crate::event::ProviderStreamDeltaEvent {
                    request_id: "req_000001".to_string(),
                    delta: "calling tool".to_string(),
                }),
            ),
            restore_fixture_event(
                run_id,
                5,
                EventActor::new(ActorKind::Worker, Some("agent_000001".to_string())),
                Some("req_000001_iter_02"),
                EventV1::ProviderRequestStarted(ProviderRequestStartedEvent {
                    request_id: "req_000001_iter_02".to_string(),
                    provider_id: "mock".to_string(),
                    model_id: "model-1".to_string(),
                    prompt_summary: "tool result follow-up".to_string(),
                    request_digest: "digest-2".to_string(),
                    metadata: None,
                }),
            ),
            restore_fixture_event(
                run_id,
                6,
                EventActor::new(ActorKind::Worker, Some("agent_000001".to_string())),
                Some("req_000001_iter_02"),
                EventV1::ProviderStreamDelta(crate::event::ProviderStreamDeltaEvent {
                    request_id: "req_000001_iter_02".to_string(),
                    delta: "final answer".to_string(),
                }),
            ),
            restore_fixture_event(
                run_id,
                7,
                EventActor::new(ActorKind::Worker, Some("agent_000001".to_string())),
                Some("req_000001"),
                EventV1::TaskCompleted(TaskCompletedEvent {
                    task_id: "task_000001".to_string(),
                    result_summary: "final answer".to_string(),
                    result_digest: "digest-task".to_string(),
                    metadata: None,
                }),
            ),
            restore_fixture_event(
                run_id,
                8,
                EventActor::new(ActorKind::System, Some("coordinator".to_string())),
                None,
                EventV1::RunFinished(RunFinishedEvent {
                    summary: "done".to_string(),
                }),
            ),
        ],
    );

    let restored = restore_provider_context_from_history(temp_dir.path(), run_id)
        .expect("restore provider context");
    let turns = restored
        .get("agent_000001")
        .expect("agent should have restored history");
    assert_eq!(turns.preserved_turns.len(), 1);
    assert_eq!(turns.preserved_turns[0].user_prompt, "first question");
    assert_eq!(turns.preserved_turns[0].assistant_response, "final answer");
}

#[test]
fn proactive_compaction_writes_checkpoint_artifact_and_updates_provider_context() {
    let temp_dir = tempfile::tempdir().expect("tempdir");
    let clock = FakeClock::new();
    let redactor = DefaultRedactor::default();
    let mut run_state = test_run_state(temp_dir.path(), "run_compaction_proactive");
    run_state.recorded_runtime_context = Some(compaction_runtime_context());
    run_state.provider_context_by_agent.insert(
        "agent_000001".to_string(),
        ProviderContext::from_turns(vec![
            long_turn("first question", 'A'),
            long_turn("second question", 'B'),
        ]),
    );

    let updated = compact_provider_context(
        &clock,
        &redactor,
        &mut run_state,
        &ProviderCompactionTrigger {
            agent_id: "agent_000001".to_string(),
            profile_name: "alpha".to_string(),
            model_ref: "default:model-1".to_string(),
            provider_id: Some("default".to_string()),
            model_id: Some("model-1".to_string()),
            through_request_id: Some("req_000002".to_string()),
            trigger_reason: "proactive".to_string(),
            tokens_before: Some(3_900),
        },
        &CompactionRuntimeConfig::default(),
        &super::CompactionSummaryDecision::deterministic(&ProviderCompactionTrigger {
            agent_id: "agent_000001".to_string(),
            profile_name: "alpha".to_string(),
            model_ref: "default:model-1".to_string(),
            provider_id: Some("default".to_string()),
            model_id: Some("model-1".to_string()),
            through_request_id: Some("req_000002".to_string()),
            trigger_reason: "proactive".to_string(),
            tokens_before: Some(3_900),
        }),
    )
    .expect("proactive compaction should succeed")
    .expect("proactive compaction should write a checkpoint");

    assert!(updated.updated_context.compacted_summary.is_some());
    assert_eq!(updated.updated_context.preserved_turns.len(), 1);
    assert_eq!(
        updated.updated_context.preserved_turns[0].user_prompt,
        "second question"
    );

    let events = read_events(&run_state.info.events_path);
    assert!(events
        .iter()
        .any(|event| matches!(event.payload, EventV1::CompactionRequested(_))));
    let written = events
        .iter()
        .find_map(|event| match &event.payload {
            EventV1::CompactionWritten(payload) => Some(payload.clone()),
            _ => None,
        })
        .expect("compaction written event");
    assert!(events
        .iter()
        .any(|event| matches!(event.payload, EventV1::CompactionApplied(_))));
    assert!(written.tokens_before_estimate.is_some());
    assert!(written.tokens_after_estimate.is_some());
    assert!(written.tokens_after_estimate < written.tokens_before_estimate);
    assert_eq!(written.compacted_turns, Some(1));

    let checkpoint_path = run_state.info.run_dir.join(&written.artifact_path);
    let checkpoint_body = fs::read_to_string(&checkpoint_path).expect("read checkpoint artifact");
    let checkpoint: ProviderContextCheckpoint =
        serde_json::from_str(&checkpoint_body).expect("parse checkpoint artifact");
    assert_eq!(checkpoint.metadata.agent_id, "agent_000001");
    assert_eq!(
        checkpoint.metadata.tokens_before_estimate,
        written.tokens_before_estimate
    );
    assert_eq!(
        checkpoint.metadata.tokens_after_estimate,
        written.tokens_after_estimate
    );
    assert_eq!(
        checkpoint.metadata.summary_tokens_estimate,
        written.summary_tokens_estimate
    );
    assert!(checkpoint.metadata.reduction_tokens_estimate.is_some());
    assert_eq!(checkpoint.recent_turns.len(), 1);
    assert_eq!(checkpoint.facts.compacted_turns.len(), 1);
    assert_eq!(
        checkpoint.facts.compacted_turns[0].user_excerpt,
        "first question"
    );
    assert_eq!(
        checkpoint
            .tail_boundary
            .as_ref()
            .map(|boundary| boundary.mode.as_str()),
        Some("whole_turn_tail")
    );
    assert_eq!(
        checkpoint
            .summary_source
            .as_ref()
            .map(|source| source.strategy.as_str()),
        Some("deterministic_rolling_summary")
    );
    assert_eq!(
        checkpoint
            .timeline_entry
            .as_ref()
            .map(|entry| entry.entry_type.as_str()),
        Some("proactive_compaction")
    );
    for heading in [
        "## Goal",
        "## Constraints & Preferences",
        "## Progress",
        "### Done",
        "### In Progress",
        "### Blocked",
        "## Key Decisions",
        "## Next Steps",
        "## Critical Context",
        "## Source Facts",
        "## Relevant Files / Artifacts",
    ] {
        assert!(
            checkpoint.summary.contains(heading),
            "summary missing structured heading {heading}:\n{}",
            checkpoint.summary
        );
    }
    assert!(checkpoint.summary.contains("first question"));
}

#[test]
fn proactive_compaction_records_pruned_tool_artifacts_for_compacted_turns() {
    let temp_dir = tempfile::tempdir().expect("tempdir");
    let clock = FakeClock::new();
    let redactor = DefaultRedactor::default();
    let first_answer = 'A'.to_string().repeat(6_000);
    let second_answer = 'B'.to_string().repeat(6_000);
    write_restore_history_fixture(
        temp_dir.path(),
        "run_compaction_pruned_artifacts",
        &[
            restore_fixture_event(
                "run_compaction_pruned_artifacts",
                1,
                EventActor::new(ActorKind::System, Some("coordinator".to_string())),
                None,
                EventV1::RunStarted(RunStartedEvent {
                    run_name: "interactive".to_string(),
                    workspace_root: "/workspace/project".to_string(),
                }),
            ),
            restore_fixture_event(
                "run_compaction_pruned_artifacts",
                2,
                EventActor::new(ActorKind::User, None),
                None,
                EventV1::UserMessageSubmitted(UserMessageSubmittedEvent {
                    request_id: "req_000001".to_string(),
                    text: "first question".to_string(),
                }),
            ),
            restore_fixture_event(
                "run_compaction_pruned_artifacts",
                3,
                EventActor::new(ActorKind::Worker, Some("agent_000001".to_string())),
                Some("req_000001"),
                EventV1::ProviderRequestStarted(ProviderRequestStartedEvent {
                    request_id: "req_000001".to_string(),
                    provider_id: "default".to_string(),
                    model_id: "model-1".to_string(),
                    prompt_summary: "first question".to_string(),
                    request_digest: "digest-1".to_string(),
                    metadata: None,
                }),
            ),
            restore_fixture_event(
                "run_compaction_pruned_artifacts",
                4,
                EventActor::new(ActorKind::System, Some("coordinator".to_string())),
                Some("req_000001"),
                EventV1::ArtifactWritten(ArtifactWrittenEvent {
                    path: "artifacts/toolcalls/toolcall_000001/result.txt".to_string(),
                    digest: "digest-artifact-1".to_string(),
                    bytes: 42,
                    tool_call_id: Some("toolcall_000001".to_string()),
                    tool_metadata: None,
                    metadata: std::collections::BTreeMap::new(),
                }),
            ),
            restore_fixture_event(
                "run_compaction_pruned_artifacts",
                5,
                EventActor::new(ActorKind::Worker, Some("agent_000001".to_string())),
                Some("req_000001"),
                EventV1::TaskCompleted(TaskCompletedEvent {
                    task_id: "task_000001".to_string(),
                    result_summary: first_answer.clone(),
                    result_digest: "digest-task-1".to_string(),
                    metadata: None,
                }),
            ),
            restore_fixture_event(
                "run_compaction_pruned_artifacts",
                6,
                EventActor::new(ActorKind::User, None),
                None,
                EventV1::UserMessageSubmitted(UserMessageSubmittedEvent {
                    request_id: "req_000002".to_string(),
                    text: "second question".to_string(),
                }),
            ),
            restore_fixture_event(
                "run_compaction_pruned_artifacts",
                7,
                EventActor::new(ActorKind::Worker, Some("agent_000001".to_string())),
                Some("req_000002"),
                EventV1::ProviderRequestStarted(ProviderRequestStartedEvent {
                    request_id: "req_000002".to_string(),
                    provider_id: "default".to_string(),
                    model_id: "model-1".to_string(),
                    prompt_summary: "second question".to_string(),
                    request_digest: "digest-2".to_string(),
                    metadata: None,
                }),
            ),
            restore_fixture_event(
                "run_compaction_pruned_artifacts",
                8,
                EventActor::new(ActorKind::Worker, Some("agent_000001".to_string())),
                Some("req_000002"),
                EventV1::TaskCompleted(TaskCompletedEvent {
                    task_id: "task_000002".to_string(),
                    result_summary: second_answer.clone(),
                    result_digest: "digest-task-2".to_string(),
                    metadata: None,
                }),
            ),
        ],
    );
    let mut run_state = test_run_state(temp_dir.path(), "run_compaction_pruned_artifacts");
    run_state.recorded_runtime_context = Some(compaction_runtime_context());
    run_state.provider_context_by_agent.insert(
        "agent_000001".to_string(),
        ProviderContext::from_turns(vec![
            ProviderConversationTurn {
                user_prompt: "first question".to_string(),
                assistant_response: first_answer.clone(),
                ..ProviderConversationTurn::default()
            },
            ProviderConversationTurn {
                user_prompt: "second question".to_string(),
                assistant_response: second_answer.clone(),
                ..ProviderConversationTurn::default()
            },
        ]),
    );
    run_state.next_event_seq = 9;

    let updated = compact_provider_context(
        &clock,
        &redactor,
        &mut run_state,
        &ProviderCompactionTrigger {
            agent_id: "agent_000001".to_string(),
            profile_name: "alpha".to_string(),
            model_ref: "default:model-1".to_string(),
            provider_id: Some("default".to_string()),
            model_id: Some("model-1".to_string()),
            through_request_id: Some("req_000002".to_string()),
            trigger_reason: "proactive".to_string(),
            tokens_before: Some(3_900),
        },
        &CompactionRuntimeConfig::default(),
        &super::CompactionSummaryDecision::deterministic(&ProviderCompactionTrigger {
            agent_id: "agent_000001".to_string(),
            profile_name: "alpha".to_string(),
            model_ref: "default:model-1".to_string(),
            provider_id: Some("default".to_string()),
            model_id: Some("model-1".to_string()),
            through_request_id: Some("req_000002".to_string()),
            trigger_reason: "proactive".to_string(),
            tokens_before: Some(3_900),
        }),
    )
    .expect("proactive compaction should succeed")
    .expect("proactive compaction should write a checkpoint");

    assert!(updated.updated_context.compacted_summary.is_some());
    let events = read_events(&run_state.info.events_path);
    let written = events
        .iter()
        .find_map(|event| match &event.payload {
            EventV1::CompactionWritten(payload) => Some(payload.clone()),
            _ => None,
        })
        .expect("compaction written event");
    let checkpoint_path = run_state.info.run_dir.join(&written.artifact_path);
    let checkpoint_body = fs::read_to_string(&checkpoint_path).expect("read checkpoint artifact");
    let checkpoint: ProviderContextCheckpoint =
        serde_json::from_str(&checkpoint_body).expect("parse checkpoint artifact");
    assert_eq!(checkpoint.pruned_tool_artifacts.len(), 1);
    assert_eq!(
        checkpoint.pruned_tool_artifacts[0].path,
        "artifacts/toolcalls/toolcall_000001/result.txt"
    );
    assert_eq!(
        checkpoint.pruned_tool_artifacts[0].digest.as_deref(),
        Some("digest-artifact-1")
    );
    assert!(checkpoint
        .summary
        .contains("artifacts/toolcalls/toolcall_000001/result.txt"));
}

#[test]
fn provider_context_checkpoint_deserializes_older_turn_shape() {
    let body = r#"{
        "checkpoint_id": "checkpoint_legacy",
        "agent_id": "agent_000001",
        "run_id": "run_legacy",
        "through_seq": 7,
        "summary": "legacy summary",
        "recent_turns": [
            {
                "user_prompt": "legacy question",
                "assistant_response": "legacy answer"
            }
        ]
    }"#;

    let checkpoint: ProviderContextCheckpoint =
        serde_json::from_str(body).expect("legacy checkpoint should deserialize");

    assert_eq!(checkpoint.metadata.tokens_before_estimate, None);
    assert_eq!(checkpoint.metadata.tokens_after_estimate, None);
    assert_eq!(checkpoint.recent_turns.len(), 1);
    assert_eq!(checkpoint.recent_turns[0].request_id, None);
    assert_eq!(checkpoint.recent_turns[0].artifacts, Vec::new());
}

#[test]
fn repeated_compaction_updates_existing_summary_without_legacy_append_format() {
    let summary = build_provider_context_summary(
        Some("## Goal\n- Keep existing constraints\n## Next Steps\n1. Continue"),
        &[ProviderConversationTurn {
            user_prompt: "new compacted question".to_string(),
            assistant_response: "new compacted answer".to_string(),
            ..ProviderConversationTurn::default()
        }],
        &[],
        &ProviderCompactionFacts::default(),
        &ProviderCompactionTailBoundary {
            mode: "whole_turn_tail".to_string(),
            preserved_turns: 1,
            preserved_tokens_estimate: 10,
            preserved_from_request_id: None,
            preserved_from_seq: None,
            note: None,
        },
        &ProviderCompactionSummarySource {
            strategy: "deterministic_rolling_summary".to_string(),
            model_ref: "default:model-1".to_string(),
            provider_id: Some("default".to_string()),
            model_id: Some("model-1".to_string()),
            reasoning_effort: None,
            text_verbosity: None,
            previous_summary_used: true,
            model_backed: false,
            deterministic_fallback: true,
        },
    );

    assert!(summary.contains("## Goal"));
    assert!(!summary.contains("Previous Summary"));
    assert!(summary.contains("Keep existing constraints"));
    assert!(summary.contains("new compacted question"));
    assert!(!summary.contains("Earlier checkpoint summary:"));
}

#[test]
fn compaction_summary_override_uses_explicit_hook_prefix_only() {
    let batch = HookExecutionBatch {
        hook_executions: vec![
            HookExecutionMetadata {
                hook_name: "ignored".to_string(),
                status: HookExecutionStatus::Succeeded,
                hook_event: Some("compaction_requested".to_string()),
                command_digest: None,
                output_digest: None,
                output_summary: Some("ordinary hook output".to_string()),
                duration_ms: Some(1),
            },
            HookExecutionMetadata {
                hook_name: "summary".to_string(),
                status: HookExecutionStatus::Succeeded,
                hook_event: Some("compaction_requested".to_string()),
                command_digest: None,
                output_digest: None,
                output_summary: Some("compaction_summary: custom compacted recap".to_string()),
                duration_ms: Some(1),
            },
        ],
        critical_failure: None,
    };

    assert_eq!(
        compaction_summary_override_from_hooks(&batch).as_deref(),
        Some("custom compacted recap")
    );
}

#[test]
fn restore_provider_context_from_history_uses_checkpoint_then_replays_post_checkpoint_turns() {
    let temp_dir = tempfile::tempdir().expect("tempdir");
    let run_id = "run_restore_checkpointed_context";
    let run_dir = temp_dir.path().join(run_id);
    let checkpoint_rel = "artifacts/compactions/agent_000001/checkpoint_000010.json".to_string();
    let checkpoint_path = run_dir.join(&checkpoint_rel);
    fs::create_dir_all(checkpoint_path.parent().expect("checkpoint parent"))
        .expect("create checkpoint directory");
    fs::write(
        &checkpoint_path,
        serde_json::to_string_pretty(&ProviderContextCheckpoint {
            metadata: ProviderContextCheckpointMetadata {
                checkpoint_id: "checkpoint_000010".to_string(),
                agent_id: "agent_000001".to_string(),
                run_id: run_id.to_string(),
                through_seq: 9,
                through_request_id: Some("req_000002".to_string()),
                provider_id: Some("default".to_string()),
                model_id: Some("model-1".to_string()),
                tokens_before: Some(3_900),
                tokens_before_estimate: None,
                tokens_after_estimate: None,
                summary_tokens_estimate: None,
                compacted_turns: None,
                preserved_turns: None,
                reduction_tokens_estimate: None,
                reduction_percent_estimate: None,
                trigger_reason: Some("proactive".to_string()),
            },
            summary: "Earlier checkpoint summary".to_string(),
            recent_turns: vec![ProviderConversationTurn {
                user_prompt: "second question".to_string(),
                assistant_response: "second answer".to_string(),
                ..ProviderConversationTurn::default()
            }],
            pruned_tool_artifacts: Vec::new(),
            facts: ProviderCompactionFacts::default(),
            tail_boundary: None,
            summary_source: None,
            timeline_entry: None,
        })
        .expect("serialize checkpoint"),
    )
    .expect("write checkpoint artifact");

    write_restore_history_fixture(
        temp_dir.path(),
        run_id,
        &[
            restore_fixture_event(
                run_id,
                1,
                EventActor::new(ActorKind::System, Some("coordinator".to_string())),
                None,
                EventV1::RunStarted(RunStartedEvent {
                    run_name: "interactive".to_string(),
                    workspace_root: "/workspace/project".to_string(),
                }),
            ),
            restore_fixture_event(
                run_id,
                2,
                EventActor::new(ActorKind::Worker, Some("agent_000001".to_string())),
                Some("req_000001"),
                EventV1::ProviderRequestStarted(ProviderRequestStartedEvent {
                    request_id: "req_000001".to_string(),
                    provider_id: "default".to_string(),
                    model_id: "model-1".to_string(),
                    prompt_summary: "first question".to_string(),
                    request_digest: "digest-1".to_string(),
                    metadata: None,
                }),
            ),
            restore_fixture_event(
                run_id,
                3,
                EventActor::new(ActorKind::Worker, Some("agent_000001".to_string())),
                Some("req_000001"),
                EventV1::ProviderStreamDelta(crate::event::ProviderStreamDeltaEvent {
                    request_id: "req_000001".to_string(),
                    delta: "first answer".to_string(),
                }),
            ),
            restore_fixture_event(
                run_id,
                4,
                EventActor::new(ActorKind::Worker, Some("agent_000001".to_string())),
                Some("req_000001"),
                EventV1::TaskCompleted(TaskCompletedEvent {
                    task_id: "task_000001".to_string(),
                    result_summary: "first answer".to_string(),
                    result_digest: "digest-task-1".to_string(),
                    metadata: None,
                }),
            ),
            restore_fixture_event(
                run_id,
                5,
                EventActor::new(ActorKind::User, None),
                None,
                EventV1::UserMessageSubmitted(crate::event::UserMessageSubmittedEvent {
                    request_id: "req_000002".to_string(),
                    text: "second question".to_string(),
                }),
            ),
            restore_fixture_event(
                run_id,
                6,
                EventActor::new(ActorKind::Worker, Some("agent_000001".to_string())),
                Some("req_000002"),
                EventV1::ProviderRequestStarted(ProviderRequestStartedEvent {
                    request_id: "req_000002".to_string(),
                    provider_id: "default".to_string(),
                    model_id: "model-1".to_string(),
                    prompt_summary: "second question".to_string(),
                    request_digest: "digest-2".to_string(),
                    metadata: None,
                }),
            ),
            restore_fixture_event(
                run_id,
                7,
                EventActor::new(ActorKind::Worker, Some("agent_000001".to_string())),
                Some("req_000002"),
                EventV1::ProviderStreamDelta(crate::event::ProviderStreamDeltaEvent {
                    request_id: "req_000002".to_string(),
                    delta: "second answer".to_string(),
                }),
            ),
            restore_fixture_event(
                run_id,
                8,
                EventActor::new(ActorKind::Worker, Some("agent_000001".to_string())),
                Some("req_000002"),
                EventV1::TaskCompleted(TaskCompletedEvent {
                    task_id: "task_000002".to_string(),
                    result_summary: "second answer".to_string(),
                    result_digest: "digest-task-2".to_string(),
                    metadata: None,
                }),
            ),
            restore_fixture_event(
                run_id,
                9,
                EventActor::new(ActorKind::System, Some("coordinator".to_string())),
                None,
                EventV1::CompactionWritten(CompactionWrittenEvent {
                    checkpoint_id: "checkpoint_000010".to_string(),
                    agent_id: "agent_000001".to_string(),
                    artifact_path: checkpoint_rel.clone(),
                    artifact_digest: Some("digest-checkpoint".to_string()),
                    artifact_bytes: 512,
                    trigger_reason: "proactive".to_string(),
                    through_seq: 9,
                    through_request_id: Some("req_000002".to_string()),
                    provider_id: Some("default".to_string()),
                    model_id: Some("model-1".to_string()),
                    tokens_before: Some(3_900),
                    tokens_before_estimate: None,
                    tokens_after_estimate: None,
                    summary_tokens_estimate: None,
                    compacted_turns: None,
                    reduction_tokens_estimate: None,
                    reduction_percent_estimate: None,
                    preserved_turns: 1,
                }),
            ),
            restore_fixture_event(
                run_id,
                10,
                EventActor::new(ActorKind::System, Some("coordinator".to_string())),
                None,
                EventV1::CompactionApplied(CompactionAppliedEvent {
                    checkpoint_id: "checkpoint_000010".to_string(),
                    agent_id: "agent_000001".to_string(),
                    through_seq: 9,
                    through_request_id: Some("req_000002".to_string()),
                    tokens_before_estimate: None,
                    tokens_after_estimate: None,
                    summary_tokens_estimate: None,
                    compacted_turns: None,
                    preserved_turns: None,
                    reduction_tokens_estimate: None,
                    reduction_percent_estimate: None,
                }),
            ),
            restore_fixture_event(
                run_id,
                11,
                EventActor::new(ActorKind::User, None),
                None,
                EventV1::UserMessageSubmitted(crate::event::UserMessageSubmittedEvent {
                    request_id: "req_000003".to_string(),
                    text: "third question".to_string(),
                }),
            ),
            restore_fixture_event(
                run_id,
                12,
                EventActor::new(ActorKind::Worker, Some("agent_000001".to_string())),
                Some("req_000003"),
                EventV1::ProviderRequestStarted(ProviderRequestStartedEvent {
                    request_id: "req_000003".to_string(),
                    provider_id: "default".to_string(),
                    model_id: "model-1".to_string(),
                    prompt_summary: "third question".to_string(),
                    request_digest: "digest-3".to_string(),
                    metadata: None,
                }),
            ),
            restore_fixture_event(
                run_id,
                13,
                EventActor::new(ActorKind::Worker, Some("agent_000001".to_string())),
                Some("req_000003"),
                EventV1::ProviderStreamDelta(crate::event::ProviderStreamDeltaEvent {
                    request_id: "req_000003".to_string(),
                    delta: "third answer".to_string(),
                }),
            ),
            restore_fixture_event(
                run_id,
                14,
                EventActor::new(ActorKind::Worker, Some("agent_000001".to_string())),
                Some("req_000003"),
                EventV1::TaskCompleted(TaskCompletedEvent {
                    task_id: "task_000003".to_string(),
                    result_summary: "third answer".to_string(),
                    result_digest: "digest-task-3".to_string(),
                    metadata: None,
                }),
            ),
        ],
    );

    let restored = restore_provider_context_from_history(temp_dir.path(), run_id)
        .expect("restore checkpointed provider context");
    let context = restored
        .get("agent_000001")
        .expect("checkpointed agent context");
    assert_eq!(
        context.compacted_summary.as_deref(),
        Some("Earlier checkpoint summary")
    );
    assert_eq!(context.preserved_turns.len(), 2);
    assert_eq!(context.preserved_turns[0].user_prompt, "second question");
    assert_eq!(context.preserved_turns[1].user_prompt, "third question");
}

#[test]
fn restore_provider_context_from_history_rejects_checkpoint_metadata_mismatch() {
    let temp_dir = tempfile::tempdir().expect("tempdir");
    let run_id = "run_restore_checkpoint_metadata_mismatch";
    let run_dir = temp_dir.path().join(run_id);
    let checkpoint_rel = "artifacts/compactions/agent_000001/checkpoint_000010.json".to_string();
    let checkpoint_path = run_dir.join(&checkpoint_rel);
    fs::create_dir_all(checkpoint_path.parent().expect("checkpoint parent"))
        .expect("create checkpoint directory");
    fs::write(
        &checkpoint_path,
        serde_json::to_string_pretty(&ProviderContextCheckpoint {
            metadata: ProviderContextCheckpointMetadata {
                checkpoint_id: "checkpoint_000010".to_string(),
                agent_id: "agent_000001".to_string(),
                run_id: "wrong_run".to_string(),
                through_seq: 8,
                through_request_id: Some("req_000002".to_string()),
                provider_id: Some("default".to_string()),
                model_id: Some("model-1".to_string()),
                tokens_before: Some(3_900),
                tokens_before_estimate: None,
                tokens_after_estimate: None,
                summary_tokens_estimate: None,
                compacted_turns: None,
                preserved_turns: None,
                reduction_tokens_estimate: None,
                reduction_percent_estimate: None,
                trigger_reason: Some("proactive".to_string()),
            },
            summary: "Earlier checkpoint summary".to_string(),
            recent_turns: vec![ProviderConversationTurn {
                user_prompt: "second question".to_string(),
                assistant_response: "second answer".to_string(),
                ..ProviderConversationTurn::default()
            }],
            pruned_tool_artifacts: Vec::new(),
            facts: ProviderCompactionFacts::default(),
            tail_boundary: None,
            summary_source: None,
            timeline_entry: None,
        })
        .expect("serialize checkpoint"),
    )
    .expect("write checkpoint artifact");

    write_restore_history_fixture(
        temp_dir.path(),
        run_id,
        &[
            restore_fixture_event(
                run_id,
                1,
                EventActor::new(ActorKind::System, Some("coordinator".to_string())),
                None,
                EventV1::RunStarted(RunStartedEvent {
                    run_name: "interactive".to_string(),
                    workspace_root: "/workspace/project".to_string(),
                }),
            ),
            restore_fixture_event(
                run_id,
                2,
                EventActor::new(ActorKind::System, Some("coordinator".to_string())),
                None,
                EventV1::CompactionWritten(CompactionWrittenEvent {
                    checkpoint_id: "checkpoint_000010".to_string(),
                    agent_id: "agent_000001".to_string(),
                    artifact_path: checkpoint_rel,
                    artifact_digest: Some("digest-checkpoint".to_string()),
                    artifact_bytes: 512,
                    trigger_reason: "proactive".to_string(),
                    through_seq: 9,
                    through_request_id: Some("req_000002".to_string()),
                    provider_id: Some("default".to_string()),
                    model_id: Some("model-1".to_string()),
                    tokens_before: Some(3_900),
                    tokens_before_estimate: None,
                    tokens_after_estimate: None,
                    summary_tokens_estimate: None,
                    compacted_turns: None,
                    reduction_tokens_estimate: None,
                    reduction_percent_estimate: None,
                    preserved_turns: 1,
                }),
            ),
            restore_fixture_event(
                run_id,
                3,
                EventActor::new(ActorKind::System, Some("coordinator".to_string())),
                None,
                EventV1::CompactionApplied(CompactionAppliedEvent {
                    checkpoint_id: "checkpoint_000010".to_string(),
                    agent_id: "agent_000001".to_string(),
                    through_seq: 9,
                    through_request_id: Some("req_000002".to_string()),
                    tokens_before_estimate: None,
                    tokens_after_estimate: None,
                    summary_tokens_estimate: None,
                    compacted_turns: None,
                    preserved_turns: None,
                    reduction_tokens_estimate: None,
                    reduction_percent_estimate: None,
                }),
            ),
        ],
    );

    let err = restore_provider_context_from_history(temp_dir.path(), run_id)
        .expect_err("restore should reject mismatched checkpoint metadata");
    assert!(matches!(err, CoordinatorError::ResumeRestoreFailed { .. }));
}

fn compaction_profile_config() -> crate::config::HarnessConfig {
    load_config_from_str(
        r#"
        {
          providers: {
            default: {
              type: "openai_compatible",
              base_url: "http://127.0.0.1:8317/v1",
              api_key: "test-key",
              models: {
                "model-1": {
                  display_name: "Model 1",
                  max_input_tokens: 4096,
                  max_output_tokens: 1024,
                  metadata: {
                    context_window_tokens: 4096
                  }
                }
              }
            }
          },
          agents: {
            alpha: {
              description: "Alpha",
              model_ref: "default:model-1",
              tools: ["read"]
            }
          },
          permissions: {
            defaults: {
              edit: "deny",
              shell: "deny",
              network: "deny"
            }
          },
          runtime: {
            background_tasks: {
              default_concurrency: 1,
              provider_concurrency: 1,
              model_concurrency: 1,
              stale_timeout_ms: 1000,
              message_staleness_timeout_ms: 1000
            },
            session_dir: ".agent-harness/sessions"
          },
          integrations: {
            remote_search: {
              endpoint: "https://mcp.exa.ai/mcp"
            }
          }
        }
        "#,
    )
    .expect("parse compaction metadata config")
}

fn compaction_runtime_context() -> RecordedRuntimeContext {
    resolve_profile_model_metadata(&compaction_profile_config(), "alpha")
        .expect("resolve compaction runtime context")
        .into()
}

fn test_run_state(session_dir: &Path, run_id: &str) -> RunState {
    let event_store =
        Arc::new(JsonlFileEventStore::open(session_dir, run_id, true).expect("open event store"));
    let run_dir = session_dir.join(run_id);
    let artifacts_dir = run_dir.join("artifacts");
    fs::create_dir_all(&artifacts_dir).expect("create artifacts dir");
    RunState {
        info: RunInfo {
            run_id: run_id.to_string(),
            run_name: "interactive".to_string(),
            workspace_root: Path::new("/workspace/project").to_path_buf(),
            run_dir,
            artifacts_dir,
            events_path: event_store.file_path().to_path_buf(),
        },
        event_store,
        next_event_seq: 1,
        next_agent_id: 1,
        next_tool_call_id: 1,
        next_task_id: 1,
        next_provider_request_id: 1,
        next_permission_id: 1,
        agents: std::collections::BTreeMap::new(),
        provider_context_by_agent: std::collections::BTreeMap::new(),
        tasks: std::collections::BTreeMap::new(),
        task_hook_state: std::collections::BTreeMap::new(),
        agent_hook_state: std::collections::BTreeMap::new(),
        subagent_parent_by_id: std::collections::BTreeMap::new(),
        pending_permissions: std::collections::BTreeMap::new(),
        active_permission_grants: crate::perm::PermissionGrantSet::default(),
        cancelled_running_tasks: std::collections::BTreeSet::new(),
        pending_agent_turn_messages: std::collections::BTreeMap::new(),
        queued_agent_turns: std::collections::BTreeMap::new(),
        running_agent_turns: std::collections::BTreeMap::new(),
        scheduler: Scheduler::new(SchedulerLimits {
            provider_model: 1,
            tool: 1,
        }),
        recorded_runtime_context: None,
        allow_initial_runtime_context_recording: false,
        shutdown_token: CancellationToken::new(),
    }
}

fn long_turn(prompt: &str, fill: char) -> ProviderConversationTurn {
    ProviderConversationTurn {
        user_prompt: prompt.to_string(),
        assistant_response: fill.to_string().repeat(6_000),
        ..ProviderConversationTurn::default()
    }
}

fn write_restore_history_fixture(session_dir: &Path, run_id: &str, events: &[EventEnvelopeV1]) {
    let run_dir = session_dir.join(run_id);
    fs::create_dir_all(&run_dir).expect("create run directory");

    let mut body = String::new();
    for event in events {
        let line = serde_json::to_string(event).expect("serialize event");
        body.push_str(&line);
        body.push('\n');
    }

    fs::write(run_dir.join("events.jsonl"), body).expect("write events");
}

fn restore_fixture_event(
    run_id: &str,
    seq: u64,
    actor: EventActor,
    correlation_id: Option<&str>,
    payload: EventV1,
) -> EventEnvelopeV1 {
    EventEnvelopeV1 {
        schema_version: SCHEMA_VERSION,
        event_id: format!("evt-{seq:04}"),
        seq,
        run_id: run_id.to_string(),
        mono_ms: seq,
        ts: None,
        actor,
        correlation_id: correlation_id.map(str::to_string),
        causation_id: None,
        stream_key: Some(format!("run:{run_id}")),
        payload,
    }
}

fn read_events(path: &Path) -> Vec<EventEnvelopeV1> {
    let text = fs::read_to_string(path).expect("read events");
    text.lines()
        .map(|line| serde_json::from_str::<EventEnvelopeV1>(line).expect("valid event"))
        .collect()
}
