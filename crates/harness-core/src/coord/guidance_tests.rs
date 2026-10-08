use super::*;
use crate::{
    clock::FakeClock,
    redact::DefaultRedactor,
    tool::{Tool, ToolCapability, ToolContext, ToolError, ToolRegistry, ToolResult},
};
use harness_providers::{mock::MockProvider, MessageRole, ProviderStreamEvent as Stream};
use serde_json::{json, Value};
use std::sync::atomic::{AtomicUsize, Ordering};
use tokio_stream::StreamExt;

mod steering;

/// Mirrors the native todo tool's committed shape: the list is echoed as `todos`.
struct TodoWrite;
#[async_trait::async_trait]
impl Tool for TodoWrite {
    fn id(&self) -> &'static str {
        "todowrite"
    }
    fn parameters_json_schema(&self) -> Value {
        json!({"type":"object"})
    }
    fn capability(&self) -> ToolCapability {
        ToolCapability::ReadFs
    }
    async fn call(&self, _: ToolContext, args: Value) -> Result<ToolResult, ToolError> {
        let todos = args.get("todos").cloned().unwrap_or_else(|| json!([]));
        Ok(ToolResult::structured(
            todos.to_string(),
            json!({ "todos": todos }),
        ))
    }
}

fn call(id: &str, name: &str, arguments: Value) -> Vec<Stream> {
    vec![
        Stream::ToolCallComplete {
            tool_call_id: id.into(),
            function_name: name.into(),
            arguments_json: arguments.to_string(),
        },
        Stream::Done { usage: None },
    ]
}

fn answer(text: &str) -> Vec<Stream> {
    vec![Stream::TextDelta(text.into()), Stream::Done { usage: None }]
}

struct Session {
    coordinator: CoordinatorHandle,
    run: RunInfo,
    agent: String,
    config: CoordinatorConfig,
}

async fn session(
    temp: &std::path::Path,
    provider: Arc<MockProvider>,
    behavior: crate::config::BehaviorSettings,
    calls: Arc<AtomicUsize>,
) -> Result<Session, Box<dyn std::error::Error>> {
    let mut registry = ToolRegistry::new();
    registry.register(Arc::new(TodoWrite));
    registry.register(Arc::new(super::tests::CountTool(calls)));
    let mut config = CoordinatorConfig::new(temp.join("sessions"));
    config.provider = provider as Arc<dyn harness_providers::Provider>;
    config.tool_registry = Arc::new(registry);
    config.permission_policy = PermissionPolicy::allow_all();
    config.behavior = behavior;
    let mut profile = crate::agent::AgentProfile::fallback("default");
    profile.toolset = vec!["todowrite".into(), "count".into()];
    config.agent_profiles.insert("default".into(), profile);
    let coordinator = spawn_coordinator(
        config.clone(),
        Arc::new(FakeClock::new()),
        Arc::new(DefaultRedactor::default()),
    );
    let run = coordinator.start_run("guidance", temp).await?;
    let agent = coordinator
        .spawn_agent_idle(
            EventActor::new(ActorKind::Supervisor, None),
            "default",
            None,
        )
        .await?;
    Ok(Session {
        coordinator,
        run,
        agent,
        config,
    })
}

/// Waits for the turn's terminal event and returns the cancellation reason, if it failed.
async fn terminal(
    coordinator: &CoordinatorHandle,
    id: &str,
) -> Result<Option<String>, Box<dyn std::error::Error>> {
    let mut events = coordinator.event_store().await?.subscribe(1)?;
    Ok(
        tokio::time::timeout(std::time::Duration::from_secs(5), async {
            while let Some(event) = events.next().await {
                match event?.payload {
                    EventV1::TaskCompleted(e) if e.task_id.as_str() == id => return Ok(None),
                    EventV1::TaskCancelled(e) if e.task_id.as_str() == id => {
                        return Ok(Some(e.reason))
                    }
                    _ => {}
                }
            }
            Err(EventStoreError::Invalid("turn did not settle"))
        })
        .await??,
    )
}

fn reminders(
    path: &std::path::Path,
) -> Result<Vec<RuntimeReminderEvent>, Box<dyn std::error::Error>> {
    Ok(crate::store::read_events(path)?
        .into_iter()
        .filter_map(|e| match e.payload {
            EventV1::RuntimeReminder(reminder) => Some(reminder),
            _ => None,
        })
        .collect())
}

