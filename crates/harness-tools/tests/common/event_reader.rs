use std::fs;
use std::path::Path;

use harness_core::event::EventEnvelopeV1;

pub(crate) fn read_events(path: &Path) -> Vec<EventEnvelopeV1> {
    fs::read_to_string(path)
        .expect("read events")
        .lines()
        .map(|line| serde_json::from_str(line).expect("parse event"))
        .collect()
}
