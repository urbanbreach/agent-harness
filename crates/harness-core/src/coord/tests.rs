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
    ProviderConversationTurn, ProviderConversationTurnStatus, ProviderFileOperationFact,
};
use crate::clock::{FakeClock, RealClock};
use crate::config::{
    clear_registered_mcp_server_first_class_tool_ids, load_config_from_str,
    resolve_profile_model_metadata, set_registered_mcp_server_first_class_tool_ids,
    CategoryPermissions, CompactionRuntimeConfig, PermissionMode,
};
use crate::event::{
    ActorKind, AgentSpawnedEvent, ArtifactWrittenEvent, CompactionAppliedEvent,
    CompactionWrittenEvent, EditAppliedEvent, EventActor, EventArtifactRef, EventEnvelopeV1,
    EventV1, HookExecutionMetadata, HookExecutionStatus, ProviderRequestFinishedEvent,
    ProviderRequestStartedEvent, ProviderStreamDeltaEvent, RunFinishedEvent, RunStartedEvent,
    TaskCancelledEvent, TaskCompletedEvent, TaskScheduleState, TaskScheduledEvent,
    ToolCallFinishedEvent, ToolCallMetadata, ToolCallRequestedEvent, ToolCallStatus,
    ToolIdentityMetadata, UserMessageSubmittedEvent, SCHEMA_VERSION,
};
use crate::perm::{PermissionDecision, PermissionGrantScope, PermissionPolicy};
use crate::proj::{inspect_resume_plan, RecordedRuntimeContext};
use crate::redact::DefaultRedactor;
use crate::sched::{ConcurrencyKey, ScheduleDecision, Scheduler, SchedulerLimits};
use crate::store::JsonlFileEventStore;
use crate::tool::{Tool, ToolCapability, ToolContext, ToolError, ToolRegistry, ToolResult};

