use harness::UnwrapOrAbort;
use std::fs;
use std::path::PathBuf;

use harness_core::config::{load_config_from_file, load_config_from_str, ProviderConfig};
use serde_json::Value;
use tempfile::tempdir;

#[path = "common/mod.rs"]
mod common;

use common::{repo_root, CliHarness};

fn harness_jsonc_source() -> PathBuf {
    repo_root().join("harness.jsonc")
}

fn copy_harness_jsonc_to_temp() -> (tempfile::TempDir, PathBuf) {
    let temp = tempdir().unwrap_or_abort();
    let dest = temp.path().join("harness.jsonc");
    fs::copy(harness_jsonc_source(), &dest).unwrap_or_abort();
    (temp, dest)
}

fn harness_command() -> CliHarness {
    CliHarness::new()
        .env_remove("HARNESS_CONFIG")
        .env_remove("HARNESS_CONFIG_CONTENT")
        .env_remove("HARNESS_TUI_CONFIG")
        .env("HOME", "/nonexistent")
        .env("XDG_CONFIG_HOME", "/nonexistent")
}

fn run_with_real_config(config_path: &PathBuf, args: &[&str]) -> common::CliHarnessOutput {
    let data_temp = tempdir().unwrap_or_abort();
    let mut full_args = vec!["--config", config_path.to_str().unwrap_or_abort()];
    full_args.extend_from_slice(args);
    harness_command()
        .env("HARNESS_DATA_HOME", data_temp.path())
        .args(full_args)
        .output()
}

