use std::collections::BTreeSet;
use std::path::PathBuf;

use crate::event::{EventEnvelopeV1, EventV1, TaskLineageMetadata};
use crate::proj::{RunStatus, SessionCatalogEntry, SessionModeSource};

use super::SessionHistoryEntry;

#[derive(Debug, Clone)]
pub struct SessionHistoryRowReducer {
    pub(super) entry: SessionHistoryEntry,
    mode_source: SessionModeSource,
    artifact_paths: BTreeSet<String>,
    child_session_ids: BTreeSet<String>,
    provider: Option<String>,
    model: Option<String>,
}

impl SessionHistoryRowReducer {
    #[must_use]
    pub fn new(
        run_dir: PathBuf,
        run_id: String,
        run_name: String,
        workspace_root: String,
        mode_source: Option<SessionModeSource>,
    ) -> Self {
        let mode_source = mode_source.unwrap_or_else(|| infer_mode_source(&run_name, None));
        let reason = resume_disabled_reason(mode_source, false, false);
        let catalog = SessionCatalogEntry {
            run_id,
            run_name: Some(run_name),
            status: None,
            last_updated_at: None,
            workspace_root: Some(workspace_root),
            profile_preset: None,
            provider_model: None,
            mode_source,
            is_resumable: false,
            resume_disabled_reason: Some(reason),
            artifact_count: 0,
            child_session_count: 0,
            parent_session_id: None,
        };
        Self {
            entry: SessionHistoryEntry {
                run_dir,
                catalog,
                sort_unix_ms: 0,
                artifact_count: 0,
                child_session_count: 0,
            },
            mode_source,
            artifact_paths: BTreeSet::new(),
            child_session_ids: BTreeSet::new(),
            provider: None,
            model: None,
        }
    }

    #[must_use]
    pub fn from_history(
        run_dir: PathBuf,
        run_id: String,
        run_name: String,
        workspace_root: String,
        mode_source: Option<SessionModeSource>,
        events: &[EventEnvelopeV1],
    ) -> Self {
        let mut reducer = Self::new(run_dir, run_id, run_name, workspace_root, mode_source);
        for event in events {
            reducer.apply_event(event);
        }
        reducer
    }

    pub(super) fn apply_event(&mut self, event: &EventEnvelopeV1) {
        match &event.payload {
            EventV1::RunStarted(payload) => {
                self.entry.catalog.run_name = Some(payload.run_name.to_string());
                self.entry.catalog.workspace_root = Some(payload.workspace_root.clone());
                self.entry.catalog.status = Some(RunStatus::Running);
            }
            EventV1::SessionTitleUpdated(payload) => {
                self.entry.catalog.run_name = Some(payload.title.clone());
            }
            EventV1::RunFinished(_) => self.entry.catalog.status = Some(RunStatus::Finished),
            EventV1::RunFailed(_) => self.entry.catalog.status = Some(RunStatus::Failed),
            EventV1::AgentSpawned(payload) if self.entry.catalog.profile_preset.is_none() => {
                self.entry.catalog.profile_preset = Some(payload.profile.clone());
            }
            EventV1::ProviderRequestStarted(payload) => {
                self.provider = Some(payload.provider_id.clone());
                self.model = Some(payload.model_id.clone());
            }
            EventV1::ArtifactWritten(payload) => {
                self.artifact_paths.insert(payload.path.clone());
            }
            payload => self.record_lineage(payload),
        }
        if self.entry.catalog.parent_session_id.is_none() {
            self.entry.catalog.parent_session_id =
                event.lineage_parent_session_id().map(str::to_string);
        }
        self.refresh_catalog();
    }

    fn record_lineage(&mut self, payload: &EventV1) {
        let lineage = match payload {
            EventV1::TaskScheduled(value) => {
                value.metadata.as_ref().and_then(|m| m.lineage.as_ref())
            }
            EventV1::TaskCompleted(value) => {
                value.metadata.as_ref().and_then(|m| m.lineage.as_ref())
            }
            EventV1::ToolCallRequested(value) => {
                value.metadata.as_ref().and_then(|m| m.lineage.as_ref())
            }
            EventV1::ToolCallFinished(value) => {
                value.metadata.as_ref().and_then(|m| m.lineage.as_ref())
            }
            _ => None,
        };
        if let Some(child_session_id) = lineage.and_then(non_empty_child_session_id) {
            self.child_session_ids.insert(child_session_id.to_string());
        }
    }

    fn refresh_catalog(&mut self) {
        self.entry.catalog.provider_model = match (&self.provider, &self.model) {
            (Some(provider), Some(model)) => Some(format!("{provider}/{model}")),
            (Some(provider), None) => Some(format!("{provider}/<unavailable>")),
            (None, Some(model)) => Some(format!("<unavailable>/{model}")),
            (None, None) => None,
        };
        if self.mode_source == SessionModeSource::Unknown {
            self.mode_source = infer_mode_source(
                self.entry.catalog.run_name.as_deref().unwrap_or_default(),
                self.provider.as_deref(),
            );
            self.entry.catalog.mode_source = self.mode_source;
        }
        self.entry.artifact_count = self.artifact_paths.len();
        self.entry.child_session_count = self.child_session_ids.len();
        self.entry.catalog.artifact_count = self.entry.artifact_count;
        self.entry.catalog.child_session_count = self.entry.child_session_count;
        let reason = resume_disabled_reason(
            self.mode_source,
            self.entry.catalog.profile_preset.is_some(),
            self.entry.catalog.provider_model.is_some(),
        );
        self.entry.catalog.is_resumable = reason.is_empty();
        self.entry.catalog.resume_disabled_reason = (!reason.is_empty()).then_some(reason);
    }
}

fn infer_mode_source(run_name: &str, provider: Option<&str>) -> SessionModeSource {
    match run_name {
        "interactive" if provider == Some("mock") => SessionModeSource::InteractiveMock,
        "interactive" => SessionModeSource::InteractiveLive,
        "prompt" => SessionModeSource::Prompt,
        "replay" => SessionModeSource::ReplayOnly,
        "golden_path" | "golden_path_interactive" => SessionModeSource::ScenarioFixture,
        _ => SessionModeSource::Unknown,
    }
}

fn resume_disabled_reason(
    mode_source: SessionModeSource,
    has_profile: bool,
    has_provider_model: bool,
) -> String {
    match mode_source {
        SessionModeSource::ScenarioFixture => "scenario fixture runs are excluded from resume",
        SessionModeSource::ReplayOnly => "replay-only launches are not resumable",
        SessionModeSource::Prompt => "prompt runs are not resumable",
        SessionModeSource::Unknown => "session mode source is unavailable",
        SessionModeSource::InteractiveLive | SessionModeSource::InteractiveMock if !has_profile => {
            "profile preset is unavailable"
        }
        SessionModeSource::InteractiveLive | SessionModeSource::InteractiveMock
            if !has_provider_model =>
        {
            "provider/model is unavailable"
        }
        SessionModeSource::InteractiveLive | SessionModeSource::InteractiveMock => "",
    }
    .to_string()
}

fn non_empty_child_session_id(lineage: &TaskLineageMetadata) -> Option<&str> {
    let value = lineage.child_session_id.as_deref()?.trim();
    (!value.is_empty()).then_some(value)
}
