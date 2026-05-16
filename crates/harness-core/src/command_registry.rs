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
    WorkflowIntent { intent: WorkflowIntent },
    StartContinuation { mode: ContinuationMode },
    StopContinuation,
    PlanArtifact { artifact: &'static str },
    HandoffArtifact { artifact: &'static str },
    TuiAction { action: &'static str },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WorkflowIntent {
    Run,
    Status,
    Signoff,
    Cancel,
    DossierExport,
    Snapshot,
    PlanConsensus,
    GoalLedger,
    ResearchMission,
    Wiki,
}

impl WorkflowIntent {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Run => "workflow.run",
            Self::Status => "workflow.status",
            Self::Signoff => "workflow.signoff",
            Self::Cancel => "workflow.cancel",
            Self::DossierExport => "workflow.dossier_export",
            Self::Snapshot => "workflow.snapshot",
            Self::PlanConsensus => "workflow.plan_consensus",
            Self::GoalLedger => "workflow.goal_ledger",
            Self::ResearchMission => "workflow.research_mission",
            Self::Wiki => "workflow.wiki",
        }
    }
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
                "workflow-run",
                "Start a coordinator-owned workflow run",
                &["workflow"],
                CommandAction::WorkflowIntent {
                    intent: WorkflowIntent::Run,
                },
            ),
            spec(
                "workflow-status",
                "Inspect projected workflow status",
                &["status-workflow"],
                CommandAction::WorkflowIntent {
                    intent: WorkflowIntent::Status,
                },
            ),
            spec(
                "workflow-signoff",
                "Record or inspect workflow signoff decisions",
                &["signoff"],
                CommandAction::WorkflowIntent {
                    intent: WorkflowIntent::Signoff,
                },
            ),
            spec(
                "workflow-cancel",
                "Cancel a coordinator-owned workflow run",
                &["cancel-workflow"],
                CommandAction::WorkflowIntent {
                    intent: WorkflowIntent::Cancel,
                },
            ),
            spec(
                "workflow-dossier",
                "Export a replay-derived workflow Run Dossier",
                &["dossier"],
                CommandAction::WorkflowIntent {
                    intent: WorkflowIntent::DossierExport,
                },
            ),
            spec(
                "workflow-snapshot",
                "List, read, or export workflow context snapshots",
                &["snapshot"],
                CommandAction::WorkflowIntent {
                    intent: WorkflowIntent::Snapshot,
                },
            ),
            spec(
                "plan-consensus",
                "Create a reviewed consensus plan artifact",
                &["ralplan", "consensus-plan"],
                CommandAction::WorkflowIntent {
                    intent: WorkflowIntent::PlanConsensus,
                },
            ),
            spec(
                "goal-ledger",
                "Inspect or checkpoint workflow goal ledger state",
                &["ultragoal"],
                CommandAction::WorkflowIntent {
                    intent: WorkflowIntent::GoalLedger,
                },
            ),
            spec(
                "research-mission",
                "Create or inspect validator-gated research mission state",
                &["research-loop", "autoresearch"],
                CommandAction::WorkflowIntent {
                    intent: WorkflowIntent::ResearchMission,
                },
            ),
            spec(
                "wiki",
                "Read, query, or update the markdown workflow wiki",
                &["workflow-wiki"],
                CommandAction::WorkflowIntent {
                    intent: WorkflowIntent::Wiki,
                },
            ),
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
                &[],
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
    use std::collections::BTreeSet;

    use super::{CommandAction, CommandRegistry, WorkflowIntent};

    #[test]
    fn builtin_registry_exposes_omo_commands_without_shell_actions() {
        let registry = CommandRegistry::builtins();
        for name in [
            "workflow-run",
            "workflow-status",
            "workflow-signoff",
            "workflow-cancel",
            "workflow-dossier",
            "workflow-snapshot",
            "plan-consensus",
            "goal-ledger",
            "research-mission",
            "wiki",
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

    #[test]
    fn workflow_commands_have_stable_intents_and_unique_aliases() {
        let registry = CommandRegistry::builtins();
        for (name, intent) in [
            ("workflow-run", WorkflowIntent::Run),
            ("workflow-status", WorkflowIntent::Status),
            ("workflow-signoff", WorkflowIntent::Signoff),
            ("workflow-cancel", WorkflowIntent::Cancel),
            ("workflow-dossier", WorkflowIntent::DossierExport),
            ("workflow-snapshot", WorkflowIntent::Snapshot),
            ("plan-consensus", WorkflowIntent::PlanConsensus),
            ("goal-ledger", WorkflowIntent::GoalLedger),
            ("research-mission", WorkflowIntent::ResearchMission),
            ("wiki", WorkflowIntent::Wiki),
        ] {
            let command = registry
                .get(name)
                .unwrap_or_else(|| panic!("missing {name}"));
            assert_eq!(
                command.action,
                CommandAction::WorkflowIntent { intent },
                "{name} drifted from workflow intent {}",
                intent.as_str()
            );
        }

        let mut names_and_aliases = BTreeSet::new();
        for command in registry.commands() {
            assert!(
                names_and_aliases.insert(command.name),
                "duplicate command name {}",
                command.name
            );
            for alias in command.aliases {
                assert!(
                    names_and_aliases.insert(alias),
                    "duplicate command alias {alias}"
                );
            }
        }
    }
}
