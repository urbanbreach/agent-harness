#[test]
fn compaction_v2_attachments_charge_budget_once() {
    // arrange
    // act
    // assert
    let base = CompleteRequestComponents {
        system_tokens: 20,
        tools_tokens: 30,
        attachments_tokens: 0,
        framing_tokens: 10,
        pending_prompt_tokens: 5,
        requested_completion_tokens: 100,
        compaction_threshold_tokens: Some(10_000),
    };
    let history = CompactionHistoryTokens {
        anchored_tokens: 500,
        trailing_tokens: 50,
        prior_summary_tokens: 8,
        anchor_includes_prior_summary: false,
        retained_tokens: 100,
    };
    let without_attachment = plan_complete_request(base, history, 100).unwrap_or_abort();

    let with_attachment = plan_complete_request(
        CompleteRequestComponents {
            attachments_tokens: 1_024,
            ..base
        },
        history,
        100,
    )
    .unwrap_or_abort();

    assert_eq!(
        (
            with_attachment.fixed_input_tokens - without_attachment.fixed_input_tokens,
            with_attachment.pre_total_tokens - without_attachment.pre_total_tokens,
            with_attachment.post_total_tokens - without_attachment.post_total_tokens,
        ),
        (1_024, 1_024, 1_024),
    );
}
