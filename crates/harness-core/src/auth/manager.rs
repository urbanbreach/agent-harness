use super::*;
use harness_providers::{
    ProviderBearerToken, ProviderCredentialError, ProviderCredentialKind, ProviderCredentialSource,
    ProviderErrorCategory,
};
use std::{sync::Arc, time::Duration};
use tokio::sync::Mutex;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ResolvedCredentialSource {
    StoredOauth,
    StoredApiKey,
    EnvApiKey { env: String },
    InlineApiKey,
}
#[derive(Clone)]
pub struct ResolvedCredential {
    pub token: String,
    pub source: ResolvedCredentialSource,
    pub expires_at: Option<String>,
    pub account_id: Option<String>,
    pub enterprise_url: Option<String>,
}
impl fmt::Debug for ResolvedCredential {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ResolvedCredential")
            .field("source", &self.source)
            .finish_non_exhaustive()
    }
}
#[derive(Debug, thiserror::Error)]
pub enum CredentialResolveError {
    #[error("credential redaction: {0}")]
    Redaction(&'static str),
    #[error(transparent)]
    Store(#[from] CredentialStoreError),
    #[error("OAuth refresh is unavailable for {provider}")]
    RefreshUnavailable { provider: ProviderId },
    #[error("OAuth refresh failed for {provider}: {message}")]
    RefreshFailed {
        provider: ProviderId,
        category: ProviderErrorCategory,
        message: String,
    },
    #[error("no credential configured for {provider}")]
    Missing { provider: ProviderId },
}
impl CredentialResolveError {
    pub fn category(&self) -> ProviderErrorCategory {
        match self {
            Self::Store(_) => ProviderErrorCategory::TransportFailure,
            Self::RefreshUnavailable { .. } => ProviderErrorCategory::InvalidCredentials,
            Self::RefreshFailed { category, .. } => *category,
            Self::Missing { .. } => ProviderErrorCategory::MissingCredentials,
            Self::Redaction(_) => ProviderErrorCategory::InvalidCredentials,
        }
    }
}
pub struct OAuthRefreshOutcome {
    pub access_token: String,
    pub refresh_token: Option<String>,
    pub expires_at: Option<String>,
    pub account_id: Option<String>,
    pub scopes: Vec<String>,
}
#[derive(Debug, thiserror::Error)]
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
#[async_trait::async_trait]
pub trait OAuthTokenRefresher: Send + Sync {
    async fn refresh(
        &self,
        provider: &ProviderId,
        credential: &StoredCredential,
    ) -> Result<OAuthRefreshOutcome, CredentialRefreshError>;
}
pub struct ProviderCredentialManager {
    store: Option<CredentialStore>,
    provider: ProviderId,
    api_key_env: Vec<String>,
    inline_api_key: String,
    env_lookup: Arc<dyn Fn(&str) -> Option<String> + Send + Sync>,
    clock: Arc<dyn CredentialClock>,
    refresher: Option<Arc<dyn OAuthTokenRefresher>>,
    refresh_lock: Mutex<()>,
    secrets: Arc<crate::redact::SecretRegistry>,
}
impl ProviderCredentialManager {
    pub fn new(
        store: impl Into<Option<CredentialStore>>,
        provider: ProviderId,
        api_key_env: Vec<String>,
        inline_api_key: impl Into<String>,
        env_lookup: impl Fn(&str) -> Option<String> + Send + Sync + 'static,
    ) -> Self {
        Self {
            store: store.into(),
            provider,
            api_key_env,
            inline_api_key: inline_api_key.into(),
            env_lookup: Arc::new(env_lookup),
            clock: Arc::new(SystemCredentialClock),
            refresher: None,
            refresh_lock: Mutex::new(()),
            secrets: Arc::new(crate::redact::SecretRegistry::default()),
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
    pub fn with_secret_registry(
        mut self,
        secrets: Arc<crate::redact::SecretRegistry>,
    ) -> Result<Self, CredentialResolveError> {
        self.secrets = secrets;
        if let Some(stored) = self
            .store
            .as_ref()
            .map(|store| store.load(&self.provider))
            .transpose()?
            .flatten()
        {
            self.register_stored(&stored)?;
        }
        Ok(self)
    }
    fn register_stored(&self, stored: &StoredCredential) -> Result<(), CredentialResolveError> {
        self.secrets
            .register(
                stored
                    .api_key
                    .iter()
                    .chain(&stored.access_token)
                    .chain(&stored.refresh_token)
                    .cloned(),
            )
            .map_err(CredentialResolveError::Redaction)
    }
    pub async fn resolve(&self) -> Result<ResolvedCredential, CredentialResolveError> {
        let stored = match &self.store {
            Some(store) => store.load(&self.provider)?,
            None => None,
        };
        if let Some(stored) = stored {
            self.register_stored(&stored)?;
            if stored.kind == StoredCredentialKind::Oauth {
                return self.refresh_oauth_if_near_expiry(Duration::ZERO).await;
            }
            return resolve_stored(stored);
        }
        for env in &self.api_key_env {
            if let Some(token) = (self.env_lookup)(env).filter(|v| usable_token(v)) {
                self.secrets
                    .register([token.clone()])
                    .map_err(CredentialResolveError::Redaction)?;
                return Ok(resolve_key(
                    token,
                    ResolvedCredentialSource::EnvApiKey { env: env.clone() },
                ));
            }
        }
        if usable_token(&self.inline_api_key) {
            self.secrets
                .register([self.inline_api_key.clone()])
                .map_err(CredentialResolveError::Redaction)?;
            return Ok(resolve_key(
                self.inline_api_key.clone(),
                ResolvedCredentialSource::InlineApiKey,
            ));
        }
        Err(self.missing())
    }
    pub async fn refresh_oauth_if_near_expiry(
        &self,
        leeway: Duration,
    ) -> Result<ResolvedCredential, CredentialResolveError> {
        let _guard = self.refresh_lock.lock().await;
        let store = self.store.as_ref().ok_or_else(|| self.missing())?;
        let mut stored = store.load(&self.provider)?.ok_or_else(|| self.missing())?;
        self.register_stored(&stored)?;
        if stored.kind != StoredCredentialKind::Oauth {
            return Err(self.missing());
        }
        if !self.expired(&stored, leeway) {
            return resolve_stored(stored);
        }
        let refresher =
            self.refresher
                .as_ref()
                .ok_or_else(|| CredentialResolveError::RefreshUnavailable {
                    provider: self.provider.clone(),
                })?;
        let outcome = refresher
            .refresh(&self.provider, &stored)
            .await
            .map_err(|e| CredentialResolveError::RefreshFailed {
                provider: self.provider.clone(),
                category: e.category,
                message: "token refresh was rejected".into(),
            })?;
        if !usable_token(&outcome.access_token) {
            return Err(CredentialResolveError::RefreshFailed {
                provider: self.provider.clone(),
                category: ProviderErrorCategory::InvalidCredentials,
                message: "refresh returned an unusable token".into(),
            });
        }
        let old = stored.clone();
        stored.access_token = Some(outcome.access_token);
        if let Some(refresh) = outcome.refresh_token {
            stored.refresh_token = Some(refresh);
        }
        stored.expires_at = outcome.expires_at;
        if outcome.account_id.is_some() {
            stored.account_id = outcome.account_id;
        }
        if !outcome.scopes.is_empty() {
            stored.scopes = outcome.scopes;
        }
        stored.updated_at = self.clock.now_rfc3339();
        self.register_stored(&stored)?;
        store.replace_if_unchanged(&old, &stored)?;
        resolve_stored(stored)
    }
    fn missing(&self) -> CredentialResolveError {
        CredentialResolveError::Missing {
            provider: self.provider.clone(),
        }
    }
    fn expired(&self, stored: &StoredCredential, leeway: Duration) -> bool {
        stored.expires_at.as_deref().is_some_and(|expiry| {
            let Ok(expiry) = humantime::parse_rfc3339(expiry) else {
                return true;
            };
            self.clock
                .now()
                .checked_add(leeway)
                .is_none_or(|now| expiry <= now)
        })
    }
}
fn resolve_key(token: String, source: ResolvedCredentialSource) -> ResolvedCredential {
    ResolvedCredential {
        token,
        source,
        expires_at: None,
        account_id: None,
        enterprise_url: None,
    }
}
fn resolve_stored(value: StoredCredential) -> Result<ResolvedCredential, CredentialResolveError> {
    value.validate(&value.provider)?;
    let (token, source) = match value.kind {
        StoredCredentialKind::ApiKey => (value.api_key, ResolvedCredentialSource::StoredApiKey),
        StoredCredentialKind::Oauth | StoredCredentialKind::WellKnown => {
            (value.access_token, ResolvedCredentialSource::StoredOauth)
        }
    };
    Ok(ResolvedCredential {
        token: token.ok_or(CredentialStoreError::Invalid("missing credential token"))?,
        source,
        expires_at: value.expires_at,
        account_id: value.account_id,
        enterprise_url: value.enterprise_url,
    })
}
#[async_trait::async_trait]
impl ProviderCredentialSource for ProviderCredentialManager {
    async fn bearer_token(&self) -> Result<ProviderBearerToken, ProviderCredentialError> {
        let value = self
            .resolve()
            .await
            .map_err(|e| ProviderCredentialError::new(e.category(), e.to_string()))?;
        Ok(ProviderBearerToken {
            token: value.token,
            account_id: value.account_id,
            enterprise_url: value.enterprise_url,
            kind: match value.source {
                ResolvedCredentialSource::StoredOauth => ProviderCredentialKind::StoredOauth,
                ResolvedCredentialSource::StoredApiKey => ProviderCredentialKind::StoredApiKey,
                ResolvedCredentialSource::EnvApiKey { .. } => ProviderCredentialKind::EnvApiKey,
                ResolvedCredentialSource::InlineApiKey => ProviderCredentialKind::InlineApiKey,
            },
        })
    }
}
