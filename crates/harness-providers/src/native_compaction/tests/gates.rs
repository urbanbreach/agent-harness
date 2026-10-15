use super::*;
use crate::MessageRole;

#[test]
fn lane_gates_follow_settings_provider_and_route() -> Result<(), Box<dyn std::error::Error>> {
    use NativeCompactionLane::{Anthropic, OpenAiV1, OpenAiV2};
    let settings = |remote: bool,
                    v2: bool,
                    enabled: Option<bool>,
                    api: Option<&str>,
                    endpoint: Option<&str>| {
        NativeLaneSettings {
            remote_enabled: remote,
            v2_enabled: v2,
            endpoint: endpoint.map(str::to_owned),
            model: Some(crate::RemoteModelOverride {
                enabled,
                api: api.map(str::to_owned),
                model: None,
            }),
            ..Default::default()
        }
    };
    let responses = "https://gateway.example/v1/responses";
    // (url, protocol, codex, provider, model, settings, lanes)
    let cases = [
        (
            responses,
            Protocol::Responses,
            false,
            "openai",
            "gpt-5",
            settings(true, true, None, None, None),
            vec![OpenAiV2, OpenAiV1],
        ),
        (
            responses,
            Protocol::Responses,
            false,
            "gateway",
            "gpt-5",
            settings(true, true, None, None, None),
            vec![OpenAiV2],
        ),
        (
            responses,
            Protocol::Responses,
            false,
            "gateway",
            "gpt-5",
            settings(true, true, Some(true), None, None),
            vec![OpenAiV2, OpenAiV1],
        ),
        (
            responses,
            Protocol::Responses,
            false,
            "openai",
            "gpt-5",
            settings(true, true, Some(false), None, None),
            vec![],
        ),
        (
            responses,
            Protocol::Responses,
            false,
            "openai",
            "gpt-5",
            settings(false, true, None, None, None),
            vec![],
        ),
        (
            responses,
            Protocol::Responses,
            false,
            "openai",
            "gpt-5",
            settings(true, false, None, None, None),
            vec![OpenAiV1],
        ),
        (
            responses,
            Protocol::Responses,
            true,
            "codex",
            "gpt-6-astra",
            settings(true, true, None, None, None),
            vec![OpenAiV2],
        ),
        (
            responses,
            Protocol::Responses,
            true,
            "codex",
            "gpt-6-astra",
            settings(
                true,
                true,
                None,
                None,
                Some("https://proxy.example/v1/responses/compact"),
            ),
            vec![OpenAiV2, OpenAiV1],
        ),
        (
            responses,
            Protocol::Responses,
            true,
            "codex",
            "gpt-6-astra",
            settings(true, true, None, None, Some("  ")),
            vec![OpenAiV2],
        ),
        (
            "https://gateway.example/v1/chat/completions",
            Protocol::Chat,
            false,
            "gateway",
            "m",
            settings(true, true, Some(true), Some("responses"), None),
            vec![OpenAiV2, OpenAiV1],
        ),
        (
            "https://gateway.example/v1/chat/completions",
            Protocol::Chat,
            false,
            "openai",
            "m",
            settings(true, true, Some(true), None, None),
            vec![],
        ),
        (
            "https://api.anthropic.com/v1/messages",
            Protocol::Anthropic,
            false,
            "anthropic",
            "claude-sonnet-4-6",
            settings(true, true, None, None, None),
            vec![Anthropic],
        ),
        (
            "https://api.anthropic.com/v1/messages",
            Protocol::Anthropic,
            false,
            "anthropic",
            "claude-future-9",
            settings(true, true, None, None, None),
            vec![],
        ),
        (
            "https://api.anthropic.com/v1/messages",
            Protocol::Anthropic,
            false,
            "anthropic",
            "claude-future-9",
            settings(true, true, Some(true), None, None),
            vec![Anthropic],
        ),
        (
            "https://x.services.ai.azure.com/anthropic/v1/messages",
            Protocol::Anthropic,
            false,
            "azure",
            "claude-sonnet-4-6",
            settings(true, true, None, None, None),
            vec![],
        ),
        (
            "https://x.services.ai.azure.com/anthropic/v1/messages",
            Protocol::Anthropic,
            false,
            "azure",
            "claude-sonnet-4-6",
            settings(true, true, Some(true), None, None),
            vec![Anthropic],
        ),
        (
            "https://gateway.example/v1/messages",
            Protocol::Anthropic,
            false,
            "gateway",
            "claude-sonnet-4-6",
            settings(true, true, Some(true), None, None),
            vec![],
        ),
    ];
    for (url, protocol, codex, provider, model, settings, expected) in cases {
        let mut http = HttpProvider::new(url, protocol, Duration::from_secs(1))?;
        if codex {
            http = http.with_auth_profile(crate::ProviderAuthProfile::Codex);
        }
        let request = CompletionRequest {
            provider_id: Some(provider.into()),
            model_id: model.into(),
            ..Default::default()
        };
        assert_eq!(
            http.compaction_lanes(&request, &settings),
            expected,
            "{url} {provider} {model} {settings:?}"
        );
    }
    Ok(())
}

