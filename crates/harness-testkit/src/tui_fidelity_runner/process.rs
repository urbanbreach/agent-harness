use std::io::Write;
use std::path::Path;
use std::thread;
use std::time::{Duration, Instant};

use portable_pty::{native_pty_system, CommandBuilder, MasterPty};

use super::actions::apply_action;
use super::cleanup::CleanupTracker;
use super::error::RunnerError;
use super::process_io::{configure_environment, pty_size, spawn_reader};
use super::process_wait::{
    collect_descendants, drain, process_error, request_normal_exit, wait_for_prompt_ready,
    wait_for_visible_stable_frame, wait_until,
};
use super::pty_child::PtyChildGuard;
use super::types::{RunnerTiming, RuntimeBinary};
use crate::tui_fidelity::{AdapterKind, CheckpointName, Scenario, Viewport};
use crate::tui_fidelity_cache::ReferenceBinaryCache;

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
    tracker: &mut CleanupTracker,
) -> Result<ProcessCapture, RunnerError> {
    let launch_path = if adapter == AdapterKind::Grok {
        ReferenceBinaryCache::new(
            runtime_dir.join("reference-binary-cache"),
            &binary.path,
            binary.sha256.clone(),
        )
        .stage_for_worker(runtime_dir)
        .map_err(|error| RunnerError::Io {
            path: runtime_dir.to_path_buf(),
            detail: format!("stage reference binary: {error}"),
        })?
    } else {
        binary.path.clone()
    };
    let pty_system = native_pty_system();
    let pair = pty_system
        .openpty(pty_size(scenario.viewport))
        .map_err(|error| process_error(adapter, "open PTY", error))?;
    let mut command = CommandBuilder::new(launch_path.as_os_str());
    command.cwd(runtime_dir);
    if let Some(run_root) = runtime_dir.parent() {
        command.env("TUI_FIDELITY_RUN_ROOT", run_root);
    }
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
    let child = pair
        .slave
        .spawn_command(command)
        .map_err(|error| process_error(adapter, "spawn", error))?;
    drop(pair.slave);
    let pid = child.process_id().ok_or_else(|| RunnerError::Process {
        adapter,
        detail: "spawned child has no process ID".to_owned(),
    })?;
    let mut guard = PtyChildGuard::new(child, pid, timing.cleanup_timeout);
    let result: Result<ProcessCapture, RunnerError> = (|| {
        let reader = pair
            .master
            .try_clone_reader()
            .map_err(|error| process_error(adapter, "clone reader", error))?;
        let mut writer = pair
            .master
            .take_writer()
            .map_err(|error| process_error(adapter, "take writer", error))?;
        let output = spawn_reader(reader);
        let process_start = Instant::now();
        let deadline = process_start + timing.scenario_timeout;
        let mut stream = Vec::new();
        let mut inputs = Vec::new();
        let mut checkpoints = Vec::new();
        let readiness_viewport = scenario
            .checkpoints
            .first()
            .map_or(scenario.viewport, |checkpoint| checkpoint.frame.viewport);

        let (child, observed) = guard.parts_mut(adapter)?;
        wait_for_prompt_ready(
            readiness_viewport,
            deadline,
            adapter,
            child,
            &output,
            &mut stream,
            observed,
            pid,
        )?;
        let start = Instant::now();

        for action in &scenario.actions {
            let (child, observed) = guard.parts_mut(adapter)?;
            wait_until(
                action.at_tick().0,
                timing.tick,
                start,
                deadline,
                adapter,
                child,
                &output,
                &mut stream,
                observed,
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
                let (child, observed) = guard.parts_mut(adapter)?;
                wait_for_visible_stable_frame(
                    viewport,
                    deadline,
                    adapter,
                    child,
                    &output,
                    &mut stream,
                    observed,
                    pid,
                )?;
            } else {
                apply_action(action, adapter, pair.master.as_ref(), writer.as_mut())?;
            }
            inputs.push(start.elapsed());
        }
        for checkpoint in &scenario.checkpoints {
            let (child, observed) = guard.parts_mut(adapter)?;
            wait_until(
                checkpoint.at_tick.0,
                timing.tick,
                start,
                deadline,
                adapter,
                child,
                &output,
                &mut stream,
                observed,
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
            return Err(RunnerError::SkippedReference);
        }
        let exit_deadline = Instant::now() + timing.normal_exit_timeout;
        let (child, observed) = guard.parts_mut(adapter)?;
        let stepped_exit = request_normal_exit(
            adapter,
            writer.as_mut(),
            child,
            exit_deadline,
            pid,
            observed,
        )?;
        let exit_code = if let Some(code) = stepped_exit {
            code
        } else {
            loop {
                let (child, observed) = guard.parts_mut(adapter)?;
                collect_descendants(pid, observed);
                match child.try_wait() {
                    Ok(Some(status)) => {
                        break i32::try_from(status.exit_code()).unwrap_or(i32::MAX);
                    }
                    Ok(None) if Instant::now() < exit_deadline => {
                        thread::sleep(Duration::from_millis(5))
                    }
                    Ok(None) => {
                        return Err(RunnerError::ForcedKillOnly { adapter });
                    }
                    Err(error) => return Err(process_error(adapter, "wait for exit", error)),
                }
            }
        };
        drain(&output, &mut stream);
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
    })();
    let cleanup = guard.cleanup();
    let detected = cleanup.detected_child_pids.clone();
    tracker.record_process(cleanup);
    if !detected.is_empty() {
        return Err(RunnerError::SurvivingChild {
            adapter,
            pids: detected,
        });
    }
    result
}
