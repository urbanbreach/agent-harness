//! Monitors: background tasks whose matching output lines wake their owner.
//!
//! A command monitor runs in a pseudo-terminal; a file monitor polls one path. Complete
//! lines that match the filter collect for two seconds, each monitor delivers at most once
//! every five seconds, and one delivery carries at most 50 lines. Delivery is a
//! `monitor_event` reminder governed by `runtime.behavior.command_notifications`. Five
//! deliveries with no tool call or user message between them pause the command monitors
//! that sent lines until rearm or user input. Monitors end with the process.
use super::*;
use crate::coord::reminders::PendingReminder;
use tokio::time::Instant;

const COALESCE: Duration = Duration::from_secs(2);
const RATE_LIMIT: Duration = Duration::from_secs(5);
const MAX_LINES: usize = 50;
/// A 4 KiB delivery minus room for headers and notes.
const MAX_CHARS: usize = 3584;
const MAX_LINE: usize = 1000;
const MAX_PARTIAL: usize = 64 * 1024;
const WAKE_BUDGET: usize = 5;

pub struct PreparedMonitor {
    pub launch: MonitorLaunch,
    pub description: String,
    pub filter: Option<regex::Regex>,
    pub timeout: Duration,
}

pub enum MonitorLaunch {
    /// A validated shell command and the scratch directory its sandbox uses.
    Command {
        command: Box<tokio::process::Command>,
        label: String,
        scratch: Option<tempfile::TempDir>,
    },
    File {
        path: PathBuf,
        event: FileEvent,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileEvent {
    Create,
    Modify,
}

impl FileEvent {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Create => "create",
            Self::Modify => "modify",
        }
    }
}

#[derive(Default)]
pub(in crate::coord) struct Monitor {
    description: String,
    filter: Option<regex::Regex>,
    file: bool,
    partial: String,
    lines: Vec<String>,
    chars: usize,
    omitted: usize,
    exit: Option<String>,
    paused: bool,
    dropped: usize,
    last_delivery: Option<Instant>,
    last_batch: Option<Vec<String>>,
}

impl Monitor {
    /// Queues the matching complete lines in `text`; true when anything was queued.
    fn feed(&mut self, text: &str) -> bool {
        self.partial.push_str(text);
        let mut queued = false;
        while let Some(end) = self.partial.find('\n') {
            let line = plain(&self.partial[..end]);
            self.partial.drain(..=end);
            if line.is_empty() || self.filter.as_ref().is_some_and(|f| !f.is_match(&line)) {
                continue;
            }
            if self.paused {
                self.dropped += 1;
            } else if self.lines.len() < MAX_LINES && self.chars + line.len() <= MAX_CHARS {
                self.chars += line.len();
                self.lines.push(line);
                queued = true;
            } else {
                self.omitted += 1;
                queued = true;
            }
        }
        if self.partial.len() > MAX_PARTIAL {
            let start = self
                .partial
                .ceil_char_boundary(self.partial.len() - MAX_PARTIAL);
            self.partial.drain(..start);
        }
        queued
    }

    fn pending(&self) -> bool {
        !self.lines.is_empty() || self.omitted > 0 || self.exit.is_some()
    }
}

/// What a terminal would show for one line: the last carriage-return segment, without
/// escape sequences or control characters, trimmed and capped.
fn plain(line: &str) -> String {
    let line = line
        .rsplit('\r')
        .find(|segment| !segment.trim().is_empty())
        .unwrap_or_default();
    let mut text = String::with_capacity(line.len());
    let mut chars = line.chars();
    while let Some(c) = chars.next() {
        match c {
            '\u{1b}' => match chars.next() {
                Some('[') => {
                    let _ = chars.by_ref().find(|c| ('@'..='~').contains(c));
                }
                Some(']') => {
                    let _ = chars.by_ref().find(|c| matches!(c, '\u{7}' | '\\'));
                }
                _ => {}
            },
            '\t' => text.push(' '),
            c if c.is_control() => {}
            c => text.push(c),
        }
    }
    let text = text.trim();
    if text.len() > MAX_LINE {
        format!("{}…", &text[..text.floor_char_boundary(MAX_LINE)])
    } else {
        text.to_owned()
    }
}

