use super::{options::Options, Format, PromptCommand};
use crate::{CliDeps, CliIo};
use std::{
    io::Read,
    path::{Path, PathBuf},
};
mod scenario;

#[derive(clap::Args)]
pub(crate) struct RunCommand {
    #[arg(long, value_enum, conflicts_with_all = ["message", "stdin", "prompt_file", "session", "continue_session", "fork_session", "session_id"])]
    scenario: Option<crate::scenarios::ScenarioName>,
    #[arg(long, requires = "scenario")]
    deterministic: bool,
    #[command(flatten)]
    options: Options,
    #[arg(long)]
    pub(crate) profile: Option<String>,
    #[arg(long)]
    pub(crate) mock: bool,
    #[arg(long)]
    stdin: bool,
    #[arg(long)]
    prompt_file: Option<PathBuf>,
    #[arg(long = "file", short = 'f')]
    files: Vec<PathBuf>,
    #[arg(long = "continue", short = 'c', conflicts_with = "session")]
    continue_session: bool,
    #[arg(long, short = 's')]
    session: Option<String>,
    #[arg(long)]
    fork_session: bool,
    #[arg(long)]
    session_id: Option<String>,
    #[arg(long)]
    out: Option<PathBuf>,
    #[arg(long)]
    print_run_dir: bool,
    #[arg(long, alias = "output-format", value_enum, default_value = "default")]
    format: Format,
    #[arg(num_args = 0..)]
    message: Vec<String>,
}
pub(crate) fn execute(
    command: RunCommand,
    config: Option<&Path>,
    session_dir: Option<PathBuf>,
    io: &mut CliIo<'_>,
    deps: &CliDeps,
) -> Result<(), String> {
    if let Some(scenario) = command.scenario {
        return scenario::execute(command, scenario, config, session_dir, io, deps);
    }
    let root = deps.current_dir().map_err(|e| e.to_string())?;
    let text = input(&command, &root, io)?;
    let resume = if command.continue_session {
        Some(latest_session(
            config,
            session_dir.as_deref(),
            deps,
            command.mock,
        )?)
    } else {
        command.session
    };
    if command.fork_session && resume.is_none() {
        return Err("--fork-session requires --session or --continue".into());
    }
    super::execute(
        PromptCommand {
            profile: command.profile,
            options: command.options,
            text: Some(text),
            stdin: false,
            prompt: Vec::new(),
            mock: command.mock,
            print_run_dir: command.print_run_dir,
            prompt_file: None,
            resume,
            fork_session: command.fork_session,
            session_id: command.session_id,
            out: command.out,
            format: command.format,
        },
        config,
        session_dir,
        io,
        deps,
    )
}
fn input(command: &RunCommand, root: &Path, io: &mut CliIo<'_>) -> Result<String, String> {
    let mut parts = vec![command.message.join(" ")];
    if command.stdin || !io.stdin_is_terminal {
        let mut text = String::new();
        io.stdin
            .take(1024 * 1024 + 1)
            .read_to_string(&mut text)
            .map_err(|e| e.to_string())?;
        parts.push(text);
    }
    if let Some(path) = &command.prompt_file {
        let path = root.join(path);
        if !path.metadata().map_err(|e| e.to_string())?.is_file() {
            return Err("prompt file must be a regular file".into());
        }
        let mut text = String::new();
        std::fs::File::open(path)
            .map_err(|e| e.to_string())?
            .take(1024 * 1024 + 1)
            .read_to_string(&mut text)
            .map_err(|e| e.to_string())?;
        parts.push(text);
    }
    if parts.iter().map(String::len).sum::<usize>() > 1024 * 1024 {
        return Err("prompt exceeds 1 MiB".into());
    }
    if !command.options.verbatim {
        for part in &mut parts {
            part.truncate(part.trim_end_matches(['\r', '\n']).len());
        }
    }
    parts.retain(|text| !text.is_empty());
    if parts.is_empty() {
        return Err("pass a message, pipe stdin or use --prompt-file".into());
    }
    for file in &command.files {
        if !root.join(file).is_file() {
            return Err(format!(
                "--file must name a regular file: {}",
                file.display()
            ));
        }
        parts.push(format!(
            "@{}",
            file.to_str().ok_or("--file path must be UTF-8")?
        ));
    }
    let text = parts.join("\n");
    if text.len() > 1024 * 1024 {
        return Err("prompt exceeds 1 MiB".into());
    }
    Ok(text)
}
fn latest_session(
    config: Option<&Path>,
    sessions: Option<&Path>,
    deps: &CliDeps,
    mock: bool,
) -> Result<String, String> {
    let root = deps.current_dir().map_err(|e| e.to_string())?;
    let directory = match sessions {
        Some(path) => root.join(path),
        None => {
            let config = config.map(|path| root.join(path));
            let loaded = if mock && config.is_none() {
                None
            } else {
                harness_core::config::load_resolved_config_with_lookup(
                    config.as_deref(),
                    &deps.config_load_context(),
                    &|name| deps.env_var_value(name),
                )
                .map_err(|e| e.to_string())?
            };
            root.join(loaded.map_or_else(
                || PathBuf::from(crate::defaults::DEFAULT_SESSION_DIR),
                |loaded| loaded.config.runtime.session_dir,
            ))
        }
    };
    crate::replay::inspect_session_catalog(&directory)?
        .into_iter()
        .find(|entry| entry.is_visible_in_operator_history() && entry.catalog.is_resumable)
        .map(|entry| entry.run_dir.to_string_lossy().into_owned())
        .ok_or_else(|| "no resumable sessions found for --continue".into())
}
