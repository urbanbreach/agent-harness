use std::collections::BTreeMap;

use harness_providers::anthropic::{
    build_anthropic_request, AnthropicProvider, AnthropicProviderConfig,
};
use harness_providers::mock::{request_digest, MockProvider};
use harness_providers::{
    Provider, ProviderOutputCapDisposition, ProviderRequestInitiator, ProviderStreamEvent,
};
use tokio_stream::StreamExt;

#[path = "support/mock_fixtures.rs"]
mod fixtures;

#[test]
fn anthropic_requires_explicit_output_budget() {
    // arrange
    let mut request = fixtures::ordinary_request_fixture();
    request.max_tokens = None;
    let provider = AnthropicProvider::new(AnthropicProviderConfig {
        base_url: "https://example.test".to_string(),
        api_key: "test-key".to_string(),
        timeout_ms: 1,
        headers: BTreeMap::new(),
    })
    .expect("Anthropic provider");

    // act
    let semantics = provider
        .request_budget_semantics(&request, 0)
        .expect("Anthropic request budget semantics");
    let body = build_anthropic_request(&request);

    // assert
    assert_eq!(request.max_tokens, None);
    assert_eq!(body["max_tokens"], 4_096);
    assert_eq!(
        semantics.output_cap_disposition,
        ProviderOutputCapDisposition::ProviderDefaulted(4_096)
    );
}

#[test]
fn mock_digest_is_byte_identical_for_the_same_semantic_fixture() {
    // arrange
    let first = request_digest(&fixtures::ordinary_request_fixture()).into_bytes();

    // act
    let second = request_digest(&fixtures::ordinary_request_fixture()).into_bytes();

    // assert
    assert_eq!(first, second);
}

#[test]
fn mock_digest_changes_when_semantic_context_changes() {
    // arrange
    let ordinary = fixtures::ordinary_request_fixture();
    let mut user_initiated = ordinary.clone();
    user_initiated.context.initiator = ProviderRequestInitiator::User;

    // act
    let digests = (request_digest(&ordinary), request_digest(&user_initiated));

    // assert
    assert_ne!(digests.0, digests.1);
}

#[test]
fn mock_digest_changes_when_attachment_metadata_changes() {
    // arrange
    let first = fixtures::attachment_request_fixture_with("attachment-one", &[1, 2, 3]);
    let second = fixtures::attachment_request_fixture_with("attachment-two", &[1, 2, 3]);

    // act
    let digests = (request_digest(&first), request_digest(&second));

    // assert
    assert_ne!(digests.0, digests.1);
}

#[test]
fn mock_digest_changes_when_attachment_payload_changes() {
    // arrange
    let first = fixtures::attachment_request_fixture_with("attachment", &[1, 2, 3]);
    let second = fixtures::attachment_request_fixture_with("attachment", &[1, 2, 4]);

    // act
    let digests = (request_digest(&first), request_digest(&second));

    // assert
    assert_ne!(digests.0, digests.1);
}

#[test]
fn mock_digest_changes_when_tool_payload_changes() {
    // arrange
    let first = fixtures::tool_request_fixture();
    let mut second = first.clone();
    second.tools.as_mut().expect("tool fixture has tools")[0].parameters =
        serde_json::json!({"type": "object", "properties": {}});

    // act
    let digests = (request_digest(&first), request_digest(&second));

    // assert
    assert_ne!(digests.0, digests.1);
}

#[test]
fn mock_digest_is_preserved_for_a_physical_retry() {
    // arrange
    let ordinary = fixtures::ordinary_request_fixture();
    let retry = fixtures::physical_retry_request_fixture();

    // act
    let digests = (request_digest(&ordinary), request_digest(&retry));

    // assert
    assert_eq!(digests.0, digests.1);
}

#[test]
fn mock_digest_isolates_root_and_child_session_identities() {
    // arrange
    let root = fixtures::ordinary_request_fixture();
    let child = fixtures::child_request_fixture();

    // act
    let digests = (request_digest(&root), request_digest(&child));

    // assert
    assert_ne!(digests.0, digests.1);
}

#[tokio::test]
async fn mock_call_accounting_is_atomic_exact_and_request_ordered() {
    // arrange
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

    // act
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

    // assert
    assert_eq!(provider.call_count(), 2);
    assert_eq!(provider.captured_requests().await, vec![ordinary, tool]);
}
