use super::ui_compaction_motion::{context_bar, remaining, render};
use crate::app::AppState;
use crate::theme::{ColorLevel, GlyphMode};
use harness_compaction::CompactionMethod;
use harness_core::event::{
    ActorKind, CompactionPhase, CompactionStage, EventActor, FrameThumbnail, LiveEventEnvelope,
    LiveEventV1, RuntimeEvent,
};
use ratatui::{backend::TestBackend, buffer::Buffer, layout::Rect, style::Modifier, Terminal};
use std::time::Duration;

type Result<T = ()> = std::result::Result<T, Box<dyn std::error::Error>>;

fn stage(method: CompactionMethod, phase: CompactionPhase) -> CompactionStage {
    CompactionStage {
        reason: "manual".into(),
        method: Some(method),
        phase,
        tokens_before: Some(182_400),
        tokens_after: Some(31_050),
        summarized_tokens: Some(154_400),
        kept_tokens: Some(20_000),
        fixed_tokens: Some(8_000),
        threshold_tokens: Some(220_000),
        window_tokens: Some(262_144),
        first_kept_seq: Some(3),
        windows_done: 1,
        windows_total: 3,
        frames_done: 1,
        frames_total: 2,
        thumbnails: Vec::new(),
        message: None,
    }
}

fn progress(app: &mut AppState, stage: CompactionStage, preview: Option<&str>) {
    app.ingest_runtime_event(RuntimeEvent::Live(Box::new(LiveEventEnvelope {
        event_id: "maintenance-evidence".into(),
        run_id: "maintenance-test".into(),
        mono_ms: 0,
        ts: None,
        actor: EventActor::new(ActorKind::Worker, Some("root".into())),
        correlation_id: None,
        causation_id: None,
        stream_key: None,
        payload: LiveEventV1::CompactionProgress {
            agent_id: "root".into(),
            generation: 1,
            trigger_reason: "manual".into(),
            preview: preview.map(str::to_string),
            stage: Some(stage),
        },
    })));
}

fn app(reduced: bool, color: ColorLevel, glyph: GlyphMode) -> AppState {
    let mut app = AppState::new_live(None, false, None);
    app.freeze_animation_clock();
    app.set_reduced_motion_for_evidence(reduced);
    app.set_startup_logo_capabilities_for_evidence(color, glyph);
    app
}

fn card(app: &AppState, width: u16, height: u16) -> Result<(bool, Buffer)> {
    let mut terminal = Terminal::new(TestBackend::new(width, height))?;
    let mut visible = false;
    terminal.draw(|frame| {
        visible = render(frame, app, Rect::new(0, 0, width, height), app.theme());
    })?;
    Ok((visible, terminal.backend().buffer().clone()))
}

fn text(buffer: &Buffer) -> String {
    buffer.content.iter().map(|cell| cell.symbol()).collect()
}

#[test]
fn live_cards_preserve_controls_counters_and_semantic_styles_across_capabilities() -> Result {
    let dimensions = [40, 80, 120]
        .into_iter()
        .flat_map(|width| [3, 8].map(move |height| (width, height)));
    for (width, height) in dimensions {
        for (glyph, color) in [
            (GlyphMode::Preferred, ColorLevel::Basic),
            (GlyphMode::Preferred, ColorLevel::Ansi256),
            (GlyphMode::Ascii, ColorLevel::Basic),
            (GlyphMode::Ascii, ColorLevel::Ansi256),
        ] {
            for reduced in [false, true] {
                assert_capability_card(width, height, glyph, color, reduced)?;
            }
        }
    }
    Ok(())
}

fn assert_capability_card(
    width: u16,
    height: u16,
    glyph: GlyphMode,
    color: ColorLevel,
    reduced: bool,
) -> Result {
    let mut app = app(reduced, color, glyph);
    progress(
        &mut app,
        stage(CompactionMethod::Soft, CompactionPhase::Summarizing),
        Some("Retain API invariants"),
    );
    app.advance_wall_clock_for_motion_evidence(Duration::from_millis(350));
    let (visible, buffer) = card(&app, width, height)?;
    let rendered = text(&buffer);
    assert!(visible);
    assert!(rendered.contains("Ctrl+C"), "{width}x{height}: {rendered}");
    if height == 3 || width < 48 {
        assert!(rendered.contains("1/3"), "{width}x{height}: {rendered}");
    }
    assert!(buffer
        .content
        .iter()
        .any(|cell| cell.fg == app.theme().text.accent && cell.modifier.contains(Modifier::BOLD)));
    if glyph == GlyphMode::Ascii {
        assert!(rendered.is_ascii(), "{rendered}");
    }
    let rich = height == 8 && width >= 48;
    if rich {
        assert!(rendered.contains("182K"), "{rendered}");
        assert_rich_border(&buffer, width, glyph, app.theme());
    }
    let map_width = if rich { width - 3 } else { width };
    let map_x = if rich { 2 } else { 0 };
    let tick = u16::try_from(220_000_u64 * u64::from(map_width) / 262_144)?;
    assert_eq!(
        buffer[(map_x + tick, 1)].symbol(),
        if glyph == GlyphMode::Ascii {
            "|"
        } else {
            "┃"
        }
    );
    assert_eq!(buffer[(map_x + tick, 1)].fg, app.theme().text.primary);
    assert_eq!(buffer[(map_x + map_width - 1, 1)].symbol(), "]");
    assert_eq!(
        buffer[(map_x + map_width - 1, 1)].fg,
        app.theme().text.primary
    );
    Ok(())
}

