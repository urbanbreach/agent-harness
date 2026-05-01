use std::env;
use std::ffi::OsStr;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};
use time::{macros::format_description, OffsetDateTime};

mod support;

use serde_json::{json, Value};
use support::live_events::{
    assert_event_log_contains, assert_question_state_matches, assert_requested_tool_args,
    assert_requested_tool_sequence, assert_run_records_live_runtime_context,
    assert_todo_state_matches, assert_tool_call_output_contains, first_requested_tool_args,
    resolve_tagged_run_dir, ToolFlowEvidence,
};
use support::live_provider_parity::{
    assert_events_show_successful_provider_turn, assert_provider_turn_completed,
    collect_provider_turn_observation, provider_turn_summary,
};
use support::live_proxy_config::{
    load_json5_config, prepare_live_prompt_chat_tool_run_config,
    prepare_live_prompt_native_tool_flow_run_config, prepare_live_prompt_run_config,
    prepare_live_tool_flow_run_config, prepare_prompt_run_config, provider_api_mode,
    provider_from_config, resolve_env_reference_value, resolve_live_prompt_request,
    resolve_live_proxy_config_path, resolve_live_vision_proxy_config, run_live_prompt_stage,
    run_live_proxy_preflight, LiveNamespaceAllocation, LivePromptRequest, LiveToolFlowNamespaces,
    LiveToolFlowRunConfig,
};
use support::live_proxy_tui::{
    live_tui_command_timeout, read_hashline_scan_line_hash, run_live_tui_smoke,
    run_live_tui_tool_flow, tool_flow_tool_call_state, write_live_tool_flow_summary_artifacts,
    LiveToolFlowArtifacts, ToolFlowToolCallState,
};
use support::live_vision::{self, LiveVisionProxyConfig};
use support::live_visual::{
    assert_checkpoint_markers, parser_with_screen, selected_live_viewport, write_tiny_png,
    FocusCapture, LiveVisualRun, LiveVisualRunOptions, CHECKPOINT_DRAFT_VISIBLE,
    CHECKPOINT_FILE_WRITE_FINISHED, CHECKPOINT_HASHLINE_SCAN_FINISHED,
    CHECKPOINT_PERMISSION_REQUESTED, CHECKPOINT_RUN_FINISHED, CHECKPOINT_STARTUP,
};
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

const DEFAULT_LIVE_PROXY_PROVIDER: &str = "default";
const DEFAULT_LIVE_PROXY_MODEL: &str = "gpt-5.4-mini";
const DEFAULT_LIVE_PROXY_VARIANT: &str = "low";
const DEFAULT_LIVE_PROXY_PROFILE: &str = "live_proxy_smoke";
const LIVE_PROXY_TOOL_FLOW_PROFILE: &str = "live_proxy_tool_flow";
const LIVE_PROXY_CHAT_TODO_FLOW_PROFILE: &str = "live_proxy_chat_todo_flow";
const LIVE_PROXY_CHAT_QUESTION_PROFILE: &str = "live_proxy_chat_question";
const LIVE_PROXY_CHAT_SKILL_PROFILE: &str = "live_proxy_chat_skill";
const LIVE_PROXY_VISION_VERIFIER_PROFILE: &str = "live_proxy_vision_verifier";
const LIVE_TUI_SESSION_NAMESPACE: &str = "live-proxy-tui-session";
const TOOL_FLOW_SESSION_NAMESPACE: &str = "tool-flow-session";
const VISION_VERIFIER_SESSION_NAMESPACE: &str = "vision-verifier-session";
#[expect(
    dead_code,
    reason = "reserved for the ignored live visual-verifier lane"
)]
const LIVE_TUI_VISUAL_VERIFIER_SESSION_NAMESPACE: &str = "live-proxy-visual-verifier-session";
#[expect(
    dead_code,
    reason = "reserved for the ignored live visual-verifier lane"
)]
const TOOL_FLOW_VISUAL_VERIFIER_SESSION_NAMESPACE: &str = "tool-flow-visual-verifier-session";
#[expect(
    dead_code,
    reason = "reserved for the ignored live visual-verifier lane"
)]
const VISION_VISUAL_VERIFIER_SESSION_NAMESPACE: &str = "vision-visual-verifier-session";
const LIVE_PROXY_TUI_TOOL_FLOW_TEST_NAME: &str = "live_proxy_e2e_tui_tool_flow";
#[expect(
    dead_code,
    reason = "reserved for the ignored live visual-verifier lane"
)]
const LIVE_PROXY_VISUAL_VERIFIER_TEST_NAME: &str = "live_proxy_e2e_visual_verifier";
const LIVE_TOOL_FLOW_RELATIVE_PATH: &str = "tmp/live_tool_flow.md";
const LIVE_TOOL_FLOW_DRAFT_MARKER: &str =
    "You must use tools only. Use exactly tmp/live_tool_flow.md.";
const LIVE_TOOL_FLOW_FINAL_CONTENT: &str = "alpha\nBETA\ngamma\n";
const LIVE_TOOL_FLOW_APPLY_EDIT_ID: &str = "live-tool-flow-apply";
const LIVE_CHAT_TODO_CONTENT: &str = "live chat todo item";
const LIVE_CHAT_TODO_FLOW_PROMPT: &str = concat!(
    "Use tools before the final answer. ",
    "First call todowrite with exactly one todo item: ",
    r#"[{\"content\":\"live chat todo item\",\"status\":\"pending\",\"priority\":\"high\"}]"#,
    ". After the tool call, reply with exactly LIVE_CHAT_TODO_CONFIRMED and nothing else."
);
const LIVE_CHAT_QUESTION_ANSWERS: &str = r#"[["Yes"]]"#;
const LIVE_CHAT_QUESTION_PROMPT: &str = concat!(
    "Call question with exactly one question using header=Choice and options Yes/No. ",
    "Use this exact payload shape: ",
    r#"[{\"question\":\"Pick one\",\"header\":\"Choice\",\"options\":[{\"label\":\"Yes\",\"description\":\"Choose yes\"},{\"label\":\"No\",\"description\":\"Choose no\"}]}]"#,
    ". After the tool call, reply with exactly LIVE_CHAT_QUESTION_CONFIRMED and nothing else."
);
const LIVE_CHAT_SKILL_PROMPT: &str = concat!(
    "Call skill with name=rust-best-practices. ",
    "After the tool call, reply with exactly LIVE_CHAT_SKILL_CONFIRMED and nothing else."
);
const LIVE_TOOL_FLOW_CREATE_PROMPT: &str = concat!(
    "You must use tools only. Use exactly tmp/live_tool_flow.md. ",
    "Now perform only step 1: call write with this exact payload shape: ",
    r#"{"filePath":"tmp/live_tool_flow.md","content":"alpha\nbeta\ngamma\n"}"#,
    ". Return exactly one write tool call and zero prose. Do not call any other tool."
);
const LIVE_TOOL_FLOW_READ_PROMPT: &str = concat!(
    "Now perform only step 2 on the same file: call read with filePath=tmp/live_tool_flow.md. ",
    "Return exactly one read tool call and zero prose. Do not call any other tool."
);
const LIVE_TOOL_FLOW_SCAN_PROMPT: &str = concat!(
    "Now perform only step 3 on the same file: call edit.hashline_scan with path=tmp/live_tool_flow.md start_line=1 limit=20. ",
    "Return exactly one edit.hashline_scan tool call and zero prose. Do not call any other tool."
);
const LIVE_TOOL_FLOW_FINAL_READ_PROMPT: &str = concat!(
    "Now perform steps 5 and 6 only: call read with filePath=tmp/live_tool_flow.md again, then summarize the final contents. ",
    "Do not make any more edits and do not use any other file path. Before the summary, there must be exactly one read tool call."
);
const DEFAULT_LIVE_PROXY_PROMPT: &str = "Say hello in exactly five words.";
const DEFAULT_LIVE_PROXY_WAIT_TIMEOUT_MS: &str = "120000";
const RESPONSES_ENDPOINT_PATH: &str = "/v1/responses";
const LIVE_TUI_READY_MARKER: &str = "Ask anything...";
const LIVE_TUI_STATUS_SUCCESS_MARKER: &str = "Live_proxy";
const LIVE_TUI_FINISHED_MARKER: &str = "live ctx";
const LIVE_TUI_ASSISTANT_STREAMING_MARKER: &str = "assistant · streaming…";
const LIVE_TUI_WAITING_FOR_RESPONSE_MARKER: &str = "Waiting for response…";
const LIVE_TUI_READ_POLL_TIMEOUT: Duration = Duration::from_millis(50);
const LIVE_TUI_STABLE_WINDOW: Duration = Duration::from_millis(180);
const WIREMOCK_REQUEST_SETTLE_TIMEOUT: Duration = Duration::from_secs(2);
const LIVE_TUI_STABLE_TIMEOUT: Duration = Duration::from_secs(2);
const LIVE_TUI_STARTUP_TIMEOUT: Duration = Duration::from_secs(10);
const LIVE_VISUAL_STARTUP_MARKERS: &[&str] = &[LIVE_TUI_READY_MARKER];
const LIVE_TOOL_FLOW_SUMMARY_JSON: &str = "run_summary.json";
const LIVE_TOOL_FLOW_SUMMARY_TXT: &str = "run_summary.txt";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct LiveVisionCheckpointContract {
    checkpoint_id: &'static str,
    expected_markers: &'static [&'static str],
}

#[test]
#[ignore = "requires HARNESS_LIVE_PROXY=1 and local CLIproxyAPI access"]
fn live_proxy_prompt_responses_smoke() {
    if env::var("HARNESS_LIVE_PROXY").as_deref() != Ok("1") {
        return;
    }

    let repo_root = repo_root();
    let live_request = resolve_live_prompt_request(&repo_root)
        .unwrap_or_else(|err| panic!("failed to resolve live proxy prompt inputs: {err}"));

    let run_config = prepare_live_prompt_run_config(&live_request)
        .unwrap_or_else(|err| panic!("failed to prepare live proxy prompt config: {err}"));

    let harness_bin = resolve_harness_bin();
    let events_path = unique_temp_file("live-proxy-events", "jsonl");

    let output = Command::new(&harness_bin)
        .arg("prompt")
        .arg("--text")
        .arg(&live_request.prompt_text)
        .arg("--profile")
        .arg(&run_config.profile)
        .arg("--config")
        .arg(&run_config.config_path)
        .arg("--out")
        .arg(&events_path)
        .env(
            "HARNESS_PROMPT_WAIT_TIMEOUT_MS",
            &live_request.wait_timeout_ms,
        )
        .current_dir(&repo_root)
        .output()
        .expect("spawn harness prompt");

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);

    assert!(
        output.status.success(),
        "harness prompt failed with status {:?}\nstdout:\n{}\nstderr:\n{}\nPrepared config: {}\nSelected profile: {}\nSelected model: {}\nSelected endpoint: {}\nHint: ensure CLIproxyAPI is running and reachable, HARNESS_LIVE_PROXY_MODEL (if set) is valid, and provider api_mode is responses or auto",
        output.status.code(),
        stdout,
        stderr,
        run_config.config_path.display(),
        run_config.profile,
        run_config.model_id,
        run_config.endpoint.path()
    );

    let events_body = fs::read_to_string(&events_path)
        .unwrap_or_else(|err| panic!("failed to read event log {}: {err}", events_path.display()));
    assert_events_show_successful_provider_turn(&live_request.provider_name, &events_body);
}

#[test]
#[ignore = "requires HARNESS_LIVE_PROXY=1 and local CLIproxyAPI access"]
fn live_proxy_prompt_parity_signoff() {
    if env::var("HARNESS_LIVE_PROXY").as_deref() != Ok("1") {
        return;
    }

    println!(
        "CLI parity signoff: live_proxy_prompt_responses_smoke -> live_proxy_prompt_chat_tool_flow -> live_proxy_prompt_native_tool_flow"
    );
    live_proxy_prompt_responses_smoke();
    live_proxy_prompt_chat_tool_flow();
    live_proxy_prompt_native_tool_flow();
}

#[test]
#[ignore = "requires HARNESS_LIVE_PROXY=1 and local CLIproxyAPI access"]
fn live_proxy_preflight() {
    if env::var("HARNESS_LIVE_PROXY").as_deref() != Ok("1") {
        return;
    }

    let report = run_live_proxy_preflight(&repo_root())
        .unwrap_or_else(|err| panic!("live proxy preflight failed: {err}"));
    println!("{}", report.summary_text());
}

#[test]
#[ignore = "requires HARNESS_LIVE_PROXY=1 and local CLIproxyAPI access"]
fn live_proxy_e2e_tui_prompt_responses_smoke() {
    if !cfg!(target_os = "linux") || env::var("HARNESS_LIVE_PROXY").as_deref() != Ok("1") {
        return;
    }

    let repo_root = repo_root();
    let live_request = resolve_live_prompt_request(&repo_root)
        .unwrap_or_else(|err| panic!("failed to resolve live proxy TUI inputs: {err}"));

    let run_config = prepare_live_prompt_run_config(&live_request)
        .unwrap_or_else(|err| panic!("failed to prepare live proxy TUI config: {err}"));

    let smoke = run_live_tui_smoke(
        &live_request,
        &run_config,
        live_tui_command_timeout(&live_request),
    )
    .unwrap_or_else(|err| panic!("live proxy TUI smoke failed: {err}"));
    assert_events_show_successful_provider_turn(&live_request.provider_name, &smoke.events_body);
}

#[test]
#[ignore = "requires HARNESS_LIVE_PROXY=1 and local CLIproxyAPI access"]
fn live_proxy_e2e_tui_parity_signoff() {
    if !cfg!(target_os = "linux") || env::var("HARNESS_LIVE_PROXY").as_deref() != Ok("1") {
        return;
    }

    println!(
        "TUI parity signoff: live_proxy_preflight -> live_proxy_e2e_tui_prompt_responses_smoke -> live_proxy_e2e_tui_tool_flow"
    );
    live_proxy_preflight();
    live_proxy_e2e_tui_prompt_responses_smoke();
    live_proxy_e2e_tui_tool_flow();
}

