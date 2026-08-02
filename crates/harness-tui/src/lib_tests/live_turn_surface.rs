use super::*;
use crate::UnwrapOrAbort;

pub(super) fn live_shell_type_first_input_snapshot() {
    let mut app = app::AppState::new_live(None, false, None);
    for c in "draft prompt".chars() {
        app.handle_key(key(crossterm::event::KeyCode::Char(c)));
    }

    let rendered = render_live_lines(&app, 80, 24);

    assert_live_shell_frame_invariants(&rendered, 80, 24);
    assert!(!rendered.contains("Waiting for first turn…"));
    assert!(rendered.contains("draft prompt"));
    assert!(!rendered.contains("┌Session"));
    assert!(!rendered.contains("Start a conversation to begin"));
    assert_live_shell_document_composer_contract(
        &app,
        80,
        24,
        Some("draft prompt"),
        None,
        "Shift+Tab:mode",
    );
}

pub(super) fn live_shell_shift_enter_keeps_draft_multiline() {
    let mut app = app::AppState::new_live(None, false, None);
    for c in "first line".chars() {
        app.handle_key(key(crossterm::event::KeyCode::Char(c)));
    }
    app.handle_key(key_with_modifiers(
        crossterm::event::KeyCode::Enter,
        crossterm::event::KeyModifiers::SHIFT,
    ));
    for c in "second line".chars() {
        app.handle_key(key(crossterm::event::KeyCode::Char(c)));
    }

    assert_eq!(app.composer.prompt_history.len(), 0);
    assert_eq!(app.composer.prompt_buffer, "first line\nsecond line");
    assert_live_shell_contains(&app, 80, 24, &["first line", "second line"]);
    let rendered = render_live_lines(&app, 80, 24);
    assert!(!rendered.contains("Composer ·"));
    assert!(rendered.contains("first line"));
    assert!(rendered.contains("second line"));
}

pub(super) fn live_shell_enter_submits_and_echoes_prompt_snapshot() {
    let mut app = app::AppState::new_live(None, false, None);
    for c in "ship it".chars() {
        app.handle_key(key(crossterm::event::KeyCode::Char(c)));
    }

    app.handle_key(key(crossterm::event::KeyCode::Enter));

    assert_eq!(app.composer.prompt_buffer, "");
    assert_eq!(
        app.composer.prompt_history.last().map(String::as_str),
        Some("ship it")
    );
    let rendered = render_live_lines(&app, 80, 24);

    assert_live_shell_frame_invariants(&rendered, 80, 24);
    assert!(!rendered.contains("user (pending turn)"));
    assert!(rendered.contains("ship it"));
    assert!(!rendered.contains("   Waiting for response…"));
    assert!(
        rendered.contains("gpt-5")
            || rendered.contains("▪")
            || rendered.contains("◇")
            || rendered.contains("⠋")
    );
    assert!(
        rendered.contains('╭') || rendered.contains('❯'),
        "submitted live shell keeps bordered composer chrome\n{rendered}"
    );
}

pub(super) fn live_submitted_event_merges_duplicate_local_echo_before_rendering_response() {
    let mut app = app::AppState::new_live(None, false, None);
    for c in "ship it".chars() {
        app.handle_key(key(crossterm::event::KeyCode::Char(c)));
    }
    app.handle_key(key(crossterm::event::KeyCode::Enter));

    let local_echo = app.activities.back_mut().unwrap_or_abort();
    local_echo.status = app::ActivityStatus::Done;
    local_echo.transcript_text = "Ack.".to_string();
    app.activities
        .push_back(transcript_turn_group_test_activity(
            "req_live_echo_merge",
            app::ActivityStatus::Done,
            None,
            "Ack.",
        ));
    app.transcript_view.selected_activity_index = 1;
    app.transcript_view.follow_mode = false;

    app.ingest_event(envelope(
        1,
        Some("req_live_echo_merge"),
        harness_core::event::EventV1::UserMessageSubmitted(
            harness_core::event::UserMessageSubmittedEvent {
                request_id: "req_live_echo_merge".into(),
                text: "ship it".to_string(),
            },
        ),
    ));

    assert_eq!(app.activities.len(), 1);
    assert_eq!(app.transcript_view.selected_activity_index, 0);
    let activity = app.activities.back().unwrap_or_abort();
    assert_eq!(activity.request_id, "req_live_echo_merge");
    assert_eq!(activity.status, app::ActivityStatus::Done);
    assert_eq!(
        activity
            .user_message
            .as_ref()
            .map(|message| message.text.as_str()),
        Some("ship it")
    );
    let rendered = render_live_lines(&app, 80, 24);
    let lines = rendered.lines().collect::<Vec<_>>();
    assert_eq!(count_lines_containing(&lines, "Ack."), 1, "{rendered}");
    assert_eq!(
        count_lines_containing(&lines, "Waiting for response…"),
        0,
        "{rendered}"
    );
}

