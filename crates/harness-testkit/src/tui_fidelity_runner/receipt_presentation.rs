use std::path::Path;

use sha2::{Digest, Sha256};

use super::presentation_receipt::{
    ExternalPresentationEvidence, InteractionObservation, NativeExternalLink, PresentationClock,
    PresentationEvidence, PresentationMetricsKind,
};
use super::process::ProcessCapture;
use super::types::{ArtifactDigest, PresentationCaptureBinding};
use super::util::{sha256_file, write_json};
use super::{native_sidecar, RunnerError, RUNNER_RECEIPT_SCHEMA};
use crate::tui_fidelity::{AdapterKind, Scenario};

pub const PTY_OBSERVER_VERSION: &str = "harness.pty-observer.v1";

pub fn build(
    scenario: &Scenario,
    adapter: AdapterKind,
    evidence_dir: &Path,
    capture: &ProcessCapture,
) -> Result<(PresentationEvidence, PresentationCaptureBinding), RunnerError> {
    std::fs::create_dir_all(evidence_dir).map_err(|error| RunnerError::Io {
        path: evidence_dir.to_path_buf(),
        detail: error.to_string(),
    })?;
    let raw_path = evidence_dir.join("raw-pty.ansi");
    std::fs::write(&raw_path, &capture.pty_stream).map_err(|error| RunnerError::Io {
        path: raw_path.clone(),
        detail: error.to_string(),
    })?;
    let observations_path = evidence_dir.join("pty-observations.json");
    write_json(&observations_path, &capture.observations)?;
    let external = ExternalPresentationEvidence {
        clock: PresentationClock {
            unit: super::presentation_receipt::ClockUnit::MonotonicMicroseconds,
            epoch_id: format!("{}:{}", scenario.id.0, adapter.as_str()),
        },
        action_receipts: capture.action_receipts.clone(),
        actual_input_sends: capture.action_sends.clone(),
        raw_reads: capture.raw_reads.clone(),
        observations: capture.observations.clone(),
        interaction_observations: interaction_mappings(capture),
        raw_ansi: digest(&raw_path)?,
        observations_artifact: digest(&observations_path)?,
        metrics_kind: PresentationMetricsKind::ExternalPtyObserved,
        native_visual_observed_at: None,
    };
    let evidence = match adapter {
        AdapterKind::Grok => PresentationEvidence::ExternalOnly { external },
        AdapterKind::Harness => {
            let sidecar_path = evidence_dir.join("native-presentation.json");
            let native = native_sidecar::read_native_trace(&sidecar_path)?;
            let links = link_native_frames(&native.frames, &capture.pty_stream)?;
            PresentationEvidence::HarnessNative {
                external,
                native: Box::new(native),
                native_trace_artifact: digest(&sidecar_path)?,
                scheduling_sidecar: scheduling_artifact(evidence_dir)?,
                links,
            }
        }
    };
    let binding = PresentationCaptureBinding {
        receipt_schema: RUNNER_RECEIPT_SCHEMA.to_owned(),
        scenario_id: scenario.id.0.clone(),
        action_schedule_sha256: hash_json(&scenario.actions)?,
        motion_contract_sha256: hash_json(&scenario.motion_capture)?,
        observer_version: PTY_OBSERVER_VERSION.to_owned(),
        terminal_identity: scenario.terminal_type.as_str().to_owned(),
        measurement_kind: PresentationMetricsKind::ExternalPtyObserved,
    };
    Ok((evidence, binding))
}

fn scheduling_artifact(evidence_dir: &Path) -> Result<Option<ArtifactDigest>, RunnerError> {
    let path = evidence_dir.join("scheduling.json");
    if path.is_file() {
        digest(&path).map(Some)
    } else {
        Ok(None)
    }
}

