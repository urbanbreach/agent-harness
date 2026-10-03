use harness::{run, CliDeps, CliIo};
use serde_json::Value;
use std::{fs, io::Cursor, path::Path};

fn invoke(root: &Path, args: &[&str]) -> (i32, Vec<u8>, Vec<u8>) {
    let (mut input, mut output, mut errors) = (Cursor::new(Vec::new()), Vec::new(), Vec::new());
    let result = run(
        ["harness", "--session-dir", "sessions"]
            .into_iter()
            .chain(args.iter().copied()),
        &mut CliIo::new(&mut input, &mut output, &mut errors),
        CliDeps::real()
            .with_current_dir(root.into())
            .without_env("HOME")
            .without_env("XDG_CONFIG_HOME")
            .without_env("HARNESS_CONFIG")
            .without_env("HARNESS_TUI_CONFIG")
            .without_env("HARNESS_CONFIG_CONTENT")
            .with_env("HARNESS_DATA_HOME", root.join("data").to_string_lossy())
            .with_env(
                "HARNESS_EXPORT_FIXTURE_TOKEN",
                "opaque-environment-credential",
            ),
    );
    (result.code, output, errors)
}
fn command(root: &Path, args: &[&str]) -> Result<Value, Box<dyn std::error::Error>> {
    let (code, output, errors) = invoke(root, args);
    assert_eq!(code, 0, "{}", String::from_utf8_lossy(&errors));
    Ok(serde_json::from_slice(&output)?)
}
fn prompt(root: &Path, id: &str) -> Result<(), Box<dyn std::error::Error>> {
    let (code, _, errors) = invoke(
        root,
        &[
            "prompt",
            "--mock",
            "--text",
            "Remember this",
            "--session-id",
            id,
        ],
    );
    assert_eq!(code, 0, "{}", String::from_utf8_lossy(&errors));
    Ok(())
}

#[test]
fn session_navigation_filters_pages_and_reports_corruption_without_writing(
) -> Result<(), Box<dyn std::error::Error>> {
    let root = tempfile::tempdir()?;
    assert_eq!(
        command(root.path(), &["sessions", "list", "--json"])?,
        serde_json::json!([])
    );
    assert!(!root.path().join("sessions").exists());
    for id in ["alpha", "beta"] {
        prompt(root.path(), id)?;
    }
    for (args, count) in [
        (vec!["dashboard", "list", "--json"], 2),
        (vec!["dashboard", "status", "--json"], 2),
        (vec!["dashboard", "recent", "--limit", "1", "--json"], 1),
    ] {
        let dashboard = command(root.path(), &args)?;
        assert_eq!(dashboard["session_count"], count);
        assert_eq!(
            dashboard["config_loaded"].as_bool(),
            (args[1] == "status").then_some(false)
        );
    }
    let source = root.path().join("sessions/alpha/events.jsonl");
    let before = fs::read(&source)?;
    let first = command(
        root.path(),
        &[
            "sessions",
            "list",
            "--json",
            "--status",
            "finished",
            "--resumable",
            "true",
            "--sort",
            "run_id_asc",
            "--limit",
            "1",
        ],
    )?;
    assert_eq!(first[0]["run_id"], "alpha");
    assert!(first[0]["last_updated_at"].is_string());
    let cursor = first[0]["cursor"].as_str().ok_or("cursor missing")?;
    let next = command(
        root.path(),
        &[
            "sessions",
            "list",
            "--json",
            "--sort",
            "run_id_asc",
            "--cursor",
            cursor,
        ],
    )?;
    assert_eq!(next.as_array().map(Vec::len), Some(1));
    assert_eq!(next[0]["run_id"], "beta");
    let search = command(root.path(), &["sessions", "search", "ALPHA", "--json"])?;
    assert_eq!(search.as_array().map(Vec::len), Some(1));
    let inspect = command(
        root.path(),
        &["sessions", "inspect", "--run", "alpha", "--json"],
    )?;
    assert_eq!(inspect["catalog"]["is_resumable"], true);
    let replay = command(root.path(), &["sessions", "replay", "alpha", "--json"])?;
    assert_eq!(replay["status"], "finished");
    assert_eq!(fs::read(&source)?, before);
    assert_eq!(fs::read_dir(root.path().join("sessions"))?.count(), 2);
    let broken = root.path().join("sessions/broken");
    fs::create_dir(&broken)?;
    fs::write(broken.join("events.jsonl"), "invalid journal\n")?;
    let unavailable = command(
        root.path(),
        &["sessions", "list", "--json", "--status", "unavailable"],
    )?;
    assert_eq!(unavailable[0]["run_id"], "broken");
    assert_eq!(unavailable[0]["is_resumable"], false);
    assert!(unavailable[0]["resume_disabled_reason"].is_string());
    assert_eq!(
        invoke(root.path(), &["sessions", "list", "--cursor", "stale"]).0,
        1
    );
    check_index(root.path())?;
    check_configured_location(root.path())?;
    Ok(())
}
fn check_configured_location(root: &Path) -> Result<(), Box<dyn std::error::Error>> {
    fs::write(
        root.join("location.json"),
        r#"{"runtime":{"session_dir":"sessions"}}"#,
    )?;
    for (args, expected) in [
        (vec!["sessions", "list", "--json"], "alpha"),
        (vec!["sessions", "inspect", "alpha", "--json"], "alpha"),
        (vec!["export", "alpha"], "Remember this"),
    ] {
        let (mut input, mut output, mut errors) = (Cursor::new(Vec::new()), Vec::new(), Vec::new());
        let result = run(
            ["harness", "--config", "location.json"]
                .into_iter()
                .chain(args),
            &mut CliIo::new(&mut input, &mut output, &mut errors),
            CliDeps::real()
                .with_current_dir(root.into())
                .without_env("HOME")
                .without_env("XDG_CONFIG_HOME")
                .without_env("HARNESS_CONFIG")
                .without_env("HARNESS_TUI_CONFIG")
                .without_env("HARNESS_CONFIG_CONTENT"),
        );
        assert_eq!(result.code, 0, "{}", String::from_utf8_lossy(&errors));
        assert!(String::from_utf8(output)?.contains(expected));
    }
    Ok(())
}

