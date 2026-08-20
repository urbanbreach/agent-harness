use super::super::*;
use crate::ui::ui_tool_question_todo::TranscriptTodoStatus;
use crate::UnwrapOrAbort;

#[cfg(test)]
pub(crate) fn exact_test_block_tool_cards_skip_empty_subtitle_rows() {
    let theme = Theme::default();

    let mut shell_call = transcript_section_model_test_tool_call("tc-shell-card", "shell.run");
    shell_call.args_summary =
        r#"{"cmd":"cargo test -p harness-tui","cwd":"/tmp/demo"}"#.to_string();
    shell_call.status = ToolCallDisplayStatus::Failed;
    shell_call.output_summary = Some("exit code: 1\nstderr: snapshot mismatch".to_string());
    shell_call.truncated_output = shell_call.output_summary.clone();
    shell_call.first_mono_ms = 10;
    shell_call.last_mono_ms = 0;

    let section = build_transcript_tool_call_section(
        &shell_call,
        &AppState::default(),
        None,
        false,
        false,
        false,
        false,
        None,
    );
    assert_eq!(section.header.subtitle, Some("Failed".to_string()));
    assert_eq!(
        section.header.visual_style,
        TranscriptToolCallVisualStyle::Block
    );

    let mut lines = Vec::new();
    {
        let render = append_tool_call_section_lines(&section, &theme, 120, theme.surface.panel);
        lines.extend(render.lines);
    }
    let text_lines = transcript_test_line_texts(lines);

    let command_row = text_lines
        .iter()
        .position(|line| line.contains("cargo test -p harness-tui"))
        .unwrap_or_abort();
    let exit_row = text_lines
        .iter()
        .position(|line| line.contains("exit code: 1"))
        .unwrap_or_abort();
    let stderr_row = text_lines
        .iter()
        .position(|line| line.contains("stderr: snapshot mismatch"))
        .unwrap_or_abort();

    assert!(command_row < exit_row && exit_row < stderr_row);
    assert!(
        !text_lines.iter().any(|line| line.contains("# Shell")),
        "failed shell summaries without structured output should stay inline like harness tool rows\n{text_lines:#?}"
    );
    assert!(
        !text_lines.iter().any(|line| line.contains("● ● ●")),
        "block tool rows should not render the removed fake terminal header icon\n{text_lines:#?}"
    );
    assert!(
        !text_lines
            .iter()
            .any(|line| line.contains('╭') || line.contains('╰') || line.contains('│')),
        "block tool rows should not render a rounded window frame\n{text_lines:#?}"
    );
}

#[cfg(test)]
pub(crate) fn failed_tool_cards_parse_legacy_error_copy() {
    let mut tool_call = transcript_section_model_test_tool_call("tc-webfetch-failure", "web.fetch");
    tool_call.status = ToolCallDisplayStatus::Failed;
    tool_call.output_summary = Some(
        "Error: webfetch Request failed: 404 Not Found while fetching https://example.com"
            .to_string(),
    );

    let section = build_transcript_tool_call_section(
        &tool_call,
        &AppState::default(),
        None,
        false,
        false,
        false,
        false,
        None,
    );

    assert_eq!(section.header.subtitle, Some("Request failed".to_string()));
    assert!(section.detail_blocks.iter().any(|block| {
        matches!(
            block,
            TranscriptToolCallDetailBlock::Message { text, tone }
                if *tone == TranscriptToolCallDetailTone::Error
                    && text == "404 Not Found while fetching https://example.com"
        )
    }));
}

#[cfg(test)]
pub(crate) fn failed_tool_cards_normalize_lowercase_error_prefixes_and_tool_separators() {
    let mut tool_call =
        transcript_section_model_test_tool_call("tc-webfetch-lowercase", "web.fetch");
    tool_call.status = ToolCallDisplayStatus::Failed;
    tool_call.output_summary = Some("error: webfetch: Request failed: 404 Not Found".to_string());

    let section = build_transcript_tool_call_section(
        &tool_call,
        &AppState::default(),
        None,
        false,
        false,
        false,
        false,
        None,
    );

    assert_eq!(section.header.subtitle, Some("Request failed".to_string()));
    assert!(section.detail_blocks.iter().any(|block| {
        matches!(
            block,
            TranscriptToolCallDetailBlock::Message { text, tone }
                if *tone == TranscriptToolCallDetailTone::Error && text == "404 Not Found"
        )
    }));
}

