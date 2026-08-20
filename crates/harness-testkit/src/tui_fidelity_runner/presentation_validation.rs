use std::collections::HashSet;

use crate::tui_fidelity::AdapterKind;

use super::presentation_receipt::{
    ActionExecutionKind, ActionExecutionResult, DecoderState, ExternalPresentationEvidence,
    PresentationEvidence,
};

mod native;

#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum PresentationValidationError {
    #[error("Harness requires native presentation evidence")]
    HarnessNativeEvidenceRequired,
    #[error("Grok accepts external PTY evidence only")]
    GrokExternalEvidenceRequired,
    #[error("presentation evidence collection {field} is empty")]
    EmptyCollection { field: &'static str },
    #[error("presentation timestamps are not monotonic in {field}")]
    NonMonotonic { field: &'static str },
    #[error("PTY decoder is incomplete at raw read {ordinal}")]
    IncompleteDecoder { ordinal: usize },
    #[error("presentation reference is unresolved: {detail}")]
    UnresolvedReference { detail: String },
    #[error("native frame {sequence} does not have exactly one acknowledgement")]
    AckCardinality { sequence: u64 },
    #[error("native frame sequences are not unique and strictly ordered")]
    FrameSequenceOrder,
    #[error("native frame acknowledgement does not match frame {sequence}")]
    AckMismatch { sequence: u64 },
    #[error("visible native cause {cause_id} has no completed frame")]
    VisibleCauseUnpresented { cause_id: String },
    #[error("runner interaction {interaction_id} has no native terminal receipt cause")]
    ExternalInteractionUnlinked { interaction_id: String },
    #[error(
        "runner interaction {interaction_id} expected {expected} native causes, observed {observed}"
    )]
    NativeInteractionCardinality {
        interaction_id: String,
        expected: u16,
        observed: usize,
    },
    #[error("disclosure transition missing: {transition}")]
    DisclosureTransitionMissing { transition: &'static str },
}

pub fn validate_packet2_disclosure(
    evidence: &ExternalPresentationEvidence,
) -> Result<(), PresentationValidationError> {
    let sentinel = crate::tui_fidelity_fixture::DISCLOSURE_SENTINEL;
    let body = crate::tui_fidelity_fixture::DISCLOSURE_BODY;
    let states = evidence.observations.iter().map(|observation| {
        let text = observation
            .frame
            .cells
            .iter()
            .filter(|cell| !cell.continuation)
            .fold(String::new(), |mut text, cell| {
                text.push_str(&cell.grapheme);
                text
            });
        (text.contains(sentinel), text.contains(body))
    });
    let mut open = false;
    let mut closed_after_open = false;
    for (sentinel_visible, body_visible) in states {
        match (open, sentinel_visible, body_visible) {
            (false, true, true) => open = true,
            (true, _, false) => closed_after_open = true,
            _ => {}
        }
    }
    if !open {
        return Err(PresentationValidationError::DisclosureTransitionMissing {
            transition: "open",
        });
    }
    if !closed_after_open {
        return Err(PresentationValidationError::DisclosureTransitionMissing {
            transition: "close",
        });
    }
    Ok(())
}

pub fn validate_presentation_evidence(
    adapter: AdapterKind,
    evidence: &PresentationEvidence,
) -> Result<(), PresentationValidationError> {
    match (adapter, evidence) {
        (AdapterKind::Harness, PresentationEvidence::ExternalOnly { .. }) => {
            Err(PresentationValidationError::HarnessNativeEvidenceRequired)
        }
        (AdapterKind::Grok, PresentationEvidence::HarnessNative { .. }) => {
            Err(PresentationValidationError::GrokExternalEvidenceRequired)
        }
        (AdapterKind::Grok, PresentationEvidence::ExternalOnly { external }) => {
            validate_external(external)
        }
        (
            AdapterKind::Harness,
            PresentationEvidence::HarnessNative {
                external,
                native,
                native_trace_artifact,
                scheduling_sidecar: _,
                links,
            },
        ) => {
            validate_external(external)?;
            native::validate(native)?;
            if native_trace_artifact.sha256.len() != 64 || links.len() != native.frames.len() {
                return Err(PresentationValidationError::UnresolvedReference {
                    detail: "native trace artifact or byte linkage".to_owned(),
                });
            }
            for (link, frame) in links.iter().zip(&native.frames) {
                if !link_matches_frame(link, frame) {
                    return Err(PresentationValidationError::UnresolvedReference {
                        detail: format!("external byte link for frame {}", frame.sequence),
                    });
                }
            }
            let external_interactions = external
                .action_receipts
                .iter()
                .filter_map(|receipt| {
                    receipt
                        .expected_native_cause_count
                        .map(|count| (&receipt.interaction_id, count))
                })
                .collect::<Vec<_>>();
            let external_interaction_ids = external_interactions
                .iter()
                .map(|(interaction_id, _)| *interaction_id)
                .collect::<HashSet<_>>();
            for (interaction_id, expected) in external_interactions {
                let observed = native
                    .causes
                    .iter()
                    .filter(|cause| {
                        cause.interaction_id.as_ref() == Some(interaction_id)
                            && matches!(
                                cause.kind.as_str(),
                                "terminal_input" | "mouse" | "wheel" | "resize" | "focus"
                            )
                    })
                    .count();
                if observed != usize::from(expected) {
                    return Err(PresentationValidationError::NativeInteractionCardinality {
                        interaction_id: interaction_id.0.clone(),
                        expected,
                        observed,
                    });
                }
            }
            if native.causes.iter().any(|cause| {
                cause
                    .interaction_id
                    .as_ref()
                    .is_some_and(|id| !external_interaction_ids.contains(id))
            }) {
                return Err(PresentationValidationError::UnresolvedReference {
                    detail: "native cause interaction".to_owned(),
                });
            }
            if links.windows(2).any(|pair| {
                pair[1].frame_sequence <= pair[0].frame_sequence
                    || pair[1].stream_offset <= pair[0].stream_offset
            }) {
                return Err(PresentationValidationError::NonMonotonic {
                    field: "native_external_links",
                });
            }
            Ok(())
        }
    }
}

