use super::*;
use harness::UnwrapOrAbort;

#[test]
fn no_config_tui_without_credentials_enters_connect_state() {
    // arrange
    // act
    // assert
    let temp = tempfile::tempdir().unwrap_or_abort();

    let settings = resolve_live_settings_for_test(
        &live_tui_command(),
        None,
        None,
        temp.path().to_path_buf(),
        &harness_core::config::ConfigLoadContext::from_env()
            .with_current_dir(temp.path().to_path_buf()),
        LiveSettingsDeps {
            credential_store: None,
            env_lookup: &|_| None,
            model_selection_path: None,
        },
    )
    .unwrap_or_abort();

    assert!(settings.config.is_some());
    assert_eq!(settings.launch_metadata.provider(), "local");
    assert_eq!(settings.launch_metadata.model(), None);
    assert!(settings.launch_metadata.available_models().is_empty());
}

#[test]
fn no_config_tui_with_stored_codex_launches_connected_catalog() {
    // arrange
    // act
    // assert
    let temp = tempfile::tempdir().unwrap_or_abort();
    let data_home = temp.path().join("data");
    let store = CredentialStore::new(data_home.join("harness"));
    store
        .save(&StoredCredential::api_key(
            AuthProviderId::codex(),
            "test-token",
            SystemCredentialClock.now_rfc3339(),
        ))
        .unwrap_or_abort();

    let settings = resolve_live_settings_for_test(
        &live_tui_command(),
        None,
        None,
        temp.path().to_path_buf(),
        &harness_core::config::ConfigLoadContext::from_env()
            .with_current_dir(temp.path().to_path_buf()),
        LiveSettingsDeps {
            credential_store: Some(&store),
            env_lookup: &|_| None,
            model_selection_path: None,
        },
    )
    .unwrap_or_abort();

    assert_eq!(settings.launch_metadata.provider(), "openai-codex");
    assert!(settings.launch_metadata.model().is_some());
    assert!(settings
        .launch_metadata
        .available_models()
        .iter()
        .all(|option| option.provider == "openai-codex"));
}

#[test]
fn auth_refresh_reloads_no_config_builtin_catalog_after_login() {
    // arrange
    // act
    // assert
    let temp = tempfile::tempdir().unwrap_or_abort();
    let data_home = temp.path().join("data");
    let store = CredentialStore::new(data_home.join("harness"));
    store
        .save(&StoredCredential::api_key(
            AuthProviderId::github_copilot(),
            "test-token",
            SystemCredentialClock.now_rfc3339(),
        ))
        .unwrap_or_abort();

    let settings = resolve_live_settings_for_test(
        &live_tui_command(),
        None,
        None,
        temp.path().to_path_buf(),
        &harness_core::config::ConfigLoadContext::from_env()
            .with_current_dir(temp.path().to_path_buf()),
        LiveSettingsDeps {
            credential_store: Some(&store),
            env_lookup: &|_| None,
            model_selection_path: None,
        },
    )
    .unwrap_or_abort();
    let launch_metadata = settings.launch_metadata;

    assert_eq!(launch_metadata.provider(), "github-copilot");
    assert!(launch_metadata.model().is_some());
    assert!(launch_metadata
        .available_models()
        .iter()
        .all(|option| option.provider == "github-copilot"));
}

#[test]
fn no_config_tui_ignores_legacy_builtin_model_selection() {
    // arrange
    // act
    // assert
    let temp = tempfile::tempdir().unwrap_or_abort();
    let data_home = temp.path().join("data");
    let state_path = temp.path().join("model.json");
    let store = CredentialStore::new(data_home.join("harness"));
    store
        .save(&StoredCredential::api_key(
            AuthProviderId::codex(),
            "test-token",
            SystemCredentialClock.now_rfc3339(),
        ))
        .unwrap_or_abort();
    std::fs::write(
        &state_path,
        r#"{"schema_version":1,"profile":"build","provider":"openai-codex","model":"gpt-5.5"}"#,
    )
    .unwrap_or_abort();

    let settings = resolve_live_settings_for_test(
        &live_tui_command(),
        None,
        None,
        temp.path().to_path_buf(),
        &harness_core::config::ConfigLoadContext::from_env()
            .with_current_dir(temp.path().to_path_buf()),
        LiveSettingsDeps {
            credential_store: Some(&store),
            env_lookup: &|_| None,
            model_selection_path: Some(&state_path),
        },
    )
    .unwrap_or_abort();

    assert_eq!(settings.launch_metadata.provider(), "openai-codex");
    assert_eq!(settings.launch_metadata.model(), Some("gpt-5.4-mini"));
}

