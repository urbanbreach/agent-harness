//! CLI leaf for permission, approval, tool, and web restrictions (shard D).
//!
//! These subcommands were false-success shards that validated input and
//! returned `status: "applied"` without calling any real permission or
//! config authority. They have been replaced with meaningful failure
//! directing users to the real config surface (`harness config`).

use std::io::Write;

use clap::{Args, Subcommand};

use crate::{CliDeps, CliIo};

#[derive(Debug, Args, Clone)]
pub(crate) struct PermissionRestrictionsCommand {
    #[arg(long)]
    permission: String,
    #[arg(long)]
    level: String,
}

#[derive(Debug, Args, Clone)]
pub(crate) struct ApprovalModeCommand {
    #[arg(long)]
    mode: String,
}

#[derive(Debug, Args, Clone)]
pub(crate) struct ToolRestrictionsCommand {
    #[arg(long)]
    tool: String,
    #[arg(long)]
    action: String,
}

#[derive(Debug, Args, Clone)]
pub(crate) struct WebRestrictionsCommand {
    #[arg(long)]
    policy: String,
    #[arg(long)]
    domain: Option<String>,
}

#[derive(Debug, Subcommand, Clone)]
pub(crate) enum RestrictionsSubcommand {
    Permission(PermissionRestrictionsCommand),
    Approval(ApprovalModeCommand),
    Tool(ToolRestrictionsCommand),
    Web(WebRestrictionsCommand),
}

#[derive(Debug, Args, Clone)]
pub(crate) struct RestrictionsLeafCommand {
    #[command(subcommand)]
    pub(crate) command: RestrictionsSubcommand,
}

pub(crate) fn execute_with_io(command: RestrictionsLeafCommand, io: &mut CliIo<'_>) -> i32 {
    match command.command {
        RestrictionsSubcommand::Permission(cmd) => run_permission(cmd, io),
        RestrictionsSubcommand::Approval(cmd) => run_approval(cmd, io),
        RestrictionsSubcommand::Tool(cmd) => run_tool(cmd, io),
        RestrictionsSubcommand::Web(cmd) => run_web(cmd, io),
    }
}

fn run_permission(_command: PermissionRestrictionsCommand, io: &mut CliIo<'_>) -> i32 {
    let _ = writeln!(
        io.stderr,
        "permission: this subcommand is not available; use 'harness config' to manage permission settings in harness.jsonc"
    );
    2
}

fn run_approval(_command: ApprovalModeCommand, io: &mut CliIo<'_>) -> i32 {
    let _ = writeln!(
        io.stderr,
        "approval: this subcommand is not available; use 'harness config' to manage approval mode in harness.jsonc"
    );
    2
}

fn run_tool(_command: ToolRestrictionsCommand, io: &mut CliIo<'_>) -> i32 {
    let _ = writeln!(
        io.stderr,
        "tool: this subcommand is not available; use 'harness config' to manage tool restrictions in harness.jsonc"
    );
    2
}

fn run_web(_command: WebRestrictionsCommand, io: &mut CliIo<'_>) -> i32 {
    let _ = writeln!(
        io.stderr,
        "web: this subcommand is not available; use 'harness config' to manage web access restrictions in harness.jsonc"
    );
    2
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    fn run_permission(permission: &str, level: &str) -> (i32, String, String) {
        let mut stdin = Cursor::new(Vec::new());
        let mut stdout = Vec::new();
        let mut stderr = Vec::new();
        let mut io = CliIo::new(&mut stdin, &mut stdout, &mut stderr);
        let cmd = RestrictionsLeafCommand {
            command: RestrictionsSubcommand::Permission(PermissionRestrictionsCommand {
                permission: permission.to_string(),
                level: level.to_string(),
            }),
        };
        let code = execute_with_io(cmd, &mut io);
        (
            code,
            String::from_utf8_lossy(&stdout).to_string(),
            String::from_utf8_lossy(&stderr).to_string(),
        )
    }

    fn run_approval(mode: &str) -> (i32, String, String) {
        let mut stdin = Cursor::new(Vec::new());
        let mut stdout = Vec::new();
        let mut stderr = Vec::new();
        let mut io = CliIo::new(&mut stdin, &mut stdout, &mut stderr);
        let cmd = RestrictionsLeafCommand {
            command: RestrictionsSubcommand::Approval(ApprovalModeCommand {
                mode: mode.to_string(),
            }),
        };
        let code = execute_with_io(cmd, &mut io);
        (
            code,
            String::from_utf8_lossy(&stdout).to_string(),
            String::from_utf8_lossy(&stderr).to_string(),
        )
    }

    fn run_tool(tool: &str, action: &str) -> (i32, String, String) {
        let mut stdin = Cursor::new(Vec::new());
        let mut stdout = Vec::new();
        let mut stderr = Vec::new();
        let mut io = CliIo::new(&mut stdin, &mut stdout, &mut stderr);
        let cmd = RestrictionsLeafCommand {
            command: RestrictionsSubcommand::Tool(ToolRestrictionsCommand {
                tool: tool.to_string(),
                action: action.to_string(),
            }),
        };
        let code = execute_with_io(cmd, &mut io);
        (
            code,
            String::from_utf8_lossy(&stdout).to_string(),
            String::from_utf8_lossy(&stderr).to_string(),
        )
    }

    fn run_web(policy: &str, domain: Option<&str>) -> (i32, String, String) {
        let mut stdin = Cursor::new(Vec::new());
        let mut stdout = Vec::new();
        let mut stderr = Vec::new();
        let mut io = CliIo::new(&mut stdin, &mut stdout, &mut stderr);
        let cmd = RestrictionsLeafCommand {
            command: RestrictionsSubcommand::Web(WebRestrictionsCommand {
                policy: policy.to_string(),
                domain: domain.map(String::from),
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
    fn permission_returns_meaningful_failure_directing_to_config() {
        let (code, stdout, stderr) = run_permission("bash", "deny");
        assert_eq!(code, 2);
        assert!(stdout.is_empty());
        assert!(stderr.contains("not available"));
        assert!(stderr.contains("harness config"));
    }

    #[test]
    fn approval_returns_meaningful_failure_directing_to_config() {
        let (code, stdout, stderr) = run_approval("always");
        assert_eq!(code, 2);
        assert!(stdout.is_empty());
        assert!(stderr.contains("not available"));
        assert!(stderr.contains("harness config"));
    }

    #[test]
    fn tool_returns_meaningful_failure_directing_to_config() {
        let (code, stdout, stderr) = run_tool("edit", "deny");
        assert_eq!(code, 2);
        assert!(stdout.is_empty());
        assert!(stderr.contains("not available"));
        assert!(stderr.contains("harness config"));
    }

    #[test]
    fn web_returns_meaningful_failure_directing_to_config() {
        let (code, stdout, stderr) = run_web("allow", Some("*.example.com"));
        assert_eq!(code, 2);
        assert!(stdout.is_empty());
        assert!(stderr.contains("not available"));
        assert!(stderr.contains("harness config"));
    }
}
