use super::*;
use crate::{
    clock::FakeClock,
    redact::DefaultRedactor,
    tool::{Tool, ToolCapability, ToolContext, ToolError, ToolRegistry},
};
use harness_providers::{mock::MockProvider, MessageRole, ProviderStreamEvent as Stream};
use serde_json::{json, Value};
use std::sync::atomic::{AtomicUsize, Ordering};

const IMAGE: &str =
    "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAIAAACQd1PeAAAADElEQVR4nGP4z8AAAAMBAQDJ/pLvAAAAAElFTkSuQmCC";

struct MediaTool(Arc<AtomicUsize>);
#[async_trait::async_trait]
impl Tool for MediaTool {
    fn id(&self) -> &str {
        "media"
    }
    fn parameters_json_schema(&self) -> Value {
        json!({"type":"object"})
    }
    fn capability(&self) -> ToolCapability {
        ToolCapability::ReadFs
    }
    async fn call(&self, _: ToolContext, _: Value) -> Result<ToolResult, ToolError> {
        self.0.fetch_add(1, Ordering::Relaxed);
        use base64::Engine;
        let png = base64::engine::general_purpose::STANDARD
            .decode(IMAGE)
            .map_err(|_| ToolError::Execution("invalid fixture".into()))?;
        Ok(ToolResult::text("Image returned.").with_attachments(vec![
            crate::attachment_transport::AttachmentMetadata::from_bytes(
                "picture",
                "image/png",
                None,
                &png,
                None,
            ),
        ]))
    }
}

#[tokio::test]
async fn tool_media_reaches_the_provider_and_survives_resume_without_reexecution(
) -> Result<(), Box<dyn std::error::Error>> {
    let root = tempfile::tempdir()?;
    let calls = Arc::new(AtomicUsize::new(0));
    let mut registry = ToolRegistry::new();
    registry.register(Arc::new(MediaTool(Arc::clone(&calls))));
    let answer = || {
        vec![
            Stream::TextDelta("Seen.".into()),
            Stream::Done { usage: None },
        ]
    };
    let provider = Arc::new(MockProvider::script([
        vec![
            Stream::ToolCallComplete {
                tool_call_id: "picture".into(),
                function_name: "media".into(),
                arguments_json: "{}".into(),
            },
            Stream::Done { usage: None },
        ],
        answer(),
        answer(),
        answer(),
    ]));
    let mut config = CoordinatorConfig::new(root.path().join("sessions"));
    config.provider = Arc::clone(&provider) as Arc<dyn harness_providers::Provider>;
    config.tool_registry = Arc::new(registry);
    config.permission_policy = PermissionPolicy::allow_all();
    let mut profile = crate::agent::AgentProfile::fallback("default");
    profile.model_ref = "mock:gpt-4.1".into();
    profile.toolset = vec!["media".into()];
    config.agent_profiles.insert("default".into(), profile);
    let coordinator = spawn_coordinator(
        config.clone(),
        Arc::new(FakeClock::new()),
        Arc::new(DefaultRedactor::default()),
    );
    let run = coordinator.start_run("media", root.path()).await?;
    let actor = EventActor::new(ActorKind::User, None);
    let agent = coordinator
        .spawn_agent_idle(actor.clone(), "default", None)
        .await?;
    let first = coordinator
        .request_agent_turn(actor.clone(), &agent, "Read an image.")
        .await?;
    super::history_tests::settled(&coordinator, &first).await?;
    let requests = provider.captured_requests().await;
    let request = requests.last().ok_or("no provider continuation")?;
    let index = request
        .messages
        .iter()
        .position(|m| m.role == MessageRole::Tool)
        .ok_or("no tool result")?;
    assert_eq!(
        request.attachments.get(&index).map(Vec::len),
        Some(1),
        "the tool image must accompany its result"
    );
    let original = request.attachments[&index][0].clone();
    assert_eq!(original.mime, "image/png");
    coordinator.stop_run().await?;
    assert!(!std::fs::read_to_string(&run.events_path)?.contains(IMAGE));
    let resumed = spawn_coordinator(
        config,
        Arc::new(FakeClock::new()),
        Arc::new(DefaultRedactor::default()),
    );
    resumed
        .resume_run(run.run_id.to_string(), "resumed")
        .await?;
    assert_eq!(calls.load(Ordering::Relaxed), 1);
    assert_eq!(provider.call_count(), 2);
    let next = resumed
        .request_agent_turn(actor.clone(), &agent, "Describe that image again.")
        .await?;
    super::history_tests::settled(&resumed, &next).await?;
    let requests = provider.captured_requests().await;
    let request = requests.last().ok_or("no resumed provider request")?;
    let restored = request
        .attachments
        .values()
        .flatten()
        .next()
        .ok_or("tool media was lost on resume")?;
    assert_eq!(restored.bytes()?, original.bytes()?);
    resumed.rewind_conversation(first).await?;
    let next = resumed
        .request_agent_turn(actor, &agent, "Start again.")
        .await?;
    super::history_tests::settled(&resumed, &next).await?;
    assert!(provider
        .captured_requests()
        .await
        .last()
        .ok_or("request missing")?
        .attachments
        .is_empty());
    assert_eq!(calls.load(Ordering::Relaxed), 1);
    resumed.stop_run().await?;
    Ok(())
}