fn check_index(root: &Path) -> Result<(), Box<dyn std::error::Error>> {
    let report = command(root, &["sessions", "rebuild-index", "--json"])?;
    assert_eq!(report["entry_count"], 3);
    let index = Path::new(report["index_path"].as_str().ok_or("index path")?);
    let original = fs::read(index)?;
    let before = command(
        root,
        &["sessions", "list", "--json", "--sort", "run_id_asc"],
    )?;
    assert_eq!(
        fs::read(index)?,
        original,
        "listing must not rewrite the index"
    );
    fs::write(index, "{\"version\":999}")?;
    assert_eq!(
        command(
            root,
            &["sessions", "list", "--json", "--sort", "run_id_asc"]
        )?,
        before
    );
    assert_eq!(fs::read_to_string(index)?, "{\"version\":999}");
    fs::write(index, original)?;
    let path = root.join("sessions/alpha/events.jsonl");
    let events = harness_core::store::read_events(&path)?;
    let mut title = events.last().ok_or("event missing")?.clone();
    title.seq += 1;
    title.event_id = "new-title".into();
    title.payload = harness_core::event::EventV1::SessionTitleUpdated(
        harness_core::event::SessionTitleUpdatedEvent {
            title: "Renamed session".into(),
        },
    );
    use std::io::Write;
    writeln!(
        fs::OpenOptions::new().append(true).open(&path)?,
        "{}",
        serde_json::to_string(&title)?
    )?;
    let changed = command(root, &["sessions", "search", "Renamed session", "--json"])?;
    assert_eq!(changed[0]["run_id"], "alpha");
    let metadata = root.join("sessions/beta/meta.json");
    let mut value: Value = serde_json::from_slice(&fs::read(&metadata)?)?;
    value["mode_source"] = "scenario_fixture".into();
    fs::write(metadata, serde_json::to_vec(&value)?)?;
    let filtered = command(root, &["sessions", "list", "--json"])?;
    assert_eq!(filtered.as_array().map(Vec::len), Some(2));
    Ok(())
}

