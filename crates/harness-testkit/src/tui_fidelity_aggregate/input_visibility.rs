use std::path::Path;

use serde::Deserialize;

use super::helpers::evidence;
use super::types::InputSend;
use super::AggregateError;

#[derive(Deserialize)]
pub(super) struct Native {
    pub(super) aggregates: NativeAggregates,
    pub(super) acknowledgements: Vec<NativeAcknowledgement>,
    #[serde(default)]
    pub(super) causes: Vec<NativeCause>,
    #[serde(default)]
    pub(super) frames: Vec<NativeFrame>,
}

#[derive(Deserialize)]
pub(super) struct NativeCause {
    #[serde(default)]
    pub(super) cause_id: String,
    pub(super) interaction_id: Option<String>,
    #[serde(default)]
    pub(super) received_at: u64,
    #[serde(default)]
    pub(super) kind: String,
    pub(super) resulting_revision: Option<u64>,
    pub(super) outcome: NativeCauseOutcome,
}

#[derive(Clone, Copy, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub(super) enum NativeCauseOutcome {
    VisibleChange,
    NoVisibleChange,
}

#[derive(Deserialize)]
pub(super) struct NativeAcknowledgement {
    #[serde(default)]
    pub(super) sequence: u64,
    #[serde(default)]
    pub(super) acknowledged_at: u64,
    pub(super) outcome: NativeAcknowledgementOutcome,
}

#[derive(Deserialize)]
pub(super) struct NativeFrame {
    pub(super) sequence: u64,
    pub(super) revision: u64,
    pub(super) cause_ids: Vec<String>,
}

#[derive(Clone, Copy, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub(super) enum NativeAcknowledgementOutcome {
    CompletedWrite,
    FailedWrite,
    ResyncRequired,
}

#[derive(Deserialize)]
pub(super) struct NativeAggregates {
    pub(super) idle_redraws: u64,
}

pub(super) fn visible_send_timestamps(
    sends: &[InputSend],
    native: &Native,
    root: &Path,
) -> Result<Vec<u64>, AggregateError> {
    let mut visible = Vec::with_capacity(sends.len());
    for send in sends {
        let causes = native
            .causes
            .iter()
            .filter(|cause| cause.interaction_id.as_deref() == Some(&send.interaction_id))
            .collect::<Vec<_>>();
        if causes.is_empty() {
            return evidence(root, "native interaction linkage missing for input send");
        }
        let no_visible = causes.iter().all(|cause| {
            cause.outcome == NativeCauseOutcome::NoVisibleChange
                && cause.resulting_revision.is_none()
        });
        if !no_visible {
            let timestamp = send.sent_at.ok_or_else(|| AggregateError::Evidence {
                path: root.to_path_buf(),
                detail: "visible input send timestamp missing".into(),
            })?;
            visible.push(timestamp);
        }
    }
    Ok(visible)
}
