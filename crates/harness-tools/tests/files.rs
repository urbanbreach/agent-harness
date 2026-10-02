use harness_core::{
    clock::FakeClock,
    config::ShellAllowlist,
    coord::{spawn_coordinator, CoordinatorConfig},
    event::{ActorKind, EventActor, EventV1},
    perm::{PermissionAction, PermissionPolicy, PermissionRule},
    redact::DefaultRedactor,
};
use serde_json::json;
use std::{fs, sync::Arc};

struct HeldPathWrite {
    entered:
        std::sync::Mutex<Option<tokio::sync::oneshot::Sender<harness_core::tool::ToolRunState>>>,
    release: std::sync::Mutex<Option<std::sync::mpsc::Receiver<()>>>,
}
#[async_trait::async_trait]
impl harness_core::tool::Tool for HeldPathWrite {
    fn id(&self) -> &str {
        "held-path-write"
    }
    fn capability(&self) -> harness_core::tool::ToolCapability {
        harness_core::tool::ToolCapability::EditFs
    }
    fn parameters_json_schema(&self) -> serde_json::Value {
        json!({"type":"object"})
    }
    fn filesystem_paths(
        &self,
        _: &serde_json::Value,
    ) -> Result<Vec<std::path::PathBuf>, harness_core::tool::ToolError> {
        Ok(vec!["blocked.txt".into()])
    }
    async fn call(
        &self,
        context: harness_core::tool::ToolContext,
        _: serde_json::Value,
    ) -> Result<harness_core::tool::ToolResult, harness_core::tool::ToolError> {
        use harness_core::tool::{ToolError, ToolResult};
        let entered = self
            .entered
            .lock()
            .map_err(|_| ToolError::Execution("barrier lock failed".into()))?
            .take()
            .ok_or_else(|| ToolError::Execution("barrier already used".into()))?;
        // Move the receiver into the worker so no async task holds a blocking mutex.
        let release = self
            .release
            .lock()
            .map_err(|_| ToolError::Execution("barrier lock failed".into()))?
            .take()
            .ok_or_else(|| ToolError::Execution("barrier already used".into()))?;
        tokio::task::spawn_blocking(move || {
            let path = context.resolve_workspace_path(std::path::Path::new("blocked.txt"))?;
            context.tool_state.edit(&path, |previous| {
                let before = fs::read(&path)?;
                if previous != Some(blake3::hash(&before).to_hex().as_str()) {
                    return Err(ToolError::Execution(
                        "held write lacks a current read".into(),
                    ));
                }
                entered
                    .send(context.tool_state.clone())
                    .map_err(|_| ToolError::Execution("barrier observer closed".into()))?;
                release
                    .recv_timeout(std::time::Duration::from_secs(5))
                    .map_err(|_| ToolError::Execution("held write was not released".into()))?;
                let executor = tokio::runtime::Handle::current();
                let digest = blake3::hash(b"held\n").to_hex().to_string();
                let receipt = executor
                    .block_on(context.coordinator.begin_tool_edit(
                        context.tool_call_id.to_string(),
                        path.clone(),
                        "replace blocked fixture".into(),
                        Some(digest.clone()),
                    ))
                    .map_err(|e| ToolError::Execution(e.to_string()))?;
                let written = fs::write(&path, b"held\n");
                executor
                    .block_on(
                        context.coordinator.finish_tool_edit(
                            context.tool_call_id.to_string(),
                            receipt.edit_id,
                            written
                                .as_ref()
                                .map(|()| digest.clone())
                                .map_err(ToString::to_string),
                        ),
                    )
                    .map_err(|e| ToolError::Execution(e.to_string()))?;
                written?;
                Ok((ToolResult::text("held write completed"), digest))
            })
        })
        .await
        .map_err(|_| ToolError::Execution("held writer stopped".into()))?
    }
}

