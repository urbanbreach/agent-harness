use super::LIMIT;
use futures_util::{StreamExt, TryStreamExt};
use harness_core::tool::ToolError;
use reqwest::header::{HeaderName, HeaderValue};
use rmcp::{
    model::ClientJsonRpcMessage,
    transport::{
        common::client_side_sse::BoxedSseResponse,
        streamable_http_client::{
            StreamableHttpClient, StreamableHttpError, StreamableHttpPostResponse,
        },
    },
};
use std::{collections::HashMap, sync::Arc};

#[derive(Clone)]
pub(super) struct Client(pub reqwest::Client);
type Error = StreamableHttpError<ToolError>;

impl StreamableHttpClient for Client {
    type Error = ToolError;
    async fn post_message(
        &self,
        uri: Arc<str>,
        message: ClientJsonRpcMessage,
        session: Option<Arc<str>>,
        auth: Option<String>,
        headers: HashMap<HeaderName, HeaderValue>,
    ) -> Result<StreamableHttpPostResponse, Error> {
        let mut request = self
            .0
            .post(uri.as_ref())
            .header("accept", "application/json, text/event-stream");
        if let Some(session) = session {
            request = request.header("mcp-session-id", session.as_ref());
        }
        if let Some(auth) = auth {
            request = request.bearer_auth(auth);
        }
        for (key, value) in headers {
            request = request.header(key, value);
        }
        let mut response = request
            .json(&message)
            .send()
            .await
            .map_err(|_| Error::Client(super::failure("MCP HTTP request failed")))?;
        if response.status() == reqwest::StatusCode::ACCEPTED {
            return Ok(StreamableHttpPostResponse::Accepted);
        }
        // An expired session must be reported without replaying a potentially mutating call.
        if !response.status().is_success() {
            return Err(Error::Client(ToolError::HttpStatus {
                status: response.status().as_u16(),
                retry_after_ms: response
                    .headers()
                    .get("retry-after")
                    .and_then(|v| v.to_str().ok())
                    .and_then(|v| v.parse::<u64>().ok())
                    .map(|s| s.saturating_mul(1000)),
            }));
        }
        let session = response
            .headers()
            .get("mcp-session-id")
            .map(|v| v.to_str().map(str::to_owned))
            .transpose()
            .map_err(|_| Error::UnexpectedServerResponse("invalid MCP session header".into()))?;
        let content_type = response
            .headers()
            .get("content-type")
            .and_then(|v| v.to_str().ok())
            .unwrap_or_default();
        if content_type.starts_with("text/event-stream") {
            // A call's entire SSE response is bounded, including progress notifications.
            let mut remaining = LIMIT;
            let bytes = response
                .bytes_stream()
                .map_err(std::io::Error::other)
                .and_then(move |chunk| {
                    let result = remaining.checked_sub(chunk.len()).map_or_else(
                        || Err(std::io::Error::other("MCP response exceeds 1 MiB")),
                        |left| {
                            remaining = left;
                            Ok(chunk)
                        },
                    );
                    std::future::ready(result)
                });
            return Ok(StreamableHttpPostResponse::Sse(
                sse_stream::SseStream::from_bytes_stream(bytes).boxed(),
                session,
            ));
        }
        if !content_type.starts_with("application/json") {
            return Err(Error::UnexpectedServerResponse(
                "MCP response must be JSON or SSE".into(),
            ));
        }
        let mut body = Vec::new();
        while let Some(chunk) = response
            .chunk()
            .await
            .map_err(|_| Error::Client(super::failure("MCP response read failed")))?
        {
            if body.len().saturating_add(chunk.len()) > LIMIT {
                return Err(Error::UnexpectedServerResponse(
                    "MCP response exceeds 1 MiB".into(),
                ));
            }
            body.extend_from_slice(&chunk);
        }
        Ok(StreamableHttpPostResponse::Json(
            serde_json::from_slice(&body)?,
            session,
        ))
    }
    async fn delete_session(
        &self,
        uri: Arc<str>,
        session: Arc<str>,
        auth: Option<String>,
        headers: HashMap<HeaderName, HeaderValue>,
    ) -> Result<(), Error> {
        self.0
            .delete_session(uri, session, auth, headers)
            .await
            .map_err(|_| Error::Client(super::failure("MCP session close failed")))
    }
    async fn get_stream(
        &self,
        uri: Arc<str>,
        session: Option<Arc<str>>,
        last_event: Option<String>,
        auth: Option<String>,
        headers: HashMap<HeaderName, HeaderValue>,
    ) -> Result<BoxedSseResponse, Error> {
        self.0
            .get_stream_with_max_sse_event_size(uri, session, last_event, auth, headers, LIMIT)
            .await
            .map_err(|_| Error::Client(super::failure("MCP event stream failed")))
    }
}

pub(super) fn tool_error(mut error: &(dyn std::error::Error + 'static)) -> ToolError {
    loop {
        if let Some(Error::Client(client)) = error.downcast_ref::<Error>() {
            error = client;
        }
        if let Some(ToolError::HttpStatus {
            status,
            retry_after_ms,
        }) = error.downcast_ref::<ToolError>()
        {
            return ToolError::HttpStatus {
                status: *status,
                retry_after_ms: *retry_after_ms,
            };
        }
        let Some(source) = error.source() else {
            return super::failure("MCP HTTP request failed");
        };
        error = source;
    }
}
