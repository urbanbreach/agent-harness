use crate::UnwrapOrAbort;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::Duration;

use serde::{Deserialize, Serialize};

const EMBEDDED_CATALOG: &str = include_str!("../../../configs/provider-catalog.generated.json");

const CACHE_TTL: Duration = Duration::from_secs(5 * 60);
const LOCK_STALE_TIMEOUT: Duration = Duration::from_secs(60);
const DEFAULT_MODELS_URL: &str = "https://models.dev/api.json";

const PROVIDER_PRIORITY: &[(&str, u8)] = &[
    ("openai", 0),
    ("codex", 0),
    ("github-copilot", 1),
    ("anthropic", 2),
    ("google", 3),
    ("openrouter", 4),
];

#[derive(Debug, Clone)]
pub struct ProviderCatalog {
    providers: BTreeMap<String, ProviderCatalogEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProviderCatalogEntry {
    pub id: String,
    pub name: String,
    pub base_url: String,
    pub api_key_env: Vec<String>,
    pub models: BTreeMap<String, ModelCatalogEntry>,
    pub auth_methods: Vec<CatalogAuthMethod>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelCatalogEntry {
    pub name: String,
    pub context_window: Option<u64>,
    pub supports_tool_calls: Option<bool>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CatalogAuthMethod {
    ApiKey,
    OAuth(OAuthFlow),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OAuthFlow {
    BrowserPkce,
    DeviceCode,
}

#[derive(Debug, thiserror::Error)]
pub enum CatalogError {
    #[error("failed to read catalog file {path}: {source}")]
    Read {
        path: String,
        source: std::io::Error,
    },
    #[error("failed to parse catalog: {source}")]
    Parse { source: serde_json::Error },
    #[error("failed to fetch catalog from {url}: {source}")]
    Fetch { url: String, source: reqwest::Error },
    #[error("failed to write catalog cache {path}: {source}")]
    CacheWrite {
        path: String,
        source: std::io::Error,
    },
}

impl ProviderCatalog {
    pub fn from_embedded() -> Result<Self, CatalogError> {
        parse_catalog(EMBEDDED_CATALOG)
    }

    pub fn from_path(path: &Path) -> Result<Self, CatalogError> {
        let content = std::fs::read_to_string(path).map_err(|source| CatalogError::Read {
            path: path.display().to_string(),
            source,
        })?;
        parse_catalog(&content)
    }

    pub fn providers(&self) -> Vec<&ProviderCatalogEntry> {
        self.sorted_by_priority()
    }

    pub fn provider(&self, id: &str) -> Option<&ProviderCatalogEntry> {
        self.providers.get(id)
    }

    pub fn sorted_by_priority(&self) -> Vec<&ProviderCatalogEntry> {
        let mut entries: Vec<_> = self.providers.values().collect();
        entries.sort_by(|a, b| {
            let a_priority = priority_of(&a.id);
            let b_priority = priority_of(&b.id);
            a_priority.cmp(&b_priority)
        });
        entries
    }

    pub fn fetch_from_url(url: &str) -> Result<Self, CatalogError> {
        let body = fetch_raw(url)?;
        parse_catalog(&body)
    }

    pub fn cached(path: &Path, url: Option<&str>) -> Result<Self, CatalogError> {
        if fetch_disabled() {
            return Self::from_embedded();
        }

        if let Ok(metadata) = std::fs::metadata(path) {
            if let Ok(modified) = metadata.modified() {
                if let Ok(elapsed) = modified.elapsed() {
                    if elapsed < CACHE_TTL {
                        return Self::from_path(path).or_else(|_| Self::from_embedded());
                    }
                }
            }
        }

        if let Some(url) = url {
            let lock = CacheLock::try_acquire(path);
            if !lock.acquired() {
                return Self::from_path(path).or_else(|_| Self::from_embedded());
            }
            match fetch_raw(url) {
                Ok(body) => {
                    let _ = write_cache_atomic(path, &body);
                    parse_catalog(&body).or_else(|_| Self::from_embedded())
                }
                Err(_) => Self::from_embedded(),
            }
        } else {
            Self::from_embedded()
        }
    }

    pub fn refresh_in_background(path: &Path, url: String) {
        let path = path.to_path_buf();
        std::thread::spawn(move || {
            if let Ok(body) = fetch_raw(&url) {
                let _ = write_cache_atomic(&path, &body);
            }
        });
    }

    pub fn from_env() -> Result<Self, CatalogError> {
        let url = models_url();
        let path = models_path();
        Self::cached(&path, url.as_deref())
    }
}

fn priority_of(id: &str) -> u8 {
    PROVIDER_PRIORITY
        .iter()
        .find(|(name, _)| *name == id)
        .map(|&(_, priority)| priority)
        .unwrap_or(255)
}

fn parse_catalog(json: &str) -> Result<ProviderCatalog, CatalogError> {
    let raw: RawCatalog =
        serde_json::from_str(json).map_err(|source| CatalogError::Parse { source })?;

    let mut providers = BTreeMap::new();
    for (id, raw_provider) in raw.provider {
        let auth_methods = auth_methods_for_provider(&id);
        let models = raw_provider
            .models
            .into_iter()
            .map(|(model_id, raw_model)| {
                let context_window = raw_model
                    .metadata
                    .as_ref()
                    .and_then(|m| m.context_window_tokens);
                let supports_tool_calls = raw_model
                    .metadata
                    .as_ref()
                    .and_then(|m| m.supports_tool_calls);
                (
                    model_id,
                    ModelCatalogEntry {
                        name: raw_model.name,
                        context_window,
                        supports_tool_calls,
                    },
                )
            })
            .collect();

        providers.insert(
            id.clone(),
            ProviderCatalogEntry {
                id,
                name: raw_provider.name,
                base_url: raw_provider.options.base_url,
                api_key_env: raw_provider.options.api_key_env,
                models,
                auth_methods,
            },
        );
    }

    Ok(ProviderCatalog { providers })
}

fn auth_methods_for_provider(id: &str) -> Vec<CatalogAuthMethod> {
    let mut methods = vec![CatalogAuthMethod::ApiKey];
    match id {
        "codex" => {
            methods.push(CatalogAuthMethod::OAuth(OAuthFlow::BrowserPkce));
            methods.push(CatalogAuthMethod::OAuth(OAuthFlow::DeviceCode));
        }
        "github-copilot" => {
            methods.push(CatalogAuthMethod::OAuth(OAuthFlow::DeviceCode));
        }
        _ => {}
    }
    methods
}

fn fetch_disabled() -> bool {
    matches!(
        std::env::var("HARNESS_DISABLE_MODELS_FETCH").as_deref(),
        Ok("1") | Ok("true") | Ok("TRUE")
    )
}

fn fetch_raw(url: &str) -> Result<String, CatalogError> {
    let client = reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(10))
        .build()
        .map_err(|source| CatalogError::Fetch {
            url: url.to_string(),
            source,
        })?;
    client
        .get(url)
        .send()
        .map_err(|source| CatalogError::Fetch {
            url: url.to_string(),
            source,
        })?
        .text()
        .map_err(|source| CatalogError::Fetch {
            url: url.to_string(),
            source,
        })
}

fn models_url() -> Option<String> {
    Some(std::env::var("HARNESS_MODELS_URL").unwrap_or_else(|_| DEFAULT_MODELS_URL.to_string()))
}

fn models_path() -> PathBuf {
    if let Ok(path) = std::env::var("HARNESS_MODELS_PATH") {
        return PathBuf::from(path);
    }
    if let Ok(home) = std::env::var("HOME") {
        return PathBuf::from(home)
            .join(".local")
            .join("share")
            .join("harness")
            .join("models-cache.json");
    }
    PathBuf::from(".harness").join("models-cache.json")
}

fn write_cache_atomic(path: &Path, content: &str) -> Result<(), std::io::Error> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let pid = std::process::id();
    let tmp = path.with_extension(format!("json.tmp.{pid}"));
    {
        use std::io::Write;
        let mut file = std::fs::File::create(&tmp)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = file.set_permissions(std::fs::Permissions::from_mode(0o600));
        }
        file.write_all(content.as_bytes())?;
        file.sync_all()?;
    }
    std::fs::rename(&tmp, path)?;
    Ok(())
}

struct CacheLock {
    lock_path: PathBuf,
    acquired: bool,
}

impl CacheLock {
    fn try_acquire(cache_path: &Path) -> Self {
        let lock_path = cache_path.with_extension("json.lock");
        if let Ok(metadata) = std::fs::metadata(&lock_path) {
            if let Ok(modified) = metadata.modified() {
                if let Ok(elapsed) = modified.elapsed() {
                    if elapsed > LOCK_STALE_TIMEOUT {
                        let _ = std::fs::remove_file(&lock_path);
                    }
                }
            }
        }
        let acquired = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&lock_path)
            .is_ok();
        Self {
            lock_path,
            acquired,
        }
    }

