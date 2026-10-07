use super::*;
use crate::app::{AppState, UiIntent};
use crate::ui::render_app;
use crate::UnwrapOrAbort;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
use ratatui::{backend::TestBackend, layout::Rect, Terminal};
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
fn login_commands_open_dialog_and_offer_browser_and_headless_sign_in() {
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

    for startup in [true, false] {
        for palette in [true, false] {
            for method in ["browser", "device"] {
                let intents = Arc::new(Mutex::new(Vec::new()));
                let captured = Arc::clone(&intents);
                let sink = Arc::new(move |intent| captured.lock().unwrap_or_abort().push(intent));
                let mut app = if startup {
                    AppState::new_startup(Vec::new(), Some(sink))
                } else {
                    AppState::new_live(None, false, Some(sink))
                };
                app.set_connect_dialog_providers(providers.clone());
                if palette {
                    app.handle_key(KeyEvent::new(KeyCode::Char('p'), KeyModifiers::CONTROL));
                }
                for ch in if palette { "login" } else { "/login" }.chars() {
                    app.handle_key(key(KeyCode::Char(ch)));
                }
                app.handle_key(key(KeyCode::Enter));
                assert!(
                    app.connect_dialog.visible,
                    "login must open the provider dialog"
                );
                assert!(intents.lock().unwrap_or_abort().is_empty());
                for ch in "openai".chars() {
                    app.handle_key(key(KeyCode::Char(ch)));
                }
                app.handle_key(key(KeyCode::Enter));
                if method == "device" {
                    app.handle_key(key(KeyCode::Down));
                }
                app.handle_key(key(KeyCode::Enter));
                assert_eq!(app.connect_dialog.step, ConnectDialogStep::Waiting);
                assert_eq!(
                    intents.lock().unwrap_or_abort().as_slice(),
                    &[UiIntent::OpenAuthManager {
                        args: vec![
                            "login".into(),
                            "openai".into(),
                            "--method".into(),
                            method.into()
                        ],
                        stdin: None,
                    }]
                );
            }
        }
    }
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

    assert!(rendered.contains("Log in to a provider"), "{rendered}");
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

    type_text(&mut app, "界🙂");
    app.handle_connect_dialog_key(key(KeyCode::Left));
    type_text(&mut app, "x");
    assert_eq!(app.connect_dialog.input_buffer, "界x🙂");

    app.handle_connect_dialog_key(key(KeyCode::Home));
    app.handle_connect_dialog_key(key(KeyCode::Delete));
    assert_eq!(app.connect_dialog.input_buffer, "x🙂");

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

fn waiting_device_auth_app() -> AppState {
    let mut app = AppState::new_live(None, false, None);
    app.connect_dialog.visible = true;
    app.connect_dialog.step = ConnectDialogStep::Waiting;
    app.append_connect_dialog_authorization_detail(
        "auth backend output: Open https://auth.example.test/device and enter TEST-CODE",
    );
    app
}

#[test]
fn waiting_device_auth_c_copies_user_code() {
    // arrange
    let copied = Arc::new(Mutex::new(None));
    let captured = Arc::clone(&copied);
    crate::clipboard::set_copy_override(Some(Box::new(move |text| {
        *captured.lock().unwrap_or_abort() = Some(text.to_string());
        Ok(())
    })));
    let mut app = waiting_device_auth_app();

    // act
    app.handle_connect_dialog_key(key(KeyCode::Char('c')));
    crate::clipboard::set_copy_override(None);

    // assert
    assert_eq!(
        copied.lock().unwrap_or_abort().as_deref(),
        Some("TEST-CODE")
    );
    assert_eq!(
        app.connect_dialog
            .toast
            .as_ref()
            .map(|toast| toast.message.as_str()),
        Some("Copied authorization code")
    );
}

#[test]
fn waiting_browser_auth_c_copies_streamed_authorization_url() {
    // arrange
    let copied = Arc::new(Mutex::new(None));
    let captured = Arc::clone(&copied);
    crate::clipboard::set_copy_override(Some(Box::new(move |text| {
        *captured.lock().unwrap_or_abort() = Some(text.to_string());
        Ok(())
    })));
    let mut app = waiting_device_auth_app();
    app.connect_dialog.notice = None;
    app.append_connect_dialog_authorization_detail(
        "auth backend output: Open this URL to sign in:",
    );
    app.handle_connect_dialog_key(key(KeyCode::Char('c')));
    assert!(copied.lock().unwrap_or_abort().is_none());
    app.append_connect_dialog_authorization_detail(
        "auth backend output: https://auth.example.test/oauth/authorize?state=test-state",
    );

    // act
    app.handle_connect_dialog_key(key(KeyCode::Char('c')));
    crate::clipboard::set_copy_override(None);

    // assert
    assert_eq!(
        copied.lock().unwrap_or_abort().as_deref(),
        Some("https://auth.example.test/oauth/authorize?state=test-state")
    );
}

#[test]
fn waiting_device_auth_control_click_is_left_to_the_terminal() {
    // arrange
    let mut app = waiting_device_auth_app();
    let frame = Rect::new(0, 0, 100, 30);
    let mouse = MouseEvent {
        kind: MouseEventKind::Down(MouseButton::Left),
        column: 24,
        row: 9,
        modifiers: KeyModifiers::CONTROL,
    };

    // act
    let handled = app.handle_connect_dialog_mouse(mouse, frame);

    // assert
    assert!(!handled);
    assert_eq!(app.connect_dialog.pointer_down, None);
}

#[test]
fn waiting_device_auth_renders_terminal_hyperlink_for_local_ctrl_click() {
    // arrange
    let app = waiting_device_auth_app();
    let backend = TestBackend::new(100, 30);
    let mut terminal = Terminal::new(backend).unwrap_or_abort();

    // act
    terminal
        .draw(|frame| render_app(frame, &app))
        .unwrap_or_abort();

    // assert
    let buffer = terminal.backend().buffer();
    assert!((0..30).any(|y| {
        (0..100).any(|x| {
            buffer[(x, y)]
                .symbol()
                .contains("\x1b]8;;https://auth.example.test/device")
        })
    }));
}

#[test]
fn waiting_device_auth_drag_copies_painted_code() {
    // arrange
    let copied = Arc::new(Mutex::new(None));
    let captured = Arc::clone(&copied);
    crate::clipboard::set_copy_override(Some(Box::new(move |text| {
        *captured.lock().unwrap_or_abort() = Some(text.to_string());
        Ok(())
    })));
    let mut app = waiting_device_auth_app();
    let frame = Rect::new(0, 0, 100, 30);
    let mouse = |kind| MouseEvent {
        kind,
        column: 24,
        row: 10,
        modifiers: KeyModifiers::NONE,
    };

    // act
    app.handle_connect_dialog_mouse(mouse(MouseEventKind::Down(MouseButton::Left)), frame);
    app.handle_connect_dialog_mouse(mouse(MouseEventKind::Drag(MouseButton::Left)), frame);
    app.handle_connect_dialog_mouse(mouse(MouseEventKind::Up(MouseButton::Left)), frame);
    crate::clipboard::set_copy_override(None);

    // assert
    assert_eq!(
        copied.lock().unwrap_or_abort().as_deref(),
        Some("TEST-CODE")
    );
}

/// The Claude subscription login waits for a pasted code: `c` copies the link only while the
/// field is empty, the field is drawn, an empty Enter is ignored, and Esc cancels the login.
#[test]
fn subscription_login_copies_until_typing_and_escape_cancels() {
    let copies = Arc::new(Mutex::new(Vec::new()));
    let captured = Arc::clone(&copies);
    crate::clipboard::set_copy_override(Some(Box::new(move |text| {
        captured.lock().unwrap_or_abort().push(text.to_string());
        Ok(())
    })));
    let intents = Arc::new(Mutex::new(Vec::new()));
    let sink_intents = Arc::clone(&intents);
    let sink = Arc::new(move |intent| sink_intents.lock().unwrap_or_abort().push(intent));
    let mut app = AppState::new_live(None, false, Some(sink));
    let providers = catalog_providers(
        &ProviderCatalog::from_embedded().unwrap_or_abort(),
        &AuthPluginRegistry::with_builtins(),
    );
    let index = providers
        .iter()
        .position(|provider| provider.id == ProviderId::anthropic_subscription())
        .unwrap_or_abort();
    app.set_connect_dialog_providers(providers);
    app.connect_dialog.selected_provider = Some(index);
    app.connect_dialog.selected_method = Some(1);
    app.connect_dialog.visible = true;
    app.connect_dialog.step = ConnectDialogStep::Waiting;
    let url = "https://claude.ai/oauth/authorize?code=true&state=test-state";
    for line in ["Open this URL to sign in:", url] {
        app.append_connect_dialog_authorization_detail(&format!("auth backend output: {line}"));
    }

    app.handle_connect_dialog_key(key(KeyCode::Char('c')));
    app.handle_connect_dialog_key(key(KeyCode::Enter));
    app.handle_connect_dialog_paste("code-from-anthropic#test-state");
    app.handle_connect_dialog_key(key(KeyCode::Char('c')));
    let screen = render_plain(&app, 120, 40);
    app.handle_connect_dialog_key(key(KeyCode::Enter));
    // After the exchange the login asks which account to save; an empty Enter keeps its default.
    app.append_connect_dialog_authorization_detail(
        "auth backend output: Name for this account (existing: default; press Enter to add account-2)",
    );
    let naming = render_plain(&app, 120, 40);
    app.handle_connect_dialog_key(key(KeyCode::Enter));
    app.handle_connect_dialog_key(key(KeyCode::Esc));
    crate::clipboard::set_copy_override(None);

    assert_eq!(copies.lock().unwrap_or_abort().as_slice(), [url]);
    assert!(
        screen.contains("Code: code-from-anthropic#test-statec"),
        "{screen}"
    );
    assert!(
        naming.contains("Name for this account") && naming.contains("Name: "),
        "{naming}"
    );
    assert_eq!(
        intents.lock().unwrap_or_abort().as_slice(),
        [
            UiIntent::AuthBackendInput {
                line: Some("code-from-anthropic#test-statec".into())
            },
            UiIntent::AuthBackendInput {
                line: Some(String::new())
            },
            UiIntent::AuthBackendInput { line: None },
        ]
    );
    assert!(!app.connect_dialog.visible);
}
