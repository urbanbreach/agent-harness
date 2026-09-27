use super::*;
use std::{
    sync::Arc,
    time::{Duration, UNIX_EPOCH},
};
pub struct CodexOAuthClient {
    issuer: String,
    http: Arc<dyn AuthHttpClient>,
    clock: Arc<dyn CredentialClock>,
}
impl CodexOAuthClient {
    pub fn new(http: Arc<dyn AuthHttpClient>) -> Self {
        Self {
            issuer: CODEX_ISSUER.into(),
            http,
            clock: Arc::new(SystemCredentialClock),
        }
    }
    pub fn with_issuer(mut self, issuer: impl Into<String>) -> Self {
        self.issuer = issuer.into();
        self
    }
    pub fn with_clock(mut self, clock: Arc<dyn CredentialClock>) -> Self {
        self.clock = clock;
        self
    }
    pub async fn complete_loopback_callback(
        &self,
        session: &CodexLoopbackSession,
        callback_url: &str,
        store: &CredentialStore,
    ) -> Result<StoredCredential, CodexOAuthError> {
        let code = session.callback_code(callback_url)?;
        let tokens = self
            .exchange_authorization_code(&code, &session.redirect_uri, &session.pkce)
            .await?;
        self.store_tokens(store, tokens)
    }
    pub async fn exchange_authorization_code(
        &self,
        code: &str,
        redirect_uri: &str,
        pkce: &PkceCodes,
    ) -> Result<CodexTokenResponse, CodexOAuthError> {
        if !usable_token(code) {
            return Err(CodexOAuthError::MissingCode);
        }
        self.token_request(&[
            ("grant_type", "authorization_code"),
            ("code", code),
            ("redirect_uri", redirect_uri),
            ("client_id", CODEX_CLIENT_ID),
            ("code_verifier", &pkce.verifier),
        ])
        .await
    }
    pub async fn refresh_access_token(
        &self,
        refresh_token: &str,
    ) -> Result<CodexTokenResponse, CodexOAuthError> {
        if !usable_token(refresh_token) {
            return Err(CodexOAuthError::MissingRefreshToken);
        }
        let mut tokens = self
            .token_request(&[
                ("grant_type", "refresh_token"),
                ("refresh_token", refresh_token),
                ("client_id", CODEX_CLIENT_ID),
            ])
            .await?;
        if tokens.refresh_token.is_empty() {
            tokens.refresh_token = refresh_token.into();
        }
        Ok(tokens)
    }
    async fn token_request(
        &self,
        fields: &[(&str, &str)],
    ) -> Result<CodexTokenResponse, CodexOAuthError> {
        let body = serde_urlencoded::to_string(fields)
            .map_err(|_| malformed("token", "cannot encode token request"))?;
        let response = self
            .post("/oauth/token", "application/x-www-form-urlencoded", body)
            .await?;
        let tokens: CodexTokenResponse = decode(&response, "token")?;
        if !usable_token(&tokens.access_token) {
            return Err(CodexOAuthError::MissingAccessToken);
        }
        Ok(tokens)
    }
    pub async fn start_device_authorization(
        &self,
    ) -> Result<CodexDeviceAuthorization, CodexOAuthError> {
        let body = serde_json::json!({"client_id": CODEX_CLIENT_ID}).to_string();
        let response = self
            .post(
                "/api/accounts/deviceauth/usercode",
                "application/json",
                body,
            )
            .await?;
        let value: Value = decode(&response, "device authorization")?;
        let string = |key| {
            value
                .get(key)
                .and_then(Value::as_str)
                .filter(|v| usable_token(v))
                .map(str::to_owned)
                .ok_or_else(|| malformed("device authorization", "required field is missing"))
        };
        let interval = value
            .get("interval")
            .and_then(|v| v.as_u64().or_else(|| v.as_str()?.parse().ok()))
            .unwrap_or(5)
            .max(1);
        if interval > 300 {
            return Err(malformed(
                "device authorization",
                "polling interval exceeds 5 minutes",
            ));
        }
        Ok(CodexDeviceAuthorization {
            device_auth_id: string("device_auth_id")?,
            user_code: string("user_code")?,
            interval_seconds: interval,
            verification_uri: self.endpoint("/codex/device")?,
        })
    }
    pub async fn poll_device_authorization(
        &self,
        device: &CodexDeviceAuthorization,
    ) -> Result<CodexDevicePoll, CodexOAuthError> {
        let body = serde_json::json!({"device_auth_id": device.device_auth_id, "user_code": device.user_code}).to_string();
        let response = self
            .post("/api/accounts/deviceauth/token", "application/json", body)
            .await?;
        if matches!(response.status, 403 | 404) {
            return Ok(CodexDevicePoll::Pending);
        }
        #[derive(Deserialize)]
        struct Grant {
            authorization_code: String,
            code_verifier: String,
        }
        let grant: Grant = decode(&response, "device poll")?;
        if !usable_token(&grant.authorization_code) || !usable_token(&grant.code_verifier) {
            return Err(malformed("device poll", "authorization grant is empty"));
        }
        Ok(CodexDevicePoll::Authorized {
            authorization_code: grant.authorization_code,
            code_verifier: grant.code_verifier,
        })
    }
    pub async fn complete_device_flow(
        &self,
        store: &CredentialStore,
        max_polls: usize,
    ) -> Result<StoredCredential, CodexOAuthError> {
        if max_polls == 0 {
            return Err(CodexOAuthError::DevicePollingTimeout {
                provider: ProviderId::codex(),
            });
        }
        let device = self.start_device_authorization().await?;
        for attempt in 0..max_polls {
            if attempt > 0 {
                tokio::time::sleep(Duration::from_secs(
                    device.interval_seconds.saturating_add(3),
                ))
                .await;
            }
            match self.poll_device_authorization(&device).await? {
                CodexDevicePoll::Pending => {}
                CodexDevicePoll::Authorized {
                    authorization_code,
                    code_verifier,
                } => {
                    let pkce = PkceCodes {
                        verifier: code_verifier,
                        challenge: String::new(),
                    };
                    let tokens = self
                        .exchange_authorization_code(
                            &authorization_code,
                            &self.endpoint("/deviceauth/callback")?,
                            &pkce,
                        )
                        .await?;
                    return self.store_tokens(store, tokens);
                }
            }
        }
        Err(CodexOAuthError::DevicePollingTimeout {
            provider: ProviderId::codex(),
        })
    }
    pub fn store_tokens(
        &self,
        store: &CredentialStore,
        tokens: CodexTokenResponse,
    ) -> Result<StoredCredential, CodexOAuthError> {
        if !usable_token(&tokens.access_token) {
            return Err(CodexOAuthError::MissingAccessToken);
        }
        if !usable_token(&tokens.refresh_token) {
            return Err(CodexOAuthError::MissingRefreshToken);
        }
        let account = extract_account_id(&tokens);
        let expires = self.expires_at(tokens.expires_in)?;
        let mut credential = StoredCredential::oauth(
            ProviderId::codex(),
            tokens.access_token,
            tokens.refresh_token,
            Some(expires),
            self.clock.now_rfc3339(),
        );
        credential.account_id = account;
        store.save(&credential)?;
        Ok(credential)
    }
    fn expires_at(&self, seconds: Option<u64>) -> Result<String, CodexOAuthError> {
        let expires = self
            .clock
            .now()
            .checked_add(Duration::from_secs(seconds.unwrap_or(3600)))
            .ok_or_else(|| malformed("token", "expiry is out of range"))?;
        let unix = expires
            .duration_since(UNIX_EPOCH)
            .map_err(|_| malformed("token", "expiry precedes 1970"))?;
        if unix.as_secs() >= 253_402_300_800 {
            return Err(malformed("token", "expiry exceeds RFC 3339 range"));
        }
        Ok(humantime::format_rfc3339(expires).to_string())
    }
    fn endpoint(&self, path: &str) -> Result<String, CodexOAuthError> {
        let url = format!("{}{path}", self.issuer.trim_end_matches('/'));
        super::super::oauth_http::validate_endpoint(&url)?;
        Ok(url)
    }
    async fn post(
        &self,
        path: &str,
        mime: &str,
        body: String,
    ) -> Result<AuthHttpResponse, CodexOAuthError> {
        self.http
            .send(AuthHttpRequest {
                method: AuthHttpMethod::Post,
                url: self.endpoint(path)?,
                body,
                headers: BTreeMap::from([
                    ("Content-Type".into(), mime.into()),
                    ("Accept".into(), "application/json".into()),
                    (
                        "User-Agent".into(),
                        concat!("agent-harness/", env!("CARGO_PKG_VERSION")).into(),
                    ),
                ]),
            })
            .await
    }
}
fn malformed(operation: &'static str, message: &str) -> CodexOAuthError {
    CodexOAuthError::Json {
        operation,
        message: message.into(),
    }
}
fn decode<T: serde::de::DeserializeOwned>(
    response: &AuthHttpResponse,
    operation: &'static str,
) -> Result<T, CodexOAuthError> {
    if !(200..300).contains(&response.status) {
        return Err(CodexOAuthError::HttpStatus {
            operation,
            status: response.status,
        });
    }
    if response.body.len() > 1024 * 1024 {
        return Err(malformed(operation, "response exceeds 1 MiB"));
    }
    serde_json::from_str(&response.body)
        .map_err(|_| malformed(operation, "invalid response fields"))
}
#[async_trait::async_trait]
impl OAuthTokenRefresher for CodexOAuthClient {
    async fn refresh(
        &self,
        provider: &ProviderId,
        credential: &StoredCredential,
    ) -> Result<OAuthRefreshOutcome, CredentialRefreshError> {
        if provider != &ProviderId::codex() || &credential.provider != provider {
            return Err(CredentialRefreshError::new(
                ProviderErrorCategory::InvalidCredentials,
                "wrong OAuth provider",
            ));
        }
        let refresh = async {
            let tokens = self
                .refresh_access_token(
                    credential
                        .refresh_token
                        .as_deref()
                        .ok_or(CodexOAuthError::MissingRefreshToken)?,
                )
                .await?;
            Ok::<_, CodexOAuthError>(OAuthRefreshOutcome {
                account_id: extract_account_id(&tokens),
                expires_at: Some(self.expires_at(tokens.expires_in)?),
                access_token: tokens.access_token,
                refresh_token: Some(tokens.refresh_token),
                scopes: Vec::new(),
            })
        }
        .await;
        refresh.map_err(|e| CredentialRefreshError::new(e.category(), "Codex token refresh failed"))
    }
}
