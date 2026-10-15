//! End to end: a model's `new_context` request rolls the window over through the real
//! coordinator, worker loop and journal, with stand-ins for the four tools.
use crate::coord::{
    compaction_tests::{answer, settled},
    handle::system,
    *,
};
use crate::{
    clock::FakeClock,
    perm::PermissionPolicy,
    redact::DefaultRedactor,
    tool::{Tool, ToolCapability, ToolContext, ToolError, ToolRegistry, ToolResult},
};
use harness_providers::{mock::MockProvider, ProviderStreamEvent as Stream};
use serde_json::{json, Value};

struct Double(&'static str);

#[async_trait::async_trait]
impl Tool for Double {
    fn id(&self) -> &str {
        self.0
    }
    fn parameters_json_schema(&self) -> Value {
        json!({"type": "object"})
    }
    fn capability(&self) -> ToolCapability {
        ToolCapability::ReadFs
    }
    fn permission_requests(&self, _: &Value) -> Vec<(String, String)> {
        Vec::new()
    }
    async fn call(&self, ctx: ToolContext, args: Value) -> Result<ToolResult, ToolError> {
        let task = ctx.tool_call_id.to_string();
        match self.0 {
            "new_context" => {
                ctx.coordinator.new_context_request(task).await?;
                Ok(ToolResult::structured(
                    "New context window requested.",
                    json!({"requested": true}),
                ))
            }
            "context_notes" => {
                let ticket = ctx.coordinator.context_notes_ticket(task.clone()).await?;
                let text = args["text"].as_str().unwrap_or_default().to_owned();
                ctx.coordinator
                    .context_notes_save(task, ticket, text)
                    .await?;
                Ok(ToolResult::text("Context notes saved."))
            }
            _ => Ok(ToolResult::text("ok")),
        }
    }
}

fn call(id: &str, name: &str, arguments: &str) -> Stream {
    Stream::ToolCallComplete {
        tool_call_id: id.into(),
        function_name: name.into(),
        arguments_json: arguments.into(),
    }
}

struct Run {
    provider: Arc<MockProvider>,
    events: Vec<EventEnvelopeV1>,
    /// Lifecycle events published while the prompts ran.
    lifecycle: usize,
}

/// Runs `prompts` in order against a scripted provider with the experiment on.
async fn run(
    script: Vec<Vec<Stream>>,
    prompts: &[&str],
    enabled: bool,
) -> Result<Run, Box<dyn std::error::Error>> {
    run_with(script, prompts, enabled, |_| {}).await
}

async fn run_with(
    script: Vec<Vec<Stream>>,
    prompts: &[&str],
    enabled: bool,
    tune: impl FnOnce(&mut CoordinatorConfig),
) -> Result<Run, Box<dyn std::error::Error>> {
    let temp = tempfile::tempdir()?;
    let provider = Arc::new(MockProvider::script(script));
    let settings = crate::config::load_config_from_str(
        r#"{
            model: 'local/big', provider: { local: {
                type: 'openai_compatible', baseUrl: 'https://example.invalid',
                models: { big: { limit: { context: 100000, output: 8192 } } }
            } }
        }"#,
    )?;
    let target = crate::config::resolve_model_selection(&settings, "local/big", None)?.primary;
    let mut registry = ToolRegistry::new();
    for name in ["context_notes", "new_context", "read", "grep"] {
        registry.register(Arc::new(Double(name)));
    }
    let mut config = CoordinatorConfig::new(temp.path().join("sessions"));
    config.provider = Arc::clone(&provider) as Arc<dyn harness_providers::Provider>;
    config.tool_registry = Arc::new(registry);
    config.permission_policy = PermissionPolicy::allow_all();
    config.agent_model_targets.insert("default".into(), target);
    let mut profile = crate::agent::AgentProfile::fallback("default");
    profile.toolset = ["context_notes", "new_context", "read", "grep"]
        .map(String::from)
        .to_vec();
    config.agent_profiles.insert("default".into(), profile);
    config.compaction.notes_windows = true;
    config.compaction.enabled = enabled;
    config.compaction.mid_turn_enabled = false;
    config.compaction.keep_recent_tokens = 512;
    tune(&mut config);
    let coordinator = spawn_coordinator(
        config,
        Arc::new(FakeClock::new()),
        Arc::new(DefaultRedactor::default()),
    );
    let info = coordinator.start_run("notes", temp.path()).await?;
    let agent = coordinator
        .spawn_agent_idle(EventActor::new(ActorKind::User, None), "default", None)
        .await?;
    let mut live = coordinator.event_store().await?.subscribe_runtime(1)?;
    let counter = tokio::spawn(async move {
        use tokio_stream::StreamExt;
        let mut seen = 0;
        while let Some(Ok(event)) = live.next().await {
            if matches!(
                event,
                crate::event::RuntimeEvent::Live(ref e)
                    if matches!(e.payload, LiveEventV1::CompactionLifecycle(_))
            ) {
                seen += 1;
            }
        }
        seen
    });
    for prompt in prompts {
        let turn = coordinator
            .request_agent_turn(EventActor::new(ActorKind::User, None), &agent, *prompt)
            .await?;
        settled(&coordinator, &turn).await?;
    }
    let events = crate::store::read_events(&info.events_path)?;
    coordinator.stop_run().await?;
    let lifecycle = counter.await?;
    Ok(Run {
        provider,
        events,
        lifecycle,
    })
}