#[test]
fn session_branch_import_and_recovery_keep_source_history_intact(
) -> Result<(), Box<dyn std::error::Error>> {
    let root = tempfile::tempdir()?;
    prompt(root.path(), "parent")?;
    let source = root.path().join("sessions/parent/events.jsonl");
    let original = fs::read(&source)?;
    let events = harness_core::store::read_events(&source)?;
    let cutoff = events.len().to_string();
    let fork = command(
        root.path(),
        &[
            "sessions", "fork", "--source", "parent", "--cutoff", &cutoff, "--json",
        ],
    )?;
    let clone = command(
        root.path(),
        &["sessions", "clone", "--source", "parent", "--json"],
    )?;
    assert_ne!(fork["child_run_id"], clone["child_run_id"]);
    let tree = command(
        root.path(),
        &["sessions", "tree", "--root", "parent", "--json"],
    )?;
    assert_eq!(tree["session_count"], 3);
    assert_eq!(tree["harness_lineage"][0]["depth"], 0);
    assert_eq!(tree["harness_lineage"][1]["parent_session_id"], "parent");
    assert_eq!(
        invoke(
            root.path(),
            &["sessions", "fork", "--source", "parent", "--cutoff", "2"]
        )
        .0,
        1
    );
    let imported = command(
        root.path(),
        &["sessions", "import", "--from", "sessions/parent", "--json"],
    )?;
    let id = imported["run_id"].as_str().ok_or("import id")?;
    let inspected = command(root.path(), &["sessions", "inspect", id, "--json"])?;
    assert_eq!(inspected["catalog"]["is_resumable"], false);
    assert_eq!(inspected["catalog"]["mode_source"], "replay_only");
    fs::write(
        root.path().join("sessions/parent/.writer.lock.recovering"),
        "interrupted",
    )?;
    let scan = command(root.path(), &["sessions", "crash-scan", "--json"])?;
    assert_eq!(scan["summary"]["previous_crash"], 1);
    let reopened = command(
        root.path(),
        &["sessions", "reopen", "--session", "parent", "--json"],
    )?;
    assert_eq!(reopened["crash_recovery"]["recovery_marker_cleared"], true);
    assert_eq!(fs::read(&source)?, original);
    assert!(!root
        .path()
        .join("sessions/parent/.writer.lock.recovering")
        .exists());
    Ok(())
}

#[test]
fn exports_redact_old_credentials_omit_reasoning_and_refuse_unsafe_outputs(
) -> Result<(), Box<dyn std::error::Error>> {
    let root = tempfile::tempdir()?;
    prompt(root.path(), "legacy")?;
    let credentials = [
        "opaque-prior-credential",
        "opaque-remote-credential",
        "opaque-mcp-credential",
        "opaque-lsp-credential",
        "opaque-hook-credential",
        "opaque-format-credential",
        "opaque-stored-credential",
        "opaque-environment-credential",
    ];
    harness_core::auth::CredentialStore::new(root.path().join("data/harness")).save(
        &harness_core::auth::StoredCredential::api_key(
            harness_core::auth::ProviderId::parse("unrelated").ok_or("provider id")?,
            "opaque-stored-credential",
            "2026-09-26T00:00:00Z",
        ),
    )?;
    let source = root.path().join("sessions/legacy/events.jsonl");
    let (journal, count) = legacy_journal(&source, &credentials)?;
    fs::write(&source, &journal)?;
    fs::write(
        root.path().join("credentials.json"),
        serde_json::json!({
            "provider":{"fixture":{"type":"openai_compatible","apiKey":"opaque-prior-credential","models":{"fixture":{}}}},
            "integrations":{"remote_search":{"auth_token":"opaque-remote-credential"}},
            "mcp":{"fixture":{"transport":"stdio","command":["never-started"],"enabled":false,"env":{"TOKEN":"opaque-mcp-credential"}}},
            "lsp":{"servers":{"fixture":{"env":{"TOKEN":"opaque-lsp-credential"}}}},
            "hooks":{"lifecycle":[{"event":"run_started","command":["never-started"],"env":{"TOKEN":"opaque-hook-credential"}}]},
            "formatter":{"fixture":{"environment":{"TOKEN":"opaque-format-credential"}}}
        }).to_string(),
    )?;
    check_exports(root.path(), &credentials, count)?;
    assert_eq!(fs::read_to_string(&source)?, journal);
    let result = invoke(
        root.path(),
        &[
            "sessions",
            "export",
            "legacy",
            "--output",
            "sessions/legacy/events.jsonl",
        ],
    );
    assert_eq!(result.0, 1);
    assert_eq!(fs::read_to_string(&source)?, journal);
    let metadata = root.path().join("sessions/legacy/meta.json");
    let mut value: Value = serde_json::from_slice(&fs::read(&metadata)?)?;
    value["opaque-prior-credential"] = true.into();
    fs::write(metadata, serde_json::to_vec(&value)?)?;
    fs::write(root.path().join("export.json"), "keep this")?;
    let (code, output, errors) = invoke(
        root.path(),
        &[
            "--config",
            "credentials.json",
            "sessions",
            "export",
            "legacy",
            "--output",
            "export.json",
        ],
    );
    assert_eq!(code, 1);
    assert!(output.is_empty());
    assert!(String::from_utf8(errors)?.contains("secret"));
    assert_eq!(
        fs::read_to_string(root.path().join("export.json"))?,
        "keep this"
    );
    Ok(())
}