#[test]
#[ignore = "requires HARNESS_LIVE_PROXY=1 and local CLIproxyAPI access"]
fn live_proxy_e2e_tui_tool_flow() {
    if !cfg!(target_os = "linux") || env::var("HARNESS_LIVE_PROXY").as_deref() != Ok("1") {
        return;
    }

    let repo_root = repo_root();
    let live_request = resolve_live_prompt_request(&repo_root)
        .unwrap_or_else(|err| panic!("failed to resolve live proxy tool-flow inputs: {err}"));
    let mut failures = Vec::new();
    for attempt in 1..=3 {
        let run_config = prepare_live_tool_flow_run_config(
            &live_request,
            LiveToolFlowNamespaces {
                live_tui_session: LIVE_TUI_SESSION_NAMESPACE,
                tool_flow_session: TOOL_FLOW_SESSION_NAMESPACE,
                vision_verifier_session: VISION_VERIFIER_SESSION_NAMESPACE,
                visual_test_name: LIVE_PROXY_TUI_TOOL_FLOW_TEST_NAME,
            },
        )
        .unwrap_or_else(|err| panic!("failed to prepare live proxy tool-flow config: {err}"));

        match run_live_proxy_tui_tool_flow_once(&live_request, &run_config) {
            Ok(()) => return,
            Err(err) => failures.push(format!("attempt {attempt}: {err}")),
        }
    }

    panic!(
        "live proxy TUI tool-flow failed after 3 attempts:\n{}",
        failures.join("\n\n")
    );
}

#[test]
#[ignore = "requires HARNESS_LIVE_PROXY=1 and local CLIproxyAPI access"]
fn live_proxy_prompt_chat_tool_flow() {
    if env::var("HARNESS_LIVE_PROXY").as_deref() != Ok("1") {
        return;
    }

    let repo_root = repo_root();
    let live_request = resolve_live_prompt_request(&repo_root)
        .unwrap_or_else(|err| panic!("failed to resolve live proxy chat-flow inputs: {err}"));
    let run_config = prepare_live_prompt_chat_tool_run_config(&live_request)
        .unwrap_or_else(|err| panic!("failed to prepare live proxy chat-flow config: {err}"));

    let todo_result = run_live_prompt_stage(
        &run_config.todo_flow,
        LIVE_CHAT_TODO_FLOW_PROMPT,
        &live_request.wait_timeout_ms,
        &[],
    )
    .unwrap_or_else(|err| panic!("live prompt todo-flow stage failed: {err}"));
    assert_requested_tool_sequence(&todo_result.events_body, &["todowrite"])
        .unwrap_or_else(|err| panic!("todo-flow tool sequence mismatch: {err}"));
    assert_requested_tool_args(
        &todo_result.events_body,
        "todowrite",
        &json!({
            "todos": [{
                "content": LIVE_CHAT_TODO_CONTENT,
                "status": "pending",
                "priority": "high",
            }]
        }),
    )
    .unwrap_or_else(|err| panic!("todo-flow tool arguments mismatch: {err}"));
    assert_todo_state_matches(&todo_result.run_dir).unwrap_or_else(|err| {
        panic!(
            "todo-flow todo state mismatch under {}: {err}",
            todo_result.run_dir.display()
        )
    });

    let question_result = run_live_prompt_stage(
        &run_config.question,
        LIVE_CHAT_QUESTION_PROMPT,
        &live_request.wait_timeout_ms,
        &[("HARNESS_QUESTION_ANSWERS", LIVE_CHAT_QUESTION_ANSWERS)],
    )
    .unwrap_or_else(|err| panic!("live prompt question stage failed: {err}"));
    assert_requested_tool_sequence(&question_result.events_body, &["question"])
        .unwrap_or_else(|err| panic!("question-stage tool sequence mismatch: {err}"));
    assert_requested_tool_args(
        &question_result.events_body,
        "question",
        &json!({
            "questions": [{
                "question": "Pick one",
                "header": "Choice",
                "options": [
                    {"label": "Yes", "description": "Choose yes"},
                    {"label": "No", "description": "Choose no"}
                ]
            }]
        }),
    )
    .unwrap_or_else(|err| panic!("question-stage tool arguments mismatch: {err}"));
    assert_tool_call_output_contains(
        &question_result.events_body,
        "question",
        "\"Pick one\"=\"Yes\"",
    )
    .unwrap_or_else(|err| panic!("question-stage answer evidence mismatch: {err}"));
    assert_event_log_contains(&question_result.events_body, "LIVE_CHAT_QUESTION_CONFIRMED")
        .unwrap_or_else(|err| panic!("question-stage final confirmation mismatch: {err}"));
    assert_question_state_matches(&question_result.run_dir, &question_result.events_body)
        .unwrap_or_else(|err| {
            panic!(
                "question-stage state mismatch under {}: {err}",
                question_result.run_dir.display()
            )
        });

    let skill_result = run_live_prompt_stage(
        &run_config.skill,
        LIVE_CHAT_SKILL_PROMPT,
        &live_request.wait_timeout_ms,
        &[],
    )
    .unwrap_or_else(|err| panic!("live prompt skill stage failed: {err}"));
    assert_requested_tool_sequence(&skill_result.events_body, &["skill"])
        .unwrap_or_else(|err| panic!("skill-stage tool sequence mismatch: {err}"));
    let skill_args = first_requested_tool_args(&skill_result.events_body, "skill")
        .unwrap_or_else(|err| panic!("skill-stage tool arguments lookup failed: {err}"))
        .unwrap_or_else(|| panic!("skill-stage tool arguments missing"));
    assert_eq!(
        skill_args.get("name").and_then(Value::as_str),
        Some("rust-best-practices"),
        "skill-stage tool name mismatch: {skill_args}"
    );
    if let Some(user_message) = skill_args.get("user_message") {
        assert!(
            user_message.is_null()
                || user_message
                    .as_str()
                    .is_some_and(|value| value.contains("rust-best-practices")),
            "skill-stage optional user_message mismatch: {skill_args}"
        );
    }
    assert_tool_call_output_contains(
        &skill_result.events_body,
        "skill",
        "# Skill: rust-best-practices",
    )
    .unwrap_or_else(|err| panic!("skill-stage skill output mismatch: {err}"));
    assert_event_log_contains(&skill_result.events_body, "LIVE_CHAT_SKILL_CONFIRMED")
        .unwrap_or_else(|err| panic!("skill-stage final confirmation mismatch: {err}"));
}

#[test]
#[ignore = "requires HARNESS_LIVE_PROXY=1 and local CLIproxyAPI access"]
fn live_proxy_prompt_native_tool_flow() {
    if env::var("HARNESS_LIVE_PROXY").as_deref() != Ok("1") {
        return;
    }

    let repo_root = repo_root();
    let live_request = resolve_live_prompt_request(&repo_root).unwrap_or_else(|err| {
        panic!("failed to resolve live proxy native tool-flow inputs: {err}")
    });
    let run_config =
        prepare_live_prompt_native_tool_flow_run_config(&live_request).unwrap_or_else(|err| {
            panic!("failed to prepare live proxy native tool-flow config: {err}")
        });

    let create_result = run_live_prompt_stage(
        &run_config.create,
        LIVE_TOOL_FLOW_CREATE_PROMPT,
        &live_request.wait_timeout_ms,
        &[],
    )
    .unwrap_or_else(|err| panic!("live prompt native tool-flow create stage failed: {err}"));
    assert_requested_tool_sequence(&create_result.events_body, &["write"])
        .unwrap_or_else(|err| panic!("native tool-flow create tool sequence mismatch: {err}"));
    assert_run_records_live_runtime_context(
        &create_result.run_dir,
        &run_config.create.profile,
        &live_request.primary_model,
        live_request.primary_variant.as_deref(),
    )
    .unwrap_or_else(|err| panic!("native tool-flow create runtime context mismatch: {err}"));

    let first_read_result = run_live_prompt_stage(
        &run_config.first_read,
        LIVE_TOOL_FLOW_READ_PROMPT,
        &live_request.wait_timeout_ms,
        &[],
    )
    .unwrap_or_else(|err| panic!("live prompt native tool-flow first-read stage failed: {err}"));
    assert_requested_tool_sequence(&first_read_result.events_body, &["read"])
        .unwrap_or_else(|err| panic!("native tool-flow first-read tool sequence mismatch: {err}"));

    let scan_result = run_live_prompt_stage(
        &run_config.scan,
        LIVE_TOOL_FLOW_SCAN_PROMPT,
        &live_request.wait_timeout_ms,
        &[],
    )
    .unwrap_or_else(|err| panic!("live prompt native tool-flow scan stage failed: {err}"));
    assert_requested_tool_sequence(&scan_result.events_body, &["edit.hashline_scan"])
        .unwrap_or_else(|err| panic!("native tool-flow scan tool sequence mismatch: {err}"));
    let line_two_hash =
        read_hashline_scan_line_hash(&scan_result.run_dir, &run_config.canonical_relative_path, 2)
            .unwrap_or_else(|err| panic!("native tool-flow scan hash evidence mismatch: {err}"));

    let apply_result = run_live_prompt_stage(
        &run_config.apply,
        &live_tool_flow_apply_prompt(&line_two_hash),
        &live_request.wait_timeout_ms,
        &[],
    )
    .unwrap_or_else(|err| panic!("live prompt native tool-flow apply stage failed: {err}"));
    assert_requested_tool_sequence(&apply_result.events_body, &["edit.hashline_apply"])
        .unwrap_or_else(|err| panic!("native tool-flow apply tool sequence mismatch: {err}"));

    let final_read_result = run_live_prompt_stage(
        &run_config.final_read,
        LIVE_TOOL_FLOW_FINAL_READ_PROMPT,
        &live_request.wait_timeout_ms,
        &[],
    )
    .unwrap_or_else(|err| panic!("live prompt native tool-flow final-read stage failed: {err}"));
    assert_requested_tool_sequence(&final_read_result.events_body, &["read"])
        .unwrap_or_else(|err| panic!("native tool-flow final-read tool sequence mismatch: {err}"));
    assert_event_log_contains(&final_read_result.events_body, "BETA")
        .unwrap_or_else(|err| panic!("native tool-flow final-read confirmation mismatch: {err}"));

    let evidence = ToolFlowEvidence::collect_many(
        &[
            create_result.run_dir.clone(),
            first_read_result.run_dir.clone(),
            scan_result.run_dir.clone(),
            apply_result.run_dir.clone(),
            final_read_result.run_dir.clone(),
        ],
        &run_config.create.workspace_root,
        &run_config.canonical_relative_path,
    )
    .unwrap_or_else(|err| panic!("native tool-flow evidence collection failed: {err}"));
    evidence
        .assert_run_succeeded()
        .unwrap_or_else(|err| panic!("native tool-flow run did not succeed: {err}"));
    evidence
        .assert_ordered_same_file_sequence()
        .unwrap_or_else(|err| panic!("native tool-flow same-file sequence mismatch: {err}"));
    evidence
        .assert_final_workspace_content(LIVE_TOOL_FLOW_FINAL_CONTENT)
        .unwrap_or_else(|err| panic!("native tool-flow final workspace content mismatch: {err}"));
}

fn run_live_proxy_tui_tool_flow_once(
    live_request: &LivePromptRequest,
    run_config: &LiveToolFlowRunConfig,
) -> Result<(), String> {
    let tool_flow_artifacts =
        run_live_tui_tool_flow(run_config, live_tui_command_timeout(live_request))?;

    let tool_flow_path = tool_flow_artifacts
        .tool_flow_workspace_root
        .join(&run_config.canonical_relative_path);
    if !tool_flow_path.exists() {
        return Err(format!(
            "canonical tool-flow file should exist after the run: {}",
            tool_flow_path.display()
        ));
    }
    let final_content = fs::read_to_string(&tool_flow_path).map_err(|err| {
        format!(
            "failed to read canonical tool-flow file {}: {err}",
            tool_flow_path.display()
        )
    })?;
    if !final_content.ends_with(LIVE_TOOL_FLOW_FINAL_CONTENT) {
        return Err(format!(
            "expected final tool-flow file to end with {:?}; actual contents:\n{}",
            LIVE_TOOL_FLOW_FINAL_CONTENT, final_content
        ));
    }

    let evidence = ToolFlowEvidence::collect(
        &tool_flow_artifacts.tool_flow_run_dir,
        &tool_flow_artifacts.tool_flow_workspace_root,
        &run_config.canonical_relative_path,
    )?;
    evidence.assert_run_succeeded()?;
    evidence.assert_ordered_same_file_sequence()?;
    evidence.assert_final_workspace_content(LIVE_TOOL_FLOW_FINAL_CONTENT)?;
    assert_final_visual_checkpoint(&tool_flow_artifacts)?;
    write_live_tool_flow_summary_artifacts(&tool_flow_artifacts, &evidence, run_config)?;
    Ok(())
}

#[tokio::test(flavor = "current_thread")]
#[ignore = "requires HARNESS_LIVE_PROXY=1 and local CLIproxyAPI access"]
async fn live_proxy_e2e_visual_verifier() {
    if !cfg!(target_os = "linux") || env::var("HARNESS_LIVE_PROXY").as_deref() != Ok("1") {
        return;
    }

    let repo_root = repo_root();
    let live_request = resolve_live_prompt_request(&repo_root)
        .unwrap_or_else(|err| panic!("failed to resolve live proxy visual-verifier inputs: {err}"));
    let run_config = prepare_live_prompt_run_config(&live_request)
        .unwrap_or_else(|err| panic!("failed to prepare live proxy visual-verifier config: {err}"));
    let smoke = run_live_tui_smoke(
        &live_request,
        &run_config,
        live_tui_command_timeout(&live_request),
    )
    .unwrap_or_else(|err| panic!("live proxy visual-verifier smoke failed: {err}"));
    assert_events_show_successful_provider_turn(&live_request.provider_name, &smoke.events_body);

    for png_path in [
        &smoke.visual_artifacts.startup_png,
        &smoke.visual_artifacts.draft_visible_png,
        &smoke.visual_artifacts.run_finished_png,
    ] {
        assert!(
            png_path.exists(),
            "expected live visual checkpoint PNG at {}",
            png_path.display()
        );
    }
    assert!(
        smoke.visual_artifacts.manifest_json_path.exists(),
        "expected live visual manifest at {}",
        smoke.visual_artifacts.manifest_json_path.display()
    );
    assert!(
        smoke.visual_artifacts.visual_run_dir.exists(),
        "expected live visual run dir at {}",
        smoke.visual_artifacts.visual_run_dir.display()
    );

    assert_checkpoint_markers(
        &smoke.visual_artifacts.manifest_json_path,
        CHECKPOINT_STARTUP,
        &[LIVE_TUI_READY_MARKER],
        &[],
    )
    .unwrap_or_else(|err| panic!("startup checkpoint markers mismatch: {err}"));
    assert_checkpoint_markers(
        &smoke.visual_artifacts.manifest_json_path,
        CHECKPOINT_DRAFT_VISIBLE,
        &[LIVE_TUI_READY_MARKER, live_request.prompt_text.as_str()],
        &[],
    )
    .unwrap_or_else(|err| panic!("draft-visible checkpoint markers mismatch: {err}"));
    assert_checkpoint_markers(
        &smoke.visual_artifacts.manifest_json_path,
        CHECKPOINT_RUN_FINISHED,
        &[
            LIVE_TUI_READY_MARKER,
            LIVE_TUI_STATUS_SUCCESS_MARKER,
            LIVE_TUI_FINISHED_MARKER,
            live_request.prompt_text.as_str(),
        ],
        &[],
    )
    .unwrap_or_else(|err| panic!("run-finished checkpoint markers mismatch: {err}"));
}

