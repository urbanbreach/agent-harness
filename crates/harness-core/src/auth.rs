use std::collections::BTreeSet;
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

const CREDENTIAL_STORE_VERSION: u32 = 1;
const CREDENTIALS_DIR_NAME: &str = "credentials";

#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, JsonSchema,
)]
#[serde(rename_all = "kebab-case")]
pub enum AuthProviderId {
    Codex,
    #[serde(rename = "github-copilot")]
    GithubCopilot,
}

impl AuthProviderId {
    pub const ALL: [Self; 2] = [Self::Codex, Self::GithubCopilot];

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Codex => "codex",
            Self::GithubCopilot => "github-copilot",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value.trim() {
            "codex" => Some(Self::Codex),
            "github-copilot" => Some(Self::GithubCopilot),
            _ => None,
        }
    }
}

impl fmt::Display for AuthProviderId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum StoredCredentialKind {
    Oauth,
    ApiKey,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct StoredCredential {
    pub version: u32,
    pub provider: AuthProviderId,
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
}

impl StoredCredential {
    pub fn oauth(
        provider: AuthProviderId,
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
        }
    }

    pub fn api_key(
        provider: AuthProviderId,
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

    pub fn credential_path(&self, provider: AuthProviderId) -> PathBuf {
        self.data_dir
            .join(CREDENTIALS_DIR_NAME)
            .join(format!("{}.json", provider.as_str()))
    }

    pub fn load(
        &self,
        provider: AuthProviderId,
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
        if credential.version != CREDENTIAL_STORE_VERSION || credential.provider != provider {
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
                path: self.credential_path(credential.provider),
                reason: format!("unsupported credential version {}", credential.version),
            });
        }

        let path = self.credential_path(credential.provider);
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

    pub fn delete(&self, provider: AuthProviderId) -> Result<bool, CredentialStoreError> {
        let path = self.credential_path(provider);
        match fs::remove_file(&path) {
            Ok(()) => Ok(true),
            Err(err) if err.kind() == io::ErrorKind::NotFound => Ok(false),
            Err(source) => Err(CredentialStoreError::Delete { path, source }),
        }
    }

