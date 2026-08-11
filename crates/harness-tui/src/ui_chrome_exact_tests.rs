#![allow(
    deprecated,
    reason = "deprecated compaction event variants kept for backward compatibility tests"
)]

use super::*;
use crate::UnwrapOrAbort;

#[cfg(test)]
pub(crate) fn exact_test_subagent_footer_matches_harness_layout() {
    use ratatui::{backend::TestBackend, Terminal};

    let app = AppState::new_replay(std::path::PathBuf::from("/tmp/subagent"), Vec::new());
    let theme = Theme::default();
    let info = crate::app::SubagentSessionInfo {
        label: "Researcher".to_string(),
        title: "audit transcript parity".to_string(),
        parent_label: "parent_run".to_string(),
        index: 2,
        total: 3,
        usage: Some("12,345 (8%) · $0.42".to_string()),
    };

    let backend = TestBackend::new(80, 6);
    let mut terminal = Terminal::new(backend).unwrap_or_abort();
    terminal
        .draw(|frame| {
            render_subagent_footer(
                frame,
                &app,
                Rect::new(0, 1, 80, 3),
                Rect::new(0, 1, 80, 3),
                &theme,
                &info,
            );
        })
        .unwrap_or_abort();

    let buffer = terminal.backend().buffer();
    let rows = buffer
        .content
        .chunks(80)
        .map(|row| row.iter().map(|cell| cell.symbol()).collect::<String>())
        .collect::<Vec<_>>();
    assert!(
        rows[2].contains("Researcher (2 of 3)  12,345 (8%) · $0.42"),
        "footer should render Harness label, sibling count, and usage\n{}",
        rows[2]
    );
    assert!(
        rows[2].contains("Parent ↑") && rows[2].contains("Prev ←") && rows[2].contains("Next →"),
        "footer should render Harness navigation labels and shortcuts\n{}",
        rows[2]
    );
    let rendered = rows.join("\n");
    assert!(
        !rendered.contains("audit transcript parity")
            && !rendered.contains("No subagent activity yet"),
        "footer should not render the old Harness title/body inspector\n{rendered}"
    );
}