#[cfg(test)]
pub(crate) fn denied_tool_cards_use_denied_subtitle() {
    let mut tool_call = transcript_section_model_test_tool_call("tc-denied-failure", "shell.run");
    tool_call.status = ToolCallDisplayStatus::Failed;
    tool_call.permissions = vec![crate::app::PermissionEntry {
        permission_id: "perm-denied".to_string(),
        kind: "shell".to_string(),
        tool_call_id: Some(tool_call.tool_call_id.clone()),
        summary: "Shell execution denied".to_string(),
        request_digest: "digest-denied".to_string(),
        timeout_ms: 30_000,
        default_decision: harness_core::event::PermissionDecision::Deny,
        resolved_decision: Some(harness_core::event::PermissionDecision::Deny),
        resolution_reason: Some("Policy denied shell execution".to_string()),
        first_seq: 1,
        last_seq: 2,
    }];

    let section = build_transcript_tool_call_section(
        &tool_call,
        &AppState::default(),
        None,
        false,
        false,
        false,
        false,
        None,
    );

    assert_eq!(section.header.subtitle, Some("Denied".to_string()));
    assert!(section.detail_blocks.iter().any(|block| {
        matches!(
            block,
            TranscriptToolCallDetailBlock::Message { text: output, tone }
                if *tone == TranscriptToolCallDetailTone::Error
                    && output.contains("Policy denied shell execution")
        )
    }));
}

#[cfg(test)]
pub(crate) fn denied_tool_cards_keep_denied_subtitle_when_reason_contains_colon() {
    let mut tool_call = transcript_section_model_test_tool_call("tc-denied-colon", "shell.run");
    tool_call.status = ToolCallDisplayStatus::Failed;
    tool_call.permissions = vec![crate::app::PermissionEntry {
        permission_id: "perm-denied-colon".to_string(),
        kind: "shell".to_string(),
        tool_call_id: Some(tool_call.tool_call_id.clone()),
        summary: "Shell execution denied".to_string(),
        request_digest: "digest-denied-colon".to_string(),
        timeout_ms: 30_000,
        default_decision: harness_core::event::PermissionDecision::Deny,
        resolved_decision: Some(harness_core::event::PermissionDecision::Deny),
        resolution_reason: Some("Permission denied: shell execution blocked".to_string()),
        first_seq: 1,
        last_seq: 2,
    }];

    let section = build_transcript_tool_call_section(
        &tool_call,
        &AppState::default(),
        None,
        false,
        false,
        false,
        false,
        None,
    );

    assert_eq!(section.header.subtitle, Some("Denied".to_string()));
    assert!(section.detail_blocks.iter().any(|block| {
        matches!(
            block,
            TranscriptToolCallDetailBlock::Message { text: output, tone }
                if *tone == TranscriptToolCallDetailTone::Error
                    && output.contains("shell execution blocked")
        )
    }));
}

#[cfg(test)]
pub(crate) fn generic_failed_tool_messages_do_not_split_arbitrary_prefixes() {
    let mut tool_call =
        transcript_section_model_test_tool_call("tc-generic-url-failure", "vendor.remote");
    tool_call.status = ToolCallDisplayStatus::Failed;
    tool_call.output_summary =
        Some("Error: GET https://example.com: connection refused".to_string());

    let section = build_transcript_tool_call_section(
        &tool_call,
        &AppState::default(),
        None,
        false,
        false,
        false,
        false,
        None,
    );

    assert_eq!(section.header.subtitle, Some("Failed".to_string()));
    assert!(section.detail_blocks.iter().any(|block| {
        matches!(
            block,
            TranscriptToolCallDetailBlock::Message { text, tone }
                if *tone == TranscriptToolCallDetailTone::Error
                    && text == "GET https://example.com: connection refused"
        )
    }));
}

#[cfg(test)]
pub(crate) fn failed_tool_cards_fallback_when_error_details_are_missing() {
    let mut tool_call = transcript_section_model_test_tool_call("tc-empty-failure", "tool.batch");
    tool_call.status = ToolCallDisplayStatus::Failed;

    let section = build_transcript_tool_call_section(
        &tool_call,
        &AppState::default(),
        None,
        false,
        false,
        false,
        false,
        None,
    );

    assert_eq!(section.header.subtitle, Some("Failed".to_string()));
    assert!(section.detail_blocks.iter().any(|block| {
        matches!(
            block,
            TranscriptToolCallDetailBlock::Message { text, tone }
                if *tone == TranscriptToolCallDetailTone::Error
                    && text == "No error details available."
        )
    }));
}

#[cfg(test)]
#[test]
fn question_tool_cards_render_answered_question_details() {
    // arrange
    // act
    // assert
    let mut tool_call =
        transcript_section_model_test_tool_call("tc-question-answered", "user.question");
    tool_call.status = ToolCallDisplayStatus::Succeeded;
    tool_call.permissions = vec![crate::app::PermissionEntry {
        permission_id: "perm-question-answered".to_string(),
        kind: "question".to_string(),
        tool_call_id: Some(tool_call.tool_call_id.clone()),
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
        request_digest: "digest-question-answered".to_string(),
        timeout_ms: 30_000,
        default_decision: harness_core::event::PermissionDecision::Deny,
        resolved_decision: Some(harness_core::event::PermissionDecision::Allow),
        resolution_reason: Some("[[\"A\"],[]]".to_string()),
        first_seq: 1,
        last_seq: 2,
    }];

    let section = build_transcript_tool_call_section(
        &tool_call,
        &AppState::default(),
        None,
        false,
        false,
        false,
        false,
        None,
    );

    assert_eq!(section.header.icon, Some("→"));
    assert_eq!(section.header.title, "Asked 2 questions");
    assert_eq!(section.header.subtitle, None);
    assert_eq!(
        section.header.visual_style,
        TranscriptToolCallVisualStyle::Inline
    );
    assert_eq!(
        section.detail_blocks,
        vec![TranscriptToolCallDetailBlock::Message {
            text: "1. Pick one\n    → A\n2. Pick another\n    → (no answer)".to_string(),
            tone: TranscriptToolCallDetailTone::Primary,
        }]
    );
}

