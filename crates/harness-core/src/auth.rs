use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::fs::{self, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use async_trait::async_trait;
use harness_providers::{
    ProviderBearerToken, ProviderCredentialError, ProviderCredentialKind, ProviderCredentialSource,
    ProviderErrorCategory,
};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use thiserror::Error;
use tokio::sync::Mutex;

pub mod codex;
pub mod copilot;
pub mod plugin;

const CREDENTIAL_STORE_VERSION: u32 = 1;
const CREDENTIALS_DIR_NAME: &str = "credentials";

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ProviderId(Arc<str>);

impl ProviderId {
    pub fn codex() -> Self {
        Self(Arc::from("codex"))
    }

    pub fn github_copilot() -> Self {
        Self(Arc::from("github-copilot"))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    pub fn parse(value: &str) -> Option<Self> {
        if value.chars().any(char::is_control) {
            return None;
        }
        let trimmed = value.trim();
        if trimmed.is_empty() {
            return None;
        }
        if trimmed.contains('/') || trimmed.contains('\\') {
            return None;
        }
        if trimmed == ".." || trimmed.contains("..") {
            return None;
        }
        Some(Self(Arc::from(trimmed)))
    }
}

impl fmt::Display for ProviderId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

impl Serialize for ProviderId {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_str(self.as_str())
    }
}

impl<'de> Deserialize<'de> for ProviderId {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        Self::parse(&value).ok_or_else(|| {
            serde::de::Error::custom(
                "invalid provider id: must be non-empty and must not contain path traversal characters, slashes, null bytes, newlines, or terminal control characters",
            )
        })
    }
}

impl schemars::JsonSchema for ProviderId {
    fn schema_name() -> std::borrow::Cow<'static, str> {
        "ProviderId".into()
    }

    fn schema_id() -> std::borrow::Cow<'static, str> {
        concat!(module_path!(), "::ProviderId").into()
    }

    fn json_schema(_: &mut schemars::SchemaGenerator) -> schemars::Schema {
        schemars::json_schema!({
            "type": "string"
        })
    }
}

