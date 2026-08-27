// allow: SIZE_OK — TUI app state (session projection + interaction)
use crate::UnwrapOrAbort;
use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

use harness_core::event::{first_lineage_parent_session_id, EventEnvelopeV1, EventV1};
use harness_core::proj::{inspect_resume_plan, load_run_metadata};
use serde_json::Value;

use super::child_session::{
    child_agent_info_from_events, child_task_info_from_events, safe_session_id_path_component,
    session_id_from_path, sibling_session_id, subagent_usage_label,
};
use super::{
    set_pending_live_prompt_draft, task_child_request_id_from_output,
    task_child_session_id_from_output, AppState, Focus, LaunchMetadata, ModelOption,
    SubagentSessionInfo, Tab, ToolCallEntry, UiIntent,
};
use crate::text::{has_trimmed_content, non_empty_trimmed};

fn non_empty_str(value: &str) -> Option<&str> {
    has_trimmed_content(value).then_some(value)
}

fn harness_lineage_parent_run_id(run_dir: &Path) -> Option<String> {
    let body = fs::read_to_string(run_dir.join("meta.json")).ok()?;
    let metadata: Value = serde_json::from_str(&body).ok()?;
    metadata
        .get("harness_lineage")
        .and_then(|lineage| lineage.get("parent_run_id"))
        .and_then(Value::as_str)
        .and_then(non_empty_trimmed)
        .map(str::to_string)
}

#[derive(Debug, Clone)]
pub(super) struct SessionNavigationSnapshot {
    pub(super) session_path: PathBuf,
    pub(super) events: Vec<EventEnvelopeV1>,
    canonical_history: Option<Vec<EventEnvelopeV1>>,
    pub(super) launch_metadata: LaunchMetadata,
    pub(super) child_session_ids: Vec<String>,
    pub(super) replay_mode: bool,
}

impl AppState {
    pub(crate) fn current_session_id(&self) -> Option<&str> {
        self.session_path
            .as_deref()
            .and_then(Path::file_name)
            .and_then(|value| value.to_str())
            .and_then(non_empty_trimmed)
    }

    pub(crate) fn current_subagent_session_present(&self) -> bool {
        let Some(current_session_id) = self.current_session_id() else {
            return false;
        };

        self.session_navigation_stack
            .last()
            .and_then(|snapshot| session_id_from_path(&snapshot.session_path))
            .or_else(|| self.current_parent_session_id())
            .is_some_and(|parent_session_id| parent_session_id != current_session_id)
    }

    pub(crate) fn focused_demote_handle_id(&self) -> Option<String> {
        if self.replay_mode {
            return None;
        }

        if let Some(activity) = self
            .activities
            .get(self.transcript_view.selected_activity_index)
        {
            for tool_call in &activity.tool_calls {
                if let Some(handle_id) = Self::task_tool_child_request_id(tool_call) {
                    return Some(handle_id);
                }
            }
        }

        if !self.current_subagent_session_present() {
            return None;
        }
        let current_session_id = self.current_session_id()?;
        let parent_snapshot = self.session_navigation_stack.last();
        let task = parent_snapshot
            .and_then(|snapshot| child_task_info_from_events(&snapshot.events, current_session_id))
            .or_else(|| {
                let parent_session_id = self.current_parent_session_id()?;
                self.session_path_for_id(&parent_session_id)
                    .and_then(|path| {
                        session_navigation_snapshot_from_path(&path, &self.launch_metadata).ok()
                    })
                    .and_then(|snapshot| {
                        child_task_info_from_events(&snapshot.events, current_session_id)
                    })
            })?;
        task.request_id
            .and_then(|id| non_empty_trimmed(&id).map(str::to_string))
    }

