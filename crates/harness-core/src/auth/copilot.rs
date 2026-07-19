// allow: SIZE_OK — Copilot OAuth device flow authentication (token exchange + polling + credential storage)
use crate::UnwrapOrAbort;
use std::collections::BTreeMap;
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use harness_providers::ProviderErrorCategory;
use serde::{Deserialize, Serialize};
use thiserror::Error;

use super::codex::{AuthHttpMethod, AuthHttpRequest, AuthHttpResponse};
use super::{
    AuthProviderId, CredentialClock, CredentialStore, CredentialStoreError, ProviderId,
    StoredCredential, SystemCredentialClock,
};

pub const COPILOT_CLIENT_ID: &str = "Ov23li8tweQw6odWQebz";
pub const COPILOT_PUBLIC_DOMAIN: &str = "github.com";
pub const COPILOT_PUBLIC_API_BASE: &str = "https://api.githubcopilot.com";
pub const COPILOT_POLLING_SAFETY_MARGIN: Duration = Duration::from_secs(3);
pub const COPILOT_SCOPE: &str = "read:user";

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CopilotDeployment {
    Public,
    Enterprise { domain: String },
}

impl CopilotDeployment {
    pub fn public() -> Self {
        Self::Public
    }

    pub fn enterprise(input: &str) -> Result<Self, CopilotOAuthError> {
        Ok(Self::Enterprise {
            domain: normalize_enterprise_domain(input)?,
        })
    }

    pub fn oauth_domain(&self) -> &str {
        match self {
            Self::Public => COPILOT_PUBLIC_DOMAIN,
            Self::Enterprise { domain } => domain,
        }
    }

    pub fn api_base_url(&self) -> String {
        match self {
            Self::Public => COPILOT_PUBLIC_API_BASE.to_string(),
            Self::Enterprise { domain } => format!("https://copilot-api.{domain}"),
        }
    }

    fn stored_enterprise_url(&self) -> Option<String> {
        match self {
            Self::Public => None,
            Self::Enterprise { domain } => Some(domain.clone()),
        }
    }
}

#[async_trait]
pub trait CopilotAuthHttpClient: Send + Sync {
    async fn send(&self, request: AuthHttpRequest) -> Result<AuthHttpResponse, CopilotOAuthError>;
}

#[derive(Clone)]
pub struct CopilotOAuthClient {
    http: Arc<dyn CopilotAuthHttpClient>,
    clock: Arc<dyn CredentialClock>,
}

impl CopilotOAuthClient {
    pub fn new(http: Arc<dyn CopilotAuthHttpClient>) -> Self {
        Self {
            http,
            clock: Arc::new(SystemCredentialClock),
        }
    }

    pub fn with_clock(mut self, clock: Arc<dyn CredentialClock>) -> Self {
        self.clock = clock;
        self
    }

    pub async fn start_device_authorization(
        &self,
        deployment: &CopilotDeployment,
    ) -> Result<CopilotDeviceAuthorization, CopilotOAuthError> {
        let response = self
            .http
            .send(AuthHttpRequest {
                method: AuthHttpMethod::Post,
                url: format!(
                    "https://{}/login/device/code",
                    deployment.oauth_domain().trim_end_matches('/')
                ),
                headers: json_accept_headers(),
                body: serde_json::json!({
                    "client_id": COPILOT_CLIENT_ID,
                    "scope": COPILOT_SCOPE,
                })
                .to_string(),
            })
            .await?;
        if !(200..300).contains(&response.status) {
            return Err(CopilotOAuthError::HttpStatus {
                operation: "device authorization",
                status: response.status,
            });
        }
        let device = serde_json::from_str::<CopilotDeviceAuthorizationResponse>(&response.body)
            .map_err(|source| CopilotOAuthError::Json {
                operation: "device authorization",
                message: source.to_string(),
            })?;
        Ok(CopilotDeviceAuthorization {
            verification_uri: non_empty(&device.verification_uri)
                .ok_or(CopilotOAuthError::MalformedResponse {
                    operation: "device authorization",
                    message: "missing verification_uri".to_string(),
                })?
                .to_string(),
            user_code: non_empty(&device.user_code)
                .ok_or(CopilotOAuthError::MalformedResponse {
                    operation: "device authorization",
                    message: "missing user_code".to_string(),
                })?
                .to_string(),
            device_code: non_empty(&device.device_code)
                .ok_or(CopilotOAuthError::MalformedResponse {
                    operation: "device authorization",
                    message: "missing device_code".to_string(),
                })?
                .to_string(),
            interval_seconds: device.interval.unwrap_or(5).max(1),
        })
    }

