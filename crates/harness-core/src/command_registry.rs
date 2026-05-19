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
    pub dollar_aliases: &'static [&'static str],
    pub dollar_alias_descriptions: &'static [(&'static str, &'static str)],
    pub surface: CommandSurface,
    pub effect: CommandEffect,
    pub action: CommandAction,
    pub availability: WorkflowCommandAvailability,
    pub enabled_by_default: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CommandSurface {
    WorkflowCommand,
    ContinuationCommand,
    StagedReferenceCommand,
    PromptTemplate,
    ProfileSwitch,
    NativeTool,
    AgentShortcut,
    TuiAction,
}

impl CommandSurface {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::WorkflowCommand => "workflow_command",
            Self::ContinuationCommand => "continuation_command",
            Self::StagedReferenceCommand => "staged_reference_command",
            Self::PromptTemplate => "prompt_template",
            Self::ProfileSwitch => "profile_switch",
            Self::NativeTool => "native_tool",
            Self::AgentShortcut => "agent_shortcut",
            Self::TuiAction => "tui_action",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CommandEffect {
    ReadProjection,
    MutateCoordinatorState,
    ControlContinuation,
    BlockedNoSideEffect,
    SubmitPrompt,
    SwitchProfile,
    InvokeNativeTool,
    ScheduleAgentTask,
    UpdateTuiState,
}

impl CommandEffect {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::ReadProjection => "read_projection",
            Self::MutateCoordinatorState => "mutate_coordinator_state",
            Self::ControlContinuation => "control_continuation",
            Self::BlockedNoSideEffect => "blocked_no_side_effect",
            Self::SubmitPrompt => "submit_prompt",
            Self::SwitchProfile => "switch_profile",
            Self::InvokeNativeTool => "invoke_native_tool",
            Self::ScheduleAgentTask => "schedule_agent_task",
            Self::UpdateTuiState => "update_tui_state",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WorkflowCommandAvailability {
    Present,
    Staged,
    Rejected,
}

impl WorkflowCommandAvailability {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Present => "present",
            Self::Staged => "staged",
            Self::Rejected => "rejected",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CommandAction {
    PromptTemplate {
        prompt: &'static str,
    },
    LoadSkills {
        skills: &'static [&'static str],
    },
    ProfileSwitch {
        profile: &'static str,
    },
    NativeTool {
        tool_id: &'static str,
    },
    SlashAgent {
        role: &'static str,
    },
    WorkflowIntent {
        intent: WorkflowIntent,
    },
    StartContinuation {
        mode: ContinuationMode,
    },
    StopContinuation,
    BlockedWorkflow {
        reason: &'static str,
        inventory_ref: &'static str,
    },
    PlanArtifact {
        artifact: &'static str,
    },
    HandoffArtifact {
        artifact: &'static str,
    },
    TuiAction {
        action: &'static str,
    },
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
    DeepInterview,
    Team,
    Autopilot,
    Analyze,
    Review,
    SecurityReview,
    Doctor,
    Help,
    Hud,
    Note,
    Skill,
    Trace,
    ConfigureNotifications,
    Design,
    Cleanup,
    Qa,
    Performance,
    Pipeline,
    Tdd,
    Visual,
    WebClone,
    Ecomode,
    DeepSearch,
    RalphInit,
    StartWork,
    Handoff,
    Hyperplan,
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
            Self::DeepInterview => "workflow.deep_interview",
            Self::Team => "workflow.team_escalation",
            Self::Autopilot => "workflow.autopilot",
            Self::Analyze => "workflow.analysis",
            Self::Review => "workflow.review",
            Self::SecurityReview => "workflow.security_review",
            Self::Doctor => "workflow.doctor",
            Self::Help => "workflow.help",
            Self::Hud => "workflow.hud",
            Self::Note => "workflow.note",
            Self::Skill => "workflow.skill_management",
            Self::Trace => "workflow.trace",
            Self::ConfigureNotifications => "workflow.configure_notifications",
            Self::Design => "workflow.design",
            Self::Cleanup => "workflow.cleanup",
            Self::Qa => "workflow.qa",
            Self::Performance => "workflow.performance",
            Self::Pipeline => "workflow.pipeline",
            Self::Tdd => "workflow.tdd",
            Self::Visual => "workflow.visual",
            Self::WebClone => "workflow.web_clone",
            Self::Ecomode => "workflow.ecomode",
            Self::DeepSearch => "workflow.deepsearch",
            Self::RalphInit => "workflow.ralph_init",
            Self::StartWork => "workflow.start_work",
            Self::Handoff => "workflow.handoff",
            Self::Hyperplan => "workflow.hyperplan",
        }
    }

