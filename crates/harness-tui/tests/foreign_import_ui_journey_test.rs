//! End-to-end test for sessions.foreign_import_ui_journey.
//!
//! Proves the full TUI journey: discover -> preview -> import -> events appended.
//! Uses AppState's foreign import picker overlay without a live runtime.

use std::path::PathBuf;

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use harness_tui::app::{AppState, UiIntent};

fn write_foreign_event_envelope(
    path: &std::path::Path,
    event_id: &str,
    run_id: &str,
    summary: &str,
) {
    let body = format!(
        r#"{{"schema_version":1,"event_id":"{event_id}","seq":1,"run_id":"{run_id}","mono_ms":1,"actor":{{"kind":"system"}},"payload":{{"event_type":"run_finished","data":{{"summary":"{summary}"}}}}}}
"#
    );
    std::fs::write(path, body).expect("write foreign events.jsonl");
}

fn unique_temp_dir(label: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "harness-tui-foreign-import-journey-{}-{label}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("create temp dir");
    dir
}

#[test]
fn foreign_import_picker_discover_preview_import_events_appended() {
    // -- arrange: foreign scan root with one importable candidate
    let scan_root = unique_temp_dir("scan");
    let foreign_session = scan_root.join("codex_session_alpha");
    std::fs::create_dir_all(&foreign_session).expect("create foreign session dir");
    write_foreign_event_envelope(
        &foreign_session.join("events.jsonl"),
        "evt_foreign_alpha",
        "run_foreign_alpha",
        "alpha session",
    );

    // Also create a corrupt candidate to prove classification
    let corrupt_session = scan_root.join("corrupt_session");
    std::fs::create_dir_all(&corrupt_session).expect("create corrupt session dir");
    std::fs::write(corrupt_session.join("events.jsonl"), "{not-valid-json\n")
        .expect("write corrupt events");

    // Destination session dir (simulates harness session store)
    let dest_session_dir = unique_temp_dir("dest");

    // Track emitted UiIntents
    let intents: std::sync::Arc<std::sync::Mutex<Vec<UiIntent>>> =
        std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
    let intents_clone = intents.clone();

    let mut app = AppState::new_live(
        Some(dest_session_dir.clone()),
        false,
        Some(std::sync::Arc::new(move |intent: UiIntent| {
            intents_clone.lock().expect("intents lock").push(intent);
        })),
    );

    // -- act step 1: discover (open picker)
    assert!(!app.foreign_import_picker.visible);
    app.open_foreign_import_picker(scan_root.clone());

    // -- assert: picker is visible and candidates discovered
    assert!(app.foreign_import_picker.visible);
    assert!(
        !app.foreign_import_picker.candidates.is_empty(),
        "expected discovered candidates"
    );

    // -- assert: preview - candidates include importable and corrupt
    let importable_count = app.foreign_import_picker.importable_count();
    assert!(
        importable_count >= 1,
        "expected at least one importable candidate, got {importable_count}"
    );
    let has_corrupt = app
        .foreign_import_picker
        .candidates
        .iter()
        .any(|candidate| candidate.is_corrupt());
    assert!(has_corrupt, "expected corrupt candidate in scan results");

    // -- assert: selected candidate is the first importable one (auto-selected)
    let selected = app
        .foreign_import_picker
        .selected_candidate()
        .expect("expected a selected candidate");
    assert!(
        selected.is_importable(),
        "auto-selection should land on an importable candidate"
    );

    let initial_selected = app.foreign_import_picker.selected;
    app.handle_key(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE));
    if app.foreign_import_picker.candidates.len() > 1 {
        assert_ne!(
            app.foreign_import_picker.selected, initial_selected,
            "down arrow should move selection"
        );
    }
    app.handle_key(KeyEvent::new(KeyCode::Up, KeyModifiers::NONE));

    // Navigate back to the importable candidate
    let importable_index = app
        .foreign_import_picker
        .candidates
        .iter()
        .position(|candidate| candidate.is_importable())
        .expect("must have importable candidate");
    app.foreign_import_picker.selected = importable_index;

    // -- act step 3: import (inline, proving events are appended)
    let result = app
        .execute_foreign_import_inline()
        .expect("inline import should succeed for importable candidate");

    // -- assert: import result is correct
    assert!(result.event_count >= 1, "expected imported events");
    assert_eq!(result.format, "events_jsonl_v1");
    assert!(result.run_dir.exists(), "imported run dir must exist");
    assert!(
        result.run_dir.join("events.jsonl").is_file(),
        "imported events.jsonl must be written"
    );
    assert!(
        result.run_dir.join("meta.json").is_file(),
        "imported meta.json must be written"
    );

    // -- assert: events.jsonl content is valid and appended
    let events_content =
        std::fs::read_to_string(result.run_dir.join("events.jsonl")).expect("read events");
    let event_count = events_content
        .lines()
        .filter(|line| !line.trim().is_empty())
        .count();
    assert_eq!(
        event_count, result.event_count,
        "events.jsonl line count must match reported event_count"
    );

    // -- assert: foreign source is NOT mutated
    assert!(
        foreign_session.join("events.jsonl").is_file(),
        "foreign source events.jsonl must still exist"
    );
    let foreign_content =
        std::fs::read_to_string(foreign_session.join("events.jsonl")).expect("read foreign source");
    assert!(
        foreign_content.contains("evt_foreign_alpha"),
        "foreign source must be unchanged"
    );

    // -- assert: status banner confirms import
    let banner = app
        .status_banner
        .as_deref()
        .expect("status banner must be set after import");
    assert!(banner.contains("imported"), "banner: {banner}");

    // -- assert: last_import_summary is set
    assert!(app.foreign_import_picker.last_import_summary.is_some());

    // -- cleanup
    let _ = std::fs::remove_dir_all(&scan_root);
    let _ = std::fs::remove_dir_all(&dest_session_dir);
}

