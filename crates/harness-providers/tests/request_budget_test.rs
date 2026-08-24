use std::collections::BTreeMap;
use std::sync::Arc;

use harness_providers::anthropic::{AnthropicProvider, AnthropicProviderConfig};
use harness_providers::cassette::{CassetteMode, RecordedProvider};
use harness_providers::mock::MockProvider;
use harness_providers::openai::{
    OpenAiApiMode, OpenAiAuthProfile, OpenAiCompatibleProvider, OpenAiCompatibleProviderConfig,
};
use harness_providers::{
    CompletionMessage, CompletionRequest, MessageRole, Provider, ProviderOutputCapDisposition,
    ProviderRequestCostError, ProviderRouter, ToolDef, UnwrapOrAbort,
};
use serde_json::json;
use tempfile::tempdir;

#[test]
fn request_budget_named_components_change_independently() {
    // arrange
    let baseline = generic_provider()
        .request_budget_semantics(&component_request(), 2)
        .unwrap_or_abort();

    let mut system = component_request();
    system.messages[0].content = "abcd".to_string();
    let mut history = component_request();
    history.messages[1].content = "abcd".to_string();
    let mut pending = component_request();
    pending.messages[2].content = "abcd".to_string();
    let mut tools = component_request();
    tools.tools.as_mut().unwrap_or_abort()[0].parameters = json!("abcd");
    let mut attachments = component_request();
    attachments.messages[3].content = tool_result_with_image();
    let mut framing = component_request();
    framing.messages.push(message(MessageRole::Assistant, ""));

    // act
    let costs = [system, tools, history, attachments, framing, pending].map(|request| {
        generic_provider()
            .request_budget_semantics(&request, 2)
            .unwrap_or_abort()
            .request_cost
    });

    // assert
    let base = baseline.request_cost;
    assert_eq!(
        costs[0],
        harness_providers::ProviderRequestCost {
            system_tokens: 1,
            ..base
        }
    );
    assert_eq!(
        costs[1],
        harness_providers::ProviderRequestCost {
            tools_tokens: base.tools_tokens + 1,
            ..base
        }
    );
    assert_eq!(
        costs[2],
        harness_providers::ProviderRequestCost {
            history_tokens: base.history_tokens + 1,
            ..base
        }
    );
    assert_eq!(
        costs[3],
        harness_providers::ProviderRequestCost {
            attachments_tokens: 7,
            ..base
        }
    );
    assert_eq!(
        costs[4],
        harness_providers::ProviderRequestCost {
            framing_tokens: base.framing_tokens + 4,
            ..base
        }
    );
    assert_eq!(
        costs[5],
        harness_providers::ProviderRequestCost {
            pending_prompt_tokens: 1,
            ..base
        }
    );
    assert_eq!(
        costs[3].total_input_tokens().unwrap_or_abort(),
        costs[3].system_tokens
            + costs[3].tools_tokens
            + costs[3].history_tokens
            + costs[3].attachments_tokens
            + costs[3].framing_tokens
            + costs[3].pending_prompt_tokens
    );
}

#[test]
fn request_budget_output_disposition_matches_protocol_and_auth() {
    // arrange
    let explicit = request(Some(128));
    let unknown = request(None);

    // act
    let chat = openai_provider(OpenAiApiMode::ChatCompletions, None)
        .request_budget_semantics(&explicit, 0)
        .unwrap_or_abort();
    let responses = openai_provider(OpenAiApiMode::Responses, None)
        .request_budget_semantics(&explicit, 0)
        .unwrap_or_abort();
    let codex = openai_provider(OpenAiApiMode::Responses, Some(OpenAiAuthProfile::Codex))
        .request_budget_semantics(&explicit, 0)
        .unwrap_or_abort();
    let generic_unknown = generic_provider()
        .request_budget_semantics(&unknown, 0)
        .unwrap_or_abort();

    // assert
    assert_eq!(
        chat.output_cap_disposition,
        ProviderOutputCapDisposition::Emitted(128)
    );
    assert_eq!(
        responses.output_cap_disposition,
        ProviderOutputCapDisposition::Emitted(128)
    );
    assert_eq!(
        codex.output_cap_disposition,
        ProviderOutputCapDisposition::ProviderControlled
    );
    assert_eq!(
        generic_unknown.output_cap_disposition,
        ProviderOutputCapDisposition::UnspecifiedUnknownLimit
    );
}

#[test]
fn request_budget_anthropic_defaults_output_without_claiming_request_reservation() {
    // arrange
    let request = request(None);
    let provider = anthropic_provider();

    // act
    let semantics = provider
        .request_budget_semantics(&request, 0)
        .unwrap_or_abort();

    // assert
    assert_eq!(request.max_tokens, None);
    assert_eq!(
        semantics.output_cap_disposition,
        ProviderOutputCapDisposition::ProviderDefaulted(4_096)
    );
}

