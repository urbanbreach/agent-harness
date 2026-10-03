use crate::{CompletionRequest, MessageRole, ProviderBearerToken, ProviderRequestInitiator};
use reqwest::{
    header::{HeaderMap, HeaderValue},
    Url,
};
use serde_json::Value;

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum ProviderAuthProfile {
    Codex,
    GithubCopilot,
}

/// Subscription support, checked against https://learn.chatgpt.com/docs/models on 2026-10-04.
pub fn codex_model_allowed(model: &str) -> bool {
    matches!(
        model,
        "gpt-6-astra"
            | "gpt-6.1-sol"
            | "gpt-6-sol"
            | "gpt-6-luna"
            | "gpt-5.6-sol"
            | "gpt-5.6-terra"
            | "gpt-5.6-luna"
            | "gpt-5.5"
    )
}

impl ProviderAuthProfile {
    pub(crate) fn decorate(
        self,
        endpoint: &mut Url,
        headers: &mut HeaderMap,
        body: &mut Value,
        request: &CompletionRequest,
        token: &ProviderBearerToken,
    ) -> Result<(), &'static str> {
        headers.insert(
            "user-agent",
            HeaderValue::from_static(concat!("harness/", env!("CARGO_PKG_VERSION"))),
        );
        match self {
            Self::Codex => {
                if matches!(endpoint.host_str(), Some("api.openai.com" | "chatgpt.com")) {
                    *endpoint = Url::parse("https://chatgpt.com/backend-api/codex/responses")
                        .map_err(|_| "invalid Codex endpoint")?;
                }
                headers.insert("originator", HeaderValue::from_static("harness"));
                for (name, value) in [
                    ("chatgpt-account-id", &token.account_id),
                    ("session-id", &request.context.session_id),
                    ("request-id", &request.context.request_id),
                ] {
                    headers.remove(name);
                    if let Some(value) = value.as_deref().filter(|s| !s.is_empty()) {
                        headers.insert(
                            name,
                            HeaderValue::from_str(value)
                                .map_err(|_| "invalid subscription metadata")?,
                        );
                    }
                }
                let object = body.as_object_mut().ok_or("invalid request body")?;
                object.remove("max_output_tokens");
                object.remove("temperature");
                object.insert("store".into(), false.into());
                object.insert(
                    "instructions".into(),
                    request
                        .messages
                        .iter()
                        .filter(|m| m.role == MessageRole::System)
                        .map(|m| m.content.as_str())
                        .collect::<Vec<_>>()
                        .join("\n\n")
                        .into(),
                );
                if let Some(input) = object.get_mut("input").and_then(Value::as_array_mut) {
                    input.retain(|item| item["role"] != "system");
                }
                if matches!(request.model_id.as_str(), "gpt-6-astra" | "gpt-6.1-sol")
                    || request.model_id.starts_with("gpt-5")
                {
                    body["include"] = serde_json::json!(["reasoning.encrypted_content"]);
                    let effort = if request.model_id == "gpt-6-astra" {
                        "low"
                    } else {
                        "medium"
                    };
                    body["reasoning"]["effort"] =
                        request.reasoning_effort.as_deref().unwrap_or(effort).into();
                    body["reasoning"]["summary"] = request
                        .reasoning_summary
                        .as_deref()
                        .unwrap_or("auto")
                        .into();
                    body["text"]["verbosity"] =
                        request.text_verbosity.as_deref().unwrap_or("low").into();
                }
            }
            Self::GithubCopilot => {
                headers.remove("x-api-key");
                headers.insert(
                    "x-initiator",
                    HeaderValue::from_static(match request.context.initiator {
                        ProviderRequestInitiator::Agent => "agent",
                        ProviderRequestInitiator::User => "user",
                    }),
                );
                headers.insert(
                    "openai-intent",
                    HeaderValue::from_static("conversation-edits"),
                );
                headers.remove("copilot-vision-request");
                if request.context.has_media
                    || request
                        .attachments
                        .values()
                        .flatten()
                        .any(|a| a.mime.starts_with("image/"))
                {
                    headers.insert("copilot-vision-request", HeaderValue::from_static("true"));
                }
                if endpoint.host_str() == Some("api.githubcopilot.com") {
                    if let Some(domain) = &token.enterprise_url {
                        endpoint
                            .set_host(Some(&enterprise_host(domain)?))
                            .map_err(|_| "invalid Copilot enterprise domain")?;
                    }
                    if endpoint.path() == "/messages" {
                        endpoint.set_path("/v1/messages");
                    } else if let Some(path) = endpoint
                        .path()
                        .strip_prefix("/v1/")
                        .filter(|path| *path != "messages")
                    {
                        endpoint.set_path(&format!("/{path}"));
                    }
                }
            }
        }
        Ok(())
    }
}

