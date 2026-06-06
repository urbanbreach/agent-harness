use std::env;
use std::io::Write;
use std::sync::Arc;
use std::time::{Duration, Instant};

use harness_core::event::{EventEnvelopeV1, EventV1, TaskCancelledEvent, TaskCompletedEvent};
use harness_core::store::{EventStore, EventStoreError};

use super::PromptOutputFormat;

pub(super) const DEFAULT_WAIT_TIMEOUT: Duration = Duration::from_secs(30);
const WAIT_TIMEOUT_ENV: &str = "HARNESS_PROMPT_WAIT_TIMEOUT_MS";
const PROVIDER_ERROR_REASON_GRACE: Duration = Duration::from_secs(2);

#[cfg(test)]
pub(super) async fn wait_for_prompt_completion(
    event_store: Arc<dyn EventStore>,
    request_id: &str,
    timeout: Duration,
) -> Result<(), String> {
    let mut output = Vec::new();
    wait_for_prompt_completion_with_output(
        event_store,
        request_id,
        timeout,
        false,
        PromptOutputFormat::Default,
        &mut output,
    )
    .await
}

pub(super) async fn wait_for_prompt_completion_with_output<W: Write + ?Sized>(
    event_store: Arc<dyn EventStore>,
    request_id: &str,
    timeout: Duration,
    show_thinking: bool,
    format: PromptOutputFormat,
    stdout: &mut W,
) -> Result<(), String> {
    let deadline = Instant::now() + timeout;
    let mut tracker = PromptCompletionTracker::new(request_id);
    let mut printer = PromptStreamPrinter::new(show_thinking, format, stdout);
    let mut next_seq = 1;
    let mut stream = event_store
        .subscribe(next_seq)
        .map_err(|err| format!("failed to subscribe to prompt event stream: {err}"))?;

    loop {
        let wait_until = tracker.next_wait_deadline(deadline);
        let wait_duration = wait_until.saturating_duration_since(Instant::now());

        match tokio::time::timeout(
            wait_duration,
            std::future::poll_fn(|cx| stream.as_mut().poll_next(cx)),
        )
        .await
        {
            Ok(Some(Ok(event))) => {
                next_seq = event.seq.saturating_add(1);
                printer.observe(&event, request_id);
                match tracker.observe(&event) {
                    PromptCompletionStatus::Continue => {}
                    PromptCompletionStatus::Completed => {
                        printer.finish();
                        return Ok(());
                    }
                    PromptCompletionStatus::Failed(error) => {
                        printer.finish();
                        return Err(error);
                    }
                }
            }
            Ok(Some(Err(EventStoreError::SubscriberLagged(_)))) => {
                stream = event_store.subscribe(next_seq).map_err(|err| {
                    format!("failed to resubscribe to prompt event stream: {err}")
                })?;
            }
            Ok(Some(Err(err))) => {
                printer.finish();
                return Err(format!("prompt event stream error: {err}"));
            }
            Ok(None) => {
                printer.finish();
                return Err(format!(
                    "prompt event stream closed before completion for {request_id}"
                ));
            }
            Err(_) => {}
        }

        if let Some(error) = tracker.provider_error_timeout() {
            printer.finish();
            return Err(error);
        }

        if Instant::now() >= deadline {
            printer.finish();
            return Err(format!(
                "timed out waiting for ProviderRequestFinished or TaskCompleted for {request_id}"
            ));
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PromptStreamSection {
    Thinking,
    Assistant,
}

struct PromptStreamPrinter<'a, W: Write + ?Sized> {
    show_thinking: bool,
    format: PromptOutputFormat,
    stdout: &'a mut W,
    active_section: Option<PromptStreamSection>,
    wrote_output: bool,
    assistant_buffer: String,
    saw_thinking: bool,
}

impl<'a, W: Write + ?Sized> PromptStreamPrinter<'a, W> {
    fn new(show_thinking: bool, format: PromptOutputFormat, stdout: &'a mut W) -> Self {
        Self {
            show_thinking,
            format,
            stdout,
            active_section: None,
            wrote_output: false,
            assistant_buffer: String::new(),
            saw_thinking: false,
        }
    }

    fn observe(&mut self, event: &EventEnvelopeV1, request_id: &str) {
        if self.format == PromptOutputFormat::Json {
            self.write_json(event, request_id);
            return;
        }

        match &event.payload {
            EventV1::ProviderReasoningDelta(data)
                if self.show_thinking
                    && provider_event_matches_prompt(event, &data.request_id, request_id) =>
            {
                self.write_thinking(&data.delta);
            }
            EventV1::ProviderStreamDelta(data)
                if provider_event_matches_prompt(event, &data.request_id, request_id) =>
            {
                self.write_assistant(&data.delta);
            }
            _ => {}
        }
    }

    fn finish(&mut self) {
        if self.format == PromptOutputFormat::Json {
            let _ = self.stdout.flush();
            return;
        }

        self.flush_assistant_buffer();
        if self.wrote_output {
            let _ = writeln!(self.stdout);
        }
        self.active_section = None;
        self.wrote_output = false;
    }

    fn write_json(&mut self, event: &EventEnvelopeV1, request_id: &str) {
        let include = match &event.payload {
            EventV1::ProviderReasoningDelta(data) => {
                provider_event_matches_prompt(event, &data.request_id, request_id)
            }
            EventV1::ProviderStreamDelta(data) => {
                provider_event_matches_prompt(event, &data.request_id, request_id)
            }
            EventV1::ProviderRequestStarted(data) => {
                provider_event_matches_prompt(event, &data.request_id, request_id)
            }
            EventV1::ProviderRequestFinished(data) => {
                provider_finish_matches_prompt(event, data, request_id)
            }
            EventV1::ToolCallRequested(_)
            | EventV1::ToolCallStarted(_)
            | EventV1::ToolCallFinished(_)
            | EventV1::TaskCompleted(_)
            | EventV1::TaskCancelled(_) => event_matches_request(event, request_id),
            EventV1::RunFailed(_) => true,
            _ => false,
        };

        if !include {
            return;
        }

        if let Ok(line) = serde_json::to_string(event) {
            let _ = writeln!(self.stdout, "{line}");
            let _ = self.stdout.flush();
        }
    }

    fn write_thinking(&mut self, delta: &str) {
        if delta.is_empty() {
            return;
        }
        if self.active_section != Some(PromptStreamSection::Thinking) {
            if self.wrote_output {
                let _ = writeln!(self.stdout);
            }
            let _ = write!(self.stdout, "Thinking: ");
            self.active_section = Some(PromptStreamSection::Thinking);
        }
        self.saw_thinking = true;
        self.wrote_output = true;
        let _ = write!(self.stdout, "{delta}");
        let _ = self.stdout.flush();
    }

    fn write_assistant(&mut self, delta: &str) {
        if delta.is_empty() {
            return;
        }
        if self.show_thinking {
            self.buffer_assistant_delta(delta);
            return;
        }
        if self.active_section == Some(PromptStreamSection::Thinking) {
            let _ = writeln!(self.stdout);
        }
        self.active_section = Some(PromptStreamSection::Assistant);
        self.wrote_output = true;
        let _ = write!(self.stdout, "{delta}");
        let _ = self.stdout.flush();
    }

    fn buffer_assistant_delta(&mut self, delta: &str) {
        if self.assistant_buffer.is_empty() || !self.saw_thinking {
            self.assistant_buffer.push_str(delta);
            return;
        }

        if assistant_delta_replays_buffer(delta, &self.assistant_buffer) {
            return;
        }

        if let Some(remainder) = delta.strip_prefix(&self.assistant_buffer) {
            self.assistant_buffer.push_str(remainder);
        } else {
            self.assistant_buffer.push_str(delta);
        }
    }

    fn flush_assistant_buffer(&mut self) {
        if self.assistant_buffer.is_empty() {
            return;
        }
        if self.wrote_output {
            let _ = writeln!(self.stdout);
        }
        let _ = write!(self.stdout, "{}", self.assistant_buffer);
        let _ = self.stdout.flush();
        self.assistant_buffer.clear();
        self.active_section = Some(PromptStreamSection::Assistant);
        self.wrote_output = true;
    }
}

fn assistant_delta_replays_buffer(delta: &str, assistant_buffer: &str) -> bool {
    if delta == assistant_buffer || delta.trim_end() == assistant_buffer.trim_end() {
        return true;
    }

    let delta_trimmed = delta.trim();
    let buffer_trimmed = assistant_buffer.trim();
    if buffer_trimmed.chars().count() < 12 {
        return false;
    }

    delta_trimmed == buffer_trimmed
        || normalize_stream_text_for_replay_check(delta_trimmed)
            == normalize_stream_text_for_replay_check(buffer_trimmed)
}

fn normalize_stream_text_for_replay_check(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Prompt mode waits on the coordinator event stream once, then processes replayed
/// and live events incrementally. That keeps the steady-state wait cost bounded by
/// new events instead of rereading and reparsing the full JSONL log every poll tick.
#[derive(Debug)]
struct PromptCompletionTracker<'a> {
    request_id: &'a str,
    agent_turn_task_id: Option<String>,
    provider_error_seen_at: Option<Instant>,
}

impl<'a> PromptCompletionTracker<'a> {
    fn new(request_id: &'a str) -> Self {
        Self {
            request_id,
            agent_turn_task_id: None,
            provider_error_seen_at: None,
        }
    }

    fn observe(&mut self, event: &EventEnvelopeV1) -> PromptCompletionStatus {
        match &event.payload {
            EventV1::RunFailed(data) => {
                return PromptCompletionStatus::Failed(format!(
                    "run failed before prompt completion for {}: {}",
                    self.request_id, data.error
                ));
            }
            EventV1::TaskScheduled(data)
                if event_matches_request(event, self.request_id)
                    && task_schedule_marks_agent_turn(data) =>
            {
                self.agent_turn_task_id = Some(data.task_id.clone());
            }
            EventV1::ProviderRequestFinished(data)
                if provider_finish_matches_prompt(event, data, self.request_id)
                    && data.finish_reason.eq_ignore_ascii_case("error")
                    && self.provider_error_seen_at.is_none() =>
            {
                self.provider_error_seen_at = Some(Instant::now());
            }
            EventV1::TaskCancelled(data) if self.matches_cancelled_prompt_task(event, data) => {
                return PromptCompletionStatus::Failed(format!(
                    "prompt request {} was cancelled: {}",
                    self.request_id, data.reason
                ));
            }
            EventV1::TaskCompleted(data) if self.matches_completed_agent_turn(event, data) => {
                return PromptCompletionStatus::Completed;
            }
            _ => {}
        }

        PromptCompletionStatus::Continue
    }

    fn next_wait_deadline(&self, timeout_deadline: Instant) -> Instant {
        self.provider_error_seen_at
            .map(|seen_at| std::cmp::min(timeout_deadline, seen_at + PROVIDER_ERROR_REASON_GRACE))
            .unwrap_or(timeout_deadline)
    }

    fn provider_error_timeout(&self) -> Option<String> {
        self.provider_error_seen_at.and_then(|seen_at| {
            (Instant::now().saturating_duration_since(seen_at) >= PROVIDER_ERROR_REASON_GRACE).then(
                || {
                    format!(
                        "prompt request {} finished with provider error",
                        self.request_id
                    )
                },
            )
        })
    }

    fn matches_completed_agent_turn(
        &self,
        event: &EventEnvelopeV1,
        data: &TaskCompletedEvent,
    ) -> bool {
        if !event_matches_request(event, self.request_id) {
            return task_completed_marks_agent_turn(data) && data.task_id == self.request_id;
        }

        if task_completed_marks_agent_turn(data) {
            return true;
        }

        if task_completed_marks_child_tool(data) {
            return false;
        }

        self.agent_turn_task_id.as_deref() == Some(data.task_id.as_str())
            || data.task_id == self.request_id
    }

    fn matches_cancelled_prompt_task(
        &self,
        event: &EventEnvelopeV1,
        data: &TaskCancelledEvent,
    ) -> bool {
        if !event_matches_request(event, self.request_id) {
            return task_cancelled_marks_agent_turn(data) && data.task_id == self.request_id;
        }

        if task_cancelled_marks_agent_turn(data) {
            return true;
        }

        if task_cancelled_marks_child_tool(data) {
            return false;
        }

        self.agent_turn_task_id.as_deref() == Some(data.task_id.as_str())
            || data.task_id == self.request_id
    }
}

fn task_schedule_marks_agent_turn(data: &harness_core::event::TaskScheduledEvent) -> bool {
    data.queue_key
        .as_deref()
        .is_some_and(|queue_key| queue_key.starts_with("provider_model:"))
}

fn task_completed_marks_agent_turn(data: &TaskCompletedEvent) -> bool {
    data.metadata
        .as_ref()
        .and_then(|metadata| metadata.task_scope)
        .is_some_and(|scope| matches!(scope, harness_core::event::TaskTerminalScope::AgentTurn))
}

fn task_completed_marks_child_tool(data: &TaskCompletedEvent) -> bool {
    data.metadata
        .as_ref()
        .and_then(|metadata| metadata.task_scope)
        .is_some_and(|scope| matches!(scope, harness_core::event::TaskTerminalScope::ToolCall))
        || data
            .metadata
            .as_ref()
            .and_then(|metadata| metadata.lineage.as_ref())
            .and_then(|lineage| lineage.parent_tool_call_id.as_deref())
            .is_some()
}

fn task_cancelled_marks_agent_turn(data: &TaskCancelledEvent) -> bool {
    data.task_scope
        .is_some_and(|scope| matches!(scope, harness_core::event::TaskTerminalScope::AgentTurn))
}

fn task_cancelled_marks_child_tool(data: &TaskCancelledEvent) -> bool {
    data.task_scope
        .is_some_and(|scope| matches!(scope, harness_core::event::TaskTerminalScope::ToolCall))
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum PromptCompletionStatus {
    Continue,
    Completed,
    Failed(String),
}

#[cfg(test)]
pub(super) fn evaluate_prompt_completion(
    events: &[EventEnvelopeV1],
    request_id: &str,
) -> PromptCompletionStatus {
    let agent_turn_task_id = agent_turn_task_id(events, request_id);

    if let Some(run_error) = events.iter().find_map(|event| match &event.payload {
        EventV1::RunFailed(data) => Some(data.error.clone()),
        _ => None,
    }) {
        return PromptCompletionStatus::Failed(format!(
            "run failed before prompt completion for {request_id}: {run_error}"
        ));
    }

    if let Some(cancel_reason) = events.iter().find_map(|event| match &event.payload {
        EventV1::TaskCancelled(data)
            if event_matches_request(event, request_id)
                && (task_cancelled_marks_agent_turn(data)
                    || (!task_cancelled_marks_child_tool(data)
                        && (agent_turn_task_id
                            .is_some_and(|task_id| data.task_id == task_id)
                            || data.task_id == request_id))) =>
        {
            Some(data.reason.clone())
        }
        _ => None,
    }) {
        return PromptCompletionStatus::Failed(format!(
            "prompt request {request_id} was cancelled: {cancel_reason}"
        ));
    }

    if events.iter().any(|event| match &event.payload {
        EventV1::TaskCompleted(data) => {
            event_matches_request(event, request_id)
                && (task_completed_marks_agent_turn(data)
                    || (!task_completed_marks_child_tool(data)
                        && (agent_turn_task_id.is_some_and(|task_id| data.task_id == task_id)
                            || data.task_id == request_id)))
        }
        _ => false,
    }) {
        return PromptCompletionStatus::Completed;
    }

    PromptCompletionStatus::Continue
}

#[cfg(test)]
fn agent_turn_task_id<'a>(events: &'a [EventEnvelopeV1], request_id: &str) -> Option<&'a str> {
    events.iter().find_map(|event| match &event.payload {
        EventV1::TaskScheduled(data)
            if event_matches_request(event, request_id) && task_schedule_marks_agent_turn(data) =>
        {
            Some(data.task_id.as_str())
        }
        _ => None,
    })
}

fn event_matches_request(event: &EventEnvelopeV1, request_id: &str) -> bool {
    event.correlation_id.as_deref() == Some(request_id)
}

fn provider_event_matches_prompt(
    event: &EventEnvelopeV1,
    provider_request_id: &str,
    request_id: &str,
) -> bool {
    provider_request_id == request_id || event_matches_request(event, request_id)
}

fn provider_finish_matches_prompt(
    event: &EventEnvelopeV1,
    data: &harness_core::event::ProviderRequestFinishedEvent,
    request_id: &str,
) -> bool {
    provider_event_matches_prompt(event, &data.request_id, request_id)
        || data
            .metadata
            .as_ref()
            .and_then(|metadata| metadata.turn_id.as_deref())
            == Some(request_id)
}

#[cfg(test)]
pub(super) fn has_provider_error_finish(events: &[EventEnvelopeV1], request_id: &str) -> bool {
    events.iter().any(|event| match &event.payload {
        EventV1::ProviderRequestFinished(data) => {
            provider_finish_matches_prompt(event, data, request_id)
                && data.finish_reason.eq_ignore_ascii_case("error")
        }
        _ => false,
    })
}

pub(super) fn prompt_wait_timeout() -> Duration {
    let raw = env::var(WAIT_TIMEOUT_ENV).ok();
    parse_wait_timeout_ms(raw.as_deref())
}

pub(super) fn parse_wait_timeout_ms(raw: Option<&str>) -> Duration {
    let Some(raw) = raw else {
        return DEFAULT_WAIT_TIMEOUT;
    };

    let Ok(ms) = raw.trim().parse::<u64>() else {
        return DEFAULT_WAIT_TIMEOUT;
    };

    if ms == 0 {
        return DEFAULT_WAIT_TIMEOUT;
    }

    Duration::from_millis(ms)
}