#[cfg(test)]
pub(crate) fn exact_test_subagent_replay_suppresses_parent_replay_dock() {
    use ratatui::{backend::TestBackend, Terminal};

    let actor = harness_core::event::EventActor::new(
        harness_core::event::ActorKind::System,
        Some("coordinator".to_string()),
    );
    let worker = harness_core::event::EventActor::new(
        harness_core::event::ActorKind::Worker,
        Some("agent_alpha".to_string()),
    );
    let event =
        |seq, correlation_id: Option<&str>, actor, payload| harness_core::event::EventEnvelopeV1 {
            schema_version: harness_core::event::SCHEMA_VERSION,
            event_id: format!("evt-subagent-footer-{seq:04}"),
            seq,
            run_id: "agent_alpha".into(),
            mono_ms: seq,
            ts: None,
            actor,
            correlation_id: correlation_id.map(str::to_string),
            causation_id: None,
            stream_key: Some("run:agent_alpha".to_string()),
            payload,
        };
    let app = AppState::new_replay(
        std::path::PathBuf::from("/tmp/harness-subagent-parent/agent_alpha"),
        vec![
            event(
                1,
                None,
                actor.clone(),
                harness_core::event::EventV1::AgentSpawned(
                    harness_core::event::AgentSpawnedEvent {
                        agent_id: "agent_alpha".to_string(),
                        profile: "researcher".to_string(),
                        parent_agent_id: Some("agent_parent".to_string()),
                    },
                ),
            ),
            event(
                2,
                Some("req_000001"),
                actor.clone(),
                harness_core::event::EventV1::ToolCallRequested(
                    harness_core::event::ToolCallRequestedEvent {
                        tool_call_id: "toolcall_000001".into(),
                        tool_id: "task".to_string(),
                        args_summary: r#"{"description":"child lineage marker"}"#.to_string(),
                        args_digest: "digest-child-marker".to_string(),
                        metadata: Some(harness_core::event::ToolCallMetadata {
                            lineage: Some(harness_core::event::TaskLineageMetadata {
                                parent_session_id: Some("parent_run".to_string()),
                                child_session_id: Some("agent_alpha".to_string()),
                                child_request_id: Some("req_000001".to_string()),
                                ..harness_core::event::TaskLineageMetadata::default()
                            }),
                            ..harness_core::event::ToolCallMetadata::default()
                        }),
                    },
                ),
            ),
            event(
                3,
                Some("req_000001"),
                worker.clone(),
                harness_core::event::EventV1::ProviderRequestStarted(
                    harness_core::event::ProviderRequestStartedEvent {
                        request_id: "req_000001".into(),
                        provider_id: "mock".to_string(),
                        model_id: "model-1".to_string(),
                        prompt_summary: "audit transcript parity".to_string(),
                        request_digest: "digest-child-start".to_string(),
                        metadata: None,
                    },
                ),
            ),
            event(
                4,
                Some("req_000001"),
                worker,
                harness_core::event::EventV1::ProviderRequestFinished(
                    harness_core::event::ProviderRequestFinishedEvent {
                        request_id: "req_000001".into(),
                        finish_reason: "stop".to_string(),
                        output_digest: Some("digest-child-output".to_string()),
                        usage: None,
                        metadata: None,
                    },
                ),
            ),
        ],
    );

    let backend = TestBackend::new(100, 30);
    let mut terminal = Terminal::new(backend).unwrap_or_abort();
    terminal
        .draw(|frame| render_app(frame, &app))
        .unwrap_or_abort();
    let rows = terminal
        .backend()
        .buffer()
        .content
        .chunks(100)
        .map(|row| row.iter().map(|cell| cell.symbol()).collect::<String>())
        .collect::<Vec<_>>()
        .join("\n");

    assert!(rows.contains("Researcher (1 of 1)"), "{rows}");
    assert!(rows.contains("Parent ↑"), "{rows}");
    assert!(!rows.contains("No subagent activity yet"), "{rows}");
    assert!(!rows.contains("Replay is read-only"), "{rows}");
    assert!(!rows.contains("Researcher · parent_run"), "{rows}");

    let theme = app.theme();
    let plan = FrameLayoutPlan::for_app(&app, Rect::new(0, 0, 100, 30));
    let transcript = plan.transcript.unwrap_or_abort();
    let sample_x = transcript.x.saturating_add(
        theme
            .live_shell
            .rhythm
            .transcript_gutter_x
            .min(transcript.width.saturating_sub(1)),
    );
    let sample_y = transcript.y.saturating_add(
        theme
            .live_shell
            .rhythm
            .transcript_gutter_y
            .min(transcript.height.saturating_sub(1)),
    );
    assert_eq!(
        terminal.backend().buffer()[(sample_x, sample_y)].bg,
        theme.surface.canvas,
        "subagent replay should use the same transcript surface as the main chat"
    );
}
#[cfg(test)]
pub(crate) fn exact_test_live_control_dock_renders_shared_surface() {
    use ratatui::{backend::TestBackend, Terminal};

    let mut app = AppState::new_live(None, false, None);
    let mut events = crate::lib_tests::session_view_events();
    events.pop();
    for event in events {
        app.ingest_event(event);
    }
    let width = 100;
    let height = 30;
    let area = Rect::new(0, 0, width, height);
    let plan = FrameLayoutPlan::for_app(&app, area);
    let dock = plan.dock.unwrap_or_abort();
    let status = dock.status.unwrap_or_abort();
    let composer = dock.composer;

    assert_eq!(dock.shell.height, composer.height.saturating_add(5));
    assert_eq!(dock.shell.y, status.y);
    assert_eq!(composer.y, status.y.saturating_add(2));
    assert_eq!(dock.disclosure, Some(Rect::new(2, 28, 96, 1)));

    let backend = TestBackend::new(width, height);
    let mut terminal = Terminal::new(backend).unwrap_or_abort();
    terminal
        .draw(|frame| super::render_app(frame, &app))
        .unwrap_or_abort();

    let buffer = terminal.backend().buffer().clone();
    let right_edge = dock
        .shell
        .x
        .saturating_add(dock.shell.width.saturating_sub(1));

    let expected_surface = app.theme().surface.canvas;
    assert_eq!(buffer[(right_edge, composer.y)].bg, expected_surface);
    assert_eq!(buffer[(right_edge, dock.shell.y)].bg, expected_surface);
    assert!(
        matches!(
            buffer[(right_edge, composer.y)].symbol(),
            "╮" | "│" | "╯" | " "
        ),
        "bordered live composer uses rounded chrome, got {:?}",
        buffer[(right_edge, composer.y)].symbol()
    );
    assert_eq!(
        buffer[(
            right_edge,
            composer.y.saturating_add(composer.height.saturating_sub(1)),
        )]
            .bg,
        expected_surface
    );
}