    pub async fn poll_device_token(
        &self,
        deployment: &CopilotDeployment,
        device: &CopilotDeviceAuthorization,
        current_interval_seconds: u64,
    ) -> Result<CopilotDevicePoll, CopilotOAuthError> {
        let response = self
            .http
            .send(AuthHttpRequest {
                method: AuthHttpMethod::Post,
                url: format!(
                    "https://{}/login/oauth/access_token",
                    deployment.oauth_domain().trim_end_matches('/')
                ),
                headers: json_accept_headers(),
                body: serde_json::json!({
                    "client_id": COPILOT_CLIENT_ID,
                    "device_code": device.device_code,
                    "grant_type": "urn:ietf:params:oauth:grant-type:device_code",
                })
                .to_string(),
            })
            .await?;
        if !(200..300).contains(&response.status) {
            return Err(CopilotOAuthError::HttpStatus {
                operation: "device poll",
                status: response.status,
            });
        }
        let data = serde_json::from_str::<CopilotDeviceTokenResponse>(&response.body).map_err(
            |source| CopilotOAuthError::Json {
                operation: "device poll",
                message: source.to_string(),
            },
        )?;
        if let Some(token) = data.access_token.as_deref().and_then(non_empty) {
            return Ok(CopilotDevicePoll::Authorized {
                access_token: token.to_string(),
            });
        }

        match data.error.as_deref().and_then(non_empty) {
            Some("authorization_pending") => Ok(CopilotDevicePoll::Pending {
                wait: poll_wait(current_interval_seconds),
            }),
            Some("slow_down") => {
                let interval_seconds = data
                    .interval
                    .filter(|interval| *interval > 0)
                    .unwrap_or(current_interval_seconds.saturating_add(5));
                Ok(CopilotDevicePoll::SlowDown {
                    interval_seconds,
                    wait: poll_wait(interval_seconds),
                })
            }
            Some("access_denied") => Err(CopilotOAuthError::AccessDenied),
            Some("expired_token") => Err(CopilotOAuthError::ExpiredToken),
            Some(error) => Err(CopilotOAuthError::OAuthError {
                error: error.to_string(),
            }),
            None => Err(CopilotOAuthError::MalformedResponse {
                operation: "device poll",
                message: "missing access_token or OAuth error".to_string(),
            }),
        }
    }

    pub async fn complete_device_flow(
        &self,
        deployment: &CopilotDeployment,
        store: &CredentialStore,
        max_polls: usize,
    ) -> Result<StoredCredential, CopilotOAuthError> {
        let device = self.start_device_authorization(deployment).await?;
        let mut interval_seconds = device.interval_seconds;
        for _ in 0..max_polls {
            match self
                .poll_device_token(deployment, &device, interval_seconds)
                .await?
            {
                CopilotDevicePoll::Authorized { access_token } => {
                    return self.store_access_token(deployment, &access_token).and_then(
                        |credential| {
                            store.save(&credential).map_err(CopilotOAuthError::Store)?;
                            Ok(credential)
                        },
                    );
                }
                CopilotDevicePoll::Pending { .. } => {}
                CopilotDevicePoll::SlowDown {
                    interval_seconds: next,
                    ..
                } => interval_seconds = next,
            }
        }
        Err(CopilotOAuthError::DevicePollingTimeout {
            provider: ProviderId::github_copilot(),
        })
    }

