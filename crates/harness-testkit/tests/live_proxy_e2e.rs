use std::cmp;
use std::env;
use std::fs;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::mpsc::{self, Receiver, RecvTimeoutError};
use std::thread;
use std::time::{Duration, Instant};

use portable_pty::{native_pty_system, CommandBuilder, PtySize};
use serde_json::{json, Value};
use vt100::Parser as VtParser;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

const DEFAULT_LIVE_PROXY_PROVIDER: &str = "default";
const DEFAULT_LIVE_PROXY_PROFILE: &str = "live_proxy_smoke";
const DEFAULT_LIVE_PROXY_PROMPT: &str = "Say hello in exactly five words.";
const DEFAULT_LIVE_PROXY_WAIT_TIMEOUT_MS: &str = "120000";
const RESPONSES_ENDPOINT_PATH: &str = "/v1/responses";
const LIVE_TUI_READY_MARKER: &str = "Composer";
const LIVE_TUI_READ_POLL_TIMEOUT: Duration = Duration::from_millis(50);
const LIVE_TUI_STABLE_WINDOW: Duration = Duration::from_millis(180);
const LIVE_TUI_STABLE_TIMEOUT: Duration = Duration::from_secs(2);
const LIVE_TUI_STARTUP_TIMEOUT: Duration = Duration::from_secs(10);

