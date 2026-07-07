use crate::UnwrapOrAbort;
use std::fs;
use std::path::Path;

use harness_core::event::EventEnvelopeV1;

const EVENTS_FILE_NAME: &str = "events.jsonl";

pub(crate) fn load_session_events(session_path: &Path) -> Result<Vec<EventEnvelopeV1>, String> {
    let events_path = session_path.join(EVENTS_FILE_NAME);
    let body = fs::read_to_string(&events_path)
        .map_err(|err| format!("failed to read {}: {err}", events_path.display()))?;
    let mut events = Vec::new();
    for (line_number, line) in body.lines().enumerate() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        let event = serde_json::from_str::<EventEnvelopeV1>(trimmed).map_err(|err| {
            format!(
                "failed to parse {} line {}: {err}",
                events_path.display(),
                line_number + 1
            )
        })?;
        events.push(event);
    }
    Ok(events)
}

#[cfg(test)]
mod tests {
    use crate::UnwrapOrAbort;
    use harness_core::event::{
        ActorKind, EventActor, EventEnvelopeV1, EventV1, RunStartedEvent, SCHEMA_VERSION,
    };

    use super::*;

    fn event(seq: u64) -> EventEnvelopeV1 {
        EventEnvelopeV1 {
            schema_version: SCHEMA_VERSION,
            event_id: format!("evt-{seq:04}"),
            seq,
            run_id: "run_fixture".to_string(),
            mono_ms: seq,
            ts: None,
            actor: EventActor::new(ActorKind::System, Some("coordinator".to_string())),
            correlation_id: None,
            causation_id: None,
            stream_key: Some("run:run_fixture".to_string()),
            payload: EventV1::RunStarted(RunStartedEvent {
                run_name: format!("run-{seq}"),
                workspace_root: "/tmp".to_string(),
            }),
        }
    }

    fn write_events(run_dir: &Path, events: &[EventEnvelopeV1], separator: &str) {
        let body = events
            .iter()
            .map(|event| serde_json::to_string(event).unwrap_or_abort())
            .collect::<Vec<_>>()
            .join(separator);
        fs::write(run_dir.join(EVENTS_FILE_NAME), body).unwrap_or_abort();
    }

    fn run_started_count(events: &[EventEnvelopeV1]) -> usize {
        events
            .iter()
            .filter(|event| matches!(event.payload, EventV1::RunStarted(_)))
            .count()
    }

    #[test]
    fn session_event_loader_skips_blank_lines_and_trims_events() {
        let run_dir = tempfile::tempdir().unwrap_or_abort();
        write_events(run_dir.path(), &[event(1), event(2)], "\n\n  \n");

        let events = load_session_events(run_dir.path()).unwrap_or_abort();

        assert_eq!(run_started_count(&events), 2);
    }
}
