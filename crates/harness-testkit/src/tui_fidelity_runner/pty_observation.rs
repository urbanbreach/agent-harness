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
    delimiter_scan: Vec<u8>,
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
            delimiter_scan: Vec::new(),
            synchronized: false,
            last_frame: None,
        }
    }

    pub fn observe(&mut self, read: &PtyRead) {
        let read_ordinal = self.raw_reads.len();
        self.stream.extend_from_slice(&read.bytes);
        self.delimiter_scan.extend_from_slice(&read.bytes);
        let sync_started = contains(&self.delimiter_scan, SYNC_START);
        let sync_ended = contains(&self.delimiter_scan, SYNC_END);
        if sync_started {
            self.synchronized = true;
        }
        if self.synchronized && sync_ended {
            self.synchronized = false;
        }
        let state = decoder_state(&self.stream, &self.delimiter_scan, self.synchronized);
        self.raw_reads.push(RawPtyRead {
            read_completed_at: PresentationTimestamp(read.completed_at_micros),
            byte_len: read.bytes.len(),
            sha256: hex_digest(&read.bytes),
            decoder_state: state,
        });
        if sync_ended {
            self.capture(
                read,
                read_ordinal,
                ObservationKind::SynchronizedUpdateComplete,
            );
        } else if !self.synchronized && !sync_started && state == DecoderState::Complete {
            self.capture(read, read_ordinal, ObservationKind::ReadCompletionDecode);
        }
        retain_delimiter_suffix(&mut self.delimiter_scan);
    }

    pub fn finish(
        mut self,
        stable_repeats: u8,
    ) -> Result<(Vec<RawPtyRead>, Vec<TimedSemanticObservation>), PtyObservationError> {
        if self.synchronized
            || decoder_state(&self.stream, &self.delimiter_scan, self.synchronized)
                != DecoderState::Complete
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

    fn capture(&mut self, read: &PtyRead, ordinal: usize, kind: ObservationKind) {
        let mut parser = vt100::Parser::new(self.viewport.rows, self.viewport.cols, 0);
        parser.process(&self.stream);
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

fn contains(haystack: &[u8], needle: &[u8]) -> bool {
    haystack
        .windows(needle.len())
        .any(|window| window == needle)
}
fn partial(bytes: &[u8], delimiter: &[u8]) -> bool {
    (1..delimiter.len()).any(|length| bytes.ends_with(&delimiter[..length]))
}

fn retain_delimiter_suffix(scan: &mut Vec<u8>) {
    let retain = SYNC_START.len().max(SYNC_END.len()).saturating_sub(1);
    if scan.len() > retain {
        scan.drain(..scan.len() - retain);
    }
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
