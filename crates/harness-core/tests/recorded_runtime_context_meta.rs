use std::collections::BTreeMap;
use std::fs;
use std::path::Path;
use std::sync::Arc;

use harness_core::agent::AgentProfile;
use harness_core::clock::FakeClock;
use harness_core::config::{
    refresh_profile_model_metadata_registry, HarnessConfig, ToolFailureMode,
};
use harness_core::coord::{spawn_coordinator, CoordinatorConfig, CoordinatorHandle};
use harness_core::event::{
    ActorKind, AgentSpawnedEvent, EventActor, EventEnvelopeV1, EventV1, RunFinishedEvent,
    RunStartedEvent, SCHEMA_VERSION,
};
use harness_core::proj::{
    project_session_catalog_entry, RecordedRuntimeContext, RunMetadata, SessionCatalogMetadata,
    SessionModeSource,
};
use harness_core::redact::DefaultRedactor;

const PROFILE_NAME: &str = "task1_deep";
const MODEL_REF: &str = "default:gpt-5.4-mini";

#[tokio::test]
async fn recorded_runtime_context_meta_roundtrips() {
    refresh_profile_model_metadata_registry(&profile_metadata_config())
        .expect("profile metadata registry should refresh");

    let temp_dir = tempfile::tempdir().expect("tempdir");
    let session_dir = temp_dir.path().join("sessions");
    fs::create_dir_all(&session_dir).expect("create session dir");

    let workspace_root = temp_dir.path().join("workspace");
    fs::create_dir_all(&workspace_root).expect("create workspace root");

    let coordinator = test_coordinator(&session_dir);
    let run = coordinator
        .start_run("interactive", &workspace_root)
        .await
        .expect("start run");
    coordinator
        .spawn_agent_idle(supervisor_actor(), PROFILE_NAME, None)
        .await
        .expect("spawn launch agent");
    coordinator.stop_run().await.expect("stop run");

    let meta_body = fs::read_to_string(run.run_dir.join("meta.json")).expect("read meta.json");
    let metadata: RunMetadata = serde_json::from_str(&meta_body).expect("parse run metadata");
    let expected_context = RecordedRuntimeContext {
        profile: PROFILE_NAME.to_string(),
        profile_description: Some("Deep work".to_string()),
        provider: "default".to_string(),
        provider_display_label: Some("default".to_string()),
        provider_backend_label: Some("OpenAI".to_string()),
        model: "gpt-5.4-mini".to_string(),
        variant: Some("deterministic".to_string()),
        display_label: "GPT-5.4 Mini · Deterministic".to_string(),
        model_display_label: Some("GPT-5.4 Mini".to_string()),
        variant_display_label: Some("Deterministic".to_string()),
        token_window_label: Some("128k ctx · 128k in · 4k out".to_string()),
        context_window_tokens: Some(128000),
        max_input_tokens: Some(128000),
        max_output_tokens: Some(4096),
        description: Some("Deterministic mode".to_string()),
        recommended_for: Some("deep".to_string()),
        reasoning_effort: Some("minimal".to_string()),
        text_verbosity: Some("low".to_string()),
    };

    assert_eq!(metadata.run_id, run.run_id);
    assert_eq!(metadata.run_name, "interactive");
    assert_eq!(
        metadata.workspace_root,
        workspace_root.display().to_string()
    );
    assert_eq!(
        metadata.recorded_runtime_context.as_ref(),
        Some(&expected_context)
    );

    let catalog_metadata: SessionCatalogMetadata =
        serde_json::from_str(&meta_body).expect("parse session catalog metadata");
    assert_eq!(
        catalog_metadata.recorded_runtime_context.as_ref(),
        Some(&expected_context)
    );

    let events = load_events(&run.events_path);
    let entry = project_session_catalog_entry(
        events.iter(),
        &run.run_id,
        Some(&catalog_metadata),
        None,
        None,
    )
    .expect("project session catalog entry");

    assert_eq!(entry.profile_preset.as_deref(), Some(PROFILE_NAME));
    assert_eq!(
        entry.provider_model.as_deref(),
        Some("default/gpt-5.4-mini")
    );
}

