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
