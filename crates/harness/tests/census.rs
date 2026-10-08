mod common;

use common::{CliHarness, CliHarnessOutput};
use harness_providers::{mock::MockProvider, Provider, ProviderStreamEvent as Stream};
use serde_json::{json, Value};
use std::{fs, path::Path, sync::Arc};

fn cli(root: &Path) -> CliHarness {
    CliHarness::new()
        .current_dir(root)
        .args(["--config", "config.json"])
        .env_remove("HOME")
        .env_remove("XDG_CONFIG_HOME")
        .env_remove("HARNESS_CONFIG")
        .env_remove("HARNESS_TUI_CONFIG")
        .env_remove("HARNESS_CONFIG_CONTENT")
        .env("HARNESS_DATA_HOME", root.join("data"))
}

fn call(id: &str, tool: &str, args: Value) -> Vec<Stream> {
    vec![
        Stream::ToolCallComplete {
            tool_call_id: id.into(),
            function_name: tool.into(),
            arguments_json: args.to_string(),
        },
        Stream::Done { usage: None },
    ]
}

fn answer() -> Vec<Stream> {
    vec![
        Stream::TextDelta("private-answer-sentinel".into()),
        Stream::Done { usage: None },
    ]
}

fn census(
    root: &Path,
    args: &[&str],
) -> Result<(Value, CliHarnessOutput), Box<dyn std::error::Error>> {
    let output = cli(root)
        .args(["sessions", "census", "--json"])
        .args(args.iter().copied())
        .output();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    Ok((serde_json::from_slice(&output.stdout)?, output))
}

#[test]
fn census_measures_scripted_turns_filters_models_and_never_changes_saved_sessions(
) -> Result<(), Box<dyn std::error::Error>> {
    let root = tempfile::tempdir()?;
    fs::write(root.path().join("config.json"), json!({
        "provider": {"local": {"type": "openai_compatible", "models": {"base": {}, "checked": {}}}},
        "model": "local/base",
        "runtime": {"session_dir": "sessions", "behavior": {
            "todo_continuation": {"enabled": true, "max_reminders": 1},
            "loop_guard": {"enabled": false},
            "directory_instructions": {"enabled": false},
            "command_notifications": {"enabled": false}
        }},
        "agent": {"census": {"tools": ["read", "write", "todowrite", "bash"], "system_prompt": "Run the scripted tools."}},
        "permission": {"*": "allow", "doom_loop": "allow"}
    }).to_string())?;
    let empty = census(root.path(), &[])?.0;
    assert_eq!(empty["session_count"], 0);
    assert!(!root.path().join("sessions").exists());
    fs::write(root.path().join("input.txt"), "private-output-sentinel")?;
    let provider = Arc::new(MockProvider::script([
        call("read-1", "read", json!({"path": "input.txt"})),
        call("read-2", "read", json!({"path": "input.txt"})),
        call("read-3", "read", json!({"path": "input.txt"})),
        call(
            "write-1",
            "write",
            json!({"path": "edited.txt", "content": "private-argument-sentinel"}),
        ),
        call(
            "todos",
            "todowrite",
            json!({"todos": [{"content": "private-todo-sentinel", "status": "pending", "priority": "high"}]}),
        ),
        answer(),
        answer(),
    ]));
    let prompt = cli(root.path())
        .args([
            "prompt",
            "--text",
            "private-prompt-sentinel",
            "--session-id",
            "unchecked",
            "--profile",
            "census",
            "--max-turns",
            "10",
        ])
        .provider_override(Arc::clone(&provider) as Arc<dyn Provider>)
        .output();
    assert!(
        prompt.status.success(),
        "{}",
        String::from_utf8_lossy(&prompt.stderr)
    );
    assert_eq!(provider.call_count(), 7);
    let checked = Arc::new(MockProvider::script([
        call(
            "write-2",
            "write",
            json!({"path": "checked.txt", "content": "checked"}),
        ),
        call("check", "bash", json!({"command": "true"})),
        answer(),
    ]));
    let prompt = cli(root.path())
        .args([
            "prompt",
            "--text",
            "check the edit",
            "--session-id",
            "checked",
            "--model",
            "local/checked",
            "--profile",
            "census",
            "--max-turns",
            "10",
        ])
        .provider_override(Arc::clone(&checked) as Arc<dyn Provider>)
        .output();
    assert!(
        prompt.status.success(),
        "{}",
        String::from_utf8_lossy(&prompt.stderr)
    );
    assert_eq!(checked.call_count(), 3);
    let before: Vec<_> = ["unchecked", "checked"]
        .into_iter()
        .map(|id| {
            let directory = root.path().join("sessions").join(id);
            Ok::<_, std::io::Error>((
                fs::read(directory.join("events.jsonl"))?,
                fs::read(directory.join("meta.json"))?,
            ))
        })
        .collect::<Result<_, _>>()?;
    // Even a corrupt index must not be rebuilt or used as behavioral evidence.
    let index = root.path().join("sessions/session-index.json");
    fs::write(&index, "invalid index")?;
    let (report, output) = census(root.path(), &[])?;
    assert_session_metrics(&report)?;
    assert_model_and_total_metrics(&report);
    assert_filters(root.path())?;
    assert_private_output(root.path(), &output);
    for (id, (events, metadata)) in ["unchecked", "checked"].into_iter().zip(before) {
        let directory = root.path().join("sessions").join(id);
        assert_eq!(fs::read(directory.join("events.jsonl"))?, events);
        assert_eq!(fs::read(directory.join("meta.json"))?, metadata);
    }
    assert_eq!(fs::read_to_string(index)?, "invalid index");
    assert_eq!(fs::read_dir(root.path().join("sessions"))?.count(), 3);
    let broken = root.path().join("sessions/broken");
    fs::create_dir(&broken)?;
    fs::write(
        broken.join("events.jsonl"),
        "private-corrupt-journal-sentinel\n",
    )?;
    let (report, output) = census(root.path(), &[])?;
    assert_eq!(report["session_count"], 2);
    assert_eq!(report["unavailable_session_ids"], json!(["broken"]));
    assert!(!String::from_utf8_lossy(&output.stdout).contains("private-corrupt-journal-sentinel"));
    assert_eq!(
        fs::read_to_string(broken.join("events.jsonl"))?,
        "private-corrupt-journal-sentinel\n"
    );
    Ok(())
}

