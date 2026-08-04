#![allow(
    clippy::expect_used,
    clippy::panic,
    clippy::unwrap_used,
    reason = "owner tests use fail-fast assertions for deterministic dispatch fixtures"
)]

use std::cell::Cell;

use harness_tui::attachment_lifecycle::{
    AttachmentIngestor, AttachmentPolicy, CancellationToken, Limits,
};
use harness_tui::completion_controller::{
    CompletionItem, CompletionRange, CompletionSource, CompletionTrigger,
};
use harness_tui::dashboard_dispatch::{
    AttachmentCapability, CoordinatorValidationError, DashboardDispatch, DispatchAction,
    DispatchIntent, TargetCapabilities, TargetIdentity, TargetLifecycle, TargetSnapshot,
};

fn target(id: &str, lifecycle: TargetLifecycle) -> TargetSnapshot {
    TargetSnapshot::new(TargetIdentity::new(id), lifecycle, 1)
}

fn dispatcher_with(snapshot: TargetSnapshot) -> DashboardDispatch {
    let mut dispatcher = DashboardDispatch::new();
    dispatcher
        .register_target(snapshot)
        .expect("target registration");
    dispatcher
        .select_target("session-a")
        .expect("target selection");
    dispatcher
}

#[test]
fn idle_and_working_targets_emit_typed_targeted_actions() {
    // Given: an idle selected session and a separate working session.
    let mut idle = dispatcher_with(target("session-a", TargetLifecycle::Idle));
    idle.insert_text("queued reply").expect("draft text");

    // When: the idle composer queues a reply.
    let queued = idle.queue().expect("queue intent");

    // Then: the intent preserves the stable target identity and action kind.
    assert_eq!(queued.target.session_id(), "session-a");
    assert_eq!(queued.action, DispatchAction::Queue);

    let mut working = DashboardDispatch::new();
    working
        .register_target(target("session-b", TargetLifecycle::Working))
        .expect("target registration");
    working
        .select_target("session-b")
        .expect("target selection");
    working.insert_text("urgent reply").expect("draft text");
    let interjected = working.interject().expect("interject intent");

    assert_eq!(interjected.target.session_id(), "session-b");
    assert_eq!(interjected.action, DispatchAction::Interject);
}

#[test]
fn finished_failed_stale_and_removed_targets_disable_and_reject_drafts() {
    // Given: each terminal dashboard state has a draft captured for dispatch.
    for lifecycle in [
        TargetLifecycle::Finished,
        TargetLifecycle::Failed,
        TargetLifecycle::Stale,
    ] {
        let mut dispatcher = dispatcher_with(target("session-a", TargetLifecycle::Idle));
        dispatcher.insert_text("must not send").expect("draft text");
        dispatcher
            .register_target(target("session-a", lifecycle))
            .expect("terminal transition");

        // When: the selected composer is queried and dispatched.
        assert!(
            dispatcher
                .selected_composer()
                .expect("composer")
                .is_disabled()
        );
        let result = dispatcher.send_now();

        // Then: the stale guard rejects before any coordinator intent is accepted.
        assert!(matches!(
            result,
            Err(harness_tui::dashboard_dispatch::DispatchError::StaleTarget(
                _
            ))
        ));
    }

    let mut removed = dispatcher_with(target("session-a", TargetLifecycle::Idle));
    removed.insert_text("removed target").expect("draft text");
    removed.remove_target("session-a").expect("remove target");
    assert!(removed.selected_composer().expect("composer").is_disabled());
    assert!(matches!(
        removed.send_now(),
        Err(harness_tui::dashboard_dispatch::DispatchError::StaleTarget(
            _
        ))
    ));
}

#[test]
fn drafts_remain_structured_and_independent_per_selected_target() {
    // Given: two eligible dashboard sessions with independent composers.
    let mut dispatcher = DashboardDispatch::new();
    dispatcher
        .register_target(target("session-a", TargetLifecycle::Idle))
        .expect("target registration");
    dispatcher
        .register_target(target("session-b", TargetLifecycle::Idle))
        .expect("target registration");

    // When: each selected target receives a different structured draft.
    dispatcher.select_target("session-a").expect("select a");
    dispatcher.insert_text("first").expect("draft a");
    dispatcher.select_target("session-b").expect("select b");
    dispatcher.insert_text("second").expect("draft b");

    // Then: returning to either target restores only its own atom-backed draft.
    dispatcher
        .select_target("session-a")
        .expect("select a again");
    assert_eq!(
        dispatcher.selected_composer().expect("composer").text(),
        "first"
    );
    dispatcher
        .select_target("session-b")
        .expect("select b again");
    assert_eq!(
        dispatcher.selected_composer().expect("composer").text(),
        "second"
    );
}