#[tokio::test(flavor = "current_thread")]
async fn live_proxy_prompt_wiremock_smoke_uses_responses_and_model_override() {
    let server = MockServer::start().await;
    let response_template = ResponseTemplate::new(200)
        .insert_header("content-type", "text/event-stream")
        .set_body_raw(deterministic_responses_sse_fixture(), "text/event-stream");

    Mock::given(method("POST"))
        .and(path(RESPONSES_ENDPOINT_PATH))
        .respond_with(response_template.clone())
        .mount(&server)
        .await;

    Mock::given(method("POST"))
        .and(path("/v1/chat/completions"))
        .respond_with(ResponseTemplate::new(500))
        .mount(&server)
        .await;

    let provider_name = "proxy";
    let configured_model = "configured-model";
    let overridden_model = "wiremock-model-override";
    let namespace = LiveNamespaceAllocation::allocate("live-proxy-wiremock")
        .expect("allocate wiremock namespace");
    let session_dir = namespace.session_dir("wiremock-session");
    let source_config_path = namespace.artifact_file("source-config", "jsonc");
    let source_config = build_live_proxy_test_config(
        provider_name,
        &server.uri(),
        "auto",
        configured_model,
        &session_dir,
    );
    fs::write(
        &source_config_path,
        serde_json::to_string_pretty(&source_config).expect("serialize wiremock config"),
    )
    .expect("write wiremock config");

    let run_config = prepare_prompt_run_config(
        &source_config_path,
        provider_name,
        overridden_model,
        None,
        "wiremock_live_profile",
    )
    .expect("prepare prompt run config");

    let harness_bin = resolve_harness_bin();
    let events_path = namespace.artifact_file("events", "jsonl");

    let harness_bin_for_run = harness_bin.clone();
    let events_path_for_run = events_path.clone();
    let run_config_for_run = run_config.clone();
    let output = tokio::task::spawn_blocking(move || {
        Command::new(&harness_bin_for_run)
            .env_clear()
            .arg("prompt")
            .arg("--text")
            .arg("Return hello from wiremock")
            .arg("--profile")
            .arg(&run_config_for_run.profile)
            .arg("--config")
            .arg(&run_config_for_run.config_path)
            .arg("--out")
            .arg(&events_path_for_run)
            .env(
                "HARNESS_PROMPT_WAIT_TIMEOUT_MS",
                DEFAULT_LIVE_PROXY_WAIT_TIMEOUT_MS,
            )
            .current_dir(&run_config_for_run.workspace_root)
            .output()
            .expect("spawn harness prompt")
    })
    .await
    .expect("join blocking harness run");

    assert!(
        output.status.success(),
        "wiremock harness prompt failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let events_body = fs::read_to_string(&events_path)
        .unwrap_or_else(|err| panic!("failed reading {}: {err}", events_path.display()));
    assert_provider_turn_completed(&collect_provider_turn_observation(&events_body))
        .unwrap_or_else(|err| panic!("wiremock provider-turn evidence mismatch: {err}"));

    let requests = wait_for_wiremock_request_path(
        &server,
        run_config.endpoint.path(),
        WIREMOCK_REQUEST_SETTLE_TIMEOUT,
    )
    .await
    .unwrap_or_else(|err| panic!("failed waiting for wiremock responses request: {err}"));
    let responses_request = requests
        .iter()
        .find(|request| request.url.path() == run_config.endpoint.path())
        .unwrap_or_else(|| {
            panic!(
                "expected at least one {} request",
                run_config.endpoint.path()
            )
        });
    let authorization = responses_request
        .headers
        .get("authorization")
        .expect("authorization header")
        .to_str()
        .expect("authorization header must be utf-8");
    assert_eq!(authorization, "Bearer test-key");
    assert!(
        !requests
            .iter()
            .any(|request| request.url.path() == "/v1/chat/completions"),
        "did not expect /v1/chat/completions fallback"
    );

    let request_body: Value = responses_request
        .body_json()
        .expect("responses request body must be JSON");
    assert_eq!(
        request_body.get("model"),
        Some(&Value::String(overridden_model.to_string()))
    );
}

#[tokio::test(flavor = "current_thread")]
async fn live_proxy_prompt_wiremock_falls_back_to_chat_on_cliproxy_400() {
    let server = MockServer::start().await;

    Mock::given(method("POST"))
        .and(path(RESPONSES_ENDPOINT_PATH))
        .respond_with(ResponseTemplate::new(400))
        .mount(&server)
        .await;

    Mock::given(method("POST"))
        .and(path("/v1/chat/completions"))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("content-type", "text/event-stream")
                .set_body_raw(deterministic_chat_sse_fixture(), "text/event-stream"),
        )
        .mount(&server)
        .await;

    let provider_name = "proxy";
    let configured_model = "configured-model";
    let namespace = LiveNamespaceAllocation::allocate("live-proxy-wiremock-fallback")
        .expect("allocate fallback namespace");
    let session_dir = namespace.session_dir("wiremock-session");
    let source_config_path = namespace.artifact_file("source-config", "jsonc");
    let source_config = build_live_proxy_test_config(
        provider_name,
        &server.uri(),
        "auto",
        configured_model,
        &session_dir,
    );
    fs::write(
        &source_config_path,
        serde_json::to_string_pretty(&source_config).expect("serialize fallback config"),
    )
    .expect("write fallback config");

    let run_config = prepare_prompt_run_config(
        &source_config_path,
        provider_name,
        configured_model,
        None,
        "wiremock_live_profile",
    )
    .expect("prepare fallback prompt run config");

    let harness_bin = resolve_harness_bin();
    let events_path = namespace.artifact_file("events", "jsonl");

    let harness_bin_for_run = harness_bin.clone();
    let events_path_for_run = events_path.clone();
    let run_config_for_run = run_config.clone();
    let output = tokio::task::spawn_blocking(move || {
        Command::new(&harness_bin_for_run)
            .env_clear()
            .arg("prompt")
            .arg("--text")
            .arg("Return hello from fallback wiremock")
            .arg("--profile")
            .arg(&run_config_for_run.profile)
            .arg("--config")
            .arg(&run_config_for_run.config_path)
            .arg("--out")
            .arg(&events_path_for_run)
            .env(
                "HARNESS_PROMPT_WAIT_TIMEOUT_MS",
                DEFAULT_LIVE_PROXY_WAIT_TIMEOUT_MS,
            )
            .current_dir(&run_config_for_run.workspace_root)
            .output()
            .expect("spawn harness prompt fallback")
    })
    .await
    .expect("join blocking harness fallback run");

    assert!(
        output.status.success(),
        "fallback wiremock harness prompt failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let events_body = fs::read_to_string(&events_path)
        .unwrap_or_else(|err| panic!("failed reading {}: {err}", events_path.display()));
    assert_events_show_successful_provider_turn(provider_name, &events_body);

    let requests = wait_for_wiremock_request_path(
        &server,
        "/v1/chat/completions",
        WIREMOCK_REQUEST_SETTLE_TIMEOUT,
    )
    .await
    .unwrap_or_else(|err| panic!("failed waiting for wiremock fallback request: {err}"));
    assert!(
        requests
            .iter()
            .any(|request| request.url.path() == RESPONSES_ENDPOINT_PATH),
        "expected the initial /v1/responses attempt"
    );
    let chat_request = requests
        .iter()
        .find(|request| request.url.path() == "/v1/chat/completions")
        .expect("expected fallback /v1/chat/completions request");
    let authorization = chat_request
        .headers
        .get("authorization")
        .expect("authorization header")
        .to_str()
        .expect("authorization header must be utf-8");
    assert_eq!(authorization, "Bearer test-key");

    let request_body: Value = chat_request
        .body_json()
        .expect("fallback chat request body must be JSON");
    assert_eq!(
        request_body.get("model"),
        Some(&Value::String(configured_model.to_string()))
    );
}

#[test]
fn prepare_prompt_run_config_rejects_chat_completions_mode() {
    let source_config_path = unique_temp_file("live-proxy-chat-mode", "jsonc");
    let session_dir = unique_temp_dir("live-proxy-chat-session");
    let source_config = build_live_proxy_test_config(
        "default",
        "http://127.0.0.1:9999",
        "chat_completions",
        "chat-model",
        &session_dir,
    );
    fs::write(
        &source_config_path,
        serde_json::to_string_pretty(&source_config).expect("serialize chat mode config"),
    )
    .expect("write chat mode config");

    let err = prepare_prompt_run_config(
        &source_config_path,
        "default",
        "chat-model",
        None,
        "chat_profile",
    )
    .expect_err("chat_completions mode should be rejected for live CLI proxy test");

    assert!(
        err.contains("responses or auto"),
        "unexpected error message: {err}"
    );
}

#[test]
fn prepare_live_prompt_run_config_applies_low_variant_when_available() {
    let request = LivePromptRequest {
        source_config_path: repo_root().join("configs").join("harness.example.jsonc"),
        provider_name: DEFAULT_LIVE_PROXY_PROVIDER.to_string(),
        primary_model: DEFAULT_LIVE_PROXY_MODEL.to_string(),
        primary_variant: Some(DEFAULT_LIVE_PROXY_VARIANT.to_string()),
        vision_model: DEFAULT_LIVE_PROXY_MODEL.to_string(),
        profile: DEFAULT_LIVE_PROXY_PROFILE.to_string(),
        prompt_text: DEFAULT_LIVE_PROXY_PROMPT.to_string(),
        wait_timeout_ms: DEFAULT_LIVE_PROXY_WAIT_TIMEOUT_MS.to_string(),
    };

    let run_config =
        prepare_live_prompt_run_config(&request).expect("prepare live prompt run config");
    let prepared = load_json5_config(&run_config.config_path).expect("load prepared config");
    let prepared_profile = prepared
        .get("agents")
        .and_then(Value::as_object)
        .and_then(|agents| agents.get(DEFAULT_LIVE_PROXY_PROFILE))
        .and_then(Value::as_object)
        .expect("prepared live smoke agent present");
    assert_eq!(
        prepared
            .get("ui")
            .and_then(Value::as_object)
            .and_then(|ui| ui.get("default_profile"))
            .and_then(Value::as_str),
        Some(DEFAULT_LIVE_PROXY_PROFILE)
    );

    assert_eq!(
        prepared_profile.get("model_ref").and_then(Value::as_str),
        Some("default:gpt-5.4-mini")
    );
    assert_eq!(
        prepared_profile.get("variant").and_then(Value::as_str),
        Some(DEFAULT_LIVE_PROXY_VARIANT)
    );
}

#[test]
fn prepare_live_tool_flow_run_config_builds_minimal_tool_profile() {
    let source_config_path = unique_temp_file("live-proxy-tool-flow", "jsonc");
    let source_session_dir = unique_temp_dir("live-proxy-tool-flow-source-session");
    let source_config = build_live_proxy_test_config(
        "default",
        "http://127.0.0.1:9999",
        "responses",
        DEFAULT_LIVE_PROXY_MODEL,
        &source_session_dir,
    );
    fs::write(
        &source_config_path,
        serde_json::to_string_pretty(&source_config).expect("serialize tool flow config"),
    )
    .expect("write tool flow config");

    let request = LivePromptRequest {
        source_config_path,
        provider_name: "default".to_string(),
        primary_model: DEFAULT_LIVE_PROXY_MODEL.to_string(),
        primary_variant: None,
        vision_model: "vision-model".to_string(),
        profile: DEFAULT_LIVE_PROXY_PROFILE.to_string(),
        prompt_text: DEFAULT_LIVE_PROXY_PROMPT.to_string(),
        wait_timeout_ms: DEFAULT_LIVE_PROXY_WAIT_TIMEOUT_MS.to_string(),
    };

    let run_config = prepare_live_tool_flow_run_config(
        &request,
        LiveToolFlowNamespaces {
            live_tui_session: LIVE_TUI_SESSION_NAMESPACE,
            tool_flow_session: TOOL_FLOW_SESSION_NAMESPACE,
            vision_verifier_session: VISION_VERIFIER_SESSION_NAMESPACE,
            visual_test_name: LIVE_PROXY_TUI_TOOL_FLOW_TEST_NAME,
        },
    )
    .expect("prepare live tool-flow config");

    assert_eq!(run_config.tool_flow.profile, LIVE_PROXY_TOOL_FLOW_PROFILE);
    assert_eq!(run_config.tool_flow.model_id, DEFAULT_LIVE_PROXY_MODEL);
    assert_eq!(
        run_config.vision_verifier.profile,
        LIVE_PROXY_VISION_VERIFIER_PROFILE
    );
    assert_eq!(run_config.vision_verifier.model_id, "vision-model");
    assert_ne!(
        run_config.tool_flow.session_dir, run_config.vision_verifier.session_dir,
        "tool-flow and vision verifier runs must use distinct session dirs"
    );
    assert_eq!(
        run_config.canonical_relative_path,
        PathBuf::from(LIVE_TOOL_FLOW_RELATIVE_PATH)
    );

    let tool_flow_config = load_json5_config(&run_config.tool_flow.config_path)
        .expect("load prepared tool-flow config");

    assert_eq!(
        tool_flow_config
            .get("permissions")
            .and_then(Value::as_object)
            .and_then(|permissions| permissions.get("defaults"))
            .and_then(Value::as_object)
            .and_then(|defaults| defaults.get("edit"))
            .and_then(Value::as_str),
        Some("allow")
    );
    assert_eq!(
        tool_flow_config
            .get("permissions")
            .and_then(Value::as_object)
            .and_then(|permissions| permissions.get("defaults"))
            .and_then(Value::as_object)
            .and_then(|defaults| defaults.get("shell"))
            .and_then(Value::as_str),
        Some("allow")
    );
    assert_eq!(
        tool_flow_config
            .get("permissions")
            .and_then(Value::as_object)
            .and_then(|permissions| permissions.get("defaults"))
            .and_then(Value::as_object)
            .and_then(|defaults| defaults.get("network"))
            .and_then(Value::as_str),
        Some("allow")
    );
    assert_eq!(
        tool_flow_config
            .get("permissions")
            .and_then(Value::as_object)
            .and_then(|permissions| permissions.get("shell_allowlist"))
            .and_then(Value::as_object)
            .and_then(|allowlist| allowlist.get("executables"))
            .and_then(Value::as_array),
        Some(&vec![Value::String("sh".to_string())])
    );
    assert_eq!(
        tool_flow_config
            .get("permissions")
            .and_then(Value::as_object)
            .and_then(|permissions| permissions.get("shell_allowlist"))
            .and_then(Value::as_object)
            .and_then(|allowlist| allowlist.get("cwd_roots"))
            .and_then(Value::as_array),
        Some(&vec![Value::String(".".to_string())])
    );
    assert_eq!(
        tool_flow_config
            .get("runtime")
            .and_then(Value::as_object)
            .and_then(|runtime| runtime.get("session_dir"))
            .and_then(Value::as_str),
        Some(run_config.tool_flow.session_dir.to_string_lossy().as_ref())
    );

    let tool_flow_profile = tool_flow_config
        .get("agents")
        .and_then(Value::as_object)
        .and_then(|agents| agents.get(LIVE_PROXY_TOOL_FLOW_PROFILE))
        .and_then(Value::as_object)
        .expect("tool-flow agent present");
    assert_eq!(
        tool_flow_profile.get("model_ref").and_then(Value::as_str),
        Some("default:gpt-5.4-mini")
    );
    assert_eq!(
        tool_flow_profile.get("tools").and_then(Value::as_array),
        Some(&vec![
            Value::String("write".to_string()),
            Value::String("read".to_string()),
            Value::String("edit.hashline_scan".to_string()),
            Value::String("edit.hashline_apply".to_string()),
        ])
    );
    let profile_permissions = tool_flow_profile
        .get("permissions")
        .and_then(Value::as_object)
        .expect("tool-flow profile permissions present");
    assert_eq!(
        profile_permissions.get("edit").and_then(Value::as_str),
        Some("allow")
    );
    assert_eq!(
        profile_permissions.get("shell").and_then(Value::as_str),
        Some("allow")
    );
    assert_eq!(
        profile_permissions.get("network").and_then(Value::as_str),
        Some("allow")
    );
}

#[test]
fn prepare_live_prompt_native_tool_flow_run_config_builds_cli_parity_stages() {
    let source_config_path = unique_temp_file("live-proxy-native-tool-flow", "jsonc");
    let source_session_dir = unique_temp_dir("live-proxy-native-tool-flow-source-session");
    let source_config = build_live_proxy_test_config(
        "default",
        "http://127.0.0.1:9999",
        "responses",
        DEFAULT_LIVE_PROXY_MODEL,
        &source_session_dir,
    );
    fs::write(
        &source_config_path,
        serde_json::to_string_pretty(&source_config).expect("serialize native tool flow config"),
    )
    .expect("write native tool flow config");

    let request = LivePromptRequest {
        source_config_path,
        provider_name: "default".to_string(),
        primary_model: DEFAULT_LIVE_PROXY_MODEL.to_string(),
        primary_variant: Some(DEFAULT_LIVE_PROXY_VARIANT.to_string()),
        vision_model: "vision-model".to_string(),
        profile: DEFAULT_LIVE_PROXY_PROFILE.to_string(),
        prompt_text: DEFAULT_LIVE_PROXY_PROMPT.to_string(),
        wait_timeout_ms: DEFAULT_LIVE_PROXY_WAIT_TIMEOUT_MS.to_string(),
    };

    let run_config = prepare_live_prompt_native_tool_flow_run_config(&request)
        .expect("prepare native tool flow config");

    assert_eq!(run_config.create.model_id, DEFAULT_LIVE_PROXY_MODEL);
    assert_eq!(
        run_config.create.variant.as_deref(),
        Some(DEFAULT_LIVE_PROXY_VARIANT)
    );
    assert_eq!(
        run_config.create.workspace_root,
        run_config.first_read.workspace_root
    );
    assert_eq!(
        run_config.create.workspace_root,
        run_config.scan.workspace_root
    );
    assert_eq!(
        run_config.create.workspace_root,
        run_config.apply.workspace_root
    );
    assert_eq!(
        run_config.create.workspace_root,
        run_config.final_read.workspace_root
    );
    assert_ne!(
        run_config.create.session_dir,
        run_config.first_read.session_dir
    );
    assert_ne!(
        run_config.first_read.session_dir,
        run_config.scan.session_dir
    );
    assert_ne!(run_config.scan.session_dir, run_config.apply.session_dir);
    assert_ne!(
        run_config.apply.session_dir,
        run_config.final_read.session_dir
    );
    assert_eq!(
        run_config.canonical_relative_path,
        PathBuf::from(LIVE_TOOL_FLOW_RELATIVE_PATH)
    );

    let prepared = load_json5_config(&run_config.create.config_path)
        .expect("load native tool flow prepared config");
    let profile = prepared
        .get("agents")
        .and_then(Value::as_object)
        .and_then(|agents| agents.get(LIVE_PROXY_TOOL_FLOW_PROFILE))
        .and_then(Value::as_object)
        .expect("native tool flow agent present");
    assert_eq!(
        profile.get("variant").and_then(Value::as_str),
        Some(DEFAULT_LIVE_PROXY_VARIANT)
    );
}

#[test]
fn prepare_live_prompt_chat_tool_run_config_builds_restricted_agents() {
    let source_config_path = unique_temp_file("live-proxy-chat-tool-config", "jsonc");
    let source_session_dir = unique_temp_dir("live-proxy-chat-tool-source-session");
    let source_config = build_live_proxy_test_config(
        "default",
        "http://127.0.0.1:9999",
        "responses",
        DEFAULT_LIVE_PROXY_MODEL,
        &source_session_dir,
    );
    fs::write(
        &source_config_path,
        serde_json::to_string_pretty(&source_config).expect("serialize chat tool config"),
    )
    .expect("write chat tool config");

    let request = LivePromptRequest {
        source_config_path,
        provider_name: "default".to_string(),
        primary_model: DEFAULT_LIVE_PROXY_MODEL.to_string(),
        primary_variant: None,
        vision_model: "vision-model".to_string(),
        profile: DEFAULT_LIVE_PROXY_PROFILE.to_string(),
        prompt_text: DEFAULT_LIVE_PROXY_PROMPT.to_string(),
        wait_timeout_ms: DEFAULT_LIVE_PROXY_WAIT_TIMEOUT_MS.to_string(),
    };

    let run_config = prepare_live_prompt_chat_tool_run_config(&request)
        .expect("prepare live prompt chat tool config");

    assert_eq!(run_config.todo_flow.model_id, DEFAULT_LIVE_PROXY_MODEL);
    assert_eq!(run_config.question.model_id, DEFAULT_LIVE_PROXY_MODEL);
    assert_eq!(run_config.skill.model_id, DEFAULT_LIVE_PROXY_MODEL);
    assert_ne!(
        run_config.todo_flow.session_dir,
        run_config.question.session_dir
    );
    assert_ne!(
        run_config.question.session_dir,
        run_config.skill.session_dir
    );

    let todo_config =
        load_json5_config(&run_config.todo_flow.config_path).expect("load prepared todo config");
    let question_config =
        load_json5_config(&run_config.question.config_path).expect("load prepared question config");
    let skill_config =
        load_json5_config(&run_config.skill.config_path).expect("load prepared skill config");

    let todo_profile = todo_config
        .get("agents")
        .and_then(Value::as_object)
        .and_then(|agents| agents.get(LIVE_PROXY_CHAT_TODO_FLOW_PROFILE))
        .and_then(Value::as_object)
        .expect("todo agent present");
    assert_eq!(
        todo_profile.get("tools").and_then(Value::as_array),
        Some(&vec![Value::String("todowrite".to_string())])
    );

    let question_profile = question_config
        .get("agents")
        .and_then(Value::as_object)
        .and_then(|agents| agents.get(LIVE_PROXY_CHAT_QUESTION_PROFILE))
        .and_then(Value::as_object)
        .expect("question agent present");
    assert_eq!(
        question_profile.get("tools").and_then(Value::as_array),
        Some(&vec![Value::String("question".to_string())])
    );

    let skill_profile = skill_config
        .get("agents")
        .and_then(Value::as_object)
        .and_then(|agents| agents.get(LIVE_PROXY_CHAT_SKILL_PROFILE))
        .and_then(Value::as_object)
        .expect("skill agent present");
    assert_eq!(
        skill_profile.get("tools").and_then(Value::as_array),
        Some(&vec![Value::String("skill".to_string())])
    );
    assert!(
        run_config
            .skill
            .workspace_root
            .join(".agent-harness")
            .join("skills")
            .join("rust-best-practices")
            .join("SKILL.md")
            .exists(),
        "prepared chat tool workspace should seed rust-best-practices into a local project skill root"
    );
}

#[test]
fn example_config_keeps_minimal_surface_and_live_helper_prepares_runtime_profile() {
    let config_path = repo_root().join("configs").join("harness.example.jsonc");
    let config = load_json5_config(&config_path).expect("load shipped example config");

    let default_provider = config
        .get("provider")
        .and_then(Value::as_object)
        .and_then(|providers| providers.get("default"))
        .and_then(Value::as_object)
        .expect("default provider present in example config");
    assert_eq!(
        default_provider
            .get("options")
            .and_then(Value::as_object)
            .and_then(|options| options.get("baseURL"))
            .and_then(Value::as_str),
        Some("http://127.0.0.1:8317/v1")
    );
    assert_eq!(
        default_provider
            .get("options")
            .and_then(Value::as_object)
            .and_then(|options| options.get("apiKey"))
            .and_then(Value::as_str),
        Some("placeholder-api-key")
    );
    assert!(
        default_provider.get("api_mode").is_none(),
        "the shipped public example should keep provider api mode implicit"
    );

    assert_eq!(
        config.get("default_agent").and_then(Value::as_str),
        Some("build")
    );

    assert!(
        config.get("agent").is_none(),
        "the shipped public example should rely on runtime-synthesized build defaults"
    );

    let build_prompt = read_shipped_agent_prompt_asset("build");
    assert!(build_prompt.contains("apply_patch"));
    assert!(build_prompt.contains("do the work without asking questions"));

    let prepared = prepare_prompt_run_config(
        &config_path,
        DEFAULT_LIVE_PROXY_PROVIDER,
        DEFAULT_LIVE_PROXY_MODEL,
        Some(DEFAULT_LIVE_PROXY_VARIANT),
        DEFAULT_LIVE_PROXY_PROFILE,
    )
    .expect("prepare shipped example config for live signoff");
    let prepared_config =
        load_json5_config(&prepared.config_path).expect("load prepared live signoff config");
    let prepared_agents = prepared_config
        .get("agents")
        .and_then(Value::as_object)
        .expect("prepared live config should define runtime agents");
    let live_profile = prepared_agents
        .get(DEFAULT_LIVE_PROXY_PROFILE)
        .and_then(Value::as_object)
        .expect("live helper should synthesize the selected runtime profile");
    assert_eq!(
        live_profile.get("model_ref").and_then(Value::as_str),
        Some("default:gpt-5.4-mini")
    );
    assert_eq!(
        live_profile.get("variant").and_then(Value::as_str),
        Some(DEFAULT_LIVE_PROXY_VARIANT)
    );
    assert!(live_profile
        .get("system_prompt")
        .and_then(Value::as_str)
        .is_some_and(|prompt| prompt.contains("apply_patch")));

    assert!(
        !prepared_agents.contains_key("plan"),
        "plan should not be part of the shipped default agent surface"
    );
    assert!(
        !prepared_agents.contains_key("explore"),
        "explore should not be part of the shipped default agent surface"
    );
    assert!(
        !prepared_agents.contains_key("executor"),
        "executor should not be part of the shipped default agent surface"
    );
    assert!(
        !prepared_agents.contains_key("tool_audit"),
        "tool_audit should not be part of the shipped default agent surface"
    );

    assert!(
        config
            .get("provider")
            .and_then(|providers| providers.get("default"))
            .and_then(|provider| provider.get("models"))
            .and_then(|models| models.get("gpt-5.4-mini"))
            .and_then(|model| model.get("variants"))
            .is_none(),
        "the shipped public example should not expose model variant clutter"
    );

    assert!(
        config
            .get("provider")
            .and_then(|providers| providers.get("default"))
            .and_then(|provider| provider.get("models"))
            .and_then(|models| models.get("gpt-5.4-mini"))
            .and_then(|model| model.get("variants"))
            .and_then(|variants| variants.get("low"))
            .is_none(),
        "live signoff helpers synthesize low when they need a live-only variant"
    );

    let low_variant = prepared_config
        .get("providers")
        .and_then(|providers| providers.get("default"))
        .and_then(|provider| provider.get("models"))
        .and_then(|models| models.get("gpt-5.4-mini"))
        .and_then(|model| model.get("variants"))
        .and_then(|variants| variants.get(DEFAULT_LIVE_PROXY_VARIANT))
        .and_then(Value::as_object)
        .expect("prepared live config should synthesize the low signoff variant");
    assert_eq!(
        low_variant
            .get("metadata")
            .and_then(|metadata| metadata.get("recommended_for"))
            .and_then(Value::as_str),
        Some("live_proxy")
    );
}

#[test]
fn tool_flow_evidence_detects_ordered_same_file_sequence() {
    let workspace_root = unique_temp_dir("live-proxy-tool-flow-evidence-workspace");
    fs::create_dir_all(workspace_root.join("tmp")).expect("create tool-flow tmp dir");
    let final_content = "alpha\nBETA\ngamma\n";
    fs::write(
        workspace_root.join(LIVE_TOOL_FLOW_RELATIVE_PATH),
        final_content,
    )
    .expect("write final tool-flow workspace file");

    let run_dir = unique_temp_dir("live-proxy-tool-flow-evidence-run");
    let event = |seq: u64, event_type: &str, data: Value| {
        json!({
            "schema_version": 1,
            "event_id": format!("evt-{seq:04}"),
            "seq": seq,
            "run_id": "prompt_tool_flow_fixture",
            "mono_ms": seq,
            "actor": {
                "kind": "system"
            },
            "payload": {
                "event_type": event_type,
                "data": data,
            }
        })
    };
    let requested = |seq: u64, tool_call_id: &str, tool_id: &str, args_summary: Value| {
        event(
            seq,
            "tool_call_requested",
            json!({
                "tool_call_id": tool_call_id,
                "tool_id": tool_id,
                "args_summary": args_summary.to_string(),
                "args_digest": format!("digest-{seq:04}"),
            }),
        )
    };
    let finished = |seq: u64, tool_call_id: &str| {
        event(
            seq,
            "tool_call_finished",
            json!({
                "tool_call_id": tool_call_id,
                "status": "succeeded",
                "output_summary": "ok",
                "output_digest": format!("output-{seq:04}"),
            }),
        )
    };
    let events_body = vec![
        requested(
            1,
            "call-write",
            "write",
            json!({
                "filePath": LIVE_TOOL_FLOW_RELATIVE_PATH,
                "content": "alpha\nbeta\ngamma\n",
            }),
        ),
        finished(2, "call-write"),
        requested(
            3,
            "call-read-1",
            "read",
            json!({
                "filePath": LIVE_TOOL_FLOW_RELATIVE_PATH,
                "offset": 1,
                "limit": 2000,
            }),
        ),
        finished(4, "call-read-1"),
        requested(
            5,
            "call-scan",
            "edit.hashline_scan",
            json!({
                "path": LIVE_TOOL_FLOW_RELATIVE_PATH,
                "start_line": 1,
                "limit": 20,
            }),
        ),
        finished(6, "call-scan"),
        requested(
            7,
            "call-apply",
            "edit.hashline_apply",
            json!({
                "edit_id": "edit-live-tool-flow",
                "path": LIVE_TOOL_FLOW_RELATIVE_PATH,
                "ops": [{
                    "kind": "replace"
                }],
            }),
        ),
        event(
            8,
            "edit_proposed",
            json!({
                "edit_id": "edit-live-tool-flow",
                "path": LIVE_TOOL_FLOW_RELATIVE_PATH,
                "summary": "apply hashline patch with 1 op(s)",
                "patch_digest": "patch-0001",
            }),
        ),
        event(
            9,
            "edit_applied",
            json!({
                "edit_id": "edit-live-tool-flow",
                "path": LIVE_TOOL_FLOW_RELATIVE_PATH,
                "new_file_digest": "file-0001",
                "diff_rel_path": "edit-live-tool-flow.diff",
                "diff_digest": "diff-0001",
            }),
        ),
        finished(10, "call-apply"),
        requested(
            11,
            "call-read-2",
            "read",
            json!({
                "filePath": LIVE_TOOL_FLOW_RELATIVE_PATH,
                "offset": 1,
                "limit": 2000,
            }),
        ),
        finished(12, "call-read-2"),
    ]
    .into_iter()
    .map(|value| value.to_string())
    .collect::<Vec<_>>()
    .join("\n");
    fs::write(run_dir.join("events.jsonl"), format!("{events_body}\n"))
        .expect("write tool-flow evidence events");

    let evidence = ToolFlowEvidence::collect(
        &run_dir,
        &workspace_root,
        Path::new(LIVE_TOOL_FLOW_RELATIVE_PATH),
    )
    .expect("collect tool-flow evidence");

    evidence
        .assert_run_succeeded()
        .expect("tool-flow run should not fail");
    evidence
        .assert_ordered_same_file_sequence()
        .expect("tool-flow helper should detect the ordered same-file sequence");
    evidence
        .assert_final_workspace_content(final_content)
        .expect("final workspace content should match the expected canonical file");
}

#[test]
fn tool_flow_evidence_collect_many_merges_stage_runs() {
    let workspace_root = unique_temp_dir("live-proxy-tool-flow-evidence-multi-workspace");
    fs::create_dir_all(workspace_root.join("tmp")).expect("create multi-run tool-flow tmp dir");
    let final_content = "alpha\nBETA\ngamma\n";
    fs::write(
        workspace_root.join(LIVE_TOOL_FLOW_RELATIVE_PATH),
        final_content,
    )
    .expect("write final multi-run tool-flow workspace file");

    let event = |seq: u64, event_type: &str, data: Value| {
        json!({
            "schema_version": 1,
            "event_id": format!("evt-{seq:04}"),
            "seq": seq,
            "run_id": format!("run-{seq:04}"),
            "mono_ms": seq,
            "actor": { "kind": "system" },
            "payload": {
                "event_type": event_type,
                "data": data,
            }
        })
    };
    let requested = |seq: u64, tool_call_id: &str, tool_id: &str, args_summary: Value| {
        event(
            seq,
            "tool_call_requested",
            json!({
                "tool_call_id": tool_call_id,
                "tool_id": tool_id,
                "args_summary": args_summary.to_string(),
                "args_digest": format!("digest-{seq:04}"),
            }),
        )
    };
    let finished = |seq: u64, tool_call_id: &str| {
        event(
            seq,
            "tool_call_finished",
            json!({
                "tool_call_id": tool_call_id,
                "status": "succeeded",
                "output_summary": "ok",
                "output_digest": format!("output-{seq:04}"),
            }),
        )
    };
    let write_run = |stem: &str, events: Vec<Value>| {
        let run_dir = unique_temp_dir(stem);
        let body = events
            .into_iter()
            .map(|value| value.to_string())
            .collect::<Vec<_>>()
            .join("\n");
        fs::write(run_dir.join("events.jsonl"), format!("{body}\n")).expect("write stage events");
        run_dir
    };

    let create_run = write_run(
        "live-proxy-tool-flow-evidence-create-run",
        vec![
            requested(
                1,
                "call-write",
                "write",
                json!({
                    "filePath": LIVE_TOOL_FLOW_RELATIVE_PATH,
                    "content": "alpha\nbeta\ngamma\n",
                }),
            ),
            finished(2, "call-write"),
        ],
    );
    let first_read_run = write_run(
        "live-proxy-tool-flow-evidence-read-1-run",
        vec![
            requested(
                3,
                "call-read-1",
                "read",
                json!({
                    "filePath": LIVE_TOOL_FLOW_RELATIVE_PATH,
                    "offset": 1,
                    "limit": 2000,
                }),
            ),
            finished(4, "call-read-1"),
        ],
    );
    let scan_run = write_run(
        "live-proxy-tool-flow-evidence-scan-run",
        vec![
            requested(
                5,
                "call-scan",
                "edit.hashline_scan",
                json!({
                    "path": LIVE_TOOL_FLOW_RELATIVE_PATH,
                    "start_line": 1,
                    "limit": 20,
                }),
            ),
            finished(6, "call-scan"),
        ],
    );
    let apply_run = write_run(
        "live-proxy-tool-flow-evidence-apply-run",
        vec![
            requested(
                7,
                "call-apply",
                "edit.hashline_apply",
                json!({
                    "edit_id": "edit-live-tool-flow",
                    "path": LIVE_TOOL_FLOW_RELATIVE_PATH,
                    "ops": [{ "kind": "replace" }],
                }),
            ),
            event(
                8,
                "edit_proposed",
                json!({
                    "edit_id": "edit-live-tool-flow",
                    "path": LIVE_TOOL_FLOW_RELATIVE_PATH,
                    "summary": "apply hashline patch with 1 op(s)",
                    "patch_digest": "patch-0001",
                }),
            ),
            event(
                9,
                "edit_applied",
                json!({
                    "edit_id": "edit-live-tool-flow",
                    "path": LIVE_TOOL_FLOW_RELATIVE_PATH,
                    "new_file_digest": "file-0001",
                    "diff_rel_path": "edit-live-tool-flow.diff",
                    "diff_digest": "diff-0001",
                }),
            ),
            finished(10, "call-apply"),
        ],
    );
    let final_read_run = write_run(
        "live-proxy-tool-flow-evidence-read-2-run",
        vec![
            requested(
                11,
                "call-read-2",
                "read",
                json!({
                    "filePath": LIVE_TOOL_FLOW_RELATIVE_PATH,
                    "offset": 1,
                    "limit": 2000,
                }),
            ),
            finished(12, "call-read-2"),
        ],
    );

    let evidence = ToolFlowEvidence::collect_many(
        &[
            create_run,
            first_read_run,
            scan_run,
            apply_run,
            final_read_run,
        ],
        &workspace_root,
        Path::new(LIVE_TOOL_FLOW_RELATIVE_PATH),
    )
    .expect("collect multi-run tool-flow evidence");

    evidence
        .assert_run_succeeded()
        .expect("multi-run tool-flow should not fail");
    evidence
        .assert_ordered_same_file_sequence()
        .expect("multi-run tool-flow helper should merge the ordered same-file sequence");
    evidence
        .assert_final_workspace_content(final_content)
        .expect("multi-run final workspace content should match the expected canonical file");
}

#[test]
fn resolve_tagged_run_dir_rejects_collisions() {
    let session_dir = unique_temp_dir(TOOL_FLOW_SESSION_NAMESPACE);
    for run_id in ["prompt_run_a", "prompt_run_b"] {
        let run_dir = session_dir.join(run_id);
        fs::create_dir_all(&run_dir).expect("create collision run dir");
        fs::write(run_dir.join("events.jsonl"), "{}\n").expect("write collision events file");
    }

    let err = resolve_tagged_run_dir(&session_dir, TOOL_FLOW_SESSION_NAMESPACE)
        .expect_err("multiple tagged run dirs should be rejected");

    assert!(
        err.contains(TOOL_FLOW_SESSION_NAMESPACE),
        "collision error should include the sub-run namespace: {err}"
    );
    assert!(
        err.contains("found 2"),
        "collision error should include the matching run-dir count: {err}"
    );
}

#[test]
fn tool_flow_tool_call_state_recognizes_write_same_file_success() {
    let events = concat!(
        r#"{"payload":{"event_type":"tool_call_requested","data":{"tool_call_id":"toolcall_000001","tool_id":"write","args_summary":"{\"content\":\"alpha\\nbeta\\ngamma\\n\",\"filePath\":\"tmp/live_tool_flow.md\"}"}}}"#,
        "\n",
        r#"{"payload":{"event_type":"tool_call_finished","data":{"tool_call_id":"toolcall_000001","status":"succeeded"}}}"#,
        "\n"
    );

    let state =
        tool_flow_tool_call_state(events, Path::new(LIVE_TOOL_FLOW_RELATIVE_PATH), "write", 1)
            .expect("write tool-flow state should parse");

    assert_eq!(state, ToolFlowToolCallState::Succeeded);
}

#[test]
fn resolve_live_request_defaults_vision_model_to_primary() {
    let _guard = live_proxy_env_lock()
        .lock()
        .expect("live proxy env test lock should not be poisoned");

    let source_config_path = unique_temp_file("live-proxy-request", "jsonc");
    let session_dir = unique_temp_dir("live-proxy-request-session");
    let source_config = build_live_proxy_test_config(
        "default",
        "http://127.0.0.1:9999",
        "responses",
        DEFAULT_LIVE_PROXY_MODEL,
        &session_dir,
    );
    fs::write(
        &source_config_path,
        serde_json::to_string_pretty(&source_config).expect("serialize request config"),
    )
    .expect("write request config");

    with_live_proxy_env(
        &[
            (
                "HARNESS_LIVE_PROXY_CONFIG",
                Some(source_config_path.as_os_str()),
            ),
            ("HARNESS_LIVE_PROXY_PROVIDER", Some(OsStr::new("default"))),
            (
                "HARNESS_LIVE_PROXY_MODEL",
                Some(OsStr::new(DEFAULT_LIVE_PROXY_MODEL)),
            ),
            ("HARNESS_LIVE_PROXY_VARIANT", None),
            ("HARNESS_LIVE_PROXY_VISION_MODEL", None),
            ("HARNESS_LIVE_PROXY_PROFILE", None),
            ("HARNESS_LIVE_PROXY_PROMPT", None),
            ("HARNESS_LIVE_PROXY_WAIT_TIMEOUT_MS", None),
        ],
        || {
            let request =
                resolve_live_prompt_request(&repo_root()).expect("resolve live prompt request");
            assert_eq!(request.primary_model, DEFAULT_LIVE_PROXY_MODEL);
            assert_eq!(request.primary_variant.as_deref(), None);
            assert_eq!(request.vision_model, request.primary_model);
        },
    );
}

#[test]
fn resolve_live_request_prefers_low_variant_for_documented_signoff_model() {
    let _guard = live_proxy_env_lock()
        .lock()
        .expect("live proxy env test lock should not be poisoned");

    let source_config_path = repo_root().join("configs").join("harness.example.jsonc");
    with_live_proxy_env(
        &[
            (
                "HARNESS_LIVE_PROXY_CONFIG",
                Some(source_config_path.as_os_str()),
            ),
            ("HARNESS_LIVE_PROXY_PROVIDER", Some(OsStr::new("default"))),
            (
                "HARNESS_LIVE_PROXY_MODEL",
                Some(OsStr::new(DEFAULT_LIVE_PROXY_MODEL)),
            ),
            ("HARNESS_LIVE_PROXY_VARIANT", None),
            ("HARNESS_LIVE_PROXY_VISION_MODEL", None),
            ("HARNESS_LIVE_PROXY_PROFILE", None),
            ("HARNESS_LIVE_PROXY_PROMPT", None),
            ("HARNESS_LIVE_PROXY_WAIT_TIMEOUT_MS", None),
        ],
        || {
            let request =
                resolve_live_prompt_request(&repo_root()).expect("resolve live prompt request");
            assert_eq!(
                request.primary_variant.as_deref(),
                Some(DEFAULT_LIVE_PROXY_VARIANT)
            );
        },
    );
}

#[test]
fn resolve_live_request_prefers_documented_default_model_when_present() {
    let _guard = live_proxy_env_lock()
        .lock()
        .expect("live proxy env test lock should not be poisoned");

    let source_config_path = unique_temp_file("live-proxy-request-default-model", "jsonc");
    let session_dir = unique_temp_dir("live-proxy-request-default-model-session");
    let mut config = build_live_proxy_test_config(
        "default",
        "http://127.0.0.1:9999",
        "auto",
        "gpt-5.4",
        &session_dir,
    );
    let provider_models = config
        .get_mut("providers")
        .and_then(Value::as_object_mut)
        .and_then(|providers| providers.get_mut("default"))
        .and_then(Value::as_object_mut)
        .and_then(|provider| provider.get_mut("models"))
        .and_then(Value::as_object_mut)
        .expect("provider models object present");
    provider_models.insert(
        DEFAULT_LIVE_PROXY_MODEL.to_string(),
        json!({
            "display_name": "Documented default model"
        }),
    );
    fs::write(
        &source_config_path,
        serde_json::to_string_pretty(&config).expect("serialize request default model config"),
    )
    .expect("write request default model config");

    with_live_proxy_env(
        &[
            (
                "HARNESS_LIVE_PROXY_CONFIG",
                Some(source_config_path.as_os_str()),
            ),
            ("HARNESS_LIVE_PROXY_PROVIDER", Some(OsStr::new("default"))),
            ("HARNESS_LIVE_PROXY_MODEL", None),
            ("HARNESS_LIVE_PROXY_VARIANT", None),
            ("HARNESS_LIVE_PROXY_VISION_MODEL", None),
            ("HARNESS_LIVE_PROXY_PROFILE", None),
            ("HARNESS_LIVE_PROXY_PROMPT", None),
            ("HARNESS_LIVE_PROXY_WAIT_TIMEOUT_MS", None),
        ],
        || {
            let request =
                resolve_live_prompt_request(&repo_root()).expect("resolve live prompt request");
            assert_eq!(request.primary_model, DEFAULT_LIVE_PROXY_MODEL);
            assert_eq!(request.primary_variant.as_deref(), None);
        },
    );
}

#[test]
fn resolve_env_reference_value_uses_fallback_for_empty_var() {
    let _guard = live_proxy_env_lock()
        .lock()
        .expect("live proxy env test lock should not be poisoned");

    with_live_proxy_env(
        &[("HARNESS_LIVE_PROXY_EMPTY_API_KEY", Some(OsStr::new("")))],
        || {
            let resolved =
                resolve_env_reference_value("${HARNESS_LIVE_PROXY_EMPTY_API_KEY:-fallback-key}")
                    .expect("empty env var should use fallback value");
            assert_eq!(resolved, "fallback-key");
        },
    );
}

#[test]
fn resolve_live_proxy_config_path_resolves_relative_override_from_repo_root() {
    let repo_root = repo_root();
    let resolved = resolve_live_proxy_config_path(
        &repo_root,
        Some(Path::new("configs/harness.example.jsonc")),
    )
    .expect("resolve relative live proxy config override");

    assert_eq!(
        resolved,
        repo_root.join("configs").join("harness.example.jsonc")
    );
}

#[test]
fn live_tui_smoke_helpers_reuse_cliproxy_config_and_endpoint_rules() {
    let repo_root = repo_root();
    let default_config = resolve_live_proxy_config_path(&repo_root, None)
        .expect("resolve default live proxy config");
    assert!(
        default_config.ends_with(Path::new("configs").join("harness.example.jsonc")),
        "unexpected default live config path: {}",
        default_config.display()
    );

    let session_dir = unique_temp_dir("live-smoke-helper-session");
    let auto_config_path = unique_temp_file("live-smoke-helper-config", "jsonc");
    let auto_config = build_live_proxy_test_config(
        "proxy",
        "http://127.0.0.1:8317",
        "auto",
        "configured-model",
        &session_dir,
    );
    fs::write(
        &auto_config_path,
        serde_json::to_string_pretty(&auto_config).expect("serialize auto config"),
    )
    .expect("write auto config");

    let live_request = LivePromptRequest {
        source_config_path: auto_config_path.clone(),
        provider_name: "proxy".to_string(),
        primary_model: "override-model".to_string(),
        primary_variant: None,
        vision_model: "override-model".to_string(),
        profile: "tui_smoke_profile".to_string(),
        prompt_text: DEFAULT_LIVE_PROXY_PROMPT.to_string(),
        wait_timeout_ms: "1500".to_string(),
    };

    let run_config = prepare_live_prompt_run_config(&live_request)
        .expect("prepare auto-mode live TUI run config");
    assert_eq!(run_config.endpoint.path(), RESPONSES_ENDPOINT_PATH);
    assert_eq!(run_config.provider_name, "proxy");
    assert_eq!(run_config.model_id, "override-model");
    assert_eq!(run_config.profile, "tui_smoke_profile");
    assert_eq!(
        live_tui_command_timeout(&live_request),
        Duration::from_millis(1500).saturating_add(Duration::from_secs(20))
    );

    let prepared_config = load_json5_config(&run_config.config_path).expect("load prepared config");
    let default_provider = provider_from_config(&prepared_config, DEFAULT_LIVE_PROXY_PROVIDER)
        .expect("default provider should be rewritten into prepared config");
    assert_eq!(provider_api_mode(default_provider), "auto");
    assert_eq!(
        default_provider
            .get("models")
            .and_then(Value::as_object)
            .and_then(|models| models.get("override-model"))
            .and_then(Value::as_object)
            .and_then(|model| model.get("display_name"))
            .and_then(Value::as_str),
        Some("Prepared override-model")
    );

    let categories = prepared_config
        .get("agents")
        .and_then(Value::as_object)
        .expect("prepared config agents object");
    assert_eq!(
        categories
            .get("deep")
            .and_then(Value::as_object)
            .and_then(|category| category.get("model_ref"))
            .and_then(Value::as_str),
        Some("default:configured-model")
    );
    assert_eq!(
        categories
            .get("tui_smoke_profile")
            .and_then(Value::as_object)
            .and_then(|category| category.get("model_ref"))
            .and_then(Value::as_str),
        Some("default:override-model")
    );

    let missing_path = unique_temp_file("live-smoke-helper-missing", "jsonc");
    let missing_err = resolve_live_proxy_config_path(&repo_root, Some(&missing_path))
        .expect_err("missing live config path should fail fast");
    assert!(
        missing_err.contains("live proxy config not found at"),
        "unexpected missing-config error: {missing_err}"
    );

    let chat_config_path = unique_temp_file("live-smoke-helper-chat", "jsonc");
    let chat_config = build_live_proxy_test_config(
        "default",
        "http://127.0.0.1:9999",
        "chat_completions",
        "chat-model",
        &session_dir,
    );
    fs::write(
        &chat_config_path,
        serde_json::to_string_pretty(&chat_config).expect("serialize chat config"),
    )
    .expect("write chat config");
    let chat_err = prepare_prompt_run_config(
        &chat_config_path,
        "default",
        "chat-model",
        None,
        "chat_profile",
    )
    .expect_err("chat-completions mode should be rejected");
    assert!(
        chat_err.contains("responses or auto"),
        "unexpected chat-mode error: {chat_err}"
    );
}

#[test]
fn prepared_live_config_synthesizes_profile_from_minimal_example() {
    let repo_root = repo_root();
    let source_config_path = repo_root.join("configs").join("harness.example.jsonc");

    let run_config = prepare_prompt_run_config(
        &source_config_path,
        DEFAULT_LIVE_PROXY_PROVIDER,
        DEFAULT_LIVE_PROXY_MODEL,
        Some(DEFAULT_LIVE_PROXY_VARIANT),
        DEFAULT_LIVE_PROXY_PROFILE,
    )
    .expect("prepare shipped live config");

    let prepared_config = load_json5_config(&run_config.config_path).expect("load prepared config");
    let agents = prepared_config
        .get("agents")
        .and_then(Value::as_object)
        .expect("prepared agents object");
    assert!(
        !agents.contains_key("build"),
        "prepared live config should not reintroduce the raw shipped build agent block"
    );
    let live_agent = agents
        .get(DEFAULT_LIVE_PROXY_PROFILE)
        .and_then(Value::as_object)
        .expect("prepared live profile");

    assert_eq!(
        live_agent.get("model_ref").and_then(Value::as_str),
        Some("default:gpt-5.4-mini")
    );
    assert_eq!(live_agent.get("model"), None);
    assert_eq!(live_agent.get("modelRef"), None);
    assert_eq!(
        live_agent.get("variant").and_then(Value::as_str),
        Some(DEFAULT_LIVE_PROXY_VARIANT)
    );
    assert!(live_agent.get("description").is_some());
    assert_eq!(live_agent.get("tools"), Some(&Value::Array(Vec::new())));
    assert_eq!(
        prepared_config.get("default_agent").and_then(Value::as_str),
        Some(DEFAULT_LIVE_PROXY_PROFILE)
    );
    assert_eq!(
        prepared_config
            .get("ui")
            .and_then(Value::as_object)
            .and_then(|ui| ui.get("default_profile"))
            .and_then(Value::as_str),
        Some(DEFAULT_LIVE_PROXY_PROFILE)
    );

    let output = Command::new(resolve_harness_bin())
        .arg("--config")
        .arg(&run_config.config_path)
        .arg("config")
        .arg("validate")
        .output()
        .expect("run config validate for prepared live config");
    assert!(
        output.status.success(),
        "prepared live config failed validation\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn live_provider_turn_summary_marks_recorded_and_unrecorded_providers() {
    let events_body = [
        r#"{"payload":{"event_type":"provider_request_started","data":{"request_id":"req-1"}}}"#,
        r#"{"payload":{"event_type":"task_scheduled","data":{"task_id":"task-1","queue_key":"provider_model:gpt-5.4-mini"}}}"#,
        r#"{"payload":{"event_type":"provider_stream_delta","data":{"request_id":"req-1","delta":"hello"}}}"#,
        r#"{"payload":{"event_type":"provider_request_finished","data":{"request_id":"req-1"}}}"#,
        r#"{"payload":{"event_type":"task_completed","data":{"task_id":"task-1","result_summary":"done"}}}"#,
    ]
    .join("\n");
    let observation = collect_provider_turn_observation(&events_body);

    let default_summary =
        provider_turn_summary(DEFAULT_LIVE_PROXY_PROVIDER, &observation).expect("default summary");
    assert_eq!(
        default_summary
            .get("expectation_status")
            .and_then(Value::as_str),
        Some("recorded")
    );
    assert_eq!(
        default_summary
            .get("observation")
            .and_then(|observation| observation.get("completion_mode"))
            .and_then(Value::as_str),
        Some("stream_delta_and_task_completion")
    );

    let proxy_summary = provider_turn_summary("proxy", &observation).expect("proxy summary");
    assert_eq!(
        proxy_summary
            .get("expectation_status")
            .and_then(Value::as_str),
        Some("unrecorded")
    );
    assert_eq!(proxy_summary.get("expectation"), Some(&Value::Null));
}

#[test]
fn live_visual_checkpoint_writes_png_and_manifest() {
    let artifact_root = unique_temp_dir("live-visual-artifacts");
    let mut visual_run = LiveVisualRun::new_in_with_options(
        artifact_root.join("custom-artifacts"),
        "live_visual_checkpoint_writes_png_and_manifest",
        "run-001",
        LiveVisualRunOptions {
            run_metadata: json!({
                "provider": "default",
                "model": "model-1",
                "profile": "deep",
                "viewport": {
                    "preset": "desktop"
                }
            }),
            ..LiveVisualRunOptions::default()
        },
    )
    .expect("create live visual run");
    let parser = parser_with_screen(&[
        LIVE_TUI_READY_MARKER,
        "draft text visible",
        "shell create finished",
    ]);

    let checkpoint = visual_run
        .capture_checkpoint_with_metadata(
            CHECKPOINT_STARTUP,
            &parser,
            &[LIVE_TUI_READY_MARKER, "draft text visible"],
            &FocusCapture::anchored_exact(LIVE_TUI_READY_MARKER, 24, 3),
            Some(json!({
                "purpose": "unit-test-startup",
                "stage": "startup"
            })),
        )
        .expect("capture startup visual checkpoint");

    assert!(checkpoint.png_path().exists(), "PNG should be written");
    assert!(
        checkpoint
            .png_path()
            .to_string_lossy()
            .contains("live-proxy/live_visual_checkpoint_writes_png_and_manifest/run-001"),
        "PNG path should be namespaced under live-proxy: {}",
        checkpoint.png_path().display()
    );
    assert!(
        checkpoint.manifest_json_path().exists(),
        "manifest.json should be written"
    );
    assert!(
        checkpoint.manifest_jsonl_path().exists(),
        "manifest.jsonl should be written"
    );

    let manifest: Value = serde_json::from_str(
        &fs::read_to_string(checkpoint.manifest_json_path()).expect("read manifest.json"),
    )
    .expect("parse manifest.json");
    assert_eq!(
        manifest.get("run_id").and_then(Value::as_str),
        Some("run-001")
    );
    assert_eq!(
        manifest
            .get("run_metadata")
            .and_then(|meta| meta.get("provider"))
            .and_then(Value::as_str),
        Some("default")
    );

    let checkpoint_entry = manifest
        .get("checkpoints")
        .and_then(Value::as_array)
        .and_then(|entries| entries.first())
        .expect("expected first checkpoint manifest entry");
    assert_eq!(
        checkpoint_entry
            .get("checkpoint_id")
            .and_then(Value::as_str),
        Some(CHECKPOINT_STARTUP)
    );
    assert_eq!(
        checkpoint_entry
            .get("captured_at_stage")
            .and_then(Value::as_str),
        Some(CHECKPOINT_STARTUP)
    );
    assert!(
        checkpoint_entry
            .get("png_path")
            .and_then(Value::as_str)
            .map(|path| path.ends_with("live_proxy_startup.png"))
            .unwrap_or(false),
        "manifest should record PNG path"
    );
    assert_eq!(
        checkpoint_entry
            .get("focus")
            .and_then(|focus| focus.get("found"))
            .and_then(Value::as_bool),
        Some(true)
    );
    assert_eq!(
        checkpoint_entry
            .get("region")
            .and_then(|region| region.get("row"))
            .and_then(Value::as_u64),
        Some(0)
    );
    assert_eq!(
        checkpoint_entry
            .get("metadata")
            .and_then(|meta| meta.get("purpose"))
            .and_then(Value::as_str),
        Some("unit-test-startup")
    );

    let manifest_jsonl =
        fs::read_to_string(checkpoint.manifest_jsonl_path()).expect("read manifest.jsonl");
    let manifest_jsonl_entry: Value = serde_json::from_str(
        manifest_jsonl
            .lines()
            .next()
            .expect("expected manifest.jsonl entry"),
    )
    .expect("parse manifest.jsonl entry");
    assert_eq!(
        manifest_jsonl_entry
            .get("screen_markers")
            .and_then(Value::as_array)
            .map(Vec::len),
        Some(2)
    );
}

#[test]
fn live_visual_checkpoint_missing_marker_falls_back_to_full_frame() {
    let artifact_root = unique_temp_dir("live-visual-artifacts-missing");
    let mut visual_run = LiveVisualRun::new_in(
        artifact_root,
        "live_visual_checkpoint_missing_marker_falls_back_to_full_frame",
        "run-002",
    )
    .expect("create live visual run");
    let parser = parser_with_screen(&[LIVE_TUI_READY_MARKER, "output without anchor token"]);

    let checkpoint = visual_run
        .capture_checkpoint(
            CHECKPOINT_RUN_FINISHED,
            &parser,
            &[LIVE_TUI_READY_MARKER, "missing marker"],
            &FocusCapture::anchored_exact("missing marker", 18, 2),
        )
        .expect("capture fallback visual checkpoint");

    assert!(
        checkpoint.png_path().exists(),
        "PNG should still be written"
    );
    assert!(
        !checkpoint.focus_marker_found(),
        "focus metadata should record missing marker fallback"
    );
    assert_eq!(checkpoint.focus_region_cells(), (0, 0, 24, 80));

    let manifest: Value = serde_json::from_str(
        &fs::read_to_string(checkpoint.manifest_json_path()).expect("read fallback manifest"),
    )
    .expect("parse fallback manifest");
    let checkpoint_entry = manifest
        .get("checkpoints")
        .and_then(Value::as_array)
        .and_then(|entries| entries.first())
        .expect("expected fallback checkpoint manifest entry");
    assert_eq!(
        checkpoint_entry
            .get("focus")
            .and_then(|focus| focus.get("found"))
            .and_then(Value::as_bool),
        Some(false)
    );
    assert_eq!(
        checkpoint_entry
            .get("focus")
            .and_then(|focus| focus.get("scope"))
            .and_then(Value::as_str),
        Some("full_frame_fallback")
    );
    assert_eq!(
        checkpoint_entry
            .get("region")
            .and_then(|region| region.get("height"))
            .and_then(Value::as_u64),
        Some(24)
    );
    assert_eq!(
        checkpoint_entry
            .get("region")
            .and_then(|region| region.get("width"))
            .and_then(Value::as_u64),
        Some(80)
    );
}

#[test]
fn live_visual_checkpoint_marker_assertions_respect_required_and_forbidden_states() {
    let artifact_root = unique_temp_dir("live-visual-assertions");
    let mut visual_run = LiveVisualRun::new_in(
        artifact_root,
        "live_visual_checkpoint_marker_assertions",
        "run-assert",
    )
    .expect("create visual run for marker assertions");
    let parser = parser_with_screen(&[
        LIVE_TUI_READY_MARKER,
        LIVE_TUI_STATUS_SUCCESS_MARKER,
        LIVE_TUI_FINISHED_MARKER,
        "read",
    ]);
    let checkpoint = visual_run
        .capture_checkpoint(
            CHECKPOINT_RUN_FINISHED,
            &parser,
            &[
                LIVE_TUI_READY_MARKER,
                LIVE_TUI_STATUS_SUCCESS_MARKER,
                LIVE_TUI_FINISHED_MARKER,
                LIVE_TUI_ASSISTANT_STREAMING_MARKER,
            ],
            &FocusCapture::anchored_exact("read", 28, 5),
        )
        .expect("capture run-finished checkpoint");

    assert_checkpoint_markers(
        checkpoint.manifest_json_path(),
        CHECKPOINT_RUN_FINISHED,
        &[
            LIVE_TUI_READY_MARKER,
            LIVE_TUI_STATUS_SUCCESS_MARKER,
            LIVE_TUI_FINISHED_MARKER,
        ],
        &[LIVE_TUI_ASSISTANT_STREAMING_MARKER],
    )
    .expect("checkpoint markers should satisfy required and forbidden expectations");
}

#[test]
fn live_visual_manifest_orders_checkpoints_and_jsonl_by_stage() {
    let artifact_root = unique_temp_dir("live-visual-ordering");
    let mut visual_run = LiveVisualRun::new_in_with_options(
        artifact_root,
        "live_visual_manifest_orders_checkpoints_and_jsonl_by_stage",
        "run-ordered",
        LiveVisualRunOptions {
            run_metadata: json!({
                "provider": "default",
                "model": "model-1",
                "profile": "deep",
                "viewport": {
                    "preset": "desktop"
                }
            }),
            ..LiveVisualRunOptions::default()
        },
    )
    .expect("create ordered live visual run");
    let parser = parser_with_screen(&[
        LIVE_TUI_READY_MARKER,
        "permission marker",
        LIVE_TUI_FINISHED_MARKER,
    ]);

    visual_run
        .capture_checkpoint_with_metadata(
            CHECKPOINT_RUN_FINISHED,
            &parser,
            &[LIVE_TUI_READY_MARKER, LIVE_TUI_FINISHED_MARKER],
            &FocusCapture::anchored_exact(LIVE_TUI_FINISHED_MARKER, 24, 3),
            Some(json!({
                "purpose": "ordered-run-finished",
                "stage": CHECKPOINT_RUN_FINISHED,
            })),
        )
        .expect("capture run-finished checkpoint first");
    let permission_checkpoint = visual_run
        .capture_checkpoint_with_metadata(
            CHECKPOINT_PERMISSION_REQUESTED,
            &parser,
            &[LIVE_TUI_READY_MARKER, "permission marker"],
            &FocusCapture::anchored_exact("permission marker", 24, 3),
            Some(json!({
                "purpose": "ordered-permission",
                "stage": CHECKPOINT_PERMISSION_REQUESTED,
            })),
        )
        .expect("capture permission checkpoint second");
    visual_run
        .capture_checkpoint_with_metadata(
            CHECKPOINT_STARTUP,
            &parser,
            &[LIVE_TUI_READY_MARKER],
            &FocusCapture::anchored_exact(LIVE_TUI_READY_MARKER, 24, 3),
            Some(json!({
                "purpose": "ordered-startup",
                "stage": CHECKPOINT_STARTUP,
            })),
        )
        .expect("capture startup checkpoint third");

    let manifest: Value = serde_json::from_str(
        &fs::read_to_string(permission_checkpoint.manifest_json_path())
            .expect("read ordered manifest"),
    )
    .expect("parse ordered manifest");
    let checkpoint_ids = manifest
        .get("checkpoints")
        .and_then(Value::as_array)
        .expect("ordered manifest checkpoints")
        .iter()
        .map(|entry| {
            entry
                .get("checkpoint_id")
                .and_then(Value::as_str)
                .expect("checkpoint id present")
        })
        .collect::<Vec<_>>();
    assert_eq!(
        checkpoint_ids,
        vec![
            CHECKPOINT_STARTUP,
            CHECKPOINT_PERMISSION_REQUESTED,
            CHECKPOINT_RUN_FINISHED,
        ],
        "manifest.json should remain stage-sorted regardless of capture order"
    );

    let manifest_jsonl = fs::read_to_string(permission_checkpoint.manifest_jsonl_path())
        .expect("read ordered manifest.jsonl");
    let manifest_jsonl_ids = manifest_jsonl
        .lines()
        .map(|line| serde_json::from_str::<Value>(line).expect("parse manifest.jsonl entry"))
        .map(|entry| {
            entry
                .get("checkpoint_id")
                .and_then(Value::as_str)
                .expect("checkpoint id present in manifest.jsonl")
                .to_string()
        })
        .collect::<Vec<_>>();
    assert_eq!(
        manifest_jsonl_ids,
        vec![
            CHECKPOINT_STARTUP.to_string(),
            CHECKPOINT_PERMISSION_REQUESTED.to_string(),
            CHECKPOINT_RUN_FINISHED.to_string(),
        ],
        "manifest.jsonl should stay aligned with manifest.json ordering"
    );
}

#[test]
fn selected_live_viewport_honors_preset_override() {
    let _guard = live_proxy_env_lock()
        .lock()
        .expect("live proxy env test lock should not be poisoned");
    with_live_proxy_env(
        &[("HARNESS_LIVE_VISUAL_VIEWPORT", Some(OsStr::new("compact")))],
        || {
            let preset = selected_live_viewport();
            assert_eq!(preset.name, "compact");
            assert_eq!(preset.cols, 120);
            assert_eq!(preset.rows, 36);
        },
    );
}

#[test]
fn live_visual_run_retention_prunes_old_runs() {
    let _guard = live_proxy_env_lock()
        .lock()
        .expect("live proxy env test lock should not be poisoned");
    let artifact_root = unique_temp_dir("live-visual-retention");
    let test_root = artifact_root
        .join("live-proxy")
        .join("live_visual_run_retention_prunes_old_runs");
    fs::create_dir_all(&test_root).expect("create retention test root");
    for name in ["run-old-1", "run-old-2", "run-old-3"] {
        let path = test_root.join(name);
        fs::create_dir_all(&path).expect("create old visual run dir");
        fs::write(path.join("manifest.json"), "{}\n").expect("write old manifest");
        std::thread::sleep(Duration::from_millis(5));
    }

    with_live_proxy_env(
        &[("HARNESS_LIVE_VISUAL_KEEP_RUNS", Some(OsStr::new("1")))],
        || {
            LiveVisualRun::new_in(
                artifact_root.clone(),
                "live_visual_run_retention_prunes_old_runs",
                "run-current",
            )
            .expect("create retained visual run")
        },
    );

    let entries = fs::read_dir(&test_root)
        .expect("read retention output dir")
        .filter_map(|entry| entry.ok())
        .map(|entry| entry.file_name().to_string_lossy().to_string())
        .collect::<Vec<_>>();
    assert!(entries.iter().any(|entry| entry == "run-current"));
    assert!(
        entries.len() <= 2,
        "retention should keep at most current + one old run: {entries:?}"
    );
}

#[test]
fn live_visual_run_retention_keeps_non_run_sidecars() {
    let _guard = live_proxy_env_lock()
        .lock()
        .expect("live proxy env test lock should not be poisoned");
    let artifact_root = unique_temp_dir("live-visual-retention-sidecars");
    let test_root = artifact_root
        .join("live-proxy")
        .join("live_visual_run_retention_keeps_non_run_sidecars");
    fs::create_dir_all(&test_root).expect("create retention sidecar test root");
    let sidecar_dir = test_root.join("notes-cache");
    fs::create_dir_all(&sidecar_dir).expect("create sidecar dir");
    fs::write(sidecar_dir.join("README.txt"), "keep me\n").expect("write sidecar marker");

    for name in ["run-old-1", "run-old-2", "run-old-3"] {
        let path = test_root.join(name);
        fs::create_dir_all(&path).expect("create old visual run dir");
        fs::write(path.join("manifest.json"), "{}\n").expect("write old manifest");
        std::thread::sleep(Duration::from_millis(5));
    }

    with_live_proxy_env(
        &[("HARNESS_LIVE_VISUAL_KEEP_RUNS", Some(OsStr::new("1")))],
        || {
            LiveVisualRun::new_in(
                artifact_root.clone(),
                "live_visual_run_retention_keeps_non_run_sidecars",
                "run-current",
            )
            .expect("create retained visual run with sidecar")
        },
    );

    let entries = fs::read_dir(&test_root)
        .expect("read retention sidecar output dir")
        .filter_map(|entry| entry.ok())
        .map(|entry| entry.file_name().to_string_lossy().to_string())
        .collect::<Vec<_>>();
    assert!(entries.iter().any(|entry| entry == "run-current"));
    assert!(
        entries.iter().any(|entry| entry == "notes-cache"),
        "retention should not prune non-run sidecars: {entries:?}"
    );
    assert!(
        entries.len() <= 3,
        "retention should keep sidecar + current + one manifest-backed old run: {entries:?}"
    );
}

#[tokio::test(flavor = "current_thread")]
async fn live_vision_request_uses_responses_endpoint_and_selected_model() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path(RESPONSES_ENDPOINT_PATH))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("content-type", "application/json")
                .set_body_json(json!({
                    "output": [{
                        "type": "message",
                        "role": "assistant",
                        "content": [{
                            "type": "output_text",
                            "text": json!({
                                "checkpoint_id": CHECKPOINT_STARTUP,
                                "status": "pass",
                                "reasons": ["ready marker is visible"],
                                "observed_markers": [LIVE_TUI_READY_MARKER],
                            })
                            .to_string(),
                        }]
                    }]
                })),
        )
        .mount(&server)
        .await;

    let source_config_path = unique_temp_file("live-vision-request-config", "jsonc");
    let session_dir = unique_temp_dir("live-vision-request-session");
    let source_config = build_live_proxy_test_config(
        "proxy",
        &server.uri(),
        "responses",
        DEFAULT_LIVE_PROXY_MODEL,
        &session_dir,
    );
    fs::write(
        &source_config_path,
        serde_json::to_string_pretty(&source_config).expect("serialize live vision config"),
    )
    .expect("write live vision config");

    let config = {
        let _guard = live_proxy_env_lock()
            .lock()
            .expect("live proxy env test lock should not be poisoned");

        with_live_proxy_env(
            &[
                (
                    "HARNESS_LIVE_PROXY_CONFIG",
                    Some(source_config_path.as_os_str()),
                ),
                ("HARNESS_LIVE_PROXY_PROVIDER", Some(OsStr::new("proxy"))),
                (
                    "HARNESS_LIVE_PROXY_MODEL",
                    Some(OsStr::new(DEFAULT_LIVE_PROXY_MODEL)),
                ),
                (
                    "HARNESS_LIVE_PROXY_VISION_MODEL",
                    Some(OsStr::new("vision-model-override")),
                ),
                ("HARNESS_LIVE_PROXY_PROFILE", None),
                ("HARNESS_LIVE_PROXY_PROMPT", None),
                ("HARNESS_LIVE_PROXY_WAIT_TIMEOUT_MS", None),
            ],
            || {
                resolve_live_prompt_request(&repo_root())
                    .and_then(|request| resolve_live_vision_proxy_config(&request))
            },
        )
    }
    .expect("resolve live vision proxy config");

    let png_path = unique_temp_file("live-vision-request", "png");
    write_tiny_png(&png_path);

    let verdict = live_vision::verify_checkpoint(
        &reqwest::Client::new(),
        &config,
        CHECKPOINT_STARTUP,
        &png_path,
        &[LIVE_TUI_READY_MARKER],
    )
    .await
    .expect("live vision verification should succeed");

    assert_eq!(config.model_id(), "vision-model-override");
    assert_eq!(verdict.checkpoint_id(), CHECKPOINT_STARTUP);
    assert_eq!(verdict.status(), "pass");
    assert_eq!(verdict.reasons(), ["ready marker is visible"]);
    assert_eq!(verdict.observed_markers(), [LIVE_TUI_READY_MARKER]);
    assert_eq!(
        verdict.artifact_path(),
        live_vision::verdict_artifact_path(&png_path)
    );
    assert!(
        verdict.artifact_path().exists(),
        "vision verdict artifact should be written"
    );

    let artifact: Value = serde_json::from_str(
        &fs::read_to_string(verdict.artifact_path()).expect("read live vision artifact"),
    )
    .expect("parse live vision artifact");
    assert_eq!(
        artifact
            .get("request")
            .and_then(|request| request.get("model_id"))
            .and_then(Value::as_str),
        Some("vision-model-override")
    );

    let requests = server
        .received_requests()
        .await
        .expect("request recording must be enabled");
    assert_eq!(requests.len(), 1, "expected exactly one verifier request");
    assert_eq!(requests[0].url.path(), RESPONSES_ENDPOINT_PATH);

    let request_body: Value = requests[0]
        .body_json()
        .expect("live vision request body must be JSON");
    assert_eq!(
        request_body.get("model"),
        Some(&Value::String("vision-model-override".to_string()))
    );
    assert_eq!(
        request_body
            .get("text")
            .and_then(|text| text.get("format"))
            .and_then(|format| format.get("type"))
            .and_then(Value::as_str),
        Some("json_schema")
    );
    assert_eq!(
        request_body
            .get("text")
            .and_then(|text| text.get("format"))
            .and_then(|format| format.get("strict"))
            .and_then(Value::as_bool),
        Some(true)
    );

    let input = request_body
        .get("input")
        .and_then(Value::as_array)
        .expect("live vision request should include input array");
    assert_eq!(
        input.len(),
        1,
        "verifier should submit one screenshot at a time"
    );

    let content = input[0]
        .get("content")
        .and_then(Value::as_array)
        .expect("live vision request should include content array");
    assert_eq!(
        content.len(),
        2,
        "verifier should send one text block and one PNG"
    );
    assert_eq!(
        content[0].get("type").and_then(Value::as_str),
        Some("input_text")
    );
    assert_eq!(
        content[1].get("type").and_then(Value::as_str),
        Some("input_image")
    );
    assert!(
        content[1]
            .get("image_url")
            .and_then(Value::as_str)
            .map(|value| value.starts_with("data:image/png;base64,"))
            .unwrap_or(false),
        "verifier request should inline the PNG as a data URL"
    );
}

