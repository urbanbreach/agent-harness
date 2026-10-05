use super::*;

#[tokio::test]
#[ignore = "requires native eval worker; scripts/test-lanes.sh eval"]
async fn child_agents_invoke_kernel_tools_without_leaking_them_to_siblings() -> Result {
    let provider = MockProvider::script([
        vec![
            Stream::ToolCallComplete {
                tool_call_id: "lookup-call".into(),
                function_name: "lookup".into(),
                arguments_json: json!({"path":"memo.txt"}).to_string(),
            },
            Stream::Done { usage: None },
        ],
        vec![
            Stream::TextDelta("kernel child done".into()),
            Stream::Done { usage: None },
        ],
    ]);
    let session = Session::new(settings(), provider, None).await?;
    std::fs::write(
        session._root.path().join("memo.txt"),
        "kernel tool reached coordinator read",
    )?;
    let result = session.good("js", "tool(async function lookup(path) { return (await tool.read({path})).text; }); await agent('Read memo.txt with lookup', {tools:['lookup','read']})").await?;
    assert!(
        result.display_text.contains("kernel child done"),
        "{}",
        result.display_text
    );
    let events = harness_core::store::read_events(&session.info.events_path)?;
    let (child, lookup) = events
        .iter()
        .find_map(|event| match &event.payload {
            EventV1::ToolCallRequested(call) if call.tool_id == "lookup" => {
                Some((event.actor.agent_id.clone(), call.tool_call_id.to_string()))
            }
            _ => None,
        })
        .ok_or("child never invoked the kernel tool")?;
    assert!(events.iter().any(|event| matches!(&event.payload, EventV1::ToolCallFinished(call) if call.tool_call_id.as_str() == lookup && call.output_summary.as_deref().is_some_and(|text| text.contains("kernel tool reached coordinator read")))));
    assert!(events.iter().any(|event| matches!(&event.payload, EventV1::ToolCallRequested(call) if call.tool_id == "read" && event.actor.agent_id == child)));
    let sibling = session
        .handle
        .spawn_agent_idle(EventActor::new(ActorKind::User, None), "default", None)
        .await?;
    assert!(matches!(
        session
            .handle
            .execute_agent_tool_call(
                EventActor::new(ActorKind::Worker, Some(sibling)),
                None,
                "lookup",
                json!({"path":"memo.txt"})
            )
            .await,
        Err(error) if error.contains("unknown tool")
    ));
    session.handle.stop_run().await?;
    Ok(())
}
