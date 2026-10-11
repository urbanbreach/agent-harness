use super::*;

#[tokio::test]
#[ignore = "requires local eval runtimes; scripts/test-lanes.sh eval"]
async fn completion_and_agent_helpers_use_the_native_provider_and_task_boundaries() -> Result {
    let provider = MockProvider::script((0..10).map(|index| {
        vec![
            Stream::TextDelta("{\"answer\":42}".into()),
            Stream::DoneWithMetadata {
                usage: Some(harness_providers::CompletionUsage {
                    prompt_tokens: 12,
                    completion_tokens: 3,
                    total_tokens: 15,
                }),
                metadata: Some(harness_providers::ProviderStreamFinishedMetadata {
                    output_limit_reached: matches!(index, 2 | 3),
                    settled_reasoning: Some(Vec::new()),
                    usage_complete: Some(true),
                    ..Default::default()
                }),
            },
        ]
    }));
    let session = Session::new(settings(), provider, None).await?;
    for language in ["js", "py"] {
        let code = if language == "js" {
            "display(await completion('answer', {model:'smol',schema:{type:'object'}}))"
        } else {
            "display(completion('answer', model='slow', schema={'type':'object'}))"
        };
        assert!(session
            .good(language, code)
            .await?
            .display_text
            .contains("42"));
    }
    for language in ["js", "py"] {
        let code = if language == "js" {
            "try { await completion('answer', {model:'smol',schema:{type:'object'}}); throw Error('accepted truncated'); } catch(e) { if (!String(e).includes('output limit')) throw e; display('rejected truncation'); }"
        } else {
            "try:\n completion('answer', model='slow', schema={'type':'object'})\n raise RuntimeError('accepted truncated')\nexcept Exception as error:\n if 'output limit' not in str(error): raise\n print('rejected truncation')"
        };
        assert!(session
            .good(language, code)
            .await?
            .display_text
            .contains("rejected truncation"));
    }
    let foreground = session
        .good(
            "js",
            "var answer = await agent('answer', {schema:{type:'object'}, tools:[]}); if(answer.answer !== 42) throw new Error('agent did not return parsed JSON'); display(answer)",
        )
        .await?;
    assert!(foreground.display_text.contains("42"));
    let background = session
        .good(
            "js",
            "var child = await agent('answer', {handle:true, tools:[]}); display(child); display(await child.control.wait({timeout:10})); display(await child.control.status()); display(await child.control.output()); try { await wait([{id:child.id,run_epoch:1}],{timeout:0}); throw new Error('stale epoch accepted'); } catch(e) { if(e.code !== 'stale_task_handle') throw e; }",
        )
        .await?;
    assert!(
        background.display_text.contains("agent://"),
        "{}",
        background.display_text
    );
    assert!(background.display_text.contains("42"));
    session.good("js", "await child.control.send('answer again'); for (const op of ['status','output','cancel']) { let rejected = false; try { await child.control[op](); } catch(e) { rejected = String(e).includes('stale task handle'); } if(!rejected) throw Error('old handle reached restarted child: '+op); } display(await wait(child.id,{timeout:10}));").await?;
    let python = session.good("py", "child = agent('answer', handle=True, tools=[])\ndisplay(child.control.wait(timeout=10))\ndisplay(child.control.status())\ndisplay(child.control.output())").await?;
    assert!(python.display_text.contains("42"));
    let output = session
        .good("js", "display(await output(child.id))")
        .await?;
    assert!(
        !output.display_text.contains("TaskNotFound"),
        "{}",
        output.display_text
    );
    session.handle.stop_run().await?;
    let events = harness_core::store::read_events(&session.info.events_path)?;
    assert_eq!(events.iter().filter(|e| matches!(&e.payload, EventV1::ProviderRequestStarted(t) if t.prompt_summary == "Eval completion")).count(),4);
    assert_eq!(events.iter().filter(|e| matches!(&e.payload, EventV1::ProviderRequestFinished(f)
        if f.metadata.as_ref().and_then(|m| m.provider_error_category) == Some(harness_providers::ProviderErrorCategory::OutputLimitExceeded)
            && f.usage.as_ref().is_some_and(|u| u.total_tokens == 15))).count(), 2);
    Ok(())
}