#[derive(Debug, Clone, PartialEq, Eq)]
struct LivePromptRequest {
    source_config_path: PathBuf,
    provider_name: String,
    model_override: Option<String>,
    profile: String,
    prompt_text: String,
    wait_timeout_ms: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum LiveSmokeEndpoint {
    Responses,
}

impl LiveSmokeEndpoint {
    const fn path(self) -> &'static str {
        match self {
            Self::Responses => RESPONSES_ENDPOINT_PATH,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct PromptRunConfig {
    config_path: PathBuf,
    profile: String,
    model_id: String,
    endpoint: LiveSmokeEndpoint,
}

#[derive(Debug, Default)]
struct ProviderTurnEvidence {
    request_id: Option<String>,
    saw_started: bool,
    saw_finished: bool,
    delta_count: usize,
    task_completed_summary: Option<String>,
    run_failed: Option<String>,
}

#[test]
#[ignore = "requires HARNESS_LIVE_PROXY=1 and local CLIproxyAPI access"]
fn live_proxy_prompt_responses_smoke() {
    if env::var("HARNESS_LIVE_PROXY").as_deref() != Ok("1") {
        return;
    }

    let repo_root = repo_root();
    let live_request = resolve_live_prompt_request(&repo_root)
        .unwrap_or_else(|err| panic!("failed to resolve live proxy prompt inputs: {err}"));

    let run_config = prepare_live_prompt_run_config(&live_request)
        .unwrap_or_else(|err| panic!("failed to prepare live proxy prompt config: {err}"));

    let harness_bin = resolve_harness_bin();
    let events_path = unique_temp_file("live-proxy-events", "jsonl");

    let output = Command::new(&harness_bin)
        .arg("prompt")
        .arg("--text")
        .arg(&live_request.prompt_text)
        .arg("--profile")
        .arg(&run_config.profile)
        .arg("--config")
        .arg(&run_config.config_path)
        .arg("--out")
        .arg(&events_path)
        .env(
            "HARNESS_PROMPT_WAIT_TIMEOUT_MS",
            &live_request.wait_timeout_ms,
        )
        .current_dir(&repo_root)
        .output()
        .expect("spawn harness prompt");

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);

    assert!(
        output.status.success(),
        "harness prompt failed with status {:?}\nstdout:\n{}\nstderr:\n{}\nPrepared config: {}\nSelected profile: {}\nSelected model: {}\nSelected endpoint: {}\nHint: ensure CLIproxyAPI is running and reachable, HARNESS_LIVE_PROXY_MODEL (if set) is valid, and provider api_mode is responses or auto",
        output.status.code(),
        stdout,
        stderr,
        run_config.config_path.display(),
        run_config.profile,
        run_config.model_id,
        run_config.endpoint.path()
    );

    let events_body = fs::read_to_string(&events_path)
        .unwrap_or_else(|err| panic!("failed to read event log {}: {err}", events_path.display()));
    assert_events_show_successful_provider_turn(&events_body);
}

#[test]
#[ignore = "requires HARNESS_LIVE_PROXY=1 and local CLIproxyAPI access"]
fn live_proxy_e2e_tui_prompt_responses_smoke() {
    if !cfg!(target_os = "linux") || env::var("HARNESS_LIVE_PROXY").as_deref() != Ok("1") {
        return;
    }

    let repo_root = repo_root();
    let live_request = resolve_live_prompt_request(&repo_root)
        .unwrap_or_else(|err| panic!("failed to resolve live proxy TUI inputs: {err}"));

    let run_config = prepare_live_prompt_run_config(&live_request)
        .unwrap_or_else(|err| panic!("failed to prepare live proxy TUI config: {err}"));

    let events_body = run_live_tui_smoke(
        &live_request,
        &run_config,
        live_tui_command_timeout(&live_request),
    )
    .unwrap_or_else(|err| panic!("live proxy TUI smoke failed: {err}"));
    assert_events_show_successful_provider_turn(&events_body);
}

#[tokio::test(flavor = "current_thread")]
async fn live_proxy_prompt_wiremock_smoke_uses_responses_and_model_override() {
    let server = MockServer::start().await;
    let response_template = ResponseTemplate::new(200)
        .insert_header("content-type", "text/event-stream")
        .set_body_raw(deterministic_responses_sse_fixture(), "text/event-stream");

    Mock::given(method("POST"))
        .and(path(RESPONSES_ENDPOINT_PATH))
        .respond_with(response_template.clone())
        .mount(&server)
        .await;

    Mock::given(method("POST"))
        .and(path("/v1/chat/completions"))
        .respond_with(ResponseTemplate::new(500))
        .mount(&server)
        .await;

    let provider_name = "proxy";
    let configured_model = "configured-model";
    let overridden_model = "wiremock-model-override";
    let session_dir = unique_temp_dir("live-proxy-wiremock-session");
    let source_config_path = unique_temp_file("live-proxy-wiremock-config", "jsonc");
    let source_config = build_live_proxy_test_config(
        provider_name,
        &server.uri(),
        "auto",
        configured_model,
        &session_dir,
    );
    fs::write(
        &source_config_path,
        serde_json::to_string_pretty(&source_config).expect("serialize wiremock config"),
    )
    .expect("write wiremock config");

    let run_config = prepare_prompt_run_config(
        &source_config_path,
        provider_name,
        Some(overridden_model),
        "wiremock_live_profile",
    )
    .expect("prepare prompt run config");

    let repo_root = repo_root();
    let harness_bin = resolve_harness_bin();
    let events_path = unique_temp_file("live-proxy-wiremock-events", "jsonl");

    let harness_bin_for_run = harness_bin.clone();
    let repo_root_for_run = repo_root.clone();
    let events_path_for_run = events_path.clone();
    let run_config_for_run = run_config.clone();
    let output = tokio::task::spawn_blocking(move || {
        Command::new(&harness_bin_for_run)
            .arg("prompt")
            .arg("--text")
            .arg("Return hello from wiremock")
            .arg("--profile")
            .arg(&run_config_for_run.profile)
            .arg("--config")
            .arg(&run_config_for_run.config_path)
            .arg("--out")
            .arg(&events_path_for_run)
            .env(
                "HARNESS_PROMPT_WAIT_TIMEOUT_MS",
                DEFAULT_LIVE_PROXY_WAIT_TIMEOUT_MS,
            )
            .current_dir(&repo_root_for_run)
            .output()
            .expect("spawn harness prompt")
    })
    .await
    .expect("join blocking harness run");

    assert!(
        output.status.success(),
        "wiremock harness prompt failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let events_body = fs::read_to_string(&events_path)
        .unwrap_or_else(|err| panic!("failed reading {}: {err}", events_path.display()));
    assert_events_show_successful_provider_turn(&events_body);

    let requests = server
        .received_requests()
        .await
        .expect("request recording must be enabled");
    let responses_request = requests
        .iter()
        .find(|request| request.url.path() == run_config.endpoint.path())
        .unwrap_or_else(|| {
            panic!(
                "expected at least one {} request",
                run_config.endpoint.path()
            )
        });
    assert!(
        !requests
            .iter()
            .any(|request| request.url.path() == "/v1/chat/completions"),
        "did not expect /v1/chat/completions fallback"
    );

    let request_body: Value = responses_request
        .body_json()
        .expect("responses request body must be JSON");
    assert_eq!(
        request_body.get("model"),
        Some(&Value::String(overridden_model.to_string()))
    );
}

#[test]
fn prepare_prompt_run_config_rejects_chat_completions_mode() {
    let source_config_path = unique_temp_file("live-proxy-chat-mode", "jsonc");
    let session_dir = unique_temp_dir("live-proxy-chat-session");
    let source_config = build_live_proxy_test_config(
        "default",
        "http://127.0.0.1:9999",
        "chat_completions",
        "chat-model",
        &session_dir,
    );
    fs::write(
        &source_config_path,
        serde_json::to_string_pretty(&source_config).expect("serialize chat mode config"),
    )
    .expect("write chat mode config");

    let err = prepare_prompt_run_config(
        &source_config_path,
        "default",
        Some("chat-model"),
        "chat_profile",
    )
    .expect_err("chat_completions mode should be rejected for live CLI proxy test");

    assert!(
        err.contains("responses or auto"),
        "unexpected error message: {err}"
    );
}

#[test]
fn live_tui_smoke_helpers_reuse_cliproxy_config_and_endpoint_rules() {
    let repo_root = repo_root();
    let default_config = resolve_live_proxy_config_path(&repo_root, None)
        .expect("resolve default live proxy config");
    assert!(
        default_config.ends_with(Path::new("configs").join("harness.example.jsonc")),
        "unexpected default live config path: {}",
        default_config.display()
    );

    let session_dir = unique_temp_dir("live-smoke-helper-session");
    let auto_config_path = unique_temp_file("live-smoke-helper-config", "jsonc");
    let auto_config = build_live_proxy_test_config(
        "proxy",
        "http://127.0.0.1:8317",
        "auto",
        "configured-model",
        &session_dir,
    );
    fs::write(
        &auto_config_path,
        serde_json::to_string_pretty(&auto_config).expect("serialize auto config"),
    )
    .expect("write auto config");

    let live_request = LivePromptRequest {
        source_config_path: auto_config_path.clone(),
        provider_name: "proxy".to_string(),
        model_override: Some(" override-model ".to_string()),
        profile: "tui_smoke_profile".to_string(),
        prompt_text: DEFAULT_LIVE_PROXY_PROMPT.to_string(),
        wait_timeout_ms: "1500".to_string(),
    };

    let run_config = prepare_live_prompt_run_config(&live_request)
        .expect("prepare auto-mode live TUI run config");
    assert_eq!(run_config.endpoint.path(), RESPONSES_ENDPOINT_PATH);
    assert_eq!(run_config.model_id, "override-model");
    assert_eq!(run_config.profile, "tui_smoke_profile");
    assert_eq!(
        live_tui_command_timeout(&live_request),
        Duration::from_millis(1500).saturating_add(Duration::from_secs(20))
    );

    let prepared_config = load_json5_config(&run_config.config_path).expect("load prepared config");
    let default_provider = provider_from_config(&prepared_config, DEFAULT_LIVE_PROXY_PROVIDER)
        .expect("default provider should be rewritten into prepared config");
    assert_eq!(provider_api_mode(default_provider), "auto");

    let categories = prepared_config
        .get("categories")
        .and_then(Value::as_object)
        .expect("prepared config categories object");
    assert_eq!(
        categories
            .get("deep")
            .and_then(Value::as_object)
            .and_then(|category| category.get("model_ref"))
            .and_then(Value::as_str),
        Some("default:configured-model")
    );
    assert_eq!(
        categories
            .get("tui_smoke_profile")
            .and_then(Value::as_object)
            .and_then(|category| category.get("model_ref"))
            .and_then(Value::as_str),
        Some("default:override-model")
    );

    let missing_path = unique_temp_file("live-smoke-helper-missing", "jsonc");
    let missing_err = resolve_live_proxy_config_path(&repo_root, Some(&missing_path))
        .expect_err("missing live config path should fail fast");
    assert!(
        missing_err.contains("live proxy config not found at"),
        "unexpected missing-config error: {missing_err}"
    );

    let chat_config_path = unique_temp_file("live-smoke-helper-chat", "jsonc");
    let chat_config = build_live_proxy_test_config(
        "default",
        "http://127.0.0.1:9999",
        "chat_completions",
        "chat-model",
        &session_dir,
    );
    fs::write(
        &chat_config_path,
        serde_json::to_string_pretty(&chat_config).expect("serialize chat config"),
    )
    .expect("write chat config");
    let chat_err = prepare_prompt_run_config(&chat_config_path, "default", None, "chat_profile")
        .expect_err("chat-completions mode should be rejected");
    assert!(
        chat_err.contains("responses or auto"),
        "unexpected chat-mode error: {chat_err}"
    );
}

fn prepare_live_prompt_run_config(request: &LivePromptRequest) -> Result<PromptRunConfig, String> {
    prepare_prompt_run_config(
        &request.source_config_path,
        &request.provider_name,
        request.model_override.as_deref(),
        &request.profile,
    )
}

fn resolve_live_prompt_request(repo_root: &Path) -> Result<LivePromptRequest, String> {
    let override_config_path = env::var("HARNESS_LIVE_PROXY_CONFIG")
        .ok()
        .map(PathBuf::from);
    let source_config_path =
        resolve_live_proxy_config_path(repo_root, override_config_path.as_deref())?;

    Ok(LivePromptRequest {
        source_config_path,
        provider_name: env::var("HARNESS_LIVE_PROXY_PROVIDER")
            .unwrap_or_else(|_| DEFAULT_LIVE_PROXY_PROVIDER.into()),
        model_override: env::var("HARNESS_LIVE_PROXY_MODEL")
            .ok()
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty()),
        profile: env::var("HARNESS_LIVE_PROXY_PROFILE")
            .unwrap_or_else(|_| DEFAULT_LIVE_PROXY_PROFILE.into()),
        prompt_text: env::var("HARNESS_LIVE_PROXY_PROMPT")
            .unwrap_or_else(|_| DEFAULT_LIVE_PROXY_PROMPT.into()),
        wait_timeout_ms: env::var("HARNESS_LIVE_PROXY_WAIT_TIMEOUT_MS")
            .unwrap_or_else(|_| DEFAULT_LIVE_PROXY_WAIT_TIMEOUT_MS.to_string()),
    })
}

fn resolve_live_proxy_config_path(
    repo_root: &Path,
    override_path: Option<&Path>,
) -> Result<PathBuf, String> {
    let config_path = override_path
        .map(Path::to_path_buf)
        .unwrap_or_else(|| repo_root.join("configs").join("harness.example.jsonc"));
    if config_path.exists() {
        Ok(config_path)
    } else {
        Err(format!(
            "live proxy config not found at {}",
            config_path.display()
        ))
    }
}

fn prepare_prompt_run_config(
    source_config_path: &Path,
    provider_name: &str,
    model_override: Option<&str>,
    profile_name: &str,
) -> Result<PromptRunConfig, String> {
    if provider_name.trim().is_empty() {
        return Err("provider name cannot be empty".to_string());
    }
    if profile_name.trim().is_empty() {
        return Err("profile name cannot be empty".to_string());
    }

    let mut config = load_json5_config(source_config_path)?;

    let provider = provider_from_config(&config, provider_name)?;
    let endpoint = resolve_live_smoke_endpoint(provider)?;

    let selected_model = if let Some(model) = model_override {
        let trimmed = model.trim();
        if trimmed.is_empty() {
            first_model_from_provider(provider)?
        } else {
            trimmed.to_string()
        }
    } else {
        first_model_from_provider(provider)?
    };

    rewrite_selected_provider_to_default(&mut config, provider_name)?;
    normalize_category_model_refs_to_default(&mut config)?;
    ensure_profile_model_ref(&mut config, profile_name, &selected_model)?;

    let prepared_config_path = unique_temp_file("live-proxy-prepared-config", "jsonc");
    let rendered = serde_json::to_string_pretty(&config)
        .map_err(|err| format!("failed to render prepared config JSON: {err}"))?;
    fs::write(&prepared_config_path, rendered).map_err(|err| {
        format!(
            "failed to write prepared config {}: {err}",
            prepared_config_path.display()
        )
    })?;

    Ok(PromptRunConfig {
        config_path: prepared_config_path,
        profile: profile_name.to_string(),
        model_id: selected_model,
        endpoint,
    })
}

fn run_live_tui_smoke(
    request: &LivePromptRequest,
    run_config: &PromptRunConfig,
    timeout: Duration,
) -> Result<String, String> {
    let harness_bin = resolve_harness_bin();
    let repo_root = repo_root();
    let session_dir = unique_temp_dir("live-proxy-tui-session");

    let pty_system = native_pty_system();
    let pair = pty_system
        .openpty(tui_pty_size())
        .map_err(|err| format!("failed to open TUI PTY: {err}"))?;

    let mut command = CommandBuilder::new(harness_bin.to_string_lossy().as_ref());
    command.arg("tui");
    command.arg("--config");
    command.arg(run_config.config_path.to_string_lossy().to_string());
    command.arg("--profile");
    command.arg(run_config.profile.clone());
    command.arg("--session-dir");
    command.arg(session_dir.to_string_lossy().to_string());
    command.arg("--deterministic");
    command.cwd(repo_root);
    configure_deterministic_tui_env(&mut command);

    let mut child = pair
        .slave
        .spawn_command(command)
        .map_err(|err| format!("failed to spawn harness TUI smoke: {err}"))?;
    drop(pair.slave);

    let reader = pair
        .master
        .try_clone_reader()
        .map_err(|err| format!("failed to clone TUI PTY reader: {err}"))?;
    let mut writer = pair
        .master
        .take_writer()
        .map_err(|err| format!("failed to take TUI PTY writer: {err}"))?;
    let output_rx = spawn_pty_reader_thread(reader);
    let mut parser = VtParser::new(tui_pty_size().rows, tui_pty_size().cols, 0);

    wait_for_screen_contains(
        &mut parser,
        &output_rx,
        LIVE_TUI_READY_MARKER,
        LIVE_TUI_STARTUP_TIMEOUT,
    )?;

    writer
        .write_all(request.prompt_text.as_bytes())
        .map_err(|err| format!("failed to type live TUI smoke prompt: {err}"))?;
    writer
        .flush()
        .map_err(|err| format!("failed to flush live TUI smoke prompt: {err}"))?;
    wait_for_screen_contains(
        &mut parser,
        &output_rx,
        &request.prompt_text,
        Duration::from_secs(5),
    )?;

    writer
        .write_all(b"\r")
        .map_err(|err| format!("failed to submit live TUI smoke prompt: {err}"))?;
    writer
        .flush()
        .map_err(|err| format!("failed to flush submitted live TUI smoke prompt: {err}"))?;

    let events_body = wait_for_tui_provider_turn(&session_dir, timeout)?;

    writer
        .write_all(b"q")
        .map_err(|err| format!("failed to quit live TUI smoke cleanly: {err}"))?;
    writer
        .flush()
        .map_err(|err| format!("failed to flush live TUI smoke quit key: {err}"))?;

    wait_for_tui_process_exit(&mut child, &output_rx, &mut parser, Duration::from_secs(10))?;

    Ok(events_body)
}

fn live_tui_command_timeout(request: &LivePromptRequest) -> Duration {
    let wait_timeout_ms = request
        .wait_timeout_ms
        .trim()
        .parse::<u64>()
        .unwrap_or_else(|_| {
            DEFAULT_LIVE_PROXY_WAIT_TIMEOUT_MS
                .parse::<u64>()
                .expect("default live proxy wait timeout must parse as u64")
        });
    Duration::from_millis(wait_timeout_ms).saturating_add(Duration::from_secs(20))
}

fn tui_pty_size() -> PtySize {
    PtySize {
        rows: 24,
        cols: 80,
        pixel_width: 0,
        pixel_height: 0,
    }
}

fn configure_deterministic_tui_env(command: &mut CommandBuilder) {
    command.env("HARNESS_DETERMINISTIC", "1");
    command.env("HARNESS_DISABLE_ANIMATIONS", "1");
    command.env("HARNESS_SEED", "42");
    command.env("TERM", "xterm-256color");
    command.env("LANG", "C.UTF-8");
    command.env("LC_ALL", "C.UTF-8");
    command.env("TZ", "UTC");
}

fn spawn_pty_reader_thread(mut reader: Box<dyn Read + Send>) -> Receiver<Vec<u8>> {
    let (tx, rx) = mpsc::channel();
    thread::spawn(move || {
        let mut buffer = [0_u8; 4096];
        loop {
            match reader.read(&mut buffer) {
                Ok(0) => break,
                Ok(read) => {
                    if tx.send(buffer[..read].to_vec()).is_err() {
                        break;
                    }
                }
                Err(err) if err.kind() == std::io::ErrorKind::Interrupted => continue,
                Err(_) => break,
            }
        }
    });
    rx
}

fn wait_for_screen_contains(
    parser: &mut VtParser,
    output_rx: &Receiver<Vec<u8>>,
    needle: &str,
    timeout: Duration,
) -> Result<String, String> {
    let deadline = Instant::now() + timeout;

    loop {
        drain_pty_output(parser, output_rx);

        let current = tui_screen_contents(parser);
        if current.contains(needle) {
            return Ok(stabilize_tui_screen(parser, output_rx, current));
        }

        let now = Instant::now();
        if now >= deadline {
            return Err(format!(
                "timed out waiting for TUI screen marker `{needle}` after {timeout:?}; final screen:\n{current}"
            ));
        }

        let wait_timeout = cmp::min(
            LIVE_TUI_READ_POLL_TIMEOUT,
            deadline.saturating_duration_since(now),
        );
        match output_rx.recv_timeout(wait_timeout) {
            Ok(chunk) => parser.process(&chunk),
            Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => {
                return Err(format!(
                    "TUI PTY output closed while waiting for `{needle}`; last screen:\n{current}"
                ));
            }
        }
    }
}

fn stabilize_tui_screen(
    parser: &mut VtParser,
    output_rx: &Receiver<Vec<u8>>,
    initial: String,
) -> String {
    let mut latest = initial;
    let mut stable_since = Instant::now();
    let deadline = Instant::now() + LIVE_TUI_STABLE_TIMEOUT;

    loop {
        let now = Instant::now();
        if now >= deadline {
            return latest;
        }

        let wait_timeout = cmp::min(
            LIVE_TUI_READ_POLL_TIMEOUT,
            deadline.saturating_duration_since(now),
        );
        match output_rx.recv_timeout(wait_timeout) {
            Ok(chunk) => parser.process(&chunk),
            Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => return latest,
        }

        let current = tui_screen_contents(parser);
        if current != latest {
            latest = current;
            stable_since = Instant::now();
            continue;
        }

        if Instant::now().saturating_duration_since(stable_since) >= LIVE_TUI_STABLE_WINDOW {
            return latest;
        }
    }
}

fn drain_pty_output(parser: &mut VtParser, output_rx: &Receiver<Vec<u8>>) {
    while let Ok(chunk) = output_rx.try_recv() {
        parser.process(&chunk);
    }
}

fn tui_screen_contents(parser: &VtParser) -> String {
    parser.screen().contents()
}

fn wait_for_tui_process_exit(
    child: &mut Box<dyn portable_pty::Child + Send + Sync>,
    output_rx: &Receiver<Vec<u8>>,
    parser: &mut VtParser,
    timeout: Duration,
) -> Result<(), String> {
    let deadline = Instant::now() + timeout;

    loop {
        drain_pty_output(parser, output_rx);

        match child
            .try_wait()
            .map_err(|err| format!("failed to poll live TUI smoke process: {err}"))?
        {
            Some(status) if status.success() => return Ok(()),
            Some(status) => {
                return Err(format!(
                    "live TUI smoke exited with status {:?}; final screen:\n{}",
                    status.exit_code(),
                    tui_screen_contents(parser)
                ));
            }
            None => {}
        }

        let now = Instant::now();
        if now >= deadline {
            child
                .kill()
                .map_err(|err| format!("failed to kill timed out live TUI smoke process: {err}"))?;
            return Err(format!(
                "timed out waiting for live TUI smoke to exit after {timeout:?}; final screen:\n{}",
                tui_screen_contents(parser)
            ));
        }

        let wait_timeout = cmp::min(
            LIVE_TUI_READ_POLL_TIMEOUT,
            deadline.saturating_duration_since(now),
        );
        match output_rx.recv_timeout(wait_timeout) {
            Ok(chunk) => parser.process(&chunk),
            Err(RecvTimeoutError::Timeout | RecvTimeoutError::Disconnected) => {}
        }
    }
}

fn wait_for_tui_provider_turn(session_dir: &Path, timeout: Duration) -> Result<String, String> {
    let deadline = Instant::now() + timeout;

    loop {
        if let Ok(run_dir) = resolve_single_run_dir(session_dir) {
            let events_path = run_dir.join("events.jsonl");
            if events_path.exists() {
                let events_body = fs::read_to_string(&events_path).map_err(|err| {
                    format!(
                        "failed to read TUI smoke events {}: {err}",
                        events_path.display()
                    )
                })?;
                let evidence = collect_provider_turn_evidence(&events_body);
                if let Some(run_failed) = evidence.run_failed.as_deref() {
                    return Err(format!(
                        "live TUI smoke run failed before provider completion: {run_failed}"
                    ));
                }
                if provider_turn_completed(&evidence) {
                    return Ok(events_body);
                }
            }
        }

        if Instant::now() >= deadline {
            return Err(format!(
                "timed out waiting for provider turn evidence under {} after {timeout:?}",
                session_dir.display()
            ));
        }

        thread::sleep(LIVE_TUI_READ_POLL_TIMEOUT);
    }
}

fn resolve_single_run_dir(session_dir: &Path) -> Result<PathBuf, String> {
    let mut run_dirs = fs::read_dir(session_dir)
        .map_err(|err| {
            format!(
                "failed to read TUI smoke session dir {}: {err}",
                session_dir.display()
            )
        })?
        .filter_map(|entry| entry.ok().map(|entry| entry.path()))
        .filter(|path| path.join("events.jsonl").exists())
        .collect::<Vec<_>>();

    match run_dirs.len() {
        1 => Ok(run_dirs.remove(0)),
        0 => Err(format!(
            "expected one run dir with events.jsonl under {}; found none",
            session_dir.display()
        )),
        count => Err(format!(
            "expected one run dir with events.jsonl under {}; found {count}",
            session_dir.display()
        )),
    }
}

fn assert_events_show_successful_provider_turn(events_body: &str) {
    let evidence = collect_provider_turn_evidence(events_body);

    assert!(
        evidence.run_failed.is_none(),
        "run failed before provider completion: {}",
        evidence
            .run_failed
            .unwrap_or_else(|| "unknown run failure".to_string())
    );
    assert!(
        evidence.saw_started,
        "expected provider_request_started event"
    );
    assert!(
        evidence.saw_finished,
        "expected provider_request_finished event"
    );

    assert!(
        provider_turn_completed(&evidence),
        "expected either provider_stream_delta events or a non-empty task_completed summary for the provider request"
    );
}

fn provider_turn_completed(evidence: &ProviderTurnEvidence) -> bool {
    evidence.delta_count > 0
        || evidence
            .task_completed_summary
            .as_deref()
            .map(str::trim)
            .map(|text| !text.is_empty())
            .unwrap_or(false)
}

fn collect_provider_turn_evidence(events_body: &str) -> ProviderTurnEvidence {
    let mut evidence = ProviderTurnEvidence::default();

    for (idx, line) in events_body.lines().enumerate() {
        let event: Value = serde_json::from_str(line).unwrap_or_else(|err| {
            panic!("events line {} is invalid JSON: {err}", idx + 1);
        });

        let event_type = event
            .get("payload")
            .and_then(|payload| payload.get("event_type"))
            .and_then(Value::as_str)
            .unwrap_or_default();

        let data = event
            .get("payload")
            .and_then(|payload| payload.get("data"))
            .cloned()
            .unwrap_or(Value::Null);

        match event_type {
            "provider_request_started" => {
                if evidence.request_id.is_none() {
                    evidence.request_id = data
                        .get("request_id")
                        .and_then(Value::as_str)
                        .map(ToOwned::to_owned);
                }
                if evidence.request_id.is_some() {
                    evidence.saw_started = true;
                }
            }
            "provider_stream_delta" => {
                if same_request(&evidence.request_id, &data) {
                    evidence.delta_count += 1;
                }
            }
            "provider_request_finished" => {
                if same_request(&evidence.request_id, &data) {
                    evidence.saw_finished = true;
                }
            }
            "task_completed" => {
                let Some(request_id) = evidence.request_id.as_deref() else {
                    continue;
                };

                let is_matching_request = data
                    .get("task_id")
                    .and_then(Value::as_str)
                    .map(|task_id| task_id == request_id)
                    .unwrap_or(false);

                if is_matching_request {
                    evidence.task_completed_summary = data
                        .get("result_summary")
                        .and_then(Value::as_str)
                        .map(ToOwned::to_owned);
                }
            }
            "run_failed" => {
                evidence.run_failed = data
                    .get("error")
                    .and_then(Value::as_str)
                    .map(ToOwned::to_owned)
                    .or_else(|| Some("run_failed event missing error detail".to_string()));
            }
            _ => {}
        }
    }

    evidence
}

fn load_json5_config(config_path: &Path) -> Result<Value, String> {
    let raw = fs::read_to_string(config_path)
        .map_err(|err| format!("failed to read config {}: {err}", config_path.display()))?;
    json5::from_str(&raw).map_err(|err| {
        format!(
            "failed to parse JSON5 config {}: {err}",
            config_path.display()
        )
    })
}

fn provider_from_config<'a>(config: &'a Value, provider_name: &str) -> Result<&'a Value, String> {
    let providers = config
        .get("providers")
        .and_then(Value::as_object)
        .ok_or_else(|| "config must define providers as an object".to_string())?;