#[cfg(test)]
#[test]
fn question_tool_cards_render_pending_ask_title() {
    // arrange
    // act
    // assert
    let mut tool_call =
        transcript_section_model_test_tool_call("tc-question-pending", "user.question");
    tool_call.status = ToolCallDisplayStatus::PendingPermission;
    tool_call.args_summary = serde_json::json!({
        "questions": [
            {
                "question": "Which color?",
                "header": "Color",
                "options": [{"label": "Red", "description": "Choose red"}],
            }
        ]
    })
    .to_string();

    let section = build_transcript_tool_call_section(
        &tool_call,
        &AppState::default(),
        None,
        false,
        false,
        false,
        false,
        None,
    );

    assert_eq!(section.header.title, "Ask Which color?");
    assert_eq!(
        section.header.presentation.status,
        crate::app::ToolCallPresentationStatus::Waiting
    );
}

#[cfg(test)]
#[test]
fn batch_write_edit_and_patch_rows_match_reference_headers() {
    // arrange
    let mut batch_call = transcript_section_model_test_tool_call("tc-batch-row", "tool.batch");
    batch_call.status = ToolCallDisplayStatus::Succeeded;
    batch_call.args_summary = serde_json::json!({
        "tool_calls": [
            {"tool_id": "fs.read"},
            {"tool_id": "fs.grep"}
        ]
    })
    .to_string();
    // act
    let batch_section = build_transcript_tool_call_section(
        &batch_call,
        &AppState::default(),
        None,
        false,
        false,
        false,
        false,
        None,
    );
    // assert
    assert_eq!(batch_section.header.icon, Some("#"));
    assert_eq!(batch_section.header.title, "Batch 2 tools");
    assert_eq!(batch_section.header.subtitle, None);
    assert_eq!(
        batch_section.header.visual_style,
        TranscriptToolCallVisualStyle::Block
    );

    let mut write_call = transcript_section_model_test_tool_call("tc-write-row", "fs.write");
    write_call.status = ToolCallDisplayStatus::Succeeded;
    write_call.args_summary = r#"{"filePath":"src/main.rs","content":"fn main() {}"}"#.to_string();
    let write_section = build_transcript_tool_call_section(
        &write_call,
        &AppState::default(),
        None,
        false,
        false,
        false,
        false,
        None,
    );
    assert_eq!(write_section.header.icon, Some("←"));
    assert_eq!(write_section.header.title, "Edit main.rs");
    assert_eq!(write_section.header.subtitle, None);

    let mut edit_call = transcript_section_model_test_tool_call("tc-edit-row", "edit");
    edit_call.status = ToolCallDisplayStatus::Succeeded;
    edit_call.args_summary =
        r#"{"filePath":"src/main.rs","oldString":"old","newString":"new"}"#.to_string();
    let edit_section = build_transcript_tool_call_section(
        &edit_call,
        &AppState::default(),
        None,
        false,
        false,
        false,
        false,
        None,
    );
    assert_eq!(edit_section.header.icon, Some("←"));
    assert_eq!(edit_section.header.title, "Edit main.rs");
    assert_eq!(edit_section.header.subtitle, None);

    let mut patch_call = transcript_section_model_test_tool_call("tc-patch-row", "apply_patch");
    patch_call.status = ToolCallDisplayStatus::Succeeded;
    patch_call.output_json = Some(serde_json::json!({
        "files": ["M src/main.rs", "M src/lib.rs"]
    }));
    let patch_section = build_transcript_tool_call_section(
        &patch_call,
        &AppState::default(),
        None,
        false,
        false,
        false,
        false,
        None,
    );
    assert_eq!(patch_section.header.icon, Some("%"));
    assert_eq!(patch_section.header.title, "Patch 2 files");
    assert_eq!(patch_section.header.subtitle, None);
}

