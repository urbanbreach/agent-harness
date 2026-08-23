// allow: SIZE_OK — provider catalog (embedded JSON + reference merge + model variant + capability metadata)
use crate::config::{ModelLimitError, ModelLimitProvenance, ResolvedModelLimits};
use crate::UnwrapOrAbort;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::Duration;

use serde::{Deserialize, Serialize};

mod duplicate_checked_json;

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

pub fn duplicate_checked_json_value(json: &str) -> Result<serde_json::Value, serde_json::Error> {
    duplicate_checked_json::parse(json)
}

#[derive(Debug, Clone)]
pub struct ProviderCatalog {
    providers: BTreeMap<String, ProviderCatalogEntry>,
    diagnostics: Vec<CatalogDiagnostic>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CatalogDiagnostic {
    pub provider: String,
    pub model: String,
    pub message: String,
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
    pub limits: ResolvedModelLimits,
    pub supports_tool_calls: Option<bool>,
    pub supports_reasoning: bool,
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
    #[error("catalog contains no provider with a usable model")]
    NoUsableModels,
}

#[derive(Debug, thiserror::Error)]
pub enum CatalogModelError {
    #[error("catalog model `{provider}:{model}` was not found")]
    NotFound { provider: String, model: String },
    #[error(transparent)]
    InvalidLimits(#[from] ModelLimitError),
}

#[derive(Debug, Clone, Copy)]
enum CatalogLimitSource {
    Generated,
    Discovered,
}

impl ProviderCatalog {
    pub fn from_embedded() -> Result<Self, CatalogError> {
        parse_catalog(
            EMBEDDED_CATALOG,
            CatalogLimitSource::Generated,
            "https://models.dev/api.json",
        )
    }

    pub fn from_path(path: &Path) -> Result<Self, CatalogError> {
        let content = std::fs::read_to_string(path).map_err(|source| CatalogError::Read {
            path: path.display().to_string(),
            source,
        })?;
        parse_catalog(
            &content,
            CatalogLimitSource::Discovered,
            &format!("file://{}", path.display()),
        )
    }

    pub fn providers(&self) -> Vec<&ProviderCatalogEntry> {
        self.sorted_by_priority()
    }

    pub fn provider(&self, id: &str) -> Option<&ProviderCatalogEntry> {
        self.providers.get(id)
    }

    pub fn diagnostics(&self) -> &[CatalogDiagnostic] {
        &self.diagnostics
    }

    pub fn validated_model(
        &self,
        provider: &str,
        model: &str,
    ) -> Result<&ModelCatalogEntry, CatalogModelError> {
        let entry = self
            .provider(provider)
            .and_then(|provider_entry| provider_entry.models.get(model))
            .ok_or_else(|| CatalogModelError::NotFound {
                provider: provider.to_string(),
                model: model.to_string(),
            })?;
        entry.limits.validate(&format!("{provider}:{model}"))?;
        Ok(entry)
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
        parse_catalog(&body, CatalogLimitSource::Discovered, url)
    }

    pub fn cached(path: &Path, url: Option<&str>) -> Result<Self, CatalogError> {
        if fetch_disabled() {
            return Self::from_embedded();
        }

        if let Ok(catalog) = Self::from_path(path) {
            let fresh = std::fs::metadata(path)
                .and_then(|metadata| metadata.modified())
                .and_then(|modified| modified.elapsed().map_err(std::io::Error::other))
                .is_ok_and(|elapsed| elapsed < CACHE_TTL);
            if fresh {
                return Ok(catalog);
            }
            if let Some(url) = url {
                Self::refresh_in_background(path, url.to_string());
            }
            return Ok(catalog);
        }

        if let Some(url) = url {
            let lock = CacheLock::try_acquire(path);
            if !lock.acquired() {
                return Self::from_path(path).or_else(|_| Self::from_embedded());
            }
            match fetch_raw(url) {
                Ok(body) => match replace_cache_with_valid_catalog(path, &body, url) {
                    Ok(catalog) => Ok(catalog),
                    Err(_) => Self::from_embedded(),
                },
                Err(_) => Self::from_embedded(),
            }
        } else {
            Self::from_embedded()
        }
    }

    pub fn refresh_in_background(path: &Path, url: String) {
        let path = path.to_path_buf();
        std::thread::spawn(move || {
            let lock = CacheLock::try_acquire(&path);
            if !lock.acquired() {
                return;
            }
            if let Ok(body) = fetch_raw(&url) {
                if parse_catalog(&body, CatalogLimitSource::Discovered, &url).is_ok() {
                    let _ = write_cache_atomic(&path, &body);
                }
            }
        });
    }

