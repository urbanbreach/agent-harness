use super::*;
use crate::coord::{handle, spawn_coordinator, CoordinatorConfig};
use crate::subagent::GetCommandOrSubagentOutputResult;
use crate::{clock::FakeClock, redact::DefaultRedactor};

fn monitor_run(owner: &str, lines: &[&str]) -> crate::coord::commands::CommandRun {
    let snapshot = crate::coord::commands::CommandSnapshot {
        result: GetCommandOrSubagentOutputResult {
            status: "running".into(),
            ..Default::default()
        },
        owner_agent_id: Some(owner.to_owned()),
        original_owner_agent_id: None,
        parent_tool_call_id: String::new(),
        parent_task_id: None,
        description: None,
        cwd: String::new(),
        pid: None,
        started_mono_ms: 0,
        finished_mono_ms: None,
        stdout: String::new(),
        stderr: String::new(),
    };
    let (updates, _) = tokio::sync::watch::channel(snapshot.clone());
    crate::coord::commands::CommandRun {
        output_tail: None,
        snapshot,
        updates,
        finished: None,
        output: None,
        file_bytes: 0,
        pending: [Vec::new(), Vec::new()],
        discarded: [false, false],
        monitor: Some(Monitor {
            lines: lines.iter().map(|line| (*line).to_owned()).collect(),
            ..Monitor::default()
        }),
    }
}

/// Direct idle compaction waits for the monitor output its own agent still has to
/// receive, and only that agent: another agent's open window does not block it.
#[tokio::test]
async fn a_pending_monitor_blocks_idle_compaction_only_for_its_owner(
) -> Result<(), Box<dyn std::error::Error>> {
    let temp = tempfile::tempdir()?;
    let coordinator = spawn_coordinator(
        CoordinatorConfig::new(temp.path().join("sessions")),
        std::sync::Arc::new(FakeClock::new()),
        std::sync::Arc::new(DefaultRedactor::default()),
    );
    coordinator.start_run("monitor idle", temp.path()).await?;
    let owner = coordinator
        .spawn_agent_idle(handle::system(), "default", None)
        .await?;
    let other = coordinator
        .spawn_agent_idle(handle::system(), "default", None)
        .await?;
    let (owner_id, other_id) = (owner.clone(), other.clone());
    let blocked = coordinator
        .call(move |s| {
            let quiet = s.owns_pending_work(&owner_id);
            s.commands
                .insert("monitor-1".into(), monitor_run(&owner_id, &["ready"]));
            // A delivered monitor (nothing queued) holds nothing back.
            s.commands
                .insert("monitor-2".into(), monitor_run(&other_id, &[]));
            s.wake_monitors_at(Instant::now());
            Ok((
                quiet,
                s.owns_pending_work(&owner_id),
                s.owns_pending_work(&other_id),
            ))
        })
        .await?;
    assert_eq!(blocked, (false, true, false));
    coordinator.stop_run().await?;
    Ok(())
}

#[test]
fn plain_keeps_what_a_terminal_shows() {
    for (raw, shown) in [
        ("\u{1b}[32mPASS\u{1b}[0m tests::ok  ", "PASS tests::ok"),
        ("10%\r55%\r100% done\r", "100% done"),
        ("\u{1b}]0;title\u{7}ready\ton 3000", "ready on 3000"),
        ("", ""),
    ] {
        assert_eq!(super::plain(raw), shown);
    }
    assert!(super::plain(&"x".repeat(5000)).ends_with('…'));
}
