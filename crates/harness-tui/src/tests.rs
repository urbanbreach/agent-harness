use std::sync::Mutex;

use std::fs;
use std::path::Path;
use std::sync::Arc;

use super::app::{ActivityEntry, ActivityStatus, AppState, ToolCallDisplayStatus};
use super::event_log::load_events_from_run_dir;
use super::lib_tests::{
    key_with_modifiers, render_live_buffer, render_live_cells, render_live_lines,
    row_text_and_palette, transcript_code_block_app, transcript_diff_block_app,
};

use super::*;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use harness_core::event::{
    ActorKind, EditAppliedEvent, EditProposedEvent, EventActor, EventEnvelopeV1, EventV1,
    PermissionRequestedEvent, PermissionResolvedEvent, ProviderRequestFinishedEvent,
    ProviderRequestStartedEvent, ProviderStreamDeltaEvent, RunFailedEvent, RunFinishedEvent,
    RunStartedEvent, TaskScheduleState, TaskScheduledEvent, ToolCallFinishedEvent,
    ToolCallRequestedEvent, ToolCallStartedEvent, ToolCallStatus, UserMessageSubmittedEvent,
    SCHEMA_VERSION,
};
use harness_core::perm::PermissionDecision;
use ratatui::style::Color;
use ratatui::{backend::TestBackend, Terminal};
use tempfile::TempDir;

#[test]
pub(super) fn module_replay_mode_snapshot_renders_two_pane_layout() {
    harness_core::config::clear_registered_integrations_config();
    harness_core::config::set_registered_lsp_config(harness_core::config::LspConfig::default());

    let run_dir = write_replay_fixture(sample_replay_events());
    let events = load_events_from_run_dir(run_dir.path()).expect("load replay events");

    let backend = TestBackend::new(80, 24);
    let mut terminal = Terminal::new(backend).expect("create terminal");

    let app = AppState::new_replay(run_dir.path().to_path_buf(), events);
    terminal
        .draw(|frame| ui::render_app(frame, &app))
        .expect("draw replay frame");

    assert_buffer_snapshot(
        "replay_mode_snapshot_renders_two_pane_layout",
        terminal.backend().buffer(),
    );
}

#[test]
fn replay_mode_r_key_marks_reload_requested() {
    let run_dir = write_replay_fixture(sample_replay_events());
    let events = load_events_from_run_dir(run_dir.path()).expect("load replay events");

    let mut app = AppState::new_replay(run_dir.path().to_path_buf(), events);
    app.handle_key(key(KeyCode::Char('r')));

    assert!(app.take_reload_requested());
}

#[test]
fn live_mode_snapshot_renders_grouped_streams() {
    let mut app = AppState::new_live(None, false, None);
    for event in sample_live_events() {
        app.ingest_event(event);
    }
    app.active_tab = app::Tab::Run;

    let backend = TestBackend::new(80, 24);
    let mut terminal = Terminal::new(backend).expect("create terminal");
    terminal
        .draw(|frame| ui::render_app(frame, &app))
        .expect("draw live frame");

    assert_buffer_snapshot(
        "live_mode_snapshot_renders_grouped_streams",
        terminal.backend().buffer(),
    );
}

#[test]
fn slash_commands_snapshot_renders_reference_style_popup() {
    let mut app = AppState::new_live(None, false, None);
    app.handle_key(KeyEvent::new(KeyCode::Char('/'), KeyModifiers::NONE));

    let backend = TestBackend::new(100, 24);
    let mut terminal = Terminal::new(backend).expect("create terminal");
    terminal
        .draw(|frame| ui::render_app(frame, &app))
        .expect("draw slash popup frame");

    assert_buffer_snapshot(
        "slash_commands_snapshot_renders_reference_style_popup",
        terminal.backend().buffer(),
    );
}

#[test]
fn tool_spacing_parity_snapshot_renders_grouped_context_and_output_transition() {
    let mut app = AppState::new_live(None, false, None);
    for event in sample_tool_spacing_events() {
        app.ingest_event(event);
    }
    app.active_tab = app::Tab::Run;

    let backend = TestBackend::new(120, 30);
    let mut terminal = Terminal::new(backend).expect("create terminal");
    terminal
        .draw(|frame| ui::render_app(frame, &app))
        .expect("draw tool spacing parity frame");

    assert_buffer_snapshot(
        "tool_spacing_parity_snapshot_renders_grouped_context_and_output_transition",
        terminal.backend().buffer(),
    );
}

#[test]
fn live_mode_renders_activity_and_transcript() {
    let mut app = AppState::new_live(None, false, None);
    for event in sample_live_events() {
        app.ingest_event(event);
    }
    app.active_tab = app::Tab::Run;

    let backend = TestBackend::new(80, 24);
    let mut terminal = Terminal::new(backend).expect("create terminal");
    terminal
        .draw(|frame| ui::render_app(frame, &app))
        .expect("draw live frame");

    let debug = format!("{:?}", terminal.backend().buffer());
    assert!(
        debug.contains("hello world"),
        "live mode must center the conversation surface"
    );
    assert!(
        !debug.contains("Activity ("),
        "live mode should not render the old activity cockpit by default"
    );
    assert!(
        debug.contains("hello world"),
        "transcript must show streaming content"
    );
}

#[test]
fn permission_modal_snapshot_renders_request() {
    let mut app = AppState::new_live(None, false, None);
    app.ingest_event(permission_requested_event(1, "perm_1", "tool_call_1"));

    let backend = TestBackend::new(80, 24);
    let mut terminal = Terminal::new(backend).expect("create terminal");
    terminal
        .draw(|frame| ui::render_app(frame, &app))
        .expect("draw modal frame");

    assert_buffer_snapshot(
        "permission_modal_snapshot_renders_request",
        terminal.backend().buffer(),
    );
}

#[test]
fn question_permission_modal_renders_questions_and_answer_input() {
    let mut app = AppState::new_live(None, false, None);
    app.ingest_event(envelope(
        1,
        Some("req_question_modal"),
        EventV1::PermissionRequested(PermissionRequestedEvent {
            permission_id: "perm_question_modal".to_string(),
            kind: "question".to_string(),
            tool_call_id: Some("tool_call_question".to_string()),
            summary: serde_json::json!({
                "questions": [{
                    "question": "Pick one",
                    "header": "Choice",
                    "options": [{"label": "A", "description": "Option A"}],
                }]
            })
            .to_string(),
            request_digest: "digest-question-modal".to_string(),
            timeout_ms: 30_000,
            default_decision: harness_core::event::PermissionDecision::Deny,
        }),
    ));

    let debug = render_live_buffer(&app, 100, 28);
    assert!(debug.contains("Pick one"));
    assert!(debug.contains("1. A"));
    assert!(debug.contains("Type your own answer"));
    assert!(debug.contains("↑↓ select"));
    assert!(debug.contains("enter submit"));
    assert!(debug.contains("esc dismiss"));
    assert!(!debug.contains("Question required"));
    assert!(!debug.contains("default deny"));
    assert!(!debug.contains("Allow once"));
}

#[test]
fn question_permission_modal_matches_reference_palette_contract() {
    let mut app = AppState::new_live(None, false, None);
    app.ingest_event(envelope(
        1,
        Some("req_question_palette"),
        EventV1::PermissionRequested(PermissionRequestedEvent {
            permission_id: "perm_question_palette".to_string(),
            kind: "question".to_string(),
            tool_call_id: Some("tool_call_question_palette".to_string()),
            summary: serde_json::json!({
                "questions": [
                    {
                        "question": "Pick one",
                        "header": "Choice",
                        "options": [{"label": "A", "description": "Option A"}],
                    },
                    {
                        "question": "Pick another",
                        "header": "Mode",
                        "options": [{"label": "B", "description": "Option B"}],
                    }
                ]
            })
            .to_string(),
            request_digest: "digest-question-palette".to_string(),
            timeout_ms: 30_000,
            default_decision: harness_core::event::PermissionDecision::Deny,
        }),
    ));

    let buffer = render_live_cells(&app, 100, 28);
    let (tab_row, tab_fgs, tab_bgs) =
        row_text_and_palette(&buffer, 100, "Choice").expect("tab row");
    let tab_start = tab_row[..tab_row.find("Choice").expect("choice tab start")]
        .chars()
        .count();
    let tab_end = tab_start + "Choice".chars().count();
    assert!(tab_bgs[tab_start..tab_end]
        .iter()
        .all(|color| *color == Color::Rgb(0x9D, 0x7C, 0xD8)));
    assert!(tab_fgs[tab_start..tab_end]
        .iter()
        .all(|color| *color == Color::Rgb(0x0A, 0x0A, 0x0A)));

    let (option_row, option_fgs, option_bgs) =
        row_text_and_palette(&buffer, 100, "1. A").expect("option row");
    let number_start = option_row[..option_row.find("1.").expect("number start")]
        .chars()
        .count();
    let label_start = option_row[..option_row.find("A").expect("label start")]
        .chars()
        .count();
    assert_eq!(option_bgs[number_start], Color::Rgb(0x1E, 0x1E, 0x1E));
    assert_eq!(option_bgs[label_start], Color::Rgb(0x1E, 0x1E, 0x1E));
    assert_eq!(option_fgs[label_start], Color::Rgb(0x5C, 0x9C, 0xF5));
}

