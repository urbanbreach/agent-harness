use super::*;
use harness_core::config::settings_registry;
use std::path::Path;

pub(super) fn settings_editor_opens_and_lists_registry_rows() {
    // Given: live app
    let mut app = AppState::new_live(None, false, None);

    // When
    app.execute_action(Action::OpenSettings);

    // Then: overlay visible with registry-bound rows
    assert!(app.settings_editor_is_visible());
    assert_eq!(app.overlay_stack().top(), Some(OverlayKind::SettingsEditor));
    let rows = app.settings_editor_rows();
    assert!(!rows.is_empty());
    assert_eq!(rows.len(), settings_registry().len());
    assert!(rows.iter().any(|row| !row.setting_id.is_empty()));
    assert!(rows
        .iter()
        .any(|row| { matches!(row.sensitivity.as_str(), "public" | "redacted" | "secret") }));
    assert_eq!(rows.iter().filter(|r| r.selected).count(), 1);
}

pub(super) fn settings_editor_navigates_and_closes_on_esc() {
    // Given: open settings editor
    let mut app = AppState::new_live(None, false, None);
    app.execute_action(Action::OpenSettings);
    assert!(app.settings_editor_is_visible());
    let len = settings_registry().len();
    assert!(len >= 2);

    // When: move down
    app.handle_key(key(KeyCode::Down));

    // Then: selection advances
    assert_eq!(app.settings_editor_selected_index(), 1);
    assert!(app.settings_editor_rows()[1].selected);

    // When: Esc
    app.handle_key(key(KeyCode::Esc));

    // Then: closed
    assert!(!app.settings_editor_is_visible());
    assert_ne!(app.overlay_stack().top(), Some(OverlayKind::SettingsEditor));
}

pub(super) fn settings_slash_command_opens_settings_editor() {
    // Given
    let mut app = AppState::new_live(None, false, None);

    // When
    app.execute_slash_command("settings", None);

    // Then
    assert!(app.settings_editor_is_visible());
    assert!(!app.settings_editor_rows().is_empty());
}

pub(super) fn settings_editor_toggles_hashline_edit_persists_and_reloads() {
    // Given: bound project config with hashline_edit=true and open editor
    let dir = std::env::temp_dir().join(format!(
        "harness-tui-settings-{}-{}",
        "toggle",
        std::process::id()
    ));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).expect("temp dir");
    let path = dir.join("harness.json");
    fs::write(
        &path,
        r#"{
  "providers": {
    "default": {
      "type": "openai_compatible",
      "base_url": "http://127.0.0.1:8317/v1",
      "api_key": "test-key",
      "models": { "gpt-4o-mini": { "display_name": "GPT-4o mini" } }
    }
  },
  "agents": {
    "build": {
      "description": "Build work",
      "model_ref": "default:gpt-4o-mini",
      "tools": ["read"]
    }
  },
  "permissions": {
    "defaults": { "edit": "ask", "shell": "ask", "network": "deny" }
  },
  "runtime": {
    "background_tasks": {
      "default_concurrency": 2,
      "provider_concurrency": 2,
      "model_concurrency": 2,
      "stale_timeout_ms": 15000,
      "message_staleness_timeout_ms": 5000
    },
    "session_dir": ".agent-harness/sessions",
    "deterministic": { "enabled": false, "seed": 42 }
  },
  "integrations": {
    "remote_search": { "endpoint": "https://mcp.exa.ai/mcp" }
  },
  "hashline_edit": true
}"#,
    )
    .expect("write fixture");

    let mut app = AppState::new_live(None, false, None);
    app.bind_settings_project_config(&path, true, true, true, true, true, false);
    app.execute_action(Action::OpenSettings);
    assert!(app.settings_editor_is_visible());
    assert!(app.settings_hashline_edit());

    // When: select hashline_edit and activate
    let hashline_index = settings_registry()
        .iter()
        .position(|entry| entry.setting_id.as_str() == "hashline_edit")
        .expect("hashline_edit registered");
    app.settings_editor_selected = hashline_index;
    app.handle_key(key(KeyCode::Enter));

    // Then: effective flipped and file persisted
    assert!(!app.settings_hashline_edit());
    let body = fs::read_to_string(&path).expect("reread");
    assert!(body.contains("\"hashline_edit\": false") || body.contains("\"hashline_edit\":false"));
    let row = app
        .settings_editor_rows()
        .into_iter()
        .find(|row| row.setting_id == "hashline_edit")
        .expect("row");
    assert_eq!(row.effective_value.as_deref(), Some("false"));
    assert!(row.editable);

    // When: reset
    app.handle_key(key(KeyCode::Char('r')));

    // Then: default true again
    assert!(app.settings_hashline_edit());
    let _ = fs::remove_dir_all(&dir);
}

