use harness::{run, CliDeps, CliIo};
use harness_core::worktree::{create_session_worktree, CreateWorktreeOptions};
use serde_json::Value;
use std::{fs, io::Cursor, path::Path, process::Command};

#[test]
fn process_entry_loads_dotenv_and_routes_debug_logs_without_polluting_command_output(
) -> Result<(), Box<dyn std::error::Error>> {
    if std::env::var("HARNESS_BINARY_SIGNOFF").as_deref() != Ok("1") {
        return Err("set HARNESS_BINARY_SIGNOFF=1 for real process checks".into());
    }
    let root = tempfile::tempdir()?;
    fs::write(
        root.path().join(".env"),
        "HARNESS_DOTENV_PROBE='from dotenv'\n",
    )?;
    fs::write(root.path().join("config.json"), serde_json::json!({
        "provider":{"local":{"type":"openai_compatible","baseURL":"http://127.0.0.1/v1",
            "apiKeyEnv":[],"models":{"fixture":{"limit":{"context":128000,"output":1000}}}}},
        "model":"local/fixture", "agent":{"default":{"system_prompt":"{env:HARNESS_DOTENV_PROBE}"}}
    }).to_string())?;
    for override_value in [None, Some("from process")] {
        let mut command = Command::new(env!("CARGO_BIN_EXE_harness"));
        command
            .current_dir(root.path())
            .env_clear()
            .env("HOME", root.path())
            .env("XDG_CONFIG_HOME", root.path())
            .env("HARNESS_DATA_HOME", root.path().join("data"))
            .arg("--debug")
            .args(["--config", "config.json", "config", "show", "--effective"]);
        if let Some(value) = override_value {
            command.env("HARNESS_DOTENV_PROBE", value);
            command.args(["--debug-file", "debug.log"]);
        }
        let output = command.output()?;
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let value: Value = serde_json::from_slice(&output.stdout)?;
        assert_eq!(
            value["effective"]["agents"]["default"]["system_prompt"],
            override_value.unwrap_or("from dotenv")
        );
        let log = if override_value.is_some() {
            assert!(output.stderr.is_empty());
            fs::read_to_string(root.path().join("debug.log"))?
        } else {
            String::from_utf8(output.stderr)?
        };
        assert!(log.contains("debug logging enabled"));
    }
    Ok(())
}

#[test]
fn worktree_cli_scopes_cleanup_and_keeps_dirty_work_until_forced(
) -> Result<(), Box<dyn std::error::Error>> {
    if std::env::var("HARNESS_BINARY_SIGNOFF").as_deref() != Ok("1") {
        return Err("set HARNESS_BINARY_SIGNOFF=1 for real process checks".into());
    }
    let temp = tempfile::tempdir()?;
    let root = temp.path().join("repo");
    fs::create_dir(&root)?;
    let git = |args: &[&str]| -> Result<(), Box<dyn std::error::Error>> {
        let output = Command::new("git")
            .current_dir(&root)
            .args([
                "-c",
                "user.name=Harness Fixture",
                "-c",
                "user.email=fixture@example.test",
                "-c",
                "commit.gpgsign=false",
                "-c",
                "core.hooksPath=/dev/null",
                "-c",
                "core.fsmonitor=false",
            ])
            .args(args)
            .output()?;
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        Ok(())
    };
    git(&["init", "--quiet"])?;
    fs::write(root.join("source.txt"), "initial\n")?;
    git(&["add", "source.txt"])?;
    git(&["commit", "--quiet", "-m", "fixture"])?;
    let data_dir = temp.path().join("data/harness");
    let create = |slug| {
        create_session_worktree(CreateWorktreeOptions {
            repository_root: &root,
            data_dir: &data_dir,
            worktree_parent: None,
            slug: Some(slug),
            start_point: None,
        })
    };
    let first = create("first")?;
    let second = create("second")?;
    assert_eq!(invoke(temp.path(), &["list"], 0)?["managed_count"], 2);
    let removed = invoke(temp.path(), &["remove", "first", "--keep-branch"], 0)?;
    assert_eq!(removed["removed"], true);
    assert!(!first.path.exists());
    git(&[
        "show-ref",
        "--verify",
        "--quiet",
        "refs/heads/harness/wt-first",
    ])?;
    fs::write(second.path.join("source.txt"), "uncommitted work\n")?;
    let blocked = invoke(temp.path(), &["cleanup"], 1)?;
    assert_eq!(blocked["failed_count"], 1);
    assert_eq!(
        fs::read_to_string(second.path.join("source.txt"))?,
        "uncommitted work\n"
    );
    let cleaned = invoke(temp.path(), &["cleanup", "--force"], 0)?;
    assert_eq!(cleaned["removed_count"], 1);
    assert!(!second.path.exists());
    assert_eq!(fs::read_to_string(root.join("source.txt"))?, "initial\n");
    assert_eq!(
        invoke(temp.path(), &["list", "--all"], 0)?["worktrees"]
            .as_array()
            .map(Vec::len),
        Some(1)
    );
    Ok(())
}

fn invoke(root: &Path, args: &[&str], expected: i32) -> Result<Value, Box<dyn std::error::Error>> {
    let (mut input, mut output, mut errors) = (Cursor::new(Vec::new()), Vec::new(), Vec::new());
    let result = run(
        ["harness", "worktree", "--workspace", "repo"]
            .into_iter()
            .chain(args.iter().copied()),
        &mut CliIo::new(&mut input, &mut output, &mut errors),
        CliDeps::real()
            .with_current_dir(root.into())
            .with_env("HARNESS_DATA_HOME", root.join("data").to_string_lossy()),
    );
    assert_eq!(
        result.code,
        expected,
        "{}",
        String::from_utf8_lossy(&errors)
    );
    Ok(serde_json::from_slice(&output)?)
}
