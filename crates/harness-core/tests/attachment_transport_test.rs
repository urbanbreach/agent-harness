use std::fs;

use harness_core::attachment_transport::{
    checkpoint_attachments, redacted_content_ref, stable_attachment_order, AttachmentDimensions,
    AttachmentMetadata,
};
use harness_core::event::{
    ActorKind, EventActor, EventEnvelopeV1, EventV1, PromptAttachmentsSubmittedEvent,
    UserMessageSubmittedEvent, SCHEMA_VERSION,
};
use harness_core::transcript_projection::{project_transcript, ProjectedMessageRole};

fn metadata(id: &str, path: Option<&std::path::Path>, bytes: &[u8]) -> AttachmentMetadata {
    AttachmentMetadata::from_bytes(
        id,
        "image/png",
        path,
        bytes,
        Some(AttachmentDimensions::new(2, 3)),
    )
}

fn envelope(seq: u64, payload: EventV1) -> EventEnvelopeV1 {
    EventEnvelopeV1 {
        schema_version: SCHEMA_VERSION,
        event_id: format!("evt-{seq}"),
        seq,
        run_id: "run-attachments".into(),
        mono_ms: seq,
        ts: None,
        actor: EventActor::new(ActorKind::User, None),
        correlation_id: Some("req-1".to_string()),
        causation_id: None,
        stream_key: None,
        payload,
    }
}

#[test]
fn attachment_event_round_trips_without_raw_content_or_path() {
    // Given: attachment bytes and a workspace path that must never enter the event.
    let path = std::path::Path::new("/workspace/private/secret.png");
    let first = metadata("first", Some(path), b"secret-pixels");
    let second = metadata("second", None, b"more-secret-pixels");
    let event = EventV1::PromptAttachmentsSubmitted(PromptAttachmentsSubmittedEvent {
        request_id: "req-1".into(),
        attachments: vec![first.clone(), second.clone()],
    });

    // When: the event is serialized and restored through the public schema.
    let encoded = serde_json::to_string(&event).expect("attachment event serializes");
    let decoded: EventV1 = serde_json::from_str(&encoded).expect("attachment event restores");

    // Then: metadata and submission order survive, while source details do not.
    assert!(!encoded.contains("secret-pixels"));
    assert!(!encoded.contains("/workspace/private/secret.png"));
    assert_eq!(decoded, event);
    assert_eq!(first.content_ref.as_str(), redacted_content_ref(Some(path), b"secret-pixels").as_str());
}

#[test]
fn checkpoint_and_projection_preserve_submission_order() {
    // Given: a prompt followed by a stable, ordered attachment event.
    let first = metadata("z-last", None, b"z");
    let second = metadata("a-first", None, b"a");
    let events = vec![
        envelope(
            1,
            EventV1::UserMessageSubmitted(UserMessageSubmittedEvent {
                request_id: "req-1".into(),
                text: "inspect these".to_string(),
            }),
        ),
        envelope(
            2,
            EventV1::PromptAttachmentsSubmitted(PromptAttachmentsSubmittedEvent {
                request_id: "req-1".into(),
                attachments: vec![first.clone(), second.clone()],
            }),
        ),
    ];

    // When: replay derives both a checkpoint and a transcript projection.
    let checkpoint = checkpoint_attachments(&events, 2);
    let projection = project_transcript(&events).expect("attachment projection succeeds");
    let projected_user = projection
        .messages
        .iter()
        .find(|message| message.role == ProjectedMessageRole::User)
        .expect("user message is projected");

    // Then: the original submission order is retained at every replay surface.
    let checkpoint_ids: Vec<_> = checkpoint
        .for_request("req-1")
        .expect("request checkpoint")
        .iter()
        .map(|attachment| attachment.id.as_str())
        .collect();
    let projected_ids: Vec<_> = projected_user
        .attachments
        .iter()
        .map(|attachment| attachment.id.as_str())
        .collect();
    assert_eq!(checkpoint_ids, ["z-last", "a-first"]);
    assert_eq!(projected_ids, ["z-last", "a-first"]);
    assert_eq!(stable_attachment_order(&[first, second]).expect("stable order"), checkpoint.for_request("req-1").expect("request checkpoint"));
}

#[test]
fn replay_checkpoint_does_not_read_deleted_source_files() {
    // Given: metadata was created before its source file was removed.
    let directory = tempfile::tempdir().expect("temporary directory");
    let path = directory.path().join("removed.png");
    fs::write(&path, b"source bytes").expect("source file writes");
    let attachment = metadata("removed", Some(&path), b"source bytes");
    fs::remove_file(&path).expect("source file removes");
    let events = vec![envelope(
        1,
        EventV1::PromptAttachmentsSubmitted(PromptAttachmentsSubmittedEvent {
            request_id: "req-1".into(),
            attachments: vec![attachment.clone()],
        }),
    )];

    // When: replay reads only the append-only metadata checkpoint.
    let checkpoint = checkpoint_attachments(&events, 1);

    // Then: replay remains deterministic after the source disappears.
    assert_eq!(checkpoint.for_request("req-1"), Some(&[attachment] as &[AttachmentMetadata]));
}
