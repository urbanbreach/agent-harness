use harness_core::tool::{Tool, ToolCapability, ToolContext, ToolError, ToolResult};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{sync::LazyLock, time::Duration};

pub(crate) struct Fetch;
const MAX_BYTES: usize = 5 * 1024 * 1024;
pub(crate) static CLIENT: LazyLock<Result<reqwest::Client, reqwest::Error>> = LazyLock::new(|| {
    reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .retry(reqwest::retry::never())
        .pool_max_idle_per_host(2)
        .pool_idle_timeout(Duration::from_secs(30))
        .connect_timeout(Duration::from_secs(10))
        .build()
});

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct Args {
    url: String,
    #[serde(default)]
    format: Format,
    #[serde(default)]
    timeout: Option<u64>,
}
#[derive(Clone, Copy, Default, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
enum Format {
    Text,
    #[default]
    Markdown,
    Html,
}
#[async_trait::async_trait]
impl Tool for Fetch {
    fn id(&self) -> &str {
        "webfetch"
    }
    fn description(&self) -> &str {
        "Fetch an HTTP(S) page as markdown, text, or HTML, return an image, or retain a binary/PDF artifact without extracting its contents. Responses are limited to 5 MiB; redirects keep permission checks."
    }
    fn capability(&self) -> ToolCapability {
        ToolCapability::Network
    }
    fn parameters_json_schema(&self) -> Value {
        schemars::schema_for!(Args).to_value()
    }
    fn permission_requests(&self, args: &Value) -> Vec<(String, String)> {
        let input = args["url"].as_str().unwrap_or_default();
        let mut requests = vec![(self.id().into(), input.into())];
        if let Ok(url) = url(input)
            && url.as_str() != input
        {
            requests.push((self.id().into(), url.to_string()));
        }
        requests
    }
    async fn call(&self, ctx: ToolContext, args: Value) -> Result<ToolResult, ToolError> {
        let args: Args =
            serde_json::from_value(args).map_err(|e| ToolError::InvalidArguments(e.to_string()))?;
        let deadline = tokio::time::Instant::now()
            + Duration::from_secs(args.timeout.unwrap_or(30).clamp(1, 120));
        let (body, metadata) = tokio::select! {
            biased;
            () = ctx.cancellation.cancelled() => return Err(ToolError::Cancelled),
            result = tokio::time::timeout_at(deadline, fetch(&ctx, &args)) => result.map_err(|_| failure("web fetch timed out"))??,
        };
        let cancel = ctx.cancellation.clone();
        // Join bounded CPU work even after cancellation; never leave detached conversions running.
        let output = tokio::task::spawn_blocking(move || {
            if ctx.cancellation.is_cancelled() {
                return Err(ToolError::Cancelled);
            }
            if let Some(mime) = crate::media::mime(&body) {
                if mime == "application/pdf" {
                    return crate::media::artifact(&ctx, body, metadata);
                }
                return Ok(
                    ToolResult::structured("Image fetched successfully.", metadata)
                        .with_attachments(vec![crate::media::attachment(
                            ctx.tool_call_id.to_string(),
                            mime,
                            &body,
                        )?]),
                );
            }
            let content_type = metadata["content_type"].as_str().unwrap_or_default();
            let mime = content_type
                .split(';')
                .next()
                .unwrap_or_default()
                .trim()
                .to_ascii_lowercase();
            let textual = mime.starts_with("text/")
                || mime.ends_with("+json")
                || mime.ends_with("+xml")
                || matches!(mime.as_str(), "" | "application/json" | "application/xml");
            if !textual
                && (mime == "application/pdf"
                    || mime.starts_with("image/")
                    || body.contains(&0)
                    || std::str::from_utf8(&body).is_err())
            {
                return crate::media::artifact(&ctx, body, metadata);
            }
            let text = convert(body, content_type, args.format)?;
            Ok(ToolResult::structured(text, metadata))
        })
        .await
        .map_err(|_| failure("HTML conversion stopped"))??;
        if cancel.is_cancelled() {
            return Err(ToolError::Cancelled);
        }
        if tokio::time::Instant::now() >= deadline {
            return Err(failure("web fetch timed out"));
        }
        Ok(output)
    }
}
fn url(input: &str) -> Result<reqwest::Url, ToolError> {
    let mut url = reqwest::Url::parse(input)
        .map_err(|_| ToolError::InvalidArguments("invalid URL".into()))?;
    if !matches!(url.scheme(), "http" | "https")
        || url.host_str().is_none()
        || !url.username().is_empty()
        || url.password().is_some()
    {
        return Err(ToolError::InvalidArguments(
            "URL must be HTTP(S) without embedded credentials".into(),
        ));
    }
    url.set_fragment(None);
    Ok(url)
}
async fn fetch(ctx: &ToolContext, args: &Args) -> Result<(Vec<u8>, Value), ToolError> {
    let client = CLIENT
        .as_ref()
        .map_err(|_| failure("HTTP client initialization failed"))?;
    let mut target = url(&args.url)?;
    for redirects in 0..=10 {
        let response = client
            .get(target.clone())
            .header(
                "accept",
                "text/markdown, text/plain, text/html, application/json;q=0.8, image/*;q=0.7, application/pdf;q=0.7, */*;q=0.5",
            )
            .header(
                "user-agent",
                concat!("agent-harness/", env!("CARGO_PKG_VERSION")),
            )
            .send()
            .await
            .map_err(|e| failure(&format!("web fetch request failed: {}", e.without_url())))?;
        if response.status().is_redirection() {
            if redirects == 10 {
                return Err(failure("web fetch exceeded 10 redirects"));
            }
            let location = response
                .headers()
                .get(reqwest::header::LOCATION)
                .and_then(|v| v.to_str().ok())
                .ok_or_else(|| failure("redirect has no valid Location"))?;
            let next = target
                .join(location)
                .map_err(|_| failure("redirect URL is invalid"))?;
            target = url(next.as_str())?;
            if !ctx
                .coordinator
                .network_redirect_allowed(ctx.tool_call_id.to_string(), target.as_str())
                .await
                .map_err(|e| failure(&e.to_string()))?
            {
                return Err(failure(&format!(
                    "redirect requires a separately approved webfetch call: {}",
                    ctx.redactor.redact_text(target.as_str())
                )));
            }
            continue;
        }
        if !response.status().is_success() {
            return Err(failure(&format!(
                "web fetch returned HTTP {}",
                response.status().as_u16()
            )));
        }
        let content_type = response
            .headers()
            .get(reqwest::header::CONTENT_TYPE)
            .and_then(|v| v.to_str().ok())
            .unwrap_or("text/plain")
            .to_owned();
        if response
            .headers()
            .get(reqwest::header::CONTENT_ENCODING)
            .is_some_and(|v| v != "identity")
        {
            return Err(failure("server returned an unsupported content encoding"));
        }
        let body = response_bytes(response, MAX_BYTES).await?;
        let bytes = body.len();
        return Ok((
            body,
            json!({"url":target.as_str(),"content_type":content_type,"format":args.format,"bytes":bytes}),
        ));
    }
    Err(failure("web redirect limit exceeded"))
}
pub(crate) async fn response_bytes(
    mut response: reqwest::Response,
    limit: usize,
) -> Result<Vec<u8>, ToolError> {
    let too_large = || failure(&format!("HTTP response exceeds {limit} bytes"));
    if response.content_length().is_some_and(|n| n > limit as u64) {
        return Err(too_large());
    }
    let mut body = Vec::new();
    while let Some(chunk) = response
        .chunk()
        .await
        .map_err(|_| failure("HTTP response body failed"))?
    {
        if body.len().saturating_add(chunk.len()) > limit {
            return Err(too_large());
        }
        body.extend_from_slice(&chunk);
    }
    Ok(body)
}
fn convert(body: Vec<u8>, content_type: &str, format: Format) -> Result<String, ToolError> {
    let mut parts = content_type.split(';');
    let mime = parts.next().unwrap_or_default().trim().to_ascii_lowercase();
    let html = matches!(mime.as_str(), "text/html" | "application/xhtml+xml");
    let charset = parts
        .filter_map(|s| s.split_once('='))
        .find(|(key, _)| key.trim().eq_ignore_ascii_case("charset"));
    let encoding = match charset {
        Some((_, label)) => {
            encoding_rs::Encoding::for_label(label.trim().trim_matches('"').as_bytes())
                .ok_or_else(|| failure("web response uses an unknown character encoding"))?
        }
        None => encoding_rs::UTF_8,
    };
    let (text, _, invalid) = encoding.decode(&body);
    if invalid || text.contains('\0') {
        return Err(failure(
            "web response is not valid text or a supported image",
        ));
    }
    if text.len() > MAX_BYTES {
        return Err(failure("decoded web text exceeds 5 MiB"));
    }
    let body = text.into_owned();
    if !html || matches!(format, Format::Html) {
        return Ok(body);
    }
    let markdown = htmd::HtmlToMarkdown::builder()
        .skip_tags(vec!["script", "style", "head", "noscript", "iframe"])
        .build()
        .convert(&body)
        .map_err(|_| failure("HTML conversion failed"))?;
    if matches!(format, Format::Markdown) {
        return Ok(markdown);
    }
    use pulldown_cmark::{Event, TagEnd};
    let mut text = String::new();
    for event in pulldown_cmark::Parser::new(&markdown) {
        match event {
            Event::Text(value) | Event::Code(value) => text.push_str(&value),
            Event::SoftBreak
            | Event::HardBreak
            | Event::Rule
            | Event::End(
                TagEnd::Paragraph | TagEnd::Heading(_) | TagEnd::Item | TagEnd::CodeBlock,
            ) => text.push('\n'),
            _ => {}
        }
    }
    Ok(text)
}
fn failure(message: &str) -> ToolError {
    ToolError::Execution(message.into())
}