pub type AuthProviderId = ProviderId;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum StoredCredentialKind {
    Oauth,
    ApiKey,
    WellKnown,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct StoredCredential {
    pub version: u32,
    pub provider: ProviderId,
    pub kind: StoredCredentialKind,
    #[serde(
        rename = "accessToken",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub access_token: Option<String>,
    #[serde(
        rename = "refreshToken",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub refresh_token: Option<String>,
    #[serde(rename = "apiKey", default, skip_serializing_if = "Option::is_none")]
    pub api_key: Option<String>,
    #[serde(rename = "expiresAt", default, skip_serializing_if = "Option::is_none")]
    pub expires_at: Option<String>,
    #[serde(rename = "accountId", default, skip_serializing_if = "Option::is_none")]
    pub account_id: Option<String>,
    #[serde(
        rename = "enterpriseUrl",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub enterprise_url: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub scopes: Vec<String>,
    #[serde(rename = "updatedAt")]
    pub updated_at: String,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub metadata: BTreeMap<String, String>,
}

impl StoredCredential {
    pub fn oauth(
        provider: ProviderId,
        access_token: impl Into<String>,
        refresh_token: impl Into<String>,
        expires_at: Option<String>,
        updated_at: impl Into<String>,
    ) -> Self {
        Self {
            version: CREDENTIAL_STORE_VERSION,
            provider,
            kind: StoredCredentialKind::Oauth,
            access_token: Some(access_token.into()),
            refresh_token: Some(refresh_token.into()),
            api_key: None,
            expires_at,
            account_id: None,
            enterprise_url: None,
            scopes: Vec::new(),
            updated_at: updated_at.into(),
            metadata: BTreeMap::new(),
        }
    }

    pub fn api_key(
        provider: ProviderId,
        api_key: impl Into<String>,
        updated_at: impl Into<String>,
    ) -> Self {
        Self {
            version: CREDENTIAL_STORE_VERSION,
            provider,
            kind: StoredCredentialKind::ApiKey,
            access_token: None,
            refresh_token: None,
            api_key: Some(api_key.into()),
            expires_at: None,
            account_id: None,
            enterprise_url: None,
            scopes: Vec::new(),
            updated_at: updated_at.into(),
            metadata: BTreeMap::new(),
        }
    }

    pub fn well_known(
        provider: ProviderId,
        access_token: impl Into<String>,
        updated_at: impl Into<String>,
    ) -> Self {
        Self {
            version: CREDENTIAL_STORE_VERSION,
            provider,
            kind: StoredCredentialKind::WellKnown,
            access_token: Some(access_token.into()),
            refresh_token: None,
            api_key: None,
            expires_at: None,
            account_id: None,
            enterprise_url: None,
            scopes: Vec::new(),
            updated_at: updated_at.into(),
            metadata: BTreeMap::new(),
        }
    }

    pub fn secret_values(&self) -> Vec<String> {
        [
            self.access_token.as_deref(),
            self.refresh_token.as_deref(),
            self.api_key.as_deref(),
        ]
        .into_iter()
        .flatten()
        .filter_map(non_empty)
        .map(str::to_string)
        .collect()
    }
}

#[derive(Debug, Clone)]
pub struct CredentialStore {
    data_dir: PathBuf,
}

impl CredentialStore {
    pub fn new(data_dir: impl Into<PathBuf>) -> Self {
        Self {
            data_dir: data_dir.into(),
        }
    }

    pub fn from_env() -> Option<Self> {
        default_data_dir_from_lookup(&|name| std::env::var(name).ok()).map(Self::new)
    }

    pub fn from_lookup(lookup: &dyn Fn(&str) -> Option<String>) -> Option<Self> {
        default_data_dir_from_lookup(lookup).map(Self::new)
    }

    pub fn data_dir(&self) -> &Path {
        &self.data_dir
    }

    pub fn credential_path(&self, provider: &ProviderId) -> PathBuf {
        let name = provider.as_str();
        if name.contains('/') || name.contains('\\') || name.contains('\0') {
            return self
                .data_dir
                .join(CREDENTIALS_DIR_NAME)
                .join("invalid.json");
        }
        self.data_dir
            .join(CREDENTIALS_DIR_NAME)
            .join(format!("{name}.json"))
    }

    pub fn load(
        &self,
        provider: &ProviderId,
    ) -> Result<Option<StoredCredential>, CredentialStoreError> {
        let path = self.credential_path(provider);
        let body = match fs::read_to_string(&path) {
            Ok(body) => body,
            Err(err) if err.kind() == io::ErrorKind::NotFound => return Ok(None),
            Err(source) => return Err(CredentialStoreError::Read { path, source }),
        };
        let credential = serde_json::from_str::<StoredCredential>(&body).map_err(|source| {
            CredentialStoreError::Parse {
                path: path.clone(),
                source,
            }
        })?;
        if credential.version != CREDENTIAL_STORE_VERSION || &credential.provider != provider {
            return Err(CredentialStoreError::InvalidCredential {
                path,
                reason: "credential version or provider does not match path".to_string(),
            });
        }
        Ok(Some(credential))
    }

    pub fn save(&self, credential: &StoredCredential) -> Result<(), CredentialStoreError> {
        if credential.version != CREDENTIAL_STORE_VERSION {
            return Err(CredentialStoreError::InvalidCredential {
                path: self.credential_path(&credential.provider),
                reason: format!("unsupported credential version {}", credential.version),
            });
        }

        let path = self.credential_path(&credential.provider);
        let parent = path
            .parent()
            .ok_or_else(|| CredentialStoreError::InvalidCredential {
                path: path.clone(),
                reason: "credential path has no parent".to_string(),
            })?;
        fs::create_dir_all(parent).map_err(|source| CredentialStoreError::CreateDir {
            path: parent.to_path_buf(),
            source,
        })?;

        let temp_path = path.with_extension(format!("json.tmp.{}", unique_suffix()));
        let body = serde_json::to_vec_pretty(credential).map_err(|source| {
            CredentialStoreError::Serialize {
                path: path.clone(),
                source,
            }
        })?;
        let write_result = write_credential_file_atomically(&temp_path, &path, &body);
        if write_result.is_err() {
            let _ = fs::remove_file(&temp_path);
        }
        write_result
    }

    pub fn delete(&self, provider: &ProviderId) -> Result<bool, CredentialStoreError> {
        let path = self.credential_path(provider);
        match fs::remove_file(&path) {
            Ok(()) => Ok(true),
            Err(err) if err.kind() == io::ErrorKind::NotFound => Ok(false),
            Err(source) => Err(CredentialStoreError::Delete { path, source }),
        }
    }

    pub fn manifest_entries(
        &self,
        providers: impl IntoIterator<Item = ProviderId>,
    ) -> Vec<CredentialStoreManifestEntry> {
        providers
            .into_iter()
            .collect::<BTreeSet<_>>()
            .into_iter()
            .map(|provider| {
                let path = self.credential_path(&provider);
                let stored = self.load(&provider).ok().flatten();
                let relative_path = format!("{CREDENTIALS_DIR_NAME}/{}.json", provider.as_str());
                CredentialStoreManifestEntry {
                    provider,
                    status: if stored.is_some() {
                        "excluded_stored".to_string()
                    } else {
                        "not_stored".to_string()
                    },
                    kind: stored.map(|credential| credential.kind),
                    relative_path,
                    absolute_path: path,
                }
            })
            .collect()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct CredentialStoreManifestEntry {
    pub provider: ProviderId,
    pub status: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub kind: Option<StoredCredentialKind>,
    pub relative_path: String,
    pub absolute_path: PathBuf,
}

#[derive(Debug, Error)]
pub enum CredentialStoreError {
    #[error("failed to create credential directory {path}: {source}")]
    CreateDir {
        path: PathBuf,
        #[source]
        source: io::Error,
    },
    #[error("failed to read credential store {path}: {source}")]
    Read {
        path: PathBuf,
        #[source]
        source: io::Error,
    },
    #[error("failed to parse credential store {path}: {source}")]
    Parse {
        path: PathBuf,
        #[source]
        source: serde_json::Error,
    },
    #[error("failed to serialize credential store {path}: {source}")]
    Serialize {
        path: PathBuf,
        #[source]
        source: serde_json::Error,
    },
    #[error("invalid credential store {path}: {reason}")]
    InvalidCredential { path: PathBuf, reason: String },
    #[error("failed to write credential store {path}: {source}")]
    Write {
        path: PathBuf,
        #[source]
        source: io::Error,
    },
    #[error("failed to replace credential store {path}: {source}")]
    Replace {
        path: PathBuf,
        #[source]
        source: io::Error,
    },
    #[error("failed to delete credential store {path}: {source}")]
    Delete {
        path: PathBuf,
        #[source]
        source: io::Error,
    },
}

fn write_credential_file_atomically(
    temp_path: &Path,
    final_path: &Path,
    body: &[u8],
) -> Result<(), CredentialStoreError> {
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(temp_path)
        .map_err(|source| CredentialStoreError::Write {
            path: temp_path.to_path_buf(),
            source,
        })?;
    restrict_file_permissions(temp_path).map_err(|source| CredentialStoreError::Write {
        path: temp_path.to_path_buf(),
        source,
    })?;
    file.write_all(body)
        .and_then(|_| file.sync_all())
        .map_err(|source| CredentialStoreError::Write {
            path: temp_path.to_path_buf(),
            source,
        })?;
    drop(file);
    fs::rename(temp_path, final_path).map_err(|source| CredentialStoreError::Replace {
        path: final_path.to_path_buf(),
        source,
    })?;
    restrict_file_permissions(final_path).map_err(|source| CredentialStoreError::Write {
        path: final_path.to_path_buf(),
        source,
    })?;
    Ok(())
}

#[cfg(unix)]
fn restrict_file_permissions(path: &Path) -> io::Result<()> {
    use std::os::unix::fs::PermissionsExt;

    fs::set_permissions(path, fs::Permissions::from_mode(0o600))
}

#[cfg(windows)]
fn restrict_file_permissions(path: &Path) -> io::Result<()> {
    use std::process::Command;

    let sid = current_windows_user_sid()?;
    let grant = format!("*{sid}:F");
    let output = Command::new("icacls")
        .arg(path)
        .args(["/inheritance:r", "/grant:r", &grant])
        .output()?;
    if output.status.success() {
        return Ok(());
    }

    let stderr = String::from_utf8_lossy(&output.stderr);
    let stdout = String::from_utf8_lossy(&output.stdout);
    let detail = [stdout.trim(), stderr.trim()]
        .into_iter()
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>()
        .join("; ");
    Err(io::Error::new(
        io::ErrorKind::PermissionDenied,
        format!("icacls failed while restricting credential file permissions: {detail}"),
    ))
}

#[cfg(all(not(unix), not(windows)))]
fn restrict_file_permissions(_path: &Path) -> io::Result<()> {
    Ok(())
}

#[cfg(windows)]
fn current_windows_user_sid() -> io::Result<String> {
    let output = std::process::Command::new("whoami")
        .args(["/user", "/fo", "csv", "/nh"])
        .output()?;
    if !output.status.success() {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            format!(
                "whoami failed while resolving current user SID: {}",
                String::from_utf8_lossy(&output.stderr).trim()
            ),
        ));
    }
    let stdout = String::from_utf8_lossy(&output.stdout);
    parse_whoami_user_sid(&stdout).ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            "whoami did not return a current user SID",
        )
    })
}

