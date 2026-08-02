use ratatui::style::{Color, Style};
use ratatui::text::{Line, Span};

use crate::theme::Theme;

use super::ui_transcript_style::blend_color;

const SHIMMER_FPS: f32 = 12.0;
const SWEEP_CYCLE_SECONDS: f32 = 4.0;
const SWEEP_ACTIVE_FRACTION: f32 = 0.32;
const SWEEP_HALF_WIDTH: f32 = 0.38;
const SWEEP_STRENGTH: f32 = 0.33;
const PULSE_STRENGTH: f32 = 0.06;
const PULSE_SECONDS: f32 = 5.0;

pub(super) fn animated_logo_line(
    row: usize,
    rows: usize,
    text: &str,
    phase: usize,
    theme: &Theme,
    background: Color,
) -> Line<'static> {
    let columns = text.chars().count().max(1);
    let phase = if std::env::var_os("HARNESS_DISABLE_ANIMATIONS").is_some() {
        0
    } else {
        phase
    };
    let seconds = shimmer_elapsed_seconds(phase);
    let mut spans = Vec::new();
    let mut run = String::new();
    let mut run_color = None;

    for (column, character) in text.chars().enumerate() {
        let diagonal = (bounded_f32(column) + bounded_f32(rows.saturating_sub(row + 1)))
            / bounded_f32(columns + rows.max(1));
        let color = animated_color(
            theme.text.secondary,
            theme.text.primary,
            shimmer_opacity(diagonal, seconds),
        );
        if run_color != Some(color) {
            if let Some(previous) = run_color {
                spans.push(Span::styled(
                    std::mem::take(&mut run),
                    Style::default().fg(previous).bg(background),
                ));
            }
            run_color = Some(color);
        }
        run.push(character);
    }
    if let Some(previous) = run_color {
        spans.push(Span::styled(
            run,
            Style::default().fg(previous).bg(background),
        ));
    }
    Line::from(spans)
}

fn shimmer_elapsed_seconds(phase: usize) -> f32 {
    bounded_f32(phase) / SHIMMER_FPS
}

fn animated_color(base: Color, highlight: Color, opacity: f32) -> Color {
    if matches!(base, Color::Rgb(_, _, _)) && matches!(highlight, Color::Rgb(_, _, _)) {
        blend_color(base, highlight, opacity)
    } else if opacity >= 0.15 {
        highlight
    } else {
        base
    }
}

fn shimmer_opacity(diagonal: f32, seconds: f32) -> f32 {
    let cycle_phase = (seconds % SWEEP_CYCLE_SECONDS) / SWEEP_CYCLE_SECONDS;
    let sweep_phase = (cycle_phase / SWEEP_ACTIVE_FRACTION).min(1.0);
    let band_position = -SWEEP_HALF_WIDTH + sweep_phase * (1.0 + 2.0 * SWEEP_HALF_WIDTH);
    let pulse =
        PULSE_STRENGTH * (0.5 - 0.5 * (std::f32::consts::TAU * seconds / PULSE_SECONDS).cos());
    let distance = (diagonal - band_position).abs();
    let shine = if distance < SWEEP_HALF_WIDTH {
        0.5 * (1.0 + ((std::f32::consts::TAU / 2.0) * distance / SWEEP_HALF_WIDTH).cos())
    } else {
        0.0
    };
    (pulse + SWEEP_STRENGTH * shine).clamp(0.0, 1.0)
}

fn bounded_f32(value: usize) -> f32 {
    f32::from(u16::try_from(value).unwrap_or(u16::MAX))
}

#[cfg(test)]
mod tests {
    use ratatui::style::Color;

    use super::{animated_color, shimmer_elapsed_seconds, shimmer_opacity};

    #[test]
    fn shimmer_opacity_stays_bounded() {
        for tick in 0_u16..100 {
            for position in 0_u16..=20 {
                let opacity = shimmer_opacity(f32::from(position) / 20.0, f32::from(tick) * 0.1);
                assert!((0.0..=1.0).contains(&opacity));
            }
        }
    }

    #[test]
    fn named_terminal_colors_have_visible_animation_states() {
        assert_eq!(animated_color(Color::Gray, Color::White, 0.0), Color::Gray);
        assert_eq!(animated_color(Color::Gray, Color::White, 0.5), Color::White);
    }

    #[test]
    fn shimmer_uses_the_slow_twelve_fps_startup_phase() {
        assert!((shimmer_elapsed_seconds(12) - 1.0).abs() < f32::EPSILON);
        assert!(shimmer_elapsed_seconds(600) > 49.0);
    }
}
