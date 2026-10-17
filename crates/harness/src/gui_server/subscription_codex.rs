use harness_core::auth::{
    codex::CodexOAuthClient, CredentialStore, ProviderCredentialManager, ProviderId,
    ReqwestAuthHttpClient, StoredCredentialKind,
};
use serde_json::{json, Value};
use std::{
    path::Path,
    sync::Arc,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

const TIMEOUT: Duration = Duration::from_secs(8);
const MAX_RESPONSE_BYTES: usize = 1024 * 1024;
const ERROR: &str = "Codex subscription usage is unavailable";

pub(super) async fn fetch(home: &Path) -> Result<Option<Value>, String> {
    let store = CredentialStore::new(home);
    let provider = ProviderId::codex();
    let stored = store.load(&provider).map_err(|_| ERROR.to_owned())?;
    if !stored.is_some_and(|credential| credential.kind == StoredCredentialKind::Oauth) {
        return Ok(None);
    }
    let http = ReqwestAuthHttpClient::new().map_err(|_| ERROR.to_owned())?;
    let manager = ProviderCredentialManager::new(store, provider, Vec::new(), "", |_| None)
        .with_refresher(Arc::new(CodexOAuthClient::new(Arc::new(http))));
    let credential = tokio::time::timeout(
        TIMEOUT,
        manager.refresh_oauth_if_near_expiry(Duration::from_secs(60)),
    )
    .await
    .map_err(|_| ERROR.to_owned())?
    .map_err(|_| ERROR.to_owned())?;
    let account_id = credential
        .account_id
        .as_deref()
        .filter(|id| !id.trim().is_empty())
        .ok_or_else(|| ERROR.to_owned())?;
    let client = reqwest::Client::builder()
        .timeout(TIMEOUT)
        .connect_timeout(TIMEOUT)
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .map_err(|_| ERROR.to_owned())?;
    let mut response = client
        .get("https://chatgpt.com/backend-api/wham/usage")
        .bearer_auth(&credential.token)
        .header("chatgpt-account-id", account_id)
        .header(reqwest::header::ACCEPT, "application/json")
        .send()
        .await
        .map_err(|_| ERROR.to_owned())?;
    if !response.status().is_success()
        || response
            .content_length()
            .is_some_and(|size| size > MAX_RESPONSE_BYTES as u64)
    {
        return Err(ERROR.to_owned());
    }
    let mut bytes = Vec::new();
    while let Some(chunk) = response.chunk().await.map_err(|_| ERROR.to_owned())? {
        if chunk.len() > MAX_RESPONSE_BYTES - bytes.len() {
            return Err(ERROR.to_owned());
        }
        bytes.extend_from_slice(&chunk);
    }
    let value = serde_json::from_slice(&bytes).map_err(|_| ERROR.to_owned())?;
    parse_usage(&value, SystemTime::now()).map(Some)
}

fn parse_usage(value: &Value, now: SystemTime) -> Result<Value, String> {
    if !value.is_object() {
        return Err(ERROR.to_owned());
    }
    let plan = value["plan_type"].as_str().and_then(|plan| {
        match plan.trim().to_ascii_lowercase().as_str() {
            "free" | "chatgpt_free" => Some("Free"),
            "go" | "chatgpt_go" => Some("Go"),
            "plus" | "chatgpt_plus" => Some("Plus"),
            "pro" | "chatgpt_pro" => Some("Pro"),
            "team" | "chatgpt_team" => Some("Team"),
            "business" | "chatgpt_business" => Some("Business"),
            "enterprise" | "chatgpt_enterprise" => Some("Enterprise"),
            "edu" | "education" | "chatgpt_edu" => Some("Edu"),
            _ => None,
        }
    });
    let mut windows = Vec::new();
    append_windows(&value["rate_limit"], None, now, &mut windows);
    if let Some(additional) = value["additional_rate_limits"].as_array() {
        for limit in additional {
            let name = limit["limit_name"]
                .as_str()
                .or_else(|| limit["metered_feature"].as_str())
                .filter(|name| {
                    !name.trim().is_empty()
                        && name.len() <= 80
                        && !name.chars().any(char::is_control)
                });
            append_windows(&limit["rate_limit"], name, now, &mut windows);
        }
    }
    Ok(json!({ "plan": plan, "windows": windows }))
}

fn append_windows(limit: &Value, name: Option<&str>, now: SystemTime, windows: &mut Vec<Value>) {
    for (key, fallback) in [
        ("primary_window", "Primary"),
        ("secondary_window", "Secondary"),
    ] {
        let window = &limit[key];
        let Some(used) = window["used_percent"]
            .as_f64()
            .filter(|used| used.is_finite())
        else {
            continue;
        };
        let label = match window["limit_window_seconds"].as_u64() {
            Some(604_800) => "Weekly".to_owned(),
            Some(86_400) => "Daily".to_owned(),
            Some(seconds) if seconds > 0 && seconds % 3_600 == 0 => {
                format!("{}h", seconds / 3_600)
            }
            Some(seconds) if seconds > 0 && seconds % 60 == 0 => {
                format!("{}m", seconds / 60)
            }
            Some(seconds) if seconds > 0 => format!("{seconds}s"),
            _ => fallback.to_owned(),
        };
        let label = match name {
            Some(name) => format!("{} · {label}", name.trim()),
            None => label,
        };
        let reset = window["reset_at"]
            .as_u64()
            .and_then(|seconds| UNIX_EPOCH.checked_add(Duration::from_secs(seconds)))
            .or_else(|| {
                window["reset_after_seconds"]
                    .as_u64()
                    .and_then(|seconds| now.checked_add(Duration::from_secs(seconds)))
            })
            .filter(|time| {
                time.duration_since(UNIX_EPOCH)
                    .is_ok_and(|duration| duration.as_secs() <= 253_402_300_799)
            })
            .map(|time| humantime::format_rfc3339_seconds(time).to_string());
        windows.push(json!({
            "label": label,
            "usedPercent": used,
            "resetsAt": reset,
        }));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_subscription_windows_and_additional_model_limits() -> Result<(), String> {
        let result = parse_usage(
            &json!({
                "plan_type": "chatgpt_plus",
                "rate_limit": {
                    "primary_window": {
                        "used_percent": 37.5,
                        "limit_window_seconds": 18000,
                        "reset_at": 1760000000,
                        "reset_after_seconds": 1
                    },
                    "secondary_window": {
                        "used_percent": 12,
                        "limit_window_seconds": 604800,
                        "reset_after_seconds": 3600
                    }
                },
                "additional_rate_limits": [
                    {"limit_name": "Codex Mini", "rate_limit": {
                        "primary_window": {"used_percent": 0, "limit_window_seconds": 86400}
                    }},
                    {"limit_name": "Missing optional data"}
                ]
            }),
            UNIX_EPOCH + Duration::from_secs(1760000000),
        )?;
        assert_eq!(
            result,
            json!({
                "plan": "Plus",
                "windows": [
                    {"label": "5h", "usedPercent": 37.5, "resetsAt": "2025-10-09T08:53:20Z"},
                    {"label": "Weekly", "usedPercent": 12.0, "resetsAt": "2025-10-09T09:53:20Z"},
                    {"label": "Codex Mini · Daily", "usedPercent": 0.0, "resetsAt": null}
                ]
            })
        );
        Ok(())
    }

    #[test]
    fn tolerates_partial_unknown_and_invalid_optional_fields() -> Result<(), String> {
        for (input, expected) in [
            (json!({}), json!({"plan": null, "windows": []})),
            (
                json!({"plan_type": " PRO ", "rate_limit": {
                    "primary_window": {"used_percent": "unknown"},
                    "secondary_window": {"used_percent": 100, "reset_at": u64::MAX}
                }}),
                json!({"plan": "Pro", "windows": [
                    {"label": "Secondary", "usedPercent": 100.0, "resetsAt": null}
                ]}),
            ),
            (
                json!({"plan_type": "future_plan", "additional_rate_limits": null}),
                json!({"plan": null, "windows": []}),
            ),
        ] {
            assert_eq!(parse_usage(&input, UNIX_EPOCH)?, expected);
        }
        assert!(parse_usage(&Value::Null, UNIX_EPOCH).is_err());
        Ok(())
    }
}