pub(super) fn settings_editor_fails_closed_for_secret_setting() {
    // Given: open settings on secret row with bound config
    let dir = std::env::temp_dir().join(format!(
        "harness-tui-settings-{}-{}",
        "secret",
        std::process::id()
    ));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).expect("temp dir");
    let path = dir.join("harness.json");
    fs::write(&path, "{}").expect("write empty");

    let mut app = AppState::new_live(None, false, None);
    app.bind_settings_project_config(&path, true, true, true, true, true, false);
    app.execute_action(Action::OpenSettings);
    let secret_index = settings_registry()
        .iter()
        .position(|entry| entry.setting_id.as_str() == "provider.apiKey")
        .expect("provider.apiKey registered");
    app.settings_editor_selected = secret_index;
    let before = fs::read_to_string(&path).expect("before");

    // When
    app.handle_key(key(KeyCode::Enter));

    // Then: file unchanged; hashline value unchanged
    assert_eq!(fs::read_to_string(&path).expect("after"), before);
    assert!(app.settings_hashline_edit());
    let _ = fs::remove_dir_all(&dir);
}

pub(super) fn pending_settings_project_config_is_applied_on_new_live() {
    // Given: staged project config binding for next live AppState
    let dir = std::env::temp_dir().join(format!(
        "harness-tui-settings-{}-{}",
        "pending",
        std::process::id()
    ));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).expect("temp dir");
    let path = dir.join("harness.json");
    fs::write(&path, r#"{"hashline_edit": false}"#).expect("write fixture");
    set_pending_settings_project_config(path.clone(), false, true, true, true, true, false);

    // When: live AppState is constructed
    let app = AppState::new_live(None, false, None);

    // Then: settings editor is bound and seeded
    assert_eq!(
        app.settings_project_config_path().map(Path::to_path_buf),
        Some(path)
    );
    assert!(!app.settings_hashline_edit());
    assert!(app.settings_compaction_enabled());
    let _ = fs::remove_dir_all(&dir);
}

pub(super) fn settings_editor_toggles_compaction_enabled_persists_and_reloads() {
    // Given: bound project config with default compaction enabled
    let dir = std::env::temp_dir().join(format!(
        "harness-tui-settings-{}-{}",
        "compaction",
        std::process::id()
    ));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).expect("temp dir");
    let path = dir.join("harness.json");
    fs::write(
        &path,
        r#"{
  "providers": {
    "default": {
      "type": "openai_compatible",
      "base_url": "http://127.0.0.1:8317/v1",
      "api_key": "test-key",
      "models": { "gpt-4o-mini": { "display_name": "GPT-4o mini" } }
    }
  },
  "agents": {
    "build": {
      "description": "Build work",
      "model_ref": "default:gpt-4o-mini",
      "tools": ["read"]
    }
  },
  "permissions": {
    "defaults": { "edit": "ask", "shell": "ask", "network": "deny" }
  },
  "runtime": {
    "background_tasks": {
      "default_concurrency": 2,
      "provider_concurrency": 2,
      "model_concurrency": 2,
      "stale_timeout_ms": 15000,
      "message_staleness_timeout_ms": 5000
    },
    "session_dir": ".agent-harness/sessions",
    "deterministic": { "enabled": false, "seed": 42 }
  },
  "integrations": {
    "remote_search": { "endpoint": "https://mcp.exa.ai/mcp" }
  },
  "hashline_edit": true
}"#,
    )
    .expect("write fixture");

    let mut app = AppState::new_live(None, false, None);
    app.bind_settings_project_config(&path, true, true, true, true, true, false);
    app.execute_action(Action::OpenSettings);
    assert!(app.settings_compaction_enabled());

    // When: select runtime.compaction.enabled and activate
    let compaction_index = settings_registry()
        .iter()
        .position(|entry| entry.setting_id.as_str() == "runtime.compaction.enabled")
        .expect("runtime.compaction.enabled registered");
    app.settings_editor_selected = compaction_index;
    app.handle_key(key(KeyCode::Enter));

    // Then: effective flipped and nested key persisted
    assert!(!app.settings_compaction_enabled());
    let body = fs::read_to_string(&path).expect("reread");
    assert!(
        body.contains("\"enabled\": false") || body.contains("\"enabled\":false"),
        "body={body}"
    );
    assert!(body.contains("\"compaction\""));
    let row = app
        .settings_editor_rows()
        .into_iter()
        .find(|row| row.setting_id == "runtime.compaction.enabled")
        .expect("row");
    assert_eq!(row.effective_value.as_deref(), Some("false"));
    assert!(row.editable);

    // When: reset
    app.handle_key(key(KeyCode::Char('r')));

    // Then: default true again
    assert!(app.settings_compaction_enabled());
    let _ = fs::remove_dir_all(&dir);
}

