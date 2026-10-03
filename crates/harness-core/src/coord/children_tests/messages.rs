use super::*;

#[tokio::test]
async fn native_messages_steer_current_request_interject_first_and_queue_later(
) -> Result<(), Box<dyn std::error::Error>> {
    let temp = tempfile::tempdir()?;
    let (requests, mut received) = mpsc::unbounded_channel();
    let provider = Arc::new(Gate {
        requests,
        permits: tokio::sync::Semaphore::new(0),
        responses: MockProvider::script([
            vec![
                Stream::ToolCallComplete {
                    tool_call_id: "inspect".into(),
                    function_name: "get_command_or_subagent_output".into(),
                    arguments_json: "{\"task_ids\":[\"missing\"]}".into(),
                },
                done("")[1].clone(),
            ],
            done("steered"),
            done("queued"),
        ]),
    });
    let (handle, parent) = start(
        configuration(temp.path(), Arc::<Gate>::clone(&provider)),
        temp.path(),
    )
    .await?;
    let mut events = handle.subscribe_new_events().await?;
    let background = launch(&handle, &parent, "spawn_subagent", spawn_args(true));
    let id = event(&mut events, |event| match &event.payload {
        EventV1::NativeSubagentRegistered(r) => Some(r.child_id.clone()),
        _ => None,
    })
    .await?;
    let _ = join(background).await?;
    let _ = tokio::time::timeout(std::time::Duration::from_secs(5), received.recv()).await?;
    for (delivery, text) in [
        ("steer", "steer marker"),
        ("interject", "interject marker"),
        ("queue", "queue marker"),
    ] {
        let output = join(launch(
            &handle,
            &parent,
            "send_subagent_message",
            json!({"subagent_id":id,"text":text,"delivery":delivery}),
        ))
        .await?;
        assert_eq!(
            output
                .structured_json
                .as_ref()
                .and_then(|v| v["outcome"].as_str()),
            Some("accepted")
        );
    }
    provider.permits.add_permits(1);
    let current = tokio::time::timeout(std::time::Duration::from_secs(5), received.recv())
        .await?
        .ok_or("steered request absent")?;
    let interject = current
        .messages
        .iter()
        .position(|m| m.content.contains("interject marker"))
        .ok_or("interject not in active request")?;
    let steer = current
        .messages
        .iter()
        .position(|m| m.content.contains("steer marker"))
        .ok_or("steer not in active request")?;
    assert!(interject < steer);
    assert!(!current
        .messages
        .iter()
        .any(|m| m.content.contains("queue marker")));
    provider.permits.add_permits(1);
    let queued = tokio::time::timeout(std::time::Duration::from_secs(5), received.recv())
        .await?
        .ok_or("queued request absent")?;
    assert!(queued
        .messages
        .last()
        .is_some_and(|m| m.content.contains("queue marker")));
    let mut terminal = handle.subscribe_new_events().await?;
    provider.permits.add_permits(1);
    event(&mut terminal, |event| match &event.payload {
        EventV1::NativeSubagentReceipt(r) if r.child_id == id && r.kind == "terminal_published" => {
            Some(())
        }
        _ => None,
    })
    .await?;
    handle.stop_run().await?;
    Ok(())
}
