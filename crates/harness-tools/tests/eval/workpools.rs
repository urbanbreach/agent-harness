use super::*;

#[tokio::test]
#[ignore = "requires Node.js 24+ and Python; scripts/test-lanes.sh eval"]
async fn workpools_bound_native_admission_and_preserve_kernel_tool_scope() -> Result {
    for language in ["js", "py"] {
        let probe = Arc::new(Probe {
            started: Semaphore::new(0),
            release: [Semaphore::new(0), Semaphore::new(0)],
        });
        let turns = (0..4).flat_map(|index| {
            [
                vec![
                    Stream::ToolCallComplete {
                        tool_call_id: format!("lookup-{index}"),
                        function_name: "lookup".into(),
                        arguments_json: "{}".into(),
                    },
                    Stream::Done { usage: None },
                ],
                vec![
                    Stream::TextDelta(format!("worker-{index}")),
                    Stream::Done { usage: None },
                ],
            ]
        });
        let session = Session::new(
            settings(),
            MockProvider::script(turns),
            Some(Arc::clone(&probe)),
        )
        .await?;
        let code = if language == "js" {
            "tool(async function lookup() { return (await tool.probe({slot:0})).text; }); var pool = await workpool({subagent_type:'general-purpose',prompt:'Call lookup once'}, 'jobs', {width:1,tools:['lookup','probe'],mode:'fresh'}); var pushed = await pool.push([{key:'a',input:'first'},{key:'b',input:'second'},{key:'c',input:'third'}]); await pool.close(); display(await wait(pushed.details.items,{mode:'settled',timeout:10})); display(await pool.inspect());"
        } else {
            "@tool\ndef lookup():\n    return tool.probe(slot=0)['text']\npool = workpool({'subagent_type':'general-purpose','prompt':'Call lookup once'}, 'jobs', width=1, tools=['lookup','probe'], mode='fresh')\npushed = pool.push([{'key':'a','input':'first'},{'key':'b','input':'second'},{'key':'c','input':'third'}])\npool.close()\ndisplay(wait(pushed['details']['items'], mode='settled', timeout=10))\ndisplay(pool.inspect())"
        };
        let run = session.good(language, code);
        tokio::pin!(run);
        for _ in 0..3 {
            tokio::select! {
                result = &mut run => return Err(format!("pool finished before the next worker: {}", result?.display_text).into()),
                permit = tokio::time::timeout(Duration::from_secs(10), probe.started.acquire()) => permit??.forget(),
            }
            assert_eq!(
                probe.started.available_permits(),
                0,
                "pool exceeded width=1"
            );
            probe.release[0].add_permits(1);
        }
        let result = run.await?;
        for marker in ["worker-0", "worker-1", "worker-2", "\"done\": true"] {
            assert!(
                result.display_text.contains(marker),
                "{language}: {}",
                result.display_text
            );
        }
        let rejected = session
            .run(
                language,
                if language == "js" {
                    "await pool.push(['late']);"
                } else {
                    "pool.push(['late'])"
                },
            )
            .await?;
        assert!(rejected.is_error());
        let creation = session.good(language, if language == "js" {
            "var cancelPool = await workpool({prompt:'Call lookup once'},'cancel-jobs',{width:1,tools:['lookup','probe']}); var pending = await cancelPool.push(['first','second']); print(cancelPool.pool_id);"
        } else {
            "cancel_pool = workpool({'prompt':'Call lookup once'}, 'cancel-jobs', width=1, tools=['lookup','probe'])\npending = cancel_pool.push(['first','second'])\nprint(cancel_pool.pool_id)"
        }).await?;
        tokio::time::timeout(Duration::from_secs(10), probe.started.acquire())
            .await??
            .forget();
        let pool_id = creation
            .display_text
            .split_whitespace()
            .find(|word| word.starts_with("wp_"))
            .ok_or("pool id missing")?;
        let sibling = session
            .handle
            .spawn_agent_idle(EventActor::new(ActorKind::User, None), "default", None)
            .await?;
        let foreign = session.handle.execute_agent_tool_call(EventActor::new(ActorKind::Worker, Some(sibling)), None, "eval",
            json!({"language":"js","code":format!("await workpool.open({}).inspect()", serde_json::to_string(pool_id)?),"summary":"Check pool ownership","on_timeout":"error"})).await?;
        assert!(
            foreign.is_error() && foreign.display_text.contains("not owned"),
            "{}",
            foreign.display_text
        );
        let stopped = session.good(language, if language == "js" {
            "display(await wait(pending.details.items,{mode:'any',timeout:0})); await cancelPool.cancel(); display(await wait(pending.details.items,{mode:'settled',timeout:10})); display(await cancelPool.inspect());"
        } else {
            "display(wait(pending['details']['items'], mode='any', timeout=0))\ncancel_pool.cancel()\ndisplay(wait(pending['details']['items'], mode='settled', timeout=10))\ndisplay(cancel_pool.inspect())"
        }).await?;
        assert!(
            stopped.display_text.contains("\"done\": false")
                && stopped.display_text.contains("cancelled"),
            "{}",
            stopped.display_text
        );
        assert_eq!(
            probe.started.available_permits(),
            0,
            "queued worker started after cancellation"
        );
        let failed_wait = session
            .run(
                language,
                if language == "js" {
                    "await wait(pending.details.items,{mode:'all',timeout:0});"
                } else {
                    "wait(pending['details']['items'], mode='all', timeout=0)"
                },
            )
            .await?;
        assert!(
            failed_wait.is_error() && failed_wait.display_text.contains("failed or was cancelled"),
            "{}",
            failed_wait.display_text
        );
        let rejected = session.good(language, if language == "js" {
            "var invalidPool = await workpool('missing-profile','invalid-jobs'); display(await invalidPool.push(['rejected'])); await invalidPool.close();"
        } else {
            "invalid_pool = workpool('missing-profile','invalid-jobs')\ndisplay(invalid_pool.push(['rejected']))\ninvalid_pool.close()"
        }).await?;
        assert!(
            rejected.display_text.contains("failed") && rejected.display_text.contains("terminal"),
            "{}",
            rejected.display_text
        );
        session.handle.stop_run().await?;
    }
    Ok(())
}

