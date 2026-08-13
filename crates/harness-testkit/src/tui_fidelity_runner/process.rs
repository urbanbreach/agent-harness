use std::fs::OpenOptions;
use std::path::Path;
use std::thread;
use std::time::{Duration, Instant};

use portable_pty::{native_pty_system, CommandBuilder, MasterPty};
use serde::Deserialize;

use super::actions::apply_action_with_frame;
use super::cleanup::CleanupTracker;
use super::error::RunnerError;
use super::interaction_queue;
use super::lifecycle_diagnostics::write_failure;
use super::process_checkpoints::capture as capture_checkpoints;
use super::process_io::{configure_environment, pty_size, spawn_reader};
use super::process_readiness::{wait_for_readiness, wait_for_stable_frame};
use super::process_wait::{
    collect_descendants, drain, process_error, request_normal_exit, semantic_frame, wait_for_text,
    wait_for_text_absent, wait_for_text_pair, wait_until,
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

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SchedulingReadiness {
    schema_version: String,
    sample_sequence: u64,
    sampled_at_micros: u64,
    ready_depth: usize,
    queued_depth: usize,
    deferred_ready: bool,
    stream_active: bool,
}

fn wait_for_literal_backlog(
    path: &Path,
    deadline: Instant,
    last_sample_sequence: &mut Option<u64>,
) -> Result<(), RunnerError> {
    loop {
        if readiness_sample(path, last_sample_sequence)? {
            return Ok(());
        }
        if Instant::now() >= deadline {
            return Err(RunnerError::Io {
                path: path.to_path_buf(),
                detail: "timed out waiting for fresh literal live backlog".to_owned(),
            });
        }
        thread::sleep(Duration::from_millis(1));
    }
}

#[expect(
    clippy::too_many_arguments,
    reason = "combined scheduling and semantic readiness is one PTY boundary"
)]
fn wait_for_literal_backlog_and_text_pair(
    path: &Path,
    last_sample_sequence: &mut Option<u64>,
    first: &str,
    second: &str,
    viewport: Viewport,
    deadline: Instant,
    adapter: AdapterKind,
    child: &mut Box<dyn portable_pty::Child + Send + Sync>,
    output: &std::sync::mpsc::Receiver<super::process_io::PtyRead>,
    stream: &mut Vec<u8>,
    observed: &mut std::collections::BTreeSet<u32>,
    pid: u32,
) -> Result<crate::parity::SemanticFrame, RunnerError> {
    loop {
        drain(output, stream);
        collect_descendants(pid, observed);
        if let Some(status) = child
            .try_wait()
            .map_err(|error| process_error(adapter, "poll combined readiness", error))?
        {
            return Err(RunnerError::PrematureExit {
                adapter,
                code: i32::try_from(status.exit_code()).unwrap_or(i32::MAX),
            });
        }
        let frame = semantic_frame(stream, viewport);
        let text_ready = super::semantic_actions::find_text(&frame, first).is_some()
            && super::semantic_actions::find_text(&frame, second).is_some();
        if text_ready && readiness_sample(path, last_sample_sequence)? {
            return Ok(frame);
        }
        if Instant::now() >= deadline {
            return Err(RunnerError::Io {
                path: path.to_path_buf(),
                detail: format!(
                    "timed out waiting for fresh literal backlog with {first} and {second}"
                ),
            });
        }
        thread::sleep(Duration::from_millis(1));
    }
}

