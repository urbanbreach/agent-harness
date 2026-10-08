//! Per-account `CLAUDE_CONFIG_DIR` credentials.
use super::*;

const CLI_OAUTH_SCOPES: [&str; 6] = [
    "org:create_api_key",
    "user:profile",
    "user:inference",
    "user:sessions:claude_code",
    "user:mcp_servers",
    "user:file_upload",
];

/// One-shot, idempotent move of the legacy per-account directory to its canonical name.
pub fn resolve_accounts_directory(agent_dir: &Path) -> PathBuf {
    let legacy = agent_dir.join("claude-sdk-oauth-accounts");
    let target = agent_dir.join("anthropic-subscription-accounts");
    if !legacy.exists() {
        return target;
    }
    if target.exists() {
        let _ = std::fs::rename(
            &legacy,
            agent_dir.join(format!("claude-sdk-oauth-accounts.{}.bak", now_ms())),
        );
        return target;
    }
    if std::fs::rename(&legacy, &target).is_ok() {
        return target;
    }
    legacy
}

/// Writes the account's `.credentials.json` into its own `CLAUDE_CONFIG_DIR`.
pub fn write_config_dir_credential(
    agent_dir: &Path,
    slot: &AccountSlot,
    access: &str,
) -> Result<PathBuf, LaneError> {
    assert_valid_account_name(&slot.name).map_err(LaneError::Message)?;
    let directory = resolve_accounts_directory(agent_dir).join(&slot.name);
    let io = |e: std::io::Error| LaneError::Message(e.to_string());
    std::fs::create_dir_all(&directory).map_err(io)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&directory, std::fs::Permissions::from_mode(0o700)).map_err(io)?;
    }
    let body = serde_json::json!({"claudeAiOauth": {
        "accessToken": access,
        "refreshToken": slot.refresh,
        "expiresAt": slot.expires,
        "scopes": CLI_OAUTH_SCOPES,
    }});
    let path = directory.join(".credentials.json");
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create(true).truncate(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    use std::io::Write;
    options
        .open(&path)
        .and_then(|mut file| file.write_all(body.to_string().as_bytes()))
        .map_err(io)?;
    Ok(directory)
}
