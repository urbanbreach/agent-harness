use super::*;
use std::time::Duration;

fn schema() -> Value {
    json!({
        "type": "object",
        "properties": {"answer": {"type": "string"}},
        "required": ["answer"],
        "additionalProperties": false
    })
}

#[tokio::test]
async fn output_contract_corrects_final_answers_and_returns_parsed_objects(
) -> Result<(), Box<dyn std::error::Error>> {
    for answer in [
        r#"{"answer":"ok"}"#,
        "Result:\n```json\n{\"answer\":\"ok\"}\n```",
        "Result: {\"answer\":\"ok\"} finished.",
    ] {
        let temp = tempfile::tempdir()?;
        let provider = Arc::new(MockProvider::script([done("{}"), done(answer)]));
        let mut config = configuration(temp.path(), Arc::<MockProvider>::clone(&provider));
        config.behavior.output_contract.max_retries = 2;
        let (handle, parent) = start(config, temp.path()).await?;
        let run = handle.run_info().await?;
        let mut args = spawn_args(false);
        args["output_schema"] = schema();
        let result = join(launch(&handle, &parent, "spawn_subagent", args)).await?;
        let output = result.structured_json.ok_or("spawn output missing")?;
        assert_eq!(output["structured_output"], json!({"answer":"ok"}));
        assert!(output.get("output_errors").is_none());
        assert_eq!(output["output"], answer);
        let id = output["subagent_id"].as_str().ok_or("child id absent")?;
        let events = crate::store::read_events(&run.events_path)?;
        let reminders: Vec<_> = events
            .iter()
            .filter_map(|event| match &event.payload {
                EventV1::RuntimeReminder(reminder)
                    if reminder.kind == RuntimeReminderKind::OutputContract =>
                {
                    Some((event, reminder))
                }
                _ => None,
            })
            .collect();
        assert_eq!(reminders.len(), 1);
        assert_eq!(reminders[0].0.actor.agent_id.as_deref(), Some(id));
        let requests = provider.captured_requests().await;
        assert_eq!(requests.len(), 2);
        assert!(requests[0]
            .messages
            .iter()
            .any(|message| message.content.contains("additionalProperties")));
        assert!(requests[1]
            .messages
            .iter()
            .any(|message| message.content == reminders[0].1.text));
        let poll = join(launch(
            &handle,
            &parent,
            "get_command_or_subagent_output",
            json!({"task_ids":[id]}),
        ))
        .await?;
        assert_eq!(
            poll.structured_json.ok_or("poll output missing")?["Result"]["structured_output"],
            json!({"answer":"ok"})
        );
        handle.stop_run().await?;
    }
    Ok(())
}

#[tokio::test]
async fn output_contract_exhaustion_completes_and_no_schema_preserves_text(
) -> Result<(), Box<dyn std::error::Error>> {
    for (with_schema, budget, answers) in [
        (true, 2, vec!["not JSON", "{\"answer\":3}", "{}"]),
        (true, 0, vec!["not JSON"]),
        (false, 2, vec!["plain answer"]),
    ] {
        let temp = tempfile::tempdir()?;
        let provider = Arc::new(MockProvider::script(
            answers.iter().map(|answer| done(answer)),
        ));
        let mut config = configuration(temp.path(), Arc::<MockProvider>::clone(&provider));
        config.behavior.output_contract.max_retries = budget;
        let (handle, parent) = start(config, temp.path()).await?;
        let run = handle.run_info().await?;
        let mut args = spawn_args(false);
        if with_schema {
            args["output_schema"] = schema();
        }
        let result = join(launch(&handle, &parent, "spawn_subagent", args)).await?;
        let output = result.structured_json.ok_or("spawn output missing")?;
        assert!(output.get("structured_output").is_none());
        assert_eq!(output["output"], *answers.last().ok_or("no final answer")?);
        if with_schema {
            assert!(!output["output_errors"]
                .as_array()
                .ok_or("errors absent")?
                .is_empty());
        } else {
            assert!(output.get("output_errors").is_none());
        }
        assert_eq!(provider.call_count(), answers.len());
        let recorded = crate::store::read_events(&run.events_path)?;
        assert_eq!(recorded.iter().filter(|event| matches!(&event.payload,
            EventV1::RuntimeReminder(reminder) if reminder.kind == RuntimeReminderKind::OutputContract
        )).count(), if with_schema { budget as usize } else { 0 });
        handle.stop_run().await?;
    }
    Ok(())
}

