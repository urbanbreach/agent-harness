//! Side effects every successful history rewrite owes the rest of the runtime.
use super::*;
use num_traits::ToPrimitive;

/// Why history was rewritten.
pub(in crate::coord) enum RewriteReason {
    Prune,
    Shake,
    Images,
    Thinking,
    Rescue,
    /// A compaction replaced the history; providers that track passes get its descriptor.
    Summary(Option<harness_providers::CompactionDescriptor>),
}

impl Worker {
    /// Tells identity-caching provider sessions the history changed under them so the
    /// next request does not reuse state built for the old history.
    pub(in crate::coord) fn after_rewrite(&self, reason: RewriteReason) {
        let descriptor = match reason {
            RewriteReason::Summary(descriptor) => descriptor,
            RewriteReason::Prune
            | RewriteReason::Shake
            | RewriteReason::Images
            | RewriteReason::Thinking
            | RewriteReason::Rescue => None,
        };
        self.provider
            .session_event(&harness_providers::ProviderSessionEvent::Compacted {
                session_id: self.session.clone(),
                descriptor,
            });
    }
}

/// Keeps a native child's context counter proportional after its history shrank.
pub(in crate::coord) fn rebase_native_usage(
    context: &mut Context,
    counter: &harness_compaction::tokenizer::Counter,
) {
    let Some(previous) = context.native_context_usage else {
        return;
    };
    context.native_context_usage =
        super::super::super::context::native_tokens_with(&context.entries, counter).map(
            |estimate| {
                let scaled = previous
                    .estimate_at_last_response
                    .filter(|old| *old > 0)
                    .filter(|_| previous.total_tokens > 0)
                    .and_then(|old| {
                        let scaled = (estimate.to_f64()?
                            * (previous.total_tokens.to_f64()? / old.to_f64()?))
                        .round();
                        Some(scaled.to_u64().unwrap_or(u64::MAX))
                    })
                    .unwrap_or(estimate);
                crate::subagent::SubagentContextUsage {
                    total_tokens: if previous.total_tokens > 0 {
                        scaled.min(previous.total_tokens)
                    } else {
                        scaled
                    },
                    estimate_at_last_response: Some(estimate),
                    estimate_after_last_response: Some(estimate),
                }
            },
        );
}
