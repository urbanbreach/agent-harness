//! Product CLI for the durable multi-agent team mailbox journal (`harness team`).
//!
//! Surfaces create/add-member/send/deliver/list/cancel over
//! [`harness_core::team_mailbox_journal::DurableTeamRegistry`], persisted at
//! `<workspace>/.agent-harness/team-mailbox.json`.

use std::io::Write;
use std::path::PathBuf;

use clap::{Args, Subcommand};
use harness_core::team_mailbox_journal::{DurableTeamRegistry, TeamMailboxJournalError};
use harness_core::team_registry::{TeamMessage, TeamRecord};
use serde::Serialize;

use crate::{CliDeps, CliIo};

#[derive(Debug, Args, Clone)]
pub(crate) struct TeamCommand {
    #[command(subcommand)]
    command: TeamSubcommand,
}

#[derive(Debug, Subcommand, Clone)]
enum TeamSubcommand {
    /// Create a new team in the durable registry (JSON result).
    Create(CreateArgs),
    /// Add an agent member to an existing team (JSON result).
    AddMember(AddMemberArgs),
    /// Send a mailbox message to a team member or broadcast (JSON result).
    Send(SendArgs),
    /// Deliver (drain) a member's pending mailbox messages (JSON result).
    Deliver(DeliverArgs),
    /// List all teams in the durable registry (JSON result).
    List(ListArgs),
    /// Cancel a team (JSON result).
    Cancel(CancelArgs),
}

#[derive(Debug, Args, Clone)]
struct CreateArgs {
    /// Team display name.
    name: String,
    /// Workspace root (default: current directory).
    #[arg(long)]
    workspace: Option<PathBuf>,
}

#[derive(Debug, Args, Clone)]
struct AddMemberArgs {
    /// Team id.
    team: String,
    /// Agent id to add.
    agent: String,
    /// Member role.
    role: String,
    /// Workspace root (default: current directory).
    #[arg(long)]
    workspace: Option<PathBuf>,
}

#[derive(Debug, Args, Clone)]
struct SendArgs {
    /// Team id.
    team: String,
    /// Sending agent id (must be a member).
    from: String,
    /// Message body text.
    body: String,
    /// Target agent id; omit to broadcast to the whole team.
    #[arg(long)]
    to: Option<String>,
    /// Workspace root (default: current directory).
    #[arg(long)]
    workspace: Option<PathBuf>,
}

#[derive(Debug, Args, Clone)]
struct DeliverArgs {
    /// Team id.
    team: String,
    /// Agent id whose pending messages to drain.
    agent: String,
    /// Workspace root (default: current directory).
    #[arg(long)]
    workspace: Option<PathBuf>,
}

#[derive(Debug, Args, Clone)]
struct ListArgs {
    /// Workspace root (default: current directory).
    #[arg(long)]
    workspace: Option<PathBuf>,
}

#[derive(Debug, Args, Clone)]
struct CancelArgs {
    /// Team id to cancel.
    team: String,
    /// Workspace root (default: current directory).
    #[arg(long)]
    workspace: Option<PathBuf>,
}

pub(crate) fn execute_with_io(command: TeamCommand, io: &mut CliIo<'_>, deps: &CliDeps) -> i32 {
    match command.command {
        TeamSubcommand::Create(args) => run_create(args, io, deps),
        TeamSubcommand::AddMember(args) => run_add_member(args, io, deps),
        TeamSubcommand::Send(args) => run_send(args, io, deps),
        TeamSubcommand::Deliver(args) => run_deliver(args, io, deps),
        TeamSubcommand::List(args) => run_list(args, io, deps),
        TeamSubcommand::Cancel(args) => run_cancel(args, io, deps),
    }
}

fn resolve_workspace_root(explicit: &Option<PathBuf>, deps: &CliDeps) -> PathBuf {
    explicit
        .clone()
        .unwrap_or_else(|| deps.current_dir().unwrap_or_else(|_| PathBuf::from(".")))
}

