use harness_core::UnwrapOrAbort;
use std::collections::BTreeMap;
use std::fs;
use std::path::Path;
use std::sync::Arc;

use harness_core::agent::AgentProfile;
use harness_core::auto_fallback::take_next_fallback_model_target;
use harness_core::clock::FakeClock;
use harness_core::config::{
    refresh_profile_model_metadata_registry, resolve_model_selection, HarnessConfig,
    MaxInputSemantics, ModelLimitProvenance, ResolvedModelLimit, ResolvedModelLimits,
    ToolFailureMode,
};
use harness_core::coord::{spawn_coordinator, CoordinatorConfig, CoordinatorHandle};
use harness_core::event::{
    ActorKind, AgentSpawnedEvent, EventActor, EventEnvelopeV1, EventV1, RunFinishedEvent,
    RunStartedEvent, SCHEMA_VERSION,
};
use harness_core::proj::{
    load_run_metadata, project_session_catalog_entry, RecordedRuntimeContext, RunMetadata,
    SessionCatalogMetadata, SessionModeSource,
};
use harness_core::redact::DefaultRedactor;

#[path = "recorded_runtime_context_meta/budget_contract_test.rs"]
mod budget_contract_test;
mod common;

use common::{load_events, supervisor_actor_with_id};

const PROFILE_NAME: &str = "task1_deep";
const MODEL_REF: &str = "default:gpt-5.4-mini";

#[test]
fn legacy_model_limit_mirrors_are_not_written_for_canonical_targets() {
    // arrange: a selected model with authoritative canonical limits.
    let config = profile_metadata_config();
    refresh_profile_model_metadata_registry(&config).unwrap_or_abort();
    let target = resolve_model_selection(&config, MODEL_REF, Some("deterministic"))
        .unwrap_or_abort()
        .primary;
    let recorded = RecordedRuntimeContext::from_model_target(PROFILE_NAME, &target);

    // act: new runtime metadata is serialized.
    let value = serde_json::to_value(recorded).unwrap_or_abort();

    // assert: canonical limits are written once and all pre-M03 scalar mirrors are omitted.
    assert!(value.get("model_limits").is_some());
    for mirror in [
        "context_window_tokens",
        "max_input_tokens",
        "max_output_tokens",
    ] {
        assert!(
            value.get(mirror).is_none(),
            "scalar mirror `{mirror}` was written"
        );
    }
}

#[tokio::test]
async fn recorded_runtime_context_meta_roundtrips() {
    // arrange
    // act
    // assert
    refresh_profile_model_metadata_registry(&profile_metadata_config()).unwrap_or_abort();

    let temp_dir = tempfile::tempdir().unwrap_or_abort();
    let session_dir = temp_dir.path().join("sessions");
    fs::create_dir_all(&session_dir).unwrap_or_abort();

    let workspace_root = temp_dir.path().join("workspace");
    fs::create_dir_all(&workspace_root).unwrap_or_abort();

    let coordinator = test_coordinator(&session_dir);
    let run = coordinator
        .start_run("interactive", &workspace_root)
        .await
        .unwrap_or_abort();
    coordinator
        .spawn_agent_idle(
            supervisor_actor_with_id("agent-supervisor"),
            PROFILE_NAME,
            None,
        )
        .await
        .unwrap_or_abort();
    coordinator.stop_run().await.unwrap_or_abort();

    let meta_body = fs::read_to_string(run.run_dir.join("meta.json")).unwrap_or_abort();
    let metadata: RunMetadata = serde_json::from_str(&meta_body).unwrap_or_abort();
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
        last_request_budget: None,
        model_limits: ResolvedModelLimits {
            context_window: ResolvedModelLimit {
                tokens: Some(128000),
                provenance: ModelLimitProvenance::explicit("model configuration"),
            },
            max_input: ResolvedModelLimit {
                tokens: Some(128000),
                provenance: ModelLimitProvenance::explicit("model configuration"),
            },
            max_output: ResolvedModelLimit {
                tokens: Some(4096),
                provenance: ModelLimitProvenance::explicit(
                    "model `default:gpt-5.4-mini` variant `deterministic`",
                ),
            },
            max_input_semantics: MaxInputSemantics::ProviderVisibleInputTokens,
        },
        context_window_tokens: None,
        max_input_tokens: None,
        max_output_tokens: None,
        description: Some("Deterministic mode".to_string()),
        recommended_for: Some("deep".to_string()),
        reasoning_effort: Some("minimal".to_string()),
        text_verbosity: Some("low".to_string()),
        thinking: None,
    };

    assert_eq!(metadata.run_id, run.run_id.as_str());
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
        serde_json::from_str(&meta_body).unwrap_or_abort();
    assert_eq!(
        catalog_metadata.recorded_runtime_context.as_ref(),
        Some(&expected_context)
    );

    let events = load_events(&run.events_path);
    let entry = project_session_catalog_entry(
        events.iter(),
        run.run_id.as_str(),
        Some(&catalog_metadata),
        None,
        None,
    )
    .unwrap_or_abort();

    assert_eq!(entry.profile_preset.as_deref(), Some(PROFILE_NAME));
    assert_eq!(
        entry.provider_model.as_deref(),
        Some("default/gpt-5.4-mini")
    );
}

