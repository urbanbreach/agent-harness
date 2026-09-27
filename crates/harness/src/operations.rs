use crate::{CliDeps, CliIo};
use harness_core::{
    cron_execute::{CronCivilTime, CronExecutor},
    cron_schedule::{CronSchedule, CronScheduleRegistry, ScheduleId},
    team_mailbox_journal::DurableTeamRegistry,
};
use serde_json::json;
use std::path::PathBuf;

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

#[derive(clap::Args)]
pub(crate) struct CronCommand {
    #[command(subcommand)]
    action: CronAction,
}
#[derive(clap::Subcommand)]
enum CronAction {
    /// Evaluate supplied schedules at an explicit civil time and record due entries.
    FireDue {
        #[arg(long, default_value_t = 0)]
        minute: u8,
        #[arg(long, default_value_t = 0)]
        hour: u8,
        #[arg(long, default_value_t = 1)]
        day_month: u8,
        #[arg(long, default_value_t = 1)]
        month: u8,
        #[arg(long, default_value_t = 0)]
        weekday: u8,
        #[arg(long)]
        journal_dir: Option<PathBuf>,
        /// Entries in the form id:minute hour day-of-month month day-of-week.
        specs: Vec<String>,
    },
}
pub(crate) fn cron(command: CronCommand, io: &mut CliIo<'_>, deps: &CliDeps) -> Result<(), String> {
    let CronAction::FireDue {
        minute,
        hour,
        day_month,
        month,
        weekday,
        journal_dir,
        specs,
    } = command.action;
    let now =
        CronCivilTime::new(minute, hour, day_month, month, weekday).map_err(|e| e.to_string())?;
    let root = crate::workspace::workspace_root(None, deps)?;
    let journal_dir =
        root.join(journal_dir.unwrap_or_else(|| ".agent-harness/cron-journal".into()));
    let mut schedules = CronScheduleRegistry::new();
    for spec in specs {
        let (id, expression) = spec
            .split_once(':')
            .ok_or("schedule must be id:<five-field expression>")?;
        schedules
            .register(CronSchedule {
                id: ScheduleId::parse(id).map_err(|e| e.to_string())?,
                expression: expression.trim().into(),
                label: None,
                payload_hint: id.trim().into(),
            })
            .map_err(|e| e.to_string())?;
    }
    let batch = CronExecutor::with_journal_dir(&journal_dir)
        .fire_due(&schedules, now)
        .map_err(|e| e.to_string())?;
    crate::inspect::print_json(
        io,
        &json!({"journal_dir":journal_dir,"journal_path":batch.journal_path,
        "fired":batch.fired,"skipped":batch.skipped}),
    )
}

#[derive(clap::Args)]
pub(crate) struct TeamCommand {
    #[arg(long, global = true)]
    workspace: Option<PathBuf>,
    #[command(subcommand)]
    action: TeamAction,
}
#[derive(clap::Subcommand)]
enum TeamAction {
    Create {
        name: String,
    },
    AddMember {
        team: String,
        agent: String,
        role: String,
    },
    Send {
        team: String,
        from: String,
        body: String,
        #[arg(long)]
        to: Option<String>,
    },
    Deliver {
        team: String,
        agent: String,
    },
    List,
    Cancel {
        team: String,
    },
}
pub(crate) fn team(command: TeamCommand, io: &mut CliIo<'_>, deps: &CliDeps) -> Result<(), String> {
    let root = crate::workspace::workspace_root(command.workspace, deps)?;
    let mut registry = DurableTeamRegistry::open(&root).map_err(|e| e.to_string())?;
    let report = match command.action {
        TeamAction::Create { name } => registry.create_team(name).map(|record| json!(record)),
        TeamAction::AddMember { team, agent, role } => registry
            .add_member(&team, agent, role)
            .map(|record| json!(record)),
        TeamAction::Cancel { team } => registry.cancel_team(&team).map(|record| json!(record)),
        TeamAction::Send {
            team,
            from,
            to,
            body,
        } => registry
            .send_message(&team, from, to, body)
            .map(|message| json!(message)),
        TeamAction::Deliver { team, agent } => registry
            .deliver_messages(&team, &agent)
            .map(|messages| json!({"count":messages.len(),"messages":messages})),
        TeamAction::List => {
            let teams = registry.registry().list_teams();
            Ok(json!({"journal_path":registry.journal_path(),"count":teams.len(),"teams":teams}))
        }
    };
    let mut report = report.map_err(|e| e.to_string())?;
    report["workspace_root"] = json!(root);
    harness_core::redact::redact_in_place(
        &harness_core::redact::DefaultRedactor::default(),
        &mut report,
    );
    crate::inspect::print_json(io, &report)
}
