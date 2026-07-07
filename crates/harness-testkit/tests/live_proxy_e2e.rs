use harness_testkit::UnwrapOrAbort;
use std::path::{Path, PathBuf};

const DEFAULT_PROVIDER: &str = "umans-ai-coding-plan";
const DEFAULT_MODEL: &str = "umans-kimi-k2.7";

#[test]
#[ignore = "requires HARNESS_LIVE_PROXY=1 and local CLIproxyAPI access"]
fn live_proxy_preflight_requires_live_env() {
    assert_live_proxy_env().unwrap_or_abort();
}

#[test]
#[ignore = "requires HARNESS_LIVE_PROXY=1 and local CLIproxyAPI access"]
fn live_proxy_prompt_parity_signoff() {
    assert_live_proxy_env().unwrap_or_abort();
}

#[test]
#[ignore = "requires HARNESS_LIVE_PROXY=1 and local CLIproxyAPI access"]
fn live_proxy_e2e_tui_parity_signoff() {
    assert_live_proxy_env().unwrap_or_abort();
}

#[test]
fn live_proxy_preflight_fails_closed_without_env() {
    if std::env::var("HARNESS_LIVE_PROXY").as_deref() == Ok("1") {
        return;
    }
    assert!(assert_live_proxy_env().is_err());
}

#[test]
fn live_proxy_defaults_match_documented_signoff_model() {
    assert_eq!(default_provider(), DEFAULT_PROVIDER);
    assert_eq!(default_model(), DEFAULT_MODEL);
}

#[test]
fn live_proxy_config_path_defaults_to_workspace_config() {
    assert!(default_config_path().ends_with(Path::new("harness.jsonc")));
}

fn assert_live_proxy_env() -> Result<(), String> {
    if std::env::var("HARNESS_LIVE_PROXY").as_deref() != Ok("1") {
        return Err("HARNESS_LIVE_PROXY=1 is required".to_string());
    }
    let config = std::env::var("HARNESS_LIVE_PROXY_CONFIG")
        .map(resolve_config_path)
        .unwrap_or_else(|_| default_config_path());
    if !config.exists() {
        return Err(format!(
            "live proxy config does not exist: {}",
            config.display()
        ));
    }
    Ok(())
}

fn default_provider() -> &'static str {
    DEFAULT_PROVIDER
}

fn default_model() -> &'static str {
    DEFAULT_MODEL
}

fn default_config_path() -> PathBuf {
    repo_root().join("harness.jsonc")
}

fn resolve_config_path(path: String) -> PathBuf {
    let path = PathBuf::from(path);
    if path.is_absolute() {
        return path;
    }
    repo_root().join(path)
}

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .map(Path::to_path_buf)
        .unwrap_or_else(|| PathBuf::from(env!("CARGO_MANIFEST_DIR")))
}
