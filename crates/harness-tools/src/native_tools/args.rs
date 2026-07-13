// allow: SIZE_OK — native tool argument parsing (websearch limits + read/grep/glob params + batch args)
use crate::fs_grep::GrepOutputMode;
use crate::network::WebFetchFormat;
use crate::read_window::READ_DEFAULT_LIMIT;
use schemars::JsonSchema;
use serde::de::Error as _;
use serde::Deserialize;
use serde_json::{json, Value};

const WEBSEARCH_MAX_NUM_RESULTS: u32 = 20;
const WEBSEARCH_MAX_CONTEXT_CHARACTERS: u32 = 50_000;

mod control;
pub(super) use control::{
    BackgroundCancelArgs, BackgroundOutputArgs, BatchArgs, InvalidArgs, QuestionArgs, SkillArgs,
    TaskArgs, TodoWriteArgs,
};

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(super) struct ReadArgs {
    #[serde(rename = "path", alias = "filePath")]
    pub(super) file_path: String,
    #[serde(default)]
    pub(super) offset: Option<u32>,
    #[serde(default)]
    pub(super) limit: Option<u32>,
    #[serde(default, rename = "hashlineAnchors", alias = "hashline_anchors")]
    pub(super) hashline_anchors: Option<bool>,
}

pub(super) fn read_parameters_json_schema(default_hashline_anchors: bool) -> Value {
    let _ = default_hashline_anchors;
    json!({
        "type": "object",
            "properties": {
            "path": {
                "type": "string",
                "description": "The path to the file or directory to read"
            },
            "offset": {
                "type": "integer",
                "minimum": 1,
                "description": "The line number to start reading from (1-indexed)"
            },
            "limit": {
                "type": "integer",
                "minimum": 1,
                "description": format!("The maximum number of lines to read (defaults to {READ_DEFAULT_LIMIT})")
            }
        },
        "required": ["path"],
        "additionalProperties": false
    })
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(super) struct ListArgs {
    #[schemars(
        description = "Directory path to list. Defaults to the workspace root when omitted."
    )]
    #[serde(default)]
    pub(super) path: Option<String>,
    #[schemars(
        description = "Optional glob-style path fragments to omit from the recursive listing."
    )]
    #[serde(default)]
    pub(super) ignore: Option<Vec<String>>,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(super) struct GlobArgs {
    #[schemars(description = "Glob pattern to match, such as `**/*.rs` or `crates/**/*.toml`.")]
    pub(super) pattern: String,
    #[schemars(description = "Directory to search from. Defaults to the current workspace root.")]
    #[serde(default)]
    pub(super) path: Option<String>,
    #[schemars(description = "Maximum results to return.")]
    #[serde(default)]
    pub(super) limit: Option<u32>,
}

pub(super) fn glob_parameters_json_schema() -> Value {
    json!({
        "type": "object",
        "additionalProperties": false,
        "required": ["pattern"],
        "properties": {
            "pattern": {
                "type": "string",
                "description": "The glob pattern to match files against"
            },
            "path": {
                "type": "string",
                "description": "The directory to search in. If not specified, the current working directory will be used. IMPORTANT: Omit this field to use the default directory. DO NOT enter \"undefined\" or \"null\" - simply omit it for the default behavior. Must be a valid directory path if provided."
            },
            "limit": {
                "type": "integer",
                "minimum": 1,
                "description": "Maximum number of matches to return"
            }
        }
    })
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(super) struct GrepArgs {
    #[schemars(description = "Rust regular expression to search for unless `literal` is true.")]
    pub(super) pattern: String,
    #[schemars(description = "File or directory to search. Defaults to the workspace root.")]
    #[serde(default)]
    pub(super) path: Option<String>,
    #[schemars(description = "Optional file glob filter such as `*.rs` or `**/*.md`.")]
    #[serde(default)]
    pub(super) include: Option<String>,
    #[schemars(description = "Maximum matches to return.")]
    #[serde(default)]
    pub(super) limit: Option<u32>,
    #[schemars(description = "When true, search for `pattern` as plain text instead of a regex.")]
    #[serde(default)]
    pub(super) literal: bool,
    #[schemars(
        description = "Output mode: `content` returns matched lines, `files_with_matches` returns file paths, `count` returns per-file match counts."
    )]
    #[serde(default)]
    pub(super) output_mode: GrepOutputMode,
    #[schemars(
        description = "Maximum number of files to return in `files_with_matches` or `count` mode; in `content` mode limits files whose matches are returned."
    )]
    #[serde(default)]
    pub(super) head_limit: Option<u32>,
}

