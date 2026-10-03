use super::*;
use crate::ProviderStreamEvent;
use crate::{CompletionMessage, MessageRole, ProviderErrorCategory};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio_stream::StreamExt;

#[tokio::test]
async fn http_stream_reports_truncation_and_keeps_server_errors_private(
) -> Result<(), Box<dyn std::error::Error>> {
    for (status, content_type, body, category) in [
        (
            "200 OK",
            "Content-Type: text/event-stream\r\n",
            "data: {\"choices\":[{\"delta\":{\"content\":\"hi\"}}]}\n\n",
            Some(ProviderErrorCategory::MalformedStream),
        ),
        (
            "401 Unauthorized",
            "Content-Type: text/event-stream\r\n",
            "secret-server-body",
            Some(ProviderErrorCategory::InvalidCredentials),
        ),
        (
            "200 OK",
            "",
            "data: {\"choices\":[{\"delta\":{},\"finish_reason\":\"stop\"}]}\n\ndata: [DONE]\n\n",
            None,
        ),
        (
            "200 OK",
            "Content-Type: text/html\r\n",
            "data: [DONE]\n\n",
            Some(ProviderErrorCategory::MalformedStream),
        ),
    ] {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
        let (protocol, path) = if status.starts_with("401") {
            (Protocol::Responses, "/v1/responses")
        } else {
            (Protocol::Chat, "/v1/chat/completions")
        };
        let endpoint = format!("http://{}{path}", listener.local_addr()?);
        let server = tokio::spawn(reply(listener, status, content_type, body, path));
        let provider =
            HttpProvider::new(&endpoint, protocol, Duration::from_secs(2))?.with_chat_fallback();
        let request = CompletionRequest {
            model_id: "fixture".into(),
            messages: vec![CompletionMessage::text(MessageRole::User, "hello")],
            ..Default::default()
        };
        let events = provider.stream(request).collect::<Vec<_>>().await;
        match category {
            Some(category) => assert!(
                matches!(events.last(), Some(ProviderStreamEvent::Error { category: Some(actual), .. }) if *actual == category)
            ),
            None => assert!(matches!(
                events.last(),
                Some(ProviderStreamEvent::DoneWithMetadata { .. })
            )),
        }
        assert!(!format!("{events:?}").contains("secret-server-body"));
        server.await??;
    }
    assert!(HttpProvider::new(
        "http://name:password@localhost/",
        Protocol::Chat,
        Duration::from_secs(2)
    )
    .is_err());
    Ok(())
}

