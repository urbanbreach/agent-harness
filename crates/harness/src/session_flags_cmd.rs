//! CLI leaf for session, resume, fork, and worktree flags (shard E).
//!
//! These subcommands were false-success shards that returned `status:
//! "resumed"`/`"forked"`/`"updated"` without calling any real session
//! authority. They have been replaced with meaningful failure directing
//! users to the real `harness sessions` commands.

use std::io::Write;
use std::path::PathBuf;

use clap::{Args, Subcommand};

use crate::{CliDeps, CliIo};

#[derive(Debug, Args, Clone)]
pub(crate) struct ResumeFlagsCommand {
    #[arg(long)]
    session: String,
    #[arg(long)]
    cutoff: Option<u64>,
    #[arg(long, default_value_t = false)]
    read_only: bool,
}

#[derive(Debug, Args, Clone)]
pub(crate) struct ForkFlagsCommand {
    #[arg(long)]
    source: String,
    #[arg(long)]
    cutoff: Option<u64>,
    #[arg(long)]
    name: Option<String>,
}

#[derive(Debug, Args, Clone)]
pub(crate) struct WorktreeFlagsCommand {
    #[arg(long)]
    session: String,
    #[arg(long)]
    path: Option<PathBuf>,
    #[arg(long, default_value_t = false)]
    cleanup: bool,
}

#[derive(Debug, Args, Clone)]
pub(crate) struct SessionFlagsCommand {
    #[arg(long)]
    session: String,
    #[arg(long)]
    title: Option<String>,
    #[arg(long, default_value_t = false)]
    pin: bool,
}

#[derive(Debug, Subcommand, Clone)]
pub(crate) enum SessionFlagsSubcommand {
    Resume(ResumeFlagsCommand),
    Fork(ForkFlagsCommand),
    Worktree(WorktreeFlagsCommand),
    Session(SessionFlagsCommand),
}

#[derive(Debug, Args, Clone)]
pub(crate) struct SessionFlagsLeafCommand {
    #[command(subcommand)]
    pub(crate) command: SessionFlagsSubcommand,
}

pub(crate) fn execute_with_io(
    command: SessionFlagsLeafCommand,
    io: &mut CliIo<'_>,
    _deps: &CliDeps,
) -> i32 {
    match command.command {
        SessionFlagsSubcommand::Resume(cmd) => run_resume(cmd, io),
        SessionFlagsSubcommand::Fork(cmd) => run_fork(cmd, io),
        SessionFlagsSubcommand::Worktree(cmd) => run_worktree(cmd, io),
        SessionFlagsSubcommand::Session(cmd) => run_session(cmd, io),
    }
}

fn run_resume(_command: ResumeFlagsCommand, io: &mut CliIo<'_>) -> i32 {
    let _ = writeln!(
        io.stderr,
        "resume: this subcommand is not available; use 'harness sessions resume --session <id>' to resume a stored session"
    );
    2
}

fn run_fork(_command: ForkFlagsCommand, io: &mut CliIo<'_>) -> i32 {
    let _ = writeln!(
        io.stderr,
        "fork: this subcommand is not available; use 'harness sessions fork --source <id> --cutoff <seq>' to fork a stored session"
    );
    2
}

fn run_worktree(_command: WorktreeFlagsCommand, io: &mut CliIo<'_>) -> i32 {
    let _ = writeln!(
        io.stderr,
        "worktree: this subcommand is not available; use 'harness worktree' to manage worktrees"
    );
    2
}