#[cfg(test)]
pub(crate) fn exact_test_live_control_dock_collapses_disclosure_before_status() {
    use ratatui::{backend::TestBackend, Terminal};

    let mut app = AppState::new_live(None, false, None);
    let mut events = crate::lib_tests::session_view_events();
    events.pop();
    for event in events {
        app.ingest_event(event);
    }
    let width = 60;
    let height = 18;

    let backend = TestBackend::new(width, height);
    let mut terminal = Terminal::new(backend).unwrap_or_abort();
    terminal
        .draw(|frame| super::render_app(frame, &app))
        .unwrap_or_abort();

    let rendered = terminal
        .backend()
        .buffer()
        .content
        .chunks(width as usize)
        .map(|row| row.iter().map(|cell| cell.symbol()).collect::<String>())
        .collect::<Vec<_>>()
        .join("\n");
    assert!(!rendered.contains("↑/↓ history"));
    assert!(!rendered.contains("q quit"));
}

#[cfg(test)]
pub(crate) fn exact_test_tool_status_summary_uses_effective_tool_identity() {
    let mut app = AppState::new_live(None, false, None);
    app.activities.push_front(ActivityEntry {
        request_id: "req_tool_identity".to_string(),
        profile_label: "build".to_string(),
        model_id: "gpt-5.4".to_string(),
        provider_id: "default".to_string(),
        status: ActivityStatus::Streaming,
        user_message: None,
        user_timestamp: None,
        request_data: None,
        thinking_text: String::new(),
        thinking_first_mono_ms: None,
        thinking_last_mono_ms: None,
        transcript_text: String::new(),
        first_delta_mono_ms: None,
        usage: None,
        cache_usage: None,
        error_message: None,
        permissions: Vec::new(),
        tool_calls: vec![crate::app::ToolCallEntry {
            tool_call_id: "tc_wrapper".to_string(),
            tool_id: "mcp.fixture.tool.call".to_string(),
            canonical_tool_id: None,
            alias_source_tool_id: None,
            resolved_tool_identity: Some(harness_core::event::ResolvedToolIdentity {
                invoked_tool_id: Some("mcp.fixture.tool.call".to_string()),
                effective_tool_id: Some("mcp.fixture.echo".to_string()),
                canonical_tool_id: None,
                alias_source_tool_id: None,
            }),
            args_summary: r#"{"tool":"echo"}"#.to_string(),
            args_digest: "digest-wrapper".to_string(),
            lifecycle_state: Some(harness_core::event::ToolCallLifecycleState::Running),
            status: ToolCallDisplayStatus::Running,
            output_summary: None,
            output_digest: None,
            output_json: None,
            truncated_output: None,
            edit: None,
            lineage: None,
            artifact_refs: Vec::new(),
            timing_elapsed_ms: None,
            permissions: Vec::new(),
            first_seq: 1,
            last_seq: 1,
            first_mono_ms: 1,
            last_mono_ms: 1,
            first_timestamp: None,
            last_timestamp: None,
        }],
        first_seq: 1,
        last_seq: 1,
        first_mono_ms: 1,
        last_mono_ms: 1,
        request_started_mono_ms: None,
        revision: 0,
    });

    let summary = control_dock_summary_segment(&app).unwrap_or_abort();
    assert_eq!(
        summary.kind,
        crate::view_model::ControlDockSummarySegmentKind::Tool
    );
    assert_eq!(
        summary.tone,
        crate::view_model::ControlDockSummaryTone::Accent
    );
    assert_eq!(summary.text, "tool mcp.fixture.echo running");
    assert!(!summary.text.contains("mcp.fixture.tool.call"));
}

