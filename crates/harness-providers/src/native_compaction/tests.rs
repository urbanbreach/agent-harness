use super::*;
use crate::{MessageRole, Provider, ProviderErrorEnvelope, RemoteModelOverride};

/// Lanes open for any provider: the remote switch on and the model opted in.
fn enabled() -> NativeLaneSettings {
    NativeLaneSettings {
        remote_enabled: true,
        v2_enabled: true,
        model: Some(RemoteModelOverride {
            enabled: Some(true),
            ..Default::default()
        }),
        ..Default::default()
    }
}
use tokio::io::{AsyncReadExt, AsyncWriteExt};
mod gates;
mod parity;
struct Credentials;
#[async_trait::async_trait]
impl crate::ProviderCredentialSource for Credentials {
    async fn bearer_token(
        &self,
    ) -> Result<crate::ProviderBearerToken, crate::ProviderCredentialError> {
        Ok(crate::ProviderBearerToken {
            token: "fixture-key".into(),
            kind: crate::ProviderCredentialKind::InlineApiKey,
            account_id: Some("account".into()),
            enterprise_url: None,
        })
    }
}
async fn server(
    response: String,
) -> Result<
    (
        String,
        tokio::task::JoinHandle<Result<(String, Value), std::io::Error>>,
    ),
    std::io::Error,
> {
    let (endpoint, task) = scripted_server(vec![(200, response)]).await?;
    Ok((
        endpoint,
        tokio::spawn(async move {
            task.await
                .map_err(std::io::Error::other)?
                .and_then(|mut exchanges| {
                    exchanges
                        .pop()
                        .ok_or_else(|| std::io::Error::other("missing exchange"))
                })
        }),
    ))
}
async fn scripted_server(
    responses: Vec<(u16, String)>,
) -> Result<
    (
        String,
        tokio::task::JoinHandle<Result<Vec<(String, Value)>, std::io::Error>>,
    ),
    std::io::Error,
