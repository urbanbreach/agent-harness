use super::*;
use crate::UnwrapOrAbort;

pub(crate) async fn permission_rule_bash_selector_is_enforced_at_tool_call_site() {
    let temp_dir = tempfile::tempdir().unwrap_or_abort();
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
            default: {
              system_prompt: "Deep work",
              tools: ["shell.run"]
            }
          },
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
    .unwrap_or_abort();
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
        .unwrap_or_abort();

    let actor = EventActor::new(ActorKind::Supervisor, Some("agent-supervisor".to_string()));
    let denied = handle
        .request_tool_call(
            actor.clone(),
            Some("default".to_string()),
            "shell.run",
            json!({"cmd": "git status"}),
        )
        .await
        .expect_err("exact bash rule should deny");
    assert!(matches!(denied, CoordinatorError::PermissionDenied(_)));

    let allowed_tool_call_id = handle
        .request_tool_call(
            actor,
            Some("default".to_string()),
            "shell.run",
            json!({"cmd": "git diff"}),
        )
        .await
        .unwrap_or_abort();

    wait_for_events(
        &handle,
        &run.events_path,
        "allowed bash rule tool call to start",
        |event| {
            matches!(
                &event.payload,
                EventV1::ToolCallStarted(data) if data.tool_call_id.as_str() == allowed_tool_call_id
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
                if data.reason.as_deref() == Some("policy denied request (shell)")
        )
    }));
    assert!(events.iter().any(|event| {
        matches!(
            &event.payload,
            EventV1::ToolCallStarted(data) if data.tool_call_id.as_str() == allowed_tool_call_id
        )
    }));
}

pub(crate) fn task_permission_rule_selector_uses_only_subagent_type() {
    assert_eq!(
        permission_rule_request_selectors(
            Path::new("/workspace"),
            PermissionKind::Task,
            &json!({"subagent_type": "explore"}),
        ),
        vec![PermissionRuleRequest::TaskAgent("explore".to_string())]
    );
    assert_eq!(
        permission_rule_request_selectors(
            Path::new("/workspace"),
            PermissionKind::Task,
            &json!({"profileName": "reviewer"}),
        ),
        Vec::<PermissionRuleRequest>::new()
    );
    assert_eq!(
        permission_rule_request_selectors(
            Path::new("/workspace"),
            PermissionKind::Task,
            &json!({"subagent_type": "  ", "agent": " general "}),
        ),
        Vec::<PermissionRuleRequest>::new()
    );
    assert_eq!(
        permission_rule_request_selectors(
            Path::new("/workspace"),
            PermissionKind::Task,
            &json!({"category": "quick"}),
        ),
        Vec::<PermissionRuleRequest>::new()
    );
}

pub(crate) async fn permission_rule_task_selector_is_enforced_at_tool_call_site() {
    let temp_dir = tempfile::tempdir().unwrap_or_abort();
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
            default: {
              system_prompt: "Deep work",
              tools: ["task"]
            }
          },
          permission: {
            bash: "allow",
            edit: "allow",
            question: "allow",
            task: {
              general: "deny",
              explore: "allow",
              "*": "allow"
            },
            webfetch: "allow",
            websearch: "allow",
            codesearch: "allow",
            lsp: "allow"
          }
        }
        "#,
    )
    .unwrap_or_abort();
    let mut config = test_config(temp_dir.path());
    config.permission_policy = PermissionPolicy::from_config(&parsed);

    let handle = spawn_coordinator(
        config,
        Arc::new(FakeClock::new()),
        Arc::new(DefaultRedactor::default()),
    );
    let run = handle
        .start_run("permission_rule_task", temp_dir.path())
        .await
        .unwrap_or_abort();
    let actor = EventActor::new(ActorKind::Supervisor, Some("agent-supervisor".to_string()));

    let whitespace_denied = handle
        .request_tool_call(
            actor.clone(),
            Some("default".to_string()),
            "task",
            json!({
                "description": "Review",
                "prompt": "Review work",
                "subagent_type": " general ",
                "load_skills": [],
            }),
        )
        .await
        .expect_err("trimmed task selector should deny general");
    assert!(matches!(
        whitespace_denied,
        CoordinatorError::PermissionDenied(_)
    ));

    let allowed_tool_call_id = handle
        .request_tool_call(
            actor,
            Some("default".to_string()),
            "task",
            json!({
                "description": "Explore",
                "prompt": "Explore work",
                "subagent_type": "explore",
                "load_skills": [],
            }),
        )
        .await
        .unwrap_or_abort();

    wait_for_events(
        &handle,
        &run.events_path,
        "allowed task rule tool call to start",
        |event| {
            matches!(
                &event.payload,
                EventV1::ToolCallStarted(data)
                    if data.tool_call_id.as_str() == allowed_tool_call_id
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
                if data.reason.as_deref() == Some("policy denied request (task)")
        )
    }));
    assert!(events.iter().any(|event| {
        matches!(
            &event.payload,
            EventV1::ToolCallStarted(data) if data.tool_call_id.as_str() == allowed_tool_call_id
        )
    }));
}