#[cfg(test)]
pub(crate) fn exact_test_retry_summary_segment_prioritizes_retry_indicator() {
    let mut app = AppState::new_live(None, false, None);
    app.activities.push_front(ActivityEntry {
        request_id: "req_retry".to_string(),
        profile_label: "build".to_string(),
        model_id: "gpt-5.4".to_string(),
        provider_id: "default".to_string(),
        status: ActivityStatus::Streaming,
        user_message: None,
        user_timestamp: None,
        request_data: Some(harness_core::event::ProviderRequestStartedEvent {
            request_id: "req_retry".into(),
            provider_id: "default".to_string(),
            model_id: "gpt-5.4".to_string(),
            prompt_summary: "prompt summary".to_string(),
            request_digest: "digest-retry".to_string(),
            metadata: Some(harness_core::event::ProviderRequestStartedMetadata {
                retry: Some(harness_core::event::ProviderRequestRetryMetadata {
                    attempt: 2,
                    max_attempts: 5,
                    delay_ms: None,
                    category: None,
                }),
                ..harness_core::event::ProviderRequestStartedMetadata::default()
            }),
        }),
        thinking_text: String::new(),
        thinking_first_mono_ms: None,
        thinking_last_mono_ms: None,
        transcript_text: String::new(),
        first_delta_mono_ms: None,
        usage: None,
        cache_usage: None,
        error_message: None,
        permissions: Vec::new(),
        tool_calls: Vec::new(),
        first_seq: 1,
        last_seq: 1,
        first_mono_ms: 1,
        last_mono_ms: 1,
        request_started_mono_ms: None,
        revision: 0,
    });

    let summary = control_dock_summary_segment(&app).unwrap_or_abort();
    assert_eq!(
        summary.kind,
        crate::view_model::ControlDockSummarySegmentKind::Retry
    );
    assert_eq!(summary.text, "retry 2/5");
    assert_eq!(
        summary.tone,
        crate::view_model::ControlDockSummaryTone::Warning
    );
}

#[cfg(test)]
pub(crate) fn exact_test_live_composer_reserves_right_gap() {
    let mut app = AppState::new_live(None, false, None);
    let mut events = crate::lib_tests::session_view_events();
    events.pop();
    for event in events {
        app.ingest_event(event);
    }

    let plan = FrameLayoutPlan::for_app(&app, Rect::new(0, 0, 160, 30));
    let dock = plan.dock.unwrap_or_abort();

    assert!(plan.operator_sidebar.is_none());
    assert_eq!(dock.composer.x, dock.shell.x);
    assert_eq!(dock.composer.width, dock.shell.width);
    assert_eq!(dock.disclosure.map(|area| area.x), Some(dock.composer.x));
    assert_eq!(
        dock.disclosure.map(|area| area.width),
        Some(dock.composer.width)
    );
}

#[cfg(test)]
pub(crate) fn exact_test_live_composer_disclosure_summarizes_compaction_metrics() {
    let mut app = AppState::new_live(None, false, None);
    app.active_context_usage = Some(crate::app::ActiveContextUsage::estimate(500));
    app.ingest_event(harness_core::event::EventEnvelopeV1 {
        schema_version: harness_core::event::SCHEMA_VERSION,
        event_id: "evt_compaction_applied".to_string(),
        seq: 1,
        run_id: "run_fixture".into(),
        mono_ms: 10,
        ts: None,
        actor: harness_core::event::EventActor::new(
            harness_core::event::ActorKind::System,
            Some("coordinator".to_string()),
        ),
        correlation_id: None,
        causation_id: None,
        stream_key: Some("run:run_fixture".to_string()),
        payload: harness_core::event::EventV1::CompactionApplied(
            harness_core::event::CompactionAppliedEvent {
                checkpoint_id: "checkpoint_000001".to_string(),
                agent_id: "agent_000001".to_string(),
                through_seq: 1,
                through_request_id: Some("req_000001".to_string()),
                tokens_before_estimate: Some(500),
                tokens_after_estimate: Some(120),
                summary_tokens_estimate: Some(80),
                compacted_turns: Some(1),
                preserved_turns: Some(1),
                reduction_tokens_estimate: Some(380),
                reduction_percent_estimate: Some(76),
                estimate_source: None,
            },
        ),
    });

    let surface = control_dock_surface(app.theme(), crate::view_model::ControlDockVariant::Live);
    let candidates = composer_context_summary_candidates(&app, app.theme(), surface)
        .into_iter()
        .map(|spans| {
            spans
                .into_iter()
                .map(|span| span.content.into_owned())
                .collect::<String>()
        })
        .collect::<Vec<_>>();

    assert!(candidates
        .iter()
        .any(|candidate| candidate.contains("live ctx 120 est")));
    assert!(candidates
        .iter()
        .any(|candidate| candidate.contains("compactions 1")
            && candidate.contains("summary 80 tok")
            && candidate.contains("saved 380 tok")));
}

