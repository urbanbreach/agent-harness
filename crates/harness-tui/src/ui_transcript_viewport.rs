use ratatui::layout::Rect;

use super::ui_transcript::{
    resolved_transcript_area, transcript_pane_context, transcript_surface_area,
    with_measured_transcript_layout_for_width_on_surface,
};
use super::ui_transcript_layout::{
    transcript_layout_has_visible_running_tool, MeasuredTranscriptLayout, TranscriptContentAnchor,
};
use super::ui_transcript_page_flip::transcript_scroll_position;
use super::ui_transcript_scrollbar::{transcript_scrollbar_needed, transcript_viewport_layout};
use crate::app::{transcript_viewport::TranscriptViewport, AppState};

pub(super) fn resolve_viewport(
    app: &AppState,
    layout: &MeasuredTranscriptLayout,
    height: u16,
) -> (TranscriptViewport, Option<TranscriptContentAnchor>) {
    let max = layout.total_height.saturating_sub(usize::from(height));
    let viewport = app.transcript_view.viewport.record_max_scroll(max);
    if viewport.is_following() {
        return (viewport, None);
    }
    let anchor = app.transcript_view.measured_anchor;
    let resolved = anchor.and_then(|anchor| layout.resolve_content_anchor(anchor));
    let top = resolved.unwrap_or_else(|| viewport.top()).min(max);
    let anchor = if resolved == Some(top) {
        anchor
    } else {
        layout.capture_content_anchor(top)
    };
    (TranscriptViewport::detached(top, max), anchor)
}

/// Commit geometry before input or paint. Rendering only reads this state.
pub(crate) fn prepare_transcript(app: &mut AppState, area: Rect) {
    app.transcript_view.hyperlinks.clear();
    if app.status_dashboard_is_active()
        || app.startup_shell_visible()
        || super::ui_lifecycle::live_empty_state_visible(app)
    {
        return;
    }
    let Some(area) = resolved_transcript_area(app, area) else {
        return;
    };
    let theme = *app.theme();
    let context = transcript_pane_context(app, area, &theme);
    let scrollbar = with_measured_transcript_layout_for_width_on_surface(
        app,
        &theme,
        context.inner_area.width,
        context.base_surface,
        |layout| transcript_scrollbar_needed(layout.total_height, context.inner_area),
    );
    let content = transcript_viewport_layout(context.inner_area, scrollbar).content;
    let (viewport, anchor, position, height, running, links) =
        with_measured_transcript_layout_for_width_on_surface(
            app,
            &theme,
            content.width,
            context.base_surface,
            |layout| {
                let (viewport, anchor) = resolve_viewport(app, layout, content.height);
                let top = app
                    .transcript_page_flip_scroll_top()
                    .unwrap_or_else(|| viewport.top());
                let position = transcript_scroll_position(
                    app.transcript_page_flip_state(),
                    layout,
                    content.height,
                    top,
                );
                let surface = transcript_surface_area(app, content, position);
                let height = surface.height;
                let running = transcript_layout_has_visible_running_tool(
                    layout,
                    usize::from(height),
                    position.top,
                );
                (
                    viewport.record_max_scroll(position.max_scroll),
                    anchor,
                    position,
                    height,
                    running,
                    super::ui_transcript::transcript_hyperlinks(layout, surface, position.top),
                )
            },
        );
    if app.transcript_view.transcript_selection_anchors.is_none() {
        app.transcript_view.transcript_selection_anchors =
            capture_selection_anchors(app, app.last_frame_area().unwrap_or(area));
    }
    app.transcript_view.hyperlinks = links;
    app.transcript_view.viewport = viewport;
    app.transcript_view.measured_anchor = anchor;
    app.transcript_view.last_transcript_viewport_height = usize::from(height);
    app.set_transcript_page_flip_state(position.page_flip);
    app.record_visible_running_tool_motion(running);
}

pub(crate) fn capture_selection_anchors(
    app: &AppState,
    area: Rect,
) -> Option<(TranscriptContentAnchor, TranscriptContentAnchor)> {
    if app.startup_shell_visible() || super::ui_lifecycle::live_empty_state_visible(app) {
        return None;
    }
    let selection = app.transcript_selection()?;
    let context = transcript_pane_context(app, resolved_transcript_area(app, area)?, app.theme());
    let scrollbar = with_measured_transcript_layout_for_width_on_surface(
        app,
        app.theme(),
        context.inner_area.width,
        context.base_surface,
        |layout| transcript_scrollbar_needed(layout.total_height, context.inner_area),
    );
    let width = transcript_viewport_layout(context.inner_area, scrollbar)
        .content
        .width;
    with_measured_transcript_layout_for_width_on_surface(
        app,
        app.theme(),
        width,
        context.base_surface,
        |layout| {
            Some((
                layout.capture_selection_anchor(selection.anchor)?,
                layout.capture_selection_anchor(selection.focus)?,
            ))
        },
    )
}