    fn acquired(&self) -> bool {
        self.acquired
    }
}

impl Drop for CacheLock {
    fn drop(&mut self) {
        if self.acquired {
            let _ = std::fs::remove_file(&self.lock_path);
        }
    }
}

#[derive(Debug, Deserialize)]
struct RawCatalog {
    provider: BTreeMap<String, RawProvider>,
}

#[derive(Debug, Deserialize)]
struct RawProvider {
    name: String,
    options: RawProviderOptions,
    #[serde(default)]
    models: BTreeMap<String, RawModel>,
}

#[derive(Debug, Deserialize)]
struct RawProviderOptions {
    #[serde(rename = "baseURL")]
    base_url: String,
    #[serde(rename = "apiKeyEnv")]
    api_key_env: Vec<String>,
}

#[derive(Debug, Deserialize)]
struct RawModel {
    name: String,
    #[serde(default)]
    metadata: Option<RawModelMetadata>,
}

#[derive(Debug, Deserialize)]
struct RawModelMetadata {
    #[serde(rename = "contextWindowTokens")]
    context_window_tokens: Option<u64>,
    #[serde(rename = "supportsToolCalls")]
    supports_tool_calls: Option<bool>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::UnwrapOrAbort;

    static ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    #[test]
    fn from_embedded_loads_all_providers() {
        // arrange
        let catalog = ProviderCatalog::from_embedded().unwrap_or_abort();

        // act
        let count = catalog.providers.len();

        // assert
        assert_eq!(count, 116, "expected 116 providers, found {count}");
    }

