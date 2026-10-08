use super::super::*;
use crate::{
    clock::FakeClock,
    redact::DefaultRedactor,
    tool::{Tool, ToolCapability, ToolContext, ToolError, ToolRegistry, ToolResult},
};
use harness_providers::{mock::MockProvider, ProviderStreamEvent as Stream};
use serde_json::{json, Value};

struct ReadPaths;
#[async_trait::async_trait]
impl Tool for ReadPaths {
    fn id(&self) -> &'static str {
        "read_paths"
    }
    fn parameters_json_schema(&self) -> Value {
        json!({"type":"object"})
    }
    fn capability(&self) -> ToolCapability {
        ToolCapability::ReadFs
    }
    fn filesystem_paths(&self, args: &Value) -> Result<Vec<PathBuf>, ToolError> {
        args["paths"]
            .as_array()
            .ok_or_else(|| ToolError::InvalidArguments("paths must be an array".into()))?
            .iter()
            .map(|path| {
                path.as_str()
                    .map(PathBuf::from)
                    .ok_or_else(|| ToolError::InvalidArguments("path must be a string".into()))
            })
            .collect()
    }
    async fn call(&self, context: ToolContext, args: Value) -> Result<ToolResult, ToolError> {
        for path in self.filesystem_paths(&args)? {
            let path = context.workspace_root.join(path);
            if path.is_dir() {
                let _ = tokio::fs::read_dir(path).await?;
            } else {
                let _ = tokio::fs::read(path).await?;
            }
        }
        Ok(ToolResult::text("Files read."))
    }
}

fn touch(id: &str, paths: Value) -> Vec<Stream> {
    vec![
        Stream::ToolCallComplete {
            tool_call_id: id.into(),
            function_name: "read_paths".into(),
            arguments_json: json!({"paths": paths}).to_string(),
        },
        Stream::Done { usage: None },
    ]
}

fn configuration(root: &Path, provider: &Arc<MockProvider>) -> CoordinatorConfig {
    let mut registry = ToolRegistry::new();
    registry.register(Arc::new(ReadPaths));
    let mut config = CoordinatorConfig::new(root.join("sessions"));
    config.provider = Arc::clone(provider) as Arc<dyn harness_providers::Provider>;
    config.tool_registry = Arc::new(registry);
    config.permission_policy = PermissionPolicy::allow_all();
    config.behavior.directory_instructions.enabled = true;
    config.behavior.directory_instructions.max_bytes = 1024;
    config.compaction.keep_recent_tokens = 1;
    config.compaction.reserve_tokens = 0;
    config.compaction.suppress_auto_compaction = true;
    let mut profile = crate::agent::AgentProfile::fallback("default");
    profile.toolset = vec!["read_paths".into()];
    config.agent_profiles.insert("default".into(), profile);
    config
}

fn spawn(config: CoordinatorConfig) -> CoordinatorHandle {
    spawn_coordinator(
        config,
        Arc::new(FakeClock::new()),
        Arc::new(DefaultRedactor::default()),
    )
}

async fn turn(
    coordinator: &CoordinatorHandle,
    agent: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    let id = coordinator
        .request_agent_turn(
            EventActor::new(ActorKind::User, None),
            agent.to_owned(),
            "read the files",
        )
        .await?;
    super::super::history_tests::settled(coordinator, &id).await
}

fn recorded(path: &Path) -> Result<Vec<RuntimeReminderEvent>, Box<dyn std::error::Error>> {
    Ok(crate::store::read_events(path)?
        .into_iter()
        .filter_map(|event| match event.payload {
            EventV1::RuntimeReminder(reminder)
                if reminder.kind == RuntimeReminderKind::DirectoryInstructions =>
            {
                Some(reminder)
            }
            _ => None,
        })
        .collect())
}