pub(super) fn live_provider_request_id_alias_reuses_local_turn_placeholder() {
    let mut app = app::AppState::new_live(None, false, None);
    for c in "hi".chars() {
        app.handle_key(key(crossterm::event::KeyCode::Char(c)));
    }
    app.handle_key(key(crossterm::event::KeyCode::Enter));

    app.ingest_event(envelope(
        1,
        Some("turn_req_alias"),
        harness_core::event::EventV1::UserMessageSubmitted(
            harness_core::event::UserMessageSubmittedEvent {
                request_id: "turn_req_alias".into(),
                text: "hi".to_string(),
            },
        ),
    ));
    app.ingest_event(envelope(
        2,
        Some("turn_req_alias"),
        harness_core::event::EventV1::ProviderRequestStarted(
            harness_core::event::ProviderRequestStartedEvent {
                request_id: "provider_req_alias".into(),
                provider_id: "default".to_string(),
                model_id: "gpt-5.4-mini".to_string(),
                prompt_summary: "hi".to_string(),
                request_digest: "digest-provider-alias".to_string(),
                metadata: None,
            },
        ),
    ));

    assert_eq!(app.activities.len(), 1);
    assert_eq!(app.activities[0].request_id, "turn_req_alias");
    assert_eq!(
        app.activities[0]
            .request_data
            .as_ref()
            .map(|data| data.request_id.as_str()),
        Some("provider_req_alias")
    );

    let rendered = render_live_lines(&app, 80, 24);
    let lines = rendered.lines().collect::<Vec<_>>();
    assert_eq!(
        count_lines_containing(&lines, "Waiting for response…"),
        1,
        "active aliased provider request keeps its waiting status visible\n{rendered}"
    );
}

pub(super) fn live_submitted_event_adopts_matching_local_echo_that_is_not_last() {
    let mut app = app::AppState::new_live(None, false, None);
    app.activities
        .push_back(transcript_turn_group_test_activity(
            "",
            app::ActivityStatus::Streaming,
            Some("ship it"),
            "",
        ));
    app.activities
        .push_back(transcript_turn_group_test_activity(
            "",
            app::ActivityStatus::Streaming,
            Some("other draft"),
            "",
        ));

    app.ingest_event(envelope(
        1,
        Some("req_non_last_echo"),
        harness_core::event::EventV1::UserMessageSubmitted(
            harness_core::event::UserMessageSubmittedEvent {
                request_id: "req_non_last_echo".into(),
                text: "ship it".to_string(),
            },
        ),
    ));

    assert_eq!(app.activities.len(), 2);
    assert_eq!(app.activities[0].request_id, "req_non_last_echo");
    assert_eq!(
        app.activities[0]
            .user_message
            .as_ref()
            .map(|message| message.text.as_str()),
        Some("ship it")
    );
    assert_eq!(app.activities[1].request_id, "");
    assert_eq!(
        app.activities[1]
            .user_message
            .as_ref()
            .map(|message| message.text.as_str()),
        Some("other draft")
    );
}

pub(super) fn live_shell_inline_tool_state_snapshot() {
    let mut app = app::AppState::new_live(None, false, None);
    app.ingest_event(envelope(
        1,
        Some("req_inline_tool"),
        harness_core::event::EventV1::UserMessageSubmitted(
            harness_core::event::UserMessageSubmittedEvent {
                request_id: "req_inline_tool".into(),
                text: "Read the file".to_string(),
            },
        ),
    ));
    app.ingest_event(envelope(
        2,
        Some("req_inline_tool"),
        harness_core::event::EventV1::ProviderRequestStarted(
            harness_core::event::ProviderRequestStartedEvent {
                request_id: "req_inline_tool".into(),
                provider_id: "openai".to_string(),
                model_id: "gpt-5-codex".to_string(),
                prompt_summary: "Read the file".to_string(),
                request_digest: "digest-inline-tool".to_string(),
                metadata: None,
            },
        ),
    ));
    app.ingest_event(envelope(
        3,
        Some("req_inline_tool"),
        harness_core::event::EventV1::ToolCallRequested(
            harness_core::event::ToolCallRequestedEvent {
                tool_call_id: "tc_inline_tool".into(),
                tool_id: "fs.read".to_string(),
                args_summary: r#"{"path":"src/lib.rs"}"#.to_string(),
                args_digest: "digest-inline-tool-args".to_string(),
                metadata: None,
            },
        ),
    ));
    app.ingest_event(permission_requested_event(
        4,
        "perm_inline_tool",
        "tc_inline_tool",
    ));

    let rendered = render_live_lines(&app, 80, 24);
    println!("{rendered}");

    assert_live_shell_contains(&app, 80, 24, &["Allow Edit", "always-approve", "┃"]);
    let rendered = render_live_lines(&app, 80, 24);
    assert!(
        !rendered.contains("timeout"),
        "permission dock must not show timeout chrome on decision stage\n{rendered}"
    );
}

