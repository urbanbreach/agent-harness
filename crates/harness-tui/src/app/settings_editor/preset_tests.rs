//! Parity scenarios for the grouped settings rows and their preset menus.

use std::path::PathBuf;

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use harness_core::config::{read_project_setting_value, write_project_setting_value};

use super::*;

const CONFIG: &str = r#"{
  "providers": {
    "default": {
      "type": "openai_compatible",
      "base_url": "http://127.0.0.1:8317/v1",
      "api_key": "test-key",
      "models": { "gpt-4o-mini": { "display_name": "GPT-4o mini" } }
    }
  },
  "model": "default:gpt-4o-mini",
  "permissions": { "defaults": { "edit": "ask", "shell": "ask", "network": "deny" } },
  "runtime": {
    "session_dir": ".harness/sessions",
    "deterministic": { "enabled": false, "seed": 42 }
  },
  "edit": { "mode": "auto" }
}"#;

fn press(app: &mut AppState, code: KeyCode) {
    app.handle_key(KeyEvent::new(code, KeyModifiers::NONE));
}

fn bound_app() -> (AppState, tempfile::TempDir, PathBuf) {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("harness.json");
    std::fs::write(&path, CONFIG).expect("write config");
    let mut app = AppState::new_live(None, false, None);
    app.bind_settings_project_config(&path, true, true, true, true, false);
    app.open_settings_editor();
    (app, dir, path)
}

fn select(app: &mut AppState, id: &str) {
    app.settings_editor_selected = settings_registry()
        .iter()
        .position(|entry| entry.setting_id.as_str() == id)
        .expect("registered setting");
}

fn menu_labels(app: &AppState) -> Vec<String> {
    app.settings_interaction
        .edit
        .as_ref()
        .and_then(|edit| edit.menu.as_ref())
        .map(PresetMenu::labels)
        .unwrap_or_default()
}

/// Open the menu of `id`, move to the entry named `label` (`None` = the Default entry) and save.
fn choose(app: &mut AppState, id: &str, label: Option<&str>) {
    select(app, id);
    press(app, KeyCode::Enter);
    let labels = menu_labels(app);
    let index = match label {
        Some(label) => labels
            .iter()
            .position(|candidate| candidate == label)
            .unwrap_or_else(|| panic!("no `{label}` in {labels:?}")),
        None => 0,
    };
    app.settings_editor_choose(index);
    press(app, KeyCode::Enter);
    assert!(app.settings_interaction.edit.is_none(), "menu stays open");
}

#[test]
fn presets_write_the_value_and_default_removes_the_key() {
    // (setting, entry label, stored value)
    let cases = [
        ("runtime.compaction.threshold_percent", "80%", "80"),
        (
            "runtime.compaction.threshold_tokens",
            "150K tokens",
            "150000",
        ),
        (
            "runtime.compaction.idle_threshold_tokens",
            "900K tokens",
            "900000",
        ),
        (
            "runtime.compaction.idle_timeout_seconds",
            "30 minutes",
            "1800",
        ),
        (
            "runtime.compaction.archive.system_prompt",
            "AGENTS.md",
            "agents_md",
        ),
        (
            "runtime.compaction.archive.shape",
            "doc-8on16-sent",
            "doc-8on16-sent",
        ),
        (
            "runtime.compaction.method_order",
            "Archive first",
            r#"["archive","remote","handoff","shake","soft"]"#,
        ),
    ];
    let (mut app, _dir, path) = bound_app();
    for (id, label, stored) in cases {
        choose(&mut app, id, Some(label));
        assert_eq!(
            read_project_setting_value(&path, id)
                .expect("read")
                .as_deref(),
            Some(stored),
            "{id}"
        );
        choose(&mut app, id, None);
        assert_eq!(
            read_project_setting_value(&path, id).expect("read"),
            None,
            "{id} after Default"
        );
    }
}

#[test]
fn a_custom_value_stays_selectable_and_shown_as_custom() {
    let (mut app, _dir, path) = bound_app();
    write_project_setting_value(&path, "runtime.compaction.idle_threshold_tokens", "123456")
        .expect("write custom");
    app.bind_settings_project_config(&path, true, true, true, true, false);
    select(&mut app, "runtime.compaction.idle_threshold_tokens");
    press(&mut app, KeyCode::Enter);
    let labels = menu_labels(&app);
    let selected = app
        .settings_interaction
        .edit
        .as_ref()
        .and_then(|edit| edit.menu.as_ref())
        .map(|menu| menu.selected)
        .unwrap_or_default();
    assert_eq!(
        labels.get(selected).map(String::as_str),
        Some("Custom: 123456")
    );
}

#[test]
fn shape_menu_offers_auto_and_every_available_shape() {
    let (mut app, _dir, _path) = bound_app();
    select(&mut app, "runtime.compaction.archive.shape");
    press(&mut app, KeyCode::Enter);
    let labels = menu_labels(&app);
    assert_eq!(labels.len(), 18);
    assert_eq!(labels.first().map(String::as_str), Some("auto (default)"));
    assert!(labels.iter().any(|label| label == "silver16-bw"));
}

#[test]
fn rows_carry_curated_labels_groups_and_neutral_wording() {
    let (mut app, _dir, _path) = bound_app();
    // (setting, label, group)
    let runtime = [
        (
            "runtime.compaction.enabled",
            "Auto-Compact",
            "Context / Compaction",
        ),
        (
            "runtime.compaction.notes_windows",
            "Notes-backed context windows (experimental)",
            "Context / Experimental",
        ),
        (
            "runtime.compaction.mid_turn_enabled",
            "Mid-Turn Compaction",
            "Context / Compaction",
        ),
        (
            "runtime.compaction.method_order",
            "Compaction Method Order",
            "Context / Compaction",
        ),
        (
            "runtime.compaction.threshold_percent",
            "Compaction Threshold",
            "Context / Compaction",
        ),
        (
            "runtime.compaction.threshold_tokens",
            "Compaction Token Limit",
            "Context / Compaction",
        ),
        (
            "runtime.compaction.idle_timeout_seconds",
            "Idle Compaction Delay",
            "Context / Compaction",
        ),
        (
            "runtime.compaction.drop_useless",
            "Elide Uneventful Results",
            "Context / Compaction",
        ),
        (
            "runtime.compaction.archive.system_prompt",
            "Archive System Prompt",
            "Context / Experimental",
        ),
        (
            "runtime.compaction.archive.tool_results",
            "Archive Tool Results",
            "Context / Experimental",
        ),
        (
            "runtime.compaction.archive.shape",
            "Archive Shape",
            "Context / Experimental",
        ),
    ];
    let rows = app.settings_editor_rows();
    for (id, label, group) in runtime {
        let row = rows
            .iter()
            .find(|row| row.setting_id == id)
            .unwrap_or_else(|| panic!("no row {id}"));
        assert_eq!(
            (row.label.as_str(), row.group.as_str()),
            (label, group),
            "{id}"
        );
        assert!(row.editable, "{id}");
    }
    let notes = rows
        .iter()
        .find(|row| row.setting_id == "runtime.compaction.notes_windows")
        .and_then(|row| row.description.as_deref());
    assert_eq!(
        notes,
        Some("Keep persistent notes and searchable raw history across context windows.")
    );

    app.settings_editor_switch_tab();
    let collapse = app
        .settings_editor_rows()
        .into_iter()
        .find(|row| row.setting_id == "collapse_compacted_history")
        .expect("collapse row");
    assert_eq!(collapse.label, "Collapse Compacted History");
    assert_eq!(collapse.group, "Appearance / Display");
    assert!(collapse.editable);
}