pub(crate) async fn perm_ask_path_blocks_until_resolved() {
    let temp_dir = tempfile::tempdir().unwrap_or_abort();
    let mut config = test_config(temp_dir.path());
    config.permission_policy = ask_shell_permission_policy(1_000);

    let handle = spawn_coordinator(
        config,
        Arc::new(FakeClock::new()),
        Arc::new(DefaultRedactor::default()),
    );

    let run = handle
        .start_run("perm_ask", temp_dir.path())
        .await
        .unwrap_or_abort();

    let tool_call_id = handle
        .request_tool_call(
            EventActor::new(ActorKind::Supervisor, Some("agent-supervisor".to_string())),
            Some("deep".to_string()),
            "shell.run",
            json!({"cmd": "echo blocked"}),
        )
        .await
        .unwrap_or_abort();

    let before_resolve = wait_for_events(
        &handle,
        &run.events_path,
        "permission request before resolve",
        |event| {
            matches!(
                &event.payload,
                EventV1::PermissionRequested(data)
                    if data.tool_call_id.as_ref().map(|id| id.as_str()) == Some(tool_call_id.as_str())
            )
        },
    )
    .await;
    assert!(
        !before_resolve.iter().any(|event| {
            matches!(
                &event.payload,
                EventV1::ToolCallStarted(data) if data.tool_call_id.as_str() == tool_call_id
            )
        }),
        "tool call must not start before permission resolution"
    );

    let permission_id = before_resolve
        .iter()
        .find_map(|event| match &event.payload {
            EventV1::PermissionRequested(data)
                if data.tool_call_id.as_ref().map(|id| id.as_str())
                    == Some(tool_call_id.as_str()) =>
            {
                Some(data.permission_id.clone())
            }
            _ => None,
        })
        .unwrap_or_abort();

    handle
        .resolve_permission(permission_id, PermissionDecision::Allow, None)
        .await
        .unwrap_or_abort();

    wait_for_events(
        &handle,
        &run.events_path,
        "tool start after permission resolve",
        |event| {
            matches!(
                &event.payload,
                EventV1::ToolCallStarted(data) if data.tool_call_id.as_str() == tool_call_id
            )
        },
    )
    .await;
    handle.stop_run().await.unwrap_or_abort();

    let events = read_events(&run.events_path);
    let requested_idx = events
        .iter()
        .position(|event| {
            matches!(
                &event.payload,
                EventV1::PermissionRequested(data)
                    if data.tool_call_id.as_ref().map(|id| id.as_str()) == Some(tool_call_id.as_str())
            )
        })
        .unwrap_or_abort();
    let resolved_idx = events
        .iter()
        .position(|event| {
            matches!(
                &event.payload,
                EventV1::PermissionResolved(data)
                    if data.decision == crate::event::PermissionDecision::Allow
            )
        })
        .unwrap_or_abort();
    let started_idx = events
        .iter()
        .position(|event| {
            matches!(
                &event.payload,
                EventV1::ToolCallStarted(data) if data.tool_call_id.as_str() == tool_call_id
            )
        })
        .unwrap_or_abort();

    assert!(requested_idx < resolved_idx);
    assert!(resolved_idx < started_idx);
}
