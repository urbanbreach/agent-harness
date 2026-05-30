mod resume_acceptance_fixture {
    use super::*;
    include!("common/resume_acceptance_fixture.rs");
}

use resume_acceptance_fixture::{
    test_digest12, test_permission_request_digest, write_resume_acceptance_fixture,
    ResumeAcceptanceFixture,
};

#[tokio::test]
async fn resume_acceptance_restores_realistic_interrupted_session_and_continues() {
    // arrange
    let ResumeAcceptanceFixture {
        temp_dir,
        run_id,
        artifact_path,
        artifact_digest,
        shell_args,
        artifact_abs_path,
    } = write_resume_acceptance_fixture();
let provider = CapturingProvider::new(vec!["post-resume answer"]);
    let coordinator =
        test_resume_coordinator_with_provider(temp_dir.path(), Arc::new(provider.clone()));

    // act
    let run = coordinator
        .resume_run(run_id, "resume acceptance coding session")
        .await
        .expect("resume run");
    // assert
    assert!(artifact_abs_path.exists(), "artifact file should still exist");

    let plan_after_resume = inspect_resume_plan(&run.run_dir);
    assert!(plan_after_resume.session_artifacts.values().any(|artifact| {
        artifact.path == artifact_path && artifact.digest.as_deref() == Some(artifact_digest.as_str())
    }));
    assert!(plan_after_resume.tool_calls.contains_key("toolcall_000001"));
    assert!(plan_after_resume.tool_calls.contains_key("toolcall_000002"));
    assert!(plan_after_resume.tool_calls.contains_key("toolcall_000003"));
    assert!(plan_after_resume.tool_calls.contains_key("toolcall_000004"));
    assert!(plan_after_resume.pending_permissions.is_empty());
    assert!(plan_after_resume
        .active_permission_grants
        .authorizes(&harness_core::perm::PermissionGrantRequest {
            kind: harness_core::perm::PermissionKind::Shell,
            tool: harness_core::perm::PermissionToolSelector {
                effective_tool_id: "shell.run".to_string(),
                canonical_tool_id: Some("shell.run".to_string()),
            },
            matcher: harness_core::perm::PermissionGrantMatcher::ShellCommand {
                command_digest: test_digest12(b"printf resume acceptance"),
                request_digest: test_permission_request_digest("shell.run", &shell_args),
            },
        }));

    let before_grant_reuse = load_events(&run.events_path)
        .iter()
        .filter(|event| matches!(event.payload, EventV1::PermissionRequested(_)))
        .count();
    let granted_tool_call_id = coordinator
        .request_tool_call(
            supervisor_actor(),
            Some("deep".to_string()),
            "shell.run",
            shell_args,
        )
        .await
        .expect("restored grant should let same shell command start without asking again");
    assert_eq!(granted_tool_call_id, "toolcall_000005");
    let events_after_grant_reuse = wait_for_events(&run.events_path, Duration::from_secs(1), |events| {
        events.iter().any(|event| {
            matches!(
                &event.payload,
                EventV1::ToolCallFinished(payload)
                    if payload.tool_call_id == "toolcall_000005"
                        && payload.status == ToolCallStatus::Succeeded
            )
        })
    })
    .await;
    let after_grant_reuse = events_after_grant_reuse
        .iter()
        .filter(|event| matches!(event.payload, EventV1::PermissionRequested(_)))
        .count();
    assert_eq!(after_grant_reuse, before_grant_reuse);

    let request_id = coordinator
        .request_agent_turn(
            supervisor_actor(),
            "agent_000001",
            "Post-resume verification prompt",
        )
        .await
        .expect("resumed agent should accept next turn");
    assert_eq!(request_id, "req_000006");
    wait_for_events(&run.events_path, Duration::from_secs(1), |events| {
        events.iter().any(|event| {
            matches!(
                &event.payload,
                EventV1::TaskCompleted(payload)
                    if event.correlation_id.as_deref() == Some("req_000006")
                        && payload.result_summary == "post-resume answer"
            )
        })
    })
    .await;
    coordinator.stop_run().await.expect("stop resumed run");

    let requests = provider.requests();
    assert_eq!(requests.len(), 1, "expected one post-resume provider call");
    let provider_messages = requests[0]
        .messages
        .iter()
        .map(|message| (message.role.clone(), message.content.clone()))
        .collect::<Vec<_>>();
    assert!(provider_messages.contains(&(
        MessageRole::User,
        "Implement resume acceptance slice".to_string(),
    )));
    assert!(provider_messages.contains(&(
        MessageRole::Tool,
        "loaded skill karpathy-guidelines".to_string(),
    )));
    assert!(provider_messages.contains(&(
        MessageRole::Tool,
        "todo checklist keeps resume acceptance in progress".to_string(),
    )));
    assert!(provider_messages.contains(&(
        MessageRole::Tool,
        "plan handoff references .agent-harness/plans/run_resume_acceptance_realistic.md"
            .to_string(),
    )));
    assert!(provider_messages.contains(&(
        MessageRole::Tool,
        "resume artifact written".to_string(),
    )));
    assert!(provider_messages.contains(&(
        MessageRole::User,
        "Continue after resume acceptance checkpoint".to_string(),
    )));
    assert_eq!(
        provider_messages.last(),
        Some(&(
            MessageRole::User,
            "Post-resume verification prompt".to_string(),
        ))
    );
}
