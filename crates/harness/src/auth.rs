use crate::{CliDeps, CliIo};
use clap::{Args, Subcommand};
use harness_core::{
    auth::{CredentialClock, CredentialStore, ProviderId, StoredCredential, SystemCredentialClock},
    config::load_resolved_config_with_lookup,
};
use std::{
    collections::BTreeSet,
    ffi::OsString,
    io::{BufRead, Read},
    path::{Path, PathBuf},
};
mod claude_account;
pub use claude_account::*;
mod oauth;

#[derive(Args)]
pub(crate) struct AuthCommand {
    #[command(subcommand)]
    command: AuthAction,
}
#[derive(Subcommand)]
enum AuthAction {
    List {
        #[arg(long)]
        json: bool,
    },
    Login(Login),
    Logout {
        provider: String,
    },
    /// Anthropic Subscription accounts: list, add, remove, pin, unpin, rename, clear-name.
    #[command(name = "claude-account")]
    ClaudeAccount {
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        args: Vec<String>,
    },
}
#[derive(Args)]
struct Login {
    provider: Option<String>,
    #[arg(long = "provider", short = 'p', conflicts_with = "provider")]
    provider_option: Option<String>,
    #[arg(long, short = 'm', value_parser = method)]
    method: Option<Method>,
    #[arg(long)]
    api_key_stdin: bool,
    #[arg(long, hide = true)]
    mock_token: Option<String>,
    #[arg(long, hide = true, requires = "mock_token")]
    mock_refresh_token: Option<String>,
    #[arg(long, hide = true, requires = "mock_token")]
    expires_at: Option<String>,
    #[arg(long, hide = true, requires = "mock_token")]
    account_id: Option<String>,
    #[arg(long)]
    enterprise_url: Option<String>,
}
#[derive(Clone, Copy)]
enum Method {
    ApiKey,
    Browser,
    Device,
}
fn method(value: &str) -> Result<Method, String> {
    match value.trim().to_ascii_lowercase().as_str() {
        "api-key" | "api_key" | "api" | "manually enter api key" => Ok(Method::ApiKey),
        "device" | "headless" | "chatgpt pro/plus (headless)" | "login with github copilot" => {
            Ok(Method::Device)
        }
        "browser" | "chatgpt pro/plus (browser)" | "browser login (default)" => Ok(Method::Browser),
        "copy-code" | "copy_code" | "copy code login (headless)" => Ok(Method::Device),
        _ => Err("expected browser, device, or api-key".into()),
    }
}
pub struct AuthBackendOutput {
    pub code: i32,
    pub stdout: String,
    pub stderr: String,
}
pub fn execute_auth_backend_args(
    args: &[String],
    config: Option<PathBuf>,
    session_dir: Option<PathBuf>,
    stdin: &str,
    deps: &CliDeps,
) -> AuthBackendOutput {
    let (mut input, mut stdout, mut stderr) = (
        std::io::Cursor::new(stdin.as_bytes()),
        Vec::new(),
        Vec::new(),
    );
    let code = execute_auth_backend_args_with_io(
        args,
        config,
        session_dir,
        &mut CliIo::new(&mut input, &mut stdout, &mut stderr),
        deps,
    );
    AuthBackendOutput {
        code,
        stdout: String::from_utf8_lossy(&stdout).into(),
        stderr: String::from_utf8_lossy(&stderr).into(),
    }
}
pub fn execute_auth_backend_args_with_io(
    args: &[String],
    config: Option<PathBuf>,
    session_dir: Option<PathBuf>,
    io: &mut CliIo<'_>,
    deps: &CliDeps,
) -> i32 {
    let mut argv = vec![OsString::from("harness")];
    for (name, value) in [("--config", config), ("--session-dir", session_dir)] {
        if let Some(value) = value {
            argv.extend([name.into(), value.into_os_string()]);
        }
    }
    argv.push("auth".into());
    argv.extend(args.iter().map(OsString::from));
    crate::run(argv, io, deps.clone()).code
}
pub(crate) fn execute(
    command: AuthCommand,
    config_path: Option<&Path>,
    io: &mut CliIo<'_>,
    deps: &CliDeps,
) -> Result<(), String> {
    let config = config_path;
    let store = CredentialStore::from_lookup(&|name| deps.env_var_value(name))
        .ok_or("set HARNESS_DATA_HOME, XDG_DATA_HOME, or HOME for credential storage")?;
    let config = load_resolved_config_with_lookup(config, &deps.config_load_context(), &|name| {
        deps.env_var_value(name)
    })
    .map_err(|e| e.to_string())?
    .map(|loaded| loaded.config);
    let resolve = |name: &str| {
        if let Some(provider) = config
            .as_ref()
            .and_then(|config| config.providers.get(name))
        {
            return provider
                .credential_provider(name)
                .ok_or_else(|| "invalid auth provider ID".to_owned());
        }
        ProviderId::parse(if name == "openai-codex" {
            "codex"
        } else {
            name
        })
        .ok_or_else(|| "invalid auth provider ID".to_owned())
    };
    match command.command {
        AuthAction::ClaudeAccount { args } => {
            let settings = config
                .as_ref()
                .and_then(|config| {
                    config.providers.values().find_map(|p| match p {
                        harness_core::config::ProviderConfig::AnthropicSubscription(p) => {
                            Some(p.settings())
                        }
                        _ => None,
                    })
                })
                .unwrap_or_default()
                .with_env(&|name| deps.env_var_value(name));
            if args.first().map(String::as_str) == Some("add") {
                return execute(
                    AuthCommand {
                        command: AuthAction::Login(Login {
                            provider: Some("anthropic-subscription".into()),
                            provider_option: None,
                            method: None,
                            api_key_stdin: false,
                            mock_token: None,
                            mock_refresh_token: None,
                            expires_at: None,
                            account_id: None,
                            enterprise_url: None,
                        }),
                    },
                    config_path,
                    io,
                    deps,
                );
            }
            let message = claude_account(&args, &store, &settings, deps)?;
            writeln!(io.stdout, "{message}").map_err(|e| e.to_string())?;
        }
        AuthAction::Logout { provider } => {
            let id = resolve(&provider)?;
            let removed = store.delete(&id).map_err(|e| e.to_string())?;
            writeln!(
                io.stdout,
                "{id}: {}",
                if removed {
                    "stored credential removed"
                } else {
                    "no stored credential"
                }
            )
            .map_err(|e| e.to_string())?;
        }
        AuthAction::List { json } => {
            let mut ids = BTreeSet::from([
                ProviderId::codex(),
                ProviderId::github_copilot(),
                ProviderId::anthropic_subscription(),
            ]);
            for name in config.iter().flat_map(|config| config.providers.keys()) {
                ids.insert(resolve(name)?);
            }
            ids.extend(store.stored_provider_ids().map_err(|e| e.to_string())?);
            let mut rows = Vec::new();
            for id in ids {
                let credential = store.load(&id).map_err(|e| e.to_string())?;
                let presence = if credential.is_some() {
                    "stored"
                } else {
                    "missing"
                };
                let row = serde_json::json!({"provider": id, "presence": presence, "kind":credential.as_ref().map(|c| c.kind), "expires_at":credential.as_ref().and_then(|c| c.expires_at.as_deref())});
                if !json {
                    writeln!(io.stdout, "{id}: presence={presence}").map_err(|e| e.to_string())?;
                }
                rows.push(row);
            }
            if json {
                serde_json::to_writer_pretty(&mut io.stdout, &rows).map_err(|e| e.to_string())?;
                writeln!(io.stdout).map_err(|e| e.to_string())?;
            }
        }
        AuthAction::Login(mut login) => {
            let selected = login.provider_option.take().or(login.provider.take());
            let name = match selected {
                Some(name) => name,
                None if io.stdin_is_terminal => {
                    writeln!(
                        io.stderr,
                        "Provider ID (codex, github-copilot, or an API provider):"
                    )
                    .map_err(|e| e.to_string())?;
                    let mut value = String::new();
                    io.stdin
                        .take(257)
                        .read_line(&mut value)
                        .map_err(|e| e.to_string())?;
                    value.trim().into()
                }
                None => return Err("choose an auth provider: harness auth login <provider>".into()),
            };
            let id = resolve(&name)?;
            let method = login.method.unwrap_or(if login.api_key_stdin {
                Method::ApiKey
            } else if id == ProviderId::codex() {
                Method::Browser
            } else if id == ProviderId::github_copilot() {
                Method::Device
            } else if id == ProviderId::anthropic_subscription() {
                Method::Browser
            } else {
                Method::ApiKey
            });
            // The OpenAI catalog offers ChatGPT OAuth, whose credentials belong to Codex.
            let id =
                if id.as_str() == "openai" && matches!(method, Method::Browser | Method::Device) {
                    ProviderId::codex()
                } else {
                    id
                };
            if let Some(token) = login.mock_token {
                let refresh = login.mock_refresh_token.unwrap_or_else(|| token.clone());
                let mut credential = StoredCredential::oauth(
                    id.clone(),
                    token,
                    refresh,
                    login.expires_at,
                    SystemCredentialClock.now_rfc3339(),
                );
                credential.account_id = login.account_id;
                store.save(&credential).map_err(|e| e.to_string())?;
                writeln!(io.stdout, "stored oauth credential for {id}")
                    .map_err(|e| e.to_string())?;
                return Ok(());
            }
            if matches!(method, Method::ApiKey) {
                let token = if login.api_key_stdin {
                    let mut token = String::new();
                    io.stdin
                        .take(65_537)
                        .read_to_string(&mut token)
                        .map_err(|e| e.to_string())?;
                    token.trim().to_owned()
                } else if io.stdin_is_terminal {
                    hidden_key(io)?
                } else {
                    return Err(
                        "use --api-key-stdin to supply an API key without a terminal".into(),
                    );
                };
                if token.is_empty() || token.len() > 65_536 {
                    return Err("API key must contain 1 to 65,536 bytes".into());
                }
                store
                    .save(&StoredCredential::api_key(
                        id.clone(),
                        token,
                        SystemCredentialClock.now_rfc3339(),
                    ))
                    .map_err(|e| e.to_string())?;
                writeln!(io.stdout, "stored api_key credential for {id}")
                    .map_err(|e| e.to_string())?;
            } else {
                let lines = if id == ProviderId::anthropic_subscription() {
                    oauth::input_lines(io, deps)
                } else {
                    tokio::sync::mpsc::unbounded_channel().1
                };
                let callback_host = deps
                    .env_var_value("HARNESS_OAUTH_CALLBACK_HOST")
                    .filter(|host| !host.is_empty())
                    .unwrap_or_else(|| "127.0.0.1".into());
                crate::cli_io::with_output_worker(io, |io| {
                    let runtime = tokio::runtime::Builder::new_current_thread()
                        .enable_all()
                        .build()
                        .map_err(|e| e.to_string())?;
                    runtime.block_on(oauth::login(
                        &id,
                        method,
                        login.enterprise_url.as_deref(),
                        &store,
                        io,
                        oauth::LoginInput {
                            lines,
                            callback_host,
                            environment: deps.environment_snapshot(),
                            closed_cancels: deps.interactive_input().is_some(),
                        },
                    ))
                })?;
                writeln!(io.stdout, "stored oauth credential for {id}")
                    .map_err(|e| e.to_string())?;
            }
        }
    }
    io.stdout.flush().map_err(|e| e.to_string())
}

