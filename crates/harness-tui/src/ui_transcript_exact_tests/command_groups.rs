use super::super::*;

fn command_tool_call(
    tool_call_id: &str,
    command: &str,
    status: ToolCallDisplayStatus,
) -> TranscriptToolCallSection {
    let failed = status == ToolCallDisplayStatus::Failed;
    TranscriptToolCallSection {
        tool_call_id: tool_call_id.to_string(),
        child_session_id: None,
        hovered_target: None,
        header: TranscriptToolCallHeader {
            tool_id: "shell.run".to_string(),
            title: "Shell".to_string(),
            subtitle: None,
            path_metadata: None,
            icon: None,
            status,
            visual_style: TranscriptToolCallVisualStyle::Block,
            struck_out: false,
            disclosure_state: Some(TranscriptToolCallDisclosureState::Collapsed),
        },
        detail_blocks: vec![TranscriptToolCallDetailBlock::BashPanel {
            command: command.to_string(),
            output: if failed {
                "command failed".to_string()
            } else {
                "command succeeded".to_string()
            },
            description: None,
            expand_hint: None,
            tone: if failed {
                TranscriptToolCallDetailTone::Error
            } else {
                TranscriptToolCallDetailTone::Primary
            },
        }],
        details_collapsed_by_default: true,
        details_preview_visible: false,
        animation_phase: 0,
        expanded: false,
    }
}

fn grouped_command_surface_with_expansion(expanded: bool) -> (TranscriptRenderSurface, Theme) {
    let mut succeeded = command_tool_call(
        "command-success",
        "echo tx-tool-output-probe-line",
        ToolCallDisplayStatus::Succeeded,
    );
    let mut failed = command_tool_call(
        "command-failed",
        "echo tx-tool-output-probe-line",
        ToolCallDisplayStatus::Failed,
    );
    if expanded {
        for tool_call in [&mut succeeded, &mut failed] {
            tool_call.expanded = true;
            tool_call.header.disclosure_state = Some(TranscriptToolCallDisclosureState::Expanded);
        }
    }
    let turn = TranscriptTurnSection {
        request_id: "request-command-group".to_string(),
        user_message: None,
        show_footer: false,
        footer_timestamp: None,
        animation_phase: 0,
        header: TranscriptTurnHeader {
            status: ActivityStatus::Done,
            is_selected: false,
            provider_request_open: false,
            profile_label: "default".to_string(),
            model_id: "model".to_string(),
            duration_ms: None,
            thinking_duration_ms: None,
            responding_duration_ms: None,
            total_tokens: None,
            retry: None,
            retry_elapsed_ms: None,
        },
        body_blocks: Vec::new(),
        tool_calls: vec![succeeded.clone(), failed.clone()],
        thinking: None,
        error: None,
        assistant_parts: vec![
            TranscriptAssistantPart::ToolCall(Box::new(succeeded)),
            TranscriptAssistantPart::ToolCall(Box::new(failed)),
        ],
    };
    let theme = Theme::default();
    let surface = build_transcript_render_surfaces(&turn, &theme, 120, Color::Reset)
        .into_iter()
        .find(|surface| surface.kind == TranscriptRenderSurfaceKind::AssistantTool)
        .expect("completed adjacent commands must render as one tool-group surface");
    (surface, theme)
}

fn grouped_command_surface() -> (TranscriptRenderSurface, Theme) {
    grouped_command_surface_with_expansion(false)
}

fn visible_line_text(line: &Line<'_>) -> String {
    line.spans
        .iter()
        .map(|span| span.content.as_ref())
        .collect::<String>()
        .trim_end()
        .to_string()
}

