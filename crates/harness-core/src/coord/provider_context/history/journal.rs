use std::fs;
use std::io::{self, BufRead, BufReader};
use std::path::Path;

use crate::event::EventEnvelopeV1;

use super::super::super::CoordinatorError;

pub(in crate::coord::provider_context) fn read_historical_events_until(
    run_id: &str,
    events_path: &Path,
    through_seq: u64,
) -> Result<Vec<EventEnvelopeV1>, CoordinatorError> {
    let file = open_history(run_id, events_path)?;
    let mut expected_seq = 1_u64;
    let mut events = Vec::new();
    for (line_number, line) in BufReader::new(file).lines().enumerate() {
        let Some(event) = parse_event_line(run_id, events_path, line_number, line)? else {
            continue;
        };
        validate_event_seq(run_id, events_path, &event, expected_seq)?;
        expected_seq = expected_seq.saturating_add(1);
        if event.seq > through_seq {
            break;
        }
        events.push(event);
    }
    Ok(events)
}

pub(super) fn open_history(run_id: &str, events_path: &Path) -> Result<fs::File, CoordinatorError> {
    fs::File::open(events_path).map_err(|source| CoordinatorError::ResumeRestoreFailed {
        run_id: run_id.to_string(),
        reason: format!(
            "failed to open historical events {}: {source}",
            events_path.display()
        ),
    })
}

pub(super) fn parse_event_line(
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

pub(super) fn validate_event_seq(
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
