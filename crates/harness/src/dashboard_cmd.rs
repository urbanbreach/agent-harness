//! CLI surface for `harness dashboard` overview commands plus the legacy
//! top-level `export` and `trace` stubs that redirect to `sessions export`.

use std::io::Write;
use std::path::PathBuf;

use clap::{Args, Subcommand};
use serde::Serialize;

use crate::replay::inspect_session_catalog;
use crate::CliIo;

#[derive(Debug, Args, Clone)]
pub(crate) struct ExportCommand {
    session_id: String,
    #[arg(long)]
    output: Option<PathBuf>,
}

#[derive(Debug, Args, Clone)]
pub(crate) struct TraceCommand {
    session_id: String,
    #[arg(long, default_value_t = true)]
    local: bool,
    #[arg(short, long)]
    output: Option<PathBuf>,
    #[arg(long)]
    json: bool,
}

#[derive(Debug, Args, Clone, Default)]
pub(crate) struct DashboardListCommand {
    /// Print results as JSON.
    #[arg(long, default_value_t = false)]
    json: bool,
}

#[derive(Debug, Args, Clone, Default)]
pub(crate) struct DashboardStatusCommand {
    /// Print results as JSON.
    #[arg(long, default_value_t = false)]
    json: bool,
}

#[derive(Debug, Args, Clone)]
pub(crate) struct DashboardRecentCommand {
    /// Maximum number of sessions to show.
    #[arg(long, default_value_t = 5)]
    limit: usize,
    /// Print results as JSON.
    #[arg(long, default_value_t = false)]
    json: bool,
}

#[derive(Debug, Subcommand, Clone)]
pub(crate) enum DashboardSubcommand {
    Export(ExportCommand),
    Trace(TraceCommand),
    /// List all stored sessions.
    List(DashboardListCommand),
    /// Show a session directory and configuration summary.
    Status(DashboardStatusCommand),
    /// Show the most recent sessions.
    Recent(DashboardRecentCommand),
}

#[derive(Debug, Args, Clone)]
pub(crate) struct DashboardLeafCommand {
    #[command(subcommand)]
    pub(crate) command: DashboardSubcommand,
}

#[derive(Debug, Serialize)]
struct DashboardSessionRow {
    run_id: String,
    status: String,
    last_updated_at: Option<String>,
    mode_source: String,
    provider_model: Option<String>,
    is_resumable: bool,
}

#[derive(Debug, Serialize)]
struct DashboardStatusResult {
    session_dir: String,
    session_count: usize,
    config_loaded: bool,
}

pub(crate) fn execute_with_io(
    command: DashboardLeafCommand,
    session_dir: Option<PathBuf>,
    io: &mut CliIo<'_>,
) -> i32 {
    match command.command {
        DashboardSubcommand::Export(cmd) => run_export(cmd, io),
        DashboardSubcommand::Trace(cmd) => run_trace(cmd, io),
        DashboardSubcommand::List(cmd) => run_list(cmd, session_dir, io),
        DashboardSubcommand::Status(cmd) => run_status(cmd, session_dir, io),
        DashboardSubcommand::Recent(cmd) => run_recent(cmd, session_dir, io),
    }
}

fn resolve_session_dir(session_dir: Option<PathBuf>) -> PathBuf {
    session_dir.unwrap_or_else(|| PathBuf::from(crate::defaults::DEFAULT_SESSION_DIR))
}

fn session_rows(
    session_dir: Option<PathBuf>,
    io: &mut CliIo<'_>,
    limit: Option<usize>,
) -> Option<(PathBuf, Vec<DashboardSessionRow>)> {
    let sdir = resolve_session_dir(session_dir);
    let entries = match inspect_session_catalog(&sdir) {
        Ok(entries) => entries,
        Err(err) => {
            let _ = writeln!(io.stderr, "dashboard: {err}");
            return None;
        }
    };
    let rows: Vec<DashboardSessionRow> = entries
        .iter()
        .take(limit.unwrap_or(entries.len()))
        .map(|entry| DashboardSessionRow {
            run_id: entry.catalog.run_id.clone(),
            status: entry
                .catalog
                .status
                .as_ref()
                .map(|s| format!("{s:?}"))
                .unwrap_or_else(|| "Unknown".to_string()),
            last_updated_at: entry.catalog.last_updated_at.clone(),
            mode_source: format!("{:?}", entry.catalog.mode_source),
            provider_model: entry.catalog.provider_model.clone(),
            is_resumable: entry.catalog.is_resumable,
        })
        .collect();
    Some((sdir, rows))
}

