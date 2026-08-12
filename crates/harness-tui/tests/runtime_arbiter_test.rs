use std::time::{Duration, Instant};

use harness_tui::scheduling::{
    BatchBudget, DeferredLiveUpdate, RuntimeArbiter, RuntimeDecision, RuntimeReady,
    INPUT_BATCH_LIMIT, INPUT_BATCH_TIME,
};

#[test]
fn writer_failure_preempts_every_nonfatal_source() {
    let arbiter = RuntimeArbiter::default();
    let all = RuntimeReady {
        fatal_writer_failure: true,
        frame_acknowledged: true,
        quit: true,
        cancel: true,
        terminal_input: true,
        pacer_deadline: true,
        animation_deadline: true,
        live_update: true,
    };
    assert_eq!(arbiter.decide(all), RuntimeDecision::FatalWriterFailure);
    assert_eq!(
        arbiter.decide(RuntimeReady {
            fatal_writer_failure: false,
            ..all
        }),
        RuntimeDecision::FrameAcknowledged
    );
}

#[test]
fn sustained_sources_obey_input_first_fairness_bound() {
    let mut arbiter = RuntimeArbiter::default();
    let ready = RuntimeReady {
        terminal_input: true,
        live_update: true,
        ..RuntimeReady::default()
    };
    for _ in 0..INPUT_BATCH_LIMIT {
        assert_eq!(arbiter.decide(ready), RuntimeDecision::TerminalInput);
    }
    arbiter.input_quantum_exhausted();
    assert_eq!(arbiter.decide(ready), RuntimeDecision::LiveUpdate);
    arbiter.live_applied();
    assert_eq!(arbiter.decide(ready), RuntimeDecision::TerminalInput);
}

#[test]
fn live_drain_stops_before_applying_deferred_update_when_input_arrives() {
    let mut deferred = DeferredLiveUpdate::default();
    deferred.defer(7_u8).expect("empty deferred slot");
    let arbiter = RuntimeArbiter::default();
    assert_eq!(
        arbiter.decide(RuntimeReady {
            terminal_input: true,
            live_update: true,
            ..RuntimeReady::default()
        }),
        RuntimeDecision::TerminalInput
    );
    assert_eq!(deferred.take(), Some(7));
}

#[test]
fn batch_budget_honors_count_and_time_boundaries() {
    let start = Instant::now();
    let mut count = BatchBudget::new(INPUT_BATCH_LIMIT, INPUT_BATCH_TIME, start);
    for _ in 0..INPUT_BATCH_LIMIT {
        count.consume();
    }
    assert!(count.exhausted(start));
    let time = BatchBudget::new(INPUT_BATCH_LIMIT, INPUT_BATCH_TIME, start);
    assert!(time.exhausted(start + Duration::from_millis(2)));
}
