use super::*;

fn provider_view_with_attachments(
    historical: Vec<AttachmentMetadata>,
    pending: Vec<AttachmentMetadata>,
) -> (
    harness_core::session::CanonicalProviderView,
    harness_core::agent::AgentProfile,
    Vec<harness_providers::ToolDef>,
) {
    let mut profile = agent_profiles().remove("default").unwrap_or_abort();
    profile.toolset = vec!["shell.run".to_string()];
    let registry = test_tool_registry();
    let tools = harness_core::agent::build_provider_tool_defs_for_model(
        &profile,
        registry.as_ref(),
        "mock:model-1",
    )
    .unwrap_or_abort();
    let target = drift_target(
        "media-variant",
        "high",
        "low",
        "auto",
        serde_json::json!({"budget": 512}),
    );
    let runtime_selection = harness_core::agent::canonical_runtime_selection(
        harness_core::agent::CanonicalRuntimeSelectionInput {
            profile: &profile,
            model: &harness_core::agent::AgentModelRef {
                provider_id: target.provider.clone(),
                model_id: target.model.clone(),
            },
            settings: harness_core::agent::AgentModelSettings::from(&target),
            resolved_limits: target.limits,
            tools: &tools,
        },
    )
    .unwrap_or_abort();
    let entry_id = harness_core::ids::EntryId::new("media-leaf");
    let view = harness_core::session::CanonicalProviderView {
        owner: harness_core::session::ProviderViewOwner::root(
            "agent_000001",
            harness_core::ids::SessionId::new("run_000001"),
        ),
        selected_leaf: entry_id.clone(),
        active_entry_ids: vec![entry_id.clone()],
        entries: vec![harness_core::session::SessionEntry {
            id: entry_id.clone(),
            parent_id: None,
            turn_id: Some(harness_core::ids::TurnId::new("turn-media")),
            run_id: harness_core::ids::RunId::new("run_000001"),
            payload: harness_core::session::SessionEntryPayload::UserMessage {
                text: "historical media".to_string(),
                attachments: historical.clone(),
            },
        }],
        pending_prompt: Some(harness_core::session::CanonicalPendingPrompt {
            turn_id: harness_core::ids::TurnId::new("turn-pending"),
            text: "pending media".to_string(),
            attachments: pending,
        }),
        latest_compaction_summary: None,
        tool_pairs: Vec::new(),
        attachments: historical
            .into_iter()
            .map(|attachment| harness_core::session::CanonicalAttachment {
                entry_id: entry_id.clone(),
                attachment,
            })
            .collect(),
        usage_boundaries: Vec::new(),
        watermark: Some(harness_core::session::RecordSequence::new(3)),
        runtime_selection,
    };
    (view, profile, tools)
}

#[test]
fn provider_context_media_flag_follows_canonical_attachments() {
    // arrange
    // act
    // assert
    // Given: two ordered historical attachments and one pending attachment.
    let first = AttachmentMetadata::from_bytes("資料-a", "image/png", None, b"one", None);
    let second = AttachmentMetadata::from_bytes("資料-b", "image/jpeg", None, b"two", None);
    let pending = AttachmentMetadata::from_bytes("資料-c", "text/plain", None, b"three", None);
    let (view, profile, tools) =
        provider_view_with_attachments(vec![first.clone(), second.clone()], vec![pending.clone()]);

    // When: the Task 6 continuation lowerer builds the provider request.
    let output = harness_core::agent::lower_provider_continuation(
        harness_core::agent::LowerProviderContinuationInput {
            view: &view,
            transient_operational_turns: &[],
            profile: &profile,
            tools: Some(tools),
            tool_choice: Some(harness_providers::ToolChoice::Auto),
            fresh_request_id: "request-media",
        },
    )
    .unwrap_or_abort();
    let encoded = serde_json::to_string(&output.request.messages).unwrap_or_abort();

    // Then: ordered metadata appears once per owning turn and the media flag is true.
    assert!(output.request.context.has_media);
    let first_offset = encoded.find("資料-a").unwrap_or_abort();
    let second_offset = encoded.find("資料-b").unwrap_or_abort();
    let pending_offset = encoded.find("資料-c").unwrap_or_abort();
    assert!(first_offset < second_offset && second_offset < pending_offset);
    assert_eq!(encoded.matches("資料-a").count(), 1);
    assert_eq!(encoded.matches("資料-b").count(), 1);
    assert_eq!(encoded.matches("資料-c").count(), 1);
    assert!(!encoded.contains("one"));
    assert!(!encoded.contains("two"));
    assert!(!encoded.contains("three"));
}

#[test]
fn resume_rejects_malformed_attachment_metadata_without_provider_call() {
    // arrange
    // act
    // assert
    // Given: persisted attachment metadata with a non-redacted raw content reference.
    let malformed: AttachmentMetadata = serde_json::from_value(serde_json::json!({
        "id": "malformed",
        "mime": "image/png",
        "size": 12,
        "content_ref": "raw-secret-bytes"
    }))
    .unwrap_or_abort();
    let (view, profile, tools) = provider_view_with_attachments(vec![malformed], Vec::new());
    let provider = CapturingProvider::new(vec!["must not dispatch"]);

    // When: resume lowers the canonical provider view before dispatch.
    let error = harness_core::agent::lower_provider_continuation(
        harness_core::agent::LowerProviderContinuationInput {
            view: &view,
            transient_operational_turns: &[],
            profile: &profile,
            tools: Some(tools),
            tool_choice: Some(harness_providers::ToolChoice::Auto),
            fresh_request_id: "request-malformed",
        },
    )
    .unwrap_err();

    // Then: a typed attachment error is returned and no provider request exists.
    assert!(matches!(
        error,
        harness_core::agent::ProviderContinuationLoweringError::InvalidAttachment { .. }
    ));
    assert!(provider.requests().is_empty());
    assert!(!error.to_string().contains("raw-secret-bytes"));
}