fn signed_payload(extra: Value) -> Result<Box<ProviderContinuation>, Box<dyn std::error::Error>> {
    let mut data = json!({"type":"anthropicCompaction","block":{"type":"compaction","content":"signed","signature":"sig"}});
    if let (Some(data), Some(extra)) = (data.as_object_mut(), extra.as_object()) {
        data.extend(extra.clone());
    }
    let mut payload = ProviderContinuation::with_limit(
        Protocol::Anthropic,
        data,
        crate::COMPACTION_PAYLOAD_BYTE_LIMIT,
    )?;
    payload.provider_id = Some("fixture".into());
    payload.model_id = "claude-sonnet-4-6".into();
    Ok(Box::new(payload))
}

#[test]
fn signed_summary_files_wait_for_message_times_not_positions(
) -> Result<(), Box<dyn std::error::Error>> {
    // Summary committed at 11, retained call at 10 and its result at 12, then a new user
    // message at 13: the metadata waits past the result instead of splitting the pair.
    let mut summary = CompletionMessage::text(MessageRole::User, "decorated summary");
    summary.continuation = Some(signed_payload(
        json!({"filesText":"files","exactTail":true,"retainedTail":0}),
    )?);
    let mut call = CompletionMessage::text(MessageRole::Assistant, "retained call");
    call.assistant_tool_calls = Some(vec![crate::AssistantToolCall {
        tool_call_id: "c".into(),
        function_name: "read".into(),
        arguments_json: "{}".into(),
    }]);
    let mut result = CompletionMessage::text(MessageRole::Tool, "result");
    result.tool_call_id = Some("c".into());
    let mut request = CompletionRequest {
        provider_id: Some("fixture".into()),
        model_id: "claude-sonnet-4-6".into(),
        messages: vec![
            summary,
            call,
            result,
            CompletionMessage::text(MessageRole::User, "new"),
        ],
        ..Default::default()
    };
    let texts = |request: &CompletionRequest| -> Result<Vec<String>, Box<dyn std::error::Error>> {
        let body = crate::wire::encode(request, Protocol::Anthropic)?;
        Ok(body["messages"]
            .as_array()
            .into_iter()
            .flatten()
            .map(|m| {
                m["content"][0]["text"]
                    .as_str()
                    .unwrap_or("<block>")
                    .to_owned()
            })
            .collect())
    };
    request.message_times_ms = vec![Some(11), Some(10), Some(12), Some(13)];
    assert_eq!(texts(&request)?, ["<block>", "<block>", "files", "new"]);
    // Without times only positions are known: the old behavior is the fallback.
    request.message_times_ms.clear();
    assert_eq!(
        texts(&request)?,
        ["<block>", "files", "retained call", "<block>", "new"]
    );
    Ok(())
}

