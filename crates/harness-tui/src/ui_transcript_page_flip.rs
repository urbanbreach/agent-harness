use super::ui_transcript::TranscriptRenderSurfaceKind;
use super::ui_transcript_layout::MeasuredTranscriptLayout;
use crate::transcript_scroll::PageFlipState;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct TranscriptScrollPosition {
    pub(super) top: usize,
    pub(super) max_scroll: usize,
    pub(super) page_flip: PageFlipState,
}

pub(super) fn transcript_scroll_position(
    page_flip: PageFlipState,
    layout: &MeasuredTranscriptLayout,
    viewport_height: u16,
    regular_top: usize,
) -> TranscriptScrollPosition {
    let viewport_height = usize::from(viewport_height);
    let regular_max = layout.total_height.saturating_sub(viewport_height);
    if matches!(page_flip, PageFlipState::Detached { .. }) {
        let pinned_surface_exists = page_flip
            .activity_first_seq()
            .and_then(|first_seq| user_surface_metrics(layout, first_seq))
            .is_some();
        if regular_max == 0 || (!pinned_surface_exists && regular_top >= regular_max) {
            return TranscriptScrollPosition {
                top: regular_max,
                max_scroll: regular_max,
                page_flip: page_flip.consume(),
            };
        }
        let detached_max = page_flip
            .activity_first_seq()
            .and_then(|first_seq| user_surface_metrics(layout, first_seq))
            .map(|(user_top, _)| user_top)
            .map_or(regular_max, |user_top| regular_max.max(user_top));
        let top = regular_top.min(detached_max);
        return TranscriptScrollPosition {
            top,
            max_scroll: detached_max,
            page_flip: page_flip.detach_at(top),
        };
    }
    if !page_flip.is_preserving() || viewport_height == 0 {
        return TranscriptScrollPosition {
            top: regular_top.min(regular_max),
            max_scroll: regular_max,
            page_flip,
        };
    }

    let Some((user_top, target_bottom)) = page_flip
        .activity_first_seq()
        .and_then(|first_seq| user_surface_metrics(layout, first_seq))
    else {
        return TranscriptScrollPosition {
            top: regular_top,
            max_scroll: regular_max,
            page_flip: page_flip.consume(),
        };
    };

    if target_bottom > user_top.saturating_add(viewport_height) {
        return TranscriptScrollPosition {
            top: regular_max,
            max_scroll: regular_max,
            page_flip: page_flip.consume(),
        };
    }

    TranscriptScrollPosition {
        top: user_top,
        max_scroll: regular_max.max(user_top),
        page_flip: page_flip.preserve_at(user_top),
    }
}

fn user_surface_metrics(
    layout: &MeasuredTranscriptLayout,
    activity_first_seq: u64,
) -> Option<(usize, usize)> {
    let section = layout
        .sections
        .iter()
        .find(|section| section.activity_first_seq == activity_first_seq)?;
    let surface = section
        .surfaces
        .iter()
        .find(|surface| surface.kind == TranscriptRenderSurfaceKind::User)?;
    let user_top = section
        .top_row
        .saturating_add(section.leading_gap_height)
        .saturating_add(surface.top_offset);
    Some((
        user_top,
        section.top_row.saturating_add(section.total_height()),
    ))
}
