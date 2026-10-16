use super::skills::{skills, validated_directory};
use super::usage::{collect_records, date, groups, history};
use super::*;

#[test]
fn grouped_toggle_preserves_bodies_and_refuses_collision() -> Result<(), Box<dyn std::error::Error>>
{
    let temp = tempfile::tempdir()?;
    fs::create_dir(temp.path().join("workspace"))?;
    let root = temp.path().join("skills");
    for name in ["one", "two"] {
        fs::create_dir_all(root.join(name))?;
        fs::write(root.join(name).join("SKILL.md"), "---\nname: deploy\ndescription: Deploy safely\nallowed-tools: read bash\n---\nKeep this body.\n")?;
    }
    let config = SkillsConfig {
        project_roots: vec![],
        global_roots: vec!["../skills".into()],
        ..SkillsConfig::default()
    };
    let workspace = temp.path().join("workspace");
    let cwd = workspace.as_path();
    let list = skills(cwd, temp.path(), &config, "skills.list", &json!({}))?;
    assert_eq!(
        list["skills"][0]["installs"].as_array().map(Vec::len),
        Some(2)
    );
    let result = skills(
        cwd,
        temp.path(),
        &config,
        "skills.setEnabled",
        &json!({"id":"user:deploy","enabled":false}),
    )?;
    assert_eq!(result["changed"], 2);
    let disabled = skills(cwd, temp.path(), &config, "skills.list", &json!({}))?;
    assert_eq!(disabled["skills"][0]["body"], "Keep this body.");
    assert_eq!(disabled["skills"][0]["enabled"], false);
    assert_eq!(
        disabled["skills"][0]["allowedTools"],
        json!(["read", "bash"])
    );
    fs::write(root.join("two/SKILL.md"), "do not overwrite")?;
    assert!(skills(
        cwd,
        temp.path(),
        &config,
        "skills.setEnabled",
        &json!({"id":"user:deploy","enabled":true})
    )
    .is_err());
    assert!(!root.join("one/SKILL.md").exists());
    assert_eq!(
        fs::read_to_string(root.join("two/SKILL.md"))?,
        "do not overwrite"
    );
    fs::remove_file(root.join("two/SKILL.md"))?;
    let restored = skills(
        cwd,
        temp.path(),
        &config,
        "skills.setEnabled",
        &json!({"id":"user:deploy","enabled":true}),
    )?;
    assert_eq!(restored["changed"], 2);
    assert_eq!(
        skills(
            cwd,
            temp.path(),
            &config,
            "skills.read",
            &json!({"id":"user:deploy"})
        )?["body"],
        "Keep this body."
    );
    Ok(())
}

#[cfg(unix)]
#[test]
fn mutation_rejects_symlink_escape_and_root_deletion() -> Result<(), Box<dyn std::error::Error>> {
    let temp = tempfile::tempdir()?;
    let root = temp.path().join("skills");
    let outside = temp.path().join("outside");
    fs::create_dir_all(&root)?;
    fs::create_dir_all(&outside)?;
    fs::write(outside.join("SKILL.md"), "body")?;
    std::os::unix::fs::symlink(&outside, root.join("escape"))?;
    assert!(validated_directory(&root, &root.join("escape/SKILL.md")).is_err());
    fs::write(root.join("SKILL.md"), "body")?;
    assert!(validated_directory(&root, &root.join("SKILL.md")).is_err());
    Ok(())
}

#[test]
fn journal_usage_prices_cache_and_keeps_unknown_cost_explicit(
) -> Result<(), Box<dyn std::error::Error>> {
    let mut events = Vec::new();
    let payloads = [
        json!({"event_type":"run_started","data":{"run_name":"test","workspace_root":"/workspace"}}),
        json!({"event_type":"provider_request_started","data":{"request_id":"a","provider_id":"local","model_id":"priced","prompt_summary":"","request_digest":"","metadata":null}}),
        json!({"event_type":"provider_request_finished","data":{"request_id":"a","finish_reason":"stop","output_digest":null,"usage":{"prompt_tokens":100,"completion_tokens":10,"total_tokens":110,"cache_read_tokens":40},"metadata":null}}),
        json!({"event_type":"provider_request_started","data":{"request_id":"b","provider_id":"local","model_id":"unknown","prompt_summary":"","request_digest":"","metadata":null}}),
        json!({"event_type":"provider_request_finished","data":{"request_id":"b","finish_reason":"stop","output_digest":null,"usage":{"prompt_tokens":5,"completion_tokens":2,"total_tokens":7},"metadata":null}}),
    ];
    for (seq, payload) in payloads.into_iter().enumerate() {
        events.push(serde_json::from_value(json!({"schema_version":1,"event_id":format!("e{seq}"),"seq":seq+1,"run_id":"run","mono_ms":seq,"ts":"2026-10-16T12:00:00Z","actor":{"kind":"system"},"payload":payload}))?);
    }
    let cost: ModelCost =
        serde_json::from_value(json!({"input":2.0,"output":4.0,"cache_read":0.5}))?;
    let prices = BTreeMap::from([(("local".into(), "priced".into()), cost)]);
    let mut errors = Vec::new();
    let records = collect_records(
        &events,
        "run",
        "2026-10-16",
        "2026-10-16",
        &prices,
        &mut errors,
    );
    let totals = groups(&records, |_| "total".into(), true);
    assert_eq!(totals[0]["inputTokens"], 65);
    assert_eq!(totals[0]["cachedInputTokens"], 40);
    assert_eq!(totals[0]["totalTokens"], 117);
    assert_eq!(totals[0]["unpricedRecords"], 1);
    assert_eq!(totals[0]["costUsd"], Value::Null);
    assert_eq!(records[0].cost, Some(180_000_000));
    assert!(collect_records(
        &events,
        "run",
        "2026-10-17",
        "2026-10-18",
        &prices,
        &mut errors
    )
    .is_empty());
    assert!(errors.is_empty());
    let temp = tempfile::tempdir()?;
    let run = temp.path().join("sessions/project/run");
    harness_core::store::create_private_dir(&run)?;
    if let EventV1::ProviderRequestFinished(end) = &mut events[2].payload {
        end.metadata = Some(harness_core::event::ProviderRequestFinishedMetadata {
            cost_picodollars: Some(180_000_000),
            ..Default::default()
        });
    }
    let mut journal = Vec::new();
    for event in &events {
        serde_json::to_writer(&mut journal, event)?;
        journal.push(b'\n');
    }
    harness_core::store::write_private_atomic(&run.join("events.jsonl"), &journal)?;
    let snapshot = history(
        temp.path(),
        &temp.path().join("sessions/project"),
        None,
        &json!({"from":"2026-10-16","to":"2026-10-16"}),
    )?;
    assert_eq!(snapshot["totals"]["totalTokens"], 117);
    assert_eq!(snapshot["totals"]["sessions"], 1);
    assert_eq!(snapshot["totals"]["unpricedRecords"], 1);
    assert_eq!(snapshot["quality"]["modelPricedRecords"], 1);
    assert_eq!(snapshot["byProject"][0]["project"], "/workspace");
    assert_eq!(snapshot["scan"]["scanned"], 1);
    exercise_journal_lineage_usage(temp.path(), &run, &events)?;
    Ok(())
}