    pub(crate) fn current_subagent_session_info(&self) -> Option<SubagentSessionInfo> {
        let current_session_id = self.current_session_id()?;
        let parent_snapshot = self.session_navigation_stack.last();
        let parent_session_id = parent_snapshot
            .and_then(|snapshot| session_id_from_path(&snapshot.session_path))
            .or_else(|| self.current_parent_session_id());

        let parent_session_id =
            parent_session_id.filter(|session_id| session_id != current_session_id)?;

        let sibling_ids = parent_snapshot
            .map(|snapshot| snapshot.child_session_ids.clone())
            .or_else(|| {
                self.session_path_for_id(&parent_session_id)
                    .and_then(|path| {
                        session_navigation_snapshot_from_path(&path, &self.launch_metadata).ok()
                    })
                    .map(|snapshot| snapshot.child_session_ids)
            })
            .unwrap_or_default();
        let total = sibling_ids.len().max(1);
        let index = sibling_ids
            .iter()
            .position(|session_id| session_id == current_session_id)
            .map(|idx| idx + 1)
            .unwrap_or(1);

        let task = parent_snapshot
            .and_then(|snapshot| child_task_info_from_events(&snapshot.events, current_session_id))
            .or_else(|| {
                self.session_path_for_id(&parent_session_id)
                    .and_then(|path| {
                        session_navigation_snapshot_from_path(&path, &self.launch_metadata).ok()
                    })
                    .and_then(|snapshot| {
                        child_task_info_from_events(&snapshot.events, current_session_id)
                    })
            });
        let child_agent = child_agent_info_from_events(&self.events, current_session_id);
        let label = task
            .as_ref()
            .and_then(|task| task.label.as_deref())
            .or_else(|| {
                child_agent
                    .as_ref()
                    .and_then(|agent| agent.label.as_deref())
            })
            .map(super::humanize_profile_label)
            .unwrap_or_else(|| "Subagent".to_string());
        let title = task
            .as_ref()
            .and_then(|task| task.description.clone())
            .or_else(|| {
                child_agent
                    .as_ref()
                    .and_then(|agent| agent.description.clone())
            })
            .filter(|description| has_trimmed_content(description))
            .unwrap_or_else(|| current_session_id.to_string());
        let parent_label = parent_session_id;

        Some(SubagentSessionInfo {
            label,
            title,
            parent_label,
            index,
            total,
            usage: subagent_usage_label(self),
        })
    }

    pub(super) fn child_session_ids(&self) -> Vec<String> {
        let mut child_session_ids = Vec::new();
        let delegated_child_request_ids = self.delegated_child_request_ids();

        for activity in &self.activities {
            if delegated_child_request_ids.contains(activity.request_id.as_str()) {
                continue;
            }
            for tool_call in &activity.tool_calls {
                if let Some(child_session_id) =
                    Self::task_tool_child_session_id_from_entry(tool_call)
                {
                    if !child_session_ids.contains(&child_session_id) {
                        child_session_ids.push(child_session_id);
                    }
                }
            }
        }

        child_session_ids
    }

    fn delegated_child_request_ids(&self) -> BTreeSet<String> {
        self.activities
            .iter()
            .flat_map(|activity| activity.tool_calls.iter())
            .filter_map(Self::task_tool_child_request_id)
            .collect()
    }

    fn task_tool_child_request_id(tool_call: &ToolCallEntry) -> Option<String> {
        if !Self::tool_call_is_task_spawn(tool_call) {
            return None;
        }

        tool_call
            .lineage
            .as_ref()
            .and_then(|lineage| lineage.child_request_id.as_deref())
            .and_then(non_empty_trimmed)
            .map(str::to_string)
            .or_else(|| task_child_request_id_from_output(tool_call.output_json.as_ref()))
    }

    fn task_tool_child_session_id_from_entry(tool_call: &ToolCallEntry) -> Option<String> {
        if !Self::tool_call_is_task_spawn(tool_call) {
            return None;
        }

        tool_call
            .lineage
            .as_ref()
            .and_then(|lineage| lineage.child_session_id.as_deref())
            .and_then(non_empty_trimmed)
            .map(str::to_string)
            .or_else(|| task_child_session_id_from_output(tool_call.output_json.as_ref()))
    }

    pub(super) fn tool_call_is_task_spawn(tool_call: &ToolCallEntry) -> bool {
        matches!(tool_call.effective_tool_id(), "agent.spawn" | "task")
            || matches!(tool_call.tool_id.as_str(), "agent.spawn" | "task")
    }

    pub(super) fn current_parent_session_id(&self) -> Option<String> {
        self.session_path
            .as_deref()
            .and_then(harness_lineage_parent_run_id)
            .or_else(|| first_lineage_parent_session_id(&self.events).map(str::to_string))
    }

    fn current_session_snapshot(&self) -> Option<SessionNavigationSnapshot> {
        let canonical_history = (self.projection.canonical_history() != self.events.as_slice())
            .then(|| self.projection.canonical_history().to_vec());
        Some(SessionNavigationSnapshot {
            session_path: self.session_path.clone()?,
            events: self.events.clone(),
            canonical_history,
            launch_metadata: self.launch_metadata.clone(),
            child_session_ids: self.child_session_ids(),
            replay_mode: self.replay_mode,
        })
    }