    fn store_access_token(
        &self,
        deployment: &CopilotDeployment,
        access_token: &str,
    ) -> Result<StoredCredential, CopilotOAuthError> {
        let token = non_empty(access_token).ok_or(CopilotOAuthError::MissingAccessToken)?;
        // reference implementation's Copilot plugin stores the same GitHub device access token as
        // both `access` and `refresh`, and sends that value directly as the
        // Copilot bearer. There is no GitHub→Copilot token exchange in that
        // reference path.
        let mut credential = StoredCredential::oauth(
            ProviderId::github_copilot(),
            token,
            token,
            None,
            self.clock.now_rfc3339(),
        );
        credential.enterprise_url = deployment.stored_enterprise_url();
        credential.scopes = vec![COPILOT_SCOPE.to_string()];
        Ok(credential)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CopilotDeviceAuthorization {
    pub verification_uri: String,
    pub user_code: String,
    pub device_code: String,
    pub interval_seconds: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CopilotDevicePoll {
    Pending {
        wait: Duration,
    },
    SlowDown {
        interval_seconds: u64,
        wait: Duration,
    },
    Authorized {
        access_token: String,
    },
}

#[derive(Debug, Deserialize)]
struct CopilotDeviceAuthorizationResponse {
    verification_uri: String,
    user_code: String,
    device_code: String,
    interval: Option<u64>,
}

#[derive(Debug, Deserialize)]
struct CopilotDeviceTokenResponse {
    #[serde(default)]
    access_token: Option<String>,
    #[serde(default)]
    error: Option<String>,
    #[serde(default)]
    interval: Option<u64>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct CopilotFallbackModel {
    pub id: &'static str,
    pub family: &'static str,
    pub context_window_tokens: u32,
    pub supports_vision: bool,
}

pub fn copilot_offline_fallback_models() -> &'static [CopilotFallbackModel] {
    &[
        CopilotFallbackModel {
            id: "gpt-5.5",
            family: "gpt",
            context_window_tokens: 272_000,
            supports_vision: true,
        },
        CopilotFallbackModel {
            id: "claude-sonnet-4.5",
            family: "claude-sonnet",
            context_window_tokens: 200_000,
            supports_vision: true,
        },
    ]
}

#[derive(Debug, Error)]
pub enum CopilotOAuthError {
    #[error("GitHub Copilot OAuth HTTP failure: {message}")]
    Http { message: String },
    #[error("GitHub Copilot OAuth {operation} failed with status {status}")]
    HttpStatus {
        operation: &'static str,
        status: u16,
    },
    #[error("GitHub Copilot OAuth {operation} returned malformed JSON: {message}")]
    Json {
        operation: &'static str,
        message: String,
    },
    #[error("GitHub Copilot OAuth {operation} response was malformed: {message}")]
    MalformedResponse {
        operation: &'static str,
        message: String,
    },
    #[error("GitHub Copilot authorization was denied")]
    AccessDenied,
    #[error("GitHub Copilot device code expired")]
    ExpiredToken,
    #[error("GitHub Copilot OAuth returned error `{error}`")]
    OAuthError { error: String },
    #[error("GitHub Copilot device authorization timed out for {provider}")]
    DevicePollingTimeout { provider: AuthProviderId },
    #[error("GitHub Copilot OAuth token response did not include an access token")]
    MissingAccessToken,
    #[error("invalid GitHub Enterprise URL or domain `{input}`: {reason}")]
    InvalidEnterpriseDomain { input: String, reason: String },
    #[error("credential store error: {0}")]
    Store(#[from] CredentialStoreError),
}

impl CopilotOAuthError {
    pub fn category(&self) -> ProviderErrorCategory {
        match self {
            Self::Http { .. } | Self::HttpStatus { .. } | Self::Json { .. } => {
                ProviderErrorCategory::TransportFailure
            }
            Self::MalformedResponse { .. }
            | Self::AccessDenied
            | Self::ExpiredToken
            | Self::OAuthError { .. }
            | Self::DevicePollingTimeout { .. }
            | Self::MissingAccessToken
            | Self::InvalidEnterpriseDomain { .. } => ProviderErrorCategory::InvalidCredentials,
            Self::Store(_) => ProviderErrorCategory::Other,
        }
    }
}

pub fn normalize_enterprise_domain(input: &str) -> Result<String, CopilotOAuthError> {
    let trimmed = input.trim().trim_end_matches('/');
    if trimmed.is_empty() {
        return Err(CopilotOAuthError::InvalidEnterpriseDomain {
            input: input.to_string(),
            reason: "domain is required".to_string(),
        });
    }
    let without_scheme = trimmed
        .strip_prefix("https://")
        .or_else(|| trimmed.strip_prefix("http://"))
        .unwrap_or(trimmed);
    if without_scheme.is_empty()
        || without_scheme.contains('/')
        || without_scheme.contains('\\')
        || without_scheme.contains('?')
        || without_scheme.contains('#')
        || without_scheme.chars().any(char::is_whitespace)
        || without_scheme.starts_with('.')
        || without_scheme.ends_with('.')
    {
        return Err(CopilotOAuthError::InvalidEnterpriseDomain {
            input: input.to_string(),
            reason: "expected a URL host or bare domain without path/query/fragment".to_string(),
        });
    }
    Ok(without_scheme.to_ascii_lowercase())
}

fn poll_wait(interval_seconds: u64) -> Duration {
    Duration::from_secs(interval_seconds).saturating_add(COPILOT_POLLING_SAFETY_MARGIN)
}

fn json_accept_headers() -> BTreeMap<String, String> {
    BTreeMap::from([
        ("Accept".to_string(), "application/json".to_string()),
        ("Content-Type".to_string(), "application/json".to_string()),
        (
            "User-Agent".to_string(),
            concat!("harness/", env!("CARGO_PKG_VERSION")).to_string(),
        ),
    ])
}

fn non_empty(value: &str) -> Option<&str> {
    let trimmed = value.trim();
    (!trimmed.is_empty()).then_some(trimmed)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::UnwrapOrAbort;
    use std::collections::VecDeque;
    use std::sync::Mutex;
    use std::time::SystemTime;

    #[derive(Debug)]
    struct FixedClock(SystemTime);

    impl CredentialClock for FixedClock {
        fn now(&self) -> SystemTime {
            self.0
        }
    }

    #[derive(Debug)]
    struct MockCopilotHttpClient {
        responses: Mutex<VecDeque<AuthHttpResponse>>,
        requests: Mutex<Vec<AuthHttpRequest>>,
    }

    impl MockCopilotHttpClient {
        fn new(responses: impl IntoIterator<Item = AuthHttpResponse>) -> Arc<Self> {
            Arc::new(Self {
                responses: Mutex::new(responses.into_iter().collect()),
                requests: Mutex::new(Vec::new()),
            })
        }

        fn requests(&self) -> Vec<AuthHttpRequest> {
            self.requests.lock().unwrap_or_abort().clone()
        }
    }

    #[async_trait]
    impl CopilotAuthHttpClient for MockCopilotHttpClient {
        async fn send(
            &self,
            request: AuthHttpRequest,
        ) -> Result<AuthHttpResponse, CopilotOAuthError> {
            self.requests.lock().unwrap_or_abort().push(request);
            self.responses
                .lock()
                .unwrap_or_abort()
                .pop_front()
                .ok_or_else(|| CopilotOAuthError::Http {
                    message: "no mocked auth response".to_string(),
                })
        }
    }

    fn response(status: u16, body: impl Into<String>) -> AuthHttpResponse {
        AuthHttpResponse {
            status,
            body: body.into(),
        }
    }

    #[tokio::test]
    async fn copilot_device_flow_uses_direct_github_token_and_stores_public_credential() {
        // arrange
        // act
        // assert
        let temp = tempfile::tempdir().unwrap_or_abort();
        let store = CredentialStore::new(temp.path());
        let http = MockCopilotHttpClient::new([
            response(
                200,
                r#"{"verification_uri":"https://github.com/login/device","user_code":"ABCD-EFGH","device_code":"device-123","interval":2}"#,
            ),
            response(200, r#"{"error":"authorization_pending"}"#),
            response(200, r#"{"error":"slow_down","interval":8}"#),
            response(200, r#"{"access_token":"gho_direct_copilot"}"#),
        ]);
        let http_dyn: Arc<dyn CopilotAuthHttpClient> = Arc::<MockCopilotHttpClient>::clone(&http);
        let client = CopilotOAuthClient::new(http_dyn).with_clock(Arc::new(FixedClock(
            humantime::parse_rfc3339("2026-05-30T00:00:00Z").unwrap_or_abort(),
        )));

        let credential = client
            .complete_device_flow(&CopilotDeployment::public(), &store, 4)
            .await
            .unwrap_or_abort();

        assert_eq!(credential.provider, ProviderId::github_copilot());
        assert_eq!(
            credential.access_token.as_deref(),
            Some("gho_direct_copilot")
        );
        assert_eq!(
            credential.refresh_token.as_deref(),
            Some("gho_direct_copilot")
        );
        assert_eq!(credential.enterprise_url, None);
        assert_eq!(credential.scopes, vec![COPILOT_SCOPE]);

        let stored = store
            .load(&ProviderId::github_copilot())
            .unwrap_or_abort()
            .unwrap_or_abort();
        assert_eq!(stored.access_token, credential.access_token);

        let requests = http.requests();
        assert_eq!(requests.len(), 4);
        assert_eq!(
            requests[0].url,
            "https://github.com/login/device/code".to_string()
        );
        assert_eq!(
            requests[1].url,
            "https://github.com/login/oauth/access_token".to_string()
        );
        assert!(
            requests
                .iter()
                .all(|request| !request.url.contains("copilot_internal")),
            "The reference flow uses the GitHub device token directly as Copilot bearer"
        );
    }

    #[tokio::test]
    async fn copilot_device_poll_honors_pending_and_slow_down_safety_margin() {
        // arrange
        // act
        // assert
        let http = MockCopilotHttpClient::new([
            response(200, r#"{"error":"authorization_pending"}"#),
            response(200, r#"{"error":"slow_down"}"#),
            response(200, r#"{"error":"slow_down","interval":11}"#),
        ]);
        let client = CopilotOAuthClient::new(http);
        let device = CopilotDeviceAuthorization {
            verification_uri: "https://github.com/login/device".to_string(),
            user_code: "ABCD-EFGH".to_string(),
            device_code: "device-123".to_string(),
            interval_seconds: 2,
        };

        assert_eq!(
            client
                .poll_device_token(&CopilotDeployment::public(), &device, 2)
                .await
                .unwrap_or_abort(),
            CopilotDevicePoll::Pending {
                wait: Duration::from_secs(5)
            }
        );
        assert_eq!(
            client
                .poll_device_token(&CopilotDeployment::public(), &device, 2)
                .await
                .unwrap_or_abort(),
            CopilotDevicePoll::SlowDown {
                interval_seconds: 7,
                wait: Duration::from_secs(10)
            }
        );
        assert_eq!(
            client
                .poll_device_token(&CopilotDeployment::public(), &device, 7)
                .await
                .unwrap_or_abort(),
            CopilotDevicePoll::SlowDown {
                interval_seconds: 11,
                wait: Duration::from_secs(14)
            }
        );
    }

    #[tokio::test]
    async fn copilot_device_errors_fail_cleanly_without_storing_credentials() {
        // arrange
        // act
        // assert
        let cases = [
            ("access_denied", r#"{"error":"access_denied"}"#),
            ("expired_token", r#"{"error":"expired_token"}"#),
            ("malformed", r#"{"not_access_token":true}"#),
        ];
        for (name, poll_body) in cases {
            let temp = tempfile::tempdir().unwrap_or_abort();
            let store = CredentialStore::new(temp.path());
            let http = MockCopilotHttpClient::new([
                response(
                    200,
                    r#"{"verification_uri":"https://github.com/login/device","user_code":"ABCD-EFGH","device_code":"device-123","interval":2}"#,
                ),
                response(200, poll_body),
            ]);
            let err = CopilotOAuthClient::new(http)
                .complete_device_flow(&CopilotDeployment::public(), &store, 1)
                .await
                .expect_err(name);

            match name {
                "access_denied" => assert!(matches!(err, CopilotOAuthError::AccessDenied)),
                "expired_token" => assert!(matches!(err, CopilotOAuthError::ExpiredToken)),
                "malformed" => assert!(matches!(
                    err,
                    CopilotOAuthError::MalformedResponse {
                        operation: "device poll",
                        ..
                    }
                )),
                _ => std::process::abort(),
            }
            assert!(
                store
                    .load(&ProviderId::github_copilot())
                    .unwrap_or_abort()
                    .is_none(),
                "{name} should not store credentials"
            );
        }
    }

    #[tokio::test]
    async fn copilot_device_timeout_stores_no_credentials() {
        // arrange
        // act
        // assert
        let temp = tempfile::tempdir().unwrap_or_abort();
        let store = CredentialStore::new(temp.path());
        let http = MockCopilotHttpClient::new([
            response(
                200,
                r#"{"verification_uri":"https://github.com/login/device","user_code":"ABCD-EFGH","device_code":"device-123","interval":2}"#,
            ),
            response(200, r#"{"error":"authorization_pending"}"#),
        ]);
        let err = CopilotOAuthClient::new(http)
            .complete_device_flow(&CopilotDeployment::public(), &store, 1)
            .await
            .expect_err("timeout");

        match &err {
            CopilotOAuthError::DevicePollingTimeout { provider } => {
                assert_eq!(*provider, ProviderId::github_copilot());
            }
            _ => panic!("expected DevicePollingTimeout, got {err:?}"),
        }
        assert!(store
            .load(&ProviderId::github_copilot())
            .unwrap_or_abort()
            .is_none());
    }

    #[tokio::test]
    async fn copilot_enterprise_normalizes_and_stores_domain() {
        // arrange
        // act
        // assert
        assert_eq!(
            normalize_enterprise_domain("https://GHE.Example.COM/").unwrap_or_abort(),
            "ghe.example.com"
        );
        assert_eq!(
            normalize_enterprise_domain("company.ghe.com").unwrap_or_abort(),
            "company.ghe.com"
        );
        assert!(normalize_enterprise_domain("").is_err());
        assert!(normalize_enterprise_domain("https://ghe.example.com/path").is_err());
        assert!(normalize_enterprise_domain("not a domain").is_err());

        let temp = tempfile::tempdir().unwrap_or_abort();
        let store = CredentialStore::new(temp.path());
        let deployment =
            CopilotDeployment::enterprise("https://ghe.example.com/").unwrap_or_abort();
        assert_eq!(deployment.oauth_domain(), "ghe.example.com");
        assert_eq!(
            deployment.api_base_url(),
            "https://copilot-api.ghe.example.com"
        );

        let http = MockCopilotHttpClient::new([
            response(
                200,
                r#"{"verification_uri":"https://ghe.example.com/login/device","user_code":"ABCD-EFGH","device_code":"device-123","interval":2}"#,
            ),
            response(200, r#"{"access_token":"ghe_direct_copilot"}"#),
        ]);
        let credential = CopilotOAuthClient::new(http)
            .complete_device_flow(&deployment, &store, 1)
            .await
            .unwrap_or_abort();
        assert_eq!(
            credential.enterprise_url.as_deref(),
            Some("ghe.example.com")
        );
    }

    #[test]
    fn copilot_offline_fallback_models_cover_gpt_and_claude_families() {
        // arrange
        // act
        // assert
        let fallback = copilot_offline_fallback_models();
        assert!(fallback
            .iter()
            .any(|model| model.id.starts_with("gpt-") && model.family == "gpt"));
        assert!(fallback
            .iter()
            .any(|model| model.id.starts_with("claude") && model.supports_vision));
    }
}