    pub fn manifest_entries(
        &self,
        providers: impl IntoIterator<Item = AuthProviderId>,
    ) -> Vec<CredentialStoreManifestEntry> {
        providers
            .into_iter()
            .collect::<BTreeSet<_>>()
            .into_iter()
            .map(|provider| {
                let path = self.credential_path(provider);
                let stored = self.load(provider).ok().flatten();
                CredentialStoreManifestEntry {
                    provider,
                    status: if stored.is_some() {
                        "excluded_stored".to_string()
                    } else {
                        "not_stored".to_string()
                    },
                    kind: stored.map(|credential| credential.kind),
                    relative_path: format!("{CREDENTIALS_DIR_NAME}/{}.json", provider.as_str()),
                    absolute_path: path,
                }
            })
            .collect()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct CredentialStoreManifestEntry {
    pub provider: AuthProviderId,
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
        provider: AuthProviderId,
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
        if let Some(credential) = self.store.load(self.provider)? {
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
            provider: self.provider,
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
        let Some(current) = self.store.load(self.provider)? else {
            return Err(CredentialResolveError::Missing {
                provider: self.provider,
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
                provider: self.provider,
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
                    provider: self.provider,
                })?;
        if current
            .refresh_token
            .as_deref()
            .and_then(non_empty)
            .is_none()
        {
            return Err(CredentialResolveError::RefreshUnavailable {
                provider: self.provider,
            });
        }

        let outcome = refresher
            .refresh(self.provider, &current)
            .await
            .map_err(|err| CredentialResolveError::RefreshFailed {
                provider: self.provider,
                category: err.category,
                message: err.message,
            })?;
        let access_token = outcome.access_token;
        let mut refreshed = StoredCredential::oauth(
            self.provider,
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
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    #[derive(Debug)]
    struct FixedClock(SystemTime);

    impl CredentialClock for FixedClock {
        fn now(&self) -> SystemTime {
            self.0
        }
    }

    #[derive(Debug)]
    struct CountingRefresher {
        calls: AtomicUsize,
        expires_at: String,
    }

    #[async_trait]
    impl OAuthTokenRefresher for CountingRefresher {
        async fn refresh(
            &self,
            provider: AuthProviderId,
            credential: &StoredCredential,
        ) -> Result<OAuthRefreshOutcome, CredentialRefreshError> {
            assert_eq!(provider, AuthProviderId::Codex);
            assert_eq!(credential.refresh_token.as_deref(), Some("refresh-old"));
            self.calls.fetch_add(1, Ordering::SeqCst);
            tokio::time::sleep(Duration::from_millis(25)).await;
            Ok(OAuthRefreshOutcome {
                access_token: "access-new".to_string(),
                refresh_token: Some("refresh-new".to_string()),
                expires_at: Some(self.expires_at.clone()),
                account_id: Some("acct-new".to_string()),
                scopes: vec!["openid".to_string()],
            })
        }
    }

    #[test]
    fn credential_store_round_trips_replaces_atomically_and_uses_restrictive_permissions() {
        let temp = tempfile::tempdir().expect("tempdir");
        let store = CredentialStore::new(temp.path());
        let first = StoredCredential::api_key(
            AuthProviderId::Codex,
            "stored-api-key-old",
            "2026-05-30T00:00:00Z",
        );
        store.save(&first).expect("save first credential");
        let second = StoredCredential::api_key(
            AuthProviderId::Codex,
            "stored-api-key-new",
            "2026-05-30T00:00:01Z",
        );
        store.save(&second).expect("replace credential atomically");

        let loaded = store
            .load(AuthProviderId::Codex)
            .expect("load credential")
            .expect("stored credential");
        assert_eq!(loaded.api_key.as_deref(), Some("stored-api-key-new"));
        assert_eq!(loaded.secret_values(), vec!["stored-api-key-new"]);

        #[cfg(unix)]
        assert_eq!(
            credential_file_mode(&store.credential_path(AuthProviderId::Codex)).expect("mode"),
            0o600
        );
    }

    #[tokio::test]
    async fn credential_resolution_precedence_prefers_stored_then_env_then_inline() {
        let temp = tempfile::tempdir().expect("tempdir");
        let store = CredentialStore::new(temp.path());
        let manager = ProviderCredentialManager::new(
            store.clone(),
            AuthProviderId::Codex,
            vec!["HARNESS_TEST_API_KEY".to_string()],
            "inline-key",
            |name| (name == "HARNESS_TEST_API_KEY").then(|| "env-key".to_string()),
        );

        let resolved = manager.resolve().await.expect("env credential");
        assert_eq!(
            resolved.source,
            ResolvedCredentialSource::EnvApiKey {
                env: "HARNESS_TEST_API_KEY".to_string()
            }
        );
        assert_eq!(resolved.token, "env-key");

        store
            .save(&StoredCredential::api_key(
                AuthProviderId::Codex,
                "stored-api-key",
                "2026-05-30T00:00:00Z",
            ))
            .expect("save api credential");
        let resolved = manager.resolve().await.expect("stored api credential");
        assert_eq!(resolved.source, ResolvedCredentialSource::StoredApiKey);
        assert_eq!(resolved.token, "stored-api-key");

        store
            .save(&StoredCredential::oauth(
                AuthProviderId::Codex,
                "stored-oauth-access",
                "stored-oauth-refresh",
                Some("2099-01-01T00:00:00Z".to_string()),
                "2026-05-30T00:00:00Z",
            ))
            .expect("replace with oauth credential");
        let resolved = manager.resolve().await.expect("stored oauth credential");
        assert_eq!(resolved.source, ResolvedCredentialSource::StoredOauth);
        assert_eq!(resolved.token, "stored-oauth-access");
    }

    #[tokio::test]
    async fn credential_resolution_preserves_copilot_enterprise_url() {
        let temp = tempfile::tempdir().expect("tempdir");
        let store = CredentialStore::new(temp.path());
        let mut credential = StoredCredential::oauth(
            AuthProviderId::GithubCopilot,
            "stored-copilot-access",
            "stored-copilot-refresh",
            None,
            "2026-05-30T00:00:00Z",
        );
        credential.enterprise_url = Some("ghe.example.com".to_string());
        store.save(&credential).expect("save copilot credential");

        let manager = ProviderCredentialManager::new(
            store,
            AuthProviderId::GithubCopilot,
            Vec::new(),
            "",
            |_| None,
        );

        let bearer = manager.bearer_token().await.expect("bearer token");
        assert_eq!(bearer.token, "stored-copilot-access");
        assert_eq!(bearer.enterprise_url.as_deref(), Some("ghe.example.com"));
    }

    #[tokio::test]
    async fn expired_oauth_refresh_is_single_flight_and_persisted() {
        let temp = tempfile::tempdir().expect("tempdir");
        let store = CredentialStore::new(temp.path());
        store
            .save(&StoredCredential::oauth(
                AuthProviderId::Codex,
                "access-old",
                "refresh-old",
                Some("2026-05-29T00:00:00Z".to_string()),
                "2026-05-29T00:00:00Z",
            ))
            .expect("save expired oauth credential");
        let refresher = Arc::new(CountingRefresher {
            calls: AtomicUsize::new(0),
            expires_at: "2026-05-31T00:00:00Z".to_string(),
        });
        let manager = Arc::new(
            ProviderCredentialManager::new(
                store.clone(),
                AuthProviderId::Codex,
                Vec::new(),
                "",
                |_| None,
            )
            .with_clock(Arc::new(FixedClock(
                humantime::parse_rfc3339("2026-05-30T00:00:00Z").expect("clock"),
            )))
            .with_refresher(refresher.clone()),
        );

        let first = tokio::spawn({
            let manager = manager.clone();
            async move { manager.resolve().await.expect("first resolve") }
        });
        let second = tokio::spawn({
            let manager = manager.clone();
            async move { manager.resolve().await.expect("second resolve") }
        });
        let first = first.await.expect("first join");
        let second = second.await.expect("second join");

        assert_eq!(first.token, "access-new");
        assert_eq!(second.token, "access-new");
        assert_eq!(refresher.calls.load(Ordering::SeqCst), 1);
        let stored = store
            .load(AuthProviderId::Codex)
            .expect("load refreshed")
            .expect("refreshed credential");
        assert_eq!(stored.access_token.as_deref(), Some("access-new"));
        assert_eq!(stored.refresh_token.as_deref(), Some("refresh-new"));
        assert_eq!(stored.account_id.as_deref(), Some("acct-new"));
    }

    #[test]
    fn credential_store_manifest_excludes_secret_material() {
        let temp = tempfile::tempdir().expect("tempdir");
        let store = CredentialStore::new(temp.path());
        store
            .save(&StoredCredential::oauth(
                AuthProviderId::GithubCopilot,
                "access-secret-value",
                "refresh-secret-value",
                Some("2099-01-01T00:00:00Z".to_string()),
                "2026-05-30T00:00:00Z",
            ))
            .expect("save credential");

        let manifest =
            serde_json::to_string(&store.manifest_entries([AuthProviderId::GithubCopilot]))
                .expect("serialize manifest");
        assert!(manifest.contains("github-copilot"));
        assert!(!manifest.contains("access-secret-value"));
        assert!(!manifest.contains("refresh-secret-value"));
    }

    #[test]
    fn windows_whoami_csv_sid_parser_accepts_quoted_user_rows() {
        assert_eq!(
            parse_whoami_user_sid("\"EXAMPLE\\\\user\",\"S-1-5-21-111-222-333-1001\"\r\n"),
            Some("S-1-5-21-111-222-333-1001".to_string())
        );
        assert_eq!(parse_whoami_user_sid("\"EXAMPLE\\\\user\",\"\""), None);
    }
}