#[cfg(test)]
pub(crate) fn exact_test_live_composer_disclosure_none_context_shows_est_zero() {
    let app = AppState::new_live(None, false, None);

    let surface = control_dock_surface(app.theme(), crate::view_model::ControlDockVariant::Live);
    let candidates = composer_context_summary_candidates(&app, app.theme(), surface)
        .into_iter()
        .map(|spans| {
            spans
                .into_iter()
                .map(|span| span.content.into_owned())
                .collect::<String>()
        })
        .collect::<Vec<_>>();

    assert!(candidates
        .iter()
        .any(|candidate| candidate.contains("live ctx 0 est")));
}

#[cfg(test)]
pub(crate) fn exact_test_live_composer_disclosure_none_context_shows_percent_when_limit_known() {
    let mut app = AppState::new_live(None, false, None);
    let option = crate::app::ModelOption {
        profile: "build".to_string(),
        provider: "test".to_string(),
        provider_display_label: None,
        provider_backend_label: None,
        model: "test-model".to_string(),
        model_display_label: None,
        variant: None,
        variant_display_label: None,
        display_label: None,
        token_window_label: None,
        context_window_tokens: Some(262144),
        max_input_tokens: None,
        max_output_tokens: None,
        description: None,
        profile_description: None,
        reasoning_effort: None,
        text_verbosity: None,
        thinking: None,
        recommended_for: None,
    };
    app.set_launch_metadata(crate::app::LaunchMetadata::from_model_option(&option));

    let surface = control_dock_surface(app.theme(), crate::view_model::ControlDockVariant::Live);
    let candidates = composer_context_summary_candidates(&app, app.theme(), surface)
        .into_iter()
        .map(|spans| {
            spans
                .into_iter()
                .map(|span| span.content.into_owned())
                .collect::<String>()
        })
        .collect::<Vec<_>>();

    assert!(
        candidates
            .iter()
            .any(|candidate| candidate.contains("live ctx 0 est") && candidate.contains("(0%)")),
        "context indicator should always show percentage when limit is known, even without usage data: {candidates:?}"
    );
}

#[cfg(test)]
pub(crate) fn exact_test_live_composer_metadata_omits_success_without_variant() {
    let mut app = AppState::new_live(None, false, None);
    app.set_launch_metadata(crate::app::LaunchMetadata::from_model_ref(
        "deep",
        "default:gpt-5.4-mini",
    ));
    let dock = crate::view_model::ControlDockViewModel {
        variant: crate::view_model::ControlDockVariant::Live,
        runtime_context: None,
        runtime_badge: "Success".to_string(),
        runtime_kind: RuntimeStateKind::Success,
        primary_summary: "turn 1 · ready for next turn".to_string(),
        summary_segment: None,
        composer_body: String::new(),
        composer_disclosure: String::new(),
        composer_focused: true,
        composer_disabled: false,
    };

    let rendered = composer_metadata_candidates(&app, &dock)
        .into_iter()
        .map(|segments| {
            segments
                .into_iter()
                .map(|(text, _)| text)
                .collect::<String>()
        })
        .collect::<Vec<_>>()
        .join("\n");

    assert!(!rendered.contains("success"));
}

#[cfg(test)]
pub(crate) fn exact_test_startup_disclosure_matches_harness_hint_row() {
    let app = AppState::new_startup(Vec::new(), None);
    let theme = app.theme();
    let surface = control_dock_surface(theme, crate::view_model::ControlDockVariant::Startup);
    let candidate = startup_disclosure_candidates(theme, surface, "Ctrl+p", "Shift+Enter/Ctrl+j")
        .into_iter()
        .next()
        .unwrap_or_abort()
        .into_iter()
        .map(|span| span.content.into_owned())
        .collect::<String>();

    assert_eq!(candidate, "ctrl+t variants  tab agents  ctrl+p commands");
}

#[cfg(test)]
pub(crate) fn exact_test_composer_viewport_wraps_by_display_width() {
    let viewport = composer_viewport("ab界c", 4, 6, Some(3));

    assert_eq!(viewport.lines, vec!["ab界".to_string(), "c".to_string()]);
    assert_eq!(viewport.cursor, Some((1, 0)));
}