#[test]
fn answered_questions_render_in_completed_tool_row() {
    let mut app = AppState::new_live(None, false, None);
    app.ingest_event(envelope(
        1,
        Some("req_question_result"),
        EventV1::UserMessageSubmitted(UserMessageSubmittedEvent {
            request_id: "req_question_result".to_string(),
            text: "Ask me a follow-up".to_string(),
        }),
    ));
    app.ingest_event(envelope(
        2,
        Some("req_question_result"),
        EventV1::ToolCallRequested(ToolCallRequestedEvent {
            tool_call_id: "tool_call_question_result".to_string(),
            tool_id: "user.question".to_string(),
            args_summary: serde_json::json!({
                "questions": [
                    {
                        "question": "Pick one",
                        "header": "Choice",
                        "options": [{"label": "A", "description": "Option A"}],
                    },
                    {
                        "question": "Pick another",
                        "header": "Mode",
                        "options": [{"label": "B", "description": "Option B"}],
                    }
                ]
            })
            .to_string(),
            args_digest: "digest-question-result-tool".to_string(),
            metadata: None,
        }),
    ));
    app.ingest_event(envelope(
        3,
        Some("tool_call_question_result"),
        EventV1::PermissionRequested(PermissionRequestedEvent {
            permission_id: "perm_question_result".to_string(),
            kind: "question".to_string(),
            tool_call_id: Some("tool_call_question_result".to_string()),
            summary: serde_json::json!({
                "questions": [
                    {
                        "question": "Pick one",
                        "header": "Choice",
                        "options": [{"label": "A", "description": "Option A"}],
                    },
                    {
                        "question": "Pick another",
                        "header": "Mode",
                        "options": [{"label": "B", "description": "Option B"}],
                    }
                ]
            })
            .to_string(),
            request_digest: "digest-question-result-permission".to_string(),
            timeout_ms: 30_000,
            default_decision: harness_core::event::PermissionDecision::Deny,
        }),
    ));
    app.ingest_event(envelope(
        4,
        Some("tool_call_question_result"),
        EventV1::PermissionResolved(PermissionResolvedEvent {
            permission_id: "perm_question_result".to_string(),
            decision: harness_core::event::PermissionDecision::Allow,
            reason: Some("[[\"A\"],[]]".to_string()),
        }),
    ));
    app.ingest_event(envelope(
        5,
        Some("req_question_result"),
        EventV1::ToolCallFinished(ToolCallFinishedEvent {
            tool_call_id: "tool_call_question_result".to_string(),
            status: ToolCallStatus::Succeeded,
            output_summary: Some("User has answered your questions.".to_string()),
            output_digest: Some("digest-question-result-output".to_string()),
            output_json: None,
            metadata: None,
        }),
    ));

    let debug = render_live_buffer(&app, 120, 30);
    assert!(debug.contains("Questions"));
    assert!(debug.contains("Pick one"));
    assert!(debug.contains("A"));
    assert!(debug.contains("Pick another"));
    assert!(debug.contains("(no answer)"));
}

#[test]
fn permission_modal_ctrl_y_emits_resolve_intent_and_closes_on_resolved() {
    let intents = Arc::new(Mutex::new(Vec::<UiIntent>::new()));
    let intent_sink = {
        let intents = Arc::clone(&intents);
        Arc::new(move |intent: UiIntent| {
            intents.lock().expect("lock intents").push(intent);
        })
    };

    let mut app = AppState::new_live(None, false, Some(intent_sink));
    app.ingest_event(permission_requested_event(1, "perm_1", "tool_call_1"));

    app.handle_key(key_with_modifiers(
        KeyCode::Char('y'),
        KeyModifiers::CONTROL,
    ));

    let intents = intents.lock().expect("lock intents");
    assert_eq!(intents.len(), 1);
    assert_eq!(
        intents[0],
        UiIntent::ResolvePermission {
            permission_id: "perm_1".to_string(),
            decision: PermissionDecision::Allow,
            reason: None,
            grant_scope: None,
        }
    );
    drop(intents);

    assert!(app.active_permission().is_some());

    app.ingest_event(permission_resolved_event(
        2,
        "perm_1",
        PermissionDecision::Allow,
    ));
    assert!(app.active_permission().is_none());
}

#[test]
pub(super) fn module_transcript_edit_snapshot_renders_inline_diff() {
    harness_core::config::clear_registered_integrations_config();
    harness_core::config::set_registered_lsp_config(harness_core::config::LspConfig::default());

    let run_dir = write_diff_fixture(true);
    let events = load_events_from_run_dir(run_dir.path()).expect("load diff fixture");

    let app = AppState::new_replay(run_dir.path().to_path_buf(), events);

    let backend = TestBackend::new(80, 24);
    let mut terminal = Terminal::new(backend).expect("create terminal");
    terminal
        .draw(|frame| ui::render_app(frame, &app))
        .expect("draw transcript edit frame");

    assert_buffer_snapshot(
        "transcript_edit_snapshot_renders_inline_diff",
        terminal.backend().buffer(),
    );
}

#[test]
pub(super) fn module_inline_diff_does_not_leave_large_gap_before_active_footer() {
    harness_core::config::clear_registered_integrations_config();
    harness_core::config::set_registered_lsp_config(harness_core::config::LspConfig::default());

    let run_dir = write_diff_fixture(true);
    let events = load_events_from_run_dir(run_dir.path()).expect("load diff fixture");
    let app = AppState::new_replay(run_dir.path().to_path_buf(), events);

    let buffer = render_live_cells(&app, 80, 24);
    let lines = buffer
        .content
        .chunks(80)
        .map(|row| row.iter().map(|cell| cell.symbol()).collect::<String>())
        .collect::<Vec<_>>();
    let diff_last_row = lines
        .iter()
        .rposition(|line| line.contains("gamma") || line.contains("BETA") || line.contains("beta"))
        .expect("inline diff row");
    let footer_row = lines
        .iter()
        .position(|line| line.contains("Assistant") && line.contains("active"))
        .expect("active assistant footer row");

    assert_eq!(
        footer_row,
        diff_last_row + 2,
        "inline diff should hand off to the active footer row with only one blank separator\n{lines:#?}"
    );
}

pub(super) fn module_fenced_code_highlighting_uses_syntect_styles_for_known_languages() {
    let app = transcript_code_block_app("rust");
    let rendered = render_live_lines(&app, 120, 30);
    let buffer = render_live_cells(&app, 120, 30);
    let theme = Theme::default();
    let (_, _, prose_backgrounds) =
        row_text_and_palette(&buffer, 120, "Here is a sample:").expect("prose row");
    let rendered_code_row = rendered
        .lines()
        .find(|row| row.contains("let answer = 42;"))
        .expect("rendered code row");
    assert!(
        !rendered_code_row.contains('┃'),
        "assistant code should render without a nested frame rail\n{rendered}"
    );
    let (row, colors, backgrounds) =
        row_text_and_palette(&buffer, 120, "let answer = 42;").expect("highlighted code row");
    let start = row.find("let answer = 42;").expect("code starts");
    let end = start + "let answer = 42;".chars().count();
    let unique = colors[start..end]
        .iter()
        .copied()
        .filter(|color| !matches!(color, ratatui::style::Color::Reset))
        .map(|color| format!("{color:?}"))
        .collect::<std::collections::HashSet<_>>();

    assert!(
        unique.len() >= 2,
        "known-language highlighting should render multiple colors: {row}"
    );
    assert!(
        unique
            .iter()
            .any(|color| *color != format!("{:?}", theme.text.primary)),
        "known-language highlighting should not fall back to plain transcript color: {row}"
    );
    assert!(
        backgrounds[start..end]
            .iter()
            .zip(&prose_backgrounds[start..end])
            .all(|(code_background, prose_background)| code_background == prose_background),
        "highlighted code should use the same background as surrounding prose: {row}"
    );
}

