use harness_core::event::{
    ActorKind, EventActor, EventEnvelopeV1, EventV1, PermissionDecision, PermissionRequestedEvent,
    ProviderRequestFinishedEvent, ProviderRequestStartedEvent, ProviderStreamDeltaEvent,
    UserMessageSubmittedEvent, SCHEMA_VERSION,
};
use harness_tui::{run_tui_with_options, LiveUpdate, TuiMode, TuiOptions, UiIntent};
use portable_pty::{native_pty_system, CommandBuilder, PtySize};
use std::cmp;
use std::fs;
use std::io::{Read, Write};
use std::path::PathBuf;
use std::sync::{
    mpsc::{self, Receiver, RecvTimeoutError, Sender},
    Arc, Mutex,
};
use std::thread;
use std::time::{Duration, Instant};
use vt100::Parser;

const STARTUP_TIMEOUT: Duration = Duration::from_secs(10);
const MARKER_TIMEOUT: Duration = Duration::from_secs(12);
const READ_POLL_TIMEOUT: Duration = Duration::from_millis(50);
const STABLE_WINDOW: Duration = Duration::from_millis(180);
const STABLE_TIMEOUT: Duration = Duration::from_secs(2);
const PRESERVED_DRAFT_TEXT: &str = "keep this draft";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct PtyGeometry {
    cols: u16,
    rows: u16,
}

impl PtyGeometry {
    const MINIMUM_SIGNOFF: Self = Self { cols: 80, rows: 24 };
    const PRIMARY_SIGNOFF: Self = Self {
        cols: 100,
        rows: 30,
    };

    fn pty_size(self) -> PtySize {
        PtySize {
            rows: self.rows,
            cols: self.cols,
            pixel_width: 0,
            pixel_height: 0,
        }
    }

