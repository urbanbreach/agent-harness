use ratatui::style::Color;

use crate::app::ActivityStatus;
use crate::text::has_trimmed_content;
use crate::theme::Theme;

use super::ui_chrome::elevated_card_surface;

const TRANSCRIPT_BRAILLE_SPINNER_FRAMES: [&str; 8] = ["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧"];
const TRANSCRIPT_SPINNER_TICK_DIVISOR: usize = 4;
const TRANSCRIPT_ANIMATION_TICKS_PER_SECOND: u64 = 30;
const TRANSCRIPT_ACTIVITY_SPINNER_ORIGIN_FRAME: usize = 3;
const TRANSCRIPT_TOOL_WAVE_SPEED: f32 = 0.15;
const USER_WAITING_PULSE_SPEED: f32 = 0.08;
const ANIMATION_PHASE_WRAP: usize = 65_536;

fn animation_phase_f32(animation_phase: usize) -> f32 {
    let wrapped = animation_phase % ANIMATION_PHASE_WRAP;
    f32::from(u16::try_from(wrapped).unwrap_or_default())
}

pub(super) fn transcript_streaming_spinner_frame(animation_phase: usize) -> &'static str {
    // Freeze the spinner glyph when animations are disabled so PTY/signoff
    // captures are reproducible across runs.
    if std::env::var_os("HARNESS_DISABLE_ANIMATIONS").is_some() {
        return TRANSCRIPT_BRAILLE_SPINNER_FRAMES[0];
    }
    let frame = animation_phase / TRANSCRIPT_SPINNER_TICK_DIVISOR;
    TRANSCRIPT_BRAILLE_SPINNER_FRAMES[frame % TRANSCRIPT_BRAILLE_SPINNER_FRAMES.len()]
}

pub(super) fn transcript_activity_spinner_frame(elapsed_ms: u64) -> &'static str {
    let elapsed_ticks = elapsed_ms.saturating_mul(TRANSCRIPT_ANIMATION_TICKS_PER_SECOND) / 1_000;
    let elapsed_ticks = usize::try_from(elapsed_ticks).unwrap_or(usize::MAX);
    let origin_ticks = TRANSCRIPT_ACTIVITY_SPINNER_ORIGIN_FRAME * TRANSCRIPT_SPINNER_TICK_DIVISOR;
    transcript_streaming_spinner_frame(origin_ticks.saturating_add(elapsed_ticks))
}

pub(super) fn thinking_header_color(theme: &Theme, _surface: Color) -> Color {
    theme.text.tertiary
}

pub(super) fn transcript_running_tool_marker_color(theme: &Theme, animation_phase: usize) -> Color {
    if std::env::var_os("HARNESS_DISABLE_ANIMATIONS").is_some() {
        return theme.text.accent;
    }
    let sine = (animation_phase_f32(animation_phase) * TRANSCRIPT_TOOL_WAVE_SPEED).sin();
    blend_color(theme.surface.canvas, theme.text.accent, sine * sine)
}

pub(super) fn pending_diamond_color(theme: &Theme, animation_phase: usize) -> Color {
    if std::env::var_os("HARNESS_DISABLE_ANIMATIONS").is_some() {
        return blend_color(theme.surface.canvas, theme.text.accent, 0.3);
    }
    let sine = (animation_phase_f32(animation_phase) * USER_WAITING_PULSE_SPEED).sin();
    blend_color(
        theme.surface.canvas,
        theme.text.accent,
        0.3 + sine * sine * 0.7,
    )
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
    use super::{
        blend_color, pending_diamond_color, thinking_header_color,
        transcript_running_tool_marker_color, Theme,
    };
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
    fn thinking_header_color_uses_muted_transcript_text() {
        // arrange
        // act
        // assert
        let theme = Theme::default();
        assert_eq!(
            thinking_header_color(&theme, theme.surface.shell),
            theme.text.tertiary
        );
    }

    #[test]
    fn running_tool_marker_wave_advances() {
        let theme = Theme::default();
        assert_ne!(
            transcript_running_tool_marker_color(&theme, 0),
            transcript_running_tool_marker_color(&theme, 10)
        );
    }

    #[test]
    fn pending_diamond_pulse_changes_color_across_ticks() {
        let theme = Theme::default();
        assert_ne!(
            pending_diamond_color(&theme, 0),
            pending_diamond_color(&theme, 10)
        );
    }
}