#[tokio::test(flavor = "current_thread")]
async fn live_vision_verdict_rejects_non_json_response() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path(RESPONSES_ENDPOINT_PATH))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("content-type", "application/json")
                .set_body_json(json!({
                    "output": [{
                        "type": "message",
                        "role": "assistant",
                        "content": [{
                            "type": "output_text",
                            "text": "definitely not json",
                        }]
                    }]
                })),
        )
        .mount(&server)
        .await;

    let config = LiveVisionProxyConfig::new(
        "proxy".to_string(),
        format!("{}/v1", server.uri()),
        "test-key".to_string(),
        "vision-model".to_string(),
    )
    .expect("build live vision config");
    let png_path = unique_temp_file("live-vision-invalid", "png");
    write_tiny_png(&png_path);

    let err = live_vision::verify_checkpoint(
        &reqwest::Client::new(),
        &config,
        CHECKPOINT_STARTUP,
        &png_path,
        &[LIVE_TUI_READY_MARKER],
    )
    .await
    .expect_err("non-JSON vision verdict should fail closed");

    assert!(
        err.contains("invalid JSON verdict"),
        "unexpected live vision error: {err}"
    );
    assert!(
        !live_vision::verdict_artifact_path(&png_path).exists(),
        "verdict artifact should not be written for malformed verifier output"
    );
}

