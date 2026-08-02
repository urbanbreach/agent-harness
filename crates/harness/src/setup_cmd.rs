use std::io::Write;
use std::path::PathBuf;

use clap::{Args, Subcommand};
use serde::Serialize;

use crate::{CliDeps, CliIo};

#[derive(Debug, Args, Clone)]
pub(crate) struct WrapCommand {
    #[arg(short, long)]
    output: Option<PathBuf>,
    #[arg(long)]
    with_sessions: bool,
}

#[derive(Debug, Subcommand, Clone)]
pub(crate) enum McpSubcommand {
    List,
    Stdio {
        command: String,
        #[arg(trailing_var_arg = true)]
        args: Vec<String>,
    },
    Health {
        server_id: String,
    },
}

#[derive(Debug, Args, Clone)]
pub(crate) struct AcpServerCommand {
    #[arg(long)]
    command: String,
    #[arg(long, default_value_t = true)]
    stdio: bool,
}

#[derive(Debug, Subcommand, Clone)]
pub(crate) enum SetupSubcommand {
    Wrap(WrapCommand),
    Mcp {
        #[command(subcommand)]
        command: McpSubcommand,
    },
    AcpServer(AcpServerCommand),
}

#[derive(Debug, Args, Clone)]
pub(crate) struct SetupLeafCommand {
    #[command(subcommand)]
    pub(crate) command: SetupSubcommand,
}

#[derive(Debug, Serialize)]
struct WrapResult {
    status: String,
    output: String,
    with_sessions: bool,
    file_count: usize,
}

#[derive(Debug, Serialize)]
struct McpServerEntry {
    id: String,
    enabled: bool,
    transport: String,
}

#[derive(Debug, Serialize)]
struct McpListResult {
    servers: Vec<McpServerEntry>,
}

#[derive(Debug, Serialize)]
struct McpHealthResult {
    server_id: String,
    configured: bool,
    enabled: bool,
}

pub(crate) fn execute_with_io(command: SetupLeafCommand, io: &mut CliIo<'_>) -> i32 {
    match command.command {
        SetupSubcommand::Wrap(cmd) => run_wrap(cmd, io),
        SetupSubcommand::Mcp { command } => run_mcp(command, io),
        SetupSubcommand::AcpServer(cmd) => run_acp_server(cmd, io),
    }
}

fn run_wrap(command: WrapCommand, io: &mut CliIo<'_>) -> i32 {
    let output = command
        .output
        .clone()
        .unwrap_or_else(|| PathBuf::from("workspace.wrap.tar.gz"));

    if let Some(parent) = output.parent() {
        if !parent.as_os_str().is_empty() {
            if let Err(err) = std::fs::create_dir_all(parent) {
                let _ = writeln!(
                    io.stderr,
                    "wrap: failed to create {}: {err}",
                    parent.display()
                );
                return 1;
            }
        }
    }

    let file_count = match build_workspace_archive(&output, command.with_sessions) {
        Ok(count) => count,
        Err(err) => {
            let _ = writeln!(io.stderr, "wrap: failed to build archive: {err}");
            return 1;
        }
    };

    let result = WrapResult {
        status: "wrapped".to_string(),
        output: output.display().to_string(),
        with_sessions: command.with_sessions,
        file_count,
    };
    match serde_json::to_string(&result) {
        Ok(json) => {
            let _ = writeln!(io.stdout, "{json}");
            0
        }
        Err(err) => {
            let _ = writeln!(io.stderr, "wrap: failed to serialize JSON: {err}");
            1
        }
    }
}

fn run_mcp(command: McpSubcommand, io: &mut CliIo<'_>) -> i32 {
    match command {
        McpSubcommand::List => run_mcp_list(io),
        McpSubcommand::Stdio { command, .. } => run_mcp_stdio(&command, io),
        McpSubcommand::Health { server_id } => run_mcp_health(&server_id, io),
    }
}

fn run_mcp_list(io: &mut CliIo<'_>) -> i32 {
    let context = harness_core::config::ConfigLoadContext::from_env();
    match harness_core::config::load_resolved_config_with_context(None, &context) {
        Ok(Some(loaded)) => {
            let servers: Vec<McpServerEntry> = loaded
                .config
                .integrations
                .mcp
                .servers
                .iter()
                .map(|(id, config)| McpServerEntry {
                    id: id.clone(),
                    enabled: config.enabled(),
                    transport: match config {
                        harness_core::config::McpServerConfig::Stdio { .. } => "stdio".to_string(),
                        harness_core::config::McpServerConfig::Http { .. } => "http".to_string(),
                    },
                })
                .collect();
            let result = McpListResult { servers };
            match serde_json::to_string(&result) {
                Ok(json) => {
                    let _ = writeln!(io.stdout, "{json}");
                    0
                }
                Err(err) => {
                    let _ = writeln!(io.stderr, "mcp list: failed to serialize JSON: {err}");
                    1
                }
            }
        }
        Ok(None) => {
            let _ = writeln!(
                io.stderr,
                "mcp list: no config file found; run 'harness config validate' to verify configuration"
            );
            2
        }
        Err(err) => {
            let _ = writeln!(io.stderr, "mcp list: config load failed: {err}");
            1
        }
    }
}

