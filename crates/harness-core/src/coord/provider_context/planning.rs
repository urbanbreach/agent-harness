// allow: SIZE_OK — coordinator state machine (turn lifecycle + scheduling)
use std::collections::BTreeSet;

use crate::agent::{
    AgentModelRef, ProviderCompactionFacts, ProviderCompactionSummarySource,
    ProviderCompactionTailBoundary, ProviderCompactionTimelineEntry, ProviderContext,
    ProviderContextCheckpoint, ProviderContextCheckpointMetadata, ProviderConversationTurn,
    ProviderConversationTurnStatus,
};
use crate::config::CompactionRuntimeConfig;
use crate::conversation::ConversationMessage;
use crate::event::EventArtifactRef;
use crate::proj::RecordedRuntimeContext;
use crate::redact::Redactor;
use crate::text::{non_empty_trimmed, truncate_with_ellipsis};

use super::super::RunState;
use super::build_provider_compaction_facts;
use super::restore::collect_historical_agent_turns_until;
use super::summary::build_provider_context_summary;
use super::tokens::{
    approximate_provider_context_tokens, approximate_text_tokens, approximate_turn_tokens,
    preserved_tokens_estimate, summarize_compaction_text,
};
use super::{
    PROVIDER_CONTEXT_COMPACTION_KEEP_RECENT_MAX_TOKENS,
    PROVIDER_CONTEXT_COMPACTION_KEEP_RECENT_MIN_TOKENS, PROVIDER_CONTEXT_COMPACTION_RESERVE_TOKENS,
    PROVIDER_CONTEXT_COMPACTION_SUMMARY_MAX_CHARS,
    PROVIDER_CONTEXT_COMPACTION_TURN_EXCERPT_MAX_CHARS, PROVIDER_CONTEXT_SUMMARY_CONTRACT_VERSION,
};

#[derive(Debug, Clone)]
pub(in crate::coord) struct ProviderCompactionTrigger {
    pub(in crate::coord) agent_id: String,
    pub(in crate::coord) profile_name: String,
    pub(in crate::coord) model_ref: String,
    pub(in crate::coord) provider_id: Option<String>,
    pub(in crate::coord) model_id: Option<String>,
    pub(in crate::coord) through_request_id: Option<String>,
    pub(in crate::coord) trigger_reason: String,
    pub(in crate::coord) tokens_before: Option<u32>,
    pub(in crate::coord) prompt_tokens_estimate: Option<u32>,
    pub(in crate::coord) estimate_source: Option<String>,
}

#[derive(Debug, Clone, Copy)]
pub(super) struct ProviderContextTriggerEstimate {
    pub(super) tokens_before_estimate: u32,
    pub(super) input_budget: u32,
    pub(super) reserve: u32,
    pub(super) source: &'static str,
}
#[derive(Debug, Clone)]
pub(in crate::coord) struct CompactionSummaryDecision {
    summary: Option<String>,
    source: SummarySourceRequest,
    split_prefix_summary: Option<SplitPrefixSummaryDecision>,
}

impl CompactionSummaryDecision {
    pub(in crate::coord) fn deterministic(trigger: &ProviderCompactionTrigger) -> Self {
        Self {
            summary: None,
            source: SummarySourceRequest::DeterministicForModelRef {
                model_ref: trigger.model_ref.clone(),
            },
            split_prefix_summary: None,
        }
    }

    pub(in crate::coord) fn hook(summary: String) -> Self {
        Self {
            summary: Some(summary),
            source: SummarySourceRequest::Hook,
            split_prefix_summary: None,
        }
    }

    pub(in crate::coord) fn model(
        model_ref: String,
        summary: String,
        deterministic_fallback: bool,
        split_prefix_summary: Option<SplitPrefixSummaryDecision>,
    ) -> Self {
        Self {
            summary: if non_empty_trimmed(&summary).is_some() {
                Some(summary)
            } else {
                None
            },
            source: SummarySourceRequest::Model {
                model_ref,
                deterministic_fallback,
            },
            split_prefix_summary,
        }
    }
}

#[derive(Debug, Clone)]
enum SummarySourceRequest {
    Hook,
    Model {
        model_ref: String,
        deterministic_fallback: bool,
    },
    Deterministic,
    DeterministicForModelRef {
        model_ref: String,
    },
}