#[derive(Default)]
pub(in crate::coord) struct MonitorWakes {
    pub deadline: Option<Instant>,
    /// Consecutive monitor-only deliveries per agent and when the last one went out.
    streaks: BTreeMap<String, (usize, Instant)>,
}

impl CoordinatorHandle {
    /// Starts a monitor and reports whether its events can reach the model.
    pub async fn start_monitor(
        &self,
        context: ToolContext,
        monitor: PreparedMonitor,
    ) -> Result<(CommandSnapshot, bool), CoordinatorError> {
        self.call(move |s| {
            let notifications = s.config.behavior.command_notifications.enabled;
            let (launch, label, scratch) = match monitor.launch {
                MonitorLaunch::Command {
                    command,
                    label,
                    scratch,
                } => (Launch::Pty(*command), label, scratch),
                MonitorLaunch::File { path, event } => {
                    let label = format!("{} {}", event.as_str(), path.display());
                    (Launch::File { path, event }, label, None)
                }
            };
            let state = Monitor {
                description: s.redactor.redact_text(&monitor.description),
                filter: monitor.filter,
                file: matches!(launch, Launch::File { .. }),
                ..Monitor::default()
            };
            if let Some(agent) = &context.actor.agent_id {
                s.monitor_wakes.streaks.remove(agent);
            }
            let snapshot = s.admit(
                context,
                launch,
                label,
                Some(monitor.description),
                monitor.timeout,
                scratch,
                Some(state),
            )?;
            Ok((snapshot, notifications))
        })
        .await
    }

    /// Resumes one paused monitor, or every paused monitor the actor owns; returns the
    /// resumed ids and how many matching lines were dropped while they were paused.
    pub async fn rearm_monitors(
        &self,
        actor: EventActor,
        task_id: Option<String>,
    ) -> Result<(Vec<String>, usize), CoordinatorError> {
        self.call(move |s| {
            s.info()?;
            if let Some(id) = &task_id {
                let command = s
                    .commands
                    .get(id)
                    .filter(|command| command.monitor.is_some())
                    .ok_or_else(|| CoordinatorError::UnknownTask(id.clone()))?;
                s.authorize_command(&actor, command)?;
                if command.snapshot.is_terminal() {
                    return Err(CoordinatorError::Invalid(format!(
                        "monitor {id} has already ended"
                    )));
                }
            }
            let ids: Vec<_> = s
                .commands
                .iter()
                .filter(|(id, command)| {
                    task_id.as_ref().is_none_or(|wanted| wanted == *id)
                        && command.monitor.as_ref().is_some_and(|m| m.paused)
                        && !command.snapshot.is_terminal()
                        && s.authorize_command(&actor, command).is_ok()
                })
                .map(|(id, _)| id.clone())
                .collect();
            Ok(s.resume_monitors(&ids, actor.agent_id.as_deref()))
        })
        .await
    }
}

impl Runtime {
    /// Opens the collection window, or brings an existing deadline forward to `at`.
    fn wake_monitors_at(&mut self, at: Instant) {
        let deadline = &mut self.monitor_wakes.deadline;
        *deadline = Some(deadline.map_or(at, |current| current.min(at)));
    }

    /// Whether a monitor `agent` owns still holds lines or an exit report waiting for its
    /// collection window. Another agent's open window does not count.
    pub(in crate::coord) fn monitor_pending_for(&self, agent: &str) -> bool {
        self.commands.values().any(|command| {
            command.snapshot.owner_agent_id.as_deref() == Some(agent)
                && command
                    .monitor
                    .as_ref()
                    .is_some_and(|monitor| monitor.pending())
        })
    }

    pub(super) fn feed_monitor(&mut self, id: &str, text: &str) {
        if self
            .commands
            .get_mut(id)
            .and_then(|command| command.monitor.as_mut())
            .is_some_and(|monitor| monitor.feed(text))
        {
            self.wake_monitors_at(Instant::now() + COALESCE);
        }
    }