fn assert_rich_border(buffer: &Buffer, width: u16, glyph: GlyphMode, theme: &crate::theme::Theme) {
    for y in 0..8 {
        assert_eq!(
            buffer[(0, y)].symbol(),
            if glyph == GlyphMode::Ascii {
                "|"
            } else {
                "▎"
            }
        );
        assert_eq!(buffer[(0, y)].fg, theme.text.accent);
        assert_eq!(buffer[(width - 1, y)].bg, theme.surface.panel);
    }
}

#[test]
fn completion_counts_down_retains_preview_then_retires_at_900ms() -> Result {
    for phase in [
        CompactionPhase::Done,
        CompactionPhase::Failed,
        CompactionPhase::Cancelled,
        CompactionPhase::Skipped,
    ] {
        let mut app = app(false, ColorLevel::Ansi256, GlyphMode::Ascii);
        progress(
            &mut app,
            stage(CompactionMethod::Soft, CompactionPhase::Summarizing),
            Some("Retain API invariants"),
        );
        progress(&mut app, stage(CompactionMethod::Soft, phase), None);
        let (visible, initial) = card(&app, 120, 8)?;
        assert!(visible);
        assert!(
            text(&initial).contains("Retain API invariants"),
            "{:?}: {}",
            phase,
            text(&initial)
        );
        assert!(!text(&initial).contains("Ctrl+C"));
        if phase == CompactionPhase::Done {
            assert!(text(&initial).contains("182K"));
            app.advance_wall_clock_for_motion_evidence(Duration::from_millis(700));
            let (_, settled) = card(&app, 120, 8)?;
            let rendered = text(&settled);
            assert!(
                rendered.contains("31K") && rendered.contains("83%"),
                "{rendered}"
            );
            assert!(settled
                .content
                .iter()
                .any(|cell| cell.fg == app.theme().status.success));
            app.advance_wall_clock_for_motion_evidence(Duration::from_millis(199));
        } else {
            if phase == CompactionPhase::Failed {
                assert!(initial
                    .content
                    .iter()
                    .any(|cell| cell.fg == app.theme().status.error));
            }
            app.advance_wall_clock_for_motion_evidence(Duration::from_millis(899));
        }
        assert!(card(&app, 120, 8)?.0);
        app.advance_wall_clock_for_motion_evidence(Duration::from_millis(1));
        app.refresh_motion_for_evidence();
        assert!(!card(&app, 120, 8)?.0, "{phase:?}");
    }
    let mut reduced = app(true, ColorLevel::Basic, GlyphMode::Ascii);
    progress(
        &mut reduced,
        stage(CompactionMethod::Soft, CompactionPhase::Summarizing),
        Some("Retain API invariants"),
    );
    progress(
        &mut reduced,
        stage(CompactionMethod::Soft, CompactionPhase::Done),
        None,
    );
    reduced.refresh_motion_for_evidence();
    assert!(!card(&reduced, 120, 8)?.0);
    assert_eq!(remaining(182_400, 31_050, 350, true), 49_968);
    assert_eq!(remaining(182_400, 31_050, 0, false), 31_050);
    Ok(())
}

