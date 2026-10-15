use harness_core::auth::{
    anthropic::{now_epoch_ms, AnthropicOAuthClient, ANTHROPIC_TOKEN_URL},
    anthropic_subscription::AnthropicSubscriptionAccounts,
    codex::{AuthHttpClient, AuthHttpRequest, AuthHttpResponse, CodexOAuthError},
    CredentialStore,
};
use harness_providers::anthropic_subscription::{
    accounts::{list_accounts, select_account, AffinityOptions},
    auth_lane::EXPIRING_WITHIN_MS,
    AccountSource, SubscriptionAccountStore,
};
use serde_json::{json, Value};
use std::{path::Path, sync::Arc, time::Duration};

const USAGE_URL: &str = "https://api.anthropic.com/api/oauth/usage";
const MAX_RESPONSE_BYTES: usize = 64 * 1024;
const REQUEST_TIMEOUT: Duration = Duration::from_secs(8);

pub(super) async fn fetch(home: &Path) -> Result<Option<Value>, String> {
    let client = reqwest::Client::builder()
        .timeout(REQUEST_TIMEOUT)
        .connect_timeout(REQUEST_TIMEOUT)
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .map_err(|_| "subscription transport unavailable".to_owned())?;
    let oauth = AnthropicOAuthClient::new(Arc::new(RefreshHttp(client.clone())));
    let accounts = AnthropicSubscriptionAccounts::new(CredentialStore::new(home), Arc::new(oauth));
    let Some(pool) = accounts
        .read()
        .map_err(|_| "credential storage unavailable".to_owned())?
    else {
        return Ok(None);
    };
    // Stored slots only: explicit usage requests never import host or environment credentials.
    let slots = list_accounts(&pool, None);
    if slots.is_empty() {
        return Ok(None);
    }
    let mut slot = select_account(
        &slots,
        &AffinityOptions {
            pinned_account: pool.pinned.as_deref(),
            now: now_epoch_ms(),
            ..AffinityOptions::default()
        },
    )
    .map_err(|_| "subscription account unavailable".to_owned())?;
    if slot.source == AccountSource::Env {
        return Err("subscription account unavailable".into());
    }
    let is_expiring = |expires: i64| now_epoch_ms() >= expires.saturating_sub(EXPIRING_WITHIN_MS);
    if is_expiring(slot.expires) {
        // Native refresh holds the credential lock and rechecks expiry before rotating tokens.
        let pool = accounts
            .refresh_slot(&slot.name, &is_expiring)
            .await
            .map_err(|_| "subscription credential refresh failed".to_owned())?
            .ok_or_else(|| "subscription account unavailable".to_owned())?;
        slot = pool
            .accounts
            .into_iter()
            .find(|candidate| candidate.name == slot.name)
            .ok_or_else(|| "subscription account unavailable".to_owned())?;
    }
    if slot.access.is_empty() {
        return Err("subscription account unavailable".into());
    }
    let response = client
        .get(USAGE_URL)
        .bearer_auth(&slot.access)
        .header("anthropic-beta", "oauth-2025-04-20")
        .header("accept", "application/json")
        .send()
        .await
        .map_err(|_| "subscription usage request failed".to_owned())?;
    if !response.status().is_success() {
        return Err("subscription usage request rejected".into());
    }
    let body = bounded_body(response).await?;
    let value = serde_json::from_slice(&body)
        .map_err(|_| "subscription usage response invalid".to_owned())?;
    parse_usage(&value).map(Some)
}

async fn bounded_body(mut response: reqwest::Response) -> Result<Vec<u8>, String> {
    if response
        .content_length()
        .is_some_and(|length| length > MAX_RESPONSE_BYTES as u64)
    {
        return Err("subscription response too large".into());
    }
    let mut body = Vec::new();
    while let Some(chunk) = response
        .chunk()
        .await
        .map_err(|_| "subscription response unavailable".to_owned())?
    {
        if chunk.len() > MAX_RESPONSE_BYTES - body.len() {
            return Err("subscription response too large".into());
        }
        body.extend_from_slice(&chunk);
    }
    Ok(body)
}

// Keep the native OAuth exchange, with the same bounded transport as the usage request.
struct RefreshHttp(reqwest::Client);