    providers
        .get(provider_name)
        .ok_or_else(|| format!("provider `{provider_name}` is missing from config.providers"))
}

fn provider_api_mode(provider: &Value) -> String {
    provider
        .get("api_mode")
        .or_else(|| provider.get("apiMode"))
        .and_then(Value::as_str)
        .unwrap_or("auto")
        .trim()
        .to_ascii_lowercase()
}

fn resolve_live_smoke_endpoint(provider: &Value) -> Result<LiveSmokeEndpoint, String> {
    let api_mode = provider_api_mode(provider);
    ensure_provider_uses_responses_compatible_mode(&api_mode)?;
    Ok(LiveSmokeEndpoint::Responses)
}

fn ensure_provider_uses_responses_compatible_mode(api_mode: &str) -> Result<(), String> {
    match api_mode {
        "responses" | "auto" => Ok(()),
        "chat_completions" => Err(
            "live CLI proxy E2E requires provider api_mode set to responses or auto; found chat_completions"
                .to_string(),
        ),
        other => Err(format!(
            "unsupported api_mode `{other}` for live CLI proxy E2E; expected responses or auto"
        )),
    }
}

fn first_model_from_provider(provider: &Value) -> Result<String, String> {
    let Some(models) = provider.get("models").and_then(Value::as_object) else {
        return Err(
            "provider config has no `models` object; set HARNESS_LIVE_PROXY_MODEL explicitly"
                .to_string(),
        );
    };

    models.keys().next().cloned().ok_or_else(|| {
        "provider config has an empty `models` map; set HARNESS_LIVE_PROXY_MODEL".to_string()
    })
}

