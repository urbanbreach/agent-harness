use std::fs;
use std::path::Path;

use crate::event::EventEnvelopeV1;
use crate::ids::RunId;

use super::LegacyWarning;

#[derive(Debug, Clone, PartialEq)]
pub struct LegacyHistoryRecovery {
    events: Vec<EventEnvelopeV1>,
    warnings: Vec<LegacyWarning>,
}

impl LegacyHistoryRecovery {
    pub fn events(&self) -> &[EventEnvelopeV1] {
        &self.events
    }

    pub fn warnings(&self) -> &[LegacyWarning] {
        &self.warnings
    }

    pub(crate) fn into_parts(self) -> (Vec<EventEnvelopeV1>, Vec<LegacyWarning>) {
        (self.events, self.warnings)
    }
}

#[derive(Debug, thiserror::Error)]
pub enum LegacyHistoryRecoveryError {
    #[error("failed to read historical events {path}: {source}")]
    Read {
        path: String,
        #[source]
        source: std::io::Error,
    },
    #[error("invalid historical event line {line_number} in {path}: {source}")]
    InvalidEvent {
        path: String,
        line_number: usize,
        #[source]
        source: serde_json::Error,
    },
    #[error("historical event line {line_number} belongs to run {actual}, expected {expected}")]
    RunMismatch {
        line_number: usize,
        expected: RunId,
        actual: RunId,
    },
}

pub fn recover_event_history(
    path: &Path,
    expected_run_id: &RunId,
) -> Result<LegacyHistoryRecovery, LegacyHistoryRecoveryError> {
    let body = fs::read_to_string(path).map_err(|source| LegacyHistoryRecoveryError::Read {
        path: path.display().to_string(),
        source,
    })?;
    let lines = body.lines().collect::<Vec<_>>();
    let final_non_empty = lines.iter().rposition(|line| !line.trim().is_empty());
    let mut events = Vec::new();
    let mut warnings = Vec::new();

    for (index, line) in lines.iter().enumerate() {
        if line.trim().is_empty() {
            continue;
        }
        let event = match serde_json::from_str::<EventEnvelopeV1>(line) {
            Ok(event) => event,
            Err(source)
                if Some(index) == final_non_empty && source.is_eof() && !body.ends_with('\n') =>
            {
                warnings.push(LegacyWarning::RecoveredCorruptFinalLine {
                    line_number: index + 1,
                });
                break;
            }
            Err(source) => {
                return Err(LegacyHistoryRecoveryError::InvalidEvent {
                    path: path.display().to_string(),
                    line_number: index + 1,
                    source,
                });
            }
        };
        if &event.run_id != expected_run_id {
            return Err(LegacyHistoryRecoveryError::RunMismatch {
                line_number: index + 1,
                expected: expected_run_id.clone(),
                actual: event.run_id,
            });
        }
        events.push(event);
    }

    Ok(LegacyHistoryRecovery { events, warnings })
}
