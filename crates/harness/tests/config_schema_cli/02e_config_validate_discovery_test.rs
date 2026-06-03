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
            config_path.to_str().expect("config path utf-8"),
            "config",
            "validate",
        ])
        .output()
        // act
        .expect("run harness config validate with reference catalog config");

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

    let parsed = load_config_from_file(&config_path).expect("reference catalog should parse");
    assert_eq!(parsed.providers.len(), 1);
    let ProviderConfig::OpenAiCompatible(provider) = parsed
        .providers
        .get("openai-codex")
        .expect("openai-codex provider present in reference catalog");
    assert!(provider.models.len() > 1);
}
#[test]
fn config_validate_cli_does_not_auto_discover_provider_catalog_reference_config() {
    // arrange
    let temp = tempdir().expect("tempdir");
    let configs_dir = temp.path().join("configs");
    fs::create_dir_all(&configs_dir).expect("create configs dir");
    fs::copy(
        repo_root()
            .join("configs")
            .join("provider-catalog.reference.jsonc"),
        configs_dir.join("provider-catalog.reference.jsonc"),
    )
    .expect("copy reference catalog fixture");

    let output = harness_command()
        .current_dir(temp.path())
        .args(["config", "validate"])
        .output()
        // act
        .expect("run harness config validate with only reference catalog present");

    // assert
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("no config file found"));
    assert!(!stderr.contains("provider-catalog.reference.jsonc"));
}
#[test]
fn config_validate_cli_merges_xdg_defaults_with_local_project_override() {
    // arrange
    let temp = tempdir().expect("tempdir");
    let xdg_root = temp.path().join("xdg");
    let xdg_config_path = xdg_root.join("harness/harness.jsonc");
    let local_config_path = temp.path().join("harness.json");

    fs::create_dir_all(xdg_config_path.parent().expect("xdg config parent"))
        .expect("create xdg config dir");
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
        .expect("run harness config validate with merged discovery");

    // assert
    assert!(
        output.status.success(),
        "stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("config valid:"));
    assert!(stdout.contains(xdg_config_path.to_str().expect("xdg path utf-8")));
    assert!(stdout.contains("harness.json"));
}
#[test]
fn load_config_allows_public_agents_without_explicit_description() {
    // arrange
    let temp = tempdir().expect("tempdir");
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
        .expect("public agent without explicit description should still load");
    let plan = parsed
        .agents
        .get("plan")
        // act
        .expect("plan profile should be translated from public config");

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
    let temp = tempdir().expect("tempdir");
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
        .expect("run harness config validate with legacy config");

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
    let temp = tempdir().expect("tempdir");
    let xdg_root = temp.path().join("xdg");
    let legacy_xdg_config = xdg_root.join("harness/config.jsonc");
    fs::create_dir_all(legacy_xdg_config.parent().expect("legacy xdg parent"))
        .expect("create legacy xdg dir");
    write_config(&legacy_xdg_config, &canonical_runtime_config());

    let output = harness_command()
        .current_dir(temp.path())
        .env("XDG_CONFIG_HOME", &xdg_root)
        .args(["config", "validate"])
        .output()
        // act
        .expect("run harness config validate with legacy xdg path");

    // assert
    assert!(
        output.status.success(),
        "stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(String::from_utf8_lossy(&output.stdout).contains("config.jsonc"));
}