use super::{
    build_model_compaction_prompt, build_provider_context_summary, compact_provider_context,
    compaction_summary_override_from_hooks, mark_failed_terminal_compaction_attempt,
    provider_context_summary_required_headings, restore_provider_context_from_history,
    spawn_coordinator, validate_model_compaction_summary, Coordinator, CoordinatorConfig,
    CoordinatorError, FailedTerminalCompactionRequest, HookExecutionBatch, JobOutcome,
    JobProgressKind, ProviderCompactionTrigger, ProviderContextCompactionPlan, RunInfo, RunState,
    TaskExecutionState, TaskState,
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
async fn permission_rule_bash_selector_is_enforced_at_tool_call_site() {
    let temp_dir = tempfile::tempdir().expect("tempdir");
    let parsed = load_config_from_str(
        r#"
        {
          provider: {
            default: {
              type: "openai_compatible",
              base_url: "http://127.0.0.1:8317/v1",
              api_key: "test-key",
              models: {
                "gpt-4o-mini": { name: "GPT-4o mini" }
              }
            }
          },
          model: "default/gpt-4o-mini",
          agent: {
            deep: {
              system_prompt: "Deep work",
              tools: ["shell.run"]
            }
          },
          default_agent: "deep",
          permission: {
            bash: {
              "git status": "deny",
              "*": "allow"
            },
            edit: "allow",
            question: "allow",
            task: "allow",
            webfetch: "allow",
            websearch: "allow",
            codesearch: "allow",
            lsp: "allow"
          }
        }
        "#,
    )
    .expect("permission rule config should parse");
    let mut config = test_config(temp_dir.path());
    config.permission_policy = PermissionPolicy::from_config(&parsed);

    let handle = spawn_coordinator(
        config,
        Arc::new(FakeClock::new()),
        Arc::new(DefaultRedactor::default()),
    );

    let run = handle
        .start_run("permission_rule_bash", temp_dir.path())
        .await
        .expect("start run");

    let actor = EventActor::new(ActorKind::Supervisor, Some("agent-supervisor".to_string()));
    let denied = handle
        .request_tool_call(
            actor.clone(),
            Some("deep".to_string()),
            "shell.run",
            json!({"cmd": "git status"}),
        )
        .await
        .expect_err("exact bash rule should deny");
    assert!(matches!(denied, CoordinatorError::PermissionDenied(_)));

    let allowed_tool_call_id = handle
        .request_tool_call(
            actor,
            Some("deep".to_string()),
            "shell.run",
            json!({"cmd": "git diff"}),
        )
        .await
        .expect("catch-all bash rule should allow");

    tokio::time::sleep(Duration::from_millis(80)).await;
    handle.stop_run().await.expect("stop run");

    let events = read_events(&run.events_path);
    assert!(events.iter().any(|event| {
        matches!(
            &event.payload,
            EventV1::PermissionResolved(data)
                if data.reason.as_deref() == Some("policy denied request (shell)")
        )
    }));
    assert!(events.iter().any(|event| {
        matches!(
            &event.payload,
            EventV1::ToolCallStarted(data) if data.tool_call_id == allowed_tool_call_id
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
            prompt_tokens_estimate: None,
            estimate_source: None,
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
            prompt_tokens_estimate: None,
            estimate_source: None,
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
    let compaction_config = CompactionRuntimeConfig::default();
    for heading in provider_context_summary_required_headings(&compaction_config) {
        assert!(
            checkpoint.summary.contains(heading),
            "summary missing structured heading {heading}:\n{}",
            checkpoint.summary
        );
    }
    assert_eq!(
        checkpoint
            .summary_source
            .as_ref()
            .and_then(|source| source.summary_contract_version),
        Some(super::PROVIDER_CONTEXT_SUMMARY_CONTRACT_VERSION)
    );
    assert_eq!(
        checkpoint
            .summary_source
            .as_ref()
            .and_then(|source| source.summary_contract_enforced),
        Some(true)
    );
    assert!(checkpoint.summary.contains("first question"));
}

#[test]
fn compaction_trigger_pre_prompt_uses_estimate_without_provider_usage() {
    let temp_dir = tempfile::tempdir().expect("tempdir");
    let clock = FakeClock::new();
    let redactor = DefaultRedactor::default();
    let mut run_state = test_run_state(temp_dir.path(), "run_compaction_pre_prompt_estimate");
    run_state.recorded_runtime_context = Some(compaction_runtime_context());
    run_state.provider_context_by_agent.insert(
        "agent_000001".to_string(),
        ProviderContext::from_turns(vec![
            ProviderConversationTurn {
                user_prompt: "pre-prompt first question".to_string(),
                assistant_response: "A".repeat(12_000),
                ..ProviderConversationTurn::default()
            },
            ProviderConversationTurn {
                user_prompt: "pre-prompt second question".to_string(),
                assistant_response: "B".repeat(12_000),
                ..ProviderConversationTurn::default()
            },
        ]),
    );
    let trigger = ProviderCompactionTrigger {
        agent_id: "agent_000001".to_string(),
        profile_name: "alpha".to_string(),
        model_ref: "default:model-1".to_string(),
        provider_id: Some("default".to_string()),
        model_id: Some("model-1".to_string()),
        through_request_id: Some("req_000003".to_string()),
        trigger_reason: "pre_prompt".to_string(),
        tokens_before: None,
        prompt_tokens_estimate: Some(512),
        estimate_source: None,
    };

    compact_provider_context(
        &clock,
        &redactor,
        &mut run_state,
        &trigger,
        &CompactionRuntimeConfig::default(),
        &super::CompactionSummaryDecision::deterministic(&trigger),
    )
    .expect("pre-prompt compaction should succeed")
    .expect("pre-prompt compaction should write a checkpoint");

    let events = read_events(&run_state.info.events_path);
    let written = events
        .iter()
        .find_map(|event| match &event.payload {
            EventV1::CompactionWritten(payload) => Some(payload.clone()),
            _ => None,
        })
        .expect("compaction written event");
    assert_eq!(written.trigger_reason, "pre_prompt");
    let checkpoint_path = run_state.info.run_dir.join(&written.artifact_path);
    let checkpoint_body = fs::read_to_string(&checkpoint_path).expect("read checkpoint artifact");
    let checkpoint_json: serde_json::Value =
        serde_json::from_str(&checkpoint_body).expect("parse checkpoint artifact json");
    assert_eq!(
        checkpoint_json
            .get("estimate_source")
            .and_then(serde_json::Value::as_str),
        Some("estimated_context_and_prompt")
    );
}

#[test]
fn compaction_trigger_uses_fallback_budget_without_model_metadata() {
    let temp_dir = tempfile::tempdir().expect("tempdir");
    let clock = FakeClock::new();
    let redactor = DefaultRedactor::default();
    let mut run_state = test_run_state(temp_dir.path(), "run_compaction_fallback_budget");
    run_state.recorded_runtime_context = Some(RecordedRuntimeContext::from_profile_model(
        "no_metadata_profile",
        "mock:model-1",
    ));
    run_state.provider_context_by_agent.insert(
        "agent_000001".to_string(),
        ProviderContext::from_turns(vec![
            ProviderConversationTurn {
                user_prompt: "fallback first question".to_string(),
                assistant_response: "A".repeat(12_000),
                ..ProviderConversationTurn::default()
            },
            ProviderConversationTurn {
                user_prompt: "fallback second question".to_string(),
                assistant_response: "B".repeat(12_000),
                ..ProviderConversationTurn::default()
            },
        ]),
    );
    let trigger = ProviderCompactionTrigger {
        agent_id: "agent_000001".to_string(),
        profile_name: "no_metadata_profile".to_string(),
        model_ref: "mock:model-1".to_string(),
        provider_id: None,
        model_id: None,
        through_request_id: Some("req_000002".to_string()),
        trigger_reason: "proactive".to_string(),
        tokens_before: None,
        prompt_tokens_estimate: None,
        estimate_source: None,
    };
    let compaction_config = CompactionRuntimeConfig {
        fallback_input_tokens: 2_000,
        ..CompactionRuntimeConfig::default()
    };

    compact_provider_context(
        &clock,
        &redactor,
        &mut run_state,
        &trigger,
        &compaction_config,
        &super::CompactionSummaryDecision::deterministic(&trigger),
    )
    .expect("fallback-budget compaction should succeed")
    .expect("fallback-budget compaction should write a checkpoint");

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
    let checkpoint_json: serde_json::Value =
        serde_json::from_str(&checkpoint_body).expect("parse checkpoint artifact json");
    assert_eq!(
        checkpoint_json
            .get("estimate_source")
            .and_then(serde_json::Value::as_str),
        Some("fallback_budget")
    );
}

#[test]
fn compaction_trigger_noops_below_estimated_threshold() {
    let temp_dir = tempfile::tempdir().expect("tempdir");
    let clock = FakeClock::new();
    let redactor = DefaultRedactor::default();
    let mut run_state = test_run_state(temp_dir.path(), "run_compaction_below_estimated_threshold");
    run_state.provider_context_by_agent.insert(
        "agent_000001".to_string(),
        ProviderContext::from_turns(vec![
            ProviderConversationTurn {
                user_prompt: "short first question".to_string(),
                assistant_response: "short first answer".to_string(),
                ..ProviderConversationTurn::default()
            },
            ProviderConversationTurn {
                user_prompt: "short second question".to_string(),
                assistant_response: "short second answer".to_string(),
                ..ProviderConversationTurn::default()
            },
        ]),
    );
    let trigger = ProviderCompactionTrigger {
        agent_id: "agent_000001".to_string(),
        profile_name: "alpha".to_string(),
        model_ref: "mock:model-1".to_string(),
        provider_id: None,
        model_id: None,
        through_request_id: Some("req_000002".to_string()),
        trigger_reason: "proactive".to_string(),
        tokens_before: None,
        prompt_tokens_estimate: None,
        estimate_source: None,
    };

    let result = compact_provider_context(
        &clock,
        &redactor,
        &mut run_state,
        &trigger,
        &CompactionRuntimeConfig::default(),
        &super::CompactionSummaryDecision::deterministic(&trigger),
    )
    .expect("below-threshold compaction check should not fail");

    assert!(result.is_none());
    let events = read_events(&run_state.info.events_path);
    assert!(events.iter().all(|event| {
        !matches!(
            event.payload,
            EventV1::CompactionRequested(_)
                | EventV1::CompactionWritten(_)
                | EventV1::CompactionApplied(_)
        )
    }));
}

#[test]
fn structured_summary_contract_can_be_disabled_for_legacy_headings() {
    let temp_dir = tempfile::tempdir().expect("tempdir");
    let clock = FakeClock::new();
    let redactor = DefaultRedactor::default();
    let mut run_state = test_run_state(temp_dir.path(), "run_compaction_legacy_contract");
    run_state.recorded_runtime_context = Some(compaction_runtime_context());
    run_state.provider_context_by_agent.insert(
        "agent_000001".to_string(),
        ProviderContext::from_turns(vec![
            long_turn("legacy first question", 'A'),
            long_turn("legacy second question", 'B'),
        ]),
    );
    let trigger = ProviderCompactionTrigger {
        agent_id: "agent_000001".to_string(),
        profile_name: "alpha".to_string(),
        model_ref: "default:model-1".to_string(),
        provider_id: Some("default".to_string()),
        model_id: Some("model-1".to_string()),
        through_request_id: Some("req_000002".to_string()),
        trigger_reason: "proactive".to_string(),
        tokens_before: Some(3_900),
        prompt_tokens_estimate: None,
        estimate_source: None,
    };
    let compaction_config = CompactionRuntimeConfig {
        structured_summary_contract: false,
        ..CompactionRuntimeConfig::default()
    };

    compact_provider_context(
        &clock,
        &redactor,
        &mut run_state,
        &trigger,
        &compaction_config,
        &super::CompactionSummaryDecision::deterministic(&trigger),
    )
    .expect("legacy contract compaction should succeed")
    .expect("legacy contract compaction should write a checkpoint");

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

    for heading in provider_context_summary_required_headings(&compaction_config) {
        assert!(
            checkpoint.summary.contains(heading),
            "legacy summary missing structured heading {heading}:\n{}",
            checkpoint.summary
        );
    }
    assert_eq!(
        checkpoint
            .summary_source
            .as_ref()
            .and_then(|source| source.summary_contract_enforced),
        Some(false)
    );
}

#[test]
fn deterministic_summary_uses_required_harness_sections() {
    let summary_source = ProviderCompactionSummarySource {
        strategy: "deterministic_rolling_summary".to_string(),
        model_ref: "default:model-1".to_string(),
        provider_id: Some("default".to_string()),
        model_id: Some("model-1".to_string()),
        reasoning_effort: None,
        text_verbosity: None,
        previous_summary_used: false,
        model_backed: false,
        deterministic_fallback: true,
        summary_contract_version: Some(super::PROVIDER_CONTEXT_SUMMARY_CONTRACT_VERSION),
        summary_contract_enforced: Some(true),
    };
    let config = CompactionRuntimeConfig::default();

    let summary = build_provider_context_summary(
        None,
        &[ProviderConversationTurn {
            user_prompt: "first compacted question".to_string(),
            assistant_response: "first compacted answer".to_string(),
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
            split_prefix_summary: None,
            note: None,
        },
        &summary_source,
        &config,
    );

    for heading in provider_context_summary_required_headings(&config) {
        assert!(
            summary.contains(heading),
            "Harness summary missing required heading {heading}:\n{summary}"
        );
    }
    let legacy_config = CompactionRuntimeConfig {
        structured_summary_contract: false,
        ..CompactionRuntimeConfig::default()
    };
    assert!(!summary.contains(provider_context_summary_required_headings(&legacy_config)[1]));
    assert!(!summary.contains(provider_context_summary_required_headings(&legacy_config)[9]));
    assert!(summary.contains("first compacted question"));
}

#[test]
fn model_summary_validation_rejects_missing_required_harness_section() {
    let config = CompactionRuntimeConfig::default();
    let plan = provider_context_compaction_plan_fixture();
    let omitted_heading = provider_context_summary_required_headings(&config)
        .last()
        .copied()
        .expect("Harness contract has headings");
    let mut headings = provider_context_summary_required_headings(&config)
        .iter()
        .copied()
        .filter(|heading| *heading != omitted_heading)
        .map(|heading| format!("{heading}\n- content"))
        .collect::<Vec<_>>()
        .join("\n\n");
    headings.push_str("\n\ncompact enough");

    let err = validate_model_compaction_summary(&headings, 20_000, &plan, &config)
        .expect_err("summary missing a Harness heading must be rejected");

    assert!(err.contains(omitted_heading));
    let prompt = build_model_compaction_prompt(None, &plan, "draft", &config);
    for heading in provider_context_summary_required_headings(&config) {
        assert!(prompt.contains(heading));
    }
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
            prompt_tokens_estimate: None,
            estimate_source: None,
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
            prompt_tokens_estimate: None,
            estimate_source: None,
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
fn operational_memory_records_read_and_modified_files_from_events() {
    let temp_dir = tempfile::tempdir().expect("tempdir");
    let clock = FakeClock::new();
    let redactor = DefaultRedactor::default();
    let first_answer = 'A'.to_string().repeat(6_000);
    let second_answer = 'B'.to_string().repeat(6_000);
    write_restore_history_fixture(
        temp_dir.path(),
        "run_operational_memory_records",
        &operational_memory_history_events(
            "run_operational_memory_records",
            &first_answer,
            &second_answer,
            vec![EventV1::ToolCallRequested(ToolCallRequestedEvent {
                tool_call_id: "toolcall_read".to_string(),
                tool_id: "read".to_string(),
                args_summary: "read src/lib.rs".to_string(),
                args_digest: "digest-read".to_string(),
                metadata: Some(tool_metadata("read")),
            })],
            vec![
                EventV1::ToolCallFinished(ToolCallFinishedEvent {
                    tool_call_id: "toolcall_read".to_string(),
                    status: ToolCallStatus::Succeeded,
                    output_summary: Some("read completed".to_string()),
                    output_digest: Some("digest-read-output".to_string()),
                    output_json: Some(json!({ "path": "src/lib.rs" })),
                    metadata: Some(tool_metadata("read")),
                }),
                EventV1::EditApplied(EditAppliedEvent {
                    edit_id: "edit_000001".to_string(),
                    path: "src/lib.rs".to_string(),
                    new_file_digest: "digest-new".to_string(),
                    diff_rel_path: None,
                    diff_digest: None,
                }),
                EventV1::ArtifactWritten(ArtifactWrittenEvent {
                    path: "artifacts/toolcalls/toolcall_edit/result.json".to_string(),
                    digest: "digest-artifact".to_string(),
                    bytes: 42,
                    tool_call_id: Some("toolcall_edit".to_string()),
                    tool_metadata: Some(ToolIdentityMetadata {
                        canonical_tool_id: Some("edit".to_string()),
                        alias_source_tool_id: None,
                    }),
                    metadata: std::collections::BTreeMap::from([(
                        "path".to_string(),
                        "src/generated.rs".to_string(),
                    )]),
                }),
            ],
        ),
    );
    let checkpoint = compact_operational_memory_fixture(
        temp_dir.path(),
        "run_operational_memory_records",
        &first_answer,
        &second_answer,
        &clock,
        &redactor,
    );

    assert_eq!(checkpoint.facts.read_files.len(), 1);
    assert_eq!(checkpoint.facts.read_files[0].path, "src/lib.rs");
    assert_eq!(checkpoint.facts.read_files[0].operation, "read");
    assert_eq!(checkpoint.facts.read_files[0].first_seq, Some(5));
    assert_eq!(checkpoint.facts.read_files[0].last_seq, Some(5));
    assert_eq!(
        checkpoint.facts.read_files[0].sources,
        vec!["tool:toolcall_read"]
    );
    assert_eq!(checkpoint.facts.modified_files.len(), 2);
    assert!(checkpoint.facts.modified_files.iter().any(|fact| {
        fact.path == "src/generated.rs" && fact.sources == vec!["artifact:toolcall_edit"]
    }));
    assert!(checkpoint
        .facts
        .modified_files
        .iter()
        .any(|fact| { fact.path == "src/lib.rs" && fact.sources == vec!["edit:edit_000001"] }));
    assert_eq!(
        checkpoint.facts.touched_files,
        vec!["src/generated.rs".to_string(), "src/lib.rs".to_string()]
    );
    assert!(checkpoint.summary.contains("## Operational Memory"));
    assert!(checkpoint.summary.contains("Read files:"));
    assert!(checkpoint.summary.contains("Modified files:"));
    assert!(checkpoint.summary.contains("src/generated.rs"));
}

#[test]
fn replay_equivalence_after_failed_turn_pre_prompt_compaction_resume() {
    let temp_dir = tempfile::tempdir().expect("tempdir");
    let run_id = "run_replay_equivalence_failed_pre_prompt";
    let run_dir = temp_dir.path().join(run_id);
    let checkpoint_rel = "artifacts/compactions/agent_000001/checkpoint_000012.json";
    let checkpoint_path = run_dir.join(checkpoint_rel);
    fs::create_dir_all(checkpoint_path.parent().expect("checkpoint parent"))
        .expect("create checkpoint directory");
    let checkpoint = ProviderContextCheckpoint {
        metadata: ProviderContextCheckpointMetadata {
            checkpoint_id: "checkpoint_000012".to_string(),
            agent_id: "agent_000001".to_string(),
            run_id: run_id.to_string(),
            through_seq: 11,
            through_request_id: Some("req_000002".to_string()),
            provider_id: Some("default".to_string()),
            model_id: Some("model-1".to_string()),
            tokens_before: None,
            tokens_before_estimate: Some(12_000),
            tokens_after_estimate: Some(3_200),
            summary_tokens_estimate: Some(600),
            compacted_turns: Some(1),
            preserved_turns: Some(1),
            reduction_tokens_estimate: Some(8_800),
            reduction_percent_estimate: Some(73),
            trigger_reason: Some("pre_prompt".to_string()),
        },
        summary: "## Goal\n- Continue after mixed success/failure compaction.\n\n## Constraints\n- Preserve incomplete turns.\n\n## Progress\n- First turn completed.\n\n## Key Decisions\n- Replay loads checkpoint artifacts only.\n\n## Next Steps\n1. Resume from checkpoint.\n\n## Critical Context\n- Successful first turn was compacted.".to_string(),
        recent_turns: vec![ProviderConversationTurn {
            user_prompt: "partial failing question".to_string(),
            assistant_response: "partial provider answer before error".to_string(),
            status: ProviderConversationTurnStatus::Failed,
            failure_stage: Some("provider_error".to_string()),
            failure_reason: Some("provider exploded".to_string()),
            request_id: Some("req_000002".to_string()),
            first_seq: Some(6),
            last_seq: Some(9),
            ..ProviderConversationTurn::default()
        }],
        pruned_tool_artifacts: Vec::new(),
        facts: ProviderCompactionFacts {
            compacted_turns: vec![crate::agent::ProviderCompactionTurnFact {
                request_id: Some("req_000001".to_string()),
                first_seq: Some(2),
                last_seq: Some(5),
                user_excerpt: "first successful question".to_string(),
                assistant_excerpt: "first successful answer".to_string(),
                ..Default::default()
            }],
            read_files: vec![ProviderFileOperationFact {
                path: "src/read_before_failure.rs".to_string(),
                operation: "read".to_string(),
                first_seq: Some(4),
                last_seq: Some(4),
                sources: vec!["tool:toolcall_read".to_string()],
                summary: Some("read before failure".to_string()),
            }],
            modified_files: vec![ProviderFileOperationFact {
                path: "src/modified_before_failure.rs".to_string(),
                operation: "modified".to_string(),
                first_seq: Some(5),
                last_seq: Some(5),
                sources: vec!["edit:edit_000001".to_string()],
                summary: Some("modified before failure".to_string()),
            }],
            touched_files: vec![
                "src/modified_before_failure.rs".to_string(),
                "src/read_before_failure.rs".to_string(),
            ],
            operation_facts: vec![
                "read src/read_before_failure.rs via tool:toolcall_read".to_string(),
                "modified src/modified_before_failure.rs via edit:edit_000001".to_string(),
            ],
            ..ProviderCompactionFacts::default()
        },
        tail_boundary: Some(ProviderCompactionTailBoundary {
            mode: "whole_turn_tail".to_string(),
            preserved_turns: 1,
            preserved_tokens_estimate: 700,
            preserved_from_request_id: Some("req_000002".to_string()),
            preserved_from_seq: Some(6),
            split_prefix_summary: None,
            note: None,
        }),
        summary_source: Some(ProviderCompactionSummarySource {
            strategy: "deterministic_rolling_summary".to_string(),
            model_ref: "default:model-1".to_string(),
            provider_id: Some("default".to_string()),
            model_id: Some("model-1".to_string()),
            reasoning_effort: None,
            text_verbosity: None,
            previous_summary_used: false,
            model_backed: false,
            deterministic_fallback: true,
            summary_contract_version: Some(super::PROVIDER_CONTEXT_SUMMARY_CONTRACT_VERSION),
            summary_contract_enforced: Some(true),
        }),
        timeline_entry: None,
    };
    fs::write(
        &checkpoint_path,
        serde_json::to_string_pretty(&checkpoint).expect("serialize checkpoint"),
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
                EventActor::new(ActorKind::User, None),
                None,
                EventV1::UserMessageSubmitted(UserMessageSubmittedEvent {
                    request_id: "req_000001".to_string(),
                    text: "first successful question".to_string(),
                }),
            ),
            restore_fixture_event(
                run_id,
                3,
                EventActor::new(ActorKind::Worker, Some("agent_000001".to_string())),
                Some("req_000001"),
                EventV1::ProviderRequestStarted(ProviderRequestStartedEvent {
                    request_id: "req_000001".to_string(),
                    provider_id: "default".to_string(),
                    model_id: "model-1".to_string(),
                    prompt_summary: "first successful question".to_string(),
                    request_digest: "digest-1".to_string(),
                    metadata: None,
                }),
            ),
            restore_fixture_event(
                run_id,
                4,
                EventActor::new(ActorKind::Worker, Some("agent_000001".to_string())),
                Some("req_000001"),
                EventV1::ToolCallFinished(ToolCallFinishedEvent {
                    tool_call_id: "toolcall_read".to_string(),
                    status: ToolCallStatus::Succeeded,
                    output_summary: Some("read before failure".to_string()),
                    output_digest: Some("digest-read".to_string()),
                    output_json: Some(json!({ "path": "src/read_before_failure.rs" })),
                    metadata: Some(tool_metadata("read")),
                }),
            ),
            restore_fixture_event(
                run_id,
                5,
                EventActor::new(ActorKind::Worker, Some("agent_000001".to_string())),
                Some("req_000001"),
                EventV1::TaskCompleted(TaskCompletedEvent {
                    task_id: "task_000001".to_string(),
                    result_summary: "first successful answer".to_string(),
                    result_digest: "digest-task-1".to_string(),
                    metadata: None,
                }),
            ),
            restore_fixture_event(
                run_id,
                6,
                EventActor::new(ActorKind::User, None),
                None,
                EventV1::UserMessageSubmitted(UserMessageSubmittedEvent {
                    request_id: "req_000002".to_string(),
                    text: "partial failing question".to_string(),
                }),
            ),
            restore_fixture_event(
                run_id,
                7,
                EventActor::new(ActorKind::Worker, Some("agent_000001".to_string())),
                Some("req_000002"),
                EventV1::ProviderRequestStarted(ProviderRequestStartedEvent {
                    request_id: "req_000002".to_string(),
                    provider_id: "default".to_string(),
                    model_id: "model-1".to_string(),
                    prompt_summary: "partial failing question".to_string(),
                    request_digest: "digest-2".to_string(),
                    metadata: None,
                }),
            ),
            restore_fixture_event(
                run_id,
                8,
                EventActor::new(ActorKind::Worker, Some("agent_000001".to_string())),
                Some("req_000002"),
                EventV1::ProviderStreamDelta(ProviderStreamDeltaEvent {
                    request_id: "req_000002".to_string(),
                    delta: "partial provider answer before error".to_string(),
                }),
            ),
            restore_fixture_event(
                run_id,
                9,
                EventActor::new(ActorKind::Worker, Some("agent_000001".to_string())),
                Some("req_000002"),
                EventV1::ProviderRequestFinished(ProviderRequestFinishedEvent {
                    request_id: "req_000002".to_string(),
                    finish_reason: "error".to_string(),
                    output_digest: None,
                    usage: None,
                    metadata: None,
                }),
            ),
            restore_fixture_event(
                run_id,
                10,
                EventActor::new(ActorKind::Worker, Some("agent_000001".to_string())),
                Some("req_000002"),
                EventV1::EditApplied(EditAppliedEvent {
                    edit_id: "edit_000001".to_string(),
                    path: "src/modified_before_failure.rs".to_string(),
                    new_file_digest: "digest-modified".to_string(),
                    diff_rel_path: None,
                    diff_digest: None,
                }),
            ),
            restore_fixture_event(
                run_id,
                11,
                EventActor::new(ActorKind::Worker, Some("agent_000001".to_string())),
                Some("req_000002"),
                EventV1::TaskCancelled(TaskCancelledEvent {
                    task_id: "task_000002".to_string(),
                    reason: "provider exploded".to_string(),
                    task_scope: None,
                }),
            ),
            restore_fixture_event(
                run_id,
                12,
                EventActor::new(ActorKind::System, Some("coordinator".to_string())),
                Some("compaction:agent_000001"),
                EventV1::CompactionWritten(CompactionWrittenEvent {
                    checkpoint_id: "checkpoint_000012".to_string(),
                    agent_id: "agent_000001".to_string(),
                    artifact_path: checkpoint_rel.to_string(),
                    artifact_digest: Some("digest-checkpoint".to_string()),
                    artifact_bytes: 2048,
                    trigger_reason: "pre_prompt".to_string(),
                    through_seq: 11,
                    through_request_id: Some("req_000002".to_string()),
                    provider_id: Some("default".to_string()),
                    model_id: Some("model-1".to_string()),
                    tokens_before: None,
                    tokens_before_estimate: Some(12_000),
                    tokens_after_estimate: Some(3_200),
                    summary_tokens_estimate: Some(600),
                    compacted_turns: Some(1),
                    reduction_tokens_estimate: Some(8_800),
                    reduction_percent_estimate: Some(73),
                    estimate_source: Some("estimated_context_and_prompt".to_string()),
                    preserved_turns: 1,
                }),
            ),
            restore_fixture_event(
                run_id,
                13,
                EventActor::new(ActorKind::System, Some("coordinator".to_string())),
                Some("compaction:agent_000001"),
                EventV1::CompactionApplied(CompactionAppliedEvent {
                    checkpoint_id: "checkpoint_000012".to_string(),
                    agent_id: "agent_000001".to_string(),
                    through_seq: 11,
                    through_request_id: Some("req_000002".to_string()),
                    tokens_before_estimate: Some(12_000),
                    tokens_after_estimate: Some(3_200),
                    summary_tokens_estimate: Some(600),
                    compacted_turns: Some(1),
                    preserved_turns: Some(1),
                    reduction_tokens_estimate: Some(8_800),
                    reduction_percent_estimate: Some(73),
                    estimate_source: Some("estimated_context_and_prompt".to_string()),
                }),
            ),
        ],
    );

    let expected_context = ProviderContext::from_checkpoint(checkpoint.clone());
    let restored = restore_provider_context_from_history(temp_dir.path(), run_id)
        .expect("restore mixed compaction history");
    let restored_context = restored
        .get("agent_000001")
        .expect("restored checkpoint provider context");

    assert_eq!(
        restored_context.compacted_summary,
        expected_context.compacted_summary
    );
    let restored_summary = restored_context
        .compacted_summary
        .as_deref()
        .expect("restored summary");
    assert!(restored_summary.contains("src/read_before_failure.rs"));
    assert!(restored_summary.contains("src/modified_before_failure.rs"));
    assert_eq!(
        restored_context.preserved_turns,
        expected_context.preserved_turns
    );
    assert_eq!(restored_context.preserved_turns.len(), 1);
    assert_eq!(
        restored_context.preserved_turns[0].status,
        ProviderConversationTurnStatus::Failed
    );
    assert_eq!(
        restored_context.preserved_turns[0].failure_stage.as_deref(),
        Some("provider_error")
    );
    assert_eq!(
        restored_context.checkpoint, expected_context.checkpoint,
        "checkpoint metadata should replay exactly"
    );
    assert_eq!(checkpoint.facts.read_files.len(), 1);
    assert_eq!(checkpoint.facts.modified_files.len(), 1);
}

#[test]
fn operational_memory_redacts_secret_shaped_facts() {
    let temp_dir = tempfile::tempdir().expect("tempdir");
    let clock = FakeClock::new();
    let redactor = DefaultRedactor::default();
    let first_answer = 'A'.to_string().repeat(6_000);
    let second_answer = 'B'.to_string().repeat(6_000);
    let raw_secret_path = "src/sk-AbCdEf0123456789-token.rs";
    let raw_bearer = "Bearer abc.def-ghi_123";
    write_restore_history_fixture(
        temp_dir.path(),
        "run_operational_memory_redacts_secrets",
        &operational_memory_history_events(
            "run_operational_memory_redacts_secrets",
            &first_answer,
            &second_answer,
            vec![EventV1::ToolCallRequested(ToolCallRequestedEvent {
                tool_call_id: "toolcall_secret_read".to_string(),
                tool_id: "read".to_string(),
                args_summary: format!("read {raw_secret_path}"),
                args_digest: "digest-secret-read".to_string(),
                metadata: Some(tool_metadata("read")),
            })],
            vec![EventV1::ToolCallFinished(ToolCallFinishedEvent {
                tool_call_id: "toolcall_secret_read".to_string(),
                status: ToolCallStatus::Succeeded,
                output_summary: Some(format!("read token-like path with {raw_bearer}")),
                output_digest: Some("digest-secret-output".to_string()),
                output_json: Some(json!({ "path": raw_secret_path })),
                metadata: Some(tool_metadata("read")),
            })],
        ),
    );
    let checkpoint = compact_operational_memory_fixture(
        temp_dir.path(),
        "run_operational_memory_redacts_secrets",
        &first_answer,
        &second_answer,
        &clock,
        &redactor,
    );
    let checkpoint_json = serde_json::to_string(&checkpoint).expect("serialize checkpoint");

    assert!(!checkpoint_json.contains("sk-AbCdEf0123456789"));
    assert!(!checkpoint_json.contains("Bearer abc.def-ghi_123"));
    assert!(checkpoint_json.contains("[REDACTED_API_KEY]"));
    assert!(checkpoint_json.contains("Bearer [REDACTED]"));
    assert!(checkpoint
        .facts
        .read_files
        .iter()
        .all(|fact| !fact.path.contains("sk-AbCdEf0123456789")));
    assert!(checkpoint
        .facts
        .operation_facts
        .iter()
        .all(|fact| !fact.contains("Bearer abc.def-ghi_123")));
    assert!(!checkpoint.summary.contains("sk-AbCdEf0123456789"));
    assert!(!checkpoint.summary.contains("Bearer abc.def-ghi_123"));
}

#[test]
fn operational_memory_dedupes_sorts_and_caps_paths() {
    let temp_dir = tempfile::tempdir().expect("tempdir");
    let clock = FakeClock::new();
    let redactor = DefaultRedactor::default();
    let first_answer = 'A'.to_string().repeat(6_000);
    let second_answer = 'B'.to_string().repeat(6_000);
    let matches = (0..55)
        .rev()
        .map(|index| json!({ "path": format!("src/file_{index:03}.rs") }))
        .chain(std::iter::once(json!({ "path": "src/file_000.rs" })))
        .collect::<Vec<_>>();
    write_restore_history_fixture(
        temp_dir.path(),
        "run_operational_memory_caps",
        &operational_memory_history_events(
            "run_operational_memory_caps",
            &first_answer,
            &second_answer,
            vec![EventV1::ToolCallRequested(ToolCallRequestedEvent {
                tool_call_id: "toolcall_grep".to_string(),
                tool_id: "grep".to_string(),
                args_summary: "grep files".to_string(),
                args_digest: "digest-grep".to_string(),
                metadata: Some(tool_metadata("grep")),
            })],
            vec![EventV1::ToolCallFinished(ToolCallFinishedEvent {
                tool_call_id: "toolcall_grep".to_string(),
                status: ToolCallStatus::Succeeded,
                output_summary: Some("grep completed".to_string()),
                output_digest: Some("digest-grep-output".to_string()),
                output_json: Some(json!({
                    "matches": matches,
                    "path": "/outside/workspace/ignored.rs"
                })),
                metadata: Some(tool_metadata("grep")),
            })],
        ),
    );
    let checkpoint = compact_operational_memory_fixture(
        temp_dir.path(),
        "run_operational_memory_caps",
        &first_answer,
        &second_answer,
        &clock,
        &redactor,
    );

    assert_eq!(checkpoint.facts.read_files.len(), 50);
    assert_eq!(checkpoint.facts.read_files[0].path, "src/file_000.rs");
    assert_eq!(checkpoint.facts.read_files[49].path, "src/file_049.rs");
    assert!(checkpoint
        .facts
        .read_files
        .iter()
        .all(|fact| fact.operation == "read"));
    assert_eq!(checkpoint.facts.touched_files.len(), 50);
    assert!(checkpoint
        .facts
        .operation_facts
        .iter()
        .any(|fact| fact == "5 additional read file(s) omitted"));
}

#[test]
fn operational_memory_ignores_freeform_path_like_output() {
    let temp_dir = tempfile::tempdir().expect("tempdir");
    let clock = FakeClock::new();
    let redactor = DefaultRedactor::default();
    let first_answer = 'A'.to_string().repeat(6_000);
    let second_answer = 'B'.to_string().repeat(6_000);
    write_restore_history_fixture(
        temp_dir.path(),
        "run_operational_memory_freeform",
        &operational_memory_history_events(
            "run_operational_memory_freeform",
            &first_answer,
            &second_answer,
            vec![EventV1::ToolCallRequested(ToolCallRequestedEvent {
                tool_call_id: "toolcall_read".to_string(),
                tool_id: "read".to_string(),
                args_summary: "read output".to_string(),
                args_digest: "digest-read".to_string(),
                metadata: Some(tool_metadata("read")),
            })],
            vec![EventV1::ToolCallFinished(ToolCallFinishedEvent {
                tool_call_id: "toolcall_read".to_string(),
                status: ToolCallStatus::Succeeded,
                output_summary: Some(
                    "free-form text mentions src/freeform.rs and /workspace/project/src/secret.rs"
                        .to_string(),
                ),
                output_digest: Some("digest-output".to_string()),
                output_json: Some(json!({ "message": "src/not-a-path-field.rs" })),
                metadata: Some(tool_metadata("read")),
            })],
        ),
    );
    let checkpoint = compact_operational_memory_fixture(
        temp_dir.path(),
        "run_operational_memory_freeform",
        &first_answer,
        &second_answer,
        &clock,
        &redactor,
    );

    assert!(checkpoint.facts.read_files.is_empty());
    assert!(checkpoint.facts.modified_files.is_empty());
    assert!(checkpoint.facts.touched_files.is_empty());
}

#[test]
fn operational_memory_preserves_touched_files_legacy_union() {
    let facts = ProviderCompactionFacts {
        read_files: vec![ProviderFileOperationFact {
            path: "src/read.rs".to_string(),
            operation: "read".to_string(),
            first_seq: Some(2),
            last_seq: Some(2),
            sources: vec!["tool:read".to_string()],
            summary: None,
        }],
        modified_files: vec![ProviderFileOperationFact {
            path: "src/modified.rs".to_string(),
            operation: "modified".to_string(),
            first_seq: Some(3),
            last_seq: Some(3),
            sources: vec!["edit:edit".to_string()],
            summary: None,
        }],
        ..ProviderCompactionFacts::default()
    };

    let summary = build_provider_context_summary(
        None,
        &[ProviderConversationTurn {
            user_prompt: "question".to_string(),
            assistant_response: "answer".to_string(),
            ..ProviderConversationTurn::default()
        }],
        &[],
        &facts,
        &ProviderCompactionTailBoundary {
            mode: "whole_turn_tail".to_string(),
            preserved_turns: 1,
            preserved_tokens_estimate: 10,
            preserved_from_request_id: None,
            preserved_from_seq: None,
            split_prefix_summary: None,
            note: None,
        },
        &ProviderCompactionSummarySource {
            strategy: "deterministic_rolling_summary".to_string(),
            model_ref: "default:model-1".to_string(),
            provider_id: Some("default".to_string()),
            model_id: Some("model-1".to_string()),
            reasoning_effort: None,
            text_verbosity: None,
            previous_summary_used: false,
            model_backed: false,
            deterministic_fallback: true,
            summary_contract_version: Some(super::PROVIDER_CONTEXT_SUMMARY_CONTRACT_VERSION),
            summary_contract_enforced: Some(true),
        },
        &CompactionRuntimeConfig::default(),
    );

    assert!(summary.contains("src/read.rs"));
    assert!(summary.contains("src/modified.rs"));
}

#[test]
fn operational_memory_resume_loads_checkpoint_facts_without_filesystem_scan() {
    let temp_dir = tempfile::tempdir().expect("tempdir");
    let run_id = "run_operational_memory_resume";
    let run_dir = temp_dir.path().join(run_id);
    let checkpoint_rel = "artifacts/compactions/agent_000001/checkpoint_000010.json".to_string();
    let checkpoint_path = run_dir.join(&checkpoint_rel);
    fs::create_dir_all(checkpoint_path.parent().expect("checkpoint parent"))
        .expect("create checkpoint directory");
    fs::write(
        &checkpoint_path,
        serde_json::to_string_pretty(&ProviderContextCheckpoint {
            metadata: checkpoint_metadata_for_run(run_id, "checkpoint_000010", 9),
            summary: "Earlier checkpoint summary".to_string(),
            recent_turns: Vec::new(),
            pruned_tool_artifacts: Vec::new(),
            facts: ProviderCompactionFacts {
                read_files: vec![ProviderFileOperationFact {
                    path: "src/restored_read.rs".to_string(),
                    operation: "read".to_string(),
                    first_seq: Some(4),
                    last_seq: Some(4),
                    sources: vec!["tool:toolcall_read".to_string()],
                    summary: Some("read source file".to_string()),
                }],
                modified_files: vec![ProviderFileOperationFact {
                    path: "src/restored_modified.rs".to_string(),
                    operation: "modified".to_string(),
                    first_seq: Some(5),
                    last_seq: Some(5),
                    sources: vec!["edit:edit_000001".to_string()],
                    summary: Some("modified source file".to_string()),
                }],
                touched_files: vec![
                    "src/restored_modified.rs".to_string(),
                    "src/restored_read.rs".to_string(),
                ],
                operation_facts: vec!["restored operational fact".to_string()],
                ..ProviderCompactionFacts::default()
            },
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
                Some("compaction:agent_000001"),
                EventV1::CompactionWritten(CompactionWrittenEvent {
                    checkpoint_id: "checkpoint_000010".to_string(),
                    agent_id: "agent_000001".to_string(),
                    artifact_path: checkpoint_rel,
                    artifact_digest: Some("digest-checkpoint".to_string()),
                    artifact_bytes: 100,
                    trigger_reason: "proactive".to_string(),
                    through_seq: 9,
                    through_request_id: Some("req_000001".to_string()),
                    provider_id: Some("default".to_string()),
                    model_id: Some("model-1".to_string()),
                    tokens_before: None,
                    tokens_before_estimate: None,
                    tokens_after_estimate: None,
                    summary_tokens_estimate: None,
                    compacted_turns: None,
                    reduction_tokens_estimate: None,
                    reduction_percent_estimate: None,
                    estimate_source: None,
                    preserved_turns: 0,
                }),
            ),
            restore_fixture_event(
                run_id,
                3,
                EventActor::new(ActorKind::System, Some("coordinator".to_string())),
                Some("compaction:agent_000001"),
                EventV1::CompactionApplied(CompactionAppliedEvent {
                    checkpoint_id: "checkpoint_000010".to_string(),
                    agent_id: "agent_000001".to_string(),
                    through_seq: 9,
                    through_request_id: Some("req_000001".to_string()),
                    tokens_before_estimate: None,
                    tokens_after_estimate: None,
                    summary_tokens_estimate: None,
                    compacted_turns: None,
                    preserved_turns: None,
                    reduction_tokens_estimate: None,
                    reduction_percent_estimate: None,
                    estimate_source: None,
                }),
            ),
        ],
    );

    let restored = restore_provider_context_from_history(temp_dir.path(), run_id)
        .expect("restore provider context");
    let summary = restored
        .get("agent_000001")
        .and_then(|context| context.compacted_summary.as_deref())
        .expect("restored checkpoint summary");
    assert!(summary.contains("## Operational Memory"));
    assert!(summary.contains("src/restored_read.rs"));
    assert!(summary.contains("src/restored_modified.rs"));
    assert!(summary.contains("restored operational fact"));
}

#[test]
fn legacy_provider_context_checkpoint_deserializes() {
    let body = r#"{
        "checkpoint_id": "checkpoint_legacy",
        "agent_id": "agent_000001",
        "run_id": "run_legacy",
        "through_seq": 7,
        "summary": "legacy summary",
        "summary_source": {
            "strategy": "deterministic_rolling_summary",
            "model_ref": "default:model-1",
            "previous_summary_used": false,
            "model_backed": false,
            "deterministic_fallback": true
        },
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
    let source = checkpoint
        .summary_source
        .as_ref()
        .expect("legacy summary source should deserialize");
    assert_eq!(source.summary_contract_version, None);
    assert_eq!(source.summary_contract_enforced, None);
}

#[test]
fn provider_context_checkpoint_legacy_round_trips_with_new_defaults() {
    let temp_dir = tempfile::tempdir().expect("tempdir");
    let run_id = "run_legacy_checkpoint_round_trip";
    let checkpoint_rel = "artifacts/compactions/agent_000001/checkpoint_legacy.json";
    let checkpoint_path = temp_dir.path().join(run_id).join(checkpoint_rel);
    fs::create_dir_all(checkpoint_path.parent().expect("checkpoint parent"))
        .expect("create checkpoint directory");
    fs::write(
        &checkpoint_path,
        r#"{
            "checkpoint_id": "checkpoint_legacy",
            "agent_id": "agent_000001",
            "run_id": "run_legacy_checkpoint_round_trip",
            "through_seq": 4,
            "through_request_id": "req_000001",
            "summary": "legacy summary that must survive",
            "summary_source": {
                "strategy": "deterministic_rolling_summary",
                "model_ref": "default:model-1",
                "previous_summary_used": false,
                "model_backed": false,
                "deterministic_fallback": true
            },
            "recent_turns": [
                {
                    "user_prompt": "legacy recent question",
                    "assistant_response": "legacy recent answer",
                    "request_id": "req_000001"
                }
            ]
        }"#,
    )
    .expect("write legacy checkpoint artifact");
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
                Some("compaction:agent_000001"),
                EventV1::CompactionWritten(CompactionWrittenEvent {
                    checkpoint_id: "checkpoint_legacy".to_string(),
                    agent_id: "agent_000001".to_string(),
                    artifact_path: checkpoint_rel.to_string(),
                    artifact_digest: None,
                    artifact_bytes: 512,
                    trigger_reason: "proactive".to_string(),
                    through_seq: 4,
                    through_request_id: Some("req_000001".to_string()),
                    provider_id: None,
                    model_id: None,
                    tokens_before: None,
                    tokens_before_estimate: None,
                    tokens_after_estimate: None,
                    summary_tokens_estimate: None,
                    compacted_turns: None,
                    reduction_tokens_estimate: None,
                    reduction_percent_estimate: None,
                    estimate_source: None,
                    preserved_turns: 1,
                }),
            ),
            restore_fixture_event(
                run_id,
                3,
                EventActor::new(ActorKind::System, Some("coordinator".to_string())),
                Some("compaction:agent_000001"),
                EventV1::CompactionApplied(CompactionAppliedEvent {
                    checkpoint_id: "checkpoint_legacy".to_string(),
                    agent_id: "agent_000001".to_string(),
                    through_seq: 4,
                    through_request_id: Some("req_000001".to_string()),
                    tokens_before_estimate: None,
                    tokens_after_estimate: None,
                    summary_tokens_estimate: None,
                    compacted_turns: None,
                    preserved_turns: Some(1),
                    reduction_tokens_estimate: None,
                    reduction_percent_estimate: None,
                    estimate_source: None,
                }),
            ),
        ],
    );

    let restored = restore_provider_context_from_history(temp_dir.path(), run_id)
        .expect("restore legacy checkpoint context");
    let mut restored_context = restored
        .get("agent_000001")
        .cloned()
        .expect("restored agent context");
    assert_eq!(
        restored_context.compacted_summary.as_deref(),
        Some("legacy summary that must survive")
    );
    assert_eq!(restored_context.preserved_turns.len(), 1);
    assert_eq!(
        restored_context.preserved_turns[0].status,
        ProviderConversationTurnStatus::Completed
    );
    restored_context.push_turn(long_turn("new follow-up question", 'N'));

    let clock = FakeClock::new();
    let redactor = DefaultRedactor::default();
    let mut run_state = test_run_state(temp_dir.path(), "run_legacy_checkpoint_round_trip_again");
    run_state.recorded_runtime_context = Some(compaction_runtime_context());
    run_state
        .provider_context_by_agent
        .insert("agent_000001".to_string(), restored_context);
    let trigger = ProviderCompactionTrigger {
        agent_id: "agent_000001".to_string(),
        profile_name: "alpha".to_string(),
        model_ref: "default:model-1".to_string(),
        provider_id: Some("default".to_string()),
        model_id: Some("model-1".to_string()),
        through_request_id: Some("req_000002".to_string()),
        trigger_reason: "manual".to_string(),
        tokens_before: Some(4_000),
        prompt_tokens_estimate: None,
        estimate_source: None,
    };

    compact_provider_context(
        &clock,
        &redactor,
        &mut run_state,
        &trigger,
        &CompactionRuntimeConfig::default(),
        &super::CompactionSummaryDecision::deterministic(&trigger),
    )
    .expect("second compaction should succeed")
    .expect("second compaction should write a checkpoint");

    let events = read_events(&run_state.info.events_path);
    let written = events
        .iter()
        .find_map(|event| match &event.payload {
            EventV1::CompactionWritten(payload) => Some(payload.clone()),
            _ => None,
        })
        .expect("new compaction written event");
    let checkpoint_path = run_state.info.run_dir.join(&written.artifact_path);
    let checkpoint_body = fs::read_to_string(checkpoint_path).expect("read new checkpoint");
    let checkpoint: ProviderContextCheckpoint =
        serde_json::from_str(&checkpoint_body).expect("parse new checkpoint");
    assert!(checkpoint
        .summary
        .contains("legacy summary that must survive"));
    assert!(checkpoint.summary.contains("legacy recent question"));
    assert_eq!(checkpoint.recent_turns.len(), 1);
    assert_eq!(
        checkpoint.recent_turns[0].user_prompt,
        "new follow-up question"
    );
    assert_eq!(
        checkpoint
            .summary_source
            .as_ref()
            .and_then(|source| source.summary_contract_version),
        Some(super::PROVIDER_CONTEXT_SUMMARY_CONTRACT_VERSION)
    );
    assert_eq!(
        checkpoint
            .summary_source
            .as_ref()
            .and_then(|source| source.summary_contract_enforced),
        Some(true)
    );
    assert_eq!(
        checkpoint.facts.previous_checkpoint_id.as_deref(),
        Some("checkpoint_legacy")
    );
    assert!(checkpoint.facts.read_files.is_empty());
    assert!(checkpoint.facts.modified_files.is_empty());
    assert!(checkpoint.facts.operation_facts.is_empty());
}

#[test]
fn failed_turn_status_defaults_to_completed_for_legacy_checkpoint() {
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
        ],
        "facts": {
            "compacted_turns": [
                {
                    "user_excerpt": "old question",
                    "assistant_excerpt": "old answer"
                }
            ]
        }
    }"#;

    let checkpoint: ProviderContextCheckpoint =
        serde_json::from_str(body).expect("legacy checkpoint should deserialize");

    assert_eq!(
        checkpoint.recent_turns[0].status,
        ProviderConversationTurnStatus::Completed
    );
    assert_eq!(checkpoint.recent_turns[0].failure_stage, None);
    assert_eq!(checkpoint.recent_turns[0].failure_reason, None);
    assert_eq!(
        checkpoint.facts.compacted_turns[0].status,
        ProviderConversationTurnStatus::Completed
    );
    let serialized = serde_json::to_value(&checkpoint).expect("serialize checkpoint");
    assert_eq!(
        serialized["recent_turns"][0].get("status"),
        None,
        "completed recent turns should omit status"
    );
    assert_eq!(
        serialized["facts"]["compacted_turns"][0].get("status"),
        None,
        "completed compacted turn facts should omit status"
    );
}

