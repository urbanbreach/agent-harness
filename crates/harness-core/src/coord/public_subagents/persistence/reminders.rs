use super::*;

impl Runtime {
    pub(in crate::coord) fn claim_native_completion_wake(
        &mut self,
        parent: &str,
        completion: &str,
        request: &str,
    ) -> Result<bool, CoordinatorError> {
        let child = self.native_subagents.iter().find(|(_, child)| {
            child.registration.spawner == parent && child.request.as_deref() == Some(completion)
        });
        let Some((id, child)) = child else {
            return Ok(true);
        };
        if child.consumed {
            return Ok(false);
        }
        let id = id.clone();
        self.record_native_completion_delivery(&id, parent, request)?;
        Ok(true)
    }

    fn record_native_completion_delivery(
        &mut self,
        id: &str,
        parent: &str,
        request: &str,
    ) -> Result<u64, CoordinatorError> {
        let event = self.emit(
            EventActor::new(ActorKind::Worker, Some(parent.into())),
            Some(request.into()),
            EventV1::NativeSubagentReceipt(Box::new(NativeSubagentReceipt {
                payload_version: 1,
                child_id: id.into(),
                attempt_id: self.agents[id].attempt.clone(),
                generation: self.agents[id].generation,
                kind: "completion_reminder_delivered".into(),
                waiter_id: Some(request.into()),
            })),
        )?;
        if let Some(child) = self.native_subagents.get_mut(id) {
            child.buffered_for = None;
            child.consumed = true;
        }
        Ok(event.seq)
    }

    pub(in crate::coord::public_subagents) fn drain_native_completion_reminders(
        &mut self,
        parent: &str,
        request: &str,
    ) -> Result<Vec<(u64, String)>, CoordinatorError> {
        let mut buffered: Vec<_> = self
            .native_subagents
            .iter()
            .filter(|(_, child)| child.buffered_for.as_deref() == Some(parent))
            .map(|(id, child)| (child.completion_age, id.clone()))
            .collect();
        buffered.sort();
        let mut messages = Vec::new();
        for (_, id) in buffered {
            let child = &self.native_subagents[&id];
            let snapshot = child.updates.borrow();
            let text = format!(
                "<subagent_completion>\nsubagent_id: {id}\nstatus: {}\n{}\n</subagent_completion>",
                snapshot.result.status, snapshot.result.output
            );
            drop(snapshot);
            let seq = self.record_native_completion_delivery(&id, parent, request)?;
            messages.push((seq, text));
        }
        Ok(messages)
    }
}
