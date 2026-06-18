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

use crate::agent::{
    AgentProfile, ProviderCompactionFacts, ProviderCompactionSummarySource,
    ProviderCompactionTailBoundary, ProviderContext, ProviderContextCheckpoint,
    ProviderContextCheckpointMetadata, ProviderConversationTurn, ProviderConversationTurnStatus,
    ProviderFileOperationFact,
};
use crate::clock::{FakeClock, RealClock};
use crate::config::{
    load_config_from_str, resolve_profile_model_metadata, CategoryPermissions,
    CompactionRuntimeConfig, HookLifecycleEvent, HookRuntimeConfig, HooksConfig,
    LifecycleHookConfig, PermissionMode, ShellAllowlist,
};
use crate::conversation::{
    ConversationAssistantMessage, ConversationMessage, ConversationToolCall,
    ConversationToolResultMessage, ConversationUserMessage,
};
use crate::event::{
    ActorKind, AgentSpawnedEvent, ArtifactWrittenEvent, BackgroundTaskNotificationStatus,
    CompactionAppliedEvent, CompactionWrittenEvent, EditAppliedEvent, EventActor, EventArtifactRef,
    EventEnvelopeV1, EventV1, HookExecutionMetadata, HookExecutionStatus,
    ProviderRequestFinishedEvent, ProviderRequestStartedEvent, ProviderStreamDeltaEvent,
    RunFinishedEvent, RunStartedEvent, TaskCancelledEvent, TaskCompletedEvent, TaskScheduleState,
    TaskScheduledEvent, ToolCallFinishedEvent, ToolCallMetadata, ToolCallRequestedEvent,
    ToolCallStatus, ToolIdentityMetadata, UserMessageSubmittedEvent, SCHEMA_VERSION,
};
use crate::perm::{
    PermissionDecision, PermissionGrant, PermissionGrantMatcher, PermissionGrantScope,
    PermissionKind, PermissionPolicy, PermissionRuleRequest, PermissionToolSelector,
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
    append_payload_event_with_correlation, append_permission_grant_recorded_event,
    append_permission_requested_event, append_tool_call_finished_event,
    append_tool_call_requested_event, append_tool_call_started_event,
    build_model_compaction_prompt, build_provider_context_summary, compact_provider_context,
    completion_messages_to_conversation_messages, permission_rule_request_selectors,
    plan_mode_shell_boundary_denial, provider_context_summary_required_headings,
    provider_tool_message_status, restore_provider_context_from_history,
    schedule_pending_agent_wakeups_for_idle_agent, spawn_coordinator, summarize_hook_output,
    validate_model_compaction_summary, ChildTaskTurnState, Coordinator, CoordinatorConfig,
    CoordinatorError, EditAppliedEventArgs, FailedTerminalCompactionRequest, HashlineEditMetadata,
    HookExecutionBatch, HookInvocationContext, JobOutcome, JobProgressKind,
    PendingPermissionResolution, PendingPermissionState, PermissionRequestedEventArgs,
    ProviderCompactionTrigger, ProviderContextCompactionPlan, QueuedAgentTurn, RunInfo, RunState,
    RunningAgentTurn, TaskExecutionState, TaskState, TokioLifecycleHookCommandExecutor,
    ToolCallFinishedEventArgs, ToolCallRequestedEventArgs,
};
use harness_providers::{CompletionMessage, MessageRole};

