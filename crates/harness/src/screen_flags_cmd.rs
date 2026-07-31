//! CLI leaf for minimal, fullscreen, alt-screen, plan, and memory flags (shard G).
//!
//! These subcommands were false-success shards that returned `status:
//! "applied"`/`"completed"` without calling any real TUI or memory
//! authority. They have been replaced with meaningful failure directing
//! users to the TUI or the real `harness memory` command.

use std::io::Write;

use clap::{Args, Subcommand};

use crate::{CliDeps, CliIo};

#[derive(Debug, Args, Clone)]
pub(crate) struct DisplayModeCommand {
    #[arg(long)]
    mode: String,
    #[arg(long, default_value_t = false)]
    no_color: bool,
}

#[derive(Debug, Args, Clone)]
pub(crate) struct PlanModeCommand {
    #[arg(long)]
    enter: bool,
    #[arg(long)]
    exit: bool,
    #[arg(long)]
    plan_file: Option<String>,
}

#[derive(Debug, Args, Clone)]
pub(crate) struct MemoryFlagsCommand {
    #[arg(long)]
    action: String,
    #[arg(long)]
    key: Option<String>,
    #[arg(long)]
    value: Option<String>,
    #[arg(long)]
    query: Option<String>,
}

#[derive(Debug, Args, Clone)]
pub(crate) struct FullscreenFlagsCommand {
    #[arg(long, default_value_t = true)]
    fullscreen: bool,
    #[arg(long, default_value_t = false)]
    alt_screen: bool,
    #[arg(long, default_value_t = false)]
    mouse: bool,
}

#[derive(Debug, Subcommand, Clone)]
pub(crate) enum ScreenFlagsSubcommand {
    DisplayMode(DisplayModeCommand),
    PlanMode(PlanModeCommand),
    Memory(MemoryFlagsCommand),
    Fullscreen(FullscreenFlagsCommand),
}

#[derive(Debug, Args, Clone)]
pub(crate) struct ScreenFlagsLeafCommand {
    #[command(subcommand)]
    pub(crate) command: ScreenFlagsSubcommand,
}

pub(crate) fn execute_with_io(command: ScreenFlagsLeafCommand, io: &mut CliIo<'_>) -> i32 {
    match command.command {
        ScreenFlagsSubcommand::DisplayMode(cmd) => run_display_mode(cmd, io),
        ScreenFlagsSubcommand::PlanMode(cmd) => run_plan_mode(cmd, io),
        ScreenFlagsSubcommand::Memory(cmd) => run_memory(cmd, io),
        ScreenFlagsSubcommand::Fullscreen(cmd) => run_fullscreen(cmd, io),
    }
}

fn run_display_mode(_command: DisplayModeCommand, io: &mut CliIo<'_>) -> i32 {
    let _ = writeln!(
        io.stderr,
        "display-mode: this subcommand is not available; display mode is a TUI setting, launch the TUI with 'harness' to configure it"
    );
    2
}

fn run_plan_mode(_command: PlanModeCommand, io: &mut CliIo<'_>) -> i32 {
    let _ = writeln!(
        io.stderr,
        "plan-mode: this subcommand is not available; plan mode is a TUI workflow, launch the TUI with 'harness' and use the /plan slash command"
    );
    2
}

fn run_memory(_command: MemoryFlagsCommand, io: &mut CliIo<'_>) -> i32 {
    let _ = writeln!(
        io.stderr,
        "memory: this subcommand is not available; use 'harness memory' to manage workspace memory"
    );
    2
}

