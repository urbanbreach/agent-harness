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
              tools: ["read"]
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
            session_dir: ".agent-harness/sessions"
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
            .add_signed_in_providers(&signed_in)
            .await
            .unwrap_or_abort();
        assert!(first.provider.request_budget_semantics(&request, 0).is_ok());
    });
}
