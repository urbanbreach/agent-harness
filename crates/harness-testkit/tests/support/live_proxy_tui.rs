use std::cmp;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{Receiver, RecvTimeoutError};
use std::thread;
use std::time::{Duration, Instant};

use portable_pty::{CommandBuilder, PtySize};
use serde_json::{json, Value};
use vt100::Parser as VtParser;

use super::live_events::{resolve_tagged_run_dir, ToolFlowEvidence};
use super::live_provider_parity::{
    assert_events_show_successful_provider_turn, collect_provider_turn_observation,
    provider_turn_summary,
};
use super::live_proxy_config::{
    session_namespace_name, LivePromptRequest, LiveToolFlowRunConfig, PromptRunConfig,
};
use super::live_visual::{
    default_live_run_metadata, selected_live_viewport, FocusCapture, LiveVisualRun,
    LiveVisualRunOptions, CHECKPOINT_DRAFT_VISIBLE, CHECKPOINT_FILE_WRITE_FINISHED,
    CHECKPOINT_HASHLINE_SCAN_FINISHED, CHECKPOINT_RUN_FINISHED, CHECKPOINT_STARTUP,
};
use super::pty_process::{spawn_pty_process, SpawnedPtyProcess};

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct LivePromptVisualArtifacts {
    pub(crate) visual_run_dir: PathBuf,
    pub(crate) manifest_json_path: PathBuf,
    pub(crate) startup_png: PathBuf,
    pub(crate) draft_visible_png: PathBuf,
    pub(crate) run_finished_png: PathBuf,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct LivePromptSmokeResult {
    pub(crate) events_body: String,
    pub(crate) visual_artifacts: LivePromptVisualArtifacts,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct LiveToolFlowArtifacts {
    pub(crate) tool_flow_run_dir: PathBuf,
    pub(crate) tool_flow_workspace_root: PathBuf,
    pub(crate) visual_run_dir: PathBuf,
    pub(crate) manifest_json_path: PathBuf,
    pub(crate) manifest_jsonl_path: PathBuf,
    pub(crate) startup_png: PathBuf,
    pub(crate) draft_visible_png: PathBuf,
    pub(crate) edit_create_finished_png: PathBuf,
    pub(crate) hashline_scan_finished_png: PathBuf,
    pub(crate) run_finished_png: PathBuf,
}

pub(crate) fn write_live_tool_flow_summary_artifacts(
    artifacts: &LiveToolFlowArtifacts,
    evidence: &ToolFlowEvidence,
    run_config: &LiveToolFlowRunConfig,
) -> Result<(), String> {
    let events_path = artifacts.tool_flow_run_dir.join("events.jsonl");
    let events_body = fs::read_to_string(&events_path)
        .map_err(|err| format!("failed to read {}: {err}", events_path.display()))?;
    let provider_turn = provider_turn_summary(
        &run_config.tool_flow.provider_name,
        &collect_provider_turn_observation(&events_body),
    )?;
    let summary_json = evidence.summary_json(std::slice::from_ref(&artifacts.tool_flow_run_dir))?;
    let summary_json = json!({
        "visual_run_dir": artifacts.visual_run_dir.display().to_string(),
        "manifest_json_path": artifacts.manifest_json_path.display().to_string(),
        "manifest_jsonl_path": artifacts.manifest_jsonl_path.display().to_string(),
        "final_png": artifacts.run_finished_png.display().to_string(),
        "workspace_root": artifacts.tool_flow_workspace_root.display().to_string(),
        "canonical_relative_path": run_config.canonical_relative_path.display().to_string(),
        "provider": run_config.tool_flow.provider_name,
        "model": run_config.tool_flow.model_id.clone(),
        "variant": run_config.tool_flow.variant.clone(),
        "profile": run_config.tool_flow.profile.clone(),
        "provider_turn": provider_turn,
        "summary": summary_json,
    });
    let summary_json_path = artifacts
        .visual_run_dir
        .join(crate::LIVE_TOOL_FLOW_SUMMARY_JSON);
    fs::write(
        &summary_json_path,
        serde_json::to_string_pretty(&summary_json)
            .map_err(|err| format!("failed to serialize tool-flow summary JSON: {err}"))?,
    )
    .map_err(|err| format!("failed to write {}: {err}", summary_json_path.display()))?;

    let final_content = fs::read_to_string(
        artifacts
            .tool_flow_workspace_root
            .join(&run_config.canonical_relative_path),
    )
    .map_err(|err| format!("failed to read final tool-flow content for summary: {err}"))?;
    let summary_txt = [
        format!("Visual run dir: {}", artifacts.visual_run_dir.display()),
        format!("Manifest: {}", artifacts.manifest_json_path.display()),
        format!("Final screenshot: {}", artifacts.run_finished_png.display()),
        format!(
            "Workspace root: {}",
            artifacts.tool_flow_workspace_root.display()
        ),
        format!(
            "Workspace file: {}",
            run_config.canonical_relative_path.display()
        ),
        format!("Provider: {}", run_config.tool_flow.provider_name),
        format!("Model: {}", run_config.tool_flow.model_id),
        format!(
            "Variant: {}",
            run_config
                .tool_flow
                .variant
                .as_deref()
                .unwrap_or("<primary>")
        ),
        format!(
            "Provider turn: {}",
            provider_turn
                .get("observation")
                .and_then(|observation| observation.get("completion_mode"))
                .and_then(Value::as_str)
                .unwrap_or("unknown")
        ),
        "Sequence:".to_string(),
        evidence
            .sequence_summary_lines()
            .into_iter()
            .map(|line| format!("  - {line}"))
            .collect::<Vec<_>>()
            .join("\n"),
        "Final content:".to_string(),
        final_content,
    ]
    .join("\n");
    let summary_txt_path = artifacts
        .visual_run_dir
        .join(crate::LIVE_TOOL_FLOW_SUMMARY_TXT);
    fs::write(&summary_txt_path, summary_txt)
        .map_err(|err| format!("failed to write {}: {err}", summary_txt_path.display()))?;

    Ok(())
}

pub(crate) fn run_live_tui_smoke(
    request: &LivePromptRequest,
    run_config: &PromptRunConfig,
    timeout: Duration,
) -> Result<LivePromptSmokeResult, String> {
    let harness_bin = crate::resolve_harness_bin();
    let session_dir = run_config.session_dir.clone();
    let mut live_visual = LiveVisualRun::new_with_options(
        "live_proxy_e2e_tui_prompt_responses_smoke",
        &crate::live_run_id()?,
        LiveVisualRunOptions {
            run_metadata: default_live_run_metadata(
                &run_config.provider_name,
                &run_config.model_id,
                run_config.variant.as_deref(),
                &run_config.profile,
                &run_config.workspace_root,
                &run_config.session_dir,
            ),
            ..LiveVisualRunOptions::default()
        },
    )?;

    let mut command = CommandBuilder::new(harness_bin.to_string_lossy().as_ref());
    command.arg("tui");
    command.arg("--exit-on-finish");
    command.arg("--config");
    command.arg(run_config.config_path.to_string_lossy().to_string());
    command.arg("--profile");
    command.arg(run_config.profile.clone());
    command.arg("--session-dir");
    command.arg(session_dir.to_string_lossy().to_string());
    command.cwd(&run_config.workspace_root);
    configure_live_tui_env(&mut command);

    let mut process = spawn_pty_process(tui_pty_size(), command, "live TUI smoke")?;

    wait_for_screen_contains(
        &mut process.parser,
        &process.output_rx,
        crate::LIVE_TUI_READY_MARKER,
        crate::LIVE_TUI_STARTUP_TIMEOUT,
    )?;
    let startup_checkpoint = live_visual.capture_checkpoint_with_metadata(
        CHECKPOINT_STARTUP,
        &process.parser,
        crate::LIVE_VISUAL_STARTUP_MARKERS,
        &FocusCapture::anchored_exact(crate::LIVE_TUI_READY_MARKER, 24, 3),
        Some(json!({
            "purpose": "startup-ready",
            "session_dir": run_config.session_dir.display().to_string(),
        })),
    )?;

    process
        .writer
        .write_all(request.prompt_text.as_bytes())
        .map_err(|err| format!("failed to type live TUI smoke prompt: {err}"))?;
    process
        .writer
        .flush()
        .map_err(|err| format!("failed to flush live TUI smoke prompt: {err}"))?;
    wait_for_screen_contains(
        &mut process.parser,
        &process.output_rx,
        &request.prompt_text,
        Duration::from_secs(5),
    )?;
    let draft_visible_checkpoint = live_visual.capture_checkpoint_with_metadata(
        CHECKPOINT_DRAFT_VISIBLE,
        &process.parser,
        &[crate::LIVE_TUI_READY_MARKER, request.prompt_text.as_str()],
        &FocusCapture::anchored(request.prompt_text.as_str(), 28, 4),
        Some(json!({
            "purpose": "draft-visible",
            "prompt_preview": request.prompt_text,
        })),
    )?;

    process
        .writer
        .write_all(b"\r")
        .map_err(|err| format!("failed to submit live TUI smoke prompt: {err}"))?;
    process
        .writer
        .flush()
        .map_err(|err| format!("failed to flush submitted live TUI smoke prompt: {err}"))?;

    let events_body =
        wait_for_tui_provider_turn(&session_dir, crate::LIVE_TUI_SESSION_NAMESPACE, timeout)?;
    wait_for_screen_state(
        &mut process.parser,
        &process.output_rx,
        &[
            crate::LIVE_TUI_STATUS_SUCCESS_MARKER,
            crate::LIVE_TUI_FINISHED_MARKER,
        ],
        &[
            crate::LIVE_TUI_ASSISTANT_STREAMING_MARKER,
            crate::LIVE_TUI_WAITING_FOR_RESPONSE_MARKER,
        ],
        Duration::from_secs(5),
    )?;
    let run_finished_checkpoint = live_visual.capture_checkpoint_with_metadata(
        CHECKPOINT_RUN_FINISHED,
        &process.parser,
        &[
            crate::LIVE_TUI_STATUS_SUCCESS_MARKER,
            crate::LIVE_TUI_FINISHED_MARKER,
            crate::LIVE_TUI_ASSISTANT_STREAMING_MARKER,
            crate::LIVE_TUI_WAITING_FOR_RESPONSE_MARKER,
            request.prompt_text.as_str(),
        ],
        &FocusCapture::anchored_exact(crate::LIVE_TUI_READY_MARKER, 28, 6),
        Some(json!({
            "purpose": "prompt-run-finished",
            "session_dir": run_config.session_dir.display().to_string(),
        })),
    )?;
    process
        .writer
        .write_all(b"\tq")
        .map_err(|err| format!("failed to quit live TUI smoke cleanly: {err}"))?;
    process
        .writer
        .flush()
        .map_err(|err| format!("failed to flush live TUI smoke quit key: {err}"))?;

    wait_for_tui_process_exit(
        &mut process.child,
        &process.output_rx,
        &mut process.parser,
        Duration::from_secs(10),
    )?;

    Ok(LivePromptSmokeResult {
        events_body,
        visual_artifacts: LivePromptVisualArtifacts {
            visual_run_dir: live_visual.run_dir().to_path_buf(),
            manifest_json_path: run_finished_checkpoint.manifest_json_path().to_path_buf(),
            startup_png: startup_checkpoint.png_path().to_path_buf(),
            draft_visible_png: draft_visible_checkpoint.png_path().to_path_buf(),
            run_finished_png: run_finished_checkpoint.png_path().to_path_buf(),
        },
    })
}

pub(crate) fn run_live_tui_tool_flow(
    run_config: &LiveToolFlowRunConfig,
    timeout: Duration,
) -> Result<LiveToolFlowArtifacts, String> {
    let mut live_visual = LiveVisualRun::new_with_options(
        run_config.visual_test_name(),
        &crate::live_run_id()?,
        LiveVisualRunOptions {
            run_metadata: default_live_run_metadata(
                &run_config.tool_flow.provider_name,
                &run_config.tool_flow.model_id,
                run_config.tool_flow.variant.as_deref(),
                &run_config.tool_flow.profile,
                &run_config.tool_flow.workspace_root,
                &run_config.tool_flow.session_dir,
            ),
            ..LiveVisualRunOptions::default()
        },
    )?;
    let deadline = Instant::now() + timeout;
    let mut stage = spawn_live_tui_stage_process(&run_config.tool_flow)?;
    wait_for_screen_contains(
        &mut stage.parser,
        &stage.output_rx,
        crate::LIVE_TUI_READY_MARKER,
        remaining_before(deadline, "create-stage ready marker")?,
    )?;
    let startup_checkpoint = live_visual.capture_checkpoint_with_metadata(
        CHECKPOINT_STARTUP,
        &stage.parser,
        crate::LIVE_VISUAL_STARTUP_MARKERS,
        &FocusCapture::anchored_exact(crate::LIVE_TUI_READY_MARKER, 24, 3),
        Some(json!({
            "purpose": "tool-flow-startup",
            "session_dir": run_config.tool_flow.session_dir.display().to_string(),
        })),
    )?;
    type_and_flush_live_prompt(&mut stage.writer, crate::LIVE_TOOL_FLOW_CREATE_PROMPT)?;
    wait_for_screen_contains(
        &mut stage.parser,
        &stage.output_rx,
        crate::LIVE_TOOL_FLOW_DRAFT_MARKER,
        remaining_before(deadline, "tool-flow draft marker")?,
    )?;
    let draft_visible_checkpoint = live_visual.capture_checkpoint_with_metadata(
        CHECKPOINT_DRAFT_VISIBLE,
        &stage.parser,
        &[
            crate::LIVE_TUI_READY_MARKER,
            crate::LIVE_TOOL_FLOW_DRAFT_MARKER,
        ],
        &FocusCapture::anchored(crate::LIVE_TOOL_FLOW_DRAFT_MARKER, 32, 5),
        Some(json!({
            "purpose": "tool-flow-draft-visible",
            "prompt_preview": crate::LIVE_TOOL_FLOW_CREATE_PROMPT,
        })),
    )?;
    submit_live_prompt(&mut stage.writer)?;

    let tool_flow_namespace = session_namespace_name(&run_config.tool_flow.session_dir)?;
    let tool_flow_run_dir = wait_for_tool_flow_tool_call_succeeded(
        &run_config.tool_flow.session_dir,
        &run_config.canonical_relative_path,
        &tool_flow_namespace,
        "edit",
        1,
        remaining_before(deadline, "create edit completion")?,
    )?;
    let create_events = wait_for_tui_provider_turn_count(
        &run_config.tool_flow.session_dir,
        &tool_flow_namespace,
        1,
        remaining_before(deadline, "create-stage provider turn completion")?,
    )?;
    assert_events_show_successful_provider_turn(
        &run_config.tool_flow.provider_name,
        &create_events,
    );
    let tool_flow_workspace_root = read_run_workspace_root(&tool_flow_run_dir)?;
    wait_for_screen_contains(
        &mut stage.parser,
        &stage.output_rx,
        "edit",
        Duration::from_secs(5),
    )?;
    let edit_create_finished_checkpoint = live_visual.capture_checkpoint_with_metadata(
        CHECKPOINT_FILE_WRITE_FINISHED,
        &stage.parser,
        &[
            crate::LIVE_TUI_READY_MARKER,
            "edit",
            crate::LIVE_TOOL_FLOW_RELATIVE_PATH,
        ],
        &FocusCapture::anchored_exact("edit", 28, 5),
        Some(json!({
            "purpose": "tool-flow-stage-finished",
            "stage": "create",
            "stage_tool": "edit",
            "session_dir": run_config.tool_flow.session_dir.display().to_string(),
        })),
    )?;
    wait_for_live_tui_idle(
        &mut stage.parser,
        &stage.output_rx,
        remaining_before(deadline, "tool-flow ready for first read")?,
    )?;

    type_and_flush_live_prompt(&mut stage.writer, crate::LIVE_TOOL_FLOW_READ_PROMPT)?;
    submit_live_prompt(&mut stage.writer)?;
    wait_for_tool_flow_tool_call_succeeded(
        &run_config.tool_flow.session_dir,
        &run_config.canonical_relative_path,
        &tool_flow_namespace,
        "read",
        1,
        remaining_before(deadline, "first read completion")?,
    )?;
    let first_read_events = wait_for_tui_provider_turn_count(
        &run_config.tool_flow.session_dir,
        &tool_flow_namespace,
        2,
        remaining_before(deadline, "first-read provider turn completion")?,
    )?;
    assert_events_show_successful_provider_turn(
        &run_config.tool_flow.provider_name,
        &first_read_events,
    );
    wait_for_live_tui_idle(
        &mut stage.parser,
        &stage.output_rx,
        remaining_before(deadline, "tool-flow ready for edit")?,
    )?;

    let edit_prompt = crate::live_tool_flow_edit_prompt();
    type_and_flush_live_prompt(&mut stage.writer, &edit_prompt)?;
    submit_live_prompt(&mut stage.writer)?;
    wait_for_tool_flow_tool_call_succeeded(
        &run_config.tool_flow.session_dir,
        &run_config.canonical_relative_path,
        &tool_flow_namespace,
        "edit",
        1,
        remaining_before(deadline, "edit completion")?,
    )?;
    wait_for_screen_contains(
        &mut stage.parser,
        &stage.output_rx,
        "edit",
        Duration::from_secs(5),
    )?;
    let edit_events = wait_for_tui_provider_turn_count(
        &run_config.tool_flow.session_dir,
        &tool_flow_namespace,
        3,
        remaining_before(deadline, "edit-stage provider turn completion")?,
    )?;
    assert_events_show_successful_provider_turn(&run_config.tool_flow.provider_name, &edit_events);
    let hashline_scan_finished_checkpoint = live_visual.capture_checkpoint_with_metadata(
        CHECKPOINT_HASHLINE_SCAN_FINISHED,
        &stage.parser,
        &[
            crate::LIVE_TUI_READY_MARKER,
            "edit",
            crate::LIVE_TOOL_FLOW_RELATIVE_PATH,
        ],
        &FocusCapture::anchored_exact("edit", 32, 5),
        Some(json!({
            "purpose": "tool-flow-stage-finished",
            "stage": "edit",
            "stage_tool": "edit",
            "session_dir": run_config.tool_flow.session_dir.display().to_string(),
        })),
    )?;
    wait_for_live_tui_idle(
        &mut stage.parser,
        &stage.output_rx,
        remaining_before(deadline, "tool-flow ready for final read")?,
    )?;

    type_and_flush_live_prompt(&mut stage.writer, crate::LIVE_TOOL_FLOW_FINAL_READ_PROMPT)?;
    submit_live_prompt(&mut stage.writer)?;
    wait_for_tool_flow_tool_call_succeeded(
        &run_config.tool_flow.session_dir,
        &run_config.canonical_relative_path,
        &tool_flow_namespace,
        "read",
        2,
        remaining_before(deadline, "final verification read completion")?,
    )?;
    let final_read_events = wait_for_tui_provider_turn_count(
        &run_config.tool_flow.session_dir,
        &tool_flow_namespace,
        4,
        remaining_before(deadline, "final-read provider turn completion")?,
    )?;
    assert_events_show_successful_provider_turn(
        &run_config.tool_flow.provider_name,
        &final_read_events,
    );
    wait_for_screen_contains(
        &mut stage.parser,
        &stage.output_rx,
        "read",
        Duration::from_secs(5),
    )?;
    wait_for_screen_state(
        &mut stage.parser,
        &stage.output_rx,
        &[
            crate::LIVE_TUI_STATUS_SUCCESS_MARKER,
            crate::LIVE_TUI_FINISHED_MARKER,
            "read",
        ],
        &[
            crate::LIVE_TUI_ASSISTANT_STREAMING_MARKER,
            crate::LIVE_TUI_WAITING_FOR_RESPONSE_MARKER,
        ],
        remaining_before(deadline, "final-read visible done state")?,
    )?;
    let run_finished_checkpoint = live_visual.capture_checkpoint_with_metadata(
        CHECKPOINT_RUN_FINISHED,
        &stage.parser,
        &[
            crate::LIVE_TUI_STATUS_SUCCESS_MARKER,
            crate::LIVE_TUI_FINISHED_MARKER,
            crate::LIVE_TUI_ASSISTANT_STREAMING_MARKER,
            crate::LIVE_TUI_WAITING_FOR_RESPONSE_MARKER,
            "read",
        ],
        &FocusCapture::anchored_exact("read", 28, 5),
        Some(json!({
            "purpose": "tool-flow-stage-finished",
            "stage": "final_read",
            "stage_tool": "read",
            "session_dir": run_config.tool_flow.session_dir.display().to_string(),
        })),
    )?;
    finish_live_tui_stage_process(
        &mut stage,
        remaining_before(deadline, "final-read-stage process exit")?,
    )?;

    Ok(LiveToolFlowArtifacts {
        tool_flow_run_dir,
        tool_flow_workspace_root,
        visual_run_dir: live_visual.run_dir().to_path_buf(),
        manifest_json_path: run_finished_checkpoint.manifest_json_path().to_path_buf(),
        manifest_jsonl_path: run_finished_checkpoint.manifest_jsonl_path().to_path_buf(),
        startup_png: startup_checkpoint.png_path().to_path_buf(),
        draft_visible_png: draft_visible_checkpoint.png_path().to_path_buf(),
        edit_create_finished_png: edit_create_finished_checkpoint.png_path().to_path_buf(),
        hashline_scan_finished_png: hashline_scan_finished_checkpoint.png_path().to_path_buf(),
        run_finished_png: run_finished_checkpoint.png_path().to_path_buf(),
    })
}

pub(crate) fn live_tui_command_timeout(request: &LivePromptRequest) -> Duration {
    let wait_timeout_ms = request
        .wait_timeout_ms
        .trim()
        .parse::<u64>()
        .unwrap_or_else(|_| {
            crate::DEFAULT_LIVE_PROXY_WAIT_TIMEOUT_MS
                .parse::<u64>()
                .expect("default live proxy wait timeout must parse as u64")
        });
    Duration::from_millis(wait_timeout_ms).saturating_add(Duration::from_secs(20))
}

type LiveTuiStageProcess = SpawnedPtyProcess;

fn spawn_live_tui_stage_process(
    run_config: &PromptRunConfig,
) -> Result<LiveTuiStageProcess, String> {
    let harness_bin = crate::resolve_harness_bin();

    let mut command = CommandBuilder::new(harness_bin.to_string_lossy().as_ref());
    command.arg("tui");
    command.arg("--config");
    command.arg(run_config.config_path.to_string_lossy().to_string());
    command.arg("--profile");
    command.arg(run_config.profile.clone());
    command.arg("--session-dir");
    command.arg(run_config.session_dir.to_string_lossy().to_string());
    command.cwd(&run_config.workspace_root);
    configure_live_tui_env(&mut command);

    spawn_pty_process(tui_pty_size(), command, "live TUI tool-flow stage")
}

fn finish_live_tui_stage_process(
    process: &mut LiveTuiStageProcess,
    timeout: Duration,
) -> Result<(), String> {
    process
        .writer
        .write_all(b"\tq")
        .map_err(|err| format!("failed to send live TUI tool-flow stage quit sequence: {err}"))?;
    process
        .writer
        .flush()
        .map_err(|err| format!("failed to flush live TUI tool-flow stage quit sequence: {err}"))?;
    wait_for_tui_process_exit(
        &mut process.child,
        &process.output_rx,
        &mut process.parser,
        timeout,
    )
}

fn remaining_before(deadline: Instant, step: &str) -> Result<Duration, String> {
    deadline
        .checked_duration_since(Instant::now())
        .ok_or_else(|| format!("timed out before completing {step}"))
}

fn wait_for_tool_flow_tool_call_succeeded(
    session_dir: &Path,
    canonical_relative_path: &Path,
    tool_flow_session_namespace: &str,
    tool_id: &str,
    minimum_successes: usize,
    timeout: Duration,
) -> Result<PathBuf, String> {
    let deadline = Instant::now() + timeout;

    loop {
        if let Ok(run_dir) = resolve_tagged_run_dir(session_dir, tool_flow_session_namespace) {
            let events_path = run_dir.join("events.jsonl");
            if events_path.exists() {
                let events_body = fs::read_to_string(&events_path).map_err(|err| {
                    format!(
                        "failed to read tool-flow events {}: {err}",
                        events_path.display()
                    )
                })?;
                match tool_flow_tool_call_state(
                    &events_body,
                    canonical_relative_path,
                    tool_id,
                    minimum_successes,
                )? {
                    ToolFlowToolCallState::Succeeded => return Ok(run_dir),
                    ToolFlowToolCallState::Failed(status) => {
                        return Err(format!(
                            "tool-flow call `{tool_id}` for {} finished with status `{status}`\n{}",
                            canonical_relative_path.display(),
                            describe_session_events_state(session_dir, tool_flow_session_namespace)
                        ));
                    }
                    ToolFlowToolCallState::Pending => {}
                }
            }
        }

        if Instant::now() >= deadline {
            return Err(format!(
                "timed out waiting for tool-flow call `{tool_id}` for {} under {} after {timeout:?}\n{}",
                canonical_relative_path.display(),
                session_dir.display(),
                describe_session_events_state(session_dir, tool_flow_session_namespace)
            ));
        }

        thread::sleep(crate::LIVE_TUI_READ_POLL_TIMEOUT);
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ToolFlowToolCallState {
    Pending,
    Succeeded,
    Failed(String),
}

pub(crate) fn tool_flow_tool_call_state(
    events_body: &str,
    canonical_relative_path: &Path,
    expected_tool_id: &str,
    minimum_successes: usize,
) -> Result<ToolFlowToolCallState, String> {
    let canonical_path = canonical_relative_path.display().to_string();
    let mut matching_call_ids = Vec::new();
    let mut success_count = 0usize;

    for (idx, line) in events_body.lines().enumerate() {
        if line.trim().is_empty() {
            continue;
        }

        let event: Value = serde_json::from_str(line)
            .map_err(|err| format!("events line {} is invalid JSON: {err}", idx + 1))?;
        let event_type = event
            .get("payload")
            .and_then(|payload| payload.get("event_type"))
            .and_then(Value::as_str)
            .unwrap_or_default();
        let data = event
            .get("payload")
            .and_then(|payload| payload.get("data"))
            .cloned()
            .unwrap_or(Value::Null);

        match event_type {
            "run_failed" => {
                let error = data
                    .get("error")
                    .and_then(Value::as_str)
                    .unwrap_or("run_failed event missing error detail");
                return Err(format!(
                    "tool-flow run failed before `{expected_tool_id}`: {error}"
                ));
            }
            "tool_call_requested" => {
                let Some(tool_id) = data.get("tool_id").and_then(Value::as_str) else {
                    continue;
                };
                if tool_id != expected_tool_id {
                    continue;
                }

                let Some(tool_call_id) = data.get("tool_call_id").and_then(Value::as_str) else {
                    continue;
                };
                let Some(args_summary) = data.get("args_summary").and_then(Value::as_str) else {
                    continue;
                };

                if tool_call_targets_path(tool_id, args_summary, &canonical_path) {
                    matching_call_ids.push(tool_call_id.to_string());
                }
            }
            "tool_call_finished" => {
                let Some(tool_call_id) = data.get("tool_call_id").and_then(Value::as_str) else {
                    continue;
                };
                if !matching_call_ids
                    .iter()
                    .any(|candidate| candidate == tool_call_id)
                {
                    continue;
                }

                let status = data
                    .get("status")
                    .and_then(Value::as_str)
                    .unwrap_or("missing")
                    .to_string();
                if status != "succeeded" {
                    return Ok(ToolFlowToolCallState::Failed(status));
                }

                if expected_tool_id == "bash"
                    && data
                        .get("output_json")
                        .and_then(|output| output.get("success"))
                        .and_then(Value::as_bool)
                        == Some(false)
                {
                    let shell_status = data
                        .get("output_json")
                        .and_then(|output| output.get("status"))
                        .and_then(Value::as_i64)
                        .map(|code| code.to_string())
                        .unwrap_or_else(|| "unknown".to_string());
                    return Ok(ToolFlowToolCallState::Failed(format!(
                        "shell_exit_{shell_status}"
                    )));
                }

                success_count += 1;
            }
            _ => {}
        }
    }

    if success_count >= minimum_successes {
        Ok(ToolFlowToolCallState::Succeeded)
    } else {
        Ok(ToolFlowToolCallState::Pending)
    }
}

fn type_and_flush_live_prompt(
    writer: &mut Box<dyn Write + Send>,
    prompt: &str,
) -> Result<(), String> {
    writer
        .write_all(prompt.as_bytes())
        .map_err(|err| format!("failed to type live TUI tool-flow prompt: {err}"))?;
    writer
        .flush()
        .map_err(|err| format!("failed to flush live TUI tool-flow prompt: {err}"))
}

fn submit_live_prompt(writer: &mut Box<dyn Write + Send>) -> Result<(), String> {
    writer
        .write_all(b"\r")
        .map_err(|err| format!("failed to submit live TUI tool-flow prompt: {err}"))?;
    writer
        .flush()
        .map_err(|err| format!("failed to flush submitted live TUI tool-flow prompt: {err}"))
}

fn wait_for_live_tui_idle(
    parser: &mut VtParser,
    output_rx: &Receiver<Vec<u8>>,
    timeout: Duration,
) -> Result<(), String> {
    wait_for_screen_state(
        parser,
        output_rx,
        &[crate::LIVE_TUI_FINISHED_MARKER],
        &[
            crate::LIVE_TUI_ASSISTANT_STREAMING_MARKER,
            crate::LIVE_TUI_WAITING_FOR_RESPONSE_MARKER,
        ],
        timeout,
    )
    .map(|_| ())
}

fn read_run_workspace_root(run_dir: &Path) -> Result<PathBuf, String> {
    let events_path = run_dir.join("events.jsonl");
    let events_body = fs::read_to_string(&events_path)
        .map_err(|err| format!("failed to read {}: {err}", events_path.display()))?;

    for (idx, line) in events_body.lines().enumerate() {
        if line.trim().is_empty() {
            continue;
        }
        let event: Value = serde_json::from_str(line)
            .map_err(|err| format!("events line {} is invalid JSON: {err}", idx + 1))?;
        let event_type = event
            .get("payload")
            .and_then(|payload| payload.get("event_type"))
            .and_then(Value::as_str)
            .unwrap_or_default();
        if event_type != "run_started" {
            continue;
        }
        let workspace_root = event
            .get("payload")
            .and_then(|payload| payload.get("data"))
            .and_then(|data| data.get("workspace_root"))
            .and_then(Value::as_str)
            .ok_or_else(|| {
                format!(
                    "run_started missing workspace_root in {}",
                    events_path.display()
                )
            })?;
        return Ok(PathBuf::from(workspace_root));
    }

    Err(format!(
        "run_started with workspace_root not found in {}",
        events_path.display()
    ))
}

fn tool_call_targets_path(tool_id: &str, args_summary: &str, canonical_path: &str) -> bool {
    let args_json = serde_json::from_str::<Value>(args_summary).ok();

    match tool_id {
        "read" | "edit" => args_json
            .as_ref()
            .and_then(|value| value.get("path").or_else(|| value.get("filePath")))
            .and_then(Value::as_str)
            .map(|path| path == canonical_path)
            .unwrap_or_else(|| args_summary.contains(canonical_path)),
        "bash" => args_json
            .as_ref()
            .map(|value| json_value_contains_path(value, canonical_path))
            .unwrap_or_else(|| args_summary.contains(canonical_path)),
        _ => false,
    }
}

fn json_value_contains_path(value: &Value, canonical_path: &str) -> bool {
    match value {
        Value::Null | Value::Bool(_) | Value::Number(_) => false,
        Value::String(text) => text.contains(canonical_path),
        Value::Array(items) => items
            .iter()
            .any(|item| json_value_contains_path(item, canonical_path)),
        Value::Object(map) => map
            .values()
            .any(|entry| json_value_contains_path(entry, canonical_path)),
    }
}

fn describe_session_events_state(session_dir: &Path, session_namespace: &str) -> String {
    let resolved = resolve_tagged_run_dir(session_dir, session_namespace)
        .ok()
        .or_else(|| latest_run_dir_under(session_dir));
    let Some(run_dir) = resolved else {
        return format!(
            "no run dir resolved yet under {} for namespace `{session_namespace}`",
            session_dir.display()
        );
    };

    let events_path = run_dir.join("events.jsonl");
    if !events_path.exists() {
        return format!(
            "latest run dir {} exists but events.jsonl is not present yet",
            run_dir.display()
        );
    }

    match fs::read_to_string(&events_path) {
        Ok(events_body) => {
            let provider = collect_provider_turn_observation(&events_body);
            format!(
                "latest run dir: {}\nevents: {}\nprovider_started={} provider_finished={} deltas={} completion_mode={} task_completed_summary_present={} run_failed={}\nlast events:\n{}",
                run_dir.display(),
                events_path.display(),
                provider.saw_started,
                provider.saw_finished,
                provider.delta_count,
                provider.completion_mode(),
                provider
                    .task_completed_summary
                    .as_deref()
                    .map(str::trim)
                    .map(|value| !value.is_empty())
                    .unwrap_or(false),
                provider.run_failed.as_deref().unwrap_or("none"),
                tail_lines(&events_body, 12),
            )
        }
        Err(err) => format!(
            "latest run dir: {}\nfailed to read {}: {err}",
            run_dir.display(),
            events_path.display()
        ),
    }
}

fn latest_run_dir_under(session_dir: &Path) -> Option<PathBuf> {
    let mut dirs = fs::read_dir(session_dir)
        .ok()?
        .filter_map(|entry| entry.ok())
        .filter_map(|entry| {
            let path = entry.path();
            if !path.is_dir() {
                return None;
            }
            let modified = entry.metadata().ok()?.modified().ok()?;
            Some((modified, path))
        })
        .collect::<Vec<_>>();
    dirs.sort_by_key(|(modified, _)| *modified);
    dirs.pop().map(|(_, path)| path)
}

fn tail_lines(text: &str, count: usize) -> String {
    text.lines()
        .rev()
        .take(count)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect::<Vec<_>>()
        .join("\n")
}

fn tui_pty_size() -> PtySize {
    let viewport = selected_live_viewport();
    PtySize {
        rows: viewport.rows,
        cols: viewport.cols,
        pixel_width: 0,
        pixel_height: 0,
    }
}

fn configure_live_tui_env(command: &mut CommandBuilder) {
    command.env("HARNESS_DISABLE_ANIMATIONS", "1");
    command.env("TERM", "xterm-256color");
    command.env("LANG", "C.UTF-8");
    command.env("LC_ALL", "C.UTF-8");
    command.env("TZ", "UTC");
}

fn wait_for_screen_contains(
    parser: &mut VtParser,
    output_rx: &Receiver<Vec<u8>>,
    needle: &str,
    timeout: Duration,
) -> Result<String, String> {
    let deadline = Instant::now() + timeout;

    loop {
        drain_pty_output(parser, output_rx);

        let current = tui_screen_contents(parser);
        if current.contains(needle) {
            return Ok(stabilize_tui_screen(parser, output_rx, current));
        }

        let now = Instant::now();
        if now >= deadline {
            return Err(format!(
                "timed out waiting for TUI screen marker `{needle}` after {timeout:?}; final screen:\n{current}"
            ));
        }

        let wait_timeout = cmp::min(
            crate::LIVE_TUI_READ_POLL_TIMEOUT,
            deadline.saturating_duration_since(now),
        );
        match output_rx.recv_timeout(wait_timeout) {
            Ok(chunk) => parser.process(&chunk),
            Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => {
                return Err(format!(
                    "TUI PTY output closed while waiting for `{needle}`; last screen:\n{current}"
                ));
            }
        }
    }
}

fn wait_for_screen_state(
    parser: &mut VtParser,
    output_rx: &Receiver<Vec<u8>>,
    required_markers: &[&str],
    forbidden_markers: &[&str],
    timeout: Duration,
) -> Result<String, String> {
    let deadline = Instant::now() + timeout;

    loop {
        drain_pty_output(parser, output_rx);

        let current = tui_screen_contents(parser);
        let has_required = required_markers
            .iter()
            .all(|marker| current.contains(marker));
        let has_forbidden = forbidden_markers
            .iter()
            .any(|marker| current.contains(marker));

        if has_required && !has_forbidden {
            return Ok(stabilize_tui_screen(parser, output_rx, current));
        }

        let now = Instant::now();
        if now >= deadline {
            return Err(format!(
                "timed out waiting for final TUI state after {timeout:?}; required={required_markers:?}; forbidden={forbidden_markers:?}; final screen:\n{current}"
            ));
        }

        let wait_timeout = cmp::min(
            crate::LIVE_TUI_READ_POLL_TIMEOUT,
            deadline.saturating_duration_since(now),
        );
        match output_rx.recv_timeout(wait_timeout) {
            Ok(chunk) => parser.process(&chunk),
            Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => {
                return Err(format!(
                    "TUI PTY output closed while waiting for final state; required={required_markers:?}; forbidden={forbidden_markers:?}; last screen:\n{current}"
                ));
            }
        }
    }
}

fn stabilize_tui_screen(
    parser: &mut VtParser,
    output_rx: &Receiver<Vec<u8>>,
    initial: String,
) -> String {
    let mut latest = initial;
    let mut stable_since = Instant::now();
    let deadline = Instant::now() + crate::LIVE_TUI_STABLE_TIMEOUT;

    loop {
        let now = Instant::now();
        if now >= deadline {
            return latest;
        }

        let wait_timeout = cmp::min(
            crate::LIVE_TUI_READ_POLL_TIMEOUT,
            deadline.saturating_duration_since(now),
        );
        match output_rx.recv_timeout(wait_timeout) {
            Ok(chunk) => parser.process(&chunk),
            Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => return latest,
        }

        let current = tui_screen_contents(parser);
        if current != latest {
            latest = current;
            stable_since = Instant::now();
            continue;
        }

        if Instant::now().saturating_duration_since(stable_since) >= crate::LIVE_TUI_STABLE_WINDOW {
            return latest;
        }
    }
}

fn drain_pty_output(parser: &mut VtParser, output_rx: &Receiver<Vec<u8>>) {
    while let Ok(chunk) = output_rx.try_recv() {
        parser.process(&chunk);
    }
}

fn tui_screen_contents(parser: &VtParser) -> String {
    parser.screen().contents()
}

fn wait_for_tui_process_exit(
    child: &mut Box<dyn portable_pty::Child + Send>,
    output_rx: &Receiver<Vec<u8>>,
    parser: &mut VtParser,
    timeout: Duration,
) -> Result<(), String> {
    let deadline = Instant::now() + timeout;

    loop {
        drain_pty_output(parser, output_rx);

        match child
            .try_wait()
            .map_err(|err| format!("failed to poll live TUI smoke process: {err}"))?
        {
            Some(status) if status.success() => return Ok(()),
            Some(status) => {
                return Err(format!(
                    "live TUI smoke exited with status {:?}; final screen:\n{}",
                    status.exit_code(),
                    tui_screen_contents(parser)
                ));
            }
            None => {}
        }

        let now = Instant::now();
        if now >= deadline {
            child
                .kill()
                .map_err(|err| format!("failed to kill timed out live TUI smoke process: {err}"))?;
            return Err(format!(
                "timed out waiting for live TUI smoke to exit after {timeout:?}; final screen:\n{}",
                tui_screen_contents(parser)
            ));
        }

        let wait_timeout = cmp::min(
            crate::LIVE_TUI_READ_POLL_TIMEOUT,
            deadline.saturating_duration_since(now),
        );
        match output_rx.recv_timeout(wait_timeout) {
            Ok(chunk) => parser.process(&chunk),
            Err(RecvTimeoutError::Timeout | RecvTimeoutError::Disconnected) => {}
        }
    }
}

fn wait_for_tui_provider_turn(
    session_dir: &Path,
    session_namespace: &str,
    timeout: Duration,
) -> Result<String, String> {
    wait_for_tui_provider_turn_count(session_dir, session_namespace, 1, timeout)
}

fn wait_for_tui_provider_turn_count(
    session_dir: &Path,
    session_namespace: &str,
    minimum_completed_turns: usize,
    timeout: Duration,
) -> Result<String, String> {
    let deadline = Instant::now() + timeout;

    loop {
        if let Ok(run_dir) = resolve_tagged_run_dir(session_dir, session_namespace) {
            let events_path = run_dir.join("events.jsonl");
            if events_path.exists() {
                let events_body = fs::read_to_string(&events_path).map_err(|err| {
                    format!(
                        "failed to read TUI smoke events {}: {err}",
                        events_path.display()
                    )
                })?;
                let observation = collect_provider_turn_observation(&events_body);
                if let Some(run_failed) = observation.run_failed.as_deref() {
                    return Err(format!(
                        "live TUI smoke run failed before provider completion: {run_failed}\n{}",
                        describe_session_events_state(session_dir, session_namespace)
                    ));
                }
                if completed_provider_task_count(&events_body) >= minimum_completed_turns {
                    return Ok(events_body);
                }
            }
        }

        if Instant::now() >= deadline {
            return Err(format!(
                "timed out waiting for provider turn evidence under {} after {timeout:?}\n{}",
                session_dir.display(),
                describe_session_events_state(session_dir, session_namespace)
            ));
        }

        thread::sleep(crate::LIVE_TUI_READ_POLL_TIMEOUT);
    }
}

fn completed_provider_task_count(events_body: &str) -> usize {
    let mut scheduled = std::collections::BTreeMap::<String, bool>::new();
    let mut completed = 0usize;

    for line in events_body.lines() {
        if line.trim().is_empty() {
            continue;
        }
        let Ok(event) = serde_json::from_str::<Value>(line) else {
            continue;
        };
        let event_type = event
            .get("payload")
            .and_then(|payload| payload.get("event_type"))
            .and_then(Value::as_str)
            .unwrap_or_default();
        let data = event
            .get("payload")
            .and_then(|payload| payload.get("data"))
            .cloned()
            .unwrap_or(Value::Null);

        match event_type {
            "task_scheduled" => {
                if data
                    .get("queue_key")
                    .and_then(Value::as_str)
                    .is_some_and(|queue_key| queue_key.starts_with("provider_model:"))
                {
                    if let Some(task_id) = data.get("task_id").and_then(Value::as_str) {
                        scheduled.insert(task_id.to_string(), false);
                    }
                }
            }
            "task_completed" => {
                let Some(task_id) = data.get("task_id").and_then(Value::as_str) else {
                    continue;
                };
                let Some(seen) = scheduled.get_mut(task_id) else {
                    continue;
                };
                if !*seen {
                    *seen = true;
                    completed += 1;
                }
            }
            _ => {}
        }
    }

    completed
}
