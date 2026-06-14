use super::*;

pub(super) fn live_shell_footer_is_shortcuts_only() {
    let mut live = app::AppState::new_live(None, false, None);
    live.set_launch_metadata(
        app::LaunchMetadata::from_model_ref("deep", "proxy:gpt-5.4").with_mode_label("continued"),
    );

    let primary_render = render_live_lines(&live, 100, 30);
    assert!(!primary_render.contains("q quit"));
    assert!(primary_render.contains("Ctrl+p commands"));

    let reduced_render = render_live_lines(&live, 80, 24);
    assert!(!reduced_render.contains("q quit"));
    assert!(reduced_render.contains("Ctrl+p commands"));

    let minimal_render = render_live_lines(&live, 60, 18);
    assert!(!minimal_render.contains("q quit"));
    assert!(minimal_render.contains("Ctrl+p commands"));

    let replay =
        app::AppState::new_replay(PathBuf::from("/tmp/replay-session"), session_view_events());
    let replay_render = render_live_lines(&replay, 100, 24);
    let replay_lines = replay_render.lines().collect::<Vec<_>>();
    let replay_footer_row = find_last_line_containing(&replay_lines, "q quit")
        .map(|row| replay_lines[row].trim_end().to_string())
        .expect("replay footer row");
    assert_markers_in_order(
        &replay_footer_row,
        &["? shortcuts", "tab focus", "r reload", "q quit"],
    );
    assert!(!replay_footer_row.contains("Replay"));
    assert!(!replay_footer_row.contains("run_fixture"));
    assert!(!replay_footer_row.contains("/tmp/replay-session"));
    assert!(!replay_footer_row.contains("/status"));
}

pub(super) fn live_footer_status_cluster_renders_and_degrades_without_losing_permissions() {
    let app = live_footer_status_cluster_fixture_app();

    let standard_render = render_live_lines(&app, 100, 30);
    let standard_lines = standard_render.lines().collect::<Vec<_>>();
    let standard_footer = standard_lines
        .iter()
        .rev()
        .find(|line| line.contains("/status"))
        .expect("live footer cluster row");
    assert_markers_in_order(
        standard_footer,
        &["△ 1 Permission", "• 1 LSP", "⊙ 1 MCP", "/status"],
    );

    let minimal_render = render_live_lines(&app, 60, 20);
    let minimal_footer = minimal_render
        .lines()
        .rev()
        .find(|line| line.contains("△ 1 Permission"))
        .expect("minimal live footer keeps permission count");
    assert!(
        !minimal_footer.contains("• 1 LSP"),
        "minimal footer should drop LSP before permission count\n{minimal_render}"
    );
    assert!(
        !minimal_footer.contains("⊙ 1 MCP"),
        "minimal footer should drop MCP before permission count\n{minimal_render}"
    );

    clear_live_footer_status_cluster_fixture();
}

pub(super) fn live_footer_status_cluster_visual_probe_geometries() {
    let app = live_footer_status_cluster_fixture_app();

    for (width, height) in [(159, 40), (100, 30), (60, 20)] {
        let rendered = render_live_lines(&app, width, height);
        if std::env::var_os("HARNESS_TUI_G010_VISUAL_PROBE").is_some() {
            println!("G010_VISUAL_CAPTURE {width}x{height}\n{rendered}\nEND_G010_VISUAL_CAPTURE");
        }
        for (index, line) in rendered.lines().enumerate() {
            assert!(
                line.chars().count() <= usize::from(width),
                "rendered line {} exceeds {} columns\n{}",
                index + 1,
                width,
                rendered
            );
        }
        if width > 60 {
            assert!(rendered.contains("△ 1 Permission"));
            assert!(rendered.contains("• 1 LSP"));
            assert!(rendered.contains("⊙ 1 MCP"));
            assert!(rendered.contains("/status"));
        } else {
            assert!(rendered.contains("△ 1 Permission"));
            assert!(!rendered.contains("• 1 LSP"));
            assert!(!rendered.contains("⊙ 1 MCP"));
        }
    }

    clear_live_footer_status_cluster_fixture();
}

