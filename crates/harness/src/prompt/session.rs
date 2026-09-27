use harness_core::{
    event::EventV1, proj::project_resume_plan, session_lineage::*, store::read_events,
};
use std::path::{Path, PathBuf};
#[cfg(test)]
#[path = "session_tests.rs"]
mod tests;

pub(super) struct Resume {
    pub path: PathBuf,
    pub id: String,
    pub agent: String,
    pub profile: String,
    pub workspace: PathBuf,
    pub from_seq: u64,
}
impl Resume {
    pub fn read(value: &str, sessions: &Path, cwd: &Path) -> Result<Self, String> {
        let path = crate::recovery::resolve_session_run_dir(value, sessions, cwd)?;
        let events = read_events(&path.join("events.jsonl")).map_err(|e| e.to_string())?;
        let id = path
            .file_name()
            .and_then(|s| s.to_str())
            .ok_or("invalid session path")?;
        let plan = project_resume_plan(&events, id).map_err(|e| e.to_string())?;
        if !plan.is_resumable {
            return Err(plan
                .resume_disabled_reason
                .unwrap_or_else(|| "session cannot be resumed".into()));
        }
        if plan.run_id != id {
            return Err("session directory and journal identity disagree".into());
        }
        let agent = events
            .iter()
            .find_map(|event| match &event.payload {
                EventV1::AgentSpawned(agent) if agent.parent_agent_id.is_none() => Some(agent),
                _ => None,
            })
            .ok_or("session has no primary agent")?;
        Ok(Self {
            id: id.into(),
            agent: agent.agent_id.clone(),
            profile: agent.profile.clone(),
            workspace: PathBuf::from(plan.workspace_root.ok_or("session has no workspace")?),
            from_seq: plan.max_seq.saturating_add(1),
            path,
        })
    }
    pub fn fork(&mut self, id: Option<&str>) -> Result<(), String> {
        let events = read_events(&self.path.join("events.jsonl")).map_err(|e| e.to_string())?;
        let prefix = validate_fork_stable_prefix(&events, events.last().map_or(0, |e| e.seq))
            .map_err(|e| e.to_string())?;
        let child = materialize_child_session_as(
            ChildSessionMaterializationRequest {
                source_run_dir: &self.path,
                events: &events,
                stable_prefix: &prefix,
                source_kind: ChildSessionMaterializationSourceKind::DiskRunDirectory,
            },
            id,
        )
        .map_err(|e| e.to_string())?;
        self.id = child.child_run_id;
        self.path = child.child_run_dir;
        self.from_seq = child.event_count as u64 + 1;
        Ok(())
    }
}

pub(super) use crate::exports::journal as export;
