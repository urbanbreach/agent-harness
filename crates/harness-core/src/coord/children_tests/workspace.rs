use super::*;

struct CheckpointSpawn(public_subagents::WorkspaceCreationCheckpoint);
#[async_trait::async_trait]
impl Tool for CheckpointSpawn {
    fn id(&self) -> &str {
        "spawn_subagent"
    }
    fn parameters_json_schema(&self) -> Value {
        json!({"type":"object"})
    }
    fn capability(&self) -> ToolCapability {
        ToolCapability::SpawnAgent
    }
    fn permission_requests(&self, _: &Value) -> Vec<(String, String)> {
        vec![("spawn_subagent".into(), "*".into())]
    }
    async fn call(&self, context: ToolContext, args: Value) -> Result<ToolResult, ToolError> {
        context
            .coordinator
            .spawn_subagent_at_creation_checkpoint(
                context.actor,
                context.tool_call_id.to_string(),
                serde_json::from_value(args)
                    .map_err(|error| ToolError::InvalidArguments(error.to_string()))?,
                self.0.clone(),
            )
            .await
            .map(Into::into)
            .map_err(Into::into)
    }
}

async fn git(root: &Path, args: &[&str]) -> Result<String, Box<dyn std::error::Error>> {
    let output = tokio::time::timeout(
        std::time::Duration::from_secs(5),
        tokio::process::Command::new("git")
            .current_dir(root)
            .args(args)
            .kill_on_drop(true)
            .output(),
    )
    .await??;
    if !output.status.success() {
        return Err(String::from_utf8_lossy(&output.stderr).into_owned().into());
    }
    Ok(String::from_utf8(output.stdout)?)
}

#[tokio::test]
async fn native_cancel_after_actual_worktree_creation_cleans_only_owned_material(
) -> Result<(), Box<dyn std::error::Error>> {
    let temp = tempfile::tempdir()?;
    let root = temp.path().join("repository");
    std::fs::create_dir(&root)?;
    git(&root, &["init", "--quiet"]).await?;
    std::fs::write(root.join("sample.txt"), "source content")?;
    git(&root, &["add", "sample.txt"]).await?;
    git(
        &root,
        &[
            "-c",
            "user.name=Fixture",
            "-c",
            "user.email=fixture@localhost",
            "commit",
            "--quiet",
            "-m",
            "fixture",
        ],
    )
    .await?;
    let provider = Arc::new(MockProvider::script([done("must not run")]));
    let mut config = configuration(temp.path(), Arc::<MockProvider>::clone(&provider));
    let (entered, mut created) = mpsc::unbounded_channel();
    let checkpoint = public_subagents::WorkspaceCreationCheckpoint {
        entered,
        proceed: Arc::new(tokio::sync::Notify::new()),
    };
    Arc::get_mut(&mut config.tool_registry)
        .ok_or("fixture registry is unexpectedly shared")?
        .register(Arc::new(CheckpointSpawn(checkpoint)));
    let (handle, parent) = start(config, &root).await?;
    let run = handle.run_info().await?;
    let unrelated = run.run_dir.join("worktrees").join("unrelated");
    std::fs::create_dir_all(&unrelated)?;
    std::fs::write(unrelated.join("marker"), "do not remove")?;
    let mut events = handle.subscribe_new_events().await?;
    let mut args = spawn_args(false);
    args["isolation"] = json!("worktree");
    let foreground = launch(&handle, &parent, "spawn_subagent", args);
    let tool = event(&mut events, |event| match &event.payload {
        EventV1::ToolCallRequested(r) if r.tool_id == "spawn_subagent" => {
            Some(r.tool_call_id.to_string())
        }
        _ => None,
    })
    .await?;
    let id = event(&mut events, |event| match &event.payload {
        EventV1::NativeSubagentRegistered(r) => Some(r.child_id.clone()),
        _ => None,
    })
    .await?;
    let owned = tokio::time::timeout(std::time::Duration::from_secs(5), created.recv())
        .await?
        .ok_or("actual worktree creation checkpoint absent")?;
    assert!(owned.join(".git").is_file());
    assert!(git(&root, &["worktree", "list", "--porcelain"])
        .await?
        .lines()
        .any(|line| line == format!("worktree {}", owned.display())));
    let mut terminal = handle.subscribe_new_events().await?;
    handle
        .cancel_task(&tool, "cancel exactly after creation")
        .await?;
    assert!(
        tokio::time::timeout(std::time::Duration::from_secs(5), foreground)
            .await??
            .is_err()
    );
    event(&mut terminal, |event| match &event.payload {
        EventV1::NativeSubagentReceipt(r) if r.child_id == id && r.kind == "terminal_published" => {
            Some(())
        }
        _ => None,
    })
    .await?;
    assert_eq!(provider.call_count(), 0);
    assert!(!owned.exists());
    assert!(!git(&root, &["worktree", "list", "--porcelain"])
        .await?
        .lines()
        .any(|line| line == format!("worktree {}", owned.display())));
    assert!(git(
        &root,
        &["for-each-ref", &format!("refs/harness/subagents/{id}")]
    )
    .await?
    .is_empty());
    assert_eq!(std::fs::read(unrelated.join("marker"))?, b"do not remove");
    assert_eq!(std::fs::read(root.join("sample.txt"))?, b"source content");
    handle.stop_run().await?;
    Ok(())
}

