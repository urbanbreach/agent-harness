use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use clap::ValueEnum;
use harness_core::agent::AgentProfile;
use harness_core::config::ShellAllowlist;
use harness_core::edit::hashline::{compute_line_hash, HashlineOp, HashlinePatch, LineAnchor};
use harness_core::event::{ActorKind, EventActor};
use harness_core::perm::PermissionPolicy;
use harness_core::tool::build_tool_function_name_mapping;
use harness_providers::mock::{request_digest, MockProvider};
use harness_providers::{
    CompletionMessage, CompletionRequest, CompletionUsage, MessageRole, ProviderStreamEvent,
    ToolChoice, ToolDef,
};
use harness_tools::coordinator_registry;
use serde_json::{json, Value};
use uuid::Uuid;

static WORKSPACE_COUNTER: AtomicU64 = AtomicU64::new(1);

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum ScenarioName {
    #[value(name = "golden_path")]
    GoldenPath,
    #[value(name = "golden_path_interactive")]
    GoldenPathInteractive,
}

impl ScenarioName {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::GoldenPath => "golden_path",
            Self::GoldenPathInteractive => "golden_path_interactive",
        }
    }

    pub fn interactive_permissions(self) -> bool {
        matches!(self, Self::GoldenPathInteractive)
    }
}

pub fn deterministic_run_id(seed: u64, scenario: ScenarioName) -> String {
    let namespace = Uuid::new_v5(
        &Uuid::NAMESPACE_OID,
        format!("harness-seed:{seed}").as_bytes(),
    );
    let run_uuid = Uuid::new_v5(&namespace, scenario.as_str().as_bytes());
    format!("run_{}", run_uuid.simple())
}

pub fn create_workspace(
    session_dir: &Path,
    scenario: ScenarioName,
    deterministic_run_id: Option<&str>,
) -> Result<PathBuf, String> {
    let workspace_dir = if let Some(run_id) = deterministic_run_id {
        session_dir
            .join("workspaces")
            .join(format!("{}-{run_id}", scenario.as_str()))
    } else {
        let id = WORKSPACE_COUNTER.fetch_add(1, Ordering::SeqCst);
        session_dir
            .join("workspaces")
            .join(format!("{}-{id:06}", scenario.as_str()))
    };

    if workspace_dir.exists() {
        fs::remove_dir_all(&workspace_dir).map_err(|err| {
            format!(
                "failed to clear workspace {}: {err}",
                workspace_dir.display()
            )
        })?;
    }

    fs::create_dir_all(&workspace_dir).map_err(|err| {
        format!(
            "failed to create workspace {}: {err}",
            workspace_dir.display()
        )
    })?;
    fs::write(workspace_dir.join("demo.txt"), "alpha\nbeta\ngamma\n")
        .map_err(|err| format!("failed to seed demo.txt: {err}"))?;

    Ok(workspace_dir)
}

pub fn golden_path_patch() -> HashlinePatch {
    let source = "alpha\nbeta\ngamma\n";
    let source_lines = source
        .trim_end_matches('\n')
        .split('\n')
        .collect::<Vec<_>>();
    let anchor = LineAnchor {
        line: 2,
        hash: compute_line_hash(source_lines[1]),
    };

    HashlinePatch {
        edit_id: "edit-golden-path".to_string(),
        path: "demo.txt".to_string(),
        ops: vec![HashlineOp::Replace {
            expected: vec![anchor],
            lines: vec!["BETA".to_string()],
        }],
    }
}

pub fn golden_path_edit_args() -> Value {
    let patch = golden_path_patch();
    json!({
        "editId": patch.edit_id,
        "filePath": patch.path,
        "edits": [
            {
                "op": "replace",
                "pos": format!("2#{}", compute_line_hash("beta")),
                "lines": ["BETA"],
            }
        ],
    })
}

