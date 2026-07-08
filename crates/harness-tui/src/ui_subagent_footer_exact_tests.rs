use super::*;
use crate::UnwrapOrAbort;

pub(super) fn footer_event(
    seq: u64,
    actor: harness_core::event::EventActor,
    correlation_id: Option<&str>,
    payload: harness_core::event::EventV1,
) -> harness_core::event::EventEnvelopeV1 {
    harness_core::event::EventEnvelopeV1 {
        schema_version: harness_core::event::SCHEMA_VERSION,
        event_id: format!("evt-subagent-footer-exact-{seq:04}"),
        seq,
        run_id: "agent_child".into(),
        mono_ms: seq * 100,
        ts: None,
        actor,
        correlation_id: correlation_id.map(str::to_string),
        causation_id: None,
        stream_key: Some("run:agent_child".to_string()),
        payload,
    }
}

fn subagent_info() -> crate::app::SubagentSessionInfo {
    crate::app::SubagentSessionInfo {
        label: "Researcher".to_string(),
        title: "audit transcript parity".to_string(),
        parent_label: "parent_run".to_string(),
        index: 1,
        total: 1,
        usage: None,
    }
}

pub(super) fn render_footer_rows(app: &AppState) -> Vec<String> {
    use ratatui::{backend::TestBackend, Terminal};

    let theme = Theme::default();
    let info = subagent_info();
    let backend = TestBackend::new(100, 6);
    let mut terminal = Terminal::new(backend).unwrap_or_abort();
    terminal
        .draw(|frame| {
            render_subagent_footer(
                frame,
                app,
                Rect::new(0, 1, 100, 3),
                Rect::new(0, 1, 100, 3),
                &theme,
                &info,
            );
        })
        .unwrap_or_abort();

    terminal
        .backend()
        .buffer()
        .content
        .chunks(100)
        .map(|row| row.iter().map(|cell| cell.symbol()).collect::<String>())
        .collect()
}

#[cfg(test)]
pub(crate) fn exact_test_subagent_footer_body_keeps_ordered_transcript_tool_rows() {
    let worker = harness_core::event::EventActor::new(
        harness_core::event::ActorKind::Worker,
        Some("agent_child".to_string()),
    );
    let system = harness_core::event::EventActor::new(
        harness_core::event::ActorKind::System,
        Some("coordinator".to_string()),
    );
    let app = AppState::new_replay(
        std::path::PathBuf::from("/tmp/harness-subagent-footer/agent_child"),
        vec![
            footer_event(
                1,
                worker.clone(),
                Some("req_child"),
                harness_core::event::EventV1::ProviderRequestStarted(
                    harness_core::event::ProviderRequestStartedEvent {
                        request_id: "req_child".into(),
                        provider_id: "mock".to_string(),
                        model_id: "model-1".to_string(),
                        prompt_summary: "audit transcript parity".to_string(),
                        request_digest: "digest-child-start".to_string(),
                        metadata: None,
                    },
                ),
            ),
            footer_event(
                2,
                worker.clone(),
                Some("req_child"),
                harness_core::event::EventV1::ProviderStreamDelta(
                    harness_core::event::ProviderStreamDeltaEvent {
                        request_id: "req_child".into(),
                        delta: "First **markdown** answer.\n".to_string(),
                    },
                ),
            ),
            footer_event(
                3,
                system.clone(),
                Some("req_child"),
                harness_core::event::EventV1::ToolCallRequested(
                    harness_core::event::ToolCallRequestedEvent {
                        tool_call_id: "tc_child_read".into(),
                        tool_id: "fs.read".to_string(),
                        args_summary: r#"{"path":"docs/rust.md"}"#.to_string(),
                        args_digest: "digest-child-read".to_string(),
                        metadata: None,
                    },
                ),
            ),
            footer_event(
                4,
                system,
                Some("req_child"),
                harness_core::event::EventV1::ToolCallStarted(
                    harness_core::event::ToolCallStartedEvent {
                        tool_call_id: "tc_child_read".into(),
                    },
                ),
            ),
            footer_event(
                5,
                worker,
                Some("req_child"),
                harness_core::event::EventV1::ProviderStreamDelta(
                    harness_core::event::ProviderStreamDeltaEvent {
                        request_id: "req_child".into(),
                        delta: "Second body after tool.".to_string(),
                    },
                ),
            ),
        ],
    );

    let rows = render_footer_rows(&app);
    let rendered = rows.join("\n");
    assert!(
        rendered.contains("Researcher (1 of 1)"),
        "subagent footer should show the Opencode label/count row\n{rendered}"
    );
    assert!(
        rendered.contains("Parent ↑") && rendered.contains("Prev ←") && rendered.contains("Next →"),
        "subagent footer should show Opencode navigation controls\n{rendered}"
    );
    assert!(
        !rendered.contains("First markdown answer.")
            && !rendered.contains("Read docs/rust.md")
            && !rendered.contains("Second body after tool."),
        "subagent footer should not embed a Harness transcript inspector body\n{rendered}"
    );
}

#[cfg(test)]
pub(crate) fn exact_test_subagent_footer_status_uses_running_and_cancelled_icons() {
    let worker = harness_core::event::EventActor::new(
        harness_core::event::ActorKind::Worker,
        Some("agent_child".to_string()),
    );
    let running_event = footer_event(
        1,
        worker.clone(),
        Some("req_child"),
        harness_core::event::EventV1::TaskScheduled(harness_core::event::TaskScheduledEvent {
            task_id: "task_child".to_string().into(),
            state: harness_core::event::TaskScheduleState::Started,
            queue_key: Some("agent:running:researcher".to_string()),
        }),
    );
    let running_app = AppState::new_replay(
        std::path::PathBuf::from("/tmp/harness-subagent-footer/agent_child"),
        vec![running_event.clone()],
    );
    let running_rows = render_footer_rows(&running_app).join("\n");
    assert!(
        running_rows.contains("Researcher (1 of 1)"),
        "{running_rows}"
    );
    assert!(
        !running_rows.contains("⠋") && !running_rows.contains("audit transcript parity"),
        "Opencode subagent footer should omit the old Harness status/title row\n{running_rows}"
    );

    let cancelled_app = AppState::new_replay(
        std::path::PathBuf::from("/tmp/harness-subagent-footer/agent_child"),
        vec![
            running_event,
            footer_event(
                2,
                worker,
                Some("req_child"),
                harness_core::event::EventV1::TaskCancelled(
                    harness_core::event::TaskCancelledEvent {
                        task_id: "task_child".to_string().into(),
                        reason: "operator cancelled".to_string(),
                        task_scope: Some(harness_core::event::TaskTerminalScope::ToolCall),
                    },
                ),
            ),
        ],
    );
    let cancelled_rows = render_footer_rows(&cancelled_app).join("\n");
    assert!(
        cancelled_rows.contains("Researcher (1 of 1)"),
        "{cancelled_rows}"
    );
    assert!(
        !cancelled_rows.contains("○") && !cancelled_rows.contains("audit transcript parity"),
        "Opencode subagent footer should omit old Harness cancelled/title row\n{cancelled_rows}"
    );
}
