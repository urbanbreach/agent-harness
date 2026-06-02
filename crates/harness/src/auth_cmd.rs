use std::collections::{BTreeMap, BTreeSet};
use std::io::{self, IsTerminal, Write};
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use clap::{Args, Parser, Subcommand};
use crossterm::event::{Event, KeyCode, KeyEventKind, KeyModifiers};
use harness_core::auth::codex::{
    codex_callback_error_html, codex_callback_success_html, pkce_challenge,
    AuthHttpClient as CodexAuthHttpClient, AuthHttpRequest, AuthHttpResponse, CodexDevicePoll,
    CodexLoopbackSession, CodexOAuthClient, CodexOAuthError, PkceCodes, CODEX_ISSUER,
    CODEX_OAUTH_PORT,
};
use harness_core::auth::copilot::{
    CopilotAuthHttpClient, CopilotDeployment, CopilotDevicePoll, CopilotOAuthClient,
    CopilotOAuthError, COPILOT_SCOPE,
};
use harness_core::auth::{
    AuthProviderId, CredentialClock, CredentialStore, CredentialStoreError, StoredCredential,
    StoredCredentialKind, SystemCredentialClock,
};
use harness_core::config::{load_resolved_config_with_context, HarnessConfig, ProviderConfig};
use serde::Serialize;

use crate::{CliDeps, CliIo};

const DEFAULT_DEVICE_POLLS: usize = 120;
const CODEX_BROWSER_TIMEOUT: Duration = Duration::from_secs(5 * 60);
const MANUAL_CALLBACK_POLL_INTERVAL: Duration = Duration::from_millis(50);

#[derive(Debug, Args, Clone)]
pub(crate) struct AuthCommand {
    #[command(subcommand)]
    command: AuthSubcommand,
}

#[derive(Debug, Subcommand, Clone)]
enum AuthSubcommand {
    /// List configured auth providers and redacted credential status.
    List(AuthListCommand),
    /// Store or refresh credentials for an auth provider.
    Login(AuthLoginCommand),
    /// Remove stored credentials for an auth provider without editing config or env.
    Logout(AuthLogoutCommand),
}

#[derive(Debug, Args, Clone, Default)]
struct AuthListCommand {
    /// Emit machine-readable JSON.
    #[arg(long, default_value_t = false)]
    json: bool,
}

#[derive(Debug, Args, Clone)]
struct AuthLoginCommand {
    /// Built-in auth provider id: codex or github-copilot.
    provider: Option<String>,

    /// Provider id or name to log in to (skips provider selection).
    #[arg(long = "provider", short = 'p', conflicts_with = "provider")]
    provider_option: Option<String>,

    /// Login method. Accepts device/browser/api-key or the supported method label.
    #[arg(long, short = 'm', value_parser = parse_login_method_arg)]
    method: Option<AuthLoginMethod>,

    /// Read one API key from stdin and store it in the secure credential store.
    #[arg(long, default_value_t = false)]
    api_key_stdin: bool,

    /// Deterministic test hook: store this OAuth access token without network.
    #[arg(long, hide = true)]
    mock_token: Option<String>,

    /// Deterministic test hook: refresh token paired with --mock-token.
    #[arg(long, hide = true)]
    mock_refresh_token: Option<String>,

    /// RFC3339 expiry to store with --mock-token.
    #[arg(long, hide = true)]
    expires_at: Option<String>,

    /// Redacted account id metadata to store with --mock-token.
    #[arg(long, hide = true)]
    account_id: Option<String>,

    /// GitHub Enterprise URL/domain for github-copilot device login.
    #[arg(long)]
    enterprise_url: Option<String>,
}

