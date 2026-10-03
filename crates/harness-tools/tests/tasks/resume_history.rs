use super::*;

#[tokio::test]
async fn new_identity_resume_retains_raw_history_read_state_and_source_without_rerun(
) -> Result<(), Box<dyn std::error::Error>> {
    let temp = tempfile::tempdir()?;
    std::fs::write(temp.path().join("note"), "before\n")?;
    let provider = Arc::new(MockProvider::script([
        vec![
            Stream::ToolCallComplete {
                tool_call_id: "source-read".into(),
                function_name: "read".into(),
                arguments_json: json!({"filePath":"note"}).to_string(),
            },
            done(),
        ],
        vec![Stream::TextDelta("source report".into()), done()],
        vec![
            Stream::ToolCallComplete {
                tool_call_id: "resumed-edit".into(),
                function_name: "edit".into(),
                arguments_json: json!({"filePath":"note","oldString":"before","newString":"after"})
                    .to_string(),
            },
            done(),
        ],
        vec![Stream::TextDelta("resumed report".into()), done()],
    ]));
    let config = config(temp.path(), Arc::<MockProvider>::clone(&provider));
    let coordinator = spawn_coordinator(
        config.clone(),
        Arc::new(FakeClock::new()),
        Arc::new(DefaultRedactor::default()),
    );
    let run = coordinator.start_run("raw resume", temp.path()).await?;
    let parent = coordinator
        .spawn_agent_idle(EventActor::new(ActorKind::User, None), "default", None)
        .await?;
    let actor = EventActor::new(ActorKind::Worker, Some(parent));
    let result = coordinator
        .execute_agent_tool_call(
            actor.clone(),
            None,
            "spawn_subagent",
            json!({"prompt":"Read the note.","description":"Read source","background":"no"}),
        )
        .await?;
    let data = result.structured_json.ok_or("source data")?;
    let child = child_id(&data)?;
    assert_eq!(data["resume_from_hint"], child);
    let source = available(coordinator.raw_finalized_state(child.clone()).await?)?;
    assert!(!source.read_state.is_empty());
    let expected_read_arguments = json!({"filePath":"note"}).to_string();
    assert!(source.conversation_items.iter().any(|item| {
        item.message
            .assistant_tool_calls
            .iter()
            .flatten()
            .any(|call| {
                call.tool_call_id == "source-read" && call.arguments_json == expected_read_arguments
            })
    }));
    coordinator.stop_run().await?;
    let child_events = config.session_dir.join(&child).join("events.jsonl");
    let source_bytes = std::fs::read(&child_events)?;
    let root_bytes = std::fs::read(&run.events_path)?;
    let resumed = spawn_coordinator(
        config,
        Arc::new(FakeClock::new()),
        Arc::new(DefaultRedactor::default()),
    );
    resumed
        .resume_run(run.run_id.to_string(), "restore")
        .await?;
    assert_eq!(
        provider.call_count(),
        2,
        "reload must not execute historical work"
    );
    assert_eq!(std::fs::read(&child_events)?, source_bytes);
    // Starting a live resumed run may append recovery records, but it must
    // preserve the historical prefix and never rewrite the source journal.
    assert!(std::fs::read(&run.events_path)?.starts_with(&root_bytes));
    let result = resumed
        .execute_agent_tool_call(
            actor.clone(),
            None,
            "spawn_subagent",
            json!({"resume_from":child,"subagent_type":"explore","model":"invalid-ignored","prompt":"Edit the note.","description":"Resume source","background":null}),
        )
        .await?;
    let data = result.structured_json.ok_or("resume data")?;
    let next = child_id(&data)?;
    assert_ne!(next, child);
    assert_eq!(data["subagent_type"], "general-purpose");
    assert_eq!(
        std::fs::read_to_string(temp.path().join("note"))?,
        "after\n"
    );
    assert_eq!(std::fs::read(&child_events)?, source_bytes);
    assert_eq!(
        available(resumed.raw_finalized_state(child.clone()).await?)?,
        source
    );
    let requests = provider.captured_requests().await;
    assert_eq!(requests[2].model_id, "chosen");
    assert!(requests[2]
        .messages
        .iter()
        .any(|message| { message.tool_call_id.as_deref() == Some("source-read") }));
    let poll = resumed
        .execute_agent_tool_call(
            actor,
            None,
            "get_command_or_subagent_output",
            json!({"task_ids":[child,next],"timeout_ms":0}),
        )
        .await?;
    let data = poll.structured_json.ok_or("multi output")?;
    assert_eq!(data["MultiResult"]["mode"], "poll");
    assert_eq!(data["MultiResult"]["results"][0]["status"], "completed");
    assert_eq!(data["MultiResult"]["results"][1]["status"], "completed");
    assert_eq!(provider.call_count(), 4);
    resumed.stop_run().await?;
    Ok(())
}