#[tokio::test]
async fn active_typed_primary_and_different_model_targets_reach_recorded_context() {
    // arrange
    let config = profile_metadata_config();
    let primary = resolve_model_selection(&config, MODEL_REF, Some("deterministic"))
        .unwrap_or_abort()
        .primary;
    let mut different = primary.clone();
    different.model_ref = "other:distinct".to_string();
    different.provider = "other".to_string();
    different.model = "distinct".to_string();
    different.variant = None;
    different.limits =
        ResolvedModelLimits::compatibility_mirror(Some(64_000), Some(48_000), Some(8_000));

    for target in [primary, different] {
        let temp_dir = tempfile::tempdir().unwrap_or_abort();
        let session_dir = temp_dir.path().join("sessions");
        let workspace_root = temp_dir.path().join("workspace");
        fs::create_dir_all(&session_dir).unwrap_or_abort();
        fs::create_dir_all(&workspace_root).unwrap_or_abort();
        let mut coordinator_config = CoordinatorConfig::new(&session_dir);
        coordinator_config.deterministic_store = true;
        coordinator_config.agent_profiles = agent_profiles();
        coordinator_config
            .agent_model_targets
            .insert(PROFILE_NAME.to_string(), target.clone());
        let coordinator = spawn_coordinator(
            coordinator_config,
            Arc::new(FakeClock::new()),
            Arc::new(DefaultRedactor::default()),
        );

        // act
        let run = coordinator
            .start_run("typed-target", &workspace_root)
            .await
            .unwrap_or_abort();
        coordinator
            .spawn_agent_idle(
                supervisor_actor_with_id("agent-supervisor"),
                PROFILE_NAME,
                None,
            )
            .await
            .unwrap_or_abort();
        coordinator.stop_run().await.unwrap_or_abort();
        let metadata: RunMetadata = serde_json::from_str(
            &fs::read_to_string(run.run_dir.join("meta.json")).unwrap_or_abort(),
        )
        .unwrap_or_abort();
        let recorded = metadata.recorded_runtime_context.unwrap_or_abort();

        // assert
        assert_eq!(recorded.provider, target.provider);
        assert_eq!(recorded.model, target.model);
        assert_eq!(recorded.variant, target.variant);
        assert_eq!(recorded.reasoning_effort, target.reasoning_effort);
        assert_eq!(recorded.model_limits, target.limits);
    }
}

