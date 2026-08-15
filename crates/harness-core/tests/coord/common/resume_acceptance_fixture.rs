use harness_core::UnwrapOrAbort;
pub(super) struct ResumeAcceptanceFixture {
    pub(super) temp_dir: tempfile::TempDir,
    pub(super) run_id: &'static str,
    pub(super) artifact_path: &'static str,
    pub(super) artifact_digest: String,
    pub(super) shell_args: serde_json::Value,
    pub(super) artifact_abs_path: PathBuf,
}

pub(super) fn write_resume_acceptance_fixture() -> ResumeAcceptanceFixture {
    let temp_dir = tempfile::tempdir().unwrap_or_abort();
    let workspace_root = temp_dir.path().join("workspace");
    fs::create_dir_all(&workspace_root).unwrap_or_abort();
    let run_id = "run_resume_acceptance_realistic";
    let artifact_path = "artifacts/reports/resume-acceptance.json";
    let artifact_body = r#"{"status":"artifact restored"}"#;
    let artifact_digest = blake3::hash(artifact_body.as_bytes()).to_hex().to_string();
    let shell_args = json!({"command": "printf resume acceptance"});
    let shell_request_digest = test_permission_request_digest("shell.run", &shell_args);
    let shell_grant = harness_core::perm::PermissionGrant {
        grant_id: "grant_perm_000001".to_string(),
        permission_id: "perm_000001".to_string(),
        scope: harness_core::perm::PermissionGrantScope::Run,
        expires_at: None,
        kind: harness_core::perm::PermissionKind::Shell,
        tool: harness_core::perm::PermissionToolSelector {
            effective_tool_id: "shell.run".to_string(),
            canonical_tool_id: Some("shell.run".to_string()),
        },
        matcher: harness_core::perm::PermissionGrantMatcher::ShellCommand {
            command_digest: test_digest12(b"printf resume acceptance"),
            request_digest: shell_request_digest.clone(),
            patterns: Vec::new(),
            always_patterns: Vec::new(),
        },
    };
    let worker = EventActor::new(ActorKind::Worker, Some("agent_000001".to_string()));
    let mut artifact_metadata = BTreeMap::new();
    artifact_metadata.insert(
        "summary".to_string(),
        "resume acceptance report".to_string(),
    );

    write_resume_fixture(
        temp_dir.path(),
        run_id,
        &[
            resume_fixture_event(
                run_id,
                1,
                EventV1::RunStarted(RunStartedEvent {
                    run_name: "resume acceptance coding session".into(),
                    workspace_root: workspace_root.display().to_string(),
                }),
            ),
            resume_fixture_event(
                run_id,
                2,
                EventV1::AgentSpawned(AgentSpawnedEvent {
                    agent_id: "agent_000001".to_string(),
                profile: "default".to_string(),
                    parent_agent_id: None,
                }),
            ),
            resume_fixture_event_with_actor_and_correlation(
                run_id,
                3,
                supervisor_actor(),
                Some("req_000001"),
                EventV1::UserMessageSubmitted(harness_core::event::UserMessageSubmittedEvent {
                    request_id: "req_000001".into(),
                    text: "Implement resume acceptance slice".to_string(),
                }),
            ),
            resume_fixture_event_with_actor_and_correlation(
                run_id,
                4,
                worker.clone(),
                Some("req_000001"),
                EventV1::TaskScheduled(TaskScheduledEvent {
                    task_id: "task_000001".to_string().into(),
                    state: TaskScheduleState::Started,
                    queue_key: Some("provider_model:mock:model-1".to_string()),
                    metadata: None,
                }),
            ),
            resume_fixture_event_with_actor_and_correlation(
                run_id,
                5,
                worker.clone(),
                Some("req_000001"),
                EventV1::ProviderRequestStarted(ProviderRequestStartedEvent {
                    request_id: "req_000002".into(),
                    provider_id: "mock".to_string(),
                    model_id: "model-1".to_string(),
                    prompt_summary: "Implement resume acceptance slice".to_string(),
                    request_digest: "digest-provider-1".to_string(),
                    metadata: None,
                }),
            ),
            resume_fixture_event_with_actor_and_correlation(
                run_id,
                6,
                worker.clone(),
                Some("req_000001"),
                EventV1::ProviderStreamDelta(harness_core::event::ProviderStreamDeltaEvent {
                    request_id: "req_000002".into(),
                    delta: "Preparing resume acceptance tools.".to_string(),
                }),
            ),
            resume_fixture_event_with_actor_and_correlation(
                run_id,
                7,
                worker.clone(),
                Some("req_000001"),
                EventV1::ProviderRequestFinished(
                    harness_core::event::ProviderRequestFinishedEvent {
                        request_id: "req_000002".into(),
                        finish_reason: "done".to_string(),
                        output_digest: Some("digest-provider-output-1".to_string()),
                        usage: None,
                        metadata: None,
                    },
                ),
            ),
            resume_fixture_event_with_actor_and_correlation(
                run_id,
                8,
                worker.clone(),
                Some("req_000001"),
                EventV1::AssistantMessageFinished(
                    harness_core::event::AssistantMessageFinishedEvent {
                        request_id: "req_000002".into(),
                        tool_call_count: 4,
                        assistant_message: None,
                    },
                ),
            ),
            resume_fixture_event_with_actor_and_correlation(
                run_id,
                9,
                worker.clone(),
                Some("req_000001"),
                EventV1::ToolCallRequested(ToolCallRequestedEvent {
                    tool_call_id: "toolcall_000001".into(),
                    tool_id: "skill".to_string(),
                    args_summary: "load karpathy-guidelines".to_string(),
                    args_digest: "digest-skill".to_string(),
                    metadata: Some(test_tool_metadata("skill")),
                }),
            ),
            resume_fixture_event_with_actor_and_correlation(
                run_id,
                10,
                worker.clone(),
                Some("req_000001"),
                EventV1::ToolCallFinished(ToolCallFinishedEvent {
                    tool_call_id: "toolcall_000001".into(),
                    status: ToolCallStatus::Succeeded,
                    output_summary: Some("loaded skill karpathy-guidelines".to_string()),
                    output_digest: Some("digest-skill-output".to_string()),
                    output_json: None,
                    metadata: Some(test_tool_metadata("skill")),
                }),
            ),
            resume_fixture_event_with_actor_and_correlation(
                run_id,
                11,
                worker.clone(),
                Some("req_000001"),
                EventV1::ToolCallRequested(ToolCallRequestedEvent {
                    tool_call_id: "toolcall_000002".into(),
                    tool_id: "todowrite".to_string(),
                    args_summary: "record resume acceptance todo checklist".to_string(),
                    args_digest: "digest-todo".to_string(),
                    metadata: Some(test_tool_metadata("todowrite")),
                }),
            ),
            resume_fixture_event_with_actor_and_correlation(
                run_id,
                12,
                worker.clone(),
                Some("req_000001"),
                EventV1::ToolCallFinished(ToolCallFinishedEvent {
                    tool_call_id: "toolcall_000002".into(),
                    status: ToolCallStatus::Succeeded,
                    output_summary: Some(
                        "todo checklist keeps resume acceptance in progress".to_string(),
                    ),
                    output_digest: Some("digest-todo-output".to_string()),
                    output_json: None,
                    metadata: Some(test_tool_metadata("todowrite")),
                }),
            ),
            resume_fixture_event_with_actor_and_correlation(
                run_id,
                13,
                worker.clone(),
                Some("req_000001"),
                EventV1::ToolCallRequested(ToolCallRequestedEvent {
                    tool_call_id: "toolcall_000003".into(),
                tool_id: "question".to_string(),
                    args_summary: "handoff .agent-harness/plans/run_resume_acceptance_realistic.md"
                        .to_string(),
                    args_digest: "digest-plan".to_string(),
                metadata: Some(test_tool_metadata("question")),
                }),
            ),
            resume_fixture_event_with_actor_and_correlation(
                run_id,
                14,
                worker.clone(),
                Some("req_000001"),
                EventV1::ToolCallFinished(ToolCallFinishedEvent {
                    tool_call_id: "toolcall_000003".into(),
                    status: ToolCallStatus::Succeeded,
                    output_summary: Some(
                        "plan handoff references .agent-harness/plans/run_resume_acceptance_realistic.md"
                            .to_string(),
                    ),
                    output_digest: Some("digest-plan-output".to_string()),
                    output_json: None,
                metadata: Some(test_tool_metadata("question")),
                }),
            ),
            resume_fixture_event_with_actor_and_correlation(
                run_id,
                15,
                worker.clone(),
                Some("req_000001"),
                EventV1::ToolCallRequested(ToolCallRequestedEvent {
                    tool_call_id: "toolcall_000004".into(),
                    tool_id: "shell.run".to_string(),
                    args_summary: shell_args.to_string(),
                    args_digest: "digest-shell".to_string(),
                    metadata: Some(test_tool_metadata("shell.run")),
                }),
            ),
            resume_fixture_event_with_actor_and_correlation(
                run_id,
                16,
                worker.clone(),
                Some("req_000001"),
                EventV1::PermissionRequested(PermissionRequestedEvent {
                    permission_id: "perm_000001".to_string(),
                    kind: "shell".to_string(),
                    tool_call_id: Some("toolcall_000004".into()),
                    summary: "allow resume acceptance shell artifact".to_string(),
                    request_digest: shell_request_digest,
                    timeout_ms: 5_000,
                    default_decision: EventPermissionDecision::Deny,
                }),
            ),
            resume_fixture_event_with_actor_and_correlation(
                run_id,
                17,
                test_system_actor(),
                Some("req_000001"),
                EventV1::PermissionGrantRecorded(
                    harness_core::event::PermissionGrantRecordedEvent { grant: shell_grant },
                ),
            ),
            resume_fixture_event_with_actor_and_correlation(
                run_id,
                18,
                test_system_actor(),
                Some("req_000001"),
                EventV1::PermissionResolved(PermissionResolvedEvent {
                    permission_id: "perm_000001".to_string(),
                    decision: EventPermissionDecision::Allow,
                    reason: Some("operator approved artifact command".to_string()),
                }),
            ),
            resume_fixture_event_with_actor_and_correlation(
                run_id,
                19,
                test_system_actor(),
                Some("req_000001"),
                EventV1::ToolCallStarted(harness_core::event::ToolCallStartedEvent {
                    tool_call_id: "toolcall_000004".into(),
                }),
            ),
            resume_fixture_event_with_actor_and_correlation(
                run_id,
                20,
                worker.clone(),
                Some("req_000001"),
                EventV1::TaskScheduled(TaskScheduledEvent {
                    task_id: "task_000002".to_string().into(),
                    state: TaskScheduleState::Started,
                    queue_key: Some("tool:shell.run".to_string()),
                    metadata: None,
                }),
            ),
            resume_fixture_event_with_actor_and_correlation(
                run_id,
                21,
                test_system_actor(),
                Some("req_000001"),
                EventV1::ArtifactWritten(harness_core::event::ArtifactWrittenEvent {
                    path: artifact_path.to_string(),
                    digest: artifact_digest.clone(),
                    bytes: artifact_body.len() as u64,
                    tool_call_id: Some("toolcall_000004".into()),
                    tool_metadata: Some(harness_core::event::ToolIdentityMetadata {
                        canonical_tool_id: Some("shell.run".to_string()),
                        alias_source_tool_id: None,
                    }),
                    metadata: artifact_metadata,
                }),
            ),
            resume_fixture_event_with_actor_and_correlation(
                run_id,
                22,
                worker.clone(),
                Some("req_000001"),
                EventV1::TaskCompleted(TaskCompletedEvent {
                    task_id: "task_000002".to_string().into(),
                    result_summary: "resume artifact written".to_string(),
                    result_digest: "digest-shell-task".to_string(),
                    metadata: Some(TaskCompletionMetadata {
                        lineage: None,
                        task_scope: Some(harness_core::event::TaskTerminalScope::ToolCall),
                        timing: None,
                        hook_executions: Vec::new(),
                    }),
                }),
            ),
            resume_fixture_event_with_actor_and_correlation(
                run_id,
                23,
                test_system_actor(),
                Some("req_000001"),
                EventV1::ToolCallFinished(ToolCallFinishedEvent {
                    tool_call_id: "toolcall_000004".into(),
                    status: ToolCallStatus::Succeeded,
                    output_summary: Some("resume artifact written".to_string()),
                    output_digest: Some("digest-shell-output".to_string()),
                    output_json: Some(json!({
                        "artifact_refs": [{"path": artifact_path, "digest": artifact_digest}],
                    })),
                    metadata: Some(ToolCallMetadata {
                        canonical_tool_id: Some("shell.run".to_string()),
                        alias_source_tool_id: None,
                        lineage: None,
                        artifact_refs: vec![harness_core::event::EventArtifactRef {
                            path: artifact_path.to_string(),
                            digest: Some(artifact_digest.clone()),
                        }],
                        timing: None,
                        hook_executions: Vec::new(),
                    }),
                }),
            ),
            resume_fixture_event_with_actor_and_correlation(
                run_id,
                24,
                worker.clone(),
                Some("req_000001"),
                EventV1::ProviderRequestStarted(ProviderRequestStartedEvent {
                    request_id: "req_000003".into(),
                    provider_id: "mock".to_string(),
                    model_id: "model-1".to_string(),
                    prompt_summary: "tool result + finish".to_string(),
                    request_digest: "digest-provider-2".to_string(),
                    metadata: None,
                }),
            ),
            resume_fixture_event_with_actor_and_correlation(
                run_id,
                25,
                worker.clone(),
                Some("req_000001"),
                EventV1::ProviderStreamDelta(harness_core::event::ProviderStreamDeltaEvent {
                    request_id: "req_000003".into(),
                    delta: "First turn final answer after artifact, skill, todo, and plan."
                        .to_string(),
                }),
            ),
            resume_fixture_event_with_actor_and_correlation(
                run_id,
                26,
                worker.clone(),
                Some("req_000001"),
                EventV1::ProviderRequestFinished(
                    harness_core::event::ProviderRequestFinishedEvent {
                        request_id: "req_000003".into(),
                        finish_reason: "done".to_string(),
                        output_digest: Some("digest-provider-output-2".to_string()),
                        usage: None,
                        metadata: None,
                    },
                ),
            ),
            resume_fixture_event_with_actor_and_correlation(
                run_id,
                27,
                worker.clone(),
                Some("req_000001"),
                EventV1::AssistantMessageFinished(
                    harness_core::event::AssistantMessageFinishedEvent {
                        request_id: "req_000003".into(),
                        tool_call_count: 0,
                        assistant_message: None,
                    },
                ),
            ),
            resume_fixture_event_with_actor_and_correlation(
                run_id,
                28,
                worker.clone(),
                Some("req_000001"),
                EventV1::TaskCompleted(TaskCompletedEvent {
                    task_id: "task_000001".to_string().into(),
                    result_summary:
                        "First turn final answer after artifact, skill, todo, and plan."
                            .to_string(),
                    result_digest: "digest-agent-turn-1".to_string(),
                    metadata: Some(TaskCompletionMetadata {
                        lineage: None,
                        task_scope: Some(harness_core::event::TaskTerminalScope::AgentTurn),
                        timing: None,
                        hook_executions: Vec::new(),
                    }),
                }),
            ),
            resume_fixture_event_with_actor_and_correlation(
                run_id,
                29,
                supervisor_actor(),
                Some("req_000004"),
                EventV1::UserMessageSubmitted(harness_core::event::UserMessageSubmittedEvent {
                    request_id: "req_000004".into(),
                    text: "Continue after resume acceptance checkpoint".to_string(),
                }),
            ),
            resume_fixture_event_with_actor_and_correlation(
                run_id,
                30,
                worker.clone(),
                Some("req_000004"),
                EventV1::TaskScheduled(TaskScheduledEvent {
                    task_id: "task_000003".to_string().into(),
                    state: TaskScheduleState::Started,
                    queue_key: Some("provider_model:mock:model-1".to_string()),
                    metadata: None,
                }),
            ),
            resume_fixture_event_with_actor_and_correlation(
                run_id,
                31,
                worker.clone(),
                Some("req_000004"),
                EventV1::ProviderRequestStarted(ProviderRequestStartedEvent {
                    request_id: "req_000005".into(),
                    provider_id: "mock".to_string(),
                    model_id: "model-1".to_string(),
                    prompt_summary: "Continue after resume acceptance checkpoint".to_string(),
                    request_digest: "digest-provider-3".to_string(),
                    metadata: None,
                }),
            ),
            resume_fixture_event_with_actor_and_correlation(
                run_id,
                32,
                worker.clone(),
                Some("req_000004"),
                EventV1::ProviderStreamDelta(harness_core::event::ProviderStreamDeltaEvent {
                    request_id: "req_000005".into(),
                    delta: "Second turn recorded todos and plan still available.".to_string(),
                }),
            ),
            resume_fixture_event_with_actor_and_correlation(
                run_id,
                33,
                worker.clone(),
                Some("req_000004"),
                EventV1::ProviderRequestFinished(
                    harness_core::event::ProviderRequestFinishedEvent {
                        request_id: "req_000005".into(),
                        finish_reason: "done".to_string(),
                        output_digest: Some("digest-provider-output-3".to_string()),
                        usage: None,
                        metadata: None,
                    },
                ),
            ),
            resume_fixture_event_with_actor_and_correlation(
                run_id,
                34,
                worker.clone(),
                Some("req_000004"),
                EventV1::AssistantMessageFinished(
                    harness_core::event::AssistantMessageFinishedEvent {
                        request_id: "req_000005".into(),
                        tool_call_count: 0,
                        assistant_message: None,
                    },
                ),
            ),
            resume_fixture_event_with_actor_and_correlation(
                run_id,
                35,
                worker,
                Some("req_000004"),
                EventV1::TaskCompleted(TaskCompletedEvent {
                    task_id: "task_000003".to_string().into(),
                    result_summary: "Second turn recorded todos and plan still available."
                        .to_string(),
                    result_digest: "digest-agent-turn-2".to_string(),
                    metadata: Some(TaskCompletionMetadata {
                        lineage: None,
                        task_scope: Some(harness_core::event::TaskTerminalScope::AgentTurn),
                        timing: None,
                        hook_executions: Vec::new(),
                    }),
                }),
            ),
            resume_fixture_event(
                run_id,
                36,
                EventV1::RunFinished(RunFinishedEvent {
                    summary: "interrupted segment persisted".to_string(),
                }),
            ),
        ],
    );
    let artifact_abs_path = temp_dir.path().join(run_id).join(artifact_path);
    fs::create_dir_all(artifact_abs_path.parent().unwrap_or_abort()).unwrap_or_abort();
    fs::write(&artifact_abs_path, artifact_body).unwrap_or_abort();

    ResumeAcceptanceFixture {
        temp_dir,
        run_id,
        artifact_path,
        artifact_digest,
        shell_args,
        artifact_abs_path,
    }
}

pub(super) fn test_digest12(bytes: &[u8]) -> String {
    blake3::hash(bytes).to_hex().chars().take(12).collect()
}

pub(super) fn test_permission_request_digest(
    tool_id: &str,
    args_json: &serde_json::Value,
) -> String {
    let canonical = serde_json::to_vec(args_json).unwrap_or_else(|_| b"null".to_vec());
    let mut bytes = Vec::with_capacity(tool_id.len() + 1 + canonical.len());
    bytes.extend_from_slice(tool_id.as_bytes());
    bytes.push(0x1f);
    bytes.extend_from_slice(&canonical);
    test_digest12(&bytes)
}

fn test_tool_metadata(canonical_tool_id: &str) -> ToolCallMetadata {
    ToolCallMetadata {
        canonical_tool_id: Some(canonical_tool_id.to_string()),
        alias_source_tool_id: None,
        lineage: None,
        artifact_refs: Vec::new(),
        timing: None,
        hook_executions: Vec::new(),
    }
}

fn test_system_actor() -> EventActor {
    EventActor::new(ActorKind::System, Some("coordinator".to_string()))
}
