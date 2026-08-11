use harness_tools::UnwrapOrAbort;
use std::sync::Arc;

use async_trait::async_trait;
use harness_providers::{
    CompletionRequest, CompletionUsage, Provider, ProviderEventStream, ProviderStreamEvent,
};

#[derive(Debug)]
struct ReasoningProvider;

#[async_trait]
impl Provider for ReasoningProvider {
    async fn stream_completion(&self, _req: CompletionRequest) -> ProviderEventStream {
        Box::pin(tokio_stream::iter(vec![
            ProviderStreamEvent::Start,
            ProviderStreamEvent::ReasoningDelta("Let me think about this task.".to_string()),
            ProviderStreamEvent::ReasoningDelta(" The answer is straightforward.".to_string()),
            ProviderStreamEvent::TextDelta("reasoning child result".to_string()),
            ProviderStreamEvent::Done {
                usage: Some(CompletionUsage {
                    prompt_tokens: 1,
                    completion_tokens: 1,
                    total_tokens: 2,
                }),
            },
        ]))
    }
}

#[tokio::test]
async fn background_output_full_session_and_thinking_return_event_stream_and_artifact() {
    // arrange
    // act
    // assert
    let temp_dir = setup_workspace();
    let workspace = temp_dir.path().join("workspace");

    let (handle, run, worker_id) =
        spawn_run_with_provider(&workspace, Arc::new(ReasoningProvider)).await;

    let task_tool_call_id = handle
        .request_tool_call(
            worker_actor(&worker_id),
            Some("deep".to_string()),
            "task",
            json!({
                "description": "Reasoning background child",
                "prompt": "Think and return a result",
                "subagent_type": "general",
                "run_in_background": true,
                "load_skills": []
            }),
        )
        .await
        .unwrap_or_abort();
    wait_for_tool_call_finish(&run.events_path, &task_tool_call_id).await;

    let task_events = read_events(&run.events_path);
    let task_finished = find_finished(&task_events, &task_tool_call_id);
    let task_output = task_finished.output_json.unwrap_or_abort();
    let request_id = task_output["child_request_id"]
        .as_str()
        .unwrap_or_abort()
        .to_string();
    wait_for_request_terminal(&run.events_path, &request_id).await;

    let output_tool_call_id = handle
        .request_tool_call(
            worker_actor(&worker_id),
            Some("deep".to_string()),
            "background_output",
            json!({
                "request_id": request_id,
                "full_session": true,
                "include_thinking": true
            }),
        )
        .await
        .unwrap_or_abort();
    wait_for_tool_call_finish(&run.events_path, &output_tool_call_id).await;

    let events = read_events(&run.events_path);
    let finished = find_finished(&events, &output_tool_call_id);
    assert_eq!(finished.status, ToolCallStatus::Succeeded);
    let output = finished.output_json.unwrap_or_abort();

    assert_eq!(output["request_id"], json!(request_id));
    assert_eq!(output["status"], json!("completed"));
    assert_eq!(output["terminal"], json!(true));

    let full_session = &output["full_session"];
    assert!(
        full_session.is_object(),
        "full_session should be an object with event data"
    );
    assert!(
        full_session["event_count"].as_u64().unwrap_or(0) > 0,
        "full_session should contain events"
    );
    assert!(
        full_session["message_count"].as_u64().unwrap_or(0) > 0,
        "full_session should contain messages"
    );

    let thinking = &output["thinking"];
    assert!(
        thinking.is_object(),
        "thinking should be an object with artifact reference"
    );
    assert_eq!(thinking["spilled"], json!(true));
    assert!(
        thinking["block_count"].as_u64().unwrap_or(0) > 0,
        "thinking should have at least one block"
    );
    assert!(
        thinking["artifact"]["path"].as_str().is_some(),
        "thinking should reference an artifact path"
    );
    assert!(
        thinking["artifact"]["digest"].as_str().is_some(),
        "thinking artifact should have a digest"
    );

    assert!(
        !output
            .to_string()
            .contains("Let me think about this task"),
        "thinking content must not be persisted inline in output_json"
    );
}

#[tokio::test]
async fn background_output_default_behavior_unchanged_without_new_params() {
    // arrange
    // act
    // assert
    let temp_dir = setup_workspace();
    let workspace = temp_dir.path().join("workspace");

    let (handle, run, worker_id) =
        spawn_run_with_provider(&workspace, Arc::new(ReasoningProvider)).await;

    let task_tool_call_id = handle
        .request_tool_call(
            worker_actor(&worker_id),
            Some("deep".to_string()),
            "task",
            json!({
                "description": "Default behavior child",
                "prompt": "Return a result",
                "subagent_type": "general",
                "run_in_background": true,
                "load_skills": []
            }),
        )
        .await
        .unwrap_or_abort();
    wait_for_tool_call_finish(&run.events_path, &task_tool_call_id).await;

    let task_events = read_events(&run.events_path);
    let task_finished = find_finished(&task_events, &task_tool_call_id);
    let task_output = task_finished.output_json.unwrap_or_abort();
    let request_id = task_output["child_request_id"]
        .as_str()
        .unwrap_or_abort()
        .to_string();
    wait_for_request_terminal(&run.events_path, &request_id).await;

    let output_tool_call_id = handle
        .request_tool_call(
            worker_actor(&worker_id),
            Some("deep".to_string()),
            "background_output",
            json!({
                "request_id": request_id
            }),
        )
        .await
        .unwrap_or_abort();
    wait_for_tool_call_finish(&run.events_path, &output_tool_call_id).await;

    let events = read_events(&run.events_path);
    let finished = find_finished(&events, &output_tool_call_id);
    assert_eq!(finished.status, ToolCallStatus::Succeeded);
    let output = finished.output_json.unwrap_or_abort();

    assert_eq!(output["request_id"], json!(request_id));
    assert_eq!(output["status"], json!("completed"));
    assert_eq!(output["terminal"], json!(true));
    assert!(
        output.get("full_session").is_none() || output["full_session"].is_null(),
        "full_session should be absent when not requested"
    );
    assert!(
        output.get("thinking").is_none() || output["thinking"].is_null(),
        "thinking should be absent when not requested"
    );
}
