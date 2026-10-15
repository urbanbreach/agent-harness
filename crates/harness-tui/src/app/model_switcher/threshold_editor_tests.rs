//! Parity scenarios for the model switcher's per-model threshold editor.

use std::path::{Path, PathBuf};

use harness_core::config::{
    read_project_setting_value, write_project_setting_value, ModelLimitProvenance,
    ResolvedModelLimits,
};
use serde_json::{json, Value};

use super::threshold_editor::ThresholdLineTone;
use super::*;

const MODEL_KEY: &str = "default/gpt-4o-mini";
const THRESHOLDS: &str = "runtime.compaction.model_thresholds";

fn config(thresholds: Value) -> String {
    json!({
        "providers": {"default": {
            "type": "openai_compatible",
            "base_url": "http://127.0.0.1:8317/v1",
            "api_key": "test-key",
            "models": {"gpt-4o-mini": {"display_name": "GPT-4o mini"}}
        }},
        "model": "default:gpt-4o-mini",
        "permissions": {"defaults": {"edit": "ask", "shell": "ask", "network": "deny"}},
        "runtime": {
            "session_dir": ".harness/sessions",
            "deterministic": {"enabled": false, "seed": 42},
            "compaction": {"model_thresholds": thresholds}
        },
        "edit": {"mode": "auto"}
    })
    .to_string()
}

fn switcher(window: Option<u32>, thresholds: Value) -> (AppState, tempfile::TempDir, PathBuf) {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("harness.json");
    std::fs::write(&path, config(thresholds)).expect("write config");
    let mut app = AppState::new_live(None, false, None);
    app.bind_settings_project_config(&path, true, true, true, true, false);
    let mut option = ModelOption::from_model_ref("default", "default:gpt-4o-mini");
    option.model_limits = ResolvedModelLimits::from_values(
        window,
        None,
        None,
        ModelLimitProvenance::explicit("test"),
    );
    app.model_options = vec![option];
    app.model_filtered = vec![0];
    app.model_selected = 0;
    app.model_switcher_visible = true;
    (app, dir, path)
}

fn key(code: KeyCode, modifiers: KeyModifiers) -> KeyEvent {
    KeyEvent::new(code, modifiers)
}

fn open(app: &mut AppState) {
    assert!(app.handle_model_key(&key(KeyCode::Char('t'), KeyModifiers::CONTROL)));
    assert!(app.threshold_editor_open());
}

/// Replace the whole input with `text` and press Enter.
fn submit(app: &mut AppState, text: &str) {
    app.handle_model_key(&key(KeyCode::Char('a'), KeyModifiers::CONTROL));
    if text.is_empty() {
        app.handle_model_key(&key(KeyCode::Backspace, KeyModifiers::NONE));
    }
    for character in text.chars() {
        app.handle_model_key(&key(KeyCode::Char(character), KeyModifiers::NONE));
    }
    app.handle_model_key(&key(KeyCode::Enter, KeyModifiers::NONE));
}

fn input(app: &AppState) -> String {
    app.threshold_editor_view().expect("editor open").input
}

fn lines(app: &AppState) -> Vec<(ThresholdLineTone, String)> {
    app.threshold_editor_view().expect("editor open").lines
}

fn stored(path: &Path) -> Value {
    read_project_setting_value(path, THRESHOLDS)
        .expect("read thresholds")
        .and_then(|text| serde_json::from_str(&text).ok())
        .unwrap_or(Value::Null)
}

#[test]
fn typed_values_round_trip_through_the_project_file() {
    // (typed, persisted spelling, shown again when reopened)
    let cases = [
        ("80%", json!("80%"), "80%"),
        ("f150000", json!("f150000"), "f150K"),
        ("150K", json!(150_000), "150K"),
    ];
    let (mut app, _dir, path) = switcher(Some(200_000), json!({}));
    for (typed, persisted, shown) in cases {
        open(&mut app);
        submit(&mut app, typed);
        assert!(!app.threshold_editor_open(), "{typed} should save");
        assert_eq!(stored(&path)[MODEL_KEY], persisted, "{typed}");
        open(&mut app);
        assert_eq!(input(&app), shown, "{typed}");
        app.handle_model_key(&key(KeyCode::Esc, KeyModifiers::NONE));
        assert!(!app.threshold_editor_open());
        assert!(app.model_switcher_visible, "Esc only leaves the editor");
    }
}

#[test]
fn empty_input_resets_and_keeps_a_wildcard_in_force() {
    let (mut app, _dir, path) =
        switcher(Some(200_000), json!({"default/*": "70%", MODEL_KEY: "90%"}));
    open(&mut app);
    assert_eq!(input(&app), "90%");
    submit(&mut app, "");
    let map = stored(&path);
    assert!(map.get(MODEL_KEY).is_none());
    assert_eq!(map["default/*"], json!("70%"));

    open(&mut app);
    assert_eq!(input(&app), "", "a wildcard is inherited, not prefilled");
    assert!(lines(&app)
        .iter()
        .any(|(_, line)| line.contains("default/*")));
}

#[test]
fn a_fixed_trigger_above_the_window_is_refused() {
    let (mut app, _dir, path) = switcher(Some(200_000), json!({}));
    open(&mut app);
    submit(&mut app, "f250000");
    assert!(app.threshold_editor_open());
    assert!(stored(&path).get(MODEL_KEY).is_none());
    assert!(lines(&app)
        .iter()
        .any(|(tone, _)| *tone == ThresholdLineTone::Error));
}

#[test]
fn preview_names_its_origin_and_is_suppressed_when_it_cannot_be_trusted() {
    let preview = |app: &AppState| {
        lines(app)
            .into_iter()
            .find(|(_, line)| line.starts_with("Compacts at"))
            .map(|(_, line)| line)
    };
    let type_only = |app: &mut AppState, text: &str| {
        app.handle_model_key(&key(KeyCode::Char('a'), KeyModifiers::CONTROL));
        for character in text.chars() {
            app.handle_model_key(&key(KeyCode::Char(character), KeyModifiers::NONE));
        }
    };

    let (mut app, _dir, path) = switcher(Some(200_000), json!({}));
    open(&mut app);
    type_only(&mut app, "f150000");
    let fixed = preview(&app).expect("fixed preview");
    assert!(
        fixed.contains("150K") && fixed.contains("configured fixed"),
        "{fixed}"
    );
    type_only(&mut app, "80%");
    let scaled = preview(&app).expect("percent preview");
    assert!(
        scaled.contains("160K") && scaled.contains("scaled from base"),
        "{scaled}"
    );
    type_only(&mut app, "200K");
    assert!(lines(&app)
        .iter()
        .any(|(tone, _)| *tone == ThresholdLineTone::Warning));
    type_only(&mut app, "not a threshold");
    assert!(preview(&app).is_none());

    write_project_setting_value(
        &path,
        "runtime.compaction.model_thresholds_enabled",
        "false",
    )
    .expect("disable");
    app.bind_settings_project_config(&path, true, true, true, true, false);
    type_only(&mut app, "80%");
    assert!(preview(&app).is_none());

    let (mut unknown, _dir, _path) = switcher(None, json!({}));
    open(&mut unknown);
    type_only(&mut unknown, "80%");
    assert!(preview(&unknown).is_none());
}
