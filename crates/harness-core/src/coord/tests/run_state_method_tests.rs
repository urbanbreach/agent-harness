use super::*;
use crate::UnwrapOrAbort;

pub(super) fn run_state_turn_queue_methods_own_agent_turn_lifecycle_state() {
    // arrange
    let temp_dir = tempfile::tempdir().unwrap_or_abort();
    let clock = FakeClock::new();
    let mut run_state = test_run_state(temp_dir.path(), "run_state_turn_methods");

    run_state.queue_agent_turn(queued_agent_turn_fixture(
        "task_000002",
        "agent_000001",
        false,
    ));
    run_state.queue_agent_turn(queued_agent_turn_fixture(
        "task_000001",
        "agent_000001",
        false,
    ));
    run_state.queue_agent_turn(queued_agent_turn_fixture(
        "task_000003",
        "agent_000001",
        true,
    ));

    // act
    assert!(run_state.agent_has_active_or_queued_turn("agent_000001"));
    assert_eq!(
        run_state
            .next_agent_blocked_turn_id("agent_000001")
            .as_deref(),
        Some("task_000001")
    );

    run_state.mark_queued_agent_turn_scheduler_queued("task_000001");
    assert_eq!(
        run_state
            .next_agent_blocked_turn_id("agent_000001")
            .as_deref(),
        Some("task_000002")
    );

    let running = queued_agent_turn_fixture("task_000004", "agent_000001", false);
    let cancellation_token = CancellationToken::new();
    run_state.begin_running_agent_turn(&clock, &running, Vec::new(), cancellation_token.clone());

    // assert
    assert!(run_state.agent_has_running_turn("agent_000001"));
    assert!(run_state.agent_has_active_or_queued_turn("agent_000001"));
    assert_eq!(
        run_state.running_agent_turns["task_000004"].request_prompt,
        "prompt for agent_000001"
    );
    assert_eq!(
        run_state.running_agent_turns["task_000004"]
            .queue_key
            .queue_key(),
        "provider_model:mock:model-1"
    );
}

pub(super) fn run_state_permission_methods_own_pending_and_grant_state() {
    // arrange
    let temp_dir = tempfile::tempdir().unwrap_or_abort();
    let mut run_state = test_run_state(temp_dir.path(), "run_state_permission_methods");
    let (respond_to, _response_rx) = oneshot::channel();
    let request = permission_grant_request_fixture();
    let pending = PendingPermissionState {
        tool_call_id: "toolcall_000001".to_string(),
        request_correlation_id: Some("req_000001".to_string()),
        hook_executions: Vec::new(),
        grant_request: Some(request.clone()),
        resolution: PendingPermissionResolution::ToolCall {
            tool_id: "bash".to_string(),
            args_json: serde_json::json!({"cmd":"git status"}),
            actor: EventActor::new(ActorKind::Worker, Some("agent_000001".to_string())),
            category: Some("build".to_string()),
            respond_to: Some(respond_to),
        },
    };

    // act
    run_state.insert_pending_permission("perm_000001".to_string(), pending);

    // assert
    assert!(run_state.pending_permission("perm_000001").is_some());
    assert!(run_state.take_pending_permission("perm_000001").is_some());
    assert!(run_state.pending_permission("perm_000001").is_none());

    run_state.record_permission_grant(PermissionGrant {
        grant_id: "grant_000001".to_string(),
        permission_id: "perm_000001".to_string(),
        scope: PermissionGrantScope::Session,
        expires_at: None,
        kind: request.kind,
        tool: request.tool.clone(),
        matcher: request.matcher.clone(),
    });

    assert!(run_state.permission_grant_authorizes(&request));
}

pub(super) fn run_state_compaction_methods_own_overflow_retry_attempt_state() {
    // arrange
    let temp_dir = tempfile::tempdir().unwrap_or_abort();
    let mut run_state = test_run_state(temp_dir.path(), "run_state_compaction_methods");
    let request = FailedTerminalCompactionRequest::new(
        "task_000001",
        "agent_000001",
        "req_000001",
        "failed_response",
    );

    // act
    assert!(run_state.failed_terminal_compaction_attempt_should_run(&request));

    // assert
    assert!(!run_state.failed_terminal_compaction_attempt_should_run(&request));

    let mut second = test_run_state(temp_dir.path(), "run_state_compaction_overflow_skip");
    let compacted = ProviderContext::from_turns(vec![long_turn("same turn", 'A')]);
    second
        .provider_context_by_agent
        .insert("agent_000001".to_string(), compacted.clone());
    second.record_overflow_retry_compacted_context("task_000001", "req_000001", compacted);

    assert!(!second.failed_terminal_compaction_attempt_should_run(&request));
}

fn queued_agent_turn_fixture(
    task_id: &str,
    agent_id: &str,
    scheduler_queued: bool,
) -> QueuedAgentTurn {
    QueuedAgentTurn {
        task_id: task_id.to_string(),
        agent_id: agent_id.to_string(),
        session_id: "run_test".into(),
        request_id: format!("req_{task_id}"),
        profile: test_agent_profile(agent_id),
        request: crate::agent::AgentRequest {
            agent_id: agent_id.to_string(),
            prompt: format!("prompt for {agent_id}"),
            prompt_context: None,
            selected_file_tags: Vec::new(),
            selected_agent_tags: Vec::new(),
            selected_resource_tags: Vec::new(),
            model_ref: "mock:model-1".to_string(),
            model_settings: Default::default(),
        },
        queue_key: ConcurrencyKey::ProviderModel {
            provider_id: "mock".to_string(),
            model_id: "model-1".to_string(),
        },
        scheduler_queued,
        child_task: None,
        model_fallback_chain: Vec::new(),
    }
}

fn permission_grant_request_fixture() -> crate::perm::PermissionGrantRequest {
    crate::perm::PermissionGrantRequest {
        kind: PermissionKind::Shell,
        tool: PermissionToolSelector {
            effective_tool_id: "bash".to_string(),
            canonical_tool_id: Some("bash".to_string()),
        },
        matcher: PermissionGrantMatcher::RequestDigest {
            request_digest: "digest-1".to_string(),
        },
    }
}
