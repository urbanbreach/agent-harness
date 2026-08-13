use super::*;

pub(super) struct ResolvedComposer {
    pub(super) body: String,
    pub(super) surface: crate::composer_integration::ComposerSurface,
    pub(super) tone: crate::composer_integration::ComposerTone,
    pub(super) viewport: ComposerViewport,
    pub(super) chrome: Vec<crate::composer_integration::ComposerChrome>,
}

pub(super) fn resolve_composer(
    app: &AppState,
    text: &str,
    focused: bool,
    disabled: bool,
    startup: bool,
    placeholder: &str,
    body_width: usize,
    max_text_rows: usize,
    available_rows: u16,
    show_cursor: bool,
) -> Option<ResolvedComposer> {
    let actual = app.composer_view_model_for_area(Rect::new(
        0,
        0,
        u16::try_from(body_width).unwrap_or(u16::MAX).max(1),
        u16::try_from(max_text_rows).unwrap_or(u16::MAX).max(1),
    ));
    let editor = if actual.editor.text() == text {
        actual
            .editor
            .reflow(
                u16::try_from(body_width).unwrap_or(u16::MAX).max(1),
                max_text_rows.max(1),
            )
            .ok()?
    } else {
        legacy_mirror_editor(app, text, body_width, max_text_rows)?
    };
    let surface = surface_for(app, startup);
    let presentation = crate::composer_integration::ComposerPresentation::resolve(
        &editor,
        crate::composer_integration::ComposerPresentationConfig::new(
            surface,
            focused,
            disabled,
            available_rows.max(1),
        )
        .with_placeholder(placeholder),
    )
    .ok()?;
    let mut viewport = composer_viewport(
        presentation.body(),
        body_width,
        usize::from(presentation.text_rows())
            .min(max_text_rows)
            .max(1),
        show_cursor.then_some(app.composer_render_cursor()),
    );
    if !show_cursor {
        viewport.cursor = None;
    }
    Some(ResolvedComposer {
        body: presentation.body().to_owned(),
        surface,
        tone: presentation.tone(),
        viewport,
        chrome: presentation.visible_chrome().to_vec(),
    })
}

fn legacy_mirror_editor(
    app: &AppState,
    text: &str,
    body_width: usize,
    max_text_rows: usize,
) -> Option<crate::composer_integration::ComposerEditorModel> {
    crate::composer_integration::ComposerEditorModel::legacy_mirror_adapter(
        text,
        app.composer_render_cursor(),
        app.composer.selection_anchor,
        u16::try_from(body_width).unwrap_or(u16::MAX).max(1),
        max_text_rows.max(1),
    )
    .ok()
}

fn surface_for(app: &AppState, startup: bool) -> crate::composer_integration::ComposerSurface {
    use crate::composer_integration::ComposerSurface;

    if let Some(permission) = app.active_permission_view() {
        if permission.question_prompts.is_some() {
            ComposerSurface::InlinePrompt
        } else {
            ComposerSurface::Permission
        }
    } else if app.shell_mode() {
        ComposerSurface::Shell
    } else if app
        .launch_mode_label()
        .is_some_and(|label| label.eq_ignore_ascii_case("plan"))
    {
        ComposerSurface::Plan
    } else if startup {
        ComposerSurface::Startup
    } else {
        ComposerSurface::Live
    }
}
