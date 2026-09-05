use crate::UnwrapOrAbort;
use std::collections::BTreeMap;
use std::fs;
use std::path::Path;
use std::process::ExitStatus;
use std::sync::{Arc, Mutex};

#[cfg(unix)]
use std::os::unix::process::ExitStatusExt;

use async_trait::async_trait;
use serde_json::json;
use tokio::sync::{mpsc, oneshot};
use tokio_stream::StreamExt;
use tokio_util::sync::CancellationToken;

use super::permission::always_approve_can_bypass;
use crate::agent::{
    AgentModelSettings, AgentProfile, ProviderContext, ProviderConversationTurn,
    ProviderConversationTurnStatus,
};
use crate::clock::{FakeClock, RealClock};
use crate::config::{
    load_config_from_str, resolve_profile_model_metadata, CompactionRuntimeConfig,
    HookLifecycleEvent, HookRuntimeConfig, HooksConfig, LifecycleHookConfig, PermissionMode,
    ProfilePermissions, ShellAllowlist,
};
use crate::context_budget::{BudgetStatus, RequestBudgetComponents, RequestBudgetSnapshot};
use crate::conversation::{
    ConversationAssistantMessage, ConversationMessage, ConversationToolCall,
    ConversationToolResultMessage, ConversationUserMessage,
};
use crate::event::{
    ActorKind, AgentSpawnedEvent, ArtifactWrittenEvent, BackgroundTaskNotificationStatus,
    EditAppliedEvent, EventActor, EventArtifactRef, EventEnvelopeV1, EventV1,
    HookExecutionMetadata, HookExecutionStatus, ProviderRequestFinishedEvent,
    ProviderRequestStartedEvent, ProviderStreamDeltaEvent, RunFinishedEvent, RunStartedEvent,
    TaskCancelledEvent, TaskCompletedEvent, TaskScheduleState, TaskScheduledEvent,
    ToolCallFinishedEvent, ToolCallMetadata, ToolCallRequestedEvent, ToolCallStatus,
    ToolIdentityMetadata, UserMessageSubmittedEvent, SCHEMA_VERSION,
};
use crate::perm::{
    PermissionDecision, PermissionGrant, PermissionGrantMatcher, PermissionGrantRequest,
    PermissionGrantScope, PermissionKind, PermissionPolicy, PermissionRuleRequest,
    PermissionToolSelector,
};
use crate::proj::RecordedRuntimeContext;
use crate::redact::DefaultRedactor;
use crate::sched::{ConcurrencyKey, ScheduleDecision, Scheduler, SchedulerLimits};
use crate::store::JsonlFileEventStore;
use crate::tool::{
    ArtifactRef, Tool, ToolCapability, ToolContext, ToolError, ToolRegistry, ToolResult,
    ToolRunState,
};

use super::{
    append_artifact_written_event, append_background_task_notification_and_schedule,
    append_edit_applied_event, append_edit_proposed_event, append_edit_rejected_event,
    append_payload_event, append_payload_event_with_correlation,
    append_permission_grant_recorded_event, append_permission_requested_event,
    append_tool_call_finished_event, append_tool_call_requested_event,
    append_tool_call_started_event, compact_session, completion_messages_to_conversation_messages,
    permission_rule_request_selectors, provider_tool_message_status,
    restore_provider_context_from_history, schedule_pending_agent_wakeups_for_idle_agent,
    spawn_coordinator, summarize_hook_output, system_actor, AppliedCompaction, ChildTaskTurnState,
    Coordinator, CoordinatorConfig, CoordinatorError, EditAppliedEventArgs,
    FailedTerminalCompactionRequest, HashlineEditMetadata, HookExecutionBatch,
    HookInvocationContext, JobOutcome, JobProgressKind, PendingPermissionResolution,
    PendingPermissionState, PermissionRequestedEventArgs, ProviderCompactionTrigger,
    QueuedAgentTurn, RunInfo, RunState, RunningAgentTurn, TaskExecutionState, TaskState,
    TokioLifecycleHookCommandExecutor, ToolCallFinishedEventArgs, ToolCallRequestedEventArgs,
};
use harness_providers::{CompletionMessage, MessageRole, ProviderOutputCapDisposition};

fn pressured_compaction_budget() -> RequestBudgetSnapshot {
    RequestBudgetSnapshot {
        status: BudgetStatus::Estimated,
        requested_output_tokens: Some(1),
        reserved_output_tokens: Some(1),
        maximum_input_tokens: Some(u32::MAX),
        safety_margin_tokens: 0,
        compaction_threshold_tokens: Some(u32::MAX),
        components: RequestBudgetComponents {
            history_tokens: u32::MAX,
            ..RequestBudgetComponents::default()
        },
        occupied_input_tokens: u32::MAX,
        remaining_input_tokens: Some(0),
        requires_compaction: Some(true),
        output_cap_disposition: ProviderOutputCapDisposition::Emitted(1),
    }
}

use super::hooks::{
    LifecycleHookCommandExecutor, LifecycleHookCommandInvocation, LifecycleHookCommandOutput,
};

macro_rules! delegate_test {
    ($name:ident => $target:path) => {
        #[test]
        fn $name() {
            $target();
        }
    };
}

macro_rules! delegate_tokio_test {
    ($name:ident => $target:path) => {
        #[tokio::test]
        async fn $name() {
            $target().await;
        }
    };
}

struct TestShellTool;

struct TestTaskTool;

struct TestMcpEchoTool;

struct TestMcpWrapperTool;

struct FakeLifecycleHookCommandExecutor {
    invocations: Mutex<Vec<LifecycleHookCommandInvocation>>,
}

impl FakeLifecycleHookCommandExecutor {
    fn new() -> Self {
        Self {
            invocations: Mutex::new(Vec::new()),
        }
    }
}

#[async_trait]
impl LifecycleHookCommandExecutor for FakeLifecycleHookCommandExecutor {
    async fn execute(
        &self,
        invocation: LifecycleHookCommandInvocation,
    ) -> Result<LifecycleHookCommandOutput, String> {
        self.invocations.lock().unwrap_or_abort().push(invocation);
        Ok(LifecycleHookCommandOutput {
            status: success_exit_status(),
            stdout: "hook stdout".to_string(),
            stderr: String::new(),
        })
    }
}

#[cfg(unix)]
fn success_exit_status() -> ExitStatus {
    ExitStatus::from_raw(0)
}

