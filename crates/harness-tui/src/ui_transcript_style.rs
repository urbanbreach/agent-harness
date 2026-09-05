use ratatui::style::Color;

use crate::app::ActivityStatus;
use crate::theme::Theme;

use super::ui_chrome::elevated_card_surface;

const TRANSCRIPT_BRAILLE_SPINNER_FRAMES: [&str; 8] = ["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧"];
const TRANSCRIPT_SPINNER_TICK_DIVISOR: usize = 4;
const MONITOR_PULSE_FRAMES: [&str; 4] = ["○", "◎", "◉", "◎"];
const MONITOR_PULSE_TICK_DIVISOR: usize = 8;
const TRANSCRIPT_TOOL_WAVE_SPEED: f32 = 0.15;
const USER_WAITING_PULSE_SPEED: f32 = 0.08;
const ANIMATION_PHASE_WRAP: usize = 65_536;

fn animation_phase_f32(animation_phase: usize) -> f32 {
    let wrapped = animation_phase % ANIMATION_PHASE_WRAP;
    f32::from(u16::try_from(wrapped).unwrap_or_default())
}

pub(super) fn glyph_routed_streaming_spinner_frame(
    theme: &Theme,
    animation_phase: usize,
    motion_enabled: bool,
) -> &'static str {
    if theme.glyph_mode() == crate::theme::GlyphMode::Ascii {
        return theme.live_shell.glyphs.streaming;
    }
    if !motion_enabled {
        return TRANSCRIPT_BRAILLE_SPINNER_FRAMES[0];
    }
    let frame = animation_phase / TRANSCRIPT_SPINNER_TICK_DIVISOR;
    TRANSCRIPT_BRAILLE_SPINNER_FRAMES[frame % TRANSCRIPT_BRAILLE_SPINNER_FRAMES.len()]
}

pub(super) fn glyph_routed_monitor_pulse_frame(
    theme: &Theme,
    animation_phase: usize,
    motion_enabled: bool,
) -> &'static str {
    if theme.glyph_mode() == crate::theme::GlyphMode::Ascii {
        return theme.live_shell.glyphs.running;
    }
    if !motion_enabled {
        return MONITOR_PULSE_FRAMES[0];
    }
    let frame = animation_phase / MONITOR_PULSE_TICK_DIVISOR;
    MONITOR_PULSE_FRAMES[frame % MONITOR_PULSE_FRAMES.len()]
}

pub(super) fn thinking_header_color(theme: &Theme, _surface: Color) -> Color {
    theme.text.secondary
}

pub(super) fn transcript_running_tool_marker_color(theme: &Theme, animation_phase: usize) -> Color {
    let sine = (animation_phase_f32(animation_phase) * TRANSCRIPT_TOOL_WAVE_SPEED).sin();
    blend_color(theme.surface.canvas, theme.text.accent, sine * sine)
}

pub(super) fn pending_diamond_color(theme: &Theme, animation_phase: usize) -> Color {
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

pub(super) fn assistant_footer_label(_value: &str) -> String {
    "Assistant".to_string()
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
        assistant_footer_label, blend_color, glyph_routed_monitor_pulse_frame,
        glyph_routed_streaming_spinner_frame, pending_diamond_color, thinking_header_color,
        transcript_running_tool_marker_color, Theme,
    };
    use crate::theme::GlyphMode;
    use ratatui::style::Color;

    #[test]
    fn blend_color_interpolates_rgb() {
        let blended = blend_color(Color::Rgb(0, 0, 0), Color::Rgb(100, 100, 100), 0.5);
        assert_eq!(blended, Color::Rgb(50, 50, 50));
    }

    #[test]
    fn blend_color_alpha_zero_returns_base() {
        assert_eq!(
            blend_color(Color::Rgb(10, 20, 30), Color::Rgb(100, 100, 100), 0.0),
            Color::Rgb(10, 20, 30)
        );
    }

    #[test]
    fn blend_color_alpha_one_returns_overlay() {
        assert_eq!(
            blend_color(Color::Rgb(10, 20, 30), Color::Rgb(100, 100, 100), 1.0),
            Color::Rgb(100, 100, 100)
        );
    }

    #[test]
    fn blend_color_clamps_to_byte_range() {
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
    fn thinking_header_color_uses_readable_secondary_text() {
        let theme = Theme::default();
        assert_eq!(
            thinking_header_color(&theme, theme.surface.shell),
            theme.text.secondary
        );
    }

    #[test]
    fn running_tool_marker_wave_advances() {
        // arrange
        // act
        let theme = Theme::default();
        // assert
        assert_ne!(
            transcript_running_tool_marker_color(&theme, 0),
            transcript_running_tool_marker_color(&theme, 10)
        );
    }

    #[test]
    fn pending_diamond_pulse_changes_color_across_ticks() {
        // arrange
        // act
        let theme = Theme::default();
        // assert
        assert_ne!(
            pending_diamond_color(&theme, 0),
            pending_diamond_color(&theme, 10)
        );
    }

    #[test]
    fn preferred_motion_glyphs_are_single_cell_unicode() {
        // Given: the preferred Harness glyph catalog.
        let theme = Theme::default();

        // When: active status frames are selected.
        let frames = [
            glyph_routed_streaming_spinner_frame(&theme, 4, true),
            glyph_routed_monitor_pulse_frame(&theme, 8, true),
        ];

        // Then: each preferred frame occupies exactly one terminal cell.
        assert!(frames
            .into_iter()
            .all(|frame| unicode_width::UnicodeWidthStr::width(frame) == 1));
    }

    #[test]
    fn ascii_motion_glyphs_are_ascii_safe() {
        // Given: the ASCII Harness glyph catalog.
        let theme = Theme::default().with_glyph_mode(GlyphMode::Ascii);

        // When: active status frames are selected at moving phases.
        let frames = [
            glyph_routed_streaming_spinner_frame(&theme, 4, true),
            glyph_routed_monitor_pulse_frame(&theme, 8, true),
        ];

        // Then: no terminal-sensitive Unicode escapes the capability boundary.
        assert!(frames.into_iter().all(str::is_ascii));
    }

    #[test]
    fn reduced_motion_keeps_routed_frames_static() {
        // Given: preferred and ASCII Harness glyph catalogs.
        let preferred = Theme::default();
        let ascii = preferred.with_glyph_mode(GlyphMode::Ascii);

        // When: reduced motion resolves distant animation phases.
        let frames = [
            (
                glyph_routed_streaming_spinner_frame(&preferred, 0, false),
                glyph_routed_streaming_spinner_frame(&preferred, 40, false),
            ),
            (
                glyph_routed_monitor_pulse_frame(&ascii, 0, false),
                glyph_routed_monitor_pulse_frame(&ascii, 40, false),
            ),
        ];

        // Then: both catalogs retain a static one-cell status cue.
        assert!(frames.into_iter().all(|(first, later)| first == later));
    }

    #[test]
    fn assistant_footer_label_is_profile_independent() {
        // arrange
        // Given: legacy primary-profile labels on recorded turns.
        // When: the fallback assistant label is derived.
        let labels = [
            assistant_footer_label("build"),
            assistant_footer_label("plan"),
        ];

        // act
        // Then: both turns retain the generic transcript message role.
        // assert
        assert_eq!(labels, ["Assistant", "Assistant"]);
    }
}
