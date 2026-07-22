//! Product CLI for the durable session-local prompt queue (`harness prompt-queue`).
//!
//! Surfaces enqueue/list/dequeue plus the mid-turn interject front-insert over
//! [`harness_core::prompt_queue::DurablePromptQueue`]. The queue is durable
//! per-session storage under `<session-dir>/tui/prompt-queue.json`; this command
//! never mutates conversation events or the active turn. The interject subcommand
//! front-inserts an entry marked with the honest `turn_was_running` flag so a
//! product layer can drain it after the current turn completes; recovery is the
//! ordinary FIFO `dequeue`.

use std::io::Write;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use clap::{Args, Subcommand};
use harness_core::prompt_queue::{
    DurablePromptQueue, MidTurnInterjection, PromptQueueEntry, PromptQueueError,
};
use serde::Serialize;

use crate::{CliDeps, CliIo};

/// Monotonic suffix so rapid enqueues within the same millisecond stay unique.
static ENTRY_COUNTER: AtomicU64 = AtomicU64::new(0);

#[derive(Debug, Args, Clone)]
pub(crate) struct PromptQueueCommand {
    #[command(subcommand)]
    command: PromptQueueSubcommand,
}

#[derive(Debug, Subcommand, Clone)]
enum PromptQueueSubcommand {
    /// Append one prompt to the durable FIFO queue (JSON).
    Enqueue(PromptQueueEnqueueCommand),
    /// List the durable queue entries in FIFO order (JSON).
    List(PromptQueueSessionCommand),
    /// Pop and return the front queue entry, or report empty (JSON).
    Dequeue(PromptQueueSessionCommand),
    /// Front-insert a mid-turn interjection into the durable queue (JSON).
    Interject(PromptQueueInterjectCommand),
}

/// Shared session-directory selector for subcommands that only need the queue.
#[derive(Debug, Args, Clone)]
struct PromptQueueSessionCommand {
    /// Session run directory that owns the durable queue (`<session>/tui/prompt-queue.json`).
    #[arg(long = "session")]
    session_dir: PathBuf,
}

#[derive(Debug, Args, Clone)]
struct PromptQueueEnqueueCommand {
    /// Prompt text to enqueue (rejected when blank after trim).
    text: String,
    /// Session run directory that owns the durable queue.
    #[arg(long = "session")]
    session_dir: PathBuf,
    /// Optional stable entry id (auto-generated when omitted).
    #[arg(long)]
    id: Option<String>,
}

#[derive(Debug, Args, Clone)]
struct PromptQueueInterjectCommand {
    /// Interjection text (rejected when blank after trim).
    text: String,
    /// Session run directory that owns the durable queue.
    #[arg(long = "session")]
    session_dir: PathBuf,
    /// Optional stable entry id (auto-generated when omitted).
    #[arg(long)]
    id: Option<String>,
    /// Mark the interjection as queued while a turn was running.
    #[arg(long, default_value_t = false)]
    turn_running: bool,
}

#[derive(Debug, Serialize)]
struct PromptQueueListJson {
    queue_path: String,
    count: usize,
    entries: Vec<PromptQueueEntry>,
}

#[derive(Debug, Serialize)]
struct PromptQueueDequeueJson {
    queue_path: String,
    #[serde(flatten)]
    entry: DequeueEntry,
}

#[derive(Debug, Serialize)]
#[serde(tag = "dequeued", rename_all = "snake_case")]
enum DequeueEntry {
    Empty {},
    Entry {
        id: String,
        text: String,
        enqueued_at_unix_ms: u64,
    },
}

pub(crate) fn execute_with_io(
    command: PromptQueueCommand,
    io: &mut CliIo<'_>,
    _deps: &CliDeps,
) -> i32 {
    match command.command {
        PromptQueueSubcommand::Enqueue(cmd) => run_enqueue(cmd, io),
        PromptQueueSubcommand::List(cmd) => run_list(cmd.session_dir, io),
        PromptQueueSubcommand::Dequeue(cmd) => run_dequeue(cmd.session_dir, io),
        PromptQueueSubcommand::Interject(cmd) => run_interject(cmd, io),
    }
}

fn queue_for_session(session_dir: &std::path::Path) -> DurablePromptQueue {
    DurablePromptQueue::for_session(session_dir)
}

fn write_json(io: &mut CliIo<'_>, value: &impl Serialize) -> i32 {
    match serde_json::to_string_pretty(value) {
        Ok(json) => {
            let _ = writeln!(io.stdout, "{json}");
            0
        }
        Err(err) => {
            let _ = writeln!(io.stderr, "prompt-queue: failed to serialize JSON: {err}");
            1
        }
    }
}

fn map_queue_error(io: &mut CliIo<'_>, err: PromptQueueError) -> i32 {
    let _ = writeln!(io.stderr, "prompt-queue: {err}");
    1
}

