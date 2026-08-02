//! Shared fixtures for independent P0 parity contract tests.
#![allow(
    clippy::expect_used,
    clippy::panic,
    clippy::unwrap_used,
    reason = "integration contract tests use fail-fast asserts for missing layout/render state"
)]

use std::path::PathBuf;

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use harness_core::event::{
    ActorKind, EventActor, EventEnvelopeV1, EventV1, PermissionRequestedEvent,
    ProviderRequestStartedEvent, ProviderStreamDeltaEvent, RunStartedEvent,
    UserMessageSubmittedEvent, SCHEMA_VERSION,
};
use harness_core::proj::{RunStatus, SessionCatalogEntry, SessionModeSource};
use harness_tui::app::{AppState, SessionHistoryEntry};
use harness_tui::render_test::render_to_string;
use harness_tui::{ui, FrameLayoutPlan};
use ratatui::layout::Rect;

pub const CANONICAL_VIEWPORTS: [(u16, u16); 6] =
    [(120, 40), (100, 30), (80, 24), (79, 24), (80, 23), (60, 20)];

pub fn live_session_app() -> AppState {
    let mut app = AppState::new_live(
        Some(PathBuf::from("/tmp/run_p0_parity_contract")),
        false,
        None,
    );
    for event in live_session_events() {
        app.ingest_event(event);
    }
    app
}

pub fn plan_for(app: &AppState, width: u16, height: u16) -> FrameLayoutPlan {
    FrameLayoutPlan::for_app(app, Rect::new(0, 0, width, height))
}

pub fn render_text(app: &AppState, width: u16, height: u16) -> String {
    render_to_string(app, Rect::new(0, 0, width, height), |app, frame, _area| {
        ui::render_app(frame, app)
    })
}

pub fn key(code: KeyCode) -> KeyEvent {
    KeyEvent::new(code, KeyModifiers::NONE)
}

pub fn key_with_modifiers(code: KeyCode, modifiers: KeyModifiers) -> KeyEvent {
    KeyEvent::new(code, modifiers)
}

pub fn permission_requested_event(
    seq: u64,
    permission_id: &str,
    tool_call_id: &str,
) -> EventEnvelopeV1 {
    envelope(
        seq,
        Some(tool_call_id),
        EventV1::PermissionRequested(PermissionRequestedEvent {
            permission_id: permission_id.to_string(),
            kind: "edit_fs".to_string(),
            tool_call_id: Some(tool_call_id.into()),
            summary: "Apply hashline edit to demo.txt".to_string(),
            request_digest: format!("digest-{permission_id}"),
            timeout_ms: 30_000,
            default_decision: harness_core::event::PermissionDecision::Deny,
        }),
    )
}

pub fn question_permission_requested_event(
    seq: u64,
    permission_id: &str,
    tool_call_id: &str,
) -> EventEnvelopeV1 {
    envelope(
        seq,
        Some(tool_call_id),
        EventV1::PermissionRequested(PermissionRequestedEvent {
            permission_id: permission_id.to_string(),
            kind: "question".to_string(),
            tool_call_id: Some(tool_call_id.into()),
            summary: serde_json::json!({
                "questions": [{
                    "question": "Pick one",
                    "header": "Choice",
                    "options": [{"label": "A", "description": "Option A"}],
                    "multiple": false,
                }]
            })
            .to_string(),
            request_digest: format!("digest-question-{permission_id}"),
            timeout_ms: 30_000,
            default_decision: harness_core::event::PermissionDecision::Deny,
        }),
    )
}

pub fn startup_session_history_entries() -> Vec<SessionHistoryEntry> {
    vec![SessionHistoryEntry {
        run_dir: PathBuf::from("/tmp/sessions/run_resume"),
        catalog: SessionCatalogEntry {
            run_id: "run_resume".into(),
            run_name: Some("alpha-run".to_string()),
            status: Some(RunStatus::Finished),
            last_updated_at: Some("2026-03-08T12:34:56Z".to_string()),
            workspace_root: Some("/tmp/workspace".to_string()),
            profile_preset: Some("deep".to_string()),
            provider_model: Some("openai/gpt-5.4-mini".to_string()),
            mode_source: SessionModeSource::InteractiveLive,
            is_resumable: true,
            resume_disabled_reason: None,
            artifact_count: 2,
            child_session_count: 1,
            parent_session_id: None,
        },
    }]
}

pub fn live_session_events() -> Vec<EventEnvelopeV1> {
    let request_id = "req_p0_geom";
    vec![
        envelope(
            1,
            Some(request_id),
            EventV1::RunStarted(RunStartedEvent {
                run_name: "p0-geom-contract".into(),
                workspace_root: "/tmp/workspace".to_string(),
            }),
        ),
        envelope(
            2,
            Some(request_id),
            EventV1::UserMessageSubmitted(UserMessageSubmittedEvent {
                request_id: request_id.into(),
                text: "Prove canonical viewport geometry".to_string(),
            }),
        ),
        envelope(
            3,
            Some(request_id),
            EventV1::ProviderRequestStarted(ProviderRequestStartedEvent {
                request_id: request_id.into(),
                provider_id: "mock".to_string(),
                model_id: "p0-model".to_string(),
                prompt_summary: "Prove canonical viewport geometry".to_string(),
                request_digest: "digest-p0-geom".to_string(),
                metadata: None,
            }),
        ),
        envelope(
            4,
            Some(request_id),
            EventV1::ProviderStreamDelta(ProviderStreamDeltaEvent {
                request_id: request_id.into(),
                delta: "Transcript content for P0 geometry checks.".to_string(),
            }),
        ),
    ]
}

pub fn envelope(seq: u64, correlation_id: Option<&str>, payload: EventV1) -> EventEnvelopeV1 {
    EventEnvelopeV1 {
        schema_version: SCHEMA_VERSION,
        event_id: format!("evt-p0-{seq:04}"),
        seq,
        run_id: "run_p0_parity_contract".into(),
        mono_ms: seq,
        ts: None,
        actor: EventActor::new(ActorKind::System, Some("p0-parity-contract".to_string())),
        correlation_id: correlation_id.map(str::to_string),
        causation_id: None,
        stream_key: Some("run:run_p0_parity_contract".to_string()),
        payload,
    }
}
