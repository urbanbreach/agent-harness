use super::*;

#[tokio::test]
async fn astra_codex_requests_apply_defaults_and_preserve_explicit_options() {
    // Given a Codex transport and both default and explicitly tuned Astra requests.
    let transport = ScriptedOpenAiTransport::new([
        ScriptedOpenAiResponse::sse(responses_done_sse_transcript()),
        ScriptedOpenAiResponse::sse(responses_done_sse_transcript()),
    ]);
    let provider = OpenAiCompatibleProvider::with_transport(
        OpenAiCompatibleProviderConfig {
            base_url: "https://api.openai.com/v1".to_string(),
            api_key: "test-key".to_string(),
            api_mode: OpenAiApiMode::Responses,
            timeout_ms: 0,
            headers: BTreeMap::new(),
        },
        Arc::clone(&transport) as Arc<dyn OpenAiHttpTransport>,
    )
    .unwrap_or_abort()
    .with_auth_profile(OpenAiAuthProfile::Codex)
    .with_credential_source(Arc::new(StaticCredentialSource {
        token: "test-oauth-token".to_string(),
        account_id: None,
        enterprise_url: None,
    }));
    let default_request = basic_request("gpt-6-astra");
    let mut explicit_request = basic_request("gpt-6-astra");
    explicit_request.reasoning_effort = Some("high".to_string());
    explicit_request.reasoning_summary = Some("detailed".to_string());
    explicit_request.text_verbosity = Some("high".to_string());

    // When the provider executes both requests.
    for request in [default_request, explicit_request] {
        let events = collect_events(&provider, request).await;
        assert!(matches!(
            events.last(),
            Some(ProviderStreamEvent::DoneWithMetadata { .. })
        ));
    }

    // Then the wire model and Codex defaults survive without overriding explicit settings.
    let requests = transport.requests();
    assert_eq!(requests.len(), 2);
    for request in &requests {
        assert_eq!(request.endpoint, CODEX_API_ENDPOINT);
        assert_eq!(request.body["model"], "gpt-6-astra");
        assert_eq!(
            request.body["include"],
            json!(["reasoning.encrypted_content"])
        );
        assert_eq!(request.body["store"], false);
        assert!(request.body.get("max_output_tokens").is_none());
    }
    assert_eq!(
        requests[0].body["reasoning"],
        json!({"effort": "low", "summary": "auto"})
    );
    assert_eq!(requests[0].body["text"], json!({"verbosity": "low"}));
    assert_eq!(
        requests[1].body["reasoning"],
        json!({"effort": "high", "summary": "detailed"})
    );
    assert_eq!(requests[1].body["text"], json!({"verbosity": "high"}));
}
