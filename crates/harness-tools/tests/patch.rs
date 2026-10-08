use harness_core::{
    clock::FakeClock,
    config::ShellAllowlist,
    coord::{spawn_coordinator, CoordinatorConfig},
    event::{ActorKind, EventActor, EventV1},
    perm::PermissionPolicy,
    redact::DefaultRedactor,
};
use serde_json::json;
use std::{fs, sync::Arc};

#[tokio::test]
async fn patches_validate_structure_and_context_and_record_each_completed_file(
) -> Result<(), Box<dyn std::error::Error>> {
    let temp = tempfile::tempdir()?;
    let root = temp.path().join("workspace");
    fs::create_dir(&root)?;
    fs::write(root.join("a.txt"), "one\r\nkeep\r\ntwo\r\n")?;
    fs::write(root.join("old.txt"), "remove\n")?;
    let mut config = CoordinatorConfig::new(temp.path().join("sessions"));
    config.secret_values = vec!["opaque-edit-credential".into()];
    config.permission_policy = PermissionPolicy::allow_all();
    config.tool_registry = Arc::new(harness_tools::coordinator_registry(
        ShellAllowlist::default(),
    ));
    let coordinator = spawn_coordinator(
        config,
        Arc::new(FakeClock::new()),
        Arc::new(DefaultRedactor::default()),
    );
    let run = coordinator.start_run("patch", &root).await?;
    let actor = || EventActor::new(ActorKind::User, None);
    for path in ["a.txt", "old.txt"] {
        coordinator
            .execute_agent_tool_call(actor(), None, "read", json!({"path":path}))
            .await?;
    }
    let patch = "*** Begin Patch\n*** Update File: a.txt\n@@\n-one\n+ONE\n keep\n two\n*** Add File: nested/new.txt\n+created\n*** Delete File: old.txt\n*** End Patch";
    let result = coordinator
        .execute_agent_tool_call(actor(), None, "apply_patch", json!({"patchText":patch}))
        .await?;
    assert!(!result.is_error());
    assert_eq!(
        fs::read_to_string(root.join("a.txt"))?,
        "ONE\r\nkeep\r\ntwo\r\n"
    );
    assert_eq!(
        fs::read_to_string(root.join("nested/new.txt"))?,
        "created\n"
    );
    assert!(!root.join("old.txt").exists());
    let invalid = "*** Begin Patch\n*** Update File: a.txt\n@@\n-ONE\n+wrong\n*** Move to: elsewhere.txt\n*** End Patch";
    assert!(coordinator
        .execute_agent_tool_call(actor(), None, "apply_patch", json!({"patchText":invalid}))
        .await
        .is_err());
    assert_eq!(
        fs::read_to_string(root.join("a.txt"))?,
        "ONE\r\nkeep\r\ntwo\r\n"
    );
    let stale = "*** Begin Patch\n*** Update File: a.txt\n@@\n-missing\n+wrong\n*** End Patch";
    assert!(coordinator
        .execute_agent_tool_call(actor(), None, "apply_patch", json!({"patchText":stale}))
        .await?
        .is_error());
    assert_eq!(
        fs::read_to_string(root.join("a.txt"))?,
        "ONE\r\nkeep\r\ntwo\r\n"
    );
    let runtime_dir =
        harness_core::storage_paths::ProjectPaths::new(&temp.path().join("sessions/data"), &root)?
            .runtime_dir();
    let attribution =
        harness_core::edit_attribution::EditAttributionJournal::open(&root, &runtime_dir)?;
    assert_eq!(attribution.blame("a.txt")?.agent_lines, 3);
    assert!(!attribution.diff("old.txt")?.drifted);
    fs::write(root.join("a.txt"), "ONE\r\nexternal\r\ntwo\r\n")?;
    let blame = attribution.blame("a.txt")?;
    assert_eq!((blame.agent_lines, blame.external_lines), (2, 1));
    let journal = fs::read(attribution.journal_path())?;
    for content in ["api_key=private-edit-credential", "opaque-edit-credential"] {
        coordinator
            .execute_agent_tool_call(actor(), None, "read", json!({"path":"nested/new.txt"}))
            .await?;
        let result = coordinator
            .execute_agent_tool_call(
                actor(),
                None,
                "write",
                json!({"path":"nested/new.txt","content":content}),
            )
            .await?;
        assert!(
            !result.is_error(),
            "an attribution refusal must not report a failed file write"
        );
        assert_eq!(fs::read_to_string(root.join("nested/new.txt"))?, content);
        assert!(attribution.diff("nested/new.txt")?.drifted);
        assert_eq!(fs::read(attribution.journal_path())?, journal);
    }
    coordinator.stop_run().await?;
    let events = harness_core::store::read_events(&run.events_path)?;
    assert_eq!(
        events
            .iter()
            .filter(|e| matches!(e.payload, EventV1::EditApplied(_)))
            .count(),
        5
    );
    Ok(())
}
