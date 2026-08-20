use harness_providers::attachment_protocol::{
    serialize_attachments, AttachmentCapability, AttachmentMetadata, AttachmentPayload,
    AttachmentProtocol, SerializedAttachment,
};

fn payload(id: &str, mime: &str, bytes: &[u8]) -> AttachmentPayload {
    AttachmentPayload::new(
        AttachmentMetadata::new(
            id,
            mime,
            bytes.len() as u64,
            None,
            format!("attachment://content-{id}"),
        ),
        bytes.to_vec(),
    )
}

#[test]
fn supported_media_serializes_in_submission_order() {
    // arrange
    // Given: supported PNG, JPEG, and UTF-8 text payloads.
    let attachments = vec![
        payload("png", "image/png", &[137, 80, 78, 71]),
        payload("jpeg", "image/jpeg", &[255, 216, 255]),
        payload("text", "text/plain", b"hello attachment"),
    ];

    // act
    // When: the opted-in OpenAI attachment protocol serializes them.
    let serialized = serialize_attachments(&AttachmentProtocol::openai(), &attachments)
        .expect("supported media serializes");

    // assert
    // Then: wire content is supported, redaction metadata is not replaced, and order is stable.
    assert_eq!(
        serialized
            .iter()
            .map(SerializedAttachment::id)
            .collect::<Vec<_>>(),
        ["png", "jpeg", "text"]
    );
    assert_eq!(
        serialized[0].data_url(),
        Some("data:image/png;base64,iVBORw==")
    );
    assert_eq!(
        serialized[1].data_url(),
        Some("data:image/jpeg;base64,/9j/")
    );
    assert_eq!(serialized[2].text(), Some("hello attachment"));
}

#[test]
fn unsupported_provider_rejects_before_serialization() {
    // arrange
    // Given: a provider that explicitly opts out of all attachment capabilities.
    let protocol = AttachmentProtocol::new(AttachmentCapability::None);
    let attachments = vec![payload("png", "image/png", &[1, 2, 3])];

    // act
    // When: a request attempts to carry an attachment.
    let error = serialize_attachments(&protocol, &attachments).expect_err("capability must reject");

    // assert
    // Then: the typed rejection identifies capability, not a transport/network failure.
    assert!(matches!(
        error,
        harness_providers::attachment_protocol::AttachmentProtocolError::UnsupportedCapability {
            attachment_id,
            mime
        } if attachment_id == "png" && mime == "image/png"
    ));
}

#[test]
fn serialized_request_shape_never_persists_attachment_bytes() {
    // arrange
    // Given: an in-process payload containing bytes and a redacted reference.
    let payload = payload("text", "text/plain", b"secret text");

    // act
    // When: provider request metadata is serialized for a cassette/digest boundary.
    let encoded = serde_json::to_string(&payload).expect("payload metadata serializes");

    // assert
    // Then: the reference remains while raw content is excluded.
    assert!(encoded.contains("attachment://content-text"));
    assert!(!encoded.contains("secret text"));
}
