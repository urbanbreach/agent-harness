use super::*;
use crate::{CacheRetention, CompletionMessage, MessageRole};
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt};
use tokio_stream::StreamExt;

#[tokio::test]
async fn extended_cache_retention_is_sent_only_to_direct_openai(
) -> Result<(), Box<dyn std::error::Error>> {
    for (host, protocol, retention, extended) in [
        (
            "api.openai.com",
            Protocol::Responses,
            CacheRetention::Long,
            true,
        ),
        ("api.openai.com", Protocol::Chat, CacheRetention::Long, true),
        (
            "api.openai.com",
            Protocol::Chat,
            CacheRetention::Short,
            false,
        ),
        (
            "api.openai.com",
            Protocol::Chat,
            CacheRetention::None,
            false,
        ),
        (
            "proxy.example.test",
            Protocol::Chat,
            CacheRetention::Long,
            false,
        ),
    ] {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
        let address = listener.local_addr()?;
        let server = tokio::spawn(capture_request(listener, protocol));
        let mut provider = HttpProvider::new(
            &format!("http://{host}:{}/v1/responses", address.port()),
            protocol,
            Duration::from_secs(2),
        )?;
        // Exercise the real host check and HTTP body while every connection stays on loopback.
        provider.client = reqwest::Client::builder()
            .no_proxy()
            .resolve(host, address)
            .timeout(Duration::from_secs(2))
            .build()?;
        let mut request = CompletionRequest {
            model_id: "gpt-5.4".into(),
            messages: vec![CompletionMessage::text(MessageRole::User, "hello")],
            ..Default::default()
        };
        request.context.cache_retention = retention;
        request.context.session_id = Some("cache-session".into());
        let events = provider.stream(request).collect::<Vec<_>>().await;
        assert!(
            matches!(events.last(), Some(Event::DoneWithMetadata { .. })),
            "{events:?}"
        );
        let body = server.await??;
        assert_eq!(
            body.get("prompt_cache_retention")
                .and_then(serde_json::Value::as_str),
            extended.then_some("24h")
        );
        assert_eq!(
            body.get("prompt_cache_key")
                .and_then(serde_json::Value::as_str),
            (retention != CacheRetention::None).then_some("cache-session")
        );
    }
    Ok(())
}

async fn capture_request(
    listener: tokio::net::TcpListener,
    protocol: Protocol,
) -> std::io::Result<serde_json::Value> {
    let (connection, _) = listener.accept().await?;
    let mut reader = tokio::io::BufReader::new(connection);
    let mut length = None;
    let mut header_bytes = 0;
    loop {
        let mut line = String::new();
        let count = reader.read_line(&mut line).await?;
        header_bytes += count;
        if count == 0 || header_bytes > 16_384 {
            return Err(std::io::Error::other("invalid request headers"));
        }
        if line == "\r\n" {
            break;
        }
        if let Some(value) = line.to_ascii_lowercase().strip_prefix("content-length:") {
            length = Some(
                value
                    .trim()
                    .parse::<usize>()
                    .map_err(std::io::Error::other)?,
            );
        }
    }
    let length = length
        .filter(|length| *length < 16_384)
        .ok_or_else(|| std::io::Error::other("invalid request length"))?;
    let mut body = vec![0; length];
    reader.read_exact(&mut body).await?;
    let events = if protocol == Protocol::Chat {
        "data: {\"choices\":[{\"delta\":{},\"finish_reason\":\"stop\"}]}\n\ndata: [DONE]\n\n"
    } else {
        "data: {\"type\":\"response.completed\",\"response\":{\"status\":\"completed\"}}\n\n"
    };
    reader.get_mut().write_all(format!("HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{events}", events.len()).as_bytes()).await?;
    serde_json::from_slice::<serde_json::Value>(&body).map_err(std::io::Error::other)
}
