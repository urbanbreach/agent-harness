use super::*;
use crate::app::{AppState, UiIntent};
use crate::ui::render_app;
use crate::UnwrapOrAbort;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::{backend::TestBackend, Terminal};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

fn api_key_provider(id: &str, label: &str) -> ConnectProviderOption {
    ConnectProviderOption {
        id: ProviderId::parse(id).unwrap_or_abort(),
        label: label.to_string(),
        description: "API key".to_string(),
        methods: vec![AuthMethodSpec::ApiKey {
            label: "API key".to_string(),
        }],
        models: Vec::new(),
    }
}

fn key(code: KeyCode) -> KeyEvent {
    KeyEvent::new(code, KeyModifiers::NONE)
}

fn type_text(app: &mut AppState, value: &str) {
    for c in value.chars() {
        app.handle_connect_dialog_key(key(KeyCode::Char(c)));
    }
}

fn render_plain(app: &AppState, width: u16, height: u16) -> String {
    let backend = TestBackend::new(width, height);
    let mut terminal = Terminal::new(backend).unwrap_or_abort();
    terminal
        .draw(|frame| render_app(frame, app))
        .unwrap_or_abort();
    let buffer = terminal.backend().buffer();
    (0..height)
        .map(|y| {
            (0..width)
                .map(|x| buffer[(x, y)].symbol())
                .collect::<String>()
        })
        .collect::<Vec<_>>()
        .join("\n")
}

#[test]
fn catalog_providers_include_models_dev_api_key_entries() {
    let catalog = ProviderCatalog::from_embedded().unwrap_or_abort();
    let registry = AuthPluginRegistry::with_builtins();

    let providers = catalog_providers(&catalog, &registry);

    assert!(providers.len() > registry.providers().len());
    let anthropic = providers
        .iter()
        .find(|provider| provider.id.as_str() == "anthropic")
        .unwrap_or_abort();
    assert_eq!(anthropic.label, "Anthropic");
    assert!(matches!(
        anthropic.methods.as_slice(),
        [AuthMethodSpec::ApiKey { .. }]
    ));
    assert!(!anthropic.models.is_empty());
}

#[test]
fn catalog_providers_overlay_openai_auth_methods() {
    let catalog = ProviderCatalog::from_embedded().unwrap_or_abort();
    let registry = AuthPluginRegistry::with_builtins();

    let providers = catalog_providers(&catalog, &registry);

    let openai = providers
        .iter()
        .find(|provider| provider.id.as_str() == "openai")
        .unwrap_or_abort();
    assert_eq!(openai.label, "OpenAI");
    assert!(openai
        .methods
        .iter()
        .any(|method| matches!(method, AuthMethodSpec::OAuthAuto { .. })));
    assert!(openai
        .methods
        .iter()
        .any(|method| matches!(method, AuthMethodSpec::ApiKey { .. })));
}

#[test]
fn connect_dialog_renders_provider_panel() {
    let mut app = AppState::new_live(None, false, None);
    app.set_connect_dialog_providers(vec![
        api_key_provider("codex", "Codex"),
        api_key_provider("deepseek", "DeepSeek"),
    ]);
    app.open_connect_dialog();

    let rendered = render_plain(&app, 100, 30);

    assert!(rendered.contains("Connect a provider"), "{rendered}");
    assert!(rendered.contains("esc"), "{rendered}");
    assert!(rendered.contains("Search"), "{rendered}");
    assert!(rendered.contains("Popular"), "{rendered}");
    assert!(rendered.contains("Providers"), "{rendered}");
    assert!(rendered.contains("Other Custom provider"), "{rendered}");
    assert!(
        !rendered.contains('┌'),
        "old bordered modal chrome should be gone: {rendered}"
    );
    assert!(
        !rendered.contains("↑↓/jk"),
        "old key-hint footer should be gone: {rendered}"
    );
}

#[test]
fn connect_dialog_renders_when_terminal_is_narrow() {
    let mut app = AppState::new_live(None, false, None);
    app.set_connect_dialog_providers(vec![api_key_provider("codex", "Codex")]);
    app.open_connect_dialog();

    let rendered = render_plain(&app, 8, 8);

    assert!(!rendered.is_empty());
}

