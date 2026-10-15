//! The save race rules, driven through the coordinator with a stand-in for a running tool.
use crate::coord::{
    handle::system,
    runtime::{Job, JobKind, Runtime},
    *,
};
use crate::{clock::FakeClock, redact::DefaultRedactor};
use tokio_util::sync::CancellationToken;

struct Fixture {
    _temp: tempfile::TempDir,
    handle: CoordinatorHandle,
    agents: [String; 2],
}

async fn fixture() -> Result<Fixture, Box<dyn std::error::Error>> {
    let temp = tempfile::tempdir()?;
    let mut config = CoordinatorConfig::new(temp.path().join("sessions"));
    config.compaction.notes_windows = true;
    let handle = spawn_coordinator(
        config,
        Arc::new(FakeClock::new()),
        Arc::new(DefaultRedactor::default()),
    );
    handle.start_run("notes", temp.path()).await?;
    let first = handle.spawn_agent_idle(system(), "default", None).await?;
    let second = handle.spawn_agent_idle(system(), "default", None).await?;
    Ok(Fixture {
        _temp: temp,
        handle,
        agents: [first, second],
    })
}

/// Registers the running tool call `id` for `agent`.
async fn running(f: &Fixture, id: &str, agent: &str) -> Result<(), CoordinatorError> {
    let (id, agent) = (id.to_owned(), agent.to_owned());
    f.handle
        .call(move |s| {
            s.running.insert(
                id,
                Job {
                    join_id: None,
                    actor: EventActor::new(ActorKind::Worker, Some(agent)),
                    kind: JobKind::Tool {
                        tool_id: "context_notes".into(),
                        reply: None,
                        capability: crate::tool::ToolCapability::ReadFs,
                        paths: Vec::new(),
                    },
                    parent: None,
                    cancellation: CancellationToken::new(),
                    reason: None,
                    hooks: Vec::new(),
                },
            );
            Ok(())
        })
        .await
}

async fn notes_events(f: &Fixture) -> Result<usize, Box<dyn std::error::Error>> {
    let path = f.handle.run_info().await?.events_path;
    Ok(crate::store::read_events(&path)?
        .iter()
        .filter(|event| matches!(event.payload, EventV1::ContextNotes(_)))
        .count())
}

type Change = Box<dyn Fn(&mut Runtime, &[String; 2]) + Send + Sync>;

#[tokio::test]
async fn a_save_is_refused_when_anything_it_captured_changed(
) -> Result<(), Box<dyn std::error::Error>> {
    let changes: [(&str, Change, &str); 4] = [
        (
            "the branch was rewound",
            Box::new(|s, _| {
                let _ = s.emit(
                    system(),
                    None,
                    EventV1::ConversationRewound(
                        crate::conversation_rewind::ConversationRewoundEvent {
                            target_seq: 1,
                            request_id: "req".into(),
                        },
                    ),
                );
            }),
            "Experimental context notes were not saved because the session branch changed.",
        ),
        (
            "the experiment was disabled",
            Box::new(|s, _| s.config.compaction.notes_windows = false),
            "Experimental context management is disabled.",
        ),
        (
            "the agent was stopped",
            Box::new(|s, agents| {
                s.killed_agents.insert(agents[0].clone());
            }),
            "Experimental context management is unavailable because this session is disposed.",
        ),
        (
            "the tool was aborted",
            Box::new(|s, _| {
                if let Some(job) = s.running.get("save") {
                    job.cancellation.cancel();
                }
            }),
            "cancelled",
        ),
    ];
    for (name, change, expected) in changes {
        let f = fixture().await?;
        running(&f, "save", &f.agents[0]).await?;
        let ticket = f.handle.context_notes_ticket("save").await?;
        let agents = f.agents.clone();
        f.handle
            .call(move |s| {
                change(s, &agents);
                Ok(())
            })
            .await?;
        let error = f
            .handle
            .context_notes_save("save", ticket, "stale notebook".into())
            .await
            .err()
            .ok_or_else(|| format!("{name}: the stale notebook was saved"))?;
        assert!(error.to_string().contains(expected), "{name}: {error}");
        assert_eq!(notes_events(&f).await?, 0, "{name}");
    }
    Ok(())
}

#[tokio::test]
async fn a_ticket_belongs_to_its_owner_and_a_save_lands_exactly_once(
) -> Result<(), Box<dyn std::error::Error>> {
    let f = fixture().await?;
    running(&f, "first", &f.agents[0]).await?;
    running(&f, "second", &f.agents[1]).await?;
    let ticket = f.handle.context_notes_ticket("first").await?;
    let error = f
        .handle
        .context_notes_save("second", ticket.clone(), "borrowed".into())
        .await
        .err()
        .ok_or("another agent used the ticket")?;
    assert!(error.to_string().contains("the session branch changed"));
    assert_eq!(notes_events(&f).await?, 0);

    let saved = f
        .handle
        .context_notes_save("first", ticket, "mine".into())
        .await?;
    assert_eq!((saved.text.as_str(), saved.bytes), ("mine", 4));
    // Each agent reads only its own notebook.
    assert_eq!(f.handle.context_notes_read("first").await?.text, "mine");
    let other = f.handle.context_notes_read("second").await?;
    assert_eq!((other.entry_id, other.text.as_str()), (None, ""));
    Ok(())
}

#[tokio::test]
async fn notes_after_a_rewind_are_the_ones_before_the_rewound_point(
) -> Result<(), Box<dyn std::error::Error>> {
    let f = fixture().await?;
    running(&f, "save", &f.agents[0]).await?;
    let ticket = f.handle.context_notes_ticket("save").await?;
    f.handle
        .context_notes_save("save", ticket, "before".into())
        .await?;
    let boundary = f.handle.run_info().await?;
    let seq = crate::store::read_events(&boundary.events_path)?
        .last()
        .map_or(0, |event| event.seq);
    let ticket = f.handle.context_notes_ticket("save").await?;
    f.handle
        .context_notes_save("save", ticket, "after".into())
        .await?;
    assert_eq!(f.handle.context_notes_read("save").await?.text, "after");
    f.handle
        .call(move |s| {
            s.emit(
                system(),
                None,
                EventV1::ConversationRewound(
                    crate::conversation_rewind::ConversationRewoundEvent {
                        target_seq: seq + 1,
                        request_id: "req".into(),
                    },
                ),
            )
            .map(|_| ())
        })
        .await?;
    assert_eq!(f.handle.context_notes_read("save").await?.text, "before");
    Ok(())
}
