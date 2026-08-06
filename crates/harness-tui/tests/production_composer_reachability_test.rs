use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use harness_tui::app::AppState;
use harness_tui::attachment_lifecycle::{AttachmentIngestor, AttachmentPolicy, CancellationToken};
use harness_tui::completion_controller::{
    CompletionItem, CompletionRange, CompletionSource, CompletionTrigger,
};
use harness_tui::composer_atoms::AttachmentId;
use harness_tui::design_contract::ViewportId;
use harness_tui::prompt_queue_actions::QueueAction;

#[test]
fn production_app_state_routes_keyboard_input_through_atom_composer() {
    // Given: a live AppState with the production key dispatcher.
    let mut app = AppState::new_live(None, false, None);

    // When: a user types through the real AppState boundary.
    app.handle_key(KeyEvent::new(KeyCode::Char('h'), KeyModifiers::NONE));
    app.handle_key(KeyEvent::new(KeyCode::Char('i'), KeyModifiers::NONE));

    // Then: the production view model exposes grapheme-backed composer state and submission data.
    let view = app.composer_view_model(ViewportId::Standard100x30);
    assert_eq!(view.text, "hi");
    assert_eq!(view.wrapped_atom_ids().len(), 2);

    let submission = app
        .composer_submission()
        .expect("valid production composer submission");
    assert_eq!(submission.text, "hi");
    assert!(submission.attachments.is_empty());
}

#[test]
fn production_app_state_routes_completion_and_queue_actions() {
    // Given: a live production composer with a completion request and an idle queue.
    let mut app = AppState::new_live(None, false, None);
    let request = app.composer_begin_completion(CompletionTrigger::new(
        CompletionRange::new(0, 0).expect("valid completion range"),
        "",
        CompletionSource::Slash,
    ));
    app.composer_apply_completion_results(
        &request,
        vec![CompletionItem::new(1, "status", "status")],
    )
    .expect("current completion results apply");

    // When: Enter accepts the active completion and the queue is edited through AppState.
    app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
    app.composer_apply_queue_action(QueueAction::Queue {
        queued_id: "queued-1".to_owned(),
        text: "queued text".to_owned(),
    })
    .expect("queue action applies");
    app.composer_apply_queue_action(QueueAction::Edit {
        queued_id: "queued-1".to_owned(),
        text: "edited text".to_owned(),
    })
    .expect("queue edit applies");

    // Then: completion output and queue state are owned by the production composer.
    assert_eq!(
        app.composer_view_model(ViewportId::Standard100x30).text,
        "status"
    );
    assert_eq!(app.composer_queue_state().queued[0].text, "edited text");
}

#[test]
fn production_app_state_routes_attachment_ingest_and_submit() {
    // Given: a bounded attachment ingestor and a real AppState composer.
    let root = tempfile::tempdir().expect("temporary workspace");
    let policy = AttachmentPolicy::new(root.path()).expect("workspace policy");
    let attachment = AttachmentIngestor::new(policy)
        .ingest_clipboard(b"hello attachment", &CancellationToken::new())
        .expect("clipboard attachment");
    let mut app = AppState::new_live(None, false, None);

    // When: the attachment is inserted into the production composer and submitted.
    app.composer_attach(AttachmentId::new(7), attachment)
        .expect("attachment attaches");
    let submission = app.composer_submission().expect("attachment submission");

    // Then: attachment identity and bytes survive the typed production submission boundary.
    assert_eq!(submission.attachments.len(), 1);
    assert_eq!(submission.attachments[0].id, AttachmentId::new(7));
    assert_eq!(submission.attachments[0].bytes, b"hello attachment");
}
