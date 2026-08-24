use harness_core::context_budget::{BudgetStatus, RequestBudgetSnapshot};
use ratatui::style::Color;

use crate::{app::AppState, theme::Theme};

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
    label: String,
    tone: ContextBudgetTone,
}

impl ContextBudget {
    pub(super) fn from_app(app: &AppState) -> Option<Self> {
        if app
            .active_context_usage()
            .is_some_and(|usage| usage.compacted_pending_refresh)
        {
            return Some(Self {
                label: "ctx compacted · refreshing".to_string(),
                tone: ContextBudgetTone::Refreshing,
            });
        }

        if let Some(snapshot) = app.current_request_budget_snapshot() {
            return Some(Self::from_snapshot(snapshot));
        }
        app.uses_unknown_budget_fallback().then(|| Self {
            label: format!(
                "ctx ~{} · capacity unknown",
                app.active_context_usage()
                    .and_then(|usage| usage.tokens)
                    .unwrap_or(0)
            ),
            tone: ContextBudgetTone::Unknown,
        })
    }

    fn from_snapshot(snapshot: RequestBudgetSnapshot) -> Self {
        match snapshot.status {
            BudgetStatus::Estimated => Self::estimated(snapshot),
            BudgetStatus::ConservativeFallback => Self {
                label: format!("ctx ~{} · conservative", snapshot.occupied_input_tokens),
                tone: if snapshot.requires_compaction == Some(true) {
                    ContextBudgetTone::Warning
                } else {
                    ContextBudgetTone::Unknown
                },
            },
            BudgetStatus::UnknownLimits => Self {
                label: format!("ctx ~{} · capacity unknown", snapshot.occupied_input_tokens),
                tone: ContextBudgetTone::Unknown,
            },
        }
    }

    fn estimated(snapshot: RequestBudgetSnapshot) -> Self {
        let Some(threshold) = snapshot
            .compaction_threshold_tokens
            .filter(|value| *value > 0)
        else {
            return Self {
                label: format!("ctx ~{} · capacity unknown", snapshot.occupied_input_tokens),
                tone: ContextBudgetTone::Unknown,
            };
        };
        let occupied = snapshot.occupied_input_tokens;
        let percent = ((u64::from(occupied) * 100 + u64::from(threshold) / 2)
            / u64::from(threshold))
        .min(999);
        let pressure = u64::from(occupied) * 100;
        let threshold = u64::from(threshold);
        Self {
            label: format!("ctx ~{occupied}/{threshold} {percent}%"),
            tone: if snapshot.requires_compaction == Some(true) || pressure >= threshold * 90 {
                ContextBudgetTone::Critical
            } else if pressure >= threshold * 75 {
                ContextBudgetTone::Warning
            } else {
                ContextBudgetTone::Normal
            },
        }
    }

    pub(super) fn full_label(&self) -> &str {
        &self.label
    }

    pub(super) fn compact_label(&self) -> &str {
        &self.label
    }

    pub(super) const fn tone(&self) -> ContextBudgetTone {
        self.tone
    }
}

impl AppState {
    pub fn runtime_context_budget_text(&self) -> Option<String> {
        ContextBudget::from_app(self).map(|budget| budget.label)
    }
}

#[cfg(test)]
mod tests {
    use harness_core::context_budget::{
        BudgetStatus, RequestBudgetComponents, RequestBudgetSnapshot,
    };
    use harness_providers::ProviderOutputCapDisposition;

    use super::{ContextBudget, ContextBudgetTone};

    fn snapshot(
        status: BudgetStatus,
        occupied_input_tokens: u32,
        compaction_threshold_tokens: Option<u32>,
    ) -> RequestBudgetSnapshot {
        RequestBudgetSnapshot {
            status,
            requested_output_tokens: None,
            reserved_output_tokens: None,
            maximum_input_tokens: compaction_threshold_tokens,
            safety_margin_tokens: 0,
            compaction_threshold_tokens,
            components: RequestBudgetComponents::default(),
            occupied_input_tokens,
            remaining_input_tokens: None,
            requires_compaction: compaction_threshold_tokens
                .map(|threshold| occupied_input_tokens >= threshold),
            output_cap_disposition: ProviderOutputCapDisposition::UnspecifiedUnknownLimit,
        }
    }

    #[test]
    fn estimated_budget_uses_snapshot_threshold_for_label_and_tone() {
        // arrange: occupied input at the warning boundary of the shared threshold.
        let snapshot = snapshot(BudgetStatus::Estimated, 900, Some(1_200));

        // act: the snapshot is formatted.
        let budget = ContextBudget::from_snapshot(snapshot);

        // assert: threshold drives both the exact label and presentation tone.
        assert_eq!(budget.full_label(), "ctx ~900/1200 75%");
        assert_eq!(budget.tone(), ContextBudgetTone::Warning);
    }

    #[test]
    fn unknown_and_conservative_budgets_never_render_percentages() {
        // arrange: snapshots without estimated capacity.
        let unknown = snapshot(BudgetStatus::UnknownLimits, 321, None);
        let conservative = snapshot(BudgetStatus::ConservativeFallback, 400, Some(500));

        // act: both snapshots are formatted.
        let labels = [
            ContextBudget::from_snapshot(unknown).label,
            ContextBudget::from_snapshot(conservative).label,
        ];

        // assert: status is explicit without fabricated percentages.
        assert_eq!(
            labels,
            ["ctx ~321 · capacity unknown", "ctx ~400 · conservative"]
        );
        assert!(labels.iter().all(|label| !label.contains('%')));
    }
}
