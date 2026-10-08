use super::{Metrics, Session};
use harness_core::{
    event::{EventEnvelopeV1, EventV1, TaskTerminalScope, ToolCallStatus},
    proj::TodoProjection,
};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Default)]
struct Turn<'a> {
    model: &'a str,
    provider: &'a str,
    models: BTreeSet<&'a str>,
    last_call: Option<(&'a str, &'a str)>,
    repeat: u64,
    last_edit: u64,
    last_check: u64,
    terminal: bool,
    previous_error: bool,
}

struct Call<'a> {
    turn: &'a str,
    tool: &'a str,
    seq: u64,
}

fn turn_id(event: &EventEnvelopeV1) -> Option<&str> {
    match &event.payload {
        EventV1::ProviderRequestStarted(e) => e
            .metadata
            .as_ref()
            .and_then(|m| m.turn_id.as_deref())
            .or(event.correlation_id.as_deref()),
        EventV1::RuntimeReminder(e) => Some(e.request_id.as_str()),
        EventV1::TaskCompleted(e)
            if e.metadata.as_ref().and_then(|m| m.task_scope)
                == Some(TaskTerminalScope::AgentTurn) =>
        {
            Some(e.task_id.as_str())
        }
        EventV1::TaskCancelled(e) if e.task_scope == Some(TaskTerminalScope::AgentTurn) => {
            Some(e.task_id.as_str())
        }
        _ => None,
    }
}

#[derive(Default)]
struct CensusFold<'a> {
    turns: BTreeMap<&'a str, Turn<'a>>,
    by_model: BTreeMap<&'a str, Metrics>,
    calls: BTreeMap<&'a str, Call<'a>>,
    requests: BTreeMap<&'a str, (&'a str, &'a str)>,
    active: BTreeMap<&'a str, &'a str>,
    todos: TodoProjection,
}

impl<'a> CensusFold<'a> {
    fn discover_turns(&mut self, events: &'a [EventEnvelopeV1]) {
        // Discover wake turns and their first model before folding preceding reminders.
        for event in events {
            let Some(id) = turn_id(event) else { continue };
            let turn = self.turns.entry(id).or_default();
            if let EventV1::ProviderRequestStarted(e) = &event.payload
                && turn.model.is_empty()
            {
                turn.model = &e.model_id;
                turn.provider = &e.provider_id;
            }
        }
    }

