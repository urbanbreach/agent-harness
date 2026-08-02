use harness::UnwrapOrAbort;
use std::collections::BTreeMap;
use std::ffi::OsString;
use std::fs;
use std::future::Future;
use std::path::PathBuf;
use std::pin::Pin;
use std::sync::{Arc, Mutex};

use harness_core::event::{
    ActorKind, AgentSpawnedEvent, EventActor, EventEnvelopeV1, EventV1,
    ProviderRequestFinishedEvent, ProviderRequestStartedEvent, RunFinishedEvent, RunStartedEvent,
    TaskTerminalScope, UserMessageSubmittedEvent, SCHEMA_VERSION,
};
use harness_providers::{
    CompletionRequest, CompletionUsage, Provider, ProviderErrorCategory, ProviderEventStream,
    ProviderStreamEvent,
};
use tempfile::tempdir;

#[path = "mod.rs"]
mod common;

use common::{CliHarness, CliHarnessOutput};

#[derive(Debug, Clone, PartialEq)]
struct CapturedPromptRequest {
    provider_id: Option<String>,
    model_id: String,
    variant: Option<String>,
    reasoning_effort: Option<String>,
    text_verbosity: Option<String>,
    reasoning_summary: Option<String>,
    body: serde_json::Value,
}

type ScriptedPromptHandler = dyn Fn(&CompletionRequest) -> Vec<ProviderStreamEvent> + Send + Sync;

struct ScriptedPromptProvider {
    requests: Mutex<Vec<CapturedPromptRequest>>,
    handler: Arc<ScriptedPromptHandler>,
}

impl ScriptedPromptProvider {
    fn new(
        handler: impl Fn(&CompletionRequest) -> Vec<ProviderStreamEvent> + Send + Sync + 'static,
    ) -> Arc<Self> {
        Arc::new(Self {
            requests: Mutex::new(Vec::new()),
            handler: Arc::new(handler),
        })
    }

    fn fixed(events: Vec<ProviderStreamEvent>) -> Arc<Self> {
        Self::new(move |_| events.clone())
    }

    fn sequence(responses: Vec<Vec<ProviderStreamEvent>>) -> Arc<Self> {
        let responses = Arc::new(responses);
        let index = Arc::new(Mutex::new(0_usize));
        Self::new(move |_| {
            let mut guard = index.lock().unwrap_or_abort();
            let response = responses
                .get(*guard)
                .or_else(|| responses.last())
                .cloned()
                .unwrap_or_else(|| text_events("Hello"));
            *guard += 1;
            response
        })
    }

    fn requests(&self) -> Vec<CapturedPromptRequest> {
        self.requests.lock().unwrap_or_abort().clone()
    }
}

impl Provider for ScriptedPromptProvider {
    fn stream_completion<'life0, 'async_trait>(
        &'life0 self,
        req: CompletionRequest,
    ) -> Pin<Box<dyn Future<Output = ProviderEventStream> + Send + 'async_trait>>
    where
        'life0: 'async_trait,
        Self: 'async_trait,
    {
        self.requests
            .lock()
            .unwrap_or_abort()
            .push(CapturedPromptRequest::from(&req));
        let events = (self.handler)(&req);
        Box::pin(async move {
            let digest = harness_providers::mock::request_digest(&req);
            let provider = harness_providers::mock::MockProvider::new(BTreeMap::from([(
                digest, events,
            )]));
            provider.stream_completion(req).await
        })
    }
}

impl CapturedPromptRequest {
    fn from(request: &CompletionRequest) -> Self {
        Self {
            provider_id: request.provider_id.clone(),
            model_id: request.model_id.clone(),
            variant: request.variant.clone(),
            reasoning_effort: request.reasoning_effort.clone(),
            text_verbosity: request.text_verbosity.clone(),
            reasoning_summary: request.reasoning_summary.clone(),
            body: serde_json::to_value(request).unwrap_or_abort(),
        }
    }
}

fn text_events(text: &str) -> Vec<ProviderStreamEvent> {
    vec![
        ProviderStreamEvent::Start,
        ProviderStreamEvent::TextDelta(text.to_string()),
        ProviderStreamEvent::Done {
            usage: Some(usage(5, 1)),
        },
    ]
}

fn reasoning_events() -> Vec<ProviderStreamEvent> {
    vec![
        ProviderStreamEvent::Start,
        ProviderStreamEvent::ReasoningDelta("Drafting a careful answer.".to_string()),
        ProviderStreamEvent::TextDelta("Hello".to_string()),
        ProviderStreamEvent::TextDelta(" world".to_string()),
        ProviderStreamEvent::Done { usage: Some(usage(5, 2)) },
    ]
}