#[test]
pub(super) fn settings_editor_toggles_compaction_auto_retry_overflow_persists_and_reloads() {
    // arrange
    // act
    // assert
    // Given: bound project config (same shape as compaction.enabled fixture)
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("harness.json");
    fs::write(
        &path,
        r#"{
  "providers": {
    "default": {
      "type": "openai_compatible",
      "base_url": "http://127.0.0.1:8317/v1",
      "api_key": "test-key",
      "models": { "gpt-4o-mini": { "display_name": "GPT-4o mini" } }
    }
  },
  "agents": {
    "build": {
      "description": "Build work",
      "model_ref": "default:gpt-4o-mini",
      "tools": ["read"]
    }
  },
  "permissions": {
    "defaults": { "edit": "ask", "shell": "ask", "network": "deny" }
  },
  "runtime": {
    "background_tasks": {
      "default_concurrency": 2,
      "provider_concurrency": 2,
      "model_concurrency": 2,
      "stale_timeout_ms": 15000,
      "message_staleness_timeout_ms": 5000
    },
    "session_dir": ".agent-harness/sessions",
    "deterministic": { "enabled": false, "seed": 42 }
  },
  "integrations": {
    "remote_search": { "endpoint": "https://mcp.exa.ai/mcp" }
  },
  "hashline_edit": true
}"#,
    )
    .expect("write fixture");

    let mut app = AppState::new_live(None, false, None);
    app.bind_settings_project_config(&path, true, true, true, true, true, false);
    app.execute_action(Action::OpenSettings);
    assert!(app.settings_compaction_auto_retry_overflow());

    // When: select auto_retry_overflow and activate
    let index = settings_registry()
        .iter()
        .position(|entry| entry.setting_id.as_str() == "runtime.compaction.auto_retry_overflow")
        .expect("auto_retry_overflow registered");
    app.settings_editor_selected = index;
    app.handle_key(key(KeyCode::Enter));

    // Then: flipped and nested key persisted
    assert!(!app.settings_compaction_auto_retry_overflow());
    let body = fs::read_to_string(&path).expect("reread");
    assert!(
        body.contains("\"auto_retry_overflow\": false")
            || body.contains("\"auto_retry_overflow\":false"),
        "body={body}"
    );

    // When: reset
    app.handle_key(key(KeyCode::Char('r')));

    // Then: default true again
    assert!(app.settings_compaction_auto_retry_overflow());
    let _ = fs::remove_dir_all(&dir);
}

