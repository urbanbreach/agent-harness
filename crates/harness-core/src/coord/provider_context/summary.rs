// allow: SIZE_OK — coordinator state machine (turn lifecycle + scheduling)
use std::collections::BTreeSet;

use crate::agent::{
    ProviderCompactionFacts, ProviderCompactionSummarySource, ProviderCompactionTailBoundary,
    ProviderConversationTurn, ProviderFileOperationFact,
};
use crate::config::CompactionRuntimeConfig;
use crate::event::EventArtifactRef;
use crate::text::{non_empty_trimmed, truncate_with_ellipsis};

use super::tokens::summarize_compaction_text;
use super::{
    PROVIDER_CONTEXT_COMPACTION_SUMMARY_MAX_CHARS, PROVIDER_CONTEXT_HARNESS_SUMMARY_HEADINGS,
    PROVIDER_CONTEXT_LEGACY_SUMMARY_HEADINGS,
};

pub(in crate::coord) fn provider_context_summary_required_headings(
    config: &CompactionRuntimeConfig,
) -> &'static [&'static str] {
    if config.structured_summary_contract {
        PROVIDER_CONTEXT_HARNESS_SUMMARY_HEADINGS
    } else {
        PROVIDER_CONTEXT_LEGACY_SUMMARY_HEADINGS
    }
}

pub(in crate::coord) fn build_provider_context_summary(
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
                .as_ref().map(|r| r.as_str())
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
                .as_ref().map(|r| r.as_str())
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

pub(super) fn operational_memory_summary_block(facts: &ProviderCompactionFacts) -> String {
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