#[tokio::test]
async fn lifecycle_hooks_use_injected_executor_without_spawning() {
    let temp_dir = tempfile::tempdir().unwrap_or_abort();
    let executor = FakeLifecycleHookCommandExecutor::new();
    let runtime = HookRuntimeConfig {
        hooks: HooksConfig {
            lifecycle: vec![LifecycleHookConfig {
                id: Some("fake-hook".to_string()),
                event: HookLifecycleEvent::RunStarted,
                command: vec!["fake-hook-bin".to_string(), "--flag".to_string()],
                cwd: Some(".".to_string()),
                timeout_ms: 123,
                critical: true,
                env: BTreeMap::from([("CUSTOM".to_string(), "value".to_string())]),
            }],
        },
        shell_allowlist: ShellAllowlist {
            executables: vec!["fake-hook-bin".to_string()],
            cwd_roots: Vec::new(),
            ..ShellAllowlist::default()
        },
        suppress_execution: false,
    };
    let clock = FakeClock::new();

    let batch = super::hooks::run_lifecycle_hooks(
        &clock,
        &executor,
        &runtime,
        HookInvocationContext {
            event: HookLifecycleEvent::RunStarted,
            run_id: "run_fake_hook".into(),
            workspace_root: temp_dir.path().to_path_buf(),
            artifacts_dir: temp_dir.path().join("artifacts"),
            actor: Some(EventActor::new(ActorKind::System, None)),
            agent_id: None,
            request_id: None,
            permission_id: None,
            task_id: None,
            tool_call_id: None,
            tool_id: None,
            provider_id: None,
            model_id: None,
            parent_agent_id: None,
            profile: None,
            outcome: Some("started".to_string()),
            output_summary: None,
            failure_reason: None,
        },
    )
    .await;

    assert_eq!(batch.critical_failure, None);
    assert_eq!(batch.hook_executions.len(), 1);
    assert_eq!(
        batch.hook_executions[0].status,
        HookExecutionStatus::Succeeded
    );
    assert_eq!(
        batch.hook_executions[0].output_summary.as_deref(),
        Some("hook stdout")
    );
    let invocations = executor.invocations.lock().unwrap_or_abort();
    assert_eq!(invocations.len(), 1);
    assert_eq!(invocations[0].executable, "fake-hook-bin");
    assert_eq!(invocations[0].args, vec!["--flag"]);
    assert_eq!(invocations[0].cwd, temp_dir.path());
    assert_eq!(invocations[0].timeout_ms, 123);
    assert_eq!(
        invocations[0].env.get("CUSTOM").map(String::as_str),
        Some("value")
    );
    assert_eq!(
        invocations[0]
            .env
            .get("HARNESS_HOOK_EVENT")
            .map(String::as_str),
        Some("run_started")
    );
}

#[tokio::test]
async fn lifecycle_hooks_scoped_to_declared_event_invoke_nothing_else() {
    // arrange — one hook declared for run_started only
    let temp_dir = tempfile::tempdir().unwrap_or_abort();
    let executor = FakeLifecycleHookCommandExecutor::new();
    let runtime = HookRuntimeConfig {
        hooks: HooksConfig {
            lifecycle: vec![LifecycleHookConfig {
                id: Some("scoped-hook".to_string()),
                event: HookLifecycleEvent::RunStarted,
                command: vec!["fake-hook-bin".to_string()],
                cwd: None,
                timeout_ms: 100,
                critical: true,
                env: BTreeMap::new(),
            }],
        },
        shell_allowlist: ShellAllowlist {
            executables: vec!["fake-hook-bin".to_string()],
            cwd_roots: Vec::new(),
            ..ShellAllowlist::default()
        },
        suppress_execution: false,
    };
    let clock = FakeClock::new();

    // act — fire a different lifecycle event
    let batch = super::hooks::run_lifecycle_hooks(
        &clock,
        &executor,
        &runtime,
        HookInvocationContext {
            event: HookLifecycleEvent::ToolCallStarted,
            run_id: "run_scoped".into(),
            workspace_root: temp_dir.path().to_path_buf(),
            artifacts_dir: temp_dir.path().join("artifacts"),
            actor: Some(EventActor::new(ActorKind::System, None)),
            agent_id: None,
            request_id: None,
            permission_id: None,
            task_id: None,
            tool_call_id: None,
            tool_id: None,
            provider_id: None,
            model_id: None,
            parent_agent_id: None,
            profile: None,
            outcome: None,
            output_summary: None,
            failure_reason: None,
        },
    )
    .await;

    // assert — discovery is event-scoped: nothing runs, nothing is recorded
    assert_eq!(batch.hook_executions.len(), 0);
    assert_eq!(batch.critical_failure, None);
    assert!(executor.invocations.lock().unwrap_or_abort().is_empty());
}

#[test]
fn summarize_hook_output_preserves_existing_summary_contract() {
    assert_eq!(summarize_hook_output("  stdout only  ", ""), "stdout only");
    assert_eq!(summarize_hook_output("", "\nstderr only\n"), "stderr only");
    assert_eq!(
        summarize_hook_output("stdout", "stderr"),
        "stdout/stderr captured"
    );
    assert_eq!(summarize_hook_output(" \n", "\t"), "no output");
}

#[test]
fn summarize_hook_output_truncates_long_single_stream_output() {
    let summary = summarize_hook_output(&"x".repeat(161), "");

    assert_eq!(summary.chars().count(), 161);
    assert!(summary.starts_with(&"x".repeat(160)));
    assert!(summary.ends_with('…'));
}

