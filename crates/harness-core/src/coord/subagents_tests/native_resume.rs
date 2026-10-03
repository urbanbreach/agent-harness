use super::*;

#[tokio::test]
async fn native_resume_retains_committed_raw_state_and_rejects_summary_only_sources(
) -> Result<(), Box<dyn std::error::Error>> {
    let temp = tempfile::tempdir()?;
    std::fs::write(temp.path().join("sample.txt"), "sample")?;
    let arguments = "{ \"path\" : \"sample.txt\" }";
    let provider = Arc::new(MockProvider::script([
        vec![
            Stream::ToolCallComplete {
                tool_call_id: "raw-call".into(),
                function_name: "large".into(),
                arguments_json: arguments.into(),
            },
            Stream::ToolCallComplete {
                tool_call_id: "unknown-call".into(),
                function_name: "missing-tool".into(),
                arguments_json: "{}".into(),
            },
            Stream::ToolCallComplete {
                tool_call_id: "denied-call".into(),
                function_name: "kill_command_or_subagent".into(),
                arguments_json: "{}".into(),
            },
            settled_metadata("first"),
        ],
        vec![
            Stream::TextDelta("child answer".into()),
            Stream::DoneWithMetadata {
                usage: None,
                metadata: Some(ProviderStreamFinishedMetadata {
                    settled_reasoning: Some(vec!["second".into()]),
                    ..Default::default()
                }),
            },
        ],
        vec![
            Stream::TextDelta("continued answer".into()),
            settled_metadata("third"),
        ],
        vec![
            Stream::TextDelta("woken answer".into()),
            settled_metadata("wake"),
        ],
        vec![
            Stream::TextDelta("summary source".into()),
            Stream::Done { usage: None },
        ],
        vec![
            Stream::TextDelta("summary continued".into()),
            settled_metadata("complete new metadata"),
        ],
    ]));
    let mut config = super::super::children_tests::configuration(
        temp.path(),
        Arc::<MockProvider>::clone(&provider),
    );
    Arc::get_mut(&mut config.tool_registry)
        .ok_or("native fixture registry is shared")?
        .register(Arc::new(LargeTool(Arc::new(AtomicUsize::new(0)))));
    let mut root = AgentProfile::fallback("default");
    root.toolset = vec![
        "spawn_subagent".into(),
        "large".into(),
        "send_subagent_message".into(),
    ];
    config.agent_profiles.insert("default".into(), root);
    let coordinator = spawn_coordinator(
        config.clone(),
        Arc::new(FakeClock::new()),
        Arc::new(DefaultRedactor::default()),
    );
    coordinator.start_run("continuation", temp.path()).await?;
    let parent = coordinator
        .spawn_agent(system_actor(), "default", None)
        .await?;
    let actor = EventActor::new(ActorKind::Worker, Some(parent));
    let first = coordinator
        .execute_tool(
            actor.clone(),
            None,
            None,
            "spawn_subagent".into(),
            json!({"prompt":"source","description":"raw source","subagent_type":"native-fixture","background":false}),
        )
        .await?;
    let child = first
        .structured_json
        .as_ref()
        .and_then(|v| v["subagent_id"].as_str())
        .ok_or("child missing")?
        .to_owned();
    let source = coordinator.raw_finalized_state(child.clone()).await?;
    let FinalizedStateResult::Available { state: original } = source else {
        return Err(format!("source unavailable: {source:?}").into());
    };
    assert_eq!(original.usage[1].usage, None);
    assert!(!original.usage[1].usage_complete);
    let history = coordinator.subagent_history().await?;
    let accounting = history.records[&child]
        .accounting
        .as_ref()
        .ok_or("accounting missing")?;
    assert_eq!(accounting.tool_calls, 1, "rejected calls did not execute");
    assert_eq!(accounting.tokens_used, Some(15));
    assert_eq!(accounting.total_tokens_used, None);
    assert!(accounting.output_usage_incomplete);
    for (call_id, error) in [
        ("unknown-call", "unknown tool: missing-tool"),
        (
            "denied-call",
            "permission denied: tool kill_command_or_subagent is not enabled for this agent",
        ),
    ] {
        let item = original
            .conversation_items
            .iter()
            .find(|item| item.message.tool_call_id.as_deref() == Some(call_id))
            .ok_or("rejected tool message missing")?;
        let raw = item.raw_tool_result.as_ref().ok_or("raw error missing")?;
        assert_eq!(raw.provider_tool_call_id.as_deref(), Some(call_id));
        assert_eq!(raw.output, None);
        assert_eq!(raw.error.as_deref(), Some(error));
        assert_eq!(item.message.content, format!("Tool error: {error}"));
    }
    let resumed = coordinator
        .execute_tool(
            actor.clone(),
            None,
            None,
            "spawn_subagent".into(),
            json!({"resume_from":child,"prompt":"continue","description":"raw resume","background":false}),
        )
        .await?;
    let resumed_id = resumed
        .structured_json
        .as_ref()
        .and_then(|value| value["subagent_id"].as_str())
        .ok_or("resumed id absent")?;
    assert_ne!(resumed_id, child);
    let FinalizedStateResult::Available { state: continued } =
        coordinator.raw_finalized_state(resumed_id).await?
    else {
        return Err("exact continuation unavailable".into());
    };
    assert_eq!(
        &continued.conversation_items[..original.conversation_items.len()],
        original.conversation_items.as_slice()
    );
    assert_eq!(
        &continued.usage[..original.usage.len()],
        original.usage.as_slice()
    );
    let events = crate::store::read_events(&coordinator.run_info().await?.events_path)?;
    let historical = super::super::history::messages(
        &events,
        &continued.owner_agent_id.0,
        false,
        "",
        temp.path(),
    )?;
    assert_eq!(
        historical.unavailable,
        Some(FinalizedStateUnavailable::LegacySummaryOnly),
        "presentation reconstruction is intrinsically summary-only"
    );
    assert_eq!(
        coordinator.subagent_history().await?.records[resumed_id]
            .accounting
            .as_ref()
            .map(|a| a.tool_calls),
        Some(0)
    );
    wake_and_check_tool_count(&coordinator, &actor, &child).await?;
    let first = coordinator
        .execute_tool(
            actor.clone(),
            None,
            None,
            "spawn_subagent".into(),
            json!({"prompt":"summary source","description":"summary source","subagent_type":"native-fixture","background":false}),
        )
        .await?;
    let summary_child = first
        .structured_json
        .as_ref()
        .and_then(|v| v["subagent_id"].as_str())
        .ok_or("summary child missing")?
        .to_owned();
    assert!(matches!(
        coordinator
            .raw_finalized_state(summary_child.clone())
            .await?,
        FinalizedStateResult::Unavailable {
            reason: FinalizedStateUnavailable::UnsupportedReasoning
        }
    ));
    let calls = provider.call_count();
    let rejected = coordinator
        .execute_tool(
            actor.clone(),
            None,
            None,
            "spawn_subagent".into(),
            json!({"resume_from":summary_child,"prompt":"summary continuation","description":"reject summary source","background":false}),
        )
        .await;
    assert!(rejected.is_err());
    assert_eq!(provider.call_count(), calls);
    let run = coordinator.run_info().await?;
    coordinator.stop_run().await?;
    let restored = spawn_coordinator(
        config,
        Arc::new(FakeClock::new()),
        Arc::new(DefaultRedactor::default()),
    );
    restored
        .resume_run(run.run_id.to_string(), "restore child counters")
        .await?;
    assert_eq!(provider.call_count(), calls);
    wake_and_check_tool_count(&restored, &actor, &child).await?;
    restored.stop_run().await?;
    Ok(())
}

async fn wake_and_check_tool_count(
    coordinator: &CoordinatorHandle,
    actor: &EventActor,
    child: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    let mut events = coordinator.subscribe_new_events().await?;
    let _ = coordinator
        .execute_tool(
            actor.clone(),
            None,
            None,
            "send_subagent_message".into(),
            json!({"subagent_id":child,"text":"continue source"}),
        )
        .await?;
    tokio::time::timeout(std::time::Duration::from_secs(3), async {
        while let Some(event) = events.next().await {
            if matches!(event?.payload, EventV1::NativeSubagentReceipt(receipt)
                if receipt.child_id == child && receipt.kind == "terminal_published")
            {
                return Ok::<_, Box<dyn std::error::Error>>(());
            }
        }
        Err("child wake did not finish".into())
    })
    .await??;
    assert_eq!(
        coordinator.subagent_history().await?.records[child]
            .accounting
            .as_ref()
            .map(|a| a.tool_calls),
        Some(1),
        "same-identity wakes retain tool calls across turns and restart"
    );
    Ok(())
}