pub(super) fn settings_editor_summary_counts_bound_writable_paths() {
    // Given: unbound editor
    let mut app = AppState::new_live(None, false, None);
    app.execute_action(Action::OpenSettings);
    let unbound = app.settings_editor_summary();
    assert_eq!(unbound.total, settings_registry().len());
    assert!(!unbound.bound);
    assert_eq!(unbound.editable, 0);
    assert_eq!(unbound.writable_paths, 6);
    assert!(unbound.secret > 0);
    assert_eq!(unbound.editable + unbound.read_only, unbound.total);
    assert!(unbound.one_line().starts_with("settings editor: "));
    assert!(unbound.one_line().contains("bound=false"));
    assert!(unbound.overlay_line().contains("unbound"));
    assert!(unbound.overlay_line().contains("write paths"));

    // When: bind project config
    let dir = std::env::temp_dir().join(format!(
        "harness-tui-settings-summary-{}-{}",
        "bound",
        std::process::id()
    ));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).expect("workspace");
    let path = dir.join("harness.json");
    fs::write(&path, r#"{ "hashline_edit": true }"#).expect("write config");
    app.bind_settings_project_config(&path, true, true, true, true, true, false);

    // Then: three write paths become editable with effective values
    let bound = app.settings_editor_summary();
    assert!(bound.bound);
    assert_eq!(bound.writable_paths, 6);
    assert_eq!(bound.editable, 6);
    assert!(bound.has_editable());
    assert_eq!(bound.with_effective_value, 6);
    assert!(bound.one_line().contains("bound=true"));
    assert!(bound.one_line().contains("editable=6"));
    assert!(bound.overlay_line().contains("bound"));
    assert!(bound.overlay_line().contains("editable"));
    assert!(!bound.overlay_line().contains("unbound"));

    let _ = fs::remove_dir_all(&dir);
}

pub(super) fn settings_editor_toggles_deterministic_enabled_persists_and_reloads() {
    // Given: bound project config with deterministic.enabled=false and open editor
    let dir = std::env::temp_dir().join(format!(
        "harness-tui-settings-det-{}-{}",
        "toggle",
        std::process::id()
    ));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).expect("temp dir");
    let path = dir.join("harness.json");
    fs::write(
        &path,
        r#"{
  "providers": {
    "default": {
      "type": "openai_compatible",
      "base_url": "http://127.0.0.1:8317/v1",
      "api_key": "test-key",
      "models": { "gpt-4o-mini": { "display_name": "GPT-4o mini" } }
    }
  },
  "agents": {
    "build": {
      "description": "Build work",
      "model_ref": "default:gpt-4o-mini",
      "tools": ["read"]
    }
  },
  "permissions": {
    "defaults": { "edit": "ask", "shell": "ask", "network": "deny" }
  },
  "runtime": {
    "background_tasks": {
      "default_concurrency": 2,
      "provider_concurrency": 2,
      "model_concurrency": 2,
      "stale_timeout_ms": 15000,
      "message_staleness_timeout_ms": 5000
    },
    "session_dir": ".agent-harness/sessions",
    "deterministic": { "enabled": false, "seed": 42 }
  },
  "integrations": {
    "remote_search": { "endpoint": "https://mcp.exa.ai/mcp" }
  },
  "hashline_edit": true
}"#,
    )
    .expect("write fixture");

    let mut app = AppState::new_live(None, false, None);
    app.bind_settings_project_config(&path, true, true, true, true, true, false);
    app.execute_action(Action::OpenSettings);
    assert!(!app.settings_deterministic_enabled());

    // When: select deterministic.enabled and activate
    let index = settings_registry()
        .iter()
        .position(|entry| entry.setting_id.as_str() == "runtime.deterministic.enabled")
        .expect("deterministic.enabled registered");
    app.settings_editor_selected = index;
    app.handle_key(key(KeyCode::Enter));

    // Then: flipped and nested key persisted
    assert!(app.settings_deterministic_enabled());
    let body = fs::read_to_string(&path).expect("reread");
    assert!(
        body.contains("\"enabled\": true") || body.contains("\"enabled\":true"),
        "body={body}"
    );
    assert!(body.contains("\"deterministic\""));

    // When: reset
    app.handle_key(key(KeyCode::Char('r')));

    // Then: default false again
    assert!(!app.settings_deterministic_enabled());
    let _ = fs::remove_dir_all(&dir);
}

