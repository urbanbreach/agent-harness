use std::fs;
use std::sync::Arc;

mod common;

use common::{
    allow_all_permission_policy, anonymous_supervisor_actor, read_events, setup_workspace_fixture,
};
use harness_core::clock::RealClock;
use harness_core::config::{CategoryPermissions, PermissionMode, ShellAllowlist};
use harness_core::coord::{spawn_coordinator, CoordinatorConfig, CoordinatorHandle, RunInfo};
use harness_core::event::EventV1;
use harness_core::perm::PermissionPolicy;
use harness_core::redact::DefaultRedactor;
use harness_core::workflow::{
    WorkflowEvidenceRequest, WorkflowStartRequest, SIMULATED_TOOL_EVIDENCE_CATEGORY,
    WORKFLOW_QUESTION_EVIDENCE_CATEGORY,
};
use harness_core::workflow_closeout::WORKFLOW_CLOSEOUT_DIMENSION_QUESTION;
use harness_tools::coordinator_registry;
use serde_json::json;

async fn spawn_run(
    permission_policy: PermissionPolicy,
) -> (common::WorkspaceFixture, CoordinatorHandle, RunInfo) {
    let workspace = setup_workspace_fixture();
    let session_dir = workspace.workspace().join("session-dir");
    fs::create_dir_all(&session_dir).expect("session dir");

    let mut config = CoordinatorConfig::new(session_dir);
    config.permission_policy = permission_policy;
    config.tool_registry = Arc::new(coordinator_registry(ShellAllowlist::default()));
    let handle = spawn_coordinator(
        config,
        Arc::new(RealClock::new()),
        Arc::new(DefaultRedactor::default()),
    );
    let run = handle
        .start_run("native_workflow_tools", workspace.workspace())
        .await
        .expect("start run");
    (workspace, handle, run)
}

async fn start_workflow(handle: &CoordinatorHandle, workflow_id: &str) {
    handle
        .start_workflow(
            anonymous_supervisor_actor(),
            WorkflowStartRequest {
                workflow_id: workflow_id.to_string(),
                mode: "workflow.run".to_string(),
                owner: "leader".to_string(),
                lane: Some("lane.leader".to_string()),
                title: Some("native workflow tool test".to_string()),
                idempotency_key: None,
            },
        )
        .await
        .expect("start workflow");
}

async fn record_required_evidence(handle: &CoordinatorHandle, workflow_id: &str) {
    for category in [
        harness_core::context_snapshot::CONTEXT_SNAPSHOT_EVIDENCE_CATEGORY,
        SIMULATED_TOOL_EVIDENCE_CATEGORY,
    ] {
        handle
            .record_workflow_evidence(
                anonymous_supervisor_actor(),
                WorkflowEvidenceRequest {
                    workflow_id: workflow_id.to_string(),
                    category: category.to_string(),
                    summary: format!("{category} present"),
                    artifact_path: Some(format!("artifacts/{category}.json")),
                    artifact_digest: Some("digest".to_string()),
                    acceptance_ref: Some(category.to_string()),
                    metadata: Default::default(),
                },
            )
            .await
            .expect("record evidence");
    }
}

fn allow_question_record_policy() -> PermissionPolicy {
    allow_all_permission_policy().with_category_override(
        "deep",
        CategoryPermissions {
            question: Some(PermissionMode::Allow),
            ..CategoryPermissions::default()
        },
    )
}

#[tokio::test]
async fn workflow_status_and_dossier_tools_share_closeout_contracts() {
    let (_workspace, handle, _run) = spawn_run(allow_all_permission_policy()).await;
    start_workflow(&handle, "wf_tools").await;
    record_required_evidence(&handle, "wf_tools").await;

    let status = handle
        .execute_agent_tool_call(
            anonymous_supervisor_actor(),
            Some("deep".to_string()),
            "workflow_status",
            json!({"workflow_id": "wf_tools"}),
        )
        .await
        .expect("workflow_status");
    let status_json = status.structured_json.expect("status json");
    assert_eq!(status_json["workflow_count"], json!(1));
    assert_eq!(
        status_json["closeout"]["wf_tools"]["closeout"]["overall_allowed"],
        json!(true)
    );
    assert!(
        status_json["catalog_health"]["visible"].is_array(),
        "catalog health is present without prompt contents"
    );

    let dossier = handle
        .execute_agent_tool_call(
            anonymous_supervisor_actor(),
            Some("deep".to_string()),
            "workflow_dossier_export",
            json!({
                "workflow_id": "wf_tools",
                "format": "json",
                "output_artifact": true,
                "slug": "wf-tools"
            }),
        )
        .await
        .expect("workflow_dossier_export");
    let dossier_json = dossier.structured_json.expect("dossier json");
    assert_eq!(
        dossier_json["dossier"]["workflows"][0]["closeout"]["overall_allowed"],
        json!(true)
    );
    assert_eq!(dossier.artifacts.len(), 1);
}

