use std::sync::Arc;

use harness_providers::{
    CompletionMessage, CompletionRequest, MessageRole, Provider, ProviderStreamEvent,
};
use tokio_stream::StreamExt;

use crate::agent::{AgentModelRef, ProviderConversationTurn};
use crate::config::CompactionRuntimeConfig;
use crate::redact::Redactor;
use crate::text::non_empty_trimmed;

use super::super::RunState;
use super::planning::{
    build_deterministic_provider_compaction_summary_source, build_provider_context_compaction_plan,
    provider_context_keep_recent_tokens, recorded_runtime_context_for_compaction,
    should_compact_provider_context, ProviderCompactionTrigger, ProviderContextCompactionPlan,
    SplitPrefixSummaryDecision,
};
use super::summary::{
    build_provider_context_summary, operational_memory_summary_block,
    provider_context_summary_required_headings,
};
use super::tokens::{
    approximate_provider_context_tokens, approximate_text_tokens, preserved_tokens_estimate,
    summarize_compaction_text,
};
use super::{
    PROVIDER_CONTEXT_COMPACTION_SUMMARY_MAX_CHARS, PROVIDER_CONTEXT_SPLIT_PREFIX_SUMMARY_HEADINGS,
    PROVIDER_CONTEXT_SPLIT_PREFIX_SUMMARY_MAX_CHARS,
};

#[derive(Debug, Clone)]
pub(in crate::coord) struct ModelBackedCompactionSummary {
    pub(in crate::coord) summary: String,
    pub(in crate::coord) split_prefix_summary: Option<SplitPrefixSummaryDecision>,
}

pub(in crate::coord) fn compaction_summary_model_ref(
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

pub(in crate::coord) async fn model_backed_compaction_summary_for(
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
    let draft_source = build_deterministic_provider_compaction_summary_source(
        &metadata,
        trigger,
        context.compacted_summary.as_deref(),
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
        context: Default::default(),
        stream: true,
    };

    let mut stream = provider.stream_completion(request).await;
    let mut output = String::new();
    while let Some(event) = stream.next().await {
        match event {
            ProviderStreamEvent::TextDelta(delta) => output.push_str(&delta),
            ProviderStreamEvent::Error { message, .. } => return Err(message),
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
        context: Default::default(),
        stream: true,
    };

    let mut stream = provider.stream_completion(request).await;
    let mut output = String::new();
    while let Some(event) = stream.next().await {
        match event {
            ProviderStreamEvent::TextDelta(delta) => output.push_str(&delta),
            ProviderStreamEvent::Error { message, .. } => return Err(message),
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

pub(in crate::coord) fn build_model_compaction_prompt(
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

pub(in crate::coord) fn validate_model_compaction_summary(
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