#[test]
fn permission_rule_request_selectors_extract_edit_file_alias() {
    // arrange
    let temp_dir = tempfile::tempdir().unwrap_or_abort();

    // act
    let selectors = permission_rule_request_selectors(
        temp_dir.path(),
        PermissionKind::EditFs,
        &json!({ "file": "src/lib.rs" }),
    );

    // assert
    assert_eq!(
        selectors,
        vec![PermissionRuleRequest::WorkspacePath(
            "src/lib.rs".to_string()
        )]
    );
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
impl Tool for TestTaskTool {
    fn id(&self) -> &str {
        "task"
    }

    fn capability(&self) -> ToolCapability {
        ToolCapability::SpawnAgent
    }

    async fn call(
        &self,
        _ctx: ToolContext,
        args_json: serde_json::Value,
    ) -> Result<ToolResult, ToolError> {
        Ok(ToolResult::text(format!("task ok {args_json}")))
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
        Ok(ToolResult::structured(
            text,
            serde_json::json!({
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
            }),
        ))
    }
}

fn fake_mcp_echo_result(text: &str) -> ToolResult {
    ToolResult::structured(
        text,
        serde_json::json!({
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
        }),
    )
}

fn test_tool_registry() -> Arc<ToolRegistry> {
    let mut registry = ToolRegistry::new();
    registry.register(Arc::new(TestShellTool));
    registry.register(Arc::new(TestTaskTool));
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

fn test_agent_profile(name: &str) -> AgentProfile {
    AgentProfile {
        name: name.to_string(),
        model_ref: "mock:model-1".to_string(),
        model_ref_explicit: true,
        system_prompt: "sys".to_string(),
        cache_retention: Default::default(),
        max_iters: Some(1),
        temperature: None,
        tool_failure_mode: crate::config::ToolFailureMode::FailTurn,
        toolset: Vec::new(),
        permission_ruleset: Vec::new(),
    }
}

#[tokio::test]
async fn fresh_run_agent_ids_skip_existing_child_session_directories() {
    let temp_dir = tempfile::tempdir().unwrap_or_abort();
    fs::create_dir_all(temp_dir.path().join("agent_000001")).unwrap_or_abort();
    let stale_child_dir = temp_dir.path().join("agent_000002");
    fs::create_dir_all(&stale_child_dir).unwrap_or_abort();
    fs::write(stale_child_dir.join(".writer.lock"), "").unwrap_or_abort();
    fs::write(stale_child_dir.join("events.jsonl"), "").unwrap_or_abort();

    let mut config = test_config(temp_dir.path());
    config
        .agent_profiles
        .insert("alpha".to_string(), test_agent_profile("alpha"));
    let handle = spawn_coordinator(
        config,
        Arc::new(FakeClock::new()),
        Arc::new(DefaultRedactor::default()),
    );

    let run = handle
        .start_run("fresh skips old child dirs", temp_dir.path())
        .await
        .unwrap_or_abort();
    let supervisor = EventActor::new(ActorKind::Supervisor, None);
    let parent_agent_id = handle
        .spawn_agent(supervisor.clone(), "alpha", None)
        .await
        .unwrap_or_abort();
    let child_agent_id = handle
        .spawn_agent_idle(supervisor, "alpha", Some(parent_agent_id.clone()))
        .await
        .unwrap_or_abort();

    assert_eq!(parent_agent_id, "agent_000003");
    assert_eq!(child_agent_id, "agent_000004");
    assert!(temp_dir.path().join("agent_000004/events.jsonl").exists());
    assert_eq!(
        fs::read_to_string(stale_child_dir.join(".writer.lock")).unwrap_or_abort(),
        ""
    );
    assert!(run.run_dir.ends_with("run_000001"));
}

#[tokio::test]
async fn canonical_commit_updates_history_index_before_first_list() {
    // arrange
    let temp_dir = tempfile::tempdir().unwrap_or_abort();
    let config = test_config(temp_dir.path());
    let handle = spawn_coordinator(
        config,
        Arc::new(FakeClock::new()),
        Arc::new(DefaultRedactor::default()),
    );

    // act
    let run = handle
        .start_run("indexed immediately", temp_dir.path())
        .await
        .unwrap_or_abort();

    // assert
    let index_path = temp_dir.path().join(".session-history-index-v1.json");
    let index: serde_json::Value =
        serde_json::from_slice(&fs::read(index_path).unwrap_or_abort()).unwrap_or_abort();
    assert_eq!(
        index["entries"][run.run_dir.to_str().unwrap_or_abort()]["entry"]["catalog"]["run_id"],
        run.run_id.to_string()
    );
}

#[tokio::test]
async fn concurrent_commit_updates_do_not_lose_rows() {
    // arrange
    let temp_dir = tempfile::tempdir().unwrap_or_abort();
    let mut first_config = test_config(temp_dir.path());
    first_config.run_id_override = Some("run_index_first".to_string());
    let mut second_config = test_config(temp_dir.path());
    second_config.run_id_override = Some("run_index_second".to_string());
    let first = spawn_coordinator(
        first_config,
        Arc::new(FakeClock::new()),
        Arc::new(DefaultRedactor::default()),
    );
    let second = spawn_coordinator(
        second_config,
        Arc::new(FakeClock::new()),
        Arc::new(DefaultRedactor::default()),
    );

    // act
    let (first_run, second_run) = tokio::join!(
        first.start_run("first", temp_dir.path()),
        second.start_run("second", temp_dir.path())
    );
    let first_run = first_run.unwrap_or_abort();
    let second_run = second_run.unwrap_or_abort();

    // assert
    let index_path = temp_dir.path().join(".session-history-index-v1.json");
    let index: serde_json::Value =
        serde_json::from_slice(&fs::read(index_path).unwrap_or_abort()).unwrap_or_abort();
    assert!(index["entries"]
        .get(first_run.run_dir.to_str().unwrap_or_abort())
        .is_some());
    assert!(index["entries"]
        .get(second_run.run_dir.to_str().unwrap_or_abort())
        .is_some());
}

fn shell_permission_policy(shell_mode: PermissionMode) -> PermissionPolicy {
    PermissionPolicy::new(PermissionMode::Deny, shell_mode, PermissionMode::Deny)
}

fn allow_shell_permission_policy() -> PermissionPolicy {
    shell_permission_policy(PermissionMode::Allow)
}

fn ask_shell_permission_policy(timeout_ms: u64) -> PermissionPolicy {
    shell_permission_policy(PermissionMode::Ask).with_ask_timeout_ms(timeout_ms)
}

#[cfg(test)]
#[path = "tests/append_permission_tests.rs"]
mod append_permission_tests;
#[cfg(test)]
#[path = "tests/permission_flow_tests.rs"]
mod permission_flow_tests;

delegate_tokio_test!(permission_rule_bash_selector_is_enforced_at_tool_call_site => permission_flow_tests::rule_permission_rule_bash_selector_is_enforced_at_tool_call_site);
delegate_test!(task_permission_rule_selector_uses_only_subagent_type => permission_flow_tests::rule_task_permission_rule_selector_uses_only_subagent_type);
delegate_tokio_test!(permission_rule_task_selector_is_enforced_at_tool_call_site => permission_flow_tests::rule_permission_rule_task_selector_is_enforced_at_tool_call_site);
delegate_tokio_test!(perm_ask_path_blocks_until_resolved => permission_flow_tests::rule_perm_ask_path_blocks_until_resolved);
delegate_tokio_test!(allow_always_records_grant_and_authorizes_matching_future_shell_call => permission_flow_tests::allow_always_records_grant_and_authorizes_matching_future_shell_call);
delegate_tokio_test!(allow_always_shell_run_grant_does_not_authorize_changed_args => permission_flow_tests::allow_always_shell_run_grant_does_not_authorize_changed_args);
delegate_tokio_test!(always_approve_mode_bypasses_future_ordinary_permission_prompts => permission_flow_tests::always_approve_mode_bypasses_future_ordinary_permission_prompts);
delegate_tokio_test!(enabling_always_approve_drains_pending_ordinary_permission => permission_flow_tests::enabling_always_approve_drains_pending_ordinary_permission);
delegate_tokio_test!(disabling_always_approve_restores_ordinary_permission_prompts => permission_flow_tests::disabling_always_approve_restores_ordinary_permission_prompts);
delegate_tokio_test!(always_approve_mode_keeps_questions_promptable => permission_flow_tests::always_approve_mode_keeps_questions_promptable);
delegate_test!(always_approve_mode_preserves_sensitive_permission_kinds => permission_flow_tests::always_approve_mode_preserves_sensitive_permission_kinds);
delegate_tokio_test!(static_deny_overrides_permission_grant => permission_flow_tests::static_deny_overrides_permission_grant);
delegate_tokio_test!(permission_grant_event_does_not_persist_raw_shell_command_secret => permission_flow_tests::permission_grant_event_does_not_persist_raw_shell_command_secret);
delegate_tokio_test!(perm_timeout_path_denies_deterministically => permission_flow_tests::perm_timeout_path_denies_deterministically);
delegate_tokio_test!(malformed_question_answer_does_not_resolve_permission => permission_flow_tests::malformed_question_answer_does_not_resolve_permission);

#[cfg(test)]
#[path = "tests/mcp_identity_tests.rs"]
mod mcp_identity_tests;

delegate_tokio_test!(mcp_effective_identity_persists_for_direct_and_wrapper_calls => mcp_identity_tests::mcp_effective_identity_persists_for_direct_and_wrapper_calls);
delegate_test!(mcp_effective_identity_uses_registered_first_class_ids_for_reserved_wrapper_names => mcp_identity_tests::mcp_effective_identity_uses_registered_first_class_ids_for_reserved_wrapper_names);

#[test]
fn stale_tool_task_late_result_preserves_owner_actor() {
    let temp_dir = tempfile::tempdir().unwrap_or_abort();
    let mut config = test_config(temp_dir.path());
    config.stale_timeout_ms = 20;
    let clock = Arc::new(FakeClock::new());
    let redactor = Arc::new(DefaultRedactor::default());
    let (_command_tx, command_rx) = mpsc::channel(1);
    let (job_tx, job_rx) = mpsc::channel(1);
    let mut coordinator = Coordinator::new(
        config,
        {
            let c: Arc<dyn crate::clock::Clock + Send + Sync> = Arc::<FakeClock>::clone(&clock);
            c
        },
        redactor,
        command_rx,
        job_tx,
        job_rx,
    );

    let run = coordinator
        .start_run_internal("stale_owner".to_string(), temp_dir.path().to_path_buf())
        .unwrap_or_abort();
    let task_id = "task_000001".to_string();
    let queue_key = ConcurrencyKey::Tool {
        tool_id: "shell.run".to_string(),
    };
    let owner_actor = EventActor::new(ActorKind::Worker, Some("agent_000001".to_string()));
    let request_correlation_id = Some("req_000001".to_string());

    {
        let run_state = coordinator.run_state.as_mut().unwrap_or_abort();
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
    coordinator.watchdog_tick_internal().unwrap_or_abort();
    coordinator
        .job_finished_internal(
            task_id.clone(),
            JobOutcome::Cancelled {
                reason: "job cancelled".to_string(),
            },
        )
        .unwrap_or_abort();

    let events = read_events(&run.events_path);
    assert!(events.iter().any(|event| {
        matches!(
            &event.payload,
            EventV1::StaleDetected(data)
                if data.task_id.as_str() == task_id
                    && event.actor == owner_actor
                    && event.correlation_id.as_deref() == request_correlation_id.as_deref()
        )
    }));
    assert!(events.iter().any(|event| {
        matches!(
            &event.payload,
            EventV1::TaskResultLate(data)
                if data.task_id.as_str() == task_id
                    && event.actor == owner_actor
                    && event.correlation_id.as_deref() == request_correlation_id.as_deref()
        )
    }));
}

#[tokio::test]
async fn background_foreground_child_tasks_releases_parent_task_and_keeps_child_running() {
    let temp_dir = tempfile::tempdir().unwrap_or_abort();
    let config = test_config(temp_dir.path());
    let clock = Arc::new(FakeClock::new());
    let redactor = Arc::new(DefaultRedactor::default());
    let (_command_tx, command_rx) = mpsc::channel(1);
    let (job_tx, job_rx) = mpsc::channel(1);
    let mut coordinator = Coordinator::new(config, clock, redactor, command_rx, job_tx, job_rx);

    let run = coordinator
        .start_run_internal_async(
            "foreground_detach".to_string(),
            temp_dir.path().to_path_buf(),
        )
        .await
        .unwrap_or_abort();
    let parent_task_id = "task_parent".to_string();
    let parent_tool_call_id = "toolcall_parent".to_string();
    let parent_request_id = "req_parent".to_string();
    let child_task_id = "task_child".to_string();
    let child_request_id = "req_child".to_string();
    let child_session_id = "agent_child".to_string();
    let queue_key = ConcurrencyKey::Tool {
        tool_id: "task".to_string(),
    };
    let (respond_to, response_rx) = oneshot::channel();

    {
        let run_state = coordinator.run_state.as_mut().unwrap_or_abort();
        assert!(matches!(
            run_state
                .scheduler
                .schedule(parent_task_id.clone(), queue_key.clone()),
            ScheduleDecision::Started(_)
        ));
        run_state.tasks.insert(
            parent_task_id.clone(),
            TaskState {
                tool_call_id: parent_tool_call_id.clone(),
                tool_metadata: None,
                owner_actor: EventActor::new(ActorKind::Worker, Some("agent_parent".to_string())),
                request_correlation_id: Some(parent_request_id.clone()),
                queue_key: queue_key.clone(),
                state: TaskExecutionState::Running,
                cancellation_token: CancellationToken::new(),
                started_mono_ms: 0,
                last_progress_mono_ms: 0,
                last_progress_kind: JobProgressKind::Heartbeat,
                hashline_edit: None,
                respond_to: Some(respond_to),
            },
        );
        run_state.running_agent_turns.insert(
            child_task_id.clone(),
            RunningAgentTurn {
                agent_id: child_session_id.clone(),
                request_id: child_request_id.clone(),
                request_prompt: "work in child".to_string(),
                attachments: Vec::new(),
                profile_name: "alpha".to_string(),
                model_ref: "mock:model-1".to_string(),
                model_settings: AgentModelSettings {
                    variant: None,
                    reasoning_effort: None,
                    text_verbosity: None,
                    reasoning_summary: None,
                    thinking: None,
                },
                profile: Some("alpha".to_string()),
                queue_key: ConcurrencyKey::ProviderModel {
                    provider_id: "mock".to_string(),
                    model_id: "model-1".to_string(),
                },
                cancellation_token: CancellationToken::new(),
                started_mono_ms: 0,
                hook_executions: Vec::new(),
                latest_provider_usage: None,
                latest_provider_request_id: None,
                latest_assistant_output: None,
                latest_provider_id: None,
                latest_model_id: None,
                child_task: Some(ChildTaskTurnState {
                    parent_tool_call_id: parent_tool_call_id.clone(),
                    parent_session_id: run.run_id.as_str().into(),
                    parent_agent_id: Some("agent_parent".to_string()),
                    child_session_id: child_session_id.into(),
                    child_request_id: child_request_id.clone(),
                    task_id: child_task_id.clone(),
                    description: "Long child work".to_string(),
                    run_in_background: false,
                }),
            },
        );
    }

    let count = coordinator
        .background_foreground_child_tasks_internal()
        .await
        .unwrap_or_abort();
    assert_eq!(count, 1);

    let response = response_rx.await.unwrap_or_abort().unwrap_or_abort();
    assert!(response
        .display_text
        .contains("Foreground subagent moved to background"));
    assert_eq!(
        response
            .structured_json
            .as_ref()
            .and_then(|json| json.get("background"))
            .and_then(serde_json::Value::as_bool),
        Some(true)
    );

    let run_state = coordinator.run_state.as_ref().unwrap_or_abort();
    assert!(!run_state.tasks.contains_key(&parent_task_id));
    let child = run_state
        .running_agent_turns
        .get(&child_task_id)
        .and_then(|running| running.child_task.as_ref())
        .unwrap_or_abort();
    assert!(child.run_in_background);
    assert_eq!(child.child_request_id, child_request_id);

    let events = read_events(&run.events_path);
    assert!(events.iter().any(|event| {
        matches!(
            &event.payload,
            EventV1::TaskCompleted(data)
                if data.task_id.as_str() == parent_task_id
                    && data.result_summary.contains("Foreground subagent moved to background")
        )
    }));
    assert!(events.iter().any(|event| {
        matches!(
            &event.payload,
            EventV1::ToolCallFinished(data)
                if data.tool_call_id.as_str() == parent_tool_call_id
                    && data.status == ToolCallStatus::Succeeded
        )
    }));
}

#[tokio::test]
async fn demote_foreground_child_task_releases_parent_for_single_handle() {
    // Given: one foreground-blocking child and a waiting parent task
    let temp_dir = tempfile::tempdir().unwrap_or_abort();
    let config = test_config(temp_dir.path());
    let clock = Arc::new(FakeClock::new());
    let redactor = Arc::new(DefaultRedactor::default());
    let (_command_tx, command_rx) = mpsc::channel(1);
    let (job_tx, job_rx) = mpsc::channel(1);
    let mut coordinator = Coordinator::new(config, clock, redactor, command_rx, job_tx, job_rx);

    let run = coordinator
        .start_run_internal_async(
            "foreground_demote_one".to_string(),
            temp_dir.path().to_path_buf(),
        )
        .await
        .unwrap_or_abort();
    let parent_task_id = "task_parent_demote".to_string();
    let parent_tool_call_id = "toolcall_parent_demote".to_string();
    let parent_request_id = "req_parent_demote".to_string();
    let child_task_id = "task_child_demote".to_string();
    let child_request_id = "req_child_demote".to_string();
    let child_session_id = "agent_child_demote".to_string();
    let queue_key = ConcurrencyKey::Tool {
        tool_id: "task".to_string(),
    };
    let (respond_to, response_rx) = oneshot::channel();

    {
        let run_state = coordinator.run_state.as_mut().unwrap_or_abort();
        assert!(matches!(
            run_state
                .scheduler
                .schedule(parent_task_id.clone(), queue_key.clone()),
            ScheduleDecision::Started(_)
        ));
        run_state.tasks.insert(
            parent_task_id.clone(),
            TaskState {
                tool_call_id: parent_tool_call_id.clone(),
                tool_metadata: None,
                owner_actor: EventActor::new(ActorKind::Worker, Some("agent_parent".to_string())),
                request_correlation_id: Some(parent_request_id.clone()),
                queue_key: queue_key.clone(),
                state: TaskExecutionState::Running,
                cancellation_token: CancellationToken::new(),
                started_mono_ms: 0,
                last_progress_mono_ms: 0,
                last_progress_kind: JobProgressKind::Heartbeat,
                hashline_edit: None,
                respond_to: Some(respond_to),
            },
        );
        run_state.running_agent_turns.insert(
            child_task_id.clone(),
            RunningAgentTurn {
                agent_id: child_session_id.clone(),
                request_id: child_request_id.clone(),
                request_prompt: "work in child".to_string(),
                attachments: Vec::new(),
                profile_name: "alpha".to_string(),
                model_ref: "mock:model-1".to_string(),
                model_settings: AgentModelSettings {
                    variant: None,
                    reasoning_effort: None,
                    text_verbosity: None,
                    reasoning_summary: None,
                    thinking: None,
                },
                profile: Some("alpha".to_string()),
                queue_key: ConcurrencyKey::ProviderModel {
                    provider_id: "mock".to_string(),
                    model_id: "model-1".to_string(),
                },
                cancellation_token: CancellationToken::new(),
                started_mono_ms: 0,
                hook_executions: Vec::new(),
                latest_provider_usage: None,
                latest_provider_request_id: None,
                latest_assistant_output: None,
                latest_provider_id: None,
                latest_model_id: None,
                child_task: Some(ChildTaskTurnState {
                    parent_tool_call_id: parent_tool_call_id.clone(),
                    parent_session_id: run.run_id.as_str().into(),
                    parent_agent_id: Some("agent_parent".to_string()),
                    child_session_id: child_session_id.into(),
                    child_request_id: child_request_id.clone(),
                    task_id: child_task_id.clone(),
                    description: "Long child work".to_string(),
                    run_in_background: false,
                }),
            },
        );
    }

    // When: demote the single child handle
    let result = coordinator
        .demote_foreground_child_task_internal(child_request_id.clone())
        .await
        .unwrap_or_abort();

    // Then: demoted under same request id; parent released; child stays running as background
    match result {
        crate::foreground_demote::DemoteToBackgroundResult::Demoted {
            handle_id,
            background_id,
            kind,
        } => {
            assert_eq!(handle_id, child_request_id);
            assert_eq!(background_id, child_request_id);
            assert_eq!(kind, crate::foreground_demote::ForegroundKind::Task);
        }
        other => panic!("expected Demoted, got {other:?}"),
    }

    let response = response_rx.await.unwrap_or_abort().unwrap_or_abort();
    assert!(response
        .display_text
        .contains("Foreground subagent moved to background"));
    assert_eq!(
        response
            .structured_json
            .as_ref()
            .and_then(|json| json.get("background"))
            .and_then(serde_json::Value::as_bool),
        Some(true)
    );

    let run_state = coordinator.run_state.as_ref().unwrap_or_abort();
    assert!(!run_state.tasks.contains_key(&parent_task_id));
    let child = run_state
        .running_agent_turns
        .get(&child_task_id)
        .and_then(|running| running.child_task.as_ref())
        .unwrap_or_abort();
    assert!(child.run_in_background);
    assert_eq!(child.child_request_id, child_request_id);

    // When: unknown handle is rejected without side effects
    let rejected = coordinator
        .demote_foreground_child_task_internal("missing-handle".to_string())
        .await
        .unwrap_or_abort();
    assert!(matches!(
        rejected,
        crate::foreground_demote::DemoteToBackgroundResult::Rejected { .. }
    ));
}

#[tokio::test]
async fn demote_all_foreground_child_tasks_releases_multiple_parents() {
    // Given: two foreground-blocking children with distinct parents
    let temp_dir = tempfile::tempdir().unwrap_or_abort();
    let config = test_config(temp_dir.path());
    let clock = Arc::new(FakeClock::new());
    let redactor = Arc::new(DefaultRedactor::default());
    let (_command_tx, command_rx) = mpsc::channel(1);
    let (job_tx, job_rx) = mpsc::channel(1);
    let mut coordinator = Coordinator::new(config, clock, redactor, command_rx, job_tx, job_rx);

    let run = coordinator
        .start_run_internal_async(
            "foreground_demote_all".to_string(),
            temp_dir.path().to_path_buf(),
        )
        .await
        .unwrap_or_abort();
    let (respond_a, response_a) = oneshot::channel();
    let (respond_b, response_b) = oneshot::channel();

    {
        let run_state = coordinator.run_state.as_mut().unwrap_or_abort();
        for (parent_task_id, parent_tool_call_id, parent_request_id, respond_to, tool_id) in [
            (
                "task_parent_a",
                "toolcall_parent_a",
                "req_parent_a",
                respond_a,
                "task-a",
            ),
            (
                "task_parent_b",
                "toolcall_parent_b",
                "req_parent_b",
                respond_b,
                "task-b",
            ),
        ] {
            let queue_key = ConcurrencyKey::Tool {
                tool_id: tool_id.to_string(),
            };
            assert!(matches!(
                run_state
                    .scheduler
                    .schedule(parent_task_id.to_string(), queue_key.clone()),
                ScheduleDecision::Started(_)
            ));
            run_state.tasks.insert(
                parent_task_id.to_string(),
                TaskState {
                    tool_call_id: parent_tool_call_id.to_string(),
                    tool_metadata: None,
                    owner_actor: EventActor::new(
                        ActorKind::Worker,
                        Some("agent_parent".to_string()),
                    ),
                    request_correlation_id: Some(parent_request_id.to_string()),
                    queue_key,
                    state: TaskExecutionState::Running,
                    cancellation_token: CancellationToken::new(),
                    started_mono_ms: 0,
                    last_progress_mono_ms: 0,
                    last_progress_kind: JobProgressKind::Heartbeat,
                    hashline_edit: None,
                    respond_to: Some(respond_to),
                },
            );
        }

        for (child_task_id, child_request_id, child_session_id, parent_tool_call_id) in [
            (
                "task_child_a",
                "req_child_a",
                "agent_child_a",
                "toolcall_parent_a",
            ),
            (
                "task_child_b",
                "req_child_b",
                "agent_child_b",
                "toolcall_parent_b",
            ),
        ] {
            run_state.running_agent_turns.insert(
                child_task_id.to_string(),
                RunningAgentTurn {
                    agent_id: child_session_id.to_string(),
                    request_id: child_request_id.to_string(),
                    request_prompt: "work in child".to_string(),
                    attachments: Vec::new(),
                    profile_name: "alpha".to_string(),
                    model_ref: "mock:model-1".to_string(),
                    model_settings: AgentModelSettings {
                        variant: None,
                        reasoning_effort: None,
                        text_verbosity: None,
                        reasoning_summary: None,
                        thinking: None,
                    },
                    profile: Some("alpha".to_string()),
                    queue_key: ConcurrencyKey::ProviderModel {
                        provider_id: "mock".to_string(),
                        model_id: "model-1".to_string(),
                    },
                    cancellation_token: CancellationToken::new(),
                    started_mono_ms: 0,
                    hook_executions: Vec::new(),
                    latest_provider_usage: None,
                    latest_provider_request_id: None,
                    latest_assistant_output: None,
                    latest_provider_id: None,
                    latest_model_id: None,
                    child_task: Some(ChildTaskTurnState {
                        parent_tool_call_id: parent_tool_call_id.to_string(),
                        parent_session_id: run.run_id.as_str().into(),
                        parent_agent_id: Some("agent_parent".to_string()),
                        child_session_id: child_session_id.into(),
                        child_request_id: child_request_id.to_string(),
                        task_id: child_task_id.to_string(),
                        description: "Long child work".to_string(),
                        run_in_background: false,
                    }),
                },
            );
        }
    }

    // When
    let results = coordinator
        .demote_all_foreground_child_tasks_internal()
        .await
        .unwrap_or_abort();
    let summary = crate::foreground_demote::summarize_demote_outcomes(&results);

    // Then: both demoted; parents released; children stay running as background
    assert_eq!(summary.demoted, 2);
    assert_eq!(summary.total, 2);
    assert!(results.iter().all(|r| r.is_demoted()));
    let _ = response_a.await.unwrap_or_abort().unwrap_or_abort();
    let _ = response_b.await.unwrap_or_abort().unwrap_or_abort();
    let run_state = coordinator.run_state.as_ref().unwrap_or_abort();
    assert!(!run_state.tasks.contains_key("task_parent_a"));
    assert!(!run_state.tasks.contains_key("task_parent_b"));
    for child_task_id in ["task_child_a", "task_child_b"] {
        let child = run_state
            .running_agent_turns
            .get(child_task_id)
            .and_then(|running| running.child_task.as_ref())
            .unwrap_or_abort();
        assert!(child.run_in_background);
    }

    // When: second bulk demote finds nothing demotable
    let empty = coordinator
        .demote_all_foreground_child_tasks_internal()
        .await
        .unwrap_or_abort();
    assert!(empty.is_empty());
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
    .unwrap_or_abort()
}

fn compaction_runtime_context() -> RecordedRuntimeContext {
    resolve_profile_model_metadata(&compaction_profile_config(), "alpha")
        .unwrap_or_abort()
        .into()
}

#[cfg(test)]
#[path = "tests/background_notification_tests.rs"]
mod background_notification_tests;

delegate_tokio_test!(background_task_completion_notifies_parent_once_and_queues_active_parent => background_notification_tests::delivery_background_task_completion_notifies_parent_once_and_queues_active_parent);
delegate_tokio_test!(background_task_completion_caps_and_redacts_description_and_summary => background_notification_tests::delivery_background_task_completion_caps_and_redacts_description_and_summary);
delegate_tokio_test!(background_task_completion_schedules_pending_wakeup_when_parent_finishes => background_notification_tests::background_task_completion_schedules_pending_wakeup_when_parent_finishes);
delegate_tokio_test!(background_task_completion_queues_parent_when_parent_is_idle => background_notification_tests::background_task_completion_queues_parent_when_parent_is_idle);
delegate_tokio_test!(background_task_completion_sync_spawn_does_not_notify => background_notification_tests::background_task_completion_sync_spawn_does_not_notify);
delegate_tokio_test!(background_task_completion_records_pending_notification_when_parent_cannot_wake => background_notification_tests::background_task_completion_records_pending_notification_when_parent_cannot_wake);
delegate_tokio_test!(background_task_completion_cancellation_and_late_terminal_do_not_duplicate => background_notification_tests::background_task_completion_cancellation_and_late_terminal_do_not_duplicate);
delegate_test!(background_task_completion_replay_projection_is_side_effect_free => background_notification_tests::background_task_completion_replay_projection_is_side_effect_free);

#[cfg(test)]
#[path = "tests/run_state_method_tests.rs"]
mod run_state_method_tests;

delegate_test!(run_state_turn_queue_methods_own_agent_turn_lifecycle_state => run_state_method_tests::run_state_turn_queue_methods_own_agent_turn_lifecycle_state);
delegate_test!(run_state_permission_methods_own_pending_and_grant_state => run_state_method_tests::run_state_permission_methods_own_pending_and_grant_state);
delegate_test!(run_state_compaction_methods_own_overflow_retry_attempt_state => run_state_method_tests::run_state_compaction_methods_own_overflow_retry_attempt_state);

#[cfg(test)]
#[path = "tests/canonical_provider_context_cache_tests.rs"]
mod canonical_provider_context_cache_tests;

delegate_test!(canonical_provider_context_cache_rejects_every_stale_identity_dimension => canonical_provider_context_cache_tests::canonical_provider_context_cache_rejects_every_stale_identity_dimension);

#[path = "tests/session_compaction_disabled_tests.rs"]
mod session_compaction_disabled_tests;
#[cfg(test)]
#[path = "tests/session_compaction_tests.rs"]
mod session_compaction_tests;

#[cfg(test)]
#[path = "tests/operational_memory_context_tests.rs"]
mod operational_memory_context_tests;

delegate_tokio_test!(operational_memory_records_read_and_modified_files_from_events => operational_memory_context_tests::operational_memory_records_read_and_modified_files_from_events);

mod workspace_snapshot_secret_tests;

#[cfg(test)]
#[path = "tests/workspace_snapshot_tests.rs"]
mod workspace_snapshot_tests;

delegate_tokio_test!(snapshot_captures_workspace_and_emits_event => workspace_snapshot_tests::snapshot_captures_workspace_and_emits_event);
delegate_tokio_test!(snapshot_omits_dotenv_files_from_artifacts => workspace_snapshot_secret_tests::snapshot_omits_dotenv_files_from_artifacts);
delegate_tokio_test!(revert_ignores_dotenv_files_missing_from_snapshot => workspace_snapshot_secret_tests::revert_ignores_dotenv_files_missing_from_snapshot);
delegate_tokio_test!(revert_ignores_dotenv_files_already_in_snapshot_artifact => workspace_snapshot_secret_tests::revert_ignores_dotenv_files_already_in_snapshot_artifact);
delegate_tokio_test!(revert_restores_workspace_from_snapshot => workspace_snapshot_tests::revert_restores_workspace_from_snapshot);
delegate_tokio_test!(replay_of_reverted_session_does_not_restore_files => workspace_snapshot_tests::replay_of_reverted_session_does_not_restore_files);
delegate_tokio_test!(formatter_runs_configured_command_on_edited_file => workspace_snapshot_tests::formatter_runs_configured_command_on_edited_file);
delegate_tokio_test!(formatter_disabled_skips_command => workspace_snapshot_tests::formatter_disabled_skips_command);
delegate_tokio_test!(formatter_missing_language_is_no_op => workspace_snapshot_tests::formatter_missing_language_is_no_op);
delegate_tokio_test!(formatter_failure_returns_warning_without_panic => workspace_snapshot_tests::formatter_failure_returns_warning_without_panic);

#[cfg(test)]
#[path = "tests/formatter_discovery_tests.rs"]
mod formatter_discovery_tests;

#[cfg(test)]
#[path = "tests/formatter_execution_command_tests.rs"]
mod formatter_execution_command_tests;
#[cfg(test)]
#[path = "tests/formatter_execution_tests.rs"]
mod formatter_execution_tests;

delegate_tokio_test!(built_in_discovery_includes_rustfmt_when_on_path => formatter_discovery_tests::built_in_discovery_includes_rustfmt_when_on_path);
delegate_tokio_test!(built_in_command_still_requires_discovery_when_only_extensions_overridden => formatter_discovery_tests::built_in_command_still_requires_discovery_when_only_extensions_overridden);
delegate_tokio_test!(multiple_matching_formatters_run_in_sorted_order => formatter_discovery_tests::multiple_matching_formatters_run_in_sorted_order);
delegate_tokio_test!(formatter_registry_order_is_declaration_order_not_alphabetical => formatter_discovery_tests::formatter_registry_order_is_declaration_order_not_alphabetical);
delegate_tokio_test!(ruff_uv_coupling_skips_both_when_one_disabled => formatter_discovery_tests::ruff_uv_coupling_skips_both_when_one_disabled);
delegate_tokio_test!(uv_disabled_skips_ruff_too => formatter_discovery_tests::uv_disabled_skips_ruff_too);
delegate_tokio_test!(formatter_status_reports_enabled_and_disabled_matches => formatter_discovery_tests::formatter_status_reports_enabled_and_disabled_matches);

delegate_tokio_test!(file_substitution_replaces_token_and_falls_back_to_append => formatter_execution_tests::file_substitution_replaces_token_and_falls_back_to_append);
delegate_tokio_test!(override_command_replaces_built_in_and_failure_is_non_fatal => formatter_execution_command_tests::override_command_replaces_built_in_and_failure_is_non_fatal);
delegate_tokio_test!(disabled_override_skips_formatter => formatter_execution_command_tests::disabled_override_skips_formatter);
delegate_tokio_test!(environment_variables_merge_with_override_winning => formatter_execution_tests::environment_variables_merge_with_override_winning);
delegate_tokio_test!(path_escape_returns_warning_and_does_not_touch_external_file => formatter_execution_tests::path_escape_returns_warning_and_does_not_touch_external_file);
delegate_tokio_test!(success_continues_after_one_formatter_fails => formatter_execution_command_tests::success_continues_after_one_formatter_fails);
delegate_tokio_test!(override_command_runs_even_when_builtin_not_on_path => formatter_execution_command_tests::override_command_runs_even_when_builtin_not_on_path);
delegate_tokio_test!(extension_override_replaces_builtin_extension_list => formatter_execution_tests::extension_override_replaces_builtin_extension_list);
delegate_tokio_test!(enabled_false_skips_all_formatters => formatter_execution_tests::enabled_false_skips_all_formatters);
delegate_tokio_test!(live_rustfmt_formats_and_diff_reflects_post_format_content => workspace_snapshot_tests::live_rustfmt_formats_and_diff_reflects_post_format_content);

fn test_run_state(session_dir: &Path, run_id: &str) -> RunState {
    let event_store =
        Arc::new(JsonlFileEventStore::open(session_dir, run_id, true).unwrap_or_abort());
    let run_dir = session_dir.join(run_id);
    let artifacts_dir = run_dir.join("artifacts");
    fs::create_dir_all(&artifacts_dir).unwrap_or_abort();
    RunState {
        info: RunInfo {
            run_id: run_id.to_string().into(),
            run_name: "interactive".into(),
            workspace_root: Path::new("/workspace/project").to_path_buf(),
            run_dir: run_dir.clone(),
            artifacts_dir,
            events_path: event_store.file_path().to_path_buf(),
        },
        event_store,
        canonical_event_history: Vec::new(),
        history_index_row: crate::session::history_index::SessionHistoryRowReducer::new(
            run_dir.clone(),
            run_id.to_string(),
            "interactive".to_string(),
            "/workspace/project".to_string(),
            Some(crate::proj::SessionModeSource::InteractiveMock),
        ),
        next_event_seq: 1,
        next_live_event_id: 1,
        next_agent_id: 1,
        next_tool_call_id: 1,
        next_task_id: 1,
        next_provider_request_id: 1,
        next_permission_id: 1,
        next_compaction_generation: 1,
        compaction_boundary_watermark: 0,
        agents: std::collections::BTreeMap::new(),
        provider_context_by_agent: std::collections::BTreeMap::new(),
        canonical_provider_view_by_agent: std::collections::BTreeMap::new(),
        provider_context_cache_key_by_agent: std::collections::BTreeMap::new(),
        live_incomplete_provider_turns_by_agent: std::collections::BTreeMap::new(),
        explicit_runtime_selection_request_ids: std::collections::BTreeSet::new(),
        tasks: std::collections::BTreeMap::new(),
        task_hook_state: std::collections::BTreeMap::new(),
        agent_hook_state: std::collections::BTreeMap::new(),
        subagent_parent_by_id: std::collections::BTreeMap::new(),
        child_session_mirrors: std::collections::BTreeMap::new(),
        child_request_session_by_id: std::collections::BTreeMap::new(),
        background_notification_child_requests: std::collections::BTreeSet::new(),
        pending_agent_wakeups: std::collections::BTreeMap::new(),
        pending_permissions: std::collections::BTreeMap::new(),
        always_approve_mode: false,
        tool_call_request_event_ids: std::collections::BTreeMap::new(),
        active_permission_grants: crate::perm::PermissionGrantSet::default(),
        cancelled_running_tasks: std::collections::BTreeSet::new(),
        queued_agent_turns: std::collections::BTreeMap::new(),
        running_agent_turns: std::collections::BTreeMap::new(),
        pending_compactions: std::collections::BTreeMap::new(),
        failed_terminal_compaction_attempts: std::collections::BTreeSet::new(),
        overflow_retry_compacted_context_by_attempt: std::collections::BTreeMap::new(),
        scheduler: Scheduler::new(SchedulerLimits {
            provider_model: 1,
            tool: 1,
        }),
        recorded_runtime_context: None,
        allow_initial_runtime_context_recording: false,
        shutdown_token: CancellationToken::new(),
        tool_state: ToolRunState::default(),
        last_identical_tool_key: None,
        identical_tool_call_streak: 0,
        doom_loop_always_granted: false,
        edit_attribution: crate::edit_attribution::EditAttributionJournal::empty(Path::new(
            "/workspace/project",
        )),
        team_registry: crate::team_registry::TeamRegistry::new(),
        cron_schedules: crate::cron_schedule::CronScheduleRegistry::new(),
        plugin_lifecycle: crate::integrations::PluginRuntimeContract::new(Path::new(
            "/workspace/project",
        )),
    }
}

fn long_turn(prompt: &str, fill: char) -> ProviderConversationTurn {
    ProviderConversationTurn {
        user_prompt: prompt.to_string(),
        assistant_response: fill.to_string().repeat(6_000),
        ..ProviderConversationTurn::default()
    }
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

fn write_restore_history_fixture(session_dir: &Path, run_id: &str, events: &[EventEnvelopeV1]) {
    let run_dir = session_dir.join(run_id);
    fs::create_dir_all(&run_dir).unwrap_or_abort();

    let mut body = String::new();
    for event in events {
        let line = serde_json::to_string(event).unwrap_or_abort();
        body.push_str(&line);
        body.push('\n');
    }

    fs::write(run_dir.join("events.jsonl"), body).unwrap_or_abort();
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
        run_id: run_id.to_string().into(),
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
    let text = fs::read_to_string(path).unwrap_or_abort();
    text.lines()
        .map(|line| serde_json::from_str::<EventEnvelopeV1>(line).unwrap_or_abort())
        .collect()
}

async fn wait_for_events(
    handle: &super::CoordinatorHandle,
    path: &Path,
    label: &str,
    matches: impl Fn(&EventEnvelopeV1) -> bool,
) -> Vec<EventEnvelopeV1> {
    let store = handle.event_store().await.unwrap_or_abort();
    let mut stream = store.subscribe(1).unwrap_or_abort();

    while let Some(next) = stream.next().await {
        let event = next.unwrap_or_abort();
        if matches(&event) {
            return read_events(path);
        }
    }

    let events = read_events(path);
    panic!("event stream ended waiting for {label}; events: {events:#?}");
}
