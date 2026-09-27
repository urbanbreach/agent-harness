#![cfg(unix)]
use harness_core::{
    clock::FakeClock,
    config::ShellAllowlist,
    coord::{spawn_coordinator, CoordinatorConfig},
    event::{ActorKind, EventActor, EventV1},
    redact::DefaultRedactor,
    store::read_events,
};
use serde_json::json;
use std::{fs, sync::Arc};

#[tokio::test]
async fn formatting_is_part_of_the_recorded_edit_and_failed_formatters_preserve_requested_text(
) -> Result<(), Box<dyn std::error::Error>> {
    let directory = tempfile::tempdir()?;
    let workspace = directory.path().join("workspace");
    fs::create_dir(&workspace)?;
    fs::write(workspace.join("record.demo"), "original\n")?;
    let script = r#"
import os, pathlib, sys
p = pathlib.Path(sys.argv[1])
text = p.read_text()
p.with_name(p.name + '.bak').write_text('staged backup')
assert os.environ['HARNESS_FORMATTER'] == '1'
if 'bad' in text:
    p.write_text('broken formatter output')
    print(os.environ['FORMAT_TOKEN'], file=sys.stderr)
    sys.exit(1)
if 'stale' in text:
    pathlib.Path('record.demo').write_text('concurrent editor\n')
p.write_text(text.upper())
"#;
    let mut config = CoordinatorConfig::new(directory.path().join("sessions"));
    config.always_approve_on_start = true;
    config.tool_registry = Arc::new(harness_tools::coordinator_registry(
        ShellAllowlist::default(),
    ));
    config.formatter = Arc::new(serde_json::from_value(json!({"fixture":{
        "command":["python3","-c",script,"$FILE"],
        "environment":{"FORMAT_TOKEN":"opaque-format-credential"},
        "extensions":[".demo"]
    }}))?);
    let coordinator = spawn_coordinator(
        config,
        Arc::new(FakeClock::new()),
        Arc::new(DefaultRedactor::default()),
    );
    let run = coordinator.start_run("formatting", &workspace).await?;
    let actor = || EventActor::new(ActorKind::User, None);
    coordinator
        .execute_agent_tool_call(actor(), None, "read", json!({"path":"record.demo"}))
        .await?;
    let result = coordinator
        .execute_agent_tool_call(
            actor(),
            None,
            "write",
            json!({"path":"record.demo","content":"alpha\n"}),
        )
        .await?;
    assert_eq!(
        fs::read_to_string(workspace.join("record.demo"))?,
        "ALPHA\n"
    );
    assert!(result.structured_json.as_ref().ok_or("result missing")?["format_warning"].is_null());
    let result = coordinator
        .execute_agent_tool_call(
            actor(),
            None,
            "edit",
            json!({"path":"record.demo","oldString":"ALPHA","newString":"bad"}),
        )
        .await?;
    assert_eq!(fs::read_to_string(workspace.join("record.demo"))?, "bad\n");
    let warning = result
        .structured_json
        .as_ref()
        .and_then(|v| v["format_warning"].as_str())
        .ok_or("warning missing")?;
    assert!(warning.contains("failed"));
    assert!(!warning.contains("opaque-format-credential"));
    let snapshots: Vec<_> = read_events(&run.events_path)?
        .into_iter()
        .filter_map(|e| match e.payload {
            EventV1::WorkspaceSnapshot(s) => Some(s.request_id.to_string()),
            _ => None,
        })
        .collect();
    assert_eq!(snapshots.len(), 2);
    coordinator.revert_workspace(snapshots[1].clone()).await?;
    assert_eq!(
        fs::read_to_string(workspace.join("record.demo"))?,
        "ALPHA\n"
    );
    coordinator
        .execute_agent_tool_call(actor(), None, "read", json!({"path":"record.demo"}))
        .await?;
    assert!(coordinator
        .execute_agent_tool_call(
            actor(),
            None,
            "edit",
            json!({"path":"record.demo","oldString":"ALPHA","newString":"stale"})
        )
        .await
        .is_err());
    assert_eq!(
        fs::read_to_string(workspace.join("record.demo"))?,
        "concurrent editor\n"
    );
    coordinator.stop_run().await?;
    let journal = fs::read_to_string(&run.events_path)?;
    assert!(!journal.contains("opaque-format-credential"));
    let edits: Vec<_> = read_events(&run.events_path)?
        .into_iter()
        .filter_map(|e| match e.payload {
            EventV1::EditApplied(edit) => edit.diff_rel_path,
            _ => None,
        })
        .collect();
    assert_eq!(edits.len(), 2);
    assert!(fs::read_to_string(run.run_dir.join(&edits[0]))?.contains("+ALPHA"));
    assert!(fs::read_dir(&workspace)?.all(|e| e.is_ok_and(|e| !e
        .file_name()
        .to_string_lossy()
        .starts_with(".harness-format-"))));
    Ok(())
}
