use std::cmp;
use std::collections::BTreeMap;
use std::env;
use std::ffi::OsStr;
use std::fs;
use std::io::Write;
use std::net::{TcpStream, ToSocketAddrs};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::mpsc::{Receiver, RecvTimeoutError};
use std::sync::{Mutex, OnceLock};
use std::thread;
use std::time::{Duration, Instant};
use time::{macros::format_description, OffsetDateTime};

mod support;

use portable_pty::{CommandBuilder, PtySize};
use serde_json::{json, Value};
use support::live_events::{resolve_tagged_run_dir, ToolFlowEvidence};
use support::live_provider_parity::{
    assert_provider_turn_completed, assert_registered_provider_turn,
    collect_provider_turn_observation, provider_turn_expectation, provider_turn_summary,
};
use support::live_vision::{self, LiveVisionProxyConfig};
use support::live_visual::{
    assert_checkpoint_markers, default_live_run_metadata, selected_live_viewport, FocusCapture,
    LiveVisualRun, LiveVisualRunOptions, CHECKPOINT_DRAFT_VISIBLE, CHECKPOINT_FILE_WRITE_FINISHED,
    CHECKPOINT_HASHLINE_SCAN_FINISHED, CHECKPOINT_PERMISSION_REQUESTED, CHECKPOINT_RUN_FINISHED,
    CHECKPOINT_STARTUP,
};
use support::pty_process::{spawn_pty_process, SpawnedPtyProcess};
use vt100::Parser as VtParser;
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
const LIVE_PROXY_COMPAT_EDIT_PROFILE: &str = "live_proxy_compat_edit";
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
    "Call user.question with exactly one question using header=Choice and options Yes/No. ",
    "Use this exact payload shape: ",
    r#"[{\"question\":\"Pick one\",\"header\":\"Choice\",\"options\":[{\"label\":\"Yes\",\"description\":\"Choose yes\"},{\"label\":\"No\",\"description\":\"Choose no\"}]}]"#,
    ". After the tool call, reply with exactly LIVE_CHAT_QUESTION_CONFIRMED and nothing else."
);
const LIVE_CHAT_SKILL_PROMPT: &str = concat!(
    "Call skill with name=rust-best-practices. ",
    "After the tool call, reply with exactly LIVE_CHAT_SKILL_CONFIRMED and nothing else."
);
const LIVE_COMPAT_EDIT_RELATIVE_PATH: &str = "tmp/live_compat_edit.md";
const LIVE_COMPAT_EDIT_INITIAL_CONTENT: &str = "alpha\nbeta\ngamma\n";
const LIVE_COMPAT_EDIT_PATCHED_CONTENT: &str = "alpha\nBETA\ngamma\n";
const LIVE_COMPAT_EDIT_PATCH_TEXT: &str = "*** Begin Patch\n*** Update File: tmp/live_compat_edit.md\n@@\n-alpha\n-beta\n-gamma\n+alpha\n+BETA\n+gamma\n*** End Patch";
const LIVE_COMPAT_EDIT_DELETE_PATCH_TEXT: &str =
    "*** Begin Patch\n*** Delete File: tmp/live_compat_edit.md\n*** End Patch";
const LIVE_COMPAT_EDIT_WRITE_PROMPT: &str = concat!(
    "Use tools before the final answer. ",
    "Call write with this exact payload shape: ",
    r#"{"filePath":"tmp/live_compat_edit.md","content":"alpha\nbeta\ngamma\n"}"#,
    ". After the tool call, reply with exactly LIVE_COMPAT_EDIT_WRITE_CONFIRMED and nothing else."
);
const LIVE_COMPAT_EDIT_INITIAL_READ_PROMPT: &str = concat!(
    "Use tools before the final answer. ",
    "Call read with this exact payload shape: ",
    r#"{"filePath":"tmp/live_compat_edit.md"}"#,
    ". After the tool call, reply with exactly LIVE_COMPAT_EDIT_INITIAL_READ_CONFIRMED and nothing else."
);
const LIVE_COMPAT_EDIT_PATCH_PROMPT: &str = concat!(
    "Use tools before the final answer. ",
    "Call apply_patch with this exact payload shape: ",
    r#"{"patchText":"*** Begin Patch\n*** Update File: tmp/live_compat_edit.md\n@@\n-alpha\n-beta\n-gamma\n+alpha\n+BETA\n+gamma\n*** End Patch"}"#,
    ". After the tool call, reply with exactly LIVE_COMPAT_EDIT_PATCH_CONFIRMED and nothing else."
);
const LIVE_COMPAT_EDIT_FINAL_READ_PROMPT: &str = concat!(
    "Use tools before the final answer. ",
    "Call read with this exact payload shape: ",
    r#"{"filePath":"tmp/live_compat_edit.md"}"#,
    ". After the tool call, reply with exactly LIVE_COMPAT_EDIT_FINAL_READ_CONFIRMED and nothing else."
);
const LIVE_COMPAT_EDIT_DELETE_PROMPT: &str = concat!(
    "Use tools before the final answer. ",
    "Call apply_patch with this exact payload shape: ",
    r#"{"patchText":"*** Begin Patch\n*** Delete File: tmp/live_compat_edit.md\n*** End Patch"}"#,
    ". After the tool call, reply with exactly LIVE_COMPAT_EDIT_DELETE_CONFIRMED and nothing else."
);
const LIVE_TOOL_FLOW_CREATE_PROMPT: &str = concat!(
    "You must use tools only. Use exactly tmp/live_tool_flow.md. ",
    "Now perform only step 1: call fs.write with this exact payload shape: ",
    r#"{"path":"tmp/live_tool_flow.md","content":"alpha\nbeta\ngamma\n"}"#,
    ". Return exactly one fs.write tool call and zero prose. Do not call any other tool."
);
const LIVE_TOOL_FLOW_READ_PROMPT: &str = concat!(
    "Now perform only step 2 on the same file: call fs.read with path=tmp/live_tool_flow.md. ",
    "Return exactly one fs.read tool call and zero prose. Do not call any other tool."
);
const LIVE_TOOL_FLOW_SCAN_PROMPT: &str = concat!(
    "Now perform only step 3 on the same file: call edit.hashline_scan with path=tmp/live_tool_flow.md start_line=1 limit=20. ",
    "Return exactly one edit.hashline_scan tool call and zero prose. Do not call any other tool."
);
const LIVE_TOOL_FLOW_FINAL_READ_PROMPT: &str = concat!(
    "Now perform steps 5 and 6 only: call fs.read with path=tmp/live_tool_flow.md again, then summarize the final contents. ",
    "Do not make any more edits and do not use any other file path. Before the summary, there must be exactly one fs.read tool call."
);
const DEFAULT_LIVE_PROXY_PROMPT: &str = "Say hello in exactly five words.";
const DEFAULT_LIVE_PROXY_WAIT_TIMEOUT_MS: &str = "120000";
const RESPONSES_ENDPOINT_PATH: &str = "/v1/responses";
const LIVE_TUI_READY_MARKER: &str = "Ask Harness anything…";
const LIVE_TUI_STATUS_SUCCESS_MARKER: &str = "Success";
const LIVE_TUI_FINISHED_MARKER: &str = "ready for next turn";
const LIVE_TUI_ASSISTANT_STREAMING_MARKER: &str = "assistant · streaming…";
const LIVE_TUI_WAITING_FOR_RESPONSE_MARKER: &str = "Waiting for response…";
const LIVE_TUI_READ_POLL_TIMEOUT: Duration = Duration::from_millis(50);
const LIVE_TUI_STABLE_WINDOW: Duration = Duration::from_millis(180);
const LIVE_TUI_STABLE_TIMEOUT: Duration = Duration::from_secs(2);
const LIVE_TUI_STARTUP_TIMEOUT: Duration = Duration::from_secs(10);
const LIVE_VISUAL_STARTUP_MARKERS: &[&str] = &[LIVE_TUI_READY_MARKER];
const LIVE_TOOL_FLOW_SUMMARY_JSON: &str = "run_summary.json";
const LIVE_TOOL_FLOW_SUMMARY_TXT: &str = "run_summary.txt";

