use serde_json::{json, Value};

#[expect(
    dead_code,
    reason = "reserved for future provider-specific parity entries as more live providers are recorded"
)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ProviderTurnCompletionExpectation {
    StreamDeltaOrTaskCompletion,
    TaskCompletionOnly,
    StreamDeltaRequired,
}

impl ProviderTurnCompletionExpectation {
    fn as_str(self) -> &'static str {
        match self {
            Self::StreamDeltaOrTaskCompletion => "stream_delta_or_task_completion",
            Self::TaskCompletionOnly => "task_completion_only",
            Self::StreamDeltaRequired => "stream_delta_required",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ProviderTurnExpectation {
    pub provider_name: &'static str,
    pub label: &'static str,
    pub completion: ProviderTurnCompletionExpectation,
    pub notes: &'static [&'static str],
}

const REGISTERED_PROVIDER_TURN_EXPECTATIONS: &[ProviderTurnExpectation] = &[ProviderTurnExpectation {
    provider_name: "default",
    label: "Blessed CLIProxy default path",
    completion: ProviderTurnCompletionExpectation::StreamDeltaOrTaskCompletion,
    notes: &[
        "Parity signoff records the selected provider explicitly instead of treating the default path as a universal stand-in.",
        "The default loopback bridge may expose streaming deltas or only provider-task completion; both count as completed provider-turn evidence.",
    ],
}];

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct ProviderTurnObservation {
    pub request_id: Option<String>,
    pub provider_task_id: Option<String>,
    pub saw_started: bool,
    pub saw_finished: bool,
    pub delta_count: usize,
    pub provider_task_completed: bool,
    pub task_completed_summary: Option<String>,
    pub run_failed: Option<String>,
}

impl ProviderTurnObservation {
    pub(crate) fn completion_observed(&self) -> bool {
        self.delta_count > 0 || self.provider_task_completed
    }

    pub(crate) fn completion_mode(&self) -> &'static str {
        match (self.delta_count > 0, self.provider_task_completed) {
            (true, true) => "stream_delta_and_task_completion",
            (true, false) => "stream_delta_only",
            (false, true) => "task_completion_only",
            (false, false) => "missing_completion_evidence",
        }
    }
}

pub(crate) fn collect_provider_turn_observation(events_body: &str) -> ProviderTurnObservation {
    let mut observation = ProviderTurnObservation::default();

    for (idx, line) in events_body.lines().enumerate() {
        let event: Value = serde_json::from_str(line).unwrap_or_else(|err| {
            panic!("events line {} is invalid JSON: {err}", idx + 1);
        });

        let event_type = event
            .get("payload")
            .and_then(|payload| payload.get("event_type"))
            .and_then(Value::as_str)
            .unwrap_or_default();

        let data = event
            .get("payload")
            .and_then(|payload| payload.get("data"))
            .cloned()
            .unwrap_or(Value::Null);

        match event_type {
            "provider_request_started" => {
                observation.request_id = data
                    .get("request_id")
                    .and_then(Value::as_str)
                    .map(ToOwned::to_owned);
                observation.saw_started = true;
            }
            "task_scheduled" => {
                if observation.provider_task_id.is_none()
                    && data
                        .get("queue_key")
                        .and_then(Value::as_str)
                        .is_some_and(|queue_key| queue_key.starts_with("provider_model:"))
                {
                    observation.provider_task_id = data
                        .get("task_id")
                        .and_then(Value::as_str)
                        .map(ToOwned::to_owned);
                }
            }
            "provider_stream_delta" => {
                observation.delta_count += 1;
            }
            "provider_request_finished" => {
                observation.saw_finished = true;
            }
            "task_completed" => {
                if observation.provider_task_id.as_deref()
                    == data.get("task_id").and_then(Value::as_str)
                {
                    observation.provider_task_completed = true;
                }
                if observation.task_completed_summary.is_none() {
                    observation.task_completed_summary = data
                        .get("result_summary")
                        .and_then(Value::as_str)
                        .map(ToOwned::to_owned);
                }
            }
            "run_failed" => {
                observation.run_failed = data
                    .get("error")
                    .and_then(Value::as_str)
                    .map(ToOwned::to_owned)
                    .or_else(|| Some("run_failed event missing error detail".to_string()));
            }
            _ => {}
        }
    }

    observation
}