    fn restore_session_snapshot(&mut self, snapshot: SessionNavigationSnapshot) {
        self.replay_mode = snapshot.replay_mode;
        self.session_path = Some(snapshot.session_path);
        self.set_launch_metadata(snapshot.launch_metadata);
        self.replace_events_with_canonical_history(snapshot.events, snapshot.canonical_history);
        let restored_metadata =
            infer_launch_metadata_from_events(&self.events, &self.launch_metadata);
        self.launch_metadata = restored_metadata.clone();
        self.runtime_context_metadata = Some(restored_metadata);
        self.active_review_surface = None;
        self.review_surface_focus_return = None;
        self.active_tab = Tab::Run;
        self.focus = if self.replay_mode {
            Focus::Details
        } else {
            Focus::Prompt
        };
        self.normalize_focus_for_active_surface();
    }

    fn session_path_for_id(&self, session_id: &str) -> Option<PathBuf> {
        let session_id = safe_session_id_path_component(session_id)?;

        self.session_path
            .as_deref()
            .and_then(Path::parent)
            .map(|parent| parent.join(session_id))
    }

    fn live_switch_to_session(&mut self, session_id: String, session_path: PathBuf) {
        let resume_plan = inspect_resume_plan(&session_path);
        set_pending_live_prompt_draft(Some(self.composer.prompt_buffer.clone()));
        if resume_plan.is_resumable {
            self.emit_ui_intent(UiIntent::ContinueSession {
                run_id: session_id.clone(),
                run_dir: session_path,
            });
        } else {
            self.emit_ui_intent(UiIntent::ReplaySession {
                run_id: session_id.clone(),
                run_dir: session_path,
            });
        }
        self.should_quit = true;
    }

    fn open_replay_session(&mut self, session_id: String, push_current: bool) {
        let Some(session_path) = self.session_path_for_id(&session_id) else {
            self.set_status_banner(Some(
                "session navigation unavailable: missing session path".to_string(),
            ));
            return;
        };

        if !session_path.is_dir() {
            self.open_inline_child_session(session_id, session_path, push_current);
            return;
        }

        let snapshot =
            match session_navigation_snapshot_from_path(&session_path, &self.launch_metadata) {
                Ok(snapshot) => snapshot,
                Err(err) => {
                    self.set_status_banner(Some(format!("session navigation failed: {err}")));
                    return;
                }
            };

        if push_current {
            if let Some(current_snapshot) = self.current_session_snapshot() {
                let already_pushed = self
                    .session_navigation_stack
                    .last()
                    .map(|existing| existing.session_path.as_path())
                    == Some(current_snapshot.session_path.as_path());
                if !already_pushed {
                    self.session_navigation_stack.push(current_snapshot);
                }
            }
        }

        self.restore_session_snapshot(snapshot);
    }

    fn open_inline_child_session(
        &mut self,
        session_id: String,
        session_path: PathBuf,
        push_current: bool,
    ) {
        let Some(snapshot) = self.inline_child_session_snapshot(&session_id, session_path) else {
            self.set_status_banner(Some(format!("subagent session unavailable: {session_id}")));
            return;
        };

        if push_current {
            if let Some(current_snapshot) = self.current_session_snapshot() {
                self.session_navigation_stack.push(current_snapshot);
            }
        }

        self.restore_session_snapshot(snapshot);
    }

    fn inline_child_session_snapshot(
        &self,
        session_id: &str,
        session_path: PathBuf,
    ) -> Option<SessionNavigationSnapshot> {
        let session_id = session_id.trim();
        if session_id.is_empty() {
            return None;
        }

        let child_request_ids = self
            .activities
            .iter()
            .flat_map(|activity| activity.tool_calls.iter())
            .filter_map(|tool_call| {
                let child_session = tool_call
                    .lineage
                    .as_ref()
                    .and_then(|lineage| lineage.child_session_id.as_deref())
                    .and_then(non_empty_trimmed)
                    .map(str::to_string)
                    .or_else(|| {
                        task_child_session_id_from_output(tool_call.output_json.as_ref())
                    })?;
                (child_session == session_id).then(|| {
                    tool_call
                        .lineage
                        .as_ref()
                        .and_then(|lineage| lineage.child_request_id.as_deref())
                        .and_then(non_empty_trimmed)
                        .map(str::to_string)
                        .or_else(|| {
                            task_child_request_id_from_output(tool_call.output_json.as_ref())
                        })
                })
            })
            .flatten()
            .collect::<BTreeSet<_>>();

        let events = self
            .events
            .iter()
            .filter(|event| {
                matches!(&event.payload, EventV1::RunStarted(_))
                    || event.actor.agent_id.as_deref() == Some(session_id)
                    || event
                        .correlation_id
                        .as_deref()
                        .is_some_and(|request_id| child_request_ids.contains(request_id))
                    || matches!(
                        &event.payload,
                        EventV1::AgentSpawned(payload) if payload.agent_id == session_id
                    )
            })
            .cloned()
            .collect::<Vec<_>>();

        (!events.is_empty()).then(|| SessionNavigationSnapshot {
            session_path,
            events,
            canonical_history: Some(self.projection.canonical_history().to_vec()),
            launch_metadata: self.launch_metadata.clone(),
            child_session_ids: Vec::new(),
            replay_mode: true,
        })
    }

