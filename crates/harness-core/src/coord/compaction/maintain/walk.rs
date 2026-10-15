use super::*;

/// How a walk over the configured methods ended.
pub(super) struct Walked {
    /// The last method failure.
    pub last_error: Option<CoordinatorError>,
    /// There was nothing older to compact.
    pub nothing: bool,
    /// The walk stopped without a commit and the pass must end as a no-op (a shake that
    /// failed outside overflow, or an idle shake that found nothing).
    pub halted: bool,
}

impl Worker {
    pub(super) async fn maintain_inner(
        &mut self,
        context: &mut Context,
        request: &Request,
        progress: &Progress,
    ) -> Result<Outcome, CoordinatorError> {
        let measure = self.measure(context)?;
        progress
            .update(|stage| {
                stage.tokens_before = Some(request.trigger_tokens.unwrap_or(measure.tokens));
                stage.threshold_tokens = (measure.window > 0).then_some(measure.threshold);
                stage.window_tokens = (measure.window > 0).then_some(measure.window);
                stage.fixed_tokens = Some(measure.fixed);
            })
            .await?;
        // Without an active model nothing can summarize: automatic passes end skipped.
        if self.turn.model.trim().is_empty() {
            if request.reason == Reason::Manual {
                return Err(CoordinatorError::CompactionRejected(
                    super::super::ManualRejection::NoModel,
                ));
            }
            return Ok(Outcome {
                skipped: true,
                fits: measure.fits(),
                noop: Some("No model selected".into()),
                ..Outcome::default()
            });
        }
        if self.notes_routes(request) {
            return self.notes_rollover(context, request, progress).await;
        }
        let mut order = match self.available_order(request, &measure).await? {
            Ok(order) => order,
            Err(outcome) => return Ok(outcome),
        };
        let mut outcome = Outcome::default();
        if request.reason.automatic()
            && let Some(draft) = self
                .claim_speculation(
                    context,
                    &measure,
                    order.first().copied(),
                    request.trigger_tokens,
                    request.pending_tokens,
                )
                .await?
        {
            progress
                .update(|stage| stage.method = Some(draft.state.method))
                .await?;
            let committed = self.finish_draft(context, draft, request, progress).await?;
            outcome.saved_path.clone_from(&committed.saved_path);
            outcome.committed = Some(committed);
            outcome.rewritten = true;
            order.clear();
        }
        let walked = self
            .walk(&order, context, request, &measure, progress, &mut outcome)
            .await?;
        if outcome.committed.is_none() && !outcome.rewritten {
            if walked.halted {
                outcome.fits = self.measure(context)?.fits();
                return Ok(outcome);
            }
            if let Some(error) = walked
                .last_error
                .filter(|_| request.reason == Reason::Manual)
            {
                return Err(error);
            }
            if walked.nothing && request.checks_progress() {
                return self
                    .dead_end_without_preparation(context, request, progress)
                    .await;
            }
            if walked.nothing {
                outcome.noop = Some(noop_reason(context, request.handoff_requested));
            }
        }
        let after = self.measure(context)?;
        outcome.fits = after.fits();
        if outcome.committed.is_some() && request.checks_progress() {
            let ok = if request.will_retry() {
                after.fits()
            } else {
                after.has_headroom()
            };
            outcome.blocked = !ok;
        }
        Ok(outcome)
    }

    /// Methods that can run for this request, or the outcome when none can.
    async fn available_order(
        &self,
        request: &Request,
        measure: &Measure,
    ) -> Result<Result<Vec<CompactionMethod>, Outcome>, CoordinatorError> {
        let mut order: Vec<CompactionMethod> = Vec::new();
        let mut notices = Vec::new();
        for method in request.order.iter().skip(request.method_offset) {
            match self.method_available(*method, request) {
                Ok(()) => order.push(*method),
                Err(Some(notice)) => notices.push(notice),
                Err(None) => {}
            }
        }
        if request.reason == Reason::Manual {
            for notice in &notices {
                self.warn(notice.clone()).await?;
            }
        }
        if order.is_empty() {
            let had_notice = !notices.is_empty();
            let message = if request.reason == Reason::Manual {
                notices
                    .pop()
                    .unwrap_or_else(|| "No configured compaction method can run manually.".into())
            } else {
                "No configured compaction method can run.".into()
            };
            if request.forced {
                return Err(match !had_notice {
                    true if request.reason == Reason::Manual => {
                        CoordinatorError::CompactionRejected(
                            super::super::ManualRejection::NoMethod,
                        )
                    }
                    _ if message.starts_with("archive needs a vision-capable model") => {
                        let model = crate::agent::AgentModelRef::parse(&self.turn.model);
                        CoordinatorError::Invalid(format!(
                            "archive cannot run locally: {} is text-only.",
                            model.model_id
                        ))
                    }
                    _ => CoordinatorError::Invalid(message),
                });
            }
            return Ok(Err(Outcome {
                noop: Some(message),
                fits: measure.fits(),
                ..Outcome::default()
            }));
        }
        // A native route that failed for good is skipped at every position, but only when a
        // later method exists: a remote-only configuration still tries it.
        if request.reason.automatic() && self.native_blocked().await? {
            super::super::speculate::skip_native_route(&mut order);
        }
        Ok(Ok(order))
    }

