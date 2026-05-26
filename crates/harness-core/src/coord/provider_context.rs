use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::io::{self, BufRead, BufReader};
use std::path::Path;
use std::sync::Arc;

use serde_json::Value;
use tokio_stream::StreamExt;

use harness_providers::{
    CompletionMessage, CompletionRequest, MessageRole, Provider, ProviderStreamEvent,
};

use crate::agent::{
    AgentModelRef, ProviderCompactionFacts, ProviderCompactionSummarySource,
    ProviderCompactionTailBoundary, ProviderCompactionTimelineEntry, ProviderCompactionTurnFact,
    ProviderContext, ProviderContextCheckpoint, ProviderContextCheckpointMetadata,
    ProviderConversationTurn, ProviderConversationTurnStatus, ProviderFileOperationFact,
};
use crate::config::CompactionRuntimeConfig;
use crate::conversation::{
    ConversationAssistantMessage, ConversationMessage, ConversationToolCall,
    ConversationToolResultMessage, ConversationUserMessage,
};
use crate::event::{
    ArtifactWrittenEvent, EventArtifactRef, EventEnvelopeV1, EventV1, ResolvedToolIdentity,
    TaskCancelledEvent, TaskCompletedEvent, TaskTerminalScope, ToolCallMetadata,
    ToolIdentityMetadata,
};
use crate::path_selector::workspace_relative_path_from_maybe_absolute;
use crate::proj::RecordedRuntimeContext;
use crate::provider_args::provider_tool_arguments_json;
use crate::redact::Redactor;
use crate::session_paths::EVENTS_FILE_NAME;
use crate::text::{non_empty_trimmed, truncate_with_ellipsis};

use super::{truncated_failure_reason, CoordinatorError, RunState};

const PROVIDER_CONTEXT_COMPACTION_RESERVE_TOKENS: u32 = 1_024;
const PROVIDER_CONTEXT_COMPACTION_KEEP_RECENT_MAX_TOKENS: u32 = 8_000;
const PROVIDER_CONTEXT_COMPACTION_KEEP_RECENT_MIN_TOKENS: u32 = 2_000;
const PROVIDER_CONTEXT_COMPACTION_SUMMARY_MAX_CHARS: usize = 6_000;
pub(super) const PROVIDER_CONTEXT_COMPACTION_TURN_EXCERPT_MAX_CHARS: usize = 240;
const PROVIDER_CONTEXT_SPLIT_PREFIX_SUMMARY_MAX_CHARS: usize = 1_200;
const PROVIDER_CONTEXT_FILE_OPERATION_FACT_LIMIT: usize = 50;
const PROVIDER_CONTEXT_OPERATION_FACT_LIMIT: usize = 20;
pub(super) const PROVIDER_CONTEXT_SUMMARY_CONTRACT_VERSION: u32 = 2;
const PROVIDER_CONTEXT_SPLIT_PREFIX_SUMMARY_HEADINGS: &[&str] = &[
    "## Original Request",
    "## Early Progress",
    "## Context for Suffix",
];
const PROVIDER_CONTEXT_HARNESS_SUMMARY_HEADINGS: &[&str] = &[
    "## Goal",
    "## Constraints",
    "## Progress",
    "## Key Decisions",
    "## Next Steps",
    "## Critical Context",
];
const PROVIDER_CONTEXT_LEGACY_SUMMARY_HEADINGS: &[&str] = &[
    "## Goal",
    "## Constraints & Preferences",
    "## Progress",
    "### Done",
    "### In Progress",
    "### Blocked",
    "## Key Decisions",
    "## Next Steps",
    "## Critical Context",
    "## Source Facts",
    "## Relevant Files / Artifacts",
];

pub(super) fn provider_context_summary_required_headings(
    config: &CompactionRuntimeConfig,
) -> &'static [&'static str] {
    if config.structured_summary_contract {
        PROVIDER_CONTEXT_HARNESS_SUMMARY_HEADINGS
    } else {
        PROVIDER_CONTEXT_LEGACY_SUMMARY_HEADINGS
    }
}
#[derive(Debug, Clone)]
pub(super) struct ProviderCompactionTrigger {
    pub(super) agent_id: String,
    pub(super) profile_name: String,
    pub(super) model_ref: String,
    pub(super) provider_id: Option<String>,
    pub(super) model_id: Option<String>,
    pub(super) through_request_id: Option<String>,
    pub(super) trigger_reason: String,
    pub(super) tokens_before: Option<u32>,
    pub(super) prompt_tokens_estimate: Option<u32>,
    pub(super) estimate_source: Option<String>,
}

#[derive(Debug, Clone, Copy)]
pub(super) struct ProviderContextTriggerEstimate {
    pub(super) tokens_before_estimate: u32,
    pub(super) input_budget: u32,
    pub(super) reserve: u32,
    pub(super) source: &'static str,
}
#[derive(Debug, Clone)]
pub(super) struct CompactionSummaryDecision {
    summary: Option<String>,
    source: SummarySourceRequest,
    split_prefix_summary: Option<SplitPrefixSummaryDecision>,
}

impl CompactionSummaryDecision {
    pub(super) fn deterministic(trigger: &ProviderCompactionTrigger) -> Self {
        Self {
            summary: None,
            source: SummarySourceRequest::DeterministicForModelRef {
                model_ref: trigger.model_ref.clone(),
            },
            split_prefix_summary: None,
        }
    }

    pub(super) fn hook(summary: String) -> Self {
        Self {
            summary: Some(summary),
            source: SummarySourceRequest::Hook,
            split_prefix_summary: None,
        }
    }

