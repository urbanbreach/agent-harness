use super::*;
use harness_compaction::{tokens::Anchor, Item};

/// Newest usage anchor at or after `start` whose item `qualifies`; an older one is tried
/// when the newest does not.
fn newest_anchor(
    items: &[Item<'_>],
    start: usize,
    qualifies: impl Fn(usize) -> bool,
) -> Option<Anchor> {
    let mut end = items.len();
    loop {
        let anchor = tokens::usage_anchor(&items[..end], start, false)?;
        if qualifies(anchor.index) {
            return Some(anchor);
        }
        end = anchor.index;
    }
}

/// Provider-anchored occupancy: the anchor's usage plus the local tail after it, opaque
/// reasoning left out. An anchor stamped strictly before the active summary describes a
/// context that no longer exists and counts as zero; equal is not older. An untimed anchor
/// is trusted.
fn anchored_estimate(
    items: &[Item<'_>],
    anchor: Option<Anchor>,
    summary_at_ms: Option<u64>,
) -> u32 {
    let Some(anchor) = anchor else {
        return 0;
    };
    // A kept original was billed before the summary replaced what came ahead of it.
    if items[anchor.index].retained {
        return 0;
    }
    if let (Some(at), Some(summary)) = (items[anchor.index].at_ms, summary_at_ms)
        && at < summary
    {
        return 0;
    }
    items[anchor.index + 1..]
        .iter()
        .fold(anchor.tokens, |total, item| {
            total.saturating_add(item.tokens_floor)
        })
}

impl Worker {
    /// The threshold pair, base window and reserve in force for this worker: the captured
    /// overlay over the live settings, then the live model's override.
    pub(in crate::coord) fn threshold_settings(&self) -> ThresholdSettings {
        let model = crate::agent::AgentModelRef::parse(&self.turn.model);
        self.overlay.thresholds(
            &self.compaction,
            &self.profile.name,
            &model.provider_id,
            &model.model_id,
        )
    }

    /// Automatic compaction is on, as this worker's root chain resolved it.
    pub(in crate::coord) fn effective_enabled(&self) -> bool {
        self.overlay.enabled(&self.compaction)
    }

    /// Mid-turn maintenance: on for children unless their spawn turned it off; roots follow
    /// the live setting.
    pub(in crate::coord) fn effective_mid_turn_enabled(&self) -> bool {
        self.overlay.mid_turn_enabled(&self.compaction)
    }

    /// Automatic compaction is on and has something to run: a configured method or a
    /// usable notebook. A configured method may still be unavailable on the model.
    pub(in crate::coord) fn auto_compaction_available(&self) -> bool {
        self.effective_enabled()
            && (!self.compaction.method_order.is_empty() || self.notes_usable())
    }

    /// Provider-anchored occupancy of `context` for recovery inspection. With
    /// `only_active_model` only assistants produced by the active model qualify (an entry
    /// without recorded provenance counts as the active model's own); threshold maintenance
    /// accepts any model. Stale usage counts as zero.
    pub(in crate::coord) fn provider_tokens(
        &self,
        context: &Context,
        only_active_model: bool,
    ) -> u32 {
        let items = context.items_with(&self.counter);
        self.provider_estimate(context, &items, only_active_model)
    }

    fn provider_estimate(
        &self,
        context: &Context,
        items: &[Item<'_>],
        only_active_model: bool,
    ) -> u32 {
        let summary = items.first().filter(|item| item.kind == ItemKind::Summary);
        let start = usize::from(summary.is_some());
        let body = context.body_start();
        let anchor = newest_anchor(items, start, |index| {
            !only_active_model
                || context
                    .entries
                    .get(body + index)
                    .is_none_or(|entry| super::promote::is_active_model(&self.turn.model, entry))
        });
        anchored_estimate(items, anchor, summary.and_then(|item| item.at_ms))
    }

    /// Measures the next request: window, threshold, and occupied tokens.
    pub(in crate::coord) fn measure(&self, context: &Context) -> Result<Measure, CoordinatorError> {
        let mut request = self.request(context);
        // After a final answer (or before the first prompt) the next request starts with a
        // new user prompt.
        if request.messages.last().is_none_or(|m| {
            !matches!(
                m.role,
                harness_providers::MessageRole::User | harness_providers::MessageRole::Tool
            )
        }) {
            request
                .messages
                .push(harness_providers::CompletionMessage::text(
                    harness_providers::MessageRole::User,
                    "",
                ));
        }
        let budget = self.request_budget(&request)?;
        let components = budget.components;
        // The fixed prefix is the system prompt and tool schemas; the admission framing
        // allowance belongs to the request budget, not to the stored-context floor.
        let fixed = components
            .system_tokens
            .saturating_add(components.tools_tokens);
        // Rescue fit, payload byte text and promotion trust only a catalog window.
        let catalog_window = self
            .turn
            .target
            .as_ref()
            .and_then(|t| t.limits.context_window_tokens())
            .filter(|window| *window > 0);
        // Admission alone may fall back to the request budget's conservative input limit.
        let window = catalog_window
            .or_else(|| {
                (budget.status == crate::BudgetStatus::ConservativeFallback)
                    .then_some(budget.maximum_input_tokens)
                    .flatten()
            })
            .unwrap_or(0);
        let settings = self.threshold_settings();
        // Deliberate safety beyond the resolved threshold: a model whose input limit sits
        // below its context window (or a limited or unknown cap) must compact before the
        // request budget refuses the prompt, so the trigger never exceeds the input cap.
        let input_cap = budget.maximum_input_tokens.unwrap_or(u32::MAX);
        let threshold = if window > 0 {
            policy::resolve_threshold(window, &settings).min(input_cap)
        } else {
            u32::MAX
        };
        let items = context.items_with(&self.counter);
        let provider = self.provider_estimate(context, &items, false);
        let local = items
            .iter()
            .fold(fixed, |total, item| total.saturating_add(item.tokens_floor));
        Ok(Measure {
            window,
            window_known: catalog_window.is_some(),
            threshold,
            tokens: tokens::compaction_context_tokens(provider, local),
            provider,
            local,
            fixed,
            input_cap,
            settings,
        })
    }

    /// The measurement after a reducer saved `saved` tokens: pruning never lowers the billed
    /// trigger retroactively, so the provider arm drops by exactly what was saved and the
    /// local arm is recounted over the pruned `context`.
    pub(in crate::coord) fn measure_after_prune(
        &self,
        before: &Measure,
        saved: u32,
        context: &Context,
    ) -> Result<Measure, CoordinatorError> {
        let after = self.measure(context)?;
        let provider = before.provider.saturating_sub(saved);
        Ok(Measure {
            tokens: tokens::compaction_context_tokens(provider, after.local),
            provider,
            ..after
        })
    }

    /// Prepares a cut over the context, honoring a manual "through this request" bound.
    pub(in crate::coord::compaction) fn prepare(
        &self,
        context: &Context,
        pending_from: Option<usize>,
    ) -> Option<(usize, Preparation)> {
        let body = context.body_start();
        let items = context.items_with(&self.counter);
        let state = context
            .entries
            .get(body)
            .and_then(|e| e.compaction.as_ref());
        let pending = pending_from.map(|index| index.saturating_sub(body));
        let previous_files = state.map(|s| (s.read_files.as_slice(), s.modified_files.as_slice()));
        let exclude_reasoning = self.turn.target.as_ref().is_some_and(|target| {
            super::super::archive::excludes_archived_reasoning(&target.provider, &target.model)
        });
        let previous_summary = state
            .map(|state| super::super::archive::archive_previous_summary(state, exclude_reasoning));
        let prep = prepare::prepare(&PrepareInput {
            items: &items,
            pending_from: pending,
            keep_recent_tokens: self.compaction.keep_recent_tokens,
            previous_summary: previous_summary.as_deref(),
            previous_files,
            previous_summary_at_ms: None,
            previous_details: state.and_then(|s| s.details.as_ref()),
            previous_preserve: state.map(|s| &s.preserve_data),
        })?;
        Some((body, prep))
    }

    pub(in crate::coord::compaction) fn method_available(
        &self,
        method: CompactionMethod,
        request: &Request,
    ) -> Result<(), Option<String>> {
        let vision = self
            .turn
            .target
            .as_ref()
            .is_some_and(|t| t.resolution.capabilities.supports_vision);
        match method {
            CompactionMethod::Remote => {
                let live = self.request(&Context::default());
                let lanes = self
                    .provider
                    .native_compaction_lanes(&live, &self.native_lane_settings(&live));
                if lanes.is_empty() && self.compaction.remote_endpoint.is_none() {
                    let model = crate::agent::AgentModelRef::parse(&self.turn.model);
                    return Err(request.reason.automatic().then_some(()).map_or_else(
                        || Some(format!("remote compaction is unavailable for {}; trying the next preferred method", model.model_id)),
                        |()| None,
                    ));
                }
                Ok(())
            }
            CompactionMethod::Archive => {
                if !vision {
                    let model = crate::agent::AgentModelRef::parse(&self.turn.model);
                    return Err(request.forced.then(|| {
                        format!(
                            "archive needs a vision-capable model ({} is text-only)",
                            model.model_id
                        )
                    }));
                }
                if request.exclude_media
                    || (request.reason == Reason::Manual && request.focus.is_some())
                {
                    return Err(None);
                }
                Ok(())
            }
            CompactionMethod::Handoff => {
                if request.reason == Reason::Overflow
                    || (request.reason == Reason::Manual && !request.handoff_requested)
                {
                    return Err(None);
                }
                Ok(())
            }
            CompactionMethod::Shake => {
                if request.reason == Reason::Manual {
                    return Err(None);
                }
                Ok(())
            }
            CompactionMethod::Soft => Ok(()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use harness_providers::{CompletionMessage, CompletionUsage, MessageRole};

    fn usage(total: u32) -> CompletionUsage {
        CompletionUsage {
            prompt_tokens: total.saturating_sub(10),
            completion_tokens: 10,
            total_tokens: total,
            ..CompletionUsage::default()
        }
    }

    #[test]
    fn stale_usage_counts_as_zero_and_equal_is_not_stale() {
        let messages = [
            CompletionMessage::text(MessageRole::User, "summary"),
            CompletionMessage::text(MessageRole::Assistant, "answer"),
            CompletionMessage::text(MessageRole::User, "tail"),
        ];
        let anchored = usage(1_000);
        // (anchor time, summary time, expected provider tokens)
        for (anchor_at, summary_at, expected) in [
            (Some(101), Some(100), 1_050),
            (Some(100), Some(100), 1_050),
            (Some(99), Some(100), 0),
            (None, Some(100), 1_050),
            (Some(1), None, 1_050),
        ] {
            let mut items = messages.iter().map(Item::new).collect::<Vec<_>>();
            items[0].kind = ItemKind::Summary;
            items[0].at_ms = summary_at;
            items[1].usage = Some(&anchored);
            items[1].at_ms = anchor_at;
            items[2].tokens_floor = 50;
            let anchor = newest_anchor(&items, 1, |_| true);
            assert_eq!(
                anchored_estimate(&items, anchor, summary_at),
                expected,
                "{anchor_at:?} vs {summary_at:?}"
            );
        }
        assert_eq!(anchored_estimate(&[], None, Some(1)), 0);
    }

    #[test]
    fn anchor_search_skips_unqualified_assistants() {
        let messages = [
            CompletionMessage::text(MessageRole::Assistant, "old"),
            CompletionMessage::text(MessageRole::Assistant, "new"),
        ];
        let (old, new) = (usage(500), usage(900));
        let mut items = messages.iter().map(Item::new).collect::<Vec<_>>();
        items[0].usage = Some(&old);
        items[1].usage = Some(&new);
        for (name, rejected, expected) in [
            ("all qualify", vec![], Some((1, 900))),
            ("newest foreign", vec![1], Some((0, 500))),
            ("none qualify", vec![0, 1], None),
        ] {
            let found = newest_anchor(&items, 0, |index| !rejected.contains(&index))
                .map(|anchor| (anchor.index, anchor.tokens));
            assert_eq!(found, expected, "{name}");
        }
    }
}