pub(super) fn narrow_transcript_wrapped_top_level_turns_keep_alignment() {
    let mut app = app::AppState::new_live(None, false, None);
    let request_id = "req_wrap_alignment";
    app.ingest_event(envelope(
        1,
        Some(request_id),
        harness_core::event::EventV1::UserMessageSubmitted(
            harness_core::event::UserMessageSubmittedEvent {
                request_id: request_id.into(),
                text: "alpha bravo charlie delta echo foxtrot golf hotel india juliet kilo lima mike november oscar papa quebec romeo sierra tango".to_string(),
            },
        ),
    ));
    app.ingest_event(envelope(
        2,
        Some(request_id),
        harness_core::event::EventV1::ProviderRequestStarted(
            harness_core::event::ProviderRequestStartedEvent {
                request_id: request_id.into(),
                provider_id: "mock".to_string(),
                model_id: "model-1".to_string(),
                prompt_summary: "wrapping transcript rows".to_string(),
                request_digest: "digest-wrap-alignment".to_string(),
                metadata: None,
            },
        ),
    ));
    app.ingest_event(envelope(
        3,
        Some(request_id),
        harness_core::event::EventV1::ProviderStreamDelta(
            harness_core::event::ProviderStreamDeltaEvent {
                request_id: request_id.into(),
                delta: "assistant reply wraps across the narrow transcript column while keeping the same left alignment on each continuation row for readability".to_string(),
            },
        ),
    ));
    // Inverted scroll: MAX shows the top of the transcript so wrapped first lines stay visible.
    app.transcript_view.follow_mode = false;
    app.transcript_view.transcript_scroll = usize::MAX;

    let rendered = render_live_lines(&app, 60, 36);
    let lines = rendered.lines().collect::<Vec<_>>();

    let user_first = find_line_containing(&lines, "alpha bravo").unwrap_or_abort();
    let assistant_first =
        find_line_containing_from(&lines, user_first + 1, "assistant reply wraps")
            .unwrap_or_abort();
    let user_continuation = lines
        .iter()
        .enumerate()
        .skip(user_first + 1)
        .take(assistant_first.saturating_sub(user_first + 1))
        .find_map(|(index, line)| line.chars().any(char::is_alphanumeric).then_some(index))
        .unwrap_or_abort();
    let assistant_footer = find_line_containing_from(&lines, assistant_first + 1, "Worked for")
        .or_else(|| find_line_containing_from(&lines, assistant_first + 1, "model-1"))
        .or_else(|| find_line_containing_from(&lines, assistant_first + 1, "▪"))
        .unwrap_or_abort();
    let assistant_continuation = lines
        .iter()
        .enumerate()
        .skip(assistant_first + 1)
        .take(assistant_footer.saturating_sub(assistant_first + 1))
        .find_map(|(index, line)| line.chars().any(char::is_alphanumeric).then_some(index))
        .unwrap_or_abort();

    assert_eq!(
        first_alphanumeric_column(lines[user_continuation]),
        first_alphanumeric_column(lines[user_first]),
        "wrapped user continuations should align with the boxed user text column\n{rendered}"
    );
    assert!(!lines[user_first].contains('┃'));
    assert!(!lines[user_continuation].contains('┃'));
    assert!(lines[user_first].contains('❯'));
    assert_eq!(
        first_alphanumeric_column(lines[assistant_first]),
        first_alphanumeric_column(lines[assistant_continuation]),
        "wrapped assistant continuations should keep the same text column in narrow layouts\n{rendered}"
    );
}

