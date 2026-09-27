use crate::{CliDeps, CliIo};
use harness_core::prompt_queue::DurablePromptQueue;
use serde_json::json;
use std::{
    path::PathBuf,
    sync::atomic::{AtomicU64, Ordering},
    time::{SystemTime, UNIX_EPOCH},
};

#[derive(clap::Args)]
pub(crate) struct PromptQueueCommand {
    #[arg(long, global = true)]
    session: Option<PathBuf>,
    #[command(subcommand)]
    action: Action,
}
#[derive(clap::Subcommand)]
enum Action {
    Enqueue {
        text: String,
        #[arg(long)]
        id: Option<String>,
    },
    Interject {
        text: String,
        #[arg(long)]
        id: Option<String>,
        #[arg(long)]
        turn_running: bool,
    },
    List,
    Dequeue,
}
pub(crate) fn execute(
    command: PromptQueueCommand,
    io: &mut CliIo<'_>,
    deps: &CliDeps,
) -> Result<(), String> {
    let session = deps
        .current_dir()
        .map_err(|e| e.to_string())?
        .join(command.session.ok_or("--session is required")?);
    let queue = DurablePromptQueue::for_session(&session);
    let mut report = match command.action {
        Action::List => {
            let entries = queue.list().map_err(|e| e.to_string())?;
            json!({"entries":entries,"count":entries.len()})
        }
        Action::Dequeue => match queue.dequeue().map_err(|e| e.to_string())? {
            Some(entry) => {
                let mut report = json!(entry);
                report["dequeued"] = "entry".into();
                report
            }
            None => json!({"dequeued":"empty"}),
        },
        Action::Enqueue { id, text } => {
            let (id, timestamp) = stamp(id)?;
            json!(queue
                .enqueue(id, text, timestamp)
                .map_err(|e| e.to_string())?)
        }
        Action::Interject {
            id,
            text,
            turn_running,
        } => {
            let (id, timestamp) = stamp(id)?;
            json!(queue
                .interject_mid_turn(id, text, timestamp, turn_running)
                .map_err(|e| e.to_string())?)
        }
    };
    report["queue_path"] = json!(queue.path());
    crate::inspect::print_json(io, &report)
}
fn stamp(id: Option<String>) -> Result<(String, u64), String> {
    let timestamp = u64::try_from(
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|e| e.to_string())?
            .as_millis(),
    )
    .map_err(|e| e.to_string())?;
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let id = id.unwrap_or_else(|| {
        format!(
            "pq-{}-{timestamp}-{}",
            std::process::id(),
            COUNTER.fetch_add(1, Ordering::Relaxed)
        )
    });
    Ok((id, timestamp))
}