struct InventoryTool {
    id: &'static str,
    capability: ToolCapability,
}
#[async_trait::async_trait]
impl Tool for InventoryTool {
    fn id(&self) -> &str {
        self.id
    }
    fn parameters_json_schema(&self) -> Value {
        json!({"type":"object"})
    }
    fn capability(&self) -> ToolCapability {
        self.capability
    }
    async fn call(&self, _: ToolContext, _: Value) -> Result<ToolResult, ToolError> {
        Ok(ToolResult::text("inventory fixture"))
    }
}

#[tokio::test]
async fn native_filters_use_registered_capabilities_aliases_and_mcp_server_metadata(
) -> Result<(), Box<dyn std::error::Error>> {
    let temp = tempfile::tempdir()?;
    let provider = Arc::new(MockProvider::script([done("filtered")]));
    let mut config = configuration(temp.path(), Arc::<MockProvider>::clone(&provider));
    let registry =
        Arc::get_mut(&mut config.tool_registry).ok_or("fixture registry is unexpectedly shared")?;
    for (id, capability) in [
        ("write", ToolCapability::EditFs),
        ("edit", ToolCapability::EditFs),
        ("custom-edit", ToolCapability::EditFs),
        ("todowrite", ToolCapability::ReadFs),
        ("task", ToolCapability::SpawnAgent),
        ("get_task_output", ToolCapability::ReadFs),
        ("wait_tasks", ToolCapability::ReadFs),
        ("kill_task", ToolCapability::ReadFs),
        ("mcp.unselected.echo", ToolCapability::ReadFs),
        ("custom-untyped", ToolCapability::ReadFs),
    ] {
        registry.register(Arc::new(InventoryTool { id, capability }));
    }
    config
        .agent_profiles
        .get_mut("default")
        .ok_or("parent fixture profile absent")?
        .toolset = config.tool_registry.tool_ids();
    let definition = config
        .subagent_definitions
        .as_mut()
        .and_then(|defs| defs.cli.get_mut("native-fixture"))
        .ok_or("native definition absent")?;
    definition.capability_mode = Some(crate::config::SubagentCapabilityMode::ReadOnly);
    definition.prompt_body = Some("Plan tool: ${{ tools.by_kind.plan }}\n${% if tools.by_kind.edit %}Unexpected editor${% endif %}".into());
    definition.mcp_inheritance =
        crate::config::SubagentMcpInheritance::Mode(crate::config::SubagentMcpMode::None);
    let (handle, parent) = start(config, temp.path()).await?;
    let _ = join(launch(
        &handle,
        &parent,
        "spawn_subagent",
        spawn_args(false),
    ))
    .await?;
    let requests = provider.captured_requests().await;
    let system = &requests[0].messages[0].content;
    assert!(system.ends_with("Plan tool: todowrite\n"));
    assert!(!system.contains("<making_code_changes>"));
    let tools: Vec<_> = requests
        .first()
        .ok_or("actual provider request absent")?
        .tools
        .as_deref()
        .ok_or("actual provider tool inventory absent")?
        .iter()
        .map(|tool| tool.tool_id.as_str())
        .collect();
    assert!(tools.contains(&"todowrite"));
    assert!(tools.contains(&"custom-untyped"));
    for denied in [
        "write",
        "edit",
        "custom-edit",
        "task",
        "get_task_output",
        "wait_tasks",
        "kill_task",
        "mcp.unselected.echo",
    ] {
        assert!(
            !tools.contains(&denied),
            "registered tool {denied} bypassed its native filter"
        );
    }
    handle.stop_run().await?;
    Ok(())
}
