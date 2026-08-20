#![allow(
    clippy::expect_used,
    clippy::panic,
    clippy::unwrap_used,
    reason = "owner tests use fail-fast assertions for state-machine fixtures"
)]

use harness_tui::prompt_queue_actions::{
    apply, reject_stale, CancelStage, QueueAction, QueueError, QueueLifecycle, QueueState,
    StaleError,
};

fn item(id: &str, text: &str) -> harness_tui::prompt_queue_actions::QueuedItem {
    harness_tui::prompt_queue_actions::QueuedItem::new(id, text)
}

fn state(lifecycle: QueueLifecycle) -> QueueState {
    QueueState::new(lifecycle)
        .with_draft("keep this draft")
        .with_queued(vec![item("a", "first"), item("b", "second")])
}

#[test]
fn lifecycle_matrix_exposes_busy_and_action_rules() {
    // arrange
    // Given: every coordinator-owned lifecycle represented by the TUI snapshot.
    let lifecycles = [
        QueueLifecycle::Idle,
        QueueLifecycle::Streaming,
        QueueLifecycle::Tool,
        QueueLifecycle::Waiting,
        QueueLifecycle::Cancelling,
        QueueLifecycle::Completed,
        QueueLifecycle::Failed,
    ];

    // When: the composer asks for lifecycle-aware visuals and actions.
    for lifecycle in lifecycles {
        let current = state(lifecycle);
        let visuals = current.visuals();

        // act
        // Then: tool/cancelling are visibly busy, while streaming is the only interject state.
        // assert
        assert_eq!(
            visuals.busy,
            matches!(
                lifecycle,
                QueueLifecycle::Streaming
                    | QueueLifecycle::Tool
                    | QueueLifecycle::Waiting
                    | QueueLifecycle::Cancelling
            )
        );
        assert_eq!(
            apply(
                current.clone(),
                QueueAction::Interject {
                    queued_id: "interjection".to_string(),
                    text: "urgent".to_string(),
                }
            )
            .is_ok(),
            lifecycle == QueueLifecycle::Streaming
        );
        assert_eq!(
            apply(
                current,
                QueueAction::Submit {
                    text: "submit".to_string()
                }
            )
            .is_ok(),
            !matches!(lifecycle, QueueLifecycle::Tool | QueueLifecycle::Cancelling)
        );
    }
}

#[test]
fn queue_mutations_preserve_order_and_current_draft() {
    // arrange
    // Given: an ordered queue and a separate composer draft.
    let current = state(QueueLifecycle::Idle);

    // When: an item is edited, removed, and a new item is queued.
    let edited = apply(
        current.clone(),
        QueueAction::Edit {
            queued_id: "b".to_string(),
            text: "edited second".to_string(),
        },
    )
    .expect("existing item can be edited");
    let removed = apply(
        edited,
        QueueAction::Remove {
            queued_id: "a".to_string(),
        },
    )
    .expect("existing item can be removed");
    let queued = apply(
        removed,
        QueueAction::Queue {
            queued_id: "c".to_string(),
            text: "third".to_string(),
        },
    )
    .expect("new item can be queued");

    // act
    // Then: the surviving order and draft are unchanged by queue mutations.
    // assert
    assert_eq!(queued.queued_ids(), ["b", "c"]);
    assert_eq!(queued.queued[0].text, "edited second");
    assert_eq!(queued.draft, "keep this draft");
}

#[test]
fn stale_ids_are_rejected_without_hidden_mutation() {
    // arrange
    // Given: a snapshot whose queue and draft are captured before a stale request.
    let current = state(QueueLifecycle::Idle);

    // When: a removed/stale identity is checked and used for an edit.
    let stale = reject_stale(&current, "missing");
    let edit = apply(
        current.clone(),
        QueueAction::Edit {
            queued_id: "missing".to_string(),
            text: "must not appear".to_string(),
        },
    );

    // act
    // Then: both paths reject, and the original snapshot remains byte-for-byte equivalent.
    // assert
    assert!(matches!(stale, Err(StaleError::MissingQueuedId(id)) if id == "missing"));
    assert!(matches!(edit, Err(QueueError::Stale(_))));
    assert_eq!(current, state(QueueLifecycle::Idle));
}

#[test]
fn cancel_requires_interrupt_before_kill() {
    // arrange
    // Given: active work and a separate active snapshot with no interrupt yet.
    let running = state(QueueLifecycle::Streaming);

    // When: kill is attempted before interrupt, then interrupt and kill are applied in order.
    let premature = apply(running.clone(), QueueAction::Cancel(CancelStage::Kill));
    let interrupted = apply(running, QueueAction::Cancel(CancelStage::Interrupt))
        .expect("interrupt is the first allowed cancel stage");
    let killed =
        apply(interrupted, QueueAction::Cancel(CancelStage::Kill)).expect("kill follows interrupt");

    // act
    // Then: premature kill is rejected and the accepted sequence is explicit.
    // assert
    assert!(matches!(premature, Err(QueueError::Cancel(_))));
    assert_eq!(killed.cancel_stage, Some(CancelStage::Kill));
    assert_eq!(killed.lifecycle, QueueLifecycle::Cancelling);
}

#[test]
fn interject_is_streaming_only_and_send_now_bypasses_queue() {
    // arrange
    // Given: a streaming snapshot and an idle snapshot with the same queue.
    let streaming = state(QueueLifecycle::Streaming);
    let idle = state(QueueLifecycle::Idle);

    // When: an interjection and a send-now intent are applied.
    let interjected = apply(
        streaming,
        QueueAction::Interject {
            queued_id: "urgent".to_string(),
            text: "interrupting prompt".to_string(),
        },
    )
    .expect("streaming permits interjection");
    let sent_now = apply(
        idle.clone(),
        QueueAction::SendNow {
            text: "bypass queue".to_string(),
        },
    )
    .expect("send-now does not require queue insertion");

    // act
    // Then: interjection is front-ordered, and send-now leaves queue/draft untouched.
    // assert
    assert_eq!(interjected.queued_ids(), ["urgent", "a", "b"]);
    assert!(interjected.queued[0].is_interjection);
    assert_eq!(sent_now.queued, idle.queued);
    assert_eq!(sent_now.draft, idle.draft);
    assert!(apply(
        idle,
        QueueAction::Interject {
            queued_id: "not-streaming".to_string(),
            text: "rejected".to_string(),
        }
    )
    .is_err());
}

#[test]
fn tool_execution_disables_editing_but_keeps_state_unchanged() {
    // arrange
    // Given: a tool-execution snapshot with an existing queue item.
    let current = state(QueueLifecycle::Tool);

    // When: edit/remove are attempted while tool work owns the lifecycle.
    let edited = apply(
        current.clone(),
        QueueAction::Edit {
            queued_id: "a".to_string(),
            text: "must be rejected".to_string(),
        },
    );
    let removed = apply(
        current.clone(),
        QueueAction::Remove {
            queued_id: "a".to_string(),
        },
    );

    // act
    // Then: the visuals and failures agree, with no hidden mutation.
    // assert
    assert!(!current.visuals().editing_enabled);
    assert!(matches!(edited, Err(QueueError::Busy { .. })));
    assert!(matches!(removed, Err(QueueError::Busy { .. })));
    assert_eq!(current, state(QueueLifecycle::Tool));
}
