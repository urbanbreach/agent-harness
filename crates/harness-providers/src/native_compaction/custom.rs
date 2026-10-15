//! A configured text-summary endpoint: a Chat Completions-compatible server or a
//! purpose-built `{systemPrompt, prompt, maxTokens}` summarizer.
use super::NativeCompactionError;
use crate::{envelope, CompactionCredential, ProviderErrorCategory, ProviderErrorEnvelope};
use reqwest::header::{HeaderMap, HeaderName, HeaderValue, AUTHORIZATION, CONTENT_TYPE};
use serde_json::{json, Map, Value};
use std::time::Duration;
use tokio_util::sync::CancellationToken;

const TIMEOUT: Duration = Duration::from_secs(300);

/// The reply of a purpose-built summarizer endpoint.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CustomSummary {
    pub summary: String,
    pub short_summary: Option<String>,
    /// Every other field of the response, verbatim.
    pub extra: Map<String, Value>,
}

/// `/chat/completions` with an optional trailing slash selects the compatible format. The
/// query is ignored when the endpoint parses as a URL; otherwise the raw text is matched.
pub(super) fn is_chat_completions(endpoint: &str) -> bool {
    let path = reqwest::Url::parse(endpoint)
        .map_or_else(|_| endpoint.to_owned(), |url| url.path().to_owned());
    path.strip_suffix('/')
        .unwrap_or(&path)
        .ends_with("/chat/completions")
}

fn failure(category: ProviderErrorCategory, message: &str) -> NativeCompactionError {
    let mut error = NativeCompactionError::new(message);
    error.category = Some(category);
    error
}

/// Asks `endpoint` for a summary.
///
/// `credential` is what the summary model's provider resolved; `None` sends no
/// authentication. Only the compatible format carries the bearer key and the model
/// headers; the purpose-built format gets just its content type.
pub async fn custom_endpoint_summary(
    endpoint: &str,
    credential: Option<&CompactionCredential>,
    model: Option<&str>,
    system: &str,
    prompt: &str,
    max_tokens: Option<u32>,
    cancel: CancellationToken,
) -> Result<CustomSummary, NativeCompactionError> {
    let chat = is_chat_completions(endpoint);
    let url = reqwest::Url::parse(endpoint)
        .map_err(|_| NativeCompactionError::new("invalid summary endpoint"))?;
    if !matches!(url.scheme(), "http" | "https")
        || !url.username().is_empty()
        || url.password().is_some()
    {
        return Err(NativeCompactionError::new("invalid summary endpoint"));
    }
    let body = if chat {
        let mut body = json!({"messages":[{"role":"system","content":system},{"role":"user","content":prompt}],"stream":false});
        if let Some(model) = model {
            body["model"] = model.into();
        }
        if let Some(n) = max_tokens {
            body["max_tokens"] = n.into();
        }
        body
    } else {
        let mut body = json!({"systemPrompt":system,"prompt":prompt});
        if let Some(n) = max_tokens {
            body["maxTokens"] = n.into();
        }
        body
    };
    let mut headers = HeaderMap::new();
    headers.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));
    if chat && let Some(credential) = credential {
        if let Some(key) = credential.api_key.as_deref().filter(|key| !key.is_empty()) {
            headers.insert(AUTHORIZATION, bearer(key)?);
        }
        for (name, value) in &credential.headers {
            headers.insert(
                HeaderName::from_bytes(name.as_bytes()).map_err(|_| header_error())?,
                HeaderValue::from_str(value).map_err(|_| header_error())?,
            );
        }
    }
    let client = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .map_err(|_| {
            failure(
                ProviderErrorCategory::TransportFailure,
                "Remote compaction client failed",
            )
        })?;
    let bytes = serde_json::to_vec(&body)
        .map_err(|_| NativeCompactionError::new("Remote compaction request invalid"))?;
    let secrets: Vec<&str> = credential
        .and_then(|c| c.api_key.as_deref())
        .into_iter()
        .collect();
    let request = client
        .post(url)
        .headers(headers)
        .body(bytes)
        .timeout(TIMEOUT);
    tokio::select! {
        biased;
        () = cancel.cancelled() => Err(failure(ProviderErrorCategory::Other, "Remote compaction cancelled")),
        result = exchange(request, chat, &secrets) => result,
    }
}

fn bearer(key: &str) -> Result<HeaderValue, NativeCompactionError> {
    let mut value = HeaderValue::from_str(&format!("Bearer {key}")).map_err(|_| header_error())?;
    value.set_sensitive(true);
    Ok(value)
}

fn header_error() -> NativeCompactionError {
    NativeCompactionError::new("Remote compaction credential header is invalid")
}

async fn exchange(
    request: reqwest::RequestBuilder,
    chat: bool,
    secrets: &[&str],
) -> Result<CustomSummary, NativeCompactionError> {
    let mut response = request.send().await.map_err(|_| {
        failure(
            ProviderErrorCategory::TransportFailure,
            "Remote compaction connection failed",
        )
    })?;
    if !response.status().is_success() {
        let status = response.status();
        let body = envelope::read_body(&mut response, secrets).await;
        tracing::debug!(
            status = status.as_u16(),
            body,
            "remote compaction endpoint failed"
        );
        let message = format!(
            "Remote compaction failed ({} {})",
            status.as_u16(),
            status.canonical_reason().unwrap_or("")
        );
        return Err(NativeCompactionError::http(
            message,
            ProviderErrorEnvelope::from_response(status.as_u16(), response.headers(), body),
        ));
    }
    let value: Value = response.json().await.map_err(|_| {
        failure(
            ProviderErrorCategory::MalformedStream,
            "Remote compaction response parse failed",
        )
    })?;
    if chat {
        chat_summary(&value)
    } else {
        generic_summary(value)
    }
}

/// `choices[0].message.content` as a string, or every string `text` of its parts joined
/// without a separator. Whitespace passes; an empty result does not.
fn chat_summary(value: &Value) -> Result<CustomSummary, NativeCompactionError> {
    let content = &value["choices"][0]["message"]["content"];
    let summary = match content {
        Value::String(text) => text.clone(),
        Value::Array(parts) => parts
            .iter()
            .filter_map(|part| part["text"].as_str())
            .collect(),
        _ => String::new(),
    };
    if summary.is_empty() {
        return Err(failure(
            ProviderErrorCategory::MalformedStream,
            "Remote compaction response missing choices[0].message.content",
        ));
    }
    Ok(CustomSummary {
        summary,
        short_summary: None,
        extra: Map::new(),
    })
}

fn generic_summary(value: Value) -> Result<CustomSummary, NativeCompactionError> {
    let Value::Object(mut object) = value else {
        return Err(missing_summary());
    };
    let Some(Value::String(summary)) = object.remove("summary") else {
        return Err(missing_summary());
    };
    let short_summary = match object.remove("shortSummary") {
        Some(Value::String(text)) => Some(text),
        _ => None,
    };
    Ok(CustomSummary {
        summary,
        short_summary,
        extra: object,
    })
}

fn missing_summary() -> NativeCompactionError {
    failure(
        ProviderErrorCategory::MalformedStream,
        "Remote compaction response missing summary",
    )
}
