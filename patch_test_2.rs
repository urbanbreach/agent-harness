use std::fs;

fn main() {
    let path = "crates/harness-tools/src/shell_safety.rs";
    let content = fs::read_to_string(path).unwrap();
    let new_content = content.replace(
        "fn validate_bash_command_rejects_external_path_arguments() {",
        r#"fn validate_bash_command_rejects_external_path_arguments() {
        let tempdir = tempfile::tempdir().expect("tempdir");
        let safety = ShellSafety::new(ShellAllowlist {
            executables: vec!["ls".to_string()],
            cwd_roots: vec![".".to_string()],
            ..ShellAllowlist::default()
        });

        let err_short = safety
            .validate_bash_command("ls -I../../../etc/passwd", tempdir.path(), tempdir.path())
            .expect_err("external relative path in short option should be blocked");
        assert!(matches!(err_short, ToolError::PathEscapesWorkspace { .. }));"#
    );
    fs::write(path, new_content).unwrap();
}
