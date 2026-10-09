// allow: SIZE_OK — CLI TUI workflow (launch + lineage + auth)
use std::io::Write;
use std::path::PathBuf;

use harness_core::redact::{DefaultRedactor, Redactor};
use harness_tui::app::LaunchMetadata;
use harness_tui::{LiveUpdate, LiveUpdateSender, OperatorNoticeLevel};

use super::live_settings::{resolve_live_settings_with_deps, LiveSettings, LiveSettingsDeps};
use super::TuiCommand;

#[derive(Clone)]
pub(super) struct TuiAuthBackendContext {
    pub(super) config_path: Option<PathBuf>,
    pub(super) session_dir: Option<PathBuf>,
    pub(super) workspace_root: PathBuf,
    pub(super) prompt_overrides: std::collections::BTreeMap<String, String>,
    /// Providers that run every turn through the installed Claude Code.
    pub(super) claude_code_providers: std::collections::BTreeSet<String>,
    /// The config every live session clones; a login adds its provider to the shared router.
    pub(super) providers: Option<super::coordinator_warmup::LiveCoordinatorConfigWarmup>,
    /// What the next session starts with; a login refreshes its model catalog.
    pub(super) launch_selection: Option<super::workflow::LaunchSelection>,
}

impl TuiAuthBackendContext {
    pub(super) fn from_settings(settings: &LiveSettings) -> Self {
        Self {
            config_path: settings.config_path.clone(),
            session_dir: Some(settings.session_dir.clone()),
            workspace_root: settings.workspace_root.clone(),
            prompt_overrides: settings
                .config
                .iter()
                .flat_map(|config| &config.agents)
                .filter_map(|(name, profile)| {
                    profile
                        .system_prompt
                        .as_deref()
                        .filter(|prompt| !prompt.trim().is_empty())
                        .map(|prompt| (name.clone(), prompt.to_string()))
                })
                .collect(),
            claude_code_providers: settings
                .config
                .iter()
                .flat_map(|config| &config.providers)
                .filter(|(_, provider)| {
                    matches!(
                        provider,
                        harness_core::config::ProviderConfig::AnthropicSubscription(_)
                    )
                })
                .map(|(name, _)| name.clone())
                .chain([crate::runtime_catalog::BUILTIN_ANTHROPIC_SUBSCRIPTION_PROVIDER_ID.into()])
                .collect(),
            providers: None,
            launch_selection: None,
        }
    }

    pub(super) fn with_login_refresh(
        mut self,
        providers: super::coordinator_warmup::LiveCoordinatorConfigWarmup,
        launch_selection: super::workflow::LaunchSelection,
    ) -> Self {
        self.providers = Some(providers);
        self.launch_selection = Some(launch_selection);
        self
    }

    pub(super) fn model_prompt_notice(&self, metadata: &LaunchMetadata) -> Option<LiveUpdate> {
        let target = super::launch_metadata::launch_metadata_model_target(metadata)?;
        if self.claude_code_providers.contains(&target.provider) {
            let environment: std::collections::BTreeMap<String, String> =
                std::env::vars().collect();
            if harness_providers::anthropic_subscription::executable::describe_claude_code_executable(
                &|name| environment.get(name).cloned(),
            )
            .is_err()
            {
                return Some(LiveUpdate::OperatorNotice {
                    message: format!(
                        "Claude Code is not installed; {} needs it for every turn. {}",
                        target.provider,
                        harness_providers::anthropic_subscription::executable::INSTALL_GUIDANCE
                    ),
                    level: OperatorNoticeLevel::Error,
                });
            }
        }
        let prompt = if self.prompt_overrides.contains_key(metadata.profile()) {
            "configured override"
        } else {
            &target.resolution.prompt_preset
        };
        Some(LiveUpdate::ModelPromptNotice(format!(
            "Selected prompt: {prompt}"
        )))
    }
}

/// The newest login's input channel, tagged with the login that owns it: a login that
/// outlives its dialog must never clear or receive a newer login's input.
type LoginInput = Option<(u64, std::sync::mpsc::Sender<String>)>;
static AUTH_BACKEND_INPUT: std::sync::Mutex<LoginInput> = std::sync::Mutex::new(None);
static AUTH_BACKEND_LOGINS: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

