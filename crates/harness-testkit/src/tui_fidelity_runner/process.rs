use std::fs::OpenOptions;
use std::path::Path;
use std::thread;
use std::time::{Duration, Instant};

use portable_pty::{native_pty_system, CommandBuilder, MasterPty};

use super::actions::apply_action;
use super::cleanup::CleanupTracker;
use super::error::RunnerError;
use super::interaction_queue;
use super::lifecycle_diagnostics::write_failure;
use super::process_checkpoints::capture as capture_checkpoints;
use super::process_io::{configure_environment, pty_size, spawn_reader};
use super::process_readiness::{wait_for_readiness, wait_for_stable_frame};
use super::process_wait::{
    collect_descendants, drain, process_error, request_normal_exit, wait_until,
};
use super::pty_child::PtyChildGuard;
use super::types::{RunnerTiming, RuntimeBinary};
use crate::tui_fidelity::{AdapterKind, CaptureMode, CheckpointName, Scenario, Viewport};
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
    pub raw_reads: Vec<super::presentation_receipt::RawPtyRead>,
    pub observations: Vec<super::presentation_receipt::TimedSemanticObservation>,
    pub action_sends: Vec<super::presentation_receipt::ActualInputSend>,
    pub pty_stream: Vec<u8>,
}

pub(super) fn execute(
    scenario: &Scenario,
    timing: RunnerTiming,
    adapter: AdapterKind,
    binary: &RuntimeBinary,
    runtime_dir: &Path,
    evidence_dir: &Path,
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
    configure_environment(&mut command, scenario.terminal_type);
    let child_evidence_dir = if evidence_dir.is_absolute() {
        evidence_dir.to_path_buf()
    } else {
        std::env::current_dir()
            .map_err(|error| RunnerError::Io {
                path: evidence_dir.to_path_buf(),
                detail: format!("resolve runner cwd for Harness evidence: {error}"),
            })?
            .join(evidence_dir)
    };
    let interaction_queue_path = child_evidence_dir.join("interaction-ids");
    if adapter == AdapterKind::Harness {
        command.env(
            "TUI_FIDELITY_PRESENTATION_TRACE",
            child_evidence_dir.join("native-presentation.json"),
        );
        command.env("TUI_FIDELITY_INTERACTION_QUEUE", &interaction_queue_path);
    }
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
    let mut stream = Vec::new();
    let mut action_timeline = Vec::new();
    let mut lifecycle_phase = "spawned";
    let result: Result<ProcessCapture, RunnerError> = (|| {
        let reader = pair
            .master
            .try_clone_reader()
            .map_err(|error| process_error(adapter, "clone reader", error))?;
        let mut writer = pair
            .master
            .take_writer()
            .map_err(|error| process_error(adapter, "take writer", error))?;
        let process_start = Instant::now();
        let (output, read_log) = spawn_reader(reader, process_start);
        let deadline = process_start + timing.scenario_timeout;
        let mut inputs = Vec::new();
        let mut action_sends = Vec::new();
        let mut interaction_queue = if adapter == AdapterKind::Harness {
            std::fs::create_dir_all(evidence_dir).map_err(|error| RunnerError::Io {
                path: evidence_dir.to_path_buf(),
                detail: format!("create Harness evidence directory: {error}"),
            })?;
            Some(
                OpenOptions::new()
                    .create(true)
                    .append(true)
                    .open(&interaction_queue_path)
                    .map_err(|error| RunnerError::Io {
                        path: interaction_queue_path.clone(),
                        detail: format!("open interaction queue: {error}"),
                    })?,
            )
        } else {
            None
        };
        let readiness_viewport = scenario.viewport;

        let (child, observed) = guard.parts_mut(adapter)?;
        wait_for_readiness(
            scenario.terminal_type,
            readiness_viewport,
            deadline,
            adapter,
            child,
            &output,
            &mut stream,
            observed,
            pid,
            adapter == AdapterKind::Grok && binary.source_revision != "reference-revision",
        )?;
        lifecycle_phase = "prompt_ready";
        let start = Instant::now();

        for (ordinal, action) in scenario.actions.iter().enumerate() {
            let interaction_id = format!("{}:action:{ordinal}", scenario.id.0);
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
                wait_for_stable_frame(
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
                if let Some(queue) = interaction_queue.as_mut() {
                    interaction_queue::append(queue, &interaction_id, action).map_err(|error| {
                        RunnerError::Io {
                            path: interaction_queue_path.clone(),
                            detail: format!("append typed interaction receipt: {error}"),
                        }
                    })?;
                }
                apply_action(action, adapter, pair.master.as_ref(), writer.as_mut())?;
            }
            action_timeline.push(serde_json::json!({
                "kind": action.kind_name(),
                "at_tick": action.at_tick().0,
                "elapsed_millis": start.elapsed().as_millis(),
            }));
            inputs.push(start.elapsed());
            if !matches!(
                action,
                crate::tui_fidelity::ScenarioAction::WaitForSemanticState(_)
            ) {
                let scheduled_at = start.duration_since(process_start)
                    + timing
                        .tick
                        .saturating_mul(u32::try_from(action.at_tick().0).unwrap_or(u32::MAX));
                action_sends.push(super::presentation_receipt::ActualInputSend {
                    interaction_id: super::presentation_receipt::InteractionId(interaction_id),
                    action_ordinal: ordinal,
                    scheduled_at: super::presentation_receipt::PresentationTimestamp(
                        u64::try_from(scheduled_at.as_micros()).unwrap_or(u64::MAX),
                    ),
                    sent_at: super::presentation_receipt::PresentationTimestamp(
                        u64::try_from(process_start.elapsed().as_micros()).unwrap_or(u64::MAX),
                    ),
                    transport_drained_at: None,
                });
            }
        }
        if scenario.capture_mode == CaptureMode::ActionTail {
            stream.clear();
        }
        let (child, observed) = guard.parts_mut(adapter)?;
        let checkpoints = capture_checkpoints(
            scenario,
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
        lifecycle_phase = "checkpoints_complete";
        if adapter == AdapterKind::Grok && String::from_utf8_lossy(&stream).contains("Skipped") {
            return Err(RunnerError::SkippedReference);
        }
        let exit_deadline = Instant::now() + timing.normal_exit_timeout;
        let (child, observed) = guard.parts_mut(adapter)?;
        lifecycle_phase = "normal_exit_requested";
        let active =
            adapter == AdapterKind::Grok && String::from_utf8_lossy(&stream).contains("Starting");
        action_timeline.extend(super::actions::normal_exit_timeline(adapter, active));
        let stepped_exit = request_normal_exit(
            adapter,
            writer.as_mut(),
            child,
            exit_deadline,
            pid,
            observed,
            active,
        )?;
        lifecycle_phase = if stepped_exit.is_some() {
            "normal_exit_observed"
        } else {
            "normal_exit_waiting"
        };
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
        let reads = read_log
            .lock()
            .map_err(|error| RunnerError::Process {
                adapter,
                detail: format!("read history lock: {error}"),
            })?
            .clone();
        let stable_repeats = scenario
            .motion_capture
            .markers
            .last()
            .map_or(0, |marker| marker.repeat_count);
        let mut observer = super::pty_observation::PtyObserver::new(scenario.viewport);
        for read in &reads {
            observer.observe(read);
        }
        let (raw_reads, observations) =
            observer
                .finish(stable_repeats)
                .map_err(|error| RunnerError::Process {
                    adapter,
                    detail: format!("PTY observation: {error}"),
                })?;
        let pty_stream = reads
            .iter()
            .flat_map(|read| read.bytes.iter().copied())
            .collect();
        Ok(ProcessCapture {
            exit_code,
            input_timestamps: inputs,
            checkpoints,
            raw_reads,
            observations,
            action_sends,
            pty_stream,
        })
    })();
    if let Err(error) = &result {
        if let Err(diagnostic_error) = write_failure(
            evidence_dir,
            adapter,
            pid,
            lifecycle_phase,
            &action_timeline,
            &stream,
            error,
        ) {
            tracker.record_error(diagnostic_error.to_string());
        }
    }
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
