use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use harness_core::clock::{Clock, FakeClock};
use harness_core::config::{PermissionMode, ShellAllowlist};
use harness_core::context_snapshot::{ContextSnapshotInput, ContextSnapshotOptions};
use harness_core::continuation::ContinuationBounds;
use harness_core::coord::{spawn_coordinator, CoordinatorConfig, CoordinatorHandle, RunInfo};
use harness_core::event::{
    ActorKind, EventActor, EventEnvelopeV1, EventV1, PersistentTask, PersistentTaskStatus,
    PersistentTaskUpdatedEvent, ToolCallStatus, WorkflowEventMetadata,
};
use harness_core::perm::{PermissionDecision, PermissionPolicy};
use harness_core::persistent_task::project_persistent_tasks;
use harness_core::proj::SessionModeSource;
use harness_core::redact::DefaultRedactor;
use harness_core::run_dossier::{build_run_dossier_with_tasks_and_closeout_policy, RunDossier};
use harness_core::workflow::{
    project_workflows, WorkflowEvidenceRequest, WorkflowProjection, WorkflowSignoffPolicy,
    WorkflowStartRequest, WorkflowStartResult, SIMULATED_TOOL_EVIDENCE_CATEGORY,
    WORKFLOW_QUESTION_EVIDENCE_CATEGORY, WORKFLOW_QUESTION_METADATA_ANSWER_REF,
    WORKFLOW_QUESTION_METADATA_ID, WORKFLOW_QUESTION_METADATA_STATUS,
    WORKFLOW_QUESTION_STATUS_ANSWERED, WORKFLOW_QUESTION_STATUS_ASKED, WORKFLOW_TASK_METADATA_KEY,
};
use harness_core::workflow_closeout::{
    WorkflowCloseoutPolicy, WorkflowCloseoutPolicyConfig, WORKFLOW_CLOSEOUT_DEFAULT_POLICY_ID,
    WORKFLOW_CLOSEOUT_DOSSIER_EVIDENCE_CATEGORY,
};
use harness_providers::mock::{request_digest, MockProvider};
use harness_providers::{
    CompletionMessage, CompletionRequest, CompletionUsage, MessageRole, Provider,
    ProviderStreamEvent,
};
use harness_tools::coordinator_registry;
use serde::{Deserialize, Serialize};
use serde_json::json;
use tokio_stream::StreamExt;

pub const SIMULATOR_WORKFLOW_ID: &str = "wf_deterministic_goal_loop";
const SIMULATOR_RUN_ID: &str = "run_workflow_simulator";

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WorkflowSimulationReport {
    pub run_id: String,
    pub run_dir: PathBuf,
    pub workflow_id: String,
    pub provider_output: String,
    pub missing_evidence_blocked_completion: bool,
    pub active_continuation_blocked_completion: bool,
    pub permission_denied_after_start: bool,
    pub owner_conflict_denied: bool,
    pub pending_task_blocked_completion: bool,
    pub question_blocked_completion: bool,
    pub missing_dossier_blocked_completion: bool,
    pub late_completion_ignored: bool,
    pub closeout_replay_equivalent: bool,
    pub replay_event_count: usize,
    pub projection: WorkflowProjection,
    pub dossier: RunDossier,
}