fn rewrite_selected_provider_to_default(
    config: &mut Value,
    provider_name: &str,
) -> Result<(), String> {
    let root = config
        .as_object_mut()
        .ok_or_else(|| "config root must be a JSON object".to_string())?;
    let providers = root
        .get_mut("providers")
        .and_then(Value::as_object_mut)
        .ok_or_else(|| "config.providers must be an object".to_string())?;

    let selected_provider = providers
        .get(provider_name)
        .cloned()
        .ok_or_else(|| format!("provider `{provider_name}` is missing from config.providers"))?;

    providers.insert(DEFAULT_LIVE_PROXY_PROVIDER.to_string(), selected_provider);
    Ok(())
}

fn normalize_category_model_refs_to_default(config: &mut Value) -> Result<(), String> {
    let root = config
        .as_object_mut()
        .ok_or_else(|| "config root must be a JSON object".to_string())?;
    let categories = root
        .get_mut("categories")
        .and_then(Value::as_object_mut)
        .ok_or_else(|| "config.categories must be an object".to_string())?;

    for (category_name, category_value) in categories.iter_mut() {
        let Some(category_obj) = category_value.as_object_mut() else {
            return Err(format!("category `{category_name}` must be an object"));
        };

        let model_ref = category_obj
            .get("model_ref")
            .or_else(|| category_obj.get("modelRef"))
            .and_then(Value::as_str)
            .map(str::trim)
            .unwrap_or_default();
        if model_ref.is_empty() {
            continue;
        }

        let model_id = model_ref
            .split_once(':')
            .map(|(_, model_id)| model_id)
            .unwrap_or(model_ref)
            .trim();
        if model_id.is_empty() {
            continue;
        }

        category_obj.insert(
            "model_ref".to_string(),
            Value::String(format!("default:{model_id}")),
        );
    }

    Ok(())
}

