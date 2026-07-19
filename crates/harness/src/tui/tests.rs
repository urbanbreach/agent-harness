#[path = "tests/auth_backend_tests.rs"]
mod auth_backend_tests;
#[path = "tests/coordinator_warmup_tests.rs"]
mod coordinator_warmup_tests;
#[path = "tests/launch_metadata_tests.rs"]
mod launch_metadata_tests;
#[path = "tests/lineage_tests.rs"]
mod lineage_tests;
#[path = "tests/live_intent_tests.rs"]
mod live_intent_tests;
#[path = "tests/live_settings_tests.rs"]
mod live_settings_tests;
#[path = "tests/model_selection_tests.rs"]
mod model_selection_tests;
#[path = "tests/runtime_toggles_tests.rs"]
mod runtime_toggles_tests;
#[path = "tests/startup_tests.rs"]
mod startup_tests;

use super::*;
use crate::recovery::most_recent_conversational_agent_id;
use crate::UnwrapOrAbort;
use harness_core::auth::{
    AuthProviderId, CredentialClock, CredentialStore, StoredCredential, SystemCredentialClock,
};
use harness_core::config::load_config_from_str;
use harness_core::event::{
    AgentSpawnedEvent, ProviderRequestFinishedEvent, ProviderRequestStartedEvent, RunFinishedEvent,
    RunStartedEvent, SCHEMA_VERSION,
};
use harness_core::store::{EventEnvelopeWithoutSeqV1, InMemoryEventStore};
use harness_tui::app::{set_pending_live_prompt_draft, AppState, ModelOption};
use std::collections::BTreeMap;
use std::sync::{Mutex, OnceLock};
use std::time::Duration;

fn mock_mode_cwd_test_lock() -> &'static Mutex<()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
}

pub(crate) fn startup_draft_test_lock() -> &'static Mutex<()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
}

fn live_tui_command() -> TuiCommand {
    TuiCommand {
        replay: None,
        continue_session: None,
        scenario: None,
        mock: false,
        deterministic: false,
        session_dir: None,
        exit_on_finish: false,
        profile: None,
    }
}

fn lineage_test_event(seq: u64, payload: EventV1) -> EventEnvelopeV1 {
    EventEnvelopeV1 {
        schema_version: SCHEMA_VERSION,
        event_id: format!("evt_tui_lineage_{seq:04}"),
        seq,
        run_id: "run_tui_lineage_source".into(),
        mono_ms: seq,
        ts: Some(format!("2026-05-03T00:00:{seq:02}Z")),
        actor: EventActor::new(ActorKind::System, Some("tui-lineage-test".to_string())),
        correlation_id: None,
        causation_id: None,
        stream_key: Some("run:run_tui_lineage_source".to_string()),
        payload,
    }
}

fn stable_lineage_test_events() -> Vec<EventEnvelopeV1> {
    vec![
        lineage_test_event(
            1,
            EventV1::RunStarted(RunStartedEvent {
                run_name: "tui lineage source".into(),
                workspace_root: "/workspace".to_string(),
            }),
        ),
        lineage_test_event(
            2,
            EventV1::RunFinished(RunFinishedEvent {
                summary: "stable".to_string(),
            }),
        ),
    ]
}

fn active_stable_lineage_test_events() -> Vec<EventEnvelopeV1> {
    vec![
        lineage_test_event(
            1,
            EventV1::RunStarted(RunStartedEvent {
                run_name: "tui active lineage source".into(),
                workspace_root: "/workspace".to_string(),
            }),
        ),
        lineage_test_event(
            2,
            EventV1::AgentSpawned(AgentSpawnedEvent {
                agent_id: "agent_tui_lineage".to_string(),
                parent_agent_id: None,
                profile: "build".to_string(),
            }),
        ),
        lineage_test_event(
            3,
            EventV1::ProviderRequestStarted(ProviderRequestStartedEvent {
                request_id: "req_000001".into(),
                provider_id: "default".to_string(),
                model_id: "gpt-5.5".to_string(),
                prompt_summary: "first turn".to_string(),
                request_digest: "digest-tui-lineage".to_string(),
                metadata: None,
            }),
        ),
        lineage_test_event(
            4,
            EventV1::ProviderRequestFinished(ProviderRequestFinishedEvent {
                request_id: "req_000001".into(),
                finish_reason: "stop".to_string(),
                output_digest: Some("digest-output".to_string()),
                usage: None,
                metadata: None,
            }),
        ),
    ]
}

