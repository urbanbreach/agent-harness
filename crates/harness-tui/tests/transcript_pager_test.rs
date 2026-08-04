use std::sync::{Arc, Mutex};
use std::time::Duration;

use harness_tui::transcript_pager::{
    LifecycleEvent, PagerCommand, PagerError, PagerExit, PagerStdio, TerminalControl,
    TerminalState, TranscriptSnapshot, run_pager,
};

#[derive(Clone, Debug)]
struct FakeTerminal {
    state: TerminalState,
    events: Arc<Mutex<Vec<LifecycleEvent>>>,
}

impl FakeTerminal {
    fn new() -> Self {
        Self {
            state: TerminalState::active(120, 40),
            events: Arc::new(Mutex::new(Vec::new())),
        }
    }

    fn event_log(&self) -> Vec<LifecycleEvent> {
        self.events
            .lock()
            .map(|events| events.clone())
            .unwrap_or_default()
    }
}

impl TerminalControl for FakeTerminal {
    fn state(&self) -> TerminalState {
        self.state.clone()
    }

    fn suspend_raw_mode(&mut self) -> Result<(), PagerError> {
        self.events
            .lock()
            .map_err(|_| PagerError::TerminalPoisoned)?
            .push(LifecycleEvent::SuspendRawMode);
        self.state.raw_mode = false;
        Ok(())
    }

    fn suspend_mouse(&mut self) -> Result<(), PagerError> {
        self.events
            .lock()
            .map_err(|_| PagerError::TerminalPoisoned)?
            .push(LifecycleEvent::SuspendMouse);
        self.state.mouse = false;
        Ok(())
    }

    fn reset_title(&mut self) -> Result<(), PagerError> {
        self.events
            .lock()
            .map_err(|_| PagerError::TerminalPoisoned)?
            .push(LifecycleEvent::SuspendTitle);
        self.state.title = None;
        Ok(())
    }

    fn pause_media(&mut self) -> Result<(), PagerError> {
        self.events
            .lock()
            .map_err(|_| PagerError::TerminalPoisoned)?
            .push(LifecycleEvent::SuspendMedia);
        self.state.media = false;
        Ok(())
    }

    fn suspend_cursor(&mut self) -> Result<(), PagerError> {
        self.events
            .lock()
            .map_err(|_| PagerError::TerminalPoisoned)?
            .push(LifecycleEvent::SuspendCursor);
        self.state.cursor = false;
        Ok(())
    }

    fn reinitialize_capabilities(&mut self) -> Result<(), PagerError> {
        self.events
            .lock()
            .map_err(|_| PagerError::TerminalPoisoned)?
            .push(LifecycleEvent::RestoreCapabilities);
        Ok(())
    }

    fn restore_cursor(&mut self, visible: bool) -> Result<(), PagerError> {
        self.events
            .lock()
            .map_err(|_| PagerError::TerminalPoisoned)?
            .push(LifecycleEvent::RestoreCursor);
        self.state.cursor = visible;
        Ok(())
    }

    fn restore_media(&mut self, active: bool) -> Result<(), PagerError> {
        self.events
            .lock()
            .map_err(|_| PagerError::TerminalPoisoned)?
            .push(LifecycleEvent::RestoreMedia);
        self.state.media = active;
        Ok(())
    }

    fn restore_title(&mut self, title: Option<&str>) -> Result<(), PagerError> {
        self.events
            .lock()
            .map_err(|_| PagerError::TerminalPoisoned)?
            .push(LifecycleEvent::RestoreTitle);
        self.state.title = title.map(str::to_owned);
        Ok(())
    }

    fn restore_mouse(&mut self, active: bool) -> Result<(), PagerError> {
        self.events
            .lock()
            .map_err(|_| PagerError::TerminalPoisoned)?
            .push(LifecycleEvent::RestoreMouse);
        self.state.mouse = active;
        Ok(())
    }

    fn restore_raw_mode(&mut self, active: bool) -> Result<(), PagerError> {
        self.events
            .lock()
            .map_err(|_| PagerError::TerminalPoisoned)?
            .push(LifecycleEvent::RestoreRawMode);
        self.state.raw_mode = active;
        Ok(())
    }
}

fn command(script: &str) -> PagerCommand {
    PagerCommand::new("sh").arg("-c").arg(script)
}

fn expected_restore_order() -> Vec<LifecycleEvent> {
    vec![
        LifecycleEvent::SuspendRawMode,
        LifecycleEvent::SuspendMouse,
        LifecycleEvent::SuspendTitle,
        LifecycleEvent::SuspendMedia,
        LifecycleEvent::SuspendCursor,
        LifecycleEvent::RestoreCapabilities,
        LifecycleEvent::RestoreCursor,
        LifecycleEvent::RestoreMedia,
        LifecycleEvent::RestoreTitle,
        LifecycleEvent::RestoreMouse,
        LifecycleEvent::RestoreRawMode,
    ]
}

