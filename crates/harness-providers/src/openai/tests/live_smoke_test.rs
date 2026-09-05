use super::*;
use crate::UnwrapOrAbort;

#[tokio::test]
#[ignore = "requires HARNESS_LIVE_PROXY=1 and local proxy access"]
async fn openai_compatible_live_proxy_config_file_smoke() {
    if env::var("HARNESS_LIVE_PROXY").as_deref() != Ok("1") {
        return;
    }

    let config_path = env::var("HARNESS_LIVE_PROXY_CONFIG")
        .map(PathBuf::from)
        .unwrap_or_else(|_| default_live_config_path());
    let provider_name =
        env::var("HARNESS_LIVE_PROXY_PROVIDER").unwrap_or_else(|_| "default".to_string());

    let live_config = load_live_config(&config_path).unwrap_or_else(|err| panic!("{err}"));

    let provider_config = live_config
        .providers
        .get(&provider_name)
        .unwrap_or_else(|| panic!("provider `{provider_name}` missing in live config"));

    assert_eq!(provider_config.provider_type, "openai_compatible");

    let provider = OpenAiCompatibleProvider::new(OpenAiCompatibleProviderConfig {
        base_url: provider_config.base_url.clone(),
        api_key: resolve_env_reference(&provider_config.api_key),
        api_mode: provider_config.api_mode,
        timeout_ms: provider_config.timeout_ms,
        headers: provider_config.headers.clone(),
    })
    .unwrap_or_abort();

    let model_id = env::var("HARNESS_LIVE_PROXY_MODEL")
        .ok()
        .or_else(|| provider_config.models.keys().next().cloned())
        .unwrap_or_abort();

    let mut stream = provider.stream_completion(basic_request(&model_id)).await;

    let mut saw_start = false;
    let mut saw_done = false;
    let mut delta_chars = 0usize;

    timeout(Duration::from_secs(45), async {
        while let Some(event) = stream.next().await {
            match event {
                ProviderStreamEvent::Start | ProviderStreamEvent::Started { .. } => {
                    saw_start = true
                }
                ProviderStreamEvent::ReasoningDelta(_) => {}
                ProviderStreamEvent::TextDelta(delta) => {
                    delta_chars += delta.len();
                }
                ProviderStreamEvent::ToolCallDelta { .. }
                | ProviderStreamEvent::ToolCallComplete { .. } => {}
                ProviderStreamEvent::Done { .. } | ProviderStreamEvent::DoneWithMetadata { .. } => {
                    saw_done = true;
                    break;
                }
                ProviderStreamEvent::Error { message, .. } => {
                    panic!("live proxy returned provider error: {message}")
                }
            }
        }
    })
    .await
    .unwrap_or_abort();

    assert!(saw_start, "expected a start event");
    assert!(saw_done, "expected a done event");
    assert!(delta_chars > 0, "expected at least one text delta");
}

#[test]
fn live_smoke_env_reference_supports_default_fallback_syntax() {
    assert_eq!(
        resolve_env_reference_with("${HARNESS_PROVIDER_TEST_API_KEY:-fallback-key}", |_| None),
        "fallback-key"
    );
    assert_eq!(
        resolve_env_reference_with("${HARNESS_PROVIDER_TEST_API_KEY:-fallback-key}", |_| {
            Some(String::new())
        }),
        "fallback-key"
    );
    assert_eq!(
        resolve_env_reference_with("${HARNESS_PROVIDER_TEST_API_KEY:-fallback-key}", |_| {
            Some("real-key".to_string())
        }),
        "real-key"
    );
}
