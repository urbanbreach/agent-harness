use std::path::PathBuf;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommandRegistry {
    commands: Vec<CommandSpec>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommandSpec {
    pub name: &'static str,
    pub description: &'static str,
    pub aliases: &'static [&'static str],
    pub action: CommandAction,
    pub enabled_by_default: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CommandAction {
    PromptTemplate { prompt: &'static str },
    LoadSkills { skills: &'static [&'static str] },
    ProfileSwitch { profile: &'static str },
    NativeTool { tool_id: &'static str },
    StartContinuation { mode: ContinuationMode },
    StopContinuation,
    PlanArtifact { artifact: &'static str },
    HandoffArtifact { artifact: &'static str },
    TuiAction { action: &'static str },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ContinuationMode {
    Ralph,
    Ultrawork,
}

impl ContinuationMode {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Ralph => "ralph",
            Self::Ultrawork => "ultrawork",
        }
    }
}

impl CommandRegistry {
    pub fn new(commands: Vec<CommandSpec>) -> Self {
        Self { commands }
    }

    pub fn builtins() -> Self {
        Self::new(vec![
            spec(
                "init-deep",
                "Start a deep requirements interview",
                &["deep-interview"],
                CommandAction::PromptTemplate {
                    prompt: "Deep interview the request before implementation.",
                },
            ),
            spec(
                "ralph-loop",
                "Start bounded Ralph continuation",
                &["ralph"],
                CommandAction::StartContinuation {
                    mode: ContinuationMode::Ralph,
                },
            ),
            spec(
                "ulw-loop",
                "Start bounded ultrawork continuation",
                &["ultrawork", "ulw"],
                CommandAction::StartContinuation {
                    mode: ContinuationMode::Ultrawork,
                },
            ),
            spec(
                "cancel-ralph",
                "Stop Ralph continuation",
                &[],
                CommandAction::StopContinuation,
            ),
            spec(
                "stop-continuation",
                "Stop active continuation",
                &["stop-continuation"],
                CommandAction::StopContinuation,
            ),
            spec(
                "refactor",
                "Load refactor cleanup guidance",
                &[],
                CommandAction::LoadSkills {
                    skills: &["ai-slop-remover"],
                },
            ),
            spec(
                "start-work",
                "Create a work handoff artifact",
                &[],
                CommandAction::PlanArtifact {
                    artifact: "work-start",
                },
            ),
            spec(
                "remove-ai-slops",
                "Load AI slop removal guidance",
                &["deslop"],
                CommandAction::LoadSkills {
                    skills: &["ai-slop-remover"],
                },
            ),
            spec(
                "handoff",
                "Create a continuation handoff artifact",
                &[],
                CommandAction::HandoffArtifact {
                    artifact: "handoff",
                },
            ),
            spec(
                "hyperplan",
                "Start team/parallel planning handoff",
                &[],
                CommandAction::PlanArtifact {
                    artifact: "hyperplan",
                },
            ),
        ])
    }

    pub fn commands(&self) -> &[CommandSpec] {
        &self.commands
    }

    pub fn get(&self, name_or_alias: &str) -> Option<&CommandSpec> {
        self.commands.iter().find(|command| {
            command.name == name_or_alias || command.aliases.contains(&name_or_alias)
        })
    }

    pub fn roots() -> Vec<PathBuf> {
        vec![
            PathBuf::from(".agent-harness/commands"),
            PathBuf::from(".opencode/command"),
            PathBuf::from(".opencode/commands"),
            PathBuf::from(".claude/commands"),
            PathBuf::from(".agents/commands"),
            PathBuf::from(".harness/commands"),
            PathBuf::from("~/.config/agent-harness/commands"),
            PathBuf::from("~/.config/opencode/command"),
            PathBuf::from("~/.claude/commands"),
            PathBuf::from("~/.agents/commands"),
        ]
    }
}

fn spec(
    name: &'static str,
    description: &'static str,
    aliases: &'static [&'static str],
    action: CommandAction,
) -> CommandSpec {
    CommandSpec {
        name,
        description,
        aliases,
        action,
        enabled_by_default: true,
    }
}

#[cfg(test)]
mod tests {
    use super::{CommandAction, CommandRegistry};

    #[test]
    fn builtin_registry_exposes_omo_commands_without_shell_actions() {
        let registry = CommandRegistry::builtins();
        for name in [
            "init-deep",
            "ralph-loop",
            "ulw-loop",
            "cancel-ralph",
            "refactor",
            "start-work",
            "stop-continuation",
            "remove-ai-slops",
            "handoff",
            "hyperplan",
        ] {
            assert!(registry.get(name).is_some(), "missing {name}");
        }

        assert!(registry.commands().iter().all(|command| !matches!(
            command.action,
            CommandAction::NativeTool {
                tool_id: "bash" | "shell.run"
            }
        )));
        assert!(CommandRegistry::roots()
            .iter()
            .any(|root| root == &std::path::PathBuf::from(".opencode/command")));
    }
}
