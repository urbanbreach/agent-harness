use super::{super::context::Context, super::runtime::*, *};
use crate::subagent::*;
use sha2::{Digest, Sha256};
use std::path::Path;

impl Runtime {
    pub(in crate::coord::subagents) fn prepare_finalized_state(
        &mut self,
        agent_id: &str,
        attempt: &str,
        messages: &Context,
    ) -> Result<FinalizedAgentStateReferenceV1, CoordinatorError> {
        let agent = &self.agents[agent_id];
        let unavailable = messages.unavailable.or_else(|| {
            (messages.model_request.is_none()
                || !messages
                    .usage
                    .iter()
                    .any(|entry| entry.attempt_id == attempt))
            .then_some(FinalizedStateUnavailable::Incomplete)
        });
        let fidelity = if messages.unavailable == Some(FinalizedStateUnavailable::LegacySummaryOnly)
        {
            FinalizedStateFidelity::SummaryOnly
        } else {
            FinalizedStateFidelity::Exact
        };
        let snapshot = RawFinalizedState {
            payload_version: 1,
            owner_run_id: self.info()?.run_id.to_string(),
            owner_agent_id: SubagentId(agent_id.into()),
            owner_session_id: if agent.info.parent_agent_id.is_some() {
                agent_id.into()
            } else {
                self.info()?.run_id.to_string()
            },
            attempt_id: attempt.into(),
            generation: agent.generation,
            conversation_items: messages
                .entries
                .iter()
                .filter(|e| e.message.role != harness_providers::MessageRole::System)
                .map(|e| FinalizedConversationItem {
                    message: e.message.clone(),
                    event_seq: e.seq,
                    turn_id: e.turn.clone(),
                    attachments: e.attachments.clone(),
                    settled_reasoning: e.settled_reasoning.clone(),
                    raw_tool_result: e.raw_tool_result.clone(),
                })
                .collect(),
            model_request: messages.model_request.clone(),
            usage: messages.usage.clone(),
            native_context_usage: messages.native_context_usage,
            source_model: agent.info.model_ref.clone(),
            model_target: agent.target.clone(),
            model_settings: agent.settings.clone(),
            execution_context: agent.execution.clone(),
            read_state: agent
                .tool_state
                .read_snapshot()
                .map_err(|e| CoordinatorError::Invalid(e.to_string()))?,
            skill_startup: agent.skill_startup.as_deref().cloned(),
            skill_preload_names: self
                .native_subagents
                .get(agent_id)
                .map_or_else(Vec::new, |child| child.preload_skills()),
            source_reference: agent.source_reference.clone(),
            fidelity,
            availability: if unavailable.is_some() {
                FinalizedStateAvailability::Unsupported
            } else {
                FinalizedStateAvailability::Available
            },
            unavailable,
        };
        let unrepresentable = agent.cwd.to_str().is_none()
            || snapshot
                .read_state
                .keys()
                .any(|path| path.to_str().is_none());
        let bytes = if unrepresentable {
            Vec::new()
        } else {
            serde_json::to_vec(&snapshot)?
        };
        let text = String::from_utf8_lossy(&bytes);
        let safe = crate::redact::redact_artifact_text(&text);
        let policy_modified =
            safe != text || self.redactor.redact_text(&text) != text || text.contains("[REDACTED");
        let unavailable = if unrepresentable {
            Some(FinalizedStateUnavailable::UnsupportedEncoding)
        } else if policy_modified {
            Some(FinalizedStateUnavailable::PolicyModified)
        } else if bytes.len() as u64 > MAX_FINALIZED_STATE_BYTES {
            Some(FinalizedStateUnavailable::Incomplete)
        } else {
            unavailable
        };
        let mut reference = FinalizedAgentStateReferenceV1 {
            payload_version: 1,
            state: FinalizedSubagentStateReference {
                payload_version: 1,
                sha256: String::new(),
                owner: SubagentId(agent_id.into()),
                fidelity: if policy_modified {
                    FinalizedStateFidelity::Redacted
                } else {
                    fidelity
                },
                availability: if unavailable.is_some() {
                    FinalizedStateAvailability::Unsupported
                } else {
                    FinalizedStateAvailability::Available
                },
            },
            byte_length: 0,
            owner_run_id: snapshot.owner_run_id.clone(),
            owner_session_id: snapshot.owner_session_id,
            attempt_id: attempt.into(),
            generation: agent.generation,
            unavailable,
        };
        if !unrepresentable && !policy_modified && bytes.len() as u64 <= MAX_FINALIZED_STATE_BYTES {
            reference.state.sha256 = hex::encode(Sha256::digest(&bytes));
            reference.byte_length = bytes.len() as u64;
            crate::store::create_private_dir(&self.info()?.artifacts_dir)?;
            crate::store::write_private_atomic(
                &self.info()?.run_dir.join(reference.artifact_path()),
                &bytes,
            )?;
        }
        Ok(reference)
    }

