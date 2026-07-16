//! T8: third identical tool call must Ask kind `doom_loop` before execute.

use harness_core::event::{EventEnvelopeV1, EventV1, ToolCallStatus};
use harness_core::perm::{PermissionDecision, PermissionGrantScope};
use harness_core::UnwrapOrAbort;
use serde_json::json;

mod common;

#[path = "common/oc_parity_permission_fixtures.rs"]
mod oc_parity;

use common::{load_events, supervisor_actor};
use oc_parity::{
    parity_coordinator, permission_kinds_for_tool_call, tool_finished_status, wait_for_tool_settled,
    KIND_DOOM_LOOP,
};

fn first_permission_id(events: &[EventEnvelopeV1], tool_call_id: &str) -> String {
    events
        .iter()
        .find_map(|event| match &event.payload {
            EventV1::PermissionRequested(data)
                if data.tool_call_id.as_ref().map(|id| id.as_str()) == Some(tool_call_id) =>
            {
                Some(data.permission_id.clone())
            }
            _ => None,
        })
        .unwrap_or_abort()
}

async fn wait_for_tool_finished(
    events_path: &std::path::Path,
    tool_call_id: &str,
    max_yields: usize,
) -> Vec<EventEnvelopeV1> {
    for _ in 0..max_yields {
        let events = load_events(events_path);
        if tool_finished_status(&events, tool_call_id).is_some() {
            return events;
        }
        tokio::task::yield_now().await;
    }
    load_events(events_path)
}

async fn request_bash_identical(
    coordinator: &harness_core::coord::CoordinatorHandle,
    args: &serde_json::Value,
) -> String {
    coordinator
        .request_tool_call(supervisor_actor(), None, "bash", args.clone())
        .await
        .unwrap_or_abort()
}

#[tokio::test]
async fn third_identical_tool_call_emits_doom_loop_ask() {
    // Given: allow-default shell and three identical bash calls (same tool id + args)
    // When: the third call is requested
    // Then: PermissionRequested kind `doom_loop` on the third only
    let temp_dir = tempfile::tempdir().unwrap_or_abort();
    let coordinator = parity_coordinator(temp_dir.path());
    let run = coordinator
        .start_run("oc_parity_doom_loop", temp_dir.path().to_path_buf())
        .await
        .unwrap_or_abort();

    let identical_args = json!({"command": "echo doom-loop-probe"});
    let mut tool_call_ids = Vec::new();
    for _ in 0..3 {
        let tool_call_id = request_bash_identical(&coordinator, &identical_args).await;
        let _ = wait_for_tool_settled(&run.events_path, &tool_call_id, 80).await;
        tool_call_ids.push(tool_call_id);
    }

    let events = load_events(&run.events_path);
    coordinator.stop_run().await.unwrap_or_abort();

    let first_kinds = permission_kinds_for_tool_call(&events, &tool_call_ids[0]);
    let second_kinds = permission_kinds_for_tool_call(&events, &tool_call_ids[1]);
    let third_kinds = permission_kinds_for_tool_call(&events, &tool_call_ids[2]);

    assert!(
        !first_kinds.iter().any(|k| k == KIND_DOOM_LOOP),
        "first identical call must not trigger doom_loop; got {first_kinds:?}"
    );
    assert!(
        !second_kinds.iter().any(|k| k == KIND_DOOM_LOOP),
        "second identical call must not trigger doom_loop; got {second_kinds:?}"
    );
    assert!(
        third_kinds.iter().any(|kind| kind == KIND_DOOM_LOOP),
        "third identical tool call (same tool id + args digest) must emit \
         PermissionRequested kind={KIND_DOOM_LOOP}; got third_kinds={third_kinds:?}; \
         finished={:?}",
        tool_finished_status(&events, &tool_call_ids[2])
    );
    assert_ne!(
        tool_finished_status(&events, &tool_call_ids[2]),
        Some(ToolCallStatus::Succeeded),
        "third call must not execute before doom_loop resolution"
    );
    assert_eq!(
        tool_finished_status(&events, &tool_call_ids[0]),
        Some(ToolCallStatus::Succeeded)
    );
    assert_eq!(
        tool_finished_status(&events, &tool_call_ids[1]),
        Some(ToolCallStatus::Succeeded)
    );
}

