use std::collections::BTreeSet;
use std::ops::Range;
use std::sync::mpsc::Receiver;
use std::time::{Duration, Instant};

use super::error::RunnerError;
use super::process::CapturedCheckpoint;
use super::process_io::PtyRead;
use super::process_wait::{drain, wait_for_text, wait_until};
use crate::tui_fidelity::{
    AdapterKind, CaptureMode, Checkpoint, CheckpointName, Scenario, Tick, Viewport,
};

type PtyChild = Box<dyn portable_pty::Child + Send + Sync>;

#[expect(
    clippy::too_many_arguments,
    reason = "checkpoint capture owns one PTY lifecycle boundary"
)]
pub(super) fn capture(
    scenario: &Scenario,
    range: Range<usize>,
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
    for checkpoint in &scenario.checkpoints[range] {
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

pub(super) fn checkpoint_end_before_action(
    checkpoints: &[Checkpoint],
    start: usize,
    action_tick: Tick,
) -> usize {
    start
        + checkpoints[start..]
            .iter()
            .take_while(|checkpoint| checkpoint.at_tick < action_tick)
            .count()
}

const fn packet3_stream_marker(checkpoint: CheckpointName) -> &'static str {
    match checkpoint {
        CheckpointName::Rest => crate::tui_fidelity_fixture::PACKET3_STREAM_REST,
        CheckpointName::Mid => crate::tui_fidelity_fixture::PACKET3_STREAM_MID,
        CheckpointName::Settled => "requested work is finished.",
    }
}

#[cfg(test)]
mod tests {
    use super::checkpoint_end_before_action;
    use crate::tui_fidelity::{
        Checkpoint, CheckpointName, FrameCapture, SemanticState, Tick, Viewport,
    };

    #[test]
    fn checkpoints_before_an_action_are_scheduled_before_that_action() {
        // arrange
        let checkpoints = [
            checkpoint(CheckpointName::Rest, 3),
            checkpoint(CheckpointName::Mid, 5),
            checkpoint(CheckpointName::Settled, 6),
        ];

        // act
        let end = checkpoint_end_before_action(&checkpoints, 0, Tick(4));
        let equal_tick_end = checkpoint_end_before_action(&checkpoints, 0, Tick(3));

        // assert
        assert_eq!(end, 1, "tick-3 rest must precede a tick-4 action");
        assert_eq!(equal_tick_end, 0, "an equal-tick action runs first");
    }

    fn checkpoint(name: CheckpointName, tick: u64) -> Checkpoint {
        Checkpoint {
            name,
            at_tick: Tick(tick),
            frame: FrameCapture {
                capture_id: name.as_str().to_owned(),
                viewport: Viewport {
                    cols: 100,
                    rows: 30,
                },
                state: SemanticState::Rest,
            },
        }
    }
}
