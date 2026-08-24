use harness_core::UnwrapOrAbort;

#[path = "../common/engine_fixture_bank.rs"]
mod engine_fixture_bank;
use engine_fixture_bank::*;

#[test]
fn fixtures_same_seed_serialized_jsonl_bytes_are_identical() {
    // arrange
    let left = SemanticSessionBuilder::new(7).build();

    // act
    let right = SemanticSessionBuilder::new(7).build();

    // assert
    assert_eq!(jsonl(&left.events()), jsonl(&right.events()));
}

#[test]
fn fixtures_different_root_child_ids_and_histories_are_isolated() {
    // arrange
    // act
    let fixture = RootChildFixture::new(7);

    // assert
    assert_ne!(fixture.root.semantic_ids(), fixture.child.semantic_ids());
    assert_ne!(
        fixture.root.provider_history(),
        fixture.child.provider_history()
    );
    assert_ne!(fixture.root.events(), fixture.child.events());
}

#[test]
fn fixtures_physical_retry_changes_only_provider_request_id() {
    // arrange
    let original = SemanticSessionBuilder::new(7).build();

    // act
    let retry = original.retry(2);

    // assert
    assert_ne!(
        original.provider_request_id(),
        retry.provider_request_id()
    );
    assert_eq!(original.semantic_ids(), retry.semantic_ids());
    assert_eq!(original.events(), retry.events());
}

#[test]
fn fixtures_all_legacy_variants_have_one_distinct_read_only_omission() {
    // arrange
    let variants = [
        LegacyLogVariant::BoundariesOmitted,
        LegacyLogVariant::MetadataAbsent,
        LegacyLogVariant::NoUsage,
        LegacyLogVariant::AttachmentMetadataMissing,
        LegacyLogVariant::LineageUnknown,
    ];

    // act
    for variant in variants {
        let fixture = legacy_log_fixture(7, variant);
        let before = fixture.bytes();
        let decoded: serde_json::Value = serde_json::from_slice(&before).unwrap_or_abort();
        let after = fixture.bytes();

        // assert
        assert_eq!(before, after);
        assert_eq!(decoded, *fixture.value());
        assert!(
            decoded["payload"]
                .get(fixture.omitted_field())
                .is_none()
        );
        assert_eq!(
            decoded["payload"]
                .as_object()
                .map(serde_json::Map::len),
            Some(4)
        );
    }
}

#[test]
fn fixtures_side_effect_recorder_counts_and_order_are_exact() {
    // arrange
    let recorder = SideEffectRecorder::default();
    let effects = ["provider", "summary", "tool", "hook", "event-open"];

    // act
    for effect in effects {
        recorder.record(effect);
    }

    // assert
    for effect in effects {
        assert_eq!(recorder.count(effect), 1);
    }
    assert_eq!(
        recorder.ordered(),
        ["provider", "summary", "tool", "hook", "event-open"]
    );
}