#[test]
fn archive_thumbnail_cells_are_derived_from_bitmap_not_frame_count() -> Result {
    for glyph in [GlyphMode::Preferred, GlyphMode::Ascii] {
        let mut app = app(true, ColorLevel::Basic, glyph);
        let mut archive = stage(CompactionMethod::Archive, CompactionPhase::Archiving);
        archive.thumbnails = vec![FrameThumbnail {
            width: 16,
            height: 8,
            bits: "gAAAAAAAAAAAAAAAAAAAAA==".into(),
        }];
        progress(&mut app, archive.clone(), Some(""));
        let (_, first) = card(&app, 120, 8)?;
        assert!(text(&first).contains("1/2"));
        assert_eq!(
            first[(3, 4)].symbol(),
            if glyph == GlyphMode::Ascii {
                "#"
            } else {
                "▘"
            }
        );
        assert_eq!(first[(10, 7)].symbol(), " ");
        archive.thumbnails[0].bits = "AAAAAAAAAAAAAAAAAAAAAQ==".into();
        progress(&mut app, archive, Some(""));
        let (_, changed) = card(&app, 120, 8)?;
        assert_eq!(changed[(3, 4)].symbol(), " ");
        assert_eq!(
            changed[(10, 7)].symbol(),
            if glyph == GlyphMode::Ascii {
                "#"
            } else {
                "▗"
            }
        );
    }
    let mut moving = app(false, ColorLevel::Ansi256, GlyphMode::Preferred);
    let mut archive = stage(CompactionMethod::Archive, CompactionPhase::Archiving);
    archive.thumbnails = vec![FrameThumbnail {
        width: 16,
        height: 8,
        bits: "gAAAAAAAAAAAAAAAAAAAAA==".into(),
    }];
    progress(&mut moving, archive, Some(""));
    let (_, entering) = card(&moving, 120, 8)?;
    assert_eq!(entering[(13, 4)].symbol(), "▘");
    assert_eq!(entering[(13, 4)].fg, moving.theme().status.success);
    moving.advance_wall_clock_for_motion_evidence(Duration::from_millis(350));
    let (_, landed) = card(&moving, 120, 8)?;
    assert_eq!(landed[(3, 4)].symbol(), "▘");
    assert_eq!(landed[(3, 4)].fg, moving.theme().text.accent);
    Ok(())
}

#[test]
fn summary_capsule_keeps_partition_and_threshold_roles_when_motion_is_reduced() {
    for glyph in [GlyphMode::Preferred, GlyphMode::Ascii] {
        let app = app(true, ColorLevel::Ansi256, glyph);
        let stage = stage(CompactionMethod::Soft, CompactionPhase::Summarizing);
        let bar = context_bar(&stage, app.theme(), 80, 350, false);
        let rendered: String = bar.spans.iter().map(|span| span.content.as_ref()).collect();
        assert!(
            rendered.contains(if glyph == GlyphMode::Ascii {
                "["
            } else {
                "⟦"
            }),
            "{rendered}"
        );
        assert_eq!(bar.spans[0].style.fg, Some(app.theme().text.tertiary));
        assert!(bar
            .spans
            .iter()
            .any(|span| span.style.fg == Some(app.theme().status.info)));
        assert_eq!(bar.spans[67].style.fg, Some(app.theme().text.primary));
        assert_eq!(bar.spans[79].content, "]");
    }
}

#[test]
fn method_flourishes_show_their_operation_and_stop_moving_when_reduced() -> Result {
    for glyph in [GlyphMode::Preferred, GlyphMode::Ascii] {
        for (method, phase, unicode, ascii) in [
            (CompactionMethod::Remote, CompactionPhase::Remote, "☁", "@"),
            (
                CompactionMethod::Handoff,
                CompactionPhase::Handoff,
                "✎",
                ">",
            ),
            (CompactionMethod::Shake, CompactionPhase::Shaking, "█", "#"),
            (
                CompactionMethod::Soft,
                CompactionPhase::Summarizing,
                "⟦",
                "[",
            ),
        ] {
            for reduced in [false, true] {
                let mut app = app(reduced, ColorLevel::Ansi256, glyph);
                progress(
                    &mut app,
                    stage(method, phase),
                    Some("Preserve API invariants"),
                );
                app.advance_wall_clock_for_motion_evidence(Duration::from_millis(350));
                let (_, first) = card(&app, 120, 8)?;
                let first_flourish: String = (4..8)
                    .flat_map(|y| (2..118).map(move |x| (x, y)))
                    .map(|pos| first[pos].symbol())
                    .collect();
                assert!(
                    first_flourish.contains(if glyph == GlyphMode::Ascii {
                        ascii
                    } else {
                        unicode
                    }),
                    "{method:?}: {first_flourish}"
                );
                if glyph == GlyphMode::Ascii {
                    assert!(first_flourish.is_ascii(), "{method:?}: {first_flourish}");
                }
                app.advance_wall_clock_for_motion_evidence(Duration::from_millis(210));
                let (_, later) = card(&app, 120, 8)?;
                let later_flourish: String = (4..8)
                    .flat_map(|y| (2..118).map(move |x| (x, y)))
                    .map(|pos| later[pos].symbol())
                    .collect();
                if reduced {
                    assert_eq!(first_flourish, later_flourish, "{method:?}");
                } else {
                    assert_ne!(first_flourish, later_flourish, "{method:?}");
                }
            }
        }
    }
    let mut shake = app(false, ColorLevel::Ansi256, GlyphMode::Preferred);
    progress(
        &mut shake,
        stage(CompactionMethod::Shake, CompactionPhase::Shaking),
        Some("Elide heavy blocks"),
    );
    progress(
        &mut shake,
        stage(CompactionMethod::Shake, CompactionPhase::Done),
        None,
    );
    shake.advance_wall_clock_for_motion_evidence(Duration::from_millis(700));
    let (_, settled) = card(&shake, 120, 8)?;
    let settled_flourish: String = (4..8)
        .flat_map(|y| (2..118).map(move |x| (x, y)))
        .map(|pos| settled[pos].symbol())
        .collect();
    assert!(settled_flourish.contains("151K"), "{settled_flourish}");
    assert!(!settled_flourish.contains("█"), "{settled_flourish}");
    Ok(())
}