#[cfg(any(windows, test))]
fn parse_whoami_user_sid(output: &str) -> Option<String> {
    output.lines().map(str::trim).find_map(|line| {
        let fields = parse_csv_line(line);
        fields
            .into_iter()
            .find(|field| field.starts_with("S-1-") && field.len() > 4)
    })
}

#[cfg(any(windows, test))]
fn parse_csv_line(line: &str) -> Vec<String> {
    let mut fields = Vec::new();
    let mut field = String::new();
    let mut chars = line.chars().peekable();
    let mut quoted = false;

    while let Some(ch) = chars.next() {
        match ch {
            '"' if quoted && chars.peek() == Some(&'"') => {
                field.push('"');
                let _ = chars.next();
            }
            '"' => quoted = !quoted,
            ',' if !quoted => {
                fields.push(field.trim().to_string());
                field.clear();
            }
            _ => field.push(ch),
        }
    }
    fields.push(field.trim().to_string());
    fields
}

#[cfg(unix)]
pub fn credential_file_mode(path: &Path) -> io::Result<u32> {
    use std::os::unix::fs::PermissionsExt;

    fs::metadata(path).map(|metadata| metadata.permissions().mode() & 0o777)
}

fn unique_suffix() -> String {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or(Duration::from_secs(0))
        .as_nanos();
    format!("{}-{nanos}", std::process::id())
}

