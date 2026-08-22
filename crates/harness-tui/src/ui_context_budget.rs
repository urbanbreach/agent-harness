use ratatui::style::Color;

use crate::{app::AppState, theme::Theme};

const METER_CELLS: u64 = 6;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum ContextBudgetTone {
    Normal,
    Warning,
    Critical,
    Refreshing,
    Unknown,
}

impl ContextBudgetTone {
    pub(super) const fn color(self, theme: &Theme) -> Color {
        match self {
            Self::Normal => theme.status.success,
            Self::Warning => theme.status.warning,
            Self::Critical => theme.status.error,
            Self::Refreshing => theme.status.info,
            Self::Unknown => theme.text.tertiary,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct ContextBudget {
    full_label: String,
    compact_label: String,
    tone: ContextBudgetTone,
}

impl ContextBudget {
    pub(super) fn from_app(app: &AppState) -> Option<Self> {
        let usage = app.active_context_usage();
        let limit = app
            .current_context_window_tokens()
            .filter(|limit| *limit > 0);
        Self::from_values(
            usage.and_then(|usage| usage.tokens),
            limit,
            usage.is_some_and(|usage| usage.compacted_pending_refresh),
        )
    }

    fn from_values(tokens: Option<u32>, limit: Option<u32>, refreshing: bool) -> Option<Self> {
        if refreshing {
            return Some(Self {
                full_label: "ctx compacted · refreshing".to_string(),
                compact_label: "ctx compacted".to_string(),
                tone: ContextBudgetTone::Refreshing,
            });
        }

        match (tokens, limit) {
            (Some(tokens), Some(limit)) => {
                let percent =
                    ((u64::from(tokens) * 100 + u64::from(limit) / 2) / u64::from(limit)).min(999);
                let pressure = u64::from(tokens) * 100;
                let critical_threshold = u64::from(limit) * 90;
                let warning_threshold = u64::from(limit) * 75;
                let filled = (u64::from(tokens) * METER_CELLS)
                    .div_ceil(u64::from(limit))
                    .min(METER_CELLS);
                let meter = format!(
                    "[{}{}]",
                    "#".repeat(usize::try_from(filled).unwrap_or(6)),
                    "-".repeat(usize::try_from(METER_CELLS - filled).unwrap_or(0))
                );
                Some(Self {
                    full_label: format!(
                        "ctx {}/{} {percent}% {meter}",
                        compact_tokens(tokens),
                        compact_tokens(limit)
                    ),
                    compact_label: format!("ctx {percent}%"),
                    tone: if pressure >= critical_threshold {
                        ContextBudgetTone::Critical
                    } else if pressure >= warning_threshold {
                        ContextBudgetTone::Warning
                    } else {
                        ContextBudgetTone::Normal
                    },
                })
            }
            (Some(tokens), None) => Some(Self {
                full_label: format!("ctx {}", compact_tokens(tokens)),
                compact_label: format!("ctx {}", compact_tokens(tokens)),
                tone: ContextBudgetTone::Unknown,
            }),
            (None, Some(limit)) => Some(Self {
                full_label: format!("ctx ?/{}", compact_tokens(limit)),
                compact_label: "ctx ?".to_string(),
                tone: ContextBudgetTone::Unknown,
            }),
            (None, None) => None,
        }
    }

    pub(super) fn full_label(&self) -> &str {
        &self.full_label
    }

    pub(super) fn compact_label(&self) -> &str {
        &self.compact_label
    }

    pub(super) const fn tone(&self) -> ContextBudgetTone {
        self.tone
    }
}

fn compact_tokens(tokens: u32) -> String {
    if tokens >= 1_000_000 {
        format!("{:.1}M", f64::from(tokens) / 1_000_000.0)
    } else if tokens >= 1_000 {
        format!("{:.1}K", f64::from(tokens) / 1_000.0)
    } else {
        tokens.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::{ContextBudget, ContextBudgetTone};
    use crate::UnwrapOrAbort;

    #[test]
    fn context_budget_maps_thresholds_and_six_cell_meter() {
        // arrange
        let normal =
            ContextBudget::from_values(Some(32_000), Some(128_000), false).unwrap_or_abort();
        let warning =
            ContextBudget::from_values(Some(96_000), Some(128_000), false).unwrap_or_abort();
        let critical =
            ContextBudget::from_values(Some(116_000), Some(128_000), false).unwrap_or_abort();

        // act
        let labels = [
            normal.full_label(),
            warning.full_label(),
            critical.full_label(),
        ];

        // assert
        assert_eq!(labels[0], "ctx 32.0K/128.0K 25% [##----]");
        assert_eq!(normal.tone(), ContextBudgetTone::Normal);
        assert_eq!(labels[1], "ctx 96.0K/128.0K 75% [#####-]");
        assert_eq!(warning.tone(), ContextBudgetTone::Warning);
        assert_eq!(labels[2], "ctx 116.0K/128.0K 91% [######]");
        assert_eq!(critical.tone(), ContextBudgetTone::Critical);
    }

    #[test]
    fn context_budget_preserves_refreshing_and_unknown_states() {
        // arrange
        let refreshing = ContextBudget::from_values(None, Some(128_000), true).unwrap_or_abort();
        let unknown = ContextBudget::from_values(None, Some(128_000), false).unwrap_or_abort();

        // act
        let refreshing_label = refreshing.full_label();
        let unknown_label = unknown.full_label();

        // assert
        assert_eq!(refreshing_label, "ctx compacted · refreshing");
        assert_eq!(refreshing.compact_label(), "ctx compacted");
        assert_eq!(refreshing.tone(), ContextBudgetTone::Refreshing);
        assert_eq!(unknown_label, "ctx ?/128.0K");
        assert_eq!(unknown.tone(), ContextBudgetTone::Unknown);
        assert!(ContextBudget::from_values(None, None, false).is_none());
    }

    #[test]
    fn context_budget_classifies_pressure_from_exact_ratio() {
        // arrange
        let below_warning =
            ContextBudget::from_values(Some(74_900), Some(100_000), false).unwrap_or_abort();
        let warning =
            ContextBudget::from_values(Some(89_900), Some(100_000), false).unwrap_or_abort();
        let critical =
            ContextBudget::from_values(Some(90_000), Some(100_000), false).unwrap_or_abort();

        // act
        let tones = [below_warning.tone(), warning.tone(), critical.tone()];

        // assert
        assert_eq!(
            tones,
            [
                ContextBudgetTone::Normal,
                ContextBudgetTone::Warning,
                ContextBudgetTone::Critical,
            ]
        );
    }
}
