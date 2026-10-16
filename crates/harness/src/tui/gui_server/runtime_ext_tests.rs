use super::*;

#[tokio::test]
async fn one_shot_sends_only_explicit_messages_and_disables_tools() -> Result<(), String> {
    let config = harness_core::config::load_config_from_str(r#"{
            provider: { local: {
                type: "openai_compatible",
                options: { baseURL: "http://127.0.0.1:8317/v1", apiKey: "test-key" },
                models: { "example": { name: "Example" } }
            }},
            model: "local/example",
            agent: { default: { model: "local/example", tools: ["read"], system_prompt: "Do not inherit this." } }
        }"#).map_err(|error| error.to_string())?;
    let settings = LiveSettings {
        config: Some(config),
        config_path: None,
        session_dir: PathBuf::new(),
        data_dir: PathBuf::new(),
        workspace_root: PathBuf::from("/"),
        shell_allowlist: Default::default(),
        deterministic: false,
        yolo: false,
        seed: 0,
        config_digest: String::new(),
        launch_mode_label: None,
        launch_metadata: harness_tui::app::LaunchMetadata::new(
            "default",
            "local",
            Some("example".into()),
        ),
        toggles: Default::default(),
    };
    let request = completion_request(
        &settings,
        &json!({"prompt":"Hello","system":"Only answer."}),
    )?;
    let provider = harness_providers::mock::MockProvider::script([vec![
        ProviderStreamEvent::TextDelta("answer".into()),
        ProviderStreamEvent::Done { usage: None },
    ]]);
    assert_eq!(collect_completion(&provider, request).await?, "answer");
    let requests = provider.captured_requests().await;
    let request = requests.first().ok_or("missing provider request")?;
    assert!(request.tools.is_none());
    assert_eq!(request.tool_choice, Some(ToolChoice::None));
    assert_eq!(
        request.messages,
        vec![
            CompletionMessage::text(MessageRole::System, "Only answer."),
            CompletionMessage::text(MessageRole::User, "Hello"),
        ]
    );
    Ok(())
}

#[tokio::test]
async fn tool_free_completion_rejects_tools_and_incomplete_streams() {
    use harness_providers::mock::MockProvider;
    for events in [
        vec![ProviderStreamEvent::ToolCallComplete {
            tool_call_id: "call".into(),
            function_name: "shell".into(),
            arguments_json: "{}".into(),
        }],
        vec![ProviderStreamEvent::TextDelta("partial".into())],
        vec![ProviderStreamEvent::Aborted { usage: None }],
    ] {
        let provider = MockProvider::script([events]);
        assert!(collect_completion(&provider, CompletionRequest::default())
            .await
            .is_err());
    }
}

#[test]
fn invalid_override_never_replaces_persisted_decisions() -> Result<(), String> {
    let root = tempfile::tempdir().map_err(|error| error.to_string())?;
    let path = root.path().join("overrides.json");
    write_private_atomic(&path, b"{\"hook:existing\":false}").map_err(|error| error.to_string())?;
    let mut overrides = read_overrides(&path)?;
    let config = HarnessConfig::default();
    let default = crate::bootstrap::interactive_profile_name(&config);
    overrides.insert(format!("profile:{default}"), false);
    assert!(save_overrides(&path, &config, &overrides).is_err());
    assert_eq!(read_overrides(&path)?.get("hook:existing"), Some(&false));
    assert_eq!(read_overrides(&path)?.len(), 1);
    Ok(())
}
