//! Lane gates, protocol attempts with retry, and the transport setup the lanes share.
use super::*;
use crate::{envelope, ProviderBearerToken, ProviderErrorEnvelope};
use reqwest::header::HeaderMap;

/// Attempts of lane V2: the first plus two retries, with 1 s then 2 s between them.
const V2_ATTEMPTS: u32 = 3;
const LANE_TIMEOUT: Duration = Duration::from_secs(300);

/// A resolved connection for one lane request.
pub(super) struct Route {
    pub endpoint: reqwest::Url,
    pub headers: HeaderMap,
    pub token: Option<ProviderBearerToken>,
}

impl HttpProvider {
    /// The wire API compaction uses: the model override, else the provider's own.
    fn compaction_protocol(
        &self,
        request: &CompletionRequest,
        settings: &NativeLaneSettings,
    ) -> Protocol {
        settings
            .model
            .as_ref()
            .and_then(|model| model.api.as_deref())
            .and_then(Protocol::from_name)
            .unwrap_or_else(|| self.protocol(request))
    }

    /// The lanes this provider offers `request`'s model under `settings`, in attempt order.
    pub(crate) fn compaction_lanes(
        &self,
        request: &CompletionRequest,
        settings: &NativeLaneSettings,
    ) -> Vec<NativeCompactionLane> {
        let codex = self.auth_profile == Some(crate::ProviderAuthProfile::Codex);
        let enabled = settings.model.as_ref().and_then(|model| model.enabled);
        if !settings.remote_enabled
            || enabled == Some(false)
            || codex && !crate::codex_model_allowed(&request.model_id)
        {
            return Vec::new();
        }
        match self.compaction_protocol(request, settings) {
            Protocol::Responses => {
                let explicit_endpoint = settings
                    .endpoint
                    .as_deref()
                    .is_some_and(|endpoint| !endpoint.trim().is_empty());
                let v1 = if codex {
                    explicit_endpoint
                } else {
                    request.provider_id.as_deref() == Some("openai") || enabled == Some(true)
                };
                let mut lanes = Vec::new();
                if settings.v2_enabled {
                    lanes.push(NativeCompactionLane::OpenAiV2);
                }
                if v1 {
                    lanes.push(NativeCompactionLane::OpenAiV1);
                }
                lanes
            }
            Protocol::Anthropic => {
                // Anthropic and Vertex serve signed summaries as is; other supported hosts
                // and models beyond Claude 4.6 and 4.7 need the explicit opt-in.
                let opted_in = enabled == Some(true);
                let capable = anthropic::anthropic_capable(&request.model_id) || opted_in;
                let route = anthropic::supported_anthropic(&self.endpoint)
                    && (anthropic::first_party_anthropic(&self.endpoint) || opted_in);
                if capable && route {
                    vec![NativeCompactionLane::Anthropic]
                } else {
                    Vec::new()
                }
            }
            Protocol::Chat => Vec::new(),
        }
    }

    /// What a compaction side request to a configured endpoint sends: the provider's key
    /// and model headers, or none when the provider needs no key.
    pub(crate) async fn resolve_compaction_credential(
        &self,
    ) -> Result<Option<CompactionCredential>, NativeCompactionError> {
        let headers = self
            .headers
            .iter()
            .filter_map(|(name, value)| {
                Some((name.as_str().to_owned(), value.to_str().ok()?.to_owned()))
            })
            .collect();
        let api_key = match &self.credentials {
            Some(source) => Some(
                source
                    .bearer_token()
                    .await
                    .map_err(|e| NativeCompactionError::credentials(e.category, e.message))?
                    .token,
            ),
            None => None,
        };
        Ok(Some(CompactionCredential { api_key, headers }))
    }