    fn sibling_child_session_target(&self, reverse: bool) -> Option<String> {
        let current_session_id = self.current_session_id()?;
        let siblings = if let Some(parent_snapshot) = self.session_navigation_stack.last() {
            parent_snapshot.child_session_ids.clone()
        } else {
            let parent_session_id = self.current_parent_session_id()?;
            let parent_session_path = self.session_path_for_id(&parent_session_id)?;
            session_navigation_snapshot_from_path(&parent_session_path, &self.launch_metadata)
                .ok()?
                .child_session_ids
        };

        sibling_session_id(&siblings, current_session_id, reverse)
    }

    pub(super) fn navigate_to_first_child_session(&mut self) {
        let Some(session_id) = self.child_session_ids().into_iter().next() else {
            return;
        };

        self.navigate_to_child_session_id(session_id);
    }

    pub(super) fn navigate_to_child_session_id(&mut self, session_id: String) {
        if self.replay_mode {
            self.open_replay_session(session_id, true);
            return;
        }

        if self.session_path_for_id(&session_id).is_some() {
            self.open_replay_session(session_id, true);
        }
    }

    pub(super) fn navigate_to_child_sibling(&mut self, reverse: bool) {
        let target_session_id = self.sibling_child_session_target(reverse).or_else(|| {
            let child_session_ids = self.child_session_ids();
            if reverse {
                child_session_ids.into_iter().last()
            } else {
                child_session_ids.into_iter().next()
            }
        });
        let Some(target_session_id) = target_session_id else {
            return;
        };

        if self.replay_mode {
            if let Some(parent_snapshot) = self.session_navigation_stack.last().cloned() {
                self.restore_session_snapshot(parent_snapshot);
                self.open_replay_session(target_session_id, false);
                return;
            }

            self.open_replay_session(
                target_session_id,
                self.current_parent_session_id().is_none()
                    && self.session_navigation_stack.is_empty(),
            );
            return;
        }

        if let Some(session_path) = self.session_path_for_id(&target_session_id) {
            if session_path.is_dir() {
                self.live_switch_to_session(target_session_id, session_path);
            } else {
                self.open_inline_child_session(target_session_id, session_path, true);
            }
        }
    }

    pub(super) fn navigate_to_parent_session(&mut self) {
        if self.replay_mode {
            if let Some(parent_snapshot) = self.session_navigation_stack.pop() {
                self.restore_session_snapshot(parent_snapshot);
                return;
            }
        }

        let Some(parent_session_id) = self.current_parent_session_id() else {
            return;
        };

        if self.replay_mode {
            let Some(parent_session_path) = self.session_path_for_id(&parent_session_id) else {
                self.set_status_banner(Some(
                    "session navigation unavailable: missing parent session path".to_string(),
                ));
                return;
            };
            if self.replay_navigation_handoff_enabled {
                self.live_switch_to_session(parent_session_id, parent_session_path);
                return;
            }
            match session_navigation_snapshot_from_path(&parent_session_path, &self.launch_metadata)
            {
                Ok(snapshot) => self.restore_session_snapshot(snapshot),
                Err(err) => {
                    self.set_status_banner(Some(format!("session navigation failed: {err}")));
                }
            }
            return;
        }

        if let Some(parent_session_path) = self.session_path_for_id(&parent_session_id) {
            self.live_switch_to_session(parent_session_id, parent_session_path);
        }
    }

    pub(super) fn restore_parent_session_for_quit(&mut self) {
        if !self.replay_mode || self.session_navigation_stack.is_empty() {
            return;
        }

        let parent_snapshot = self.session_navigation_stack.remove(0);
        self.session_navigation_stack.clear();
        self.restore_session_snapshot(parent_snapshot);
    }
}