fn live_footer_status_cluster_fixture_app() -> app::AppState {
    let mut lsp_servers = std::collections::BTreeMap::new();
    lsp_servers.insert(
        "rust".to_string(),
        harness_core::config::LspServerConfig::default(),
    );
    harness_core::config::set_registered_lsp_config(harness_core::config::LspConfig {
        disabled: false,
        servers: lsp_servers,
    });

    let mut integrations = harness_core::config::IntegrationsConfig::default();
    integrations.mcp.servers.insert(
        "docs".to_string(),
        harness_core::config::McpServerConfig::Stdio {
            command: vec!["docs-mcp".to_string()],
            env: std::collections::BTreeMap::new(),
            cwd: None,
            timeout_secs: 30,
            enabled: true,
        },
    );
    harness_core::config::set_registered_integrations_config(integrations);
    harness_core::config::set_registered_mcp_server_connection_states(
        std::collections::BTreeMap::from([(
            "docs".to_string(),
            harness_core::config::McpServerConnectionState::Failed("offline".to_string()),
        )]),
    );

    let mut app = app::AppState::new_live(None, false, None);
    app.ingest_event(envelope(
        1,
        Some("req_footer"),
        harness_core::event::EventV1::UserMessageSubmitted(
            harness_core::event::UserMessageSubmittedEvent {
                request_id: "req_footer".to_string(),
                text: "Need permission".to_string(),
            },
        ),
    ));
    app.ingest_event(envelope(
        2,
        Some("req_footer"),
        harness_core::event::EventV1::ProviderRequestStarted(
            harness_core::event::ProviderRequestStartedEvent {
                request_id: "req_footer".to_string(),
                provider_id: "openai".to_string(),
                model_id: "gpt-5".to_string(),
                prompt_summary: "Need permission".to_string(),
                request_digest: "digest-footer".to_string(),
                metadata: None,
            },
        ),
    ));
    app.ingest_event(envelope(
        3,
        Some("req_footer"),
        harness_core::event::EventV1::ToolCallRequested(
            harness_core::event::ToolCallRequestedEvent {
                tool_call_id: "tool_footer".to_string(),
                tool_id: "edit".to_string(),
                args_summary: r#"{"path":"demo.txt"}"#.to_string(),
                args_digest: "digest-footer-tool".to_string(),
                metadata: None,
            },
        ),
    ));
    app.ingest_event(permission_requested_event(4, "perm_footer", "tool_footer"));

    app
}

fn clear_live_footer_status_cluster_fixture() {
    harness_core::config::set_registered_lsp_config(harness_core::config::LspConfig::default());
    harness_core::config::clear_registered_integrations_config();
}

pub(super) fn primary_and_wide_live_shells_hide_metadata_header() {
    let mut app = app::AppState::new_live(None, false, None);
    app.set_launch_metadata(
        app::LaunchMetadata::from_model_ref("deep", "proxy:gpt-5.4").with_mode_label("Demo"),
    );
    for event in session_view_events() {
        app.ingest_event(event);
    }

    for (width, height) in [(100, 30), (160, 48)] {
        let plan = FrameLayoutPlan::for_app(&app, ratatui::layout::Rect::new(0, 0, width, height));
        assert_eq!(
            plan.header.height, 0,
            "live shell header should stay hidden at {width}x{height}"
        );
        assert!(plan.live_anchor.is_none());

        let rendered = render_live_lines(&app, width, height);
        assert!(
            !rendered.contains("Composer ·"),
            "wide live shells should not reintroduce composer label chrome\n{rendered}"
        );
        assert!(
            !rendered
                .lines()
                .next()
                .unwrap_or_default()
                .contains("run run_fixture"),
            "wide live shells should not surface the old top identity bar\n{rendered}"
        );
    }
}

pub(super) fn completed_shell_bottom_rows_do_not_duplicate_command_help_footers() {
    let mut app = app::AppState::new_live(
        Some(PathBuf::from("/tmp/sessions/run_fixture")),
        false,
        None,
    );
    app.active_review_surface = Some(app::ReviewSurface::Events);
    app.focus = app::Focus::Prompt;
    app.composer.prompt_buffer = "keep this draft".to_string();
    app.composer.prompt_cursor = app.composer.prompt_buffer.chars().count();
    app.ingest_event(envelope(
        1,
        Some("req_completed_decrowded_footer"),
        harness_core::event::EventV1::RunFinished(harness_core::event::RunFinishedEvent {
            summary: "all tasks complete".to_string(),
        }),
    ));

    let rendered = render_live_lines(&app, 100, 24);
    let lines = rendered.lines().collect::<Vec<_>>();
    let footer_row = find_last_line_containing(&lines, "Tab focus").expect("completed footer row");

    assert_eq!(
        lines
            .iter()
            .filter(|line| line.contains("Tab focus"))
            .count(),
        1,
        "completed shell should keep a single footer hint row\n{rendered}"
    );
    assert!(lines[footer_row].contains("Ctrl+p commands"));
    assert!(lines[footer_row].contains("q quit"));
}