fn enterprise_host(domain: &str) -> Result<String, &'static str> {
    if domain.len() > 253
        || domain.split('.').any(|part| {
            part.is_empty()
                || part.len() > 63
                || part.starts_with('-')
                || part.ends_with('-')
                || !part.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-')
        })
    {
        return Err("invalid Copilot enterprise domain");
    }
    Ok(format!("copilot-api.{domain}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        CompletionMessage, HttpProvider, Protocol, Provider, ProviderCredentialKind,
        ProviderOutputCapDisposition,
    };

    #[tokio::test]
    async fn subscription_routes_preserve_custom_hosts_and_reject_header_injection(
    ) -> Result<(), Box<dyn std::error::Error>> {
        let mut request = CompletionRequest {
            model_id: "gpt-6-sol".into(),
            max_tokens: Some(4096),
            messages: vec![CompletionMessage::text(MessageRole::User, "Hi")],
            ..Default::default()
        };
        let mut token = ProviderBearerToken {
            token: "fixture-token".into(),
            kind: ProviderCredentialKind::StoredOauth,
            account_id: Some("fixture-account".into()),
            enterprise_url: None,
        };
        for (profile, source, enterprise, expected) in [
            (
                ProviderAuthProfile::Codex,
                "https://api.openai.com/v1/responses",
                None,
                "https://chatgpt.com/backend-api/codex/responses",
            ),
            (
                ProviderAuthProfile::Codex,
                "http://localhost:1234/v1/responses",
                None,
                "http://localhost:1234/v1/responses",
            ),
            (
                ProviderAuthProfile::GithubCopilot,
                "https://api.githubcopilot.com/v1/chat/completions",
                None,
                "https://api.githubcopilot.com/chat/completions",
            ),
            (
                ProviderAuthProfile::GithubCopilot,
                "https://api.githubcopilot.com/responses",
                Some("acme.ghe.com"),
                "https://copilot-api.acme.ghe.com/responses",
            ),
        ] {
            token.enterprise_url = enterprise.map(str::to_owned);
            let mut url = Url::parse(source)?;
            profile.decorate(
                &mut url,
                &mut HeaderMap::new(),
                &mut crate::wire::encode(&request, Protocol::Responses)?,
                &request,
                &token,
            )?;
            assert_eq!(url.as_str(), expected);
        }
        let mut url = Url::parse("https://api.githubcopilot.com/responses")?;
        for domain in ["bad@host", "host/path", "host:443", "-bad.ghe.com"] {
            token.enterprise_url = Some(domain.into());
            assert!(ProviderAuthProfile::GithubCopilot
                .decorate(
                    &mut url,
                    &mut HeaderMap::new(),
                    &mut serde_json::json!({}),
                    &request,
                    &token
                )
                .is_err());
        }
        token.account_id = Some("bad\r\nx-injected: yes".into());
        assert!(ProviderAuthProfile::Codex
            .decorate(
                &mut url,
                &mut HeaderMap::new(),
                &mut serde_json::json!({}),
                &request,
                &token
            )
            .is_err());
        let provider = HttpProvider::new(
            "https://api.openai.com/v1/responses",
            Protocol::Responses,
            std::time::Duration::from_secs(1),
        )?
        .with_auth_profile(ProviderAuthProfile::Codex);
        assert_eq!(
            provider
                .request_budget_semantics(&request, 0)?
                .output_cap_disposition,
            ProviderOutputCapDisposition::ProviderControlled
        );
        request.model_id = "gpt-5.4-mini".into();
        use tokio_stream::StreamExt;
        assert!(
            matches!(provider.stream(request).next().await, Some(crate::ProviderStreamEvent::Error { category: Some(crate::ProviderErrorCategory::Other), message, .. }) if message.contains("Codex subscription"))
        );
        Ok(())
    }
}
