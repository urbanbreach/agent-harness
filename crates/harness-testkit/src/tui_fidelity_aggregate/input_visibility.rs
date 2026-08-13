use super::*;

#[derive(Deserialize)]
pub(super) struct Native {
    pub(super) aggregates: NativeAggregates,
    pub(super) acknowledgements: Vec<NativeAcknowledgement>,
    #[serde(default)]
    causes: Vec<NativeCause>,
}

#[derive(Deserialize)]
struct NativeCause {
    interaction_id: Option<String>,
    resulting_revision: Option<u64>,
    outcome: NativeCauseOutcome,
}

#[derive(Clone, Copy, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
enum NativeCauseOutcome {
    VisibleChange,
    NoVisibleChange,
}

#[derive(Deserialize)]
pub(super) struct NativeAcknowledgement {
    pub(super) outcome: NativeAcknowledgementOutcome,
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