#[derive(Debug, Args, Clone)]
struct AuthLogoutCommand {
    /// Built-in auth provider id: codex or github-copilot.
    provider: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AuthLoginMethod {
    Device,
    Browser,
    ApiKey,
}

fn parse_login_method_arg(value: &str) -> Result<AuthLoginMethod, String> {
    match value.trim().to_ascii_lowercase().as_str() {
        "browser" | "chatgpt pro/plus (browser)" => Ok(AuthLoginMethod::Browser),
        "device" | "headless" | "chatgpt pro/plus (headless)" | "login with github copilot" => {
            Ok(AuthLoginMethod::Device)
        }
        "api-key" | "api_key" | "api" | "manually enter api key" => Ok(AuthLoginMethod::ApiKey),
        _ => Err(
            "expected device, browser, api-key, or one of the supported supported method labels"
                .to_string(),
        ),
    }
}

impl AuthLoginCommand {
    fn provider_arg(&self) -> Option<&str> {
        self.provider_option.as_deref().or(self.provider.as_deref())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AuthLoginUi {
    Plain,
    Interactive,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CopilotDeploymentChoice {
    Public,
    Enterprise,
}

#[derive(Debug, Clone, Copy)]
struct AuthPromptOption<T> {
    label: &'static str,
    value: T,
    hint: Option<&'static str>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AuthPromptKey {
    Up,
    Down,
    Submit,
    Cancel,
    Backspace,
    Char(char),
    Ignored,
}

struct RawModeGuard {
    enabled: bool,
}

impl RawModeGuard {
    fn new(enabled: bool) -> Self {
        let enabled = enabled && crossterm::terminal::enable_raw_mode().is_ok();
        Self { enabled }
    }
}

impl Drop for RawModeGuard {
    fn drop(&mut self) {
        if self.enabled {
            let _ = crossterm::terminal::disable_raw_mode();
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub(crate) struct ProviderAuthStatus {
    pub auth_provider: String,
    pub provider_ids: Vec<String>,
    pub source: String,
    pub presence: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub kind: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub expires_at: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub account_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub enterprise_url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    pub env_fallback_configured: bool,
    pub inline_fallback_configured: bool,
    pub usable_without_network_probe: bool,
}

#[derive(Debug, Clone, Default)]
struct AuthProviderFallbacks {
    provider_ids: BTreeSet<String>,
    api_key_env: BTreeSet<String>,
    inline_configured: bool,
}

#[derive(Debug, Parser)]
struct AuthBackendCli {
    #[command(subcommand)]
    command: AuthSubcommand,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AuthBackendOutput {
    pub code: i32,
    pub stdout: String,
    pub stderr: String,
}

pub(crate) fn execute_with_io(
    command: AuthCommand,
    config_path: Option<PathBuf>,
    session_dir: Option<PathBuf>,
    io: &mut CliIo<'_>,
    deps: &CliDeps,
) -> i32 {
    match command.command {
        AuthSubcommand::List(command) => execute_list(command, config_path, session_dir, io, deps),
        AuthSubcommand::Login(command) => {
            execute_login(command, config_path, session_dir, io, deps)
        }
        AuthSubcommand::Logout(command) => {
            execute_logout(command, config_path, session_dir, io, deps)
        }
    }
}

pub(crate) fn execute_backend_args(
    args: &[String],
    config_path: Option<PathBuf>,
    session_dir: Option<PathBuf>,
    stdin: &str,
    deps: &CliDeps,
) -> AuthBackendOutput {
    let mut stdin = std::io::Cursor::new(stdin.as_bytes().to_vec());
    let mut stdout = Vec::new();
    let mut stderr = Vec::new();
    let mut io = CliIo::new(&mut stdin, &mut stdout, &mut stderr);
    let code = execute_backend_args_with_io(args, config_path, session_dir, &mut io, deps);

    AuthBackendOutput {
        code,
        stdout: String::from_utf8_lossy(&stdout).into_owned(),
        stderr: String::from_utf8_lossy(&stderr).into_owned(),
    }
}

pub(crate) fn execute_backend_args_with_io(
    args: &[String],
    config_path: Option<PathBuf>,
    session_dir: Option<PathBuf>,
    io: &mut CliIo<'_>,
    deps: &CliDeps,
) -> i32 {
    let command = match AuthBackendCli::try_parse_from(
        std::iter::once("auth").chain(args.iter().map(String::as_str)),
    ) {
        Ok(parsed) => AuthCommand {
            command: parsed.command,
        },
        Err(err) => {
            let _ = write!(io.stderr, "{err}");
            return 2;
        }
    };

    execute_with_io(command, config_path, session_dir, io, deps)
}

pub(crate) fn auth_statuses(
    config: Option<&HarnessConfig>,
    env_var_is_set: &dyn Fn(&str) -> bool,
    credential_store: Option<&CredentialStore>,
) -> Vec<ProviderAuthStatus> {
    let fallback_map = configured_auth_provider_fallbacks(config);
    AuthProviderId::ALL
        .into_iter()
        .map(|auth_provider| {
            auth_status(
                auth_provider,
                fallback_map.get(&auth_provider),
                env_var_is_set,
                credential_store,
            )
        })
        .collect()
}

pub(crate) fn onboarding_required_for_config(
    config: Option<&HarnessConfig>,
    env_var_is_set: &dyn Fn(&str) -> bool,
    credential_store: Option<&CredentialStore>,
) -> bool {
    let Some(config) = config else {
        return false;
    };
    let fallback_map = configured_auth_provider_fallbacks(Some(config));
    fallback_map.iter().any(|(provider, fallbacks)| {
        let status = auth_status(*provider, Some(fallbacks), env_var_is_set, credential_store);
        !status.usable_without_network_probe
    })
}

fn execute_list(
    command: AuthListCommand,
    config_path: Option<PathBuf>,
    session_dir: Option<PathBuf>,
    io: &mut CliIo<'_>,
    deps: &CliDeps,
) -> i32 {
    let config = load_optional_config(config_path, session_dir, io.stderr, deps);
    let store = credential_store_from_deps(deps);
    let statuses = auth_statuses(
        config.as_ref(),
        &|name| deps.env_var_is_set(name),
        store.as_ref(),
    );

    if command.json {
        match serde_json::to_string_pretty(&statuses) {
            Ok(body) => {
                let _ = writeln!(io.stdout, "{body}");
                0
            }
            Err(err) => {
                let _ = writeln!(io.stderr, "auth list failed to render JSON: {err}");
                1
            }
        }
    } else {
        for status in statuses {
            let kind = status.kind.as_deref().unwrap_or("none");
            let expires = status.expires_at.as_deref().unwrap_or("n/a");
            let account = status.account_id.as_deref().unwrap_or("n/a");
            let enterprise = status.enterprise_url.as_deref().unwrap_or("n/a");
            let providers = if status.provider_ids.is_empty() {
                "unconfigured".to_string()
            } else {
                status.provider_ids.join(",")
            };
            let _ = writeln!(
                io.stdout,
                "{} providers={} presence={} source={} kind={} expires={} account={} enterprise={}",
                status.auth_provider,
                providers,
                status.presence,
                status.source,
                kind,
                expires,
                account,
                enterprise
            );
        }
        0
    }
}

fn execute_logout(
    command: AuthLogoutCommand,
    config_path: Option<PathBuf>,
    session_dir: Option<PathBuf>,
    io: &mut CliIo<'_>,
    deps: &CliDeps,
) -> i32 {
    let config = load_optional_config(config_path, session_dir, io.stderr, deps);
    let Some(store) = credential_store_or_error(io.stderr, deps) else {
        return 2;
    };
    let Some(auth_provider) =
        resolve_provider_arg(command.provider.as_deref(), config.as_ref(), io.stderr)
    else {
        return 2;
    };

    match store.delete(auth_provider) {
        Ok(true) => {
            let _ = writeln!(
                io.stdout,
                "removed stored credential for {} (config/env fallbacks unchanged)",
                auth_provider
            );
            0
        }
        Ok(false) => {
            let _ = writeln!(
                io.stdout,
                "no stored credential for {} (config/env fallbacks unchanged)",
                auth_provider
            );
            0
        }
        Err(err) => {
            let _ = writeln!(io.stderr, "auth logout failed: {err}");
            1
        }
    }
}

fn execute_login(
    command: AuthLoginCommand,
    _config_path: Option<PathBuf>,
    _session_dir: Option<PathBuf>,
    io: &mut CliIo<'_>,
    deps: &CliDeps,
) -> i32 {
    let Some(store) = credential_store_or_error(io.stderr, deps) else {
        return 2;
    };
    let Some(provider_input) = command.provider_arg() else {
        return execute_interactive_login(command, io, &store);
    };
    let Some(auth_provider) = resolve_login_provider_arg(provider_input, io.stderr) else {
        return 2;
    };
    let method = command.method.unwrap_or(AuthLoginMethod::Device);

    execute_login_selection(
        auth_provider,
        method,
        command.enterprise_url.clone(),
        command,
        io,
        &store,
        AuthLoginUi::Plain,
    )
}

fn execute_login_selection(
    auth_provider: AuthProviderId,
    method: AuthLoginMethod,
    enterprise_url: Option<String>,
    command: AuthLoginCommand,
    io: &mut CliIo<'_>,
    store: &CredentialStore,
    ui: AuthLoginUi,
) -> i32 {
    if method == AuthLoginMethod::ApiKey {
        if ui == AuthLoginUi::Interactive {
            return store_interactive_api_key_login(auth_provider, io, store);
        }
        return store_api_key_login(auth_provider, command, io, store);
    }

    if auth_provider == AuthProviderId::GithubCopilot && method != AuthLoginMethod::Device {
        let _ = writeln!(
            io.stderr,
            "auth login failed: github-copilot supports only device login in V1"
        );
        return 2;
    }

    if let Some(token) = command.mock_token.clone() {
        return store_mock_oauth_login(auth_provider, command, &token, io, store, ui);
    }

    match method {
        AuthLoginMethod::Device => run_device_login(auth_provider, enterprise_url, io, store, ui),
        AuthLoginMethod::Browser => run_codex_browser_login(auth_provider, io, store, ui),
        AuthLoginMethod::ApiKey => unreachable!("api-key handled above"),
    }
}

fn execute_interactive_login(
    command: AuthLoginCommand,
    io: &mut CliIo<'_>,
    store: &CredentialStore,
) -> i32 {
    let _ = clack_intro(io.stdout, "Add credential");
    let auth_provider = match prompt_auth_provider(io) {
        Ok(Some(auth_provider)) => auth_provider,
        Ok(None) => return 1,
        Err(err) => return auth_prompt_io_error(err, io.stderr),
    };
    let method = match command.method {
        Some(method) => method,
        None => match prompt_login_method(auth_provider, io) {
            Ok(Some(method)) => method,
            Ok(None) => return 1,
            Err(err) => return auth_prompt_io_error(err, io.stderr),
        },
    };
    let enterprise_url =
        match interactive_enterprise_url(auth_provider, command.enterprise_url.clone(), io) {
            Ok(value) => value,
            Err(AuthInteractiveError::Cancelled) => return 1,
            Err(AuthInteractiveError::Io(err)) => return auth_prompt_io_error(err, io.stderr),
        };

    execute_login_selection(
        auth_provider,
        method,
        enterprise_url,
        command,
        io,
        store,
        AuthLoginUi::Interactive,
    )
}

enum AuthInteractiveError {
    Cancelled,
    Io(io::Error),
}

fn prompt_auth_provider(io: &mut CliIo<'_>) -> io::Result<Option<AuthProviderId>> {
    prompt_pick(
        io,
        "Select provider",
        &[
            AuthPromptOption {
                label: "OpenAI",
                value: AuthProviderId::Codex,
                hint: Some("ChatGPT Plus/Pro or API key"),
            },
            AuthPromptOption {
                label: "GitHub Copilot",
                value: AuthProviderId::GithubCopilot,
                hint: None,
            },
        ],
        true,
    )
}

fn prompt_login_method(
    auth_provider: AuthProviderId,
    io: &mut CliIo<'_>,
) -> io::Result<Option<AuthLoginMethod>> {
    if auth_provider == AuthProviderId::GithubCopilot {
        return Ok(Some(AuthLoginMethod::Device));
    }

    prompt_pick(
        io,
        "Login method",
        &[
            AuthPromptOption {
                label: "ChatGPT Pro/Plus (browser)",
                value: AuthLoginMethod::Browser,
                hint: None,
            },
            AuthPromptOption {
                label: "ChatGPT Pro/Plus (headless)",
                value: AuthLoginMethod::Device,
                hint: None,
            },
            AuthPromptOption {
                label: "Manually enter API Key",
                value: AuthLoginMethod::ApiKey,
                hint: None,
            },
        ],
        false,
    )
}

fn interactive_enterprise_url(
    auth_provider: AuthProviderId,
    explicit_enterprise_url: Option<String>,
    io: &mut CliIo<'_>,
) -> Result<Option<String>, AuthInteractiveError> {
    if auth_provider != AuthProviderId::GithubCopilot || explicit_enterprise_url.is_some() {
        return Ok(explicit_enterprise_url);
    }

    let deployment = prompt_pick(
        io,
        "Select GitHub deployment type",
        &[
            AuthPromptOption {
                label: "GitHub.com",
                value: CopilotDeploymentChoice::Public,
                hint: Some("Public"),
            },
            AuthPromptOption {
                label: "GitHub Enterprise",
                value: CopilotDeploymentChoice::Enterprise,
                hint: Some("Data residency or self-hosted"),
            },
        ],
        false,
    )
    .map_err(AuthInteractiveError::Io)?;

    match deployment {
        Some(CopilotDeploymentChoice::Public) => Ok(None),
        Some(CopilotDeploymentChoice::Enterprise) => prompt_copilot_enterprise_url(io),
        None => Err(AuthInteractiveError::Cancelled),
    }
}

fn prompt_copilot_enterprise_url(
    io: &mut CliIo<'_>,
) -> Result<Option<String>, AuthInteractiveError> {
    loop {
        let value = prompt_input(
            io,
            "Enter your GitHub Enterprise URL or domain",
            Some("company.ghe.com or https://company.ghe.com"),
            false,
        )
        .map_err(AuthInteractiveError::Io)?;
        let Some(value) = value else {
            return Err(AuthInteractiveError::Cancelled);
        };
        match CopilotDeployment::enterprise(&value) {
            Ok(CopilotDeployment::Enterprise { domain }) => return Ok(Some(domain)),
            Ok(CopilotDeployment::Public) => return Ok(None),
            Err(err) => {
                let _ = clack_log_error(io.stdout, &err.to_string());
            }
        }
    }
}

fn prompt_pick<T: Copy>(
    io: &mut CliIo<'_>,
    message: &str,
    options: &[AuthPromptOption<T>],
    searchable: bool,
) -> io::Result<Option<T>> {
    let terminal = auth_prompt_terminal_events_enabled();
    let _raw_mode = RawModeGuard::new(terminal);
    let mut selected = 0_usize;
    let mut filter = String::new();
    let mut rendered_lines = 0_usize;

    loop {
        let visible = visible_auth_options(options, &filter, searchable);
        if selected >= visible.len() {
            selected = 0;
        }
        rendered_lines = render_auth_picker(
            io.stdout,
            rendered_lines,
            AuthPickerRender {
                message,
                filter: &filter,
                options,
                visible: &visible,
                selected,
                searchable,
            },
        )?;

        match read_auth_prompt_key(io.stdin, terminal)? {
            AuthPromptKey::Up => {
                if !visible.is_empty() {
                    selected = if selected == 0 {
                        visible.len() - 1
                    } else {
                        selected - 1
                    };
                }
            }
            AuthPromptKey::Down => {
                if !visible.is_empty() {
                    selected = (selected + 1) % visible.len();
                }
            }
            AuthPromptKey::Submit => {
                clear_auth_prompt(io.stdout, rendered_lines)?;
                if let Some(option_index) = visible.get(selected) {
                    render_auth_picker_result(io.stdout, message, &options[*option_index])?;
                    return Ok(Some(options[*option_index].value));
                }
            }
            AuthPromptKey::Cancel => {
                clear_auth_prompt(io.stdout, rendered_lines)?;
                return Ok(None);
            }
            AuthPromptKey::Backspace => {
                if searchable {
                    filter.pop();
                    selected = 0;
                }
            }
            AuthPromptKey::Char(ch) => {
                if searchable && !ch.is_control() {
                    filter.push(ch);
                    selected = 0;
                }
            }
            AuthPromptKey::Ignored => {}
        }
    }
}

fn prompt_input(
    io: &mut CliIo<'_>,
    message: &str,
    placeholder: Option<&str>,
    secret: bool,
) -> io::Result<Option<String>> {
    let terminal = auth_prompt_terminal_events_enabled();
    let _raw_mode = RawModeGuard::new(terminal);
    let mut value = String::new();
    let mut rendered_lines = 0_usize;

    loop {
        rendered_lines = render_auth_input(
            io.stdout,
            rendered_lines,
            message,
            placeholder,
            &value,
            secret,
        )?;
        match read_auth_prompt_key(io.stdin, terminal)? {
            AuthPromptKey::Submit => {
                let trimmed = value.trim().to_string();
                if trimmed.is_empty() {
                    let _ = clack_log_error(io.stdout, "Required");
                    continue;
                }
                clear_auth_prompt(io.stdout, rendered_lines)?;
                render_auth_input_result(io.stdout, message, &value, secret)?;
                return Ok(Some(trimmed));
            }
            AuthPromptKey::Cancel => {
                clear_auth_prompt(io.stdout, rendered_lines)?;
                return Ok(None);
            }
            AuthPromptKey::Backspace => {
                value.pop();
            }
            AuthPromptKey::Char(ch) => {
                if !ch.is_control() {
                    value.push(ch);
                }
            }
            AuthPromptKey::Up | AuthPromptKey::Down | AuthPromptKey::Ignored => {}
        }
    }
}

fn visible_auth_options<T>(
    options: &[AuthPromptOption<T>],
    filter: &str,
    searchable: bool,
) -> Vec<usize> {
    let needle = filter.trim().to_ascii_lowercase();
    options
        .iter()
        .enumerate()
        .filter_map(|(index, option)| {
            if !searchable || needle.is_empty() {
                return Some(index);
            }
            let haystack = format!("{} {:?}", option.label, option.hint).to_ascii_lowercase();
            haystack.contains(&needle).then_some(index)
        })
        .collect()
}

struct AuthPickerRender<'a, T> {
    message: &'a str,
    filter: &'a str,
    options: &'a [AuthPromptOption<T>],
    visible: &'a [usize],
    selected: usize,
    searchable: bool,
}

fn render_auth_picker<T>(
    stdout: &mut dyn Write,
    previous_lines: usize,
    render: AuthPickerRender<'_, T>,
) -> io::Result<usize> {
    clear_auth_prompt(stdout, previous_lines)?;
    let mut lines = 0;
    write!(stdout, "│\r\n")?;
    lines += 1;
    if render.searchable && !render.filter.is_empty() {
        write!(
            stdout,
            "◆  {} {AUTH_DIM}{}{AUTH_RESET}\r\n",
            render.message, render.filter
        )?;
    } else {
        write!(stdout, "◆  {}\r\n", render.message)?;
    }
    lines += 1;
    if render.visible.is_empty() {
        write!(stdout, "│  {AUTH_DIM}No matches{AUTH_RESET}\r\n")?;
        lines += 1;
    } else {
        for (position, option_index) in render.visible.iter().enumerate() {
            let option = &render.options[*option_index];
            let marker = if position == render.selected {
                "●"
            } else {
                "○"
            };
            let hint = option
                .hint
                .map(|hint| format!(" {AUTH_DIM}{hint}{AUTH_RESET}"))
                .unwrap_or_default();
            write!(stdout, "│  {marker} {}{hint}\r\n", option.label)?;
            lines += 1;
        }
    }
    write!(
        stdout,
        "└  {AUTH_DIM}↑/↓ select, enter confirm, esc cancel{AUTH_RESET}\r\n"
    )?;
    lines += 1;
    stdout.flush()?;
    Ok(lines)
}

fn render_auth_picker_result<T>(
    stdout: &mut dyn Write,
    message: &str,
    option: &AuthPromptOption<T>,
) -> io::Result<()> {
    let hint = option
        .hint
        .map(|hint| format!(" {AUTH_DIM}{hint}{AUTH_RESET}"))
        .unwrap_or_default();
    write!(stdout, "│\r\n")?;
    write!(stdout, "◇  {message}\r\n")?;
    write!(stdout, "│  {}{hint}\r\n", option.label)?;
    stdout.flush()
}

fn render_auth_input(
    stdout: &mut dyn Write,
    previous_lines: usize,
    message: &str,
    placeholder: Option<&str>,
    value: &str,
    secret: bool,
) -> io::Result<usize> {
    clear_auth_prompt(stdout, previous_lines)?;
    let mut lines = 0;
    write!(stdout, "│\r\n")?;
    lines += 1;
    write!(stdout, "◆  {message}\r\n")?;
    lines += 1;
    let rendered_value = if value.is_empty() {
        placeholder
            .map(|value| format!("{AUTH_DIM}{value}{AUTH_RESET}"))
            .unwrap_or_default()
    } else if secret {
        "•".repeat(value.chars().count())
    } else {
        value.to_string()
    };
    write!(stdout, "│  {rendered_value}\r\n")?;
    lines += 1;
    write!(
        stdout,
        "└  {AUTH_DIM}enter confirm, esc cancel{AUTH_RESET}\r\n"
    )?;
    lines += 1;
    stdout.flush()?;
    Ok(lines)
}

fn render_auth_input_result(
    stdout: &mut dyn Write,
    message: &str,
    value: &str,
    secret: bool,
) -> io::Result<()> {
    let rendered_value = if secret {
        "•".repeat(value.chars().count())
    } else {
        value.to_string()
    };
    write!(stdout, "│\r\n")?;
    write!(stdout, "◇  {message}\r\n")?;
    write!(stdout, "│  {rendered_value}\r\n")?;
    stdout.flush()
}

fn clear_auth_prompt(stdout: &mut dyn Write, lines: usize) -> io::Result<()> {
    if lines > 0 {
        write!(stdout, "\x1b[{lines}A\x1b[J")?;
    }
    Ok(())
}

fn read_auth_prompt_key(
    stdin: &mut dyn std::io::BufRead,
    terminal: bool,
) -> io::Result<AuthPromptKey> {
    if terminal {
        return read_terminal_auth_prompt_key();
    }

    let Some(byte) = read_auth_prompt_byte(stdin)? else {
        return Ok(AuthPromptKey::Cancel);
    };
    match byte {
        b'\r' | b'\n' => Ok(AuthPromptKey::Submit),
        0x03 => Ok(AuthPromptKey::Cancel),
        0x7f | 0x08 => Ok(AuthPromptKey::Backspace),
        b'\t' => Ok(AuthPromptKey::Down),
        0x1b => read_auth_escape_key(stdin),
        byte if byte.is_ascii() => Ok(AuthPromptKey::Char(byte as char)),
        _ => Ok(AuthPromptKey::Ignored),
    }
}

fn read_terminal_auth_prompt_key() -> io::Result<AuthPromptKey> {
    loop {
        match crossterm::event::read().map_err(io::Error::other)? {
            Event::Key(key) if key.kind != KeyEventKind::Release => match key.code {
                KeyCode::Up => return Ok(AuthPromptKey::Up),
                KeyCode::Down | KeyCode::Tab => return Ok(AuthPromptKey::Down),
                KeyCode::Enter => return Ok(AuthPromptKey::Submit),
                KeyCode::Esc => return Ok(AuthPromptKey::Cancel),
                KeyCode::Backspace => return Ok(AuthPromptKey::Backspace),
                KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                    return Ok(AuthPromptKey::Cancel)
                }
                KeyCode::Char('n') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                    return Ok(AuthPromptKey::Down)
                }
                KeyCode::Char('p') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                    return Ok(AuthPromptKey::Up)
                }
                KeyCode::Char(ch) => return Ok(AuthPromptKey::Char(ch)),
                _ => return Ok(AuthPromptKey::Ignored),
            },
            _ => {}
        }
    }
}

fn read_auth_escape_key(stdin: &mut dyn std::io::BufRead) -> io::Result<AuthPromptKey> {
    let Some(next) = read_auth_prompt_byte(stdin)? else {
        return Ok(AuthPromptKey::Cancel);
    };
    if next != b'[' {
        return Ok(AuthPromptKey::Cancel);
    }
    match read_auth_prompt_byte(stdin)? {
        Some(b'A') => Ok(AuthPromptKey::Up),
        Some(b'B') => Ok(AuthPromptKey::Down),
        _ => Ok(AuthPromptKey::Ignored),
    }
}

fn read_auth_prompt_byte(stdin: &mut dyn std::io::BufRead) -> io::Result<Option<u8>> {
    let mut byte = [0_u8; 1];
    match stdin.read(&mut byte)? {
        0 => Ok(None),
        _ => Ok(Some(byte[0])),
    }
}

fn auth_prompt_terminal_events_enabled() -> bool {
    std::io::stdin().is_terminal()
}

fn auth_prompt_io_error(err: io::Error, stderr: &mut dyn Write) -> i32 {
    let _ = writeln!(
        stderr,
        "auth login failed: failed to read terminal input: {err}"
    );
    1
}

const AUTH_DIM: &str = "\x1b[90m";
const AUTH_RESET: &str = "\x1b[0m";
const AUTH_GREEN: &str = "\x1b[92m";
const AUTH_RED: &str = "\x1b[91m";

fn clack_intro(stdout: &mut dyn Write, message: &str) -> io::Result<()> {
    writeln!(stdout, "◇  {message}")
}

fn clack_outro(stdout: &mut dyn Write, message: &str) -> io::Result<()> {
    writeln!(stdout, "└  {message}")
}

fn clack_log_info(stdout: &mut dyn Write, message: &str) -> io::Result<()> {
    writeln!(stdout, "│  {message}")
}

fn clack_log_success(stdout: &mut dyn Write, message: &str) -> io::Result<()> {
    writeln!(stdout, "│  {AUTH_GREEN}{message}{AUTH_RESET}")
}

fn clack_log_error(stdout: &mut dyn Write, message: &str) -> io::Result<()> {
    writeln!(stdout, "│  {AUTH_RED}{message}{AUTH_RESET}")
}

fn store_api_key_login(
    auth_provider: AuthProviderId,
    command: AuthLoginCommand,
    io: &mut CliIo<'_>,
    store: &CredentialStore,
) -> i32 {
    if auth_provider != AuthProviderId::Codex {
        let _ = writeln!(
            io.stderr,
            "auth login failed: api-key login is supported for codex only in V1"
        );
        return 2;
    }
    if !command.api_key_stdin {
        let _ = writeln!(
            io.stderr,
            "auth login failed: pass --api-key-stdin and provide the key on stdin"
        );
        return 2;
    }
    let mut body = String::new();
    if let Err(err) = io.stdin.read_to_string(&mut body) {
        let _ = writeln!(io.stderr, "auth login failed: failed to read stdin: {err}");
        return 1;
    }
    let Some(api_key) = non_empty(&body).map(str::to_string) else {
        let _ = writeln!(
            io.stderr,
            "auth login failed: stdin did not contain an API key"
        );
        return 2;
    };
    store_api_key_value(auth_provider, api_key, io, store, AuthLoginUi::Plain)
}

fn store_interactive_api_key_login(
    auth_provider: AuthProviderId,
    io: &mut CliIo<'_>,
    store: &CredentialStore,
) -> i32 {
    if auth_provider != AuthProviderId::Codex {
        let _ = writeln!(
            io.stderr,
            "auth login failed: api-key login is supported for codex only in V1"
        );
        return 2;
    }
    let api_key = match prompt_input(io, "Enter your API key", None, true) {
        Ok(Some(api_key)) => api_key,
        Ok(None) => return 1,
        Err(err) => return auth_prompt_io_error(err, io.stderr),
    };
    store_api_key_value(auth_provider, api_key, io, store, AuthLoginUi::Interactive)
}

fn store_api_key_value(
    auth_provider: AuthProviderId,
    api_key: String,
    io: &mut CliIo<'_>,
    store: &CredentialStore,
    ui: AuthLoginUi,
) -> i32 {
    let credential =
        StoredCredential::api_key(auth_provider, api_key, SystemCredentialClock.now_rfc3339());
    match store.save(&credential) {
        Ok(()) => {
            if ui == AuthLoginUi::Interactive {
                let _ = clack_outro(io.stdout, "Done");
            } else {
                let _ = writeln!(
                    io.stdout,
                    "stored api_key credential for {} (secret redacted)",
                    auth_provider
                );
            }
            0
        }
        Err(err) => {
            let _ = writeln!(io.stderr, "auth login failed: {err}");
            1
        }
    }
}

fn store_mock_oauth_login(
    auth_provider: AuthProviderId,
    command: AuthLoginCommand,
    token: &str,
    io: &mut CliIo<'_>,
    store: &CredentialStore,
    ui: AuthLoginUi,
) -> i32 {
    let Some(token) = non_empty(token).map(str::to_string) else {
        let _ = writeln!(io.stderr, "auth login failed: mock token was empty");
        return 2;
    };
    let refresh = command
        .mock_refresh_token
        .as_deref()
        .and_then(non_empty)
        .unwrap_or(&token)
        .to_string();
    let mut credential = StoredCredential::oauth(
        auth_provider,
        token,
        refresh,
        command.expires_at.clone(),
        SystemCredentialClock.now_rfc3339(),
    );
    credential.account_id = command.account_id.clone();
    if auth_provider == AuthProviderId::GithubCopilot {
        credential.scopes = vec![COPILOT_SCOPE.to_string()];
        if let Some(input) = command.enterprise_url.as_deref() {
            match CopilotDeployment::enterprise(input) {
                Ok(CopilotDeployment::Enterprise { domain }) => {
                    credential.enterprise_url = Some(domain);
                }
                Ok(CopilotDeployment::Public) => {}
                Err(err) => {
                    let _ = writeln!(io.stderr, "auth login failed: {err}");
                    return 2;
                }
            }
        }
    }
    match store.save(&credential) {
        Ok(()) => {
            if ui == AuthLoginUi::Interactive {
                let _ = clack_log_success(io.stdout, "Login successful");
                let _ = clack_outro(io.stdout, "Done");
            } else {
                let _ = writeln!(
                    io.stdout,
                    "stored oauth credential for {} (secret redacted)",
                    auth_provider
                );
            }
            0
        }
        Err(err) => {
            let _ = writeln!(io.stderr, "auth login failed: {err}");
            1
        }
    }
}

fn run_device_login(
    auth_provider: AuthProviderId,
    enterprise_url: Option<String>,
    io: &mut CliIo<'_>,
    store: &CredentialStore,
    ui: AuthLoginUi,
) -> i32 {
    let runtime = match tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
    {
        Ok(runtime) => runtime,
        Err(err) => {
            let _ = writeln!(
                io.stderr,
                "auth login failed: failed to create runtime: {err}"
            );
            return 1;
        }
    };

    match auth_provider {
        AuthProviderId::Codex => runtime.block_on(run_codex_device_login(io, store, ui)),
        AuthProviderId::GithubCopilot => {
            runtime.block_on(run_copilot_device_login(enterprise_url, io, store, ui))
        }
    }
}

async fn run_codex_device_login(
    io: &mut CliIo<'_>,
    store: &CredentialStore,
    ui: AuthLoginUi,
) -> i32 {
    let client = CodexOAuthClient::new(Arc::new(ReqwestCodexAuthClient::new()));
    let device = match client.start_device_authorization().await {
        Ok(device) => device,
        Err(err) => return auth_oauth_error("auth login failed", err, io.stderr),
    };
    if ui == AuthLoginUi::Interactive {
        let _ = clack_log_info(io.stdout, &format!("Go to: {}", device.verification_uri));
        let _ = clack_log_info(io.stdout, &format!("Enter code: {}", device.user_code));
        let _ = clack_log_info(io.stdout, "Waiting for authorization...");
    } else {
        let _ = writeln!(io.stdout, "Harness Codex device login");
        let _ = writeln!(io.stdout, "Open {}", device.verification_uri);
        let _ = writeln!(io.stdout, "Enter code {}", device.user_code);
    }

    for _ in 0..DEFAULT_DEVICE_POLLS {
        match client.poll_device_authorization(&device).await {
            Ok(CodexDevicePoll::Pending) => {
                tokio::time::sleep(Duration::from_secs(device.interval_seconds.max(1))).await;
            }
            Ok(CodexDevicePoll::Authorized {
                authorization_code,
                code_verifier,
            }) => {
                let token_response = match client
                    .exchange_authorization_code(
                        &authorization_code,
                        "https://auth.openai.com/deviceauth/callback",
                        &PkceCodes {
                            verifier: code_verifier,
                            challenge: String::new(),
                        },
                    )
                    .await
                {
                    Ok(tokens) => tokens,
                    Err(err) => return auth_oauth_error("auth login failed", err, io.stderr),
                };
                let _credential = match client.store_tokens(store, token_response).await {
                    Ok(credential) => credential,
                    Err(err) => return auth_oauth_error("auth login failed", err, io.stderr),
                };
                if ui == AuthLoginUi::Interactive {
                    let _ = clack_log_success(io.stdout, "Login successful");
                    let _ = clack_outro(io.stdout, "Done");
                } else {
                    let _ = writeln!(
                        io.stdout,
                        "stored oauth credential for codex (secret redacted)"
                    );
                }
                return 0;
            }
            Err(err) => return auth_oauth_error("auth login failed", err, io.stderr),
        }
    }

    let _ = writeln!(io.stderr, "auth login failed: Codex device login timed out");
    1
}

async fn run_copilot_device_login(
    enterprise_url: Option<String>,
    io: &mut CliIo<'_>,
    store: &CredentialStore,
    ui: AuthLoginUi,
) -> i32 {
    let deployment = match enterprise_url.as_deref() {
        Some(input) => match CopilotDeployment::enterprise(input) {
            Ok(deployment) => deployment,
            Err(err) => return auth_oauth_error("auth login failed", err, io.stderr),
        },
        None => CopilotDeployment::public(),
    };
    let client = CopilotOAuthClient::new(Arc::new(ReqwestCopilotAuthClient::new()));
    let device = match client.start_device_authorization(&deployment).await {
        Ok(device) => device,
        Err(err) => return auth_oauth_error("auth login failed", err, io.stderr),
    };
    if ui == AuthLoginUi::Interactive {
        let _ = clack_log_info(io.stdout, &format!("Go to: {}", device.verification_uri));
        let _ = clack_log_info(io.stdout, &format!("Enter code: {}", device.user_code));
        let _ = clack_log_info(io.stdout, "Waiting for authorization...");
    } else {
        let _ = writeln!(io.stdout, "Harness GitHub Copilot device login");
        let _ = writeln!(io.stdout, "Open {}", device.verification_uri);
        let _ = writeln!(io.stdout, "Enter code {}", device.user_code);
    }

    let mut interval = device.interval_seconds;
    for _ in 0..DEFAULT_DEVICE_POLLS {
        match client
            .poll_device_token(&deployment, &device, interval)
            .await
        {
            Ok(CopilotDevicePoll::Pending { wait }) => tokio::time::sleep(wait).await,
            Ok(CopilotDevicePoll::SlowDown {
                interval_seconds,
                wait,
            }) => {
                interval = interval_seconds;
                tokio::time::sleep(wait).await;
            }
            Ok(CopilotDevicePoll::Authorized { access_token }) => {
                let mut credential = StoredCredential::oauth(
                    AuthProviderId::GithubCopilot,
                    access_token.clone(),
                    access_token,
                    None,
                    SystemCredentialClock.now_rfc3339(),
                );
                credential.scopes = vec![COPILOT_SCOPE.to_string()];
                if let CopilotDeployment::Enterprise { domain } = deployment {
                    credential.enterprise_url = Some(domain);
                }
                return match store.save(&credential) {
                    Ok(()) => {
                        if ui == AuthLoginUi::Interactive {
                            let _ = clack_log_success(io.stdout, "Login successful");
                            let _ = clack_outro(io.stdout, "Done");
                        } else {
                            let _ = writeln!(
                                io.stdout,
                                "stored oauth credential for github-copilot (secret redacted)"
                            );
                        }
                        0
                    }
                    Err(err) => credential_store_error("auth login failed", err, io.stderr),
                };
            }
            Err(err) => return auth_oauth_error("auth login failed", err, io.stderr),
        }
    }

    let _ = writeln!(
        io.stderr,
        "auth login failed: GitHub Copilot device login timed out"
    );
    1
}

fn run_codex_browser_login(
    auth_provider: AuthProviderId,
    io: &mut CliIo<'_>,
    store: &CredentialStore,
    ui: AuthLoginUi,
) -> i32 {
    if auth_provider != AuthProviderId::Codex {
        let _ = writeln!(
            io.stderr,
            "auth login failed: browser login is supported for codex only in V1"
        );
        return 2;
    }
    let runtime = match tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
    {
        Ok(runtime) => runtime,
        Err(err) => {
            let _ = writeln!(
                io.stderr,
                "auth login failed: failed to create runtime: {err}"
            );
            return 1;
        }
    };

    runtime.block_on(run_codex_browser_login_with_client(
        CODEX_ISSUER,
        Arc::new(ReqwestCodexAuthClient::new()),
        CODEX_OAUTH_PORT,
        io,
        store,
        ui,
    ))
}

async fn run_codex_browser_login_with_client(
    issuer: &str,
    http: Arc<dyn CodexAuthHttpClient>,
    port: u16,
    io: &mut CliIo<'_>,
    store: &CredentialStore,
    ui: AuthLoginUi,
) -> i32 {
    let listener = match tokio::net::TcpListener::bind(SocketAddr::from(([127, 0, 0, 1], port)))
        .await
    {
        Ok(listener) => listener,
        Err(err) => {
            let _ = writeln!(
                io.stderr,
                "auth login failed: could not bind Codex loopback callback on 127.0.0.1:{port}: {err}"
            );
            return 1;
        }
    };
    let redirect_uri = format!(
        "http://localhost:{}/auth/callback",
        listener
            .local_addr()
            .map(|addr| addr.port())
            .unwrap_or(port)
    );
    let pkce = match harness_core::auth::codex::generate_pkce() {
        Ok(pkce) => pkce,
        Err(err) => return auth_oauth_error("auth login failed", err, io.stderr),
    };
    let state = codex_browser_state(&pkce);
    let session = CodexLoopbackSession::with_redirect_uri(pkce, state, redirect_uri, issuer);
    let client = CodexOAuthClient::new(http).with_issuer(issuer);
    complete_codex_browser_loopback(
        listener,
        client,
        session,
        io,
        store,
        CODEX_BROWSER_TIMEOUT,
        ui,
    )
    .await
}

async fn complete_codex_browser_loopback(
    listener: tokio::net::TcpListener,
    client: CodexOAuthClient,
    session: CodexLoopbackSession,
    io: &mut CliIo<'_>,
    store: &CredentialStore,
    timeout: Duration,
    ui: AuthLoginUi,
) -> i32 {
    let terminal_manual_callback = auth_prompt_terminal_events_enabled();
    if ui == AuthLoginUi::Interactive {
        let _ = clack_log_info(io.stdout, &format!("Go to: {}", session.authorize_url));
        let _ = clack_log_info(
            io.stdout,
            "Complete authorization in your browser. This window will close automatically.",
        );
        if terminal_manual_callback {
            let _ = clack_log_info(
                io.stdout,
                "If the browser cannot reach this SSH host, paste the final localhost callback URL here and press Enter.",
            );
        }
        let _ = clack_log_info(io.stdout, "Waiting for authorization...");
    } else {
        let _ = writeln!(io.stdout, "Harness Codex browser login");
        let _ = writeln!(io.stdout, "Open {}", session.authorize_url);
        if terminal_manual_callback {
            let _ = writeln!(
                io.stdout,
                "If the browser cannot reach this SSH host, paste the final localhost callback URL here and press Enter."
            );
        }
        let _ = writeln!(
            io.stdout,
            "Waiting for callback on {} (timeout {}s)",
            session.redirect_uri,
            timeout.as_secs()
        );
    }

    let deadline = tokio::time::Instant::now() + timeout;
    let completion = if terminal_manual_callback {
        let _raw_mode = RawModeGuard::new(true);
        tokio::select! {
            result = tokio::time::timeout_at(
                deadline,
                receive_codex_loopback_callback(&listener, &client, &session, store),
            ) => match result {
                Ok(result) => CodexBrowserCompletion::Credential(result),
                Err(_) => CodexBrowserCompletion::Timeout,
            },
            manual = read_terminal_manual_callback_url(deadline) => match manual {
                Ok(ManualCallbackInput::Url(callback_url)) => match tokio::time::timeout_at(
                    deadline,
                    complete_codex_pasted_callback(&client, &session, store, &callback_url),
                ).await {
                    Ok(result) => CodexBrowserCompletion::Credential(result),
                    Err(_) => CodexBrowserCompletion::Timeout,
                },
                Ok(ManualCallbackInput::Cancelled) => CodexBrowserCompletion::Cancelled,
                Ok(ManualCallbackInput::Timeout) => CodexBrowserCompletion::Timeout,
                Err(err) => CodexBrowserCompletion::InputError(err),
            },
        }
    } else {
        match tokio::time::timeout(
            timeout,
            receive_codex_loopback_callback(&listener, &client, &session, store),
        )
        .await
        {
            Ok(result) => CodexBrowserCompletion::Credential(result),
            Err(_) => CodexBrowserCompletion::Timeout,
        }
    };

    match completion {
        CodexBrowserCompletion::Credential(Ok(_credential)) => {
            if ui == AuthLoginUi::Interactive {
                let _ = clack_log_success(io.stdout, "Login successful");
                let _ = clack_outro(io.stdout, "Done");
            } else {
                let _ = writeln!(
                    io.stdout,
                    "stored oauth credential for codex (secret redacted)"
                );
            }
            0
        }
        CodexBrowserCompletion::Credential(Err(err)) => {
            auth_oauth_error("auth login failed", err, io.stderr)
        }
        CodexBrowserCompletion::InputError(err) => auth_prompt_io_error(err, io.stderr),
        CodexBrowserCompletion::Cancelled => 1,
        CodexBrowserCompletion::Timeout => {
            auth_oauth_error("auth login failed", session.timeout_error(), io.stderr)
        }
    }
}

enum CodexBrowserCompletion {
    Credential(Result<StoredCredential, CodexOAuthError>),
    InputError(io::Error),
    Cancelled,
    Timeout,
}

enum ManualCallbackInput {
    Url(String),
    Cancelled,
    Timeout,
}

async fn read_terminal_manual_callback_url(
    deadline: tokio::time::Instant,
) -> io::Result<ManualCallbackInput> {
    let mut value = String::new();

    loop {
        if tokio::time::Instant::now() >= deadline {
            return Ok(ManualCallbackInput::Timeout);
        }
        if crossterm::event::poll(Duration::ZERO).map_err(io::Error::other)? {
            match crossterm::event::read().map_err(io::Error::other)? {
                Event::Key(key) if key.kind != KeyEventKind::Release => match key.code {
                    KeyCode::Enter => {
                        let callback_url = value.trim().to_string();
                        if !callback_url.is_empty() {
                            return Ok(ManualCallbackInput::Url(callback_url));
                        }
                    }
                    KeyCode::Esc => return Ok(ManualCallbackInput::Cancelled),
                    KeyCode::Backspace => {
                        value.pop();
                    }
                    KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                        return Ok(ManualCallbackInput::Cancelled);
                    }
                    KeyCode::Char(ch) => {
                        if !ch.is_control() {
                            value.push(ch);
                        }
                    }
                    _ => {}
                },
                Event::Paste(text) => {
                    let callback_url = text.trim().to_string();
                    if !callback_url.is_empty() {
                        return Ok(ManualCallbackInput::Url(callback_url));
                    }
                }
                _ => {}
            }
        }
        tokio::time::sleep(MANUAL_CALLBACK_POLL_INTERVAL).await;
    }
}

async fn complete_codex_pasted_callback(
    client: &CodexOAuthClient,
    session: &CodexLoopbackSession,
    store: &CredentialStore,
    callback_url: &str,
) -> Result<StoredCredential, CodexOAuthError> {
    client
        .complete_loopback_callback(session, callback_url, store)
        .await
}

async fn receive_codex_loopback_callback(
    listener: &tokio::net::TcpListener,
    client: &CodexOAuthClient,
    session: &CodexLoopbackSession,
    store: &CredentialStore,
) -> Result<StoredCredential, CodexOAuthError> {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    let (mut stream, _) = listener
        .accept()
        .await
        .map_err(|err| CodexOAuthError::Http {
            message: format!("loopback accept failed: {err}"),
        })?;
    let mut buffer = [0_u8; 16 * 1024];
    let bytes_read = stream
        .read(&mut buffer)
        .await
        .map_err(|err| CodexOAuthError::Http {
            message: format!("loopback read failed: {err}"),
        })?;
    let request = String::from_utf8_lossy(&buffer[..bytes_read]);
    let target = loopback_request_target(&request)?;
    let callback_url = if target.starts_with("http://") || target.starts_with("https://") {
        target
    } else {
        format!("{}{}", loopback_origin(&session.redirect_uri), target)
    };

    let result = client
        .complete_loopback_callback(session, &callback_url, store)
        .await;
    let (status, body) = match &result {
        Ok(_) => (200_u16, codex_callback_success_html().to_string()),
        Err(err) => (400_u16, codex_callback_error_html(&err.to_string())),
    };
    let response = format!(
        "HTTP/1.1 {status} {}\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
        if status == 200 { "OK" } else { "Bad Request" },
        body.len(),
        body
    );
    stream
        .write_all(response.as_bytes())
        .await
        .map_err(|err| CodexOAuthError::Http {
            message: format!("loopback response failed: {err}"),
        })?;
    let _ = stream.shutdown().await;
    result
}

fn loopback_request_target(request: &str) -> Result<String, CodexOAuthError> {
    let line = request
        .lines()
        .next()
        .ok_or_else(|| CodexOAuthError::Http {
            message: "loopback request was empty".to_string(),
        })?;
    let mut parts = line.split_whitespace();
    let method = parts.next().unwrap_or_default();
    let target = parts.next().unwrap_or_default();
    if method != "GET" || target.is_empty() {
        return Err(CodexOAuthError::Http {
            message: "loopback request was not a GET callback".to_string(),
        });
    }
    Ok(target.to_string())
}

fn loopback_origin(redirect_uri: &str) -> String {
    redirect_uri
        .split_once("/auth/callback")
        .map(|(origin, _)| origin.to_string())
        .unwrap_or_else(|| "http://localhost".to_string())
}

fn codex_browser_state(pkce: &PkceCodes) -> String {
    let seed = format!("harness-codex-oauth-state:{}", pkce.verifier);
    pkce_challenge(&seed).chars().take(32).collect()
}

fn auth_status(
    auth_provider: AuthProviderId,
    fallbacks: Option<&AuthProviderFallbacks>,
    env_var_is_set: &dyn Fn(&str) -> bool,
    credential_store: Option<&CredentialStore>,
) -> ProviderAuthStatus {
    let provider_ids = fallbacks
        .map(|fallbacks| fallbacks.provider_ids.iter().cloned().collect::<Vec<_>>())
        .unwrap_or_default();
    let env_fallback_configured = fallbacks
        .map(|fallbacks| {
            fallbacks
                .api_key_env
                .iter()
                .any(|name| env_var_is_set(name))
        })
        .unwrap_or(false);
    let inline_fallback_configured = fallbacks
        .map(|fallbacks| fallbacks.inline_configured)
        .unwrap_or(false);

    let stored = match credential_store.map(|store| store.load(auth_provider)) {
        Some(Ok(stored)) => stored,
        Some(Err(err)) => {
            return ProviderAuthStatus {
                auth_provider: auth_provider.to_string(),
                provider_ids,
                source: "credential_store_error".to_string(),
                presence: "error".to_string(),
                kind: None,
                expires_at: None,
                account_id: None,
                enterprise_url: None,
                error: Some(err.to_string()),
                env_fallback_configured,
                inline_fallback_configured,
                usable_without_network_probe: false,
            };
        }
        None => None,
    };

    if let Some(stored) = stored {
        let kind = stored_credential_kind_label(stored.kind).to_string();
        return ProviderAuthStatus {
            auth_provider: auth_provider.to_string(),
            provider_ids,
            source: format!("stored_{kind}"),
            presence: "stored".to_string(),
            kind: Some(kind),
            expires_at: stored.expires_at.clone(),
            account_id: stored
                .account_id
                .as_ref()
                .and_then(|value| redact_present(value)),
            enterprise_url: stored
                .enterprise_url
                .as_ref()
                .and_then(|value| redact_present(value)),
            error: None,
            env_fallback_configured,
            inline_fallback_configured,
            usable_without_network_probe: true,
        };
    }

    let (presence, source, usable) = if env_fallback_configured {
        ("env", "apiKeyEnv", true)
    } else if inline_fallback_configured {
        ("inline", "inline_apiKey", true)
    } else {
        ("missing", "none", false)
    };

    ProviderAuthStatus {
        auth_provider: auth_provider.to_string(),
        provider_ids,
        source: source.to_string(),
        presence: presence.to_string(),
        kind: None,
        expires_at: None,
        account_id: None,
        enterprise_url: None,
        error: None,
        env_fallback_configured,
        inline_fallback_configured,
        usable_without_network_probe: usable,
    }
}

fn configured_auth_provider_fallbacks(
    config: Option<&HarnessConfig>,
) -> BTreeMap<AuthProviderId, AuthProviderFallbacks> {
    let mut map = BTreeMap::<AuthProviderId, AuthProviderFallbacks>::new();
    let Some(config) = config else {
        return map;
    };

    for (provider_id, provider) in &config.providers {
        let ProviderConfig::OpenAiCompatible(provider) = provider;
        let Some(auth_provider) = provider.auth_provider else {
            continue;
        };
        let entry = map.entry(auth_provider).or_default();
        entry.provider_ids.insert(provider_id.clone());
        entry
            .api_key_env
            .extend(provider.api_key_env.iter().cloned());
        entry.inline_configured |= non_empty(&provider.api_key).is_some();
    }
    map
}

fn resolve_provider_arg(
    provider: Option<&str>,
    config: Option<&HarnessConfig>,
    stderr: &mut dyn Write,
) -> Option<AuthProviderId> {
    if let Some(provider) = provider {
        if let Some(auth_provider) = AuthProviderId::parse(provider) {
            return Some(auth_provider);
        }
        let _ = writeln!(
            stderr,
            "unknown auth provider `{provider}`; expected codex or github-copilot"
        );
        return None;
    }

    let configured = configured_auth_provider_fallbacks(config)
        .into_keys()
        .collect::<Vec<_>>();
    match configured.as_slice() {
        [only] => Some(*only),
        [] => {
            let _ = writeln!(
                stderr,
                "auth provider is required when config has no provider authProvider; expected codex or github-copilot"
            );
            None
        }
        _ => {
            let _ = writeln!(
                stderr,
                "auth provider is required when multiple auth providers are configured; expected codex or github-copilot"
            );
            None
        }
    }
}

fn resolve_login_provider_arg(provider: &str, stderr: &mut dyn Write) -> Option<AuthProviderId> {
    match provider.trim().to_ascii_lowercase().as_str() {
        "codex" | "openai" => Some(AuthProviderId::Codex),
        "github-copilot" | "github copilot" => Some(AuthProviderId::GithubCopilot),
        _ => {
            let _ = writeln!(
                stderr,
                "unknown auth provider `{provider}`; expected codex, openai, or github-copilot"
            );
            None
        }
    }
}

fn load_optional_config(
    config_path: Option<PathBuf>,
    session_dir: Option<PathBuf>,
    stderr: &mut dyn Write,
    deps: &CliDeps,
) -> Option<HarnessConfig> {
    let context = match deps.config_load_context() {
        Ok(context) => context,
        Err(err) => {
            let _ = writeln!(
                stderr,
                "auth warning: failed to resolve config context: {err}"
            );
            return None;
        }
    };
    let mut config = match load_resolved_config_with_context(config_path.as_deref(), &context) {
        Ok(Some(loaded)) => loaded.config,
        Ok(None) => return None,
        Err(err) => {
            let _ = writeln!(stderr, "auth warning: failed to load config: {err}");
            return None;
        }
    };
    config.apply_session_dir_override(session_dir);
    Some(config)
}

fn credential_store_from_deps(deps: &CliDeps) -> Option<CredentialStore> {
    CredentialStore::from_lookup(&|name| deps.env_var_value(name))
}

fn credential_store_or_error(stderr: &mut dyn Write, deps: &CliDeps) -> Option<CredentialStore> {
    let store = credential_store_from_deps(deps);
    if store.is_none() {
        let _ = writeln!(
            stderr,
            "auth failed: could not resolve a Harness data directory; set HARNESS_DATA_HOME, XDG_DATA_HOME, or HOME"
        );
    }
    store
}

fn stored_credential_kind_label(kind: StoredCredentialKind) -> &'static str {
    match kind {
        StoredCredentialKind::Oauth => "oauth",
        StoredCredentialKind::ApiKey => "api_key",
    }
}

fn redact_present(value: &str) -> Option<String> {
    non_empty(value).map(|_| "<redacted>".to_string())
}

fn non_empty(value: &str) -> Option<&str> {
    let trimmed = value.trim();
    (!trimmed.is_empty()).then_some(trimmed)
}

fn credential_store_error(prefix: &str, err: CredentialStoreError, stderr: &mut dyn Write) -> i32 {
    let _ = writeln!(stderr, "{prefix}: {err}");
    1
}

fn auth_oauth_error<E: std::fmt::Display>(prefix: &str, err: E, stderr: &mut dyn Write) -> i32 {
    let _ = writeln!(stderr, "{prefix}: {err}");
    1
}

#[derive(Debug, Default)]
struct ReqwestCodexAuthClient {
    client: reqwest::Client,
}

impl ReqwestCodexAuthClient {
    fn new() -> Self {
        Self {
            client: reqwest::Client::new(),
        }
    }
}

#[async_trait]
impl CodexAuthHttpClient for ReqwestCodexAuthClient {
    async fn send(&self, request: AuthHttpRequest) -> Result<AuthHttpResponse, CodexOAuthError> {
        let mut builder = self.client.post(&request.url).body(request.body);
        for (name, value) in request.headers {
            builder = builder.header(name, value);
        }
        let response = builder.send().await.map_err(|err| CodexOAuthError::Http {
            message: err.to_string(),
        })?;
        let status = response.status().as_u16();
        let body = response.text().await.map_err(|err| CodexOAuthError::Http {
            message: err.to_string(),
        })?;
        Ok(AuthHttpResponse { status, body })
    }
}

#[derive(Debug, Default)]
struct ReqwestCopilotAuthClient {
    client: reqwest::Client,
}

impl ReqwestCopilotAuthClient {
    fn new() -> Self {
        Self {
            client: reqwest::Client::new(),
        }
    }
}

#[async_trait]
impl CopilotAuthHttpClient for ReqwestCopilotAuthClient {
    async fn send(&self, request: AuthHttpRequest) -> Result<AuthHttpResponse, CopilotOAuthError> {
        let mut builder = self.client.post(&request.url).body(request.body);
        for (name, value) in request.headers {
            builder = builder.header(name, value);
        }
        let response = builder
            .send()
            .await
            .map_err(|err| CopilotOAuthError::Http {
                message: err.to_string(),
            })?;
        let status = response.status().as_u16();
        let body = response
            .text()
            .await
            .map_err(|err| CopilotOAuthError::Http {
                message: err.to_string(),
            })?;
        Ok(AuthHttpResponse { status, body })
    }
}

#[cfg(test)]
mod tests {
    use harness_core::auth::codex::{
        AuthHttpMethod, AuthHttpRequest, AuthHttpResponse, CodexLoopbackSession, CodexOAuthClient,
        CodexOAuthError, PkceCodes,
    };
    use harness_core::auth::{
        AuthProviderId, CredentialStore, StoredCredential, StoredCredentialKind,
    };
    use harness_core::config::load_config_from_str;
    use std::collections::VecDeque;
    use std::path::Path;
    use std::sync::{Arc, Mutex};
    use std::time::Duration;
    use tempfile::tempdir;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    use super::{
        complete_codex_browser_loopback, complete_codex_pasted_callback,
        onboarding_required_for_config,
    };

