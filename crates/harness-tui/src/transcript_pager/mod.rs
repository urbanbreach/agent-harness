mod cleanup;
mod launch;
mod restore;
mod snapshot;
mod suspend;

pub use cleanup::CleanupReport;
pub use launch::{launch_pager, PagerCommand, PagerExit, PagerStdio};
pub use restore::{restore_terminal_state, restore_terminal_state_with, RestoreGuard};
pub use snapshot::TranscriptSnapshot;
pub use suspend::{
    suspend_terminal_state, suspend_terminal_state_with, LifecycleEvent, SavedState,
    SystemTerminal, TerminalControl, TerminalState,
};

#[derive(Debug, PartialEq, Eq)]
pub enum PagerError {
    Terminal {
        operation: &'static str,
        detail: String,
    },
    TerminalPoisoned,
    Spawn {
        command: String,
        detail: String,
    },
    Write {
        detail: String,
    },
    Wait {
        detail: String,
    },
    Output {
        stream: &'static str,
        detail: String,
    },
    Timeout {
        cleanup: CleanupReport,
    },
    Cleanup {
        cleanup: CleanupReport,
    },
}

impl std::fmt::Display for PagerError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Terminal { operation, detail } => {
                write!(formatter, "terminal {operation}: {detail}")
            }
            Self::TerminalPoisoned => formatter.write_str("terminal event log was poisoned"),
            Self::Spawn { command, detail } => {
                write!(formatter, "failed to spawn pager {command}: {detail}")
            }
            Self::Write { detail } => write!(formatter, "failed to write pager snapshot: {detail}"),
            Self::Wait { detail } => write!(formatter, "failed waiting for pager: {detail}"),
            Self::Output { stream, detail } => {
                write!(formatter, "failed reading pager {stream}: {detail}")
            }
            Self::Timeout { cleanup } => write!(formatter, "pager timed out; cleanup={cleanup}"),
            Self::Cleanup { cleanup } => {
                write!(formatter, "pager cleanup left descendants: {cleanup}")
            }
        }
    }
}

impl std::error::Error for PagerError {}

/// Runs a pager while making terminal suspension and restoration one operation.
pub fn run_pager<T: TerminalControl>(
    snapshot: &TranscriptSnapshot,
    pager_cmd: &PagerCommand,
    stdio: PagerStdio,
    terminal: &mut T,
) -> Result<PagerExit, PagerError> {
    let saved = suspend_terminal_state_with(terminal)?;
    let mut guard = RestoreGuard::new(terminal, saved);
    let launch_result = launch_pager(snapshot, pager_cmd, stdio);
    let restore_result = guard.restore();

    match (launch_result, restore_result) {
        (Ok(exit), Ok(())) => Ok(exit),
        (Err(error), Ok(())) => Err(error),
        (Ok(_), Err(error)) | (Err(_), Err(error)) => Err(error),
    }
}
