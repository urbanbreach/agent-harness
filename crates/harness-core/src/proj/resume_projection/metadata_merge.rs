use crate::event::{
    EventArtifactRef, HookExecutionMetadata, ResolvedToolIdentity, ToolCallMetadata,
};

use super::ResumeToolCallSnapshot;

pub(super) fn merge_resolved_tool_identity(
    snapshot: &mut ResumeToolCallSnapshot,
    incoming: ResolvedToolIdentity,
) {
    if incoming.is_empty() {
        return;
    }

    let identity = snapshot
        .resolved_tool_identity
        .get_or_insert_with(ResolvedToolIdentity::default);
    if identity.invoked_tool_id.is_none() {
        identity.invoked_tool_id = incoming.invoked_tool_id;
    }
    if identity.effective_tool_id.is_none() {
        identity.effective_tool_id = incoming.effective_tool_id;
    }
    if identity.canonical_tool_id.is_none() {
        identity.canonical_tool_id = incoming.canonical_tool_id;
    }
    if identity.alias_source_tool_id.is_none() {
        identity.alias_source_tool_id = incoming.alias_source_tool_id;
    }
}

pub(super) fn merge_tool_call_metadata(
    snapshot: &mut ResumeToolCallSnapshot,
    incoming: ToolCallMetadata,
) {
    let ToolCallMetadata {
        canonical_tool_id,
        alias_source_tool_id,
        lineage,
        artifact_refs,
        timing,
        hook_executions,
    } = incoming;

    let metadata = snapshot
        .metadata
        .get_or_insert_with(ToolCallMetadata::default);
    if metadata.canonical_tool_id.is_none() {
        metadata.canonical_tool_id = canonical_tool_id;
    }
    if metadata.alias_source_tool_id.is_none() {
        metadata.alias_source_tool_id = alias_source_tool_id;
    }
    if metadata.lineage.is_none() {
        metadata.lineage = lineage;
    }
    if metadata.timing.is_none() {
        metadata.timing = timing;
    }
    for artifact_ref in artifact_refs {
        merge_artifact_ref(&mut metadata.artifact_refs, artifact_ref);
    }
    for hook_execution in hook_executions {
        merge_hook_execution(&mut metadata.hook_executions, hook_execution);
    }
}

pub(super) fn merge_artifact_ref(
    existing: &mut Vec<EventArtifactRef>,
    candidate: EventArtifactRef,
) {
    if existing
        .iter()
        .any(|current| current.path == candidate.path && current.digest == candidate.digest)
    {
        return;
    }

    existing.push(candidate);
    existing.sort_by(|left, right| {
        left.path
            .cmp(&right.path)
            .then_with(|| left.digest.cmp(&right.digest))
    });
}

pub(super) fn merge_hook_execution(
    existing: &mut Vec<HookExecutionMetadata>,
    candidate: HookExecutionMetadata,
) {
    if existing.iter().any(|current| current == &candidate) {
        return;
    }
    existing.push(candidate);
}
