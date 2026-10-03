use harness::{run, CliDeps, CliIo};
use std::io::Cursor;

#[cfg(unix)]
#[test]
fn agent_stdio_reports_a_local_exchange_and_failure_without_disclosing_command_credentials(
) -> Result<(), Box<dyn std::error::Error>> {
    let root = tempfile::tempdir()?;
    std::fs::write(root.path().join("marker"), "present")?;
    for (command, succeeds) in [
        (
            "API_KEY=opaque-peer-credential; test -f marker && cat",
            true,
        ),
        ("exit 1", false),
        ("  ", false),
    ] {
        let (mut input, mut output, mut errors) = (Cursor::new(Vec::new()), Vec::new(), Vec::new());
        let result = run(
            ["harness", "agent", "stdio", "--command", command, "--json"],
            &mut CliIo::new(&mut input, &mut output, &mut errors),
            CliDeps::real()
                .with_current_dir(root.path().into())
                .without_env("HOME")
                .without_env("XDG_CONFIG_HOME")
                .without_env("HARNESS_CONFIG")
                .without_env("HARNESS_TUI_CONFIG")
                .without_env("HARNESS_CONFIG_CONTENT"),
        );
        assert_eq!(
            result.code == 0,
            succeeds,
            "{}",
            String::from_utf8_lossy(&errors)
        );
        assert!(!String::from_utf8_lossy(&output).contains("opaque-peer-credential"));
        assert!(!String::from_utf8_lossy(&errors).contains("opaque-peer-credential"));
        if !command.trim().is_empty() {
            let report: serde_json::Value = serde_json::from_slice(&output)?;
            assert_eq!(report["meets_agent_mode_contract"], succeeds);
            assert_eq!(report["operate_ok"], succeeds);
        }
    }
    assert_eq!(std::fs::read_dir(root.path())?.count(), 1);
    Ok(())
}