fn compactions(events: &[EventEnvelopeV1]) -> Vec<&SessionCompactionEvent> {
    events
        .iter()
        .filter_map(|event| match &event.payload {
            EventV1::SessionCompaction(compaction) => Some(compaction),
            _ => None,
        })
        .collect()
}

fn working(calls: Vec<Stream>) -> Vec<Stream> {
    let mut stream = vec![Stream::TextDelta("Working on the task. ".repeat(100))];
    stream.extend(calls);
    stream.push(Stream::Done { usage: None });
    stream
}

const REQUEST: &str = "Implement the task; preserve the public API and do not deploy.";

#[tokio::test]
async fn an_acknowledged_request_rolls_over_once_below_the_threshold_whatever_auto_compact_says(
) -> Result<(), Box<dyn std::error::Error>> {
    for (name, enabled, request, calls, boundaries) in [
        ("explicit request", true, true, 3, 1),
        ("auto-compact off", false, true, 3, 1),
        ("no request", true, false, 2, 0),
    ] {
        let tools = if request {
            vec![
                call(
                    "notes",
                    "context_notes",
                    r#"{"text":"Notebook saved with the request."}"#,
                ),
                call("roll", "new_context", "{}"),
            ]
        } else {
            Vec::new()
        };
        let mut script = vec![answer("old result"), working(tools)];
        script.push(answer("done"));
        let Run {
            provider, events, ..
        } = run(script, &["old task", REQUEST], enabled).await?;
        assert_eq!(provider.call_count(), calls, "{name}");
        let found = compactions(&events);
        assert_eq!(found.len(), boundaries, "{name}");
        if !request {
            continue;
        }
        let boundary = found[0];
        assert_eq!(boundary.method, None, "{name}");
        assert_eq!(
            boundary.details,
            Some(json!({"kind": "notes_rollover", "version": 1})),
            "{name}"
        );
        assert!(
            boundary.summary.starts_with("<context-window-rollover>"),
            "{name}"
        );
        assert_eq!(boundary.short_summary, None, "{name}");
        assert!(boundary.read_files.is_empty() && boundary.modified_files.is_empty());
        assert!(
            boundary.tokens_after.is_some_and(|after| after > 0),
            "{name}"
        );

        // The window after the boundary: the notebook once, the request once, and nothing
        // that only the summarized history held.
        let sent = provider.captured_requests().await;
        let next = sent.last().ok_or("no request after the rollover")?;
        let texts: Vec<&str> = next.messages.iter().map(|m| m.content.as_str()).collect();
        assert_eq!(texts.iter().filter(|t| **t == REQUEST).count(), 1, "{name}");
        assert_eq!(
            texts
                .iter()
                .filter(|t| t.starts_with("<experimental-context-notes>")
                    && t.contains("Notebook saved with the request."))
                .count(),
            1,
            "{name}"
        );
        assert!(texts.iter().all(|t| !t.contains("old task")), "{name}");
        // The notebook leads the conversation, ahead of the rollover notice.
        let notes = texts
            .iter()
            .position(|t| t.starts_with("<experimental-context-notes>"));
        let notice = texts
            .iter()
            .position(|t| t.contains("<context-window-rollover>"));
        assert!(
            notes.is_some() && notice.is_some() && notes < notice,
            "{name}"
        );
    }
    Ok(())
}

#[tokio::test]
async fn the_near_threshold_reminder_is_delivered_once_per_window_and_nothing_is_summarized(
) -> Result<(), Box<dyn std::error::Error>> {
    // A 20,000-token threshold puts the lead band at [11,808, 20,000); the first prompt
    // alone lands inside it.
    let big = "word ".repeat(12_000);
    let script = vec![
        vec![call("look", "read", "{}"), Stream::Done { usage: None }],
        answer("done"),
        answer("again"),
    ];
    let Run {
        provider, events, ..
    } = run_with(script, &[&big, "continue"], true, |config| {
        config.compaction.mid_turn_enabled = true;
        config.compaction.threshold_tokens = std::num::NonZeroU32::new(20_000);
    })
    .await?;
    let reminders = events
        .iter()
        .filter(|event| {
            matches!(&event.payload, EventV1::RuntimeReminder(r) if r.kind == RuntimeReminderKind::NotesWindow)
        })
        .count();
    assert_eq!(reminders, 1);
    assert!(compactions(&events).is_empty());
    // Only the three turn requests happened: no speculative or summarizing request.
    assert_eq!(provider.call_count(), 3);
    let sent = provider.captured_requests().await;
    let delivered = sent
        .iter()
        .filter(|request| {
            request
                .messages
                .iter()
                .any(|m| m.content.contains("<context-window-reminder>"))
        })
        .count();
    assert!(delivered >= 1);
    Ok(())
}

#[tokio::test]
async fn a_rollover_with_nothing_to_roll_emits_no_lifecycle_and_no_boundary(
) -> Result<(), Box<dyn std::error::Error>> {
    // One short exchange: the keep-recent window already holds everything.
    let script = vec![
        working(vec![call("roll", "new_context", "{}")]),
        answer("done"),
    ];
    let Run {
        events, lifecycle, ..
    } = run_with(script, &[REQUEST], true, |config| {
        config.compaction.keep_recent_tokens = 90_000;
    })
    .await?;
    assert!(compactions(&events).is_empty());
    assert_eq!(lifecycle, 0);
    Ok(())
}