#[test]
fn project_config_tui_ignores_legacy_model_selection() {
    // arrange
    // act
    // assert
    let temp = tempfile::tempdir().unwrap_or_abort();
    let config_path = temp.path().join("harness.jsonc");
    let state_path = temp.path().join("model.json");
    std::fs::write(
        &config_path,
        r#"{
          provider: {
            "openai-codex": {
              type: "openai_compatible",
              options: {
                authProvider: "codex",
                baseURL: "https://api.openai.com/v1",
                apiKeyEnv: ["OPENAI_API_KEY"],
              },
              models: {
                "gpt-5.4-mini": { name: "GPT 5.4 Mini" },
                "gpt-5.5": { name: "GPT 5.5" },
              },
            },
          },
          model: "openai-codex/gpt-5.4-mini",
          agent: {
            default: { model: "openai-codex/gpt-5.4-mini" },
          },
          permission: "ask",
        }"#,
    )
    .unwrap_or_abort();
    std::fs::write(
        &state_path,
        r#"{"schema_version":1,"profile":"build","provider":"openai-codex","model":"gpt-5.5"}"#,
    )
    .unwrap_or_abort();

    let result = resolve_live_settings_for_test(
        &live_tui_command(),
        Some(config_path),
        None,
        temp.path().to_path_buf(),
        &harness_core::config::ConfigLoadContext::from_env()
            .with_current_dir(temp.path().to_path_buf()),
        LiveSettingsDeps {
            credential_store: None,
            env_lookup: &|name| (name == "OPENAI_API_KEY").then(|| "test-token".to_string()),
            model_selection_path: Some(&state_path),
        },
    );

    let settings = result.unwrap_or_abort();
    assert_eq!(settings.launch_metadata.provider(), "openai-codex");
    assert_eq!(settings.launch_metadata.model(), Some("gpt-5.4-mini"));
}

#[test]
fn mock_mode_ignores_discovered_cwd_config() {
    // arrange
    // act
    // assert
    let _guard = mock_mode_cwd_test_lock().lock().unwrap_or_abort();
    let temp = tempfile::tempdir().unwrap_or_abort();
    std::fs::write(
        temp.path().join("harness.jsonc"),
        r#"{
          providers: {
            default: {
              type: "openai_compatible",
              base_url: "http://127.0.0.1:8317/v1",
              api_key: "test-key",
              api_mode: "responses",
              timeout_ms: 60000,
              models: {
                "gpt-5.4-mini": {
                  display_name: "GPT-5.4 Mini"
                }
              }
            }
          },
          model: "default/gpt-5.4-mini",
          agent: {
            default: {
              system_prompt: "Implement carefully.",
              model: "default/gpt-5.4-mini",
              tools: []
            }
          },
          permissions: {
            defaults: {
              edit: "allow",
              shell: "allow",
              network: "allow"
            }
          },
          runtime: {
            background_tasks: {
              default_concurrency: 2,
              provider_concurrency: 2,
              model_concurrency: 2,
              stale_timeout_ms: 15000,
              message_staleness_timeout_ms: 5000
            },
            session_dir: ".agent-harness/sessions"
          },
          integrations: {
            remote_search: {
              endpoint: "https://mcp.exa.ai/mcp"
            }
          }
        }"#,
    )
    .unwrap_or_abort();

    let result = resolve_live_settings(
        &TuiCommand {
            replay: None,
            continue_session: None,
            scenario: None,
            mock: true,
            deterministic: false,
            session_dir: None,
            exit_on_finish: false,
            profile: None,
            no_alt_screen: false,
            minimal: false,
            fullscreen: false,
        },
        None,
        None,
        temp.path().to_path_buf(),
        &harness_core::config::ConfigLoadContext::from_env()
            .with_current_dir(temp.path().to_path_buf()),
    );

    let settings = result.unwrap_or_abort();
    assert!(settings.config.is_none());
    assert_eq!(settings.launch_mode_label.as_deref(), Some("Demo"));
    assert_eq!(settings.launch_metadata.profile(), "default");
    assert_eq!(settings.launch_metadata.provider(), "mock");
    assert_eq!(settings.launch_metadata.model(), Some("model-1"));
}