    pub(in crate::coord) fn resolve_agent_finalized_state(
        &self,
        id: &str,
    ) -> Result<FinalizedStateResult, CoordinatorError> {
        let agent = self
            .agents
            .get(id)
            .ok_or_else(|| CoordinatorError::UnknownAgent(id.into()))?;
        if self.fault.is_some() {
            return Ok(FinalizedStateResult::Unavailable {
                reason: FinalizedStateUnavailable::Incomplete,
            });
        }
        if agent.busy
            || !agent.queue.is_empty()
            || self
                .running
                .values()
                .any(|job| job.actor.agent_id.as_deref() == Some(id))
        {
            return Ok(FinalizedStateResult::Unavailable {
                reason: FinalizedStateUnavailable::Active,
            });
        }
        let Some(reference) = &agent.finalized else {
            return Ok(FinalizedStateResult::Unavailable {
                reason: agent
                    .messages
                    .unavailable
                    .unwrap_or(FinalizedStateUnavailable::Missing),
            });
        };
        let result = resolve_finalized_state(
            &self.info()?.run_dir,
            reference,
            self.projection_owner
                .as_deref()
                .filter(|owner| *owner == reference.owner_run_id)
                .unwrap_or(self.info()?.run_id.as_str()),
            &SubagentId(id.into()),
            &reference.attempt_id,
        );
        if let FinalizedStateResult::Available { state } = &result {
            let value = serde_json::to_value(state)?;
            if crate::redact::redact_value(self.redactor.as_ref(), &value) != value {
                return Ok(FinalizedStateResult::Unavailable {
                    reason: FinalizedStateUnavailable::PolicyModified,
                });
            }
            for attachment in state
                .conversation_items
                .iter()
                .flat_map(|item| {
                    item.attachments
                        .iter()
                        .chain(item.raw_tool_result.iter().flat_map(|result| {
                            result.output.iter().flat_map(|output| &output.attachments)
                        }))
                })
                .chain(
                    state
                        .model_request
                        .iter()
                        .flat_map(|request| request.attachments.values().flatten()),
                )
            {
                let bytes = attachment
                    .bytes()
                    .map_err(|e| CoordinatorError::Invalid(e.to_string()))?;
                let text = String::from_utf8_lossy(bytes);
                if self.redactor.redact_text(&text) != text {
                    return Ok(FinalizedStateResult::Unavailable {
                        reason: FinalizedStateUnavailable::PolicyModified,
                    });
                }
            }
        }
        Ok(result)
    }

    pub(in crate::coord) fn initialize_finalized(
        &mut self,
        actor: EventActor,
        target: &str,
        source: &str,
        mode: FinalizedContextCopy,
    ) -> Result<(), CoordinatorError> {
        self.accepting()?;
        Runtime::authorize_agent_state(self, &actor, target)?;
        Runtime::authorize_agent_state(self, &actor, source)?;
        let target_state = &self.agents[target];
        if target_state.busy
            || !target_state.queue.is_empty()
            || self
                .running
                .values()
                .any(|job| job.actor.agent_id.as_deref() == Some(target))
            || (mode != FinalizedContextCopy::Wake
                && (target == source || target_state.generation != 0))
            || (mode == FinalizedContextCopy::Wake
                && (target != source || self.killed_agents.contains(target)))
        {
            return Err(CoordinatorError::Invalid(
                "copy requires a fresh target or an idle same-identity wake".into(),
            ));
        }
        if mode == FinalizedContextCopy::Resume
            && target_state.info.parent_agent_id != self.agents[source].info.parent_agent_id
        {
            return Err(CoordinatorError::PermissionDenied(
                "resume source must have the same spawner".into(),
            ));
        }
        let FinalizedStateResult::Available { mut state } =
            self.resolve_agent_finalized_state(source)?
        else {
            return Err(CoordinatorError::Invalid(
                "finalized source is unavailable".into(),
            ));
        };
        let source_cwd = PathBuf::from(&state.execution_context.effective_cwd).canonicalize()?;
        let root = &self.info()?.workspace_root;
        let selector = source_cwd
            .strip_prefix(root)
            .unwrap_or(&source_cwd)
            .to_string_lossy();
        if source_cwd != Path::new(&state.execution_context.effective_cwd)
            || !source_cwd.is_dir()
            || !source_cwd.starts_with(root)
            || matches!(
                state.execution_context.isolation,
                ResolvedSubagentIsolation::Worktree { .. }
            )
            || self
                .config
                .permission_policy
                .check("read", &selector, Some(&target_state.policy))
                != crate::perm::PermissionAction::Allow
        {
            return Err(CoordinatorError::PermissionDenied(
                "source cwd is no longer supported by the target policy".into(),
            ));
        }
        let reference = self.agents[source]
            .finalized
            .clone()
            .ok_or_else(|| CoordinatorError::Invalid("source reference missing".into()))?;
        let context = super::restored_context(
            &mut state,
            &target_state.profile.system_prompt,
            &self.info()?.run_dir,
        )?;
        let reads = self
            .tool_state
            .with_read_snapshot(state.read_state.clone())
            .map_err(|e| CoordinatorError::Invalid(e.to_string()))?;
        let target_id = target.to_owned();
        self.emit_applied(
            actor,
            None,
            EventV1::AgentContextInitialized(AgentContextInitializedV1 {
                payload_version: 1,
                agent_id: SubagentId(target.into()),
                mode,
                source: reference.clone(),
            }),
            move |runtime, _| {
                if let Some(target) = runtime.agents.get_mut(&target_id) {
                    target.messages = context;
                    target.tool_state = reads;
                    target.source_reference = Some(Box::new(reference));
                    if mode != FinalizedContextCopy::Fork {
                        target.execution = state.execution_context;
                        target.cwd = source_cwd;
                        target.info.model_ref = state.source_model;
                        target.settings = state.model_settings;
                        target.target = state.model_target;
                    }
                }
            },
        )?;
        Ok(())
    }
}