pub(super) fn grep_parameters_json_schema() -> Value {
    json!({
        "type": "object",
        "additionalProperties": false,
        "required": ["pattern"],
        "properties": {
            "pattern": {
                "type": "string",
                "description": "The regex pattern to search for in file contents"
            },
            "path": {
                "type": "string",
                "description": "The directory to search in. Defaults to the current working directory."
            },
            "include": {
                "type": "string",
                "description": "File pattern to include in the search (e.g. \"*.js\", \"*.{ts,tsx}\")"
            },
            "limit": {
                "type": "integer",
                "minimum": 1,
                "description": "Maximum number of matches to return"
            },
            "literal": {
                "type": "boolean",
                "description": "When true, search for pattern as plain text instead of a regex"
            },
            "output_mode": {
                "type": "string",
                "enum": ["content", "files_with_matches", "count"],
                "description": "Output mode: content returns matched lines, files_with_matches returns file paths, count returns per-file match counts",
                "default": "content"
            },
            "head_limit": {
                "type": "integer",
                "minimum": 0,
                "description": "Maximum number of files to return in files_with_matches or count mode; in content mode limits files whose matches are returned"
            }
        }
    })
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(super) struct BashArgs {
    #[schemars(
        description = "Shell command to execute. Native read/glob/grep/list/edit tools are preferred for file IO, search, and edits; shell shortcuts are controlled by permission patterns and workspace path safety."
    )]
    pub(super) command: String,
    #[schemars(
        description = "Optional command timeout in milliseconds. Defaults to the shell tool timeout."
    )]
    #[serde(default)]
    pub(super) timeout: Option<u64>,
    #[schemars(
        description = "Optional working directory for the command. Must stay inside the workspace."
    )]
    #[serde(default)]
    pub(super) workdir: Option<String>,
    #[schemars(
        description = "Short human-readable reason for running the command, used in permission prompts and summaries."
    )]
    #[serde(default)]
    pub(super) description: Option<String>,
}

pub(super) fn bash_parameters_json_schema() -> Value {
    json!({
        "type": "object",
        "additionalProperties": false,
        "required": ["command"],
        "properties": {
            "command": {
                "type": "string",
                "description": "Shell command string to execute"
            },
            "workdir": {
                "type": "string",
                "description": "Working directory. Defaults to the active workspace; relative paths resolve from that workspace."
            },
            "timeout": {
                "type": "integer",
                "minimum": 1,
                "description": "Timeout in milliseconds."
            }
        }
    })
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(super) struct WebFetchArgs {
    #[schemars(description = "Fully qualified URL to fetch, including https:// when known.")]
    pub(super) url: String,
    #[schemars(description = "Response format to return to the model: markdown, text, or html.")]
    #[serde(default = "default_webfetch_format")]
    pub(super) format: WebFetchFormat,
    #[schemars(description = "Optional network timeout in seconds for this fetch.")]
    #[serde(default)]
    pub(super) timeout: Option<u64>,
}

fn default_webfetch_format() -> WebFetchFormat {
    WebFetchFormat::Markdown
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct WebSearchArgs {
    pub(super) query: String,
    #[serde(default, deserialize_with = "deserialize_websearch_num_results")]
    #[serde(rename = "numResults", alias = "num_results")]
    pub(super) num_results: Option<u32>,
    #[serde(default)]
    pub(super) livecrawl: Option<WebSearchLivecrawl>,
    #[serde(default)]
    pub(super) r#type: Option<WebSearchType>,
    #[serde(
        default,
        deserialize_with = "deserialize_websearch_context_max_characters"
    )]
    #[serde(rename = "contextMaxCharacters", alias = "context_max_characters")]
    pub(super) context_max_characters: Option<u32>,
}