#[expect(
    dead_code,
    reason = "reserved for the ignored live visual-verifier lane"
)]
fn vision_verdict_satisfies(status: &str) -> bool {
    matches!(
        status.trim().to_ascii_lowercase().as_str(),
        "pass" | "passed" | "satisfied"
    )
}

fn assert_final_visual_checkpoint(artifacts: &LiveToolFlowArtifacts) -> Result<(), String> {
    assert_checkpoint_markers(
        &artifacts.manifest_json_path,
        CHECKPOINT_RUN_FINISHED,
        &[
            LIVE_TUI_STATUS_SUCCESS_MARKER,
            LIVE_TUI_FINISHED_MARKER,
            "read",
        ],
        &[
            LIVE_TUI_ASSISTANT_STREAMING_MARKER,
            LIVE_TUI_WAITING_FOR_RESPONSE_MARKER,
        ],
    )
}

fn live_tool_flow_apply_prompt(line_two_hash: &str) -> String {
    format!(
        concat!(
            "Now perform only step 4 on the same file. Return exactly one edit.hashline_apply tool call and zero prose. ",
            "Use exactly these arguments: ",
            r#"{{\"edit_id\":\"{edit_id}\",\"path\":\"tmp/live_tool_flow.md\",\"ops\":[{{\"Replace\":{{\"expected\":[{{\"line\":2,\"hash\":\"{line_two_hash}\"}}],\"lines\":[\"BETA\"]}}}}]}}. "#,
            "That means only line 2 changes from beta to BETA. Do not insert lines, do not delete lines, do not change alpha, and do not change gamma."
        ),
        edit_id = LIVE_TOOL_FLOW_APPLY_EDIT_ID,
        line_two_hash = line_two_hash,
    )
}

