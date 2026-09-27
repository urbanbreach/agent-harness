use super::*;

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, Debug, Default)]
#[serde(default, deny_unknown_fields)]
pub struct IntegrationsConfig {
    #[serde(alias = "remoteSearch")]
    pub remote_search: RemoteSearchConfig,
    pub mcp: McpConfig,
}

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, Debug)]
pub enum McpServerConnectionState {
    Connected,
    Failed(String),
}

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, Debug)]
#[serde(default, deny_unknown_fields)]
pub struct RemoteSearchConfig {
    pub endpoint: String,
    #[serde(alias = "authToken")]
    pub auth_token: Option<String>,
    #[serde(alias = "requireAuth")]
    pub require_auth: bool,
    #[serde(alias = "timeoutSecs")]
    pub timeout_secs: u64,
    #[serde(alias = "maxRetries")]
    pub max_retries: u32,
    #[serde(alias = "retryBackoffMs")]
    pub retry_backoff_ms: u64,
}

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, Debug, Default)]
#[serde(default, deny_unknown_fields)]
pub struct McpConfig {
    pub servers: BTreeMap<String, McpServerConfig>,
}

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, Debug)]
#[serde(tag = "transport", rename_all = "snake_case", deny_unknown_fields)]
pub enum McpServerConfig {
    Stdio {
        command: Vec<String>,
        #[serde(default, alias = "environment")]
        env: BTreeMap<String, String>,
        #[serde(default)]
        cwd: Option<PathBuf>,
        #[serde(default = "mcp_timeout", alias = "timeoutSecs", alias = "timeout")]
        timeout_secs: u64,
        #[serde(default = "yes")]
        enabled: bool,
    },
    #[serde(alias = "streamable_http")]
    Http {
        #[serde(alias = "url")]
        endpoint: String,
        #[serde(default)]
        headers: BTreeMap<String, String>,
        #[serde(default = "mcp_timeout", alias = "timeoutSecs", alias = "timeout")]
        timeout_secs: u64,
        #[serde(default = "yes")]
        enabled: bool,
    },
}

impl McpServerConfig {
    pub const fn enabled(&self) -> bool {
        match self {
            Self::Stdio { enabled, .. } | Self::Http { enabled, .. } => *enabled,
        }
    }
}