#[tokio::test]
async fn http_completions_finalize_only_complete_logical_reasoning(
) -> Result<(), Box<dyn std::error::Error>> {
    for (body, expected, expected_usage_complete, expected_delta) in [
        (
            concat!(
                "data: {\"choices\":[{\"delta\":{\"reasoning_content\":\"first \"}}]}\n\n",
                "data: {\"choices\":[{\"delta\":{\"reasoning_content\":\"second\"}}]}\n\n",
                "data: {\"choices\":[{\"delta\":{},\"finish_reason\":\"stop\"}]}\n\n",
                "data: {\"choices\":[],\"usage\":{\"prompt_tokens\":4,\"completion_tokens\":2,\"total_tokens\":6}}\n\n",
                "data: [DONE]\n\n"
            ),
            Some(vec!["first second".to_owned()]),
            Some(true),
            None,
        ),
        (
            "data: {\"choices\":[{\"delta\":{\"reasoning_content\":\"partial\"},\"finish_reason\":\"length\"}]}\n\ndata: [DONE]\n\n",
            None,
            None,
            None,
        ),
        (
            "data: {\"choices\":[{\"delta\":{},\"finish_reason\":\"stop\"}],\"usage\":{\"prompt_tokens\":4}}\n\ndata: [DONE]\n\n",
            Some(Vec::new()),
            Some(false),
            None,
        ),
        (
            "data: {\"choices\":[{\"delta\":{\"reasoning_content\":null,\"reasoning\":\"fallback-null\"},\"finish_reason\":\"stop\"}]}\n\ndata: [DONE]\n\n",
            Some(vec!["fallback-null".to_owned()]),
            None,
            Some("fallback-null"),
        ),
        (
            "data: {\"choices\":[{\"delta\":{\"reasoning_content\":{\"type\":\"structured\"},\"reasoning\":\"fallback-structured\"},\"finish_reason\":\"stop\"}]}\n\ndata: [DONE]\n\n",
            None,
            None,
            Some("fallback-structured"),
        ),
        (
            "data: {\"choices\":[{\"delta\":{\"reasoning_content\":null},\"finish_reason\":\"stop\"}],\"usage\":{\"prompt_tokens\":0,\"completion_tokens\":0,\"total_tokens\":0}}\n\ndata: [DONE]\n\n",
            Some(Vec::new()),
            Some(true),
            None,
        ),
    ] {
        let events = http_fixture_events(Protocol::Chat, body).await?;
        assert!(events.iter().any(
            |event| matches!(event, ProviderStreamEvent::DoneWithMetadata { .. })
        ));
        if let Some(text) = expected_delta {
            assert!(events.contains(&ProviderStreamEvent::ReasoningDelta(text.into())));
        }
        assert_eq!(settled_reasoning(&events), expected);
        assert_eq!(usage_complete(&events), expected_usage_complete);
    }

    let responses = http_fixture_events(
        Protocol::Responses,
        concat!(
            "data: {\"type\":\"response.reasoning_summary_text.delta\",\"delta\":\"summary\"}\n\n",
            "data: {\"type\":\"response.completed\",\"response\":{\"status\":\"completed\",\"output\":[{\"type\":\"reasoning\",\"summary\":[{\"type\":\"summary_text\",\"text\":\"summary\"}],\"encrypted_content\":\"opaque\"}],\"usage\":{\"input_tokens\":3,\"output_tokens\":2}}}\n\n"
        ),
    )
    .await?;
    assert_eq!(settled_reasoning(&responses), None);
    assert_eq!(usage_complete(&responses), Some(true));

    let anthropic = http_fixture_events(
        Protocol::Anthropic,
        concat!(
            "data: {\"type\":\"message_start\",\"message\":{\"usage\":{\"input_tokens\":0}}}\n\n",
            "data: {\"type\":\"content_block_start\",\"index\":0,\"content_block\":{\"type\":\"thinking\",\"thinking\":\"\"}}\n\n",
            "data: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"thinking_delta\",\"thinking\":\"first\"}}\n\n",
            "data: {\"type\":\"content_block_stop\",\"index\":0}\n\n",
            "data: {\"type\":\"content_block_start\",\"index\":1,\"content_block\":{\"type\":\"thinking\",\"thinking\":\"\"}}\n\n",
            "data: {\"type\":\"content_block_delta\",\"index\":1,\"delta\":{\"type\":\"thinking_delta\",\"thinking\":\"second\"}}\n\n",
            "data: {\"type\":\"content_block_stop\",\"index\":1}\n\n",
            "data: {\"type\":\"message_delta\",\"delta\":{\"stop_reason\":\"end_turn\"},\"usage\":{\"output_tokens\":0}}\n\n",
            "data: {\"type\":\"message_stop\"}\n\n"
        ),
    )
    .await?;
    assert_eq!(
        settled_reasoning(&anthropic),
        Some(vec!["first".into(), "second".into()])
    );
    assert_eq!(usage_complete(&anthropic), Some(true));

    let signed_anthropic = http_fixture_events(
        Protocol::Anthropic,
        concat!(
            "data: {\"type\":\"content_block_start\",\"index\":0,\"content_block\":{\"type\":\"thinking\",\"thinking\":\"\"}}\n\n",
            "data: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"thinking_delta\",\"thinking\":\"thought\"}}\n\n",
            "data: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"signature_delta\",\"signature\":\"fixture-signature\"}}\n\n",
            "data: {\"type\":\"content_block_stop\",\"index\":0}\n\n",
            "data: {\"type\":\"message_delta\",\"delta\":{\"stop_reason\":\"end_turn\"}}\n\n",
            "data: {\"type\":\"message_stop\"}\n\n"
        ),
    )
    .await?;
    assert_eq!(settled_reasoning(&signed_anthropic), None);

    let truncated_anthropic = http_fixture_events(
        Protocol::Anthropic,
        concat!(
            "data: {\"type\":\"content_block_start\",\"index\":0,\"content_block\":{\"type\":\"thinking\",\"thinking\":\"\"}}\n\n",
            "data: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"thinking_delta\",\"thinking\":\"partial\"}}\n\n",
            "data: {\"type\":\"content_block_stop\",\"index\":0}\n\n",
            "data: {\"type\":\"message_delta\",\"delta\":{\"stop_reason\":\"max_tokens\"}}\n\n",
            "data: {\"type\":\"message_stop\"}\n\n"
        ),
    )
    .await?;
    assert_eq!(settled_reasoning(&truncated_anthropic), None);

    for body in [
        "data: {\"choices\":[{\"delta\":{\"reasoning_content\":\"unfinished\"}}]}\n\n",
        "data: {\"choices\":[{\"delta\":{\"reasoning_content\":\"unfinished\"},\"finish_reason\":\"stop\"}]}\n\ndata: {not-json}\n\n",
    ] {
        let events = http_fixture_events(Protocol::Chat, body).await?;
        assert!(matches!(
            events.last(),
            Some(ProviderStreamEvent::Error {
                category: Some(ProviderErrorCategory::MalformedStream),
                ..
            })
        ));
        assert!(!events.iter().any(
            |event| matches!(event, ProviderStreamEvent::DoneWithMetadata { .. })
        ));
    }
    Ok(())
}