#[tokio::test]
async fn different_args_reset_doom_loop_streak() {
    // Given: two identical calls then a different-args call then two more of the first shape
    // When: five admissions with a mid-stream args change
    // Then: no doom_loop (streak never reaches 3 consecutive identical)
    let temp_dir = tempfile::tempdir().unwrap_or_abort();
    let coordinator = parity_coordinator(temp_dir.path());
    let run = coordinator
        .start_run("oc_parity_doom_loop_reset", temp_dir.path().to_path_buf())
        .await
        .unwrap_or_abort();

    let a = json!({"command": "echo streak-a"});
    let b = json!({"command": "echo streak-b"});
    let sequence = [&a, &a, &b, &a, &a];
    let mut tool_call_ids = Vec::new();
    for args in sequence {
        let tool_call_id = request_bash_identical(&coordinator, args).await;
        let _ = wait_for_tool_settled(&run.events_path, &tool_call_id, 80).await;
        tool_call_ids.push(tool_call_id);
    }

    let events = load_events(&run.events_path);
    coordinator.stop_run().await.unwrap_or_abort();

    for (index, tool_call_id) in tool_call_ids.iter().enumerate() {
        let kinds = permission_kinds_for_tool_call(&events, tool_call_id);
        assert!(
            !kinds.iter().any(|k| k == KIND_DOOM_LOOP),
            "call {index} must not emit doom_loop after args reset; got {kinds:?}"
        );
        assert_eq!(
            tool_finished_status(&events, tool_call_id),
            Some(ToolCallStatus::Succeeded),
            "call {index} should complete without doom_loop ask"
        );
    }
}

#[tokio::test]
async fn doom_loop_grant_once_allows_third_and_resets_streak() {
    // Given: three identical calls → doom_loop ask
    // When: resolve Allow once (no grant_scope)
    // Then: third executes; next two identical do not re-ask yet; sixth would ask again
    let temp_dir = tempfile::tempdir().unwrap_or_abort();
    let coordinator = parity_coordinator(temp_dir.path());
    let run = coordinator
        .start_run("oc_parity_doom_loop_grant_once", temp_dir.path().to_path_buf())
        .await
        .unwrap_or_abort();

    let identical_args = json!({"command": "echo grant-once"});
    let mut ids = Vec::new();
    for _ in 0..3 {
        let id = request_bash_identical(&coordinator, &identical_args).await;
        let _ = wait_for_tool_settled(&run.events_path, &id, 80).await;
        ids.push(id);
    }

    let events = load_events(&run.events_path);
    let permission_id = first_permission_id(&events, &ids[2]);
    coordinator
        .resolve_permission(permission_id, PermissionDecision::Allow, None)
        .await
        .unwrap_or_abort();
    let events = wait_for_tool_finished(&run.events_path, &ids[2], 80).await;
    assert_eq!(
        tool_finished_status(&events, &ids[2]),
        Some(ToolCallStatus::Succeeded),
        "allow once must execute the third call"
    );

    for _ in 0..2 {
        let id = request_bash_identical(&coordinator, &identical_args).await;
        let events = wait_for_tool_settled(&run.events_path, &id, 80).await;
        let kinds = permission_kinds_for_tool_call(&events, &id);
        assert!(
            !kinds.iter().any(|k| k == KIND_DOOM_LOOP),
            "post-once streak reset: first two must not re-ask; got {kinds:?}"
        );
        assert_eq!(
            tool_finished_status(&events, &id),
            Some(ToolCallStatus::Succeeded)
        );
        ids.push(id);
    }

    let sixth = request_bash_identical(&coordinator, &identical_args).await;
    let events = wait_for_tool_settled(&run.events_path, &sixth, 80).await;
    coordinator.stop_run().await.unwrap_or_abort();
    let kinds = permission_kinds_for_tool_call(&events, &sixth);
    assert!(
        kinds.iter().any(|k| k == KIND_DOOM_LOOP),
        "third identical after once-reset must ask doom_loop again; got {kinds:?}"
    );
}

