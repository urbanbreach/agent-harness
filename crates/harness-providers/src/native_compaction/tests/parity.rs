use super::*;
use crate::MessageRole;
fn request(lanes: Vec<NativeCompactionLane>) -> NativeCompactionRequest {
    NativeCompactionRequest {
        request: CompletionRequest {
            provider_id: Some("fixture".into()),
            model_id: "fixture".into(),
            messages: vec![CompletionMessage::text(MessageRole::User, "question")],
            ..Default::default()
        },
        lanes,
        instructions: "summarize entire prefix".into(),
        retained_message_budget: 64000,
        context_window: None,
        max_tokens: 100,
        retained: Vec::new(),
        settings: enabled(),
        ..Default::default()
    }
}
fn completed() -> String {
    "event: response.output_item.done\r\ndata: {\"item\":{\"type\":\"compaction\"}}\r\n\r\nevent: response.completed\r\ndata: {\"response\":{\"usage\":{\"input_tokens\":123,\"output_tokens\":4,\"total_tokens\":127}}}".into()
}
#[tokio::test]
async fn native_retries_fallback_and_terminal_protocol_errors(
) -> Result<(), Box<dyn std::error::Error>> {
    for (responses, lanes, expected_lane, expected_auth) in [
        (vec![(500, "server_error".into()),(200,completed())], vec![NativeCompactionLane::OpenAiV2],Some(NativeCompactionLane::OpenAiV2),false),
        (vec![(503,"auth_unavailable".into()),(200,json!({"output":[{"type":"compaction_summary"}]}).to_string())],vec![NativeCompactionLane::OpenAiV2,NativeCompactionLane::OpenAiV1],Some(NativeCompactionLane::OpenAiV1),false),
        (vec![(503,"auth_unavailable".into())],vec![NativeCompactionLane::OpenAiV2],None,true),
        (vec![(200,"data: {\"type\":\"error\",\"status\":400,\"message\":\"context too large\"}\n\n".into()),(401,"unauthorized".into())],vec![NativeCompactionLane::OpenAiV2,NativeCompactionLane::OpenAiV1],None,false),
        (vec![(200,"data: {\"type\":\"response.output_item.done\",\"item\":{\"type\":\"compaction\"}}\n\n".into());3],vec![NativeCompactionLane::OpenAiV2],None,false),
        (vec![(200,"data: {\"type\":\"response.completed\"}\n\n".into())],vec![NativeCompactionLane::OpenAiV2],None,false),
        (vec![(200,"data: {\"type\":\"response.output_item.done\",\"item\":{\"type\":\"compaction_summary\"}}\n\ndata: {\"type\":\"response.completed\"}\n\n".into())],vec![NativeCompactionLane::OpenAiV2],None,false),
        (vec![(200,"data: {\"type\":\"response.output_item.done\",\"item\":{\"type\":\"compaction\"}}\n\ndata: {\"type\":\"response.output_item.done\",\"item\":{\"type\":\"compaction\"}}\n\ndata: {\"type\":\"response.completed\"}\n\n".into())],vec![NativeCompactionLane::OpenAiV2],None,false),
    ] {
        let attempts = responses.len(); let (endpoint, task) = scripted_server(responses).await?;
        let provider = HttpProvider::new(&endpoint, Protocol::Responses, Duration::from_secs(5))?;
        let result = provider.native_compact(request(lanes), CancellationToken::new()).await;
        if let Some(lane) = expected_lane { assert_eq!(result?.lane,lane); } else { let error = result.err().ok_or("expected error")?; assert_eq!(error.auth,expected_auth); if error.status == Some(400) { assert_eq!(error.category,Some(ProviderErrorCategory::ContextWindowExceeded)); } }
        assert_eq!(task.await??.len(),attempts);
    }
    let cancel = CancellationToken::new();
    cancel.cancel();
    let provider = HttpProvider::new(
        "http://127.0.0.1:9/v1/responses",
        Protocol::Responses,
        Duration::from_secs(1),
    )?;
    assert!(provider
        .native_compact(
            request(vec![
                NativeCompactionLane::OpenAiV2,
                NativeCompactionLane::OpenAiV1
            ]),
            cancel
        )
        .await
        .err()
        .is_some_and(|e| e.message.contains("cancelled")));
    Ok(())
}
#[tokio::test]
async fn fit_preserves_calls_and_rejects_earlier_overflow() -> Result<(), Box<dyn std::error::Error>>
{
    let (endpoint, task) =
        server(json!({"output":[{"type":"compaction_summary"}]}).to_string()).await?;
    let provider = HttpProvider::new(&endpoint, Protocol::Responses, Duration::from_secs(5))?;
    let mut input = request(vec![NativeCompactionLane::OpenAiV1]);
    input.context_window = Some(400);
    let mut assistant = CompletionMessage::text(MessageRole::Assistant, "");
    assistant.assistant_tool_calls = Some(vec![crate::AssistantToolCall {
        tool_call_id: "call-1".into(),
        function_name: "read".into(),
        arguments_json: "{}".into(),
    }]);
    let mut output = CompletionMessage::text(MessageRole::Tool, "x".repeat(10000));
    output.tool_call_id = Some("call-1".into());
    input.request.messages.extend([assistant, output]);
    provider
        .native_compact(input, CancellationToken::new())
        .await?;
    let (_, body) = task.await??;
    assert_eq!(body["input"][1]["call_id"], "call-1");
    assert_eq!(
        body["input"][2]["output"],
        "Output: exceeded available model context → truncated."
    );
    let mut input = request(vec![NativeCompactionLane::OpenAiV1]);
    input.context_window = Some(400);
    input.request.messages[0].content = "x".repeat(10000);
    let error = provider
        .native_compact(input, CancellationToken::new())
        .await
        .err()
        .ok_or("expected overflow")?;
    assert_eq!(
        error.category,
        Some(ProviderErrorCategory::ContextWindowExceeded)
    );
    Ok(())
}
#[tokio::test]
async fn signed_anthropic_compaction_and_native_replay() -> Result<(), Box<dyn std::error::Error>> {
    use tokio_stream::StreamExt;
    for host in ["api.anthropic.com", "us-central1-aiplatform.googleapis.com"] {
        let native = "data: {\"type\":\"content_block_start\",\"content_block\":{\"type\":\"compaction\",\"content\":\"signed summary\",\"signature\":\"signature\"}}\n\ndata: {\"type\":\"message_delta\",\"delta\":{\"stop_reason\":\"compaction\"},\"usage\":{\"iterations\":[{\"type\":\"compaction\",\"input_tokens\":64,\"output_tokens\":2002,\"cache_creation_input_tokens\":80082}]}}\n\ndata: {\"type\":\"message_stop\"}\n\n";
        let (endpoint, task) = scripted_server(vec![
            (200, native.into()),
            (200, "data: {\"type\":\"message_stop\"}\n\n".into()),
        ])
        .await?;
        let provider = anthropic_provider(&endpoint, host)?;
        let mut input = request(vec![NativeCompactionLane::Anthropic]);
        input.request.model_id = "claude-sonnet-4-6".into();
        input.request.messages.insert(
            0,
            CompletionMessage::text(MessageRole::System, "live system"),
        );
        let mut result = provider
            .native_compact(input, CancellationToken::new())
            .await?;
        assert_eq!(result.summary.as_deref(), Some("signed summary"));
        assert_eq!(result.used_tokens, 80146);
        result.replay.set_anthropic_files(
            "<files>\nsource.rs (Read)\n</files>".into(),
            2,
            Vec::new(),
        )?;
        let mut summary = CompletionMessage::text(MessageRole::User, "decorated summary");
        summary.continuation = Some(result.replay);
        let mut call = CompletionMessage::text(MessageRole::Assistant, "tool call");
        call.assistant_tool_calls = Some(vec![crate::AssistantToolCall {
            tool_call_id: "read".into(),
            function_name: "read".into(),
            arguments_json: "{}".into(),
        }]);
        let mut tool_result = CompletionMessage::text(MessageRole::Tool, "read result");
        tool_result.tool_call_id = Some("read".into());
        let replay = CompletionRequest {
            provider_id: Some("fixture".into()),
            model_id: "claude-sonnet-4-6".into(),
            messages: vec![
                summary,
                CompletionMessage::text(MessageRole::Assistant, "retained assistant"),
                call,
                tool_result,
                CompletionMessage::text(MessageRole::User, "new request"),
            ],
            ..Default::default()
        };
        let _events = provider
            .stream_completion(replay)
            .await
            .collect::<Vec<_>>()
            .await;
        let exchanges = task.await??;
        let (headers, body) = &exchanges[0];
        if host == "api.anthropic.com" {
            assert!(headers
                .to_lowercase()
                .contains("anthropic-beta: compact-2026-09-04"));
        } else {
            assert!(!headers.to_lowercase().contains("anthropic-beta:"));
            assert_eq!(body["anthropic_beta"], json!(["compact-2026-09-04"]));
        }
        assert!(headers.to_lowercase().contains("x-api-key: fixture-key"));
        assert!(headers
            .to_lowercase()
            .contains("anthropic-version: 2023-06-01"));
        assert_eq!(
            body["compaction"],
            json!({"type":"summarize","instructions":"summarize entire prefix"})
        );
        assert_eq!(body["system"], "live system");
        assert_eq!(
            exchanges[1].1["messages"],
            json!([
                {"role":"assistant","content":[{"type":"compaction","content":"signed summary","signature":"signature"},{"type":"text","text":"retained assistant"}]},
                {"role":"assistant","content":[{"type":"text","text":"tool call"},{"type":"tool_use","id":"read","name":"read","input":{}}]},
                {"role":"user","content":[{"type":"tool_result","tool_use_id":"read","content":"read result"}]},
                {"role":"user","content":[{"type":"text","text":"<files>\nsource.rs (Read)\n</files>"}]},
                {"role":"user","content":[{"type":"text","text":"new request"}]}
            ])
        );
    }
    Ok(())
}
fn anthropic_provider(
    endpoint: &str,
    host: &str,
) -> Result<HttpProvider, Box<dyn std::error::Error>> {
    let mut url = reqwest::Url::parse(endpoint)?;
    let addr = url
        .socket_addrs(|| None)?
        .into_iter()
        .next()
        .ok_or("address")?;
    url.set_host(Some(host))?;
    url.set_path(if host == "api.anthropic.com" {
        "/v1/messages"
    } else {
        "/v1/projects/fixture:rawPredict"
    });
    let mut provider =
        HttpProvider::new(url.as_str(), Protocol::Anthropic, Duration::from_secs(5))?
            .with_credentials(std::sync::Arc::new(Credentials));
    provider.client = reqwest::Client::builder()
        .no_proxy()
        .resolve(host, addr)
        .build()?;
    Ok(provider)
}
#[tokio::test]
async fn anthropic_requires_signed_compaction_stop_and_preserves_http_auth(
) -> Result<(), Box<dyn std::error::Error>> {
    for (status, signature, stop, expected_status) in [
        (200, "", "compaction", None),
        (200, "signature", "end_turn", None),
        (401, "signature", "compaction", Some(401)),
    ] {
        let response = format!(
            "data: {}\n\ndata: {}\n\ndata: {{\"type\":\"message_stop\"}}\n\n",
            json!({"type":"content_block_start","content_block":{"type":"compaction","content":"summary","signature":signature}}),
            json!({"type":"message_delta","delta":{"stop_reason":stop}})
        );
        let (endpoint, task) = scripted_server(vec![(status, response)]).await?;
        let provider = anthropic_provider(&endpoint, "api.anthropic.com")?;
        let mut input = request(vec![NativeCompactionLane::Anthropic]);
        input.request.model_id = "claude-sonnet-4-6".into();
        let error = provider
            .native_compact(input, CancellationToken::new())
            .await
            .err()
            .ok_or("expected rejection")?;
        assert_eq!(error.status, expected_status);
        assert_eq!(error.auth, status == 401);
        assert_eq!(task.await??.len(), 1);
    }
    Ok(())
}
#[tokio::test]
async fn codex_compaction_reuses_transport_identity_and_requires_explicit_v1(
) -> Result<(), Box<dyn std::error::Error>> {
    for explicit in [false, true] {
        let lane = if explicit {
            NativeCompactionLane::OpenAiV1
        } else {
            NativeCompactionLane::OpenAiV2
        };
        let response = if explicit {
            json!({"output":[{"type":"compaction_summary"}]}).to_string()
        } else {
            completed()
        };
        let (mut endpoint, task) = server(response).await?;
        if explicit {
            endpoint.push_str("/compact");
        }
        let provider = HttpProvider::new(&endpoint, Protocol::Responses, Duration::from_secs(5))?
            .with_auth_profile(crate::ProviderAuthProfile::Codex)
            .with_credentials(std::sync::Arc::new(Credentials));
        let mut input = request(vec![lane]);
        input.settings.endpoint = explicit.then(|| endpoint.clone());
        input.request.model_id = "gpt-6-astra".into();
        input.request.context.session_id = Some("session-1".into());
        input.request.context.request_id = Some("request-1".into());
        if !explicit {
            let mut unavailable = request(vec![NativeCompactionLane::OpenAiV1]);
            unavailable.request.model_id = "gpt-6-astra".into();
            assert!(provider
                .native_compact(unavailable, CancellationToken::new())
                .await
                .is_err());
        }
        let result = provider
            .native_compact(input, CancellationToken::new())
            .await?;
        assert_eq!(result.lane, lane);
        let (headers, body) = task.await??;
        let headers = headers.to_lowercase();
        assert!(headers.contains("originator: harness"));
        assert!(headers.contains("chatgpt-account-id: account"));
        assert!(headers.contains("session-id: session-1"));
        assert!(headers.contains("request-id: request-1"));
        if explicit {
            assert!(headers.starts_with("post /v1/responses/compact "));
            assert_eq!(body.as_object().map(|o| o.len()), Some(3));
        } else {
            assert_eq!(body["prompt_cache_key"], "session-1");
            assert_eq!(body["include"], json!(["reasoning.encrypted_content"]));
            assert_eq!(body["reasoning"], json!({"effort":"low","summary":"auto"}));
            assert_eq!(
                body["input"].as_array().and_then(|a| a.last()),
                Some(&json!({"type":"compaction_trigger"}))
            );
        }
    }
    Ok(())
}