fn run_create(args: CreateArgs, io: &mut CliIo<'_>, deps: &CliDeps) -> i32 {
    let root = resolve_workspace_root(&args.workspace, deps);
    let mut durable = match open(&root, io) {
        Ok(durable) => durable,
        Err(code) => return code,
    };
    match durable.create_team(args.name) {
        Ok(record) => write_json(
            io,
            &TeamRecordJson {
                workspace_root: root.display().to_string(),
                record,
            },
        ),
        Err(err) => map_journal_error(io, err),
    }
}

fn run_add_member(args: AddMemberArgs, io: &mut CliIo<'_>, deps: &CliDeps) -> i32 {
    let root = resolve_workspace_root(&args.workspace, deps);
    let mut durable = match open(&root, io) {
        Ok(durable) => durable,
        Err(code) => return code,
    };
    match durable.add_member(&args.team, args.agent, args.role) {
        Ok(record) => write_json(
            io,
            &TeamRecordJson {
                workspace_root: root.display().to_string(),
                record,
            },
        ),
        Err(err) => map_journal_error(io, err),
    }
}

fn run_send(args: SendArgs, io: &mut CliIo<'_>, deps: &CliDeps) -> i32 {
    let root = resolve_workspace_root(&args.workspace, deps);
    let mut durable = match open(&root, io) {
        Ok(durable) => durable,
        Err(code) => return code,
    };
    match durable.send_message(&args.team, args.from, args.to, args.body) {
        Ok(message) => write_json(
            io,
            &TeamMessageJson {
                workspace_root: root.display().to_string(),
                message,
            },
        ),
        Err(err) => map_journal_error(io, err),
    }
}

fn run_deliver(args: DeliverArgs, io: &mut CliIo<'_>, deps: &CliDeps) -> i32 {
    let root = resolve_workspace_root(&args.workspace, deps);
    let mut durable = match open(&root, io) {
        Ok(durable) => durable,
        Err(code) => return code,
    };
    match durable.deliver_messages(&args.team, &args.agent) {
        Ok(messages) => {
            let count = messages.len();
            write_json(
                io,
                &DeliverJson {
                    workspace_root: root.display().to_string(),
                    count,
                    messages,
                },
            )
        }
        Err(err) => map_journal_error(io, err),
    }
}

fn run_list(args: ListArgs, io: &mut CliIo<'_>, deps: &CliDeps) -> i32 {
    let root = resolve_workspace_root(&args.workspace, deps);
    let durable = match open(&root, io) {
        Ok(durable) => durable,
        Err(code) => return code,
    };
    let teams = durable.registry().list_teams();
    let count = teams.len();
    write_json(
        io,
        &ListJson {
            workspace_root: root.display().to_string(),
            journal_path: durable.journal_path().display().to_string(),
            count,
            teams,
        },
    )
}

fn run_cancel(args: CancelArgs, io: &mut CliIo<'_>, deps: &CliDeps) -> i32 {
    let root = resolve_workspace_root(&args.workspace, deps);
    let mut durable = match open(&root, io) {
        Ok(durable) => durable,
        Err(code) => return code,
    };
    match durable.cancel_team(&args.team) {
        Ok(record) => write_json(
            io,
            &TeamRecordJson {
                workspace_root: root.display().to_string(),
                record,
            },
        ),
        Err(err) => map_journal_error(io, err),
    }
}

fn open(root: &std::path::Path, io: &mut CliIo<'_>) -> Result<DurableTeamRegistry, i32> {
    DurableTeamRegistry::open(root).map_err(|err| map_journal_error(io, err))
}

fn map_journal_error(io: &mut CliIo<'_>, err: TeamMailboxJournalError) -> i32 {
    let _ = writeln!(io.stderr, "team: {err}");
    1
}