    pub(super) fn model(
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
pub(super) struct ProviderContextCompactionPlan {
    pub(super) older_turns: Vec<ProviderConversationTurn>,
    pub(super) recent_turns: Vec<ProviderConversationTurn>,
    pub(super) pruned_tool_artifacts: Vec<EventArtifactRef>,
    pub(super) facts: ProviderCompactionFacts,
    pub(super) tail_boundary: ProviderCompactionTailBoundary,
}

pub(super) struct ProviderContextCompactionRequest<'a> {
    run_state: &'a RunState,
    trigger: ProviderCompactionTrigger,
    compaction_config: &'a CompactionRuntimeConfig,
    summary_decision: &'a CompactionSummaryDecision,
}

impl<'a> ProviderContextCompactionRequest<'a> {
    pub(super) fn new(
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

    pub(super) fn plan(
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

pub(super) struct ProviderContextCompactionDecision {
    pub(super) trigger: ProviderCompactionTrigger,
    pub(super) checkpoint: ProviderContextCheckpoint,
    pub(super) updated_context: ProviderContext,
    pub(super) tokens_before_estimate: u32,
    pub(super) tokens_after_estimate: u32,
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
pub(super) struct SplitPrefixSummaryDecision {
    summary: String,
    source: SplitPrefixSummarySource,
    fallback_reason: Option<String>,
}

impl SplitPrefixSummaryDecision {
    fn deterministic(summary: String) -> Self {
        Self {
            summary,
            source: SplitPrefixSummarySource::Deterministic,
            fallback_reason: None,
        }
    }

    fn model(summary: String) -> Self {
        Self {
            summary,
            source: SplitPrefixSummarySource::ModelBacked,
            fallback_reason: None,
        }
    }

    fn model_fallback(summary: String, reason: String) -> Self {
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

#[derive(Debug, Clone)]
pub(super) struct ModelBackedCompactionSummary {
    pub(super) summary: String,
    pub(super) split_prefix_summary: Option<SplitPrefixSummaryDecision>,
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
        ((u64::from(reduction_tokens_estimate) * 100) / u64::from(tokens_before_estimate)) as u32
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
        first_kept_request_id,
        compacted_turns: plan.older_turns.len() as u32,
        preserved_turns: plan.recent_turns.len() as u32,
        tokens_before_estimate: Some(tokens_before_estimate),
        tokens_after_estimate: Some(tokens_after_estimate),
    };

    Some(ProviderContextCheckpoint {
        metadata: ProviderContextCheckpointMetadata {
            checkpoint_id: format!("checkpoint_{:06}", run_state.next_event_seq),
            agent_id: trigger.agent_id.clone(),
            run_id: run_state.info.run_id.clone(),
            through_seq: run_state.next_event_seq.saturating_sub(1),
            through_request_id: trigger.through_request_id.clone(),
            provider_id: trigger.provider_id.clone(),
            model_id: trigger.model_id.clone(),
            tokens_before: trigger.tokens_before,
            tokens_before_estimate: Some(tokens_before_estimate),
            tokens_after_estimate: Some(tokens_after_estimate),
            summary_tokens_estimate: Some(summary_tokens_estimate),
            compacted_turns: Some(plan.older_turns.len() as u32),
            preserved_turns: Some(plan.recent_turns.len() as u32),
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

pub(super) fn serialize_provider_context_checkpoint(
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

fn build_provider_context_compaction_plan(
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
    let operational_memory =
        collect_compacted_file_operation_facts(run_state, trigger, context, &older_turns, redactor);
    let facts = build_provider_compaction_facts(
        context,
        &older_turns,
        &pruned_tool_artifacts,
        operational_memory,
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

fn preserved_tokens_estimate(turns: &[ProviderConversationTurn]) -> u32 {
    turns.iter().map(approximate_turn_tokens).sum::<u32>()
}

#[derive(Debug, Clone, Default)]
struct ProviderOperationalMemoryFacts {
    read_files: Vec<ProviderFileOperationFact>,
    modified_files: Vec<ProviderFileOperationFact>,
    operation_facts: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ProviderFileOperationKind {
    Read,
    Modified,
}

impl ProviderFileOperationKind {
    fn as_str(self) -> &'static str {
        match self {
            Self::Read => "read",
            Self::Modified => "modified",
        }
    }
}

fn collect_compacted_file_operation_facts(
    run_state: &RunState,
    trigger: &ProviderCompactionTrigger,
    context: &ProviderContext,
    older_turns: &[ProviderConversationTurn],
    redactor: &(impl Redactor + ?Sized),
) -> ProviderOperationalMemoryFacts {
    if older_turns.is_empty() {
        return ProviderOperationalMemoryFacts::default();
    }

    let lower_bound_seq = context
        .checkpoint
        .as_ref()
        .map(|checkpoint| checkpoint.through_seq)
        .unwrap_or(0);
    let through_seq = run_state.next_event_seq.saturating_sub(1);
    let compacted_request_ids = compacted_request_ids_for_operational_memory(
        run_state,
        trigger,
        context,
        older_turns,
        lower_bound_seq,
        through_seq,
    );
    if compacted_request_ids.is_empty() {
        return ProviderOperationalMemoryFacts::default();
    }

    let events = match read_historical_events_until(
        &run_state.info.run_id,
        &run_state.info.events_path,
        through_seq,
    ) {
        Ok(events) => events,
        Err(_) => return ProviderOperationalMemoryFacts::default(),
    };

    let mut tool_operations: BTreeMap<String, ProviderFileOperationKind> = BTreeMap::new();
    let mut tool_output_paths: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for event in events
        .iter()
        .filter(|event| event.seq > lower_bound_seq && event.seq <= through_seq)
    {
        if !event_belongs_to_compacted_request(event, &compacted_request_ids) {
            continue;
        }
        match &event.payload {
            EventV1::ToolCallRequested(payload) => {
                if let Some(operation) = tool_call_operation(
                    Some(payload.tool_id.as_str()),
                    payload.metadata.as_ref(),
                    None,
                ) {
                    tool_operations.insert(payload.tool_call_id.clone(), operation);
                }
            }
            EventV1::ToolCallFinished(payload) => {
                if let Some(operation) = tool_call_operation(None, payload.metadata.as_ref(), None)
                {
                    tool_operations
                        .entry(payload.tool_call_id.clone())
                        .or_insert(operation);
                }
                let paths = extract_output_json_path_fields(payload.output_json.as_ref());
                if !paths.is_empty() {
                    tool_output_paths.insert(payload.tool_call_id.clone(), paths);
                }
            }
            _ => {}
        }
    }

    let mut read = BTreeMap::new();
    let mut modified = BTreeMap::new();
    for event in events
        .iter()
        .filter(|event| event.seq > lower_bound_seq && event.seq <= through_seq)
    {
        if !event_belongs_to_compacted_request(event, &compacted_request_ids) {
            continue;
        }
        match &event.payload {
            EventV1::EditApplied(payload) => {
                add_file_operation_fact(
                    &mut modified,
                    &run_state.info.workspace_root,
                    &payload.path,
                    ProviderFileOperationKind::Modified,
                    event.seq,
                    format!("edit:{}", payload.edit_id),
                    None,
                    redactor,
                );
            }
            EventV1::ArtifactWritten(payload) => {
                let Some(tool_call_id) = payload.tool_call_id.as_deref() else {
                    continue;
                };
                let operation = tool_call_operation(None, None, payload.tool_metadata.as_ref())
                    .or_else(|| tool_operations.get(tool_call_id).copied())
                    .unwrap_or(ProviderFileOperationKind::Read);
                let paths = extract_artifact_workspace_paths(
                    payload,
                    tool_output_paths.get(tool_call_id).map(Vec::as_slice),
                );
                let summary = payload
                    .metadata
                    .get("summary")
                    .or_else(|| payload.metadata.get("operation_summary"))
                    .map(|value| summarize_compaction_text(value));
                for path in paths {
                    let target = match operation {
                        ProviderFileOperationKind::Read => &mut read,
                        ProviderFileOperationKind::Modified => &mut modified,
                    };
                    add_file_operation_fact(
                        target,
                        &run_state.info.workspace_root,
                        &path,
                        operation,
                        event.seq,
                        format!("artifact:{tool_call_id}"),
                        summary.clone(),
                        redactor,
                    );
                }
            }
            EventV1::ToolCallFinished(payload) => {
                let operation = tool_operations
                    .get(&payload.tool_call_id)
                    .copied()
                    .or_else(|| tool_call_operation(None, payload.metadata.as_ref(), None));
                if operation != Some(ProviderFileOperationKind::Read) {
                    continue;
                }
                for path in extract_output_json_path_fields(payload.output_json.as_ref()) {
                    add_file_operation_fact(
                        &mut read,
                        &run_state.info.workspace_root,
                        &path,
                        ProviderFileOperationKind::Read,
                        event.seq,
                        format!("tool:{}", payload.tool_call_id),
                        payload
                            .output_summary
                            .as_deref()
                            .map(summarize_compaction_text),
                        redactor,
                    );
                }
            }
            _ => {}
        }
    }

    finalize_provider_operational_memory(read, modified)
}

fn compacted_request_ids_for_operational_memory(
    run_state: &RunState,
    trigger: &ProviderCompactionTrigger,
    context: &ProviderContext,
    older_turns: &[ProviderConversationTurn],
    lower_bound_seq: u64,
    through_seq: u64,
) -> BTreeSet<String> {
    let mut request_ids = older_turns
        .iter()
        .filter_map(|turn| turn.request_id.as_deref())
        .map(str::to_string)
        .collect::<BTreeSet<_>>();
    if !request_ids.is_empty() {
        return request_ids;
    }

    let Ok(historical_turns) = collect_historical_agent_turns_until(
        &run_state.info.run_id,
        &run_state.info.events_path,
        &trigger.agent_id,
        lower_bound_seq,
        through_seq,
    ) else {
        return BTreeSet::new();
    };
    if historical_turns.len() < context.preserved_turns.len() {
        return BTreeSet::new();
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
        return BTreeSet::new();
    }
    request_ids.extend(
        aligned_turns
            .iter()
            .take(older_turns.len())
            .map(|turn| turn.request_id.clone()),
    );
    request_ids
}

fn read_historical_events_until(
    run_id: &str,
    events_path: &Path,
    through_seq: u64,
) -> Result<Vec<EventEnvelopeV1>, CoordinatorError> {
    let file =
        fs::File::open(events_path).map_err(|source| CoordinatorError::ResumeRestoreFailed {
            run_id: run_id.to_string(),
            reason: format!(
                "failed to open historical events {}: {source}",
                events_path.display()
            ),
        })?;
    let mut expected_seq = 1_u64;
    let mut events = Vec::new();
    for (line_number, line) in BufReader::new(file).lines().enumerate() {
        let Some(event) = parse_historical_event_line(run_id, events_path, line_number, line)?
        else {
            continue;
        };
        validate_historical_event_seq(run_id, events_path, &event, expected_seq)?;
        expected_seq = expected_seq.saturating_add(1);
        if event.seq > through_seq {
            break;
        }
        events.push(event);
    }
    Ok(events)
}

fn parse_historical_event_line(
    run_id: &str,
    events_path: &Path,
    line_number: usize,
    line: io::Result<String>,
) -> Result<Option<EventEnvelopeV1>, CoordinatorError> {
    let line = line.map_err(|source| CoordinatorError::ResumeRestoreFailed {
        run_id: run_id.to_string(),
        reason: format!(
            "failed to read historical event line {} in {}: {source}",
            line_number + 1,
            events_path.display()
        ),
    })?;
    if line.trim().is_empty() {
        return Ok(None);
    }
    serde_json::from_str(&line)
        .map(Some)
        .map_err(|source| CoordinatorError::ResumeRestoreFailed {
            run_id: run_id.to_string(),
            reason: format!(
                "invalid historical event line {} in {}: {source}",
                line_number + 1,
                events_path.display()
            ),
        })
}

fn validate_historical_event_seq(
    run_id: &str,
    events_path: &Path,
    event: &EventEnvelopeV1,
    expected_seq: u64,
) -> Result<(), CoordinatorError> {
    if event.seq == expected_seq {
        return Ok(());
    }
    Err(CoordinatorError::ResumeRestoreFailed {
        run_id: run_id.to_string(),
        reason: format!(
            "historical sequence mismatch at {}: expected {expected_seq}, got {}",
            events_path.display(),
            event.seq
        ),
    })
}

fn event_belongs_to_compacted_request(
    event: &EventEnvelopeV1,
    compacted_request_ids: &BTreeSet<String>,
) -> bool {
    event
        .correlation_id
        .as_deref()
        .is_some_and(|request_id| compacted_request_ids.contains(request_id))
}

fn tool_call_operation(
    invoked_tool_id: Option<&str>,
    call_metadata: Option<&ToolCallMetadata>,
    artifact_metadata: Option<&ToolIdentityMetadata>,
) -> Option<ProviderFileOperationKind> {
    let identity = if artifact_metadata.is_some() {
        ResolvedToolIdentity::from_tool_artifact(invoked_tool_id, artifact_metadata)
    } else {
        ResolvedToolIdentity::from_tool_call(invoked_tool_id, call_metadata)
    };
    let operation = [
        identity.canonical_tool_id.as_deref(),
        identity.effective_tool_id.as_deref(),
        identity.invoked_tool_id.as_deref(),
        identity.alias_source_tool_id.as_deref(),
    ]
    .into_iter()
    .flatten()
    .find_map(operation_for_tool_id);
    operation
}

fn operation_for_tool_id(tool_id: &str) -> Option<ProviderFileOperationKind> {
    let normalized = tool_id.trim().to_ascii_lowercase();
    if matches!(
        normalized.as_str(),
        "edit" | "apply" | "edit.hashline_apply"
    ) {
        return Some(ProviderFileOperationKind::Modified);
    }
    if matches!(
        normalized.as_str(),
        "read" | "grep" | "glob" | "list" | "lsp"
    ) || normalized.starts_with("lsp.")
    {
        return Some(ProviderFileOperationKind::Read);
    }
    None
}

fn extract_output_json_path_fields(output_json: Option<&Value>) -> Vec<String> {
    let Some(value) = output_json else {
        return Vec::new();
    };
    let mut paths = Vec::new();
    collect_direct_path_fields(value, &mut paths);
    for key in ["files", "matches"] {
        if let Some(items) = value.get(key).and_then(Value::as_array) {
            for item in items {
                collect_direct_path_fields(item, &mut paths);
            }
        }
    }
    paths
}

fn collect_direct_path_fields(value: &Value, paths: &mut Vec<String>) {
    for key in ["path", "filePath", "file_path"] {
        if let Some(path) = value
            .get(key)
            .and_then(Value::as_str)
            .and_then(non_empty_trimmed)
        {
            paths.push(path.to_string());
        }
    }
}

fn extract_artifact_workspace_paths(
    payload: &ArtifactWrittenEvent,
    output_paths: Option<&[String]>,
) -> Vec<String> {
    let mut paths = Vec::new();
    for key in ["path", "filePath", "file_path"] {
        if let Some(path) = payload
            .metadata
            .get(key)
            .map(String::as_str)
            .and_then(non_empty_trimmed)
        {
            paths.push(path.to_string());
        }
    }
    if let Some(output_paths) = output_paths {
        paths.extend(output_paths.iter().cloned());
    }
    paths.sort();
    paths.dedup();
    paths
}

#[expect(
    clippy::too_many_arguments,
    reason = "operational-memory fact construction keeps path normalization, provenance, and redaction inputs explicit"
)]
fn add_file_operation_fact(
    facts: &mut BTreeMap<(String, String), ProviderFileOperationFact>,
    workspace_root: &Path,
    raw_path: &str,
    operation: ProviderFileOperationKind,
    seq: u64,
    source: String,
    summary: Option<String>,
    redactor: &(impl Redactor + ?Sized),
) {
    let Some(path) =
        workspace_relative_path_from_maybe_absolute(workspace_root, Path::new(raw_path))
    else {
        return;
    };
    let path = redactor.redact_text(&path);
    let operation = operation.as_str().to_string();
    let summary = summary
        .map(|summary| redactor.redact_text(&summary))
        .map(|summary| summarize_compaction_text(&summary));
    let fact = facts
        .entry((path.clone(), operation.clone()))
        .or_insert_with(|| ProviderFileOperationFact {
            path,
            operation,
            first_seq: Some(seq),
            last_seq: Some(seq),
            sources: Vec::new(),
            summary: None,
        });
    fact.first_seq = Some(fact.first_seq.map_or(seq, |first_seq| first_seq.min(seq)));
    fact.last_seq = Some(fact.last_seq.map_or(seq, |last_seq| last_seq.max(seq)));
    if !fact.sources.iter().any(|existing| existing == &source) {
        fact.sources.push(source);
        fact.sources.sort();
    }
    if fact.summary.is_none() {
        fact.summary = summary;
    }
}

fn finalize_provider_operational_memory(
    read: BTreeMap<(String, String), ProviderFileOperationFact>,
    modified: BTreeMap<(String, String), ProviderFileOperationFact>,
) -> ProviderOperationalMemoryFacts {
    let (read_files, read_omitted) = cap_file_operation_facts(read);
    let (modified_files, modified_omitted) = cap_file_operation_facts(modified);
    let mut operation_facts = Vec::new();
    if read_omitted > 0 {
        operation_facts.push(format!("{read_omitted} additional read file(s) omitted"));
    }
    if modified_omitted > 0 {
        operation_facts.push(format!(
            "{modified_omitted} additional modified file(s) omitted"
        ));
    }
    for fact in read_files.iter().chain(modified_files.iter()) {
        if operation_facts.len() >= PROVIDER_CONTEXT_OPERATION_FACT_LIMIT {
            break;
        }
        let sources = if fact.sources.is_empty() {
            "unknown source".to_string()
        } else {
            fact.sources.join(", ")
        };
        let mut line = format!("{} {} via {}", fact.operation, fact.path, sources);
        if let Some(summary) = fact
            .summary
            .as_deref()
            .filter(|summary| !summary.is_empty())
        {
            line.push_str(": ");
            line.push_str(summary);
        }
        operation_facts.push(summarize_compaction_text(&line));
    }
    operation_facts.truncate(PROVIDER_CONTEXT_OPERATION_FACT_LIMIT);
    ProviderOperationalMemoryFacts {
        read_files,
        modified_files,
        operation_facts,
    }
}

fn cap_file_operation_facts(
    facts: BTreeMap<(String, String), ProviderFileOperationFact>,
) -> (Vec<ProviderFileOperationFact>, usize) {
    let total = facts.len();
    let retained = facts
        .into_values()
        .take(PROVIDER_CONTEXT_FILE_OPERATION_FACT_LIMIT)
        .collect::<Vec<_>>();
    (
        retained,
        total.saturating_sub(PROVIDER_CONTEXT_FILE_OPERATION_FACT_LIMIT),
    )
}

fn build_provider_compaction_facts(
    context: &ProviderContext,
    older_turns: &[ProviderConversationTurn],
    pruned_tool_artifacts: &[EventArtifactRef],
    operational_memory: ProviderOperationalMemoryFacts,
) -> ProviderCompactionFacts {
    let compacted_turns = older_turns
        .iter()
        .map(|turn| ProviderCompactionTurnFact {
            request_id: turn.request_id.clone(),
            first_seq: turn.first_seq,
            last_seq: turn.last_seq,
            user_excerpt: summarize_compaction_text(&turn.user_prompt),
            assistant_excerpt: summarize_compaction_text(&turn.assistant_response),
            status: turn.status,
            failure_stage: turn.failure_stage.clone(),
            failure_reason: turn.failure_reason.clone(),
            artifacts: turn.artifacts.clone(),
        })
        .collect::<Vec<_>>();

    let mut relevant_artifacts = Vec::new();
    let mut artifact_seen = BTreeSet::new();
    for artifact in pruned_tool_artifacts
        .iter()
        .chain(older_turns.iter().flat_map(|turn| turn.artifacts.iter()))
    {
        let key = (artifact.path.clone(), artifact.digest.clone());
        if artifact_seen.insert(key) {
            relevant_artifacts.push(artifact.clone());
        }
    }

    let mut touched_files = operational_memory
        .read_files
        .iter()
        .chain(operational_memory.modified_files.iter())
        .map(|fact| fact.path.clone())
        .collect::<Vec<_>>();
    touched_files.sort();
    touched_files.dedup();

    ProviderCompactionFacts {
        previous_checkpoint_id: context
            .checkpoint
            .as_ref()
            .map(|checkpoint| checkpoint.checkpoint_id.clone()),
        compacted_turns,
        relevant_artifacts,
        read_files: operational_memory.read_files,
        modified_files: operational_memory.modified_files,
        operation_facts: operational_memory.operation_facts,
        touched_files,
        pending_work: Vec::new(),
        blockers: Vec::new(),
    }
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
        preserved_turns: recent_turns.len() as u32,
        preserved_tokens_estimate,
        preserved_from_request_id: first_preserved.and_then(|turn| turn.request_id.clone()),
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

pub(super) fn compaction_summary_model_ref(
    config: &CompactionRuntimeConfig,
    trigger: &ProviderCompactionTrigger,
) -> String {
    config
        .model_ref
        .as_deref()
        .and_then(non_empty_trimmed)
        .unwrap_or(trigger.model_ref.as_str())
        .to_string()
}

pub(super) async fn model_backed_compaction_summary_for(
    provider: Arc<dyn Provider>,
    compaction_config: &CompactionRuntimeConfig,
    run_state: &RunState,
    trigger: &ProviderCompactionTrigger,
    redactor: &(impl Redactor + ?Sized),
) -> Result<ModelBackedCompactionSummary, String> {
    let context = run_state
        .provider_context_by_agent
        .get(&trigger.agent_id)
        .cloned()
        .unwrap_or_default();
    let metadata = recorded_runtime_context_for_compaction(run_state, trigger);
    if !should_compact_provider_context(&context, &metadata, trigger, compaction_config) {
        return Err("compaction would be a no-op".to_string());
    }

    let keep_recent_budget = provider_context_keep_recent_tokens(&metadata);
    let tokens_before = approximate_provider_context_tokens(&context);
    let Some(initial_plan) = build_provider_context_compaction_plan(
        run_state,
        trigger,
        &context,
        redactor,
        keep_recent_budget,
        compaction_config,
        None,
    ) else {
        return Err("no compactable provider turns were available".to_string());
    };
    let model_ref = compaction_summary_model_ref(compaction_config, trigger);
    let split_prefix_summary = model_backed_split_prefix_summary_decision(
        provider.clone(),
        &model_ref,
        &initial_plan,
        trigger,
    )
    .await;
    let plan = if let Some(split_prefix_summary) = split_prefix_summary.as_ref() {
        build_provider_context_compaction_plan(
            run_state,
            trigger,
            &context,
            redactor,
            keep_recent_budget,
            compaction_config,
            Some(split_prefix_summary),
        )
        .ok_or_else(|| "no compactable provider turns were available".to_string())?
    } else {
        initial_plan
    };
    let draft_source = build_provider_compaction_summary_source(
        &metadata,
        trigger,
        context.compacted_summary.as_deref(),
        SummarySourceRequest::Deterministic,
        compaction_config,
    );
    let deterministic_draft = build_provider_context_summary(
        context.compacted_summary.as_deref(),
        &plan.older_turns,
        &plan.pruned_tool_artifacts,
        &plan.facts,
        &plan.tail_boundary,
        &draft_source,
        compaction_config,
    );
    let model = AgentModelRef::parse(&model_ref);
    let request = CompletionRequest {
        provider_id: Some(model.provider_id),
        model_id: model.model_id,
        messages: vec![
            CompletionMessage {
                role: MessageRole::System,
                content: "You create Harness provider-context checkpoint summaries. Return only the updated structured checkpoint summary, preserving the requested markdown headings and rolling forward prior summary content instead of appending a raw previous-summary blob.".to_string(),
                name: None,
                tool_call_id: None,
                assistant_tool_calls: None,
            },
            CompletionMessage {
                role: MessageRole::User,
                content: build_model_compaction_prompt(
                    context.compacted_summary.as_deref(),
                    &plan,
                    &deterministic_draft,
                    compaction_config,
                ),
                name: None,
                tool_call_id: None,
                assistant_tool_calls: None,
            },
        ],
        temperature: None,
        max_tokens: Some(PROVIDER_CONTEXT_COMPACTION_SUMMARY_MAX_CHARS as u32 / 3),
        variant: None,
        reasoning_effort: None,
        text_verbosity: None,
        reasoning_summary: None,
        tools: None,
        tool_choice: None,
        stream: true,
    };

    let mut stream = provider.stream_completion(request).await;
    let mut output = String::new();
    while let Some(event) = stream.next().await {
        match event {
            ProviderStreamEvent::TextDelta(delta) => output.push_str(&delta),
            ProviderStreamEvent::Error { message } => return Err(message),
            ProviderStreamEvent::Done { .. } | ProviderStreamEvent::DoneWithMetadata { .. } => {
                break
            }
            ProviderStreamEvent::Start
            | ProviderStreamEvent::Started { .. }
            | ProviderStreamEvent::ReasoningDelta(_)
            | ProviderStreamEvent::ToolCallDelta { .. }
            | ProviderStreamEvent::ToolCallComplete { .. } => {}
        }
    }

    validate_model_compaction_summary(&output, tokens_before, &plan, compaction_config).map(
        |summary| ModelBackedCompactionSummary {
            summary,
            split_prefix_summary,
        },
    )
}

async fn model_backed_split_prefix_summary_decision(
    provider: Arc<dyn Provider>,
    model_ref: &str,
    plan: &ProviderContextCompactionPlan,
    trigger: &ProviderCompactionTrigger,
) -> Option<SplitPrefixSummaryDecision> {
    if plan.tail_boundary.mode != "split_oversized_turn_tail" {
        return None;
    }
    let deterministic_summary = plan.tail_boundary.split_prefix_summary.clone()?;
    let Some(prefix_turn) = plan.older_turns.last() else {
        return Some(SplitPrefixSummaryDecision::model_fallback(
            deterministic_summary,
            "split prefix turn was unavailable".to_string(),
        ));
    };

    match model_backed_split_prefix_summary_for(provider, model_ref, prefix_turn).await {
        Ok(summary) => Some(SplitPrefixSummaryDecision::model(summary)),
        Err(reason) => {
            tracing::warn!(
                %reason,
                agent_id = %trigger.agent_id,
                "model-backed split prefix summary fell back to deterministic summary"
            );
            Some(SplitPrefixSummaryDecision::model_fallback(
                deterministic_summary,
                reason,
            ))
        }
    }
}

async fn model_backed_split_prefix_summary_for(
    provider: Arc<dyn Provider>,
    model_ref: &str,
    prefix_turn: &ProviderConversationTurn,
) -> Result<String, String> {
    let model = AgentModelRef::parse(model_ref);
    let request = CompletionRequest {
        provider_id: Some(model.provider_id),
        model_id: model.model_id,
        messages: vec![
            CompletionMessage {
                role: MessageRole::System,
                content: "You summarize oversized Harness turn prefixes for context compaction. Return only the requested markdown summary.".to_string(),
                name: None,
                tool_call_id: None,
                assistant_tool_calls: None,
            },
            CompletionMessage {
                role: MessageRole::User,
                content: build_split_prefix_summary_prompt(prefix_turn),
                name: None,
                tool_call_id: None,
                assistant_tool_calls: None,
            },
        ],
        temperature: None,
        max_tokens: Some(PROVIDER_CONTEXT_SPLIT_PREFIX_SUMMARY_MAX_CHARS as u32 / 3),
        variant: None,
        reasoning_effort: None,
        text_verbosity: None,
        reasoning_summary: None,
        tools: None,
        tool_choice: None,
        stream: true,
    };

    let mut stream = provider.stream_completion(request).await;
    let mut output = String::new();
    while let Some(event) = stream.next().await {
        match event {
            ProviderStreamEvent::TextDelta(delta) => output.push_str(&delta),
            ProviderStreamEvent::Error { message } => return Err(message),
            ProviderStreamEvent::Done { .. } | ProviderStreamEvent::DoneWithMetadata { .. } => {
                break
            }
            ProviderStreamEvent::Start
            | ProviderStreamEvent::Started { .. }
            | ProviderStreamEvent::ReasoningDelta(_)
            | ProviderStreamEvent::ToolCallDelta { .. }
            | ProviderStreamEvent::ToolCallComplete { .. } => {}
        }
    }

    validate_model_split_prefix_summary(&output)
}

fn build_split_prefix_summary_prompt(prefix_turn: &ProviderConversationTurn) -> String {
    format!(
        "<conversation>\nUser: {user}\nAssistant prefix: {assistant}\n</conversation>\n\nThis is the PREFIX of a turn that was too large to keep. The SUFFIX (recent work) is retained.\n\nSummarize the prefix to provide context for the retained suffix:\n\n## Original Request\n[What did the user ask for in this turn?]\n\n## Early Progress\n- [Key decisions and work done in the prefix]\n\n## Context for Suffix\n- [Information needed to understand the retained recent work]\n\nBe concise. Focus on what's needed to understand the kept suffix.",
        user = prefix_turn.user_prompt,
        assistant = prefix_turn.assistant_response,
    )
}

fn validate_model_split_prefix_summary(summary: &str) -> Result<String, String> {
    let trimmed = summary.trim();
    if trimmed.is_empty() {
        return Err("model split prefix summary was empty".to_string());
    }
    if trimmed.chars().count() > PROVIDER_CONTEXT_SPLIT_PREFIX_SUMMARY_MAX_CHARS {
        return Err("model split prefix summary exceeded the character budget".to_string());
    }
    for heading in PROVIDER_CONTEXT_SPLIT_PREFIX_SUMMARY_HEADINGS {
        if !summary_contains_heading(trimmed, heading) {
            return Err(format!(
                "model split prefix summary missed required heading `{heading}`"
            ));
        }
    }
    Ok(trimmed.to_string())
}

pub(super) fn build_model_compaction_prompt(
    existing_summary: Option<&str>,
    plan: &ProviderContextCompactionPlan,
    deterministic_draft: &str,
    config: &CompactionRuntimeConfig,
) -> String {
    let compacted_facts = plan
        .facts
        .compacted_turns
        .iter()
        .enumerate()
        .map(|(index, fact)| {
            format!(
                "{}. user={} assistant={}",
                index + 1,
                fact.user_excerpt,
                fact.assistant_excerpt
            )
        })
        .collect::<Vec<_>>()
        .join("\n");
    let prior = existing_summary
        .and_then(non_empty_trimmed)
        .unwrap_or("(none)");
    let required_headings = provider_context_summary_required_headings(config).join(", ");
    let split_prefix_summary = plan
        .tail_boundary
        .split_prefix_summary
        .as_deref()
        .unwrap_or("none");
    let operational_memory = operational_memory_summary_block(&plan.facts);

    format!(
        "Update the Harness checkpoint summary for compacted provider context.\n\nRequired output rules:\n- Return markdown only.\n- Keep these headings exactly: {required_headings}.\n- Include `## Operational Memory` with `Read files:` and `Modified files:` subsections when operational memory is present.\n- Roll forward any still-relevant previous summary content into the structured sections. Do not append or label a raw previous-summary blob.\n- If split prefix summary is not `none`, preserve it under Critical Context and Source Facts wording.\n- Keep under {max_chars} characters.\n\nPrevious checkpoint summary:\n{prior}\n\nNew compacted turn facts:\n{compacted_facts}\n\nOperational memory facts:\n{operational_memory}\n\nTail boundary: {mode}; preserved turns: {preserved_turns}; note: {note}; split prefix summary: {split_prefix_summary}\n\nDeterministic Harness draft to improve:\n{deterministic_draft}",
        max_chars = PROVIDER_CONTEXT_COMPACTION_SUMMARY_MAX_CHARS,
        mode = plan.tail_boundary.mode,
        preserved_turns = plan.tail_boundary.preserved_turns,
        note = plan.tail_boundary.note.as_deref().unwrap_or("none"),
    )
}

pub(super) fn validate_model_compaction_summary(
    summary: &str,
    tokens_before: u32,
    plan: &ProviderContextCompactionPlan,
    config: &CompactionRuntimeConfig,
) -> Result<String, String> {
    let trimmed = summary.trim();
    if trimmed.is_empty() {
        return Err("model summary was empty".to_string());
    }
    if trimmed.chars().count() > PROVIDER_CONTEXT_COMPACTION_SUMMARY_MAX_CHARS {
        return Err("model summary exceeded the checkpoint summary character budget".to_string());
    }
    for heading in provider_context_summary_required_headings(config) {
        if !summary_contains_heading(trimmed, heading) {
            return Err(format!("model summary missed required heading `{heading}`"));
        }
    }
    if let Some(split_prefix_summary) = plan.tail_boundary.split_prefix_summary.as_deref() {
        if !trimmed.contains("Split prefix summary") {
            return Err(
                "model summary missed split prefix summary in Critical Context".to_string(),
            );
        }
        if !trimmed.contains("Source facts: split prefix summary") {
            return Err("model summary missed split prefix summary source facts".to_string());
        }
        let split_prefix_summary = split_prefix_summary.trim();
        let deterministic_excerpt = summarize_compaction_text(split_prefix_summary);
        if !trimmed.contains(split_prefix_summary) && !trimmed.contains(&deterministic_excerpt) {
            return Err("model summary missed split prefix summary content".to_string());
        }
    }
    let tokens_after = approximate_text_tokens(trimmed)
        .saturating_add(preserved_tokens_estimate(&plan.recent_turns));
    if tokens_after >= tokens_before {
        return Err("model summary would not reduce active provider context".to_string());
    }

    Ok(trimmed.to_string())
}

fn summary_contains_heading(summary: &str, heading: &str) -> bool {
    summary.lines().any(|line| line.trim() == heading)
}

pub(super) fn build_provider_context_summary(
    existing_summary: Option<&str>,
    older_turns: &[ProviderConversationTurn],
    pruned_tool_artifacts: &[EventArtifactRef],
    facts: &ProviderCompactionFacts,
    tail_boundary: &ProviderCompactionTailBoundary,
    summary_source: &ProviderCompactionSummarySource,
    config: &CompactionRuntimeConfig,
) -> String {
    if !config.structured_summary_contract {
        return build_legacy_provider_context_summary(
            existing_summary,
            older_turns,
            pruned_tool_artifacts,
            facts,
            tail_boundary,
            summary_source,
            config,
        );
    }

    build_harness_provider_context_summary(
        existing_summary,
        older_turns,
        pruned_tool_artifacts,
        facts,
        tail_boundary,
        summary_source,
        config,
    )
}

fn build_legacy_provider_context_summary(
    existing_summary: Option<&str>,
    older_turns: &[ProviderConversationTurn],
    pruned_tool_artifacts: &[EventArtifactRef],
    facts: &ProviderCompactionFacts,
    tail_boundary: &ProviderCompactionTailBoundary,
    summary_source: &ProviderCompactionSummarySource,
    config: &CompactionRuntimeConfig,
) -> String {
    let headings = provider_context_summary_required_headings(config);
    let mut lines = Vec::new();
    lines.push(headings[0].to_string());
    lines.push(format!(
        "- Continue the current agent session after compacting {} older turn(s).",
        older_turns.len()
    ));
    lines.push(String::new());

    lines.push(headings[1].to_string());
    if let Some(existing_summary) = existing_summary.and_then(non_empty_trimmed) {
        lines.push("- Preserve still-relevant constraints, decisions, files, and next steps from the previous checkpoint summary.".to_string());
        lines.push(format!(
            "- Prior checkpoint constraints/context carried forward: {}",
            summarize_compaction_text(existing_summary)
        ));
    } else {
        lines.push("- (none recorded explicitly)".to_string());
    }
    lines.push(String::new());

    lines.push(headings[2].to_string());
    lines.push(headings[3].to_string());
    for (index, turn) in older_turns.iter().enumerate() {
        lines.push(format!(
            "- Turn {} user: {}",
            index + 1,
            summarize_compaction_text(&turn.user_prompt)
        ));
        lines.push(format!(
            "  Assistant: {}",
            summarize_compaction_text(&turn.assistant_response)
        ));
    }
    lines.push(headings[4].to_string());
    lines.push(
        "- Continue from the preserved recent turn(s) that follow this checkpoint summary."
            .to_string(),
    );
    lines.push(headings[5].to_string());
    lines.push("- (none recorded explicitly)".to_string());
    lines.push(String::new());

    lines.push(headings[6].to_string());
    lines.push("- Older provider-visible turns were compacted into this checkpoint; preserved recent turns and the current user message take precedence over this lossy summary.".to_string());
    if let Some(split_prefix_summary) = tail_boundary.split_prefix_summary.as_deref() {
        lines.push(format!(
            "- Split prefix summary: {split_prefix_summary}; the provider-visible suffix follows this checkpoint as recent context."
        ));
    }
    if let Some(existing_summary) = existing_summary.and_then(non_empty_trimmed) {
        lines.push(format!(
            "- Prior checkpoint decisions/context were rolled into this structured summary: {}",
            summarize_compaction_text(existing_summary)
        ));
    }
    lines.push(String::new());

    lines.push(headings[7].to_string());
    lines.push("1. Use the preserved recent turn(s) plus this checkpoint summary to continue the user's current task.".to_string());
    lines.push(String::new());

    lines.push(headings[8].to_string());
    lines.push(format!("- Compacted turns: {}", older_turns.len()));
    if let Some(previous_checkpoint_id) = facts.previous_checkpoint_id.as_deref() {
        lines.push(format!(
            "- Previous checkpoint: {previous_checkpoint_id}; this summary rolls forward from it."
        ));
    }
    lines.push(format!(
        "- Tail boundary: {} ({} preserved turn(s), ~{} token(s)).",
        tail_boundary.mode, tail_boundary.preserved_turns, tail_boundary.preserved_tokens_estimate
    ));
    if let Some(note) = tail_boundary.note.as_deref() {
        lines.push(format!("- Tail note: {note}"));
    }
    lines.push(format!(
        "- Summary source: {} using {} (model-backed: {}, deterministic fallback: {}).",
        summary_source.strategy,
        summary_source.model_ref,
        summary_source.model_backed,
        summary_source.deterministic_fallback
    ));
    lines.push("- This summary is deterministic and lossy; verify details against artifacts or the event log when precision matters.".to_string());
    lines.push(String::new());

    lines.push(headings[9].to_string());
    if let Some(split_prefix_summary) = tail_boundary.split_prefix_summary.as_deref() {
        lines.push(format!(
            "- Source facts: split prefix summary: {split_prefix_summary}"
        ));
    }
    if facts.compacted_turns.is_empty() {
        lines.push("- (no compacted turn facts recorded)".to_string());
    } else {
        for fact in facts.compacted_turns.iter().take(8) {
            let request = fact
                .request_id
                .as_deref()
                .map(|request_id| format!(" `{request_id}`"))
                .unwrap_or_default();
            lines.push(format!("- Request{request}: {}", fact.user_excerpt));
            lines.push(format!("  Assistant: {}", fact.assistant_excerpt));
        }
    }
    if !facts.touched_files.is_empty() {
        lines.push("<read-files>".to_string());
        lines.extend(facts.touched_files.iter().take(12).cloned());
        lines.push("</read-files>".to_string());
    }
    lines.push(String::new());

    lines.push(headings[10].to_string());
    let mut artifact_lines = Vec::new();
    let mut seen = BTreeSet::new();
    for artifact in pruned_tool_artifacts {
        if seen.insert((artifact.path.clone(), artifact.digest.clone())) {
            let digest = artifact
                .digest
                .as_deref()
                .map(|value| format!(" ({value})"))
                .unwrap_or_default();
            artifact_lines.push(format!(
                "- {}{}: referenced by compacted turn/tool output",
                artifact.path, digest
            ));
        }
    }
    for turn in older_turns {
        for artifact in &turn.artifacts {
            if seen.insert((artifact.path.clone(), artifact.digest.clone())) {
                let digest = artifact
                    .digest
                    .as_deref()
                    .map(|value| format!(" ({value})"))
                    .unwrap_or_default();
                artifact_lines.push(format!(
                    "- {}{}: referenced by compacted provider turn",
                    artifact.path, digest
                ));
            }
        }
    }
    if artifact_lines.is_empty() {
        lines.push("- (none recorded)".to_string());
    } else {
        lines.extend(artifact_lines.into_iter().take(12));
    }

    truncate_with_ellipsis(
        &lines.join("\n"),
        PROVIDER_CONTEXT_COMPACTION_SUMMARY_MAX_CHARS,
    )
}

fn build_harness_provider_context_summary(
    existing_summary: Option<&str>,
    older_turns: &[ProviderConversationTurn],
    pruned_tool_artifacts: &[EventArtifactRef],
    facts: &ProviderCompactionFacts,
    tail_boundary: &ProviderCompactionTailBoundary,
    summary_source: &ProviderCompactionSummarySource,
    config: &CompactionRuntimeConfig,
) -> String {
    let headings = provider_context_summary_required_headings(config);
    let mut lines = Vec::new();
    lines.push(headings[0].to_string());
    lines.push(format!(
        "- Continue the current agent session after compacting {} older turn(s).",
        older_turns.len()
    ));
    lines.push(String::new());

    lines.push(headings[1].to_string());
    if let Some(existing_summary) = existing_summary.and_then(non_empty_trimmed) {
        lines.push("- Preserve still-relevant constraints, decisions, files, and next steps from the previous checkpoint summary.".to_string());
        lines.push(format!(
            "- Prior checkpoint constraints/context carried forward: {}",
            summarize_compaction_text(existing_summary)
        ));
    } else {
        lines.push("- (none recorded explicitly)".to_string());
    }
    lines.push(String::new());

    lines.push(headings[2].to_string());
    for (index, turn) in older_turns.iter().enumerate() {
        lines.push(format!(
            "- Done turn {} user: {}",
            index + 1,
            summarize_compaction_text(&turn.user_prompt)
        ));
        lines.push(format!(
            "  Assistant: {}",
            summarize_compaction_text(&turn.assistant_response)
        ));
    }
    lines.push("- In progress: continue from the preserved recent turn(s) that follow this checkpoint summary.".to_string());
    lines.push("- Blocked: (none recorded explicitly)".to_string());
    lines.push(String::new());

    lines.push(headings[3].to_string());
    lines.push("- Older provider-visible turns were compacted into this checkpoint; preserved recent turns and the current user message take precedence over this lossy summary.".to_string());
    if let Some(split_prefix_summary) = tail_boundary.split_prefix_summary.as_deref() {
        lines.push(format!(
            "- Split prefix summary: {split_prefix_summary}; the provider-visible suffix follows this checkpoint as recent context."
        ));
    }
    if let Some(existing_summary) = existing_summary.and_then(non_empty_trimmed) {
        lines.push(format!(
            "- Prior checkpoint decisions/context were rolled into this structured summary: {}",
            summarize_compaction_text(existing_summary)
        ));
    }
    lines.push(String::new());

    lines.push(headings[4].to_string());
    lines.push("1. Use the preserved recent turn(s) plus this checkpoint summary to continue the user's current task.".to_string());
    lines.push(String::new());

    lines.push(headings[5].to_string());
    lines.push(format!("- Compacted turns: {}", older_turns.len()));
    if let Some(previous_checkpoint_id) = facts.previous_checkpoint_id.as_deref() {
        lines.push(format!(
            "- Previous checkpoint: {previous_checkpoint_id}; this summary rolls forward from it."
        ));
    }
    lines.push(format!(
        "- Tail boundary: {} ({} preserved turn(s), ~{} token(s)).",
        tail_boundary.mode, tail_boundary.preserved_turns, tail_boundary.preserved_tokens_estimate
    ));
    if let Some(note) = tail_boundary.note.as_deref() {
        lines.push(format!("- Tail note: {note}"));
    }
    lines.push(format!(
        "- Summary source: {} using {} (model-backed: {}, deterministic fallback: {}).",
        summary_source.strategy,
        summary_source.model_ref,
        summary_source.model_backed,
        summary_source.deterministic_fallback
    ));
    if facts.compacted_turns.is_empty() {
        lines.push("- Source facts: (no compacted turn facts recorded)".to_string());
    } else {
        for fact in facts.compacted_turns.iter().take(8) {
            let request = fact
                .request_id
                .as_deref()
                .map(|request_id| format!(" `{request_id}`"))
                .unwrap_or_default();
            lines.push(format!(
                "- Source fact request{request}: {}",
                fact.user_excerpt
            ));
            lines.push(format!("  Assistant: {}", fact.assistant_excerpt));
        }
    }
    if let Some(split_prefix_summary) = tail_boundary.split_prefix_summary.as_deref() {
        lines.push(format!(
            "- Source facts: split prefix summary: {split_prefix_summary}"
        ));
    }
    let mut artifact_lines = Vec::new();
    let mut seen = BTreeSet::new();
    for artifact in pruned_tool_artifacts {
        if seen.insert((artifact.path.clone(), artifact.digest.clone())) {
            let digest = artifact
                .digest
                .as_deref()
                .map(|value| format!(" ({value})"))
                .unwrap_or_default();
            artifact_lines.push(format!(
                "- Artifact {}{}: referenced by compacted turn/tool output",
                artifact.path, digest
            ));
        }
    }
    for turn in older_turns {
        for artifact in &turn.artifacts {
            if seen.insert((artifact.path.clone(), artifact.digest.clone())) {
                let digest = artifact
                    .digest
                    .as_deref()
                    .map(|value| format!(" ({value})"))
                    .unwrap_or_default();
                artifact_lines.push(format!(
                    "- Artifact {}{}: referenced by compacted provider turn",
                    artifact.path, digest
                ));
            }
        }
    }
    if artifact_lines.is_empty() {
        lines.push("- Relevant files/artifacts: (none recorded)".to_string());
    } else {
        lines.extend(artifact_lines.into_iter().take(12));
    }
    lines.push("- This summary is deterministic and lossy; verify details against artifacts or the event log when precision matters.".to_string());
    append_operational_memory_section(&mut lines, facts);

    truncate_with_ellipsis(
        &lines.join("\n"),
        PROVIDER_CONTEXT_COMPACTION_SUMMARY_MAX_CHARS,
    )
}

fn operational_memory_summary_block(facts: &ProviderCompactionFacts) -> String {
    if facts.read_files.is_empty()
        && facts.modified_files.is_empty()
        && facts.operation_facts.is_empty()
    {
        return "(none recorded)".to_string();
    }

    let mut lines = Vec::new();
    lines.push("Read files:".to_string());
    if facts.read_files.is_empty() {
        lines.push("- (none recorded)".to_string());
    } else {
        lines.extend(
            facts
                .read_files
                .iter()
                .take(12)
                .map(file_operation_fact_line),
        );
    }
    lines.push("Modified files:".to_string());
    if facts.modified_files.is_empty() {
        lines.push("- (none recorded)".to_string());
    } else {
        lines.extend(
            facts
                .modified_files
                .iter()
                .take(12)
                .map(file_operation_fact_line),
        );
    }
    lines.extend(
        facts
            .operation_facts
            .iter()
            .take(20)
            .map(|fact| format!("- {fact}")),
    );
    lines.join("\n")
}

fn append_operational_memory_section(lines: &mut Vec<String>, facts: &ProviderCompactionFacts) {
    if facts.read_files.is_empty()
        && facts.modified_files.is_empty()
        && facts.operation_facts.is_empty()
    {
        return;
    }
    lines.push(String::new());
    lines.push("## Operational Memory".to_string());
    lines.push("Read files:".to_string());
    if facts.read_files.is_empty() {
        lines.push("- (none recorded)".to_string());
    } else {
        lines.extend(
            facts
                .read_files
                .iter()
                .take(12)
                .map(file_operation_fact_line),
        );
    }
    lines.push("Modified files:".to_string());
    if facts.modified_files.is_empty() {
        lines.push("- (none recorded)".to_string());
    } else {
        lines.extend(
            facts
                .modified_files
                .iter()
                .take(12)
                .map(file_operation_fact_line),
        );
    }
    lines.extend(
        facts
            .operation_facts
            .iter()
            .take(20)
            .map(|fact| format!("- {fact}")),
    );
}

fn file_operation_fact_line(fact: &ProviderFileOperationFact) -> String {
    let seq = match (fact.first_seq, fact.last_seq) {
        (Some(first), Some(last)) if first == last => format!(" seq {first}"),
        (Some(first), Some(last)) => format!(" seq {first}-{last}"),
        (Some(first), None) => format!(" seq {first}"),
        (None, Some(last)) => format!(" seq {last}"),
        (None, None) => String::new(),
    };
    let sources = if fact.sources.is_empty() {
        String::new()
    } else {
        format!(" via {}", fact.sources.join(", "))
    };
    let summary = fact
        .summary
        .as_deref()
        .filter(|summary| !summary.is_empty())
        .map(|summary| format!(": {summary}"))
        .unwrap_or_default();
    format!("- {}{}{}{}", fact.path, seq, sources, summary)
}

fn summarize_compaction_text(text: &str) -> String {
    let normalized = text.split_whitespace().collect::<Vec<_>>().join(" ");
    truncate_with_ellipsis(
        &normalized,
        PROVIDER_CONTEXT_COMPACTION_TURN_EXCERPT_MAX_CHARS,
    )
}

fn approximate_turn_tokens(turn: &ProviderConversationTurn) -> u32 {
    if !turn.messages.is_empty() {
        return turn
            .messages
            .iter()
            .map(approximate_conversation_message_tokens)
            .sum();
    }

    approximate_text_tokens(&turn.user_prompt)
        .saturating_add(approximate_text_tokens(&turn.assistant_response))
}

fn approximate_conversation_message_tokens(message: &ConversationMessage) -> u32 {
    match message {
        ConversationMessage::Checkpoint(checkpoint) => approximate_text_tokens(&checkpoint.summary),
        ConversationMessage::User(user) => approximate_text_tokens(&user.text),
        ConversationMessage::Assistant(assistant) => assistant.tool_calls.iter().fold(
            approximate_text_tokens(&assistant.text),
            |tokens, tool_call| {
                tokens
                    .saturating_add(approximate_text_tokens(&tool_call.tool_call_id))
                    .saturating_add(approximate_text_tokens(&tool_call.tool_id))
                    .saturating_add(approximate_text_tokens(&tool_call.args_summary))
            },
        ),
        ConversationMessage::ToolResult(tool_result) => {
            approximate_text_tokens(&tool_result.tool_call_id)
                .saturating_add(
                    tool_result
                        .tool_id
                        .as_deref()
                        .map(approximate_text_tokens)
                        .unwrap_or(0),
                )
                .saturating_add(
                    tool_result
                        .output_summary
                        .as_deref()
                        .map(approximate_text_tokens)
                        .unwrap_or(0),
                )
                .saturating_add(
                    tool_result
                        .output_json
                        .as_ref()
                        .map(Value::to_string)
                        .as_deref()
                        .map(approximate_text_tokens)
                        .unwrap_or(0),
                )
        }
    }
}

pub(super) fn approximate_text_tokens(text: &str) -> u32 {
    (text.chars().count() as u32 / 4).max(1)
}

pub(super) fn approximate_provider_context_tokens(context: &ProviderContext) -> u32 {
    let summary_tokens = context
        .compacted_summary
        .as_deref()
        .map(approximate_text_tokens)
        .unwrap_or(0);
    summary_tokens.saturating_add(
        context
            .preserved_turns
            .iter()
            .map(approximate_turn_tokens)
            .sum::<u32>(),
    )
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
        &run_state.info.run_id,
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

fn collect_historical_agent_turns_until(
    run_id: &str,
    events_path: &Path,
    agent_id: &str,
    lower_bound_seq: u64,
    through_seq: u64,
) -> Result<Vec<HistoricalCompletedAgentTurn>, CoordinatorError> {
    let file =
        fs::File::open(events_path).map_err(|source| CoordinatorError::ResumeRestoreFailed {
            run_id: run_id.to_string(),
            reason: format!(
                "failed to open historical events {}: {source}",
                events_path.display()
            ),
        })?;

    let mut expected_seq = 1_u64;
    let mut requests: BTreeMap<String, HistoricalRequestState> = BTreeMap::new();
    let mut request_turn_task_ids: BTreeMap<String, String> = BTreeMap::new();
    let mut historical_task_scopes: BTreeMap<String, TaskTerminalScope> = BTreeMap::new();
    let mut request_artifacts: BTreeMap<String, Vec<EventArtifactRef>> = BTreeMap::new();
    let mut turns = Vec::new();

    for (line_number, line) in BufReader::new(file).lines().enumerate() {
        let Some(event) = parse_historical_event_line(run_id, events_path, line_number, line)?
        else {
            continue;
        };
        validate_historical_event_seq(run_id, events_path, &event, expected_seq)?;
        expected_seq = expected_seq.saturating_add(1);

        if event.seq > through_seq {
            break;
        }
        if event.seq <= lower_bound_seq {
            continue;
        }

        match &event.payload {
            EventV1::UserMessageSubmitted(payload) => {
                let request = requests.entry(payload.request_id.clone()).or_default();
                request.first_seq.get_or_insert(event.seq);
                request.user_text = Some(payload.text.clone());
            }
            EventV1::ProviderRequestStarted(payload)
                if event.actor.agent_id.as_deref() == Some(agent_id) =>
            {
                let request = requests.entry(payload.request_id.clone()).or_default();
                request.first_seq.get_or_insert(event.seq);
                request.prompt_summary = Some(payload.prompt_summary.clone());
                request.agent_id = Some(agent_id.to_string());
            }
            EventV1::ProviderStreamDelta(payload)
                if event.actor.agent_id.as_deref() == Some(agent_id) =>
            {
                requests
                    .entry(payload.request_id.clone())
                    .or_default()
                    .assistant_output
                    .push_str(&payload.delta);
            }
            EventV1::TaskScheduled(payload)
                if event.actor.agent_id.as_deref() == Some(agent_id) =>
            {
                let Some(queue_key) = payload.queue_key.as_deref() else {
                    continue;
                };

                let scope = if queue_key.starts_with("provider_model:") {
                    Some(TaskTerminalScope::AgentTurn)
                } else if queue_key.starts_with("tool:") {
                    Some(TaskTerminalScope::ToolCall)
                } else {
                    None
                };

                if let Some(scope) = scope {
                    historical_task_scopes.insert(payload.task_id.clone(), scope);
                    if matches!(scope, TaskTerminalScope::AgentTurn) {
                        if let Some(request_id) = event.correlation_id.as_deref() {
                            request_turn_task_ids
                                .insert(request_id.to_string(), payload.task_id.clone());
                        }
                    }
                }
            }
            EventV1::ArtifactWritten(payload) => {
                let Some(request_id) = event.correlation_id.as_deref() else {
                    continue;
                };
                request_artifacts
                    .entry(request_id.to_string())
                    .or_default()
                    .push(EventArtifactRef {
                        path: payload.path.clone(),
                        digest: Some(payload.digest.clone()),
                    });
            }
            EventV1::TaskCompleted(payload)
                if event.actor.agent_id.as_deref() == Some(agent_id) =>
            {
                let Some(request_id) = event.correlation_id.as_deref() else {
                    continue;
                };

                if !historical_task_completion_marks_agent_turn(
                    request_id,
                    payload,
                    &historical_task_scopes,
                    &request_turn_task_ids,
                ) {
                    continue;
                }

                let request_state = requests.remove(request_id).ok_or_else(|| {
                    CoordinatorError::ResumeRestoreFailed {
                        run_id: run_id.to_string(),
                        reason: format!(
                            "missing provider request history for completed request `{request_id}`"
                        ),
                    }
                })?;

                let user_prompt = restore_historical_user_prompt(
                    run_id,
                    request_id,
                    request_state.user_text.clone(),
                    request_state.prompt_summary.clone(),
                )?;
                let assistant_response = if payload.result_summary.is_empty() {
                    request_state.assistant_output.clone()
                } else {
                    payload.result_summary.clone()
                };
                let mut artifact_refs = request_artifacts.remove(request_id).unwrap_or_default();
                artifact_refs.sort_by(|left, right| {
                    left.path
                        .cmp(&right.path)
                        .then_with(|| left.digest.cmp(&right.digest))
                });
                artifact_refs
                    .dedup_by(|left, right| left.path == right.path && left.digest == right.digest);
                turns.push(HistoricalCompletedAgentTurn {
                    request_id: request_id.to_string(),
                    user_prompt,
                    assistant_response,
                    artifact_refs,
                });
            }
            _ => {}
        }
    }

    Ok(turns)
}
#[derive(Default)]
struct HistoricalRequestState {
    user_text: Option<String>,
    prompt_summary: Option<String>,
    assistant_output: String,
    messages: Vec<ConversationMessage>,
    active_assistant_message_index: Option<usize>,
    tool_ids_by_call_id: BTreeMap<String, String>,
    agent_id: Option<String>,
    provider_request_id: Option<String>,
    provider_finish_reason: Option<String>,
    first_seq: Option<u64>,
}

#[derive(Debug, Clone)]
struct AppliedCheckpointRecord {
    checkpoint_id: String,
    artifact_path: String,
    through_seq: u64,
    through_request_id: Option<String>,
}

#[derive(Debug, Clone)]
struct HistoricalCompletedAgentTurn {
    request_id: String,
    user_prompt: String,
    assistant_response: String,
    artifact_refs: Vec<EventArtifactRef>,
}

fn historical_conversation_messages_for_completed_turn(
    user_prompt: &str,
    assistant_response: &str,
    request_state: &HistoricalRequestState,
) -> Vec<ConversationMessage> {
    if request_state.messages.is_empty() {
        return Vec::new();
    }

    let request_id = request_state
        .messages
        .iter()
        .find_map(|message| match message {
            ConversationMessage::Assistant(assistant) => Some(assistant.request_id.clone()),
            ConversationMessage::ToolResult(tool_result) => Some(tool_result.request_id.clone()),
            ConversationMessage::User(user) => Some(user.request_id.clone()),
            ConversationMessage::Checkpoint(_) => None,
        })
        .unwrap_or_default();
    let agent_id = request_state.agent_id.clone();

    let mut messages = Vec::with_capacity(request_state.messages.len() + 1);
    messages.push(ConversationMessage::User(ConversationUserMessage {
        request_id,
        text: user_prompt.to_string(),
        seq: request_state.first_seq,
        agent_id,
    }));
    messages.extend(request_state.messages.clone());
    if let Some(ConversationMessage::Assistant(assistant)) = messages.last_mut() {
        if assistant.tool_calls.is_empty() && assistant.text != assistant_response {
            assistant.text = assistant_response.to_string();
        }
    }
    messages
}

fn restore_historical_user_prompt(
    run_id: &str,
    request_id: &str,
    user_text: Option<String>,
    prompt_summary: Option<String>,
) -> Result<String, CoordinatorError> {
    if let Some(user_text) = user_text {
        return Ok(user_text);
    }

    let Some(prompt_summary) = prompt_summary.as_deref().and_then(non_empty_trimmed) else {
        return Err(CoordinatorError::ResumeRestoreFailed {
            run_id: run_id.to_string(),
            reason: format!("missing user message for completed request `{request_id}`"),
        });
    };

    if prompt_summary.ends_with('…') {
        return Err(CoordinatorError::ResumeRestoreFailed {
            run_id: run_id.to_string(),
            reason: format!(
                "missing user message for completed request `{request_id}` and prompt_summary is truncated"
            ),
        });
    }

    Ok(prompt_summary.to_string())
}

pub(super) fn restore_provider_context_from_history(
    session_dir: &Path,
    run_id: &str,
) -> Result<BTreeMap<String, ProviderContext>, CoordinatorError> {
    let run_dir = session_dir.join(run_id);
    let events_path = run_dir.join(EVENTS_FILE_NAME);
    let historical_events = read_historical_events_until(run_id, &events_path, u64::MAX)?;

    let applied_checkpoints = discover_applied_checkpoints(run_id, &run_dir, &historical_events)?;
    let checkpoint_boundaries = applied_checkpoints
        .iter()
        .map(|(agent_id, checkpoint)| (agent_id.clone(), checkpoint.through_seq))
        .collect::<BTreeMap<_, _>>();

    let mut histories = BTreeMap::new();
    for (agent_id, checkpoint) in &applied_checkpoints {
        let checkpoint_artifact = load_provider_context_checkpoint(run_id, &run_dir, checkpoint)?;
        if checkpoint_artifact.metadata.run_id != run_id {
            return Err(CoordinatorError::ResumeRestoreFailed {
                run_id: run_id.to_string(),
                reason: format!(
                    "checkpoint `{}` run mismatch: expected `{run_id}`, got `{}`",
                    checkpoint.checkpoint_id, checkpoint_artifact.metadata.run_id
                ),
            });
        }
        if checkpoint_artifact.metadata.checkpoint_id != checkpoint.checkpoint_id {
            return Err(CoordinatorError::ResumeRestoreFailed {
                run_id: run_id.to_string(),
                reason: format!(
                    "checkpoint artifact id mismatch for agent `{agent_id}`: expected `{}`, got `{}`",
                    checkpoint.checkpoint_id, checkpoint_artifact.metadata.checkpoint_id
                ),
            });
        }
        if checkpoint_artifact.metadata.agent_id != *agent_id {
            return Err(CoordinatorError::ResumeRestoreFailed {
                run_id: run_id.to_string(),
                reason: format!(
                    "checkpoint `{}` agent mismatch: expected `{agent_id}`, got `{}`",
                    checkpoint.checkpoint_id, checkpoint_artifact.metadata.agent_id
                ),
            });
        }
        if checkpoint_artifact.metadata.through_seq != checkpoint.through_seq {
            return Err(CoordinatorError::ResumeRestoreFailed {
                run_id: run_id.to_string(),
                reason: format!(
                    "checkpoint `{}` through_seq mismatch: expected `{}`, got `{}`",
                    checkpoint.checkpoint_id,
                    checkpoint.through_seq,
                    checkpoint_artifact.metadata.through_seq
                ),
            });
        }
        if checkpoint_artifact.metadata.through_request_id != checkpoint.through_request_id {
            return Err(CoordinatorError::ResumeRestoreFailed {
                run_id: run_id.to_string(),
                reason: format!(
                    "checkpoint `{}` through_request_id mismatch: expected `{:?}`, got `{:?}`",
                    checkpoint.checkpoint_id,
                    checkpoint.through_request_id,
                    checkpoint_artifact.metadata.through_request_id
                ),
            });
        }
        histories.insert(
            agent_id.clone(),
            ProviderContext::from_checkpoint(checkpoint_artifact),
        );
    }

    let mut requests: BTreeMap<String, HistoricalRequestState> = BTreeMap::new();
    let mut request_turn_task_ids: BTreeMap<String, String> = BTreeMap::new();
    let mut historical_task_scopes: BTreeMap<String, TaskTerminalScope> = BTreeMap::new();
    let mut request_artifacts: BTreeMap<String, Vec<EventArtifactRef>> = BTreeMap::new();
    let mut agent_turn_agent_by_task: BTreeMap<String, String> = BTreeMap::new();

    for event in &historical_events {
        let replay_agent_event = should_replay_agent_scoped_event(
            event.seq,
            event.actor.agent_id.as_deref(),
            &checkpoint_boundaries,
        );

        match &event.payload {
            EventV1::UserMessageSubmitted(payload) => {
                let request = requests.entry(payload.request_id.clone()).or_default();
                request.first_seq.get_or_insert(event.seq);
                request.user_text = Some(payload.text.clone());
            }
            EventV1::ProviderRequestStarted(payload) => {
                if !replay_agent_event {
                    continue;
                }
                let request_id = event
                    .correlation_id
                    .as_deref()
                    .and_then(non_empty_trimmed)
                    .unwrap_or(&payload.request_id);
                let request = requests.entry(request_id.to_string()).or_default();
                request.first_seq.get_or_insert(event.seq);
                request.prompt_summary = Some(payload.prompt_summary.clone());
                request.provider_request_id = Some(payload.request_id.clone());
                request.messages.push(ConversationMessage::Assistant(
                    ConversationAssistantMessage {
                        request_id: request_id.to_string(),
                        agent_id: event.actor.agent_id.clone(),
                        text: String::new(),
                        tool_calls: Vec::new(),
                        stop_reason: None,
                        first_seq: Some(event.seq),
                        last_seq: Some(event.seq),
                        provider_id: Some(payload.provider_id.clone()),
                        model_id: Some(payload.model_id.clone()),
                        output_digest: None,
                    },
                ));
                request.active_assistant_message_index =
                    Some(request.messages.len().saturating_sub(1));
                if let Some(agent_id) = event.actor.agent_id.as_deref().and_then(non_empty_trimmed)
                {
                    request.agent_id = Some(agent_id.to_string());
                }
            }
            EventV1::ProviderStreamDelta(payload) => {
                if !replay_agent_event {
                    continue;
                }
                let request_id = event
                    .correlation_id
                    .as_deref()
                    .and_then(non_empty_trimmed)
                    .unwrap_or(&payload.request_id);
                let request = requests.entry(request_id.to_string()).or_default();
                request.assistant_output.push_str(&payload.delta);
                if let Some(index) = request.active_assistant_message_index {
                    if let Some(ConversationMessage::Assistant(assistant)) =
                        request.messages.get_mut(index)
                    {
                        assistant.text.push_str(&payload.delta);
                        assistant.last_seq = Some(event.seq);
                    }
                }
            }
            EventV1::ProviderRequestFinished(payload) => {
                if !replay_agent_event {
                    continue;
                }
                let request_id = event
                    .correlation_id
                    .as_deref()
                    .and_then(non_empty_trimmed)
                    .unwrap_or(&payload.request_id);
                let request = requests.entry(request_id.to_string()).or_default();
                request.first_seq.get_or_insert(event.seq);
                request.provider_request_id = Some(payload.request_id.clone());
                request.provider_finish_reason = Some(payload.finish_reason.clone());
                if let Some(index) = request.active_assistant_message_index {
                    if let Some(ConversationMessage::Assistant(assistant)) =
                        request.messages.get_mut(index)
                    {
                        assistant.stop_reason = Some(payload.finish_reason.clone());
                        assistant.output_digest = payload.output_digest.clone();
                        assistant.last_seq = Some(event.seq);
                    }
                }
            }
            EventV1::TaskScheduled(payload) => {
                if !replay_agent_event {
                    continue;
                }
                let Some(queue_key) = payload.queue_key.as_deref() else {
                    continue;
                };

                let scope = if queue_key.starts_with("provider_model:") {
                    Some(TaskTerminalScope::AgentTurn)
                } else if queue_key.starts_with("tool:") {
                    Some(TaskTerminalScope::ToolCall)
                } else {
                    None
                };

                if let Some(scope) = scope {
                    historical_task_scopes.insert(payload.task_id.clone(), scope);
                    if matches!(scope, TaskTerminalScope::AgentTurn) {
                        if let Some(request_id) = event.correlation_id.as_deref() {
                            requests
                                .entry(request_id.to_string())
                                .or_default()
                                .first_seq
                                .get_or_insert(event.seq);
                            request_turn_task_ids
                                .insert(request_id.to_string(), payload.task_id.clone());
                            if let Some(agent_id) = event.actor.agent_id.as_deref() {
                                agent_turn_agent_by_task
                                    .insert(payload.task_id.clone(), agent_id.to_string());
                            }
                        }
                    }
                }
            }
            EventV1::ArtifactWritten(payload) => {
                let Some(request_id) = event.correlation_id.as_deref() else {
                    continue;
                };
                request_artifacts
                    .entry(request_id.to_string())
                    .or_default()
                    .push(EventArtifactRef {
                        path: payload.path.clone(),
                        digest: Some(payload.digest.clone()),
                    });
            }
            EventV1::ToolCallRequested(payload) => {
                if !replay_agent_event {
                    continue;
                }
                let Some(request_id) = event.correlation_id.as_deref() else {
                    continue;
                };
                let request = requests.entry(request_id.to_string()).or_default();
                request
                    .tool_ids_by_call_id
                    .insert(payload.tool_call_id.clone(), payload.tool_id.clone());
                if let Some(index) = request.active_assistant_message_index {
                    if let Some(ConversationMessage::Assistant(assistant)) =
                        request.messages.get_mut(index)
                    {
                        assistant.tool_calls.push(ConversationToolCall {
                            tool_call_id: payload.tool_call_id.clone(),
                            tool_id: payload.tool_id.clone(),
                            args_summary: provider_tool_arguments_json(&payload.args_summary),
                            args_digest: payload.args_digest.clone(),
                            seq: Some(event.seq),
                            metadata: payload.metadata.clone(),
                        });
                        assistant.last_seq = Some(event.seq);
                    }
                }
            }
            EventV1::ToolCallFinished(payload) => {
                if !replay_agent_event {
                    continue;
                }
                let Some(request_id) = event.correlation_id.as_deref() else {
                    continue;
                };
                let Some(request) = requests.get_mut(request_id) else {
                    continue;
                };
                let Some(tool_id) = request
                    .tool_ids_by_call_id
                    .get(&payload.tool_call_id)
                    .cloned()
                else {
                    continue;
                };
                request
                    .messages
                    .push(ConversationMessage::ToolResult(Box::new(
                        ConversationToolResultMessage {
                            request_id: request_id.to_string(),
                            tool_call_id: payload.tool_call_id.clone(),
                            tool_id: Some(tool_id),
                            status: payload.status,
                            output_summary: payload.output_summary.clone(),
                            output_digest: payload.output_digest.clone(),
                            output_json: payload.output_json.clone(),
                            seq: Some(event.seq),
                            metadata: payload.metadata.clone(),
                        },
                    )));
            }
            EventV1::TaskCompleted(payload) => {
                if matches!(
                    historical_task_scopes.get(&payload.task_id),
                    Some(TaskTerminalScope::AgentTurn)
                ) {
                    agent_turn_agent_by_task.remove(&payload.task_id);
                }
                if !replay_agent_event {
                    continue;
                }
                let Some(request_id) = event.correlation_id.as_deref() else {
                    continue;
                };

                if !historical_task_completion_marks_agent_turn(
                    request_id,
                    payload,
                    &historical_task_scopes,
                    &request_turn_task_ids,
                ) {
                    continue;
                }

                let Some(agent_id) = event
                    .actor
                    .agent_id
                    .as_deref()
                    .and_then(non_empty_trimmed)
                    .map(str::to_string)
                else {
                    return Err(CoordinatorError::ResumeRestoreFailed {
                        run_id: run_id.to_string(),
                        reason: format!(
                            "task completion for request `{request_id}` missing agent actor"
                        ),
                    });
                };
                let request_state = requests.remove(request_id).ok_or_else(|| {
                    CoordinatorError::ResumeRestoreFailed {
                        run_id: run_id.to_string(),
                        reason: format!(
                            "missing provider request history for completed request `{request_id}`"
                        ),
                    }
                })?;

                let user_prompt = restore_historical_user_prompt(
                    run_id,
                    request_id,
                    request_state.user_text.clone(),
                    request_state.prompt_summary.clone(),
                )?;

                let assistant_response = if payload.result_summary.is_empty() {
                    request_state.assistant_output.clone()
                } else {
                    payload.result_summary.clone()
                };
                let messages = historical_conversation_messages_for_completed_turn(
                    &user_prompt,
                    &assistant_response,
                    &request_state,
                );
                let mut artifacts = request_artifacts.remove(request_id).unwrap_or_default();
                artifacts.sort_by(|left, right| {
                    left.path
                        .cmp(&right.path)
                        .then_with(|| left.digest.cmp(&right.digest))
                });
                artifacts
                    .dedup_by(|left, right| left.path == right.path && left.digest == right.digest);

                histories
                    .entry(request_state.agent_id.unwrap_or(agent_id))
                    .or_default()
                    .push_turn(ProviderConversationTurn {
                        user_prompt,
                        assistant_response,
                        request_id: Some(request_id.to_string()),
                        first_seq: request_state.first_seq,
                        last_seq: Some(event.seq),
                        artifacts,
                        messages,
                        ..ProviderConversationTurn::default()
                    });
            }
            EventV1::TaskCancelled(payload) => {
                let agent_id_from_task = if matches!(
                    historical_task_scopes.get(&payload.task_id),
                    Some(TaskTerminalScope::AgentTurn)
                ) {
                    agent_turn_agent_by_task.remove(&payload.task_id)
                } else {
                    None
                };
                if !replay_agent_event {
                    continue;
                }
                let Some(request_id) = event.correlation_id.as_deref() else {
                    continue;
                };

                if !historical_task_cancellation_marks_agent_turn(
                    request_id,
                    payload,
                    &historical_task_scopes,
                    &request_turn_task_ids,
                ) {
                    continue;
                }

                let Some(request_state) = requests.remove(request_id) else {
                    continue;
                };

                let agent_id = event
                    .actor
                    .agent_id
                    .as_deref()
                    .and_then(non_empty_trimmed)
                    .map(str::to_string)
                    .or(agent_id_from_task)
                    .or_else(|| request_state.agent_id.clone())
                    .ok_or_else(|| CoordinatorError::ResumeRestoreFailed {
                        run_id: run_id.to_string(),
                        reason: format!(
                            "task cancellation for request `{request_id}` missing agent actor"
                        ),
                    })?;
                let user_prompt = restore_historical_user_prompt(
                    run_id,
                    request_id,
                    request_state.user_text.clone(),
                    request_state.prompt_summary.clone(),
                )?;

                let (status, failure_stage) = historical_cancelled_turn_status_stage(
                    request_state.provider_finish_reason.as_deref(),
                    &payload.reason,
                );
                let messages = if failure_stage == "max_iters" {
                    historical_conversation_messages_for_completed_turn(
                        &user_prompt,
                        &request_state.assistant_output,
                        &request_state,
                    )
                } else {
                    Vec::new()
                };
                let provider_request_id = request_state
                    .provider_request_id
                    .unwrap_or_else(|| request_id.to_string());
                histories
                    .entry(agent_id)
                    .or_default()
                    .push_turn(ProviderConversationTurn {
                        user_prompt,
                        assistant_response: request_state.assistant_output,
                        status,
                        failure_stage: Some(failure_stage),
                        failure_reason: truncated_failure_reason(&payload.reason),
                        request_id: Some(provider_request_id),
                        first_seq: request_state.first_seq,
                        last_seq: Some(event.seq),
                        artifacts: Vec::new(),
                        messages,
                    });
            }
            _ => {}
        }
    }

    Ok(histories)
}

fn should_replay_agent_scoped_event(
    seq: u64,
    agent_id: Option<&str>,
    checkpoint_boundaries: &BTreeMap<String, u64>,
) -> bool {
    let Some(agent_id) = agent_id else {
        return true;
    };

    seq > checkpoint_boundaries.get(agent_id).copied().unwrap_or(0)
}

fn discover_applied_checkpoints(
    run_id: &str,
    run_dir: &Path,
    events: &[EventEnvelopeV1],
) -> Result<BTreeMap<String, AppliedCheckpointRecord>, CoordinatorError> {
    let mut written_by_id = BTreeMap::new();
    let mut latest_applied_by_agent: BTreeMap<String, (u64, String)> = BTreeMap::new();

    for event in events {
        match &event.payload {
            EventV1::CompactionWritten(payload) => {
                written_by_id.insert(payload.checkpoint_id.clone(), payload.clone());
            }
            EventV1::CompactionApplied(payload) => {
                latest_applied_by_agent.insert(
                    payload.agent_id.clone(),
                    (event.seq, payload.checkpoint_id.clone()),
                );
            }
            _ => {}
        }
    }

    let mut applied = BTreeMap::new();
    for (agent_id, (_, checkpoint_id)) in latest_applied_by_agent {
        let Some(written) = written_by_id.get(&checkpoint_id) else {
            return Err(CoordinatorError::ResumeRestoreFailed {
                run_id: run_id.to_string(),
                reason: format!(
                    "compaction checkpoint `{checkpoint_id}` was applied without a matching written event"
                ),
            });
        };

        if written.agent_id != agent_id {
            return Err(CoordinatorError::ResumeRestoreFailed {
                run_id: run_id.to_string(),
                reason: format!(
                    "compaction checkpoint `{checkpoint_id}` agent mismatch between applied `{agent_id}` and written `{}`",
                    written.agent_id
                ),
            });
        }

        applied.insert(
            agent_id,
            AppliedCheckpointRecord {
                checkpoint_id: checkpoint_id.clone(),
                artifact_path: written.artifact_path.clone(),
                through_seq: written.through_seq,
                through_request_id: written.through_request_id.clone(),
            },
        );
    }

    let _ = run_dir;
    Ok(applied)
}

fn load_provider_context_checkpoint(
    run_id: &str,
    run_dir: &Path,
    checkpoint: &AppliedCheckpointRecord,
) -> Result<ProviderContextCheckpoint, CoordinatorError> {
    let checkpoint_path = run_dir.join(&checkpoint.artifact_path);
    let body = fs::read_to_string(&checkpoint_path).map_err(|source| {
        CoordinatorError::ResumeRestoreFailed {
            run_id: run_id.to_string(),
            reason: format!(
                "failed to read checkpoint artifact {}: {source}",
                checkpoint_path.display()
            ),
        }
    })?;

    serde_json::from_str(&body).map_err(|source| CoordinatorError::ResumeRestoreFailed {
        run_id: run_id.to_string(),
        reason: format!(
            "invalid checkpoint artifact {}: {source}",
            checkpoint_path.display()
        ),
    })
}

fn historical_task_completion_marks_agent_turn(
    request_id: &str,
    payload: &TaskCompletedEvent,
    historical_task_scopes: &BTreeMap<String, TaskTerminalScope>,
    request_turn_task_ids: &BTreeMap<String, String>,
) -> bool {
    if let Some(scope) = payload
        .metadata
        .as_ref()
        .and_then(|metadata| metadata.task_scope)
    {
        return matches!(scope, TaskTerminalScope::AgentTurn);
    }

    if let Some(scope) = historical_task_scopes.get(&payload.task_id) {
        return matches!(scope, TaskTerminalScope::AgentTurn);
    }

    if let Some(turn_task_id) = request_turn_task_ids.get(request_id) {
        return turn_task_id == &payload.task_id;
    }

    true
}

fn historical_task_cancellation_marks_agent_turn(
    request_id: &str,
    payload: &TaskCancelledEvent,
    historical_task_scopes: &BTreeMap<String, TaskTerminalScope>,
    request_turn_task_ids: &BTreeMap<String, String>,
) -> bool {
    if let Some(scope) = payload.task_scope {
        return matches!(scope, TaskTerminalScope::AgentTurn);
    }

    if let Some(scope) = historical_task_scopes.get(&payload.task_id) {
        return matches!(scope, TaskTerminalScope::AgentTurn);
    }

    if let Some(turn_task_id) = request_turn_task_ids.get(request_id) {
        return turn_task_id == &payload.task_id;
    }

    false
}

fn historical_cancelled_turn_status_stage(
    provider_finish_reason: Option<&str>,
    cancellation_reason: &str,
) -> (ProviderConversationTurnStatus, String) {
    if provider_finish_reason == Some("error") {
        return (
            ProviderConversationTurnStatus::Failed,
            "provider_error".to_string(),
        );
    }

    if cancellation_reason.contains("overflow persisted after checkpoint compaction") {
        return (
            ProviderConversationTurnStatus::Failed,
            "overflow_retry_failed".to_string(),
        );
    }

    if cancellation_reason.contains("failed closed") {
        return (
            ProviderConversationTurnStatus::Failed,
            "tool_failure".to_string(),
        );
    }

    if cancellation_reason.contains("critical lifecycle hook failed")
        || cancellation_reason.contains("lifecycle hook failed")
    {
        return (
            ProviderConversationTurnStatus::Failed,
            "hook_failure".to_string(),
        );
    }

    if cancellation_reason.contains("agent turn exceeded profile max_iters=") {
        return (
            ProviderConversationTurnStatus::Aborted,
            "max_iters".to_string(),
        );
    }

    (
        ProviderConversationTurnStatus::Aborted,
        "cancelled".to_string(),
    )
}