fn infer_launch_metadata_from_events(
    events: &[EventEnvelopeV1],
    fallback: &LaunchMetadata,
) -> LaunchMetadata {
    let profile = events
        .iter()
        .find_map(|event| match &event.payload {
            EventV1::AgentSpawned(payload) => Some(payload.profile.clone()),
            _ => None,
        })
        .unwrap_or_else(|| fallback.profile().to_string());
    let (provider, model) = events
        .iter()
        .find_map(|event| match &event.payload {
            EventV1::ProviderRequestStarted(payload) => {
                Some((payload.provider_id.clone(), Some(payload.model_id.clone())))
            }
            _ => None,
        })
        .unwrap_or_else(|| {
            (
                fallback.provider().to_string(),
                fallback.model().map(str::to_string),
            )
        });

    let mut launch_metadata = LaunchMetadata::new(profile, provider, model)
        .with_available_models(fallback.available_models().to_vec())
        .with_switchable_profiles(fallback.switchable_profiles().to_vec());
    if let Some(mode_label) = fallback.mode_label().map(str::to_owned) {
        launch_metadata = launch_metadata.with_mode_label(mode_label);
    }
    launch_metadata
}

fn replay_launch_metadata_from_session(
    session_path: &Path,
    events: &[EventEnvelopeV1],
    fallback: &LaunchMetadata,
) -> LaunchMetadata {
    load_run_metadata(session_path)
        .and_then(|metadata| {
            metadata
                .recorded_runtime_context
                .as_ref()
                .map(|context| launch_metadata_from_recorded_runtime_context(context, fallback))
        })
        .unwrap_or_else(|| infer_launch_metadata_from_events(events, fallback))
}

fn launch_metadata_from_recorded_runtime_context(
    recorded_runtime_context: &harness_core::proj::RecordedRuntimeContext,
    fallback: &LaunchMetadata,
) -> LaunchMetadata {
    let mut launch_metadata = LaunchMetadata::from_model_option(&ModelOption {
        profile: recorded_runtime_context.profile.clone(),
        provider: recorded_runtime_context.provider.clone(),
        provider_display_label: recorded_runtime_context.provider_display_label.clone(),
        provider_backend_label: recorded_runtime_context.provider_backend_label.clone(),
        model: recorded_runtime_context.model.clone(),
        model_display_label: recorded_runtime_context.model_display_label.clone(),
        variant: recorded_runtime_context.variant.clone(),
        variant_display_label: recorded_runtime_context.variant_display_label.clone(),
        display_label: Some(recorded_runtime_context.display_label.clone())
            .filter(|value| non_empty_str(value).is_some()),
        token_window_label: recorded_runtime_context.token_window_label.clone(),
        model_limits: recorded_runtime_context.effective_model_limits(),
        description: recorded_runtime_context.description.clone(),
        profile_description: recorded_runtime_context.profile_description.clone(),
        reasoning_effort: recorded_runtime_context.reasoning_effort.clone(),
        text_verbosity: recorded_runtime_context.text_verbosity.clone(),
        thinking: recorded_runtime_context.thinking.clone(),
        recommended_for: recorded_runtime_context.recommended_for.clone(),
    })
    .with_available_models(fallback.available_models().to_vec())
    .with_switchable_profiles(fallback.switchable_profiles().to_vec());
    if let Some(snapshot) = recorded_runtime_context.last_request_budget {
        launch_metadata = launch_metadata.with_last_request_budget(snapshot);
    }
    if let Some(mode_label) = fallback.mode_label().map(str::to_owned) {
        launch_metadata = launch_metadata.with_mode_label(mode_label);
    }
    launch_metadata
}

