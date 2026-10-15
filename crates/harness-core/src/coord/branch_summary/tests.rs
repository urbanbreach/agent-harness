use super::super::*;
use crate::{clock::FakeClock, redact::DefaultRedactor};
use harness_providers::{mock::MockProvider, MessageRole, ProviderStreamEvent as Stream};

#[tokio::test]
async fn rewind_branch_summary_is_opt_in_and_survives_resume(
) -> Result<(), Box<dyn std::error::Error>> {
    for (enabled, fails) in [(false, false), (true, false), (true, true)] {
        let temp = tempfile::tempdir()?;
        let answer =
            |text: &str| vec![Stream::TextDelta(text.into()), Stream::Done { usage: None }];
        let mut script = vec![answer("kept answer"), answer("abandoned answer")];
        if enabled {
            script.push(if fails {
                vec![Stream::Error {
                    message: "summary unavailable".into(),
                    category: None,
                    remediation: None,
                    retry_after_ms: None,
                    envelope: None,
                }]
            } else {
                answer("Recovered branch findings")
            });
        }
        script.extend([answer("replacement answer"), answer("resumed answer")]);
        let provider = Arc::new(MockProvider::script(script));
        let mut config = CoordinatorConfig::new(temp.path().join("sessions"));
        config.provider = Arc::<MockProvider>::clone(&provider);
        config.compaction.async_enabled = false;
        config.compaction.branch_summary.enabled = enabled;
        let coordinator = spawn_coordinator(
            config.clone(),
            Arc::new(FakeClock::new()),
            Arc::new(DefaultRedactor::default()),
        );
        let run = coordinator.start_run("branches", temp.path()).await?;
        let agent = coordinator
            .spawn_agent_idle(
                EventActor::new(ActorKind::Supervisor, None),
                "default",
                None,
            )
            .await?;
        let user = EventActor::new(ActorKind::User, None);
        let kept = coordinator
            .request_agent_turn(user.clone(), &agent, "kept prompt")
            .await?;
        super::super::history_tests::settled(&coordinator, &kept).await?;
        let abandoned = coordinator
            .request_agent_turn(user.clone(), &agent, "abandoned prompt")
            .await?;
        super::super::history_tests::settled(&coordinator, &abandoned).await?;
        coordinator.rewind_conversation(&abandoned).await?;
        assert_eq!(provider.call_count(), if enabled { 3 } else { 2 });
        let journal = crate::store::read_events(&run.events_path)?;
        let summaries: Vec<_> = journal
            .iter()
            .filter_map(|e| match &e.payload {
                EventV1::BranchSummary(summary) => Some(summary),
                _ => None,
            })
            .collect();
        assert_eq!(summaries.len(), usize::from(enabled && !fails));
        let replacement = coordinator
            .request_agent_turn(user.clone(), &agent, "replacement")
            .await?;
        super::super::history_tests::settled(&coordinator, &replacement).await?;
        let requests = provider.captured_requests().await;
        let request = requests.last().ok_or("missing replacement request")?;
        assert!(!request
            .messages
            .iter()
            .any(|m| m.content == "abandoned prompt" || m.content == "abandoned answer"));
        if enabled && !fails {
            let summary = summaries.first().ok_or("missing branch summary")?;
            assert!(summary
                .summary
                .starts_with(harness_compaction::prompts::branch_preamble()));
            let wrapped = harness_compaction::prompts::branch_summary_message(&summary.summary);
            let branch = request
                .messages
                .iter()
                .position(|m| m.role == MessageRole::User && m.content == wrapped)
                .ok_or("summary missing from provider context")?;
            assert_eq!(request.messages[branch - 1].content, "kept answer");
            assert_eq!(request.messages[branch + 1].content, "replacement");
            let summary_request = requests.get(2).ok_or("missing summarizer request")?;
            assert!(summary_request.messages[1]
                .content
                .contains("abandoned prompt"));
            assert!(summary_request.messages[1]
                .content
                .contains("abandoned answer"));
            assert!(!summary_request.messages[1].content.contains("kept prompt"));
            assert_eq!(summary_request.max_tokens, Some(2048));
            assert!(summary_request.reasoning_effort.is_none());
        }
        let prefix = request.messages.clone();
        coordinator.stop_run().await?;
        drop(coordinator);
        let resumed = spawn_coordinator(
            config,
            Arc::new(FakeClock::new()),
            Arc::new(DefaultRedactor::default()),
        );
        resumed
            .resume_run(run.run_id.to_string(), "branches")
            .await?;
        let next = resumed.request_agent_turn(user, &agent, "resume").await?;
        super::super::history_tests::settled(&resumed, &next).await?;
        let requests = provider.captured_requests().await;
        let restored = requests.last().ok_or("missing resumed request")?;
        assert_eq!(&restored.messages[..prefix.len()], prefix.as_slice());
        resumed.stop_run().await?;
    }
    Ok(())
}
