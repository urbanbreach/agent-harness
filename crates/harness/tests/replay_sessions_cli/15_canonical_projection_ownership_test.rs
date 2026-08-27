#[test]
fn replay_and_export_share_canonical_projection_owner() {
    // arrange
    let replay_source = include_str!("../../src/replay.rs");
    let export_source = include_str!("../../src/sessions/export.rs");

    // act
    let replay_uses_canonical_projection =
        replay_source.contains("CanonicalSessionProjection");
    let export_uses_replay_projection = export_source.contains("summarize_session");

    // assert
    assert!(
        replay_uses_canonical_projection,
        "replay/session inspection must consume CanonicalSessionProjection"
    );
    assert!(
        export_uses_replay_projection,
        "session export must consume the replay-derived canonical summary"
    );
}