fn now_unix_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .ok()
        .and_then(|duration| u64::try_from(duration.as_millis()).ok())
        .unwrap_or(0)
}

fn next_entry_id(prefix: &str) -> String {
    let stamp = now_unix_ms();
    let counter = ENTRY_COUNTER.fetch_add(1, Ordering::Relaxed);
    format!("{prefix}-{stamp}-{counter}")
}

fn run_enqueue(cmd: PromptQueueEnqueueCommand, io: &mut CliIo<'_>) -> i32 {
    let queue = queue_for_session(&cmd.session_dir);
    let id = cmd.id.unwrap_or_else(|| next_entry_id("pq"));
    let timestamp = now_unix_ms();
    match queue.enqueue(id, cmd.text, timestamp) {
        Ok(entry) => write_json(io, &entry),
        Err(err) => map_queue_error(io, err),
    }
}

fn run_interject(cmd: PromptQueueInterjectCommand, io: &mut CliIo<'_>) -> i32 {
    let queue = queue_for_session(&cmd.session_dir);
    let queue_path = queue.path().display().to_string();
    let id = cmd.id.unwrap_or_else(|| next_entry_id("inj"));
    let timestamp = now_unix_ms();
    match queue.interject_mid_turn(id, cmd.text, timestamp, cmd.turn_running) {
        Ok(interjection) => write_json(io, &InterjectionJson::new(queue_path, interjection)),
        Err(err) => map_queue_error(io, err),
    }
}

fn run_list(session_dir: PathBuf, io: &mut CliIo<'_>) -> i32 {
    let queue = queue_for_session(&session_dir);
    match queue.list() {
        Ok(entries) => write_json(
            io,
            &PromptQueueListJson {
                queue_path: queue.path().display().to_string(),
                count: entries.len(),
                entries,
            },
        ),
        Err(err) => map_queue_error(io, err),
    }
}

fn run_dequeue(session_dir: PathBuf, io: &mut CliIo<'_>) -> i32 {
    let queue = queue_for_session(&session_dir);
    match queue.dequeue() {
        Ok(Some(entry)) => write_json(
            io,
            &PromptQueueDequeueJson {
                queue_path: queue.path().display().to_string(),
                entry: DequeueEntry::Entry {
                    id: entry.id,
                    text: entry.text,
                    enqueued_at_unix_ms: entry.enqueued_at_unix_ms,
                },
            },
        ),
        Ok(None) => write_json(
            io,
            &PromptQueueDequeueJson {
                queue_path: queue.path().display().to_string(),
                entry: DequeueEntry::Empty {},
            },
        ),
        Err(err) => map_queue_error(io, err),
    }
}

#[derive(Debug, Serialize)]
struct InterjectionJson {
    queue_path: String,
    id: String,
    text: String,
    position: usize,
    turn_was_running: bool,
    mutates_conversation_events: bool,
}

