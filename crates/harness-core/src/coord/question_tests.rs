use super::*;
use crate::{clock::FakeClock, redact::DefaultRedactor};
use serde_json::json;
use tokio_stream::StreamExt;

#[tokio::test]
async fn questions_validate_answers_and_cancel_without_consuming_tool_capacity(
) -> Result<(), Box<dyn std::error::Error>> {
    let temp = tempfile::tempdir()?;
    let mut config = CoordinatorConfig::new(temp.path().join("sessions"));
    config.tool_concurrency = 1;
    config.yolo_on_start = true;
    let mut profile = AgentProfile::fallback("default");
    profile.toolset = vec!["question".into()];
    config.agent_profiles.insert("default".into(), profile);
    let coordinator = spawn_coordinator(
        config,
        Arc::new(FakeClock::new()),
        Arc::new(DefaultRedactor::default()),
    );
    let run = coordinator.start_run("questions", temp.path()).await?;
    let agent = coordinator
        .spawn_agent_idle(
            EventActor::new(ActorKind::Supervisor, None),
            "default",
            None,
        )
        .await?;
    let actor = EventActor::new(ActorKind::Worker, Some(agent));
    let mut events = coordinator.event_store().await?.subscribe(1)?;
    let request = json!({"questions":[{"header":"Choice", "question":"Pick one", "options":[{"label":"A","description":"First"},{"label":"B","description":"Second"}], "custom":false}]});
    let mut workers = Vec::new();
    for id in ["first-question", "second-question"] {
        let handle = coordinator.clone();
        let (actor, request) = (actor.clone(), request.clone());
        workers.push(tokio::spawn(async move {
            handle.request_question(actor, id, request).await
        }));
    }
    let approvals = tokio::time::timeout(std::time::Duration::from_secs(3), async {
        let mut approvals = Vec::new();
        while approvals.len() < 2 {
            if let EventV1::PermissionRequested(request) =
                events.next().await.ok_or("missing question")??.payload
            {
                assert_eq!(request.kind, "question");
                approvals.push(request);
            }
        }
        Ok::<_, Box<dyn std::error::Error>>(approvals)
    })
    .await??;
    coordinator.set_yolo_mode(true).await?;
    let first = approvals
        .iter()
        .find(|p| {
            p.tool_call_id
                .as_ref()
                .is_some_and(|id| id.as_str() == "first-question")
        })
        .ok_or("missing first approval")?;
    // Bad selections must not consume the pending question or release the worker.
    for answer in ["not JSON", r#"[["A","B"]]"#, r#"[["C"]]"#] {
        assert!(coordinator
            .resolve_permission(
                &first.permission_id,
                PermissionDecision::Allow,
                Some(answer.into())
            )
            .await
            .is_err());
    }
    coordinator
        .resolve_permission(
            &first.permission_id,
            PermissionDecision::Allow,
            Some(r#"[["B"]]"#.into()),
        )
        .await?;
    let first_worker = workers.remove(0);
    let answer = tokio::time::timeout(std::time::Duration::from_secs(3), first_worker).await???;
    assert_eq!(answer.structured_json, Some(json!({"answers":[["B"]]})));
    tokio::time::timeout(std::time::Duration::from_secs(3), coordinator.stop_run()).await??;
    assert!(workers.remove(0).await?.is_err());
    let events = crate::store::read_events(&run.events_path)?;
    assert_eq!(
        events
            .iter()
            .filter(|e| matches!(e.payload, EventV1::PermissionRequested(_)))
            .count(),
        2
    );
    assert_eq!(
        events
            .iter()
            .filter(|e| matches!(e.payload, EventV1::PermissionResolved(_)))
            .count(),
        2
    );
    assert!(matches!(
        events.last().map(|e| &e.payload),
        Some(EventV1::RunFinished(_))
    ));
    Ok(())
}