    pub fn effect(self) -> CommandEffect {
        match self {
            Self::Status | Self::DossierExport => CommandEffect::ReadProjection,
            Self::Run
            | Self::Signoff
            | Self::Cancel
            | Self::Snapshot
            | Self::PlanConsensus
            | Self::GoalLedger
            | Self::ResearchMission
            | Self::Wiki
            | Self::DeepInterview
            | Self::Team
            | Self::Autopilot
            | Self::Analyze
            | Self::Review
            | Self::SecurityReview
            | Self::Doctor
            | Self::Help
            | Self::Hud
            | Self::Note
            | Self::Skill
            | Self::Trace
            | Self::ConfigureNotifications
            | Self::Design
            | Self::Cleanup
            | Self::Qa
            | Self::Performance
            | Self::Pipeline
            | Self::Tdd
            | Self::Visual
            | Self::WebClone
            | Self::Ecomode
            | Self::DeepSearch
            | Self::RalphInit
            | Self::StartWork
            | Self::Handoff
            | Self::Hyperplan => CommandEffect::MutateCoordinatorState,
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
                &["plan", "ralplan", "consensus-plan", "workflow-plan-consensus"],
                CommandAction::WorkflowIntent {
                    intent: WorkflowIntent::PlanConsensus,
                },
            )
            .with_dollar_aliases(&["plan", "ralplan"])
            .with_dollar_alias_descriptions(&[
                ("plan", "Create a strategic plan with optional interview workflow"),
                ("ralplan", "Create a reviewed consensus plan"),
            ]),
            spec(
                "goal-ledger",
                "Inspect or checkpoint workflow goal ledger state",
                &["goal", "ultragoal", "workflow-goal"],
                CommandAction::WorkflowIntent {
                    intent: WorkflowIntent::GoalLedger,
                },
            )
            .with_dollar_aliases(&["goal", "ultragoal"])
            .with_dollar_alias_descriptions(&[
                ("goal", "Inspect or checkpoint workflow goal ledger state"),
                (
                    "ultragoal",
                    "Create and execute durable repo-native multi-goal plans over goal artifacts",
                ),
            ]),
            spec(
                "research-mission",
                "Create or inspect validator-gated research mission state",
                &[
                    "mission",
                    "research-loop",
                    "autoresearch",
                    "workflow-mission",
                ],
                CommandAction::WorkflowIntent {
                    intent: WorkflowIntent::ResearchMission,
                },
            )
            .with_dollar_aliases(&["autoresearch"]),
            spec(
                "wiki",
                "Read, query, or update the markdown workflow wiki",
                &["workflow-wiki"],
                CommandAction::WorkflowIntent {
                    intent: WorkflowIntent::Wiki,
                },
            )
            .with_dollar_aliases(&["wiki"]),
            spec(
                "init-deep",
                "Run one-question-at-a-time intake with mathematical ambiguity gating before execution",
                &["deep-interview"],
                CommandAction::WorkflowIntent {
                    intent: WorkflowIntent::DeepInterview,
                },
            )
            .with_dollar_aliases(&["deep-interview"]),
            spec(
                "ralph-loop",
                "Run a self-referential completion loop with architect verification",
                &["ralph"],
                CommandAction::StartContinuation {
                    mode: ContinuationMode::Ralph,
                },
            )
            .with_dollar_aliases(&["ralph"]),
            spec(
                "ulw-loop",
                "Run parallel execution for high-throughput task completion",
                &["ultrawork", "ulw"],
                CommandAction::StartContinuation {
                    mode: ContinuationMode::Ultrawork,
                },
            )
            .with_dollar_aliases(&["ultrawork"]),
            spec(
                "cancel-ralph",
                "Stop Ralph continuation",
                &["stop-ralph"],
                CommandAction::StopContinuation,
            ),
            spec(
                "stop-continuation",
                "Cancel an active workflow or continuation mode",
                &[],
                CommandAction::StopContinuation,
            )
            .with_dollar_aliases(&["cancel"]),
            spec(
                "refactor",
                "Run a refactor workflow with behavior locks and verification evidence",
                &[],
                CommandAction::WorkflowIntent {
                    intent: WorkflowIntent::Cleanup,
                },
            )
            .with_dollar_aliases(&["refactor"]),
            spec(
                "start-work",
                "Create workflow-owned work-start handoff evidence",
                &[],
                CommandAction::WorkflowIntent {
                    intent: WorkflowIntent::StartWork,
                },
            ),
            spec(
                "remove-ai-slops",
                "Run an anti-slop cleanup, refactor, or deslop workflow",
                &["deslop", "ai-slop-cleaner"],
                CommandAction::WorkflowIntent {
                    intent: WorkflowIntent::Cleanup,
                },
            )
            .with_dollar_aliases(&["ai-slop-cleaner", "deslop"]),
            spec(
                "handoff",
                "Write workflow-owned handoff evidence and closeout readiness notes",
                &[],
                CommandAction::WorkflowIntent {
                    intent: WorkflowIntent::Handoff,
                },
            ),
            spec(
                "hyperplan",
                "Create workflow-owned team and parallel handoff planning evidence",
                &[],
                CommandAction::WorkflowIntent {
                    intent: WorkflowIntent::Hyperplan,
                },
            ),
            spec(
                "omx-skill:team",
                "Coordinate multiple agents on a shared task list",
                &["team"],
                CommandAction::WorkflowIntent {
                    intent: WorkflowIntent::Team,
                },
            )
            .with_dollar_aliases(&["team"]),
            spec(
                "omx-skill:swarm",
                "Coordinate team-style parallel execution through the team workflow",
                &["swarm"],
                CommandAction::WorkflowIntent {
                    intent: WorkflowIntent::Team,
                },
            )
            .with_dollar_aliases(&["swarm"]),
            spec(
                "omx-skill:ultraqa",
                "Run adversarial dynamic end-to-end QA: generate hostile scenarios, test, verify, fix, report, and clean up",
                &["ultraqa"],
                CommandAction::WorkflowIntent {
                    intent: WorkflowIntent::Qa,
                },
            )
            .with_dollar_aliases(&["ultraqa"]),
            spec(
                "omx-skill:analyze",
                "Run read-only deep repository analysis with ranked findings, explicit confidence, and file evidence",
                &["analyze"],
                CommandAction::WorkflowIntent {
                    intent: WorkflowIntent::Analyze,
                },
            )
            .with_dollar_aliases(&["analyze"]),
            spec(
                "omx-skill:code-review",
                "Run a comprehensive code review",
                &["code-review"],
                CommandAction::WorkflowIntent {
                    intent: WorkflowIntent::Review,
                },
            )
            .with_dollar_aliases(&["code-review"]),
            spec(
                "omx-skill:review",
                "Run a review workflow and record findings as workflow evidence",
                &["review"],
                CommandAction::WorkflowIntent {
                    intent: WorkflowIntent::Review,
                },
            )
            .with_dollar_aliases(&["review"]),
            spec(
                "omx-skill:security-review",
                "Run a security review for vulnerabilities, trust boundaries, authentication, and authorization",
                &["security-review"],
                CommandAction::WorkflowIntent {
                    intent: WorkflowIntent::SecurityReview,
                },
            )
            .with_dollar_aliases(&["security-review"]),
            spec(
                "omx-skill:doctor",
                "Diagnose and fix Harness installation and runtime issues",
                &["doctor"],
                CommandAction::WorkflowIntent {
                    intent: WorkflowIntent::Doctor,
                },
            )
            .with_dollar_aliases(&["doctor"]),
            spec(
                "omx-skill:help",
                "Show Harness workflow and command help",
                &["help"],
                CommandAction::WorkflowIntent {
                    intent: WorkflowIntent::Help,
                },
            )
            .with_dollar_aliases(&["help"]),
            spec(
                "omx-skill:hud",
                "Show or configure the Harness HUD and status projection",
                &["hud"],
                CommandAction::WorkflowIntent {
                    intent: WorkflowIntent::Hud,
                },
            )
            .with_dollar_aliases(&["hud"]),
            spec(
                "omx-skill:note",
                "Capture a workflow note or project-memory evidence artifact",
                &["note"],
                CommandAction::WorkflowIntent {
                    intent: WorkflowIntent::Note,
                },
            )
            .with_dollar_aliases(&["note"]),
            spec(
                "omx-skill:skill",
                "Manage local skills: list, add, remove, search, edit, and verify",
                &["skill"],
                CommandAction::WorkflowIntent {
                    intent: WorkflowIntent::Skill,
                },
            )
            .with_dollar_aliases(&["skill"]),
            spec(
                "omx-skill:trace",
                "Show agent flow trace timeline and summary",
                &["trace"],
                CommandAction::WorkflowIntent {
                    intent: WorkflowIntent::Trace,
                },
            )
            .with_dollar_aliases(&["trace"]),
            spec(
                "omx-skill:configure-notifications",
                "Configure Harness notifications through an explicit workflow",
                &["configure-notifications"],
                CommandAction::WorkflowIntent {
                    intent: WorkflowIntent::ConfigureNotifications,
                },
            )
            .with_dollar_aliases(&["configure-notifications"]),
            spec(
                "omx-skill:design",
                "Maintain a canonical repo-local design source of truth for product, UI, UX, and frontend decisions",
                &["design"],
                CommandAction::WorkflowIntent {
                    intent: WorkflowIntent::Design,
                },
            )
            .with_dollar_aliases(&["design"]),
            spec(
                "omx-skill:frontend-ui-ux",
                "Route frontend UI and UX work through design or visual workflow evidence",
                &["frontend-ui-ux"],
                CommandAction::WorkflowIntent {
                    intent: WorkflowIntent::Design,
                },
            )
            .with_dollar_aliases(&["frontend-ui-ux"]),
            spec(
                "omx-skill:autopilot",
                "Run an autonomous loop over planning, completion, and code review gates",
                &["autopilot"],
                CommandAction::WorkflowIntent {
                    intent: WorkflowIntent::Autopilot,
                },
            )
            .with_dollar_aliases(&["autopilot"]),
            spec(
                "omx-skill:autoresearch-goal",
                "Run a durable professor-critic research workflow over goal artifacts",
                &["autoresearch-goal"],
                CommandAction::WorkflowIntent {
                    intent: WorkflowIntent::ResearchMission,
                },
            )
            .with_dollar_aliases(&["autoresearch-goal"]),
            spec(
                "omx-skill:deepsearch",
                "Run a deep search workflow with research and evidence capture",
                &["deepsearch"],
                CommandAction::WorkflowIntent {
                    intent: WorkflowIntent::DeepSearch,
                },
            )
            .with_dollar_aliases(&["deepsearch"]),
            spec(
                "omx-skill:performance-goal",
                "Run an evaluator-gated performance optimization workflow with durable artifacts and safe goal handoffs",
                &["performance-goal"],
                CommandAction::WorkflowIntent {
                    intent: WorkflowIntent::Performance,
                },
            )
            .with_dollar_aliases(&["performance-goal"]),
            spec(
                "omx-skill:pipeline",
                "Run a configurable pipeline orchestrator for sequencing workflow stages",
                &["pipeline"],
                CommandAction::WorkflowIntent {
                    intent: WorkflowIntent::Pipeline,
                },
            )
            .with_dollar_aliases(&["pipeline"]),
            spec(
                "omx-skill:ecomode",
                "Apply token-efficient model-routing guidance through a Harness workflow",
                &["ecomode"],
                CommandAction::WorkflowIntent {
                    intent: WorkflowIntent::Ecomode,
                },
            )
            .with_dollar_aliases(&["ecomode"]),
            spec(
                "omx-skill:tdd",
                "Run a test-driven-development workflow with test-first state and verification evidence",
                &["tdd"],
                CommandAction::WorkflowIntent {
                    intent: WorkflowIntent::Tdd,
                },
            )
            .with_dollar_aliases(&["tdd"]),
            spec(
                "omx-skill:git-master",
                "Use a git expert for atomic commits, rebasing, and history hygiene",
                &["git-master"],
                CommandAction::SlashAgent {
                    role: "git-master",
                },
            )
            .with_dollar_aliases(&["git-master"]),
            spec(
                "omx-skill:visual-ralph",
                "Run a measured visual-reference implementation loop with verdict and pixel-diff evidence",
                &["visual-ralph"],
                CommandAction::WorkflowIntent {
                    intent: WorkflowIntent::Visual,
                },
            )
            .with_dollar_aliases(&["visual-ralph"]),
            spec(
                "omx-skill:visual-verdict",
                "Run structured visual QA verdicts for screenshot-to-reference comparisons",
                &["visual-verdict"],
                CommandAction::WorkflowIntent {
                    intent: WorkflowIntent::Visual,
                },
            )
            .with_dollar_aliases(&["visual-verdict"]),
            spec(
                "omx-skill:web-clone",
                "Clone a website from a URL with visual and functional verification evidence",
                &["web-clone"],
                CommandAction::WorkflowIntent {
                    intent: WorkflowIntent::WebClone,
                },
            )
            .with_dollar_aliases(&["web-clone"]),
            spec(
                "omx-skill:ralph-init",
                "Initialize a Ralph-style completion workflow through the bounded continuation contract",
                &["ralph-init"],
                CommandAction::WorkflowIntent {
                    intent: WorkflowIntent::RalphInit,
                },
            )
            .with_dollar_aliases(&["ralph-init"]),
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

impl CommandSpec {
    fn with_dollar_aliases(mut self, aliases: &'static [&'static str]) -> Self {
        self.dollar_aliases = aliases;
        self
    }

    fn with_dollar_alias_descriptions(
        mut self,
        descriptions: &'static [(&'static str, &'static str)],
    ) -> Self {
        self.dollar_alias_descriptions = descriptions;
        self
    }

    pub fn dollar_alias_description(&self, alias: &str) -> &'static str {
        self.dollar_alias_descriptions
            .iter()
            .find_map(|(name, description)| (*name == alias).then_some(*description))
            .unwrap_or(self.description)
    }
}