#[tokio::test]
async fn directory_instructions_reach_provider_once_survive_resume_and_rearm_after_compaction(
) -> Result<(), Box<dyn std::error::Error>> {
    for enabled in [true, false] {
        let temp = tempfile::tempdir()?;
        let root = temp.path();
        std::fs::create_dir(root.join("sub"))?;
        std::fs::write(root.join("AGENTS.md"), "STARTUP_ONLY")?;
        std::fs::write(root.join("sub/AGENTS.md"), "SUB_INITIAL")?;
        std::fs::write(root.join("sub/file"), "data")?;
        let mut batch = touch("first", json!(["sub/file"]));
        let _ = batch.pop();
        batch.extend(touch("concurrent", json!(["sub/file"])));
        let provider = Arc::new(MockProvider::script([
            batch,
            touch("repeat", json!(["sub/file"])),
            super::super::compaction_tests::answer("old answer ".repeat(1000)),
            touch("resumed", json!(["sub/file"])),
            super::super::compaction_tests::answer("recent answer ".repeat(1000)),
            super::super::compaction_tests::answer(super::super::compaction_tests::SUMMARY),
            touch("compacted", json!(["sub/file"])),
            super::super::compaction_tests::answer("done"),
        ]));
        let mut config = configuration(root, &provider);
        config.behavior.directory_instructions.enabled = enabled;
        config.instruction_paths = vec![root.join("sub/../AGENTS.md")];
        let coordinator = spawn(config.clone());
        let run = coordinator.start_run("instructions", root).await?;
        let agent = coordinator
            .spawn_agent_idle(
                EventActor::new(ActorKind::Supervisor, None),
                "default",
                None,
            )
            .await?;
        turn(&coordinator, &agent).await?;
        let reminders = recorded(&run.events_path)?;
        assert_eq!(reminders.len(), usize::from(enabled));
        let requests = provider.captured_requests().await;
        assert_eq!(requests.len(), 3);
        for request in &requests[1..] {
            assert_eq!(
                request
                    .messages
                    .iter()
                    .filter(|m| m.content.contains("SUB_INITIAL"))
                    .count(),
                usize::from(enabled)
            );
            assert!(!request
                .messages
                .iter()
                .any(|m| m.content.contains("STARTUP_ONLY")));
        }
        if enabled {
            assert_eq!(
                reminders[0].source.as_deref(),
                root.join("sub/AGENTS.md").canonicalize()?.to_str()
            );
        }
        coordinator.stop_run().await?;
        let resumed = spawn(config);
        resumed
            .resume_run(run.run_id.to_string(), "instructions resumed")
            .await?;
        assert_eq!(provider.call_count(), 3, "resume must not sample");
        std::fs::write(root.join("sub/AGENTS.md"), "SUB_UPDATED")?;
        turn(&resumed, &agent).await?;
        assert_eq!(recorded(&run.events_path)?.len(), usize::from(enabled));
        let requests = provider.captured_requests().await;
        assert!(!requests[4]
            .messages
            .iter()
            .any(|m| m.content.contains("SUB_UPDATED")));
        assert!(matches!(
            resumed
                .compact_agent_context(agent.clone(), None, "manual")
                .await?,
            ManualCompactionOutcome::Compacted { .. }
        ));
        turn(&resumed, &agent).await?;
        assert_eq!(recorded(&run.events_path)?.len(), 2 * usize::from(enabled));
        let requests = provider.captured_requests().await;
        assert_eq!(requests.len(), 8);
        assert_eq!(
            requests[7]
                .messages
                .iter()
                .filter(|m| m.content.contains("SUB_UPDATED"))
                .count(),
            usize::from(enabled)
        );
        assert!(!requests[7]
            .messages
            .iter()
            .any(|m| m.content.contains("STARTUP_ONLY")));
        resumed.stop_run().await?;
    }
    Ok(())
}

