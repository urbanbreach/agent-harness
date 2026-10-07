use crate::{CliDeps, CliIo};
use clap::Args;
use harness_core::{
    config::load_resolved_config_with_lookup,
    coord::{spawn_coordinator, CoordinatorConfig, CoordinatorHandle},
    event::{ActorKind, EventActor, EventV1, RuntimeEvent, TaskTerminalScope},
    perm::PermissionDecision,
    redact::DefaultRedactor,
    store::{EventStore, EventStoreError, RuntimeEventStream},
};
use std::{
    io::Read,
    path::{Path, PathBuf},
    sync::Arc,
    time::Duration,
};
use tokio_stream::StreamExt;
mod options;
mod output;
mod run;
mod session;
use output::{Format, Output};
pub(crate) use run::{execute as execute_run, RunCommand};

#[derive(Args)]
#[group(skip)]
#[command(group(clap::ArgGroup::new("input").required(true).args(["text", "stdin", "prompt", "prompt_file"])))]
pub(crate) struct PromptCommand {
    #[arg(long)]
    pub(crate) profile: Option<String>,
    #[command(flatten)]
    pub(crate) options: options::Options,
    #[arg(long)]
    text: Option<String>,
    #[arg(long)]
    stdin: bool,
    #[arg(num_args = 1..)]
    prompt: Vec<String>,
    #[arg(long)]
    pub(crate) mock: bool,
    #[arg(long)]
    print_run_dir: bool,
    #[arg(long)]
    prompt_file: Option<PathBuf>,
    #[arg(long)]
    resume: Option<String>,
    #[arg(long, requires = "resume")]
    fork_session: bool,
    #[arg(long)]
    session_id: Option<String>,
    #[arg(long)]
    out: Option<PathBuf>,
    #[arg(long, alias = "output-format", value_enum, default_value = "default")]
    format: Format,
}

pub(crate) fn execute(
    mut command: PromptCommand,
    config: Option<&Path>,
    session_dir: Option<PathBuf>,
    io: &mut CliIo<'_>,
    deps: &CliDeps,
) -> Result<(), String> {
    let mut text = command
        .text
        .take()
        .unwrap_or_else(|| command.prompt.join(" "));
    if command.stdin {
        io.stdin
            .take(1024 * 1024 + 1)
            .read_to_string(&mut text)
            .map_err(|e| e.to_string())?;
    }
    if let Some(path) = &command.prompt_file {
        let path = deps.current_dir().map_err(|e| e.to_string())?.join(path);
        if !path.metadata().map_err(|e| e.to_string())?.is_file() {
            return Err("prompt file must be a regular file".into());
        }
        std::fs::File::open(path)
            .map_err(|e| e.to_string())?
            .take(1024 * 1024 + 1)
            .read_to_string(&mut text)
            .map_err(|e| e.to_string())?;
    }
    if text.trim().is_empty() || text.len() > 1024 * 1024 {
        return Err("prompt must contain 1 byte to 1 MiB of text".into());
    }
    if command.stdin && !command.options.verbatim {
        text.truncate(text.trim_end_matches(['\r', '\n']).len());
    }
    crate::cli_io::with_output_worker(io, move |io| {
        execute_prompt(command, text, config, session_dir, io, deps)
    })
}