pub(super) fn live_shell_permission_preserves_draft_snapshot() {
    let mut app = app::AppState::new_live(None, false, None);
    for c in "keep this draft".chars() {
        app.handle_key(key(crossterm::event::KeyCode::Char(c)));
    }
    app.ingest_event(permission_requested_event(
        1,
        "perm_snapshot",
        "tool_call_snapshot",
    ));

    assert_eq!(app.composer.prompt_buffer, "keep this draft");

    let rendered = render_live_lines(&app, 80, 24);
    println!("{rendered}");

    assert_live_shell_contains(&app, 80, 24, &["Allow Edit", "always-approve"]);
    assert!(rendered.contains("Draft preserved"));
}

pub(super) fn live_shell_degraded_bootstrap_snapshot() {
    let mut app = app::AppState::new_live(None, false, None);
    app.set_status_banner(Some(
        "live stream lagged by 2; replaying from seq 1".to_string(),
    ));
    println!("{}", render_live_lines(&app, 80, 24));

    assert_live_shell_contains(
        &app,
        80,
        24,
        &[
            "Degraded",
            "live stream lagged by 2; replaying from seq 1",
            "Draft locally until recovery completes.",
        ],
    );
    assert!(!render_live_lines(&app, 80, 24)
        .contains("Draft preserved locally while recovery completes."));
}

pub(super) fn live_shell_disconnected_stream_snapshot() {
    let mut app = app::AppState::new_live(None, false, None);
    app.set_status_banner(Some("live event stream disconnected".to_string()));
    println!("{}", render_live_lines(&app, 80, 24));

    assert_live_shell_contains(
        &app,
        80,
        24,
        &[
            "Disconnected",
            "live event stream disconnected",
            "Reopen the TUI, then continue from the transcript.",
        ],
    );
    assert!(!render_live_lines(&app, 80, 24)
        .contains("Draft preserved locally — reopen the TUI to reconnect."));
}

pub(super) fn live_status_strip_suppresses_request_digest_banner_details() {
    let mut app = app::AppState::new_live(None, false, None);
    app.handle_key(key(crossterm::event::KeyCode::Char('x')));
    app.set_status_banner(Some(
        "mock fixture missing for request_digest=digest-qa-crowding".to_string(),
    ));

    let rendered = render_live_lines(&app, 100, 24);
    assert!(!rendered.contains("request_digest="));
    assert!(!rendered.contains("digest-qa-crowding"));
    assert!(!app.runtime_state().summary.contains("request_digest="));
    assert!(!app.runtime_state().summary.contains("digest-qa-crowding"));
}

pub(super) fn live_status_strip_suppresses_request_digest_from_cancelled_summary() {
    let mut app = app::AppState::new_live(None, false, None);
    app.ingest_event(envelope(
        1,
        None,
        harness_core::event::EventV1::TaskCancelled(harness_core::event::TaskCancelledEvent {
            task_id: "req_cancelled_visual".to_string().into(),
            reason: "mock fixture missing for request_digest=digest-cancelled-visual".to_string(),
            task_scope: Some(harness_core::event::TaskTerminalScope::AgentTurn),
        }),
    ));

    let rendered = render_live_lines(&app, 160, 24);
    assert!(!rendered.contains("request_digest="));
    assert!(!rendered.contains("digest-cancelled-visual"));
    assert!(!app.runtime_state().summary.contains("request_digest="));
    assert!(!app
        .runtime_state()
        .summary
        .contains("digest-cancelled-visual"));
}

