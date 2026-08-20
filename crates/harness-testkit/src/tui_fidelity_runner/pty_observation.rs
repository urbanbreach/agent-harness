use sha2::{Digest, Sha256};

use crate::parity::{semantic_frame_from_vt100_screen, SemanticFrame};
use crate::tui_fidelity::Viewport;

use super::presentation_receipt::{
    DecoderState, ObservationKind, PresentationTimestamp, RawPtyRead, TimedSemanticObservation,
};
use super::process_io::PtyRead;

const SYNC_START: &[u8] = b"\x1b[?2026h";
const SYNC_END: &[u8] = b"\x1b[?2026l";

#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum PtyObservationError {
    #[error("PTY stream ended with a truncated decoder state")]
    TruncatedStream,
}

pub struct PtyObserver {
    viewport: Viewport,
    raw_reads: Vec<RawPtyRead>,
    observations: Vec<TimedSemanticObservation>,
    stream: Vec<u8>,
    scan_cursor: usize,
    synchronized: bool,
    last_frame: Option<SemanticFrame>,
}

impl PtyObserver {
    pub const fn new(viewport: Viewport) -> Self {
        Self {
            viewport,
            raw_reads: Vec::new(),
            observations: Vec::new(),
            stream: Vec::new(),
            scan_cursor: 0,
            synchronized: false,
            last_frame: None,
        }
    }

    pub fn observe(&mut self, read: &PtyRead) {
        let read_ordinal = self.raw_reads.len();
        self.stream.extend_from_slice(&read.bytes);
        let mut capture_ends = Vec::new();
        let mut saw_delimiter = false;
        while self.scan_cursor < self.stream.len() {
            let remaining = &self.stream[self.scan_cursor..];
            if remaining.starts_with(SYNC_START) {
                self.synchronized = true;
                self.scan_cursor += SYNC_START.len();
                saw_delimiter = true;
            } else if remaining.starts_with(SYNC_END) {
                if self.synchronized {
                    self.synchronized = false;
                    capture_ends.push(self.scan_cursor + SYNC_END.len());
                }
                self.scan_cursor += SYNC_END.len();
                saw_delimiter = true;
            } else if partial_prefix(remaining, SYNC_START) || partial_prefix(remaining, SYNC_END) {
                break;
            } else {
                self.scan_cursor += 1;
            }
        }
        let state = decoder_state(
            &self.stream,
            &self.stream[self.scan_cursor..],
            self.synchronized,
        );
        self.raw_reads.push(RawPtyRead {
            read_completed_at: PresentationTimestamp(read.completed_at_micros),
            byte_len: read.bytes.len(),
            sha256: hex_digest(&read.bytes),
            decoder_state: state,
        });
        for stream_end in capture_ends.iter().copied() {
            self.capture(
                read,
                read_ordinal,
                ObservationKind::SynchronizedUpdateComplete,
                stream_end,
            );
        }
        if capture_ends.is_empty()
            && !self.synchronized
            && !saw_delimiter
            && state == DecoderState::Complete
        {
            self.capture(
                read,
                read_ordinal,
                ObservationKind::ReadCompletionDecode,
                self.stream.len(),
            );
        }
    }

    pub fn finish(
        mut self,
        stable_repeats: u8,
    ) -> Result<(Vec<RawPtyRead>, Vec<TimedSemanticObservation>), PtyObservationError> {
        if self.synchronized
            || decoder_state(
                &self.stream,
                &self.stream[self.scan_cursor..],
                self.synchronized,
            ) != DecoderState::Complete
        {
            return Err(PtyObservationError::TruncatedStream);
        }
        if let Some(frame) = self.last_frame.clone() {
            let at = self
                .raw_reads
                .last()
                .map_or(PresentationTimestamp(0), |read| read.read_completed_at);
            for _ in 0..stable_repeats {
                self.observations.push(TimedSemanticObservation {
                    observation_ordinal: self.observations.len(),
                    observed_at: at,
                    kind: ObservationKind::StableRepeat,
                    decoder_state: DecoderState::Complete,
                    raw_read_ordinals: Vec::new(),
                    frame: frame.clone(),
                });
            }
        }
        Ok((self.raw_reads, self.observations))
    }

    fn capture(
        &mut self,
        read: &PtyRead,
        ordinal: usize,
        kind: ObservationKind,
        stream_end: usize,
    ) {
        let mut parser = vt100::Parser::new(self.viewport.rows, self.viewport.cols, 0);
        parser.process(&self.stream[..stream_end]);
        let frame = semantic_frame_from_vt100_screen(parser.screen());
        if self.last_frame.as_ref() == Some(&frame) {
            return;
        }
        self.last_frame = Some(frame.clone());
        self.observations.push(TimedSemanticObservation {
            observation_ordinal: self.observations.len(),
            observed_at: PresentationTimestamp(read.completed_at_micros),
            kind,
            decoder_state: DecoderState::Complete,
            raw_read_ordinals: vec![ordinal],
            frame,
        });
    }
}

fn decoder_state(stream: &[u8], scan: &[u8], synchronized: bool) -> DecoderState {
    if synchronized || partial(scan, SYNC_START) || partial(scan, SYNC_END) {
        DecoderState::AwaitingMore
    } else {
        match std::str::from_utf8(stream) {
            Ok(_) => DecoderState::Complete,
            Err(error) if error.error_len().is_none() => DecoderState::AwaitingMore,
            Err(_) => DecoderState::Malformed,
        }
    }
}

fn partial(bytes: &[u8], delimiter: &[u8]) -> bool {
    (1..delimiter.len()).any(|length| bytes.ends_with(&delimiter[..length]))
}

fn partial_prefix(bytes: &[u8], delimiter: &[u8]) -> bool {
    bytes.len() < delimiter.len() && delimiter.starts_with(bytes)
}

fn hex_digest(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .fold(String::with_capacity(64), |mut encoded, byte| {
            const HEX: &[u8; 16] = b"0123456789abcdef";
            encoded.push(char::from(HEX[usize::from(byte >> 4)]));
            encoded.push(char::from(HEX[usize::from(byte & 0x0f)]));
            encoded
        })
}
