use super::super::*;

#[cfg(test)]
pub(crate) fn exact_test_transcript_follow_mode_uses_measured_surface_heights() {
    let mut app = AppState::default();
    app.activities = std::collections::VecDeque::from(vec![
        transcript_section_model_test_activity(
            "request-long",
            ActivityStatus::Done,
        "this reply is intentionally long enough to wrap across several measured surface rows and keep wrapping even after the harness footer gap is accounted for in measured layout",
        ),
        transcript_section_model_test_activity(
            "request-short",
            ActivityStatus::Done,
            "short reply",
        ),
    ]);
    app.transcript_view.selected_activity_index = 1;
    app.transcript_view.follow_mode = true;

    let width = 28;
    let viewport_height = 6;
    let layout = build_measured_transcript_layout_for_width(&app, &Theme::default(), width);

    assert_eq!(layout.sections.len(), 2);
    assert!(
        layout.sections[0].content_height >= layout.sections[0].lines.len(),
        "measured transcript height should never undercount the rendered line inventory"
    );
    assert!(
        layout.sections[0].content_height > layout.sections[1].content_height,
        "the longer wrapped section should still measure taller than the short reply"
    );

    let measured_total_height = layout
        .sections
        .iter()
        .map(MeasuredTranscriptSection::total_height)
        .sum::<usize>();
    assert_eq!(layout.total_height, measured_total_height);

    let scroll = transcript_scroll_offset(
        app.transcript_view.follow_mode,
        app.transcript_view.transcript_scroll,
        layout.total_height,
        viewport_height,
    );
    let expected = layout
        .total_height
        .saturating_sub(usize::from(viewport_height));

    assert_eq!(scroll, expected);
    assert!(
        scroll > 1,
        "follow mode should scroll by measured surface height, not just section count"
    );
}

#[cfg(test)]
pub(crate) fn exact_test_transcript_scroll_offset_preserves_large_overflow() {
    let mut app = AppState::default();
    app.transcript_view.follow_mode = false;
    app.transcript_view.transcript_scroll = 17;

    let layout = MeasuredTranscriptLayout {
        sections: Vec::new(),
        total_height: usize::from(u16::MAX) + 512,
    };

    let scroll = transcript_scroll_offset(
        app.transcript_view.follow_mode,
        app.transcript_view.transcript_scroll,
        layout.total_height,
        12,
    );
    let expected = layout
        .total_height
        .saturating_sub(12)
        .saturating_sub(app.transcript_view.transcript_scroll);

    assert_eq!(scroll, expected);
    assert!(
        scroll > usize::from(u16::MAX),
        "large transcript offsets should not truncate to u16"
    );
}

#[cfg(test)]
pub(crate) fn exact_test_visible_surface_lines_support_large_offsets() {
    let surface = MeasuredTranscriptSurface {
        kind: TranscriptRenderSurfaceKind::AssistantBody,
        top_offset: 0,
        height: usize::from(u16::MAX) + 1024,
        width: 24,
        show_outer_rail: false,
        rail_glyph: TRANSCRIPT_RAIL_GLYPH,
        rail_color: Color::Reset,
        surface: Color::Reset,
        lines: (0..(usize::from(u16::MAX) + 1024))
            .map(|index| Line::from(format!("line {index}")))
            .collect(),
        interaction_rows: None,
        selection_rows: None,
        diff_hunk_offsets: Vec::new(),
        selected_rail: false,
        tool_rail_motion: None,
    };

    let visible = visible_surface_lines(&surface, usize::from(u16::MAX) + 7, 3)
        .into_iter()
        .map(|line| {
            line.spans
                .into_iter()
                .map(|span| span.content.into_owned())
                .collect::<String>()
        })
        .collect::<Vec<_>>();

    assert_eq!(
        visible,
        vec![
            format!("line {}", usize::from(u16::MAX) + 7),
            format!("line {}", usize::from(u16::MAX) + 8),
            format!("line {}", usize::from(u16::MAX) + 9),
        ]
    );
}

#[cfg(test)]
pub(crate) fn exact_test_transcript_pending_permission_stays_after_last_activity() {
    let mut app = AppState::default();
    app.activities =
        std::collections::VecDeque::from(vec![transcript_section_model_test_activity(
            "request-a",
            ActivityStatus::Done,
            "assistant reply",
        )]);
    app.ingest_event(harness_core::event::EventEnvelopeV1 {
        schema_version: harness_core::event::SCHEMA_VERSION,
        event_id: "evt_pending_permission_order".to_string(),
        seq: 1,
        run_id: "run_pending_permission_order".into(),
        mono_ms: 0,
        ts: None,
        actor: harness_core::event::EventActor::new(
            harness_core::event::ActorKind::Supervisor,
            None,
        ),
        correlation_id: Some("tool_call_pending_permission_order".to_string()),
        causation_id: None,
        stream_key: Some("tool_call_pending_permission_order".to_string()),
        payload: harness_core::event::EventV1::PermissionRequested(
            harness_core::event::PermissionRequestedEvent {
                permission_id: "perm_pending_permission_order".to_string(),
                kind: "edit_fs".to_string(),
                tool_call_id: Some("tool_call_pending_permission_order".into()),
                summary: "Apply hashline edit to demo.txt".to_string(),
                request_digest: "digest-perm-order".to_string(),
                timeout_ms: 30_000,
                default_decision: harness_core::event::PermissionDecision::Deny,
            },
        ),
    });

    let layout = build_measured_transcript_layout_for_width(&app, &Theme::default(), 80);

    assert_eq!(layout.sections.len(), 1);
}
