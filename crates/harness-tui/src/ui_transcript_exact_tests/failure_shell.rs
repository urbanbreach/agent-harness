use super::super::*;

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
    assert_eq!(section.header.subtitle, None);
    assert_eq!(
        section.header.visual_style,
        TranscriptToolCallVisualStyle::Inline
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
        .expect("shell inline command");
    let exit_row = text_lines
        .iter()
        .position(|line| line.contains("exit code: 1"))
        .expect("shell inline error exit");
    let stderr_row = text_lines
        .iter()
        .position(|line| line.contains("stderr: snapshot mismatch"))
        .expect("shell inline error stderr");

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

    assert_eq!(section.header.title, "Questions");
    assert_eq!(
        section.header.subtitle,
        Some("answered 2 questions".to_string())
    );
    assert!(section.detail_blocks.iter().any(|block| {
        matches!(
            block,
            TranscriptToolCallDetailBlock::Message { text, tone }
                if *tone == TranscriptToolCallDetailTone::Primary && text == "Pick one"
        )
    }));
    assert!(section.detail_blocks.iter().any(|block| {
        matches!(
            block,
            TranscriptToolCallDetailBlock::Message { text, tone }
                if *tone == TranscriptToolCallDetailTone::Secondary && text == "↳ A"
        )
    }));
    assert!(section.detail_blocks.iter().any(|block| {
        matches!(
            block,
            TranscriptToolCallDetailBlock::Message { text, tone }
                if *tone == TranscriptToolCallDetailTone::Secondary
                    && text == "↳ (no answer)"
        )
    }));
}

#[test]
fn consecutive_tool_rows_do_not_insert_terminal_blank_rows() {
    let mut app = AppState::default();
    let mut activity =
        transcript_section_model_test_activity("request-tool-stacking", ActivityStatus::Done, "");

    let mut cancel_tool =
        transcript_section_model_test_tool_call("tc-cancel-stacking", "background.cancel");
    cancel_tool.status = ToolCallDisplayStatus::Succeeded;
    cancel_tool.args_summary = r#"{"taskId":"bg_123"}"#.to_string();
    cancel_tool.first_mono_ms = 10;
    cancel_tool.last_mono_ms = 20;

    let mut lsp_tool = transcript_section_model_test_tool_call("tc-lsp-stacking", "code.lsp");
    lsp_tool.status = ToolCallDisplayStatus::Succeeded;
    lsp_tool.args_summary = r#"{"operation":"goto_definition","symbol":"AppState"}"#.to_string();
    lsp_tool.first_mono_ms = 30;
    lsp_tool.last_mono_ms = 45;

    activity.tool_calls = vec![cancel_tool, lsp_tool];
    app.activities = std::collections::VecDeque::from(vec![activity]);
    app.transcript_view.selected_activity_index = 0;

    let layout = build_measured_transcript_layout_for_width(&app, &Theme::default(), 120);
    let surfaces = &layout.sections[0].surfaces;

    let cancel_surface = surfaces
        .iter()
        .find(|surface| {
            surface.lines.iter().any(|line| {
                line.spans
                    .iter()
                    .any(|span| span.content.as_ref().contains("background.cancel"))
            })
        })
        .expect("background.cancel surface");
    let lsp_surface = surfaces
        .iter()
        .find(|surface| {
            surface.lines.iter().any(|line| {
                line.spans
                    .iter()
                    .any(|span| span.content.as_ref().contains("lsp"))
            })
        })
        .expect("lsp surface");

    assert_eq!(
        lsp_surface.top_offset,
        cancel_surface.top_offset + cancel_surface.height,
        "consecutive tool surfaces should stack without an extra blank terminal row"
    );
}

