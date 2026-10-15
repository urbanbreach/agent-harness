//! A live context and the one rebuilt from its journal must price and order identically.
use super::*;
use harness_compaction::tokenizer::Counter;
use std::sync::atomic::AtomicUsize;

#[tokio::test]
async fn interleaved_response_blocks_are_identical_live_and_after_rebuild(
) -> Result<(), Box<dyn std::error::Error>> {
    let temp = tempfile::tempdir()?;
    let mut registry = ToolRegistry::new();
    registry.register(Arc::new(super::super::tests::CountTool(Arc::new(
        AtomicUsize::new(0),
    ))));
    let provider = Arc::new(MockProvider::script([
        vec![
            Stream::TextDelta("before ".into()),
            Stream::TextDelta("the call".into()),
            Stream::ReasoningDelta("thinking aloud".into()),
            Stream::ToolCallComplete {
                tool_call_id: "call-1".into(),
                function_name: "count".into(),
                arguments_json: r#"{"intent":"count things"}"#.into(),
            },
            Stream::TextDelta("after the call".into()),
            Stream::DoneWithMetadata {
                usage: None,
                metadata: Some(harness_providers::ProviderStreamFinishedMetadata {
                    settled_reasoning: Some(vec!["thinking aloud".into()]),
                    ..Default::default()
                }),
            },
        ],
        vec![
            Stream::TextDelta("final answer".into()),
            Stream::Done { usage: None },
        ],
    ]));
    let mut config = CoordinatorConfig::new(temp.path().join("sessions"));
    config.provider = Arc::clone(&provider) as Arc<dyn harness_providers::Provider>;
    config.tool_registry = Arc::new(registry);
    config.permission_policy = PermissionPolicy::allow_all();
    let mut profile = crate::agent::AgentProfile::fallback("default");
    profile.toolset = vec!["count".into()];
    config.agent_profiles.insert("default".into(), profile);
    let coordinator = spawn_coordinator(
        config,
        Arc::new(FakeClock::new()),
        Arc::new(DefaultRedactor::default()),
    );
    let run = coordinator.start_run("live parity", temp.path()).await?;
    let agent = coordinator
        .spawn_agent_idle(
            EventActor::new(ActorKind::Supervisor, None),
            "default",
            None,
        )
        .await?;
    let turn = coordinator
        .request_agent_turn(EventActor::new(ActorKind::User, None), agent.clone(), "go")
        .await?;
    settled(&coordinator, &turn).await?;
    let live = coordinator
        .call({
            let agent = agent.clone();
            move |s| Ok(s.agents[&agent].messages.clone())
        })
        .await?;
    let events = crate::store::read_events(&run.events_path)?;
    let rebuilt = history::messages(&events, &agent, true, "", &run.run_dir)?;
    let counter = Counter::new(None);
    // Reasoning is not durable, so the restored side never has it.
    let mut stripped = live.clone();
    for entry in &mut stripped.entries {
        entry.settled_reasoning.clear();
        entry
            .parts
            .retain(|part| !matches!(part, harness_compaction::ItemPart::Reasoning(_)));
        entry.version = entry.version.wrapping_add(1);
    }
    let shape = |context: &super::super::context::Context| {
        context
            .items_with(&counter)
            .iter()
            .map(|item| {
                (
                    item.message.role,
                    item.message.content.clone(),
                    format!("{:?}", item.parts),
                    (item.tokens, item.tokens_floor),
                    item.stop,
                    format!("{:?}/{:?}", item.origin, item.note_kind),
                )
            })
            .collect::<Vec<_>>()
    };
    let shape_tokens = |context: &super::super::context::Context| {
        context
            .items_with(&counter)
            .iter()
            .map(|item| (item.tokens, item.tokens_floor))
            .collect::<Vec<_>>()
    };
    let assistant = live
        .entries
        .iter()
        .find(|entry| !entry.parts.is_empty())
        .ok_or("live assistant has no ordered parts")?;
    assert!(matches!(
        assistant.parts.as_slice(),
        [
            harness_compaction::ItemPart::Text(_),
            harness_compaction::ItemPart::Reasoning(thought),
            harness_compaction::ItemPart::Call(0),
            harness_compaction::ItemPart::Text(_),
        ] if thought == "thinking aloud"
    ));
    assert_eq!(shape(&stripped), shape(&rebuilt));
    // Ordered and flat representations price the same, and the archive keeps the thinking.
    let mut flat = live.clone();
    for entry in &mut flat.entries {
        entry.parts.clear();
        entry.version = entry.version.wrapping_add(1);
    }
    assert_eq!(shape_tokens(&live), shape_tokens(&flat));
    let transcript = harness_compaction::archive::serialize_for_archive(
        &live.items_with(&counter),
        &harness_compaction::archive::SerializeOptions::default(),
    );
    assert!(transcript.contains("¶think:thinking aloud"), "{transcript}");
    let hidden = harness_compaction::archive::serialize_for_archive(
        &live.items_with(&counter),
        &harness_compaction::archive::SerializeOptions {
            include_thinking: false,
            ..Default::default()
        },
    );
    assert!(!hidden.contains("thinking aloud"));
    Ok(())
}
