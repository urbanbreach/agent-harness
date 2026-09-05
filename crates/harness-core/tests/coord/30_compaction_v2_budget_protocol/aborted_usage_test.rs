#[test]
fn compaction_v2_aborted_usage_not_anchor() {
    let completed_usage = CompletionUsage {
        prompt_tokens: 7_777,
        completion_tokens: 1,
        total_tokens: 7_778,
    };
    let aborted_usage = CompletionUsage {
        prompt_tokens: 99_999,
        completion_tokens: 1,
        total_tokens: 100_000,
    };
    let candidates = [
        UsageCandidate {
            terminal_status: RequestTerminalStatus::Completed,
            request_id: "completed-request",
            provider_id: "mock",
            model_id: "model-1",
            semantic_usage: Some(&completed_usage),
            finished_usage: Some(&completed_usage),
            started: Some(StartedRequestMetadata {
                request_id: "completed-request",
                provider_id: "mock",
                model_id: "model-1",
                budget: Some(AnchorBudgetComponents {
                    system_tokens: 20,
                    tools_tokens: 30,
                    attachments_tokens: 0,
                    framing_tokens: 10,
                }),
            }),
            through_index: 4,
            includes_prior_summary: false,
        },
        UsageCandidate {
            terminal_status: RequestTerminalStatus::Aborted,
            request_id: "aborted-request",
            provider_id: "mock",
            model_id: "model-1",
            semantic_usage: Some(&aborted_usage),
            finished_usage: Some(&aborted_usage),
            started: None,
            through_index: 8,
            includes_prior_summary: false,
        },
    ];

    let resolution = resolve_usage_anchor(
        &candidates,
        CurrentRequestModel {
            provider_id: "mock",
            model_id: "model-1",
        },
    );

    assert!(matches!(
        resolution,
        UsageAnchorResolution::Valid(anchor)
            if anchor.through_index == 4 && anchor.usage_total_tokens == 7_778
    ));
}