#[test]
fn consecutive_tool_rows_form_a_zero_gap_activity_run() {
    // arrange
    // act
    // assert
    let mut app = AppState::default();
    let mut activity =
        transcript_section_model_test_activity("request-tool-stacking", ActivityStatus::Done, "");

    let mut command_tool =
        transcript_section_model_test_tool_call("tc-command-stacking", "shell.run");
    command_tool.status = ToolCallDisplayStatus::Succeeded;
    command_tool.args_summary = r#"{"cmd":"pwd"}"#.to_string();
    command_tool.first_mono_ms = 10;
    command_tool.last_mono_ms = 20;

    let mut lsp_tool = transcript_section_model_test_tool_call("tc-lsp-stacking", "code.lsp");
    lsp_tool.status = ToolCallDisplayStatus::Succeeded;
    lsp_tool.args_summary = r#"{"operation":"goto_definition","symbol":"AppState"}"#.to_string();
    lsp_tool.first_mono_ms = 30;
    lsp_tool.last_mono_ms = 45;

    activity.tool_calls = vec![command_tool, lsp_tool];
    app.activities = std::collections::VecDeque::from(vec![activity]);
    app.transcript_view.selected_activity_index = 0;

    let layout = build_measured_transcript_layout_for_width(&app, &Theme::default(), 120);
    let surfaces = &layout.sections[0].surfaces;
    assert_eq!(surfaces.len(), 2);
    let command_surface = surfaces.first().unwrap_or_abort();
    let context_surface = surfaces.get(1).unwrap_or_abort();

    assert_eq!(
        context_surface.top_offset,
        command_surface.top_offset + command_surface.height,
        "consecutive tool surfaces should remain visually grouped"
    );
}

#[test]
fn block_tool_cards_render_subtitle_inline_with_title() {
    // arrange
    // act
    // assert
    let theme = Theme::default();
    let section = TranscriptToolCallSection {
        tool_call_id: "tool-agent-spawn".to_string(),
        coalesced_tool_call_ids: vec!["tool-agent-spawn".to_string()],
        child_session_id: None,
        subagent_background: false,
        output_truncated: false,
        replay_read_only: false,
        hovered_target: None,
        header: TranscriptToolCallHeader {
            tool_id: "agent.spawn".to_string(),
            title: "Spawn researcher · audit transcript parity".to_string(),
            subtitle: Some("14:36 · 1.6s".to_string()),
            path_metadata: None,
            icon: Some("↗"),
            presentation: crate::app::ToolCallPresentation::from_display_status(
                ToolCallDisplayStatus::Succeeded,
            ),
            visual_style: TranscriptToolCallVisualStyle::Block,
            struck_out: false,
            disclosure_state: None,
        },
        detail_blocks: vec![TranscriptToolCallDetailBlock::Message {
            text: "┃ agent_worker · req_child · completed · 2 child tool calls".to_string(),
            tone: TranscriptToolCallDetailTone::Secondary,
        }],
        details_collapsed_by_default: false,
        details_preview_visible: false,
        animation_phase: 0,
        expanded: false,
        rail_motion: ToolRailMotion::Settled,
    };

    let mut lines = Vec::new();
    {
        let render = append_tool_call_section_lines(&section, &theme, 120, theme.surface.panel);
        lines.extend(render.lines);
    }
    let text_lines = transcript_test_line_texts(lines);

    let title_row = text_lines
        .iter()
        .position(|line| line.contains("Spawn researcher · audit transcript parity · 14:36 · 1.6s"))
        .unwrap_or_abort();

    assert!(
        text_lines[title_row].contains("Spawn researcher · audit transcript parity · 14:36 · 1.6s"),
        "block tool card title row should keep subtitle metadata inline\n{text_lines:#?}"
    );
    assert!(
        !text_lines
            .iter()
            .enumerate()
            .any(|(index, line)| index != title_row && line.contains("14:36 · 1.6s")),
        "block tool cards should not dedicate a separate subtitle row\n{text_lines:#?}"
    );
}

#[test]
fn running_and_finished_shell_tools_keep_one_command_header_identity() {
    // arrange
    // Given: one shell tool before and after output arrives.
    let theme = Theme::default();
    let mut running = transcript_section_model_test_tool_call("tc-shell-stable", "shell.run");
    running.args_summary = r#"{"command":"echo hi"}"#.to_string();
    running.status = ToolCallDisplayStatus::Running;
    let running_section = build_transcript_tool_call_section(
        &running,
        &AppState::default(),
        None,
        false,
        false,
        false,
        false,
        None,
    );

    let mut finished = running;
    finished.status = ToolCallDisplayStatus::Succeeded;
    finished.output_summary = Some("hi".to_string());
    let finished_section = build_transcript_tool_call_section(
        &finished,
        &AppState::default(),
        None,
        false,
        false,
        false,
        false,
        None,
    );

    // When: both lifecycle states are rendered through the transcript surface.
    let running_lines = transcript_test_line_texts(
        append_tool_call_section_lines(&running_section, &theme, 96, theme.surface.panel).lines,
    );
    let finished_lines = transcript_test_line_texts(
        append_tool_call_section_lines(&finished_section, &theme, 96, theme.surface.panel).lines,
    );
    let running_row = running_lines
        .iter()
        .position(|line| line.contains("echo hi"))
        .unwrap_or_abort();
    let finished_row = finished_lines
        .iter()
        .position(|line| line.contains("echo hi"))
        .unwrap_or_abort();

    // act
    // Then: output appends below one stable `Run` header instead of replacing
    // an inline command row with a differently shaped completed block.
    // assert
    assert_eq!(
        running_section.header.visual_style,
        TranscriptToolCallVisualStyle::Block
    );
    assert_eq!(
        finished_section.header.visual_style,
        TranscriptToolCallVisualStyle::Block
    );
    assert!(running_lines[running_row].contains("◆ Run echo hi"));
    assert!(finished_lines[finished_row].contains("◆ Run echo hi"));
    assert_eq!(running_row, finished_row);
    assert_eq!(
        running_lines[running_row].find("◆ Run"),
        finished_lines[finished_row].find("◆ Run")
    );
}

