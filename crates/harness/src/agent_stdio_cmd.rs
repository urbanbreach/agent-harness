//! CLI surface for the ACP stdio agent mode.
//!
//! `agent stdio` wires through to
//! `harness_core::integrations::acp_stdio::run_stdio_acp_agent_mode_product`,
//! giving the stdio ACP transport a real public CLI server surface.

use std::io::Write;

use clap::{Args, Subcommand};
use serde::Serialize;

use crate::CliIo;

#[derive(Debug, Args, Clone)]
pub(crate) struct AgentStdioCommand {
    /// Shell command that starts the ACP server subprocess.
    #[arg(long)]
    command: String,

    /// Print the result as JSON.
    #[arg(long, default_value_t = false)]
    json: bool,
}

#[derive(Debug, Subcommand, Clone)]
pub(crate) enum AgentSubcommand {
    /// Start the stdio ACP agent mode.
    Stdio(AgentStdioCommand),
}

#[derive(Debug, Serialize)]
struct AgentStdioResult {
    command: String,
    connected: bool,
    bound: bool,
    operate_ok: bool,
    session_id: Option<String>,
    agent_name: Option<String>,
    meets_agent_mode_contract: bool,
}

pub(crate) fn execute_with_io(command: AgentSubcommand, io: &mut CliIo<'_>) -> i32 {
    match command {
        AgentSubcommand::Stdio(cmd) => run_stdio(cmd, io),
    }
}

fn run_stdio(command: AgentStdioCommand, io: &mut CliIo<'_>) -> i32 {
    let shell_command = command.command.trim().to_string();
    if shell_command.is_empty() {
        let _ = writeln!(io.stderr, "agent stdio: --command must not be empty");
        return 2;
    }

    let product =
        harness_core::integrations::acp_stdio::run_stdio_acp_agent_mode_product(&shell_command);

    let result = AgentStdioResult {
        command: product.command.clone(),
        connected: product.last_connect.is_connected(),
        bound: product.last_bind.is_bound(),
        operate_ok: product.operate_ok,
        session_id: product.session.as_ref().map(|s| s.session_id.clone()),
        agent_name: product.session.as_ref().map(|s| s.agent_name.clone()),
        meets_agent_mode_contract: product.meets_agent_mode_contract(),
    };

    if command.json {
        match serde_json::to_string_pretty(&result) {
            Ok(json) => {
                let _ = writeln!(io.stdout, "{json}");
            }
            Err(err) => {
                let _ = writeln!(io.stderr, "agent stdio: failed to serialize result: {err}");
                return 1;
            }
        }
    } else {
        let status = if result.meets_agent_mode_contract {
            "ok"
        } else {
            "degraded"
        };
        let _ = writeln!(
            io.stdout,
            "agent stdio: command={} status={} connected={} bound={} operate_ok={}",
            result.command, status, result.connected, result.bound, result.operate_ok,
        );
        if let (Some(sid), Some(name)) = (&result.session_id, &result.agent_name) {
            let _ = writeln!(io.stdout, "  session_id={sid} agent_name={name}");
        }
    }

    if result.meets_agent_mode_contract {
        0
    } else {
        1
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    fn run_stdio(command: &str, json: bool) -> (i32, String, String) {
        let mut stdin = Cursor::new(Vec::new());
        let mut stdout = Vec::new();
        let mut stderr = Vec::new();
        let mut io = CliIo::new(&mut stdin, &mut stdout, &mut stderr);
        let code = execute_with_io(
            AgentSubcommand::Stdio(AgentStdioCommand {
                command: command.to_string(),
                json,
            }),
            &mut io,
        );
        (
            code,
            String::from_utf8_lossy(&stdout).to_string(),
            String::from_utf8_lossy(&stderr).to_string(),
        )
    }

    #[test]
    fn stdio_with_cat_succeeds_and_meets_contract() {
        // arrange
        // act
        let (code, stdout, stderr) = run_stdio("cat", true);
        // assert
        assert_eq!(code, 0, "stderr: {stderr}");
        let json: serde_json::Value = serde_json::from_str(&stdout)
            .unwrap_or_else(|e| panic!("expected JSON output, got: {stdout}, parse error: {e}"));
        assert_eq!(json["connected"], true);
        assert_eq!(json["bound"], true);
        assert_eq!(json["operate_ok"], true);
        assert_eq!(json["meets_agent_mode_contract"], true);
    }

    #[test]
    fn stdio_with_invalid_command_fails() {
        // arrange
        // act
        let (code, _stdout, _stderr) = run_stdio("exit 1", false);
        // assert
        assert_ne!(code, 0);
    }

    #[test]
    fn stdio_with_empty_command_rejects() {
        // arrange
        // act
        let (code, stdout, stderr) = run_stdio("", false);
        // assert
        assert_eq!(code, 2);
        assert!(stdout.is_empty());
        assert!(stderr.contains("must not be empty"));
    }
}