pub(super) fn module_fenced_code_highlighting_falls_back_to_plain_text_when_unknown() {
    let app = transcript_code_block_app("totally-unknown-lang");
    let buffer = render_live_cells(&app, 120, 30);
    let theme = Theme::default();
    let (_, _, prose_backgrounds) =
        row_text_and_palette(&buffer, 120, "Here is a sample:").expect("prose row");
    let (row, colors, backgrounds) =
        row_text_and_palette(&buffer, 120, "let answer = 42;").expect("plain code row");
    let start = row.find("let answer = 42;").expect("code starts");
    let end = start + "let answer = 42;".chars().count();
    let unique = colors[start..end]
        .iter()
        .copied()
        .filter(|color| !matches!(color, ratatui::style::Color::Reset))
        .map(|color| format!("{color:?}"))
        .collect::<std::collections::HashSet<_>>();

    assert_eq!(
        unique,
        std::collections::HashSet::from([format!("{:?}", theme.text.primary)])
    );
    assert!(
        backgrounds[start..end]
            .iter()
            .zip(&prose_backgrounds[start..end])
            .all(|(code_background, prose_background)| code_background == prose_background),
        "plain code fallback should use the same background as surrounding prose: {row}"
    );
}

pub(super) fn module_diff_renderer_uses_stacked_layout_in_narrow_geometries() {
    let app = transcript_diff_block_app();

    let rendered = render_live_lines(&app, 80, 24);
    assert!(
        rendered.contains("1    1   alpha"),
        "narrow diff should show paired context line numbers\n{rendered}"
    );
    assert!(
        rendered.contains("2      - beta"),
        "narrow diff should stack removals with a deletion gutter\n{rendered}"
    );
    assert!(
        rendered.contains("2 + BETA"),
        "narrow diff should stack additions with an insertion gutter\n{rendered}"
    );
    assert!(
        !rendered
            .lines()
            .any(|line| line.contains("- beta") && line.contains("+ BETA")),
        "narrow diff should not pair both columns on one row\n{rendered}"
    );
}

pub(super) fn module_wide_diff_renderer_pairs_before_and_after_columns() {
    let app = transcript_diff_block_app();

    let rendered = render_live_lines(&app, 220, 30);
    assert!(
        rendered
            .lines()
            .any(|line| line.contains("2 - beta") && line.contains("2 + BETA")),
        "wide diff should pair before/after columns on one row with preserved gutters\n{rendered}"
    );
}

#[test]
pub(super) fn module_diff_renderer_switches_to_side_by_side_at_primary_widths() {
    let app = transcript_diff_block_app();

    let rendered = render_live_lines(&app, 120, 30);
    assert!(
        rendered
            .lines()
            .any(|line| line.contains("- beta") && line.contains("+ BETA")),
        "primary-width layouts should pair before/after columns on one row\n{rendered}"
    );
}

pub(super) fn module_transcript_edit_tool_wide_diff_uses_syntax_highlighting_and_split_palettes() {
    let run_dir = tempfile::tempdir().expect("create run dir");
    let artifacts_dir = run_dir.path().join("artifacts");
    std::fs::create_dir_all(&artifacts_dir).expect("create artifacts dir");
    std::fs::write(
        artifacts_dir.join("harness-inline.diff"),
        "--- crates/harness-tui/src/ui.rs\n+++ crates/harness-tui/src/ui.rs\n@@ -44,8 +44,7 @@\n use ui_secondary::{\n-    render_diff_tab, render_events_tab, render_help_tab,\n+    render_events_tab, render_help_tab, render_live_details_overlay,\n     render_operator_sidebar,\n };\n",
    )
    .expect("write inline diff fixture");

    let mut app = AppState::new_live(Some(run_dir.path().to_path_buf()), false, None);
    let mut entry = ActivityEntry {
        request_id: "request-edit-inline-wide".to_string(),
        profile_label: "build".to_string(),
        model_id: "model-1".to_string(),
        provider_id: "default".to_string(),
        status: ActivityStatus::Done,
        user_message: None,
        user_timestamp: None,
        request_data: None,
        thinking_text: String::new(),
        transcript_text: String::new(),
        usage: None,
        cache_usage: None,
        error_message: None,
        permissions: Vec::new(),
        tool_calls: Vec::new(),
        first_seq: 10,
        last_seq: 11,
        first_mono_ms: 10,
        last_mono_ms: 11,
    };
    entry.tool_calls.push(crate::app::ToolCallEntry {
        tool_call_id: "call-edit-wide-1".to_string(),
        tool_id: "edit.hashline_apply".to_string(),
        canonical_tool_id: None,
        alias_source_tool_id: None,
        resolved_tool_identity: None,
        args_summary: r#"{"path":"crates/harness-tui/src/ui.rs"}"#.to_string(),
        args_digest: "digest-edit-wide".to_string(),
        lifecycle_state: None,
        status: ToolCallDisplayStatus::Succeeded,
        output_summary: Some("Edit applied".to_string()),
        output_digest: Some("digest-edit-wide-output".to_string()),
        output_json: None,
        truncated_output: Some("Edit applied".to_string()),
        edit: Some(crate::app::EditEntry {
            edit_id: "edit-inline-wide-1".to_string(),
            path: "crates/harness-tui/src/ui.rs".to_string(),
            status: crate::app::EditDisplayStatus::Applied,
            summary: Some("Remove diff review surface".to_string()),
            patch_digest: Some("digest-patch-wide".to_string()),
            new_file_digest: Some("digest-new-file-wide".to_string()),
            diff_rel_path: Some("artifacts/harness-inline.diff".to_string()),
            diff_digest: Some("digest-diff-wide".to_string()),
            rejection_reason: None,
        }),
        lineage: None,
        artifact_refs: Vec::new(),
        timing_elapsed_ms: None,
        permissions: Vec::new(),
        first_seq: 10,
        last_seq: 11,
        first_mono_ms: 10,
        last_mono_ms: 11,
        first_timestamp: None,
        last_timestamp: None,
    });
    app.activities = std::collections::VecDeque::from(vec![entry]);

    let rendered = render_live_lines(&app, 220, 30);
    assert!(
        !rendered.contains("@@ -44,8 +44,7 @@"),
        "wide transcript inline diff should suppress raw hunk headers\n{rendered}"
    );
    assert!(
        rendered
            .lines()
            .any(|line| line.matches("use ui_secondary::{").count() == 2),
        "wide transcript inline diff should keep the context row side-by-side\n{rendered}"
    );

    let buffer = render_live_cells(&app, 220, 30);
    let theme = Theme::default();

    let (context_row, context_fgs, _) = row_text_and_palette(&buffer, 220, "use ui_secondary::{")
        .expect("wide context row with syntax-highlighted code");
    let context_matches = context_row
        .match_indices("use ui_secondary::{")
        .map(|(index, _)| index)
        .collect::<Vec<_>>();
    assert!(
        context_matches.len() >= 2,
        "expected both diff columns in row: {context_row}"
    );
    let context_start = context_matches[0];
    let context_end = context_start + "use ui_secondary::{".chars().count();
    let syntax_colors = context_fgs[context_start..context_end]
        .iter()
        .copied()
        .filter(|color| !matches!(color, ratatui::style::Color::Reset))
        .map(|color| format!("{color:?}"))
        .collect::<std::collections::HashSet<_>>();
    assert!(
        syntax_colors.len() >= 2,
        "wide transcript code rows should retain syntax highlighting, not collapse to a single color: {context_row}"
    );
    assert!(
        syntax_colors
            .iter()
            .any(|color| *color != format!("{:?}", theme.text.primary)),
        "wide transcript code rows should not fall back to plain transcript coloring: {context_row}"
    );

    let (changed_row, _, changed_bgs) =
        row_text_and_palette(&buffer, 220, "render_live_details_overlay")
            .expect("wide changed row with side-by-side edit columns");
    let removed_start = changed_row.find("render_diff_tab").expect("removed symbol");
    let added_start = changed_row
        .find("render_live_details_overlay")
        .expect("added symbol");
    assert_ne!(
        changed_bgs[removed_start], changed_bgs[added_start],
        "wide transcript inline diff should tint before/after columns independently: {changed_row}"
    );
}

