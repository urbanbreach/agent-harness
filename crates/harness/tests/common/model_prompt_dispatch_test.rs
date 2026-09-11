use super::*;
use async_trait::async_trait;
use harness_core::clock::FakeClock;
use harness_core::coord::spawn_coordinator;
use harness_core::event::{ActorKind, EventActor, EventV1};
use harness_core::model_resolution::{
    configured_prompt_override, effective_prompt_status, PromptFamily,
};
use harness_core::redact::DefaultRedactor;
use harness_providers::{
    CompletionRequest, MessageRole, Provider, ProviderErrorCategory, ProviderEventStream,
    ProviderStreamEvent,
};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio_stream::StreamExt;

struct CapturingProvider(Mutex<Vec<CompletionRequest>>);

#[async_trait]
impl Provider for CapturingProvider {
    fn request_budget_semantics(
        &self,
        request: &CompletionRequest,
        pending_prompt_index: usize,
    ) -> Result<
        harness_providers::ProviderBudgetSemantics,
        harness_providers::ProviderRequestCostError,
    > {
        harness_providers::generic_request_budget_semantics(request, pending_prompt_index)
    }

    async fn stream_completion(&self, request: CompletionRequest) -> ProviderEventStream {
        let mut requests = self.0.lock().unwrap_or_abort();
        let first = requests.is_empty();
        requests.push(request);
        let events = if first {
            vec![ProviderStreamEvent::categorized_error(
                "fixture primary unavailable",
                ProviderErrorCategory::MissingCredentials,
            )]
        } else {
            vec![
                ProviderStreamEvent::Start,
                ProviderStreamEvent::TextDelta("ANSWER_SENTINEL".to_string()),
                ProviderStreamEvent::Done { usage: None },
            ]
        };
        Box::pin(tokio_stream::iter(events))
    }
}

fn completed_summary(
    event: harness_core::event::EventEnvelopeV1,
    request_id: &str,
) -> Option<String> {
    if event.correlation_id.as_deref() != Some(request_id) {
        return None;
    }
    match event.payload {
        EventV1::TaskCompleted(completed) => Some(completed.result_summary),
        _ => None,
    }
}

fn fixture() -> HarnessConfig {
    let mut config = load_config_from_str(r#"{
        provider: { local: { type: "openai_compatible", baseURL: "http://127.0.0.1:1/v1", apiKey: "fixture", models: {
            "gpt-6-astra": { name: "GPT", limit: { context: 128000, output: 4096 } },
            "llama-4": { name: "Meta", limit: { context: 128000, output: 4096 } },
            "claude-sonnet-4": { name: "Claude", limit: { context: 128000, output: 4096 } },
            "gemini-2.5-pro": { name: "Gemini", limit: { context: 128000, output: 4096 } },
            "unknown-model": { name: "Unknown", limit: { context: 128000, output: 4096 } },
            "opaque-catalog": { name: "Metadata", metadata: { family: "gemini" }, limit: { context: 128000, output: 4096 } }
        } } },
        model: "work",
        model_profile: { work: { model: "local:gpt-6-astra", fallback: [{ model: "local:claude-sonnet-4" }] } },
        agent: { default: { model: "work", tools: ["read", "skill"] } },
        permission: "deny"
    }"#).unwrap_or_abort();
    config.instruction_files = vec![harness_core::config::InstructionFile {
        path: PathBuf::from("PROJECT_SENTINEL.md"),
        content: "PROJECT_SENTINEL".to_string(),
    }];
    config
}

