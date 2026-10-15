//! One pass over an agent's journal that restores every settled original entry in order,
//! grouped by turn rather than arrival time, and records the compactions it passes.
use super::{provenance::*, restore::*, *};
use crate::event::{ArtifactWrittenEvent, CompactionPatchedEvent, SessionCompactionEvent};
use harness_compaction::{ItemPart, Origin, Stop};

pub(super) struct Recorded<'a> {
    pub seq: u64,
    pub at_ms: Option<u64>,
    pub event: &'a SessionCompactionEvent,
    pub patches: Vec<CompactionPatchedEvent>,
}

pub(super) struct Folded<'a> {
    /// Every original entry, ignoring compactions.
    pub entries: Vec<Entry>,
    pub compactions: Vec<Recorded<'a>>,
    pub tool_ids: HashMap<String, (u64, String)>,
    pub contexts: HashMap<String, &'a ArtifactWrittenEvent>,
}

struct Fold<'a> {
    provenance: Provenance,
    finishes: HashMap<&'a str, Finish<'a>>,
    turns: Vec<Vec<Entry>>,
    users: HashMap<String, usize>,
    providers: HashMap<String, usize>,
    tools: HashMap<String, (usize, usize)>,
    tool_ids: HashMap<String, (u64, String)>,
    started: HashSet<String>,
    cancelled: HashSet<String>,
    legacy_text: HashMap<String, String>,
    request_seqs: Vec<(String, u64)>,
    limited_seqs: HashSet<u64>,
    contexts: HashMap<String, &'a ArtifactWrittenEvent>,
    compactions: Vec<Recorded<'a>>,
    patches: Vec<&'a CompactionPatchedEvent>,
}

pub(super) fn fold<'a>(events: &'a [EventEnvelopeV1], agent: &str, primary: bool) -> Folded<'a> {
    let mut fold = Fold {
        provenance: Provenance::scan(events, agent),
        finishes: finishes(events),
        turns: Vec::new(),
        users: HashMap::new(),
        providers: HashMap::new(),
        tools: HashMap::new(),
        tool_ids: HashMap::new(),
        started: HashSet::new(),
        cancelled: HashSet::new(),
        legacy_text: HashMap::new(),
        request_seqs: Vec::new(),
        limited_seqs: HashSet::new(),
        contexts: HashMap::new(),
        compactions: Vec::new(),
        patches: Vec::new(),
    };
    for event in events.iter().filter(|e| {
        e.actor
            .agent_id
            .as_deref()
            .map_or(primary, |id| id == agent)
    }) {
        fold.apply(event, agent);
    }
    fold.finish()
}

impl<'a> Fold<'a> {
    fn apply(&mut self, event: &'a EventEnvelopeV1, agent: &str) {
        match &event.payload {
            EventV1::UserMessageSubmitted(e) => self.user_message(event, e),
            EventV1::RuntimeReminder(e) => self.reminder(event, e),
            EventV1::PromptAttachmentsSubmitted(e) => {
                if let Some(turn) = self
                    .users
                    .get(e.request_id.as_str())
                    .and_then(|index| self.turns.get_mut(*index))
                    && let Some(user) = turn.first_mut()
                {
                    user.attachments.clone_from(&e.attachments);
                }
            }
            EventV1::ArtifactWritten(e) => {
                if let Some(request) = e.metadata.get("prompt_context") {
                    self.contexts.insert(request.clone(), e);
                }
            }
            EventV1::TaskCancelled(e) if e.task_scope != Some(TaskTerminalScope::ToolCall) => {
                self.cancelled.insert(e.task_id.to_string());
            }
            EventV1::ProviderRequestStarted(e) => self.provider_started(event, e),
            EventV1::ProviderStreamDelta(e) => {
                self.legacy_text
                    .entry(e.request_id.to_string())
                    .or_default()
                    .push_str(&e.delta);
            }
            EventV1::AssistantMessageFinished(e) => self.assistant(event, e),
            EventV1::ToolCallFinished(e) => self.tool_finished(event, e),
            EventV1::BranchSummary(e) if e.agent_id == agent => self.branch(event, e),
            EventV1::SessionCompaction(e) if e.agent_id == agent => {
                if e.trigger_reason == "incomplete" {
                    let index = event
                        .correlation_id
                        .as_ref()
                        .and_then(|id| self.users.get(id))
                        .copied();
                    drop_recovered_truncation(&mut self.turns, index, &self.limited_seqs);
                }
                self.compactions.push(Recorded {
                    seq: event.seq,
                    at_ms: event_time(event),
                    event: e,
                    patches: Vec::new(),
                });
            }
            EventV1::CompactionPatched(e) if e.agent_id == agent => self.patches.push(e),
            _ => {}
        }
    }