#[test]
fn transcript_edit_snapshot_handles_missing_artifact() {
    let run_dir = write_diff_fixture(false);
    let events = load_events_from_run_dir(run_dir.path()).expect("load diff fixture");

    let app = AppState::new_replay(run_dir.path().to_path_buf(), events);

    let backend = TestBackend::new(80, 24);
    let mut terminal = Terminal::new(backend).expect("create terminal");
    terminal
        .draw(|frame| ui::render_app(frame, &app))
        .expect("draw transcript edit frame without diff artifact");

    let debug = format!("{:?}", terminal.backend().buffer());
    assert!(debug.contains("← Patch · demo.txt"));
    assert!(!debug.contains("Select an edit event to view diff"));
    assert!(!debug.contains("diff artifact missing"));
}

#[test]
fn prompt_focus_enter_emits_submit_intent() {
    let intents = Arc::new(Mutex::new(Vec::<UiIntent>::new()));
    let intent_sink = {
        let intents = Arc::clone(&intents);
        Arc::new(move |intent: UiIntent| {
            intents.lock().expect("lock intents").push(intent);
        })
    };

    let mut app = AppState::new_live(None, false, Some(intent_sink));
    app.focus = app::Focus::Prompt;

    for c in "hello".chars() {
        app.handle_key(key(KeyCode::Char(c)));
    }

    app.handle_key(key(KeyCode::Enter));

    let intents = intents.lock().expect("lock intents");
    assert_eq!(intents.len(), 1);
    assert_eq!(
        intents[0],
        UiIntent::SubmitPrompt {
            text: "hello".to_string(),
            selected_file_tags: Vec::new(),
            selected_agent_tags: Vec::new(),
            selected_resource_tags: Vec::new(),
            launch_metadata: app::LaunchMetadata::default(),
        }
    );
    drop(intents);

    assert_eq!(app.prompt_buffer, "");
    assert_eq!(app.prompt_history.len(), 1);
    assert_eq!(app.prompt_history[0], "hello");
}

#[test]
fn activity_groups_by_request_id() {
    let mut app = AppState::new_live(None, false, None);

    app.ingest_event(envelope(
        1,
        Some("req_001"),
        EventV1::UserMessageSubmitted(UserMessageSubmittedEvent {
            request_id: "req_001".to_string(),
            text: "Hello AI".to_string(),
        }),
    ));

    app.ingest_event(envelope(
        2,
        Some("req_001"),
        EventV1::ProviderRequestStarted(ProviderRequestStartedEvent {
            request_id: "req_001".to_string(),
            provider_id: "openai".to_string(),
            model_id: "gpt-5-codex".to_string(),
            prompt_summary: "Hello AI".to_string(),
            request_digest: "digest-1".to_string(),
            metadata: None,
        }),
    ));

    assert_eq!(app.activities.len(), 1);
    let activity = app.activities.front().unwrap();
    assert_eq!(activity.request_id, "req_001");
    assert_eq!(activity.provider_id, "openai");
    assert_eq!(activity.model_id, "gpt-5-codex");
    assert!(activity.user_message.is_some());
    assert_eq!(activity.user_message.as_ref().unwrap().text, "Hello AI");
}

#[test]
fn transcript_accumulates_stream_deltas() {
    let mut app = AppState::new_live(None, false, None);

    app.ingest_event(envelope(
        1,
        Some("req_001"),
        EventV1::ProviderRequestStarted(ProviderRequestStartedEvent {
            request_id: "req_001".to_string(),
            provider_id: "openai".to_string(),
            model_id: "gpt-5-codex".to_string(),
            prompt_summary: "test".to_string(),
            request_digest: "digest-1".to_string(),
            metadata: None,
        }),
    ));

    app.ingest_event(envelope(
        2,
        Some("req_001"),
        EventV1::ProviderStreamDelta(ProviderStreamDeltaEvent {
            request_id: "req_001".to_string(),
            delta: "Hello ".to_string(),
        }),
    ));

    app.ingest_event(envelope(
        3,
        Some("req_001"),
        EventV1::ProviderStreamDelta(ProviderStreamDeltaEvent {
            request_id: "req_001".to_string(),
            delta: "world!".to_string(),
        }),
    ));

    let activity = app.activities.front().unwrap();
    assert_eq!(activity.transcript_text, "Hello world!");
}

#[test]
fn activity_status_done_on_request_finished() {
    let mut app = AppState::new_live(None, false, None);

    app.ingest_event(envelope(
        1,
        Some("req_001"),
        EventV1::ProviderRequestStarted(ProviderRequestStartedEvent {
            request_id: "req_001".to_string(),
            provider_id: "openai".to_string(),
            model_id: "gpt-5-codex".to_string(),
            prompt_summary: "test".to_string(),
            request_digest: "digest-1".to_string(),
            metadata: None,
        }),
    ));

    assert_eq!(
        app.activities.front().unwrap().status,
        crate::app::ActivityStatus::Streaming
    );

    app.ingest_event(envelope(
        2,
        Some("req_001"),
        EventV1::ProviderRequestFinished(ProviderRequestFinishedEvent {
            request_id: "req_001".to_string(),
            finish_reason: "stop".to_string(),
            output_digest: Some("digest-out".to_string()),
            usage: None,
            metadata: None,
        }),
    ));

    assert_eq!(
        app.activities.front().unwrap().status,
        crate::app::ActivityStatus::Done
    );
}

#[test]
fn activity_status_error_on_run_failed() {
    let mut app = AppState::new_live(None, false, None);

    app.ingest_event(envelope(
        1,
        Some("req_001"),
        EventV1::ProviderRequestStarted(ProviderRequestStartedEvent {
            request_id: "req_001".to_string(),
            provider_id: "openai".to_string(),
            model_id: "gpt-5-codex".to_string(),
            prompt_summary: "test".to_string(),
            request_digest: "digest-1".to_string(),
            metadata: None,
        }),
    ));

    app.ingest_event(envelope(
        2,
        None,
        EventV1::RunFailed(RunFailedEvent {
            error: "API rate limit exceeded".to_string(),
        }),
    ));

    let activity = app.activities.front().unwrap();
    assert_eq!(activity.status, crate::app::ActivityStatus::Error);
    assert_eq!(
        activity.error_message.as_ref().unwrap(),
        "API rate limit exceeded"
    );
}

#[test]
fn memory_cap_enforces_max_events() {
    let mut app = AppState::new_live(None, false, None);
    app.memory_caps.max_events = 5;

    for i in 1..=10 {
        app.ingest_event(envelope(
            i,
            None,
            EventV1::RunStarted(RunStartedEvent {
                run_name: format!("run-{}", i),
                workspace_root: "/tmp".to_string(),
            }),
        ));
    }

    assert_eq!(app.events.len(), 5);
    assert_eq!(app.events_trimmed_count, 5);
}

#[test]
fn memory_cap_enforces_max_transcript_chars() {
    let mut app = AppState::new_live(None, false, None);
    app.memory_caps.max_transcript_chars = 20;

    app.ingest_event(envelope(
        1,
        Some("req_001"),
        EventV1::ProviderRequestStarted(ProviderRequestStartedEvent {
            request_id: "req_001".to_string(),
            provider_id: "openai".to_string(),
            model_id: "gpt-5-codex".to_string(),
            prompt_summary: "test".to_string(),
            request_digest: "digest-1".to_string(),
            metadata: None,
        }),
    ));

    // Add 30 characters in deltas
    for i in 0..3 {
        app.ingest_event(envelope(
            2 + i,
            Some("req_001"),
            EventV1::ProviderStreamDelta(ProviderStreamDeltaEvent {
                request_id: "req_001".to_string(),
                delta: "0123456789".to_string(),
            }),
        ));
    }

    assert!(app.transcript_trimmed_count > 0);
}