fn exercise_journal_lineage_usage(
    home: &Path,
    source: &Path,
    events: &[EventEnvelopeV1],
) -> Result<(), Box<dyn std::error::Error>> {
    use harness_core::session_lineage::*;
    use harness_core::store::{EventStore, Journal};
    let sessions = source.parent().ok_or("session parent")?;
    let query = || {
        history(
            home,
            sessions,
            None,
            &json!({"from":"2026-10-16","to":"2026-10-16"}),
        )
    };
    let mirror = Journal::open(sessions, "child", true)?;
    // Child journals have their own sequence space and source:<run>:<seq> provenance.
    let mut header = events[0].clone();
    header.run_id = "child".into();
    header.event_id = "child-header".into();
    mirror.append(header.clone().into())?;
    header.event_id = "child-agent".into();
    header.payload = EventV1::AgentSpawned(harness_core::event::AgentSpawnedEvent {
        agent_id: "child".into(),
        profile: "default".into(),
        parent_agent_id: Some("parent".into()),
    });
    mirror.append(header.into())?;
    for event in &events[1..] {
        let mut copy = event.clone();
        copy.run_id = "child".into();
        copy.event_id = format!("source:{}:{}", event.run_id, event.seq);
        mirror.append(copy.into())?;
    }
    let mut terminal = events[0].clone();
    terminal.run_id = "child".into();
    terminal.event_id = "child-finished".into();
    terminal.payload = EventV1::RunFinished(harness_core::event::RunFinishedEvent {
        summary: "done".into(),
    });
    mirror.append(terminal.into())?;
    mirror.close_writer()?;
    assert_eq!(query()?["totals"]["totalTokens"], 117);
    let child_dir = sessions.join("child");
    let child_events = harness_core::store::read_events(&child_dir.join("events.jsonl"))?;
    let stable = latest_clone_stable_prefix(&child_events)?;
    let fork = materialize_child_session_as(
        ChildSessionMaterializationRequest {
            source_run_dir: &child_dir,
            events: &child_events,
            stable_prefix: &stable,
            source_kind: ChildSessionMaterializationSourceKind::DiskRunDirectory,
        },
        Some("fork"),
    )?;
    assert_eq!(query()?["totals"]["totalTokens"], 117);
    let fork_events = harness_core::store::read_events(&fork.child_run_dir.join("events.jsonl"))?;
    let stable = validate_fork_stable_prefix(&fork_events, fork_events.len() as u64)?;
    let clone = materialize_child_session_as(
        ChildSessionMaterializationRequest {
            source_run_dir: &fork.child_run_dir,
            events: &fork_events,
            stable_prefix: &stable,
            source_kind: ChildSessionMaterializationSourceKind::DiskRunDirectory,
        },
        Some("clone"),
    )?;
    assert_eq!(query()?["totals"]["totalTokens"], 117);
    let writer = Journal::open(sessions, &clone.child_run_id, false)?;
    // Request IDs may be reused after a branch; new finishes are separate provider work.
    for event in &events[1..3] {
        let mut continued = event.clone();
        continued.run_id = clone.child_run_id.clone().into();
        continued.event_id = format!("continued-{}", event.seq);
        writer.append(continued.into())?;
    }
    writer.close_writer()?;
    let snapshot = query()?;
    assert_eq!(snapshot["totals"]["totalTokens"], 227);
    assert_eq!(snapshot["totals"]["knownCostUsd"], 0.00036);
    let child_writer = Journal::open(sessions, "child", false)?;
    for event in &events[1..3] {
        let mut continued = event.clone();
        continued.run_id = "child".into();
        continued.event_id = format!("child-continued-{}", event.seq);
        child_writer.append(continued.into())?;
    }
    child_writer.close_writer()?;
    assert_eq!(query()?["totals"]["totalTokens"], 337);
    Ok(())
}

#[test]
fn date_validation_rejects_invalid_calendar_days_without_panicking() {
    for value in ["2026-02-29", "2026-13-01", "0000-01-01", "ééééé"] {
        assert!(date(value).is_err(), "{value}");
    }
    assert!(date("2024-02-29").is_ok());
}