fn ensure_profile_model_ref(
    config: &mut Value,
    profile_name: &str,
    model_id: &str,
) -> Result<(), String> {
    let root = config
        .as_object_mut()
        .ok_or_else(|| "config root must be a JSON object".to_string())?;

    let categories = root
        .entry("categories".to_string())
        .or_insert_with(|| Value::Object(serde_json::Map::new()))
        .as_object_mut()
        .ok_or_else(|| "config.categories must be an object".to_string())?;

    let mut profile = categories.get(profile_name).cloned().unwrap_or_else(|| {
        json!({
            "description": "Live proxy smoke profile",
            "tools": []
        })
    });

    let profile_obj = profile
        .as_object_mut()
        .ok_or_else(|| format!("category `{profile_name}` must be an object"))?;
    profile_obj.insert(
        "model_ref".to_string(),
        Value::String(format!("default:{model_id}")),
    );
    profile_obj
        .entry("description".to_string())
        .or_insert_with(|| Value::String("Live proxy smoke profile".to_string()));
    profile_obj
        .entry("tools".to_string())
        .or_insert_with(|| Value::Array(Vec::new()));

    categories.insert(profile_name.to_string(), profile);
    Ok(())
}

fn build_live_proxy_test_config(
    provider_name: &str,
    provider_base_uri: &str,
    api_mode: &str,
    configured_model: &str,
    session_dir: &Path,
) -> Value {
    let mut providers = serde_json::Map::new();
    providers.insert(
        provider_name.to_string(),
        json!({
            "type": "openai_compatible",
            "base_url": format!("{provider_base_uri}/v1"),
            "api_key": "test-key",
            "api_mode": api_mode,
            "timeout_ms": 60000,
            "models": {
                configured_model: {
                    "display_name": "Configured model"
                }
            }
        }),
    );

    let mut categories = serde_json::Map::new();
    categories.insert(
        "deep".to_string(),
        json!({
            "description": "Deep profile",
            "model_ref": format!("{provider_name}:{configured_model}"),
            "tools": []
        }),
    );

    json!({
        "backgroundTask": {
            "defaultConcurrency": 2,
            "providerConcurrency": 2,
            "modelConcurrency": 2,
            "staleTimeoutMs": 30000,
            "messageStalenessTimeoutMs": 10000
        },
        "providers": providers,
        "categories": categories,
        "permissions": {
            "edit": "allow",
            "shell": "allow",
            "network": "allow"
        },
        "paths": {
            "session_dir": session_dir.display().to_string()
        },
        "ui": {
            "default_profile": "deep"
        }
    })
}

