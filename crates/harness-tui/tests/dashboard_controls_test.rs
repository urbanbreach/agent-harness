#![allow(
    clippy::expect_used,
    clippy::panic,
    clippy::unwrap_used,
    reason = "owner tests use fail-fast assertions for typed dashboard control fixtures"
)]

use harness_core::perm::{PermissionDecision, PermissionGrantScope};
use harness_tui::dashboard::{
    DashboardActivity, DashboardEntryEligibility, DashboardGroup, DashboardReadModel,
    DashboardRelationship, DashboardRow, DashboardStatus, SelectionKey,
};
use harness_tui::dashboard_controls::{
    Confirmation, ControlVisual, CoordinatorIntent, CoordinatorOutcome, DashboardControlErrorKind,
    DashboardControlState, PermissionDecisionRequest, QuestionAnswerRequest, answer_question,
    cancel, rename, resolve_permission, settle, stop,
};

fn control_state(status: DashboardStatus) -> DashboardControlState {
    let key = SelectionKey::new("run-dashboard-controls");
    let row = DashboardRow {
        selection_key: key.clone(),
        title: Some("Original title".to_string()),
        status,
        activity: DashboardActivity {
            last_event_seq: 4,
            last_event_id: Some("event-4".to_string()),
            unread_count: 0,
        },
        relationship: DashboardRelationship {
            parent: None,
            children: Vec::new(),
            group: harness_tui::dashboard::DashboardGroupKey::Root(key.clone()),
            lineage_depth: 0,
            parent_missing: false,
            is_parent: false,
            is_child: false,
            is_background: false,
            is_foreign: false,
        },
        eligibility: DashboardEntryEligibility {
            is_eligible: true,
            excluded_by: None,
        },
        creation_seq: 1,
    };
    DashboardControlState::new(
        DashboardReadModel {
            rows: vec![row.clone()],
            all_rows: vec![row],
            groups: vec![DashboardGroup {
                key: harness_tui::dashboard::DashboardGroupKey::Root(key.clone()),
                row_keys: vec![key.clone()],
            }],
        },
        Some(key),
        "draft survives control errors",
    )
}

#[test]
fn rename_and_stop_require_explicit_confirmation_before_pending_intents() {
    // Given: an active selected session with a composer draft.
    let initial = control_state(DashboardStatus::Running);

    // When: rename is previewed, then explicitly confirmed.
    let confirmation = rename(&initial, "Renamed", Confirmation::Required)
        .expect("rename confirmation should be representable");
    let pending = rename(&confirmation.state, "Renamed", Confirmation::Confirmed)
        .expect("confirmed rename should route to the coordinator");

    // Then: confirmation has no effect, while confirmation emits only a pending typed intent.
    assert_eq!(confirmation.visual.state, ControlVisual::Confirming);
    assert!(confirmation.intent.is_none());
    assert_eq!(pending.visual.state, ControlVisual::Pending);
    assert_eq!(pending.state.context.draft, initial.context.draft);
    assert!(matches!(
        pending.intent,
        Some(CoordinatorIntent::RenameSession { ref session_id, ref title })
            if session_id.as_str() == "run-dashboard-controls" && title == "Renamed"
    ));

    // When: stop is requested without confirmation.
    let stop_preview = stop(&initial, Confirmation::Required)
        .expect("destructive stop should expose confirmation state");

    // Then: the destructive command does not emit an effect before confirmation.
    assert_eq!(stop_preview.visual.state, ControlVisual::Confirming);
    assert!(stop_preview.intent.is_none());
}

#[test]
fn duplicate_stop_is_rejected_and_failed_coordinator_response_preserves_context() {
    // Given: a confirmed stop request waiting for the coordinator.
    let initial = control_state(DashboardStatus::Running);
    let pending = stop(&initial, Confirmation::Confirmed).expect("stop should become pending");
    let intent = pending.intent.clone().expect("pending stop intent");

    // When: the same stop is submitted again and the owner reports a failure.
    let duplicate = stop(&pending.state, Confirmation::Confirmed);
    let failed = settle(
        &pending.state,
        &intent,
        CoordinatorOutcome::Failed("coordinator refused stop".to_string()),
    )
    .expect("coordinator failure should become a failure visual");
    let cancel_pending = cancel(&initial, "task-dashboard-controls", Confirmation::Confirmed)
        .expect("cancel should become pending");
    let cancel_duplicate = cancel(
        &cancel_pending.state,
        "task-dashboard-controls",
        Confirmation::Confirmed,
    );

    // Then: duplicate destructive work is rejected, and failure keeps the dashboard context.
    assert!(matches!(
        duplicate,
        Err(error) if matches!(error.kind(), DashboardControlErrorKind::Duplicate(_))
    ));
    assert!(matches!(
        cancel_duplicate,
        Err(error) if matches!(error.kind(), DashboardControlErrorKind::Duplicate(_))
    ));
    assert_eq!(failed.visual.state, ControlVisual::Failure);
    assert_eq!(failed.state.context, pending.state.context);
}

