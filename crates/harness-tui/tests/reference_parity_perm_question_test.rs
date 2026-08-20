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

use std::sync::{Arc, Mutex};

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use harness_core::event::{
    ActorKind, EventActor, EventEnvelopeV1, EventV1, PermissionRequestedEvent, SCHEMA_VERSION,
};
use harness_core::perm::PermissionDecision;
use harness_tui::app::{AppState, Focus, LaunchMetadata, UiIntent};
use harness_tui::render_test::render_to_string;
use harness_tui::{ui, FrameLayoutPlan, UnwrapOrAbort};
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
    render_at(app, W, H)
}

fn render_at(app: &AppState, width: u16, height: u16) -> String {
    render_to_string(app, Rect::new(0, 0, width, height), |app, frame, _area| {
        ui::render_app(frame, app)
    })
}

fn plan_for(app: &AppState) -> FrameLayoutPlan {
    plan_at(app, W, H)
}

fn plan_at(app: &AppState, width: u16, height: u16) -> FrameLayoutPlan {
    FrameLayoutPlan::for_app(app, Rect::new(0, 0, width, height))
}

fn permission_requested_event(
    seq: u64,
    permission_id: &str,
    tool_call_id: &str,
) -> EventEnvelopeV1 {
    permission_requested_event_with_summary(
        seq,
        permission_id,
        tool_call_id,
        "Apply hashline edit to demo.txt",
    )
}