#[test]
fn compaction_turn_facts_include_failed_turn_status() {
    let temp_dir = tempfile::tempdir().expect("tempdir");
    let clock = FakeClock::new();
    let redactor = DefaultRedactor::default();
    let mut run_state = test_run_state(temp_dir.path(), "run_compaction_failed_fact_status");
    run_state.recorded_runtime_context = Some(compaction_runtime_context());
    run_state.provider_context_by_agent.insert(
        "agent_000001".to_string(),
        ProviderContext::from_turns(vec![
            ProviderConversationTurn {
                user_prompt: "first question".to_string(),
                assistant_response: format!("partial answer {}", "A".repeat(6_000)),
                status: ProviderConversationTurnStatus::Failed,
                failure_stage: Some("provider_error".to_string()),
                failure_reason: Some(format!(
                    "provider failed with sk-ABCDE12345ABCDE {}",
                    "details ".repeat(80)
                )),
                ..ProviderConversationTurn::default()
            },
            long_turn("second question", 'B'),
        ]),
    );

    let trigger = ProviderCompactionTrigger {
        agent_id: "agent_000001".to_string(),
        profile_name: "alpha".to_string(),
        model_ref: "default:model-1".to_string(),
        provider_id: Some("default".to_string()),
        model_id: Some("model-1".to_string()),
        through_request_id: Some("req_000002".to_string()),
        trigger_reason: "proactive".to_string(),
        tokens_before: Some(3_900),
        prompt_tokens_estimate: None,
        estimate_source: None,
    };
    compact_provider_context(
        &clock,
        &redactor,
        &mut run_state,
        &trigger,
        &CompactionRuntimeConfig::default(),
        &super::CompactionSummaryDecision::deterministic(&trigger),
    )
    .expect("failed-turn compaction should succeed")
    .expect("failed-turn compaction should write a checkpoint");

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
    assert!(!checkpoint_body.contains("sk-ABCDE12345ABCDE"));
    let checkpoint: ProviderContextCheckpoint =
        serde_json::from_str(&checkpoint_body).expect("parse checkpoint artifact");
    let compacted_fact = checkpoint
        .facts
        .compacted_turns
        .first()
        .expect("compacted failed turn fact");
    assert_eq!(
        compacted_fact.status,
        ProviderConversationTurnStatus::Failed
    );
    assert_eq!(
        compacted_fact.failure_stage.as_deref(),
        Some("provider_error")
    );
    let reason = compacted_fact
        .failure_reason
        .as_deref()
        .expect("failure reason should be retained");
    assert!(reason.contains("[REDACTED_API_KEY]"));
    assert!(
        reason.chars().count() <= super::PROVIDER_CONTEXT_COMPACTION_TURN_EXCERPT_MAX_CHARS + 1
    );
}

