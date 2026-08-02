// allow: SIZE_OK — CLI TUI workflow (launch + lineage + auth)
use std::io::Write;
use std::path::PathBuf;
use std::sync::mpsc as std_mpsc;

use harness_core::redact::{DefaultRedactor, Redactor};
use harness_tui::app::LaunchMetadata;
use harness_tui::{LiveUpdate, OperatorNoticeLevel};

use super::live_settings::{resolve_live_settings, LiveSettings};
use super::TuiCommand;

#[derive(Clone)]
pub(super) struct TuiAuthBackendContext {
    pub(super) config_path: Option<PathBuf>,
    pub(super) session_dir: Option<PathBuf>,
    pub(super) workspace_root: PathBuf,
    pub(super) config_digest: String,
}

impl TuiAuthBackendContext {
    pub(super) fn from_settings(settings: &LiveSettings) -> Self {
        Self {
            config_path: settings.config_path.clone(),
            session_dir: Some(settings.session_dir.clone()),
            workspace_root: settings.workspace_root.clone(),
            config_digest: settings.config_digest.clone(),
        }
    }
}

pub(super) fn spawn_tui_auth_backend_task(
    args: Vec<String>,
    stdin: Option<String>,
    config_path: Option<PathBuf>,
    session_dir: Option<PathBuf>,
    workspace_root: PathBuf,
    live_update_tx: std_mpsc::Sender<LiveUpdate>,
) {
    let normalized_args = normalize_tui_auth_args(args.clone());
    let display = display_tui_auth_args(&normalized_args);
    let _ = live_update_tx.send(LiveUpdate::OperatorNotice {
        message: format!("auth backend running: harness auth {display}"),
        level: OperatorNoticeLevel::Info,
    });
    std::thread::spawn(move || {
        let (message, level, success) = run_tui_auth_backend_once(
            args,
            config_path.clone(),
            session_dir.clone(),
            workspace_root.clone(),
            stdin.unwrap_or_default(),
            Some(live_update_tx.clone()),
        );
        let _ = live_update_tx.send(LiveUpdate::OperatorNotice { message, level });
        let _ = live_update_tx.send(LiveUpdate::AuthBackendResult { success });
        if success {
            match refreshed_launch_metadata_after_auth(
                normalized_args.first().map(String::as_str),
                config_path,
                session_dir,
                workspace_root,
            ) {
                Ok(Some(launch_metadata)) => {
                    let _ = live_update_tx.send(LiveUpdate::AuthProviderCatalogRefreshed {
                        launch_metadata: Box::new(launch_metadata),
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

pub(super) fn refreshed_launch_metadata_after_auth(
    command: Option<&str>,
    config_path: Option<PathBuf>,
    session_dir: Option<PathBuf>,
    workspace_root: PathBuf,
) -> Result<Option<LaunchMetadata>, String> {
    if command != Some("login") {
        return Ok(None);
    }
    let settings = resolve_live_settings(
        &TuiCommand {
            replay: None,
            continue_session: None,
            scenario: None,
            mock: false,
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
        &harness_core::config::ConfigLoadContext::from_env().with_current_dir(workspace_root),
    )?;
    Ok(Some(settings.launch_metadata))
}

pub(super) fn run_tui_auth_backend_once(
    args: Vec<String>,
    config_path: Option<PathBuf>,
    session_dir: Option<PathBuf>,
    workspace_root: PathBuf,
    stdin: String,
    live_update_tx: Option<std_mpsc::Sender<LiveUpdate>>,
) -> (String, OperatorNoticeLevel, bool) {
    let deps = harness::CliDeps::real().with_current_dir(workspace_root);
    run_tui_auth_backend_streaming_with_deps(
        args,
        config_path,
        session_dir,
        &stdin,
        &deps,
        live_update_tx,
    )
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
    live_update_tx: Option<std_mpsc::Sender<LiveUpdate>>,
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
    live_update_tx: Option<std_mpsc::Sender<LiveUpdate>>,
    level: OperatorNoticeLevel,
    prefix: &'static str,
    redactor: DefaultRedactor,
    pending: String,
    captured: String,
}

impl TuiAuthNoticeWriter {
    fn new(
        live_update_tx: Option<std_mpsc::Sender<LiveUpdate>>,
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
                "auth backend failed (exit {}): harness auth {command}",
                output.code
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