#[test]
fn grouped_commands_match_the_dense_frozen_rows_when_collapsed() {
    // Given a completed group containing one successful and one failed command.
    let (surface, _) = grouped_command_surface();

    // When its collapsed transcript rows are flattened to visible terminal text.
    let rows = surface
        .lines
        .iter()
        .map(visible_line_text)
        .collect::<Vec<_>>();

    // Then the aggregate counts completed commands separately and no failure body is duplicated.
    assert_eq!(
        rows,
        vec![
            "┃  ◈ Ran 1 command · 1 failed",
            "┃  ◆ Run echo tx-tool-output-probe-line",
            "┃  ◆ Run echo tx-tool-output-probe-line",
        ]
    );
}

#[test]
fn grouped_command_members_use_the_group_failure_accent() {
    // Given a completed command group with a failed member.
    let (surface, _) = grouped_command_surface();

    // When the rail and diamond span colors are inspected for every member row.
    let member_accents = surface.lines[1..]
        .iter()
        .map(|line| {
            (
                line.spans.first().and_then(|span| span.style.fg),
                line.spans.get(1).and_then(|span| span.style.fg),
            )
        })
        .collect::<Vec<_>>();

    // Then the failed group carries one consistent error rail through all dense members.
    assert_eq!(
        member_accents,
        vec![
            (Some(Color::Rgb(239, 41, 41)), Some(Color::Rgb(239, 41, 41)),),
            (Some(Color::Rgb(239, 41, 41)), Some(Color::Rgb(239, 41, 41)),),
        ]
    );
}

#[test]
fn grouped_command_labels_match_the_exact_reference_style() {
    // Given the frozen failed command-group presentation.
    let (surface, _) = grouped_command_surface();

    // When the failure suffix and command-label spans are inspected.
    let failure_suffix = surface
        .lines
        .iter()
        .flat_map(|line| line.spans.iter())
        .find(|span| span.content.contains("failed"))
        .expect("group summary must contain the failure suffix");
    let run_label = surface
        .lines
        .iter()
        .flat_map(|line| line.spans.iter())
        .find(|span| span.content.trim() == "Run")
        .expect("command member must split the Run label from its command");

    // Then the ANSI-native error and bright bold label match the frozen source theme.
    assert_eq!(failure_suffix.style.fg, Some(Color::Rgb(239, 41, 41)));
    assert_eq!(
        run_label.style.fg,
        Some(Theme::default().reference_terminal.secondary)
    );
    assert!(run_label.style.add_modifier.contains(Modifier::BOLD));
}

#[test]
fn grouped_command_interactions_stay_aligned_with_dense_rows() {
    // Given the dense grouped command surface.
    let (surface, _) = grouped_command_surface();

    // When its interaction rows are paired with visible rows.
    let interaction_rows = surface
        .interaction_rows
        .expect("grouped commands must expose interaction rows");

    // Then only the aggregate row is interactive and row counts remain one-to-one.
    assert_eq!(interaction_rows.len(), surface.lines.len());
    assert!(matches!(
        interaction_rows.first().and_then(Option::as_ref),
        Some(TranscriptInteractionRow {
            target: TranscriptMouseTarget::ToolGroup { tool_call_ids },
            ..
        }) if tool_call_ids == &["command-success", "command-failed"]
    ));
    assert!(interaction_rows[1..].iter().all(Option::is_none));
}

#[test]
fn grouped_command_expansion_preserves_details_and_row_alignment() {
    // Given the same command group after its existing disclosure state is expanded.
    let (surface, _) = grouped_command_surface_with_expansion(true);

    // When the expanded rows and their interaction mapping are inspected.
    let rendered = surface
        .lines
        .iter()
        .map(visible_line_text)
        .collect::<Vec<_>>()
        .join("\n");
    let interaction_rows = surface
        .interaction_rows
        .expect("expanded command groups must retain interaction rows");

    // Then unchanged command details return and measured interaction rows stay aligned.
    assert!(rendered.contains("command succeeded"), "{rendered}");
    assert!(rendered.contains("command failed"), "{rendered}");
    assert_eq!(interaction_rows.len(), surface.lines.len());
}