#[tokio::test]
async fn directory_instructions_are_ordered_bounded_and_skip_unreadable_or_failed_paths(
) -> Result<(), Box<dyn std::error::Error>> {
    let temp = tempfile::tempdir()?;
    let root = temp.path();
    std::fs::create_dir_all(root.join("sub/deep"))?;
    std::fs::create_dir(root.join("invalid"))?;
    std::fs::create_dir_all(root.join("nonregular/AGENTS.md"))?;
    std::fs::create_dir(root.join("failed"))?;
    std::fs::write(root.join("AGENTS.md"), "ROOT")?;
    std::fs::write(root.join("sub/AGENTS.md"), "SUB")?;
    std::fs::write(root.join("sub/deep/AGENTS.md"), "ABCé remainder")?;
    std::fs::write(root.join("sub/deep/file"), "data")?;
    std::fs::write(root.join("invalid/AGENTS.md"), [0xff])?;
    std::fs::write(root.join("failed/AGENTS.md"), "FAIL")?;
    let provider = Arc::new(MockProvider::script([
        touch("failed", json!(["failed/missing"])),
        touch(
            "directories",
            json!(["sub/deep/file", "invalid", "nonregular"]),
        ),
        super::super::compaction_tests::answer("done"),
    ]));
    let mut config = configuration(root, &provider);
    config.behavior.directory_instructions.max_bytes = 4;
    let coordinator = spawn(config);
    let run = coordinator.start_run("instruction bounds", root).await?;
    let agent = coordinator
        .spawn_agent_idle(
            EventActor::new(ActorKind::Supervisor, None),
            "default",
            None,
        )
        .await?;
    turn(&coordinator, &agent).await?;
    let reminders = recorded(&run.events_path)?;
    let sources: Vec<_> = reminders
        .iter()
        .filter_map(|r| r.source.as_deref())
        .collect();
    let expected: Vec<_> = ["AGENTS.md", "sub/AGENTS.md", "sub/deep/AGENTS.md"]
        .into_iter()
        .map(|path| root.join(path).canonicalize())
        .collect::<Result<_, _>>()?;
    assert_eq!(
        sources,
        expected
            .iter()
            .map(|p| p.to_string_lossy())
            .collect::<Vec<_>>()
    );
    let requests = provider.captured_requests().await;
    assert!(!requests[1]
        .messages
        .iter()
        .any(|m| m.content.contains("FAIL")));
    for reminder in &reminders {
        assert!(requests[2]
            .messages
            .iter()
            .any(|m| m.content == reminder.text));
    }
    assert!(reminders[2].text.contains("ABC"));
    assert!(reminders[2].text.contains("truncated"));
    assert!(!reminders[2].text.contains('é'));
    assert!(!reminders[2].text.contains("remainder"));
    coordinator.stop_run().await?;
    Ok(())
}

#[cfg(unix)]
#[tokio::test]
async fn directory_instructions_never_read_symlink_escapes_or_outside_ancestors(
) -> Result<(), Box<dyn std::error::Error>> {
    let temp = tempfile::tempdir()?;
    let root = temp.path().join("workspace");
    std::fs::create_dir_all(root.join("sub"))?;
    std::fs::write(temp.path().join("AGENTS.md"), "OUTSIDE_PARENT")?;
    std::fs::write(temp.path().join("outside"), "OUTSIDE_CONTENT")?;
    std::os::unix::fs::symlink(temp.path().join("outside"), root.join("sub/AGENTS.md"))?;
    std::fs::write(root.join("sub/file"), "data")?;
    let provider = Arc::new(MockProvider::script([
        touch("inside", json!(["sub/file"])),
        touch("outside", json!([temp.path().join("outside")])),
        super::super::compaction_tests::answer("done"),
    ]));
    let coordinator = spawn(configuration(&root, &provider));
    let run = coordinator
        .start_run("instruction containment", &root)
        .await?;
    let agent = coordinator
        .spawn_agent_idle(
            EventActor::new(ActorKind::Supervisor, None),
            "default",
            None,
        )
        .await?;
    turn(&coordinator, &agent).await?;
    assert!(recorded(&run.events_path)?.is_empty());
    let requests = provider.captured_requests().await;
    assert_eq!(requests.len(), 3);
    assert!(!requests[2]
        .messages
        .iter()
        .any(|m| m.content.contains("OUTSIDE_")));
    coordinator.stop_run().await?;
    Ok(())
}

#[path = "instructions_tests/policy.rs"]
mod policy;

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
            super::super::compaction_tests::answer("discarded"),
            touch("replacement", json!(["sub/file"])),
            super::super::compaction_tests::answer("done"),
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
        super::super::history_tests::settled(&coordinator, &request).await?;
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
        super::super::compaction_tests::answer("child done"),
    ]));
    let mut config = super::super::children_tests::configuration(
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