#[test]
fn filtered_provider_enter_selects_filtered_provider() {
    let mut app = AppState::new_live(Some(PathBuf::from("/tmp/session")), false, None);
    app.set_connect_dialog_providers(vec![
        api_key_provider("codex", "Codex"),
        api_key_provider("github-copilot", "GitHub Copilot"),
    ]);
    app.open_connect_dialog();

    type_text(&mut app, "git");
    app.handle_connect_dialog_key(key(KeyCode::Enter));

    assert_eq!(app.connect_dialog.selected_provider, Some(1));
    assert_eq!(app.connect_dialog.step, ConnectDialogStep::ApiKeyInput);
}

#[test]
fn end_key_moves_to_other_provider_row() {
    let mut app = AppState::new_live(Some(PathBuf::from("/tmp/session")), false, None);
    app.set_connect_dialog_providers(vec![api_key_provider("codex", "Codex")]);
    app.open_connect_dialog();

    app.handle_connect_dialog_key(key(KeyCode::End));
    app.handle_connect_dialog_key(key(KeyCode::Enter));

    assert_eq!(app.connect_dialog.step, ConnectDialogStep::CustomProviderId);
}

#[test]
fn prompt_input_supports_cursor_navigation_and_delete() {
    let mut app = AppState::new_live(Some(PathBuf::from("/tmp/session")), false, None);
    app.open_connect_dialog();

    app.handle_connect_dialog_key(key(KeyCode::Enter));
    assert_eq!(app.connect_dialog.step, ConnectDialogStep::CustomProviderId);

    type_text(&mut app, "ab");
    app.handle_connect_dialog_key(key(KeyCode::Left));
    type_text(&mut app, "x");
    assert_eq!(app.connect_dialog.input_buffer, "axb");

    app.handle_connect_dialog_key(key(KeyCode::Home));
    app.handle_connect_dialog_key(key(KeyCode::Delete));
    assert_eq!(app.connect_dialog.input_buffer, "xb");

    app.handle_connect_dialog_key(key(KeyCode::End));
    app.handle_connect_dialog_key(key(KeyCode::Left));
    app.handle_connect_dialog_key(key(KeyCode::Right));
    app.handle_connect_dialog_key(key(KeyCode::Backspace));
    assert_eq!(app.connect_dialog.input_buffer, "x");

    app.handle_connect_dialog_key(key(KeyCode::Enter));
    assert_eq!(app.connect_dialog.step, ConnectDialogStep::ApiKeyInput);
    assert_eq!(
        app.connect_dialog
            .custom_provider
            .as_ref()
            .map(ProviderId::as_str),
        Some("x")
    );
}

#[test]
fn other_provider_api_key_emits_generic_auth_login() {
    let intents = Arc::new(Mutex::new(Vec::new()));
    let sink = {
        let intents = Arc::clone(&intents);
        Arc::new(move |intent: UiIntent| {
            intents.lock().unwrap_or_abort().push(intent);
        })
    };
    let mut app = AppState::new_live(Some(PathBuf::from("/tmp/session")), false, Some(sink));
    app.open_connect_dialog();

    app.handle_connect_dialog_key(key(KeyCode::Enter));
    assert_eq!(app.connect_dialog.step, ConnectDialogStep::CustomProviderId);

    type_text(&mut app, "my-provider");
    app.handle_connect_dialog_key(key(KeyCode::Enter));
    assert_eq!(app.connect_dialog.step, ConnectDialogStep::ApiKeyInput);

    type_text(&mut app, "secret-key");
    app.handle_connect_dialog_key(key(KeyCode::Enter));

    assert_eq!(app.connect_dialog.step, ConnectDialogStep::Waiting);
    assert_eq!(
        intents.lock().unwrap_or_abort().as_slice(),
        &[UiIntent::OpenAuthManager {
            args: vec![
                "login".to_string(),
                "my-provider".to_string(),
                "--method".to_string(),
                "api-key".to_string(),
                "--api-key-stdin".to_string(),
            ],
            stdin: Some("secret-key".to_string()),
        }]
    );
}
