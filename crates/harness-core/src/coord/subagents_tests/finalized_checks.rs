macro_rules! assert_raw_source_integrity {
    ($source:expr, $arguments:expr, $root:expr) => {{
        assert_eq!($source.conversation_items.len(), 4);
        assert_eq!(
            $source.conversation_items[0].attachments[0].bytes()?,
            b"faithful attachment"
        );
        assert_eq!(
            $source.conversation_items[1]
                .message
                .assistant_tool_calls
                .as_ref()
                .ok_or("calls missing")?[0]
                .arguments_json,
            $arguments
        );
        let raw = $source.conversation_items[2]
            .raw_tool_result
            .as_ref()
            .ok_or("raw result missing")?;
        assert_eq!(raw.provider_tool_call_id.as_deref(), Some("call-a"));
        assert_ne!(raw.tool_call_id, "call-a");
        assert_eq!(
            raw.output
                .as_ref()
                .ok_or("raw output missing")?
                .display_text
                .len(),
            "raw-output-marker\n".len() * 4000
        );
        assert!($source.conversation_items[2]
            .message
            .content
            .contains("Output shortened"));
        assert_eq!(
            $source.usage[0]
                .settled_reasoning
                .as_ref()
                .ok_or("reasoning missing")?[0],
            "settled-first"
        );
        assert_eq!(
            $source
                .read_state
                .get(&$root.join("sample.txt"))
                .map(String::as_str),
            Some(blake3::hash(b"sample").to_hex().as_str())
        );
        assert!($source
            .model_request
            .as_ref()
            .ok_or("last logical input missing")?
            .messages
            .iter()
            .all(|m| m.role != harness_providers::MessageRole::System));
    }};
}

macro_rules! assert_finalized_reference_rejections {
    ($run_dir:expr, $run_id:expr, $source:expr, $attempt:expr, $reference:expr, $artifact:expr) => {{
        for (change, reason) in [
            (0, FinalizedStateUnavailable::OwnerMismatch),
            (1, FinalizedStateUnavailable::UnsupportedVersion),
            (2, FinalizedStateUnavailable::Corrupt),
        ] {
            let mut changed = $reference.clone();
            match change {
                0 => changed.state.owner = SubagentId("foreign".into()),
                1 => changed.payload_version = 2,
                _ => changed.byte_length += 1,
            }
            assert_eq!(
                resolve_finalized_state(
                    $run_dir,
                    &changed,
                    $run_id,
                    &SubagentId($source.clone()),
                    $attempt
                ),
                FinalizedStateResult::Unavailable { reason }
            );
        }
        std::fs::write($artifact, b"{}")?;
        assert_eq!(
            resolve_finalized_state(
                $run_dir,
                $reference,
                $run_id,
                &SubagentId($source.clone()),
                $attempt
            ),
            FinalizedStateResult::Unavailable {
                reason: FinalizedStateUnavailable::Corrupt
            }
        );
        std::fs::remove_file($artifact)?;
        assert_eq!(
            resolve_finalized_state(
                $run_dir,
                $reference,
                $run_id,
                &SubagentId($source.clone()),
                $attempt
            ),
            FinalizedStateResult::Unavailable {
                reason: FinalizedStateUnavailable::Missing
            }
        );
    }};
}

pub(super) fn assert_semantic_source_integrity(
    durable: &super::RawFinalizedState,
    live: &super::RawFinalizedState,
) -> Result<(), Box<dyn std::error::Error>> {
    use super::*;
    assert_eq!(durable.fidelity, FinalizedStateFidelity::Semantic);
    assert!(durable.model_request.is_none());
    assert_eq!(durable.read_state, live.read_state);
    assert_eq!(durable.execution_context, live.execution_context);
    assert_eq!(
        durable.conversation_items.len(),
        live.conversation_items.len()
    );
    for (saved, original) in durable
        .conversation_items
        .iter()
        .zip(&live.conversation_items)
    {
        assert!(saved.settled_reasoning.is_empty());
        assert!(saved.raw_tool_result.is_none());
        assert_eq!(saved.message.content, original.message.content);
        assert_eq!(saved.attachments, original.attachments);
        for (a, b) in saved
            .message
            .assistant_tool_calls
            .iter()
            .flatten()
            .zip(original.message.assistant_tool_calls.iter().flatten())
        {
            assert_eq!(a.function_name, b.function_name);
            assert_eq!(
                serde_json::from_str::<Value>(&a.arguments_json)?,
                serde_json::from_str::<Value>(&b.arguments_json)?
            );
        }
    }
    assert_eq!(durable.usage.len(), live.usage.len());
    for (saved, original) in durable.usage.iter().zip(&live.usage) {
        assert!(saved.thinking.is_none());
        assert!(saved.settled_reasoning.is_none());
        assert_eq!(saved.usage, original.usage);
        assert_eq!(saved.usage_complete, original.usage_complete);
    }
    Ok(())
}

pub(super) fn check_legacy_snapshot(
    reference: &super::FinalizedAgentStateReferenceV1,
    live: &super::RawFinalizedState,
) -> Result<(), Box<dyn std::error::Error>> {
    use super::*;
    use sha2::{Digest, Sha256};
    let root = tempfile::tempdir()?;
    let mut payload = serde_json::to_value(live)?;
    payload["model_request"] = serde_json::to_value(&live.model_request)?;
    for (item, original) in payload["conversation_items"]
        .as_array_mut()
        .ok_or("items missing")?
        .iter_mut()
        .zip(&live.conversation_items)
    {
        item["settled_reasoning"] = serde_json::to_value(&original.settled_reasoning)?;
        item["raw_tool_result"] = serde_json::to_value(&original.raw_tool_result)?;
    }
    for (usage, original) in payload["usage"]
        .as_array_mut()
        .ok_or("usage missing")?
        .iter_mut()
        .zip(&live.usage)
    {
        usage["thinking"] = serde_json::to_value(&original.thinking)?;
        usage["settled_reasoning"] = serde_json::to_value(&original.settled_reasoning)?;
    }
    let bytes = serde_json::to_vec(&payload)?;
    assert!(String::from_utf8_lossy(&bytes).contains("settled-first"));
    let mut legacy = reference.clone();
    legacy.state.fidelity = FinalizedStateFidelity::Exact;
    legacy.state.sha256 = hex::encode(Sha256::digest(&bytes));
    legacy.byte_length = bytes.len() as u64;
    crate::store::create_private_dir(&root.path().join("artifacts"))?;
    crate::store::write_private_atomic(&root.path().join(legacy.artifact_path()), &bytes)?;
    // Attachment hydration is checked by the surrounding test; the verified reader
    // also serves session cloning and must strip old transient fields before copying.
    let state = crate::subagent::read_finalized_payload(
        root.path(),
        &legacy,
        &legacy.owner_run_id,
        &legacy.state.owner,
        &legacy.attempt_id,
    )
    .map_err(|reason| format!("legacy state rejected: {reason:?}"))?;
    assert_semantic_source_integrity(&state, live)?;
    assert!(!String::from_utf8(serde_json::to_vec(&state)?)?.contains("settled-first"));
    assert_eq!(
        std::fs::read(root.path().join(legacy.artifact_path()))?,
        bytes
    );
    Ok(())
}
