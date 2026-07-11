use std::pin::Pin;
use std::sync::Arc;

use async_trait::async_trait;
use reqwest::header::HeaderMap;
use tokio_stream::{self as stream, Stream, StreamExt};

use super::error::format_transport_error;

pub type OpenAiResponseBody = Pin<Box<dyn Stream<Item = Result<Vec<u8>, String>> + Send>>;

pub struct OpenAiHttpResponse {
    pub status: u16,
    pub headers: HeaderMap,
    pub body: OpenAiResponseBody,
}

impl OpenAiHttpResponse {
    pub fn new(status: u16, headers: HeaderMap, body: OpenAiResponseBody) -> Self {
        Self {
            status,
            headers,
            body,
        }
    }

    pub fn text(status: u16, headers: HeaderMap, body: impl Into<String>) -> Self {
        Self::new(
            status,
            headers,
            Box::pin(stream::iter(vec![Ok(body.into().into_bytes())])),
        )
    }
}

#[async_trait]
pub trait OpenAiHttpTransport: Send + Sync {
    async fn post_json(
        &self,
        endpoint: String,
        headers: HeaderMap,
        bearer_token: String,
        body: serde_json::Value,
    ) -> Result<OpenAiHttpResponse, String>;
}

#[derive(Debug, Clone)]
pub(crate) struct ReqwestOpenAiHttpTransport {
    client: reqwest::Client,
}

#[async_trait]
impl OpenAiHttpTransport for ReqwestOpenAiHttpTransport {
    async fn post_json(
        &self,
        endpoint: String,
        headers: HeaderMap,
        bearer_token: String,
        body: serde_json::Value,
    ) -> Result<OpenAiHttpResponse, String> {
        let response = self
            .client
            .post(endpoint)
            .headers(headers)
            .bearer_auth(bearer_token)
            .json(&body)
            .send()
            .await
            .map_err(format_transport_error)?;

        let status = response.status().as_u16();
        let headers = response.headers().clone();
        let body = response.bytes_stream().map(|chunk| {
            chunk
                .map(|bytes| bytes.to_vec())
                .map_err(format_transport_error)
        });

        Ok(OpenAiHttpResponse::new(status, headers, Box::pin(body)))
    }
}

impl ReqwestOpenAiHttpTransport {
    pub(crate) fn new(client: reqwest::Client) -> Self {
        Self { client }
    }
}