#[test]
fn live_new_session_uses_current_workspace_instead_of_seeded_demo_workspace() {
    // arrange
    // act
    // assert
    let _guard = mock_mode_cwd_test_lock().lock().unwrap_or_abort();
    let temp = tempfile::tempdir().unwrap_or_abort();
    let config_path = temp.path().join("harness.jsonc");
    std::fs::write(
        &config_path,
        r#"{
          providers: {
            default: {
              type: "openai_compatible",
              base_url: "http://127.0.0.1:8317/v1",
              api_key: "test-key",
              api_mode: "responses",
              timeout_ms: 60000,
              models: {
                "gpt-5.4-mini": {
                  display_name: "GPT-5.4 Mini"
                }
              }
            }
          },
          model: "default/gpt-5.4-mini",
          agent: {
            default: {
              system_prompt: "Implement carefully.",
              model: "default/gpt-5.4-mini",
              tools: []
            }
          },
          permissions: {
            defaults: {
              edit: "allow",
              shell: "allow",
              network: "allow"
            }
          },
          runtime: {
            background_tasks: {
              default_concurrency: 2,
              provider_concurrency: 2,
              model_concurrency: 2,
              stale_timeout_ms: 15000,
              message_staleness_timeout_ms: 5000
            },
            session_dir: ".agent-harness/sessions"
          },
          integrations: {
            remote_search: {
              endpoint: "https://mcp.exa.ai/mcp"
            }
          }
        }"#,
    )
    .unwrap_or_abort();

    let result = resolve_live_settings(
        &TuiCommand {
            replay: None,
            continue_session: None,
            scenario: None,
            mock: false,
            deterministic: false,
            session_dir: None,
            exit_on_finish: false,
            profile: None,
            no_alt_screen: false,
            minimal: false,
            fullscreen: false,
        },
        Some(config_path.clone()),
        None,
        temp.path().to_path_buf(),
        &harness_core::config::ConfigLoadContext::from_env()
            .with_current_dir(temp.path().to_path_buf()),
    );

    let settings = result.unwrap_or_abort();
    let workspace = prepare_new_live_workspace(&settings, false, "run_test").unwrap_or_abort();

    assert_eq!(settings.launch_mode_label, None);
    assert_eq!(workspace, temp.path());
    assert!(!workspace.join("demo.txt").exists());
    assert!(!settings
        .session_dir
        .join("workspaces")
        .join("golden_path_interactive-run_test")
        .exists());
}

