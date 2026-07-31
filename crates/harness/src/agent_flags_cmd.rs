//! CLI leaf for agent, model, reasoning, and system-prompt flags (shard F).
//!
//! These subcommands were false-success shards that returned `status:
//! "selected"`/`"applied"` without calling any real agent or config
//! authority. They have been replaced with meaningful failure directing
//! users to the real CLI flags (--agent, --model, --reasoning-effort).

use std::io::Write;

use clap::{Args, Subcommand};

use crate::{CliDeps, CliIo};

#[derive(Debug, Args, Clone)]
pub(crate) struct AgentFlagsCommand {
    #[arg(long)]
    agent: String,
    #[arg(long)]
    model: Option<String>,
}

#[derive(Debug, Args, Clone)]
pub(crate) struct ModelFlagsCommand {
    #[arg(long)]
    model: String,
    #[arg(long)]
    provider: Option<String>,
}

#[derive(Debug, Args, Clone)]
pub(crate) struct ReasoningFlagsCommand {
    #[arg(long)]
    effort: String,
    #[arg(long)]
    max_tokens: Option<u32>,
}

#[derive(Debug, Args, Clone)]
pub(crate) struct SystemPromptFlagsCommand {
    #[arg(long)]
    prompt: String,
    #[arg(long, default_value_t = false)]
    append: bool,
}

#[derive(Debug, Subcommand, Clone)]
pub(crate) enum AgentFlagsSubcommand {
    Agent(AgentFlagsCommand),
    Model(ModelFlagsCommand),
    Reasoning(ReasoningFlagsCommand),
    SystemPrompt(SystemPromptFlagsCommand),
}

#[derive(Debug, Args, Clone)]
pub(crate) struct AgentFlagsLeafCommand {
    #[command(subcommand)]
    pub(crate) command: AgentFlagsSubcommand,
}

pub(crate) fn execute_with_io(command: AgentFlagsLeafCommand, io: &mut CliIo<'_>) -> i32 {
    match command.command {
        AgentFlagsSubcommand::Agent(cmd) => run_agent(cmd, io),
        AgentFlagsSubcommand::Model(cmd) => run_model(cmd, io),
        AgentFlagsSubcommand::Reasoning(cmd) => run_reasoning(cmd, io),
        AgentFlagsSubcommand::SystemPrompt(cmd) => run_system_prompt(cmd, io),
    }
}

fn run_agent(_command: AgentFlagsCommand, io: &mut CliIo<'_>) -> i32 {
    let _ = writeln!(
        io.stderr,
        "agent: this subcommand is not available; use --agent <profile> to select an agent profile"
    );
    2
}

fn run_model(_command: ModelFlagsCommand, io: &mut CliIo<'_>) -> i32 {
    let _ = writeln!(
        io.stderr,
        "model: this subcommand is not available; use --model <provider/model> to override the model"
    );
    2
}

fn run_reasoning(_command: ReasoningFlagsCommand, io: &mut CliIo<'_>) -> i32 {
    let _ = writeln!(
        io.stderr,
        "reasoning: this subcommand is not available; use --reasoning-effort <level> to set reasoning effort"
    );
    2
}

fn run_system_prompt(_command: SystemPromptFlagsCommand, io: &mut CliIo<'_>) -> i32 {
    let _ = writeln!(
        io.stderr,
        "system-prompt: this subcommand is not available; use --system-prompt-override <prompt> to override the system prompt"
    );
    2
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    fn run_agent(agent: &str) -> (i32, String, String) {
        let mut stdin = Cursor::new(Vec::new());
        let mut stdout = Vec::new();
        let mut stderr = Vec::new();
        let mut io = CliIo::new(&mut stdin, &mut stdout, &mut stderr);
        let cmd = AgentFlagsLeafCommand {
            command: AgentFlagsSubcommand::Agent(AgentFlagsCommand {
                agent: agent.to_string(),
                model: None,
            }),
        };
        let code = execute_with_io(cmd, &mut io);
        (
            code,
            String::from_utf8_lossy(&stdout).to_string(),
            String::from_utf8_lossy(&stderr).to_string(),
        )
    }

    fn run_model(model: &str) -> (i32, String, String) {
        let mut stdin = Cursor::new(Vec::new());
        let mut stdout = Vec::new();
        let mut stderr = Vec::new();
        let mut io = CliIo::new(&mut stdin, &mut stdout, &mut stderr);
        let cmd = AgentFlagsLeafCommand {
            command: AgentFlagsSubcommand::Model(ModelFlagsCommand {
                model: model.to_string(),
                provider: None,
            }),
        };
        let code = execute_with_io(cmd, &mut io);
        (
            code,
            String::from_utf8_lossy(&stdout).to_string(),
            String::from_utf8_lossy(&stderr).to_string(),
        )
    }

    fn run_reasoning(effort: &str) -> (i32, String, String) {
        let mut stdin = Cursor::new(Vec::new());
        let mut stdout = Vec::new();
        let mut stderr = Vec::new();
        let mut io = CliIo::new(&mut stdin, &mut stdout, &mut stderr);
        let cmd = AgentFlagsLeafCommand {
            command: AgentFlagsSubcommand::Reasoning(ReasoningFlagsCommand {
                effort: effort.to_string(),
                max_tokens: None,
            }),
        };
        let code = execute_with_io(cmd, &mut io);
        (
            code,
            String::from_utf8_lossy(&stdout).to_string(),
            String::from_utf8_lossy(&stderr).to_string(),
        )
    }

    fn run_system_prompt(prompt: &str) -> (i32, String, String) {
        let mut stdin = Cursor::new(Vec::new());
        let mut stdout = Vec::new();
        let mut stderr = Vec::new();
        let mut io = CliIo::new(&mut stdin, &mut stdout, &mut stderr);
        let cmd = AgentFlagsLeafCommand {
            command: AgentFlagsSubcommand::SystemPrompt(SystemPromptFlagsCommand {
                prompt: prompt.to_string(),
                append: false,
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
    fn agent_returns_meaningful_failure_directing_to_flag() {
        let (code, stdout, stderr) = run_agent("build");
        assert_eq!(code, 2);
        assert!(stdout.is_empty());
        assert!(stderr.contains("not available"));
        assert!(stderr.contains("--agent"));
    }

    #[test]
    fn model_returns_meaningful_failure_directing_to_flag() {
        let (code, stdout, stderr) = run_model("openai-codex/gpt-5.5");
        assert_eq!(code, 2);
        assert!(stdout.is_empty());
        assert!(stderr.contains("not available"));
        assert!(stderr.contains("--model"));
    }

    #[test]
    fn reasoning_returns_meaningful_failure_directing_to_flag() {
        let (code, stdout, stderr) = run_reasoning("high");
        assert_eq!(code, 2);
        assert!(stdout.is_empty());
        assert!(stderr.contains("not available"));
        assert!(stderr.contains("--reasoning-effort"));
    }

    #[test]
    fn system_prompt_returns_meaningful_failure_directing_to_flag() {
        let (code, stdout, stderr) = run_system_prompt("You are a coding assistant.");
        assert_eq!(code, 2);
        assert!(stdout.is_empty());
        assert!(stderr.contains("not available"));
        assert!(stderr.contains("--system-prompt-override"));
    }
}
