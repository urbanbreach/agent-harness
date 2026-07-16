use super::*;

pub(super) fn operator_sidebar_dedupes_running_task_tool_against_child_orchestration_row() {
    let mut app = app::AppState::new_live(None, false, None);

    app.ingest_event(envelope(
        1,
        None,
        harness_core::event::EventV1::AgentSpawned(harness_core::event::AgentSpawnedEvent {
            agent_id: "agent_child".to_string(),
            profile: "explore".to_string(),
            parent_agent_id: Some("agent_parent".to_string()),
        }),
    ));
    app.ingest_event(envelope(
        2,
        Some("req_parent_sidebar"),
        harness_core::event::EventV1::UserMessageSubmitted(
            harness_core::event::UserMessageSubmittedEvent {
                request_id: "req_parent_sidebar".into(),
                text: "Run an explore subagent".to_string(),
            },
        ),
    ));
    app.ingest_event(envelope(
        3,
        Some("req_parent_sidebar"),
        harness_core::event::EventV1::ToolCallRequested(
            harness_core::event::ToolCallRequestedEvent {
                tool_call_id: "tool_call_explore_running".into(),
                tool_id: "task".to_string(),
                args_summary: serde_json::json!({
                    "description": "inspect sidebar lifecycle",
                    "subagent_type": "explore",
                    "run_in_background": true
                })
                .to_string(),
                args_digest: "digest-explore-running".to_string(),
                metadata: None,
            },
        ),
    ));
    app.ingest_event(envelope(
        4,
        Some("req_parent_sidebar"),
        harness_core::event::EventV1::ToolCallStarted(harness_core::event::ToolCallStartedEvent {
            tool_call_id: "tool_call_explore_running".into(),
        }),
    ));
    app.ingest_event(envelope_with_actor(
        5,
        Some("provider_req_child_sidebar"),
        harness_core::event::EventActor::new(
            harness_core::event::ActorKind::Worker,
            Some("agent_child".to_string()),
        ),
        harness_core::event::EventV1::TaskScheduled(harness_core::event::TaskScheduledEvent {
            task_id: "task_child_sidebar".to_string().into(),
            state: harness_core::event::TaskScheduleState::Started,
            queue_key: Some("agent:running:explore".to_string()),
        }),
    ));

    let sidebar = operator_sidebar_text(&app);
    assert!(sidebar.contains("• ⠋ Explore Task"), "{sidebar}");
    assert!(!sidebar.contains("2 tasks"), "{sidebar}");
    assert_eq!(sidebar.matches("Explore Task").count(), 1, "{sidebar}");
}

