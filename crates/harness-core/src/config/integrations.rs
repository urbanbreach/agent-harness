use std::{collections::BTreeMap, path::PathBuf};

use schemars::JsonSchema;
use serde::{Deserialize, Deserializer, Serialize};

use super::defaults::{
    default_mcp_enabled, default_mcp_timeout_secs, default_remote_search_endpoint,
    default_remote_search_max_retries, default_remote_search_retry_backoff_ms,
    default_remote_search_timeout_secs,
};

/// Current public integration settings.
///
/// Agent Harness exposes the native remote search transport used by the built-in
/// `web_search` and `code_search` tools, plus configured MCP servers.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, Default)]
#[serde(deny_unknown_fields)]
pub struct IntegrationsConfig {
    /// Configuration for the currently supported built-in external integrations.
    ///
    /// Agent Harness exposes the native remote search transport used by the
    /// built-in `web_search` and `code_search` tools, plus configured MCP
    /// servers.
    #[serde(default, alias = "remoteSearch")]
    pub remote_search: RemoteSearchConfig,
    #[serde(default)]
    pub mcp: McpConfig,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum McpServerConnectionState {
    Connected,
    Failed(String),
}

/// Settings for the built-in remote search bridge.
///
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RemoteSearchConfig {
    /// Endpoint used by the built-in remote search bridge.
    /// Exa-compatible MCP endpoint for native `web_search` and `code_search`.
    ///
    #[serde(default = "default_remote_search_endpoint")]
    pub endpoint: String,
    /// Optional bearer token for the remote search endpoint.
    #[serde(default, alias = "authToken")]
    pub auth_token: Option<String>,
    /// Require an auth token before the native search tools make requests.
    #[serde(default, alias = "requireAuth")]
    pub require_auth: bool,
    /// Request timeout for native remote search calls.
    #[serde(default = "default_remote_search_timeout_secs", alias = "timeoutSecs")]
    pub timeout_secs: u64,
    /// Maximum retry attempts for retryable remote search failures.
    #[serde(default = "default_remote_search_max_retries", alias = "maxRetries")]
    pub max_retries: u32,
    /// Backoff, in milliseconds, between retry attempts.
    #[serde(
        default = "default_remote_search_retry_backoff_ms",
        alias = "retryBackoffMs"
    )]
    pub retry_backoff_ms: u64,
}

impl Default for RemoteSearchConfig {
    fn default() -> Self {
        Self {
            endpoint: default_remote_search_endpoint(),
            auth_token: None,
            require_auth: false,
            timeout_secs: default_remote_search_timeout_secs(),
            max_retries: default_remote_search_max_retries(),
            retry_backoff_ms: default_remote_search_retry_backoff_ms(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, Default)]
#[serde(deny_unknown_fields)]
pub struct McpConfig {
    #[serde(default)]
    pub servers: BTreeMap<String, McpServerConfig>,
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
#[serde(tag = "transport", rename_all = "snake_case", deny_unknown_fields)]
pub enum McpServerConfig {
    Stdio {
        command: Vec<String>,
        #[serde(default, alias = "environment")]
        env: BTreeMap<String, String>,
        #[serde(default)]
        cwd: Option<PathBuf>,
        #[serde(
            default = "default_mcp_timeout_secs",
            alias = "timeoutSecs",
            alias = "timeout"
        )]
        timeout_secs: u64,
        #[serde(default = "default_mcp_enabled")]
        enabled: bool,
    },
    #[serde(alias = "streamable_http")]
    Http {
        #[serde(alias = "url")]
        endpoint: String,
        #[serde(default)]
        headers: BTreeMap<String, String>,
        #[serde(
            default = "default_mcp_timeout_secs",
            alias = "timeoutSecs",
            alias = "timeout"
        )]
        timeout_secs: u64,
        #[serde(default = "default_mcp_enabled")]
        enabled: bool,
    },
}

impl McpServerConfig {
    pub fn timeout_secs(&self) -> u64 {
        match self {
            Self::Stdio { timeout_secs, .. } | Self::Http { timeout_secs, .. } => *timeout_secs,
        }
    }

    pub fn enabled(&self) -> bool {
        match self {
            Self::Stdio { enabled, .. } | Self::Http { enabled, .. } => *enabled,
        }
    }
}

impl<'de> Deserialize<'de> for McpServerConfig {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let raw = HarnessTaggedMcpServerConfig::deserialize(deserializer)?;
        Ok(raw.into_runtime())
    }
}

#[derive(Debug, Deserialize)]
#[serde(tag = "transport", rename_all = "snake_case", deny_unknown_fields)]
enum HarnessTaggedMcpServerConfig {
    Stdio {
        command: Vec<String>,
        #[serde(default, alias = "environment")]
        env: BTreeMap<String, String>,
        #[serde(default)]
        cwd: Option<PathBuf>,
        #[serde(
            default = "default_mcp_timeout_secs",
            alias = "timeoutSecs",
            alias = "timeout"
        )]
        timeout_secs: u64,
        #[serde(default = "default_mcp_enabled")]
        enabled: bool,
    },
    #[serde(alias = "streamable_http")]
    Http {
        #[serde(alias = "url")]
        endpoint: String,
        #[serde(default)]
        headers: BTreeMap<String, String>,
        #[serde(
            default = "default_mcp_timeout_secs",
            alias = "timeoutSecs",
            alias = "timeout"
        )]
        timeout_secs: u64,
        #[serde(default = "default_mcp_enabled")]
        enabled: bool,
    },
}

impl HarnessTaggedMcpServerConfig {
    fn into_runtime(self) -> McpServerConfig {
        match self {
            Self::Stdio {
                command,
                env,
                cwd,
                timeout_secs,
                enabled,
            } => McpServerConfig::Stdio {
                command,
                env,
                cwd,
                timeout_secs,
                enabled,
            },
            Self::Http {
                endpoint,
                headers,
                timeout_secs,
                enabled,
            } => McpServerConfig::Http {
                endpoint,
                headers,
                timeout_secs,
                enabled,
            },
        }
    }
}