fn readiness_sample(
    path: &Path,
    last_sample_sequence: &mut Option<u64>,
) -> Result<bool, RunnerError> {
    let Ok(bytes) = std::fs::read(path) else {
        return Ok(false);
    };
    let sample: SchedulingReadiness =
        serde_json::from_slice(&bytes).map_err(|error| RunnerError::Io {
            path: path.to_path_buf(),
            detail: format!("invalid scheduling readiness signal: {error}"),
        })?;
    let literal_depth = sample
        .queued_depth
        .saturating_add(usize::from(sample.deferred_ready));
    if sample.schema_version != "harness.packet2-scheduling-readiness.v1"
        || sample.ready_depth != literal_depth
    {
        return Err(RunnerError::Io {
            path: path.to_path_buf(),
            detail: "untruthful scheduling readiness signal".to_owned(),
        });
    }
    if sample.ready_depth >= 16
        && sample.stream_active
        && Some(sample.sample_sequence) != *last_sample_sequence
    {
        *last_sample_sequence = Some(sample.sample_sequence);
        let _ = sample.sampled_at_micros;
        return Ok(true);
    }
    Ok(false)
}

fn action_requires_literal_backlog(
    adapter: AdapterKind,
    fixture_active: bool,
    action: &crate::tui_fidelity::ScenarioAction,
) -> bool {
    adapter == AdapterKind::Harness
        && fixture_active
        && !matches!(
            action,
            crate::tui_fidelity::ScenarioAction::WaitForSemanticState(_)
                | crate::tui_fidelity::ScenarioAction::WaitForText(_)
                | crate::tui_fidelity::ScenarioAction::ClickText(_)
        )
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
    fixture_base_url: Option<&str>,
    packet2_scheduling: bool,
) -> Result<ProcessCapture, RunnerError> {
    let child_evidence_dir = if evidence_dir.is_absolute() {
        evidence_dir.to_path_buf()
    } else {
        std::env::current_dir()
            .map_err(|error| RunnerError::Io {
                path: evidence_dir.to_path_buf(),
                detail: format!("resolve runner cwd for evidence: {error}"),
            })?
            .join(evidence_dir)
    };
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
    if fixture_base_url.is_some() {
        let isolated_home = runtime_dir.join("home");
        std::fs::create_dir_all(&isolated_home).map_err(|error| RunnerError::Io {
            path: isolated_home.clone(),
            detail: format!("create isolated runtime home: {error}"),
        })?;
        command.env("HOME", &isolated_home);
        command.env("XDG_CONFIG_HOME", isolated_home.join(".config"));
    }
    if adapter == AdapterKind::Harness {
        command.arg("tui");
        if let Some(base_url) = fixture_base_url {
            let config_path = write_packet2_config(runtime_dir, base_url)?;
            command.args(["--config", config_path.to_string_lossy().as_ref()]);
            command.env("PACKET2_API_KEY", "packet2-local-only");
        } else {
            command.arg("--mock");
        }
        command.args([
            "--deterministic",
            "--session-dir",
            runtime_dir.join("sessions").to_string_lossy().as_ref(),
        ]);
    } else if let Some(base_url) = fixture_base_url {
        command.args(["--always-approve", "--no-leader"]);
        command.env("XAI_API_KEY", "test-key-for-ci");
        command.env("GROK_HOME", runtime_dir.join("home/.grok"));
        command.env("GROK_XAI_API_BASE_URL", base_url);
        command.env("GROK_CLI_CHAT_PROXY_BASE_URL", base_url);
        command.env("GROK_TELEMETRY_ENABLED", "false");
        command.env("GROK_FEEDBACK_ENABLED", "false");
        command.env("GROK_TRACE_UPLOAD", "false");
    }
    configure_environment(&mut command, scenario.terminal_type);
    let interaction_queue_path = child_evidence_dir.join("interaction-ids");
    let scheduling_readiness_path = child_evidence_dir.join("scheduling-readiness.json");
    if adapter == AdapterKind::Harness {
        command.env(
            "TUI_FIDELITY_PRESENTATION_TRACE",
            child_evidence_dir.join("native-presentation.json"),
        );
        command.env("TUI_FIDELITY_INTERACTION_QUEUE", &interaction_queue_path);
        if packet2_scheduling {
            command.env(
                "TUI_FIDELITY_SCHEDULING_TRACE",
                child_evidence_dir.join("scheduling.json"),
            );
            command.env(
                "TUI_FIDELITY_SCHEDULING_READINESS",
                &scheduling_readiness_path,
            );
        }
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
        if packet2_scheduling {
            super::actions::write_input(writer.as_mut(), b"start packet2 fixture\r", adapter)?;
            let (child, observed) = guard.parts_mut(adapter)?;
            wait_for_text(
                crate::tui_fidelity_fixture::STREAM_SENTINEL,
                scenario.viewport,
                deadline,
                adapter,
                child,
                &output,
                &mut stream,
                observed,
                pid,
            )?;
        }
        let start = Instant::now();

        let mut action_viewport = scenario.viewport;
        let mut disclosure_open = false;
        let mut readiness_sample_sequence = None;
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
            if action_requires_literal_backlog(adapter, packet2_scheduling, action) {
                wait_for_literal_backlog(
                    &scheduling_readiness_path,
                    deadline,
                    &mut readiness_sample_sequence,
                )?;
            }
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
            } else if let crate::tui_fidelity::ScenarioAction::WaitForText(wait) = action {
                let (child, observed) = guard.parts_mut(adapter)?;
                wait_for_text(
                    &wait.text,
                    action_viewport,
                    deadline,
                    adapter,
                    child,
                    &output,
                    &mut stream,
                    observed,
                    pid,
                )?;
            } else {
                let prepared_frame = if adapter == AdapterKind::Harness {
                    if let crate::tui_fidelity::ScenarioAction::ClickText(click) = action {
                        let disclosure = if disclosure_open { "▾" } else { "▸" };
                        let (child, observed) = guard.parts_mut(adapter)?;
                        Some(wait_for_literal_backlog_and_text_pair(
                            &scheduling_readiness_path,
                            &mut readiness_sample_sequence,
                            &click.text,
                            disclosure,
                            action_viewport,
                            deadline,
                            adapter,
                            child,
                            &output,
                            &mut stream,
                            observed,
                            pid,
                        )?)
                    } else {
                        None
                    }
                } else {
                    None
                };
                if let Some(queue) = interaction_queue.as_mut() {
                    interaction_queue::append(
                        queue,
                        &interaction_id,
                        action,
                        scenario.id.0.starts_with("packet3-baseline-"),
                    )
                    .map_err(|error| RunnerError::Io {
                        path: interaction_queue_path.clone(),
                        detail: format!("append typed interaction receipt: {error}"),
                    })?;
                }
                let frame = prepared_frame.unwrap_or_else(|| {
                    drain(&output, &mut stream);
                    semantic_frame(&stream, action_viewport)
                });
                let disclosure_click = matches!(
                    action,
                    crate::tui_fidelity::ScenarioAction::ClickText(click)
                        if click.text == crate::tui_fidelity_fixture::DISCLOSURE_SENTINEL
                );
                let sent_at = process_start.elapsed();
                if scenario.id.0.starts_with("packet3-baseline-") {
                    if let crate::tui_fidelity::ScenarioAction::TypeText(typed) = action {
                        super::actions::write_input(
                            writer.as_mut(),
                            typed.text.as_bytes(),
                            adapter,
                        )?;
                        let (child, observed) = guard.parts_mut(adapter)?;
                        wait_for_text(
                            &typed.text,
                            action_viewport,
                            deadline,
                            adapter,
                            child,
                            &output,
                            &mut stream,
                            observed,
                            pid,
                        )?;
                    } else {
                        apply_action_with_frame(
                            action,
                            adapter,
                            pair.master.as_ref(),
                            writer.as_mut(),
                            Some(&frame),
                        )?;
                    }
                } else {
                    apply_action_with_frame(
                        action,
                        adapter,
                        pair.master.as_ref(),
                        writer.as_mut(),
                        Some(&frame),
                    )?;
                }
                if let crate::tui_fidelity::ScenarioAction::Resize(resize) = action {
                    std::thread::sleep(Duration::from_millis(resize.dwell_millis));
                }
                if disclosure_click && disclosure_open {
                    let (child, observed) = guard.parts_mut(adapter)?;
                    if adapter == AdapterKind::Harness {
                        wait_for_text_pair(
                            crate::tui_fidelity_fixture::DISCLOSURE_SENTINEL,
                            "▸",
                            action_viewport,
                            deadline,
                            adapter,
                            child,
                            &output,
                            &mut stream,
                            observed,
                            pid,
                        )?;
                    } else {
                        wait_for_text_absent(
                            crate::tui_fidelity_fixture::DISCLOSURE_BODY,
                            action_viewport,
                            deadline,
                            adapter,
                            child,
                            &output,
                            &mut stream,
                            observed,
                            pid,
                        )?;
                    }
                }
                if disclosure_click {
                    disclosure_open = !disclosure_open;
                }
                if let crate::tui_fidelity::ScenarioAction::Resize(resize) = action {
                    action_viewport = resize.viewport;
                }
                if !matches!(
                    action,
                    crate::tui_fidelity::ScenarioAction::WaitForSemanticState(_)
                        | crate::tui_fidelity::ScenarioAction::WaitForText(_)
                        | crate::tui_fidelity::ScenarioAction::TerminalReply(_)
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
                            u64::try_from(sent_at.as_micros()).unwrap_or(u64::MAX),
                        ),
                        transport_drained_at: None,
                    });
                }
            }
            action_timeline.push(serde_json::json!({
                "kind": action.kind_name(),
                "at_tick": action.at_tick().0,
                "elapsed_millis": start.elapsed().as_millis(),
            }));
            inputs.push(start.elapsed());
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

