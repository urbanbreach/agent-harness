//! CLI leaf for best-of-n, check, schema, and output-format commands (shard C).
//!
//! Schema is retained with real authority: it calls
//! `harness_core::config::harness_schema_pretty_json()` to emit the
//! actual JSON schema. BestOfN, Check, and OutputFormat were
//! false-success shards that returned `status: "completed"`/`"ok"`/
//! `"applied"` without calling any real authority; they have been
//! replaced with meaningful failure.

use std::io::Write;

use clap::{Args, Subcommand};

use crate::{CliDeps, CliIo};

#[derive(Debug, Args, Clone)]
pub(crate) struct BestOfNCommand {
    #[arg(long)]
    prompt: String,
    #[arg(long, default_value_t = 3)]
    n: u32,
    #[arg(long, default_value = "first")]
    strategy: String,
}

#[derive(Debug, Args, Clone)]
pub(crate) struct CheckCommand {
    #[arg(long)]
    component: String,
    #[arg(long)]
    json: bool,
}

#[derive(Debug, Args, Clone)]
pub(crate) struct SchemaCommand {
    #[arg(long, default_value_t = false)]
    tui: bool,
}

#[derive(Debug, Args, Clone)]
pub(crate) struct OutputFormatCommand {
    #[arg(long)]
    format: String,
    #[arg(long, default_value_t = false)]
    pretty: bool,
}

#[derive(Debug, Subcommand, Clone)]
pub(crate) enum CheckSubcommand {
    BestOfN(BestOfNCommand),
    Check(CheckCommand),
    Schema(SchemaCommand),
    OutputFormat(OutputFormatCommand),
}

#[derive(Debug, Args, Clone)]
pub(crate) struct CheckLeafCommand {
    #[command(subcommand)]
    pub(crate) command: CheckSubcommand,
}

pub(crate) fn execute_with_io(command: CheckLeafCommand, io: &mut CliIo<'_>) -> i32 {
    match command.command {
        CheckSubcommand::BestOfN(cmd) => run_best_of_n(cmd, io),
        CheckSubcommand::Check(cmd) => run_check(cmd, io),
        CheckSubcommand::Schema(cmd) => run_schema(cmd, io),
        CheckSubcommand::OutputFormat(cmd) => run_output_format(cmd, io),
    }
}

fn run_best_of_n(_command: BestOfNCommand, io: &mut CliIo<'_>) -> i32 {
    let _ = writeln!(
        io.stderr,
        "best-of-n: this subcommand is not available; use 'harness run' for multi-candidate evaluation"
    );
    2
}

fn run_check(_command: CheckCommand, io: &mut CliIo<'_>) -> i32 {
    let _ = writeln!(
        io.stderr,
        "check: this subcommand is not available; use 'harness doctor' for component health checks"
    );
    2
}

fn run_schema(command: SchemaCommand, io: &mut CliIo<'_>) -> i32 {
    let result = if command.tui {
        harness_core::config::harness_tui_schema_pretty_json()
    } else {
        harness_core::config::harness_schema_pretty_json()
    };
    match result {
        Ok(schema) => {
            let _ = writeln!(io.stdout, "{schema}");
            0
        }
        Err(err) => {
            let _ = writeln!(io.stderr, "schema generation failed: {err}");
            1
        }
    }
}