fn run_session(_command: SessionFlagsCommand, io: &mut CliIo<'_>) -> i32 {
    let _ = writeln!(
        io.stderr,
        "session: this subcommand is not available; use 'harness sessions rename --session <id> --title <title>' to rename a session"
    );
    2
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    fn make_deps() -> CliDeps {
        CliDeps::real()
    }

    fn run_resume(session: &str) -> (i32, String, String) {
        let mut stdin = Cursor::new(Vec::new());
        let mut stdout = Vec::new();
        let mut stderr = Vec::new();
        let mut io = CliIo::new(&mut stdin, &mut stdout, &mut stderr);
        let cmd = SessionFlagsLeafCommand {
            command: SessionFlagsSubcommand::Resume(ResumeFlagsCommand {
                session: session.to_string(),
                cutoff: None,
                read_only: false,
            }),
        };
        let code = execute_with_io(cmd, &mut io, &make_deps());
        (
            code,
            String::from_utf8_lossy(&stdout).to_string(),
            String::from_utf8_lossy(&stderr).to_string(),
        )
    }

    fn run_fork(source: &str) -> (i32, String, String) {
        let mut stdin = Cursor::new(Vec::new());
        let mut stdout = Vec::new();
        let mut stderr = Vec::new();
        let mut io = CliIo::new(&mut stdin, &mut stdout, &mut stderr);
        let cmd = SessionFlagsLeafCommand {
            command: SessionFlagsSubcommand::Fork(ForkFlagsCommand {
                source: source.to_string(),
                cutoff: None,
                name: None,
            }),
        };
        let code = execute_with_io(cmd, &mut io, &make_deps());
        (
            code,
            String::from_utf8_lossy(&stdout).to_string(),
            String::from_utf8_lossy(&stderr).to_string(),
        )
    }

    fn run_worktree(session: &str) -> (i32, String, String) {
        let mut stdin = Cursor::new(Vec::new());
        let mut stdout = Vec::new();
        let mut stderr = Vec::new();
        let mut io = CliIo::new(&mut stdin, &mut stdout, &mut stderr);
        let cmd = SessionFlagsLeafCommand {
            command: SessionFlagsSubcommand::Worktree(WorktreeFlagsCommand {
                session: session.to_string(),
                path: None,
                cleanup: false,
            }),
        };
        let code = execute_with_io(cmd, &mut io, &make_deps());
        (
            code,
            String::from_utf8_lossy(&stdout).to_string(),
            String::from_utf8_lossy(&stderr).to_string(),
        )
    }

    fn run_session(session: &str) -> (i32, String, String) {
        let mut stdin = Cursor::new(Vec::new());
        let mut stdout = Vec::new();
        let mut stderr = Vec::new();
        let mut io = CliIo::new(&mut stdin, &mut stdout, &mut stderr);
        let cmd = SessionFlagsLeafCommand {
            command: SessionFlagsSubcommand::Session(SessionFlagsCommand {
                session: session.to_string(),
                title: None,
                pin: false,
            }),
        };
        let code = execute_with_io(cmd, &mut io, &make_deps());
        (
            code,
            String::from_utf8_lossy(&stdout).to_string(),
            String::from_utf8_lossy(&stderr).to_string(),
        )
    }

    #[test]
    fn resume_returns_meaningful_failure_directing_to_sessions_resume() {
        // arrange
        // act
        let (code, stdout, stderr) = run_resume("sess-123");
        // assert
        assert_eq!(code, 2);
        assert!(stdout.is_empty());
        assert!(stderr.contains("not available"));
        assert!(stderr.contains("harness sessions resume"));
    }

    #[test]
    fn fork_returns_meaningful_failure_directing_to_sessions_fork() {
        // arrange
        // act
        let (code, stdout, stderr) = run_fork("src-sess");
        // assert
        assert_eq!(code, 2);
        assert!(stdout.is_empty());
        assert!(stderr.contains("not available"));
        assert!(stderr.contains("harness sessions fork"));
    }

    #[test]
    fn worktree_returns_meaningful_failure_directing_to_worktree() {
        // arrange
        // act
        let (code, stdout, stderr) = run_worktree("sess-wt");
        // assert
        assert_eq!(code, 2);
        assert!(stdout.is_empty());
        assert!(stderr.contains("not available"));
        assert!(stderr.contains("harness worktree"));
    }

    #[test]
    fn session_returns_meaningful_failure_directing_to_sessions_rename() {
        // arrange
        // act
        let (code, stdout, stderr) = run_session("sess-flags");
        // assert
        assert_eq!(code, 2);
        assert!(stdout.is_empty());
        assert!(stderr.contains("not available"));
        assert!(stderr.contains("harness sessions rename"));
    }
}
