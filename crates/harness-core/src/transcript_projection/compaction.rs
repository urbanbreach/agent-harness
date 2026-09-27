use super::*;

pub(super) fn apply(
    output: &mut TranscriptProjection,
    index: &TranscriptIndex,
    event: &EventEnvelopeV1,
) {
    let mut checkpoint = CompactionCheckpointProjection {
        provenance: ProvenanceRange::at(event),
        ..Default::default()
    };
    let mut part = ProjectedCompactionPart {
        provenance: ProvenanceRange::at(event),
        ..Default::default()
    };
    match &event.payload {
        EventV1::CompactionWritten(data) => {
            output.artifacts.push(TranscriptArtifactRef {
                path: data.artifact_path.clone(),
                digest: data.artifact_digest.clone(),
                bytes: Some(data.artifact_bytes),
                tool_call_id: None,
                source: ArtifactProjectionSource::CompactionWritten,
                metadata: Default::default(),
                provenance: ProvenanceRange::at(event),
            });
            return;
        }
        EventV1::SessionCompaction(data) => {
            checkpoint.agent_id.clone_from(&data.agent_id);
            checkpoint.status = CompactionCheckpointStatus::SessionCompacted;
            checkpoint.through_seq = Some(data.first_kept_event_seq.saturating_sub(1));
            checkpoint.trigger_reason = Some(data.trigger_reason.clone());
            checkpoint.tokens_before = Some(data.tokens_before);
            checkpoint.tokens_after_estimate = data.tokens_after;
            part.summary = Some(data.summary.clone());
            part.read_files.clone_from(&data.read_files);
            part.modified_files.clone_from(&data.modified_files);
            part.from_hook = Some(data.from_hook);
        }
        EventV1::BranchSummary(data) => {
            checkpoint.agent_id.clone_from(&data.agent_id);
            checkpoint.status = CompactionCheckpointStatus::BranchSummary;
            checkpoint.through_seq = Some(data.from_event_seq);
            part.summary = Some(data.summary.clone());
            part.read_files.clone_from(&data.read_files);
            part.modified_files.clone_from(&data.modified_files);
            part.from_hook = Some(data.from_hook);
        }
        _ => return,
    }
    part.agent_id.clone_from(&checkpoint.agent_id);
    part.status = checkpoint.status;
    part.trigger_reason.clone_from(&checkpoint.trigger_reason);
    part.through_seq = checkpoint.through_seq;
    part.tokens_before = checkpoint.tokens_before;
    output.compaction_checkpoints.push(checkpoint);
    push_part(output, index, event, ProjectedPart::Compaction(part));
}