#[test]
fn foreign_import_picker_emits_ui_intent_on_enter() {
    // -- arrange
    let scan_root = unique_temp_dir("intent-scan");
    let foreign_session = scan_root.join("session_bravo");
    std::fs::create_dir_all(&foreign_session).expect("create foreign session dir");
    write_foreign_event_envelope(
        &foreign_session.join("events.jsonl"),
        "evt_foreign_bravo",
        "run_foreign_bravo",
        "bravo session",
    );

    let dest_session_dir = unique_temp_dir("intent-dest");
    let intents: std::sync::Arc<std::sync::Mutex<Vec<UiIntent>>> =
        std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
    let intents_clone = intents.clone();

    let mut app = AppState::new_live(
        Some(dest_session_dir.clone()),
        false,
        Some(std::sync::Arc::new(move |intent: UiIntent| {
            intents_clone.lock().expect("intents lock").push(intent);
        })),
    );

    // -- act: open picker, press enter via public handle_key
    app.open_foreign_import_picker(scan_root.clone());
    assert!(app.foreign_import_picker.visible);

    app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));

    // -- assert: UiIntent::ImportForeignSession was emitted
    let intents_guard = intents.lock().expect("intents lock");
    let import_intents: Vec<_> = intents_guard
        .iter()
        .filter(|intent| matches!(intent, UiIntent::ImportForeignSession { .. }))
        .collect();
    assert_eq!(
        import_intents.len(),
        1,
        "expected exactly one ImportForeignSession intent"
    );
    if let UiIntent::ImportForeignSession {
        source_path,
        dest_session_dir: dest,
    } = &import_intents[0]
    {
        assert!(
            source_path.ends_with("session_bravo"),
            "source path should point to the foreign session: {source_path:?}"
        );
        assert_eq!(dest, &dest_session_dir);
    }

    // -- assert: picker closed after intent emission
    assert!(!app.foreign_import_picker.visible);

    // -- cleanup
    drop(intents_guard);
    let _ = std::fs::remove_dir_all(&scan_root);
    let _ = std::fs::remove_dir_all(&dest_session_dir);
}

#[test]
fn foreign_import_picker_esc_closes_overlay() {
    let scan_root = unique_temp_dir("esc-scan");
    let dest_session_dir = unique_temp_dir("esc-dest");

    let mut app = AppState::new_live(Some(dest_session_dir.clone()), false, None);
    app.open_foreign_import_picker(scan_root.clone());
    assert!(app.foreign_import_picker.visible);

    app.handle_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
    assert!(
        !app.foreign_import_picker.visible,
        "esc should close picker"
    );

    let _ = std::fs::remove_dir_all(&scan_root);
    let _ = std::fs::remove_dir_all(&dest_session_dir);
}

#[test]
fn foreign_import_picker_overlay_appears_in_overlay_stack() {
    let scan_root = unique_temp_dir("overlay-scan");
    let dest_session_dir = unique_temp_dir("overlay-dest");

    let mut app = AppState::new_live(Some(dest_session_dir.clone()), false, None);
    app.open_foreign_import_picker(scan_root.clone());

    let stack = app.overlay_stack();
    let overlays: Vec<_> = stack.ordered().to_vec();
    assert!(
        overlays.contains(&harness_tui::overlay::OverlayKind::ForeignImportPicker),
        "ForeignImportPicker must be in overlay stack: {overlays:?}"
    );

    let _ = std::fs::remove_dir_all(&scan_root);
    let _ = std::fs::remove_dir_all(&dest_session_dir);
}

#[test]
fn import_slash_command_is_registered() {
    // -- assert: /import is registered in slash commands
    let commands = harness_tui::keybindings::slash_commands();
    let import_cmd = commands.iter().find(|cmd| cmd.id == "import");
    assert!(
        import_cmd.is_some(),
        "/import slash command must be registered"
    );

    // -- assert: /import has a description
    let description = harness_tui::keybindings::slash_command_description("import");
    assert!(!description.is_empty(), "/import must have a description");

    // -- assert: /import has an alias
    let aliases = harness_tui::keybindings::slash_command_aliases("import");
    assert!(
        aliases.contains(&"import-session"),
        "/import should have an import-session alias"
    );
}
