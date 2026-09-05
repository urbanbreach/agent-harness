use harness_core::UnwrapOrAbort;
#[test]
fn projects_user_assistant_text_and_reasoning_parts() {
    let events = vec![
        envelope(
            1,
            supervisor(),
            None,
            EventV1::RunStarted(RunStartedEvent {
                run_name: "interactive".into(),
                workspace_root: "/workspace/project".to_string(),
            }),
        ),
        envelope(
            2,
            user(),
            None,
            EventV1::UserMessageSubmitted(UserMessageSubmittedEvent {
                request_id: "req_000001".into(),
                text: "Explain the plan.".to_string(),
            }),
        ),
        envelope(
            3,
            worker(),
            Some("req_000001"),
            EventV1::ProviderRequestStarted(ProviderRequestStartedEvent {
                request_id: "provider_req_1".into(),
                provider_id: "default".to_string(),
                model_id: "gpt-5".to_string(),
                prompt_summary: "Explain the plan.".to_string(),
                request_digest: "digest-request".to_string(),
                metadata: None,
            }),
        ),
        envelope(
            4,
            worker(),
            Some("req_000001"),
            EventV1::ProviderReasoningDelta(ProviderReasoningDeltaEvent {
                request_id: "provider_req_1".into(),
                delta: "think ".to_string(),
            }),
        ),
        envelope(
            5,
            worker(),
            Some("req_000001"),
            EventV1::ProviderStreamDelta(ProviderStreamDeltaEvent {
                request_id: "provider_req_1".into(),
                delta: "The ".to_string(),
            }),
        ),
        envelope(
            6,
            worker(),
            Some("req_000001"),
            EventV1::ProviderReasoningDelta(ProviderReasoningDeltaEvent {
                request_id: "provider_req_1".into(),
                delta: "first".to_string(),
            }),
        ),
        envelope(
            7,
            worker(),
            Some("req_000001"),
            EventV1::ProviderStreamDelta(ProviderStreamDeltaEvent {
                request_id: "provider_req_1".into(),
                delta: "plan.".to_string(),
            }),
        ),
        envelope(
            8,
            worker(),
            Some("req_000001"),
            EventV1::ProviderRequestFinished(ProviderRequestFinishedEvent {
                request_id: "provider_req_1".into(),
                finish_reason: "stop".to_string(),
                output_digest: Some("digest-output".to_string()),
                usage: None,
                metadata: None,
            }),
        ),
        envelope(
            9,
            worker(),
            Some("req_000001"),
            EventV1::AssistantMessageFinished(AssistantMessageFinishedEvent {
                request_id: "provider_req_1".into(),
                tool_call_count: 0,
                parts: Vec::new(),
                provenance: None,
                assistant_message: None,
            }),
        ),
        envelope(
            10,
            supervisor(),
            None,
            EventV1::RunFinished(RunFinishedEvent {
                summary: "finished".to_string(),
            }),
        ),
    ];

    let projection = project_transcript(&events).unwrap_or_abort();

    assert_eq!(projection.session.status, TranscriptRunStatus::Finished);
    assert_eq!(projection.session.run_name.as_deref(), Some("interactive"));
    serde_json::to_value(&projection).unwrap_or_abort();

    let user = projection
        .messages
        .iter()
        .find(|message| message.role == ProjectedMessageRole::User)
        .unwrap_or_abort();
    assert_eq!(user.request_id.as_ref().map(|r| r.as_str()), Some("req_000001"));
    let ProjectedPart::Text(user_text) = &user.parts[0] else {
        panic!("expected user text part")
    };
    assert_eq!(user_text.text, "Explain the plan.");

    let assistant = assistant_message(&projection, "req_000001");
    let provider = assistant.provider.as_ref().unwrap_or_abort();
    assert_eq!(
        provider.provider_request_id.as_deref(),
        Some("provider_req_1")
    );
    assert_eq!(provider.provider_id.as_deref(), Some("default"));
    assert_eq!(provider.model_id.as_deref(), Some("gpt-5"));
    assert_eq!(provider.finish_reason.as_deref(), Some("stop"));
    assert_eq!(provider.output_digest.as_deref(), Some("digest-output"));
    assert_eq!(assistant.parts.len(), 2);
    let ProjectedPart::Reasoning(reasoning) = &assistant.parts[0] else {
        panic!("expected reasoning part first")
    };
    assert_eq!(reasoning.text, "think first");
    assert_eq!(reasoning.provenance.first_seq, 4);
    assert_eq!(reasoning.provenance.last_seq, 6);
    let ProjectedPart::Text(text) = &assistant.parts[1] else {
        panic!("expected assistant text part second")
    };
    assert_eq!(text.text, "The plan.");
    assert_eq!(text.provenance.first_seq, 5);
    assert_eq!(text.provenance.last_seq, 7);
}
#[test]
fn keeps_tool_results_on_source_ordered_tool_parts_when_finishes_arrive_out_of_order() {
    let events = vec![
        envelope(
            1,
            worker(),
            Some("req_000001"),
            EventV1::ProviderRequestStarted(ProviderRequestStartedEvent {
                request_id: "req_000001".into(),
                provider_id: "default".to_string(),
                model_id: "gpt-5".to_string(),
                prompt_summary: "use tools".to_string(),
                request_digest: "digest-request".to_string(),
                metadata: None,
            }),
        ),
        tool_requested(
            2,
            "req_000001",
            "toolcall_000001",
            "read",
            "README.md",
            None,
        ),
        tool_requested(
            3,
            "req_000001",
            "toolcall_000002",
            "bash",
            "git status",
            None,
        ),
        envelope(
            4,
            system(),
            Some("req_000001"),
            EventV1::ToolCallFinished(ToolCallFinishedEvent {
                tool_call_id: "toolcall_000002".into(),
                status: ToolCallStatus::Succeeded,
                output_summary: Some("clean".to_string()),
                output_digest: Some("digest-bash".to_string()),
                output_json: None,
                metadata: None,
            }),
        ),
        envelope(
            5,
            system(),
            Some("req_000001"),
            EventV1::ToolCallFinished(ToolCallFinishedEvent {
                tool_call_id: "toolcall_000001".into(),
                status: ToolCallStatus::Succeeded,
                output_summary: Some("readme contents".to_string()),
                output_digest: Some("digest-read".to_string()),
                output_json: None,
                metadata: None,
            }),
        ),
    ];

    let projection = project_transcript(&events).unwrap_or_abort();
    let assistant = assistant_message(&projection, "req_000001");
    let tool_parts = assistant
        .parts
        .iter()
        .filter_map(|part| match part {
            ProjectedPart::ToolCall(tool) => Some(tool.as_ref()),
            _ => None,
        })
        .collect::<Vec<_>>();

    assert_eq!(tool_parts.len(), 2);
    assert_eq!(tool_parts[0].tool_call_id.as_str(), "toolcall_000001");
    assert_eq!(tool_parts[0].tool_id, "read");
    assert_eq!(tool_parts[0].state, ProjectedToolCallState::Succeeded);
    assert_eq!(
        tool_parts[0].output_summary.as_deref(),
        Some("readme contents")
    );
    assert_eq!(tool_parts[0].finished_seq, Some(5));
    assert_eq!(tool_parts[1].tool_call_id.as_str(), "toolcall_000002");
    assert_eq!(tool_parts[1].tool_id, "bash");
    assert_eq!(tool_parts[1].state, ProjectedToolCallState::Succeeded);
    assert_eq!(tool_parts[1].output_summary.as_deref(), Some("clean"));
    assert_eq!(tool_parts[1].finished_seq, Some(4));
}
#[test]
fn projects_session_compaction_event_as_compaction_activity() {
    let events = vec![
        envelope(
            1,
            system(),
            None,
            EventV1::SessionCompaction(SessionCompactionEvent {
                agent_id: "agent_000001".to_string(),
                summary: "## Goal\nDo the thing.\n## Progress\nDid it.".to_string(),
                first_kept_event_seq: 10,
                first_kept_request_id: Some("req_000001".to_string()),
                first_kept_entry_id: None,
                tokens_before: 5000,
                tokens_after: None,
                summary_usage: None,
                summary_provider_id: None,
                summary_model_id: None,
                read_files: vec!["src/main.rs".to_string()],
                modified_files: vec!["src/lib.rs".to_string()],
                current_intent: None,
                trigger_reason: "overflow".to_string(),
                from_hook: false,
            }),
        ),
    ];

    let projection = project_transcript(&events).unwrap_or_abort();

    let compaction_parts: Vec<_> = projection
        .messages
        .iter()
        .flat_map(|message| message.parts.iter())
        .filter_map(|part| match part {
            ProjectedPart::Compaction(compaction) => Some(compaction),
            _ => None,
        })
        .collect();
    assert_eq!(compaction_parts.len(), 1);

    let part = &compaction_parts[0];
    assert_eq!(part.agent_id, "agent_000001");
    assert_eq!(part.status, CompactionCheckpointStatus::SessionCompacted);
    assert_eq!(part.trigger_reason.as_deref(), Some("overflow"));
    assert_eq!(part.through_seq, Some(10));
    assert_eq!(part.through_request_id.as_deref(), Some("req_000001"));
    assert_eq!(part.tokens_before, Some(5000));
    assert_eq!(part.read_files, vec!["src/main.rs".to_string()]);
    assert_eq!(part.modified_files, vec!["src/lib.rs".to_string()]);
    assert_eq!(part.from_hook, Some(false));
    assert!(part.summary.as_deref().unwrap_or("").contains("## Goal"));
}
#[test]
fn projects_branch_summary_event_as_compaction_activity() {
    let events = vec![
        envelope(
            1,
            system(),
            None,
            EventV1::BranchSummary(BranchSummaryEvent {
                agent_id: "agent_000001".to_string(),
                summary: "Branch explored authentication flow.".to_string(),
                from_event_seq: 5,
                read_files: vec!["src/auth.rs".to_string()],
                modified_files: vec![],
                from_hook: true,
            }),
        ),
    ];

    let projection = project_transcript(&events).unwrap_or_abort();

    let compaction_parts: Vec<_> = projection
        .messages
        .iter()
        .flat_map(|message| message.parts.iter())
        .filter_map(|part| match part {
            ProjectedPart::Compaction(compaction) => Some(compaction),
            _ => None,
        })
        .collect();
    assert_eq!(compaction_parts.len(), 1);

    let part = &compaction_parts[0];
    assert_eq!(part.agent_id, "agent_000001");
    assert_eq!(part.status, CompactionCheckpointStatus::BranchSummary);
    assert_eq!(part.through_seq, Some(5));
    assert_eq!(part.summary.as_deref(), Some("Branch explored authentication flow."));
    assert_eq!(part.read_files, vec!["src/auth.rs".to_string()]);
    assert_eq!(part.from_hook, Some(true));
    assert!(part.trigger_reason.is_none());
}
#[test]
fn projects_artifact_metadata_without_reading_artifact_contents() {
    let metadata = Some(ToolCallMetadata {
        canonical_tool_id: Some("read".to_string()),
        alias_source_tool_id: None,
        lineage: None,
        artifact_refs: vec![EventArtifactRef {
            path: "artifacts/toolcalls/toolcall_000001/request.json".to_string(),
            digest: Some("digest-request-artifact".to_string()),
        }],
        timing: None,
        hook_executions: Vec::new(),
    });
    let finish_metadata = Some(ToolCallMetadata {
        canonical_tool_id: Some("read".to_string()),
        alias_source_tool_id: None,
        lineage: None,
        artifact_refs: vec![EventArtifactRef {
            path: "artifacts/toolcalls/toolcall_000001/result.json".to_string(),
            digest: Some("digest-result-artifact".to_string()),
        }],
        timing: None,
        hook_executions: Vec::new(),
    });
    let events = vec![
        tool_requested(
            1,
            "req_000001",
            "toolcall_000001",
            "read",
            "README.md",
            metadata,
        ),
        envelope(
            2,
            system(),
            Some("req_000001"),
            EventV1::PermissionRequested(PermissionRequestedEvent {
                permission_id: "perm_000001".to_string(),
                kind: "edit".to_string(),
                tool_call_id: Some("toolcall_000001".into()),
                summary: "read file".to_string(),
                request_digest: "digest-permission".to_string(),
                timeout_ms: 1000,
                default_decision: PermissionDecision::Deny,
            }),
        ),
        envelope(
            3,
            system(),
            Some("req_000001"),
            EventV1::PermissionResolved(PermissionResolvedEvent {
                permission_id: "perm_000001".to_string(),
                decision: PermissionDecision::Allow,
                reason: Some("approved".to_string()),
            }),
        ),
        envelope(
            4,
            system(),
            Some("req_000001"),
            EventV1::ArtifactWritten(ArtifactWrittenEvent {
                path: "artifacts/toolcalls/toolcall_000001/stdout.txt".to_string(),
                digest: "digest-stdout".to_string(),
                bytes: 42,
                tool_call_id: Some("toolcall_000001".into()),
                tool_metadata: None,
                metadata: BTreeMap::from([
                    ("artifact_kind".to_string(), "tool_output".to_string()),
                    ("path".to_string(), "src/lib.rs".to_string()),
                ]),
            }),
        ),
        envelope(
            5,
            system(),
            Some("req_000001"),
            EventV1::ToolCallFinished(ToolCallFinishedEvent {
                tool_call_id: "toolcall_000001".into(),
                status: ToolCallStatus::Succeeded,
                output_summary: Some("summary only".to_string()),
                output_digest: Some("digest-output".to_string()),
                output_json: None,
                metadata: finish_metadata,
            }),
        ),
    ];

    let projection = project_transcript(&events).unwrap_or_abort();

    assert_eq!(projection.artifacts.len(), 3);
    assert!(projection.artifacts.iter().any(|artifact| {
        artifact.path.ends_with("request.json")
            && artifact.source == ArtifactProjectionSource::ToolCallMetadata
            && artifact.bytes.is_none()
    }));
    assert!(projection.artifacts.iter().any(|artifact| {
        artifact.path.ends_with("stdout.txt")
            && artifact.source == ArtifactProjectionSource::ArtifactWritten
            && artifact.bytes == Some(42)
            && artifact.metadata.get("artifact_kind").map(String::as_str) == Some("tool_output")
            && artifact.metadata.get("path").map(String::as_str) == Some("src/lib.rs")
    }));

    let assistant = assistant_message(&projection, "req_000001");
    let ProjectedPart::ToolCall(tool) = &assistant.parts[0] else {
        panic!("expected tool part")
    };
    assert_eq!(tool.artifacts.len(), 3);
    assert_eq!(tool.permissions.len(), 1);
    assert_eq!(
        tool.permissions[0].state,
        ProjectedPermissionState::Resolved
    );
    assert_eq!(
        tool.permissions[0].decision,
        Some(PermissionDecision::Allow)
    );
    assert_eq!(tool.output_summary.as_deref(), Some("summary only"));
}
