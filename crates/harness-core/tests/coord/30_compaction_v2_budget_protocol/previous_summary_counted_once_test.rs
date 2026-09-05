#[tokio::test]
async fn compaction_v2_previous_summary_counted_once() {
    const PREVIOUS_SUMMARY: &str = "PPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPP";
    let (harness, provider) = CompactionV2Harness::scripted(
        vec![
            provider_text_events("one"),
            provider_text_events("two"),
            provider_text_events(PREVIOUS_SUMMARY),
            provider_text_events("three"),
            provider_text_events("new rolling summary"),
        ],
        CompactionRuntimeConfig::default(),
    )
    .await;
    harness.turn("budget one").await;
    harness.turn("budget two").await;
    harness
        .coordinator
        .compact_agent_context(harness.agent_id.clone(), None, "manual")
        .await
        .unwrap_or_abort();
    harness.turn("budget three").await;

    harness
        .coordinator
        .compact_agent_context(harness.agent_id.clone(), None, "manual")
        .await
        .unwrap_or_abort();
    harness.stop().await;

    let requests = provider.requests();
    let second_summary_request = requests.get(4).unwrap_or_abort();
    let occurrences = second_summary_request
        .messages
        .iter()
        .map(|message| message.content.matches(PREVIOUS_SUMMARY).count())
        .sum::<usize>();
    let pending_index = second_summary_request.messages.len() - 1;
    let full_cost =
        harness_providers::generic_request_budget_semantics(second_summary_request, pending_index)
            .unwrap_or_abort()
            .request_cost;
    let mut without_previous = second_summary_request.clone();
    for message in &mut without_previous.messages {
        message.content = message.content.replace(PREVIOUS_SUMMARY, "");
    }
    let without_cost =
        harness_providers::generic_request_budget_semantics(&without_previous, pending_index)
            .unwrap_or_abort()
            .request_cost;
    let components = CompleteRequestComponents {
        system_tokens: 20,
        tools_tokens: 30,
        attachments_tokens: 0,
        framing_tokens: 10,
        pending_prompt_tokens: 5,
        requested_completion_tokens: 100,
        compaction_threshold_tokens: Some(2_000),
    };
    let with_previous = plan_complete_request(
        components,
        CompactionHistoryTokens {
            anchored_tokens: 508,
            trailing_tokens: 50,
            prior_summary_tokens: 8,
            anchor_includes_prior_summary: true,
            retained_tokens: 100,
        },
        100,
    )
    .unwrap_or_abort();
    let without_previous_plan = plan_complete_request(
        components,
        CompactionHistoryTokens {
            anchored_tokens: 500,
            trailing_tokens: 50,
            prior_summary_tokens: 0,
            anchor_includes_prior_summary: false,
            retained_tokens: 100,
        },
        100,
    )
    .unwrap_or_abort();
    assert_eq!(
        (
            occurrences,
            full_cost.pending_prompt_tokens - without_cost.pending_prompt_tokens,
            with_previous.pre_total_tokens - without_previous_plan.pre_total_tokens,
        ),
        (1, 8, 8),
    );
}