#[test]
fn split_oversized_turn_pre_prompt_preserves_suffix_and_prefix_summary() {
    let temp_dir = tempfile::tempdir().expect("tempdir");
    let clock = FakeClock::new();
    let redactor = DefaultRedactor::default();
    let mut run_state = test_run_state(temp_dir.path(), "run_split_pre_prompt_prefix_summary");
    run_state.recorded_runtime_context = Some(compaction_runtime_context());
    let oversized_answer = format!(
        "PREFIX_ANCHOR {} {} SUFFIX_ANCHOR",
        "A".repeat(4_000),
        "B".repeat(7_900)
    );
    run_state.provider_context_by_agent.insert(
        "agent_000001".to_string(),
        ProviderContext::from_turns(vec![
            ProviderConversationTurn {
                user_prompt: "earlier question".to_string(),
                assistant_response: "earlier answer".to_string(),
                ..ProviderConversationTurn::default()
            },
            ProviderConversationTurn {
                user_prompt: "latest oversized question".to_string(),
                assistant_response: oversized_answer,
                request_id: Some("req_latest".to_string()),
                ..ProviderConversationTurn::default()
            },
        ]),
    );
    let trigger = ProviderCompactionTrigger {
        agent_id: "agent_000001".to_string(),
        profile_name: "alpha".to_string(),
        model_ref: "default:model-1".to_string(),
        provider_id: Some("default".to_string()),
        model_id: Some("model-1".to_string()),
        through_request_id: Some("req_latest".to_string()),
        trigger_reason: "pre_prompt".to_string(),
        tokens_before: Some(4_000),
        prompt_tokens_estimate: None,
        estimate_source: None,
    };
    let compaction_config = CompactionRuntimeConfig {
        split_oversized_turns: true,
        ..CompactionRuntimeConfig::default()
    };

    let updated = compact_provider_context(
        &clock,
        &redactor,
        &mut run_state,
        &trigger,
        &compaction_config,
        &super::CompactionSummaryDecision::deterministic(&trigger),
    )
    .expect("pre-prompt split compaction should succeed")
    .expect("pre-prompt split compaction should write checkpoint");

    assert_eq!(updated.updated_context.preserved_turns.len(), 1);
    let recent = &updated.updated_context.preserved_turns[0];
    assert!(recent.assistant_response.contains("SUFFIX_ANCHOR"));
    assert!(!recent.assistant_response.contains("PREFIX_ANCHOR"));
    assert!(recent
        .user_prompt
        .contains("earlier prefix is summarized in the checkpoint"));

    let events = read_events(&run_state.info.events_path);
    let written = events
        .iter()
        .find_map(|event| match &event.payload {
            EventV1::CompactionWritten(payload) if payload.trigger_reason == "pre_prompt" => {
                Some(payload.clone())
            }
            _ => None,
        })
        .expect("pre-prompt compaction written event");
    let checkpoint_path = run_state.info.run_dir.join(&written.artifact_path);
    let checkpoint_body = fs::read_to_string(&checkpoint_path).expect("read checkpoint artifact");
    let checkpoint: ProviderContextCheckpoint =
        serde_json::from_str(&checkpoint_body).expect("parse checkpoint artifact");
    let tail_boundary = checkpoint.tail_boundary.expect("tail boundary");
    assert_eq!(tail_boundary.mode, "split_oversized_turn_tail");
    let prefix_summary = tail_boundary
        .split_prefix_summary
        .expect("split prefix summary");
    assert!(prefix_summary.contains("PREFIX_ANCHOR"));
    assert!(checkpoint.summary.contains("## Critical Context"));
    assert!(checkpoint.summary.contains("Split prefix summary"));
    assert!(checkpoint
        .summary
        .contains("Source facts: split prefix summary"));
}

