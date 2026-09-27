use super::codex::{AuthHttpClient, AuthHttpRequest, AuthHttpResponse, CodexOAuthError};
use std::time::Duration;

#[derive(Clone)]
pub struct ReqwestAuthHttpClient {
    client: reqwest::Client,
}
impl ReqwestAuthHttpClient {
    pub fn new() -> Result<Self, CodexOAuthError> {
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(30))
            .connect_timeout(Duration::from_secs(10))
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .map_err(|_| failure("cannot initialize authentication transport"))?;
        Ok(Self { client })
    }
}
#[async_trait::async_trait]
impl AuthHttpClient for ReqwestAuthHttpClient {
    async fn send(&self, request: AuthHttpRequest) -> Result<AuthHttpResponse, CodexOAuthError> {
        let url = validate_endpoint(&request.url)?;
        if request.body.len() > 1024 * 1024 {
            return Err(failure("authentication request exceeds 1 MiB"));
        }
        let mut post = self.client.post(url).body(request.body);
        for (key, value) in request.headers {
            post = post.header(key, value);
        }
        let mut response = post
            .send()
            .await
            .map_err(|_| failure("authentication connection failed"))?;
        let status = response.status().as_u16();
        if response
            .content_length()
            .is_some_and(|size| size > 1024 * 1024)
        {
            return Err(failure("authentication response exceeds 1 MiB"));
        }
        let mut bytes = Vec::new();
        while let Some(chunk) = response
            .chunk()
            .await
            .map_err(|_| failure("authentication response interrupted"))?
        {
            if bytes.len() + chunk.len() > 1024 * 1024 {
                return Err(failure("authentication response exceeds 1 MiB"));
            }
            bytes.extend_from_slice(&chunk);
        }
        Ok(AuthHttpResponse {
            status,
            body: String::from_utf8(bytes)
                .map_err(|_| failure("authentication response is not UTF-8"))?,
        })
    }
}
pub(super) fn validate_endpoint(url: &str) -> Result<reqwest::Url, CodexOAuthError> {
    let parsed = reqwest::Url::parse(url).map_err(|_| failure("invalid authentication URL"))?;
    let loopback = parsed.host_str().is_some_and(|host| {
        host == "localhost"
            || host
                .trim_matches(['[', ']'])
                .parse::<std::net::IpAddr>()
                .is_ok_and(|ip| ip.is_loopback())
    });
    if !parsed.username().is_empty()
        || parsed.password().is_some()
        || parsed.query().is_some()
        || parsed.fragment().is_some()
        || url.chars().any(char::is_control)
        || !(parsed.scheme() == "https" || parsed.scheme() == "http" && loopback)
    {
        return Err(failure("authentication requires HTTPS or a local loopback endpoint without credentials, query, or fragment"));
    }
    Ok(parsed)
}
fn failure(message: &str) -> CodexOAuthError {
    CodexOAuthError::Http {
        message: message.into(),
    }
}