#[tokio::test]
async fn chunked_compaction_sends_each_attachment_with_its_owner_only_once(
) -> Result<(), Box<dyn std::error::Error>> {
    use super::compaction_tests::{answer, SUMMARY};
    let root = tempfile::tempdir()?;
    // JSON escaping makes the summary transcript larger than the original request.
    let provider = Arc::new(MockProvider::script(
        [answer("\"".repeat(8000)), answer("recent reply")]
            .into_iter()
            .chain(std::iter::repeat_n(answer(SUMMARY), 20)),
    ));
    let mut config = CoordinatorConfig::new(root.path().join("sessions"));
    config.provider = Arc::clone(&provider) as Arc<dyn harness_providers::Provider>;
    config.compaction.keep_recent_tokens = 1;
    config.compaction.reserve_tokens = 0;
    config.compaction.fallback_input_tokens = 5000;
    config.compaction.suppress_auto_compaction = true;
    let coordinator = spawn_coordinator(
        config,
        Arc::new(FakeClock::new()),
        Arc::new(DefaultRedactor::default()),
    );
    coordinator.start_run("chunked media", root.path()).await?;
    let actor = EventActor::new(ActorKind::User, None);
    let agent = coordinator
        .spawn_agent_idle(actor.clone(), "default", None)
        .await?;
    let first = coordinator
        .request_agent_turn_with_model_and_selected_tags_and_attachments(
            actor.clone(),
            &agent,
            format!("first attachment owner {}", "\"".repeat(8000)),
            crate::file_tag::SelectedPromptTags::default(),
            vec![crate::attachment_transport::AttachmentMetadata::from_bytes(
                "one-note",
                "text/plain",
                None,
                b"Only belongs to the first entry.",
                None,
            )],
            None,
            None,
        )
        .await?;
    super::history_tests::settled(&coordinator, &first).await?;
    let recent = coordinator
        .request_agent_turn(actor, &agent, "recent request")
        .await?;
    super::history_tests::settled(&coordinator, &recent).await?;
    coordinator
        .compact_agent_context(&agent, None, "manual")
        .await?;
    let requests = provider.captured_requests().await;
    let chunks = &requests[2..];
    assert!(chunks.len() > 1, "fixture must require chunking");
    let attached: Vec<_> = chunks
        .iter()
        .filter(|r| !r.attachments.is_empty())
        .collect();
    assert_eq!(
        attached.len(),
        1,
        "attachments must not be copied into every chunk"
    );
    assert!(attached[0].messages[1]
        .content
        .contains("first attachment owner"));
    assert_eq!(attached[0].attachments[&1][0].id, "one-note");
    coordinator.stop_run().await?;
    Ok(())
}
