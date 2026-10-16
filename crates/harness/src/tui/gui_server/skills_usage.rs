use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{Instant, UNIX_EPOCH};

use harness_core::config::{BilledTokens, HarnessConfig, ModelCost, SkillsConfig};
use harness_core::event::{EventEnvelopeV1, EventV1};
use harness_core::provider_catalog::ProviderCatalog;
use harness_providers::CompletionUsage;
use serde_json::{json, Value};

#[path = "skills_usage/skills.rs"]
mod skills;
#[cfg(test)]
#[path = "skills_usage/tests.rs"]
mod tests;
#[path = "skills_usage/usage.rs"]
mod usage;
use skills::skills;
use usage::history;

// Keep grouped mutations from racing another request's discovery/preflight.
static SKILLS_LOCK: Mutex<()> = Mutex::new(());

pub(super) async fn dispatch(
    server: &super::Server,
    method: &str,
    params: &Value,
) -> Result<Value, String> {
    let cwd = params.get("cwd").map_or_else(
        || Ok(server.config_context.discovery.current_dir.clone()),
        |value| {
            value
                .as_str()
                .map(PathBuf::from)
                .ok_or_else(|| "invalid_params: cwd must be a string".to_owned())
        },
    )?;
    let settings = server.settings(&cwd).await?;
    if method == "usage.subscription" {
        return subscription(&settings, params).await;
    }
    let method = method.to_owned();
    let params = params.clone();
    tokio::task::spawn_blocking(move || match method.as_str() {
        "skills.list" | "skills.read" | "skills.setEnabled" | "skills.trash" => {
            let _lock = SKILLS_LOCK
                .lock()
                .map_err(|_| "skills mutation lock unavailable")?;
            let config = settings
                .config
                .as_ref()
                .map(|c| c.skills.clone())
                .unwrap_or_default();
            skills(
                &settings.workspace_root,
                &settings.data_dir,
                &config,
                &method,
                &params,
            )
        }
        "usage.history" => history(
            &settings.data_dir,
            &settings.session_dir,
            settings.config.as_ref(),
            &params,
        ),
        _ => Err(format!("method_not_found: {method}")),
    })
    .await
    .map_err(|error| error.to_string())?
}

async fn subscription(settings: &super::LiveSettings, params: &Value) -> Result<Value, String> {
    let provider = super::required(params, "provider")?;
    let id = settings
        .config
        .as_ref()
        .and_then(|config| config.providers.get(provider))
        .and_then(|configured| configured.credential_provider(provider))
        .or_else(|| harness_core::auth::ProviderId::parse(provider))
        .ok_or("invalid_params: invalid provider")?;
    let operation = async {
        if id == harness_core::auth::ProviderId::anthropic_subscription() {
            let config = settings
                .config
                .as_ref()
                .and_then(|config| config.providers.get(provider))
                .and_then(|provider| match provider {
                    harness_core::config::ProviderConfig::AnthropicSubscription(config) => {
                        Some(config)
                    }
                    _ => None,
                });
            super::subscription_anthropic::fetch(&settings.data_dir, config).await
        } else if id == harness_core::auth::ProviderId::codex() {
            super::subscription_codex::fetch(&settings.data_dir).await
        } else {
            Err("Subscription usage is not supported for this provider".into())
        }
    };
    let result = tokio::time::timeout(std::time::Duration::from_secs(20), operation).await;
    Ok(match result {
        Ok(Ok(Some(mut usage))) => {
            usage["provider"] = json!(provider);
            usage["status"] = json!("available");
            usage
        }
        Ok(Ok(None)) => {
            json!({"provider":provider,"status":"unconfigured","plan":null,"windows":[],"message":"No subscription OAuth credential is configured"})
        }
        Ok(Err(message)) => {
            json!({"provider":provider,"status":"error","plan":null,"windows":[],"message":message})
        }
        Err(_) => {
            json!({"provider":provider,"status":"error","plan":null,"windows":[],"message":"Subscription usage request timed out"})
        }
    })
}
