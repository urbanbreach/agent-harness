use super::*;
use std::fs;
use std::path::PathBuf;

fn temp_config_path(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "harness-settings-write-{}-{}",
        name,
        std::process::id()
    ));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).expect("temp dir");
    dir.join("harness.json")
}

fn minimal_runtime_config(hashline_edit: bool) -> String {
    format!(
        r#"{{
  "providers": {{
    "default": {{
      "type": "openai_compatible",
      "base_url": "http://127.0.0.1:8317/v1",
      "api_key": "test-key",
      "models": {{
        "gpt-4o-mini": {{
          "display_name": "GPT-4o mini"
        }}
      }}
    }}
  }},
  "model": "default/gpt-4o-mini",
  "agent": {{
    "default": {{
      "tools": ["read"]
    }}
  }},
  "permissions": {{
    "defaults": {{
      "edit": "ask",
      "shell": "ask",
      "network": "deny"
    }}
  }},
  "runtime": {{
    "background_tasks": {{
      "default_concurrency": 2,
      "provider_concurrency": 2,
      "model_concurrency": 2,
      "stale_timeout_ms": 15000,
      "message_staleness_timeout_ms": 5000
    }},
    "session_dir": ".agent-harness/sessions",
    "deterministic": {{
      "enabled": false,
      "seed": 42
    }}
  }},
  "integrations": {{
    "remote_search": {{
      "endpoint": "https://mcp.exa.ai/mcp"
    }}
  }},
  "hashline_edit": {hashline_edit}
}}"#
    )
}

#[test]
fn write_project_hashline_edit_persists_and_reloads_effective_value() {
    // arrange
    // act
    // assert
    // Given: project runtime config with hashline_edit=true
    let path = temp_config_path("toggle");
    fs::write(&path, minimal_runtime_config(true)).expect("write fixture");
    assert!(read_effective_hashline_edit(&path).expect("read initial"));

    // When: write false
    let effective = write_project_hashline_edit(&path, false).expect("write false");

    // Then: reloaded effective is false and file contains the key
    assert!(!effective);
    let body = fs::read_to_string(&path).expect("reread");
    assert!(body.contains("\"hashline_edit\": false") || body.contains("\"hashline_edit\":false"));
    assert!(!read_effective_hashline_edit(&path).expect("reload"));
    let _ = fs::remove_dir_all(path.parent().expect("parent"));
}

#[test]
fn reset_project_hashline_edit_restores_registry_default() {
    // arrange
    // act
    // assert
    // Given: overridden to false
    let path = temp_config_path("reset");
    fs::write(&path, minimal_runtime_config(false)).expect("write fixture");
    assert!(!read_effective_hashline_edit(&path).expect("read initial"));

    // When: reset to default
    let effective = reset_project_hashline_edit(&path).expect("reset");

    // Then: default true
    assert!(effective);
    assert!(read_effective_hashline_edit(&path).expect("reload"));
    let _ = fs::remove_dir_all(path.parent().expect("parent"));
}

#[test]
fn write_project_setting_bool_fails_closed_for_secret() {
    // arrange
    // act
    // assert
    let path = temp_config_path("secret");
    fs::write(&path, minimal_runtime_config(true)).expect("write fixture");

    let err = write_project_setting_bool(&path, "provider.apiKey", true)
        .expect_err("secret must fail closed");
    assert!(
        matches!(err, SettingWriteError::SecretSetting(_)),
        "got {err:?}"
    );
    let _ = fs::remove_dir_all(path.parent().expect("parent"));
}

#[test]
fn write_project_setting_bool_rejects_unknown_and_unsupported() {
    // arrange
    // act
    // assert
    let path = temp_config_path("unsupported");
    fs::write(&path, minimal_runtime_config(true)).expect("write fixture");

    let unknown = write_project_setting_bool(&path, "not.a.setting", true).expect_err("unknown");
    assert!(matches!(unknown, SettingWriteError::UnknownSetting(_)));

    let unsupported = write_project_setting_bool(&path, "model", true).expect_err("unsupported");
    assert!(matches!(
        unsupported,
        SettingWriteError::UnsupportedWrite(_)
    ));
    let _ = fs::remove_dir_all(path.parent().expect("parent"));
}

#[test]
fn write_project_setting_bool_accepts_legacy_hashline_edit_id() {
    // arrange
    // act
    // assert
    // Given: project config with hashline_edit true
    let path = temp_config_path("legacy-hashline");
    fs::write(&path, minimal_runtime_config(true)).expect("write fixture");

    // When: writing via camelCase legacy id
    let value = write_project_setting_bool(&path, "hashlineEdit", false).expect("legacy write");

    // Then: canonical key is updated
    assert!(!value);
    assert!(!read_effective_hashline_edit(&path).expect("reload"));
    let _ = fs::remove_dir_all(path.parent().expect("parent"));
}

#[test]
fn reset_project_setting_to_default_for_hashline_edit() {
    // arrange
    // act
    // assert
    let path = temp_config_path("reset-api");
    fs::write(&path, minimal_runtime_config(false)).expect("write fixture");
    let value = reset_project_setting_to_default(&path, "hashline_edit").expect("reset");
    assert_eq!(value, "true");
    let _ = fs::remove_dir_all(path.parent().expect("parent"));
}

