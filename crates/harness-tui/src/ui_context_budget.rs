//! Breadcrumb context segment: auto-compaction icon, `42.0%/200K` text and threshold gauge,
//! all drawn from the shared [`ContextStatus`].
use ratatui::style::{Color, Style};
use ratatui::text::Span;

use crate::app::context_status::{AutoState, ContextSeverity, ContextStatus, ContextSymbols};
use crate::app::AppState;
use crate::theme::Theme;

/// Cells of the threshold gauge.
const GAUGE_CELLS: u16 = 10;

/// How much of the segment fits: the gauge drops first, then the auto-compaction icon.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum ContextDetail {
    Full,
    NoGauge,
    TextOnly,
}

impl ContextDetail {
    pub(super) const ALL: [Self; 3] = [Self::Full, Self::NoGauge, Self::TextOnly];
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Tone {
    Severity(ContextSeverity),
    Accent,
    Muted,
}

#[derive(Debug, Clone)]
pub(super) struct ContextChrome {
    auto: Option<(String, Tone)>,
    text: String,
    gauge: Option<String>,
    severity: ContextSeverity,
}

impl ContextChrome {
    pub(super) fn from_app(app: &AppState) -> Option<Self> {
        let symbols = app.context_symbols();
        let mut status = app.context_status()?;
        if let Some(occupied) = settling_occupied(app) {
            status = status.with_used_tokens(occupied, &symbols);
        }
        Some(Self::new(app, &status, &symbols))
    }

    fn new(app: &AppState, status: &ContextStatus, symbols: &ContextSymbols) -> Self {
        let auto = status.auto().map(|state| {
            let tone = match state {
                // Pulses between accent and muted; a still accent under reduced motion.
                AutoState::Running
                    if app.transcript_motion_enabled()
                        && !app.transcript_animation_phase().is_multiple_of(2) =>
                {
                    Tone::Muted
                }
                AutoState::Running | AutoState::Armed => Tone::Accent,
                AutoState::Idle => Tone::Severity(status.severity()),
            };
            (symbols.auto.to_owned(), tone)
        });
        Self {
            auto,
            text: status.text(symbols),
            gauge: status.gauge_text(GAUGE_CELLS, symbols),
            severity: status.severity(),
        }
    }

    fn parts(&self, detail: ContextDetail) -> Vec<(&str, Tone)> {
        let mut parts = Vec::with_capacity(3);
        if detail != ContextDetail::TextOnly
            && let Some((icon, tone)) = &self.auto
        {
            parts.push((icon.as_str(), *tone));
        }
        parts.push((self.text.as_str(), Tone::Severity(self.severity)));
        if detail == ContextDetail::Full
            && let Some(gauge) = &self.gauge
        {
            parts.push((gauge.as_str(), Tone::Severity(self.severity)));
        }
        parts
    }

    pub(super) fn plain(&self, detail: ContextDetail) -> String {
        self.parts(detail)
            .into_iter()
            .map(|(text, _)| text)
            .collect::<Vec<_>>()
            .join(" ")
    }

    /// Spans whose concatenated text equals [`Self::plain`].
    pub(super) fn spans(
        &self,
        detail: ContextDetail,
        theme: &Theme,
        background: Color,
    ) -> Vec<Span<'static>> {
        let mut spans = Vec::new();
        for (index, (text, tone)) in self.parts(detail).into_iter().enumerate() {
            let content = if index == 0 {
                text.to_owned()
            } else {
                format!(" {text}")
            };
            spans.push(Span::styled(
                content,
                Style::default().fg(tone_color(tone, theme)).bg(background),
            ));
        }
        spans
    }
}

fn tone_color(tone: Tone, theme: &Theme) -> Color {
    match tone {
        Tone::Accent => theme.text.accent,
        Tone::Muted => theme.text.tertiary,
        Tone::Severity(ContextSeverity::Normal) => theme.text.primary,
        Tone::Severity(ContextSeverity::Warning) => theme.status.warning,
        // The theme has no dedicated purple status role; the plan agent colour is its purple.
        Tone::Severity(ContextSeverity::Elevated) => theme.agents.plan,
        Tone::Severity(ContextSeverity::Error) => theme.status.error,
    }
}