/// A line for the running login, or `None` to cancel it (the channel closes).
pub(super) fn send_tui_auth_backend_input(line: Option<String>) {
    let Ok(mut slot) = AUTH_BACKEND_INPUT.lock() else {
        return;
    };
    match line {
        Some(line) => {
            if let Some((_, input)) = slot.as_ref() {
                let _ = input.send(line);
            }
        }
        None => *slot = None,
    }
}

pub(super) fn spawn_tui_auth_backend_task(
    args: Vec<String>,
    stdin: Option<String>,
    context: TuiAuthBackendContext,
    live_update_tx: LiveUpdateSender,
) {
    let TuiAuthBackendContext {
        config_path,
        session_dir,
        workspace_root,
        providers,
        launch_selection,
        ..
    } = context;
    let runtime = tokio::runtime::Handle::try_current().ok();
    let normalized_args = normalize_tui_auth_args(args.clone());
    let display = display_tui_auth_args(&normalized_args);
    let _ = live_update_tx.send(LiveUpdate::OperatorNotice {
        message: format!("auth backend running: harness auth {display}"),
        level: OperatorNoticeLevel::Info,
    });
    let (input, lines) = std::sync::mpsc::channel();
    let login = AUTH_BACKEND_LOGINS.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
    let interactive = normalized_args.first().map(String::as_str) == Some("login");
    if interactive && let Ok(mut slot) = AUTH_BACKEND_INPUT.lock() {
        *slot = Some((login, input));
    }
    std::thread::spawn(move || {
        let mut deps = harness::CliDeps::real().with_current_dir(workspace_root.clone());
        if interactive {
            deps = deps.with_interactive_input(lines);
        }
        let (message, level, success) = run_tui_auth_backend_streaming_with_deps(
            args,
            config_path.clone(),
            session_dir.clone(),
            &stdin.unwrap_or_default(),
            &deps,
            Some(live_update_tx.clone()),
        );
        if let Ok(mut slot) = AUTH_BACKEND_INPUT.lock()
            && slot.as_ref().is_some_and(|(owner, _)| *owner == login)
        {
            *slot = None;
        }
        let _ = live_update_tx.send(LiveUpdate::OperatorNotice {
            message: message.clone(),
            level,
        });
        let _ = live_update_tx.send(LiveUpdate::AuthBackendResult { success, message });
        if success {
            match refreshed_settings_after_auth(
                normalized_args.first().map(String::as_str),
                config_path,
                session_dir,
                workspace_root,
                &deps,
                launch_selection.as_ref(),
            ) {
                Ok(Some(settings)) => {
                    if let (Some(providers), Some(runtime)) = (&providers, &runtime)
                        && let Err(err) =
                            runtime.block_on(providers.add_signed_in_providers(&settings))
                    {
                        let _ = live_update_tx.send(LiveUpdate::OperatorNotice {
                            message: format!("signed-in provider unavailable until restart: {err}"),
                            level: OperatorNoticeLevel::Error,
                        });
                    }
                    let _ = live_update_tx.send(LiveUpdate::AuthProviderCatalogRefreshed {
                        launch_metadata: Box::new(settings.launch_metadata),
                    });
                    let _ = live_update_tx.send(LiveUpdate::OperatorNotice {
                        message: "provider catalog refreshed; choose a model with /model"
                            .to_string(),
                        level: OperatorNoticeLevel::Info,
                    });
                }
                Ok(None) => {}
                Err(err) => {
                    let _ = live_update_tx.send(LiveUpdate::OperatorNotice {
                        message: format!("provider catalog refresh skipped: {err}"),
                        level: OperatorNoticeLevel::Error,
                    });
                }
            }
        }
    });
}