fn deterministic_responses_sse_fixture() -> String {
    concat!(
        "event: response.created\n",
        "data: {\"type\":\"response.created\",\"response\":{\"id\":\"resp_123\"}}\n\n",
        "event: response.output_text.delta\n",
        "data: {\"type\":\"response.output_text.delta\",\"delta\":\"Hello\"}\n\n",
        "event: response.output_text.delta\n",
        "data: {\"type\":\"response.output_text.delta\",\"delta\":\" world\"}\n\n",
        "event: response.completed\n",
        "data: {\"type\":\"response.completed\",\"response\":{\"usage\":{\"prompt_tokens\":4,\"completion_tokens\":2,\"total_tokens\":6}}}\n\n",
        "data: [DONE]\n\n"
    )
    .to_string()
}

fn same_request(request_id: &Option<String>, data: &Value) -> bool {
    let Some(expected) = request_id else {
        return false;
    };
    data.get("request_id")
        .and_then(Value::as_str)
        .map(|current| current == expected)
        .unwrap_or(false)
}

fn resolve_harness_bin() -> PathBuf {
    if let Ok(path) = env::var("HARNESS_BIN") {
        let harness_bin = PathBuf::from(path);
        assert!(
            harness_bin.exists(),
            "HARNESS_BIN points to missing path: {}",
            harness_bin.display()
        );
        return harness_bin;
    }

    let repo = repo_root();
    let status = Command::new("cargo")
        .arg("build")
        .arg("-p")
        .arg("harness")
        .current_dir(&repo)
        .status()
        .expect("spawn cargo build -p harness");
    assert!(
        status.success(),
        "cargo build -p harness failed with status {status}"
    );

    let harness_bin = repo
        .join("target")
        .join("debug")
        .join(binary_name("harness"));
    assert!(
        harness_bin.exists(),
        "expected harness binary at {}",
        harness_bin.display()
    );
    harness_bin
}

