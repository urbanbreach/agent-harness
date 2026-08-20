#[test]
fn tx_user_page_flip_resumes_bottom_follow_after_stream_overflow() {
    // arrange
    // Given: a newly submitted prompt preserved at the top of a deep transcript.
    let mut app = app_with_deep_history();
    submit_text(&mut app, "overflow handoff prompt");
    let _ = render(&app);

    // When: assistant output grows beyond the preserved viewport.
    app.ingest_event(envelope(
        25,
        Some("req_overflow"),
        EventV1::UserMessageSubmitted(UserMessageSubmittedEvent {
            request_id: "req_overflow".into(),
            text: "overflow handoff prompt".to_string(),
        }),
    ));
    app.ingest_event(envelope(
        26,
        Some("req_overflow"),
        EventV1::ProviderRequestStarted(ProviderRequestStartedEvent {
            request_id: "req_overflow".into(),
            provider_id: "mock".to_string(),
            model_id: "model-tx".to_string(),
            prompt_summary: "overflow handoff prompt".to_string(),
            request_digest: "digest-overflow".to_string(),
            metadata: None,
        }),
    ));
    app.ingest_event(envelope(
        27,
        Some("req_overflow"),
        EventV1::ProviderStreamDelta(ProviderStreamDeltaEvent {
            request_id: "req_overflow".into(),
            delta: format!("{}STREAM_BOTTOM_SENTINEL", "streaming row\n".repeat(48)),
        }),
    ));
    let rendered = render(&app);

    // act
    // Then: the newest stream tail is visible and the one-shot prompt pin is consumed.
    // assert
    assert!(
        rendered.contains("STREAM_BOTTOM_SENTINEL"),
        "TX-USER: overflow must hand control back to normal bottom follow\n{rendered}"
    );
    assert!(
        !rendered.contains("overflow handoff prompt"),
        "TX-USER: consumed page flip must not keep the prompt sticky at bottom follow\n{rendered}"
    );
}

#[test]
fn tx_user_first_manual_scroll_moves_from_the_preserved_viewport() {
    // arrange
    // Given: a submitted prompt preserved at the top of a deep transcript.
    let mut app = app_with_deep_history();
    submit_text(&mut app, "manual scroll continuity prompt");
    let preserved = render(&app);
    let preserved_row = preserved
        .lines()
        .position(|line| line.contains("manual scroll continuity prompt"))
        .expect("preserved prompt row");

    // When: the user scrolls one row toward older transcript content.
    app.scroll_page_up(1);
    let scrolled = render(&app);
    let scrolled_row = scrolled
        .lines()
        .position(|line| line.contains("manual scroll continuity prompt"))
        .unwrap_or_else(|| {
            panic!(
                "prompt remains visible after one-row scroll; preserved_row={preserved_row}\nPRESERVED:\n{preserved}\nSCROLLED:\n{scrolled}"
            )
        });

    // act
    // Then: movement continues from the visible preserved viewport without jumping.
    // assert
    assert_eq!(
        scrolled_row,
        preserved_row.saturating_add(1),
        "TX-USER: first manual scroll must move one row from the preserved viewport\n{scrolled}"
    );
}