#[tokio::test]
#[ignore = "requires Node.js 24+; scripts/test-lanes.sh eval"]
async fn closed_pool_delivers_one_aggregate_notification_to_model() -> Result {
    let code = "var pool = await workpool('general-purpose','notify',{width:1}); var pushed = await pool.push(['first','second']); await pool.close(); await wait(pushed.details.items,{mode:'settled',timeout:10});";
    let first = vec![Stream::ToolCallComplete {
        tool_call_id: "pool-eval".into(), function_name: "eval".into(),
        arguments_json: json!({"language":"js","summary":"Run pooled work","code":code,"on_timeout":"error"}).to_string(),
    }, Stream::Done {usage:None}];
    let rest = [
        "first eval-test-credential-9f3724",
        "second",
        "submitted",
        "notification received",
    ]
    .map(|text| vec![Stream::TextDelta(text.into()), Stream::Done { usage: None }]);
    let session = Session::new(
        settings(),
        MockProvider::script(std::iter::once(first).chain(rest)),
        None,
    )
    .await?;
    let mut events = session.handle.subscribe_new_events().await?;
    let owner = session.actor.agent_id.clone().ok_or("owner missing")?;
    session
        .handle
        .request_agent_turn(
            EventActor::new(ActorKind::User, None),
            owner.clone(),
            "Run pooled jobs",
        )
        .await?;
    tokio::time::timeout(Duration::from_secs(20), async {
        let mut completed = 0;
        while let Some(event) = events.next().await {
            let event = event?;
            if event.actor.agent_id.as_deref() != Some(&owner) {
                continue;
            }
            match event.payload {
                EventV1::TaskCompleted(t)
                    if t.metadata.as_ref().and_then(|m| m.task_scope.as_ref())
                        == Some(&harness_core::event::TaskTerminalScope::AgentTurn) =>
                {
                    completed += 1
                }
                EventV1::TaskCancelled(t) => {
                    return Err(format!("pool owner failed: {}", t.reason).into())
                }
                _ => {}
            }
            if completed == 2 {
                return Ok::<_, Box<dyn std::error::Error>>(());
            }
        }
        Err("pool notification never completed".into())
    })
    .await??;
    session.handle.stop_run().await?;
    let events = harness_core::store::read_events(&session.info.events_path)?;
    let notices = events
        .iter()
        .filter_map(|event| match &event.payload {
            EventV1::UserMessageSubmitted(t) if t.text.starts_with("Workpool notify ") => {
                Some(&t.text)
            }
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(notices.len(), 1);
    assert!(notices[0].contains("first") && notices[0].contains("second"));
    assert!(!notices[0].contains("eval-test-credential-9f3724"));
    assert_eq!(events.iter().filter(|event| matches!(&event.payload, EventV1::NativeSubagentReceipt(r) if r.kind == "notification_consumed")).count(), 2);
    Ok(())
}
