use std::collections::BTreeSet;
use std::sync::mpsc::Receiver;
use std::time::{Duration, Instant};

use super::error::RunnerError;
use super::process::CapturedCheckpoint;
use super::process_io::PtyRead;
use super::process_wait::{drain, wait_for_text, wait_until};
use crate::tui_fidelity::{AdapterKind, CaptureMode, CheckpointName, Scenario, Viewport};

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
    output: &Receiver<PtyRead>,
    stream: &mut Vec<u8>,
    observed: &mut BTreeSet<u32>,
    pid: u32,
) -> Result<Vec<CapturedCheckpoint>, RunnerError> {
    let mut checkpoints = Vec::new();
    for checkpoint in &scenario.checkpoints {
        if scenario.id.0.starts_with("packet3-baseline-stream--") {
            wait_for_text(
                packet3_stream_marker(checkpoint.name),
                checkpoint.frame.viewport,
                deadline,
                adapter,
                child,
                output,
                stream,
                observed,
                pid,
            )?;
        }
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
        if scenario.capture_mode == CaptureMode::ActionTail {
            stream.clear();
        }
        checkpoints.push(CapturedCheckpoint {
            name: checkpoint.name,
            viewport: checkpoint.frame.viewport,
            elapsed: start.elapsed(),
            stream: stream.clone(),
        });
    }
    Ok(checkpoints)
}

const fn packet3_stream_marker(checkpoint: CheckpointName) -> &'static str {
    match checkpoint {
        CheckpointName::Rest => crate::tui_fidelity_fixture::PACKET3_STREAM_REST,
        CheckpointName::Mid => crate::tui_fidelity_fixture::PACKET3_STREAM_MID,
        CheckpointName::Settled => "requested work is finished.",
    }
}