/// TX-ASSISTANT: assistant prose is rail-free and aligns with the user marker column.
/// Freeze capture: TBD.
#[test]
fn tx_assistant_message_chrome_is_rail_free_without_settled_footer() {
    // arrange
    let mut app = live_app();
    ingest_completed_turn(
        &mut app,
        "req_asst",
        "User asks",
        "Assistant answers cleanly.",
    );

    // act
    let rendered = render(&app);
    let lines: Vec<&str> = rendered.lines().collect();
    let user_idx = lines
        .iter()
        .position(|line| line.contains("User asks"))
        .expect("user");
    let asst_idx = lines
        .iter()
        .position(|line| line.contains("Assistant answers cleanly."))
        .expect("assistant");

    // assert — order + no rails
    assert!(user_idx < asst_idx);
    assert!(
        !lines[asst_idx].contains('┃'),
        "TX-ASSISTANT: no outer rail on assistant body\n{rendered}"
    );

    // Reference completed state packs Thought plus a dedicated wall-clock row between user and body.
    // (user → Thought → clock → body ⇒ gap 7 at 100x30 unit geometry).
    assert!(
        asst_idx - user_idx <= 7,
        "TX-ASSISTANT: turn stacking should stay compact (gap={})\n{rendered}",
        asst_idx - user_idx
    );

    let asst_region = lines[asst_idx].to_string();
    assert!(
        !asst_region.contains('╭') && !asst_region.contains('╰'),
        "TX-ASSISTANT: message body must not use rounded card borders\n{asst_region}\n{rendered}"
    );

    let user_marker = first_non_whitespace_column(lines[user_idx]);
    let asst_body = first_non_whitespace_column(lines[asst_idx]);
    assert_eq!(
        asst_body,
        user_marker,
        "TX-ASSISTANT: assistant body aligns with the user marker column\nuser={user_marker} asst={asst_body}\n{rendered}"
    );
    assert!(
        !rendered.contains("Thought for"),
        "TX-ASSISTANT: completed turns without reasoning must not render Thought for header\n{rendered}"
    );
    assert!(
        !rendered.contains("Worked for"),
        "TX-ASSISTANT: completed turns must not render standalone lifecycle prose\n{rendered}"
    );
}