fn spec(
    name: &'static str,
    description: &'static str,
    aliases: &'static [&'static str],
    action: CommandAction,
) -> CommandSpec {
    let surface = action.surface(WorkflowCommandAvailability::Present);
    let effect = action.effect();
    CommandSpec {
        name,
        description,
        aliases,
        dollar_aliases: &[],
        dollar_alias_descriptions: &[],
        surface,
        effect,
        action,
        availability: WorkflowCommandAvailability::Present,
        enabled_by_default: true,
    }
}

impl CommandAction {
    pub fn surface(&self, availability: WorkflowCommandAvailability) -> CommandSurface {
        match self {
            Self::WorkflowIntent { .. } => CommandSurface::WorkflowCommand,
            Self::StartContinuation { .. } | Self::StopContinuation => {
                CommandSurface::ContinuationCommand
            }
            Self::BlockedWorkflow { .. } => match availability {
                WorkflowCommandAvailability::Present => CommandSurface::WorkflowCommand,
                WorkflowCommandAvailability::Staged | WorkflowCommandAvailability::Rejected => {
                    CommandSurface::StagedReferenceCommand
                }
            },
            Self::PromptTemplate { .. } => CommandSurface::PromptTemplate,
            Self::LoadSkills { .. } => CommandSurface::PromptTemplate,
            Self::ProfileSwitch { .. } => CommandSurface::ProfileSwitch,
            Self::NativeTool { .. } => CommandSurface::NativeTool,
            Self::SlashAgent { .. } => CommandSurface::AgentShortcut,
            Self::PlanArtifact { .. } | Self::HandoffArtifact { .. } => {
                CommandSurface::StagedReferenceCommand
            }
            Self::TuiAction { .. } => CommandSurface::TuiAction,
        }
    }

