use super::*;
use crate::UnwrapOrAbort;

#[path = "permission_flow_rule_tests.rs"]
mod permission_flow_rule_tests;
pub(super) use permission_flow_rule_tests::{
    perm_ask_path_blocks_until_resolved as rule_perm_ask_path_blocks_until_resolved,
    permission_rule_bash_selector_is_enforced_at_tool_call_site as rule_permission_rule_bash_selector_is_enforced_at_tool_call_site,
    permission_rule_task_selector_is_enforced_at_tool_call_site as rule_permission_rule_task_selector_is_enforced_at_tool_call_site,
    task_permission_rule_selector_uses_only_subagent_type as rule_task_permission_rule_selector_uses_only_subagent_type,
};

pub(super) async fn allow_always_records_grant_and_authorizes_matching_future_shell_call() {
    let temp_dir = tempfile::tempdir().unwrap_or_abort();
    let mut config = test_config(temp_dir.path());
    config.permission_policy = ask_shell_permission_policy(1_000);

    let handle = spawn_coordinator(
        config,
        Arc::new(FakeClock::new()),
        Arc::new(DefaultRedactor::default()),
    );

    let run = handle
        .start_run("perm_allow_always", temp_dir.path())
        .await
        .unwrap_or_abort();

    let actor = EventActor::new(ActorKind::Supervisor, Some("agent-supervisor".to_string()));
    let first_tool_call_id = handle
        .request_tool_call(
            actor.clone(),
            Some("deep".to_string()),
            "shell.run",
            json!({"cmd": "echo durable"}),
        )
        .await
        .unwrap_or_abort();

    let before_resolve = wait_for_events(
        &handle,
        &run.events_path,
        "first durable-grant permission request",
        |event| {
            matches!(
                &event.payload,
                EventV1::PermissionRequested(data)
                    if data.tool_call_id.as_ref().map(|id| id.as_str()) == Some(first_tool_call_id.as_str())
            )
        },
    )
    .await;
    let permission_id = before_resolve
        .iter()
        .find_map(|event| match &event.payload {
            EventV1::PermissionRequested(data)
                if data.tool_call_id.as_ref().map(|id| id.as_str())
                    == Some(first_tool_call_id.as_str()) =>
            {
                Some(data.permission_id.clone())
            }
            _ => None,
        })
        .unwrap_or_abort();

    handle
        .resolve_permission_with_grant_scope(
            permission_id,
            PermissionDecision::Allow,
            None,
            Some(PermissionGrantScope::Run),
        )
        .await
        .unwrap_or_abort();
    wait_for_events(
        &handle,
        &run.events_path,
        "durable permission grant",
        |event| matches!(event.payload, EventV1::PermissionGrantRecorded(_)),
    )
    .await;

    let second_tool_call_id = handle
        .request_tool_call(
            actor,
            Some("deep".to_string()),
            "shell.run",
            json!({"cmd": "echo durable", "note": "different digest"}),
        )
        .await
        .unwrap_or_abort();

    wait_for_events(
        &handle,
        &run.events_path,
        "second durable-grant tool call to start",
        |event| {
            matches!(
                &event.payload,
                EventV1::ToolCallStarted(data) if data.tool_call_id.as_str() == second_tool_call_id
            )
        },
    )
    .await;
    handle.stop_run().await.unwrap_or_abort();

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
            EventV1::ToolCallStarted(data) if data.tool_call_id.as_str() == first_tool_call_id
        )
    }));
    assert!(events.iter().any(|event| {
        matches!(
            &event.payload,
            EventV1::ToolCallStarted(data) if data.tool_call_id.as_str() == second_tool_call_id
        )
    }));
}