    fn open_turn(&mut self, id: &str, seq: u64, entry: Entry) {
        self.users.insert(id.to_owned(), self.turns.len());
        self.request_seqs.push((id.to_owned(), seq));
        self.turns.push(vec![entry]);
    }

    fn user_message(
        &mut self,
        event: &EventEnvelopeV1,
        e: &crate::event::UserMessageSubmittedEvent,
    ) {
        let (origin, note) = self.provenance.submitted(event, e.request_id.as_str());
        // Messages delivered into a running turn correlate to that turn, not themselves.
        let running = event
            .correlation_id
            .as_deref()
            .filter(|turn| *turn != e.request_id.as_str())
            .and_then(|turn| self.users.get(turn))
            .copied();
        if let Some(index) = running {
            let turn = self.request_seqs[index].0.clone();
            self.turns[index].push(user_entry(&e.text, event, turn, origin, note));
            return;
        }
        let id = e.request_id.to_string();
        let entry = user_entry(&e.text, event, id.clone(), origin, note);
        self.open_turn(&id, event.seq, entry);
    }

    fn reminder(&mut self, event: &EventEnvelopeV1, e: &crate::event::RuntimeReminderEvent) {
        let id = e.request_id.to_string();
        if let Some(index) = self.users.get(&id).copied() {
            let turn = self.request_seqs[index].0.clone();
            let note = Some(reminder_note(e.kind, false));
            self.turns[index].push(user_entry(&e.text, event, turn, Origin::Host, note));
        } else {
            // A reminder opens a wake turn that has no submitted user message.
            let note = Some(reminder_note(e.kind, true));
            let entry = user_entry(&e.text, event, id.clone(), Origin::Host, note);
            self.open_turn(&id, event.seq, entry);
        }
    }

    fn provider_started(
        &mut self,
        event: &EventEnvelopeV1,
        e: &crate::event::ProviderRequestStartedEvent,
    ) {
        let turn = e
            .metadata
            .as_ref()
            .and_then(|m| m.turn_id.as_ref())
            .or(event.correlation_id.as_ref())
            .and_then(|id| self.users.get(id))
            .copied()
            .or_else(|| self.turns.len().checked_sub(1));
        if let Some(turn) = turn {
            if let Some((id, _)) = self.request_seqs.get(turn) {
                self.started.insert(id.clone());
            }
            self.providers.insert(e.request_id.to_string(), turn);
        }
    }

    fn assistant(
        &mut self,
        event: &EventEnvelopeV1,
        e: &crate::event::AssistantMessageFinishedEvent,
    ) {
        let Some(index) = self
            .providers
            .get(e.request_id.as_str())
            .copied()
            .or_else(|| {
                event
                    .correlation_id
                    .as_ref()
                    .and_then(|id| self.users.get(id))
                    .copied()
            })
        else {
            return;
        };
        let turn = self.request_seqs[index].0.clone();
        let mut message = CompletionMessage::text(MessageRole::Assistant, "");
        let (mut calls, mut results, mut parts) = (Vec::new(), Vec::new(), Vec::new());
        for part in &e.parts {
            match part {
                crate::session::AssistantPart::Text { text } => {
                    message.content.push_str(text);
                    parts.push(ItemPart::text(text.clone()));
                }
                crate::session::AssistantPart::Reasoning { text } => {
                    parts.push(ItemPart::reasoning(text.clone()));
                }
                crate::session::AssistantPart::ToolCall(call) => {
                    let provider_id = call
                        .provider_tool_call_id
                        .clone()
                        .unwrap_or_else(|| call.tool_call_id.to_string());
                    self.tool_ids.insert(
                        call.tool_call_id.to_string(),
                        (event.seq, provider_id.clone()),
                    );
                    self.tools.insert(
                        call.tool_call_id.to_string(),
                        (index, self.turns[index].len() + 1 + results.len()),
                    );
                    // Reserve call order, including an explicit unknown result for
                    // interrupted tools.
                    let mut result = Entry::new(
                        CompletionMessage {
                            continuation: None,
                            role: MessageRole::Tool,
                            content: INTERRUPTED_TOOL_RESULT.into(),
                            tool_call_id: Some(provider_id.clone()),
                            name: Some(call.tool_id.clone()),
                            assistant_tool_calls: None,
                        },
                        event.seq,
                        Some(turn.clone()),
                    );
                    result.at_ms = event_time(event);
                    results.push(result);
                    parts.push(ItemPart::Call(calls.len()));
                    calls.push(AssistantToolCall {
                        tool_call_id: provider_id,
                        function_name: call.tool_id.clone(),
                        arguments_json: replayed_arguments(&call.args_summary),
                    });
                }
            }
        }
        if e.parts.is_empty() {
            message.content = self
                .legacy_text
                .remove(e.request_id.as_str())
                .unwrap_or_default();
        }
        let finish = self.finishes.get(e.request_id.as_str());
        let stop = finish.map_or(Stop::Complete, |finish| finish.stop);
        // A response that stopped at the output limit or failed and delivered nothing is
        // discarded; the retry regenerates it.
        if message.content.is_empty() && calls.is_empty() && stop != Stop::Complete {
            return;
        }
        if stop == Stop::Truncated && calls.is_empty() {
            self.limited_seqs.insert(event.seq);
        }
        message.assistant_tool_calls = (!calls.is_empty()).then_some(calls);
        let mut entry = Entry::new(message, event.seq, Some(turn));
        entry.settled_reasoning = assistant_reasoning(&e.parts);
        entry.usage = finish.and_then(|finish| finish.usage.cloned());
        entry.at_ms = event_time(event);
        entry.stop = stop;
        entry.parts = parts;
        self.turns[index].push(entry);
        self.turns[index].extend(results);
    }

