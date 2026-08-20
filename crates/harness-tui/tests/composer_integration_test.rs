#![allow(
    clippy::expect_used,
    clippy::panic,
    clippy::unwrap_used,
    reason = "owner tests use fail-fast assertions for deterministic composer fixtures"
)]

use std::fs;

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
use harness_tui::app::interaction_reducer::{
    keyboard_intent, InteractionState, ScreenMode, UiIntent as InteractionIntent,
};
use harness_tui::attachment_lifecycle::{
    AttachmentIngestor, AttachmentPolicy, CancellationToken, Limits,
};
use harness_tui::completion_controller::{
    CompletionItem, CompletionRange, CompletionSource, CompletionTrigger,
};
use harness_tui::composer_atoms::{AtomId, AtomKind, AttachmentId};
use harness_tui::composer_integration::{
    ComposerHitTarget, ComposerSlice, ComposerUiIntent, UiIntent, ViewportId,
};
use harness_tui::prompt_queue_actions::{QueueLifecycle, QueueState, QueuedItem};
use ratatui::style::Modifier;

const VIEWPORTS: [ViewportId; 7] = ViewportId::ALL;

fn trigger() -> CompletionTrigger {
    CompletionTrigger::new(
        CompletionRange::new(5, 8).expect("fixture range is ordered"),
        "/mo",
        CompletionSource::Slash,
    )
}

fn text_attachment() -> harness_tui::attachment_lifecycle::Attachment {
    let directory = tempfile::tempdir().expect("fixture directory");
    let path = directory.path().join("note.txt");
    fs::write(&path, "attachment preview").expect("fixture bytes");
    let ingestor = AttachmentIngestor::new(
        AttachmentPolicy::new(directory.path())
            .expect("fixture policy")
            .with_limits(Limits::default()),
    );
    ingestor
        .ingest_file(&path, &CancellationToken::new())
        .expect("fixture attachment")
}

#[test]
fn atoms_and_editing_share_one_identity_safe_composer_state() {
    // arrange
    let mut slice = ComposerSlice::from_text("e\u{301}界🙂");
    let original: Vec<AtomId> = slice
        .editor()
        .buffer()
        .atoms()
        .iter()
        .map(|atom| atom.id)
        .collect();

    // act
    slice.insert_text("!").expect("text insertion");

    // assert
    assert_eq!(slice.editor().text(), "e\u{301}界🙂!");
    let retained: Vec<AtomId> = slice
        .editor()
        .buffer()
        .atoms()
        .iter()
        .take(original.len())
        .map(|atom| atom.id)
        .collect();
    assert_eq!(retained, original);
    assert!(matches!(
        slice.editor().buffer().atoms()[0].kind,
        AtomKind::Text(_)
    ));
}

#[test]
fn completion_dropdown_follows_composer_anchor_at_every_viewport() {
    // arrange
    let mut slice = ComposerSlice::from_text("say /mo");
    let request = slice.begin_completion(trigger());
    slice
        .apply_completion_results(&request, vec![CompletionItem::new(1, "model", "model")])
        .expect("current completion results");

    // act
    for viewport in VIEWPORTS {
        let view = slice.view_model(viewport);
        let dropdown = view.completion.expect("ready dropdown");
        // assert
        assert!(dropdown.geometry.rect.right() <= view.viewport.width);
        assert!(dropdown.geometry.rect.bottom() <= view.viewport.height);
        assert!(dropdown.geometry.anchor.x < view.viewport.width);
        assert!(dropdown.geometry.anchor.y < view.viewport.height);
    }
}

#[test]
fn ghost_suggestion_is_muted_and_clears_on_edit_invalidation() {
    // arrange
    let mut slice = ComposerSlice::from_text("inspect ");
    let request = slice.request_suggestion("inspect ").expect("request");
    slice.advance_flush(100);
    assert_eq!(slice.ready_suggestion(), Some(request.clone()));
    slice
        .apply_suggestion_response(&request, "the workspace")
        .expect("current suggestion");

    let ghost = slice
        .view_model(ViewportId::Default80x24)
        .ghost
        .expect("ghost");
    assert_eq!(ghost.text, "the workspace");
    assert!(ghost.style.add_modifier.contains(Modifier::DIM));

    // act
    slice.insert_text("now").expect("edit invalidation");
    // assert
    assert!(slice.view_model(ViewportId::Default80x24).ghost.is_none());
}

