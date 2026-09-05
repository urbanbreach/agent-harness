use super::*;
use crate::UnwrapOrAbort;

#[tokio::test]
async fn openai_compatible_offline_transport_parses_sse_deltas() {
    let transport =
        ScriptedOpenAiTransport::new([ScriptedOpenAiResponse::sse(deterministic_sse_transcript())]);
    let provider = provider_for_transport(Arc::clone(&transport), "test-secret-key");
    let events = collect_events(&provider, basic_request("gpt-4o-mini")).await;

    assert_eq!(
        events,
        vec![
            ProviderStreamEvent::Started { metadata: None },
            ProviderStreamEvent::TextDelta("Hello".to_string()),
            ProviderStreamEvent::TextDelta(" world".to_string()),
            ProviderStreamEvent::DoneWithMetadata {
                usage: Some(CompletionUsage {
                    prompt_tokens: 4,
                    completion_tokens: 2,
                    total_tokens: 6,
                }),
                metadata: Some(ProviderStreamFinishedMetadata {
                    provider_response_id: Some("chatcmpl-1".to_string()),
                    provider_stop_reason: Some("stop".to_string()),
                    ..ProviderStreamFinishedMetadata::default()
                }),
            },
        ]
    );

    let requests = transport.requests();
    assert_eq!(requests.len(), 1);

    let request = &requests[0];
    assert!(request.endpoint.ends_with("/v1/chat/completions"));
    assert_eq!(request.bearer_token, "test-secret-key");

    let body = &request.body;
    assert_eq!(body.get("stream"), Some(&serde_json::Value::Bool(true)));
    assert_eq!(
        body.get("model"),
        Some(&serde_json::Value::String("gpt-4o-mini".to_string()))
    );
    assert!(body.get("api_key").is_none());
}

#[tokio::test]
async fn openai_compatible_uses_credential_source_before_static_api_key() {
    let transport =
        ScriptedOpenAiTransport::new([ScriptedOpenAiResponse::sse(deterministic_sse_transcript())]);
    let provider = provider_for_transport(Arc::clone(&transport), "static-key")
        .with_credential_source(Arc::new(StaticCredentialSource {
            token: "stored-oauth-token".to_string(),
            account_id: None,
            enterprise_url: None,
        }));

    let events = collect_events(&provider, basic_request("gpt-4o-mini")).await;

    assert!(matches!(
        events.last(),
        Some(ProviderStreamEvent::DoneWithMetadata { .. })
    ));
    let requests = transport.requests();
    assert_eq!(requests.len(), 1);
    assert_eq!(requests[0].bearer_token, "stored-oauth-token");
}

#[tokio::test]
async fn codex_auth_profile_rewrites_endpoint_and_adds_context_headers() {
    let mut config_headers = BTreeMap::new();
    config_headers.insert(
        "Authorization".to_string(),
        "Bearer stale-config-token".to_string(),
    );
    config_headers.insert("x-test-header".to_string(), "kept".to_string());
    let transport =
        ScriptedOpenAiTransport::new([
            ScriptedOpenAiResponse::sse(responses_done_sse_transcript()),
        ]);
    let provider = OpenAiCompatibleProvider::with_transport(
        OpenAiCompatibleProviderConfig {
            base_url: "https://api.openai.com/v1".to_string(),
            api_key: "static-key".to_string(),
            api_mode: OpenAiApiMode::Responses,
            timeout_ms: 0,
            headers: config_headers,
        },
        Arc::clone(&transport) as Arc<dyn OpenAiHttpTransport>,
    )
    .unwrap_or_abort()
    .with_auth_profile(OpenAiAuthProfile::Codex)
    .with_credential_source(Arc::new(StaticCredentialSource {
        token: "codex-oauth-token".to_string(),
        account_id: Some("acct_123".to_string()),
        enterprise_url: None,
    }));

    let mut request = basic_request("gpt-5.5");
    request.messages.insert(
        0,
        CompletionMessage {
            role: MessageRole::System,
            content: "codex base prompt".to_string(),
            name: None,
            tool_call_id: None,
            assistant_tool_calls: None,
        },
    );
    request.context.session_id = Some("session-abc".to_string());
    request.context.request_id = Some("request-def".to_string());
    let budget = provider
        .request_budget_semantics(&request, 1)
        .expect("Codex request budget semantics");
    let events = collect_events(&provider, request).await;

    assert!(matches!(
        events.last(),
        Some(ProviderStreamEvent::DoneWithMetadata { .. })
    ));
    assert_eq!(
        budget.output_cap_disposition,
        ProviderOutputCapDisposition::ProviderControlled
    );
    let requests = transport.requests();
    assert_eq!(requests.len(), 1);
    let request = &requests[0];
    assert_eq!(request.endpoint, CODEX_API_ENDPOINT);
    assert_eq!(request.bearer_token, "codex-oauth-token");
    assert!(request.headers.get("authorization").is_none());
    assert_eq!(
        request
            .headers
            .get("chatgpt-account-id")
            .and_then(|value| value.to_str().ok()),
        Some("acct_123")
    );
    assert_eq!(
        request
            .headers
            .get("session-id")
            .and_then(|value| value.to_str().ok()),
        Some("session-abc")
    );
    assert_eq!(
        request
            .headers
            .get("request-id")
            .and_then(|value| value.to_str().ok()),
        Some("request-def")
    );
    assert_eq!(
        request
            .headers
            .get("originator")
            .and_then(|value| value.to_str().ok()),
        Some("harness")
    );
    assert_eq!(
        request
            .headers
            .get("x-test-header")
            .and_then(|value| value.to_str().ok()),
        Some("kept")
    );
    assert_eq!(
        request.body.get("instructions"),
        Some(&serde_json::Value::String("codex base prompt".to_string()))
    );
    assert_eq!(
        request.body.get("store"),
        Some(&serde_json::Value::Bool(false))
    );
    assert_eq!(
        request.body.get("reasoning"),
        Some(&serde_json::json!({
            "effort": "medium",
            "summary": "auto"
        }))
    );
    assert_eq!(
        request.body.get("include"),
        Some(&serde_json::json!(["reasoning.encrypted_content"]))
    );
    assert_eq!(
        request.body.get("text"),
        Some(&serde_json::json!({
            "verbosity": "low"
        }))
    );
    let input = request
        .body
        .get("input")
        .and_then(serde_json::Value::as_array)
        .unwrap_or_abort();
    assert_eq!(input.len(), 1);
    assert_eq!(
        input[0].get("role"),
        Some(&serde_json::Value::String("user".to_string()))
    );
}