fn write_packet2_config(
    runtime_dir: &Path,
    base_url: &str,
) -> Result<std::path::PathBuf, RunnerError> {
    let path = runtime_dir.join("packet2-harness.json");
    let config = serde_json::json!({
        "provider": {
            "packet2": {
                "type": "openai_compatible",
                "name": "Packet 2 loopback",
                "options": {
                    "baseURL": base_url,
                    "apiKeyEnv": ["PACKET2_API_KEY"],
                    "timeoutMs": 20000
                },
                "models": {
                    "fixture": {
                        "name": "Fixture",
                        "metadata": {"supportsToolCalls": true},
                        "limit": {"context": 16000, "input": 16000, "output": 8000}
                    }
                }
            }
        },
        "model": "packet2/fixture",
        "agent": {"default": {"model": "packet2/fixture"}},
        "permission": "allow"
    });
    std::fs::write(
        &path,
        serde_json::to_vec_pretty(&config).map_err(|error| RunnerError::Io {
            path: path.clone(),
            detail: error.to_string(),
        })?,
    )
    .map_err(|error| RunnerError::Io {
        path: path.clone(),
        detail: error.to_string(),
    })?;
    Ok(path)
}

#[cfg(test)]
mod readiness_scope_tests {
    use super::action_requires_literal_backlog;
    use crate::tui_fidelity::{
        AdapterKind, KeyCode, KeyModifiers, KeySpec, ScenarioAction, Tick, TimedKeyAction,
    };

    #[test]
    fn baseline_harness_actions_do_not_require_packet2_backlog() {
        // Given: an ordinary Harness input action outside the Packet 2 fixture.
        let action = ScenarioAction::TimedKey(TimedKeyAction {
            at_tick: Tick(1),
            key: KeySpec {
                code: KeyCode::Char('x'),
                modifiers: KeyModifiers {
                    shift: false,
                    alt: false,
                    ctrl: false,
                    meta: false,
                },
            },
        });

        // When: the action's scheduling handshake scope is selected.
        let baseline = action_requires_literal_backlog(AdapterKind::Harness, false, &action);

        // Then: only Harness actions backed by the Packet 2 fixture require backlog.
        assert!(!baseline);
        assert!(action_requires_literal_backlog(
            AdapterKind::Harness,
            true,
            &action
        ));
        assert!(!action_requires_literal_backlog(
            AdapterKind::Grok,
            true,
            &action
        ));
    }
}