fn run_mcp_stdio(command: &str, io: &mut CliIo<'_>) -> i32 {
    if command.trim().is_empty() {
        let _ = writeln!(io.stderr, "mcp stdio: command must not be empty");
        return 1;
    }
    let _ = writeln!(
        io.stderr,
        "mcp stdio: starting a live MCP stdio proxy requires a running server process; configure MCP servers in harness.jsonc and use the TUI or 'harness run' to interact with them"
    );
    2
}

fn run_mcp_health(server_id: &str, io: &mut CliIo<'_>) -> i32 {
    if server_id.trim().is_empty() {
        let _ = writeln!(io.stderr, "mcp health: server_id must not be empty");
        return 1;
    }
    let context = harness_core::config::ConfigLoadContext::from_env();
    match harness_core::config::load_resolved_config_with_context(None, &context) {
        Ok(Some(loaded)) => {
            let configured = loaded
                .config
                .integrations
                .mcp
                .servers
                .contains_key(server_id);
            let enabled = loaded
                .config
                .integrations
                .mcp
                .servers
                .get(server_id)
                .map(|c| c.enabled())
                .unwrap_or(false);
            let result = McpHealthResult {
                server_id: server_id.to_string(),
                configured,
                enabled,
            };
            match serde_json::to_string(&result) {
                Ok(json) => {
                    let _ = writeln!(io.stdout, "{json}");
                    0
                }
                Err(err) => {
                    let _ = writeln!(io.stderr, "mcp health: failed to serialize JSON: {err}");
                    1
                }
            }
        }
        Ok(None) => {
            let _ = writeln!(
                io.stderr,
                "mcp health: no config file found; run 'harness config validate' to verify configuration"
            );
            2
        }
        Err(err) => {
            let _ = writeln!(io.stderr, "mcp health: config load failed: {err}");
            1
        }
    }
}

fn run_acp_server(_command: AcpServerCommand, io: &mut CliIo<'_>) -> i32 {
    let _ = writeln!(
        io.stderr,
        "acp-server: the ACP server proxy is not yet implemented; use 'harness run' or 'harness prompt' for headless agent execution"
    );
    2
}

