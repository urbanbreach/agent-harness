//! CLI for agent edit attribution diff and blame (`harness attribution`).
//!
//! Surfaces `diff` and `blame` subcommands over
//! [`harness_core::edit_attribution::EditAttributionJournal`]. Unknown or
//! untracked paths fail closed with a truthful error; external drift is
//! reported separately from agent-attributed changes.

use std::io::Write;
use std::path::PathBuf;

use clap::{Args, Subcommand};
use harness_core::edit_attribution::{EditAttributionError, EditAttributionJournal};
use serde::Serialize;

use crate::{CliDeps, CliIo};

#[derive(Debug, Args, Clone)]
pub(crate) struct AttributionCommand {
    #[command(subcommand)]
    command: AttributionSubcommand,
}

#[derive(Debug, Subcommand, Clone)]
enum AttributionSubcommand {
    /// Show a unified diff between the agent snapshot and the current file.
    Diff(AttributionPathCommand),
    /// Show per-line blame (agent_tool vs external) for the current file.
    Blame(AttributionPathCommand),
}

#[derive(Debug, Args, Clone)]
struct AttributionPathCommand {
    /// Workspace-relative path to inspect.
    path: String,
    /// Workspace root (defaults to current directory).
    #[arg(long)]
    workspace: Option<PathBuf>,
}

pub(crate) fn execute_with_io(
    command: AttributionCommand,
    io: &mut CliIo<'_>,
    deps: &CliDeps,
) -> i32 {
    match command.command {
        AttributionSubcommand::Diff(cmd) => run_diff(cmd, io, deps),
        AttributionSubcommand::Blame(cmd) => run_blame(cmd, io, deps),
    }
}

fn resolve_workspace(
    workspace: Option<PathBuf>,
    io: &mut CliIo<'_>,
    deps: &CliDeps,
) -> Result<PathBuf, i32> {
    match workspace {
        Some(path) => Ok(path),
        None => deps.current_dir().map_err(|err| {
            let _ = writeln!(
                io.stderr,
                "attribution: failed to resolve current working directory: {err}"
            );
            2
        }),
    }
}

fn write_json(io: &mut CliIo<'_>, value: &impl Serialize) -> i32 {
    match serde_json::to_string_pretty(value) {
        Ok(json) => {
            let _ = writeln!(io.stdout, "{json}");
            0
        }
        Err(err) => {
            let _ = writeln!(io.stderr, "attribution: failed to serialize JSON: {err}");
            1
        }
    }
}

fn map_error(io: &mut CliIo<'_>, err: EditAttributionError) -> i32 {
    let _ = writeln!(io.stderr, "attribution: {err}");
    1
}

fn run_diff(cmd: AttributionPathCommand, io: &mut CliIo<'_>, deps: &CliDeps) -> i32 {
    let workspace = match resolve_workspace(cmd.workspace, io, deps) {
        Ok(path) => path,
        Err(code) => return code,
    };
    let journal = match EditAttributionJournal::open(&workspace) {
        Ok(j) => j,
        Err(err) => return map_error(io, err),
    };
    match journal.diff(&cmd.path) {
        Ok(result) => write_json(io, &result),
        Err(err) => {
            let _ = writeln!(io.stderr, "attribution: {err}");
            1
        }
    }
}

fn run_blame(cmd: AttributionPathCommand, io: &mut CliIo<'_>, deps: &CliDeps) -> i32 {
    let workspace = match resolve_workspace(cmd.workspace, io, deps) {
        Ok(path) => path,
        Err(code) => return code,
    };
    let journal = match EditAttributionJournal::open(&workspace) {
        Ok(j) => j,
        Err(err) => return map_error(io, err),
    };
    match journal.blame(&cmd.path) {
        Ok(result) => write_json(io, &result),
        Err(err) => {
            let _ = writeln!(io.stderr, "attribution: {err}");
            1
        }
    }
}