fn run_output_format(_command: OutputFormatCommand, io: &mut CliIo<'_>) -> i32 {
    let _ = writeln!(
        io.stderr,
        "output-format: this subcommand is not available; use --output-format <format> flag on individual commands"
    );
    2
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    fn run_schema(tui: bool) -> (i32, String, String) {
        let mut stdin = Cursor::new(Vec::new());
        let mut stdout = Vec::new();
        let mut stderr = Vec::new();
        let mut io = CliIo::new(&mut stdin, &mut stdout, &mut stderr);
        let cmd = CheckLeafCommand {
            command: CheckSubcommand::Schema(SchemaCommand { tui }),
        };
        let code = execute_with_io(cmd, &mut io);
        (
            code,
            String::from_utf8_lossy(&stdout).to_string(),
            String::from_utf8_lossy(&stderr).to_string(),
        )
    }

    fn run_best_of_n(prompt: &str) -> (i32, String, String) {
        let mut stdin = Cursor::new(Vec::new());
        let mut stdout = Vec::new();
        let mut stderr = Vec::new();
        let mut io = CliIo::new(&mut stdin, &mut stdout, &mut stderr);
        let cmd = CheckLeafCommand {
            command: CheckSubcommand::BestOfN(BestOfNCommand {
                prompt: prompt.to_string(),
                n: 3,
                strategy: "first".to_string(),
            }),
        };
        let code = execute_with_io(cmd, &mut io);
        (
            code,
            String::from_utf8_lossy(&stdout).to_string(),
            String::from_utf8_lossy(&stderr).to_string(),
        )
    }

    fn run_check(component: &str) -> (i32, String, String) {
        let mut stdin = Cursor::new(Vec::new());
        let mut stdout = Vec::new();
        let mut stderr = Vec::new();
        let mut io = CliIo::new(&mut stdin, &mut stdout, &mut stderr);
        let cmd = CheckLeafCommand {
            command: CheckSubcommand::Check(CheckCommand {
                component: component.to_string(),
                json: true,
            }),
        };
        let code = execute_with_io(cmd, &mut io);
        (
            code,
            String::from_utf8_lossy(&stdout).to_string(),
            String::from_utf8_lossy(&stderr).to_string(),
        )
    }

    fn run_output_format(format: &str) -> (i32, String, String) {
        let mut stdin = Cursor::new(Vec::new());
        let mut stdout = Vec::new();
        let mut stderr = Vec::new();
        let mut io = CliIo::new(&mut stdin, &mut stdout, &mut stderr);
        let cmd = CheckLeafCommand {
            command: CheckSubcommand::OutputFormat(OutputFormatCommand {
                format: format.to_string(),
                pretty: false,
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
    fn schema_happy_runtime_emits_real_schema_json() {
        // arrange
        // act
        let (code, stdout, _stderr) = run_schema(false);
        // assert
        assert_eq!(code, 0);
        let parsed: serde_json::Value = serde_json::from_str(stdout.trim()).unwrap();
        assert!(
            parsed["$schema"].is_string()
                || parsed["type"].is_string()
                || parsed["properties"].is_object()
        );
    }

    #[test]
    fn schema_happy_tui_emits_real_tui_schema_json() {
        // arrange
        // act
        let (code, stdout, _stderr) = run_schema(true);
        // assert
        assert_eq!(code, 0);
        let parsed: serde_json::Value = serde_json::from_str(stdout.trim()).unwrap();
        assert!(
            parsed["$schema"].is_string()
                || parsed["type"].is_string()
                || parsed["properties"].is_object()
        );
    }

    #[test]
    fn best_of_n_returns_meaningful_failure_directing_to_run() {
        // arrange
        // act
        let (code, stdout, stderr) = run_best_of_n("hello");
        // assert
        assert_eq!(code, 2);
        assert!(stdout.is_empty());
        assert!(stderr.contains("not available"));
        assert!(stderr.contains("harness run"));
    }

    #[test]
    fn check_returns_meaningful_failure_directing_to_doctor() {
        // arrange
        // act
        let (code, stdout, stderr) = run_check("config");
        // assert
        assert_eq!(code, 2);
        assert!(stdout.is_empty());
        assert!(stderr.contains("not available"));
        assert!(stderr.contains("harness doctor"));
    }

    #[test]
    fn output_format_returns_meaningful_failure_directing_to_flag() {
        // arrange
        // act
        let (code, stdout, stderr) = run_output_format("json");
        // assert
        assert_eq!(code, 2);
        assert!(stdout.is_empty());
        assert!(stderr.contains("not available"));
        assert!(stderr.contains("--output-format"));
    }
}
