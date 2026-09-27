use harness::{run, CliDeps, CliIo};
use serde_json::Value;
use std::{fs, io::Cursor};

#[test]
fn memory_commands_keep_reads_empty_and_scope_redacted_writes_to_the_requested_workspace(
) -> Result<(), Box<dyn std::error::Error>> {
    let root = tempfile::tempdir()?;
    let selected = root.path().join("selected");
    fs::create_dir(&selected)?;
    let deps = CliDeps::real().with_current_dir(root.path().into());
    for action in [
        vec!["list"],
        vec!["put", "preference", "Use tabs. api_key=private-value"],
        vec!["get", "preference"],
        vec!["search", "TABS"],
    ] {
        let (mut input, mut stdout, mut stderr) = (Cursor::new(Vec::new()), Vec::new(), Vec::new());
        let result = run(
            ["harness", "memory", "--workspace", "selected"]
                .into_iter()
                .chain(action.clone()),
            &mut CliIo::new(&mut input, &mut stdout, &mut stderr),
            deps.clone(),
        );
        assert_eq!(result.code, 0, "{}", String::from_utf8_lossy(&stderr));
        let text = String::from_utf8(stdout)?;
        assert!(!text.contains("private-value"));
        let json: Value = serde_json::from_str(&text)?;
        if action[0] == "list" {
            assert_eq!(json["entries"].as_array().map(Vec::len), Some(0));
            assert!(!selected.join(".agent-harness").exists());
        } else {
            assert!(text.contains("Use tabs."));
            assert!(text.contains("[REDACTED]"));
        }
        assert!(!root.path().join(".agent-harness").exists());
    }
    let stored = selected.join(".agent-harness/memory/entries.json");
    assert!(!fs::read_to_string(stored)?.contains("private-value"));
    Ok(())
}

#[test]
fn graph_commands_report_missing_indexes_and_query_the_selected_workspace(
) -> Result<(), Box<dyn std::error::Error>> {
    let root = tempfile::tempdir()?;
    fs::write(
        root.path().join("sample.rs"),
        "pub fn entry() {\n    helper();\n}\npub fn helper() {}\n",
    )?;
    let index = root.path().join(harness_core::code_graph::GRAPH_INDEX_REL);
    for (args, succeeds) in [
        (vec!["query", "helper"], false),
        (vec!["build"], true),
        (vec!["query", "helper", "--kind", "callers"], true),
    ] {
        let (mut input, mut stdout, mut stderr) = (Cursor::new(Vec::new()), Vec::new(), Vec::new());
        let result = run(
            ["harness", "code-graph"].into_iter().chain(args.clone()),
            &mut CliIo::new(&mut input, &mut stdout, &mut stderr),
            CliDeps::real().with_current_dir(root.path().into()),
        );
        assert_eq!(
            result.code == 0,
            succeeds,
            "{}",
            String::from_utf8_lossy(&stderr)
        );
        let report: Value = serde_json::from_slice(&stdout)?;
        if !succeeds {
            assert_eq!(report["result"]["outcome"], "unavailable");
            assert!(!index.exists());
        } else if args[0] == "query" {
            assert_eq!(report["result"]["hits"][0]["symbol"], "entry");
            assert_eq!(report["result"]["hits"][0]["path"], "sample.rs");
        } else {
            assert_eq!(report["symbol_count"], 2);
        }
    }
    Ok(())
}

fn json_command(
    root: &std::path::Path,
    args: &[&str],
) -> Result<Value, Box<dyn std::error::Error>> {
    let (mut input, mut output, mut errors) = (Cursor::new(Vec::new()), Vec::new(), Vec::new());
    let result = run(
        std::iter::once("harness").chain(args.iter().copied()),
        &mut CliIo::new(&mut input, &mut output, &mut errors),
        CliDeps::real().with_current_dir(root.into()),
    );
    assert_eq!(result.code, 0, "{}", String::from_utf8_lossy(&errors));
    Ok(serde_json::from_slice(&output)?)
}

