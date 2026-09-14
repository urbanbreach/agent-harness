#[test]
fn compaction_v2_huge_turn_keeps_whole_unicode_message() {
    let huge = "🙂漢字e\u{301}".repeat(4_000);
    let candidates = [
        SafeCutCandidate::text("small prior answer"),
        SafeCutCandidate::text(&huge),
    ];

    let cut = plan_safe_cut(&candidates, 1_000, estimate_compaction_text_tokens).unwrap_or_abort();
    assert_eq!(cut.first_kept_index, 1);
    assert_eq!(cut.retained_tokens, estimate_compaction_text_tokens(&huge));
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
        cut.retained_tokens,
    );

    assert_eq!(budget, Err(complete_request::CompleteRequestBudgetError::NoSummaryAllowance));
}