fn late_reasoning_duplicate_body_events() -> Vec<ProviderStreamEvent> {
    vec![
        ProviderStreamEvent::Start,
        ProviderStreamEvent::TextDelta("Hi! How can I help?".to_string()),
        ProviderStreamEvent::ReasoningDelta("Responding to greetings".to_string()),
        ProviderStreamEvent::TextDelta("\nHi! How can I help? ".to_string()),
        ProviderStreamEvent::Done { usage: Some(usage(5, 2)) },
    ]
}

fn repeated_body_chunks_before_reasoning_events() -> Vec<ProviderStreamEvent> {
    vec![
        ProviderStreamEvent::Start,
        ProviderStreamEvent::TextDelta("ha".to_string()),
        ProviderStreamEvent::TextDelta(" ha".to_string()),
        ProviderStreamEvent::ReasoningDelta("Done planning.".to_string()),
        ProviderStreamEvent::Done { usage: Some(usage(5, 2)) },
    ]
}

fn tool_call_events(
    tool_call_id: &str,
    function_name: &str,
    arguments: serde_json::Value,
) -> Vec<ProviderStreamEvent> {
    vec![
        ProviderStreamEvent::Start,
        ProviderStreamEvent::ToolCallComplete {
            tool_call_id: tool_call_id.to_string(),
            function_name: function_name.to_string(),
            arguments_json: arguments.to_string(),
        },
        ProviderStreamEvent::Done { usage: Some(usage(12, 3)) },
    ]
}

fn provider_error_events() -> Vec<ProviderStreamEvent> {
    vec![
        ProviderStreamEvent::Start,
        ProviderStreamEvent::error("fixture provider failure"),
    ]
}

fn categorized_provider_error_events(category: ProviderErrorCategory) -> Vec<ProviderStreamEvent> {
    vec![
        ProviderStreamEvent::Start,
        ProviderStreamEvent::categorized_error("fixture provider failure", category),
    ]
}

fn usage(prompt_tokens: u32, completion_tokens: u32) -> CompletionUsage {
    CompletionUsage {
        prompt_tokens,
        completion_tokens,
        total_tokens: prompt_tokens + completion_tokens,
    }
}

fn run_harness_in<I, S, P>(current_dir: P, args: I) -> CliHarnessOutput
where
    I: IntoIterator<Item = S>,
    S: Into<OsString>,
    P: Into<PathBuf>,
{
    CliHarness::new()
        .current_dir(current_dir)
        .args(args)
        .output()
}

fn run_harness_in_with_stdin<I, S, P>(
    current_dir: P,
    args: I,
    stdin: impl Into<Vec<u8>>,
) -> CliHarnessOutput
where
    I: IntoIterator<Item = S>,
    S: Into<OsString>,
    P: Into<PathBuf>,
{
    CliHarness::new()
        .current_dir(current_dir)
        .stdin(stdin)
        .args(args)
        .output()
}

fn run_harness_in_with_provider<I, S, P>(
    current_dir: P,
    args: I,
    provider: Arc<dyn Provider>,
) -> CliHarnessOutput
where
    I: IntoIterator<Item = S>,
    S: Into<OsString>,
    P: Into<PathBuf>,
{
    CliHarness::new()
        .current_dir(current_dir)
        .provider_override(provider)
        .args(args)
        .output()
}

async fn run_harness_in_blocking_with_provider<I, S, P>(
    current_dir: P,
    args: I,
    provider: Arc<dyn Provider>,
) -> CliHarnessOutput
where
    I: IntoIterator<Item = S>,
    S: Into<OsString>,
    P: Into<PathBuf>,
{
    let current_dir = current_dir.into();
    let args = args.into_iter().map(Into::into).collect::<Vec<OsString>>();
    tokio::task::spawn_blocking(move || {
        CliHarness::new()
            .current_dir(current_dir)
            .provider_override(provider)
            .args(args)
            .output()
    })
    .await
    .unwrap_or_abort()
}

