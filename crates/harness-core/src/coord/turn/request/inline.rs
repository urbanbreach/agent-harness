//! Inline imaging of the system prompt and large tool results in the outgoing request.
use super::*;
use crate::config::ArchiveSystemPrompt;
use crate::coord::context::Context;
use harness_compaction::archive::{self, ArchiveShape};
use harness_compaction::tokenizer::{AnthropicTier, Counter, Family};
use harness_providers::{CompletionMessage, MessageContentPart};
use std::{collections::BTreeMap, sync::Arc};

mod analysis;
mod cache;
mod estimate;
mod planner;
mod savings;
mod system;
#[cfg(test)]
mod tests;
mod tools;

pub(in crate::coord) use estimate::{InlineEstimate, SystemOutcome, SystemReason, ToolSaving};

type Attachments = BTreeMap<usize, Vec<crate::attachment_transport::AttachmentMetadata>>;
type Content = BTreeMap<usize, Vec<MessageContentPart>>;

/// Request-scoped inputs of one imaging pass.
pub(super) struct Env<'a> {
    pub shape: ArchiveShape,
    /// Total images the deployment accepts.
    pub limit: u32,
    pub mode: ArchiveSystemPrompt,
    pub tool_results: bool,
    /// The reader may image `read` output.
    pub read_opt_in: bool,
    pub counter: &'a Counter,
}

/// What the live context knows of one request message.
#[derive(Debug, Clone, Copy, Default)]
pub(super) struct Fact {
    pub seq: Option<u64>,
    pub at_ms: Option<u64>,
    pub is_error: bool,
    pub pruned_at_ms: Option<u64>,
    /// The carrier's existing history rewrite time (a summary entry records one).
    pub history_rewrite_at_ms: Option<u64>,
}

/// Anthropic Opus revisions from 5.5 up to, not including, 10 read images of `read` output.
fn reads_images(provider: &str, model: &str) -> bool {
    let identity = harness_compaction::tokenizer::classify(provider, model);
    matches!(
        identity.family,
        Family::Anthropic {
            tier: AnthropicTier::Opus
        }
    ) && identity
        .revision
        .is_some_and(|(major, minor, _)| (major, minor) >= (5, 5) && major < 10)
}

/// Facts of the final request arrays; the leading messages the request added (skill
/// system text) have none.
fn facts_of(messages: &Context, request_len: usize) -> Vec<Fact> {
    let offset = request_len.saturating_sub(messages.entries.len());
    let mut facts = vec![Fact::default(); request_len];
    for (slot, entry) in facts.iter_mut().skip(offset).zip(&messages.entries) {
        *slot = Fact {
            seq: Some(entry.seq),
            at_ms: entry.at_ms,
            is_error: entry.tool.is_error,
            pruned_at_ms: entry.pruned_at_ms,
            history_rewrite_at_ms: entry
                .compaction
                .as_ref()
                .and_then(|state| state.history_rewrite_at_ms),
        };
    }
    facts
}

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |elapsed| {
            u64::try_from(elapsed.as_millis()).unwrap_or(u64::MAX)
        })
}

impl Worker {
    fn vision_active(&self) -> bool {
        self.turn
            .target
            .as_ref()
            .is_some_and(|target| target.resolution.capabilities.supports_vision)
    }