    fn codex_config(provider_fields: &str) -> harness_core::config::HarnessConfig {
        load_config_from_str(&format!(
            r#"
            {{
              provider: {{
                codex_route: {{
                  type: "openai_compatible",
                  baseURL: "http://127.0.0.1:8317/v1",
                  authProvider: "codex",
                  {provider_fields}
                  models: {{
                    "gpt-5.4-mini": {{ name: "GPT-5.4 mini" }},
                  }},
                }},
              }},
              model: "codex_route/gpt-5.4-mini",
              permission: "ask",
            }}
            "#
        ))
        .expect("load auth config")
    }

    fn auth_args(args: &[&str]) -> Vec<String> {
        args.iter().map(|arg| (*arg).to_string()).collect()
    }

    fn auth_deps(data_home: &Path) -> crate::CliDeps {
        crate::CliDeps::real().with_env(
            "HARNESS_DATA_HOME",
            data_home.to_string_lossy().into_owned(),
        )
    }

    fn load_stored(data_home: &Path, provider: AuthProviderId) -> StoredCredential {
        CredentialStore::new(data_home.join("harness"))
            .load(provider)
            .expect("load stored credential")
            .expect("credential stored")
    }

    #[test]
    fn onboarding_required_only_when_configured_auth_provider_has_no_usable_fallback() {
        let missing = codex_config("");
        assert!(onboarding_required_for_config(
            Some(&missing),
            &|_| false,
            None
        ));

        let env_config = codex_config(r#"apiKeyEnv: ["HARNESS_ONBOARDING_KEY"],"#);
        assert!(!onboarding_required_for_config(
            Some(&env_config),
            &|name| name == "HARNESS_ONBOARDING_KEY",
            None
        ));

        let inline_config = codex_config(r#"apiKey: "INLINE_TEST_KEY","#);
        assert!(!onboarding_required_for_config(
            Some(&inline_config),
            &|_| false,
            None
        ));

        let temp = tempdir().expect("tempdir");
        let store = CredentialStore::new(temp.path());
        store
            .save(&StoredCredential::oauth(
                AuthProviderId::Codex,
                "stored-access-secret",
                "stored-refresh-secret",
                Some("2099-01-02T03:04:05Z".to_string()),
                "2026-05-30T00:00:00Z",
            ))
            .expect("save stored credential");
        assert!(!onboarding_required_for_config(
            Some(&missing),
            &|_| false,
            Some(&store)
        ));
    }

    #[test]
    fn interactive_auth_login_provider_picker_cancels_without_stacktrace() {
        let temp = tempdir().expect("tempdir");
        let args = auth_args(&["login"]);

        let output =
            super::execute_backend_args(&args, None, None, "\x1b", &auth_deps(temp.path()));

        assert_eq!(output.code, 1);
        assert!(output.stdout.contains("Add credential"));
        assert!(output.stdout.contains("Select provider"));
        assert!(output.stdout.contains("OpenAI"));
        assert!(output.stdout.contains("GitHub Copilot"));
        assert!(output.stderr.is_empty(), "stderr: {}", output.stderr);
        assert!(
            !temp.path().join("harness/credentials/codex.json").exists(),
            "cancelled picker must not store credentials"
        );
    }

    #[test]
    fn interactive_codex_api_key_stores_without_echoing_secret() {
        let temp = tempdir().expect("tempdir");
        let secret = "sk-interactive-auth-secret-value";
        let args = auth_args(&["login"]);
        let stdin = format!("\n\x1b[B\x1b[B\n{secret}\n");

        let output =
            super::execute_backend_args(&args, None, None, &stdin, &auth_deps(temp.path()));

        assert_eq!(
            output.code, 0,
            "stdout:\n{}\nstderr:\n{}",
            output.stdout, output.stderr
        );
        assert!(output.stdout.contains("Manually enter API Key"));
        assert!(output.stdout.contains("Done"));
        assert!(!output.stdout.contains(secret));
        assert!(!output.stderr.contains(secret));
        let stored = load_stored(temp.path(), AuthProviderId::Codex);
        assert_eq!(stored.kind, StoredCredentialKind::ApiKey);
        assert_eq!(stored.api_key.as_deref(), Some(secret));
    }

    #[test]
    fn interactive_codex_browser_and_device_resolve_to_mockable_oauth_paths() {
        for (stdin, expected_label) in [
            ("\n\n", "ChatGPT Pro/Plus (browser)"),
            ("\n\x1b[B\n", "ChatGPT Pro/Plus (headless)"),
        ] {
            let temp = tempdir().expect("tempdir");
            let token = format!("oauth-{expected_label}-secret");
            let args = auth_args(&[
                "login",
                "--mock-token",
                &token,
                "--mock-refresh-token",
                "refresh-secret",
            ]);

            let output =
                super::execute_backend_args(&args, None, None, stdin, &auth_deps(temp.path()));

            assert_eq!(
                output.code, 0,
                "label: {expected_label}\nstdout:\n{}\nstderr:\n{}",
                output.stdout, output.stderr
            );
            assert!(output.stdout.contains(expected_label));
            assert!(output.stdout.contains("Login successful"));
            assert!(!output.stdout.contains(&token));
            assert!(!output.stderr.contains(&token));
            let stored = load_stored(temp.path(), AuthProviderId::Codex);
            assert_eq!(stored.kind, StoredCredentialKind::Oauth);
            assert_eq!(stored.access_token.as_deref(), Some(token.as_str()));
        }
    }

    #[test]
    fn interactive_github_copilot_resolves_to_mockable_device_flow() {
        let temp = tempdir().expect("tempdir");
        let token = "copilot-interactive-secret";
        let args = auth_args(&["login", "--mock-token", token]);

        let output =
            super::execute_backend_args(&args, None, None, "\x1b[B\n\n", &auth_deps(temp.path()));

        assert_eq!(
            output.code, 0,
            "stdout:\n{}\nstderr:\n{}",
            output.stdout, output.stderr
        );
        assert!(output.stdout.contains("GitHub Copilot"));
        assert!(output.stdout.contains("Select GitHub deployment type"));
        assert!(output.stdout.contains("Login successful"));
        assert!(!output.stdout.contains(token));
        assert!(!output.stderr.contains(token));
        let stored = load_stored(temp.path(), AuthProviderId::GithubCopilot);
        assert_eq!(stored.kind, StoredCredentialKind::Oauth);
        assert_eq!(stored.access_token.as_deref(), Some(token));
    }

    #[test]
    fn explicit_auth_login_args_bypass_interactive_picker() {
        let temp = tempdir().expect("tempdir");
        let secret = "sk-explicit-auth-secret-value";
        let args = auth_args(&[
            "login",
            "OpenAI",
            "--method",
            "Manually enter API Key",
            "--api-key-stdin",
        ]);

        let output = super::execute_backend_args(
            &args,
            None,
            None,
            &format!("{secret}\n"),
            &auth_deps(temp.path()),
        );

        assert_eq!(
            output.code, 0,
            "stdout:\n{}\nstderr:\n{}",
            output.stdout, output.stderr
        );
        assert!(!output.stdout.contains("Select provider"));
        assert!(!output.stdout.contains(secret));
        assert!(!output.stderr.contains(secret));
        let stored = load_stored(temp.path(), AuthProviderId::Codex);
        assert_eq!(stored.kind, StoredCredentialKind::ApiKey);
        assert_eq!(stored.api_key.as_deref(), Some(secret));
    }

    #[test]
    fn supported_method_labels_parse_for_supported_providers() {
        assert_eq!(
            super::parse_login_method_arg("ChatGPT Pro/Plus (browser)"),
            Ok(super::AuthLoginMethod::Browser)
        );
        assert_eq!(
            super::parse_login_method_arg("ChatGPT Pro/Plus (headless)"),
            Ok(super::AuthLoginMethod::Device)
        );
        assert_eq!(
            super::parse_login_method_arg("Manually enter API Key"),
            Ok(super::AuthLoginMethod::ApiKey)
        );
        assert_eq!(
            super::parse_login_method_arg("Login with GitHub Copilot"),
            Ok(super::AuthLoginMethod::Device)
        );
    }

    #[derive(Debug)]
    struct MockCodexAuthHttpClient {
        responses: Mutex<VecDeque<AuthHttpResponse>>,
        requests: Mutex<Vec<AuthHttpRequest>>,
    }

    #[async_trait::async_trait]
    impl super::CodexAuthHttpClient for MockCodexAuthHttpClient {
        async fn send(
            &self,
            request: AuthHttpRequest,
        ) -> Result<AuthHttpResponse, CodexOAuthError> {
            self.requests.lock().expect("requests").push(request);
            self.responses
                .lock()
                .expect("responses")
                .pop_front()
                .ok_or_else(|| CodexOAuthError::Http {
                    message: "no mocked auth response".to_string(),
                })
        }
    }

    impl MockCodexAuthHttpClient {
        fn new(response: AuthHttpResponse) -> Arc<Self> {
            Arc::new(Self {
                responses: Mutex::new(VecDeque::from([response])),
                requests: Mutex::new(Vec::new()),
            })
        }

        fn requests(&self) -> Vec<AuthHttpRequest> {
            self.requests.lock().expect("requests").clone()
        }
    }

    #[tokio::test]
    async fn codex_browser_login_accepts_pasted_localhost_callback_url() {
        let http = MockCodexAuthHttpClient::new(AuthHttpResponse {
            status: 200,
            body: serde_json::json!({
                "access_token": "pasted-access-secret",
                "refresh_token": "pasted-refresh-secret",
                "expires_in": 3600
            })
            .to_string(),
        });
        let client = CodexOAuthClient::new(http.clone()).with_issuer("https://issuer.test");
        let session = CodexLoopbackSession::with_redirect_uri(
            PkceCodes {
                verifier: "pasted-verifier-123".to_string(),
                challenge: "pasted-challenge-123".to_string(),
            },
            "state-123",
            "http://localhost:1455/auth/callback",
            "https://issuer.test",
        );
        let temp = tempdir().expect("tempdir");
        let store = CredentialStore::new(temp.path());

        let credential = complete_codex_pasted_callback(
            &client,
            &session,
            &store,
            "http://localhost:1455/auth/callback?code=pasted-code-123&state=state-123",
        )
        .await
        .expect("complete pasted callback");

        assert_eq!(
            credential.access_token.as_deref(),
            Some("pasted-access-secret")
        );
        let stored = store
            .load(AuthProviderId::Codex)
            .expect("load credential")
            .expect("stored credential");
        assert_eq!(stored.access_token.as_deref(), Some("pasted-access-secret"));
        assert_eq!(
            stored.refresh_token.as_deref(),
            Some("pasted-refresh-secret")
        );
        let requests = http.requests();
        assert_eq!(requests.len(), 1);
        assert_eq!(requests[0].method, AuthHttpMethod::Post);
        assert_eq!(requests[0].url, "https://issuer.test/oauth/token");
        assert!(requests[0].body.contains("grant_type=authorization_code"));
        assert!(requests[0].body.contains("code=pasted-code-123"));
        assert!(requests[0]
            .body
            .contains("code_verifier=pasted-verifier-123"));
    }

    #[tokio::test]
    async fn codex_browser_login_loopback_uses_cli_listener_and_stores_credential() {
        let http = MockCodexAuthHttpClient::new(AuthHttpResponse {
            status: 200,
            body: serde_json::json!({
                "access_token": "browser-access-secret",
                "refresh_token": "browser-refresh-secret",
                "expires_in": 3600
            })
            .to_string(),
        });
        let client = CodexOAuthClient::new(http.clone()).with_issuer("https://issuer.test");
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind loopback");
        let port = listener.local_addr().expect("loopback addr").port();
        let session = CodexLoopbackSession::with_redirect_uri(
            PkceCodes {
                verifier: "browser-verifier-123".to_string(),
                challenge: "browser-challenge-123".to_string(),
            },
            "state-123",
            format!("http://localhost:{port}/auth/callback"),
            "https://issuer.test",
        );
        let temp = tempdir().expect("tempdir");
        let store = CredentialStore::new(temp.path());
        let callback_sender = tokio::spawn(async move {
            let mut stream = tokio::net::TcpStream::connect(("127.0.0.1", port))
                .await
                .expect("connect callback");
            stream
                .write_all(
                    b"GET /auth/callback?code=browser-code-123&state=state-123 HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n",
                )
                .await
                .expect("write callback");
            let mut response = String::new();
            stream
                .read_to_string(&mut response)
                .await
                .expect("read callback response");
            assert!(response.contains("Authorization Successful"));
        });

        let mut stdin = std::io::Cursor::new(Vec::<u8>::new());
        let mut stdout = Vec::new();
        let mut stderr = Vec::new();
        let mut io = crate::CliIo::new(&mut stdin, &mut stdout, &mut stderr);
        let code = complete_codex_browser_loopback(
            listener,
            client,
            session,
            &mut io,
            &store,
            Duration::from_secs(5),
            super::AuthLoginUi::Plain,
        )
        .await;

        callback_sender.await.expect("callback task");
        assert_eq!(
            code,
            0,
            "stdout:\n{}\nstderr:\n{}",
            String::from_utf8_lossy(&stdout),
            String::from_utf8_lossy(&stderr)
        );
        let stored = store
            .load(AuthProviderId::Codex)
            .expect("load credential")
            .expect("stored credential");
        assert_eq!(
            stored.access_token.as_deref(),
            Some("browser-access-secret")
        );
        assert_eq!(
            stored.refresh_token.as_deref(),
            Some("browser-refresh-secret")
        );
        assert!(
            stored.expires_at.is_some(),
            "CLI Codex OAuth storage must preserve token expiry"
        );
        let requests = http.requests();
        assert_eq!(requests.len(), 1);
        assert_eq!(requests[0].method, AuthHttpMethod::Post);
        assert_eq!(requests[0].url, "https://issuer.test/oauth/token");
        assert!(requests[0].body.contains("grant_type=authorization_code"));
        assert!(requests[0].body.contains("code=browser-code-123"));
        assert!(requests[0]
            .body
            .contains("code_verifier=browser-verifier-123"));
        let stdout = String::from_utf8_lossy(&stdout);
        assert!(stdout.contains("Harness Codex browser login"));
        assert!(stdout.contains("Waiting for callback"));
        assert!(!stdout.contains("browser-access-secret"));
        assert!(!String::from_utf8_lossy(&stderr).contains("browser-access-secret"));
    }
}
