use super::*;

fn grammar_diff(id: &str, content: &str) -> TranscriptAssistantPart {
    let mut part = grammar_tool(id, "edit", ToolCallPresentationStatus::Succeeded);
    let TranscriptAssistantPart::ToolCall(tool) = &mut part else {
        panic!("tool")
    };
    tool.header.visual_style = TranscriptToolCallVisualStyle::Block;
    tool.header.disclosure_state = Some(TranscriptToolCallDisclosureState::Expanded);
    tool.expanded = true;
    tool.detail_blocks = vec![TranscriptToolCallDetailBlock::StructuredDiff {
        diff_content: content.into(),
        fallback_path: Some("src/界面.rs".into()),
        force_stacked: false,
        plain_numbered: false,
        highlight_syntax: true,
        show_file_header: true,
    }];
    part
}

#[test]
fn grammar_shell_preserves_status_disclosure_and_motion_policy() {
    let mut turn = canonical_turn();
    for (status, expected) in [
        (
            ToolCallPresentationStatus::Queued,
            TranscriptToolStatus::Queued,
        ),
        (
            ToolCallPresentationStatus::Running,
            TranscriptToolStatus::Running,
        ),
        (
            ToolCallPresentationStatus::Failed,
            TranscriptToolStatus::Failed,
        ),
        (
            ToolCallPresentationStatus::Waiting,
            TranscriptToolStatus::Waiting,
        ),
        (
            ToolCallPresentationStatus::Succeeded,
            TranscriptToolStatus::Succeeded,
        ),
        (
            ToolCallPresentationStatus::Cancelled,
            TranscriptToolStatus::Cancelled,
        ),
    ] {
        turn.assistant_parts = vec![grammar_tool("shell", "bash", status)];
        let spec = normalized_tool_spec(&turn, TranscriptToolFamily::Execute);
        let TranscriptBlockContent::Tool { policy, .. } = spec.content else {
            panic!("shell")
        };
        assert_eq!(policy.status, expected);
        assert!(!spec.chrome.rail);
    }
    let mut settled = grammar_tool(
        "shell-finish",
        "bash",
        ToolCallPresentationStatus::Succeeded,
    );
    let TranscriptAssistantPart::ToolCall(tool) = &mut settled else {
        panic!("shell")
    };
    tool.rail_motion = ToolRailMotion::FinishFlash {
        elapsed: std::time::Duration::from_millis(20),
        sampled_phase: 1,
    };
    turn.assistant_parts = vec![settled];
    assert_eq!(
        normalized_tool_spec(&turn, TranscriptToolFamily::Execute).motion,
        TranscriptBlockMotionDemand::None
    );
}

#[test]
fn grammar_diff_preserves_hunk_offsets_at_contract_widths() {
    let theme = Theme::default();
    let mut turn = canonical_turn();
    turn.assistant_parts = vec![grammar_diff(
        "diff",
        "--- src/a.rs\n+++ src/a.rs\n@@ -1 +1 @@\n-old\n+new\n",
    )];
    assert!(
        !normalized_tool_spec(&turn, TranscriptToolFamily::Edit)
            .chrome
            .rail
    );
    for width in [120, 80, 60] {
        let surfaces = build_transcript_render_surfaces(&turn, &theme, width, theme.surface.shell);
        let diff = surfaces
            .iter()
            .find(|surface| surface.kind == TranscriptRenderSurfaceKind::AssistantTool)
            .expect("diff");
        assert!(!diff.diff_hunk_offsets.is_empty());
    }
}

#[test]
fn grammar_malformed_diff_uses_bounded_fallback() {
    let theme = Theme::default();
    let mut turn = canonical_turn();
    turn.assistant_parts = vec![grammar_diff(
        "bad-diff",
        "not a unified diff\n\u{1b}[31mraw",
    )];
    let surfaces = build_transcript_render_surfaces(&turn, &theme, 60, theme.surface.shell);
    assert!(surfaces
        .iter()
        .flat_map(|surface| &surface.lines)
        .all(|line| line.width() <= 60));
}

#[test]
fn grammar_shell_cjk_boundary_remains_grapheme_safe() {
    let theme = Theme::default();
    let mut turn = canonical_turn();
    turn.assistant_parts = vec![grammar_tool(
        "shell-cjk",
        "bash",
        ToolCallPresentationStatus::Running,
    )];
    let TranscriptAssistantPart::ToolCall(tool) = &mut turn.assistant_parts[0] else {
        panic!("tool")
    };
    tool.header.title = "printf '界面🧭' && printf '\u{1b}[31mhuge'".repeat(32);
    let surfaces = build_transcript_render_surfaces(&turn, &theme, 60, theme.surface.shell);
    assert!(surfaces
        .iter()
        .flat_map(|surface| &surface.lines)
        .all(|line| line.width() <= 60));
}
