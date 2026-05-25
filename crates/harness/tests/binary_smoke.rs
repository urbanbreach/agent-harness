use std::process::Command;

#[test]
#[ignore = "T5 binary smoke; set HARNESS_BINARY_SMOKE=1 and run explicitly"]
fn harness_binary_prints_help_from_real_process() {
    assert_eq!(
        std::env::var("HARNESS_BINARY_SMOKE").as_deref(),
        Ok("1"),
        "set HARNESS_BINARY_SMOKE=1 to run the T5 binary smoke"
    );

    let output = Command::new(env!("CARGO_BIN_EXE_harness"))
        .arg("--help")
        .output()
        .expect("run harness --help through real binary");

    assert!(
        output.status.success(),
        "stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("Usage:"), "stdout:\n{stdout}");
    assert!(stdout.contains("config"), "stdout:\n{stdout}");
}
