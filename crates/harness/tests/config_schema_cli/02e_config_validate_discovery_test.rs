use harness::UnwrapOrAbort;
#[test]
fn config_validate_cli_accepts_provider_catalog_reference_config_by_explicit_path() {
    // arrange
    let repo_root = repo_root();
    let config_path = repo_root
        .join("configs")
        .join("provider-catalog.reference.jsonc");

    let output = harness_command()
        .current_dir(&repo_root)
        .args([
            "--config",
            config_path.to_str().unwrap_or_abort(),
            "config",
            "validate",
        ])
        .output()
        // act
        .unwrap_or_abort();

    // assert
    assert!(
        output.status.success(),
        "stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("config valid:"));
    assert!(stdout.contains("configs/provider-catalog.reference.jsonc"));

    let parsed = load_config_from_file(&config_path).unwrap_or_abort();
    assert_eq!(parsed.providers.len(), 1);
    let ProviderConfig::OpenAiCompatible(provider) = parsed
        .providers
        .get("openai-codex")
        .unwrap_or_abort();
    assert!(provider.models.len() > 1);
}
#[test]
fn config_validate_cli_does_not_auto_discover_provider_catalog_reference_config() {
    // arrange
    let temp = tempdir().unwrap_or_abort();
    let configs_dir = temp.path().join("configs");
    fs::create_dir_all(&configs_dir).unwrap_or_abort();
    fs::copy(
        repo_root()
            .join("configs")
            .join("provider-catalog.reference.jsonc"),
        configs_dir.join("provider-catalog.reference.jsonc"),
    )
    .unwrap_or_abort();

    let output = harness_command()
        .current_dir(temp.path())
        .args(["config", "validate"])
        .output()
        // act
        .unwrap_or_abort();

    // assert
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("no config file found"));
    assert!(!stderr.contains("provider-catalog.reference.jsonc"));
}
#[test]
fn config_validate_cli_merges_xdg_defaults_with_local_project_override() {
    // arrange
    let temp = tempdir().unwrap_or_abort();
    let xdg_root = temp.path().join("xdg");
    let xdg_config_path = xdg_root.join("harness/harness.jsonc");
    let local_config_path = temp.path().join("harness.json");

    fs::create_dir_all(xdg_config_path.parent().unwrap_or_abort())
        .unwrap_or_abort();
    write_config(&xdg_config_path, &canonical_runtime_config());
    write_config(
        &local_config_path,
        &serde_json::json!({
            "default_agent": "plan"
        }),
    );

    let output = harness_command()
        .current_dir(temp.path())
        .env("XDG_CONFIG_HOME", &xdg_root)
        .args(["config", "validate"])
        .output()
        // act
        .unwrap_or_abort();

    // assert
    assert!(
        output.status.success(),
        "stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("config valid:"));
    assert!(stdout.contains(xdg_config_path.to_str().unwrap_or_abort()));
    assert!(stdout.contains("harness.json"));
}
#[test]
fn load_config_allows_public_agents_without_explicit_description() {
    // arrange
    let temp = tempdir().unwrap_or_abort();
    let config_path = temp.path().join("harness.jsonc");
    let mut config = canonical_runtime_config();
    config["agent"] = serde_json::json!({
        "plan": {
            "use_small_model": true,
            "tools": []
        }
    });
    config["default_agent"] = serde_json::json!("plan");
    write_config(&config_path, &config);

    let parsed = load_config_from_file(&config_path)
        .unwrap_or_abort();
    let plan = parsed
        .agents
        .get("plan")
        // act
        .unwrap_or_abort();

    // assert
    assert_eq!(
        plan.description,
        "Plan mode. Disallows all edit tools except the active plan file."
    );
    assert_eq!(plan.model_ref, "default/gpt-4o-mini");
}
#[test]
fn config_validate_cli_accepts_legacy_harness_native_shape() {
    // arrange
    let temp = tempdir().unwrap_or_abort();
    let config_path = temp.path().join("harness.jsonc");
    write_config(
        &config_path,
        &legacy_runtime_config(&temp.path().join("sessions")),
    );

    let output = harness_command()
        .current_dir(temp.path())
        .args(["config", "validate"])
        .output()
        // act
        .unwrap_or_abort();

    // assert
    assert!(
        output.status.success(),
        "stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}
#[test]
fn config_validate_cli_accepts_legacy_xdg_config_path_for_migration() {
    // arrange
    let temp = tempdir().unwrap_or_abort();
    let xdg_root = temp.path().join("xdg");
    let legacy_xdg_config = xdg_root.join("harness/config.jsonc");
    fs::create_dir_all(legacy_xdg_config.parent().unwrap_or_abort())
        .unwrap_or_abort();
    write_config(&legacy_xdg_config, &canonical_runtime_config());

    let output = harness_command()
        .current_dir(temp.path())
        .env("XDG_CONFIG_HOME", &xdg_root)
        .args(["config", "validate"])
        .output()
        // act
        .unwrap_or_abort();

    // assert
    assert!(
        output.status.success(),
        "stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(String::from_utf8_lossy(&output.stdout).contains("config.jsonc"));
}
