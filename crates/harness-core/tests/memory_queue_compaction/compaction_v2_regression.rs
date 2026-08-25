use super::*;

use harness_core::session::legacy::LegacyEventLogAdapter;
use harness_core::session::SessionEntryPayload;

struct FileStateTool {
    id: &'static str,
}

#[async_trait]
impl Tool for FileStateTool {
    fn id(&self) -> &str {
        self.id
    }

    fn capability(&self) -> ToolCapability {
        match self.id {
            "read" => ToolCapability::ReadFs,
            _ => ToolCapability::EditFs,
        }
    }

    async fn call(
        &self,
        _context: ToolContext,
        _arguments: serde_json::Value,
    ) -> Result<ToolResult, ToolError> {
        Ok(ToolResult::text(format!("{} complete", self.id)))
    }
}

fn tool_events(id: &str, tool: &str, path: &str) -> Vec<ProviderStreamEvent> {
    vec![
        ProviderStreamEvent::Start,
        ProviderStreamEvent::ToolCallComplete {
            tool_call_id: id.to_string(),
            function_name: tool.to_string(),
            arguments_json: serde_json::json!({"path": path}).to_string(),
        },
        ProviderStreamEvent::Done {
            usage: Some(CompletionUsage {
                prompt_tokens: 100,
                completion_tokens: 100,
                total_tokens: 200,
            }),
        },
    ]
}

async fn wait_for_turn(coordinator: &CoordinatorHandle, agent_id: &str, prompt: &str) {
    let store = coordinator.event_store().await.unwrap_or_abort();
    let mut events = store.subscribe(1).unwrap_or_abort();
    let request_id = coordinator
        .request_agent_turn(supervisor_actor(), agent_id, prompt)
        .await
        .unwrap_or_abort();
    tokio::time::timeout(Duration::from_secs(2), async {
        loop {
            let event = events.next().await.unwrap_or_abort().unwrap_or_abort();
            if event.correlation_id.as_deref() == Some(request_id.as_str())
                && matches!(
                    event.payload,
                    EventV1::TaskCompleted(_) | EventV1::TaskCancelled(_)
                )
            {
                break;
            }
        }
    })
    .await
    .unwrap_or_abort();
}

async fn runtime_state_after_reopen() -> (
    harness_core::event::SessionCompactionEvent,
    harness_core::session::CompactionPreservedState,
) {
    let shared_path = "/workspace/shared.rs";
    let read_only_path = "/workspace/read_only.rs";
    let provider = SequentialScriptedProvider::new(vec![
        tool_events("read-shared", "read", shared_path),
        provider_text_events("shared read complete"),
        tool_events("read-only", "read", read_only_path),
        provider_text_events("read-only complete"),
        provider_text_events("first runtime summary"),
        tool_events("edit-shared", "edit", shared_path),
        provider_text_events("shared edit complete"),
        provider_text_events("post-edit answer"),
        provider_text_events("second runtime summary"),
    ]);
    let mut registry = ToolRegistry::new();
    for id in ["read", "edit"] {
        registry.register(Arc::new(FileStateTool { id }));
    }
    let temp_dir = tempfile::tempdir().unwrap_or_abort();
    let coordinator = test_agent_tool_coordinator_with_compaction(
        temp_dir.path(),
        Arc::new(provider),
        Arc::new(registry),
        allow_all_permission_policy(),
        vec!["read".to_string(), "edit".to_string()],
        8,
        CompactionRuntimeConfig::default(),
    );
    let run = coordinator
        .start_run("compaction-v2-durable-state", PathBuf::from("/workspace"))
        .await
        .unwrap_or_abort();
    let agent_id = coordinator
        .spawn_agent_idle(supervisor_actor(), "alpha", None)
        .await
        .unwrap_or_abort();

    coordinator
        .record_ui_intent(
            agent_id.clone(),
            "initial_task",
            BTreeMap::from([("todo".to_string(), "inspect files".to_string())]),
        )
        .await
        .unwrap_or_abort();
    wait_for_turn(&coordinator, &agent_id, "inspect shared").await;
    wait_for_turn(&coordinator, &agent_id, "inspect read-only").await;
    coordinator
        .compact_agent_context(agent_id.clone(), None, "manual")
        .await
        .unwrap_or_abort();
    coordinator
        .record_ui_intent(
            agent_id.clone(),
            "continue_task_12",
            BTreeMap::from([("todo".to_string(), "verify reopen".to_string())]),
        )
        .await
        .unwrap_or_abort();
    wait_for_turn(&coordinator, &agent_id, "edit shared").await;
    wait_for_turn(&coordinator, &agent_id, "continue after edit").await;
    coordinator
        .compact_agent_context(agent_id, None, "manual")
        .await
        .unwrap_or_abort();
    coordinator.stop_run().await.unwrap_or_abort();

    let events = load_events(&run.events_path);
    let written = events
        .iter()
        .rev()
        .find_map(|event| match &event.payload {
            EventV1::SessionCompaction(payload) => Some(payload.clone()),
            _ => None,
        })
        .unwrap_or_abort();
    let reopened = LegacyEventLogAdapter::new()
        .project(&events)
        .unwrap_or_abort();
    let preserved = reopened
        .session
        .entries()
        .values()
        .filter_map(|entry| match &entry.payload {
            SessionEntryPayload::CompactionSummary {
                preserved_state: Some(state),
                ..
            } => Some(state.as_ref().clone()),
            _ => None,
        })
        .next_back()
        .unwrap_or_abort();
    (written, preserved)
}

#[tokio::test]
async fn compaction_v2_current_intent_survives_summary() {
    let (written, reopened) = runtime_state_after_reopen().await;
    assert_eq!(
        written
            .current_intent
            .as_ref()
            .map(|intent| intent.intent.as_str()),
        Some("continue_task_12")
    );
    assert_eq!(reopened.current_intent, written.current_intent);
}

#[tokio::test]
async fn compaction_v2_file_state_survives_summary() {
    let (written, reopened) = runtime_state_after_reopen().await;
    assert_eq!(written.read_files, ["/workspace/read_only.rs"]);
    assert_eq!(written.modified_files, ["/workspace/shared.rs"]);
    assert_eq!(reopened.read_files, written.read_files);
    assert_eq!(reopened.modified_files, written.modified_files);
}