#[async_trait::async_trait]
impl AuthHttpClient for RefreshHttp {
    async fn send(&self, request: AuthHttpRequest) -> Result<AuthHttpResponse, CodexOAuthError> {
        let failure = || CodexOAuthError::Http {
            message: "subscription credential refresh failed".into(),
        };
        if request.url != ANTHROPIC_TOKEN_URL || request.body.len() > MAX_RESPONSE_BYTES {
            return Err(failure());
        }
        let mut post = self.0.post(ANTHROPIC_TOKEN_URL).body(request.body);
        for (name, value) in request.headers {
            post = post.header(name, value);
        }
        let response = post.send().await.map_err(|_| failure())?;
        let status = response.status().as_u16();
        if !(200..300).contains(&status) {
            return Err(failure());
        }
        let bytes = bounded_body(response).await.map_err(|_| failure())?;
        Ok(AuthHttpResponse {
            status,
            body: String::from_utf8(bytes).map_err(|_| failure())?,
        })
    }
}

fn parse_usage(value: &Value) -> Result<Value, String> {
    let object = value
        .as_object()
        .ok_or_else(|| "subscription usage response invalid".to_owned())?;
    let windows: Vec<Value> = [
        ("five_hour", "5 hours"),
        ("seven_day", "7 days"),
        ("seven_day_opus", "7 days (Opus)"),
        ("seven_day_sonnet", "7 days (Sonnet)"),
        ("seven_day_oauth_apps", "7 days (OAuth apps)"),
        ("seven_day_cowork", "7 days (Cowork)"),
    ]
    .into_iter()
    .filter_map(|(key, label)| {
        let window = object.get(key)?.as_object()?;
        // Anthropic reports utilization in percentage points, not a 0..1 fraction.
        let used = window.get("utilization")?.as_f64()?;
        if !used.is_finite() || used < 0.0 {
            return None;
        }
        let resets_at = match window.get("resets_at") {
            None | Some(Value::Null) => None,
            Some(Value::String(time)) if humantime::parse_rfc3339(time).is_ok() => {
                Some(time.as_str())
            }
            _ => return None,
        };
        Some(json!({"label":label,"usedPercent":used,"resetsAt":resets_at}))
    })
    .collect();
    // This endpoint reports windows, not a subscription tier; never infer one from limits.
    Ok(json!({"plan":null,"windows":windows}))
}

#[cfg(test)]
mod tests {
    use super::parse_usage;
    use serde_json::json;

    #[test]
    fn parses_known_windows_without_scaling_or_leaking_unknown_fields() -> Result<(), String> {
        let parsed = parse_usage(&json!({
            "five_hour": {"utilization": 42.5, "resets_at": "2026-10-16T18:00:00Z"},
            "seven_day": {"utilization": 100, "resets_at": "2026-10-20T18:00:00+00:00"},
            "seven_day_opus": {"utilization": 0, "resets_at": null},
            "seven_day_sonnet": {"utilization": 12.25},
            "seven_day_oauth_apps": {"utilization": 125},
            "seven_day_cowork": {"utilization": 8},
            "unknown_window": {"utilization": 50},
            "account_id": "private-account",
            "extra_usage": {"is_enabled": true, "used_credits": 400}
        }))?;
        assert_eq!(
            parsed,
            json!({"plan":null,"windows":[
                {"label":"5 hours","usedPercent":42.5,"resetsAt":"2026-10-16T18:00:00Z"},
                {"label":"7 days","usedPercent":100.0,"resetsAt":"2026-10-20T18:00:00+00:00"},
                {"label":"7 days (Opus)","usedPercent":0.0,"resetsAt":null},
                {"label":"7 days (Sonnet)","usedPercent":12.25,"resetsAt":null},
                {"label":"7 days (OAuth apps)","usedPercent":125.0,"resetsAt":null},
                {"label":"7 days (Cowork)","usedPercent":8.0,"resetsAt":null}
            ]})
        );
        Ok(())
    }

    #[test]
    fn skips_invalid_optional_windows_and_rejects_non_object_responses() -> Result<(), String> {
        for window in [
            json!(null),
            json!([]),
            json!({}),
            json!({"utilization":"20"}),
            json!({"utilization":-1}),
            json!({"utilization":20,"resets_at":"tomorrow"}),
            json!({"utilization":20,"resets_at":42}),
        ] {
            assert_eq!(
                parse_usage(&json!({"five_hour":window}))?,
                json!({"plan":null,"windows":[]})
            );
        }
        assert_eq!(parse_usage(&json!({}))?, json!({"plan":null,"windows":[]}));
        for value in [json!(null), json!([]), json!("not a response")] {
            assert!(parse_usage(&value).is_err());
        }
        Ok(())
    }
}
