//! Folded and single-pass summary generation with checkpoint text.
use super::extract::{from_coordinator, into_coordinator, RetryMode};
use super::*;
use crate::coord::compaction::{CompactionError, CompactionErrorKind};
use harness_compaction::tokenizer::{Counter, ReaderApi};

/// What the host hands summary generation besides the history.
#[derive(Debug, Clone, Copy)]
pub(in crate::coord::compaction) struct SummaryInputs<'a> {
    /// Replaces the initial or update base prompt when nonempty; focus is still appended.
    pub prompt_override: Option<&'a str>,
    /// Lines rendered as bullets inside `<additional-context>`, unescaped, in order.
    pub extra_context: &'a [String],
    pub focus: Option<&'a str>,
    /// The effective reserve all output caps derive from.
    pub reserve: u32,
}

/// One candidate's view of a summary run.
struct Call<'a> {
    target: &'a ResolvedModelTarget,
    candidate: &'a Candidate,
    context: &'a Context,
    inputs: &'a SummaryInputs<'a>,
    /// The host reserve, lowered to what this candidate's own window can afford.
    reserve: u32,
    retry: RetryMode,
    transport: Transport<'a>,
    progress: &'a Progress,
}

fn truthy(text: Option<&str>) -> Option<&str> {
    text.filter(|text| !text.is_empty())
}