#[test]
fn finished_shell_output_stays_collapsed_until_disclosed() {
    // arrange
    // Given: a finished command with useful output.
    let theme = Theme::default();
    let mut tool_call = transcript_section_model_test_tool_call("tc-shell-collapsed", "shell.run");
    tool_call.args_summary = r#"{"command":"echo hi"}"#.to_string();
    tool_call.status = ToolCallDisplayStatus::Succeeded;
    tool_call.output_summary = Some("line 1\nline 2".to_string());

    // When: the transcript builds the normal finished presentation.
    let section = build_transcript_tool_call_section(
        &tool_call,
        &AppState::default(),
        None,
        false,
        false,
        false,
        false,
        None,
    );
    let rendered = transcript_test_line_texts(
        append_tool_call_section_lines(&section, &theme, 96, theme.surface.panel).lines,
    )
    .join("\n");

    // act
    // Then: the row is flat and its result remains behind explicit disclosure.
    // assert
    assert!(section.details_collapsed_by_default);
    assert_eq!(
        section.header.disclosure_state,
        Some(TranscriptToolCallDisclosureState::Collapsed)
    );
    assert!(rendered.contains("◆ Run echo hi"), "{rendered}");
    assert!(!rendered.contains("line 1"), "{rendered}");
    assert!(!rendered.contains("Click to"), "{rendered}");
}

#[test]
fn expanded_shell_tool_renders_output_with_clamping() {
    // arrange
    let theme = Theme::default();
    let mut tool_call = transcript_section_model_test_tool_call("tc-shell-harness", "shell.run");
    tool_call.args_summary = r#"{"command":"echo hi","description":"list files"}"#.to_string();
    tool_call.status = ToolCallDisplayStatus::Succeeded;
    tool_call.output_summary = Some(
        (1..=20)
            .map(|line| format!("line {line}"))
            .collect::<Vec<_>>()
            .join("\n"),
    );

    // act
    let section = build_transcript_tool_call_section(
        &tool_call,
        &AppState::default(),
        None,
        false,
        false,
        true,
        false,
        None,
    );
    // assert - output is clamped to 15 lines with expand hint
    assert_eq!(
        section.detail_blocks[0],
        TranscriptToolCallDetailBlock::BashPanel {
            command: "echo hi".to_string(),
            output: (1..=20)
                .map(|line| format!("line {line}"))
                .collect::<Vec<_>>()
                .join("\n"),
            description: None,
            expand_hint: Some("Click to collapse".to_string()),
            tone: TranscriptToolCallDetailTone::Primary,
        }
    );

    let text_lines = transcript_test_line_texts({
        let mut lines = Vec::new();
        let render = append_tool_call_section_lines(&section, &theme, 96, theme.surface.panel);
        lines.extend(render.lines);
        lines
    });
    let rendered = text_lines.join("\n");

    assert!(
        rendered.contains('◈') || rendered.contains('◆'),
        "harness bash panel should have flat tool header (◈ completed / ◆ active)\n{text_lines:#?}"
    );
    assert!(
        !rendered.contains('┃'),
        "harness bash panel should not have split rail chrome\n{text_lines:#?}"
    );
    assert!(
        !rendered.contains("# Shell"),
        "harness bash panels should not render a fallback title without a workdir description\n{text_lines:#?}"
    );
    assert!(rendered.contains("◆ Run echo hi"));
    assert!(!rendered.contains("stdout>"));
    assert!(rendered.contains("line 20"));
    assert!(rendered.contains("Click to collapse"));
}

