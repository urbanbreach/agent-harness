// allow: SIZE_OK — coordinator state machine (turn lifecycle + scheduling)
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use serde_json::Value;

use crate::agent::{
    ProviderCompactionFacts, ProviderCompactionTurnFact, ProviderContext, ProviderConversationTurn,
    ProviderFileOperationFact,
};
use crate::event::{
    ArtifactWrittenEvent, EventArtifactRef, EventEnvelopeV1, EventV1, ResolvedToolIdentity,
    ToolCallMetadata, ToolIdentityMetadata,
};
use crate::path_selector::workspace_relative_path_from_maybe_absolute;
use crate::redact::Redactor;
use crate::text::non_empty_trimmed;

use super::super::RunState;
use super::restore::{collect_historical_agent_turns_until, read_historical_events_until};
use super::{summarize_compaction_text, ProviderCompactionTrigger};

const PROVIDER_CONTEXT_FILE_OPERATION_FACT_LIMIT: usize = 50;
const PROVIDER_CONTEXT_OPERATION_FACT_LIMIT: usize = 20;

#[derive(Debug, Clone, Default)]
struct ProviderOperationalMemoryFacts {
    read_files: Vec<ProviderFileOperationFact>,
    modified_files: Vec<ProviderFileOperationFact>,
    operation_facts: Vec<String>,
}

#[derive(Debug, Clone)]
struct ProviderToolContextFact {
    tool_id: String,
    args_summary: Option<String>,
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
        run_state.info.run_id.as_str(),
        &run_state.info.events_path,
        through_seq,
    ) {
        Ok(events) => events,
        Err(_) => return ProviderOperationalMemoryFacts::default(),
    };

    let mut tool_operations: BTreeMap<String, ProviderFileOperationKind> = BTreeMap::new();
    let mut tool_output_paths: BTreeMap<String, Vec<String>> = BTreeMap::new();
    let mut tool_contexts: BTreeMap<String, ProviderToolContextFact> = BTreeMap::new();
    for event in events
        .iter()
        .filter(|event| event.seq > lower_bound_seq && event.seq <= through_seq)
    {
        if !event_belongs_to_compacted_request(event, &compacted_request_ids) {
            continue;
        }
        match &event.payload {
            EventV1::ToolCallRequested(payload) => {
                tool_contexts.insert(
                    payload.tool_call_id.to_string(),
                    ProviderToolContextFact {
                        tool_id: redactor.redact_text(&payload.tool_id),
                        args_summary: non_empty_trimmed(&payload.args_summary).map(|summary| {
                            summarize_compaction_text(&redactor.redact_text(summary))
                        }),
                    },
                );
                if let Some(operation) = tool_call_operation(
                    Some(payload.tool_id.as_str()),
                    payload.metadata.as_ref(),
                    None,
                ) {
                    tool_operations.insert(payload.tool_call_id.to_string(), operation);
                }
            }
            EventV1::ToolCallFinished(payload) => {
                if let Some(operation) = tool_call_operation(None, payload.metadata.as_ref(), None)
                {
                    tool_operations
                        .entry(payload.tool_call_id.to_string())
                        .or_insert(operation);
                }
                let paths = extract_output_json_path_fields(payload.output_json.as_ref());
                if !paths.is_empty() {
                    tool_output_paths.insert(payload.tool_call_id.to_string(), paths);
                }
            }
            _ => {}
        }
    }

    let mut read = BTreeMap::new();
    let mut modified = BTreeMap::new();
    let mut operation_facts = Vec::new();
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
                let Some(tool_call_id) = payload.tool_call_id.as_ref().map(|id| id.as_str()) else {
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
                    .get(payload.tool_call_id.as_str())
                    .copied()
                    .or_else(|| tool_call_operation(None, payload.metadata.as_ref(), None));
                if operation.is_none() {
                    if let Some(context) = tool_contexts.get(payload.tool_call_id.as_str()) {
                        add_tool_operation_fact(
                            &mut operation_facts,
                            &context.tool_id,
                            payload.tool_call_id.as_str(),
                            context.args_summary.as_deref(),
                            payload.output_summary.as_deref(),
                            redactor,
                        );
                    }
                }
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

    finalize_provider_operational_memory(read, modified, operation_facts)
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
        .filter_map(|turn| turn.request_id.as_ref().map(|r| r.as_str()))
        .map(str::to_string)
        .collect::<BTreeSet<_>>();
    if !request_ids.is_empty() {
        return request_ids;
    }

    let Ok(historical_turns) = collect_historical_agent_turns_until(
        run_state.info.run_id.as_str(),
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

fn add_tool_operation_fact(
    facts: &mut Vec<String>,
    tool_id: &str,
    tool_call_id: &str,
    args_summary: Option<&str>,
    output_summary: Option<&str>,
    redactor: &(impl Redactor + ?Sized),
) {
    let mut line = format!(
        "tool {} via {}",
        redactor.redact_text(tool_id),
        tool_call_id
    );
    let args_summary = args_summary
        .and_then(non_empty_trimmed)
        .map(|summary| summarize_compaction_text(&redactor.redact_text(summary)));
    let output_summary = output_summary
        .and_then(non_empty_trimmed)
        .map(|summary| summarize_compaction_text(&redactor.redact_text(summary)));

    match (args_summary, output_summary) {
        (Some(args), Some(output)) => {
            line.push_str(": ");
            line.push_str(&args);
            line.push_str(" -> ");
            line.push_str(&output);
        }
        (Some(args), None) => {
            line.push_str(": ");
            line.push_str(&args);
        }
        (None, Some(output)) => {
            line.push_str(": ");
            line.push_str(&output);
        }
        (None, None) => {}
    }

    facts.push(summarize_compaction_text(&line));
}

fn finalize_provider_operational_memory(
    read: BTreeMap<(String, String), ProviderFileOperationFact>,
    modified: BTreeMap<(String, String), ProviderFileOperationFact>,
    extra_operation_facts: Vec<String>,
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
    for fact in extra_operation_facts {
        if operation_facts.len() >= PROVIDER_CONTEXT_OPERATION_FACT_LIMIT {
            break;
        }
        operation_facts.push(fact);
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

pub(super) fn build_provider_compaction_facts(
    run_state: &RunState,
    trigger: &ProviderCompactionTrigger,
    context: &ProviderContext,
    older_turns: &[ProviderConversationTurn],
    pruned_tool_artifacts: &[EventArtifactRef],
    redactor: &(impl Redactor + ?Sized),
) -> ProviderCompactionFacts {
    let operational_memory =
        collect_compacted_file_operation_facts(run_state, trigger, context, older_turns, redactor);
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