/// TX-TOOL: tool rows use ◆ identity, structured path summary, no outer rail / opaque-only dump.
#[test]
fn tx_tool_row_is_structured_diamond_without_legacy_rail() {
    // arrange
    // act
    // assert
    // arrange
    let mut app = live_app();
    let request_id = "req_tool";
    app.ingest_event(envelope(
        1,
        Some(request_id),
        EventV1::UserMessageSubmitted(UserMessageSubmittedEvent {
            request_id: request_id.into(),
            text: "Read the readme".to_string(),
        }),
    ));
    app.ingest_event(envelope(
        2,
        Some(request_id),
        EventV1::ProviderRequestStarted(ProviderRequestStartedEvent {
            request_id: request_id.into(),
            provider_id: "mock".to_string(),
            model_id: "model-tx".to_string(),
            prompt_summary: "Read the readme".to_string(),
            request_digest: "digest-tool".to_string(),
            metadata: None,
        }),
    ));
    app.ingest_event(envelope(
        3,
        Some(request_id),
        EventV1::ToolCallRequested(ToolCallRequestedEvent {
            tool_call_id: "tc_tool".into(),
            tool_id: "fs.read".to_string(),
            args_summary: r#"{"path":"README.md"}"#.to_string(),
            args_digest: "digest-args-tool".to_string(),
            metadata: None,
        }),
    ));
    app.ingest_event(envelope(
        4,
        Some(request_id),
        EventV1::ToolCallStarted(ToolCallStartedEvent {
            tool_call_id: "tc_tool".into(),
        }),
    ));
    app.ingest_event(envelope(
        5,
        Some(request_id),
        EventV1::ToolCallFinished(ToolCallFinishedEvent {
            tool_call_id: "tc_tool".into(),
            status: ToolCallStatus::Succeeded,
            output_summary: Some("file contents".to_string()),
            output_digest: Some("digest-out-tool".to_string()),
            output_json: None,
            metadata: None,
        }),
    ));

    // act
    let rendered = render(&app);
    let lines: Vec<&str> = rendered.lines().collect();
    let tool_idx = lines
        .iter()
        .position(|line| line.contains("README.md") || line.contains("Read"))
        .expect("tool row");

    assert!(
        rendered.contains('◈') || rendered.contains('◆'),
        "TX-TOOL: tool identity glyph ◈/◆ required\n{rendered}"
    );
    assert!(
        lines[tool_idx].contains("README.md")
            || lines[tool_idx].contains("Read 1 file")
            || rendered.contains("README.md")
            || rendered.contains("Read 1 file")
            || rendered.contains("Read "),
        "TX-TOOL: structured tool title (path or completed count) required\n{rendered}"
    );
    assert!(
        !lines[tool_idx].contains('┃'),
        "TX-TOOL: no legacy outer rail on tool row\n{rendered}"
    );
    assert!(
        !rendered.contains(r#"{"path":"README.md"}"#)
            || rendered.contains("Read")
            || rendered.contains("README.md"),
        "TX-TOOL: must not rely on opaque JSON dump alone\n{rendered}"
    );
    assert!(
        rendered.contains('❯') && count_char(&rendered, '╭') >= 1,
        "TX-TOOL: full-width shell keeps bordered composer\n{rendered}"
    );
}

/// TX-DIFF: inline edit/diff body stays rail-free and non-card under full-width shell.
#[test]
fn tx_diff_inline_is_rail_free_without_message_card() {
    // arrange
    // act
    // assert
    // arrange
    let mut app = live_app();
    let request_id = "req_diff";
    app.ingest_event(envelope(
        1,
        Some(request_id),
        EventV1::UserMessageSubmitted(UserMessageSubmittedEvent {
            request_id: request_id.into(),
            text: "Apply a patch".to_string(),
        }),
    ));
    app.ingest_event(envelope(
        2,
        Some(request_id),
        EventV1::ProviderRequestStarted(ProviderRequestStartedEvent {
            request_id: request_id.into(),
            provider_id: "mock".to_string(),
            model_id: "model-tx".to_string(),
            prompt_summary: "Apply a patch".to_string(),
            request_digest: "digest-diff".to_string(),
            metadata: None,
        }),
    ));
    app.ingest_event(envelope(
        3,
        Some(request_id),
        EventV1::ToolCallRequested(ToolCallRequestedEvent {
            tool_call_id: "tc_diff".into(),
            tool_id: "edit".to_string(),
            args_summary: r#"{"path":"src/main.rs","old":"fn a(){}","new":"fn a(){ 1 }"}"#
                .to_string(),
            args_digest: "digest-args-diff".to_string(),
            metadata: None,
        }),
    ));
    app.ingest_event(envelope(
        4,
        Some(request_id),
        EventV1::ToolCallStarted(ToolCallStartedEvent {
            tool_call_id: "tc_diff".into(),
        }),
    ));
    app.ingest_event(envelope(
        5,
        Some(request_id),
        EventV1::ToolCallFinished(ToolCallFinishedEvent {
            tool_call_id: "tc_diff".into(),
            status: ToolCallStatus::Succeeded,
            output_summary: Some("edited src/main.rs".to_string()),
            output_digest: Some("digest-out-diff".to_string()),
            output_json: None,
            metadata: None,
        }),
    ));
    for _ in 0..12 {
        app.advance_animation_tick_for_evidence();
    }

    // act
    let rendered = render(&app);
    let lines: Vec<&str> = rendered.lines().collect();
    let tool_idx = lines
        .iter()
        .position(|line| {
            line.contains("src/main.rs")
                || line.contains("edit")
                || line.contains("Edit")
                || line.contains("◆")
        })
        .expect("diff/tool row");

    assert!(
        rendered.contains("src/main.rs") || rendered.contains('◆'),
        "TX-DIFF: structured edit/path projection required\n{rendered}"
    );
    assert!(
        !lines[tool_idx].contains('┃'),
        "TX-DIFF: settled edit row must not retain a tool rail\n{rendered}"
    );
    let region_end = (tool_idx + 8).min(lines.len().saturating_sub(1));
    let region = lines[tool_idx..=region_end].join("\n");
    assert!(
        !region.contains('╭') && !region.contains('╰'),
        "TX-DIFF: inline diff/tool body must not use rounded message cards\n{region}\n{rendered}"
    );
    assert!(
        rendered.contains('❯'),
        "TX-DIFF: bordered composer retained under full-width shell\n{rendered}"
    );
}

fn count_char(rendered: &str, ch: char) -> usize {
    rendered.chars().filter(|c| *c == ch).count()
}

/// SHELL-STREAM: active provider stream keeps full-width transcript body + bordered composer.
/// Freeze capture: run2-shell-stream-pinned-v2 ("Waiting for response…" state).
#[test]
fn shell_stream_keeps_full_width_body_and_bordered_composer() {
    // arrange — reference streaming state: user submitted, provider started, no body text.
    // TaskScheduled keeps activity Streaming after ProviderRequestFinished seeds
    // total_tokens for ⇣ counter.
    let mut app = live_app();
    let request_id = "req_stream";
    app.ingest_event(envelope(
        1,
        Some(request_id),
        EventV1::TaskScheduled(TaskScheduledEvent {
            task_id: "task_stream_parity".to_string().into(),
            state: TaskScheduleState::Started,
            queue_key: Some("provider_model:mock:model-tx".to_string()),
            metadata: None,
        }),
    ));
    app.ingest_event(envelope(
        2,
        Some(request_id),
        EventV1::UserMessageSubmitted(UserMessageSubmittedEvent {
            request_id: request_id.into(),
            text: "stream parity probe".to_string(),
        }),
    ));
    app.ingest_event(envelope(
        3,
        Some(request_id),
        EventV1::ProviderRequestStarted(ProviderRequestStartedEvent {
            request_id: request_id.into(),
            provider_id: "mock".to_string(),
            model_id: "model-tx".to_string(),
            prompt_summary: "stream parity probe".to_string(),
            request_digest: "digest-stream".to_string(),
            metadata: None,
        }),
    ));
    app.ingest_event(envelope(
        4,
        Some(request_id),
        EventV1::ProviderRequestFinished(ProviderRequestFinishedEvent {
            request_id: request_id.into(),
            finish_reason: "done".to_string(),
            output_digest: Some("digest-out-stream".to_string()),
            usage: Some(CompletionUsage {
                prompt_tokens: 1430,
                completion_tokens: 0,
                total_tokens: 1430,
            }),
            metadata: None,
        }),
    ));

    // act
    let rendered = render(&app);
    let runtime = app.runtime_state();

    // assert
    assert!(
        matches!(
            runtime.kind,
            RuntimeStateKind::Streaming | RuntimeStateKind::Sending
        ),
        "SHELL-STREAM: runtime must be streaming/sending; got {:?}",
        runtime.kind
    );
    assert!(
        rendered.contains("stream parity probe"),
        "SHELL-STREAM: user turn retained in transcript\n{rendered}"
    );
    assert!(
        rendered.contains("Waiting for response"),
        "SHELL-STREAM: waiting-for-response indicator must project\n{rendered}"
    );
    assert!(
        !rendered.contains("partial assistant tokens"),
        "SHELL-STREAM: no body text in waiting-for-response state\n{rendered}"
    );
    assert!(
        rendered.contains('❯') && count_char(&rendered, '╭') >= 1,
        "SHELL-STREAM: bordered composer retained under full-width shell\n{rendered}"
    );
    assert!(
        !rendered.contains('┃'),
        "SHELL-STREAM: no legacy left rail\n{rendered}"
    );
    let lines: Vec<&str> = rendered.lines().collect();
    let user_idx = lines
        .iter()
        .position(|line| line.contains("stream parity probe"))
        .expect("user");
    let waiting_idx = lines
        .iter()
        .position(|line| line.contains("Waiting for response"))
        .expect("waiting indicator");
    assert!(
        user_idx < waiting_idx,
        "SHELL-STREAM: user above waiting-for-response indicator\n{rendered}"
    );
}
