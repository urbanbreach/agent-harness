use clap::ValueEnum;
use harness_core::{
    agent::AgentProfile,
    event::{ActorKind, EventActor},
    perm::PermissionPolicy,
};
use harness_providers::mock::MockProvider;
use serde_json::{json, Value};
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
#[value(rename_all = "snake_case")]
pub enum ScenarioName {
    GoldenPath,
    GoldenPathInteractive,
    QuestionInteractive,
    CompactionInteractive,
    CompactionNotesInteractive,
}
impl ScenarioName {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::GoldenPath => "golden_path",
            Self::GoldenPathInteractive => "golden_path_interactive",
            Self::QuestionInteractive => "question_interactive",
            Self::CompactionInteractive => "compaction_interactive",
            Self::CompactionNotesInteractive => "compaction_notes_interactive",
        }
    }
    /// Scenarios whose conversation is driven by the local compaction provider.
    pub fn is_compaction(self) -> bool {
        matches!(
            self,
            Self::CompactionInteractive | Self::CompactionNotesInteractive
        )
    }
    pub fn interactive_permissions(self) -> bool {
        self != Self::GoldenPath
    }
    pub fn is_question(self) -> bool {
        self == Self::QuestionInteractive
    }
}
pub fn deterministic_run_id(seed: u64, scenario: ScenarioName) -> String {
    let namespace = uuid::Uuid::new_v5(
        &uuid::Uuid::NAMESPACE_OID,
        format!("harness-seed:{seed}").as_bytes(),
    );
    format!(
        "run_{}",
        uuid::Uuid::new_v5(&namespace, scenario.as_str().as_bytes()).simple()
    )
}
pub fn create_workspace(
    session_dir: &Path,
    scenario: ScenarioName,
    run_id: Option<&str>,
) -> Result<PathBuf, String> {
    let id = run_id.map(str::to_owned).unwrap_or_else(|| {
        format!(
            "{}-{}-{}",
            scenario.as_str(),
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map_or(0, |d| d.as_nanos())
        )
    });
    harness_core::store::validate_session_id(&id).map_err(|e| e.to_string())?;
    // Demo workspaces sit beside session storage so tools cannot edit managed history.
    let workspace = session_dir.with_extension("workspaces").join(id);
    harness_core::store::create_private_dir(&workspace).map_err(|e| e.to_string())?;
    let demo = workspace.join("demo.txt");
    match std::fs::symlink_metadata(&demo) {
        Ok(metadata) if metadata.is_file() => {
            std::fs::remove_file(demo).map_err(|e| e.to_string())?
        }
        Ok(_) => return Err("demo.txt must be a regular generated file".into()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(e) => return Err(e.to_string()),
    }
    Ok(workspace)
}
pub fn supervisor_actor() -> EventActor {
    EventActor::new(ActorKind::Supervisor, None)
}
pub fn worker_actor(id: String) -> EventActor {
    EventActor::new(ActorKind::Worker, Some(id))
}
pub fn default_permission_policy() -> PermissionPolicy {
    PermissionPolicy::default()
}
pub fn golden_path_profiles() -> BTreeMap<String, AgentProfile> {
    let mut profile = AgentProfile::fallback("default");
    profile.model_ref = "mock:model-1".into();
    profile.max_iters = Some(8);
    profile.toolset = [
        "read",
        "write",
        "edit",
        "list",
        "glob",
        "grep",
        "bash",
        "apply_patch",
        "question",
    ]
    .map(str::to_owned)
    .into();
    BTreeMap::from([("default".into(), profile)])
}
pub fn golden_path_provider() -> MockProvider {
    MockProvider::default()
}
pub fn golden_path_edit_args() -> Value {
    json!({"filePath":"demo.txt", "oldString":"", "newString":"Hello world\n"})
}
pub fn question_interactive_request_json() -> Value {
    json!({"questions":[{"header":"Choice", "question":"Pick an option", "options":[{"label":"A","description":"First option"},{"label":"B","description":"Second option"}], "multiple":false,"custom":true}]})
}

pub(crate) fn reset_fixture(sessions: &Path, id: &str) -> Result<(), String> {
    let directory = sessions.join(id);
    if !directory.try_exists().map_err(|e| e.to_string())? {
        return Ok(());
    }
    let metadata = harness_core::proj::load_run_metadata(&directory)
        .ok_or("cannot replace a fixture with missing or invalid metadata")?;
    if metadata.mode_source != Some(harness_core::proj::SessionModeSource::ScenarioFixture)
        || metadata.run_id != id
    {
        return Err(
            "refusing to replace a session that is not a generated scenario fixture".into(),
        );
    }
    let _writer = harness_core::store::Journal::open_existing(sessions, id, true)
        .map_err(|e| e.to_string())?;
    std::fs::remove_dir_all(directory).map_err(|e| e.to_string())
}
/// A local provider whose responses exercise the coordinator's real maintenance path.
pub struct CompactionScenarioProvider;

#[async_trait::async_trait]
impl harness_providers::Provider for CompactionScenarioProvider {
    async fn stream_completion(
        &self,
        request: harness_providers::CompletionRequest,
    ) -> harness_providers::ProviderEventStream {
        use harness_providers::ProviderStreamEvent;
        let summary = request.messages.first().is_some_and(|message| {
            message.content == harness_compaction::prompts::summarization_system_prompt()
        });
        let short = summary
            && request.messages.iter().any(|message| {
                message
                    .content
                    .contains("Write exactly two or three first-person sentences")
            });
        let text = if short {
            "I preserved the synthetic checkpoint and its pending request. I kept the continuation ready for the next answer.".to_string()
        } else if summary {
            "## Goal
Continue the synthetic conversation after context maintenance.
## Constraints & Preferences
Use only generated public fixture text.
## Progress
### Done
- [x] Recorded the synthetic context seed.
### In Progress
- [ ] Answer the latest continuation request.
## Key Decisions
**Retain the checkpoint** so the next turn can continue.
## Next Steps
1. Answer the latest user request.
## Critical Context
The checkpoint marker is cedar-checkpoint.
## Additional Notes
No private files or credentials are involved."
                .to_string()
        } else if request
            .messages
            .iter()
            .any(|message| message.content.starts_with("Seed the synthetic context:"))
            && !request
                .messages
                .iter()
                .any(|message| message.role == harness_providers::MessageRole::Assistant)
        {
            format!(
                "Synthetic context recorded: cedar-checkpoint.\n{}",
                "Generated public answer record. ".repeat(20)
            )
        } else if request.messages.iter().any(|message| {
            message.content.contains("<summary>") && message.content.contains("cedar-checkpoint")
        }) {
            "Continued answer: cedar-checkpoint survived real coordinator compaction.".to_string()
        } else {
            "No compacted checkpoint was present in the provider request.".to_string()
        };
        let (sender, receiver) = tokio::sync::mpsc::channel(8);
        tokio::spawn(async move {
            if sender.send(ProviderStreamEvent::Start).await.is_err() {
                return;
            }
            // Keep the real live card observable without altering coordinator events. The pause
            // outlasts a screenshot, so a captured frame still shows the pass it was taken in.
            if summary {
                tokio::time::sleep(std::time::Duration::from_millis(3000)).await;
            }
            for chunk in text.as_bytes().chunks(if summary { 48 } else { 1024 }) {
                tokio::time::sleep(std::time::Duration::from_millis(if summary {
                    120
                } else {
                    80
                }))
                .await;
                if sender
                    .send(ProviderStreamEvent::TextDelta(
                        String::from_utf8_lossy(chunk).into_owned(),
                    ))
                    .await
                    .is_err()
                {
                    return;
                }
            }
            let _ = sender.send(ProviderStreamEvent::Done { usage: None }).await;
        });
        Box::pin(tokio_stream::wrappers::ReceiverStream::new(receiver))
    }
}

pub fn configure_compaction_scenario(
    config: &mut harness_core::coord::CoordinatorConfig,
    scenario: ScenarioName,
) {
    use harness_core::config::{ResolvedModelLimits, ResolvedModelTarget};
    config.provider = std::sync::Arc::new(CompactionScenarioProvider);
    config.interactive = true;
    let notes = scenario == ScenarioName::CompactionNotesInteractive;
    // The notes rollover needs its tools callable, so that scenario keeps the real registry.
    if !notes {
        config.tool_registry = std::sync::Arc::new(harness_core::tool::ToolRegistry::new());
    }
    let mut profile = AgentProfile::fallback("default");
    if notes {
        profile.toolset = ["read", "grep", "edit", "context_notes", "new_context"]
            .map(str::to_owned)
            .into();
    }
    config.agent_profiles.insert("default".into(), profile);
    config.compaction.notes_windows = notes;
    config.agent_model_targets.insert(
        "default".into(),
        ResolvedModelTarget {
            model_ref: "mock:default".into(),
            provider: "mock".into(),
            model: "default".into(),
            variant: None,
            reasoning_effort: None,
            text_verbosity: None,
            reasoning_summary: None,
            thinking: None,
            limits: ResolvedModelLimits::from_values(
                Some(64_000),
                Some(64_000),
                Some(8_192),
                Default::default(),
            ),
            resolution: Default::default(),
            catalog_entry: None,
        },
    );
    config.compaction.enabled = true;
    config.compaction.method_order = vec![harness_compaction::CompactionMethod::Soft];
    config.compaction.async_enabled = false;
    config.compaction.idle_enabled = false;
    config.compaction.threshold_tokens = std::num::NonZeroU32::new(5_000);
    config.compaction.keep_recent_tokens = 100;
    config.compaction.reserve_tokens = Some(0);
}
