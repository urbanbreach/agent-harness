use crate::{CliDeps, CliIo};
use serde_json::json;

#[derive(clap::Args)]
pub(crate) struct AgentCommand {
    #[command(subcommand)]
    action: AgentAction,
}
#[derive(clap::Subcommand)]
enum AgentAction {
    Stdio {
        #[arg(long)]
        command: String,
        #[arg(long)]
        json: bool,
    },
}
pub(crate) fn agent(
    command: AgentCommand,
    config: Option<&std::path::Path>,
    io: &mut CliIo<'_>,
    deps: &CliDeps,
) -> Result<(), String> {
    let AgentAction::Stdio { command, json } = command.action;
    if command.trim().is_empty() {
        return Err("--command must not be empty".into());
    }
    let root = deps.current_dir().map_err(|e| e.to_string())?;
    let configured = crate::inspect::configured(config, deps)?;
    let redactor = crate::inspect::redactor(&configured.config, deps)?;
    let product = harness_core::integrations::acp_stdio::run_stdio_acp_agent_mode_product_in(
        &command,
        Some(&root),
    );
    let mut report = json!({"command":product.command,"connected":product.last_connect.is_connected(),
        "bound":product.last_bind.is_bound(),"operate_ok":product.operate_ok,
        "session_id":product.session.as_ref().map(|s| &s.session_id),
        "agent_name":product.session.as_ref().map(|s| &s.agent_name),
        "meets_agent_mode_contract":product.meets_agent_mode_contract()});
    harness_core::redact::redact_in_place(&redactor, &mut report);
    if json {
        crate::inspect::print_json(io, &report)?;
    } else {
        writeln!(
            io.stdout,
            "stdio peer: connected={} bound={} exchange={}",
            report["connected"], report["bound"], report["operate_ok"]
        )
        .map_err(|e| e.to_string())?;
    }
    if product.meets_agent_mode_contract() {
        Ok(())
    } else {
        Err("stdio peer exchange failed".into())
    }
}