#[test]
fn continue_selects_most_recent_conversational_agent_not_first_key() {
    // arrange
    // act
    // assert
    let mut known_agents = BTreeMap::new();
    known_agents.insert("agent_000001".to_string(), "alpha".to_string());
    known_agents.insert("agent_000002".to_string(), "beta".to_string());

    let historical_events = vec![
        EventEnvelopeV1 {
            schema_version: 1,
            event_id: "evt-0001".to_string(),
            seq: 1,
            run_id: "run_fixture".into(),
            mono_ms: 1,
            ts: None,
            actor: EventActor::new(ActorKind::System, Some("coordinator".to_string())),
            correlation_id: None,
            causation_id: None,
            stream_key: Some("run:run_fixture".to_string()),
            payload: EventV1::AgentSpawned(AgentSpawnedEvent {
                agent_id: "agent_000001".to_string(),
                profile: "alpha".to_string(),
                parent_agent_id: None,
            }),
        },
        EventEnvelopeV1 {
            schema_version: 1,
            event_id: "evt-0002".to_string(),
            seq: 2,
            run_id: "run_fixture".into(),
            mono_ms: 2,
            ts: None,
            actor: EventActor::new(ActorKind::System, Some("coordinator".to_string())),
            correlation_id: None,
            causation_id: None,
            stream_key: Some("run:run_fixture".to_string()),
            payload: EventV1::AgentSpawned(AgentSpawnedEvent {
                agent_id: "agent_000002".to_string(),
                profile: "beta".to_string(),
                parent_agent_id: None,
            }),
        },
        EventEnvelopeV1 {
            schema_version: 1,
            event_id: "evt-0003".to_string(),
            seq: 3,
            run_id: "run_fixture".into(),
            mono_ms: 3,
            ts: None,
            actor: EventActor::new(ActorKind::Worker, Some("agent_000001".to_string())),
            correlation_id: Some("req_000010".to_string()),
            causation_id: None,
            stream_key: Some("agent:agent_000001".to_string()),
            payload: EventV1::ProviderRequestStarted(ProviderRequestStartedEvent {
                request_id: "req_000010".into(),
                provider_id: "mock".to_string(),
                model_id: "model-a".to_string(),
                prompt_summary: "first".to_string(),
                request_digest: "digest-a".to_string(),
                metadata: None,
            }),
        },
        EventEnvelopeV1 {
            schema_version: 1,
            event_id: "evt-0004".to_string(),
            seq: 4,
            run_id: "run_fixture".into(),
            mono_ms: 4,
            ts: None,
            actor: EventActor::new(ActorKind::Worker, Some("agent_000002".to_string())),
            correlation_id: Some("req_000011".to_string()),
            causation_id: None,
            stream_key: Some("agent:agent_000002".to_string()),
            payload: EventV1::ProviderRequestStarted(ProviderRequestStartedEvent {
                request_id: "req_000011".into(),
                provider_id: "mock".to_string(),
                model_id: "model-b".to_string(),
                prompt_summary: "second".to_string(),
                request_digest: "digest-b".to_string(),
                metadata: None,
            }),
        },
    ];

    let selected = most_recent_conversational_agent_id(&historical_events, &known_agents);
    assert_eq!(selected.as_deref(), Some("agent_000002"));
}

#[test]
fn continue_metadata_uses_selected_agent_history_in_multi_agent_session() {
    // arrange
    // act
    // assert
    let historical_events = vec![
        EventEnvelopeV1 {
            schema_version: 1,
            event_id: "evt-0001".to_string(),
            seq: 1,
            run_id: "run_fixture".into(),
            mono_ms: 1,
            ts: None,
            actor: EventActor::new(ActorKind::System, Some("coordinator".to_string())),
            correlation_id: None,
            causation_id: None,
            stream_key: Some("run:run_fixture".to_string()),
            payload: EventV1::AgentSpawned(AgentSpawnedEvent {
                agent_id: "agent_000001".to_string(),
                profile: "alpha".to_string(),
                parent_agent_id: None,
            }),
        },
        EventEnvelopeV1 {
            schema_version: 1,
            event_id: "evt-0002".to_string(),
            seq: 2,
            run_id: "run_fixture".into(),
            mono_ms: 2,
            ts: None,
            actor: EventActor::new(ActorKind::System, Some("coordinator".to_string())),
            correlation_id: None,
            causation_id: None,
            stream_key: Some("run:run_fixture".to_string()),
            payload: EventV1::AgentSpawned(AgentSpawnedEvent {
                agent_id: "agent_000002".to_string(),
                profile: "beta".to_string(),
                parent_agent_id: None,
            }),
        },
        EventEnvelopeV1 {
            schema_version: 1,
            event_id: "evt-0003".to_string(),
            seq: 3,
            run_id: "run_fixture".into(),
            mono_ms: 3,
            ts: None,
            actor: EventActor::new(ActorKind::Worker, Some("agent_000001".to_string())),
            correlation_id: Some("req_000010".to_string()),
            causation_id: None,
            stream_key: Some("agent:agent_000001".to_string()),
            payload: EventV1::ProviderRequestStarted(ProviderRequestStartedEvent {
                request_id: "req_000010".into(),
                provider_id: "provider-alpha".to_string(),
                model_id: "model-alpha".to_string(),
                prompt_summary: "alpha turn".to_string(),
                request_digest: "digest-alpha".to_string(),
                metadata: None,
            }),
        },
        EventEnvelopeV1 {
            schema_version: 1,
            event_id: "evt-0004".to_string(),
            seq: 4,
            run_id: "run_fixture".into(),
            mono_ms: 4,
            ts: None,
            actor: EventActor::new(ActorKind::Worker, Some("agent_000002".to_string())),
            correlation_id: Some("req_000011".to_string()),
            causation_id: None,
            stream_key: Some("agent:agent_000002".to_string()),
            payload: EventV1::ProviderRequestStarted(ProviderRequestStartedEvent {
                request_id: "req_000011".into(),
                provider_id: "provider-beta".to_string(),
                model_id: "model-beta".to_string(),
                prompt_summary: "beta turn".to_string(),
                request_digest: "digest-beta".to_string(),
                metadata: None,
            }),
        },
    ];

    let metadata = continue_launch_metadata(
        "run_fixture",
        None,
        &historical_events,
        "agent_000001",
        Some("alpha"),
    );

    assert_eq!(metadata.profile(), "alpha");
    assert_eq!(metadata.provider(), "provider-alpha");
    assert_eq!(metadata.model(), Some("model-alpha"));
    assert_eq!(metadata.mode_label(), Some("Continued"));
}

