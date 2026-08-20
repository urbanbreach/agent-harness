use std::collections::VecDeque;

use std::time::Instant;

use harness_tui::scheduling::{BatchBudget, RuntimeArbiter, RuntimeDecision, RuntimeReady};

#[derive(Clone, Debug, PartialEq, Eq)]
enum Interaction {
    Typed(char),
    Wheel,
    Resize(u16, u16),
    DisclosureOpen,
    DisclosureClose,
    Cancel,
}

#[test]
fn sustained_stream_prioritizes_all_terminal_interactions() {
    // arrange
    let mut terminal = "typed-while-streaming"
        .chars()
        .map(Interaction::Typed)
        .collect::<VecDeque<_>>();
    terminal.push_back(Interaction::Wheel);
    for size in [(100, 35), (160, 55), (100, 35), (160, 55), (120, 40)] {
        terminal.push_back(Interaction::Resize(size.0, size.1));
    }
    terminal.push_back(Interaction::DisclosureOpen);
    terminal.push_back(Interaction::DisclosureClose);
    terminal.push_back(Interaction::Cancel);

    let mut arbiter = RuntimeArbiter::default();
    let mut applied = Vec::new();
    let mut live_remaining = 10_000_usize;
    let now = Instant::now();
    let mut input_budget = BatchBudget::input(now);
    while !terminal.is_empty() {
        if input_budget.exhausted(now) {
            arbiter.input_quantum_exhausted();
        }
        let decision = arbiter.decide(RuntimeReady {
            terminal_input: !terminal.is_empty(),
            live_update: live_remaining > 0,
            ..RuntimeReady::default()
        });
        match decision {
            RuntimeDecision::TerminalInput => {
                if let Some(interaction) = terminal.pop_front() {
                    applied.push(interaction);
                }
                input_budget.consume();
            }
            RuntimeDecision::LiveUpdate => {
                live_remaining -= 1;
                input_budget = BatchBudget::input(now);
                arbiter.live_applied();
            }
            _ => {}
        }
    }

    // act
    let typed: String = applied
        .iter()
        .filter_map(|interaction| match interaction {
            Interaction::Typed(character) => Some(*character),
            _ => None,
        })
        .collect();
    // assert
    assert_eq!(typed, "typed-while-streaming");
    assert_eq!(
        applied
            .iter()
            .filter(|item| matches!(item, Interaction::Wheel))
            .count(),
        1
    );
    assert!(matches!(applied.last(), Some(Interaction::Cancel)));
    assert!(live_remaining > 0);
}

#[test]
fn writer_failure_interrupts_active_input_and_live_batches() {
    // arrange
    // act
    let arbiter = RuntimeArbiter::default();
    // assert
    assert_eq!(
        arbiter.decide(RuntimeReady {
            fatal_writer_failure: true,
            terminal_input: true,
            live_update: true,
            ..RuntimeReady::default()
        }),
        RuntimeDecision::FatalWriterFailure
    );
}