#[test]
fn run_workspace_renders_activity_with_compact_format() {
    let mut app = AppState::new_live(None, false, None);

    app.ingest_event(envelope(
        1,
        Some("req_000123"),
        EventV1::UserMessageSubmitted(UserMessageSubmittedEvent {
            request_id: "req_000123".to_string(),
            text: "Hello".to_string(),
        }),
    ));

    app.ingest_event(envelope(
        2,
        Some("req_000123"),
        EventV1::ProviderRequestStarted(ProviderRequestStartedEvent {
            request_id: "req_000123".to_string(),
            provider_id: "openai".to_string(),
            model_id: "gpt-5-codex".to_string(),
            prompt_summary: "Hello".to_string(),
            request_digest: "digest-1".to_string(),
            metadata: None,
        }),
    ));

    app.handle_key(key(crossterm::event::KeyCode::Tab));
    app.handle_key(key(crossterm::event::KeyCode::Char('i')));

    let backend = TestBackend::new(80, 24);
    let mut terminal = Terminal::new(backend).expect("create terminal");
    terminal
        .draw(|frame| ui::render_app(frame, &app))
        .expect("draw run workspace frame");

    let debug = format!("{:?}", terminal.backend().buffer());
    assert!(
        debug.contains("gpt-5-codex"),
        "operator sidebar must show model_id"
    );
}

#[test]
fn tool_call_requested_renders_pending_status() {
    let mut app = AppState::new_live(None, false, None);

    app.ingest_event(envelope(
        1,
        Some("req_001"),
        EventV1::UserMessageSubmitted(UserMessageSubmittedEvent {
            request_id: "req_001".to_string(),
            text: "Hello".to_string(),
        }),
    ));

    app.ingest_event(envelope(
        2,
        Some("req_001"),
        EventV1::ProviderRequestStarted(ProviderRequestStartedEvent {
            request_id: "req_001".to_string(),
            provider_id: "openai".to_string(),
            model_id: "gpt-5-codex".to_string(),
            prompt_summary: "Hello".to_string(),
            request_digest: "digest-1".to_string(),
            metadata: None,
        }),
    ));

    app.ingest_event(envelope(
        3,
        Some("req_001"),
        EventV1::ToolCallRequested(ToolCallRequestedEvent {
            tool_call_id: "tc_001".to_string(),
            tool_id: "fs.read".to_string(),
            args_summary: r#"{"path":"test.txt"}"#.to_string(),
            args_digest: "digest-args".to_string(),
            metadata: None,
        }),
    ));

    app.active_tab = app::Tab::Run;

    let backend = TestBackend::new(80, 24);
    let mut terminal = Terminal::new(backend).expect("create terminal");
    terminal
        .draw(|frame| ui::render_app(frame, &app))
        .expect("draw tool call frame");

    let debug = format!("{:?}", terminal.backend().buffer());
    assert!(
        debug.contains("Read test.txt"),
        "transcript must show tool title"
    );
}

#[test]
fn tool_call_started_renders_running_status() {
    let mut app = AppState::new_live(None, false, None);

    app.ingest_event(envelope(
        1,
        Some("req_001"),
        EventV1::ProviderRequestStarted(ProviderRequestStartedEvent {
            request_id: "req_001".to_string(),
            provider_id: "openai".to_string(),
            model_id: "gpt-5-codex".to_string(),
            prompt_summary: "Hello".to_string(),
            request_digest: "digest-1".to_string(),
            metadata: None,
        }),
    ));

    app.ingest_event(envelope(
        2,
        Some("req_001"),
        EventV1::ToolCallRequested(ToolCallRequestedEvent {
            tool_call_id: "tc_001".to_string(),
            tool_id: "fs.read".to_string(),
            args_summary: r#"{"path":"test.txt"}"#.to_string(),
            args_digest: "digest-args".to_string(),
            metadata: None,
        }),
    ));

    app.ingest_event(envelope(
        3,
        Some("req_001"),
        EventV1::ToolCallStarted(ToolCallStartedEvent {
            tool_call_id: "tc_001".to_string(),
        }),
    ));

    app.active_tab = app::Tab::Run;

    let backend = TestBackend::new(80, 24);
    let mut terminal = Terminal::new(backend).expect("create terminal");
    terminal
        .draw(|frame| ui::render_app(frame, &app))
        .expect("draw tool call frame");

    let debug = format!("{:?}", terminal.backend().buffer());
    assert!(
        debug.contains("Read test.txt"),
        "transcript must show tool title"
    );
}

#[test]
fn tool_call_finished_renders_truncated_output() {
    let mut app = AppState::new_live(None, false, None);

    app.ingest_event(envelope(
        1,
        Some("req_001"),
        EventV1::ProviderRequestStarted(ProviderRequestStartedEvent {
            request_id: "req_001".to_string(),
            provider_id: "openai".to_string(),
            model_id: "gpt-5-codex".to_string(),
            prompt_summary: "Hello".to_string(),
            request_digest: "digest-1".to_string(),
            metadata: None,
        }),
    ));

    app.ingest_event(envelope(
        2,
        Some("req_001"),
        EventV1::ToolCallRequested(ToolCallRequestedEvent {
            tool_call_id: "tc_001".to_string(),
            tool_id: "fs.read".to_string(),
            args_summary: r#"{"path":"test.txt"}"#.to_string(),
            args_digest: "digest-args".to_string(),
            metadata: None,
        }),
    ));

    app.ingest_event(envelope(
        3,
        Some("req_001"),
        EventV1::ToolCallStarted(ToolCallStartedEvent {
            tool_call_id: "tc_001".to_string(),
        }),
    ));

    let long_output = "x".repeat(150);
    app.ingest_event(envelope(
        4,
        Some("req_001"),
        EventV1::ToolCallFinished(ToolCallFinishedEvent {
            tool_call_id: "tc_001".to_string(),
            status: ToolCallStatus::Succeeded,
            output_summary: Some(long_output.clone()),
            output_digest: Some("digest-output".to_string()),
            output_json: None,
            metadata: None,
        }),
    ));

    app.active_tab = app::Tab::Run;

    let backend = TestBackend::new(80, 24);
    let mut terminal = Terminal::new(backend).expect("create terminal");
    terminal
        .draw(|frame| ui::render_app(frame, &app))
        .expect("draw tool call frame");

    let debug = format!("{:?}", terminal.backend().buffer());
    assert!(
        debug.contains("Read test.txt"),
        "transcript must show tool title"
    );
    assert!(
        !debug.contains(&"x".repeat(20)),
        "successful read rows should not dump large output payloads"
    );
}

#[test]
fn tool_call_failed_renders_error() {
    let mut app = AppState::new_live(None, false, None);

    app.ingest_event(envelope(
        1,
        Some("req_001"),
        EventV1::ProviderRequestStarted(ProviderRequestStartedEvent {
            request_id: "req_001".to_string(),
            provider_id: "openai".to_string(),
            model_id: "gpt-5-codex".to_string(),
            prompt_summary: "Hello".to_string(),
            request_digest: "digest-1".to_string(),
            metadata: None,
        }),
    ));

    app.ingest_event(envelope(
        2,
        Some("req_001"),
        EventV1::ToolCallRequested(ToolCallRequestedEvent {
            tool_call_id: "tc_001".to_string(),
            tool_id: "shell.run".to_string(),
            args_summary: r#"{"cmd":"false"}"#.to_string(),
            args_digest: "digest-args".to_string(),
            metadata: None,
        }),
    ));

    app.ingest_event(envelope(
        3,
        Some("req_001"),
        EventV1::ToolCallStarted(ToolCallStartedEvent {
            tool_call_id: "tc_001".to_string(),
        }),
    ));

    app.ingest_event(envelope(
        4,
        Some("req_001"),
        EventV1::ToolCallFinished(ToolCallFinishedEvent {
            tool_call_id: "tc_001".to_string(),
            status: ToolCallStatus::Failed,
            output_summary: Some("exit code: 1".to_string()),
            output_digest: None,
            output_json: None,
            metadata: None,
        }),
    ));

    app.active_tab = app::Tab::Run;

    let backend = TestBackend::new(80, 24);
    let mut terminal = Terminal::new(backend).expect("create terminal");
    terminal
        .draw(|frame| ui::render_app(frame, &app))
        .expect("draw tool call frame");

    let debug = format!("{:?}", terminal.backend().buffer());
    assert!(
        debug.contains("exit code: 1") || debug.contains("tool call"),
        "transcript must show error message"
    );
}

