use super::*;

impl Runtime {
    pub(super) fn verify_native_message_sender(
        &self,
        sender: &str,
        tool: &str,
        attempt: Option<&str>,
        generation: u64,
    ) -> bool {
        let Some(child) = self.native_subagents.get(sender) else {
            return self.agents.contains_key(sender) && self.running.contains_key(tool);
        };
        let Some(agent) = self.agents.get(sender) else {
            return false;
        };
        child.phase == NativePhase::Running
            && child.registration.messaging_granted
            && !child.explicitly_killed
            && !child.cancellation.is_cancelled()
            && agent.generation == generation
            && agent.attempt.as_deref() == attempt
            && self.running.get(tool).is_some_and(|job| {
                job.actor.agent_id.as_deref() == Some(sender)
                    && job.parent.as_deref() == attempt
                    && !job.cancellation.is_cancelled()
            })
    }

    pub(super) fn admit_native_message(
        &mut self,
        actor: EventActor,
        tool: String,
        input: SendSubagentMessageInput,
    ) -> Result<Admission, CoordinatorError> {
        self.authenticated_native_tool(&actor, &tool, false)?;
        if !self.config.subagents.messaging_enabled {
            return Ok(Admission::Immediate(SendSubagentMessageResult::Unsupported));
        }
        let sender = actor
            .agent_id
            .as_deref()
            .ok_or_else(|| {
                CoordinatorError::PermissionDenied("message sender is not authenticated".into())
            })?
            .to_owned();
        let child_sender = self.native_subagents.contains_key(&sender);
        if child_sender
            && !self.native_subagents[&sender]
                .registration
                .messaging_granted
        {
            return Ok(Admission::Immediate(SendSubagentMessageResult::Unsupported));
        }
        let bytes = input.text.len();
        if bytes == 0 || bytes > 32768 {
            return Ok(Admission::Immediate(SendSubagentMessageResult::Limit {
                max_bytes: 32768,
                observed_bytes: bytes,
            }));
        }
        if child_sender
            && input.subagent_id != "parent"
            && uuid::Uuid::parse_str(&input.subagent_id).is_err()
        {
            return Err(native_invalid(
                "subagent_id must be a valid agent ID".into(),
            ));
        }
        let attempt = self.agents[&sender].attempt.clone();
        let generation = self.agents[&sender].generation;
        if !self.verify_native_message_sender(&sender, &tool, attempt.as_deref(), generation) {
            return Ok(Admission::Immediate(
                SendSubagentMessageResult::NotActiveOrFinalizing,
            ));
        }
        if let Some(child) = self.native_subagents.get_mut(&sender) {
            if child.sender_generation != generation {
                child.sender_generation = generation;
                child.outbound = 0;
            }
            if child.outbound >= 32 {
                return Ok(Admission::Immediate(
                    SendSubagentMessageResult::QuotaExceeded {
                        kind: SendSubagentMessageQuotaKind::AttemptOutbound,
                        limit: 32,
                    },
                ));
            }
            // Native quota counts ingress, including target-resolution failures.
            child.outbound += 1;
        }
        if input.subagent_id == "parent" && !child_sender {
            return Ok(Admission::Immediate(SendSubagentMessageResult::Unsupported));
        }
        let target = if input.subagent_id == "parent" && child_sender {
            let target = self.native_subagents[&sender].registration.spawner.clone();
            if !self.native_subagents.contains_key(&target) {
                return Ok(Admission::Immediate(SendSubagentMessageResult::Unsupported));
            }
            target
        } else {
            uuid::Uuid::parse_str(&input.subagent_id)
                .map_or_else(|_| input.subagent_id.clone(), |id| id.to_string())
        };
        if target == sender
            || !self.native_subagents.contains_key(&target)
            || (!child_sender && !self.native_reachable(&actor, &target))
        {
            return Ok(Admission::Immediate(
                SendSubagentMessageResult::NotFoundOrNotOwned,
            ));
        }
        let child = &self.native_subagents[&target];
        if child.explicitly_killed
            || child.cancellation.is_cancelled()
            || child.phase == NativePhase::Finalizing
            || self.killed_agents.contains(&target)
            || self
                .stopped_sessions
                .contains(&child.registration.root_agent)
            || self.stopped_sessions.contains(&target)
            || child.phase == NativePhase::Terminal
                && (child.updates.borrow().result.status == "cancelled"
                    || !child.terminal_published)
        {
            return Ok(Admission::Immediate(
                SendSubagentMessageResult::NotActiveOrFinalizing,
            ));
        }
        let pending_pairs = child
            .parked
            .iter()
            .filter(|pending| {
                pending.receipt.sender == sender
                    && pending.receipt.sender_attempt == attempt
                    && pending.receipt.sender_generation == generation
            })
            .count();
        if child_sender && pending_pairs >= 4 {
            return Ok(Admission::Immediate(
                SendSubagentMessageResult::QuotaExceeded {
                    kind: SendSubagentMessageQuotaKind::SenderTargetInFlight,
                    limit: 4,
                },
            ));
        }
        if self.native_message_ingress >= 64 {
            return Ok(Admission::Immediate(SendSubagentMessageResult::Saturated {
                max_in_flight: 64,
            }));
        }
        if child.parked.len() >= 8 {
            return Ok(Admission::Immediate(SendSubagentMessageResult::Saturated {
                max_in_flight: 8,
            }));
        }
        let mut receipt = NativeMessageReceipt {
            payload_version: 1,
            message_id: format!("parent-agent-message-{}", native_id(None)?),
            sender,
            sender_attempt: attempt,
            sender_generation: generation,
            recipient: target.clone(),
            recipient_attempt: self.agents[&target].attempt.clone(),
            recipient_generation: self.agents[&target].generation,
            delivery: input.delivery(),
            text: input.text,
            status: "admitted".into(),
        };
        if child.phase == NativePhase::Terminal {
            let id = receipt.message_id.clone();
            self.wake_native_subagent(&target, &receipt)?;
            receipt.status = "wake_initial".into();
            let (reply, receiver) = oneshot::channel();
            self.native_message_ingress += 1;
            if let Some(child) = self.native_subagents.get_mut(&target) {
                child.parked.push_back(PendingAdmission {
                    receipt,
                    sender_tool: tool,
                    reply,
                });
            }
            return Ok(Admission::Pending {
                id,
                target,
                reply: receiver,
            });
        }
        if matches!(child.phase, NativePhase::Queued | NativePhase::Preparing) {
            let id = receipt.message_id.clone();
            let (reply, receiver) = oneshot::channel();
            self.native_message_ingress += 1;
            if let Some(child) = self.native_subagents.get_mut(&target) {
                child.parked.push_back(PendingAdmission {
                    receipt,
                    sender_tool: tool,
                    reply,
                });
            }
            return Ok(Admission::Pending {
                id,
                target,
                reply: receiver,
            });
        }
        let outcome = self.commit_native_message(receipt)?;
        Ok(Admission::Immediate(outcome))
    }
}