#[tokio::test]
async fn codex_gpt_request_defaults_match_reference_matrix() {
    let transport = ScriptedOpenAiTransport::new([
        ScriptedOpenAiResponse::sse(responses_done_sse_transcript()),
        ScriptedOpenAiResponse::sse(responses_done_sse_transcript()),
        ScriptedOpenAiResponse::sse(responses_done_sse_transcript()),
        ScriptedOpenAiResponse::sse(responses_done_sse_transcript()),
    ]);
    let provider = OpenAiCompatibleProvider::with_transport(
        OpenAiCompatibleProviderConfig {
            base_url: "https://api.openai.com/v1".to_string(),
            api_key: "static-key".to_string(),
            api_mode: OpenAiApiMode::Responses,
            timeout_ms: 0,
            headers: BTreeMap::new(),
        },
        Arc::clone(&transport) as Arc<dyn OpenAiHttpTransport>,
    )
    .unwrap_or_abort()
    .with_auth_profile(OpenAiAuthProfile::Codex)
    .with_credential_source(Arc::new(StaticCredentialSource {
        token: "codex-oauth-token".to_string(),
        account_id: None,
        enterprise_url: None,
    }));

    let default_gpt = basic_request("gpt-5.5");
    let mut explicit_gpt = basic_request("gpt-5.5");
    explicit_gpt.reasoning_effort = Some("xhigh".to_string());
    explicit_gpt.reasoning_summary = Some("auto".to_string());
    let codex_gpt = basic_request("gpt-5.3-codex");
    let pro_gpt = basic_request("gpt-5.5-pro");

    for request in [default_gpt, explicit_gpt, codex_gpt, pro_gpt] {
        let events = collect_events(&provider, request).await;
        assert!(matches!(
            events.last(),
            Some(ProviderStreamEvent::DoneWithMetadata { .. })
        ));
    }

    let requests = transport.requests();
    assert_eq!(requests.len(), 4);
    assert_eq!(
        requests[0].body.get("reasoning"),
        Some(&serde_json::json!({ "effort": "medium", "summary": "auto" }))
    );
    assert!(requests[0].body.get("max_output_tokens").is_none());
    assert!(requests[0].body.get("max_tokens").is_none());
    assert_eq!(
        requests[0].body.get("include"),
        Some(&serde_json::json!(["reasoning.encrypted_content"]))
    );
    assert_eq!(
        requests[0].body.get("text"),
        Some(&serde_json::json!({ "verbosity": "low" }))
    );
    assert_eq!(
        requests[1].body.get("reasoning"),
        Some(&serde_json::json!({ "effort": "xhigh", "summary": "auto" }))
    );
    assert_eq!(
        requests[1].body.get("include"),
        Some(&serde_json::json!(["reasoning.encrypted_content"]))
    );
    assert_eq!(
        requests[2].body.get("reasoning"),
        Some(&serde_json::json!({ "effort": "medium", "summary": "auto" }))
    );
    assert_eq!(
        requests[2].body.get("include"),
        Some(&serde_json::json!(["reasoning.encrypted_content"]))
    );
    assert!(requests[2].body.get("text").is_none());
    assert_eq!(
        requests[3].body.get("reasoning"),
        Some(&serde_json::json!({ "effort": "medium", "summary": "auto" }))
    );
    assert_eq!(
        requests[3].body.get("text"),
        Some(&serde_json::json!({ "verbosity": "low" }))
    );
}

