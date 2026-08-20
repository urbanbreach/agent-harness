#![allow(
    clippy::expect_used,
    clippy::panic,
    clippy::unwrap_used,
    reason = "the reachability scenario uses fail-fast deterministic fixtures"
)]

use std::sync::{Arc, Mutex};

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use harness_core::attachment_transport::AttachmentMetadata;
use harness_core::event::{
    ActorKind, EventActor, EventEnvelopeV1, EventV1, PromptAttachmentsSubmittedEvent,
    UserMessageSubmittedEvent,
};
use harness_providers::attachment_protocol::{
    serialize_attachments, AttachmentMetadata as ProviderAttachmentMetadata, AttachmentPayload,
    AttachmentProtocol,
};
use harness_tui::app::{AppState, UiIntent};
use harness_tui::attachment_lifecycle::{AttachmentIngestor, AttachmentPolicy, CancellationToken};
use harness_tui::completion_controller::{
    CompletionItem, CompletionRange, CompletionSource, CompletionTrigger,
};
use harness_tui::composer_atoms::AttachmentId;
use harness_tui::prompt_queue_actions::{CancelStage, QueueAction, QueueLifecycle, QueueState};

fn key(code: KeyCode) -> KeyEvent {
    KeyEvent::new(code, KeyModifiers::NONE)
}

fn text_attachment() -> harness_tui::attachment_lifecycle::Attachment {
    let root = tempfile::tempdir().expect("attachment root");
    AttachmentIngestor::new(AttachmentPolicy::new(root.path()).expect("attachment policy"))
        .ingest_clipboard(b"hello attachment", &CancellationToken::new())
        .expect("clipboard attachment")
}

fn event(seq: u64, payload: EventV1) -> EventEnvelopeV1 {
    EventEnvelopeV1 {
        schema_version: harness_core::event::SCHEMA_VERSION,
        event_id: format!("event-{seq}"),
        run_id: "run-parity-composer".into(),
        seq,
        mono_ms: seq,
        ts: None,
        actor: EventActor::new(ActorKind::User, None),
        correlation_id: None,
        causation_id: None,
        stream_key: None,
        payload,
    }
}

#[test]
fn production_composer_reaches_keyboard_paste_completion_queue_attachment_and_submit() {
    // arrange
    // Given: one production AppState owner and its live UI intent sink.
    let intents = Arc::new(Mutex::new(Vec::<UiIntent>::new()));
    let sink = {
        let intents = Arc::clone(&intents);
        Arc::new(move |intent| intents.lock().expect("intent lock").push(intent))
    };
    let mut app = AppState::new_live(None, false, Some(sink));

    // When: keyboard input and paste edit the live composer.
    app.handle_key(key(KeyCode::Char('h')));
    app.handle_paste("ello");

    // Then: the structured editor is the production render/submission owner.
    assert_eq!(
        app.composer_view_model(harness_tui::design_contract::ViewportId::Standard100x30)
            .text,
        "hello"
    );

    // When: the debounced suggestion controller is advanced and accepted.
    let suggestion = app
        .composer_request_suggestion("hello")
        .expect("suggestion request");
    app.composer_advance_suggestion_clock(100);
    assert_eq!(app.composer_ready_suggestion(), Some(suggestion.clone()));
    app.composer_apply_suggestion_response(&suggestion, " world")
        .expect("suggestion response");
    app.composer_accept_full_suggestion()
        .expect("suggestion acceptance");

    // When: completion is accepted through the live key dispatcher.
    let request = app.composer_begin_completion(CompletionTrigger::new(
        CompletionRange::new(0, 1).expect("completion range"),
        "h",
        CompletionSource::Slash,
    ));
    app.composer_apply_completion_results(&request, vec![CompletionItem::new(1, "hello", "hi")])
        .expect("completion results");
    app.handle_key(key(KeyCode::Enter));

    // When: queue edit/remove/cancellation are applied through the same owner.
    app.composer_apply_queue_action(QueueAction::Queue {
        queued_id: "queued-1".into(),
        text: "follow-up".into(),
    })
    .expect("queue prompt");
    app.composer_apply_queue_action(QueueAction::Edit {
        queued_id: "queued-1".into(),
        text: "edited follow-up".into(),
    })
    .expect("edit queued prompt");
    app.composer_apply_queue_action(QueueAction::Remove {
        queued_id: "queued-1".into(),
    })
    .expect("remove queued prompt");
    app.composer_set_queue_state(QueueState::new(QueueLifecycle::Streaming))
        .expect("streaming queue state");
    app.composer_apply_queue_action(QueueAction::Cancel(CancelStage::Interrupt))
        .expect("cancel active queue");

    // When: an attachment is submitted through Enter, not the test-only facade.
    app.composer_apply_queue_action(QueueAction::Cancel(CancelStage::Kill))
        .expect("kill cancellation stage");
    app.composer_set_queue_state(QueueState::default())
        .expect("reset queue lifecycle");
    app.composer_attach(AttachmentId::new(7), text_attachment())
        .expect("attach payload");
    app.handle_key(key(KeyCode::Char('s')));
    app.handle_key(key(KeyCode::Enter));

    // act
    // Then: the production intent retains attachment bytes and ordering.
    let intents = intents.lock().expect("intent lock");
    let Some(UiIntent::SubmitPrompt { attachments, .. }) = intents.last() else {
        panic!("expected attachment-bearing submit intent, got {intents:?}");
    };
    // assert
    assert_eq!(attachments.len(), 1);
    assert_eq!(attachments[0].bytes, b"hello attachment");
}

#[test]
fn replay_and_provider_transport_preserve_attachment_contract() {
    // arrange
    // Given: append-only user and attachment events with a shared request id.
    let request_id = "request-1".to_string();
    let events = vec![
        event(
            1,
            EventV1::UserMessageSubmitted(UserMessageSubmittedEvent {
                request_id: request_id.clone().into(),
                text: "hello".into(),
            }),
        ),
        event(
            2,
            EventV1::PromptAttachmentsSubmitted(PromptAttachmentsSubmittedEvent {
                request_id: request_id.clone().into(),
                attachments: vec![AttachmentMetadata::from_bytes(
                    "7",
                    "text/plain",
                    None,
                    b"hello attachment",
                    None,
                )],
            }),
        ),
    ];
    let replay_projection = harness_core::transcript_projection::project_transcript(&events)
        .expect("replay projection");

    // Then: replay derives the attachment without executing a provider.
    assert_eq!(replay_projection.messages[0].attachments.len(), 1);

    // When: the same payload crosses the provider attachment serializer.
    let payload = AttachmentPayload::new(
        ProviderAttachmentMetadata::new(
            "7",
            "text/plain",
            16,
            None,
            "attachment:v1:content-redacted",
        ),
        b"hello attachment".to_vec(),
    );
    let serialized = serialize_attachments(&AttachmentProtocol::openai(), &[payload])
        .expect("provider serialization");

    // act
    // Then: wire order and redacted metadata survive the transport boundary.
    // assert
    assert_eq!(serialized[0].id(), "7");
    assert_eq!(serialized[0].text(), Some("hello attachment"));
    assert_eq!(
        serialized[0].metadata().content_ref,
        "attachment:v1:content-redacted"
    );
}