fn prompt_cli_config(base_url: &str, session_dir: &std::path::Path, tools: &[&str]) -> String {
    serde_json::json!({
        "providers": {
            "default": {
                "type": "openai_compatible",
                "base_url": base_url,
                "api_key": "DUMMY",
                "api_mode": "responses",
                "timeout_ms": 60000,
                "models": {
                    "gpt-4o-mini": {
                        "display_name": "GPT-4o mini",
                        "metadata": {
                            "supports_reasoning_summaries": true
                        },
                        "variants": {
                            "low": {
                                "display_name": "Low",
                                "metadata": {
                                    "reasoning_effort": "low",
                                    "text_verbosity": "low"
                                }
                            }
                        }
                    }
                }
            }
        },
        "agents": {
            "deep": {
                "description": "Deep profile",
                "system_prompt": "You are the deep profile.",
                "model_ref": "default:gpt-4o-mini",
                "tool_failure_mode": "continue_as_tool_message",
                "tools": tools
            }
        },
        "permissions": {
            "defaults": {
                "edit": "allow",
                "shell": "allow",
                "network": "allow"
            }
        },
        "runtime": {
            "background_tasks": {
                "default_concurrency": 2,
                "provider_concurrency": 2,
                "model_concurrency": 2,
                "stale_timeout_ms": 30000,
                "message_staleness_timeout_ms": 10000
            },
            "provider_retry": {
                "max_retries": 0
            },
            "session_dir": session_dir,
            "deterministic": {
                "enabled": false,
                "seed": 42
            }
        },
        "integrations": {
            "remote_search": {
                "endpoint": "https://mcp.exa.ai/mcp"
            }
        },
        "ui": {
            "default_profile": "deep"
        }
    })
    .to_string()
}

fn prompt_cli_multi_provider_config(
    default_base_url: &str,
    ops_base_url: &str,
    session_dir: &std::path::Path,
) -> String {
    serde_json::json!({
        "providers": {
            "default": {
                "type": "openai_compatible",
                "base_url": default_base_url,
                "api_key": "DUMMY",
                "api_mode": "responses",
                "timeout_ms": 60000,
                "models": {
                    "gpt-4o-mini": {
                        "display_name": "GPT-4o mini"
                    }
                }
            },
            "anthropic": {
                "type": "openai_compatible",
                "base_url": ops_base_url,
                "api_key": "DUMMY",
                "api_mode": "responses",
                "timeout_ms": 60000,
                "models": {
                    "claude-3.7": {
                        "display_name": "Claude 3.7"
                    }
                }
            }
        },
        "agents": {
            "deep": {
                "description": "Deep profile",
                "system_prompt": "You are the deep profile.",
                "model_ref": "default:gpt-4o-mini",
                "tools": []
            },
            "ops": {
                "description": "Ops profile",
                "system_prompt": "You are the ops profile.",
                "model_ref": "anthropic:claude-3.7",
                "tools": []
            }
        },
        "permissions": {
            "defaults": {
                "edit": "allow",
                "shell": "allow",
                "network": "allow"
            }
        },
        "runtime": {
            "background_tasks": {
                "default_concurrency": 2,
                "provider_concurrency": 2,
                "model_concurrency": 2,
                "stale_timeout_ms": 30000,
                "message_staleness_timeout_ms": 10000
            },
            "session_dir": session_dir,
            "deterministic": {
                "enabled": false,
                "seed": 42
            }
        },
        "integrations": {
            "remote_search": {
                "endpoint": "https://mcp.exa.ai/mcp"
            }
        },
        "ui": {
            "default_profile": "deep"
        }
    })
    .to_string()
}

fn prompt_cli_public_runtime_config(base_url: &str) -> String {
    serde_json::json!({
        "provider": {
            "default": {
                "type": "openai_compatible",
                "name": "CLIProxyAPI (OpenAI)",
                "options": {
                    "baseURL": base_url,
                    "apiKey": "DUMMY",
                    "apiMode": "responses",
                    "timeoutMs": 1800000,
                },
                "models": {
                    "gpt-5.4": {
                        "name": "GPT 5.4 (272k)",
                        "metadata": {
                            "family": "gpt-5",
                            "context_window_tokens": 272000,
                            "supports_tool_calls": true,
                            "supports_reasoning_summaries": true
                        },
                        "max_input_tokens": 272000,
                        "max_output_tokens": 128000
                    },
                    "gpt-5.4-mini": {
                        "name": "GPT 5.4 Mini",
                        "metadata": {
                            "family": "gpt-5",
                            "context_window_tokens": 272000,
                            "supports_tool_calls": true,
                            "supports_reasoning_summaries": true
                        },
                        "max_input_tokens": 272000,
                        "max_output_tokens": 128000,
                        "variants": {
                            "high": {
                                "name": "High",
                                "metadata": {
                                    "reasoning_effort": "high",
                                    "text_verbosity": "low"
                                }
                            }
                        }
                    }
                }
            }
        },
        "model": "default/gpt-5.4",
        "small_model": "default/gpt-5.4-mini",
        "agent": {
            "build": {
                "system_prompt": "You are the build profile.",
                "model": "default/gpt-5.4-mini",
                "variant": "high"
            }
        },
        "default_agent": "build",
        "permission": {
            "edit": "allow",
            "bash": "allow",
            "question": "allow",
            "task": "allow",
            "webfetch": "allow",
            "websearch": "allow",
            "codesearch": "allow",
            "lsp": "allow"
        }
    })
    .to_string()
}