#[test]
fn attachment_preview_and_queue_badges_cover_lifecycle_states() {
    // arrange
    // act
    let mut slice = ComposerSlice::new();
    slice
        .attach(AttachmentId::new(7), text_attachment())
        .expect("attachment insertion");
    for lifecycle in [
        QueueLifecycle::Idle,
        QueueLifecycle::Streaming,
        QueueLifecycle::Tool,
        QueueLifecycle::Waiting,
        QueueLifecycle::Cancelling,
        QueueLifecycle::Completed,
        QueueLifecycle::Failed,
    ] {
        slice
            .set_queue_state(
                QueueState::new(lifecycle).with_queued(vec![QueuedItem::new("q1", "queued")]),
            )
            .expect("queue state invalidation");
        for viewport in VIEWPORTS {
            let view = slice.view_model(viewport);
            // assert
            assert_eq!(view.lifecycle, lifecycle);
            assert_eq!(view.attachments.len(), 1);
            assert!(view.queue_badges.iter().any(|badge| badge == "q1"));
        }
    }
}

#[test]
fn submission_is_typed_reducer_intent_without_mutating_composer_state() {
    // arrange
    let slice = ComposerSlice::from_text("send this");
    let before = slice.editor().state();

    // act
    let intent: UiIntent = slice.submit().expect("submission intent");

    // assert
    assert_eq!(slice.editor().state(), before);
    assert_eq!(intent.text, "send this");
    assert!(matches!(
        intent.interaction,
        InteractionIntent::DispatchAction(harness_tui::keybindings::Action::SubmitPrompt)
    ));
    assert!(matches!(intent, ComposerUiIntent { .. }));
}

#[test]
fn hit_map_routes_shell_completion_and_attachment_targets() {
    // arrange
    let mut slice = ComposerSlice::from_text("say /mo");
    slice
        .attach(AttachmentId::new(7), text_attachment())
        .expect("attachment insertion");
    let request = slice.begin_completion(trigger());
    slice
        .apply_completion_results(&request, vec![CompletionItem::new(1, "model", "model")])
        .expect("completion results");

    // act
    let map = slice.hit_map(ViewportId::Default80x24);
    let composer = map.composer_rect;
    // assert
    assert_eq!(
        map.hit_test(composer.x, composer.y),
        Some(ComposerHitTarget::Shell(
            harness_tui::shell_geometry::HitTarget::Composer
        ))
    );
    assert!(map
        .regions
        .iter()
        .any(|region| { matches!(region.target, ComposerHitTarget::Completion(_)) }));
    assert!(map.regions.iter().any(|region| {
        matches!(
            region.target,
            ComposerHitTarget::Attachment(id) if id == AttachmentId::new(7)
        )
    }));
    let click = MouseEvent {
        kind: MouseEventKind::Down(MouseButton::Left),
        column: composer.x,
        row: composer.y,
        modifiers: KeyModifiers::NONE,
    };
    assert!(map.intent_for(click).is_some());
}

#[test]
fn responsive_wrapping_preserves_atom_identity_and_focus_bounds() {
    // arrange
    let slice = ComposerSlice::from_text("wide 界🙂 text with e\u{301} and more");
    let ids: Vec<AtomId> = slice
        .editor()
        .buffer()
        .atoms()
        .iter()
        .map(|atom| atom.id)
        .collect();

    for viewport in VIEWPORTS {
        let view = slice.view_model(viewport);
        assert_eq!(view.wrapped_atom_ids(), ids);
        assert!(view.cursor.position.0 < view.viewport.width);
        assert!(view.cursor.position.1 < view.viewport.height);
        assert!(!view.cursor.clipped);
    }

    // act
    let state = InteractionState::new(ScreenMode::Live, harness_tui::app::Focus::Prompt);
    // assert
    assert_eq!(
        keyboard_intent(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE)),
        Some(InteractionIntent::DispatchAction(
            harness_tui::keybindings::Action::SubmitPrompt
        ))
    );
    assert_eq!(slice.interaction_state(), &state);
}