pub async fn run_deterministic_workflow_simulator(
    root: impl AsRef<Path>,
) -> Result<WorkflowSimulationReport, String> {
    let root = root.as_ref();
    let session_dir = root.join("sessions");
    let workspace = root.join("workspace");
    fs::create_dir_all(&workspace).map_err(|err| {
        format!(
            "failed to create simulator workspace {}: {err}",
            workspace.display()
        )
    })?;

    let provider_output = scripted_provider_output().await?;
    let (coordinator, run) = start_simulator_run(&session_dir, &workspace).await?;
    let actor = supervisor_actor();

    coordinator
        .start_workflow(
            actor.clone(),
            WorkflowStartRequest {
                workflow_id: SIMULATOR_WORKFLOW_ID.to_string(),
                mode: "workflow.run".to_string(),
                owner: "simulator".to_string(),
                lane: Some("simulated".to_string()),
                title: Some("Deterministic goal-loop demonstrator".to_string()),
                idempotency_key: Some("simulator-goal-loop".to_string()),
            },
        )
        .await
        .map_err(|err| err.to_string())?;

    let conflict = coordinator
        .start_workflow(
            actor.clone(),
            WorkflowStartRequest {
                workflow_id: SIMULATOR_WORKFLOW_ID.to_string(),
                mode: "workflow.run".to_string(),
                owner: "competing-owner".to_string(),
                lane: Some("simulated".to_string()),
                title: None,
                idempotency_key: None,
            },
        )
        .await
        .map_err(|err| err.to_string())?;
    let owner_conflict_denied = matches!(conflict, WorkflowStartResult::Denied { .. });

    let mut snapshot_input = ContextSnapshotInput::new(
        "/workflow run",
        "Demonstrate deterministic workflow signoff without live dependencies",
        "Replay-derived dossier with mapped evidence",
    );
    snapshot_input.probable_intent =
        "Run a simulated goal-loop to prove workflow evidence gates".to_string();

    coordinator
        .write_context_snapshot(
            actor.clone(),
            Some(SIMULATOR_WORKFLOW_ID.to_string()),
            snapshot_input,
            ContextSnapshotOptions::default(),
        )
        .await
        .map_err(|err| err.to_string())?;

    coordinator
        .start_workflow_continuation(
            actor.clone(),
            "workflow.work_loop",
            "/workflow run --simulated",
            ContinuationBounds {
                max_iterations: 1,
                max_wall_clock_ms: 1_000,
                max_provider_calls: 1,
                max_tool_calls: 4,
            },
            WorkflowEventMetadata {
                workflow_id: Some(SIMULATOR_WORKFLOW_ID.to_string()),
                lane: Some("lane.delivery".to_string()),
                iteration: Some(0),
                stop_reason: None,
                evidence_category: Some("evidence.verification".to_string()),
                owner: Some("simulator".to_string()),
            },
        )
        .await
        .map_err(|err| err.to_string())?;
    let active_continuation_blocked_completion = coordinator
        .complete_workflow_with_closeout_policy(
            actor.clone(),
            SIMULATOR_WORKFLOW_ID,
            "outcome.finished",
            "attempted closeout while continuation was still active",
            "simulator",
            WorkflowSignoffPolicy::simulator_default(),
            closeout_policy_without_evidence_or_dossier(),
        )
        .await
        .is_err();
    coordinator
        .stop_continuation(
            actor.clone(),
            "simulated work loop reached acceptance evidence",
        )
        .await
        .map_err(|err| err.to_string())?;

    let missing_readiness = !project_current_workflows(&run)?
        .signoff_readiness(
            SIMULATOR_WORKFLOW_ID,
            WorkflowSignoffPolicy::simulator_default().required_evidence_categories(),
        )
        .allowed;
    if missing_readiness {
        coordinator
            .record_workflow_operator_decision(
                actor.clone(),
                SIMULATOR_WORKFLOW_ID,
                "request-evidence",
                "simulator",
                Some("simulated tool evidence is missing".to_string()),
                None,
            )
            .await
            .map_err(|err| err.to_string())?;
    }
    let missing_evidence_blocked_completion = coordinator
        .complete_workflow_with_signoff_policy(
            actor.clone(),
            SIMULATOR_WORKFLOW_ID,
            "outcome.finished",
            "attempted signoff before mapped simulator evidence",
            "simulator",
            WorkflowSignoffPolicy::simulator_default(),
        )
        .await
        .is_err();

    let denied = run_permission_gated_noop(&coordinator, actor.clone(), "perm_000001", false).await;
    let permission_denied_after_start = denied.is_err();
    let tool_result =
        run_permission_gated_noop(&coordinator, actor.clone(), "perm_000002", true).await?;

    coordinator
        .record_workflow_evidence(
            actor.clone(),
            WorkflowEvidenceRequest {
                workflow_id: SIMULATOR_WORKFLOW_ID.to_string(),
                category: SIMULATED_TOOL_EVIDENCE_CATEGORY.to_string(),
                summary: format!("deterministic no-op tool completed: {}", tool_result.trim()),
                artifact_path: None,
                artifact_digest: None,
                acceptance_ref: Some("acceptance.noop-tool".to_string()),
                metadata: BTreeMap::from([
                    ("tool_id".to_string(), "bash".to_string()),
                    ("side_effect_class".to_string(), "no-op".to_string()),
                ]),
            },
        )
        .await
        .map_err(|err| err.to_string())?;

    coordinator
        .record_workflow_evidence(
            actor.clone(),
            WorkflowEvidenceRequest {
                workflow_id: SIMULATOR_WORKFLOW_ID.to_string(),
                category: WORKFLOW_QUESTION_EVIDENCE_CATEGORY.to_string(),
                summary: "operator clarification is pending".to_string(),
                artifact_path: None,
                artifact_digest: None,
                acceptance_ref: Some("question.closeout-ready".to_string()),
                metadata: BTreeMap::from([
                    (
                        WORKFLOW_QUESTION_METADATA_ID.to_string(),
                        "question.closeout-ready".to_string(),
                    ),
                    (
                        WORKFLOW_QUESTION_METADATA_STATUS.to_string(),
                        WORKFLOW_QUESTION_STATUS_ASKED.to_string(),
                    ),
                ]),
            },
        )
        .await
        .map_err(|err| err.to_string())?;
    let question_blocked_completion = coordinator
        .complete_workflow_with_closeout_policy(
            actor.clone(),
            SIMULATOR_WORKFLOW_ID,
            "outcome.finished",
            "attempted closeout with an unanswered operator question",
            "simulator",
            WorkflowSignoffPolicy::simulator_default(),
            closeout_policy_without_evidence_or_dossier(),
        )
        .await
        .is_err();
    coordinator
        .record_workflow_evidence(
            actor.clone(),
            WorkflowEvidenceRequest {
                workflow_id: SIMULATOR_WORKFLOW_ID.to_string(),
                category: WORKFLOW_QUESTION_EVIDENCE_CATEGORY.to_string(),
                summary: "operator clarification was answered".to_string(),
                artifact_path: None,
                artifact_digest: None,
                acceptance_ref: Some("question.closeout-ready".to_string()),
                metadata: BTreeMap::from([
                    (
                        WORKFLOW_QUESTION_METADATA_ID.to_string(),
                        "question.closeout-ready".to_string(),
                    ),
                    (
                        WORKFLOW_QUESTION_METADATA_STATUS.to_string(),
                        WORKFLOW_QUESTION_STATUS_ANSWERED.to_string(),
                    ),
                    (
                        WORKFLOW_QUESTION_METADATA_ANSWER_REF.to_string(),
                        "answers/closeout-ready.json".to_string(),
                    ),
                ]),
            },
        )
        .await
        .map_err(|err| err.to_string())?;

    let ready = project_current_workflows(&run)?.signoff_readiness(
        SIMULATOR_WORKFLOW_ID,
        WorkflowSignoffPolicy::simulator_default().required_evidence_categories(),
    );
    if !ready.allowed {
        return Err(format!(
            "simulator signoff unexpectedly blocked after evidence: {:?}",
            ready.missing_evidence_categories
        ));
    }

    coordinator
        .create_persistent_task(
            actor.clone(),
            PersistentTask {
                version: 1,
                task_id: "pt_simulator_verify".to_string(),
                run_id: None,
                thread_id: None,
                subject: "Verify simulator evidence".to_string(),
                description: "Workflow-owned task must complete before signoff".to_string(),
                status: PersistentTaskStatus::Pending,
                active_form: None,
                owner: Some("simulator".to_string()),
                blocks: Vec::new(),
                blocked_by: Vec::new(),
                metadata: BTreeMap::from([(
                    WORKFLOW_TASK_METADATA_KEY.to_string(),
                    SIMULATOR_WORKFLOW_ID.to_string(),
                )]),
            },
        )
        .await
        .map_err(|err| err.to_string())?;
    let pending_task_blocked_completion = coordinator
        .complete_workflow_with_signoff_policy(
            actor.clone(),
            SIMULATOR_WORKFLOW_ID,
            "outcome.finished",
            "attempted signoff with a pending workflow-owned task",
            "simulator",
            WorkflowSignoffPolicy::simulator_default(),
        )
        .await
        .is_err();
    coordinator
        .update_persistent_task(
            actor.clone(),
            PersistentTaskUpdatedEvent {
                task_id: "pt_simulator_verify".to_string(),
                status: PersistentTaskStatus::Completed,
                active_form: None,
                owner: Some("simulator".to_string()),
                description: None,
                subject: None,
                blocked_by: None,
                metadata: BTreeMap::from([(
                    "completion_evidence".to_string(),
                    "acceptance.noop-tool".to_string(),
                )]),
            },
        )
        .await
        .map_err(|err| err.to_string())?;

    let closeout_policy = closeout_policy_requiring_dossier_artifact();
    let missing_dossier_blocked_completion = coordinator
        .complete_workflow_with_closeout_policy(
            actor.clone(),
            SIMULATOR_WORKFLOW_ID,
            "outcome.finished",
            "attempted closeout before dossier export evidence",
            "simulator",
            WorkflowSignoffPolicy::simulator_default(),
            closeout_policy.clone(),
        )
        .await
        .is_err();
    coordinator
        .record_workflow_evidence(
            actor.clone(),
            WorkflowEvidenceRequest {
                workflow_id: SIMULATOR_WORKFLOW_ID.to_string(),
                category: WORKFLOW_CLOSEOUT_DOSSIER_EVIDENCE_CATEGORY.to_string(),
                summary: "replay-derived dossier export recorded".to_string(),
                artifact_path: Some("artifacts/workflow_dossiers/simulator.json".to_string()),
                artifact_digest: Some("simulator-dossier-digest".to_string()),
                acceptance_ref: Some("dossier.simulator".to_string()),
                metadata: BTreeMap::from([("dossier_status".to_string(), "exported".to_string())]),
            },
        )
        .await
        .map_err(|err| err.to_string())?;

    coordinator
        .record_workflow_operator_decision(
            actor.clone(),
            SIMULATOR_WORKFLOW_ID,
            "signoff-approved",
            "simulator",
            Some("all simulator evidence mapped".to_string()),
            None,
        )
        .await
        .map_err(|err| err.to_string())?;
    coordinator
        .complete_workflow_with_closeout_policy(
            actor.clone(),
            SIMULATOR_WORKFLOW_ID,
            "outcome.finished",
            "deterministic simulator completed with mapped evidence",
            "simulator",
            WorkflowSignoffPolicy::simulator_default(),
            closeout_policy.clone(),
        )
        .await
        .map_err(|err| err.to_string())?;
    coordinator
        .complete_workflow(
            actor,
            SIMULATOR_WORKFLOW_ID,
            "outcome.failed",
            "late completion after terminal outcome must not mutate replay projection",
            "late-worker",
        )
        .await
        .map_err(|err| err.to_string())?;
    coordinator
        .stop_run()
        .await
        .map_err(|err| err.to_string())?;

    let events = load_events(&run)?;
    let projection = project_workflows(events.iter().map(|event| &event.payload));
    let persistent_tasks = project_persistent_tasks(&events);
    let dossier = build_run_dossier_with_tasks_and_closeout_policy(
        &projection,
        &persistent_tasks,
        &WorkflowSignoffPolicy::simulator_default(),
        &closeout_policy,
    );
    let replay_projection =
        project_workflows(load_events(&run)?.iter().map(|event| &event.payload));
    let closeout_replay_equivalent = replay_projection == projection;
    let workflow = projection
        .workflows
        .get(SIMULATOR_WORKFLOW_ID)
        .ok_or_else(|| "simulator workflow missing from replay projection".to_string())?;
    if workflow.status != "outcome.finished" || !workflow.terminal {
        return Err(format!(
            "simulator workflow did not finish from replay: status={} terminal={}",
            workflow.status, workflow.terminal
        ));
    }
    let late_completion_ignored = workflow.owner == "simulator";

    Ok(WorkflowSimulationReport {
        run_id: run.run_id,
        run_dir: run.run_dir,
        workflow_id: SIMULATOR_WORKFLOW_ID.to_string(),
        provider_output,
        missing_evidence_blocked_completion,
        active_continuation_blocked_completion,
        permission_denied_after_start,
        owner_conflict_denied,
        pending_task_blocked_completion,
        question_blocked_completion,
        missing_dossier_blocked_completion,
        late_completion_ignored,
        closeout_replay_equivalent,
        replay_event_count: events.len(),
        projection,
        dossier,
    })
}