#[test]
fn config_validate_passes_with_real_harness_jsonc() {
    // arrange
    let (_config_temp, config_path) = copy_harness_jsonc_to_temp();

    // act
    let output = run_with_real_config(&config_path, &["config", "validate"]);

    // assert
    assert!(
        output.status.success(),
        "config validate failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn doctor_json_passes_with_real_harness_jsonc() {
    // arrange
    let (_config_temp, config_path) = copy_harness_jsonc_to_temp();

    // act
    let output = run_with_real_config(&config_path, &["doctor", "--json"]);

    // assert
    assert!(
        output.status.success(),
        "doctor --json failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let report: Value = serde_json::from_slice(&output.stdout).unwrap_or_abort();
    assert!(
        report["checks"].is_array(),
        "doctor report has checks array"
    );
    let checks = report["checks"].as_array().unwrap_or_abort();
    for check in checks {
        assert_ne!(
            check["status"], "fail",
            "doctor check {} failed: {}",
            check["name"], check["message"]
        );
    }
}

#[test]
fn openai_codex_provider_resolves_codex_auth_provider() {
    // arrange
    let (_config_temp, config_path) = copy_harness_jsonc_to_temp();

    // act
    let config = load_config_from_file(&config_path).unwrap_or_abort();
    let provider = config.providers.get("openai-codex").unwrap_or_abort();

    // assert
    match provider {
        ProviderConfig::OpenAiCompatible(opts) => {
            assert_eq!(
                opts.auth_provider.as_ref().map(|p| p.as_str()),
                Some("codex"),
                "openai-codex provider must have authProvider codex"
            );
        }
        ProviderConfig::Anthropic(_) => {
            panic!("unexpected Anthropic provider in openai_codex_provider_has_codex_auth_provider test")
        }
    }
}

#[test]
fn default_provider_has_inline_api_key_without_auth_provider() {
    // arrange
    let (_config_temp, config_path) = copy_harness_jsonc_to_temp();

    // act
    let config = load_config_from_file(&config_path).unwrap_or_abort();
    let provider = config.providers.get("default").unwrap_or_abort();

    // assert
    match provider {
        ProviderConfig::OpenAiCompatible(opts) => {
            assert!(
                opts.auth_provider.is_none(),
                "default provider must not have authProvider"
            );
            assert!(
                !opts.api_key.is_empty(),
                "default provider must have inline apiKey"
            );
        }
        ProviderConfig::Anthropic(_) => {
            panic!("unexpected Anthropic provider in default_provider_has_inline_api_key_without_auth_provider test")
        }
    }
}

#[test]
fn umans_provider_has_api_key_env_without_auth_provider() {
    // arrange
    let (_config_temp, config_path) = copy_harness_jsonc_to_temp();

    // act
    let config = load_config_from_file(&config_path).unwrap_or_abort();
    let provider = config
        .providers
        .get("umans-ai-coding-plan")
        .unwrap_or_abort();

    // assert
    match provider {
        ProviderConfig::OpenAiCompatible(opts) => {
            assert!(
                opts.auth_provider.is_none(),
                "umans-ai-coding-plan provider must not have authProvider"
            );
            assert!(
                !opts.api_key_env.is_empty(),
                "umans-ai-coding-plan provider must have apiKeyEnv"
            );
            assert!(
                opts.api_key_env
                    .iter()
                    .any(|env| env == "UMANS_AI_CODING_PLAN_API_KEY"),
                "umans-ai-coding-plan provider must reference UMANS_AI_CODING_PLAN_API_KEY"
            );
        }
        ProviderConfig::Anthropic(_) => {
            panic!("unexpected Anthropic provider in umans_provider_has_api_key_env_without_auth_provider test")
        }
    }
}

#[test]
fn dogfood_agents_use_gpt_56_luna_requested_variants() {
    // arrange
    let (_config_temp, config_path) = copy_harness_jsonc_to_temp();

    // act
    let config = load_config_from_file(&config_path).unwrap_or_abort();

    // assert
    for agent_name in ["default", "general"] {
        let agent = config.agents.get(agent_name).unwrap_or_abort();
        assert_eq!(agent.model_ref, "openai-codex/gpt-5.6-luna");
        assert_eq!(agent.variant.as_deref(), Some("max"));
    }
    for agent_name in ["explore", "librarian"] {
        let agent = config.agents.get(agent_name).unwrap_or_abort();
        assert_eq!(agent.model_ref, "openai-codex/gpt-5.6-luna");
        assert_eq!(agent.variant.as_deref(), Some("low"));
    }
}

#[test]
fn adding_anthropic_auth_provider_to_real_config_works() {
    // arrange
    let (_config_temp, config_path) = copy_harness_jsonc_to_temp();
    let original = fs::read_to_string(&config_path).unwrap_or_abort();
    let modified = original.replace(
        "\"umans-ai-coding-plan\": {",
        "\"anthropic-test\": {\n      \"type\": \"openai_compatible\",\n      \"name\": \"Anthropic Test\",\n      \"options\": {\n        \"authProvider\": \"anthropic\",\n        \"baseURL\": \"https://api.anthropic.com/v1\",\n        \"apiKey\": \"sk-test-anthropic\",\n      },\n      \"models\": {\n        \"claude-test\": { \"name\": \"Claude Test\" },\n      },\n    },\n    \"umans-ai-coding-plan\": {",
    );

    // act
    fs::write(&config_path, &modified).unwrap_or_abort();
    let config = load_config_from_file(&config_path).unwrap_or_abort();
    let provider = config.providers.get("anthropic-test").unwrap_or_abort();

    // assert
    match provider {
        ProviderConfig::OpenAiCompatible(opts) => {
            assert_eq!(
                opts.auth_provider.as_ref().map(|p| p.as_str()),
                Some("anthropic"),
                "anthropic-test provider must have authProvider anthropic"
            );
        }
        ProviderConfig::Anthropic(_) => {
            panic!("unexpected Anthropic provider in adding_anthropic_auth_provider_to_real_config_works test")
        }
    }
}

#[test]
fn config_validate_with_anthropic_auth_provider_added_to_real_config() {
    // arrange
    let (_config_temp, config_path) = copy_harness_jsonc_to_temp();
    let original = fs::read_to_string(&config_path).unwrap_or_abort();
    let modified = original.replace(
        "\"umans-ai-coding-plan\": {",
        "\"anthropic-test\": {\n      \"type\": \"openai_compatible\",\n      \"name\": \"Anthropic Test\",\n      \"options\": {\n        \"authProvider\": \"anthropic\",\n        \"baseURL\": \"https://api.anthropic.com/v1\",\n        \"apiKey\": \"sk-test-anthropic\",\n      },\n      \"models\": {\n        \"claude-test\": { \"name\": \"Claude Test\" },\n      },\n    },\n    \"umans-ai-coding-plan\": {",
    );
    fs::write(&config_path, &modified).unwrap_or_abort();

    // act
    let output = run_with_real_config(&config_path, &["config", "validate"]);

    // assert
    assert!(
        output.status.success(),
        "config validate with anthropic authProvider failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let _ = load_config_from_str(&modified).unwrap_or_abort();
}