pub fn golden_path_provider() -> MockProvider {
    let mut scripted_events = BTreeMap::new();

    for prompt in ["planner-prompt", "worker-prompt"] {
        let request = CompletionRequest {
            provider_id: Some("mock".to_string()),
            model_id: "model-1".to_string(),
            messages: vec![
                CompletionMessage {
                    role: MessageRole::System,
                    content: prompt.to_string(),
                    name: None,
                    tool_call_id: None,
                    assistant_tool_calls: None,
                },
                CompletionMessage {
                    role: MessageRole::User,
                    content: prompt.to_string(),
                    name: None,
                    tool_call_id: None,
                    assistant_tool_calls: None,
                },
            ],
            temperature: Some(0.0),
            max_tokens: None,
            variant: None,
            reasoning_effort: None,
            text_verbosity: None,
            reasoning_summary: None,
            tools: None,
            tool_choice: None,
            context: Default::default(),
            stream: true,
        };

        scripted_events.insert(
            request_digest(&request),
            vec![
                ProviderStreamEvent::Start,
                ProviderStreamEvent::TextDelta(format!("{prompt}-delta")),
                ProviderStreamEvent::Done {
                    usage: CompletionUsage {
                        prompt_tokens: 2,
                        completion_tokens: 1,
                        total_tokens: 3,
                    },
                },
            ],
        );

        if prompt == "worker-prompt" {
            let worker_request_with_tools = CompletionRequest {
                provider_id: request.provider_id.clone(),
                model_id: request.model_id.clone(),
                messages: request.messages.clone(),
                temperature: request.temperature,
                max_tokens: request.max_tokens,
                variant: request.variant.clone(),
                reasoning_effort: request.reasoning_effort.clone(),
                text_verbosity: request.text_verbosity.clone(),
                reasoning_summary: request.reasoning_summary.clone(),
                tools: Some(vec![demo_edit_tool_def()]),
                tool_choice: Some(ToolChoice::Auto),
                context: Default::default(),
                stream: request.stream,
            };

            scripted_events.insert(
                request_digest(&worker_request_with_tools),
                vec![
                    ProviderStreamEvent::Start,
                    ProviderStreamEvent::TextDelta(format!("{prompt}-delta")),
                    ProviderStreamEvent::Done {
                        usage: CompletionUsage {
                            prompt_tokens: 2,
                            completion_tokens: 1,
                            total_tokens: 3,
                        },
                    },
                ],
            );
        }
    }

    let interactive_request = CompletionRequest {
        provider_id: Some("mock".to_string()),
        model_id: "model-1".to_string(),
        messages: vec![
            CompletionMessage {
                role: MessageRole::System,
                content: "worker-prompt".to_string(),
                name: None,
                tool_call_id: None,
                assistant_tool_calls: None,
            },
            CompletionMessage {
                role: MessageRole::User,
                content: "Hello from PTY".to_string(),
                name: None,
                tool_call_id: None,
                assistant_tool_calls: None,
            },
        ],
        temperature: Some(0.0),
        max_tokens: None,
        variant: None,
        reasoning_effort: None,
        text_verbosity: None,
        reasoning_summary: None,
        tools: None,
        tool_choice: None,
        context: Default::default(),
        stream: true,
    };

    scripted_events.insert(
        request_digest(&interactive_request),
        vec![
            ProviderStreamEvent::Start,
            ProviderStreamEvent::TextDelta("Hello".to_string()),
            ProviderStreamEvent::TextDelta(" world".to_string()),
            ProviderStreamEvent::Done {
                usage: CompletionUsage {
                    prompt_tokens: 4,
                    completion_tokens: 2,
                    total_tokens: 6,
                },
            },
        ],
    );

    let interactive_request_with_tools = CompletionRequest {
        provider_id: Some("mock".to_string()),
        model_id: "model-1".to_string(),
        messages: interactive_request.messages.clone(),
        temperature: interactive_request.temperature,
        max_tokens: interactive_request.max_tokens,
        variant: interactive_request.variant.clone(),
        reasoning_effort: interactive_request.reasoning_effort.clone(),
        text_verbosity: interactive_request.text_verbosity.clone(),
        reasoning_summary: interactive_request.reasoning_summary.clone(),
        tools: Some(vec![demo_edit_tool_def()]),
        tool_choice: Some(ToolChoice::Auto),
        context: Default::default(),
        stream: interactive_request.stream,
    };

    scripted_events.insert(
        request_digest(&interactive_request_with_tools),
        vec![
            ProviderStreamEvent::Start,
            ProviderStreamEvent::TextDelta("Hello".to_string()),
            ProviderStreamEvent::TextDelta(" world".to_string()),
            ProviderStreamEvent::Done {
                usage: CompletionUsage {
                    prompt_tokens: 4,
                    completion_tokens: 2,
                    total_tokens: 6,
                },
            },
        ],
    );

    for prompt_text in ["hello", "hi", "pipe", "arg\npipe", "Hello"] {
        insert_worker_text_response(&mut scripted_events, prompt_text, true, "Hello world");
    }

    let shell_parity_request = CompletionRequest {
        provider_id: Some("mock".to_string()),
        model_id: "model-1".to_string(),
        messages: vec![
            CompletionMessage {
                role: MessageRole::System,
                content: "worker-prompt".to_string(),
                name: None,
                tool_call_id: None,
                assistant_tool_calls: None,
            },
            CompletionMessage {
                role: MessageRole::User,
                content: "shell parity task".to_string(),
                name: None,
                tool_call_id: None,
                assistant_tool_calls: None,
            },
        ],
        temperature: Some(0.0),
        max_tokens: None,
        variant: None,
        reasoning_effort: None,
        text_verbosity: None,
        reasoning_summary: None,
        tools: Some(vec![demo_edit_tool_def()]),
        tool_choice: Some(ToolChoice::Auto),
        context: Default::default(),
        stream: true,
    };

    scripted_events.insert(
        request_digest(&shell_parity_request),
        vec![
            ProviderStreamEvent::Start,
            ProviderStreamEvent::TextDelta("Shell parity".to_string()),
            ProviderStreamEvent::TextDelta(" looks good.".to_string()),
            ProviderStreamEvent::Done {
                usage: CompletionUsage {
                    prompt_tokens: 10,
                    completion_tokens: 4,
                    total_tokens: 14,
                },
            },
        ],
    );

    MockProvider::new(scripted_events)
}