#[test]
fn completion_acceptance_reuses_the_shared_controller() {
    // Given: a selected target with a slash completion trigger.
    let mut dispatcher = dispatcher_with(target("session-a", TargetLifecycle::Idle));
    dispatcher.insert_text("/mo").expect("draft text");
    let trigger = CompletionTrigger::new(
        CompletionRange::new(0, 3).expect("ordered range"),
        "/mo",
        CompletionSource::Slash,
    );
    let request = dispatcher
        .begin_completion(trigger)
        .expect("completion request");
    dispatcher
        .apply_completion_results(&request, vec![CompletionItem::new(1, "model", "model")])
        .expect("completion result");

    // When: the shared completion controller accepts the selected result.
    dispatcher.accept_completion_keyboard().expect("completion");

    // Then: the structured editor contains the completion replacement.
    assert_eq!(
        dispatcher.selected_composer().expect("composer").text(),
        "model"
    );
}

#[test]
fn attachment_capability_is_disabled_without_reimplementing_ingestion() {
    // Given: a text attachment produced by the existing lifecycle ingestor.
    let directory = tempfile::tempdir().expect("fixture directory");
    let path = directory.path().join("note.txt");
    std::fs::write(&path, "attachment").expect("fixture bytes");
    let attachment = AttachmentIngestor::new(
        AttachmentPolicy::new(directory.path())
            .expect("fixture policy")
            .with_limits(Limits::default()),
    )
    .ingest_file(&path, &CancellationToken::new())
    .expect("fixture attachment");
    let capabilities =
        TargetCapabilities::interactive().with_attachment_capability(AttachmentCapability::None);
    let snapshot = target("session-a", TargetLifecycle::Idle).with_capabilities(capabilities);
    let mut dispatcher = dispatcher_with(snapshot);

    // When: the composer attempts to attach the ingested file.
    let result = dispatcher.attach(7_u64, attachment);

    // Then: capability gating is explicit, while ingestion remains task-18-owned.
    assert!(matches!(
        result,
        Err(harness_tui::dashboard_dispatch::DispatchError::AttachmentCapability { .. })
    ));

    let allowed_attachment = AttachmentIngestor::new(
        AttachmentPolicy::new(directory.path())
            .expect("fixture policy")
            .with_limits(Limits::default()),
    )
    .ingest_file(&path, &CancellationToken::new())
    .expect("allowed attachment");
    let allowed_capabilities = TargetCapabilities::interactive()
        .with_attachment_capability(AttachmentCapability::ImagesAndText);
    let mut allowed = dispatcher_with(
        target("session-a", TargetLifecycle::Idle).with_capabilities(allowed_capabilities),
    );
    allowed
        .attach(8_u64, allowed_attachment)
        .expect("supported attachment");
    let intent = allowed.send_now().expect("attachment intent");
    assert_eq!(intent.attachments.len(), 1);
    assert_eq!(intent.attachments[0].metadata.mime, "text/plain");
}

#[test]
fn queue_order_and_target_identity_are_preserved_for_coordinator_intents() {
    // Given: one working target whose queue is owned by the dispatch facade.
    let mut dispatcher = dispatcher_with(target("session-a", TargetLifecycle::Working));
    dispatcher.insert_text("one").expect("first draft");
    let first = dispatcher.queue().expect("first queue");
    dispatcher.insert_text("two").expect("second draft");
    let second = dispatcher.queue().expect("second queue");
    dispatcher
        .insert_text("urgent")
        .expect("interjection draft");
    let urgent = dispatcher.interject().expect("interject");

    // Then: FIFO sequence numbers and the stable target ID survive every action.
    assert_eq!(first.sequence + 1, second.sequence);
    assert_eq!(urgent.sequence, second.sequence + 1);
    assert_eq!(
        [
            first.target.session_id(),
            second.target.session_id(),
            urgent.target.session_id()
        ],
        ["session-a", "session-a", "session-a"]
    );
}

struct RecordingCoordinator {
    calls: Cell<usize>,
    reject: bool,
}

impl harness_tui::dashboard_dispatch::CoordinatorValidator for RecordingCoordinator {
    fn validate(&self, _intent: &DispatchIntent) -> Result<(), CoordinatorValidationError> {
        self.calls.set(self.calls.get() + 1);
        if self.reject {
            Err(CoordinatorValidationError::Rejected)
        } else {
            Ok(())
        }
    }
}

#[test]
fn coordinator_validation_is_required_before_a_dispatch_is_accepted() {
    // Given: a valid UI intent and a coordinator validator with no runtime bypass.
    let mut dispatcher = dispatcher_with(target("session-a", TargetLifecycle::Idle));
    dispatcher
        .insert_text("authority check")
        .expect("draft text");
    let coordinator = RecordingCoordinator {
        calls: Cell::new(0),
        reject: false,
    };

    // When: the dashboard sends the intent through the coordinator validation seam.
    let intent = dispatcher
        .dispatch_with(DispatchAction::SendNow, &coordinator)
        .expect("coordinator accepts intent");

    // Then: validation was invoked and the emitted intent still carries the stable target.
    assert_eq!(coordinator.calls.get(), 1);
    assert_eq!(intent.target.session_id(), "session-a");
}