fn run_export(_command: ExportCommand, io: &mut CliIo<'_>) -> i32 {
    let _ = writeln!(
        io.stderr,
        "export: this subcommand is not available; use 'harness sessions export <session-id> --output <path>' to export a session transcript"
    );
    2
}

fn run_trace(_command: TraceCommand, io: &mut CliIo<'_>) -> i32 {
    let _ = writeln!(
        io.stderr,
        "trace: this subcommand is not available; use 'harness sessions export <session-id> --output <path>' to export session trace data"
    );
    2
}

fn run_list(cmd: DashboardListCommand, session_dir: Option<PathBuf>, io: &mut CliIo<'_>) -> i32 {
    let Some((sdir, rows)) = session_rows(session_dir, io, None) else {
        return 1;
    };
    if cmd.json {
        let envelope = serde_json::json!({
            "schema_version": "harness-dashboard-list-v1",
            "session_dir": sdir.display().to_string(),
            "session_count": rows.len(),
            "sessions": rows,
        });
        let _ = writeln!(
            io.stdout,
            "{}",
            serde_json::to_string_pretty(&envelope).unwrap_or_else(|_| "{}".to_string())
        );
    } else {
        let _ = writeln!(io.stdout, "sessions in {}", sdir.display());
        let _ = writeln!(io.stdout, "  count: {}", rows.len());
        for row in &rows {
            let model = row.provider_model.as_deref().unwrap_or("-");
            let updated = row.last_updated_at.as_deref().unwrap_or("-");
            let _ = writeln!(
                io.stdout,
                "  {} [{}] model={} updated={}",
                row.run_id, row.status, model, updated,
            );
        }
    }
    0
}

fn run_status(
    cmd: DashboardStatusCommand,
    session_dir: Option<PathBuf>,
    io: &mut CliIo<'_>,
) -> i32 {
    let sdir = resolve_session_dir(session_dir);
    let session_count = match inspect_session_catalog(&sdir) {
        Ok(entries) => entries.len(),
        Err(_) => 0,
    };

    let context = harness_core::config::ConfigLoadContext::from_env();
    let config_loaded = matches!(
        harness_core::config::load_resolved_config_with_context(None, &context),
        Ok(Some(_))
    );

    let result = DashboardStatusResult {
        session_dir: sdir.display().to_string(),
        session_count,
        config_loaded,
    };

    if cmd.json {
        match serde_json::to_string_pretty(&result) {
            Ok(json) => {
                let _ = writeln!(io.stdout, "{json}");
            }
            Err(err) => {
                let _ = writeln!(io.stderr, "dashboard status: serialize failed: {err}");
                return 1;
            }
        }
    } else {
        let _ = writeln!(io.stdout, "dashboard status");
        let _ = writeln!(io.stdout, "  session_dir:   {}", result.session_dir);
        let _ = writeln!(io.stdout, "  session_count: {}", result.session_count);
        let _ = writeln!(io.stdout, "  config_loaded: {}", result.config_loaded);
    }
    0
}