#[test]
fn assistant_markdown_renders_headings_lists_and_quotes() {
    let mut app = AppState::new_live(None, false, None);

    app.ingest_event(envelope(
        1,
        Some("req_markdown"),
        EventV1::ProviderRequestStarted(ProviderRequestStartedEvent {
            request_id: "req_markdown".to_string(),
            provider_id: "openai".to_string(),
            model_id: "gpt-5-codex".to_string(),
            prompt_summary: "Render markdown".to_string(),
            request_digest: "digest-markdown".to_string(),
            metadata: None,
        }),
    ));
    app.ingest_event(envelope(
        2,
        Some("req_markdown"),
        EventV1::ProviderStreamDelta(ProviderStreamDeltaEvent {
            request_id: "req_markdown".to_string(),
            delta: "# Plan\n- [x] Capture transcript parity\n- [ ] Ship final polish\n> Keep the chrome muted".to_string(),
        }),
    ));
    app.ingest_event(envelope(
        3,
        Some("req_markdown"),
        EventV1::ProviderRequestFinished(ProviderRequestFinishedEvent {
            request_id: "req_markdown".to_string(),
            finish_reason: "stop".to_string(),
            output_digest: Some("digest-markdown-finished".to_string()),
            usage: None,
            metadata: None,
        }),
    ));

    app.active_tab = app::Tab::Run;

    let backend = TestBackend::new(120, 30);
    let mut terminal = Terminal::new(backend).expect("create terminal");
    terminal
        .draw(|frame| ui::render_app(frame, &app))
        .expect("draw markdown frame");

    let debug = format!("{:?}", terminal.backend().buffer());
    assert!(debug.contains("Plan"), "heading text should render");
    assert!(
        debug.contains("☑ Capture transcript parity"),
        "checked task should render with checkbox"
    );
    assert!(
        debug.contains("☐ Ship final polish"),
        "unchecked task should render with checkbox"
    );
    assert!(
        debug.contains("▍ Keep the chrome muted"),
        "blockquote should render with quote glyph"
    );
}

#[test]
fn block_style_tool_rows_render_titles_and_argument_blocks() {
    let mut app = AppState::new_live(None, false, None);

    app.ingest_event(envelope(
        1,
        Some("req_shell"),
        EventV1::ProviderRequestStarted(ProviderRequestStartedEvent {
            request_id: "req_shell".to_string(),
            provider_id: "openai".to_string(),
            model_id: "gpt-5-codex".to_string(),
            prompt_summary: "Run the suite".to_string(),
            request_digest: "digest-shell".to_string(),
            metadata: None,
        }),
    ));
    app.ingest_event(envelope(
        2,
        Some("req_shell"),
        EventV1::ToolCallRequested(ToolCallRequestedEvent {
            tool_call_id: "tc_shell".to_string(),
            tool_id: "shell.run".to_string(),
            args_summary: r#"{"cmd":"cargo test -p harness-tui","cwd":"/tmp/demo"}"#.to_string(),
            args_digest: "digest-shell-args".to_string(),
            metadata: None,
        }),
    ));
    app.ingest_event(envelope(
        3,
        Some("req_shell"),
        EventV1::ToolCallStarted(ToolCallStartedEvent {
            tool_call_id: "tc_shell".to_string(),
        }),
    ));
    app.ingest_event(envelope(
        4,
        Some("req_shell"),
        EventV1::ToolCallFinished(ToolCallFinishedEvent {
            tool_call_id: "tc_shell".to_string(),
            status: ToolCallStatus::Failed,
            output_summary: Some("exit code: 1\nstderr: snapshot mismatch".to_string()),
            output_digest: None,
            output_json: None,
            metadata: None,
        }),
    ));

    app.active_tab = app::Tab::Run;

    let backend = TestBackend::new(120, 30);
    let mut terminal = Terminal::new(backend).expect("create terminal");
    terminal
        .draw(|frame| ui::render_app(frame, &app))
        .expect("draw shell tool frame");

    let debug = format!("{:?}", terminal.backend().buffer());
    assert!(
        !debug.contains("# Shell"),
        "failed shell calls without structured output should not render a block tool card"
    );
    assert!(
        !debug.contains("● ● ●"),
        "inline shell failures should not render the removed fake terminal header icon"
    );
    assert!(
        !debug.contains("Shell · Failed"),
        "inline shell failures should keep failure state out of the subtitle row"
    );
    assert!(
        debug.contains("cargo test -p harness-tui"),
        "inline shell failures should render the command row"
    );
    assert!(
        debug.contains("stderr: snapshot mismatch"),
        "tool error text should render inline beneath the command"
    );
}

#[test]
fn generic_tool_output_toggle_reveals_block_payload() {
    let mut app = AppState::new_live(None, false, None);

    app.ingest_event(envelope(
        1,
        Some("req_generic_tool"),
        EventV1::ProviderRequestStarted(ProviderRequestStartedEvent {
            request_id: "req_generic_tool".to_string(),
            provider_id: "openai".to_string(),
            model_id: "gpt-5-codex".to_string(),
            prompt_summary: "Inspect generic tool output".to_string(),
            request_digest: "digest-generic-tool".to_string(),
            metadata: None,
        }),
    ));
    app.ingest_event(envelope(
        2,
        Some("req_generic_tool"),
        EventV1::ToolCallRequested(ToolCallRequestedEvent {
            tool_call_id: "tc_generic_tool".to_string(),
            tool_id: "background.cancel".to_string(),
            args_summary: r#"{"taskId":"bg_123"}"#.to_string(),
            args_digest: "digest-generic-tool-args".to_string(),
            metadata: None,
        }),
    ));
    app.ingest_event(envelope(
        3,
        Some("req_generic_tool"),
        EventV1::ToolCallFinished(ToolCallFinishedEvent { tool_call_id: "tc_generic_tool".to_string(),
        status: ToolCallStatus::Succeeded,
        output_summary: Some(
            "cancelled background task\nstatus: complete\ntask: bg_123\nsource: palette\nresult: ok"
                .to_string(),
        ),
        output_digest: Some("digest-generic-tool-output".to_string()),
        output_json: None, metadata: None }),
    ));

    app.active_tab = app::Tab::Run;

    let backend = TestBackend::new(120, 30);
    let mut terminal = Terminal::new(backend).expect("create terminal");
    terminal
        .draw(|frame| ui::render_app(frame, &app))
        .expect("draw generic tool frame");
    let collapsed = format!("{:?}", terminal.backend().buffer());
    assert!(collapsed.contains("background.cancel"));
    assert!(collapsed.contains("[taskId=bg_123]"));
    assert!(!collapsed.contains("cancelled background task"));
    assert!(!collapsed.contains("result: ok"));

    app.handle_key(key_with_modifiers(
        KeyCode::Char('p'),
        KeyModifiers::CONTROL,
    ));
    for c in "show generic tool".chars() {
        app.handle_key(key(KeyCode::Char(c)));
    }
    app.handle_key(key(KeyCode::Enter));

    let backend = TestBackend::new(120, 30);
    let mut terminal = Terminal::new(backend).expect("create terminal");
    terminal
        .draw(|frame| ui::render_app(frame, &app))
        .expect("draw expanded generic tool frame");
    let expanded = format!("{:?}", terminal.backend().buffer());
    assert!(expanded.contains("background.cancel"));
    assert!(expanded.contains("[taskId=bg_123]"));
    assert!(expanded.contains("cancelled background task"));
    assert!(!expanded.contains("result: ok"));

    app.handle_key(key_with_modifiers(
        KeyCode::Char('p'),
        KeyModifiers::CONTROL,
    ));
    for c in "expand turn results".chars() {
        app.handle_key(key(KeyCode::Char(c)));
    }
    app.handle_key(key(KeyCode::Enter));

    let backend = TestBackend::new(120, 30);
    let mut terminal = Terminal::new(backend).expect("create terminal");
    terminal
        .draw(|frame| ui::render_app(frame, &app))
        .expect("draw fully expanded generic tool frame");
    let fully_expanded = format!("{:?}", terminal.backend().buffer());
    assert!(fully_expanded.contains("result: ok"));
}

