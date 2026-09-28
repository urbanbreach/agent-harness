use super::*;

pub(super) struct ResolvedComposer<'a> {
    pub(super) body: &'a str,
    pub(super) surface: crate::composer_integration::ComposerSurface,
    pub(super) tone: crate::composer_integration::ComposerTone,
    pub(super) viewport: ComposerViewport,
    pub(super) chrome: &'static [crate::composer_integration::ComposerChrome],
}

pub(super) fn resolve_composer<'a>(
    app: &AppState,
    actual: &crate::composer_integration::ComposerRenderData<'_>,
    text: &'a str,
    focused: bool,
    disabled: bool,
    startup: bool,
    placeholder: &'a str,
    body_width: usize,
    max_text_rows: usize,
    available_rows: u16,
    show_cursor: bool,
) -> ResolvedComposer<'a> {
    // Atom widths budget rows; the separate string layout places painted cells.
    let mirror;
    let buffer = if actual.text == text {
        actual.buffer
    } else {
        mirror = crate::composer_atoms::AtomBuffer::from_text(text);
        &mirror
    };
    let rows = buffer
        .wrap(u16::try_from(body_width).unwrap_or(u16::MAX).max(1))
        .len()
        .min(max_text_rows.max(1));
    let surface = surface_for(app, startup);
    let (text_rows, chrome, _) = crate::composer_integration::ComposerPresentationConfig::new(
        surface,
        focused,
        disabled,
        available_rows.max(1),
    )
    .layout(text.is_empty(), rows);
    let body = if text.is_empty() { placeholder } else { text };
    let mut viewport = composer_viewport(
        body,
        body_width,
        usize::from(text_rows).min(max_text_rows).max(1),
        show_cursor.then_some(app.composer_render_cursor()),
    );
    if !show_cursor {
        viewport.cursor = None;
    }
    ResolvedComposer {
        body,
        surface,
        tone: surface.tone(),
        viewport,
        chrome,
    }
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