fn session_navigation_snapshot_from_path(
    session_path: &Path,
    fallback_launch_metadata: &LaunchMetadata,
) -> Result<SessionNavigationSnapshot, String> {
    let events = crate::session_events::load_session_events(session_path)?;
    let launch_metadata =
        replay_launch_metadata_from_session(session_path, &events, fallback_launch_metadata);
    let replay = AppState::new_replay(session_path.to_path_buf(), events.clone());

    Ok(SessionNavigationSnapshot {
        session_path: session_path.to_path_buf(),
        events,
        canonical_history: None,
        launch_metadata,
        child_session_ids: replay.child_session_ids(),
        replay_mode: true,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::UnwrapOrAbort;

    fn actor(
        kind: harness_core::event::ActorKind,
        agent_id: &str,
    ) -> harness_core::event::EventActor {
        harness_core::event::EventActor::new(kind, Some(agent_id.to_string()))
    }

    fn event(
        seq: u64,
        correlation_id: Option<&str>,
        actor: harness_core::event::EventActor,
        payload: EventV1,
    ) -> EventEnvelopeV1 {
        EventEnvelopeV1 {
            schema_version: harness_core::event::SCHEMA_VERSION,
            event_id: format!("evt_subagent_nav_{seq:04}"),
            seq,
            run_id: "parent_run".into(),
            mono_ms: seq * 100,
            ts: Some(format!("2026-03-22T14:36:{seq:02}Z")),
            actor,
            correlation_id: correlation_id.map(str::to_string),
            causation_id: None,
            stream_key: None,
            payload,
        }
    }

    #[test]
    fn parent_session_with_child_lineage_is_not_its_own_subagent() {
        // arrange
        // act
        // assert
        let mut app = AppState::new_replay(
            PathBuf::from("/tmp/harness-subagent-parent/parent_run"),
            vec![event(
                1,
                Some("req_parent"),
                actor(harness_core::event::ActorKind::System, "coordinator"),
                EventV1::ToolCallRequested(harness_core::event::ToolCallRequestedEvent {
                    tool_call_id: "toolcall_000001".into(),
                    tool_id: "task".to_string(),
                    args_summary:
                        r#"{"description":"map chat renderers","subagent_type":"researcher"}"#
                            .to_string(),
                    args_digest: "digest-task-call".to_string(),
                    metadata: Some(harness_core::event::ToolCallMetadata {
                        lineage: Some(harness_core::event::TaskLineageMetadata {
                            parent_tool_call_id: Some("toolcall_000001".to_string()),
                            parent_request_id: Some("req_parent".to_string()),
                            parent_session_id: Some("parent_run".to_string()),
                            child_session_id: Some("agent_worker".to_string()),
                            child_request_id: Some("req_child".to_string()),
                            ..harness_core::event::TaskLineageMetadata::default()
                        }),
                        ..harness_core::event::ToolCallMetadata::default()
                    }),
                }),
            )],
        );
        app.session_path = Some(PathBuf::from("/tmp/harness-subagent-parent/parent_run"));

        assert!(!app.current_subagent_session_present());
        assert!(app.current_subagent_session_info().is_none());
    }

    #[test]
    fn subagent_session_info_uses_spawned_profile_when_task_args_omit_label() {
        // arrange
        // act
        // assert
        let mut app = AppState::new_live(None, false, None);
        app.session_path = Some(PathBuf::from("/tmp/harness-subagent-parent/parent_run"));
        app.ingest_event(event(
            1,
            Some("req_parent"),
            actor(harness_core::event::ActorKind::System, "coordinator"),
            EventV1::ToolCallRequested(harness_core::event::ToolCallRequestedEvent {
                tool_call_id: "tc_task".into(),
                tool_id: "task".to_string(),
                args_summary: r#"{"description":"map chat renderers"}"#.to_string(),
                args_digest: "digest-task-call".to_string(),
                metadata: Some(harness_core::event::ToolCallMetadata {
                    lineage: Some(harness_core::event::TaskLineageMetadata {
                        parent_tool_call_id: Some("tc_task".to_string()),
                        parent_request_id: Some("req_parent".to_string()),
                        parent_session_id: Some("parent_run".to_string()),
                        child_session_id: Some("agent_worker".to_string()),
                        child_request_id: Some("req_child".to_string()),
                        ..harness_core::event::TaskLineageMetadata::default()
                    }),
                    ..harness_core::event::ToolCallMetadata::default()
                }),
            }),
        ));
        app.ingest_event(event(
            2,
            Some("req_child"),
            actor(harness_core::event::ActorKind::Worker, "agent_worker"),
            EventV1::AgentSpawned(harness_core::event::AgentSpawnedEvent {
                agent_id: "agent_worker".to_string(),
                profile: "sisyphus-junior".to_string(),
                parent_agent_id: Some("agent_parent".to_string()),
            }),
        ));

        app.navigate_to_child_session_id("agent_worker".to_string());

        let info = app.current_subagent_session_info().unwrap_or_abort();
        assert_eq!(info.label, "Sisyphus Junior");
        assert_eq!(info.title, "map chat renderers");
    }

    #[test]
    fn session_path_for_id_rejects_unsafe_event_derived_ids() {
        // arrange
        // act
        // assert
        let mut app = AppState::new_live(None, false, None);
        app.session_path = Some(PathBuf::from("/tmp/harness-sessions/parent_run"));

        assert_eq!(
            app.session_path_for_id("child_run"),
            Some(PathBuf::from("/tmp/harness-sessions/child_run"))
        );
        for unsafe_id in [
            "",
            ".",
            "..",
            "../secrets",
            "/tmp/secrets",
            "child/run",
            "child\\run",
            "child\nrun",
        ] {
            assert_eq!(
                app.session_path_for_id(unsafe_id),
                None,
                "unsafe session id should be rejected: {unsafe_id:?}"
            );
        }
    }

    // -----------------------------------------------------------------------
    // Relocated from dashboard_queue_worktree_consistency_test.rs (private API).
    // These scenarios exercise pub(super)/pub(crate) session-stack navigation
    // state that integration tests cannot reach without widening visibility.
    // -----------------------------------------------------------------------

    fn setup_parent_with_child(app: &mut AppState) {
        app.session_path = Some(PathBuf::from("/tmp/harness-dash-consistency/parent_run"));
        app.ingest_event(event(
            1,
            None,
            actor(harness_core::event::ActorKind::System, "dash-consistency"),
            EventV1::RunStarted(harness_core::event::RunStartedEvent {
                run_name: "parent_run".into(),
                workspace_root: "/workspace".to_string(),
            }),
        ));
        app.ingest_event(event(
            2,
            Some("req_parent"),
            actor(harness_core::event::ActorKind::User, "interactive-user"),
            EventV1::UserMessageSubmitted(harness_core::event::UserMessageSubmittedEvent {
                request_id: "req_parent".into(),
                text: "Run audit".to_string(),
            }),
        ));
        app.ingest_event(event(
            3,
            Some("req_parent"),
            actor(harness_core::event::ActorKind::Worker, "agent_parent"),
            EventV1::ProviderRequestStarted(harness_core::event::ProviderRequestStartedEvent {
                request_id: "req_parent".into(),
                provider_id: "openai".to_string(),
                model_id: "gpt-5-codex".to_string(),
                prompt_summary: "prompt".to_string(),
                request_digest: "digest-req_parent".to_string(),
                metadata: None,
            }),
        ));
        app.ingest_event(event(
            4,
            Some("req_parent"),
            actor(harness_core::event::ActorKind::System, "coordinator"),
            EventV1::ToolCallRequested(harness_core::event::ToolCallRequestedEvent {
                tool_call_id: "tc_task".into(),
                tool_id: "task".to_string(),
                args_summary: r#"{"description":"test task","subagent_type":"explore"}"#
                    .to_string(),
                args_digest: "digest-tc_task".to_string(),
                metadata: Some(harness_core::event::ToolCallMetadata {
                    lineage: Some(harness_core::event::TaskLineageMetadata {
                        parent_tool_call_id: Some("tc_task".to_string()),
                        parent_request_id: Some("req_parent".to_string()),
                        parent_session_id: Some("parent_run".to_string()),
                        child_session_id: Some("agent_worker".to_string()),
                        child_request_id: Some("req_child".to_string()),
                        ..harness_core::event::TaskLineageMetadata::default()
                    }),
                    ..harness_core::event::ToolCallMetadata::default()
                }),
            }),
        ));
        app.ingest_event(event(
            5,
            Some("req_child"),
            actor(harness_core::event::ActorKind::Worker, "agent_worker"),
            EventV1::AgentSpawned(harness_core::event::AgentSpawnedEvent {
                agent_id: "agent_worker".to_string(),
                profile: "explore".to_string(),
                parent_agent_id: Some("agent_parent".to_string()),
            }),
        ));
        app.ingest_event(event(
            6,
            Some("req_child"),
            actor(harness_core::event::ActorKind::Worker, "agent_worker"),
            EventV1::ProviderRequestStarted(harness_core::event::ProviderRequestStartedEvent {
                request_id: "req_child".into(),
                provider_id: "default".to_string(),
                model_id: "model-1".to_string(),
                prompt_summary: "child task".to_string(),
                request_digest: "digest-child".to_string(),
                metadata: None,
            }),
        ));
    }

    #[test]
    fn subagent_status_returns_info_for_child_session() {
        // arrange
        // act
        let mut app = AppState::new_live(None, false, None);
        setup_parent_with_child(&mut app);
        app.navigate_to_child_session_id("agent_worker".to_string());
        let info = app
            .current_subagent_session_info()
            .expect("subagent info must be available after navigating to child");
        // assert
        assert!(!info.label.is_empty(), "subagent label must be non-empty");
        assert!(!info.title.is_empty(), "subagent title must be non-empty");
        assert_eq!(
            info.parent_label, "parent_run",
            "subagent parent_label must match parent session id"
        );
    }

    #[test]
    fn subagent_catalog_lists_child_session_ids() {
        // arrange
        // act
        let mut app = AppState::new_live(None, false, None);
        setup_parent_with_child(&mut app);
        let children = app.child_session_ids();
        // assert
        assert!(
            children.iter().any(|id| id == "agent_worker"),
            "child_session_ids must include agent_worker: {:?}",
            children
        );
    }

    #[test]
    fn subagent_catalog_empty_when_no_children() {
        // arrange
        // act
        let app = AppState::new_live(None, false, None);
        let children = app.child_session_ids();
        // assert
        assert!(
            children.is_empty(),
            "child_session_ids must be empty when no task spawns exist"
        );
    }

    #[test]
    fn subagent_current_session_id_returns_none_without_session_path() {
        // arrange
        // act
        let app = AppState::new_live(None, false, None);
        // assert
        assert!(
            app.current_session_id().is_none(),
            "current_session_id must be None without session_path"
        );
    }

    #[test]
    fn subagent_current_session_id_returns_path_component() {
        // arrange
        // act
        let mut app = AppState::new_live(None, false, None);
        app.session_path = Some(PathBuf::from("/tmp/harness-sessions/run-001"));
        // assert
        assert_eq!(
            app.current_session_id(),
            Some("run-001"),
            "current_session_id must return the last path component"
        );
    }

    #[test]
    fn subagent_session_present_false_for_root_session() {
        // arrange
        // act
        let mut app = AppState::new_live(None, false, None);
        app.session_path = Some(PathBuf::from("/tmp/harness-sessions/parent_run"));
        setup_parent_with_child(&mut app);
        // assert
        assert!(
            !app.current_subagent_session_present(),
            "root session must not be a subagent"
        );
    }

    #[test]
    fn session_entry_navigates_to_child_session() {
        // arrange
        // act
        let mut app = AppState::new_live(None, false, None);
        app.session_path = Some(PathBuf::from("/tmp/harness-dash-consistency/parent_run"));
        setup_parent_with_child(&mut app);
        app.navigate_to_child_session_id("agent_worker".to_string());
        // assert
        assert!(
            app.replay_mode,
            "navigating to inline child must enter replay mode"
        );
        assert_eq!(
            app.current_session_id(),
            Some("agent_worker"),
            "after navigation, current_session_id must be the child"
        );
    }

    #[test]
    fn session_return_navigates_to_parent_session() {
        // arrange
        // act
        let mut app = AppState::new_live(None, false, None);
        app.session_path = Some(PathBuf::from("/tmp/harness-dash-consistency/parent_run"));
        setup_parent_with_child(&mut app);
        app.navigate_to_child_session_id("agent_worker".to_string());
        // assert
        assert_eq!(app.current_session_id(), Some("agent_worker"));
        app.navigate_to_parent_session();
        assert_eq!(
            app.current_session_id(),
            Some("parent_run"),
            "after return, current_session_id must be the parent"
        );
    }

    #[test]
    fn session_entry_sibling_cycle_wraps_around() {
        // arrange
        // act
        let mut app = AppState::new_live(None, false, None);
        app.session_path = Some(PathBuf::from("/tmp/harness-dash-consistency/parent_run"));
        setup_parent_with_child(&mut app);
        app.ingest_event(event(
            7,
            Some("req_parent"),
            actor(harness_core::event::ActorKind::System, "coordinator"),
            EventV1::ToolCallRequested(harness_core::event::ToolCallRequestedEvent {
                tool_call_id: "tc_task2".into(),
                tool_id: "task".to_string(),
                args_summary: r#"{"description":"second task","subagent_type":"general"}"#
                    .to_string(),
                args_digest: "digest-tc_task2".to_string(),
                metadata: Some(harness_core::event::ToolCallMetadata {
                    lineage: Some(harness_core::event::TaskLineageMetadata {
                        parent_tool_call_id: Some("tc_task2".to_string()),
                        parent_request_id: Some("req_parent".to_string()),
                        parent_session_id: Some("parent_run".to_string()),
                        child_session_id: Some("agent_worker2".to_string()),
                        child_request_id: Some("req_child2".to_string()),
                        ..harness_core::event::TaskLineageMetadata::default()
                    }),
                    ..harness_core::event::ToolCallMetadata::default()
                }),
            }),
        ));
        app.ingest_event(event(
            8,
            Some("req_child2"),
            actor(harness_core::event::ActorKind::Worker, "agent_worker2"),
            EventV1::AgentSpawned(harness_core::event::AgentSpawnedEvent {
                agent_id: "agent_worker2".to_string(),
                profile: "general".to_string(),
                parent_agent_id: Some("agent_parent".to_string()),
            }),
        ));
        let children = app.child_session_ids();
        // assert
        assert!(
            children.len() >= 2,
            "must have at least 2 children for sibling cycle: {:?}",
            children
        );
    }

    #[test]
    fn empty_dashboard_has_no_child_sessions() {
        // arrange
        // act
        let app = AppState::new_live(None, false, None);
        let children = app.child_session_ids();
        // assert
        assert!(
            children.is_empty(),
            "empty dashboard must have no child sessions"
        );
    }

    #[test]
    fn empty_dashboard_has_no_subagent_info() {
        // arrange
        // act
        let app = AppState::new_live(None, false, None);
        // assert
        assert!(
            app.current_subagent_session_info().is_none(),
            "empty dashboard must have no subagent session info"
        );
    }
}