#[derive(Debug, Clone)]
pub(in crate::coord) struct ProviderContextCompactionPlan {
    pub(in crate::coord) older_turns: Vec<ProviderConversationTurn>,
    pub(in crate::coord) recent_turns: Vec<ProviderConversationTurn>,
    pub(in crate::coord) pruned_tool_artifacts: Vec<EventArtifactRef>,
    pub(in crate::coord) facts: ProviderCompactionFacts,
    pub(in crate::coord) tail_boundary: ProviderCompactionTailBoundary,
}

pub(in crate::coord) struct ProviderContextCompactionRequest<'a> {
    run_state: &'a RunState,
    trigger: ProviderCompactionTrigger,
    compaction_config: &'a CompactionRuntimeConfig,
    summary_decision: &'a CompactionSummaryDecision,
}

impl<'a> ProviderContextCompactionRequest<'a> {
    pub(in crate::coord) fn new(
        run_state: &'a RunState,
        trigger: ProviderCompactionTrigger,
        compaction_config: &'a CompactionRuntimeConfig,
        summary_decision: &'a CompactionSummaryDecision,
    ) -> Self {
        Self {
            run_state,
            trigger,
            compaction_config,
            summary_decision,
        }
    }

    pub(in crate::coord) fn plan(
        self,
        redactor: &(impl Redactor + ?Sized),
    ) -> Option<ProviderContextCompactionDecision> {
        let current_context = self
            .run_state
            .provider_context_by_agent
            .get(&self.trigger.agent_id)
            .cloned()
            .unwrap_or_default();
        let current_context_tokens = approximate_provider_context_tokens(&current_context);
        let metadata = recorded_runtime_context_for_compaction(self.run_state, &self.trigger);
        if !should_compact_provider_context(
            &current_context,
            &metadata,
            &self.trigger,
            self.compaction_config,
        ) {
            return None;
        }

        let trigger_estimate = provider_context_trigger_estimate(
            &current_context,
            &metadata,
            &self.trigger,
            self.compaction_config,
        );
        let tokens_before_estimate = trigger_estimate
            .as_ref()
            .map(|estimate| estimate.tokens_before_estimate)
            .unwrap_or(current_context_tokens);
        let mut trigger = self.trigger;
        if trigger.estimate_source.is_none() {
            trigger.estimate_source = trigger_estimate.map(|estimate| estimate.source.to_string());
        }

        let checkpoint = build_provider_context_checkpoint(
            ProviderContextCheckpointRequest {
                run_state: self.run_state,
                trigger: &trigger,
                context: &current_context,
                keep_recent_budget: provider_context_keep_recent_tokens(&metadata),
                tokens_before_estimate,
                compaction_config: self.compaction_config,
                summary_decision: self.summary_decision,
            },
            redactor,
        )?;
        let updated_context = ProviderContext::from_checkpoint(checkpoint.clone());
        let tokens_after_estimate = approximate_provider_context_tokens(&updated_context);

        Some(ProviderContextCompactionDecision {
            trigger,
            checkpoint,
            updated_context,
            tokens_before_estimate,
            tokens_after_estimate,
        })
    }
}

pub(in crate::coord) struct ProviderContextCompactionDecision {
    pub(in crate::coord) trigger: ProviderCompactionTrigger,
    pub(in crate::coord) checkpoint: ProviderContextCheckpoint,
    pub(in crate::coord) updated_context: ProviderContext,
    pub(in crate::coord) tokens_before_estimate: u32,
    pub(in crate::coord) tokens_after_estimate: u32,
}

struct ProviderContextCheckpointRequest<'a> {
    run_state: &'a RunState,
    trigger: &'a ProviderCompactionTrigger,
    context: &'a ProviderContext,
    keep_recent_budget: u32,
    tokens_before_estimate: u32,
    compaction_config: &'a CompactionRuntimeConfig,
    summary_decision: &'a CompactionSummaryDecision,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(in crate::coord) struct SplitPrefixSummaryDecision {
    summary: String,
    source: SplitPrefixSummarySource,
    fallback_reason: Option<String>,
}