#[tokio::test]
async fn doom_loop_always_grant_skips_subsequent_asks() {
    // Given: third identical asks doom_loop
    // When: resolve Allow with Run grant_scope (always)
    // Then: further identical calls do not re-ask
    let temp_dir = tempfile::tempdir().unwrap_or_abort();
    let coordinator = parity_coordinator(temp_dir.path());
    let run = coordinator
        .start_run(
            "oc_parity_doom_loop_grant_always",
            temp_dir.path().to_path_buf(),
        )
        .await
        .unwrap_or_abort();

    let identical_args = json!({"command": "echo grant-always"});
    let mut third_id = String::new();
    for i in 0..3 {
        let id = request_bash_identical(&coordinator, &identical_args).await;
        let _ = wait_for_tool_settled(&run.events_path, &id, 80).await;
        if i == 2 {
            third_id = id;
        }
    }

    let events = load_events(&run.events_path);
    let permission_id = first_permission_id(&events, &third_id);
    coordinator
        .resolve_permission_with_grant_scope(
            permission_id,
            PermissionDecision::Allow,
            None,
            Some(PermissionGrantScope::Run),
        )
        .await
        .unwrap_or_abort();
    let _ = wait_for_tool_finished(&run.events_path, &third_id, 80).await;

    for _ in 0..3 {
        let id = request_bash_identical(&coordinator, &identical_args).await;
        let events = wait_for_tool_settled(&run.events_path, &id, 80).await;
        let kinds = permission_kinds_for_tool_call(&events, &id);
        assert!(
            !kinds.iter().any(|k| k == KIND_DOOM_LOOP),
            "always grant must skip doom_loop re-ask; got {kinds:?}"
        );
        assert_eq!(
            tool_finished_status(&events, &id),
            Some(ToolCallStatus::Succeeded)
        );
    }

    coordinator.stop_run().await.unwrap_or_abort();
}

#[tokio::test]
async fn doom_loop_timeout_denies_third_without_execute() {
    // Given: short ask timeout and three identical calls
    // When: third call times out (PermissionTimedOut → deny)
    // Then: no ToolCallStarted/Succeeded for the third call
    use std::sync::Arc;

    use harness_core::clock::FakeClock;
    use harness_core::config::PermissionMode;
    use harness_core::coord::{spawn_coordinator, CoordinatorConfig};
    use harness_core::perm::PermissionPolicy;
    use harness_core::redact::DefaultRedactor;
    use harness_core::tool::{Tool, ToolCapability, ToolContext, ToolError, ToolRegistry, ToolResult};
    use async_trait::async_trait;
    use serde_json::Value;

    struct TestShellTool;
    #[async_trait]
    impl Tool for TestShellTool {
        fn id(&self) -> &str {
            "bash"
        }
        fn capability(&self) -> ToolCapability {
            ToolCapability::Shell
        }
        async fn call(&self, _ctx: ToolContext, args_json: Value) -> Result<ToolResult, ToolError> {
            Ok(ToolResult::text(format!("bash ok {args_json}")))
        }
    }

    let temp_dir = tempfile::tempdir().unwrap_or_abort();
    let mut registry = ToolRegistry::new();
    registry.register(Arc::new(TestShellTool));
    let mut config = CoordinatorConfig::new(temp_dir.path().to_path_buf());
    config.deterministic_store = true;
    config.command_buffer = 64;
    config.permission_policy = PermissionPolicy::new(
        PermissionMode::Allow,
        PermissionMode::Allow,
        PermissionMode::Allow,
    )
    .with_ask_timeout_ms(25);
    config.tool_registry = Arc::new(registry);
    let coordinator = spawn_coordinator(
        config,
        Arc::new(FakeClock::new()),
        Arc::new(DefaultRedactor::default()),
    );

    let run = coordinator
        .start_run("oc_parity_doom_loop_timeout", temp_dir.path().to_path_buf())
        .await
        .unwrap_or_abort();

    let identical_args = json!({"command": "echo timeout-probe"});
    let mut third_id = String::new();
    for i in 0..3 {
        let id = request_bash_identical(&coordinator, &identical_args).await;
        let _ = wait_for_tool_settled(&run.events_path, &id, 80).await;
        if i == 2 {
            third_id = id;
        }
    }

    let events = wait_for_tool_finished(&run.events_path, &third_id, 400).await;
    coordinator.stop_run().await.unwrap_or_abort();

    let kinds = permission_kinds_for_tool_call(&events, &third_id);
    assert!(
        kinds.iter().any(|k| k == KIND_DOOM_LOOP),
        "third must emit doom_loop before timeout deny; got {kinds:?}"
    );
    assert!(
        events.iter().any(|event| {
            matches!(
                &event.payload,
                EventV1::PermissionResolved(data)
                    if data.decision == harness_core::event::PermissionDecision::Deny
                        && data.reason.as_deref() == Some("permission request timed out")
            )
        }),
        "timeout must resolve doom_loop as deny"
    );
    assert_ne!(
        tool_finished_status(&events, &third_id),
        Some(ToolCallStatus::Succeeded),
        "timeout must fail-closed without successful execution"
    );
    assert!(
        !events.iter().any(|event| {
            matches!(
                &event.payload,
                EventV1::ToolCallStarted(data) if data.tool_call_id.as_str() == third_id
            )
        }),
        "timeout deny must not start the third tool call"
    );
}