#[test]
fn task_scheduled_queued_does_not_reuse_tool_call_id_as_task_id() {
    let mut app = AppState::new_live(None, false, None);

    app.ingest_event(envelope(
        1,
        Some("req_001"),
        EventV1::ProviderRequestStarted(ProviderRequestStartedEvent {
            request_id: "req_001".to_string(),
            provider_id: "openai".to_string(),
            model_id: "gpt-5-codex".to_string(),
            prompt_summary: "Hello".to_string(),
            request_digest: "digest-1".to_string(),
            metadata: None,
        }),
    ));

    app.ingest_event(envelope(
        2,
        Some("req_001"),
        EventV1::ToolCallRequested(ToolCallRequestedEvent {
            tool_call_id: "tc_001".to_string(),
            tool_id: "fs.read".to_string(),
            args_summary: r#"{"path":"test.txt"}"#.to_string(),
            args_digest: "digest-args".to_string(),
            metadata: None,
        }),
    ));

    app.ingest_event(envelope(
        3,
        Some("req_001"),
        EventV1::TaskScheduled(TaskScheduledEvent {
            task_id: "tc_001".to_string(),
            state: TaskScheduleState::Queued,
            queue_key: Some("tool:fs.read".to_string()),
        }),
    ));

    let activity = app.activities.front().unwrap();
    let tool_call = activity.tool_calls.first().unwrap();
    assert_eq!(
        tool_call.status,
        crate::app::ToolCallDisplayStatus::Queued,
        "TaskScheduled must not treat task_id as a tool_call_id"
    );

    let rows = app.orchestration_visible_rows();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].task_id, "tc_001");
    assert_eq!(rows[0].state, crate::app::OrchestrationTaskState::Queued);
}

fn key(code: KeyCode) -> KeyEvent {
    KeyEvent::new(code, KeyModifiers::NONE)
}

fn sample_replay_events() -> Vec<EventEnvelopeV1> {
    vec![
        envelope(
            1,
            None,
            EventV1::RunStarted(RunStartedEvent {
                run_name: "replay-run".to_string(),
                workspace_root: "/tmp/workspace".to_string(),
            }),
        ),
        envelope(
            2,
            None,
            EventV1::RunFinished(RunFinishedEvent {
                summary: "done".to_string(),
            }),
        ),
    ]
}

fn sample_live_events() -> Vec<EventEnvelopeV1> {
    vec![
        envelope(
            1,
            None,
            EventV1::RunStarted(RunStartedEvent {
                run_name: "live-run".to_string(),
                workspace_root: "/tmp/workspace".to_string(),
            }),
        ),
        envelope(
            2,
            Some("req_1"),
            EventV1::ProviderRequestStarted(ProviderRequestStartedEvent {
                request_id: "req_1".to_string(),
                provider_id: "mock".to_string(),
                model_id: "model-1".to_string(),
                prompt_summary: "summarized prompt".to_string(),
                request_digest: "digest-req-1".to_string(),
                metadata: None,
            }),
        ),
        envelope(
            3,
            Some("req_1"),
            EventV1::ProviderStreamDelta(ProviderStreamDeltaEvent {
                request_id: "req_1".to_string(),
                delta: "hello ".to_string(),
            }),
        ),
        envelope(
            4,
            Some("req_1"),
            EventV1::ProviderStreamDelta(ProviderStreamDeltaEvent {
                request_id: "req_1".to_string(),
                delta: "world".to_string(),
            }),
        ),
        envelope(
            5,
            Some("req_1"),
            EventV1::ProviderRequestFinished(ProviderRequestFinishedEvent {
                request_id: "req_1".to_string(),
                finish_reason: "stop".to_string(),
                output_digest: Some("digest-output".to_string()),
                usage: None,
                metadata: None,
            }),
        ),
        permission_requested_event(6, "perm_1", "tool_call_1"),
        permission_resolved_event(7, "perm_1", PermissionDecision::Allow),
        envelope(
            8,
            Some("tool_call_1"),
            EventV1::EditApplied(EditAppliedEvent {
                edit_id: "edit_1".to_string(),
                path: "demo.txt".to_string(),
                new_file_digest: "digest-new-file".to_string(),
                diff_rel_path: Some("artifacts/edit-1.diff".to_string()),
                diff_digest: Some("diff-digest".to_string()),
            }),
        ),
        envelope(
            9,
            None,
            EventV1::RunFinished(RunFinishedEvent {
                summary: "done".to_string(),
            }),
        ),
    ]
}

fn sample_tool_spacing_events() -> Vec<EventEnvelopeV1> {
    vec![
        envelope(
            1,
            None,
            EventV1::RunStarted(RunStartedEvent {
                run_name: "tool-spacing".to_string(),
                workspace_root: "/tmp/workspace".to_string(),
            }),
        ),
        envelope(
            2,
            Some("req_spacing"),
            EventV1::UserMessageSubmitted(UserMessageSubmittedEvent {
                request_id: "req_spacing".to_string(),
                text: "Match harness tool spacing".to_string(),
            }),
        ),
        envelope(
            3,
            Some("req_spacing"),
            EventV1::ProviderRequestStarted(ProviderRequestStartedEvent {
                request_id: "req_spacing".to_string(),
                provider_id: "openai".to_string(),
                model_id: "gpt-5-codex".to_string(),
                prompt_summary: "Match harness tool spacing".to_string(),
                request_digest: "digest-spacing".to_string(),
                metadata: None,
            }),
        ),
        envelope(
            4,
            Some("req_spacing"),
            EventV1::ToolCallRequested(ToolCallRequestedEvent {
                tool_call_id: "tc_read_spacing".to_string(),
                tool_id: "fs.read".to_string(),
                args_summary: r#"{"path":"src/ui_transcript.rs"}"#.to_string(),
                args_digest: "digest-read-spacing".to_string(),
                metadata: None,
            }),
        ),
        envelope(
            5,
            Some("req_spacing"),
            EventV1::ToolCallFinished(ToolCallFinishedEvent {
                tool_call_id: "tc_read_spacing".to_string(),
                status: ToolCallStatus::Succeeded,
                output_summary: Some("24 lines read".to_string()),
                output_digest: Some("digest-read-output".to_string()),
                output_json: None,
                metadata: None,
            }),
        ),
        envelope(
            6,
            Some("req_spacing"),
            EventV1::ToolCallRequested(ToolCallRequestedEvent {
                tool_call_id: "tc_glob_spacing".to_string(),
                tool_id: "fs.glob".to_string(),
                args_summary: r#"{"pattern":"src/**/*.rs","path":"."}"#.to_string(),
                args_digest: "digest-glob-spacing".to_string(),
                metadata: None,
            }),
        ),
        envelope(
            7,
            Some("req_spacing"),
            EventV1::ToolCallFinished(ToolCallFinishedEvent {
                tool_call_id: "tc_glob_spacing".to_string(),
                status: ToolCallStatus::Succeeded,
                output_summary: Some("4 files".to_string()),
                output_digest: Some("digest-glob-output".to_string()),
                output_json: None,
                metadata: None,
            }),
        ),
        envelope(
            8,
            Some("req_spacing"),
            EventV1::ToolCallRequested(ToolCallRequestedEvent {
                tool_call_id: "tc_grep_spacing".to_string(),
                tool_id: "fs.grep".to_string(),
                args_summary: r#"{"pattern":"tool spacing","include":"*.rs","path":"src"}"#
                    .to_string(),
                args_digest: "digest-grep-spacing".to_string(),
                metadata: None,
            }),
        ),
        envelope(
            9,
            Some("req_spacing"),
            EventV1::ToolCallFinished(ToolCallFinishedEvent {
                tool_call_id: "tc_grep_spacing".to_string(),
                status: ToolCallStatus::Succeeded,
                output_summary: Some("3 matches".to_string()),
                output_digest: Some("digest-grep-output".to_string()),
                output_json: None,
                metadata: None,
            }),
        ),
        envelope(
            10,
            Some("req_spacing"),
            EventV1::ToolCallRequested(ToolCallRequestedEvent {
                tool_call_id: "tc_list_spacing".to_string(),
                tool_id: "fs.ls".to_string(),
                args_summary: r#"{"path":"src"}"#.to_string(),
                args_digest: "digest-list-spacing".to_string(),
                metadata: None,
            }),
        ),
        envelope(
            11,
            Some("req_spacing"),
            EventV1::ToolCallFinished(ToolCallFinishedEvent {
                tool_call_id: "tc_list_spacing".to_string(),
                status: ToolCallStatus::Succeeded,
                output_summary: Some("ui_transcript.rs\nlayout.rs".to_string()),
                output_digest: Some("digest-list-output".to_string()),
                output_json: None,
                metadata: None,
            }),
        ),
        envelope(
            12,
            Some("req_spacing"),
            EventV1::ToolCallRequested(ToolCallRequestedEvent {
                tool_call_id: "tc_shell_spacing".to_string(),
                tool_id: "shell.run".to_string(),
                args_summary:
                    r#"{"cmd":"cargo test -p harness-tui ui::ui_transcript::","cwd":"/tmp/demo"}"#
                        .to_string(),
                args_digest: "digest-shell-spacing".to_string(),
                metadata: None,
            }),
        ),
        envelope(
            13,
            Some("req_spacing"),
            EventV1::ToolCallStarted(ToolCallStartedEvent {
                tool_call_id: "tc_shell_spacing".to_string(),
            }),
        ),
        envelope(
            14,
            Some("req_spacing"),
            EventV1::ToolCallFinished(ToolCallFinishedEvent {
                tool_call_id: "tc_shell_spacing".to_string(),
                status: ToolCallStatus::Failed,
                output_summary: Some("exit code: 1\nstderr: snapshot mismatch".to_string()),
                output_digest: Some("digest-shell-output".to_string()),
                output_json: None,
                metadata: None,
            }),
        ),
        envelope(
            15,
            Some("req_spacing"),
            EventV1::ProviderReasoningDelta(harness_core::event::ProviderReasoningDeltaEvent {
                request_id: "req_spacing".to_string(),
                delta:
                    "The grouped context rows need to stay compact before the visible shell block."
                        .to_string(),
            }),
        ),
        envelope(
            16,
            Some("req_spacing"),
            EventV1::ProviderStreamDelta(ProviderStreamDeltaEvent {
                request_id: "req_spacing".to_string(),
                delta: "I matched the body-to-tool and tool-to-body spacing to the harness shell."
                    .to_string(),
            }),
        ),
        envelope(
            17,
            Some("req_spacing"),
            EventV1::ProviderRequestFinished(ProviderRequestFinishedEvent {
                request_id: "req_spacing".to_string(),
                finish_reason: "stop".to_string(),
                output_digest: Some("digest-spacing-output".to_string()),
                usage: None,
                metadata: None,
            }),
        ),
        envelope(
            18,
            None,
            EventV1::RunFinished(RunFinishedEvent {
                summary: "done".to_string(),
            }),
        ),
    ]
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
            tool_call_id: Some(tool_call_id.to_string()),
            summary: "Apply hashline edit to demo.txt".to_string(),
            request_digest: "digest-perm".to_string(),
            timeout_ms: 30_000,
            default_decision: harness_core::event::PermissionDecision::Deny,
        }),
    )
}

