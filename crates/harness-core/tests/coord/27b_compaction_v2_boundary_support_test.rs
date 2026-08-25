use harness_core::UnwrapOrAbort;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct ActiveCompactionBoundary {
    pub(super) count: usize,
    pub(super) latest_seq: Option<u64>,
    pub(super) latest_payload_hash: Option<String>,
}

pub(super) fn active_compaction_boundary(
    events: &[EventEnvelopeV1],
    agent_id: &str,
) -> ActiveCompactionBoundary {
    let matching = events.iter().filter_map(|event| match &event.payload {
        EventV1::SessionCompaction(payload) if payload.agent_id == agent_id => {
            Some((event.seq, payload))
        }
        _ => None,
    });
    let mut count = 0;
    let mut latest_seq = None;
    let mut latest_payload_hash = None;
    for (seq, payload) in matching {
        count += 1;
        latest_seq = Some(seq);
        let bytes = serde_json::to_vec(payload).unwrap_or_abort();
        latest_payload_hash = Some(blake3::hash(&bytes).to_hex().to_string());
    }
    ActiveCompactionBoundary {
        count,
        latest_seq,
        latest_payload_hash,
    }
}

pub(super) fn spawn_compaction(
    harness: &CompactionV2Harness,
) -> tokio::task::JoinHandle<Result<ManualCompactionOutcome, CoordinatorError>> {
    let coordinator = harness.coordinator.clone();
    let agent_id = harness.agent_id.clone();
    tokio::spawn(async move {
        coordinator
            .compact_agent_context(agent_id, None, "manual")
            .await
    })
}