fn insert_worker_text_response(
    scripted_events: &mut BTreeMap<String, Vec<ProviderStreamEvent>>,
    prompt_text: &str,
    include_tools: bool,
    response: &str,
) {
    let mut request = CompletionRequest {
        provider_id: Some("mock".to_string()),
        model_id: "model-1".to_string(),
        messages: vec![
            CompletionMessage {
                role: MessageRole::System,
                content: "worker-prompt".to_string(),
                name: None,
                tool_call_id: None,
                assistant_tool_calls: None,
            },
            CompletionMessage {
                role: MessageRole::User,
                content: prompt_text.to_string(),
                name: None,
                tool_call_id: None,
                assistant_tool_calls: None,
            },
        ],
        temperature: Some(0.0),
        max_tokens: None,
        variant: None,
        reasoning_effort: None,
        text_verbosity: None,
        reasoning_summary: None,
        tools: None,
        tool_choice: None,
        context: Default::default(),
        stream: true,
    };
    if include_tools {
        request.tools = Some(vec![demo_edit_tool_def()]);
        request.tool_choice = Some(ToolChoice::Auto);
    }

    scripted_events.insert(
        request_digest(&request),
        vec![
            ProviderStreamEvent::Start,
            ProviderStreamEvent::TextDelta(response.to_string()),
            ProviderStreamEvent::Done {
                usage: CompletionUsage {
                    prompt_tokens: 4,
                    completion_tokens: 2,
                    total_tokens: 6,
                },
            },
        ],
    );
}

fn demo_edit_tool_def() -> ToolDef {
    let tool_id = "edit";
    let registry = coordinator_registry(ShellAllowlist::default());
    let tool = registry
        .get(tool_id)
        .expect("golden path scenario requires edit");
    let function_name = build_tool_function_name_mapping([tool_id])
        .function_name_for_tool_id(tool_id)
        .expect("golden path scenario requires a deterministic edit function name")
        .to_string();

    ToolDef {
        tool_id: tool_id.to_string(),
        function_name,
        description: Some(tool.description().to_string()),
        parameters: tool.parameters_json_schema(),
    }
}

pub fn golden_path_profiles() -> BTreeMap<String, AgentProfile> {
    let mut profiles = BTreeMap::new();
    profiles.insert(
        "planner".to_string(),
        AgentProfile {
            name: "planner".to_string(),
            category: "deep".to_string(),
            model_ref: "mock:model-1".to_string(),
            model_ref_explicit: true,
            system_prompt: "planner-prompt".to_string(),
            cache_retention: Default::default(),
            max_iters: Some(12),
            temperature: Some(0.0),
            tool_failure_mode: harness_core::config::ToolFailureMode::ContinueAsToolMessage,
            toolset: vec![],
        },
    );
    profiles.insert(
        "worker".to_string(),
        AgentProfile {
            name: "worker".to_string(),
            category: "deep".to_string(),
            model_ref: "mock:model-1".to_string(),
            model_ref_explicit: true,
            system_prompt: "worker-prompt".to_string(),
            cache_retention: Default::default(),
            max_iters: Some(12),
            temperature: Some(0.0),
            tool_failure_mode: harness_core::config::ToolFailureMode::ContinueAsToolMessage,
            toolset: vec!["edit".to_string()],
        },
    );
    profiles
}

pub fn default_permission_policy() -> PermissionPolicy {
    use harness_core::config::PermissionMode;
    PermissionPolicy::new(
        PermissionMode::Ask,
        PermissionMode::Deny,
        PermissionMode::Deny,
    )
    .with_ask_timeout_ms(30_000)
}

pub fn supervisor_actor() -> EventActor {
    EventActor::new(ActorKind::Supervisor, Some("agent-supervisor".to_string()))
}

pub fn worker_actor(agent_id: String) -> EventActor {
    EventActor::new(ActorKind::Worker, Some(agent_id))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scenario_name_reports_interactive_permission_mode() {
        assert!(!ScenarioName::GoldenPath.interactive_permissions());
        assert!(ScenarioName::GoldenPathInteractive.interactive_permissions());
    }
}
