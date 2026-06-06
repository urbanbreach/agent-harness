use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::Path;

use harness_core::event::{EventEnvelopeV1, EventV1};
use harness_core::proj::{project_resume_plan, ChildSessionTerminalState};
use serde_json::Value;

use crate::cli_labels::provider_model_label;

use super::{sanitize_human_text, ReplayArtifactSummary, ReplayChildSessionSummary};

pub(super) fn summarize_recovery_counts(
    run_dir: &Path,
    events: &[EventEnvelopeV1],
    fallback_run_id: &str,
) -> (usize, usize) {
    let (artifacts, child_sessions) = summarize_recovery_story(run_dir, events, fallback_run_id);
    (artifacts.len(), child_sessions.len())
}

pub(super) fn summarize_recovery_story(
    run_dir: &Path,
    events: &[EventEnvelopeV1],
    fallback_run_id: &str,
) -> (Vec<ReplayArtifactSummary>, Vec<ReplayChildSessionSummary>) {
    let resume_plan = project_resume_plan(events.iter(), fallback_run_id).ok();

    let mut artifacts = artifact_inventory(run_dir, events, resume_plan.as_ref());
    artifacts.sort_by(|left, right| {
        left.path
            .cmp(&right.path)
            .then_with(|| left.tool_call_id.cmp(&right.tool_call_id))
            .then_with(|| left.digest.cmp(&right.digest))
    });

    let mut child_artifact_paths: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    let mut child_tool_call_ids: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    let mut child_routes: BTreeMap<String, Value> = BTreeMap::new();
    if let Some(resume_plan) = resume_plan.as_ref() {
        for (tool_call_id, tool_call) in &resume_plan.tool_calls {
            let Some(metadata) = tool_call.metadata.as_ref() else {
                continue;
            };
            let Some(lineage) = metadata.lineage.as_ref() else {
                continue;
            };
            let Some(child_session_id) = lineage.child_session_id.as_ref() else {
                continue;
            };

            child_tool_call_ids
                .entry(child_session_id.clone())
                .or_default()
                .insert(tool_call_id.clone());
            if let Some(route) = tool_call.output_json.as_ref().and_then(|output_json| {
                task_output_route_for_child(
                    output_json,
                    child_session_id,
                    lineage.child_request_id.as_deref(),
                )
            }) {
                child_routes
                    .entry(child_session_id.clone())
                    .or_insert(route);
            }
            for artifact_ref in &metadata.artifact_refs {
                child_artifact_paths
                    .entry(child_session_id.clone())
                    .or_default()
                    .insert(artifact_ref.path.clone());
            }
        }
    }

    let mut child_sessions = resume_plan
        .as_ref()
        .map(|resume_plan| {
            resume_plan
                .child_sessions
                .iter()
                .filter(|(_, child)| {
                    child.parent_session_id.is_some()
                        || child.parent_tool_call_id.is_some()
                        || child.parent_task_id.is_some()
                        || child.parent_request_id.is_some()
                })
                .map(|(child_session_id, child)| ReplayChildSessionSummary {
                    child_session_id: child_session_id.clone(),
                    profile: child.profile.clone(),
                    provider_model: provider_model_label(
                        child.provider_id.as_deref(),
                        child.model_id.as_deref(),
                    ),
                    latest_child_request_id: child.latest_child_request_id.clone(),
                    route: child_routes.remove(child_session_id),
                    parent_session_id: child.parent_session_id.clone(),
                    parent_tool_call_id: child.parent_tool_call_id.clone(),
                    parent_task_id: child.parent_task_id.clone(),
                    parent_request_id: child.parent_request_id.clone(),
                    terminal_state: child.terminal_state,
                    terminal_reason: child.terminal_reason.clone(),
                    notification_status: child
                        .background_notification
                        .as_ref()
                        .map(|notification| notification.status.as_str().to_string()),
                    notification_summary: child
                        .background_notification
                        .as_ref()
                        .map(|notification| notification.summary.clone()),
                    notification_terminal_event_id: child
                        .background_notification
                        .as_ref()
                        .map(|notification| notification.terminal_event_id.clone()),
                    notification_terminal_task_id: child
                        .background_notification
                        .as_ref()
                        .map(|notification| notification.terminal_task_id.clone()),
                    notification_delivered_turn_request_id: child
                        .background_notification
                        .as_ref()
                        .and_then(|notification| notification.delivered_turn_request_id.clone()),
                    elapsed_ms: child.timing.as_ref().and_then(|timing| timing.elapsed_ms),
                    related_tool_call_ids: child_tool_call_ids
                        .remove(child_session_id)
                        .unwrap_or_default()
                        .into_iter()
                        .collect(),
                    artifact_paths: child_artifact_paths
                        .remove(child_session_id)
                        .unwrap_or_default()
                        .into_iter()
                        .collect(),
                    next_actions: child_session_next_actions(
                        child_session_id,
                        child.latest_child_request_id.as_deref(),
                        child.terminal_state,
                    ),
                })
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    child_sessions.sort_by(|left, right| left.child_session_id.cmp(&right.child_session_id));

    (artifacts, child_sessions)
}

fn task_output_route_for_child(
    output_json: &Value,
    child_session_id: &str,
    child_request_id: Option<&str>,
) -> Option<Value> {
    let route = output_json.get("route")?.clone();
    let output_child_session_id = output_string_field(output_json, "child_session_id")
        .or_else(|| output_string_field(output_json, "session_id"))
        .or_else(|| output_string_field(output_json, "task_id"));
    let output_child_request_id = output_string_field(output_json, "child_request_id")
        .or_else(|| output_string_field(output_json, "request_id"));

    let session_matches = output_child_session_id
        .as_deref()
        .is_some_and(|value| value == child_session_id);
    let request_matches = child_request_id.is_some_and(|request_id| {
        output_child_request_id
            .as_deref()
            .is_some_and(|value| value == request_id)
    });
    let output_has_no_child_selector =
        output_child_session_id.is_none() && output_child_request_id.is_none();
    (session_matches || request_matches || output_has_no_child_selector).then_some(route)
}

fn output_string_field(output_json: &Value, key: &str) -> Option<String> {
    output_json
        .get(key)
        .and_then(Value::as_str)
        .map(str::to_string)
}

fn child_session_next_actions(
    child_session_id: &str,
    child_request_id: Option<&str>,
    terminal_state: Option<ChildSessionTerminalState>,
) -> Vec<String> {
    let mut actions = Vec::new();
    if let Some(request_id) = child_request_id {
        actions.push(format!(
            "background_output(request_id=\"{}\", block=false)",
            sanitize_human_text(request_id)
        ));
        if terminal_state.is_none() {
            actions.push(format!(
                "background_output(request_id=\"{}\", block=true)",
                sanitize_human_text(request_id)
            ));
            actions.push(format!(
                "background_cancel(request_id=\"{}\", reason=\"cancelled by parent request\")",
                sanitize_human_text(request_id)
            ));
        }
    }
    actions.push(format!(
        "task(session_id=\"{}\", run_in_background=false, load_skills=[], prompt=\"Continue this child task with additional instructions.\")",
        sanitize_human_text(child_session_id)
    ));
    actions
}

#[derive(Debug, Clone, Default)]
struct ToolCallDiscovery {
    tool_id: Option<String>,
    effective_tool_id: Option<String>,
    canonical_tool_id: Option<String>,
    alias_source_tool_id: Option<String>,
    child_session_id: Option<String>,
}

fn artifact_inventory(
    run_dir: &Path,
    events: &[EventEnvelopeV1],
    resume_plan: Option<&harness_core::proj::ResumePlan>,
) -> Vec<ReplayArtifactSummary> {
    let mut by_key: BTreeMap<(String, Option<String>, Option<String>), ReplayArtifactSummary> =
        BTreeMap::new();
    let tool_call_lookup = tool_call_discovery_lookup(resume_plan);

    for event in events {
        let EventV1::ArtifactWritten(payload) = &event.payload else {
            continue;
        };
        let discovery = payload
            .tool_call_id
            .as_ref()
            .and_then(|tool_call_id| tool_call_lookup.get(tool_call_id));
        let entry = by_key
            .entry((
                payload.path.clone(),
                payload.tool_call_id.clone(),
                Some(payload.digest.clone()),
            ))
            .or_insert_with(|| {
                artifact_summary(
                    payload.path.clone(),
                    Some(payload.digest.clone()),
                    Some(payload.bytes),
                    payload.tool_call_id.clone(),
                    run_dir.join(&payload.path).exists(),
                )
            });
        fill_missing_artifact_identity(
            entry,
            discovery,
            payload
                .tool_metadata
                .as_ref()
                .and_then(|metadata| metadata.canonical_tool_id.as_deref()),
            payload
                .tool_metadata
                .as_ref()
                .and_then(|metadata| metadata.alias_source_tool_id.as_deref()),
        );
        if entry.digest.is_none() {
            entry.digest = Some(payload.digest.clone());
        }
        if entry.bytes.is_none() {
            entry.bytes = Some(payload.bytes);
        }
        if entry.tool_call_id.is_none() {
            entry.tool_call_id = payload.tool_call_id.clone();
        }
        entry.present_on_disk |= run_dir.join(&payload.path).exists();
    }

    if let Some(resume_plan) = resume_plan {
        for (tool_call_id, tool_call) in &resume_plan.tool_calls {
            let Some(metadata) = tool_call.metadata.as_ref() else {
                continue;
            };
            let discovery = tool_call_lookup.get(tool_call_id);
            for artifact_ref in &metadata.artifact_refs {
                let entry = by_key
                    .entry((
                        artifact_ref.path.clone(),
                        Some(tool_call_id.clone()),
                        artifact_ref.digest.clone(),
                    ))
                    .or_insert_with(|| {
                        artifact_summary(
                            artifact_ref.path.clone(),
                            artifact_ref.digest.clone(),
                            None,
                            Some(tool_call_id.clone()),
                            run_dir.join(&artifact_ref.path).exists(),
                        )
                    });
                fill_missing_artifact_identity(
                    entry,
                    discovery,
                    metadata.canonical_tool_id.as_deref(),
                    metadata.alias_source_tool_id.as_deref(),
                );
                if entry.digest.is_none() {
                    entry.digest = artifact_ref.digest.clone();
                }
                if entry.tool_call_id.is_none() {
                    entry.tool_call_id = Some(tool_call_id.clone());
                }
                entry.present_on_disk |= run_dir.join(&artifact_ref.path).exists();
            }
        }
    }

    let mut on_disk = BTreeMap::new();
    collect_artifact_files(&run_dir.join("artifacts"), run_dir, &mut on_disk);
    for (path, bytes) in on_disk {
        let mut matched = false;
        for artifact in by_key.values_mut() {
            if artifact.path == path {
                matched = true;
                artifact.present_on_disk = true;
                if artifact.bytes.is_none() {
                    artifact.bytes = Some(bytes);
                }
            }
        }
        if !matched {
            by_key.insert(
                (path.clone(), None, None),
                artifact_summary(path, None, Some(bytes), None, true),
            );
        }
    }

    by_key.into_values().collect()
}

fn artifact_summary(
    path: String,
    digest: Option<String>,
    bytes: Option<u64>,
    tool_call_id: Option<String>,
    present_on_disk: bool,
) -> ReplayArtifactSummary {
    ReplayArtifactSummary {
        path,
        digest,
        bytes,
        tool_call_id,
        tool_id: None,
        effective_tool_id: None,
        canonical_tool_id: None,
        alias_source_tool_id: None,
        child_session_id: None,
        present_on_disk,
    }
}

fn fill_missing_artifact_identity(
    entry: &mut ReplayArtifactSummary,
    discovery: Option<&ToolCallDiscovery>,
    canonical_tool_id_fallback: Option<&str>,
    alias_source_tool_id_fallback: Option<&str>,
) {
    if entry.tool_id.is_none() {
        entry.tool_id = discovery.and_then(|it| it.tool_id.clone());
    }
    if entry.effective_tool_id.is_none() {
        entry.effective_tool_id = discovery.and_then(|it| it.effective_tool_id.clone());
    }
    if entry.canonical_tool_id.is_none() {
        entry.canonical_tool_id = discovery
            .and_then(|it| it.canonical_tool_id.clone())
            .or_else(|| canonical_tool_id_fallback.map(str::to_string));
    }
    if entry.alias_source_tool_id.is_none() {
        entry.alias_source_tool_id = discovery
            .and_then(|it| it.alias_source_tool_id.clone())
            .or_else(|| alias_source_tool_id_fallback.map(str::to_string));
    }
    if entry.child_session_id.is_none() {
        entry.child_session_id = discovery.and_then(|it| it.child_session_id.clone());
    }
}

fn tool_call_discovery_lookup(
    resume_plan: Option<&harness_core::proj::ResumePlan>,
) -> BTreeMap<String, ToolCallDiscovery> {
    resume_plan
        .map(|resume_plan| {
            resume_plan
                .tool_calls
                .iter()
                .map(|(tool_call_id, snapshot)| {
                    let metadata = snapshot.metadata.as_ref();
                    (
                        tool_call_id.clone(),
                        ToolCallDiscovery {
                            tool_id: snapshot
                                .tool_id
                                .clone()
                                .or_else(|| {
                                    snapshot
                                        .resolved_tool_identity
                                        .as_ref()
                                        .and_then(|identity| identity.invoked_tool_id.clone())
                                })
                                .or_else(|| {
                                    snapshot
                                        .resolved_tool_identity
                                        .as_ref()
                                        .and_then(|identity| identity.effective_tool_id.clone())
                                })
                                .or_else(|| metadata.and_then(|it| it.canonical_tool_id.clone())),
                            effective_tool_id: snapshot
                                .resolved_tool_identity
                                .as_ref()
                                .and_then(|identity| identity.effective_tool_id.clone())
                                .or_else(|| {
                                    snapshot
                                        .resolved_tool_identity
                                        .as_ref()
                                        .and_then(|identity| identity.invoked_tool_id.clone())
                                })
                                .or_else(|| metadata.and_then(|it| it.canonical_tool_id.clone()))
                                .or_else(|| snapshot.tool_id.clone()),
                            canonical_tool_id: snapshot
                                .resolved_tool_identity
                                .as_ref()
                                .and_then(|identity| identity.canonical_tool_id.clone())
                                .or_else(|| metadata.and_then(|it| it.canonical_tool_id.clone())),
                            alias_source_tool_id: snapshot
                                .resolved_tool_identity
                                .as_ref()
                                .and_then(|identity| identity.alias_source_tool_id.clone())
                                .or_else(|| {
                                    metadata.and_then(|it| it.alias_source_tool_id.clone())
                                }),
                            child_session_id: metadata
                                .and_then(|it| it.lineage.as_ref())
                                .and_then(|lineage| lineage.child_session_id.clone()),
                        },
                    )
                })
                .collect()
        })
        .unwrap_or_default()
}

fn collect_artifact_files(
    artifacts_dir: &Path,
    run_dir: &Path,
    artifacts: &mut BTreeMap<String, u64>,
) {
    let Ok(read_dir) = fs::read_dir(artifacts_dir) else {
        return;
    };

    for entry in read_dir.filter_map(Result::ok) {
        let path = entry.path();
        if path.is_dir() {
            collect_artifact_files(&path, run_dir, artifacts);
            continue;
        }
        if !path.is_file() {
            continue;
        }

        let relative_path = path.strip_prefix(run_dir).unwrap_or(&path);
        let relative = relative_path.to_string_lossy().replace('\\', "/");
        let bytes = path
            .metadata()
            .ok()
            .map(|metadata| metadata.len())
            .unwrap_or(0);
        artifacts.entry(relative).or_insert(bytes);
    }
}
