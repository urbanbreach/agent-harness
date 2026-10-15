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

struct MediaSkills;
impl crate::config::SkillCatalogDiscovery for MediaSkills {
    fn discover(
        &self,
        cwd: &std::path::Path,
        _: &crate::config::SkillsConfig,
        _: Option<bool>,
    ) -> Result<crate::config::SkillCatalog, ToolError> {
        Ok(crate::config::SkillCatalog {
            entries: vec![crate::config::SkillCatalogEntry {
                stable_id: "image-review".into(),
                name: "image-review".into(),
                description: "Review an image".into(),
                source_scope: "project".into(),
                root_path: cwd.to_path_buf(),
                location: cwd.join("SKILL.md"),
                loadable: true,
                permission_mode: "allow".into(),
                status: crate::config::SkillCatalogStatus::Loadable,
                reason: None,
                argument_hint: None,
                allowed_tools: Vec::new(),
                deferred_mcp: None,
                deferred_resources: None,
                body_loaded: false,
                body_digest: None,
            }],
        })
    }
}

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
    config.skill_catalog_discovery = Some(Arc::new(MediaSkills));
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
async fn compaction_omits_summarized_images_and_keeps_recent_images_with_their_owner_after_resume(
) -> Result<(), Box<dyn std::error::Error>> {
    use super::compaction_tests::{answer, SUMMARY};
    let root = tempfile::tempdir()?;
    let provider = Arc::new(MockProvider::script([
        answer("old answer ".repeat(800)),
        answer("recent reply"),
        answer(SUMMARY),
        answer(SUMMARY),
        answer("Image checkpoint"),
        answer("continued"),
        answer("resumed"),
    ]));
    let mut config = CoordinatorConfig::new(root.path().join("sessions"));
    config.provider = Arc::clone(&provider) as Arc<dyn harness_providers::Provider>;
    // An image prices at 12,000 tokens on this model, so the recent turn needs that much.
    config.compaction.keep_recent_tokens = 13_000;
    config.compaction.reserve_tokens = Some(0);
    config.compaction.enabled = false;
    config.compaction.method_order = vec![harness_compaction::CompactionMethod::Soft];
    let settings = crate::config::load_config_from_str(
        r#"{ model: 'local/gpt-4.1', provider: { local: {
        type: 'openai_compatible', baseUrl: 'https://example.invalid',
        models: { 'gpt-4.1': { limit: { context: 16000, output: 1000 } } }
    } } }"#,
    )?;
    config.agent_model_targets.insert(
        "default".into(),
        crate::config::resolve_model_selection(&settings, "local/gpt-4.1", None)?.primary,
    );
    config.compaction.async_enabled = false;
    let mut profile = crate::agent::AgentProfile::fallback("default");
    profile.model_ref = "local:gpt-4.1".into();
    config.agent_profiles.insert("default".into(), profile);
    let coordinator = spawn_coordinator(
        config.clone(),
        Arc::new(FakeClock::new()),
        Arc::new(DefaultRedactor::default()),
    );
    let run = coordinator
        .start_run("compacted images", root.path())
        .await?;
    let actor = EventActor::new(ActorKind::User, None);
    let agent = coordinator
        .spawn_agent_idle(actor.clone(), "default", None)
        .await?;
    for (prompt, id) in [
        (
            format!("old image owner {}", "old request ".repeat(800)),
            "old-image",
        ),
        ("recent image owner".into(), "recent-image"),
    ] {
        let turn = coordinator
            .request_agent_turn_with_model_and_selected_tags_and_attachments(
                actor.clone(),
                &agent,
                prompt,
                crate::file_tag::SelectedPromptTags::default(),
                vec![crate::attachment_transport::AttachmentMetadata::from_bytes(
                    id,
                    "image/png",
                    None,
                    &base64::Engine::decode(&base64::engine::general_purpose::STANDARD, IMAGE)?,
                    None,
                )],
                None,
                None,
            )
            .await?;
        super::compaction_tests::settled(&coordinator, &turn).await?;
    }
    coordinator
        .compact_agent_context(&agent, None, "manual")
        .await?;
    let requests = provider.captured_requests().await;
    let summaries = &requests[2..];
    assert!(
        summaries.len() > 2,
        "fixture must fold multiple history windows"
    );
    assert!(summaries.iter().all(|r| r.attachments.is_empty()));
    assert!(summaries
        .iter()
        .flat_map(|r| &r.messages)
        .all(|m| !m.content.contains(IMAGE) && !m.content.contains("data:image")));
    let turn = coordinator
        .request_agent_turn(actor.clone(), &agent, "continue")
        .await?;
    super::compaction_tests::settled(&coordinator, &turn).await?;
    let requests = provider.captured_requests().await;
    let next = requests.last().ok_or("missing continuation")?;
    let (owner, images) = next.attachments.iter().next().ok_or("recent image lost")?;
    assert_eq!(next.attachments.len(), 1);
    assert_eq!(next.messages[*owner].content, "recent image owner");
    assert_eq!(images.len(), 1);
    assert_eq!(images[0].id, "recent-image");
    let kept = next.messages.clone();
    coordinator.stop_run().await?;
    let resumed = spawn_coordinator(
        config,
        Arc::new(FakeClock::new()),
        Arc::new(DefaultRedactor::default()),
    );
    resumed
        .resume_run(run.run_id.to_string(), "images resumed")
        .await?;
    let turn = resumed.request_agent_turn(actor, &agent, "resume").await?;
    super::compaction_tests::settled(&resumed, &turn).await?;
    let requests = provider.captured_requests().await;
    let next = requests.last().ok_or("missing resumed request")?;
    assert_eq!(&next.messages[..kept.len()], kept);
    let (owner, images) = next.attachments.iter().next().ok_or("resumed image lost")?;
    assert_eq!(next.messages[*owner].content, "recent image owner");
    assert_eq!(images.len(), 1);
    assert_eq!(images[0].id, "recent-image");
    resumed.stop_run().await?;
    Ok(())
}
