use std::path::PathBuf;

use harness_core::event::EventEnvelopeV1;
use harness_core::session_lineage::{
    materialize_child_session, ChildSessionMaterializationRequest,
    ChildSessionMaterializationResult, ChildSessionMaterializationSourceKind, StableSessionPrefix,
};
use harness_tui::{LiveUpdate, OperatorNoticeLevel};

pub(super) fn materialize_tui_lineage_child(
    operation: &'static str,
    source_run_dir: PathBuf,
    events: Vec<EventEnvelopeV1>,
    stable_prefix: StableSessionPrefix,
) -> LiveUpdate {
    let result = materialize_child_session(ChildSessionMaterializationRequest {
        source_run_dir: &source_run_dir,
        events: &events,
        stable_prefix: &stable_prefix,
        source_kind: ChildSessionMaterializationSourceKind::TuiStableInMemorySnapshot,
    });

    match result {
        Ok(result) => LiveUpdate::OperatorNotice {
            message: tui_lineage_success_message(operation, &result),
            level: OperatorNoticeLevel::Info,
        },
        Err(err) => LiveUpdate::OperatorNotice {
            message: format!("Harness session {operation} blocked: {err}"),
            level: OperatorNoticeLevel::Error,
        },
    }
}

pub(super) fn materialize_tui_fork_child(
    source_run_dir: PathBuf,
    events: Vec<EventEnvelopeV1>,
    stable_prefix: StableSessionPrefix,
    prompt_text: String,
) -> LiveUpdate {
    let result = materialize_child_session(ChildSessionMaterializationRequest {
        source_run_dir: &source_run_dir,
        events: &events,
        stable_prefix: &stable_prefix,
        source_kind: ChildSessionMaterializationSourceKind::TuiStableInMemorySnapshot,
    });

    match result {
        Ok(result) => LiveUpdate::ContinueSession {
            run_id: result.child_run_id,
            run_dir: result.child_run_dir,
            prompt_draft: prompt_text,
        },
        Err(err) => LiveUpdate::OperatorNotice {
            message: format!("Harness session fork blocked: {err}"),
            level: OperatorNoticeLevel::Error,
        },
    }
}

fn tui_lineage_success_message(
    operation: &str,
    result: &ChildSessionMaterializationResult,
) -> String {
    format!(
        "Harness session {operation} created {} from seq {} ({} events, {} artifacts)",
        result.child_run_id, result.source_cutoff_seq, result.event_count, result.artifact_count
    )
}