#[derive(Debug, Serialize)]
struct TeamRecordJson {
    workspace_root: String,
    #[serde(flatten)]
    record: TeamRecord,
}

#[derive(Debug, Serialize)]
struct TeamMessageJson {
    workspace_root: String,
    #[serde(flatten)]
    message: TeamMessage,
}

#[derive(Debug, Serialize)]
struct DeliverJson {
    workspace_root: String,
    count: usize,
    messages: Vec<TeamMessage>,
}

#[derive(Debug, Serialize)]
struct ListJson {
    workspace_root: String,
    journal_path: String,
    count: usize,
    teams: Vec<TeamRecord>,
}

fn write_json(io: &mut CliIo<'_>, value: &impl Serialize) -> i32 {
    match serde_json::to_string_pretty(value) {
        Ok(json) => {
            let _ = writeln!(io.stdout, "{json}");
            0
        }
        Err(err) => {
            let _ = writeln!(io.stderr, "team: failed to serialize JSON: {err}");
            1
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

    fn run_cli(workspace: &std::path::Path, args: &[&str]) -> (i32, String, String) {
        let mut stdin = Cursor::new(Vec::<u8>::new());
        let mut stdout = Vec::new();
        let mut stderr = Vec::new();
        let mut io = CliIo::new(&mut stdin, &mut stdout, &mut stderr);
        let deps = CliDeps::real().with_current_dir(workspace.to_path_buf());
        let mut argv: Vec<String> = vec!["harness".to_string(), "team".to_string()];
        for arg in args {
            argv.push((*arg).to_string());
        }
        let outcome = crate::run(argv, &mut io, deps);
        (
            outcome.code,
            String::from_utf8_lossy(&stdout).to_string(),
            String::from_utf8_lossy(&stderr).to_string(),
        )
    }

    fn extract_team_id(json: &str) -> String {
        let value: serde_json::Value =
            serde_json::from_str(json).expect("valid JSON from team command");
        value["team_id"]
            .as_str()
            .expect("team_id in JSON")
            .to_string()
    }

    #[test]
    fn create_add_member_send_deliver_list_cancel_lifecycle_succeeds_with_durable_journal() {
        // arrange — a workspace that owns the durable team journal
        let dir = tempdir().unwrap();
        let ws = dir.path();
        let journal = ws.join(".agent-harness/team-mailbox.json");

        // act — create a team; the registry generates a team_id (not the display name)
        let (code, stdout, stderr) = run_cli(ws, &["create", "alpha"]);
        // assert
        assert_eq!(code, 0, "stderr: {stderr}");
        assert!(
            stdout.contains("\"status\": \"active\""),
            "stdout: {stdout}"
        );
        assert!(journal.is_file(), "journal file must exist after create");
        let team_id = extract_team_id(&stdout);

        // act — add two members; each persists
        let (code, stdout, stderr) = run_cli(ws, &["add-member", team_id.as_str(), "lead", "lead"]);
        // assert
        assert_eq!(code, 0, "stderr: {stderr}");
        assert!(
            stdout.contains("\"agent_id\": \"lead\""),
            "stdout: {stdout}"
        );
        let (code, _, stderr) = run_cli(ws, &["add-member", team_id.as_str(), "worker", "worker"]);
        assert_eq!(code, 0, "stderr: {stderr}");

        // act — lead sends to worker
        let (code, stdout, stderr) = run_cli(
            ws,
            &[
                "send",
                team_id.as_str(),
                "lead",
                "hello worker",
                "--to",
                "worker",
            ],
        );
        // assert
        assert_eq!(code, 0, "stderr: {stderr}");
        assert!(
            stdout.contains("\"from_agent_id\": \"lead\""),
            "stdout: {stdout}"
        );
        assert!(
            stdout.contains("\"to_agent_id\": \"worker\""),
            "stdout: {stdout}"
        );

        // act — deliver drains the pending message
        let (code, stdout, stderr) = run_cli(ws, &["deliver", team_id.as_str(), "worker"]);
        // assert
        assert_eq!(code, 0, "stderr: {stderr}");
        assert!(stdout.contains("\"count\": 1"), "stdout: {stdout}");
        assert!(
            stdout.contains("\"body\": \"hello worker\""),
            "stdout: {stdout}"
        );

        // act — list shows the team
        let (code, stdout, stderr) = run_cli(ws, &["list"]);
        // assert
        assert_eq!(code, 0, "stderr: {stderr}");
        assert!(stdout.contains("\"count\": 1"), "stdout: {stdout}");
        assert!(
            stdout.contains("\"status\": \"active\""),
            "stdout: {stdout}"
        );

        // act — cancel the team
        let (code, stdout, stderr) = run_cli(ws, &["cancel", team_id.as_str()]);
        // assert
        assert_eq!(code, 0, "stderr: {stderr}");
        assert!(
            stdout.contains("\"status\": \"cancelled\""),
            "stdout: {stdout}"
        );
    }

    #[test]
    fn non_member_send_fails_closed_without_journal_corruption() {
        // arrange — a team with only "lead" as a member
        let dir = tempdir().unwrap();
        let ws = dir.path();
        let (code, stdout, _stderr) = run_cli(ws, &["create", "strict"]);
        assert_eq!(code, 0);
        let team_id = extract_team_id(&stdout);
        let (code, _, stderr) = run_cli(ws, &["add-member", team_id.as_str(), "lead", "lead"]);
        assert_eq!(code, 0, "stderr: {stderr}");

        // act — a non-member attempts to send
        let (code, _, stderr) = run_cli(ws, &["send", team_id.as_str(), "ghost", "intrusion"]);

        // assert — fail-closed with an error, journal remains intact
        assert_eq!(code, 1);
        assert!(stderr.contains("team:"), "stderr: {stderr}");
    }

    #[test]
    fn deliver_empty_inbox_returns_zero_count() {
        // arrange — a team with a member who has never received a message
        let dir = tempdir().unwrap();
        let ws = dir.path();
        let (code, _, stderr) = run_cli(ws, &["create", "quiet"]);
        assert_eq!(code, 0, "stderr: {stderr}");
        let (code, stdout, stderr) = run_cli(ws, &["add-member", "team_1", "observer", "watcher"]);
        assert_eq!(code, 0, "stderr: {stderr}");
        let team_id = extract_team_id(&stdout);

        // act — deliver an empty inbox
        let (code, stdout, stderr) = run_cli(ws, &["deliver", team_id.as_str(), "observer"]);

        // assert — zero delivered, structured empty JSON, no crash
        assert_eq!(code, 0, "stderr: {stderr}");
        assert!(stdout.contains("\"count\": 0"), "stdout: {stdout}");
    }

    #[test]
    fn explicit_workspace_path_isolates_journal_location() {
        // arrange — separate workspace directory from the CLI process cwd
        let dir = tempdir().unwrap();
        let ws = dir.path();
        let ws_root = ws.join("explicit-ws");
        fs::create_dir_all(&ws_root).unwrap();

        // act — create a team with an explicit --workspace flag
        let (code, stdout, stderr) = run_cli(
            ws,
            &[
                "create",
                "isolated",
                "--workspace",
                ws_root.to_str().unwrap(),
            ],
        );

        // assert — journal written to the explicit workspace, not the cwd
        assert_eq!(code, 0, "stderr: {stderr}");
        assert!(
            stdout.contains("\"status\": \"active\""),
            "stdout: {stdout}"
        );
        assert!(
            ws_root.join(".agent-harness/team-mailbox.json").is_file(),
            "journal must exist in explicit workspace"
        );
        assert!(
            !ws.join(".agent-harness/team-mailbox.json").is_file(),
            "journal must not leak into unrelated cwd"
        );
    }
}