    /// Queues the exit report; a killed or shut-down monitor reports nothing.
    pub(super) fn monitor_exited(&mut self, id: &str) {
        let Some(command) = self.commands.get_mut(id) else {
            return;
        };
        let result = &command.snapshot.result;
        let Some(monitor) = command.monitor.as_mut() else {
            return;
        };
        let code = result
            .exit_code
            .map_or_else(String::new, |code| format!(" (exit code {code})"));
        monitor.exit = match result.status.as_str() {
            "cancelled" => {
                monitor.lines.clear();
                monitor.omitted = 0;
                return;
            }
            "timed_out" => Some("monitor timed out".into()),
            status => Some(format!("monitor {status}{code}")),
        };
        self.wake_monitors_at(Instant::now() + COALESCE);
    }

    /// Any tool call by the agent ends a streak of monitor-only deliveries.
    pub(in crate::coord) fn monitor_activity(&mut self, agent: &str) {
        self.monitor_wakes.streaks.remove(agent);
    }

    /// A user message resumes every paused monitor the agent owns.
    pub(in crate::coord) fn user_resumes_monitors(&mut self, agent: &str) {
        let ids: Vec<_> = self
            .commands
            .iter()
            .filter(|(_, command)| {
                command.snapshot.owner_agent_id.as_deref() == Some(agent)
                    && command.monitor.as_ref().is_some_and(|m| m.paused)
            })
            .map(|(id, _)| id.clone())
            .collect();
        self.resume_monitors(&ids, Some(agent));
    }

    fn resume_monitors(&mut self, ids: &[String], agent: Option<&str>) -> (Vec<String>, usize) {
        let mut dropped = 0;
        for id in ids {
            if let Some(monitor) = self.commands.get_mut(id).and_then(|c| c.monitor.as_mut()) {
                dropped += std::mem::take(&mut monitor.dropped);
                monitor.paused = false;
                monitor.last_batch = None;
                self.publish_monitor_state(id, false);
            }
        }
        if let Some(agent) = agent {
            self.monitor_wakes.streaks.remove(agent);
        }
        (ids.to_vec(), dropped)
    }

    fn publish_monitor_state(&mut self, id: &str, paused: bool) {
        let Ok(event_id) = self.id("live") else {
            return;
        };
        let (Some(store), Ok(info)) = (&self.store, self.info()) else {
            return;
        };
        store.publish_live(LiveEventEnvelope {
            event_id,
            run_id: info.run_id.clone(),
            mono_ms: self.clock.mono_ms(),
            ts: None,
            actor: EventActor::new(ActorKind::System, None),
            correlation_id: Some(id.to_owned()),
            causation_id: None,
            stream_key: None,
            payload: LiveEventV1::MonitorState {
                task_id: id.to_owned(),
                paused,
            },
        });
    }

    /// Delivers every monitor whose window closed and whose rate limit allows it.
    pub(in crate::coord) fn flush_monitors(&mut self) -> Result<(), CoordinatorError> {
        self.monitor_wakes.deadline = None;
        let now = Instant::now();
        let mut next = None::<Instant>;
        let mut batches = BTreeMap::<String, Vec<String>>::new();
        for (id, command) in &self.commands {
            let Some(monitor) = command.monitor.as_ref().filter(|m| m.pending()) else {
                continue;
            };
            if let Some(ready) = monitor.last_delivery.map(|last| last + RATE_LIMIT)
                && monitor.exit.is_none()
                && now < ready
            {
                next = Some(next.map_or(ready, |next| next.min(ready)));
                continue;
            }
            if let Some(owner) = &command.snapshot.owner_agent_id {
                let owner = self.native_command_parent(owner).unwrap_or(owner);
                batches
                    .entry(owner.to_owned())
                    .or_default()
                    .push(id.clone());
            }
        }
        for (agent, ids) in batches {
            self.deliver_monitors(&agent, &ids, now)?;
        }
        if let Some(next) = next {
            self.wake_monitors_at(next);
        }
        Ok(())
    }

