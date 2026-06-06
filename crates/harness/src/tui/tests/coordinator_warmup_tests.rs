use super::*;

#[test]
fn live_coordinator_config_warmup_reuses_interactive_config() {
    let config = load_config_from_str(
        r#"
        {
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
          agents: {
            build: {
              description: "Implementation",
              system_prompt: "Implement carefully.",
              model_ref: "default:gpt-5.4-mini",
              tools: ["read"]
            }
          },
          default_agent: "build",
          permissions: {
            defaults: {
              edit: "allow",
              shell: "allow",
              network: "allow"
            },
            shell_allowlist: {
              executables: ["bash"],
              cwd_roots: ["."]
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
        }
        "#,
    )
    .expect("config should parse");
    let session_dir = PathBuf::from("/tmp/warmed-session-dir");
    let agent_profiles = bootstrap::interactive_agent_profiles(&config)
        .expect("interactive agent profiles should build");
    let settings = LiveSettings {
        config: Some(config),
        config_path: None,
        session_dir: session_dir.clone(),
        workspace_root: PathBuf::from("/tmp/warmed-workspace"),
        shell_allowlist: ShellAllowlist::default(),
        deterministic: false,
        seed: 0,
        config_digest: "digest".to_string(),
        launch_metadata: interactive_launch_metadata(None, &agent_profiles, "build")
            .expect("launch metadata should build"),
        launch_mode_label: None,
        toggles: TogglesConfig::default(),
    };

    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime should build");
    runtime.block_on(async {
        let warmup = LiveCoordinatorConfigWarmup::start(&settings, false);
        let first = warmup
            .coordinator_config(&settings, false)
            .await
            .expect("warmup should build interactive coordinator config");
        let second = warmup
            .coordinator_config(&settings, false)
            .await
            .expect("warmup should reuse cached coordinator config");

        assert_eq!(first.session_dir, session_dir);
        assert_eq!(second.session_dir, session_dir);
        assert!(first.agent_profiles.contains_key("build"));
        assert!(second.tool_registry.get("read").is_some());
    });
}