    /// Provider and model the shape, price and image allowance come from.
    fn imaging_model<'a>(&'a self, model: &'a crate::agent::AgentModelRef) -> (&'a str, &'a str) {
        self.turn.target.as_ref().map_or(
            (model.provider_id.as_str(), model.model_id.as_str()),
            |target| (target.provider.as_str(), target.model.as_str()),
        )
    }

    fn imaging_env<'a>(&'a self, model: &'a crate::agent::AgentModelRef) -> Env<'a> {
        let settings = &self.compaction.archive;
        let (provider, model_id) = self.imaging_model(model);
        Env {
            shape: *self.shape_for(provider, model_id),
            limit: self.image_limits_for(provider, model_id).total_images,
            mode: settings.system_prompt,
            tool_results: settings.tool_results,
            read_opt_in: reads_images(provider, model_id),
            counter: &self.counter,
        }
    }

    /// Makes the Harness data directory known to the savings journal. Run once per turn
    /// before requests are built; without it nothing is journaled.
    pub(in crate::coord::turn) async fn prime_inline_imaging(&self, messages: &Context) {
        let settings = &self.compaction.archive;
        if !settings.tool_results {
            return;
        }
        let known = messages
            .render_caches
            .inline
            .lock()
            .is_ok_and(|cache| cache.data_dir.is_some());
        if known {
            return;
        }
        if let Ok(dir) = self
            .handle
            .call(|runtime| Ok(runtime.config.data_dir.clone()))
            .await
            && let Ok(mut cache) = messages.render_caches.inline.lock()
        {
            cache.data_dir = Some(dir);
        }
    }

    /// Swaps eligible system text and tool output for archive frames in the outgoing
    /// request copy only; persisted context is never touched.
    pub(super) fn apply_inline_imaging(
        &self,
        messages: &Context,
        model: &crate::agent::AgentModelRef,
        request_messages: &mut Vec<CompletionMessage>,
        attachments: &mut Attachments,
        content: &mut Content,
    ) {
        let settings = &self.compaction.archive;
        let enabled = settings.system_prompt != ArchiveSystemPrompt::None || settings.tool_results;
        let Ok(mut cache) = messages.render_caches.inline.lock() else {
            return;
        };
        if !enabled {
            cache.tools.clear();
            cache.system = None;
            cache.system_tokens = None;
            return;
        }
        if !self.vision_active() {
            return;
        }
        if !settings.tool_results {
            cache.tools.clear();
        }
        let env = self.imaging_env(model);
        let facts = facts_of(messages, request_messages.len());
        let analysis = analysis::analyze(
            &env,
            request_messages,
            attachments,
            &facts,
            &mut cache,
            false,
        );
        analysis::stamp_rewrites(&analysis, request_messages, &facts, &mut cache);
        analysis::apply(&analysis, request_messages, attachments, content);
        let savings = estimate::tools(&analysis, env.shape.raw_frame_tokens());
        let (state, dir) = (Arc::clone(&cache.savings), cache.data_dir.clone());
        drop(cache);
        let Some(dir) = dir else {
            return;
        };
        let (provider, model_id) = self.imaging_model(model);
        let batch: Vec<_> = savings
            .into_iter()
            .map(|saving| savings::Saving {
                tool_call_id: saving.tool_call_id,
                saved_tokens: saving.saved_tokens,
            })
            .collect();
        savings::record(
            &state,
            &dir,
            Some(self.session.as_str()),
            provider,
            model_id,
            &batch,
            now_ms(),
        );
    }

    /// What imaging would do to the request this context produces: per-target eligibility
    /// and savings after the total image cap backstop. An empty live history assumes the
    /// next request supplies a user message to carry system frames.
    pub(in crate::coord) fn estimate_inline_imaging(&self, messages: &Context) -> InlineEstimate {
        let settings = &self.compaction.archive;
        if !self.vision_active() {
            return InlineEstimate::vision_inactive();
        }
        let model = crate::agent::AgentModelRef::parse(&self.turn.model);
        let mut request_messages = messages.messages();
        let mut attachments = messages.attachments();
        let mut content = Content::new();
        self.project_archives(
            messages,
            &model,
            &mut request_messages,
            &mut attachments,
            &mut content,
        );
        self.insert_skill_system(&mut request_messages, &mut attachments, &mut content);
        let env = self.imaging_env(&model);
        let facts = facts_of(messages, request_messages.len());
        let Ok(mut cache) = messages.render_caches.inline.lock() else {
            return InlineEstimate::vision_inactive();
        };
        let assume_carrier = messages.body_start() == messages.entries.len();
        let analysis = analysis::analyze(
            &env,
            &request_messages,
            &attachments,
            &facts,
            &mut cache,
            assume_carrier,
        );
        let mut result = estimate::estimate(&analysis, env.shape.raw_frame_tokens());
        if !settings.tool_results {
            result.tools.clear();
        }
        result
    }
}
