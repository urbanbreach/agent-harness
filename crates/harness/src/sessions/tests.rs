// allow: SIZE_OK — sessions command unit tests
use super::*;
use crate::replay::{ReplaySummary, SessionInspectionEntry};
use crate::sessions::export::{
    write_redacted_export_output_with_redactor, SessionExportBundle, SessionExportSupport,
};
use crate::sessions::list::{
    collect_list_entries, render_human_session_table, render_json_session_list,
};
use harness_core::proj::{RunStatus, SessionCatalogEntry, SessionModeSource};
use harness_core::redact::{DefaultRedactor, Redactor};
use serde_json::json;

/// Typed value object grouping the fields needed to build a sample
/// `SessionInspectionEntry` in tests. Replaces the nine-parameter
/// `sample_entry` helper to satisfy the `too_many_arguments` lint.
struct SampleEntryInput<'a> {
    run_id: &'a str,
    sort_unix_ms: u128,
    status: Option<RunStatus>,
    profile_preset: Option<&'a str>,
    mode_source: SessionModeSource,
    is_resumable: bool,
    artifact_count: usize,
    child_session_count: usize,
    parent_session_id: Option<&'a str>,
}

fn sample_entry(input: &SampleEntryInput<'_>) -> SessionInspectionEntry {
    SessionInspectionEntry {
        run_dir: PathBuf::from(format!("/tmp/{}", input.run_id)),
        catalog: SessionCatalogEntry {
            run_id: input.run_id.to_string(),
            run_name: Some(format!("{}-name", input.run_id)),
            status: input.status,
            last_updated_at: Some(input.sort_unix_ms.to_string()),
            workspace_root: Some(format!("/workspaces/{}", input.run_id)),
            profile_preset: input.profile_preset.map(str::to_string),
            provider_model: Some("openai/gpt-5.4".to_string()),
            mode_source: input.mode_source,
            is_resumable: input.is_resumable,
            resume_disabled_reason: (!input.is_resumable).then(|| "resume blocked".to_string()),
            artifact_count: input.artifact_count,
            child_session_count: input.child_session_count,
            parent_session_id: input.parent_session_id.map(str::to_string),
        },
        sort_unix_ms: input.sort_unix_ms,
        artifact_count: input.artifact_count,
        child_session_count: input.child_session_count,
    }
}

fn minimal_export_bundle_with_secret(secret: &str) -> SessionExportBundle {
    let catalog = sample_entry(&SampleEntryInput {
        run_id: "run-secret-export",
        sort_unix_ms: 1,
        status: Some(RunStatus::Finished),
        profile_preset: Some("build"),
        mode_source: SessionModeSource::Prompt,
        is_resumable: false,
        artifact_count: 0,
        child_session_count: 0,
        parent_session_id: None,
    })
    .catalog;
    let mut bundle = SessionExportBundle {
        run_dir: PathBuf::from("/tmp/run-secret-export"),
        catalog,
        metadata: None,
        replay: ReplaySummary {
            run_id: "run-secret-export".into(),
            run_name: Some("secret-export".to_string()),
            session_path: PathBuf::from("/tmp/run-secret-export"),
            status: RunStatus::Finished,
            workspace_root: Some("/tmp/workspace".to_string()),
            mode_source: SessionModeSource::Prompt,
            is_resumable: false,
            resume_disabled_reason: None,
            artifact_count: 0,
            child_session_count: 0,
            parent_session_id: None,
            total_events: 0,
            counts_by_type: std::collections::BTreeMap::new(),
            pending_permissions: Vec::new(),
            tasks_in_flight: Vec::new(),
            last_error: None,
            artifacts: Vec::new(),
            child_sessions: Vec::new(),
        },
        support: SessionExportSupport {
            doctor_json: json!({}),
            config_summary: json!({}),
            provider_summary: json!({}),
            agent_catalog_summary: json!({}),
            skill_catalog_summary: json!({}),
            native_tool_catalog_summary: json!({}),
            session_tool_readiness: json!({}),
            credential_store_manifest: json!({}),
            route_metadata: Vec::new(),
            artifact_index: Vec::new(),
        },
        events: Vec::new(),
    };
    bundle.support.doctor_json = json!({ "leaked": secret });
    bundle
}

struct NoopRedactor;

impl Redactor for NoopRedactor {
    fn redact_text(&self, s: &str) -> String {
        s.to_string()
    }
}

#[test]
fn support_export_fails_closed_when_redaction_scan_finds_secret() {
    // arrange
    let export = minimal_export_bundle_with_secret("sk-proj-leaked_0123456789abcdef");
    let output_dir = tempfile::tempdir().unwrap();
    let output_path = output_dir.path().join("support.json");
    let mut stdout = Vec::new();
    let mut stderr = Vec::new();

    // act
    let code = write_redacted_export_output_with_redactor(
        &export,
        Some(output_path.clone()),
        &mut stdout,
        &mut stderr,
        &NoopRedactor,
        &DefaultRedactor::default(),
        &[],
    );

    // assert
    assert_eq!(code, 1);
    assert!(!output_path.exists());
    assert!(stdout.is_empty());
    assert!(
        String::from_utf8_lossy(&stderr).contains("redaction scanner found"),
        "stderr should explain fail-closed redaction: {}",
        String::from_utf8_lossy(&stderr)
    );
}

