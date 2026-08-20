use harness_core::event::{EventV1, TaskScheduleState};
use harness_core::UnwrapOrAbort;

#[test]
fn old_task_scheduled_event_deserializes_without_lineage_metadata() {
    // arrange — a task-scheduled payload written before schedule metadata existed.
    let json = r#"{
        "event_type":"task_scheduled",
        "data":{
            "task_id":"task_000001",
            "state":"started",
            "queue_key":"tool:task"
        }
    }"#;

    // act — the old payload is deserialized through the current event contract.
    let event: EventV1 = serde_json::from_str(json).unwrap_or_abort();

    // assert — legacy fields survive and absent metadata defaults to none.
    let EventV1::TaskScheduled(scheduled) = event else {
        panic!("expected task_scheduled event");
    };
    assert_eq!(
        (scheduled.state, scheduled.metadata),
        (TaskScheduleState::Started, None)
    );
}