#[expect(
    dead_code,
    reason = "reserved for the ignored live visual-verifier lane"
)]
fn live_vision_checkpoint_contracts() -> &'static [LiveVisionCheckpointContract] {
    &[
        LiveVisionCheckpointContract {
            checkpoint_id: CHECKPOINT_STARTUP,
            expected_markers: &[
                "Compose-first input is visible.",
                "No fatal error banner is visible.",
            ],
        },
        LiveVisionCheckpointContract {
            checkpoint_id: CHECKPOINT_DRAFT_VISIBLE,
            expected_markers: &[LIVE_TOOL_FLOW_DRAFT_MARKER],
        },
        LiveVisionCheckpointContract {
            checkpoint_id: CHECKPOINT_FILE_WRITE_FINISHED,
            expected_markers: &[
                "UI shows file-creation progress for tmp/live_tool_flow.md.",
                "write",
                LIVE_TOOL_FLOW_RELATIVE_PATH,
            ],
        },
        LiveVisionCheckpointContract {
            checkpoint_id: CHECKPOINT_HASHLINE_SCAN_FINISHED,
            expected_markers: &[
                "UI shows scan/edit stage for tmp/live_tool_flow.md.",
                "edit.hashline_scan",
                LIVE_TOOL_FLOW_RELATIVE_PATH,
            ],
        },
        LiveVisionCheckpointContract {
            checkpoint_id: CHECKPOINT_RUN_FINISHED,
            expected_markers: &[
                "Final state shows verification read or assistant confirmation for tmp/live_tool_flow.md.",
                "read",
                LIVE_TOOL_FLOW_RELATIVE_PATH,
            ],
        },
    ]
}

