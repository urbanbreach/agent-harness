use super::render_transcript_surface;
use crate::theme::{GlyphMode, Theme};
use crate::ui::ui_transcript::{
    ToolRailMotion, TranscriptBlockPlacement, TranscriptRenderSurfaceKind,
    TranscriptVisualEntryDisplayMode, TranscriptVisualEntryHitRegion, TranscriptVisualEntryMetadata,
};
use crate::ui::ui_transcript_layout::TranscriptVisualEntry;
use ratatui::{backend::TestBackend, layout::Rect, style::Style, text::{Line, Span}, Terminal};
use std::time::Duration;

fn surface(theme: &Theme) -> TranscriptVisualEntry {
    TranscriptVisualEntry {
        source_text: None,
        rendered_text: std::sync::Arc::from(""),
        metadata: TranscriptVisualEntryMetadata::settled(0, 0, TranscriptVisualEntryDisplayMode::Flow),
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
            Span::styled(format!("{} ", theme.live_shell.transcript_glyphs.tool_marker), Style::default().fg(theme.text.tertiary)),
            Span::styled("Working 界 e\u{301} 👩‍💻 #\u{fe0f}", Style::default().fg(theme.text.secondary)),
        ])],
        interaction_rows: None,
        selection_rows: Vec::new(),
        semantic_selection: false,
        diff_hunk_offsets: Vec::new(),
        selected_rail: false,
        tool_rail_motion: Some(ToolRailMotion::Running { elapsed: Duration::ZERO, sampled_phase: 0 }),
        hit_region: TranscriptVisualEntryHitRegion::new(0, 80, 1),
    }
}


fn record(records: &mut Vec<serde_json::Value>, s: &TranscriptVisualEntry, theme: &Theme, width: u16, scroll: usize, phase: usize) {
    let mut terminal = Terminal::new(TestBackend::new(44, 5)).expect("terminal");
    terminal.draw(|frame| {
        for cell in &mut frame.buffer_mut().content {
            cell.set_symbol("x").set_style(Style::default().fg(ratatui::style::Color::Red).bg(ratatui::style::Color::Blue).add_modifier(ratatui::style::Modifier::BOLD));
        }
        render_transcript_surface(frame, s, Rect::new(2, 1, width, 3), scroll, phase, theme);
    }).expect("paint");
    let buffer = terminal.backend().buffer();
    records.push(serde_json::json!({"width": width, "scroll": scroll, "phase": phase,
        "cells": buffer.content.iter().map(|c| (c.symbol(), format!("{:?}", c.style()), c.skip)).collect::<Vec<_>>() }));
}

#[test]
fn diagnostic_painter_parity() {
    let mut records = Vec::new();
    let theme = Theme::default();
    for source in ["ｶﾞX","ｶﾟX","ﾞX","ﾟX","aﾞX","あﾞX","", "abcde", "界x界", "e\u{301} 👩‍💻 #\u{fe0f} done", "\u{301}x", "a\0b\tc\nd\re", "\u{200b}a\u{200b}", "longword abc rest", "🫩 🏳️‍🌈", "한글", "क\u{93e}Z"] {
        for split in [false, true] {
            for align in [ratatui::layout::Alignment::Left, ratatui::layout::Alignment::Center, ratatui::layout::Alignment::Right] {
                let mut s = surface(&theme);
                s.kind = TranscriptRenderSurfaceKind::AssistantBody;
                s.tool_rail_motion = None;
                s.show_outer_rail = false;
                let spans = if split { source.chars().enumerate().map(|(i,c)| Span::styled(c.to_string(), Style::default().fg(if i%2 == 0 { ratatui::style::Color::Green } else {ratatui::style::Color::Yellow}))).collect() }
                    else { vec![Span::styled(source.to_string(), Style::default().fg(ratatui::style::Color::Green))] };
                s.lines = vec![Line::from(spans).alignment(align).style(Style::default().bg(ratatui::style::Color::Magenta).add_modifier(ratatui::style::Modifier::ITALIC)), Line::default()];
                for width in [0,1,2,3,4,5,6,7,8,9,12,20,40,60] {
                    record(&mut records,&s,&theme,width,0,0);
                }
            }
        }
    }
    for mode in [GlyphMode::Preferred, GlyphMode::Ascii] {
        let theme = Theme::default().with_glyph_mode(mode);
        for kind in [TranscriptRenderSurfaceKind::User, TranscriptRenderSurfaceKind::AssistantFooter, TranscriptRenderSurfaceKind::AssistantReasoning, TranscriptRenderSurfaceKind::AssistantTool, TranscriptRenderSurfaceKind::AssistantCommandTool] {
            for motion in [None, Some(ToolRailMotion::Running{elapsed: Duration::from_millis(73),sampled_phase:3}),Some(ToolRailMotion::Waiting),Some(ToolRailMotion::Queued),Some(ToolRailMotion::FinishFlash{elapsed:Duration::ZERO,sampled_phase:0}),Some(ToolRailMotion::Settled)] {
                for outer in [false,true] {
                    for rail in ["", " ", "┃", "界a", "ﾞ", "ﾟ"] {
                        let mut s = surface(&theme);
                        s.kind=kind;s.tool_rail_motion=motion;s.show_outer_rail=outer;s.rail_glyph=rail;
                        s.lines.push(Line::from(vec![Span::styled("┃",Style::default().fg(ratatui::style::Color::Cyan)),Span::raw(" "),Span::styled(format!("{} ",theme.live_shell.transcript_glyphs.group_marker),Style::default()),Span::raw("Read "),Span::raw("group")]));
                        s.lines.push(Line::from(vec![Span::raw("⠋"),Span::raw(theme.live_shell.transcript_glyphs.tool_marker),Span::raw("Running")]));
                        s.lines.push(Line::default());
                        for scroll in [0,1,3] {
                            for phase in [0,3,10,200] {
                                record(&mut records,&s,&theme,20,scroll,phase);
                            }
                        }
                        // Group coloring is determined by the original first row.
                        s.lines.swap(0,1);
                        record(&mut records,&s,&theme,20,0,10);
                    }
                }
            }
        }
    }
    let path = std::env::var("TUI_SURFACE_PAINT_OUT").expect("output");
    std::fs::write(path,serde_json::to_vec(&records).expect("serialize")).expect("save");
}