#[test]
fn block_tool_cards_render_subtitle_inline_with_title() {
    let theme = Theme::default();
    let section = TranscriptToolCallSection {
        tool_call_id: "tool-agent-spawn".to_string(),
        child_session_id: None,
        hovered_target: None,
        header: TranscriptToolCallHeader {
            tool_id: "agent.spawn".to_string(),
            title: "Spawn researcher · audit transcript parity".to_string(),
            subtitle: Some("14:36 · 1.6s".to_string()),
            path_metadata: None,
            icon: Some("↗"),
            status: ToolCallDisplayStatus::Succeeded,
            visual_style: TranscriptToolCallVisualStyle::Block,
            struck_out: false,
            disclosure_state: None,
        },
        detail_blocks: vec![TranscriptToolCallDetailBlock::Message {
            text: "┃ agent_worker · req_child · completed · 2 child tool calls".to_string(),
            tone: TranscriptToolCallDetailTone::Secondary,
        }],
        expanded: false,
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
        .expect("block tool title row");

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
fn shell_tool_cards_use_harness_bash_styling_values() {
    let theme = Theme::default();
    let mut tool_call = transcript_section_model_test_tool_call("tc-shell-harness", "shell.run");
    tool_call.args_summary = r#"{"command":"echo hi","description":"list files"}"#.to_string();
    tool_call.status = ToolCallDisplayStatus::Succeeded;
    tool_call.output_summary = Some(
        (1..=11)
            .map(|line| format!("line {line}"))
            .collect::<Vec<_>>()
            .join("\n"),
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
    assert_eq!(
        section.detail_blocks[0],
        TranscriptToolCallDetailBlock::BashPanel {
            command: "echo hi".to_string(),
            output:
                "line 1\nline 2\nline 3\nline 4\nline 5\nline 6\nline 7\nline 8\nline 9\nline 10\n…"
                    .to_string(),
            description: Some("list files".to_string()),
            expand_hint: Some("Click to expand".to_string()),
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

    assert!(rendered.contains('┃'));
    assert!(!rendered.contains("● ● ●"));
    assert!(rendered.contains("# list files"));
    assert!(!rendered.contains('╭'));
    assert!(!rendered.contains('├'));
    assert!(rendered.contains("$ echo hi"));
    assert!(!rendered.contains("stdout>"));
    assert!(rendered.contains("line 10"));
    assert!(!rendered.contains("line 11"));
    assert!(rendered.contains("Click to expand"));
    assert!(!rendered.contains('╰'));
}

#[test]
fn shell_tool_cards_render_workdir_in_harness_title() {
    let mut tool_call = transcript_section_model_test_tool_call("tc-shell-workdir", "bash");
    tool_call.args_summary =
        r#"{"command":"pwd","description":"show cwd","workdir":"crates/harness-tui"}"#.to_string();
    tool_call.status = ToolCallDisplayStatus::Succeeded;
    tool_call.output_summary = Some("/workspace/crates/harness-tui".to_string());

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

    assert!(matches!(
        &section.detail_blocks[0],
        TranscriptToolCallDetailBlock::BashPanel { description, .. }
            if description.as_deref() == Some("show cwd in /workspace/crates/harness-tui")
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
    assert!(rendered.contains("# show cwd in /workspace/crates/harness-tui"));
}

#[test]
fn shell_tool_cards_render_cmd_with_args_and_structured_output() {
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
            if output == "boom" && *tone == TranscriptToolCallDetailTone::Primary
    ));
}

#[test]
fn shell_tool_cards_strip_ansi_from_output() {
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
fn running_shell_without_output_metadata_uses_inline_fallback_until_output_event() {
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

    // The harness transcript model has no running-output presence bit while a
    // shell call is running without output. Until a result event carries
    // `output_summary` or structured stdout/stderr, the closest deterministic
    // equivalent is the harness inline shell row.
    assert_eq!(
        section.header.visual_style,
        TranscriptToolCallVisualStyle::Inline
    );
    assert_eq!(section.header.icon, Some("$"));
    assert!(section.detail_blocks.is_empty());
}

#[test]
fn shell_tool_cards_toggle_overflow_expand_and_collapse_hints() {
    let mut tool_call = transcript_section_model_test_tool_call("tc-shell-overflow", "bash");
    tool_call.args_summary = r#"{"command":"seq 12"}"#.to_string();
    tool_call.status = ToolCallDisplayStatus::Succeeded;
    tool_call.output_summary = Some(
        (1..=12)
            .map(|line| format!("line {line}"))
            .collect::<Vec<_>>()
            .join("\n"),
    );

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
    let expanded = build_transcript_tool_call_section(
        &tool_call,
        &AppState::default(),
        None,
        false,
        false,
        true,
        false,
        None,
    );

    assert!(matches!(
        &collapsed.detail_blocks[0],
        TranscriptToolCallDetailBlock::BashPanel { output, expand_hint, .. }
            if output.ends_with("line 10\n…")
                && !output.contains("line 11")
                && expand_hint.as_deref() == Some("Click to expand")
    ));
    assert!(matches!(
        &expanded.detail_blocks[0],
        TranscriptToolCallDetailBlock::BashPanel { output, expand_hint, .. }
            if output.contains("line 12")
                && expand_hint.as_deref() == Some("Click to collapse")
    ));
}

#[cfg(test)]
pub(crate) fn exact_test_inline_tool_rows_wrap_long_subtitles_cleanly() {
    let theme = Theme::default();
    let section = TranscriptToolCallSection {
        tool_call_id: "tool-inline-read".to_string(),
        child_session_id: None,
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
            status: ToolCallDisplayStatus::Succeeded,
            visual_style: TranscriptToolCallVisualStyle::Inline,
            struck_out: false,
            disclosure_state: None,
        },
        detail_blocks: Vec::new(),
        expanded: false,
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
        text_lines.iter().any(|line| line.contains("completed · 3")),
        "wrapped inline subtitle should preserve completion count metadata\n{text_lines:#?}"
    );
    assert!(
        text_lines
            .iter()
            .any(|line| line.contains("child tool calls")),
        "wrapped inline subtitle should preserve child-call wording after wrap\n{text_lines:#?}"
    );
    assert!(text_lines.iter().all(|line| line.starts_with("   ")));
}
