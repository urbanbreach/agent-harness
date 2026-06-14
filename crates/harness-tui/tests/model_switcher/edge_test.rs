use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use harness_tui::app::{AppState, LaunchMetadata, UiIntent};

use crate::model_switcher_fixtures::*;

fn ctrl(code: char) -> KeyEvent {
    KeyEvent::new(KeyCode::Char(code), KeyModifiers::CONTROL)
}

#[allow(clippy::type_complexity)]
fn recent_intent_sink() -> (
    Arc<Mutex<Vec<UiIntent>>>,
    Arc<dyn Fn(UiIntent) + Send + Sync>,
) {
    let intents = Arc::new(Mutex::new(Vec::<UiIntent>::new()));
    let sink = {
        let intents = Arc::clone(&intents);
        Arc::new(move |intent: UiIntent| {
            intents.lock().expect("lock intents").push(intent);
        })
    };
    (intents, sink)
}

#[test]
fn f2_recent_cycle_noops_with_zero_or_one_recent_model() {
    let (empty_intents, empty_sink) = recent_intent_sink();
    let mut empty = AppState::new_live(None, false, Some(empty_sink));
    empty.set_launch_metadata(
        LaunchMetadata::from_model_option(&multi_provider_models()[1])
            .with_available_models(multi_provider_models()),
    );

    empty.handle_key(key(KeyCode::F(2)));
    empty.handle_key(KeyEvent::new(KeyCode::F(2), KeyModifiers::SHIFT));

    assert!(empty_intents.lock().expect("lock empty intents").is_empty());
    assert!(!empty.overlay_state.model_switcher_visible);

    let root = tempfile::tempdir().expect("temp model state root");
    let run_dir = root.path().join("run_current");
    std::fs::create_dir_all(&run_dir).expect("run dir");
    let (single_intents, single_sink) = recent_intent_sink();
    let mut single = AppState::new_live(Some(run_dir), false, Some(single_sink));
    single.set_launch_metadata(
        LaunchMetadata::from_model_option(&multi_provider_models()[1])
            .with_available_models(multi_provider_models()),
    );
    for ch in "/model".chars() {
        single.handle_key(key(KeyCode::Char(ch)));
    }
    single.handle_key(key(KeyCode::Enter));
    single.handle_key(key(KeyCode::Enter));
    single_intents.lock().expect("lock single intents").clear();

    single.handle_key(key(KeyCode::F(2)));
    single.handle_key(KeyEvent::new(KeyCode::F(2), KeyModifiers::SHIFT));

    assert!(single_intents
        .lock()
        .expect("lock single intents")
        .is_empty());
    assert!(!single.overlay_state.model_switcher_visible);
}

#[test]
fn replay_and_startup_block_model_mutation_dialogs() {
    let mut replay = AppState::new_replay(PathBuf::from("/tmp/replay-model-edge"), Vec::new());
    replay.set_launch_metadata(
        LaunchMetadata::from_model_option(&multi_provider_models()[1])
            .with_available_models(multi_provider_models()),
    );

    replay.handle_key(ctrl('v'));
    replay.handle_key(ctrl('x'));
    replay.handle_key(key(KeyCode::Char('a')));
    replay.handle_key(ctrl('p'));

    assert!(!replay.overlay_state.model_switcher_visible);
    assert!(!replay
        .palette_filtered
        .contains(&"variant_list".to_string()));
    assert!(!replay.palette_filtered.contains(&"agent_list".to_string()));

    let mut startup = AppState::new_startup(Vec::new(), None);
    startup.set_launch_metadata(
        LaunchMetadata::from_model_option(&multi_provider_models()[1])
            .with_available_models(multi_provider_models()),
    );

    startup.handle_key(ctrl('p'));

    assert!(!startup
        .palette_filtered
        .contains(&"variant_list".to_string()));
    assert!(!startup.palette_filtered.contains(&"agent_list".to_string()));
}
