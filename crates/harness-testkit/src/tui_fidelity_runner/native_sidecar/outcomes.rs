use serde::Deserialize;

use crate::tui_fidelity_runner::NativePresentationOutcome;

#[derive(Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub(super) enum RuntimeTraceOutcome {
    NoVisibleChange {
        cause_id: String,
        closed_at: u64,
    },
    ResyncRequired {
        rejected_revision: u64,
        replacement_revision: u64,
        recorded_at: u64,
    },
}

pub(super) fn convert_trace_outcome(
    value: RuntimeTraceOutcome,
) -> Result<NativePresentationOutcome, crate::tui_fidelity_runner::RunnerError> {
    Ok(match value {
        RuntimeTraceOutcome::NoVisibleChange {
            cause_id,
            closed_at,
        } => NativePresentationOutcome {
            cause_id: Some(cause_id),
            kind: "no_visible_change".to_owned(),
            closed_at: Some(crate::tui_fidelity_runner::PresentationTimestamp(closed_at)),
            rejected_revision: None,
            replacement_revision: None,
            recorded_at: None,
        },
        RuntimeTraceOutcome::ResyncRequired {
            rejected_revision,
            replacement_revision,
            recorded_at,
        } => NativePresentationOutcome {
            cause_id: None,
            kind: "resync_required".to_owned(),
            closed_at: None,
            rejected_revision: Some(rejected_revision),
            replacement_revision: Some(replacement_revision),
            recorded_at: Some(crate::tui_fidelity_runner::PresentationTimestamp(
                recorded_at,
            )),
        },
    })
}