pub(crate) fn assert_provider_turn_completed(
    observation: &ProviderTurnObservation,
) -> Result<(), String> {
    if let Some(run_failed) = observation.run_failed.as_deref() {
        return Err(format!(
            "run failed before provider completion: {run_failed}"
        ));
    }
    if !observation.saw_started {
        return Err("expected provider_request_started event".to_string());
    }
    if !observation.saw_finished {
        return Err("expected provider_request_finished event".to_string());
    }
    if !observation.completion_observed() {
        return Err(
            "expected either provider_stream_delta events or a completed provider task for the provider request"
                .to_string(),
        );
    }

    Ok(())
}

pub(crate) fn provider_turn_expectation(
    provider_name: &str,
) -> Option<&'static ProviderTurnExpectation> {
    REGISTERED_PROVIDER_TURN_EXPECTATIONS
        .iter()
        .find(|expectation| expectation.provider_name == provider_name)
}

pub(crate) fn require_provider_turn_expectation(
    provider_name: &str,
) -> Result<&'static ProviderTurnExpectation, String> {
    REGISTERED_PROVIDER_TURN_EXPECTATIONS
        .iter()
        .find(|expectation| expectation.provider_name == provider_name)
        .ok_or_else(|| {
            let registered = REGISTERED_PROVIDER_TURN_EXPECTATIONS
                .iter()
                .map(|expectation| expectation.provider_name)
                .collect::<Vec<_>>()
                .join(", ");
            format!(
                "provider `{provider_name}` is not recorded in live parity expectations; add an explicit entry to REGISTERED_PROVIDER_TURN_EXPECTATIONS (currently: {registered})"
            )
        })
}

pub(crate) fn assert_registered_provider_turn(
    provider_name: &str,
    observation: &ProviderTurnObservation,
) -> Result<&'static ProviderTurnExpectation, String> {
    assert_provider_turn_completed(observation)?;
    let expectation = require_provider_turn_expectation(provider_name)?;

    let matches = match expectation.completion {
        ProviderTurnCompletionExpectation::StreamDeltaOrTaskCompletion => {
            observation.completion_observed()
        }
        ProviderTurnCompletionExpectation::TaskCompletionOnly => {
            observation.delta_count == 0 && observation.provider_task_completed
        }
        ProviderTurnCompletionExpectation::StreamDeltaRequired => observation.delta_count > 0,
    };

    if matches {
        Ok(expectation)
    } else {
        Err(format!(
            "provider `{provider_name}` expected {:?}, observed {} (deltas={}, provider_task_completed={})",
            expectation.completion,
            observation.completion_mode(),
            observation.delta_count,
            observation.provider_task_completed
        ))
    }
}

pub(crate) fn provider_turn_summary(
    provider_name: &str,
    observation: &ProviderTurnObservation,
) -> Result<Value, String> {
    Ok(json!({
        "provider": provider_name,
        "expectation": provider_turn_expectation(provider_name).map(|expectation| {
            json!({
                "label": expectation.label,
                "completion": expectation.completion.as_str(),
                "notes": expectation.notes,
            })
        }),
        "expectation_status": if provider_turn_expectation(provider_name).is_some() {
            "recorded"
        } else {
            "unrecorded"
        },
        "observation": {
            "request_id": observation.request_id,
            "provider_task_id": observation.provider_task_id,
            "provider_request_started": observation.saw_started,
            "provider_request_finished": observation.saw_finished,
            "delta_count": observation.delta_count,
            "provider_task_completed": observation.provider_task_completed,
            "completion_mode": observation.completion_mode(),
            "task_completed_summary": observation.task_completed_summary,
            "run_failed": observation.run_failed,
        }
    }))
}

pub(crate) fn assert_events_show_successful_provider_turn(provider_name: &str, events_body: &str) {
    let observation = collect_provider_turn_observation(events_body);
    assert_provider_turn_completed(&observation).unwrap_or_else(|err| {
        panic!("provider turn did not complete successfully: {err}\nevents:\n{events_body}")
    });
    if provider_turn_expectation(provider_name).is_some() {
        assert_registered_provider_turn(provider_name, &observation).unwrap_or_else(|err| {
            panic!("provider turn parity mismatch: {err}\nevents:\n{events_body}")
        });
    }
}
