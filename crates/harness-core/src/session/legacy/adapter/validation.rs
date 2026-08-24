use super::*;

pub(super) fn validate_envelopes(events: &[EventEnvelopeV1]) -> Result<RunId, LegacyAdapterError> {
    let Some(first) = events.first() else {
        return Err(LegacyAdapterError::EmptyInput);
    };
    let run_id = first.run_id.clone();
    let mut event_ids = BTreeSet::new();
    let mut previous_sequence = 0_u64;

    for event in events {
        if event.schema_version != SCHEMA_VERSION {
            return Err(LegacyAdapterError::UnsupportedSchema {
                expected: SCHEMA_VERSION,
                actual: event.schema_version,
            });
        }
        if event.run_id != run_id {
            return Err(LegacyAdapterError::MixedRun {
                expected: run_id,
                actual: event.run_id.clone(),
            });
        }
        if !event_ids.insert(event.event_id.as_str()) {
            return Err(LegacyAdapterError::DuplicateEvent {
                event_id: event.event_id.clone(),
            });
        }
        let Some(expected_sequence) = previous_sequence.checked_add(1) else {
            return Err(LegacyAdapterError::NonContiguousSequence {
                expected_previous: previous_sequence,
                actual: event.seq,
            });
        };
        if event.seq != expected_sequence {
            return Err(LegacyAdapterError::NonContiguousSequence {
                expected_previous: previous_sequence,
                actual: event.seq,
            });
        }
        if LegacyBoundary::non_empty(event.event_id.as_str()).is_none()
            || LegacyBoundary::non_empty(event.run_id.as_str()).is_none()
            || event
                .actor
                .agent_id
                .as_deref()
                .is_some_and(|agent_id| LegacyBoundary::non_empty(agent_id).is_none())
            || has_foreign_run_stream(event)
        {
            return Err(LegacyBoundary::invalid(event));
        }
        previous_sequence = event.seq;
    }
    Ok(run_id)
}

fn has_foreign_run_stream(event: &EventEnvelopeV1) -> bool {
    event
        .stream_key
        .as_deref()
        .and_then(|key| key.strip_prefix("run:"))
        .is_some_and(|stream_run_id| stream_run_id != event.run_id.as_str())
}