fn assert_session_metrics(report: &Value) -> Result<(), Box<dyn std::error::Error>> {
    assert_eq!(report["schema_version"], "harness-sessions-census-v1");
    assert_eq!(report["session_count"], 2);
    let sessions = report["sessions"].as_array().ok_or("sessions missing")?;
    let unchecked = &sessions
        .iter()
        .find(|s| s["run_id"] == "unchecked")
        .ok_or("unchecked missing")?["metrics"];
    assert_eq!(unchecked["turns"], 1);
    assert_eq!(unchecked["provider_requests"], 7);
    assert_eq!(unchecked["tool_calls"], 5);
    assert_eq!(
        unchecked["tool_calls_by_tool"],
        json!({"read": 3, "write": 1, "todowrite": 1})
    );
    assert_eq!(unchecked["longest_identical_tool_run"], 3);
    assert_eq!(unchecked["identical_tool_runs_ge_3"], 1);
    assert_eq!(unchecked["open_todo_turns"], 1);
    assert_eq!(unchecked["unverified_edit_turns"], 1);
    assert_eq!(
        unchecked["runtime_reminders_by_kind"]["todo_continuation"],
        1
    );
    Ok(())
}

fn assert_model_and_total_metrics(report: &Value) {
    assert_eq!(report["models"]["base"]["provider_requests"], 7);
    assert_eq!(report["models"]["checked"]["unverified_edit_turns"], 0);
    assert_eq!(report["totals"]["turns"], 2);
    assert_eq!(report["totals"]["unverified_edit_turns"], 1);
    assert_eq!(report["totals"]["tool_calls"], 7);
    assert_eq!(report["totals"]["eval_share"], 0.0);
}

fn assert_filters(root: &Path) -> Result<(), Box<dyn std::error::Error>> {
    assert_eq!(census(root, &["--model", "BASE"])?.0["session_count"], 1);
    assert_eq!(census(root, &["--model", "absent"])?.0["session_count"], 0);
    assert_eq!(census(root, &["--limit", "1"])?.0["session_count"], 1);
    for (since, count) in [("2000-01-01", 2), ("9999-01-01T00:00:00Z", 0)] {
        assert_eq!(census(root, &["--since", since])?.0["session_count"], count);
    }
    Ok(())
}

fn assert_private_output(root: &Path, output: &CliHarnessOutput) {
    let text = cli(root).args(["sessions", "census"]).output();
    assert!(text.status.success());
    for bytes in [&output.stdout, &text.stdout] {
        let text = String::from_utf8_lossy(bytes);
        for sentinel in [
            "private-prompt-sentinel",
            "private-argument-sentinel",
            "private-output-sentinel",
            "private-answer-sentinel",
            "private-todo-sentinel",
        ] {
            assert!(!text.contains(sentinel), "census disclosed {sentinel}");
        }
    }
}