fn hidden_key(io: &mut CliIo<'_>) -> Result<String, String> {
    use crossterm::{
        event::{Event, KeyCode, KeyEventKind, KeyModifiers},
        terminal,
    };
    write!(io.stderr, "API key: ")
        .and_then(|()| io.stderr.flush())
        .map_err(|e| e.to_string())?;
    terminal::enable_raw_mode().map_err(|e| e.to_string())?;
    struct Restore;
    impl Drop for Restore {
        fn drop(&mut self) {
            let _ = terminal::disable_raw_mode();
        }
    }
    let _restore = Restore;
    let mut token = String::new();
    loop {
        if let Event::Key(key) = crossterm::event::read().map_err(|e| e.to_string())? {
            if key.kind != KeyEventKind::Press {
                continue;
            }
            if key.modifiers.contains(KeyModifiers::CONTROL)
                && matches!(key.code, KeyCode::Char('c' | 'd'))
            {
                return Err("login cancelled".into());
            }
            match key.code {
                KeyCode::Enter => break,
                KeyCode::Esc => return Err("login cancelled".into()),
                KeyCode::Backspace => {
                    token.pop();
                }
                KeyCode::Char(c) if !c.is_control() && token.len() < 65_536 => {
                    token.push(c);
                }
                _ => {}
            }
        }
    }
    drop(_restore);
    writeln!(io.stderr).map_err(|e| e.to_string())?;
    Ok(token)
}