fn interaction_mappings(capture: &ProcessCapture) -> Vec<InteractionObservation> {
    capture
        .action_sends
        .iter()
        .enumerate()
        .map(|(send_index, send)| {
            let next_send_at = capture
                .action_sends
                .get(send_index + 1)
                .map(|next| next.sent_at);
            let baseline = capture
                .observations
                .iter()
                .rev()
                .find(|observation| observation.observed_at < send.sent_at)
                .map(|observation| &observation.frame);
            let first = capture
                .observations
                .iter()
                .find(|observation| {
                    observation.observed_at >= send.sent_at
                        && next_send_at.is_none_or(|next| observation.observed_at < next)
                        && baseline != Some(&observation.frame)
                })
                .map(|observation| observation.observation_ordinal);
            InteractionObservation {
                interaction_id: send.interaction_id.clone(),
                first_changed_observation: first,
                diagnostic: first
                    .is_none()
                    .then(|| "no changed PTY observation".to_owned()),
            }
        })
        .collect()
}

fn link_native_frames(
    frames: &[super::presentation_receipt::NativeFrame],
    stream: &[u8],
) -> Result<Vec<NativeExternalLink>, RunnerError> {
    let mut cursor = 0;
    let mut links = Vec::with_capacity(frames.len());
    for frame in frames {
        let offset = stream[cursor..]
            .windows(frame.byte_count)
            .position(|window| hex_digest(window) == frame.byte_sha256)
            .map(|relative| cursor + relative)
            .ok_or_else(|| RunnerError::Process {
                adapter: AdapterKind::Harness,
                detail: format!(
                    "native frame {} has no ordered PTY byte match",
                    frame.sequence
                ),
            })?;
        links.push(NativeExternalLink {
            frame_sequence: frame.sequence,
            byte_sha256: frame.byte_sha256.clone(),
            stream_offset: offset,
        });
        cursor = offset.saturating_add(frame.byte_count);
    }
    Ok(links)
}

fn digest(path: &Path) -> Result<ArtifactDigest, RunnerError> {
    Ok(ArtifactDigest {
        path: path.to_string_lossy().into_owned(),
        sha256: sha256_file(path)?,
    })
}

fn hash_json(value: &impl serde::Serialize) -> Result<String, RunnerError> {
    let bytes = serde_json::to_vec(value).map_err(|error| RunnerError::Arguments {
        detail: format!("presentation binding serialization: {error}"),
    })?;
    Ok(hex_digest(&bytes))
}

fn hex_digest(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .fold(String::with_capacity(64), |mut output, byte| {
            use std::fmt::Write as _;
            let _ = write!(output, "{byte:02x}");
            output
        })
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use crate::parity::{CursorState, SemanticFrame};

    use super::super::presentation_receipt::{
        ActualInputSend, DecoderState, InteractionId, ObservationKind, PresentationTimestamp,
        TimedSemanticObservation,
    };
    use super::{interaction_mappings, ProcessCapture};

    #[test]
    fn changed_observation_is_bounded_by_the_next_input_send() {
        // arrange
        let unchanged = SemanticFrame::new(1, 1, CursorState::hidden(0, 0));
        let mut changed = unchanged.clone();
        changed.cells[0].grapheme = "x".into();
        let capture = ProcessCapture {
            exit_code: 0,
            input_timestamps: Vec::<Duration>::new(),
            checkpoints: Vec::new(),
            raw_reads: Vec::new(),
            observations: vec![
                observation(0, 5, unchanged.clone()),
                observation(1, 12, unchanged),
                observation(2, 25, changed),
            ],
            action_sends: vec![send("click", 0, 10), send("escape", 1, 20)],
            action_receipts: Vec::new(),
            pty_stream: Vec::new(),
        };

        // act
        let mappings = interaction_mappings(&capture);

        // assert
        assert_eq!(mappings[0].first_changed_observation, None);
        assert_eq!(mappings[1].first_changed_observation, Some(2));
    }

    fn send(id: &str, action_ordinal: usize, at: u64) -> ActualInputSend {
        ActualInputSend {
            interaction_id: InteractionId(id.into()),
            action_ordinal,
            scheduled_at: PresentationTimestamp(at),
            sent_at: PresentationTimestamp(at),
            transport_drained_at: None,
        }
    }

    fn observation(
        observation_ordinal: usize,
        at: u64,
        frame: SemanticFrame,
    ) -> TimedSemanticObservation {
        TimedSemanticObservation {
            observation_ordinal,
            observed_at: PresentationTimestamp(at),
            kind: ObservationKind::ReadCompletionDecode,
            decoder_state: DecoderState::Complete,
            raw_read_ordinals: vec![observation_ordinal],
            frame,
        }
    }
}
