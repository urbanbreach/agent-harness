//! Render cache reuse and invalidation, and the request path of a running coordinator.
use super::*;
use crate::coord::context::archive::inline_cache::InlineCache;
use crate::coord::turn::request::inline::analysis::analyze;
use crate::coord::*;
use crate::{
    clock::FakeClock,
    redact::DefaultRedactor,
    tool::{Tool, ToolCapability, ToolContext, ToolError, ToolRegistry, ToolResult},
};
use harness_providers::{mock::MockProvider, ProviderStreamEvent as Stream};
use std::sync::Arc;

type Test = Result<(), Box<dyn std::error::Error>>;

fn conversation() -> Vec<CompletionMessage> {
    vec![
        message(MessageRole::System, &text("sys", 20_000)),
        message(MessageRole::User, "go"),
        call("c1", "grep"),
        tool("c1", &text("big", 8_000)),
    ]
}

#[test]
fn identical_text_shape_and_sink_generation_render_once() {
    let counter = counter();
    let sink = Arc::new(FakeSink::new(&[]));
    let mut cache = with_sink(Arc::clone(&sink));
    let messages = conversation();
    let flags = facts(messages.len());
    let run = |env: &Env, cache: &mut InlineCache| {
        analyze(env, &messages, &Attachments::new(), &flags, cache, false);
        sink.calls()
    };
    let base = env(&counter, 90, ArchiveSystemPrompt::All, true);
    // One render for the system prompt and one for the tool result.
    assert_eq!(run(&base, &mut cache), 2);
    assert_eq!(run(&base, &mut cache), 2);
    // A new sink generation renders both again.
    sink.generation
        .store(1, std::sync::atomic::Ordering::SeqCst);
    assert_eq!(run(&base, &mut cache), 4);
    // The same shape id at another price is another render key.
    let mut repriced = env(&counter, 90, ArchiveSystemPrompt::All, true);
    repriced.shape = repriced.shape.with_frame_size(1024);
    assert_eq!(repriced.shape.id, base.shape.id);
    assert_eq!(run(&repriced, &mut cache), 6);
    // Another model's shape renders again too.
    let mut other = env(&counter, 90, ArchiveSystemPrompt::All, true);
    other.shape = *harness_compaction::archive::resolve_shape("gpt-5.5", "openai", None);
    assert_eq!(run(&other, &mut cache), 8);
}

#[test]
fn disabled_targets_and_vanished_calls_leave_the_caches() {
    let counter = counter();
    let mut cache = with_sink(Arc::new(FakeSink::new(&[])));
    let mut messages = conversation();
    let flags = facts(messages.len());
    let on = env(&counter, 90, ArchiveSystemPrompt::All, true);
    analyze(
        &on,
        &messages,
        &Attachments::new(),
        &flags,
        &mut cache,
        false,
    );
    assert!(cache.tools.contains_key("c1") && cache.system.is_some());
    // The call left the live history.
    messages.truncate(2);
    analyze(
        &on,
        &messages,
        &Attachments::new(),
        &facts(2),
        &mut cache,
        false,
    );
    assert!(cache.tools.is_empty());
    let tools_off = env(&counter, 90, ArchiveSystemPrompt::None, false);
    analyze(
        &tools_off,
        &conversation(),
        &Attachments::new(),
        &flags,
        &mut cache,
        false,
    );
    assert!(cache.system.is_none() && cache.tools.is_empty());
}

struct Bulk;
#[async_trait::async_trait]
impl Tool for Bulk {
    fn id(&self) -> &str {
        "bulk"
    }
    fn parameters_json_schema(&self) -> serde_json::Value {
        serde_json::json!({"type":"object"})
    }
    fn capability(&self) -> ToolCapability {
        ToolCapability::ReadFs
    }
    async fn call(&self, _: ToolContext, _: serde_json::Value) -> Result<ToolResult, ToolError> {
        Ok(ToolResult::text(
            "lorem ipsum dolor sit amet ".repeat(3_000),
        ))
    }
}

/// Runs a two request turn and returns the second request, which carries the tool result.
async fn second_request(
    vision: bool,
) -> Result<harness_providers::CompletionRequest, Box<dyn std::error::Error>> {
    let root = tempfile::tempdir()?;
    let mut registry = ToolRegistry::new();
    registry.register(Arc::new(Bulk));
    let provider = Arc::new(MockProvider::script([
        vec![
            Stream::ToolCallComplete {
                tool_call_id: "bulk-1".into(),
                function_name: "bulk".into(),
                arguments_json: "{}".into(),
            },
            Stream::Done { usage: None },
        ],
        vec![
            Stream::TextDelta("Done.".into()),
            Stream::Done { usage: None },
        ],
    ]));
    let mut config = CoordinatorConfig::new(root.path().join("sessions"));
    config.provider = Arc::clone(&provider) as Arc<dyn harness_providers::Provider>;
    config.tool_registry = Arc::new(registry);
    config.permission_policy = PermissionPolicy::allow_all();
    config.compaction.archive.tool_results = true;
    let modalities = if vision {
        "['text','image']"
    } else {
        "['text']"
    };
    let settings = crate::config::load_config_from_str(&format!(
        "{{ model: 'local/gpt-4.1', provider: {{ local: {{ type: 'openai_compatible', \
         baseUrl: 'https://example.invalid', models: {{ 'gpt-4.1': {{ \
         modalities: {{ input: {modalities}, output: ['text'] }}, \
         limit: {{ context: 128000, output: 1000 }} }} }} }} }} }}"
    ))?;
    config.agent_model_targets.insert(
        "default".into(),
        crate::config::resolve_model_selection(&settings, "local/gpt-4.1", None)?.primary,
    );
    let mut profile = crate::agent::AgentProfile::fallback("default");
    profile.model_ref = "local:gpt-4.1".into();
    profile.toolset = vec!["bulk".into()];
    config.agent_profiles.insert("default".into(), profile);
    let coordinator = spawn_coordinator(
        config,
        Arc::new(FakeClock::new()),
        Arc::new(DefaultRedactor::default()),
    );
    coordinator.start_run("inline", root.path()).await?;
    let actor = EventActor::new(ActorKind::User, None);
    let agent = coordinator
        .spawn_agent_idle(actor.clone(), "default", None)
        .await?;
    let turn = coordinator
        .request_agent_turn(actor, &agent, "Run it.")
        .await?;
    crate::coord::history_tests::settled(&coordinator, &turn).await?;
    let requests = provider.captured_requests().await;
    coordinator.stop_run().await?;
    requests
        .last()
        .cloned()
        .ok_or_else(|| "no second request".into())
}

#[tokio::test]
async fn a_vision_reader_gets_frames_and_a_text_only_reader_the_untouched_result() -> Test {
    let imaged = second_request(true).await?;
    let index = imaged
        .messages
        .iter()
        .position(|m| m.role == MessageRole::Tool)
        .ok_or("tool result missing")?;
    let frames = imaged.attachments.get(&index).ok_or("no frames")?;
    assert!(frames
        .iter()
        .all(|frame| frame.id.starts_with("inline-tool-")));
    assert!(imaged.message_content.contains_key(&index));
    assert!(!imaged.messages[index].content.contains("lorem ipsum"));

    let plain = second_request(false).await?;
    let index = plain
        .messages
        .iter()
        .position(|m| m.role == MessageRole::Tool)
        .ok_or("tool result missing")?;
    assert!(plain.attachments.is_empty() && plain.message_content.is_empty());
    assert!(plain.messages[index].content.contains("lorem ipsum"));
    Ok(())
}
