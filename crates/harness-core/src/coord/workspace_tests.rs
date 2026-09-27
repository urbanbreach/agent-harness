use super::*;
use crate::{
    clock::FakeClock,
    redact::DefaultRedactor,
    tool::{Tool, ToolCapability, ToolContext, ToolError},
};
use harness_providers::{mock::MockProvider, ProviderStreamEvent as Stream};
use serde_json::{json, Value};
use std::{
    fs,
    sync::atomic::{AtomicBool, Ordering},
};

struct ChangeFiles;
#[async_trait::async_trait]
impl Tool for ChangeFiles {
    fn id(&self) -> &str {
        "change"
    }
    fn capability(&self) -> ToolCapability {
        ToolCapability::EditFs
    }
    fn parameters_json_schema(&self) -> Value {
        json!({"type":"object"})
    }
    fn filesystem_paths(&self, _: &Value) -> Result<Vec<PathBuf>, ToolError> {
        Ok(vec!["a".into(), "b".into()])
    }
    async fn call(&self, ctx: ToolContext, _: Value) -> Result<ToolResult, ToolError> {
        for name in ["a", "b"] {
            let path = ctx.workspace_root.join(name);
            let text = format!("edited {name}");
            let digest = blake3::hash(text.as_bytes()).to_hex().to_string();
            let receipt = ctx
                .coordinator
                .begin_tool_edit(
                    ctx.tool_call_id.to_string(),
                    path.clone(),
                    "fixture diff".into(),
                    Some(digest.clone()),
                )
                .await
                .map_err(|e| ToolError::Execution(e.to_string()))?;
            fs::write(path, text)?;
            ctx.coordinator
                .finish_tool_edit(ctx.tool_call_id.to_string(), receipt.edit_id, Ok(digest))
                .await
                .map_err(|e| ToolError::Execution(e.to_string()))?;
        }
        Ok(ToolResult::text("changed both files"))
    }
}
#[tokio::test]
async fn revert_rolls_back_its_writes_without_overwriting_concurrent_changes(
) -> Result<(), Box<dyn std::error::Error>> {
    let temp = tempfile::tempdir()?;
    for name in ["a", "b"] {
        fs::write(temp.path().join(name), format!("original {name}"))?;
    }
    let mut config = CoordinatorConfig::new(temp.path().join("sessions"));
    config.permission_policy = PermissionPolicy::allow_all();
    let mut registry = ToolRegistry::new();
    registry.register(Arc::new(ChangeFiles));
    config.tool_registry = Arc::new(registry);
    let mut profile = AgentProfile::fallback("default");
    profile.toolset = vec!["change".into()];
    config.agent_profiles.insert("default".into(), profile);
    config.provider = Arc::new(MockProvider::script([
        vec![
            Stream::ToolCallComplete {
                tool_call_id: "change".into(),
                function_name: "change".into(),
                arguments_json: "{}".into(),
            },
            Stream::Done { usage: None },
        ],
        vec![
            Stream::TextDelta("done".into()),
            Stream::Done { usage: None },
        ],
    ]));
    let coordinator = spawn_coordinator(
        config,
        Arc::new(FakeClock::new()),
        Arc::new(DefaultRedactor::default()),
    );
    let run = coordinator.start_run("rollback", temp.path()).await?;
    let agent = coordinator
        .spawn_agent_idle(EventActor::new(ActorKind::User, None), "default", None)
        .await?;
    let turn = coordinator
        .request_agent_turn(EventActor::new(ActorKind::User, None), agent, "change")
        .await?;
    super::history_tests::settled(&coordinator, &turn).await?;
    let fail_commit = Arc::new(AtomicBool::new(false));
    let (flag, changed) = (Arc::clone(&fail_commit), temp.path().join("b"));
    coordinator.call(move |s| {
        let inner = s.store.clone().ok_or(CoordinatorError::RunNotStarted)?;
        s.store = Some(Arc::new(super::tests::InterceptStore {inner, before_append:Box::new(move |event| {
            if matches!(&event.payload, EventV1::UiIntentReceived(e) if e.intent == "workspace_revert") && !flag.load(Ordering::SeqCst) {
                fs::write(&changed, "concurrent editor change")?;
            }
            if matches!(event.payload, EventV1::WorkspaceReverted(_)) && flag.load(Ordering::SeqCst) {
                return Err(EventStoreError::Io(std::io::Error::other("revert commit failed")));
            }
            Ok(())
        })}));
        Ok(())
    }).await?;
    assert!(coordinator.revert_workspace(&turn).await.is_err());
    assert_eq!(fs::read_to_string(temp.path().join("a"))?, "edited a");
    assert_eq!(
        fs::read_to_string(temp.path().join("b"))?,
        "concurrent editor change"
    );
    fs::write(temp.path().join("b"), "edited b")?;
    fail_commit.store(true, Ordering::SeqCst);
    assert!(coordinator.revert_workspace(&turn).await.is_err());
    for name in ["a", "b"] {
        assert_eq!(
            fs::read_to_string(temp.path().join(name))?,
            format!("edited {name}")
        );
    }
    assert!(!crate::store::read_events(&run.events_path)?
        .iter()
        .any(|e| matches!(e.payload, EventV1::WorkspaceReverted(_))));
    assert!(coordinator.stop_run().await.is_err());
    Ok(())
}
