use super::*;
use harness::UnwrapOrAbort;

#[test]
fn live_coordinator_config_warmup_reuses_interactive_config() {
    let config = load_config_from_str(
        r#"
        {
          provider: {
            default: {
              type: "openai_compatible",
              options: {
                baseURL: "http://127.0.0.1:8317/v1",
                apiKey: "test-key",
                apiMode: "responses",
                timeoutMs: 60000
              },
              models: {
                "gpt-5.4-mini": {
                  name: "GPT-5.4 Mini"
                }
              }
            }
          },
          model: "default/gpt-5.4-mini",
          agent: {
            default: {
              system_prompt: "Implement carefully.",
              model: "default/gpt-5.4-mini",
              tools: ["read", "spawn_subagent"]
            }
          },
          permission: "allow",
          runtime: {
            background_tasks: {
              default_concurrency: 2,
              provider_concurrency: 2,
              model_concurrency: 2,
              stale_timeout_ms: 15000,
              message_staleness_timeout_ms: 5000
            },
            session_dir: ".harness/sessions"
          },
        }
        "#,
    )
    .unwrap_or_abort();
    let temp = tempfile::tempdir().unwrap_or_abort();
    let session_dir = temp.path().join("sessions");
    let agent_profiles = bootstrap::interactive_agent_profiles(&config).unwrap_or_abort();
    let settings = LiveSettings {
        config: Some(config),
        config_path: None,
        session_dir: session_dir.clone(),
        data_dir: temp.path().join("data/harness"),
        workspace_root: temp.path().to_path_buf(),
        shell_allowlist: ShellAllowlist::default(),
        deterministic: false,
        yolo: true,
        seed: 0,
        config_digest: "digest".to_string(),
        launch_metadata: interactive_launch_metadata(None, &agent_profiles, "default")
            .unwrap_or_abort(),
        launch_mode_label: None,
        toggles: TogglesConfig::default(),
    };

    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap_or_abort();
    runtime.block_on(async {
        let warmup = LiveCoordinatorConfigWarmup::start(&settings, false);
        let first = warmup
            .coordinator_config(&settings, false)
            .await
            .unwrap_or_abort();
        let second = warmup
            .coordinator_config(&settings, false)
            .await
            .unwrap_or_abort();

        assert_eq!(first.session_dir, session_dir);
        assert_eq!(second.session_dir, session_dir);
        assert!(first.agent_profiles.contains_key("default"));
        assert!(second.tool_registry.get("read").is_some());
        assert!(first.yolo_on_start && second.yolo_on_start);
        assert_eq!(
            first.subagent_discovery.as_ref().unwrap_or_abort().cwd,
            settings.workspace_root
        );

        let mut active = first.clone();
        let fixture = Arc::new(harness_providers::mock::MockProvider::default());
        active.provider = Arc::clone(&fixture) as Arc<dyn harness_providers::Provider>;
        let coordinator = harness_core::coord::spawn_coordinator(active,
            Arc::new(harness_core::clock::FakeClock::new()),
            Arc::new(harness_core::redact::DefaultRedactor::default()));
        coordinator.start_run("login-refresh", temp.path()).await.unwrap_or_abort();
        let parent = coordinator.spawn_agent(user_actor(), "default", None).await.unwrap_or_abort();
        let input = serde_json::json!({
            "prompt": "Answer once", "description": "new provider", "model": "later/gpt-5.4-mini", "background": false
        });
        let actor = harness_core::event::EventActor::new(harness_core::event::ActorKind::Worker, Some(parent));
        let rejected = coordinator.execute_agent_tool_call(actor.clone(), None, "spawn_subagent", input.clone()).await;
        assert!(matches!(rejected, Err(error) if error.contains("unavailable")));
        // A provider signed in after a session started serves that session's next request.
        let request = harness_providers::CompletionRequest {
            provider_id: Some("later".into()),
            model_id: "gpt-5.4-mini".into(),
            messages: vec![harness_providers::CompletionMessage::text(
                harness_providers::MessageRole::User,
                "hi",
            )],
            ..Default::default()
        };
        assert!(first
            .provider
            .request_budget_semantics(&request, 0)
            .is_err());
        let mut signed_in = settings.clone();
        let config = signed_in.config.as_mut().unwrap_or_abort();
        let provider = config.providers["default"].clone();
        config.providers.insert("later".into(), provider);
        warmup
            .add_signed_in_providers(&signed_in, Some(&coordinator))
            .await
            .unwrap_or_abort();
        assert!(first.provider.request_budget_semantics(&request, 0).is_ok());
        let refreshed = warmup.coordinator_config(&signed_in, false).await.unwrap_or_abort();
        assert!(refreshed.model_catalog.iter().any(|entry| entry.provider == "later"));

        let child = coordinator.execute_agent_tool_call(
            actor, None, "spawn_subagent", input).await;
        assert!(child.is_ok(), "{child:?}");
        let requests = fixture.captured_requests().await;
        assert_eq!(requests.len(), 1);
        assert_eq!(requests[0].provider_id.as_deref(), Some("later"));
        assert_eq!(requests[0].context.working_directory.as_deref(), Some(temp.path()));
        coordinator.stop_run().await.unwrap_or_abort();
        let other = tempfile::tempdir().unwrap_or_abort();
        let mut switched = settings.clone();
        switched.workspace_root = other.path().into();
        let switched_config = warmup
            .coordinator_config(&switched, false)
            .await
            .unwrap_or_abort();
        assert_eq!(
            switched_config
                .subagent_discovery
                .as_ref()
                .unwrap_or_abort()
                .cwd,
            switched.workspace_root
        );
    });
}