#[test]
fn model_target_recording_preserves_matching_rich_metadata_and_clears_stale_fallback_fields() {
    // arrange
    let config = profile_metadata_config();
    refresh_profile_model_metadata_registry(&config).unwrap_or_abort();
    let primary = resolve_model_selection(&config, MODEL_REF, Some("deterministic"))
        .unwrap_or_abort()
        .primary;
    let mut fallback = primary.clone();
    fallback.model_ref = "other:distinct".to_string();
    fallback.provider = "other".to_string();
    fallback.model = "distinct".to_string();
    fallback.variant = None;
    fallback.reasoning_effort = Some("high".to_string());
    fallback.limits =
        ResolvedModelLimits::compatibility_mirror(Some(64_000), Some(48_000), Some(8_000));

    // act
    let recorded_primary = RecordedRuntimeContext::from_model_target(PROFILE_NAME, &primary);
    let recorded_fallback = RecordedRuntimeContext::from_model_target(PROFILE_NAME, &fallback);

    // assert
    assert_eq!(
        recorded_primary.provider_display_label.as_deref(),
        Some("default")
    );
    assert_eq!(
        recorded_primary.provider_backend_label.as_deref(),
        Some("OpenAI")
    );
    assert_eq!(
        recorded_primary.model_display_label.as_deref(),
        Some("GPT-5.4 Mini")
    );
    assert_eq!(
        recorded_primary.variant_display_label.as_deref(),
        Some("Deterministic")
    );
    assert_eq!(
        recorded_primary.token_window_label.as_deref(),
        Some("128k ctx · 128k in · 4k out")
    );
    assert_eq!(
        recorded_primary.description.as_deref(),
        Some("Deterministic mode")
    );
    assert_eq!(
        recorded_fallback.profile_description.as_deref(),
        Some("Deep work")
    );
    assert_eq!(recorded_fallback.provider, "other");
    assert_eq!(recorded_fallback.model, "distinct");
    assert_eq!(
        recorded_fallback.model_display_label.as_deref(),
        Some("distinct")
    );
    assert_eq!(recorded_fallback.description, None);
    assert_eq!(recorded_fallback.recommended_for, None);
    assert_eq!(recorded_fallback.model_limits, fallback.limits);
}

#[tokio::test]
async fn selected_same_model_variant_rich_metadata_roundtrips_recorded_context() {
    // arrange
    let config = profile_metadata_config();
    refresh_profile_model_metadata_registry(&config).unwrap_or_abort();
    let selected = resolve_model_selection(&config, MODEL_REF, Some("high"))
        .unwrap_or_abort()
        .primary;
    let temp_dir = tempfile::tempdir().unwrap_or_abort();
    let session_dir = temp_dir.path().join("sessions");
    let workspace_root = temp_dir.path().join("workspace");
    fs::create_dir_all(&session_dir).unwrap_or_abort();
    fs::create_dir_all(&workspace_root).unwrap_or_abort();
    let mut coordinator_config = CoordinatorConfig::new(&session_dir);
    coordinator_config.deterministic_store = true;
    coordinator_config.agent_profiles = agent_profiles();
    coordinator_config
        .agent_model_targets
        .insert(PROFILE_NAME.to_string(), selected);
    let coordinator = spawn_coordinator(
        coordinator_config,
        Arc::new(FakeClock::new()),
        Arc::new(DefaultRedactor::default()),
    );

    // act
    let run = coordinator
        .start_run("selected-high-variant", &workspace_root)
        .await
        .unwrap_or_abort();
    coordinator
        .spawn_agent_idle(
            supervisor_actor_with_id("agent-supervisor"),
            PROFILE_NAME,
            None,
        )
        .await
        .unwrap_or_abort();
    coordinator.stop_run().await.unwrap_or_abort();
    let metadata: RunMetadata =
        serde_json::from_str(&fs::read_to_string(run.run_dir.join("meta.json")).unwrap_or_abort())
            .unwrap_or_abort();
    let recorded = metadata.recorded_runtime_context.unwrap_or_abort();
    let replayed: RecordedRuntimeContext =
        serde_json::from_str(&serde_json::to_string(&recorded).unwrap_or_abort()).unwrap_or_abort();

    // assert
    assert_eq!(replayed.variant.as_deref(), Some("high"));
    assert_eq!(replayed.variant_display_label.as_deref(), Some("High"));
    assert_eq!(
        replayed.token_window_label.as_deref(),
        Some("128k ctx · 128k in · 6k out")
    );
    assert_eq!(replayed.description.as_deref(), Some("High reasoning mode"));
    assert_eq!(replayed.recommended_for.as_deref(), Some("review"));
}