async fn wait_for_wiremock_request_path(
    server: &MockServer,
    expected_path: &str,
    timeout: Duration,
) -> Result<Vec<wiremock::Request>, String> {
    let deadline = Instant::now() + timeout;

    loop {
        let requests = server
            .received_requests()
            .await
            .ok_or_else(|| "request recording must be enabled".to_string())?;
        if requests
            .iter()
            .any(|request| request.url.path() == expected_path)
        {
            return Ok(requests);
        }

        if Instant::now() >= deadline {
            let observed_paths = requests
                .iter()
                .map(|request| request.url.path().to_string())
                .collect::<Vec<_>>();
            return Err(format!(
                "timed out waiting for {expected_path} after {timeout:?}; observed paths: {observed_paths:?}"
            ));
        }

        tokio::time::sleep(LIVE_TUI_READ_POLL_TIMEOUT).await;
    }
}

fn read_shipped_agent_prompt_asset(agent_name: &str) -> String {
    for path in [
        repo_root()
            .join(".agent-harness")
            .join("agents")
            .join(format!("{agent_name}.md")),
        repo_root()
            .join(".agent-harness")
            .join("agents")
            .join(format!("{agent_name}.md")),
    ] {
        if let Ok(content) = fs::read_to_string(&path) {
            return content;
        }
    }

    panic!("failed to read prompt asset for {agent_name}")
}

