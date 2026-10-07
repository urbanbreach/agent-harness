use super::*;
use harness_providers::{HttpProvider, Protocol};

pub(crate) fn provider_map(
    config: &HarnessConfig,
    deps: &CliDeps,
    secrets: &Arc<harness_core::redact::SecretRegistry>,
) -> Result<BTreeMap<String, Arc<dyn Provider>>, String> {
    let store = CredentialStore::from_lookup(&|name| deps.env_var_value(name));
    let mut providers = BTreeMap::<String, Arc<dyn Provider>>::new();
    for (name, definition) in &config.providers {
        if config.disabled_providers.contains(name)
            || !config.enabled_providers.is_empty() && !config.enabled_providers.contains(name)
        {
            continue;
        }
        if let ProviderConfig::AnthropicSubscription(subscription) = definition {
            providers.insert(
                name.clone(),
                anthropic_subscription_provider(subscription, store.as_ref(), deps)?,
            );
            continue;
        }
        let (base, protocol, key, env, timeout, headers) = match definition {
            ProviderConfig::OpenAiCompatible(p) => (
                &p.base_url,
                if p.api_mode != OpenAiApiMode::ChatCompletions {
                    Protocol::Responses
                } else {
                    Protocol::Chat
                },
                &p.api_key,
                &p.api_key_env,
                p.timeout_ms,
                &p.headers,
            ),
            ProviderConfig::Anthropic(p) => (
                &p.base_url,
                Protocol::Anthropic,
                &p.api_key,
                &p.api_key_env,
                p.timeout_ms,
                &p.headers,
            ),
            ProviderConfig::AnthropicSubscription(_) => continue,
        };
        let suffix = match protocol {
            Protocol::Chat => "/chat/completions",
            Protocol::Responses => "/responses",
            Protocol::Anthropic => "/messages",
        };
        let mut endpoint = reqwest::Url::parse(base).map_err(|_| "invalid provider URL")?;
        let path = endpoint.path().trim_end_matches('/');
        let path = if path.ends_with(suffix) {
            path.to_owned()
        } else {
            format!("{path}{suffix}")
        };
        endpoint.set_path(&path);
        let mut provider =
            HttpProvider::new(endpoint.as_str(), protocol, Duration::from_millis(timeout))?;
        if matches!(definition, ProviderConfig::OpenAiCompatible(p) if p.api_mode == OpenAiApiMode::Auto)
        {
            provider = provider.with_chat_fallback();
        }
        let mut extra_headers = reqwest::header::HeaderMap::new();
        for (key, value) in headers {
            extra_headers.insert(
                reqwest::header::HeaderName::try_from(key)
                    .map_err(|_| "invalid provider header name")?,
                reqwest::header::HeaderValue::try_from(value)
                    .map_err(|_| "invalid provider header value")?,
            );
        }
        provider = provider.with_headers(extra_headers);
        let auth = definition
            .credential_provider(name)
            .ok_or("invalid provider ID")?;
        if auth == ProviderId::codex() {
            provider = provider.with_auth_profile(harness_providers::ProviderAuthProfile::Codex);
        } else if auth == ProviderId::github_copilot() {
            provider =
                provider.with_auth_profile(harness_providers::ProviderAuthProfile::GithubCopilot);
        }
        let stored = store
            .as_ref()
            .is_some_and(|store| store.credential_path(&auth).exists());
        if stored || !key.is_empty() || !env.is_empty() {
            let deps = deps.clone();
            let mut manager = ProviderCredentialManager::new(
                store.clone(),
                auth.clone(),
                env.clone(),
                key,
                move |name| deps.env_var_value(name),
            )
            .with_secret_registry(Arc::clone(secrets))
            .map_err(|e| e.to_string())?;
            if auth == ProviderId::codex() {
                let http =
                    harness_core::auth::ReqwestAuthHttpClient::new().map_err(|e| e.to_string())?;
                manager = manager.with_refresher(Arc::new(
                    harness_core::auth::codex::CodexOAuthClient::new(Arc::new(http)),
                ));
            }
            provider = provider.with_credentials(Arc::new(manager));
        }
        providers.insert(name.clone(), Arc::new(provider));
    }
    Ok(providers)
}

fn anthropic_subscription_provider(
    config: &harness_core::config::AnthropicSubscriptionProviderConfig,
    store: Option<&CredentialStore>,
    deps: &CliDeps,
) -> Result<Arc<dyn Provider>, String> {
    use harness_providers::anthropic_subscription::AnthropicSubscriptionProvider;
    let cwd = deps.current_dir().map_err(|e| e.to_string())?;
    let windows = config
        .models
        .iter()
        .filter_map(|(id, model)| {
            model
                .limit
                .context
                .or(model.metadata.context_window_tokens)
                .map(|w| (id.clone(), u64::from(w)))
        })
        .collect();
    let mut provider = AnthropicSubscriptionProvider::new(cwd)
        .with_settings(config.settings())
        .with_context_windows(windows)
        .with_environment(deps.environment_snapshot());
    if let Some(store) = store {
        let http = harness_core::auth::ReqwestAuthHttpClient::new().map_err(|e| e.to_string())?;
        let oauth = harness_core::auth::anthropic::AnthropicOAuthClient::new(Arc::new(http));
        provider = provider
            .with_agent_dir(store.data_dir().to_path_buf())
            .with_store(Arc::new(
                harness_core::auth::anthropic_subscription::AnthropicSubscriptionAccounts::new(
                    store.clone(),
                    Arc::new(oauth),
                ),
            ));
    }
    Ok(Arc::new(provider))
}