    fn owner(&self, event: &'a EventEnvelopeV1) -> Option<&'a str> {
        let actor = event.actor.agent_id.as_deref().unwrap_or("");
        turn_id(event)
            .or_else(|| {
                event
                    .correlation_id
                    .as_deref()
                    .filter(|id| self.turns.contains_key(id))
            })
            .or_else(|| {
                event
                    .correlation_id
                    .as_deref()
                    .and_then(|id| self.calls.get(id).map(|c| c.turn))
            })
            .or_else(|| self.active.get(actor).copied())
    }

    fn apply(&mut self, event: &'a EventEnvelopeV1) {
        self.todos.apply(event);
        let owner = self.owner(event);
        let model = owner
            .and_then(|id| self.turns.get(id))
            .map_or("", |turn| turn.model);
        match &event.payload {
            EventV1::ProviderRequestStarted(e) => self.start_request(event, e, owner),
            EventV1::ProviderRequestFinished(e) => self.finish_request(e),
            EventV1::AgentSpawned(e) if e.parent_agent_id.is_some() => self.spawn(e),
            EventV1::ToolCallRequested(e) => self.request_tool(event, e, owner, model),
            EventV1::ToolCallFinished(e) | EventV1::EvalCellFinished(e)
                if e.status == ToolCallStatus::Succeeded =>
            {
                self.finish_tool(event, e)
            }
            EventV1::RuntimeReminder(e) => {
                increment(
                    &mut self
                        .by_model
                        .entry(model)
                        .or_default()
                        .runtime_reminders_by_kind,
                    e.kind.as_str(),
                );
            }
            EventV1::SessionCompaction(_) | EventV1::CompactionApplied(_) => {
                self.by_model.entry(model).or_default().compactions += 1;
            }
            EventV1::TaskCompleted(_) | EventV1::TaskCancelled(_) if turn_id(event).is_some() => {
                self.finish_turn(event, owner, model);
                let actor = event.actor.agent_id.as_deref().unwrap_or("");
                if self.active.get(actor).copied() == owner {
                    self.active.remove(actor);
                }
            }
            _ => {}
        }
    }

    fn start_request(
        &mut self,
        event: &'a EventEnvelopeV1,
        request: &'a harness_core::event::ProviderRequestStartedEvent,
        owner: Option<&'a str>,
    ) {
        let Some(id) = owner else { return };
        self.active
            .insert(event.actor.agent_id.as_deref().unwrap_or(""), id);
        let turn = self.turns.entry(id).or_default();
        let metrics = self.by_model.entry(&request.model_id).or_default();
        if turn.previous_error
            && (turn.model != request.model_id || turn.provider != request.provider_id)
        {
            metrics.provider_fallbacks += 1;
        }
        turn.previous_error = false;
        turn.model = &request.model_id;
        turn.provider = &request.provider_id;
        turn.models.insert(&request.model_id);
        self.requests
            .insert(request.request_id.as_str(), (id, &request.model_id));
        metrics.provider_requests += 1;
    }

    fn finish_request(&mut self, request: &harness_core::event::ProviderRequestFinishedEvent) {
        let Some(&(id, model)) = self.requests.get(request.request_id.as_str()) else {
            return;
        };
        let error = request.finish_reason == "error";
        let metrics = self.by_model.entry(model).or_default();
        if error {
            metrics.provider_errors += 1;
        }
        if let Some(turn) = self.turns.get_mut(id) {
            turn.previous_error = error;
        }
        metrics.compactions += request
            .metadata
            .as_ref()
            .map_or(0, |m| m.native_compactions.len() as u64);
    }

    fn spawn(&mut self, event: &harness_core::event::AgentSpawnedEvent) {
        let model = event
            .parent_agent_id
            .as_deref()
            .and_then(|parent| self.active.get(parent))
            .and_then(|id| self.turns.get(id))
            .map_or("", |turn| turn.model);
        self.by_model.entry(model).or_default().subagent_spawns += 1;
    }

    fn request_tool(
        &mut self,
        event: &'a EventEnvelopeV1,
        request: &'a harness_core::event::ToolCallRequestedEvent,
        owner: Option<&'a str>,
        model: &'a str,
    ) {
        let metrics = self.by_model.entry(model).or_default();
        metrics.tool_calls += 1;
        increment(&mut metrics.tool_calls_by_tool, &request.tool_id);
        if request.tool_id == "eval" {
            metrics.eval_calls += 1;
        } else {
            metrics.direct_tool_calls += 1;
        }
        let Some(id) = owner else { return };
        self.calls.insert(
            request.tool_call_id.as_str(),
            Call {
                turn: id,
                tool: &request.tool_id,
                seq: event.seq,
            },
        );
        let Some(turn) = self.turns.get_mut(id) else {
            return;
        };
        let key = (request.tool_id.as_str(), request.args_digest.as_str());
        turn.repeat = if turn.last_call == Some(key) {
            turn.repeat + 1
        } else {
            1
        };
        turn.last_call = Some(key);
        metrics.longest_identical_tool_run = metrics.longest_identical_tool_run.max(turn.repeat);
        if turn.repeat == 3 {
            metrics.identical_tool_runs_ge_3 += 1;
        }
    }

    fn finish_tool(
        &mut self,
        event: &EventEnvelopeV1,
        result: &harness_core::event::ToolCallFinishedEvent,
    ) {
        let Some(call) = self.calls.get(result.tool_call_id.as_str()) else {
            return;
        };
        let Some(turn) = self.turns.get_mut(call.turn) else {
            return;
        };
        if matches!(
            call.tool,
            "edit" | "write" | "apply_patch" | "ast_grep_replace"
        ) {
            turn.last_edit = event.seq;
        }
        if matches!(call.tool, "bash" | "eval" | "lsp") {
            turn.last_check = turn.last_check.max(call.seq);
        }
    }

    fn finish_turn(&mut self, event: &EventEnvelopeV1, owner: Option<&str>, model: &'a str) {
        let Some(id) = owner else { return };
        let Some(turn) = self.turns.get_mut(id) else {
            return;
        };
        if turn.terminal {
            return;
        }
        turn.terminal = true;
        let metrics = self.by_model.entry(model).or_default();
        if self.todos.has_open() {
            metrics.open_todo_turns += 1;
        }
        if turn.last_edit > turn.last_check {
            metrics.unverified_edit_turns += 1;
        }
        if let Some(kind) = failure_kind(event) {
            increment(&mut metrics.turn_failures_by_kind, kind);
        }
    }

    fn finish(mut self, run_id: String, events: &[EventEnvelopeV1]) -> Session {
        for turn in self.turns.values() {
            if turn.models.is_empty() {
                self.by_model.entry("").or_default().turns += 1;
                continue;
            }
            for model in &turn.models {
                self.by_model.entry(model).or_default().turns += 1;
            }
        }
        let mut metrics = Metrics::default();
        for row in self.by_model.values_mut() {
            row.update_eval_share();
            metrics.merge(row);
        }
        // A fallback turn participates in several models but is still one session turn.
        metrics.turns = self.turns.len() as u64;
        let models = self
            .by_model
            .keys()
            .filter(|model| !model.is_empty())
            .map(|model| (*model).to_owned())
            .collect();
        let by_model = self
            .by_model
            .into_iter()
            .map(|(model, row)| (model.to_owned(), row))
            .collect();
        let latest = events
            .iter()
            .filter_map(|e| e.ts.as_deref())
            .filter_map(|ts| humantime::parse_rfc3339(ts).ok())
            .max();
        Session {
            run_id,
            models,
            metrics,
            by_model,
            latest,
        }
    }
}

fn increment(counts: &mut BTreeMap<String, u64>, key: &str) {
    if let Some(count) = counts.get_mut(key) {
        *count += 1;
    } else {
        counts.insert(key.to_owned(), 1);
    }
}

fn failure_kind(event: &EventEnvelopeV1) -> Option<&'static str> {
    let EventV1::TaskCancelled(cancelled) = &event.payload else {
        return None;
    };
    if !cancelled.failure {
        return None;
    }
    [
        ("iteration limit", "iteration_limit"),
        ("loop guard", "loop_guard"),
        ("stream guard", "stream_guard"),
    ]
    .into_iter()
    .find_map(|(text, kind)| cancelled.reason.contains(text).then_some(kind))
}

pub(super) fn project(run_id: String, events: &[EventEnvelopeV1]) -> Session {
    let mut state = CensusFold::default();
    state.discover_turns(events);
    for event in events {
        state.apply(event);
    }
    state.finish(run_id, events)
}