    pub fn effect(&self) -> CommandEffect {
        match self {
            Self::WorkflowIntent { intent } => intent.effect(),
            Self::StartContinuation { .. } | Self::StopContinuation => {
                CommandEffect::ControlContinuation
            }
            Self::BlockedWorkflow { .. }
            | Self::PlanArtifact { .. }
            | Self::HandoffArtifact { .. } => CommandEffect::BlockedNoSideEffect,
            Self::PromptTemplate { .. } | Self::LoadSkills { .. } => CommandEffect::SubmitPrompt,
            Self::ProfileSwitch { .. } => CommandEffect::SwitchProfile,
            Self::NativeTool { .. } => CommandEffect::InvokeNativeTool,
            Self::SlashAgent { .. } => CommandEffect::ScheduleAgentTask,
            Self::TuiAction { .. } => CommandEffect::UpdateTuiState,
        }
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use super::{
        CommandAction, CommandEffect, CommandRegistry, CommandSurface, WorkflowCommandAvailability,
        WorkflowIntent,
    };

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
            "omx-skill:team",
            "omx-skill:swarm",
            "omx-skill:ultraqa",
            "omx-skill:analyze",
            "omx-skill:code-review",
            "omx-skill:review",
            "omx-skill:security-review",
            "omx-skill:doctor",
            "omx-skill:help",
            "omx-skill:hud",
            "omx-skill:note",
            "omx-skill:skill",
            "omx-skill:trace",
            "omx-skill:configure-notifications",
            "omx-skill:design",
            "omx-skill:frontend-ui-ux",
            "omx-skill:autopilot",
            "omx-skill:autoresearch-goal",
            "omx-skill:deepsearch",
            "omx-skill:performance-goal",
            "omx-skill:pipeline",
            "omx-skill:ecomode",
            "omx-skill:tdd",
            "omx-skill:git-master",
            "omx-skill:visual-ralph",
            "omx-skill:visual-verdict",
            "omx-skill:web-clone",
            "omx-skill:ralph-init",
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
            ("init-deep", WorkflowIntent::DeepInterview),
            ("refactor", WorkflowIntent::Cleanup),
            ("start-work", WorkflowIntent::StartWork),
            ("remove-ai-slops", WorkflowIntent::Cleanup),
            ("handoff", WorkflowIntent::Handoff),
            ("hyperplan", WorkflowIntent::Hyperplan),
            ("omx-skill:team", WorkflowIntent::Team),
            ("omx-skill:swarm", WorkflowIntent::Team),
            ("omx-skill:ultraqa", WorkflowIntent::Qa),
            ("omx-skill:analyze", WorkflowIntent::Analyze),
            ("omx-skill:code-review", WorkflowIntent::Review),
            ("omx-skill:review", WorkflowIntent::Review),
            ("omx-skill:security-review", WorkflowIntent::SecurityReview),
            ("omx-skill:doctor", WorkflowIntent::Doctor),
            ("omx-skill:help", WorkflowIntent::Help),
            ("omx-skill:hud", WorkflowIntent::Hud),
            ("omx-skill:note", WorkflowIntent::Note),
            ("omx-skill:skill", WorkflowIntent::Skill),
            ("omx-skill:trace", WorkflowIntent::Trace),
            (
                "omx-skill:configure-notifications",
                WorkflowIntent::ConfigureNotifications,
            ),
            ("omx-skill:design", WorkflowIntent::Design),
            ("omx-skill:frontend-ui-ux", WorkflowIntent::Design),
            ("omx-skill:autopilot", WorkflowIntent::Autopilot),
            (
                "omx-skill:autoresearch-goal",
                WorkflowIntent::ResearchMission,
            ),
            ("omx-skill:deepsearch", WorkflowIntent::DeepSearch),
            ("omx-skill:performance-goal", WorkflowIntent::Performance),
            ("omx-skill:pipeline", WorkflowIntent::Pipeline),
            ("omx-skill:ecomode", WorkflowIntent::Ecomode),
            ("omx-skill:tdd", WorkflowIntent::Tdd),
            ("omx-skill:visual-ralph", WorkflowIntent::Visual),
            ("omx-skill:visual-verdict", WorkflowIntent::Visual),
            ("omx-skill:web-clone", WorkflowIntent::WebClone),
            ("omx-skill:ralph-init", WorkflowIntent::RalphInit),
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
            assert_eq!(command.surface, CommandSurface::WorkflowCommand);
            assert_eq!(command.effect, intent.effect());
        }

