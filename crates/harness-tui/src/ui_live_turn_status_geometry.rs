use ratatui::layout::Rect;

use crate::app::{ActivityStatus, AppState};

use super::super::{ui_chrome::display_width, ui_transcript_style::monitor_pulse_frame};
use super::status_model::format_still_running;

pub(super) const STOP_LABEL: &str = "[stop]";
pub(super) const MIN_STATUS_LABEL_WIDTH: usize = 8;

const BACKGROUND_LABEL: &str = "[↓]";
const BACKGROUND_HOVER_LABEL: &str = "[send to bg]";

#[derive(Clone, Copy, Default)]
pub(super) struct LiveTurnControlVisibility {
    pub(super) stop: bool,
    pub(super) background: bool,
}

pub(super) fn live_turn_control_visibility(
    app: &AppState,
    area: Rect,
) -> LiveTurnControlVisibility {
    if live_turn_is_parked(app) {
        return LiveTurnControlVisibility::default();
    }
    let spinner_width =
        display_width(monitor_pulse_frame(app.transcript_animation_phase())).saturating_add(1);
    let stop = app.live_turn_stop_available()
        && spinner_width
            .saturating_add(MIN_STATUS_LABEL_WIDTH)
            .saturating_add(display_width(STOP_LABEL))
            .saturating_add(1)
            <= usize::from(area.width);
    let background_label = live_turn_background_label(app);
    let background = stop
        && app.live_turn_background_available()
        && spinner_width
            .saturating_add(MIN_STATUS_LABEL_WIDTH)
            .saturating_add(display_width(background_label))
            .saturating_add(display_width(STOP_LABEL))
            .saturating_add(2)
            <= usize::from(area.width);
    LiveTurnControlVisibility { stop, background }
}

pub(crate) fn live_turn_stop_rect(app: &AppState, frame_area: Rect) -> Option<Rect> {
    if !app.live_turn_status_visible() {
        return None;
    }
    let area = crate::layout::FrameLayoutPlan::for_app(app, frame_area).status?;
    let area = crate::layout::live_turn_status_content_area(area, app.theme());
    if !live_turn_control_visibility(app, area).stop {
        return None;
    }
    let width = u16::try_from(display_width(STOP_LABEL)).unwrap_or(u16::MAX);
    Some(Rect::new(
        area.x.saturating_add(area.width.saturating_sub(width)),
        area.y,
        width,
        1,
    ))
}

pub(super) fn live_turn_background_label(app: &AppState) -> &'static str {
    if app.live_turn_background_hovered() {
        BACKGROUND_HOVER_LABEL
    } else {
        BACKGROUND_LABEL
    }
}

pub(crate) fn live_turn_background_rect(app: &AppState, frame_area: Rect) -> Option<Rect> {
    if !app.live_turn_status_visible() {
        return None;
    }
    let area = crate::layout::FrameLayoutPlan::for_app(app, frame_area).status?;
    let area = crate::layout::live_turn_status_content_area(area, app.theme());
    if !live_turn_control_visibility(app, area).background {
        return None;
    }
    let label_width = display_width(live_turn_background_label(app));
    let stop_width = display_width(STOP_LABEL);
    let width = u16::try_from(label_width).unwrap_or(u16::MAX);
    let right_controls_width = u16::try_from(stop_width.saturating_add(1)).unwrap_or(u16::MAX);
    Some(Rect::new(
        area.x
            .saturating_add(area.width)
            .saturating_sub(right_controls_width)
            .saturating_sub(width),
        area.y,
        width,
        1,
    ))
}

pub(super) fn live_turn_is_parked(app: &AppState) -> bool {
    app.runtime_state_activity()
        .filter(|entry| entry.status == ActivityStatus::Streaming)
        .is_some_and(|entry| entry.is_parked_wait())
}

pub(super) fn parked_suffix(app: &AppState) -> String {
    if app.queued_prompt_count == 0 {
        return "· send a message to interrupt".to_string();
    }
    if app.queued_prompt_send_now_available() {
        format!("· {} queued — Enter to send now", app.queued_prompt_count)
    } else {
        format!("· {} queued", app.queued_prompt_count)
    }
}

pub(crate) fn live_turn_watching_rect(app: &AppState, frame_area: Rect) -> Option<Rect> {
    let watchers = app.live_turn_watchers();
    if watchers.total() == 0 || !app.live_turn_status_visible() {
        return None;
    }
    let parked = live_turn_is_parked(app);
    if app.live_turn_stop_available() && !parked {
        return None;
    }
    let area = crate::layout::FrameLayoutPlan::for_app(app, frame_area).status?;
    let area = crate::layout::live_turn_status_content_area(area, app.theme());
    let suffix = parked.then(|| parked_suffix(app));
    let cue_width = display_width(monitor_pulse_frame(app.transcript_animation_phase()))
        .saturating_add(1)
        .saturating_add(display_width(&format_still_running(watchers)))
        .saturating_add(
            suffix
                .as_deref()
                .map_or(0, |value| display_width(value).saturating_add(1)),
        );
    Some(Rect::new(
        area.x,
        area.y,
        u16::try_from(cue_width.min(usize::from(area.width))).unwrap_or(area.width),
        1,
    ))
}
