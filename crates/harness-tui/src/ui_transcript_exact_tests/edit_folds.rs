use super::super::*;

fn edit_section(expanded: bool) -> TranscriptToolCallSection {
    TranscriptToolCallSection {
        tool_call_id: "edit-fold".to_string(),
        child_session_id: None,
        hovered_target: None,
        header: TranscriptToolCallHeader {
            tool_id: "edit".to_string(),
            title: "edit src/lib.rs".to_string(),
            subtitle: None,
            path_metadata: None,
            icon: None,
            status: ToolCallDisplayStatus::Succeeded,
            visual_style: TranscriptToolCallVisualStyle::Block,
            struck_out: false,
            disclosure_state: Some(if expanded {
                TranscriptToolCallDisclosureState::Expanded
            } else {
                TranscriptToolCallDisclosureState::Collapsed
            }),
        },
        detail_blocks: vec![TranscriptToolCallDetailBlock::StructuredDiff {
            diff_content: "--- a/src/lib.rs\n+++ b/src/lib.rs\n@@ -1 +1 @@\n-old\n+new\n"
                .to_string(),
            fallback_path: Some("src/lib.rs".to_string()),
            force_stacked: false,
            plain_numbered: false,
            show_file_header: false,
        }],
        details_collapsed_by_default: true,
        details_preview_visible: false,
        animation_phase: 0,
        expanded,
    }
}

fn render_text(render: &ToolSectionRender) -> String {
    render
        .lines
        .iter()
        .flat_map(|line| line.spans.iter().map(|span| span.content.as_ref()))
        .collect::<String>()
}

#[test]
fn completed_write_with_diff_projects_as_an_edit_row() {
    // Given a completed write whose arguments can produce a structured diff.
    let mut tool_call = transcript_section_model_test_tool_call("write-fold", "fs.write");
    tool_call.status = ToolCallDisplayStatus::Succeeded;
    tool_call.args_summary =
        r#"{"path":"demo.txt","oldContent":"before","content":"after"}"#.to_string();

    // When the transcript section model is built with diff previews available.
    let section = build_transcript_tool_call_section(
        &tool_call,
        &AppState::default(),
        None,
        false,
        false,
        false,
        false,
        Some(std::path::Path::new(".")),
    );

    // Then the collapsed write uses the frozen edit/path vocabulary.
    assert_eq!(section.header.title, "edit demo.txt");
    assert_eq!(
        section.header.visual_style,
        TranscriptToolCallVisualStyle::Block
    );
    assert_eq!(
        section.header.disclosure_state,
        Some(TranscriptToolCallDisclosureState::Collapsed)
    );
}

#[test]
fn collapsed_edit_uses_one_concise_fold_row() {
    // Given an edit with a structured diff whose disclosure starts collapsed.
    let section = edit_section(false);

    // When the tool section is rendered at transcript width.
    let render = append_tool_call_section_lines(&section, &Theme::default(), 120, Color::Reset);
    let text = render_text(&render);

    // Then only the edit/path summary and canonical fold indicator are visible.
    assert_eq!(render.lines.len(), 1, "{text}");
    assert!(text.contains("◆ edit src/lib.rs"), "{text}");
    assert!(text.contains('▸'), "{text}");
    assert!(!text.contains("old") && !text.contains("new"), "{text}");
    let marker = render.lines[0]
        .spans
        .iter()
        .find(|span| span.content.trim() == "◆")
        .expect("collapsed edit row must retain its marker span");
    let edit_label = render.lines[0]
        .spans
        .iter()
        .find(|span| span.content.trim() == "edit")
        .expect("collapsed edit row must split its verb from path metadata");
    assert_eq!(
        marker.style.fg,
        Some(Theme::default().reference_terminal.error)
    );
    assert_eq!(edit_label.style.fg, Some(Theme::default().text.primary));
    assert!(edit_label.style.add_modifier.contains(Modifier::BOLD));
    assert!(render.diff_hunk_offsets.is_empty());
    assert_eq!(render.interaction_rows.len(), render.lines.len());
}

#[test]
fn expanded_edit_preserves_diff_hunks_and_row_alignment() {
    // Given the same edit after its existing disclosure state is expanded.
    let section = edit_section(true);

    // When its detailed diff rows are rendered.
    let render = append_tool_call_section_lines(&section, &Theme::default(), 120, Color::Reset);
    let text = render_text(&render);

    // Then the fold opens without losing diff navigation or interaction-row alignment.
    assert!(text.contains('▾'), "{text}");
    assert!(text.contains("old") && text.contains("new"), "{text}");
    assert!(!render.diff_hunk_offsets.is_empty());
    assert_eq!(render.interaction_rows.len(), render.lines.len());
}