pub(super) async fn allow_always_shell_run_grant_does_not_authorize_changed_args() {
    let temp_dir = tempfile::tempdir().unwrap_or_abort();
    let mut config = test_config(temp_dir.path());
    config.permission_policy = ask_shell_permission_policy(1_000);

    let handle = spawn_coordinator(
        config,
        Arc::new(FakeClock::new()),
        Arc::new(DefaultRedactor::default()),
    );

    let run = handle
        .start_run("perm_allow_always_args", temp_dir.path())
        .await
        .unwrap_or_abort();

    let actor = EventActor::new(ActorKind::Supervisor, Some("agent-supervisor".to_string()));
    let first_tool_call_id = handle
        .request_tool_call(
            actor.clone(),
            Some("deep".to_string()),
            "shell.run",
            json!({"cmd": "bash", "args": ["-lc", "echo durable"]}),
        )
        .await
        .unwrap_or_abort();

    let permission_id = wait_for_events(
        &handle,
        &run.events_path,
        "first shell args permission request",
        |event| {
            matches!(
                &event.payload,
                EventV1::PermissionRequested(data)
                    if data.tool_call_id.as_ref().map(|id| id.as_str()) == Some(first_tool_call_id.as_str())
            )
        },
    )
    .await
    .iter()
    .find_map(|event| match &event.payload {
        EventV1::PermissionRequested(data)
            if data.tool_call_id.as_ref().map(|id| id.as_str()) == Some(first_tool_call_id.as_str()) =>
        {
            Some(data.permission_id.clone())
        }
        _ => None,
    })
    .unwrap_or_abort();

    handle
        .resolve_permission_with_grant_scope(
            permission_id,
            PermissionDecision::Allow,
            None,
            Some(PermissionGrantScope::Run),
        )
        .await
        .unwrap_or_abort();
    wait_for_events(
        &handle,
        &run.events_path,
        "shell args durable permission grant",
        |event| matches!(event.payload, EventV1::PermissionGrantRecorded(_)),
    )
    .await;

    let second_tool_call_id = handle
        .request_tool_call(
            actor,
            Some("deep".to_string()),
            "shell.run",
            json!({"cmd": "bash", "args": ["-lc", "echo changed"]}),
        )
        .await
        .unwrap_or_abort();

    wait_for_events(
        &handle,
        &run.events_path,
        "changed args permission request",
        |event| {
            matches!(
                &event.payload,
                EventV1::PermissionRequested(data)
                    if data.tool_call_id.as_ref().map(|id| id.as_str()) == Some(second_tool_call_id.as_str())
            )
        },
    )
    .await;
    handle.stop_run().await.unwrap_or_abort();

    let events = read_events(&run.events_path);
    let requested_count = events
        .iter()
        .filter(|event| matches!(event.payload, EventV1::PermissionRequested(_)))
        .count();
    assert_eq!(requested_count, 2, "changed args should ask again");
    assert!(!events.iter().any(|event| {
        matches!(
            &event.payload,
            EventV1::ToolCallStarted(data) if data.tool_call_id.as_str() == second_tool_call_id
        )
    }));
}

pub(super) async fn static_deny_overrides_permission_grant() {
    let temp_dir = tempfile::tempdir().unwrap_or_abort();
    let mut config = test_config(temp_dir.path());
    config.permission_policy = ask_shell_permission_policy(1_000).with_profile_override(
        "locked",
        ProfilePermissions {
            shell: Some(PermissionMode::Deny),
            ..ProfilePermissions::default()
        },
    );

    let handle = spawn_coordinator(
        config,
        Arc::new(FakeClock::new()),
        Arc::new(DefaultRedactor::default()),
    );
    let run = handle
        .start_run("static_deny", temp_dir.path())
        .await
        .unwrap_or_abort();

    let actor = EventActor::new(ActorKind::Supervisor, Some("agent-supervisor".to_string()));
    let granted_tool_call_id = handle
        .request_tool_call(
            actor.clone(),
            Some("deep".to_string()),
            "shell.run",
            json!({"cmd": "echo durable"}),
        )
        .await
        .unwrap_or_abort();
    let permission_id = wait_for_events(
        &handle,
        &run.events_path,
        "static-deny grantable permission request",
        |event| {
            matches!(
                &event.payload,
                EventV1::PermissionRequested(data)
                    if data.tool_call_id.as_ref().map(|id| id.as_str()) == Some(granted_tool_call_id.as_str())
            )
        },
    )
    .await
    .iter()
    .find_map(|event| match &event.payload {
        EventV1::PermissionRequested(data)
            if data.tool_call_id.as_ref().map(|id| id.as_str()) == Some(granted_tool_call_id.as_str()) =>
        {
            Some(data.permission_id.clone())
        }
        _ => None,
    })
    .unwrap_or_abort();
    handle
        .resolve_permission_with_grant_scope(
            permission_id,
            PermissionDecision::Allow,
            None,
            Some(PermissionGrantScope::Run),
        )
        .await
        .unwrap_or_abort();
    wait_for_events(
        &handle,
        &run.events_path,
        "static-deny durable permission grant",
        |event| matches!(event.payload, EventV1::PermissionGrantRecorded(_)),
    )
    .await;

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

    handle.stop_run().await.unwrap_or_abort();
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
            EventV1::ToolCallStarted(data) if data.tool_call_id.as_str() == "toolcall_000002"
        )
    });
    assert!(!denied_tool_started);
}

