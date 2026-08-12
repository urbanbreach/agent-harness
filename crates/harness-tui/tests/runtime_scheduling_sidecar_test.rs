#![allow(clippy::expect_used, reason = "owner test assertions")]

use harness_tui::presentation::{CauseId, InteractionId};
use harness_tui::runtime_scheduling::{
    SchedulingLiveReadiness, SchedulingReadinessSignal, SchedulingTelemetrySession,
};

#[test]
fn scheduling_sidecar_is_content_free_ordered_and_backlog_bound() {
    // Given: repeated terminal receipts for one typed action and a second action.
    let root = tempfile::tempdir().expect("evidence root");
    let path = root.path().join("scheduling.json");
    let mut session = SchedulingTelemetrySession::new(path.clone()).expect("session");

    // When: the runtime records terminal-ready decisions while live work remains ready.
    let typed = InteractionId::new("packet2-sustained-stream:action:0");
    let first_cause = CauseId::new("native-presentation:cause:10");
    session.record_terminal_ready(
        None,
        &first_cause,
        SchedulingLiveReadiness {
            queued_depth: 999,
            deferred_ready: true,
            stream_active: true,
        },
        false,
        Some(16),
    );
    session.record_terminal_ready(
        Some(&InteractionId::new("not-an-action")),
        &first_cause,
        SchedulingLiveReadiness {
            queued_depth: 998,
            deferred_ready: true,
            stream_active: true,
        },
        false,
        Some(16),
    );
    session.record_terminal_ready(
        Some(&typed),
        &first_cause,
        SchedulingLiveReadiness {
            queued_depth: 127,
            deferred_ready: true,
            stream_active: true,
        },
        false,
        Some(16),
    );
    session.record_terminal_ready(
        Some(&typed),
        &first_cause,
        SchedulingLiveReadiness {
            queued_depth: 997,
            deferred_ready: true,
            stream_active: true,
        },
        false,
        Some(16),
    );
    session.record_terminal_ready(
        Some(&InteractionId::new("packet2-sustained-stream:action:1")),
        &CauseId::new("native-presentation:cause:11"),
        SchedulingLiveReadiness {
            queued_depth: 64,
            deferred_ready: false,
            stream_active: true,
        },
        true,
        Some(16),
    );
    session.finish().expect("persist sidecar");

    // Then: actions are unique and ordered, backlog is retained, and no user text is stored.
    let bytes = std::fs::read(&path).expect("sidecar bytes");
    let value: serde_json::Value = serde_json::from_slice(&bytes).expect("sidecar JSON");
    assert_eq!(value["maximum_backlog_depth"], 128);
    assert_eq!(value["actual_input_sends"][0]["queued_live_depth"], 127);
    assert_eq!(value["actual_input_sends"][0]["deferred_live_ready"], true);
    assert_eq!(value["actual_input_sends"][0]["stream_active"], true);
    assert_eq!(
        value["actual_input_sends"].as_array().map(Vec::len),
        Some(2)
    );
    assert_eq!(
        value["actual_input_sends"][0]["cause_id"],
        "native-presentation:cause:10"
    );
    let causes = [
        "native-presentation:cause:10",
        "native-presentation:cause:11",
    ];
    assert!(value["actual_input_sends"]
        .as_array()
        .expect("input sends")
        .iter()
        .all(|send| causes.contains(&send["cause_id"].as_str().expect("cause id"))));
    assert!(!String::from_utf8_lossy(&bytes).contains("typed-while-streaming"));
}

#[test]
fn active_stream_with_empty_receiver_does_not_claim_preemption() {
    let root = tempfile::tempdir().expect("evidence root");
    let path = root.path().join("scheduling.json");
    let mut session = SchedulingTelemetrySession::new(path.clone()).expect("session");
    session.record_terminal_ready(
        Some(&InteractionId::new("packet2-sustained-stream:action:0")),
        &CauseId::new("native-presentation:cause:1"),
        SchedulingLiveReadiness {
            stream_active: true,
            ..SchedulingLiveReadiness::default()
        },
        false,
        Some(16),
    );
    session.finish().expect("persist sidecar");

    let value: serde_json::Value =
        serde_json::from_slice(&std::fs::read(path).expect("sidecar bytes")).expect("sidecar JSON");
    assert_eq!(value["actual_input_sends"][0]["live_ready_depth"], 0);
    assert_eq!(value["actual_input_sends"][0]["preempted_live"], false);
    assert_eq!(value["actual_input_sends"][0]["stream_active"], true);
}

#[test]
fn readiness_signal_atomically_reports_only_literal_work() {
    let root = tempfile::tempdir().expect("evidence root");
    let path = root.path().join("readiness.json");
    let mut signal = SchedulingReadinessSignal::new(path.clone()).expect("signal");

    assert!(signal
        .publish_if_changed(SchedulingLiveReadiness {
            stream_active: true,
            ..SchedulingLiveReadiness::default()
        })
        .expect("empty snapshot"));
    let empty: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&path).expect("empty snapshot bytes"))
            .expect("empty snapshot JSON");
    assert_eq!(empty["ready_depth"], 0);
    assert_eq!(empty["sample_sequence"], 1);
    assert_eq!(empty["stream_active"], true);
    assert!(!signal
        .publish_if_changed(SchedulingLiveReadiness {
            stream_active: true,
            ..SchedulingLiveReadiness::default()
        })
        .expect("unchanged snapshot"));

    assert!(signal
        .publish_if_changed(SchedulingLiveReadiness {
            queued_depth: 4,
            deferred_ready: true,
            stream_active: true,
        })
        .expect("ready snapshot"));
    let ready: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&path).expect("ready snapshot bytes"))
            .expect("ready snapshot JSON");
    assert_eq!(ready["ready_depth"], 5);
    assert_eq!(ready["queued_depth"], 4);
    assert_eq!(ready["deferred_ready"], true);
    assert_eq!(ready["sample_sequence"], 2);
    assert!(ready["sampled_at_micros"].is_number());
    assert!(!path.with_extension("tmp").exists());
}