#[test]
fn support_export_fails_closed_when_env_credential_value_survives_redaction() {
    // arrange
    let export = minimal_export_bundle_with_secret("plain-env-secret-value");
    let output_dir = tempfile::tempdir().unwrap();
    let output_path = output_dir.path().join("support.json");
    let mut stdout = Vec::new();
    let mut stderr = Vec::new();
    let env_values = vec!["plain-env-secret-value".to_string()];

    // act
    let code = write_redacted_export_output_with_redactor(
        &export,
        Some(output_path.clone()),
        &mut stdout,
        &mut stderr,
        &NoopRedactor,
        &DefaultRedactor::default(),
        &env_values,
    );

    // assert
    assert_eq!(code, 1);
    assert!(!output_path.exists());
    assert!(stdout.is_empty());
    assert!(
        String::from_utf8_lossy(&stderr).contains("redaction scanner found"),
        "stderr should explain env credential fail-closed scan: {}",
        String::from_utf8_lossy(&stderr)
    );
}

#[test]
fn support_export_omits_provider_reasoning_delta_events() {
    // arrange
    let raw_reasoning = "raw hidden reasoning text must not leave local events";
    let mut export = minimal_export_bundle_with_secret("non-secret-placeholder");
    export.support.doctor_json = json!({});
    export.replay.total_events = 1;
    export
        .replay
        .counts_by_type
        .insert("provider_reasoning_delta".to_string(), 1);
    export.events.push(harness_core::event::EventEnvelopeV1 {
        schema_version: harness_core::event::SCHEMA_VERSION,
        event_id: "event-reasoning".to_string(),
        seq: 1,
        run_id: "run-secret-export".into(),
        mono_ms: 1,
        ts: None,
        actor: harness_core::event::EventActor::new(harness_core::event::ActorKind::System, None),
        correlation_id: None,
        causation_id: None,
        stream_key: None,
        payload: harness_core::event::EventV1::ProviderReasoningDelta(
            harness_core::event::ProviderReasoningDeltaEvent {
                request_id: "provider-request-1".into(),
                delta: raw_reasoning.to_string(),
            },
        ),
    });
    let mut stdout = Vec::new();
    let mut stderr = Vec::new();

    // act
    let code = write_redacted_export_output_with_redactor(
        &export,
        None,
        &mut stdout,
        &mut stderr,
        &NoopRedactor,
        &DefaultRedactor::default(),
        &[],
    );

    // assert
    assert_eq!(code, 0, "stderr: {}", String::from_utf8_lossy(&stderr));
    assert!(stderr.is_empty());
    let rendered = String::from_utf8(stdout).unwrap();
    assert!(!rendered.contains(raw_reasoning));
    assert!(!rendered.contains("provider_reasoning_delta"));
    let parsed: serde_json::Value = serde_json::from_str(&rendered).unwrap();
    assert_eq!(parsed["events"].as_array().map(Vec::len), Some(0));
    assert_eq!(parsed["replay"]["total_events"], 0);
    assert_eq!(parsed["replay"]["counts_by_type"], json!({}));
}

#[test]
fn collect_list_entries_applies_filters_and_hides_non_operator_modes() {
    let entries = vec![
        sample_entry(&SampleEntryInput {
            run_id: "run-running",
            sort_unix_ms: 30,
            status: Some(RunStatus::Running),
            profile_preset: Some("worker"),
            mode_source: SessionModeSource::InteractiveLive,
            is_resumable: true,
            artifact_count: 0,
            child_session_count: 0,
            parent_session_id: None,
        }),
        sample_entry(&SampleEntryInput {
            run_id: "run-finished",
            sort_unix_ms: 20,
            status: Some(RunStatus::Finished),
            profile_preset: Some("reviewer"),
            mode_source: SessionModeSource::Prompt,
            is_resumable: false,
            artifact_count: 0,
            child_session_count: 0,
            parent_session_id: None,
        }),
        sample_entry(&SampleEntryInput {
            run_id: "fixture-hidden",
            sort_unix_ms: 10,
            status: Some(RunStatus::Finished),
            profile_preset: Some("worker"),
            mode_source: SessionModeSource::ScenarioFixture,
            is_resumable: true,
            artifact_count: 0,
            child_session_count: 0,
            parent_session_id: None,
        }),
    ];

    let command = SessionsListCommand {
        status: Some(SessionStatusFilter::Running),
        profile: Some("worker".to_string()),
        resumable: Some(true),
        ..SessionsListCommand::default()
    };

    let filtered = collect_list_entries(entries, &command);
    assert_eq!(filtered.len(), 1);
    assert_eq!(filtered[0].catalog.run_id, "run-running");
}

