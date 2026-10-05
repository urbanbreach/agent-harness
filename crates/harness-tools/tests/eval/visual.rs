use super::*;
use harness_core::event::RuntimeEvent;

// QA evidence exercises the production coordinator and kernels. xterm.js replays
// these exact events through the normal TUI runtime, pausing at visible states.
#[tokio::test]
#[ignore = "requires HARNESS_EVAL_CAPTURE; scripts/qa/capture-eval.mjs"]
async fn capture_eval() -> Result {
    let destination =
        std::env::var_os("HARNESS_EVAL_CAPTURE").ok_or("HARNESS_EVAL_CAPTURE is required")?;
    let code = "var files = ['alpha.txt', 'beta.txt', 'notes.txt'];\nprint('Reading 3 files…');\nvar results = await parallel(files.map(path =>\n  () => tool.read({path})\n));\nawait new Promise(resolve => setTimeout(resolve, 150));\nfor (var i = 0; i < results.length; i++) {\n  print(files[i] + ': ' + results[i].text);\n}\ndisplay({files: results.length, status: 'ready ✓'});";
    let cells = [
        json!({"language":"js","summary":"Compare the three workspace notes","code":code}),
        json!({"language":"py","summary":"Validate the report data","code":"import json\nprint('Checking report.json…')\njson.loads('{invalid}')"}),
        json!({"language":"js","summary":"Inspect the large output","code":"print('Large report: ' + 'entry '.repeat(10000));\nprint('Report complete.');"}),
        json!({"language":"js","summary":"Build the summary in the background","code":"print('Preparing the summary…');\nawait new Promise(resolve => setTimeout(resolve, 1300));\ndisplay({status: 'complete', files: 3});"}),
    ];
    let responses = cells
        .iter()
        .enumerate()
        .flat_map(|(i, args)| {
            [
                vec![
                    Stream::ToolCallComplete {
                        tool_call_id: format!("eval-scene-{i}"),
                        function_name: "eval".into(),
                        arguments_json: args.to_string(),
                    },
                    Stream::Done { usage: None },
                ],
                vec![
                    Stream::TextDelta(
                        [
                            "All three notes are ready.",
                            "The report contains invalid JSON.",
                            "Full output retained.",
                            "The summary is running in the background.",
                        ][i]
                            .into(),
                    ),
                    Stream::Done { usage: None },
                ],
            ]
        })
        .chain([vec![
            Stream::TextDelta("The background summary is complete.".into()),
            Stream::Done { usage: None },
        ]]);
    let session = Session::new(
        EvalConfig {
            cell_timeout_seconds: 1,
            foreground_window_seconds: 1,
            ..settings()
        },
        MockProvider::script(responses),
        None,
    )
    .await?;
    for (name, text) in [
        (
            "alpha.txt",
            "First note\nsecond line\nthird line\nfourth line",
        ),
        ("beta.txt", "第二のノート · café 👩🏽‍💻\nUnicode stays aligned."),
        (
            "notes.txt",
            "One\nTwo\nThree\nFour\nFive\nSix\nSeven\nEight",
        ),
    ] {
        std::fs::write(session._root.path().join(name), text)?;
    }
    let mut stream = session.handle.event_store().await?.subscribe_runtime(1)?;
    let stop = tokio_util::sync::CancellationToken::new();
    let finished = stop.clone();
    let recorder = tokio::spawn(async move {
        let mut events = Vec::new();
        loop {
            tokio::select! {
                biased;
                event = stream.next() => match event { Some(event) => events.push(event?), None => break },
                () = finished.cancelled() => break,
            }
        }
        Ok::<_, harness_core::store::EventStoreError>(events)
    });
    let mut events = session.handle.subscribe_new_events().await?;
    let agent = session.actor.agent_id.as_deref().ok_or("missing actor")?;
    for prompt in [
        "Compare the workspace notes.",
        "Validate the report data.",
        "Inspect a large output.",
        "Build a summary in the background.",
    ] {
        let id = session
            .handle
            .request_agent_turn(EventActor::new(ActorKind::User, None), agent, prompt)
            .await?;
        wait_event(
            &mut events,
            |event| matches!(event, EventV1::TaskCompleted(task) if task.task_id.as_str() == id),
        )
        .await?;
    }
    wait_event(&mut events, |event| {
        matches!(event, EventV1::EvalCellFinished(_))
    })
    .await?;
    session.handle.stop_run().await?;
    stop.cancel();
    let events = recorder.await??;
    assert!(events.iter().any(|e| matches!(e, RuntimeEvent::Live(e) if matches!(e.payload, harness_core::event::LiveEventV1::EvalProgress {..}))));
    let artifacts = std::path::Path::new(&destination)
        .parent()
        .ok_or("capture destination needs a parent")?
        .join("run-artifacts");
    std::fs::create_dir_all(&artifacts)?;
    for entry in std::fs::read_dir(&session.info.artifacts_dir)? {
        let entry = entry?;
        if entry.file_type()?.is_file() {
            std::fs::copy(entry.path(), artifacts.join(entry.file_name()))?;
        }
    }
    std::fs::write(destination, serde_json::to_vec_pretty(&events)?)?;
    Ok(())
}

async fn wait_event(
    events: &mut harness_core::store::EventStream,
    matches: impl Fn(&EventV1) -> bool,
) -> Result {
    tokio::time::timeout(Duration::from_secs(15), async {
        while let Some(event) = events.next().await {
            let event = event?;
            if let EventV1::TaskCancelled(task) = &event.payload
                && task.failure
                && task.task_scope == Some(harness_core::event::TaskTerminalScope::AgentTurn)
            {
                return Err(format!("capture turn failed: {}", task.reason).into());
            }
            if matches(&event.payload) {
                return Ok::<_, Box<dyn std::error::Error>>(());
            }
        }
        Err("required eval capture event missing".into())
    })
    .await?
}