fn link_matches_frame(
    link: &super::presentation_receipt::NativeExternalLink,
    frame: &super::presentation_receipt::NativeFrame,
) -> bool {
    if link.frame_sequence != frame.sequence {
        return false;
    }
    link.byte_sha256 == frame.byte_sha256
}

fn validate_external(
    evidence: &ExternalPresentationEvidence,
) -> Result<(), PresentationValidationError> {
    require_nonempty("action_receipts", &evidence.action_receipts)?;
    require_nonempty("actual_input_sends", &evidence.actual_input_sends)?;
    require_nonempty("raw_reads", &evidence.raw_reads)?;
    require_nonempty("observations", &evidence.observations)?;
    require_nonempty(
        "interaction_observations",
        &evidence.interaction_observations,
    )?;
    monotonic(
        "action_receipts",
        evidence
            .action_receipts
            .iter()
            .map(|receipt| receipt.started_at.0),
    )?;
    monotonic(
        "actual_input_sends",
        evidence
            .actual_input_sends
            .iter()
            .map(|send| send.sent_at.0),
    )?;
    validate_action_receipts(evidence)?;
    monotonic(
        "raw_reads",
        evidence
            .raw_reads
            .iter()
            .map(|read| read.read_completed_at.0),
    )?;
    monotonic(
        "observations",
        evidence.observations.iter().map(|item| item.observed_at.0),
    )?;
    for (ordinal, read) in evidence.raw_reads.iter().enumerate() {
        if matches!(
            read.decoder_state,
            DecoderState::Truncated | DecoderState::Malformed
        ) {
            return Err(PresentationValidationError::IncompleteDecoder { ordinal });
        }
    }
    let interactions = evidence
        .actual_input_sends
        .iter()
        .map(|send| &send.interaction_id)
        .collect::<HashSet<_>>();
    for mapping in &evidence.interaction_observations {
        if !interactions.contains(&mapping.interaction_id) {
            return Err(PresentationValidationError::UnresolvedReference {
                detail: mapping.interaction_id.0.clone(),
            });
        }
        if mapping
            .first_changed_observation
            .is_some_and(|ordinal| ordinal >= evidence.observations.len())
        {
            return Err(PresentationValidationError::UnresolvedReference {
                detail: format!("observation for {}", mapping.interaction_id.0),
            });
        }
    }
    Ok(())
}

fn validate_action_receipts(
    evidence: &ExternalPresentationEvidence,
) -> Result<(), PresentationValidationError> {
    let sends = evidence
        .actual_input_sends
        .iter()
        .map(|send| &send.interaction_id)
        .collect::<HashSet<_>>();
    let mut ids = HashSet::new();
    for (ordinal, receipt) in evidence.action_receipts.iter().enumerate() {
        if receipt.action_ordinal != ordinal
            || !ids.insert(&receipt.interaction_id)
            || receipt.scheduled_at > receipt.started_at
            || receipt.started_at > receipt.ended_at
        {
            return Err(PresentationValidationError::UnresolvedReference {
                detail: format!("action receipt {}", receipt.interaction_id.0),
            });
        }
        let sent = sends.contains(&receipt.interaction_id);
        match (receipt.kind, &receipt.result, sent) {
            (
                ActionExecutionKind::Observer,
                ActionExecutionResult::ObservedState { state },
                false,
            ) if receipt.semantic_post.states.contains(state) => {}
            (ActionExecutionKind::Observer, ActionExecutionResult::ObservedText, false) => {}
            (
                ActionExecutionKind::Input
                | ActionExecutionKind::Resize
                | ActionExecutionKind::TerminalReply,
                ActionExecutionResult::Applied,
                true,
            ) => {}
            _ => {
                return Err(PresentationValidationError::UnresolvedReference {
                    detail: format!("action receipt result {}", receipt.interaction_id.0),
                });
            }
        }
    }
    if sends.len()
        != evidence
            .action_receipts
            .iter()
            .filter(|receipt| receipt.kind != ActionExecutionKind::Observer)
            .count()
    {
        return Err(PresentationValidationError::UnresolvedReference {
            detail: "action send cardinality".to_owned(),
        });
    }
    Ok(())
}

pub(super) fn require_nonempty<T>(
    field: &'static str,
    values: &[T],
) -> Result<(), PresentationValidationError> {
    if values.is_empty() {
        Err(PresentationValidationError::EmptyCollection { field })
    } else {
        Ok(())
    }
}

pub(super) fn monotonic(
    field: &'static str,
    values: impl Iterator<Item = u64>,
) -> Result<(), PresentationValidationError> {
    let mut previous = None;
    for value in values {
        if previous.is_some_and(|prior| value < prior) {
            return Err(PresentationValidationError::NonMonotonic { field });
        }
        previous = Some(value);
    }
    Ok(())
}
