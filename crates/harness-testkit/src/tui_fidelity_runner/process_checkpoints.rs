use std::collections::BTreeSet;
use std::sync::mpsc::Receiver;
use std::time::{Duration, Instant};

use super::error::RunnerError;
use super::process::CapturedCheckpoint;
use super::process_wait::{drain, wait_until};
use crate::tui_fidelity::{AdapterKind, Scenario, Viewport};

type PtyChild = Box<dyn portable_pty::Child + Send + Sync>;

#[expect(
    clippy::too_many_arguments,
    reason = "checkpoint capture owns one PTY lifecycle boundary"
)]
pub(super) fn capture(
    scenario: &Scenario,
    tick: Duration,
    start: Instant,
    deadline: Instant,
    adapter: AdapterKind,
    child: &mut PtyChild,
    output: &Receiver<Vec<u8>>,
    stream: &mut Vec<u8>,
    observed: &mut BTreeSet<u32>,
    pid: u32,
) -> Result<Vec<CapturedCheckpoint>, RunnerError> {
    let mut checkpoints = Vec::new();
    for checkpoint in &scenario.checkpoints {
        wait_until(
            checkpoint.at_tick.0,
            tick,
            start,
            deadline,
            adapter,
            child,
            output,
            stream,
            observed,
            pid,
        )?;
        drain(output, stream);
        checkpoints.push(CapturedCheckpoint {
            name: checkpoint.name,
            viewport: checkpoint.frame.viewport,
            elapsed: start.elapsed(),
            stream: stream.clone(),
        });
    }
    Ok(checkpoints)
}
