//! Manual passes through the coordinator: ownership, rejection and the interrupted-turn resume.
use super::super::super::{compaction_tests::settled, handle, *};
use crate::{
    clock::FakeClock,
    redact::DefaultRedactor,
    tool::{Tool, ToolCapability, ToolContext, ToolError, ToolRegistry, ToolResult},
};
use harness_providers::{mock::MockProvider, ProviderStreamEvent as Stream};
use serde_json::{json, Value};

/// Holds the turn inside a tool call until the test releases it, then reports it began.
struct Hold {
    entered: Arc<tokio::sync::Notify>,
    release: Arc<tokio::sync::Notify>,
}

#[async_trait::async_trait]
impl Tool for Hold {
    fn id(&self) -> &'static str {
        "hold"
    }
    fn parameters_json_schema(&self) -> Value {
        json!({"type":"object"})
    }
    fn capability(&self) -> ToolCapability {
        ToolCapability::ReadFs
    }
    async fn call(&self, _: ToolContext, _: Value) -> Result<ToolResult, ToolError> {
        self.entered.notify_one();
        self.release.notified().await;
        Ok(ToolResult::text("released"))
    }
}

fn hold_call() -> Vec<Stream> {
    vec![
        Stream::ToolCallComplete {
            tool_call_id: "hold-1".into(),
            function_name: "hold".into(),
            arguments_json: "{}".into(),
        },
        Stream::Done { usage: None },
    ]
}

fn answer(text: &str) -> Vec<Stream> {
    vec![Stream::TextDelta(text.into()), Stream::Done { usage: None }]
}

struct Fixture {
    coordinator: CoordinatorHandle,
    provider: Arc<MockProvider>,
    agent: String,
    entered: Arc<tokio::sync::Notify>,
    release: Arc<tokio::sync::Notify>,
    _temp: tempfile::TempDir,
}

async fn fixture() -> Result<Fixture, Box<dyn std::error::Error>> {
    let temp = tempfile::tempdir()?;
    let (entered, release) = (
        Arc::new(tokio::sync::Notify::new()),
        Arc::new(tokio::sync::Notify::new()),
    );
    let provider = Arc::new(MockProvider::script([
        hold_call(),
        answer("one"),
        answer("two"),
        answer("three"),
        answer("four"),
    ]));
    let mut registry = ToolRegistry::new();
    registry.register(Arc::new(Hold {
        entered: Arc::clone(&entered),
        release: Arc::clone(&release),
    }));
    let mut config = CoordinatorConfig::new(temp.path().join("sessions"));
    config.provider = Arc::clone(&provider) as Arc<dyn harness_providers::Provider>;
    config.tool_registry = Arc::new(registry);
    config.permission_policy = PermissionPolicy::allow_all();
    let mut profile = crate::agent::AgentProfile::fallback("default");
    profile.toolset = vec!["hold".into()];
    config.agent_profiles.insert("default".into(), profile);
    let coordinator = spawn_coordinator(
        config,
        Arc::new(FakeClock::new()),
        Arc::new(DefaultRedactor::default()),
    );
    coordinator.start_run("manual passes", temp.path()).await?;
    let agent = coordinator
        .spawn_agent_idle(handle::system(), "default", None)
        .await?;
    Ok(Fixture {
        coordinator,
        provider,
        agent,
        entered,
        release,
        _temp: temp,
    })
}

impl Fixture {
    /// Whether any provider request carried the synthetic resume prompt. A probe turn queued
    /// after the pass finishes settles behind any resume the pass queued.
    async fn resumed(&self) -> Result<bool, Box<dyn std::error::Error>> {
        let probe = self
            .coordinator
            .request_agent_turn(handle::system(), self.agent.clone(), "probe")
            .await?;
        settled(&self.coordinator, &probe).await?;
        let prompt = harness_compaction::prompts::auto_continue_prompt();
        Ok(self
            .provider
            .captured_requests()
            .await
            .iter()
            .any(|r| r.messages.iter().any(|m| m.content.contains(prompt))))
    }
}

fn finished_without_commit(result: &Result<ManualCompactionOutcome, CoordinatorError>) -> bool {
    matches!(
        result,
        Ok(ManualCompactionOutcome::Compacted { .. })
            | Err(CoordinatorError::CompactionRejected(_))
    )
}

#[tokio::test]
async fn a_pass_resumes_only_a_turn_it_interrupted_while_streaming_and_not_when_suppressed() {
    // (a turn is running, caller suppresses continuation, a resume is queued)
    for (running, suppress, resumes) in [
        (true, false, true),
        (true, true, false),
        (false, false, false),
    ] {
        let fixture = fixture().await.expect("fixture");
        if running {
            fixture
                .coordinator
                .request_agent_turn(handle::system(), fixture.agent.clone(), "start")
                .await
                .expect("turn");
            fixture.entered.notified().await;
            // The tool ignores cancellation until released, so the pass queues behind it.
            let options = ManualCompactionOptions {
                reason: "manual".into(),
                suppress_continuation: suppress,
                ..Default::default()
            };
            let (result, ()) = tokio::join!(
                fixture
                    .coordinator
                    .compact_agent_context_with_options(fixture.agent.clone(), options),
                async { fixture.release.notify_one() }
            );
            assert!(finished_without_commit(&result), "{result:?}");
        } else {
            let result = fixture
                .coordinator
                .compact_agent_context(fixture.agent.clone(), None, "manual")
                .await;
            assert!(
                matches!(result, Err(CoordinatorError::CompactionRejected(_))),
                "{result:?}"
            );
        }
        // The probe turn's first scripted reply calls the tool; let it through.
        fixture.release.notify_one();
        let got = fixture
            .resumed()
            .await
            .unwrap_or_else(|e| panic!("probe running={running} suppress={suppress}: {e}"));
        assert_eq!(got, resumes, "running={running} suppress={suppress}");
        fixture.coordinator.stop_run().await.expect("stop");
    }
}

#[tokio::test]
async fn a_second_manual_pass_is_rejected_while_the_first_owns_the_agent() {
    let fixture = fixture().await.expect("fixture");
    fixture
        .coordinator
        .request_agent_turn(handle::system(), fixture.agent.clone(), "start")
        .await
        .expect("turn");
    fixture.entered.notified().await;
    // Both calls reach the coordinator in order: the first installs its pass before the
    // interrupted turn has finished tearing down, so the second finds it.
    let (first, second) = tokio::join!(
        fixture
            .coordinator
            .compact_agent_context(fixture.agent.clone(), None, "manual"),
        async {
            let second = fixture
                .coordinator
                .handoff_agent_context(fixture.agent.clone(), None)
                .await;
            fixture.release.notify_one();
            second
        }
    );
    assert!(
        matches!(
            second,
            Err(CoordinatorError::CompactionRejected(
                ManualRejection::InProgress
            ))
        ),
        "{second:?}"
    );
    assert!(finished_without_commit(&first), "{first:?}");
    fixture.coordinator.stop_run().await.expect("stop");
}