#[tokio::test]
async fn finalized_actor_reads_copy_privately_but_native_edits_share_serialization(
) -> Result<(), Box<dyn std::error::Error>> {
    use harness_core::{
        agent::AgentProfile,
        subagent::{FinalizedContextCopy, FinalizedStateResult},
    };
    use harness_providers::{
        mock::MockProvider, CompletionUsage, ProviderStreamEvent as Stream,
        ProviderStreamFinishedMetadata,
    };
    use tokio_stream::StreamExt;
    let temp = tempfile::tempdir()?;
    let root = temp.path().join("workspace");
    let cwd = root.join("child");
    fs::create_dir_all(&cwd)?;
    fs::write(cwd.join("sample.txt"), "alpha\n")?;
    fs::write(cwd.join("blocked.txt"), "alpha\n")?;
    fs::write(cwd.join("independent.txt"), "alpha\n")?;
    fs::write(root.join("sample.txt"), "root must stay unchanged\n")?;
    let done = || Stream::DoneWithMetadata {
        usage: Some(CompletionUsage {
            prompt_tokens: 10,
            completion_tokens: 2,
            total_tokens: 12,
        }),
        metadata: Some(ProviderStreamFinishedMetadata {
            settled_reasoning: Some(Vec::new()),
            usage_complete: Some(true),
            ..Default::default()
        }),
    };
    let raw_arguments = "{ \"path\" : \"sample.txt\" }";
    let provider = Arc::new(MockProvider::script([
        vec![
            Stream::ToolCallComplete {
                tool_call_id: "native-read-call".into(),
                function_name: "read".into(),
                arguments_json: raw_arguments.into(),
            },
            done(),
        ],
        vec![Stream::TextDelta("finished source".into()), done()],
        vec![Stream::TextDelta("finished copy".into()), done()],
    ]));
    let mut config = CoordinatorConfig::new(temp.path().join("sessions"));
    config.provider = Arc::clone(&provider) as Arc<dyn harness_providers::Provider>;
    let (entered, entered_event) = tokio::sync::oneshot::channel();
    let (release, release_event) = std::sync::mpsc::channel();
    let mut registry = harness_tools::coordinator_registry(ShellAllowlist::default());
    registry.register(Arc::new(HeldPathWrite {
        entered: std::sync::Mutex::new(Some(entered)),
        release: std::sync::Mutex::new(Some(release_event)),
    }));
    config.tool_registry = Arc::new(registry);
    config.permission_policy = PermissionPolicy::allow_all();
    let mut profile = AgentProfile::fallback("child");
    profile.toolset = vec!["read".into(), "edit".into(), "held-path-write".into()];
    config.agent_profiles.insert("child".into(), profile);
    let coordinator = spawn_coordinator(
        config,
        Arc::new(FakeClock::new()),
        Arc::new(DefaultRedactor::default()),
    );
    let run = coordinator.start_run("actors", &root).await?;
    let owner = EventActor::new(ActorKind::Supervisor, None);
    let parent = coordinator
        .spawn_agent(owner.clone(), "default", None)
        .await?;
    let source = coordinator
        .spawn_agent(owner.clone(), "child", Some(parent.clone()))
        .await?;
    let target = coordinator
        .spawn_agent(owner.clone(), "child", Some(parent.clone()))
        .await?;
    let fresh = coordinator
        .spawn_agent(owner.clone(), "child", Some(parent))
        .await?;
    coordinator
        .set_agent_execution_cwd(owner.clone(), source.clone(), cwd.clone())
        .await?;
    coordinator
        .set_agent_execution_cwd(owner.clone(), fresh.clone(), cwd.clone())
        .await?;
    let mut events = coordinator.subscribe_new_events().await?;
    let turn = coordinator
        .request_agent_turn(owner.clone(), source.clone(), "read file")
        .await?;
    tokio::time::timeout(std::time::Duration::from_secs(5), async {
        while let Some(event) = events.next().await {
            match event?.payload {
                EventV1::TaskCompleted(e) if e.task_id.as_str() == turn => return Ok(()),
                EventV1::TaskCancelled(e) if e.task_id.as_str() == turn => {
                    return Err("source turn failed".into())
                }
                _ => {}
            }
        }
        Err::<(), Box<dyn std::error::Error>>("source terminal missing".into())
    })
    .await??;
    let FinalizedStateResult::Available { state } =
        coordinator.raw_finalized_state(source.clone()).await?
    else {
        return Err("native source state unavailable".into());
    };
    assert_eq!(state.execution_context.effective_cwd, cwd.to_string_lossy());
    assert_eq!(
        state.read_state[&cwd.join("sample.txt")],
        blake3::hash(b"alpha\n").to_hex().as_str()
    );
    assert_eq!(
        state.conversation_items[1]
            .message
            .assistant_tool_calls
            .as_ref()
            .ok_or("calls missing")?[0]
            .arguments_json,
        raw_arguments
    );
    let edit = |new: &str| json!({"path":"sample.txt","oldString":"alpha","newString":new});
    let actor = |id: &str| EventActor::new(ActorKind::Worker, Some(id.into()));
    assert!(coordinator
        .execute_agent_tool_call(actor(&fresh), None, "edit", edit("fresh"))
        .await
        .is_err_and(|e| e.contains("unread")));
    coordinator
        .initialize_agent_from_finalized(
            owner.clone(),
            target.clone(),
            source.clone(),
            FinalizedContextCopy::Resume,
        )
        .await?;
    let mut events = coordinator.subscribe_new_events().await?;
    let turn = coordinator
        .request_agent_turn(owner.clone(), target.clone(), "copied prompt")
        .await?;
    tokio::time::timeout(std::time::Duration::from_secs(5), async {
        while let Some(event) = events.next().await {
            if matches!(event?.payload, EventV1::TaskCompleted(e) if e.task_id.as_str() == turn) {
                return Ok::<_, harness_core::store::EventStoreError>(());
            }
        }
        Err(harness_core::store::EventStoreError::Invalid(
            "copy terminal missing",
        ))
    })
    .await??;
    let requests = provider.captured_requests().await;
    let request = requests.last().ok_or("copied provider request missing")?;
    assert!(request
        .messages
        .iter()
        .any(|m| m.content == "finished source"));
    assert!(request
        .messages
        .iter()
        .any(|m| m.tool_call_id.as_deref() == Some("native-read-call")));
    for id in [&source, &target] {
        coordinator
            .execute_agent_tool_call(actor(id), None, "read", json!({"path":"blocked.txt"}))
            .await?;
    }
    coordinator
        .execute_agent_tool_call(
            actor(&target),
            None,
            "read",
            json!({"path":"independent.txt"}),
        )
        .await?;
    let held_handle = coordinator.clone();
    let held_actor = actor(&source);
    let held = tokio::spawn(async move {
        held_handle
            .execute_agent_tool_call(held_actor, None, "held-path-write", json!({}))
            .await
    });
    let state = tokio::time::timeout(std::time::Duration::from_secs(5), entered_event).await??;
    let mut write_locks = state.subscribe_write_locks()?;
    let competing_handle = coordinator.clone();
    let competing_actor = actor(&target);
    let competing = tokio::spawn(async move {
        competing_handle
            .execute_agent_tool_call(
                competing_actor,
                None,
                "edit",
                json!({"path":"./blocked.txt","oldString":"alpha","newString":"racing"}),
            )
            .await
    });
    let contested = tokio::time::timeout(std::time::Duration::from_secs(5), async {
        loop {
            let (path, contended) = write_locks.recv().await?;
            if path == cwd.join("blocked.txt") {
                return Ok::<_, tokio::sync::broadcast::error::RecvError>(contended);
            }
        }
    })
    .await;
    let independent = tokio::time::timeout(
        std::time::Duration::from_secs(3),
        coordinator.execute_agent_tool_call(
            actor(&target),
            None,
            "edit",
            json!({"path":"independent.txt","oldString":"alpha","newString":"independent"}),
        ),
    )
    .await;
    // Release before propagating failures so the fixture always cleans up its held writer.
    release.send(())?;
    tokio::time::timeout(std::time::Duration::from_secs(5), held).await???;
    assert!(
        contested??,
        "the actual native edit must reach the held canonical write lock"
    );
    independent??;
    assert!(
        tokio::time::timeout(std::time::Duration::from_secs(5), competing)
            .await??
            .is_err_and(|e| e.contains("has changed"))
    );
    assert_eq!(fs::read_to_string(cwd.join("blocked.txt"))?, "held\n");
    assert_eq!(
        fs::read_to_string(cwd.join("independent.txt"))?,
        "independent\n"
    );
    let (first, second) = tokio::join!(
        coordinator.execute_agent_tool_call(actor(&source), None, "edit", edit("source")),
        coordinator.execute_agent_tool_call(actor(&target), None, "edit", edit("target")),
    );
    assert_ne!(
        first.is_ok(),
        second.is_ok(),
        "copied reads must share write serialization"
    );
    let loser = if first.is_err() { &source } else { &target };
    coordinator
        .execute_agent_tool_call(actor(loser), None, "read", json!({"path":"sample.txt"}))
        .await?;
    let winner = if first.is_ok() { &source } else { &target };
    // A read in the other owner must not refresh this owner's recorded digest.
    fs::write(cwd.join("sample.txt"), "external\n")?;
    coordinator
        .execute_agent_tool_call(actor(loser), None, "read", json!({"path":"sample.txt"}))
        .await?;
    assert!(coordinator
        .execute_agent_tool_call(
            actor(winner),
            None,
            "edit",
            json!({"path":"sample.txt","oldString":"external","newString":"bad"})
        )
        .await
        .is_err_and(|e| e.contains("has changed")));
    assert_eq!(
        fs::read_to_string(root.join("sample.txt"))?,
        "root must stay unchanged\n"
    );
    assert!(coordinator
        .set_agent_execution_cwd(owner, fresh, temp.path().into())
        .await
        .is_err());
    coordinator.stop_run().await?;
    assert!(!fs::read_to_string(run.events_path)?.contains("settled_reasoning"));
    Ok(())
}