fn execute_prompt(
    command: PromptCommand,
    text: String,
    config: Option<&Path>,
    session_dir: Option<PathBuf>,
    io: &mut CliIo<'_>,
    deps: &CliDeps,
) -> Result<(), String> {
    let root = deps.current_dir().map_err(|e| e.to_string())?;
    let config_path = config.map(|path| root.join(path));
    let mut loaded = if command.mock && config_path.is_none() {
        None
    } else {
        load_resolved_config_with_lookup(
            config_path.as_deref(),
            &deps.config_load_context(),
            &|name| deps.env_var_value(name),
        )
        .map_err(|e| e.to_string())?
        .map(|c| c.config)
    };
    if let Some(id) = &command.session_id {
        harness_core::store::validate_session_id(id).map_err(|e| e.to_string())?;
        if command.resume.is_some() && !command.fork_session {
            return Err("--session-id with --resume requires --fork-session".into());
        }
    }
    let sessions = root.join(session_dir.unwrap_or_else(|| {
        loaded.as_ref().map_or_else(
            || PathBuf::from(crate::defaults::DEFAULT_SESSION_DIR),
            |config| config.runtime.session_dir.clone(),
        )
    }));
    let mut resume = command
        .resume
        .as_deref()
        .map(|id| session::Resume::read(id, &sessions, &root))
        .transpose()?;
    let deps = resume.as_ref().map_or_else(
        || deps.clone(),
        |r| deps.clone().with_current_dir(r.workspace.clone()),
    );
    if resume.is_some() && !(command.mock && config_path.is_none()) {
        loaded = load_resolved_config_with_lookup(
            config_path.as_deref(),
            &deps.config_load_context(),
            &|name| deps.env_var_value(name),
        )
        .map_err(|e| e.to_string())?
        .map(|c| c.config);
    }
    let mut config = if command.mock {
        loaded.unwrap_or_default()
    } else {
        crate::runtime_catalog::resolve_runtime_catalog(
            loaded,
            None,
            None,
            harness_core::auth::CredentialStore::from_lookup(&|name| deps.env_var_value(name))
                .as_ref(),
            &|name| deps.env_var_value(name),
        )?
        .config
    };
    let profile = command
        .profile
        .clone()
        .or_else(|| resume.as_ref().map(|r| r.profile.clone()))
        .unwrap_or_else(|| crate::bootstrap::interactive_profile_name(&config));
    if resume.as_ref().is_some_and(|r| r.profile != profile) {
        return Err("resuming a prompt cannot change its agent profile".into());
    }
    command.options.prepare(&mut config, &profile)?;
    let mut coordinator_config = crate::bootstrap::build(&config, &deps, command.mock, false)?;
    command.options.apply(&mut coordinator_config, &profile)?;
    let target = command
        .options
        .overrides_model()
        .then(|| {
            coordinator_config
                .agent_model_targets
                .get(&profile)
                .cloned()
        })
        .flatten();
    if !coordinator_config.agent_profiles.contains_key(&profile) {
        return Err(format!("unknown profile: {profile}"));
    }
    if command.fork_session
        && let Some(resume) = &mut resume
    {
        resume.fork(command.session_id.as_deref())?;
    }
    coordinator_config.session_dir = resume
        .as_ref()
        .and_then(|r| r.path.parent().map(Path::to_path_buf))
        .unwrap_or(sessions);
    coordinator_config.run_id_override = command.session_id;
    let cancellation = deps.cancellation();
    let handle_signals = cancellation.is_none();
    let cancellation = cancellation.unwrap_or_default();
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|e| e.to_string())?;
    runtime.block_on(async {
        if cancellation.is_cancelled() {
            return Err("prompt interrupted".into());
        }
        let coordinator = spawn_coordinator(
            coordinator_config,
            deps.clock(false),
            Arc::new(DefaultRedactor::default()),
        );
        let redactor = coordinator
            .output_redactor()
            .await
            .map_err(|e| e.to_string())?;
        let run = match &resume {
            Some(resume) => coordinator.resume_run(&resume.id, "Prompt").await,
            None => coordinator.start_run("Prompt", &root).await,
        }
        .map_err(|e| e.to_string())?;
        let mut output = Output::new(io.stdout, command.format, command.options.thinking);
        let result = async {
            if command.print_run_dir {
                writeln!(io.stderr, "{}", run.run_dir.display()).map_err(|e| e.to_string())?;
            }
            let actor = EventActor::new(ActorKind::User, None);
            let agent = match &resume {
                Some(resume) => resume.agent.clone(),
                None => coordinator
                    .spawn_agent_idle(actor.clone(), profile, None)
                    .await
                    .map_err(|e| e.to_string())?,
            };
            let store = coordinator.event_store().await.map_err(|e| e.to_string())?;
            let from_seq = resume.as_ref().map_or(1, |r| r.from_seq);
            let stream = store
                .subscribe_runtime(from_seq)
                .map_err(|e| e.to_string())?;
            let task = match target {
                Some(target) => {
                    coordinator
                        .request_agent_turn_with_model_target(actor, agent, text, target)
                        .await
                }
                None => {
                    coordinator
                        .request_agent_turn_with_model(
                            actor,
                            agent,
                            text,
                            None,
                            command
                                .options
                                .overrides_model()
                                .then(|| command.options.model_settings()),
                        )
                        .await
                }
            }
            .map_err(|e| e.to_string())?;
            tokio::select! {
                biased;
                () = cancellation.cancelled() => Err("prompt interrupted".into()),
                signal = tokio::signal::ctrl_c(), if handle_signals => {
                    signal.map_err(|e| e.to_string())?;
                    Err("prompt interrupted".into())
                },
                result = tokio::time::timeout(
                Duration::from_millis(config.runtime.prompt.wait_timeout_ms),
                wait(&coordinator, store.as_ref(), &task, from_seq, stream, &mut output),
                ) => result.map_err(|_| "prompt timed out".to_owned()).and_then(|result| result),
            }
        }
        .await;
        match &result {
            Ok(()) => coordinator.stop_run().await,
            Err(error) => coordinator.fail_run(error).await,
        }
        .map_err(|e| e.to_string())?;
        output.finish().map_err(|e| e.to_string())?;
        result?;
        if let Some(path) = command.out {
            session::export(&run.events_path, &root.join(path), redactor.as_ref())?;
        }
        Ok(())
    })
}

async fn wait(
    coordinator: &CoordinatorHandle,
    store: &dyn EventStore,
    task: &str,
    from_seq: u64,
    mut stream: RuntimeEventStream,
    output: &mut Output<'_>,
) -> Result<(), String> {
    let mut sequence = from_seq.saturating_sub(1);
    while let Some(event) = stream.next().await {
        let event = match event {
            Err(EventStoreError::SubscriberLagged(_)) => {
                output.lagged();
                stream = store
                    .subscribe_runtime(sequence + 1)
                    .map_err(|e| e.to_string())?;
                continue;
            }
            result => result.map_err(|e| e.to_string())?,
        };
        let event = match event {
            RuntimeEvent::Live(event) => {
                output.live(*event, task).map_err(|e| e.to_string())?;
                continue;
            }
            RuntimeEvent::Durable(event) => *event,
        };
        sequence = event.seq;
        output.event(&event, task).map_err(|e| e.to_string())?;
        match event.payload {
            EventV1::PermissionRequested(permission) => coordinator
                .resolve_permission(
                    permission.permission_id,
                    PermissionDecision::Deny,
                    Some(
                        "approval requires an interactive session or an explicit permission rule"
                            .into(),
                    ),
                )
                .await
                .map_err(|e| e.to_string())?,
            EventV1::TaskCompleted(done)
                if done.task_id.as_str() == task
                    && done
                        .metadata
                        .as_ref()
                        .is_some_and(|m| m.task_scope == Some(TaskTerminalScope::AgentTurn)) =>
            {
                return Ok(())
            }
            EventV1::TaskCancelled(done) if done.task_id.as_str() == task => {
                return Err(done.reason)
            }
            EventV1::RunFailed(failure) => return Err(failure.error),
            _ => {}
        }
    }
    Err("session ended before the prompt completed".into())
}