    #[test]
    fn provider_302ai_returns_valid_entry() {
        // arrange
        let catalog = ProviderCatalog::from_embedded().unwrap_or_abort();

        // act
        let provider = catalog.provider("302ai").unwrap_or_abort();

        // assert
        assert_eq!(provider.name, "302.AI");
        assert!(provider.base_url.contains("302.ai"));
        assert!(!provider.api_key_env.is_empty());
    }

    #[test]
    fn provider_nonexistent_returns_none() {
        // arrange
        let catalog = ProviderCatalog::from_embedded().unwrap_or_abort();

        // act
        let result = catalog.provider("nonexistent");

        // assert
        assert!(result.is_none());
    }

    #[test]
    fn sorted_by_priority_puts_openai_first() {
        // arrange
        let catalog = ProviderCatalog::from_embedded().unwrap_or_abort();

        // act
        let sorted = catalog.sorted_by_priority();

        // assert
        assert_eq!(
            sorted[0].id, "openai",
            "first should be openai, got {}",
            sorted[0].id
        );
        assert_eq!(
            sorted[1].id, "github-copilot",
            "second should be github-copilot, got {}",
            sorted[1].id
        );
    }

    #[test]
    fn provider_github_copilot_has_device_code_oauth() {
        // arrange
        let catalog = ProviderCatalog::from_embedded().unwrap_or_abort();

        // act
        let provider = catalog.provider("github-copilot").unwrap_or_abort();

        // assert
        assert!(
            provider.auth_methods.contains(&CatalogAuthMethod::ApiKey),
            "github-copilot should have ApiKey"
        );
        assert!(
            provider
                .auth_methods
                .contains(&CatalogAuthMethod::OAuth(OAuthFlow::DeviceCode)),
            "github-copilot should have DeviceCode OAuth"
        );
    }