#[test]
fn continue_mode_uses_session_workspace_root_not_process_cwd() {
    // arrange
    // act
    // assert
    let process_cwd = tempfile::tempdir().unwrap_or_abort();
    let session_workspace = tempfile::tempdir().unwrap_or_abort();
    let session_dir = tempfile::tempdir().unwrap_or_abort();
    let run_dir = session_dir.path().join("run_continue_ws");
    std::fs::create_dir_all(&run_dir).unwrap_or_abort();

    let session_root = session_workspace.path().display().to_string();
    let event = format!(
        r#"{{"schema_version":1,"event_id":"evt-0001","seq":1,"run_id":"run_continue_ws","mono_ms":1,"actor":{{"kind":"system","agent_id":"coordinator"}},"stream_key":"run:run_continue_ws","payload":{{"event_type":"run_started","data":{{"run_name":"continued","workspace_root":"{session_root}"}}}}}}
{{"schema_version":1,"event_id":"evt-0002","seq":2,"run_id":"run_continue_ws","mono_ms":2,"actor":{{"kind":"system","agent_id":"coordinator"}},"stream_key":"run:run_continue_ws","payload":{{"event_type":"run_finished","data":{{"status":"completed","summary":"done"}}}}}}
"#
    );
    std::fs::write(run_dir.join("events.jsonl"), event).unwrap_or_abort();

    let mode = resolve_tui_mode(
        &TuiCommand {
            replay: None,
            continue_session: Some(run_dir.clone()),
            scenario: None,
            mock: true,
            deterministic: true,
            session_dir: Some(session_dir.path().to_path_buf()),
            exit_on_finish: false,
            profile: None,
            no_alt_screen: false,
            minimal: false,
            fullscreen: false,
        },
        None,
        None,
        process_cwd.path().to_path_buf(),
        &harness_core::config::ConfigLoadContext::from_env()
            .with_current_dir(process_cwd.path().to_path_buf()),
    )
    .unwrap_or_abort();

    let ResolvedTuiMode::Continue {
        settings,
        run_dir: resolved_run_dir,
    } = mode
    else {
        panic!("expected Continue mode");
    };
    assert_eq!(resolved_run_dir, run_dir);
    assert_eq!(
        settings.workspace_root,
        session_workspace.path(),
        "continue must prefer session RunStarted.workspace_root over process cwd"
    );
    assert_ne!(settings.workspace_root, process_cwd.path());
}