#[test]
fn split_oversized_failed_provider_error_preserves_incomplete_suffix() {
    let temp_dir = tempfile::tempdir().expect("tempdir");
    let clock = FakeClock::new();
    let redactor = DefaultRedactor::default();
    let mut run_state = test_run_state(temp_dir.path(), "run_split_failed_provider_error");
    run_state.recorded_runtime_context = Some(compaction_runtime_context());
    let oversized_answer = format!(
        "FAILED_PREFIX {} {} FAILED_SUFFIX",
        "C".repeat(4_000),
        "D".repeat(7_900)
    );
    run_state.provider_context_by_agent.insert(
        "agent_000001".to_string(),
        ProviderContext::from_turns(vec![
            long_turn("earlier successful question", 'A'),
            ProviderConversationTurn {
                user_prompt: "failed latest question".to_string(),
                assistant_response: oversized_answer,
                status: ProviderConversationTurnStatus::Failed,
                failure_stage: Some("provider_error".to_string()),
                failure_reason: Some("provider exploded".to_string()),
                request_id: Some("req_failed".to_string()),
                ..ProviderConversationTurn::default()
            },
        ]),
    );
    let trigger = ProviderCompactionTrigger {
        agent_id: "agent_000001".to_string(),
        profile_name: "alpha".to_string(),
        model_ref: "default:model-1".to_string(),
        provider_id: Some("default".to_string()),
        model_id: Some("model-1".to_string()),
        through_request_id: Some("req_failed".to_string()),
        trigger_reason: "failed_response".to_string(),
        tokens_before: Some(4_000),
        prompt_tokens_estimate: None,
        estimate_source: None,
    };
    let compaction_config = CompactionRuntimeConfig {
        split_oversized_turns: true,
        ..CompactionRuntimeConfig::default()
    };

    compact_provider_context(
        &clock,
        &redactor,
        &mut run_state,
        &trigger,
        &compaction_config,
        &super::CompactionSummaryDecision::deterministic(&trigger),
    )
    .expect("failed-response split compaction should succeed")
    .expect("failed-response split compaction should write checkpoint");

    let events = read_events(&run_state.info.events_path);
    let written = events
        .iter()
        .find_map(|event| match &event.payload {
            EventV1::CompactionWritten(payload) if payload.trigger_reason == "failed_response" => {
                Some(payload.clone())
            }
            _ => None,
        })
        .expect("failed-response compaction written event");
    let checkpoint_path = run_state.info.run_dir.join(&written.artifact_path);
    let checkpoint_body = fs::read_to_string(&checkpoint_path).expect("read checkpoint artifact");
    let checkpoint: ProviderContextCheckpoint =
        serde_json::from_str(&checkpoint_body).expect("parse checkpoint artifact");
    assert_eq!(checkpoint.recent_turns.len(), 1);
    let suffix = &checkpoint.recent_turns[0];
    assert_eq!(suffix.status, ProviderConversationTurnStatus::Failed);
    assert_eq!(suffix.failure_stage.as_deref(), Some("provider_error"));
    assert_eq!(suffix.failure_reason.as_deref(), Some("provider exploded"));
    assert!(suffix.assistant_response.contains("FAILED_SUFFIX"));
    assert!(!suffix.assistant_response.contains("FAILED_PREFIX"));
    assert!(suffix
        .user_prompt
        .contains("earlier prefix is summarized in the checkpoint"));
    assert!(checkpoint
        .tail_boundary
        .as_ref()
        .and_then(|boundary| boundary.split_prefix_summary.as_deref())
        .is_some_and(|summary| summary.contains("FAILED_PREFIX")));
}

