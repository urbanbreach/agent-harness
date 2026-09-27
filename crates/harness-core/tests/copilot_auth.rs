use harness_core::auth::{
    codex::{AuthHttpRequest, AuthHttpResponse},
    copilot::*,
    CredentialStore, ProviderId,
};
use std::{
    collections::VecDeque,
    sync::{Arc, Mutex},
    time::Duration,
};

#[derive(Default)]
struct Http {
    responses: Mutex<VecDeque<AuthHttpResponse>>,
    requests: Mutex<Vec<(tokio::time::Instant, AuthHttpRequest)>>,
}
#[async_trait::async_trait]
impl CopilotAuthHttpClient for Http {
    async fn send(&self, request: AuthHttpRequest) -> Result<AuthHttpResponse, CopilotOAuthError> {
        self.requests
            .lock()
            .map_err(|_| CopilotOAuthError::MissingAccessToken)?
            .push((tokio::time::Instant::now(), request));
        self.responses
            .lock()
            .map_err(|_| CopilotOAuthError::MissingAccessToken)?
            .pop_front()
            .ok_or(CopilotOAuthError::MissingAccessToken)
    }
}
#[tokio::test(start_paused = true)]
async fn copilot_device_flow_waits_after_pending_and_slow_down_and_keeps_enterprise_identity(
) -> Result<(), Box<dyn std::error::Error>> {
    let temp = tempfile::tempdir()?;
    let store = CredentialStore::new(temp.path());
    let deployment = CopilotDeployment::enterprise("https://GitHub.Example.test/")?;
    assert_eq!(deployment.oauth_domain(), "github.example.test");
    assert_eq!(
        deployment.api_base_url(),
        "https://copilot-api.github.example.test"
    );
    let http = Arc::new(Http::default());
    http.responses.lock().map_err(|_| "poison")?.extend([
        r#"{"verification_uri":"https://github.example.test/login/device","device_code":"private-device","user_code":"1234-5678","interval":2}"#,
        r#"{"error":"authorization_pending"}"#,
        r#"{"error":"slow_down","interval":1}"#,
        r#"{"access_token":"private-access"}"#,
        r#"{"error":"access_denied","error_description":"private-server-message"}"#,
    ].into_iter().map(|body| AuthHttpResponse { status: 200, body: body.into() }));
    let client = CopilotOAuthClient::new(Arc::clone(&http) as Arc<dyn CopilotAuthHttpClient>);
    let stored = client.complete_device_flow(&deployment, &store, 4).await?;
    assert_eq!(
        stored.enterprise_url.as_deref(),
        Some("github.example.test")
    );
    assert_eq!(stored.scopes, [COPILOT_SCOPE]);
    assert_eq!(stored.access_token.as_deref(), Some("private-access"));
    {
        let requests = http.requests.lock().map_err(|_| "poison")?;
        assert_eq!(requests.len(), 4);
        assert!(requests[1].0.duration_since(requests[0].0) >= Duration::from_secs(2));
        assert!(requests[2].0.duration_since(requests[1].0) >= Duration::from_secs(5));
        assert!(requests[3].0.duration_since(requests[2].0) >= Duration::from_secs(10));
        assert!(requests
            .iter()
            .all(|(_, r)| r.url.starts_with("https://github.example.test/")));
    }
    let device = CopilotDeviceAuthorization {
        verification_uri: "https://github.example.test/login/device".into(),
        user_code: "user".into(),
        device_code: "private-device".into(),
        interval_seconds: 2,
    };
    let error = client
        .poll_device_token(&deployment, &device, 2)
        .await
        .err()
        .ok_or("denial accepted")?;
    assert!(matches!(error, CopilotOAuthError::AccessDenied));
    assert!(!error.to_string().contains("private-"));
    assert_eq!(store.load(&ProviderId::github_copilot())?, Some(stored));
    Ok(())
}

#[tokio::test]
async fn copilot_rejects_unsafe_domains_and_empty_authorization_without_storage(
) -> Result<(), Box<dyn std::error::Error>> {
    for domain in [
        "https://user:secret@example.test",
        "https://example.test/path",
        "https://example.test?key=secret",
        "https://example.test#x",
        "example.test:8443",
        "file:///tmp/auth",
        "https://example.test\n",
    ] {
        assert!(normalize_enterprise_domain(domain).is_err());
    }
    let temp = tempfile::tempdir()?;
    let store = CredentialStore::new(temp.path());
    let http = Arc::new(Http::default());
    http.responses.lock().map_err(|_| "poison")?.push_back(AuthHttpResponse { status: 200, body: r#"{"verification_uri":"javascript:alert(1)","device_code":"device","user_code":"user","interval":2}"#.into() });
    let client = CopilotOAuthClient::new(http);
    assert!(client
        .complete_device_flow(&CopilotDeployment::public(), &store, 2)
        .await
        .is_err());
    assert_eq!(std::fs::read_dir(temp.path())?.count(), 0);
    Ok(())
}
