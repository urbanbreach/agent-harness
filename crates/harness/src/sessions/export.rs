mod credentials;
mod readiness;
mod redaction;
mod route_metadata;

use std::path::PathBuf;

use harness_core::proj::{RunMetadata, SessionCatalogEntry};
use serde::Serialize;
use serde_json::Value;

use crate::cli_io::{load_events_from_run_dir, load_run_metadata};
use crate::replay::{summarize_session, ReplayArtifactSummary, ReplaySummary};
use crate::CliDeps;

use super::{ensure_session_dir_exists, resolve_session, session_dir, ExportSessionCommand};

pub(super) use redaction::write_redacted_export_output_with_redactor;

#[derive(Debug, Clone, Serialize)]
pub(super) struct SessionExportBundle {
    pub(super) run_dir: PathBuf,
    pub(super) catalog: SessionCatalogEntry,
    pub(super) metadata: Option<RunMetadata>,
    pub(super) replay: ReplaySummary,
    pub(super) support: SessionExportSupport,
    pub(super) events: Vec<harness_core::event::EventEnvelopeV1>,
}

#[derive(Debug, Clone, Serialize)]
pub(super) struct SessionExportSupport {
    pub(super) doctor_json: Value,
    pub(super) config_summary: Value,
    pub(super) provider_summary: Value,
    pub(super) agent_catalog_summary: Value,
    pub(super) skill_catalog_summary: Value,
    pub(super) native_tool_catalog_summary: Value,
    pub(super) session_tool_readiness: Value,
    pub(super) credential_store_manifest: Value,
    pub(super) route_metadata: Vec<SessionExportRouteMetadata>,
    pub(super) artifact_index: Vec<ReplayArtifactSummary>,
}

#[derive(Debug, Clone)]
struct SessionExportReadiness {
    doctor_json: Value,
    config_summary: Value,
    provider_summary: Value,
    agent_catalog_summary: Value,
    skill_catalog_summary: Value,
    native_tool_catalog_summary: Value,
    session_tool_readiness: Value,
    credential_store_manifest: Value,
    credential_values: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub(super) struct SessionExportRouteMetadata {
    source: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    tool_call_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    child_session_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    child_request_id: Option<String>,
    route: Value,
}

pub(super) fn export_session(
    command: ExportSessionCommand,
    config_path: Option<PathBuf>,
    global_session_dir: Option<PathBuf>,
    stdout: &mut dyn std::io::Write,
    stderr: &mut dyn std::io::Write,
    deps: &CliDeps,
) -> i32 {
    let session_dir_override = global_session_dir.clone();
    let session_dir = session_dir(global_session_dir);
    if let Err(code) = ensure_session_dir_exists(&session_dir, stderr) {
        return code;
    }

    let session = match resolve_session(&session_dir, &command.session) {
        Ok(session) => session,
        Err(err) => {
            let _ = writeln!(stderr, "{err}");
            return 1;
        }
    };

    let events = match load_events_from_run_dir(&session.run_dir) {
        Ok(events) => events,
        Err(err) => {
            let _ = writeln!(
                stderr,
                "failed to export session {}: {err}",
                session.catalog.run_id
            );
            return 1;
        }
    };
    let replay = match summarize_session(&session.run_dir) {
        Ok(summary) => summary,
        Err(err) => {
            let _ = writeln!(
                stderr,
                "failed to export session {}: {err}",
                session.catalog.run_id
            );
            return 1;
        }
    };

    let run_dir = session.run_dir;
    let metadata = load_run_metadata(&run_dir);
    let session_workspace_root = replay.workspace_root.as_deref().map(PathBuf::from);
    let readiness = readiness::session_export_readiness(
        config_path.as_deref(),
        session_dir_override,
        session_workspace_root,
        deps,
    );
    let mut credential_values = deps.credential_env_values();
    credential_values.extend(readiness.credential_values.clone());
    let credential_values = credentials::dedupe_credential_values(credential_values);
    let export = SessionExportBundle {
        run_dir,
        support: session_export_support(&events, &replay, &session.catalog, readiness),
        catalog: session.catalog,
        metadata,
        replay,
        events,
    };

    redaction::write_redacted_export_output(
        &export,
        command.output,
        stdout,
        stderr,
        &credential_values,
    )
}

fn session_export_support(
    events: &[harness_core::event::EventEnvelopeV1],
    replay: &ReplaySummary,
    catalog: &SessionCatalogEntry,
    readiness: SessionExportReadiness,
) -> SessionExportSupport {
    SessionExportSupport {
        doctor_json: readiness.doctor_json,
        config_summary: readiness.config_summary,
        provider_summary: readiness.provider_summary,
        agent_catalog_summary: readiness.agent_catalog_summary,
        skill_catalog_summary: readiness.skill_catalog_summary,
        native_tool_catalog_summary: readiness.native_tool_catalog_summary,
        session_tool_readiness: readiness.session_tool_readiness,
        credential_store_manifest: readiness.credential_store_manifest,
        route_metadata: route_metadata::session_export_route_metadata(events, replay, catalog),
        artifact_index: replay.artifacts.clone(),
    }
}