#[test]
fn split_oversized_turn_refuses_tool_failure_to_avoid_orphan_tools() {
    let temp_dir = tempfile::tempdir().expect("tempdir");
    let clock = FakeClock::new();
    let redactor = DefaultRedactor::default();
    let mut run_state = test_run_state(temp_dir.path(), "run_split_refuses_tool_failure");
    run_state.recorded_runtime_context = Some(compaction_runtime_context());
    run_state.provider_context_by_agent.insert(
        "agent_000001".to_string(),
        ProviderContext::from_turns(vec![
            long_turn("earlier successful question", 'A'),
            ProviderConversationTurn {
                user_prompt: "tool failure latest question".to_string(),
                assistant_response: format!(
                    "TOOL_PREFIX {} {} TOOL_SUFFIX",
                    "E".repeat(4_000),
                    "F".repeat(7_900)
                ),
                status: ProviderConversationTurnStatus::Failed,
                failure_stage: Some("tool_failure".to_string()),
                failure_reason: Some("tool failed closed".to_string()),
                request_id: Some("req_tool_failed".to_string()),
                ..ProviderConversationTurn::default()
            },
        ]),
    );
    let trigger = ProviderCompactionTrigger {
        agent_id: "agent_000001".to_string(),
        profile_name: "alpha".to_string(),
        model_ref: "default:model-1".to_string(),
        provider_id: Some("default".to_string()),
        model_id: Some("model-1".to_string()),
        through_request_id: Some("req_tool_failed".to_string()),
        trigger_reason: "failed_response".to_string(),
        tokens_before: Some(4_000),
        prompt_tokens_estimate: None,
        estimate_source: None,
    };
    let compaction_config = CompactionRuntimeConfig {
        split_oversized_turns: true,
        ..CompactionRuntimeConfig::default()
    };

    compact_provider_context(
        &clock,
        &redactor,
        &mut run_state,
        &trigger,
        &compaction_config,
        &super::CompactionSummaryDecision::deterministic(&trigger),
    )
    .expect("tool-failure summary-only compaction should succeed")
    .expect("tool-failure summary-only compaction should write checkpoint");

    let events = read_events(&run_state.info.events_path);
    let written = events
        .iter()
        .find_map(|event| match &event.payload {
            EventV1::CompactionWritten(payload) if payload.trigger_reason == "failed_response" => {
                Some(payload.clone())
            }
            _ => None,
        })
        .expect("failed-response compaction written event");
    let checkpoint_path = run_state.info.run_dir.join(&written.artifact_path);
    let checkpoint_body = fs::read_to_string(&checkpoint_path).expect("read checkpoint artifact");
    let checkpoint: ProviderContextCheckpoint =
        serde_json::from_str(&checkpoint_body).expect("parse checkpoint artifact");
    assert!(checkpoint.recent_turns.is_empty());
    let tail_boundary = checkpoint.tail_boundary.expect("tail boundary");
    assert_eq!(tail_boundary.mode, "summary_only");
    assert_eq!(tail_boundary.split_prefix_summary, None);
    assert!(checkpoint.facts.compacted_turns.iter().any(|fact| {
        fact.status == ProviderConversationTurnStatus::Failed
            && fact.failure_stage.as_deref() == Some("tool_failure")
    }));
}

#[test]
fn split_oversized_turn_refuses_artifact_backed_turn() {
    let temp_dir = tempfile::tempdir().expect("tempdir");
    let clock = FakeClock::new();
    let redactor = DefaultRedactor::default();
    let mut run_state = test_run_state(temp_dir.path(), "run_split_refuses_artifact_turn");
    run_state.recorded_runtime_context = Some(compaction_runtime_context());
    run_state.provider_context_by_agent.insert(
        "agent_000001".to_string(),
        ProviderContext::from_turns(vec![ProviderConversationTurn {
            user_prompt: "artifact backed latest question".to_string(),
            assistant_response: format!(
                "ARTIFACT_PREFIX {} {} ARTIFACT_SUFFIX",
                "G".repeat(4_000),
                "H".repeat(7_900)
            ),
            request_id: Some("req_artifact".to_string()),
            artifacts: vec![EventArtifactRef {
                path: "artifacts/toolcalls/toolcall_000001/result.txt".to_string(),
                digest: Some("digest-artifact".to_string()),
            }],
            ..ProviderConversationTurn::default()
        }]),
    );
    let trigger = ProviderCompactionTrigger {
        agent_id: "agent_000001".to_string(),
        profile_name: "alpha".to_string(),
        model_ref: "default:model-1".to_string(),
        provider_id: Some("default".to_string()),
        model_id: Some("model-1".to_string()),
        through_request_id: Some("req_artifact".to_string()),
        trigger_reason: "overflow_retry".to_string(),
        tokens_before: Some(4_000),
        prompt_tokens_estimate: None,
        estimate_source: None,
    };
    let compaction_config = CompactionRuntimeConfig {
        split_oversized_turns: true,
        ..CompactionRuntimeConfig::default()
    };

    compact_provider_context(
        &clock,
        &redactor,
        &mut run_state,
        &trigger,
        &compaction_config,
        &super::CompactionSummaryDecision::deterministic(&trigger),
    )
    .expect("artifact-backed summary-only compaction should succeed")
    .expect("artifact-backed summary-only compaction should write checkpoint");

    let events = read_events(&run_state.info.events_path);
    let written = events
        .iter()
        .find_map(|event| match &event.payload {
            EventV1::CompactionWritten(payload) if payload.trigger_reason == "overflow_retry" => {
                Some(payload.clone())
            }
            _ => None,
        })
        .expect("overflow compaction written event");
    let checkpoint_path = run_state.info.run_dir.join(&written.artifact_path);
    let checkpoint_body = fs::read_to_string(&checkpoint_path).expect("read checkpoint artifact");
    let checkpoint: ProviderContextCheckpoint =
        serde_json::from_str(&checkpoint_body).expect("parse checkpoint artifact");
    assert!(checkpoint.recent_turns.is_empty());
    let tail_boundary = checkpoint.tail_boundary.expect("tail boundary");
    assert_eq!(tail_boundary.mode, "summary_only");
    assert_eq!(tail_boundary.split_prefix_summary, None);
    assert!(checkpoint
        .facts
        .relevant_artifacts
        .iter()
        .any(|artifact| { artifact.path == "artifacts/toolcalls/toolcall_000001/result.txt" }));
}