#[test]
fn collect_list_entries_supports_machine_sorting() {
    let entries = vec![
        sample_entry(&SampleEntryInput {
            run_id: "run-b",
            sort_unix_ms: 20,
            status: Some(RunStatus::Finished),
            profile_preset: Some("worker"),
            mode_source: SessionModeSource::InteractiveLive,
            is_resumable: true,
            artifact_count: 0,
            child_session_count: 0,
            parent_session_id: None,
        }),
        sample_entry(&SampleEntryInput {
            run_id: "run-c",
            sort_unix_ms: 10,
            status: Some(RunStatus::Finished),
            profile_preset: Some("worker"),
            mode_source: SessionModeSource::InteractiveLive,
            is_resumable: true,
            artifact_count: 0,
            child_session_count: 0,
            parent_session_id: None,
        }),
        sample_entry(&SampleEntryInput {
            run_id: "run-a",
            sort_unix_ms: 30,
            status: Some(RunStatus::Finished),
            profile_preset: Some("worker"),
            mode_source: SessionModeSource::InteractiveLive,
            is_resumable: true,
            artifact_count: 0,
            child_session_count: 0,
            parent_session_id: None,
        }),
    ];

    let command = SessionsListCommand {
        sort: SessionListSort::RunIdAsc,
        ..SessionsListCommand::default()
    };

    let filtered = collect_list_entries(entries, &command);
    let run_ids = filtered
        .iter()
        .map(|entry| entry.catalog.run_id.as_str())
        .collect::<Vec<_>>();

    assert_eq!(run_ids, vec!["run-a", "run-b", "run-c"]);
}

#[test]
fn render_json_session_list_emits_machine_readable_fields() {
    let entries = vec![sample_entry(&SampleEntryInput {
        run_id: "run-json",
        sort_unix_ms: 42,
        status: Some(RunStatus::Running),
        profile_preset: Some("worker"),
        mode_source: SessionModeSource::InteractiveMock,
        is_resumable: true,
        artifact_count: 2,
        child_session_count: 1,
        parent_session_id: Some("run-parent"),
    })];

    let rendered = render_json_session_list(&entries).unwrap();
    let parsed: serde_json::Value = serde_json::from_str(&rendered).unwrap();

    assert_eq!(
        parsed,
        json!([
            {
                "run_dir": "/tmp/run-json",
                "run_id": "run-json",
                "run_name": "run-json-name",
                "status": "running",
                "last_updated_at": "42",
                "workspace_root": "/workspaces/run-json",
                "profile_preset": "worker",
                "provider_model": "openai/gpt-5.4",
                "mode_source": "interactive_mock",
                "is_resumable": true,
                "resume_disabled_reason": null,
                "artifact_count": 2,
                "child_session_count": 1,
                "parent_session_id": "run-parent"
            }
        ])
    );
}

#[test]
fn render_human_session_table_keeps_operator_facing_default() {
    let entries = vec![sample_entry(&SampleEntryInput {
        run_id: "run-human",
        sort_unix_ms: 42,
        status: None,
        profile_preset: None,
        mode_source: SessionModeSource::Unknown,
        is_resumable: false,
        artifact_count: 3,
        child_session_count: 2,
        parent_session_id: Some("run-parent"),
    })];

    let rendered = render_human_session_table(&entries);

    assert!(rendered.contains("run_id"));
    assert!(rendered.contains("artifacts"));
    assert!(rendered.contains("children"));
    assert!(rendered.contains("session_path"));
    assert!(rendered.contains("parent"));
    assert!(rendered.contains("run-human"));
    assert!(rendered.contains("<unavailable>"));
    assert!(rendered.contains("resume blocked"));
    assert!(rendered.contains("/tmp/run-human"));
    assert!(rendered.contains("run-parent"));
}

#[test]
fn render_human_session_table_surfaces_meaningful_session_title() {
    // arrange
    let mut entry = sample_entry(&SampleEntryInput {
        run_id: "run-title",
        sort_unix_ms: 42,
        status: Some(RunStatus::Finished),
        profile_preset: Some("build"),
        mode_source: SessionModeSource::InteractiveLive,
        is_resumable: true,
        artifact_count: 0,
        child_session_count: 0,
        parent_session_id: None,
    });
    entry.catalog.run_name = Some("map chat renderers".to_string());

    // act
    let rendered = render_human_session_table(&[entry]);

    // assert
    assert!(rendered.contains("run_name"));
    assert!(rendered.contains("map chat renderers"));
    assert!(
        !rendered.contains("<unavailable>"),
        "a titled session should not degrade to only run id/path: {rendered}"
    );
}
