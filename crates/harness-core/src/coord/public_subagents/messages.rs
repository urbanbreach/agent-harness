use super::*;

mod admission;
mod delivery;

pub(super) struct PendingAdmission {
    pub(super) receipt: NativeMessageReceipt,
    sender_tool: String,
    pub(super) reply: oneshot::Sender<SendSubagentMessageResult>,
}

enum Admission {
    Immediate(SendSubagentMessageResult),
    Pending {
        id: String,
        target: String,
        reply: oneshot::Receiver<SendSubagentMessageResult>,
    },
}

impl CoordinatorHandle {
    pub async fn send_subagent_message(
        &self,
        actor: EventActor,
        tool_call_id: String,
        input: SendSubagentMessageInput,
    ) -> Result<SendSubagentMessageResult, CoordinatorError> {
        let admitted = tokio::time::timeout(
            Duration::from_secs(5),
            self.call(move |s| s.admit_native_message(actor, tool_call_id, input)),
        )
        .await;
        let admission = match admitted {
            Ok(Ok(admission)) => admission,
            Ok(Err(CoordinatorError::Closed)) => {
                return Ok(SendSubagentMessageResult::ChannelClosed)
            }
            Ok(Err(error)) => return Err(error),
            // Actor admission may have committed. A lost confirmation cannot
            // be described as a definite rejection.
            Err(_) => return Ok(SendSubagentMessageResult::AdmissionUncertain),
        };
        let (id, target, reply) = match admission {
            Admission::Immediate(outcome) => return Ok(outcome),
            Admission::Pending { id, target, reply } => (id, target, reply),
        };
        match tokio::time::timeout(Duration::from_secs(5), reply).await {
            Ok(Ok(outcome)) => Ok(outcome),
            Ok(Err(_)) => Ok(SendSubagentMessageResult::ChannelClosed),
            Err(_) => {
                self.call(move |s| {
                    let Some(child) = s.native_subagents.get_mut(&target) else {
                        return Ok(SendSubagentMessageResult::NotAcceptedBeforeDeadline);
                    };
                    let Some(index) = child
                        .parked
                        .iter()
                        .position(|pending| pending.receipt.message_id == id)
                    else {
                        return Ok(SendSubagentMessageResult::AdmissionUncertain);
                    };
                    child.parked.remove(index);
                    s.native_message_ingress = s.native_message_ingress.saturating_sub(1);
                    Ok(SendSubagentMessageResult::NotAcceptedBeforeDeadline)
                })
                .await
            }
        }
    }

    /// The actual provider worker calls this between tool completion and its
    /// next inference request. Identity and attempt are checked by the actor.
    pub async fn drain_subagent_messages(
        &self,
        actor: EventActor,
        request_id: String,
    ) -> Result<Vec<(u64, String)>, CoordinatorError> {
        self.call(move |s| {
            s.check_task(&request_id)?;
            let job = &s.running[&request_id];
            if job.actor != actor || !matches!(job.kind, JobKind::Turn { .. }) {
                return Err(CoordinatorError::PermissionDenied(
                    "message drain requires the current worker".into(),
                ));
            }
            let agent = actor.agent_id.as_deref().ok_or_else(|| {
                CoordinatorError::PermissionDenied("message drain requires an agent".into())
            })?;
            s.drain_native_subagent_messages(agent, &request_id)
        })
        .await
    }
}

fn message_text(receipt: &NativeMessageReceipt) -> String {
    format!(
        "<agent_message sender=\"{}\">\n{}\n</agent_message>",
        receipt.sender, receipt.text
    )
}