use super::hooks::{
    LifecycleHookCommandExecutor, LifecycleHookCommandInvocation, LifecycleHookCommandOutput,
};
use super::provider_context::{
    compaction_summary_override_from_hooks, ProviderContextCompactionRequest,
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
        self.invocations
            .lock()
            .expect("fake hook executor lock")
            .push(invocation);
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
    let temp_dir = tempfile::tempdir().expect("tempdir");
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
            run_id: "run_fake_hook".to_string(),
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
            category: None,
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
    let invocations = executor
        .invocations
        .lock()
        .expect("fake hook executor lock");
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
fn plan_mode_shell_boundary_allows_only_read_only_inspection_commands() {
    assert_eq!(
        plan_mode_shell_boundary_denial(
            Some(crate::plan::PLAN_AGENT_NAME),
            Some(PermissionKind::Shell),
            &json!({ "command": "git status --short" }),
        ),
        None
    );
    assert_eq!(
        plan_mode_shell_boundary_denial(
            Some(crate::plan::PLAN_AGENT_NAME),
            Some(PermissionKind::Shell),
            &json!({ "command": "git branch --show-current" }),
        ),
        None
    );
    assert_eq!(
        plan_mode_shell_boundary_denial(
            Some(crate::plan::PLAN_AGENT_NAME),
            Some(PermissionKind::Shell),
            &json!({ "command": "ls crates" }),
        ),
        None
    );

    let denied = plan_mode_shell_boundary_denial(
        Some(crate::plan::PLAN_AGENT_NAME),
        Some(PermissionKind::Shell),
        &json!({ "command": "touch src/lib.rs" }),
    )
    .expect("mutating command denied");
    assert!(denied.contains("read-only inspection commands"));

    let redirected = plan_mode_shell_boundary_denial(
        Some(crate::plan::PLAN_AGENT_NAME),
        Some(PermissionKind::Shell),
        &json!({ "command": "git status > status.txt" }),
    )
    .expect("redirection denied");
    assert!(redirected.contains("read-only inspection commands"));

    for command in [
        "git branch new-plan-branch",
        "git branch -D old-plan-branch",
        "git diff --output=plan-leak.txt",
        "git show --output plan-leak.txt HEAD",
        "git diff --ext-diff",
        "git show --textconv HEAD:README.md",
        "git log --ext-diff -p",
        "git diff '--ext-diff'",
        "git show \"--textconv\" HEAD:README.md",
        "git diff --ext\\-diff",
    ] {
        let denied = plan_mode_shell_boundary_denial(
            Some(crate::plan::PLAN_AGENT_NAME),
            Some(PermissionKind::Shell),
            &json!({ "command": command }),
        )
        .unwrap_or_else(|| panic!("command `{command}` should be denied"));
        assert!(denied.contains("read-only inspection commands"));
    }
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
        category: name.to_string(),
        model_ref: "mock:model-1".to_string(),
        model_ref_explicit: true,
        system_prompt: "sys".to_string(),
        cache_retention: Default::default(),
        max_iters: Some(1),
        temperature: None,
        tool_failure_mode: crate::config::ToolFailureMode::FailTurn,
        toolset: Vec::new(),
    }
}