pub(super) fn settings_editor_toggles_compaction_structured_summary_contract_persists_and_reloads()
{
    // Given: bound project config (same shape as compaction.enabled fixture)
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("harness.json");
    fs::write(
        &path,
        r#"{
  "providers": {
    "default": {
      "type": "openai_compatible",
      "base_url": "http://127.0.0.1:8317/v1",
      "api_key": "test-key",
      "models": { "gpt-4o-mini": { "display_name": "GPT-4o mini" } }
    }
  },
  "agents": {
    "build": {
      "description": "Build work",
      "model_ref": "default:gpt-4o-mini",
      "tools": ["read"]
    }
  },
  "permissions": {
    "defaults": { "edit": "ask", "shell": "ask", "network": "deny" }
  },
  "runtime": {
    "background_tasks": {
      "default_concurrency": 2,
      "provider_concurrency": 2,
      "model_concurrency": 2,
      "stale_timeout_ms": 15000,
      "message_staleness_timeout_ms": 5000
    },
    "session_dir": ".agent-harness/sessions",
    "deterministic": { "enabled": false, "seed": 42 }
  },
  "integrations": {
    "remote_search": { "endpoint": "https://mcp.exa.ai/mcp" }
  },
  "hashline_edit": true
}"#,
    )
    .expect("write fixture");

    let mut app = AppState::new_live(None, false, None);
    app.bind_settings_project_config(&path, true, true, true, true, true, false);
    app.execute_action(Action::OpenSettings);
    assert!(app.settings_compaction_structured_summary_contract());

    // When: select structured_summary_contract and activate
    let index = settings_registry()
        .iter()
        .position(|entry| {
            entry.setting_id.as_str() == "runtime.compaction.structured_summary_contract"
        })
        .expect("structured_summary_contract registered");
    app.settings_editor_selected = index;
    app.handle_key(key(KeyCode::Enter));

    // Then: flipped and nested key persisted
    assert!(!app.settings_compaction_structured_summary_contract());
    let body = fs::read_to_string(&path).expect("reread");
    assert!(
        body.contains("\"structured_summary_contract\": false")
            || body.contains("\"structured_summary_contract\":false"),
        "body={body}"
    );

    // When: reset
    app.handle_key(key(KeyCode::Char('r')));

    // Then: default true again
    assert!(app.settings_compaction_structured_summary_contract());
}

pub(super) fn settings_editor_toggles_compaction_estimated_token_triggers_persists_and_reloads() {
    // Given: bound project config
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("harness.json");
    fs::write(
        &path,
        r#"{
  "providers": {
    "default": {
      "type": "openai_compatible",
      "base_url": "http://127.0.0.1:8317/v1",
      "api_key": "test-key",
      "models": { "gpt-4o-mini": { "display_name": "GPT-4o mini" } }
    }
  },
  "agents": {
    "build": {
      "description": "Build work",
      "model_ref": "default:gpt-4o-mini",
      "tools": ["read"]
    }
  },
  "permissions": {
    "defaults": { "edit": "ask", "shell": "ask", "network": "deny" }
  },
  "runtime": {
    "background_tasks": {
      "default_concurrency": 2,
      "provider_concurrency": 2,
      "model_concurrency": 2,
      "stale_timeout_ms": 15000,
      "message_staleness_timeout_ms": 5000
    },
    "session_dir": ".agent-harness/sessions",
    "deterministic": { "enabled": false, "seed": 42 }
  },
  "integrations": {
    "remote_search": { "endpoint": "https://mcp.exa.ai/mcp" }
  },
  "hashline_edit": true
}"#,
    )
    .expect("write fixture");

    let mut app = AppState::new_live(None, false, None);
    app.bind_settings_project_config(&path, true, true, true, true, true, false);
    app.execute_action(Action::OpenSettings);
    assert!(app.settings_compaction_estimated_token_triggers());

    // When: select estimated_token_triggers and activate
    let index = settings_registry()
        .iter()
        .position(|entry| {
            entry.setting_id.as_str() == "runtime.compaction.estimated_token_triggers"
        })
        .expect("estimated_token_triggers registered");
    app.settings_editor_selected = index;
    app.handle_key(key(KeyCode::Enter));

    // Then: flipped and nested key persisted
    assert!(!app.settings_compaction_estimated_token_triggers());
    let body = fs::read_to_string(&path).expect("reread");
    assert!(
        body.contains("\"estimated_token_triggers\": false")
            || body.contains("\"estimated_token_triggers\":false"),
        "body={body}"
    );

    // When: reset
    app.handle_key(key(KeyCode::Char('r')));

    // Then: default true again
    assert!(app.settings_compaction_estimated_token_triggers());
}