fn permission_resolved_event(
    seq: u64,
    permission_id: &str,
    decision: PermissionDecision,
) -> EventEnvelopeV1 {
    envelope(
        seq,
        Some("tool_call_1"),
        EventV1::PermissionResolved(PermissionResolvedEvent {
            permission_id: permission_id.to_string(),
            decision: match decision {
                PermissionDecision::Allow => harness_core::event::PermissionDecision::Allow,
                PermissionDecision::Deny => harness_core::event::PermissionDecision::Deny,
            },
            reason: Some("resolved in test".to_string()),
        }),
    )
}

fn envelope(seq: u64, correlation_id: Option<&str>, payload: EventV1) -> EventEnvelopeV1 {
    envelope_with_actor(
        seq,
        correlation_id,
        EventActor::new(ActorKind::System, Some("coordinator".to_string())),
        payload,
    )
}

fn envelope_with_actor(
    seq: u64,
    correlation_id: Option<&str>,
    actor: EventActor,
    payload: EventV1,
) -> EventEnvelopeV1 {
    EventEnvelopeV1 {
        schema_version: SCHEMA_VERSION,
        event_id: format!("evt-{seq:04}"),
        seq,
        run_id: "run_fixture".to_string(),
        mono_ms: seq,
        ts: None,
        actor,
        correlation_id: correlation_id.map(str::to_string),
        causation_id: None,
        stream_key: Some("run:run_fixture".to_string()),
        payload,
    }
}

fn write_replay_fixture(events: Vec<EventEnvelopeV1>) -> TempDir {
    let run_dir = tempfile::tempdir().expect("create temp run dir");
    write_events_jsonl(run_dir.path(), &events);
    run_dir
}

fn write_diff_fixture(with_diff_file: bool) -> TempDir {
    let run_dir = tempfile::tempdir().expect("create temp run dir");

    if with_diff_file {
        let artifacts_dir = run_dir.path().join("artifacts");
        fs::create_dir_all(&artifacts_dir).expect("create artifacts dir");
        fs::write(
            artifacts_dir.join("edit-edit-golden-path.diff"),
            "--- demo.txt\n+++ demo.txt\n@@ -1,3 +1,3 @@\n alpha\n-beta\n+BETA\n gamma\n",
        )
        .expect("write diff fixture");
    }

    let events = vec![
        envelope(
            1,
            None,
            EventV1::RunStarted(RunStartedEvent {
                run_name: "diff-fixture".to_string(),
                workspace_root: "/tmp/workspace".to_string(),
            }),
        ),
        envelope(
            2,
            Some("req_diff_1"),
            EventV1::UserMessageSubmitted(UserMessageSubmittedEvent {
                request_id: "req_diff_1".to_string(),
                text: "Show me the changes inline".to_string(),
            }),
        ),
        envelope(
            3,
            Some("req_diff_1"),
            EventV1::ProviderRequestStarted(ProviderRequestStartedEvent {
                request_id: "req_diff_1".to_string(),
                provider_id: "mock".to_string(),
                model_id: "model-1".to_string(),
                prompt_summary: "Show me the changes inline".to_string(),
                request_digest: "digest-inline-diff-request".to_string(),
                metadata: None,
            }),
        ),
        envelope(
            4,
            Some("req_diff_1"),
            EventV1::ToolCallRequested(ToolCallRequestedEvent {
                tool_call_id: "tool_call_1".to_string(),
                tool_id: "edit.hashline_apply".to_string(),
                args_summary: r#"{"path":"demo.txt"}"#.to_string(),
                args_digest: "digest-inline-diff-tool".to_string(),
                metadata: None,
            }),
        ),
        envelope(
            5,
            Some("tool_call_1"),
            EventV1::ToolCallStarted(ToolCallStartedEvent {
                tool_call_id: "tool_call_1".to_string(),
            }),
        ),
        envelope(
            6,
            Some("tool_call_1"),
            EventV1::EditProposed(EditProposedEvent {
                edit_id: "edit-golden-path".to_string(),
                path: "demo.txt".to_string(),
                summary: "Replace beta with BETA".to_string(),
                patch_digest: "digest-inline-diff-patch".to_string(),
            }),
        ),
        envelope(
            7,
            Some("tool_call_1"),
            EventV1::EditApplied(EditAppliedEvent {
                edit_id: "edit-golden-path".to_string(),
                path: "demo.txt".to_string(),
                new_file_digest: "digest".to_string(),
                diff_rel_path: Some("artifacts/edit-edit-golden-path.diff".to_string()),
                diff_digest: Some("digest-diff".to_string()),
            }),
        ),
        envelope(
            8,
            Some("tool_call_1"),
            EventV1::ToolCallFinished(ToolCallFinishedEvent {
                tool_call_id: "tool_call_1".to_string(),
                status: ToolCallStatus::Succeeded,
                output_summary: Some("Edit applied".to_string()),
                output_digest: Some("digest-inline-diff-output".to_string()),
                output_json: None,
                metadata: None,
            }),
        ),
        envelope(
            9,
            Some("req_diff_1"),
            EventV1::RunFinished(RunFinishedEvent {
                summary: "done".to_string(),
            }),
        ),
    ];

    write_events_jsonl(run_dir.path(), &events);
    run_dir
}

fn write_events_jsonl(run_dir: &Path, events: &[EventEnvelopeV1]) {
    let body = events
        .iter()
        .map(|event| serde_json::to_string(event).expect("serialize event"))
        .collect::<Vec<_>>()
        .join("\n");
    fs::write(run_dir.join("events.jsonl"), format!("{body}\n")).expect("write events jsonl");
}

fn assert_buffer_snapshot(name: &str, buffer: &ratatui::buffer::Buffer) {
    let normalized = normalize_temp_paths(&format!("{buffer:#?}"));
    insta::assert_snapshot!(name, normalized);
}

fn normalize_temp_paths(input: &str) -> String {
    let bytes = input.as_bytes();
    let mut output = String::with_capacity(input.len());
    let mut index = 0;

    while index < bytes.len() {
        if bytes[index..].starts_with(b"/tmp/.tmp") {
            output.push_str("/tmp/TMPDIR");
            index += b"/tmp/.tmp".len();
            while index < bytes.len() && bytes[index].is_ascii_alphanumeric() {
                index += 1;
            }
        } else {
            output.push(bytes[index] as char);
            index += 1;
        }
    }

    output
}
