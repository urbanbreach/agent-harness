use super::{EventEnvelopeV1, EventStoreError};
use std::{
    fs::File,
    io::{BufRead, BufReader, Read, Take},
    path::Path,
};

pub(super) const MAX_RECORD_BYTES: usize = 16 * 1024 * 1024;

/// Reads a fixed journal prefix without acquiring a writer lock or changing files.
pub struct JournalReader {
    input: BufReader<Take<File>>,
    line: Vec<u8>,
    next_seq: u64,
    run_id: Option<String>,
    failed: bool,
    needs_newline: bool,
    consumed: u64,
    validated: u64,
}

impl JournalReader {
    pub fn open(path: &Path, length: u64) -> Result<Self, EventStoreError> {
        Ok(Self {
            input: BufReader::new(super::open_private_file(path)?.take(length)),
            line: Vec::new(),
            next_seq: 1,
            run_id: None,
            failed: false,
            needs_newline: false,
            consumed: 0,
            validated: 0,
        })
    }
    pub(super) fn needs_newline(&self) -> bool {
        self.needs_newline
    }
    pub(super) fn validated_length(&self) -> u64 {
        self.validated
    }

    fn read(&mut self) -> Result<Option<EventEnvelopeV1>, EventStoreError> {
        loop {
            self.line.clear();
            loop {
                let available = self.input.fill_buf()?;
                if available.is_empty() {
                    break;
                }
                let count = available
                    .iter()
                    .position(|b| *b == b'\n')
                    .map_or(available.len(), |n| n + 1);
                if self.line.len() + count > MAX_RECORD_BYTES {
                    return Err(EventStoreError::Invalid("journal record exceeds 16 MiB"));
                }
                self.line.extend_from_slice(&available[..count]);
                self.input.consume(count);
                self.consumed += count as u64;
                if self.line.last() == Some(&b'\n') {
                    break;
                }
            }
            if self.line.is_empty() {
                return Ok(None);
            }
            self.needs_newline = self.line.last() != Some(&b'\n');
            if self.line.iter().all(u8::is_ascii_whitespace) {
                self.validated = self.consumed;
                continue;
            }
            let event: EventEnvelopeV1 = serde_json::from_slice(&self.line)?;
            if event.seq != self.next_seq
                || event.schema_version != crate::event::SCHEMA_VERSION
                || event.event_id.is_empty()
                || event.run_id.as_str().is_empty()
                || self
                    .run_id
                    .as_ref()
                    .is_some_and(|id| id != event.run_id.as_str())
            {
                return Err(EventStoreError::Invalid(
                    "invalid journal sequence, run identity, or schema",
                ));
            }
            self.run_id.get_or_insert_with(|| event.run_id.to_string());
            self.next_seq = self
                .next_seq
                .checked_add(1)
                .ok_or(EventStoreError::Invalid("event sequence exhausted"))?;
            self.validated = self.consumed;
            return Ok(Some(event));
        }
    }
}

impl Iterator for JournalReader {
    type Item = Result<EventEnvelopeV1, EventStoreError>;
    fn next(&mut self) -> Option<Self::Item> {
        if self.failed {
            return None;
        }
        match self.read() {
            Ok(event) => event.map(Ok),
            Err(e) => {
                self.failed = true;
                Some(Err(e))
            }
        }
    }
}

pub fn read_events(path: &Path) -> Result<Vec<EventEnvelopeV1>, EventStoreError> {
    JournalReader::open(path, std::fs::metadata(path)?.len())?.collect()
}
