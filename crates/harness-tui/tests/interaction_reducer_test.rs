use crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
use harness_tui::app::interaction_reducer::{
    keyboard_intent, mouse_intent, render_purity::RenderPurityProbe, transition_cases,
    FocusDirection, GestureKind, InteractionState, MouseTarget, OverlayTarget, ScreenMode,
    TransitionOutcome, TransitionTable, UiIntent,
};
use harness_tui::app::{AppState, Focus};
use harness_tui::keybindings::Action;
use harness_tui::render_test::render_to_buffer;
use harness_tui::ui;
use ratatui::layout::Rect;

#[test]
fn every_generated_transition_case_is_checked_by_the_owner() {
    // Given: the generated table is the complete finite state/event contract.
    let table = TransitionTable;

    // When: every generated state/event pair is applied.
    for case in transition_cases() {
        let result = table.apply(case.state(), case.intent());

        // Then: the table's declared outcome is observed, not inferred from the result.
        match case.outcome() {
            TransitionOutcome::Applied => assert!(
                result.is_ok(),
                "expected applied transition for {}: {result:?}",
                case.name()
            ),
            TransitionOutcome::Illegal | TransitionOutcome::Stale => assert!(
                result.is_err(),
                "expected rejected transition for {}: {result:?}",
                case.name()
            ),
        }
    }
}

#[test]
fn illegal_and_stale_transitions_fail_closed() {
    // Given: an empty interaction state.
    let table = TransitionTable;
    let state = InteractionState::new(ScreenMode::Live, Focus::Prompt);

    // When: a close is requested without an overlay.
    let close_empty = table.apply(state.clone(), UiIntent::CloseOverlay(OverlayTarget::Top));

    // Then: the stale close does not manufacture state.
    assert!(close_empty.is_err());

    // When: an overlay is opened twice and a non-top overlay is closed.
    let opened = table
        .apply(
            state,
            UiIntent::OpenOverlay(harness_tui::overlay::OverlayKind::CommandPalette),
        )
        .ok();
    assert!(opened.is_some());
    let Some(opened) = opened else {
        return;
    };
    let duplicate = table.apply(
        opened.clone(),
        UiIntent::OpenOverlay(harness_tui::overlay::OverlayKind::CommandPalette),
    );
    let stale_close = table.apply(
        opened,
        UiIntent::CloseOverlay(OverlayTarget::Kind(
            harness_tui::overlay::OverlayKind::StatusDialog,
        )),
    );

    // Then: both stale requests are rejected.
    assert!(duplicate.is_err());
    assert!(stale_close.is_err());
}

#[test]
fn keyboard_and_mouse_equivalents_emit_the_same_intent() {
    // Given: Tab and a left-click on the next-focus target are equivalent user intent.
    let key = KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE);
    let mouse = MouseEvent {
        kind: MouseEventKind::Down(MouseButton::Left),
        column: 2,
        row: 2,
        modifiers: KeyModifiers::NONE,
    };

    // When: both terminal inputs are normalized.
    let keyboard = keyboard_intent(key);
    let pointer = mouse_intent(mouse, MouseTarget::FocusNext);

    // Then: dispatch is source-independent.
    assert_eq!(keyboard, pointer);
    assert_eq!(keyboard, Some(UiIntent::MoveFocus(FocusDirection::Next)));
}

#[test]
fn render_purity_wraps_the_real_harness_render_path() {
    // Given: a real live AppState and the production renderer.
    let app = AppState::new_live(None, false, None);
    let area = Rect::new(0, 0, 80, 24);
    let mut probe = RenderPurityProbe::default();

    // When: the real render path is invoked through the probe.
    let result = probe.draw(|_| {
        let _buffer = render_to_buffer(&app, area, |app, frame, _| ui::render_app(frame, app));
    });

    // Then: drawing emits no reducer-visible effects.
    assert!(result.is_ok());
    assert!(probe.side_effects().is_empty());

    // When: a renderer attempts to dispatch an action.
    let rejected = probe.draw(|probe| probe.record_action(Action::SubmitPrompt));

    // Then: the probe rejects the observable side effect.
    assert!(rejected.is_err());
}

#[test]
fn gesture_lifecycle_requires_matching_begin_and_end() {
    // Given: an idle state and the reducer table.
    let table = TransitionTable;
    let state = InteractionState::new(ScreenMode::Live, Focus::Details);

    // When: a gesture begins, updates, and ends in order.
    let begun = table
        .apply(
            state,
            UiIntent::BeginGesture(GestureKind::TranscriptSelection),
        )
        .ok();
    let Some(begun) = begun else {
        return;
    };
    let updated = table.apply(
        begun,
        UiIntent::UpdateGesture(GestureKind::TranscriptSelection),
    );
    let ended = updated.and_then(|state| table.apply(state, UiIntent::EndGesture));

    // Then: the lifecycle reaches idle again.
    assert!(ended.is_ok());
    assert!(matches!(
        ended.map(|state| state.gesture),
        Ok(harness_tui::app::interaction_reducer::GestureState::Idle)
    ));
}
