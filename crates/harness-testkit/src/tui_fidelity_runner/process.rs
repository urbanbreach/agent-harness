use std::collections::BTreeSet;
use std::io::{Read, Write};
use std::path::Path;
use std::sync::mpsc::{self, Receiver};
use std::thread;
use std::time::{Duration, Instant};

use portable_pty::{native_pty_system, CommandBuilder, MasterPty, PtySize};

use super::actions::apply_action;
use super::error::RunnerError;
use super::process_tree::{terminate_group, terminate_pids, wait_for_living};
use super::process_wait::{
    collect_descendants, drain, process_error, request_normal_exit, wait_for_visible_stable_frame,
    wait_until,
};
use super::types::{RunnerTiming, RuntimeBinary};
use crate::tui_fidelity::{AdapterKind, CheckpointName, Scenario, Viewport};

pub(super) struct CapturedCheckpoint {
    pub name: CheckpointName,
    pub viewport: Viewport,
    pub elapsed: Duration,
    pub stream: Vec<u8>,
}

pub(super) struct ProcessCapture {
    pub exit_code: i32,
    pub input_timestamps: Vec<Duration>,
    pub checkpoints: Vec<CapturedCheckpoint>,
}

pub(super) fn execute(
    scenario: &Scenario,
    timing: RunnerTiming,
    adapter: AdapterKind,
    binary: &RuntimeBinary,
    runtime_dir: &Path,
) -> Result<ProcessCapture, RunnerError> {
    let pty_system = native_pty_system();
    let pair = pty_system
        .openpty(pty_size(scenario.viewport))
        .map_err(|error| process_error(adapter, "open PTY", error))?;
    let mut command = CommandBuilder::new(binary.path.as_os_str());
    command.cwd(runtime_dir);
    if adapter == AdapterKind::Harness {
        command.args([
            "tui",
            "--mock",
            "--deterministic",
            "--session-dir",
            runtime_dir.join("sessions").to_string_lossy().as_ref(),
        ]);
    }
    configure_environment(&mut command);
    let mut child = pair
        .slave
        .spawn_command(command)
        .map_err(|error| process_error(adapter, "spawn", error))?;
    drop(pair.slave);
    let pid = child.process_id().ok_or_else(|| RunnerError::Process {
        adapter,
        detail: "spawned child has no process ID".to_owned(),
    })?;
    let reader = pair
        .master
        .try_clone_reader()
        .map_err(|error| process_error(adapter, "clone reader", error))?;
    let mut writer = pair
        .master
        .take_writer()
        .map_err(|error| process_error(adapter, "take writer", error))?;
    let output = spawn_reader(reader);
    let start = Instant::now();
    let deadline = start + timing.scenario_timeout;
    let mut stream = Vec::new();
    let mut observed = BTreeSet::new();
    let mut inputs = Vec::new();
    let mut checkpoints = Vec::new();

    for action in &scenario.actions {
        wait_until(
            action.at_tick().0,
            timing.tick,
            start,
            deadline,
            adapter,
            &mut child,
            &output,
            &mut stream,
            &mut observed,
            pid,
        )?;
        if matches!(
            action,
            crate::tui_fidelity::ScenarioAction::WaitForSemanticState(_)
        ) {
            let viewport = scenario
                .checkpoints
                .first()
                .map_or(scenario.viewport, |checkpoint| checkpoint.frame.viewport);
            wait_for_visible_stable_frame(
                viewport,
                deadline,
                adapter,
                &mut child,
                &output,
                &mut stream,
                &mut observed,
                pid,
            )?;
        } else {
            apply_action(action, adapter, pair.master.as_ref(), writer.as_mut())?;
        }
        inputs.push(start.elapsed());
    }
    for checkpoint in &scenario.checkpoints {
        wait_until(
            checkpoint.at_tick.0,
            timing.tick,
            start,
            deadline,
            adapter,
            &mut child,
            &output,
            &mut stream,
            &mut observed,
            pid,
        )?;
        drain(&output, &mut stream);
        checkpoints.push(CapturedCheckpoint {
            name: checkpoint.name,
            viewport: checkpoint.frame.viewport,
            elapsed: start.elapsed(),
            stream: stream.clone(),
        });
    }
    if adapter == AdapterKind::Grok && String::from_utf8_lossy(&stream).contains("Skipped") {
        terminate_group(pid, timing.cleanup_timeout);
        let _ = child.wait();
        return Err(RunnerError::SkippedReference);
    }
    let exit_deadline = Instant::now() + timing.normal_exit_timeout;
    let stepped_exit = request_normal_exit(
        adapter,
        writer.as_mut(),
        &mut child,
        exit_deadline,
        pid,
        &mut observed,
    )?;
    let exit_code = if let Some(code) = stepped_exit {
        code
    } else {
        loop {
            collect_descendants(pid, &mut observed);
            match child.try_wait() {
                Ok(Some(status)) => break i32::try_from(status.exit_code()).unwrap_or(i32::MAX),
                Ok(None) if Instant::now() < exit_deadline => {
                    thread::sleep(Duration::from_millis(5))
                }
                Ok(None) => {
                    terminate_group(pid, timing.cleanup_timeout);
                    let _ = child.wait();
                    return Err(RunnerError::ForcedKillOnly { adapter });
                }
                Err(error) => return Err(process_error(adapter, "wait for exit", error)),
            }
        }
    };
    drain(&output, &mut stream);
    let surviving = wait_for_living(&observed, timing.cleanup_timeout);
    if !surviving.is_empty() {
        terminate_pids(&surviving);
        return Err(RunnerError::SurvivingChild {
            adapter,
            pids: surviving,
        });
    }
    if exit_code != scenario.expected_exit.code {
        return Err(RunnerError::UnexpectedExit {
            adapter,
            expected: scenario.expected_exit.code,
            actual: exit_code,
        });
    }
    Ok(ProcessCapture {
        exit_code,
        input_timestamps: inputs,
        checkpoints,
    })
}

fn configure_environment(command: &mut CommandBuilder) {
    for (key, value) in [
        ("TERM", "xterm-256color"),
        ("COLORTERM", "truecolor"),
        ("LANG", "C.UTF-8"),
        ("LC_ALL", "C.UTF-8"),
        ("TZ", "UTC"),
        ("HARNESS_DETERMINISTIC", "1"),
        ("HARNESS_SEED", "42"),
    ] {
        command.env(key, value);
    }
}

const fn pty_size(viewport: Viewport) -> PtySize {
    PtySize {
        rows: viewport.rows,
        cols: viewport.cols,
        pixel_width: 0,
        pixel_height: 0,
    }
}

fn spawn_reader(mut reader: Box<dyn Read + Send>) -> Receiver<Vec<u8>> {
    let (sender, receiver) = mpsc::channel();
    thread::spawn(move || {
        let mut buffer = [0_u8; 4096];
        while let Ok(count) = reader.read(&mut buffer) {
            if count == 0 || sender.send(buffer[..count].to_vec()).is_err() {
                break;
            }
        }
    });
    receiver
}
