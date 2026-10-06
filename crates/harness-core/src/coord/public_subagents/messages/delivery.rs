use super::*;

impl Runtime {
    pub(super) fn commit_native_message(
        &mut self,
        mut receipt: NativeMessageReceipt,
    ) -> Result<SendSubagentMessageResult, CoordinatorError> {
        let target = receipt.recipient.clone();
        receipt.recipient_attempt = self.agents[&target].attempt.clone();
        receipt.recipient_generation = self.agents[&target].generation;
        let message_id = receipt.message_id.clone();
        let queue =
            receipt.delivery == SendSubagentMessageDelivery::Queue || !self.agents[&target].busy;
        super::super::prompt::Prompt {
            text: message_text(&receipt),
            native_subagent: true,
            ..Default::default()
        }
        .validate(self.redactor.as_ref())?;
        if queue {
            self.check_turn_capacity(&target)?;
        }
        let accepted = receipt.clone();
        let owner = target.clone();
        // The owning append is the persistence barrier. No visible steering,
        // queue insertion or acknowledgement can precede it.
        self.emit_applied(
            EventActor::new(ActorKind::Worker, Some(receipt.sender.clone())),
            Some(message_id.clone()),
            EventV1::NativeSubagentMessage(Box::new(receipt.clone())),
            move |runtime, _| {
                if !queue && let Some(child) = runtime.native_subagents.get_mut(&owner) {
                    child.messages.push_back(accepted);
                }
            },
        )?;
        if queue {
            receipt.status = "delivered".into();
            self.emit(
                EventActor::new(ActorKind::Worker, Some(target.clone())),
                Some(message_id.clone()),
                EventV1::NativeSubagentMessage(Box::new(receipt.clone())),
            )?;
            self.queue_turn(
                EventActor::new(ActorKind::Worker, Some(target.clone())),
                &target,
                super::super::prompt::Prompt {
                    text: message_text(&receipt),
                    native_subagent: true,
                    ..Default::default()
                },
                None,
                None,
                None,
            )?;
        } else if receipt.delivery == SendSubagentMessageDelivery::Interject
            && let Some(child) = self.native_subagents.get_mut(&target)
        {
            child
                .wait_interrupt
                .send_modify(|epoch| *epoch = epoch.saturating_add(1));
        }
        Ok(SendSubagentMessageResult::Accepted { message_id })
    }

    pub(in crate::coord::public_subagents) fn promote_native_message_admissions(
        &mut self,
        target: &str,
    ) -> Result<(), CoordinatorError> {
        let parked = self
            .native_subagents
            .get_mut(target)
            .map(|child| std::mem::take(&mut child.parked))
            .unwrap_or_default();
        for pending in parked {
            self.native_message_ingress = self.native_message_ingress.saturating_sub(1);
            let receipt = pending.receipt;
            if !self.verify_native_message_sender(
                &receipt.sender,
                &pending.sender_tool,
                receipt.sender_attempt.as_deref(),
                receipt.sender_generation,
            ) {
                let _ = pending
                    .reply
                    .send(SendSubagentMessageResult::NotActiveOrFinalizing);
                continue;
            }
            let outcome = if receipt.status == "wake_initial" {
                let mut receipt = receipt;
                receipt.status = "delivered".into();
                receipt.recipient_attempt = self.agents[target].attempt.clone();
                receipt.recipient_generation = self.agents[target].generation;
                let message_id = receipt.message_id.clone();
                self.emit(
                    EventActor::new(ActorKind::Worker, Some(target.into())),
                    Some(message_id.clone()),
                    EventV1::NativeSubagentMessage(Box::new(receipt)),
                )?;
                SendSubagentMessageResult::Accepted { message_id }
            } else {
                self.commit_native_message(receipt)?
            };
            let _ = pending.reply.send(outcome);
        }
        Ok(())
    }

    pub(in crate::coord::public_subagents) fn drain_native_subagent_messages(
        &mut self,
        target: &str,
        request: &str,
    ) -> Result<Vec<(u64, String)>, CoordinatorError> {
        if !self.native_subagents.contains_key(target) {
            return Ok(Vec::new());
        }
        let Some(child) = self.native_subagents.get_mut(target) else {
            return Ok(Vec::new());
        };
        if child.request.as_deref() != Some(request)
            || child.phase != NativePhase::Running
            || child.cancellation.is_cancelled()
        {
            return Ok(Vec::new());
        }
        let pending = std::mem::take(&mut child.messages);
        let mut interject = Vec::new();
        let mut steer = Vec::new();
        for receipt in pending {
            if receipt.delivery == SendSubagentMessageDelivery::Interject {
                interject.push(receipt);
            } else {
                steer.push(receipt);
            }
        }
        let mut delivered = Vec::new();
        for mut receipt in interject.into_iter().chain(steer) {
            receipt.status = "delivered".into();
            self.emit(
                EventActor::new(ActorKind::Worker, Some(target.into())),
                Some(request.into()),
                EventV1::NativeSubagentMessage(Box::new(receipt.clone())),
            )?;
            let text = message_text(&receipt);
            let event = self.emit(
                EventActor::new(ActorKind::Worker, Some(target.into())),
                Some(request.into()),
                EventV1::UserMessageSubmitted(UserMessageSubmittedEvent {
                    request_id: receipt.message_id.into(),
                    text: text.clone(),
                }),
            )?;
            delivered.push((event.seq, text));
        }
        Ok(delivered)
    }

    pub(super) fn wake_native_subagent(
        &mut self,
        target: &str,
        receipt: &NativeMessageReceipt,
    ) -> Result<(), CoordinatorError> {
        if self.is_eval_pool_child(target) {
            return Err(native_invalid(
                "workpool children cannot be restarted; push a new item instead".into(),
            ));
        }
        let FinalizedStateResult::Available { state } =
            self.resolve_agent_finalized_state(target)?
        else {
            return Err(native_invalid(format!(
                "Cannot reactivate subagent '{target}': persisted session state is unavailable."
            )));
        };
        let source_reference = self.agents[target].finalized.clone();
        let child = self
            .native_subagents
            .get_mut(target)
            .ok_or_else(|| CoordinatorError::UnknownAgent(target.into()))?;
        child.displaced = Some(NativeDisplaced {
            registration: child.registration.clone(),
            snapshot: child.updates.borrow().clone(),
            request: child.request.clone(),
            started_ms: child.started_ms,
            started: child.started.clone(),
            ended: child.ended.clone(),
        });
        child.registration.prompt = message_text(receipt);
        child.registration.source = Some(target.into());
        child.registration.background = true;
        child.registration.parent_request = None;
        child.source_state = Some(state);
        child.source_reference = source_reference;
        child.cancellation = CancellationToken::new();
        child.foreground_attached = false;
        child.consumed = true;
        child.buffered_for = None;
        child.phase = NativePhase::Queued;
        child.request = None;
        child.started_ms = self.clock.mono_ms();
        child.started = native_timestamp(self.clock.as_ref());
        child.ended = None;
        child.terminal_published = false;
        child.updates.send_modify(|snapshot| {
            snapshot.terminal = false;
            snapshot.completed = None;
            snapshot.error = None;
            snapshot.result.status = "initializing".into();
            snapshot.result.output =
                "Subagent is initializing (creating worktree, resolving config).".into();
            snapshot.result.exit_code = None;
            snapshot.result.ended = None;
        });
        self.native_subagent_queue.push_back(target.into());
        self.pump_native_subagents()
    }
}
