use std::process::Command;
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Duration;

use harness_core::agent::AgentProfile;
use harness_core::clock::FakeClock;
use harness_core::config::ToolFailureMode;
use harness_core::coord::{spawn_coordinator, CoordinatorConfig};
use harness_core::event::{
    ActorKind, EventActor, EventEnvelopeV1, EventV1, RunFinishedEvent, RunStartedEvent,
    TaskCompletedEvent, SCHEMA_VERSION,
};
use harness_core::proj::RunMetadata;
use harness_core::redact::DefaultRedactor;
use harness_core::tool::ToolSurface;
use harness_tui::app::{
    set_pending_live_launch_metadata, set_pending_live_prompt_draft, AppState, LaunchMetadata,
    RuntimeStateKind,
};
use harness_tui::{load_events_from_run_dir, Action};
use tempfile::tempdir;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

#[allow(dead_code)]
#[path = "../src/bootstrap.rs"]
mod bootstrap;
#[allow(dead_code)]
#[path = "../src/recovery.rs"]
mod recovery;
#[allow(dead_code)]
#[path = "../src/replay.rs"]
mod replay;
#[allow(dead_code)]
#[path = "../src/scenarios.rs"]
mod scenarios;
#[allow(dead_code)]
#[path = "../src/tui.rs"]
mod tui_impl;