#[test]
fn shell_tool_cards_without_workdir_start_with_command_row() {
    // arrange
    // act
    // assert
    let mut tool_call = transcript_section_model_test_tool_call("tc-shell-no-workdir", "bash");
    tool_call.args_summary = r#"{"command":"cargo test -p harness-tui"}"#.to_string();
    tool_call.status = ToolCallDisplayStatus::Succeeded;
    tool_call.output_summary = Some("ok".to_string());

    let section = build_transcript_tool_call_section(
        &tool_call,
        &AppState::default(),
        None,
        false,
        false,
        false,
        false,
        None,
    );
    let text_lines = transcript_test_line_texts({
        let render = append_tool_call_section_lines(
            &section,
            &Theme::default(),
            96,
            Theme::default().surface.panel,
        );
        render.lines
    });

    assert!(
        !text_lines.iter().any(|line| line.contains("# Shell")),
        "no fallback shell title row should be rendered\n{text_lines:#?}"
    );
    let command_row = text_lines
        .iter()
        .position(|line| line.contains("◆ Run cargo test -p harness-tui"))
        .unwrap_or_abort();
    let preceding_content = text_lines[..command_row]
        .iter()
        .filter(|line| line.contains("# "))
        .collect::<Vec<_>>();
    assert!(
        preceding_content.is_empty(),
        "no title row should precede the command row when no workdir description exists\n{text_lines:#?}"
    );
}