pub(super) fn live_state_matrix_preserves_shell_structure() {
    let mut ready = app::AppState::new_live(None, false, None);
    assert_live_shell_document_composer_contract(&ready, 100, 24, None, None, "Ctrl+p commands");

    for ch in "draft next turn".chars() {
        ready.handle_key(key(crossterm::event::KeyCode::Char(ch)));
    }
    assert_live_shell_document_composer_contract(
        &ready,
        100,
        24,
        Some("draft next turn"),
        None,
        "Ctrl+p commands",
    );

    let mut multiline = app::AppState::new_live(None, false, None);
    for ch in "draft".chars() {
        multiline.handle_key(key(crossterm::event::KeyCode::Char(ch)));
    }
    multiline.handle_key(key_with_modifiers(
        crossterm::event::KeyCode::Enter,
        crossterm::event::KeyModifiers::SHIFT,
    ));
    for ch in "second line".chars() {
        multiline.handle_key(key(crossterm::event::KeyCode::Char(ch)));
    }
    assert_live_shell_document_composer_contract(
        &multiline,
        100,
        24,
        Some("draft"),
        None,
        "Ctrl+p commands",
    );

    let mut streaming = app::AppState::new_live(None, false, None);
    streaming.ingest_event(envelope(
        1,
        Some("req_streaming_matrix"),
        harness_core::event::EventV1::ProviderRequestStarted(
            harness_core::event::ProviderRequestStartedEvent {
                request_id: "req_streaming_matrix".to_string(),
                provider_id: "openai".to_string(),
                model_id: "gpt-5-codex".to_string(),
                prompt_summary: "streaming".to_string(),
                request_digest: "digest-streaming".to_string(),
                metadata: None,
            },
        ),
    ));
    streaming.ingest_event(envelope(
        2,
        Some("req_streaming_matrix"),
        harness_core::event::EventV1::ProviderStreamDelta(
            harness_core::event::ProviderStreamDeltaEvent {
                request_id: "req_streaming_matrix".to_string(),
                delta: "partial output".to_string(),
            },
        ),
    ));
    assert_live_shell_document_composer_contract(
        &streaming,
        100,
        24,
        None,
        None,
        "Ctrl+p commands",
    );

    let mut degraded = app::AppState::new_live(None, false, None);
    degraded.set_status_banner(Some(
        "live stream lagged by 2; replaying from seq 1".to_string(),
    ));
    assert_live_shell_document_composer_contract(
        &degraded,
        100,
        24,
        Some("Draft preserved locally while recovery completes."),
        None,
        "Degraded",
    );

    let mut disconnected = app::AppState::new_live(None, false, None);
    disconnected.set_status_banner(Some("live event stream disconnected".to_string()));
    assert_live_shell_document_composer_contract(
        &disconnected,
        100,
        24,
        Some("Draft preserved locally — reopen the TUI to reconnect."),
        None,
        "Disconnected",
    );

    let mut failure = app::AppState::new_live(None, false, None);
    failure.set_status_banner(Some("runtime error while updating session".to_string()));
    assert_live_shell_document_composer_contract(&failure, 100, 24, None, None, "Failure");

    let mut completed = app::AppState::new_live(
        Some(PathBuf::from("/tmp/sessions/run_fixture")),
        false,
        None,
    );
    completed.ingest_event(envelope(
        1,
        Some("req_completed_matrix"),
        harness_core::event::EventV1::RunFinished(harness_core::event::RunFinishedEvent {
            summary: "done".to_string(),
        }),
    ));
    assert_live_shell_document_composer_contract(&completed, 100, 24, None, None, "Tab focus");
}