#[test]
fn split_oversized_turn_prefix_summary_in_checkpoint_facts() {
    let temp_dir = tempfile::tempdir().expect("tempdir");
    let clock = FakeClock::new();
    let redactor = DefaultRedactor::default();
    let mut run_state = test_run_state(temp_dir.path(), "run_split_prefix_summary_facts");
    run_state.recorded_runtime_context = Some(compaction_runtime_context());
    run_state.provider_context_by_agent.insert(
        "agent_000001".to_string(),
        ProviderContext::from_turns(vec![ProviderConversationTurn {
            user_prompt: "latest oversized facts question".to_string(),
            assistant_response: format!(
                "FACT_PREFIX_ANCHOR {} {} FACT_SUFFIX_ANCHOR",
                "I".repeat(4_000),
                "J".repeat(7_900)
            ),
            request_id: Some("req_fact".to_string()),
            ..ProviderConversationTurn::default()
        }]),
    );
    let trigger = ProviderCompactionTrigger {
        agent_id: "agent_000001".to_string(),
        profile_name: "alpha".to_string(),
        model_ref: "default:model-1".to_string(),
        provider_id: Some("default".to_string()),
        model_id: Some("model-1".to_string()),
        through_request_id: Some("req_fact".to_string()),
        trigger_reason: "overflow_retry".to_string(),
        tokens_before: Some(4_000),
        prompt_tokens_estimate: None,
        estimate_source: None,
    };
    let compaction_config = CompactionRuntimeConfig {
        split_oversized_turns: true,
        ..CompactionRuntimeConfig::default()
    };

    compact_provider_context(
        &clock,
        &redactor,
        &mut run_state,
        &trigger,
        &compaction_config,
        &super::CompactionSummaryDecision::deterministic(&trigger),
    )
    .expect("overflow split compaction should succeed")
    .expect("overflow split compaction should write checkpoint");

    let events = read_events(&run_state.info.events_path);
    let written = events
        .iter()
        .find_map(|event| match &event.payload {
            EventV1::CompactionWritten(payload) if payload.trigger_reason == "overflow_retry" => {
                Some(payload.clone())
            }
            _ => None,
        })
        .expect("overflow compaction written event");
    let checkpoint_path = run_state.info.run_dir.join(&written.artifact_path);
    let checkpoint_body = fs::read_to_string(&checkpoint_path).expect("read checkpoint artifact");
    let checkpoint_json: serde_json::Value =
        serde_json::from_str(&checkpoint_body).expect("parse checkpoint artifact json");
    assert_eq!(
        checkpoint_json["tail_boundary"]["mode"].as_str(),
        Some("split_oversized_turn_tail")
    );
    let split_prefix_summary = checkpoint_json["tail_boundary"]["split_prefix_summary"]
        .as_str()
        .expect("serialized split prefix summary");
    assert!(split_prefix_summary.contains("FACT_PREFIX_ANCHOR"));
    let summary = checkpoint_json["summary"].as_str().expect("summary");
    assert!(summary.contains("Split prefix summary"));
    assert!(summary.contains("Source facts: split prefix summary"));
    assert!(!checkpoint_json["recent_turns"][0]["assistant_response"]
        .as_str()
        .expect("recent assistant suffix")
        .contains("FACT_PREFIX_ANCHOR"));
}

#[test]
fn failed_response_compaction_does_not_double_compact_same_request() {
    let temp_dir = tempfile::tempdir().expect("tempdir");
    let agent_id = "agent_000001".to_string();
    let task_id = "task_000001".to_string();
    let request_id = "req_000001".to_string();
    let overflow_context = ProviderContext::from_turns(vec![long_turn("overflow question", 'A')]);

    let mut unchanged = test_run_state(temp_dir.path(), "run_failed_terminal_guard_unchanged");
    unchanged
        .provider_context_by_agent
        .insert(agent_id.clone(), overflow_context.clone());
    unchanged
        .overflow_retry_compacted_context_by_attempt
        .insert(
            (task_id.clone(), request_id.clone()),
            overflow_context.clone(),
        );
    let failed_request = FailedTerminalCompactionRequest {
        task_id: task_id.clone(),
        agent_id: agent_id.clone(),
        request_id: request_id.clone(),
        trigger_reason: "failed_response".to_string(),
    };
    assert!(
        !mark_failed_terminal_compaction_attempt(&mut unchanged, &failed_request),
        "unchanged context already checkpointed by overflow retry should not compact again"
    );
    let aborted_request = FailedTerminalCompactionRequest {
        trigger_reason: "aborted_response".to_string(),
        ..failed_request.clone()
    };
    assert!(
        !mark_failed_terminal_compaction_attempt(&mut unchanged, &aborted_request),
        "same task/request must not run both failed and aborted terminal compaction"
    );

    let mut changed = test_run_state(temp_dir.path(), "run_failed_terminal_guard_changed");
    changed
        .provider_context_by_agent
        .insert(agent_id.clone(), overflow_context.clone());
    changed
        .overflow_retry_compacted_context_by_attempt
        .insert((task_id.clone(), request_id.clone()), overflow_context);
    changed
        .provider_context_by_agent
        .get_mut(&agent_id)
        .expect("agent context")
        .push_turn(ProviderConversationTurn {
            user_prompt: "failed retry question".to_string(),
            assistant_response: "partial retry output".to_string(),
            status: ProviderConversationTurnStatus::Failed,
            failure_stage: Some("overflow_retry_failed".to_string()),
            failure_reason: Some("overflow persisted".to_string()),
            request_id: Some("req_provider_retry".to_string()),
            ..ProviderConversationTurn::default()
        });
    assert!(
        mark_failed_terminal_compaction_attempt(&mut changed, &failed_request),
        "appending the failed turn materially changes context and permits terminal compaction"
    );
    assert!(
        !mark_failed_terminal_compaction_attempt(&mut changed, &aborted_request),
        "terminal compaction is still one-shot per task/request after a real attempt"
    );
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
            split_prefix_summary: None,
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
            summary_contract_version: Some(super::PROVIDER_CONTEXT_SUMMARY_CONTRACT_VERSION),
            summary_contract_enforced: Some(true),
        },
        &CompactionRuntimeConfig::default(),
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
                    estimate_source: None,
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
                    estimate_source: None,
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
fn failed_turn_context_resume_reconstructs_failed_turn_after_checkpoint() {
    let temp_dir = tempfile::tempdir().expect("tempdir");
    let run_id = "run_restore_failed_turn_after_checkpoint";
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
                through_seq: 5,
                through_request_id: Some("req_000001".to_string()),
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
                user_prompt: "first question".to_string(),
                assistant_response: "first answer".to_string(),
                request_id: Some("req_000001".to_string()),
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
                EventV1::ProviderStreamDelta(ProviderStreamDeltaEvent {
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
                EventActor::new(ActorKind::System, Some("coordinator".to_string())),
                None,
                EventV1::CompactionWritten(CompactionWrittenEvent {
                    checkpoint_id: "checkpoint_000010".to_string(),
                    agent_id: "agent_000001".to_string(),
                    artifact_path: checkpoint_rel.clone(),
                    artifact_digest: Some("digest-checkpoint".to_string()),
                    artifact_bytes: 512,
                    trigger_reason: "proactive".to_string(),
                    through_seq: 5,
                    through_request_id: Some("req_000001".to_string()),
                    provider_id: Some("default".to_string()),
                    model_id: Some("model-1".to_string()),
                    tokens_before: Some(3_900),
                    tokens_before_estimate: None,
                    tokens_after_estimate: None,
                    summary_tokens_estimate: None,
                    compacted_turns: None,
                    reduction_tokens_estimate: None,
                    reduction_percent_estimate: None,
                    estimate_source: None,
                    preserved_turns: 1,
                }),
            ),
            restore_fixture_event(
                run_id,
                6,
                EventActor::new(ActorKind::System, Some("coordinator".to_string())),
                None,
                EventV1::CompactionApplied(CompactionAppliedEvent {
                    checkpoint_id: "checkpoint_000010".to_string(),
                    agent_id: "agent_000001".to_string(),
                    through_seq: 5,
                    through_request_id: Some("req_000001".to_string()),
                    tokens_before_estimate: None,
                    tokens_after_estimate: None,
                    summary_tokens_estimate: None,
                    compacted_turns: None,
                    preserved_turns: None,
                    reduction_tokens_estimate: None,
                    reduction_percent_estimate: None,
                    estimate_source: None,
                }),
            ),
            restore_fixture_event(
                run_id,
                7,
                EventActor::new(ActorKind::User, None),
                None,
                EventV1::UserMessageSubmitted(UserMessageSubmittedEvent {
                    request_id: "req_000002".to_string(),
                    text: "failed question".to_string(),
                }),
            ),
            restore_fixture_event(
                run_id,
                8,
                EventActor::new(ActorKind::Worker, Some("agent_000001".to_string())),
                Some("req_000002"),
                EventV1::TaskScheduled(TaskScheduledEvent {
                    task_id: "task_000002".to_string(),
                    state: TaskScheduleState::Started,
                    queue_key: Some("provider_model:default:model-1".to_string()),
                }),
            ),
            restore_fixture_event(
                run_id,
                9,
                EventActor::new(ActorKind::Worker, Some("agent_000001".to_string())),
                Some("req_000002"),
                EventV1::ProviderRequestStarted(ProviderRequestStartedEvent {
                    request_id: "req_000002_provider".to_string(),
                    provider_id: "default".to_string(),
                    model_id: "model-1".to_string(),
                    prompt_summary: "failed question".to_string(),
                    request_digest: "digest-2".to_string(),
                    metadata: None,
                }),
            ),
            restore_fixture_event(
                run_id,
                10,
                EventActor::new(ActorKind::Worker, Some("agent_000001".to_string())),
                Some("req_000002"),
                EventV1::ProviderStreamDelta(ProviderStreamDeltaEvent {
                    request_id: "req_000002_provider".to_string(),
                    delta: "partial failed answer".to_string(),
                }),
            ),
            restore_fixture_event(
                run_id,
                11,
                EventActor::new(ActorKind::Worker, Some("agent_000001".to_string())),
                Some("req_000002"),
                EventV1::ProviderRequestFinished(ProviderRequestFinishedEvent {
                    request_id: "req_000002_provider".to_string(),
                    finish_reason: "error".to_string(),
                    output_digest: None,
                    usage: None,
                    metadata: None,
                }),
            ),
            restore_fixture_event(
                run_id,
                12,
                EventActor::new(ActorKind::Worker, Some("agent_000001".to_string())),
                Some("req_000002"),
                EventV1::TaskCancelled(TaskCancelledEvent {
                    task_id: "task_000002".to_string(),
                    reason: "provider exploded".to_string(),
                    task_scope: Some(crate::event::TaskTerminalScope::AgentTurn),
                }),
            ),
        ],
    );

    let restored = restore_provider_context_from_history(temp_dir.path(), run_id)
        .expect("restore checkpointed provider context");
    let context = restored
        .get("agent_000001")
        .expect("checkpointed agent context");
    assert_eq!(context.preserved_turns.len(), 2);
    let failed_turn = &context.preserved_turns[1];
    assert_eq!(failed_turn.user_prompt, "failed question");
    assert_eq!(failed_turn.assistant_response, "partial failed answer");
    assert_eq!(failed_turn.status, ProviderConversationTurnStatus::Failed);
    assert_eq!(failed_turn.failure_stage.as_deref(), Some("provider_error"));
    assert_eq!(
        failed_turn.failure_reason.as_deref(),
        Some("provider exploded")
    );
    assert_eq!(
        failed_turn.request_id.as_deref(),
        Some("req_000002_provider")
    );
    assert_eq!(failed_turn.first_seq, Some(7));
    assert_eq!(failed_turn.last_seq, Some(12));
}

