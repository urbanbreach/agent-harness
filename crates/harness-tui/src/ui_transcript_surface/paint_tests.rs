use super::render_transcript_surface;
use crate::theme::{GlyphMode, Theme};
use crate::ui::ui_transcript::{
    ToolRailMotion, TranscriptBlockPlacement, TranscriptRenderSurfaceKind,
    TranscriptVisualEntryDisplayMode, TranscriptVisualEntryHitRegion,
    TranscriptVisualEntryMetadata,
};
use crate::ui::ui_transcript_layout::TranscriptVisualEntry;
use ratatui::{
    backend::TestBackend,
    layout::{Alignment, Rect},
    style::Style,
    text::{Line, Span},
    Terminal,
};
use std::time::Duration;

fn surface(theme: &Theme) -> TranscriptVisualEntry {
    TranscriptVisualEntry {
        source_text: None,
        rendered_text: std::sync::Arc::from(""),
        metadata: TranscriptVisualEntryMetadata::settled(
            0,
            0,
            TranscriptVisualEntryDisplayMode::Flow,
        ),
        kind: TranscriptRenderSurfaceKind::AssistantReasoning,
        leading_gap_rows: 0,
        placement: TranscriptBlockPlacement::Flow,
        top_offset: 0,
        height: 1,
        width: 80,
        show_outer_rail: true,
        rail_glyph: "┃",
        rail_color: theme.text.tertiary,
        surface: theme.surface.canvas,
        lines: vec![Line::from(vec![
            Span::raw("  "),
            Span::styled(
                format!("{} ", theme.live_shell.transcript_glyphs.tool_marker),
                Style::default().fg(theme.text.tertiary),
            ),
            Span::styled(
                "Working 界 e\u{301} 👩‍💻 #\u{fe0f}",
                Style::default().fg(theme.text.secondary),
            ),
        ])],
        interaction_rows: None,
        selection_rows: Vec::new(),
        semantic_selection: false,
        diff_hunk_offsets: Vec::new(),
        selected_rail: false,
        tool_rail_motion: Some(ToolRailMotion::Running {
            elapsed: Duration::ZERO,
            sampled_phase: 0,
        }),
        hit_region: TranscriptVisualEntryHitRegion::new(0, 80, 1),
    }
}

#[test]
fn cached_transcript_paint_preserves_motion_and_terminal_cells() {
    for mode in [GlyphMode::Preferred, GlyphMode::Ascii] {
        let theme = Theme::default().with_glyph_mode(mode);
        for kind in [
            TranscriptRenderSurfaceKind::User,
            TranscriptRenderSurfaceKind::AssistantFooter,
            TranscriptRenderSurfaceKind::AssistantReasoning,
            TranscriptRenderSurfaceKind::AssistantTool,
        ] {
            let mut surface = surface(&theme);
            surface.kind = kind;
            surface.show_outer_rail = kind == TranscriptRenderSurfaceKind::AssistantReasoning;
            if matches!(
                kind,
                TranscriptRenderSurfaceKind::User | TranscriptRenderSurfaceKind::AssistantFooter
            ) {
                surface.tool_rail_motion = None;
            }
            let mut terminal = Terminal::new(TestBackend::new(40, 2)).expect("terminal");
            let frames = [0, 10].map(|phase| {
                terminal
                    .draw(|frame| {
                        render_transcript_surface(
                            frame,
                            &surface,
                            Rect::new(0, 0, 40, 2),
                            0,
                            phase,
                            &theme,
                            false,
                        )
                    })
                    .expect("paint");
                terminal.backend().buffer().clone()
            });
            assert_ne!(
                frames[0][(2, 0)].fg,
                frames[1][(2, 0)].fg,
                "{mode:?} {kind:?}"
            );
            for (index, (before, after)) in
                frames[0].content.iter().zip(&frames[1].content).enumerate()
            {
                assert_eq!(
                    before.symbol(),
                    after.symbol(),
                    "{mode:?} {kind:?} at {index}"
                );
                if (4..40).contains(&index) {
                    assert_eq!(before, after, "label or padding changed at {index}");
                }
            }
            assert_eq!(frames[1][(4, 0)].fg, theme.text.secondary);
            if surface.show_outer_rail {
                assert_eq!(frames[1][(0, 0)].symbol(), "┃");
                assert_eq!(frames[1][(0, 1)].symbol(), " ");
            }
        }
    }

    let theme = Theme::default();
    let mut surface = surface(&theme);
    surface.kind = TranscriptRenderSurfaceKind::AssistantBody;
    surface.tool_rail_motion = None;
    surface.show_outer_rail = false;
    for (text, columns) in [
        ("ｶﾞX", [2, 3, 4]),
        ("ｶﾟX", [2, 3, 4]),
        ("ﾞX", [1, 2, 4]),
        ("ﾟX", [1, 2, 4]),
    ] {
        for split in [false, true] {
            for (alignment, column) in [Alignment::Left, Alignment::Center, Alignment::Right]
                .into_iter()
                .zip(columns)
            {
                let spans = if split {
                    text.chars().map(|ch| Span::raw(ch.to_string())).collect()
                } else {
                    vec![Span::raw(text)]
                };
                surface.lines = vec![Line::from(spans).alignment(alignment)];
                let mut terminal = Terminal::new(TestBackend::new(5, 1)).expect("terminal");
                terminal
                    .draw(|frame| {
                        render_transcript_surface(
                            frame,
                            &surface,
                            Rect::new(0, 0, 5, 1),
                            0,
                            0,
                            &theme,
                            false,
                        )
                    })
                    .expect("paint");
                assert_eq!(
                    terminal.backend().buffer()[(column, 0)].symbol(),
                    "X",
                    "{text} split={split} {alignment:?}"
                );
            }
        }
    }
}
