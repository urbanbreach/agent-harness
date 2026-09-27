use schemars::JsonSchema;
use serde::Deserialize;
use std::path::PathBuf;

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(super) struct List {
    #[serde(default, rename = "sessionRoot", alias = "session_root")]
    pub session_root: Option<PathBuf>,
    #[serde(default)]
    pub status: Option<String>,
    #[serde(default)]
    pub profile: Option<String>,
    #[serde(default)]
    pub resumable: Option<bool>,
    #[serde(default)]
    pub filter: Option<String>,
    #[serde(default)]
    pub sort: Option<String>,
    #[serde(default)]
    pub limit: Option<usize>,
}
#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(super) struct Read {
    #[serde(alias = "run_id", alias = "path")]
    pub session: String,
    #[serde(default, rename = "sessionRoot", alias = "session_root")]
    pub session_root: Option<PathBuf>,
    #[serde(default, rename = "eventOffset", alias = "event_offset")]
    pub event_offset: usize,
    #[serde(default, rename = "eventLimit", alias = "event_limit")]
    pub event_limit: Option<usize>,
    #[serde(default, rename = "messageOffset", alias = "message_offset")]
    pub message_offset: usize,
    #[serde(default, rename = "messageLimit", alias = "message_limit")]
    pub message_limit: Option<usize>,
    #[serde(default, rename = "includeTodos", alias = "include_todos")]
    pub include_todos: bool,
    #[serde(default, rename = "fromEnd", alias = "from_end")]
    pub from_end: bool,
}
#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(super) struct Search {
    pub query: String,
    #[serde(default, alias = "run_id")]
    pub session: Option<String>,
    #[serde(default, rename = "sessionRoot", alias = "session_root")]
    pub session_root: Option<PathBuf>,
    #[serde(default, rename = "caseSensitive", alias = "case_sensitive")]
    pub case_sensitive: bool,
    #[serde(default)]
    pub limit: Option<usize>,
    #[serde(default, rename = "contextLimit", alias = "context_limit")]
    pub context_limit: Option<usize>,
}
#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(super) struct Info {
    #[serde(alias = "run_id", alias = "path")]
    pub session: String,
    #[serde(default, rename = "sessionRoot", alias = "session_root")]
    pub session_root: Option<PathBuf>,
}