fn permission_requested_event_with_summary(
    seq: u64,
    permission_id: &str,
    tool_call_id: &str,
    summary: &str,
) -> EventEnvelopeV1 {
    envelope(
        seq,
        Some(tool_call_id),
        EventV1::PermissionRequested(PermissionRequestedEvent {
            permission_id: permission_id.to_string(),
            kind: "edit_fs".to_string(),
            tool_call_id: Some(tool_call_id.into()),
            summary: summary.to_string(),
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

/// Question dock with three selectable options (digit keys 1-3).
fn three_option_question_event(
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
                    "question": "Which color?",
                    "header": "Color",
                    "options": [
                        {"label": "A", "description": "Option A"},
                        {"label": "B", "description": "Option B"},
                        {"label": "C", "description": "Option C"},
                    ],
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

/// Question dock with two prompts (multi-tab: Tab/left/right switch prompts).
fn multi_question_event(seq: u64, permission_id: &str, tool_call_id: &str) -> EventEnvelopeV1 {
    envelope(
        seq,
        Some(tool_call_id),
        EventV1::PermissionRequested(PermissionRequestedEvent {
            permission_id: permission_id.to_string(),
            kind: "question".to_string(),
            tool_call_id: Some(tool_call_id.into()),
            summary: serde_json::json!({
                "questions": [
                    {
                        "question": "First parity question",
                        "header": "Q1",
                        "options": [{"label": "A1", "description": "First option"}],
                        "multiple": false,
                    },
                    {
                        "question": "Second parity question",
                        "header": "Q2",
                        "options": [{"label": "B1", "description": "Second option"}],
                        "multiple": false,
                    },
                ]
            })
            .to_string(),
            request_digest: format!("digest-question-{permission_id}"),
            timeout_ms: 30_000,
            default_decision: harness_core::event::PermissionDecision::Deny,
        }),
    )
}

fn question_event_with_summary(
    seq: u64,
    permission_id: &str,
    tool_call_id: &str,
    summary: serde_json::Value,
) -> EventEnvelopeV1 {
    envelope(
        seq,
        Some(tool_call_id),
        EventV1::PermissionRequested(PermissionRequestedEvent {
            permission_id: permission_id.to_string(),
            kind: "question".to_string(),
            tool_call_id: Some(tool_call_id.into()),
            summary: summary.to_string(),
            request_digest: format!("digest-question-{permission_id}"),
            timeout_ms: 30_000,
            default_decision: harness_core::event::PermissionDecision::Deny,
        }),
    )
}

fn question_live_app_with_sink() -> (AppState, Arc<Mutex<Vec<UiIntent>>>) {
    let intents = Arc::new(Mutex::new(Vec::<UiIntent>::new()));
    let sink_intents = Arc::clone(&intents);
    let sink: Arc<dyn Fn(UiIntent) + Send + Sync> =
        Arc::new(move |intent| sink_intents.lock().unwrap_or_abort().push(intent));
    let mut app = AppState::new_live(None, false, Some(sink));
    app.set_launch_metadata(
        LaunchMetadata::from_model_ref("build", "mock:model-tx").with_mode_label("Demo"),
    );
    (app, intents)
}

#[test]
fn permission_and_question_keep_pinned_composer_rects() {
    // arrange
    let mut permission = live_app();
    permission.ingest_event(permission_requested_event(
        1,
        "perm_geometry",
        "tool_perm_geometry",
    ));
    let mut question = live_app();
    question.ingest_event(question_permission_requested_event(
        1,
        "question_geometry",
        "tool_question_geometry",
    ));

    // act
    for (width, height, expected) in [
        (120, 40, Rect::new(2, 35, 116, 3)),
        (60, 20, Rect::new(1, 17, 58, 3)),
    ] {
        // assert
        assert_eq!(plan_at(&permission, width, height).composer, Some(expected));
        assert_eq!(plan_at(&question, width, height).composer, Some(expected));
    }
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
    let live_plan = plan_for(&app);

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
    assert_eq!(
        plan.composer.map(|area| area.y),
        live_plan.composer.map(|area| area.y.saturating_add(1)),
        "SHELL-PERM: suppressing idle keybinds moves the permission composer down one row"
    );
    assert_eq!(
        plan.composer.map(|area| (area.width, area.height)),
        live_plan.composer.map(|area| (area.width, area.height)),
        "SHELL-PERM: permission interaction must not resize the composer"
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
        rendered.contains(draft),
        "SHELL-PERM: preserved draft must remain visible in the attached composer\n{rendered}"
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
    let live_plan = plan_for(&app);

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
    assert_eq!(
        plan.composer.map(|area| area.y),
        live_plan.composer.map(|area| area.y.saturating_add(1)),
        "SHELL-QUESTION: suppressing idle keybinds moves the question composer down one row"
    );
    assert_eq!(
        plan.composer.map(|area| (area.width, area.height)),
        live_plan.composer.map(|area| (area.width, area.height)),
        "SHELL-QUESTION: question interaction must not resize the composer"
    );
    assert!(
        rendered.contains(draft),
        "SHELL-QUESTION: preserved draft must remain visible in the attached composer\n{rendered}"
    );
    assert!(
        rendered.contains('┃'),
        "OVL-QUESTION: question dock must paint ┃ rail matching reference packing\n{rendered}"
    );
}

/// OVL-PERM decision surface: the dock renders all four judgment options
/// (always-approve / session / once / reject) with the default selection
/// marker on always-approve.
#[test]
fn shell_perm_dock_renders_all_decision_options_with_default_marker() {
    // arrange
    let mut app = live_app();
    app.ingest_event(permission_requested_event(
        1,
        "perm_options_parity",
        "tool_call_options",
    ));

    // act
    let rendered = render(&app);

    // assert — the full ask/deny/allow surface with numbered radio options
    assert!(
        rendered.contains("1 (●) Yes, and don't ask again for anything (always-approve mode)"),
        "OVL-PERM: option 1 always-approve selected by default\n{rendered}"
    );
    assert!(
        rendered.contains("2 (○) Yes, allow all edits during this session"),
        "OVL-PERM: option 2 session grant rendered\n{rendered}"
    );
    assert!(
        rendered.contains("3 (○) Yes"),
        "OVL-PERM: option 3 allow-once rendered\n{rendered}"
    );
    assert!(
        rendered.contains("4 (○) No, reject (type to add feedback)"),
        "OVL-PERM: option 4 reject rendered\n{rendered}"
    );
}

#[test]
fn shell_perm_dock_keeps_all_choices_and_footer_visible_at_60x20() {
    // arrange
    // Given: a permission request rendered at the minimum supported viewport.
    let mut app = live_app();
    app.ingest_event(permission_requested_event(
        1,
        "perm_compact_parity",
        "tool_call_compact",
    ));

    // When: the live shell renders at 60x20.
    let rendered = render_to_string(&app, Rect::new(0, 0, 60, 20), |app, frame, _area| {
        ui::render_app(frame, app)
    });

    // act
    // Then: every decision and both essential footer actions remain truthful and visible.
    for expected in [
        "1 (●) Yes, always approve",
        "2 (○) Yes, allow edits this session",
        "3 (○) Yes, once",
        "4 (○) No, reject and add feedback",
        "1/4:select",
        "Ctrl+o:always",
        "Ctrl+c:cancel",
    ] {
        // assert
        assert!(
            rendered.contains(expected),
            "compact permission dock must retain {expected:?}\n{rendered}"
        );
    }
    assert!(
        rendered.lines().all(|line| line.chars().count() <= 60),
        "compact permission dock must not overflow 60 columns\n{rendered}"
    );
}

#[test]
fn shell_perm_short_content_uses_measured_height() {
    // arrange
    // Given: a one-line permission request at the primary reference viewport.
    let mut app = live_app();
    app.ingest_event(permission_requested_event(
        1,
        "perm_short_height",
        "tool_call_short_height",
    ));

    // When: the shell allocates the permission status region.
    let status = plan_for(&app)
        .status
        .expect("permission request must allocate a status region");

    // act
    // Then: the dock uses only its measured chrome, content, options, and footer rows.
    // assert
    assert_eq!(status.height, 9);
}

#[test]
fn shell_perm_long_content_collapses_with_truthful_indicator() {
    // arrange
    // Given: permission detail that exceeds the five-row collapsed budget.
    let mut app = live_app();
    let summary = (1..=12)
        .map(|line| format!("planned edit line {line}"))
        .collect::<Vec<_>>()
        .join("\n");
    app.ingest_event(permission_requested_event_with_summary(
        1,
        "perm_collapsed_height",
        "tool_call_collapsed_height",
        &summary,
    ));

    // When: the shell measures and renders the collapsed permission.
    let status = plan_for(&app)
        .status
        .expect("permission request must allocate a status region");
    let rendered = render(&app);

    // act
    // Then: four detail rows plus the expansion indicator determine the dock height.
    // assert
    assert_eq!(status.height, 13);
    assert!(
        rendered.contains("Ctrl-F to expand"),
        "collapsed permission must disclose hidden content\n{rendered}"
    );
}

#[test]
fn shell_perm_collapsed_height_obeys_small_screen_cap() {
    // arrange
    // Given: long permission detail at the minimum supported terminal height.
    let mut app = live_app();
    let summary = (1..=12)
        .map(|line| format!("compact planned edit line {line}"))
        .collect::<Vec<_>>()
        .join("\n");
    app.ingest_event(permission_requested_event_with_summary(
        1,
        "perm_small_screen_cap",
        "tool_call_small_screen_cap",
        &summary,
    ));

    // When: the shell allocates the permission at 60x20.
    let status = plan_at(&app, 60, 20)
        .status
        .expect("permission request must allocate a status region");

    // act
    // Then: Grok's min(half-screen max 10, eighty-percent) cap limits the dock.
    // assert
    assert_eq!(status.height, 10);
}

#[test]
fn shell_perm_ctrl_f_expands_and_collapses_long_content() {
    // arrange
    // Given: a collapsed permission whose complete detail needs twelve rows.
    let mut app = live_app();
    let summary = (1..=12)
        .map(|line| format!("expandable edit line {line}"))
        .collect::<Vec<_>>()
        .join("\n");
    app.ingest_event(permission_requested_event_with_summary(
        1,
        "perm_expand_toggle",
        "tool_call_expand_toggle",
        &summary,
    ));
    let collapsed_height = plan_for(&app)
        .status
        .expect("permission request must allocate a status region")
        .height;

    // When: Ctrl-F expands the detail, then toggles it closed again.
    app.handle_key(KeyEvent::new(KeyCode::Char('f'), KeyModifiers::CONTROL));
    let expanded_height = plan_for(&app)
        .status
        .expect("expanded permission must retain a status region")
        .height;
    let expanded = render(&app);
    app.handle_key(KeyEvent::new(KeyCode::Char('f'), KeyModifiers::CONTROL));
    let recollapsed_height = plan_for(&app)
        .status
        .expect("recollapsed permission must retain a status region")
        .height;

    // act
    // Then: expansion lifts the collapsed cap without disturbing the reversible state.
    // assert
    assert_eq!(
        (collapsed_height, expanded_height, recollapsed_height),
        (13, 20, 13)
    );
    assert!(
        expanded.contains("Ctrl-F to collapse"),
        "expanded permission must advertise the inverse action\n{expanded}"
    );
}

#[test]
fn shell_perm_measurement_uses_terminal_cell_width_for_cjk() {
    // arrange
    // Given: equal character counts whose terminal cell widths differ.
    let mut ascii = live_app();
    ascii.ingest_event(permission_requested_event_with_summary(
        1,
        "perm_ascii_width",
        "tool_call_ascii_width",
        &"a".repeat(30),
    ));
    let mut cjk = live_app();
    cjk.ingest_event(permission_requested_event_with_summary(
        1,
        "perm_cjk_width",
        "tool_call_cjk_width",
        &"界".repeat(30),
    ));

    // When: both requests are measured at the compact width.
    let ascii_height = plan_at(&ascii, 60, 20)
        .status
        .expect("ASCII permission must allocate a status region")
        .height;
    let cjk_height = plan_at(&cjk, 60, 20)
        .status
        .expect("CJK permission must allocate a status region")
        .height;

    // act
    // Then: double-width glyphs wrap into one additional visible row.
    // assert
    assert_eq!(cjk_height, ascii_height.saturating_add(1));
}

#[test]
fn shell_perm_renders_wide_cjk_without_narrow_viewport_overflow() {
    // arrange
    // Given: a permission summary containing CJK Extension A at compact width.
    let mut app = live_app();
    app.ingest_event(permission_requested_event_with_summary(
        1,
        "perm_cjk_render",
        "tool_call_cjk_render",
        &"㐀".repeat(24),
    ));

    // When: the real permission surface is rendered at 60x20.
    let rendered = render_at(&app, 60, 20);

    // act
    // Then: CJK content and every decision remain visible without cell overflow.
    // assert
    assert!(
        rendered.contains('㐀'),
        "CJK detail must remain visible\n{rendered}"
    );
    assert!(rendered.contains("No, reject and add feedback"));
    assert!(rendered.lines().all(|line| line.chars().count() <= 60));
}

include!("support/reference_parity_perm_question_test_part2_test.rs");