#[test]
fn unsupported_attachment_budget_rejected() {
    // arrange
    let mut request = request(Some(128));
    request
        .messages
        .push(message(MessageRole::Tool, &tool_result_with_image()));

    // act
    let error = match anthropic_provider().request_budget_semantics(&request, 0) {
        Ok(_) => panic!("Anthropic must reject OpenAI image URLs"),
        Err(error) => error,
    };

    // assert
    assert!(matches!(
        error,
        ProviderRequestCostError::UnsupportedAttachment { message_index: 1, mime }
            if mime == "image/png"
    ));
}

#[test]
fn request_budget_metadata_only_media_contributes_zero() {
    // arrange
    let mut request = request(Some(128));
    request.context.has_media = true;

    // act
    let semantics = generic_provider()
        .request_budget_semantics(&request, 0)
        .unwrap_or_abort();

    // assert
    assert_eq!(semantics.request_cost.attachments_tokens, 0);
}

#[test]
fn request_budget_router_mock_and_recorded_delegate_explicitly() {
    // arrange
    let request = request(Some(128));
    let mock = MockProvider::default();
    let temp = tempdir().unwrap_or_abort();
    let recorded = RecordedProvider::with_ci(
        MockProvider::default(),
        temp.path().join("budget.json"),
        CassetteMode::Record,
        false,
    )
    .unwrap_or_abort();
    let mut providers: BTreeMap<String, Arc<dyn Provider>> = BTreeMap::new();
    providers.insert("mock".to_string(), Arc::new(MockProvider::default()));
    let router = ProviderRouter::new(providers);
    let mut routed_request = request.clone();
    routed_request.provider_id = Some("mock".to_string());

    // act
    let direct = mock.request_budget_semantics(&request, 0).unwrap_or_abort();
    let recorded = recorded
        .request_budget_semantics(&request, 0)
        .unwrap_or_abort();
    let routed = router
        .request_budget_semantics(&routed_request, 0)
        .unwrap_or_abort();

    // assert
    assert_eq!(recorded, direct);
    assert_eq!(routed, direct);
}

#[test]
fn request_budget_rejects_invalid_pending_prompt_index_and_role() {
    // arrange
    let mut wrong_role = request(Some(128));
    wrong_role.messages[0].role = MessageRole::Assistant;

    // act
    let out_of_bounds = generic_provider().request_budget_semantics(&request(Some(128)), 1);
    let wrong_role = generic_provider().request_budget_semantics(&wrong_role, 0);

    // assert
    assert!(matches!(
        out_of_bounds,
        Err(ProviderRequestCostError::PendingPromptOutOfBounds { .. })
    ));
    assert!(matches!(
        wrong_role,
        Err(ProviderRequestCostError::PendingPromptNotUser { .. })
    ));
}

fn component_request() -> CompletionRequest {
    CompletionRequest {
        messages: vec![
            message(MessageRole::System, ""),
            message(MessageRole::Assistant, ""),
            message(MessageRole::User, ""),
            message(MessageRole::Tool, &tool_result_without_image()),
        ],
        tools: Some(vec![ToolDef {
            tool_id: "ignored-by-wire".to_string(),
            function_name: String::new(),
            description: None,
            parameters: serde_json::Value::Null,
        }]),
        ..request(Some(128))
    }
}

fn request(max_tokens: Option<u32>) -> CompletionRequest {
    CompletionRequest {
        provider_id: None,
        model_id: "test-model".to_string(),
        messages: vec![message(MessageRole::User, "")],
        temperature: None,
        max_tokens,
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

fn message(role: MessageRole, content: &str) -> CompletionMessage {
    CompletionMessage {
        role,
        content: content.to_string(),
        name: None,
        tool_call_id: None,
        assistant_tool_calls: None,
    }
}

fn tool_result_without_image() -> String {
    json!({
        "_harness_tool_result": {
            "text": "abcd",
            "content": [{ "type": "text", "text": "abcd" }]
        }
    })
    .to_string()
}

fn tool_result_with_image() -> String {
    json!({
        "_harness_tool_result": {
            "text": "abcd",
            "content": [
                { "type": "text", "text": "abcd" },
                { "type": "file", "uri": (["data", ":", "image/png", ";base64,", "AAAA"].concat()), "mime": "image/png" }
            ]
        }
    })
    .to_string()
}

fn generic_provider() -> MockProvider {
    MockProvider::default()
}

fn openai_provider(
    api_mode: OpenAiApiMode,
    auth_profile: Option<OpenAiAuthProfile>,
) -> OpenAiCompatibleProvider {
    let provider = OpenAiCompatibleProvider::new(OpenAiCompatibleProviderConfig {
        base_url: "https://example.test/v1".to_string(),
        api_key: "test-key".to_string(),
        api_mode,
        timeout_ms: 1,
        headers: BTreeMap::new(),
    })
    .unwrap_or_abort();
    match auth_profile {
        Some(profile) => provider.with_auth_profile(profile),
        None => provider,
    }
}

fn anthropic_provider() -> AnthropicProvider {
    AnthropicProvider::new(AnthropicProviderConfig {
        base_url: "https://example.test".to_string(),
        api_key: "test-key".to_string(),
        timeout_ms: 1,
        headers: BTreeMap::new(),
    })
    .unwrap_or_abort()
}
