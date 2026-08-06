use std::collections::BTreeSet;
use std::sync::mpsc::Receiver;
use std::time::Instant;

use super::error::RunnerError;
use super::process_wait::{wait_for_prompt_ready, wait_for_visible_stable_frame};
use crate::tui_fidelity::{AdapterKind, TerminalType, Viewport};

type PtyChild = Box<dyn portable_pty::Child + Send + Sync>;

#[expect(
    clippy::too_many_arguments,
    reason = "readiness observes one PTY lifecycle boundary"
)]
pub(super) fn wait_for_readiness(
    terminal_type: TerminalType,
    viewport: Viewport,
    deadline: Instant,
    adapter: AdapterKind,
    child: &mut PtyChild,
    output: &Receiver<Vec<u8>>,
    stream: &mut Vec<u8>,
    observed: &mut BTreeSet<u32>,
    pid: u32,
    require_grok_identity: bool,
) -> Result<(), RunnerError> {
    if terminal_type == TerminalType::Xterm {
        wait_for_visible_stable_frame(
            viewport, deadline, adapter, child, output, stream, observed, pid,
        )
    } else {
        wait_for_prompt_ready(
            viewport,
            deadline,
            adapter,
            child,
            output,
            stream,
            observed,
            pid,
            require_grok_identity,
        )
    }
}

#[expect(
    clippy::too_many_arguments,
    reason = "stable-frame wait observes one PTY lifecycle boundary"
)]
pub(super) fn wait_for_stable_frame(
    viewport: Viewport,
    deadline: Instant,
    adapter: AdapterKind,
    child: &mut PtyChild,
    output: &Receiver<Vec<u8>>,
    stream: &mut Vec<u8>,
    observed: &mut BTreeSet<u32>,
    pid: u32,
) -> Result<(), RunnerError> {
    wait_for_visible_stable_frame(
        viewport, deadline, adapter, child, output, stream, observed, pid,
    )
}
