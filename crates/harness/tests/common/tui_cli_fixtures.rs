use harness::UnwrapOrAbort;
use std::collections::BTreeMap;
use std::ffi::OsString;
use std::future::Future;
use std::path::PathBuf;
use std::pin::Pin;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use cli_io::load_events_from_run_dir;
use harness_core::agent::AgentProfile;
use harness_core::clock::FakeClock;
use harness_core::config::{load_config_from_file, ToolFailureMode};
use harness_core::coord::{spawn_coordinator, CoordinatorConfig};
use harness_core::event::{
    ActorKind, EventActor, EventEnvelopeV1, EventV1, RunFinishedEvent, RunStartedEvent,
    TaskCompletedEvent, SCHEMA_VERSION,
};
use harness_core::proj::RunMetadata;
use harness_core::redact::DefaultRedactor;
use harness_providers::{
    CompletionRequest, CompletionUsage, Provider, ProviderEventStream, ProviderRouter,
    ProviderStreamEvent,
};
use harness_tui::app::{
    set_pending_live_launch_metadata, set_pending_live_prompt_draft, AppState, LaunchMetadata,
    RuntimeStateKind,
};
use harness_tui::Action;
use tempfile::tempdir;

#[path = "mod.rs"]
mod common;
use common::{CliHarness, CliHarnessOutput};

#[path = "../../src/bootstrap.rs"]
mod bootstrap;
#[path = "../../src/cli_config.rs"]
mod cli_config;
#[path = "../../src/cli_io.rs"]
mod cli_io;
#[path = "../../src/cli_labels.rs"]
mod cli_labels;
#[path = "../../src/defaults.rs"]
mod defaults;
#[path = "../../src/dynamic_prompt.rs"]
mod dynamic_prompt;
#[path = "../../src/generated_model_catalog.rs"]
mod generated_model_catalog;
#[path = "../../src/logging.rs"]
mod logging;
#[path = "../../src/recovery.rs"]
mod recovery;
#[path = "../../src/replay.rs"]
mod replay;
#[path = "../../src/runtime_catalog.rs"]
mod runtime_catalog;
#[path = "../../src/scenarios.rs"]
mod scenarios;
#[path = "../../src/tui.rs"]
mod tui_impl;

fn startup_draft_test_lock() -> &'static Mutex<()> {
    tui_impl::tests::startup_draft_test_lock()
}

fn run_harness<I, S>(args: I) -> CliHarnessOutput
where
    I: IntoIterator<Item = S>,
    S: Into<OsString>,
{
    CliHarness::new().args(args).output()
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

fn multi_provider_interactive_config(
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

#[derive(Default)]
struct CapturingInteractiveProvider {
    requests: Mutex<Vec<CompletionRequest>>,
}

impl CapturingInteractiveProvider {
    fn requests(&self) -> Vec<CompletionRequest> {
        self.requests.lock().unwrap_or_abort().clone()
    }
}

impl Provider for CapturingInteractiveProvider {
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
            .push(req.clone());
        Box::pin(async move {
            let digest = harness_providers::mock::request_digest(&req);
            let provider = harness_providers::mock::MockProvider::new(BTreeMap::from([(
                digest,
                vec![
                    ProviderStreamEvent::Start,
                    ProviderStreamEvent::TextDelta("Hello".to_string()),
                    ProviderStreamEvent::Done {
                        usage: CompletionUsage {
                            prompt_tokens: 5,
                            completion_tokens: 1,
                            total_tokens: 6,
                        },
                    },
                ],
            )]));
            provider.stream_completion(req).await
        })
    }
}

fn capturing_interactive_provider_router() -> (
    Arc<CapturingInteractiveProvider>,
    Arc<CapturingInteractiveProvider>,
    Arc<dyn Provider>,
) {
    let default_provider = Arc::new(CapturingInteractiveProvider::default());
    let ops_provider = Arc::new(CapturingInteractiveProvider::default());
    let router = ProviderRouter::new(BTreeMap::from([
        (
            "default".to_string(),
            Arc::clone(&default_provider) as Arc<dyn Provider>,
        ),
        (
            "anthropic".to_string(),
            Arc::clone(&ops_provider) as Arc<dyn Provider>,
        ),
    ]));
    (default_provider, ops_provider, Arc::new(router))
}

fn write_events_jsonl(run_dir: &std::path::Path, events: &[EventEnvelopeV1]) {
    let body = events
        .iter()
        .map(|event| serde_json::to_string(event).unwrap_or_abort())
        .collect::<Vec<_>>()
        .join("\n");
    std::fs::write(run_dir.join("events.jsonl"), format!("{body}\n")).unwrap_or_abort();
}

fn envelope(seq: u64, payload: EventV1) -> EventEnvelopeV1 {
    envelope_with_correlation(seq, None, payload)
}

fn envelope_with_correlation(
    seq: u64,
    correlation_id: Option<&str>,
    payload: EventV1,
) -> EventEnvelopeV1 {
    EventEnvelopeV1 {
        schema_version: SCHEMA_VERSION,
        event_id: format!("evt-{seq:04}"),
        seq,
        run_id: "run_fixture".to_string(),
        mono_ms: seq,
        ts: None,
        actor: EventActor::new(ActorKind::System, Some("test".to_string())),
        correlation_id: correlation_id.map(str::to_string),
        causation_id: None,
        stream_key: Some("run:run_fixture".to_string()),
        payload,
    }
}
