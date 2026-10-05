use super::*;

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct SessionCatalogMetadata {
    pub run_id: Option<String>,
    pub run_name: Option<String>,
    pub workspace_root: Option<String>,
    pub profile_preset: Option<String>,
    pub provider: Option<String>,
    pub model: Option<String>,
    pub recorded_runtime_context: Option<RecordedRuntimeContext>,
    pub mode_source: Option<SessionModeSource>,
}
impl From<&RunMetadata> for SessionCatalogMetadata {
    fn from(metadata: &RunMetadata) -> Self {
        let context = metadata.recorded_runtime_context.as_ref();
        Self {
            run_id: Some(metadata.run_id.clone()),
            run_name: Some(metadata.run_name.clone()),
            workspace_root: Some(metadata.workspace_root.clone()),
            profile_preset: context.map(|c| c.profile.clone()),
            provider: context.map(|c| c.provider.clone()),
            model: context.map(|c| c.model.clone()),
            recorded_runtime_context: context.cloned(),
            mode_source: metadata.mode_source,
        }
    }
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SessionCatalogEntry {
    pub run_id: String,
    pub run_name: Option<String>,
    pub status: Option<RunStatus>,
    pub last_updated_at: Option<String>,
    pub workspace_root: Option<String>,
    pub profile_preset: Option<String>,
    pub provider_model: Option<String>,
    pub mode_source: SessionModeSource,
    pub is_resumable: bool,
    pub resume_disabled_reason: Option<String>,
    pub artifact_count: usize,
    pub child_session_count: usize,
    pub parent_session_id: Option<String>,
}
pub fn project_session_catalog_entry<'a>(
    events: impl IntoIterator<Item = &'a EventEnvelopeV1>,
    fallback_run_id: &str,
    metadata: Option<&SessionCatalogMetadata>,
    last_updated_at: Option<String>,
    degraded_reason: Option<String>,
) -> Result<SessionCatalogEntry, ProjectionError> {
    let events = checked_history(events)?;
    let mut plan = super::resume::from_history(&events, fallback_run_id);
    plan.metadata(metadata);
    if let Some(reason) = degraded_reason {
        plan.disable(reason);
    }
    let mut name = metadata.and_then(|m| m.run_name.clone());
    let mut profile = metadata.and_then(|m| m.profile_preset.clone());
    let (mut artifacts, mut children) = (BTreeSet::new(), BTreeSet::new());
    let mut parent = None;
    for event in crate::conversation_rewind::active_refs(events.iter().copied()) {
        let lineage = match &event.payload {
            EventV1::RunStarted(e) => {
                name = Some(e.run_name.to_string());
                None
            }
            EventV1::SessionTitleUpdated(e) => {
                name = Some(e.title.clone());
                None
            }
            EventV1::AgentSpawned(e) => {
                if e.parent_agent_id.is_none() {
                    profile = Some(e.profile.clone());
                }
                None
            }
            EventV1::ArtifactWritten(e) => {
                artifacts.insert(e.path.as_str());
                None
            }
            EventV1::ToolCallRequested(e) => e.metadata.as_ref().and_then(|m| m.lineage.as_ref()),
            EventV1::ToolCallFinished(e) | EventV1::EvalCellFinished(e) => {
                if let Some(metadata) = &e.metadata {
                    artifacts.extend(metadata.artifact_refs.iter().map(|a| a.path.as_str()));
                }
                e.metadata.as_ref().and_then(|m| m.lineage.as_ref())
            }
            EventV1::TaskScheduled(e) => e.metadata.as_ref().and_then(|m| m.lineage.as_ref()),
            EventV1::TaskCompleted(e) => e.metadata.as_ref().and_then(|m| m.lineage.as_ref()),
            _ => None,
        };
        if let Some(lineage) = lineage {
            if let Some(child) = lineage
                .child_session_id
                .as_deref()
                .filter(|id| *id != plan.run_id)
            {
                children.insert(child);
            }
            if let Some(id) = lineage
                .non_empty_parent_session_id()
                .filter(|id| *id != plan.run_id)
                .filter(|_| {
                    lineage
                        .child_session_id
                        .as_deref()
                        .is_none_or(|child| child == plan.run_id)
                })
            {
                parent = Some(id.to_owned());
            }
        }
    }
    Ok(SessionCatalogEntry {
        run_id: plan.run_id,
        run_name: name,
        status: match plan.latest_lifecycle_status {
            LifecycleSegmentStatus::Missing => None,
            LifecycleSegmentStatus::Active => Some(RunStatus::Running),
            LifecycleSegmentStatus::Finished => Some(RunStatus::Finished),
            LifecycleSegmentStatus::Failed => Some(RunStatus::Failed),
        },
        last_updated_at: last_updated_at.or_else(|| events.iter().rev().find_map(|e| e.ts.clone())),
        workspace_root: plan.workspace_root,
        profile_preset: profile,
        provider_model: plan.provider_model,
        mode_source: metadata.and_then(|m| m.mode_source).unwrap_or_default(),
        is_resumable: plan.is_resumable,
        resume_disabled_reason: plan.resume_disabled_reason,
        artifact_count: artifacts.len(),
        child_session_count: children.len(),
        parent_session_id: parent,
    })
}
