use super::*;
use std::path::Path;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LifecycleSegmentStatus {
    #[default]
    Missing,
    Active,
    Finished,
    Failed,
}
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ResumePlan {
    pub run_id: String,
    pub latest_lifecycle_status: LifecycleSegmentStatus,
    pub max_seq: u64,
    pub known_agents: BTreeMap<String, String>,
    pub known_profiles: BTreeSet<String>,
    pub pending_permissions: BTreeSet<String>,
    pub tasks_in_flight: BTreeSet<String>,
    pub workspace_root: Option<String>,
    pub provider_model: Option<String>,
    pub is_resumable: bool,
    pub resume_disabled_reason: Option<String>,
}
impl ResumePlan {
    pub(super) fn readiness(&mut self) {
        let reason = match self.latest_lifecycle_status {
            LifecycleSegmentStatus::Missing => Some("session has no lifecycle record"),
            LifecycleSegmentStatus::Active => {
                Some("session lifecycle is still active; stop or recover it before continuing")
            }
            _ if self.known_agents.is_empty() => Some("session has no recorded agent"),
            _ if self
                .workspace_root
                .as_deref()
                .is_none_or(|p| p.trim().is_empty()) =>
            {
                Some("session has no workspace")
            }
            _ => None,
        };
        self.resume_disabled_reason = reason.map(str::to_owned);
        self.is_resumable = reason.is_none();
    }
    pub(super) fn metadata(&mut self, metadata: Option<&SessionCatalogMetadata>) {
        let Some(metadata) = metadata else {
            return;
        };
        if self.provider_model.is_none() {
            self.provider_model = metadata
                .recorded_runtime_context
                .as_ref()
                .map(|c| format!("{}/{}", c.provider, c.model))
                .or_else(|| {
                    metadata
                        .provider
                        .as_ref()
                        .zip(metadata.model.as_ref())
                        .map(|(p, m)| format!("{p}/{m}"))
                });
        }
        self.readiness();
        if metadata
            .run_id
            .as_ref()
            .is_some_and(|id| id != &self.run_id)
        {
            self.disable("session metadata belongs to a different run");
        } else if matches!(
            metadata.mode_source,
            Some(SessionModeSource::ReplayOnly | SessionModeSource::ScenarioFixture)
        ) {
            self.disable("session mode permits replay only");
        }
    }
    pub(super) fn disable(&mut self, reason: impl Into<String>) {
        self.is_resumable = false;
        self.resume_disabled_reason = Some(reason.into());
    }
}
pub fn project_resume_plan<'a>(
    events: impl IntoIterator<Item = &'a EventEnvelopeV1>,
    fallback_run_id: &str,
) -> Result<ResumePlan, ProjectionError> {
    let events = checked_history(events)?;
    Ok(from_history(&events, fallback_run_id))
}
pub(super) fn from_history(events: &[&EventEnvelopeV1], fallback: &str) -> ResumePlan {
    let mut plan = ResumePlan {
        run_id: events
            .first()
            .map_or_else(|| fallback.into(), |e| e.run_id.to_string()),
        max_seq: events.last().map_or(0, |e| e.seq),
        ..Default::default()
    };
    let mut summary = RunSummary::default();
    let mut primary = None;
    for event in crate::conversation_rewind::active_refs(events.iter().copied()) {
        summary.apply(event);
        match &event.payload {
            EventV1::RunStarted(e) => {
                plan.latest_lifecycle_status = LifecycleSegmentStatus::Active;
                plan.workspace_root = Some(e.workspace_root.clone());
            }
            EventV1::RunFinished(_) => {
                plan.latest_lifecycle_status = LifecycleSegmentStatus::Finished
            }
            EventV1::RunFailed(_) => plan.latest_lifecycle_status = LifecycleSegmentStatus::Failed,
            EventV1::AgentSpawned(e) => {
                plan.known_agents
                    .insert(e.agent_id.clone(), e.profile.clone());
                plan.known_profiles.insert(e.profile.clone());
                if e.parent_agent_id.is_none() {
                    primary.get_or_insert(e.agent_id.as_str());
                }
            }
            EventV1::ProviderRequestStarted(e)
                if event
                    .actor
                    .agent_id
                    .as_deref()
                    .is_none_or(|id| Some(id) == primary) =>
            {
                plan.provider_model = Some(format!("{}/{}", e.provider_id, e.model_id));
            }
            _ => {}
        }
    }
    plan.tasks_in_flight = summary.tasks_in_flight;
    plan.pending_permissions = summary.pending_permissions;
    plan.readiness();
    plan
}
/// Reads a fixed journal prefix. It never opens a writer or repairs a source history.
pub fn inspect_resume_plan(run_dir: &Path) -> ResumePlan {
    let fallback = run_dir
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or_default();
    let inspected = (|| -> Result<ResumePlan, Box<dyn std::error::Error>> {
        if !std::fs::symlink_metadata(run_dir)?.is_dir() {
            return Err("session directory cannot be a symlink".into());
        }
        let path = run_dir.join("events.jsonl");
        if !std::fs::symlink_metadata(&path)?.is_file() {
            return Err("session journal must be a regular file".into());
        }
        let events = crate::store::read_events(&path)?;
        let mut plan = project_resume_plan(&events, fallback)?;
        let metadata = read_run_metadata(run_dir)?
            .as_ref()
            .map(SessionCatalogMetadata::from);
        plan.metadata(metadata.as_ref());
        if plan.run_id != fallback {
            plan.disable("session directory and journal identity disagree");
        }
        // Readiness describes the recorded session; resume validates the live workspace.
        if let Err(error) = crate::store::existing_writer_lock(run_dir) {
            plan.disable(format!("session writer is unavailable: {error}"));
        }
        Ok(plan)
    })();
    inspected.unwrap_or_else(|error| {
        use crate::redact::Redactor;
        let mut plan = ResumePlan {
            run_id: fallback.into(),
            ..Default::default()
        };
        plan.disable(crate::redact::DefaultRedactor::default().redact_text(&error.to_string()));
        plan
    })
}
