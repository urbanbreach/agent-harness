use super::*;

fn grammar_compaction_part(
    kind: TranscriptCompactionKind,
    summary: &str,
) -> TranscriptAssistantPart {
    TranscriptAssistantPart::Compaction(TranscriptCompactionSection {
        kind,
        summary: summary.into(),
        tokens_before: Some(12_345),
        read_files: vec!["src/界面.rs".into()],
        modified_files: vec!["src/🧭.rs".into()],
    })
}

#[test]
fn grammar_compaction_policy_and_compact_widths() {
    let mut turn = canonical_turn();
    turn.assistant_parts = vec![grammar_compaction_part(
        TranscriptCompactionKind::SessionCompaction,
        &"要約🧭".repeat(64),
    )];
    let spec = normalize_turn_blocks(&turn)
        .into_iter()
        .find(|spec| spec.role == TranscriptBlockRole::Compaction)
        .expect("compaction");
    let TranscriptBlockContent::Compaction {
        tokens_before,
        read_files,
        modified_files,
        ..
    } = &spec.content
    else {
        panic!("compaction content")
    };
    assert_eq!(*tokens_before, Some(12_345));
    assert_eq!(read_files, &["src/界面.rs"]);
    assert_eq!(modified_files, &["src/🧭.rs"]);
    assert!(spec.chrome.accent);
    assert!(spec.fold.foldable);
    assert!(!spec.fold.expanded);
    assert_eq!(spec.motion, TranscriptBlockMotionDemand::None);
    let theme = Theme::default();
    for width in [120, 80, 60] {
        assert!(
            build_transcript_render_surfaces(&turn, &theme, width, theme.surface.shell)
                .iter()
                .flat_map(|surface| &surface.lines)
                .all(|line| line.width() <= width as usize)
        );
    }
}

#[test]
fn grammar_branch_summary_preserves_order_and_anchor() {
    let mut turn = canonical_turn();
    turn.assistant_parts = vec![
        TranscriptAssistantPart::Body(TranscriptBodyBlock::RichText("before".into())),
        grammar_compaction_part(TranscriptCompactionKind::BranchSummary, "branch summary"),
        grammar_tool("after", "read", ToolCallPresentationStatus::Succeeded),
    ];
    let specs = normalize_turn_blocks(&turn);
    let roles = specs.iter().map(|spec| spec.role).collect::<Vec<_>>();
    let index = roles
        .iter()
        .position(|role| *role == TranscriptBlockRole::Compaction)
        .expect("compaction role");
    assert_eq!(roles[index - 1], TranscriptBlockRole::AssistantBody);
    assert_eq!(roles[index + 1], TranscriptBlockRole::Tool);
    let id = specs[index].id.clone();
    let TranscriptAssistantPart::Compaction(compaction) = &mut turn.assistant_parts[1] else {
        panic!("compaction")
    };
    compaction.summary.push_str(" updated");
    assert_eq!(normalize_turn_blocks(&turn)[index].id, id);
}

#[test]
fn grammar_compaction_empty_has_no_orphan_disclosure_row() {
    let mut turn = canonical_turn();
    turn.show_footer = false;
    turn.assistant_parts = vec![TranscriptAssistantPart::Compaction(
        TranscriptCompactionSection {
            kind: TranscriptCompactionKind::SessionCompaction,
            summary: String::new(),
            tokens_before: None,
            read_files: Vec::new(),
            modified_files: Vec::new(),
        },
    )];
    let theme = Theme::default();
    let surface = build_transcript_render_surfaces(&turn, &theme, 60, theme.surface.shell)
        .into_iter()
        .find(|surface| surface.kind == TranscriptRenderSurfaceKind::Compaction)
        .expect("compaction surface");
    assert_eq!(surface.lines.len(), 1);
    assert!(surface.interaction_rows.is_none());
}

#[test]
fn grammar_compaction_invalid_policy_is_rejected() {
    let mut turn = canonical_turn();
    turn.assistant_parts = vec![grammar_compaction_part(
        TranscriptCompactionKind::SessionCompaction,
        "summary",
    )];
    let mut spec = normalize_turn_blocks(&turn)
        .into_iter()
        .find(|spec| spec.role == TranscriptBlockRole::Compaction)
        .expect("compaction");
    spec.disclosure.available = false;
    spec.disclosure.expanded = true;
    assert_eq!(
        validate_block_spec(&spec),
        Err(TranscriptGrammarError::InvalidDisclosure)
    );
}