fn default_data_dir_from_lookup(lookup: &dyn Fn(&str) -> Option<String>) -> Option<PathBuf> {
    if let Some(path) = lookup("HARNESS_DATA_HOME").and_then(non_empty_owned) {
        return Some(PathBuf::from(path).join("harness"));
    }

    #[cfg(windows)]
    {
        lookup("LOCALAPPDATA")
            .or_else(|| lookup("APPDATA"))
            .and_then(non_empty_owned)
            .map(|path| PathBuf::from(path).join("harness"))
    }

    #[cfg(not(windows))]
    {
        lookup("XDG_DATA_HOME")
            .and_then(non_empty_owned)
            .map(|path| PathBuf::from(path).join("harness"))
            .or_else(|| {
                lookup("HOME").and_then(non_empty_owned).map(|home| {
                    PathBuf::from(home)
                        .join(".local")
                        .join("share")
                        .join("harness")
                })
            })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedCredential {
    pub token: String,
    pub source: ResolvedCredentialSource,
    pub expires_at: Option<String>,
    pub account_id: Option<String>,
    pub enterprise_url: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ResolvedCredentialSource {
    StoredOauth,
    StoredApiKey,
    EnvApiKey { env: String },
    InlineApiKey,
}

impl ResolvedCredentialSource {
    fn provider_kind(&self) -> ProviderCredentialKind {
        match self {
            Self::StoredOauth => ProviderCredentialKind::StoredOauth,
            Self::StoredApiKey => ProviderCredentialKind::StoredApiKey,
            Self::EnvApiKey { .. } => ProviderCredentialKind::EnvApiKey,
            Self::InlineApiKey => ProviderCredentialKind::InlineApiKey,
        }
    }
}

#[derive(Debug, Error)]
pub enum CredentialResolveError {
    #[error("credential store error: {0}")]
    Store(#[from] CredentialStoreError),
    #[error("stored OAuth credential for {provider} is expired and cannot be refreshed")]
    RefreshUnavailable { provider: AuthProviderId },
    #[error("credential refresh failed for {provider}: {message}")]
    RefreshFailed {
        provider: AuthProviderId,
        category: ProviderErrorCategory,
        message: String,
    },
    #[error("no usable credential found for {provider}")]
    Missing { provider: AuthProviderId },
}

impl CredentialResolveError {
    pub fn category(&self) -> ProviderErrorCategory {
        match self {
            Self::Store(_) => ProviderErrorCategory::TransportFailure,
            Self::RefreshUnavailable { .. } => ProviderErrorCategory::InvalidCredentials,
            Self::RefreshFailed { category, .. } => *category,
            Self::Missing { .. } => ProviderErrorCategory::MissingCredentials,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OAuthRefreshOutcome {
    pub access_token: String,
    pub refresh_token: Option<String>,
    pub expires_at: Option<String>,
    pub account_id: Option<String>,
    pub scopes: Vec<String>,
}

#[derive(Debug, Clone, Error)]
#[error("{message}")]
pub struct CredentialRefreshError {
    pub category: ProviderErrorCategory,
    pub message: String,
}

impl CredentialRefreshError {
    pub fn new(category: ProviderErrorCategory, message: impl Into<String>) -> Self {
        Self {
            category,
            message: message.into(),
        }
    }
}

#[async_trait]
pub trait OAuthTokenRefresher: Send + Sync {
    async fn refresh(
        &self,
        provider: &ProviderId,
        credential: &StoredCredential,
    ) -> Result<OAuthRefreshOutcome, CredentialRefreshError>;
}

pub trait CredentialClock: Send + Sync {
    fn now(&self) -> SystemTime;

    fn now_rfc3339(&self) -> String {
        humantime::format_rfc3339(self.now()).to_string()
    }
}

#[derive(Debug, Default)]
pub struct SystemCredentialClock;

impl CredentialClock for SystemCredentialClock {
    fn now(&self) -> SystemTime {
        SystemTime::now()
    }
}

pub struct ProviderCredentialManager {
    store: CredentialStore,
    provider: AuthProviderId,
    api_key_env: Vec<String>,
    inline_api_key: String,
    env_lookup: Arc<CredentialEnvLookup>,
    clock: Arc<dyn CredentialClock>,
    refresher: Option<Arc<dyn OAuthTokenRefresher>>,
    refresh_lock: Arc<Mutex<()>>,
}

type CredentialEnvLookup = dyn Fn(&str) -> Option<String> + Send + Sync;

impl ProviderCredentialManager {
    pub fn new(
        store: CredentialStore,
        provider: AuthProviderId,
        api_key_env: Vec<String>,
        inline_api_key: impl Into<String>,
        env_lookup: impl Fn(&str) -> Option<String> + Send + Sync + 'static,
    ) -> Self {
        Self {
            store,
            provider,
            api_key_env,
            inline_api_key: inline_api_key.into(),
            env_lookup: Arc::new(env_lookup),
            clock: Arc::new(SystemCredentialClock),
            refresher: None,
            refresh_lock: Arc::new(Mutex::new(())),
        }
    }

    pub fn with_clock(mut self, clock: Arc<dyn CredentialClock>) -> Self {
        self.clock = clock;
        self
    }

    pub fn with_refresher(mut self, refresher: Arc<dyn OAuthTokenRefresher>) -> Self {
        self.refresher = Some(refresher);
        self
    }

    pub async fn resolve(&self) -> Result<ResolvedCredential, CredentialResolveError> {
        if let Some(credential) = self.store.load(&self.provider)? {
            if let Some(resolved) = self.resolve_stored(&credential).await? {
                return Ok(resolved);
            }
        }

        if let Some((env, token)) = self.first_env_api_key() {
            return Ok(ResolvedCredential {
                token,
                source: ResolvedCredentialSource::EnvApiKey { env },
                expires_at: None,
                account_id: None,
                enterprise_url: None,
            });
        }

        if let Some(token) = non_empty(&self.inline_api_key).map(str::to_string) {
            return Ok(ResolvedCredential {
                token,
                source: ResolvedCredentialSource::InlineApiKey,
                expires_at: None,
                account_id: None,
                enterprise_url: None,
            });
        }

        Err(CredentialResolveError::Missing {
            provider: self.provider.clone(),
        })
    }

    async fn resolve_stored(
        &self,
        credential: &StoredCredential,
    ) -> Result<Option<ResolvedCredential>, CredentialResolveError> {
        match credential.kind {
            StoredCredentialKind::ApiKey => Ok(credential
                .api_key
                .as_deref()
                .and_then(non_empty)
                .map(|token| ResolvedCredential {
                    token: token.to_string(),
                    source: ResolvedCredentialSource::StoredApiKey,
                    expires_at: None,
                    account_id: credential.account_id.clone(),
                    enterprise_url: credential.enterprise_url.clone(),
                })),
            StoredCredentialKind::WellKnown => Ok(credential
                .access_token
                .as_deref()
                .and_then(non_empty)
                .map(|token| ResolvedCredential {
                    token: token.to_string(),
                    source: ResolvedCredentialSource::StoredOauth,
                    expires_at: credential.expires_at.clone(),
                    account_id: credential.account_id.clone(),
                    enterprise_url: credential.enterprise_url.clone(),
                })),
            StoredCredentialKind::Oauth => {
                if credential
                    .access_token
                    .as_deref()
                    .and_then(non_empty)
                    .is_some()
                    && !self.oauth_is_expired(credential)
                {
                    return Ok(credential.access_token.as_deref().map(|token| {
                        ResolvedCredential {
                            token: token.to_string(),
                            source: ResolvedCredentialSource::StoredOauth,
                            expires_at: credential.expires_at.clone(),
                            account_id: credential.account_id.clone(),
                            enterprise_url: credential.enterprise_url.clone(),
                        }
                    }));
                }

                self.refresh_oauth_single_flight().await.map(Some)
            }
        }
    }

    async fn refresh_oauth_single_flight(
        &self,
    ) -> Result<ResolvedCredential, CredentialResolveError> {
        let _guard = self.refresh_lock.lock().await;
        let Some(current) = self.store.load(&self.provider)? else {
            return Err(CredentialResolveError::Missing {
                provider: self.provider.clone(),
            });
        };
        if current.kind != StoredCredentialKind::Oauth {
            if let Some(token) = current.api_key.as_deref().and_then(non_empty) {
                return Ok(ResolvedCredential {
                    token: token.to_string(),
                    source: ResolvedCredentialSource::StoredApiKey,
                    expires_at: None,
                    account_id: current.account_id,
                    enterprise_url: current.enterprise_url,
                });
            }
            return Err(CredentialResolveError::Missing {
                provider: self.provider.clone(),
            });
        }
        if current
            .access_token
            .as_deref()
            .and_then(non_empty)
            .is_some()
            && !self.oauth_is_expired(&current)
        {
            return Ok(ResolvedCredential {
                token: current.access_token.unwrap_or_default(),
                source: ResolvedCredentialSource::StoredOauth,
                expires_at: current.expires_at,
                account_id: current.account_id,
                enterprise_url: current.enterprise_url,
            });
        }

        let refresher =
            self.refresher
                .as_ref()
                .ok_or(CredentialResolveError::RefreshUnavailable {
                    provider: self.provider.clone(),
                })?;
        if current
            .refresh_token
            .as_deref()
            .and_then(non_empty)
            .is_none()
        {
            return Err(CredentialResolveError::RefreshUnavailable {
                provider: self.provider.clone(),
            });
        }

        let outcome = refresher
            .refresh(&self.provider, &current)
            .await
            .map_err(|err| CredentialResolveError::RefreshFailed {
                provider: self.provider.clone(),
                category: err.category,
                message: err.message,
            })?;
        let access_token = outcome.access_token;
        let mut refreshed = StoredCredential::oauth(
            self.provider.clone(),
            access_token.clone(),
            outcome
                .refresh_token
                .or(current.refresh_token)
                .unwrap_or_default(),
            outcome.expires_at.clone(),
            self.clock.now_rfc3339(),
        );
        refreshed.account_id = outcome.account_id.or(current.account_id);
        refreshed.enterprise_url = current.enterprise_url;
        refreshed.scopes = if outcome.scopes.is_empty() {
            current.scopes
        } else {
            outcome.scopes
        };
        self.store.save(&refreshed)?;

        Ok(ResolvedCredential {
            token: access_token,
            source: ResolvedCredentialSource::StoredOauth,
            expires_at: refreshed.expires_at,
            account_id: refreshed.account_id,
            enterprise_url: refreshed.enterprise_url,
        })
    }

    fn oauth_is_expired(&self, credential: &StoredCredential) -> bool {
        let Some(expires_at) = credential.expires_at.as_deref().and_then(non_empty) else {
            return false;
        };
        let Ok(expires_at) = humantime::parse_rfc3339(expires_at) else {
            return true;
        };
        expires_at <= self.clock.now()
    }

    fn first_env_api_key(&self) -> Option<(String, String)> {
        self.api_key_env.iter().find_map(|env| {
            let value = (self.env_lookup)(env)?;
            non_empty(&value).map(|value| (env.clone(), value.to_string()))
        })
    }
}

#[async_trait]
impl ProviderCredentialSource for ProviderCredentialManager {
    async fn bearer_token(&self) -> Result<ProviderBearerToken, ProviderCredentialError> {
        self.resolve()
            .await
            .map(|credential| ProviderBearerToken {
                token: credential.token,
                kind: credential.source.provider_kind(),
                account_id: credential.account_id,
                enterprise_url: credential.enterprise_url,
            })
            .map_err(|err| ProviderCredentialError::new(err.category(), err.to_string()))
    }
}

fn non_empty(value: &str) -> Option<&str> {
    let trimmed = value.trim();
    (!trimmed.is_empty()).then_some(trimmed)
}

fn non_empty_owned(value: String) -> Option<String> {
    let trimmed = value.trim();
    (!trimmed.is_empty()).then(|| trimmed.to_string())
}

#[cfg(test)]
#[cfg(test)]
mod tests;