fn closeout_policy_without_evidence_or_dossier() -> WorkflowCloseoutPolicy {
    WorkflowCloseoutPolicy::from_config(
        WORKFLOW_CLOSEOUT_DEFAULT_POLICY_ID,
        WorkflowCloseoutPolicyConfig {
            require_evidence: false,
            require_dossier: false,
            ..WorkflowCloseoutPolicyConfig::default()
        },
    )
    .expect("test closeout policy")
}

fn closeout_policy_requiring_dossier_artifact() -> WorkflowCloseoutPolicy {
    WorkflowCloseoutPolicy::from_config(
        WORKFLOW_CLOSEOUT_DEFAULT_POLICY_ID,
        WorkflowCloseoutPolicyConfig {
            require_export_artifact: true,
            ..WorkflowCloseoutPolicyConfig::default()
        },
    )
    .expect("test closeout policy")
}

async fn start_simulator_run(
    session_dir: &Path,
    workspace: &Path,
) -> Result<(CoordinatorHandle, RunInfo), String> {
    let mut config = CoordinatorConfig::new(session_dir.to_path_buf());
    config.run_id_override = Some(SIMULATOR_RUN_ID.to_string());
    config.deterministic_store = true;
    config.session_mode_source = Some(SessionModeSource::ScenarioFixture);
    config.permission_policy = PermissionPolicy::new(
        PermissionMode::Allow,
        PermissionMode::Ask,
        PermissionMode::Allow,
    );
    config.tool_registry = Arc::new(coordinator_registry(ShellAllowlist {
        executables: vec!["true".to_string()],
        cwd_roots: Vec::new(),
    }));

    let clock: Arc<dyn Clock + Send + Sync> = Arc::new(FakeClock::new());
    let coordinator = spawn_coordinator(config, clock, Arc::new(DefaultRedactor::default()));
    let run = coordinator
        .start_run("deterministic workflow simulator", workspace)
        .await
        .map_err(|err| err.to_string())?;
    Ok((coordinator, run))
}

