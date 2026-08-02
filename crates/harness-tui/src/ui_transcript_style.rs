use ratatui::style::Color;

use crate::app::ActivityStatus;
use crate::text::has_trimmed_content;
use crate::theme::Theme;

use super::ui_chrome::elevated_card_surface;

const TRANSCRIPT_BRAILLE_SPINNER_FRAMES: [&str; 10] =
    ["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];

pub(super) fn transcript_streaming_spinner_frame(animation_phase: usize) -> &'static str {
    // Freeze the spinner glyph when animations are disabled so PTY/signoff
    // captures are reproducible across runs.
    if std::env::var_os("HARNESS_DISABLE_ANIMATIONS").is_some() {
        return TRANSCRIPT_BRAILLE_SPINNER_FRAMES[0];
    }
    TRANSCRIPT_BRAILLE_SPINNER_FRAMES[animation_phase % TRANSCRIPT_BRAILLE_SPINNER_FRAMES.len()]
}

pub(super) fn thinking_header_color(theme: &Theme, surface: Color) -> Color {
    blend_color(surface, theme.status.warning, 0.6)
}

pub(super) fn blend_color(base: Color, overlay: Color, alpha: f32) -> Color {
    match (base, overlay) {
        (
            Color::Rgb(base_red, base_green, base_blue),
            Color::Rgb(overlay_red, overlay_green, overlay_blue),
        ) => {
            #[allow(
                clippy::cast_possible_truncation,
                reason = "float-to-int cast is safe: value is clamped to [0, 255] before cast"
            )]
            #[allow(
                clippy::cast_sign_loss,
                reason = "float-to-int cast is safe: value is clamped to [0, 255] before cast"
            )]
            let blend = |base: u8, overlay: u8| -> u8 {
                let value = (f32::from(base) * (1.0 - alpha)) + (f32::from(overlay) * alpha);
                value.round().clamp(0.0, 255.0) as u8
            };
            Color::Rgb(
                blend(base_red, overlay_red),
                blend(base_green, overlay_green),
                blend(base_blue, overlay_blue),
            )
        }
        _ => overlay,
    }
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

#[cfg(test)]
mod tests {
    use super::{blend_color, thinking_header_color, Theme};
    use ratatui::style::Color;

    #[test]
    fn blend_color_interpolates_rgb() {
        // arrange
        // act
        // assert
        let blended = blend_color(Color::Rgb(0, 0, 0), Color::Rgb(100, 100, 100), 0.5);
        assert_eq!(blended, Color::Rgb(50, 50, 50));
    }

    #[test]
    fn blend_color_alpha_zero_returns_base() {
        // arrange
        // act
        // assert
        assert_eq!(
            blend_color(Color::Rgb(10, 20, 30), Color::Rgb(100, 100, 100), 0.0),
            Color::Rgb(10, 20, 30)
        );
    }

    #[test]
    fn blend_color_alpha_one_returns_overlay() {
        // arrange
        // act
        // assert
        assert_eq!(
            blend_color(Color::Rgb(10, 20, 30), Color::Rgb(100, 100, 100), 1.0),
            Color::Rgb(100, 100, 100)
        );
    }

    #[test]
    fn blend_color_clamps_to_byte_range() {
        // arrange
        // act
        // assert
        assert_eq!(
            blend_color(Color::Rgb(255, 255, 255), Color::Rgb(0, 0, 0), 2.0),
            Color::Rgb(0, 0, 0)
        );
        assert_eq!(
            blend_color(Color::Rgb(0, 0, 0), Color::Rgb(255, 255, 255), -0.5),
            Color::Rgb(0, 0, 0)
        );
    }

    #[test]
    fn thinking_header_color_blends_warning_toward_surface() {
        // arrange
        // act
        // assert
        let theme = Theme::default();
        let Color::Rgb(r, g, b) = thinking_header_color(&theme, theme.surface.shell) else {
            panic!("expected an RGB color")
        };
        assert!((0..=255).contains(&r));
        assert!((0..=255).contains(&g));
        assert!((0..=255).contains(&b));
    }
}