        let mut names_and_aliases = BTreeSet::new();
        let mut dollar_aliases = BTreeSet::new();
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
            for alias in command.dollar_aliases {
                assert!(
                    dollar_aliases.insert(alias),
                    "duplicate dollar command alias {alias}"
                );
            }
        }
    }

    #[test]
    fn enabled_registry_commands_do_not_use_prompt_or_artifact_placeholders() {
        let registry = CommandRegistry::builtins();

        for command in registry
            .commands()
            .iter()
            .filter(|command| command.enabled_by_default)
        {
            assert!(
                !matches!(
                    command.action,
                    CommandAction::PromptTemplate { .. }
                        | CommandAction::LoadSkills { .. }
                        | CommandAction::PlanArtifact { .. }
                        | CommandAction::HandoffArtifact { .. }
                        | CommandAction::NativeTool { .. }
                ),
                "{} is enabled by default through a placeholder action",
                command.name
            );
        }

        for command in registry
            .commands()
            .iter()
            .filter(|command| matches!(&command.action, CommandAction::BlockedWorkflow { .. }))
        {
            assert!(
                !command.enabled_by_default,
                "{} must remain hidden until its blocked workflow action is implemented",
                command.name
            );
            assert_eq!(
                command.availability,
                WorkflowCommandAvailability::Staged,
                "{} blocked workflow must be explicitly classified as staged",
                command.name
            );
            let CommandAction::BlockedWorkflow {
                reason,
                inventory_ref,
            } = &command.action
            else {
                unreachable!("filtered blocked commands");
            };
            assert!(
                !reason.trim().is_empty(),
                "{} blocked workflow reason is empty",
                command.name
            );
            assert_eq!(
                *inventory_ref, command.name,
                "{} blocked workflow inventory ref should point at the command entry",
                command.name
            );
        }
    }

    #[test]
    fn command_availability_is_truthful_for_visible_and_staged_workflows() {
        let registry = CommandRegistry::builtins();
        let mut has_present = false;
        let mut has_staged = false;

        for command in registry.commands() {
            assert!(
                !command.description.trim().is_empty(),
                "{} should have an operator-facing purpose",
                command.name
            );
            match command.availability {
                WorkflowCommandAvailability::Present => {
                    has_present = true;
                    assert!(
                        command.enabled_by_default,
                        "{} present command should be visible by default",
                        command.name
                    );
                    assert!(
                        !matches!(&command.action, CommandAction::BlockedWorkflow { .. }),
                        "{} present command cannot be a blocked workflow",
                        command.name
                    );
                    assert_ne!(
                        command.effect,
                        CommandEffect::BlockedNoSideEffect,
                        "{} present command cannot have blocked/no-side-effect semantics",
                        command.name
                    );
                }
                WorkflowCommandAvailability::Staged => {
                    has_staged = true;
                    assert!(
                        !command.enabled_by_default,
                        "{} staged command must not be visible by default",
                        command.name
                    );
                    assert!(
                        matches!(&command.action, CommandAction::BlockedWorkflow { .. }),
                        "{} staged command must fail closed instead of faking completion",
                        command.name
                    );
                    assert_eq!(
                        command.surface,
                        CommandSurface::StagedReferenceCommand,
                        "{} staged command must remain on the hidden reference surface",
                        command.name
                    );
                    assert_eq!(
                        command.effect,
                        CommandEffect::BlockedNoSideEffect,
                        "{} staged command must have no runtime side effect",
                        command.name
                    );
                }
                WorkflowCommandAvailability::Rejected => {
                    assert!(
                        !command.enabled_by_default,
                        "{} rejected command must never be visible by default",
                        command.name
                    );
                    assert!(
                        matches!(&command.action, CommandAction::BlockedWorkflow { .. }),
                        "{} rejected command must fail closed instead of dispatching",
                        command.name
                    );
                }
            }
        }

        assert!(
            has_present,
            "registry should classify implemented workflows"
        );
        assert!(
            !has_staged,
            "applicable dollar workflow rows should be implemented, not staged"
        );
    }

    #[test]
    fn omx_compatibility_aliases_resolve_without_prompt_only_dispatch() {
        let registry = CommandRegistry::builtins();
        for (alias, expected_name) in [
            ("deep-interview", "init-deep"),
            ("plan", "plan-consensus"),
            ("ralplan", "plan-consensus"),
            ("ralph", "ralph-loop"),
            ("ultrawork", "ulw-loop"),
            ("ultragoal", "goal-ledger"),
            ("autoresearch", "research-mission"),
            ("ai-slop-cleaner", "remove-ai-slops"),
        ] {
            let command = registry
                .get(alias)
                .unwrap_or_else(|| panic!("missing compatibility alias {alias}"));
            assert_eq!(
                command.name, expected_name,
                "{alias} resolved to unexpected command"
            );
            assert!(
                !matches!(command.action, CommandAction::PromptTemplate { .. }),
                "{alias} must not resolve to prompt-only dispatch"
            );
        }
    }

    #[test]
    fn dollar_workflow_family_aliases_resolve_to_native_or_fail_closed_actions() {
        let registry = CommandRegistry::builtins();

        for alias in [
            "ai-slop-cleaner",
            "analyze",
            "autopilot",
            "autoresearch",
            "autoresearch-goal",
            "code-review",
            "configure-notifications",
            "deep-interview",
            "deepsearch",
            "design",
            "deslop",
            "doctor",
            "ecomode",
            "frontend-ui-ux",
            "git-master",
            "goal",
            "help",
            "hud",
            "note",
            "performance-goal",
            "pipeline",
            "plan",
            "ralph",
            "ralph-init",
            "ralplan",
            "refactor",
            "review",
            "security-review",
            "skill",
            "swarm",
            "tdd",
            "team",
            "trace",
            "ultragoal",
            "ultraqa",
            "ultrawork",
            "visual-ralph",
            "visual-verdict",
            "web-clone",
            "wiki",
        ] {
            let command = registry
                .commands()
                .iter()
                .find(|command| command.dollar_aliases.contains(&alias))
                .unwrap_or_else(|| panic!("missing dollar workflow alias ${alias}"));
            assert!(
                !matches!(
                    command.action,
                    CommandAction::PromptTemplate { .. }
                        | CommandAction::LoadSkills { .. }
                        | CommandAction::PlanArtifact { .. }
                        | CommandAction::HandoffArtifact { .. }
                ),
                "${alias} must not dispatch through a prompt/artifact placeholder"
            );
            if command.enabled_by_default {
                assert_eq!(
                    command.availability,
                    WorkflowCommandAvailability::Present,
                    "${alias} enabled command should be present"
                );
                assert!(
                    !matches!(command.action, CommandAction::BlockedWorkflow { .. }),
                    "${alias} must not resolve to a blocked workflow"
                );
            } else {
                assert!(
                    matches!(command.action, CommandAction::BlockedWorkflow { .. }),
                    "${alias} staged command must fail closed"
                );
                assert_eq!(
                    command.effect,
                    CommandEffect::BlockedNoSideEffect,
                    "${alias} staged command must not mutate state"
                );
            }
        }
    }

    #[test]
    fn removed_omx_commands_are_not_registered() {
        let registry = CommandRegistry::builtins();
        for name in [
            "omx-skill:ask",
            "omx-skill:ask-claude",
            "omx-skill:ask-gemini",
            "omx-skill:build-fix",
            "omx-skill:omx-setup",
            "ask",
            "ask-claude",
            "ask-gemini",
            "build-fix",
            "omx-setup",
        ] {
            assert!(
                registry.get(name).is_none(),
                "{name} should not resolve after command removal"
            );
        }
    }
}