fn run_recent(
    cmd: DashboardRecentCommand,
    session_dir: Option<PathBuf>,
    io: &mut CliIo<'_>,
) -> i32 {
    let limit = cmd.limit.max(1);
    let Some((sdir, rows)) = session_rows(session_dir, io, Some(limit)) else {
        return 1;
    };
    if cmd.json {
        let envelope = serde_json::json!({
            "schema_version": "harness-dashboard-recent-v1",
            "session_dir": sdir.display().to_string(),
            "limit": limit,
            "session_count": rows.len(),
            "sessions": rows,
        });
        let _ = writeln!(
            io.stdout,
            "{}",
            serde_json::to_string_pretty(&envelope).unwrap_or_else(|_| "{}".to_string())
        );
    } else {
        let _ = writeln!(
            io.stdout,
            "recent sessions in {} (limit {limit})",
            sdir.display()
        );
        for row in &rows {
            let model = row.provider_model.as_deref().unwrap_or("-");
            let updated = row.last_updated_at.as_deref().unwrap_or("-");
            let _ = writeln!(
                io.stdout,
                "  {} [{}] model={} updated={}",
                row.run_id, row.status, model, updated,
            );
        }
    }
    0
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    fn run_export(session_id: &str) -> (i32, String, String) {
        let mut stdin = Cursor::new(Vec::new());
        let mut stdout = Vec::new();
        let mut stderr = Vec::new();
        let mut io = CliIo::new(&mut stdin, &mut stdout, &mut stderr);
        let cmd = DashboardLeafCommand {
            command: DashboardSubcommand::Export(ExportCommand {
                session_id: session_id.to_string(),
                output: None,
            }),
        };
        let code = execute_with_io(cmd, None, &mut io);
        (
            code,
            String::from_utf8_lossy(&stdout).to_string(),
            String::from_utf8_lossy(&stderr).to_string(),
        )
    }

    fn run_trace(session_id: &str) -> (i32, String, String) {
        let mut stdin = Cursor::new(Vec::new());
        let mut stdout = Vec::new();
        let mut stderr = Vec::new();
        let mut io = CliIo::new(&mut stdin, &mut stdout, &mut stderr);
        let cmd = DashboardLeafCommand {
            command: DashboardSubcommand::Trace(TraceCommand {
                session_id: session_id.to_string(),
                local: true,
                output: None,
                json: true,
            }),
        };
        let code = execute_with_io(cmd, None, &mut io);
        (
            code,
            String::from_utf8_lossy(&stdout).to_string(),
            String::from_utf8_lossy(&stderr).to_string(),
        )
    }

    fn run_list(session_dir: Option<PathBuf>, json: bool) -> (i32, String, String) {
        let mut stdin = Cursor::new(Vec::new());
        let mut stdout = Vec::new();
        let mut stderr = Vec::new();
        let mut io = CliIo::new(&mut stdin, &mut stdout, &mut stderr);
        let cmd = DashboardLeafCommand {
            command: DashboardSubcommand::List(DashboardListCommand { json }),
        };
        let code = execute_with_io(cmd, session_dir, &mut io);
        (
            code,
            String::from_utf8_lossy(&stdout).to_string(),
            String::from_utf8_lossy(&stderr).to_string(),
        )
    }

    fn run_status(session_dir: Option<PathBuf>, json: bool) -> (i32, String, String) {
        let mut stdin = Cursor::new(Vec::new());
        let mut stdout = Vec::new();
        let mut stderr = Vec::new();
        let mut io = CliIo::new(&mut stdin, &mut stdout, &mut stderr);
        let cmd = DashboardLeafCommand {
            command: DashboardSubcommand::Status(DashboardStatusCommand { json }),
        };
        let code = execute_with_io(cmd, session_dir, &mut io);
        (
            code,
            String::from_utf8_lossy(&stdout).to_string(),
            String::from_utf8_lossy(&stderr).to_string(),
        )
    }

    #[test]
    fn export_returns_meaningful_failure_directing_to_sessions_export() {
        // arrange
        // act
        let (code, stdout, stderr) = run_export("sess-export");
        // assert
        assert_eq!(code, 2);
        assert!(stdout.is_empty());
        assert!(stderr.contains("not available"));
        assert!(stderr.contains("harness sessions export"));
    }

    #[test]
    fn trace_returns_meaningful_failure_directing_to_sessions_export() {
        // arrange
        // act
        let (code, stdout, stderr) = run_trace("sess-trace");
        // assert
        assert_eq!(code, 2);
        assert!(stdout.is_empty());
        assert!(stderr.contains("not available"));
        assert!(stderr.contains("harness sessions export"));
    }

    #[test]
    fn list_with_empty_session_dir_returns_zero_count() {
        // arrange
        let dir = tempfile::tempdir().unwrap();
        // act
        let (code, stdout, stderr) = run_list(Some(dir.path().to_path_buf()), true);
        // assert
        assert_eq!(code, 0, "stderr: {stderr}");
        let json: serde_json::Value = serde_json::from_str(&stdout).unwrap();
        assert_eq!(json["session_count"], 0);
        assert_eq!(json["schema_version"], "harness-dashboard-list-v1");
    }

    #[test]
    fn status_reports_session_count_and_config_state() {
        // arrange
        let dir = tempfile::tempdir().unwrap();
        // act
        let (code, stdout, stderr) = run_status(Some(dir.path().to_path_buf()), true);
        // assert
        assert_eq!(code, 0, "stderr: {stderr}");
        let json: serde_json::Value = serde_json::from_str(&stdout).unwrap();
        assert_eq!(json["session_count"], 0);
        assert!(json["session_dir"].is_string());
        assert!(json["config_loaded"].is_boolean());
    }
}