async fn scripted_provider_output() -> Result<String, String> {
    let request = CompletionRequest {
        provider_id: Some("mock".to_string()),
        model_id: "model-workflow-simulator".to_string(),
        messages: vec![CompletionMessage {
            role: MessageRole::User,
            content: "plan deterministic workflow demonstrator".to_string(),
            name: None,
            tool_call_id: None,
            assistant_tool_calls: None,
        }],
        temperature: Some(0.0),
        max_tokens: None,
        variant: None,
        reasoning_effort: None,
        text_verbosity: None,
        reasoning_summary: None,
        tools: None,
        tool_choice: None,
        stream: true,
    };
    let provider = MockProvider::new(BTreeMap::from([(
        request_digest(&request),
        vec![
            ProviderStreamEvent::Start,
            ProviderStreamEvent::TextDelta("plan: direct simulator path".to_string()),
            ProviderStreamEvent::Done {
                usage: CompletionUsage {
                    prompt_tokens: 5,
                    completion_tokens: 4,
                    total_tokens: 9,
                },
            },
        ],
    )]));
    let events = provider
        .stream_completion(request)
        .await
        .collect::<Vec<_>>()
        .await;
    let mut output = String::new();
    for event in events {
        if let ProviderStreamEvent::TextDelta(delta) = event {
            output.push_str(&delta);
        }
    }
    Ok(output)
}

