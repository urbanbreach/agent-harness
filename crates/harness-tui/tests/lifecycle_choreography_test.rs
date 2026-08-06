use harness_tui::design_contract::LifecycleState;
use harness_tui::lifecycle_choreography::*;

#[test]
fn transitions_allow_self_loops_and_documented_edges() {
    for state in LifecycleState::ALL {
        assert!(TransitionTable::is_valid(state, state));
    }
    let edges = [
        (LifecycleState::Idle, LifecycleState::Drafting),
        (LifecycleState::Idle, LifecycleState::Submitting),
        (LifecycleState::Idle, LifecycleState::Compacting),
        (LifecycleState::Drafting, LifecycleState::Submitting),
        (LifecycleState::Drafting, LifecycleState::Idle),
        (LifecycleState::Submitting, LifecycleState::Streaming),
        (LifecycleState::Submitting, LifecycleState::Failed),
        (LifecycleState::Submitting, LifecycleState::Idle),
        (LifecycleState::Streaming, LifecycleState::Thinking),
        (LifecycleState::Streaming, LifecycleState::Tool),
        (LifecycleState::Streaming, LifecycleState::Diff),
        (LifecycleState::Streaming, LifecycleState::Permission),
        (LifecycleState::Streaming, LifecycleState::Question),
        (LifecycleState::Streaming, LifecycleState::Completed),
        (LifecycleState::Streaming, LifecycleState::Failed),
        (LifecycleState::Streaming, LifecycleState::Cancelling),
        (LifecycleState::Streaming, LifecycleState::Recovering),
        (LifecycleState::Streaming, LifecycleState::Compacting),
        (LifecycleState::Thinking, LifecycleState::Streaming),
        (LifecycleState::Thinking, LifecycleState::Tool),
        (LifecycleState::Thinking, LifecycleState::Permission),
        (LifecycleState::Thinking, LifecycleState::Question),
        (LifecycleState::Tool, LifecycleState::Streaming),
        (LifecycleState::Tool, LifecycleState::Diff),
        (LifecycleState::Tool, LifecycleState::Permission),
        (LifecycleState::Tool, LifecycleState::Question),
        (LifecycleState::Tool, LifecycleState::Completed),
        (LifecycleState::Tool, LifecycleState::Failed),
        (LifecycleState::Diff, LifecycleState::Streaming),
        (LifecycleState::Diff, LifecycleState::Tool),
        (LifecycleState::Diff, LifecycleState::Completed),
        (LifecycleState::Permission, LifecycleState::Streaming),
        (LifecycleState::Permission, LifecycleState::Tool),
        (LifecycleState::Permission, LifecycleState::Cancelling),
        (LifecycleState::Permission, LifecycleState::Failed),
        (LifecycleState::Question, LifecycleState::Streaming),
        (LifecycleState::Question, LifecycleState::Tool),
        (LifecycleState::Question, LifecycleState::Permission),
        (LifecycleState::Queued, LifecycleState::Submitting),
        (LifecycleState::Queued, LifecycleState::Idle),
        (LifecycleState::Interjected, LifecycleState::Streaming),
        (LifecycleState::Interjected, LifecycleState::Cancelling),
        (LifecycleState::Cancelling, LifecycleState::Idle),
        (LifecycleState::Cancelling, LifecycleState::Failed),
        (LifecycleState::Recovering, LifecycleState::Streaming),
        (LifecycleState::Recovering, LifecycleState::Idle),
        (LifecycleState::Recovering, LifecycleState::Failed),
        (LifecycleState::Failed, LifecycleState::Idle),
        (LifecycleState::Failed, LifecycleState::Recovering),
        (LifecycleState::Failed, LifecycleState::Drafting),
        (LifecycleState::Completed, LifecycleState::Idle),
        (LifecycleState::Completed, LifecycleState::Drafting),
        (LifecycleState::Completed, LifecycleState::Compacting),
        (LifecycleState::Compacting, LifecycleState::Idle),
        (LifecycleState::Compacting, LifecycleState::Streaming),
    ];
    for (from, to) in edges {
        assert!(TransitionTable::is_valid(from, to));
    }
}