fn shipped_agent_prompt_body(agent_name: &str) -> String {
    let raw = read_shipped_agent_prompt_asset(agent_name);
    let mut lines = raw.lines();
    if lines.next() != Some("---") {
        return raw;
    }

    for line in lines.by_ref() {
        if line == "---" {
            let body = lines.collect::<Vec<_>>().join("\n");
            return body.trim().to_string();
        }
    }

    raw
}

fn live_run_id() -> Result<String, String> {
    static FORMAT: &[time::format_description::FormatItem<'static>] =
        format_description!("[year][month][day]-[hour][minute][second]-[subsecond digits:6]");
    OffsetDateTime::now_utc()
        .format(FORMAT)
        .map(|timestamp| format!("run-{timestamp}Z"))
        .map_err(|err| format!("failed to format live visual run id: {err}"))
}

fn live_proxy_env_lock() -> &'static Mutex<()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
}

#[allow(unsafe_code)]
fn with_live_proxy_env<T>(vars: &[(&str, Option<&std::ffi::OsStr>)], run: impl FnOnce() -> T) -> T {
    let previous = vars
        .iter()
        .map(|(name, _)| ((*name).to_string(), env::var_os(name)))
        .collect::<Vec<_>>();

    for (name, value) in vars {
        match value {
            Some(value) => {
                unsafe { env::set_var(name, value) };
            }
            None => {
                unsafe { env::remove_var(name) };
            }
        }
    }

    let result = run();

    for (name, value) in previous {
        match value {
            Some(value) => {
                unsafe { env::set_var(&name, value) };
            }
            None => {
                unsafe { env::remove_var(&name) };
            }
        }
    }

    result
}

fn build_live_proxy_test_config(
    provider_name: &str,
    provider_base_uri: &str,
    api_mode: &str,
    configured_model: &str,
    session_dir: &Path,
) -> Value {
    let mut providers = serde_json::Map::new();
    providers.insert(
        provider_name.to_string(),
        json!({
            "type": "openai_compatible",
            "base_url": format!("{provider_base_uri}/v1"),
            "api_key": "test-key",
            "api_mode": api_mode,
            "timeout_ms": 60000,
            "models": {
                configured_model: {
                    "display_name": "Configured model",
                    "variants": {
                        "low": {
                            "display_name": "Low",
                            "metadata": {
                                "reasoning_effort": "low",
                                "text_verbosity": "low",
                                "recommended_for": "live_proxy"
                            }
                        }
                    }
                }
            }
        }),
    );

    let mut categories = serde_json::Map::new();
    categories.insert(
        "deep".to_string(),
        json!({
            "description": "Deep profile",
            "model_ref": format!("{provider_name}:{configured_model}"),
            "tools": []
        }),
    );

    json!({
        "providers": providers,
        "agents": categories,
        "permissions": {
            "defaults": {
                "edit": "allow",
                "shell": "allow",
                "network": "allow"
            }
        },
        "runtime": {
            "background_tasks": {
                "default_concurrency": 2,
                "provider_concurrency": 2,
                "model_concurrency": 2,
                "stale_timeout_ms": 30000,
                "message_staleness_timeout_ms": 10000
            },
            "session_dir": session_dir.display().to_string(),
            "deterministic": {
                "enabled": false,
                "seed": 42
            }
        },
        "integrations": {
            "remote_search": {
                "endpoint": "https://mcp.exa.ai/mcp"
            }
        },
        "ui": {
            "default_profile": "deep"
        }
    })
}

fn deterministic_responses_sse_fixture() -> String {
    concat!(
        "event: response.created\n",
        "data: {\"type\":\"response.created\",\"response\":{\"id\":\"resp_123\"}}\n\n",
        "event: response.output_text.delta\n",
        "data: {\"type\":\"response.output_text.delta\",\"delta\":\"Hello\"}\n\n",
        "event: response.output_text.delta\n",
        "data: {\"type\":\"response.output_text.delta\",\"delta\":\" world\"}\n\n",
        "event: response.completed\n",
        "data: {\"type\":\"response.completed\",\"response\":{\"usage\":{\"prompt_tokens\":4,\"completion_tokens\":2,\"total_tokens\":6}}}\n\n",
        "data: [DONE]\n\n"
    )
    .to_string()
}

fn deterministic_chat_sse_fixture() -> String {
    concat!(
        "data: {\"id\":\"chatcmpl-1\",\"object\":\"chat.completion.chunk\",\"choices\":[{\"index\":0,\"delta\":{\"content\":\"Hello\"},\"finish_reason\":null}],\"usage\":null}\n\n",
        "data: {\"id\":\"chatcmpl-1\",\"object\":\"chat.completion.chunk\",\"choices\":[{\"index\":0,\"delta\":{\"content\":\" world\"},\"finish_reason\":null}],\"usage\":null}\n\n",
        "data: {\"id\":\"chatcmpl-1\",\"object\":\"chat.completion.chunk\",\"choices\":[{\"index\":0,\"delta\":{},\"finish_reason\":\"stop\"}],\"usage\":{\"prompt_tokens\":4,\"completion_tokens\":2,\"total_tokens\":6}}\n\n",
        "data: [DONE]\n\n"
    )
    .to_string()
}

fn resolve_harness_bin() -> PathBuf {
    static HARNESS_BIN_CACHE: OnceLock<PathBuf> = OnceLock::new();
    HARNESS_BIN_CACHE
        .get_or_init(|| {
            if let Ok(path) = env::var("HARNESS_BIN") {
                let harness_bin = PathBuf::from(path);
                assert!(
                    harness_bin.exists(),
                    "HARNESS_BIN points to missing path: {}",
                    harness_bin.display()
                );
                return harness_bin;
            }

            if let Some(path) = option_env!("CARGO_BIN_EXE_harness") {
                let harness_bin = PathBuf::from(path);
                if harness_bin.exists() {
                    return harness_bin;
                }
            }

            let repo = repo_root();
            let harness_bin = repo
                .join("target")
                .join("debug")
                .join(binary_name("harness"));
            let status = Command::new("cargo")
                .arg("build")
                .arg("-p")
                .arg("harness")
                .current_dir(&repo)
                .status()
                .expect("spawn cargo build -p harness");
            assert!(
                status.success(),
                "cargo build -p harness failed with status {status}"
            );
            assert!(
                harness_bin.exists(),
                "expected harness binary at {}",
                harness_bin.display()
            );
            harness_bin
        })
        .clone()
}

fn repo_root() -> PathBuf {
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    manifest_dir
        .parent()
        .and_then(Path::parent)
        .map(Path::to_path_buf)
        .expect("harness-testkit should live under <repo>/crates/harness-testkit")
}

fn unique_temp_file(prefix: &str, ext: &str) -> PathBuf {
    let base = env::temp_dir().join("harness-testkit");
    fs::create_dir_all(&base).expect("create base temp dir");

    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("system clock before unix epoch")
        .as_nanos();

    base.join(format!("{prefix}-{}-{nanos}.{ext}", std::process::id()))
}

fn unique_temp_dir(prefix: &str) -> PathBuf {
    let base = env::temp_dir().join("harness-testkit");
    fs::create_dir_all(&base).expect("create base temp dir");

    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("system clock before unix epoch")
        .as_nanos();

    let dir = base.join(format!("{prefix}-{}-{nanos}", std::process::id()));
    fs::create_dir_all(&dir)
        .unwrap_or_else(|err| panic!("failed creating temp dir {}: {err}", dir.display()));
    dir
}

#[cfg(target_os = "windows")]
fn binary_name(name: &str) -> String {
    format!("{name}.exe")
}

#[cfg(not(target_os = "windows"))]
fn binary_name(name: &str) -> String {
    name.to_string()
}