    #[test]
    fn provider_302ai_has_only_api_key_auth_method() {
        // arrange
        let catalog = ProviderCatalog::from_embedded().unwrap_or_abort();

        // act
        let provider = catalog.provider("302ai").unwrap_or_abort();

        // assert
        assert_eq!(provider.auth_methods, vec![CatalogAuthMethod::ApiKey]);
    }

    #[test]
    fn cached_loads_from_fresh_cache_file() {
        // arrange
        let _guard = ENV_LOCK.lock().unwrap();
        let dir = tempfile::tempdir().unwrap_or_abort();
        let cache_path = dir.path().join("cache.json");
        std::fs::write(&cache_path, EMBEDDED_CATALOG).unwrap_or_abort();

        // act
        let catalog = ProviderCatalog::cached(&cache_path, None).unwrap_or_abort();

        // assert
        assert_eq!(catalog.providers.len(), 116);
    }

    #[test]
    fn cached_falls_back_to_embedded_on_missing_file() {
        // arrange
        let _guard = ENV_LOCK.lock().unwrap();
        let dir = tempfile::tempdir().unwrap_or_abort();
        let cache_path = dir.path().join("nonexistent.json");

        // act
        let catalog = ProviderCatalog::cached(&cache_path, None).unwrap_or_abort();

        // assert
        assert_eq!(catalog.providers.len(), 116);
    }

    #[test]
    fn cached_falls_back_to_embedded_on_fetch_failure() {
        // arrange
        let _guard = ENV_LOCK.lock().unwrap();
        let dir = tempfile::tempdir().unwrap_or_abort();
        let cache_path = dir.path().join("cache.json");
        let invalid_url = "http://127.0.0.1:1/nonexistent";

        // act
        let catalog = ProviderCatalog::cached(&cache_path, Some(invalid_url));

        // assert
        let catalog = catalog.unwrap_or_abort();
        assert_eq!(catalog.providers.len(), 116);
    }

    #[test]
    fn cached_disabled_env_var_uses_embedded() {
        // arrange
        let _guard = ENV_LOCK.lock().unwrap();
        let dir = tempfile::tempdir().unwrap_or_abort();
        let cache_path = dir.path().join("cache.json");
        std::env::set_var("HARNESS_DISABLE_MODELS_FETCH", "1");

        // act
        let catalog = ProviderCatalog::cached(&cache_path, Some("http://invalid"));

        // assert
        std::env::remove_var("HARNESS_DISABLE_MODELS_FETCH");
        let catalog = catalog.unwrap_or_abort();
        assert_eq!(catalog.providers.len(), 116);
    }

    #[test]
    fn write_cache_atomic_creates_file() {
        // arrange
        let dir = tempfile::tempdir().unwrap_or_abort();
        let cache_path = dir.path().join("cache.json");
        let content = r#"{"provider":{}}"#;

        // act
        write_cache_atomic(&cache_path, content).unwrap_or_abort();

        // assert
        let written = std::fs::read_to_string(&cache_path).unwrap_or_abort();
        assert_eq!(written, content);
    }