#[tokio::test]
async fn output_contract_invalid_schema_is_rejected_before_registration(
) -> Result<(), Box<dyn std::error::Error>> {
    let temp = tempfile::tempdir()?;
    let provider = Arc::new(MockProvider::default());
    let config = configuration(temp.path(), Arc::<MockProvider>::clone(&provider));
    let (handle, parent) = start(config, temp.path()).await?;
    let run = handle.run_info().await?;
    let mut definitions = serde_json::Map::new();
    definitions.insert("level0".into(), json!(false));
    for level in 1..=25 {
        let reference = format!("#/$defs/level{}", level - 1);
        definitions.insert(
            format!("level{level}"),
            json!({"allOf":[{"$ref":reference},{"$ref":reference}]}),
        );
    }
    let expanding = json!({"$defs":definitions,"$ref":"#/$defs/level25"});
    for schema in [
        json!({"type":7}),
        json!({"required":"answer"}),
        json!([]),
        json!({"properties":{"token":{"type":"string"}}}),
        expanding,
    ] {
        let mut args = spawn_args(false);
        args["output_schema"] = schema;
        let task = launch(&handle, &parent, "spawn_subagent", args);
        let error = tokio::time::timeout(Duration::from_secs(5), task)
            .await??
            .err()
            .ok_or("invalid schema was accepted")?;
        assert!(error.contains("output_schema") || error.contains("map"));
    }
    assert_eq!(provider.call_count(), 0);
    assert!(!crate::store::read_events(&run.events_path)?
        .iter()
        .any(|event| matches!(event.payload, EventV1::NativeSubagentRegistered(_))));
    handle.stop_run().await?;
    Ok(())
}

