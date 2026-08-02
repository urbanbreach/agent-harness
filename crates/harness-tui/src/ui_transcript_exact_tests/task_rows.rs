use super::super::*;

#[cfg(test)]
pub(crate) fn exact_test_transcript_task_rows_show_child_status_duration_and_counts() {
    fn event(
        seq: u64,
        ts: &str,
        actor: harness_core::event::EventActor,
        correlation_id: Option<&str>,
        payload: harness_core::event::EventV1,
    ) -> harness_core::event::EventEnvelopeV1 {
        harness_core::event::EventEnvelopeV1 {
            schema_version: harness_core::event::SCHEMA_VERSION,
            event_id: format!("evt_task_rows_{seq:04}"),
            seq,
            run_id: "run_task_rows".into(),
            mono_ms: seq * 100,
            ts: Some(ts.to_string()),
            actor,
            correlation_id: correlation_id.map(str::to_string),
            causation_id: None,
            stream_key: None,
            payload,
        }
    }

    let mut app = AppState::new_live(None, false, None);
    app.ingest_event(event(
        1,
        "2026-03-22T14:36:00Z",
        harness_core::event::EventActor::new(
            harness_core::event::ActorKind::User,
            Some("interactive-user".to_string()),
        ),
        Some("req_parent"),
        harness_core::event::EventV1::UserMessageSubmitted(
            harness_core::event::UserMessageSubmittedEvent {
                request_id: "req_parent".into(),
                text: "Audit transcript parity".to_string(),
            },
        ),
    ));
    app.ingest_event(event(
        2,
        "2026-03-22T14:36:01Z",
        harness_core::event::EventActor::new(
            harness_core::event::ActorKind::Worker,
            Some("agent_parent".to_string()),
        ),
        Some("req_parent"),
        harness_core::event::EventV1::ProviderRequestStarted(
            harness_core::event::ProviderRequestStartedEvent {
                request_id: "req_parent".into(),
                provider_id: "default".to_string(),
                model_id: "model-1".to_string(),
                prompt_summary: "Audit transcript parity".to_string(),
                request_digest: "digest-parent".to_string(),
                metadata: None,
            },
        ),
    ));
    app.ingest_event(event(
        3,
        "2026-03-22T14:36:02Z",
        harness_core::event::EventActor::new(
            harness_core::event::ActorKind::System,
            Some("coordinator".to_string()),
        ),
        Some("req_parent"),
        harness_core::event::EventV1::ToolCallRequested(
            harness_core::event::ToolCallRequestedEvent {
                tool_call_id: "tc_task".into(),
                tool_id: "task".to_string(),
                args_summary:
                    r#"{"description":"audit transcript parity","subagent_type":"researcher"}"#
                        .to_string(),
                args_digest: "digest-task-call".to_string(),
                metadata: Some(harness_core::event::ToolCallMetadata {
                    canonical_tool_id: Some("task".to_string()),
                    alias_source_tool_id: None,
                    lineage: Some(harness_core::event::TaskLineageMetadata {
                        parent_tool_call_id: Some("tc_task".to_string()),
                        parent_request_id: Some("req_parent".to_string()),
                        child_session_id: Some("agent_worker".to_string()),
                        child_request_id: Some("req_child".to_string()),
                        ..harness_core::event::TaskLineageMetadata::default()
                    }),
                    ..harness_core::event::ToolCallMetadata::default()
                }),
            },
        ),
    ));
    app.ingest_event(event(
        4,
        "2026-03-22T14:36:03Z",
        harness_core::event::EventActor::new(
            harness_core::event::ActorKind::System,
            Some("coordinator".to_string()),
        ),
        Some("req_parent"),
        harness_core::event::EventV1::ToolCallStarted(harness_core::event::ToolCallStartedEvent {
            tool_call_id: "tc_task".into(),
        }),
    ));
    app.ingest_event(event(
        5,
        "2026-03-22T14:36:04Z",
        harness_core::event::EventActor::new(
            harness_core::event::ActorKind::Worker,
            Some("agent_worker".to_string()),
        ),
        Some("req_child"),
        harness_core::event::EventV1::TaskScheduled(harness_core::event::TaskScheduledEvent {
            task_id: "task_child".to_string().into(),
            state: harness_core::event::TaskScheduleState::Started,
            queue_key: Some("agent:running:researcher".to_string()),
        }),
    ));
    app.ingest_event(event(
        6,
        "2026-03-22T14:36:05Z",
        harness_core::event::EventActor::new(
            harness_core::event::ActorKind::System,
            Some("coordinator".to_string()),
        ),
        Some("req_child"),
        harness_core::event::EventV1::ToolCallRequested(
            harness_core::event::ToolCallRequestedEvent {
                tool_call_id: "tc_child_read".into(),
                tool_id: "fs.read".to_string(),
                args_summary: r#"{"path":"src/ui.rs"}"#.to_string(),
                args_digest: "digest-child-read".to_string(),
                metadata: None,
            },
        ),
    ));

    let running_tool = &app.activities[0].tool_calls[0];
    let running_row = app.transcript_task_row_for_tool_call(running_tool);
    let running_section = build_transcript_tool_call_section(
        running_tool,
        &AppState::default(),
        running_row.as_ref(),
        true,
        false,
        false,
        false,
        None,
    );
    let mut running_lines = Vec::new();
    {
        let render = append_tool_call_section_lines(
            &running_section,
            &Theme::default(),
            120,
            Theme::default().surface.panel,
        );
        running_lines.extend(render.lines);
    }
    let running_text = transcript_test_line_texts(running_lines).join("\n");
    assert!(running_text.contains("Researcher Task — audit transcript parity"));
    assert!(
        running_text.contains("↳ Read src/ui.rs"),
        "running task row should show the active child tool detail\n{running_text}"
    );
    assert!(!running_text.contains("Researcher Agent"));
    assert!(!running_text.contains("↳ 1 toolcalls"));
    assert!(!running_text.contains("1 toolcalls · 100ms"));

    app.ingest_event(event(
        7,
        "2026-03-22T14:36:06Z",
        harness_core::event::EventActor::new(
            harness_core::event::ActorKind::Worker,
            Some("agent_worker".to_string()),
        ),
        Some("req_child"),
        harness_core::event::EventV1::ProviderRequestStarted(
            harness_core::event::ProviderRequestStartedEvent {
                request_id: "req_child".into(),
                provider_id: "default".to_string(),
                model_id: "model-1".to_string(),
                prompt_summary: "Audit transcript parity".to_string(),
                request_digest: "digest-child-retry".to_string(),
                metadata: Some(harness_core::event::ProviderRequestStartedMetadata {
                    retry: Some(harness_core::event::ProviderRequestRetryMetadata {
                        attempt: 1,
                        max_attempts: 3,
                        delay_ms: Some(1_000),
                        category: Some(harness_providers::ProviderErrorCategory::RateLimited),
                    }),
                    ..harness_core::event::ProviderRequestStartedMetadata::default()
                }),
            },
        ),
    ));

    let retry_tool = &app.activities[0].tool_calls[0];
    let retry_row = app.transcript_task_row_for_tool_call(retry_tool);
    let retry_section = build_transcript_tool_call_section(
        retry_tool,
        &AppState::default(),
        retry_row.as_ref(),
        true,
        false,
        false,
        false,
        None,
    );
    let retry_text = transcript_test_line_texts(
        append_tool_call_section_lines(
            &retry_section,
            &Theme::default(),
            120,
            Theme::default().surface.panel,
        )
        .lines,
    )
    .join("\n");
    assert!(retry_text.contains("↳ Retrying (attempt 1) · rate_limited"));

    app.ingest_event(event(
        8,
        "2026-03-22T14:36:06Z",
        harness_core::event::EventActor::new(
            harness_core::event::ActorKind::System,
            Some("coordinator".to_string()),
        ),
        Some("req_parent"),
        harness_core::event::EventV1::ToolCallFinished(
            harness_core::event::ToolCallFinishedEvent {
                tool_call_id: "tc_task".into(),
                status: harness_core::event::ToolCallStatus::Succeeded,
                output_summary: Some("foreground subagent moved to background".to_string()),
                output_digest: Some("digest-task-backgrounded".to_string()),
                output_json: Some(serde_json::json!({
                    "background": true,
                    "child_session_id": "agent_worker",
                    "child_request_id": "req_child",
                    "mode": "background",
                    "status": "scheduled",
                })),
                metadata: Some(harness_core::event::ToolCallMetadata {
                    canonical_tool_id: Some("task".to_string()),
                    lineage: Some(harness_core::event::TaskLineageMetadata {
                        parent_tool_call_id: Some("tc_task".to_string()),
                        parent_request_id: Some("req_parent".to_string()),
                        child_session_id: Some("agent_worker".to_string()),
                        child_request_id: Some("req_child".to_string()),
                        ..harness_core::event::TaskLineageMetadata::default()
                    }),
                    ..harness_core::event::ToolCallMetadata::default()
                }),
            },
        ),
    ));

    let detached_active_tool = &app.activities[0].tool_calls[0];
    let detached_active_row = app.transcript_task_row_for_tool_call(detached_active_tool);
    let detached_active_section = build_transcript_tool_call_section(
        detached_active_tool,
        &AppState::default(),
        detached_active_row.as_ref(),
        true,
        false,
        false,
        false,
        None,
    );
    let detached_active_text = transcript_test_line_texts(
        append_tool_call_section_lines(
            &detached_active_section,
            &Theme::default(),
            120,
            Theme::default().surface.panel,
        )
        .lines,
    )
    .join("\n");
    assert!(detached_active_text.contains("Researcher Task (background) — audit transcript parity"));
    assert!(detached_active_text.contains("↳ Retrying (attempt 1) · rate_limited"));
    assert!(!detached_active_text.contains("↳ 1 toolcall ·"));

    app.ingest_event(event(
        9,
        "2026-03-22T14:36:06Z",
        harness_core::event::EventActor::new(
            harness_core::event::ActorKind::System,
            Some("coordinator".to_string()),
        ),
        Some("req_child"),
        harness_core::event::EventV1::ToolCallRequested(
            harness_core::event::ToolCallRequestedEvent {
                tool_call_id: "tc_child_grep".into(),
                tool_id: "fs.grep".to_string(),
                args_summary: r#"{"pattern":"task row"}"#.to_string(),
                args_digest: "digest-child-grep".to_string(),
                metadata: None,
            },
        ),
    ));
    app.ingest_event(event(
        10,
        "2026-03-22T14:36:07Z",
        harness_core::event::EventActor::new(
            harness_core::event::ActorKind::Worker,
            Some("agent_worker".to_string()),
        ),
        Some("req_child"),
        harness_core::event::EventV1::ProviderStreamDelta(
            harness_core::event::ProviderStreamDeltaEvent {
                request_id: "req_child".into(),
                delta: "CHILD SUBAGENT DETAILS SHOULD STAY OUT OF PARENT".to_string(),
            },
        ),
    ));
    app.ingest_event(event(
        11,
        "2026-03-22T14:36:07Z",
        harness_core::event::EventActor::new(
            harness_core::event::ActorKind::Worker,
            Some("agent_worker".to_string()),
        ),
        Some("req_child"),
        harness_core::event::EventV1::TaskCompleted(harness_core::event::TaskCompletedEvent {
            task_id: "task_child".to_string().into(),
            result_summary: "Found the inline transcript path.".to_string(),
            result_digest: "digest-task-child".to_string(),
            metadata: Some(harness_core::event::TaskCompletionMetadata {
                lineage: Some(harness_core::event::TaskLineageMetadata {
                    parent_tool_call_id: Some("tc_task".to_string()),
                    parent_request_id: Some("req_parent".to_string()),
                    child_session_id: Some("agent_worker".to_string()),
                    child_request_id: Some("req_child".to_string()),
                    ..harness_core::event::TaskLineageMetadata::default()
                }),
                task_scope: Some(harness_core::event::TaskTerminalScope::ToolCall),
                timing: Some(harness_core::event::ExecutionTimingMetadata {
                    started_mono_ms: Some(400),
                    finished_mono_ms: Some(2_000),
                    elapsed_ms: Some(1_600),
                }),
                hook_executions: Vec::new(),
            }),
        }),
    ));
    let completed_tool = &app.activities[0].tool_calls[0];
    let completed_row = app.transcript_task_row_for_tool_call(completed_tool);
    let completed_section = build_transcript_tool_call_section(
        completed_tool,
        &AppState::default(),
        completed_row.as_ref(),
        true,
        false,
        false,
        false,
        None,
    );
    let mut completed_lines = Vec::new();
    {
        let render = append_tool_call_section_lines(
            &completed_section,
            &Theme::default(),
            120,
            Theme::default().surface.panel,
        );
        completed_lines.extend(render.lines);
    }
    let completed_text = transcript_test_line_texts(completed_lines).join("\n");
    assert!(completed_text.contains("Researcher Task (background) — audit transcript parity"));
    assert!(completed_text.contains("↳ 2 toolcalls · 1.6s"));
    assert!(!completed_text.contains("background_output("));
    assert!(!completed_text.contains("task(task_id=\"agent_worker\")"));
    assert!(!completed_text.contains("Found the inline transcript path."));
    assert!(!completed_text.contains("child session finished"));

    let expanded_completed_section = build_transcript_tool_call_section(
        completed_tool,
        &AppState::default(),
        completed_row.as_ref(),
        true,
        false,
        true,
        false,
        None,
    );
    let expanded_completed_render = append_tool_call_section_lines(
        &expanded_completed_section,
        &Theme::default(),
        120,
        Theme::default().surface.panel,
    );
    let expanded_completed_text =
        transcript_test_line_texts(expanded_completed_render.lines).join("\n");
    assert!(!expanded_completed_text.contains("Found the inline transcript path."));
    assert!(expanded_completed_text.contains("↳ 2 toolcalls · 1.6s"));
    assert!(!expanded_completed_text.contains("child session finished"));

    let parent_transcript_text = transcript_test_line_texts(build_transcript_lines_for_width(
        &app,
        &Theme::default(),
        120,
    ))
    .join("\n");
    assert!(
        parent_transcript_text.contains("Researcher Task (background) — audit transcript parity")
    );
    assert!(parent_transcript_text.contains("↳ 2 toolcalls · 1.6s"));
    assert!(!parent_transcript_text.contains("background_output("));
    assert!(!parent_transcript_text.contains("task(task_id=\"agent_worker\")"));
    assert!(!parent_transcript_text.contains("view subagents"));
    assert!(
        !parent_transcript_text.contains("CHILD SUBAGENT DETAILS SHOULD STAY OUT OF PARENT"),
        "parent transcript should keep delegated child turns behind the task row\n{parent_transcript_text}"
    );
}