fn first_prompt_lineage_test_events() -> Vec<EventEnvelopeV1> {
    vec![
        lineage_test_event(
            1,
            EventV1::RunStarted(RunStartedEvent {
                run_name: "tui first prompt lineage source".into(),
                workspace_root: "/workspace".to_string(),
            }),
        ),
        lineage_test_event(
            2,
            EventV1::AgentSpawned(AgentSpawnedEvent {
                agent_id: "agent_tui_lineage".to_string(),
                parent_agent_id: None,
                profile: "build".to_string(),
            }),
        ),
    ]
}

fn write_recorded_runtime_context_meta(run_dir: &Path) {
    let meta = serde_json::json!({
        "run_id": "run_tui_lineage_source",
        "run_name": "tui lineage source",
        "workspace_root": "/workspace",
        "created_at": "2026-05-04T00:00:00Z",
        "config_digest": "digest-config",
        "harness_version": env!("CARGO_PKG_VERSION"),
        "recorded_runtime_context": {
            "profile": "build",
            "provider": "default",
            "model": "gpt-5.5",
            "variant": null,
            "display_label": "gpt-5.5",
            "token_window_label": null,
            "context_window_tokens": null,
            "max_input_tokens": null,
            "max_output_tokens": null,
            "description": null,
            "recommended_for": null,
            "reasoning_effort": null,
            "text_verbosity": null
        }
    });
    std::fs::write(
        run_dir.join("meta.json"),
        serde_json::to_vec_pretty(&meta).unwrap_or_abort(),
    )
    .unwrap_or_abort();
}

fn catalog_event(run_id: &str, seq: u64, payload: EventV1) -> EventEnvelopeV1 {
    EventEnvelopeV1 {
        schema_version: SCHEMA_VERSION,
        event_id: format!("evt_{run_id}_{seq:04}"),
        seq,
        run_id: run_id.to_string().into(),
        mono_ms: seq,
        ts: Some(format!("2026-05-03T00:01:{seq:02}Z")),
        actor: EventActor::new(ActorKind::System, Some("tui-catalog-test".to_string())),
        correlation_id: None,
        causation_id: None,
        stream_key: Some(format!("run:{run_id}")),
        payload,
    }
}

fn forwarder_event_draft(
    run_id: &str,
    marker: &str,
    payload: EventV1,
) -> EventEnvelopeWithoutSeqV1 {
    EventEnvelopeWithoutSeqV1 {
        schema_version: SCHEMA_VERSION,
        event_id: format!("evt_forwarder_{marker}"),
        run_id: run_id.to_string().into(),
        mono_ms: 0,
        ts: Some("2026-05-03T00:02:00Z".to_string()),
        actor: EventActor::new(ActorKind::System, Some("tui-forwarder-test".to_string())),
        correlation_id: None,
        causation_id: None,
        stream_key: Some(format!("run:{run_id}")),
        payload,
    }
}

fn catalog_events(run_id: &str) -> Vec<EventEnvelopeV1> {
    vec![
        catalog_event(
            run_id,
            1,
            EventV1::RunStarted(RunStartedEvent {
                run_name: run_id.replace('_', " ").into(),
                workspace_root: "/workspace".to_string(),
            }),
        ),
        catalog_event(
            run_id,
            2,
            EventV1::RunFinished(RunFinishedEvent {
                summary: "stable".to_string(),
            }),
        ),
    ]
}

fn write_catalog_run(run_dir: &Path, events: &[EventEnvelopeV1]) {
    std::fs::create_dir_all(run_dir).unwrap_or_abort();
    let body = events
        .iter()
        .map(|event| serde_json::to_string(event).unwrap_or_abort())
        .collect::<Vec<_>>()
        .join("\n");
    std::fs::write(run_dir.join("events.jsonl"), format!("{body}\n")).unwrap_or_abort();
}

#[test]
fn connect_provider_options_seed_from_models_dev_catalog() {
    // arrange
    // act
    // assert
    let catalog =
        harness_core::provider_catalog::ProviderCatalog::from_embedded().unwrap_or_abort();
    let registry = AuthPluginRegistry::with_builtins();

    let providers = connect_provider_options(None, &registry, Some(&catalog));

    assert!(providers.len() > registry.providers().len());
    assert!(providers
        .iter()
        .any(|provider| provider.id.as_str() == "anthropic"));
    assert!(providers
        .iter()
        .any(|provider| provider.id.as_str() == "openai"));
}
