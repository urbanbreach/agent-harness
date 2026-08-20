use std::time::{Duration, Instant};

use harness_tui::scheduling::{
    BatchBudget, DeferredLiveUpdate, RuntimeArbiter, RuntimeDecision, RuntimeReady,
    INPUT_BATCH_LIMIT, INPUT_BATCH_TIME,
};

#[test]
fn writer_failure_preempts_every_nonfatal_source() {
    // arrange
    // act
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
    // assert
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
    // arrange
    // act
    let mut arbiter = RuntimeArbiter::default();
    let ready = RuntimeReady {
        terminal_input: true,
        pacer_deadline: true,
        live_update: true,
        ..RuntimeReady::default()
    };
    for _ in 0..INPUT_BATCH_LIMIT {
        // assert
        assert_eq!(arbiter.decide(ready), RuntimeDecision::TerminalInput);
    }
    arbiter.input_quantum_exhausted();
    assert_eq!(arbiter.decide(ready), RuntimeDecision::LiveUpdate);
    arbiter.live_applied();
    assert_eq!(arbiter.decide(ready), RuntimeDecision::TerminalInput);
}

#[test]
fn live_drain_stops_before_applying_deferred_update_when_input_arrives() {
    // arrange
    // act
    let mut deferred = DeferredLiveUpdate::default();
    deferred.defer(7_u8).expect("empty deferred slot");
    let arbiter = RuntimeArbiter::default();
    // assert
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
    // arrange
    // act
    let start = Instant::now();
    let mut count = BatchBudget::new(INPUT_BATCH_LIMIT, INPUT_BATCH_TIME, start);
    for _ in 0..INPUT_BATCH_LIMIT {
        count.consume();
    }
    // assert
    assert!(count.exhausted(start));
    let time = BatchBudget::new(INPUT_BATCH_LIMIT, INPUT_BATCH_TIME, start);
    assert!(time.exhausted(start + Duration::from_millis(2)));
}

#[test]
fn continuously_due_pacer_yields_to_live_after_one_deadline() {
    // arrange
    // Given: motion remains due while a provider stream has queued work.
    let mut arbiter = RuntimeArbiter::default();
    let ready = RuntimeReady {
        pacer_deadline: true,
        live_update: true,
        ..RuntimeReady::default()
    };

    // When: one natural deadline is selected and consumed.
    assert_eq!(arbiter.decide(ready), RuntimeDecision::PacerDeadline);
    arbiter.deadline_served();

    // act
    // Then: live work must progress before another continuously due deadline.
    // assert
    assert_eq!(arbiter.decide(ready), RuntimeDecision::LiveUpdate);
    arbiter.live_applied();
    assert_eq!(arbiter.decide(ready), RuntimeDecision::PacerDeadline);
}

#[test]
fn forced_live_turn_preserves_fatal_ack_quit_cancel_and_input_priority() {
    // arrange
    // Given: a due deadline has rotated the arbiter toward queued live work.
    let mut arbiter = RuntimeArbiter::default();
    arbiter.deadline_served();

    // act
    // When/Then: protected sources and input-first remain ahead of that live turn.
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
    // assert
    assert_eq!(arbiter.decide(all), RuntimeDecision::FatalWriterFailure);
    assert_eq!(
        arbiter.decide(RuntimeReady {
            fatal_writer_failure: false,
            ..all
        }),
        RuntimeDecision::FrameAcknowledged
    );
    assert_eq!(
        arbiter.decide(RuntimeReady {
            fatal_writer_failure: false,
            frame_acknowledged: false,
            quit: false,
            cancel: false,
            ..all
        }),
        RuntimeDecision::TerminalInput
    );
}
