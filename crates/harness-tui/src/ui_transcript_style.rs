use ratatui::style::Color;

use crate::app::ActivityStatus;
use crate::text::has_trimmed_content;
use crate::theme::Theme;

use super::ui_chrome::elevated_card_surface;

const TRANSCRIPT_BRAILLE_SPINNER_FRAMES: [&str; 10] =
    ["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];

pub(super) fn transcript_streaming_spinner_frame(animation_phase: usize) -> &'static str {
    TRANSCRIPT_BRAILLE_SPINNER_FRAMES[animation_phase % TRANSCRIPT_BRAILLE_SPINNER_FRAMES.len()]
}

pub(super) fn assistant_footer_label(value: &str) -> String {
    if !has_trimmed_content(value)
        || value.eq_ignore_ascii_case("unknown")
        || value.eq_ignore_ascii_case("default")
    {
        return "Assistant".to_string();
    }
    titlecase_label(value.trim())
}

fn titlecase_label(value: &str) -> String {
    let mut chars = value.chars();
    let Some(first) = chars.next() else {
        return String::new();
    };
    format!("{}{}", first.to_uppercase(), chars.as_str())
}

pub(super) fn assistant_primary_label_color(status: ActivityStatus, theme: &Theme) -> Color {
    match status {
        ActivityStatus::Queued => theme.text.secondary,
        ActivityStatus::Streaming => theme.text.primary,
        ActivityStatus::Done => theme.text.primary,
        ActivityStatus::Error => theme.status.error,
    }
}

pub(super) fn activity_status_supports_footer_only(status: ActivityStatus) -> bool {
    matches!(status, ActivityStatus::Streaming)
}

pub(super) fn selected_foreground_for_badge(background: Color, theme: &Theme) -> Color {
    match background {
        Color::Rgb(red, green, blue) => {
            let luminance =
                0.299 * f64::from(red) + 0.587 * f64::from(green) + 0.114 * f64::from(blue);
            if luminance > 127.5 {
                theme.text.inverse
            } else {
                theme.text.primary
            }
        }
        _ => theme.text.inverse,
    }
}

pub(super) fn assistant_primary_rail_color(
    status: ActivityStatus,
    profile_label: &str,
    theme: &Theme,
) -> Color {
    match status {
        ActivityStatus::Queued | ActivityStatus::Streaming | ActivityStatus::Done => {
            theme.agent_accent(profile_label)
        }
        ActivityStatus::Error => theme.status.error,
    }
}

pub(super) fn transcript_nested_rail_color(theme: &Theme) -> Color {
    theme.text.secondary
}

pub(super) fn transcript_emphasized_surface(theme: &Theme, base_surface: Color) -> Color {
    if base_surface == theme.surface.panel {
        elevated_card_surface(theme)
    } else {
        theme.surface.panel
    }
}