pub(super) fn settings_editor_e2e_open_edit_persist_and_read_effective() {
    // Given: valid project harness.json bound from read_effective_*
    use harness_core::config::{
        read_effective_compaction_enabled, read_effective_hashline_edit,
        write_project_hashline_edit,
    };

    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("harness.json");
    fs::write(
        &path,
        r#"{
  "providers": {
    "default": {
      "type": "openai_compatible",
      "base_url": "http://127.0.0.1:8317/v1",
      "api_key": "test-key",
      "models": { "gpt-4o-mini": { "display_name": "GPT-4o mini" } }
    }
  },
  "agents": {
    "build": {
      "description": "Build work",
      "model_ref": "default:gpt-4o-mini",
      "tools": ["read"]
    }
  },
  "permissions": {
    "defaults": { "edit": "ask", "shell": "ask", "network": "deny" }
  },
  "runtime": {
    "background_tasks": {
      "default_concurrency": 2,
      "provider_concurrency": 2,
      "model_concurrency": 2,
      "stale_timeout_ms": 15000,
      "message_staleness_timeout_ms": 5000
    },
    "session_dir": ".agent-harness/sessions",
    "deterministic": { "enabled": false, "seed": 42 },
    "compaction": { "enabled": true }
  },
  "integrations": {
    "remote_search": { "endpoint": "https://mcp.exa.ai/mcp" }
  },
  "hashline_edit": true
}"#,
    )
    .expect("write fixture");

    let hashline = read_effective_hashline_edit(&path).expect("hashline effective");
    let compaction = read_effective_compaction_enabled(&path).expect("compaction effective");
    assert!(hashline);
    assert!(compaction);

    let mut app = AppState::new_live(None, false, None);
    app.bind_settings_project_config(&path, hashline, compaction, true, true, true, false);

    // When: open settings editor (product path)
    app.execute_action(Action::OpenSettings);
    assert!(app.settings_editor_is_visible());
    assert_eq!(app.overlay_stack().top(), Some(OverlayKind::SettingsEditor));
    let summary = app.settings_editor_summary();
    assert!(summary.bound);
    assert_eq!(summary.writable_paths, 6);
    assert_eq!(summary.editable, 6);
    assert!(summary.with_effective_value >= 6);

    // When: edit hashline_edit via Enter
    let hashline_index = settings_registry()
        .iter()
        .position(|entry| entry.setting_id.as_str() == "hashline_edit")
        .expect("hashline_edit registered");
    app.settings_editor_selected = hashline_index;
    app.handle_key(key(KeyCode::Enter));

    // Then: AppState flipped, file persisted, read_effective matches
    assert!(!app.settings_hashline_edit());
    let effective = read_effective_hashline_edit(&path).expect("reread effective");
    assert!(!effective);
    let row = app
        .settings_editor_rows()
        .into_iter()
        .find(|row| row.setting_id == "hashline_edit")
        .expect("row");
    assert_eq!(row.effective_value.as_deref(), Some("false"));
    assert!(row.editable);

    // When: write another value via backend and re-bind from read_effective
    write_project_hashline_edit(&path, true).expect("write true");
    let reloaded = read_effective_hashline_edit(&path).expect("reload");
    app.bind_settings_project_config(&path, reloaded, compaction, true, true, true, false);
    assert!(app.settings_hashline_edit());
    let row = app
        .settings_editor_rows()
        .into_iter()
        .find(|row| row.setting_id == "hashline_edit")
        .expect("row after rebind");
    assert_eq!(row.effective_value.as_deref(), Some("true"));
}
