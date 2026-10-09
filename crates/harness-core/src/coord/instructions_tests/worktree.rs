use super::*;

#[tokio::test]
async fn directory_instructions_rearm_after_rewind_with_or_without_resume(
) -> Result<(), Box<dyn std::error::Error>> {
    for resume in [false, true] {
        let temp = tempfile::tempdir()?;
        std::fs::create_dir(temp.path().join("sub"))?;
        std::fs::write(temp.path().join("sub/file"), "data")?;
        std::fs::write(temp.path().join("sub/AGENTS.md"), "REWIND_INSTRUCTIONS")?;
        let provider = Arc::new(MockProvider::script([
            touch("initial", json!(["sub/file"])),
            super::super::super::compaction_tests::answer("discarded"),
            touch("replacement", json!(["sub/file"])),
            super::super::super::compaction_tests::answer("done"),
        ]));
        let config = configuration(temp.path(), &provider);
        let mut coordinator = spawn(config.clone());
        let run = coordinator
            .start_run("instruction rewind", temp.path())
            .await?;
        let agent = coordinator
            .spawn_agent_idle(
                EventActor::new(ActorKind::Supervisor, None),
                "default",
                None,
            )
            .await?;
        let request = coordinator
            .request_agent_turn(
                EventActor::new(ActorKind::User, None),
                agent.clone(),
                "discard this",
            )
            .await?;
        super::super::super::history_tests::settled(&coordinator, &request).await?;
        assert_eq!(recorded(&run.events_path)?.len(), 1);
        coordinator.rewind_conversation(request).await?;
        if resume {
            coordinator.stop_run().await?;
            coordinator = spawn(config);
            coordinator
                .resume_run(run.run_id.to_string(), "instruction rewind resumed")
                .await?;
        }
        turn(&coordinator, &agent).await?;
        assert_eq!(recorded(&run.events_path)?.len(), 2);
        let requests = provider.captured_requests().await;
        assert_eq!(
            requests[3]
                .messages
                .iter()
                .filter(|m| m.content.contains("REWIND_INSTRUCTIONS"))
                .count(),
            1
        );
        coordinator.stop_run().await?;
    }
    Ok(())
}

#[tokio::test]
async fn directory_instructions_use_the_native_worktree_root_and_map_startup_paths(
) -> Result<(), Box<dyn std::error::Error>> {
    let temp = tempfile::tempdir()?;
    let root = temp.path().join("repository");
    std::fs::create_dir_all(root.join("sub"))?;
    std::fs::write(root.join("AGENTS.md"), "WORKTREE_STARTUP")?;
    std::fs::write(root.join("sub/AGENTS.md"), "WORKTREE_NESTED")?;
    std::fs::write(root.join("sub/file"), "data")?;
    for args in [
        vec!["init", "--quiet"],
        vec!["add", "."],
        vec![
            "-c",
            "user.name=Fixture",
            "-c",
            "user.email=fixture@localhost",
            "commit",
            "--quiet",
            "-m",
            "fixture",
        ],
    ] {
        let output = tokio::process::Command::new("git")
            .current_dir(&root)
            .args(args)
            .output()
            .await?;
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
    let provider = Arc::new(MockProvider::script([
        touch("child-read", json!(["sub/file"])),
        super::super::super::compaction_tests::answer("child done"),
    ]));
    let mut config = super::super::super::children_tests::configuration(
        temp.path(),
        Arc::clone(&provider) as Arc<dyn harness_providers::Provider>,
    );
    Arc::get_mut(&mut config.tool_registry)
        .ok_or("shared fixture registry")?
        .register(Arc::new(ReadPaths));
    config
        .agent_profiles
        .get_mut("default")
        .ok_or("missing profile")?
        .toolset
        .push("read_paths".into());
    config.behavior.directory_instructions.enabled = true;
    config.instruction_paths = vec![root.join("AGENTS.md")];
    let worktrees =
        crate::storage_paths::ProjectPaths::new(&config.data_dir, &root)?.worktrees_dir();
    let coordinator = spawn(config);
    let run = coordinator.start_run("instruction worktree", &root).await?;
    let parent = coordinator
        .spawn_agent_idle(
            EventActor::new(ActorKind::Supervisor, None),
            "default",
            None,
        )
        .await?;
    let result = coordinator.execute_tool(EventActor::new(ActorKind::Worker, Some(parent)), None, Some("spawn-worktree".into()), "spawn_subagent".into(), json!({"prompt":"read sub/file", "description":"instruction worktree", "subagent_type":"native-fixture", "background":false, "isolation":"worktree"})).await?;
    assert!(!result.is_error(), "{}", result.display_text);
    let requests = provider.captured_requests().await;
    assert_eq!(requests.len(), 2);
    assert!(requests[1]
        .messages
        .iter()
        .any(|m| m.content.contains("WORKTREE_NESTED")));
    let reminders = recorded(&run.events_path)?;
    assert_eq!(reminders.len(), 1);
    assert!(reminders[0]
        .source
        .as_ref()
        .is_some_and(|p| Path::new(p).starts_with(worktrees.join(run.run_id.as_str()))));
    assert!(!reminders[0].text.contains("WORKTREE_STARTUP"));
    coordinator.stop_run().await?;
    Ok(())
}