#[test]
fn plugin_commands_persist_lifecycle_without_executing_package_entries(
) -> Result<(), Box<dyn std::error::Error>> {
    let root = tempfile::tempdir()?;
    let workspace = root.path().join("selected");
    fs::create_dir(&workspace)?;
    let command = |args: &[&str]| {
        json_command(
            root.path(),
            &[&["plugin", "--workspace", "selected"][..], args].concat(),
        )
    };
    assert_eq!(command(&["list"])?["count"], 0);
    assert!(!workspace.join(".agent-harness").exists());
    let manifest = |version: &str| serde_json::json!({"schemaVersion":"extension.manifest.v1","id":"review.plugin","version":version});
    let package = workspace.join("package");
    fs::create_dir(&package)?;
    fs::write(
        package.join("extension.manifest.json"),
        manifest("1").to_string(),
    )?;
    fs::write(
        package.join("hooks.json"),
        serde_json::json!({"command":format!("touch {}",workspace.join("executed").display())})
            .to_string(),
    )?;
    let discovered = command(&["discover"])?;
    assert_eq!(discovered["discovered"], 1);
    assert_eq!(discovered["loads_external_code"], false);
    assert_eq!(command(&["list"])?["count"], 0);
    assert_eq!(
        command(&["install", "package"])?["plugin"]["enablement"],
        "disabled"
    );
    assert_eq!(
        command(&["activate", "review.plugin"])?["plugin"]["enablement"],
        "enabled"
    );
    assert_eq!(command(&["list"])?["plugins"][0]["enablement"], "enabled");
    let replacement = workspace.join("replacement");
    fs::create_dir(&replacement)?;
    fs::write(
        replacement.join("extension.manifest.json"),
        manifest("2").to_string(),
    )?;
    let upgraded = command(&["upgrade", "review.plugin", "replacement"])?;
    assert_eq!(upgraded["previous_version"], "1");
    assert_eq!(upgraded["version"], "2");
    assert_eq!(upgraded["plugin"]["enablement"], "enabled");
    assert_eq!(
        command(&["deactivate", "review.plugin"])?["plugin"]["enablement"],
        "disabled"
    );
    assert_eq!(
        command(&["remove", "review.plugin"])?["removed"]["id"],
        "review.plugin"
    );
    assert_eq!(command(&["list"])?["count"], 0);
    assert!(!workspace.join("executed").exists());
    assert!(package.join("hooks.json").exists());
    assert!(replacement.join("extension.manifest.json").exists());
    assert!(!root.path().join(".agent-harness").exists());
    Ok(())
}

#[cfg(unix)]
#[test]
fn update_cli_checks_downloads_replaces_and_restarts_without_repeating_update_arguments(
) -> Result<(), Box<dyn std::error::Error>> {
    use sha2::{Digest, Sha256};
    use std::os::unix::fs::PermissionsExt;
    let root = tempfile::tempdir()?;
    let source = root.path().join("next binary");
    let script = b"#!/bin/sh\nprintf 'launched:%s:%s\\n' \"$#\" \"$PWD\"\n";
    fs::write(&source, script)?;
    fs::write(root.path().join("installed"), "old executable")?;
    fs::set_permissions(
        root.path().join("installed"),
        fs::Permissions::from_mode(0o755),
    )?;
    let url = reqwest::Url::from_file_path(&source).map_err(|()| "file URL")?;
    let digest = format!("{:x}", Sha256::digest(script));
    harness_core::binary_update::write_local_update_manifest(
        root.path(),
        &harness_core::binary_update::LocalUpdateManifest {
            version: "99.0.0".into(),
            channel: None,
            min_version: None,
            download_url: Some(url.to_string()),
            sha256: Some(digest.clone()),
        },
    )?;
    let check = json_command(root.path(), &["update", "check"])?;
    assert_eq!(check["check"]["status"], "update_available");
    assert!(root
        .path()
        .join(harness_core::binary_update::UPDATE_CHECK_RECEIPT_REL)
        .is_file());
    let downloaded = json_command(
        root.path(),
        &[
            "update",
            "download",
            "--url",
            url.as_str(),
            "--expected-sha256",
            &digest,
            "--dest-dir",
            "downloads",
        ],
    )?;
    assert_eq!(downloaded["download"]["sha256_verified"], true);
    let artifact = downloaded["download"]["artifact_path"]
        .as_str()
        .ok_or("download path")?;
    let applied = json_command(
        root.path(),
        &[
            "update",
            "apply",
            "--artifact-path",
            artifact,
            "--target",
            "installed",
        ],
    )?;
    assert_eq!(applied["apply"]["status"], "applied");
    assert_eq!(
        fs::read(root.path().join("installed.backup"))?,
        b"old executable"
    );
    assert_eq!(fs::read(root.path().join("installed"))?, script);
    for action in ["restart", "run"] {
        if action == "run" {
            fs::remove_file(root.path().join("installed.backup"))?;
        }
        let output = std::process::Command::new(env!("CARGO_BIN_EXE_harness"))
            .args([
                "--cwd",
                root.path().to_str().ok_or("workspace")?,
                "update",
                action,
                "--target",
                "installed",
            ])
            .output()?;
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(String::from_utf8(output.stdout)?
            .ends_with(&format!("launched:0:{}\n", root.path().display())));
    }
    Ok(())
}

#[test]
fn cron_cli_evaluates_explicit_time_and_reuses_its_workspace_journal(
) -> Result<(), Box<dyn std::error::Error>> {
    let root = tempfile::tempdir()?;
    let journal = root.path().join("receipts/cron-fires.jsonl");
    for (minute, expected) in [("29", 0), ("30", 1), ("30", 0)] {
        let report = json_command(
            root.path(),
            &[
                "cron",
                "fire-due",
                "--minute",
                minute,
                "--hour",
                "14",
                "--journal-dir",
                "receipts",
                "check:30 14 * * *",
            ],
        )?;
        assert_eq!(report["fired"].as_array().map(Vec::len), Some(expected));
        if minute == "29" {
            assert!(!journal.exists());
        }
    }
    assert_eq!(fs::read_to_string(journal)?.lines().count(), 1);
    assert!(!root.path().join(".agent-harness").exists());
    Ok(())
}