    #[test]
    fn write_cache_atomic_file_permissions_0600() {
        // arrange
        let dir = tempfile::tempdir().unwrap_or_abort();
        let cache_path = dir.path().join("cache.json");

        // act
        write_cache_atomic(&cache_path, r#"{"provider":{}}"#).unwrap_or_abort();

        // assert
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let perms = std::fs::metadata(&cache_path)
                .unwrap_or_abort()
                .permissions();
            assert_eq!(perms.mode() & 0o777, 0o600);
        }
    }

    #[test]
    fn models_url_reads_env_var() {
        // arrange
        let _guard = ENV_LOCK.lock().unwrap();
        std::env::set_var("HARNESS_MODELS_URL", "https://custom.example.com/api.json");

        // act
        let url = models_url();

        // assert
        std::env::remove_var("HARNESS_MODELS_URL");
        assert_eq!(url.as_deref(), Some("https://custom.example.com/api.json"));
    }

    #[test]
    fn models_path_reads_env_var() {
        // arrange
        let _guard = ENV_LOCK.lock().unwrap();
        std::env::set_var("HARNESS_MODELS_PATH", "/tmp/custom-cache.json");

        // act
        let path = models_path();

        // assert
        std::env::remove_var("HARNESS_MODELS_PATH");
        assert_eq!(path, std::path::PathBuf::from("/tmp/custom-cache.json"));
    }

    #[test]
    fn refresh_in_background_does_not_panic() {
        // arrange
        let dir = tempfile::tempdir().unwrap_or_abort();
        let cache_path = dir.path().join("cache.json");
        let invalid_url = "http://127.0.0.1:1/nonexistent".to_string();

        // act
        let result = std::panic::catch_unwind(|| {
            ProviderCatalog::refresh_in_background(&cache_path, invalid_url);
        });
        std::thread::sleep(std::time::Duration::from_millis(100));

        // assert
        assert!(result.is_ok(), "refresh_in_background should not panic");
    }

    #[test]
    fn fetch_from_url_succeeds_with_mock_server() {
        // arrange
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap_or_abort();
        let addr = listener.local_addr().unwrap_or_abort();
        let url = format!("http://{addr}/api.json");
        let body = EMBEDDED_CATALOG.to_string();
        std::thread::spawn(move || {
            if let Ok((mut stream, _)) = listener.accept() {
                use std::io::{Read, Write};
                let mut buf = [0u8; 1024];
                let _ = stream.read(&mut buf);
                let response = format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                    body.len(),
                    body
                );
                let _ = stream.write_all(response.as_bytes());
            }
        });

        // act
        let catalog = ProviderCatalog::fetch_from_url(&url);

        // assert
        let catalog = catalog.unwrap_or_abort();
        assert_eq!(catalog.providers.len(), 116);
    }

    #[test]
    fn cached_writes_fetched_data_to_cache_file() {
        // arrange
        let _guard = ENV_LOCK.lock().unwrap();
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap_or_abort();
        let addr = listener.local_addr().unwrap_or_abort();
        let url = format!("http://{addr}/api.json");
        let body = EMBEDDED_CATALOG.to_string();
        std::thread::spawn(move || {
            if let Ok((mut stream, _)) = listener.accept() {
                use std::io::{Read, Write};
                let mut buf = [0u8; 1024];
                let _ = stream.read(&mut buf);
                let response = format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                    body.len(),
                    body
                );
                let _ = stream.write_all(response.as_bytes());
            }
        });
        std::thread::sleep(std::time::Duration::from_millis(50));
        let dir = tempfile::tempdir().unwrap_or_abort();
        let cache_path = dir.path().join("cache.json");

        // act
        let catalog = ProviderCatalog::cached(&cache_path, Some(&url)).unwrap_or_abort();

        // assert
        assert_eq!(catalog.providers.len(), 116);
        assert!(cache_path.exists(), "cache file should be written");
        let cached = ProviderCatalog::from_path(&cache_path).unwrap_or_abort();
        assert_eq!(cached.providers.len(), 116);
    }
}