#[test]
fn todo_write_cards_parse_metadata_and_output_todos() {
    // arrange
    // act
    // assert
    let mut metadata_call =
        transcript_section_model_test_tool_call("tc-todo-metadata", "todowrite");
    metadata_call.status = ToolCallDisplayStatus::Succeeded;
    metadata_call.output_json = Some(serde_json::json!({
        "title": "0 todos",
        "metadata": {
            "todos": [
                {"content": "Ship transcript parity", "status": "completed"},
                {"content": "Capture visual QA", "status": "in_progress"}
            ]
        },
        "output": "[]"
    }));

    let metadata_section = build_transcript_tool_call_section(
        &metadata_call,
        &AppState::default(),
        None,
        false,
        false,
        false,
        false,
        None,
    );
    assert!(matches!(
        &metadata_section.detail_blocks[0],
        TranscriptToolCallDetailBlock::TodoList { items }
            if items.len() == 2
                && items[0].content == "Ship transcript parity"
                && items[0].status == TranscriptTodoStatus::Completed
                && items[1].status == TranscriptTodoStatus::InProgress
    ));

    let mut output_call = transcript_section_model_test_tool_call("tc-todo-output", "todowrite");
    output_call.status = ToolCallDisplayStatus::Succeeded;
    output_call.output_summary =
        Some(r#"[{"content":"Render completed box","status":"completed"}]"#.to_string());

    let output_section = build_transcript_tool_call_section(
        &output_call,
        &AppState::default(),
        None,
        false,
        false,
        false,
        false,
        None,
    );
    assert!(matches!(
        &output_section.detail_blocks[0],
        TranscriptToolCallDetailBlock::TodoList { items }
            if items.len() == 1
                && items[0].content == "Render completed box"
                && items[0].status == TranscriptTodoStatus::Completed
    ));
}

#[test]
fn shell_tool_cards_render_workdir_as_reference_running_prefix() {
    // arrange
    let mut tool_call = transcript_section_model_test_tool_call("tc-shell-workdir", "bash");
    tool_call.args_summary =
        r#"{"command":"pwd","description":"show cwd","workdir":"crates/harness-tui"}"#.to_string();
    tool_call.status = ToolCallDisplayStatus::Succeeded;
    tool_call.output_summary = Some("/workspace/crates/harness-tui".to_string());

    // act
    let section = build_transcript_tool_call_section(
        &tool_call,
        &AppState::default(),
        None,
        false,
        false,
        false,
        false,
        Some(Path::new("/workspace")),
    );

    // assert
    assert!(matches!(
        &section.detail_blocks[0],
        TranscriptToolCallDetailBlock::BashPanel { description, .. }
            if description.as_deref() == Some("# Running in /workspace/crates/harness-tui")
    ));
    let rendered = transcript_test_line_texts({
        let render = append_tool_call_section_lines(
            &section,
            &Theme::default(),
            96,
            Theme::default().surface.panel,
        );
        render.lines
    })
    .join("\n");
    assert!(rendered.contains("# Running in /workspace/crates/harness-tui"));
    assert!(rendered.contains("◆ Run pwd"));
    assert!(!rendered.contains("# show cwd"));
}

#[test]
fn shell_tool_cards_render_cmd_with_args_and_structured_output() {
    // arrange
    // act
    // assert
    let mut tool_call = transcript_section_model_test_tool_call("tc-shell-cmd-args", "shell.run");
    tool_call.args_summary =
        r#"{"cmd":"bash","args":["-lc","printf shell-run"],"cwd":"."}"#.to_string();
    tool_call.status = ToolCallDisplayStatus::Succeeded;
    tool_call.output_summary = None;
    tool_call.output_json = Some(serde_json::json!({
        "stdout": "shell-run\n",
        "stderr": "",
        "status": 0,
        "success": true,
        "truncated": false
    }));

    let section = build_transcript_tool_call_section(
        &tool_call,
        &AppState::default(),
        None,
        false,
        false,
        false,
        false,
        Some(Path::new("/workspace")),
    );

    assert_eq!(
        section.header.visual_style,
        TranscriptToolCallVisualStyle::Block
    );
    assert!(matches!(
        &section.detail_blocks[0],
        TranscriptToolCallDetailBlock::BashPanel { command, output, .. }
            if command == "bash -lc printf shell-run" && output == "shell-run"
    ));
}

#[test]
fn failed_structured_shell_output_does_not_duplicate_matching_error_summary() {
    // arrange
    // act
    // assert
    let mut tool_call = transcript_section_model_test_tool_call("tc-shell-structured-fail", "bash");
    tool_call.args_summary = r#"{"command":"false"}"#.to_string();
    tool_call.status = ToolCallDisplayStatus::Failed;
    tool_call.output_summary = Some("boom".to_string());
    tool_call.output_json = Some(serde_json::json!({
        "stdout": "",
        "stderr": "boom",
        "status": 1,
        "success": false,
        "truncated": false
    }));

    let section = build_transcript_tool_call_section(
        &tool_call,
        &AppState::default(),
        None,
        false,
        false,
        false,
        false,
        None,
    );

    assert_eq!(section.detail_blocks.len(), 1);
    assert!(matches!(
        &section.detail_blocks[0],
        TranscriptToolCallDetailBlock::BashPanel { output, tone, .. }
            if output == "boom" && *tone == TranscriptToolCallDetailTone::Error
    ));
}

#[test]
fn collapsed_failed_shell_omits_redundant_command_failed_copy() {
    // arrange
    // Given: a failed command whose only result repeats its lifecycle state.
    let theme = Theme::default();
    let mut tool_call = transcript_section_model_test_tool_call("tc-shell-generic-fail", "bash");
    tool_call.args_summary = r#"{"command":"false"}"#.to_string();
    tool_call.status = ToolCallDisplayStatus::Failed;
    tool_call.output_summary = Some("command failed".to_string());

    // When: the normal collapsed row is rendered.
    let section = build_transcript_tool_call_section(
        &tool_call,
        &AppState::default(),
        None,
        false,
        false,
        false,
        false,
        None,
    );
    let rendered = transcript_test_line_texts(
        append_tool_call_section_lines(&section, &theme, 96, theme.surface.panel).lines,
    );

    // act
    // Then: color and the Failed subtitle carry the state without a duplicate paragraph.
    // assert
    assert!(rendered.iter().any(|line| line.contains("Failed")));
    assert!(!rendered
        .iter()
        .any(|line| line.trim().eq_ignore_ascii_case("command failed")));
}

#[test]
fn shell_tool_cards_strip_ansi_from_output() {
    // arrange
    // act
    // assert
    let mut tool_call = transcript_section_model_test_tool_call("tc-shell-ansi", "bash");
    tool_call.args_summary = r#"{"command":"printf color"}"#.to_string();
    tool_call.status = ToolCallDisplayStatus::Succeeded;
    tool_call.output_summary = Some("\u{1b}[31mred\u{1b}[0m\nplain".to_string());

    let section = build_transcript_tool_call_section(
        &tool_call,
        &AppState::default(),
        None,
        false,
        false,
        false,
        false,
        None,
    );

    assert!(matches!(
        &section.detail_blocks[0],
        TranscriptToolCallDetailBlock::BashPanel { output, .. }
            if output == "red\nplain"
    ));
}

#[test]
fn shell_tool_aliases_use_canonical_bash_card_path() {
    // arrange
    // act
    // assert
    let mut tool_call =
        transcript_section_model_test_tool_call("tc-shell-alias", "shell.run.wrapper");
    tool_call.resolved_tool_identity = Some(harness_core::event::ResolvedToolIdentity {
        invoked_tool_id: Some("shell.run.wrapper".to_string()),
        effective_tool_id: Some("bash".to_string()),
        canonical_tool_id: Some("bash".to_string()),
        alias_source_tool_id: Some("shell.run".to_string()),
    });
    tool_call.args_summary = r#"{"command":"echo alias"}"#.to_string();
    tool_call.status = ToolCallDisplayStatus::Succeeded;
    tool_call.output_summary = Some("alias".to_string());

    let section = build_transcript_tool_call_section(
        &tool_call,
        &AppState::default(),
        None,
        false,
        false,
        false,
        false,
        None,
    );
    assert_eq!(section.header.tool_id, "bash");
    assert_eq!(
        section.header.visual_style,
        TranscriptToolCallVisualStyle::Block
    );
    assert!(matches!(
        section.detail_blocks.first(),
        Some(TranscriptToolCallDetailBlock::BashPanel { .. })
    ));
}

#[test]
fn completed_empty_shell_output_keeps_block_card() {
    // arrange
    // act
    // assert
    let mut tool_call = transcript_section_model_test_tool_call("tc-shell-empty", "bash");
    tool_call.args_summary = r#"{"command":"true"}"#.to_string();
    tool_call.status = ToolCallDisplayStatus::Succeeded;
    tool_call.output_json = Some(serde_json::json!({
        "stdout": "",
        "stderr": "",
        "status": 0,
        "success": true,
        "truncated": false
    }));

    let section = build_transcript_tool_call_section(
        &tool_call,
        &AppState::default(),
        None,
        false,
        false,
        false,
        false,
        None,
    );

    assert_eq!(
        section.header.visual_style,
        TranscriptToolCallVisualStyle::Block
    );
    assert!(matches!(
        &section.detail_blocks[0],
        TranscriptToolCallDetailBlock::BashPanel { command, output, expand_hint, .. }
            if command == "true" && output.is_empty() && expand_hint.is_none()
    ));
}

#[test]
fn running_shell_without_output_metadata_uses_stable_command_surface() {
    // arrange
    // act
    // assert
    let mut tool_call = transcript_section_model_test_tool_call("tc-shell-running-empty", "bash");
    tool_call.args_summary = r#"{"command":"sleep 1"}"#.to_string();
    tool_call.status = ToolCallDisplayStatus::Running;

    let section = build_transcript_tool_call_section(
        &tool_call,
        &AppState::default(),
        None,
        false,
        false,
        false,
        false,
        None,
    );

    assert_eq!(
        section.header.visual_style,
        TranscriptToolCallVisualStyle::Block
    );
    assert_eq!(section.header.icon, None);
    assert_eq!(section.header.title, "Run sleep 1");
    assert!(section.detail_blocks.is_empty());
}

#[test]
fn shell_tool_cards_render_full_overflow_without_expand_hints() {
    // arrange
    let mut tool_call = transcript_section_model_test_tool_call("tc-shell-overflow", "bash");
    tool_call.args_summary = r#"{"command":"seq 12"}"#.to_string();
    tool_call.status = ToolCallDisplayStatus::Succeeded;
    tool_call.output_summary = Some(
        (1..=20)
            .map(|line| format!("line {line}"))
            .collect::<Vec<_>>()
            .join("\n"),
    );

    // act
    let collapsed = build_transcript_tool_call_section(
        &tool_call,
        &AppState::default(),
        None,
        false,
        false,
        false,
        false,
        None,
    );
    // assert - output is clamped to 15 lines with expand hint
    assert!(matches!(
        &collapsed.detail_blocks[0],
        TranscriptToolCallDetailBlock::BashPanel { output, expand_hint, .. }
            if !output.contains("line 16") && expand_hint.is_some()
    ));
}

#[cfg(test)]
pub(crate) fn exact_test_inline_tool_rows_wrap_long_subtitles_cleanly() {
    let theme = Theme::default();
    let section = TranscriptToolCallSection {
        tool_call_id: "tool-inline-read".to_string(),
        coalesced_tool_call_ids: vec!["tool-inline-read".to_string()],
        child_session_id: None,
        subagent_background: false,
        output_truncated: false,
        replay_read_only: false,
        hovered_target: None,
        header: TranscriptToolCallHeader {
            tool_id: "fs.read".to_string(),
            title: "Read src/ui.rs [offset=12, limit=24]".to_string(),
            subtitle: Some(
                "14:35 · 1.2s · foreground · agent_worker · req_child · completed · 3 child tool calls"
                    .to_string(),
            ),
            path_metadata: None,
            icon: None,
            presentation: crate::app::ToolCallPresentation::from_display_status(
                ToolCallDisplayStatus::Succeeded,
            ),
            visual_style: TranscriptToolCallVisualStyle::Inline,
            struck_out: false,
            disclosure_state: None,
        },
        detail_blocks: Vec::new(),
        details_collapsed_by_default: false,
        details_preview_visible: false,
        animation_phase: 0,
        expanded: false,
        rail_motion: ToolRailMotion::Settled,
    };

    let mut lines = Vec::new();
    {
        let render = append_tool_call_section_lines(&section, &theme, 56, theme.surface.panel);
        lines.extend(render.lines);
    }
    let text_lines = transcript_test_line_texts(lines);

    assert!(
        text_lines.len() >= 2,
        "long inline subtitles should wrap in narrow widths"
    );
    assert!(text_lines[0].contains("Read src/ui.rs [offset=12, limit=24]"));
    assert!(text_lines.iter().any(|line| line.contains("14:35 · 1.2s")));
    assert!(text_lines
        .iter()
        .any(|line| line.contains("foreground · agent_worker")));
    assert!(
        text_lines.iter().any(|line| line.contains("completed")),
        "wrapped inline subtitle should preserve completion count metadata\n{text_lines:#?}"
    );
    assert!(
        text_lines
            .iter()
            .any(|line| line.contains("child tool calls")),
        "wrapped inline subtitle should preserve child-call wording after wrap\n{text_lines:#?}"
    );
    assert!(
        text_lines[0].contains('◈') || text_lines[0].contains('◆'),
        "inline tool header should keep the completed/active tool marker\n{text_lines:#?}"
    );
}