impl Worker {
    /// Folds an arbitrarily long history into one summary, window by window.
    async fn folded_summary(
        &self,
        call: &Call<'_>,
        items: &[Item<'_>],
        previous: Option<&str>,
        usage: &mut Option<CompletionUsage>,
    ) -> Result<String, CompactionError> {
        let kind = CompactionErrorKind::Summary;
        let target = call.target;
        let dialect = serialize::dialect_for_model(&target.model);
        let max_tokens = request::summary_max_tokens(call.reserve);
        let window = call.candidate.window;
        let budget = request::summary_input_budget(window, max_tokens);
        let minimum = request::min_summary_input(window);
        let counter = Counter::for_model(&target.provider, &target.model, ReaderApi::Other);
        let plan = request::plan_windows(items, dialect, budget, &counter);
        let mut whole = plan.whole;
        let mut pending: Vec<(std::ops::Range<usize>, u32)> = plan
            .windows
            .into_iter()
            .map(|range| (range, budget))
            .collect();
        let total = u32::try_from(pending.len()).unwrap_or(u32::MAX);
        call.progress
            .update(|stage| {
                stage.windows_done = 0;
                stage.windows_total = total;
            })
            .await
            .map_err(|error| from_coordinator(kind, error))?;
        let mut carried = previous.map(str::to_owned);
        let mut done = 0u32;
        while let Some((range, window_budget)) = pending.first().cloned() {
            // A history that fit whole already carries its serialization and proof of fit.
            let (text, probe) = match whole.take() {
                Some(text) => (text, None),
                None => {
                    let text = serialize::serialize_for_summary(&items[range.clone()], dialect);
                    let probe = counter.probe_budget([text.as_str()], i64::from(window_budget));
                    (text, Some(probe))
                }
            };
            let sent = match probe {
                Some(probe) if !probe.fits => {
                    let tokens = u32::try_from(probe.tokens).unwrap_or(u32::MAX);
                    Some((
                        request::clamp_to_budget(&text, window_budget, tokens),
                        probe,
                    ))
                }
                _ => None,
            };
            let body = sent.as_ref().map_or(text.as_str(), |(clamped, _)| clamped);
            let prompt = if truthy(carried.as_deref()).is_some() {
                SummaryPrompt::Update
            } else {
                SummaryPrompt::Initial
            };
            let user = prompts::render_summary_request(
                body,
                carried.as_deref(),
                call.inputs.extra_context,
                prompt,
                call.inputs.focus,
                call.inputs.prompt_override,
            );
            match self.summary_request(call, user, max_tokens, kind).await {
                Ok(side) => {
                    usage_sum(usage, side.usage);
                    carried = Some(side.text);
                    pending.remove(0);
                    done += 1;
                    let total = done + u32::try_from(pending.len()).unwrap_or(0);
                    call.progress
                        .update(|stage| {
                            stage.windows_done = done;
                            stage.windows_total = total;
                        })
                        .await
                        .map_err(|error| from_coordinator(kind, error))?;
                }
                Err(error) if super::extract::classify(&error).overflow => {
                    // The tokens sent come from an exact probe, otherwise a strict count of
                    // the unclamped text.
                    let sent_tokens = match probe {
                        Some(probe) if probe.exact => probe.tokens,
                        _ => u64::from(counter.count_text_mode(
                            &text,
                            harness_compaction::tokenizer::CountMode::Strict,
                        )),
                    };
                    let sent_tokens = u32::try_from(sent_tokens).unwrap_or(u32::MAX);
                    let Some(halved) = request::halved_budget(window_budget, sent_tokens, minimum)
                    else {
                        return Err(error);
                    };
                    let replanned: Vec<_> =
                        request::plan_windows(&items[range.clone()], dialect, halved, &counter)
                            .windows
                            .into_iter()
                            .map(|inner| {
                                (inner.start + range.start..inner.end + range.start, halved)
                            })
                            .collect();
                    pending.splice(0..1, replanned);
                }
                Err(error) => return Err(error),
            }
        }
        Ok(carried.unwrap_or_default())
    }

    /// One summary request over the model or the configured endpoint.
    async fn summary_request(
        &self,
        call: &Call<'_>,
        user: String,
        max_tokens: u32,
        kind: CompactionErrorKind,
    ) -> Result<Side, CompactionError> {
        match call.transport {
            Transport::Model => {
                let req = self.side_request(call.target, call.context, user, max_tokens);
                self.side(req, kind, call.progress, call.retry, true).await
            }
            Transport::Endpoint(endpoint) => {
                self.endpoint_summary(
                    endpoint,
                    call.candidate,
                    &self.side_system(call.context),
                    &user,
                    Self::clamp_output(call.target, max_tokens),
                    kind,
                    call.progress,
                )
                .await
            }
        }
    }

    /// The split-turn prefix or display summary: one request, no folding or clamping.
    async fn single_summary(
        &self,
        call: &Call<'_>,
        items: &[Item<'_>],
        previous: Option<&str>,
        prompt: SummaryPrompt,
        max_tokens: u32,
    ) -> Result<Side, CompactionError> {
        let dialect = serialize::dialect_for_model(&call.target.model);
        let text = serialize::serialize_for_summary(items, dialect);
        // The prefix keeps its own prompt and ignores context; the display summary takes
        // the host context but never focus or an override.
        let (context, kind): (&[String], _) = match prompt {
            SummaryPrompt::Short => (call.inputs.extra_context, CompactionErrorKind::ShortSummary),
            _ => (&[], CompactionErrorKind::TurnPrefix),
        };
        let user = prompts::render_summary_request(&text, previous, context, prompt, None, None);
        // Only the display summary goes through a configured endpoint.
        let call = Call {
            transport: if prompt == SummaryPrompt::Short {
                call.transport
            } else {
                Transport::Model
            },
            ..*call
        };
        self.summary_request(&call, user, max_tokens, kind).await
    }

    /// The local summary method over a preparation, walking summarizer candidates in order.
    ///
    /// Automatic walks disable the summarizer's own retry and loop over whole attempts;
    /// manual and speculative walks keep its default retry and never back off between
    /// candidates.
    #[expect(
        clippy::too_many_arguments,
        reason = "The soft method composes every preparation region"
    )]
    pub(in crate::coord::compaction) async fn soft_summary(
        &self,
        context: &Context,
        prep: &Preparation,
        inputs: &SummaryInputs<'_>,
        mode: WalkMode,
        transport: Transport<'_>,
        method: CompactionMethod,
        native_boundary: Option<&NativeBoundary>,
        progress: &Progress,
    ) -> Result<SoftOutput, CoordinatorError> {
        let task = self.turn.id.as_str();
        let targets = self
            .summary_targets(native_boundary)
            .await
            .map_err(|error| into_coordinator(error, task))?;
        let candidates = targets
            .iter()
            .map(|(candidate, _)| candidate.clone())
            .collect();
        let items = context.items_with(&self.counter);
        let retry = if mode == WalkMode::Automatic {
            RetryMode::Disabled
        } else {
            RetryMode::Default
        };
        self.walk_candidates(mode, &progress.cancel, candidates, |candidate, _| {
            let candidate = candidate.clone();
            let target = targets
                .iter()
                .find(|(c, _)| c.provider == candidate.provider && c.model == candidate.model)
                .map(|(_, target)| target);
            let items = &items;
            async move {
                let Some(target) = target else {
                    return Err(CompactionError::new(
                        CompactionErrorKind::Summary,
                        "Compaction failed: no available model",
                    ));
                };
                let call = Call {
                    target,
                    candidate: &candidate,
                    context,
                    inputs,
                    reserve: candidate.window.filter(|window| *window > 0).map_or(
                        inputs.reserve,
                        |window| {
                            inputs
                                .reserve
                                .min(harness_compaction::policy::effective_reserve(
                                    window,
                                    self.compaction.reserve_tokens,
                                ))
                        },
                    ),
                    retry,
                    transport,
                    progress,
                };
                self.soft_with(&call, items, prep, method).await
            }
        })
        .await
        .map_err(|error| into_coordinator(error, task))
    }

