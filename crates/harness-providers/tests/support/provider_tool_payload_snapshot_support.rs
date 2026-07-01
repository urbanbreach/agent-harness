use std::collections::BTreeMap;
use std::error::Error;
use std::path::{Path, PathBuf};

use harness_core::agent::{build_provider_tool_defs, AgentProfile};
use harness_core::config::{McpConfig, McpServerConfig, ShellAllowlist, ToolFailureMode};
use harness_providers::mock::request_digest;
use harness_providers::openai::OpenAiApiMode;
use harness_providers::{
    CacheRetention, CompletionMessage, CompletionRequest, MessageRole, ToolChoice, ToolDef,
};
use harness_tools::{coordinator_registry, coordinator_registry_with_mcp};
use serde::{Deserialize, Serialize};

#[path = "openai_support.rs"]
mod openai;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct Snapshot {
    pub(crate) version: u32,
    pub(crate) profiles: Vec<ProfileSnapshot>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct ProfileSnapshot {
    pub(crate) profile: String,
    pub(crate) category: String,
    pub(crate) completion_request_digest: String,
    pub(crate) tools: Vec<ToolSnapshot>,
    pub(crate) openai: Vec<OpenAiRequestSnapshot>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct ToolSnapshot {
    pub(crate) canonical_id: String,
    pub(crate) provider_function_name: String,
    pub(crate) description_digest: String,
    pub(crate) parameters_digest: String,
    pub(crate) parameters_type: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct OpenAiRequestSnapshot {
    pub(crate) api_mode: String,
    pub(crate) endpoint_path: String,
    pub(crate) bearer_token: String,
    pub(crate) request_body_digest: String,
    pub(crate) tool_function_names: Vec<String>,
}

pub(crate) async fn generate_snapshot() -> Result<Snapshot, Box<dyn Error>> {
    let native_registry = coordinator_registry(ShellAllowlist::default());
    let mcp_registry = coordinator_registry_with_mcp(ShellAllowlist::default(), mcp_config());
    let mut profiles = Vec::new();

    for spec in profile_specs() {
        let registry = if spec.name == "mcp" {
            &mcp_registry
        } else {
            &native_registry
        };
        let profile = profile(spec.name, spec.category, spec.toolset);
        let tools = build_provider_tool_defs(&profile, registry)?;
        let request = completion_request(&profile, tools.clone());
        profiles.push(ProfileSnapshot {
            profile: profile.name.clone(),
            category: profile.category.clone(),
            completion_request_digest: request_digest(&request),
            tools: summarize_tools(&tools),
            openai: vec![
                openai::capture_openai_request(OpenAiApiMode::ChatCompletions, request.clone())
                    .await?,
                openai::capture_openai_request(OpenAiApiMode::Responses, request).await?,
            ],
        });
    }

    Ok(Snapshot {
        version: 1,
        profiles,
    })
}

fn summarize_tools(tools: &[ToolDef]) -> Vec<ToolSnapshot> {
    tools
        .iter()
        .map(|tool| ToolSnapshot {
            canonical_id: tool.tool_id.clone(),
            provider_function_name: tool.function_name.clone(),
            description_digest: digest_str(tool.description.as_deref().unwrap_or_default()),
            parameters_digest: digest_value(&tool.parameters),
            parameters_type: tool.parameters["type"]
                .as_str()
                .unwrap_or("missing")
                .to_string(),
        })
        .collect()
}

struct ProfileSpec {
    name: &'static str,
    category: &'static str,
    toolset: Vec<&'static str>,
}

fn profile_specs() -> Vec<ProfileSpec> {
    vec![
        ProfileSpec {
            name: "build",
            category: "build",
            toolset: vec![
                "read",
                "edit",
                "write",
                "apply_patch",
                "bash",
                "shell.run",
                "github.issue",
                "lsp.rename",
            ],
        },
        ProfileSpec {
            name: "plan",
            category: "plan",
            toolset: vec!["read", "glob", "grep", "list", "question", "plan_exit"],
        },
        ProfileSpec {
            name: "category",
            category: "quick",
            toolset: vec!["task", "background_output", "todowrite", "skill"],
        },
        ProfileSpec {
            name: "mcp",
            category: "mcp",
            toolset: vec!["mcp.docs.rs.tools.list", "mcp.docs.rs.tool.call"],
        },
    ]
}

fn profile(name: &str, category: &str, toolset: Vec<&str>) -> AgentProfile {
    AgentProfile {
        name: name.to_string(),
        category: category.to_string(),
        model_ref: "openai/gpt-5.5".to_string(),
        model_ref_explicit: true,
        system_prompt: format!("{name} profile snapshot"),
        temperature: Some(0.0),
        cache_retention: CacheRetention::Short,
        max_iters: Some(3),
        tool_failure_mode: ToolFailureMode::FailTurn,
        toolset: toolset.into_iter().map(str::to_string).collect(),
    }
}

fn completion_request(profile: &AgentProfile, tools: Vec<ToolDef>) -> CompletionRequest {
    CompletionRequest {
        provider_id: None,
        model_id: profile.model_ref.clone(),
        messages: vec![CompletionMessage {
            role: MessageRole::User,
            content: format!("snapshot provider tool payloads for {}", profile.name),
            name: None,
            tool_call_id: None,
            assistant_tool_calls: None,
        }],
        temperature: profile.temperature,
        max_tokens: None,
        variant: None,
        reasoning_effort: None,
        text_verbosity: None,
        reasoning_summary: None,
        thinking: None,
        tools: Some(tools),
        tool_choice: Some(ToolChoice::Auto),
        context: Default::default(),
        stream: true,
    }
}

fn mcp_config() -> McpConfig {
    let mut servers = BTreeMap::new();
    servers.insert(
        "docs.rs".to_string(),
        McpServerConfig::Stdio {
            command: vec!["false".to_string()],
            env: BTreeMap::new(),
            cwd: None,
            timeout_secs: 1,
            enabled: true,
        },
    );
    McpConfig { servers }
}

pub(super) fn digest_value(value: &serde_json::Value) -> String {
    let bytes = serde_json::to_vec(value).unwrap_or_else(|_| b"null".to_vec());
    blake3::hash(&bytes).to_hex().to_string()
}

fn digest_str(value: &str) -> String {
    blake3::hash(value.as_bytes()).to_hex().to_string()
}

pub(crate) fn fixture_path(relative: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(relative)
}