#[tokio::test]
async fn anthropic_post_stop_reasoning_is_not_settled() -> Result<(), Box<dyn std::error::Error>> {
    let events = http_fixture_events(
        Protocol::Anthropic,
        concat!(
            "data: {\"type\":\"content_block_start\",\"index\":0,\"content_block\":{\"type\":\"thinking\",\"thinking\":\"\"}}\n\n",
            "data: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"thinking_delta\",\"thinking\":\"before-stop\"}}\n\n",
            "data: {\"type\":\"content_block_stop\",\"index\":0}\n\n",
            "data: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"thinking_delta\",\"thinking\":\"after-stop\"}}\n\n",
            "data: {\"type\":\"message_delta\",\"delta\":{\"stop_reason\":\"end_turn\"}}\n\n",
            "data: {\"type\":\"message_stop\"}\n\n"
        ),
    )
    .await?;
    assert!(events.contains(&ProviderStreamEvent::ReasoningDelta("after-stop".into())));
    assert_eq!(settled_reasoning(&events), None);
    Ok(())
}

fn settled_reasoning(events: &[ProviderStreamEvent]) -> Option<Vec<String>> {
    events.iter().find_map(|event| match event {
        ProviderStreamEvent::DoneWithMetadata { metadata, .. } => metadata
            .as_ref()
            .and_then(|metadata| metadata.settled_reasoning.clone()),
        _ => None,
    })
}

fn usage_complete(events: &[ProviderStreamEvent]) -> Option<bool> {
    events.iter().find_map(|event| match event {
        ProviderStreamEvent::DoneWithMetadata { metadata, .. } => metadata
            .as_ref()
            .and_then(|metadata| metadata.usage_complete),
        _ => None,
    })
}

async fn http_fixture_events(
    protocol: Protocol,
    body: &'static str,
) -> Result<Vec<ProviderStreamEvent>, Box<dyn std::error::Error>> {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let path = match protocol {
        Protocol::Chat => "/v1/chat/completions",
        Protocol::Responses => "/v1/responses",
        Protocol::Anthropic => "/v1/messages",
    };
    let endpoint = format!("http://{}{path}", listener.local_addr()?);
    let server = tokio::spawn(reply(listener, "200 OK", "", body, path));
    let provider = HttpProvider::new(&endpoint, protocol, Duration::from_secs(2))?;
    let request = CompletionRequest {
        model_id: "fixture".into(),
        messages: vec![CompletionMessage::text(MessageRole::User, "hello")],
        ..Default::default()
    };
    let events = provider.stream(request).collect::<Vec<_>>().await;
    server.await??;
    Ok(events)
}

async fn reply(
    listener: tokio::net::TcpListener,
    status: &str,
    content_type: &str,
    body: &str,
    path: &str,
) -> std::io::Result<()> {
    let (mut connection, _) = listener.accept().await?;
    let mut request = vec![0; 8192];
    let count = connection.read(&mut request).await?;
    if !request[..count].starts_with(format!("POST {path} ").as_bytes()) {
        return Err(std::io::Error::other("expected completion request"));
    }
    let reply = format!(
        "HTTP/1.1 {status}\r\n{content_type}Content-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
    connection.write_all(reply.as_bytes()).await
}