#[cfg(test)]
pub(crate) fn exact_test_composer_viewport_wraps_at_word_boundaries() {
    let viewport = composer_viewport("alpha beta", 8, 6, Some(6));

    assert_eq!(
        viewport.lines,
        vec!["alpha ".to_string(), "beta".to_string()]
    );
    assert_eq!(viewport.cursor, Some((1, 0)));
}

#[cfg(test)]
#[test]
fn composer_file_tag_line_uses_warning_bold_style() {
    // arrange
    // act
    // assert
    let app = AppState::new_startup(Vec::new(), None);
    let theme = app.theme();
    let base = Style::default()
        .fg(theme.text.primary)
        .bg(theme.surface.panel_elevated);
    let tag = Style::default()
        .fg(theme.status.warning)
        .bg(theme.surface.panel_elevated)
        .add_modifier(Modifier::BOLD);

    let line = composer_line_with_file_tags(
        "use @docs/ now",
        0,
        &[crate::app::FileMentionTag {
            start: 4,
            end: 10,
            part: crate::app::FileMentionSelectedTag::File(
                harness_core::file_tag::SelectedFileTag {
                    path: "docs/".to_string(),
                    filename: "docs/".to_string(),
                    url: "file:///workspace/docs/".to_string(),
                    mime: "application/x-directory".to_string(),
                    source: harness_core::file_tag::FileTagSource {
                        start: 4,
                        end: 10,
                        value: "@docs/".to_string(),
                    },
                    line_range: None,
                },
            ),
        }],
        base,
        tag,
    );

    assert_eq!(line.spans.len(), 3);
    assert_eq!(line.spans[0].content.as_ref(), "use ");
    assert_eq!(line.spans[0].style, base);
    assert_eq!(line.spans[1].content.as_ref(), "@docs/");
    assert_eq!(line.spans[1].style, tag);
    assert_eq!(line.spans[2].content.as_ref(), " now");
    assert_eq!(line.spans[2].style, base);
}

#[cfg(test)]
pub(crate) fn exact_test_footer_status_cluster_shows_pending_permission_count() {
    let mut app = AppState::new_live(None, false, None);
    app.ingest_event(harness_core::event::EventEnvelopeV1 {
        schema_version: harness_core::event::SCHEMA_VERSION,
        event_id: "evt_footer_cluster".to_string(),
        seq: 1,
        run_id: "run_footer_cluster".into(),
        mono_ms: 0,
        ts: None,
        actor: harness_core::event::EventActor::new(
            harness_core::event::ActorKind::Supervisor,
            None,
        ),
        correlation_id: Some("tool_call_footer_cluster".to_string()),
        causation_id: None,
        stream_key: Some("tool_call_footer_cluster".to_string()),
        payload: harness_core::event::EventV1::PermissionRequested(
            harness_core::event::PermissionRequestedEvent {
                permission_id: "perm_footer_cluster".to_string(),
                kind: "edit_fs".to_string(),
                tool_call_id: Some("tool_call_footer_cluster".into()),
                summary: "Apply edit to demo.txt".to_string(),
                request_digest: "digest-footer".to_string(),
                timeout_ms: 30_000,
                default_decision: harness_core::event::PermissionDecision::Deny,
            },
        ),
    });

    let data = crate::ui::ui_secondary::footer_status_cluster_data(&app);
    assert_eq!(data.pending_permissions, 1);
}

#[cfg(test)]
pub(crate) fn exact_test_footer_status_cluster_empty_when_no_activity() {
    let app = AppState::new_live(None, false, None);

    let data = crate::ui::ui_secondary::footer_status_cluster_data(&app);
    assert_eq!(data.pending_permissions, 0);
}

#[test]
fn hidden_header_identity_omits_primary_profile() {
    // Given live sessions launched with either primary profile.
    let identities = ["build", "plan"].map(|profile| {
        let mut app = AppState::new_live(None, false, None);
        app.set_launch_metadata(crate::app::LaunchMetadata::new(
            profile,
            "default",
            Some("model-alpha".to_string()),
        ));

        // When the compact header identity is composed.
        header_identity_text(&app, SessionHeaderMode::Hidden)
    });

    // Then provider and model remain visible without the primary profile.
    assert_eq!(
        identities,
        [
            "run unknown · default/model-alpha".to_string(),
            "run unknown · default/model-alpha".to_string(),
        ]
    );
}