#[test]
fn carried_file_metadata_is_split_at_the_due_cutoff() -> Result<(), Box<dyn std::error::Error>> {
    use crate::FilesDue;
    let data = json!({"filesText":"own","retainedTail":3,"retainedFiles":[{"text":"prior","after":1,"at":20}]});
    // (summary time, due, expected carried texts, expected left in the prefix request)
    let cases = [
        (Some(15), FilesDue::Before(10), vec!["own", "prior"], 0),
        (Some(15), FilesDue::Before(18), vec!["prior"], 1),
        (Some(15), FilesDue::Nothing, vec!["own", "prior"], 0),
        (Some(15), FilesDue::All, vec![], 2),
    ];
    for (summary_at, due, carried, left) in cases {
        let mut payload = signed_payload(data.clone())?;
        let mut kept: Vec<_> = payload
            .retain_anthropic_files(summary_at, due, 0, 2, 5)?
            .into_iter()
            .map(|file| file.text)
            .collect();
        kept.sort_by_key(|text| text.len());
        let mut want: Vec<String> = carried.into_iter().map(str::to_owned).collect();
        want.sort_by_key(String::len);
        assert_eq!(kept, want, "{due:?}");
        let remaining = payload.export().data["retainedFiles"]
            .as_array()
            .map_or(0, Vec::len);
        assert_eq!(remaining, left, "{due:?}");
    }
    Ok(())
}

#[tokio::test]
async fn v2_identity_keeps_cache_routing_and_request_ids_apart(
) -> Result<(), Box<dyn std::error::Error>> {
    let completed = "data: {\"type\":\"response.output_item.done\",\"item\":{\"type\":\"compaction\"}}\n\ndata: {\"type\":\"response.completed\",\"response\":{}}\n\n";
    let (endpoint, task) = scripted_server(vec![(200, completed.into())]).await?;
    let provider = HttpProvider::new(&endpoint, Protocol::Responses, Duration::from_secs(5))?;
    let mut request = CompletionRequest {
        provider_id: Some("fixture".into()),
        model_id: "fixture".into(),
        messages: vec![CompletionMessage::text(MessageRole::User, "q")],
        ..Default::default()
    };
    request.context.session_id = Some("side-session".into());
    request.context.request_id = Some("request-9".into());
    provider
        .native_compact(
            NativeCompactionRequest {
                request,
                lanes: vec![NativeCompactionLane::OpenAiV2],
                retained_message_budget: 100,
                max_tokens: 10,
                settings: enabled(),
                cache_key: Some("main-session".into()),
                ..Default::default()
            },
            CancellationToken::new(),
        )
        .await?;
    let exchanges = task.await??;
    let (headers, body) = exchanges.first().ok_or("no request")?;
    let headers = headers.to_lowercase();
    assert_eq!(body["prompt_cache_key"], "main-session");
    assert!(headers.contains("session_id: side-session"));
    assert!(headers.contains("x-client-request-id: request-9"));
    Ok(())
}

#[test]
fn signed_summary_requests_keep_live_controls_and_drop_unsupported_fields() {
    use super::super::anthropic::{apply_saved_controls, strip_anthropic};
    let saved = CompletionRequest {
        provider_id: Some("fixture".into()),
        model_id: "claude-sonnet-4-6".into(),
        reasoning_effort: Some("high".into()),
        thinking: Some(json!({"type":"adaptive"})),
        ..Default::default()
    };
    let mut fresh = CompletionRequest {
        provider_id: Some("fixture".into()),
        model_id: "claude-sonnet-4-6".into(),
        ..Default::default()
    };
    apply_saved_controls(&mut fresh, &saved);
    assert_eq!(fresh.reasoning_effort.as_deref(), Some("high"));
    // Another model's controls are never borrowed.
    let mut other = CompletionRequest {
        model_id: "other".into(),
        ..Default::default()
    };
    apply_saved_controls(&mut other, &saved);
    assert!(other.reasoning_effort.is_none());
    let mut body = json!({"stop_sequences":["HALT"],"context_management":{"edits":[]},"tool_choice":{"type":"any"},
        "output_config":{"format":{"type":"json"},"task_budget":{"total":4096,"remaining":1024}}});
    strip_anthropic(&mut body);
    assert_eq!(
        body,
        json!({"tool_choice":{"type":"auto"},"output_config":{"task_budget":{"total":4096}}})
    );
}