#[tokio::test]
async fn output_contract_survives_resume_for_polling_and_child_corrections(
) -> Result<(), Box<dyn std::error::Error>> {
    let temp = tempfile::tempdir()?;
    let provider = Arc::new(MockProvider::script([
        done(r#"{"answer":"first"}"#),
        done("{}"),
        done(r#"{"answer":"resumed"}"#),
    ]));
    let mut config = configuration(temp.path(), Arc::<MockProvider>::clone(&provider));
    config.behavior.output_contract.max_retries = 1;
    let (handle, parent) = start(config.clone(), temp.path()).await?;
    let run = handle.run_info().await?;
    let mut args = spawn_args(false);
    args["output_schema"] = schema();
    let output = join(launch(&handle, &parent, "spawn_subagent", args))
        .await?
        .structured_json
        .ok_or("spawn output absent")?;
    let id = output["subagent_id"]
        .as_str()
        .ok_or("child id absent")?
        .to_owned();
    handle.stop_run().await?;
    let restored = spawn_coordinator(
        config,
        Arc::new(FakeClock::new()),
        Arc::new(DefaultRedactor::default()),
    );
    restored
        .resume_run(run.run_id.to_string(), "contract recovery")
        .await?;
    assert_eq!(provider.call_count(), 1);
    let poll = join(launch(
        &restored,
        &parent,
        "get_command_or_subagent_output",
        json!({"task_ids":[id]}),
    ))
    .await?;
    assert_eq!(
        poll.structured_json.ok_or("restored output missing")?["Result"]["structured_output"],
        json!({"answer":"first"})
    );
    let mut terminal = restored.subscribe_new_events().await?;
    let _ = join(launch(
        &restored,
        &parent,
        "send_subagent_message",
        json!({"subagent_id":id,"text":"answer again"}),
    ))
    .await?;
    event(&mut terminal, |event| match &event.payload {
        EventV1::NativeSubagentReceipt(receipt)
            if receipt.child_id == id && receipt.kind == "terminal_published" =>
        {
            Some(())
        }
        _ => None,
    })
    .await?;
    let poll = join(launch(
        &restored,
        &parent,
        "get_command_or_subagent_output",
        json!({"task_ids":[id]}),
    ))
    .await?;
    assert_eq!(
        poll.structured_json.ok_or("corrected output missing")?["Result"]["structured_output"],
        json!({"answer":"resumed"})
    );
    assert_eq!(provider.call_count(), 3);
    assert_eq!(crate::store::read_events(&run.events_path)?.iter().filter(|event| matches!(&event.payload,
        EventV1::RuntimeReminder(reminder) if reminder.kind == RuntimeReminderKind::OutputContract
    )).count(), 1);
    restored.stop_run().await?;
    Ok(())
}

#[tokio::test]
async fn output_contract_background_reminders_include_structured_results_and_errors(
) -> Result<(), Box<dyn std::error::Error>> {
    let large = json!({"answer":"x".repeat(1_100_000)}).to_string();
    for (answer, field) in [
        (r#"{"answer":"background"}"#, "structured_output"),
        ("{}", "output_errors"),
        (large.as_str(), "structured_output"),
    ] {
        let temp = tempfile::tempdir()?;
        let provider = Arc::new(MockProvider::script([
            done(answer),
            done("parent finished"),
        ]));
        let config = configuration(temp.path(), Arc::<MockProvider>::clone(&provider));
        let (handle, parent) = start(config, temp.path()).await?;
        let mut terminal = handle.subscribe_new_events().await?;
        let mut args = spawn_args(true);
        args["output_schema"] = schema();
        let background = join(launch(&handle, &parent, "spawn_subagent", args)).await?;
        let output = background
            .structured_json
            .ok_or("background handle absent")?;
        let id = output["subagent_id"].as_str().ok_or("child id absent")?;
        event(&mut terminal, |event| match &event.payload {
            EventV1::NativeSubagentReceipt(receipt)
                if receipt.child_id == id && receipt.kind == "terminal_published" =>
            {
                Some(())
            }
            _ => None,
        })
        .await?;
        let mut terminal = handle.subscribe_new_events().await?;
        let turn = handle
            .request_agent_turn(
                EventActor::new(ActorKind::User, None),
                parent.clone(),
                "use the child result",
            )
            .await?;
        event(&mut terminal, |event| match &event.payload {
            EventV1::TaskCompleted(done) if done.task_id.as_str() == turn => Some(()),
            _ => None,
        })
        .await?;
        let requests = provider.captured_requests().await;
        assert_eq!(requests.len(), 2);
        assert!(requests[1]
            .messages
            .iter()
            .any(|message| message.content.contains("<system-reminder>")
                && message.content.contains(field)));
        if answer.len() > 16_000 {
            let reminder = requests[1]
                .messages
                .iter()
                .find(|message| message.content.contains("[output truncated:"))
                .ok_or("large result reminder was not truncated")?;
            assert!(reminder.content.len() < 20_000);
            assert!(reminder.content.contains("get_command_or_subagent_output"));
        }
        let poll = join(launch(
            &handle,
            &parent,
            "get_command_or_subagent_output",
            json!({"task_ids":[id]}),
        ))
        .await?;
        assert!(
            poll.structured_json.as_ref().ok_or("poll output absent")?["Result"]
                .get(field)
                .is_some()
        );
        if answer.len() > 16_000 {
            assert_eq!(
                poll.structured_json.as_ref().ok_or("poll output absent")?["Result"]
                    ["structured_output"],
                json!({"answer":"x".repeat(1_100_000)})
            );
        }
        handle.stop_run().await?;
    }
    Ok(())
}

#[tokio::test]
async fn output_contract_woken_child_does_not_expose_previous_attempt_results(
) -> Result<(), Box<dyn std::error::Error>> {
    for first in [r#"{"answer":"first"}"#, "{}"] {
        let temp = tempfile::tempdir()?;
        let (requests, mut received) = mpsc::unbounded_channel();
        let provider = Arc::new(Gate {
            requests,
            permits: tokio::sync::Semaphore::new(1),
            responses: MockProvider::script([done(first), done(r#"{"answer":"second"}"#)]),
        });
        let config = configuration(temp.path(), Arc::<Gate>::clone(&provider));
        let (handle, parent) = start(config, temp.path()).await?;
        let mut args = spawn_args(false);
        args["output_schema"] = schema();
        let initial = join(launch(&handle, &parent, "spawn_subagent", args)).await?;
        let id = initial
            .structured_json
            .as_ref()
            .ok_or("spawn output absent")?["subagent_id"]
            .as_str()
            .ok_or("child id absent")?;
        let _ = received.recv().await.ok_or("initial request absent")?;
        let mut terminal = handle.subscribe_new_events().await?;
        let _ = join(launch(
            &handle,
            &parent,
            "send_subagent_message",
            json!({"subagent_id":id,"text":"answer again"}),
        ))
        .await?;
        let _ = tokio::time::timeout(Duration::from_secs(5), received.recv())
            .await?
            .ok_or("wake request absent")?;
        let poll = join(launch(
            &handle,
            &parent,
            "get_command_or_subagent_output",
            json!({"task_ids":[id]}),
        ))
        .await?;
        let result = &poll.structured_json.as_ref().ok_or("poll output absent")?["Result"];
        assert_eq!(result["status"], "running");
        assert!(result.get("structured_output").is_none());
        assert!(result.get("output_errors").is_none());
        provider.permits.add_permits(1);
        event(&mut terminal, |event| match &event.payload {
            EventV1::NativeSubagentReceipt(receipt)
                if receipt.child_id == id && receipt.kind == "terminal_published" =>
            {
                Some(())
            }
            _ => None,
        })
        .await?;
        handle.stop_run().await?;
    }
    Ok(())
}
