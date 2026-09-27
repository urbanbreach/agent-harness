use serde::{Deserialize, Serialize};
use std::{collections::BTreeMap, fmt, time::SystemTime};
pub mod codex;
pub mod copilot;
mod manager;
mod oauth_http;
pub use oauth_http::ReqwestAuthHttpClient;
pub mod plugin;
mod store;
pub use manager::*;
pub use store::*;
#[cfg(test)]
mod tests;

#[derive(
    Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, schemars::JsonSchema,
)]
#[serde(try_from = "String", into = "String")]
pub struct ProviderId(String);
pub type AuthProviderId = ProviderId;

impl ProviderId {
    pub fn codex() -> Self {
        Self("codex".into())
    }
    pub fn github_copilot() -> Self {
        Self("github-copilot".into())
    }
    pub fn parse(value: &str) -> Option<Self> {
        let value = value.trim();
        (!value.is_empty()
            && value.len() <= 128
            && !value.contains("..")
            && value
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"._-".contains(&b)))
        .then(|| Self(value.into()))
    }
    pub fn as_str(&self) -> &str {
        &self.0
    }
}
impl TryFrom<String> for ProviderId {
    type Error = &'static str;
    fn try_from(value: String) -> Result<Self, Self::Error> {
        Self::parse(&value).ok_or("invalid authentication provider identifier")
    }
}
impl From<ProviderId> for String {
    fn from(value: ProviderId) -> Self {
        value.0
    }
}
impl std::fmt::Display for ProviderId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum StoredCredentialKind {
    Oauth,
    ApiKey,
    WellKnown,
}

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct StoredCredential {
    pub version: u32,
    pub provider: ProviderId,
    pub kind: StoredCredentialKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub access_token: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub refresh_token: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub api_key: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expires_at: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub account_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub enterprise_url: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub scopes: Vec<String>,
    pub updated_at: String,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub metadata: BTreeMap<String, String>,
}
impl fmt::Debug for StoredCredential {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("StoredCredential")
            .field("provider", &self.provider)
            .field("kind", &self.kind)
            .finish_non_exhaustive()
    }
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
            version: 1,
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
        key: impl Into<String>,
        updated_at: impl Into<String>,
    ) -> Self {
        let mut value = Self::oauth(provider, "", "", None, updated_at);
        value.kind = StoredCredentialKind::ApiKey;
        value.access_token = None;
        value.refresh_token = None;
        value.api_key = Some(key.into());
        value
    }
    pub fn well_known(
        provider: ProviderId,
        token: impl Into<String>,
        updated_at: impl Into<String>,
    ) -> Self {
        let mut value = Self::oauth(provider, token, "", None, updated_at);
        value.kind = StoredCredentialKind::WellKnown;
        value.refresh_token = None;
        value
    }
    pub fn secret_values(&self) -> Vec<String> {
        [&self.access_token, &self.refresh_token, &self.api_key]
            .into_iter()
            .filter_map(|v| v.as_ref())
            .filter(|v| !v.is_empty())
            .cloned()
            .collect()
    }
    pub(crate) fn validate(&self, provider: &ProviderId) -> Result<(), CredentialStoreError> {
        let token = match self.kind {
            StoredCredentialKind::ApiKey => &self.api_key,
            StoredCredentialKind::Oauth | StoredCredentialKind::WellKnown => &self.access_token,
        };
        if self.version != 1
            || &self.provider != provider
            || !token.as_deref().is_some_and(usable_token)
        {
            return Err(CredentialStoreError::Invalid(
                "invalid credential version, identity, or token",
            ));
        }
        Ok(())
    }
}
pub(crate) fn usable_token(token: &str) -> bool {
    !token.trim().is_empty() && !token.chars().any(char::is_control)
}

pub trait CredentialClock: Send + Sync {
    fn now(&self) -> SystemTime;
    fn now_rfc3339(&self) -> String {
        humantime::format_rfc3339(self.now()).to_string()
    }
}
pub struct SystemCredentialClock;
impl CredentialClock for SystemCredentialClock {
    fn now(&self) -> SystemTime {
        SystemTime::now()
    }
}
