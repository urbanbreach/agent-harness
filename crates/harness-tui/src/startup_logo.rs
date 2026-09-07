use ratatui::style::{Color, Style};
use ratatui::text::Span;

const FULL_WIDTH: usize = 9;
const FULL_ROWS: [&str; 7] = [
    " ██╗  ██╗",
    " ██║  ██║",
    " ██║  ██║",
    " ███████║",
    " ██╔══██║",
    " ██║  ██║",
    " ╚═╝  ╚═╝",
];

#[derive(Clone, Copy)]
pub(crate) struct Logo {
    rows: &'static [&'static str],
    width: usize,
}

impl Logo {
    pub(crate) const fn rows(self) -> &'static [&'static str] {
        self.rows
    }

    pub(crate) const fn width(self) -> usize {
        self.width
    }

    pub(crate) const fn height(self) -> usize {
        self.rows.len()
    }
}

const FULL_LOGO: Logo = Logo {
    rows: &FULL_ROWS,
    width: FULL_WIDTH,
};

pub(crate) fn row_spans(logo: Logo, row_index: usize, color: Color) -> Vec<Span<'static>> {
    vec![Span::styled(
        padded_row(logo.rows()[row_index], logo.width()),
        Style::default().fg(color),
    )]
}

pub(crate) const fn full_logo(glyphs_supported: bool) -> Option<Logo> {
    if glyphs_supported {
        Some(FULL_LOGO)
    } else {
        None
    }
}

fn padded_row(row: &str, width: usize) -> String {
    let mut output = row.to_string();
    while output.chars().count() < width {
        output.push(' ');
    }
    output.chars().take(width).collect()
}

const SMALL_ROWS: [&str; 3] = ["██  ██", "██████", "██  ██"];
pub(crate) fn for_height(height: u16, supported: bool) -> Option<Logo> {
    if !supported || height < 22 {
        None
    } else if height < 26 {
        Some(Logo {
            rows: &SMALL_ROWS,
            width: 6,
        })
    } else {
        full_logo(true)
    }
}

/// Same 4-second diagonal sweep and 12 Hz cadence as the reference,
/// applied to Harness's H artwork. Reduced motion holds the resting color.
pub(crate) fn shimmer_row(
    logo: Logo,
    row: usize,
    elapsed: std::time::Duration,
    motion: bool,
    theme: &crate::theme::Theme,
) -> Vec<Span<'static>> {
    let secs = elapsed.as_secs_f32();
    let phase = ((secs % 4.0) / 1.28).min(1.0);
    let position = -0.38 + phase * 1.76;
    let pulse = 0.06 * (0.5 - 0.5 * (std::f32::consts::TAU * secs / 5.0).cos());
    let base = theme.text.secondary;
    let bright = theme.text.primary;
    let mut spans: Vec<Span<'static>> = Vec::new();
    for (col, ch) in logo.rows()[row].chars().enumerate() {
        let diagonal = f32::from(u16::try_from(col + logo.height() - 1 - row).unwrap_or(u16::MAX))
            / f32::from(u16::try_from(logo.width() + logo.height()).unwrap_or(u16::MAX));
        let distance = (diagonal - position).abs();
        let shine = if distance < 0.38 {
            0.5 * (1.0 + (std::f32::consts::PI * distance / 0.38).cos())
        } else {
            0.0
        };
        let opacity = if motion {
            (pulse + 0.33 * shine).clamp(0.0, 1.0)
        } else {
            0.0
        };
        let color = match (
            crate::theme::resolve_to_rgb(base),
            crate::theme::resolve_to_rgb(bright),
        ) {
            (Some((r, g, b)), Some((hr, hg, hb))) => crate::theme::quantize_color(
                Color::Rgb(
                    blend(r, hr, opacity),
                    blend(g, hg, opacity),
                    blend(b, hb, opacity),
                ),
                theme.color_level(),
            ),
            _ => base,
        };
        if let Some(span) = spans.last_mut().filter(|span| span.style.fg == Some(color)) {
            span.content.to_mut().push(ch);
        } else {
            spans.push(Span::styled(ch.to_string(), Style::default().fg(color)));
        }
    }
    spans
}

#[expect(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "rounded interpolation of u8 endpoints with a clamped weight stays within 0..=255"
)]
fn blend(a: u8, b: u8, opacity: f32) -> u8 {
    (f32::from(a) + (f32::from(b) - f32::from(a)) * opacity.clamp(0.0, 1.0)).round() as u8
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hidden_logo_collapses_its_layout_width() {
        assert!(full_logo(false).is_none());
    }
}
