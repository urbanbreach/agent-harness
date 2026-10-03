use harness_core::{
    agent::AgentProfile,
    clock::FakeClock,
    config::ShellAllowlist,
    coord::{spawn_coordinator, CoordinatorConfig, CoordinatorHandle},
    event::{ActorKind, EventActor},
    perm::PermissionPolicy,
    redact::DefaultRedactor,
};
use harness_providers::{mock::MockProvider, CompletionRequest, Provider, ProviderEventStream};
use std::sync::Arc;

struct Gate {
    inner: MockProvider,
    started: tokio::sync::Notify,
    release: tokio::sync::Semaphore,
}

#[async_trait::async_trait]
impl Provider for Gate {
    async fn stream_completion(&self, request: CompletionRequest) -> ProviderEventStream {
        self.started.notify_one();
        if let Ok(permit) = self.release.acquire().await {
            permit.forget();
        }
        self.inner.stream_completion(request).await
    }
}

async fn setup(
    temp: &tempfile::TempDir,
    provider: Arc<dyn Provider>,
) -> Result<(CoordinatorHandle, EventActor), Box<dyn std::error::Error>> {
    let registry = harness_tools::coordinator_registry(ShellAllowlist::default());
    let mut parent = AgentProfile::fallback("default");
    parent.toolset = registry.tool_ids();
    let mut config = CoordinatorConfig::new(temp.path().join("sessions"));
    config.tool_registry = Arc::new(registry);
    config.provider = provider;
    config.permission_policy = PermissionPolicy::allow_all();
    config.agent_profiles.insert("default".into(), parent);
    let coordinator = spawn_coordinator(
        config,
        Arc::new(FakeClock::new()),
        Arc::new(DefaultRedactor::default()),
    );
    coordinator
        .start_run("public subagent adapters", temp.path())
        .await?;
    let parent = coordinator
        .spawn_agent_idle(EventActor::new(ActorKind::User, None), "default", None)
        .await?;
    Ok((
        coordinator,
        EventActor::new(ActorKind::Worker, Some(parent)),
    ))
}

#[path = "subagents/actor_schema.rs"]
mod actor_schema;
#[path = "subagents/background_lifecycle.rs"]
mod background_lifecycle;
#[path = "subagents/durable_projection.rs"]
mod durable_projection;
#[path = "subagents/native_output.rs"]
mod native_output;
#[path = "subagents/public_messages.rs"]
mod public_messages;
#[path = "subagents/schema_dispatch.rs"]
mod schema_dispatch;
#[path = "subagents/skill_startup.rs"]
mod skill_startup;