#[test]
fn team_cli_keeps_targeted_messages_durable_until_the_recipient_drains_them(
) -> Result<(), Box<dyn std::error::Error>> {
    let root = tempfile::tempdir()?;
    assert_eq!(json_command(root.path(), &["team", "list"])?["count"], 0);
    assert_eq!(fs::read_dir(root.path())?.count(), 0);
    let team = json_command(root.path(), &["team", "create", "implementation"])?;
    let team = team["team_id"].as_str().ok_or("team id missing")?;
    for member in ["writer", "reviewer"] {
        json_command(
            root.path(),
            &["team", "add-member", team, member, "general"],
        )?;
    }
    let sent = json_command(
        root.path(),
        &[
            "team",
            "send",
            team,
            "writer",
            "Review it. api_key=private-mail",
            "--to",
            "reviewer",
        ],
    )?;
    assert!(!sent.to_string().contains("private-mail"));
    for (member, expected) in [("writer", 0), ("reviewer", 1), ("reviewer", 0)] {
        let report = json_command(root.path(), &["team", "deliver", team, member])?;
        assert_eq!(report["count"], expected);
        if expected == 1 {
            assert_eq!(report["messages"][0]["from_agent_id"], "writer");
        }
    }
    assert!(!fs::read_to_string(
        root.path()
            .join(harness_core::team_mailbox_journal::TEAM_MAILBOX_JOURNAL_REL)
    )?
    .contains("private-mail"));
    Ok(())
}

#[test]
fn prompt_queue_keeps_parallel_writes_and_prioritizes_interjections_without_changing_history(
) -> Result<(), Box<dyn std::error::Error>> {
    let root = tempfile::tempdir()?;
    assert_eq!(
        json_command(
            root.path(),
            &["prompt-queue", "list", "--session", "session"]
        )?["count"],
        0
    );
    assert_eq!(
        json_command(
            root.path(),
            &["prompt-queue", "dequeue", "--session", "session"]
        )?["dequeued"],
        "empty"
    );
    assert_eq!(fs::read_dir(root.path())?.count(), 0);
    let session = root.path().join("session");
    fs::create_dir(&session)?;
    fs::write(session.join("events.jsonl"), "untouched history\n")?;
    let start = std::sync::Arc::new(std::sync::Barrier::new(2));
    std::thread::scope(|scope| {
        let threads: Vec<_> = ["first", "second"]
            .into_iter()
            .map(|id| {
                let start = std::sync::Arc::clone(&start);
                let root = root.path();
                scope.spawn(move || {
                    start.wait();
                    json_command(
                        root,
                        &[
                            "prompt-queue",
                            "enqueue",
                            id,
                            "--id",
                            id,
                            "--session",
                            "session",
                        ],
                    )
                    .map_err(|e| e.to_string())
                })
            })
            .collect();
        for thread in threads {
            thread.join().map_err(|_| "queue writer failed")??;
        }
        Ok::<_, String>(())
    })?;
    let urgent = json_command(
        root.path(),
        &[
            "prompt-queue",
            "interject",
            "Revise. api_key=private-prompt",
            "--id",
            "urgent",
            "--turn-running",
            "--session",
            "session",
        ],
    )?;
    assert_eq!(urgent["position"], 0);
    assert_eq!(urgent["turn_was_running"], true);
    assert_eq!(urgent["mutates_conversation_events"], false);
    let mut drained = Vec::new();
    for _ in 0..3 {
        drained.push(json_command(
            root.path(),
            &["prompt-queue", "dequeue", "--session", "session"],
        )?);
    }
    assert_eq!(drained[0]["id"], "urgent");
    assert!(!drained[0]["text"]
        .as_str()
        .ok_or("prompt text")?
        .contains("private-prompt"));
    let mut ids: Vec<_> = drained[1..]
        .iter()
        .filter_map(|v| v["id"].as_str())
        .collect();
    ids.sort_unstable();
    assert_eq!(ids, ["first", "second"]);
    assert_eq!(
        fs::read_to_string(session.join("events.jsonl"))?,
        "untouched history\n"
    );
    let store = session.join("tui/prompt-queue.json");
    fs::write(&store, r#"{"version":99,"entries":[]}"#)?;
    let (mut input, mut output, mut errors) = (Cursor::new(Vec::new()), Vec::new(), Vec::new());
    let result = run(
        [
            "harness",
            "prompt-queue",
            "enqueue",
            "must not overwrite",
            "--session",
            "session",
        ],
        &mut CliIo::new(&mut input, &mut output, &mut errors),
        CliDeps::real().with_current_dir(root.path().into()),
    );
    assert_ne!(result.code, 0);
    assert!(output.is_empty());
    assert_eq!(
        fs::read_to_string(&store)?,
        r#"{"version":99,"entries":[]}"#
    );
    Ok(())
}