#[test]
fn transitions_reject_impossible_edges() {
    assert!(!TransitionTable::is_valid(
        LifecycleState::Idle,
        LifecycleState::Completed
    ));
    assert!(!TransitionTable::is_valid(
        LifecycleState::Failed,
        LifecycleState::Streaming
    ));
    assert!(!TransitionTable::is_valid(
        LifecycleState::Completed,
        LifecycleState::Streaming
    ));
}

#[test]
fn valid_targets_include_idle_edges() {
    let targets = TransitionTable::valid_targets(LifecycleState::Idle);
    assert!(targets.contains(&LifecycleState::Drafting));
    assert!(targets.contains(&LifecycleState::Submitting));
    assert!(targets.contains(&LifecycleState::Compacting));
}

#[test]
fn authority_validates_and_runs_full_lifecycle() {
    let mut authority = LifecycleAuthority::new();
    assert_eq!(authority.snapshot().state, LifecycleState::Idle);
    assert!(authority.transition(LifecycleState::Drafting).is_ok());
    assert!(authority.transition(LifecycleState::Completed).is_err());
    for state in [
        LifecycleState::Submitting,
        LifecycleState::Streaming,
        LifecycleState::Thinking,
        LifecycleState::Streaming,
        LifecycleState::Tool,
        LifecycleState::Streaming,
        LifecycleState::Completed,
        LifecycleState::Idle,
    ] {
        assert!(authority.transition(state).is_ok());
    }
}

#[test]
fn authority_tracks_counts_and_rest_invariant() {
    let mut authority = LifecycleAuthority::new();
    assert!(authority.snapshot().rest_frame());
    authority.set_pending_permissions(1);
    authority.set_queued_prompts(2);
    authority.set_recovering(true);
    authority.tick();
    assert_eq!(authority.snapshot().pending_permissions, 1);
    assert_eq!(authority.snapshot().queued_prompts, 2);
    assert!(authority.snapshot().recovering);
    assert_eq!(authority.snapshot().tick, 1);
    assert!(!authority.snapshot().rest_frame());
    assert!(!authority.snapshot().is_composite_impossible());
}

#[test]
fn surface_states_drive_visible_controls() {
    let idle = SurfaceState::from_state(LifecycleState::Idle);
    assert_eq!(idle.composer_enabled, ActionAvailability::Enabled);
    assert_eq!(idle.transcript_focus, FocusDirective::Composer);
    assert!(!idle.any_prompt_visible());

    let streaming = SurfaceState::from_state(LifecycleState::Streaming);
    assert_eq!(streaming.composer_enabled, ActionAvailability::Disabled);
    assert_eq!(streaming.transcript_focus, FocusDirective::Transcript);
    assert_eq!(streaming.cancel_available, ActionAvailability::Enabled);

    let permission = SurfaceState::from_state(LifecycleState::Permission);
    assert!(permission.permission_visible);
    assert_eq!(
        permission.transcript_focus,
        FocusDirective::PermissionPrompt
    );

    let question = SurfaceState::from_state(LifecycleState::Question);
    assert!(question.question_visible);
    assert_eq!(question.transcript_focus, FocusDirective::QuestionPrompt);

    let completed = SurfaceState::from_state(LifecycleState::Completed);
    assert_eq!(completed.composer_enabled, ActionAvailability::Enabled);
    assert_eq!(completed.cancel_available, ActionAvailability::Hidden);
    assert!(completed.cursor_visible());
    assert!(permission.any_prompt_visible());
    assert!(question.any_prompt_visible());
    assert!(SurfaceState::from_state(LifecycleState::Idle).rest_mid_settled());
    assert!(completed.rest_mid_settled());
    assert!(SurfaceState::from_state(LifecycleState::Failed).rest_mid_settled());
}
