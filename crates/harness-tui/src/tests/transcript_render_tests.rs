use super::*;
use crate::UnwrapOrAbort;

pub(super) fn module_transcript_edit_snapshot_renders_inline_diff() {
    harness_core::config::clear_registered_integrations_config();
    harness_core::config::set_registered_lsp_config(harness_core::config::LspConfig::default());

    let run_dir = write_diff_fixture(true);
    let events = load_events_from_run_dir(run_dir.path()).unwrap_or_abort();

    let app = AppState::new_replay(run_dir.path().to_path_buf(), events);

    let backend = TestBackend::new(80, 24);
    let mut terminal = Terminal::new(backend).unwrap_or_abort();
    terminal
        .draw(|frame| ui::render_app(frame, &app))
        .unwrap_or_abort();

    assert_buffer_snapshot(
        "transcript_edit_snapshot_renders_inline_diff",
        terminal.backend().buffer(),
    );
}

pub(super) fn module_inline_diff_does_not_leave_large_gap_before_active_footer() {
    harness_core::config::clear_registered_integrations_config();
    harness_core::config::set_registered_lsp_config(harness_core::config::LspConfig::default());

    let run_dir = write_diff_fixture(true);
    let events = load_events_from_run_dir(run_dir.path()).unwrap_or_abort();
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
        .unwrap_or_abort();
    let footer_row = lines
        .iter()
        .enumerate()
        .skip(diff_last_row + 1)
        .find_map(|(index, line)| {
            let trimmed = line.trim();
            if trimmed.is_empty() {
                return None;
            }
            if trimmed.contains("▪")
                || trimmed.contains("◇")
                || trimmed.contains("gpt")
                || trimmed.contains("model")
                || trimmed.contains("codex")
            {
                Some(index)
            } else {
                None
            }
        })
        .unwrap_or_else(|| panic!("assistant footer after diff\n{lines:#?}"));

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
    let rendered_code_row = rendered
        .lines()
        .find(|row| row.contains("let answer = 42;"))
        .unwrap_or_abort();
    assert!(
        !rendered_code_row.contains('┃'),
        "assistant code should render without a nested frame rail\n{rendered}"
    );
    let (row, colors, backgrounds) =
        row_text_and_palette(&buffer, 120, "let answer = 42;").unwrap_or_abort();
    let start = row.find("let answer = 42;").unwrap_or_abort();
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
    assert!(backgrounds[start..end]
        .iter()
        .all(|background| *background == theme.markdown.code_background));
}

pub(super) fn module_fenced_code_highlighting_falls_back_to_plain_text_when_unknown() {
    let app = transcript_code_block_app("totally-unknown-lang");
    let buffer = render_live_cells(&app, 120, 30);
    let theme = Theme::default();
    let (row, colors, backgrounds) =
        row_text_and_palette(&buffer, 120, "let answer = 42;").unwrap_or_abort();
    let start = row.find("let answer = 42;").unwrap_or_abort();
    let end = start + "let answer = 42;".chars().count();
    let unique = colors[start..end]
        .iter()
        .copied()
        .filter(|color| !matches!(color, ratatui::style::Color::Reset))
        .map(|color| format!("{color:?}"))
        .collect::<std::collections::HashSet<_>>();

    assert_eq!(
        unique,
        std::collections::HashSet::from([format!("{:?}", theme.markdown.text)])
    );
    assert!(backgrounds[start..end]
        .iter()
        .all(|background| *background == theme.markdown.code_background));
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
    let run_dir = tempfile::tempdir().unwrap_or_abort();
    let artifacts_dir = run_dir.path().join("artifacts");
    std::fs::create_dir_all(&artifacts_dir).unwrap_or_abort();
    std::fs::write(
        artifacts_dir.join("harness-inline.diff"),
        "--- crates/harness-tui/src/ui.rs\n+++ crates/harness-tui/src/ui.rs\n@@ -44,8 +44,7 @@\n use ui_secondary::{\n-    render_diff_tab, render_events_tab, render_help_tab,\n+    render_events_tab, render_help_tab, render_live_details_overlay,\n     render_operator_sidebar,\n };\n",
    )
    .unwrap_or_abort();

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
        thinking_first_mono_ms: None,
        thinking_last_mono_ms: None,
        transcript_text: String::new(),
        first_delta_mono_ms: None,
        usage: None,
        cache_usage: None,
        error_message: None,
        permissions: Vec::new(),
        tool_calls: Vec::new(),
        first_seq: 10,
        last_seq: 11,
        first_mono_ms: 10,
        last_mono_ms: 11,
        request_started_mono_ms: None,
        revision: 0,
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

    let (context_row, context_fgs, _) =
        row_text_and_palette(&buffer, 220, "use ui_secondary::{").unwrap_or_abort();
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
        row_text_and_palette(&buffer, 220, "render_live_details_overlay").unwrap_or_abort();
    let removed_start = changed_row.find("render_diff_tab").unwrap_or_abort();
    let added_start = changed_row
        .find("render_live_details_overlay")
        .unwrap_or_abort();
    assert_ne!(
        changed_bgs[removed_start], changed_bgs[added_start],
        "wide transcript inline diff should tint before/after columns independently: {changed_row}"
    );
}