pub(super) async fn permission_grant_event_does_not_persist_raw_shell_command_secret() {
    let temp_dir = tempfile::tempdir().unwrap_or_abort();
    let mut config = test_config(temp_dir.path());
    config.permission_policy = ask_shell_permission_policy(1_000);

    let handle = spawn_coordinator(
        config,
        Arc::new(FakeClock::new()),
        Arc::new(DefaultRedactor::default()),
    );
    let run = handle
        .start_run("perm_grant_redaction", temp_dir.path())
        .await
        .unwrap_or_abort();

    let tool_call_id = handle
        .request_tool_call(
            EventActor::new(ActorKind::Supervisor, Some("agent-supervisor".to_string())),
            Some("deep".to_string()),
            "shell.run",
            json!({"cmd": "curl -H 'Authorization: Bearer secret.value' https://example.invalid"}),
        )
        .await
        .unwrap_or_abort();
    let permission_id = wait_for_events(
        &handle,
        &run.events_path,
        "redaction permission request",
        |event| {
            matches!(
                &event.payload,
                EventV1::PermissionRequested(data)
                    if data.tool_call_id.as_ref().map(|id| id.as_str()) == Some(tool_call_id.as_str())
            )
        },
    )
    .await
    .iter()
    .find_map(|event| match &event.payload {
        EventV1::PermissionRequested(data)
            if data.tool_call_id.as_ref().map(|id| id.as_str()) == Some(tool_call_id.as_str()) =>
        {
            Some(data.permission_id.clone())
        }
        _ => None,
    })
    .unwrap_or_abort();

    handle
        .resolve_permission_with_grant_scope(
            permission_id,
            PermissionDecision::Allow,
            None,
            Some(PermissionGrantScope::Run),
        )
        .await
        .unwrap_or_abort();
    wait_for_events(
        &handle,
        &run.events_path,
        "redacted permission grant",
        |event| matches!(event.payload, EventV1::PermissionGrantRecorded(_)),
    )
    .await;
    handle.stop_run().await.unwrap_or_abort();

    let events_body = fs::read_to_string(&run.events_path).unwrap_or_abort();
    let grant_line = events_body
        .lines()
        .find(|line| line.contains("permission_grant_recorded"))
        .unwrap_or_abort();
    assert!(!grant_line.contains("secret.value"));
    assert!(!grant_line.contains("Authorization"));
    assert!(!grant_line.contains("Bearer"));
}

pub(super) async fn perm_timeout_path_denies_deterministically() {
    let temp_dir = tempfile::tempdir().unwrap_or_abort();
    let mut config = test_config(temp_dir.path());
    config.permission_policy = ask_shell_permission_policy(25);

    let handle = spawn_coordinator(
        config,
        Arc::new(FakeClock::new()),
        Arc::new(DefaultRedactor::default()),
    );

    let run = handle
        .start_run("perm_timeout", temp_dir.path())
        .await
        .unwrap_or_abort();

    let tool_call_id = handle
        .request_tool_call(
            EventActor::new(ActorKind::Supervisor, Some("agent-supervisor".to_string())),
            Some("deep".to_string()),
            "shell.run",
            json!({"cmd": "sleep 1"}),
        )
        .await
        .unwrap_or_abort();

    wait_for_events(
        &handle,
        &run.events_path,
        "permission timeout failure",
        |event| {
            matches!(
                &event.payload,
                EventV1::ToolCallFinished(data)
                    if data.tool_call_id.as_str() == tool_call_id && data.status == ToolCallStatus::Failed
            )
        },
    )
    .await;
    handle.stop_run().await.unwrap_or_abort();

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
                if data.tool_call_id.as_str() == tool_call_id && data.status == ToolCallStatus::Failed
        )
    }));
    assert!(!events.iter().any(|event| {
        matches!(
            &event.payload,
            EventV1::ToolCallStarted(data) if data.tool_call_id.as_str() == tool_call_id
        )
    }));
}

pub(super) async fn malformed_question_answer_does_not_resolve_permission() {
    let temp_dir = tempfile::tempdir().unwrap_or_abort();
    let config = test_config(temp_dir.path());
    let handle = spawn_coordinator(
        config,
        Arc::new(RealClock::new()),
        Arc::new(DefaultRedactor::default()),
    );
    let run = handle
        .start_run("question_validation", temp_dir.path())
        .await
        .unwrap_or_abort();

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

    let before = wait_for_events(
        &handle,
        &run.events_path,
        "question permission request",
        |event| {
            matches!(
                &event.payload,
                EventV1::PermissionRequested(data) if data.kind == "question"
            )
        },
    )
    .await;
    let permission_id = before
        .iter()
        .find_map(|event| match &event.payload {
            EventV1::PermissionRequested(data) if data.kind == "question" => {
                Some(data.permission_id.clone())
            }
            _ => None,
        })
        .unwrap_or_abort();

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
    handle.stop_run().await.unwrap_or_abort();
}
