use harness_tui::design_contract::DESIGN_TOKENS;
use harness_tui::transcript_blocks::{
    BlockEvent, BlockKind, BlockLifecycle, FoldState, RawDisclosure, RawPayload, TranscriptBlocks,
    default_fold, style_for,
};
use harness_tui::transcript_identity::ReplayTurn;

fn example_turn() -> ReplayTurn {
    ReplayTurn::event(41, 2, 6)
}

#[test]
fn every_block_kind_has_design_contract_styling() {
    for kind in BlockKind::ALL {
        let style = style_for(kind);

        assert_eq!(style.indent, DESIGN_TOKENS.spacing.transcript_gutter_x);
        assert!(!style.glyph.is_empty());
        assert!(!style.separator.is_empty());
    }
}

#[test]
fn lifecycle_defaults_cover_every_kind_and_state() {
    let lifecycles = [
        BlockLifecycle::Streaming,
        BlockLifecycle::Tool,
        BlockLifecycle::Waiting,
        BlockLifecycle::Completed,
        BlockLifecycle::Failed,
    ];

    for kind in BlockKind::ALL {
        for lifecycle in lifecycles {
            let state = default_fold(kind, lifecycle);
            let expected = match (kind, lifecycle) {
                (BlockKind::Thinking, BlockLifecycle::Completed)
                | (BlockKind::Tool, BlockLifecycle::Completed) => FoldState::Collapsed,
                _ => FoldState::Expanded,
            };
            assert_eq!(state, expected, "{kind:?} {lifecycle:?}");
        }
    }
}

#[test]
fn per_block_fold_toggle_preserves_content_and_raw_data() {
    let turn = example_turn();
    let id = turn.block_id(0);
    let raw = RawDisclosure::from_text("result: unchanged");
    let mut blocks = TranscriptBlocks::new();
    blocks.insert(
        id,
        BlockKind::Tool,
        BlockLifecycle::Streaming,
        "result",
        Some(raw),
    );

    assert!(blocks.toggle_fold(id).is_ok());
    let block = blocks.get(id);

    assert_eq!(
        block.map(|block| block.fold_state()),
        Some(FoldState::Collapsed)
    );
    assert_eq!(block.map(|block| block.content()), Some("result"));
    assert_eq!(
        block.and_then(|block| block.raw()).map(|raw| &raw.payload),
        Some(&RawPayload::Text("result: unchanged".to_string(),))
    );
}

#[test]
fn append_while_folded_keeps_the_block_folded() {
    let id = example_turn().block_id(0);
    let mut blocks = TranscriptBlocks::new();
    blocks.insert(
        id,
        BlockKind::Thinking,
        BlockLifecycle::Streaming,
        "draft",
        None,
    );
    assert!(blocks.toggle_fold(id).is_ok());

    assert!(blocks.append(id, " +more").is_ok());
    let block = blocks.get(id);

    assert_eq!(
        block.map(|block| block.fold_state()),
        Some(FoldState::Collapsed)
    );
    assert_eq!(block.map(|block| block.content()), Some("draft +more"));
}

#[test]
fn replay_reconstructs_identical_blocks_and_fold_state() {
    let id = example_turn().block_id(0);
    let events = [
        BlockEvent::Create {
            id,
            kind: BlockKind::Thinking,
            lifecycle: BlockLifecycle::Streaming,
            content: "draft".to_string(),
        },
        BlockEvent::ToggleFold { id },
        BlockEvent::Append {
            id,
            content: " +more".to_string(),
        },
        BlockEvent::Lifecycle {
            id,
            lifecycle: BlockLifecycle::Completed,
        },
    ];

    let replayed = TranscriptBlocks::replay(&events);
    assert!(replayed.is_ok());
    let mut expected = TranscriptBlocks::new();
    assert!(expected.apply_all(&events).is_ok());

    assert_eq!(
        replayed.map(|blocks| blocks.snapshot()),
        Ok(expected.snapshot())
    );
}

#[test]
fn raw_disclosure_redacts_provider_and_auth_secrets_without_dropping_shape() {
    let source = serde_json::json!({
        "model": "gpt",
        "api_key": "sk-1234567890abcdefghij",
        "authorization": "Bearer live-token-1234567890",
        "nested": [
            {"ok": true, "value": "AIza12345678901234567890"},
            null
        ]
    });

    let disclosure = RawDisclosure::from_json(&source);
    assert!(matches!(disclosure.payload, RawPayload::Json(_)));
    let redacted = match &disclosure.payload {
        RawPayload::Json(value) => value,
        RawPayload::Text(_) => return,
    };

    assert_eq!(redacted["model"], "gpt");
    assert_eq!(redacted["nested"][0]["ok"], true);
    assert_eq!(redacted["nested"].as_array().map(Vec::len), Some(2));
    assert_eq!(redacted["api_key"], "<redacted>");
    assert_eq!(redacted["authorization"], "Bearer <redacted>");
    assert_eq!(redacted["nested"][0]["value"], "<redacted>");
    assert!(!redacted.to_string().contains("sk-1234567890abcdefghij"));
    assert!(!redacted.to_string().contains("live-token-1234567890"));
    let text = RawDisclosure::from_text("Authorization: Bearer live-token-1234567890");
    assert_eq!(
        &text.payload,
        &RawPayload::Text("Authorization: Bearer <redacted>".to_string())
    );

    insta::assert_json_snapshot!(redacted, @r###"
    {
      "api_key": "<redacted>",
      "authorization": "Bearer <redacted>",
      "model": "gpt",
      "nested": [
        {
          "ok": true,
          "value": "<redacted>"
        },
        null
      ]
    }
    "###);
}