#[test]
fn session_catalog_entry_tolerates_legacy_meta_without_runtime_context() {
    // arrange
    // act
    // assert
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
    .unwrap_or_abort();

    assert_eq!(metadata.recorded_runtime_context, None);

    let events = [
        envelope(
            "run_legacy",
            1,
            EventV1::RunStarted(RunStartedEvent {
                run_name: "interactive".into(),
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
    .unwrap_or_abort();

    assert_eq!(entry.run_id, "run_legacy");
    assert_eq!(entry.profile_preset.as_deref(), Some("legacy-profile"));
    assert_eq!(entry.provider_model.as_deref(), Some("default/gpt-4.1"));
    assert_eq!(entry.mode_source, SessionModeSource::InteractiveLive);
}

#[test]
fn legacy_model_limit_mirrors_reconstruct_compatibility_limits() {
    // arrange: a pre-M03 meta.json containing only scalar model-limit mirrors.
    let temp_dir = tempfile::tempdir().unwrap_or_abort();
    fs::write(
        temp_dir.path().join("meta.json"),
        r#"{
          "run_id":"run_legacy","run_name":"interactive","workspace_root":"/workspace",
          "config_digest":"digest","harness_version":"test",
          "recorded_runtime_context":{
            "profile":"legacy","provider":"default","model":"legacy-model",
            "variant":null,"display_label":"Legacy","token_window_label":null,
            "context_window_tokens":128000,"max_input_tokens":96000,
            "max_output_tokens":16000,"description":null,"recommended_for":null,
            "reasoning_effort":null,"text_verbosity":null
          }
        }"#,
    )
    .unwrap_or_abort();

    // act: the canonical metadata loader hydrates the compatibility input.
    let metadata = load_run_metadata(temp_dir.path()).unwrap_or_abort();
    let context = metadata.recorded_runtime_context.unwrap_or_abort();
    let limits = context.effective_model_limits();
    let serialized = serde_json::to_value(&context).unwrap_or_abort();

    // assert: canonical limits survive and legacy mirrors are cleared from future writes.
    assert_eq!(limits.context_window_tokens(), Some(128_000));
    assert_eq!(limits.max_input_tokens(), Some(96_000));
    assert_eq!(limits.max_output_tokens(), Some(16_000));
    assert_eq!(
        limits.context_window.provenance.kind,
        harness_core::config::ModelLimitProvenanceKind::CompatibilityFallback
    );
    for mirror in [
        "context_window_tokens",
        "max_input_tokens",
        "max_output_tokens",
    ] {
        assert!(
            serialized.get(mirror).is_none(),
            "scalar mirror `{mirror}` was rewritten"
        );
    }
}

#[test]
fn fallback_queue_preserves_the_complete_target_for_runtime_recording() {
    // arrange
    let config = profile_metadata_config();
    let mut fallback = resolve_model_selection(&config, MODEL_REF, Some("deterministic"))
        .unwrap_or_abort()
        .primary;
    fallback.model_ref = "other:fallback".to_string();
    fallback.provider = "other".to_string();
    fallback.model = "fallback".to_string();
    fallback.limits =
        ResolvedModelLimits::compatibility_mirror(Some(96_000), Some(80_000), Some(12_000));
    let mut queue = vec![fallback.clone()];

    // act
    let active = take_next_fallback_model_target(&mut queue).unwrap_or_abort();
    let recorded = RecordedRuntimeContext::from_model_target(PROFILE_NAME, &active);

    // assert
    assert!(queue.is_empty());
    assert_eq!(active, fallback);
    assert_eq!(recorded.provider, "other");
    assert_eq!(recorded.model, "fallback");
    assert_eq!(recorded.model_limits, fallback.limits);
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
                    high: {{
                      display_name: "High",
                      max_output_tokens: 6144,
                      metadata: {{
                        description: "High reasoning mode",
                        reasoning_effort: "high",
                        recommended_for: "review",
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
              model_ref_explicit: true,
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
    .unwrap_or_abort()
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
            model_ref: MODEL_REF.to_string(),
            model_ref_explicit: true,
            system_prompt: "system prompt".to_string(),
            cache_retention: Default::default(),
            max_iters: Some(12),
            temperature: Some(0.0),
            tool_failure_mode: ToolFailureMode::FailTurn,
            toolset: Vec::new(),
            permission_ruleset: Vec::new(),
        },
    );
    profiles
}

fn envelope(run_id: &str, seq: u64, payload: EventV1) -> EventEnvelopeV1 {
    EventEnvelopeV1 {
        schema_version: SCHEMA_VERSION,
        event_id: format!("evt-{seq:04}"),
        seq,
        run_id: run_id.to_string().into(),
        mono_ms: seq,
        ts: None,
        actor: EventActor::new(ActorKind::System, Some("coordinator".to_string())),
        correlation_id: None,
        causation_id: None,
        stream_key: Some(format!("run:{run_id}")),
        payload,
    }
}
