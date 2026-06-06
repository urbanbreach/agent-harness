use super::*;

#[test]
fn env_var_substitution_works() {
    let expected = env::var("PATH").expect("PATH must exist in test environment");
    let cfg = config_fixture(
        &deep_profile(
            r#"
                system_prompt: "Be precise.",
                tool_failure_mode: "continue_as_tool_message",
                tools: ["fs.read"],
                "#,
        ),
        "${PATH}",
        None,
        None,
    );

    let parsed = load_config_from_str(&cfg).expect("config with env reference must parse");
    let ProviderConfig::OpenAiCompatible(provider) = parsed.providers.get("default").unwrap();
    assert_eq!(provider.api_key, expected);
}

#[test]
fn ui_default_profile_parses() {
    let cfg = config_fixture(
        &deep_profile(r#"tools: ["fs.read"],"#),
        "test-key",
        Some(
            r#"
                ui: {
                  defaultProfile: "deep",
                },
                "#,
        ),
        None,
    );

    let parsed = load_config_from_str(&cfg).expect("config with ui.defaultProfile must parse");
    assert_eq!(parsed.ui.default_profile, Some("deep".to_string()));
}

#[test]
fn ui_default_profile_defaults_to_none() {
    let cfg = config_fixture(
        &deep_profile(r#"tools: ["fs.read"],"#),
        "test-key",
        None,
        None,
    );

    let parsed = load_config_from_str(&cfg).expect("config without ui section must parse");
    assert_eq!(parsed.ui.default_profile, None);
}

#[test]
fn runtime_profile_max_iters_defaults_to_unbounded() {
    let cfg = config_fixture(&deep_profile(r#"tools: ["read"],"#), "test-key", None, None);

    let parsed =
        load_config_from_str(&cfg).expect("config with default tool failure mode must parse");
    assert_eq!(parsed.agents["deep"].max_iters, None);
    assert_eq!(
        parsed.agents["deep"].tool_failure_mode,
        ToolFailureMode::ContinueAsToolMessage
    );
}

#[test]
fn profile_tool_failure_mode_and_system_prompt_parse_explicitly() {
    let cfg = config_fixture(
        &deep_profile(
            r#"
                system_prompt: "Be precise.",
                max_iters: 24,
                tool_failure_mode: "continue_as_tool_message",
                tools: ["fs.read"],
                "#,
        ),
        "test-key",
        None,
        None,
    );

    let parsed = load_config_from_str(&cfg)
        .expect("config with explicit tool failure mode and prompt must parse");
    assert_eq!(
        parsed.agents["deep"].tool_failure_mode,
        ToolFailureMode::ContinueAsToolMessage
    );
    assert_eq!(parsed.agents["deep"].max_iters, Some(24));
    assert_eq!(
        parsed.agents["deep"].system_prompt.as_deref(),
        Some("Be precise.")
    );
}

#[test]
fn env_var_default_fallback_works() {
    let cfg = config_fixture(
        &deep_profile(r#"tools: ["fs.read"],"#),
        "${HARNESS_CONFIG_TEST_API_KEY_FALLBACK:-fallback-key}",
        None,
        None,
    );

    let parsed = loader::load_config_from_str_with_lookup(&cfg, &|_| None)
        .expect("config with fallback env reference must parse");
    let ProviderConfig::OpenAiCompatible(provider) = parsed.providers.get("default").unwrap();
    assert_eq!(provider.api_key, "fallback-key");
}

#[test]
fn env_var_default_fallback_uses_fallback_for_empty_var() {
    let cfg = config_fixture(
        &deep_profile(r#"tools: ["fs.read"],"#),
        "${HARNESS_CONFIG_TEST_API_KEY_FALLBACK:-fallback-key}",
        None,
        None,
    );

    let parsed = loader::load_config_from_str_with_lookup(&cfg, &|_| Some(String::new()))
        .expect("config with empty fallback env reference must parse");
    let ProviderConfig::OpenAiCompatible(provider) = parsed.providers.get("default").unwrap();
    assert_eq!(provider.api_key, "fallback-key");
}

#[test]
fn empty_env_var_uses_default_fallback() {
    let cfg = config_fixture(
        &deep_profile(r#"tools: ["fs.read"],"#),
        "${HARNESS_CONFIG_TEST_API_KEY_EMPTY:-fallback-key}",
        None,
        None,
    );

    let parsed = loader::load_config_from_str_with_lookup(&cfg, &|_| Some(String::new()))
        .expect("config with empty env reference should use fallback value");
    let ProviderConfig::OpenAiCompatible(provider) = parsed.providers.get("default").unwrap();
    assert_eq!(provider.api_key, "fallback-key");
}

#[test]
fn missing_required_env_var_is_an_error() {
    let cfg = config_fixture(
        &deep_profile(r#"tools: ["fs.read"],"#),
        "${HARNESS_CONFIG_TEST_API_KEY_REQUIRED}",
        None,
        None,
    );

    let err = loader::load_config_from_str_with_lookup(&cfg, &|_| None)
        .expect_err("missing required env variable should fail");
    assert_eq!(
            err.to_string(),
            "environment variable `HARNESS_CONFIG_TEST_API_KEY_REQUIRED` referenced in config is not set"
        );
}

#[test]
fn missing_openai_api_key_errors_even_for_cliproxy_loopback_base_url() {
    let err = loader::resolve_string_reference_with_lookup("${OPENAI_API_KEY}", None, &|_| None)
        .expect_err("loopback providers should still require OPENAI_API_KEY");

    assert_eq!(
        err.to_string(),
        "environment variable `OPENAI_API_KEY` referenced in config is not set"
    );
}

#[test]
fn configured_openai_api_key_env_reference_resolves_without_fallback() {
    let resolved = loader::resolve_string_reference_with_lookup("${OPENAI_API_KEY}", None, &|_| {
        Some("test-openai-api-key".to_string())
    })
    .expect("OPENAI_API_KEY should resolve when it is set");

    assert_eq!(resolved, "test-openai-api-key");
}

#[test]
fn upstream_env_reference_uses_empty_string_when_missing() {
    let cfg = config_fixture(
        &deep_profile(r#"tools: ["fs.read"],"#),
        "{env:HARNESS_CONFIG_TEST_OPTIONAL_EMPTY}",
        None,
        None,
    );

    let parsed = loader::load_config_from_str_with_lookup(&cfg, &|_| None)
        .expect("upstream env reference should parse");
    let ProviderConfig::OpenAiCompatible(provider) = parsed.providers.get("default").unwrap();
    assert_eq!(provider.api_key, "");
}

#[test]
fn upstream_file_reference_resolves_relative_to_config_file() {
    let temp = tempfile::tempdir().expect("tempdir");
    let config_dir = temp.path().join("nested");
    let secret_path = config_dir.join("secrets/api-key.txt");
    let config_path = config_dir.join("harness.jsonc");
    fs::create_dir_all(secret_path.parent().expect("secret parent")).expect("create secret parent");
    fs::write(&secret_path, "file-key").expect("write secret file");
    fs::write(
        &config_path,
        config_fixture(
            &deep_profile(r#"tools: ["fs.read"],"#),
            "{file:secrets/api-key.txt}",
            None,
            None,
        ),
    )
    .expect("write config");

    let parsed = load_config_from_file(&config_path).expect("file reference config should parse");
    let ProviderConfig::OpenAiCompatible(provider) = parsed.providers.get("default").unwrap();
    assert_eq!(provider.api_key, "file-key");
}

#[test]
fn load_config_from_file_can_define_agent_from_markdown_frontmatter() {
    let _lock = CONFIG_DISCOVERY_TEST_LOCK
        .lock()
        .expect("lock discovery tests");
    let temp = tempfile::tempdir().expect("tempdir");
    let repo = temp.path().join("repo");
    fs::create_dir_all(&repo).expect("create repo dir");
    fs::create_dir_all(repo.join(".git")).expect("create git dir");

    let config_path = repo.join("harness.jsonc");
    fs::write(&config_path, config_fixture("", "test-key", None, None)).expect("write config");
    write_agent_markdown(
        &repo,
        "build",
        r#"---
{
  description: "Build from markdown",
  model_ref: "default:gpt-4o-mini",
  tools: ["read", "grep"],
  max_iters: 18
}
---

Execute from markdown only."#,
    );

    let parsed = load_config_from_file(&config_path).expect("markdown-only agent config");
    let build = parsed.agents.get("build").expect("build agent");
    assert_eq!(build.description, "Build from markdown");
    assert_eq!(build.model_ref, "default:gpt-4o-mini");
    assert_eq!(build.tools, vec!["read", "grep"]);
    assert_eq!(build.max_iters, Some(18));
    assert_eq!(
        build.system_prompt.as_deref(),
        Some("Execute from markdown only.")
    );
}

#[test]
fn load_config_from_file_still_accepts_legacy_agent_harness_prompt_dir() {
    let _lock = CONFIG_DISCOVERY_TEST_LOCK
        .lock()
        .expect("lock discovery tests");
    let temp = tempfile::tempdir().expect("tempdir");
    let repo = temp.path().join("repo");
    fs::create_dir_all(&repo).expect("create repo dir");
    fs::create_dir_all(repo.join(".git")).expect("create git dir");

    let config_path = repo.join("harness.jsonc");
    fs::write(&config_path, config_fixture("", "test-key", None, None)).expect("write config");
    write_legacy_agent_markdown(
        &repo,
        "build",
        r#"---
{
  description: "Legacy build prompt",
  model_ref: "default:gpt-4o-mini"
}
---

Legacy prompt body."#,
    );

    let parsed =
        load_config_from_file(&config_path).expect("legacy prompt dir should remain compatible");
    assert_eq!(
        parsed.agents["build"].system_prompt.as_deref(),
        Some("Legacy prompt body.")
    );
}

#[test]
fn load_config_from_file_keeps_inline_system_prompt_over_markdown_prompt() {
    let _lock = CONFIG_DISCOVERY_TEST_LOCK
        .lock()
        .expect("lock discovery tests");
    let temp = tempfile::tempdir().expect("tempdir");
    let repo = temp.path().join("repo");
    fs::create_dir_all(&repo).expect("create repo dir");
    fs::create_dir_all(repo.join(".git")).expect("create git dir");

    let config_path = repo.join("harness.jsonc");
    fs::write(
        &config_path,
        config_fixture(
            &deep_profile(
                r#"
                    system_prompt: "Inline prompt",
                    tools: ["read"],
                    "#,
            ),
            "test-key",
            None,
            None,
        ),
    )
    .expect("write config");
    write_agent_markdown(&repo, "deep", "Markdown prompt body.");

    let parsed = load_config_from_file(&config_path).expect("config with markdown prompt");
    assert_eq!(
        parsed.agents["deep"].system_prompt.as_deref(),
        Some("Inline prompt")
    );
}

#[test]
fn load_config_from_file_discovers_project_agents_md_separately() {
    let _lock = CONFIG_DISCOVERY_TEST_LOCK
        .lock()
        .expect("lock discovery tests");
    let temp = tempfile::tempdir().expect("tempdir");
    let repo = temp.path().join("repo");
    fs::create_dir_all(&repo).expect("create repo dir");
    fs::create_dir_all(repo.join(".git")).expect("create git dir");

    let config_path = repo.join("harness.jsonc");
    fs::write(
        &config_path,
        config_fixture(&deep_profile(r#"tools: ["read"],"#), "test-key", None, None),
    )
    .expect("write config");
    fs::write(repo.join("AGENTS.md"), "Project instructions live here.").expect("write AGENTS.md");

    let parsed = load_config_from_file(&config_path).expect("config with project instructions");
    assert_eq!(parsed.instruction_files.len(), 1);
    assert_eq!(
        parsed.instruction_files[0].content,
        "Project instructions live here."
    );
    assert!(parsed.instruction_files[0].path.ends_with("AGENTS.md"));
}

#[test]
fn load_config_from_file_discovers_repo_assets_when_cwd_differs() {
    let _lock = CONFIG_DISCOVERY_TEST_LOCK
        .lock()
        .expect("lock discovery tests");
    let temp = tempfile::tempdir().expect("tempdir");
    let outside = temp.path().join("outside");
    let repo = temp.path().join("repo");
    let config_dir = repo.join("configs").join("nested");
    fs::create_dir_all(&outside).expect("create outside dir");
    fs::create_dir_all(&config_dir).expect("create config dir");
    fs::create_dir_all(repo.join(".git")).expect("create git dir");

    let config_path = config_dir.join("harness.jsonc");
    fs::write(&config_path, config_fixture("", "test-key", None, None)).expect("write config");
    write_agent_markdown(
        &repo,
        "build",
        r#"---
{
  description: "Build from repo root markdown",
  model_ref: "default:gpt-4o-mini"
}
---

Prompt discovered from the config repo root."#,
    );
    fs::write(repo.join("AGENTS.md"), "Repo-root instructions.").expect("write repo AGENTS.md");

    let parsed = load_config_from_file(&config_path).expect("discover repo-root assets");
    let build = parsed.agents.get("build").expect("build agent");
    assert_eq!(build.description, "Build from repo root markdown");
    assert_eq!(
        build.system_prompt.as_deref(),
        Some("Prompt discovered from the config repo root.")
    );
    assert_eq!(parsed.instruction_files.len(), 1);
    assert_eq!(
        parsed.instruction_files[0].content,
        "Repo-root instructions."
    );
    assert_eq!(parsed.instruction_files[0].path, repo.join("AGENTS.md"));
}

#[test]
fn load_config_from_file_ignores_unmatched_prompt_only_markdown_assets() {
    let _lock = CONFIG_DISCOVERY_TEST_LOCK
        .lock()
        .expect("lock discovery tests");
    let temp = tempfile::tempdir().expect("tempdir");
    let repo = temp.path().join("repo");
    fs::create_dir_all(&repo).expect("create repo dir");
    fs::create_dir_all(repo.join(".git")).expect("create git dir");

    let config_path = repo.join("harness.jsonc");
    fs::write(
        &config_path,
        config_fixture(&deep_profile(r#"tools: ["read"],"#), "test-key", None, None),
    )
    .expect("write config");
    write_agent_markdown(&repo, "stray", "Prompt body without frontmatter metadata.");

    let parsed = load_config_from_file(&config_path).expect("prompt-only stray asset ignored");
    assert!(!parsed.agents.contains_key("stray"));
    assert!(parsed.agents.contains_key("deep"));
}

#[test]
fn load_config_from_file_rejects_invalid_markdown_frontmatter() {
    let _lock = CONFIG_DISCOVERY_TEST_LOCK
        .lock()
        .expect("lock discovery tests");
    let temp = tempfile::tempdir().expect("tempdir");
    let repo = temp.path().join("repo");
    fs::create_dir_all(&repo).expect("create repo dir");
    fs::create_dir_all(repo.join(".git")).expect("create git dir");

    let config_path = repo.join("harness.jsonc");
    fs::write(
        &config_path,
        config_fixture(&deep_profile(r#"tools: ["read"],"#), "test-key", None, None),
    )
    .expect("write config");
    write_agent_markdown(
        &repo,
        "deep",
        r#"---
{ description: }
---

Broken prompt."#,
    );

    let err = load_config_from_file(&config_path).expect_err("invalid markdown should fail");
    assert!(err.to_string().contains("invalid markdown frontmatter"));
    assert!(err.to_string().contains("deep.md"));
}

#[test]
fn load_config_from_file_rejects_legacy_plan_markdown_frontmatter() {
    let _lock = CONFIG_DISCOVERY_TEST_LOCK
        .lock()
        .expect("lock discovery tests");
    let temp = tempfile::tempdir().expect("tempdir");
    let repo = temp.path().join("repo");
    fs::create_dir_all(&repo).expect("create repo dir");
    fs::create_dir_all(repo.join(".git")).expect("create git dir");

    let config_path = repo.join("harness.jsonc");
    fs::write(
        &config_path,
        config_fixture(&deep_profile(r#"tools: ["read"],"#), "test-key", None, None),
    )
    .expect("write config");
    write_agent_markdown(
        &repo,
        "deep",
        r#"---
{
  description: "Legacy plan prompt",
  model_ref: "default:gpt-4o-mini",
  planMode: true
}
---

Legacy prompt."#,
    );

    let err = load_config_from_file(&config_path).expect_err("legacy plan frontmatter should fail");
    assert!(err.to_string().contains("invalid markdown frontmatter"));
    assert!(err.to_string().contains("unknown field `planMode`"));
}