pub(super) fn live_shell_details_drawer_orchestration_snapshot() {
    let app = orchestration_details_drawer_app(0);
    let card_body = orchestration_details_drawer_card_body(&app, 7, 76);

    println!("{card_body}");
    insta::assert_snapshot!(card_body, @r###"
overview · 1 active agents · 2 queued · 1 running · 1 stale
watch · stale for 3001 ms
 stale  task_stale · w1/deep · scan
 running  task_run · supervisor/n/a · queue:none
 queued  task_queue · system/n/a · tool:read
 queued  tool_call_1 · system/n/a · tool:fs.read
 completed  task_done · w2/scout · tool:done
"###);
}

pub(super) fn live_shell_details_drawer_orchestration_primary_snapshot() {
    let app = orchestration_details_drawer_app(0);

    let rendered = render_live_lines(&app, 100, 30);
    println!("{rendered}");
    assert!(rendered.contains("Explain the refactor"));
    assert!(rendered.contains("▼ MCP"));
    assert!(rendered.contains("▼ LSP"));
    assert!(rendered.contains("▶ Modified Files"));
    assert!(!rendered.contains("turn 1"));
    assert!(!rendered.contains("Current runtime: default · gpt-5-codex"));
    assert!(!rendered.contains("provider openai"));
    assert!(
        rendered.contains("No MCP integrations configured")
            || rendered.contains("No MCP servers configured")
            || rendered.contains("websearch Disconnected")
    );
    assert!(rendered.contains("No active LSP servers"));
    assert!(!rendered.contains("No modified files"));
}

pub(super) fn live_shell_details_drawer_orchestration_overflow_snapshot() {
    let app = orchestration_details_drawer_app(4);
    let card_body = orchestration_details_drawer_card_body(&app, 7, 76);

    println!("{card_body}");
    insta::assert_snapshot!(card_body, @r###"
overview · 1 active agents · 2 queued · 1 running · 1 stale
watch · stale for 3001 ms
 stale  task_stale · w1/deep · scan
 running  task_run · supervisor/n/a · queue:none
 queued  task_queue · system/n/a · tool:read
 queued  tool_call_1 · system/n/a · tool:fs.read
+5 more
"###);
}

pub(super) fn live_details_drawer_orchestration_warning_fallback() {
    let mut app = app::AppState::new_live(None, false, None);
    for event in session_view_events() {
        app.ingest_event(event);
    }
    app.live_details_drawer_open = true;

    let card_body = orchestration_details_drawer_card_body(&app, 7, 76);
    assert!(card_body.contains("watch · none"));
    assert!(card_body.contains("overview · 0 active agents · 1 queued · 0 running · 0 stale"));
}

pub(super) fn layout_plan_primary_geometry_docks_live_details_sidebar() {
    let mut app = app::AppState::new_live(None, false, None);
    app.active_tab = app::Tab::Run;
    for event in session_view_events() {
        app.ingest_event(event);
    }
    app.live_details_drawer_open = true;

    let plan = layout::FrameLayoutPlan::for_app(&app, ratatui::layout::Rect::new(0, 0, 100, 30));

    assert_eq!(plan.shell, ratatui::layout::Rect::new(0, 0, 100, 30));
    assert_eq!(
        plan.transcript,
        Some(ratatui::layout::Rect::new(0, 0, 100, 24))
    );
    assert_eq!(plan.operator_sidebar, None);
    assert_eq!(
        plan.details_overlay,
        Some(ratatui::layout::Rect::new(58, 0, 42, 24))
    );
    assert_eq!(plan.status, None);
    assert_eq!(
        plan.composer,
        Some(ratatui::layout::Rect::new(0, 24, 100, 5))
    );
    assert_eq!(
        plan.disclosure,
        Some(ratatui::layout::Rect::new(0, 29, 100, 1))
    );
}

pub(super) fn layout_plan_minimum_geometry_stacks_live_details_drawer() {
    let mut app = app::AppState::new_live(None, false, None);
    app.active_tab = app::Tab::Run;
    app.live_details_drawer_open = true;

    let plan = layout::FrameLayoutPlan::for_app(&app, ratatui::layout::Rect::new(0, 0, 80, 24));

    assert_eq!(plan.shell, ratatui::layout::Rect::new(2, 0, 76, 24));
    assert_eq!(
        plan.transcript,
        Some(ratatui::layout::Rect::new(2, 0, 76, 18))
    );
    assert_eq!(
        plan.details_overlay,
        Some(ratatui::layout::Rect::new(36, 0, 42, 18))
    );
    assert_eq!(plan.status, None);
    assert_eq!(
        plan.composer,
        Some(ratatui::layout::Rect::new(2, 18, 76, 5))
    );
    assert_eq!(
        plan.disclosure,
        Some(ratatui::layout::Rect::new(2, 23, 76, 1))
    );
}

pub(super) fn live_session_shell_removes_tab_chrome_and_debug_drawer() {
    let mut app = app::AppState::new_live(None, false, None);
    for event in session_view_events() {
        app.ingest_event(event);
    }

    let plan = layout::FrameLayoutPlan::for_app(&app, ratatui::layout::Rect::new(0, 0, 160, 48));
    let rendered = render_live_lines(&app, 160, 48);

    assert!(plan.operator_sidebar.is_none());
    assert!(plan.details_overlay.is_none());
    assert!(!rendered.contains("Tabs"));
}

pub(super) fn wide_shell_hides_header_when_sidebar_is_visible() {
    let mut app = app::AppState::new_live(None, false, None);
    for event in session_view_events() {
        app.ingest_event(event);
    }

    let plan = layout::FrameLayoutPlan::for_app(&app, ratatui::layout::Rect::new(0, 0, 160, 48));
    let rendered = render_live_lines(&app, 160, 48);
    let lines = rendered.lines().collect::<Vec<_>>();
    let first_line = lines.first().copied().unwrap_or_default().to_string();

    assert_eq!(plan.header.height, 0);
    assert!(plan.operator_sidebar.is_none());
    assert!(plan.live_anchor.is_none());
    assert!(!first_line.contains("run run_fixture"));
    assert!(
        lines.iter().take(4).any(|line| {
            line.contains("Explain the refactor")
                || line.contains("Working through the steps.")
                || line.contains("Read src/ui.rs")
        }),
        "wide shell transcript content should begin immediately at the top of the shell\n{rendered}"
    );
}

pub(super) fn replay_shell_uses_read_only_operator_layout() {
    let app = app::AppState::new_replay(
        std::path::PathBuf::from("/tmp/replay-session"),
        session_view_events(),
    );

    let plan = layout::FrameLayoutPlan::for_app(&app, ratatui::layout::Rect::new(0, 0, 160, 48));
    let rendered = render_live_lines(&app, 160, 48);

    assert!(plan.header.height > 0);
    assert!(plan.operator_sidebar.is_some());
    assert!(plan.details_overlay.is_none());
    assert!(!rendered.contains("Tabs"));
    assert!(rendered.contains("Replay · read-only"));
    assert!(rendered.contains("▼ MCP"));
    assert!(rendered.contains("▶ Modified Files"));
}

pub(super) fn replay_read_only_composer_matches_quiet_contract() {
    let app =
        app::AppState::new_replay(PathBuf::from("/tmp/replay-session"), session_view_events());

    assert_replay_read_only_composer_contract(&app, 100, 24, "Replay · read-only", "? shortcuts");

    let rendered = render_live_lines(&app, 100, 24);
    assert!(!rendered.contains("Replay archive · read-only"));
    assert!(!rendered.contains("Composer ·"));
}

pub(super) fn replay_read_only_quiet_contract_survives_primary_compact_and_dense_geometries() {
    let app =
        app::AppState::new_replay(PathBuf::from("/tmp/replay-session"), session_view_events());

    assert_replay_read_only_composer_contract(&app, 100, 30, "Replay · read-only", "? shortcuts");
    assert_replay_read_only_composer_contract(&app, 90, 36, "Replay · read-only", "? shortcuts");
    assert_replay_read_only_composer_contract(&app, 80, 24, "Replay · read-only", "? shortcuts");
    assert_replay_read_only_composer_contract(&app, 60, 18, "Replay · read-only", "q quit");
}

pub(super) fn completed_shell_uses_disabled_quiet_composer() {
    let mut app = app::AppState::new_live(
        Some(PathBuf::from("/tmp/sessions/run_fixture")),
        false,
        None,
    );
    app.ingest_event(envelope(
        1,
        Some("req_completed_quiet_composer"),
        harness_core::event::EventV1::RunFinished(harness_core::event::RunFinishedEvent {
            summary: "done".to_string(),
        }),
    ));

    assert_live_shell_document_composer_contract(&app, 100, 30, None, None, "Tab focus");

    let rendered = render_live_lines(&app, 100, 30);
    assert!(!rendered.contains("Next action"));
    assert!(!rendered.contains("Continue this session"));
    assert!(!rendered.contains("Composer ·"));
}

pub(super) fn replay_and_completed_states_preserve_read_only_and_session_preserved_copy() {
    let mut replay =
        app::AppState::new_replay(PathBuf::from("/tmp/replay-session"), session_view_events());
    replay.focus = app::Focus::Prompt;
    for ch in "blocked in replay".chars() {
        replay.handle_key(key(crossterm::event::KeyCode::Char(ch)));
    }
    replay.handle_key(key(crossterm::event::KeyCode::Enter));

    let replay_render = render_live_lines(&replay, 100, 24);
    assert!(replay.composer.prompt_buffer.is_empty());
    assert!(replay_render.contains("Replay · read-only"));
    assert!(replay_render.contains("Replay is read-only"));
    assert!(!replay_render.contains("blocked in replay"));

    let mut completed = app::AppState::new_live(
        Some(PathBuf::from("/tmp/sessions/run_fixture")),
        false,
        None,
    );
    completed.ingest_event(envelope(
        1,
        Some("req_completed_copy_guard"),
        harness_core::event::EventV1::RunFinished(harness_core::event::RunFinishedEvent {
            summary: "done".to_string(),
        }),
    ));

    let completed_render = render_live_lines(&completed, 100, 30);
    assert!(completed_render.contains("Tab focus"));
    assert!(completed_render.contains("q quit"));
}