#[tokio::test]
async fn actual_dispatch_recomposes_base_for_fallback_and_live_model_switches() {
    let temp = tempdir().unwrap_or_abort();
    let mut config = fixture();
    config.paths.session_dir = temp.path().join("sessions");
    let provider = Arc::new(CapturingProvider(Mutex::new(Vec::new())));
    let mut runtime = bootstrap::build_interactive_coordinator_config(&config).unwrap_or_abort();
    runtime.provider = Arc::clone(&provider) as Arc<dyn Provider>;
    runtime.deterministic_store = true;
    runtime
        .agent_prompt_templates
        .get_mut("default")
        .unwrap_or_abort()
        .extra_rules = Some("RULES_SENTINEL".to_string());
    let coordinator = spawn_coordinator(
        runtime,
        Arc::new(FakeClock::new()),
        Arc::new(DefaultRedactor::default()),
    );
    coordinator
        .start_run("model_prompt_dispatch", temp.path())
        .await
        .unwrap_or_abort();
    let actor = EventActor::new(ActorKind::Supervisor, None);
    let agent = coordinator
        .spawn_agent_idle(actor.clone(), "default", None)
        .await
        .unwrap_or_abort();
    let store = coordinator.event_store().await.unwrap_or_abort();

    let selections = [
        None,
        Some("gpt-6-astra"),
        Some("llama-4"),
        Some("gemini-2.5-pro"),
        Some("unknown-model"),
        Some("opaque-catalog"),
    ];
    for selection in selections {
        let events = store.subscribe(0).unwrap_or_abort();
        let request_id = if let Some(model) = selection {
            let target = harness_core::config::resolve_model_selection(
                &config,
                &format!("local:{model}"),
                None,
            )
            .unwrap_or_abort()
            .primary;
            coordinator
                .request_agent_turn_with_model_target(
                    actor.clone(),
                    agent.clone(),
                    "USER_SENTINEL",
                    target,
                )
                .await
                .unwrap_or_abort()
        } else {
            coordinator
                .request_agent_turn(actor.clone(), agent.clone(), "USER_SENTINEL")
                .await
                .unwrap_or_abort()
        };
        let mut completions =
            events.filter_map(|event| completed_summary(event.unwrap_or_abort(), &request_id));
        let summary = tokio::time::timeout(Duration::from_secs(5), completions.next())
            .await
            .unwrap_or_abort()
            .unwrap_or_abort();
        assert_eq!(summary, "ANSWER_SENTINEL");
    }
    coordinator.stop_run().await.unwrap_or_abort();

    let requests = provider.0.lock().unwrap_or_abort();
    let expected = [
        ("gpt-6-astra", PromptFamily::Gpt),
        ("claude-sonnet-4", PromptFamily::Anthropic),
        ("gpt-6-astra", PromptFamily::Gpt),
        ("llama-4", PromptFamily::Meta),
        ("gemini-2.5-pro", PromptFamily::Gemini),
        ("unknown-model", PromptFamily::Default),
        ("opaque-catalog", PromptFamily::Gemini),
    ];
    assert_eq!(requests.len(), expected.len());
    for (request, (model, family)) in requests.iter().zip(expected) {
        assert_eq!(request.model_id, model);
        let system = request
            .messages
            .iter()
            .find(|message| message.role == MessageRole::System)
            .unwrap_or_abort();
        assert!(
            system.content.starts_with(family.bundled_prompt()),
            "{model}"
        );
        assert!(system.content.contains(&format!("local/{model}")));
        assert!(system.content.contains("PROJECT_SENTINEL"));
        assert!(system.content.ends_with("RULES_SENTINEL"));
        assert!(system.content.contains(
            harness_core::model_resolution::shipped_agent_prompt("default").unwrap_or_abort()
        ));
    }
}

#[test]
fn configured_override_survives_recomposition_and_status_matches_effective_base() {
    let mut config = fixture();
    config
        .agents
        .get_mut("default")
        .unwrap_or_abort()
        .system_prompt = Some("OVERRIDE_SENTINEL".to_string());
    let runtime = bootstrap::build_interactive_coordinator_config(&config).unwrap_or_abort();
    let template = &runtime.agent_prompt_templates["default"];
    for model in [
        "gpt-6-astra",
        "llama-4",
        "claude-sonnet-4",
        "gemini-2.5-pro",
        "unknown-model",
    ] {
        let target =
            harness_core::config::resolve_model_selection(&config, &format!("local:{model}"), None)
                .unwrap_or_abort()
                .primary;
        let prompt = template.compose(&target, true);
        assert!(prompt.starts_with("OVERRIDE_SENTINEL\n\n"));
        assert!(prompt.contains("PROJECT_SENTINEL"));
        let status = effective_prompt_status(
            target.resolution.prompt_family,
            configured_prompt_override(
                "default",
                config.agents["default"].system_prompt.as_deref(),
            ),
            Path::new("/"),
        );
        assert_eq!(status.source, "configured_prompt");
        assert_eq!(status.family, target.resolution.prompt_family.id());
    }
}