async fn run_permission_gated_noop(
    coordinator: &CoordinatorHandle,
    actor: EventActor,
    permission_id: &str,
    allow: bool,
) -> Result<String, String> {
    let tool_call = {
        let coordinator = coordinator.clone();
        tokio::spawn(async move {
            coordinator
                .execute_agent_tool_call(
                    actor,
                    Some("workflow-simulator".to_string()),
                    "bash",
                    json!({
                        "command": "true",
                        "description": "deterministic no-op side-effect adapter"
                    }),
                )
                .await
        })
    };

    resolve_pending_permission(coordinator, permission_id, allow).await?;
    match tool_call.await.map_err(|err| err.to_string())? {
        Ok(result) => Ok(result.display_text),
        Err(err) => Err(err),
    }
}

async fn resolve_pending_permission(
    coordinator: &CoordinatorHandle,
    permission_id: &str,
    allow: bool,
) -> Result<(), String> {
    let decision = if allow {
        PermissionDecision::Allow
    } else {
        PermissionDecision::Deny
    };
    let reason = if allow {
        "simulator permits deterministic no-op"
    } else {
        "simulator negative case denies after workflow start"
    };
    let mut last_err = None;
    for _ in 0..50 {
        match coordinator
            .resolve_permission(permission_id, decision, Some(reason.to_string()))
            .await
        {
            Ok(()) => return Ok(()),
            Err(err) => {
                last_err = Some(err.to_string());
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        }
    }
    Err(format!(
        "permission {permission_id} was not resolvable: {}",
        last_err.unwrap_or_else(|| "no coordinator error".to_string())
    ))
}

fn project_current_workflows(run: &RunInfo) -> Result<WorkflowProjection, String> {
    Ok(project_workflows(
        load_events(run)?.iter().map(|event| &event.payload),
    ))
}

fn load_events(run: &RunInfo) -> Result<Vec<EventEnvelopeV1>, String> {
    let body = fs::read_to_string(&run.events_path)
        .map_err(|err| format!("failed to read {}: {err}", run.events_path.display()))?;
    body.lines()
        .map(|line| serde_json::from_str::<EventEnvelopeV1>(line).map_err(|err| err.to_string()))
        .collect()
}

pub fn simulator_negative_evidence(report: &WorkflowSimulationReport) -> bool {
    report.missing_evidence_blocked_completion
        && report.active_continuation_blocked_completion
        && report.permission_denied_after_start
        && report.owner_conflict_denied
        && report.pending_task_blocked_completion
        && report.question_blocked_completion
        && report.missing_dossier_blocked_completion
        && report.late_completion_ignored
        && report.closeout_replay_equivalent
        && has_denied_transition(report, "transition.owner_conflict_denied")
        && has_denied_transition(report, "transition.evidence_gated_completion")
        && has_denied_transition(report, "transition.workflow_tasks_incomplete")
        && has_denied_transition(report, WORKFLOW_CLOSEOUT_DEFAULT_POLICY_ID)
        && report.dossier.denied_transitions == report.projection.denied_transitions
}

fn has_denied_transition(report: &WorkflowSimulationReport, policy_id: &str) -> bool {
    report
        .projection
        .denied_transitions
        .iter()
        .any(|transition| transition.policy_id == policy_id)
}

pub fn simulator_replay_evidence(report: &WorkflowSimulationReport) -> bool {
    let has_denied_permission = report
        .projection
        .workflows
        .contains_key(SIMULATOR_WORKFLOW_ID)
        && report
            .dossier
            .workflows
            .iter()
            .any(|workflow| workflow.signoff.allowed && workflow.status == "outcome.finished");
    has_denied_permission && report.replay_event_count > 0
}

pub fn event_log_contains_permission_denial_and_success(
    events: &[EventEnvelopeV1],
) -> (bool, bool) {
    let denied = events.iter().any(|event| {
        matches!(
            &event.payload,
            EventV1::PermissionResolved(resolved)
                if resolved.decision == harness_core::event::PermissionDecision::Deny
        )
    });
    let succeeded = events.iter().any(|event| {
        matches!(
            &event.payload,
            EventV1::ToolCallFinished(finished) if finished.status == ToolCallStatus::Succeeded
        )
    });
    (denied, succeeded)
}

fn supervisor_actor() -> EventActor {
    EventActor::new(
        ActorKind::Supervisor,
        Some("workflow-simulator".to_string()),
    )
}

#[cfg(test)]
mod tests {
    use super::{
        event_log_contains_permission_denial_and_success, load_events,
        run_deterministic_workflow_simulator, simulator_negative_evidence,
        simulator_replay_evidence, SIMULATOR_WORKFLOW_ID,
    };

    #[tokio::test]
    async fn deterministic_workflow_simulator_runs_through_signoff_and_replay() {
        let temp = tempfile::tempdir().expect("tempdir");
        let report = run_deterministic_workflow_simulator(temp.path())
            .await
            .expect("workflow simulator should complete");

        assert_eq!(report.workflow_id, SIMULATOR_WORKFLOW_ID);
        assert!(report.provider_output.contains("direct simulator path"));
        assert!(simulator_negative_evidence(&report));
        assert!(simulator_replay_evidence(&report));
        assert!(report.active_continuation_blocked_completion);
        assert!(report.pending_task_blocked_completion);
        assert!(report.question_blocked_completion);
        assert!(report.missing_dossier_blocked_completion);
        assert!(report.closeout_replay_equivalent);
        let workflow = &report.projection.workflows[SIMULATOR_WORKFLOW_ID];
        assert_eq!(workflow.status, "outcome.finished");
        assert!(workflow.terminal);
        assert_eq!(workflow.owner, "simulator");
        assert!(report.late_completion_ignored);
        let dossier_workflow = report
            .dossier
            .workflows
            .iter()
            .find(|workflow| workflow.workflow_id == SIMULATOR_WORKFLOW_ID)
            .expect("simulator workflow dossier entry");
        assert!(dossier_workflow.quality_gate.passed);
        assert_eq!(dossier_workflow.continuations.len(), 1);
        assert_eq!(dossier_workflow.continuations[0].status, "stopped");
        assert!(workflow
            .evidence_categories
            .contains(super::SIMULATED_TOOL_EVIDENCE_CATEGORY));
        let (denied, succeeded) =
            event_log_contains_permission_denial_and_success(&load_events_from_report(&report));
        assert!(denied);
        assert!(succeeded);
    }

    fn load_events_from_report(
        report: &super::WorkflowSimulationReport,
    ) -> Vec<harness_core::event::EventEnvelopeV1> {
        load_events(&harness_core::coord::RunInfo {
            run_id: report.run_id.clone(),
            run_name: "deterministic workflow simulator".to_string(),
            workspace_root: report.run_dir.join("../../workspace"),
            run_dir: report.run_dir.clone(),
            artifacts_dir: report.run_dir.join("artifacts"),
            events_path: report.run_dir.join("events.jsonl"),
        })
        .expect("load simulator events")
    }
}