fn legacy_journal(
    source: &Path,
    credentials: &[&str],
) -> Result<(String, usize), Box<dyn std::error::Error>> {
    use harness_core::{event::EventV1, session::AssistantPart};
    let mut events = harness_core::store::read_events(&source)?;
    for event in &mut events {
        if let EventV1::AssistantMessageFinished(message) = &mut event.payload {
            message.parts.push(AssistantPart::Text {
                text: credentials.join(" "),
            });
            message.parts.push(AssistantPart::Reasoning {
                text: "private reasoning must be omitted".into(),
            });
        }
    }
    let mut legacy = events.last().ok_or("no events")?.clone();
    legacy.event_id = "legacy-reasoning".into();
    legacy.payload =
        EventV1::ProviderReasoningDelta(harness_core::event::ProviderReasoningDeltaEvent {
            request_id: "old-provider-request".into(),
            delta: "private reasoning must be omitted".into(),
        });
    events.insert(events.len() - 1, legacy);
    let mut journal = String::new();
    for (index, event) in events.iter_mut().enumerate() {
        event.seq = index as u64 + 1;
        journal.push_str(&serde_json::to_string(event)?);
        journal.push('\n');
    }
    Ok((journal, events.len()))
}

fn check_exports(
    root: &Path,
    credentials: &[&str],
    count: usize,
) -> Result<(), Box<dyn std::error::Error>> {
    let bundle = command(
        root,
        &[
            "--config",
            "credentials.json",
            "sessions",
            "export",
            "legacy",
        ],
    )?;
    let rendered = bundle.to_string();
    for credential in credentials {
        assert!(!rendered.contains(credential), "leaked {credential}");
    }
    assert!(!rendered.contains("private reasoning"));
    assert!(!rendered.contains("provider_reasoning_delta"));
    assert_eq!(bundle["replay"]["counts"]["total_events"], count - 1);
    assert_eq!(bundle["support"]["secret_scan_status"]["status"], "clean");
    let (code, markdown, errors) =
        invoke(root, &["--config", "credentials.json", "export", "legacy"]);
    assert_eq!(code, 0, "{}", String::from_utf8_lossy(&errors));
    let markdown = String::from_utf8(markdown)?;
    assert!(markdown.contains("## User"));
    assert!(markdown.contains("## Assistant"));
    assert!(markdown.contains("[REDACTED]"));
    assert!(!markdown.contains("private reasoning"));
    assert!(credentials
        .iter()
        .all(|credential| !markdown.contains(credential)));
    Ok(())
}
