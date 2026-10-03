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
            settled_metadata("first"),
        ],
        vec![
            Stream::TextDelta("child answer".into()),
            settled_metadata("second"),
        ],
        vec![
            Stream::TextDelta("continued answer".into()),
            settled_metadata("third"),
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
    root.toolset = vec!["spawn_subagent".into(), "large".into()];
    config.agent_profiles.insert("default".into(), root);
    let coordinator = spawn_coordinator(
        config,
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
            actor,
            None,
            None,
            "spawn_subagent".into(),
            json!({"resume_from":summary_child,"prompt":"summary continuation","description":"reject summary source","background":false}),
        )
        .await;
    assert!(rejected.is_err());
    assert_eq!(provider.call_count(), calls);
    coordinator.stop_run().await?;
    Ok(())
}
