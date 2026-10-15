//! Pre-prompt, mid-turn and post-turn threshold maintenance.
use super::super::promote::{is_active_model, latest_assistant};
use super::*;

impl Worker {
    /// Tokens of the entries from `pending` on, which have not reached the provider yet.
    fn pending_tokens(&self, context: &Context, pending: Option<usize>) -> u32 {
        let Some(from) = pending else {
            return 0;
        };
        let skip = from.saturating_sub(context.body_start());
        context
            .items_with(&self.counter)
            .iter()
            .skip(skip)
            .fold(0u32, |sum, item| sum.saturating_add(item.tokens))
    }

    /// Pre-request (pre-prompt and mid-turn) and post-turn threshold maintenance.
    ///
    /// The trigger is the larger of billed and local occupancy measured before stale
    /// results are pruned: pruning never lowers it. The pass target is that value minus the
    /// pruning savings, so a prune that already made room makes the pass unnecessary.
    /// Failures are warnings: the request still goes out if it fits.
    pub(in crate::coord) async fn threshold_maintenance(
        &mut self,
        context: &mut Context,
        site: Site,
    ) -> Result<Maintained, CoordinatorError> {
        let post_turn = matches!(site, Site::PostTurn { .. });
        let pending = if post_turn {
            None
        } else {
            self.pending_turn(context)
        };
        let mid_turn = !post_turn && pending.is_none();
        let active = self.auto_compaction_available();
        let before = if active {
            Some(self.measure(context)?)
        } else {
            None
        };
        // Superseded reads are pruned after a turn, even when automatic maintenance is off;
        // pre-prompt and mid-turn boundaries never prune.
        let saved = if post_turn {
            self.prune_stale(context).await?
        } else {
            0
        };
        let pruned = Maintained {
            changed: saved > 0,
            ..Maintained::default()
        };
        let Some(before) = before else {
            return Ok(pruned);
        };
        if before.window == 0 || (mid_turn && !self.effective_mid_turn_enabled()) {
            return Ok(pruned);
        }
        let trigger = before.tokens;
        let over =
            harness_compaction::policy::should_compact(trigger, before.window, before.threshold);
        let after = if saved > 0 {
            self.measure_after_prune(&before, saved, context)?
        } else {
            before
        };
        if !over {
            self.speculate_below(context, &after, pending).await?;
            return Ok(pruned);
        }
        if saved > 0 && !after.over_threshold() {
            return Ok(pruned);
        }
        if pending.is_some() {
            // A mid-turn dead end leaves a one-shot marker for the next user pre-prompt.
            let futile = self.take_futile_pre_prompt();
            if futile {
                self.sync_parked().await?;
                if self.prepare(context, pending).is_none() {
                    return Ok(pruned);
                }
            }
        }
        if self.is_parked() {
            if self.prepare(context, pending).is_none() {
                return Ok(pruned);
            }
            self.unpark_context();
            self.sync_parked().await?;
        }
        if self.defer_to_speculation(context, &after, pending).await? {
            return Ok(pruned);
        }
        let why = if post_turn {
            PromoteWhy::PostTurn
        } else {
            PromoteWhy::PreRequest
        };
        let promotable = !post_turn
            || latest_assistant(context)
                .is_some_and(|entry| is_active_model(&self.turn.model, entry));
        if promotable && self.promote(why).await? {
            return Ok(Maintained {
                changed: true,
                ..Maintained::default()
            });
        }
        let order = self.compaction.method_order.clone();
        let mut request = Request::automatic(Reason::Threshold, &order, pending);
        request.trigger_tokens = Some(trigger);
        request.pending_tokens = self.pending_tokens(context, pending);
        request.prepared_tokens = Some(after.tokens);
        match site {
            Site::PreRequest => {
                request.auto_continue = false;
                if mid_turn {
                    request.phase = TurnPhase::Mid;
                    request.suppress_continuation = true;
                    request.detach_post_commit = true;
                }
            }
            Site::PostTurn {
                terminal_text_answer,
            } => request.terminal_text_answer = terminal_text_answer,
        }
        let Some(outcome) = self
            .recovery_pass(context, request, "Auto-compaction failed")
            .await?
        else {
            return Ok(pruned);
        };
        if outcome.blocked {
            self.park_context();
            if mid_turn {
                self.mark_futile_pre_prompt();
            }
            self.sync_parked().await?;
        }
        Ok(Maintained {
            changed: saved > 0 || outcome.rewritten || outcome.promoted,
            continuation_scheduled: outcome.continuation_scheduled,
            synthetic_resume: outcome.synthetic_resume,
            blocked: outcome.blocked,
        })
    }
}