pub(super) fn web_search_parameters_json_schema() -> Value {
    json!({
        "type": "object",
        "additionalProperties": false,
        "required": ["query"],
        "properties": {
            "query": {
                "type": "string",
                "description": "Natural-language web search query describing the page or fact needed."
            },
            "numResults": {
                "type": "integer",
                "minimum": 1,
                "maximum": WEBSEARCH_MAX_NUM_RESULTS,
                "description": format!("Number of search results to return (default: 8, maximum: {WEBSEARCH_MAX_NUM_RESULTS}).")
            },
            "livecrawl": {
                "type": "string",
                "enum": ["fallback", "preferred"],
                "description": "Live crawl mode: fallback uses live crawling as backup; preferred prioritizes live crawling."
            },
            "type": {
                "type": "string",
                "enum": ["auto", "fast", "deep"],
                "description": "Search type: auto balances search, fast returns quick results, deep is more comprehensive."
            },
            "contextMaxCharacters": {
                "type": "integer",
                "minimum": 1,
                "maximum": WEBSEARCH_MAX_CONTEXT_CHARACTERS,
                "description": format!("Maximum characters for model-optimized context (default: 10000, maximum: {WEBSEARCH_MAX_CONTEXT_CHARACTERS}).")
            }
        }
    })
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "lowercase")]
pub(super) enum WebSearchLivecrawl {
    Fallback,
    Preferred,
}

impl WebSearchLivecrawl {
    pub(super) const fn as_str(&self) -> &'static str {
        match self {
            Self::Fallback => "fallback",
            Self::Preferred => "preferred",
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "lowercase")]
pub(super) enum WebSearchType {
    Auto,
    Fast,
    Deep,
}

impl WebSearchType {
    pub(super) const fn as_str(&self) -> &'static str {
        match self {
            Self::Auto => "auto",
            Self::Fast => "fast",
            Self::Deep => "deep",
        }
    }
}

fn deserialize_websearch_num_results<'de, D>(deserializer: D) -> Result<Option<u32>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let value = Option::<u32>::deserialize(deserializer)?;
    if value.is_some_and(|value| !(1..=WEBSEARCH_MAX_NUM_RESULTS).contains(&value)) {
        return Err(D::Error::custom(format!(
            "numResults must be between 1 and {WEBSEARCH_MAX_NUM_RESULTS}"
        )));
    }
    Ok(value)
}

fn deserialize_websearch_context_max_characters<'de, D>(
    deserializer: D,
) -> Result<Option<u32>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let value = Option::<u32>::deserialize(deserializer)?;
    if value.is_some_and(|value| !(1..=WEBSEARCH_MAX_CONTEXT_CHARACTERS).contains(&value)) {
        return Err(D::Error::custom(format!(
            "contextMaxCharacters must be between 1 and {WEBSEARCH_MAX_CONTEXT_CHARACTERS}"
        )));
    }
    Ok(value)
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(super) struct CodeSearchArgs {
    #[schemars(
        description = "Remote/public code-search query or code pattern to send to the configured backend; not local workspace symbol lookup. Use grep, ast_grep_search, or lsp for local code."
    )]
    pub(super) query: String,
    #[schemars(
        description = "Optional maximum token budget requested from the code search backend."
    )]
    #[serde(default)]
    #[serde(rename = "tokensNum", alias = "tokens_num")]
    pub(super) tokens_num: Option<u32>,
}
