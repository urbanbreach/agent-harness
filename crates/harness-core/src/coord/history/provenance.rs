//! Who authored a replayed message, from the journal facts that record it.
use super::*;
use harness_compaction::{NoteKind, Origin};

/// Journal facts that tell typed input from agent steering and harness notes.
pub(super) struct Provenance {
    parent: Option<String>,
    /// Delivered peer messages by message id, with their sender.
    peers: HashMap<String, String>,
    /// Requests whose child completions were handed to the agent.
    completions: HashSet<String>,
}

impl Provenance {
    pub(super) fn scan(events: &[EventEnvelopeV1], agent: &str) -> Self {
        let mut found = Self {
            parent: None,
            peers: HashMap::new(),
            completions: HashSet::new(),
        };
        for event in events {
            match &event.payload {
                EventV1::AgentSpawned(spawn) if spawn.agent_id == agent => {
                    found.parent.clone_from(&spawn.parent_agent_id);
                }
                EventV1::NativeSubagentMessage(receipt)
                    if receipt.recipient == agent && receipt.status == "delivered" =>
                {
                    found
                        .peers
                        .insert(receipt.message_id.clone(), receipt.sender.clone());
                }
                EventV1::NativeSubagentReceipt(receipt)
                    if receipt.kind == "completion_reminder_delivered" =>
                {
                    found.completions.extend(event.correlation_id.clone());
                }
                _ => {}
            }
        }
        found
    }

    /// Origin and note kind of a submitted message. Typed input is `User`; a parent's
    /// steering is `Agent`; peer messages, completion digests and wakes are harness notes.
    pub(super) fn submitted(
        &self,
        event: &EventEnvelopeV1,
        request_id: &str,
    ) -> (Origin, Option<NoteKind>) {
        if let Some(sender) = self.peers.get(request_id) {
            return if self.parent.as_deref() == Some(sender.as_str()) {
                (Origin::Agent, None)
            } else {
                (Origin::Host, Some(NoteKind::Peer))
            };
        }
        match event.actor.kind {
            ActorKind::User | ActorKind::Supervisor => (Origin::User, None),
            _ if self.completed(event, request_id) => {
                (Origin::Host, Some(NoteKind::ChildCompletion))
            }
            _ if self.parent.is_some() => (Origin::Agent, None),
            _ => (Origin::Host, Some(NoteKind::Wake)),
        }
    }

    fn completed(&self, event: &EventEnvelopeV1, request_id: &str) -> bool {
        self.completions.contains(request_id)
            || event
                .correlation_id
                .as_ref()
                .is_some_and(|id| self.completions.contains(id))
    }
}

/// The note kind of a reminder; one that opens a turn on its own is a wake.
pub(super) fn reminder_note(kind: RuntimeReminderKind, opens_turn: bool) -> NoteKind {
    match kind {
        RuntimeReminderKind::CompactionResume => NoteKind::CompactionResume,
        RuntimeReminderKind::LengthStop => NoteKind::LengthStop,
        _ if opens_turn => NoteKind::Wake,
        _ => NoteKind::Reminder,
    }
}
