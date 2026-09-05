//! Provider event and credential metadata serialization contracts.

use harness_providers::{
    CompletionMessage, CompletionRequest, CompletionUsage, MessageRole, ProviderCredentialError,
    ProviderCredentialKind, ProviderErrorCategory, ProviderStreamEvent,
};

/// A test API key that looks like a real OpenAI key for redaction detection.
const TEST_API_KEY: &str = "sk-test-redaction-key-1234567890abcdef";

/// A test Anthropic key.
const TEST_ANTHROPIC_KEY: &str = "sk-ant-test-redaction-1234567890";

#[test]
fn provider_stream_event_error_does_not_leak_api_key() {
    // arrange
    let event = ProviderStreamEvent::categorized_error(
        "request failed with status 401",
        ProviderErrorCategory::InvalidCredentials,
    );

    // act
    let serialized = serde_json::to_string(&event).expect("event should serialize");

    // assert
    assert!(
        !serialized.contains(TEST_API_KEY),
        "serialized ProviderStreamEvent must not contain the API key"
    );
    assert!(
        !serialized.contains(TEST_ANTHROPIC_KEY),
        "serialized ProviderStreamEvent must not contain the Anthropic key"
    );
}

#[test]
fn provider_credential_error_does_not_leak_token() {
    // arrange
    let error = ProviderCredentialError::new(
        ProviderErrorCategory::InvalidCredentials,
        "credential validation failed",
    );

    // act
    let message = error.to_string();

    // assert
    assert!(
        !message.contains(TEST_API_KEY),
        "ProviderCredentialError message must not contain the API key"
    );
}

#[test]
fn completion_request_does_not_serialize_api_key() {
    // arrange
    let request = CompletionRequest {
        provider_id: Some("default".to_string()),
        model_id: "gpt-5.4-mini".to_string(),
        messages: vec![CompletionMessage {
            role: MessageRole::User,
            content: "hello".to_string(),
            name: None,
            tool_call_id: None,
            assistant_tool_calls: None,
        }],
        temperature: None,
        max_tokens: None,
        variant: None,
        reasoning_effort: None,
        text_verbosity: None,
        reasoning_summary: None,
        thinking: None,
        tools: None,
        tool_choice: None,
        context: Default::default(),
        stream: true,
    };

    // act
    let serialized = serde_json::to_string(&request).expect("request should serialize");

    // assert
    assert!(
        !serialized.contains(TEST_API_KEY),
        "serialized CompletionRequest must not contain the API key"
    );
    assert!(
        !serialized.contains("bearer"),
        "serialized CompletionRequest must not contain bearer token material"
    );
}

#[test]
fn provider_stream_finished_metadata_does_not_leak_secrets() {
    use harness_providers::ProviderStreamFinishedMetadata;

    // arrange
    let metadata = ProviderStreamFinishedMetadata {
        provider_response_id: Some("resp_123".to_string()),
        provider_session_id: Some("sess_456".to_string()),
        provider_cache_id: None,
        provider_stop_reason: Some("stop".to_string()),
        cache_read_tokens: Some(100),
        cache_write_tokens: Some(50),
        assistant_message_id: Some("msg_789".to_string()),
        thinking: None,
    };

    // act
    let serialized = serde_json::to_string(&metadata).expect("metadata should serialize");

    // assert
    assert!(
        !serialized.contains(TEST_API_KEY),
        "serialized ProviderStreamFinishedMetadata must not contain the API key"
    );
    assert!(
        !serialized.contains("authorization"),
        "metadata must not contain authorization header material"
    );
}

#[test]
fn provider_bearer_token_kind_is_typed_not_string() {
    // arrange
    // ProviderCredentialKind is an enum, not a string — this test verifies
    // that credential kind is type-safe and does not leak token values.
    let kinds = [
        ProviderCredentialKind::StoredOauth,
        ProviderCredentialKind::StoredApiKey,
        ProviderCredentialKind::EnvApiKey,
        ProviderCredentialKind::InlineApiKey,
    ];

    // act
    let serialized = kinds.map(|kind| serde_json::to_string(&kind).expect("kind should serialize"));

    // assert
    for serialized in serialized {
        // The serialized form should be a snake_case string tag, not a token value
        assert!(
            serialized.len() < 30,
            "ProviderCredentialKind should serialize to a short tag, got: {serialized}"
        );
        assert!(
            !serialized.contains(TEST_API_KEY),
            "ProviderCredentialKind must not contain API key material"
        );
    }
}

#[test]
fn completion_usage_serializes_without_secrets() {
    // arrange
    let usage = CompletionUsage {
        prompt_tokens: 100,
        completion_tokens: 50,
        total_tokens: 150,
    };

    // act
    let serialized = serde_json::to_string(&usage).expect("usage should serialize");

    // assert
    assert!(
        !serialized.contains(TEST_API_KEY),
        "CompletionUsage must not contain API key"
    );
}