#[test]
fn write_project_compaction_enabled_persists_under_runtime_and_reloads() {
    // arrange
    // act
    // assert
    // Given: project runtime config without compaction override
    let path = temp_config_path("compaction");
    fs::write(&path, minimal_runtime_config(true)).expect("write fixture");
    assert!(read_effective_compaction_enabled(&path).expect("read default"));

    // When: write false
    let effective = write_project_compaction_enabled(&path, false).expect("write false");

    // Then: effective false and nested key present
    assert!(!effective);
    let body = fs::read_to_string(&path).expect("reread");
    assert!(
        body.contains("\"enabled\": false") || body.contains("\"enabled\":false"),
        "body={body}"
    );
    assert!(body.contains("\"compaction\""));
    assert!(!read_effective_compaction_enabled(&path).expect("reload"));

    // When: reset to default
    let restored = reset_project_compaction_enabled(&path).expect("reset");
    assert!(restored);
    assert!(read_effective_compaction_enabled(&path).expect("reload after reset"));
    let _ = fs::remove_dir_all(path.parent().expect("parent"));
}

#[test]
fn write_project_setting_bool_routes_compaction_enabled() {
    // arrange
    // act
    // assert
    let path = temp_config_path("compaction-api");
    fs::write(&path, minimal_runtime_config(true)).expect("write fixture");
    let effective =
        write_project_setting_bool(&path, "runtime.compaction.enabled", false).expect("write");
    assert!(!effective);
    let restored =
        reset_project_setting_to_default(&path, "runtime.compaction.enabled").expect("reset");
    assert_eq!(restored, "true");
    let _ = fs::remove_dir_all(path.parent().expect("parent"));
}

#[test]
fn write_project_compaction_auto_retry_overflow_persists_and_reloads() {
    // arrange
    // act
    // assert
    // Given: project runtime config without auto_retry_overflow override
    let path = temp_config_path("auto-retry");
    fs::write(&path, minimal_runtime_config(true)).expect("write fixture");
    assert!(read_effective_compaction_auto_retry_overflow(&path).expect("read default"));

    // When: write false
    let effective =
        write_project_compaction_auto_retry_overflow(&path, false).expect("write false");

    // Then: effective false and nested key present
    assert!(!effective);
    let body = fs::read_to_string(&path).expect("reread");
    assert!(
        body.contains("\"auto_retry_overflow\": false")
            || body.contains("\"auto_retry_overflow\":false"),
        "body={body}"
    );
    assert!(!read_effective_compaction_auto_retry_overflow(&path).expect("reload"));

    // When: reset via generic API
    let restored =
        reset_project_setting_to_default(&path, "runtime.compaction.auto_retry_overflow")
            .expect("reset");
    assert_eq!(restored, "true");
    assert!(read_effective_compaction_auto_retry_overflow(&path).expect("reload after reset"));
    let _ = fs::remove_dir_all(path.parent().expect("parent"));
}

#[test]
fn write_project_deterministic_enabled_persists_and_reloads() {
    // arrange
    // act
    // assert
    // Given: project runtime config with deterministic.enabled=false
    let path = temp_config_path("deterministic");
    fs::write(&path, minimal_runtime_config(true)).expect("write fixture");
    assert!(!read_effective_deterministic_enabled(&path).expect("read default"));

    // When: write true
    let effective = write_project_deterministic_enabled(&path, true).expect("write true");

    // Then: effective true and nested key present
    assert!(effective);
    let body = fs::read_to_string(&path).expect("reread");
    assert!(
        body.contains("\"enabled\": true") || body.contains("\"enabled\":true"),
        "body={body}"
    );
    assert!(body.contains("\"deterministic\""));
    assert!(read_effective_deterministic_enabled(&path).expect("reload"));

    // When: reset to registry default false
    let restored = reset_project_deterministic_enabled(&path).expect("reset");
    assert!(!restored);
    assert!(!read_effective_deterministic_enabled(&path).expect("reload after reset"));

    // When: generic API routes
    let again = write_project_setting_bool(&path, "runtime.deterministic.enabled", true)
        .expect("generic write");
    assert!(again);
    let restored = reset_project_setting_to_default(&path, "runtime.deterministic.enabled")
        .expect("generic reset");
    assert_eq!(restored, "false");
    let _ = fs::remove_dir_all(path.parent().expect("parent"));
}

#[test]
fn write_project_compaction_structured_summary_contract_persists_and_reloads() {
    // arrange
    // act
    // assert
    // Given: project runtime config without structured_summary_contract override
    let path = temp_config_path("structured-summary");
    fs::write(&path, minimal_runtime_config(true)).expect("write fixture");
    assert!(read_effective_compaction_structured_summary_contract(&path).expect("read default"));

    // When: write false
    let effective =
        write_project_compaction_structured_summary_contract(&path, false).expect("write false");

    // Then: effective false and nested key present
    assert!(!effective);
    let body = fs::read_to_string(&path).expect("reread");
    assert!(
        body.contains("\"structured_summary_contract\": false")
            || body.contains("\"structured_summary_contract\":false"),
        "body={body}"
    );
    assert!(!read_effective_compaction_structured_summary_contract(&path).expect("reload"));

    // When: reset via generic API
    let restored =
        reset_project_setting_to_default(&path, "runtime.compaction.structured_summary_contract")
            .expect("reset");
    assert_eq!(restored, "true");
    assert!(
        read_effective_compaction_structured_summary_contract(&path).expect("reload after reset")
    );

    // When: generic write routes
    let again = write_project_setting_bool(
        &path,
        "runtime.compaction.structured_summary_contract",
        false,
    )
    .expect("generic write");
    assert!(!again);
    let _ = fs::remove_dir_all(path.parent().expect("parent"));
}