#[tokio::test]
async fn fresh_run_agent_ids_skip_existing_child_session_directories() {
    let temp_dir = tempfile::tempdir().expect("tempdir");
    fs::create_dir_all(temp_dir.path().join("agent_000001")).expect("create first old child dir");
    let stale_child_dir = temp_dir.path().join("agent_000002");
    fs::create_dir_all(&stale_child_dir).expect("create stale child dir");
    fs::write(stale_child_dir.join(".writer.lock"), "").expect("write stale legacy lock");
    fs::write(stale_child_dir.join("events.jsonl"), "").expect("write stale event log");

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
        .expect("start run");
    let supervisor = EventActor::new(ActorKind::Supervisor, None);
    let parent_agent_id = handle
        .spawn_agent(supervisor.clone(), "alpha", None)
        .await
        .expect("spawn parent agent");
    let child_agent_id = handle
        .spawn_agent_idle(supervisor, "alpha", Some(parent_agent_id.clone()))
        .await
        .expect("spawn child agent without colliding with stale lock");

    assert_eq!(parent_agent_id, "agent_000003");
    assert_eq!(child_agent_id, "agent_000004");
    assert!(temp_dir.path().join("agent_000004/events.jsonl").exists());
    assert_eq!(
        fs::read_to_string(stale_child_dir.join(".writer.lock")).expect("stale lock remains"),
        ""
    );
    assert!(run.run_dir.ends_with("run_000001"));
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
delegate_test!(task_permission_rule_selector_uses_subagent_type_before_aliases => permission_flow_tests::rule_task_permission_rule_selector_uses_subagent_type_before_aliases);
delegate_tokio_test!(permission_rule_task_selector_is_enforced_at_tool_call_site => permission_flow_tests::rule_permission_rule_task_selector_is_enforced_at_tool_call_site);
delegate_tokio_test!(perm_ask_path_blocks_until_resolved => permission_flow_tests::rule_perm_ask_path_blocks_until_resolved);
delegate_tokio_test!(allow_always_records_grant_and_authorizes_matching_future_shell_call => permission_flow_tests::allow_always_records_grant_and_authorizes_matching_future_shell_call);
delegate_tokio_test!(allow_always_shell_run_grant_does_not_authorize_changed_args => permission_flow_tests::allow_always_shell_run_grant_does_not_authorize_changed_args);
delegate_tokio_test!(static_deny_overrides_permission_grant => permission_flow_tests::static_deny_overrides_permission_grant);
delegate_tokio_test!(permission_grant_event_does_not_persist_raw_shell_command_secret => permission_flow_tests::permission_grant_event_does_not_persist_raw_shell_command_secret);
delegate_tokio_test!(perm_timeout_path_denies_deterministically => permission_flow_tests::perm_timeout_path_denies_deterministically);
delegate_tokio_test!(malformed_question_answer_does_not_resolve_permission => permission_flow_tests::malformed_question_answer_does_not_resolve_permission);

#[cfg(test)]
#[path = "tests/operational_memory_tests.rs"]
mod operational_memory_tests;

delegate_test!(operational_memory_records_read_and_modified_files_from_events => operational_memory_tests::context_operational_memory_records_read_and_modified_files_from_events);
delegate_test!(compaction_preserves_file_tool_skill_todo_and_plan_context => operational_memory_tests::context_compaction_preserves_file_tool_skill_todo_and_plan_context);
delegate_test!(operational_memory_redacts_secret_shaped_facts => operational_memory_tests::operational_memory_redacts_secret_shaped_facts);
delegate_test!(operational_memory_dedupes_sorts_and_caps_paths => operational_memory_tests::operational_memory_dedupes_sorts_and_caps_paths);
delegate_test!(operational_memory_ignores_freeform_path_like_output => operational_memory_tests::operational_memory_ignores_freeform_path_like_output);
delegate_test!(operational_memory_preserves_touched_files_legacy_union => operational_memory_tests::operational_memory_preserves_touched_files_legacy_union);
delegate_test!(operational_memory_resume_loads_checkpoint_facts_without_filesystem_scan => operational_memory_tests::operational_memory_resume_loads_checkpoint_facts_without_filesystem_scan);

#[cfg(test)]
#[path = "tests/mcp_identity_tests.rs"]
mod mcp_identity_tests;

delegate_tokio_test!(mcp_effective_identity_persists_for_direct_and_wrapper_calls => mcp_identity_tests::mcp_effective_identity_persists_for_direct_and_wrapper_calls);
delegate_test!(mcp_effective_identity_uses_registered_first_class_ids_for_reserved_wrapper_names => mcp_identity_tests::mcp_effective_identity_uses_registered_first_class_ids_for_reserved_wrapper_names);

#[cfg(test)]
#[path = "tests/oversized_turn_tests.rs"]
mod oversized_turn_tests;

delegate_test!(split_oversized_turn_pre_prompt_preserves_suffix_and_prefix_summary => oversized_turn_tests::split_oversized_turn_pre_prompt_preserves_suffix_and_prefix_summary);
delegate_test!(split_oversized_failed_provider_error_preserves_incomplete_suffix => oversized_turn_tests::split_oversized_failed_provider_error_preserves_incomplete_suffix);
delegate_test!(split_oversized_turn_refuses_tool_failure_to_avoid_orphan_tools => oversized_turn_tests::split_oversized_turn_refuses_tool_failure_to_avoid_orphan_tools);
delegate_test!(split_oversized_turn_refuses_artifact_backed_turn => oversized_turn_tests::split_oversized_turn_refuses_artifact_backed_turn);
delegate_test!(split_oversized_turn_refuses_provider_neutral_tool_messages => oversized_turn_tests::split_oversized_turn_refuses_provider_neutral_tool_messages);
delegate_test!(split_oversized_turn_prefix_summary_in_checkpoint_facts => oversized_turn_tests::split_oversized_turn_prefix_summary_in_checkpoint_facts);

#[cfg(test)]
#[path = "tests/compaction_planning_tests.rs"]
mod compaction_planning_tests;

delegate_test!(proactive_compaction_writes_checkpoint_artifact_and_updates_provider_context => compaction_planning_tests::checkpoint_proactive_compaction_writes_checkpoint_artifact_and_updates_provider_context);
delegate_test!(provider_context_compaction_request_returns_none_for_single_turn_manual_context => compaction_planning_tests::provider_context_compaction_request_returns_none_for_single_turn_manual_context);
delegate_test!(provider_context_compaction_request_builds_checkpoint_decision_without_appending_events => compaction_planning_tests::provider_context_compaction_request_builds_checkpoint_decision_without_appending_events);
delegate_test!(compaction_trigger_pre_prompt_uses_estimate_without_provider_usage => compaction_planning_tests::compaction_trigger_pre_prompt_uses_estimate_without_provider_usage);
delegate_test!(compaction_trigger_uses_fallback_budget_without_model_metadata => compaction_planning_tests::compaction_trigger_uses_fallback_budget_without_model_metadata);
delegate_test!(compaction_trigger_noops_below_estimated_threshold => compaction_planning_tests::compaction_trigger_noops_below_estimated_threshold);
delegate_test!(structured_summary_contract_can_be_disabled_for_legacy_headings => compaction_planning_tests::structured_summary_contract_can_be_disabled_for_legacy_headings);
delegate_test!(deterministic_summary_uses_required_harness_sections => compaction_planning_tests::deterministic_summary_uses_required_harness_sections);
delegate_test!(model_summary_validation_rejects_missing_required_harness_section => compaction_planning_tests::model_summary_validation_rejects_missing_required_harness_section);
delegate_test!(proactive_compaction_records_pruned_tool_artifacts_for_compacted_turns => compaction_planning_tests::checkpoint_proactive_compaction_records_pruned_tool_artifacts_for_compacted_turns);
delegate_test!(repeated_compaction_updates_existing_summary_without_legacy_append_format => compaction_planning_tests::repeated_compaction_updates_existing_summary_without_legacy_append_format);
delegate_test!(compaction_summary_override_uses_explicit_hook_prefix_only => compaction_planning_tests::compaction_summary_override_uses_explicit_hook_prefix_only);

#[cfg(test)]
#[path = "tests/provider_context_checkpoint_tests.rs"]
mod provider_context_checkpoint_tests;

delegate_test!(replay_equivalence_after_failed_turn_pre_prompt_compaction_resume => provider_context_checkpoint_tests::replay_replay_equivalence_after_failed_turn_pre_prompt_compaction_resume);
delegate_test!(legacy_provider_context_checkpoint_deserializes => provider_context_checkpoint_tests::legacy_provider_context_checkpoint_deserializes);
delegate_test!(provider_neutral_reconstruction_marks_continue_as_tool_message_failures => provider_context_checkpoint_tests::provider_neutral_reconstruction_marks_continue_as_tool_message_failures);
delegate_test!(provider_context_checkpoint_legacy_round_trips_with_new_defaults => provider_context_checkpoint_tests::provider_context_checkpoint_legacy_round_trips_with_new_defaults);
delegate_test!(failed_turn_status_defaults_to_completed_for_legacy_checkpoint => provider_context_checkpoint_tests::failed_turn_status_defaults_to_completed_for_legacy_checkpoint);
delegate_test!(compaction_turn_facts_include_failed_turn_status => provider_context_checkpoint_tests::compaction_turn_facts_include_failed_turn_status);

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

#[cfg(test)]
#[path = "tests/provider_context_restore_tests.rs"]
mod provider_context_restore_tests;

delegate_test!(restore_provider_context_uses_task_completed_summary_for_iterative_history => provider_context_restore_tests::restore_provider_context_uses_task_completed_summary_for_iterative_history);
delegate_test!(failed_response_compaction_does_not_double_compact_same_request => provider_context_restore_tests::failed_response_compaction_does_not_double_compact_same_request);
delegate_test!(restore_provider_context_from_history_uses_checkpoint_then_replays_post_checkpoint_turns => provider_context_restore_tests::checkpoint_restore_provider_context_from_history_uses_checkpoint_then_replays_post_checkpoint_turns);
delegate_test!(failed_turn_context_resume_reconstructs_failed_turn_after_checkpoint => provider_context_restore_tests::checkpoint_failed_turn_context_resume_reconstructs_failed_turn_after_checkpoint);
delegate_test!(failed_turn_context_does_not_duplicate_completed_turns => provider_context_restore_tests::failed_turn_context_does_not_duplicate_completed_turns);
delegate_test!(restore_provider_context_from_history_rejects_checkpoint_metadata_mismatch => provider_context_restore_tests::restore_provider_context_from_history_rejects_checkpoint_metadata_mismatch);

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
#[path = "tests/workspace_snapshot_tests.rs"]
mod workspace_snapshot_tests;

delegate_tokio_test!(snapshot_captures_workspace_and_emits_event => workspace_snapshot_tests::snapshot_captures_workspace_and_emits_event);
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
        child_session_mirrors: std::collections::BTreeMap::new(),
        child_request_session_by_id: std::collections::BTreeMap::new(),
        background_notification_child_requests: std::collections::BTreeSet::new(),
        pending_agent_wakeups: std::collections::BTreeMap::new(),
        pending_permissions: std::collections::BTreeMap::new(),
        active_permission_grants: crate::perm::PermissionGrantSet::default(),
        cancelled_running_tasks: std::collections::BTreeSet::new(),
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
        tool_state: ToolRunState::default(),
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

async fn wait_for_events(
    handle: &super::CoordinatorHandle,
    path: &Path,
    label: &str,
    matches: impl Fn(&EventEnvelopeV1) -> bool,
) -> Vec<EventEnvelopeV1> {
    let store = handle.event_store().await.expect("get event store");
    let mut stream = store.subscribe(1).expect("subscribe to event store");

    while let Some(next) = stream.next().await {
        let event = next.expect("event stream item");
        if matches(&event) {
            return read_events(path);
        }
    }

    let events = read_events(path);
    panic!("event stream ended waiting for {label}; events: {events:#?}");
}