    fn tool_finished(&mut self, event: &EventEnvelopeV1, e: &crate::event::ToolCallFinishedEvent) {
        let Some((turn, result)) = self.tools.remove(e.tool_call_id.as_str()) else {
            return;
        };
        let entry = &mut self.turns[turn][result];
        entry.at_ms = event_time(event).or(entry.at_ms);
        entry.tool =
            harness_compaction::prune::tool_facts(harness_compaction::prune::ResultFacts {
                tool: entry.message.name.as_deref().unwrap_or_default(),
                is_error: e.status == ToolCallStatus::Failed,
                useless: e.metadata.as_ref().is_some_and(|m| m.useless),
                source_artifact: e.metadata.as_ref().is_some_and(|m| m.source_artifact),
                structured: e.output_json.as_ref(),
            });
        entry.attachments = e
            .metadata
            .as_ref()
            .map(|m| m.attachments.clone())
            .unwrap_or_default();
        let summary = e.output_summary.clone().unwrap_or_else(|| match e.status {
            ToolCallStatus::Succeeded => "Tool completed.".into(),
            ToolCallStatus::Failed => "Tool execution failed.".into(),
        });
        entry.message.content = crate::tool::provider_text(
            entry.message.name.as_deref().unwrap_or_default(),
            summary,
            e.output_json.clone(),
        );
    }

    fn branch(&mut self, event: &EventEnvelopeV1, e: &crate::event::BranchSummaryEvent) {
        // An empty branch summary contributes nothing.
        if e.summary.trim().is_empty() {
            return;
        }
        let mut entry = Entry::new(
            CompletionMessage::text(
                MessageRole::User,
                harness_compaction::prompts::branch_summary_message(&e.summary),
            ),
            event.seq,
            None,
        );
        entry.branch_summary = Some(std::sync::Arc::new(e.clone()));
        entry.at_ms = event_time(event);
        entry.origin = Origin::Host;
        let id = format!("branch:{}", event.seq);
        self.request_seqs.push((id, event.seq));
        self.turns.push(vec![entry]);
    }

    fn finish(mut self) -> Folded<'a> {
        for patch in self.patches {
            if let Some(recorded) = self
                .compactions
                .iter_mut()
                .find(|recorded| recorded.seq == patch.compaction_seq)
            {
                recorded.patches.push(patch.clone());
            }
        }
        let mut entries = Vec::new();
        for ((id, _), turn) in self.request_seqs.into_iter().zip(self.turns) {
            if self.cancelled.contains(&id) && !self.started.contains(&id) {
                continue;
            }
            entries.extend(turn);
        }
        Folded {
            entries,
            compactions: self.compactions,
            tool_ids: self.tool_ids,
            contexts: self.contexts,
        }
    }
}