pub(super) fn parent_view_ignores_streaming_child_activity_after_returning_from_subagent() {
    let mut app =
        app::AppState::new_live(Some(PathBuf::from("/tmp/sessions/parent_run")), false, None);

    app.ingest_event(envelope(
        1,
        None,
        harness_core::event::EventV1::AgentSpawned(harness_core::event::AgentSpawnedEvent {
            agent_id: "agent_parent".to_string(),
            profile: "build".to_string(),
            parent_agent_id: None,
        }),
    ));
    app.ingest_event(envelope(
        2,
        None,
        harness_core::event::EventV1::AgentSpawned(harness_core::event::AgentSpawnedEvent {
            agent_id: "agent_child".to_string(),
            profile: "explore".to_string(),
            parent_agent_id: Some("agent_parent".to_string()),
        }),
    ));
    app.ingest_event(envelope_with_actor(
        3,
        Some("req_parent"),
        harness_core::event::EventActor::new(
            harness_core::event::ActorKind::Worker,
            Some("agent_parent".to_string()),
        ),
        harness_core::event::EventV1::UserMessageSubmitted(
            harness_core::event::UserMessageSubmittedEvent {
                request_id: "req_parent".into(),
                text: "Delegate the investigation".to_string(),
            },
        ),
    ));
    app.ingest_event(envelope_with_actor(
        4,
        Some("req_parent"),
        harness_core::event::EventActor::new(
            harness_core::event::ActorKind::Worker,
            Some("agent_parent".to_string()),
        ),
        harness_core::event::EventV1::ProviderRequestStarted(
            harness_core::event::ProviderRequestStartedEvent {
                request_id: "provider_req_parent".into(),
                provider_id: "default".to_string(),
                model_id: "gpt-5.4-mini".to_string(),
                prompt_summary: "Delegate the investigation".to_string(),
                request_digest: "digest-provider-parent".to_string(),
                metadata: None,
            },
        ),
    ));
    app.ingest_event(envelope_with_actor(
        5,
        Some("req_parent"),
        harness_core::event::EventActor::new(
            harness_core::event::ActorKind::Worker,
            Some("agent_parent".to_string()),
        ),
        harness_core::event::EventV1::ToolCallRequested(
            harness_core::event::ToolCallRequestedEvent {
                tool_call_id: "tool_call_child".into(),
                tool_id: "task".to_string(),
                args_summary: serde_json::json!({
                    "description": "inspect the lifecycle state",
                    "subagent_type": "explore",
                    "run_in_background": true
                })
                .to_string(),
                args_digest: "digest-tool-child".to_string(),
                metadata: None,
            },
        ),
    ));
    app.ingest_event(envelope_with_actor(
        6,
        Some("req_parent"),
        harness_core::event::EventActor::new(
            harness_core::event::ActorKind::Worker,
            Some("agent_parent".to_string()),
        ),
        harness_core::event::EventV1::ToolCallFinished(
            harness_core::event::ToolCallFinishedEvent {
                tool_call_id: "tool_call_child".into(),
                status: harness_core::event::ToolCallStatus::Succeeded,
                output_summary: Some("Background task scheduled".to_string()),
                output_digest: Some("digest-tool-child-output".to_string()),
                output_json: Some(serde_json::json!({
                    "profile": "explore",
                    "background": true,
                    "status": "scheduled",
                    "child_session_id": "agent_child"
                })),
                metadata: None,
            },
        ),
    ));
    app.ingest_event(envelope_with_actor(
        7,
        Some("req_parent"),
        harness_core::event::EventActor::new(
            harness_core::event::ActorKind::Worker,
            Some("agent_parent".to_string()),
        ),
        harness_core::event::EventV1::ProviderStreamDelta(
            harness_core::event::ProviderStreamDeltaEvent {
                request_id: "provider_req_parent".into(),
                delta: "Child task is running in the background.".to_string(),
            },
        ),
    ));
    app.ingest_event(envelope_with_actor(
        8,
        Some("req_parent"),
        harness_core::event::EventActor::new(
            harness_core::event::ActorKind::Worker,
            Some("agent_parent".to_string()),
        ),
        harness_core::event::EventV1::ProviderRequestFinished(
            harness_core::event::ProviderRequestFinishedEvent {
                request_id: "provider_req_parent".into(),
                finish_reason: "stop".to_string(),
                output_digest: Some("digest-parent-finished".to_string()),
                usage: None,
                metadata: None,
            },
        ),
    ));
    app.ingest_event(envelope_with_actor(
        9,
        Some("req_child_turn"),
        harness_core::event::EventActor::new(
            harness_core::event::ActorKind::Worker,
            Some("agent_child".to_string()),
        ),
        harness_core::event::EventV1::ProviderRequestStarted(
            harness_core::event::ProviderRequestStartedEvent {
                request_id: "provider_req_child".into(),
                provider_id: "default".to_string(),
                model_id: "gpt-5.4-mini".to_string(),
                prompt_summary: "Inspect the lifecycle state".to_string(),
                request_digest: "digest-provider-child".to_string(),
                metadata: None,
            },
        ),
    ));
    app.ingest_event(envelope_with_actor(
        10,
        Some("req_child_turn"),
        harness_core::event::EventActor::new(
            harness_core::event::ActorKind::Worker,
            Some("agent_child".to_string()),
        ),
        harness_core::event::EventV1::ProviderStreamDelta(
            harness_core::event::ProviderStreamDeltaEvent {
                request_id: "provider_req_child".into(),
                delta: "child-only work is still streaming".to_string(),
            },
        ),
    ));

    assert_eq!(app.runtime_state().kind, app::RuntimeStateKind::Success);
    assert!(!app.has_active_animations());

    let rendered = render_live_lines(&app, 100, 30);
    assert!(!rendered.contains("child-only work is still streaming"));
    assert!(!rendered.contains("Explore · gpt-5.4-mini"));
}
