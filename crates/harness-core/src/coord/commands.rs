//! Native command execution belongs to the coordinator, not a tool-global manager.
//!
//! Shared integration: `coord.rs` declares/exports this module; Runtime owns a
//! `commands: BTreeMap<String, CommandRun>`; JobKind has `Command`; Completion has
//! `Command { id: String, result: Result<CommandExit, CoordinatorError> }`.
//! Completion dispatch calls `finish_command` before ordinary tool bookkeeping.
//! Run shutdown cancels these ordinary jobs and clears commands after draining.
//! Normal child completion calls `reparent_commands(child, parent)` before child
//! shutdown; explicit child/session cancellation cancels its command jobs first.
//! Enabled command notifications deliver a redacted output tail at the next provider
//! request, or open a reminder-only idle wake. Terminal tool observations suppress
//! undelivered notices; explicit cancellation and shutdown never notify.
//! Durable intent/observation receipts survive resume without rerunning commands.
//! The pending window holds 16 detailed notices and a 64-command overflow summary;
//! overflow retains only identity/status until delivered. Rejected wake hooks do
//! not retry unchanged notices, and final-request completions continue that turn.
mod actor;
mod notifications;
pub(super) use notifications::CommandNoticeBacklog;

use super::{runtime::*, *};
use crate::{
    subagent::{GetCommandOrSubagentOutputResult, KillCommandOrSubagentResult},
    tool::{ToolCapability, ToolContext},
};
use std::{
    fs::{File, OpenOptions},
    io::Write,
    process::Stdio,
    time::{Duration, Instant},
};
use tokio::{
    io::{AsyncRead, AsyncReadExt},
    sync::watch,
    task::JoinSet,
};

const PREVIEW_LIMIT: usize = 40_000;
const FILE_LIMIT: usize = 8 * 1024 * 1024;
const PENDING_LIMIT: usize = 4 * 1024 * 1024;

/// A shell adapter must validate its command, cwd and sandbox before admission.
/// The scratch directory moves with the process instead of the returning tool.
pub struct PreparedCommand {
    pub command: tokio::process::Command,
    pub label: String,
    pub description: Option<String>,
    pub timeout: Duration,
    pub scratch: Option<tempfile::TempDir>,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct CommandSnapshot {
    pub result: GetCommandOrSubagentOutputResult,
    pub owner_agent_id: Option<String>,
    pub original_owner_agent_id: Option<String>,
    pub parent_tool_call_id: String,
    pub parent_task_id: Option<String>,
    pub description: Option<String>,
    pub cwd: String,
    pub pid: Option<u32>,
    pub started_mono_ms: u64,
    pub finished_mono_ms: Option<u64>,
    pub stdout: String,
    pub stderr: String,
}
impl CommandSnapshot {
    pub fn is_terminal(&self) -> bool {
        self.result.status != "running"
    }
}

/// Snapshot and receiver are obtained in the same actor operation: no lost wake.
pub struct CommandSubscription {
    pub snapshot: CommandSnapshot,
    pub updates: watch::Receiver<CommandSnapshot>,
}

pub struct CommandExit {
    pub exit_code: Option<i32>,
    pub timed_out: bool,
}

pub(super) struct CommandRun {
    output_tail: Option<String>,
    snapshot: CommandSnapshot,
    updates: watch::Sender<CommandSnapshot>,
    finished: Option<Instant>,
    output: Option<File>,
    file_bytes: usize,
    pending: [Vec<u8>; 2],
    discarded: [bool; 2],
}

impl CoordinatorHandle {
    /// Marks a terminal snapshot returned to its owner as already delivered.
    pub async fn observe_command_result(
        &self,
        actor: EventActor,
        id: String,
    ) -> Result<(), CoordinatorError> {
        self.call(move |runtime| {
            runtime.info()?;
            if let Some(agent) = actor.agent_id.as_deref()
                && runtime.command_notice_unobserved(agent, &id)
            {
                runtime.emit(
                    actor.clone(),
                    Some(id.clone()),
                    EventV1::CommandNotice(CommandNoticeEvent::Observed { task_id: id }),
                )?;
            }
            Ok(())
        })
        .await
    }

    pub async fn start_command(
        &self,
        context: ToolContext,
        prepared: PreparedCommand,
    ) -> Result<CommandSnapshot, CoordinatorError> {
        self.call(move |s| s.admit_command(context, prepared)).await
    }

    pub async fn subscribe_command(
        &self,
        actor: EventActor,
        id: String,
    ) -> Result<Option<CommandSubscription>, CoordinatorError> {
        self.call(move |s| {
            s.info()?;
            let Some(command) = s.commands.get(&id) else {
                return Ok(None);
            };
            s.authorize_command(&actor, command)?;
            Ok(Some(CommandSubscription {
                snapshot: command.snapshot(s.clock.mono_ms()),
                updates: command.updates.subscribe(),
            }))
        })
        .await
    }

    pub async fn list_commands(
        &self,
        actor: EventActor,
    ) -> Result<Vec<CommandSnapshot>, CoordinatorError> {
        self.call(move |s| {
            s.info()?;
            let now = s.clock.mono_ms();
            Ok(s.commands
                .values()
                .filter(|command| s.authorize_command(&actor, command).is_ok())
                .map(|command| command.snapshot(now))
                .collect())
        })
        .await
    }

