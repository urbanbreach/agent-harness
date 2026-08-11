use super::*;
use crate::theme::GlyphMode;
use harness_core::event::PermissionDecision;

pub(super) fn default_app_uses_harness_chat_theme() {
    let app = AppState::default();

    assert_eq!(app.theme(), &Theme::harness_chat());
    assert_eq!(rendered_cell_bg(&app, 0, 0), Color::Rgb(20, 20, 20));
}

pub(super) fn explicit_harness_chat_selection_uses_harness_chat_theme() {
    let mut app = AppState::default();

    app.apply_theme_by_name("harness-chat");

    assert_eq!(app.theme(), &Theme::harness_chat());
    assert_eq!(app.theme_name, "harness-chat");
}

pub(super) fn explicit_harness_dark_selection_remains_available() {
    let mut app = AppState::default();

    app.apply_theme_by_name("harness-dark");

    assert_eq!(app.theme(), &Theme::harness_dark());
    assert_eq!(app.theme_name, "harness-dark");
}

pub(super) fn default_harness_chat_survives_color_level_changes() {
    let mut app = AppState::default();

    app.set_color_level(ColorLevel::Basic);

    assert_eq!(
        app.theme(),
        &Theme::harness_chat().for_color_level(ColorLevel::Basic)
    );
}

pub(super) fn legacy_glyph_mode_survives_color_and_theme_changes() {
    let mut app = AppState::default();

    app.set_glyph_mode(GlyphMode::Ascii);
    app.set_color_level(ColorLevel::Basic);
    app.apply_theme_by_name("harness-dark");

    assert_eq!(app.theme().live_shell.glyphs.streaming, "o");
    assert_eq!(app.theme().live_shell.glyphs.done, "*");
    assert_eq!(app.theme().live_shell.transcript_glyphs.user_marker, ">");
    assert_eq!(app.theme().color_level(), ColorLevel::Basic);
}

pub(super) fn legacy_glyph_mode_reaches_permission_and_transcript_surfaces() {
    let mut app = AppState::new_live(None, false, None);
    app.set_glyph_mode(GlyphMode::Ascii);
    app.ingest_event(envelope(
        1,
        "req_ascii_permission",
        EventV1::PermissionRequested(PermissionRequestedEvent {
            permission_id: "perm_ascii".to_string(),
            kind: "edit".to_string(),
            tool_call_id: Some("tool_ascii".into()),
            summary: "Allow editing fallback.txt?".to_string(),
            request_digest: "digest-ascii".to_string(),
            timeout_ms: 30_000,
            default_decision: PermissionDecision::Deny,
        }),
    ));

    let rendered = render_text(&app, 120, 40);

    assert_eq!(app.theme().glyph_mode(), GlyphMode::Ascii);
    assert!(rendered.contains('|'));
    assert!(rendered.contains("1 (*) Yes"), "{rendered}");
    assert!(rendered.contains("4 (o) No, reject"), "{rendered}");
    for unsupported in ['❯', '┃', '●', '○', '◆', '◈', '▸', '▾'] {
        assert!(
            !rendered.contains(unsupported),
            "legacy render contains unsupported {unsupported:?}\n{rendered}"
        );
    }
}

pub(super) fn legacy_glyph_mode_reaches_question_permission_surfaces() {
    let mut app = AppState::new_live(None, false, None);
    app.set_glyph_mode(GlyphMode::Ascii);
    app.ingest_event(envelope(
        1,
        "req_ascii_question",
        EventV1::PermissionRequested(PermissionRequestedEvent {
            permission_id: "perm_ascii_question".to_string(),
            kind: "question".to_string(),
            tool_call_id: None,
            summary: r#"{"questions":[{"question":"Pick one","header":"Choice","options":[{"label":"Alpha","description":"First"},{"label":"Beta","description":"Second"}],"multiple":false,"custom":true}]}"#.to_string(),
            request_digest: "digest-ascii-question".to_string(),
            timeout_ms: 30_000,
            default_decision: PermissionDecision::Deny,
        }),
    ));

    let rendered = render_text(&app, 120, 40);

    assert!(rendered.contains("1 (o) Alpha"), "{rendered}");
    for unsupported in ['❯', '●', '○', '✓'] {
        assert!(
            !rendered.contains(unsupported),
            "legacy question render contains unsupported {unsupported:?}\n{rendered}"
        );
    }
}

pub(super) fn arbitrary_viewport_composer_mouse_uses_rendered_geometry() {
    let mut app = AppState::new_live(None, false, None);
    app.focus = Focus::List;
    let frame_area = Rect::new(0, 0, 120, 40);
    let composer = FrameLayoutPlan::for_app(&app, frame_area)
        .composer
        .expect("120x40 composer");
    let mouse = MouseEvent {
        kind: MouseEventKind::Down(MouseButton::Left),
        column: composer.x.saturating_add(2),
        row: composer.y.saturating_add(1),
        modifiers: KeyModifiers::NONE,
    };

    assert!(
        mouse.column >= composer.x
            && mouse.column < composer.right()
            && mouse.row >= composer.y
            && mouse.row < composer.bottom()
    );
    let viewport = crate::design_contract::ViewportId::closest(120, 40);
    assert_eq!(viewport, crate::design_contract::ViewportId::Wide132x40);
    assert!(app.handle_composer_mouse_event(mouse, frame_area));
    assert_eq!(app.focus, Focus::Prompt);
}