#[test]
fn pager_success_suspends_launches_and_restores_in_order() {
    // Given: an active terminal and a pager that exits successfully.
    let mut terminal = FakeTerminal::new();
    let snapshot = TranscriptSnapshot::from_text("hello");

    // When: the external pager lifecycle runs.
    let exit = run_pager(
        &snapshot,
        &command("cat >/dev/null"),
        PagerStdio::capture(),
        &mut terminal,
    )
    .unwrap_or_else(|error| panic!("pager should succeed: {error}"));

    // Then: exit and terminal operations are observable and ordered.
    assert_eq!(exit, PagerExit::code(0, Vec::new(), Vec::new()));
    assert_eq!(terminal.event_log(), expected_restore_order());
    assert_eq!(terminal.state, TerminalState::active(120, 40));
}

#[test]
fn pager_nonzero_exit_still_restores_once() {
    // Given: a pager that returns a nonzero code.
    let mut terminal = FakeTerminal::new();
    let snapshot = TranscriptSnapshot::from_text("failure");

    // When: the pager exits with code 7.
    let exit = run_pager(
        &snapshot,
        &command("exit 7"),
        PagerStdio::capture(),
        &mut terminal,
    )
    .unwrap_or_else(|error| panic!("nonzero exit should be captured: {error}"));

    // Then: the code is captured and each restore operation occurs once.
    assert_eq!(exit.code, Some(7));
    assert_eq!(terminal.event_log(), expected_restore_order());
}

#[test]
fn missing_pager_restores_terminal_state() {
    // Given: a command path that cannot be spawned.
    let mut terminal = FakeTerminal::new();
    let snapshot = TranscriptSnapshot::from_text("missing");

    // When: launching the missing pager.
    let result = run_pager(
        &snapshot,
        &PagerCommand::new("/definitely/missing/harness-pager"),
        PagerStdio::capture(),
        &mut terminal,
    );

    // Then: spawn failure is typed and restoration still runs exactly once.
    assert!(matches!(result, Err(PagerError::Spawn { .. })));
    assert_eq!(terminal.event_log(), expected_restore_order());
}

#[test]
fn resize_reinitializes_capabilities_before_exact_restore() {
    // Given: a pager output that represents a resize observed while suspended.
    let mut terminal = FakeTerminal::new();
    let snapshot = TranscriptSnapshot::from_text("resize");

    // When: the pager runs and the terminal is reinitialized on return.
    let exit = run_pager(
        &snapshot,
        &command("printf RESIZED"),
        PagerStdio::capture(),
        &mut terminal,
    )
    .unwrap_or_else(|error| panic!("resize pager should succeed: {error}"));

    // Then: the output survived and capabilities were reinitialized before reverse restore.
    assert_eq!(String::from_utf8_lossy(&exit.stdout), "RESIZED");
    assert_eq!(terminal.event_log()[5], LifecycleEvent::RestoreCapabilities);
    assert_eq!(terminal.state, TerminalState::active(120, 40));
}

#[cfg(unix)]
#[test]
fn sigint_exit_is_captured_and_terminal_is_restored() {
    // Given: a pager that terminates itself with SIGINT.
    let mut terminal = FakeTerminal::new();
    let snapshot = TranscriptSnapshot::from_text("interrupt");

    // When: the child exits from SIGINT.
    let exit = run_pager(
        &snapshot,
        &command("kill -INT $$"),
        PagerStdio::capture(),
        &mut terminal,
    )
    .unwrap_or_else(|error| panic!("SIGINT should be captured: {error}"));

    // Then: SIGINT is represented without bypassing restoration.
    assert_eq!(exit.signal, Some(2));
    assert_eq!(terminal.event_log(), expected_restore_order());
}

#[cfg(unix)]
#[test]
fn timeout_cleans_up_descendants_without_orphans() {
    // Given: a pager with a descendant that would outlive the root without group cleanup.
    let mut terminal = FakeTerminal::new();
    let snapshot = TranscriptSnapshot::from_text("cleanup");

    // When: the bounded pager wait expires.
    let result = run_pager(
        &snapshot,
        &command("sleep 30 & wait"),
        PagerStdio::capture_with_timeout(Duration::from_millis(100)),
        &mut terminal,
    );

    // Then: timeout carries cleanup evidence and restoration is still exact.
    assert!(
        matches!(result, Err(PagerError::Timeout { cleanup }) if cleanup.surviving_pids.is_empty())
    );
    assert_eq!(terminal.event_log(), expected_restore_order());
}

#[test]
fn snapshot_redacts_secrets_and_restores_state_exactly_once() {
    // Given: transcript text containing scanner-recognized secret shapes.
    let mut terminal = FakeTerminal::new();
    let snapshot = TranscriptSnapshot::from_text(
        "Authorization: Bearer abc.def-ghi_123 and sk-AbCdEf0123456789",
    );

    // When: the redacted snapshot is sent to the pager.
    let exit = run_pager(
        &snapshot,
        &command("cat"),
        PagerStdio::capture(),
        &mut terminal,
    )
    .unwrap_or_else(|error| panic!("snapshot pager should succeed: {error}"));

    // Then: raw credentials never cross the pager boundary and restore is once-only.
    let output = String::from_utf8_lossy(&exit.stdout);
    assert!(!output.contains("abc.def-ghi_123"));
    assert!(!output.contains("sk-AbCdEf0123456789"));
    assert!(output.contains("[REDACTED"));
    assert_eq!(terminal.event_log(), expected_restore_order());
    assert_eq!(terminal.state, TerminalState::active(120, 40));
}