#[cfg(test)]
pub(crate) fn exact_test_transcript_task_rows_match_reference_inline_title_and_no_hint() {
    let mut app = AppState::default();
    let mut entry =
        transcript_section_model_test_activity("request-task", ActivityStatus::Streaming, "");
    let mut tool_call = transcript_section_model_test_tool_call("call-task", "task");
    tool_call.args_summary =
        r#"{"description":"audit transcript parity","subagent_type":"researcher"}"#.to_string();
    tool_call.status = ToolCallDisplayStatus::Running;
    tool_call.lineage = Some(crate::app::TaskLineageEntry {
        parent_tool_call_id: Some("call-task".to_string()),
        parent_task_id: Some("task-parent".to_string()),
        parent_request_id: Some("request-task".to_string()),
        child_session_id: Some("session-child".to_string()),
        child_request_id: Some("request-child".to_string()),
    });
    entry.tool_calls.push(tool_call);
    app.activities = std::collections::VecDeque::from(vec![entry]);

    let lines = transcript_test_line_texts(build_transcript_lines_for_width(
        &app,
        &Theme::default(),
        96,
    ));
    let rendered = lines.join("\n");

    assert!(
        rendered.contains("◆ Researcher Task — audit transcript parity")
            || rendered.contains("Researcher Task — audit transcript parity"),
        "task row should use Harness task title shape\n{rendered}"
    );
    assert!(
        !rendered.contains("audit transcript parity · Researcher Agent"),
        "task row should not use Harness title/subtitle wording\n{rendered}"
    );
    assert!(
        !rendered.contains("view subagents"),
        "subagent inspection belongs to the footer surface, not an extra transcript hint row\n{rendered}"
    );
}