pub(super) fn transcript_edit_snapshot_handles_missing_artifact() {
    let run_dir = write_diff_fixture(false);
    let events = load_events_from_run_dir(run_dir.path()).unwrap_or_abort();

    let app = AppState::new_replay(run_dir.path().to_path_buf(), events);

    let backend = TestBackend::new(80, 24);
    let mut terminal = Terminal::new(backend).unwrap_or_abort();
    terminal
        .draw(|frame| ui::render_app(frame, &app))
        .unwrap_or_abort();

    let debug = format!("{:?}", terminal.backend().buffer());
    assert!(debug.contains("Patch · demo.txt") || debug.contains("◆ Patch"));
    assert!(!debug.contains("Select an edit event to view diff"));
    assert!(!debug.contains("diff artifact missing"));
}

pub(super) fn assistant_markdown_renders_headings_lists_and_quotes() {
    let mut app = AppState::new_live(None, false, None);

    app.ingest_event(envelope(
        1,
        Some("req_markdown"),
        EventV1::ProviderRequestStarted(ProviderRequestStartedEvent {
            request_id: "req_markdown".into(),
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
            request_id: "req_markdown".into(),
            delta: "# Plan\n- [x] Capture transcript parity\n- [ ] Ship final polish\n> Keep the chrome muted".to_string(),
        }),
    ));
    app.ingest_event(envelope(
        3,
        Some("req_markdown"),
        EventV1::ProviderRequestFinished(ProviderRequestFinishedEvent {
            request_id: "req_markdown".into(),
            finish_reason: "stop".to_string(),
            output_digest: Some("digest-markdown-finished".to_string()),
            usage: None,
            metadata: None,
        }),
    ));

    app.active_tab = app::Tab::Run;

    let backend = TestBackend::new(120, 30);
    let mut terminal = Terminal::new(backend).unwrap_or_abort();
    terminal
        .draw(|frame| ui::render_app(frame, &app))
        .unwrap_or_abort();

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

pub(super) fn assistant_markdown_tables_match_reference_top_level_columns() {
    let mut app = AppState::new_live(None, false, None);

    app.ingest_event(envelope(
        1,
        Some("req_markdown_table"),
        EventV1::ProviderRequestStarted(ProviderRequestStartedEvent {
            request_id: "req_markdown_table".into(),
            provider_id: "openai".to_string(),
            model_id: "gpt-5-codex".to_string(),
            prompt_summary: "Render markdown table".to_string(),
            request_digest: "digest-markdown-table".to_string(),
            metadata: None,
        }),
    ));
    app.ingest_event(envelope(
        2,
        Some("req_markdown_table"),
        EventV1::ProviderStreamDelta(ProviderStreamDeltaEvent {
            request_id: "req_markdown_table".into(),
            delta: "| Name | Age |\n|---|---|\n| Alice | 30 |\n| Bob | 5 |".to_string(),
        }),
    ));
    app.ingest_event(envelope(
        3,
        Some("req_markdown_table"),
        EventV1::ProviderRequestFinished(ProviderRequestFinishedEvent {
            request_id: "req_markdown_table".into(),
            finish_reason: "stop".to_string(),
            output_digest: Some("digest-markdown-table-finished".to_string()),
            usage: None,
            metadata: None,
        }),
    ));

    app.active_tab = app::Tab::Run;

    let rendered = render_live_lines(&app, 120, 30);
    assert!(
        rendered.contains("Name   Age"),
        "table header should render as borderless content-width columns\n{rendered}"
    );
    assert!(
        rendered.contains("Alice  30"),
        "table body should align content-width columns\n{rendered}"
    );
    assert!(
        !rendered.contains("| Name | Age |"),
        "markdown table source rows should not render as raw pipe text\n{rendered}"
    );
    assert!(
        !rendered.contains('┌'),
        "top-level markdown tables should be borderless like the reference transcript\n{rendered}"
    );
}

pub(super) fn block_style_tool_rows_render_titles_and_argument_blocks() {
    let mut app = AppState::new_live(None, false, None);

    app.ingest_event(envelope(
        1,
        Some("req_shell"),
        EventV1::ProviderRequestStarted(ProviderRequestStartedEvent {
            request_id: "req_shell".into(),
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
            tool_call_id: "tc_shell".into(),
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
            tool_call_id: "tc_shell".into(),
        }),
    ));
    app.ingest_event(envelope(
        4,
        Some("req_shell"),
        EventV1::ToolCallFinished(ToolCallFinishedEvent {
            tool_call_id: "tc_shell".into(),
            status: ToolCallStatus::Failed,
            output_summary: Some("exit code: 1\nstderr: snapshot mismatch".to_string()),
            output_digest: None,
            output_json: None,
            metadata: None,
        }),
    ));

    app.active_tab = app::Tab::Run;

    let backend = TestBackend::new(120, 30);
    let mut terminal = Terminal::new(backend).unwrap_or_abort();
    terminal
        .draw(|frame| ui::render_app(frame, &app))
        .unwrap_or_abort();

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

pub(super) fn generic_tool_output_toggle_reveals_block_payload() {
    let mut app = AppState::new_live(None, false, None);

    app.ingest_event(envelope(
        1,
        Some("req_generic_tool"),
        EventV1::ProviderRequestStarted(ProviderRequestStartedEvent {
            request_id: "req_generic_tool".into(),
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
            tool_call_id: "tc_generic_tool".into(),
            tool_id: "background.cancel".to_string(),
            args_summary: r#"{"taskId":"bg_123"}"#.to_string(),
            args_digest: "digest-generic-tool-args".to_string(),
            metadata: None,
        }),
    ));
    app.ingest_event(envelope(
        3,
        Some("req_generic_tool"),
        EventV1::ToolCallFinished(ToolCallFinishedEvent { tool_call_id: "tc_generic_tool".into(),
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
    let mut terminal = Terminal::new(backend).unwrap_or_abort();
    terminal
        .draw(|frame| ui::render_app(frame, &app))
        .unwrap_or_abort();
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
    let mut terminal = Terminal::new(backend).unwrap_or_abort();
    terminal
        .draw(|frame| ui::render_app(frame, &app))
        .unwrap_or_abort();
    let expanded = format!("{:?}", terminal.backend().buffer());
    assert!(expanded.contains("background.cancel"));
    assert!(expanded.contains("[taskId=bg_123]"));
    assert!(expanded.contains("cancelled background task"));
    assert!(!expanded.contains("result: ok"));
    assert!(expanded.contains("Click to expand"));

    app::palette_controller::dispatch_palette_command(&mut app, "harness.expand_turn_results");

    let backend = TestBackend::new(120, 30);
    let mut terminal = Terminal::new(backend).unwrap_or_abort();
    terminal
        .draw(|frame| ui::render_app(frame, &app))
        .unwrap_or_abort();
    let fully_expanded = format!("{:?}", terminal.backend().buffer());
    assert!(fully_expanded.contains("result: ok"));
}