#[tokio::test]
async fn file_edits_require_current_reads_and_obey_external_path_policy(
) -> Result<(), Box<dyn std::error::Error>> {
    let temp = tempfile::tempdir()?;
    let root = temp.path().join("workspace");
    fs::create_dir(&root)?;
    let file = root.join("sample.txt");
    fs::write(&file, "alpha\r\nbeta\r\n")?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&file, fs::Permissions::from_mode(0o755))?;
    }
    let outside = temp.path().join("outside.txt");
    fs::write(&outside, "outside")?;
    let mut config = CoordinatorConfig::new(temp.path().join("sessions"));
    config.tool_registry = Arc::new(harness_tools::coordinator_registry(
        ShellAllowlist::default(),
    ));
    config.permission_policy = PermissionPolicy::from_rules(vec![
        PermissionRule {
            permission: "*".into(),
            pattern: "*".into(),
            action: PermissionAction::Allow,
        },
        PermissionRule {
            permission: "read".into(),
            pattern: "secret.txt".into(),
            action: PermissionAction::Deny,
        },
        PermissionRule {
            permission: "external_directory".into(),
            pattern: "*".into(),
            action: PermissionAction::Deny,
        },
    ])?;
    let coordinator = spawn_coordinator(
        config.clone(),
        Arc::new(FakeClock::new()),
        Arc::new(DefaultRedactor::default()),
    );
    let run = coordinator.start_run("files", &root).await?;
    let actor = || EventActor::new(ActorKind::User, None);
    use base64::Engine;
    let encoded = "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAIAAACQd1PeAAAADElEQVR4nGP4z8AAAAMBAQDJ/pLvAAAAAElFTkSuQmCC";
    let png = base64::engine::general_purpose::STANDARD.decode(encoded)?;
    fs::write(root.join("picture.png"), &png)?;
    let picture = coordinator
        .execute_agent_tool_call(actor(), None, "read", json!({"path":"picture.png"}))
        .await?;
    assert_eq!(picture.attachments.len(), 1);
    assert_eq!(picture.attachments[0].mime, "image/png");
    assert_eq!(picture.attachments[0].bytes()?, png);
    let pdf = b"%PDF-1.7\nPDF_BODY_NOT_JOURNALED\n%%EOF\n";
    fs::write(root.join("document.pdf"), pdf)?;
    let document = coordinator
        .execute_agent_tool_call(actor(), None, "read", json!({"path":"document.pdf"}))
        .await?;
    assert!(document.attachments.is_empty());
    assert!(document
        .display_text
        .contains("contents were not extracted"));
    let artifact = document.artifacts.first().ok_or("missing PDF artifact")?;
    assert!(artifact.path.ends_with(".pdf"));
    let retained = run.run_dir.join(&artifact.path);
    assert_eq!(fs::read(&retained)?, pdf);
    assert_eq!(artifact.digest, blake3::hash(pdf).to_hex().as_str());
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(fs::metadata(&retained)?.permissions().mode() & 0o777, 0o600);
    }
    let before = fs::read_dir(&run.artifacts_dir)?.count();
    fs::write(
        root.join("document.pdf"),
        b"%PDF-1.7\nsk-private-pdf-credential\n%%EOF",
    )?;
    assert!(coordinator
        .execute_agent_tool_call(actor(), None, "read", json!({"path":"document.pdf"}))
        .await
        .is_err_and(|error| error.contains("credential")));
    assert_eq!(fs::read_dir(&run.artifacts_dir)?.count(), before);
    assert!(!fs::read_to_string(&run.events_path)?.contains("PDF_BODY_NOT_JOURNALED"));
    let images = coordinator
        .execute_agent_tool_call(
            actor(),
            None,
            "batch",
            json!({"tool_calls":[
                {"tool":"read","parameters":{"path":"picture.png"}},
                {"tool":"read","parameters":{"path":"picture.png"}},
            ]}),
        )
        .await?;
    assert_eq!(images.attachments.len(), 2, "batch must keep nested media");
    assert_ne!(images.attachments[0].id, images.attachments[1].id);
    let too_many = vec![json!({"tool":"read","parameters":{"path":"picture.png"}}); 17];
    assert!(coordinator
        .execute_agent_tool_call(actor(), None, "batch", json!({"tool_calls":too_many}))
        .await
        .is_err_and(|e| e.contains("attachment limit")));
    assert!(!std::fs::read_to_string(&run.events_path)?.contains(encoded));
    assert!(
        coordinator
            .execute_agent_tool_call(
                actor(),
                None,
                "write",
                json!({"path":".agent-harness/permission-grants.json","content":"{}"})
            )
            .await
            .is_err(),
        "tools must not rewrite their persistent permissions"
    );
    assert!(!root.join(".agent-harness/permission-grants.json").exists());
    let edit = || json!({"filePath":"sample.txt", "oldString":"alpha", "newString":"gamma"});
    assert!(coordinator
        .execute_agent_tool_call(actor(), None, "edit", edit())
        .await
        .is_err());
    let read = coordinator
        .execute_agent_tool_call(actor(), None, "read", json!({"filePath":"sample.txt"}))
        .await?;
    assert!(read.display_text.contains("alpha"));
    coordinator
        .execute_agent_tool_call(actor(), None, "edit", edit())
        .await?;
    assert_eq!(fs::read_to_string(&file)?, "gamma\r\nbeta\r\n");
    let scan = coordinator
        .execute_agent_tool_call(actor(), None, "read", json!({"path":"sample.txt"}))
        .await?;
    let anchors = scan
        .structured_json
        .as_ref()
        .and_then(|v| v.get("anchors"))
        .and_then(serde_json::Value::as_array)
        .ok_or("read did not include line anchors")?;
    let first = format!(
        "1#{}",
        anchors[0]["hash"].as_str().ok_or("missing anchor hash")?
    );
    let second = format!(
        "2#{}",
        anchors[1]["hash"].as_str().ok_or("missing anchor hash")?
    );
    assert!(coordinator
        .execute_agent_tool_call(
            actor(),
            None,
            "edit",
            json!({"path":"sample.txt", "edits":[
                {"op":"replace", "pos":first, "lines":["would change"]},
                {"op":"replace", "pos":"2#00000000", "lines":["stale"]}
            ]})
        )
        .await
        .is_err());
    assert_eq!(
        fs::read_to_string(&file)?,
        "gamma\r\nbeta\r\n",
        "all anchors must be checked before any change"
    );
    assert!(coordinator
        .execute_agent_tool_call(
            actor(),
            None,
            "edit",
            json!({"path":"sample.txt", "edits":[
                {"op":"replace", "pos":second, "end":first, "lines":["reversed"]}
            ]})
        )
        .await
        .is_err());
    coordinator
        .execute_agent_tool_call(
            actor(),
            None,
            "edit",
            json!({"path":"sample.txt", "edits":[
                {"op":"replace", "pos":first, "end":second, "lines":["delta", "epsilon"]}
            ]}),
        )
        .await?;
    assert_eq!(fs::read_to_string(&file)?, "delta\r\nepsilon\r\n");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(fs::metadata(&file)?.permissions().mode() & 0o777, 0o755);
    }
    fs::write(&file, "changed by editor\n")?;
    assert!(coordinator
        .execute_agent_tool_call(
            actor(),
            None,
            "write",
            json!({"filePath":"sample.txt", "content":"overwrite"})
        )
        .await
        .is_err());
    assert_eq!(fs::read_to_string(&file)?, "changed by editor\n");
    coordinator
        .execute_agent_tool_call(
            actor(),
            None,
            "write",
            json!({"filePath":"nested/new.txt", "content":"created\n"}),
        )
        .await?;
    assert_eq!(
        fs::read_to_string(root.join("nested/new.txt"))?,
        "created\n"
    );
    assert!(coordinator
        .execute_agent_tool_call(actor(), None, "read", json!({"filePath":"../outside.txt"}))
        .await
        .is_err());
    #[cfg(unix)]
    {
        fs::write(root.join("secret.txt"), "private data")?;
        std::os::unix::fs::symlink(root.join("secret.txt"), root.join("alias"))?;
        assert!(coordinator
            .execute_agent_tool_call(actor(), None, "read", json!({"filePath":"alias"}))
            .await
            .is_err());
        std::os::unix::fs::symlink(&outside, root.join("link"))?;
        assert!(coordinator
            .execute_agent_tool_call(
                actor(),
                None,
                "write",
                json!({"filePath":"link", "content":"escape"})
            )
            .await
            .is_err());
    }
    assert_eq!(fs::read_to_string(outside)?, "outside");
    coordinator
        .execute_agent_tool_call(
            actor(),
            None,
            "edit",
            json!({"path":"created-by-edit.txt", "oldString":"", "newString":"created\n"}),
        )
        .await?;
    assert_eq!(
        fs::read_to_string(root.join("created-by-edit.txt"))?,
        "created\n"
    );
    assert!(coordinator
        .execute_agent_tool_call(
            actor(),
            None,
            "edit",
            json!({"path":"created-by-edit.txt", "oldString":"", "newString":"overwrite"})
        )
        .await
        .is_err());
    let snapshots: Vec<_> = harness_core::store::read_events(&run.events_path)?
        .into_iter()
        .filter_map(|e| match e.payload {
            EventV1::WorkspaceSnapshot(snapshot) => Some(snapshot.request_id.to_string()),
            _ => None,
        })
        .collect();
    assert_eq!(snapshots.len(), 4);
    let removed = coordinator.revert_workspace(snapshots[3].clone()).await?;
    assert_eq!(removed.removed_paths, ["created-by-edit.txt"]);
    assert!(!root.join("created-by-edit.txt").exists());
    assert!(coordinator
        .revert_workspace(snapshots[1].clone())
        .await
        .is_err());
    assert_eq!(fs::read_to_string(&file)?, "changed by editor\n");
    fs::write(&file, "delta\r\nepsilon\r\n")?;
    let restored = coordinator.revert_workspace(snapshots[1].clone()).await?;
    assert_eq!(restored.restored_paths, ["sample.txt"]);
    assert_eq!(fs::read_to_string(&file)?, "gamma\r\nbeta\r\n");
    assert!(coordinator
        .revert_workspace(snapshots[1].clone())
        .await?
        .restored_paths
        .is_empty());
    coordinator.stop_run().await?;
    let resumed = spawn_coordinator(
        config,
        Arc::new(FakeClock::new()),
        Arc::new(DefaultRedactor::default()),
    );
    resumed
        .resume_run(run.run_id.to_string(), "files resumed")
        .await?;
    resumed.revert_workspace(snapshots[0].clone()).await?;
    assert_eq!(fs::read_to_string(&file)?, "alpha\r\nbeta\r\n");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(fs::metadata(&file)?.permissions().mode() & 0o777, 0o755);
    }
    resumed.stop_run().await?;
    let events = harness_core::store::read_events(&run.events_path)?;
    let edits: Vec<_> = events
        .iter()
        .filter_map(|event| match &event.payload {
            EventV1::EditApplied(edit) => Some((event.seq, edit)),
            _ => None,
        })
        .collect();
    assert_eq!(edits.len(), 4, "only completed file writes are recorded");
    assert!(
        edits
            .iter()
            .all(|(_, edit)| edit.new_file_digest.len() == 12),
        "edit digests must match the TUI's 12-character content fingerprint"
    );
    let (seq, edit) = edits[0];
    assert!(events.iter().any(|e| e.seq < seq
        && matches!(&e.payload, EventV1::EditProposed(p) if p.edit_id == edit.edit_id)));
    let diff = fs::read_to_string(
        run.run_dir
            .join(edit.diff_rel_path.as_ref().ok_or("missing edit diff")?),
    )?;
    assert!(diff.contains("-alpha") && diff.contains("+gamma"));
    Ok(())
}