    /// Walks the methods in order until one commits or reduces enough.
    pub(super) async fn walk(
        &mut self,
        order: &[CompactionMethod],
        context: &mut Context,
        request: &Request,
        measure: &Measure,
        progress: &Progress,
        outcome: &mut Outcome,
    ) -> Result<Walked, CoordinatorError> {
        let mode = if request.reason == Reason::Manual {
            WalkMode::Manual
        } else {
            WalkMode::Automatic
        };
        let mut walked = Walked {
            last_error: None,
            nothing: false,
            halted: false,
        };
        for (index, method) in order.iter().copied().enumerate() {
            let more = index + 1 < order.len();
            // Which later passes of this request already ran a shake that fell through.
            let request = Request {
                fallback_from_shake: request.fallback_from_shake
                    || order[..index].contains(&CompactionMethod::Shake),
                ..request.clone()
            };
            match self
                .run_method(method, context, &request, measure, mode, progress)
                .await
            {
                Ok(MethodResult::Committed(committed)) => {
                    outcome.saved_path.clone_from(&committed.saved_path);
                    outcome.committed = Some(committed);
                    outcome.rewritten = true;
                    break;
                }
                Ok(MethodResult::Reduced { enough }) => {
                    outcome.rewritten = true;
                    if enough || request.reason == Reason::Idle {
                        break;
                    }
                }
                Ok(MethodResult::Unavailable(message)) => {
                    // An idle shake that found nothing ends the pass.
                    if method == CompactionMethod::Shake && request.reason == Reason::Idle {
                        walked.halted = true;
                        return Ok(walked);
                    }
                    if let Some(message) = message {
                        self.warn(message).await?;
                    }
                }
                Ok(MethodResult::NothingToCompact) => {
                    walked.nothing = true;
                    return Ok(walked);
                }
                Err(error) if progress.cancel.is_cancelled() => return Err(error),
                // A veto or a failed critical hook ends the whole pass: no fallback.
                Err(
                    error @ (CoordinatorError::CompactionCancelled { .. }
                    | CoordinatorError::Compaction(CompactionError {
                        kind: CompactionErrorKind::Hook,
                        ..
                    })),
                ) => return Err(error),
                Err(error) => {
                    self.report_failure(&request, method, more, &error, progress)
                        .await?;
                    // Only an overflow falls through after a shake error.
                    if method == CompactionMethod::Shake && request.reason != Reason::Overflow {
                        walked.halted = true;
                        walked.last_error = Some(error);
                        return Ok(walked);
                    }
                    walked.last_error = Some(error);
                }
            }
        }
        Ok(walked)
    }

    /// Reason-specific warning for a method that failed before committing.
    async fn report_failure(
        &self,
        request: &Request,
        method: CompactionMethod,
        more: bool,
        error: &CoordinatorError,
        progress: &Progress,
    ) -> Result<(), CoordinatorError> {
        let warning = match (request.reason, more) {
            (Reason::Manual, false) => None,
            (Reason::Manual, true) => Some(format!(
                "{} compaction failed; trying the next preferred method",
                method.as_str()
            )),
            (reason, more) => {
                let prefix = match reason {
                    Reason::Overflow => "Context overflow recovery failed",
                    Reason::Incomplete => "Incomplete response recovery failed",
                    _ => "Auto-compaction failed",
                };
                let suffix = if more {
                    "; trying the next preferred compaction method."
                } else {
                    ""
                };
                let message = error.to_string();
                let message = if message.is_empty() {
                    "compaction failed"
                } else {
                    &message
                };
                Some(format!("{prefix}: {message}{suffix}"))
            }
        };
        if let Some(warning) = warning {
            self.warn(warning).await?;
        }
        progress
            .update(|stage| stage.message = Some(error.to_string()))
            .await
    }
}