#[tokio::test]
async fn github_copilot_auth_profile_rewrites_public_and_enterprise_headers() {
    let mut config_headers = BTreeMap::new();
    config_headers.insert(
        "Authorization".to_string(),
        "Bearer stale-config-token".to_string(),
    );
    config_headers.insert("x-api-key".to_string(), "stale-api-key".to_string());
    config_headers.insert("x-test-header".to_string(), "kept".to_string());
    let transport =
        ScriptedOpenAiTransport::new([ScriptedOpenAiResponse::sse(deterministic_sse_transcript())]);
    let provider = OpenAiCompatibleProvider::with_transport(
        OpenAiCompatibleProviderConfig {
            base_url: "https://api.openai.com/v1".to_string(),
            api_key: "static-key".to_string(),
            api_mode: OpenAiApiMode::ChatCompletions,
            timeout_ms: 0,
            headers: config_headers.clone(),
        },
        Arc::clone(&transport) as Arc<dyn OpenAiHttpTransport>,
    )
    .unwrap_or_abort()
    .with_auth_profile(OpenAiAuthProfile::GithubCopilot)
    .with_credential_source(Arc::new(StaticCredentialSource {
        token: "copilot-public-token".to_string(),
        account_id: None,
        enterprise_url: None,
    }));

    let mut public_request = basic_request("gpt-5.5");
    public_request.context.initiator = ProviderRequestInitiator::User;
    public_request.context.has_media = false;
    let events = collect_events(&provider, public_request).await;

    assert!(matches!(
        events.last(),
        Some(ProviderStreamEvent::DoneWithMetadata { .. })
    ));
    let requests = transport.requests();
    assert_eq!(requests.len(), 1);
    let public = &requests[0];
    assert_eq!(
        public.endpoint,
        format!("{COPILOT_API_BASE}/chat/completions")
    );
    assert_eq!(public.bearer_token, "copilot-public-token");
    assert!(public.headers.get("authorization").is_none());
    assert!(public.headers.get("x-api-key").is_none());
    assert_eq!(
        public
            .headers
            .get("x-initiator")
            .and_then(|value| value.to_str().ok()),
        Some("user")
    );
    assert_eq!(
        public
            .headers
            .get("Openai-Intent")
            .and_then(|value| value.to_str().ok()),
        Some("conversation-edits")
    );
    assert!(public.headers.get("Copilot-Vision-Request").is_none());
    assert_eq!(
        public
            .headers
            .get("x-test-header")
            .and_then(|value| value.to_str().ok()),
        Some("kept")
    );

    let enterprise_transport =
        ScriptedOpenAiTransport::new([
            ScriptedOpenAiResponse::sse(responses_done_sse_transcript()),
        ]);
    let enterprise_provider = OpenAiCompatibleProvider::with_transport(
        OpenAiCompatibleProviderConfig {
            base_url: "https://api.openai.com/v1".to_string(),
            api_key: "static-key".to_string(),
            api_mode: OpenAiApiMode::Responses,
            timeout_ms: 0,
            headers: config_headers,
        },
        Arc::clone(&enterprise_transport) as Arc<dyn OpenAiHttpTransport>,
    )
    .unwrap_or_abort()
    .with_auth_profile(OpenAiAuthProfile::GithubCopilot)
    .with_credential_source(Arc::new(StaticCredentialSource {
        token: "copilot-enterprise-token".to_string(),
        account_id: None,
        enterprise_url: Some("https://GHE.Example.COM/".to_string()),
    }));

    let mut enterprise_request = basic_request("claude-sonnet-4.5");
    enterprise_request.context.initiator = ProviderRequestInitiator::Agent;
    enterprise_request.context.has_media = true;
    let events = collect_events(&enterprise_provider, enterprise_request).await;

    assert!(matches!(
        events.last(),
        Some(ProviderStreamEvent::DoneWithMetadata { .. })
    ));
    let requests = enterprise_transport.requests();
    assert_eq!(requests.len(), 1);
    let enterprise = &requests[0];
    assert_eq!(
        enterprise.endpoint,
        "https://copilot-api.ghe.example.com/responses"
    );
    assert_eq!(enterprise.bearer_token, "copilot-enterprise-token");
    assert_eq!(
        enterprise
            .headers
            .get("x-initiator")
            .and_then(|value| value.to_str().ok()),
        Some("agent")
    );
    assert_eq!(
        enterprise
            .headers
            .get("Copilot-Vision-Request")
            .and_then(|value| value.to_str().ok()),
        Some("true")
    );
}