    fn deliver_monitors(
        &mut self,
        agent: &str,
        ids: &[String],
        now: Instant,
    ) -> Result<(), CoordinatorError> {
        let enabled = self.config.behavior.command_notifications.enabled
            && self.command_notice_allowed(agent);
        let (mut text, mut lines, mut chars, mut omitted) = (String::new(), 0, 0, 0);
        let (mut exited, mut noisy) = (false, Vec::new());
        for id in ids {
            let Some(monitor) = self.commands.get_mut(id).and_then(|c| c.monitor.as_mut()) else {
                continue;
            };
            let batch = std::mem::take(&mut monitor.lines);
            let own_omitted = std::mem::take(&mut monitor.omitted);
            let exit = monitor.exit.take();
            monitor.chars = 0;
            // A repeat of this monitor's previous line-only batch adds nothing.
            if !enabled
                || (exit.is_none()
                    && own_omitted == 0
                    && monitor.last_batch.as_ref() == Some(&batch))
            {
                continue;
            }
            omitted += own_omitted;
            let mut body = Vec::new();
            for line in &batch {
                if lines < MAX_LINES && chars + line.len() <= MAX_CHARS {
                    (lines, chars) = (lines + 1, chars + line.len());
                    body.push(line.as_str());
                } else {
                    omitted += 1;
                }
            }
            if !monitor.file && !batch.is_empty() && exit.is_none() {
                noisy.push(id.clone());
            }
            exited |= exit.is_some();
            body.extend(exit.as_deref());
            monitor.last_delivery = Some(now);
            if !body.is_empty() {
                text.push_str(&format!(
                    "Monitor event({}) task_id {id}:\n{}\n",
                    monitor.description,
                    body.join("\n")
                ));
            }
            monitor.last_batch = Some(batch);
        }
        if text.is_empty() && omitted == 0 {
            return Ok(());
        }
        if omitted > 0 {
            text.push_str(&format!("[{omitted} more matching lines omitted; get_command_or_subagent_output returns the full output.]\n"));
        }
        let (count, last) = self
            .monitor_wakes
            .streaks
            .get(agent)
            .copied()
            .unwrap_or((0, now));
        let quiet = now.duration_since(last) > RATE_LIMIT * 2;
        // A delivered exit ends the streak; a quiet gap starts a new one.
        let mut count = if exited {
            0
        } else if quiet {
            1
        } else {
            count + 1
        };
        if count >= WAKE_BUDGET && !noisy.is_empty() {
            for id in &noisy {
                if let Some(monitor) = self.commands.get_mut(id).and_then(|c| c.monitor.as_mut()) {
                    monitor.paused = true;
                }
                self.publish_monitor_state(id, true);
            }
            text.push_str(&format!("Monitor paused after {WAKE_BUDGET} updates in a row: {}. It keeps running and its exit is still reported, but matching lines are dropped until you call monitor with action \"rearm\" or the user sends a message.\n", noisy.join(", ")));
            count = 0;
        }
        self.monitor_wakes
            .streaks
            .insert(agent.to_owned(), (count, now));
        self.pending_reminders
            .entry(agent.to_owned())
            .or_default()
            .push(PendingReminder {
                kind: RuntimeReminderKind::MonitorEvent,
                body: text.trim_end().to_owned(),
                source: Some(serde_json::json!(ids).to_string()),
            });
        if self.config.behavior.command_notifications.wake_idle
            && self
                .agents
                .get(agent)
                .is_some_and(|state| !state.busy && state.queue.is_empty())
            && self.check_turn_capacity(agent).is_ok()
        {
            self.queue_turn(
                EventActor::new(ActorKind::Worker, Some(agent.to_owned())),
                agent,
                crate::coord::prompt::Prompt {
                    reminder_wake: true,
                    ..Default::default()
                },
                None,
                None,
                None,
            )?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests;
