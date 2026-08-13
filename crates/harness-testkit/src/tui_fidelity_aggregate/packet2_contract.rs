use std::collections::BTreeSet;

use super::helpers::evidence;
use super::input_visibility::{Native, NativeAcknowledgementOutcome, NativeCauseOutcome};
use super::types::{External, InputSend, PresentationLink};
use super::AggregateError;
use crate::tui_fidelity::ScenarioAction;
use crate::tui_fidelity_compare::PresentationTimingMetrics;

mod clock_bridge;

const FAST_PHYSICAL_MICROS: u64 = 32_000;
const STREAM_CADENCE_MICROS: u64 = 33_000;

pub(super) struct Packet2Contract {
    type_windows: Vec<TypeWindow>,
    linked_type_observations: Vec<u64>,
}

struct TypeWindow {
    start: u64,
    end: u64,
    cadence_micros: u64,
}

pub(super) fn verify(
    sends: &[InputSend],
    external: &External,
    native: &Native,
    links: &[PresentationLink],
    root: &std::path::Path,
) -> Result<Packet2Contract, AggregateError> {
    let scenario = crate::tui_fidelity::Scenario::from_json(include_str!(
        "../../tests/fixtures/tui_fidelity/packet2-sustained-stream.json"
    ))
    .map_err(|error| AggregateError::Evidence {
        path: root.to_path_buf(),
        detail: format!("invalid canonical Packet 2 scenario: {error}"),
    })?;
    let actions = sends
        .iter()
        .map(|send| {
            scenario
                .actions
                .get(send.action_ordinal)
                .map(|action| (send, action))
                .ok_or_else(|| AggregateError::Evidence {
                    path: root.to_path_buf(),
                    detail: format!("unknown Packet 2 action ordinal {}", send.action_ordinal),
                })
        })
        .collect::<Result<Vec<_>, _>>()?;
    let mut type_windows = Vec::new();
    for (index, (send, action)) in actions.iter().enumerate() {
        let linked = native
            .causes
            .iter()
            .filter(|cause| cause.interaction_id.as_deref() == Some(&send.interaction_id))
            .collect::<Vec<_>>();
        if linked.is_empty() {
            return evidence(root, "native interaction linkage missing for input send");
        }
        let bound = match action {
            ScenarioAction::TypeText(action) => action.inter_byte_millis.saturating_mul(2_000),
            _ => FAST_PHYSICAL_MICROS,
        };
        for cause in linked.iter().filter(|cause| {
            cause.outcome == NativeCauseOutcome::VisibleChange || cause.resulting_revision.is_some()
        }) {
            verify_visible_revision(native, cause, bound, root)?;
        }
        if let ScenarioAction::TypeText(action) = action {
            let start = linked
                .iter()
                .map(|cause| cause.received_at)
                .min()
                .ok_or_else(|| AggregateError::Evidence {
                    path: root.to_path_buf(),
                    detail: "TypeText native receive timestamp missing".into(),
                })?;
            let end = actions
                .get(index + 1)
                .and_then(|(next, _)| {
                    native
                        .causes
                        .iter()
                        .filter(|cause| {
                            cause.interaction_id.as_deref() == Some(&next.interaction_id)
                        })
                        .map(|cause| cause.received_at)
                        .min()
                })
                .unwrap_or(u64::MAX);
            verify_type_window(native, start, end, bound, root)?;
            type_windows.push(TypeWindow {
                start,
                end,
                cadence_micros: action.inter_byte_millis.saturating_mul(1_000),
            });
        }
    }
    let linked_type_observations =
        clock_bridge::linked_type_observations(external, links, native, &type_windows);
    Ok(Packet2Contract {
        type_windows,
        linked_type_observations,
    })
}

fn verify_type_window(
    native: &Native,
    start: u64,
    end: u64,
    bound: u64,
    root: &std::path::Path,
) -> Result<(), AggregateError> {
    let causes = native.causes.iter().filter(|cause| {
        cause.kind == "terminal_input"
            && cause.received_at >= start
            && cause.received_at < end
            && cause.outcome == NativeCauseOutcome::VisibleChange
    });
    for cause in causes {
        verify_visible_revision(native, cause, bound, root)?;
    }
    Ok(())
}

fn verify_visible_revision(
    native: &Native,
    cause: &super::input_visibility::NativeCause,
    bound: u64,
    root: &std::path::Path,
) -> Result<(), AggregateError> {
    let revision = cause
        .resulting_revision
        .ok_or_else(|| AggregateError::Evidence {
            path: root.to_path_buf(),
            detail: "visible native cause lacks resulting revision".into(),
        })?;
    let frame = native
        .frames
        .iter()
        .filter(|frame| frame.revision == revision && frame.cause_ids.contains(&cause.cause_id))
        .min_by_key(|frame| frame.sequence)
        .ok_or_else(|| AggregateError::Evidence {
            path: root.to_path_buf(),
            detail: "visible native revision lacks containing frame".into(),
        })?;
    let anchor = native
        .causes
        .iter()
        .filter(|candidate| {
            frame.cause_ids.contains(&candidate.cause_id)
                && candidate.kind == cause.kind
                && candidate.resulting_revision == Some(revision)
        })
        .max_by_key(|candidate| candidate.received_at)
        .ok_or_else(|| AggregateError::Evidence {
            path: root.to_path_buf(),
            detail: "native frame lacks physical receive anchor".into(),
        })?;
    let ack = native
        .acknowledgements
        .iter()
        .find(|ack| {
            ack.sequence == frame.sequence
                && ack.outcome == NativeAcknowledgementOutcome::CompletedWrite
        })
        .ok_or_else(|| AggregateError::Evidence {
            path: root.to_path_buf(),
            detail: "native revision lacks completed write acknowledgement".into(),
        })?;
    if ack.acknowledged_at.saturating_sub(anchor.received_at) > bound {
        return Err(AggregateError::Threshold(format!(
            "native input receive to completed write exceeds {bound} microseconds: root={} cause={} kind={} latency={}",
            root.display(),
            cause.cause_id,
            cause.kind,
            ack.acknowledged_at.saturating_sub(anchor.received_at)
        )));
    }
    Ok(())
}

impl Packet2Contract {
    pub(super) fn check_gaps(
        &self,
        metrics: &PresentationTimingMetrics,
        active_window: Option<(u64, u64)>,
    ) -> Result<(), AggregateError> {
        let Some((active_start, active_end)) = active_window else {
            return Err(AggregateError::Threshold(
                "Packet 2 active window is missing".into(),
            ));
        };
        let observations = self
            .linked_type_observations
            .iter()
            .copied()
            .collect::<BTreeSet<_>>();
        for window in metrics.external_observation_timestamps_micros.windows(2) {
            if window[0] < active_start || window[1] > active_end {
                continue;
            }
            let gap = window[1].saturating_sub(window[0]);
            if gap <= STREAM_CADENCE_MICROS.saturating_mul(2) {
                continue;
            }
            let typed = self.type_windows.iter().any(|typed| {
                gap <= typed.cadence_micros.saturating_mul(2)
                    && observations.contains(&window[0])
                    && observations.contains(&window[1])
            });
            if !typed {
                return Err(AggregateError::Threshold(
                    "streaming gap exceeds twice 33 ms cadence".into(),
                ));
            }
        }
        Ok(())
    }
}
