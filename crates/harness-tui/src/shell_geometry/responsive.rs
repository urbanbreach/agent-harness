use ratatui::layout::Rect;

use crate::design_contract::{ViewportId, DESIGN_TOKENS, VIEWPORTS};

use super::regions::{ShellRegions, ShellState};

pub fn layout_for(viewport: ViewportId, state: ShellState) -> ShellRegions {
    let (width, height) = viewport.dimensions();
    let _breakpoint = VIEWPORTS
        .iter()
        .find(|candidate| candidate.id == viewport)
        .map(|candidate| candidate.band);
    layout_for_rect(Rect::new(0, 0, width, height), state)
}

pub fn layout_for_rect(viewport: Rect, state: ShellState) -> ShellRegions {
    let spacing = DESIGN_TOKENS.spacing;
    let top_height = spacing.header_rows.min(viewport.height);
    let top_bar = Rect::new(viewport.x, viewport.y, viewport.width, top_height);
    let footer_height = spacing
        .footer_rows
        .min(viewport.height.saturating_sub(top_height));
    let footer_y = viewport.bottom().saturating_sub(footer_height);
    let status_footer = Rect::new(viewport.x, footer_y, viewport.width, footer_height);
    let body_height = footer_y.saturating_sub(top_bar.bottom());
    let composer_height = spacing.prompt_input_rows.min(body_height);
    let composer_y = footer_y.saturating_sub(composer_height);
    let composer = Rect::new(viewport.x, composer_y, viewport.width, composer_height);
    let transcript_height = composer_y.saturating_sub(top_bar.bottom());
    let transcript_viewport = Rect::new(
        viewport.x,
        top_bar.bottom(),
        viewport.width,
        transcript_height,
    );
    let welcome = if matches!(state, ShellState::Idle) {
        inset_welcome(transcript_viewport, spacing.modal_margin)
    } else {
        Rect::default()
    };
    let overlay = overlay_rect(viewport, transcript_viewport, state);
    let overlays = if state.is_overlay() {
        vec![overlay]
    } else {
        Vec::new()
    };

    ShellRegions {
        viewport,
        state,
        top_bar,
        transcript_viewport,
        composer,
        status_footer,
        overlays,
        welcome,
    }
}

fn inset_welcome(area: Rect, margin: u16) -> Rect {
    let horizontal = margin.min(area.width / 2);
    let vertical = margin.min(area.height / 2);
    Rect::new(
        area.x.saturating_add(horizontal),
        area.y.saturating_add(vertical),
        area.width.saturating_sub(horizontal.saturating_mul(2)),
        area.height.saturating_sub(vertical.saturating_mul(2)),
    )
}

fn overlay_rect(viewport: Rect, transcript: Rect, state: ShellState) -> Rect {
    if !state.is_overlay() {
        return Rect::default();
    }
    let margin = DESIGN_TOKENS.spacing.modal_margin.min(viewport.width / 2);
    let width = viewport.width.saturating_sub(margin.saturating_mul(2));
    let height = 7.min(transcript.height);
    let x = viewport
        .x
        .saturating_add(viewport.width.saturating_sub(width) / 2);
    let y = transcript
        .y
        .saturating_add(transcript.height.saturating_sub(height) / 2);
    Rect::new(x, y, width, height)
}