    pub async fn kill_command(
        &self,
        actor: EventActor,
        id: String,
    ) -> Result<Option<KillCommandOrSubagentResult>, CoordinatorError> {
        self.call(move |s| {
            s.info()?;
            let Some(command) = s.commands.get(&id) else {
                return Ok(None);
            };
            s.authorize_command(&actor, command)?;
            let exited = command.snapshot.is_terminal();
            if !exited {
                s.cancel(&id, "command explicitly killed")?;
            }
            Ok(Some(KillCommandOrSubagentResult {
                task_id: id,
                outcome: if exited { "already_exited" } else { "killed" }.into(),
                message: if exited {
                    "Task had already completed"
                } else {
                    "Task was terminated successfully"
                }
                .into(),
            }))
        })
        .await
    }
}

impl CommandRun {
    fn snapshot(&self, now: u64) -> CommandSnapshot {
        let mut snapshot = self.snapshot.clone();
        if !snapshot.is_terminal() {
            snapshot.result.duration_secs =
                Duration::from_millis(now.saturating_sub(snapshot.started_mono_ms)).as_secs_f64();
            snapshot.finished_mono_ms = None;
        }
        snapshot
    }

    fn append(&mut self, stream: usize, text: &str) -> Result<(), std::io::Error> {
        if let Some(tail) = self.output_tail.as_mut() {
            let start = text.ceil_char_boundary(text.len().saturating_sub(4000));
            let text = &text[start..];
            let discard = tail.ceil_char_boundary((tail.len() + text.len()).saturating_sub(4000));
            tail.drain(..discard);
            tail.push_str(text);
        }
        let keep = text.floor_char_boundary(FILE_LIMIT.saturating_sub(self.file_bytes));
        let output = self
            .output
            .as_mut()
            .ok_or_else(|| std::io::Error::other("command output is already closed"))?;
        output.write_all(&text.as_bytes()[..keep])?;
        self.file_bytes += keep;
        self.snapshot.result.truncated |= keep < text.len();
        let preview = &mut self.snapshot.result.output;
        let keep = text.floor_char_boundary(PREVIEW_LIMIT.saturating_sub(preview.len()));
        preview.push_str(&text[..keep]);
        self.snapshot.result.truncated |= keep < text.len();
        let captured = if stream == 0 {
            &mut self.snapshot.stdout
        } else {
            &mut self.snapshot.stderr
        };
        let keep = text.floor_char_boundary(PREVIEW_LIMIT.saturating_sub(captured.len()));
        captured.push_str(&text[..keep]);
        if self.snapshot.result.truncated {
            self.snapshot.result.truncation_hint =
                "[truncated - use read on output_file for captured content]".into();
        }
        Ok(())
    }
}

#[cfg(unix)]
async fn run_command(
    handle: CoordinatorHandle,
    id: String,
    mut child: tokio::process::Child,
    mut group: crate::process::Group,
    timeout: Duration,
    cancellation: CancellationToken,
) -> Result<CommandExit, CoordinatorError> {
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| CoordinatorError::Invalid("stdout pipe is unavailable".into()))?;
    let stderr = child
        .stderr
        .take()
        .ok_or_else(|| CoordinatorError::Invalid("stderr pipe is unavailable".into()))?;
    let mut readers = JoinSet::new();
    readers.spawn(read_output(handle.clone(), id.clone(), 0, stdout));
    readers.spawn(read_output(handle, id, 1, stderr));
    let deadline = tokio::time::sleep(timeout);
    tokio::pin!(deadline);
    let mut timed_out = false;
    let mut failure = None;
    let status = loop {
        tokio::select! {
            biased;
            () = cancellation.cancelled() => break None,
            () = &mut deadline => { timed_out = true; break None; }
            status = child.wait() => break Some(status),
            result = readers.join_next(), if !readers.is_empty() => {
                match result {
                    Some(Ok(Ok(()))) => {},
                    Some(Ok(Err(error))) => { failure = Some(error); break None; },
                    Some(Err(error)) => {
                        failure = Some(CoordinatorError::Invalid(error.to_string()));
                        break None;
                    },
                    None => {},
                }
            }
        }
    };
    let status = match status {
        Some(status) => {
            group.stop();
            status?
        }
        None => group.terminate(&mut child).await?,
    };
    // Descendants cannot hold the pipes after normal shell completion either.
    group.stop();
    let drained = tokio::time::timeout(Duration::from_secs(1), async {
        while let Some(result) = readers.join_next().await {
            result.map_err(|error| CoordinatorError::Invalid(error.to_string()))??;
        }
        Ok::<_, CoordinatorError>(())
    })
    .await;
    // JoinSet drops/aborts every reader on all error paths.
    match drained {
        Ok(result) => result?,
        Err(_) => {
            return Err(CoordinatorError::Invalid(
                "command descendants did not close their output pipes".into(),
            ));
        }
    }
    if let Some(failure) = failure {
        return Err(failure);
    }
    Ok(CommandExit {
        exit_code: status.code(),
        timed_out,
    })
}

async fn read_output(
    handle: CoordinatorHandle,
    id: String,
    stream: usize,
    mut pipe: impl AsyncRead + Unpin,
) -> Result<(), CoordinatorError> {
    let mut buffer = [0; 8192];
    loop {
        let count = pipe.read(&mut buffer).await?;
        let bytes = buffer[..count].to_vec();
        let id = id.clone();
        handle
            .call(move |s| s.command_bytes(&id, stream, &bytes, count == 0))
            .await?;
        if count == 0 {
            return Ok(());
        }
    }
}

impl Runtime {
    /// Drops an undelivered completion notice the owner no longer needs.
    fn forget_command_notice(&mut self, agent: &str, id: &str) {
        if let Some(backlog) = self.command_notices.get_mut(agent) {
            backlog.pending.remove(id);
        }
        self.sync_command_notices(agent);
    }
}