fn startup_draft_test_lock() -> &'static Mutex<()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
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
        "profiles": {
            "deep": {
                "description": "Deep profile",
                "model_ref": "default:gpt-4o-mini",
                "tools": []
            },
            "ops": {
                "description": "Ops profile",
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

fn deterministic_responses_sse_transcript() -> String {
    [
        "event: response.output_text.delta\n",
        "data: {\"type\":\"response.output_text.delta\",\"delta\":\"Hello\"}\n\n",
        "event: response.completed\n",
        "data: {\"type\":\"response.completed\",\"response\":{\"usage\":{\"prompt_tokens\":5,\"completion_tokens\":1,\"total_tokens\":6}}}\n\n",
        "data: [DONE]\n\n",
    ]
    .concat()
}

fn write_events_jsonl(run_dir: &std::path::Path, events: &[EventEnvelopeV1]) {
    let body = events
        .iter()
        .map(|event| serde_json::to_string(event).expect("serialize event"))
        .collect::<Vec<_>>()
        .join("\n");
    std::fs::write(run_dir.join("events.jsonl"), format!("{body}\n")).expect("write events");
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

#[test]
fn tui_startup_new_session_bootstraps_live_after_intent() {
    let _guard = startup_draft_test_lock()
        .lock()
        .expect("startup draft test lock poisoned");
    set_pending_live_prompt_draft(None);

    set_pending_live_prompt_draft(Some("launcher draft".to_string()));
    let app = AppState::new_live(None, false, None);

    assert_eq!(app.prompt_buffer, "launcher draft");
    assert!(
        app.prompt_history.is_empty(),
        "draft carry-over must not auto-submit"
    );
    assert_eq!(app.runtime_state().kind, RuntimeStateKind::Ready);
    assert!(
        !app.runtime_state().summary.contains("startup launcher"),
        "startup-only status must not leak into live runtime state"
    );

    set_pending_live_prompt_draft(None);
}

#[test]
fn tui_startup_replay_session_uses_replay_mode() {
    let app = AppState::new_replay(std::path::PathBuf::from("/tmp/run"), Vec::new());
    assert!(app.replay_mode, "replay launch should enter replay mode");
}

#[test]
fn tui_startup_carries_unsent_draft_into_new_live_session() {
    let _guard = startup_draft_test_lock()
        .lock()
        .expect("startup draft test lock poisoned");
    set_pending_live_prompt_draft(None);

    set_pending_live_prompt_draft(Some("unsent startup draft".to_string()));
    let app = AppState::new_live(None, false, None);

    assert_eq!(app.prompt_buffer, "unsent startup draft");
    assert_eq!(app.prompt_cursor, "unsent startup draft".chars().count());
    assert!(!app.startup_mode, "live handoff must clear startup mode");
    assert!(
        app.prompt_history.is_empty(),
        "live handoff must not create pending turn"
    );
    assert_eq!(app.runtime_state().kind, RuntimeStateKind::Ready);

    set_pending_live_prompt_draft(None);
}

#[tokio::test]
async fn tui_new_live_bootstrap_stays_idle_until_first_user_prompt() {
    let temp = tempdir().expect("tempdir");
    let session_dir = temp.path().join("sessions");
    let workspace = temp.path().join("workspace");
    std::fs::create_dir_all(&workspace).expect("create workspace");

    let mut coordinator_config = CoordinatorConfig::new(&session_dir);
    coordinator_config.agent_profiles.insert(
        "deep".to_string(),
        AgentProfile {
            name: "deep".to_string(),
            category: "deep".to_string(),
            model_ref: "default:default".to_string(),
            system_prompt: "deep agent mode intro".to_string(),
            max_iters: 12,
            temperature: Some(0.0),
            tool_failure_mode: harness_core::config::ToolFailureMode::FailTurn,
            tool_surface: ToolSurface::Native,
            toolset: Vec::new(),
        },
    );

    let coordinator = spawn_coordinator(
        coordinator_config,
        Arc::new(FakeClock::new()),
        Arc::new(DefaultRedactor::default()),
    );

    let run = coordinator
        .start_run("interactive", &workspace)
        .await
        .expect("start interactive run");
    let agent_id = coordinator
        .spawn_agent_idle(
            EventActor::new(ActorKind::Supervisor, Some("agent-supervisor".to_string())),
            "deep",
            None,
        )
        .await
        .expect("spawn idle agent");

    let before = load_events_from_run_dir(&run.run_dir).expect("load idle bootstrap events");
    assert!(before
        .iter()
        .any(|event| matches!(&event.payload, EventV1::AgentSpawned(_))));
    assert!(
        !before.iter().any(|event| matches!(
            &event.payload,
            EventV1::UserMessageSubmitted(_) | EventV1::ProviderRequestStarted(_)
        )),
        "idle live bootstrap must not auto-submit a synthetic first turn"
    );

    let request_id = coordinator
        .request_agent_turn(
            EventActor::new(ActorKind::User, Some("interactive-user".to_string())),
            agent_id,
            "first real prompt",
        )
        .await
        .expect("submit first live prompt");

    tokio::time::sleep(Duration::from_millis(80)).await;
    coordinator.stop_run().await.expect("stop interactive run");

    let after = load_events_from_run_dir(&run.run_dir).expect("load submitted live events");
    let first_started = after
        .iter()
        .find_map(|event| match &event.payload {
            EventV1::ProviderRequestStarted(payload) => Some(payload),
            _ => None,
        })
        .expect("provider request should start after first user prompt");
    assert_eq!(first_started.request_id, request_id);
    assert_eq!(first_started.prompt_summary, "first real prompt");
    assert_eq!(
        after.iter()
            .filter(|event| matches!(&event.payload, EventV1::ProviderRequestStarted(_)))
            .count(),
        1,
        "interactive bootstrap should only create one provider request after the user's first prompt"
    );
}

#[tokio::test]
async fn interactive_runtime_routes_non_default_profile_to_matching_provider() {
    let default_server = MockServer::start().await;
    let ops_server = MockServer::start().await;
    for server in [&default_server, &ops_server] {
        Mock::given(method("POST"))
            .and(path("/v1/responses"))
            .respond_with(
                ResponseTemplate::new(200)
                    .insert_header("content-type", "text/event-stream")
                    .set_body_raw(
                        deterministic_responses_sse_transcript(),
                        "text/event-stream",
                    ),
            )
            .mount(server)
            .await;
    }

    let temp = tempdir().expect("tempdir");
    let session_dir = temp.path().join("sessions");
    let workspace = temp.path().join("workspace");
    std::fs::create_dir_all(&workspace).expect("create workspace");

    let config_path = temp.path().join("harness.multi-provider.jsonc");
    std::fs::write(
        &config_path,
        multi_provider_interactive_config(
            &format!("{}/v1", default_server.uri()),
            &format!("{}/v1", ops_server.uri()),
            &session_dir,
        ),
    )
    .expect("write config");

    let config = bootstrap::load_harness_config(&config_path).expect("load config");
    let coordinator = spawn_coordinator(
        bootstrap::build_interactive_coordinator_config(&config)
            .expect("build multi-provider interactive config"),
        Arc::new(FakeClock::new()),
        Arc::new(DefaultRedactor::default()),
    );

    let run = coordinator
        .start_run("interactive", &workspace)
        .await
        .expect("start interactive run");
    let agent_id = coordinator
        .spawn_agent_idle(
            EventActor::new(ActorKind::Supervisor, Some("agent-supervisor".to_string())),
            "ops",
            None,
        )
        .await
        .expect("spawn non-default provider agent");
    let request_id = coordinator
        .request_agent_turn(
            EventActor::new(ActorKind::User, Some("interactive-user".to_string())),
            agent_id,
            "Hello from ops",
        )
        .await
        .expect("submit prompt");

    for _ in 0..50 {
        if !ops_server
            .received_requests()
            .await
            .expect("ops request recording must be enabled")
            .is_empty()
        {
            break;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }

    coordinator.stop_run().await.expect("stop interactive run");

    let default_requests = default_server
        .received_requests()
        .await
        .expect("default request recording must be enabled");
    let ops_requests = ops_server
        .received_requests()
        .await
        .expect("ops request recording must be enabled");
    assert!(
        default_requests.is_empty(),
        "interactive runtime should not hit providers.default for ops profile"
    );
    assert_eq!(
        ops_requests
            .iter()
            .filter(|req| req.url.path() == "/v1/responses")
            .count(),
        1,
        "interactive runtime should hit the selected provider exactly once"
    );

    let events = load_events_from_run_dir(&run.run_dir).expect("load interactive events");
    let provider_started = events
        .iter()
        .find_map(|event| match &event.payload {
            EventV1::ProviderRequestStarted(data) if data.request_id == request_id => Some(data),
            _ => None,
        })
        .expect("provider request should be recorded");
    assert_eq!(provider_started.provider_id, "anthropic");
    assert_eq!(provider_started.model_id, "claude-3.7");
}

#[tokio::test]
async fn new_live_session_persists_selected_runtime_context_into_run_metadata() {
    let temp = tempdir().expect("tempdir");
    let session_dir = temp.path().join("sessions");
    let workspace = temp.path().join("workspace");
    std::fs::create_dir_all(&workspace).expect("create workspace");

    let mut coordinator_config = CoordinatorConfig::new(&session_dir);
    coordinator_config.agent_profiles.insert(
        "deep".to_string(),
        AgentProfile {
            name: "deep".to_string(),
            category: "deep".to_string(),
            model_ref: "default:gpt-5.4-mini".to_string(),
            system_prompt: "deep agent mode intro".to_string(),
            max_iters: 12,
            temperature: Some(0.0),
            tool_failure_mode: ToolFailureMode::FailTurn,
            tool_surface: ToolSurface::Native,
            toolset: Vec::new(),
        },
    );
    coordinator_config.agent_profiles.insert(
        "ops".to_string(),
        AgentProfile {
            name: "ops".to_string(),
            category: "ops".to_string(),
            model_ref: "anthropic:claude-3.7".to_string(),
            system_prompt: "ops agent mode intro".to_string(),
            max_iters: 12,
            temperature: Some(0.0),
            tool_failure_mode: ToolFailureMode::FailTurn,
            tool_surface: ToolSurface::Native,
            toolset: Vec::new(),
        },
    );

    let coordinator = spawn_coordinator(
        coordinator_config,
        Arc::new(FakeClock::new()),
        Arc::new(DefaultRedactor::default()),
    );

    let run = coordinator
        .start_run("interactive", &workspace)
        .await
        .expect("start interactive run");
    coordinator
        .spawn_agent_idle(
            EventActor::new(ActorKind::Supervisor, Some("agent-supervisor".to_string())),
            "ops",
            None,
        )
        .await
        .expect("spawn selected launch agent");

    let meta_body = std::fs::read_to_string(run.run_dir.join("meta.json")).expect("read meta");
    let metadata: RunMetadata = serde_json::from_str(&meta_body).expect("parse meta");
    let recorded_runtime_context = metadata
        .recorded_runtime_context
        .expect("selected runtime context should be recorded before first turn");

    assert_eq!(recorded_runtime_context.profile, "ops");
    assert_eq!(recorded_runtime_context.provider, "anthropic");
    assert_eq!(recorded_runtime_context.model, "claude-3.7");

    let bootstrap_events = load_events_from_run_dir(&run.run_dir).expect("load bootstrap events");
    assert!(bootstrap_events
        .iter()
        .any(|event| matches!(&event.payload, EventV1::AgentSpawned(_))));
    assert!(
        !bootstrap_events.iter().any(|event| matches!(
            &event.payload,
            EventV1::UserMessageSubmitted(_) | EventV1::ProviderRequestStarted(_)
        )),
        "selected runtime context must persist before the first user turn starts"
    );

    coordinator.stop_run().await.expect("stop interactive run");
}

#[test]
fn tui_continue_session_bootstraps_live_with_preloaded_history() {
    let _guard = startup_draft_test_lock()
        .lock()
        .expect("startup draft test lock poisoned");
    set_pending_live_prompt_draft(None);

    set_pending_live_prompt_draft(Some("preserved continue draft".to_string()));
    set_pending_live_launch_metadata(
        LaunchMetadata::new("alpha", "mock", Some("model-1".to_string()))
            .with_mode_label("Continued"),
    );

    let mut app = AppState::new_live(
        Some(std::path::PathBuf::from("/tmp/sessions/run_continue")),
        false,
        None,
    );
    for event in [
        envelope(
            1,
            EventV1::RunStarted(RunStartedEvent {
                run_name: "interactive".to_string(),
                workspace_root: "/tmp/workspace".to_string(),
            }),
        ),
        envelope_with_correlation(
            2,
            Some("req_000001"),
            EventV1::UserMessageSubmitted(harness_core::event::UserMessageSubmittedEvent {
                request_id: "req_000001".to_string(),
                text: "first question".to_string(),
            }),
        ),
        envelope_with_correlation(
            3,
            Some("req_000001"),
            EventV1::ProviderRequestStarted(harness_core::event::ProviderRequestStartedEvent {
                request_id: "req_000001".to_string(),
                provider_id: "mock".to_string(),
                model_id: "model-1".to_string(),
                prompt_summary: "first question".to_string(),
                request_digest: "digest-1".to_string(),
            }),
        ),
        envelope_with_correlation(
            4,
            Some("req_000001"),
            EventV1::ProviderStreamDelta(harness_core::event::ProviderStreamDeltaEvent {
                request_id: "req_000001".to_string(),
                delta: "first answer".to_string(),
            }),
        ),
        envelope_with_correlation(
            5,
            Some("req_000001"),
            EventV1::TaskCompleted(TaskCompletedEvent {
                task_id: "task_000001".to_string(),
                result_summary: "first answer".to_string(),
                result_digest: "digest-out".to_string(),
                metadata: None,
            }),
        ),
    ] {
        app.ingest_event(event);
    }

    assert_eq!(app.active_provider(), "mock");
    assert_eq!(app.current_model_label(), "model-1");
    assert_eq!(app.runtime_state().kind, RuntimeStateKind::Success);
    assert_eq!(app.prompt_buffer, "preserved continue draft");
}

#[test]
fn tui_continue_session_restores_launch_metadata_from_history() {
    let _guard = startup_draft_test_lock()
        .lock()
        .expect("startup draft test lock poisoned");

    set_pending_live_launch_metadata(
        LaunchMetadata::new(
            "history-profile",
            "history-provider",
            Some("history-model".to_string()),
        )
        .with_mode_label("Continued"),
    );

    let app = AppState::new_live(
        Some(std::path::PathBuf::from("/tmp/sessions/run_continue")),
        false,
        None,
    );

    assert_eq!(app.launch_mode_label(), Some("Continued"));
    assert_eq!(app.active_profile(), "history-profile");
    assert_eq!(app.active_provider(), "history-provider");
    assert_eq!(app.current_model_label(), "history-model");
}

#[test]
fn replay_bootstrap_falls_back_when_recorded_runtime_context_missing() {
    let run_dir = tempdir().expect("tempdir");
    write_events_jsonl(
        run_dir.path(),
        &[
            envelope(
                1,
                EventV1::RunStarted(RunStartedEvent {
                    run_name: "interactive".to_string(),
                    workspace_root: "/tmp/workspace".to_string(),
                }),
            ),
            envelope(
                2,
                EventV1::AgentSpawned(harness_core::event::AgentSpawnedEvent {
                    agent_id: "agent_000001".to_string(),
                    profile: "legacy-profile".to_string(),
                    parent_agent_id: None,
                }),
            ),
            envelope_with_correlation(
                3,
                Some("req_000001"),
                EventV1::ProviderRequestStarted(harness_core::event::ProviderRequestStartedEvent {
                    request_id: "req_000001".to_string(),
                    provider_id: "legacy-provider".to_string(),
                    model_id: "legacy-model".to_string(),
                    prompt_summary: "hello".to_string(),
                    request_digest: "digest-1".to_string(),
                }),
            ),
            envelope(
                4,
                EventV1::RunFinished(RunFinishedEvent {
                    summary: "done".to_string(),
                }),
            ),
        ],
    );
    std::fs::write(
        run_dir.path().join("meta.json"),
        serde_json::json!({
            "run_id": "run_fixture",
            "run_name": "interactive",
            "workspace_root": "/tmp/workspace",
            "config_digest": "none",
            "harness_version": env!("CARGO_PKG_VERSION")
        })
        .to_string(),
    )
    .expect("write legacy meta");

    let events = load_events_from_run_dir(run_dir.path()).expect("load replay events");
    let launch_metadata = tui_impl::replay_launch_metadata_for_test(run_dir.path(), &events);

    assert_eq!(launch_metadata.profile(), "legacy-profile");
    assert_eq!(launch_metadata.provider(), "legacy-provider");
    assert_eq!(launch_metadata.model(), Some("legacy-model"));
    assert_eq!(launch_metadata.mode_label(), Some("Replay"));
}

#[test]
fn replay_bootstrap_prefers_recorded_runtime_context_from_meta() {
    let run_dir = tempdir().expect("tempdir");
    write_events_jsonl(
        run_dir.path(),
        &[
            envelope(
                1,
                EventV1::RunStarted(RunStartedEvent {
                    run_name: "interactive".to_string(),
                    workspace_root: "/tmp/workspace".to_string(),
                }),
            ),
            envelope(
                2,
                EventV1::AgentSpawned(harness_core::event::AgentSpawnedEvent {
                    agent_id: "agent_000001".to_string(),
                    profile: "legacy-profile".to_string(),
                    parent_agent_id: None,
                }),
            ),
            envelope_with_correlation(
                3,
                Some("req_000001"),
                EventV1::ProviderRequestStarted(harness_core::event::ProviderRequestStartedEvent {
                    request_id: "req_000001".to_string(),
                    provider_id: "legacy-provider".to_string(),
                    model_id: "legacy-model".to_string(),
                    prompt_summary: "hello".to_string(),
                    request_digest: "digest-1".to_string(),
                }),
            ),
        ],
    );
    std::fs::write(
        run_dir.path().join("meta.json"),
        serde_json::json!({
            "run_id": "run_fixture",
            "run_name": "interactive",
            "workspace_root": "/tmp/workspace",
            "config_digest": "none",
            "harness_version": env!("CARGO_PKG_VERSION"),
            "recorded_runtime_context": {
                "profile": "archive",
                "provider": "default",
                "model": "gpt-5.4-mini",
                "variant": "deterministic",
                "display_label": "GPT-5.4 Mini · Deterministic"
            }
        })
        .to_string(),
    )
    .expect("write replay meta");

    let events = load_events_from_run_dir(run_dir.path()).expect("load replay events");
    let launch_metadata = tui_impl::replay_launch_metadata_for_test(run_dir.path(), &events);

    assert_eq!(launch_metadata.profile(), "archive");
    assert_eq!(launch_metadata.provider(), "default");
    assert_eq!(launch_metadata.model(), Some("gpt-5.4-mini"));
    assert_eq!(launch_metadata.variant(), Some("deterministic"));
    assert_eq!(
        launch_metadata.display_label(),
        Some("GPT-5.4 Mini · Deterministic")
    );
    assert_eq!(launch_metadata.mode_label(), Some("Replay"));
}

#[test]
fn tui_replay_and_continue_headers_are_distinct() {
    let _guard = startup_draft_test_lock()
        .lock()
        .expect("startup draft test lock poisoned");

    let events = vec![
        envelope(
            1,
            EventV1::RunStarted(RunStartedEvent {
                run_name: "interactive".to_string(),
                workspace_root: "/tmp/workspace".to_string(),
            }),
        ),
        envelope(
            2,
            EventV1::RunFinished(RunFinishedEvent {
                summary: "done".to_string(),
            }),
        ),
    ];

    set_pending_live_launch_metadata(
        LaunchMetadata::new("alpha", "mock", Some("model-1".to_string()))
            .with_mode_label("Continued"),
    );
    let mut continued = AppState::new_live(
        Some(std::path::PathBuf::from("/tmp/sessions/run_continue")),
        false,
        None,
    );
    continued.replace_events(events.clone());

    let replay = AppState::new_replay(
        std::path::PathBuf::from("/tmp/sessions/run_continue"),
        events,
    );

    assert_eq!(continued.launch_mode_label(), Some("Continued"));
    assert!(!continued.replay_mode);
    assert!(replay.replay_mode);
    assert!(
        replay.runtime_state().summary.contains("events loaded"),
        "replay runtime should stay read-only and distinct from continued live mode"
    );
}

#[test]
fn tui_cli() {
    let run_dir = tempdir().expect("tempdir");
    write_events_jsonl(
        run_dir.path(),
        &[
            envelope(
                1,
                EventV1::RunStarted(RunStartedEvent {
                    run_name: "replay-fixture".to_string(),
                    workspace_root: "/tmp/workspace".to_string(),
                }),
            ),
            envelope(
                2,
                EventV1::RunFinished(RunFinishedEvent {
                    summary: "done".to_string(),
                }),
            ),
        ],
    );

    let output = Command::new(env!("CARGO_BIN_EXE_harness"))
        .args([
            "tui",
            "--replay",
            run_dir.path().to_str().expect("run dir utf-8"),
            "--exit-on-finish",
        ])
        .output()
        .expect("run harness tui replay");

    assert!(
        output.status.success(),
        "stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn tui_cli_replay_flag_bypasses_launcher_shell() {
    let run_dir = tempdir().expect("tempdir");
    write_events_jsonl(
        run_dir.path(),
        &[
            envelope(
                1,
                EventV1::RunStarted(RunStartedEvent {
                    run_name: "replay-fixture".to_string(),
                    workspace_root: "/tmp/workspace".to_string(),
                }),
            ),
            envelope(
                2,
                EventV1::RunFinished(RunFinishedEvent {
                    summary: "done".to_string(),
                }),
            ),
        ],
    );

    let output = Command::new(env!("CARGO_BIN_EXE_harness"))
        .args([
            "tui",
            "--replay",
            run_dir.path().to_str().expect("run dir utf-8"),
            "--exit-on-finish",
        ])
        .output()
        .expect("run harness tui replay");

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        output.status.success(),
        "stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        stderr
    );
    assert!(
        !stderr.contains("startup launcher"),
        "--replay should bypass launcher shell, got:\n{stderr}"
    );
}

#[test]
fn tui_cli_without_config_prints_config_guidance() {
    let temp = tempdir().expect("tempdir");
    let output = Command::new(env!("CARGO_BIN_EXE_harness"))
        .current_dir(temp.path())
        .args(["tui", "--exit-on-finish"])
        .output()
        .expect("run harness tui");

    assert!(
        !output.status.success(),
        "stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("tui setup failed: interactive mode requires a config file"),
        "expected config guidance prefix, got:\n{stderr}"
    );
    assert!(
        stderr.contains("./harness.jsonc"),
        "expected current-directory config location, got:\n{stderr}"
    );
    assert!(
        stderr.contains("$XDG_CONFIG_HOME/harness/config.jsonc"),
        "expected XDG config location, got:\n{stderr}"
    );
    assert!(
        stderr.contains("configs/harness.example.jsonc"),
        "expected shipped example config hint, got:\n{stderr}"
    );
    assert!(
        stderr.contains("--mock"),
        "expected explicit --mock escape hatch, got:\n{stderr}"
    );
}

#[test]
fn tui_cli_bare_harness_reuses_interactive_mode() {
    let temp = tempdir().expect("tempdir");
    let output = Command::new(env!("CARGO_BIN_EXE_harness"))
        .current_dir(temp.path())
        .output()
        .expect("run bare harness");

    assert!(
        !output.status.success(),
        "stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("tui setup failed: interactive mode requires a config file"),
        "expected bare harness to enter interactive tui mode, got:\n{stderr}"
    );
}

#[test]
fn tui_cli_legacy_tui_alias_still_works() {
    let temp = tempdir().expect("tempdir");
    let output = Command::new(env!("CARGO_BIN_EXE_harness"))
        .current_dir(temp.path())
        .args(["tui", "--exit-on-finish"])
        .output()
        .expect("run harness tui");

    assert!(
        !output.status.success(),
        "stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("tui setup failed: interactive mode requires a config file"),
        "expected legacy tui alias to keep interactive mode behavior, got:\n{stderr}"
    );
}

#[test]
fn tui_cli_mock_flag_starts_demo_mode() {
    let temp = tempdir().expect("tempdir");
    let output = Command::new(env!("CARGO_BIN_EXE_harness"))
        .current_dir(temp.path())
        .args(["tui", "--mock", "--exit-on-finish"])
        .output()
        .expect("run harness tui mock");

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        !stderr.contains("tui setup failed:"),
        "expected --mock to bypass config guidance, got:\n{stderr}"
    );
    assert!(
        output.status.success()
            || stderr.contains("tui failed: TUI error:")
            || stderr.contains("tui failed: startup launcher error:"),
        "expected --mock to reach demo mode startup, got stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        stderr,
    );
}

#[test]
fn tui_mock_mode_still_boots_through_launcher() {
    let temp = tempdir().expect("tempdir");
    let output = Command::new(env!("CARGO_BIN_EXE_harness"))
        .current_dir(temp.path())
        .args(["tui", "--mock", "--exit-on-finish"])
        .output()
        .expect("run harness tui mock");

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        !stderr.contains("tui setup failed:"),
        "expected --mock to bypass config guidance, got:\n{stderr}"
    );
    assert!(
        output.status.success() || stderr.contains("startup launcher"),
        "expected --mock to pass through launcher startup shell, got stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        stderr,
    );
}

#[test]
fn tui_cli_root_help_only_shows_minimal_interactive_overrides() {
    let output = Command::new(env!("CARGO_BIN_EXE_harness"))
        .args(["--help"])
        .output()
        .expect("run harness help");

    assert!(
        output.status.success(),
        "stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("Launch the interactive harness UI or run subcommands"));
    assert!(stdout.contains("--profile <PROFILE>"));
    assert!(stdout.contains("--mock"));
    assert!(stdout.contains("tui"));
    assert!(stdout.contains("run"));
    assert!(stdout.contains("prompt"));
    assert!(
        !stdout.contains("--replay"),
        "root help should keep replay off the bare surface"
    );
    assert!(
        !stdout.contains("--scenario"),
        "root help should keep scenario off the bare surface"
    );
    assert!(
        !stdout.contains("--deterministic"),
        "root help should keep deterministic off the bare surface"
    );
    assert!(
        !stdout.contains("--exit-on-finish"),
        "root help should keep advanced tui flags off the bare surface"
    );
}

#[test]
fn tui_subcommand_help_surfaces_direct_continue_recovery_flag() {
    let output = Command::new(env!("CARGO_BIN_EXE_harness"))
        .args(["tui", "--help"])
        .output()
        .expect("run harness tui help");

    assert!(
        output.status.success(),
        "stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("--continue <SESSION>"));
    assert!(stdout.contains("--replay <REPLAY>"));
}

#[test]
fn command_palette_includes_task5_session_actions() {
    let palette_commands = Action::palette_commands();
    let palette_surface = palette_commands
        .iter()
        .map(|(command, description)| format!("{command}:{description}"))
        .collect::<Vec<_>>()
        .join("\n")
        .to_ascii_lowercase();

    assert!(
        palette_surface.contains("open_event_log:open the review event log surface")
            && !palette_surface.contains("open_diff_review:")
            && !palette_surface.contains("help:"),
        "expected the ctrl-p surface to expose the event-log review surface without stale diff-review or tab chrome commands, got:
{palette_surface}"
    );
    assert!(
        palette_surface.contains("new_session:start a fresh live session")
            && palette_surface.contains("resume_session:continue a prior session when resumable")
            && palette_surface.contains("replay_session:replay a previous session as read-only"),
        "expected task-5 ctrl-p surface to include session actions, got:\n{palette_surface}"
    );
}

#[test]
fn tui_cli_invalid_config_fails_without_mock_fallback() {
    let temp = tempdir().expect("tempdir");
    let missing_config = temp.path().join("does-not-exist.jsonc");
    let output = Command::new(env!("CARGO_BIN_EXE_harness"))
        .current_dir(temp.path())
        .args([
            "--config",
            missing_config
                .to_str()
                .expect("missing config path should be valid utf-8"),
            "tui",
            "--exit-on-finish",
        ])
        .output()
        .expect("run harness tui with invalid config path");

    assert!(
        !output.status.success(),
        "stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("tui setup failed:"),
        "expected setup failure prefix, got:\n{stderr}"
    );
    assert!(
        !stderr.contains("golden_path") && !stderr.contains("scenario"),
        "invalid interactive config should fail before scenario/mock fallback, got:\n{stderr}"
    );
}
