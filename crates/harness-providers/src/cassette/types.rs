use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use reqwest::header::{HeaderMap, HeaderName, HeaderValue};
use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::{CompletionRequest, ProviderStreamEvent};

use super::{assert_cassette_is_safe, assert_http_cassette_is_safe};

pub(crate) const CASSETTE_VERSION: u32 = 1;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CassetteMode {
    Replay,
    Record,
    Auto,
}

impl CassetteMode {
    pub fn resolve_for_ci(self, ci: bool) -> Self {
        if ci {
            Self::Replay
        } else {
            self
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CassetteInteraction {
    pub request: CompletionRequest,
    pub events: Vec<ProviderStreamEvent>,
}

impl CassetteInteraction {
    pub fn new(request: CompletionRequest, events: Vec<ProviderStreamEvent>) -> Self {
        Self { request, events }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ProviderCassette {
    pub version: u32,
    pub interactions: Vec<CassetteInteraction>,
}

impl ProviderCassette {
    pub fn new(interactions: Vec<CassetteInteraction>) -> Self {
        Self {
            version: CASSETTE_VERSION,
            interactions,
        }
    }

    pub fn read_from(path: &Path) -> Result<Self, CassetteError> {
        let body = fs::read_to_string(path).map_err(|source| CassetteError::Read {
            path: path.to_path_buf(),
            source,
        })?;
        let cassette: Self =
            serde_json::from_str(&body).map_err(|source| CassetteError::Parse {
                path: path.to_path_buf(),
                source,
            })?;
        if cassette.version != CASSETTE_VERSION {
            return Err(CassetteError::UnsupportedVersion {
                path: path.to_path_buf(),
                version: cassette.version,
            });
        }
        Ok(cassette)
    }

    pub fn write_to(&self, path: &Path) -> Result<(), CassetteError> {
        assert_cassette_is_safe(self)?;
        if let Some(parent) = path
            .parent()
            .filter(|parent| !parent.as_os_str().is_empty())
        {
            fs::create_dir_all(parent).map_err(|source| CassetteError::CreateDir {
                path: parent.to_path_buf(),
                source,
            })?;
        }
        let body = serde_json::to_string_pretty(self).map_err(CassetteError::Serialize)?;
        fs::write(path, format!("{body}\n")).map_err(|source| CassetteError::Write {
            path: path.to_path_buf(),
            source,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct OpenAiHttpCassette {
    pub version: u32,
    pub interactions: Vec<OpenAiHttpInteraction>,
}

impl OpenAiHttpCassette {
    pub fn new(interactions: Vec<OpenAiHttpInteraction>) -> Self {
        Self {
            version: CASSETTE_VERSION,
            interactions,
        }
    }

    pub fn read_from(path: &Path) -> Result<Self, CassetteError> {
        let body = fs::read_to_string(path).map_err(|source| CassetteError::Read {
            path: path.to_path_buf(),
            source,
        })?;
        let cassette: Self =
            serde_json::from_str(&body).map_err(|source| CassetteError::Parse {
                path: path.to_path_buf(),
                source,
            })?;
        if cassette.version != CASSETTE_VERSION {
            return Err(CassetteError::UnsupportedVersion {
                path: path.to_path_buf(),
                version: cassette.version,
            });
        }
        Ok(cassette)
    }

    pub fn write_to(&self, path: &Path) -> Result<(), CassetteError> {
        assert_http_cassette_is_safe(self)?;
        if let Some(parent) = path
            .parent()
            .filter(|parent| !parent.as_os_str().is_empty())
        {
            fs::create_dir_all(parent).map_err(|source| CassetteError::CreateDir {
                path: parent.to_path_buf(),
                source,
            })?;
        }
        let body = serde_json::to_string_pretty(self).map_err(CassetteError::Serialize)?;
        fs::write(path, format!("{body}\n")).map_err(|source| CassetteError::Write {
            path: path.to_path_buf(),
            source,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct OpenAiHttpInteraction {
    pub request: OpenAiHttpRecordedRequest,
    pub response: OpenAiHttpRecordedResponse,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct OpenAiHttpRecordedRequest {
    pub endpoint_path: String,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub headers: BTreeMap<String, String>,
    pub body: serde_json::Value,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct OpenAiHttpRecordedResponse {
    pub status: u16,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub headers: BTreeMap<String, String>,
    pub body: String,
}

#[derive(Debug, Error)]
pub enum CassetteError {
    #[error("failed to read cassette {path}: {source}")]
    Read {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("failed to parse cassette {path}: {source}")]
    Parse {
        path: PathBuf,
        #[source]
        source: serde_json::Error,
    },
    #[error("unsupported cassette version {version} in {path}")]
    UnsupportedVersion { path: PathBuf, version: u32 },
    #[error("missing cassette {path} in replay mode")]
    MissingReplayCassette { path: PathBuf },
    #[error("failed to serialize cassette: {0}")]
    Serialize(#[source] serde_json::Error),
    #[error("failed to create cassette directory {path}: {source}")]
    CreateDir {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("failed to write cassette {path}: {source}")]
    Write {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("cassette request mismatch at interaction {index}: expected {expected}, got {actual}")]
    RequestMismatch {
        index: usize,
        expected: String,
        actual: String,
    },
    #[error("cassette exhausted after {count} interaction(s)")]
    Exhausted { count: usize },
    #[error("unsafe cassette secret detected: {kind}")]
    UnsafeSecret { kind: String },
}

pub fn recorded_headers_to_header_map(
    headers: &BTreeMap<String, String>,
) -> Result<HeaderMap, String> {
    let mut parsed = HeaderMap::new();
    for (name, value) in headers {
        let name = HeaderName::from_bytes(name.as_bytes())
            .map_err(|err| format!("invalid recorded header name `{name}`: {err}"))?;
        let value = HeaderValue::from_str(value)
            .map_err(|err| format!("invalid recorded header value for `{name}`: {err}"))?;
        parsed.insert(name, value);
    }
    Ok(parsed)
}