fn build_workspace_archive(output: &std::path::Path, with_sessions: bool) -> Result<usize, String> {
    use flate2::write::GzEncoder;
    use flate2::Compression;
    use std::io::Write as _;

    let mut archive_data = Vec::new();
    let encoder = GzEncoder::new(&mut archive_data, Compression::default());
    let mut archive = tar::Builder::new(encoder);

    let mut file_count = 0usize;

    let manifest = std::path::Path::new("Cargo.toml");
    if manifest.exists() {
        let bytes =
            std::fs::read(manifest).map_err(|e| format!("failed to read Cargo.toml: {e}"))?;
        let mut header = tar::Header::new_gnu();
        header.set_size(bytes.len() as u64);
        header.set_mode(0o644);
        header.set_cksum();
        archive
            .append_data(&mut header, "Cargo.toml", std::io::Cursor::new(bytes))
            .map_err(|e| format!("failed to append Cargo.toml: {e}"))?;
        file_count += 1;
    }

    let config_path = std::path::Path::new("harness.jsonc");
    if config_path.exists() {
        let bytes =
            std::fs::read(config_path).map_err(|e| format!("failed to read harness.jsonc: {e}"))?;
        let mut header = tar::Header::new_gnu();
        header.set_size(bytes.len() as u64);
        header.set_mode(0o644);
        header.set_cksum();
        archive
            .append_data(&mut header, "harness.jsonc", std::io::Cursor::new(bytes))
            .map_err(|e| format!("failed to append harness.jsonc: {e}"))?;
        file_count += 1;
    }

    if with_sessions {
        let sessions_dir = std::path::Path::new("sessions");
        if sessions_dir.exists() {
            archive
                .append_dir_all("sessions", sessions_dir)
                .map_err(|e| format!("failed to append sessions dir: {e}"))?;
            file_count += 1;
        }
    }

    archive
        .into_inner()
        .map_err(|e| format!("failed to finalize tar.gz: {e}"))?
        .finish()
        .map_err(|e| format!("failed to compress archive: {e}"))?;

    std::fs::write(output, &archive_data)
        .map_err(|e| format!("failed to write {}: {e}", output.display()))?;

    Ok(file_count)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    fn run_wrap(output: Option<&str>, with_sessions: bool) -> (i32, String, String) {
        let mut stdin = Cursor::new(Vec::new());
        let mut stdout = Vec::new();
        let mut stderr = Vec::new();
        let mut io = CliIo::new(&mut stdin, &mut stdout, &mut stderr);
        let cmd = SetupLeafCommand {
            command: SetupSubcommand::Wrap(WrapCommand {
                output: output.map(PathBuf::from),
                with_sessions,
            }),
        };
        let code = execute_with_io(cmd, &mut io);
        (
            code,
            String::from_utf8_lossy(&stdout).to_string(),
            String::from_utf8_lossy(&stderr).to_string(),
        )
    }

    fn run_mcp_stdio(command: &str) -> (i32, String, String) {
        let mut stdin = Cursor::new(Vec::new());
        let mut stdout = Vec::new();
        let mut stderr = Vec::new();
        let mut io = CliIo::new(&mut stdin, &mut stdout, &mut stderr);
        let cmd = SetupLeafCommand {
            command: SetupSubcommand::Mcp {
                command: McpSubcommand::Stdio {
                    command: command.to_string(),
                    args: vec![],
                },
            },
        };
        let code = execute_with_io(cmd, &mut io);
        (
            code,
            String::from_utf8_lossy(&stdout).to_string(),
            String::from_utf8_lossy(&stderr).to_string(),
        )
    }

    fn run_mcp_health(server_id: &str) -> (i32, String, String) {
        let mut stdin = Cursor::new(Vec::new());
        let mut stdout = Vec::new();
        let mut stderr = Vec::new();
        let mut io = CliIo::new(&mut stdin, &mut stdout, &mut stderr);
        let cmd = SetupLeafCommand {
            command: SetupSubcommand::Mcp {
                command: McpSubcommand::Health {
                    server_id: server_id.to_string(),
                },
            },
        };
        let code = execute_with_io(cmd, &mut io);
        (
            code,
            String::from_utf8_lossy(&stdout).to_string(),
            String::from_utf8_lossy(&stderr).to_string(),
        )
    }

    fn run_acp_server(command: &str) -> (i32, String, String) {
        let mut stdin = Cursor::new(Vec::new());
        let mut stdout = Vec::new();
        let mut stderr = Vec::new();
        let mut io = CliIo::new(&mut stdin, &mut stdout, &mut stderr);
        let cmd = SetupLeafCommand {
            command: SetupSubcommand::AcpServer(AcpServerCommand {
                command: command.to_string(),
                stdio: true,
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
    fn wrap_happy_creates_real_tar_gz_archive() {
        let dir = tempfile::tempdir().unwrap();
        let output = dir.path().join("workspace.tar.gz");
        let (code, stdout, stderr) = run_wrap(Some(output.to_str().unwrap()), false);
        assert_eq!(code, 0, "{stderr}");
        assert!(output.exists(), "archive file must exist on disk");
        let json: serde_json::Value = serde_json::from_str(stdout.trim()).unwrap();
        assert_eq!(json["status"].as_str().unwrap(), "wrapped");
        assert!(json["file_count"].as_u64().unwrap() >= 1);
    }

    #[test]
    fn wrap_failure_invalid_output_path_returns_error() {
        let (code, _stdout, stderr) = run_wrap(Some("/nonexistent/dir/pkg.tar.gz"), false);
        assert_ne!(code, 0);
        assert!(stderr.contains("failed to create") || stderr.contains("failed to write"));
    }

    #[test]
    fn mcp_stdio_returns_meaningful_failure_for_empty_command() {
        let (code, stdout, stderr) = run_mcp_stdio("");
        assert_ne!(code, 0);
        assert!(stdout.is_empty());
        assert!(stderr.contains("command must not be empty"));
    }

    #[test]
    fn mcp_stdio_returns_meaningful_failure_directing_to_config() {
        let (code, stdout, stderr) = run_mcp_stdio("npx");
        assert_eq!(code, 2);
        assert!(stdout.is_empty());
        assert!(stderr.contains("requires a running server process"));
    }

    #[test]
    fn mcp_health_invalid_empty_server_id_returns_error() {
        let (code, stdout, stderr) = run_mcp_health("");
        assert_ne!(code, 0);
        assert!(stdout.is_empty());
        assert!(stderr.contains("server_id must not be empty"));
    }

    #[test]
    fn acp_server_returns_meaningful_failure_directing_to_run() {
        let (code, stdout, stderr) = run_acp_server("acp-server");
        assert_eq!(code, 2);
        assert!(stdout.is_empty());
        assert!(stderr.contains("not yet implemented"));
        assert!(stderr.contains("harness run"));
    }
}