#[test]
fn failed_turn_context_does_not_duplicate_completed_turns() {
    let temp_dir = tempfile::tempdir().expect("tempdir");
    let run_id = "run_restore_no_duplicate_completed_turn";
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
                EventActor::new(ActorKind::User, None),
                None,
                EventV1::UserMessageSubmitted(UserMessageSubmittedEvent {
                    request_id: "req_000001".to_string(),
                    text: "completed question".to_string(),
                }),
            ),
            restore_fixture_event(
                run_id,
                3,
                EventActor::new(ActorKind::Worker, Some("agent_000001".to_string())),
                Some("req_000001"),
                EventV1::TaskScheduled(TaskScheduledEvent {
                    task_id: "task_000001".to_string(),
                    state: TaskScheduleState::Started,
                    queue_key: Some("provider_model:default:model-1".to_string()),
                }),
            ),
            restore_fixture_event(
                run_id,
                4,
                EventActor::new(ActorKind::Worker, Some("agent_000001".to_string())),
                Some("req_000001"),
                EventV1::ProviderRequestStarted(ProviderRequestStartedEvent {
                    request_id: "req_000001_provider".to_string(),
                    provider_id: "default".to_string(),
                    model_id: "model-1".to_string(),
                    prompt_summary: "completed question".to_string(),
                    request_digest: "digest-1".to_string(),
                    metadata: None,
                }),
            ),
            restore_fixture_event(
                run_id,
                5,
                EventActor::new(ActorKind::Worker, Some("agent_000001".to_string())),
                Some("req_000001"),
                EventV1::ProviderStreamDelta(ProviderStreamDeltaEvent {
                    request_id: "req_000001_provider".to_string(),
                    delta: "completed answer".to_string(),
                }),
            ),
            restore_fixture_event(
                run_id,
                6,
                EventActor::new(ActorKind::Worker, Some("agent_000001".to_string())),
                Some("req_000001"),
                EventV1::ProviderRequestFinished(ProviderRequestFinishedEvent {
                    request_id: "req_000001_provider".to_string(),
                    finish_reason: "done".to_string(),
                    output_digest: Some("digest-out-1".to_string()),
                    usage: None,
                    metadata: None,
                }),
            ),
            restore_fixture_event(
                run_id,
                7,
                EventActor::new(ActorKind::Worker, Some("agent_000001".to_string())),
                Some("req_000001"),
                EventV1::TaskCompleted(TaskCompletedEvent {
                    task_id: "task_000001".to_string(),
                    result_summary: "completed answer".to_string(),
                    result_digest: "digest-task-1".to_string(),
                    metadata: None,
                }),
            ),
            restore_fixture_event(
                run_id,
                8,
                EventActor::new(ActorKind::Worker, Some("agent_000001".to_string())),
                Some("req_000001"),
                EventV1::TaskCancelled(TaskCancelledEvent {
                    task_id: "task_000001".to_string(),
                    reason: "late cancellation after completed turn".to_string(),
                    task_scope: Some(crate::event::TaskTerminalScope::AgentTurn),
                }),
            ),
        ],
    );

    let restored = restore_provider_context_from_history(temp_dir.path(), run_id)
        .expect("restore provider context");
    let context = restored
        .get("agent_000001")
        .expect("agent context should be restored");
    assert_eq!(context.preserved_turns.len(), 1);
    assert_eq!(context.preserved_turns[0].user_prompt, "completed question");
    assert_eq!(
        context.preserved_turns[0].assistant_response,
        "completed answer"
    );
    assert_eq!(
        context.preserved_turns[0].status,
        ProviderConversationTurnStatus::Completed
    );
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
                    estimate_source: None,
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
                    estimate_source: None,
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
        failed_terminal_compaction_attempts: std::collections::BTreeSet::new(),
        overflow_retry_compacted_context_by_attempt: std::collections::BTreeMap::new(),
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

fn provider_context_compaction_plan_fixture() -> ProviderContextCompactionPlan {
    ProviderContextCompactionPlan {
        older_turns: vec![ProviderConversationTurn {
            user_prompt: "model validation compacted question".to_string(),
            assistant_response: "model validation compacted answer".to_string(),
            ..ProviderConversationTurn::default()
        }],
        recent_turns: vec![ProviderConversationTurn {
            user_prompt: "recent question".to_string(),
            assistant_response: "recent answer".to_string(),
            ..ProviderConversationTurn::default()
        }],
        pruned_tool_artifacts: Vec::new(),
        facts: ProviderCompactionFacts::default(),
        tail_boundary: ProviderCompactionTailBoundary {
            mode: "whole_turn_tail".to_string(),
            preserved_turns: 1,
            preserved_tokens_estimate: 10,
            preserved_from_request_id: None,
            preserved_from_seq: None,
            split_prefix_summary: None,
            note: None,
        },
    }
}

fn compact_operational_memory_fixture(
    session_dir: &Path,
    run_id: &str,
    first_answer: &str,
    second_answer: &str,
    clock: &FakeClock,
    redactor: &DefaultRedactor,
) -> ProviderContextCheckpoint {
    let mut run_state = test_run_state(session_dir, run_id);
    run_state.recorded_runtime_context = Some(compaction_runtime_context());
    run_state.provider_context_by_agent.insert(
        "agent_000001".to_string(),
        ProviderContext::from_turns(vec![
            ProviderConversationTurn {
                user_prompt: "first question".to_string(),
                assistant_response: first_answer.to_string(),
                request_id: Some("req_000001".to_string()),
                first_seq: Some(2),
                last_seq: None,
                ..ProviderConversationTurn::default()
            },
            ProviderConversationTurn {
                user_prompt: "second question".to_string(),
                assistant_response: second_answer.to_string(),
                request_id: Some("req_000002".to_string()),
                first_seq: None,
                last_seq: None,
                ..ProviderConversationTurn::default()
            },
        ]),
    );
    run_state.next_event_seq = read_events(&run_state.info.events_path)
        .last()
        .map(|event| event.seq.saturating_add(1))
        .unwrap_or(1);
    let trigger = ProviderCompactionTrigger {
        agent_id: "agent_000001".to_string(),
        profile_name: "alpha".to_string(),
        model_ref: "default:model-1".to_string(),
        provider_id: Some("default".to_string()),
        model_id: Some("model-1".to_string()),
        through_request_id: Some("req_000002".to_string()),
        trigger_reason: "proactive".to_string(),
        tokens_before: Some(3_900),
        prompt_tokens_estimate: None,
        estimate_source: None,
    };
    compact_provider_context(
        clock,
        redactor,
        &mut run_state,
        &trigger,
        &CompactionRuntimeConfig::default(),
        &super::CompactionSummaryDecision::deterministic(&trigger),
    )
    .expect("operational-memory compaction should succeed")
    .expect("operational-memory compaction should write a checkpoint");

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
    serde_json::from_str(&checkpoint_body).expect("parse checkpoint artifact")
}

fn operational_memory_history_events(
    run_id: &str,
    first_answer: &str,
    second_answer: &str,
    before_finish: Vec<EventV1>,
    after_finish: Vec<EventV1>,
) -> Vec<EventEnvelopeV1> {
    let mut events = vec![
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
            EventActor::new(ActorKind::User, None),
            None,
            EventV1::UserMessageSubmitted(UserMessageSubmittedEvent {
                request_id: "req_000001".to_string(),
                text: "first question".to_string(),
            }),
        ),
        restore_fixture_event(
            run_id,
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
    ];
    let mut seq = 4_u64;
    for payload in before_finish.into_iter().chain(after_finish) {
        events.push(restore_fixture_event(
            run_id,
            seq,
            EventActor::new(ActorKind::Worker, Some("agent_000001".to_string())),
            Some("req_000001"),
            payload,
        ));
        seq += 1;
    }
    events.push(restore_fixture_event(
        run_id,
        seq,
        EventActor::new(ActorKind::Worker, Some("agent_000001".to_string())),
        Some("req_000001"),
        EventV1::TaskCompleted(TaskCompletedEvent {
            task_id: "task_000001".to_string(),
            result_summary: first_answer.to_string(),
            result_digest: "digest-task-1".to_string(),
            metadata: None,
        }),
    ));
    seq += 1;
    events.push(restore_fixture_event(
        run_id,
        seq,
        EventActor::new(ActorKind::User, None),
        None,
        EventV1::UserMessageSubmitted(UserMessageSubmittedEvent {
            request_id: "req_000002".to_string(),
            text: "second question".to_string(),
        }),
    ));
    seq += 1;
    events.push(restore_fixture_event(
        run_id,
        seq,
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
    ));
    seq += 1;
    events.push(restore_fixture_event(
        run_id,
        seq,
        EventActor::new(ActorKind::Worker, Some("agent_000001".to_string())),
        Some("req_000002"),
        EventV1::TaskCompleted(TaskCompletedEvent {
            task_id: "task_000002".to_string(),
            result_summary: second_answer.to_string(),
            result_digest: "digest-task-2".to_string(),
            metadata: None,
        }),
    ));
    events.sort_by_key(|event| event.seq);
    events
}

fn tool_metadata(canonical_tool_id: &str) -> ToolCallMetadata {
    ToolCallMetadata {
        canonical_tool_id: Some(canonical_tool_id.to_string()),
        alias_source_tool_id: None,
        lineage: None,
        artifact_refs: Vec::new(),
        timing: None,
        hook_executions: Vec::new(),
    }
}

fn checkpoint_metadata_for_run(
    run_id: &str,
    checkpoint_id: &str,
    through_seq: u64,
) -> ProviderContextCheckpointMetadata {
    ProviderContextCheckpointMetadata {
        checkpoint_id: checkpoint_id.to_string(),
        agent_id: "agent_000001".to_string(),
        run_id: run_id.to_string(),
        through_seq,
        through_request_id: Some("req_000001".to_string()),
        provider_id: Some("default".to_string()),
        model_id: Some("model-1".to_string()),
        tokens_before: None,
        tokens_before_estimate: None,
        tokens_after_estimate: None,
        summary_tokens_estimate: None,
        compacted_turns: None,
        preserved_turns: None,
        reduction_tokens_estimate: None,
        reduction_percent_estimate: None,
        trigger_reason: Some("proactive".to_string()),
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
