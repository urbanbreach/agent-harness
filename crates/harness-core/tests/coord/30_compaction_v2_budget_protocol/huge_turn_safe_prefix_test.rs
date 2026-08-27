#[test]
fn compaction_v2_huge_turn_splits_utf8_safe_prefix() {
    // arrange
    // act
    // assert
    let huge = "🙂漢字e\u{301}".repeat(4_000);
    let candidates = [
        SafeCutCandidate::text("small prior answer"),
        SafeCutCandidate::text(&huge),
    ];

    let cut = plan_safe_cut(&candidates, 1_000, estimate_compaction_text_tokens).unwrap_or_abort();
    let split = cut.text_split.unwrap_or_abort();
    let prefix = huge.get(..split.byte_index).unwrap_or_abort();
    let suffix = huge.get(split.byte_index..).unwrap_or_abort();
    let budget = plan_complete_request(
        CompleteRequestComponents {
            system_tokens: 50,
            tools_tokens: 100,
            attachments_tokens: 0,
            framing_tokens: 20,
            pending_prompt_tokens: 40,
            requested_completion_tokens: 500,
            compaction_threshold_tokens: Some(5_000),
        },
        CompactionHistoryTokens {
            anchored_tokens: 0,
            trailing_tokens: estimate_compaction_text_tokens(&huge),
            prior_summary_tokens: 0,
            anchor_includes_prior_summary: false,
            retained_tokens: cut.retained_tokens,
        },
        1_000,
    )
    .unwrap_or_abort();

    assert_eq!(
        (
            [prefix, suffix].concat(),
            prefix.is_empty(),
            suffix.is_empty(),
            cut.retained_tokens <= 1_000,
            budget.summary_allowance_tokens,
        ),
        (huge, false, false, true, 500),
    );
}