/// While a finished compaction card eases its context bar down, the segment follows it.
fn settling_occupied(app: &AppState) -> Option<u32> {
    let card = app.displayed_compaction()?;
    let stage = card.stage.as_ref()?;
    if stage.phase != harness_core::event::CompactionPhase::Done
        || stage.window_tokens.is_none_or(|window| window == 0)
    {
        return None;
    }
    let before = stage.tokens_before?;
    let elapsed = u64::try_from(
        app.now()
            .saturating_duration_since(card.updated_at)
            .as_millis(),
    )
    .unwrap_or(u64::MAX);
    Some(super::ui_compaction_motion::remaining(
        before,
        stage.tokens_after.unwrap_or(before),
        elapsed,
        app.transcript_motion_enabled(),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::context_status::{ContextInputs, PublishedAuto};

    fn chrome(
        inputs: ContextInputs,
        symbols: &ContextSymbols,
        motion_phase: usize,
    ) -> ContextChrome {
        let status = ContextStatus::new(inputs, symbols).expect("known");
        let app = AppState::new_live(None, false, None);
        let mut chrome = ContextChrome::new(&app, &status, symbols);
        // The pulse reads the live animation phase; cover it through the tone directly.
        if let Some((_, tone)) = chrome.auto.as_mut()
            && status.auto() == Some(AutoState::Running)
        {
            *tone = if motion_phase.is_multiple_of(2) {
                Tone::Accent
            } else {
                Tone::Muted
            };
        }
        chrome
    }

    fn inputs(used: u32) -> ContextInputs {
        with_state(used, AutoState::Idle)
    }

    fn with_state(used: u32, state: AutoState) -> ContextInputs {
        ContextInputs {
            used_tokens: Some(used),
            window_tokens: Some(200_000),
            auto: Some(PublishedAuto {
                threshold_tokens: Some(160_000),
                speculation_tokens: Some(140_000),
                state,
            }),
        }
    }

    #[test]
    fn detail_levels_drop_the_gauge_then_the_auto_icon() {
        let unicode = chrome(inputs(84_000), &ContextSymbols::UNICODE, 0);
        assert_eq!(
            unicode.plain(ContextDetail::Full),
            "⟲ ◫ 42.0%/200K ████░░░╎┃░"
        );
        assert_eq!(unicode.plain(ContextDetail::NoGauge), "⟲ ◫ 42.0%/200K");
        assert_eq!(unicode.plain(ContextDetail::TextOnly), "◫ 42.0%/200K");
        let ascii = chrome(inputs(84_000), &ContextSymbols::ASCII, 0);
        assert_eq!(
            ascii.plain(ContextDetail::Full),
            "[A] ctx: 42.0%/200K ####---:|-"
        );
    }

    #[test]
    fn auto_icon_tone_pulses_while_running_and_follows_severity_when_idle() {
        let idle = chrome(inputs(184_000), &ContextSymbols::UNICODE, 0);
        assert_eq!(
            idle.auto.as_ref().map(|(_, tone)| *tone),
            Some(Tone::Severity(ContextSeverity::Error))
        );
        let running = with_state(10_000, AutoState::Running);
        let tones = [0, 1].map(|phase| {
            chrome(running, &ContextSymbols::UNICODE, phase)
                .auto
                .map(|(_, tone)| tone)
        });
        assert_eq!(tones, [Some(Tone::Accent), Some(Tone::Muted)]);
        let armed = chrome(
            with_state(150_000, AutoState::Armed),
            &ContextSymbols::UNICODE,
            0,
        );
        assert_eq!(
            armed.auto.as_ref().map(|(_, tone)| *tone),
            Some(Tone::Accent)
        );
        let disabled = ContextInputs {
            auto: None,
            ..inputs(10_000)
        };
        assert!(chrome(disabled, &ContextSymbols::UNICODE, 0).auto.is_none());
    }
}