#[test]
fn session_catalog_entry_tolerates_legacy_meta_without_runtime_context() {
    let metadata: SessionCatalogMetadata = serde_json::from_str(
        r#"
        {
          "run_id": "run_legacy",
          "run_name": "interactive",
          "workspace_root": "/workspace/legacy",
          "profile_preset": "legacy-profile",
          "provider": "default",
          "model": "gpt-4.1",
          "mode_source": "interactive_live"
        }
        "#,
    )
    .expect("legacy metadata should deserialize");

    assert_eq!(metadata.recorded_runtime_context, None);

    let events = [
        envelope(
            "run_legacy",
            1,
            EventV1::RunStarted(RunStartedEvent {
                run_name: "interactive".to_string(),
                workspace_root: "/workspace/legacy".to_string(),
            }),
        ),
        envelope(
            "run_legacy",
            2,
            EventV1::AgentSpawned(AgentSpawnedEvent {
                agent_id: "agent_000001".to_string(),
                profile: "legacy-profile".to_string(),
                parent_agent_id: None,
            }),
        ),
        envelope(
            "run_legacy",
            3,
            EventV1::RunFinished(RunFinishedEvent {
                summary: "finished".to_string(),
            }),
        ),
    ];

    let entry = project_session_catalog_entry(
        events.iter(),
        "run_legacy",
        Some(&metadata),
        Some("2026-03-25T00:00:00Z".to_string()),
        None,
    )
    .expect("legacy metadata should still project cleanly");

    assert_eq!(entry.run_id, "run_legacy");
    assert_eq!(entry.profile_preset.as_deref(), Some("legacy-profile"));
    assert_eq!(entry.provider_model.as_deref(), Some("default/gpt-4.1"));
    assert_eq!(entry.mode_source, SessionModeSource::InteractiveLive);
}

fn profile_metadata_config() -> HarnessConfig {
    json5::from_str(&format!(
        r#"
        {{
          providers: {{
            default: {{
              type: "openai_compatible",
              base_url: "http://127.0.0.1:8317/v1",
              api_key: "test-key",
              api_mode: "responses",
              timeout_ms: 60000,
              models: {{
                "gpt-5.4-mini": {{
                  display_name: "GPT-5.4 Mini",
                  metadata: {{
                    context_window_tokens: 128000,
                  }},
                  max_input_tokens: 128000,
                  max_output_tokens: 8192,
                  variants: {{
                    deterministic: {{
                      display_name: "Deterministic",
                      max_output_tokens: 4096,
                      metadata: {{
                        description: "Deterministic mode",
                        reasoning_effort: "minimal",
                        text_verbosity: "low",
                        recommended_for: "deep",
                      }},
                    }},
                  }},
                }},
              }},
            }},
          }},
          agents: {{
            {PROFILE_NAME}: {{
              description: "Deep work",
              model_ref: "{MODEL_REF}",
              variant: "deterministic",
              tools: ["fs.read"],
            }},
          }},
          permissions: {{
            defaults: {{
              edit: "ask",
              shell: "ask",
              network: "deny",
            }},
          }},
          runtime: {{
            background_tasks: {{
              default_concurrency: 2,
              provider_concurrency: 2,
              model_concurrency: 2,
              stale_timeout_ms: 15000,
              message_staleness_timeout_ms: 5000,
            }},
            session_dir: ".agent-harness/sessions",
          }},
          integrations: {{
            remote_search: {{
              endpoint: "https://mcp.exa.ai/mcp",
            }},
          }},
        }}
        "#
    ))
    .expect("config shape should deserialize")
}

fn test_coordinator(session_dir: &Path) -> CoordinatorHandle {
    let mut config = CoordinatorConfig::new(session_dir);
    config.deterministic_store = true;
    config.config_digest = "config-digest".to_string();
    config.harness_version = "test-version".to_string();
    config.agent_profiles = agent_profiles();

    spawn_coordinator(
        config,
        Arc::new(FakeClock::new()),
        Arc::new(DefaultRedactor::default()),
    )
}

fn agent_profiles() -> BTreeMap<String, AgentProfile> {
    let mut profiles = BTreeMap::new();
    profiles.insert(
        PROFILE_NAME.to_string(),
        AgentProfile {
            name: PROFILE_NAME.to_string(),
            category: "deep".to_string(),
            model_ref: MODEL_REF.to_string(),
            system_prompt: "system prompt".to_string(),
            max_iters: 12,
            temperature: Some(0.0),
            tool_failure_mode: ToolFailureMode::FailTurn,
            toolset: Vec::new(),
        },
    );
    profiles
}

fn supervisor_actor() -> EventActor {
    EventActor::new(ActorKind::Supervisor, Some("agent-supervisor".to_string()))
}

fn load_events(path: &Path) -> Vec<EventEnvelopeV1> {
    let body = fs::read_to_string(path).expect("read events");
    body.lines()
        .map(|line| serde_json::from_str::<EventEnvelopeV1>(line).expect("parse event"))
        .collect()
}

fn envelope(run_id: &str, seq: u64, payload: EventV1) -> EventEnvelopeV1 {
    EventEnvelopeV1 {
        schema_version: SCHEMA_VERSION,
        event_id: format!("evt-{seq:04}"),
        seq,
        run_id: run_id.to_string(),
        mono_ms: seq,
        ts: None,
        actor: EventActor::new(ActorKind::System, Some("coordinator".to_string())),
        correlation_id: None,
        causation_id: None,
        stream_key: Some(format!("run:{run_id}")),
        payload,
    }
}
