use harness_tui::presentation::{
    InteractionId, PresentationCauseKind, PresentationOutcome, RenderReason,
};
use harness_tui::runtime_presentation::{InteractionEventClass, PresentationTelemetrySession};

#[test]
fn absent_trace_path_disables_local_sidecar() {
    // Given: ordinary execution without a runner-owned evidence path.
    let session = PresentationTelemetrySession::from_trace_path(None).expect("session setup");

    // When/Then: telemetry remains disabled and no evidence target is invented.
    assert!(session.is_none());
}

#[test]
fn local_sidecar_records_received_cause_and_noop_atomically() {
    // Given: an opt-in trace target beneath an isolated evidence directory.
    let directory = tempfile::tempdir().expect("temporary evidence directory");
    let path = directory.path().join("presentation-trace.json");
    let mut session = PresentationTelemetrySession::from_trace_path(Some(path.clone()))
        .expect("session setup")
        .expect("enabled telemetry");

    // When: a received input requests rendering but produces no visible bytes.
    session.record_visible_cause(
        PresentationCauseKind::TerminalInput,
        RenderReason::TerminalInput,
        None,
    );
    let demand = session.take_render_demand().expect("render demand");
    session
        .record_no_visible_change(&demand)
        .expect("record no-op");
    session.finish().expect("atomic sidecar write");
    let bytes = std::fs::read(&path).expect("read sidecar");
    let trace: harness_tui::presentation::PresentationTrace =
        serde_json::from_slice(&bytes).expect("parse sidecar");

    // Then: the persisted trace closes the cause explicitly without a frame.
    assert_eq!(trace.causes.len(), 1);
    assert!(matches!(
        trace.causes[0].outcome,
        PresentationOutcome::NoVisibleChange { .. }
    ));
    assert!(trace.frames.is_empty());
}

#[test]
fn runner_interaction_ids_are_consumed_at_terminal_receipt() {
    // Given: a runner-owned content-free interaction queue and native trace target.
    let directory = tempfile::tempdir().expect("temporary evidence directory");
    let trace_path = directory.path().join("presentation-trace.json");
    let interaction_path = directory.path().join("interaction-ids");
    std::fs::write(
        &interaction_path,
        concat!(
            r#"{"interaction_id":"scenario:action:0","event_class":"key","receipt_count":1}"#,
            "\n",
            r#"{"interaction_id":"scenario:action:1","event_class":"resize","receipt_count":1}"#,
            "\n",
        ),
    )
    .expect("write interaction queue");
    let mut session =
        PresentationTelemetrySession::from_paths(Some(trace_path), Some(interaction_path))
            .expect("session setup")
            .expect("enabled telemetry");

    // When: an unsolicited event arrives before two matching scripted receipts.
    assert!(session
        .take_interaction_id(InteractionEventClass::Focus)
        .expect("read unsolicited focus")
        .is_none());
    let first = session
        .take_interaction_id(InteractionEventClass::Key)
        .expect("read interaction queue")
        .expect("first interaction identity");
    session.record_visible_cause(
        PresentationCauseKind::TerminalInput,
        RenderReason::TerminalInput,
        Some(first),
    );
    let second = session
        .take_interaction_id(InteractionEventClass::Resize)
        .expect("read interaction queue")
        .expect("second interaction identity");
    session.record_visible_cause(
        PresentationCauseKind::Resize,
        RenderReason::Resize,
        Some(second),
    );

    // Then: the mismatched event cannot shift either scripted interaction identity.
    assert_eq!(
        session.trace().causes[0].interaction_id,
        Some(InteractionId::new("scenario:action:0"))
    );
    assert_eq!(
        session.trace().causes[1].interaction_id,
        Some(InteractionId::new("scenario:action:1"))
    );
    assert!(session
        .take_interaction_id(InteractionEventClass::Key)
        .expect("read exhausted interaction queue")
        .is_none());
}

#[test]
fn interaction_queue_correlates_every_supported_terminal_event_class() {
    let directory = tempfile::tempdir().expect("temporary evidence directory");
    let interaction_path = directory.path().join("interaction-ids");
    std::fs::write(
        &interaction_path,
        concat!(
            r#"{"interaction_id":"scenario:action:0","event_class":"key","receipt_count":1}"#,
            "\n",
            r#"{"interaction_id":"scenario:action:1","event_class":"paste","receipt_count":1}"#,
            "\n",
            r#"{"interaction_id":"scenario:action:2","event_class":"mouse","receipt_count":2}"#,
            "\n",
            r#"{"interaction_id":"scenario:action:3","event_class":"resize","receipt_count":1}"#,
            "\n",
            r#"{"interaction_id":"scenario:action:4","event_class":"focus","receipt_count":1}"#,
            "\n",
        ),
    )
    .expect("write interaction queue");
    let trace_path = directory.path().join("presentation-trace.json");
    let mut session =
        PresentationTelemetrySession::from_paths(Some(trace_path), Some(interaction_path))
            .expect("session setup")
            .expect("enabled telemetry");

    for (event_class, ordinal) in [
        (InteractionEventClass::Key, Some(0)),
        (InteractionEventClass::Paste, Some(1)),
        (InteractionEventClass::Mouse, Some(2)),
        (InteractionEventClass::Mouse, None),
        (InteractionEventClass::Resize, Some(3)),
        (InteractionEventClass::Focus, Some(4)),
    ] {
        assert_eq!(
            session
                .take_interaction_id(event_class)
                .expect("read matching interaction"),
            ordinal.map(|ordinal| InteractionId::new(format!("scenario:action:{ordinal}")))
        );
    }
}
