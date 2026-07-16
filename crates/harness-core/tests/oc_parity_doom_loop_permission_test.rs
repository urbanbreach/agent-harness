//! RED (T2): third identical tool call must Ask kind `doom_loop`.

use harness_core::UnwrapOrAbort;
use serde_json::json;

mod common;

#[path = "common/oc_parity_permission_fixtures.rs"]
mod oc_parity;

use common::{load_events, supervisor_actor};
use oc_parity::{
    parity_coordinator, permission_kinds_for_tool_call, tool_finished_status,
    wait_for_tool_settled, KIND_DOOM_LOOP,
};

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
        let tool_call_id = coordinator
            .request_tool_call(supervisor_actor(), None, "bash", identical_args.clone())
            .await
            .unwrap_or_abort();
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
         finished={:?}. Today there is no streak tracker / DoomLoop kind.",
        tool_finished_status(&events, &tool_call_ids[2])
    );
}