impl SplitPrefixSummaryDecision {
    pub(super) fn deterministic(summary: String) -> Self {
        Self {
            summary,
            source: SplitPrefixSummarySource::Deterministic,
            fallback_reason: None,
        }
    }

    pub(super) fn model(summary: String) -> Self {
        Self {
            summary,
            source: SplitPrefixSummarySource::ModelBacked,
            fallback_reason: None,
        }
    }

    pub(super) fn model_fallback(summary: String, reason: String) -> Self {
        Self {
            summary,
            source: SplitPrefixSummarySource::ModelBackedDeterministicFallback,
            fallback_reason: Some(reason),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SplitPrefixSummarySource {
    Deterministic,
    ModelBacked,
    ModelBackedDeterministicFallback,
}

impl SplitPrefixSummarySource {
    fn as_str(self) -> &'static str {
        match self {
            Self::Deterministic => "deterministic",
            Self::ModelBacked => "model_backed",
            Self::ModelBackedDeterministicFallback => "model_backed_deterministic_fallback",
        }
    }
}

pub(super) fn recorded_runtime_context_for_compaction(
    run_state: &RunState,
    trigger: &ProviderCompactionTrigger,
) -> RecordedRuntimeContext {
    let requested_model = AgentModelRef::parse(&trigger.model_ref);
    let requested_provider_id = trigger
        .provider_id
        .as_deref()
        .unwrap_or(requested_model.provider_id.as_str());
    let requested_model_id = trigger
        .model_id
        .as_deref()
        .unwrap_or(requested_model.model_id.as_str());

    if let Some(recorded) = run_state
        .recorded_runtime_context
        .as_ref()
        .filter(|context| {
            context.profile == trigger.profile_name
                && context.provider == requested_provider_id
                && context.model == requested_model_id
        })
    {
        return recorded.clone();
    }

    RecordedRuntimeContext::from_profile_model(&trigger.profile_name, &trigger.model_ref)
}

pub(super) fn should_compact_provider_context(
    context: &ProviderContext,
    metadata: &RecordedRuntimeContext,
    trigger: &ProviderCompactionTrigger,
    compaction_config: &CompactionRuntimeConfig,
) -> bool {
    if trigger.trigger_reason == "manual" {
        return context.preserved_turns.len() >= 2;
    }

    if trigger.trigger_reason == "overflow_retry" {
        return !context.is_empty();
    }

    if context.preserved_turns.len() < 2 {
        return false;
    }

    provider_context_trigger_estimate(context, metadata, trigger, compaction_config).is_some_and(
        |estimate| {
            estimate.tokens_before_estimate
                >= estimate.input_budget.saturating_sub(estimate.reserve)
        },
    )
}

pub(super) fn provider_context_trigger_estimate(
    context: &ProviderContext,
    metadata: &RecordedRuntimeContext,
    trigger: &ProviderCompactionTrigger,
    compaction_config: &CompactionRuntimeConfig,
) -> Option<ProviderContextTriggerEstimate> {
    let (input_budget, reserve, uses_fallback_budget) =
        if let Some(input_budget) = metadata.max_input_tokens.or(metadata.context_window_tokens) {
            (
                input_budget,
                provider_context_reserve_tokens(metadata, input_budget),
                false,
            )
        } else if compaction_config.estimated_token_triggers {
            let input_budget = compaction_config.fallback_input_tokens;
            if input_budget == 0 {
                return None;
            }
            (
                input_budget,
                PROVIDER_CONTEXT_COMPACTION_RESERVE_TOKENS.max(input_budget / 8),
                true,
            )
        } else {
            return None;
        };

    let context_tokens = approximate_provider_context_tokens(context);
    let tokens_before_estimate = trigger.tokens_before.unwrap_or_else(|| {
        context_tokens.saturating_add(trigger.prompt_tokens_estimate.unwrap_or(0))
    });
    let source = if uses_fallback_budget {
        "fallback_budget"
    } else if trigger.tokens_before.is_some() {
        "provider_usage"
    } else if trigger.prompt_tokens_estimate.is_some() {
        "estimated_context_and_prompt"
    } else {
        "estimated_context"
    };

    Some(ProviderContextTriggerEstimate {
        tokens_before_estimate,
        input_budget,
        reserve,
        source,
    })
}

fn build_provider_context_checkpoint(
    request: ProviderContextCheckpointRequest<'_>,
    redactor: &(impl Redactor + ?Sized),
) -> Option<ProviderContextCheckpoint> {
    let ProviderContextCheckpointRequest {
        run_state,
        trigger,
        context,
        keep_recent_budget,
        tokens_before_estimate,
        compaction_config,
        summary_decision,
    } = request;

    let plan = build_provider_context_compaction_plan(
        run_state,
        trigger,
        context,
        redactor,
        keep_recent_budget,
        compaction_config,
        summary_decision.split_prefix_summary.as_ref(),
    )?;
    let metadata = recorded_runtime_context_for_compaction(run_state, trigger);
    let summary_source = build_provider_compaction_summary_source(
        &metadata,
        trigger,
        context.compacted_summary.as_deref(),
        summary_decision.source.clone(),
        compaction_config,
    );
    let summary = summary_decision
        .summary
        .as_deref()
        .and_then(non_empty_trimmed)
        .map(|summary| {
            truncate_with_ellipsis(summary, PROVIDER_CONTEXT_COMPACTION_SUMMARY_MAX_CHARS)
        })
        .unwrap_or_else(|| {
            build_provider_context_summary(
                context.compacted_summary.as_deref(),
                &plan.older_turns,
                &plan.pruned_tool_artifacts,
                &plan.facts,
                &plan.tail_boundary,
                &summary_source,
                compaction_config,
            )
        });
    non_empty_trimmed(&summary)?;
    let summary_tokens_estimate = approximate_text_tokens(&summary);
    let preserved_tokens_estimate = preserved_tokens_estimate(&plan.recent_turns);
    let tokens_after_estimate = summary_tokens_estimate.saturating_add(preserved_tokens_estimate);
    let reduction_tokens_estimate = tokens_before_estimate.saturating_sub(tokens_after_estimate);
    let reduction_percent_estimate = (tokens_before_estimate > 0).then(|| {
        u32::try_from(
            (u64::from(reduction_tokens_estimate) * 100) / u64::from(tokens_before_estimate),
        )
        .unwrap_or(u32::MAX)
    });

    let first_kept_request_id = plan
        .recent_turns
        .first()
        .and_then(|turn| turn.request_id.clone());
    let timeline_entry = ProviderCompactionTimelineEntry {
        entry_type: if trigger.trigger_reason == "manual" {
            "manual_compaction".to_string()
        } else if trigger.trigger_reason == "overflow_retry" {
            "overflow_compaction".to_string()
        } else {
            "proactive_compaction".to_string()
        },
        summary: summarize_compaction_text(&summary),
        first_kept_request_id: first_kept_request_id.map(|r| r.to_string()),
        compacted_turns: u32::try_from(plan.older_turns.len()).unwrap_or(u32::MAX),
        preserved_turns: u32::try_from(plan.recent_turns.len()).unwrap_or(u32::MAX),
        tokens_before_estimate: Some(tokens_before_estimate),
        tokens_after_estimate: Some(tokens_after_estimate),
    };

    Some(ProviderContextCheckpoint {
        metadata: ProviderContextCheckpointMetadata {
            checkpoint_id: format!("checkpoint_{:06}", run_state.next_event_seq),
            agent_id: trigger.agent_id.clone(),
            run_id: run_state.info.run_id.to_string(),
            through_seq: run_state.next_event_seq.saturating_sub(1),
            through_request_id: trigger.through_request_id.clone(),
            provider_id: trigger.provider_id.clone(),
            model_id: trigger.model_id.clone(),
            tokens_before: trigger.tokens_before,
            tokens_before_estimate: Some(tokens_before_estimate),
            tokens_after_estimate: Some(tokens_after_estimate),
            summary_tokens_estimate: Some(summary_tokens_estimate),
            compacted_turns: Some(u32::try_from(plan.older_turns.len()).unwrap_or(u32::MAX)),
            preserved_turns: Some(u32::try_from(plan.recent_turns.len()).unwrap_or(u32::MAX)),
            reduction_tokens_estimate: Some(reduction_tokens_estimate),
            reduction_percent_estimate,
            trigger_reason: Some(trigger.trigger_reason.clone()),
        },
        summary,
        recent_turns: plan.recent_turns,
        pruned_tool_artifacts: plan.pruned_tool_artifacts,
        facts: plan.facts,
        tail_boundary: Some(plan.tail_boundary),
        summary_source: Some(summary_source),
        timeline_entry: Some(timeline_entry),
    })
}

pub(in crate::coord) fn serialize_provider_context_checkpoint(
    checkpoint: &ProviderContextCheckpoint,
    estimate_source: Option<&str>,
) -> Result<String, serde_json::Error> {
    let mut value = serde_json::to_value(checkpoint)?;
    if let (Some(source), Some(object)) = (estimate_source, value.as_object_mut()) {
        object.insert(
            "estimate_source".to_string(),
            serde_json::Value::String(source.to_string()),
        );
    }
    serde_json::to_string_pretty(&value)
}

pub(super) fn build_provider_context_compaction_plan(
    run_state: &RunState,
    trigger: &ProviderCompactionTrigger,
    context: &ProviderContext,
    redactor: &(impl Redactor + ?Sized),
    keep_recent_budget: u32,
    compaction_config: &CompactionRuntimeConfig,
    split_prefix_summary_override: Option<&SplitPrefixSummaryDecision>,
) -> Option<ProviderContextCompactionPlan> {
    let (mut older_turns, mut recent_turns, split_tail, split_prefix_summary) = if compaction_config
        .split_oversized_turns
    {
        if let Some((older_latest_turn, recent_latest_turn, split_prefix_summary)) =
            split_latest_oversized_turn(
                &context.preserved_turns,
                keep_recent_budget,
                trigger.trigger_reason.as_str(),
            )
        {
            let split_prefix_summary = split_prefix_summary_override
                .cloned()
                .unwrap_or(split_prefix_summary);
            let mut older_turns =
                context.preserved_turns[..context.preserved_turns.len() - 1].to_vec();
            older_turns.push(older_latest_turn);
            (
                older_turns,
                vec![recent_latest_turn],
                true,
                Some(split_prefix_summary),
            )
        } else if latest_oversized_turn_needs_summary_only(
            &context.preserved_turns,
            keep_recent_budget,
            trigger.trigger_reason.as_str(),
        ) {
            (context.preserved_turns.clone(), Vec::new(), false, None)
        } else if let Some(split_index) =
            provider_context_split_index(&context.preserved_turns, keep_recent_budget)
        {
            (
                context.preserved_turns[..split_index].to_vec(),
                context.preserved_turns[split_index..].to_vec(),
                false,
                None,
            )
        } else if trigger.trigger_reason == "manual" && context.preserved_turns.len() >= 2 {
            let split_index = context.preserved_turns.len() - 1;
            (
                context.preserved_turns[..split_index].to_vec(),
                context.preserved_turns[split_index..].to_vec(),
                false,
                None,
            )
        } else if trigger.trigger_reason == "overflow_retry" && !context.preserved_turns.is_empty()
        {
            (context.preserved_turns.clone(), Vec::new(), false, None)
        } else {
            return None;
        }
    } else if let Some(split_index) =
        provider_context_split_index(&context.preserved_turns, keep_recent_budget)
    {
        (
            context.preserved_turns[..split_index].to_vec(),
            context.preserved_turns[split_index..].to_vec(),
            false,
            None,
        )
    } else if trigger.trigger_reason == "manual" && context.preserved_turns.len() >= 2 {
        let split_index = context.preserved_turns.len() - 1;
        (
            context.preserved_turns[..split_index].to_vec(),
            context.preserved_turns[split_index..].to_vec(),
            false,
            None,
        )
    } else if trigger.trigger_reason == "overflow_retry" && !context.preserved_turns.is_empty() {
        (context.preserved_turns.clone(), Vec::new(), false, None)
    } else {
        return None;
    };

    for turn in older_turns.iter_mut().chain(recent_turns.iter_mut()) {
        sanitize_provider_turn_failure_metadata(turn, redactor);
    }

    let pruned_tool_artifacts =
        collect_pruned_tool_artifacts(run_state, trigger, context, &older_turns);
    let facts = build_provider_compaction_facts(
        run_state,
        trigger,
        context,
        &older_turns,
        &pruned_tool_artifacts,
        redactor,
    );
    let tail_boundary = build_provider_compaction_tail_boundary(
        &recent_turns,
        preserved_tokens_estimate(&recent_turns),
        keep_recent_budget,
        trigger,
        split_tail,
        split_prefix_summary,
    );

    Some(ProviderContextCompactionPlan {
        older_turns,
        recent_turns,
        pruned_tool_artifacts,
        facts,
        tail_boundary,
    })
}

pub(super) fn provider_context_keep_recent_tokens(metadata: &RecordedRuntimeContext) -> u32 {
    metadata
        .max_input_tokens
        .or(metadata.context_window_tokens)
        .map(|budget| {
            (budget / 4).clamp(
                PROVIDER_CONTEXT_COMPACTION_KEEP_RECENT_MIN_TOKENS,
                PROVIDER_CONTEXT_COMPACTION_KEEP_RECENT_MAX_TOKENS,
            )
        })
        .unwrap_or(2_048)
}

fn provider_context_reserve_tokens(metadata: &RecordedRuntimeContext, input_budget: u32) -> u32 {
    metadata
        .max_output_tokens
        .unwrap_or(PROVIDER_CONTEXT_COMPACTION_RESERVE_TOKENS)
        .max(PROVIDER_CONTEXT_COMPACTION_RESERVE_TOKENS)
        .min(input_budget.saturating_sub(1))
}

fn provider_context_split_index(
    turns: &[ProviderConversationTurn],
    keep_recent_budget: u32,
) -> Option<usize> {
    if turns.len() < 2 {
        return None;
    }

    let mut keep_from = turns.len() - 1;
    let mut kept_tokens = approximate_turn_tokens(&turns[keep_from]);
    for index in (0..keep_from).rev() {
        let candidate_tokens = approximate_turn_tokens(&turns[index]);
        if kept_tokens.saturating_add(candidate_tokens) > keep_recent_budget {
            break;
        }
        kept_tokens = kept_tokens.saturating_add(candidate_tokens);
        keep_from = index;
    }

    (keep_from > 0).then_some(keep_from)
}

fn split_latest_oversized_turn(
    turns: &[ProviderConversationTurn],
    keep_recent_budget: u32,
    trigger_reason: &str,
) -> Option<(
    ProviderConversationTurn,
    ProviderConversationTurn,
    SplitPrefixSummaryDecision,
)> {
    if turns.is_empty()
        || !matches!(
            trigger_reason,
            "manual" | "overflow_retry" | "pre_prompt" | "failed_response"
        )
    {
        return None;
    }

    let latest = turns.last()?;
    if !can_split_latest_turn_safely(latest) {
        return None;
    }

    let latest_tokens = approximate_turn_tokens(latest);
    if latest_tokens <= keep_recent_budget || latest.assistant_response.chars().count() < 2 {
        return None;
    }

    let suffix_chars = (keep_recent_budget.saturating_mul(4) as usize)
        .max(PROVIDER_CONTEXT_COMPACTION_TURN_EXCERPT_MAX_CHARS)
        .min(latest.assistant_response.chars().count().saturating_sub(1));
    let split_at = latest
        .assistant_response
        .chars()
        .count()
        .saturating_sub(suffix_chars);
    let assistant_prefix = latest
        .assistant_response
        .chars()
        .take(split_at)
        .collect::<String>();
    let assistant_suffix = latest
        .assistant_response
        .chars()
        .skip(split_at)
        .collect::<String>();
    if assistant_prefix.trim().is_empty() || assistant_suffix.trim().is_empty() {
        return None;
    }
    let split_prefix_summary =
        SplitPrefixSummaryDecision::deterministic(summarize_compaction_text(&assistant_prefix));

    let mut older_turn = latest.clone();
    older_turn.user_prompt = format!(
        "{}\n\n[Harness compaction note: earlier prefix of an oversized latest turn; this prefix is summarized in the checkpoint and the suffix remains provider-visible.]",
        latest.user_prompt
    );
    older_turn.assistant_response = assistant_prefix;
    older_turn.messages.clear();
    let mut recent_turn = latest.clone();
    recent_turn.user_prompt = format!(
        "{}\n\n[Harness compaction note: preserved suffix of an oversized latest turn; earlier prefix is summarized in the checkpoint.]",
        latest.user_prompt
    );
    recent_turn.assistant_response = assistant_suffix;
    recent_turn.messages.clear();
    Some((older_turn, recent_turn, split_prefix_summary))
}

fn can_split_latest_turn_safely(turn: &ProviderConversationTurn) -> bool {
    if !turn.artifacts.is_empty() {
        return false;
    }
    if turn.messages.iter().any(|message| match message {
        ConversationMessage::Assistant(assistant) => !assistant.tool_calls.is_empty(),
        ConversationMessage::ToolResult(_) => true,
        ConversationMessage::Checkpoint(_) | ConversationMessage::User(_) => false,
    }) {
        return false;
    }

    match turn.status {
        ProviderConversationTurnStatus::Completed => true,
        ProviderConversationTurnStatus::Failed => {
            turn.failure_stage.as_deref() == Some("provider_error")
        }
        ProviderConversationTurnStatus::Aborted => false,
    }
}

fn latest_oversized_turn_needs_summary_only(
    turns: &[ProviderConversationTurn],
    keep_recent_budget: u32,
    trigger_reason: &str,
) -> bool {
    if !matches!(trigger_reason, "overflow_retry" | "failed_response") {
        return false;
    }

    let Some(latest) = turns.last() else {
        return false;
    };
    approximate_turn_tokens(latest) > keep_recent_budget && !can_split_latest_turn_safely(latest)
}

fn sanitize_provider_turn_failure_metadata(
    turn: &mut ProviderConversationTurn,
    redactor: &(impl Redactor + ?Sized),
) {
    if let Some(reason) = turn.failure_reason.take() {
        let redacted = redactor.redact_text(&reason);
        let summarized = summarize_compaction_text(&redacted);
        turn.failure_reason = if non_empty_trimmed(&summarized).is_some() {
            Some(summarized)
        } else {
            None
        };
    }
}

fn build_provider_compaction_tail_boundary(
    recent_turns: &[ProviderConversationTurn],
    preserved_tokens_estimate: u32,
    keep_recent_budget: u32,
    trigger: &ProviderCompactionTrigger,
    split_tail: bool,
    split_prefix_summary: Option<SplitPrefixSummaryDecision>,
) -> ProviderCompactionTailBoundary {
    let first_preserved = recent_turns.first();
    let mode = if split_tail {
        "split_oversized_turn_tail".to_string()
    } else if recent_turns.is_empty() {
        "summary_only".to_string()
    } else if preserved_tokens_estimate > keep_recent_budget {
        "oversized_whole_turn_tail".to_string()
    } else {
        "whole_turn_tail".to_string()
    };
    let note = if mode == "split_oversized_turn_tail" {
        let mut note = "The latest oversized turn was split inside the checkpoint artifact: the earlier prefix is summarized in the checkpoint and a suffix remains provider-visible as recent context.".to_string();
        if let Some(split_prefix_summary) = split_prefix_summary.as_ref() {
            note.push_str(" Split prefix summary source: ");
            note.push_str(split_prefix_summary.source.as_str());
            note.push('.');
            if let Some(reason) = split_prefix_summary.fallback_reason.as_deref() {
                note.push_str(" Fallback reason: ");
                note.push_str(&summarize_compaction_text(reason));
                note.push('.');
            }
        }
        Some(note)
    } else if mode == "oversized_whole_turn_tail" {
        Some("Latest preserved turn exceeds the keep-recent budget; the harness records this tail boundary but does not split provider/tool turns yet.".to_string())
    } else if matches!(
        trigger.trigger_reason.as_str(),
        "overflow_retry" | "failed_response"
    ) && recent_turns.is_empty()
    {
        Some(format!(
            "{} compaction used summary-only context because preserving or splitting the latest oversized turn would risk invalid provider ordering or still exceed the provider window.",
            trigger.trigger_reason
        ))
    } else {
        None
    };

    ProviderCompactionTailBoundary {
        mode,
        preserved_turns: u32::try_from(recent_turns.len()).unwrap_or(u32::MAX),
        preserved_tokens_estimate,
        preserved_from_request_id: first_preserved
            .and_then(|turn| turn.request_id.clone())
            .map(|r| r.to_string()),
        preserved_from_seq: first_preserved.and_then(|turn| turn.first_seq),
        split_prefix_summary: split_prefix_summary.map(|decision| decision.summary),
        note,
    }
}

fn build_provider_compaction_summary_source(
    metadata: &RecordedRuntimeContext,
    trigger: &ProviderCompactionTrigger,
    existing_summary: Option<&str>,
    request: SummarySourceRequest,
    config: &CompactionRuntimeConfig,
) -> ProviderCompactionSummarySource {
    let (strategy, model_ref, model_backed, deterministic_fallback) = match request {
        SummarySourceRequest::Hook => (
            "hook_supplied_summary".to_string(),
            trigger.model_ref.clone(),
            false,
            false,
        ),
        SummarySourceRequest::Model {
            model_ref,
            deterministic_fallback,
        } => (
            if deterministic_fallback {
                "model_backed_deterministic_fallback".to_string()
            } else {
                "model_backed_summary".to_string()
            },
            model_ref,
            true,
            deterministic_fallback,
        ),
        SummarySourceRequest::Deterministic => (
            "deterministic_rolling_summary".to_string(),
            trigger.model_ref.clone(),
            false,
            true,
        ),
        SummarySourceRequest::DeterministicForModelRef { model_ref } => (
            "deterministic_rolling_summary".to_string(),
            model_ref,
            false,
            true,
        ),
    };
    ProviderCompactionSummarySource {
        strategy,
        model_ref,
        provider_id: trigger
            .provider_id
            .clone()
            .or_else(|| Some(metadata.provider.clone())),
        model_id: trigger
            .model_id
            .clone()
            .or_else(|| Some(metadata.model.clone())),
        reasoning_effort: metadata.reasoning_effort.clone(),
        text_verbosity: metadata.text_verbosity.clone(),
        previous_summary_used: existing_summary.and_then(non_empty_trimmed).is_some(),
        model_backed,
        deterministic_fallback,
        summary_contract_version: Some(PROVIDER_CONTEXT_SUMMARY_CONTRACT_VERSION),
        summary_contract_enforced: Some(config.structured_summary_contract),
    }
}

fn collect_pruned_tool_artifacts(
    run_state: &RunState,
    trigger: &ProviderCompactionTrigger,
    context: &ProviderContext,
    older_turns: &[ProviderConversationTurn],
) -> Vec<EventArtifactRef> {
    if older_turns.is_empty() {
        return Vec::new();
    }

    let lower_bound_seq = context
        .checkpoint
        .as_ref()
        .map(|checkpoint| checkpoint.through_seq)
        .unwrap_or(0);
    let historical_turns = match collect_historical_agent_turns_until(
        run_state.info.run_id.as_str(),
        &run_state.info.events_path,
        &trigger.agent_id,
        lower_bound_seq,
        run_state.next_event_seq.saturating_sub(1),
    ) {
        Ok(turns) => turns,
        Err(_) => return Vec::new(),
    };

    if historical_turns.len() < context.preserved_turns.len() {
        return Vec::new();
    }

    let aligned_turns = &historical_turns[historical_turns.len() - context.preserved_turns.len()..];
    if !aligned_turns
        .iter()
        .zip(&context.preserved_turns)
        .all(|(historical, current)| {
            historical.user_prompt == current.user_prompt
                && historical.assistant_response == current.assistant_response
        })
    {
        return Vec::new();
    }

    let mut refs = Vec::new();
    let mut seen = BTreeSet::new();
    for historical in aligned_turns.iter().take(older_turns.len()) {
        for artifact in &historical.artifact_refs {
            let key = (artifact.path.clone(), artifact.digest.clone());
            if seen.insert(key) {
                refs.push(artifact.clone());
            }
        }
    }
    refs
}

pub(super) fn build_deterministic_provider_compaction_summary_source(
    metadata: &RecordedRuntimeContext,
    trigger: &ProviderCompactionTrigger,
    existing_summary: Option<&str>,
    config: &CompactionRuntimeConfig,
) -> ProviderCompactionSummarySource {
    build_provider_compaction_summary_source(
        metadata,
        trigger,
        existing_summary,
        SummarySourceRequest::Deterministic,
        config,
    )
}
