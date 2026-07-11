use crate::cassette::{assert_cassette_is_safe, CassetteInteraction, ProviderCassette};
use crate::{CompletionMessage, CompletionRequest, MessageRole, ProviderStreamEvent};

#[test]
fn safety_scan_rejects_common_secret_shapes() {
    // arrange
    let cassette = ProviderCassette::new(vec![CassetteInteraction::new(
        request_with_content("leaked sk-testsecret123"),
        vec![ProviderStreamEvent::TextDelta("never written".to_string())],
    )]);

    // act
    let err = assert_cassette_is_safe(&cassette).expect_err("unsafe cassette");

    // assert
    assert!(err.to_string().contains("openai_api_key"));
}

fn request_with_content(content: &str) -> CompletionRequest {
    CompletionRequest {
        provider_id: None,
        model_id: "test-model".to_string(),
        messages: vec![CompletionMessage {
            role: MessageRole::User,
            content: content.to_string(),
            name: None,
            tool_call_id: None,
            assistant_tool_calls: None,
        }],
        temperature: Some(0.0),
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
    }
}
