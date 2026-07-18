//! Structural owners for SHELL-PERM / OVL-PERM / SHELL-QUESTION / OVL-QUESTION.
//!
//! Contract: `docs/grok-build-tui-implementation-prompt.md` +
//! `crates/harness-tui/DESIGN.md` §3 / §8.
//!
//! Permission/question freezes are TBD. These tests lock DESIGN-observable density:
//! full-width shell, draft preserved, radio choice chrome, no legacy rail, and
//! question must not render edit-permission allow chrome.

#![allow(
    clippy::expect_used,
    clippy::panic,
    clippy::unwrap_used,
    reason = "integration parity tests use fail-fast asserts"
)]

use harness_core::event::{
    ActorKind, EventActor, EventEnvelopeV1, EventV1, PermissionRequestedEvent, SCHEMA_VERSION,
};
use harness_tui::app::{AppState, Focus, LaunchMetadata};
use harness_tui::render_test::render_to_string;
use harness_tui::{ui, FrameLayoutPlan};
use ratatui::layout::Rect;

const W: u16 = 120;
const H: u16 = 40;

fn envelope(seq: u64, correlation_id: Option<&str>, payload: EventV1) -> EventEnvelopeV1 {
    EventEnvelopeV1 {
        schema_version: SCHEMA_VERSION,
        event_id: format!("evt-perm-q-{seq:04}"),
        seq,
        run_id: "run_perm_question_parity".into(),
        mono_ms: seq,
        ts: None,
        actor: EventActor::new(ActorKind::System, Some("perm-question-parity".to_string())),
        correlation_id: correlation_id.map(str::to_string),
        causation_id: None,
        stream_key: Some("run:run_perm_question_parity".to_string()),
        payload,
    }
}

fn live_app() -> AppState {
    let mut app = AppState::new_live(None, false, None);
    app.set_launch_metadata(
        LaunchMetadata::from_model_ref("build", "mock:model-perm").with_mode_label("Demo"),
    );
    app
}

fn render(app: &AppState) -> String {
    render_to_string(app, Rect::new(0, 0, W, H), |app, frame, _area| {
        ui::render_app(frame, app)
    })
}

fn plan_for(app: &AppState) -> FrameLayoutPlan {
    FrameLayoutPlan::for_app(app, Rect::new(0, 0, W, H))
}

fn permission_requested_event(
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

fn question_permission_requested_event(
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

/// SHELL-PERM / OVL-PERM: permission dock preempts composer, preserves draft, full-width shell.
#[test]
fn shell_perm_preempts_composer_preserves_draft_full_width() {
    // arrange
    let mut app = live_app();
    let draft = "keep this draft under permission";
    app.composer.prompt_buffer = draft.to_string();
    app.composer.prompt_cursor = draft.chars().count();
    app.focus = Focus::Prompt;

    // act
    app.ingest_event(permission_requested_event(
        1,
        "perm_shell_parity",
        "tool_call_shell_parity",
    ));
    let plan = plan_for(&app);
    let rendered = render(&app);

    // assert
    assert!(
        plan.operator_sidebar.is_none(),
        "SHELL-PERM: full-width shell (no operator sidebar)"
    );
    if let (Some(transcript), Some(composer)) = (plan.transcript, plan.composer) {
        assert_eq!(
            transcript.width, plan.shell.width,
            "SHELL-PERM: transcript full width under permission"
        );
        let composer_inset: u16 = if plan.shell.width <= 60 { 0 } else { 2 };
        assert_eq!(
            composer.x,
            plan.shell.x.saturating_add(composer_inset),
            "SHELL-PERM: composer freeze horizontal inset under permission"
        );
        assert_eq!(
            composer.width,
            plan.shell
                .width
                .saturating_sub(composer_inset.saturating_mul(2)),
            "SHELL-PERM: composer freeze-matched width under permission"
        );
    }
    assert!(
        rendered.contains("Allow Edit") || rendered.contains("always-approve"),
        "OVL-PERM: permission dock must render\n{rendered}"
    );
    assert!(
        rendered.contains("always-approve"),
        "OVL-PERM: permission options must include always-approve\n{rendered}"
    );
    assert!(
        rendered.contains('●') || rendered.contains("(●)"),
        "OVL-PERM: selected option uses filled radio marker\n{rendered}"
    );
    assert!(
        rendered.contains('┃'),
        "OVL-PERM: permission dock must paint freeze-matched ┃ rail\n{rendered}"
    );
    assert_eq!(
        app.composer.prompt_buffer, draft,
        "SHELL-PERM: draft preserved under permission dock"
    );
    assert!(
        app.active_permission().is_some() || app.active_permission_view().is_some(),
        "SHELL-PERM: active permission required"
    );
}

/// SHELL-QUESTION / OVL-QUESTION: question dock parses prompts, preserves draft, no allow chrome.
#[test]
fn shell_question_parses_prompts_preserves_draft_no_allow_chrome() {
    // arrange
    let mut app = live_app();
    let draft = "keep draft under question";
    app.composer.prompt_buffer = draft.to_string();
    app.composer.prompt_cursor = draft.chars().count();
    app.focus = Focus::Prompt;

    // act
    app.ingest_event(question_permission_requested_event(
        1,
        "perm_question_parity",
        "tool_call_question_parity",
    ));
    let view = app
        .active_permission_view()
        .expect("SHELL-QUESTION: active permission view required");
    let plan = plan_for(&app);
    let rendered = render(&app);

    // assert
    assert_eq!(view.kind, "question");
    let prompts = view
        .question_prompts
        .as_ref()
        .expect("OVL-QUESTION: question_prompts must parse from summary JSON");
    assert_eq!(prompts.len(), 1);
    assert_eq!(prompts[0].question, "Pick one");
    assert_eq!(prompts[0].header, "Choice");
    assert_eq!(prompts[0].options[0].label, "A");
    assert_eq!(
        app.composer.prompt_buffer, draft,
        "SHELL-QUESTION: draft preserved under question dock"
    );
    assert!(
        rendered.contains("Pick one")
            || rendered.contains("Choice")
            || rendered.contains('●')
            || rendered.contains('○'),
        "OVL-QUESTION: question dock must render\n{rendered}"
    );
    assert!(
        !rendered.contains("always-approve"),
        "OVL-QUESTION: must not render edit-permission allow chrome\n{rendered}"
    );
    assert!(
        plan.operator_sidebar.is_none(),
        "SHELL-QUESTION: full-width shell (no operator sidebar)"
    );
    assert!(
        rendered.contains('┃'),
        "OVL-QUESTION: question dock must paint ┃ rail matching Grok packing\n{rendered}"
    );
}
