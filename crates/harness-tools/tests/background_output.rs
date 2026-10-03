use harness_core::{
    agent::AgentProfile,
    clock::FakeClock,
    config::ShellAllowlist,
    coord::{spawn_coordinator, CoordinatorConfig},
    event::{ActorKind, EventActor},
    perm::PermissionPolicy,
    redact::DefaultRedactor,
    subagent::{FinalizedStateResult, FinalizedStateUnavailable},
};
use harness_providers::{
    mock::MockProvider, CompletionUsage, ProviderStreamEvent as Stream,
    ProviderStreamFinishedMetadata,
};
use serde_json::json;
use std::sync::Arc;

#[path = "background_output/notifications.rs"]
mod notifications;
#[path = "background_output/progress.rs"]
mod progress;

fn done(reasoning: Vec<String>) -> Stream {
    Stream::DoneWithMetadata {
        usage: Some(CompletionUsage {
            prompt_tokens: 12,
            completion_tokens: 3,
            total_tokens: 15,
        }),
        metadata: Some(ProviderStreamFinishedMetadata {
            settled_reasoning: Some(reasoning),
            usage_complete: Some(true),
            ..Default::default()
        }),
    }
}

#[tokio::test]
async fn native_output_preserves_owned_redacted_child_history_and_reload_never_reruns_tools(
) -> Result<(), Box<dyn std::error::Error>> {
    let temp = tempfile::tempdir()?;
    std::fs::write(
        temp.path().join("note"),
        "child tool output credential-for-history-redaction",
    )?;
    let provider = Arc::new(MockProvider::script([
        vec![
            Stream::ReasoningDelta("synthetic settled reasoning marker".into()),
            Stream::ToolCallComplete {
                tool_call_id: "read-note".into(),
                function_name: "read".into(),
                arguments_json: json!({"filePath":"note"}).to_string(),
            },
            Stream::ToolCallComplete {
                tool_call_id: "write-note".into(),
                function_name: "write".into(),
                arguments_json: json!({"filePath":"child.txt","content":"created by the child\n"})
                    .to_string(),
            },
            done(vec!["synthetic settled reasoning marker".into()]),
        ],
        vec![
            Stream::TextDelta("report credential-for-history-redaction".into()),
            done(Vec::new()),
        ],
    ]));
    let registry = harness_tools::coordinator_registry(ShellAllowlist::default());
    let mut config = CoordinatorConfig::new(temp.path().join("sessions"));
    config.provider = Arc::clone(&provider) as Arc<dyn harness_providers::Provider>;
    config.tool_registry = Arc::new(registry);
    config.permission_policy = PermissionPolicy::allow_all();
    config
        .secret_values
        .push("credential-for-history-redaction".into());
    let mut parent = AgentProfile::fallback("default");
    parent.toolset = config.tool_registry.tool_ids();
    config.agent_profiles.insert("default".into(), parent);
    let coordinator = spawn_coordinator(
        config.clone(),
        Arc::new(FakeClock::new()),
        Arc::new(DefaultRedactor::default()),
    );
    let run = coordinator
        .start_run("native child history", temp.path())
        .await?;
    let parent = coordinator
        .spawn_agent_idle(EventActor::new(ActorKind::User, None), "default", None)
        .await?;
    let actor = EventActor::new(ActorKind::Worker, Some(parent));
    let output = coordinator
        .execute_agent_tool_call(
            actor.clone(),
            None,
            "spawn_subagent",
            json!({"prompt":"Read the note.","description":"Read child note","background":false}),
        )
        .await?;
    let child = output.structured_json.ok_or("missing child output")?["subagent_id"]
        .as_str()
        .ok_or("missing child id")?
        .to_owned();
    assert_eq!(
        std::fs::read_to_string(temp.path().join("child.txt"))?,
        "created by the child\n"
    );
    let child_dir = config.session_dir.join(&child);
    let child_events = harness_core::store::read_events(&child_dir.join("events.jsonl"))?;
    assert!(child_events
        .iter()
        .all(|event| event.run_id.as_str() == child));
    assert!(
        harness_core::store::Journal::open_existing(&config.session_dir, &child, false).is_err(),
        "the parent retains the only child projection writer"
    );
    for artifact in child_events
        .iter()
        .filter_map(|event| match &event.payload {
            harness_core::event::EventV1::ArtifactWritten(artifact) => Some(artifact),
            _ => None,
        })
    {
        let bytes = std::fs::read(child_dir.join(&artifact.path))?;
        assert_eq!(blake3::hash(&bytes).to_hex().as_str(), artifact.digest);
    }
    assert!(child_events
        .iter()
        .any(|event| matches!(event.payload, harness_core::event::EventV1::EditApplied(_))));
    let output = coordinator
        .execute_agent_tool_call(
            actor.clone(),
            None,
            "get_command_or_subagent_output",
            // Native unknown keys are ignored, not retained legacy history options.
            json!({"task_id":child,"timeout_ms":0,"full_session":true,"include_thinking":true}),
        )
        .await?;
    let data = output.structured_json.ok_or("missing native output")?;
    assert_eq!(data["Result"]["status"], "completed");
    assert_eq!(data["Result"]["exit_code"], 0);
    assert!(data["Result"]["output"]
        .as_str()
        .ok_or("output text")?
        .contains("report"));
    assert!(data.get("full_session").is_none());
    assert!(data.get("thinking").is_none());
    assert!(!data
        .to_string()
        .contains("credential-for-history-redaction"));
    assert!(!data
        .to_string()
        .contains("synthetic settled reasoning marker"));
    assert!(matches!(
        coordinator.raw_finalized_state(child.clone()).await?,
        FinalizedStateResult::Unavailable {
            reason: FinalizedStateUnavailable::PolicyModified
        }
    ));
    let other = coordinator
        .spawn_agent_idle(EventActor::new(ActorKind::User, None), "default", None)
        .await?;
    let foreign = coordinator
        .execute_agent_tool_call(
            EventActor::new(ActorKind::Worker, Some(other)),
            None,
            "get_command_or_subagent_output",
            json!({"task_ids":[child]}),
        )
        .await?;
    assert!(
        foreign.is_error(),
        "an unrelated parent must not obtain child output"
    );
    assert!(!foreign.display_text.contains("report"));
    coordinator.stop_run().await?;
    let root_prefix = std::fs::read(&run.events_path)?;
    let child_prefix = std::fs::read(child_dir.join("events.jsonl"))?;
    let metadata = harness_core::proj::read_run_metadata(&child_dir)?
        .map(|metadata| harness_core::proj::SessionCatalogMetadata::from(&metadata));
    let catalog = harness_core::proj::project_session_catalog_entry(
        &harness_core::store::read_events(&child_dir.join("events.jsonl"))?,
        &child,
        metadata.as_ref(),
        None,
        None,
    )?;
    assert_eq!(
        catalog.parent_session_id.as_deref(),
        Some(run.run_id.as_str())
    );
    for missing in [false, true] {
        if missing {
            std::fs::remove_dir_all(&child_dir)?;
        } else {
            use std::io::Write;
            std::fs::OpenOptions::new()
                .append(true)
                .open(child_dir.join("events.jsonl"))?
                .write_all(b"{\"")?;
        }
        let restored = spawn_coordinator(
            config.clone(),
            Arc::new(FakeClock::new()),
            Arc::new(DefaultRedactor::default()),
        );
        restored
            .resume_run(run.run_id.to_string(), "restore owned history")
            .await?;
        let output = restored
            .execute_agent_tool_call(
                actor.clone(),
                None,
                "get_command_or_subagent_output",
                json!({"task_ids":[child],"timeout_ms":0}),
            )
            .await?;
        assert_eq!(
            output.structured_json.ok_or("restored output")?["Result"]["status"],
            "completed"
        );
        assert_eq!(
            provider.call_count(),
            2,
            "inspection/reload must not replay tools"
        );
        restored.stop_run().await?;
        let recovered = std::fs::read(child_dir.join("events.jsonl"))?;
        assert!(recovered.starts_with(&child_prefix));
        assert!(std::fs::read(&run.events_path)?.starts_with(&root_prefix));
        harness_core::proj::project_resume_plan(
            &harness_core::store::read_events(&child_dir.join("events.jsonl"))?,
            &child,
        )?;
    }
    Ok(())
}
