use super::*;

pub(crate) fn secret_values(
    config: &HarnessConfig,
    deps: &CliDeps,
    store: Option<&CredentialStore>,
) -> Result<Vec<String>, String> {
    let mut values = deps.credential_env_values();
    for provider in config.providers.values() {
        let (key, env, headers, base) = match provider {
            ProviderConfig::OpenAiCompatible(p) => {
                (&p.api_key, &p.api_key_env, &p.headers, &p.base_url)
            }
            ProviderConfig::Anthropic(p) => (&p.api_key, &p.api_key_env, &p.headers, &p.base_url),
            // Its tokens live in the credential store and `CLAUDE_CODE_OAUTH_TOKEN*`, both covered.
            ProviderConfig::AnthropicSubscription(_) => continue,
        };
        values.push(key.clone());
        values.extend(env.iter().filter_map(|name| deps.env_var_value(name)));
        values.extend(headers.values().cloned());
        let url = reqwest::Url::parse(base).map_err(|_| "invalid provider URL")?;
        values.extend(url.query_pairs().map(|(_, value)| value.into_owned()));
        values.extend(
            url.query()
                .into_iter()
                .flat_map(|query| query.split('&'))
                .filter_map(|pair| pair.split_once('=').map(|(_, value)| value.to_owned())),
        );
    }
    values.extend(config.integrations.remote_search.auth_token.iter().cloned());
    for server in config.integrations.mcp.servers.values() {
        let entries = match server {
            harness_core::config::McpServerConfig::Stdio { env, .. } => env,
            harness_core::config::McpServerConfig::Http { headers, .. } => headers,
        };
        values.extend(entries.values().cloned());
    }
    values.extend(
        config
            .lsp
            .servers
            .values()
            .flat_map(|server| server.env.values().cloned()),
    );
    values.extend(
        config
            .hooks
            .lifecycle
            .iter()
            .flat_map(|hook| hook.env.values().cloned()),
    );
    values.extend(
        config
            .formatter
            .overrides
            .values()
            .filter_map(|formatter| formatter.environment.as_ref())
            .flat_map(|env| env.values().cloned()),
    );
    if let Some(store) = store {
        for id in store.stored_provider_ids().map_err(|e| e.to_string())? {
            if let Some(credential) = store.load(&id).map_err(|e| e.to_string())? {
                values.extend(credential.secret_values());
            }
            if values.iter().map(String::len).sum::<usize>() > 4 * 1024 * 1024 {
                return Err("credential redaction input exceeds 4 MiB".into());
            }
        }
    }
    values.sort_unstable();
    values.dedup();
    Ok(values)
}
