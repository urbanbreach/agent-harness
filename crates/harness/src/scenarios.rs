use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use clap::ValueEnum;
use harness_core::agent::AgentProfile;
use harness_core::edit::hashline::{compute_line_hash, HashlineOp, HashlinePatch, LineAnchor};
use harness_core::event::{ActorKind, EventActor};
use harness_core::perm::PermissionPolicy;
use harness_core::tool::ToolSurface;
use harness_providers::mock::{request_digest, MockProvider};
use harness_providers::{
    CompletionMessage, CompletionRequest, CompletionUsage, MessageRole, ProviderStreamEvent,
    ToolChoice, ToolDef,
};
use serde_json::json;

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
            tools: None,
            tool_choice: None,
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
        tools: None,
        tool_choice: None,
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
        tools: Some(vec![demo_hashline_apply_tool_def()]),
        tool_choice: Some(ToolChoice::Auto),
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

fn demo_hashline_apply_tool_def() -> ToolDef {
    ToolDef {
        tool_id: "edit.hashline_apply".to_string(),
        function_name: "edit_hashline_apply".to_string(),
        description: Some(
            "Applies a hashline patch to a workspace file and writes an artifact diff.".to_string(),
        ),
        parameters: json!({
            "type": "object",
            "additionalProperties": false,
            "required": ["edit_id", "path", "ops"],
            "properties": {
                "edit_id": { "type": "string" },
                "path": { "type": "string" },
                "ops": {
                    "type": "array",
                    "items": {
                        "oneOf": [
                            {
                                "type": "object",
                                "additionalProperties": false,
                                "required": ["Rewrite"],
                                "properties": {
                                    "Rewrite": {
                                        "type": "object",
                                        "additionalProperties": false,
                                        "required": ["lines"],
                                        "properties": {
                                            "lines": {
                                                "type": "array",
                                                "items": { "type": "string" }
                                            }
                                        }
                                    }
                                }
                            },
                            {
                                "type": "object",
                                "additionalProperties": false,
                                "required": ["InsertBefore"],
                                "properties": {
                                    "InsertBefore": {
                                        "type": "object",
                                        "additionalProperties": false,
                                        "required": ["anchor", "lines"],
                                        "properties": {
                                            "anchor": { "$ref": "#/definitions/LineAnchor" },
                                            "lines": {
                                                "type": "array",
                                                "items": { "type": "string" }
                                            }
                                        }
                                    }
                                }
                            },
                            {
                                "type": "object",
                                "additionalProperties": false,
                                "required": ["InsertAfter"],
                                "properties": {
                                    "InsertAfter": {
                                        "type": "object",
                                        "additionalProperties": false,
                                        "required": ["anchor", "lines"],
                                        "properties": {
                                            "anchor": { "$ref": "#/definitions/LineAnchor" },
                                            "lines": {
                                                "type": "array",
                                                "items": { "type": "string" }
                                            }
                                        }
                                    }
                                }
                            },
                            {
                                "type": "object",
                                "additionalProperties": false,
                                "required": ["Replace"],
                                "properties": {
                                    "Replace": {
                                        "type": "object",
                                        "additionalProperties": false,
                                        "required": ["expected", "lines"],
                                        "properties": {
                                            "expected": {
                                                "type": "array",
                                                "items": { "$ref": "#/definitions/LineAnchor" }
                                            },
                                            "lines": {
                                                "type": "array",
                                                "items": { "type": "string" }
                                            }
                                        }
                                    }
                                }
                            },
                            {
                                "type": "object",
                                "additionalProperties": false,
                                "required": ["Delete"],
                                "properties": {
                                    "Delete": {
                                        "type": "object",
                                        "additionalProperties": false,
                                        "required": ["expected"],
                                        "properties": {
                                            "expected": {
                                                "type": "array",
                                                "items": { "$ref": "#/definitions/LineAnchor" }
                                            }
                                        }
                                    }
                                }
                            }
                        ]
                    }
                }
            },
            "definitions": {
                "LineAnchor": {
                    "type": "object",
                    "additionalProperties": false,
                    "required": ["line", "hash"],
                    "properties": {
                        "line": { "type": "integer", "minimum": 0 },
                        "hash": { "type": "string" }
                    }
                }
            }
        }),
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
            system_prompt: "planner-prompt".to_string(),
            tool_failure_mode: harness_core::config::ToolFailureMode::FailTurn,
            tool_surface: ToolSurface::Native,
            toolset: vec![],
        },
    );
    profiles.insert(
        "worker".to_string(),
        AgentProfile {
            name: "worker".to_string(),
            category: "deep".to_string(),
            model_ref: "mock:model-1".to_string(),
            system_prompt: "worker-prompt".to_string(),
            tool_failure_mode: harness_core::config::ToolFailureMode::FailTurn,
            tool_surface: ToolSurface::Native,
            toolset: vec!["edit.hashline_apply".to_string()],
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