fn run_fullscreen(_command: FullscreenFlagsCommand, io: &mut CliIo<'_>) -> i32 {
    let _ = writeln!(
        io.stderr,
        "fullscreen: this subcommand is not available; fullscreen mode is a TUI setting, launch the TUI with 'harness' to configure it"
    );
    2
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    fn run_display_mode(mode: &str) -> (i32, String, String) {
        let mut stdin = Cursor::new(Vec::new());
        let mut stdout = Vec::new();
        let mut stderr = Vec::new();
        let mut io = CliIo::new(&mut stdin, &mut stdout, &mut stderr);
        let cmd = ScreenFlagsLeafCommand {
            command: ScreenFlagsSubcommand::DisplayMode(DisplayModeCommand {
                mode: mode.to_string(),
                no_color: false,
            }),
        };
        let code = execute_with_io(cmd, &mut io);
        (
            code,
            String::from_utf8_lossy(&stdout).to_string(),
            String::from_utf8_lossy(&stderr).to_string(),
        )
    }

    fn run_plan_mode(enter: bool, exit: bool) -> (i32, String, String) {
        let mut stdin = Cursor::new(Vec::new());
        let mut stdout = Vec::new();
        let mut stderr = Vec::new();
        let mut io = CliIo::new(&mut stdin, &mut stdout, &mut stderr);
        let cmd = ScreenFlagsLeafCommand {
            command: ScreenFlagsSubcommand::PlanMode(PlanModeCommand {
                enter,
                exit,
                plan_file: None,
            }),
        };
        let code = execute_with_io(cmd, &mut io);
        (
            code,
            String::from_utf8_lossy(&stdout).to_string(),
            String::from_utf8_lossy(&stderr).to_string(),
        )
    }

    fn run_memory(action: &str) -> (i32, String, String) {
        let mut stdin = Cursor::new(Vec::new());
        let mut stdout = Vec::new();
        let mut stderr = Vec::new();
        let mut io = CliIo::new(&mut stdin, &mut stdout, &mut stderr);
        let cmd = ScreenFlagsLeafCommand {
            command: ScreenFlagsSubcommand::Memory(MemoryFlagsCommand {
                action: action.to_string(),
                key: None,
                value: None,
                query: None,
            }),
        };
        let code = execute_with_io(cmd, &mut io);
        (
            code,
            String::from_utf8_lossy(&stdout).to_string(),
            String::from_utf8_lossy(&stderr).to_string(),
        )
    }

    fn run_fullscreen() -> (i32, String, String) {
        let mut stdin = Cursor::new(Vec::new());
        let mut stdout = Vec::new();
        let mut stderr = Vec::new();
        let mut io = CliIo::new(&mut stdin, &mut stdout, &mut stderr);
        let cmd = ScreenFlagsLeafCommand {
            command: ScreenFlagsSubcommand::Fullscreen(FullscreenFlagsCommand {
                fullscreen: true,
                alt_screen: false,
                mouse: false,
            }),
        };
        let code = execute_with_io(cmd, &mut io);
        (
            code,
            String::from_utf8_lossy(&stdout).to_string(),
            String::from_utf8_lossy(&stderr).to_string(),
        )
    }

    #[test]
    fn display_mode_returns_meaningful_failure_directing_to_tui() {
        let (code, stdout, stderr) = run_display_mode("minimal");
        assert_eq!(code, 2);
        assert!(stdout.is_empty());
        assert!(stderr.contains("not available"));
        assert!(stderr.contains("TUI"));
    }

    #[test]
    fn plan_mode_returns_meaningful_failure_directing_to_tui() {
        let (code, stdout, stderr) = run_plan_mode(true, false);
        assert_eq!(code, 2);
        assert!(stdout.is_empty());
        assert!(stderr.contains("not available"));
        assert!(stderr.contains("TUI"));
    }

    #[test]
    fn memory_returns_meaningful_failure_directing_to_memory_command() {
        let (code, stdout, stderr) = run_memory("put");
        assert_eq!(code, 2);
        assert!(stdout.is_empty());
        assert!(stderr.contains("not available"));
        assert!(stderr.contains("harness memory"));
    }

    #[test]
    fn fullscreen_returns_meaningful_failure_directing_to_tui() {
        let (code, stdout, stderr) = run_fullscreen();
        assert_eq!(code, 2);
        assert!(stdout.is_empty());
        assert!(stderr.contains("not available"));
        assert!(stderr.contains("TUI"));
    }
}