    pub fn from_env() -> Result<Self, CatalogError> {
        let url = models_url();
        let path = models_path();
        Self::cached(&path, url.as_deref())
    }
}

fn replace_cache_with_valid_catalog(
    path: &Path,
    body: &str,
    source_reference: &str,
) -> Result<ProviderCatalog, CatalogError> {
    let catalog = parse_catalog(body, CatalogLimitSource::Discovered, source_reference)?;
    write_cache_atomic(path, body).map_err(|source| CatalogError::CacheWrite {
        path: path.display().to_string(),
        source,
    })?;
    Ok(catalog)
}

fn priority_of(id: &str) -> u8 {
    PROVIDER_PRIORITY
        .iter()
        .find(|(name, _)| *name == id)
        .map(|&(_, priority)| priority)
        .unwrap_or(255)
}

fn parse_catalog(
    json: &str,
    source_kind: CatalogLimitSource,
    source_reference: &str,
) -> Result<ProviderCatalog, CatalogError> {
    let value =
        duplicate_checked_json_value(json).map_err(|source| CatalogError::Parse { source })?;
    let raw: RawCatalog =
        serde_json::from_value(value).map_err(|source| CatalogError::Parse { source })?;
    let raw_providers = match raw {
        RawCatalog::Generated { provider } | RawCatalog::ModelsDev(provider) => provider,
    };

    let mut providers = BTreeMap::new();
    let mut diagnostics = Vec::new();
    for (id, raw_provider) in raw_providers {
        let auth_methods = auth_methods_for_provider(&id);
        let models = raw_provider
            .models
            .into_iter()
            .filter_map(|(model_id, raw_model)| {
                let context_window = raw_model
                    .metadata
                    .as_ref()
                    .and_then(|metadata| metadata.context_window_tokens)
                    .or_else(|| raw_model.limit.as_ref().and_then(|limit| limit.context));
                let max_input = raw_model.limit.as_ref().and_then(|limit| limit.input);
                let max_output = raw_model.limit.as_ref().and_then(|limit| limit.output);
                let supports_tool_calls = raw_model
                    .metadata
                    .as_ref()
                    .and_then(|metadata| metadata.supports_tool_calls)
                    .or(Some(raw_model.tool_call));
                let verified_at = raw_model
                    .last_updated
                    .clone()
                    .or_else(|| models_dev_option(&raw_model.options, &["model", "lastUpdated"]));
                let source = sanitize_catalog_source_origin(
                    &models_dev_option(&raw_model.options, &["source"])
                        .unwrap_or_else(|| source_reference.to_string()),
                );
                let provenance = match source_kind {
                    CatalogLimitSource::Generated => {
                        ModelLimitProvenance::generated(source, verified_at)
                    }
                    CatalogLimitSource::Discovered => {
                        ModelLimitProvenance::discovered(source, verified_at)
                    }
                };
                let limits = match checked_catalog_limits(
                    context_window,
                    max_input,
                    max_output,
                    provenance,
                    &format!("{id}:{model_id}"),
                ) {
                    Ok(limits) => limits,
                    Err(error) => {
                        diagnostics.push(CatalogDiagnostic {
                            provider: id.clone(),
                            model: model_id,
                            message: error,
                        });
                        return None;
                    }
                };
                Some((
                    model_id.clone(),
                    ModelCatalogEntry {
                        name: raw_model.name.unwrap_or(model_id),
                        limits,
                        supports_tool_calls,
                        supports_reasoning: raw_model.reasoning,
                    },
                ))
            })
            .collect();

        providers.insert(
            id.clone(),
            ProviderCatalogEntry {
                id: id.clone(),
                name: raw_provider.name.unwrap_or_else(|| id.clone()),
                base_url: raw_provider
                    .options
                    .as_ref()
                    .map(|options| options.base_url.clone())
                    .or(raw_provider.api)
                    .unwrap_or_default(),
                api_key_env: raw_provider
                    .options
                    .map(|options| options.api_key_env)
                    .filter(|env| !env.is_empty())
                    .unwrap_or(raw_provider.env),
                models,
                auth_methods,
            },
        );
    }

    if !providers
        .values()
        .any(|provider| !provider.models.is_empty())
    {
        return Err(CatalogError::NoUsableModels);
    }

    Ok(ProviderCatalog {
        providers,
        diagnostics,
    })
}

fn checked_limit(value: Option<u64>) -> Result<Option<u32>, std::num::TryFromIntError> {
    value.map(u32::try_from).transpose()
}

pub fn checked_catalog_limits(
    context_window: Option<u64>,
    max_input: Option<u64>,
    max_output: Option<u64>,
    mut provenance: ModelLimitProvenance,
    identity: &str,
) -> Result<ResolvedModelLimits, String> {
    provenance.verified_at = provenance
        .verified_at
        .as_deref()
        .and_then(sanitize_catalog_verified_at);
    let context_window = checked_limit(context_window)
        .map_err(|_| format!("model `{identity}` context exceeds the supported u32 token range"))?;
    let max_input = checked_limit(max_input)
        .map_err(|_| format!("model `{identity}` input exceeds the supported u32 token range"))?;
    let max_output = checked_limit(max_output)
        .map_err(|_| format!("model `{identity}` output exceeds the supported u32 token range"))?;
    let limits =
        ResolvedModelLimits::from_values(context_window, max_input, max_output, provenance);
    limits
        .validate(identity)
        .map_err(|error| error.to_string())?;
    if !limits.is_selectable_known() {
        return Err(format!(
            "model `{identity}` does not define selectable context and output limits"
        ));
    }
    Ok(limits)
}

pub fn sanitize_catalog_verified_at(value: &str) -> Option<String> {
    let bytes = value.as_bytes();
    let digits = |range: std::ops::Range<usize>| {
        bytes
            .get(range)
            .is_some_and(|segment| segment.iter().all(u8::is_ascii_digit))
    };
    let year_valid = bytes.len() >= 4 && digits(0..4);
    let month_valid = bytes.len() >= 7
        && bytes.get(4) == Some(&b'-')
        && digits(5..7)
        && (1..=12).contains(&((bytes[5] - b'0') * 10 + bytes[6] - b'0'));
    let day_valid = bytes.len() == 10
        && bytes.get(7) == Some(&b'-')
        && digits(8..10)
        && (1..=31).contains(&((bytes[8] - b'0') * 10 + bytes[9] - b'0'));
    if (bytes.len() == 4 && year_valid)
        || (bytes.len() == 7 && year_valid && month_valid)
        || (year_valid && month_valid && day_valid)
    {
        Some(value.to_string())
    } else {
        None
    }
}

pub fn sanitize_catalog_source_origin(source: &str) -> String {
    let trimmed = source.trim();
    if trimmed.starts_with("file:") {
        return "file://<redacted>".to_string();
    }
    let Ok(mut url) = reqwest::Url::parse(trimmed) else {
        return "<redacted>".to_string();
    };
    let _ = url.set_username("");
    let _ = url.set_password(None);
    url.set_query(None);
    url.set_fragment(None);
    url.to_string()
}

fn models_dev_option(
    options: &BTreeMap<String, serde_json::Value>,
    path: &[&str],
) -> Option<String> {
    let mut value = options.get("modelsDev")?;
    for segment in path {
        value = value.get(*segment)?;
    }
    value.as_str().map(str::to_string)
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
#[serde(untagged)]
enum RawCatalog {
    Generated {
        provider: BTreeMap<String, RawProvider>,
    },
    ModelsDev(BTreeMap<String, RawProvider>),
}

#[derive(Debug, Deserialize)]
struct RawProvider {
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    options: Option<RawProviderOptions>,
    #[serde(default)]
    api: Option<String>,
    #[serde(default)]
    env: Vec<String>,
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
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    metadata: Option<RawModelMetadata>,
    #[serde(default)]
    reasoning: bool,
    #[serde(default)]
    tool_call: bool,
    #[serde(default)]
    limit: Option<RawModelLimit>,
    #[serde(default)]
    options: BTreeMap<String, serde_json::Value>,
    #[serde(default, alias = "lastUpdated")]
    last_updated: Option<String>,
}

#[derive(Debug, Deserialize)]
struct RawModelMetadata {
    #[serde(rename = "contextWindowTokens")]
    context_window_tokens: Option<u64>,
    #[serde(rename = "supportsToolCalls")]
    supports_tool_calls: Option<bool>,
}

#[derive(Debug, Deserialize)]
struct RawModelLimit {
    #[serde(default)]
    context: Option<u64>,
    #[serde(default)]
    input: Option<u64>,
    #[serde(default)]
    output: Option<u64>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{ModelLimitError, ModelLimitProvenance, ResolvedModelLimits};
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
    fn parses_models_dev_direct_provider_shape() {
        // arrange
        let body = r#"{
          "openai": {
            "name": "OpenAI",
            "env": ["OPENAI_API_KEY"],
            "models": {
              "gpt-5.6": {
                "id": "gpt-5.6",
                "name": "GPT-5.6",
                "reasoning": true,
                "tool_call": true,
                "limit": { "context": 1050000, "output": 128000 }
              }
            }
          }
        }"#;

        // act
        let catalog = parse_catalog(
            body,
            CatalogLimitSource::Discovered,
            "https://example.test/models",
        )
        .unwrap_or_abort();

        // assert
        let model = &catalog.provider("openai").unwrap_or_abort().models["gpt-5.6"];
        assert_eq!(model.name, "GPT-5.6");
        assert_eq!(model.limits.context_window_tokens(), Some(1_050_000));
        assert_eq!(model.supports_tool_calls, Some(true));
        assert!(model.supports_reasoning);
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
    fn cache_replacement_validates_the_body_before_atomic_write() {
        // arrange
        let dir = tempfile::tempdir().unwrap_or_abort();
        let cache_path = dir.path().join("cache.json");
        let original =
            r#"{"provider":{"safe":{"models":{"safe":{"limit":{"context":8192,"output":1024}}}}}}"#;
        std::fs::write(&cache_path, original).unwrap_or_abort();

        // act
        let result = replace_cache_with_valid_catalog(
            &cache_path,
            r#"{"provider":{"bad":{"models":{"bad":{"limit":{"context":0,"output":1}}}}}}"#,
            "https://example.test/models",
        );

        // assert
        assert!(result.is_err());
        assert_eq!(
            std::fs::read_to_string(&cache_path).unwrap_or_abort(),
            original
        );
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
