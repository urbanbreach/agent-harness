use super::*;

#[test]
fn transcript_grammar_synthetic_extension_is_exhaustive_and_resolvable() {
    let mut spec = test_spec(
        TranscriptBlockRole::Synthetic,
        TranscriptBlockContent::Synthetic {
            value: "extension".into(),
        },
    );
    spec.spacing.leading_gap_rows = 1;
    spec.interaction.selectable = true;
    spec.interaction.selected = true;
    assert_eq!(validate_block_spec(&spec), Ok(()));

    let role_name = match spec.role {
        TranscriptBlockRole::UserPrompt => "user",
        TranscriptBlockRole::Reasoning => "reasoning",
        TranscriptBlockRole::AssistantBody => "body",
        TranscriptBlockRole::Tool => "tool",
        TranscriptBlockRole::Error => "error",
        TranscriptBlockRole::Footer => "footer",
        TranscriptBlockRole::Compaction => "compaction",
        TranscriptBlockRole::Synthetic => "synthetic",
    };
    let content_value = match &spec.content {
        TranscriptBlockContent::UserMessage { text, .. }
        | TranscriptBlockContent::AssistantBody { text, .. }
        | TranscriptBlockContent::Reasoning { text, .. } => text.as_str(),
        TranscriptBlockContent::Tool { .. } => "tool",
        TranscriptBlockContent::Footer { .. } => "footer",
        TranscriptBlockContent::Error { message } => message.as_str(),
        TranscriptBlockContent::Compaction { summary, .. } => summary.as_str(),
        TranscriptBlockContent::Synthetic { value } => value.as_str(),
    };
    assert_eq!((role_name, content_value), ("synthetic", "extension"));

    let surface = TranscriptVisualEntryDraft {
        kind: TranscriptRenderSurfaceKind::AssistantBody,
        leading_gap_rows: 0,
        placement: TranscriptBlockPlacement::Flow,
        show_outer_rail: false,
        rail_glyph: " ",
        rail_color: ratatui::style::Color::Reset,
        surface: ratatui::style::Color::Reset,
        lines: vec![ratatui::text::Line::from("extension")],
        interaction_rows: Some(vec![Some(
            super::ui_transcript_interaction::TranscriptInteractionRow {
                target: super::ui_transcript_interaction::TranscriptMouseTarget::Reasoning {
                    request_id: "synthetic".into(),
                },
                hit_start: 0,
                hit_width: 9,
            },
        )]),
        selection_rows: Some(vec![
            super::ui_transcript_selection::TranscriptSelectionRow {
                cells: vec!["extension".into()],
                continues_previous: false,
                copy_offset: 0,
            },
        ]),
        diff_hunk_offsets: Vec::new(),
        selected_rail: false,
        tool_rail_motion: None,
    };
    let resolved = resolve_block_surface(&spec, surface).expect("synthetic surface resolves");
    assert_eq!(resolved.leading_gap_rows, 1);
    assert_eq!(resolved.interaction_rows.as_ref().map(Vec::len), Some(1));
    assert_eq!(resolved.selection_rows.as_ref().map(Vec::len), Some(1));
    let painted = super::ui_transcript_surface::render_transcript_surface_lines(&[resolved]);
    assert_eq!(painted.len(), 2);
    assert_eq!(painted[1].spans[0].content, "extension");
}

#[test]
fn transcript_grammar_all_families_are_exhaustive() {
    let mut turn = canonical_turn();
    turn.show_footer = true;
    let roles = normalize_turn_blocks(&turn)
        .into_iter()
        .map(|spec| spec.role)
        .collect::<Vec<_>>();
    assert!(roles.contains(&TranscriptBlockRole::UserPrompt));
    assert!(roles.contains(&TranscriptBlockRole::Reasoning));
    assert!(roles.contains(&TranscriptBlockRole::AssistantBody));
    assert!(roles.contains(&TranscriptBlockRole::Tool));
    assert!(roles.contains(&TranscriptBlockRole::Error));
    assert!(roles.contains(&TranscriptBlockRole::Footer));
    assert!(roles.contains(&TranscriptBlockRole::Compaction));
}

#[test]
fn transcript_grammar_static_guard_has_one_production_surface_constructor_boundary() {
    let forbidden_mirrors = [
        "assistant_part_needs_leading_gap",
        "transcript_surface_leading_gap",
        "leading_pad_rows",
    ];
    let production_sources = [
        (include_str!("ui_transcript.rs"), "#[cfg(test)]"),
        (include_str!("ui_transcript_layout.rs"), "#[cfg(test)]"),
        (include_str!("ui_transcript_sections.rs"), "#[cfg(test)]"),
        (include_str!("ui_transcript_surface.rs"), ""),
        (include_str!("ui_transcript_types.rs"), "#[cfg(test)]"),
    ];
    for (source, test_marker) in production_sources {
        let production = if test_marker.is_empty() {
            source
        } else {
            source.split(test_marker).next().unwrap_or(source)
        };
        for forbidden in forbidden_mirrors {
            assert!(
                !production.contains(forbidden),
                "legacy helper remains: {forbidden}"
            );
        }
        assert!(
            !production.contains("= TranscriptRenderSurface {")
                && !production.contains("return TranscriptRenderSurface {"),
            "direct surface construction escaped the render/grammar adapter boundary"
        );
    }

    let renderer = include_str!("ui_transcript_render.rs")
        .split("#[cfg(test)]")
        .next()
        .unwrap_or(include_str!("ui_transcript_render.rs"));
    assert!(
        !renderer.contains("TranscriptBlockPlacement::CompatibilityFallback"),
        "approved renderer adapter must construct typed placement"
    );
    assert!(
        !renderer.contains("std::process::abort")
            && !renderer.contains("unwrap_or_abort")
            && !renderer.contains("panic!("),
        "grammar/render boundary must propagate typed errors without panicking"
    );
    assert!(
        !renderer.contains(
            "resolve_compatibility_surfaces(&specs, surfaces)\n        .unwrap_or_default()"
        ) && !renderer.contains("resolve_block_surface(&spec, raw.clone()).unwrap_or(raw)")
            && !renderer.contains("resolve_block_surface(&spec, raw)\n        .unwrap_or_else"),
        "approved renderer adapter must fail closed on grammar errors"
    );
}

#[test]
fn transcript_grammar_invalid_spec_returns_err_without_partial_paint() {
    let turn = canonical_turn();
    let mut specs = normalize_turn_blocks(&turn);
    let body = specs
        .iter_mut()
        .find(|spec| spec.role == TranscriptBlockRole::AssistantBody)
        .expect("body spec");
    body.interaction.selectable = false;
    body.interaction.selected = true;
    let theme = Theme::default();

    let result = super::ui_transcript_render::try_build_transcript_render_surfaces_with_specs(
        &turn,
        &specs,
        &theme,
        80,
        theme.surface.shell,
    );

    assert!(matches!(
        result,
        Err(TranscriptGrammarError::InvalidInteraction)
    ));
}