impl InterjectionJson {
    fn new(queue_path: String, inner: MidTurnInterjection) -> Self {
        Self {
            queue_path,
            id: inner.entry.id,
            text: inner.entry.text,
            position: inner.position,
            turn_was_running: inner.turn_was_running,
            mutates_conversation_events: inner.mutates_conversation_events,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::CliIo;
    use std::fs;
    use std::io::Cursor;
    use tempfile::tempdir;

    fn run_cli(session_dir: &std::path::Path, args: &[&str]) -> (i32, String, String) {
        let mut stdin = Cursor::new(Vec::<u8>::new());
        let mut stdout = Vec::new();
        let mut stderr = Vec::new();
        let mut io = CliIo::new(&mut stdin, &mut stdout, &mut stderr);
        let deps = CliDeps::real().with_current_dir(session_dir.to_path_buf());
        let session_dir_str = session_dir.to_string_lossy().to_string();
        let mut argv: Vec<String> = vec!["harness".to_string(), "prompt-queue".to_string()];
        for arg in args {
            argv.push((*arg).to_string());
        }
        argv.push("--session".to_string());
        argv.push(session_dir_str);
        let outcome = crate::run(argv, &mut io, deps);
        (
            outcome.code,
            String::from_utf8_lossy(&stdout).to_string(),
            String::from_utf8_lossy(&stderr).to_string(),
        )
    }

    #[test]
    fn enqueue_list_dequeue_survive_reopen_and_preserve_fifo() {
        // arrange — a session directory that owns the durable queue
        let dir = tempdir().unwrap();
        let session_dir = dir.path().join("session");
        fs::create_dir_all(&session_dir).unwrap();
        let queue_path = session_dir.join("tui/prompt-queue.json");

        // act — enqueue two prompts through the CLI surface
        let (code, stdout, stderr) = run_cli(&session_dir, &["enqueue", "first", "--id", "a"]);
        // assert
        assert_eq!(code, 0, "stderr: {stderr}");
        assert!(stdout.contains("\"id\": \"a\""), "stdout: {stdout}");
        assert!(stdout.contains("\"text\": \"first\""), "stdout: {stdout}");

        let (code, _stdout, stderr) = run_cli(&session_dir, &["enqueue", "second", "--id", "b"]);
        assert_eq!(code, 0, "stderr: {stderr}");

        // act — the queue file is durable on disk and lists FIFO order
        assert!(queue_path.is_file(), "durable queue file must exist");
        let (code, stdout, stderr) = run_cli(&session_dir, &["list"]);
        // assert
        assert_eq!(code, 0, "stderr: {stderr}");
        assert!(stdout.contains("\"count\": 2"), "stdout: {stdout}");
        let first_at = stdout.find("\"id\": \"a\"").expect("a listed");
        let second_at = stdout.find("\"id\": \"b\"").expect("b listed");
        assert!(first_at < second_at, "FIFO order preserved: {stdout}");

        // act — dequeue recovers the front entry, durable across reopens
        let (code, stdout, stderr) = run_cli(&session_dir, &["dequeue"]);
        // assert
        assert_eq!(code, 0, "stderr: {stderr}");
        assert!(
            stdout.contains("\"dequeued\": \"entry\""),
            "stdout: {stdout}"
        );
        assert!(stdout.contains("\"id\": \"a\""), "stdout: {stdout}");

        // act — the remaining tail survives a fresh CLI process (reopen)
        let (code, stdout, stderr) = run_cli(&session_dir, &["list"]);
        // assert
        assert_eq!(code, 0, "stderr: {stderr}");
        assert!(stdout.contains("\"count\": 1"), "stdout: {stdout}");
        assert!(stdout.contains("\"id\": \"b\""), "stdout: {stdout}");
    }

    #[test]
    fn interject_front_inserts_with_honest_turn_flag_and_recovers() {
        // arrange — an ordinary FIFO tail entry already queued while a turn runs
        let dir = tempdir().unwrap();
        let session_dir = dir.path().join("session");
        fs::create_dir_all(&session_dir).unwrap();

        let (code, _stdout, stderr) = run_cli(&session_dir, &["enqueue", "later", "--id", "tail"]);
        assert_eq!(code, 0, "stderr: {stderr}");

        // act — interject mid-turn while a turn is reported running
        let (code, stdout, stderr) = run_cli(
            &session_dir,
            &["interject", "urgent", "--id", "inj", "--turn-running"],
        );

        // assert — front position, honest running flag, events never mutated
        assert_eq!(code, 0, "stderr: {stderr}");
        assert!(stdout.contains("\"position\": 0"), "stdout: {stdout}");
        assert!(
            stdout.contains("\"turn_was_running\": true"),
            "stdout: {stdout}"
        );
        assert!(
            stdout.contains("\"mutates_conversation_events\": false"),
            "stdout: {stdout}"
        );

        // act — recovery drains the interjection before the FIFO tail
        let (code, stdout, stderr) = run_cli(&session_dir, &["dequeue"]);
        // assert
        assert_eq!(code, 0, "stderr: {stderr}");
        assert!(stdout.contains("\"id\": \"inj\""), "stdout: {stdout}");
        let (code, stdout, stderr) = run_cli(&session_dir, &["dequeue"]);
        assert_eq!(code, 0, "stderr: {stderr}");
        assert!(stdout.contains("\"id\": \"tail\""), "stdout: {stdout}");
    }

    #[test]
    fn interject_records_idle_state_without_turn_running_flag() {
        // arrange
        let dir = tempdir().unwrap();
        let session_dir = dir.path().join("session");
        fs::create_dir_all(&session_dir).unwrap();

        // act — interject while no turn is running (no --turn-running flag)
        let (code, stdout, stderr) = run_cli(
            &session_dir,
            &["interject", "queued while idle", "--id", "idle"],
        );

        // assert — honest idle flag preserved durably
        assert_eq!(code, 0, "stderr: {stderr}");
        assert!(
            stdout.contains("\"turn_was_running\": false"),
            "stdout: {stdout}"
        );
        assert!(
            stdout.contains("\"mutates_conversation_events\": false"),
            "stdout: {stdout}"
        );
    }

    #[test]
    fn enqueue_blank_text_fails_closed_without_writing_queue() {
        // arrange
        let dir = tempdir().unwrap();
        let session_dir = dir.path().join("session");
        fs::create_dir_all(&session_dir).unwrap();

        // act
        let (code, _stdout, stderr) = run_cli(&session_dir, &["enqueue", "   "]);

        // assert — blank text rejected, no durable queue file created
        assert_eq!(code, 1);
        assert!(stderr.contains("non-empty"), "stderr: {stderr}");
        assert!(!session_dir.join("tui/prompt-queue.json").exists());
    }
}