fn repo_root() -> PathBuf {
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    manifest_dir
        .parent()
        .and_then(Path::parent)
        .map(Path::to_path_buf)
        .expect("harness-testkit should live under <repo>/crates/harness-testkit")
}

fn unique_temp_file(prefix: &str, ext: &str) -> PathBuf {
    let base = env::temp_dir().join("harness-testkit");
    fs::create_dir_all(&base).expect("create base temp dir");

    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("system clock before unix epoch")
        .as_nanos();

    base.join(format!("{prefix}-{}-{nanos}.{ext}", std::process::id()))
}

fn unique_temp_dir(prefix: &str) -> PathBuf {
    let base = env::temp_dir().join("harness-testkit");
    fs::create_dir_all(&base).expect("create base temp dir");

    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("system clock before unix epoch")
        .as_nanos();

    let dir = base.join(format!("{prefix}-{}-{nanos}", std::process::id()));
    fs::create_dir_all(&dir)
        .unwrap_or_else(|err| panic!("failed creating temp dir {}: {err}", dir.display()));
    dir
}

#[cfg(target_os = "windows")]
fn binary_name(name: &str) -> String {
    format!("{name}.exe")
}

#[cfg(not(target_os = "windows"))]
fn binary_name(name: &str) -> String {
    name.to_string()
}
