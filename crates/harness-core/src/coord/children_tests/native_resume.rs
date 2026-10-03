use super::*;

async fn original_reference(
    original: &RawFinalizedState,
    handle: &CoordinatorHandle,
) -> Result<String, Box<dyn std::error::Error>> {
    let events = crate::store::read_events(&handle.run_info().await?.events_path)?;
    events
        .iter()
        .find_map(|event| match &event.payload {
            EventV1::SubagentTransition(t)
                if t.child_id == original.owner_agent_id
                    && t.attempt_id.as_deref() == Some(original.attempt_id.as_str()) =>
            {
                t.finalized_state.as_ref().map(|r| r.artifact_path())
            }
            _ => None,
        })
        .ok_or_else(|| "source reference absent".into())
}

#[tokio::test]
async fn native_new_identity_resume_pins_state_and_same_identity_message_wake(
) -> Result<(), Box<dyn std::error::Error>> {
    let temp = tempfile::tempdir()?;
    let provider = Arc::new(MockProvider::script([
        done("source answer"),
        done("resumed answer"),
        done("awake answer"),
    ]));
    let (handle, parent) = start(
        configuration(temp.path(), Arc::<MockProvider>::clone(&provider)),
        temp.path(),
    )
    .await?;
    let first = join(launch(
        &handle,
        &parent,
        "spawn_subagent",
        spawn_args(false),
    ))
    .await?;
    let id = first
        .structured_json
        .as_ref()
        .and_then(|v| v["subagent_id"].as_str())
        .ok_or("source id absent")?
        .to_owned();
    let FinalizedStateResult::Available { state: original } =
        handle.raw_finalized_state(id.clone()).await?
    else {
        return Err("source raw state unavailable".into());
    };
    let source_bytes = std::fs::read(
        handle
            .run_info()
            .await?
            .run_dir
            .join(handle.subagent_history().await?.finalized[&id].artifact_path()),
    )?;
    let resumed = join(launch(
        &handle,
        &parent,
        "spawn_subagent",
        json!({
            "prompt":"resume work", "description":"resume fixture", "background":false,
            "subagent_type":"caller type ignored", "model":"caller model ignored", "resume_from":id,
        }),
    ))
    .await?;
    let new_id = resumed
        .structured_json
        .as_ref()
        .and_then(|v| v["subagent_id"].as_str())
        .ok_or("resume id absent")?
        .to_owned();
    assert_ne!(new_id, id);
    let FinalizedStateResult::Available { state } =
        handle.raw_finalized_state(new_id.clone()).await?
    else {
        return Err("resumed raw state unavailable".into());
    };
    assert_eq!(
        &state.conversation_items[..original.conversation_items.len()],
        original.conversation_items.as_slice()
    );
    assert_eq!(
        &state.usage[..original.usage.len()],
        original.usage.as_slice()
    );
    assert_eq!(state.source_model, original.source_model);
    assert_eq!(state.execution_context, original.execution_context);
    assert_eq!(
        provider.captured_requests().await[1]
            .context
            .session_id
            .as_deref(),
        Some(new_id.as_str())
    );
    let exited = join(launch(
        &handle,
        &parent,
        "kill_command_or_subagent",
        json!({"task_id":id}),
    ))
    .await?;
    assert_eq!(
        exited
            .structured_json
            .as_ref()
            .and_then(|v| v["Result"]["outcome"].as_str()),
        Some("already_exited")
    );
    let mut terminal = handle.subscribe_new_events().await?;
    let sent = join(launch(
        &handle,
        &parent,
        "send_subagent_message",
        json!({"subagent_id":id,"text":"wake work"}),
    ))
    .await?;
    assert_eq!(
        sent.structured_json
            .as_ref()
            .and_then(|v| v["outcome"].as_str()),
        Some("accepted")
    );
    event(&mut terminal, |event| match &event.payload {
        EventV1::NativeSubagentReceipt(r) if r.child_id == id && r.kind == "terminal_published" => {
            Some(())
        }
        _ => None,
    })
    .await?;
    assert_eq!(
        provider.captured_requests().await[2]
            .context
            .session_id
            .as_deref(),
        Some(id.as_str())
    );
    assert_eq!(provider.captured_requests().await.len(), 3);
    assert_eq!(
        std::fs::read(
            handle
                .run_info()
                .await?
                .run_dir
                .join(original_reference(&original, &handle).await?)
        )?,
        source_bytes
    );
    let history = handle.subagent_history().await?;
    assert_eq!(history.records[&id].generation, 2);
    handle.stop_run().await?;
    Ok(())
}

struct ForkSpawn;
#[async_trait::async_trait]
impl Tool for ForkSpawn {
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
            .spawn_subagent_from_current(
                context.actor,
                context.tool_call_id.to_string(),
                serde_json::from_value(args)
                    .map_err(|error| ToolError::InvalidArguments(error.to_string()))?,
            )
            .await
            .map(Into::into)
            .map_err(Into::into)
    }
}

#[tokio::test]
async fn native_current_parent_fork_uses_actual_prefix_without_missing_tool_results(
) -> Result<(), Box<dyn std::error::Error>> {
    let temp = tempfile::tempdir()?;
    let provider = Arc::new(MockProvider::script([
        vec![
            Stream::ToolCallComplete {
                tool_call_id: "fork-boundary".into(),
                function_name: "spawn_subagent".into(),
                arguments_json: spawn_args(false).to_string(),
            },
            done("")[1].clone(),
        ],
        done("fork answer"),
        done("parent answer"),
    ]));
    let mut config = configuration(temp.path(), Arc::<MockProvider>::clone(&provider));
    Arc::get_mut(&mut config.tool_registry)
        .ok_or("native fixture registry is shared")?
        .register(Arc::new(ForkSpawn));
    let (handle, parent) = start(config, temp.path()).await?;
    let mut events = handle.subscribe_new_events().await?;
    let request = handle
        .request_agent_turn(
            EventActor::new(ActorKind::User, None),
            parent,
            "parent-prefix-marker",
        )
        .await?;
    event(&mut events, |event| match &event.payload {
        EventV1::TaskCompleted(t) if t.task_id.as_str() == request => Some(()),
        _ => None,
    })
    .await?;
    let requests = provider.captured_requests().await;
    assert_eq!(requests.len(), 3);
    let child = &requests[1];
    assert!(child
        .messages
        .iter()
        .any(|message| message.content.contains("parent-prefix-marker")));
    assert!(child.messages.iter().all(|message| {
        message.role != harness_providers::MessageRole::Tool
            && message
                .assistant_tool_calls
                .as_ref()
                .is_none_or(Vec::is_empty)
    }));
    assert!(child
        .messages
        .first()
        .is_some_and(|message| message.content == "current native definition"));
    assert_eq!(
        requests[2]
            .messages
            .iter()
            .filter(|message| {
                message.role == harness_providers::MessageRole::Tool
                    && message.tool_call_id.as_deref() == Some("fork-boundary")
            })
            .count(),
        1
    );
    handle.stop_run().await?;
    Ok(())
}