> {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let endpoint = format!("http://{}/v1/responses", listener.local_addr()?);
    let task = tokio::spawn(async move {
        let mut exchanges = Vec::new();
        for (status, response) in responses {
            let (mut socket, _) = listener.accept().await?;
            let bytes = read_request(&mut socket).await?;
            socket.write_all(format!("HTTP/1.1 {status} Fixture\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{response}", response.len()).as_bytes()).await?;
            let offset = bytes
                .windows(4)
                .position(|w| w == b"\r\n\r\n")
                .ok_or_else(|| std::io::Error::other("missing headers"))?;
            exchanges.push((
                String::from_utf8_lossy(&bytes[..offset]).into_owned(),
                serde_json::from_slice(&bytes[offset + 4..]).map_err(std::io::Error::other)?,
            ));
        }
        Ok(exchanges)
    });
    Ok((endpoint, task))
}
async fn read_request(socket: &mut tokio::net::TcpStream) -> Result<Vec<u8>, std::io::Error> {
    let mut bytes = Vec::new();
    loop {
        let mut chunk = [0; 8192];
        let n = socket.read(&mut chunk).await?;
        if n == 0 {
            break;
        }
        bytes.extend_from_slice(&chunk[..n]);
        if let Some(offset) = bytes.windows(4).position(|w| w == b"\r\n\r\n") {
            let headers = String::from_utf8_lossy(&bytes[..offset]);
            let length = headers
                .lines()
                .find_map(|line| {
                    line.to_lowercase()
                        .strip_prefix("content-length:")
                        .and_then(|s| s.trim().parse::<usize>().ok())
                })
                .unwrap_or(0);
            if bytes.len() >= offset + 4 + length {
                break;
            }
        }
    }
    Ok(bytes)
}
#[tokio::test]
async fn native_responses_wire_and_replay() -> Result<(), Box<dyn std::error::Error>> {
    for lane in [
        NativeCompactionLane::OpenAiV1,
        NativeCompactionLane::OpenAiV2,
    ] {
        let response = if lane == NativeCompactionLane::OpenAiV1 {
            json!({"output":[{"type":"compaction","encrypted_content":""}]}).to_string()
        } else {
            "data: {\"type\":\"response.output_item.done\",\"item\":{\"type\":\"compaction\"}}\n\ndata: {\"type\":\"response.completed\",\"response\":{\"usage\":{\"input_tokens\":123,\"output_tokens\":4,\"total_tokens\":127,\"input_tokens_details\":{\"cached_tokens\":7},\"output_tokens_details\":{\"reasoning_tokens\":1}}}}\n\n".into()
        };
        let (endpoint, task) = scripted_server(vec![
            (200, response),
            (
                200,
                "data: {\"type\":\"response.completed\",\"response\":{\"output\":[]}}\n\n".into(),
            ),
        ])
        .await?;
        let provider = HttpProvider::new(&endpoint, Protocol::Responses, Duration::from_secs(5))?
            .with_credentials(std::sync::Arc::new(Credentials));
        let request = CompletionRequest {
            provider_id: Some("fixture".into()),
            model_id: "fixture".into(),
            messages: vec![
                CompletionMessage::text(MessageRole::System, "live system"),
                CompletionMessage::text(MessageRole::User, "question"),
            ],
            ..Default::default()
        };
        let result = provider
            .native_compact(
                NativeCompactionRequest {
                    request: request.clone(),
                    lanes: vec![lane],
                    instructions: "ignored focus".into(),
                    retained_message_budget: 2,
                    context_window: None,
                    max_tokens: 100,
                    retained: vec![CompletionMessage::text(
                        MessageRole::User,
                        "0123456789abcdef",
                    )],
                    settings: enabled(),
                    ..Default::default()
                },
                CancellationToken::new(),
            )
            .await?;
        let mut summary = CompletionMessage::text(MessageRole::User, "placeholder");
        summary.continuation = Some(result.replay.clone());
        let mut http_replay = request.clone();
        http_replay.messages = vec![summary];
        use tokio_stream::StreamExt;
        let _events = provider
            .stream_completion(http_replay)
            .await
            .collect::<Vec<_>>()
            .await;
        let exchanges = task.await??;
        let (headers, body) = &exchanges[0];
        assert!(headers
            .to_lowercase()
            .contains("authorization: bearer fixture-key"));
        assert!(headers
            .to_lowercase()
            .contains("content-type: application/json"));
        assert_eq!(exchanges[1].1["input"], result.replay.data["items"]);
        assert_eq!(body["instructions"], "live system");
        assert_eq!(
            body,
            &if lane == NativeCompactionLane::OpenAiV1 {
                json!({"model":"fixture","input":[{"type":"message","role":"user","content":[{"type":"input_text","text":"question"}]}],"instructions":"live system"})
            } else {
                json!({"model":"fixture","input":[{"type":"message","role":"user","content":[{"type":"input_text","text":"question"}]},{"type":"compaction_trigger"}],"instructions":"live system","stream":true,"store":false})
            }
        );
        if lane == NativeCompactionLane::OpenAiV1 {
            assert!(headers.starts_with("POST /v1/responses/compact "));
            assert_eq!(body.as_object().map(|o| o.len()), Some(3));
        } else {
            assert_eq!(
                body["input"].as_array().and_then(|a| a.last()),
                Some(&json!({"type":"compaction_trigger"}))
            );
            assert_eq!(body["store"], false);
            assert_eq!(result.used_tokens, 123);
            assert_eq!(result.replay.data["usage"]["cachedInputTokens"], 7);
            assert_eq!(result.replay.data["usage"]["reasoningOutputTokens"], 1);
        }
        let mut summary = CompletionMessage::text(MessageRole::User, "placeholder");
        summary.continuation = Some(result.replay);
        let mut replay = request;
        replay.model_id = "another-model".into();
        replay.messages = vec![summary];
        let encoded = crate::wire::encode(&replay, Protocol::Responses)?;
        assert_eq!(
            encoded["input"]
                .as_array()
                .and_then(|a| a.last())
                .map(|v| &v["type"]),
            Some(&json!("compaction"))
        );
        replay.provider_id = Some("foreign".into());
        assert_eq!(
            crate::wire::encode(&replay, Protocol::Responses)?["input"][0]["content"],
            "placeholder"
        );
    }
    Ok(())
}
#[tokio::test]
async fn custom_formats_and_auth_policy() -> Result<(), Box<dyn std::error::Error>> {
    let credential = CompactionCredential {
        api_key: Some("key".into()),
        headers: [("x-model-route".to_owned(), "m1".to_owned())].into(),
    };
    for chat in [false, true] {
        let response = if chat {
            json!({"choices":[{"message":{"content":[{"text":"a"},{"type":"other","text":"b"}]}}]})
        } else {
            json!({"summary":"","shortSummary":"display","usage":{"n":1}})
        };
        let (endpoint, task) = server(response.to_string()).await?;
        let endpoint = if chat {
            endpoint.replace("responses", "chat/completions/?fixture=1")
        } else {
            endpoint
        };
        let result = custom_endpoint_summary(
            &endpoint,
            Some(&credential),
            Some("wire"),
            "system",
            "prompt",
            Some(16384),
            CancellationToken::new(),
        )
        .await?;
        let (headers, body) = task.await??;
        let headers = headers.to_lowercase();
        // Only the compatible format carries the key and the model headers.
        assert_eq!(headers.contains("authorization: bearer key"), chat);
        assert_eq!(headers.contains("x-model-route: m1"), chat);
        assert!(headers.contains("content-type: application/json"));
        if chat {
            assert_eq!(result.summary, "ab");
            assert_eq!(result.short_summary, None);
            assert_eq!(
                body,
                json!({"model":"wire","messages":[{"role":"system","content":"system"},{"role":"user","content":"prompt"}],"stream":false,"max_tokens":16384})
            );
        } else {
            assert_eq!(result.summary, "");
            assert_eq!(result.short_summary.as_deref(), Some("display"));
            assert_eq!(result.extra.get("usage"), Some(&json!({"n":1})));
            assert_eq!(
                body,
                json!({"systemPrompt":"system","prompt":"prompt","maxTokens":16384})
            );
        }
    }
    Ok(())
}
#[tokio::test]
async fn custom_endpoint_failures_use_the_documented_messages(
) -> Result<(), Box<dyn std::error::Error>> {
    let chat_missing = json!({"choices":[{"message":{"content":""}}]}).to_string();
    for (status, body, chat, message, http_status) in [
        (
            401,
            "denied sk-abcdefghijklmnopqrstuvwxyz".to_owned(),
            false,
            "Remote compaction failed (401 Unauthorized)",
            Some(401),
        ),
        (
            200,
            chat_missing,
            true,
            "Remote compaction response missing choices[0].message.content",
            None,
        ),
        (
            200,
            json!({"summary":1}).to_string(),
            false,
            "Remote compaction response missing summary",
            None,
        ),
    ] {
        let (endpoint, task) = scripted_server(vec![(status, body)]).await?;
        let endpoint = if chat {
            endpoint.replace("responses", "chat/completions")
        } else {
            endpoint
        };
        let error = custom_endpoint_summary(
            &endpoint,
            None,
            None,
            "s",
            "p",
            None,
            CancellationToken::new(),
        )
        .await
        .err()
        .ok_or("expected failure")?;
        task.await??;
        assert_eq!(
            (error.message.as_str(), error.status),
            (message, http_status)
        );
        // The body words stay out of the message, and secrets stay out of the cause.
        assert!(error
            .cause
            .as_deref()
            .is_none_or(|cause| !cause.contains("abcdefghijklmnop")));
    }
    Ok(())
}
#[test]
fn retention_and_failure_classification() {
    let candidates = vec![
        json!({"role":"user","content":[{"type":"input_text","text":"<environment_context>hidden"}]}),
        json!({"role":"user","content":[{"type":"input_text","text":"01234567890123456789"}]}),
    ];
    let retained = retention::retain(&candidates, 2, json!({"type":"compaction"}));
    assert_eq!(
        retained,
        vec![
            json!({"role":"user","content":[{"type":"input_text","text":"01234567"}]}),
            json!({"type":"compaction"})
        ]
    );
    let images = vec![
        json!({"role":"user","content":[{"type":"input_text","text":"earlier"}]}),
        json!({"role":"user","content":[{"type":"input_image","image_url":"image"}]}),
    ];
    for (budget, expected) in [(764, 0), (765, 1)] {
        let history = retention::retain(&images, budget, json!({"type":"compaction"}));
        assert_eq!(
            history
                .iter()
                .flat_map(|v| v["content"].as_array().into_iter().flatten())
                .filter(|p| p["type"] == "input_image")
                .count(),
            expected
        );
    }
    let large =
        vec![json!({"role":"user","content":[{"type":"input_text","text":"x".repeat(400000)}]})];
    let capped = retention::retain(&large, 100000, json!({"type":"compaction"}));
    assert_eq!(
        capped[0]["content"][0]["text"]
            .as_str()
            .map(|s| s.encode_utf16().count()),
        Some(256000)
    );
    assert!(capped[0]["content"][0]["text"]
        .as_str()
        .is_some_and(|s| s.contains("36000 tokens truncated")));
    for (status, text, auth, retry) in [
        (503, "auth_unavailable", true, false),
        (500, "server_error", false, true),
        (400, "context too large", false, false),
    ] {
        let e = NativeCompactionError::lane_http(ProviderErrorEnvelope {
            status: Some(status),
            body: Some(text.into()),
            ..Default::default()
        });
        assert_eq!((e.auth, e.transient()), (auth, retry));
    }
}