    /// Tries each offered lane in order. A failure is kept for the caller only while every
    /// earlier failure was an authentication one; caller cancellation stops at once.
    pub(crate) async fn compact_native(
        &self,
        mut input: NativeCompactionRequest,
        cancel: CancellationToken,
    ) -> Result<NativeCompactionResult, NativeCompactionError> {
        input.request = input::sanitize(std::mem::take(&mut input.request));
        if let Some(saved) = input.saved_request.take() {
            anthropic::apply_saved_controls(&mut input.request, &saved);
        }
        let offered = self.compaction_lanes(&input.request, &input.settings);
        let mut failure = None;
        for lane in &input.lanes {
            if !offered.contains(lane) {
                continue;
            }
            let attempts = if *lane == NativeCompactionLane::OpenAiV2 {
                V2_ATTEMPTS
            } else {
                1
            };
            for attempt in 0..attempts {
                let outcome = tokio::select! {
                    biased;
                    () = cancel.cancelled() => return Err(NativeCompactionError::new("native compaction cancelled")),
                    result = tokio::time::timeout(LANE_TIMEOUT, self.compact_attempt(&input, *lane)) => {
                        result.unwrap_or_else(|_| Err(NativeCompactionError::new("native compaction timeout")))
                    }
                };
                let error = match outcome {
                    Ok(result) => return Ok(result),
                    Err(error) => error,
                };
                let retry = attempt + 1 < attempts && error.transient();
                if failure
                    .as_ref()
                    .is_none_or(|old: &NativeCompactionError| old.auth)
                {
                    failure = Some(error);
                }
                if !retry {
                    break;
                }
                tokio::select! {
                    () = cancel.cancelled() => return Err(NativeCompactionError::new("native compaction cancelled")),
                    () = tokio::time::sleep(Duration::from_millis(1000 << attempt)) => {}
                }
            }
        }
        Err(failure.unwrap_or_else(|| NativeCompactionError::new("native compaction unsupported")))
    }

    async fn compact_attempt(
        &self,
        input: &NativeCompactionRequest,
        lane: NativeCompactionLane,
    ) -> Result<NativeCompactionResult, NativeCompactionError> {
        if input.request.messages.is_empty() {
            return Err(NativeCompactionError::new("native compaction input empty"));
        }
        if lane == NativeCompactionLane::Anthropic {
            self.compact_anthropic(input).await
        } else {
            self.compact_openai(input, lane).await
        }
    }

    /// Resolves the credential and applies the provider's profile decoration to `body`.
    pub(super) async fn route(
        &self,
        req: &CompletionRequest,
        body: &mut Value,
    ) -> Result<Route, NativeCompactionError> {
        let mut endpoint = self.endpoint.clone();
        let mut headers = self.headers.clone();
        let token = match &self.credentials {
            Some(source) => Some(
                source
                    .bearer_token()
                    .await
                    .map_err(|e| NativeCompactionError::credentials(e.category, e.message))?,
            ),
            None => None,
        };
        if let (Some(profile), Some(token)) = (self.auth_profile, &token) {
            profile
                .decorate(&mut endpoint, &mut headers, body, req, token)
                .map_err(NativeCompactionError::new)?;
        }
        Ok(Route {
            endpoint,
            headers,
            token,
        })
    }

    /// Sends `body` over `route`: protocol authentication, then the provider headers.
    pub(super) async fn send_lane(
        &self,
        route: Route,
        protocol: Protocol,
        body: &Value,
        streaming: bool,
    ) -> Result<reqwest::Response, NativeCompactionError> {
        let mut post = self.client.post(route.endpoint).json(body);
        if protocol == Protocol::Anthropic {
            post = post.header("anthropic-version", "2023-06-01");
        }
        if let Some(token) = &route.token {
            post = if protocol == Protocol::Anthropic {
                post.header("x-api-key", &token.token)
            } else {
                post.bearer_auth(&token.token)
            };
        }
        post = post.headers(route.headers);
        if streaming {
            post = post.header("accept", "text/event-stream");
        }
        let mut response = post
            .send()
            .await
            .map_err(|_| NativeCompactionError::new("provider connection failed"))?;
        if response.status().is_success() {
            return Ok(response);
        }
        let status = response.status().as_u16();
        let secrets: Vec<&str> = route.token.iter().map(|t| t.token.as_str()).collect();
        let body = envelope::read_body(&mut response, &secrets).await;
        Err(NativeCompactionError::lane_http(
            ProviderErrorEnvelope::from_response(status, response.headers(), body),
        ))
    }
}