fn hero_map(
    stage: &CompactionStage,
    glyph: GlyphMode,
    elapsed: u64,
    moving: bool,
) -> Result<Buffer> {
    let app = app(!moving, ColorLevel::Ansi256, glyph);
    let mut terminal = Terminal::new(TestBackend::new(80, 4))?;
    terminal.draw(|frame| {
        super::ui_compaction_map::render(
            frame,
            Rect::new(0, 0, 80, 4),
            stage,
            app.theme(),
            elapsed,
            moving,
        )
    })?;
    Ok(terminal.backend().buffer().clone())
}

#[test]
fn hero_bar_preserves_two_rows_literal_partitions_and_static_fallbacks() -> Result {
    for glyph in [GlyphMode::Preferred, GlyphMode::Ascii] {
        let stage = stage(CompactionMethod::Soft, CompactionPhase::Preparing);
        let first = hero_map(&stage, glyph, 200, false)?;
        assert_eq!(first, hero_map(&stage, glyph, 850, false)?);
        for x in 0..80 {
            assert_eq!(first[(x, 1)], first[(x, 2)]);
        }
        assert_eq!(
            first[(0, 1)].symbol(),
            if glyph == GlyphMode::Ascii {
                "#"
            } else {
                "█"
            }
        );
        assert_eq!(
            first[(66, 1)].symbol(),
            if glyph == GlyphMode::Ascii {
                "|"
            } else {
                "┃"
            }
        );
        assert_eq!(
            first[(79, 1)].symbol(),
            if glyph == GlyphMode::Ascii {
                "]"
            } else {
                "▕"
            }
        );
        let theme = *app(true, ColorLevel::Ansi256, glyph).theme();
        assert_eq!(first[(0, 1)].fg, theme.text.tertiary);
        assert_eq!(first[(20, 1)].fg, theme.text.accent);
        assert_eq!(first[(50, 1)].fg, theme.status.info);
        let rendered = text(&first);
        for value in [
            "system 8K",
            "summarizing 154K",
            "kept 20K",
            "threshold 220K",
        ] {
            assert!(rendered.contains(value), "{rendered}");
        }
        if glyph == GlyphMode::Ascii {
            assert!(rendered.is_ascii());
        }
    }
    Ok(())
}

#[test]
fn hero_dissolve_tracks_windows_and_commit_squashes_history() -> Result {
    let mut stage = stage(CompactionMethod::Soft, CompactionPhase::Summarizing);
    let early = hero_map(&stage, GlyphMode::Preferred, 200, true)?;
    let late = hero_map(&stage, GlyphMode::Preferred, 850, true)?;
    assert!(text(&early).contains("▓"));
    assert_ne!(early, late);
    stage.windows_done = 3;
    let complete = hero_map(&stage, GlyphMode::Preferred, 200, true)?;
    assert_eq!(complete[(0, 0)].symbol(), "◆");
    assert_ne!(early[(30, 1)].symbol(), complete[(30, 1)].symbol());
    stage.phase = CompactionPhase::Committing;
    // Commit starts before the final usage measurement is available.
    stage.tokens_after = None;
    let expanded = hero_map(&stage, GlyphMode::Preferred, 0, true)?;
    let collapsed = hero_map(&stage, GlyphMode::Preferred, 700, true)?;
    let theme = *app(true, ColorLevel::Ansi256, GlyphMode::Preferred).theme();
    let kept_start = |buffer: &Buffer| (0..80).find(|x| buffer[(*x, 1)].fg == theme.status.info);
    assert!(kept_start(&collapsed) < kept_start(&expanded));
    assert_eq!(collapsed, hero_map(&stage, GlyphMode::Preferred, 0, false)?);
    Ok(())
}
