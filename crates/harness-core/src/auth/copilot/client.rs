use super::super::codex::AuthHttpMethod;
use super::*;
use serde_json::Value;
use std::sync::Arc;
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
        let value = self
            .post(
                deployment,
                "/login/device/code",
                serde_json::json!({"client_id": COPILOT_CLIENT_ID, "scope": COPILOT_SCOPE}),
                "device authorization",
            )
            .await?;
        #[derive(Deserialize)]
        struct Device {
            verification_uri: String,
            user_code: String,
            device_code: String,
            interval: Option<u64>,
        }
        let device: Device = serde_json::from_value(value)
            .map_err(|_| malformed("device authorization", "invalid device response"))?;
        let verification = super::super::oauth_http::validate_endpoint(&device.verification_uri)
            .map_err(|_| malformed("device authorization", "invalid verification URL"))?;
        if verification.scheme() != "https"
            || verification.host_str() != Some(deployment.oauth_domain())
            || !usable_token(&device.device_code)
            || !usable_token(&device.user_code)
        {
            return Err(malformed(
                "device authorization",
                "invalid verification origin or code",
            ));
        }
        let interval_seconds = device.interval.unwrap_or(5).max(1);
        let _ = poll_wait(interval_seconds)?;
        Ok(CopilotDeviceAuthorization {
            verification_uri: device.verification_uri,
            user_code: device.user_code,
            device_code: device.device_code,
            interval_seconds,
        })
    }
    pub async fn poll_device_token(
        &self,
        deployment: &CopilotDeployment,
        device: &CopilotDeviceAuthorization,
        current_interval_seconds: u64,
    ) -> Result<CopilotDevicePoll, CopilotOAuthError> {
        let value = self.post(deployment, "/login/oauth/access_token", serde_json::json!({"client_id": COPILOT_CLIENT_ID, "device_code": device.device_code, "grant_type": "urn:ietf:params:oauth:grant-type:device_code"}), "device poll").await?;
        if let Some(token) = value
            .get("access_token")
            .and_then(Value::as_str)
            .filter(|v| usable_token(v))
        {
            return Ok(CopilotDevicePoll::Authorized {
                access_token: token.into(),
            });
        }
        match value.get("error").and_then(Value::as_str) {
            Some("authorization_pending") => Ok(CopilotDevicePoll::Pending {
                wait: poll_wait(current_interval_seconds.max(1))?,
            }),
            Some("slow_down") => {
                let interval_seconds = current_interval_seconds
                    .checked_add(5)
                    .ok_or_else(|| malformed("device poll", "polling interval overflow"))?
                    .max(value.get("interval").and_then(Value::as_u64).unwrap_or(0));
                Ok(CopilotDevicePoll::SlowDown {
                    interval_seconds,
                    wait: poll_wait(interval_seconds)?,
                })
            }
            Some("access_denied") => Err(CopilotOAuthError::AccessDenied),
            Some("expired_token" | "token_expired") => Err(CopilotOAuthError::ExpiredToken),
            Some(
                "unsupported_grant_type"
                | "incorrect_client_credentials"
                | "incorrect_device_code"
                | "device_flow_disabled",
            ) => Err(CopilotOAuthError::OAuthError {
                error: value["error"].as_str().unwrap_or("oauth_error").into(),
            }),
            Some(_) => Err(CopilotOAuthError::OAuthError {
                error: "unrecognized_error".into(),
            }),
            None => Err(malformed(
                "device poll",
                "missing access token or OAuth outcome",
            )),
        }
    }
    pub async fn complete_device_flow(
        &self,
        deployment: &CopilotDeployment,
        store: &CredentialStore,
        max_polls: usize,
    ) -> Result<StoredCredential, CopilotOAuthError> {
        let timed_out = || CopilotOAuthError::DevicePollingTimeout {
            provider: ProviderId::github_copilot(),
        };
        if max_polls == 0 {
            return Err(timed_out());
        }
        tokio::time::timeout(Duration::from_secs(900), async {
            let device = self.start_device_authorization(deployment).await?;
            let mut interval = device.interval_seconds;
            let mut wait = poll_wait(interval)?;
            for _ in 0..max_polls {
                if !wait.is_zero() {
                    tokio::time::sleep(wait).await;
                }
                match self
                    .poll_device_token(deployment, &device, interval)
                    .await?
                {
                    CopilotDevicePoll::Pending { wait: next } => wait = next,
                    CopilotDevicePoll::SlowDown {
                        interval_seconds,
                        wait: next,
                    } => {
                        interval = interval_seconds;
                        wait = next;
                    }
                    CopilotDevicePoll::Authorized { access_token } => {
                        return self.store_access_token(deployment, store, &access_token);
                    }
                }
            }
            Err(timed_out())
        })
        .await
        .map_err(|_| timed_out())?
    }
    pub fn store_access_token(
        &self,
        deployment: &CopilotDeployment,
        store: &CredentialStore,
        token: &str,
    ) -> Result<StoredCredential, CopilotOAuthError> {
        let mut credential = StoredCredential::oauth(
            ProviderId::github_copilot(),
            token,
            token,
            None,
            self.clock.now_rfc3339(),
        );
        if let CopilotDeployment::Enterprise { domain } = deployment {
            credential.enterprise_url = Some(domain.clone());
        }
        credential.scopes = vec![COPILOT_SCOPE.into()];
        store.save(&credential)?;
        Ok(credential)
    }
    async fn post(
        &self,
        deployment: &CopilotDeployment,
        path: &str,
        body: Value,
        operation: &'static str,
    ) -> Result<Value, CopilotOAuthError> {
        let domain = normalize_enterprise_domain(deployment.oauth_domain())?;
        let response = self
            .http
            .send(AuthHttpRequest {
                method: AuthHttpMethod::Post,
                url: format!("https://{domain}{path}"),
                body: body.to_string(),
                headers: BTreeMap::from([
                    ("Content-Type".into(), "application/json".into()),
                    ("Accept".into(), "application/json".into()),
                    (
                        "User-Agent".into(),
                        concat!("agent-harness/", env!("CARGO_PKG_VERSION")).into(),
                    ),
                ]),
            })
            .await?;
        if !(200..300).contains(&response.status) {
            return Err(CopilotOAuthError::HttpStatus {
                operation,
                status: response.status,
            });
        }
        if response.body.len() > 1024 * 1024 {
            return Err(malformed(operation, "response exceeds 1 MiB"));
        }
        serde_json::from_str(&response.body).map_err(|_| CopilotOAuthError::Json {
            operation,
            message: "invalid response fields".into(),
        })
    }
}
fn malformed(operation: &'static str, message: &str) -> CopilotOAuthError {
    CopilotOAuthError::MalformedResponse {
        operation,
        message: message.into(),
    }
}
fn poll_wait(seconds: u64) -> Result<Duration, CopilotOAuthError> {
    let wait = Duration::from_secs(seconds)
        .checked_add(COPILOT_POLLING_SAFETY_MARGIN)
        .ok_or_else(|| malformed("device poll", "polling interval overflow"))?;
    if tokio::time::Instant::now().checked_add(wait).is_none() {
        return Err(malformed("device poll", "polling deadline overflow"));
    }
    Ok(wait)
}
