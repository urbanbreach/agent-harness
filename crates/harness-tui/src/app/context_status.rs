//! Collects the context status inputs from the app mirror of coordinator state.
mod value;

use harness_core::context_budget::{BudgetStatus, RequestBudgetSnapshot};
use harness_core::event::SpeculationState;

use super::AppState;
pub(crate) use value::{
    AutoState, ContextInputs, ContextSeverity, ContextStatus, ContextSymbols, PublishedAuto,
};

impl AppState {
    /// The shared status for every chrome that shows context; `None` without usage or window.
    ///
    /// The window prefers the compaction stage the coordinator published, then the active
    /// model's catalog window, then the latest request budget snapshot; the threshold and
    /// speculation boundaries are what the coordinator last published.
    pub(crate) fn context_status(&self) -> Option<ContextStatus> {
        ContextStatus::new(self.context_inputs(), &self.context_symbols())
    }

    pub(crate) fn context_symbols(&self) -> ContextSymbols {
        ContextSymbols::for_mode(self.theme().glyph_mode() == crate::theme::GlyphMode::Ascii)
    }

    /// Status text such as `42.0%/200K`.
    pub fn runtime_context_budget_text(&self) -> Option<String> {
        self.context_status()
            .map(|status| status.text(&self.context_symbols()))
    }

    fn context_inputs(&self) -> ContextInputs {
        let snapshot = self.current_request_budget_snapshot();
        let stage = self
            .displayed_compaction()
            .and_then(|card| card.stage.as_ref())
            .filter(|stage| {
                stage.tokens_before.is_some()
                    && stage.window_tokens.is_some_and(|window| window > 0)
            });
        let window = stage.and_then(|stage| stage.window_tokens).or_else(|| {
            self.catalog_context_window()
                .or_else(|| snapshot.and_then(snapshot_window))
        });
        let used_tokens = match (stage, self.active_context_usage()) {
            (Some(stage), _) => stage.tokens_before,
            (None, Some(usage)) if usage.compacted_pending_refresh => None,
            (None, Some(usage)) if usage.tokens.is_some() => usage.tokens,
            _ => snapshot.map(|snapshot| snapshot.occupied_input_tokens),
        };
        // An unknown window with nothing in context says nothing.
        let used_tokens = used_tokens.filter(|used| window.is_some() || *used > 0);
        ContextInputs {
            used_tokens,
            window_tokens: window,
            auto: self.published_auto(),
        }
    }

    /// The automatic-compaction status the coordinator last published for the shown agent.
    /// Nothing is assumed before one arrives or while automatic compaction is off.
    fn published_auto(&self) -> Option<PublishedAuto> {
        if self.replay_mode {
            return None;
        }
        let status = self.maintenance_status()?;
        let tokens =
            |value: Option<u64>| value.map(|tokens| u32::try_from(tokens).unwrap_or(u32::MAX));
        status.auto.then(|| PublishedAuto {
            threshold_tokens: tokens(status.threshold_tokens),
            speculation_tokens: tokens(status.speculation_tokens),
            state: match status.speculation {
                SpeculationState::Idle => AutoState::Idle,
                SpeculationState::Armed => AutoState::Armed,
                SpeculationState::Running => AutoState::Running,
            },
        })
    }

    /// Live sessions trust the model catalog; a replay only knows what the run recorded.
    fn catalog_context_window(&self) -> Option<u32> {
        (!self.replay_mode)
            .then(|| {
                self.runtime_context_metadata()
                    .model_limits()
                    .context_window_tokens()
            })
            .flatten()
            .filter(|window| *window > 0)
    }
}

/// Window of an estimated snapshot: input limit plus the reserved output. Other statuses
/// carry only a guessed capacity, which is not a window.
fn snapshot_window(snapshot: RequestBudgetSnapshot) -> Option<u32> {
    (snapshot.status == BudgetStatus::Estimated)
        .then_some(snapshot.maximum_input_tokens)
        .flatten()?
        .checked_add(snapshot.reserved_output_tokens.unwrap_or(0))
        .filter(|window| *window > 0)
}
