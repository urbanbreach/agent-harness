use harness_core::{
    agent::AgentProfile,
    clock::FakeClock,
    config::ShellAllowlist,
    coord::{spawn_coordinator, CoordinatorConfig, CoordinatorHandle},
    event::{ActorKind, EventActor, EventV1},
    file_tag::{FileTagLineRange, FileTagSource, SelectedFileTag, SelectedPromptTags},
    perm::{PermissionAction, PermissionPolicy, PermissionRule},
    redact::DefaultRedactor,
};
use std::{fs, sync::Arc};
use tokio_stream::StreamExt;

async fn finished(
    handle: &CoordinatorHandle,
    id: &str,
) -> Result<bool, Box<dyn std::error::Error>> {
    let mut events = handle.event_store().await?.subscribe(1)?;
    Ok(
        tokio::time::timeout(std::time::Duration::from_secs(3), async {
            while let Some(event) = events.next().await {
                match event?.payload {
                    EventV1::TaskCompleted(e) if e.task_id.as_str() == id => return Ok(true),
                    EventV1::TaskCancelled(e) if e.task_id.as_str() == id => return Ok(false),
                    _ => {}
                }
            }
            Err(harness_core::store::EventStoreError::Invalid(
                "turn never settled",
            ))
        })
        .await??,
    )
}

#[tokio::test]
async fn selected_files_obey_permissions_and_replay_the_selected_content(
) -> Result<(), Box<dyn std::error::Error>> {
    let temp = tempfile::tempdir()?;
    fs::write(
        temp.path().join("sample.txt"),
        "excluded first line\nselected second line\n",
    )?;
    fs::write(temp.path().join("secret.txt"), "PRIVATE FILE CONTENT")?;
    let provider = Arc::new(harness_providers::mock::MockProvider::default());
    let mut config = CoordinatorConfig::new(temp.path().join("sessions"));
    config.provider = Arc::clone(&provider) as Arc<dyn harness_providers::Provider>;
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
    ])?;
    let mut profile = AgentProfile::fallback("default");
    profile.toolset = vec!["read".into()];
    config.agent_profiles.insert("default".into(), profile);
    let handle = spawn_coordinator(
        config.clone(),
        Arc::new(FakeClock::new()),
        Arc::new(DefaultRedactor::default()),
    );
    let run = handle.start_run("prompt", temp.path()).await?;
    let actor = || EventActor::new(ActorKind::User, None);
    let agent = handle.spawn_agent_idle(actor(), "default", None).await?;
    let tags = |path: &str| SelectedPromptTags {
        files: vec![SelectedFileTag {
            path: path.into(),
            filename: path.into(),
            url: String::new(),
            mime: "text/plain".into(),
            source: FileTagSource {
                start: 2,
                end: 3 + path.chars().count(),
                value: format!("@{path}"),
            },
            line_range: Some(FileTagLineRange {
                start: 2,
                end: Some(2),
            }),
        }],
        ..Default::default()
    };
    let id = handle
        .request_agent_turn_with_model_and_selected_tags_and_attachments(
            actor(),
            agent.clone(),
            "先 @sample.txt",
            tags("sample.txt"),
            vec![],
            None,
            None,
        )
        .await?;
    assert!(finished(&handle, &id).await?);
    let mut overlap = tags("sample.txt");
    overlap.files.push(overlap.files[0].clone());
    assert!(handle
        .request_agent_turn_with_model_and_selected_tags_and_attachments(
            actor(),
            agent.clone(),
            "先 @sample.txt",
            overlap,
            vec![],
            None,
            None,
        )
        .await
        .is_err());
    let mut stale = tags("sample.txt");
    stale.files[0].source.end = usize::MAX;
    assert!(handle
        .request_agent_turn_with_model_and_selected_tags_and_attachments(
            actor(),
            agent.clone(),
            "先 @sample.txt",
            stale,
            vec![],
            None,
            None,
        )
        .await
        .is_err());
    let denied = handle
        .request_agent_turn_with_model_and_selected_tags_and_attachments(
            actor(),
            agent.clone(),
            "先 @secret.txt",
            tags("secret.txt"),
            vec![],
            None,
            None,
        )
        .await?;
    assert!(!finished(&handle, &denied).await?);
    assert_eq!(provider.call_count(), 1);
    handle.stop_run().await?;
    fs::write(
        temp.path().join("sample.txt"),
        "changed since the selected turn",
    )?;
    let resumed = spawn_coordinator(
        config,
        Arc::new(FakeClock::new()),
        Arc::new(DefaultRedactor::default()),
    );
    resumed
        .resume_run(run.run_id.to_string(), "continued")
        .await?;
    let id = resumed
        .request_agent_turn(actor(), agent, "Continue")
        .await?;
    assert!(finished(&resumed, &id).await?);
    let requests = provider.captured_requests().await;
    let text = requests
        .last()
        .ok_or("missing provider request")?
        .messages
        .iter()
        .map(|m| m.content.as_str())
        .collect::<Vec<_>>()
        .join("\n");
    assert!(text.contains("selected second line"));
    assert!(
        !text.contains("excluded first line")
            && !text.contains("changed since")
            && !text.contains("PRIVATE FILE CONTENT")
    );
    resumed.stop_run().await?;
    let events = harness_core::store::read_events(&run.events_path)?;
    assert_eq!(
        events
            .iter()
            .filter(|e| matches!(&e.payload, EventV1::ToolCallStarted(_)))
            .count(),
        1
    );
    Ok(())
}
