use harness_core::ids::{EntryId, SessionId, ToolCallId};
use harness_core::session::reducer::replay;
use harness_core::session::{CanonicalRecord, SessionEntryPayload};
use harness_core::UnwrapOrAbort;
use serde_json::json;

mod support {
    include!("03_provider_view_selected_branch_support.rs");
}
use support::{complete_tool_pairs, fixture_records, provider_context_digest, tool_pair_json};

#[test]
fn session_lineage_selected_branch_resume_context_excludes_source_tail() {
    // arrange
    // act
    // assert
    // Given: a child prefix has a selected leaf, completed tool pair, typed attachment, and usage,
    // while later source/sibling tails contain interrupted and legacy-only state.
    let child_session_id = SessionId::new("child-session");
    let child_records = fixture_records(&child_session_id);
    let live = replay(child_session_id.clone(), &child_records).unwrap_or_abort();
    let live_path = live.active_path().unwrap_or_abort();

    // When: the same durable records are serialized and replayed as a fresh coordinator restore.
    let encoded = serde_json::to_vec(&child_records).unwrap_or_abort();
    let restored_records: Vec<CanonicalRecord> = serde_json::from_slice(&encoded).unwrap_or_abort();
    let restored = replay(child_session_id, &restored_records).unwrap_or_abort();
    let restart_path = restored.active_path().unwrap_or_abort();

    // Then: only selected ancestors and complete protocol-safe pairs reach the provider view.
    let selected_ids = vec![
        "root-user",
        "root-assistant",
        "root-tool-result",
        "selected-user",
        "selected-assistant",
    ];
    assert_eq!(
        live_path
            .iter()
            .map(|entry| entry.id.as_str())
            .collect::<Vec<_>>(),
        selected_ids
    );
    assert_eq!(
        restart_path
            .iter()
            .map(|entry| entry.id.as_str())
            .collect::<Vec<_>>(),
        selected_ids
    );
    assert!(!selected_ids.contains(&"source-tail-user"));
    assert!(!selected_ids.contains(&"sibling-tail"));
    assert!(!selected_ids.contains(&"off-path-interrupted"));
    assert!(live_path.iter().all(|entry| {
        let payload = serde_json::to_string(&entry.payload).unwrap_or_abort();
        !payload.contains("legacy delta") && !payload.contains("live delta")
    }));

    let complete_pairs = complete_tool_pairs(&live_path);
    assert_eq!(complete_pairs.len(), 1);
    assert_eq!(
        complete_pairs[0].assistant_entry_id,
        EntryId::new("root-assistant")
    );
    assert_eq!(
        complete_pairs[0].result_entry_id,
        EntryId::new("root-tool-result")
    );
    assert_eq!(
        complete_pairs[0].tool_call_id,
        ToolCallId::new("selected-tool-call")
    );

    let selected_attachment = live_path
        .iter()
        .find_map(|entry| match &entry.payload {
            SessionEntryPayload::UserMessage { attachments, .. } if !attachments.is_empty() => {
                attachments.first().cloned()
            }
            _ => None,
        })
        .unwrap_or_abort();
    assert_eq!(selected_attachment.id, "名-é.png");
    assert_eq!(selected_attachment.mime, "image/png");
    assert_eq!(selected_attachment.size, 22);

    let selected_usage = live_path
        .iter()
        .find_map(|entry| match &entry.payload {
            SessionEntryPayload::AssistantMessage { provenance, .. }
                if entry.id == EntryId::new("selected-assistant") =>
            {
                provenance.as_ref()?.usage.clone()
            }
            _ => None,
        })
        .unwrap_or_abort();
    assert_eq!(selected_usage.total_tokens, 118);
    assert!(!live_path.iter().any(|entry| {
        matches!(
            &entry.payload,
            SessionEntryPayload::AssistantMessage {
                provenance: Some(provenance),
                ..
            } if provenance.usage.as_ref().is_some_and(|usage| usage.total_tokens == 999)
        )
    }));

    let live_digest = provider_context_digest(&live, "child");
    let restart_digest = provider_context_digest(&restored, "child");
    assert_eq!(live_digest, restart_digest);

    let root_session_id = SessionId::new("root-session");
    let root_records = fixture_records(&root_session_id);
    let root = replay(root_session_id, &root_records).unwrap_or_abort();
    let root_digest = provider_context_digest(&root, "root");
    assert_ne!(root_digest, live_digest);

    eprintln!(
        "task3_selected_branch_evidence {}",
        json!({
            "selected_entry_ids": selected_ids,
            "excluded_entry_ids": ["source-tail-user", "sibling-tail", "off-path-interrupted"],
            "tool_pairs": complete_pairs.iter().map(tool_pair_json).collect::<Vec<_>>(),
            "attachment_id": selected_attachment.id,
            "attachment_mime": selected_attachment.mime,
            "selected_usage_owner": "selected-assistant",
            "selected_usage_total_tokens": selected_usage.total_tokens,
            "live_delta_absent": true,
            "provider_calls": 0,
            "tool_calls": 0,
            "hook_calls": 0,
            "child_live_digest": live_digest,
            "child_restart_digest": restart_digest,
            "root_digest": root_digest,
            "child_digest": live_digest,
            "child_root_digests_differ": root_digest != live_digest,
        })
    );
}