pub(super) fn refreshed_settings_after_auth(
    command: Option<&str>,
    config_path: Option<PathBuf>,
    session_dir: Option<PathBuf>,
    workspace_root: PathBuf,
    deps: &harness::CliDeps,
    launch_selection: Option<&super::workflow::LaunchSelection>,
) -> Result<Option<LiveSettings>, String> {
    if command != Some("login") {
        return Ok(None);
    }
    let store = harness_core::auth::CredentialStore::from_lookup(&|name| deps.env_var_value(name));
    resolve_live_settings_with_deps(
        &TuiCommand {
            replay: None,
            continue_session: None,
            scenario: None,
            mock: false,
            yolo: false,
            deterministic: false,
            session_dir: None,
            exit_on_finish: false,
            profile: None,
            no_alt_screen: false,
            minimal: false,
            fullscreen: false,
        },
        config_path,
        session_dir,
        workspace_root.clone(),
        &deps.config_load_context().with_current_dir(workspace_root),
        LiveSettingsDeps {
            credential_store: store.as_ref(),
            env_lookup: &|name| deps.env_var_value(name),
            model_selection_path: None,
        },
    )
    .map(|settings| {
        if let Some(selection) = launch_selection {
            let mut selection = super::recover_mutex_lock(selection);
            selection.metadata = settings.launch_metadata.clone().without_mode_label();
            selection.config_digest.clone_from(&settings.config_digest);
        }
        Some(settings)
    })
}

#[cfg(test)]
pub(super) fn run_tui_auth_backend_once_with_deps(
    args: Vec<String>,
    config_path: Option<PathBuf>,
    session_dir: Option<PathBuf>,
    deps: &harness::CliDeps,
) -> (String, OperatorNoticeLevel) {
    let (message, level, _) =
        run_tui_auth_backend_streaming_with_deps(args, config_path, session_dir, "", deps, None);
    (message, level)
}

pub(super) fn run_tui_auth_backend_streaming_with_deps(
    args: Vec<String>,
    config_path: Option<PathBuf>,
    session_dir: Option<PathBuf>,
    stdin: &str,
    deps: &harness::CliDeps,
    live_update_tx: Option<LiveUpdateSender>,
) -> (String, OperatorNoticeLevel, bool) {
    let args = normalize_tui_auth_args(args);
    let mut stdin = std::io::Cursor::new(stdin.as_bytes().to_vec());
    let mut stdout = TuiAuthNoticeWriter::new(
        live_update_tx.clone(),
        OperatorNoticeLevel::Info,
        "auth backend output",
    );
    let mut stderr = TuiAuthNoticeWriter::new(
        live_update_tx,
        OperatorNoticeLevel::Error,
        "auth backend error",
    );
    let mut io = harness::CliIo::new(&mut stdin, &mut stdout, &mut stderr);
    let code =
        harness::execute_auth_backend_args_with_io(&args, config_path, session_dir, &mut io, deps);
    stdout.flush_pending();
    stderr.flush_pending();
    let output = harness::AuthBackendOutput {
        code,
        stdout: stdout.captured(),
        stderr: stderr.captured(),
    };
    let level = if output.code == 0 {
        OperatorNoticeLevel::Info
    } else {
        OperatorNoticeLevel::Error
    };
    (
        format_tui_auth_backend_output(&args, &output),
        level,
        output.code == 0,
    )
}

struct TuiAuthNoticeWriter {
    live_update_tx: Option<LiveUpdateSender>,
    level: OperatorNoticeLevel,
    prefix: &'static str,
    redactor: DefaultRedactor,
    pending: String,
    captured: String,
}

impl TuiAuthNoticeWriter {
    fn new(
        live_update_tx: Option<LiveUpdateSender>,
        level: OperatorNoticeLevel,
        prefix: &'static str,
    ) -> Self {
        Self {
            live_update_tx,
            level,
            prefix,
            redactor: DefaultRedactor::default(),
            pending: String::new(),
            captured: String::new(),
        }
    }

    fn captured(&self) -> String {
        self.captured.clone()
    }

    fn flush_pending(&mut self) {
        if self.pending.is_empty() {
            return;
        }
        let line = std::mem::take(&mut self.pending);
        self.emit_line(&line);
    }