    fn parser(self) -> Parser {
        Parser::new(self.rows, self.cols, 0)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct PromptFixture {
    ready_marker: &'static str,
}

impl PromptFixture {
    const LIVE_COMPOSER: Self = Self {
        ready_marker: "Composer",
    };
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct DraftFixture {
    text: &'static str,
    response_marker: &'static str,
    submit_key: u8,
}

impl DraftFixture {
    const HELLO_WORLD: Self = Self {
        text: "Hello from PTY",
        response_marker: "Hello world",
        submit_key: b'\r',
    };

    fn write(self, writer: &mut dyn Write) -> std::io::Result<()> {
        writer.write_all(self.text.as_bytes())?;
        writer.flush()
    }

    fn submit(self, writer: &mut dyn Write) -> std::io::Result<()> {
        send_key(writer, self.submit_key)
    }

    fn write_and_submit(self, writer: &mut dyn Write) -> std::io::Result<()> {
        self.write(writer)?;
        self.submit(writer)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct PermissionFixture {
    marker: &'static str,
}

impl PermissionFixture {
    const TOOL_CALL: Self = Self {
        marker: "Permission Requested",
    };
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct LiveStateFixtures {
    prompt: PromptFixture,
    draft: DraftFixture,
    permission: PermissionFixture,
}

const LIVE_STATE_FIXTURES: LiveStateFixtures = LiveStateFixtures {
    prompt: PromptFixture::LIVE_COMPOSER,
    draft: DraftFixture::HELLO_WORLD,
    permission: PermissionFixture::TOOL_CALL,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum HelperScenario {
    TypeFirstStartup,
    StreamedResponse,
    PermissionWithDraft,
    DetailsDrawer,
    DegradedBootstrap,
    DisconnectedStream,
}

impl HelperScenario {
    fn env_value(self) -> &'static str {
        match self {
            Self::TypeFirstStartup => "type_first_startup",
            Self::StreamedResponse => "streamed_response",
            Self::PermissionWithDraft => "permission_with_draft",
            Self::DetailsDrawer => "details_drawer",
            Self::DegradedBootstrap => "degraded_bootstrap",
            Self::DisconnectedStream => "disconnected_stream",
        }
    }

    fn helper_test_name(self) -> &'static str {
        match self {
            Self::TypeFirstStartup => "pty_helper_type_first_startup",
            Self::StreamedResponse => "pty_helper_streamed_response",
            Self::PermissionWithDraft => "pty_helper_permission_with_draft",
            Self::DetailsDrawer => "pty_helper_details_drawer",
            Self::DegradedBootstrap => "pty_helper_degraded_bootstrap",
            Self::DisconnectedStream => "pty_helper_disconnected_stream",
        }
    }
}

#[test]
fn pty_e2e_snapshots_are_stable() {
    if !cfg!(target_os = "linux") {
        return;
    }

    let type_first_startup = capture_type_first_startup_snapshot(PtyGeometry::PRIMARY_SIGNOFF);
    assert_or_update_snapshot("type_first_startup", &type_first_startup);

    let streamed_response = capture_streamed_response_snapshot(PtyGeometry::PRIMARY_SIGNOFF);
    assert_or_update_snapshot("streamed_response", &streamed_response);

    let permission_with_draft =
        capture_permission_with_draft_snapshot(PtyGeometry::PRIMARY_SIGNOFF);
    assert_or_update_snapshot("permission_with_draft", &permission_with_draft);

    let narrow_80x24 = capture_type_first_startup_snapshot(PtyGeometry::MINIMUM_SIGNOFF);
    assert_or_update_snapshot("narrow_80x24", &narrow_80x24);

    let degraded_bootstrap = capture_helper_screen_snapshot(
        HelperScenario::DegradedBootstrap,
        PtyGeometry::MINIMUM_SIGNOFF,
        "Degraded",
    );
    assert_or_update_snapshot("degraded_bootstrap", &degraded_bootstrap);

    let disconnected_stream = capture_helper_screen_snapshot(
        HelperScenario::DisconnectedStream,
        PtyGeometry::MINIMUM_SIGNOFF,
        "Disconnected",
    );
    assert_or_update_snapshot("disconnected_stream", &disconnected_stream);

    assert_snapshot_secrets_clean();
}

#[test]
fn snapshot_files_exist_and_are_secret_clean() {
    let snapshot_dir = snapshot_dir();
    let expected = [
        snapshot_dir.join("type_first_startup.snap"),
        snapshot_dir.join("streamed_response.snap"),
        snapshot_dir.join("permission_with_draft.snap"),
        snapshot_dir.join("narrow_80x24.snap"),
        snapshot_dir.join("degraded_bootstrap.snap"),
        snapshot_dir.join("disconnected_stream.snap"),
    ];

    for path in expected {
        assert!(path.exists(), "missing snapshot file: {}", path.display());
    }

    assert_snapshot_secrets_clean();
}

#[test]
fn pty_helpers_support_primary_and_minimum_geometries() {
    for (geometry, expected_cols, expected_rows) in [
        (PtyGeometry::MINIMUM_SIGNOFF, 80, 24),
        (PtyGeometry::PRIMARY_SIGNOFF, 100, 30),
    ] {
        let size = geometry.pty_size();
        assert_eq!(size.cols, expected_cols);
        assert_eq!(size.rows, expected_rows);
        assert_eq!(
            geometry.parser().screen().size(),
            (expected_rows, expected_cols)
        );
    }

    let mut draft_bytes = Vec::new();
    LIVE_STATE_FIXTURES
        .draft
        .write_and_submit(&mut draft_bytes)
        .expect("serialize reusable draft fixture");
    assert_eq!(draft_bytes, b"Hello from PTY\r");
}

#[test]
fn pty_helper_type_first_startup() {
    run_helper_if_requested(HelperScenario::TypeFirstStartup);
}

#[test]
fn pty_helper_streamed_response() {
    run_helper_if_requested(HelperScenario::StreamedResponse);
}

#[test]
fn pty_helper_permission_with_draft() {
    run_helper_if_requested(HelperScenario::PermissionWithDraft);
}

#[test]
fn pty_helper_details_drawer() {
    run_helper_if_requested(HelperScenario::DetailsDrawer);
}

#[test]
fn pty_helper_degraded_bootstrap() {
    run_helper_if_requested(HelperScenario::DegradedBootstrap);
}

#[test]
fn pty_helper_disconnected_stream() {
    run_helper_if_requested(HelperScenario::DisconnectedStream);
}

#[test]
fn pty_live_details_drawer_remains_reachable() {
    if !cfg!(target_os = "linux") {
        return;
    }

    let mut helper = spawn_helper_pty(HelperScenario::DetailsDrawer, PtyGeometry::PRIMARY_SIGNOFF);
    wait_for_screen_contains(
        &mut helper.parser,
        &helper.output_rx,
        LIVE_STATE_FIXTURES.prompt.ready_marker,
        STARTUP_TIMEOUT,
    )
    .expect("wait for startup before details drawer flow");

    send_key(helper.writer.as_mut(), b'\t').expect("focus transcript before opening details");
    send_key(helper.writer.as_mut(), b'i').expect("open details drawer");

    let screen = wait_for_screen_contains(
        &mut helper.parser,
        &helper.output_rx,
        "Request ID:",
        MARKER_TIMEOUT,
    )
    .expect("wait for details drawer markers");

    assert!(screen.contains("req_details_drawer"));
    assert!(screen.contains("gpt-5-codex"));

    terminate_child(helper.child);
}

fn run_helper_if_requested(scenario: HelperScenario) {
    if !cfg!(target_os = "linux") {
        return;
    }
    if helper_scenario_from_env() != Some(scenario) {
        return;
    }

    let run_dir = tempfile::tempdir().expect("create temp helper run dir");
    let (tx, rx) = mpsc::channel::<LiveUpdate>();

    let on_ui_intent = match scenario {
        HelperScenario::StreamedResponse => Some(streamed_response_intent_handler(tx.clone())),
        _ => None,
    };

    match scenario {
        HelperScenario::TypeFirstStartup | HelperScenario::StreamedResponse => {}
        HelperScenario::PermissionWithDraft => {
            let permission_tx = tx.clone();
            thread::spawn(move || {
                thread::sleep(Duration::from_millis(250));
                permission_tx
                    .send(LiveUpdate::Event(Box::new(permission_requested_event(
                        1,
                        "perm_pty",
                        "tool_call_pty",
                    ))))
                    .expect("send permission request event");
                thread::park();
            });
        }
        HelperScenario::DetailsDrawer => {
            let details_tx = tx.clone();
            thread::spawn(move || {
                for event in details_drawer_events() {
                    details_tx
                        .send(LiveUpdate::Event(Box::new(event)))
                        .expect("send details drawer events");
                }
                thread::park();
            });
        }
        HelperScenario::DegradedBootstrap => {
            tx.send(LiveUpdate::Status(
                "live stream lagged by 2; replaying from seq 1".to_string(),
            ))
            .expect("send degraded bootstrap status");
        }
        HelperScenario::DisconnectedStream => {
            drop(tx);
            run_tui_with_options(TuiOptions {
                mode: TuiMode::Live {
                    run_dir: run_dir.path().to_path_buf(),
                    update_rx: rx,
                },
                exit_on_finish: false,
                on_ui_intent,
                keybindings: None,
            })
            .expect("run disconnected helper tui");
            return;
        }
    }

    let _keepalive = tx;
    run_tui_with_options(TuiOptions {
        mode: TuiMode::Live {
            run_dir: run_dir.path().to_path_buf(),
            update_rx: rx,
        },
        exit_on_finish: false,
        on_ui_intent,
        keybindings: None,
    })
    .expect("run helper tui");
}

fn streamed_response_intent_handler(tx: Sender<LiveUpdate>) -> Arc<dyn Fn(UiIntent) + Send + Sync> {
    let submitted = Arc::new(Mutex::new(false));
    Arc::new(move |intent: UiIntent| {
        let UiIntent::SubmitPrompt { text } = intent else {
            return;
        };

        let mut submitted = submitted.lock().expect("lock submit guard");
        if *submitted {
            return;
        }
        *submitted = true;
        drop(submitted);

        let tx = tx.clone();
        thread::spawn(move || {
            for event in streamed_response_events(&text) {
                tx.send(LiveUpdate::Event(Box::new(event)))
                    .expect("send helper live event");
                thread::sleep(Duration::from_millis(40));
            }
        });
    })
}

fn streamed_response_events(text: &str) -> Vec<EventEnvelopeV1> {
    let request_id = "req_pty_001";
    vec![
        envelope(
            1,
            Some(request_id),
            EventV1::UserMessageSubmitted(UserMessageSubmittedEvent {
                request_id: request_id.to_string(),
                text: text.to_string(),
            }),
        ),
        envelope(
            2,
            Some(request_id),
            EventV1::ProviderRequestStarted(ProviderRequestStartedEvent {
                request_id: request_id.to_string(),
                provider_id: "mock".to_string(),
                model_id: "model-1".to_string(),
                prompt_summary: text.to_string(),
                request_digest: "digest-req-pty-001".to_string(),
            }),
        ),
        envelope(
            3,
            Some(request_id),
            EventV1::ProviderStreamDelta(ProviderStreamDeltaEvent {
                request_id: request_id.to_string(),
                delta: "Hello".to_string(),
            }),
        ),
        envelope(
            4,
            Some(request_id),
            EventV1::ProviderStreamDelta(ProviderStreamDeltaEvent {
                request_id: request_id.to_string(),
                delta: " world".to_string(),
            }),
        ),
        envelope(
            5,
            Some(request_id),
            EventV1::ProviderRequestFinished(ProviderRequestFinishedEvent {
                request_id: request_id.to_string(),
                finish_reason: "stop".to_string(),
                output_digest: Some("digest-output-pty-001".to_string()),
            }),
        ),
    ]
}

fn details_drawer_events() -> Vec<EventEnvelopeV1> {
    let request_id = "req_details_drawer";
    vec![
        envelope(
            1,
            Some(request_id),
            EventV1::UserMessageSubmitted(UserMessageSubmittedEvent {
                request_id: request_id.to_string(),
                text: "Inspect the details drawer".to_string(),
            }),
        ),
        envelope(
            2,
            Some(request_id),
            EventV1::ProviderRequestStarted(ProviderRequestStartedEvent {
                request_id: request_id.to_string(),
                provider_id: "mock".to_string(),
                model_id: "gpt-5-codex".to_string(),
                prompt_summary: "Inspect the details drawer".to_string(),
                request_digest: "digest-details-drawer".to_string(),
            }),
        ),
    ]
}

fn permission_requested_event(
    seq: u64,
    permission_id: &str,
    tool_call_id: &str,
) -> EventEnvelopeV1 {
    envelope(
        seq,
        Some(tool_call_id),
        EventV1::PermissionRequested(PermissionRequestedEvent {
            permission_id: permission_id.to_string(),
            kind: "edit_fs".to_string(),
            tool_call_id: Some(tool_call_id.to_string()),
            summary: "Apply hashline edit to demo.txt".to_string(),
            request_digest: "digest-perm-pty".to_string(),
            timeout_ms: 30_000,
            default_decision: PermissionDecision::Deny,
        }),
    )
}

fn envelope(seq: u64, correlation_id: Option<&str>, payload: EventV1) -> EventEnvelopeV1 {
    EventEnvelopeV1 {
        schema_version: SCHEMA_VERSION,
        event_id: format!("evt-{seq:04}"),
        seq,
        run_id: "run_fixture".to_string(),
        mono_ms: seq,
        ts: None,
        actor: EventActor::new(ActorKind::System, Some("pty-helper".to_string())),
        correlation_id: correlation_id.map(str::to_string),
        causation_id: None,
        stream_key: Some("run:run_fixture".to_string()),
        payload,
    }
}

fn capture_type_first_startup_snapshot(geometry: PtyGeometry) -> String {
    let mut helper = spawn_helper_pty(HelperScenario::TypeFirstStartup, geometry);
    wait_for_screen_contains(
        &mut helper.parser,
        &helper.output_rx,
        LIVE_STATE_FIXTURES.prompt.ready_marker,
        STARTUP_TIMEOUT,
    )
    .expect("wait for helper startup render");

    LIVE_STATE_FIXTURES
        .draft
        .write(helper.writer.as_mut())
        .expect("type startup draft");

    let screen = wait_for_screen_contains(
        &mut helper.parser,
        &helper.output_rx,
        LIVE_STATE_FIXTURES.draft.text,
        MARKER_TIMEOUT,
    )
    .expect("wait for typed startup draft");

    terminate_child(helper.child);
    normalize_snapshot(&screen)
}

fn capture_streamed_response_snapshot(geometry: PtyGeometry) -> String {
    let mut helper = spawn_helper_pty(HelperScenario::StreamedResponse, geometry);
    wait_for_screen_contains(
        &mut helper.parser,
        &helper.output_rx,
        LIVE_STATE_FIXTURES.prompt.ready_marker,
        STARTUP_TIMEOUT,
    )
    .expect("wait for startup before prompt submit");

    LIVE_STATE_FIXTURES
        .draft
        .write_and_submit(helper.writer.as_mut())
        .expect("submit streamed response draft");

    let screen = wait_for_screen_contains(
        &mut helper.parser,
        &helper.output_rx,
        LIVE_STATE_FIXTURES.draft.response_marker,
        MARKER_TIMEOUT,
    )
    .expect("wait for streamed response marker");

    terminate_child(helper.child);
    normalize_snapshot(&screen)
}

fn capture_permission_with_draft_snapshot(geometry: PtyGeometry) -> String {
    let mut helper = spawn_helper_pty(HelperScenario::PermissionWithDraft, geometry);
    wait_for_screen_contains(
        &mut helper.parser,
        &helper.output_rx,
        LIVE_STATE_FIXTURES.prompt.ready_marker,
        STARTUP_TIMEOUT,
    )
    .expect("wait for startup before permission overlay");

    helper
        .writer
        .write_all(PRESERVED_DRAFT_TEXT.as_bytes())
        .expect("type preserved draft");
    helper.writer.flush().expect("flush preserved draft");

    let screen = wait_for_screen_contains(
        &mut helper.parser,
        &helper.output_rx,
        LIVE_STATE_FIXTURES.permission.marker,
        MARKER_TIMEOUT,
    )
    .expect("wait for permission overlay marker");

    assert!(
        screen.contains(PRESERVED_DRAFT_TEXT),
        "permission snapshot lost draft"
    );
    terminate_child(helper.child);
    normalize_snapshot(&screen)
}

fn capture_helper_screen_snapshot(
    scenario: HelperScenario,
    geometry: PtyGeometry,
    marker: &str,
) -> String {
    let mut helper = spawn_helper_pty(scenario, geometry);
    let screen = wait_for_screen_contains(
        &mut helper.parser,
        &helper.output_rx,
        marker,
        STARTUP_TIMEOUT,
    )
    .expect("wait for helper status marker");
    terminate_child(helper.child);
    normalize_snapshot(&screen)
}

struct SpawnedHelper {
    child: Box<dyn portable_pty::Child + Send>,
    writer: Box<dyn Write + Send>,
    output_rx: Receiver<Vec<u8>>,
    parser: Parser,
}

fn spawn_helper_pty(scenario: HelperScenario, geometry: PtyGeometry) -> SpawnedHelper {
    let pty_system = native_pty_system();
    let pair = pty_system
        .openpty(geometry.pty_size())
        .expect("open helper pty pair");

    let current_test_bin = std::env::current_exe().expect("resolve current test binary");
    let mut command = CommandBuilder::new(current_test_bin.to_string_lossy().as_ref());
    command.arg("--exact");
    command.arg(scenario.helper_test_name());
    command.arg("--nocapture");
    command.env("HARNESS_TUI_PTY_HELPER_SCENARIO", scenario.env_value());
    configure_deterministic_env(&mut command);

    let child = pair
        .slave
        .spawn_command(command)
        .expect("spawn helper test binary");
    drop(pair.slave);

    let reader = pair
        .master
        .try_clone_reader()
        .expect("clone helper pty reader");
    let writer = pair.master.take_writer().expect("take helper pty writer");
    let output_rx = spawn_reader_thread(reader);

    SpawnedHelper {
        child,
        writer,
        output_rx,
        parser: geometry.parser(),
    }
}

fn helper_scenario_from_env() -> Option<HelperScenario> {
    match std::env::var("HARNESS_TUI_PTY_HELPER_SCENARIO")
        .ok()
        .as_deref()
    {
        Some("type_first_startup") => Some(HelperScenario::TypeFirstStartup),
        Some("streamed_response") => Some(HelperScenario::StreamedResponse),
        Some("permission_with_draft") => Some(HelperScenario::PermissionWithDraft),
        Some("details_drawer") => Some(HelperScenario::DetailsDrawer),
        Some("degraded_bootstrap") => Some(HelperScenario::DegradedBootstrap),
        Some("disconnected_stream") => Some(HelperScenario::DisconnectedStream),
        _ => None,
    }
}

fn assert_or_update_snapshot(name: &str, actual: &str) {
    let path = snapshot_dir().join(format!("{name}.snap"));
    if std::env::var("HARNESS_UPDATE_SNAPSHOTS").as_deref() == Ok("1") {
        fs::create_dir_all(snapshot_dir()).expect("create snapshot directory");
        fs::write(&path, actual).expect("write updated snapshot file");
    }

    let expected = fs::read_to_string(&path).unwrap_or_else(|err| {
        panic!(
            "failed to read snapshot {} ({err}); run with HARNESS_UPDATE_SNAPSHOTS=1 to generate baselines",
            path.display()
        )
    });
    assert_eq!(
        actual,
        expected,
        "snapshot mismatch for {}; run with HARNESS_UPDATE_SNAPSHOTS=1 to accept changes",
        path.display()
    );
}

fn assert_snapshot_secrets_clean() {
    let dir = snapshot_dir();
    if !dir.exists() {
        return;
    }

    for entry in fs::read_dir(&dir).expect("read snapshot directory") {
        let path = entry.expect("snapshot dir entry").path();
        if path.extension().and_then(|ext| ext.to_str()) != Some("snap") {
            continue;
        }
        let text = fs::read_to_string(&path).expect("read snapshot file");
        assert!(
            !text.contains("sk-"),
            "secret-like token found in snapshot {}",
            path.display()
        );
    }
}

fn normalize_snapshot(input: &str) -> String {
    let normalized = input
        .lines()
        .map(|line| normalize_volatile_line(line.trim_end()))
        .collect::<Vec<_>>()
        .join("\n");
    normalized.trim_end().to_string()
}

fn normalize_volatile_line(line: &str) -> String {
    let Some(marker_idx) = line.find("Sequences:") else {
        return line.to_string();
    };

    let trailing_border = if line.ends_with('│') { " │" } else { "" };
    format!("{}Sequences: <RANGE>{trailing_border}", &line[..marker_idx])
}

fn wait_for_screen_contains(
    parser: &mut Parser,
    output_rx: &Receiver<Vec<u8>>,
    needle: &str,
    timeout: Duration,
) -> Result<String, String> {
    let deadline = Instant::now() + timeout;

    loop {
        drain_output(parser, output_rx);

        let current = parser.screen().contents();
        if current.contains(needle) {
            return Ok(stabilize_screen(parser, output_rx, current));
        }

        let now = Instant::now();
        if now >= deadline {
            return Err(format!(
                "timed out waiting for marker '{needle}' after {timeout:?}; final screen:\n{current}"
            ));
        }

        let wait_timeout = cmp::min(READ_POLL_TIMEOUT, deadline.saturating_duration_since(now));
        match output_rx.recv_timeout(wait_timeout) {
            Ok(chunk) => parser.process(&chunk),
            Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => {
                return Err(format!(
                    "pty output disconnected while waiting for '{needle}'; final screen:\n{current}"
                ));
            }
        }
    }
}

fn stabilize_screen(parser: &mut Parser, output_rx: &Receiver<Vec<u8>>, initial: String) -> String {
    let mut latest = initial;
    let mut stable_since = Instant::now();
    let deadline = Instant::now() + STABLE_TIMEOUT;

    loop {
        let now = Instant::now();
        if now >= deadline {
            return latest;
        }

        let wait_timeout = cmp::min(READ_POLL_TIMEOUT, deadline.saturating_duration_since(now));
        match output_rx.recv_timeout(wait_timeout) {
            Ok(chunk) => parser.process(&chunk),
            Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => return latest,
        }

        let current = parser.screen().contents();
        if current != latest {
            latest = current;
            stable_since = Instant::now();
            continue;
        }

        if Instant::now().saturating_duration_since(stable_since) >= STABLE_WINDOW {
            return latest;
        }
    }
}

fn drain_output(parser: &mut Parser, output_rx: &Receiver<Vec<u8>>) {
    while let Ok(chunk) = output_rx.try_recv() {
        parser.process(&chunk);
    }
}

fn send_key(writer: &mut dyn Write, key: u8) -> std::io::Result<()> {
    writer.write_all(&[key])?;
    writer.flush()
}

fn spawn_reader_thread(mut reader: Box<dyn Read + Send>) -> Receiver<Vec<u8>> {
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

fn terminate_child(mut child: Box<dyn portable_pty::Child + Send>) {
    child.kill().expect("terminate helper tui child");
    std::mem::forget(child);
}

fn configure_deterministic_env(command: &mut CommandBuilder) {
    command.env("HARNESS_DETERMINISTIC", "1");
    command.env("HARNESS_DISABLE_ANIMATIONS", "1");
    command.env("HARNESS_SEED", "42");
    command.env("TERM", "xterm-256color");
    command.env("LANG", "C.UTF-8");
    command.env("LC_ALL", "C.UTF-8");
    command.env("TZ", "UTC");
}

fn snapshot_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("snapshots")
}
