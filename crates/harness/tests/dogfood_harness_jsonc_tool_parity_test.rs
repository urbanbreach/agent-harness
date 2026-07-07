use harness::UnwrapOrAbort;
use std::ffi::OsString;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use tempfile::{tempdir, TempDir};

#[path = "common/mod.rs"]
mod common;
#[path = "support/dogfood_harness_jsonc_tool_parity_support.rs"]
mod dogfood_support;

use common::{repo_root, CliHarness, CliHarnessOutput};
use dogfood_support::{DogfoodPromptProvider, RECOVERY_INSTRUCTION, SERDE_DETAIL};

struct PromptRunFixture {
    workspace: TempDir,
    config_path: PathBuf,
    session_dir: PathBuf,
    out_path: PathBuf,
}

impl PromptRunFixture {
    fn new() -> Self {
        let workspace = tempdir().unwrap_or_abort();
        let config_path = workspace.path().join("harness.jsonc");
        fs::copy(repo_root().join("harness.jsonc"), &config_path).unwrap_or_abort();
        let session_dir = workspace.path().join("sessions");
        let out_path = workspace.path().join("events.jsonl");
        Self {
            workspace,
            config_path,
            session_dir,
            out_path,
        }
    }

    fn run_prompt(
        &self,
        provider: std::sync::Arc<dyn harness_providers::Provider>,
        prompt_text: &str,
    ) -> CliHarnessOutput {
        CliHarness::new()
            .current_dir(self.workspace.path())
            .provider_override(provider)
            .env_remove("HARNESS_CONFIG")
            .env_remove("HARNESS_CONFIG_CONTENT")
            .env_remove("HARNESS_TUI_CONFIG")
            .env("HOME", "/nonexistent")
            .env("XDG_CONFIG_HOME", "/nonexistent")
            .env("UMANS_AI_CODING_PLAN_API_KEY", "test-dogfood-key")
            .args([
                OsString::from("--config"),
                self.config_path.as_os_str().to_owned(),
                OsString::from("--session-dir"),
                self.session_dir.as_os_str().to_owned(),
                OsString::from("prompt"),
                OsString::from("--text"),
                OsString::from(prompt_text),
                OsString::from("--out"),
                self.out_path.as_os_str().to_owned(),
            ])
            .output()
    }

    fn write_fixture_file(&self, relative_path: &str, body: &str) -> PathBuf {
        let path = self.workspace.path().join(relative_path);
        let parent = path.parent().unwrap_or_abort();
        fs::create_dir_all(parent).unwrap_or_abort();
        fs::write(&path, body).unwrap_or_abort();
        path
    }

    fn events_body(&self) -> String {
        fs::read_to_string(&self.out_path).unwrap_or_abort()
    }
}

#[allow(
    clippy::clone_on_ref_ptr,
    reason = "trait object coercion requires .clone() not Arc::clone"
)]
#[test]
fn vague_prompt_uses_model_visible_tool_definitions_to_select_glob() {
    // arrange: a workspace with a markdown file and a provider scripted to choose tools.
    let fixture = PromptRunFixture::new();
    fixture.write_fixture_file("fixtures/decision.md", "dogfood selection fixture\n");
    let provider = DogfoodPromptProvider::vague_selection();

    // act: run the mocked harness prompt path using the real harness.jsonc.
    let output = fixture.run_prompt(
        provider.clone(),
        "Find the markdown note in fixtures and report the match.",
    );

    // assert: provider exposed tool definitions and the run selected glob successfully.
    assert_prompt_success(&output);
    let requests = provider.requests();
    let first_request = requests
        .iter()
        .find(|request| request.tool("glob").is_some())
        .unwrap_or_else(|| panic!("provider request did not expose glob; requests: {requests:?}"));
    let glob_tool = first_request.tool("glob").unwrap_or_abort();
    assert_eq!(glob_tool.parameters["type"], "object");
    assert!(first_request.tool("grep").is_some());
    assert!(first_request.tool("read").is_some());

    let events_body = fixture.events_body();
    assert!(events_body.contains("\"event_type\":\"tool_call_requested\""));
    assert!(events_body.contains("\"event_type\":\"tool_call_started\""));
    assert!(events_body.contains("\"event_type\":\"tool_call_finished\""));
    assert!(events_body.contains("\"tool_id\":\"glob\""));
    assert!(events_body.contains("\"status\":\"succeeded\""));
    assert!(events_body.contains("decision.md"));
}

#[allow(
    clippy::clone_on_ref_ptr,
    reason = "trait object coercion requires .clone() not Arc::clone"
)]
#[test]
fn malformed_tool_args_return_recovery_text_then_corrected_read_succeeds() {
    // arrange: a workspace text file and a provider scripted to emit bad args then recover.
    let fixture = PromptRunFixture::new();
    let target = fixture.write_fixture_file("fixtures/recovery.txt", "alpha\nbeta\ngamma\n");
    let provider = DogfoodPromptProvider::bad_argument_recovery(path_string(&target));

    // act: run the mocked harness prompt path using the real harness.jsonc.
    let output = fixture.run_prompt(
        provider.clone(),
        "Read the recovery fixture; if the first call fails, fix the arguments.",
    );

    // assert: the runtime returned recovery text and the corrected read succeeded.
    assert_prompt_success(&output);
    let requests = provider.requests();
    assert!(
        requests.len() >= 3,
        "expected malformed call, corrected call, and final answer requests; got {}",
        requests.len()
    );
    let recovery_request_text = requests
        .iter()
        .map(|request| request.messages_text())
        .find(|messages| messages.contains(RECOVERY_INSTRUCTION))
        .unwrap_or_abort();
    assert!(recovery_request_text.contains(SERDE_DETAIL));

    let events_body = fixture.events_body();
    assert!(events_body.contains(RECOVERY_INSTRUCTION));
    assert!(events_body.contains(SERDE_DETAIL));
    assert!(events_body.contains("\"tool_id\":\"read\""));
    assert!(events_body.contains("\"status\":\"failed\""));
    assert!(events_body.contains("\"status\":\"succeeded\""));
    assert!(events_body.contains("recovery.txt"));
    assert!(events_body.contains("alpha"));
}

fn assert_prompt_success(output: &CliHarnessOutput) {
    assert!(
        output.status.success(),
        "stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

fn path_string(path: &Path) -> String {
    path.to_str().unwrap_or_abort().to_string()
}