async fn run_prompt_with_single_tool(
    workspace_root: &std::path::Path,
    provider: Arc<dyn Provider>,
    tools: &[&str],
    prompt_text: &str,
) -> CliHarnessOutput {
    let config_path = workspace_root.join("harness.tool.jsonc");
    let session_dir = workspace_root.join("sessions");
    let out_path = workspace_root.join("events.jsonl");

    let config = prompt_cli_config("https://fixture.test/v1", &session_dir, tools);

    fs::write(&config_path, config).unwrap_or_abort();

    run_harness_in_blocking_with_provider(
        workspace_root,
        [
            OsString::from("--config"),
            config_path.as_os_str().to_owned(),
            OsString::from("prompt"),
            OsString::from("--text"),
            OsString::from(prompt_text),
            OsString::from("--out"),
            out_path.as_os_str().to_owned(),
        ],
        provider,
    )
    .await
}

fn assert_successful_tool_roundtrip(
    output: &CliHarnessOutput,
    events_body: &str,
    tool_id: &str,
) {
    assert!(
        output.status.success(),
        "stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(events_body.contains("\"event_type\":\"tool_call_requested\""));
    assert!(events_body.contains("\"event_type\":\"tool_call_started\""));
    assert!(events_body.contains("\"event_type\":\"tool_call_finished\""));
    assert!(events_body.contains("\"status\":\"succeeded\""));
    assert!(
        events_body.contains(tool_id),
        "expected events to mention {tool_id}: {events_body}"
    );
}

fn write_resume_fixture_events(run_dir: &std::path::Path) {
    let events = [
        resume_envelope(
            "run_resume_cli",
            1,
            EventV1::RunStarted(RunStartedEvent {
                run_name: "interactive".into(),
                workspace_root: "/tmp/workspace".to_string(),
            }),
        ),
        resume_envelope(
            "run_resume_cli",
            2,
            EventV1::AgentSpawned(AgentSpawnedEvent {
                agent_id: "agent_000001".to_string(),
                profile: "deep".to_string(),
                parent_agent_id: None,
            }),
        ),
        resume_envelope(
            "run_resume_cli",
            3,
            EventV1::UserMessageSubmitted(UserMessageSubmittedEvent {
                request_id: "req_000001".into(),
                text: "Original prompt".to_string(),
            }),
        ),
        resume_envelope(
            "run_resume_cli",
            4,
            EventV1::ProviderRequestStarted(ProviderRequestStartedEvent {
                request_id: "req_000001".into(),
                provider_id: "default".to_string(),
                model_id: "gpt-4o-mini".to_string(),
                prompt_summary: "Original prompt".to_string(),
                request_digest: "digest-original".to_string(),
                metadata: None,
            }),
        ),
        resume_envelope(
            "run_resume_cli",
            5,
            EventV1::ProviderRequestFinished(ProviderRequestFinishedEvent {
                request_id: "req_000001".into(),
                finish_reason: "stop".to_string(),
                output_digest: Some("digest-output".to_string()),
                usage: None,
                metadata: None,
            }),
        ),
        resume_envelope(
            "run_resume_cli",
            6,
            EventV1::RunFinished(RunFinishedEvent {
                summary: "done".to_string(),
            }),
        ),
    ];

    let body = events
        .iter()
        .map(|event| serde_json::to_string(event).unwrap_or_abort())
        .collect::<Vec<_>>()
        .join("\n");
    fs::write(run_dir.join("events.jsonl"), format!("{body}\n")).unwrap_or_abort();
}

fn resume_envelope(run_id: &str, seq: u64, payload: EventV1) -> EventEnvelopeV1 {
    EventEnvelopeV1 {
        schema_version: SCHEMA_VERSION,
        event_id: format!("evt-{seq:04}"),
        seq,
        run_id: run_id.to_string().into(),
        mono_ms: seq,
        ts: None,
        actor: EventActor::new(ActorKind::Supervisor, Some("resume-test".to_string())),
        correlation_id: None,
        causation_id: None,
        stream_key: Some(format!("run:{run_id}")),
        payload,
    }
}
