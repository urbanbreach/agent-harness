use harness::{CliDeps, CliIo, ExitOutcome, UnwrapOrAbort};
use std::io::Cursor;

fn run_cli(args: &[&str]) -> (i32, String, String) {
    let args: Vec<&str> = std::iter::once("harness")
        .chain(args.iter().copied())
        .collect();
    let mut stdin = Cursor::new(Vec::new());
    let mut stdout = Vec::new();
    let mut stderr = Vec::new();
    let mut io = CliIo::new(&mut stdin, &mut stdout, &mut stderr);
    let ExitOutcome { code, .. } = harness::run(args, &mut io, CliDeps::real());
    (
        code,
        String::from_utf8_lossy(&stdout).to_string(),
        String::from_utf8_lossy(&stderr).to_string(),
    )
}

#[test]
fn agent_stdio_with_cat_meets_agent_mode_contract_json() {
    let (code, stdout, stderr) = run_cli(&["agent", "stdio", "--command", "cat", "--json"]);
    assert_eq!(code, 0, "stderr: {stderr}");
    let json: serde_json::Value = serde_json::from_str(&stdout).unwrap_or_abort();
    assert_eq!(json["connected"], true, "json: {json}");
    assert_eq!(json["bound"], true, "json: {json}");
    assert_eq!(json["operate_ok"], true, "json: {json}");
    assert_eq!(json["meets_agent_mode_contract"], true, "json: {json}");
    assert!(
        json["session_id"].as_str().is_some_and(|s| !s.is_empty()),
        "json: {json}"
    );
    assert!(
        json["agent_name"].as_str().is_some_and(|s| !s.is_empty()),
        "json: {json}"
    );
}

#[test]
fn agent_stdio_with_cat_text_mode_reports_ok() {
    let (code, stdout, stderr) = run_cli(&["agent", "stdio", "--command", "cat"]);
    assert_eq!(code, 0, "stderr: {stderr}");
    assert!(stdout.contains("agent stdio:"), "stdout: {stdout}");
    assert!(stdout.contains("status=ok"), "stdout: {stdout}");
}

#[test]
fn agent_stdio_with_failing_command_exits_nonzero() {
    let (code, _stdout, _stderr) = run_cli(&["agent", "stdio", "--command", "exit 1"]);
    assert_ne!(code, 0);
}

#[test]
fn agent_stdio_with_empty_command_rejects_with_usage() {
    let (code, stdout, stderr) = run_cli(&["agent", "stdio", "--command", ""]);
    assert_eq!(code, 2);
    assert!(stdout.is_empty());
    assert!(stderr.contains("must not be empty"), "stderr: {stderr}");
}

#[test]
fn agent_select_stub_directs_to_flag() {
    let (code, stdout, stderr) = run_cli(&["agent", "select", "--agent", "build"]);
    assert_eq!(code, 2);
    assert!(stdout.is_empty());
    assert!(stderr.contains("not available"), "stderr: {stderr}");
    assert!(stderr.contains("--agent"), "stderr: {stderr}");
}
