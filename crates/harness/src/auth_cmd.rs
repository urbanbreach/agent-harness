use std::path::PathBuf;

use clap::{Args, Parser, Subcommand};
#[cfg(test)]
use harness_core::auth::codex::AuthHttpClient as CodexAuthHttpClient;
use harness_core::auth::copilot::CopilotDeployment;
use harness_core::auth::plugin::{AuthMethodSpec, AuthPluginRegistry, PromptField};
use harness_core::auth::{
    AuthProviderId, CredentialClock, CredentialStore, StoredCredential, SystemCredentialClock,
};
use harness_core::provider_catalog::ProviderCatalog;

use crate::{CliDeps, CliIo};

#[path = "auth_cmd/login.rs"]
mod login;
#[path = "auth_cmd/prompt_ui.rs"]
mod prompt_ui;
#[path = "auth_cmd/support.rs"]
mod support;

#[cfg(test)]
use self::login::{complete_codex_pasted_callback, handle_codex_loopback_stream};
use self::login::{
    run_codex_browser_login, run_device_login, store_api_key_login,
    store_interactive_api_key_login, store_mock_oauth_login,
};
use self::prompt_ui::{
    auth_prompt_io_error, clack_intro, clack_outro, prompt_auth_provider, prompt_input,
    prompt_login_method, run_prompts, AuthInteractiveError,
};
pub(crate) use self::support::auth_statuses;
use self::support::{
    credential_store_from_deps, credential_store_or_error, load_optional_config, non_empty,
    resolve_login_provider_arg, resolve_provider_arg,
};

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
pub(super) enum AuthLoginUi {
    Plain,
    Interactive,
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

    match store.delete(&auth_provider) {
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
    let is_builtin = auth_provider == AuthProviderId::codex()
        || auth_provider == AuthProviderId::github_copilot();

    if method == AuthLoginMethod::ApiKey {
        if auth_provider == AuthProviderId::codex() {
            if ui == AuthLoginUi::Interactive {
                return store_interactive_api_key_login(auth_provider, io, store);
            }
            return store_api_key_login(auth_provider, command, io, store);
        }
        if auth_provider == AuthProviderId::github_copilot() {
            if ui == AuthLoginUi::Interactive {
                return store_interactive_api_key_login(auth_provider, io, store);
            }
            return store_api_key_login(auth_provider, command, io, store);
        }
        return store_generic_api_key_login(auth_provider, command, io, store, ui);
    }

    if !is_builtin {
        let _ = writeln!(
            io.stderr,
            "auth login failed: only api-key login is supported for {auth_provider}"
        );
        return 2;
    }

    if auth_provider == AuthProviderId::github_copilot() && method != AuthLoginMethod::Device {
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
        AuthLoginMethod::ApiKey => std::process::abort(),
    }
}

fn map_method_spec_to_login_method(spec: &AuthMethodSpec) -> AuthLoginMethod {
    match spec {
        AuthMethodSpec::OAuthAuto { .. } => AuthLoginMethod::Browser,
        AuthMethodSpec::OAuthCode { .. } => AuthLoginMethod::Device,
        AuthMethodSpec::ApiKey { .. } => AuthLoginMethod::ApiKey,
        AuthMethodSpec::Prompts { .. } => AuthLoginMethod::Device,
    }
}

fn store_generic_api_key_login(
    auth_provider: AuthProviderId,
    command: AuthLoginCommand,
    io: &mut CliIo<'_>,
    store: &CredentialStore,
    ui: AuthLoginUi,
) -> i32 {
    if ui == AuthLoginUi::Interactive {
        let api_key = match prompt_input(io, "Enter your API key", None, true) {
            Ok(Some(api_key)) => api_key,
            Ok(None) => return 1,
            Err(err) => return auth_prompt_io_error(err, io.stderr),
        };
        return store_api_key_credential(auth_provider, api_key, io, store, ui);
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
    store_api_key_credential(auth_provider, api_key, io, store, ui)
}

fn store_api_key_credential(
    auth_provider: AuthProviderId,
    api_key: String,
    io: &mut CliIo<'_>,
    store: &CredentialStore,
    ui: AuthLoginUi,
) -> i32 {
    let credential = StoredCredential::api_key(
        auth_provider.clone(),
        api_key,
        SystemCredentialClock.now_rfc3339(),
    );
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

fn execute_interactive_login(
    command: AuthLoginCommand,
    io: &mut CliIo<'_>,
    store: &CredentialStore,
) -> i32 {
    let _ = clack_intro(io.stdout, "Add credential");

    let catalog = match ProviderCatalog::from_env() {
        Ok(catalog) => catalog,
        Err(err) => {
            let _ = writeln!(
                io.stderr,
                "auth login failed: failed to load provider catalog: {err}"
            );
            return 1;
        }
    };
    let registry = AuthPluginRegistry::with_builtins();

    let auth_provider = match prompt_auth_provider(&catalog, &registry, io) {
        Ok(Some(auth_provider)) => auth_provider,
        Ok(None) => return 1,
        Err(err) => return auth_prompt_io_error(err, io.stderr),
    };

    let methods: &[AuthMethodSpec] = registry
        .get(&auth_provider)
        .map(|p| p.auth_methods())
        .unwrap_or(&[]);

    let method = match command.method {
        Some(method) => method,
        None => {
            if methods.is_empty() {
                AuthLoginMethod::ApiKey
            } else if methods.len() == 1 {
                map_method_spec_to_login_method(&methods[0])
            } else {
                match prompt_login_method(methods, io) {
                    Ok(Some(index)) => map_method_spec_to_login_method(&methods[index]),
                    Ok(None) => return 1,
                    Err(err) => return auth_prompt_io_error(err, io.stderr),
                }
            }
        }
    };

    let enterprise_url = if command.enterprise_url.is_some() {
        command.enterprise_url.clone()
    } else {
        let prompts: &[PromptField] = registry
            .get(&auth_provider)
            .and_then(|p| {
                p.auth_methods().iter().find_map(|m| {
                    if let AuthMethodSpec::Prompts { prompts, .. } = m {
                        Some(prompts.as_slice())
                    } else {
                        None
                    }
                })
            })
            .unwrap_or(&[]);

        if prompts.is_empty() {
            None
        } else {
            match run_prompts(prompts, io) {
                Ok(values) => {
                    if let Some(url) = values.get("enterprise_url") {
                        match CopilotDeployment::enterprise(url) {
                            Ok(CopilotDeployment::Enterprise { domain }) => Some(domain),
                            Ok(CopilotDeployment::Public) => None,
                            Err(err) => {
                                let _ = writeln!(io.stderr, "auth login failed: {err}");
                                return 2;
                            }
                        }
                    } else {
                        None
                    }
                }
                Err(AuthInteractiveError::Cancelled) => return 1,
                Err(AuthInteractiveError::Io(err)) => return auth_prompt_io_error(err, io.stderr),
            }
        }
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

#[cfg(test)]
#[path = "auth_cmd/tests.rs"]
mod tests;