    async fn soft_with(
        &self,
        call: &Call<'_>,
        items: &[Item<'_>],
        prep: &Preparation,
        method: CompactionMethod,
    ) -> Result<SoftOutput, CompactionError> {
        let target = call.target;
        let reserve = call.reserve;
        let mut usage = None;
        let history = &items[prep.history.clone()];
        let reader_previous = truthy(prep.previous_summary.as_deref()).map(|previous| {
            if super::super::archive::excludes_archived_reasoning(&target.provider, &target.model) {
                std::borrow::Cow::Owned(harness_compaction::archive::strip_reasoning(previous))
            } else {
                std::borrow::Cow::Borrowed(previous)
            }
        });
        let previous = reader_previous.as_deref();
        call.progress
            .phase(Some(method), CompactionPhase::Summarizing)
            .await
            .map_err(|error| from_coordinator(CompactionErrorKind::Summary, error))?;
        let split = prep.is_split && !prep.turn_prefix.is_empty();
        let history_summary = async {
            // A split turn still updates a prior summary even when no history remains.
            if history.is_empty() && !(split && previous.is_some()) {
                return Ok((
                    previous.map_or_else(|| request::NO_PRIOR_HISTORY.to_owned(), str::to_owned),
                    None,
                ));
            }
            let mut local = None;
            let text = self
                .folded_summary(call, history, previous, &mut local)
                .await?;
            Ok::<_, CompactionError>((text, local))
        };
        let summary = if split {
            let prefix = self.single_summary(
                call,
                &items[prep.turn_prefix.clone()],
                None,
                SummaryPrompt::TurnPrefix,
                request::turn_prefix_max_tokens(reserve),
            );
            let (history, prefix) = tokio::join!(history_summary, prefix);
            let (history, history_usage) = history?;
            let prefix = prefix?;
            usage_sum(&mut usage, history_usage);
            usage_sum(&mut usage, prefix.usage);
            request::merge_split(&history, &prefix.text)
        } else {
            let (history, history_usage) = history_summary.await?;
            usage_sum(&mut usage, history_usage);
            history
        };
        call.progress
            .phase(Some(method), CompactionPhase::ShortSummary)
            .await
            .map_err(|error| from_coordinator(CompactionErrorKind::ShortSummary, error))?;
        let short = self
            .single_summary(
                call,
                &items[prep.recent.clone()],
                truthy(Some(summary.as_str())),
                SummaryPrompt::Short,
                request::short_summary_max_tokens(reserve),
            )
            .await?;
        usage_sum(&mut usage, short.usage);
        Ok(SoftOutput {
            summary,
            short: Some(short.text),
            usage,
            provider_id: target.provider.clone(),
            model_id: target.model.clone(),
        })
    }
}