#[test]
fn stale_finished_expired_unauthorized_and_replay_actions_fail_before_intents() {
    // Given: sessions in each state that must not accept a mutating dashboard action.
    let finished = control_state(DashboardStatus::Completed);
    let expired = control_state(DashboardStatus::Running)
        .with_session_expired(SelectionKey::new("run-dashboard-controls"), true);
    let unauthorized = control_state(DashboardStatus::Running)
        .with_session_authorized(SelectionKey::new("run-dashboard-controls"), false);
    let replay = control_state(DashboardStatus::Running).with_replay_mode(true);

    // When: rename is attempted against every forbidden snapshot.
    let finished_error = rename(&finished, "new", Confirmation::Confirmed).expect_err("finished");
    let expired_error = rename(&expired, "new", Confirmation::Confirmed).expect_err("expired");
    let unauthorized_error =
        rename(&unauthorized, "new", Confirmation::Confirmed).expect_err("unauthorized");
    let replay_error = rename(&replay, "new", Confirmation::Confirmed).expect_err("replay");

    // Then: all checks reject before a coordinator intent can exist.
    assert!(matches!(
        finished_error.kind(),
        DashboardControlErrorKind::FinishedSession(_)
    ));
    assert!(matches!(
        expired_error.kind(),
        DashboardControlErrorKind::ExpiredSession(_)
    ));
    assert!(matches!(
        unauthorized_error.kind(),
        DashboardControlErrorKind::UnauthorizedSession(_)
    ));
    assert!(matches!(
        replay_error.kind(),
        DashboardControlErrorKind::ReplayReadOnly
    ));
    assert_eq!(replay_error.visual().state, ControlVisual::Failure);
}

#[test]
fn permission_checks_precede_effects_and_question_answers_are_typed_and_idempotent() {
    // Given: pending coordinator-owned permission and question requests.
    let key = SelectionKey::new("run-dashboard-controls");
    let state = control_state(DashboardStatus::Running)
        .with_pending_permission_for("permission-1", key.clone())
        .with_pending_question_for("question-1", key.clone());
    let unauthorized = state.clone().with_session_authorized(key.clone(), false);

    // When: an unauthorized permission decision is attempted, then a typed question answer is sent.
    let denied_before_effect = resolve_permission(
        &unauthorized,
        PermissionDecisionRequest::new("permission-1", PermissionDecision::Allow)
            .with_grant_scope(PermissionGrantScope::Session),
    )
    .expect_err("permission authority must reject before intent emission");
    let question = answer_question(
        &state,
        QuestionAnswerRequest::new("question-1", vec![vec!["option-a".to_string()]]),
    )
    .expect("question answer should route as a typed intent");
    let question_intent = question.intent.clone().expect("question intent");
    let settled = settle(
        &question.state,
        &question_intent,
        CoordinatorOutcome::Succeeded,
    )
    .expect("question success response");
    let duplicate = answer_question(
        &settled.state,
        QuestionAnswerRequest::new("question-1", vec![vec!["option-a".to_string()]]),
    );

    // Then: the rejected permission remains pending, and duplicate answers are idempotently rejected.
    assert!(matches!(
        denied_before_effect.kind(),
        DashboardControlErrorKind::UnauthorizedSession(_)
    ));
    assert!(unauthorized.has_pending_permission("permission-1"));
    assert!(matches!(
        question_intent,
        CoordinatorIntent::AnswerQuestion { ref session_id, ref permission_id, ref answers }
            if session_id == &key && permission_id == "question-1" && answers == &vec![vec!["option-a".to_string()]]
    ));
    assert_eq!(settled.visual.state, ControlVisual::Success);
    assert!(matches!(
        duplicate,
        Err(error) if matches!(error.kind(), DashboardControlErrorKind::Duplicate(_))
    ));
}

#[test]
fn invalid_input_and_stale_responses_preserve_selection_and_draft() {
    // Given: a selected dashboard row and a draft that must survive failures.
    let state = control_state(DashboardStatus::Running);

    // When: invalid rename input and a response for an unsubmitted intent are received.
    let invalid = rename(&state, "  ", Confirmation::Confirmed).expect_err("blank title");
    let unknown_intent = CoordinatorIntent::StopSession {
        session_id: SelectionKey::new("run-dashboard-controls"),
    };
    let stale_response = settle(&state, &unknown_intent, CoordinatorOutcome::Succeeded)
        .expect_err("unsubmitted response");

    // Then: both failures retain the same context and render the failure visual.
    assert_eq!(invalid.context(), &state.context);
    assert_eq!(invalid.visual().state, ControlVisual::Failure);
    assert_eq!(stale_response.context(), &state.context);
    assert_eq!(stale_response.visual().state, ControlVisual::Failure);
}