    fn emit_line(&mut self, raw_line: &str) {
        let redacted = compact_auth_backend_text(&self.redactor.redact_text(raw_line));
        if redacted.is_empty() {
            return;
        }
        self.captured.push_str(&redacted);
        self.captured.push('\n');
        if let Some(tx) = &self.live_update_tx {
            let _ = tx.send(LiveUpdate::OperatorNotice {
                message: format!("{}: {redacted}", self.prefix),
                level: self.level,
            });
        }
    }
}

impl Write for TuiAuthNoticeWriter {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        let text = String::from_utf8_lossy(buf);
        self.pending.push_str(&text);
        while let Some(newline_index) = self.pending.find('\n') {
            let line = self.pending[..newline_index]
                .trim_end_matches('\r')
                .to_string();
            self.pending.drain(..=newline_index);
            self.emit_line(&line);
        }
        Ok(buf.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        self.flush_pending();
        Ok(())
    }
}

pub(super) fn normalize_tui_auth_args(args: Vec<String>) -> Vec<String> {
    if args.is_empty() {
        vec!["list".to_string()]
    } else {
        args
    }
}

pub(super) fn display_tui_auth_args(args: &[String]) -> String {
    let mut display = Vec::with_capacity(args.len());
    let mut redact_next = false;
    for arg in args {
        if redact_next {
            display.push("<redacted>".to_string());
            redact_next = false;
            continue;
        }
        if tui_auth_arg_redacts_next(arg) {
            display.push(arg.clone());
            redact_next = true;
            continue;
        }
        if let Some(redacted) = redact_tui_auth_arg_value(arg) {
            display.push(redacted);
            continue;
        }
        display.push(arg.clone());
    }
    display.join(" ")
}

fn tui_auth_arg_redacts_next(arg: &str) -> bool {
    matches!(
        arg,
        "--mock-token" | "--mock-refresh-token" | "--enterprise-url"
    )
}

fn redact_tui_auth_arg_value(arg: &str) -> Option<String> {
    [
        "--mock-token=",
        "--mock-refresh-token=",
        "--enterprise-url=",
    ]
    .into_iter()
    .find_map(|prefix| {
        arg.strip_prefix(prefix)
            .map(|_| format!("{prefix}<redacted>"))
    })
}

fn format_tui_auth_backend_output(args: &[String], output: &harness::AuthBackendOutput) -> String {
    let command = display_tui_auth_args(args);
    let stdout = compact_auth_backend_text(&output.stdout);
    let stderr = compact_auth_backend_text(&output.stderr);
    match (output.code, stdout.is_empty(), stderr.is_empty()) {
        (0, false, true) => format!("auth backend completed: harness auth {command}\n{stdout}"),
        (0, true, false) => format!("auth backend completed: harness auth {command}\n{stderr}"),
        (0, false, false) => {
            format!("auth backend completed: harness auth {command}\n{stdout}\n{stderr}")
        }
        (0, true, true) => format!("auth backend completed: harness auth {command}"),
        (_, false, true) => {
            format!(
                "auth backend failed (exit {}): harness auth {command}\n{stdout}",
                output.code
            )
        }
        (_, true, false) => {
            format!(
                "auth backend failed (exit {}): harness auth {command}\n{stderr}",
                output.code
            )
        }
        (_, false, false) => {
            format!(
                "auth backend failed (exit {}): harness auth {command}\n{stdout}\n{stderr}",
                output.code
            )
        }
        (_, true, true) => {
            format!(
                "auth backend failed (exit {}): harness auth {command}\n\
                 no diagnostic output; verify the Codex OAuth callback at \
                 localhost:1455 is reachable and not already in use",
                output.code,
            )
        }
    }
}

fn compact_auth_backend_text(text: &str) -> String {
    const MAX_AUTH_NOTICE_CHARS: usize = 1600;
    let compact = text
        .lines()
        .map(str::trim_end)
        .filter(|line| !line.is_empty())
        .collect::<Vec<_>>()
        .join("\n");
    if compact.chars().count() <= MAX_AUTH_NOTICE_CHARS {
        compact
    } else {
        let mut truncated = compact
            .chars()
            .take(MAX_AUTH_NOTICE_CHARS)
            .collect::<String>();
        truncated.push_str("\n… truncated");
        truncated
    }
}
