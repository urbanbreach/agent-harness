use std::collections::BTreeMap;

use harness_providers::mock::{request_digest, MockProvider};
use harness_providers::{
    CompletionUsage, Provider, ProviderRequestInitiator, ProviderStreamEvent, ToolChoice,
};
use tokio_stream::StreamExt;

#[path = "support/mock_fixtures.rs"]
mod fixtures;

#[test]
fn mock_ordinary_tool_attachment_and_retry_fixtures_use_protocol_types() {
    let ordinary = fixtures::ordinary_request_fixture();
    let tool = fixtures::tool_request_fixture();
    let attachment = fixtures::attachment_request_fixture();
    let retry = fixtures::physical_retry_request_fixture();

    assert_eq!(ordinary.model_id, "model-fixture");
    assert_eq!(tool.tool_choice, Some(ToolChoice::Auto));
    assert!(attachment.context.has_media);
    assert_ne!(ordinary.context.request_id, retry.context.request_id);
}

#[test]
fn mock_named_usage_fixtures_are_exact() {
    assert_eq!(
        (
            fixtures::ordinary_usage_fixture(),
            fixtures::tool_usage_fixture(),
        ),
        (
            CompletionUsage {
                prompt_tokens: 21,
                completion_tokens: 8,
                total_tokens: 29,
            },
            CompletionUsage {
                prompt_tokens: 34,
                completion_tokens: 13,
                total_tokens: 47,
            },
        )
    );
}

#[test]
fn mock_digest_is_byte_identical_for_the_same_semantic_fixture() {
    let first = request_digest(&fixtures::ordinary_request_fixture()).into_bytes();
    let second = request_digest(&fixtures::ordinary_request_fixture()).into_bytes();

    assert_eq!(first, second);
}

#[test]
fn mock_digest_changes_when_semantic_context_changes() {
    let ordinary = fixtures::ordinary_request_fixture();
    let mut user_initiated = ordinary.clone();
    user_initiated.context.initiator = ProviderRequestInitiator::User;

    assert_ne!(request_digest(&ordinary), request_digest(&user_initiated));
}

#[test]
fn mock_digest_changes_when_attachment_metadata_changes() {
    let first = fixtures::attachment_request_fixture_with("attachment-one", &[1, 2, 3]);
    let second = fixtures::attachment_request_fixture_with("attachment-two", &[1, 2, 3]);

    assert_ne!(request_digest(&first), request_digest(&second));
}

#[test]
fn mock_digest_changes_when_attachment_payload_changes() {
    let first = fixtures::attachment_request_fixture_with("attachment", &[1, 2, 3]);
    let second = fixtures::attachment_request_fixture_with("attachment", &[1, 2, 4]);

    assert_ne!(request_digest(&first), request_digest(&second));
}

#[test]
fn mock_digest_changes_when_tool_payload_changes() {
    let first = fixtures::tool_request_fixture();
    let mut second = first.clone();
    second.tools.as_mut().expect("tool fixture has tools")[0].parameters =
        serde_json::json!({"type": "object", "properties": {}});

    assert_ne!(request_digest(&first), request_digest(&second));
}

#[test]
fn mock_digest_is_preserved_for_a_physical_retry() {
    let ordinary = fixtures::ordinary_request_fixture();
    let retry = fixtures::physical_retry_request_fixture();

    assert_eq!(request_digest(&ordinary), request_digest(&retry));
}

#[test]
fn mock_digest_isolates_root_and_child_session_identities() {
    let root = fixtures::ordinary_request_fixture();
    let child = fixtures::child_request_fixture();

    assert_ne!(request_digest(&root), request_digest(&child));
}

#[tokio::test]
async fn mock_call_accounting_is_atomic_exact_and_request_ordered() {
    let ordinary = fixtures::ordinary_request_fixture();
    let tool = fixtures::tool_request_fixture();
    let mut scripted_events = BTreeMap::new();
    scripted_events.insert(
        request_digest(&ordinary),
        vec![ProviderStreamEvent::Done {
            usage: Some(fixtures::ordinary_usage_fixture()),
        }],
    );
    scripted_events.insert(
        request_digest(&tool),
        vec![ProviderStreamEvent::Done {
            usage: Some(fixtures::tool_usage_fixture()),
        }],
    );
    let provider = MockProvider::new(scripted_events);

    provider
        .stream_completion(ordinary.clone())
        .await
        .collect::<Vec<_>>()
        .await;
    provider
        .stream_completion(tool.clone())
        .await
        .collect::<Vec<_>>()
        .await;

    assert_eq!(provider.call_count(), 2);
    assert_eq!(provider.captured_requests().await, vec![ordinary, tool]);
}