#[tokio::test]
async fn open_todos_continue_the_turn_only_while_the_agent_acts_and_survive_resume(
) -> Result<(), Box<dyn std::error::Error>> {
    let todos = json!({"todos": [
        {"content": "Write the parser", "status": "in_progress", "priority": "high"},
        {"content": "Add tests", "status": "pending", "priority": "medium"},
        {"content": "Old idea", "status": "cancelled", "priority": "low"},
    ]});
    for enabled in [true, false] {
        let temp = tempfile::tempdir()?;
        let provider = Arc::new(MockProvider::script([
            call("todo", "todowrite", todos.clone()),
            answer("Stopping early."),
            call("work", "count", json!({})),
            answer("Stopping again."),
            answer("Final answer."),
            answer("Resumed stop."),
            answer("Resumed final."),
        ]));
        let mut behavior = crate::config::BehaviorSettings::off();
        behavior.todo_continuation.enabled = enabled;
        let calls = Arc::new(AtomicUsize::new(0));
        let s = session(temp.path(), Arc::clone(&provider), behavior, calls).await?;
        let turn = s
            .coordinator
            .request_agent_turn(
                EventActor::new(ActorKind::User, None),
                s.agent.clone(),
                "build it",
            )
            .await?;
        assert_eq!(terminal(&s.coordinator, &turn).await?, None);
        let recorded = reminders(&s.run.events_path)?;
        if !enabled {
            assert!(recorded.is_empty());
            assert_eq!(provider.call_count(), 2);
            continue;
        }
        // A reminder after each stop that followed tool work; none after a stop with no action.
        assert_eq!(provider.call_count(), 5);
        assert_eq!(recorded.len(), 2);
        assert!(recorded
            .iter()
            .all(|r| r.kind == RuntimeReminderKind::TodoContinuation
                && r.request_id.as_str() == turn));
        let reminder = &recorded[0].text;
        assert!(reminder.contains("[in_progress] Write the parser"));
        assert!(reminder.contains("[pending] Add tests"));
        assert!(!reminder.contains("Old idea"));
        let requests = provider.captured_requests().await;
        let continued = &requests[2].messages;
        let last = continued.last().ok_or("continued request is empty")?;
        assert_eq!(
            (last.role, last.content.as_str()),
            (MessageRole::User, reminder.as_str())
        );
        assert!(continued.iter().any(|m| m.content == "Stopping early."));

        // Event history rebuilds the reminders in the order the model saw them.
        s.coordinator.stop_run().await?;
        let events = crate::store::read_events(&s.run.events_path)?;
        let rebuilt = super::history::messages(&events, &s.agent, true, "", &s.run.run_dir)?;
        let contents: Vec<_> = rebuilt
            .entries
            .iter()
            .map(|e| e.message.content.as_str())
            .collect();
        let early = contents
            .iter()
            .position(|c| *c == "Stopping early.")
            .ok_or("missing answer")?;
        assert_eq!(contents[early + 1], recorded[0].text);
        let again = contents
            .iter()
            .position(|c| *c == "Stopping again.")
            .ok_or("missing answer")?;
        assert_eq!(contents[again + 1], recorded[1].text);
        assert_eq!(contents.last(), Some(&"Final answer."));

        // A resumed run restores the todo list, so its next early stop is continued too.
        let resumed = spawn_coordinator(
            s.config.clone(),
            Arc::new(FakeClock::new()),
            Arc::new(DefaultRedactor::default()),
        );
        resumed
            .resume_run(s.run.run_id.to_string(), "resumed")
            .await?;
        let turn = resumed
            .request_agent_turn(
                EventActor::new(ActorKind::User, None),
                s.agent.clone(),
                "keep going",
            )
            .await?;
        assert_eq!(terminal(&resumed, &turn).await?, None);
        let recorded = reminders(&s.run.events_path)?;
        assert_eq!(recorded.len(), 3);
        assert_eq!(recorded[2].request_id.as_str(), turn);
        assert!(recorded[2].text.contains("[pending] Add tests"));
        resumed.stop_run().await?;
    }
    Ok(())
}

#[tokio::test]
async fn loop_guard_reminds_once_then_stops_identical_calls_unless_exempt(
) -> Result<(), Box<dyn std::error::Error>> {
    for exempt in [false, true] {
        let temp = tempfile::tempdir()?;
        let provider = Arc::new(MockProvider::script([
            call("a", "count", json!({"path": "x"})),
            call("b", "count", json!({"path": "x"})),
            call("c", "count", json!({"path": "x"})),
            call("d", "count", json!({"path": "x"})),
            answer("Done."),
        ]));
        let mut behavior = crate::config::BehaviorSettings::off();
        behavior.loop_guard.enabled = true;
        behavior.loop_guard.threshold = 2;
        behavior.loop_guard.exempt_tools = if exempt {
            vec!["count".into()]
        } else {
            Vec::new()
        };
        let calls = Arc::new(AtomicUsize::new(0));
        let s = session(
            temp.path(),
            Arc::clone(&provider),
            behavior,
            Arc::clone(&calls),
        )
        .await?;
        std::fs::write(temp.path().join("x"), "x")?;
        let turn = s
            .coordinator
            .request_agent_turn(
                EventActor::new(ActorKind::User, None),
                s.agent.clone(),
                "look",
            )
            .await?;
        let outcome = terminal(&s.coordinator, &turn).await?;
        let recorded = reminders(&s.run.events_path)?;
        assert_eq!(calls.load(Ordering::SeqCst), 4);
        if exempt {
            assert_eq!(outcome, None);
            assert!(recorded.is_empty());
            continue;
        }
        let reason = outcome.ok_or("repeated calls were not stopped")?;
        assert!(reason.contains("loop guard"), "{reason}");
        assert_eq!(recorded.len(), 1);
        assert_eq!(recorded[0].kind, RuntimeReminderKind::LoopGuard);
        assert!(recorded[0].text.contains("2 times in a row"));
        // The reminder reached the model before the repeats that ended the turn.
        let requests = provider.captured_requests().await;
        assert_eq!(requests.len(), 4);
        assert!(requests[2]
            .messages
            .iter()
            .any(|m| m.content == recorded[0].text));
    }
    Ok(())
}