#[tokio::test]
async fn workflow_question_record_tool_projects_lifecycle_and_stable_failures() {
    let (_workspace, handle, run) = spawn_run(allow_question_record_policy()).await;
    start_workflow(&handle, "wf_questions").await;

    let unknown = handle
        .execute_agent_tool_call(
            anonymous_supervisor_actor(),
            Some("deep".to_string()),
            "workflow_question_record",
            json!({
                "workflow_id": "wf_questions",
                "question_id": "q-1",
                "status": "answered",
                "answer_ref": "answers/q-1.json"
            }),
        )
        .await
        .expect_err("answering an unknown question must fail closed");
    assert!(
        unknown.contains("reason_code=unknown_question"),
        "unexpected unknown-question error: {unknown}"
    );

    let asked = handle
        .execute_agent_tool_call(
            anonymous_supervisor_actor(),
            Some("deep".to_string()),
            "workflow_question_record",
            json!({
                "workflow_id": "wf_questions",
                "question_id": "q-1",
                "status": "asked",
                "prompt_ref": "questions/q-1.json"
            }),
        )
        .await
        .expect("record asked question");
    let asked_json = asked.structured_json.expect("asked json");
    assert_eq!(asked_json["question"]["status"], json!("asked"));
    assert_eq!(
        asked_json["closeout"]["dimensions"]
            .as_array()
            .expect("dimensions")
            .iter()
            .find(|dimension| dimension["id"] == json!(WORKFLOW_CLOSEOUT_DIMENSION_QUESTION))
            .expect("question dimension")["allowed"],
        json!(false)
    );

    let malformed = handle
        .execute_agent_tool_call(
            anonymous_supervisor_actor(),
            Some("deep".to_string()),
            "workflow_question_record",
            json!({
                "workflow_id": "wf_questions",
                "question_id": "q-1",
                "status": "answered"
            }),
        )
        .await
        .expect_err("answered state requires answer_ref");
    assert!(
        malformed.contains("reason_code=malformed_answer"),
        "unexpected malformed-answer error: {malformed}"
    );

    let answered = handle
        .execute_agent_tool_call(
            anonymous_supervisor_actor(),
            Some("deep".to_string()),
            "workflow_question_record",
            json!({
                "workflow_id": "wf_questions",
                "question_id": "q-1",
                "status": "answered",
                "answer_ref": "answers/q-1.json"
            }),
        )
        .await
        .expect("record answered question");
    let answered_json = answered.structured_json.expect("answered json");
    assert_eq!(answered_json["question"]["status"], json!("answered"));
    assert_eq!(
        answered_json["closeout"]["dimensions"]
            .as_array()
            .expect("dimensions")
            .iter()
            .find(|dimension| dimension["id"] == json!(WORKFLOW_CLOSEOUT_DIMENSION_QUESTION))
            .expect("question dimension")["allowed"],
        json!(true)
    );

    handle
        .execute_agent_tool_call(
            anonymous_supervisor_actor(),
            Some("deep".to_string()),
            "workflow_question_record",
            json!({
                "workflow_id": "wf_questions",
                "question_id": "q-1",
                "status": "closed"
            }),
        )
        .await
        .expect("record closed question");

    let closed = handle
        .execute_agent_tool_call(
            anonymous_supervisor_actor(),
            Some("deep".to_string()),
            "workflow_question_record",
            json!({
                "workflow_id": "wf_questions",
                "question_id": "q-1",
                "status": "answered",
                "answer_ref": "answers/q-1-late.json"
            }),
        )
        .await
        .expect_err("answering a closed question must fail closed");
    assert!(
        closed.contains("reason_code=question_closed"),
        "unexpected closed-question error: {closed}"
    );

    let events = read_events(&run.events_path);
    let question_events = events
        .iter()
        .filter(|event| {
            matches!(
                &event.payload,
                EventV1::WorkflowEvidenceRecorded(payload)
                    if payload.category == WORKFLOW_QUESTION_EVIDENCE_CATEGORY
            )
        })
        .count();
    assert_eq!(question_events, 3);
}

#[tokio::test]
async fn workflow_signoff_rejects_invalid_or_premature_mutations() {
    let (_workspace, handle, run) = spawn_run(allow_all_permission_policy()).await;
    start_workflow(&handle, "wf_blocked").await;

    let waiver = handle
        .execute_agent_tool_call(
            anonymous_supervisor_actor(),
            Some("deep".to_string()),
            "workflow_signoff",
            json!({"workflow_id": "wf_blocked", "decision": "waive"}),
        )
        .await
        .expect_err("waive without reason/scope must fail closed");
    assert!(waiver.contains("reason is required"));

    let approve = handle
        .execute_agent_tool_call(
            anonymous_supervisor_actor(),
            Some("deep".to_string()),
            "workflow_signoff",
            json!({"workflow_id": "wf_blocked", "decision": "approve"}),
        )
        .await
        .expect_err("premature approve must fail closed");
    assert!(
        approve.contains("closeout policy") && approve.contains("denied terminal approval"),
        "unexpected approve error: {approve}"
    );

    let events = read_events(&run.events_path);
    assert!(events
        .iter()
        .any(|event| matches!(event.payload, EventV1::WorkflowTransitionDenied(_))));
    assert!(!events
        .iter()
        .any(|event| matches!(event.payload, EventV1::WorkflowCompleted(_))));
}

#[tokio::test]
async fn workflow_signoff_requires_edit_permission() {
    let (_workspace, handle, _run) = spawn_run(PermissionPolicy::new(
        PermissionMode::Deny,
        PermissionMode::Allow,
        PermissionMode::Allow,
    ))
    .await;
    start_workflow(&handle, "wf_permission").await;
    record_required_evidence(&handle, "wf_permission").await;

    let err = handle
        .execute_agent_tool_call(
            anonymous_supervisor_actor(),
            Some("deep".to_string()),
            "workflow_signoff",
            json!({"workflow_id": "wf_permission", "decision": "approve"}),
        )
        .await
        .expect_err("signoff should require edit permission");
    assert!(
        err.contains("PermissionDenied")
            || err.contains("permission denied")
            || err.contains("policy denied request"),
        "unexpected permission error: {err}"
    );
}