#[derive(Debug, Clone, PartialEq, Eq)]
struct LivePromptRequest {
    source_config_path: PathBuf,
    provider_name: String,
    primary_model: String,
    primary_variant: Option<String>,
    vision_model: String,
    profile: String,
    prompt_text: String,
    wait_timeout_ms: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum LiveSmokeEndpoint {
    Responses,
}

impl LiveSmokeEndpoint {
    const fn path(self) -> &'static str {
        match self {
            Self::Responses => RESPONSES_ENDPOINT_PATH,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct PromptRunConfig {
    config_path: PathBuf,
    provider_name: String,
    profile: String,
    model_id: String,
    variant: Option<String>,
    endpoint: LiveSmokeEndpoint,
    workspace_root: PathBuf,
    session_dir: PathBuf,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct LivePromptVisualArtifacts {
    visual_run_dir: PathBuf,
    manifest_json_path: PathBuf,
    startup_png: PathBuf,
    draft_visible_png: PathBuf,
    run_finished_png: PathBuf,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct LivePromptSmokeResult {
    events_body: String,
    visual_artifacts: LivePromptVisualArtifacts,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct LiveToolFlowRunConfig {
    tool_flow: PromptRunConfig,
    vision_verifier: PromptRunConfig,
    canonical_relative_path: PathBuf,
    namespaces: LiveToolFlowNamespaces,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct LivePromptChatToolRunConfig {
    todo_flow: PromptRunConfig,
    question: PromptRunConfig,
    skill: PromptRunConfig,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct LivePromptNativeToolFlowRunConfig {
    create: PromptRunConfig,
    first_read: PromptRunConfig,
    scan: PromptRunConfig,
    apply: PromptRunConfig,
    final_read: PromptRunConfig,
    canonical_relative_path: PathBuf,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct LivePromptCompatEditRunConfig {
    write: PromptRunConfig,
    first_read: PromptRunConfig,
    patch: PromptRunConfig,
    second_read: PromptRunConfig,
    delete: PromptRunConfig,
    canonical_relative_path: PathBuf,
}

impl LiveToolFlowRunConfig {
    fn visual_test_name(&self) -> &str {
        self.namespaces.visual_test_name
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct LiveToolFlowNamespaces {
    live_tui_session: &'static str,
    tool_flow_session: &'static str,
    vision_verifier_session: &'static str,
    visual_test_name: &'static str,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct LiveToolFlowArtifacts {
    tool_flow_run_dir: PathBuf,
    tool_flow_workspace_root: PathBuf,
    visual_run_dir: PathBuf,
    manifest_json_path: PathBuf,
    manifest_jsonl_path: PathBuf,
    startup_png: PathBuf,
    draft_visible_png: PathBuf,
    shell_create_finished_png: PathBuf,
    hashline_scan_finished_png: PathBuf,
    run_finished_png: PathBuf,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct LivePromptStageResult {
    run_dir: PathBuf,
    events_body: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct LiveVisionCheckpointContract {
    checkpoint_id: &'static str,
    expected_markers: &'static [&'static str],
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct LiveNamespaceAllocation {
    root_dir: PathBuf,
}

impl LiveNamespaceAllocation {
    fn allocate(prefix: &str) -> Result<Self, String> {
        let root_dir = unique_temp_dir(prefix);
        Ok(Self { root_dir })
    }

    fn root_dir(&self) -> &Path {
        &self.root_dir
    }

    fn artifact_file(&self, stem: &str, ext: &str) -> PathBuf {
        self.root_dir.join(format!("{stem}.{ext}"))
    }

    fn session_dir(&self, session_namespace: &str) -> PathBuf {
        self.root_dir.join(".agent-harness").join(session_namespace)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct LiveProxyPreflightReport {
    source_config_path: PathBuf,
    provider_name: String,
    model_id: String,
    variant: Option<String>,
    vision_model_id: String,
    profile: String,
    endpoint_path: &'static str,
    base_url: String,
    socket_address: String,
    harness_bin: PathBuf,
    viewport_preset: &'static str,
}

impl LiveProxyPreflightReport {
    fn summary_text(&self) -> String {
        [
            "Live proxy preflight".to_string(),
            format!("  config: {}", self.source_config_path.display()),
            format!("  provider: {}", self.provider_name),
            format!("  model: {}", self.model_id),
            format!(
                "  variant: {}",
                self.variant.as_deref().unwrap_or("<primary>")
            ),
            format!("  vision model: {}", self.vision_model_id),
            format!("  profile: {}", self.profile),
            format!("  endpoint: {}", self.endpoint_path),
            format!("  base URL: {}", self.base_url),
            format!("  reachable socket: {}", self.socket_address),
            format!("  harness bin: {}", self.harness_bin.display()),
            format!("  viewport preset: {}", self.viewport_preset),
        ]
        .join("\n")
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ToolFlowStage {
    Full,
}

impl ToolFlowStage {
    fn tools(self) -> &'static [&'static str] {
        match self {
            Self::Full => &[
                "fs.write",
                "fs.read",
                "edit.hashline_scan",
                "edit.hashline_apply",
            ],
        }
    }

    fn description(self) -> &'static str {
        match self {
            Self::Full => concat!(
                "Execute the full live tool-flow task in one session. ",
                "Use only fs.write, fs.read, edit.hashline_scan, and edit.hashline_apply against tmp/live_tool_flow.md."
            ),
        }
    }
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
        "CLI parity signoff: live_proxy_prompt_responses_smoke -> live_proxy_prompt_chat_tool_flow -> live_proxy_prompt_native_tool_flow -> live_proxy_prompt_compat_edit_flow"
    );
    live_proxy_prompt_responses_smoke();
    live_proxy_prompt_chat_tool_flow();
    live_proxy_prompt_native_tool_flow();
    live_proxy_prompt_compat_edit_flow();
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
    assert_requested_tool_sequence(&question_result.events_body, &["user.question"])
        .unwrap_or_else(|err| panic!("question-stage tool sequence mismatch: {err}"));
    assert_requested_tool_args(
        &question_result.events_body,
        "user.question",
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
        "user.question",
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
    assert!(
        skill_args
            .get("user_message")
            .and_then(Value::as_str)
            .is_some_and(|value| value.contains("Call skill with name=rust-best-practices.")),
        "skill-stage tool arguments mismatch: {skill_args}"
    );
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
    assert_requested_tool_sequence(&create_result.events_body, &["fs.write"])
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
    assert_requested_tool_sequence(&first_read_result.events_body, &["fs.read"])
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
    assert_requested_tool_sequence(&final_read_result.events_body, &["fs.read"])
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

#[test]
#[ignore = "requires HARNESS_LIVE_PROXY=1 and local CLIproxyAPI access"]
fn live_proxy_prompt_compat_edit_flow() {
    if env::var("HARNESS_LIVE_PROXY").as_deref() != Ok("1") {
        return;
    }

    let repo_root = repo_root();
    let live_request = resolve_live_prompt_request(&repo_root)
        .unwrap_or_else(|err| panic!("failed to resolve live proxy compat-edit inputs: {err}"));
    let run_config = prepare_live_prompt_compat_edit_run_config(&live_request)
        .unwrap_or_else(|err| panic!("failed to prepare live proxy compat-edit config: {err}"));
    let compat_edit_path = run_config
        .write
        .workspace_root
        .join(&run_config.canonical_relative_path);

    let write_result = run_live_prompt_stage(
        &run_config.write,
        LIVE_COMPAT_EDIT_WRITE_PROMPT,
        &live_request.wait_timeout_ms,
        &[],
    )
    .unwrap_or_else(|err| panic!("live compat-edit write stage failed: {err}"));
    assert_requested_tool_sequence(&write_result.events_body, &["write"])
        .unwrap_or_else(|err| panic!("compat-edit write tool sequence mismatch: {err}"));
    assert_requested_tool_args(
        &write_result.events_body,
        "write",
        &json!({
            "filePath": LIVE_COMPAT_EDIT_RELATIVE_PATH,
            "content": LIVE_COMPAT_EDIT_INITIAL_CONTENT,
        }),
    )
    .unwrap_or_else(|err| panic!("compat-edit write tool arguments mismatch: {err}"));
    assert_tool_call_output_contains(&write_result.events_body, "write", "live_compat_edit.md")
        .unwrap_or_else(|err| panic!("compat-edit write output mismatch: {err}"));
    assert_event_log_contains(
        &write_result.events_body,
        "LIVE_COMPAT_EDIT_WRITE_CONFIRMED",
    )
    .unwrap_or_else(|err| panic!("compat-edit write confirmation mismatch: {err}"));
    let initial_content = fs::read_to_string(&compat_edit_path).unwrap_or_else(|err| {
        panic!(
            "failed to read created compat-edit file {}: {err}",
            compat_edit_path.display()
        )
    });
    assert_eq!(initial_content, LIVE_COMPAT_EDIT_INITIAL_CONTENT);

    let first_read_result = run_live_prompt_stage(
        &run_config.first_read,
        LIVE_COMPAT_EDIT_INITIAL_READ_PROMPT,
        &live_request.wait_timeout_ms,
        &[],
    )
    .unwrap_or_else(|err| panic!("live compat-edit initial read stage failed: {err}"));
    assert_requested_tool_sequence(&first_read_result.events_body, &["read"])
        .unwrap_or_else(|err| panic!("compat-edit initial read tool sequence mismatch: {err}"));
    assert_requested_tool_args(
        &first_read_result.events_body,
        "read",
        &json!({
            "filePath": LIVE_COMPAT_EDIT_RELATIVE_PATH,
            "offset": Value::Null,
            "limit": Value::Null,
        }),
    )
    .unwrap_or_else(|err| panic!("compat-edit initial read tool arguments mismatch: {err}"));
    assert_tool_call_output_contains(&first_read_result.events_body, "read", "2: beta")
        .unwrap_or_else(|err| panic!("compat-edit initial read output mismatch: {err}"));
    assert_event_log_contains(
        &first_read_result.events_body,
        "LIVE_COMPAT_EDIT_INITIAL_READ_CONFIRMED",
    )
    .unwrap_or_else(|err| panic!("compat-edit initial read confirmation mismatch: {err}"));

    let patch_result = run_live_prompt_stage(
        &run_config.patch,
        LIVE_COMPAT_EDIT_PATCH_PROMPT,
        &live_request.wait_timeout_ms,
        &[],
    )
    .unwrap_or_else(|err| panic!("live compat-edit patch stage failed: {err}"));
    assert_requested_tool_sequence(&patch_result.events_body, &["apply_patch"])
        .unwrap_or_else(|err| panic!("compat-edit patch tool sequence mismatch: {err}"));
    assert_requested_tool_args(
        &patch_result.events_body,
        "apply_patch",
        &json!({
            "patchText": LIVE_COMPAT_EDIT_PATCH_TEXT,
        }),
    )
    .unwrap_or_else(|err| panic!("compat-edit patch tool arguments mismatch: {err}"));
    assert_tool_call_output_contains(
        &patch_result.events_body,
        "apply_patch",
        "Success. Updated the following files",
    )
    .unwrap_or_else(|err| panic!("compat-edit patch output mismatch: {err}"));
    assert_event_log_contains(
        &patch_result.events_body,
        "LIVE_COMPAT_EDIT_PATCH_CONFIRMED",
    )
    .unwrap_or_else(|err| panic!("compat-edit patch confirmation mismatch: {err}"));
    let patched_content = fs::read_to_string(&compat_edit_path).unwrap_or_else(|err| {
        panic!(
            "failed to read patched compat-edit file {}: {err}",
            compat_edit_path.display()
        )
    });
    assert_eq!(patched_content, LIVE_COMPAT_EDIT_PATCHED_CONTENT);

    let second_read_result = run_live_prompt_stage(
        &run_config.second_read,
        LIVE_COMPAT_EDIT_FINAL_READ_PROMPT,
        &live_request.wait_timeout_ms,
        &[],
    )
    .unwrap_or_else(|err| panic!("live compat-edit final read stage failed: {err}"));
    assert_requested_tool_sequence(&second_read_result.events_body, &["read"])
        .unwrap_or_else(|err| panic!("compat-edit final read tool sequence mismatch: {err}"));
    assert_requested_tool_args(
        &second_read_result.events_body,
        "read",
        &json!({
            "filePath": LIVE_COMPAT_EDIT_RELATIVE_PATH,
            "offset": Value::Null,
            "limit": Value::Null,
        }),
    )
    .unwrap_or_else(|err| panic!("compat-edit final read tool arguments mismatch: {err}"));
    assert_tool_call_output_contains(&second_read_result.events_body, "read", "2: BETA")
        .unwrap_or_else(|err| panic!("compat-edit final read output mismatch: {err}"));
    assert_event_log_contains(
        &second_read_result.events_body,
        "LIVE_COMPAT_EDIT_FINAL_READ_CONFIRMED",
    )
    .unwrap_or_else(|err| panic!("compat-edit final read confirmation mismatch: {err}"));

    let delete_result = run_live_prompt_stage(
        &run_config.delete,
        LIVE_COMPAT_EDIT_DELETE_PROMPT,
        &live_request.wait_timeout_ms,
        &[],
    )
    .unwrap_or_else(|err| panic!("live compat-edit delete stage failed: {err}"));
    assert_requested_tool_sequence(&delete_result.events_body, &["apply_patch"])
        .unwrap_or_else(|err| panic!("compat-edit delete tool sequence mismatch: {err}"));
    assert_requested_tool_args(
        &delete_result.events_body,
        "apply_patch",
        &json!({
            "patchText": LIVE_COMPAT_EDIT_DELETE_PATCH_TEXT,
        }),
    )
    .unwrap_or_else(|err| panic!("compat-edit delete tool arguments mismatch: {err}"));
    let deleted_summary = "live_compat_edit.md";
    assert_tool_call_output_contains(&delete_result.events_body, "apply_patch", deleted_summary)
        .unwrap_or_else(|err| panic!("compat-edit delete output mismatch: {err}"));
    assert_event_log_contains(
        &delete_result.events_body,
        "LIVE_COMPAT_EDIT_DELETE_CONFIRMED",
    )
    .unwrap_or_else(|err| panic!("compat-edit delete confirmation mismatch: {err}"));
    assert!(
        !compat_edit_path.exists(),
        "compat-edit file should be deleted at {}",
        compat_edit_path.display()
    );
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

    let requests = server
        .received_requests()
        .await
        .expect("request recording must be enabled");
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

    let requests = server
        .received_requests()
        .await
        .expect("request recording must be enabled");
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

#[tokio::test(flavor = "current_thread")]
async fn prepared_restricted_tools_config_from_example_loads_in_harness_prompt() {
    let server = MockServer::start().await;
    let response_template = ResponseTemplate::new(200)
        .insert_header("content-type", "text/event-stream")
        .set_body_raw(deterministic_responses_sse_fixture(), "text/event-stream");

    Mock::given(method("POST"))
        .and(path(RESPONSES_ENDPOINT_PATH))
        .respond_with(response_template)
        .mount(&server)
        .await;

    let namespace = LiveNamespaceAllocation::allocate("live-proxy-example-restricted-tools")
        .expect("allocate example restricted-tools namespace");
    let source_config_path = namespace.artifact_file("source-config", "jsonc");
    let source_session_dir = namespace.session_dir("source-session");
    let mut source_config =
        load_json5_config(&repo_root().join("configs").join("harness.example.jsonc"))
            .expect("load shipped example config");
    source_config
        .get_mut("providers")
        .and_then(Value::as_object_mut)
        .and_then(|providers| providers.get_mut(DEFAULT_LIVE_PROXY_PROVIDER))
        .and_then(Value::as_object_mut)
        .expect("example default provider present")
        .extend([
            (
                "base_url".to_string(),
                Value::String(format!("{}/v1", server.uri())),
            ),
            ("api_key".to_string(), Value::String("test-key".to_string())),
            ("api_mode".to_string(), Value::String("auto".to_string())),
        ]);
    source_config
        .get_mut("runtime")
        .and_then(Value::as_object_mut)
        .expect("example runtime present")
        .insert(
            "session_dir".to_string(),
            Value::String(source_session_dir.to_string_lossy().into_owned()),
        );
    fs::write(
        &source_config_path,
        serde_json::to_string_pretty(&source_config).expect("serialize example source config"),
    )
    .expect("write example source config");

    let run_config = prepare_prompt_run_config_with_contract(
        &source_config_path,
        DEFAULT_LIVE_PROXY_PROVIDER,
        DEFAULT_LIVE_PROXY_MODEL,
        Some(DEFAULT_LIVE_PROXY_VARIANT),
        LIVE_PROXY_CHAT_TODO_FLOW_PROFILE,
        PreparedLiveConfigContract::RestrictedTools {
            paths: PreparedLiveConfigPaths {
                workspace_root: namespace.root_dir().to_path_buf(),
                session_dir: namespace.session_dir("prepared-session"),
                prepared_config_path: namespace.artifact_file("prepared-config", "jsonc"),
            },
            description: "Execute the live chat todo flow via todowrite.".to_string(),
            tools: vec!["todowrite".to_string()],
        },
    )
    .expect("prepare restricted-tools config from shipped example");

    let prepared = load_json5_config(&run_config.config_path).expect("load prepared config");
    assert_prepared_config_uses_canonical_profile_keys(&prepared);

    let harness_bin = resolve_harness_bin();
    let events_path = namespace.artifact_file("events", "jsonl");
    let harness_bin_for_run = harness_bin.clone();
    let events_path_for_run = events_path.clone();
    let run_config_for_run = run_config.clone();
    let output = tokio::task::spawn_blocking(move || {
        Command::new(&harness_bin_for_run)
            .arg("prompt")
            .arg("--text")
            .arg("Return hello from the prepared restricted-tools config.")
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
            .expect("spawn harness prompt for prepared restricted-tools config")
    })
    .await
    .expect("join blocking harness run");

    assert!(
        output.status.success(),
        "prepared restricted-tools harness prompt failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let events_body = fs::read_to_string(&events_path)
        .unwrap_or_else(|err| panic!("failed reading {}: {err}", events_path.display()));
    assert_provider_turn_completed(&collect_provider_turn_observation(&events_body))
        .unwrap_or_else(|err| {
            panic!("prepared restricted-tools provider-turn evidence mismatch: {err}")
        });
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
    assert_prepared_config_uses_canonical_profile_keys(&prepared);
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
fn prepare_live_tool_flow_run_config_canonicalizes_example_agent_alias() {
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

    let run_config = prepare_live_tool_flow_run_config(
        &request,
        LiveToolFlowNamespaces {
            live_tui_session: LIVE_TUI_SESSION_NAMESPACE,
            tool_flow_session: TOOL_FLOW_SESSION_NAMESPACE,
            vision_verifier_session: VISION_VERIFIER_SESSION_NAMESPACE,
            visual_test_name: LIVE_PROXY_TUI_TOOL_FLOW_TEST_NAME,
        },
    )
    .expect("prepare live tool-flow config from example config");

    let tool_flow_config = load_json5_config(&run_config.tool_flow.config_path)
        .expect("load prepared tool-flow config");
    assert_prepared_config_uses_canonical_profile_keys(&tool_flow_config);
    assert!(tool_flow_config
        .get("agents")
        .and_then(Value::as_object)
        .is_some_and(|agents| agents.contains_key(LIVE_PROXY_TOOL_FLOW_PROFILE)));

    let vision_config = load_json5_config(&run_config.vision_verifier.config_path)
        .expect("load prepared vision verifier config");
    assert_prepared_config_uses_canonical_profile_keys(&vision_config);
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
            Value::String("fs.write".to_string()),
            Value::String("fs.read".to_string()),
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
    assert_prepared_config_uses_canonical_profile_keys(&todo_config);
    assert_prepared_config_uses_canonical_profile_keys(&question_config);
    assert_prepared_config_uses_canonical_profile_keys(&skill_config);

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
        Some(&vec![Value::String("user.question".to_string())])
    );
    assert_eq!(
        question_profile.get("tool_surface").and_then(Value::as_str),
        Some("native")
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
            .join(".agents")
            .join("skills")
            .join("rust-best-practices")
            .join("SKILL.md")
            .exists(),
        "prepared chat tool workspace should seed rust-best-practices into a local project skill root"
    );
}

#[test]
fn prepare_live_prompt_chat_tool_run_config_canonicalizes_example_agent_alias() {
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

    let run_config = prepare_live_prompt_chat_tool_run_config(&request)
        .expect("prepare live prompt chat tool config from example config");

    for prepared_path in [
        &run_config.todo_flow.config_path,
        &run_config.question.config_path,
        &run_config.skill.config_path,
    ] {
        let prepared = load_json5_config(prepared_path).expect("load prepared chat tool config");
        assert_prepared_config_uses_canonical_profile_keys(&prepared);
    }
}

#[test]
fn prepare_live_prompt_compat_edit_run_config_builds_restricted_agent() {
    let source_config_path = unique_temp_file("live-proxy-compat-edit-config", "jsonc");
    let source_session_dir = unique_temp_dir("live-proxy-compat-edit-source-session");
    let source_config = build_live_proxy_test_config(
        "default",
        "http://127.0.0.1:9999",
        "responses",
        DEFAULT_LIVE_PROXY_MODEL,
        &source_session_dir,
    );
    fs::write(
        &source_config_path,
        serde_json::to_string_pretty(&source_config).expect("serialize compat edit config"),
    )
    .expect("write compat edit config");

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

    let run_config = prepare_live_prompt_compat_edit_run_config(&request)
        .expect("prepare live prompt compat edit config");

    assert_eq!(run_config.write.model_id, DEFAULT_LIVE_PROXY_MODEL);
    assert_eq!(run_config.first_read.model_id, DEFAULT_LIVE_PROXY_MODEL);
    assert_eq!(run_config.patch.model_id, DEFAULT_LIVE_PROXY_MODEL);
    assert_eq!(run_config.second_read.model_id, DEFAULT_LIVE_PROXY_MODEL);
    assert_eq!(run_config.delete.model_id, DEFAULT_LIVE_PROXY_MODEL);
    assert_eq!(
        run_config.canonical_relative_path,
        PathBuf::from(LIVE_COMPAT_EDIT_RELATIVE_PATH)
    );
    assert_eq!(
        run_config.write.workspace_root,
        run_config.first_read.workspace_root
    );
    assert_eq!(
        run_config.write.workspace_root,
        run_config.patch.workspace_root
    );
    assert_eq!(
        run_config.write.workspace_root,
        run_config.second_read.workspace_root
    );
    assert_eq!(
        run_config.write.workspace_root,
        run_config.delete.workspace_root
    );
    assert_ne!(
        run_config.write.session_dir,
        run_config.first_read.session_dir
    );
    assert_ne!(
        run_config.first_read.session_dir,
        run_config.patch.session_dir
    );
    assert_ne!(
        run_config.patch.session_dir,
        run_config.second_read.session_dir
    );
    assert_ne!(
        run_config.second_read.session_dir,
        run_config.delete.session_dir
    );

    let write_config =
        load_json5_config(&run_config.write.config_path).expect("load prepared compat edit config");
    let compat_profile = write_config
        .get("agents")
        .and_then(Value::as_object)
        .and_then(|agents| agents.get(LIVE_PROXY_COMPAT_EDIT_PROFILE))
        .and_then(Value::as_object)
        .expect("compat edit agent present");
    assert_eq!(
        compat_profile.get("tools").and_then(Value::as_array),
        Some(&vec![
            Value::String("write".to_string()),
            Value::String("read".to_string()),
            Value::String("apply_patch".to_string()),
        ])
    );
}

#[test]
fn example_config_ships_canonical_plan_build_and_audit_agents() {
    let config = load_json5_config(&repo_root().join("configs").join("harness.example.jsonc"))
        .expect("load shipped example config");

    let default_provider = config
        .get("providers")
        .and_then(Value::as_object)
        .and_then(|providers| providers.get("default"))
        .and_then(Value::as_object)
        .expect("default provider present in example config");
    assert_eq!(
        default_provider.get("base_url").and_then(Value::as_str),
        Some("http://127.0.0.1:8317/v1")
    );
    assert_eq!(
        default_provider.get("api_key").and_then(Value::as_str),
        Some("${OPENAI_API_KEY:-sk-zerolimit}")
    );
    assert_eq!(
        default_provider.get("api_mode").and_then(Value::as_str),
        Some("auto")
    );

    assert_eq!(
        config.get("default_agent").and_then(Value::as_str),
        Some("build")
    );

    let plan = config
        .get("agent")
        .and_then(Value::as_object)
        .and_then(|agents| agents.get("plan"))
        .and_then(Value::as_object)
        .expect("plan agent present in example config");
    assert_eq!(plan.get("plan_mode").and_then(Value::as_bool), Some(true));
    assert_eq!(
        plan.get("exit_target_profile").and_then(Value::as_str),
        Some("build")
    );
    assert_eq!(
        plan.get("model_ref").and_then(Value::as_str),
        Some("default:gpt-5.4-mini")
    );
    let plan_tools = plan
        .get("tools")
        .and_then(Value::as_array)
        .expect("plan tools array present");
    for required_tool in [
        "plan.exit",
        "todo.write",
        "todo.read",
        "user.question",
        "search.web",
        "web.fetch",
        "search.code",
        "code.lsp",
    ] {
        assert!(
            plan_tools.contains(&Value::String(required_tool.to_string())),
            "plan should expose {required_tool} in the shipped example config"
        );
    }
    let plan_prompt = plan
        .get("system_prompt")
        .and_then(Value::as_str)
        .expect("plan system prompt present");
    assert!(plan_prompt.contains("Remain read-only"));
    assert!(plan_prompt.contains("do not edit files"));
    assert!(plan_prompt.contains("verification steps"));
    assert!(plan_prompt.contains("plan.exit"));
    assert!(plan_prompt.contains("hand off"));

    let build = config
        .get("agent")
        .and_then(Value::as_object)
        .and_then(|agents| agents.get("build"))
        .and_then(Value::as_object)
        .expect("build agent present in example config");
    assert_eq!(
        build.get("model_ref").and_then(Value::as_str),
        Some("default:gpt-5.4-mini")
    );
    assert_eq!(build.get("variant").and_then(Value::as_str), Some("high"));
    let build_tools = build
        .get("tools")
        .and_then(Value::as_array)
        .expect("build tools array present");
    for required_tool in [
        "fs.write",
        "shell.run",
        "edit.hashline_apply",
        "edit.hashline_scan",
        "tool.batch",
        "agent.spawn",
    ] {
        assert!(
            build_tools.contains(&Value::String(required_tool.to_string())),
            "build should expose {required_tool} in the shipped example config"
        );
    }
    let build_prompt = build
        .get("system_prompt")
        .and_then(Value::as_str)
        .expect("build system prompt present");
    assert!(build_prompt.contains("Implement only the approved plan"));
    assert!(build_prompt.contains("narrowest useful verification"));
    assert!(build_prompt.contains("changed files"));
    assert!(build_prompt.contains("what was not tested"));
    assert!(build_prompt.contains("remaining risks"));

    let tool_audit = config
        .get("agent")
        .and_then(Value::as_object)
        .and_then(|agents| agents.get("tool_audit"))
        .and_then(Value::as_object)
        .expect("tool_audit agent present in example config");

    assert_eq!(
        tool_audit.get("model_ref").and_then(Value::as_str),
        Some("default:gpt-5.4-mini")
    );
    assert_eq!(
        tool_audit.get("variant").and_then(Value::as_str),
        Some("deterministic")
    );
    assert_eq!(
        tool_audit.get("tool_failure_mode").and_then(Value::as_str),
        Some("continue_as_tool_message")
    );

    let tools = tool_audit
        .get("tools")
        .and_then(Value::as_array)
        .expect("tool_audit tools array present");
    for required_tool in [
        "skill.load",
        "user.question",
        "code.lsp",
        "agent.spawn",
        "tool.batch",
        "tool.invalid",
        "todo.write",
        "todo.read",
    ] {
        assert!(
            tools.contains(&Value::String(required_tool.to_string())),
            "tool_audit should expose {required_tool} in the shipped example config"
        );
    }

    let system_prompt = tool_audit
        .get("system_prompt")
        .and_then(Value::as_str)
        .expect("tool_audit system prompt present");
    for needle in [
        "skills",
        "question flow",
        "hooks evidence",
        "LSP evidence",
        "subagent",
        "variants",
        "model metadata",
        "HARNESS_QUESTION_ANSWERS",
    ] {
        assert!(
            system_prompt.contains(needle),
            "tool_audit prompt should mention `{needle}` for evidence-first signoff"
        );
    }

    let lifecycle_hooks = config
        .get("hooks")
        .and_then(|hooks| hooks.get("lifecycle"))
        .and_then(Value::as_array)
        .expect("example config lifecycle hooks present");
    assert!(
        !lifecycle_hooks.is_empty(),
        "example config should ship lifecycle hook examples for audit coverage"
    );

    let rust_server = config
        .get("lsp")
        .and_then(|lsp| lsp.get("servers"))
        .and_then(|servers| servers.get("rust"))
        .and_then(Value::as_object)
        .expect("example config rust LSP server present");
    assert_eq!(
        rust_server
            .get("command")
            .and_then(Value::as_array)
            .and_then(|command| command.first())
            .and_then(Value::as_str),
        Some("rust-analyzer")
    );

    let deterministic_variant = config
        .get("providers")
        .and_then(|providers| providers.get("default"))
        .and_then(|provider| provider.get("models"))
        .and_then(|models| models.get("gpt-5.4-mini"))
        .and_then(|model| model.get("variants"))
        .and_then(|variants| variants.get("deterministic"))
        .and_then(Value::as_object)
        .expect("gpt-5.4-mini deterministic variant present");
    assert_eq!(
        deterministic_variant
            .get("metadata")
            .and_then(|metadata| metadata.get("recommended_for"))
            .and_then(Value::as_str),
        Some("tool_audit")
    );

    let low_variant = config
        .get("providers")
        .and_then(|providers| providers.get("default"))
        .and_then(|provider| provider.get("models"))
        .and_then(|models| models.get("gpt-5.4-mini"))
        .and_then(|model| model.get("variants"))
        .and_then(|variants| variants.get("low"))
        .and_then(Value::as_object)
        .expect("gpt-5.4-mini low variant present");
    assert_eq!(
        low_variant
            .get("metadata")
            .and_then(|metadata| metadata.get("reasoning_effort"))
            .and_then(Value::as_str),
        Some("low")
    );
    assert_eq!(
        low_variant
            .get("metadata")
            .and_then(|metadata| metadata.get("recommended_for"))
            .and_then(Value::as_str),
        Some("live_proxy")
    );

    let high_variant = config
        .get("providers")
        .and_then(|providers| providers.get("default"))
        .and_then(|provider| provider.get("models"))
        .and_then(|models| models.get("gpt-5.4-mini"))
        .and_then(|model| model.get("variants"))
        .and_then(|variants| variants.get("high"))
        .and_then(Value::as_object)
        .expect("gpt-5.4-mini high variant present");
    assert_eq!(
        high_variant
            .get("metadata")
            .and_then(|metadata| metadata.get("reasoning_effort"))
            .and_then(Value::as_str),
        Some("high")
    );
    assert_eq!(
        high_variant
            .get("metadata")
            .and_then(|metadata| metadata.get("recommended_for"))
            .and_then(Value::as_str),
        Some("interactive_live")
    );

    let deep_compat = config
        .get("agent")
        .and_then(Value::as_object)
        .and_then(|agents| agents.get("deep_compat"))
        .and_then(Value::as_object)
        .expect("deep_compat agent present in example config");
    assert_eq!(
        deep_compat.get("tool_surface").and_then(Value::as_str),
        Some("compat")
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
            "fs.write",
            json!({
                "path": LIVE_TOOL_FLOW_RELATIVE_PATH,
                "content": "alpha\nbeta\ngamma\n",
            }),
        ),
        finished(2, "call-write"),
        requested(
            3,
            "call-read-1",
            "fs.read",
            json!({
                "path": LIVE_TOOL_FLOW_RELATIVE_PATH,
                "offset": 1,
                "limit": 2000,
                "line_numbers": true,
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
            "fs.read",
            json!({
                "path": LIVE_TOOL_FLOW_RELATIVE_PATH,
                "offset": 1,
                "limit": 2000,
                "line_numbers": true,
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
                "fs.write",
                json!({
                    "path": LIVE_TOOL_FLOW_RELATIVE_PATH,
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
                "fs.read",
                json!({
                    "path": LIVE_TOOL_FLOW_RELATIVE_PATH,
                    "offset": 1,
                    "limit": 2000,
                    "line_numbers": true,
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
                "fs.read",
                json!({
                    "path": LIVE_TOOL_FLOW_RELATIVE_PATH,
                    "offset": 1,
                    "limit": 2000,
                    "line_numbers": true,
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
fn tool_flow_tool_call_state_recognizes_fs_write_same_file_success() {
    let events = concat!(
        r#"{"payload":{"event_type":"tool_call_requested","data":{"tool_call_id":"toolcall_000001","tool_id":"fs.write","args_summary":"{\"content\":\"alpha\\nbeta\\ngamma\\n\",\"path\":\"tmp/live_tool_flow.md\"}"}}}"#,
        "\n",
        r#"{"payload":{"event_type":"tool_call_finished","data":{"tool_call_id":"toolcall_000001","status":"succeeded"}}}"#,
        "\n"
    );

    let state = tool_flow_tool_call_state(
        events,
        Path::new(LIVE_TOOL_FLOW_RELATIVE_PATH),
        "fs.write",
        1,
    )
    .expect("fs.write tool-flow state should parse");

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
                resolve_env_reference_value("${HARNESS_LIVE_PROXY_EMPTY_API_KEY:-sk-zerolimit}")
                    .expect("empty env var should use fallback value");
            assert_eq!(resolved, "sk-zerolimit");
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
        "fs.read",
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
            &FocusCapture::anchored_exact("fs.read", 28, 5),
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

fn parser_with_screen(lines: &[&str]) -> VtParser {
    let mut parser = VtParser::new(24, 80, 0);
    let mut frame = String::from("\u{1b}[2J\u{1b}[H");
    for (idx, line) in lines.iter().enumerate() {
        if idx > 0 {
            frame.push('\n');
        }
        frame.push_str(line);
    }
    parser.process(frame.as_bytes());
    parser
}

fn write_tiny_png(path: &Path) {
    image::RgbImage::from_pixel(1, 1, image::Rgb([0, 0, 0]))
        .save(path)
        .unwrap_or_else(|err| panic!("failed to write tiny PNG {}: {err}", path.display()));
}

fn prepare_live_prompt_run_config(request: &LivePromptRequest) -> Result<PromptRunConfig, String> {
    prepare_prompt_run_config(
        &request.source_config_path,
        &request.provider_name,
        &request.primary_model,
        request.primary_variant.as_deref(),
        &request.profile,
    )
}

fn prepare_live_tool_flow_run_config(
    request: &LivePromptRequest,
    namespaces: LiveToolFlowNamespaces,
) -> Result<LiveToolFlowRunConfig, String> {
    let namespace = LiveNamespaceAllocation::allocate("live-proxy-tool-flow-workspace")?;
    let workspace_root = namespace.root_dir().to_path_buf();
    fs::create_dir_all(workspace_root.join("tmp")).map_err(|err| {
        format!(
            "failed to create tool-flow workspace {}: {err}",
            workspace_root.display()
        )
    })?;

    let tool_flow = prepare_prompt_run_config_with_contract(
        &request.source_config_path,
        &request.provider_name,
        &request.primary_model,
        request.primary_variant.as_deref(),
        LIVE_PROXY_TOOL_FLOW_PROFILE,
        PreparedLiveConfigContract::ToolFlow {
            paths: PreparedLiveConfigPaths {
                workspace_root: workspace_root.clone(),
                session_dir: namespace.session_dir(namespaces.tool_flow_session),
                prepared_config_path: namespace.artifact_file("tool-flow-config", "jsonc"),
            },
            stage: ToolFlowStage::Full,
        },
    )?;

    let vision_verifier = prepare_prompt_run_config_with_contract(
        &request.source_config_path,
        &request.provider_name,
        &request.vision_model,
        None,
        LIVE_PROXY_VISION_VERIFIER_PROFILE,
        PreparedLiveConfigContract::VisionVerifier(PreparedLiveConfigPaths {
            workspace_root: workspace_root.clone(),
            session_dir: namespace.session_dir(namespaces.vision_verifier_session),
            prepared_config_path: namespace.artifact_file("vision-verifier-config", "jsonc"),
        }),
    )?;

    Ok(LiveToolFlowRunConfig {
        tool_flow,
        vision_verifier,
        canonical_relative_path: PathBuf::from(LIVE_TOOL_FLOW_RELATIVE_PATH),
        namespaces,
    })
}

fn prepare_live_prompt_chat_tool_run_config(
    request: &LivePromptRequest,
) -> Result<LivePromptChatToolRunConfig, String> {
    let namespace = LiveNamespaceAllocation::allocate("live-proxy-chat-tool-workspace")?;
    let workspace_root = namespace.root_dir().to_path_buf();
    seed_project_skill(&workspace_root, "rust-best-practices")?;

    let todo_flow = prepare_prompt_run_config_with_contract(
        &request.source_config_path,
        &request.provider_name,
        &request.primary_model,
        request.primary_variant.as_deref(),
        LIVE_PROXY_CHAT_TODO_FLOW_PROFILE,
        PreparedLiveConfigContract::RestrictedTools {
            paths: PreparedLiveConfigPaths {
                workspace_root: workspace_root.clone(),
                session_dir: namespace.session_dir("chat-tool-todo-flow"),
                prepared_config_path: namespace
                    .artifact_file("chat-tool-todo-flow-config", "jsonc"),
            },
            description: "Execute the live chat todo flow via todowrite.".to_string(),
            tools: vec!["todowrite".to_string()],
        },
    )?;

    let question = prepare_prompt_run_config_with_contract(
        &request.source_config_path,
        &request.provider_name,
        &request.primary_model,
        request.primary_variant.as_deref(),
        LIVE_PROXY_CHAT_QUESTION_PROFILE,
        PreparedLiveConfigContract::RestrictedTools {
            paths: PreparedLiveConfigPaths {
                workspace_root: workspace_root.clone(),
                session_dir: namespace.session_dir("chat-tool-question"),
                prepared_config_path: namespace.artifact_file("chat-tool-question-config", "jsonc"),
            },
            description: "Execute the live question flow and stop after answering.".to_string(),
            tools: vec!["user.question".to_string()],
        },
    )?;

    let skill = prepare_prompt_run_config_with_contract(
        &request.source_config_path,
        &request.provider_name,
        &request.primary_model,
        request.primary_variant.as_deref(),
        LIVE_PROXY_CHAT_SKILL_PROFILE,
        PreparedLiveConfigContract::RestrictedTools {
            paths: PreparedLiveConfigPaths {
                workspace_root,
                session_dir: namespace.session_dir("chat-tool-skill"),
                prepared_config_path: namespace.artifact_file("chat-tool-skill-config", "jsonc"),
            },
            description: "Execute the live skill-loading flow and stop after the skill tool call."
                .to_string(),
            tools: vec!["skill".to_string()],
        },
    )?;

    Ok(LivePromptChatToolRunConfig {
        todo_flow,
        question,
        skill,
    })
}

fn seed_project_skill(workspace_root: &Path, skill_name: &str) -> Result<(), String> {
    let source_root = repo_root().join(".agents").join("skills").join(skill_name);
    if !source_root.exists() {
        return Err(format!(
            "required skill `{skill_name}` not found at {}",
            source_root.display()
        ));
    }

    let dest_root = workspace_root
        .join(".agents")
        .join("skills")
        .join(skill_name);
    copy_dir_recursive(&source_root, &dest_root)
}

fn copy_dir_recursive(source: &Path, dest: &Path) -> Result<(), String> {
    fs::create_dir_all(dest)
        .map_err(|err| format!("failed to create {}: {err}", dest.display()))?;
    for entry in
        fs::read_dir(source).map_err(|err| format!("failed to read {}: {err}", source.display()))?
    {
        let entry = entry
            .map_err(|err| format!("failed to read entry under {}: {err}", source.display()))?;
        let source_path = entry.path();
        let dest_path = dest.join(entry.file_name());
        let file_type = entry.file_type().map_err(|err| {
            format!(
                "failed to inspect {} while copying skill fixture: {err}",
                source_path.display()
            )
        })?;
        if file_type.is_dir() {
            copy_dir_recursive(&source_path, &dest_path)?;
        } else if file_type.is_file() {
            fs::copy(&source_path, &dest_path).map_err(|err| {
                format!(
                    "failed to copy {} to {}: {err}",
                    source_path.display(),
                    dest_path.display()
                )
            })?;
        }
    }
    Ok(())
}

fn prepare_live_prompt_native_tool_flow_run_config(
    request: &LivePromptRequest,
) -> Result<LivePromptNativeToolFlowRunConfig, String> {
    let namespace = LiveNamespaceAllocation::allocate("live-proxy-native-tool-flow-workspace")?;
    let workspace_root = namespace.root_dir().to_path_buf();
    fs::create_dir_all(workspace_root.join("tmp")).map_err(|err| {
        format!(
            "failed to create native tool-flow workspace {}: {err}",
            workspace_root.display()
        )
    })?;
    let prepare_stage = |session_namespace: &str, config_stem: &str| {
        prepare_prompt_run_config_with_contract(
            &request.source_config_path,
            &request.provider_name,
            &request.primary_model,
            request.primary_variant.as_deref(),
            LIVE_PROXY_TOOL_FLOW_PROFILE,
            PreparedLiveConfigContract::ToolFlow {
                paths: PreparedLiveConfigPaths {
                    workspace_root: workspace_root.clone(),
                    session_dir: namespace.session_dir(session_namespace),
                    prepared_config_path: namespace.artifact_file(config_stem, "jsonc"),
                },
                stage: ToolFlowStage::Full,
            },
        )
    };

    Ok(LivePromptNativeToolFlowRunConfig {
        create: prepare_stage("native-tool-create", "native-tool-create-config")?,
        first_read: prepare_stage("native-tool-first-read", "native-tool-first-read-config")?,
        scan: prepare_stage("native-tool-scan", "native-tool-scan-config")?,
        apply: prepare_stage("native-tool-apply", "native-tool-apply-config")?,
        final_read: prepare_stage("native-tool-final-read", "native-tool-final-read-config")?,
        canonical_relative_path: PathBuf::from(LIVE_TOOL_FLOW_RELATIVE_PATH),
    })
}

fn prepare_live_prompt_compat_edit_run_config(
    request: &LivePromptRequest,
) -> Result<LivePromptCompatEditRunConfig, String> {
    let namespace = LiveNamespaceAllocation::allocate("live-proxy-compat-edit-workspace")?;
    let workspace_root = namespace.root_dir().to_path_buf();
    let prepare_stage = |session_namespace: &str, config_stem: &str| {
        prepare_prompt_run_config_with_contract(
            &request.source_config_path,
            &request.provider_name,
            &request.primary_model,
            request.primary_variant.as_deref(),
            LIVE_PROXY_COMPAT_EDIT_PROFILE,
            PreparedLiveConfigContract::RestrictedTools {
                paths: PreparedLiveConfigPaths {
                    workspace_root: workspace_root.clone(),
                    session_dir: namespace.session_dir(session_namespace),
                    prepared_config_path: namespace.artifact_file(config_stem, "jsonc"),
                },
                description: "Execute the live compat edit flow via write, read, and apply_patch."
                    .to_string(),
                tools: vec![
                    "write".to_string(),
                    "read".to_string(),
                    "apply_patch".to_string(),
                ],
            },
        )
    };

    Ok(LivePromptCompatEditRunConfig {
        write: prepare_stage("compat-edit-write", "compat-edit-write-config")?,
        first_read: prepare_stage("compat-edit-first-read", "compat-edit-first-read-config")?,
        patch: prepare_stage("compat-edit-patch", "compat-edit-patch-config")?,
        second_read: prepare_stage("compat-edit-second-read", "compat-edit-second-read-config")?,
        delete: prepare_stage("compat-edit-delete", "compat-edit-delete-config")?,
        canonical_relative_path: PathBuf::from(LIVE_COMPAT_EDIT_RELATIVE_PATH),
    })
}

fn resolve_live_prompt_request(repo_root: &Path) -> Result<LivePromptRequest, String> {
    let override_config_path = env::var("HARNESS_LIVE_PROXY_CONFIG")
        .ok()
        .map(PathBuf::from);
    let source_config_path =
        resolve_live_proxy_config_path(repo_root, override_config_path.as_deref())?;
    let config = load_json5_config(&source_config_path)?;
    let provider_name = env::var("HARNESS_LIVE_PROXY_PROVIDER")
        .unwrap_or_else(|_| DEFAULT_LIVE_PROXY_PROVIDER.into());
    let provider = provider_from_config(&config, &provider_name)?;
    let primary_model = resolve_trimmed_env_var("HARNESS_LIVE_PROXY_MODEL")
        .unwrap_or_else(|| first_model_from_provider(provider))?;
    let default_variant = (source_config_path
        == repo_root.join("configs").join("harness.example.jsonc"))
    .then(|| resolve_live_proxy_variant(&config, &provider_name, &primary_model))
    .flatten();
    let primary_variant = resolve_trimmed_env_var("HARNESS_LIVE_PROXY_VARIANT")
        .transpose()?
        .or(default_variant);
    let vision_model = resolve_trimmed_env_var("HARNESS_LIVE_PROXY_VISION_MODEL")
        .unwrap_or_else(|| Ok(primary_model.clone()))?;

    Ok(LivePromptRequest {
        source_config_path,
        provider_name,
        primary_model,
        primary_variant,
        vision_model,
        profile: env::var("HARNESS_LIVE_PROXY_PROFILE")
            .unwrap_or_else(|_| DEFAULT_LIVE_PROXY_PROFILE.into()),
        prompt_text: env::var("HARNESS_LIVE_PROXY_PROMPT")
            .unwrap_or_else(|_| DEFAULT_LIVE_PROXY_PROMPT.into()),
        wait_timeout_ms: env::var("HARNESS_LIVE_PROXY_WAIT_TIMEOUT_MS")
            .unwrap_or_else(|_| DEFAULT_LIVE_PROXY_WAIT_TIMEOUT_MS.to_string()),
    })
}

fn resolve_live_vision_proxy_config(
    request: &LivePromptRequest,
) -> Result<LiveVisionProxyConfig, String> {
    let config = load_json5_config(&request.source_config_path)?;
    let provider = provider_from_config(&config, &request.provider_name)?;
    ensure_provider_uses_responses_compatible_mode(&provider_api_mode(provider))?;

    LiveVisionProxyConfig::new(
        request.provider_name.clone(),
        provider_base_url(provider)?,
        provider_api_key(provider)?,
        request.vision_model.clone(),
    )
}

#[expect(
    dead_code,
    reason = "reserved for the ignored live visual-verifier lane"
)]
fn resolve_live_vision_proxy_config_for_run(
    run_config: &PromptRunConfig,
) -> Result<LiveVisionProxyConfig, String> {
    let config = load_json5_config(&run_config.config_path)?;
    let provider = provider_from_config(&config, DEFAULT_LIVE_PROXY_PROVIDER)?;
    ensure_provider_uses_responses_compatible_mode(&provider_api_mode(provider))?;

    LiveVisionProxyConfig::new(
        run_config.provider_name.clone(),
        provider_base_url(provider)?,
        provider_api_key(provider)?,
        run_config.model_id.clone(),
    )
}

fn resolve_live_proxy_config_path(
    repo_root: &Path,
    override_path: Option<&Path>,
) -> Result<PathBuf, String> {
    let config_path = override_path
        .map(|path| {
            if path.is_absolute() {
                path.to_path_buf()
            } else {
                repo_root.join(path)
            }
        })
        .unwrap_or_else(|| repo_root.join("configs").join("harness.example.jsonc"));
    if config_path.exists() {
        Ok(config_path)
    } else {
        Err(format!(
            "live proxy config not found at {}",
            config_path.display()
        ))
    }
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

fn run_live_proxy_preflight(repo_root: &Path) -> Result<LiveProxyPreflightReport, String> {
    if !cfg!(target_os = "linux") {
        return Err(
            "live proxy preflight currently expects Linux for the TUI live lane".to_string(),
        );
    }

    let request = resolve_live_prompt_request(repo_root)?;
    let run_config = prepare_live_prompt_run_config(&request)?;
    let config = load_json5_config(&request.source_config_path)?;
    let provider = provider_from_config(&config, &request.provider_name)?;
    let base_url = provider_base_url(provider)?;
    let endpoint = resolve_live_smoke_endpoint(provider)?;
    let parsed = reqwest::Url::parse(&base_url)
        .map_err(|err| format!("failed to parse provider base_url `{base_url}`: {err}"))?;
    let host = parsed
        .host_str()
        .ok_or_else(|| format!("provider base_url `{base_url}` is missing a host"))?;
    let port = parsed
        .port_or_known_default()
        .ok_or_else(|| format!("provider base_url `{base_url}` is missing a known port"))?;
    let socket_address = format!("{host}:{port}");
    let resolved = socket_address
        .to_socket_addrs()
        .map_err(|err| format!("failed to resolve {socket_address}: {err}"))?
        .next()
        .ok_or_else(|| format!("no socket addresses resolved for {socket_address}"))?;
    TcpStream::connect_timeout(&resolved, Duration::from_secs(2))
        .map_err(|err| format!("failed to connect to {socket_address}: {err}"))?;

    Ok(LiveProxyPreflightReport {
        source_config_path: request.source_config_path,
        provider_name: request.provider_name,
        model_id: request.primary_model,
        variant: request.primary_variant,
        vision_model_id: request.vision_model,
        profile: run_config.profile,
        endpoint_path: endpoint.path(),
        base_url,
        socket_address,
        harness_bin: resolve_harness_bin(),
        viewport_preset: selected_live_viewport().name,
    })
}

fn prepare_prompt_run_config(
    source_config_path: &Path,
    provider_name: &str,
    selected_model: &str,
    selected_variant: Option<&str>,
    profile_name: &str,
) -> Result<PromptRunConfig, String> {
    let namespace = LiveNamespaceAllocation::allocate("live-proxy-session")?;
    prepare_prompt_run_config_with_contract(
        source_config_path,
        provider_name,
        selected_model,
        selected_variant,
        profile_name,
        PreparedLiveConfigContract::Standard(PreparedLiveConfigPaths {
            workspace_root: repo_root(),
            session_dir: namespace.session_dir("prompt-session"),
            prepared_config_path: namespace.artifact_file("prepared-config", "jsonc"),
        }),
    )
}

fn prepare_prompt_run_config_with_contract(
    source_config_path: &Path,
    provider_name: &str,
    selected_model: &str,
    selected_variant: Option<&str>,
    profile_name: &str,
    contract: PreparedLiveConfigContract,
) -> Result<PromptRunConfig, String> {
    if provider_name.trim().is_empty() {
        return Err("provider name cannot be empty".to_string());
    }
    if profile_name.trim().is_empty() {
        return Err("profile name cannot be empty".to_string());
    }
    if selected_model.trim().is_empty() {
        return Err("selected model cannot be empty".to_string());
    }

    let mut config = load_json5_config(source_config_path)?;
    normalize_legacy_profile_aliases(&mut config)?;

    let provider = provider_from_config(&config, provider_name)?;
    let endpoint = resolve_live_smoke_endpoint(provider)?;
    let selected_model = selected_model.trim().to_string();

    rewrite_selected_provider_to_default(&mut config, provider_name)?;
    normalize_category_model_refs_to_default(&mut config)?;
    ensure_provider_model_entry(&mut config, &selected_model)?;
    ensure_provider_model_variant(&mut config, &selected_model, selected_variant)?;
    ensure_profile_model_ref(&mut config, profile_name, &selected_model)?;
    ensure_profile_variant(&mut config, profile_name, selected_variant)?;
    disable_prepared_determinism(&mut config)?;

    let paths = contract.paths().clone();
    apply_prepared_run_paths(&mut config, &paths.session_dir, profile_name)?;
    apply_allow_permissions(&mut config)?;
    match &contract {
        PreparedLiveConfigContract::Standard(_) => {}
        PreparedLiveConfigContract::ToolFlow { stage, .. } => {
            apply_tool_flow_contract(&mut config, profile_name, *stage)?;
        }
        PreparedLiveConfigContract::RestrictedTools {
            description, tools, ..
        } => apply_restricted_tools_contract(&mut config, profile_name, description, tools)?,
        PreparedLiveConfigContract::VisionVerifier(_) => {}
    }
    normalize_legacy_profile_aliases(&mut config)?;

    let rendered = serde_json::to_string_pretty(&config)
        .map_err(|err| format!("failed to render prepared config JSON: {err}"))?;
    fs::write(&paths.prepared_config_path, rendered).map_err(|err| {
        format!(
            "failed to write prepared config {}: {err}",
            paths.prepared_config_path.display()
        )
    })?;

    Ok(PromptRunConfig {
        config_path: paths.prepared_config_path,
        provider_name: provider_name.trim().to_string(),
        profile: profile_name.to_string(),
        model_id: selected_model,
        variant: selected_variant.map(str::to_string),
        endpoint,
        workspace_root: paths.workspace_root,
        session_dir: paths.session_dir,
    })
}

fn run_live_prompt_stage(
    run_config: &PromptRunConfig,
    prompt: &str,
    wait_timeout_ms: &str,
    extra_env: &[(&str, &str)],
) -> Result<LivePromptStageResult, String> {
    let harness_bin = resolve_harness_bin();
    let mut command = Command::new(&harness_bin);
    command
        .arg("prompt")
        .arg("--text")
        .arg(prompt)
        .arg("--profile")
        .arg(&run_config.profile)
        .arg("--config")
        .arg(&run_config.config_path)
        .env("HARNESS_PROMPT_WAIT_TIMEOUT_MS", wait_timeout_ms)
        .current_dir(&run_config.workspace_root);
    for (name, value) in extra_env {
        command.env(name, value);
    }

    let output = command
        .output()
        .map_err(|err| format!("spawn harness prompt stage: {err}"))?;
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    if !output.status.success() {
        return Err(format!(
            "prompt stage failed with status {:?}\nstdout:\n{}\nstderr:\n{}\nPrepared config: {}\nSelected profile: {}",
            output.status.code(),
            stdout,
            stderr,
            run_config.config_path.display(),
            run_config.profile,
        ));
    }

    let session_namespace = session_namespace_name(&run_config.session_dir)?;
    let run_dir = resolve_tagged_run_dir(&run_config.session_dir, &session_namespace)?;
    let events_path = run_dir.join("events.jsonl");
    let events_body = fs::read_to_string(&events_path)
        .map_err(|err| format!("failed to read {}: {err}", events_path.display()))?;
    assert_events_show_successful_provider_turn(&run_config.provider_name, &events_body);

    Ok(LivePromptStageResult {
        run_dir,
        events_body,
    })
}

fn run_live_tui_smoke(
    request: &LivePromptRequest,
    run_config: &PromptRunConfig,
    timeout: Duration,
) -> Result<LivePromptSmokeResult, String> {
    let harness_bin = resolve_harness_bin();
    let session_dir = run_config.session_dir.clone();
    let mut live_visual = LiveVisualRun::new_with_options(
        "live_proxy_e2e_tui_prompt_responses_smoke",
        &live_run_id()?,
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
        LIVE_TUI_READY_MARKER,
        LIVE_TUI_STARTUP_TIMEOUT,
    )?;
    let startup_checkpoint = live_visual.capture_checkpoint_with_metadata(
        CHECKPOINT_STARTUP,
        &process.parser,
        LIVE_VISUAL_STARTUP_MARKERS,
        &FocusCapture::anchored_exact(LIVE_TUI_READY_MARKER, 24, 3),
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
        &[LIVE_TUI_READY_MARKER, request.prompt_text.as_str()],
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
        wait_for_tui_provider_turn(&session_dir, LIVE_TUI_SESSION_NAMESPACE, timeout)?;
    wait_for_screen_state(
        &mut process.parser,
        &process.output_rx,
        &[LIVE_TUI_STATUS_SUCCESS_MARKER, LIVE_TUI_FINISHED_MARKER],
        &[
            LIVE_TUI_ASSISTANT_STREAMING_MARKER,
            LIVE_TUI_WAITING_FOR_RESPONSE_MARKER,
        ],
        Duration::from_secs(5),
    )?;
    let run_finished_checkpoint = live_visual.capture_checkpoint_with_metadata(
        CHECKPOINT_RUN_FINISHED,
        &process.parser,
        &[
            LIVE_TUI_STATUS_SUCCESS_MARKER,
            LIVE_TUI_FINISHED_MARKER,
            LIVE_TUI_ASSISTANT_STREAMING_MARKER,
            LIVE_TUI_WAITING_FOR_RESPONSE_MARKER,
            request.prompt_text.as_str(),
        ],
        &FocusCapture::anchored_exact(LIVE_TUI_READY_MARKER, 28, 6),
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

fn run_live_tui_tool_flow(
    run_config: &LiveToolFlowRunConfig,
    timeout: Duration,
) -> Result<LiveToolFlowArtifacts, String> {
    let mut live_visual = LiveVisualRun::new_with_options(
        run_config.visual_test_name(),
        &live_run_id()?,
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
        LIVE_TUI_READY_MARKER,
        remaining_before(deadline, "create-stage ready marker")?,
    )?;
    let startup_checkpoint = live_visual.capture_checkpoint_with_metadata(
        CHECKPOINT_STARTUP,
        &stage.parser,
        LIVE_VISUAL_STARTUP_MARKERS,
        &FocusCapture::anchored_exact(LIVE_TUI_READY_MARKER, 24, 3),
        Some(json!({
            "purpose": "tool-flow-startup",
            "session_dir": run_config.tool_flow.session_dir.display().to_string(),
        })),
    )?;
    type_and_flush_live_prompt(&mut stage.writer, LIVE_TOOL_FLOW_CREATE_PROMPT)?;
    wait_for_screen_contains(
        &mut stage.parser,
        &stage.output_rx,
        LIVE_TOOL_FLOW_DRAFT_MARKER,
        remaining_before(deadline, "tool-flow draft marker")?,
    )?;
    let draft_visible_checkpoint = live_visual.capture_checkpoint_with_metadata(
        CHECKPOINT_DRAFT_VISIBLE,
        &stage.parser,
        &[LIVE_TUI_READY_MARKER, LIVE_TOOL_FLOW_DRAFT_MARKER],
        &FocusCapture::anchored(LIVE_TOOL_FLOW_DRAFT_MARKER, 32, 5),
        Some(json!({
            "purpose": "tool-flow-draft-visible",
            "prompt_preview": LIVE_TOOL_FLOW_CREATE_PROMPT,
        })),
    )?;
    submit_live_prompt(&mut stage.writer)?;

    let tool_flow_namespace = session_namespace_name(&run_config.tool_flow.session_dir)?;
    let tool_flow_run_dir = wait_for_tool_flow_tool_call_succeeded(
        &run_config.tool_flow.session_dir,
        &run_config.canonical_relative_path,
        &tool_flow_namespace,
        "fs.write",
        1,
        remaining_before(deadline, "fs.write tool completion")?,
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
        "fs.write",
        Duration::from_secs(5),
    )?;
    let shell_create_finished_checkpoint = live_visual.capture_checkpoint_with_metadata(
        CHECKPOINT_FILE_WRITE_FINISHED,
        &stage.parser,
        &[
            LIVE_TUI_READY_MARKER,
            "fs.write",
            LIVE_TOOL_FLOW_RELATIVE_PATH,
        ],
        &FocusCapture::anchored_exact("fs.write", 28, 5),
        Some(json!({
            "purpose": "tool-flow-stage-finished",
            "stage": "create",
            "stage_tool": "fs.write",
            "session_dir": run_config.tool_flow.session_dir.display().to_string(),
        })),
    )?;
    wait_for_live_tui_idle(
        &mut stage.parser,
        &stage.output_rx,
        remaining_before(deadline, "tool-flow ready for first read")?,
    )?;

    type_and_flush_live_prompt(&mut stage.writer, LIVE_TOOL_FLOW_READ_PROMPT)?;
    submit_live_prompt(&mut stage.writer)?;
    wait_for_tool_flow_tool_call_succeeded(
        &run_config.tool_flow.session_dir,
        &run_config.canonical_relative_path,
        &tool_flow_namespace,
        "fs.read",
        1,
        remaining_before(deadline, "first fs.read completion")?,
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
        remaining_before(deadline, "tool-flow ready for scan")?,
    )?;

    type_and_flush_live_prompt(&mut stage.writer, LIVE_TOOL_FLOW_SCAN_PROMPT)?;
    submit_live_prompt(&mut stage.writer)?;
    wait_for_tool_flow_tool_call_succeeded(
        &run_config.tool_flow.session_dir,
        &run_config.canonical_relative_path,
        &tool_flow_namespace,
        "edit.hashline_scan",
        1,
        remaining_before(deadline, "edit.hashline_scan completion")?,
    )?;
    wait_for_screen_contains(
        &mut stage.parser,
        &stage.output_rx,
        "edit.hashline_scan",
        Duration::from_secs(5),
    )?;
    let scan_events = wait_for_tui_provider_turn_count(
        &run_config.tool_flow.session_dir,
        &tool_flow_namespace,
        3,
        remaining_before(deadline, "scan-stage provider turn completion")?,
    )?;
    assert_events_show_successful_provider_turn(&run_config.tool_flow.provider_name, &scan_events);
    let hashline_scan_finished_checkpoint = live_visual.capture_checkpoint_with_metadata(
        CHECKPOINT_HASHLINE_SCAN_FINISHED,
        &stage.parser,
        &[
            LIVE_TUI_READY_MARKER,
            "edit.hashline_scan",
            LIVE_TOOL_FLOW_RELATIVE_PATH,
        ],
        &FocusCapture::anchored_exact("edit.hashline_scan", 32, 5),
        Some(json!({
            "purpose": "tool-flow-stage-finished",
            "stage": "scan",
            "stage_tool": "edit.hashline_scan",
            "session_dir": run_config.tool_flow.session_dir.display().to_string(),
        })),
    )?;
    let line_two_hash =
        read_hashline_scan_line_hash(&tool_flow_run_dir, &run_config.canonical_relative_path, 2)?;
    wait_for_live_tui_idle(
        &mut stage.parser,
        &stage.output_rx,
        remaining_before(deadline, "tool-flow ready for apply")?,
    )?;

    let apply_prompt = live_tool_flow_apply_prompt(&line_two_hash);
    type_and_flush_live_prompt(&mut stage.writer, &apply_prompt)?;
    submit_live_prompt(&mut stage.writer)?;
    wait_for_tool_flow_tool_call_succeeded(
        &run_config.tool_flow.session_dir,
        &run_config.canonical_relative_path,
        &tool_flow_namespace,
        "edit.hashline_apply",
        1,
        remaining_before(deadline, "edit.hashline_apply completion")?,
    )?;
    let apply_events = wait_for_tui_provider_turn_count(
        &run_config.tool_flow.session_dir,
        &tool_flow_namespace,
        4,
        remaining_before(deadline, "apply-stage provider turn completion")?,
    )?;
    assert_events_show_successful_provider_turn(&run_config.tool_flow.provider_name, &apply_events);
    wait_for_live_tui_idle(
        &mut stage.parser,
        &stage.output_rx,
        remaining_before(deadline, "tool-flow ready for final read")?,
    )?;

    type_and_flush_live_prompt(&mut stage.writer, LIVE_TOOL_FLOW_FINAL_READ_PROMPT)?;
    submit_live_prompt(&mut stage.writer)?;
    wait_for_tool_flow_tool_call_succeeded(
        &run_config.tool_flow.session_dir,
        &run_config.canonical_relative_path,
        &tool_flow_namespace,
        "fs.read",
        2,
        remaining_before(deadline, "final verification fs.read completion")?,
    )?;
    let final_read_events = wait_for_tui_provider_turn_count(
        &run_config.tool_flow.session_dir,
        &tool_flow_namespace,
        5,
        remaining_before(deadline, "final-read provider turn completion")?,
    )?;
    assert_events_show_successful_provider_turn(
        &run_config.tool_flow.provider_name,
        &final_read_events,
    );
    wait_for_screen_contains(
        &mut stage.parser,
        &stage.output_rx,
        "fs.read",
        Duration::from_secs(5),
    )?;
    wait_for_screen_state(
        &mut stage.parser,
        &stage.output_rx,
        &[
            LIVE_TUI_STATUS_SUCCESS_MARKER,
            LIVE_TUI_FINISHED_MARKER,
            "fs.read",
        ],
        &[
            LIVE_TUI_ASSISTANT_STREAMING_MARKER,
            LIVE_TUI_WAITING_FOR_RESPONSE_MARKER,
        ],
        remaining_before(deadline, "final-read visible done state")?,
    )?;
    let run_finished_checkpoint = live_visual.capture_checkpoint_with_metadata(
        CHECKPOINT_RUN_FINISHED,
        &stage.parser,
        &[
            LIVE_TUI_STATUS_SUCCESS_MARKER,
            LIVE_TUI_FINISHED_MARKER,
            LIVE_TUI_ASSISTANT_STREAMING_MARKER,
            LIVE_TUI_WAITING_FOR_RESPONSE_MARKER,
            "fs.read",
        ],
        &FocusCapture::anchored_exact("fs.read", 28, 5),
        Some(json!({
            "purpose": "tool-flow-stage-finished",
            "stage": "final_read",
            "stage_tool": "fs.read",
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
        shell_create_finished_png: shell_create_finished_checkpoint.png_path().to_path_buf(),
        hashline_scan_finished_png: hashline_scan_finished_checkpoint.png_path().to_path_buf(),
        run_finished_png: run_finished_checkpoint.png_path().to_path_buf(),
    })
}

fn assert_final_visual_checkpoint(artifacts: &LiveToolFlowArtifacts) -> Result<(), String> {
    assert_checkpoint_markers(
        &artifacts.manifest_json_path,
        CHECKPOINT_RUN_FINISHED,
        &[
            LIVE_TUI_STATUS_SUCCESS_MARKER,
            LIVE_TUI_FINISHED_MARKER,
            "fs.read",
        ],
        &[
            LIVE_TUI_ASSISTANT_STREAMING_MARKER,
            LIVE_TUI_WAITING_FOR_RESPONSE_MARKER,
        ],
    )
}

fn write_live_tool_flow_summary_artifacts(
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
    let summary_json_path = artifacts.visual_run_dir.join(LIVE_TOOL_FLOW_SUMMARY_JSON);
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
    let summary_txt_path = artifacts.visual_run_dir.join(LIVE_TOOL_FLOW_SUMMARY_TXT);
    fs::write(&summary_txt_path, summary_txt)
        .map_err(|err| format!("failed to write {}: {err}", summary_txt_path.display()))?;

    Ok(())
}

type LiveTuiStageProcess = SpawnedPtyProcess;

fn spawn_live_tui_stage_process(
    run_config: &PromptRunConfig,
) -> Result<LiveTuiStageProcess, String> {
    let harness_bin = resolve_harness_bin();

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

fn session_namespace_name(session_dir: &Path) -> Result<String, String> {
    session_dir
        .file_name()
        .and_then(|value| value.to_str())
        .map(ToOwned::to_owned)
        .ok_or_else(|| {
            format!(
                "failed to derive session namespace label from {}",
                session_dir.display()
            )
        })
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

fn read_hashline_scan_line_hash(
    run_dir: &Path,
    canonical_relative_path: &Path,
    line_number: u64,
) -> Result<String, String> {
    let artifact_path = run_dir
        .join("artifacts")
        .join("hashline_scan")
        .join(format!(
            "{}.json",
            sanitize_hashline_artifact_name(canonical_relative_path)
        ));
    let artifact = read_required_json(&artifact_path)?;
    let anchors = artifact
        .get("anchors")
        .and_then(Value::as_array)
        .ok_or_else(|| {
            format!(
                "hashline scan artifact missing anchors array: {}",
                artifact_path.display()
            )
        })?;
    let hash = anchors.iter().find_map(|anchor| {
        (anchor.get("line").and_then(Value::as_u64) == Some(line_number)).then(|| {
            anchor
                .get("hash")
                .and_then(Value::as_str)
                .map(ToOwned::to_owned)
        })
    });
    hash.flatten().ok_or_else(|| {
        format!(
            "hashline scan artifact {} missing hash for line {}",
            artifact_path.display(),
            line_number
        )
    })
}

fn sanitize_hashline_artifact_name(path: &Path) -> String {
    let rendered = path.display().to_string();
    let sanitized = rendered
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || matches!(ch, '.' | '-' | '_') {
                ch
            } else {
                '_'
            }
        })
        .collect::<String>();
    let trimmed = sanitized.trim_matches('_');
    if trimmed.is_empty() {
        "workspace_root".to_string()
    } else {
        trimmed.to_string()
    }
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
                "fs.write",
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
                "fs.read",
                LIVE_TOOL_FLOW_RELATIVE_PATH,
            ],
        },
    ]
}

#[expect(
    dead_code,
    reason = "reserved for the ignored live visual-verifier lane"
)]
fn write_structured_vision_verdict(
    checkpoint_id: &str,
    verdict: &live_vision::LiveVisionVerdict,
) -> Result<PathBuf, String> {
    let verdict_json = read_required_json(verdict.artifact_path())?;
    let structured_path = verdict
        .artifact_path()
        .with_file_name(format!("{checkpoint_id}.vision.json"));
    let rendered = serde_json::to_string_pretty(&verdict_json)
        .map_err(|err| format!("failed to serialize structured vision verdict JSON: {err}"))?;
    fs::write(&structured_path, rendered).map_err(|err| {
        format!(
            "failed to write structured vision verdict artifact {}: {err}",
            structured_path.display()
        )
    })?;

    Ok(structured_path)
}

fn read_required_json(path: &Path) -> Result<Value, String> {
    let body = fs::read_to_string(path)
        .map_err(|err| format!("failed to read JSON artifact {}: {err}", path.display()))?;
    serde_json::from_str(&body)
        .map_err(|err| format!("failed to parse JSON artifact {}: {err}", path.display()))
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

        thread::sleep(LIVE_TUI_READ_POLL_TIMEOUT);
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum ToolFlowToolCallState {
    Pending,
    Succeeded,
    Failed(String),
}

fn tool_flow_tool_call_state(
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
                } else if expected_tool_id == "shell.run"
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
                } else {
                    success_count += 1;
                }
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
        &[LIVE_TUI_FINISHED_MARKER],
        &[
            LIVE_TUI_ASSISTANT_STREAMING_MARKER,
            LIVE_TUI_WAITING_FOR_RESPONSE_MARKER,
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
        "fs.write" | "fs.read" | "edit.hashline_scan" | "edit.hashline_apply" => args_json
            .as_ref()
            .and_then(|value| value.get("path"))
            .and_then(Value::as_str)
            .map(|path| path == canonical_path)
            .unwrap_or_else(|| args_summary.contains(canonical_path)),
        "shell.run" => args_json
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

fn live_tui_command_timeout(request: &LivePromptRequest) -> Duration {
    let wait_timeout_ms = request
        .wait_timeout_ms
        .trim()
        .parse::<u64>()
        .unwrap_or_else(|_| {
            DEFAULT_LIVE_PROXY_WAIT_TIMEOUT_MS
                .parse::<u64>()
                .expect("default live proxy wait timeout must parse as u64")
        });
    Duration::from_millis(wait_timeout_ms).saturating_add(Duration::from_secs(20))
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
            LIVE_TUI_READ_POLL_TIMEOUT,
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
            LIVE_TUI_READ_POLL_TIMEOUT,
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
    let deadline = Instant::now() + LIVE_TUI_STABLE_TIMEOUT;

    loop {
        let now = Instant::now();
        if now >= deadline {
            return latest;
        }

        let wait_timeout = cmp::min(
            LIVE_TUI_READ_POLL_TIMEOUT,
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

        if Instant::now().saturating_duration_since(stable_since) >= LIVE_TUI_STABLE_WINDOW {
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
            LIVE_TUI_READ_POLL_TIMEOUT,
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

        thread::sleep(LIVE_TUI_READ_POLL_TIMEOUT);
    }
}

fn completed_provider_task_count(events_body: &str) -> usize {
    let mut scheduled = BTreeMap::<String, bool>::new();
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

fn assert_events_show_successful_provider_turn(provider_name: &str, events_body: &str) {
    let observation = collect_provider_turn_observation(events_body);
    assert_provider_turn_completed(&observation).unwrap_or_else(|err| {
        panic!("provider turn evidence mismatch for `{provider_name}`: {err}")
    });
    if provider_turn_expectation(provider_name).is_some() {
        assert_registered_provider_turn(provider_name, &observation).unwrap_or_else(|err| {
            panic!("provider-specific parity expectation mismatch for `{provider_name}`: {err}")
        });
    }
}

fn load_json5_config(config_path: &Path) -> Result<Value, String> {
    let raw = fs::read_to_string(config_path)
        .map_err(|err| format!("failed to read config {}: {err}", config_path.display()))?;
    json5::from_str(&raw).map_err(|err| {
        format!(
            "failed to parse JSON5 config {}: {err}",
            config_path.display()
        )
    })
}

fn provider_from_config<'a>(config: &'a Value, provider_name: &str) -> Result<&'a Value, String> {
    let providers = config
        .get("providers")
        .and_then(Value::as_object)
        .ok_or_else(|| "config must define providers as an object".to_string())?;

    providers
        .get(provider_name)
        .ok_or_else(|| format!("provider `{provider_name}` is missing from config.providers"))
}

fn provider_api_mode(provider: &Value) -> String {
    provider
        .get("api_mode")
        .or_else(|| provider.get("apiMode"))
        .and_then(Value::as_str)
        .unwrap_or("auto")
        .trim()
        .to_ascii_lowercase()
}

fn provider_base_url(provider: &Value) -> Result<String, String> {
    provider
        .get("base_url")
        .or_else(|| provider.get("baseUrl"))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
        .ok_or_else(|| "provider config missing non-empty `base_url`".to_string())
}

fn provider_api_key(provider: &Value) -> Result<String, String> {
    let raw = provider
        .get("api_key")
        .or_else(|| provider.get("apiKey"))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| "provider config missing non-empty `api_key`".to_string())?;

    resolve_env_reference_value(raw)
}

fn resolve_live_smoke_endpoint(provider: &Value) -> Result<LiveSmokeEndpoint, String> {
    let api_mode = provider_api_mode(provider);
    ensure_provider_uses_responses_compatible_mode(&api_mode)?;
    Ok(LiveSmokeEndpoint::Responses)
}

fn ensure_provider_uses_responses_compatible_mode(api_mode: &str) -> Result<(), String> {
    match api_mode {
        "responses" | "auto" => Ok(()),
        "chat_completions" => Err(
            "live CLI proxy E2E requires provider api_mode set to responses or auto; found chat_completions"
                .to_string(),
        ),
        other => Err(format!(
            "unsupported api_mode `{other}` for live CLI proxy E2E; expected responses or auto"
        )),
    }
}

fn first_model_from_provider(provider: &Value) -> Result<String, String> {
    let Some(models) = provider.get("models").and_then(Value::as_object) else {
        return Err(
            "provider config has no `models` object; set HARNESS_LIVE_PROXY_MODEL explicitly"
                .to_string(),
        );
    };

    if models.contains_key(DEFAULT_LIVE_PROXY_MODEL) {
        return Ok(DEFAULT_LIVE_PROXY_MODEL.to_string());
    }

    models.keys().next().cloned().ok_or_else(|| {
        "provider config has an empty `models` map; set HARNESS_LIVE_PROXY_MODEL".to_string()
    })
}

fn resolve_live_proxy_variant(
    config: &Value,
    provider_name: &str,
    model_id: &str,
) -> Option<String> {
    let provider = provider_from_config(config, provider_name).ok()?;
    provider
        .get("models")
        .and_then(Value::as_object)
        .and_then(|models| models.get(model_id))
        .and_then(Value::as_object)
        .and_then(|model| model.get("variants"))
        .and_then(Value::as_object)
        .filter(|variants| variants.contains_key(DEFAULT_LIVE_PROXY_VARIANT))
        .map(|_| DEFAULT_LIVE_PROXY_VARIANT.to_string())
}

fn rewrite_selected_provider_to_default(
    config: &mut Value,
    provider_name: &str,
) -> Result<(), String> {
    let root = config
        .as_object_mut()
        .ok_or_else(|| "config root must be a JSON object".to_string())?;
    let providers = root
        .get_mut("providers")
        .and_then(Value::as_object_mut)
        .ok_or_else(|| "config.providers must be an object".to_string())?;

    let selected_provider = providers
        .get(provider_name)
        .cloned()
        .ok_or_else(|| format!("provider `{provider_name}` is missing from config.providers"))?;

    providers.insert(DEFAULT_LIVE_PROXY_PROVIDER.to_string(), selected_provider);
    Ok(())
}

fn normalize_category_model_refs_to_default(config: &mut Value) -> Result<(), String> {
    let root = config
        .as_object_mut()
        .ok_or_else(|| "config root must be a JSON object".to_string())?;
    let categories = root
        .get_mut("agents")
        .and_then(Value::as_object_mut)
        .ok_or_else(|| "config.agents must be an object".to_string())?;

    for (category_name, category_value) in categories.iter_mut() {
        let Some(category_obj) = category_value.as_object_mut() else {
            return Err(format!("agent `{category_name}` must be an object"));
        };

        let model_ref = category_obj
            .get("model_ref")
            .or_else(|| category_obj.get("modelRef"))
            .and_then(Value::as_str)
            .map(str::trim)
            .unwrap_or_default();
        if model_ref.is_empty() {
            continue;
        }

        let model_id = model_ref
            .split_once(':')
            .map(|(_, model_id)| model_id)
            .unwrap_or(model_ref)
            .trim();
        if model_id.is_empty() {
            continue;
        }

        category_obj.insert(
            "model_ref".to_string(),
            Value::String(format!("default:{model_id}")),
        );
    }

    Ok(())
}

fn normalize_legacy_profile_aliases(config: &mut Value) -> Result<(), String> {
    let root = config
        .as_object_mut()
        .ok_or_else(|| "config root must be a JSON object".to_string())?;

    if !root.contains_key("agents") {
        if let Some(agent_alias) = root.get("agent").cloned() {
            root.insert("agents".to_string(), agent_alias);
        }
    }
    root.remove("agent");

    let default_profile = root
        .get("default_agent")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned);

    if let Some(default_profile) = default_profile {
        let ui = root
            .entry("ui".to_string())
            .or_insert_with(|| Value::Object(serde_json::Map::new()))
            .as_object_mut()
            .ok_or_else(|| "config.ui must be an object".to_string())?;
        ui.entry("default_profile".to_string())
            .or_insert_with(|| Value::String(default_profile));
    }
    root.remove("default_agent");

    Ok(())
}

fn assert_prepared_config_uses_canonical_profile_keys(config: &Value) {
    assert!(
        config.get("agent").is_none(),
        "prepared config should not retain legacy top-level `agent`: {config:#}"
    );
    assert!(
        config.get("agents").and_then(Value::as_object).is_some(),
        "prepared config should retain canonical top-level `agents`: {config:#}"
    );
    assert!(
        config.get("default_agent").is_none(),
        "prepared config should not retain legacy top-level `default_agent`: {config:#}"
    );
    assert!(
        config
            .get("ui")
            .and_then(Value::as_object)
            .and_then(|ui| ui.get("default_profile"))
            .is_some(),
        "prepared config should retain canonical `ui.default_profile`: {config:#}"
    );
}

fn ensure_profile_model_ref(
    config: &mut Value,
    profile_name: &str,
    model_id: &str,
) -> Result<(), String> {
    let root = config
        .as_object_mut()
        .ok_or_else(|| "config root must be a JSON object".to_string())?;

    let categories = root
        .entry("agents".to_string())
        .or_insert_with(|| Value::Object(serde_json::Map::new()))
        .as_object_mut()
        .ok_or_else(|| "config.agents must be an object".to_string())?;

    let mut profile = categories.get(profile_name).cloned().unwrap_or_else(|| {
        json!({
            "description": "Live proxy smoke profile",
            "tools": []
        })
    });

    let profile_obj = profile
        .as_object_mut()
        .ok_or_else(|| format!("agent `{profile_name}` must be an object"))?;
    profile_obj.insert(
        "model_ref".to_string(),
        Value::String(format!("default:{model_id}")),
    );
    profile_obj
        .entry("description".to_string())
        .or_insert_with(|| Value::String("Live proxy smoke profile".to_string()));
    profile_obj
        .entry("tools".to_string())
        .or_insert_with(|| Value::Array(Vec::new()));

    categories.insert(profile_name.to_string(), profile);
    Ok(())
}

fn ensure_profile_variant(
    config: &mut Value,
    profile_name: &str,
    selected_variant: Option<&str>,
) -> Result<(), String> {
    let root = config
        .as_object_mut()
        .ok_or_else(|| "config root must be a JSON object".to_string())?;
    let categories = root
        .get_mut("agents")
        .and_then(Value::as_object_mut)
        .ok_or_else(|| "config.agents must be an object".to_string())?;
    let profile = categories
        .get_mut(profile_name)
        .and_then(Value::as_object_mut)
        .ok_or_else(|| format!("agent `{profile_name}` must be an object"))?;

    match selected_variant {
        Some(variant) if !variant.trim().is_empty() => {
            profile.insert(
                "variant".to_string(),
                Value::String(variant.trim().to_string()),
            );
        }
        _ => {
            profile.remove("variant");
        }
    }

    Ok(())
}

fn ensure_provider_model_entry(config: &mut Value, model_id: &str) -> Result<(), String> {
    let root = config
        .as_object_mut()
        .ok_or_else(|| "config root must be a JSON object".to_string())?;
    let providers = root
        .get_mut("providers")
        .and_then(Value::as_object_mut)
        .ok_or_else(|| "config.providers must be an object".to_string())?;
    let provider = providers
        .get_mut(DEFAULT_LIVE_PROXY_PROVIDER)
        .and_then(Value::as_object_mut)
        .ok_or_else(|| format!("provider `{DEFAULT_LIVE_PROXY_PROVIDER}` must be an object"))?;
    let models = provider
        .entry("models".to_string())
        .or_insert_with(|| Value::Object(serde_json::Map::new()))
        .as_object_mut()
        .ok_or_else(|| {
            format!("provider `{DEFAULT_LIVE_PROXY_PROVIDER}` models must be an object")
        })?;

    if models.contains_key(model_id) {
        return Ok(());
    }

    let mut prepared_model = models
        .values()
        .next()
        .cloned()
        .unwrap_or_else(|| Value::Object(serde_json::Map::new()));
    let prepared_model_obj = prepared_model.as_object_mut().ok_or_else(|| {
        format!("provider `{DEFAULT_LIVE_PROXY_PROVIDER}` model entries must be objects")
    })?;
    prepared_model_obj.insert(
        "display_name".to_string(),
        Value::String(format!("Prepared {model_id}")),
    );
    models.insert(model_id.to_string(), prepared_model);
    Ok(())
}

fn ensure_provider_model_variant(
    config: &mut Value,
    model_id: &str,
    selected_variant: Option<&str>,
) -> Result<(), String> {
    let Some(selected_variant) = selected_variant
        .map(str::trim)
        .filter(|value| !value.is_empty())
    else {
        return Ok(());
    };

    if selected_variant != DEFAULT_LIVE_PROXY_VARIANT {
        return Ok(());
    }

    let root = config
        .as_object_mut()
        .ok_or_else(|| "config root must be a JSON object".to_string())?;
    let providers = root
        .get_mut("providers")
        .and_then(Value::as_object_mut)
        .ok_or_else(|| "config.providers must be an object".to_string())?;
    let provider = providers
        .get_mut(DEFAULT_LIVE_PROXY_PROVIDER)
        .and_then(Value::as_object_mut)
        .ok_or_else(|| format!("provider `{DEFAULT_LIVE_PROXY_PROVIDER}` must be an object"))?;
    let models = provider
        .get_mut("models")
        .and_then(Value::as_object_mut)
        .ok_or_else(|| {
            format!("provider `{DEFAULT_LIVE_PROXY_PROVIDER}` models must be an object")
        })?;
    let model = models
        .get_mut(model_id)
        .and_then(Value::as_object_mut)
        .ok_or_else(|| {
            format!("provider `{DEFAULT_LIVE_PROXY_PROVIDER}` is missing model `{model_id}`")
        })?;
    let variants = model
        .entry("variants".to_string())
        .or_insert_with(|| Value::Object(serde_json::Map::new()))
        .as_object_mut()
        .ok_or_else(|| format!("model `{model_id}` variants must be an object"))?;

    variants
        .entry(DEFAULT_LIVE_PROXY_VARIANT.to_string())
        .or_insert_with(|| {
            json!({
                "display_name": "Live signoff",
                "metadata": {
                    "reasoning_effort": "low",
                    "text_verbosity": "low",
                    "recommended_for": "live_proxy",
                }
            })
        });

    Ok(())
}

#[derive(Debug, Clone)]
struct PreparedLiveConfigPaths {
    workspace_root: PathBuf,
    session_dir: PathBuf,
    prepared_config_path: PathBuf,
}

#[derive(Debug, Clone)]
enum PreparedLiveConfigContract {
    Standard(PreparedLiveConfigPaths),
    ToolFlow {
        paths: PreparedLiveConfigPaths,
        stage: ToolFlowStage,
    },
    RestrictedTools {
        paths: PreparedLiveConfigPaths,
        description: String,
        tools: Vec<String>,
    },
    VisionVerifier(PreparedLiveConfigPaths),
}

impl PreparedLiveConfigContract {
    fn paths(&self) -> &PreparedLiveConfigPaths {
        match self {
            Self::Standard(paths) | Self::VisionVerifier(paths) => paths,
            Self::ToolFlow { paths, .. } => paths,
            Self::RestrictedTools { paths, .. } => paths,
        }
    }
}

fn apply_prepared_run_paths(
    config: &mut Value,
    session_dir: &Path,
    profile_name: &str,
) -> Result<(), String> {
    fs::create_dir_all(session_dir).map_err(|err| {
        format!(
            "failed to create prepared session dir {}: {err}",
            session_dir.display()
        )
    })?;

    let root = config
        .as_object_mut()
        .ok_or_else(|| "config root must be a JSON object".to_string())?;
    let runtime = root
        .entry("runtime".to_string())
        .or_insert_with(|| Value::Object(serde_json::Map::new()))
        .as_object_mut()
        .ok_or_else(|| "config.runtime must be an object".to_string())?;
    runtime.insert(
        "session_dir".to_string(),
        Value::String(session_dir.display().to_string()),
    );

    let ui = root
        .entry("ui".to_string())
        .or_insert_with(|| Value::Object(serde_json::Map::new()))
        .as_object_mut()
        .ok_or_else(|| "config.ui must be an object".to_string())?;
    ui.insert(
        "default_profile".to_string(),
        Value::String(profile_name.to_string()),
    );

    Ok(())
}

fn apply_allow_permissions(config: &mut Value) -> Result<(), String> {
    let root = config
        .as_object_mut()
        .ok_or_else(|| "config root must be a JSON object".to_string())?;
    let permissions = root
        .entry("permissions".to_string())
        .or_insert_with(|| Value::Object(serde_json::Map::new()))
        .as_object_mut()
        .ok_or_else(|| "config.permissions must be an object".to_string())?;
    let defaults = permissions
        .entry("defaults".to_string())
        .or_insert_with(|| Value::Object(serde_json::Map::new()))
        .as_object_mut()
        .ok_or_else(|| "config.permissions.defaults must be an object".to_string())?;
    defaults.insert("edit".to_string(), Value::String("allow".to_string()));
    defaults.insert("shell".to_string(), Value::String("allow".to_string()));
    defaults.insert("network".to_string(), Value::String("allow".to_string()));
    defaults.insert("question".to_string(), Value::String("allow".to_string()));
    Ok(())
}

fn disable_prepared_determinism(config: &mut Value) -> Result<(), String> {
    let root = config
        .as_object_mut()
        .ok_or_else(|| "config root must be a JSON object".to_string())?;
    let runtime = root
        .entry("runtime".to_string())
        .or_insert_with(|| Value::Object(serde_json::Map::new()))
        .as_object_mut()
        .ok_or_else(|| "config.runtime must be an object".to_string())?;
    let deterministic = runtime
        .entry("deterministic".to_string())
        .or_insert_with(|| Value::Object(serde_json::Map::new()))
        .as_object_mut()
        .ok_or_else(|| "config.runtime.deterministic must be an object".to_string())?;
    deterministic.insert("enabled".to_string(), Value::Bool(false));
    Ok(())
}

fn apply_tool_flow_contract(
    config: &mut Value,
    profile_name: &str,
    stage: ToolFlowStage,
) -> Result<(), String> {
    let root = config
        .as_object_mut()
        .ok_or_else(|| "config root must be a JSON object".to_string())?;
    let permissions = root
        .entry("permissions".to_string())
        .or_insert_with(|| Value::Object(serde_json::Map::new()))
        .as_object_mut()
        .ok_or_else(|| "config.permissions must be an object".to_string())?;
    permissions.insert(
        "shell_allowlist".to_string(),
        json!({
            "executables": ["sh"],
            "cwd_roots": ["."],
        }),
    );

    let categories = root
        .get_mut("agents")
        .and_then(Value::as_object_mut)
        .ok_or_else(|| "config.agents must be an object".to_string())?;
    let profile = categories
        .get_mut(profile_name)
        .and_then(Value::as_object_mut)
        .ok_or_else(|| format!("agent `{profile_name}` must be present and be an object"))?;
    profile.insert(
        "description".to_string(),
        Value::String(stage.description().to_string()),
    );
    profile.insert(
        "permissions".to_string(),
        json!({
            "edit": "allow",
            "shell": "allow",
            "network": "allow",
            "question": "allow",
        }),
    );
    profile.insert(
        "tools".to_string(),
        Value::Array(
            stage
                .tools()
                .iter()
                .map(|tool| Value::String((*tool).to_string()))
                .collect(),
        ),
    );

    Ok(())
}

fn apply_restricted_tools_contract(
    config: &mut Value,
    profile_name: &str,
    description: &str,
    tools: &[String],
) -> Result<(), String> {
    let root = config
        .as_object_mut()
        .ok_or_else(|| "config root must be a JSON object".to_string())?;
    let categories = root
        .get_mut("agents")
        .and_then(Value::as_object_mut)
        .ok_or_else(|| "config.agents must be an object".to_string())?;
    let profile = categories
        .get_mut(profile_name)
        .and_then(Value::as_object_mut)
        .ok_or_else(|| format!("agent `{profile_name}` must be present and be an object"))?;
    profile.insert(
        "description".to_string(),
        Value::String(description.to_string()),
    );
    profile.insert(
        "tool_surface".to_string(),
        Value::String(restricted_tools_surface(tools).to_string()),
    );
    profile.insert(
        "permissions".to_string(),
        json!({
            "edit": "allow",
            "shell": "allow",
            "network": "allow",
        }),
    );
    profile.insert(
        "tools".to_string(),
        Value::Array(tools.iter().cloned().map(Value::String).collect()),
    );
    Ok(())
}

fn restricted_tools_surface(tools: &[String]) -> &'static str {
    if tools.iter().any(|tool| tool.contains('.')) {
        "native"
    } else {
        "compat"
    }
}

fn assert_requested_tool_args(
    events_body: &str,
    expected_tool_id: &str,
    expected_args: &Value,
) -> Result<(), String> {
    let args = first_requested_tool_args(events_body, expected_tool_id)?
        .ok_or_else(|| format!("expected requested args for `{expected_tool_id}`"))?;
    if &args == expected_args {
        Ok(())
    } else {
        Err(format!(
            "expected `{expected_tool_id}` args {} ; found {}",
            expected_args, args
        ))
    }
}

fn assert_tool_call_output_contains(
    events_body: &str,
    expected_tool_id: &str,
    needle: &str,
) -> Result<(), String> {
    let output = first_tool_call_output_summary(events_body, expected_tool_id)?
        .ok_or_else(|| format!("expected output summary for `{expected_tool_id}`"))?;
    if output.contains(needle) {
        Ok(())
    } else {
        Err(format!(
            "expected `{expected_tool_id}` output summary to contain `{needle}`; found `{output}`"
        ))
    }
}

fn assert_event_log_contains(events_body: &str, needle: &str) -> Result<(), String> {
    if events_body.contains(needle) {
        Ok(())
    } else {
        Err(format!("expected event log to contain `{needle}`"))
    }
}

fn assert_requested_tool_sequence(
    events_body: &str,
    expected_tools: &[&str],
) -> Result<(), String> {
    let mut requested = Vec::<(String, String)>::new();
    let mut finished = BTreeMap::<String, String>::new();

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
            "tool_call_requested" => {
                let Some(tool_id) = data.get("tool_id").and_then(Value::as_str) else {
                    continue;
                };
                let Some(tool_call_id) = data.get("tool_call_id").and_then(Value::as_str) else {
                    continue;
                };
                requested.push((tool_id.to_string(), tool_call_id.to_string()));
            }
            "tool_call_finished" => {
                let Some(tool_call_id) = data.get("tool_call_id").and_then(Value::as_str) else {
                    continue;
                };
                let status = data
                    .get("status")
                    .and_then(Value::as_str)
                    .unwrap_or("missing");
                finished.insert(tool_call_id.to_string(), status.to_string());
            }
            _ => {}
        }
    }

    let actual_tools = requested
        .iter()
        .map(|(tool_id, _)| tool_id.as_str())
        .collect::<Vec<_>>();
    if actual_tools != expected_tools {
        return Err(format!(
            "expected requested tool sequence {:?}; found {:?}",
            expected_tools, actual_tools
        ));
    }

    for (tool_id, tool_call_id) in requested {
        let status = finished
            .get(&tool_call_id)
            .map(String::as_str)
            .unwrap_or("missing");
        if status != "succeeded" {
            return Err(format!(
                "expected `{tool_id}` ({tool_call_id}) to finish with status `succeeded`; found `{status}`"
            ));
        }
    }

    Ok(())
}

fn assert_run_records_live_runtime_context(
    run_dir: &Path,
    expected_profile: &str,
    expected_model: &str,
    expected_variant: Option<&str>,
) -> Result<(), String> {
    let meta_path = run_dir.join("meta.json");
    let meta = read_required_json(&meta_path)?;
    let context = meta
        .get("recorded_runtime_context")
        .and_then(Value::as_object)
        .ok_or_else(|| {
            format!(
                "expected recorded_runtime_context in {}",
                meta_path.display()
            )
        })?;

    if context.get("profile").and_then(Value::as_str) != Some(expected_profile) {
        return Err(format!(
            "expected runtime context profile `{expected_profile}` in {}; found {:?}",
            meta_path.display(),
            context.get("profile")
        ));
    }
    if context.get("model").and_then(Value::as_str) != Some(expected_model) {
        return Err(format!(
            "expected runtime context model `{expected_model}` in {}; found {:?}",
            meta_path.display(),
            context.get("model")
        ));
    }
    if context.get("variant").and_then(Value::as_str) != expected_variant {
        return Err(format!(
            "expected runtime context variant {:?} in {}; found {:?}",
            expected_variant,
            meta_path.display(),
            context.get("variant")
        ));
    }
    if expected_variant == Some(DEFAULT_LIVE_PROXY_VARIANT)
        && context.get("reasoning_effort").and_then(Value::as_str) != Some("low")
    {
        return Err(format!(
            "expected runtime context reasoning_effort `low` in {}; found {:?}",
            meta_path.display(),
            context.get("reasoning_effort")
        ));
    }

    Ok(())
}

fn assert_todo_state_matches(run_dir: &Path) -> Result<(), String> {
    let todos_path = run_dir.join("opencode-compat").join("todos.json");
    let todos = read_required_json(&todos_path)?;
    let expected = json!([
        {
            "content": LIVE_CHAT_TODO_CONTENT,
            "status": "pending",
            "priority": "high",
        }
    ]);
    if todos == expected {
        Ok(())
    } else {
        Err(format!(
            "expected {} to equal {}; found {}",
            todos_path.display(),
            expected,
            todos
        ))
    }
}

fn assert_question_state_matches(run_dir: &Path, events_body: &str) -> Result<(), String> {
    let tool_call_id = first_requested_tool_call_id(events_body, "user.question")?
        .ok_or_else(|| "expected requested user.question tool_call_id".to_string())?;
    let question_path = run_dir
        .join("opencode-compat")
        .join("questions")
        .join(format!("{tool_call_id}.json"));
    let question_state = read_required_json(&question_path)?;
    let expected = json!([
        {
            "question": "Pick one",
            "header": "Choice",
            "multiple": Value::Null,
            "options": [
                {"label": "Yes", "description": "Choose yes"},
                {"label": "No", "description": "Choose no"}
            ]
        }
    ]);
    if question_state == expected {
        Ok(())
    } else {
        Err(format!(
            "expected {} to equal {}; found {}",
            question_path.display(),
            expected,
            question_state
        ))
    }
}

fn first_requested_tool_call_id(
    events_body: &str,
    expected_tool_id: &str,
) -> Result<Option<String>, String> {
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
        if event_type != "tool_call_requested" {
            continue;
        }
        let data = event
            .get("payload")
            .and_then(|payload| payload.get("data"))
            .cloned()
            .unwrap_or(Value::Null);
        if data.get("tool_id").and_then(Value::as_str) != Some(expected_tool_id) {
            continue;
        }
        if let Some(tool_call_id) = data.get("tool_call_id").and_then(Value::as_str) {
            return Ok(Some(tool_call_id.to_string()));
        }
    }

    Ok(None)
}

fn first_requested_tool_args(
    events_body: &str,
    expected_tool_id: &str,
) -> Result<Option<Value>, String> {
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
        if event_type != "tool_call_requested" {
            continue;
        }
        let data = event
            .get("payload")
            .and_then(|payload| payload.get("data"))
            .cloned()
            .unwrap_or(Value::Null);
        if data.get("tool_id").and_then(Value::as_str) != Some(expected_tool_id) {
            continue;
        }
        if let Some(args_summary) = data.get("args_summary").and_then(Value::as_str) {
            let args = serde_json::from_str(args_summary).map_err(|err| {
                format!(
                    "failed to parse args_summary for `{expected_tool_id}` on line {}: {err}",
                    idx + 1
                )
            })?;
            return Ok(Some(args));
        }
    }

    Ok(None)
}

fn first_tool_call_output_summary(
    events_body: &str,
    expected_tool_id: &str,
) -> Result<Option<String>, String> {
    let tool_call_id = first_requested_tool_call_id(events_body, expected_tool_id)?;
    let Some(tool_call_id) = tool_call_id else {
        return Ok(None);
    };

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
        if event_type != "tool_call_finished" {
            continue;
        }
        let data = event
            .get("payload")
            .and_then(|payload| payload.get("data"))
            .cloned()
            .unwrap_or(Value::Null);
        if data.get("tool_call_id").and_then(Value::as_str) != Some(tool_call_id.as_str()) {
            continue;
        }
        return Ok(data
            .get("output_summary")
            .and_then(Value::as_str)
            .map(ToOwned::to_owned));
    }

    Ok(None)
}

fn resolve_env_reference_value(value: &str) -> Result<String, String> {
    if !(value.starts_with("${") && value.ends_with('}')) {
        return Ok(value.to_string());
    }

    let reference = &value[2..value.len() - 1];
    if reference.is_empty() {
        return Ok(value.to_string());
    }

    if let Some((key, fallback)) = reference.split_once(":-") {
        if key.is_empty() {
            return Ok(value.to_string());
        }
        return Ok(env::var(key)
            .ok()
            .filter(|resolved| !resolved.is_empty())
            .unwrap_or_else(|| fallback.to_string()));
    }

    env::var(reference).map_err(|_| {
        format!("environment variable `{reference}` required by live proxy api_key is not set")
    })
}

fn resolve_trimmed_env_var(name: &str) -> Option<Result<String, String>> {
    env::var(name)
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .map(Ok)
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
